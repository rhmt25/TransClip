package io.transclip.app.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import io.transclip.app.AppState
import io.transclip.app.MainActivity
import io.transclip.app.R
import io.transclip.app.TransClipApp
import io.transclip.app.clip.ClipAccessibilityService
import io.transclip.app.clip.ClipCaptureActivity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.launch

private const val TAG = "TransClip"
private const val CHANNEL_ID = "sync"
private const val NOTIFICATION_ID = 1

/**
 * Foreground service that keeps the engine alive (PLAN section 6.5): ongoing notification with
 * the actions *Kirim clipboard*, *Jeda* and *Putuskan*.
 */
class SyncService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val app get() = application as TransClipApp
    private var callback: ConnectivityManager.NetworkCallback? = null

    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, getString(R.string.channel_sync), NotificationManager.IMPORTANCE_LOW),
        )
        startForegroundCompat(buildNotification())
        AppState.peers.onEach { refreshNotification() }.launchIn(scope)
        AppState.paused.onEach { refreshNotification() }.launchIn(scope)
        AppState.paired.onEach { refreshNotification() }.launchIn(scope)
        registerNetworkCallback()
        BackgroundReadiness.check(this)
        // Keeps the accessibility service bound: HyperOS and similar can drop it silently.
        scope.launch {
            while (true) {
                ClipAccessibilityService.rebindIfDropped(this@SyncService)
                delay(15_000)
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Re-checked whenever the app is opened, so a fixed setting clears its notification.
        BackgroundReadiness.check(this)
        when (intent?.action) {
            ACTION_PAUSE -> scope.launch {
                val paused = !AppState.paused.value
                app.settings.update { it.copy(paused = paused) }
                AppState.setPaused(paused)
            }
            ACTION_STOP -> {
                scope.launch {
                    app.engine.stop()
                    stopSelf()
                }
                return START_NOT_STICKY
            }
            ACTION_RESTART -> scope.launch {
                app.engine.stop()
                app.engine.start()
            }
            else -> scope.launch { app.engine.start() }
        }
        return START_STICKY
    }

    private fun registerNetworkCallback() {
        val cm = getSystemService(ConnectivityManager::class.java)
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                Log.d(TAG, "network available, announcing")
                app.engine.onNetworkChanged()
            }

            override fun onCapabilitiesChanged(network: Network, caps: android.net.NetworkCapabilities) {
                // Validated after joining: peers on this network can be dialed now.
                if (caps.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_VALIDATED)) app.engine.onNetworkChanged()
            }
        }
        callback = cb
        runCatching { cm.registerDefaultNetworkCallback(cb) }
    }

    private fun refreshNotification() {
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    }

    private fun buildNotification(): Notification {
        val online = AppState.peers.value.count { it.state == io.transclip.engine.PeerState.ONLINE }
        val title = when {
            !AppState.paired.value -> getString(R.string.status_not_paired)
            AppState.paused.value -> getString(R.string.status_paused)
            else -> resources.getQuantityString(R.plurals.status_online, online, online)
        }
        fun action(code: Int, label: Int, intent: Intent) = NotificationCompat.Action.Builder(
            0,
            getString(label),
            PendingIntent.getActivity(
                this, code, intent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            ),
        ).build()

        fun serviceAction(code: Int, label: Int, action: String) = NotificationCompat.Action.Builder(
            0,
            getString(label),
            PendingIntent.getService(
                this, code, Intent(this, SyncService::class.java).setAction(action),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            ),
        ).build()

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_transclip)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(title)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(
                PendingIntent.getActivity(
                    this, 0, Intent(this, MainActivity::class.java),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
            .addAction(action(1, R.string.action_send_clip, Intent(this, ClipCaptureActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)))
            .addAction(serviceAction(2, R.string.action_pause, ACTION_PAUSE))
            .addAction(serviceAction(3, R.string.action_disconnect, ACTION_STOP))
            .build()
    }

    private fun startForegroundCompat(n: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIFICATION_ID, n)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        callback?.let { runCatching { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) } }
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        const val ACTION_PAUSE = "io.transclip.action.PAUSE"
        const val ACTION_STOP = "io.transclip.action.STOP"
        const val ACTION_RESTART = "io.transclip.action.RESTART"

        fun start(context: Context, action: String? = null) {
            val i = Intent(context, SyncService::class.java).setAction(action)
            ContextCompat.startForegroundService(context, i)
        }
    }
}
