package io.transclip.app.ui

import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
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
import androidx.compose.ui.text.style.TextOverflow
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
private fun IconBadge(icon: Int, tint: Color, size: Int = 44) {
    Box(
        Modifier.size(size.dp).clip(CircleShape).background(tint.copy(alpha = if (tokens.dark) 0.16f else 0.12f)),
        contentAlignment = Alignment.Center,
    ) { Icon(painterResource(icon), contentDescription = null, tint = tint, modifier = Modifier.size((size * 0.5f).dp)) }
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
    LazyColumn(Modifier.fillMaxWidth(), contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        // --- hero: how many devices are connected, shown as overlapping avatars
        item {
            Column(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(t.surface)
                    .border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(18.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text(
                        if (paired) online.toString() else "–",
                        style = MaterialTheme.typography.displayMedium,
                        color = if (online > 0 && !paused) t.accent else t.text2,
                    )
                    Column(Modifier.weight(1f)) {
                        Text(
                            when {
                                !paired -> stringResource(R.string.home_not_paired_title)
                                paused -> stringResource(R.string.status_paused)
                                online > 0 -> pluralStringResource(R.plurals.status_online, online, online).substringAfter(' ')
                                else -> stringResource(R.string.status_waiting)
                            },
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            StatusDot(if (!paired) t.text2 else if (paused) t.warn else if (autoOn) t.ok else t.warn)
                            Text(
                                stringResource(if (!paired) R.string.home_not_paired_hint else if (autoOn) R.string.status_auto_on else R.string.status_auto_off),
                                style = MaterialTheme.typography.bodySmall,
                                color = t.text2,
                            )
                        }
                    }
                    if (peers.isNotEmpty()) {
                        Row(horizontalArrangement = Arrangement.spacedBy((-10).dp)) {
                            peers.take(3).forEach { p ->
                                Box(
                                    Modifier.size(36.dp).clip(CircleShape).background(t.canvas)
                                        .border(2.dp, if (p.state == PeerState.ONLINE) t.ok else t.border, CircleShape),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(painterResource(deviceIcon(p.platform)), null, tint = t.text, modifier = Modifier.size(18.dp))
                                }
                            }
                        }
                    }
                }
                if (!paired) PrimaryButton(stringResource(R.string.home_go_pairing), Modifier.fillMaxWidth(), onClick = actions.openPairing)
            }
        }

        if (paired) {
            // --- two big action tiles
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    ActionTile(R.drawable.ic_clipboard, stringResource(R.string.action_send_clip), Modifier.weight(1f), actions.sendClipboard)
                    ActionTile(R.drawable.ic_upload, stringResource(R.string.action_send_files), Modifier.weight(1f), actions.pickFiles)
                    ActionTile(
                        if (paused) R.drawable.ic_play else R.drawable.ic_pause,
                        stringResource(if (paused) R.string.action_resume else R.string.action_pause),
                        Modifier.weight(1f),
                        actions.togglePause,
                        tint = t.warn,
                    )
                }
            }

            // --- devices as a horizontal carousel of cards
            item {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.section_devices), style = MaterialTheme.typography.titleSmall, color = t.text2, modifier = Modifier.weight(1f))
                    TextButton(onClick = actions.openPairing) { Text("+ " + stringResource(R.string.home_add_device), color = t.accent) }
                }
                if (peers.isEmpty()) {
                    DeviceCardShell(Modifier.fillMaxWidth().clickable(onClick = actions.openPairing)) {
                        IconBadge(R.drawable.ic_plus, t.text2)
                        Text(stringResource(R.string.devices_empty_title), style = MaterialTheme.typography.titleMedium)
                        Text(stringResource(R.string.devices_empty_body), style = MaterialTheme.typography.bodySmall, color = t.text2)
                    }
                } else {
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        items(peers, key = { it.id.toHex() }) { p -> DeviceCard(p) }
                    }
                }
            }

            if (transfers.isNotEmpty()) {
                items(transfers, key = { it.xferId }) { tr ->
                    Column(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(t.surface)
                            .border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(14.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                (if (tr.incoming) "↓  " else "↑  ") + tr.name,
                                Modifier.weight(1f),
                                style = MaterialTheme.typography.bodyMedium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(if (tr.total > 0) "${(tr.done * 100 / tr.total).coerceIn(0, 100)}%" else "", style = Mono, color = t.text2)
                            TextButton(onClick = { actions.cancel(tr) }) { Text(stringResource(R.string.transfer_cancel), color = t.err) }
                        }
                        LinearProgressIndicator(
                            progress = { if (tr.total > 0) (tr.done.toFloat() / tr.total).coerceIn(0f, 1f) else 0f },
                            modifier = Modifier.fillMaxWidth(),
                            color = t.accent,
                            trackColor = t.subtle,
                        )
                    }
                }
            }

            // --- recent activity, short
            if (history.isNotEmpty()) {
                item {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text(stringResource(R.string.section_activity), style = MaterialTheme.typography.titleSmall, color = t.text2, modifier = Modifier.weight(1f))
                        TextButton(onClick = actions.clearHistory) { Text(stringResource(R.string.history_clear), color = t.text2) }
                    }
                    Rule()
                }
                items(history.take(5)) { h ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(h.time)), style = Mono, color = t.text2)
                        Text(h.text, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        }
        item { Spacer(Modifier.height(12.dp)) }
    }
}

@Composable
private fun ActionTile(icon: Int, label: String, modifier: Modifier, onClick: () -> Unit, tint: Color = tokens.accent) {
    val t = tokens
    Column(
        modifier.clip(RoundedCornerShape(8.dp)).background(t.surface).border(1.dp, t.border, RoundedCornerShape(8.dp))
            .clickable(onClick = onClick).padding(vertical = 16.dp, horizontal = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        IconBadge(icon, tint)
        Text(label, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun DeviceCardShell(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val t = tokens
    Column(
        modifier.clip(RoundedCornerShape(8.dp)).background(t.surface).border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) { content() }
}

@Composable
private fun DeviceCard(p: PeerStatus) {
    val t = tokens
    val colour = when (p.state) {
        PeerState.ONLINE -> t.ok
        PeerState.CONNECTING -> t.warn
        PeerState.OFFLINE -> t.text2
    }
    DeviceCardShell(Modifier.width(176.dp)) {
        IconBadge(deviceIcon(p.platform), if (p.state == PeerState.ONLINE) t.accent else t.text2)
        Text(p.title(), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(p.id.toHex().take(8), style = Mono, color = t.text2)
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
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
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
        if (paired) {
            DangerButton(stringResource(R.string.rotate_title), Modifier.fillMaxWidth(), onClick = actions.rotate)
        }
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

/** One switch with its label inside a bordered tile; two of them sit side by side. */
@Composable
private fun SwitchTile(label: String, checked: Boolean, modifier: Modifier, onChange: (Boolean) -> Unit) {
    val t = tokens
    Row(
        modifier.clip(RoundedCornerShape(8.dp)).background(t.surface).border(1.dp, t.border, RoundedCornerShape(8.dp))
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
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
    actions: SettingsActions,
) {
    val t = tokens
    val s = settings ?: return
    var name by remember(s.deviceName) { mutableStateOf(s.deviceName) }
    val version = LocalContext.current.let { c -> runCatching { c.packageManager.getPackageInfo(c.packageName, 0).versionName }.getOrNull().orEmpty() }
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionTitle(stringResource(R.string.settings_device))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(name, { name = it }, Modifier.weight(1f), singleLine = true, shape = RoundedCornerShape(6.dp))
            SecondaryButton(stringResource(R.string.settings_apply), enabled = name.isNotBlank() && name != s.deviceName, onClick = { actions.applyName(name) })
        }

        SectionTitle(stringResource(R.string.settings_sync))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            SwitchTile(stringResource(R.string.settings_sync_text), s.syncText, Modifier.weight(1f), actions.setSyncText)
            SwitchTile(stringResource(R.string.settings_sync_images), s.syncImages, Modifier.weight(1f), actions.setSyncImages)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            SwitchTile(stringResource(R.string.settings_receive_files), s.receiveFiles, Modifier.weight(1f), actions.setReceiveFiles)
            SwitchTile(stringResource(R.string.settings_screenshots), s.autoScreenshots, Modifier.weight(1f), actions.setScreenshots)
        }
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(t.surface).border(1.dp, t.border, RoundedCornerShape(8.dp)).padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(stringResource(R.string.auto_capture_title), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
            if (autoOn) Badge(stringResource(R.string.badge_active), t.ok) else SecondaryButton(stringResource(R.string.auto_capture_open), onClick = actions.openAccessibility)
        }

        SectionTitle(stringResource(R.string.settings_storage))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(targetLabel, Modifier.weight(1f), style = Mono, color = t.text2, maxLines = 2, overflow = TextOverflow.Ellipsis)
            SecondaryButton(stringResource(R.string.target_dir_change), onClick = actions.pickFolder)
            if (s.downloadTreeUri.isNotEmpty()) SecondaryButton(stringResource(R.string.target_dir_reset), onClick = actions.resetFolder)
        }

        if (peers.isNotEmpty()) {
            SectionTitle(stringResource(R.string.settings_bt))
            peers.forEach { p ->
                SwitchTile(p.title(), p.id.toHex() in s.btPriority, Modifier.fillMaxWidth()) { on -> actions.setBluetooth(p, on) }
            }
        }

        SectionTitle(stringResource(R.string.settings_system))
        SwitchTile(stringResource(R.string.settings_autostart), s.autoStart, Modifier.fillMaxWidth(), actions.setAutostart)

        SectionTitle(stringResource(R.string.settings_appearance))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.settings_theme), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
            ThemeSegments(themeMode, actions.setTheme)
        }
        Text("TransClip $version", style = Mono, color = t.text2, modifier = Modifier.padding(top = 12.dp, bottom = 24.dp))
    }
}
