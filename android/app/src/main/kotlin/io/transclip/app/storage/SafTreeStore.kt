package io.transclip.app.storage

import android.content.Context
import android.net.Uri
import android.os.StatFs
import androidx.documentfile.provider.DocumentFile
import io.transclip.engine.IncomingStore
import io.transclip.engine.PART_SUFFIX
import io.transclip.engine.PartWriter
import io.transclip.proto.PathRules
import java.io.File
import java.io.IOException
import java.security.MessageDigest

/** Receives into a folder the user picked with the Storage Access Framework (PLAN section 4.5). */
class SafTreeStore(private val context: Context, treeUri: Uri) : IncomingStore {
    private val root: DocumentFile = DocumentFile.fromTreeUri(context, treeUri)
        ?: throw IOException("invalid tree uri")
    private val partsDir = File(context.cacheDir, "parts").apply { mkdirs() }

    override fun beginBatch(batchName: String): String {
        if (batchName.isEmpty()) return ""
        val name = PathRules.uniqueName(batchName) { root.findFile(it) != null }
        root.createDirectory(name) ?: throw IOException("cannot create folder")
        return name
    }

    override fun makeDir(root: String, rel: List<String>) {
        dirFor(root, rel)
    }

    override fun openPart(root: String, rel: List<String>): PartWriter {
        val dir = dirFor(root, rel.dropLast(1))
        return SafPart(File(partsDir, partName(root, rel)), dir, rel.last())
    }

    override fun freeBytes(): Long? = runCatching {
        StatFs(context.filesDir.path).availableBytes.takeIf { it > 0 }
    }.getOrNull()

    /** Walk (and create) nested folders below the batch root. */
    private fun dirFor(batch: String, rel: List<String>): DocumentFile {
        var cur = if (batch.isEmpty()) {
            root
        } else {
            root.findFile(batch) ?: root.createDirectory(batch) ?: throw IOException("cannot create folder")
        }
        for (seg in rel) {
            cur = cur.findFile(seg)?.takeIf { it.isDirectory } ?: cur.createDirectory(seg)
                ?: throw IOException("cannot create folder")
        }
        return cur
    }

    private fun partName(root: String, rel: List<String>): String {
        val md = MessageDigest.getInstance("SHA-1").digest("saf/$root/${rel.joinToString("/")}".toByteArray())
        return md.joinToString("") { "%02x".format(it) } + PART_SUFFIX
    }

    private inner class SafPart(part: File, private val dir: DocumentFile, private val name: String) : CachedPart(part) {
        override fun publish(part: File, mtimeSecs: Long): String {
            val finalName = PathRules.uniqueName(name) { dir.findFile(it) != null }
            val doc = dir.createFile("application/octet-stream", finalName)
                ?: throw IOException("cannot create file")
            try {
                context.contentResolver.openOutputStream(doc.uri)?.use { out -> part.inputStream().use { it.copyTo(out) } }
                    ?: throw IOException("cannot open file")
            } catch (e: Exception) {
                doc.delete()
                throw e
            }
            return finalName
        }
    }
}
