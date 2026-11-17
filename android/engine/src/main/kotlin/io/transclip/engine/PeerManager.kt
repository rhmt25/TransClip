package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.Frame
import io.transclip.proto.GroupKey
import io.transclip.proto.Message
import io.transclip.proto.ProtoException
import io.transclip.proto.initiate
import io.transclip.proto.respond
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.withContext
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap

const val MAX_PEERS = 10
private const val MAX_PENDING_HANDSHAKES = 16
private const val HANDSHAKE_TIMEOUT_MS = 10_000

/** Manager settings; the defaults are the values in PROTOCOL.md. */
data class ManagerConfig(
    val local: LocalInfo,
    val gk: GroupKey,
    val listenPort: Int = 47820,
    val link: LinkConfig = LinkConfig(),
    val backoffMinMs: Long = 2_000,
    val backoffMaxMs: Long = 30_000,
    val fallbackAfterMs: Long = 30_000,
    val maxPeers: Int = MAX_PEERS,
    val connectTimeoutMs: Int = 15_000,
)

enum class PeerState { OFFLINE, CONNECTING, ONLINE }

/** Public view of one peer. */
data class PeerStatus(
    val id: DeviceId,
    val name: String,
    val state: PeerState,
    val addr: InetSocketAddress?,
)

/** Events for the application. */
sealed interface ManagerEvent {
    data class Online(val info: PeerInfo) : ManagerEvent
    data class Offline(val id: DeviceId, val reason: CloseReason) : ManagerEvent
    class FrameIn(val from: DeviceId, val frame: Frame) : ManagerEvent
    data class Rejected(val id: DeviceId, val name: String) : ManagerEvent
}

/** Next reconnect delay after a failed attempt. */
fun nextBackoff(currentMs: Long, maxMs: Long): Long = (currentMs * 2).coerceAtMost(maxMs)

private enum class Dir { INBOUND, OUTBOUND }

private sealed interface Internal {
    class Seen(val seen: PeerSeen) : Internal
    class Ready(
        val link: Long,
        val dir: Dir,
        val info: PeerInfo,
        val handle: Link,
        val accept: CompletableDeferred<Boolean>,
    ) : Internal
    class Closed(val link: Long, val reason: CloseReason) : Internal
    class DialFailed(val id: DeviceId) : Internal
    data object Tick : Internal
    class Stop(val done: CompletableDeferred<Unit>) : Internal
}

private class Active(val link: Long, val dir: Dir, val handle: Link)

private class Peer(var name: String, var addr: InetSocketAddress?, backoffMs: Long, nowMs: Long) {
    var active: Active? = null
    var dialing = false
    var backoffMs = backoffMs
    var nextTryMs = nowMs
    var offlineSinceMs = nowMs

    val state: PeerState
        get() = when {
            active != null -> PeerState.ONLINE
            dialing -> PeerState.CONNECTING
            else -> PeerState.OFFLINE
        }
}

private fun nowMs() = System.nanoTime() / 1_000_000

/** The connection initiated by the smaller device id wins a duplicate. */
private fun initiator(dir: Dir, local: DeviceId, peer: DeviceId) = if (dir == Dir.OUTBOUND) local else peer

/**
 * One control link per peer: dial rule, backoff, duplicate resolution and peer limit
 * (PROTOCOL.md section 5). A single actor coroutine owns all peer state.
 */
class PeerManager(
    private val cfg: ManagerConfig,
    parent: CoroutineScope,
    private val seen: ReceiveChannel<PeerSeen>,
) {
    private val scope = CoroutineScope(parent.coroutineContext + SupervisorJob(parent.coroutineContext[Job]))
    private val internal = Channel<Internal>(Channel.UNLIMITED)
    private val handles = ConcurrentHashMap<DeviceId, Link>()
    private val linkIds = java.util.concurrent.atomic.AtomicLong(1)
    private val statusFlow = MutableStateFlow<List<PeerStatus>>(emptyList())
    private var server: ServerSocket? = null

    /** Events for the application. Unbounded so the manager never blocks on a slow consumer. */
    val events = Channel<ManagerEvent>(Channel.UNLIMITED)

    val status: StateFlow<List<PeerStatus>> get() = statusFlow

    val localPort: Int get() = server?.localPort ?: 0

    fun onlinePeers(): List<DeviceId> = handles.keys.toList()

    fun handle(id: DeviceId): Link? = handles[id]

    /** Send a control-priority message to one peer. */
    suspend fun send(to: DeviceId, message: Message, payload: ByteArray = ByteArray(0)) {
        val link = handles[to] ?: throw ProtoException.Closed()
        link.sendControl(message, payload)
    }

    /** Bind the listener and start the actor. */
    fun start() {
        val s = ServerSocket().apply {
            reuseAddress = true
            bind(InetSocketAddress(cfg.listenPort), 16)
        }
        server = s
        scope.launch { actor() }
        scope.launch(Dispatchers.IO) { acceptLoop(s) }
        scope.launch {
            for (x in seen) internal.send(Internal.Seen(x))
        }
        scope.launch {
            while (isActive) {
                delay(100)
                internal.trySend(Internal.Tick)
            }
        }
    }

    /** Say `bye` to all peers, close the listener and stop. */
    suspend fun stop() {
        val done = CompletableDeferred<Unit>()
        internal.trySend(Internal.Stop(done))
        done.await()
        runCatching { server?.close() }
        scope.cancel()
    }

    // ------------------------------------------------------------- listener / dialer

    private suspend fun acceptLoop(s: ServerSocket) {
        val slots = Semaphore(MAX_PENDING_HANDSHAKES)
        while (scope.isActive) {
            val sock = try {
                s.accept()
            } catch (e: Exception) {
                if (s.isClosed) return
                delay(50)
                continue
            }
            if (!slots.tryAcquire()) {
                runCatching { sock.close() }
                continue
            }
            val link = linkIds.getAndIncrement()
            scope.launch(Dispatchers.IO) {
                val conn = try {
                    sock.tcpNoDelay = true
                    sock.soTimeout = HANDSHAKE_TIMEOUT_MS
                    respond(sock.getInputStream(), sock.getOutputStream(), cfg.gk)
                } catch (e: Exception) {
                    runCatching { sock.close() }
                    null
                } finally {
                    slots.release()
                }
                if (conn != null) {
                    val l = Link(sock, conn, cfg.local, cfg.link, scope)
                    l.start()
                    pump(link, Dir.INBOUND, null, l)
                }
            }
        }
    }

    private fun spawnDial(id: DeviceId, addr: InetSocketAddress) {
        val link = linkIds.getAndIncrement()
        scope.launch(Dispatchers.IO) {
            val sock = Socket()
            val conn = try {
                sock.tcpNoDelay = true
                sock.connect(addr, cfg.connectTimeoutMs)
                sock.soTimeout = HANDSHAKE_TIMEOUT_MS
                initiate(sock.getInputStream(), sock.getOutputStream(), cfg.gk)
            } catch (e: Exception) {
                runCatching { sock.close() }
                null
            }
            val ready = if (conn != null) {
                val l = Link(sock, conn, cfg.local, cfg.link, scope)
                l.start()
                pump(link, Dir.OUTBOUND, id, l)
            } else {
                false
            }
            if (!ready) internal.send(Internal.DialFailed(id))
        }
    }

    /** Forward one link's events. Returns false when the link never became ready or was refused. */
    private suspend fun pump(link: Long, dir: Dir, expect: DeviceId?, handle: Link): Boolean {
        var info: PeerInfo? = null
        for (ev in handle.events) {
            when (ev) {
                is LinkEvent.Ready -> {
                    info = ev.info
                }
                is LinkEvent.Closed -> return false
                is LinkEvent.FrameIn -> {}
            }
            if (info != null) break
        }
        val peer = info ?: return false
        if (expect != null && expect != peer.id) {
            handle.close("unexpected-peer")
            return false
        }
        val accept = CompletableDeferred<Boolean>()
        internal.send(Internal.Ready(link, dir, peer, handle, accept))
        if (!accept.await()) {
            runCatching { handle.sendControl(Message.Error(code = "duplicate")) }
            handle.close("duplicate")
            return false
        }
        for (ev in handle.events) {
            when (ev) {
                is LinkEvent.FrameIn -> events.send(ManagerEvent.FrameIn(peer.id, ev.frame))
                is LinkEvent.Closed -> {
                    internal.send(Internal.Closed(link, ev.reason))
                    return true
                }
                is LinkEvent.Ready -> {}
            }
        }
        return true
    }

    // ------------------------------------------------------------- actor

    private val peers = HashMap<DeviceId, Peer>()
    private val rejected = HashSet<DeviceId>()

    private suspend fun actor() {
        for (msg in internal) {
            when (msg) {
                is Internal.Stop -> {
                    peers.values.forEach { it.active?.handle?.close("shutdown") }
                    handles.clear()
                    msg.done.complete(Unit)
                    return
                }
                is Internal.Seen -> onSeen(msg.seen)
                is Internal.Tick -> peers.keys.toList().forEach(::maybeDial)
                is Internal.DialFailed -> {
                    peers[msg.id]?.let {
                        it.dialing = false
                        it.nextTryMs = nowMs() + it.backoffMs
                        it.backoffMs = nextBackoff(it.backoffMs, cfg.backoffMaxMs)
                    }
                    publish()
                }
                is Internal.Ready -> msg.accept.complete(onReady(msg))
                is Internal.Closed -> onClosed(msg)
            }
        }
    }

    private fun publish() {
        statusFlow.value = peers.map { (id, p) -> PeerStatus(id, p.name, p.state, p.addr) }.sortedBy { it.id }
    }

    private suspend fun admit(id: DeviceId, name: String): Boolean {
        if (peers.containsKey(id)) return true
        if (peers.size >= cfg.maxPeers) {
            if (rejected.add(id)) events.send(ManagerEvent.Rejected(id, name))
            return false
        }
        return true
    }

    private suspend fun onSeen(s: PeerSeen) {
        if (s.deviceId == cfg.local.id || !admit(s.deviceId, s.name)) return
        val peer = peers.getOrPut(s.deviceId) { Peer(s.name, s.addr, cfg.backoffMinMs, nowMs()) }
        peer.name = s.name
        if (peer.addr != s.addr) {
            peer.addr = s.addr
            peer.nextTryMs = nowMs()
        }
        publish()
        maybeDial(s.deviceId)
    }

    private fun maybeDial(id: DeviceId) {
        val peer = peers[id] ?: return
        val addr = peer.addr ?: return
        val now = nowMs()
        val allowed = cfg.local.id < id || now - peer.offlineSinceMs >= cfg.fallbackAfterMs
        if (peer.active != null || peer.dialing || now < peer.nextTryMs || !allowed) return
        peer.dialing = true
        spawnDial(id, addr)
        publish()
    }

    private suspend fun onReady(r: Internal.Ready): Boolean {
        val id = r.info.id
        val local = cfg.local.id
        if (id == local || !admit(id, r.info.name)) return false
        val peer = peers.getOrPut(id) { Peer(r.info.name, null, cfg.backoffMinMs, nowMs()) }
        if (r.dir == Dir.OUTBOUND) peer.dialing = false
        val wasOnline = peer.active != null
        peer.active?.let { old ->
            if (initiator(r.dir, local, id) > initiator(old.dir, local, id)) return false
            scope.launch {
                runCatching { old.handle.sendControl(Message.Error(code = "duplicate")) }
                old.handle.close("duplicate")
            }
        }
        peer.name = r.info.name
        peer.backoffMs = cfg.backoffMinMs
        peer.active = Active(r.link, r.dir, r.handle)
        handles[id] = r.handle
        publish()
        if (!wasOnline) events.send(ManagerEvent.Online(r.info))
        return true
    }

    private suspend fun onClosed(c: Internal.Closed) {
        val entry = peers.entries.firstOrNull { it.value.active?.link == c.link } ?: return // replaced duplicate
        val (id, peer) = entry
        peer.active = null
        val now = nowMs()
        peer.offlineSinceMs = now
        peer.nextTryMs = now + cfg.backoffMinMs
        handles.remove(id)
        publish()
        events.send(ManagerEvent.Offline(id, c.reason))
    }
}
