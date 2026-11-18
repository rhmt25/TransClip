package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.jupiter.api.Assumptions.assumeTrue
import java.io.File
import java.net.InetAddress
import java.net.InetSocketAddress
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals

/**
 * Sends and receives folders against the real Rust engine (`interop_peer` example).
 * Skipped unless `TRANSCLIP_INTEROP_PEER` points at the built executable.
 */
class LiveTransferInteropTest {
    private val exe: String? = System.getenv("TRANSCLIP_INTEROP_PEER")
    private val peerId = DeviceId(ByteArray(16) { 0x5a })

    private fun pattern(len: Int, seed: Int) = ByteArray(len) { ((it * 31 + seed) % 251).toByte() }

    private fun walk(d: File): List<File> = d.listFiles().orEmpty().flatMap { listOf(it) + if (it.isDirectory) walk(it) else emptyList() }

    private fun makeTree(root: File) {
        File(root, "a/b/c").mkdirs()
        File(root, "kosong").mkdirs()
        File(root, "a/b/c/日本 😀.bin").writeBytes(pattern(300_000, 1))
        File(root, "a/dua.txt").writeText("dua")
        File(root, "nol.dat").writeBytes(ByteArray(0))
        File(root, "top.bin").writeBytes(pattern(3_000_000, 2))
    }

    private fun assertSameTree(a: File, b: File) {
        val l = walk(a).map { it.relativeTo(a).path }.sorted()
        val r = walk(b).map { it.relativeTo(b).path }.sorted()
        assertEquals(l, r)
        for (rel in l) {
            val fa = File(a, rel)
            if (fa.isFile) assertContentEquals(fa.readBytes(), File(b, rel).readBytes(), rel)
        }
    }

    private fun start(recv: File, send: File?): Pair<Process, Int> {
        val args = mutableListOf(exe!!, groupKey().bytes.joinToString("") { "%02x".format(it) }, recv.path)
        if (send != null) args += send.path
        val p = ProcessBuilder(args).redirectErrorStream(false).start()
        val line = p.inputStream.bufferedReader().readLine() ?: error("interop_peer printed nothing")
        check(line.startsWith("PORT ")) { "unexpected output: $line" }
        return p to line.removePrefix("PORT ").trim().toInt()
    }

    private class Local(val mgr: PeerManager, val tm: TransferManager, val seen: Channel<PeerSeen>, val scope: CoroutineScope)

    private fun local(dest: File): Local {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val seen = Channel<PeerSeen>(4)
        val mgr = PeerManager(
            ManagerConfig(localInfo(1), groupKey(), 0, backoffMinMs = 50, backoffMaxMs = 200, fallbackAfterMs = 3_600_000),
            scope,
            seen,
        )
        mgr.start()
        val tm = TransferManager(mgr, FsStore(dest), scope, TransferConfig())
        scope.launch { for (ev in mgr.events) tm.handleEvent(ev) }
        return Local(mgr, tm, seen, scope)
    }

    private suspend fun connect(l: Local, port: Int) {
        l.seen.send(PeerSeen(peerId, InetSocketAddress(InetAddress.getLoopbackAddress(), port), Platform.WINDOWS, "rust", Via.BEACON))
        repeat(100) {
            if (l.mgr.onlinePeers().isNotEmpty()) return
            delay(50)
        }
        error("rust peer did not come online")
    }

    @Test
    fun kotlinSendsAFolderToRust() = runBlocking {
        assumeTrue(exe != null && File(exe).exists(), "TRANSCLIP_INTEROP_PEER not set")
        val src = Files.createTempDirectory("tc-k2r-src").toFile()
        val rustDir = Files.createTempDirectory("tc-k2r-rust").toFile()
        val root = File(src, "Foto liburan").also(::makeTree)
        val (process, port) = start(rustDir, null)
        val l = local(Files.createTempDirectory("tc-k2r-own").toFile())
        try {
            connect(l, port)
            val report = l.tm.sendBatch(peerId, batchesFromPaths(listOf(root)).single())
            assertEquals(0, report.failed, "$report")
            assertSameTree(root, File(rustDir, "Foto liburan"))
        } finally {
            l.mgr.stop()
            process.destroy()
        }
    }

    @Test
    fun rustSendsAFolderToKotlin() = runBlocking {
        assumeTrue(exe != null && File(exe).exists(), "TRANSCLIP_INTEROP_PEER not set")
        val src = Files.createTempDirectory("tc-r2k-src").toFile()
        val ownDir = Files.createTempDirectory("tc-r2k-own").toFile()
        val root = File(src, "Dari Rust").also(::makeTree)
        val (process, port) = start(Files.createTempDirectory("tc-r2k-rust").toFile(), root)
        val l = local(ownDir)
        try {
            connect(l, port)
            val target = File(ownDir, "Dari Rust")
            repeat(200) {
                if (walk(target).count { it.isFile } >= 4 && walk(ownDir).none { it.name.endsWith(PART_SUFFIX) }) return@repeat
                delay(100)
            }
            assertSameTree(root, target)
        } finally {
            l.mgr.stop()
            process.destroy()
        }
    }
}
