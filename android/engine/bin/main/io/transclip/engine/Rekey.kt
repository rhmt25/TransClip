package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.GroupKey
import io.transclip.proto.Message
import io.transclip.proto.b64urlDecodeExact
import io.transclip.proto.b64urlEncode

/** A rotation request received from a peer (PROTOCOL.md section 9.4). */
class RekeyRequest(val newGk: GroupKey, val name: String, val by: DeviceId, val ts: Long)

object Rekey {
    /** Read a `rekey` message; null for other messages or an invalid key. */
    fun parse(m: Message): RekeyRequest? {
        if (m !is Message.Rekey) return null
        return runCatching {
            RekeyRequest(GroupKey(b64urlDecodeExact(m.newGk, 32)), m.name, DeviceId.fromHex(m.by), m.ts)
        }.getOrNull()
    }

    fun ack(req: RekeyRequest): Message = Message.RekeyAck(req.ts)

    /** Send the new key to [targets] (revoked peers are left out). Returns how many accepted it. */
    suspend fun broadcast(mgr: PeerManager, newGk: GroupKey, name: String, by: DeviceId, targets: List<DeviceId>): Int {
        val ts = System.currentTimeMillis() / 1000
        var sent = 0
        for (peer in targets) {
            val msg = Message.Rekey(b64urlEncode(newGk.bytes), name, by.toHex(), ts)
            if (runCatching { mgr.send(peer, msg) }.isSuccess) sent++
        }
        return sent
    }
}
