//! Best-effort IP geolocation (city/country) via the free `ipwho.is` API, with a small
//! in-memory cache so repeated hops (e.g. your own gateway showing up across many traceroute
//! runs) don't re-query. Every failure mode — private/local address, network error, malformed
//! response — just yields `None`; geolocation is a nice-to-have annotation, never a blocker.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct GeoInfo {
    pub city: Option<String>,
    pub country: Option<String>,
}

#[derive(Deserialize)]
struct ApiResponse {
    success: bool,
    city: Option<String>,
    country: Option<String>,
}

fn cache() -> &'static Mutex<HashMap<IpAddr, Option<GeoInfo>>> {
    static CACHE: LazyLock<Mutex<HashMap<IpAddr, Option<GeoInfo>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    &CACHE
}

/// `false` for loopback/private/link-local/unspecified addresses — the vast majority of near
/// hops (your own router, ISP CGNAT, etc.) — which no public geo-IP service can meaningfully
/// place on a map, so it's not worth the network round trip.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation())
        }
        IpAddr::V6(v6) => {
            let is_unique_local = (v6.segments()[0] & 0xfe00) == 0xfc00;
            let is_link_local = (v6.segments()[0] & 0xffc0) == 0xfe80;
            !(v6.is_loopback() || v6.is_unspecified() || is_unique_local || is_link_local)
        }
    }
}

pub async fn lookup(ip: IpAddr) -> Option<GeoInfo> {
    if !is_public(ip) {
        return None;
    }
    if let Some(hit) = cache().lock().get(&ip).cloned() {
        return hit;
    }
    let result = fetch(ip).await;
    cache().lock().insert(ip, result.clone());
    result
}

async fn fetch(ip: IpAddr) -> Option<GeoInfo> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().ok()?;
    let url = format!("https://ipwho.is/{ip}");
    let body: ApiResponse = client.get(&url).send().await.ok()?.json().await.ok()?;
    if !body.success || (body.city.is_none() && body.country.is_none()) {
        return None;
    }
    Some(GeoInfo { city: body.city, country: body.country })
}
