package io.transclip.app.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.shape.RoundedCornerShape

/** Appearance setting (stored as `auto`, `light` or `dark`). */
enum class ThemeMode(val key: String) {
    AUTO("auto"),
    LIGHT("light"),
    DARK("dark"),
    ;

    /** Next mode when the top-bar toggle is tapped. */
    fun next(): ThemeMode = entries[(ordinal + 1) % entries.size]

    companion object {
        fun parse(key: String): ThemeMode = entries.firstOrNull { it.key == key } ?: AUTO
    }
}

/** Colour tokens of docs/design/guidelines.md that Material 3 has no slot for. */
@Immutable
class Tokens(
    val canvas: Color,
    val surface: Color,
    val subtle: Color,
    val text: Color,
    val text2: Color,
    val border: Color,
    val accent: Color,
    val onAccent: Color,
    val ok: Color,
    val warn: Color,
    val err: Color,
    val dark: Boolean,
)

private val LightTokens = Tokens(
    canvas = Color(0xFFF7F9FC),
    surface = Color(0xFFFFFFFF),
    subtle = Color(0xFFEDF2F7),
    text = Color(0xFF1A202C),
    text2 = Color(0xFF4A5568),
    border = Color(0xFFE2E8F0),
    accent = Color(0xFF0EA5E9),
    onAccent = Color(0xFFFFFFFF),
    ok = Color(0xFF10B981),
    warn = Color(0xFFF59E0B),
    err = Color(0xFFEF4444),
    dark = false,
)

private val DarkTokens = Tokens(
    canvas = Color(0xFF12151A),
    surface = Color(0xFF1A1E26),
    subtle = Color(0xFF232934),
    text = Color(0xFFF1F5F9),
    text2 = Color(0xFF94A3B8),
    border = Color(0xFF2A313D),
    accent = Color(0xFF38BDF8),
    onAccent = Color(0xFF0B111A),
    ok = Color(0xFF34D399),
    warn = Color(0xFFFBBF24),
    err = Color(0xFFF87171),
    dark = true,
)

val LocalTokens = staticCompositionLocalOf { LightTokens }

/** The current design tokens. */
val tokens: Tokens
    @Composable
    @ReadOnlyComposable
    get() = LocalTokens.current

private fun scheme(t: Tokens): ColorScheme {
    val base = if (t.dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = t.accent,
        onPrimary = t.onAccent,
        primaryContainer = t.subtle,
        onPrimaryContainer = t.text,
        secondary = t.accent,
        onSecondary = t.onAccent,
        background = t.canvas,
        onBackground = t.text,
        surface = t.canvas,
        onSurface = t.text,
        surfaceVariant = t.subtle,
        onSurfaceVariant = t.text2,
        surfaceContainerLowest = t.canvas,
        surfaceContainerLow = t.canvas,
        surfaceContainer = t.surface,
        surfaceContainerHigh = t.surface,
        surfaceContainerHighest = t.subtle,
        outline = t.border,
        outlineVariant = t.border,
        error = t.err,
        onError = t.onAccent,
    )
}

private val AppTypography = Typography(
    headlineSmall = TextStyle(fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold),
    titleLarge = TextStyle(fontSize = 20.sp, lineHeight = 26.sp, fontWeight = FontWeight.SemiBold),
    titleMedium = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold),
    titleSmall = TextStyle(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.2.sp),
    bodyLarge = TextStyle(fontSize = 16.sp, lineHeight = 22.sp),
    bodyMedium = TextStyle(fontSize = 14.sp, lineHeight = 20.sp),
    bodySmall = TextStyle(fontSize = 12.sp, lineHeight = 16.sp),
    labelLarge = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium),
    labelMedium = TextStyle(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium),
)

/** 6 dp for controls, 8 dp for containers (docs/design/guidelines.md). */
private val AppShapes = Shapes(
    extraSmall = RoundedCornerShape(6.dp),
    small = RoundedCornerShape(6.dp),
    medium = RoundedCornerShape(8.dp),
    large = RoundedCornerShape(8.dp),
    extraLarge = RoundedCornerShape(8.dp),
)

/** Monospace style for device ids, addresses and pairing codes. */
val Mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 12.sp, lineHeight = 16.sp)

@Composable
fun TransClipTheme(mode: ThemeMode, content: @Composable () -> Unit) {
    val dark = when (mode) {
        ThemeMode.AUTO -> isSystemInDarkTheme()
        ThemeMode.LIGHT -> false
        ThemeMode.DARK -> true
    }
    val t = if (dark) DarkTokens else LightTokens
    CompositionLocalProvider(LocalTokens provides t) {
        MaterialTheme(colorScheme = scheme(t), typography = AppTypography, shapes = AppShapes, content = content)
    }
}
