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

    private val listener = ClipboardManager.OnPrimaryClipChangedListener {
        val now = System.currentTimeMillis()
        Log.d(TAG, "listener: clip changed (paused=${AppState.paused.value} sinceOwnWrite=${now - lastOwnWriteMs}ms sinceLaunch=${now - lastLaunch}ms)")
        if (AppState.paused.value) return@OnPrimaryClipChangedListener
        if (now - lastOwnWriteMs < 2_000) return@OnPrimaryClipChangedListener
        if (now - lastLaunch < 400) return@OnPrimaryClipChangedListener
        lastLaunch = now
        Log.d(TAG, "clipboard changed, starting capture")
        val intent = Intent(this, ClipCaptureActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
            .putExtra(ClipCaptureActivity.EXTRA_AUTO, true)
        runCatching { startActivity(intent) }.onFailure { Log.w(TAG, "cannot start capture: ${it.javaClass.simpleName}") }
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cm.addPrimaryClipChangedListener(listener)
        Log.i(TAG, "auto-capture service connected")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        if (this::cm.isInitialized) cm.removePrimaryClipChangedListener(listener)
        super.onDestroy()
    }

    companion object {
        /** True when the user has switched the service on in system settings. */
        fun isEnabled(context: Context): Boolean {
            val enabled = Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
                ?: return false
            val me = "${context.packageName}/${ClipAccessibilityService::class.java.name}"
            return enabled.split(':').any { it.equals(me, ignoreCase = true) }
        }
    }
}
