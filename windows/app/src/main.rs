//! TransClip Windows daemon.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod cli;
mod config;
mod logging;
mod paths;
mod secrets;
mod single_instance;

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

    match cli.mode {
        cli::Mode::Daemon => {}
        other => {
            // Settings, pairing and quit need the IPC channel (task P2-08).
            tracing::warn!(mode = ?other, "mode not available yet");
            return Ok(0);
        }
    }
    let _guard = match single_instance::acquire()? {
        single_instance::Instance::First(g) => g,
        single_instance::Instance::Already => {
            tracing::info!("another instance is already running");
            return Ok(0);
        }
    };
    let secrets = secrets::Secrets::load_or_create(&paths.secrets_file())?;
    if !paths.config_file().exists() {
        cfg.save(&paths.config_file())?;
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
    // The Win32 message loop replaces this wait in task P2-06.
    match cli.hold_secs {
        Some(s) => std::thread::sleep(Duration::from_secs(s)),
        None => loop {
            std::thread::park();
        },
    }
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
