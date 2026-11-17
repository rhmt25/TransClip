package io.transclip.proto

import java.io.PipedInputStream
import java.io.PipedOutputStream
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

private const val UUID = "11111111-1111-4111-8111-111111111111"

private fun text(len: Int): Pair<Message, ByteArray> =
    Message.Text(UUID, "ab".repeat(16), 1, "cd".repeat(32)) to ByteArray(len) { (it % 251).toByte() }

/** A bidirectional in-memory link between two endpoints. */
private class Link {
    private val aToB = PipedInputStream(1 shl 21)
    private val bToA = PipedInputStream(1 shl 21)
    private val aOut = PipedOutputStream(aToB)
    private val bOut = PipedOutputStream(bToA)
    val aIn get() = bToA
    val aOutput get() = aOut
    val bIn get() = aToB
    val bOutput get() = bOut
}

class FrameTest {
    @Test
    fun encodeThenDecode() {
        val (m, p) = text(5)
        val frames = FrameDecoder().push(encodeFrame(m, p))
        assertEquals(1, frames.size)
        assertEquals(m, frames[0].message)
        assertContentEquals(p, frames[0].payload)
    }

    @Test
    fun decoderHandlesOneByteAtATime() {
        val (m, p) = text(300)
        val bytes = encodeFrame(m, p) + encodeFrame(Message.Ping)
        val dec = FrameDecoder()
        val got = bytes.flatMap { dec.push(byteArrayOf(it)) }
        assertEquals(2, got.size)
        assertEquals(300, got[0].payload.size)
        assertEquals(Message.Ping, got[1].message)
    }

    @Test
    fun limits() {
        assertFailsWith<ProtoException.Limit> { FrameDecoder().push(byteArrayOf(0, 0, 0, 0)) }
        assertFailsWith<ProtoException.Limit> { FrameDecoder().push(byteArrayOf(0, 1, 0, 1)) }
        assertEquals(0, FrameDecoder().push(byteArrayOf(0, 1, 0, 0)).size)
        val (m, _) = text(0)
        assertFailsWith<ProtoException.Limit> { encodeFrame(m, ByteArray(MAX_TEXT + 1)) }
        assertFailsWith<ProtoException.Limit> { encodeFrame(Message.Ping, ByteArray(1)) }
    }

    @Test
    fun unknownMessageTypeIsReportedAsUnknown() {
        val header = """{"t":"from_the_future","size":3,"x":1}""".toByteArray()
        val bytes = byteArrayOf(0, 0, 0, header.size.toByte()) + header + "abc".toByteArray()
        val f = FrameDecoder().push(bytes)[0]
        assertEquals(Message.Unknown("from_the_future"), f.message)
        assertContentEquals("abc".toByteArray(), f.payload)
    }

    @Test
    fun randomBytesNeverCrashTheDecoder() {
        val rnd = java.util.Random(42)
        repeat(5000) {
            val data = ByteArray(rnd.nextInt(512)).also { rnd.nextBytes(it) }
            try {
                FrameDecoder().push(data)
            } catch (e: ProtoException) {
                // Expected: malformed input must end in a protocol error, never anything else.
            }
        }
    }

    @Test
    fun everyMessageTypeSurvivesARoundTrip() {
        val dev = "ab".repeat(16)
        val digest = "cd".repeat(32)
        val samples = listOf(
            Message.Hs1(1, "00".repeat(8), "A".repeat(43), "B".repeat(22)),
            Message.Hs3("C".repeat(43)),
            Message.Hello(1, dev, "PC", "windows", "0.1.0", listOf("text", "image"), "control"),
            Message.Ping, Message.Pong,
            Message.Error("limit", "x", "r"), Message.Error("busy"),
            Message.Bye("app_exit"),
            Message.Image(UUID, dev, 2, digest, "image/png", 10, 20),
            Message.Announce(UUID, dev, 3, digest, "image", "image/png", 1, 2, 3),
            Message.Pull(UUID), Message.Ack(UUID, false, "busy"), Message.Ack(UUID, true),
            Message.FileOffer(UUID, UUID, "photos", 2, 10, EntryKind.FILE, "photos/a.jpg", 5, digest, 99),
            Message.FileOffer(UUID, UUID, "photos", 2, 10, EntryKind.DIR, "photos", 0, "", 99),
            Message.FileAccept(UUID, 7), Message.FileReject(UUID, "quota"), Message.FileStream(UUID, 0),
            Message.FileChunk(UUID, 64), Message.FileEnd(UUID), Message.FileResult(UUID, true),
            Message.FileCancel(UUID, "cancelled"), Message.BatchDone(UUID, 1, 0),
            Message.Rekey("A".repeat(43), "home", dev, 4), Message.RekeyAck(5),
        )
        for (m in samples) {
            val back = FrameDecoder().push(encodeFrame(m))
            assertEquals(m, back[0].message)
        }
    }

    @Test
    fun validationRejectsBadFields() {
        assertFailsWith<ProtoException.BadHeader> { Message.Pull("not-a-uuid").validate() }
        assertFailsWith<ProtoException.Version> { Message.Hello(2, "ab".repeat(16), "", "", "", emptyList(), "").validate() }
        val dirWithBytes = Message.FileOffer(UUID, UUID, "", 1, 0, EntryKind.DIR, "d", 1, "", 0)
        assertFailsWith<ProtoException.BadHeader> { dirWithBytes.validate() }
    }
}

class RecordTest {
    private val key = ByteArray(32) { 7 }

    @Test
    fun sealOpenRoundTripAndTamperDetection() {
        val ct = Record.seal(key, 0, "hello".toByteArray())
        assertEquals(5 + Record.TAG_LEN, ct.size)
        assertContentEquals("hello".toByteArray(), Record.open(key, 0, ct))
        for (i in ct.indices) {
            val bad = ct.copyOf().also { it[i] = (it[i].toInt() xor 1).toByte() }
            assertFailsWith<ProtoException.Auth>("byte $i") { Record.open(key, 0, bad) }
        }
        assertFailsWith<ProtoException.Auth> { Record.open(key, 1, ct) }
        assertFailsWith<ProtoException.Auth> { Record.open(ByteArray(32) { 8 }, 0, ct) }
    }

    @Test
    fun lengthAndCounterLimits() {
        assertFailsWith<ProtoException.Limit> { Record.seal(key, 0, ByteArray(0)) }
        assertFailsWith<ProtoException.Limit> { Record.seal(key, 0, ByteArray(Record.MAX_PLAINTEXT + 1)) }
        assertFailsWith<ProtoException.Limit> { Record.seal(key, Record.COUNTER_LIMIT, ByteArray(1)) }
        assertFailsWith<ProtoException.Limit> { Record.open(key, 0, ByteArray(Record.MIN_LEN - 1)) }
        assertFailsWith<ProtoException.Limit> { Record.open(key, 0, ByteArray(Record.MAX_LEN + 1)) }
    }

    @Test
    fun framesRoundTripOverASecureStream() {
        val link = Link()
        val out = SecureOutput(link.aOutput, key)
        val inp = SecureInput(link.bIn, key)
        for (len in listOf(0, 1, 100, 65_000, 65_536, 200_000)) {
            val (m, p) = text(len)
            out.writeFrame(m, p)
            val f = inp.readFrame()
            assertEquals(m, f.message)
            assertContentEquals(p, f.payload, "len $len")
        }
        assertEquals(out.recordsSent, inp.recordsReceived)
    }

    @Test
    fun tamperedRecordIsRejected() {
        val link = Link()
        val inp = SecureInput(link.bIn, key)
        val body = Record.seal(key, 0, "hello".toByteArray())
        body[0] = (body[0].toInt() xor 0x80).toByte()
        link.aOutput.write(byteArrayOf(0, 0, 0, body.size.toByte()))
        link.aOutput.write(body)
        assertFailsWith<ProtoException.Auth> { inp.readFrame() }
    }

    @Test
    fun truncatedStreamReportsClosed() {
        val link = Link()
        val inp = SecureInput(link.bIn, key)
        link.aOutput.write(byteArrayOf(0, 0, 0, 40, 1, 2, 3))
        link.aOutput.close()
        assertFailsWith<ProtoException.Closed> { inp.readFrame() }
    }
}

class HandshakeTest {
    private val pool = Executors.newFixedThreadPool(2)

    private fun <A, B> both(a: Callable<A>, b: Callable<B>): Pair<Result<A>, Result<B>> {
        val fa = pool.submit(a)
        val fb = pool.submit(b)
        fun <T> Callable<T>.await(f: java.util.concurrent.Future<T>): Result<T> =
            try {
                Result.success(f.get(20, TimeUnit.SECONDS))
            } catch (e: java.util.concurrent.ExecutionException) {
                Result.failure(e.cause ?: e)
            }
        return a.await(fa) to b.await(fb)
    }

    @Test
    fun bothSidesAgreeAndCanTalk() {
        val link = Link()
        val gk = GroupKey(ByteArray(32) { 1 })
        val (i, r) = both(
            Callable { initiate(link.aIn, link.aOutput, gk) },
            Callable { respond(link.bIn, link.bOutput, gk) },
        )
        val ic = i.getOrThrow()
        val rc = r.getOrThrow()
        ic.output.writeFrame(Message.Ping)
        assertEquals(Message.Ping, rc.input.readFrame().message)
        rc.output.writeFrame(Message.Pong)
        assertEquals(Message.Pong, ic.input.readFrame().message)
    }

    @Test
    fun differentGroupsFailWithWrongGroup() {
        val link = Link()
        val (i, r) = both(
            Callable { initiate(link.aIn, link.aOutput, GroupKey(ByteArray(32) { 1 })) },
            Callable { respond(link.bIn, link.bOutput, GroupKey(ByteArray(32) { 2 })) },
        )
        assertIs<ProtoException.WrongGroup>(i.exceptionOrNull())
        assertIs<ProtoException.WrongGroup>(r.exceptionOrNull())
    }

    @Test
    fun badMacFromInitiatorIsRejected() {
        val link = Link()
        val gk = GroupKey(ByteArray(32) { 3 })
        val (r, _) = both(
            Callable { respond(link.bIn, link.bOutput, gk) },
            Callable {
                writePlainFrame(
                    link.aOutput,
                    Message.Hs1(1, gk.groupId().toHex(), b64urlEncode(x25519Public(ByteArray(32) { 5 })), b64urlEncode(ByteArray(16) { 1 })),
                )
                readPlainFrame(link.aIn)
                writePlainFrame(link.aOutput, Message.Hs3(b64urlEncode(ByteArray(32))))
            },
        )
        assertIs<ProtoException.Auth>(r.exceptionOrNull())
    }

    @Test
    fun lowOrderPublicKeyIsRejected() {
        val link = Link()
        val gk = GroupKey(ByteArray(32) { 5 })
        val (r, _) = both(
            Callable { respond(link.bIn, link.bOutput, gk) },
            Callable {
                writePlainFrame(
                    link.aOutput,
                    Message.Hs1(1, gk.groupId().toHex(), b64urlEncode(ByteArray(32)), b64urlEncode(ByteArray(16) { 1 })),
                )
            },
        )
        assertIs<ProtoException.Auth>(r.exceptionOrNull())
    }

    @Test
    fun unsupportedVersionIsReported() {
        val link = Link()
        val gk = GroupKey(ByteArray(32) { 6 })
        val (r, reply) = both(
            Callable { respond(link.bIn, link.bOutput, gk) },
            Callable {
                writePlainFrame(
                    link.aOutput,
                    Message.Hs1(9, gk.groupId().toHex(), b64urlEncode(ByteArray(32) { 1 }), b64urlEncode(ByteArray(16) { 1 })),
                )
                readPlainFrame(link.aIn).message
            },
        )
        assertEquals(9L, (r.exceptionOrNull() as ProtoException.Version).version)
        assertEquals(Message.Error("version"), reply.getOrThrow())
    }

    @Test
    fun groupKeyBasics() {
        val a = GroupKey(ByteArray(32) { 1 })
        assertEquals(a.groupId(), GroupKey(ByteArray(32) { 1 }).groupId())
        assertFalse(a.groupId() == GroupKey(ByteArray(32) { 2 }).groupId())
        assertEquals("GroupKey(..)", a.toString())
        assertFalse(GroupKey.generate() == GroupKey.generate())
    }
}

class RulesTest {
    @Test
    fun seenCacheDetectsDuplicatesExpiresAndEvicts() {
        val c = SeenCache(3, 1000)
        assertTrue(c.insert("a", 0))
        assertFalse(c.insert("a", 1))
        assertTrue(c.contains("a", 999))
        assertFalse(c.contains("a", 1000))
        val d = SeenCache(3, 1_000_000)
        listOf("a", "b", "c", "d").forEachIndexed { i, id -> assertTrue(d.insert(id, i.toLong())) }
        assertEquals(3, d.size)
        assertFalse(d.contains("a", 10))
        assertTrue(d.contains("d", 10))
        val big = SeenCache()
        repeat(2000) { big.insert("id$it", it.toLong()) }
        assertEquals(1024, big.size)
    }

    @Test
    fun conflictClockRules() {
        val k = ConflictClock()
        assertTrue(k.shouldApply(10, DeviceId(ByteArray(16) { 5 })))
        assertTrue(k.shouldApply(11, DeviceId(ByteArray(16) { 9 })))
        assertFalse(k.shouldApply(10, DeviceId(ByteArray(16) { 1 })))
        val j = ConflictClock()
        assertTrue(j.shouldApply(10, DeviceId(ByteArray(16) { 5 })))
        assertFalse(j.shouldApply(10, DeviceId(ByteArray(16) { 7 })))
        assertTrue(j.shouldApply(10, DeviceId(ByteArray(16) { 3 })))
        assertFalse(j.shouldApply(10, DeviceId(ByteArray(16) { 3 })))
    }

    @Test
    fun uniqueNameSkipsExisting() {
        val taken = setOf("a.txt", "a (1).txt", "a (2).txt")
        assertEquals("a (3).txt", PathRules.uniqueName("a.txt") { it in taken })
        assertEquals("b.txt", PathRules.uniqueName("b.txt") { it in taken })
    }

    @Test
    fun idsAndEncodings() {
        assertTrue(isUuidV4(newUuidV4()))
        assertFalse(isUuidV4("00000000-0000-0000-0000-000000000000"))
        assertFailsWith<ProtoException.BadHeader> { DeviceId.fromHex("AB".repeat(16)) }
        assertFailsWith<ProtoException.BadHeader> { b64urlDecode("AA==") }
        assertFailsWith<ProtoException.BadHeader> { b64urlDecode("a+b/") }
        assertTrue(DeviceId(ByteArray(16) { if (it == 15) 1 else 0 }) < DeviceId(ByteArray(16) { if (it == 0) 1 else 0 }))
        for (s in listOf("", "abc", "a b", "é😀", "100%")) assertEquals(s, percentDecode(percentEncode(s)))
    }

    @Test
    fun pathLimits() {
        assertTrue(PathRules.isValid("a".repeat(255)))
        assertFalse(PathRules.isValid("a".repeat(256)))
        assertFalse(PathRules.isValid("é".repeat(200)))
        assertFalse(PathRules.isValid(List(21) { "a".repeat(200) }.joinToString("/")))
    }
}
