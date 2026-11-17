package io.transclip.app.storage

import android.content.Context
import android.os.Build
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map

private val Context.settingsStore: DataStore<Preferences> by preferencesDataStore(name = "settings")

/** User settings (PLAN section 6.3). */
data class Settings(
    val deviceName: String,
    val paused: Boolean = false,
    val syncText: Boolean = true,
    val syncImages: Boolean = true,
    val receiveFiles: Boolean = true,
    val autoStart: Boolean = true,
    val autoCapture: Boolean = false,
    /** SAF tree URI of the receive folder; empty means `Download/TransClip`. */
    val downloadTreeUri: String = "",
    val onboardingDone: Boolean = false,
) {
    companion object {
        const val MAX_NAME_BYTES = 64

        /** A name that fits the beacon limit, falling back to the device model. */
        fun sanitizeName(raw: String): String {
            val trimmed = raw.trim().ifEmpty { Build.MODEL ?: "Android" }
            var out = trimmed
            while (out.toByteArray(Charsets.UTF_8).size > MAX_NAME_BYTES) out = out.dropLast(1)
            return out
        }
    }
}

class SettingsRepo(private val store: DataStore<Preferences>) {
    constructor(context: Context) : this(context.applicationContext.settingsStore)

    val flow: Flow<Settings> = store.data.map { p ->
        Settings(
            deviceName = Settings.sanitizeName(p[NAME].orEmpty()),
            paused = p[PAUSED] ?: false,
            syncText = p[SYNC_TEXT] ?: true,
            syncImages = p[SYNC_IMAGES] ?: true,
            receiveFiles = p[RECEIVE_FILES] ?: true,
            autoStart = p[AUTO_START] ?: true,
            autoCapture = p[AUTO_CAPTURE] ?: false,
            downloadTreeUri = p[DOWNLOAD_URI].orEmpty(),
            onboardingDone = p[ONBOARDING] ?: false,
        )
    }

    suspend fun get(): Settings = flow.first()

    suspend fun update(change: (Settings) -> Settings) {
        val next = change(get())
        store.edit { p ->
            p[NAME] = Settings.sanitizeName(next.deviceName)
            p[PAUSED] = next.paused
            p[SYNC_TEXT] = next.syncText
            p[SYNC_IMAGES] = next.syncImages
            p[RECEIVE_FILES] = next.receiveFiles
            p[AUTO_START] = next.autoStart
            p[AUTO_CAPTURE] = next.autoCapture
            p[DOWNLOAD_URI] = next.downloadTreeUri
            p[ONBOARDING] = next.onboardingDone
        }
    }

    private companion object {
        val NAME = stringPreferencesKey("device_name")
        val PAUSED = booleanPreferencesKey("paused")
        val SYNC_TEXT = booleanPreferencesKey("sync_text")
        val SYNC_IMAGES = booleanPreferencesKey("sync_images")
        val RECEIVE_FILES = booleanPreferencesKey("receive_files")
        val AUTO_START = booleanPreferencesKey("auto_start")
        val AUTO_CAPTURE = booleanPreferencesKey("auto_capture")
        val DOWNLOAD_URI = stringPreferencesKey("download_tree_uri")
        val ONBOARDING = booleanPreferencesKey("onboarding_done")
    }
}
