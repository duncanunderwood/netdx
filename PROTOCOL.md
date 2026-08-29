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
    { "ts": "2026-08-30T14:03:41Z", "message": "traceroute to example.com started" }
  ],   // newest last, cap 200 entries. `ts` is full RFC3339 UTC (not just a time-of-day string).
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

`log_export` writes the current log (server-side, full ~200-entry buffer, not just what the SPA happens to have rendered) to a CSV file under the app's data directory and appends a log line noting the path (or the error) — the SPA doesn't receive the file directly, just watch `log` for the resulting entry. `log_clear` empties the log (both server and SPA copies update via the next `state` push) and also appends one fresh "event log cleared" entry.

`check_for_update` triggers a background GitHub Releases check; watch `update.checking` (spinner) then `update.update_available`/`update.error`. `install_update` only does something if `update.update_available` is currently true (silently logs a message and no-ops otherwise) — downloads the new binary, swaps it in, and relaunches netdx as a new process. **The server process exits on success** — the WebSocket will drop and the SPA's reconnect-with-backoff logic should kick in and quietly reconnect once the new process is back up (same port, same token, since the relaunch preserves the original CLI args). Show `update.installing` as a blocking "installing, restarting shortly…" state meanwhile.

## Design brief for the SPA (single file: `src/web/static/index.html`, inline `<style>`/`<script>`, zero external requests/CDNs/fonts/build tools)

- Single HTML file, works when opened via `http://<lan-ip>:<port>/?token=...` on a phone or laptop browser on the same network (or over the internet if port-forwarded/tunneled). No React/build step — vanilla JS + WebSocket + DOM (or plain `<canvas>` for the sparkline), so the Rust binary can `include_str!` it with zero JS toolchain dependency.
- Tabs/sections: **Overview** (interfaces — like `ipconfig`/`ifconfig`, shows default interface highlighted, gateway, DNS, MAC, IPs, MTU, up/down badge), **Traceroute** (input box + start/stop, live hop table with RTT bars, hostname + location columns), **Telnet** (host:port input, connect/disconnect, terminal-style scrollback pane + input line, monospace), **Speed Test** (big start button, live gauges/chart for download/upload while running, final stats grid: ping/jitter/loss/down/up).
- Fully responsive: usable on a phone screen (techs walking around a site) and a desktop monitor. Mobile-first flex/grid layout, tap targets >= 40px.
- Reconnect automatically if the WebSocket drops (retry with backoff), show a small connection-status pill. This already exists and must keep working through an `install_update`-triggered restart (server exits, comes back up moments later on the same port).
- **Colourblind-safe palette — do not use raw red/green as the only signal.** Use the Okabe-Ito palette as CSS custom properties (already defined, keep as-is):
  - `--ok:#009E73`, `--warn:#E69F00`, `--bad:#D55E00` (never pure red), `--info:#0072B2`, `--accent2:#56B4E9`, `--accent3:#CC79A7`.
  - Every status indicator pairs colour with a shape/icon/text label, never colour alone.
- "Quick and modern": subtle transitions, no layout jank, live-updating numbers without full-page re-render (diff the DOM or just update text content).

### Already implemented (do not redo, just don't break)

Light/dark theme toggle, the speed test server `<select>`, tab switching, WS reconnect-with-backoff, the traceroute hop table (with hostname/location columns), the telnet terminal pane, and the persistent "Developed by MyEvent Labs" footer. All already work correctly against this protocol.

### Task: rework the activity log from a floating drawer into an always-visible panel

Currently there's a floating "Log" button (`#logToggle`) that opens an overlay drawer
(`#logDrawer`) showing the *entire* log as one big scrollable blob of plain text. Replace this
with an **always-visible panel** below `<main>` (visible regardless of which tab is active — so
outside the `tabpanel` divs, same idea as the footer), matching what the terminal UI now does:

- Shows the **latest 10 entries** by default, one per row, each formatted as the `HH:MM:SS` time
  (sliced out of the full `ts` RFC3339 string — e.g. `entry.ts.slice(11, 19)`) plus the message.
- When there are more than 10 total entries, the panel becomes scrollable (a bounded-height
  `overflow-y:auto` container sized to ~10 rows) so the tech can scroll up to see older entries —
  a real native scrollbar is fine and expected here, no need to hand-roll virtual scrolling.
- A small header row with the panel title, an **"Export CSV"** button that sends
  `{"cmd":"log_export"}`, and a **"Clear"** button that sends `{"cmd":"log_clear"}` (a plain
  click is fine, no confirmation dialog needed — it's a local diagnostics log, not destructive to
  anything else).
- Remove the old floating button/drawer entirely (`#logToggle`, `#logDrawer`, their CSS and JS)
  once the new panel replaces them — no dead code left behind.
- Reuse the existing colourblind-safe `.card`/`.btn` styling conventions already in the file;
  this is a diagnostics/utility panel, keep it visually quiet (muted, not competing with the tab
  content above it).

### Task: speed test chart — smoother, and closer to a real dashboard

The current `drawSparkline` draws a raw jagged line straight from `download_samples`/
`upload_samples` on `<canvas id="spark">`. Two changes:

1. **Smooth it**: apply a short centered moving average (window of ~3–4 samples) to each series
   before plotting — flattens the sample-to-sample zigzag (each sample is an instantaneous
   ~200ms reading, inherently noisy) into a readable trend, without hiding the real trend shape.
2. **Make it look like a real speed-test dashboard**: filled area under each curve (a
   `canvas` linear gradient fading from the series colour at ~25% opacity down to transparent,
   drawn as a closed path down to the baseline before the stroked line on top), plus a large,
   prominent live number above the chart per direction (Download / Upload) that updates in real
   time while that stage is running and settles to the final average once done — mirroring a
   typical "Your Internet Speed" style layout (big number, small filled sparkline beneath it).
   Follow the existing rule: `download_mbps` if set, else the last entry of `download_samples`
   if any, else a placeholder — same for upload.
   Use `--info` for the download series/number and `--accent3` for upload, consistent with the
   rest of the app's colour semantics — don't switch to orange/purple just to visually match a
   reference screenshot; the palette here is deliberately colourblind-distinct from `--warn`.

### Task: "Check for Updates"

Add a **"Check for Updates"** button (header area, near the theme toggle is a reasonable spot)
that sends `{"cmd":"check_for_update"}`. Behavior driven entirely by `state.update`:

- While `update.checking` is true, show a brief inline "Checking…" state on/near the button.
- Once resolved: if `update.update_available` is true, show a small banner or modal — "Update
  available: vX.Y.Z" with an **"Install & Restart"** button sending
  `{"cmd":"install_update"}`, and a way to dismiss it. If `update.error` is set, show it (reuse
  the existing `.error-text` styling). If neither and `update.latest_version` is set, briefly
  indicate "up to date" then let it fade/dismiss — don't leave a permanent "you're current"
  banner cluttering the header.
- While `update.installing` is true, show a blocking-but-calm "Installing update, restarting…"
  state (the server process is about to exit and come back up in a new process) — the existing
  WS reconnect logic will pick the new instance back up automatically once it's listening again;
  no special handling needed beyond not panicking the UI when the socket drops during this.

Write the file to `C:/tmp/netdx/src/web/static/index.html`.
