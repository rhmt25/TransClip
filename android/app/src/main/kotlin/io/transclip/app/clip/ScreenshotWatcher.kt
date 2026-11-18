package io.transclip.app.clip

import android.content.Context
import android.database.ContentObserver
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.util.Log
import io.transclip.engine.Clip
import io.transclip.engine.IMAGE_MIMES
import io.transclip.proto.MAX_IMAGE
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

private const val TAG = "TransClip"

/**
 * Sends new screenshots to the other devices as clipboard images. Many Android versions never put
 * a screenshot on the system clipboard, so there is nothing for a clipboard listener to see; the
 * media store is the only place the picture appears. Opt-in, needs the photo permission.
 */
class ScreenshotWatcher(
    private val context: Context,
    private val scope: CoroutineScope,
    private val send: suspend (Clip.Image) -> Unit,
) {
    private var lastId = -1L
    private val observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean, uri: Uri?) {
            scope.launch(Dispatchers.IO) { check() }
        }
    }

    fun start() {
        lastId = newest()?.first ?: -1L
        context.contentResolver.registerContentObserver(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, true, observer)
        Log.i(TAG, "screenshot watcher started")
    }

    fun stop() {
        context.contentResolver.unregisterContentObserver(observer)
    }

    private fun newest(): Triple<Long, String, String>? = runCatching {
        // Android 11+ rejects "LIMIT" inside the sort order; the limit goes in the query arguments.
        val args = android.os.Bundle().apply {
            putString(android.content.ContentResolver.QUERY_ARG_SQL_SELECTION, "${MediaStore.Images.Media.IS_PENDING} = 0")
            putStringArray(android.content.ContentResolver.QUERY_ARG_SORT_COLUMNS, arrayOf(MediaStore.Images.Media._ID))
            putInt(
                android.content.ContentResolver.QUERY_ARG_SORT_DIRECTION,
                android.content.ContentResolver.QUERY_SORT_DIRECTION_DESCENDING,
            )
            putInt(android.content.ContentResolver.QUERY_ARG_LIMIT, 1)
        }
        context.contentResolver.query(
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            arrayOf(MediaStore.Images.Media._ID, MediaStore.Images.Media.MIME_TYPE, MediaStore.Images.Media.RELATIVE_PATH),
            args,
            null,
        )?.use { c ->
            if (c.moveToFirst()) Triple(c.getLong(0), c.getString(1).orEmpty(), c.getString(2).orEmpty()) else null
        }
    }.onFailure { Log.w(TAG, "media query failed: ${it.javaClass.simpleName} ${it.message}") }.getOrNull()

    private suspend fun check() {
        val (id, mime, path) = newest() ?: return
        if (id <= lastId) return
        lastId = id
        // Only screenshots; our own received pictures and ordinary photos are left alone.
        if (!path.contains("screenshot", ignoreCase = true) || path.contains("TransClip")) return
        if (mime !in IMAGE_MIMES) return
        kotlinx.coroutines.delay(400) // let the system finish writing the file
        val uri = Uri.withAppendedPath(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, id.toString())
        val bytes = runCatching { context.contentResolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() ?: return
        if (bytes.size > MAX_IMAGE) {
            Log.d(TAG, "screenshot too large: ${bytes.size}")
            return
        }
        val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
        android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        Log.d(TAG, "screenshot detected: ${bytes.size} bytes")
        send(Clip.Image(mime, bounds.outWidth.coerceAtLeast(0), bounds.outHeight.coerceAtLeast(0), bytes))
    }
}
