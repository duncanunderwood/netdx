//! Classic TTL-incrementing ICMP traceroute — the same technique Windows `tracert` and Unix
//! `traceroute` use.
//!
//! This talks to a raw ICMP socket directly instead of going through `surge_ping::Client`/
//! `Pinger`. That matters: `surge_ping` correlates every incoming packet to a waiting `Pinger` by
//! matching the packet's **source address** against the address the `Pinger` was created for
//! (see its `ReplyToken(IpAddr, ...)`). That's correct for plain ping (the reply always comes
//! from the host you pinged), but it's wrong for traceroute: an intermediate router's "TTL
//! exceeded" reply arrives *from the router's address*, not the target's, so `surge_ping` never
//! delivers it anywhere — every intermediate hop reads as a timeout even when the router
//! genuinely replied. Owning the socket ourselves and matching replies by the ICMP
//! identifier/sequence embedded in the packet (which is valid for any replying host, router or
//! target) fixes that and brings netdx to parity with `tracert`/`traceroute`.
//!
//! We still reuse `surge_ping`'s battle-tested `Icmpv4Packet`/`Icmpv6Packet::decode` for parsing
//! — including digging the original identifier/sequence out of a "TTL exceeded" packet's embedded
//! copy of our probe — just not its socket ownership or reply routing.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockRef, Socket, Type};
use surge_ping::{Icmpv4Packet, Icmpv6Packet};
use tokio::net::{lookup_host, UdpSocket};

use crate::state::HopInfo;

const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
const RETRY_TIMEOUT: Duration = Duration::from_millis(1000);
const PAYLOAD: &[u8] = b"netdx traceroute probe";
const ICMPV4_ECHO_REQUEST: u8 = 8;
const ICMPV6_ECHO_REQUEST: u8 = 128;

/// Resolve a hostname or literal IP to a single address, preferring IPv4.
pub async fn resolve_target(target: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        return Ok(ip);
    }
    let mut addrs: Vec<IpAddr> = lookup_host((target, 0))
        .await
        .map_err(|e| format!("could not resolve '{target}': {e}"))?
        .map(|sa| sa.ip())
        .collect();
    if addrs.is_empty() {
        return Err(format!("could not resolve '{target}': no addresses found"));
    }
    addrs.sort_by_key(|ip| !ip.is_ipv4());
    Ok(addrs.remove(0))
}

/// Reverse-DNS lookup for a hop address, best-effort: returns `None` on any failure, timeout,
/// or when the platform just echoes the IP back (no real PTR record).
pub async fn reverse_dns(ip: IpAddr) -> Option<String> {
    let task = tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&ip));
    match tokio::time::timeout(Duration::from_secs(2), task).await {
        Ok(Ok(Ok(name))) if name != ip.to_string() => Some(name),
        _ => None,
    }
}

/// Runs a TTL-incrementing traceroute, invoking `on_hop(hop, reached_destination)` after every
/// probed hop so callers can stream results live. Probes each hop up to twice (a second, faster
/// retry) before giving up on it — but many real routers genuinely never answer TTL-exceeded
/// probes at all (security policy, rate-limiting), so a run of `* * *` lines can still be
/// expected/normal, same as with `tracert`/`traceroute`.
pub async fn run(
    target: IpAddr,
    max_hops: u8,
    mut on_hop: impl FnMut(HopInfo, bool),
) -> Result<(), String> {
    #[cfg(windows)]
    if let IpAddr::V4(v4) = target {
        return crate::net::traceroute_windows::run(v4, max_hops, on_hop).await;
    }

    let socket = open_raw_socket(target).await.map_err(|e| permission_hint(&e))?;
    let ident: u16 = rand::random();
    let target_str = target.to_string();

    for ttl in 1..=max_hops {
        let mut hop = probe_hop(&socket, target, ident, ttl, PROBE_TIMEOUT).await;
        if hop.timeout {
            hop = probe_hop(&socket, target, ident, ttl, RETRY_TIMEOUT).await;
        }

        let reached = hop.addr.as_deref() == Some(target_str.as_str());
        on_hop(hop, reached);

        if reached {
            return Ok(());
        }
    }
    Ok(())
}

async fn open_raw_socket(target: IpAddr) -> io::Result<UdpSocket> {
    let (domain, protocol, bind_addr) = match target {
        IpAddr::V4(_) => (Domain::IPV4, Protocol::ICMPV4, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))),
        IpAddr::V6(_) => (Domain::IPV6, Protocol::ICMPV6, SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))),
    };

    let socket = Socket::new(domain, Type::RAW, Some(protocol))?;
    socket.set_nonblocking(true)?;
    socket.bind(&bind_addr.into())?;

    #[cfg(windows)]
    let std_socket = unsafe {
        use std::os::windows::io::{FromRawSocket, IntoRawSocket};
        std::net::UdpSocket::from_raw_socket(socket.into_raw_socket())
    };
    #[cfg(unix)]
    let std_socket = unsafe {
        use std::os::unix::io::{FromRawFd, IntoRawFd};
        std::net::UdpSocket::from_raw_fd(socket.into_raw_fd())
    };

    UdpSocket::from_std(std_socket)
}

async fn probe_hop(socket: &UdpSocket, target: IpAddr, ident: u16, ttl: u8, timeout: Duration) -> HopInfo {
    let ttl_result = match target {
        IpAddr::V4(_) => SockRef::from(socket).set_ttl_v4(ttl as u32),
        IpAddr::V6(_) => SockRef::from(socket).set_unicast_hops_v6(ttl as u32),
    };
    if ttl_result.is_err() {
        return HopInfo { ttl, timeout: true, ..Default::default() };
    }

    let seq = ttl as u16;
    let packet = build_echo_request(target, ident, seq);
    let sent_at = Instant::now();
    if socket.send_to(&packet, SocketAddr::new(target, 0)).await.is_err() {
        return HopInfo { ttl, timeout: true, ..Default::default() };
    }

    let deadline = tokio::time::Instant::now() + timeout;
    let mut buf = [0u8; 1024];

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return HopInfo { ttl, timeout: true, ..Default::default() };
        }
        let Ok(Ok((n, from))) = tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await else {
            return HopInfo { ttl, timeout: true, ..Default::default() };
        };
        if reply_matches(target, &buf[..n], ident, seq) {
            return HopInfo {
                ttl,
                addr: Some(from.ip().to_string()),
                rtt_ms: Some(sent_at.elapsed().as_secs_f64() * 1000.0),
                timeout: false,
                ..Default::default()
            };
        }
        // Some other in-flight probe's reply (or unrelated ICMP traffic) — keep waiting for ours.
    }
}

/// Decodes `buf` and checks whether it's a reply to *our* probe (matching identifier + sequence,
/// which `surge_ping`'s parser correctly extracts from the embedded original packet even for a
/// "TTL exceeded" message) — the reporting address comes from the UDP `recv_from` peer address,
/// not from the packet, since it's identical and saves re-deriving it.
fn reply_matches(target: IpAddr, buf: &[u8], ident: u16, seq: u16) -> bool {
    match target {
        IpAddr::V4(_) => match Icmpv4Packet::decode(buf, Type::RAW, Ipv4Addr::UNSPECIFIED, Ipv4Addr::UNSPECIFIED) {
            Ok(pkt) => pkt.get_identifier().into_u16() == ident && pkt.get_sequence().into_u16() == seq,
            Err(_) => false,
        },
        IpAddr::V6(_) => match Icmpv6Packet::decode(buf, Ipv6Addr::UNSPECIFIED) {
            Ok(pkt) => pkt.get_identifier().into_u16() == ident && pkt.get_sequence().into_u16() == seq,
            Err(_) => false,
        },
    }
}

/// Builds a raw ICMP echo request. IPv4 checksums must be computed by the sender; IPv6 raw
/// sockets have the kernel fill in the checksum on send per RFC 3542 (§3.1), so it's left zero.
fn build_echo_request(target: IpAddr, ident: u16, seq: u16) -> Vec<u8> {
    let mut buf = vec![0u8; 8 + PAYLOAD.len()];
    buf[0] = match target {
        IpAddr::V4(_) => ICMPV4_ECHO_REQUEST,
        IpAddr::V6(_) => ICMPV6_ECHO_REQUEST,
    };
    buf[1] = 0;
    buf[4..6].copy_from_slice(&ident.to_be_bytes());
    buf[6..8].copy_from_slice(&seq.to_be_bytes());
    buf[8..].copy_from_slice(PAYLOAD);
    if target.is_ipv4() {
        let checksum = internet_checksum(&buf);
        buf[2..4].copy_from_slice(&checksum.to_be_bytes());
    }
    buf
}

fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let (chunks, remainder) = data.as_chunks::<2>();
    for chunk in chunks {
        sum += u16::from_be_bytes(*chunk) as u32;
    }
    if let [last] = *remainder {
        sum += (last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn permission_hint(e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::PermissionDenied {
        "permission denied opening a raw ICMP socket — run netdx as root/Administrator \
         (or on Linux: sudo setcap cap_net_raw+ep $(which netdx))"
            .to_string()
    } else {
        format!("failed to open ICMP socket: {e}")
    }
}
