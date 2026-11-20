package io.transclip.app

import android.app.Application
import io.transclip.app.service.EngineHost
import io.transclip.app.service.KeepAliveJob
import io.transclip.app.storage.SecretStore
import io.transclip.app.storage.SettingsRepo

/** Application object: holds the singletons shared by the service, activities and tiles. */
class TransClipApp : Application() {
    val secrets by lazy { SecretStore(this) }
    val settings by lazy { SettingsRepo(this) }
    val engine by lazy { EngineHost(this) }

    override fun onCreate() {
        super.onCreate()
        KeepAliveJob.schedule(this)
    }
}
