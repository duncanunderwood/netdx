# netdx web protocol (WebSocket, JSON)

Backend: Rust/axum. Endpoint `GET /ws?token=<TOKEN>` upgrades to WebSocket.
Page: `GET /?token=<TOKEN>` serves the SPA (single static HTML file, no build step, no external CDN — must work fully offline since the target machine may have no internet access other than the LAN the tech is diagnosing).

Auth: every HTTP and WS request must include `?token=<TOKEN>` matching the server-generated token, else `401`. The SPA must read `token` from `location.search` and append it to the WS URL it opens (`ws(s)://<host>/ws?token=<TOKEN>`), and must also append it to its own address bar usage is not required (server already validated the page load).

## Server -> Client messages (one JSON object per WS text frame)

Sent once on connect, and again every time state changes (server pushes full snapshot, not diffs):

```jsonc
{
  "type": "state",
  "network": {
    "default_interface": "en0" | null,
    "public_ip": "203.0.113.7" | null,
    "interfaces": [
      {
        "name": "en0",
        "friendly_name": "Wi-Fi" | null,
        "display_name": "Wi-Fi",            // ALWAYS use this as the card heading — never `name`
        "system_name": "eth0" | null,       // `name`, but only when it's not a meaningless Windows GUID; null on Windows almost always
        "if_type": "Wifi" | "Ethernet" | "Loopback" | "Other" | ... (free-form string, display as-is),
        "is_up": true,
        "is_loopback": false,
        "is_default": true,
        "mac": "aa:bb:cc:dd:ee:ff" | null,
        "ipv4": [ { "addr": "192.168.1.23", "prefix_len": 24 } ],
        "ipv6": [ { "addr": "fe80::1", "prefix_len": 64 } ],
        "mtu": 1500 | null,
        "dns_servers": ["1.1.1.1", "8.8.8.8"],
        "gateway": "192.168.1.1" | null,
        "rx_bytes": 12345678 | null,
        "tx_bytes": 12345678 | null
      }
    ]
  },
  "traceroute": {
    "target": "example.com",
    "resolved_ip": "93.184.216.34" | null,
    "running": false,
    "done": true,
    "max_hops": 30,
    "hops": [
      {
        "ttl": 1,
        "addr": "192.168.1.1" | null,
        "hostname": "router.home" | null,   // reverse-DNS, arrives a moment after the hop itself (best-effort)
        "city": "Sydney" | null,            // best-effort geo-IP, also arrives slightly late; usually null for private/local hops
        "country": "Australia" | null,
        "rtt_ms": 1.2 | null,
        "timeout": false
      }
    ],
    "error": null | "Traceroute requires elevated privileges: run as root/Administrator."
  },
  "telnet": {
    "connected": false,
    "connecting": false,
    "host": "192.168.1.1",
    "port": 23,
    "buffer": "last ~16000 chars of session transcript, newest at the end",
    "error": null | "Connection refused"
  },
  "speedtest": {
    "running": false,
    "stage": "idle" | "ping" | "download" | "upload" | "done" | "error",
    "ping_ms": 14.2 | null,
    "jitter_ms": 1.1 | null,
    "packet_loss_pct": 0.0 | null,
    "download_mbps": 234.5 | null,          // final average — null until the download phase finishes
    "upload_mbps": 45.2 | null,             // final average — null until the upload phase finishes
    "download_samples": [12.1, 45.6, ...],  // Mbps over time, appended live roughly every 200ms while stage === "download"
    "upload_samples": [3.2, 10.4, ...],     // same, while stage === "upload"
    "server": "Cloudflare (global)",        // human-readable label of the server actually used for the last/current run
    "available_servers": [                  // static list, always present, for populating a server picker
      { "id": "cloudflare", "label": "Cloudflare (global)", "supports_upload": true }
    ],
    "selected_server": "cloudflare",        // id of the currently-selected server (persists across runs)
    "error": null
  },
  "log": [
    { "ts": "2026-08-30T14:03:41+10:00", "message": "traceroute to example.com started" },
    { "ts": "2026-08-30T14:04:02+10:00", "message": "event log exported: netdx-log-20260830-140402.csv", "export_filename": "netdx-log-20260830-140402.csv" }
  ],   // newest last, cap 200 entries. `ts` is the *server machine's local time*, ISO-8601
       // with its UTC offset (`±HH:MM`) — not UTC, so it matches whatever the technician's own
       // clock says. `entry.ts.slice(11, 19)` still gets you `HH:MM:SS` regardless of offset
       // width. `export_filename` is present (non-null) only on the one entry announcing a
       // completed `log_export` — render that row as a link to
       // `GET /exports/<export_filename>?token=<TOKEN>` (see below); every other entry omits
       // the field entirely (not just `null` — check for its presence/truthiness).
  "update": {
    "current_version": "0.2.0",
    "checking": false,
    "latest_version": "0.3.0" | null,        // set after any check, whether or not it's newer
    "update_available": true,
    "installing": false,
    "error": null | "GitHub returned HTTP 403",
    "release_url": "https://github.com/duncanunderwood/netdx/releases/tag/v0.3.0" | null
  }
}
```

### Live speed test numbers while running

There is no separate "current reading" field — while `stage` is `"download"` or `"upload"`, the
*last element* of `download_samples`/`upload_samples` **is** the live instantaneous reading (a
new sample lands roughly every 200ms, each push triggers a fresh `state` message). Once that
phase finishes, `download_mbps`/`upload_mbps` holds the final average and should be shown
instead. In other words: display `download_mbps` if it's non-null, otherwise the last entry of
`download_samples` if any exist, otherwise a placeholder — and equivalently for upload.

## Client -> Server messages (one JSON object per WS text frame)

```jsonc
{"cmd":"refresh_interfaces"}
{"cmd":"traceroute_start","target":"8.8.8.8","max_hops":30}
{"cmd":"traceroute_stop"}
{"cmd":"telnet_connect","host":"192.168.1.1","port":23}
{"cmd":"telnet_send","data":"text\n"}
{"cmd":"telnet_disconnect"}
{"cmd":"speedtest_start","server":"cloudflare"}
{"cmd":"speedtest_stop"}
{"cmd":"log_clear"}
{"cmd":"log_export"}
{"cmd":"check_for_update"}
{"cmd":"install_update"}
```

`max_hops` optional in `traceroute_start` (server defaults to 30).

`server` is optional in `speedtest_start` (an omitted/unrecognized id falls back to the last-selected server, or Cloudflare). Its value must be one of `available_servers[].id` from the most recent state message. When the selected server has `"supports_upload": false`, the backend skips the upload stage entirely — `upload_mbps` stays `null` and `stage` goes straight from `"download"` to `"done"`; the SPA should show "not supported by this server" instead of a dash for the upload stat in that case, not imply the test hung. (Today only Cloudflare is offered, and it always supports upload — but don't hardcode that assumption away.)

`log_export` writes the current log (server-side, full ~200-entry buffer, not just what the SPA happens to have rendered) to a CSV file under the app's data directory and appends a log line noting the path (or the error) — that entry carries `export_filename`, which the SPA turns into a `GET /exports/<export_filename>?token=<TOKEN>` download link (see below). `log_clear` empties the log (both server and SPA copies update via the next `state` push) and also appends one fresh "event log cleared" entry.

`check_for_update` triggers a background GitHub Releases check; watch `update.checking` (spinner) then `update.update_available`/`update.error`. `install_update` only does something if `update.update_available` is currently true (silently logs a message and no-ops otherwise) — downloads the new binary, swaps it in, and relaunches netdx as a new process. **The server process exits on success** — the WebSocket will drop and the SPA's reconnect-with-backoff logic should kick in and quietly reconnect once the new process is back up (same port, same token, since the relaunch preserves the original CLI args). Show `update.installing` as a blocking "installing, restarting shortly…" state meanwhile.

## `GET /exports/{filename}?token=<TOKEN>`

Downloads a previously-exported CSV (`Content-Disposition: attachment`). `filename` must exactly match what a `log` entry's `export_filename` field gave you — the server only ever serves its own `netdx-log-<timestamp>.csv` naming pattern and 400s on anything else (no arbitrary path access). `401` on a missing/bad token, `404` if the file's since been moved/deleted.

## Design brief for the SPA (single file: `src/web/static/index.html`, inline `<style>`/`<script>`, zero external requests/CDNs/fonts/build tools)

- Single HTML file, works when opened via `http://<lan-ip>:<port>/?token=...` on a phone or laptop browser on the same network (or over the internet if port-forwarded/tunneled). No React/build step — vanilla JS + WebSocket + DOM (or plain `<canvas>` for the sparkline), so the Rust binary can `include_str!` it with zero JS toolchain dependency.
- Tabs/sections: **Overview** (interfaces — like `ipconfig`/`ifconfig`, shows default interface highlighted, gateway, DNS, MAC, IPs, MTU, up/down badge), **Traceroute** (input box + start/stop, live hop table with RTT bars, hostname + location columns), **Telnet** (host:port input, connect/disconnect, terminal-style scrollback pane + input line, monospace), **Speed Test** (big start button, live gauges/chart for download/upload while running, final stats grid: ping/jitter/loss/down/up).
- Fully responsive: usable on a phone screen (techs walking around a site) and a desktop monitor. Mobile-first flex/grid layout, tap targets >= 40px.
- Reconnect automatically if the WebSocket drops (retry with backoff), show a small connection-status pill. Must keep working through an `install_update`-triggered restart (server exits, comes back up moments later on the same port).
- **Colourblind-safe palette — do not use raw red/green as the only signal.** Use the Okabe-Ito palette as CSS custom properties:
  - `--ok:#009E73`, `--warn:#E69F00`, `--bad:#D55E00` (never pure red), `--info:#0072B2`, `--accent2:#56B4E9`, `--accent3:#CC79A7`.
  - Every status indicator pairs colour with a shape/icon/text label, never colour alone.
- "Quick and modern": subtle transitions, no layout jank, live-updating numbers without full-page re-render (diff the DOM or just update text content).

### Already implemented (do not redo, just don't break)

Light/dark theme toggle; tab switching; WS reconnect-with-backoff; the traceroute hop table (hostname/location columns); the telnet terminal pane; the persistent "Developed by MyEvent Labs" footer; an always-visible Activity Log panel below `<main>` (latest 10 rows, scrollable past that, Export CSV / Clear buttons) with export rows rendered as a clickable link to `/exports/<filename>`; a smoothed, filled-gradient speed test chart with live Download/Upload numbers labeled `(live)`/`(avg)`; and a "Check for Updates" header button with a persistent pulsing-dot badge (independent of the dismissible "update available" banner) that only clears once the update installs or a fresh check finds nothing newer. All already work correctly against this protocol — extend, don't replace.
