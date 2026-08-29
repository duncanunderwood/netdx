//! Ping/jitter/loss, download, and upload measurements against a choice of public speed-test
//! endpoints. ICMP ping is attempted first and falls back transparently to TCP-connect timing
//! when raw sockets aren't available (unprivileged process), so the speed test always works even
//! without root/Administrator.
//!
//! Requests are deliberately conservative: large chunks (fewer, bigger requests) plus a small
//! inter-request gap and exponential-backoff retry on HTTP 429, because unauthenticated public
//! speed-test endpoints (Cloudflare's in particular) rate-limit bursts of small/rapid requests.

use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::StreamExt;
use rand::RngCore;
use reqwest::StatusCode;

const PING_ATTEMPTS: usize = 6;
const DOWNLOAD_TIME_BUDGET: Duration = Duration::from_secs(10);
const UPLOAD_TIME_BUDGET: Duration = Duration::from_secs(10);
const SAMPLE_INTERVAL: Duration = Duration::from_millis(200);
/// Progressive download sizes: small requests first (accurate quickly on slow links, and gets a
/// sample on screen fast) ramping up to large ones (fewer, bigger requests dominate the average
/// on fast links, where a 10MB chunk would round-trip in a fraction of a second and mostly
/// measure request overhead rather than throughput). Cycles through in order, holding at the
/// largest size for the remainder of `DOWNLOAD_TIME_BUDGET` once reached. All four sizes are
/// requested via Cloudflare's `?bytes=N` endpoint, which accepts any size up to its ~80MB
/// practical cap in one shot — kept under that per single request below.
const DOWNLOAD_SIZES_BYTES: [u64; 4] = [10_000_000, 25_000_000, 50_000_000, 100_000_000];
/// Cloudflare's `__down` endpoint 403s above roughly 80MB in a single request (undocumented,
/// found empirically) — any tier larger than this is split into multiple requests instead.
const MAX_SINGLE_REQUEST_BYTES: u64 = 75_000_000;
const UPLOAD_CHUNK_BYTES: usize = 20 * 1024 * 1024;
/// Small proactive gap between requests to the same server, on top of reactive 429 backoff —
/// keeps a very fast connection (which would otherwise fire off a chunk request every few tens
/// of milliseconds) from tripping the limiter in the first place.
const INTER_REQUEST_GAP: Duration = Duration::from_millis(120);
const MAX_RETRIES: u32 = 5;

/// A speed-test backend a user can pick from. Only servers that support both download **and**
/// upload are offered — Hetzner/OVH/Vultr-style providers only publish static download test
/// files with no upload endpoint, so they're deliberately excluded rather than shown with a
/// disabled/partial test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ServerId {
    #[default]
    Cloudflare,
}

impl ServerId {
    pub const ALL: [ServerId; 1] = [ServerId::Cloudflare];

    pub fn id_str(self) -> &'static str {
        match self {
            ServerId::Cloudflare => "cloudflare",
        }
    }

    pub fn from_id_str(s: &str) -> Option<ServerId> {
        Self::ALL.iter().copied().find(|id| id.id_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            ServerId::Cloudflare => "Cloudflare (global)",
        }
    }

    pub fn ping_host(self) -> &'static str {
        match self {
            ServerId::Cloudflare => "speed.cloudflare.com",
        }
    }

    pub fn supports_upload(self) -> bool {
        matches!(self, ServerId::Cloudflare)
    }

    pub fn next(self) -> ServerId {
        let idx = Self::ALL.iter().position(|id| *id == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

}

pub struct PingResult {
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub loss_pct: f64,
}

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

/// Sends a request built fresh by `make` (so it can be retried), retrying with exponential
/// backoff on HTTP 429 up to `MAX_RETRIES` times before giving up.
async fn send_with_retry(make: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    let mut delay = Duration::from_millis(500);
    for attempt in 0..=MAX_RETRIES {
        let resp = make().send().await.map_err(|e| format!("request failed: {e}"))?;
        if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            if attempt == MAX_RETRIES {
                return Err(
                    "rate limited (HTTP 429) after several retries — try a different speed test server".to_string(),
                );
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(8));
            continue;
        }
        if !resp.status().is_success() {
            return Err(format!("server returned HTTP {}", resp.status()));
        }
        return Ok(resp);
    }
    unreachable!("loop always returns before exhausting attempts")
}

pub async fn measure_ping(server: ServerId) -> PingResult {
    let host = server.ping_host();
    let ip = tokio::net::lookup_host((host, 0))
        .await
        .ok()
        .and_then(|mut it| it.next())
        .map(|sa| sa.ip());

    let mut samples: Vec<Option<f64>> = Vec::with_capacity(PING_ATTEMPTS);

    if let Some(ip) = ip {
        let first = tokio::time::timeout(Duration::from_secs(2), surge_ping::ping(ip, &[0u8; 32])).await;
        if let Ok(Ok((_, dur))) = first {
            samples.push(Some(dur.as_secs_f64() * 1000.0));
            for _ in 1..PING_ATTEMPTS {
                match tokio::time::timeout(Duration::from_secs(2), surge_ping::ping(ip, &[0u8; 32])).await {
                    Ok(Ok((_, dur))) => samples.push(Some(dur.as_secs_f64() * 1000.0)),
                    _ => samples.push(None),
                }
            }
        } else {
            // ICMP unusable (no raw-socket privileges, or the very first probe failed outright) —
            // fall back to TCP-connect timing so the speed test still works unprivileged.
            for _ in 0..PING_ATTEMPTS {
                samples.push(tcp_ping_once(host).await);
            }
        }
    } else {
        for _ in 0..PING_ATTEMPTS {
            samples.push(tcp_ping_once(host).await);
        }
    }

    summarize(samples)
}

async fn tcp_ping_once(host: &str) -> Option<f64> {
    let started = Instant::now();
    let res = tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect((host, 443))).await;
    match res {
        Ok(Ok(_stream)) => Some(started.elapsed().as_secs_f64() * 1000.0),
        _ => None,
    }
}

fn summarize(samples: Vec<Option<f64>>) -> PingResult {
    let ok: Vec<f64> = samples.iter().filter_map(|s| *s).collect();
    let loss_pct = if samples.is_empty() {
        100.0
    } else {
        (samples.len() - ok.len()) as f64 / samples.len() as f64 * 100.0
    };
    if ok.is_empty() {
        return PingResult { ping_ms: None, jitter_ms: None, loss_pct };
    }
    let avg = ok.iter().sum::<f64>() / ok.len() as f64;
    let jitter_ms = if ok.len() > 1 {
        let diffs: Vec<f64> = ok.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        Some(diffs.iter().sum::<f64>() / diffs.len() as f64)
    } else {
        None
    };
    PingResult { ping_ms: Some(avg), jitter_ms, loss_pct }
}

/// Flattens `DOWNLOAD_SIZES_BYTES` into a sequence of individual HTTP request sizes, splitting
/// any tier above `MAX_SINGLE_REQUEST_BYTES` into multiple back-to-back requests (the byte
/// stream is summed continuously across request boundaries for sampling purposes, so this is
/// transparent to the caller) — reaches the nominal 100MB tier's total transfer volume without
/// tripping Cloudflare's undocumented single-request cap.
fn download_request_sizes() -> Vec<u64> {
    let mut sizes = Vec::new();
    for &tier in &DOWNLOAD_SIZES_BYTES {
        let mut remaining = tier;
        while remaining > 0 {
            let chunk = remaining.min(MAX_SINGLE_REQUEST_BYTES);
            sizes.push(chunk);
            remaining -= chunk;
        }
    }
    sizes
}

/// Streams downloads for up to `DOWNLOAD_TIME_BUDGET`, calling `on_sample` with an instantaneous
/// Mbps reading roughly every `SAMPLE_INTERVAL`. Returns the overall average Mbps.
pub async fn measure_download(server: ServerId, mut on_sample: impl FnMut(f64)) -> Result<f64, String> {
    let _ = server; // kept for API symmetry with measure_ping; only one server exists today
    let client = build_client()?;
    let sizes = download_request_sizes();

    let start = Instant::now();
    let mut total: u64 = 0;
    let mut last_sample_at = start;
    let mut last_sample_bytes: u64 = 0;
    let mut first_request = true;
    let mut idx = 0usize;

    while start.elapsed() < DOWNLOAD_TIME_BUDGET {
        if !first_request {
            tokio::time::sleep(INTER_REQUEST_GAP).await;
        }
        first_request = false;

        // Once every tier has been requested once, keep re-requesting the largest safe chunk
        // size for the rest of the time budget — needed so a fast connection keeps being
        // measured at a size large enough that request overhead doesn't dominate the reading.
        let bytes = sizes.get(idx).copied().unwrap_or(MAX_SINGLE_REQUEST_BYTES);
        idx += 1;
        let url = format!("https://speed.cloudflare.com/__down?bytes={bytes}");

        let resp = send_with_retry(|| client.get(&url)).await?;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("download stream error: {e}"))?;
            total += chunk.len() as u64;

            if start.elapsed() >= DOWNLOAD_TIME_BUDGET {
                break;
            }
            let since_sample = last_sample_at.elapsed();
            if since_sample >= SAMPLE_INTERVAL {
                let delta_bytes = total - last_sample_bytes;
                let mbps = (delta_bytes as f64 * 8.0) / since_sample.as_secs_f64() / 1_000_000.0;
                on_sample(mbps);
                last_sample_at = Instant::now();
                last_sample_bytes = total;
            }
        }
    }

    if total == 0 {
        return Err("no data received from download server".to_string());
    }
    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    Ok((total as f64 * 8.0) / elapsed / 1_000_000.0)
}

/// Repeatedly uploads fixed-size random chunks to Cloudflare (the only server here that supports
/// it) for up to `UPLOAD_TIME_BUDGET`, calling `on_sample` with each chunk's Mbps. Returns the
/// overall average Mbps. Callers must check `ServerId::supports_upload` first.
pub async fn measure_upload(mut on_sample: impl FnMut(f64)) -> Result<f64, String> {
    let client = build_client()?;

    let mut raw = vec![0u8; UPLOAD_CHUNK_BYTES];
    rand::thread_rng().fill_bytes(&mut raw);
    let chunk = Bytes::from(raw);

    let start = Instant::now();
    let mut total: u64 = 0;
    let mut first_request = true;

    while start.elapsed() < UPLOAD_TIME_BUDGET {
        if !first_request {
            tokio::time::sleep(INTER_REQUEST_GAP).await;
        }
        first_request = false;

        let req_start = Instant::now();
        let body = chunk.clone();
        let resp = send_with_retry(|| client.post("https://speed.cloudflare.com/__up").body(body.clone())).await?;
        let _ = resp.bytes().await;

        let elapsed = req_start.elapsed().as_secs_f64().max(0.001);
        total += chunk.len() as u64;
        let mbps = (chunk.len() as f64 * 8.0) / elapsed / 1_000_000.0;
        on_sample(mbps);
    }

    if total == 0 {
        return Err("no data uploaded".to_string());
    }
    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    Ok((total as f64 * 8.0) / elapsed / 1_000_000.0)
}
