package io.transclip.app.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import io.transclip.app.TransClipApp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** Starts the sync service after boot when the user enabled it and a group exists. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val app = context.applicationContext as TransClipApp
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                if (app.settings.get().autoStart && app.secrets.groupKey() != null) {
                    SyncService.start(context)
                }
            } finally {
                pending.finish()
            }
        }
    }
}
