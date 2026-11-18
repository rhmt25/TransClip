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
import android.provider.Settings as AndroidSettings
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import io.transclip.app.clip.ClipAccessibilityService
import io.transclip.app.clip.ClipCaptureActivity
import io.transclip.app.clip.ORIGIN_KEY
import io.transclip.app.service.SyncService
import io.transclip.app.service.TCP_PORT
import io.transclip.app.share.copyToCache
import io.transclip.app.storage.DEFAULT_RELATIVE_ROOT
import io.transclip.app.storage.SettingsRepo
import io.transclip.app.storage.TargetDirRepo
import io.transclip.app.ui.Checkbox
import io.transclip.app.ui.HomeActions
import io.transclip.app.ui.HomeScreen
import io.transclip.app.ui.PairingActions
import io.transclip.app.ui.PairingContent
import io.transclip.app.ui.SettingsActions
import io.transclip.app.ui.SettingsScreen
import io.transclip.app.ui.ThemeMode
import io.transclip.app.ui.ThemeToggle
import io.transclip.app.ui.TransClipTheme
import io.transclip.app.ui.tokens
import io.transclip.proto.DeviceId
import io.transclip.proto.GroupKey
import io.transclip.proto.PairingInfo
import io.transclip.proto.ProtoException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.NetworkInterface

class MainActivity : ComponentActivity() {
    /** Code from a `transclip://pair` link; shown in the field, never applied without a tap. */
    private var prefill by mutableStateOf("")
    private var openPairingTab by mutableStateOf(false)

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.toString()?.let {
            prefill = it
            openPairingTab = true
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.data?.toString()?.let {
            prefill = it
            openPairingTab = true
        }
        setContent { App() }
    }

    @OptIn(ExperimentalMaterial3Api::class)
    @Composable
    private fun App() {
        val app = application as TransClipApp
        val scope = rememberCoroutineScope()
        val settingsRepo = remember { SettingsRepo(this@MainActivity) }
        val settings by settingsRepo.flow.collectAsStateWithLifecycle(initialValue = null)
        val themeMode = ThemeMode.parse(settings?.themeMode ?: "auto")

        val peers by AppState.peers.collectAsStateWithLifecycle()
        val paired by AppState.paired.collectAsStateWithLifecycle()
        val paused by AppState.paused.collectAsStateWithLifecycle()
        val history by AppState.history.collectAsStateWithLifecycle()
        val transfers by AppState.transfers.collectAsStateWithLifecycle()

        var tab by rememberSaveable { mutableIntStateOf(0) }
        var pairingOpen by remember { mutableStateOf(false) }
        var code by remember(prefill) { mutableStateOf(prefill) }
        var invite by remember { mutableStateOf("") }
        var inviteExpiresAt by remember { mutableLongStateOf(0L) }
        val snackbar = remember { SnackbarHostState() }
        fun say(text: String) {
            scope.launch {
                snackbar.currentSnackbarData?.dismiss()
                snackbar.showSnackbar(text)
            }
        }
        LaunchedEffect(openPairingTab) {
            if (openPairingTab) {
                pairingOpen = true
                openPairingTab = false
            }
        }

        // Settings and the receive folder are read when the engine starts.
        fun restartEngine() {
            scope.launch {
                app.engine.stop()
                app.engine.start()
            }
        }
        fun apply(change: (io.transclip.app.storage.Settings) -> io.transclip.app.storage.Settings) {
            scope.launch {
                settingsRepo.update(change)
                app.engine.stop()
                app.engine.start()
            }
        }

        // --- accessibility status, refreshed whenever the app returns to the foreground
        val lifecycleOwner = LocalLifecycleOwner.current
        var autoOn by remember { mutableStateOf(ClipAccessibilityService.isEnabled(this@MainActivity)) }
        DisposableEffect(lifecycleOwner) {
            val obs = LifecycleEventObserver { _, e ->
                if (e == Lifecycle.Event.ON_RESUME) autoOn = ClipAccessibilityService.isEnabled(this@MainActivity)
            }
            lifecycleOwner.lifecycle.addObserver(obs)
            onDispose { lifecycleOwner.lifecycle.removeObserver(obs) }
        }

        // --- receive folder
        val targetRepo = remember { TargetDirRepo(this@MainActivity, SettingsRepo(this@MainActivity)) }
        var targetLabel by remember { mutableStateOf(DEFAULT_RELATIVE_ROOT) }
        LaunchedEffect(settings?.downloadTreeUri) { targetLabel = targetRepo.label() }
        val pickTree = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            if (uri != null) {
                scope.launch {
                    targetRepo.choose(uri)
                    targetLabel = targetRepo.label()
                    restartEngine()
                }
            }
        }

        // --- sending files
        val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
            if (uris.isNotEmpty()) {
                scope.launch {
                    val files = withContext(Dispatchers.IO) { uris.mapNotNull { copyToCache(this@MainActivity, it) } }
                    val n = app.engine.sendFiles(files)
                    say(getString(if (n > 0) R.string.share_sent else R.string.share_no_device, n))
                }
            }
        }

        // --- pairing
        var pendingJoin by remember { mutableStateOf<String?>(null) }
        val doJoin: (String) -> Unit = { text ->
            scope.launch {
                say(
                    try {
                        val info = PairingInfo.parse(text)
                        if (info.isExpired(System.currentTimeMillis() / 1000)) {
                            getString(R.string.pairing_expired)
                        } else {
                            app.secrets.setGroupKey(info.gk)
                            code = ""
                            SyncService.start(this@MainActivity, SyncService.ACTION_RESTART)
                            pairingOpen = false
                            getString(R.string.pairing_joined)
                        }
                    } catch (e: ProtoException) {
                        getString(R.string.pairing_invalid)
                    },
                )
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
                title = { Text(stringResource(R.string.pairing_replace_title)) },
                text = { Text(stringResource(R.string.pairing_replace_body)) },
                confirmButton = {
                    TextButton(onClick = {
                        pendingJoin = null
                        doJoin(text)
                    }) { Text(stringResource(R.string.pairing_replace_confirm), color = tokens.err) }
                },
                dismissButton = { TextButton(onClick = { pendingJoin = null }) { Text(stringResource(R.string.cancel)) } },
            )
        }

        // --- key rotation
        var rotateOpen by remember { mutableStateOf(false) }
        val revoked = remember { mutableStateListOf<DeviceId>() }
        if (rotateOpen) {
            AlertDialog(
                onDismissRequest = { rotateOpen = false },
                title = { Text(stringResource(R.string.rotate_title)) },
                text = {
                    Column {
                        Text(stringResource(R.string.rotate_body))
                        peers.forEach { p ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(p.id in revoked) { on -> if (on) revoked.add(p.id) else revoked.remove(p.id) }
                                Spacer(Modifier.width(8.dp))
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
                        scope.launch { say(getString(R.string.rotate_done, app.engine.rotateKey(gone))) }
                    }) { Text(stringResource(R.string.rotate_confirm), color = tokens.err) }
                },
                dismissButton = { TextButton(onClick = { rotateOpen = false }) { Text(stringResource(R.string.cancel)) } },
            )
        }

        // --- permissions
        val askPhotos = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) apply { it.copy(autoScreenshots = true) }
        }
        val askBluetooth = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) restartEngine()
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

        val home = HomeActions(
            sendClipboard = { startActivity(Intent(this@MainActivity, ClipCaptureActivity::class.java)) },
            pickFiles = { pickFiles.launch(arrayOf("*/*")) },
            togglePause = {
                scope.launch {
                    val next = !AppState.paused.value
                    app.settings.update { it.copy(paused = next) }
                    AppState.setPaused(next)
                }
            },
            cancel = { t -> scope.launch { app.engine.cancelTransfer(t) } },
            clearHistory = { AppState.clearHistory() },
            openPairing = { pairingOpen = true },
        )
        val pairing = PairingActions(
            scan = {
                scan.launch(
                    ScanOptions()
                        .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                        .setPrompt(getString(R.string.pairing_scan_prompt))
                        .setBeepEnabled(false)
                        .setOrientationLocked(false),
                )
            },
            join = requestJoin,
            showCode = {
                scope.launch {
                    val gk = app.secrets.groupKey() ?: GroupKey.generate().also { app.secrets.setGroupKey(it) }
                    val expires = System.currentTimeMillis() / 1000 + 120
                    invite = PairingInfo(
                        gk = gk,
                        groupName = "TransClip",
                        inviter = app.secrets.deviceId(),
                        addrs = localAddresses(),
                        expires = expires,
                    ).toUri()
                    inviteExpiresAt = expires * 1000
                    SyncService.start(this@MainActivity, SyncService.ACTION_RESTART)
                }
            },
            copyCode = {
                copyInvite(invite)
                say(getString(R.string.pairing_created))
            },
            rotate = { rotateOpen = true },
            onCodeChange = { code = it },
        )
        val settingsActions = SettingsActions(
            applyName = { n -> apply { it.copy(deviceName = n) } },
            setSyncText = { v -> apply { it.copy(syncText = v) } },
            setSyncImages = { v -> apply { it.copy(syncImages = v) } },
            setReceiveFiles = { v -> apply { it.copy(receiveFiles = v) } },
            setScreenshots = { v ->
                if (v) {
                    askPhotos.launch(
                        if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_IMAGES else Manifest.permission.READ_EXTERNAL_STORAGE,
                    )
                } else {
                    apply { it.copy(autoScreenshots = false) }
                }
            },
            setAutostart = { v -> apply { it.copy(autoStart = v) } },
            openAccessibility = { startActivity(Intent(AndroidSettings.ACTION_ACCESSIBILITY_SETTINGS)) },
            pickFolder = { pickTree.launch(null) },
            resetFolder = {
                scope.launch {
                    targetRepo.reset()
                    targetLabel = targetRepo.label()
                    restartEngine()
                }
            },
            setBluetooth = { p, on ->
                val needsPermission = on && Build.VERSION.SDK_INT >= 31 &&
                    checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED
                if (needsPermission) {
                    say(getString(R.string.bt_permission_needed))
                    askBluetooth.launch(Manifest.permission.BLUETOOTH_CONNECT)
                } else {
                    apply { it.copy(btPriority = if (on) it.btPriority + p.id.toHex() else it.btPriority - p.id.toHex()) }
                }
            },
            setTheme = { m -> scope.launch { settingsRepo.update { it.copy(themeMode = m.key) } } },
        )

        TransClipTheme(themeMode) {
            val t = tokens
            Scaffold(
                modifier = Modifier.fillMaxSize(),
                containerColor = t.canvas,
                snackbarHost = { SnackbarHost(snackbar) },
                topBar = {
                    CenterAlignedTopAppBar(
                        title = {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Image(painterResource(R.drawable.logo), contentDescription = null, modifier = Modifier.size(28.dp))
                                Spacer(Modifier.width(10.dp))
                                Text(stringResource(R.string.app_name), style = androidx.compose.material3.MaterialTheme.typography.titleLarge)
                            }
                        },
                        actions = { ThemeToggle(themeMode) { m -> settingsActions.setTheme(m) } },
                        colors = TopAppBarDefaults.centerAlignedTopAppBarColors(containerColor = t.canvas, titleContentColor = t.text),
                    )
                },
                bottomBar = {
                    NavigationBar(containerColor = t.surface, tonalElevation = 0.dp) {
                        val colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = t.accent,
                            selectedTextColor = t.text,
                            indicatorColor = t.subtle,
                            unselectedIconColor = t.text2,
                            unselectedTextColor = t.text2,
                        )
                        NavigationBarItem(
                            selected = tab == 0,
                            onClick = { tab = 0 },
                            icon = { Icon(Icons.Filled.Home, contentDescription = null) },
                            label = { Text(stringResource(R.string.nav_home)) },
                            colors = colors,
                        )
                        NavigationBarItem(
                            selected = tab == 1,
                            onClick = { tab = 1 },
                            icon = { Icon(Icons.Filled.Settings, contentDescription = null) },
                            label = { Text(stringResource(R.string.nav_settings)) },
                            colors = colors,
                        )
                    }
                },
            ) { inner ->
                Column(Modifier.padding(inner).fillMaxSize()) {
                    when (tab) {
                        0 -> HomeScreen(peers, paired, paused, autoOn, transfers, history, home)
                        else -> SettingsScreen(settings, peers, autoOn, targetLabel, themeMode, settingsActions)
                    }
                }
            }
            if (pairingOpen) {
                ModalBottomSheet(
                    onDismissRequest = { pairingOpen = false },
                    sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
                    containerColor = t.canvas,
                ) { PairingContent(paired, code, invite, inviteExpiresAt, pairing) }
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
