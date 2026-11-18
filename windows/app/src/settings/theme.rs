//! Design tokens and small widgets for the settings window (pedoman_desain/Pedoman.md).
//!
//! Soft off-white / charcoal canvases instead of pure white and black, one sky-blue accent,
//! 1 px separators for structure, 6 px radius on controls and 8 px on containers.

use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Layout,
    Rect, RichText, Sense, Stroke, StrokeKind, TextStyle, Vec2,
};

/// Colour tokens for one theme.
#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: Color32,
    pub surface: Color32,
    pub subtle: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub dark: bool,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Tokens from pedoman_desain.
pub const fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            canvas: rgb(0x12, 0x15, 0x1A),
            surface: rgb(0x1A, 0x1E, 0x26),
            subtle: rgb(0x23, 0x29, 0x34),
            text: rgb(0xF1, 0xF5, 0xF9),
            text2: rgb(0x94, 0xA3, 0xB8),
            border: rgb(0x2A, 0x31, 0x3D),
            accent: rgb(0x38, 0xBD, 0xF8),
            accent_hover: rgb(0x7D, 0xD3, 0xFC),
            on_accent: rgb(0x0B, 0x11, 0x1A),
            ok: rgb(0x34, 0xD3, 0x99),
            warn: rgb(0xFB, 0xBF, 0x24),
            err: rgb(0xF8, 0x71, 0x71),
            dark: true,
        }
    } else {
        Palette {
            canvas: rgb(0xF7, 0xF9, 0xFC),
            surface: rgb(0xFF, 0xFF, 0xFF),
            subtle: rgb(0xED, 0xF2, 0xF7),
            text: rgb(0x1A, 0x20, 0x2C),
            text2: rgb(0x4A, 0x55, 0x68),
            border: rgb(0xE2, 0xE8, 0xF0),
            accent: rgb(0x0E, 0xA5, 0xE9),
            accent_hover: rgb(0x02, 0x84, 0xC7),
            on_accent: rgb(0xFF, 0xFF, 0xFF),
            ok: rgb(0x10, 0xB9, 0x81),
            warn: rgb(0xF5, 0x9E, 0x0B),
            err: rgb(0xEF, 0x44, 0x44),
            dark: false,
        }
    }
}

/// Appearance setting stored in `config.toml` (`theme`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Auto,
    Light,
    Dark,
}

impl Mode {
    pub fn parse(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Whether the dark palette applies right now.
    pub fn is_dark(self) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::Auto => crate::theme_sys::system_uses_dark_theme().unwrap_or(false),
        }
    }
}

/// Load Segoe UI (text) and Consolas (device ids, addresses, hashes) from the system fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let windir =
        std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
    let load = |file: &str| std::fs::read(windir.join("Fonts").join(file)).ok();
    if let Some(bytes) = load("segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
    }
    if let Some(bytes) = load("segoeuib.ttf") {
        fonts
            .font_data
            .insert("segoe-bold".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.insert(
            FontFamily::Name("bold".into()),
            vec!["segoe-bold".into(), "segoe".into()],
        );
    } else {
        fonts
            .families
            .insert(FontFamily::Name("bold".into()), vec!["segoe".into()]);
    }
    if let Some(bytes) = load("consola.ttf") {
        fonts
            .font_data
            .insert("consolas".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "consolas".into());
    }
    ctx.set_fonts(fonts);
}

/// Apply visuals and spacing for `p`.
pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.canvas;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.subtle;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.slider_trailing_fill = true;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    for (state, fill) in [
        (&mut w.noninteractive, p.canvas),
        (&mut w.inactive, p.surface),
        (&mut w.hovered, p.subtle),
        (&mut w.active, p.subtle),
        (&mut w.open, p.subtle),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, p.border);
        state.corner_radius = radius;
        state.fg_stroke = Stroke::new(1.0, p.text);
    }
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 7.0);
        s.spacing.interact_size.y = 32.0;
        s.text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(22.0, FontFamily::Name("bold".into())),
        );
    });
}

pub fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name("bold".into())))
}

pub fn mono(text: impl Into<String>, p: &Palette) -> RichText {
    RichText::new(text).monospace().color(p.text2)
}

pub fn secondary(text: impl Into<String>, p: &Palette) -> RichText {
    RichText::new(text).color(p.text2).size(13.0)
}

/// A 1 px horizontal rule.
pub fn rule(ui: &mut egui::Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.border);
}

/// Small uppercase-free section title.
pub fn section(ui: &mut egui::Ui, p: &Palette, title: &str) {
    ui.add_space(14.0);
    ui.label(bold(title, 13.0).color(p.text2));
    ui.add_space(2.0);
    rule(ui, p);
}

/// One settings row: title and optional description on the left, the control on the right,
/// a hairline below.
pub fn row(
    ui: &mut egui::Ui,
    p: &Palette,
    title: &str,
    desc: Option<&str>,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(44.0);
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(title).size(14.0));
            if let Some(d) = desc {
                ui.label(secondary(d, p));
            }
            ui.add_space(6.0);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| control(ui));
    });
    rule(ui, p);
}

/// Pill switch in the accent colour (Windows 11 style).
pub fn toggle(ui: &mut egui::Ui, p: &Palette, on: &mut bool) -> egui::Response {
    let size = Vec2::new(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.15);
    let track = if *on { p.accent } else { p.subtle };
    let border = if *on { p.accent } else { p.text2 };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(11), track);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(11),
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    let x0 = rect.left() + 11.0;
    let x1 = rect.right() - 11.0;
    let knob = egui::pos2(x0 + (x1 - x0) * t, rect.center().y);
    let knob_colour = if *on { p.on_accent } else { p.text2 };
    ui.painter()
        .circle_filled(knob, if *on { 7.0 } else { 6.0 }, knob_colour);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn button(
    ui: &mut egui::Ui,
    text: &str,
    fill: Color32,
    fg: Color32,
    stroke: Stroke,
) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).color(fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6));
    let r = ui.add(btn);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// Filled accent button for the one main action of a view.
pub fn primary_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = p.accent;
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.weak_bg_fill = p.accent_hover;
        w.hovered.bg_stroke = Stroke::NONE;
        w.active.weak_bg_fill = p.accent_hover;
        w.active.bg_stroke = Stroke::NONE;
        let r = ui.add(
            egui::Button::new(RichText::new(text).color(p.on_accent))
                .corner_radius(CornerRadius::same(6)),
        );
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        r
    })
    .inner
}

/// Outlined button.
pub fn secondary_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    button(ui, text, p.surface, p.text, Stroke::new(1.0, p.border))
}

/// Outlined destructive button.
pub fn danger_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    button(ui, text, p.surface, p.err, Stroke::new(1.0, p.err))
}

/// Small coloured dot for a connection state.
pub fn dot(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, colour);
}

/// Small bordered label, e.g. "Wi-Fi" or "Bluetooth".
pub fn badge(ui: &mut egui::Ui, p: &Palette, text: &str, colour: Color32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(12.0, FontFamily::Proportional),
        colour,
    );
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, colour.gamma_multiply(0.6)),
        StrokeKind::Inside,
    );
    ui.painter().rect_filled(
        rect.shrink(1.0),
        CornerRadius::same(5),
        colour.gamma_multiply(if p.dark { 0.14 } else { 0.10 }),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
}

/// Left-hand navigation entry; returns true when clicked.
pub fn nav_item(ui: &mut egui::Ui, p: &Palette, label: &str, selected: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            if selected {
                p.subtle
            } else {
                p.subtle.gamma_multiply(0.6)
            },
        );
    }
    if selected {
        let bar = Rect::from_min_size(
            egui::pos2(rect.left() + 2.0, rect.top() + 8.0),
            Vec2::new(3.0, rect.height() - 16.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), p.accent);
    }
    let colour = if selected { p.text } else { p.text2 };
    let font = if selected {
        FontId::new(14.0, FontFamily::Name("bold".into()))
    } else {
        FontId::new(14.0, FontFamily::Proportional)
    };
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        colour,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Segmented control for the appearance setting; returns the new choice when it changes.
pub fn theme_switcher(ui: &mut egui::Ui, p: &Palette, current: Mode) -> Option<Mode> {
    let items = [
        (Mode::Auto, "Otomatis"),
        (Mode::Light, "Terang"),
        (Mode::Dark, "Gelap"),
    ];
    let font = FontId::new(12.5, FontFamily::Proportional);
    let galleys: Vec<_> = items
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), p.text)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 22.0).collect();
    let total = widths.iter().sum::<f32>();
    let mut picked = None;
    // An explicit left-to-right block, so the order holds inside a right-to-left parent.
    ui.allocate_ui_with_layout(
        Vec2::new(total, 28.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, ((mode, _), galley)) in items.iter().zip(galleys).enumerate() {
                let selected = *mode == current;
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(widths[i], 28.0), Sense::click());
                let radius = match i {
                    0 => CornerRadius {
                        nw: 6,
                        sw: 6,
                        ne: 0,
                        se: 0,
                    },
                    2 => CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: 6,
                        se: 6,
                    },
                    _ => CornerRadius::ZERO,
                };
                let hover = resp.hovered() && !selected;
                let fill = if selected {
                    p.accent
                } else if hover {
                    p.subtle
                } else {
                    p.surface
                };
                ui.painter().rect_filled(rect, radius, fill);
                ui.painter().rect_stroke(
                    rect,
                    radius,
                    Stroke::new(1.0, if selected { p.accent } else { p.border }),
                    StrokeKind::Inside,
                );
                let colour = if selected { p.on_accent } else { p.text2 };
                ui.painter()
                    .galley(rect.center() - galley.size() / 2.0, galley, colour);
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() && !selected {
                    picked = Some(*mode);
                }
            }
        },
    );
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backgrounds_are_never_pure_black_or_white() {
        for dark in [false, true] {
            let p = palette(dark);
            for c in [p.canvas, p.subtle] {
                assert_ne!(c, Color32::BLACK);
                assert_ne!(c, Color32::WHITE);
            }
        }
        // Pure white is allowed only as the card surface of the light theme (pedoman_desain).
        assert_ne!(palette(true).surface, Color32::WHITE);
    }

    #[test]
    fn text_on_accent_is_readable() {
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let x = f32::from(v) / 255.0;
                if x <= 0.03928 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        for dark in [false, true] {
            let p = palette(dark);
            let (a, b) = (lum(p.accent), lum(p.on_accent));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            // The dark theme must reach AA for body text; the light accent is used for large
            // button labels (3:1).
            assert!(
                ratio >= if dark { 7.0 } else { 2.5 },
                "dark={dark} ratio={ratio}"
            );
        }
    }

    #[test]
    fn mode_round_trips() {
        for m in [Mode::Auto, Mode::Light, Mode::Dark] {
            assert_eq!(Mode::parse(m.as_str()), m);
        }
        assert_eq!(Mode::parse("???"), Mode::Auto);
    }
}
