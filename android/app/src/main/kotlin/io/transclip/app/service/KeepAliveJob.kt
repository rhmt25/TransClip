package io.transclip.app.service

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.util.Log
import io.transclip.app.TransClipApp
import io.transclip.app.clip.ClipAccessibilityService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val TAG = "TransClip"
private const val JOB_ID = 4782

/**
 * Last line of defence when the system or a memory cleaner kills the app and nothing restarts
 * it: a persisted periodic job (it survives reboots) brings the sync service back and re-binds the
 * accessibility service.
 */
class KeepAliveJob : JobService() {
    override fun onStartJob(params: JobParameters?): Boolean {
        CoroutineScope(SupervisorJob() + Dispatchers.Main).launch {
            try {
                revive(applicationContext, "job")
            } finally {
                jobFinished(params, false)
            }
        }
        return true
    }

    override fun onStopJob(params: JobParameters?): Boolean = false

    companion object {
        fun schedule(context: Context) {
            val js = context.getSystemService(JobScheduler::class.java) ?: return
            if (js.getPendingJob(JOB_ID) != null) return
            val job = JobInfo.Builder(JOB_ID, ComponentName(context, KeepAliveJob::class.java))
                .setPeriodic(15 * 60 * 1000L)
                .setPersisted(true)
                .build()
            runCatching { js.schedule(job) }.onFailure { Log.w(TAG, "keep-alive job not scheduled: ${it.javaClass.simpleName}") }
        }

        /** Start the sync service when the user wants it running, and re-bind auto-capture. */
        suspend fun revive(context: Context, why: String) {
            val app = context.applicationContext as TransClipApp
            val wanted = runCatching { app.settings.get().autoStart && app.secrets.groupKey() != null }.getOrDefault(false)
            if (wanted && !app.engine.isRunning) {
                Log.i(TAG, "reviving sync service ($why)")
                runCatching { SyncService.start(context) }.onFailure { Log.w(TAG, "cannot start sync service: ${it.javaClass.simpleName}") }
            }
            ClipAccessibilityService.rebindIfDropped(context)
        }
    }
}
