//! Best-effort, privacy-conscious usage analytics sent to Supabase. Fire-and-forget: never
//! blocks the caller, never surfaces an error to the user — a network diagnostic tool is
//! routinely run on exactly the kind of broken network that would make this fail, and that must
//! never itself become a problem netdx reports. No personally-identifying data leaves the
//! machine: no hostnames, no IP addresses, no traceroute/telnet targets — only an event name, a
//! coarse numeric payload (e.g. measured Mbps), and OS/arch/app version.
//!
//! Disabled entirely with `--no-analytics` or `NETDX_NO_ANALYTICS=1` (see `set_enabled`); see
//! `README.md` for the disclosure and the Supabase table this posts to.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

const SUPABASE_URL: &str = "https://kecqjwaclmuafniwegme.supabase.co";
const SUPABASE_KEY: &str = "sb_publishable_Gf0j6BiHCXriOW4GMmdTxw_Y3EuBjhv";
const TABLE: &str = "netdx_events";

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Called once at startup from `main`, before any `track` call, based on `--no-analytics` /
/// `NETDX_NO_ANALYTICS`.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Posts one event to Supabase in the background. Every failure mode (disabled, no network, DNS
/// failure, table not yet provisioned, rate limiting) is swallowed identically — silently — by
/// design.
pub fn track(event_type: &'static str, payload: Value) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    tokio::spawn(async move {
        let body = serde_json::json!({
            "event_type": event_type,
            "app_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "payload": payload,
        });
        let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(5)).build() else {
            return;
        };
        let _ = client
            .post(format!("{SUPABASE_URL}/rest/v1/{TABLE}"))
            .header("apikey", SUPABASE_KEY)
            .header("Authorization", format!("Bearer {SUPABASE_KEY}"))
            .header("Content-Type", "application/json")
            .header("Prefer", "return=minimal")
            .json(&body)
            .send()
            .await;
    });
}
