use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::net::speedtest::ServerId;
use crate::state::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Traceroute,
    Telnet,
    Speedtest,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Overview, Tab::Traceroute, Tab::Telnet, Tab::Speedtest];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "1 Overview",
            Tab::Traceroute => "2 Traceroute",
            Tab::Telnet => "3 Telnet",
            Tab::Speedtest => "4 Speed Test",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap()
    }

    fn from_index(i: usize) -> Tab {
        Self::ALL[i % Self::ALL.len()]
    }

    pub fn next(self) -> Tab {
        Self::from_index(self.index() + 1)
    }

    pub fn prev(self) -> Tab {
        Self::from_index(self.index() + Self::ALL.len() - 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    EditTraceroute,
    EditTelnetConnect,
    EditTelnetSend,
}

pub struct App {
    pub tab: Tab,
    pub mode: Mode,
    pub traceroute_input: String,
    pub telnet_connect_input: String,
    pub telnet_send_input: String,
    pub speedtest_server: ServerId,
    pub should_quit: bool,
    pub web_url: Option<String>,
    pub qr: Option<String>,
    pub show_qr: bool,
}

impl App {
    pub fn new(web_url: Option<String>, qr: Option<String>) -> Self {
        Self {
            tab: Tab::Overview,
            mode: Mode::Normal,
            traceroute_input: String::new(),
            telnet_connect_input: "192.168.1.1:23".to_string(),
            telnet_send_input: String::new(),
            speedtest_server: ServerId::default(),
            should_quit: false,
            web_url,
            qr,
            show_qr: false,
        }
    }


    pub fn status_hint(&self) -> String {
        let base = match self.mode {
            Mode::Normal => match self.tab {
                Tab::Overview => "r refresh · Tab switch · w web link QR · q quit".to_string(),
                Tab::Traceroute => "Enter start traceroute · s stop · Tab switch · w web link QR · q quit".to_string(),
                Tab::Telnet => "Enter connect · d disconnect · i type · Tab switch · w web link QR · q quit".to_string(),
                Tab::Speedtest => format!(
                    "Enter start speed test · s stop · n server ({}) · Tab switch · w web link QR · q quit",
                    self.speedtest_server.label()
                ),
            },
            Mode::EditTraceroute | Mode::EditTelnetConnect => "type target, Enter to confirm, Esc to cancel".to_string(),
            Mode::EditTelnetSend => "type a line, Enter to send, Esc to stop typing".to_string(),
        };
        if self.mode == Mode::Normal && self.show_qr {
            format!("{base}  (Esc/w to close QR)")
        } else {
            base
        }
    }

    /// Handles one key event. Returns true if the key was consumed (caller shouldn't do
    /// anything else with it).
    pub fn on_key(&mut self, key: KeyEvent, commands: &mpsc::UnboundedSender<Command>) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }
        if self.mode == Mode::Normal && self.show_qr && key.code == KeyCode::Esc {
            self.show_qr = false;
            return;
        }

        match self.mode {
            Mode::Normal => self.on_key_normal(key, commands),
            Mode::EditTraceroute => self.on_key_edit_traceroute(key, commands),
            Mode::EditTelnetConnect => self.on_key_edit_telnet_connect(key, commands),
            Mode::EditTelnetSend => self.on_key_edit_telnet_send(key, commands),
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent, commands: &mpsc::UnboundedSender<Command>) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('w') => self.show_qr = !self.show_qr,
            KeyCode::Tab | KeyCode::Right => self.tab = self.tab.next(),
            KeyCode::BackTab | KeyCode::Left => self.tab = self.tab.prev(),
            KeyCode::Char('1') => self.tab = Tab::Overview,
            KeyCode::Char('2') => self.tab = Tab::Traceroute,
            KeyCode::Char('3') => self.tab = Tab::Telnet,
            KeyCode::Char('4') => self.tab = Tab::Speedtest,
            KeyCode::Char('r') if self.tab == Tab::Overview => {
                let _ = commands.send(Command::RefreshInterfaces);
            }
            KeyCode::Enter if self.tab == Tab::Traceroute => {
                self.mode = Mode::EditTraceroute;
            }
            KeyCode::Char('s') if self.tab == Tab::Traceroute => {
                let _ = commands.send(Command::TracerouteStop);
            }
            KeyCode::Enter if self.tab == Tab::Telnet => {
                self.mode = Mode::EditTelnetConnect;
            }
            KeyCode::Char('d') if self.tab == Tab::Telnet => {
                let _ = commands.send(Command::TelnetDisconnect);
            }
            KeyCode::Char('i') if self.tab == Tab::Telnet => {
                self.mode = Mode::EditTelnetSend;
            }
            KeyCode::Char('n') if self.tab == Tab::Speedtest => {
                self.speedtest_server = self.speedtest_server.next();
            }
            KeyCode::Enter | KeyCode::Char('s') if self.tab == Tab::Speedtest => {
                let _ = commands.send(Command::SpeedtestStart {
                    server: Some(self.speedtest_server.id_str().to_string()),
                });
            }
            KeyCode::Char('x') if self.tab == Tab::Speedtest => {
                let _ = commands.send(Command::SpeedtestStop);
            }
            _ => {}
        }
    }


    fn on_key_edit_traceroute(&mut self, key: KeyEvent, commands: &mpsc::UnboundedSender<Command>) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let target = self.traceroute_input.trim().to_string();
                if !target.is_empty() {
                    let _ = commands.send(Command::TracerouteStart { target, max_hops: 30 });
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.traceroute_input.pop();
            }
            KeyCode::Char(c) => self.traceroute_input.push(c),
            _ => {}
        }
    }

    fn on_key_edit_telnet_connect(&mut self, key: KeyEvent, commands: &mpsc::UnboundedSender<Command>) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let raw = self.telnet_connect_input.trim().to_string();
                if let Some((host, port)) = parse_host_port(&raw) {
                    let _ = commands.send(Command::TelnetConnect { host, port });
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.telnet_connect_input.pop();
            }
            KeyCode::Char(c) => self.telnet_connect_input.push(c),
            _ => {}
        }
    }

    fn on_key_edit_telnet_send(&mut self, key: KeyEvent, commands: &mpsc::UnboundedSender<Command>) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let data = std::mem::take(&mut self.telnet_send_input);
                if !data.is_empty() {
                    let _ = commands.send(Command::TelnetSend { data });
                }
                // stay in send mode so a session feels like a live terminal
            }
            KeyCode::Backspace => {
                self.telnet_send_input.pop();
            }
            KeyCode::Char(c) => self.telnet_send_input.push(c),
            _ => {}
        }
    }
}

fn parse_host_port(raw: &str) -> Option<(String, u16)> {
    match raw.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => port.parse::<u16>().ok().map(|p| (host.to_string(), p)),
        _ if !raw.is_empty() => Some((raw.to_string(), 23)),
        _ => None,
    }
}
