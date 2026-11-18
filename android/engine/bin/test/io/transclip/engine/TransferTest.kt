package io.transclip.engine

import io.transclip.proto.EntryKind
import io.transclip.proto.Message
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import java.io.File
import java.net.InetAddress
import java.net.InetSocketAddress
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class TransferTest {
    private val loop = InetAddress.getLoopbackAddress()

    private class Node(
        val id: Int,
        val mgr: PeerManager,
        val tm: TransferManager,
        val seen: Channel<PeerSeen>,
        val pump: kotlinx.coroutines.Job,
        val scope: CoroutineScope,
    )

    private fun tmp(tag: String): File = Files.createTempDirectory("tc-$tag-").toFile()

    private fun node(id: Int, dest: File, port: Int = 0, cfg: TransferConfig = TransferConfig()): Node {
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
        val tm = TransferManager(mgr, FsStore(dest), scope, cfg)
        val pump = scope.launch { for (ev in mgr.events) tm.handleEvent(ev) }
        return Node(id, mgr, tm, seen, pump, scope)
    }

    private suspend fun introduce(a: Node, b: Node) {
        a.seen.send(PeerSeen(deviceId(b.id), InetSocketAddress(loop, b.mgr.localPort), Platform.WINDOWS, "b", Via.BEACON))
    }

    private suspend fun waitOnline(n: Node) {
        repeat(100) {
            if (n.mgr.onlinePeers().isNotEmpty()) return
            delay(50)
        }
        error("peer did not come online")
    }

    private fun pattern(len: Int) = ByteArray(len) { ((it * 31) % 251).toByte() }

    private fun walk(d: File): List<File> = d.listFiles().orEmpty().flatMap { listOf(it) + if (it.isDirectory) walk(it) else emptyList() }

    private fun assertSameTree(a: File, b: File) {
        val l = walk(a).map { it.relativeTo(a).path }.sorted()
        val r = walk(b).map { it.relativeTo(b).path }.sorted()
        assertEquals(l, r)
        for (rel in l) {
            val fa = File(a, rel)
            if (fa.isFile) assertContentEquals(fa.readBytes(), File(b, rel).readBytes(), rel)
        }
    }

    @Test
    fun threeLevelFolderArrivesIdentical() = runBlocking {
        val src = tmp("src")
        val dst = tmp("dst")
        val a = node(1, src)
        val b = node(2, dst)
        introduce(a, b)
        waitOnline(a)
        waitOnline(b)
        val root = File(src, "Foto liburan")
        File(root, "a/b/c").mkdirs()
        File(root, "kosong").mkdirs()
        File(root, "a/b/c/日本 😀.bin").writeBytes(pattern(200_000))
        File(root, "a/dua.txt").writeText("dua")
        File(root, "nol.dat").writeBytes(ByteArray(0))
        File(root, "top.bin").writeBytes(pattern(1_000_000))
        val batch = batchesFromPaths(listOf(root)).single()
        val report = a.tm.sendBatch(deviceId(2), batch)
        assertEquals(8, report.ok, "$report")
        assertEquals(0, report.failed)
        assertSameTree(root, File(dst, "Foto liburan"))
        assertTrue(File(dst, "Foto liburan/kosong").isDirectory)
        assertTrue(walk(dst).none { it.name.endsWith(PART_SUFFIX) })
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun looseFilesLandInTheRootAndConflictsGetACounter() = runBlocking {
        val src = tmp("src")
        val dst = tmp("dst")
        val a = node(1, src)
        val b = node(2, dst)
        introduce(a, b)
        waitOnline(a)
        waitOnline(b)
        val f = File(src, "catatan.txt").apply { writeText("isi") }
        repeat(2) {
            val r = a.tm.sendBatch(deviceId(2), batchesFromPaths(listOf(f)).single())
            assertEquals(1, r.ok)
        }
        assertEquals("isi", File(dst, "catatan.txt").readText())
        assertEquals("isi", File(dst, "catatan (1).txt").readText())
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun hostilePathsAreRejected() = runBlocking {
        val dst = tmp("dst")
        val b = node(2, dst)
        val evilScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val seen = Channel<PeerSeen>(4)
        val evil = PeerManager(ManagerConfig(localInfo(1), groupKey(), 0, fallbackAfterMs = 3_600_000), evilScope, seen)
        evil.start()
        seen.send(PeerSeen(deviceId(2), InetSocketAddress(loop, b.mgr.localPort), Platform.WINDOWS, "b", Via.BEACON))
        repeat(100) { if (evil.onlinePeers().isEmpty()) delay(50) }
        for (bad in listOf("../x", "/etc/passwd", "a//b", "CON", "a\\b", "a/./b", "x.")) {
            evil.send(
                deviceId(2),
                Message.FileOffer(
                    "00000000-0000-4000-8000-%012d".format(bad.length),
                    "11111111-1111-4111-8111-111111111111",
                    "",
                    1,
                    1,
                    EntryKind.FILE,
                    bad,
                    1,
                    "0".repeat(64),
                    0,
                ),
            )
            val reply = within(3_000) {
                var m: Message
                do {
                    val ev = evil.events.receive()
                    m = (ev as? ManagerEvent.FrameIn)?.frame?.message ?: Message.Ping
                } while (m is Message.Ping)
                m
            }
            assertTrue(reply is Message.FileReject && reply.reason == "path", "$bad -> $reply")
        }
        assertEquals(0, dst.list()?.size ?: 0, "nothing may be written")
        evil.stop()
        b.mgr.stop()
    }

    @Test
    fun disabledAndOversizedFilesAreRefused() = runBlocking {
        for (cfg in listOf(TransferConfig(receiveFiles = false), TransferConfig(maxFileBytes = 100))) {
            val src = tmp("src")
            val dst = tmp("dst")
            val a = node(1, src)
            val b = node(2, dst, cfg = cfg)
            introduce(a, b)
            waitOnline(a)
            waitOnline(b)
            val f = File(src, "x.bin").apply { writeBytes(pattern(1000)) }
            val r = a.tm.sendBatch(deviceId(2), batchesFromPaths(listOf(f)).single())
            assertEquals(0, r.ok)
            assertEquals(1, r.failed)
            assertTrue(walk(dst).isEmpty())
            a.mgr.stop()
            b.mgr.stop()
        }
    }

    @Test
    fun interruptedTransferResumesFromThePartFile() = runBlocking {
        val src = tmp("src")
        val dst = tmp("dst")
        val a = node(1, src, cfg = TransferConfig(chunkDelayMs = 2, reconnectWaitMs = 20_000))
        var b = node(2, dst)
        val bPort = b.mgr.localPort
        introduce(a, b)
        waitOnline(a)
        val data = pattern(24 * 1024 * 1024)
        val f = File(src, "besar.bin").apply { writeBytes(data) }
        val batch = batchesFromPaths(listOf(f)).single()
        val sending = async { a.tm.sendBatch(deviceId(2), batch) }

        val part = File(dst, "besar.bin$PART_SUFFIX")
        repeat(200) { if (part.length() <= 2L * 1024 * 1024) delay(20) }
        val partial = part.length()
        assertTrue(partial > 0 && partial < data.size, "part is $partial")
        b.pump.cancel()
        b.mgr.stop()

        var revived: Node? = null
        repeat(50) {
            if (revived == null) {
                revived = runCatching { node(2, dst, port = bPort) }.getOrNull()
                if (revived == null) delay(50)
            }
        }
        b = revived!!
        val report = within(60_000) { sending.await() }
        assertEquals(1, report.ok, "$report")
        assertContentEquals(data, File(dst, "besar.bin").readBytes())
        assertFalse(part.exists())
        a.mgr.stop()
        b.mgr.stop()
    }

    @Test
    fun cancellingRemovesThePartAndLeavesNoFile() = runBlocking {
        val src = tmp("src")
        val dst = tmp("dst")
        val a = node(1, src, cfg = TransferConfig(chunkDelayMs = 2))
        val b = node(2, dst)
        introduce(a, b)
        waitOnline(a)
        waitOnline(b)
        val f = File(src, "lama.bin").apply { writeBytes(pattern(16 * 1024 * 1024)) }
        val sending = async { a.tm.sendBatch(deviceId(2), batchesFromPaths(listOf(f)).single()) }
        val started = within { var e: TransferEvent; do { e = a.tm.events.receive() } while (e !is TransferEvent.Started); e }
        delay(200)
        assertIs<TransferEvent.Started>(started)
        a.tm.cancelBatch(started.batchId)
        val report = within { sending.await() }
        assertEquals(0, report.ok)
        assertEquals(1, report.failed)
        delay(300)
        assertTrue(walk(dst).isEmpty(), "left over: ${walk(dst)}")
        a.mgr.stop()
        b.mgr.stop()
    }
}
