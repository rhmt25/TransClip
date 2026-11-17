package io.transclip.engine

import io.transclip.proto.Beacon
import io.transclip.proto.GroupId
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DiscoveryTest {
    private val loop = InetAddress.getLoopbackAddress()

    private fun freePort(): Int = DatagramSocket(0, loop).use { it.localPort }

    private fun cfg(id: Int, port: Int, target: Int, gid: GroupId, interval: Long = 50) = BeaconConfig(
        gid = gid,
        deviceId = deviceId(id),
        tcpPort = 40_000 + id,
        platform = Platform.ANDROID,
        name = "dev$id",
        listenPort = port,
        targets = listOf(InetSocketAddress(loop, target)),
        intervalMs = interval,
    )

    @Test
    fun twoInstancesFindEachOther() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val gid = GroupId(ByteArray(8) { 7 })
        val (pa, pb) = freePort() to freePort()
        val ra = Channel<PeerSeen>(8)
        val rb = Channel<PeerSeen>(8)
        val a = BeaconService(cfg(1, pa, pb, gid), ra, scope).also { it.start() }
        val b = BeaconService(cfg(2, pb, pa, gid), rb, scope).also { it.start() }
        val seenByA = within(3_000) { ra.receive() }
        val seenByB = within(3_000) { rb.receive() }
        assertEquals(deviceId(2), seenByA.deviceId)
        assertEquals(InetSocketAddress(loop, 40_002), seenByA.addr)
        assertEquals("dev2", seenByA.name)
        assertEquals(deviceId(1), seenByB.deviceId)
        a.stop()
        b.stop()
        scope.cancel()
    }

    @Test
    fun foreignGroupIsIgnored() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val (pa, pb) = freePort() to freePort()
        val ra = Channel<PeerSeen>(8)
        val a = BeaconService(cfg(1, pa, pb, GroupId(ByteArray(8) { 7 })), ra, scope).also { it.start() }
        val b = BeaconService(cfg(2, pb, pa, GroupId(ByteArray(8) { 8 })), Channel(8), scope).also { it.start() }
        assertNull(withTimeoutOrNull(400) { ra.receive() })
        a.stop()
        b.stop()
        scope.cancel()
    }

    @Test
    fun announceNowSendsBeforeTheInterval() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val gid = GroupId(ByteArray(8) { 7 })
        val (pa, pb) = freePort() to freePort()
        val rb = Channel<PeerSeen>(8)
        // The listener must exist before A's immediate first beacon, so start B first.
        val b = BeaconService(cfg(2, pb, pa, gid), rb, scope).also { it.start() }
        val a = BeaconService(cfg(1, pa, pb, gid, interval = 3_600_000), Channel(8), scope).also { it.start() }
        within { rb.receive() } // the immediate first beacon
        kotlinx.coroutines.delay(100)
        while (rb.tryReceive().isSuccess) {
            // drain
        }
        a.announceNow()
        assertNotNull(withTimeoutOrNull(2_000) { rb.receive() })
        a.stop()
        b.stop()
        scope.cancel()
    }

    @Test
    fun ownAndGarbageDatagramsAreRejected() {
        val gid = GroupId(ByteArray(8) { 7 })
        val me = deviceId(1)
        val own = Beacon(gid, me, 1, Platform.ANDROID, "").encode()
        assertNull(acceptBeacon(own, loop, gid, me))
        assertNull(acceptBeacon("garbage".toByteArray(), loop, gid, me))
        assertNull(acceptBeacon(ByteArray(0), loop, gid, me))
    }

    @Test
    fun longNamesAreClampedOnCharacterBoundary() {
        val clamped = clampName("日".repeat(30))
        assertTrue(clamped.toByteArray().size <= Beacon.MAX_NAME)
        assertTrue(clamped.all { it == '日' })
    }

    @Test
    fun addressCachePersistsAndToleratesDamage() {
        val dir = File(System.getProperty("java.io.tmpdir"), "tc-cache-${System.nanoTime()}").apply { mkdirs() }
        val f = File(dir, "peers.json")
        val c = AddressCache(f)
        assertEquals(0, c.size)
        val addr = InetSocketAddress(InetAddress.getByName("192.168.1.5"), 47820)
        c.record(deviceId(3), addr)
        assertEquals(addr, AddressCache(f).get(deviceId(3)))

        f.writeText("{not json")
        assertEquals(0, AddressCache(f).size)
        f.writeText("""{"peers":{"zz":"1.2.3.4:5","${deviceId(1).toHex()}":"nonsense","${deviceId(2).toHex()}":"10.0.0.1:9"}}""")
        val tolerant = AddressCache(f)
        assertEquals(1, tolerant.size)
        assertNotNull(tolerant.get(deviceId(2)))
        dir.deleteRecursively()
    }
}
