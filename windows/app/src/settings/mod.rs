//! Settings window (PLAN section 5.6): a separate process (`transclip.exe --settings`) that
//! talks to the daemon over the named pipe, so the daemon's memory stays small.
//!
//! Layout: a left navigation column (logo, connection summary, pages) and a content area with a
//! page title, the theme switcher in its top-right corner (pedoman_desain), and rows separated by
//! 1 px rules. There are no nested cards.

mod theme;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Vec2};
use serde_json::{json, Value};

use self::theme::{Mode, Palette};
use crate::config::{Config, LOG_LEVELS};
use crate::ipc;

const POLL_EVERY: Duration = Duration::from_millis(1500);
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-256.png");

fn window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    Some(egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

fn logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ctx.load_texture(
        "logo",
        egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
        egui::TextureOptions::LINEAR,
    ))
}

fn runtime() -> Option<Arc<Backend>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .ok()?;
    Some(Arc::new(Backend { rt }))
}

fn viewport(title: &str, size: [f32; 2], min: [f32; 2]) -> egui::ViewportBuilder {
    let mut v = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size(min);
    if let Some(icon) = window_icon() {
        v = v.with_icon(Arc::new(icon));
    }
    v
}

/// Open the window; returns the process exit code.
pub fn run() -> i32 {
    let Some(backend) = runtime() else { return 1 };
    let options = eframe::NativeOptions {
        viewport: viewport("TransClip", [860.0, 600.0], [720.0, 500.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "TransClip",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(backend, &cc.egui_ctx)))
        }),
    );
    match result {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "settings window failed");
            1
        }
    }
}

/// Blocking access to the daemon; local pipe calls take milliseconds.
struct Backend {
    rt: tokio::runtime::Runtime,
}

impl Backend {
    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.rt.block_on(async {
            tokio::time::timeout(
                CALL_TIMEOUT,
                ipc::client::one_shot(&ipc::pipe_name(), method, params),
            )
            .await
            .map_err(|_| "daemon tidak menjawab".to_string())?
        })
    }
}

// ---------------------------------------------------------------------------------- picker

/// Small window listing the online devices; sends `paths` to the chosen one (SendTo shortcut).
pub fn run_send_picker(paths: Vec<String>) -> i32 {
    let Some(backend) = runtime() else { return 1 };
    let options = eframe::NativeOptions {
        viewport: viewport("Kirim dengan TransClip", [380.0, 360.0], [320.0, 260.0]),
        ..Default::default()
    };
    match eframe::run_native(
        "TransClip",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            Ok(Box::new(PickerApp {
                backend,
                paths,
                peers: Vec::new(),
                last_poll: None,
                message: String::new(),
                mode: Mode::Auto,
            }))
        }),
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

struct PickerApp {
    backend: Arc<Backend>,
    paths: Vec<String>,
    peers: Vec<(String, String)>,
    last_poll: Option<Instant>,
    message: String,
    mode: Mode,
}

impl PickerApp {
    fn refresh(&mut self) {
        self.last_poll = Some(Instant::now());
        if let Ok(cfg) = self.backend.call("get_config", Value::Null) {
            if let Some(t) = cfg.get("theme").and_then(Value::as_str) {
                self.mode = Mode::parse(t);
            }
        }
        match self.backend.call("list_peers", Value::Null) {
            Ok(Value::Array(list)) => {
                self.peers = list
                    .iter()
                    .filter(|p| p.get("state").and_then(Value::as_str) == Some("online"))
                    .map(|p| {
                        (
                            p.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
                            p.get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("?")
                                .to_owned(),
                        )
                    })
                    .collect();
                self.message.clear();
            }
            Ok(_) => {}
            Err(_) => self.message = "TransClip belum berjalan.".into(),
        }
    }

    /// Send and close the window on success.
    fn send(&mut self, ctx: &egui::Context, to: &str) {
        match self
            .backend
            .call("send_files", json!({ "paths": self.paths, "to": to }))
        {
            Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => self.message = format!("Gagal mengirim: {e}"),
        }
    }
}

impl eframe::App for PickerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.last_poll.is_none_or(|t| t.elapsed() >= POLL_EVERY) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(POLL_EVERY);
        let p = theme::palette(self.mode.is_dark());
        theme::apply(ui.ctx(), &p);
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.canvas).inner_margin(20.0))
            .show(ui, |ui| {
                let count = self.paths.len();
                ui.label(theme::bold(
                    if count == 1 {
                        "Kirim 1 item ke…".to_owned()
                    } else {
                        format!("Kirim {count} item ke…")
                    },
                    18.0,
                ));
                ui.add_space(4.0);
                theme::rule(ui, &p);
                ui.add_space(8.0);
                if self.peers.is_empty() {
                    let text = if self.message.is_empty() {
                        "Tidak ada perangkat yang online. Pastikan perangkat lain terhubung ke jaringan yang sama."
                    } else {
                        self.message.as_str()
                    };
                    ui.label(theme::secondary(text, &p));
                    return;
                }
                let peers = self.peers.clone();
                if peers.len() > 1 {
                    let all = ui.add_sized(
                        [ui.available_width(), 38.0],
                        egui::Button::new(RichText::new("Semua perangkat").color(p.on_accent))
                            .fill(p.accent)
                            .corner_radius(CornerRadius::same(6)),
                    );
                    if all.clicked() {
                        self.send(&ctx, "all");
                    }
                }
                for (id, name) in peers {
                    let b = ui.add_sized(
                        [ui.available_width(), 38.0],
                        egui::Button::new(RichText::new(name))
                            .fill(p.surface)
                            .stroke(egui::Stroke::new(1.0, p.border))
                            .corner_radius(CornerRadius::same(6)),
                    );
                    if b.clicked() {
                        self.send(&ctx, &id);
                    }
                }
                if !self.message.is_empty() {
                    ui.label(RichText::new(&self.message).color(p.err));
                }
            });
    }
}

// ---------------------------------------------------------------------------------- main app

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Devices,
    Pairing,
    General,
    Radial,
    Advanced,
}

impl Page {
    const ALL: [(Page, &'static str); 5] = [
        (Page::Devices, "Perangkat"),
        (Page::Pairing, "Pairing"),
        (Page::General, "Umum"),
        (Page::Radial, "Menu radial"),
        (Page::Advanced, "Lanjutan"),
    ];

    fn title(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(p, _)| *p == self)
            .map_or("", |(_, t)| t)
    }
}

#[derive(Clone)]
struct PeerRow {
    id: String,
    name: String,
    state: String,
    addr: String,
    transport: String,
}

struct Pairing {
    uri: String,
    qr: egui::TextureHandle,
    expires: Instant,
}

enum Note {
    Info(String),
    Error(String),
}

struct SettingsApp {
    backend: Arc<Backend>,
    logo: Option<egui::TextureHandle>,
    mode: Mode,
    page: Page,
    daemon_up: bool,
    paired: bool,
    config: Config,
    name_edit: String,
    peers: Vec<PeerRow>,
    revoke: Vec<bool>,
    confirm_rotate: bool,
    recording_key: bool,
    record_armed: bool,
    pairing: Option<Pairing>,
    join_text: String,
    note: Option<(Note, Instant)>,
    last_poll: Option<Instant>,
    send_to: bool,
    start_menu: bool,
}

impl SettingsApp {
    fn new(backend: Arc<Backend>, ctx: &egui::Context) -> Self {
        let mut app = Self {
            backend,
            logo: logo_texture(ctx),
            mode: Mode::Auto,
            page: Page::Devices,
            daemon_up: false,
            paired: false,
            config: Config::default(),
            name_edit: String::new(),
            peers: Vec::new(),
            revoke: Vec::new(),
            confirm_rotate: false,
            recording_key: false,
            record_armed: false,
            pairing: None,
            join_text: String::new(),
            note: None,
            last_poll: None,
            send_to: crate::sendto::is_enabled(),
            start_menu: crate::sendto::start_menu_enabled(),
        };
        app.poll(true);
        app
    }

    fn info(&mut self, text: impl Into<String>) {
        self.note = Some((Note::Info(text.into()), Instant::now()));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.note = Some((Note::Error(text.into()), Instant::now()));
    }

    fn poll(&mut self, first: bool) {
        self.last_poll = Some(Instant::now());
        match self.backend.call("get_status", Value::Null) {
            Ok(st) => {
                self.daemon_up = true;
                self.paired = st.get("paired").and_then(Value::as_bool).unwrap_or(false);
            }
            Err(_) => {
                self.daemon_up = false;
                return;
            }
        }
        if let Ok(v) = self.backend.call("get_config", Value::Null) {
            if let Ok(cfg) = serde_json::from_value::<Config>(v) {
                if first || self.name_edit.is_empty() {
                    self.name_edit.clone_from(&cfg.device_name);
                }
                self.mode = Mode::parse(&cfg.theme);
                self.config = cfg;
            }
        }
        if let Ok(Value::Array(list)) = self.backend.call("list_peers", Value::Null) {
            let text = |p: &Value, k: &str| {
                p.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            self.peers = list
                .iter()
                .map(|p| PeerRow {
                    id: text(p, "id"),
                    name: text(p, "name"),
                    state: text(p, "state"),
                    addr: text(p, "addr"),
                    transport: text(p, "transport"),
                })
                .collect();
            self.revoke.resize(self.peers.len(), false);
        }
    }

    fn set_config(&mut self, patch: Value) {
        match self.backend.call("set_config", patch) {
            Ok(v) => {
                if let Ok(cfg) = serde_json::from_value::<Config>(v) {
                    self.mode = Mode::parse(&cfg.theme);
                    self.config = cfg;
                }
            }
            Err(e) => self.error(format!("Gagal menyimpan: {e}")),
        }
    }

    fn online_count(&self) -> usize {
        self.peers.iter().filter(|p| p.state == "online").count()
    }

    fn show_pairing_code(&mut self, ctx: &egui::Context) {
        let method = if self.paired {
            "get_pairing_uri"
        } else {
            "create_group"
        };
        match self.backend.call(method, Value::Null) {
            Ok(v) => {
                let Some(uri) = v.get("uri").and_then(Value::as_str) else {
                    self.error("Balasan daemon tidak dikenali");
                    return;
                };
                let tex = ctx.load_texture(
                    "pairing-qr",
                    qr_image(uri)
                        .unwrap_or_else(|| egui::ColorImage::filled([1, 1], Color32::WHITE)),
                    egui::TextureOptions::NEAREST,
                );
                self.pairing = Some(Pairing {
                    uri: uri.to_owned(),
                    qr: tex,
                    expires: Instant::now() + Duration::from_secs(120),
                });
                self.paired = true;
                self.note = None;
            }
            Err(e) => self.error(format!("Gagal membuat kode: {e}")),
        }
    }

    fn set_dir(&mut self, dir: &str) {
        match self.backend.call("set_download_dir", json!({ "dir": dir })) {
            Ok(_) => {
                self.config.download_dir = dir.to_owned();
                self.note = None;
            }
            Err(e) => self.error(format!("Folder tidak dapat dipakai: {e}")),
        }
    }

    // ------------------------------------------------------------------ chrome

    fn sidebar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(34.0)));
            }
            ui.vertical(|ui| {
                ui.add_space(1.0);
                ui.label(theme::bold("TransClip", 17.0));
                let (dot, text) = if !self.daemon_up {
                    (p.err, "Tidak berjalan".to_owned())
                } else if self.online_count() > 0 {
                    (p.ok, format!("{} perangkat terhubung", self.online_count()))
                } else if self.paired {
                    (p.warn, "Menunggu perangkat".to_owned())
                } else {
                    (p.text2, "Belum ada grup".to_owned())
                };
                ui.horizontal(|ui| {
                    theme::dot(ui, dot);
                    ui.label(theme::secondary(text, p));
                });
            });
        });
        ui.add_space(14.0);
        theme::rule(ui, p);
        ui.add_space(10.0);
        for (page, label) in Page::ALL {
            if theme::nav_item(ui, p, label, self.page == page) {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(8.0);
            ui.label(theme::secondary(
                format!("Versi {}", env!("CARGO_PKG_VERSION")),
                p,
            ));
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.page.title()).heading());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(m) = theme::theme_switcher(ui, p, self.mode) {
                    self.mode = m;
                    self.set_config(json!({ "theme": m.as_str() }));
                }
            });
        });
        ui.add_space(4.0);
        theme::rule(ui, p);
    }

    fn note_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        if self
            .note
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(8))
        {
            self.note = None;
        }
        let Some((note, _)) = &self.note else { return };
        let (text, colour) = match note {
            Note::Info(t) => (t.clone(), p.ok),
            Note::Error(t) => (t.clone(), p.err),
        };
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        ui.add_space(8.0);
        egui::Frame::NONE
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::dot(ui, colour);
                    ui.label(text);
                });
            });
    }

    // ------------------------------------------------------------------ pages

    fn page_devices(&mut self, ui: &mut egui::Ui, p: &Palette) {
        if !self.paired {
            empty_state(
                ui,
                p,
                "Belum tergabung dalam grup",
                "Buat grup baru di PC ini, lalu pindai kodenya dari perangkat lain, atau gabung dengan kode dari perangkat yang sudah punya grup.",
            );
            if theme::primary_button(ui, p, "Buka Pairing").clicked() {
                self.page = Page::Pairing;
            }
            return;
        }
        if self.peers.is_empty() {
            empty_state(
                ui,
                p,
                "Belum ada perangkat lain",
                "Pastikan perangkat lain berada di jaringan yang sama dan TransClip berjalan di sana. Bila belum pernah dipasangkan, gunakan halaman Pairing.",
            );
            return;
        }
        let mut priority = self.config.bt_priority_peers.clone();
        for peer in self.peers.clone() {
            let online = peer.state == "online";
            let colour = match peer.state.as_str() {
                "online" => p.ok,
                "connecting" => p.warn,
                _ => p.text2,
            };
            ui.horizontal(|ui| {
                ui.set_min_height(56.0);
                ui.add_space(2.0);
                theme::dot(ui, colour);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let name = if peer.name.is_empty() {
                            "(tanpa nama)"
                        } else {
                            &peer.name
                        };
                        ui.label(theme::bold(name, 14.5));
                        match peer.transport.as_str() {
                            "lan" => theme::badge(ui, p, "Wi-Fi", p.ok),
                            "bluetooth" => theme::badge(ui, p, "Bluetooth", p.warn),
                            _ => {}
                        }
                    });
                    let state = match peer.state.as_str() {
                        "online" => "Terhubung",
                        "connecting" => "Menghubungkan…",
                        _ => "Tidak terhubung",
                    };
                    ui.horizontal(|ui| {
                        ui.label(theme::secondary(state, p));
                        if online && !peer.addr.is_empty() {
                            ui.label(theme::mono(&peer.addr, p));
                        }
                        let short: String = peer.id.chars().take(8).collect();
                        ui.label(theme::mono(short, p));
                    });
                    ui.add_space(8.0);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut on = priority.contains(&peer.id);
                    if theme::toggle(ui, p, &mut on).changed() {
                        if on {
                            priority.push(peer.id.clone());
                        } else {
                            priority.retain(|x| x != &peer.id);
                        }
                    }
                    ui.label(theme::secondary("Bluetooth bila Wi-Fi putus", p));
                });
            });
            theme::rule(ui, p);
        }
        if priority != self.config.bt_priority_peers {
            self.set_config(json!({ "bt_priority_peers": priority }));
        }
        ui.add_space(8.0);
        ui.label(theme::secondary(
            "Bluetooth hanya dipakai bila perangkat sudah dipasangkan di Windows dan Wi-Fi putus lebih dari 5 detik.",
            p,
        ));
    }

    fn page_pairing(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let ctx = ui.ctx().clone();
        theme::section(ui, p, "Tambah perangkat ke grup ini");
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.label(theme::secondary(
                    "1. Tampilkan kode di PC ini.\n2. Di perangkat lain buka TransClip, pilih Scan QR, lalu arahkan kamera ke layar.\n3. Kode berlaku 2 menit dan hanya dipakai sekali untuk bergabung.",
                    p,
                ));
                ui.add_space(8.0);
                let label = if self.paired {
                    "Tampilkan kode pairing"
                } else {
                    "Buat grup baru"
                };
                if theme::primary_button(ui, p, label).clicked() {
                    self.show_pairing_code(&ctx);
                }
                let mut expired = false;
                if let Some(pairing) = &self.pairing {
                    let left = pairing
                        .expires
                        .saturating_duration_since(Instant::now())
                        .as_secs();
                    if left == 0 {
                        expired = true;
                    } else {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            theme::badge(
                                ui,
                                p,
                                &format!("Berlaku {}:{:02}", left / 60, left % 60),
                                p.warn,
                            );
                            if theme::secondary_button(ui, p, "Salin kode").clicked() {
                                ctx.copy_text(pairing.uri.clone());
                            }
                        });
                        ctx.request_repaint_after(Duration::from_millis(500));
                    }
                }
                if expired {
                    self.pairing = None;
                    self.error("Kode kedaluwarsa. Buat kode baru.");
                }
            });
            ui.add_space(24.0);
            if let Some(pairing) = &self.pairing {
                // A QR code needs a light quiet zone, so it sits on white in both themes.
                egui::Frame::NONE
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.add(egui::Image::new(&pairing.qr).fit_to_exact_size(Vec2::splat(216.0)));
                    });
            }
        });

        theme::section(ui, p, "Gabung ke grup lain");
        ui.add_space(6.0);
        ui.label(theme::secondary(
            "Tempel kode pairing dari perangkat yang sudah punya grup.",
            p,
        ));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.join_text)
                    .desired_width(ui.available_width() - 110.0)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("transclip://pair?…"),
            );
            if theme::primary_button(ui, p, "Gabung").clicked() {
                match self
                    .backend
                    .call("join_group", json!({ "uri": self.join_text.trim() }))
                {
                    Ok(_) => {
                        self.info("Berhasil bergabung.");
                        self.join_text.clear();
                        self.paired = true;
                    }
                    Err(e) => self.error(format!("Gagal bergabung: {e}")),
                }
            }
        });

        if self.paired {
            theme::section(ui, p, "Ganti Group Key");
            ui.add_space(6.0);
            ui.label(theme::secondary(
                "Membuat kunci baru untuk seluruh grup. Perangkat yang dicentang dicabut dan harus dipasangkan ulang; yang lain tersambung ulang otomatis.",
                p,
            ));
            for (i, peer) in self.peers.clone().iter().enumerate() {
                if let Some(flag) = self.revoke.get_mut(i) {
                    ui.horizontal(|ui| {
                        theme::toggle(ui, p, flag);
                        ui.label(format!("Cabut {}", peer.name));
                    });
                }
            }
            ui.add_space(4.0);
            if self.confirm_rotate {
                ui.horizontal(|ui| {
                    if theme::danger_button(ui, p, "Ya, ganti kunci sekarang").clicked() {
                        let revoked: Vec<&String> = self
                            .peers
                            .iter()
                            .zip(&self.revoke)
                            .filter(|(_, r)| **r)
                            .map(|(peer, _)| &peer.id)
                            .collect();
                        match self
                            .backend
                            .call("rotate_key", json!({ "revoke": revoked }))
                        {
                            Ok(_) => self.info(
                                "Kunci diganti. Perangkat yang dicabut perlu dipasangkan ulang.",
                            ),
                            Err(e) => self.error(format!("Gagal mengganti kunci: {e}")),
                        }
                        self.confirm_rotate = false;
                    }
                    if theme::secondary_button(ui, p, "Batal").clicked() {
                        self.confirm_rotate = false;
                    }
                });
            } else if theme::secondary_button(ui, p, "Ganti Group Key…").clicked() {
                self.confirm_rotate = true;
            }
        }
    }

    fn page_general(&mut self, ui: &mut egui::Ui, p: &Palette) {
        theme::section(ui, p, "Perangkat ini");
        let mut apply_name = false;
        theme::row(
            ui,
            p,
            "Nama perangkat",
            Some("Nama yang terlihat di perangkat lain."),
            |ui| {
                if theme::primary_button(ui, p, "Terapkan").clicked() {
                    apply_name = true;
                }
                ui.add(egui::TextEdit::singleline(&mut self.name_edit).desired_width(200.0));
            },
        );
        if apply_name {
            let name = self.name_edit.clone();
            self.set_config(json!({ "device_name": name }));
        }

        theme::section(ui, p, "Sinkronisasi");
        let mut c = self.config.clone();
        let mut changed = false;
        for (title, desc, field) in [
            (
                "Jeda sinkronisasi",
                "Hentikan sementara semua pengiriman dan penerimaan.",
                &mut c.paused,
            ),
            (
                "Sinkronkan teks",
                "Teks yang disalin dikirim ke perangkat lain.",
                &mut c.sync_text,
            ),
            (
                "Sinkronkan gambar",
                "Gambar dikirim tanpa kompresi (PNG).",
                &mut c.sync_images,
            ),
            (
                "Terima file",
                "Izinkan perangkat lain mengirim file dan folder ke sini.",
                &mut c.receive_files,
            ),
        ] {
            theme::row(ui, p, title, Some(desc), |ui| {
                changed |= theme::toggle(ui, p, field).changed();
            });
        }
        if changed {
            self.set_config(json!({
                "paused": c.paused, "sync_text": c.sync_text, "sync_images": c.sync_images,
                "receive_files": c.receive_files,
            }));
        }

        theme::section(ui, p, "Penyimpanan file");
        let shown = if self.config.download_dir.is_empty() {
            crate::paths::default_receive_dir().map_or_else(
                |_| "Downloads\\TransClip".into(),
                |d| d.display().to_string(),
            )
        } else {
            self.config.download_dir.clone()
        };
        let mut pick = false;
        let mut reset = false;
        let mut open = false;
        let custom = !self.config.download_dir.is_empty();
        theme::row(ui, p, "Folder penerimaan", None, |ui| {
            if theme::secondary_button(ui, p, "Buka").clicked() {
                open = true;
            }
            if custom && theme::secondary_button(ui, p, "Reset").clicked() {
                reset = true;
            }
            if theme::secondary_button(ui, p, "Ubah…").clicked() {
                pick = true;
            }
        });
        ui.label(theme::mono(&shown, p));
        ui.add_space(6.0);
        if pick {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                self.set_dir(&dir.display().to_string());
            }
        }
        if reset {
            self.set_dir("");
        }
        if open {
            let _ = std::process::Command::new("explorer.exe")
                .arg(&shown)
                .spawn();
        }

        theme::section(ui, p, "Windows");
        let mut sys = [self.config.autostart, self.send_to, self.start_menu];
        let before = sys;
        for (i, (title, desc)) in [
            (
                "Jalankan saat Windows mulai",
                "TransClip aktif di tray setelah masuk ke Windows.",
            ),
            (
                "Tampilkan di menu Kirim ke",
                "Klik kanan file, Kirim ke, TransClip.",
            ),
            (
                "Tampilkan di menu Start dan pencarian",
                "Cari “TransClip” di Windows Search untuk membuka pengaturan.",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            theme::row(ui, p, title, Some(desc), |ui| {
                theme::toggle(ui, p, &mut sys[i]);
            });
        }
        if sys[0] != before[0] {
            self.set_config(json!({ "autostart": sys[0] }));
        }
        if sys[1] != before[1] {
            match self
                .backend
                .call("set_sendto", json!({ "enabled": sys[1] }))
            {
                Ok(_) => self.send_to = sys[1],
                Err(e) => self.error(format!("Gagal mengatur menu Kirim ke: {e}")),
            }
        }
        if sys[2] != before[2] {
            self.set_config(json!({ "start_menu": sys[2] }));
            self.start_menu = sys[2];
        }
    }

    /// While recording, watch the keyboard (the window does not need focus) and store the first key.
    fn poll_key_recording(&mut self, ctx: &egui::Context) {
        use crate::radial::keys;
        if !self.recording_key {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(30));
        if !self.record_armed {
            self.record_armed = !keys::any_held() && !keys::escape_down();
            return;
        }
        if keys::escape_down() {
            self.recording_key = false;
            return;
        }
        if let Some(vk) = keys::pressed_key() {
            self.recording_key = false;
            self.set_config(json!({ "radial_trigger": keys::vk_to_name(vk) }));
        }
    }

    fn page_radial(&mut self, ui: &mut egui::Ui, p: &Palette) {
        self.poll_key_recording(ui.ctx());
        ui.add_space(8.0);
        ui.label(theme::secondary(
            "Tahan klik kiri pada file, seret, lalu tekan tombol pemicu. Lingkaran perangkat muncul di kursor; lepas file di perangkat tujuan, atau di pusat lingkaran untuk membatalkan. Esc juga membatalkan.",
            p,
        ));
        theme::section(ui, p, "Tombol pemicu");
        let mut record = false;
        let mut reset = false;
        let current = crate::radial::keys::display(&self.config.radial_trigger);
        let recording = self.recording_key;
        let is_default = self.config.radial_trigger == crate::radial::keys::DEFAULT;
        theme::row(
            ui,
            p,
            "Tombol",
            Some("Tombol kiri dan kanan dianggap sama."),
            |ui| {
                if recording {
                    ui.label(RichText::new("Tekan tombol… (Esc batal)").color(p.warn));
                } else {
                    if !is_default && theme::secondary_button(ui, p, "Reset").clicked() {
                        reset = true;
                    }
                    if theme::secondary_button(ui, p, "Rekam tombol").clicked() {
                        record = true;
                    }
                    ui.label(theme::mono(&current, p));
                }
            },
        );
        if record {
            self.recording_key = true;
            self.record_armed = false;
        }
        if reset {
            self.set_config(json!({ "radial_trigger": crate::radial::keys::DEFAULT }));
        }
        theme::section(ui, p, "Ukuran");
        let mut radius = self.config.radial_radius_px;
        let mut released = false;
        theme::row(
            ui,
            p,
            "Radius lingkaran",
            Some("Dalam piksel pada skala 100 %."),
            |ui| {
                let r = ui.add(egui::Slider::new(&mut radius, 80..=300));
                released = r.drag_stopped() || (r.changed() && !r.dragged());
            },
        );
        if released {
            self.set_config(json!({ "radial_radius_px": radius }));
        }
        ui.add_space(8.0);
        ui.label(theme::secondary(
            "Drag dari aplikasi yang dijalankan sebagai Administrator tidak didukung oleh Windows (UIPI).",
            p,
        ));
    }

    fn page_advanced(&mut self, ui: &mut egui::Ui, p: &Palette) {
        theme::section(ui, p, "Jaringan");
        theme::row(
            ui,
            p,
            "Port TCP",
            Some("Diubah lewat berkas konfigurasi, lalu jalankan ulang."),
            |ui| {
                ui.label(theme::mono(self.config.port.to_string(), p));
            },
        );
        theme::section(ui, p, "Diagnostik");
        let mut level = self.config.log_level.clone();
        theme::row(
            ui,
            p,
            "Level log",
            Some("Log tidak pernah memuat isi clipboard atau kunci. Berlaku setelah daemon dijalankan ulang."),
            |ui| {
                egui::ComboBox::from_id_salt("log-level")
                    .selected_text(&level)
                    .show_ui(ui, |ui| {
                        for l in LOG_LEVELS {
                            ui.selectable_value(&mut level, l.to_owned(), l);
                        }
                    });
            },
        );
        if level != self.config.log_level {
            self.set_config(json!({ "log_level": level }));
        }
        let mut logs = false;
        theme::row(ui, p, "Berkas log", None, |ui| {
            logs = theme::secondary_button(ui, p, "Buka folder log").clicked();
        });
        if logs {
            let _ = self.backend.call("open_log_dir", Value::Null);
        }
        theme::section(ui, p, "Tentang");
        theme::row(ui, p, "Versi", None, |ui| {
            ui.label(theme::mono(env!("CARGO_PKG_VERSION"), p));
        });
    }

    fn not_running(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.28);
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(72.0)));
            }
            ui.add_space(12.0);
            ui.label(theme::bold("TransClip belum berjalan", 20.0));
            ui.add_space(4.0);
            ui.label(theme::secondary(
                "Jendela ini mengatur TransClip yang berjalan di tray. Jalankan dulu untuk melanjutkan.",
                p,
            ));
            ui.add_space(14.0);
            if theme::primary_button(ui, p, "Jalankan TransClip").clicked() {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).spawn();
                }
                self.last_poll = None;
            }
        });
    }
}

fn empty_state(ui: &mut egui::Ui, p: &Palette, title: &str, body: &str) {
    ui.add_space(28.0);
    ui.label(theme::bold(title, 17.0));
    ui.add_space(4.0);
    ui.label(theme::secondary(body, p));
    ui.add_space(12.0);
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.last_poll.is_none_or(|t| t.elapsed() >= POLL_EVERY) {
            self.poll(false);
        }
        ui.ctx().request_repaint_after(POLL_EVERY);
        let p = theme::palette(self.mode.is_dark());
        theme::apply(ui.ctx(), &p);

        egui::Panel::left("nav")
            .exact_size(224.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ui, |ui| self.sidebar(ui, &p));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(p.canvas)
                    .inner_margin(egui::Margin::symmetric(28, 20)),
            )
            .show(ui, |ui| {
                if !self.daemon_up {
                    self.not_running(ui, &p);
                    return;
                }
                self.header(ui, &p);
                self.note_bar(ui, &p);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Devices => self.page_devices(ui, &p),
                        Page::Pairing => self.page_pairing(ui, &p),
                        Page::General => self.page_general(ui, &p),
                        Page::Radial => self.page_radial(ui, &p),
                        Page::Advanced => self.page_advanced(ui, &p),
                    });
            });
    }
}

/// Render `text` as a QR code with a four-module quiet zone, black on white.
fn qr_image(text: &str) -> Option<egui::ColorImage> {
    const SCALE: usize = 6;
    const QUIET: usize = 4;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let modules = code.width();
    let side = (modules + 2 * QUIET) * SCALE;
    let colors = code.to_colors();
    let mut pixels = vec![255u8; side * side];
    for y in 0..modules {
        for x in 0..modules {
            if colors[y * modules + x] == qrcode::Color::Dark {
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        pixels[((y + QUIET) * SCALE + dy) * side + (x + QUIET) * SCALE + dx] = 0;
                    }
                }
            }
        }
    }
    Some(egui::ColorImage::from_gray([side, side], &pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_is_square_with_quiet_zone() {
        let img = qr_image("transclip://pair?v=1&k=abc").expect("qr");
        assert_eq!(img.size[0], img.size[1]);
        // The corner belongs to the quiet zone, so it is white.
        assert_eq!(img.pixels[0], egui::Color32::WHITE);
        assert!(img.pixels.contains(&egui::Color32::BLACK));
    }

    #[test]
    fn real_pairing_uri_fits_in_a_qr_code() {
        let uri = "transclip://pair?v=1&k=Fw6I9SSL3av6jJ0FNrxQ5xZvVetFzvh58S4Gf_IGL4g&g=TransClip&d=e1bd36038e148152f9612881ba3da0b7&a=192.168.1.16%3A47820&x=1794974934";
        assert!(qr_image(uri).is_some());
    }

    #[test]
    fn every_page_has_a_title() {
        for (page, title) in Page::ALL {
            assert_eq!(page.title(), title);
        }
    }
}
