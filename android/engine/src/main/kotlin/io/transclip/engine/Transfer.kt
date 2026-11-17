package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.EntryKind
import io.transclip.proto.Message
import io.transclip.proto.PathException
import io.transclip.proto.PathRules
import io.transclip.proto.ProtoException
import io.transclip.proto.newUuidV4
import io.transclip.proto.toHex
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File
import java.io.RandomAccessFile
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap

const val CHUNK = 64 * 1024
const val MAX_PARALLEL = 3
const val PART_SUFFIX = ".transclip-part"

/** A file being received. Dropping it without finish() or abort() keeps the part for resuming. */
interface PartWriter {
    val length: Long
    fun hashExisting(md: MessageDigest)
    fun reset()
    fun write(data: ByteArray, off: Int, len: Int)

    /** Move to the final name (adding " (n)" on conflict). Returns the final file name. */
    fun finish(mtimeSecs: Long): String
    fun abort()
}

/** Receiver-side storage; the platform decides where files go (PLAN section 4.5). */
interface IncomingStore {
    /** Returns the (possibly renamed) folder of the batch; empty for loose files. */
    fun beginBatch(batchName: String): String
    fun makeDir(root: String, rel: List<String>)
    fun openPart(root: String, rel: List<String>): PartWriter
    fun freeBytes(): Long?
}

/** A plain folder on a local disk. */
class FsStore(private val dest: File) : IncomingStore {
    private fun dirOf(root: String) = if (root.isEmpty()) dest else File(dest, root)

    override fun beginBatch(batchName: String): String {
        dest.mkdirs()
        if (batchName.isEmpty()) return ""
        val name = io.transclip.proto.PathRules.uniqueName(batchName) { existsCi(dest, it) }
        File(dest, name).mkdirs()
        return name
    }

    override fun makeDir(root: String, rel: List<String>) {
        val d = rel.fold(dirOf(root)) { f, s -> File(f, s) }
        if (!d.isDirectory && !d.mkdirs()) throw java.io.IOException("cannot create folder")
    }

    override fun openPart(root: String, rel: List<String>): PartWriter {
        val parent = rel.dropLast(1).fold(dirOf(root)) { f, s -> File(f, s) }
        parent.mkdirs()
        return FsPart(parent, rel.last())
    }

    override fun freeBytes(): Long? = dest.takeIf { it.exists() }?.usableSpace

    private class FsPart(private val dir: File, private val name: String) : PartWriter {
        private val part = File(dir, name + PART_SUFFIX)
        private val file = RandomAccessFile(part, "rw").also { it.seek(it.length()) }
        override var length: Long = file.length()
            private set

        override fun hashExisting(md: MessageDigest) {
            file.seek(0)
            val buf = ByteArray(256 * 1024)
            while (true) {
                val n = file.read(buf)
                if (n <= 0) break
                md.update(buf, 0, n)
            }
            file.seek(file.length())
        }

        override fun reset() {
            file.setLength(0)
            file.seek(0)
            length = 0
        }

        override fun write(data: ByteArray, off: Int, len: Int) {
            file.write(data, off, len)
            length += len
        }

        override fun finish(mtimeSecs: Long): String {
            file.fd.sync()
            file.close()
            val finalName = PathRules.uniqueName(name) { existsCi(dir, it) }
            val target = File(dir, finalName)
            if (!part.renameTo(target)) throw java.io.IOException("cannot rename part")
            if (mtimeSecs > 0) target.setLastModified(mtimeSecs * 1000)
            return finalName
        }

        override fun abort() {
            runCatching { file.close() }
            part.delete()
        }
    }

    private companion object {
        fun existsCi(dir: File, name: String) =
            File(dir, name).exists() || dir.list()?.any { it.equals(name, ignoreCase = true) } == true
    }
}

/** An entry to send: [rel] is the path the receiver sees, [path] the local file. */
class EntrySource(val rel: String, val kind: EntryKind, val path: File?)

class Batch(val name: String, val entries: List<EntrySource>)

data class BatchReport(val batchId: String, val ok: Int, val failed: Int)

data class TransferConfig(
    val receiveFiles: Boolean = true,
    val maxFileBytes: Long = 0,
    val replyTimeoutMs: Long = 30_000,
    val reconnectWaitMs: Long = 60_000,
    val maxReconnects: Int = 5,
    /** Pause after every chunk; tests use it to make a transfer slow enough to interrupt. */
    val chunkDelayMs: Long = 0,
)

sealed interface TransferEvent {
    data class Started(
        val xferId: String,
        val batchId: String,
        val peer: DeviceId,
        val name: String,
        val bytes: Long,
        val offset: Long,
        val incoming: Boolean,
    ) : TransferEvent

    data class Progress(val xferId: String, val done: Long, val total: Long) : TransferEvent
    data class Finished(val xferId: String, val peer: DeviceId, val ok: Boolean, val reason: String?, val incoming: Boolean) : TransferEvent
    data class BatchFinished(val batchId: String, val peer: DeviceId, val ok: Int, val failed: Int, val incoming: Boolean) : TransferEvent
}

class TransferException(message: String) : Exception(message)

private sealed interface OutMsg {
    data class Accept(val offset: Long) : OutMsg
    data class Reject(val reason: String) : OutMsg
    data class Result(val ok: Boolean, val reason: String?) : OutMsg
    data class Cancel(val reason: String) : OutMsg
    data object Disconnected : OutMsg
}

private sealed interface Fail {
    data object Disconnected : Fail
    data object Hash : Fail
    data class Final(val reason: String) : Fail
}

private class FailException(val fail: Fail) : Exception()

private class Incoming(
    val peer: DeviceId,
    val batchId: String,
    val name: String,
    val bytes: Long,
    val sha256: String,
    val mtime: Long,
    val writer: PartWriter,
    val md: MessageDigest,
) {
    var received: Long = writer.length
    var streaming = false
    var lastProgress: Long = received
}

private class BatchState(val root: String, val count: Long) {
    var ok = 0
    var failed = 0
}

private class Prepared(val rel: String, val kind: EntryKind, val path: File?, val bytes: Long, val sha256: String, val mtime: Long)

private fun sha256Of(file: File): String {
    val md = MessageDigest.getInstance("SHA-256")
    file.inputStream().use { input ->
        val buf = ByteArray(256 * 1024)
        while (true) {
            val n = input.read(buf)
            if (n <= 0) break
            md.update(buf, 0, n)
        }
    }
    return md.digest().toHex()
}

/** File and folder transfer over the control link (PROTOCOL.md section 9.3, D-013). */
class TransferManager(
    private val mgr: PeerManager,
    private val store: IncomingStore,
    @Suppress("unused") private val scope: CoroutineScope,
    private val cfg: TransferConfig = TransferConfig(),
) {
    val events = Channel<TransferEvent>(Channel.UNLIMITED)

    private val lock = Any()
    private val incoming = HashMap<String, Incoming>()
    private val batches = HashMap<String, BatchState>()
    private val waiters = ConcurrentHashMap<String, Pair<DeviceId, Channel<OutMsg>>>()
    private val limits = ConcurrentHashMap<DeviceId, Semaphore>()
    private val cancelled = ConcurrentHashMap.newKeySet<String>()

    /** Cancel a batch we are sending. */
    fun cancelBatch(batchId: String) {
        cancelled.add(batchId)
    }

    /** Process a manager event. File frames are consumed (null); everything else is returned. */
    suspend fun handleEvent(event: ManagerEvent): ManagerEvent? {
        when (event) {
            is ManagerEvent.FrameIn -> {
                val from = event.from
                when (val m = event.frame.message) {
                    is Message.FileOffer -> onOffer(from, m)
                    is Message.FileStream -> onStream(from, m.xferId, m.offset)
                    is Message.FileChunk -> onChunk(from, m.xferId, m.offset, event.frame.payload)
                    is Message.FileEnd -> onEnd(from, m.xferId)
                    is Message.FileCancel -> {
                        notify(m.xferId, OutMsg.Cancel(m.reason))
                        onCancel(from, m.xferId, m.reason)
                    }
                    is Message.FileAccept -> notify(m.xferId, OutMsg.Accept(m.offset))
                    is Message.FileReject -> notify(m.xferId, OutMsg.Reject(m.reason))
                    is Message.FileResult -> notify(m.xferId, OutMsg.Result(m.ok, m.reason))
                    is Message.BatchDone -> {}
                    else -> return event
                }
                return null
            }
            is ManagerEvent.Offline -> {
                onOffline(event.id)
                return event
            }
            else -> return event
        }
    }

    private fun notify(xferId: String, msg: OutMsg) {
        waiters[xferId]?.second?.trySend(msg)
    }

    private fun onOffline(peer: DeviceId) {
        synchronized(lock) { incoming.entries.removeAll { it.value.peer == peer } }
        waiters.values.filter { it.first == peer }.forEach { it.second.trySend(OutMsg.Disconnected) }
    }

    // ------------------------------------------------------------------ receiving

    private suspend fun reply(to: DeviceId, m: Message) {
        runCatching { mgr.send(to, m) }
    }

    private suspend fun reject(from: DeviceId, xferId: String, reason: String) =
        reply(from, Message.FileReject(xferId, reason))

    private suspend fun entryDone(from: DeviceId, xferId: String, batchId: String, ok: Boolean, reason: String?) {
        reply(from, Message.FileResult(xferId, ok, reason))
        events.send(TransferEvent.Finished(xferId, from, ok, reason, incoming = true))
        val done = synchronized(lock) {
            val b = batches[batchId] ?: return@synchronized null
            if (ok) b.ok++ else b.failed++
            if (b.ok + b.failed >= b.count) {
                batches.remove(batchId)
                b.ok to b.failed
            } else {
                null
            }
        }
        if (done != null) {
            reply(from, Message.BatchDone(batchId, done.first.toLong(), done.second.toLong()))
            events.send(TransferEvent.BatchFinished(batchId, from, done.first, done.second, incoming = true))
        }
    }

    private suspend fun onOffer(from: DeviceId, o: Message.FileOffer) {
        if (!cfg.receiveFiles) return reject(from, o.xferId, "disabled")
        val rel = try {
            PathRules.validate(o.relativePath)
        } catch (e: PathException) {
            return reject(from, o.xferId, "path")
        }
        if (o.batchName.isNotEmpty() && runCatching { PathRules.validate(o.batchName).size == 1 }.getOrDefault(false).not()) {
            return reject(from, o.xferId, "path")
        }
        if (cfg.maxFileBytes > 0 && o.bytes > cfg.maxFileBytes) return reject(from, o.xferId, "quota")
        store.freeBytes()?.let { if (o.bytes > it) return reject(from, o.xferId, "disk_full") }

        val root = synchronized(lock) { batches[o.batchId]?.root } ?: try {
            store.beginBatch(o.batchName).also { r ->
                synchronized(lock) { batches[o.batchId] = BatchState(r, o.batchCount) }
            }
        } catch (e: Exception) {
            return reject(from, o.xferId, "disk_full")
        }

        when (o.kind) {
            EntryKind.DIR -> {
                val ok = runCatching { store.makeDir(root, rel) }.isSuccess
                events.send(TransferEvent.Started(o.xferId, o.batchId, from, o.relativePath, 0, 0, true))
                entryDone(from, o.xferId, o.batchId, ok, if (ok) null else "io")
            }
            EntryKind.FILE -> {
                val writer = try {
                    store.openPart(root, rel)
                } catch (e: Exception) {
                    return reject(from, o.xferId, "disk_full")
                }
                try {
                    if (writer.length > o.bytes) writer.reset()
                    val md = MessageDigest.getInstance("SHA-256")
                    val offset = writer.length
                    if (offset > 0) writer.hashExisting(md)
                    synchronized(lock) {
                        incoming[o.xferId] = Incoming(from, o.batchId, o.relativePath, o.bytes, o.sha256, o.mtime, writer, md)
                    }
                    events.send(TransferEvent.Started(o.xferId, o.batchId, from, o.relativePath, o.bytes, offset, true))
                    reply(from, Message.FileAccept(o.xferId, offset))
                } catch (e: Exception) {
                    writer.abort()
                    reject(from, o.xferId, "disk_full")
                }
            }
        }
    }

    private suspend fun onStream(from: DeviceId, xferId: String, offset: Long) {
        val ok = synchronized(lock) {
            val i = incoming[xferId]
            if (i != null && i.peer == from && i.received == offset) {
                i.streaming = true
                true
            } else {
                false
            }
        }
        if (!ok) abortIncoming(from, xferId, "protocol")
    }

    private suspend fun onChunk(from: DeviceId, xferId: String, offset: Long, data: ByteArray) {
        val problem = synchronized(lock) {
            val i = incoming[xferId]
            when {
                i == null || i.peer != from || !i.streaming || i.received != offset -> "protocol"
                i.received + data.size > i.bytes -> "limit"
                else -> try {
                    i.writer.write(data, 0, data.size)
                    i.md.update(data)
                    i.received += data.size
                    if (i.received - i.lastProgress >= 512 * 1024) {
                        i.lastProgress = i.received
                        events.trySend(TransferEvent.Progress(xferId, i.received, i.bytes))
                    }
                    null
                } catch (e: Exception) {
                    "io"
                }
            }
        }
        if (problem != null) abortIncoming(from, xferId, problem)
    }

    private suspend fun onEnd(from: DeviceId, xferId: String) {
        val i = synchronized(lock) { incoming[xferId]?.takeIf { it.peer == from }?.also { incoming.remove(xferId) } }
        if (i == null) {
            return reply(from, Message.FileResult(xferId, false, "protocol"))
        }
        if (i.received != i.bytes || i.md.digest().toHex() != i.sha256) {
            i.writer.abort()
            return entryDone(from, xferId, i.batchId, false, "hash")
        }
        events.trySend(TransferEvent.Progress(xferId, i.bytes, i.bytes))
        val ok = withContext(Dispatchers.IO) { runCatching { i.writer.finish(i.mtime) }.isSuccess }
        entryDone(from, xferId, i.batchId, ok, if (ok) null else "io")
    }

    private suspend fun onCancel(from: DeviceId, xferId: String, reason: String) {
        val i = synchronized(lock) { incoming[xferId]?.takeIf { it.peer == from }?.also { incoming.remove(xferId) } } ?: return
        i.writer.abort()
        events.send(TransferEvent.Finished(xferId, from, false, reason, incoming = true))
        val done = synchronized(lock) {
            val b = batches[i.batchId] ?: return@synchronized null
            b.failed++
            if (b.ok + b.failed >= b.count) {
                batches.remove(i.batchId)
                b.ok to b.failed
            } else {
                null
            }
        }
        if (done != null) events.send(TransferEvent.BatchFinished(i.batchId, from, done.first, done.second, incoming = true))
    }

    private suspend fun abortIncoming(from: DeviceId, xferId: String, reason: String) {
        val i = synchronized(lock) { incoming.remove(xferId) } ?: return
        i.writer.abort()
        reply(from, Message.FileCancel(xferId, reason))
        entryDone(from, xferId, i.batchId, false, reason)
    }

    /** The user cancelled a transfer we are receiving. */
    suspend fun cancelIncoming(xferId: String) {
        val peer = synchronized(lock) { incoming[xferId]?.peer } ?: return
        abortIncoming(peer, xferId, "cancelled")
    }

    // ------------------------------------------------------------------ sending

    /** Send a batch. Lost connections are retried, resuming where the receiver left off. */
    suspend fun sendBatch(peer: DeviceId, batch: Batch): BatchReport {
        if (peer !in mgr.onlinePeers()) throw TransferException("peer is offline")
        if (batch.name.isNotEmpty() && runCatching { PathRules.validate(batch.name).size == 1 }.getOrDefault(false).not()) {
            throw TransferException("invalid batch name")
        }
        return limits.getOrPut(peer) { Semaphore(MAX_PARALLEL) }.withPermit {
            val prepared = withContext(Dispatchers.IO) {
                batch.entries.map { e ->
                    runCatching { PathRules.validate(e.rel) }.getOrElse { throw TransferException("invalid path: ${e.rel}") }
                    if (e.kind == EntryKind.DIR) {
                        Prepared(e.rel, e.kind, null, 0, "", 0)
                    } else {
                        val f = e.path ?: throw TransferException("missing file for ${e.rel}")
                        Prepared(e.rel, e.kind, f, f.length(), sha256Of(f), f.lastModified() / 1000)
                    }
                }
            }
            val batchId = newUuidV4()
            val total = prepared.sumOf { it.bytes }
            var ok = 0
            var failed = 0
            for (entry in prepared) {
                if (batchId in cancelled) {
                    failed++
                    continue
                }
                if (sendEntry(peer, batchId, batch.name, prepared.size.toLong(), total, entry)) ok++ else failed++
            }
            cancelled.remove(batchId)
            events.send(TransferEvent.BatchFinished(batchId, peer, ok, failed, incoming = false))
            BatchReport(batchId, ok, failed)
        }
    }

    private suspend fun waitOnline(peer: DeviceId): Boolean {
        val end = System.nanoTime() + cfg.reconnectWaitMs * 1_000_000
        while (System.nanoTime() < end) {
            if (peer in mgr.onlinePeers()) return true
            delay(100)
        }
        return false
    }

    private suspend fun sendEntry(peer: DeviceId, batchId: String, batchName: String, count: Long, total: Long, e: Prepared): Boolean {
        val xferId = newUuidV4()
        val inbox = Channel<OutMsg>(Channel.UNLIMITED)
        waiters[xferId] = peer to inbox
        var reconnects = 0
        var hashRetries = 0
        var reason: String? = null
        while (true) {
            try {
                tryEntry(peer, batchId, batchName, count, total, e, xferId, inbox)
                break
            } catch (f: FailException) {
                when (val fail = f.fail) {
                    Fail.Disconnected -> {
                        reconnects++
                        if (reconnects > cfg.maxReconnects || !waitOnline(peer)) {
                            reason = "disconnected"
                            break
                        }
                    }
                    Fail.Hash -> {
                        hashRetries++
                        if (hashRetries > 1) {
                            reason = "hash"
                            break
                        }
                    }
                    is Fail.Final -> {
                        reason = fail.reason
                        break
                    }
                }
            }
        }
        waiters.remove(xferId)
        events.send(TransferEvent.Finished(xferId, peer, reason == null, reason, incoming = false))
        return reason == null
    }

    private suspend fun recv(inbox: Channel<OutMsg>): OutMsg {
        val m = withTimeoutOrNull(cfg.replyTimeoutMs) { inbox.receive() }
            ?: throw FailException(Fail.Final("timeout"))
        if (m is OutMsg.Disconnected) throw FailException(Fail.Disconnected)
        return m
    }

    private suspend fun tryEntry(
        peer: DeviceId,
        batchId: String,
        batchName: String,
        count: Long,
        total: Long,
        e: Prepared,
        xferId: String,
        inbox: Channel<OutMsg>,
    ) {
        while (inbox.tryReceive().isSuccess) { /* stale messages from an earlier attempt */ }
        try {
            mgr.send(
                peer,
                Message.FileOffer(xferId, batchId, batchName, count, total, e.kind, e.rel, e.bytes, e.sha256, e.mtime),
            )
        } catch (x: java.io.IOException) {
            throw FailException(Fail.Disconnected)
        }
        val first = recv(inbox)
        if (e.kind == EntryKind.DIR) {
            when (first) {
                is OutMsg.Result -> if (first.ok) return else throw FailException(Fail.Final(first.reason ?: "io"))
                is OutMsg.Reject -> throw FailException(Fail.Final(first.reason))
                else -> throw FailException(Fail.Final("protocol"))
            }
        }
        val offset = when (first) {
            is OutMsg.Accept -> first.offset
            is OutMsg.Reject -> throw FailException(Fail.Final(first.reason))
            is OutMsg.Cancel -> throw FailException(Fail.Final(first.reason))
            else -> throw FailException(Fail.Final("protocol"))
        }
        if (offset < 0 || offset > e.bytes) throw FailException(Fail.Final("protocol"))
        events.send(TransferEvent.Started(xferId, batchId, peer, e.rel, e.bytes, offset, incoming = false))
        streamFile(peer, batchId, e, xferId, offset, inbox)
        when (val r = recv(inbox)) {
            is OutMsg.Result -> when {
                r.ok -> return
                r.reason == "hash" -> throw FailException(Fail.Hash)
                else -> throw FailException(Fail.Final(r.reason ?: "io"))
            }
            is OutMsg.Reject -> throw FailException(Fail.Final(r.reason))
            is OutMsg.Cancel -> throw FailException(Fail.Final(r.reason))
            else -> throw FailException(Fail.Final("protocol"))
        }
    }

    private suspend fun streamFile(peer: DeviceId, batchId: String, e: Prepared, xferId: String, offset: Long, inbox: Channel<OutMsg>) {
        val link = mgr.handle(peer) ?: throw FailException(Fail.Disconnected)
        val file = e.path ?: throw FailException(Fail.Final("io"))
        suspend fun data(m: Message, payload: ByteArray = ByteArray(0)) {
            try {
                link.sendData(m, payload)
            } catch (x: ProtoException) {
                throw FailException(Fail.Disconnected)
            }
        }
        withContext(Dispatchers.IO) { RandomAccessFile(file, "r") }.use { raf ->
            raf.seek(offset)
            data(Message.FileStream(xferId, offset))
            var pos = offset
            var lastReport = offset
            val buf = ByteArray(CHUNK)
            while (pos < e.bytes) {
                while (true) {
                    when (val m = inbox.tryReceive().getOrNull() ?: break) {
                        OutMsg.Disconnected -> throw FailException(Fail.Disconnected)
                        is OutMsg.Cancel -> throw FailException(Fail.Final(m.reason))
                        is OutMsg.Reject -> throw FailException(Fail.Final(m.reason))
                        else -> {}
                    }
                }
                if (batchId in cancelled) {
                    runCatching { mgr.send(peer, Message.FileCancel(xferId, "cancelled")) }
                    throw FailException(Fail.Final("cancelled"))
                }
                val want = minOf((e.bytes - pos).toInt().coerceAtLeast(0), CHUNK)
                val n = withContext(Dispatchers.IO) { raf.read(buf, 0, want) }
                if (n <= 0) throw FailException(Fail.Final("io"))
                data(Message.FileChunk(xferId, pos), buf.copyOf(n))
                pos += n
                if (pos - lastReport >= 512 * 1024) {
                    lastReport = pos
                    events.trySend(TransferEvent.Progress(xferId, pos, e.bytes))
                }
                if (cfg.chunkDelayMs > 0) delay(cfg.chunkDelayMs)
            }
            data(Message.FileEnd(xferId))
        }
    }
}

/** Build batches from files and folders on disk: a folder becomes a named batch, files are loose. */
fun batchesFromPaths(paths: List<File>): List<Batch> {
    val loose = mutableListOf<EntrySource>()
    val out = mutableListOf<Batch>()
    for (p in paths) {
        PathRules.validate(p.name)
        if (p.isDirectory) {
            val entries = mutableListOf<EntrySource>()
            fun walk(dir: File, prefix: String) {
                for (c in dir.listFiles().orEmpty().sortedBy { it.name }) {
                    if (java.nio.file.Files.isSymbolicLink(c.toPath())) continue
                    val rel = if (prefix.isEmpty()) c.name else "$prefix/${c.name}"
                    if (c.isDirectory) {
                        entries += EntrySource(rel, EntryKind.DIR, null)
                        walk(c, rel)
                    } else if (c.isFile) {
                        entries += EntrySource(rel, EntryKind.FILE, c)
                    }
                }
            }
            walk(p, "")
            out += Batch(p.name, entries)
        } else if (p.isFile) {
            loose += EntrySource(p.name, EntryKind.FILE, p)
        }
    }
    if (loose.isNotEmpty()) out.add(0, Batch("", loose))
    return out
}
