//! File logging for Diagnostics: tracing writes to stderr (dev) and to a
//! daily-rotated file under ~/Library/Logs/Speakly, the conventional macOS
//! location so users can also find it in Console.app.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const FILE_PREFIX: &str = "speakly.log";

/// Keeps the non-blocking writer flushing for the app's lifetime.
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn log_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Logs/Speakly")
}

pub fn init() {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let appender = tracing_appender::rolling::daily(&dir, FILE_PREFIX);
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let _ = GUARD.set(guard);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .init();
}

/// Newest rotated log file, if any exist yet.
pub fn current_log_file() -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(log_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(FILE_PREFIX))
        })
        .collect();
    files.sort();
    files.pop()
}

/// Last `lines` lines of the newest log file. Daily files stay small, so a
/// full read is fine.
pub fn tail(lines: usize) -> Result<String, String> {
    let Some(path) = current_log_file() else {
        return Ok(String::new());
    };
    let content = std::fs::read_to_string(&path).map_err(|e| format!("read log: {e}"))?;
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].join("\n"))
}
