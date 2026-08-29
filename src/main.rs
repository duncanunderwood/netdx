mod engine;
mod net;
mod qr;
mod state;
mod ui;
mod web;

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use rand::distributions::Alphanumeric;
use rand::Rng;
use tokio::sync::{mpsc, watch};

use state::Command;

/// netdx — fast, colourblind-friendly terminal network diagnostics: traceroute, telnet, speed
/// test, and ipconfig-style interface stats, with a built-in web UI for remote/browser access.
#[derive(Parser, Debug)]
#[command(name = "netdx", version, about)]
struct Cli {
    /// Skip the local terminal UI (useful when running headless and driving netdx purely
    /// through the web UI, e.g. on a remote box).
    #[arg(long)]
    no_tui: bool,

    /// Don't start the companion web server.
    #[arg(long)]
    no_web: bool,

    /// Address:port for the web UI to listen on.
    #[arg(long, default_value = "0.0.0.0:7878")]
    web_bind: String,

    /// Access token required by the web UI (?token=...). Auto-generated and printed at startup
    /// if not provided.
    #[arg(long)]
    web_token: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.no_tui && cli.no_web {
        anyhow::bail!("--no-tui and --no-web together leave nothing running; drop one of them");
    }

    let shared_state = engine::new_shared_state();
    let (changed_tx, changed_rx) = watch::channel(());
    let (commands_tx, commands_rx) = mpsc::unbounded_channel::<Command>();

    let engine_state = shared_state.clone();
    let engine_changed = changed_tx.clone();
    tokio::spawn(engine::run(engine_state, engine_changed, commands_rx));

    let mut web_url = None;
    let mut web_qr = None;
    if !cli.no_web {
        let bind: SocketAddr = cli
            .web_bind
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid --web-bind '{}': {e}", cli.web_bind))?;
        let token = cli.web_token.unwrap_or_else(generate_token);

        let app = Arc::new(web::WebApp {
            state: shared_state.clone(),
            commands: commands_tx.clone(),
            changed: changed_rx.clone(),
            token: token.clone(),
        });

        let display_host = if bind.ip().is_unspecified() {
            local_ip_hint().unwrap_or_else(|| "127.0.0.1".to_string())
        } else {
            bind.ip().to_string()
        };
        let url = format!("http://{display_host}:{}/?token={token}", bind.port());
        web_url = Some(url.clone());
        web_qr = qr::terminal_qr(&url);

        println!("netdx web UI:");
        println!("  {url}");
        if bind.ip().is_unspecified() {
            println!("  (bound on {}, reachable from any interface on this machine)", cli.web_bind);
        }
        println!("  Keep the token secret. For internet access, prefer a TLS reverse proxy or a tunnel");
        println!("  (Tailscale / Cloudflare Tunnel / ngrok) over raw port-forwarding.");
        if let Some(qr_text) = &web_qr {
            println!();
            println!("  Scan with a phone camera:");
            println!();
            for line in qr_text.lines() {
                println!("  {line}");
            }
        }
        println!();

        tokio::spawn(async move {
            if let Err(e) = web::serve(bind, app).await {
                eprintln!("web server error: {e:#}");
            }
        });
    }

    if cli.no_tui {
        println!("Running headless (--no-tui). Press Ctrl+C to exit.");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    ui::run(shared_state, changed_rx, commands_tx, web_url, web_qr).await
}

fn generate_token() -> String {
    rand::thread_rng().sample_iter(&Alphanumeric).take(24).map(char::from).collect()
}

/// Best-effort LAN IP to show in the printed URL when binding to 0.0.0.0/::.
fn local_ip_hint() -> Option<String> {
    net::interfaces::snapshot()
        .interfaces
        .into_iter()
        .find(|i| i.is_default)
        .and_then(|i| i.ipv4.first().map(|e| e.addr.clone()))
}
