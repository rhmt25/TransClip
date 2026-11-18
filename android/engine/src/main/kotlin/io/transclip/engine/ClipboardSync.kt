package io.transclip.engine

import io.transclip.proto.ConflictClock
import io.transclip.proto.DeviceId
import io.transclip.proto.MAX_IMAGE
import io.transclip.proto.MAX_TEXT
import io.transclip.proto.Message
import io.transclip.proto.SeenCache
import io.transclip.proto.newUuidV4
import io.transclip.proto.toHex
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap

val IMAGE_MIMES = setOf("image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp")

/** Clipboard content. */
sealed interface Clip {
    data class Text(val text: String) : Clip

    class Image(val mime: String, val w: Int, val h: Int, val bytes: ByteArray) : Clip {
        override fun equals(other: Any?) =
            other is Image && mime == other.mime && w == other.w && h == other.h && bytes.contentEquals(other.bytes)

        override fun hashCode() = bytes.contentHashCode() + mime.hashCode()
    }

    val payload: ByteArray
        get() = when (this) {
            is Text -> text.toByteArray(Charsets.UTF_8)
            is Image -> bytes
        }

    /** Lower-case hex SHA-256 of the payload. */
    fun hash(): String = sha256Hex(payload)
}

fun sha256Hex(data: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(data).toHex()

/** Identity of an item. */
data class ItemMeta(val id: String, val origin: DeviceId, val ts: Long, val hash: String)

/** An item received from a peer, ready to be written to the OS clipboard. */
data class RemoteClip(val meta: ItemMeta, val clip: Clip)

/**
 * Implemented by the platform to write received items. The implementation must mark the write as
 * originating from TransClip so it is not captured again. Called off the main thread.
 */
fun interface ClipboardSink {
    /** Returns normally on success; throws to report failure (the peer is told `ok:false`). */
    fun apply(clip: RemoteClip)
}

data class ClipConfig(val ackTimeoutMs: Long = 15_000)

class ClipException(val kind: Kind) : Exception(kind.name) {
    enum class Kind { TOO_LARGE, BAD_MIME }
}

sealed interface Submitted {
    /** Same content as the current clipboard item: nothing was sent. */
    data object Unchanged : Submitted

    data class Sent(val meta: ItemMeta) : Submitted
}

/** What happened to an item. Never carries content. */
sealed interface ClipEvent {
    data class Delivered(val peer: DeviceId, val id: String) : ClipEvent
    data class Failed(val peer: DeviceId, val id: String, val reason: String) : ClipEvent
    data class Queued(val peer: DeviceId, val id: String) : ClipEvent
    data class Dropped(val peer: DeviceId, val id: String) : ClipEvent
    data class Applied(val from: DeviceId, val id: String) : ClipEvent
}

/** An item waiting for delivery. */
class QueuedItem(val meta: ItemMeta, val clip: Clip)

const val MAX_QUEUED = 5

/** FIFO of at most [MAX_QUEUED] items; the oldest is dropped on overflow. */
class OfflineQueue {
    private val items = ArrayDeque<QueuedItem>()

    val size: Int get() = items.size

    /** Returns the evicted item, if any. Items with an id already queued are ignored. */
    fun push(item: QueuedItem): QueuedItem? {
        if (items.any { it.meta.id == item.meta.id }) return null
        items.addLast(item)
        return if (items.size > MAX_QUEUED) items.removeFirst() else null
    }

    fun drain(): List<QueuedItem> = items.toList().also { items.clear() }
}

private fun nowMs() = System.currentTimeMillis()

/** The clipboard sync component (PLAN sections 4.3 and 4.4). */
class ClipboardSync(
    private val mgr: PeerManager,
    private val sink: ClipboardSink,
    private val local: DeviceId,
    private val scope: CoroutineScope,
    private val cfg: ClipConfig = ClipConfig(),
) {
    val events = Channel<ClipEvent>(Channel.UNLIMITED)

    private val lock = Any()
    private val seen = SeenCache()
    private val clock = ConflictClock()
    private var lastHash: String? = null
    private var lastTs = 0L
    private val queues = HashMap<DeviceId, OfflineQueue>()
    private val pending = ConcurrentHashMap<Pair<DeviceId, String>, CompletableDeferred<Pair<Boolean, String?>>>()

    /** The OS clipboard changed locally. Sends to online peers and queues for offline ones. */
    suspend fun submitLocal(clip: Clip): Submitted {
        when (clip) {
            is Clip.Text -> if (clip.payload.size > MAX_TEXT) throw ClipException(ClipException.Kind.TOO_LARGE)
            is Clip.Image -> {
                if (clip.bytes.size > MAX_IMAGE) throw ClipException(ClipException.Kind.TOO_LARGE)
                if (clip.mime !in IMAGE_MIMES) throw ClipException(ClipException.Kind.BAD_MIME)
            }
        }
        val hash = clip.hash()
        val meta = synchronized(lock) {
            if (lastHash == hash) return Submitted.Unchanged
            val ts = maxOf(nowMs(), lastTs + 1)
            lastTs = ts
            lastHash = hash
            val id = newUuidV4()
            // Our own item counts as applied, and its id must never be applied if it echoes back.
            seen.insert(id, ts)
            clock.shouldApply(ts, local)
            ItemMeta(id, local, ts, hash)
        }
        val item = QueuedItem(meta, clip)
        for (p in mgr.status.value) {
            if (p.state == PeerState.ONLINE) launchSend(p.id, item) else enqueue(p.id, item)
        }
        return Submitted.Sent(meta)
    }

    /** Process a manager event. Clipboard frames are consumed (null); everything else is returned. */
    suspend fun handleEvent(event: ManagerEvent): ManagerEvent? {
        when (event) {
            is ManagerEvent.FrameIn -> when (val m = event.frame.message) {
                is Message.Ack -> {
                    pending.remove(event.from to m.id)?.complete(m.ok to m.reason)
                    return null
                }
                is Message.Text, is Message.Image -> {
                    onItem(event.from, m, event.frame.payload)
                    return null
                }
                else -> return event
            }
            is ManagerEvent.Online -> {
                val queued = synchronized(lock) { queues[event.info.id]?.drain().orEmpty() }
                queued.forEach { launchSend(event.info.id, it) }
                return event
            }
            else -> return event
        }
    }

    private val rate = PeerRate()

    private suspend fun onItem(from: DeviceId, message: Message, payload: ByteArray) {
        val (id, parsed) = parseItem(message, payload)
        suspend fun reply(ok: Boolean, reason: String? = null) {
            runCatching { mgr.send(from, Message.Ack(id, ok, reason)) }
        }
        val remote = parsed.getOrElse { return reply(false, it.message) }
        if (!rate.allow(from, nowMs())) return reply(false, "busy")
        val apply = synchronized(lock) {
            val fresh = seen.insert(remote.meta.id, nowMs())
            remote.meta.origin != local &&
                fresh &&
                lastHash != remote.meta.hash &&
                clock.shouldApply(remote.meta.ts, remote.meta.origin)
        }
        if (!apply) return reply(true)
        val outcome = withContext(Dispatchers.IO) { runCatching { sink.apply(remote) } }
        if (outcome.isSuccess) {
            synchronized(lock) { lastHash = remote.meta.hash }
            events.send(ClipEvent.Applied(from, remote.meta.id))
            reply(true)
        } else {
            reply(false, "apply")
        }
    }

    private fun launchSend(peer: DeviceId, item: QueuedItem) {
        scope.launch { sendWithAck(peer, item) }
    }

    private suspend fun enqueue(peer: DeviceId, item: QueuedItem) {
        val evicted = synchronized(lock) { queues.getOrPut(peer) { OfflineQueue() }.push(item) }
        events.send(ClipEvent.Queued(peer, item.meta.id))
        if (evicted != null) events.send(ClipEvent.Dropped(peer, evicted.meta.id))
    }

    /** Send once, wait for the ack, retry once, then report failure (PROTOCOL.md section 10). */
    private suspend fun sendWithAck(peer: DeviceId, item: QueuedItem) {
        val id = item.meta.id
        repeat(2) {
            val waiter = CompletableDeferred<Pair<Boolean, String?>>()
            pending[peer to id] = waiter
            try {
                mgr.send(peer, wire(item.meta, item.clip), item.clip.payload)
            } catch (e: java.io.IOException) {
                pending.remove(peer to id)
                enqueue(peer, item)
                return
            }
            val answer = withTimeoutOrNull(cfg.ackTimeoutMs) { waiter.await() }
            pending.remove(peer to id)
            when {
                answer == null -> {}
                answer.first -> return events.send(ClipEvent.Delivered(peer, id))
                else -> return events.send(ClipEvent.Failed(peer, id, answer.second ?: "refused"))
            }
        }
        events.send(ClipEvent.Failed(peer, id, "timeout"))
    }

    private fun wire(meta: ItemMeta, clip: Clip): Message = when (clip) {
        is Clip.Text -> Message.Text(meta.id, meta.origin.toHex(), meta.ts, meta.hash)
        is Clip.Image -> Message.Image(meta.id, meta.origin.toHex(), meta.ts, meta.hash, clip.mime, clip.w.toLong(), clip.h.toLong())
    }
}

/** Validate an incoming item. Returns its id (for the ack) and the parsed clip or the refusal. */
internal fun parseItem(message: Message, payload: ByteArray): Pair<String, Result<RemoteClip>> {
    fun refuse(reason: String): Result<RemoteClip> = Result.failure(IllegalArgumentException(reason))
    return when (message) {
        is Message.Text -> message.id to run {
            val origin = runCatching { DeviceId.fromHex(message.origin) }.getOrNull() ?: return@run refuse("origin")
            when {
                payload.size > MAX_TEXT -> refuse("limit")
                sha256Hex(payload) != message.hash -> refuse("hash")
                else -> {
                    val text = runCatching {
                        Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(payload)).toString()
                    }.getOrNull() ?: return@run refuse("utf8")
                    Result.success(RemoteClip(ItemMeta(message.id, origin, message.ts, message.hash), Clip.Text(text)))
                }
            }
        }
        is Message.Image -> message.id to run {
            val origin = runCatching { DeviceId.fromHex(message.origin) }.getOrNull() ?: return@run refuse("origin")
            when {
                payload.size > MAX_IMAGE -> refuse("limit")
                sha256Hex(payload) != message.hash -> refuse("hash")
                message.mime !in IMAGE_MIMES -> refuse("mime")
                else -> Result.success(
                    RemoteClip(
                        ItemMeta(message.id, origin, message.ts, message.hash),
                        Clip.Image(message.mime, message.w.toInt(), message.h.toInt(), payload),
                    ),
                )
            }
        }
        else -> "" to refuse("type")
    }
}
