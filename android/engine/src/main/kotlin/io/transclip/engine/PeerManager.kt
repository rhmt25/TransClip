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
import kotlinx.coroutines.withTimeout
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
    /** Bluetooth dialer; null disables the Bluetooth fallback. */
    val bt: BtDialer? = null,
    /** Open Bluetooth when the LAN link has been down this long (PLAN section 4.2). */
    val btDownAfterMs: Long = 10_000,
    /** Close Bluetooth when the LAN link has been up this long. */
    val btCloseAfterMs: Long = 60_000,
    val btMaxLinks: Int = 3,
)

/** How a link reaches the peer. */
enum class Transport { LAN, BLUETOOTH }

/** Opens Bluetooth connections to paired devices; implemented by the platform. */
interface BtDialer {
    /** Connect to [peer]; throws when the device is not reachable. */
    suspend fun connect(peer: DeviceId): RawConn
}

enum class PeerState { OFFLINE, CONNECTING, ONLINE }

/** Public view of one peer. */
data class PeerStatus(
    val id: DeviceId,
    val name: String,
    val state: PeerState,
    val addr: InetSocketAddress?,
    val transport: Transport? = null,
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
        val via: Transport,
        val info: PeerInfo,
        val handle: Link,
        val accept: CompletableDeferred<Boolean>,
    ) : Internal
    class Closed(val link: Long, val reason: CloseReason) : Internal
    class DialFailed(val id: DeviceId) : Internal
    class BtDialFailed(val id: DeviceId) : Internal
    class SetBtPriority(val peers: Set<DeviceId>) : Internal
    data object Tick : Internal
    class Stop(val done: CompletableDeferred<Unit>) : Internal
}

private class Active(val link: Long, val dir: Dir, val via: Transport, val handle: Link)

private class Peer(var name: String, var addr: InetSocketAddress?, backoffMs: Long, nowMs: Long) {
    /** The link in use (LAN preferred). */
    var active: Active? = null

    /** The other transport's link, kept as a spare. */
    var standby: Active? = null
    var dialing = false
    var btDialing = false
    var backoffMs = backoffMs
    var nextTryMs = nowMs
    var btNextTryMs = nowMs

    /** Since when there has been no LAN link. */
    var offlineSinceMs = nowMs

    /** Since when a LAN link has been up. */
    var lanUpSinceMs: Long? = null

    fun links(): List<Active> = listOfNotNull(active, standby)
    fun has(via: Transport) = links().any { it.via == via }

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

    /** Hand over a connection accepted by a platform listener (Bluetooth server socket). */
    fun acceptConnection(conn: RawConn) {
        val link = linkIds.getAndIncrement()
        scope.launch(Dispatchers.IO) {
            val ch = try {
                conn.setReadTimeout(HANDSHAKE_TIMEOUT_MS)
                respond(conn.input, conn.output, cfg.gk)
            } catch (e: Exception) {
                runCatching { conn.close() }
                null
            }
            if (ch != null) {
                val l = Link(conn, ch, cfg.local, cfg.link, scope)
                l.start()
                pump(link, Dir.INBOUND, Transport.BLUETOOTH, null, l)
            }
        }
    }

    /** Peers that should get a Bluetooth link when the LAN is down (PLAN section 4.2). */
    fun setBtPriority(peers: Set<DeviceId>) {
        internal.trySend(Internal.SetBtPriority(peers))
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
                    pump(link, Dir.INBOUND, Transport.LAN, null, l)
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
                pump(link, Dir.OUTBOUND, Transport.LAN, id, l)
            } else {
                false
            }
            if (!ready) internal.send(Internal.DialFailed(id))
        }
    }

    /** Forward one link's events. Returns false when the link never became ready or was refused. */
    private suspend fun pump(link: Long, dir: Dir, via: Transport, expect: DeviceId?, handle: Link): Boolean {
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
        internal.send(Internal.Ready(link, dir, via, peer, handle, accept))
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
    private var btPriority: Set<DeviceId> = emptySet()

    private suspend fun actor() {
        for (msg in internal) {
            when (msg) {
                is Internal.Stop -> {
                    peers.values.forEach { p -> p.links().forEach { it.handle.close("shutdown") } }
                    handles.clear()
                    msg.done.complete(Unit)
                    return
                }
                is Internal.Seen -> onSeen(msg.seen)
                is Internal.Tick -> peers.keys.toList().forEach {
                    maybeDial(it)
                    btPolicy(it)
                }
                is Internal.DialFailed -> {
                    peers[msg.id]?.let {
                        it.dialing = false
                        it.nextTryMs = nowMs() + it.backoffMs
                        it.backoffMs = nextBackoff(it.backoffMs, cfg.backoffMaxMs)
                    }
                    publish()
                }
                is Internal.BtDialFailed -> {
                    peers[msg.id]?.let {
                        it.btDialing = false
                        it.btNextTryMs = nowMs() + 10_000
                    }
                    publish()
                }
                is Internal.SetBtPriority -> btPriority = msg.peers
                is Internal.Ready -> msg.accept.complete(onReady(msg))
                is Internal.Closed -> onClosed(msg)
            }
        }
    }

    private fun publish() {
        statusFlow.value = peers.map { (id, p) -> PeerStatus(id, p.name, p.state, p.addr, p.active?.via) }.sortedBy { it.id }
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
        if (peer.has(Transport.LAN) || peer.dialing || now < peer.nextTryMs || !allowed) return
        peer.dialing = true
        spawnDial(id, addr)
        publish()
    }

    /**
     * Bluetooth fallback and release (PLAN section 4.2): open a Bluetooth link for a priority peer
     * once the LAN has been down for `btDownAfterMs`, close it after the LAN has been stable for
     * `btCloseAfterMs`.
     */
    private fun btPolicy(id: DeviceId) {
        val dialer = cfg.bt ?: return
        val peer = peers[id] ?: return
        val now = nowMs()
        if (peer.has(Transport.LAN)) {
            val up = peer.lanUpSinceMs
            if (up != null && now - up >= cfg.btCloseAfterMs) {
                peer.links().firstOrNull { it.via == Transport.BLUETOOTH }?.handle?.close("lan_restored")
            }
            return
        }
        val btLinks = peers.values.count { it.has(Transport.BLUETOOTH) } + peers.values.count { it.btDialing }
        if (id !in btPriority || now - peer.offlineSinceMs < cfg.btDownAfterMs ||
            peer.has(Transport.BLUETOOTH) || peer.btDialing || now < peer.btNextTryMs || btLinks >= cfg.btMaxLinks
        ) {
            return
        }
        peer.btDialing = true
        val link = linkIds.getAndIncrement()
        scope.launch(Dispatchers.IO) {
            val conn = try {
                withTimeout(cfg.connectTimeoutMs.toLong()) {
                    val c = dialer.connect(id)
                    try {
                        c.setReadTimeout(HANDSHAKE_TIMEOUT_MS)
                        val ch = withContext(Dispatchers.IO) { initiate(c.input, c.output, cfg.gk) }
                        c to ch
                    } catch (e: Exception) {
                        runCatching { c.close() }
                        throw e
                    }
                }
            } catch (e: Exception) {
                null
            }
            val ready = if (conn != null) {
                val l = Link(conn.first, conn.second, cfg.local, cfg.link, scope)
                l.start()
                pump(link, Dir.OUTBOUND, Transport.BLUETOOTH, id, l)
            } else {
                false
            }
            if (!ready) internal.send(Internal.BtDialFailed(id))
        }
        publish()
    }

    private suspend fun onReady(r: Internal.Ready): Boolean {
        val id = r.info.id
        val local = cfg.local.id
        if (id == local || !admit(id, r.info.name)) return false
        val peer = peers.getOrPut(id) { Peer(r.info.name, null, cfg.backoffMinMs, nowMs()) }
        if (r.dir == Dir.OUTBOUND) {
            if (r.via == Transport.LAN) peer.dialing = false else peer.btDialing = false
        }
        val wasOnline = peer.active != null
        // Two links over the same transport are a duplicate: the one started by the smaller id
        // wins. Links over different transports coexist; the LAN one is used.
        peer.links().firstOrNull { it.via == r.via }?.let { old ->
            if (initiator(r.dir, local, id) > initiator(old.dir, local, id)) return false
            scope.launch {
                runCatching { old.handle.sendControl(Message.Error(code = "duplicate")) }
                old.handle.close("duplicate")
            }
        }
        val links = (peer.links().filter { it.via != r.via } + Active(r.link, r.dir, r.via, r.handle))
            .sortedBy { it.via != Transport.LAN }
        peer.active = links.first()
        peer.standby = links.getOrNull(1)
        peer.name = r.info.name
        peer.backoffMs = cfg.backoffMinMs
        if (r.via == Transport.LAN && peer.lanUpSinceMs == null) peer.lanUpSinceMs = nowMs()
        handles[id] = peer.active!!.handle
        publish()
        if (!wasOnline) events.send(ManagerEvent.Online(r.info))
        return true
    }

    private suspend fun onClosed(c: Internal.Closed) {
        val entry = peers.entries.firstOrNull { e -> e.value.links().any { it.link == c.link } } ?: return // replaced duplicate
        val (id, peer) = entry
        val now = nowMs()
        if (peer.links().any { it.link == c.link && it.via == Transport.LAN }) {
            peer.offlineSinceMs = now
            peer.lanUpSinceMs = null
        }
        if (peer.standby?.link == c.link) {
            peer.standby = null
            publish()
            return
        }
        // The link in use ended: fall back to the spare, if any, without going offline.
        peer.active = peer.standby
        peer.standby = null
        val next = peer.active
        if (next != null) {
            handles[id] = next.handle
            publish()
            return
        }
        peer.nextTryMs = now + cfg.backoffMinMs
        handles.remove(id)
        publish()
        events.send(ManagerEvent.Offline(id, c.reason))
    }
}
