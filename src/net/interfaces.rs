use std::time::Duration;

use crate::state::{InterfaceInfo, Ipv4Entry, Ipv6Entry, NetworkOverview};

/// Enumerate local network interfaces (the "ipconfig"/"ifconfig" view). Pure local syscalls,
/// no network I/O, so this is cheap enough to call on every manual refresh.
pub fn snapshot() -> NetworkOverview {
    let default_name = netdev::get_default_interface().ok().map(|i| i.name);

    let interfaces: Vec<InterfaceInfo> = netdev::get_interfaces()
        .into_iter()
        .map(|iface| {
            let is_default = default_name.as_deref() == Some(iface.name.as_str());
            let gateway = iface.gateway.as_ref().and_then(|gw| {
                gw.ipv4
                    .first()
                    .map(|ip| ip.to_string())
                    .or_else(|| gw.ipv6.first().map(|ip| ip.to_string()))
            });

            let display_name = non_empty(iface.friendly_name.as_deref())
                .or_else(|| non_empty(iface.description.as_deref()))
                .map(str::to_string)
                .unwrap_or_else(|| iface.name.clone());
            let system_name = if looks_like_guid(&iface.name) { None } else { Some(iface.name.clone()) };

            InterfaceInfo {
                name: iface.name.clone(),
                friendly_name: iface.friendly_name.clone(),
                display_name,
                system_name,
                if_type: iface.if_type.name(),
                is_up: iface.is_up(),
                is_loopback: iface.is_loopback(),
                is_default,
                mac: iface.mac_addr.map(|m| m.to_string()),
                ipv4: iface
                    .ipv4
                    .iter()
                    .map(|net| Ipv4Entry {
                        addr: net.addr().to_string(),
                        prefix_len: net.prefix_len(),
                    })
                    .collect(),
                ipv6: iface
                    .ipv6
                    .iter()
                    .map(|net| Ipv6Entry {
                        addr: net.addr().to_string(),
                        prefix_len: net.prefix_len(),
                    })
                    .collect(),
                mtu: iface.mtu,
                dns_servers: iface.dns_servers.iter().map(|ip| ip.to_string()).collect(),
                gateway,
                rx_bytes: iface.stats.as_ref().map(|s| s.rx_bytes),
                tx_bytes: iface.stats.as_ref().map(|s| s.tx_bytes),
            }
        })
        .collect();

    // Never surface the raw (GUID, on Windows) system name here — show the same friendly label
    // the matching interface card uses.
    let default_interface = interfaces.iter().find(|i| i.is_default).map(|i| i.display_name.clone());

    NetworkOverview {
        default_interface,
        public_ip: None,
        interfaces,
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// Windows adapter names are GUIDs like `{CB40F214-85E6-4CDE-96B7-5670A433AA8A}` — meaningless
/// to a technician and never worth displaying.
fn looks_like_guid(s: &str) -> bool {
    let t = s.trim_start_matches('{').trim_end_matches('}');
    t.len() == 36
        && t.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Best-effort public IP lookup. Never blocks callers for long: bounded by an overall timeout
/// so an offline/air-gapped machine (which is exactly what a network tech might be diagnosing)
/// doesn't stall startup.
pub async fn lookup_public_ip() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .ok()?;
    let resp = client.get("https://api.ipify.org").send().await.ok()?;
    let text = resp.text().await.ok()?;
    let ip = text.trim();
    if ip.is_empty() || ip.parse::<std::net::IpAddr>().is_err() {
        None
    } else {
        Some(ip.to_string())
    }
}
