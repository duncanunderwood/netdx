//! Renders a URL as a compact terminal QR code (stacked half-block Unicode characters, 2 modules
//! per character row — the same style tools like `qrencode -t UTF8` produce), so a phone camera
//! can scan straight off the terminal without anyone typing the address or token by hand.

use qrcode::render::unicode;
use qrcode::QrCode;

/// Returns `None` if `data` can't be encoded (e.g. absurdly long input exceeding the largest QR
/// version) — callers should just skip printing a QR code in that case, not fail startup over it.
pub fn terminal_qr(data: &str) -> Option<String> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    Some(code.render::<unicode::Dense1x2>().build())
}
