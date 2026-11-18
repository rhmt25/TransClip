package io.transclip.app.clip

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.PersistableBundle
import android.provider.MediaStore
import io.transclip.engine.Clip
import io.transclip.engine.ClipboardSink
import io.transclip.engine.RemoteClip
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking

/** Marker stored in the clip description so our own writes are not captured again. */
const val ORIGIN_KEY = "io.transclip.origin"

/** When we last wrote the clipboard ourselves; the auto-capture service ignores changes right after. */
@Volatile
var lastOwnWriteMs: Long = 0

/** True when the clip was written by TransClip. */
fun ClipDescription?.isTransClipOrigin(): Boolean = this?.extras?.containsKey(ORIGIN_KEY) == true

/** Writes items received from peers to the system clipboard (PLAN section 6.4). */
class ClipboardWriter(
    private val context: Context,
    private val paused: () -> Boolean,
) : ClipboardSink {
    override fun apply(clip: RemoteClip) {
        if (paused()) error("paused")
        val data = when (val c = clip.clip) {
            is Clip.Text -> ClipData.newPlainText("TransClip", c.text)
            is Clip.Image -> {
                val uri = saveImage(c, clip.meta.id)
                ClipData.newUri(context.contentResolver, "TransClip image", uri)
            }
        }
        data.description.extras = PersistableBundle().apply { putString(ORIGIN_KEY, clip.meta.id) }
        // The clipboard must be touched from the main thread; apply() runs on an I/O thread.
        runBlocking(Dispatchers.Main) {
            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            lastOwnWriteMs = System.currentTimeMillis()
            cm.setPrimaryClip(data)
        }
    }

    private fun saveImage(image: Clip.Image, id: String): android.net.Uri {
        val ext = when (image.mime) {
            "image/jpeg" -> "jpg"
            "image/webp" -> "webp"
            "image/gif" -> "gif"
            "image/bmp" -> "bmp"
            else -> "png"
        }
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, "TransClip-${id.take(8)}.$ext")
            put(MediaStore.Images.Media.MIME_TYPE, image.mime)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                put(MediaStore.Images.Media.RELATIVE_PATH, "${Environment.DIRECTORY_PICTURES}/TransClip")
                put(MediaStore.Images.Media.IS_PENDING, 1)
            }
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
            ?: error("cannot create image entry")
        try {
            resolver.openOutputStream(uri)?.use { it.write(image.bytes) } ?: error("cannot open image entry")
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
            }
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }
        return uri
    }
}
