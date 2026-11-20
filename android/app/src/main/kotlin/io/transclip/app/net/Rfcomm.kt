package io.transclip.app.net

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import io.transclip.engine.BtDialer
import io.transclip.engine.PeerManager
import io.transclip.engine.RawConn
import io.transclip.proto.DeviceId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.UUID

private const val TAG = "TransClip"

/** Service UUID of PROTOCOL.md section 2. */
val RFCOMM_UUID: UUID = UUID.fromString("7b3c2a10-6f1e-4b8e-9d1a-5c0e2f7a4b61")
private const val SERVICE_NAME = "TransClip"

/** Bluetooth socket as a [RawConn]. It has no read time-out, so the link watches for silence. */
class BtConn(private val socket: BluetoothSocket) : RawConn {
    override val input: InputStream get() = socket.inputStream
    override val output: OutputStream get() = socket.outputStream
    override val manualTimeout: Boolean get() = true

    override fun setReadTimeout(ms: Int) = Unit

    override fun close() = socket.close()
}

/** True when the app may use Bluetooth and the adapter is on. */
fun bluetoothReady(context: Context): Boolean {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
        context.checkSelfPermission(android.Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED
    ) {
        return false
    }
    return adapterOf(context)?.isEnabled == true
}

private fun adapterOf(context: Context): BluetoothAdapter? =
    (context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

/** Accepts Bluetooth connections from paired devices and hands them to the peer manager. */
class RfcommServer(private val context: Context, private val mgr: PeerManager, private val scope: CoroutineScope) {
    @Volatile
    private var server: BluetoothServerSocket? = null

    @SuppressLint("MissingPermission")
    fun start() {
        val adapter = adapterOf(context) ?: return
        scope.launch(Dispatchers.IO) {
            while (isActive) {
                // Bluetooth may be switched on (or the permission granted) after the engine started.
                if (!bluetoothReady(context)) {
                    delay(3_000)
                    continue
                }
                val s = try {
                    adapter.listenUsingRfcommWithServiceRecord(SERVICE_NAME, RFCOMM_UUID)
                } catch (e: IOException) {
                    Log.w(TAG, "bluetooth listen failed: ${e.message}")
                    delay(10_000)
                    continue
                }
                server = s
                try {
                    while (isActive) {
                        val sock = s.accept()
                        Log.d(TAG, "bluetooth connection accepted")
                        mgr.acceptConnection(BtConn(sock))
                    }
                } catch (e: IOException) {
                    // The server socket was closed or Bluetooth was switched off; listen again.
                    runCatching { s.close() }
                    if (isActive) delay(2_000)
                }
            }
        }
    }

    fun stop() {
        runCatching { server?.close() }
    }
}

/** True when the device has a Bluetooth adapter at all (it may still be switched off). */
fun hasBluetooth(context: Context): Boolean = adapterOf(context) != null

/**
 * Connects to paired devices that run TransClip. The peer's Bluetooth address is not known, so
 * paired computers and phones are tried (headsets, controllers and the like are skipped); the
 * device that answered last time goes first. The handshake then checks who answered.
 */
class RfcommDialer(private val context: Context) : BtDialer {
    private val prefs = context.applicationContext.getSharedPreferences("bt_dialer", Context.MODE_PRIVATE)

    @SuppressLint("MissingPermission")
    private fun mayRunTransClip(d: BluetoothDevice): Boolean {
        val major = d.bluetoothClass?.majorDeviceClass ?: return true
        return major == BluetoothClass.Device.Major.COMPUTER ||
            major == BluetoothClass.Device.Major.PHONE ||
            major == BluetoothClass.Device.Major.UNCATEGORIZED
    }

    @SuppressLint("MissingPermission")
    override suspend fun connect(peer: DeviceId): RawConn {
        val adapter = adapterOf(context) ?: throw IOException("no bluetooth adapter")
        if (!bluetoothReady(context)) throw IOException("bluetooth not available")
        runCatching { adapter.cancelDiscovery() }
        val lastGood = prefs.getString(LAST_GOOD, null)
        val candidates = adapter.bondedDevices.orEmpty()
            .filter(::mayRunTransClip)
            .sortedBy { it.address != lastGood }
        if (candidates.isEmpty()) {
            Log.w(TAG, "no paired computer or phone; pair this phone with the PC in Bluetooth settings")
        }
        var last: Exception? = null
        for (device in candidates) {
            val sock = device.createRfcommSocketToServiceRecord(RFCOMM_UUID)
            try {
                withContext(Dispatchers.IO) {
                    // connect() cannot be cancelled; closing the socket is what ends it early.
                    val watchdog = launch {
                        delay(PER_DEVICE_TIMEOUT_MS)
                        runCatching { sock.close() }
                    }
                    try {
                        sock.connect()
                    } finally {
                        watchdog.cancel()
                    }
                }
                Log.d(TAG, "bluetooth connected to a paired device")
                prefs.edit().putString(LAST_GOOD, device.address).apply()
                return BtConn(sock)
            } catch (e: Exception) {
                last = e
                runCatching { sock.close() }
            }
        }
        throw last ?: IOException("no paired computers or phones")
    }

    private companion object {
        const val LAST_GOOD = "last_good"
        const val PER_DEVICE_TIMEOUT_MS = 5_000L
    }
}
