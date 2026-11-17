//! The tokio side: owns the engine and answers UI commands.

use std::sync::{Arc, Mutex, PoisonError};

use engine::discovery::beacon::{BeaconConfig, BeaconService};
use engine::discovery::mdns::{MdnsConfig, MdnsService};
use engine::link::LocalInfo;
use engine::peers::{ManagerConfig, PeerManager, PeerState};
use proto::beacon::Platform;
use proto::group::GroupKey;
use proto::ids::DeviceId;
use tokio::sync::mpsc;

use crate::bridge::{Status, ToEngine, ToUi, UiHandle};
use crate::config::Config;
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
}

fn open_folder(path: &std::path::Path) {
    // The shell opens the folder in Explorer; failure is not actionable for the user.
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

/// Change the shared status and tell the UI.
fn publish(ui: &UiHandle, shared: &Mutex<Status>, change: impl FnOnce(&mut Status)) {
    let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
    change(&mut s);
    ui.send(ToUi::Status(*s));
}

/// Run until `ToEngine::Quit`.
pub async fn run(input: HostInput, ui: UiHandle, mut commands: mpsc::UnboundedReceiver<ToEngine>) {
    let HostInput {
        config,
        paths,
        device_id,
        group_key,
    } = input;
    let shared = Arc::new(Mutex::new(Status {
        paired: group_key.is_some(),
        paused: config.paused,
        ..Status::default()
    }));
    publish(&ui, &shared, |_| {});

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
                        let ui2 = ui.clone();
                        let shared2 = Arc::clone(&shared);
                        tokio::spawn(async move {
                            while watch.changed().await.is_ok() {
                                let online = watch
                                    .borrow()
                                    .iter()
                                    .filter(|p| p.state == PeerState::Online)
                                    .count();
                                publish(&ui2, &shared2, |s| s.online = online);
                            }
                        });
                        // Events are consumed by clipboard sync in task P2-07.
                        tokio::spawn(async move { while events.recv().await.is_some() {} });
                        started = Some((mgr, b, mdns.ok()));
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "cannot start discovery");
                        publish(&ui, &shared, |s| s.error = true);
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "cannot listen on the TCP port");
                publish(&ui, &shared, |s| s.error = true);
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
            ToEngine::SetPaused(p) => {
                publish(&ui, &shared, |s| s.paused = p);
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
