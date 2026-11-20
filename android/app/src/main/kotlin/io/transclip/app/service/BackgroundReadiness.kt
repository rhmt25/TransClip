package io.transclip.app.service

import android.app.AppOpsManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import androidx.core.app.NotificationCompat
import io.transclip.app.R

private const val TAG = "TransClip"
private const val CHANNEL = "setup"
private const val ID_AUTOSTART = 7
private const val ID_BATTERY = 8

/**
 * Checks the two things that decide whether the app survives a reboot or being swiped away, and
 * offers a one-tap fix as a notification when one is missing:
 *  - Xiaomi/POCO/Redmi "Autostart": without it HyperOS refuses every restart of a killed app
 *    (seen as `MIUIOP(10008): ignore; rejectTime=...`). There is no API to switch it on.
 *  - Battery optimisation exemption, asked through the standard system dialog.
 */
object BackgroundReadiness {
    /** MIUI's private app-op for Autostart. */
    private const val OP_MIUI_AUTOSTART = 10008

    private fun isXiaomi(): Boolean =
        Build.MANUFACTURER.equals("Xiaomi", ignoreCase = true) ||
            Build.BRAND.lowercase() in setOf("xiaomi", "redmi", "poco")

    /** True/false when known; null when the check is not possible on this system. */
    fun xiaomiAutostartAllowed(context: Context): Boolean? {
        if (!isXiaomi()) return null
        return runCatching {
            val ops = context.getSystemService(AppOpsManager::class.java)
            val m = AppOpsManager::class.java.getMethod("checkOpNoThrow", Int::class.javaPrimitiveType, Int::class.javaPrimitiveType, String::class.java)
            m.invoke(ops, OP_MIUI_AUTOSTART, context.applicationInfo.uid, context.packageName) as Int == AppOpsManager.MODE_ALLOWED
        }.onFailure { Log.d(TAG, "autostart state unknown: ${it.javaClass.simpleName}") }.getOrNull()
    }

    fun batteryUnrestricted(context: Context): Boolean =
        context.getSystemService(PowerManager::class.java)?.isIgnoringBatteryOptimizations(context.packageName) == true

    fun autostartIntent(): Intent = Intent().setComponent(
        ComponentName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"),
    ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    @android.annotation.SuppressLint("BatteryLife")
    fun batteryIntent(context: Context): Intent =
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${context.packageName}"))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    /** Post (or clear) the setup notifications. */
    fun check(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_setup), NotificationManager.IMPORTANCE_DEFAULT),
        )
        val autostart = xiaomiAutostartAllowed(context)
        Log.i(TAG, "background readiness: autostart=$autostart battery=${batteryUnrestricted(context)}")
        if (autostart == false && context.packageManager.resolveActivity(autostartIntent(), 0) != null) {
            post(context, nm, ID_AUTOSTART, R.string.setup_autostart, autostartIntent())
        } else {
            nm.cancel(ID_AUTOSTART)
        }
        if (!batteryUnrestricted(context)) {
            post(context, nm, ID_BATTERY, R.string.setup_battery, batteryIntent(context))
        } else {
            nm.cancel(ID_BATTERY)
        }
    }

    private fun post(context: Context, nm: NotificationManager, id: Int, text: Int, intent: Intent) {
        val pi = PendingIntent.getActivity(context, id, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_transclip)
            .setContentTitle(context.getString(R.string.setup_title))
            .setContentText(context.getString(text))
            .setStyle(NotificationCompat.BigTextStyle().bigText(context.getString(text)))
            .setContentIntent(pi)
            .setAutoCancel(true)
            .build()
        runCatching { nm.notify(id, n) }
    }
}
