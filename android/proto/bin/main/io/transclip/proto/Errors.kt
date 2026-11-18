package io.transclip.proto

import java.io.IOException

/** Everything that can go wrong while speaking the TransClip protocol. */
sealed class ProtoException(message: String) : IOException(message) {
    /** A size or count limit from PROTOCOL.md section 10 was exceeded. */
    class Limit(detail: String) : ProtoException("limit exceeded: $detail")

    /** A frame header or message was malformed. */
    class BadHeader(detail: String) : ProtoException("bad header: $detail")

    /** Authentication or decryption failed. Deliberately carries no detail. */
    class Auth : ProtoException("authentication failed")

    /** The peer belongs to a different group. */
    class WrongGroup : ProtoException("wrong group")

    /** The peer speaks a protocol version we do not support. */
    class Version(val version: Long) : ProtoException("unsupported protocol version $version")

    /** The other side closed the connection. */
    class Closed : ProtoException("connection closed")

    /** A deadline passed. */
    class Timeout : ProtoException("timeout")
}
