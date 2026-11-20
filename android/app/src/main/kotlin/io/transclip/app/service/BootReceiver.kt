package io.transclip.app.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import io.transclip.app.TransClipApp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Starts the sync service after boot and after the app was updated, when the user enabled it and
 * a group exists, so nothing has to be opened by hand.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val app = context.applicationContext as TransClipApp
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                if (app.settings.get().autoStart && app.secrets.groupKey() != null) {
                    SyncService.start(context)
                }
                KeepAliveJob.schedule(context)
            } finally {
                pending.finish()
            }
        }
    }
}
