use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Cell, Chart, Clear, Dataset, GraphType, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::state::AppState;
use crate::ui::app::{App, Mode, Tab};
use crate::ui::theme;

pub fn draw(frame: &mut Frame, app: &App, state: &AppState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    draw_tabs(frame, app, chunks[0]);

    match app.tab {
        Tab::Overview => draw_overview(frame, state, chunks[1]),
        Tab::Traceroute => draw_traceroute(frame, app, state, chunks[1]),
        Tab::Telnet => draw_telnet(frame, app, state, chunks[1]),
        Tab::Speedtest => draw_speedtest(frame, app, state, chunks[1]),
    }

    draw_status(frame, app, state, chunks[2]);
    draw_footer(frame, chunks[3]);

    if app.show_qr {
        draw_qr_popup(frame, app, area);
    }
}

pub const FOOTER_PREFIX: &str = "Developed by MyEvent Labs — ";
pub const FOOTER_LINK_LABEL: &str = "myevent-labs.io";
pub const FOOTER_LINK_URL: &str = "https://myevent-labs.io";

/// Column where `FOOTER_LINK_LABEL` starts when the footer line is centered in a row of the
/// given `width`, or `None` if it doesn't fit. Used by the render loop to overlay a real OSC 8
/// terminal hyperlink on top of the plain text this module draws (ratatui has no native concept
/// of a clickable link, so that overlay happens outside ratatui's `Buffer`).
pub fn footer_link_col(width: u16) -> Option<u16> {
    let total = (FOOTER_PREFIX.width() + FOOTER_LINK_LABEL.width()) as u16;
    if width < total {
        return None;
    }
    Some((width - total) / 2 + FOOTER_PREFIX.width() as u16)
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    let line = Line::from(Span::styled(format!("{FOOTER_PREFIX}{FOOTER_LINK_LABEL}"), theme::muted_style()))
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(Paragraph::new(line), area);
}




/// Centers a fixed-size box within `area`, clamped so it never exceeds the available space.
fn centered_fixed_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width, height }
}

fn draw_qr_popup(frame: &mut Frame, app: &App, area: Rect) {
    let Some(qr) = &app.qr else { return };
    let qr_width = qr.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
    let qr_height = qr.lines().count() as u16;

    let url_line = app.web_url.clone().unwrap_or_default();
    let box_width = qr_width.max(url_line.chars().count() as u16).saturating_add(4);
    let box_height = qr_height.saturating_add(5);

    let popup = centered_fixed_rect(box_width, box_height, area);
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = qr.lines().map(|l| Line::from(l.to_string())).collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(url_line, theme::accent2_style())));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Scan to open netdx on your phone — Esc/w to close ")
        .style(theme::fg_style());
    let paragraph = Paragraph::new(lines).block(block).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(paragraph, popup);
}


fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.title())).collect();
    let idx = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let title = match &app.web_url {
        Some(url) => format!(" netdx — remote UI: {url} "),
        None => " netdx — remote web UI disabled ".to_string(),
    };
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(Span::styled(title, theme::title_style())))
        .select(idx)
        .style(theme::inactive_tab_style())
        .highlight_style(theme::active_tab_style())
        .divider(" ");
    frame.render_widget(tabs, area);
}

fn draw_status(frame: &mut Frame, app: &App, state: &AppState, area: Rect) {
    let hint = app.status_hint();
    let last_log = state.log.back().cloned().unwrap_or_default();
    let line = Line::from(vec![
        Span::styled(hint, theme::muted_style()),
        Span::raw("   "),
        Span::styled(last_log, theme::muted_style()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn status_badge(up: bool) -> Span<'static> {
    if up {
        Span::styled("\u{25CF} UP", theme::ok_style())
    } else {
        Span::styled("\u{25CB} DOWN", theme::bad_style())
    }
}

fn draw_overview(frame: &mut Frame, state: &AppState, area: Rect) {
    let net = &state.network;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let summary = Line::from(vec![
        Span::styled("Default interface: ", theme::muted_style()),
        Span::styled(
            net.default_interface.clone().unwrap_or_else(|| "-".to_string()),
            theme::title_style(),
        ),
        Span::raw("    "),
        Span::styled("Public IP: ", theme::muted_style()),
        Span::styled(net.public_ip.clone().unwrap_or_else(|| "looking up…".to_string()), theme::accent2_style()),
    ]);
    frame.render_widget(
        Paragraph::new(summary).block(Block::default().borders(Borders::ALL).title(" This Machine ")),
        chunks[0],
    );

    let header = Row::new(vec!["", "Interface", "Type", "IPv4 / IPv6", "Gateway", "DNS", "MAC", "MTU"])
        .style(theme::muted_style());
    let rows: Vec<Row> = net
        .interfaces
        .iter()
        .map(|i| {
            let mut name = i.display_name.clone();
            if i.is_default {
                name.push_str(" *");
            }
            let mut ips: Vec<String> = i.ipv4.iter().map(|e| format!("{}/{}", e.addr, e.prefix_len)).collect();
            ips.extend(i.ipv6.iter().map(|e| format!("{}/{}", e.addr, e.prefix_len)));
            let ips = if ips.is_empty() { "-".to_string() } else { ips.join(", ") };
            let dns = if i.dns_servers.is_empty() { "-".to_string() } else { i.dns_servers.join(", ") };

            Row::new(vec![
                Cell::from(status_badge(i.is_up)),
                Cell::from(name),
                Cell::from(i.if_type.clone()),
                Cell::from(ips),
                Cell::from(i.gateway.clone().unwrap_or_else(|| "-".to_string())),
                Cell::from(dns),
                Cell::from(i.mac.clone().unwrap_or_else(|| "-".to_string())),
                Cell::from(i.mtu.map(|m| m.to_string()).unwrap_or_else(|| "-".to_string())),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(18),
        Constraint::Length(12),
        Constraint::Length(28),
        Constraint::Length(15),
        Constraint::Length(24),
        Constraint::Length(17),
        Constraint::Length(6),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Network Interfaces (ipconfig / ifconfig) — r to refresh "));
    frame.render_widget(table, chunks[1]);
}

fn draw_traceroute(frame: &mut Frame, app: &App, state: &AppState, area: Rect) {
    let tr = &state.traceroute;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let input_line = if app.mode == Mode::EditTraceroute {
        Line::from(Span::styled(format!("target> {}_", app.traceroute_input), theme::editing_style()))
    } else {
        let mut spans = vec![
            Span::styled("target: ", theme::muted_style()),
            Span::raw(if tr.target.is_empty() { "(none — press Enter to set)".to_string() } else { tr.target.clone() }),
        ];
        if let Some(ip) = &tr.resolved_ip {
            spans.push(Span::raw("  →  "));
            spans.push(Span::styled(ip.clone(), theme::title_style()));
        }
        if tr.running {
            spans.push(Span::raw("   "));
            spans.push(Span::styled("probing…", theme::warn_style()));
        } else if tr.done && tr.error.is_none() {
            spans.push(Span::raw("   "));
            spans.push(Span::styled("done", theme::ok_style()));
        }
        if let Some(err) = &tr.error {
            spans.push(Span::raw("   "));
            spans.push(Span::styled(format!("error: {err}"), theme::bad_style()));
        }
        Line::from(spans)
    };
    frame.render_widget(
        Paragraph::new(input_line).block(Block::default().borders(Borders::ALL).title(" Traceroute ")),
        chunks[0],
    );

    let header = Row::new(vec!["TTL", "Address", "Hostname", "Location", "RTT"]).style(theme::muted_style());
    let rows: Vec<Row> = tr
        .hops
        .iter()
        .map(|h| {
            let (addr, style) = if h.timeout {
                ("* * *".to_string(), theme::muted_style())
            } else {
                (h.addr.clone().unwrap_or_else(|| "-".to_string()), theme::fg_style())
            };
            let hostname = h.hostname.clone().unwrap_or_else(|| "-".to_string());
            let location = match (&h.city, &h.country) {
                (Some(city), Some(country)) => format!("{city}, {country}"),
                (Some(city), None) => city.clone(),
                (None, Some(country)) => country.clone(),
                (None, None) => "-".to_string(),
            };
            let rtt = h.rtt_ms.map(|r| format!("{r:.1} ms")).unwrap_or_else(|| "-".to_string());
            Row::new(vec![
                Cell::from(h.ttl.to_string()),
                Cell::from(Span::styled(addr, style)),
                Cell::from(hostname),
                Cell::from(location),
                Cell::from(rtt),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(5),
        Constraint::Length(16),
        Constraint::Length(28),
        Constraint::Length(20),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Hops "));
    frame.render_widget(table, chunks[1]);
}

fn draw_telnet(frame: &mut Frame, app: &App, state: &AppState, area: Rect) {
    let tn = &state.telnet;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let status_line = if app.mode == Mode::EditTelnetConnect {
        Line::from(Span::styled(format!("host:port> {}_", app.telnet_connect_input), theme::editing_style()))
    } else {
        let mut spans = vec![Span::styled("target: ", theme::muted_style()), Span::raw(format!("{}:{}", tn.host, tn.port))];
        spans.push(Span::raw("   "));
        if tn.connected {
            spans.push(Span::styled("● connected", theme::ok_style()));
        } else if tn.connecting {
            spans.push(Span::styled("connecting…", theme::warn_style()));
        } else {
            spans.push(Span::styled("○ disconnected", theme::bad_style()));
        }
        if let Some(err) = &tn.error {
            spans.push(Span::raw("   "));
            spans.push(Span::styled(err.clone(), theme::bad_style()));
        }
        Line::from(spans)
    };
    frame.render_widget(
        Paragraph::new(status_line).block(Block::default().borders(Borders::ALL).title(" Telnet ")),
        chunks[0],
    );

    let text: Vec<Line> = tn.buffer.lines().map(|l| Line::from(l.to_string())).collect();
    let visible_rows = chunks[1].height.saturating_sub(2) as usize;
    let scroll = text.len().saturating_sub(visible_rows) as u16;
    let session = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" Session "))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(session, chunks[1]);

    let input_line = if app.mode == Mode::EditTelnetSend {
        Line::from(Span::styled(format!("> {}_", app.telnet_send_input), theme::editing_style()))
    } else {
        Line::from(Span::styled("press i to type, d to disconnect", theme::muted_style()))
    };
    frame.render_widget(
        Paragraph::new(input_line).block(Block::default().borders(Borders::ALL).title(" Send ")),
        chunks[2],
    );
}

fn draw_speedtest(frame: &mut Frame, app: &App, state: &AppState, area: Rect) {
    let sp = &state.speedtest;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(7), Constraint::Length(7), Constraint::Min(0)])
        .split(area);

    let stage_style = match sp.stage.as_str() {
        "done" => theme::ok_style(),
        "error" => theme::bad_style(),
        "idle" => theme::muted_style(),
        _ => theme::warn_style(),
    };
    let server_label = if sp.server.is_empty() {
        app.speedtest_server.label().to_string()
    } else {
        sp.server.clone()
    };
    let supports_upload = sp
        .available_servers
        .iter()
        .find(|s| s.id == app.speedtest_server.id_str())
        .map(|s| s.supports_upload)
        .unwrap_or(true);
    let mut summary = vec![
        Span::styled("stage: ", theme::muted_style()),
        Span::styled(sp.stage.clone(), stage_style),
        Span::raw("   "),
        Span::styled("server: ", theme::muted_style()),
        Span::styled(server_label, theme::accent2_style()),
        Span::raw("  (n to cycle)"),
    ];
    if let Some(err) = &sp.error {
        summary.push(Span::raw("   "));
        summary.push(Span::styled(format!("error: {err}"), theme::bad_style()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(summary)).block(Block::default().borders(Borders::ALL).title(" Speed Test — Enter to start, x to stop ")),
        chunks[0],
    );

    let dl_points: Vec<(f64, f64)> = sp.download_samples.iter().enumerate().map(|(i, v)| (i as f64, *v)).collect();
    let dl_title = format!(
        " Download {} ",
        sp.download_mbps.map(|m| format!("{m:.1} Mbps avg")).unwrap_or_else(|| "…".to_string())
    );
    frame.render_widget(braille_chart(dl_title, &sp.download_samples, &dl_points, theme::INFO), chunks[1]);

    if supports_upload {
        let ul_points: Vec<(f64, f64)> = sp.upload_samples.iter().enumerate().map(|(i, v)| (i as f64, *v)).collect();
        let ul_title = format!(
            " Upload {} ",
            sp.upload_mbps.map(|m| format!("{m:.1} Mbps avg")).unwrap_or_else(|| "…".to_string())
        );
        frame.render_widget(braille_chart(ul_title, &sp.upload_samples, &ul_points, theme::ACCENT3), chunks[2]);
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "not supported by this server — pick Cloudflare for an upload test",
                theme::muted_style(),
            )))
            .block(Block::default().borders(Borders::ALL).title(" Upload ")),
            chunks[2],
        );
    }

    let stats = Line::from(vec![
        Span::styled("ping: ", theme::muted_style()),
        Span::raw(sp.ping_ms.map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "-".to_string())),
        Span::raw("    "),
        Span::styled("jitter: ", theme::muted_style()),
        Span::raw(sp.jitter_ms.map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "-".to_string())),
        Span::raw("    "),
        Span::styled("loss: ", theme::muted_style()),
        Span::raw(sp.packet_loss_pct.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "-".to_string())),
    ]);
    frame.render_widget(Paragraph::new(stats).block(Block::default().borders(Borders::ALL).title(" Latency ")), chunks[3]);
}

/// A high-resolution line chart using Unicode Braille Patterns (2x4 sub-cell dots per terminal
/// cell) instead of `Sparkline`'s one-block-per-cell bars — much smoother for a Mbps-over-time
/// trace at typical terminal sizes.
fn braille_chart<'a>(title: String, samples: &[f64], points: &'a [(f64, f64)], color: Color) -> Chart<'a> {
    let y_max = samples.iter().cloned().fold(0.0_f64, f64::max).max(1.0) * 1.15;
    let y_mid = y_max / 2.0;
    let x_max = samples.len().saturating_sub(1).max(1) as f64;

    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(color))
        .data(points);

    Chart::new(vec![dataset])
        .block(Block::default().borders(Borders::ALL).title(title))
        .x_axis(Axis::default().bounds([0.0, x_max]))
        .y_axis(
            Axis::default()
                .style(theme::muted_style())
                .bounds([0.0, y_max])
                .labels([format!("{:.0}", 0.0), format!("{y_mid:.0}"), format!("{y_max:.0}")]),
        )
}
