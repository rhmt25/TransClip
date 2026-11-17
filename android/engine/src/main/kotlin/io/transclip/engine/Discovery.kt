package io.transclip.engine

import io.transclip.proto.Beacon
import io.transclip.proto.DeviceId
import io.transclip.proto.GroupId
import io.transclip.proto.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.SendChannel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.SocketException

/** How a peer was discovered. */
enum class Via { BEACON, NSD }

/** A peer of our group was seen on the network. */
data class PeerSeen(
    val deviceId: DeviceId,
    val addr: InetSocketAddress,
    val platform: Platform,
    val name: String,
    val via: Via,
)

const val DEFAULT_BEACON_PORT = 47821
const val BEACON_INTERVAL_MS = 10_000L

/** Truncate to at most [max] UTF-8 bytes without splitting a character. */
fun clampName(name: String, max: Int = Beacon.MAX_NAME): String {
    var out = name
    while (out.toByteArray(Charsets.UTF_8).size > max) out = out.dropLast(1)
    return out
}

/** Beacon service settings. */
data class BeaconConfig(
    val gid: GroupId,
    val deviceId: DeviceId,
    val tcpPort: Int,
    val platform: Platform,
    val name: String,
    val listenPort: Int = DEFAULT_BEACON_PORT,
    /** Destinations for outgoing beacons; production uses the limited broadcast address. */
    val targets: List<InetSocketAddress> = listOf(InetSocketAddress(InetAddress.getByName("255.255.255.255"), DEFAULT_BEACON_PORT)),
    val intervalMs: Long = BEACON_INTERVAL_MS,
)

/** Validate a datagram; returns the peer when it is a foreign beacon of our group. */
internal fun acceptBeacon(data: ByteArray, from: InetAddress, gid: GroupId, me: DeviceId): PeerSeen? {
    val b = try {
        Beacon.decode(data)
    } catch (e: io.transclip.proto.ProtoException) {
        return null
    }
    if (b.gid != gid || b.deviceId == me || b.port == 0) return null
    return PeerSeen(b.deviceId, InetSocketAddress(from, b.port), b.platform, b.name, Via.BEACON)
}

/** Sends a beacon every 10 s (and on request) and reports beacons of our group. */
class BeaconService(
    private val cfg: BeaconConfig,
    private val out: SendChannel<PeerSeen>,
    private val scope: CoroutineScope,
) {
    private var socket: DatagramSocket? = null
    private val jobs = mutableListOf<Job>()
    private val nudge = Channel<Unit>(Channel.CONFLATED)

    val localPort: Int get() = socket?.localPort ?: 0

    fun start() {
        val s = DatagramSocket(null).apply {
            reuseAddress = true
            broadcast = true
            bind(InetSocketAddress(cfg.listenPort))
        }
        socket = s
        val packet = Beacon(cfg.gid, cfg.deviceId, cfg.tcpPort, cfg.platform, clampName(cfg.name)).encode()
        jobs += scope.launch(Dispatchers.IO) {
            while (isActive) {
                for (t in cfg.targets) {
                    runCatching { s.send(DatagramPacket(packet, packet.size, t)) }
                }
                // Wait for the interval, or earlier when announceNow() is called.
                kotlinx.coroutines.withTimeoutOrNull(cfg.intervalMs) { nudge.receive() }
            }
        }
        jobs += scope.launch(Dispatchers.IO) {
            val buf = ByteArray(Beacon.FIXED_LEN + Beacon.MAX_NAME + 1)
            while (isActive) {
                val dp = DatagramPacket(buf, buf.size)
                try {
                    s.receive(dp)
                } catch (e: SocketException) {
                    if (s.isClosed) return@launch
                    delay(50)
                    continue
                }
                val seen = acceptBeacon(buf.copyOf(dp.length), dp.address, cfg.gid, cfg.deviceId) ?: continue
                out.send(seen)
            }
        }
    }

    /** Send a beacon now (for example after a network change). */
    fun announceNow() {
        nudge.trySend(Unit)
    }

    fun stop() {
        jobs.forEach { it.cancel() }
        socket?.close()
    }
}

/** Last address that worked for each device, persisted as JSON (PROTOCOL.md section 4.3). */
class AddressCache(private val file: File?) {
    private val map = LinkedHashMap<String, String>()

    init {
        file?.takeIf { it.exists() }?.let { f ->
            runCatching {
                val json = kotlinx.serialization.json.Json.parseToJsonElement(f.readText()) as kotlinx.serialization.json.JsonObject
                val peers = json["peers"] as? kotlinx.serialization.json.JsonObject
                peers?.forEach { (k, v) ->
                    val addr = (v as? kotlinx.serialization.json.JsonPrimitive)?.content ?: return@forEach
                    if (runCatching { DeviceId.fromHex(k) }.isSuccess && parseAddr(addr) != null) map[k] = addr
                }
            }
        }
    }

    val size: Int get() = map.size

    fun get(id: DeviceId): InetSocketAddress? = map[id.toHex()]?.let(::parseAddr)

    fun remove(id: DeviceId) {
        map.remove(id.toHex())
    }

    /** Remember a successful address; persists only when it changed. */
    fun record(id: DeviceId, addr: InetSocketAddress) {
        val text = "${addr.address.hostAddress}:${addr.port}"
        if (map.put(id.toHex(), text) != text) save()
    }

    fun save() {
        val f = file ?: return
        f.parentFile?.mkdirs()
        val body = kotlinx.serialization.json.buildJsonObject {
            put(
                "peers",
                kotlinx.serialization.json.buildJsonObject {
                    map.forEach { (k, v) -> put(k, kotlinx.serialization.json.JsonPrimitive(v)) }
                },
            )
        }
        val tmp = File(f.path + ".tmp")
        tmp.writeText(body.toString())
        if (!tmp.renameTo(f)) {
            f.delete()
            tmp.renameTo(f)
        }
    }

    private fun parseAddr(s: String): InetSocketAddress? {
        val i = s.lastIndexOf(':')
        if (i <= 0) return null
        val port = s.substring(i + 1).toIntOrNull() ?: return null
        val host = s.substring(0, i)
        if (port !in 1..65535 || !host.all { it.isDigit() || it == '.' }) return null
        return runCatching { InetSocketAddress(InetAddress.getByName(host), port) }.getOrNull()
    }
}
