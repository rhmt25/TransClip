package io.transclip.engine

import io.transclip.proto.Message
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

class LinkTest {
    private val fast = LinkConfig(pingIntervalMs = 100, deadAfterMs = 600)

    private suspend fun next(link: Link): LinkEvent = within { link.events.receive() }

    private suspend fun nextFrame(link: Link): LinkEvent.FrameIn {
        while (true) {
            val e = next(link)
            if (e is LinkEvent.FrameIn) return e
            if (e is LinkEvent.Closed) error("closed: ${e.reason}")
        }
    }

    private fun scope() = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private suspend fun links(cfgA: LinkConfig = fast, cfgB: LinkConfig = fast): Pair<Link, Link> {
        val p = connectedPair()
        val s = scope()
        val la = Link(p.a, p.aChannel, localInfo(1), cfgA, s)
        val lb = Link(p.b, p.bChannel, localInfo(2), cfgB, s)
        la.start()
        lb.start()
        return la to lb
    }

    @Test
    fun helloIsExchangedAndFramesFlowBothWays() = runBlocking {
        val (a, b) = links(LinkConfig(), LinkConfig())
        val readyA = next(a)
        val readyB = next(b)
        assertIs<LinkEvent.Ready>(readyA)
        assertEquals("dev2", readyA.info.name)
        assertEquals("dev1", (readyB as LinkEvent.Ready).info.name)

        val pull = Message.Pull("00000000-0000-4000-8000-000000000000")
        a.sendControl(pull)
        assertEquals(pull, nextFrame(b).frame.message)
        b.sendControl(Message.Ack("00000000-0000-4000-8000-000000000000", true))
        assertIs<Message.Ack>(nextFrame(a).frame.message)
        a.abort()
        b.abort()
    }

    @Test
    fun keepAliveKeepsAnIdleLinkOpen() = runBlocking {
        val (a, b) = links()
        next(a)
        next(b)
        kotlinx.coroutines.delay(1_800) // three times the dead timeout
        assertTrue(!a.isClosed && !b.isClosed)
        a.abort()
        b.abort()
    }

    @Test
    fun silentPeerIsDeclaredDead() = runBlocking {
        val p = connectedPair()
        val s = scope()
        val live = Link(p.a, p.aChannel, localInfo(1), fast, s)
        live.start()
        // The other side completes the handshake but never answers or pings.
        assertEquals(CloseReason.Dead, within { drainUntilClosed(live) })
        p.b.close()
        s.cancel()
    }

    @Test
    fun byeClosesBothSidesCleanly() = runBlocking {
        val (a, b) = links()
        next(a)
        next(b)
        a.close("test")
        assertEquals(CloseReason.PeerBye("test"), within { drainUntilClosed(b) })
        assertEquals(CloseReason.Local, within { drainUntilClosed(a) })
    }

    private suspend fun drainUntilClosed(link: Link): CloseReason {
        while (true) {
            val e = link.events.receive()
            if (e is LinkEvent.Closed) return e.reason
        }
    }

    @Test
    fun transportLossIsReportedAsPeerClosed() = runBlocking {
        val (a, b) = links()
        next(a)
        next(b)
        a.abort()
        assertEquals(CloseReason.PeerClosed, within { drainUntilClosed(b) })
    }

    @Test
    fun controlMessagesOvertakeQueuedData() = runBlocking {
        val (a, b) = links(LinkConfig(), LinkConfig())
        next(a)
        next(b)
        val chunk = ByteArray(60_000)
        val id = "00000000-0000-4000-8000-000000000000"
        repeat(40) { a.sendData(Message.FileChunk(id, it * 60_000L), chunk) }
        a.sendControl(Message.Pull(id))
        var dataBefore = 0
        while (true) {
            val f = nextFrame(b).frame.message
            if (f is Message.Pull) break
            dataBefore++
        }
        assertTrue(dataBefore < 40, "control message arrived after $dataBefore of 40 data frames")
        a.abort()
        b.abort()
    }

    @Test
    fun closedIsTheLastEventAndSentOnce() = runBlocking {
        val (a, b) = links()
        next(a)
        next(b)
        b.abort()
        assertEquals(CloseReason.PeerClosed, within { drainUntilClosed(a) })
        // The channel is closed after Closed, so nothing else can follow.
        assertTrue(a.events.receiveCatching().isClosed)
    }
}
