//! Runs the real executable twice with an isolated data folder.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;
use std::time::{Duration, Instant};

fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_transclip"))
}

#[test]
fn second_instance_exits_and_files_are_created() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = exe()
        .env("TRANSCLIP_DATA_DIR", dir.path())
        .env("TRANSCLIP_NO_WINDOW", "1")
        .args(["--hold-secs", "20"])
        .spawn()
        .unwrap();
    let secrets = dir.path().join("roaming").join("secrets.bin");
    let config = dir.path().join("roaming").join("config.toml");
    let started = Instant::now();
    while !(secrets.exists() && config.exists()) && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        secrets.exists() && config.exists(),
        "first instance did not initialise"
    );

    let begin = Instant::now();
    let status = exe()
        .env("TRANSCLIP_DATA_DIR", dir.path())
        .env("TRANSCLIP_NO_WINDOW", "1")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        begin.elapsed() < Duration::from_secs(5),
        "second instance must exit quickly"
    );
    assert!(
        first.try_wait().unwrap().is_none(),
        "first instance must keep running"
    );

    // The second launch asked the first one to show its settings (IPC), which it logs.
    let log_dir = dir.path().join("local").join("logs");
    let wait_for_log = |needle: &str| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let found = std::fs::read_dir(&log_dir).unwrap().any(|e| {
                std::fs::read_to_string(e.unwrap().path()).is_ok_and(|t| t.contains(needle))
            });
            if found {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    assert!(
        wait_for_log("ShowSettings"),
        "daemon did not receive show_settings"
    );

    // `--quit` stops the running daemon cleanly.
    let quit = exe()
        .env("TRANSCLIP_DATA_DIR", dir.path())
        .env("TRANSCLIP_NO_WINDOW", "1")
        .arg("--quit")
        .status()
        .unwrap();
    assert!(quit.success());
    let start = Instant::now();
    let exit = loop {
        if let Some(status) = first.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "daemon did not quit"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(exit.success());

    let logs: Vec<_> = std::fs::read_dir(dir.path().join("local").join("logs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        logs.iter()
            .any(|n| n.starts_with("transclip.") && n.ends_with(".log")),
        "log files: {logs:?}"
    );
    let raw = std::fs::read(&secrets).unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("device_id"),
        "secrets must not be readable as text"
    );
}
