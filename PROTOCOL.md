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
        "display_name": "Wi-Fi",            // ALWAYS use this as the card heading — never `name` (see Task below)
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
    "buffer": "last ~8000 chars of session transcript, newest at the end",
    "error": null | "Connection refused"
  },
  "speedtest": {
    "running": false,
    "stage": "idle" | "ping" | "download" | "upload" | "done" | "error",
    "ping_ms": 14.2 | null,
    "jitter_ms": 1.1 | null,
    "packet_loss_pct": 0.0 | null,
    "download_mbps": 234.5 | null,
    "upload_mbps": 45.2 | null,
    "download_samples": [12.1, 45.6, ...],   // Mbps over time, for a live sparkline/chart
    "upload_samples": [3.2, 10.4, ...],
    "server": "Cloudflare",                  // human-readable label of the server actually used for the last/current run
    "available_servers": [                   // static list, always present, for populating a server picker
      { "id": "cloudflare", "label": "Cloudflare", "supports_upload": true },
      { "id": "hetzner", "label": "Hetzner (Falkenstein, DE)", "supports_upload": false },
      { "id": "ovh", "label": "OVH (France)", "supports_upload": false }
    ],
    "selected_server": "cloudflare",         // id of the currently-selected server (persists across runs)
    "error": null
  },
  "log": ["12:03:41 traceroute to example.com started", "..."]   // newest last, cap ~200 entries
}
```

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
```

`max_hops` optional in `traceroute_start` (server defaults to 30).

`server` is optional in `speedtest_start` (an omitted/unrecognized id falls back to the last-selected server, or Cloudflare). Its value must be one of `available_servers[].id` from the most recent state message. When the selected server has `"supports_upload": false`, the backend skips the upload stage entirely — `upload_mbps` stays `null` and `stage` goes straight from `"download"` to `"done"`; the SPA should show "not supported by this server" instead of a dash for the upload stat in that case, not imply the test hung.

## Design brief for the SPA (single file: `src/web/static/index.html`, inline `<style>`/`<script>`, zero external requests/CDNs/fonts/build tools)

- Single HTML file, works when opened via `http://<lan-ip>:<port>/?token=...` on a phone or laptop browser on the same network (or over the internet if port-forwarded/tunneled). No React/build step — vanilla JS + WebSocket + DOM (or plain `<canvas>` for the sparkline), so the Rust binary can `include_str!` it with zero JS toolchain dependency.
- Tabs/sections: **Overview** (interfaces — like `ipconfig`/`ifconfig`, shows default interface highlighted, gateway, DNS, MAC, IPs, MTU, up/down badge), **Traceroute** (input box + start/stop, live hop table with RTT bars), **Telnet** (host:port input, connect/disconnect, terminal-style scrollback pane + input line, monospace), **Speed Test** (big start button, live gauges/sparkline for download/upload while running, final stats grid: ping/jitter/loss/down/up).
- Fully responsive: usable on a phone screen (techs walking around a site) and a desktop monitor. Mobile-first flex/grid layout, tap targets >= 40px.
- Reconnect automatically if the WebSocket drops (retry with backoff), show a small connection-status pill.
- **Colourblind-safe palette — do not use raw red/green as the only signal.** Use the Okabe-Ito palette as CSS custom properties:
  - `--ok:#009E73` (bluish green, success/up)
  - `--warn:#E69F00` (orange, warning/degraded)
  - `--bad:#D55E00` (vermillion, error/down — NOT pure red)
  - `--info:#0072B2` (blue, informational/accent)
  - `--accent2:#56B4E9` (sky blue, secondary accent)
  - `--accent3:#CC79A7` (reddish purple, tertiary accent, e.g. upload vs download series)
  - `--fg:#1a1a1a` on `--bg:#f5f5f5` for light mode; `--fg:#f0f0f0` on `--bg:#121212` for dark mode, keeping the same accent hues.
  - Every status indicator pairs colour with a shape/icon/text label (e.g. a filled circle + "UP"/"DOWN" text, or ✓/✗), never colour alone, so protanopia/deuteranopia/tritanopia users can distinguish state without hue perception.
  - Typography: system font stack (`-apple-system, Segoe UI, Roboto, sans-serif`), monospace (`ui-monospace, Consolas, Menlo, monospace`) for IPs/MACs/telnet output/hop tables so columns align.
- "Quick and modern": subtle transitions, no layout jank, live-updating numbers without full-page re-render (diff the DOM or just update text content), a live sparkline canvas for speedtest Mbps over time using `--info`/`--accent3` for down/up series respectively.

### Already implemented (do not redo, just don't break): explicit light/dark theme toggle and the speed test server `<select>`. Both already work correctly against this protocol.

### Task: never show raw interface identifiers (GUIDs)

Windows adapter `name`s are opaque GUIDs like `{CB40F214-85E6-4CDE-96B7-5670A433AA8A}` — currently the interface card heading. Fix: use `display_name` as the card heading everywhere `name` was previously shown (it's already the friendly label — "Ethernet", "Wi-Fi", "Tailscale", etc. — with a sane fallback baked in server-side). If you want a small secondary/system identifier under the heading (optional, e.g. for Linux/macOS techs who care about `eth0`), use `system_name` — but only render it when it is non-null; never fall back to `name` for display. `name` itself should not appear anywhere in the rendered UI.

### Task: traceroute hop hostname + location

Each hop in `traceroute.hops` now carries `hostname`, `city`, and `country`, populated a moment after the hop's `addr`/`rtt_ms` (separate state push — the row should update in place, not flicker/reflow). Add two columns (or a compact combined sub-line under the address, whichever fits the existing hop table/list styling better) showing: reverse-DNS hostname (fall back to "—" when null — very common for the private IPs of near hops), and location as `"{city}, {country}"` (or whichever of the two is present, or "—" if both are null — also very common for private/local hops, which no geo-IP service can place). Keep the RTT bar/timeout styling exactly as-is.

### Task: footer credit on every page/tab

Add a persistent footer (below `<main>`, above or alongside the existing log drawer toggle — pick whatever doesn't visually collide) reading "Developed by MyEvent Labs" where "MyEvent Labs" is a real `<a href="https://myevent-labs.io" target="_blank" rel="noopener noreferrer">` link, muted/secondary styling consistent with the rest of the chrome (not distracting), visible regardless of which tab is active.

Write the file to `C:/tmp/netdx/src/web/static/index.html` (create the `src/web/static/` directory).
