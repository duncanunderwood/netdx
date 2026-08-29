//! Where the event log lives on disk, and CSV export for it.
//!
//! The in-memory log (`AppState.log`, capped at `state::LOG_CAP` entries) is what the TUI/web UI
//! render live. Export is an explicit, on-demand action (not continuous disk-logging) that dumps
//! the current in-memory log — full timestamps included — to a CSV file for a technician to keep
//! or attach to a ticket.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

use crate::state::{now_rfc3339, LogEntry};

/// Per-user application data directory, following each OS's convention — deliberately not the
/// directory the binary happens to run from (which may be a shared `bin/` alongside unrelated
/// tools, e.g. `~/.local/bin` per the installer script), so exported logs land somewhere sane
/// and consistent regardless of where `netdx` was invoked from.
pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("netdx");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/netdx");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("netdx");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local/share/netdx");
        }
    }
    // Last-resort fallback if even $HOME is unset (rare, e.g. some service accounts): relative
    // to the current working directory rather than failing outright.
    PathBuf::from(".netdx")
}

/// `<app_data_dir>/logs` — created on demand by `export_csv`, never eagerly at startup.
pub fn logs_dir() -> PathBuf {
    app_data_dir().join("logs")
}

/// Writes `entries` as CSV (`timestamp,message`, one row per entry, header included) to a fresh
/// timestamped file under `logs_dir()`, creating the directory if needed. Returns the path
/// written on success.
pub fn export_csv(entries: &VecDeque<LogEntry>) -> Result<PathBuf, String> {
    let dir = logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;

    // e.g. "2026-08-30T14:03:41Z" -> "20260830-140341", safe as a filename on every platform.
    let stamp = now_rfc3339().replace(['-', ':'], "").replace('T', "-").trim_end_matches('Z').to_string();
    let path = dir.join(format!("netdx-log-{stamp}.csv"));

    let mut file = std::fs::File::create(&path).map_err(|e| format!("couldn't create {}: {e}", path.display()))?;
    writeln!(file, "timestamp,message").map_err(|e| e.to_string())?;
    for entry in entries {
        writeln!(file, "{},{}", entry.ts, csv_escape(&entry.message)).map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(path)
}

/// RFC 4180 field escaping: quote (and double up embedded quotes) whenever a field contains a
/// comma, quote, or newline — log messages routinely contain all three (e.g. "speed test error:
/// server returned HTTP 429, retrying").
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
