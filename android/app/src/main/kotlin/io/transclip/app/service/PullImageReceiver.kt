package io.transclip.app.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import io.transclip.app.TransClipApp
import io.transclip.proto.DeviceId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/** Tapping the "image available" notification asks the sender for the full image. */
class PullImageReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val from = intent.getStringExtra("from") ?: return
        val id = intent.getStringExtra("id") ?: return
        val app = context.applicationContext as TransClipApp
        val pending = goAsync()
        CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
            try {
                runCatching { app.engine.pullImage(DeviceId.fromHex(from), id) }
            } finally {
                pending.finish()
            }
        }
    }
}
