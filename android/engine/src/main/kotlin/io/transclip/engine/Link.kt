package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.Frame
import io.transclip.proto.Message
import io.transclip.proto.PROTOCOL_VERSION
import io.transclip.proto.ProtoException
import io.transclip.proto.SecureChannel
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.withContext
import java.io.EOFException
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.util.concurrent.atomic.AtomicBoolean

/** Timings of a link; the defaults are the values in PROTOCOL.md section 10. */
data class LinkConfig(
    val pingIntervalMs: Long = 30_000,
    val deadAfterMs: Long = 90_000,
)

/** What this device tells the peer in `hello`. */
data class LocalInfo(
    val id: DeviceId,
    val name: String,
    val platform: String,
    val app: String,
    val caps: List<String>,
    val role: String,
)

/** What the peer told us in `hello`. */
data class PeerInfo(
    val id: DeviceId,
    val name: String,
    val platform: String,
    val app: String,
    val caps: List<String>,
    val role: String,
)

/** Why a link ended. */
sealed interface CloseReason {
    data class PeerBye(val reason: String) : CloseReason
    data object PeerClosed : CloseReason
    data object Dead : CloseReason
    data object Local : CloseReason
    data class Error(val message: String) : CloseReason
}

/** Something that happened on a link. `Closed` is always the last event. */
sealed interface LinkEvent {
    data class Ready(val info: PeerInfo) : LinkEvent
    class FrameIn(val frame: Frame) : LinkEvent
    data class Closed(val reason: CloseReason) : LinkEvent
}

private sealed interface Outgoing {
    class Send(val message: Message, val payload: ByteArray) : Outgoing
    class Close(val reason: String) : Outgoing
}

/**
 * One encrypted connection to a peer: `hello` exchange, keep-alive, dead-peer detection and
 * two outgoing queues where control messages overtake queued data (PROTOCOL.md sections 7, 9.1).
 *
 * Call [start] once. Events arrive on [events]; the last one is [LinkEvent.Closed].
 */
class Link(
    private val socket: Socket,
    private val channel: SecureChannel,
    private val local: LocalInfo,
    private val cfg: LinkConfig,
    private val scope: CoroutineScope,
) {
    val events = Channel<LinkEvent>(Channel.UNLIMITED)
    private val control = Channel<Outgoing>(64)
    private val data = Channel<Outgoing>(64)
    private val finished = AtomicBoolean(false)
    private val jobs = mutableListOf<Job>()

    @Volatile
    private var lastWrite = System.nanoTime()

    val isClosed: Boolean get() = finished.get()

    fun start() {
        socket.soTimeout = cfg.deadAfterMs.toInt()
        jobs += scope.launch(Dispatchers.IO) { reader() }
        jobs += scope.launch(Dispatchers.IO) { writer() }
        jobs += scope.launch { pinger() }
    }

    /** Send a high-priority message (clipboard items, acks, offers). */
    suspend fun sendControl(message: Message, payload: ByteArray = ByteArray(0)) {
        if (finished.get()) throw ProtoException.Closed()
        control.send(Outgoing.Send(message, payload))
    }

    /** Send a low-priority message (file chunks). */
    suspend fun sendData(message: Message, payload: ByteArray = ByteArray(0)) {
        if (finished.get()) throw ProtoException.Closed()
        data.send(Outgoing.Send(message, payload))
    }

    /** Say `bye` and close. Queued control messages go out first. */
    fun close(reason: String) {
        if (!finished.get()) control.trySend(Outgoing.Close(reason))
    }

    /** Close immediately without `bye`. */
    fun abort() = finish(CloseReason.Local)

    private fun finish(reason: CloseReason) {
        if (!finished.compareAndSet(false, true)) return
        events.trySend(LinkEvent.Closed(reason))
        events.close()
        runCatching { socket.close() }
        control.close()
        data.close()
        jobs.forEach { it.cancel() }
    }

    private fun mapError(e: Throwable): CloseReason = when (e) {
        is SocketTimeoutException, is ProtoException.Timeout -> CloseReason.Dead
        is ProtoException.Closed, is EOFException, is SocketException -> CloseReason.PeerClosed
        else -> CloseReason.Error(e.message ?: e::class.simpleName.orEmpty())
    }

    private fun reader() {
        var ready = false
        try {
            while (true) {
                val frame = channel.input.readFrame()
                when (val m = frame.message) {
                    is Message.Hello -> {
                        if (ready) continue
                        if (m.v != PROTOCOL_VERSION) throw ProtoException.Version(m.v)
                        ready = true
                        val info = PeerInfo(DeviceId.fromHex(m.id), m.name, m.pl, m.app, m.caps, m.role)
                        events.trySend(LinkEvent.Ready(info))
                    }
                    is Message.Ping -> control.trySend(Outgoing.Send(Message.Pong, ByteArray(0)))
                    is Message.Pong, is Message.Unknown -> {}
                    is Message.Bye -> {
                        finish(CloseReason.PeerBye(m.reason))
                        return
                    }
                    else -> {
                        if (!ready) throw ProtoException.BadHeader("message before hello")
                        events.trySend(LinkEvent.FrameIn(frame))
                    }
                }
            }
        } catch (e: Throwable) {
            finish(mapError(e))
        }
    }

    private suspend fun writer() {
        try {
            withContext(Dispatchers.IO) {
                channel.output.writeFrame(
                    Message.Hello(
                        PROTOCOL_VERSION,
                        local.id.toHex(),
                        local.name,
                        local.platform,
                        local.app,
                        local.caps,
                        local.role,
                    ),
                )
            }
            lastWrite = System.nanoTime()
            while (true) {
                // select is biased to the first clause, so control always wins over data.
                val out = select<Outgoing> {
                    control.onReceive { it }
                    data.onReceive { it }
                }
                when (out) {
                    is Outgoing.Send -> {
                        withContext(Dispatchers.IO) { channel.output.writeFrame(out.message, out.payload) }
                        lastWrite = System.nanoTime()
                    }
                    is Outgoing.Close -> {
                        runCatching {
                            withContext(Dispatchers.IO) { channel.output.writeFrame(Message.Bye(out.reason)) }
                        }
                        finish(CloseReason.Local)
                        return
                    }
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Throwable) {
            finish(mapError(e))
        }
    }

    private suspend fun pinger() {
        val step = (cfg.pingIntervalMs / 2).coerceAtLeast(10)
        while (!finished.get()) {
            delay(step)
            val idleMs = (System.nanoTime() - lastWrite) / 1_000_000
            if (idleMs >= cfg.pingIntervalMs) control.trySend(Outgoing.Send(Message.Ping, ByteArray(0)))
        }
    }
}
