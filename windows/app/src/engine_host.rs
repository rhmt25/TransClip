//! The tokio side: owns the engine, serves the IPC pipe and answers UI commands.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use engine::clip::{Clip, ClipConfig, ClipboardSync};
use engine::discovery::beacon::{BeaconConfig, BeaconService};
use engine::discovery::mdns::{MdnsConfig, MdnsService};
use engine::link::LocalInfo;
use engine::peers::{ManagerConfig, PeerManager, PeerState, PeerStatus};
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
    online: Mutex<Vec<DeviceId>>,
    commands: mpsc::UnboundedSender<ToEngine>,
    config: Config,
    /// Configured receive folder (`""` = default); changeable at runtime.
    download_dir: Mutex<String>,
    paths: Paths,
    device_id: DeviceId,
}

impl Hub {
    fn download_dir(&self) -> String {
        self.download_dir
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
        let port = self.config.port;
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
                "get_config" => serde_json::to_value(&hub.config).map_err(|e| e.to_string()),
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
                    let mut cfg = Config::load(&hub.paths.config_file());
                    cfg.download_dir.clone_from(&dir);
                    cfg.save(&hub.paths.config_file())
                        .map_err(|e| e.to_string())?;
                    *hub.download_dir
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = dir;
                    let _ = hub.commands.send(ToEngine::Reload);
                    Ok(json!({ "dir": crate::target_dir::resolve(&hub.download_dir()).dir }))
                }
                "show_settings" => {
                    let _ = hub.commands.send(ToEngine::ShowSettings);
                    Ok(Value::Null)
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
                    let tm = hub
                        .transfers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone()
                        .ok_or("not in a group yet")?;
                    let targets: Vec<DeviceId> = if to == "all" {
                        hub.online
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .clone()
                    } else {
                        vec![DeviceId::from_hex(to).map_err(|_| "bad device id")?]
                    };
                    if targets.is_empty() {
                        return Err("no device is online".into());
                    }
                    let batches = scan::batch_from_paths(&paths).map_err(|e| e.to_string())?;
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
    let config = &hub.config;
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
    let (mgr, mut events) =
        match PeerManager::start(ManagerConfig::new(local, gk, listen), seen_rx).await {
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

    let mgr = Arc::new(mgr);
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
            if let Some(ipc) = hub2.ipc.get() {
                ipc.publish(Event {
                    event: "peers_changed".into(),
                    data: hub2.peers_json(),
                });
            }
        }
    });
    let sink = Arc::new(WindowsSink::new(ui.hwnd(), Arc::clone(paused)));
    let (sync, mut clip_events) =
        ClipboardSync::new(Arc::clone(&mgr), sink, device_id, ClipConfig::default());
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
    let pump_sync = sync.clone();
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
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
    tokio::spawn(async move {
        while let Some(ev) = clip_events.recv().await {
            tracing::debug!(event = ?ev, "clipboard event");
        }
    });
    Some(Running {
        mgr,
        sync,
        beacon,
        _mdns: mdns.ok(),
    })
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
        online: Mutex::new(Vec::new()),
        commands: command_tx,
        config: config.clone(),
        download_dir: Mutex::new(config.download_dir.clone()),
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
                    Clip::Text(_) => config.sync_text,
                    Clip::Image { .. } => config.sync_images,
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
                let mut cfg = config.clone();
                cfg.paused = p;
                if let Err(e) = cfg.save(&paths.config_file()) {
                    tracing::warn!(error = %e, "cannot save config");
                }
            }
            ToEngine::ShowSettings | ToEngine::Pair | ToEngine::SendFiles => {
                tracing::info!(?cmd, "UI request (handled in a later task)");
            }
        }
    }
    if let Some(r) = running {
        r.beacon.announce_now();
        r.stop().await;
    }
}
