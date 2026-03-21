use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::theme;

pub struct TextInput {
    pub label: String,
    pub value: String,
    pub cursor_pos: usize,
    pub masked: bool,
    pub focused: bool,
}

impl TextInput {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            value: String::new(),
            cursor_pos: 0,
            masked: false,
            focused: false,
        }
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self.cursor_pos = value.len();
        self
    }

    pub fn handle_char(&mut self, c: char) {
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    pub fn handle_backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn handle_delete(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.value.remove(self.cursor_pos);
        }
    }

    pub fn handle_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn handle_right(&mut self) {
        if self.cursor_pos < self.value.len() {
            self.cursor_pos += 1;
        }
    }

    pub fn handle_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn handle_end(&mut self) {
        self.cursor_pos = self.value.len();
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let border_style = if self.focused {
            Style::default().fg(theme::BRAND_PRIMARY)
        } else {
            theme::border()
        };

        let display_value = if self.masked {
            "*".repeat(self.value.len())
        } else {
            self.value.clone()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!(" {} ", self.label));

        let paragraph = Paragraph::new(display_value.clone()).block(block);
        f.render_widget(paragraph, area);

        if self.focused {
            let cursor_x = area.x + 1 + self.cursor_pos as u16;
            let cursor_y = area.y + 1;
            if cursor_x < area.x + area.width - 1 {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum ValidationState {
    None,
    Validating,
    Valid,
    Invalid,
}

pub fn validation_indicator(state: ValidationState) -> Span<'static> {
    match state {
        ValidationState::None => Span::raw(""),
        ValidationState::Validating => Span::styled("  validating...", theme::dim()),
        ValidationState::Valid => Span::styled("  OK", theme::status_ok()),
        ValidationState::Invalid => Span::styled("  FAILED", theme::status_err()),
    }
}

pub fn status_line<'a>(label: &'a str, connected: bool) -> Line<'a> {
    let indicator = if connected {
        Span::styled("● ", theme::status_ok())
    } else {
        Span::styled("○ ", theme::status_err())
    };
    Line::from(vec![
        indicator,
        Span::styled(label, theme::normal()),
    ])
}

pub fn key_hint(key: &str, action: &str) -> Span<'static> {
    Span::styled(
        format!("{}:{}", key, action),
        Style::default()
            .fg(theme::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    )
}
