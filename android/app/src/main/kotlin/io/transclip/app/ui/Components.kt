package io.transclip.app.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.transclip.app.R

/** 1 dp rule used instead of cards to structure a screen. */
@Composable
fun Rule(modifier: Modifier = Modifier) {
    HorizontalDivider(modifier, thickness = 1.dp, color = tokens.border)
}

/** Small section heading followed by a rule. */
@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth().padding(top = 20.dp)) {
        Text(text, style = MaterialTheme.typography.titleSmall, color = tokens.text2)
        Rule(Modifier.padding(top = 6.dp))
    }
}

/** Title and optional description on the left, a control on the right. */
@Composable
fun SettingRow(
    title: String,
    description: String? = null,
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    control: @Composable RowScope.() -> Unit,
) {
    Column(Modifier.fillMaxWidth()) {
        Row(
            modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp)
                .let { if (onClick != null) it.clickable(onClick = onClick) else it }
                .padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.bodyLarge)
                if (description != null) {
                    Text(description, style = MaterialTheme.typography.bodySmall, color = tokens.text2)
                }
            }
            control()
        }
        Rule()
    }
}

/** Accent-coloured switch (Material 3, tuned to the palette). */
@Composable
fun AppSwitch(checked: Boolean, onChange: (Boolean) -> Unit) {
    val t = tokens
    Switch(
        checked = checked,
        onCheckedChange = onChange,
        colors = SwitchDefaults.colors(
            checkedTrackColor = t.accent,
            checkedThumbColor = t.onAccent,
            checkedBorderColor = t.accent,
            uncheckedTrackColor = t.subtle,
            uncheckedThumbColor = t.text2,
            uncheckedBorderColor = t.text2,
        ),
    )
}

@Composable
fun PrimaryButton(text: String, modifier: Modifier = Modifier, enabled: Boolean = true, onClick: () -> Unit) {
    val t = tokens
    Button(
        onClick = onClick,
        modifier = modifier.heightIn(min = 44.dp),
        enabled = enabled,
        shape = RoundedCornerShape(6.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = t.accent,
            contentColor = t.onAccent,
            disabledContainerColor = t.subtle,
            disabledContentColor = t.text2,
        ),
    ) { Text(text, fontWeight = FontWeight.Medium) }
}

@Composable
fun SecondaryButton(text: String, modifier: Modifier = Modifier, enabled: Boolean = true, onClick: () -> Unit) {
    val t = tokens
    OutlinedButton(
        onClick = onClick,
        modifier = modifier.heightIn(min = 44.dp),
        enabled = enabled,
        shape = RoundedCornerShape(6.dp),
        border = BorderStroke(1.dp, t.border),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = t.text),
    ) { Text(text, fontWeight = FontWeight.Medium) }
}

@Composable
fun DangerButton(text: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val t = tokens
    OutlinedButton(
        onClick = onClick,
        modifier = modifier.heightIn(min = 44.dp),
        shape = RoundedCornerShape(6.dp),
        border = BorderStroke(1.dp, t.err),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = t.err),
    ) { Text(text, fontWeight = FontWeight.Medium) }
}

/** Connection state dot. */
@Composable
fun StatusDot(color: Color, modifier: Modifier = Modifier) {
    val animated = animateColorAsState(color, tween(150), label = "dot")
    Box(modifier.size(10.dp).clip(CircleShape).background(animated.value))
}

/** Small bordered label, e.g. "Wi-Fi" or "Bluetooth". */
@Composable
fun Badge(text: String, color: Color, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier = modifier
            .clip(RoundedCornerShape(6.dp))
            .background(color.copy(alpha = if (tokens.dark) 0.14f else 0.10f))
            .border(1.dp, color.copy(alpha = 0.6f), RoundedCornerShape(6.dp))
            .padding(horizontal = 7.dp, vertical = 2.dp),
        style = MaterialTheme.typography.labelMedium,
        color = color,
    )
}

/** Theme switcher for the top app bar: cycles Auto, Light, Dark. */
@Composable
fun ThemeToggle(mode: ThemeMode, onChange: (ThemeMode) -> Unit) {
    val icon = when (mode) {
        ThemeMode.AUTO -> R.drawable.ic_theme_auto
        ThemeMode.LIGHT -> R.drawable.ic_theme_light
        ThemeMode.DARK -> R.drawable.ic_theme_dark
    }
    IconButton(onClick = { onChange(mode.next()) }) {
        Icon(painterResource(icon), contentDescription = "Tema: ${themeLabel(mode)}", tint = tokens.text)
    }
}

fun themeLabel(mode: ThemeMode) = when (mode) {
    ThemeMode.AUTO -> "Otomatis"
    ThemeMode.LIGHT -> "Terang"
    ThemeMode.DARK -> "Gelap"
}

/** Segmented control with the same options as the toggle, for the settings screen. */
@Composable
fun ThemeSegments(mode: ThemeMode, onChange: (ThemeMode) -> Unit) {
    val t = tokens
    Row(Modifier.border(1.dp, t.border, RoundedCornerShape(6.dp)).clip(RoundedCornerShape(6.dp))) {
        ThemeMode.entries.forEach { m ->
            val selected = m == mode
            Text(
                themeLabel(m),
                modifier = Modifier
                    .background(if (selected) t.accent else t.surface)
                    .clickable { onChange(m) }
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                style = MaterialTheme.typography.labelMedium,
                color = if (selected) t.onAccent else t.text2,
            )
        }
    }
}

/** Heading, explanation and an optional action for a screen that has nothing to show yet. */
@Composable
fun EmptyState(title: String, body: String, modifier: Modifier = Modifier, action: @Composable (() -> Unit)? = null) {
    Column(modifier.fillMaxWidth().padding(vertical = 28.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = tokens.text2)
        if (action != null) {
            Box(Modifier.padding(top = 8.dp)) { action() }
        }
    }
}

/** Checkbox in the accent colour, for dialogs. */
@Composable
fun Checkbox(checked: Boolean, onChange: (Boolean) -> Unit) {
    val t = tokens
    androidx.compose.material3.Checkbox(
        checked = checked,
        onCheckedChange = onChange,
        colors = androidx.compose.material3.CheckboxDefaults.colors(
            checkedColor = t.accent,
            checkmarkColor = t.onAccent,
            uncheckedColor = t.text2,
        ),
    )
}
