package io.transclip.proto

import kotlin.random.Random
import kotlin.test.Test
import kotlin.test.assertTrue

/** Random-input tests (P7-01): hostile input may fail with [ProtoException] but must never crash. */
class FuzzTest {
    private val rnd = Random(12345)
    private val iterations = 10_000

    private fun bytes(max: Int) = ByteArray(rnd.nextInt(max)) { rnd.nextInt().toByte() }

    private fun text(max: Int): String {
        val alphabet = "abcXYZ019./\\: _-<>|?*\u0000é日😀%&=,"
        return buildString { repeat(rnd.nextInt(max)) { append(alphabet[rnd.nextInt(alphabet.length)]) } }
    }

    private inline fun tolerant(block: () -> Unit) {
        try {
            block()
        } catch (_: ProtoException) {
        } catch (_: PathException) {
        }
    }

    @Test
    fun beaconDecode() = repeat(iterations) { tolerant { Beacon.decode(bytes(512)) } }

    @Test
    fun pairingParse() = repeat(iterations) {
        tolerant { PairingInfo.parse(text(300)) }
        tolerant { PairingInfo.parse("transclip://pair?v=1&k=${text(50)}&g=x&d=${text(40)}&a=${text(40)}&x=${text(20)}") }
    }

    @Test
    fun pathValidation() = repeat(iterations) {
        val s = text(120)
        tolerant {
            val segs = PathRules.validate(s)
            assertTrue(segs.none { it.isEmpty() || it == "." || it == ".." || it.any { c -> c == '/' || c == '\\' || c == ':' } })
        }
    }

    @Test
    fun frameDecoder() = repeat(2_000) {
        val d = FrameDecoder()
        tolerant { repeat(rnd.nextInt(8)) { d.push(bytes(200)) } }
    }
}
