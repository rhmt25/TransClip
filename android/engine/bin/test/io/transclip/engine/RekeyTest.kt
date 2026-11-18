package io.transclip.engine

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
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RekeyTest {
    @Test
    fun parseRejectsBadKeysAndOtherMessages() {
        assertNull(Rekey.parse(Message.Rekey("short", "g", deviceId(1).toHex(), 1)))
        assertNull(Rekey.parse(Message.RekeyAck(1)))
    }

    @Test
    fun rekeyReachesOnlyTheTargetedPeer() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        fun mk(id: Int): Pair<PeerManager, Channel<PeerSeen>> {
            val seen = Channel<PeerSeen>(16)
            val m = PeerManager(
                ManagerConfig(localInfo(id), groupKey(), 0, backoffMinMs = 50, backoffMaxMs = 200, fallbackAfterMs = 3_600_000),
                scope,
                seen,
            )
            m.start()
            return m to seen
        }
        val (a, aSeen) = mk(1)
        val (b, _) = mk(2)
        val (c, _) = mk(3)
        val lo = InetAddress.getLoopbackAddress()
        aSeen.send(PeerSeen(deviceId(2), InetSocketAddress(lo, b.localPort), Platform.WINDOWS, "b", Via.BEACON))
        aSeen.send(PeerSeen(deviceId(3), InetSocketAddress(lo, c.localPort), Platform.WINDOWS, "c", Via.BEACON))
        repeat(100) { if (a.onlinePeers().size < 2) delay(50) }
        assertEquals(2, a.onlinePeers().size)

        val newKey = groupKey(9)
        val sent = Rekey.broadcast(a, newKey, "home", deviceId(1), listOf(deviceId(2)))
        assertEquals(1, sent)
        val req = within {
            var found: RekeyRequest? = null
            while (found == null) {
                val ev = b.events.receive()
                if (ev is ManagerEvent.FrameIn) found = Rekey.parse(ev.frame.message)
            }
            found
        }
        assertNotNull(req)
        assertTrue(req.newGk.bytes.contentEquals(newKey.bytes))
        assertEquals(deviceId(1), req.by)
        scope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
    }
}
