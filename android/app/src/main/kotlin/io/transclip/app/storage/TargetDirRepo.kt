package io.transclip.app.storage

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import io.transclip.engine.IncomingStore

/** The store in effect and whether a chosen folder had to be replaced by the default. */
data class ResolvedStore(val store: IncomingStore, val fellBack: Boolean)

/** Keeps the receive-folder choice and its persisted SAF permission in step (PLAN section 4.5). */
class TargetDirRepo(private val context: Context, private val settings: SettingsRepo) {
    private val resolver get() = context.contentResolver

    /** Resolve the store for the current setting; a lost permission or folder falls back to the default. */
    suspend fun resolve(): ResolvedStore {
        val uriStr = settings.get().downloadTreeUri
        if (uriStr.isEmpty()) return ResolvedStore(AndroidStore(context), false)
        val uri = Uri.parse(uriStr)
        val granted = resolver.persistedUriPermissions.any { it.uri == uri && it.isReadPermission && it.isWritePermission }
        val usable = granted && DocumentFile.fromTreeUri(context, uri)?.let { it.exists() && it.canWrite() } == true
        val saf = if (usable) runCatching { SafTreeStore(context, uri) }.getOrNull() else null
        return if (saf != null) ResolvedStore(saf, false) else ResolvedStore(AndroidStore(context), true)
    }

    /** Remember a folder picked with `OpenDocumentTree`, releasing the permission of the previous one. */
    suspend fun choose(uri: Uri) {
        resolver.takePersistableUriPermission(uri, FLAGS)
        val old = settings.get().downloadTreeUri
        if (old.isNotEmpty() && old != uri.toString()) release(Uri.parse(old))
        settings.update { it.copy(downloadTreeUri = uri.toString()) }
    }

    /** Back to `Download/TransClip`. */
    suspend fun reset() {
        val old = settings.get().downloadTreeUri
        if (old.isNotEmpty()) release(Uri.parse(old))
        settings.update { it.copy(downloadTreeUri = "") }
    }

    /** Human-readable description of the current target. */
    suspend fun label(): String {
        val s = settings.get().downloadTreeUri
        if (s.isEmpty()) return DEFAULT_RELATIVE_ROOT
        return DocumentFile.fromTreeUri(context, Uri.parse(s))?.name ?: s
    }

    private fun release(uri: Uri) {
        runCatching { resolver.releasePersistableUriPermission(uri, FLAGS) }
    }

    private companion object {
        const val FLAGS = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
    }
}
