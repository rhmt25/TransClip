//! Settings window (PLAN section 5.6): a separate process (`transclip.exe --settings`) that
//! talks to the daemon over the named pipe, so the daemon's memory stays small.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use serde_json::{json, Value};

use crate::config::{Config, LOG_LEVELS};
use crate::ipc;

const POLL_EVERY: Duration = Duration::from_millis(1500);
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Open the window; returns the process exit code.
pub fn run() -> i32 {
    let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    else {
        return 1;
    };
    let backend = Arc::new(Backend { rt });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TransClip")
            .with_inner_size([560.0, 520.0])
            .with_min_inner_size([440.0, 400.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "TransClip",
        options,
        Box::new(move |_cc| Ok(Box::new(SettingsApp::new(backend)))),
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

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Devices,
    Pairing,
    General,
    Advanced,
}

struct Pairing {
    uri: String,
    qr: egui::TextureHandle,
    expires: Instant,
}

struct SettingsApp {
    backend: Arc<Backend>,
    tab: Tab,
    daemon_up: bool,
    paired: bool,
    config: Config,
    name_edit: String,
    peers: Vec<(String, String)>,
    peer_ids: Vec<String>,
    revoke: Vec<bool>,
    confirm_rotate: bool,
    pairing: Option<Pairing>,
    join_text: String,
    message: String,
    last_poll: Option<Instant>,
}

impl SettingsApp {
    fn new(backend: Arc<Backend>) -> Self {
        let mut app = Self {
            backend,
            tab: Tab::Devices,
            daemon_up: false,
            paired: false,
            config: Config::default(),
            name_edit: String::new(),
            peers: Vec::new(),
            peer_ids: Vec::new(),
            revoke: Vec::new(),
            confirm_rotate: false,
            pairing: None,
            join_text: String::new(),
            message: String::new(),
            last_poll: None,
        };
        app.poll(true);
        app
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
                self.config = cfg;
            }
        }
        if let Ok(Value::Array(list)) = self.backend.call("list_peers", Value::Null) {
            self.peer_ids = list
                .iter()
                .map(|p| p.get("id").and_then(Value::as_str).unwrap_or("").to_owned())
                .collect();
            self.revoke.resize(list.len(), false);
            self.peers = list
                .iter()
                .map(|p| {
                    let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
                    let state = p.get("state").and_then(Value::as_str).unwrap_or("?");
                    (name.to_owned(), state.to_owned())
                })
                .collect();
        }
    }

    fn set_config(&mut self, patch: Value) {
        match self.backend.call("set_config", patch) {
            Ok(v) => {
                if let Ok(cfg) = serde_json::from_value::<Config>(v) {
                    self.config = cfg;
                }
                self.message.clear();
            }
            Err(e) => self.message = format!("Gagal menyimpan: {e}"),
        }
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
                    self.message = "Balasan daemon tidak dikenali".into();
                    return;
                };
                let tex = ctx.load_texture(
                    "pairing-qr",
                    qr_image(uri)
                        .unwrap_or_else(|| egui::ColorImage::filled([1, 1], egui::Color32::WHITE)),
                    egui::TextureOptions::NEAREST,
                );
                self.pairing = Some(Pairing {
                    uri: uri.to_owned(),
                    qr: tex,
                    expires: Instant::now() + Duration::from_secs(120),
                });
                self.paired = true;
                self.message.clear();
            }
            Err(e) => self.message = format!("Gagal membuat kode: {e}"),
        }
    }

    fn ui_devices(&mut self, ui: &mut egui::Ui) {
        if !self.paired {
            ui.label("Belum tergabung dalam grup. Buka tab Pairing.");
            return;
        }
        if self.peers.is_empty() {
            ui.label("Belum ada perangkat lain.");
        }
        for (name, state) in &self.peers {
            ui.horizontal(|ui| {
                let color = if state == "online" {
                    egui::Color32::from_rgb(40, 160, 70)
                } else {
                    egui::Color32::GRAY
                };
                ui.colored_label(color, "●");
                ui.label(name);
                ui.weak(match state.as_str() {
                    "online" => "online",
                    "connecting" => "menghubungkan",
                    _ => "offline",
                });
            });
        }
    }

    fn ui_pairing(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if ui
            .button(if self.paired {
                "Tampilkan kode pairing"
            } else {
                "Buat grup baru"
            })
            .clicked()
        {
            self.show_pairing_code(&ctx);
        }
        let mut expired = false;
        if let Some(p) = &self.pairing {
            let left = p
                .expires
                .saturating_duration_since(Instant::now())
                .as_secs();
            if left == 0 {
                expired = true;
            } else {
                ui.add(egui::Image::new(&p.qr).fit_to_exact_size(egui::vec2(240.0, 240.0)));
                ui.label(format!("Berlaku {}:{:02}", left / 60, left % 60));
                if ui.button("Salin kode").clicked() {
                    ctx.copy_text(p.uri.clone());
                }
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        if expired {
            self.pairing = None;
            self.message = "Kode kedaluwarsa. Buat kode baru.".into();
        }
        ui.separator();
        ui.label("Ganti Group Key");
        ui.weak("Centang perangkat yang dicabut. Perangkat lain menerima kunci baru dan tersambung ulang otomatis.");
        for (i, (name, _)) in self.peers.iter().enumerate() {
            if let Some(flag) = self.revoke.get_mut(i) {
                ui.checkbox(flag, format!("Cabut {name}"));
            }
        }
        if self.confirm_rotate {
            ui.horizontal(|ui| {
                if ui.button("Ya, ganti kunci").clicked() {
                    let revoked: Vec<&String> = self
                        .peer_ids
                        .iter()
                        .zip(&self.revoke)
                        .filter(|(_, r)| **r)
                        .map(|(id, _)| id)
                        .collect();
                    self.message = match self
                        .backend
                        .call("rotate_key", json!({ "revoke": revoked }))
                    {
                        Ok(_) => {
                            "Kunci diganti. Perangkat yang dicabut perlu pairing ulang.".into()
                        }
                        Err(e) => format!("Gagal mengganti kunci: {e}"),
                    };
                    self.confirm_rotate = false;
                }
                if ui.button("Batal").clicked() {
                    self.confirm_rotate = false;
                }
            });
        } else if self.paired && ui.button("Ganti Group Key…").clicked() {
            self.confirm_rotate = true;
        }
        ui.separator();
        ui.label("Gabung dengan kode dari perangkat lain");
        ui.text_edit_singleline(&mut self.join_text);
        if ui.button("Gabung").clicked() {
            match self
                .backend
                .call("join_group", json!({ "uri": self.join_text.trim() }))
            {
                Ok(_) => {
                    self.message = "Berhasil bergabung.".into();
                    self.join_text.clear();
                    self.paired = true;
                }
                Err(e) => self.message = format!("Gagal bergabung: {e}"),
            }
        }
    }

    fn ui_general(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Nama perangkat");
            ui.text_edit_singleline(&mut self.name_edit);
            if ui.button("Terapkan").clicked() {
                let name = self.name_edit.clone();
                self.set_config(json!({ "device_name": name }));
            }
        });
        let mut c = self.config.clone();
        let mut changed = false;
        changed |= ui.checkbox(&mut c.paused, "Jeda sinkronisasi").changed();
        changed |= ui.checkbox(&mut c.sync_text, "Sinkronkan teks").changed();
        changed |= ui
            .checkbox(&mut c.sync_images, "Sinkronkan gambar")
            .changed();
        changed |= ui.checkbox(&mut c.receive_files, "Terima file").changed();
        changed |= ui
            .checkbox(&mut c.autostart, "Jalankan saat Windows mulai")
            .changed();
        if changed {
            self.set_config(json!({
                "paused": c.paused, "sync_text": c.sync_text, "sync_images": c.sync_images,
                "receive_files": c.receive_files, "autostart": c.autostart,
            }));
        }
        ui.separator();
        ui.label("Folder tujuan");
        let shown = if self.config.download_dir.is_empty() {
            crate::paths::default_receive_dir().map_or_else(
                |_| "Downloads\\TransClip".into(),
                |p| p.display().to_string(),
            )
        } else {
            self.config.download_dir.clone()
        };
        ui.monospace(&shown);
        ui.horizontal(|ui| {
            if ui.button("Ubah…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.set_dir(&dir.display().to_string());
                }
            }
            if ui.button("Reset ke default").clicked() {
                self.set_dir("");
            }
            if ui.button("Buka folder").clicked() {
                let _ = std::process::Command::new("explorer.exe")
                    .arg(&shown)
                    .spawn();
            }
        });
    }

    fn set_dir(&mut self, dir: &str) {
        match self.backend.call("set_download_dir", json!({ "dir": dir })) {
            Ok(_) => {
                self.config.download_dir = dir.to_owned();
                self.message.clear();
            }
            Err(e) => self.message = format!("Folder tidak dapat dipakai: {e}"),
        }
    }

    fn ui_advanced(&mut self, ui: &mut egui::Ui) {
        ui.label(format!("Port: {}", self.config.port));
        ui.horizontal(|ui| {
            ui.label("Level log");
            let mut level = self.config.log_level.clone();
            egui::ComboBox::from_id_salt("log-level")
                .selected_text(&level)
                .show_ui(ui, |ui| {
                    for l in LOG_LEVELS {
                        ui.selectable_value(&mut level, l.to_owned(), l);
                    }
                });
            if level != self.config.log_level {
                self.set_config(json!({ "log_level": level }));
                self.message = "Level log berlaku setelah daemon dijalankan ulang.".into();
            }
        });
        if ui.button("Buka folder log").clicked() {
            let _ = self.backend.call("open_log_dir", Value::Null);
        }
        ui.weak(format!("Versi {}", env!("CARGO_PKG_VERSION")));
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.last_poll.is_none_or(|t| t.elapsed() >= POLL_EVERY) {
            self.poll(false);
        }
        ui.ctx().request_repaint_after(POLL_EVERY);
        egui::CentralPanel::default().show(ui, |ui| {
            if !self.daemon_up {
                ui.heading("TransClip belum berjalan");
                ui.label("Jendela ini mengatur daemon TransClip yang berjalan di tray.");
                if ui.button("Jalankan TransClip").clicked() {
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    self.last_poll = None;
                }
                return;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Devices, "Perangkat");
                ui.selectable_value(&mut self.tab, Tab::Pairing, "Pairing");
                ui.selectable_value(&mut self.tab, Tab::General, "Umum");
                ui.selectable_value(&mut self.tab, Tab::Advanced, "Lanjutan");
            });
            ui.separator();
            match self.tab {
                Tab::Devices => self.ui_devices(ui),
                Tab::Pairing => self.ui_pairing(ui),
                Tab::General => self.ui_general(ui),
                Tab::Advanced => self.ui_advanced(ui),
            }
            if !self.message.is_empty() {
                ui.separator();
                ui.colored_label(egui::Color32::from_rgb(200, 90, 30), &self.message);
            }
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
}
