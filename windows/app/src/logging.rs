//! Rolling file logging. Never log clipboard content, file names above `debug`, or keys.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::LevelFilter;

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
    let filter = LevelFilter::from_str(level).unwrap_or(LevelFilter::INFO);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("logger already set: {e}"))?;
    Ok(guard)
}
