//! Rolling file logging. Never log clipboard content, file names above `debug`, or keys.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Number of daily log files kept.
const KEEP_FILES: usize = 14;

/// Start logging to `<dir>\transclip.YYYY-MM-DD.log`. Keep the guard alive until exit.
pub fn init(dir: &Path, level: &str) -> Result<WorkerGuard> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("transclip")
        .filename_suffix("log")
        .max_log_files(KEEP_FILES)
        .build(dir)
        .context("cannot open log file")?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let level = LevelFilter::from_str(level).unwrap_or(LevelFilter::INFO);
    // Our own crates log at the chosen level; dependencies (mDNS in particular) only warn.
    let filter = Targets::new()
        .with_target("transclip", level)
        .with_target("engine", level)
        .with_target("proto", level)
        .with_default(LevelFilter::WARN);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false),
        )
        .with(filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("logger already set: {e}"))?;
    Ok(guard)
}
