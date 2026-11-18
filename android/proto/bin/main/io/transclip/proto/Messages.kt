package io.transclip.proto

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** Protocol version spoken by this implementation. */
const val PROTOCOL_VERSION = 1L

const val MAX_TEXT = 8 * 1024 * 1024
const val MAX_IMAGE = 50 * 1024 * 1024
const val MAX_CHUNK = 64 * 1024

/** Kind of a transferred entry. */
@Serializable
enum class EntryKind {
    @SerialName("file")
    FILE,

    @SerialName("dir")
    DIR,
}

/**
 * One protocol message (PROTOCOL.md sections 6 and 9). The JSON `t` field selects the subclass.
 * The frame-level `size` field is owned by the frame codec.
 */
@Serializable
sealed interface Message {
    @Serializable
    @SerialName("hs1")
    data class Hs1(
        val v: Long,
        val gid: String,
        @SerialName("eph_i") val ephI: String,
        @SerialName("n_i") val nI: String,
    ) : Message

    @Serializable
    @SerialName("hs2")
    data class Hs2(
        val v: Long,
        @SerialName("eph_r") val ephR: String,
        @SerialName("n_r") val nR: String,
        @SerialName("mac_r") val macR: String,
    ) : Message

    @Serializable
    @SerialName("hs3")
    data class Hs3(@SerialName("mac_i") val macI: String) : Message

    @Serializable
    @SerialName("hello")
    data class Hello(
        val v: Long,
        val id: String,
        val name: String,
        val pl: String,
        val app: String,
        val caps: List<String>,
        val role: String,
    ) : Message

    @Serializable
    @SerialName("ping")
    data object Ping : Message

    @Serializable
    @SerialName("pong")
    data object Pong : Message

    @Serializable
    @SerialName("error")
    data class Error(
        val code: String,
        val msg: String? = null,
        @SerialName("ref") val reference: String? = null,
    ) : Message

    @Serializable
    @SerialName("bye")
    data class Bye(val reason: String) : Message

    @Serializable
    @SerialName("text")
    data class Text(val id: String, val origin: String, val ts: Long, val hash: String) : Message

    @Serializable
    @SerialName("image")
    data class Image(
        val id: String,
        val origin: String,
        val ts: Long,
        val hash: String,
        val mime: String,
        val w: Long,
        val h: Long,
    ) : Message

    @Serializable
    @SerialName("announce")
    data class Announce(
        val id: String,
        val origin: String,
        val ts: Long,
        val hash: String,
        val kind: String,
        val mime: String,
        val w: Long,
        val h: Long,
        val bytes: Long,
    ) : Message

    @Serializable
    @SerialName("pull")
    data class Pull(val id: String) : Message

    @Serializable
    @SerialName("ack")
    data class Ack(val id: String, val ok: Boolean, val reason: String? = null) : Message

    @Serializable
    @SerialName("file_offer")
    data class FileOffer(
        @SerialName("xfer_id") val xferId: String,
        @SerialName("batch_id") val batchId: String,
        @SerialName("batch_name") val batchName: String,
        @SerialName("batch_count") val batchCount: Long,
        @SerialName("batch_bytes") val batchBytes: Long,
        val kind: EntryKind,
        @SerialName("relative_path") val relativePath: String,
        val bytes: Long,
        val sha256: String,
        val mtime: Long,
    ) : Message

    @Serializable
    @SerialName("file_accept")
    data class FileAccept(@SerialName("xfer_id") val xferId: String, val offset: Long) : Message

    @Serializable
    @SerialName("file_reject")
    data class FileReject(@SerialName("xfer_id") val xferId: String, val reason: String) : Message

    @Serializable
    @SerialName("file_stream")
    data class FileStream(@SerialName("xfer_id") val xferId: String, val offset: Long) : Message

    @Serializable
    @SerialName("file_chunk")
    data class FileChunk(@SerialName("xfer_id") val xferId: String, val offset: Long) : Message

    @Serializable
    @SerialName("file_end")
    data class FileEnd(@SerialName("xfer_id") val xferId: String) : Message

    @Serializable
    @SerialName("file_result")
    data class FileResult(
        @SerialName("xfer_id") val xferId: String,
        val ok: Boolean,
        val reason: String? = null,
    ) : Message

    @Serializable
    @SerialName("file_cancel")
    data class FileCancel(@SerialName("xfer_id") val xferId: String, val reason: String) : Message

    @Serializable
    @SerialName("batch_done")
    data class BatchDone(
        @SerialName("batch_id") val batchId: String,
        @SerialName("ok_count") val okCount: Long,
        @SerialName("fail_count") val failCount: Long,
    ) : Message

    @Serializable
    @SerialName("rekey")
    data class Rekey(
        @SerialName("new_gk") val newGk: String,
        val name: String,
        val by: String,
        val ts: Long,
    ) : Message

    @Serializable
    @SerialName("rekey_ack")
    data class RekeyAck(val ts: Long) : Message

    /** A message type this version does not know. Never serialized. */
    data class Unknown(val type: String) : Message
}

/** Every `t` value this version understands. */
val KNOWN_TYPES: Set<String> = setOf(
    "hs1", "hs2", "hs3", "hello", "ping", "pong", "error", "bye", "text", "image", "announce",
    "pull", "ack", "file_offer", "file_accept", "file_reject", "file_stream", "file_chunk",
    "file_end", "file_result", "file_cancel", "batch_done", "rekey", "rekey_ack",
)

/** Largest payload this message type may carry (PROTOCOL.md section 10). */
fun Message.maxPayload(): Int = when (this) {
    is Message.Text -> MAX_TEXT
    is Message.Image -> MAX_IMAGE
    is Message.FileChunk -> MAX_CHUNK
    is Message.Unknown -> MAX_IMAGE
    else -> 0
}

private fun badField(what: String): Nothing = throw ProtoException.BadHeader(what)

private fun checkVersion(v: Long) {
    if (v != PROTOCOL_VERSION) throw ProtoException.Version(v)
}

private fun uuid(s: String) {
    if (!isUuidV4(s)) badField("expected a lowercase UUIDv4")
}

private fun device(s: String) {
    DeviceId.fromHex(s)
}

private fun sha256(s: String) {
    if (!isSha256Hex(s)) badField("expected a 64-character lowercase SHA-256 hex digest")
}

/** Check field formats (ids, hashes, versions). Does not check payloads or paths. */
fun Message.validate() {
    when (this) {
        is Message.Hs1 -> {
            checkVersion(v)
            GroupId.fromHex(gid)
        }
        is Message.Hs2 -> checkVersion(v)
        is Message.Hello -> {
            checkVersion(v)
            device(id)
        }
        is Message.Text -> {
            uuid(id)
            device(origin)
            sha256(hash)
        }
        is Message.Image -> {
            uuid(id)
            device(origin)
            sha256(hash)
        }
        is Message.Announce -> {
            uuid(id)
            device(origin)
            sha256(hash)
        }
        is Message.Pull -> uuid(id)
        is Message.Ack -> uuid(id)
        is Message.FileOffer -> {
            uuid(xferId)
            uuid(batchId)
            when (kind) {
                EntryKind.FILE -> sha256(sha256)
                EntryKind.DIR -> if (bytes != 0L) badField("directory entries must have bytes = 0")
            }
        }
        is Message.FileAccept -> uuid(xferId)
        is Message.FileReject -> uuid(xferId)
        is Message.FileStream -> uuid(xferId)
        is Message.FileChunk -> uuid(xferId)
        is Message.FileEnd -> uuid(xferId)
        is Message.FileResult -> uuid(xferId)
        is Message.FileCancel -> uuid(xferId)
        is Message.BatchDone -> uuid(batchId)
        is Message.Rekey -> device(by)
        else -> Unit
    }
}
