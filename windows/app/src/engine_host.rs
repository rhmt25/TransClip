//! The tokio side: owns the engine, serves the IPC pipe and answers UI commands.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use engine::clip::{Clip, ClipConfig, ClipboardSync};
use engine::discovery::beacon::{BeaconConfig, BeaconService};
use engine::discovery::mdns::{MdnsConfig, MdnsService};
use engine::link::LocalInfo;
use engine::peers::{ManagerConfig, ManagerEvent, PeerManager, PeerState, PeerStatus};
use engine::transfer::{scan, FsStore, TransferConfig, TransferEvent, TransferManager};
use proto::beacon::Platform;
use proto::group::GroupKey;
use proto::ids::DeviceId;
use proto::pairing::PairingInfo;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use crate::bridge::{Status, ToEngine, ToUi, UiHandle};
use crate::clipboard::write::WindowsSink;
use crate::config::Config;
use crate::ipc::protocol::Event;
use crate::ipc::server::{Handler, IpcServer};
use crate::paths::Paths;
use crate::secrets::Secrets;
use crate::strings;

/// Pairing codes are valid for two minutes (PLAN section 7).
const PAIRING_TTL_SECS: u64 = 120;

/// Everything the engine needs from the daemon's start-up.
pub struct HostInput {
    /// User settings.
    pub config: Config,
    /// Data folders.
    pub paths: Paths,
    /// This device.
    pub device_id: DeviceId,
    /// Sender for commands to this host (used by IPC requests).
    pub commands: mpsc::UnboundedSender<ToEngine>,
}

fn open_folder(path: &Path) {
    // The shell opens the folder in Explorer; failure is not actionable for the user.
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// State shared between the command loop, status watchers and IPC handlers.
struct Hub {
    ui: UiHandle,
    status: Mutex<Status>,
    ipc: OnceLock<IpcServer>,
    peers: Mutex<Option<watch::Receiver<Vec<PeerStatus>>>>,
    transfers: Mutex<Option<TransferManager>>,
    /// Peer manager of the running engine, for operations started over IPC.
    live_mgr: Mutex<Option<Arc<PeerManager>>>,
    online: Mutex<Vec<DeviceId>>,
    commands: mpsc::UnboundedSender<ToEngine>,
    /// Current settings; changed at runtime through IPC.
    config: Mutex<Config>,
    /// The settings window process, while it is open.
    settings_child: Mutex<Option<std::process::Child>>,
    paths: Paths,
    device_id: DeviceId,
}

impl Hub {
    fn cfg(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Start sending `paths` to one device (hex id) or to every online device (`"all"`).
    fn send_paths(&self, paths: &[std::path::PathBuf], to: &str) -> Result<(), String> {
        let tm = self
            .transfers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or("not in a group yet")?;
        let targets: Vec<DeviceId> = if to == "all" {
            self.online
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        } else {
            vec![DeviceId::from_hex(to).map_err(|_| "bad device id")?]
        };
        if targets.is_empty() {
            return Err("no device is online".into());
        }
        let batches = scan::batch_from_paths(paths).map_err(|e| e.to_string())?;
        for peer in targets {
            for batch in batches.clone() {
                let tm = tm.clone();
                tokio::spawn(async move {
                    if let Err(e) = tm.send_batch(peer, batch).await {
                        tracing::warn!(error = %e, "could not start the transfer");
                    }
                });
            }
        }
        Ok(())
    }

    fn download_dir(&self) -> String {
        self.cfg().download_dir
    }

    /// Persist and publish a new configuration.
    fn store_config(&self, cfg: Config) -> Result<(), String> {
        cfg.save(&self.paths.config_file())
            .map_err(|e| e.to_string())?;
        *self.config.lock().unwrap_or_else(PoisonError::into_inner) = cfg;
        Ok(())
    }

    /// Open the settings window unless one is already open.
    fn show_settings(&self) {
        tracing::info!("ShowSettings requested");
        if std::env::var_os("TRANSCLIP_NO_WINDOW").is_some() {
            return;
        }
        let mut child = self
            .settings_child
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
        {
            return;
        }
        *child = std::env::current_exe().ok().and_then(|exe| {
            std::process::Command::new(exe)
                .arg("--settings")
                .spawn()
                .ok()
        });
    }

    /// Change the status, then tell the tray and IPC clients.
    fn update(&self, change: impl FnOnce(&mut Status)) {
        let snapshot = {
            let mut s = self.status.lock().unwrap_or_else(PoisonError::into_inner);
            change(&mut s);
            *s
        };
        self.ui.send(ToUi::Status(snapshot));
        if let Some(ipc) = self.ipc.get() {
            ipc.publish(Event {
                event: "status_changed".into(),
                data: status_json(&snapshot),
            });
        }
    }

    fn snapshot(&self) -> Status {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn peers_json(&self) -> Value {
        let list = self
            .peers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|rx| rx.borrow().clone())
            .unwrap_or_default();
        Value::Array(
            list.iter()
                .map(|p| {
                    json!({
                        "id": p.id.to_hex(),
                        "name": p.name,
                        "state": match p.state {
                            PeerState::Online => "online",
                            PeerState::Connecting => "connecting",
                            PeerState::Offline => "offline",
                        },
                        "addr": p.addr.map(|a| a.to_string()),
                    })
                })
                .collect(),
        )
    }

    /// Build a fresh pairing URI for the current group.
    fn pairing_uri(&self, gk: GroupKey) -> String {
        let port = self.cfg().port;
        PairingInfo {
            gk,
            group_name: "TransClip".into(),
            inviter: self.device_id,
            addrs: local_addresses(port),
            expires: now_secs() + PAIRING_TTL_SECS,
        }
        .to_uri()
    }
}

/// Private IPv4 addresses of this PC as `ip:port`, best effort.
fn local_addresses(port: u16) -> Vec<String> {
    // Connecting a UDP socket sends nothing but makes the OS pick the outgoing interface.
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        s.connect("192.0.2.1:9")?;
        s.local_addr()
    });
    match probe {
        Ok(a) if !a.ip().is_unspecified() => vec![format!("{}:{port}", a.ip())],
        _ => Vec::new(),
    }
}

fn status_json(s: &Status) -> Value {
    json!({"online": s.online, "paired": s.paired, "paused": s.paused, "error": s.error})
}

/// Methods are answered here; the ones that need later tasks report "not implemented".
fn handler(hub: Arc<Hub>) -> Handler {
    Arc::new(move |method: String, params: Value| {
        let hub = Arc::clone(&hub);
        Box::pin(async move {
            match method.as_str() {
                "get_status" => Ok(status_json(&hub.snapshot())),
                "get_config" => serde_json::to_value(hub.cfg()).map_err(|e| e.to_string()),
                "list_peers" => Ok(hub.peers_json()),
                "set_paused" => {
                    let paused = params
                        .get("paused")
                        .and_then(Value::as_bool)
                        .ok_or("params.paused must be a boolean")?;
                    let _ = hub.commands.send(ToEngine::SetPaused(paused));
                    Ok(Value::Null)
                }
                "set_download_dir" => {
                    // An empty string resets to the default folder.
                    let dir = params
                        .get("dir")
                        .and_then(Value::as_str)
                        .ok_or("params.dir must be a string")?
                        .trim()
                        .to_owned();
                    if !dir.is_empty() {
                        crate::target_dir::validate(Path::new(&dir))?;
                    }
                    let mut cfg = hub.cfg();
                    cfg.download_dir = dir;
                    hub.store_config(cfg)?;
                    let _ = hub.commands.send(ToEngine::Reload);
                    Ok(json!({ "dir": crate::target_dir::resolve(&hub.download_dir()).dir }))
                }
                "set_sendto" => {
                    let enabled = params
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .ok_or("params.enabled must be a boolean")?;
                    crate::sendto::set_enabled(enabled).map_err(|e| e.to_string())?;
                    Ok(Value::Null)
                }
                "rotate_key" => {
                    // params.revoke: device ids (hex) that must not receive the new key.
                    let revoke: Vec<DeviceId> = params
                        .get("revoke")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().and_then(|h| DeviceId::from_hex(h).ok()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mgr = hub
                        .live_mgr
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone()
                        .ok_or("not connected to a group")?;
                    let new_gk = GroupKey::generate().map_err(|e| e.to_string())?;
                    let targets: Vec<DeviceId> = mgr
                        .online_peers()
                        .into_iter()
                        .filter(|p| !revoke.contains(p))
                        .collect();
                    let sent = engine::rekey::broadcast(
                        &mgr,
                        &new_gk,
                        "TransClip",
                        hub.device_id,
                        &targets,
                    )
                    .await;
                    let path = hub.paths.secrets_file();
                    let mut secrets = Secrets::load_or_create(&path).map_err(|e| e.to_string())?;
                    secrets.group_key = Some(new_gk);
                    secrets.save(&path).map_err(|e| e.to_string())?;
                    // Let the frames leave before the links are torn down.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let _ = hub.commands.send(ToEngine::Reload);
                    Ok(json!({ "notified": sent, "revoked": revoke.len() }))
                }
                "show_settings" => {
                    let _ = hub.commands.send(ToEngine::ShowSettings);
                    Ok(Value::Null)
                }
                "set_config" => {
                    // Partial update: only the keys present change. The folder has its own method.
                    let Value::Object(patch) = params else {
                        return Err("params must be an object".into());
                    };
                    let old = hub.cfg();
                    let mut merged = serde_json::to_value(&old).map_err(|e| e.to_string())?;
                    if let Value::Object(map) = &mut merged {
                        for (k, v) in patch {
                            if k != "download_dir" {
                                map.insert(k, v);
                            }
                        }
                    }
                    let mut cfg: Config =
                        serde_json::from_value(merged).map_err(|e| e.to_string())?;
                    cfg.validate();
                    hub.store_config(cfg.clone())?;
                    let live = hub
                        .live_mgr
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    if let Some(m) = live {
                        m.set_bt_priority(bt_priority_set(&cfg.bt_priority_peers))
                            .await;
                    }
                    hub.ui.send(ToUi::RadialConfig {
                        trigger: cfg.radial_trigger.clone(),
                        radius: cfg.radial_radius_px,
                    });
                    if cfg.autostart != old.autostart
                        && std::env::var_os(crate::paths::DATA_DIR_ENV).is_none()
                    {
                        crate::autostart::apply(cfg.autostart).map_err(|e| e.to_string())?;
                    }
                    if cfg.paused != old.paused {
                        let _ = hub.commands.send(ToEngine::SetPaused(cfg.paused));
                    }
                    if cfg.device_name != old.device_name
                        || cfg.port != old.port
                        || cfg.receive_files != old.receive_files
                        || cfg.max_incoming_file_bytes != old.max_incoming_file_bytes
                    {
                        let _ = hub.commands.send(ToEngine::Reload);
                    }
                    serde_json::to_value(cfg).map_err(|e| e.to_string())
                }
                "open_log_dir" => {
                    open_folder(&hub.paths.log_dir());
                    Ok(Value::Null)
                }
                "create_group" => {
                    let path = hub.paths.secrets_file();
                    let mut secrets = Secrets::load_or_create(&path).map_err(|e| e.to_string())?;
                    let gk = match &secrets.group_key {
                        Some(k) => k.clone(),
                        None => {
                            let k = GroupKey::generate().map_err(|e| e.to_string())?;
                            secrets.group_key = Some(k.clone());
                            secrets.save(&path).map_err(|e| e.to_string())?;
                            let _ = hub.commands.send(ToEngine::Reload);
                            k
                        }
                    };
                    Ok(json!({ "uri": hub.pairing_uri(gk) }))
                }
                "get_pairing_uri" => {
                    let secrets = Secrets::load_or_create(&hub.paths.secrets_file())
                        .map_err(|e| e.to_string())?;
                    let gk = secrets.group_key.ok_or("not in a group yet")?;
                    Ok(json!({ "uri": hub.pairing_uri(gk) }))
                }
                "join_group" => {
                    let uri = params
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or("params.uri must be a string")?;
                    let info = PairingInfo::parse(uri).map_err(|_| "invalid pairing code")?;
                    if info.is_expired(now_secs()) {
                        return Err("the pairing code has expired".into());
                    }
                    let path = hub.paths.secrets_file();
                    let mut secrets = Secrets::load_or_create(&path).map_err(|e| e.to_string())?;
                    secrets.group_key = Some(info.gk);
                    secrets.save(&path).map_err(|e| e.to_string())?;
                    let _ = hub.commands.send(ToEngine::Reload);
                    Ok(json!({ "joined": info.group_name }))
                }
                "send_files" => {
                    let paths: Vec<std::path::PathBuf> = params
                        .get("paths")
                        .and_then(Value::as_array)
                        .ok_or("params.paths must be an array")?
                        .iter()
                        .filter_map(|v| v.as_str().map(Into::into))
                        .collect();
                    let to = params.get("to").and_then(Value::as_str).unwrap_or("all");
                    hub.send_paths(&paths, to)?;
                    Ok(Value::Null)
                }
                "quit" => {
                    hub.ui.send(ToUi::Quit);
                    Ok(Value::Null)
                }
                m if crate::ipc::protocol::METHODS.contains(&m) => {
                    Err(format!("not implemented yet: {m}"))
                }
                m => Err(format!("unknown method: {m}")),
            }
        })
    })
}

/// The running pieces that exist only while the device is in a group.
struct Running {
    mgr: Arc<PeerManager>,
    sync: ClipboardSync,
    beacon: BeaconService,
    _mdns: Option<MdnsService>,
    _bt: Option<crate::bluetooth::RfcommServer>,
}

impl Running {
    async fn stop(self) {
        self.mgr.stop().await;
    }
}

/// Start discovery, the peer manager and clipboard sync for `gk`.
async fn start_engine(
    hub: &Arc<Hub>,
    ui: &UiHandle,
    gk: GroupKey,
    paused: &Arc<AtomicBool>,
) -> Option<Running> {
    let config = &hub.cfg();
    hub.ui.send(ToUi::RadialConfig {
        trigger: config.radial_trigger.clone(),
        radius: config.radial_radius_px,
    });
    let device_id = hub.device_id;
    let gid = gk.group_id();
    let (seen_tx, seen_rx) = mpsc::channel(64);
    let local = LocalInfo {
        id: device_id,
        name: config.device_name.clone(),
        platform: "windows".into(),
        app: format!("transclip/{}", env!("CARGO_PKG_VERSION")),
        caps: vec!["clip".into()],
        role: "full".into(),
    };
    let listen = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    let mut manager_cfg = ManagerConfig::new(local, gk, listen);
    let bt_ok = crate::bluetooth::available();
    if bt_ok {
        manager_cfg.bt = Some(Arc::new(crate::bluetooth::WinBtDialer::new(
            tokio::runtime::Handle::current(),
        )));
    }
    let (mgr, mut events) = match PeerManager::start(manager_cfg, seen_rx).await {
        Ok(x) => x,
        Err(e) => {
            tracing::error!(error = %e, "cannot listen on the TCP port");
            hub.update(|s| s.error = true);
            ui.send(ToUi::Notify {
                title: strings::APP_NAME.into(),
                text: format!("Port {} tidak dapat dipakai", config.port),
            });
            return None;
        }
    };
    let mgr = Arc::new(mgr);
    let bt_server = if bt_ok {
        let m = Arc::clone(&mgr);
        let handle = tokio::runtime::Handle::current();
        // WinRT calls block, so the service is registered off the async workers.
        tokio::task::spawn_blocking(move || crate::bluetooth::RfcommServer::start(m, handle))
            .await
            .ok()
            .and_then(|r| {
                r.map_err(|e| tracing::info!(error = %e, "bluetooth is not available"))
                    .ok()
            })
    } else {
        None
    };
    mgr.set_bt_priority(bt_priority_set(&config.bt_priority_peers))
        .await;
    let beacon = match BeaconService::start(
        BeaconConfig::lan(
            gid,
            device_id,
            config.port,
            Platform::Windows,
            config.device_name.clone(),
        ),
        seen_tx.clone(),
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "cannot start discovery");
            hub.update(|s| s.error = true);
            mgr.stop().await;
            return None;
        }
    };
    let mdns = MdnsService::start(
        &MdnsConfig {
            gid,
            device_id,
            tcp_port: config.port,
            platform: Platform::Windows,
            name: config.device_name.clone(),
        },
        seen_tx,
    );
    if let Err(e) = &mdns {
        tracing::warn!(error = %e, "mDNS unavailable, relying on beacons");
    }
    hub.update(|s| s.error = false);

    *hub.peers.lock().unwrap_or_else(PoisonError::into_inner) = Some(mgr.status());
    let mut watch = mgr.status();
    let hub2 = Arc::clone(hub);
    tokio::spawn(async move {
        while watch.changed().await.is_ok() {
            let online = watch
                .borrow()
                .iter()
                .filter(|p| p.state == PeerState::Online)
                .count();
            *hub2.online.lock().unwrap_or_else(PoisonError::into_inner) = watch
                .borrow()
                .iter()
                .filter(|p| p.state == PeerState::Online)
                .map(|p| p.id)
                .collect();
            hub2.update(|s| s.online = online);
            let radial: Vec<(DeviceId, String)> = watch
                .borrow()
                .iter()
                .filter(|p| p.state == PeerState::Online)
                .map(|p| (p.id, p.name.clone()))
                .collect();
            hub2.ui.send(ToUi::Peers(radial));
            if let Some(ipc) = hub2.ipc.get() {
                ipc.publish(Event {
                    event: "peers_changed".into(),
                    data: hub2.peers_json(),
                });
            }
        }
    });
    let sink = Arc::new(WindowsSink::new(ui.hwnd(), Arc::clone(paused)));
    let (sync, mut clip_events) = ClipboardSync::new(
        Arc::clone(&mgr),
        sink,
        device_id,
        ClipConfig {
            bt_inline_image_max: usize::try_from(config.bt_inline_image_max).unwrap_or(usize::MAX),
            ..ClipConfig::default()
        },
    );
    let configured = hub.download_dir();
    let target = crate::target_dir::resolve(&configured);
    if target.fell_back {
        tracing::warn!("the configured receive folder is not usable; using the default");
        ui.send(ToUi::Notify {
            title: strings::APP_NAME.into(),
            text: "Folder tujuan tidak dapat dipakai, memakai folder default".into(),
        });
    }
    let receive_dir = target.dir;
    let (transfers, mut transfer_events) = TransferManager::new(
        Arc::clone(&mgr),
        Arc::new(FsStore::new(receive_dir).with_free_space(crate::paths::free_space)),
        TransferConfig {
            receive_files: config.receive_files,
            max_file_bytes: config.max_incoming_file_bytes,
            ..TransferConfig::default()
        },
    );
    *hub.transfers.lock().unwrap_or_else(PoisonError::into_inner) = Some(transfers.clone());
    *hub.live_mgr.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&mgr));
    let pump_sync = sync.clone();
    let rekey_hub = Arc::clone(hub);
    let rekey_mgr = Arc::clone(&mgr);
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            if let ManagerEvent::Frame { frame, .. } = &ev {
                if let Some(req) = engine::rekey::parse(&frame.message) {
                    // Switch to the new key, tell the sender, then reconnect with it.
                    let path = rekey_hub.paths.secrets_file();
                    let saved = Secrets::load_or_create(&path).and_then(|mut s| {
                        s.group_key = Some(req.new_gk.clone());
                        s.save(&path)
                    });
                    match saved {
                        Ok(()) => {
                            let _ = rekey_mgr
                                .send(&req.by, engine::rekey::ack(&req), Vec::new())
                                .await;
                            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                            let _ = rekey_hub.commands.send(ToEngine::Reload);
                        }
                        Err(e) => tracing::error!(error = %e, "cannot store the new group key"),
                    }
                    continue;
                }
            }
            // Clipboard and file frames are consumed; the rest needs no action yet.
            if let Some(rest) = pump_sync.handle_event(ev).await {
                let _ = transfers.handle_event(rest).await;
            }
        }
    });
    let ui_notify = ui.clone();
    tokio::spawn(async move {
        while let Some(ev) = transfer_events.recv().await {
            match ev {
                TransferEvent::BatchFinished {
                    ok,
                    failed,
                    incoming,
                    ..
                } => {
                    let text = if incoming {
                        format!("{ok} file diterima, {failed} gagal")
                    } else {
                        format!("{ok} file terkirim, {failed} gagal")
                    };
                    ui_notify.send(ToUi::Notify {
                        title: strings::APP_NAME.into(),
                        text,
                    });
                }
                other => tracing::debug!(event = ?other, "transfer event"),
            }
        }
    });
    let ui_images = ui.clone();
    let sync_for_pull = sync.clone();
    tokio::spawn(async move {
        while let Some(ev) = clip_events.recv().await {
            tracing::debug!(event = ?ev, "clipboard event");
            if let engine::clip::ClipEvent::ImageAvailable { from, id, bytes } = ev {
                // A large image waits on the sender over a slow link; fetch it now and say so.
                ui_images.send(ToUi::Notify {
                    title: strings::APP_NAME.into(),
                    text: format!(
                        "Mengambil gambar besar ({} KB) lewat Bluetooth…",
                        bytes / 1024
                    ),
                });
                let sync = sync_for_pull.clone();
                tokio::spawn(async move {
                    let _ = sync.pull(from, &id).await;
                });
            }
        }
    });
    Some(Running {
        mgr,
        sync,
        beacon,
        _mdns: mdns.ok(),
        _bt: bt_server,
    })
}

fn bt_priority_set(ids: &[String]) -> std::collections::HashSet<DeviceId> {
    ids.iter()
        .filter_map(|h| DeviceId::from_hex(h).ok())
        .collect()
}

/// Run until `ToEngine::Quit`.
pub async fn run(input: HostInput, ui: UiHandle, mut commands: mpsc::UnboundedReceiver<ToEngine>) {
    let HostInput {
        config,
        paths,
        device_id,
        commands: command_tx,
    } = input;
    let hub = Arc::new(Hub {
        ui: ui.clone(),
        status: Mutex::new(Status {
            paused: config.paused,
            ..Status::default()
        }),
        ipc: OnceLock::new(),
        peers: Mutex::new(None),
        transfers: Mutex::new(None),
        live_mgr: Mutex::new(None),
        online: Mutex::new(Vec::new()),
        commands: command_tx,
        config: Mutex::new(config.clone()),
        settings_child: Mutex::new(None),
        paths: paths.clone(),
        device_id,
    });
    match IpcServer::start(&crate::ipc::pipe_name(), handler(Arc::clone(&hub))) {
        Ok(server) => {
            let _ = hub.ipc.set(server);
        }
        Err(e) => tracing::error!(error = %e, "cannot create the IPC pipe"),
    }

    let paused = Arc::new(AtomicBool::new(config.paused));
    let mut running: Option<Running> = None;

    // (Re)load the group key from disk and (re)start the engine accordingly.
    let reload = |hub: &Arc<Hub>| -> Option<GroupKey> {
        let gk = Secrets::load_or_create(&hub.paths.secrets_file())
            .ok()
            .and_then(|s| s.group_key);
        hub.update(|s| {
            s.paired = gk.is_some();
            s.online = 0;
        });
        gk
    };
    if let Some(gk) = reload(&hub) {
        running = start_engine(&hub, &ui, gk, &paused).await;
    }

    while let Some(cmd) = commands.recv().await {
        match cmd {
            ToEngine::Quit => break,
            ToEngine::Reload => {
                if let Some(r) = running.take() {
                    r.stop().await;
                }
                *hub.peers.lock().unwrap_or_else(PoisonError::into_inner) = None;
                *hub.transfers.lock().unwrap_or_else(PoisonError::into_inner) = None;
                *hub.live_mgr.lock().unwrap_or_else(PoisonError::into_inner) = None;
                if let Some(gk) = reload(&hub) {
                    running = start_engine(&hub, &ui, gk, &paused).await;
                    ui.send(ToUi::Notify {
                        title: strings::APP_NAME.into(),
                        text: "Grup siap dipakai".into(),
                    });
                }
            }
            ToEngine::OpenDownloads => {
                let dir = crate::target_dir::resolve(&hub.download_dir()).dir;
                open_folder(&dir);
            }
            ToEngine::LocalClip(clip) => {
                let allowed = match &clip {
                    Clip::Text(_) => hub.cfg().sync_text,
                    Clip::Image { .. } => hub.cfg().sync_images,
                };
                if let (Some(r), true, false) = (&running, allowed, paused.load(Ordering::Relaxed))
                {
                    if let Err(e) = r.sync.submit_local(clip).await {
                        tracing::warn!(error = %e, "clipboard item not sent");
                    }
                }
            }
            ToEngine::SetPaused(p) => {
                paused.store(p, Ordering::Relaxed);
                hub.update(|s| s.paused = p);
                let mut cfg = hub.cfg();
                cfg.paused = p;
                if let Err(e) = hub.store_config(cfg) {
                    tracing::warn!(error = %e, "cannot save config");
                }
            }
            ToEngine::ShowSettings | ToEngine::Pair => hub.show_settings(),
            ToEngine::RadialDrop { paths, to } => {
                let to = to.map_or_else(|| "all".to_owned(), |id| id.to_hex());
                if let Err(e) = hub.send_paths(&paths, &to) {
                    tracing::warn!(error = %e, "radial send failed");
                    hub.ui.send(ToUi::Notify {
                        title: strings::APP_NAME.into(),
                        text: "Tidak ada perangkat yang online".into(),
                    });
                }
            }
            ToEngine::SendFiles => {
                // The tray asked for "Kirim file...": pick files, then send to every online device.
                let hub = Arc::clone(&hub);
                tokio::task::spawn_blocking(move || {
                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                        if let Err(e) = hub.send_paths(&files, "all") {
                            tracing::warn!(error = %e, "tray send failed");
                            hub.ui.send(ToUi::Notify {
                                title: strings::APP_NAME.into(),
                                text: "Tidak ada perangkat yang online".into(),
                            });
                        }
                    }
                });
            }
        }
    }
    if let Some(r) = running {
        r.beacon.announce_now();
        r.stop().await;
    }
}
