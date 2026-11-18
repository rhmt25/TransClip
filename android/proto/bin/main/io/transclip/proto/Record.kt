package io.transclip.proto

import org.bouncycastle.crypto.modes.ChaCha20Poly1305
import org.bouncycastle.crypto.params.KeyParameter
import org.bouncycastle.crypto.params.ParametersWithIV
import java.io.InputStream
import java.io.OutputStream

/**
 * Encrypted record layer (PROTOCOL.md section 7).
 *
 * Every record is `[u32 BE length L][L bytes ChaCha20-Poly1305 ciphertext]` with
 * `17 <= L <= 65552`. The nonce is `0x00000000 || counter (u64 BE)`; counters are kept per
 * direction and start at 0.
 */
object Record {
    const val MAX_PLAINTEXT = 65_536
    const val TAG_LEN = 16
    const val MIN_LEN = 1 + TAG_LEN
    const val MAX_LEN = MAX_PLAINTEXT + TAG_LEN
    const val COUNTER_LIMIT = 1L shl 32

    private fun nonce(counter: Long): ByteArray {
        val n = ByteArray(12)
        for (i in 0 until 8) n[11 - i] = (counter ushr (8 * i)).toByte()
        return n
    }

    private fun run(encrypt: Boolean, key: ByteArray, counter: Long, input: ByteArray): ByteArray {
        val cipher = ChaCha20Poly1305()
        cipher.init(encrypt, ParametersWithIV(KeyParameter(key), nonce(counter)))
        val out = ByteArray(cipher.getOutputSize(input.size))
        val n = cipher.processBytes(input, 0, input.size, out, 0)
        val m = cipher.doFinal(out, n)
        return out.copyOf(n + m)
    }

    /** Encrypt one record body (without the length prefix). */
    fun seal(key: ByteArray, counter: Long, plaintext: ByteArray): ByteArray {
        if (plaintext.isEmpty() || plaintext.size > MAX_PLAINTEXT) throw ProtoException.Limit("record plaintext size")
        if (counter < 0 || counter >= COUNTER_LIMIT) throw ProtoException.Limit("record counter")
        return run(true, key, counter, plaintext)
    }

    /** Decrypt one record body (without the length prefix). */
    fun open(key: ByteArray, counter: Long, ciphertext: ByteArray): ByteArray {
        if (ciphertext.size < MIN_LEN || ciphertext.size > MAX_LEN) throw ProtoException.Limit("record length")
        if (counter < 0 || counter >= COUNTER_LIMIT) throw ProtoException.Limit("record counter")
        return try {
            run(false, key, counter, ciphertext)
        } catch (e: org.bouncycastle.crypto.InvalidCipherTextException) {
            throw ProtoException.Auth()
        }
    }
}

/** Sending half of an encrypted connection. */
class SecureOutput internal constructor(private val out: OutputStream, key: ByteArray) {
    private val key = key.copyOf()
    private var counter = 0L

    /** Number of records sent so far. */
    val recordsSent: Long get() = counter

    /** Encrypt and send [data] as one or more records. Does not flush. */
    fun writeBytes(data: ByteArray) {
        var offset = 0
        while (offset < data.size) {
            val n = minOf(Record.MAX_PLAINTEXT, data.size - offset)
            val body = Record.seal(key, counter, data.copyOfRange(offset, offset + n))
            counter++
            out.write(byteArrayOf((body.size ushr 24).toByte(), (body.size ushr 16).toByte(), (body.size ushr 8).toByte(), body.size.toByte()))
            out.write(body)
            offset += n
        }
    }

    /** Send one frame and flush so the peer sees it immediately. */
    fun writeFrame(message: Message, payload: ByteArray = ByteArray(0)) {
        writeBytes(encodeFrame(message, payload))
        out.flush()
    }

    /** Wipe the key from memory. */
    fun wipe() = key.fill(0)
}

/** Receiving half of an encrypted connection. */
class SecureInput internal constructor(private val input: InputStream, key: ByteArray) {
    private val key = key.copyOf()
    private var counter = 0L
    private var pending = ByteArray(0)
    private var pos = 0

    /** Number of records received so far. */
    val recordsReceived: Long get() = counter

    private fun nextRecord() {
        val prefix = ByteArray(4)
        input.readExactly(prefix)
        val len = ((prefix[0].toLong() and 0xff) shl 24) or ((prefix[1].toLong() and 0xff) shl 16) or
            ((prefix[2].toLong() and 0xff) shl 8) or (prefix[3].toLong() and 0xff)
        if (len < Record.MIN_LEN || len > Record.MAX_LEN) throw ProtoException.Limit("record length $len")
        val body = ByteArray(len.toInt())
        input.readExactly(body)
        val plain = Record.open(key, counter, body)
        counter++
        pending = pending.copyOfRange(pos, pending.size) + plain
        pos = 0
    }

    private fun readExact(buf: ByteArray) {
        while (pending.size - pos < buf.size) nextRecord()
        pending.copyInto(buf, 0, pos, pos + buf.size)
        pos += buf.size
    }

    /** Read the next frame from the decrypted stream. */
    fun readFrame(): Frame = readFrameWith(::readExact)

    /** Wipe the key from memory. */
    fun wipe() {
        key.fill(0)
        pending.fill(0)
    }
}
