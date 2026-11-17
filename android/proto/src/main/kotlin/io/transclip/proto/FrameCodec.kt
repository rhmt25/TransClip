package io.transclip.proto

import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import java.io.InputStream
import java.io.OutputStream

/** Largest allowed JSON header (PROTOCOL.md section 8). */
const val MAX_HEADER = 65_536

/** One decoded frame. */
class Frame(val message: Message, val payload: ByteArray)

internal val wireJson = Json {
    classDiscriminator = "t"
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = false
}

/** Serialize a message and payload into one frame. */
fun encodeFrame(message: Message, payload: ByteArray = ByteArray(0)): ByteArray {
    if (message is Message.Unknown) throw ProtoException.BadHeader("cannot encode an unknown message")
    if (payload.size > message.maxPayload()) {
        throw ProtoException.Limit("payload of ${payload.size} bytes exceeds the limit")
    }
    val fields = wireJson.encodeToJsonElement(Message.serializer(), message).jsonObject
    val header = JsonObject(fields + ("size" to JsonPrimitive(payload.size)))
    val json = wireJson.encodeToString(JsonObject.serializer(), header).toByteArray(Charsets.UTF_8)
    if (json.isEmpty() || json.size > MAX_HEADER) throw ProtoException.Limit("header too large")
    val out = ByteArray(4 + json.size + payload.size)
    out[0] = (json.size ushr 24).toByte()
    out[1] = (json.size ushr 16).toByte()
    out[2] = (json.size ushr 8).toByte()
    out[3] = json.size.toByte()
    json.copyInto(out, 4)
    payload.copyInto(out, 4 + json.size)
    return out
}

internal fun checkHeaderLength(n: Long) {
    if (n <= 0 || n > MAX_HEADER) throw ProtoException.Limit("header length $n")
}

/** Parse a header (without the length prefix) into a message and its declared payload size. */
fun decodeHeader(header: ByteArray): Pair<Message, Int> {
    val element = try {
        wireJson.parseToJsonElement(String(header, Charsets.UTF_8))
    } catch (e: SerializationException) {
        throw ProtoException.BadHeader("header is not JSON")
    } catch (e: IllegalArgumentException) {
        throw ProtoException.BadHeader("header is not JSON")
    }
    val obj = element as? JsonObject ?: throw ProtoException.BadHeader("header is not a JSON object")
    val t = (obj["t"] as? JsonPrimitive)?.takeIf { it.isString }?.content
        ?: throw ProtoException.BadHeader("missing \"t\"")
    val size = (obj["size"] as? JsonPrimitive)?.takeIf { !it.isString }?.content?.toLongOrNull()
        ?.takeIf { it >= 0 }
        ?: throw ProtoException.BadHeader("missing or invalid \"size\"")
    val message = if (t in KNOWN_TYPES) {
        try {
            wireJson.decodeFromJsonElement(Message.serializer(), obj)
        } catch (e: SerializationException) {
            throw ProtoException.BadHeader("invalid $t message")
        } catch (e: IllegalArgumentException) {
            throw ProtoException.BadHeader("invalid $t message")
        }
    } else {
        Message.Unknown(t)
    }
    if (size > message.maxPayload()) throw ProtoException.Limit("payload of $size bytes exceeds the limit")
    return message to size.toInt()
}

/** Incremental decoder: feed it bytes in any chunking, get whole frames out. */
class FrameDecoder {
    private var buf = ByteArray(0)

    /** Append [data] and return every frame that is now complete. */
    fun push(data: ByteArray): List<Frame> {
        buf += data
        val frames = mutableListOf<Frame>()
        var offset = 0
        while (true) {
            val (frame, used) = parseAt(offset) ?: break
            frames += frame
            offset += used
        }
        buf = buf.copyOfRange(offset, buf.size)
        return frames
    }

    private fun parseAt(offset: Int): Pair<Frame, Int>? {
        if (buf.size - offset < 4) return null
        val n = ((buf[offset].toLong() and 0xff) shl 24) or
            ((buf[offset + 1].toLong() and 0xff) shl 16) or
            ((buf[offset + 2].toLong() and 0xff) shl 8) or
            (buf[offset + 3].toLong() and 0xff)
        checkHeaderLength(n)
        val headerEnd = offset + 4 + n.toInt()
        if (buf.size < headerEnd) return null
        val (message, size) = decodeHeader(buf.copyOfRange(offset + 4, headerEnd))
        if (buf.size < headerEnd + size) return null
        return Frame(message, buf.copyOfRange(headerEnd, headerEnd + size)) to (headerEnd + size - offset)
    }
}

internal fun InputStream.readExactly(buf: ByteArray) {
    var done = 0
    while (done < buf.size) {
        val n = read(buf, done, buf.size - done)
        if (n < 0) throw ProtoException.Closed()
        done += n
    }
}

/** Read one frame using [readExact] to obtain exactly the requested number of bytes. */
internal fun readFrameWith(readExact: (ByteArray) -> Unit): Frame {
    val prefix = ByteArray(4)
    readExact(prefix)
    val n = ((prefix[0].toLong() and 0xff) shl 24) or ((prefix[1].toLong() and 0xff) shl 16) or
        ((prefix[2].toLong() and 0xff) shl 8) or (prefix[3].toLong() and 0xff)
    checkHeaderLength(n)
    val header = ByteArray(n.toInt())
    readExact(header)
    val (message, size) = decodeHeader(header)
    val payload = ByteArray(size)
    readExact(payload)
    return Frame(message, payload)
}

/** Read one plaintext frame (used during the handshake). */
fun readPlainFrame(input: InputStream): Frame = readFrameWith { input.readExactly(it) }

/** Write one plaintext frame and flush. */
fun writePlainFrame(output: OutputStream, message: Message, payload: ByteArray = ByteArray(0)) {
    output.write(encodeFrame(message, payload))
    output.flush()
}
