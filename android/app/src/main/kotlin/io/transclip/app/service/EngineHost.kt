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
    private var beacon: BeaconService? = null
    private var nsd: NsdDiscovery? = null
    private var lock: WifiManager.MulticastLock? = null

    val isRunning: Boolean get() = mgr != null

    /** Start the engine. Without a group key nothing is started and `paired` stays false. */
    suspend fun start(): Boolean {
        if (isRunning) return true
        val gk: GroupKey? = secrets.groupKey()
        AppState.setPaired(gk != null)
        if (gk == null) return false
        val cfg = settings.get()
        AppState.setPaused(cfg.paused)
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
                local = LocalInfo(me, cfg.deviceName, "android", "transclip/0.1.0", listOf("clip"), "full"),
                gk = gk,
                listenPort = TCP_PORT,
            ),
            s,
            seen,
        )
        manager.start()
        mgr = manager

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
        val clipSync = ClipboardSync(manager, writer, me, s)
        sync = clipSync

        manager.status.onEach { AppState.setPeers(it) }.launchIn(s)
        s.launch {
            for (ev in manager.events) clipSync.handleEvent(ev)
        }
        s.launch {
            for (ev in clipSync.events) {
                Log.d(TAG, "clip event: ${ev::class.simpleName}")
                when (ev) {
                    is ClipEvent.Applied -> AppState.addHistory("Diterima dari perangkat lain")
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

    suspend fun stop() {
        beacon?.stop()
        nsd?.stop()
        mgr?.stop()
        lock?.let { if (it.isHeld) it.release() }
        scope?.cancel()
        beacon = null
        nsd = null
        mgr = null
        sync = null
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
        return syncer.submitLocal(clip)
    }

    fun onlineCount(): Int = mgr?.onlinePeers()?.size ?: 0
}
