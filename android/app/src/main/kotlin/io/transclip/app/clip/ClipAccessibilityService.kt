package io.transclip.app.clip

import android.accessibilityservice.AccessibilityService
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.provider.Settings
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import io.transclip.app.AppState

private const val TAG = "TransClip"

/**
 * Optional auto-capture (PLAN section 6.4, path 2). Android 10+ lets only the focused app read the
 * clipboard. This service reads no screen content; it only listens for clipboard changes and, being
 * an accessibility service, may bring up [ClipCaptureActivity] from the background so the change
 * is read while that screen has focus.
 */
class ClipAccessibilityService : AccessibilityService() {
    private lateinit var cm: ClipboardManager
    private var lastLaunch = 0L

    private val handler = android.os.Handler(android.os.Looper.getMainLooper())

    private val listener = ClipboardManager.OnPrimaryClipChangedListener {
        Log.d(TAG, "listener: clip changed")
        requestCapture("listener", 0)
    }

    /** Bring up the capture screen, unless we wrote the clipboard ourselves or just did so. */
    private fun requestCapture(why: String, delayMs: Long) {
        handler.postDelayed({
            val now = System.currentTimeMillis()
            if (AppState.paused.value) return@postDelayed
            if (now - lastOwnWriteMs < 2_000) return@postDelayed
            if (now - lastLaunch < 400) return@postDelayed
            lastLaunch = now
            Log.d(TAG, "clipboard probably changed ($why), starting capture")
            val intent = Intent(this, ClipCaptureActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
                .putExtra(ClipCaptureActivity.EXTRA_AUTO, true)
            runCatching { startActivity(intent) }.onFailure { Log.w(TAG, "cannot start capture: ${it.javaClass.simpleName}") }
        }, delayMs)
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        connected = true
        cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cm.addPrimaryClipChangedListener(listener)
        Log.i(TAG, "auto-capture service connected")
    }

    /**
     * Android 10+ does not tell background apps about clipboard changes (the listener above is
     * refused with "not in focus"), so a copy is recognised from its side effects instead: a tap
     * on a "Copy"/"Salin" button or a "Copied" toast. Only the short label of that button or toast
     * is looked at, never the screen content.
     */
    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        event ?: return
        if (event.packageName == packageName) return
        val label = buildString {
            event.text.forEach { append(it).append(' ') }
            event.contentDescription?.let { append(it) }
        }.trim()
        if (label.isEmpty() || label.length > MAX_LABEL) return
        when (event.eventType) {
            AccessibilityEvent.TYPE_VIEW_CLICKED ->
                if (looksLikeCopy(label)) requestCapture("copy tapped", 250)
            AccessibilityEvent.TYPE_NOTIFICATION_STATE_CHANGED ->
                // Toasts carry no Notification; real notifications are ignored.
                if (event.parcelableData !is android.app.Notification && looksLikeCopied(label)) requestCapture("copied toast", 100)
        }
    }

    override fun onInterrupt() = Unit

    override fun onUnbind(intent: Intent?): Boolean {
        connected = false
        return super.onUnbind(intent)
    }

    override fun onDestroy() {
        connected = false
        if (this::cm.isInitialized) cm.removePrimaryClipChangedListener(listener)
        super.onDestroy()
    }

    companion object {
        private const val MAX_LABEL = 60
        private val COPY_WORDS = listOf("copy", "salin", "salinan")
        private val COPIED_WORDS = listOf("copied", "disalin", "tersalin", "clipboard", "papan klip")

        /** A button label such as "Copy", "Salin", "Copy link", "Salin teks". */
        internal fun looksLikeCopy(label: String): Boolean {
            val l = label.lowercase()
            return COPY_WORDS.any { w -> Regex("(^|\\W)$w(\\W|$)").containsMatchIn(l) }
        }

        /** A toast such as "Copied to clipboard" or "Disalin ke papan klip". */
        internal fun looksLikeCopied(label: String): Boolean {
            val l = label.lowercase()
            return COPIED_WORDS.any { it in l }
        }

        /** True while the system has this service bound; false after the process was killed. */
        @Volatile
        var connected = false
            private set

        /**
         * The user switched the service on but the system dropped the binding (typical on HyperOS
         * after the memory cleaner kills the app), which Settings shows as "this service is not
         * working". Switching the entry off and on again makes Android bind it afresh. Needs
         * WRITE_SECURE_SETTINGS, granted once with
         * `adb shell pm grant io.transclip.app android.permission.WRITE_SECURE_SETTINGS`.
         */
        fun rebindIfDropped(context: Context): Boolean {
            if (connected || !isEnabled(context)) return false
            if (context.checkSelfPermission(android.Manifest.permission.WRITE_SECURE_SETTINGS) !=
                android.content.pm.PackageManager.PERMISSION_GRANTED
            ) {
                return false
            }
            val me = "${context.packageName}/${ClipAccessibilityService::class.java.name}"
            return runCatching {
                val cr = context.contentResolver
                val current = Settings.Secure.getString(cr, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES).orEmpty()
                val others = current.split(':').filter { it.isNotEmpty() && !it.equals(me, ignoreCase = true) }
                Settings.Secure.putString(cr, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES, others.joinToString(":"))
                Settings.Secure.putString(cr, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES, (others + me).joinToString(":"))
                Log.i(TAG, "accessibility service was dropped by the system; asked Android to bind it again")
                true
            }.getOrElse {
                Log.w(TAG, "cannot rebind accessibility service: ${it.javaClass.simpleName}")
                false
            }
        }

        /** True when the user has switched the service on in system settings. */
        fun isEnabled(context: Context): Boolean {
            val enabled = Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
                ?: return false
            val me = "${context.packageName}/${ClipAccessibilityService::class.java.name}"
            return enabled.split(':').any { it.equals(me, ignoreCase = true) }
        }
    }
}
