//! The tokio side: owns the engine, serves the IPC pipe and answers UI commands.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use engine::clip::{Clip, ClipConfig, ClipboardSync};
use engine::discovery::beacon::{BeaconConfig, BeaconService};
use engine::discovery::mdns::{MdnsConfig, MdnsService};
use engine::link::LocalInfo;
use engine::peers::{ManagerConfig, PeerManager, PeerState, PeerStatus};
use proto::beacon::Platform;
use proto::group::GroupKey;
use proto::ids::DeviceId;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use crate::bridge::{Status, ToEngine, ToUi, UiHandle};
use crate::clipboard::write::WindowsSink;
use crate::config::Config;
use crate::ipc::protocol::Event;
use crate::ipc::server::{Handler, IpcServer};
use crate::paths::Paths;
use crate::strings;

/// Everything the engine needs from the daemon's start-up.
pub struct HostInput {
    /// User settings.
    pub config: Config,
    /// Data folders.
    pub paths: Paths,
    /// This device.
    pub device_id: DeviceId,
    /// Group key, when paired.
    pub group_key: Option<GroupKey>,
    /// Sender for commands to this host (used by IPC requests).
    pub commands: mpsc::UnboundedSender<ToEngine>,
}

fn open_folder(path: &Path) {
    // The shell opens the folder in Explorer; failure is not actionable for the user.
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

/// State shared between the command loop, status watchers and IPC handlers.
struct Hub {
    ui: UiHandle,
    status: Mutex<Status>,
    ipc: OnceLock<IpcServer>,
    peers: OnceLock<watch::Receiver<Vec<PeerStatus>>>,
    commands: mpsc::UnboundedSender<ToEngine>,
    config: Config,
    paths: Paths,
}

impl Hub {
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
            .get()
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
                "show_settings" => {
                    let _ = hub.commands.send(ToEngine::ShowSettings);
                    Ok(Value::Null)
                }
                "open_log_dir" => {
                    open_folder(&hub.paths.log_dir());
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

/// Run until `ToEngine::Quit`.
pub async fn run(input: HostInput, ui: UiHandle, mut commands: mpsc::UnboundedReceiver<ToEngine>) {
    let HostInput {
        config,
        paths,
        device_id,
        group_key,
        commands: command_tx,
    } = input;
    let hub = Arc::new(Hub {
        ui: ui.clone(),
        status: Mutex::new(Status {
            paired: group_key.is_some(),
            paused: config.paused,
            ..Status::default()
        }),
        ipc: OnceLock::new(),
        peers: OnceLock::new(),
        commands: command_tx,
        config: config.clone(),
        paths: paths.clone(),
    });
    hub.update(|_| {});
    match IpcServer::start(&crate::ipc::pipe_name(), handler(Arc::clone(&hub))) {
        Ok(server) => {
            let _ = hub.ipc.set(server);
        }
        Err(e) => tracing::error!(error = %e, "cannot create the IPC pipe"),
    }

    let paused = Arc::new(AtomicBool::new(config.paused));
    let mut sync: Option<ClipboardSync> = None;
    let (seen_tx, seen_rx) = mpsc::channel(64);
    let mut started: Option<(Arc<PeerManager>, BeaconService, Option<MdnsService>)> = None;
    if let Some(gk) = group_key {
        let gid = gk.group_id();
        let local = LocalInfo {
            id: device_id,
            name: config.device_name.clone(),
            platform: "windows".into(),
            app: format!("transclip/{}", env!("CARGO_PKG_VERSION")),
            caps: vec!["clip".into()],
            role: "full".into(),
        };
        let listen = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
        match PeerManager::start(ManagerConfig::new(local, gk, listen), seen_rx).await {
            Ok((mgr, mut events)) => {
                let beacon = BeaconService::start(
                    BeaconConfig::lan(
                        gid,
                        device_id,
                        config.port,
                        Platform::Windows,
                        config.device_name.clone(),
                    ),
                    seen_tx.clone(),
                );
                let mdns = MdnsService::start(
                    &MdnsConfig {
                        gid,
                        device_id,
                        tcp_port: config.port,
                        platform: Platform::Windows,
                        name: config.device_name.clone(),
                    },
                    seen_tx.clone(),
                );
                if let Err(e) = &mdns {
                    tracing::warn!(error = %e, "mDNS unavailable, relying on beacons");
                }
                match beacon {
                    Ok(b) => {
                        let mgr = Arc::new(mgr);
                        let mut watch = mgr.status();
                        let _ = hub.peers.set(mgr.status());
                        let hub2 = Arc::clone(&hub);
                        tokio::spawn(async move {
                            while watch.changed().await.is_ok() {
                                let online = watch
                                    .borrow()
                                    .iter()
                                    .filter(|p| p.state == PeerState::Online)
                                    .count();
                                hub2.update(|s| s.online = online);
                                if let Some(ipc) = hub2.ipc.get() {
                                    ipc.publish(Event {
                                        event: "peers_changed".into(),
                                        data: hub2.peers_json(),
                                    });
                                }
                            }
                        });
                        let sink = Arc::new(WindowsSink::new(ui.hwnd(), Arc::clone(&paused)));
                        let (clip_sync, mut clip_events) = ClipboardSync::new(
                            Arc::clone(&mgr),
                            sink,
                            device_id,
                            ClipConfig::default(),
                        );
                        let pump_sync = clip_sync.clone();
                        tokio::spawn(async move {
                            while let Some(ev) = events.recv().await {
                                // Clipboard frames are consumed; other events need no action yet.
                                let _ = pump_sync.handle_event(ev).await;
                            }
                        });
                        tokio::spawn(async move {
                            while let Some(ev) = clip_events.recv().await {
                                tracing::debug!(event = ?ev, "clipboard event");
                            }
                        });
                        sync = Some(clip_sync);
                        started = Some((mgr, b, mdns.ok()));
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "cannot start discovery");
                        hub.update(|s| s.error = true);
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "cannot listen on the TCP port");
                hub.update(|s| s.error = true);
                ui.send(ToUi::Notify {
                    title: strings::APP_NAME.into(),
                    text: format!("Port {} tidak dapat dipakai", config.port),
                });
            }
        }
    }

    while let Some(cmd) = commands.recv().await {
        match cmd {
            ToEngine::Quit => break,
            ToEngine::OpenDownloads => {
                let dir = if config.download_dir.is_empty() {
                    crate::paths::default_receive_dir().ok()
                } else {
                    Some(config.download_dir.clone().into())
                };
                if let Some(dir) = dir {
                    let _ = std::fs::create_dir_all(&dir);
                    open_folder(&dir);
                }
            }
            ToEngine::LocalClip(clip) => {
                let allowed = match &clip {
                    Clip::Text(_) => config.sync_text,
                    Clip::Image { .. } => config.sync_images,
                };
                if let (Some(sync), true, false) = (&sync, allowed, paused.load(Ordering::Relaxed))
                {
                    if let Err(e) = sync.submit_local(clip).await {
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
    if let Some((mgr, _beacon, _mdns)) = started {
        mgr.stop().await;
    }
}
