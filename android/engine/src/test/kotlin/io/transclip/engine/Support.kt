package io.transclip.engine

import io.transclip.proto.DeviceId
import io.transclip.proto.GroupKey
import io.transclip.proto.SecureChannel
import io.transclip.proto.initiate
import io.transclip.proto.respond
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket

fun localInfo(id: Int, name: String = "dev$id") =
    LocalInfo(DeviceId(ByteArray(16) { id.toByte() }), name, "android", "test", emptyList(), "full")

fun deviceId(id: Int) = DeviceId(ByteArray(16) { id.toByte() })

fun groupKey(b: Int = 7) = GroupKey(ByteArray(32) { b.toByte() })

/** Two connected sockets with completed handshakes (initiator side, responder side). */
data class SocketPair(
    val a: Socket,
    val b: Socket,
    val aChannel: SecureChannel,
    val bChannel: SecureChannel,
)

suspend fun connectedPair(gk: GroupKey = groupKey()): SocketPair = withContext(Dispatchers.IO) {
    ServerSocket(0, 1, InetAddress.getLoopbackAddress()).use { server ->
        val accepted = async { server.accept() }
        val a = Socket(InetAddress.getLoopbackAddress(), server.localPort)
        val b = accepted.await()
        a.tcpNoDelay = true
        b.tcpNoDelay = true
        val respondSide = async { respond(b.getInputStream(), b.getOutputStream(), gk) }
        val aChan = initiate(a.getInputStream(), a.getOutputStream(), gk)
        SocketPair(a, b, aChan, respondSide.await())
    }
}

suspend fun <T> within(ms: Long = 5_000, block: suspend () -> T): T = withTimeout(ms) { block() }
