package io.transclip.app.share

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.widget.Toast
import io.transclip.app.AppState
import io.transclip.app.R
import io.transclip.engine.PeerState
import io.transclip.proto.DeviceId
import io.transclip.app.TransClipApp
import io.transclip.engine.Clip
import io.transclip.engine.IMAGE_MIMES
import io.transclip.proto.MAX_IMAGE
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.File
import java.util.UUID

/**
 * Any app can fire a SEND intent at the exported share screen. Only `content:` URIs are read: a
 * `file:` URI would let another app make us open our own private files (settings, secrets) and
 * forward them to the paired devices.
 */
fun isSafeShareUri(uri: Uri): Boolean = uri.scheme == "content"

/** A shared picture as a clipboard image, or null when it is not a supported image or is too large. */
fun readImageClip(context: Context, uri: Uri, declaredType: String?): Clip.Image? = runCatching {
    if (!isSafeShareUri(uri)) return null
    val mime = (context.contentResolver.getType(uri) ?: declaredType)?.lowercase()
    if (mime == null || mime !in IMAGE_MIMES) return null
    val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
    if (bytes.size > MAX_IMAGE) return null
    val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
    android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    Clip.Image(mime, bounds.outWidth.coerceAtLeast(0), bounds.outHeight.coerceAtLeast(0), bytes)
}.getOrNull()

/** Copy a content URI into the cache so the engine can read it as a plain file. Null on failure. */
fun copyToCache(context: Context, uri: Uri): File? = runCatching {
    if (!isSafeShareUri(uri)) return null
    var name = uri.lastPathSegment?.substringAfterLast('/') ?: "file"
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
        if (it.moveToFirst()) name = it.getString(0) ?: name
    }
    // Keep the original name (receivers show it) but isolate each share in its own folder.
    val dir = File(context.cacheDir, "outgoing/${UUID.randomUUID()}").apply { mkdirs() }
    val safe = name.replace(Regex("[\\\\/:*?\"<>|\\p{Cntrl}]"), "_").trim().trimEnd('.', ' ').ifEmpty { "file" }
    val target = File(dir, safe)
    context.contentResolver.openInputStream(uri)?.use { input -> target.outputStream().use { input.copyTo(it) } }
        ?: return null
    target
}.getOrNull()

/**
 * Share Sheet entry (PLAN section 6.6): text becomes a clipboard item, files are sent to every
 * online device, or to the device chosen in a small dialog when several are online.
 */
class ShareReceiverActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val app = application as TransClipApp
        val appContext = applicationContext
        val intent = intent
        val uris: List<Uri> = when (intent.action) {
            Intent.ACTION_SEND -> listOfNotNull(intent.getParcelableExtra(Intent.EXTRA_STREAM))
            Intent.ACTION_SEND_MULTIPLE -> intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM).orEmpty()
            else -> emptyList()
        }
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
        val online = AppState.peers.value.filter { it.state == PeerState.ONLINE }
        // Files go to one chosen device or to all; with a single device there is nothing to choose.
        val isFiles = uris.isNotEmpty() && (uris.size > 1 || readImageIsNull(appContext, uris[0], intent.type))
        if (isFiles && online.size > 1) {
            val names = arrayOf(getString(R.string.share_all_devices)) +
                online.map { it.name.ifEmpty { it.id.toHex().take(8) } }
            android.app.AlertDialog.Builder(this)
                .setTitle(R.string.share_pick_title)
                .setItems(names) { _, which ->
                    val target = if (which == 0) emptyList() else listOf(online[which - 1].id)
                    dispatch(app, appContext, uris, text, intent.type, target)
                    finish()
                }
                .setOnCancelListener { finish() }
                .show()
            return
        }
        dispatch(app, appContext, uris, text, intent.type, emptyList())
        finish()
    }

    private fun readImageIsNull(context: Context, uri: Uri, type: String?): Boolean {
        val mime = (context.contentResolver.getType(uri) ?: type)?.lowercase()
        return mime == null || mime !in IMAGE_MIMES
    }

    private fun dispatch(app: TransClipApp, appContext: Context, uris: List<Uri>, text: String?, type: String?, to: List<DeviceId>) {
        CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
            val image = uris.singleOrNull()?.takeIf { app.engine.allows(isText = false) }?.let { readImageClip(appContext, it, type) }
            val sentTo = when {
                // One picture shared from the gallery or a screenshot: put it on the other devices' clipboard.
                image != null -> if (app.engine.submit(image) != null) app.engine.onlineCount() else 0
                uris.isNotEmpty() -> app.engine.sendFiles(uris.mapNotNull { copyToCache(appContext, it) }, to)
                !text.isNullOrEmpty() -> if (app.engine.submit(Clip.Text(text)) != null) app.engine.onlineCount() else 0
                else -> 0
            }
            launch(Dispatchers.Main) {
                val msg = if (sentTo > 0) appContext.getString(R.string.share_sent, sentTo) else appContext.getString(R.string.share_no_device)
                Toast.makeText(appContext, msg, Toast.LENGTH_SHORT).show()
            }
        }
    }
}
