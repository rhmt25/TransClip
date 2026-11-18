package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.Message
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicReference
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class BluetoothFailoverTest {
    private val lo = InetAddress.getLoopbackAddress()

    /** Stands in for Bluetooth: a loopback socket pair whose far end goes to [target]. */
    private class FakeBt(val target: AtomicReference<PeerManager?>) : BtDialer {
        override suspend fun connect(peer: DeviceId): RawConn {
            val t = target.get() ?: throw java.io.IOException("no bluetooth peer")
            val (a, b) = ServerSocket(0, 1, InetAddress.getLoopbackAddress()).use { server ->
                val near = Socket(InetAddress.getLoopbackAddress(), server.localPort)
                near to server.accept()
            }
            t.acceptConnection(SocketConn(b))
            return SocketConn(a)
        }
    }

    private class Node(val mgr: PeerManager, val seen: Channel<PeerSeen>)

    private fun node(scope: CoroutineScope, id: Int, bt: BtDialer? = null): Node {
        val seen = Channel<PeerSeen>(16)
        val m = PeerManager(
            ManagerConfig(
                localInfo(id),
                groupKey(),
                0,
                backoffMinMs = 50,
                backoffMaxMs = 200,
                fallbackAfterMs = 3_600_000,
                bt = bt,
                btDownAfterMs = 300,
                btCloseAfterMs = 600,
            ),
            scope,
            seen,
        )
        m.start()
        return Node(m, seen)
    }

    private suspend fun waitFor(what: String, cond: () -> Boolean) {
        repeat(200) {
            if (cond()) return
            delay(50)
        }
        throw AssertionError("timed out waiting for: $what")
    }

    private fun transport(n: Node, id: Int) = n.mgr.status.value.firstOrNull { it.id == deviceId(id) }?.transport

    private fun seen(id: Int, port: Int) =
        PeerSeen(deviceId(id), InetSocketAddress(lo, port), Platform.WINDOWS, "p$id", Via.BEACON)

    @Test
    fun bluetoothTakesOverWhenLanDiesAndHandsBackWhenLanReturns() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val target = AtomicReference<PeerManager?>(null)
        val a = node(scope, 1, FakeBt(target))
        a.mgr.setBtPriority(setOf(deviceId(2)))
        val b = node(scope, 2)
        a.seen.send(seen(2, b.mgr.localPort))
        waitFor("lan link") { transport(a, 2) == Transport.LAN }

        // The LAN peer disappears and returns as a device reachable only over Bluetooth.
        b.mgr.stop()
        waitFor("offline") { a.mgr.onlinePeers().isEmpty() }
        val b2 = node(scope, 2)
        target.set(b2.mgr)
        waitFor("bluetooth link") { transport(a, 2) == Transport.BLUETOOTH }
        a.mgr.send(deviceId(2), Message.Pull("00000000-0000-4000-8000-000000000000"))
        val got = within {
            var ok = false
            while (!ok) {
                val ev = b2.mgr.events.receive()
                if (ev is ManagerEvent.FrameIn && ev.frame.message is Message.Pull) ok = true
            }
            ok
        }
        assertTrue(got)

        // The LAN is reachable again: it takes over and Bluetooth is released without going offline.
        while (a.mgr.events.tryReceive().isSuccess) { /* drain */ }
        a.seen.send(seen(2, b2.mgr.localPort))
        waitFor("lan link back") { transport(a, 2) == Transport.LAN }
        delay(1_200)
        assertEquals(Transport.LAN, transport(a, 2))
        var offline = 0
        while (true) {
            val r = a.mgr.events.tryReceive()
            if (r.isFailure) break
            if (r.getOrNull() is ManagerEvent.Offline) offline++
        }
        assertEquals(0, offline, "no offline event while handing back to the LAN")
        assertEquals(listOf(deviceId(2)), a.mgr.onlinePeers())
        scope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
    }

    @Test
    fun bluetoothIsNotUsedWithoutPriority() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val target = AtomicReference<PeerManager?>(null)
        val a = node(scope, 1, FakeBt(target))
        val b = node(scope, 2)
        target.set(b.mgr)
        a.seen.send(seen(2, 1)) // nothing listens on port 1
        delay(900)
        assertTrue(a.mgr.onlinePeers().isEmpty())
        a.mgr.setBtPriority(setOf(deviceId(2)))
        waitFor("bluetooth link") { transport(a, 2) == Transport.BLUETOOTH }
        scope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
    }
}
