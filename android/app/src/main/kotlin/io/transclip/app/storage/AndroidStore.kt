package io.transclip.app.storage

import android.content.ContentValues
import android.content.Context
import android.os.Environment
import android.os.StatFs
import android.provider.MediaStore
import io.transclip.engine.IncomingStore
import io.transclip.engine.PART_SUFFIX
import io.transclip.engine.PartWriter
import io.transclip.proto.PathRules
import java.security.MessageDigest
import java.io.File
import java.io.IOException

/** Default folder shown to the user: `Download/TransClip/`. */
const val DEFAULT_RELATIVE_ROOT = "Download/TransClip"

/**
 * Receives into `Download/TransClip/` through MediaStore (no permission needed on API 29+).
 * Bytes are written to a part file in the app cache first, so an interrupted transfer can resume;
 * `finish` copies the verified file into MediaStore.
 */
class AndroidStore(private val context: Context) : IncomingStore {
    private val partsDir = File(context.cacheDir, "parts").apply { mkdirs() }

    override fun beginBatch(batchName: String): String {
        if (batchName.isEmpty()) return ""
        return PathRules.uniqueName(batchName) { folderExists(it) }
    }

    /** MediaStore has no empty folders, so directory entries need no work on this store. */
    override fun makeDir(root: String, rel: List<String>) {}

    override fun openPart(root: String, rel: List<String>): PartWriter {
        val relDir = (listOf(root).filter { it.isNotEmpty() } + rel.dropLast(1)).joinToString("/")
        return MediaPart(File(partsDir, partName(root, rel)), relDir, rel.last())
    }

    override fun freeBytes(): Long? = runCatching {
        StatFs(Environment.getExternalStorageDirectory().path).availableBytes
    }.getOrNull()

    private fun folderExists(name: String): Boolean {
        val uri = MediaStore.Downloads.EXTERNAL_CONTENT_URI
        context.contentResolver.query(
            uri,
            arrayOf(MediaStore.Downloads._ID),
            "${MediaStore.Downloads.RELATIVE_PATH} LIKE ?",
            arrayOf("$DEFAULT_RELATIVE_ROOT/$name/%"),
            null,
        )?.use { return it.moveToFirst() }
        return false
    }

    private fun partName(root: String, rel: List<String>): String {
        val md = MessageDigest.getInstance("SHA-1").digest("$root/${rel.joinToString("/")}".toByteArray())
        return md.joinToString("") { "%02x".format(it) } + PART_SUFFIX
    }

    private inner class MediaPart(part: File, private val relDir: String, private val name: String) : CachedPart(part) {
        override fun publish(part: File, mtimeSecs: Long): String {
            val relativePath = if (relDir.isEmpty()) DEFAULT_RELATIVE_ROOT else DEFAULT_RELATIVE_ROOT + "/" + relDir
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.RELATIVE_PATH, relativePath)
                put(MediaStore.Downloads.IS_PENDING, 1)
                if (mtimeSecs > 0) put(MediaStore.Downloads.DATE_MODIFIED, mtimeSecs)
            }
            val resolver = context.contentResolver
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: throw IOException("cannot create download entry")
            try {
                resolver.openOutputStream(uri)?.use { out -> part.inputStream().use { it.copyTo(out) } }
                    ?: throw IOException("cannot open download entry")
                resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            } catch (e: Exception) {
                resolver.delete(uri, null, null)
                throw e
            }
            return name
        }
    }
}
