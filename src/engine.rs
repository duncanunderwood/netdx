//! Single-owner actor: consumes `Command`s from either the local TUI or remote web clients,
//! owns every background task (traceroute probes, the telnet socket, speed test transfers), and
//! is the only writer of `AppState`. Readers (TUI render loop, web `/ws` handlers) take a brief
//! read lock and otherwise just watch `changed` for wake-ups. `AppState` is guarded by a plain
//! `parking_lot::RwLock`: every critical section here is a few field writes with no `.await`
//! inside it, so a blocking lock is simpler and cheaper than an async one.

use std::sync::Arc;
use parking_lot::RwLock;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::net::{interfaces, speedtest, telnet, traceroute};
use crate::state::{AppState, Command, SpeedtestState, TelnetState, TracerouteState};

pub type SharedState = Arc<RwLock<AppState>>;

pub fn new_shared_state() -> SharedState {
    Arc::new(RwLock::new(AppState::default()))
}

pub async fn run(
    state: SharedState,
    changed_tx: watch::Sender<()>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let mut traceroute_task: Option<JoinHandle<()>> = None;
    let mut telnet_task: Option<JoinHandle<()>> = None;
    let mut telnet_input: Option<mpsc::UnboundedSender<Vec<u8>>> = None;
    let mut speedtest_task: Option<JoinHandle<()>> = None;

    load_interfaces(&state, &changed_tx, "interfaces loaded");
    spawn_public_ip_lookup(&state, &changed_tx);

    while let Some(cmd) = commands.recv().await {
        match cmd {
            Command::RefreshInterfaces => {
                load_interfaces(&state, &changed_tx, "interfaces refreshed");
                spawn_public_ip_lookup(&state, &changed_tx);
            }

            Command::TracerouteStart { target, max_hops } => {
                if let Some(handle) = traceroute_task.take() {
                    handle.abort();
                }
                let run_id = {
                    let mut st = state.write();
                    let run_id = st.traceroute.run_id.wrapping_add(1);
                    st.traceroute = TracerouteState {
                        target: target.clone(),
                        max_hops,
                        running: true,
                        run_id,
                        ..Default::default()
                    };
                    st.push_log(format!("traceroute to {target} started"));
                    run_id
                };
                let _ = changed_tx.send(());

                let state2 = state.clone();
                let changed2 = changed_tx.clone();
                traceroute_task = Some(tokio::spawn(run_traceroute(state2, changed2, target, max_hops, run_id)));
            }
            Command::TracerouteStop => {
                if let Some(handle) = traceroute_task.take() {
                    handle.abort();
                }
                let mut st = state.write();
                st.traceroute.running = false;
                st.push_log("traceroute stopped");
                drop(st);
                let _ = changed_tx.send(());
            }

            Command::TelnetConnect { host, port } => {
                if let Some(handle) = telnet_task.take() {
                    handle.abort();
                }
                {
                    let mut st = state.write();
                    st.telnet = TelnetState {
                        connecting: true,
                        host: host.clone(),
                        port,
                        buffer: format!("Trying {host}:{port}...\r\n"),
                        ..Default::default()
                    };
                    st.push_log(format!("telnet connecting to {host}:{port}"));
                }
                let _ = changed_tx.send(());

                let (input_tx, input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
                telnet_input = Some(input_tx);
                let (event_tx, mut event_rx) = mpsc::unbounded_channel::<telnet::TelnetEvent>();

                telnet_task = Some(tokio::spawn(telnet::run(host.clone(), port, event_tx, input_rx)));

                let state2 = state.clone();
                let changed2 = changed_tx.clone();
                tokio::spawn(async move {
                    while let Some(ev) = event_rx.recv().await {
                        let mut st = state2.write();
                        match ev {
                            telnet::TelnetEvent::Connected => {
                                st.telnet.connected = true;
                                st.telnet.connecting = false;
                                append_capped(
                                    &mut st.telnet.buffer,
                                    &format!("Connected to {host}.\r\nEscape character is '^]'.\r\n"),
                                    crate::state::TELNET_BUFFER_CAP,
                                );
                                st.push_log("telnet connected");
                            }
                            telnet::TelnetEvent::Data(text) => {
                                append_capped(&mut st.telnet.buffer, &text, crate::state::TELNET_BUFFER_CAP);
                            }
                            telnet::TelnetEvent::Error(e) => {
                                st.telnet.error = Some(e.clone());
                                st.telnet.connecting = false;
                                st.telnet.connected = false;
                                append_capped(
                                    &mut st.telnet.buffer,
                                    &format!("{host}: {e}\r\n"),
                                    crate::state::TELNET_BUFFER_CAP,
                                );
                                st.push_log(format!("telnet error: {e}"));
                            }
                            telnet::TelnetEvent::Disconnected => {
                                st.telnet.connected = false;
                                st.telnet.connecting = false;
                                append_capped(
                                    &mut st.telnet.buffer,
                                    "Connection closed by foreign host.\r\n",
                                    crate::state::TELNET_BUFFER_CAP,
                                );
                                st.push_log("telnet disconnected");
                            }
                        }
                        drop(st);
                        let _ = changed2.send(());
                    }
                });
            }
            Command::TelnetSend { data } => {
                if let Some(tx) = &telnet_input {
                    let mut bytes = data.into_bytes();
                    if !bytes.ends_with(b"\n") {
                        bytes.extend_from_slice(b"\r\n");
                    }
                    let _ = tx.send(bytes);
                }
            }
            Command::TelnetDisconnect => {
                telnet_input = None; // closes the channel; telnet::run treats that as hang-up
                if let Some(handle) = telnet_task.take() {
                    handle.abort();
                }
                let mut st = state.write();
                let was_connected = st.telnet.connected;
                st.telnet.connected = false;
                st.telnet.connecting = false;
                if was_connected {
                    append_capped(&mut st.telnet.buffer, "Connection closed.\r\n", crate::state::TELNET_BUFFER_CAP);
                }
                st.push_log("telnet disconnected by user");
                drop(st);
                let _ = changed_tx.send(());
            }


            Command::SpeedtestStart { server } => {
                if let Some(handle) = speedtest_task.take() {
                    handle.abort();
                }
                let requested = server.as_deref().and_then(speedtest::ServerId::from_id_str);
                let fallback = {
                    let st = state.read();
                    speedtest::ServerId::from_id_str(&st.speedtest.selected_server).unwrap_or_default()
                };
                let resolved = requested.unwrap_or(fallback);
                {
                    let mut st = state.write();
                    st.speedtest = SpeedtestState {
                        running: true,
                        stage: "ping".to_string(),
                        server: resolved.label().to_string(),
                        selected_server: resolved.id_str().to_string(),
                        ..Default::default()
                    };
                    st.push_log(format!("speed test started ({})", resolved.label()));
                }
                let _ = changed_tx.send(());

                let state2 = state.clone();
                let changed2 = changed_tx.clone();
                speedtest_task = Some(tokio::spawn(run_speedtest(state2, changed2, resolved)));
            }
            Command::SpeedtestStop => {
                if let Some(handle) = speedtest_task.take() {
                    handle.abort();
                }
                let mut st = state.write();
                st.speedtest.running = false;
                st.speedtest.stage = "idle".to_string();
                st.push_log("speed test stopped");
                drop(st);
                let _ = changed_tx.send(());
            }
        }
    }
}

fn load_interfaces(state: &SharedState, changed_tx: &watch::Sender<()>, log_msg: &str) {
    let snap = interfaces::snapshot();
    let mut st = state.write();
    st.network = snap;
    st.push_log(log_msg);
    drop(st);
    let _ = changed_tx.send(());
}

fn spawn_public_ip_lookup(state: &SharedState, changed_tx: &watch::Sender<()>) {
    let state = state.clone();
    let changed_tx = changed_tx.clone();
    tokio::spawn(async move {
        if let Some(ip) = interfaces::lookup_public_ip().await {
            let mut st = state.write();
            st.network.public_ip = Some(ip);
            drop(st);
            let _ = changed_tx.send(());
        }
    });
}

/// Appends `text` to `buffer`, trimming from the front (at a char boundary) once `cap` bytes is
/// exceeded, so a chatty telnet session can't grow the scrollback without bound.
fn append_capped(buffer: &mut String, text: &str, cap: usize) {
    buffer.push_str(text);
    let overflow = buffer.len().saturating_sub(cap);
    if overflow > 0 {
        let cut = buffer
            .char_indices()
            .find(|(i, _)| *i >= overflow)
            .map(|(i, _)| i)
            .unwrap_or(0);
        buffer.drain(..cut);
    }
}

async fn run_traceroute(state: SharedState, changed: watch::Sender<()>, target: String, max_hops: u8, run_id: u64) {
    let ip = match traceroute::resolve_target(&target).await {
        Ok(ip) => ip,
        Err(e) => {
            let mut st = state.write();
            st.traceroute.running = false;
            st.traceroute.done = true;
            st.traceroute.error = Some(e.clone());
            st.push_log(format!("traceroute error: {e}"));
            drop(st);
            let _ = changed.send(());
            return;
        }
    };
    {
        let mut st = state.write();
        st.traceroute.resolved_ip = Some(ip.to_string());
        drop(st);
        let _ = changed.send(());
    }

    let state_cb = state.clone();
    let changed_cb = changed.clone();
    let result = traceroute::run(ip, max_hops, move |hop, reached| {
        let mut st = state_cb.write();
        st.push_log(format!(
            "hop {}: {}{}",
            hop.ttl,
            hop.addr.clone().unwrap_or_else(|| "*".to_string()),
            if reached { " (destination reached)" } else { "" }
        ));
        let ttl = hop.ttl;
        let addr = hop.addr.clone();
        st.traceroute.hops.push(hop);
        drop(st);
        let _ = changed_cb.send(());

        if let Some(addr) = addr {
            tokio::spawn(enrich_hop(state_cb.clone(), changed_cb.clone(), run_id, ttl, addr));
        }
    })
    .await;

    let mut st = state.write();
    st.traceroute.running = false;
    st.traceroute.done = true;
    if let Err(e) = result {
        st.traceroute.error = Some(e.clone());
        st.push_log(format!("traceroute error: {e}"));
    }
    drop(st);
    let _ = changed.send(());
}

/// Best-effort reverse-DNS + geolocation for one hop, applied after the hop already appears
/// with just its address so the traceroute keeps moving — this only ever *adds* detail to an
/// existing row. Skipped if a newer traceroute run has since started (`run_id` mismatch).
async fn enrich_hop(state: SharedState, changed: watch::Sender<()>, run_id: u64, ttl: u8, addr: String) {
    let Ok(ip) = addr.parse::<std::net::IpAddr>() else { return };
    let (hostname, geo) = tokio::join!(traceroute::reverse_dns(ip), crate::net::geoip::lookup(ip));
    if hostname.is_none() && geo.is_none() {
        return;
    }

    let mut st = state.write();
    if st.traceroute.run_id != run_id {
        return; // a newer traceroute has since replaced this hop list
    }
    if let Some(hop) = st.traceroute.hops.iter_mut().find(|h| h.ttl == ttl && h.addr.as_deref() == Some(addr.as_str())) {
        hop.hostname = hostname;
        if let Some(geo) = geo {
            hop.city = geo.city;
            hop.country = geo.country;
        }
    }
    drop(st);
    let _ = changed.send(());
}


async fn run_speedtest(state: SharedState, changed: watch::Sender<()>, server: speedtest::ServerId) {
    let ping = speedtest::measure_ping(server).await;
    {
        let mut st = state.write();
        st.speedtest.ping_ms = ping.ping_ms;
        st.speedtest.jitter_ms = ping.jitter_ms;
        st.speedtest.packet_loss_pct = Some(ping.loss_pct);
        st.speedtest.stage = "download".to_string();
        st.push_log("speed test: ping measured");
        drop(st);
        let _ = changed.send(());
    }

    let state_dl = state.clone();
    let changed_dl = changed.clone();
    let dl_result = speedtest::measure_download(server, move |mbps| {
        let mut st = state_dl.write();
        st.speedtest.download_samples.push(mbps);
        drop(st);
        let _ = changed_dl.send(());
    })
    .await;

    {
        let mut st = state.write();
        match &dl_result {
            Ok(mbps) => {
                st.speedtest.download_mbps = Some(*mbps);
                st.push_log(format!("speed test: download {mbps:.1} Mbps"));
            }
            Err(e) => {
                st.speedtest.error = Some(e.clone());
                st.push_log(format!("speed test download error: {e}"));
            }
        }
        if server.supports_upload() {
            st.speedtest.stage = "upload".to_string();
        } else {
            st.push_log(format!("{} doesn't support an upload test — skipping", server.label()));
        }
        drop(st);
        let _ = changed.send(());
    }

    if server.supports_upload() {
        let state_ul = state.clone();
        let changed_ul = changed.clone();
        let ul_result = speedtest::measure_upload(move |mbps| {
            let mut st = state_ul.write();
            st.speedtest.upload_samples.push(mbps);
            drop(st);
            let _ = changed_ul.send(());
        })
        .await;

        let mut st = state.write();
        match &ul_result {
            Ok(mbps) => {
                st.speedtest.upload_mbps = Some(*mbps);
                st.push_log(format!("speed test: upload {mbps:.1} Mbps"));
            }
            Err(e) => {
                st.speedtest.error = Some(e.clone());
                st.push_log(format!("speed test upload error: {e}"));
            }
        }
    }

    let mut st = state.write();
    st.speedtest.running = false;
    st.speedtest.stage = "done".to_string();
    drop(st);
    let _ = changed.send(());
}
