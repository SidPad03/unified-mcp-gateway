use ratatui::style::{Color, Modifier, Style};

pub const BRAND_PRIMARY: Color = Color::Cyan;
pub const BRAND_ACCENT: Color = Color::Magenta;
pub const STATUS_OK: Color = Color::Green;
pub const STATUS_WARN: Color = Color::Yellow;
pub const STATUS_ERR: Color = Color::Red;
pub const TEXT_DIM: Color = Color::DarkGray;
pub const TEXT_NORMAL: Color = Color::White;
pub const BORDER: Color = Color::DarkGray;

pub fn title_style() -> Style {
    Style::default().fg(BRAND_PRIMARY).add_modifier(Modifier::BOLD)
}

pub fn status_ok() -> Style {
    Style::default().fg(STATUS_OK).add_modifier(Modifier::BOLD)
}

pub fn status_warn() -> Style {
    Style::default().fg(STATUS_WARN).add_modifier(Modifier::BOLD)
}

pub fn status_err() -> Style {
    Style::default().fg(STATUS_ERR).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn normal() -> Style {
    Style::default().fg(TEXT_NORMAL)
}

pub fn border() -> Style {
    Style::default().fg(BORDER)
}

pub fn highlight() -> Style {
    Style::default().fg(BRAND_ACCENT).add_modifier(Modifier::BOLD)
}

pub const ASCII_LOGO: &str = r#"  __  __  ___ ___    ___      _
 |  \/  |/ __| _ \  / __|__ _| |_ _____ __ ____ _ _  _
 | |\/| | (__|  _/ | (_ / _` |  _/ -_) V  V / _` | || |
 |_|  |_|\___|_|    \___\__,_|\__\___|\_/\_/\__,_|\_, |
                                                   |__/"#;
