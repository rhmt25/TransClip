package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.Message
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import java.net.InetAddress
import java.net.InetSocketAddress
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PeerManagerTest {
    private val loop = InetAddress.getLoopbackAddress()
    private val probe = Message.Pull("00000000-0000-4000-8000-000000000000")

    private class Node(val mgr: PeerManager, val seen: Channel<PeerSeen>, val scope: CoroutineScope)

    private fun config(id: Int, key: Int = 7, port: Int = 0, fallback: Long = 3_600_000) = ManagerConfig(
        local = localInfo(id),
        gk = groupKey(key),
        listenPort = port,
        backoffMinMs = 50,
        backoffMaxMs = 200,
        fallbackAfterMs = fallback,
    )

    private fun node(cfg: ManagerConfig): Node {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val seen = Channel<PeerSeen>(16)
        val mgr = PeerManager(cfg, scope, seen)
        mgr.start()
        return Node(mgr, seen, scope)
    }

    private fun seenOf(id: Int, port: Int, addr: InetSocketAddress = InetSocketAddress(loop, port)) =
        PeerSeen(deviceId(id), addr, Platform.WINDOWS, "dev$id", Via.BEACON)

    private suspend fun expectOnline(n: Node, id: Int) {
        while (true) {
            val e = within(5_000) { n.mgr.events.receive() }
            if (e is ManagerEvent.Online) {
                assertEquals(deviceId(id), e.info.id)
                return
            }
        }
    }

    private suspend fun expectOffline(n: Node, id: Int) {
        while (true) {
            val e = within(5_000) { n.mgr.events.receive() }
            if (e is ManagerEvent.Offline) {
                assertEquals(deviceId(id), e.id)
                return
            }
        }
    }

    private suspend fun expectProbe(n: Node) {
        while (true) {
            val e = within(5_000) { n.mgr.events.receive() }
            if (e is ManagerEvent.FrameIn && e.frame.message is Message.Pull) return
        }
    }

    @Test
    fun twoEnginesLinkExactlyOnceAndExchangeMessages() = runBlocking {
        val a = node(config(1))
        val b = node(config(2))
        a.seen.send(seenOf(2, b.mgr.localPort))
        b.seen.send(seenOf(1, a.mgr.localPort))
        expectOnline(a, 2)
        expectOnline(b, 1)
        a.mgr.send(deviceId(2), probe)
        expectProbe(b)
        b.mgr.send(deviceId(1), probe)
        expectProbe(a)
        assertEquals(listOf(deviceId(2)), a.mgr.onlinePeers())
        assertEquals(listOf(deviceId(1)), b.mgr.onlinePeers())
        val st = a.mgr.status.value
        assertEquals(1, st.size)
        assertEquals(PeerState.ONLINE, st[0].state)
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun peerGoesOfflineAndReconnects() = runBlocking {
        val a = node(config(1))
        var b = node(config(2))
        val bPort = b.mgr.localPort
        a.seen.send(seenOf(2, bPort))
        expectOnline(a, 2)
        b.mgr.stop()
        expectOffline(a, 2)
        assertEquals(PeerState.OFFLINE, a.mgr.status.value[0].state)

        // Bring B back on the same port; A keeps retrying with backoff.
        var revived: Node? = null
        repeat(50) {
            if (revived == null) {
                revived = runCatching { node(config(2, port = bPort)) }.getOrNull()
                if (revived == null) delay(50)
            }
        }
        b = revived!!
        expectOnline(a, 2)
        expectOnline(b, 1)
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun simultaneousDialsLeaveASingleLink() = runBlocking {
        val a = node(config(1, fallback = 0)) // both sides dial at once
        val b = node(config(2, fallback = 0))
        a.seen.send(seenOf(2, b.mgr.localPort))
        b.seen.send(seenOf(1, a.mgr.localPort))
        expectOnline(a, 2)
        expectOnline(b, 1)
        delay(500)
        a.mgr.send(deviceId(2), probe)
        expectProbe(b)
        b.mgr.send(deviceId(1), probe)
        expectProbe(a)
        for (n in listOf(a, b)) {
            val st = n.mgr.status.value
            assertEquals(1, st.size)
            assertEquals(PeerState.ONLINE, st[0].state)
        }
        // No Offline flapping while the duplicate was settled.
        assertTrue(a.mgr.events.tryReceive().getOrNull() !is ManagerEvent.Offline)
        assertTrue(b.mgr.events.tryReceive().getOrNull() !is ManagerEvent.Offline)
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun peerBeyondTheLimitIsRejected() = runBlocking {
        val a = node(config(1).copy(maxPeers = 2))
        val dead = InetSocketAddress(loop, 9)
        for (id in 2..4) a.seen.send(seenOf(id, 9, dead))
        while (true) {
            val e = within(5_000) { a.mgr.events.receive() }
            if (e is ManagerEvent.Rejected) {
                assertEquals(deviceId(4), e.id)
                break
            }
        }
        assertEquals(2, a.mgr.status.value.size)
        a.mgr.stop()
    }

    @Test
    fun wrongGroupKeyNeverLinks() = runBlocking {
        val a = node(config(1, key = 7))
        val b = node(config(2, key = 8))
        a.seen.send(seenOf(2, b.mgr.localPort))
        assertNull(withTimeoutOrNull(700) { a.mgr.events.receive() })
        assertTrue(a.mgr.onlinePeers().isEmpty())
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun largerIdWaitsForTheFallbackBeforeDialing() = runBlocking {
        val b = node(config(2)) // larger id, fallback far away
        val a = node(config(1))
        b.seen.send(seenOf(1, a.mgr.localPort))
        assertNull(withTimeoutOrNull(500) { b.mgr.events.receive() })
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun backoffDoublesToTheCap() {
        var d = 2_000L
        val seq = (1..6).map { d.also { d = nextBackoff(d, 30_000) } }
        assertEquals(listOf(2_000L, 4_000L, 8_000L, 16_000L, 30_000L, 30_000L), seq)
    }

    @Test
    fun deviceIdOrderingDecidesWhoDials() {
        assertIs<Comparable<DeviceId>>(deviceId(1))
        assertTrue(deviceId(1) < deviceId(2))
    }
}
