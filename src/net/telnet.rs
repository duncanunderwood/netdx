//! Minimal RFC 854 telnet client: connects over TCP, negotiates every option away (WONT/DONT),
//! and otherwise passes bytes through as plain text. This is intentionally not a full option
//! negotiator (no NAWS/terminal-type/etc.) — enough for techs to reach a device's telnet banner
//! and CLI, which is the common diagnostic use case.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const IAC: u8 = 255;
const WILL: u8 = 251;
const WONT: u8 = 252;
const DO: u8 = 253;
const DONT: u8 = 254;
const SB: u8 = 250;
const SE: u8 = 240;

#[derive(Debug)]
pub enum TelnetEvent {
    Connected,
    Data(String),
    Error(String),
    Disconnected,
}

#[derive(Default)]
enum DecodeState {
    #[default]
    Normal,
    Iac,
    Command(u8),
    Subneg,
    SubnegIac,
}

/// Strips/answers telnet IAC negotiation, passing everything else through as text.
#[derive(Default)]
struct TelnetCodec {
    state: DecodeState,
}

impl TelnetCodec {
    fn feed(&mut self, chunk: &[u8]) -> (String, Vec<u8>) {
        let mut payload = Vec::with_capacity(chunk.len());
        let mut reply = Vec::new();

        for &b in chunk {
            match self.state {
                DecodeState::Normal => {
                    if b == IAC {
                        self.state = DecodeState::Iac;
                    } else {
                        payload.push(b);
                    }
                }
                DecodeState::Iac => match b {
                    IAC => {
                        payload.push(IAC);
                        self.state = DecodeState::Normal;
                    }
                    WILL | WONT | DO | DONT => self.state = DecodeState::Command(b),
                    SB => self.state = DecodeState::Subneg,
                    _ => self.state = DecodeState::Normal,
                },
                DecodeState::Command(cmd) => {
                    // Refuse every option: mirror DO->WONT and WILL->DONT, ignore the rest.
                    match cmd {
                        DO => reply.extend_from_slice(&[IAC, WONT, b]),
                        WILL => reply.extend_from_slice(&[IAC, DONT, b]),
                        _ => {}
                    }
                    self.state = DecodeState::Normal;
                }
                DecodeState::Subneg => {
                    if b == IAC {
                        self.state = DecodeState::SubnegIac;
                    }
                }
                DecodeState::SubnegIac => {
                    self.state = if b == SE {
                        DecodeState::Normal
                    } else {
                        DecodeState::Subneg
                    };
                }
            }
        }

        (String::from_utf8_lossy(&payload).into_owned(), reply)
    }
}

/// Owns the TCP connection for the lifetime of a telnet session: reads server output (decoding
/// telnet negotiation), answers negotiation requests, and forwards outbound bytes from
/// `input_rx`. Runs until the peer closes the connection, an I/O error occurs, or `input_rx` is
/// closed (used as the disconnect signal).
pub async fn run(
    host: String,
    port: u16,
    events: mpsc::UnboundedSender<TelnetEvent>,
    mut input_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let stream = match TcpStream::connect((host.as_str(), port)).await {
        Ok(s) => s,
        Err(e) => {
            let _ = events.send(TelnetEvent::Error(format!("connect failed: {e}")));
            return;
        }
    };
    let _ = events.send(TelnetEvent::Connected);

    let (mut rd, mut wr) = stream.into_split();
    let mut codec = TelnetCodec::default();
    let mut buf = [0u8; 4096];

    loop {
        tokio::select! {
            res = rd.read(&mut buf) => {
                match res {
                    Ok(0) => {
                        let _ = events.send(TelnetEvent::Disconnected);
                        return;
                    }
                    Ok(n) => {
                        let (text, reply) = codec.feed(&buf[..n]);
                        if !reply.is_empty() && wr.write_all(&reply).await.is_err() {
                            let _ = events.send(TelnetEvent::Disconnected);
                            return;
                        }
                        if !text.is_empty() {
                            let _ = events.send(TelnetEvent::Data(text));
                        }
                    }
                    Err(e) => {
                        let _ = events.send(TelnetEvent::Error(e.to_string()));
                        return;
                    }
                }
            }
            input = input_rx.recv() => {
                match input {
                    Some(bytes) => {
                        if wr.write_all(&bytes).await.is_err() {
                            let _ = events.send(TelnetEvent::Disconnected);
                            return;
                        }
                    }
                    None => return,
                }
            }
        }
    }
}
