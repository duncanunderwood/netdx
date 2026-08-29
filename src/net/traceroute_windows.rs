//! Windows-native IPv4 traceroute via the IP Helper API (`IcmpSendEcho2Ex`, the same function
//! Windows' own `tracert.exe` and `ping.exe` are built on).
//!
//! This exists because raw ICMP sockets — what `net::traceroute`'s cross-platform implementation
//! uses — silently receive nothing from intermediate routers on Windows: an unconnected raw
//! socket only gets ICMP traffic that matches an established connection state in the Windows
//! Filtering Platform, and a "TTL exceeded" reply from a router (a different source address than
//! the traceroute target) never matches that state, so it's dropped before it ever reaches
//! userspace. `tracert.exe` doesn't hit this because it doesn't use a raw socket at all — it goes
//! through this same IP Helper API, which is mediated by the kernel's TCP/IP driver rather than
//! ordinary socket I/O and isn't subject to that filtering. Bonus: unlike raw sockets, this API
//! doesn't require Administrator privileges.

use std::net::Ipv4Addr;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2Ex, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IP_SUCCESS, IP_TTL_EXPIRED_TRANSIT,
};

use crate::state::HopInfo;

const PROBE_TIMEOUT_MS: u32 = 2000;
const PAYLOAD: [u8; 32] = [0u8; 32];

/// Runs an IPv4 traceroute via `IcmpSendEcho2Ex`, invoking `on_hop` after every probed hop.
pub async fn run(target: Ipv4Addr, max_hops: u8, mut on_hop: impl FnMut(HopInfo, bool)) -> Result<(), String> {
    // Raw pointers aren't `Send`, so `handle` must not be held live across an `.await` point
    // (which would make this whole async fn's generated future non-`Send`, and it needs to be
    // `Send` to be spawned as a task). Scoping it to a block that only ever yields the `usize`
    // bits keeps it out of the future's state entirely; the pointer is reconstituted only
    // inside synchronous code (the `spawn_blocking` closures, and the final close call).
    let handle_bits = {
        let handle = unsafe { IcmpCreateFile() };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err("failed to open Windows ICMP handle (IcmpCreateFile)".to_string());
        }
        handle as usize
    };

    let target_bits = u32::from_ne_bytes(target.octets());
    let target_str = target.to_string();

    for ttl in 1..=max_hops {
        let hop = tokio::task::spawn_blocking(move || probe_hop(handle_bits as HANDLE, target_bits, ttl))
            .await
            .unwrap_or(HopInfo { ttl, timeout: true, ..Default::default() });

        let reached = hop.addr.as_deref() == Some(target_str.as_str());
        on_hop(hop, reached);

        if reached {
            break;
        }
    }

    unsafe {
        IcmpCloseHandle(handle_bits as HANDLE);
    }
    Ok(())
}

fn probe_hop(handle: HANDLE, target_bits: u32, ttl: u8) -> HopInfo {
    let options = IP_OPTION_INFORMATION {
        Ttl: ttl,
        Tos: 0,
        Flags: 0,
        OptionsSize: 0,
        OptionsData: std::ptr::null_mut(),
    };

    // MSDN: the reply buffer must be large enough for at least one ICMP_ECHO_REPLY plus the
    // echoed request data, plus 8 extra bytes for a possible ICMP error message.
    let reply_capacity = std::mem::size_of::<ICMP_ECHO_REPLY>() + PAYLOAD.len() + 8;
    let mut reply_buf = vec![0u8; reply_capacity];

    let replies = unsafe {
        IcmpSendEcho2Ex(
            handle,
            std::ptr::null_mut(),
            None,
            std::ptr::null(),
            0, // source address: let Windows pick the outgoing interface
            target_bits,
            PAYLOAD.as_ptr().cast(),
            PAYLOAD.len() as u16,
            &options,
            reply_buf.as_mut_ptr().cast(),
            reply_capacity as u32,
            PROBE_TIMEOUT_MS,
        )
    };

    if replies == 0 {
        return HopInfo { ttl, timeout: true, ..Default::default() };
    }

    // SAFETY: `replies > 0` guarantees the API wrote at least one ICMP_ECHO_REPLY at the start
    // of `reply_buf`, which we sized for exactly that above.
    let reply = unsafe { &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY) };

    match reply.Status {
        IP_SUCCESS | IP_TTL_EXPIRED_TRANSIT => HopInfo {
            ttl,
            addr: Some(Ipv4Addr::from(reply.Address.to_ne_bytes()).to_string()),
            rtt_ms: Some(f64::from(reply.RoundTripTime)),
            timeout: false,
            ..Default::default()
        },
        _ => HopInfo { ttl, timeout: true, ..Default::default() },
    }
}
