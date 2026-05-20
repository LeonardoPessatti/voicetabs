use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, time::SystemTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Hold this guard for the lifetime of the application so the background
/// log-writing thread is not dropped.
pub struct LogGuard {
    _file_guard: WorkerGuard,
}

pub fn init(log_dir: PathBuf) -> anyhow::Result<LogGuard> {
    std::fs::create_dir_all(&log_dir)?;
    let file_appender =
        tracing_appender::rolling::daily(&log_dir, "voicetabs.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_env("VOICETABS_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_timer(SystemTime)
        .with_target(true);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_timer(SystemTime)
        .with_target(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("logger already initialized: {e}"))?;

    tracing::info!("logging initialized; dir = {}", log_dir.display());

    Ok(LogGuard { _file_guard: file_guard })
}
