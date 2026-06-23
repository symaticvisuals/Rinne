//! Logging setup. Rinne logs to a file inside `.rinne/`, never to stdout/stderr
//! where it would pollute the TUI (`CONTEXT.md` §14).

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

/// Initialize file-based logging into `<blackboard>/logs/`.
///
/// Returns a [`WorkerGuard`] that must be held for the lifetime of the process
/// so the non-blocking writer flushes on shutdown.
///
/// `verbosity` maps repeated `-v` flags onto log levels (0 = warn, 1 = info,
/// 2 = debug, 3+ = trace). `RUST_LOG` overrides it when set.
pub fn init(blackboard_dir: &Path, verbosity: u8) -> WorkerGuard {
    let log_dir = blackboard_dir.join("logs");
    // Best-effort: if we can't make the dir, fall back to the current dir so
    // logging never aborts startup.
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "rinne.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let level = match verbosity {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(non_blocking),
        )
        .init();

    guard
}
