//! TransClip Windows daemon.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod autostart;
mod bridge;
mod cli;
mod clipboard;
mod config;
mod engine_host;
mod ipc;
mod logging;
mod paths;
mod secrets;
mod single_instance;
mod strings;
mod tray;
mod ui_thread;

use std::time::Duration;

use anyhow::Result;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

fn message_box(text: &str) {
    // SAFETY: both strings are valid wide strings that live for the duration of the call.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from("TransClip"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        tracing::error!(panic = %info, "daemon panicked");
        message_box("TransClip mengalami kesalahan fatal dan akan ditutup. Lihat berkas log.");
        std::process::exit(101);
    }));
}

/// Send one request to the daemon that is already running. Returns the process exit code.
fn ask_running_daemon(method: &str, params: serde_json::Value) -> i32 {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return 1;
    };
    match rt.block_on(ipc::client::one_shot(&ipc::pipe_name(), method, params)) {
        Ok(_) => 0,
        Err(e) => {
            tracing::warn!(error = %e, method, "the running daemon did not accept the request");
            1
        }
    }
}

fn run() -> Result<i32> {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            message_box(&e);
            return Ok(2);
        }
    };
    let paths = paths::Paths::discover()?;
    paths.ensure()?;
    let mut cfg = config::Config::load(&paths.config_file());
    if let Some(level) = &cli.log_level {
        cfg.log_level.clone_from(level);
    }
    let _log = logging::init(&paths.log_dir(), &cfg.log_level)?;
    install_panic_hook();

    match &cli.mode {
        cli::Mode::Daemon => {}
        cli::Mode::Quit => return Ok(ask_running_daemon("quit", serde_json::Value::Null)),
        cli::Mode::Pair(uri) => {
            return Ok(ask_running_daemon(
                "join_group",
                serde_json::json!({ "uri": uri }),
            ));
        }
        cli::Mode::Settings => {
            // The settings window arrives with task P4-01; until then ask the daemon.
            return Ok(ask_running_daemon("show_settings", serde_json::Value::Null));
        }
    }
    let _guard = match single_instance::acquire()? {
        single_instance::Instance::First(g) => g,
        single_instance::Instance::Already => {
            tracing::info!("another instance is already running, asking it to show settings");
            return Ok(ask_running_daemon("show_settings", serde_json::Value::Null));
        }
    };
    let secrets = secrets::Secrets::load_or_create(&paths.secrets_file())?;
    if !paths.config_file().exists() {
        cfg.save(&paths.config_file())?;
    }
    // Under a data-dir override (tests, portable use) the real Run key is left alone.
    if std::env::var_os(paths::DATA_DIR_ENV).is_none() {
        if let Err(e) = autostart::apply(cfg.autostart) {
            tracing::warn!(error = %e, "cannot update the autostart entry");
        }
    }
    tracing::debug!(
        peers = %paths.peers_file().display(),
        receive_dir = ?paths::default_receive_dir().ok(),
        "data locations"
    );
    tracing::info!(
        device = %secrets.device_id.to_hex().chars().take(8).collect::<String>(),
        paired = secrets.group_key.is_some(),
        "daemon started"
    );

    let status = bridge::Status {
        paired: secrets.group_key.is_some(),
        paused: cfg.paused,
        ..bridge::Status::default()
    };
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let quit_tx = cmd_tx.clone();
    let input = engine_host::HostInput {
        config: cfg,
        paths,
        device_id: secrets.device_id,
        commands: quit_tx.clone(),
    };
    let hold = cli.hold_secs;
    let mut engine_thread = None;
    ui_thread::run(cmd_tx, status, |ui| {
        if let Some(secs) = hold {
            // Debug aid: close the daemon after a while.
            let ui = ui.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(secs));
                ui.send(bridge::ToUi::Quit);
            });
        }
        engine_thread =
            Some(std::thread::spawn(
                move || match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt.block_on(engine_host::run(input, ui, cmd_rx)),
                    Err(e) => tracing::error!(error = %e, "cannot start the async runtime"),
                },
            ));
    })?;
    // The window is gone: make sure the engine stops, then wait for it.
    let _ = quit_tx.send(bridge::ToEngine::Quit);
    if let Some(t) = engine_thread {
        let _ = t.join();
    }
    tracing::info!("daemon stopped");
    Ok(0)
}

fn main() {
    match run() {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => {
            message_box(&format!("TransClip tidak dapat dimulai: {e:#}"));
            std::process::exit(1);
        }
    }
}
