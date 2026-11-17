package io.transclip.proto

import org.bouncycastle.crypto.digests.SHA256Digest
import org.bouncycastle.crypto.generators.HKDFBytesGenerator
import org.bouncycastle.crypto.params.HKDFParameters
import org.bouncycastle.math.ec.rfc7748.X25519
import java.io.InputStream
import java.io.OutputStream
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

private val GID_DOMAIN = "TransClip-v1 group-id".toByteArray(Charsets.US_ASCII)
private val TRANSCRIPT_DOMAIN = "TransClip-v1 transcript".toByteArray(Charsets.US_ASCII)
private val KEYS_INFO = "TransClip-v1 keys".toByteArray(Charsets.US_ASCII)

private fun sha256(vararg parts: ByteArray): ByteArray {
    val md = MessageDigest.getInstance("SHA-256")
    for (p in parts) md.update(p)
    return md.digest()
}

/** 32-byte secret shared by every device of a group (PROTOCOL.md section 3). */
class GroupKey(val bytes: ByteArray) {
    init {
        require(bytes.size == 32) { "group key must be 32 bytes" }
    }

    /** The group id other devices can see without learning the key. */
    fun groupId(): GroupId = GroupId(sha256(GID_DOMAIN, bytes).copyOf(8))

    /** Overwrite the key bytes. */
    fun wipe() = bytes.fill(0)

    override fun equals(other: Any?): Boolean = other is GroupKey && MessageDigest.isEqual(bytes, other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "GroupKey(..)"

    companion object {
        fun generate(): GroupKey = GroupKey(ByteArray(32).also { fillRandom(it) })
    }
}

/** Keys and MAC tags derived from a handshake. */
class Derived(
    val th: ByteArray,
    val kI2r: ByteArray,
    val kR2i: ByteArray,
    val macR: ByteArray,
    val macI: ByteArray,
) {
    fun wipe() {
        th.fill(0)
        kI2r.fill(0)
        kR2i.fill(0)
        macR.fill(0)
        macI.fill(0)
    }
}

/** Hash of everything both sides have seen in the clear. */
fun transcriptHash(gid: GroupId, ephI: ByteArray, nI: ByteArray, ephR: ByteArray, nR: ByteArray): ByteArray =
    sha256(TRANSCRIPT_DOMAIN, gid.bytes, ephI, nI, ephR, nR)

private fun hmacTag(key: ByteArray, label: String, th: ByteArray): ByteArray {
    val mac = Mac.getInstance("HmacSHA256")
    mac.init(SecretKeySpec(key, "HmacSHA256"))
    mac.update(label.toByteArray(Charsets.US_ASCII))
    mac.update(th)
    return mac.doFinal()
}

/** Derive the session keys and MAC tags (PROTOCOL.md section 6.1). */
fun deriveKeys(
    gk: GroupKey,
    gid: GroupId,
    ephI: ByteArray,
    nI: ByteArray,
    ephR: ByteArray,
    nR: ByteArray,
    dh: ByteArray,
): Derived {
    val th = transcriptHash(gid, ephI, nI, ephR, nR)
    val ikm = dh + gk.bytes
    val hkdf = HKDFBytesGenerator(SHA256Digest())
    hkdf.init(HKDFParameters(ikm, th, KEYS_INFO))
    val okm = ByteArray(128)
    hkdf.generateBytes(okm, 0, okm.size)
    ikm.fill(0)
    val derived = Derived(
        th = th,
        kI2r = okm.copyOfRange(0, 32),
        kR2i = okm.copyOfRange(32, 64),
        macR = hmacTag(okm.copyOfRange(64, 96), "R", th),
        macI = hmacTag(okm.copyOfRange(96, 128), "I", th),
    )
    okm.fill(0)
    return derived
}

/** X25519 public key for a 32-byte secret (the scalar is clamped internally). */
fun x25519Public(secret: ByteArray): ByteArray {
    val pub = ByteArray(32)
    X25519.generatePublicKey(secret, 0, pub, 0)
    return pub
}

/** X25519 shared secret, or null when the result is the all-zero low-order value. */
fun x25519Agree(secret: ByteArray, peerPublic: ByteArray): ByteArray? {
    val out = ByteArray(32)
    return if (X25519.calculateAgreement(secret, 0, peerPublic, 0, out, 0)) out else null
}

/** An established encrypted connection. */
class SecureChannel(val input: SecureInput, val output: SecureOutput) {
    fun wipe() {
        input.wipe()
        output.wipe()
    }
}

private fun verify(expected: ByteArray, got: ByteArray) {
    if (!MessageDigest.isEqual(expected, got)) throw ProtoException.Auth()
}

private fun errorMessage(code: String) = Message.Error(code = code)

/**
 * Run the initiator side. Blocking; callers should set a socket read timeout of about
 * 10 seconds before calling (PROTOCOL.md section 6).
 */
fun initiate(
    input: InputStream,
    output: OutputStream,
    gk: GroupKey,
    ephSecret: ByteArray = ByteArray(32).also { fillRandom(it) },
    nonce: ByteArray = ByteArray(16).also { fillRandom(it) },
): SecureChannel {
    val gid = gk.groupId()
    val ephI = x25519Public(ephSecret)
    writePlainFrame(
        output,
        Message.Hs1(PROTOCOL_VERSION, gid.toHex(), b64urlEncode(ephI), b64urlEncode(nonce)),
    )
    val reply = readPlainFrame(input).message
    val (ephR, nR, macR) = when (reply) {
        is Message.Hs2 -> {
            if (reply.v != PROTOCOL_VERSION) throw ProtoException.Version(reply.v)
            Triple(b64urlDecodeExact(reply.ephR, 32), b64urlDecodeExact(reply.nR, 16), b64urlDecodeExact(reply.macR, 32))
        }
        is Message.Error -> when (reply.code) {
            "wrong_group" -> throw ProtoException.WrongGroup()
            "version" -> throw ProtoException.Version(0)
            else -> throw ProtoException.BadHeader("expected hs2")
        }
        else -> throw ProtoException.BadHeader("expected hs2")
    }
    val dh = x25519Agree(ephSecret, ephR) ?: throw ProtoException.Auth()
    val keys = deriveKeys(gk, gid, ephI, nonce, ephR, nR, dh)
    verify(keys.macR, macR)
    writePlainFrame(output, Message.Hs3(b64urlEncode(keys.macI)))
    val channel = SecureChannel(SecureInput(input, keys.kR2i), SecureOutput(output, keys.kI2r))
    keys.wipe()
    return channel
}

/** Run the responder side. Blocking; see [initiate] for timeouts. */
fun respond(
    input: InputStream,
    output: OutputStream,
    gk: GroupKey,
    ephSecret: ByteArray = ByteArray(32).also { fillRandom(it) },
    nonce: ByteArray = ByteArray(16).also { fillRandom(it) },
): SecureChannel {
    val gid = gk.groupId()
    val ephR = x25519Public(ephSecret)
    val first = readPlainFrame(input).message as? Message.Hs1 ?: throw ProtoException.BadHeader("expected hs1")
    if (first.v != PROTOCOL_VERSION) {
        runCatching { writePlainFrame(output, errorMessage("version")) }
        throw ProtoException.Version(first.v)
    }
    if (GroupId.fromHex(first.gid) != gid) {
        runCatching { writePlainFrame(output, errorMessage("wrong_group")) }
        throw ProtoException.WrongGroup()
    }
    val ephI = b64urlDecodeExact(first.ephI, 32)
    val nI = b64urlDecodeExact(first.nI, 16)
    val dh = x25519Agree(ephSecret, ephI) ?: throw ProtoException.Auth()
    val keys = deriveKeys(gk, gid, ephI, nI, ephR, nonce, dh)
    writePlainFrame(
        output,
        Message.Hs2(PROTOCOL_VERSION, b64urlEncode(ephR), b64urlEncode(nonce), b64urlEncode(keys.macR)),
    )
    val last = readPlainFrame(input).message as? Message.Hs3 ?: throw ProtoException.BadHeader("expected hs3")
    verify(keys.macI, b64urlDecodeExact(last.macI, 32))
    val channel = SecureChannel(SecureInput(input, keys.kI2r), SecureOutput(output, keys.kR2i))
    keys.wipe()
    return channel
}
