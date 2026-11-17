package io.transclip.app.net

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import io.transclip.engine.PeerSeen
import io.transclip.engine.Via
import io.transclip.engine.clampName
import io.transclip.proto.DeviceId
import io.transclip.proto.GroupId
import io.transclip.proto.Platform
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.SendChannel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.net.InetSocketAddress

private const val TAG = "TransClip"
const val SERVICE_TYPE = "_transclip._tcp."

/**
 * mDNS registration and browsing through [NsdManager]. The old resolve API handles only one
 * resolve at a time, so found services are resolved from a queue, one after another.
 */
class NsdDiscovery(
    context: Context,
    private val gid: GroupId,
    private val me: DeviceId,
    private val tcpPort: Int,
    private val name: String,
    private val out: SendChannel<PeerSeen>,
    private val scope: CoroutineScope,
) {
    private val nsd = context.applicationContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val toResolve = Channel<NsdServiceInfo>(Channel.UNLIMITED)
    private var worker: Job? = null
    private var registration: NsdManager.RegistrationListener? = null
    private var discovery: NsdManager.DiscoveryListener? = null

    fun start() {
        val info = NsdServiceInfo().apply {
            serviceName = clampName(name, 63)
            serviceType = SERVICE_TYPE
            port = tcpPort
            setAttribute("v", "1")
            setAttribute("id", me.toHex())
            setAttribute("gid", gid.toHex())
            setAttribute("pl", "android")
            setAttribute("nm", clampName(name))
        }
        val reg = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(i: NsdServiceInfo) {
                Log.d(TAG, "NSD registered")
            }
            override fun onRegistrationFailed(i: NsdServiceInfo, code: Int) {
                Log.w(TAG, "NSD registration failed: $code")
            }
            override fun onServiceUnregistered(i: NsdServiceInfo) {}
            override fun onUnregistrationFailed(i: NsdServiceInfo, code: Int) {}
        }
        registration = reg
        runCatching { nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, reg) }
            .onFailure { Log.w(TAG, "NSD register: ${it.message}") }

        val disc = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(type: String) {}
            override fun onDiscoveryStopped(type: String) {}
            override fun onStartDiscoveryFailed(type: String, code: Int) {
                Log.w(TAG, "NSD discovery failed: $code")
            }
            override fun onStopDiscoveryFailed(type: String, code: Int) {}
            override fun onServiceFound(s: NsdServiceInfo) {
                toResolve.trySend(s)
            }
            override fun onServiceLost(s: NsdServiceInfo) {}
        }
        discovery = disc
        runCatching { nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, disc) }
            .onFailure { Log.w(TAG, "NSD discover: ${it.message}") }

        worker = scope.launch {
            for (found in toResolve) {
                resolve(found)?.let { out.send(it) }
            }
        }
    }

    @Suppress("DEPRECATION")
    private suspend fun resolve(found: NsdServiceInfo): PeerSeen? {
        val done = CompletableDeferred<NsdServiceInfo?>()
        val listener = object : NsdManager.ResolveListener {
            override fun onResolveFailed(i: NsdServiceInfo, code: Int) {
                done.complete(null)
            }
            override fun onServiceResolved(i: NsdServiceInfo) {
                done.complete(i)
            }
        }
        try {
            nsd.resolveService(found, listener)
        } catch (e: Exception) {
            return null
        }
        val info = withTimeoutOrNull(5_000) { done.await() } ?: return null
        return toPeerSeen(info)
    }

    @Suppress("DEPRECATION")
    private fun toPeerSeen(info: NsdServiceInfo): PeerSeen? {
        fun attr(key: String) = info.attributes[key]?.toString(Charsets.UTF_8)
        if (attr("v") != "1") return null
        val theirGid = runCatching { GroupId.fromHex(attr("gid") ?: return null) }.getOrNull() ?: return null
        val id = runCatching { DeviceId.fromHex(attr("id") ?: return null) }.getOrNull() ?: return null
        if (theirGid != gid || id == me || info.port == 0) return null
        val platform = when (attr("pl")) {
            "windows" -> Platform.WINDOWS
            "android" -> Platform.ANDROID
            else -> return null
        }
        val host = info.host ?: return null
        return PeerSeen(id, InetSocketAddress(host, info.port), platform, clampName(attr("nm").orEmpty()), Via.NSD)
    }

    fun stop() {
        worker?.cancel()
        registration?.let { runCatching { nsd.unregisterService(it) } }
        discovery?.let { runCatching { nsd.stopServiceDiscovery(it) } }
    }
}
