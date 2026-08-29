pub mod app;
pub mod render;
pub mod theme;

use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};

use crate::engine::SharedState;
use crate::state::Command;
use app::App;

/// Drives the local terminal UI: redraws on a short tick or whenever the shared engine state
/// changes, and forwards key input into `App`, which turns it into `Command`s on the same
/// channel the web UI uses.
pub async fn run(
    state: SharedState,
    mut changed_rx: watch::Receiver<()>,
    commands: mpsc::UnboundedSender<Command>,
    web_url: Option<String>,
    qr: Option<String>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new(web_url, qr);
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(200));

    let result: Result<()> = loop {
        let snapshot = state.read().clone();
        let mut links = Vec::new();
        if let Err(e) = terminal.draw(|frame| {
            links = render::draw(frame, &app, &snapshot);
        }) {
            break Err(e.into());
        }
        // ratatui has no concept of a clickable link, so real OSC 8 hyperlinks (the footer
        // credit, and any event-log row announcing a CSV export) are written directly to the
        // terminal after each frame, on top of the plain text ratatui just drew — self-healing
        // every frame regardless of ratatui's internal diffing/redraw behavior. Skipped while
        // the QR/update popups may be covering those rows (`render::draw` already returns an
        // empty `links` list in that case; the footer check is separate since the footer sits
        // outside the log panel area).
        if !app.show_qr {
            if let Ok(size) = terminal.size() {
                if size.height > 0 {
                    if let Some(col) = render::footer_link_col(size.width) {
                        let _ = write_footer_hyperlink(col, size.height - 1);
                    }
                }
            }
        }
        for link in &links {
            let _ = write_hyperlink(link.col, link.row, &link.text, &link.url);
        }

        tokio::select! {
            _ = ticker.tick() => {}
            _ = changed_rx.changed() => {}
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        app.on_key(key, &commands);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break Err(e.into()),
                    None => break Ok(()),
                }
            }
        }

        if app.should_quit {
            break Ok(());
        }
    };

    ratatui::restore();
    result
}

/// Writes the "myevent-labs.io" footer text as a real OSC 8 terminal hyperlink at `(col, row)`.
/// A no-op-looking sequence of bytes on terminals that don't understand OSC 8 (they just show
/// the label, matching what ratatui already drew there) — a real clickable link on ones that do
/// (Windows Terminal, iTerm2, kitty, WezTerm, GNOME Terminal, and others).
fn write_footer_hyperlink(col: u16, row: u16) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    execute!(stdout, MoveTo(col, row), SetForegroundColor(Color::Rgb { r: 0x90, g: 0x90, b: 0x90 }))?;
    write!(stdout, "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", render::FOOTER_LINK_URL, render::FOOTER_LINK_LABEL)?;
    execute!(stdout, ResetColor)?;
    stdout.flush()
}

/// Writes `text` as a real OSC 8 hyperlink to `url` at `(col, row)` — used for event-log rows
/// that announce a CSV export, so clicking the row opens the file directly (terminal support
/// permitting; same graceful-degradation story as `write_footer_hyperlink`).
fn write_hyperlink(col: u16, row: u16, text: &str, url: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    execute!(stdout, MoveTo(col, row), SetForegroundColor(Color::Rgb { r: 0x56, g: 0xB4, b: 0xE9 }))?;
    write!(stdout, "\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")?;
    execute!(stdout, ResetColor)?;
    stdout.flush()
}
