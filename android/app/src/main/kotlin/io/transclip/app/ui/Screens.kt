package io.transclip.app.ui

import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
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
import androidx.compose.ui.res.painterResource
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

private fun PeerStatus.title() = name.ifEmpty { id.toHex().take(8) }

private fun deviceIcon(platform: String) = if (platform == "android") R.drawable.ic_phone else R.drawable.ic_laptop

/** Round tinted badge that holds an icon. */
@Composable
private fun IconBadge(icon: Int, tint: Color, size: Int = 40) {
    Box(
        Modifier.size(size.dp).clip(CircleShape).background(tint.copy(alpha = if (tokens.dark) 0.16f else 0.12f)),
        contentAlignment = Alignment.Center,
    ) { Icon(painterResource(icon), contentDescription = null, tint = tint, modifier = Modifier.size((size * 0.5f).dp)) }
}

/** Surface with a 1 dp border: the only container style (never nested). */
@Composable
private fun Bordered(modifier: Modifier = Modifier, padding: Int = 14, content: @Composable () -> Unit) {
    val t = tokens
    Column(
        modifier.clip(RoundedCornerShape(8.dp)).background(t.surface).border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(padding.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) { content() }
}

// ------------------------------------------------------------------------------------- Home

class HomeActions(
    val sendClipboard: () -> Unit,
    val pickFiles: () -> Unit,
    val togglePause: () -> Unit,
    val cancel: (ActiveTransfer) -> Unit,
    val clearHistory: () -> Unit,
    val openPairing: () -> Unit,
)

/**
 * One column on phones (portrait), two panes from 600 dp wide (tablets, landscape): devices and
 * actions on the left, transfers and activity on the right.
 */
@Composable
fun HomeScreen(
    peers: List<PeerStatus>,
    paired: Boolean,
    paused: Boolean,
    autoOn: Boolean,
    transfers: List<ActiveTransfer>,
    history: List<HistoryEntry>,
    wide: Boolean,
    actions: HomeActions,
) {
    val main: @Composable () -> Unit = {
        StatusStrip(peers, paired, paused, autoOn, actions)
        if (paired) {
            ActionButtons(actions)
            DevicesSection(peers, columns = if (wide) 3 else 2, actions)
        } else {
            EmptyState(stringResource(R.string.home_not_paired_title), stringResource(R.string.home_not_paired_body)) {
                PrimaryButton(stringResource(R.string.home_go_pairing), onClick = actions.openPairing)
            }
        }
    }
    val side: @Composable () -> Unit = {
        if (paired) {
            TransfersSection(transfers, actions)
            ActivitySection(history, actions)
        }
    }
    if (wide) {
        Row(Modifier.fillMaxSize().padding(horizontal = 24.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(28.dp)) {
            Column(Modifier.weight(1.5f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(18.dp)) { main() }
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(18.dp)) { side() }
        }
    } else {
        Column(Modifier.fillMaxSize().padding(horizontal = 20.dp, vertical = 8.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(18.dp)) {
            main()
            side()
            Spacer(Modifier.height(8.dp))
        }
    }
}

/** Plain status line: dot, sentence, and the pause control. No big numbers, no hero card. */
@Composable
private fun StatusStrip(peers: List<PeerStatus>, paired: Boolean, paused: Boolean, autoOn: Boolean, actions: HomeActions) {
    val t = tokens
    val online = peers.count { it.state == PeerState.ONLINE }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        StatusDot(if (!paired) t.text2 else if (paused) t.warn else if (online > 0) t.ok else t.warn)
        Column(Modifier.weight(1f)) {
            Text(
                when {
                    !paired -> stringResource(R.string.home_not_paired_title)
                    paused -> stringResource(R.string.status_paused)
                    online > 0 -> pluralStringResource(R.plurals.status_online, online, online)
                    else -> stringResource(R.string.status_waiting)
                },
                style = MaterialTheme.typography.titleMedium,
            )
            if (paired) {
                Text(
                    stringResource(if (autoOn) R.string.status_auto_on else R.string.status_auto_off),
                    style = MaterialTheme.typography.bodySmall,
                    color = t.text2,
                )
            }
        }
        if (paired) {
            TextButton(onClick = actions.togglePause) {
                Text(stringResource(if (paused) R.string.action_resume else R.string.action_pause), color = t.text2)
            }
        }
    }
}

@Composable
private fun ActionButtons(actions: HomeActions) {
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        PrimaryButton(stringResource(R.string.action_send_clip), Modifier.weight(1f), onClick = actions.sendClipboard)
        SecondaryButton(stringResource(R.string.action_send_files), Modifier.weight(1f), onClick = actions.pickFiles)
    }
}

@Composable
private fun DevicesSection(peers: List<PeerStatus>, columns: Int, actions: HomeActions) {
    val t = tokens
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.section_devices), style = MaterialTheme.typography.titleSmall, color = t.text2, modifier = Modifier.weight(1f))
            TextButton(onClick = actions.openPairing) { Text(stringResource(R.string.home_add_device), color = t.accent) }
        }
        if (peers.isEmpty()) {
            Bordered(Modifier.fillMaxWidth().clickable(onClick = actions.openPairing)) {
                Text(stringResource(R.string.devices_empty_title), style = MaterialTheme.typography.titleMedium)
                Text(stringResource(R.string.devices_empty_body), style = MaterialTheme.typography.bodySmall, color = t.text2)
            }
        } else {
            // A fixed number of equal columns that reflow with the screen; no horizontal scrolling.
            peers.chunked(columns).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    row.forEach { p -> DeviceCard(p, Modifier.weight(1f)) }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

@Composable
private fun DeviceCard(p: PeerStatus, modifier: Modifier) {
    val t = tokens
    val colour = when (p.state) {
        PeerState.ONLINE -> t.ok
        PeerState.CONNECTING -> t.warn
        PeerState.OFFLINE -> t.text2
    }
    Bordered(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            IconBadge(deviceIcon(p.platform), if (p.state == PeerState.ONLINE) t.accent else t.text2, 36)
            Column(Modifier.weight(1f)) {
                Text(p.title(), style = MaterialTheme.typography.titleMedium, maxLines = 2)
                Text(p.id.toHex().take(8), style = Mono, color = t.text2)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            StatusDot(colour)
            when (p.transport) {
                Transport.LAN -> Badge(stringResource(R.string.via_wifi), t.ok)
                Transport.BLUETOOTH -> Badge(stringResource(R.string.via_bt), t.warn)
                null -> Text(
                    stringResource(if (p.state == PeerState.CONNECTING) R.string.state_connecting else R.string.state_offline),
                    style = MaterialTheme.typography.bodySmall,
                    color = t.text2,
                )
            }
        }
    }
}

@Composable
private fun TransfersSection(transfers: List<ActiveTransfer>, actions: HomeActions) {
    val t = tokens
    if (transfers.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(stringResource(R.string.transfers_title), style = MaterialTheme.typography.titleSmall, color = t.text2)
        transfers.forEach { tr ->
            Bordered(Modifier.fillMaxWidth()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text((if (tr.incoming) "↓  " else "↑  ") + tr.name, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 2)
                    Text(if (tr.total > 0) "${(tr.done * 100 / tr.total).coerceIn(0, 100)}%" else "", style = Mono, color = t.text2)
                }
                LinearProgressIndicator(
                    progress = { if (tr.total > 0) (tr.done.toFloat() / tr.total).coerceIn(0f, 1f) else 0f },
                    modifier = Modifier.fillMaxWidth(),
                    color = t.accent,
                    trackColor = t.subtle,
                )
                TextButton(onClick = { actions.cancel(tr) }) { Text(stringResource(R.string.transfer_cancel), color = t.err) }
            }
        }
    }
}

@Composable
private fun ActivitySection(history: List<HistoryEntry>, actions: HomeActions) {
    val t = tokens
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.section_activity), style = MaterialTheme.typography.titleSmall, color = t.text2, modifier = Modifier.weight(1f))
            if (history.isNotEmpty()) TextButton(onClick = actions.clearHistory) { Text(stringResource(R.string.history_clear), color = t.text2) }
        }
        Rule()
        if (history.isEmpty()) {
            Text(stringResource(R.string.activity_empty), Modifier.padding(vertical = 8.dp), style = MaterialTheme.typography.bodySmall, color = t.text2)
        }
        history.take(6).forEach { h ->
            Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(h.time)), style = Mono, color = t.text2)
                Text(h.text, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

// ------------------------------------------------------------------------------------- Pairing (bottom sheet)

class PairingActions(
    val scan: () -> Unit,
    val join: (String) -> Unit,
    val showCode: () -> Unit,
    val copyCode: () -> Unit,
    val rotate: () -> Unit,
    val onCodeChange: (String) -> Unit,
)

@Composable
fun PairingContent(
    paired: Boolean,
    code: String,
    invite: String,
    inviteExpiresAtMs: Long,
    actions: PairingActions,
) {
    val t = tokens
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Text(stringResource(R.string.home_add_device), style = MaterialTheme.typography.titleLarge)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            PrimaryButton(stringResource(R.string.pairing_scan), Modifier.weight(1f), onClick = actions.scan)
            SecondaryButton(
                stringResource(if (invite.isEmpty()) R.string.pairing_show_code else R.string.pairing_refresh_code),
                Modifier.weight(1f),
                onClick = actions.showCode,
            )
        }
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
                    Modifier.align(Alignment.CenterHorizontally).clip(RoundedCornerShape(8.dp)).background(Color.White)
                        .border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(12.dp),
                ) {
                    qrBitmap(invite)?.let { Image(it.asImageBitmap(), stringResource(R.string.pairing_qr_description), Modifier.size(232.dp)) }
                }
                Row(Modifier.align(Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Badge(stringResource(R.string.pairing_valid_for, left / 60, left % 60), t.warn)
                    TextButton(onClick = actions.copyCode) { Text(stringResource(R.string.pairing_copy), color = t.accent) }
                }
            } else {
                Text(stringResource(R.string.pairing_expired), style = MaterialTheme.typography.bodyMedium, color = t.err)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
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
        if (paired) DangerButton(stringResource(R.string.rotate_title), Modifier.fillMaxWidth(), onClick = actions.rotate)
        Spacer(Modifier.height(24.dp))
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

/** A switch with its label; the label wraps instead of being cut. */
@Composable
private fun SwitchTile(label: String, checked: Boolean, modifier: Modifier, onChange: (Boolean) -> Unit) {
    Row(
        modifier.clip(RoundedCornerShape(8.dp)).background(tokens.surface).border(1.dp, tokens.border, RoundedCornerShape(8.dp))
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        AppSwitch(checked, onChange)
    }
}

@Composable
fun SettingsScreen(
    settings: Settings?,
    peers: List<PeerStatus>,
    autoOn: Boolean,
    targetLabel: String,
    themeMode: ThemeMode,
    wide: Boolean,
    actions: SettingsActions,
) {
    val s = settings ?: return
    val t = tokens
    var name by remember(s.deviceName) { mutableStateOf(s.deviceName) }
    val version = LocalContext.current.let { c -> runCatching { c.packageManager.getPackageInfo(c.packageName, 0).versionName }.getOrNull().orEmpty() }

    val groupA: @Composable () -> Unit = {
        Section(stringResource(R.string.settings_device)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(name, { name = it }, Modifier.weight(1f), singleLine = true, shape = RoundedCornerShape(6.dp))
                SecondaryButton(stringResource(R.string.settings_apply), enabled = name.isNotBlank() && name != s.deviceName, onClick = { actions.applyName(name) })
            }
        }
        Section(stringResource(R.string.settings_sync)) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                SwitchTile(stringResource(R.string.settings_sync_text), s.syncText, Modifier.weight(1f), actions.setSyncText)
                SwitchTile(stringResource(R.string.settings_sync_images), s.syncImages, Modifier.weight(1f), actions.setSyncImages)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                SwitchTile(stringResource(R.string.settings_receive_files), s.receiveFiles, Modifier.weight(1f), actions.setReceiveFiles)
                SwitchTile(stringResource(R.string.settings_screenshots), s.autoScreenshots, Modifier.weight(1f), actions.setScreenshots)
            }
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Text(stringResource(R.string.auto_capture_title), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                if (autoOn) Badge(stringResource(R.string.badge_active), t.ok) else SecondaryButton(stringResource(R.string.auto_capture_open), onClick = actions.openAccessibility)
            }
        }
        Section(stringResource(R.string.settings_storage)) {
            Text(targetLabel, style = Mono, color = t.text2)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryButton(stringResource(R.string.target_dir_change), onClick = actions.pickFolder)
                if (s.downloadTreeUri.isNotEmpty()) SecondaryButton(stringResource(R.string.target_dir_reset), onClick = actions.resetFolder)
            }
        }
    }
    val groupB: @Composable () -> Unit = {
        if (peers.isNotEmpty()) {
            Section(stringResource(R.string.settings_bt)) {
                peers.forEach { p -> SwitchTile(p.title(), p.id.toHex() in s.btPriority, Modifier.fillMaxWidth()) { on -> actions.setBluetooth(p, on) } }
            }
        }
        Section(stringResource(R.string.settings_system)) {
            SwitchTile(stringResource(R.string.settings_autostart), s.autoStart, Modifier.fillMaxWidth(), actions.setAutostart)
        }
        Section(stringResource(R.string.settings_appearance)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.settings_theme), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                ThemeSegments(themeMode, actions.setTheme)
            }
        }
        Text("TransClip $version", style = Mono, color = t.text2, modifier = Modifier.padding(bottom = 24.dp))
    }
    if (wide) {
        Row(Modifier.fillMaxSize().padding(horizontal = 24.dp).verticalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(28.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(16.dp)) { groupA() }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(16.dp)) { groupB() }
        }
    } else {
        Column(Modifier.fillMaxSize().padding(horizontal = 20.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            groupA()
            groupB()
        }
    }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(title, style = MaterialTheme.typography.titleSmall, color = tokens.text2)
        Rule()
        content()
    }
}
