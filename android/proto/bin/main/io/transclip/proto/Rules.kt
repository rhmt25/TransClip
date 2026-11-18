package io.transclip.proto

import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

/* ---------------------------------------------------------------- beacon */

/** Operating system of the announcing device. */
enum class Platform(val code: Int) {
    WINDOWS(1),
    ANDROID(2),
}

/** UDP discovery beacon (PROTOCOL.md section 4.2). */
data class Beacon(
    val gid: GroupId,
    val deviceId: DeviceId,
    val port: Int,
    val platform: Platform,
    val name: String,
) {
    fun encode(): ByteArray {
        val nameBytes = name.toByteArray(Charsets.UTF_8)
        if (nameBytes.size > MAX_NAME) throw ProtoException.Limit("beacon name longer than 64 bytes")
        val out = ByteArray(FIXED_LEN + nameBytes.size)
        MAGIC.copyInto(out, 0)
        out[4] = BEACON_VERSION.toByte()
        gid.bytes.copyInto(out, 5)
        deviceId.bytes.copyInto(out, 13)
        out[29] = (port ushr 8).toByte()
        out[30] = port.toByte()
        out[31] = platform.code.toByte()
        nameBytes.copyInto(out, FIXED_LEN)
        return out
    }

    companion object {
        private val MAGIC = "TCLB".toByteArray(Charsets.US_ASCII)
        const val BEACON_VERSION = 1
        const val FIXED_LEN = 44
        const val MAX_NAME = 64

        /** Parse a received datagram. Anything that is not a valid v1 beacon throws. */
        fun decode(data: ByteArray): Beacon {
            if (data.size < FIXED_LEN || !data.copyOfRange(0, 4).contentEquals(MAGIC)) {
                throw ProtoException.BadHeader("not a beacon")
            }
            if (data[4].toInt() != BEACON_VERSION) throw ProtoException.Version(data[4].toLong() and 0xff)
            if (data.size - FIXED_LEN > MAX_NAME) throw ProtoException.Limit("beacon name longer than 64 bytes")
            val name = try {
                Charsets.UTF_8.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(java.nio.ByteBuffer.wrap(data, FIXED_LEN, data.size - FIXED_LEN))
                    .toString()
            } catch (e: CharacterCodingException) {
                throw ProtoException.BadHeader("beacon name is not UTF-8")
            }
            val platform = Platform.entries.firstOrNull { it.code == (data[31].toInt() and 0xff) }
                ?: throw ProtoException.BadHeader("unknown platform")
            return Beacon(
                gid = GroupId(data.copyOfRange(5, 13)),
                deviceId = DeviceId(data.copyOfRange(13, 29)),
                port = ((data[29].toInt() and 0xff) shl 8) or (data[30].toInt() and 0xff),
                platform = platform,
                name = name,
            )
        }
    }
}

/* ---------------------------------------------------------------- pairing */

/** Percent-encode everything except RFC 3986 unreserved characters. */
fun percentEncode(s: String): String {
    val sb = StringBuilder()
    for (b in s.toByteArray(Charsets.UTF_8)) {
        val c = b.toInt() and 0xff
        val ch = c.toChar()
        if (ch in 'a'..'z' || ch in 'A'..'Z' || ch in '0'..'9' || ch == '-' || ch == '.' || ch == '_' || ch == '~') {
            sb.append(ch)
        } else {
            sb.append('%').append("0123456789ABCDEF"[c shr 4]).append("0123456789ABCDEF"[c and 0xf])
        }
    }
    return sb.toString()
}

/** Decode percent-encoding into a UTF-8 string. */
fun percentDecode(s: String): String {
    val out = java.io.ByteArrayOutputStream()
    var i = 0
    while (i < s.length) {
        if (s[i] == '%') {
            val v = s.substring(i + 1, minOf(i + 3, s.length)).takeIf { it.length == 2 }?.toIntOrNull(16)
                ?: throw ProtoException.BadHeader("bad percent-encoding")
            out.write(v)
            i += 3
        } else {
            out.write(s[i].code)
            i++
        }
    }
    return try {
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .decode(java.nio.ByteBuffer.wrap(out.toByteArray()))
            .toString()
    } catch (e: CharacterCodingException) {
        throw ProtoException.BadHeader("not UTF-8")
    }
}

/** Everything a new device learns from a pairing code (PROTOCOL.md section 3.1). */
class PairingInfo(
    val gk: GroupKey,
    val groupName: String,
    val inviter: DeviceId,
    val addrs: List<String>,
    val expires: Long,
) {
    fun toUri(): String {
        val sb = StringBuilder(PREFIX)
        sb.append("v=1&k=").append(b64urlEncode(gk.bytes))
        sb.append("&g=").append(percentEncode(groupName))
        sb.append("&d=").append(inviter.toHex())
        if (addrs.isNotEmpty()) sb.append("&a=").append(percentEncode(addrs.joinToString(",")))
        sb.append("&x=").append(expires)
        return sb.toString()
    }

    /** True when the code is past its expiry at [nowSeconds] (Unix seconds). */
    fun isExpired(nowSeconds: Long): Boolean = nowSeconds > expires

    companion object {
        private const val PREFIX = "transclip://pair?"

        fun parse(uri: String): PairingInfo {
            val query = uri.trim().removePrefix(PREFIX).takeIf { uri.trim().startsWith(PREFIX) }
                ?: throw ProtoException.BadHeader("not a transclip pairing code")
            var v: String? = null
            var k: String? = null
            var g: String? = null
            var d: String? = null
            var a: String? = null
            var x: String? = null
            for (pair in query.split('&')) {
                val idx = pair.indexOf('=')
                val key = if (idx < 0) pair else pair.substring(0, idx)
                val value = if (idx < 0) "" else pair.substring(idx + 1)
                when (key) {
                    "v" -> v = value
                    "k" -> k = value
                    "g" -> g = percentDecode(value)
                    "d" -> d = value
                    "a" -> a = percentDecode(value)
                    "x" -> x = value
                }
            }
            val version = v?.toLongOrNull() ?: throw ProtoException.BadHeader("missing version")
            if (version != 1L) throw ProtoException.Version(version)
            return PairingInfo(
                gk = GroupKey(b64urlDecodeExact(k ?: throw ProtoException.BadHeader("missing key"), 32)),
                groupName = g ?: throw ProtoException.BadHeader("missing group name"),
                inviter = DeviceId.fromHex(d ?: throw ProtoException.BadHeader("missing inviter")),
                addrs = a?.split(',')?.filter { it.isNotEmpty() } ?: emptyList(),
                expires = x?.toLongOrNull() ?: throw ProtoException.BadHeader("missing expiry"),
            )
        }
    }
}

/* ---------------------------------------------------------------- paths */

/** Why a path was refused. */
class PathException(message: String) : IllegalArgumentException(message)

/** Validation of `relative_path` received from peers (PROTOCOL.md section 9.3). */
object PathRules {
    const val MAX_SEGMENT = 255
    const val MAX_PATH = 4096
    private const val FORBIDDEN = "\\:<>\"|?*"

    private fun isReserved(segment: String): Boolean {
        val stem = segment.substringBefore('.').trimEnd(' ').uppercase(java.util.Locale.ROOT)
        if (stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL") return true
        return stem.length == 4 && (stem.startsWith("COM") || stem.startsWith("LPT")) && stem[3] in '1'..'9'
    }

    /** Check [path] and return its segments; throws [PathException] when it must be rejected. */
    fun validate(path: String): List<String> {
        if (path.isEmpty()) throw PathException("empty path")
        if (path.toByteArray(Charsets.UTF_8).size > MAX_PATH) throw PathException("path too long")
        if (path.startsWith("/")) throw PathException("absolute path")
        for (c in path) {
            if (c.isISOControl() || c in FORBIDDEN) throw PathException("forbidden character")
        }
        val segments = path.split('/')
        for (seg in segments) {
            if (seg.isEmpty() || seg == "." || seg == "..") throw PathException("bad segment")
            if (seg.toByteArray(Charsets.UTF_8).size > MAX_SEGMENT) throw PathException("segment too long")
            if (seg.endsWith(".") || seg.endsWith(" ")) throw PathException("trailing dot or space")
            if (isReserved(seg)) throw PathException("reserved Windows name")
        }
        return segments
    }

    fun isValid(path: String): Boolean = try {
        validate(path)
        true
    } catch (e: PathException) {
        false
    }

    /** `a.txt` becomes `a (1).txt`; the extension is the part after the last dot (not at index 0). */
    fun withCounter(name: String, n: Int): String {
        val i = name.lastIndexOf('.')
        return if (i > 0) "${name.substring(0, i)} ($n)${name.substring(i)}" else "$name ($n)"
    }

    /** First free name, using [exists] to probe. */
    fun uniqueName(name: String, exists: (String) -> Boolean): String {
        if (!exists(name)) return name
        var n = 1
        while (true) {
            val candidate = withCounter(name, n)
            if (!exists(candidate)) return candidate
            n++
        }
    }
}

/* ---------------------------------------------------------------- sync rules */

/** Remembers recently seen item ids so the same item is applied only once. */
class SeenCache(private val capacity: Int = 1024, private val ttlMs: Long = 10 * 60 * 1000L) {
    private val index = HashMap<String, Long>()
    private val order = ArrayDeque<Pair<String, Long>>()

    /** Record [id]. Returns true when it is new, false when it was already seen. */
    fun insert(id: String, nowMs: Long): Boolean {
        expire(nowMs)
        if (index.containsKey(id)) return false
        index[id] = nowMs
        order.addLast(id to nowMs)
        while (order.size > capacity) index.remove(order.removeFirst().first)
        return true
    }

    fun contains(id: String, nowMs: Long): Boolean {
        expire(nowMs)
        return index.containsKey(id)
    }

    val size: Int get() = index.size

    private fun expire(nowMs: Long) {
        while (order.isNotEmpty() && nowMs - order.first().second >= ttlMs) {
            index.remove(order.removeFirst().first)
        }
    }
}

/**
 * Decides whether an incoming item is newer than the one already applied: newer `ts` wins; on
 * equal `ts` the smaller origin wins; an identical pair is a duplicate.
 */
class ConflictClock {
    private var lastTs: Long = 0
    private var lastOrigin: DeviceId? = null

    /** Returns true and remembers the item when it should be applied. */
    fun shouldApply(ts: Long, origin: DeviceId): Boolean {
        val last = lastOrigin
        val apply = last == null || ts > lastTs || (ts == lastTs && origin < last)
        if (apply) {
            lastTs = ts
            lastOrigin = origin
        }
        return apply
    }
}
