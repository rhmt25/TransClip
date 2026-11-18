package io.transclip.app.service

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import androidx.core.app.NotificationCompat
import io.transclip.app.R

/** Result notifications for file transfers. Never shows file names. */
object TransferNotifier {
    private const val CHANNEL = "transfers"
    private var counter = 100

    fun batchFinished(context: Context, ok: Int, failed: Int, incoming: Boolean) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_transfers), NotificationManager.IMPORTANCE_DEFAULT),
        )
        val text = context.getString(if (incoming) R.string.transfer_received else R.string.transfer_sent, ok, failed)
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_transclip)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(text)
            .setAutoCancel(true)
            .build()
        runCatching { nm.notify(counter++, n) }
    }

    fun targetDirFallback(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_transfers), NotificationManager.IMPORTANCE_DEFAULT),
        )
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_transclip)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.getString(R.string.target_dir_fallback))
            .setAutoCancel(true)
            .build()
        runCatching { nm.notify(counter++, n) }
    }

    /** "Image available - tap to fetch": the tap pulls it over the slow link. */
    fun imageAvailable(context: Context, fromHex: String, id: String, bytes: Long) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_transfers), NotificationManager.IMPORTANCE_DEFAULT),
        )
        val intent = android.content.Intent(context, PullImageReceiver::class.java)
            .putExtra("from", fromHex)
            .putExtra("id", id)
        val pi = android.app.PendingIntent.getBroadcast(
            context,
            id.hashCode(),
            intent,
            android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_transclip)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.getString(R.string.image_available, bytes / 1024))
            .setContentIntent(pi)
            .setAutoCancel(true)
            .build()
        runCatching { nm.notify(counter++, n) }
    }
}
