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
                        Text(p.state.name.lowercase(), style = MaterialTheme.typography.bodySmall)
                    }
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
                    Button(onClick = {
                        scope.launch {
                            message = try {
                                val info = PairingInfo.parse(code)
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
                    }) { Text(getString(R.string.pairing_join)) }
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
                if (message.isNotEmpty()) Text(message)
                if (invite.isNotEmpty()) Text(invite, style = MaterialTheme.typography.bodySmall)
            }
            if (history.isNotEmpty()) {
                item { Text(getString(R.string.history_title), style = MaterialTheme.typography.titleMedium) }
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
