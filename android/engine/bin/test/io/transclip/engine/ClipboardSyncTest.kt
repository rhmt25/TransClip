package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.Message
import io.transclip.proto.MAX_TEXT
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import java.net.InetAddress
import java.net.InetSocketAddress
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertTrue

class ClipboardSyncTest {
    private val loop = InetAddress.getLoopbackAddress()

    private class MemSink : ClipboardSink {
        val items = CopyOnWriteArrayList<RemoteClip>()
        override fun apply(clip: RemoteClip) {
            items += clip
        }

        fun texts() = items.mapNotNull { (it.clip as? Clip.Text)?.text }
    }

    private class Node(
        val id: Int,
        val mgr: PeerManager,
        val sync: ClipboardSync,
        val sink: MemSink,
        val seen: Channel<PeerSeen>,
        val pump: kotlinx.coroutines.Job,
    )

    private fun node(id: Int, port: Int = 0, runSync: Boolean = true, ackMs: Long = 5_000): Node {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val seen = Channel<PeerSeen>(16)
        val mgr = PeerManager(
            ManagerConfig(
                local = localInfo(id),
                gk = groupKey(),
                listenPort = port,
                backoffMinMs = 50,
                backoffMaxMs = 200,
                fallbackAfterMs = 3_600_000,
            ),
            scope,
            seen,
        )
        mgr.start()
        val sink = MemSink()
        val sync = ClipboardSync(mgr, sink, deviceId(id), scope, ClipConfig(ackMs))
        val pump = scope.launch {
            for (ev in mgr.events) if (runSync) sync.handleEvent(ev)
        }
        return Node(id, mgr, sync, sink, seen, pump)
    }

    private suspend fun introduce(a: Node, b: Node) {
        a.seen.send(PeerSeen(deviceId(b.id), InetSocketAddress(loop, b.mgr.localPort), Platform.WINDOWS, "b", Via.BEACON))
    }

    private suspend fun waitOnline(n: Node, count: Int) {
        repeat(100) {
            if (n.mgr.onlinePeers().size >= count) return
            delay(50)
        }
        error("peers did not come online")
    }

    private suspend fun nextEvent(n: Node) = within { n.sync.events.receive() }

    @Test
    fun textReachesBothPeersExactlyOnceWithoutEchoOrRelay() = runBlocking {
        val a = node(1)
        val b = node(2)
        val c = node(3)
        introduce(a, b)
        introduce(a, c)
        introduce(b, c)
        waitOnline(a, 2)
        waitOnline(b, 2)
        waitOnline(c, 2)

        assertIs<Submitted.Sent>(a.sync.submitLocal(Clip.Text("hello")))
        var delivered = 0
        while (delivered < 2) if (nextEvent(a) is ClipEvent.Delivered) delivered++
        delay(300)
        assertEquals(listOf("hello"), b.sink.texts())
        assertEquals(listOf("hello"), c.sink.texts())
        assertTrue(a.sink.texts().isEmpty(), "no echo to the sender")

        assertEquals(Submitted.Unchanged, a.sync.submitLocal(Clip.Text("hello")))
        b.sync.submitLocal(Clip.Text("reply"))
        delay(500)
        assertEquals(listOf("reply"), a.sink.texts())
        assertEquals(listOf("hello", "reply"), c.sink.texts())
        assertEquals(listOf("hello"), b.sink.texts())
        listOf(a, b, c).forEach { it.mgr.stop() }
    }

    @Test
    fun offlinePeerReceivesTheItemAfterItComesOnline() = runBlocking {
        val a = node(1)
        var b = node(2)
        val bPort = b.mgr.localPort
        introduce(a, b)
        waitOnline(a, 1)
        b.pump.cancel()
        b.mgr.stop()
        repeat(100) {
            if (a.mgr.onlinePeers().isNotEmpty()) delay(20)
        }

        a.sync.submitLocal(Clip.Text("while you were away"))
        assertIs<ClipEvent.Queued>(nextEvent(a))

        var revived: Node? = null
        repeat(50) {
            if (revived == null) {
                revived = runCatching { node(2, port = bPort) }.getOrNull()
                if (revived == null) delay(50)
            }
        }
        b = revived!!
        while (nextEvent(a) !is ClipEvent.Delivered) {
            // wait for delivery after the reconnect
        }
        assertEquals(listOf("while you were away"), b.sink.texts())
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun oversizedAndInvalidItemsAreRefused() = runBlocking {
        val a = node(1)
        val big = Clip.Text("x".repeat(MAX_TEXT + 1))
        assertEquals(ClipException.Kind.TOO_LARGE, assertFailsWith<ClipException> { a.sync.submitLocal(big) }.kind)
        val bad = Clip.Image("image/tiff", 1, 1, byteArrayOf(1))
        assertEquals(ClipException.Kind.BAD_MIME, assertFailsWith<ClipException> { a.sync.submitLocal(bad) }.kind)
        assertIs<Submitted.Sent>(a.sync.submitLocal(Clip.Text("x".repeat(MAX_TEXT))))
        a.mgr.stop()
    }

    @Test
    fun missingAckIsRetriedOnceThenReported() = runBlocking {
        val a = node(1, ackMs = 150)
        val b = node(2, runSync = false) // never acknowledges
        introduce(a, b)
        waitOnline(a, 1)
        a.sync.submitLocal(Clip.Text("hi"))
        val e = nextEvent(a)
        assertIs<ClipEvent.Failed>(e)
        assertEquals("timeout", e.reason)
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun imageRoundTripKeepsBytesExact() = runBlocking {
        val a = node(1)
        val b = node(2)
        introduce(a, b)
        waitOnline(a, 1)
        waitOnline(b, 1)
        val bytes = ByteArray(300_000) { (it % 251).toByte() }
        a.sync.submitLocal(Clip.Image("image/png", 10, 10, bytes))
        while (nextEvent(a) !is ClipEvent.Delivered) {
            // wait
        }
        assertEquals(1, b.sink.items.size)
        assertEquals(Clip.Image("image/png", 10, 10, bytes), b.sink.items[0].clip)
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun corruptedPayloadIsRefused() {
        val msg = Message.Text(
            "00000000-0000-4000-8000-000000000000",
            DeviceId(ByteArray(16) { 1 }).toHex(),
            1,
            sha256Hex("right".toByteArray()),
        )
        val (_, parsed) = parseItem(msg, "wrong".toByteArray())
        assertEquals("hash", parsed.exceptionOrNull()?.message)
    }

    @Test
    fun offlineQueueKeepsTheNewestFiveInOrder() {
        val q = OfflineQueue()
        val evicted = mutableListOf<String>()
        for (n in 0 until 8) {
            val meta = ItemMeta("id$n", deviceId(1), n.toLong(), "")
            q.push(QueuedItem(meta, Clip.Text("t$n")))?.let { evicted += it.meta.id }
        }
        assertEquals(listOf("id0", "id1", "id2"), evicted)
        assertEquals(listOf("id3", "id4", "id5", "id6", "id7"), q.drain().map { it.meta.id })
        assertEquals(0, q.size)
    }
}
