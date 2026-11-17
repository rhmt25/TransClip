package io.transclip.proto

import org.junit.jupiter.api.Assumptions.assumeTrue
import java.io.File
import java.net.Socket
import java.util.Random
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

/**
 * Talks to the real Rust implementation over a TCP socket.
 *
 * Needs the `echo_peer` example built from `windows/crates/proto`; point the environment
 * variable `TRANSCLIP_ECHO_PEER` at the executable. The test is skipped when it is not set.
 */
class LiveInteropTest {
    private val exe: String? = System.getenv("TRANSCLIP_ECHO_PEER")

    private fun start(gk: GroupKey): Pair<Process, Int> {
        val process = ProcessBuilder(exe, gk.bytes.toHex()).redirectErrorStream(false).start()
        val line = process.inputStream.bufferedReader().readLine() ?: error("echo_peer printed nothing")
        check(line.startsWith("PORT ")) { "unexpected output: $line" }
        return process to line.removePrefix("PORT ").trim().toInt()
    }

    @Test
    fun handshakeAndEchoWithRust() {
        assumeTrue(exe != null && File(exe).exists(), "TRANSCLIP_ECHO_PEER not set")
        val gk = GroupKey(ByteArray(32) { (it * 7 + 1).toByte() })
        val (process, port) = start(gk)
        try {
            Socket("127.0.0.1", port).use { socket ->
                socket.tcpNoDelay = true
                socket.soTimeout = 10_000
                val ch = initiate(socket.getInputStream(), socket.getOutputStream(), gk)
                val rnd = Random(7)
                val dev = "ab".repeat(16)
                val hello = Message.Hello(1, dev, "Kotlin", "android", "0.1.0", listOf("text"), "control")
                ch.output.writeFrame(hello)
                assertEquals(hello, ch.input.readFrame().message)
                repeat(100) { i ->
                    val payload = ByteArray(rnd.nextInt(5000)).also { rnd.nextBytes(it) }
                    val msg = Message.Text(newUuidV4(), dev, i.toLong(), "cd".repeat(32))
                    ch.output.writeFrame(msg, payload)
                    val back = ch.input.readFrame()
                    assertEquals(msg, back.message)
                    assertContentEquals(payload, back.payload, "frame $i")
                }
                // A payload spanning many 64 KiB records.
                val big = ByteArray(5 * 1024 * 1024).also { rnd.nextBytes(it) }
                val image = Message.Image(newUuidV4(), dev, 1, "cd".repeat(32), "image/png", 1, 1)
                val writer = Thread { ch.output.writeFrame(image, big) }
                writer.start()
                val back = ch.input.readFrame()
                writer.join()
                assertEquals(image, back.message)
                assertContentEquals(big, back.payload)
            }
        } finally {
            process.destroy()
        }
    }

    @Test
    fun wrongGroupIsRejectedByRust() {
        assumeTrue(exe != null && File(exe).exists(), "TRANSCLIP_ECHO_PEER not set")
        val (process, port) = start(GroupKey(ByteArray(32) { 1 }))
        try {
            Socket("127.0.0.1", port).use { socket ->
                socket.soTimeout = 10_000
                assertFailsWith<ProtoException.WrongGroup> {
                    initiate(socket.getInputStream(), socket.getOutputStream(), GroupKey(ByteArray(32) { 2 }))
                }
            }
        } finally {
            process.destroy()
        }
    }
}
