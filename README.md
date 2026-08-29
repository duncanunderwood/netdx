# netdx

**netdx** is a free, all-in-one network troubleshooting tool. It runs in a
terminal window and shows you, in real time:

- **Your network setup** — like Windows' `ipconfig` or macOS/Linux's
  `ifconfig`, but laid out clearly, with plain-English adapter names (no
  cryptic device codes) and colour-coded up/down status.
- **Traceroute** — see every "hop" your internet traffic takes to reach a
  website or server, including the hostname and city/country of each hop
  where that information is available.
- **Telnet** — connect to another device or port and watch the full
  back-and-forth, exactly like a classic telnet terminal window.
- **Speed test** — measure ping, jitter, packet loss, download and upload
  speed against Cloudflare's global network, with a live-updating chart as
  the test runs.
- **Event log** — every action netdx takes is timestamped and kept on
  screen (the last 10 entries, scrollable further back), and can be
  exported to a CSV file or cleared with one keypress/click.

It also opens a **web page** you (or anyone else on the network, or a
colleague on their phone) can visit to see and control the exact same
screens from a browser — with light and dark mode, and a QR code printed
right in the terminal so a phone camera can jump straight to the page
without typing anything.

netdx can check GitHub for newer releases on demand and install them
in place (see [Checking for updates](#checking-for-updates)). It also sends
a small amount of anonymous, aggregate usage analytics — see
[SUPABASE.md](SUPABASE.md) for exactly what's sent and how to opt out with
`--no-analytics`.

## Getting netdx

Pick your operating system below.

### Windows

1. Open **PowerShell** (search for it in the Start menu).
2. Paste this line in and press **Enter**:
   ```powershell
   irm https://github.com/duncanunderwood/netdx/releases/latest/download/install.ps1 | iex
   ```
3. Close and reopen PowerShell once it finishes.
4. Type `netdx` and press **Enter** to run it.

### macOS

1. Open **Terminal** (find it in Spotlight search).
2. Paste this line in and press **Enter**:
   ```sh
   curl -fsSL https://github.com/duncanunderwood/netdx/releases/latest/download/install.sh | sh
   ```
3. Type `netdx` and press **Enter** to run it.

### Linux

Same as macOS — open a terminal and run:
```sh
curl -fsSL https://github.com/duncanunderwood/netdx/releases/latest/download/install.sh | sh
```
Then run `netdx`.

> Those two commands download the correct ready-to-run program for your
> computer automatically — nothing else to configure. If your organisation
> builds netdx from source instead, see [Building from source](#building-from-source)
> below.

## Using netdx

When you run `netdx`, you'll see four screens across the top — use the
**Tab** key, arrow keys, or click the tabs in the web page to switch
between them:

1. **Overview** — your network adapters (Wi-Fi, Ethernet, VPNs, etc.),
   whether each is up or down, its IP addresses, gateway, DNS servers, and
   which one is your computer's main connection.
2. **Traceroute** — type an address (like `google.com`), hit Start, and
   watch each step of the journey appear, with the location of each step
   when it's known.
3. **Telnet** — type a host and port, hit Connect, and you get a live
   terminal session — everything sent and received is shown, just like the
   classic `telnet` command.
4. **Speed Test** — pick a server from the dropdown (or press `n` to cycle
   through them in the terminal) and hit Start.

A line at the bottom of the terminal window always shows the web address
(and a scannable QR code — press `w` to bring it up) so you can check the
same screens from your phone or another computer on the network.


## Checking for updates

Press `u` in the terminal (or the **Check for Updates** button on the web
page) to check GitHub for a newer release. If one's available, the button
turns highlighted with a pulsing dot — impossible to miss, and it stays
that way even if you dismiss the notification banner — until you install
it or a later check finds nothing newer. Installing downloads it, swaps
netdx out for the new version, and restarts automatically with the same
settings you were already running with.

## Event log

Every screen shows the last 10 log entries at the bottom (older ones are
one scroll away — `↑`/`↓` in the terminal, or just scroll the panel on the
web page). Two actions are always available:

- **Export** (`e` in the terminal, "Export CSV" on the web page) — writes
  the full log, fully timestamped, to a CSV file you can open in Excel or
  attach to a ticket. The log entry announcing the export is itself a
  clickable link straight to the file — a real terminal hyperlink in the
  terminal window (works in Windows Terminal, iTerm2, kitty, WezTerm, and
  most modern terminals), or a download link on the web page. It's saved
  under netdx's application data folder either way:
  `%LOCALAPPDATA%\netdx\logs` on Windows, `~/Library/Application
  Support/netdx/logs` on macOS, `~/.local/share/netdx/logs` on Linux.
- **Clear** (`C` in the terminal, "Clear" on the web page) — empties the
  on-screen log. Doesn't touch any CSV files you've already exported.

## Speed test

netdx tests ping, jitter, packet loss, download, and upload against
[Cloudflare](https://speed.cloudflare.com) — the only public speed-test
endpoint that offers a real upload test (others only publish download-only
test files, so they aren't offered as options here). While a test is
running, the Download/Upload numbers update live and are marked `(live)`;
once that phase finishes they settle to `(avg)` — the average measured
over the whole test, not just the last reading.

## Keyboard shortcuts (terminal window)

| Key | What it does |
| --- | --- |
| `Tab` / `Shift+Tab` or `1`–`4` | Switch screens |
| `Enter` | Start / confirm whatever you're on (start a traceroute, connect telnet, start a speed test) |
| `n` | On the Speed Test screen, cycle to the next server |
| `↑` / `↓` | Scroll the event log further back / forward |
| `e` | Export the event log to CSV |
| `C` | Clear the event log |
| `u` | Check for updates |
| `w` | Show/hide the QR code for the web page |
| `r` | On the Overview screen, refresh your network info |
| `Esc` | Cancel typing / close a popup |
| `q` or `Ctrl+C` | Quit |

## Advanced / IT notes

<details>
<summary>Elevated privileges (why traceroute needs <code>sudo</code> on macOS/Linux)</summary>

**Traceroute** needs to send raw network packets (ICMP) to see each step of
a route:

- **Windows**: no special permissions needed — netdx talks to the same
  underlying Windows networking API `tracert.exe` itself uses.
- **macOS / Linux**: run `sudo netdx`, or grant just the one permission
  netdx needs so you don't have to run the whole program as root:
  ```sh
  sudo setcap cap_net_raw+ep $(which netdx)
  ```

Without that permission, traceroute on macOS/Linux will show a clear
message explaining why instead of hanging. Everything else — the Overview
screen, telnet, the speed test, and the web page — works normally without
any special permissions everywhere, including traceroute on Windows.

</details>

<details>
<summary>Command-line options</summary>

```sh
netdx [OPTIONS]
```

| Flag | Default | Description |
| --- | --- | --- |
| `--no-tui` | off | Don't open the terminal screen — run only as a background web server (handy on a headless machine). |
| `--no-web` | off | Don't start the web page — terminal only. |
| `--web-bind <addr:port>` | `0.0.0.0:7878` | Address and port the web page listens on. |
| `--web-token <token>` | auto-generated | Access code required to open the web page. A random one is generated and printed each time you start netdx unless you set your own. |
| `--no-analytics` | off | Disable anonymous usage analytics (same as `NETDX_NO_ANALYTICS=1`) — see [SUPABASE.md](SUPABASE.md). |
| `-h`, `--help` | — | Show help. |
| `-V`, `--version` | — | Show version. |

</details>

<details>
<summary>Sharing the web page safely</summary>

The web page is meant for your local network. If you want to reach it from
somewhere else:

- **Keep the access code (token) private** — anyone with the full web
  address can use it.
- **Don't expose it directly to the internet.** Use a proper reverse proxy
  with HTTPS, or a private tunnel such as [Tailscale](https://tailscale.com/)
  or [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/).
- If you think the code has leaked, restart netdx (or set a new one with
  `--web-token`).

</details>

<details>
<summary>Building from source</summary>

Requires the [Rust toolchain](https://rustup.rs/).

```sh
cargo install --path .
```

</details>

---

Developed by [MyEvent Labs](https://myevent-labs.io) · MIT License
