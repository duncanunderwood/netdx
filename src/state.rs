use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

pub const LOG_CAP: usize = 200;
pub const TELNET_BUFFER_CAP: usize = 16_000;

#[derive(Serialize, Clone, Debug, Default)]
pub struct Ipv4Entry {
    pub addr: String,
    pub prefix_len: u8,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Ipv6Entry {
    pub addr: String,
    pub prefix_len: u8,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct InterfaceInfo {
    /// Raw OS-level identifier (a GUID on Windows, `eth0`/`en0`/etc. elsewhere). Rarely
    /// meaningful to a technician on its own — prefer `display_name` in UI.
    pub name: String,
    pub friendly_name: Option<String>,
    /// Human-friendly label to show as the primary heading: `friendly_name`, falling back to
    /// `name` when the platform doesn't provide one (already sensible on Linux/macOS, e.g.
    /// `eth0`). Never a raw Windows adapter GUID.
    pub display_name: String,
    /// `name`, but only when it's actually worth showing a technician (e.g. `eth0`) — `None`
    /// when it's a GUID-shaped Windows adapter id, so UIs never need their own GUID filter.
    pub system_name: Option<String>,
    pub if_type: String,
    pub is_up: bool,
    pub is_loopback: bool,
    pub is_default: bool,
    pub mac: Option<String>,
    pub ipv4: Vec<Ipv4Entry>,
    pub ipv6: Vec<Ipv6Entry>,
    pub mtu: Option<u32>,
    pub dns_servers: Vec<String>,
    pub gateway: Option<String>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct NetworkOverview {
    pub default_interface: Option<String>,
    pub public_ip: Option<String>,
    pub interfaces: Vec<InterfaceInfo>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct HopInfo {
    pub ttl: u8,
    pub addr: Option<String>,
    /// Reverse-DNS name for `addr`, filled in shortly after the hop appears (best-effort).
    pub hostname: Option<String>,
    /// Best-effort geolocation of `addr`, filled in shortly after the hop appears. `None` for
    /// private/local addresses (most early hops) or when the lookup fails.
    pub city: Option<String>,
    pub country: Option<String>,
    pub rtt_ms: Option<f64>,
    pub timeout: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct TracerouteState {
    pub target: String,
    pub resolved_ip: Option<String>,
    pub running: bool,
    pub done: bool,
    pub max_hops: u8,
    pub hops: Vec<HopInfo>,
    pub error: Option<String>,
    /// Bumped on every `TracerouteStart`; lets a late-arriving background hostname/geo lookup
    /// from a previous run recognize it's stale and avoid patching the wrong run's hop list.
    #[serde(skip)]
    pub run_id: u64,
}


#[derive(Serialize, Clone, Debug, Default)]
pub struct TelnetState {
    pub connected: bool,
    pub connecting: bool,
    pub host: String,
    pub port: u16,
    pub buffer: String,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SpeedtestServerInfo {
    pub id: String,
    pub label: String,
    pub supports_upload: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct SpeedtestState {
    pub running: bool,
    pub stage: String,
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_pct: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub download_samples: Vec<f64>,
    pub upload_samples: Vec<f64>,
    pub server: String,
    pub available_servers: Vec<SpeedtestServerInfo>,
    pub selected_server: String,
    pub error: Option<String>,
}

impl Default for SpeedtestState {
    fn default() -> Self {
        let available_servers = crate::net::speedtest::ServerId::ALL
            .iter()
            .map(|id| SpeedtestServerInfo {
                id: id.id_str().to_string(),
                label: id.label().to_string(),
                supports_upload: id.supports_upload(),
            })
            .collect();
        Self {
            running: false,
            stage: "idle".to_string(),
            ping_ms: None,
            jitter_ms: None,
            packet_loss_pct: None,
            download_mbps: None,
            upload_mbps: None,
            download_samples: Vec::new(),
            upload_samples: Vec::new(),
            server: String::new(),
            available_servers,
            selected_server: crate::net::speedtest::ServerId::default().id_str().to_string(),
            error: None,
        }
    }
}

impl Default for TracerouteState {
    fn default() -> Self {
        Self {
            target: String::new(),
            resolved_ip: None,
            running: false,
            done: false,
            max_hops: 30,
            hops: Vec::new(),
            error: None,
            run_id: 0,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct LogEntry {
    /// Full RFC3339-ish UTC timestamp (`2026-08-30T14:03:41Z`), so a CSV export is meaningful
    /// across midnight/day boundaries — the old HH:MM:SS-only clock silently lost the date.
    pub ts: String,
    pub message: String,
    /// Set only on the "event log exported" entry itself: just the CSV's filename (never a
    /// full local path — the web client is often a phone/laptop on another machine, so it's
    /// meaningless there anyway, and it's needless disclosure of local filesystem layout).
    /// The web UI turns this into a `/exports/<name>?token=...` download link; the TUI, which
    /// always runs on the same machine that wrote the file, resolves it back to a full local
    /// path and overlays a real OSC 8 terminal hyperlink over that row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_filename: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct UpdateState {
    pub current_version: String,
    pub checking: bool,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub installing: bool,
    pub error: Option<String>,
    pub release_url: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct AppState {
    pub network: NetworkOverview,
    pub traceroute: TracerouteState,
    pub telnet: TelnetState,
    pub speedtest: SpeedtestState,
    pub log: VecDeque<LogEntry>,
    pub update: UpdateState,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            network: NetworkOverview::default(),
            traceroute: TracerouteState::default(),
            telnet: TelnetState::default(),
            speedtest: SpeedtestState::default(),
            log: VecDeque::new(),
            update: UpdateState { current_version: env!("CARGO_PKG_VERSION").to_string(), ..Default::default() },
        }
    }
}

impl AppState {
    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push_back(LogEntry { ts: now_local_iso(), message: msg.into(), export_filename: None });
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
    }

    /// Same as `push_log`, but tags the entry with the CSV filename it announces, so the UI can
    /// render that specific line as a clickable link to open/download the file.
    pub fn push_log_with_export(&mut self, msg: impl Into<String>, export_filename: String) {
        self.log.push_back(LogEntry { ts: now_local_iso(), message: msg.into(), export_filename: Some(export_filename) });
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
    }
}

/// The local computer's current date/time (not UTC — a technician reading the log wants to
/// match it against what their own clock/watch says), as `YYYY-MM-DDTHH:MM:SS±HH:MM`. Getting
/// this right (DST, non-whole-hour zones, the OS's actual configured zone) needs a real
/// timezone database, which is why this uses `chrono` rather than hand-rolled calendar math —
/// unlike a fixed always-UTC clock, "local time" isn't something you can derive from
/// `SystemTime` alone.
pub(crate) fn now_local_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// Commands accepted from either the local TUI or a remote web client.
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    RefreshInterfaces,
    TracerouteStart {
        target: String,
        #[serde(default = "default_max_hops")]
        max_hops: u8,
    },
    TracerouteStop,
    TelnetConnect {
        host: String,
        port: u16,
    },
    TelnetSend {
        data: String,
    },
    TelnetDisconnect,
    SpeedtestStart {
        #[serde(default)]
        server: Option<String>,
    },
    SpeedtestStop,
    LogClear,
    LogExport,
    CheckForUpdate,
    InstallUpdate,
}

pub fn default_max_hops() -> u8 {
    30
}

