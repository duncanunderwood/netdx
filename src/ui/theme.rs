//! Okabe-Ito colourblind-safe palette, shared in spirit with the web UI's CSS custom properties.
//! Every status indicator pairs colour with a text/icon label elsewhere in the UI — never hue
//! alone — so the same rules apply here.

use ratatui::style::{Color, Modifier, Style};

pub const OK: Color = Color::Rgb(0x00, 0x9E, 0x73); // bluish green — success / up
pub const WARN: Color = Color::Rgb(0xE6, 0x9F, 0x00); // orange — warning / degraded
pub const BAD: Color = Color::Rgb(0xD5, 0x5E, 0x00); // vermillion — error / down (never pure red)
pub const INFO: Color = Color::Rgb(0x00, 0x72, 0xB2); // blue — informational / accent
pub const ACCENT2: Color = Color::Rgb(0x56, 0xB4, 0xE9); // sky blue — secondary accent
pub const ACCENT3: Color = Color::Rgb(0xCC, 0x79, 0xA7); // reddish purple — tertiary accent
pub const FG: Color = Color::Rgb(0xE8, 0xE8, 0xE8);
pub const MUTED: Color = Color::Rgb(0x90, 0x90, 0x90);

pub fn title_style() -> Style {
    Style::default().fg(INFO).add_modifier(Modifier::BOLD)
}
pub fn ok_style() -> Style {
    Style::default().fg(OK).add_modifier(Modifier::BOLD)
}
pub fn warn_style() -> Style {
    Style::default().fg(WARN).add_modifier(Modifier::BOLD)
}
pub fn bad_style() -> Style {
    Style::default().fg(BAD).add_modifier(Modifier::BOLD)
}
pub fn muted_style() -> Style {
    Style::default().fg(MUTED)
}
pub fn fg_style() -> Style {
    Style::default().fg(FG)
}
pub fn accent2_style() -> Style {
    Style::default().fg(ACCENT2)
}
pub fn active_tab_style() -> Style {
    Style::default().fg(Color::Black).bg(INFO).add_modifier(Modifier::BOLD)
}
pub fn inactive_tab_style() -> Style {
    Style::default().fg(MUTED)
}
pub fn editing_style() -> Style {
    Style::default().fg(Color::Black).bg(WARN)
}
