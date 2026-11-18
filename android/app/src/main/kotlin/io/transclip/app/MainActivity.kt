package io.transclip.app

import android.Manifest
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.transclip.app.clip.ClipCaptureActivity
import io.transclip.app.clip.ORIGIN_KEY
import io.transclip.app.service.SyncService
import io.transclip.app.service.TCP_PORT
import io.transclip.engine.PeerState
import io.transclip.proto.GroupKey
import io.transclip.proto.PairingInfo
import io.transclip.proto.ProtoException
import android.graphics.Bitmap
import android.graphics.Color
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.compose.ui.graphics.asImageBitmap
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import androidx.compose.material3.Checkbox
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Alignment
import io.transclip.app.share.copyToCache
import io.transclip.app.storage.DEFAULT_RELATIVE_ROOT
import io.transclip.app.storage.SettingsRepo
import io.transclip.app.storage.TargetDirRepo
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.NetworkInterface

class MainActivity : ComponentActivity() {
    /** Code from a `transclip://pair` link; shown in the field, never applied without a tap. */
    private var prefill by mutableStateOf("")

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.toString()?.let { prefill = it }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.data?.toString()?.let { prefill = it }
        setContent {
            MaterialTheme {
                Surface(Modifier.fillMaxSize()) { HomeScreen() }
            }
        }
    }

    @Composable
    private fun HomeScreen() {
        val app = application as TransClipApp
        val scope = rememberCoroutineScope()
        val peers by AppState.peers.collectAsStateWithLifecycle()
        val paired by AppState.paired.collectAsStateWithLifecycle()
        val paused by AppState.paused.collectAsStateWithLifecycle()
        val history by AppState.history.collectAsStateWithLifecycle()
        val transfers by AppState.transfers.collectAsStateWithLifecycle()
        var code by remember(prefill) { mutableStateOf(prefill) }
        var message by remember { mutableStateOf("") }
        var invite by remember { mutableStateOf("") }

        val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
            if (uris.isNotEmpty()) {
                scope.launch {
                    val files = withContext(Dispatchers.IO) { uris.mapNotNull { copyToCache(this@MainActivity, it) } }
                    val n = app.engine.sendFiles(files)
                    message = getString(if (n > 0) R.string.share_sent else R.string.share_no_device, n)
                }
            }
        }
        val settingsRepo = remember { SettingsRepo(this@MainActivity) }
        val targetRepo = remember { TargetDirRepo(this@MainActivity, SettingsRepo(this@MainActivity)) }
        var targetLabel by remember { mutableStateOf(DEFAULT_RELATIVE_ROOT) }
        LaunchedEffect(Unit) { targetLabel = targetRepo.label() }
        val pickTree = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            if (uri != null) {
                scope.launch {
                    targetRepo.choose(uri)
                    targetLabel = targetRepo.label()
                    // The receive folder is bound when the engine starts.
                    app.engine.stop()
                    app.engine.start()
                }
            }
        }
        var rotateOpen by remember { mutableStateOf(false) }
        val revoked = remember { mutableStateListOf<io.transclip.proto.DeviceId>() }
        if (rotateOpen) {
            AlertDialog(
                onDismissRequest = { rotateOpen = false },
                title = { Text(getString(R.string.rotate_title)) },
                text = {
                    Column {
                        Text(getString(R.string.rotate_body))
                        peers.forEach { p ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(checked = p.id in revoked, onCheckedChange = { on ->
                                    if (on) revoked.add(p.id) else revoked.remove(p.id)
                                })
                                Text(p.name.ifEmpty { p.id.toHex().take(8) })
                            }
                        }
                    }
                },
                confirmButton = {
                    TextButton(onClick = {
                        rotateOpen = false
                        val gone = revoked.toList()
                        revoked.clear()
                        scope.launch { message = getString(R.string.rotate_done, app.engine.rotateKey(gone)) }
                    }) { Text(getString(R.string.rotate_confirm)) }
                },
                dismissButton = { TextButton(onClick = { rotateOpen = false }) { Text(getString(R.string.cancel)) } },
            )
        }
        var pendingJoin by remember { mutableStateOf<String?>(null) }
        val doJoin: (String) -> Unit = { text ->
            scope.launch {
                message = try {
                    val info = PairingInfo.parse(text)
                    if (info.isExpired(System.currentTimeMillis() / 1000)) {
                        getString(R.string.pairing_expired)
                    } else {
                        app.secrets.setGroupKey(info.gk)
                        code = ""
                        SyncService.start(this@MainActivity, SyncService.ACTION_RESTART)
                        getString(R.string.pairing_joined)
                    }
                } catch (e: ProtoException) {
                    getString(R.string.pairing_invalid)
                }
            }
        }
        // Joining replaces the current group, so ask first when this device already has one.
        val requestJoin: (String) -> Unit = { text ->
            scope.launch { if (app.secrets.groupKey() != null) pendingJoin = text else doJoin(text) }
        }
        val scan = rememberLauncherForActivityResult(ScanContract()) { result ->
            result.contents?.let {
                code = it
                requestJoin(it)
            }
        }
        pendingJoin?.let { text ->
            AlertDialog(
                onDismissRequest = { pendingJoin = null },
                title = { Text(getString(R.string.pairing_replace_title)) },
                text = { Text(getString(R.string.pairing_replace_body)) },
                confirmButton = {
                    TextButton(onClick = {
                        pendingJoin = null
                        doJoin(text)
                    }) { Text(getString(R.string.pairing_replace_confirm)) }
                },
                dismissButton = { TextButton(onClick = { pendingJoin = null }) { Text(getString(R.string.cancel)) } },
            )
        }
        val askPhotos = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) {
                scope.launch {
                    settingsRepo.update { it.copy(autoScreenshots = true) }
                    app.engine.stop()
                    app.engine.start()
                }
            }
        }
        val askBluetooth = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) {
                scope.launch {
                    app.engine.stop()
                    app.engine.start()
                }
            }
        }
        val askNotifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
            SyncService.start(this)
        }
        LaunchedEffect(Unit) {
            AppState.setPaired(app.secrets.groupKey() != null)
            AppState.setPaused(app.settings.get().paused)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                ContextCompat.checkSelfPermission(this@MainActivity, Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
            ) {
                askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
            } else {
                SyncService.start(this@MainActivity)
            }
        }

        LazyColumn(
            Modifier.fillMaxSize().padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item {
                Text(getString(R.string.app_name), style = MaterialTheme.typography.headlineMedium)
                val online = peers.count { it.state == PeerState.ONLINE }
                Text(
                    when {
                        !paired -> getString(R.string.status_not_paired)
                        paused -> getString(R.string.status_paused)
                        else -> resources.getQuantityString(R.plurals.status_online, online, online)
                    },
                )
            }
            if (paired) {
                item {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(onClick = {
                            startActivity(Intent(this@MainActivity, ClipCaptureActivity::class.java))
                        }) { Text(getString(R.string.action_send_clip)) }
                        OutlinedButton(onClick = { pickFiles.launch(arrayOf("*/*")) }) {
                            Text(getString(R.string.action_send_files))
                        }
                        OutlinedButton(onClick = {
                            scope.launch {
                                val next = !AppState.paused.value
                                app.settings.update { it.copy(paused = next) }
                                AppState.setPaused(next)
                            }
                        }) { Text(getString(if (paused) R.string.action_resume else R.string.action_pause)) }
                    }
                }
                items(peers) { p ->
                    Column(Modifier.fillMaxWidth()) {
                        Text(p.name.ifEmpty { p.id.toHex().take(8) })
                        val via = when (p.transport) {
                            io.transclip.engine.Transport.BLUETOOTH -> " · Bluetooth"
                            io.transclip.engine.Transport.LAN -> " · Wi-Fi"
                            null -> ""
                        }
                        Text(p.state.name.lowercase() + via, style = MaterialTheme.typography.bodySmall)
                        val prio by settingsRepo.flow.collectAsStateWithLifecycle(initialValue = null)
                        prio?.let { c ->
                            SettingSwitch(getString(R.string.bt_priority), p.id.toHex() in c.btPriority) { on ->
                                val needsPermission = on && android.os.Build.VERSION.SDK_INT >= 31 &&
                                    checkSelfPermission(android.Manifest.permission.BLUETOOTH_CONNECT) !=
                                    android.content.pm.PackageManager.PERMISSION_GRANTED
                                if (needsPermission) {
                                    message = getString(R.string.bt_permission_needed)
                                    askBluetooth.launch(android.Manifest.permission.BLUETOOTH_CONNECT)
                                } else {
                                    scope.launch {
                                        settingsRepo.update {
                                            it.copy(btPriority = if (on) it.btPriority + p.id.toHex() else it.btPriority - p.id.toHex())
                                        }
                                        app.engine.stop()
                                        app.engine.start()
                                    }
                                }
                            }
                        }
                    }
                }
            }
            item {
                val cfg by settingsRepo.flow.collectAsStateWithLifecycle(initialValue = null)
                cfg?.let { c ->
                    var nameEdit by remember(c.deviceName) { mutableStateOf(c.deviceName) }
                    fun apply(change: (io.transclip.app.storage.Settings) -> io.transclip.app.storage.Settings) {
                        scope.launch {
                            settingsRepo.update(change)
                            // Settings are read when the engine starts.
                            app.engine.stop()
                            app.engine.start()
                        }
                    }
                    Text(getString(R.string.settings_title), style = MaterialTheme.typography.titleMedium)
                    OutlinedTextField(
                        value = nameEdit,
                        onValueChange = { nameEdit = it },
                        label = { Text(getString(R.string.settings_name)) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedButton(onClick = { apply { it.copy(deviceName = nameEdit) } }) {
                        Text(getString(R.string.settings_apply))
                    }
                    SettingSwitch(getString(R.string.settings_sync_text), c.syncText) { v -> apply { it.copy(syncText = v) } }
                    SettingSwitch(getString(R.string.settings_sync_images), c.syncImages) { v -> apply { it.copy(syncImages = v) } }
                    SettingSwitch(getString(R.string.settings_receive_files), c.receiveFiles) { v -> apply { it.copy(receiveFiles = v) } }
                    SettingSwitch(getString(R.string.settings_screenshots), c.autoScreenshots) { v ->
                        if (v) {
                            val perm = if (android.os.Build.VERSION.SDK_INT >= 33) {
                                android.Manifest.permission.READ_MEDIA_IMAGES
                            } else {
                                android.Manifest.permission.READ_EXTERNAL_STORAGE
                            }
                            askPhotos.launch(perm)
                        } else {
                            apply { it.copy(autoScreenshots = false) }
                        }
                    }
                    SettingSwitch(getString(R.string.settings_autostart), c.autoStart) { v -> apply { it.copy(autoStart = v) } }
                }
            }
            item {
                val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
                var autoOn by remember { mutableStateOf(io.transclip.app.clip.ClipAccessibilityService.isEnabled(this@MainActivity)) }
                androidx.compose.runtime.DisposableEffect(lifecycleOwner) {
                    val obs = androidx.lifecycle.LifecycleEventObserver { _, e ->
                        if (e == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                            autoOn = io.transclip.app.clip.ClipAccessibilityService.isEnabled(this@MainActivity)
                        }
                    }
                    lifecycleOwner.lifecycle.addObserver(obs)
                    onDispose { lifecycleOwner.lifecycle.removeObserver(obs) }
                }
                Text(getString(R.string.auto_capture_title), style = MaterialTheme.typography.titleMedium)
                Text(getString(if (autoOn) R.string.auto_capture_on else R.string.auto_capture_off), style = MaterialTheme.typography.bodySmall)
                if (!autoOn) {
                    OutlinedButton(onClick = {
                        startActivity(android.content.Intent(android.provider.Settings.ACTION_ACCESSIBILITY_SETTINGS))
                    }) { Text(getString(R.string.auto_capture_open)) }
                }
            }
            item {
                Text(getString(R.string.target_dir_title), style = MaterialTheme.typography.titleMedium)
                Text(targetLabel, style = MaterialTheme.typography.bodySmall)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = { pickTree.launch(null) }) { Text(getString(R.string.target_dir_change)) }
                    OutlinedButton(onClick = {
                        scope.launch {
                            targetRepo.reset()
                            targetLabel = targetRepo.label()
                            app.engine.stop()
                            app.engine.start()
                        }
                    }) { Text(getString(R.string.target_dir_reset)) }
                }
            }
            item {
                Text(getString(R.string.pairing_title), style = MaterialTheme.typography.titleMedium)
                OutlinedTextField(
                    value = code,
                    onValueChange = { code = it },
                    label = { Text(getString(R.string.pairing_code_hint)) },
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { requestJoin(code) }) { Text(getString(R.string.pairing_join)) }
                    OutlinedButton(onClick = {
                        scan.launch(
                            ScanOptions()
                                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                                .setPrompt(getString(R.string.pairing_scan_prompt))
                                .setBeepEnabled(false)
                                .setOrientationLocked(false),
                        )
                    }) { Text(getString(R.string.pairing_scan)) }
                    OutlinedButton(onClick = {
                        scope.launch {
                            val gk = app.secrets.groupKey() ?: GroupKey.generate().also { app.secrets.setGroupKey(it) }
                            val info = PairingInfo(
                                gk = gk,
                                groupName = "TransClip",
                                inviter = app.secrets.deviceId(),
                                addrs = localAddresses(),
                                expires = System.currentTimeMillis() / 1000 + 120,
                            )
                            invite = info.toUri()
                            copyInvite(invite)
                            SyncService.start(this@MainActivity, SyncService.ACTION_RESTART)
                            message = getString(R.string.pairing_created)
                        }
                    }) { Text(getString(R.string.pairing_create)) }
                }
                if (paired) OutlinedButton(onClick = { rotateOpen = true }) { Text(getString(R.string.rotate_title)) }
                if (message.isNotEmpty()) Text(message)
                if (invite.isNotEmpty()) {
                    qrBitmap(invite)?.let {
                        Image(it.asImageBitmap(), contentDescription = null, modifier = Modifier.size(240.dp))
                    }
                    Text(invite, style = MaterialTheme.typography.bodySmall)
                }
            }
            if (transfers.isNotEmpty()) {
                item { Text(getString(R.string.transfers_title), style = MaterialTheme.typography.titleMedium) }
                items(transfers) { t ->
                    Column(Modifier.fillMaxWidth()) {
                        Text((if (t.incoming) "↓ " else "↑ ") + t.name, style = MaterialTheme.typography.bodyMedium)
                        androidx.compose.material3.LinearProgressIndicator(
                            progress = { if (t.total > 0) (t.done.toFloat() / t.total).coerceIn(0f, 1f) else 0f },
                            modifier = Modifier.fillMaxWidth(),
                        )
                        TextButton(onClick = { scope.launch { app.engine.cancelTransfer(t) } }) {
                            Text(getString(R.string.transfer_cancel))
                        }
                    }
                }
            }
            if (history.isNotEmpty()) {
                item { Text(getString(R.string.history_title), style = MaterialTheme.typography.titleMedium) }
                item { TextButton(onClick = { AppState.clearHistory() }) { Text(getString(R.string.history_clear)) } }
                items(history) { Text(it.text, style = MaterialTheme.typography.bodySmall) }
            }
        }
    }

    /** Copy the invitation without letting it be synced or kept in the clipboard history. */
    private fun copyInvite(uri: String) {
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val data = ClipData.newPlainText("TransClip pairing", uri)
        data.description.extras = PersistableBundle().apply {
            putString(ORIGIN_KEY, "pairing")
            putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
        }
        cm.setPrimaryClip(data)
    }

    private fun localAddresses(): List<String> = runCatching {
        NetworkInterface.getNetworkInterfaces().toList()
            .filter { it.isUp && !it.isLoopback }
            .flatMap { it.inetAddresses.toList() }
            .filterIsInstance<java.net.Inet4Address>()
            .map { "${it.hostAddress}:$TCP_PORT" }
    }.getOrDefault(emptyList())
}

/** Render [text] as a QR code bitmap, black on white. Null when it does not fit. */
private fun qrBitmap(text: String, size: Int = 640): Bitmap? = runCatching {
    val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size)
    val px = IntArray(size * size) { i -> if (m.get(i % size, i / size)) Color.BLACK else Color.WHITE }
    Bitmap.createBitmap(px, size, size, Bitmap.Config.ARGB_8888)
}.getOrNull()

@androidx.compose.runtime.Composable
private fun SettingSwitch(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label)
        androidx.compose.material3.Switch(checked = checked, onCheckedChange = onChange)
    }
}
