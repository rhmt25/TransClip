package io.transclip.app.debug

import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.provider.MediaStore
import android.util.Base64

/** `adb shell am broadcast -n io.transclip.app/.debug.DebugClipReceiver --es text hello` or `--es png <base64>`. */
class DebugClipReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        android.util.Log.d("TransClip", "debug clip receiver: text=${intent.hasExtra("text")} png=${intent.hasExtra("png")}")
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val text = intent.getStringExtra("text")
        val png = intent.getStringExtra("png")
        when {
            text != null -> {
                cm.setPrimaryClip(ClipData.newPlainText("debug", text))
                android.util.Log.d("TransClip", "debug clip set")
            }
            png != null -> {
                val bytes = Base64.decode(png, Base64.DEFAULT)
                val values = ContentValues().apply {
                    put(MediaStore.Images.Media.DISPLAY_NAME, "debug-${System.nanoTime()}.png")
                    put(MediaStore.Images.Media.MIME_TYPE, "image/png")
                    put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/TransClipDebug")
                }
                val uri = context.contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values) ?: return
                context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                cm.setPrimaryClip(ClipData.newUri(context.contentResolver, "debug image", uri))
            }
        }
    }
}
