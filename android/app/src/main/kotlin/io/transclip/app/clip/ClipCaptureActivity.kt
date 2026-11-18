package io.transclip.app.clip

import android.app.Activity
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import io.transclip.app.AppState
import io.transclip.app.R
import io.transclip.app.TransClipApp
import io.transclip.engine.Clip
import io.transclip.engine.IMAGE_MIMES
import io.transclip.engine.PeerState
import io.transclip.engine.Submitted
import io.transclip.proto.MAX_IMAGE
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val TAG = "TransClip"

/**
 * Transparent activity that reads the clipboard while it has focus (Android 10+ allows clipboard
 * reads only for the focused app) and sends it to the other devices (PLAN section 6.4, path 1).
 */
class ClipCaptureActivity : Activity() {
    private var handled = false
    private val auto get() = intent.getBooleanExtra(EXTRA_AUTO, false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || handled) return
        handled = true
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val data = cm.primaryClip
        Log.d(TAG, "capture: items=${data?.itemCount} mimes=${(0 until (data?.description?.mimeTypeCount ?: 0)).map { data?.description?.getMimeType(it) }} uri=${data?.getItemAt(0)?.uri != null} auto=$auto")
        val outcome = describe(data)
        if (outcome != null) {
            // Copy what we need now; the read must finish before the activity closes.
            val app = application as TransClipApp
            val appContext = applicationContext
            CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
                val clip = outcome(appContext)
                val message = if (clip == null) {
                    R.string.capture_nothing
                } else {
                    when (app.engine.submit(clip)) {
                        is Submitted.Sent -> R.string.capture_sent
                        Submitted.Unchanged -> R.string.capture_unchanged
                        null -> R.string.capture_not_running
                    }
                }
                if (auto) return@launch
                launch(Dispatchers.Main) {
                    val online = AppState.peers.value.count { it.state == PeerState.ONLINE }
                    Toast.makeText(
                        appContext,
                        if (message == R.string.capture_sent) appContext.getString(message, online) else appContext.getString(message),
                        Toast.LENGTH_SHORT,
                    ).show()
                }
            }
        } else {
            if (!auto) Toast.makeText(this, R.string.capture_nothing, Toast.LENGTH_SHORT).show()
        }
        finish()
        if (auto) overridePendingTransition(0, 0)
    }

    /** Decide what to send. Returns a reader to run off the main thread, or null to skip. */
    private fun describe(data: ClipData?): ((Context) -> Clip?)? {
        if (data == null || data.itemCount == 0) return null
        val description = data.description
        if (description.isTransClipOrigin()) return null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            description.extras?.getBoolean(ClipDescription.EXTRA_IS_SENSITIVE, false) == true
        ) {
            Log.d(TAG, "clipboard marked sensitive, not sent")
            return null
        }
        val item = data.getItemAt(0)
        val uri = item.uri
        val mime = (0 until description.mimeTypeCount).map { description.getMimeType(it) }.firstOrNull { it in IMAGE_MIMES }
        if (uri != null && mime != null) {
            return { ctx ->
                runCatching {
                    ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                }.onFailure { Log.w(TAG, "cannot read image: ${it.javaClass.simpleName}") }.getOrNull()
                    ?.also { Log.d(TAG, "image bytes=${it.size} max=$MAX_IMAGE") }
                    ?.takeIf { it.size <= MAX_IMAGE }?.let { bytes ->
                    // Width and height are informational; the receiver decodes the real image.
                    val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                    Clip.Image(mime, bounds.outWidth.coerceAtLeast(0), bounds.outHeight.coerceAtLeast(0), bytes)
                }
            }
        }
        val text = item.text?.toString()
        if (text.isNullOrEmpty()) return null
        return { Clip.Text(text) }
    }

    companion object {
        /** Set by the auto-capture service so the screen stays silent. */
        const val EXTRA_AUTO = "io.transclip.extra.AUTO"
    }
}
