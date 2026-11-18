package io.transclip.app.ui

import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import io.transclip.app.ActiveTransfer
import io.transclip.app.HistoryEntry
import io.transclip.app.R
import io.transclip.app.storage.Settings
import io.transclip.engine.PeerState
import io.transclip.engine.PeerStatus
import io.transclip.engine.Transport
import java.text.DateFormat
import java.util.Date

private val ScreenPadding = Modifier.padding(horizontal = 20.dp)

private fun PeerStatus.title() = name.ifEmpty { id.toHex().take(8) }

// ------------------------------------------------------------------------------------- Home

class HomeActions(
    val sendClipboard: () -> Unit,
    val pickFiles: () -> Unit,
    val togglePause: () -> Unit,
    val cancel: (ActiveTransfer) -> Unit,
    val clearHistory: () -> Unit,
    val openPairing: () -> Unit,
)

@Composable
fun HomeScreen(
    peers: List<PeerStatus>,
    paired: Boolean,
    paused: Boolean,
    autoOn: Boolean,
    transfers: List<ActiveTransfer>,
    history: List<HistoryEntry>,
    actions: HomeActions,
) {
    val t = tokens
    val online = peers.count { it.state == PeerState.ONLINE }
    LazyColumn(Modifier.fillMaxWidth(), contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 20.dp, vertical = 12.dp)) {
        item {
            Row(Modifier.padding(top = 8.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                StatusDot(
                    when {
                        !paired -> t.text2
                        paused -> t.warn
                        online > 0 -> t.ok
                        else -> t.warn
                    },
                )
                Column {
                    Text(
                        when {
                            !paired -> stringResource(R.string.home_not_paired_title)
                            paused -> stringResource(R.string.status_paused)
                            online > 0 -> pluralStringResource(R.plurals.status_online, online, online)
                            else -> stringResource(R.string.status_waiting)
                        },
                        style = MaterialTheme.typography.titleLarge,
                    )
                    if (paired) {
                        Text(
                            stringResource(if (autoOn) R.string.status_auto_on else R.string.status_auto_off),
                            style = MaterialTheme.typography.bodySmall,
                            color = t.text2,
                        )
                    }
                }
            }
        }
        if (!paired) {
            item {
                EmptyState(
                    stringResource(R.string.home_not_paired_title),
                    stringResource(R.string.home_not_paired_body),
                ) { PrimaryButton(stringResource(R.string.home_go_pairing), onClick = actions.openPairing) }
            }
            return@LazyColumn
        }
        item {
            Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PrimaryButton(stringResource(R.string.action_send_clip), Modifier.weight(1f), onClick = actions.sendClipboard)
                SecondaryButton(stringResource(R.string.action_send_files), Modifier.weight(1f), onClick = actions.pickFiles)
            }
            TextButton(onClick = actions.togglePause, modifier = Modifier.padding(top = 2.dp)) {
                Text(stringResource(if (paused) R.string.action_resume else R.string.action_pause), color = t.text2)
            }
        }

        item { SectionTitle(stringResource(R.string.section_devices)) }
        if (peers.isEmpty()) {
            item { EmptyState(stringResource(R.string.devices_empty_title), stringResource(R.string.devices_empty_body)) }
        }
        items(peers, key = { it.id.toHex() }) { p ->
            Column {
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    StatusDot(
                        when (p.state) {
                            PeerState.ONLINE -> t.ok
                            PeerState.CONNECTING -> t.warn
                            PeerState.OFFLINE -> t.text2
                        },
                    )
                    Column(Modifier.weight(1f)) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(p.title(), style = MaterialTheme.typography.titleMedium)
                            when (p.transport) {
                                Transport.LAN -> Badge(stringResource(R.string.via_wifi), t.ok)
                                Transport.BLUETOOTH -> Badge(stringResource(R.string.via_bt), t.warn)
                                null -> {}
                            }
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                stringResource(
                                    when (p.state) {
                                        PeerState.ONLINE -> R.string.state_online
                                        PeerState.CONNECTING -> R.string.state_connecting
                                        PeerState.OFFLINE -> R.string.state_offline
                                    },
                                ),
                                style = MaterialTheme.typography.bodySmall,
                                color = t.text2,
                            )
                            Text(p.id.toHex().take(8), style = Mono, color = t.text2)
                        }
                    }
                }
                Rule()
            }
        }

        if (transfers.isNotEmpty()) {
            item { SectionTitle(stringResource(R.string.transfers_title)) }
            items(transfers, key = { it.xferId }) { tr ->
                Column(Modifier.fillMaxWidth().padding(top = 12.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            (if (tr.incoming) "↓  " else "↑  ") + tr.name,
                            Modifier.weight(1f),
                            style = MaterialTheme.typography.bodyMedium,
                            maxLines = 1,
                            overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                        )
                        Text(
                            if (tr.total > 0) "${(tr.done * 100 / tr.total).coerceIn(0, 100)}%" else "",
                            style = Mono,
                            color = t.text2,
                        )
                    }
                    LinearProgressIndicator(
                        progress = { if (tr.total > 0) (tr.done.toFloat() / tr.total).coerceIn(0f, 1f) else 0f },
                        modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
                        color = t.accent,
                        trackColor = t.subtle,
                    )
                    TextButton(onClick = { actions.cancel(tr) }) { Text(stringResource(R.string.transfer_cancel), color = t.err) }
                    Rule()
                }
            }
        }

        item {
            Row(Modifier.fillMaxWidth().padding(top = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.section_activity), style = MaterialTheme.typography.titleSmall, color = t.text2, modifier = Modifier.weight(1f))
                if (history.isNotEmpty()) {
                    TextButton(onClick = actions.clearHistory) { Text(stringResource(R.string.history_clear), color = t.text2) }
                }
            }
            Rule()
            if (history.isEmpty()) {
                Text(stringResource(R.string.activity_empty), Modifier.padding(vertical = 16.dp), style = MaterialTheme.typography.bodyMedium, color = t.text2)
            }
        }
        items(history.take(30)) { h ->
            Column {
                Row(Modifier.fillMaxWidth().padding(vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(h.time)), style = Mono, color = t.text2)
                    Text(h.text, style = MaterialTheme.typography.bodyMedium)
                }
                Rule()
            }
        }
    }
}

// ------------------------------------------------------------------------------------- Pairing

class PairingActions(
    val scan: () -> Unit,
    val join: (String) -> Unit,
    val showCode: () -> Unit,
    val copyCode: () -> Unit,
    val rotate: () -> Unit,
    val onCodeChange: (String) -> Unit,
)

@Composable
fun PairingScreen(
    paired: Boolean,
    code: String,
    invite: String,
    inviteExpiresAtMs: Long,
    actions: PairingActions,
) {
    val t = tokens
    Column(Modifier.fillMaxWidth().then(ScreenPadding).verticalScroll(rememberScrollState())) {
        SectionTitle(stringResource(R.string.pairing_join_section))
        Text(stringResource(R.string.pairing_scan_hint), Modifier.padding(top = 10.dp), style = MaterialTheme.typography.bodyMedium, color = t.text2)
        PrimaryButton(stringResource(R.string.pairing_scan), Modifier.fillMaxWidth().padding(top = 12.dp), onClick = actions.scan)
        Text(stringResource(R.string.pairing_or_paste), Modifier.padding(top = 16.dp), style = MaterialTheme.typography.bodySmall, color = t.text2)
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = code,
                onValueChange = actions.onCodeChange,
                modifier = Modifier.weight(1f),
                placeholder = { Text(stringResource(R.string.pairing_code_hint), style = Mono) },
                textStyle = Mono,
                singleLine = true,
                shape = RoundedCornerShape(6.dp),
            )
            SecondaryButton(stringResource(R.string.pairing_join), enabled = code.isNotBlank(), onClick = { actions.join(code) })
        }

        SectionTitle(stringResource(R.string.pairing_add_section))
        Text(stringResource(R.string.pairing_add_hint), Modifier.padding(top = 10.dp), style = MaterialTheme.typography.bodyMedium, color = t.text2)
        SecondaryButton(
            stringResource(if (invite.isEmpty()) R.string.pairing_show_code else R.string.pairing_refresh_code),
            Modifier.fillMaxWidth().padding(top = 12.dp),
            onClick = actions.showCode,
        )
        if (invite.isNotEmpty()) {
            var left by remember(inviteExpiresAtMs) { mutableStateOf(((inviteExpiresAtMs - System.currentTimeMillis()) / 1000).coerceAtLeast(0)) }
            LaunchedEffect(inviteExpiresAtMs) {
                while (left > 0) {
                    kotlinx.coroutines.delay(1000)
                    left = ((inviteExpiresAtMs - System.currentTimeMillis()) / 1000).coerceAtLeast(0)
                }
            }
            if (left > 0) {
                // A QR code needs a light quiet zone, so it sits on white in both themes.
                Box(
                    Modifier.padding(top = 16.dp).align(Alignment.CenterHorizontally)
                        .clip(RoundedCornerShape(8.dp)).background(Color.White)
                        .border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(12.dp),
                ) {
                    qrBitmap(invite)?.let { Image(it.asImageBitmap(), contentDescription = stringResource(R.string.pairing_qr_description), modifier = Modifier.size(232.dp)) }
                }
                Row(Modifier.fillMaxWidth().padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Badge(stringResource(R.string.pairing_valid_for, left / 60, left % 60), t.warn)
                    TextButton(onClick = actions.copyCode) { Text(stringResource(R.string.pairing_copy), color = t.accent) }
                }
            } else {
                Text(stringResource(R.string.pairing_expired), Modifier.padding(top = 12.dp), style = MaterialTheme.typography.bodyMedium, color = t.err)
            }
        }

        if (paired) {
            SectionTitle(stringResource(R.string.pairing_security))
            Text(stringResource(R.string.rotate_hint), Modifier.padding(top = 10.dp), style = MaterialTheme.typography.bodyMedium, color = t.text2)
            DangerButton(stringResource(R.string.rotate_title), Modifier.padding(top = 12.dp), onClick = actions.rotate)
        }
        androidx.compose.foundation.layout.Spacer(Modifier.size(24.dp))
    }
}

/** Render [text] as a QR code bitmap, black on white. Null when it does not fit. */
fun qrBitmap(text: String, size: Int = 640): Bitmap? = runCatching {
    val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size)
    val px = IntArray(size * size) { i -> if (m.get(i % size, i / size)) AndroidColor.BLACK else AndroidColor.WHITE }
    Bitmap.createBitmap(px, size, size, Bitmap.Config.ARGB_8888)
}.getOrNull()

// ------------------------------------------------------------------------------------- Settings

class SettingsActions(
    val applyName: (String) -> Unit,
    val setSyncText: (Boolean) -> Unit,
    val setSyncImages: (Boolean) -> Unit,
    val setReceiveFiles: (Boolean) -> Unit,
    val setScreenshots: (Boolean) -> Unit,
    val setAutostart: (Boolean) -> Unit,
    val openAccessibility: () -> Unit,
    val pickFolder: () -> Unit,
    val resetFolder: () -> Unit,
    val setBluetooth: (PeerStatus, Boolean) -> Unit,
    val setTheme: (ThemeMode) -> Unit,
)

@Composable
fun SettingsScreen(
    settings: Settings?,
    peers: List<PeerStatus>,
    autoOn: Boolean,
    targetLabel: String,
    themeMode: ThemeMode,
    actions: SettingsActions,
) {
    val t = tokens
    val s = settings ?: return
    var name by remember(s.deviceName) { mutableStateOf(s.deviceName) }
    val version = LocalContext.current.let { c -> runCatching { c.packageManager.getPackageInfo(c.packageName, 0).versionName }.getOrNull().orEmpty() }
    Column(Modifier.fillMaxWidth().then(ScreenPadding).verticalScroll(rememberScrollState())) {
        SectionTitle(stringResource(R.string.settings_device))
        Column(Modifier.padding(vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.settings_name), style = MaterialTheme.typography.bodyLarge)
            Text(stringResource(R.string.settings_name_desc), style = MaterialTheme.typography.bodySmall, color = t.text2)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(name, { name = it }, Modifier.weight(1f), singleLine = true, shape = RoundedCornerShape(6.dp))
                SecondaryButton(stringResource(R.string.settings_apply), enabled = name.isNotBlank() && name != s.deviceName, onClick = { actions.applyName(name) })
            }
        }
        Rule()

        SectionTitle(stringResource(R.string.settings_sync))
        SettingRow(stringResource(R.string.settings_sync_text), stringResource(R.string.settings_sync_text_desc)) { AppSwitch(s.syncText, actions.setSyncText) }
        SettingRow(stringResource(R.string.settings_sync_images), stringResource(R.string.settings_sync_images_desc)) { AppSwitch(s.syncImages, actions.setSyncImages) }
        SettingRow(stringResource(R.string.settings_receive_files), stringResource(R.string.settings_receive_files_desc)) { AppSwitch(s.receiveFiles, actions.setReceiveFiles) }

        SectionTitle(stringResource(R.string.settings_auto))
        SettingRow(
            stringResource(R.string.auto_capture_title),
            stringResource(if (autoOn) R.string.auto_capture_on else R.string.auto_capture_off),
        ) {
            if (autoOn) Badge(stringResource(R.string.badge_active), t.ok) else SecondaryButton(stringResource(R.string.auto_capture_open), onClick = actions.openAccessibility)
        }
        SettingRow(stringResource(R.string.settings_screenshots), stringResource(R.string.settings_screenshots_desc)) { AppSwitch(s.autoScreenshots, actions.setScreenshots) }

        SectionTitle(stringResource(R.string.settings_storage))
        Column(Modifier.padding(vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.target_dir_title), style = MaterialTheme.typography.bodyLarge)
            Text(targetLabel, style = Mono, color = t.text2)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryButton(stringResource(R.string.target_dir_change), onClick = actions.pickFolder)
                if (s.downloadTreeUri.isNotEmpty()) SecondaryButton(stringResource(R.string.target_dir_reset), onClick = actions.resetFolder)
            }
        }
        Rule()

        if (peers.isNotEmpty()) {
            SectionTitle(stringResource(R.string.settings_bt))
            Text(stringResource(R.string.settings_bt_desc), Modifier.padding(top = 10.dp), style = MaterialTheme.typography.bodySmall, color = t.text2)
            peers.forEach { p ->
                SettingRow(p.title(), p.id.toHex().take(8)) { AppSwitch(p.id.toHex() in s.btPriority) { on -> actions.setBluetooth(p, on) } }
            }
        }

        SectionTitle(stringResource(R.string.settings_system))
        SettingRow(stringResource(R.string.settings_autostart), stringResource(R.string.settings_autostart_desc)) { AppSwitch(s.autoStart, actions.setAutostart) }

        SectionTitle(stringResource(R.string.settings_appearance))
        SettingRow(stringResource(R.string.settings_theme), stringResource(R.string.settings_theme_desc)) { ThemeSegments(themeMode, actions.setTheme) }

        SectionTitle(stringResource(R.string.settings_about))
        SettingRow(stringResource(R.string.settings_version)) { Text(version, style = Mono, color = t.text2) }
        androidx.compose.foundation.layout.Spacer(Modifier.size(24.dp))
    }
}
