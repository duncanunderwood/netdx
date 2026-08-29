//! Companion web UI: serves a single-page app and a `/ws` endpoint that mirrors the same
//! `AppState`/`Command` contract the TUI uses, so a phone or laptop on the LAN (or over the
//! internet, via port-forward/tunnel) can drive and watch diagnostics in real time.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::engine::SharedState;
use crate::state::{AppState, Command};

const INDEX_HTML: &str = include_str!("static/index.html");

pub struct WebApp {
    pub state: SharedState,
    pub commands: mpsc::UnboundedSender<Command>,
    pub changed: watch::Receiver<()>,
    pub token: String,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

#[derive(Serialize)]
struct StateMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    state: &'a AppState,
}

pub async fn serve(bind: SocketAddr, app: Arc<WebApp>) -> anyhow::Result<()> {
    let router = Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .route("/exports/{filename}", get(export_download_handler))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

fn authorized(app: &WebApp, query: &TokenQuery) -> bool {
    query.token.as_deref() == Some(app.token.as_str())
}

async fn index_handler(State(app): State<Arc<WebApp>>, Query(q): Query<TokenQuery>) -> Response {
    if !authorized(&app, &q) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid ?token=").into_response();
    }
    Html(INDEX_HTML).into_response()
}

async fn ws_handler(
    State(app): State<Arc<WebApp>>,
    Query(q): Query<TokenQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !authorized(&app, &q) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid ?token=").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, app))
}

/// Serves a previously-exported event-log CSV for download — the file itself already exists
/// on disk (written synchronously by the `log_export` command); this just streams it back with
/// a `Content-Disposition` so a browser click downloads/opens it instead of navigating to raw
/// CSV text. Filenames are restricted to netdx's own generated pattern
/// (`netdx-log-<timestamp>.csv`, alphanumeric/`-`/`.` only) so this can never be tricked into
/// serving an arbitrary path off the filesystem.
async fn export_download_handler(
    State(app): State<Arc<WebApp>>,
    Path(filename): Path<String>,
    Query(q): Query<TokenQuery>,
) -> Response {
    if !authorized(&app, &q) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid ?token=").into_response();
    }
    if !is_safe_export_filename(&filename) {
        return (StatusCode::BAD_REQUEST, "invalid filename").into_response();
    }
    let path = crate::eventlog::logs_dir().join(&filename);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "text/csv".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "export not found — it may have been moved or deleted").into_response(),
    }
}

fn is_safe_export_filename(name: &str) -> bool {
    name.starts_with("netdx-log-")
        && name.ends_with(".csv")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

async fn handle_socket(socket: WebSocket, app: Arc<WebApp>) {
    let (mut sender, mut receiver) = socket.split();
    let mut changed_rx = app.changed.clone();

    if send_snapshot(&mut sender, &app.state).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            changed = changed_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                if send_snapshot(&mut sender, &app.state).await.is_err() {
                    break;
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(cmd) = serde_json::from_str::<Command>(text.as_str()) {
                            let _ = app.commands.send(cmd);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn send_snapshot(
    sender: &mut SplitSink<WebSocket, Message>,
    state: &SharedState,
) -> Result<(), axum::Error> {
    let json = {
        let st = state.read();
        serde_json::to_string(&StateMessage { kind: "state", state: &st })
    };
    match json {
        Ok(text) => sender.send(Message::text(text)).await,
        Err(_) => Ok(()),
    }
}
