package io.transclip.app.service

import android.content.Context
import android.net.wifi.WifiManager
import android.util.Log
import io.transclip.app.AppState
import io.transclip.app.clip.ClipboardWriter
import io.transclip.app.net.NsdDiscovery
import io.transclip.app.storage.SecretStore
import io.transclip.app.storage.SettingsRepo
import io.transclip.engine.BeaconConfig
import io.transclip.engine.BeaconService
import io.transclip.engine.Clip
import io.transclip.engine.ClipEvent
import io.transclip.engine.ClipboardSync
import io.transclip.engine.LocalInfo
import io.transclip.engine.ManagerConfig
import io.transclip.engine.PeerManager
import io.transclip.engine.PeerSeen
import io.transclip.engine.Submitted
import io.transclip.engine.Batch
import io.transclip.engine.TransferEvent
import io.transclip.engine.TransferManager
import io.transclip.engine.batchesFromPaths
import io.transclip.proto.DeviceId
import io.transclip.app.R
import io.transclip.app.storage.TargetDirRepo
import java.io.File
import io.transclip.proto.GroupKey
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.launch
import java.net.InetAddress
import java.net.InetSocketAddress

private const val TAG = "TransClip"
const val TCP_PORT = 47820

/**
 * Owns the engine for the lifetime of the foreground service: discovery, peer manager and
 * clipboard sync. Started and stopped by [SyncService].
 */
class EngineHost(private val context: Context) {
    private val secrets = SecretStore(context)
    private val settings = SettingsRepo(context)
    private var scope: CoroutineScope? = null
    private var mgr: PeerManager? = null
    private var sync: ClipboardSync? = null
    private var transfers: TransferManager? = null
    private var beacon: BeaconService? = null
    private var nsd: NsdDiscovery? = null
    private var lock: WifiManager.MulticastLock? = null
    private var rfcomm: io.transclip.app.net.RfcommServer? = null
    private var screenshots: io.transclip.app.clip.ScreenshotWatcher? = null
    private var allowText = true
    private var allowImages = true

    val isRunning: Boolean get() = mgr != null

    /** Start the engine. Without a group key nothing is started and `paired` stays false. */
    suspend fun start(): Boolean {
        if (isRunning) return true
        val gk: GroupKey? = secrets.groupKey()
        AppState.setPaired(gk != null)
        if (gk == null) return false
        val cfg = settings.get()
        AppState.setPaused(cfg.paused)
        pruneOutgoingCache()
        val me = secrets.deviceId()
        val s = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        scope = s

        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        lock = wifi.createMulticastLock("transclip").apply {
            setReferenceCounted(false)
            acquire()
        }

        val seen = Channel<PeerSeen>(64)
        val manager = PeerManager(
            ManagerConfig(
                local = LocalInfo(me, cfg.deviceName, "android", "transclip/1.0.0", listOf("clip"), "full"),
                gk = gk,
                listenPort = TCP_PORT,
                bt = if (io.transclip.app.net.bluetoothReady(context)) io.transclip.app.net.RfcommDialer(context) else null,
            ),
            s,
            seen,
        )
        manager.start()
        mgr = manager
        if (io.transclip.app.net.bluetoothReady(context)) {
            // Paired devices can reach us over Bluetooth; priority peers are dialed when the LAN is down.
            rfcomm = io.transclip.app.net.RfcommServer(context, manager, s).also { it.start() }
            manager.setBtPriority(cfg.btPriority.mapNotNull { runCatching { DeviceId.fromHex(it) }.getOrNull() }.toSet())
        }

        val gid = gk.groupId()
        beacon = BeaconService(
            BeaconConfig(
                gid = gid,
                deviceId = me,
                tcpPort = manager.localPort,
                platform = Platform.ANDROID,
                name = cfg.deviceName,
                targets = listOf(InetSocketAddress(InetAddress.getByName("255.255.255.255"), 47821)),
            ),
            seen,
            s,
        ).also { it.start() }
        nsd = NsdDiscovery(context, gid, me, manager.localPort, cfg.deviceName, seen, s).also { it.start() }

        val writer = ClipboardWriter(context) { AppState.paused.value }
        // Incoming items of a disabled kind are dropped here; outgoing ones in submit().
        val gate = object : io.transclip.engine.ClipboardSink {
            override fun apply(clip: io.transclip.engine.RemoteClip) {
                val allowed = if (clip.clip is Clip.Text) cfg.syncText else cfg.syncImages
                if (allowed) writer.apply(clip)
            }
        }
        if (cfg.autoScreenshots && hasPhotoPermission()) {
            screenshots = io.transclip.app.clip.ScreenshotWatcher(context, s) { submit(it) }.also { it.start() }
        }
        allowText = cfg.syncText
        allowImages = cfg.syncImages
        val clipSync = ClipboardSync(manager, gate, me, s, io.transclip.engine.ClipConfig(btInlineImageMax = cfg.btInlineImageMax))
        sync = clipSync
        val resolved = TargetDirRepo(context, settings).resolve()
        if (resolved.fellBack) {
            AppState.addHistory(context.getString(R.string.target_dir_fallback))
            TransferNotifier.targetDirFallback(context)
        }
        val tm = TransferManager(
            manager,
            resolved.store,
            s,
            io.transclip.engine.TransferConfig(receiveFiles = cfg.receiveFiles),
        )
        transfers = tm

        manager.status.onEach { AppState.setPeers(it) }.launchIn(s)
        s.launch {
            for (ev in manager.events) {
                if (ev is io.transclip.engine.ManagerEvent.FrameIn) {
                    val req = io.transclip.engine.Rekey.parse(ev.frame.message)
                    if (req != null) {
                        // Switch to the new key, tell the sender, then reconnect with it.
                        secrets.setGroupKey(req.newGk)
                        runCatching { manager.send(req.by, io.transclip.engine.Rekey.ack(req)) }
                        kotlinx.coroutines.delay(300)
                        restartDetached()
                        continue
                    }
                }
                // Clipboard frames are consumed by clipSync, file frames by the transfer manager.
                clipSync.handleEvent(ev)?.let { tm.handleEvent(it) }
            }
        }
        s.launch {
            for (ev in tm.events) {
                when (ev) {
                    is TransferEvent.Started -> AppState.transferStarted(
                        io.transclip.app.ActiveTransfer(ev.xferId, ev.batchId, ev.name, ev.offset, ev.bytes, ev.incoming),
                    )
                    is TransferEvent.Progress -> AppState.transferProgress(ev.xferId, ev.done, ev.total)
                    is TransferEvent.Finished -> AppState.transferEnded(ev.xferId)
                    else -> {}
                }
                if (ev is TransferEvent.BatchFinished) {
                    TransferNotifier.batchFinished(context, ev.ok, ev.failed, ev.incoming)
                    AppState.addHistory(
                        (if (ev.incoming) "File diterima: " else "File terkirim: ") + "${ev.ok} berhasil, ${ev.failed} gagal",
                    )
                }
            }
        }
        s.launch {
            for (ev in clipSync.events) {
                Log.d(TAG, "clip event: ${ev::class.simpleName}")
                when (ev) {
                    is ClipEvent.Applied -> AppState.addHistory("Diterima dari perangkat lain")
                    is ClipEvent.ImageAvailable -> {
                        AppState.addHistory("Gambar besar tersedia (${ev.bytes / 1024} KB)")
                        TransferNotifier.imageAvailable(context, ev.from.toHex(), ev.id, ev.bytes)
                    }
                    is ClipEvent.Delivered -> AppState.addHistory("Terkirim")
                    is ClipEvent.Failed -> AppState.addHistory("Gagal mengirim (${ev.reason})")
                    else -> {}
                }
            }
        }
        AppState.setRunning(true)
        Log.i(TAG, "engine started on port ${manager.localPort}")
        return true
    }

    private fun hasPhotoPermission(): Boolean {
        val perm = if (android.os.Build.VERSION.SDK_INT >= 33) {
            android.Manifest.permission.READ_MEDIA_IMAGES
        } else {
            android.Manifest.permission.READ_EXTERNAL_STORAGE
        }
        return context.checkSelfPermission(perm) == android.content.pm.PackageManager.PERMISSION_GRANTED
    }

    /** Stop and start again from a scope that outlives the engine's own. */
    private fun restartDetached() {
        CoroutineScope(SupervisorJob() + Dispatchers.Default).launch {
            stop()
            start()
        }
    }

    /** Give the group a new key; peers in [revoke] are not told and so drop out. Returns peers notified. */
    suspend fun rotateKey(revoke: List<DeviceId>): Int {
        val manager = mgr ?: return 0
        val newKey = GroupKey.generate()
        val targets = manager.onlinePeers().filter { it !in revoke }
        val sent = io.transclip.engine.Rekey.broadcast(manager, newKey, "TransClip", secrets.deviceId(), targets)
        secrets.setGroupKey(newKey)
        kotlinx.coroutines.delay(500)
        restartDetached()
        return sent
    }

    /** Files copied for sharing are only needed while sending; drop the ones older than a day. */
    private fun pruneOutgoingCache() {
        val cutoff = System.currentTimeMillis() - 24 * 60 * 60 * 1000
        File(context.cacheDir, "outgoing").listFiles()?.filter { it.lastModified() < cutoff }?.forEach { it.deleteRecursively() }
    }

    suspend fun stop() {
        screenshots?.stop()
        screenshots = null
        rfcomm?.stop()
        rfcomm = null
        beacon?.stop()
        nsd?.stop()
        mgr?.stop()
        lock?.let { if (it.isHeld) it.release() }
        scope?.cancel()
        beacon = null
        nsd = null
        mgr = null
        sync = null
        transfers = null
        lock = null
        scope = null
        AppState.setRunning(false)
        AppState.setPeers(emptyList())
    }

    /** Called when the network changes: announce and let discovery run again. */
    fun onNetworkChanged() {
        beacon?.announceNow()
    }

    /** Share a clipboard item with all peers. Returns null when the engine is not running. */
    suspend fun submit(clip: Clip): Submitted? {
        if (AppState.paused.value) return null
        val syncer = sync ?: return null
        if (if (clip is Clip.Text) !allowText else !allowImages) return null
        return syncer.submitLocal(clip)
    }

    /** Send files and folders to the given peers (all online peers when [to] is empty). */
    suspend fun sendFiles(files: List<File>, to: List<DeviceId> = emptyList()): Int {
        val tm = transfers ?: return 0
        val targets = to.ifEmpty { mgr?.onlinePeers().orEmpty() }
        if (targets.isEmpty()) return 0
        val batches: List<Batch> = batchesFromPaths(files)
        val s = scope ?: return 0
        for (peer in targets) {
            for (b in batches) s.launch { runCatching { tm.sendBatch(peer, b) } }
        }
        return targets.size
    }

    /** Stop a running transfer: outgoing batches by batch id, incoming files by transfer id. */
    suspend fun cancelTransfer(t: io.transclip.app.ActiveTransfer) {
        val tm = transfers ?: return
        if (t.incoming) tm.cancelIncoming(t.xferId) else tm.cancelBatch(t.batchId)
    }

    /** Fetch an announced image (the user tapped the notification). */
    suspend fun pullImage(from: DeviceId, id: String) {
        sync?.pull(from, id)
    }

    fun onlineCount(): Int = mgr?.onlinePeers()?.size ?: 0
}
