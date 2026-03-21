use std::collections::VecDeque;
use std::time::Instant;

use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Constraint, Layout},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::events::{AgentEvent, ConnectionState, LogLevel};
use super::theme;

const MAX_TOOL_CALLS: usize = 100;
const MAX_LOG_LINES: usize = 200;

struct ToolCallRecord {
    time: String,
    tool: String,
    duration_ms: Option<u64>,
    success: Option<bool>,
}

struct LogRecord {
    level: LogLevel,
    message: String,
}

pub struct Dashboard {
    connection_state: ConnectionState,
    backend_id: Option<String>,
    backends: Vec<BackendInfo>,
    total_tools: usize,
    tool_calls: VecDeque<ToolCallRecord>,
    logs: VecDeque<LogRecord>,
    log_scroll: usize,
    update_available: Option<String>,
    pub should_quit: bool,
    pub should_setup: bool,
    pub should_update: bool,
    start_time: Instant,
}

struct BackendInfo {
    name: String,
    transport: String,
    tool_count: usize,
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            connection_state: ConnectionState::Connecting,
            backend_id: None,
            backends: Vec::new(),
            total_tools: 0,
            tool_calls: VecDeque::with_capacity(MAX_TOOL_CALLS),
            logs: VecDeque::with_capacity(MAX_LOG_LINES),
            log_scroll: 0,
            update_available: None,
            should_quit: false,
            should_setup: false,
            should_update: false,
            start_time: Instant::now(),
        }
    }

    pub fn handle_event(&mut self, event: &Event) {
        let Event::Key(key) = event else { return };
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('s') => self.should_setup = true,
            KeyCode::Char('u') => {
                if self.update_available.is_some() {
                    self.should_update = true;
                }
            }
            KeyCode::Up => {
                if self.log_scroll > 0 {
                    self.log_scroll -= 1;
                }
            }
            KeyCode::Down => {
                if self.log_scroll < self.logs.len().saturating_sub(1) {
                    self.log_scroll += 1;
                }
            }
            _ => {}
        }
    }

    pub fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::ConnectionStatus(state) => {
                self.connection_state = state;
            }
            AgentEvent::Registered { backend_id } => {
                self.backend_id = Some(backend_id);
                self.connection_state = ConnectionState::Connected;
            }
            AgentEvent::ToolCallReceived { request_id, tool } => {
                let now = chrono_now();
                if self.tool_calls.len() >= MAX_TOOL_CALLS {
                    self.tool_calls.pop_front();
                }
                self.tool_calls.push_back(ToolCallRecord {
                    time: now,
                    tool,
                    duration_ms: None,
                    success: None,
                });
                let _ = request_id; // Used for matching in ToolCallCompleted
            }
            AgentEvent::ToolCallCompleted {
                request_id: _,
                tool,
                duration_ms,
                success,
            } => {
                // Update the matching record if found, otherwise add new
                if let Some(record) = self.tool_calls.iter_mut().rev().find(|r| r.tool == tool && r.duration_ms.is_none()) {
                    record.duration_ms = Some(duration_ms);
                    record.success = Some(success);
                }
            }
            AgentEvent::Log { level, message } => {
                if self.logs.len() >= MAX_LOG_LINES {
                    self.logs.pop_front();
                }
                self.logs.push_back(LogRecord { level, message });
                self.log_scroll = self.logs.len().saturating_sub(1);
            }
            AgentEvent::UpdateAvailable { version } => {
                self.update_available = Some(version);
            }
            AgentEvent::BackendStarted {
                name,
                transport,
                tool_count,
            } => {
                self.total_tools += tool_count;
                self.backends.push(BackendInfo {
                    name,
                    transport,
                    tool_count,
                });
            }
        }
    }

    pub fn render(&self, f: &mut Frame) {
        let area = f.area();

        let main_chunks = Layout::vertical([
            Constraint::Length(2),  // Header
            Constraint::Fill(1),   // Middle
            Constraint::Length(8), // Logs
            Constraint::Length(1), // Status bar
        ])
        .split(area);

        self.render_header(f, main_chunks[0]);

        let middle = Layout::horizontal([
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(main_chunks[1]);

        self.render_backends_panel(f, middle[0]);
        self.render_tool_calls_panel(f, middle[1]);
        self.render_logs_panel(f, main_chunks[2]);
        self.render_status_bar(f, main_chunks[3]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let version = env!("CARGO_PKG_VERSION");

        let conn_style = match &self.connection_state {
            ConnectionState::Connected => theme::status_ok(),
            ConnectionState::Connecting | ConnectionState::Reconnecting(_) => theme::status_warn(),
            ConnectionState::Disconnected(_) => theme::status_err(),
        };

        let conn_indicator = match &self.connection_state {
            ConnectionState::Connected => "●",
            _ => "○",
        };

        let mut spans = vec![
            Span::styled(
                format!("  MCP Gateway Agent v{}  ", version),
                theme::title_style(),
            ),
            Span::styled(
                format!("{}  {}", conn_indicator, self.connection_state),
                conn_style,
            ),
        ];

        if let Some(ref update_ver) = self.update_available {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("  Update available: v{} (press u)  ", update_ver),
                theme::status_warn(),
            ));
        }

        let header = Paragraph::new(Line::from(spans));
        f.render_widget(header, area);
    }

    fn render_backends_panel(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .backends
            .iter()
            .map(|b| {
                let line = Line::from(vec![
                    Span::styled("● ", theme::status_ok()),
                    Span::styled(&b.name, theme::normal()),
                    Span::styled(
                        format!(" [{}] ({} tools)", b.transport, b.tool_count),
                        theme::dim(),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let title = format!(" Backends ({}) ", self.backends.len());
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(Span::styled(title, theme::title_style())),
        );
        f.render_widget(list, area);
    }

    fn render_tool_calls_panel(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .tool_calls
            .iter()
            .rev()
            .take(area.height.saturating_sub(2) as usize)
            .map(|tc| {
                let duration_str = match tc.duration_ms {
                    Some(ms) => format!("{:>5}ms", ms),
                    None => " ...  ".to_string(),
                };
                let status_style = match tc.success {
                    Some(true) => theme::status_ok(),
                    Some(false) => theme::status_err(),
                    None => theme::status_warn(),
                };
                let line = Line::from(vec![
                    Span::styled(&tc.time, theme::dim()),
                    Span::raw(" "),
                    Span::styled(&tc.tool, theme::normal()),
                    Span::raw(" "),
                    Span::styled(duration_str, status_style),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(Span::styled(
                    " Recent Tool Calls ",
                    theme::title_style(),
                )),
        );
        f.render_widget(list, area);
    }

    fn render_logs_panel(&self, f: &mut Frame, area: Rect) {
        let visible_height = area.height.saturating_sub(2) as usize;
        let items: Vec<ListItem> = self
            .logs
            .iter()
            .rev()
            .skip(self.logs.len().saturating_sub(self.log_scroll + visible_height))
            .take(visible_height)
            .map(|log| {
                let level_style = match log.level {
                    LogLevel::Info => theme::status_ok(),
                    LogLevel::Warn => theme::status_warn(),
                    LogLevel::Error => theme::status_err(),
                };
                let line = Line::from(vec![
                    Span::styled(format!("[{}] ", log.level), level_style),
                    Span::styled(&log.message, theme::normal()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(Span::styled(" Logs ", theme::title_style())),
        );
        f.render_widget(list, area);
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled("  q", ratatui::style::Style::default().fg(theme::TEXT_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(":quit  ", theme::dim()),
            Span::styled("s", ratatui::style::Style::default().fg(theme::TEXT_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(":setup  ", theme::dim()),
            Span::styled("u", ratatui::style::Style::default().fg(theme::TEXT_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(":update  ", theme::dim()),
            Span::styled("\u{2191}\u{2193}", ratatui::style::Style::default().fg(theme::TEXT_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(":scroll logs  ", theme::dim()),
            Span::styled(
                format!("tools:{}", self.total_tools),
                theme::dim(),
            ),
        ]);
        f.render_widget(Paragraph::new(hints), area);
    }
}

use ratatui::layout::Rect;

fn chrono_now() -> String {
    let now = std::time::SystemTime::now();
    let since_epoch = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = since_epoch.as_secs() % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
