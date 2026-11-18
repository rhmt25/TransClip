package io.transclip.engine

import java.io.Closeable
import java.io.InputStream
import java.io.OutputStream
import java.net.Socket

/** A byte stream to a peer: a TCP socket, a Bluetooth RFCOMM socket, or a test pipe. */
interface RawConn : Closeable {
    val input: InputStream
    val output: OutputStream

    /** True when [setReadTimeout] has no effect and the link must watch for silence itself. */
    val manualTimeout: Boolean get() = false

    fun setReadTimeout(ms: Int)
}

/** TCP socket as a [RawConn]. */
class SocketConn(private val socket: Socket) : RawConn {
    override val input: InputStream get() = socket.getInputStream()
    override val output: OutputStream get() = socket.getOutputStream()

    override fun setReadTimeout(ms: Int) {
        socket.soTimeout = ms
    }

    override fun close() = socket.close()
}
