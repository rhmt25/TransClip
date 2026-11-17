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
    first.kill().unwrap();
    first.wait().unwrap();

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
