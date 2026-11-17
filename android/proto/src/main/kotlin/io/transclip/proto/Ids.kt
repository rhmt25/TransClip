package io.transclip.proto

import java.security.SecureRandom
import java.util.Base64
import java.util.UUID

private val random = SecureRandom()

/** Fill [buf] from the operating system RNG. */
fun fillRandom(buf: ByteArray) = random.nextBytes(buf)

private const val HEX = "0123456789abcdef"

/** Lowercase hex form of these bytes. */
fun ByteArray.toHex(): String {
    val sb = StringBuilder(size * 2)
    for (b in this) {
        sb.append(HEX[(b.toInt() shr 4) and 0xf]).append(HEX[b.toInt() and 0xf])
    }
    return sb.toString()
}

/** True when every character is `0-9` or `a-f`. */
fun isLowerHex(s: String): Boolean = s.all { it in '0'..'9' || it in 'a'..'f' }

/** Decode lowercase hex of exactly [bytes] bytes; anything else is a bad header. */
fun hexToBytes(s: String, bytes: Int? = null): ByteArray {
    if (s.length % 2 != 0 || !isLowerHex(s) || (bytes != null && s.length != bytes * 2)) {
        throw ProtoException.BadHeader("expected lowercase hex")
    }
    return ByteArray(s.length / 2) { i -> s.substring(i * 2, i * 2 + 2).toInt(16).toByte() }
}

/** True for a 64-character lowercase hex SHA-256 digest. */
fun isSha256Hex(s: String): Boolean = s.length == 64 && isLowerHex(s)

/** Encode as base64url without padding. */
fun b64urlEncode(data: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(data)

/** Decode base64url without padding. Padding or other alphabets are rejected. */
fun b64urlDecode(s: String): ByteArray {
    if (s.contains('=')) throw ProtoException.BadHeader("invalid base64url")
    return try {
        Base64.getUrlDecoder().decode(s)
    } catch (e: IllegalArgumentException) {
        throw ProtoException.BadHeader("invalid base64url")
    }
}

/** Decode base64url into exactly [n] bytes. */
fun b64urlDecodeExact(s: String, n: Int): ByteArray {
    val v = b64urlDecode(s)
    if (v.size != n) throw ProtoException.BadHeader("expected $n bytes")
    return v
}

/** True for a lowercase UUIDv4 string (`8-4-4-4-12`). */
fun isUuidV4(s: String): Boolean {
    if (s.length != 36) return false
    for ((i, c) in s.withIndex()) {
        val ok = when (i) {
            8, 13, 18, 23 -> c == '-'
            14 -> c == '4'
            19 -> c == '8' || c == '9' || c == 'a' || c == 'b'
            else -> c in '0'..'9' || c in 'a'..'f'
        }
        if (!ok) return false
    }
    return true
}

/** Generate a random UUIDv4 string. */
fun newUuidV4(): String = UUID.randomUUID().toString()

/** Compare byte arrays as unsigned big-endian numbers (lexicographically). */
fun compareUnsigned(a: ByteArray, b: ByteArray): Int {
    val n = minOf(a.size, b.size)
    for (i in 0 until n) {
        val x = a[i].toInt() and 0xff
        val y = b[i].toInt() and 0xff
        if (x != y) return x - y
    }
    return a.size - b.size
}

/** 16-byte device identifier, shown as 32 lowercase hex characters. */
class DeviceId(val bytes: ByteArray) : Comparable<DeviceId> {
    init {
        require(bytes.size == 16) { "device id must be 16 bytes" }
    }

    fun toHex(): String = bytes.toHex()

    override fun compareTo(other: DeviceId): Int = compareUnsigned(bytes, other.bytes)

    override fun equals(other: Any?): Boolean = other is DeviceId && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "DeviceId(${toHex()})"

    companion object {
        fun random(): DeviceId = DeviceId(ByteArray(16).also { fillRandom(it) })

        fun fromHex(s: String): DeviceId = DeviceId(hexToBytes(s, 16))
    }
}

/** 8-byte group identifier, shown as 16 lowercase hex characters. */
class GroupId(val bytes: ByteArray) {
    init {
        require(bytes.size == 8) { "group id must be 8 bytes" }
    }

    fun toHex(): String = bytes.toHex()

    override fun equals(other: Any?): Boolean = other is GroupId && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "GroupId(${toHex()})"

    companion object {
        fun fromHex(s: String): GroupId = GroupId(hexToBytes(s, 8))
    }
}
