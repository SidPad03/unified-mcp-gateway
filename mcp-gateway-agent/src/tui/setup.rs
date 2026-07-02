use std::collections::HashMap;

use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::config::{AgentConfig, Config, LocalBackendConfig};
use super::theme;
use super::widgets::{TextInput, ValidationState};

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Welcome,
    GatewayUrl,
    ApiKey,
    AgentId,
    Backends,
    AddBackendName,
    AddBackendTransport,
    AddBackendDetail,
    Summary,
    Done,
}

#[derive(Clone, Copy, PartialEq)]
enum TransportChoice {
    Stdio,
    Http,
}

pub struct SetupWizard {
    screen: Screen,
    gateway_input: TextInput,
    gateway_validation: ValidationState,
    dashboard_input: TextInput,
    api_key_input: TextInput,
    api_key_validation: ValidationState,
    agent_id_input: TextInput,
    tls_skip_verify: bool,
    backends: Vec<LocalBackendConfig>,
    backend_list_state: ListState,
    // Add backend flow
    add_name_input: TextInput,
    add_transport: TransportChoice,
    add_command_input: TextInput,
    add_args_input: TextInput,
    add_url_input: TextInput,
    // Validation task handle
    validation_tx: tokio::sync::mpsc::UnboundedSender<ValidationResult>,
    validation_rx: tokio::sync::mpsc::UnboundedReceiver<ValidationResult>,
    // Whether we finished and should exit
    pub finished: bool,
    pub should_start: bool,
    pub should_install_service: bool,
    error_message: Option<String>,
}

struct ValidationResult {
    target: ValidationTarget,
    success: bool,
}

#[derive(Clone, Copy)]
enum ValidationTarget {
    Gateway,
    ApiKey,
}

impl SetupWizard {
    pub fn new() -> Self {
        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "my-agent".to_string());

        let (validation_tx, validation_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            screen: Screen::Welcome,
            gateway_input: TextInput::new("Gateway URL"),
            gateway_validation: ValidationState::None,
            dashboard_input: TextInput::new("Dashboard URL"),
            api_key_input: TextInput::new("API Key").masked(),
            api_key_validation: ValidationState::None,
            agent_id_input: TextInput::new("Agent ID").with_value(&hostname),
            tls_skip_verify: false,
            backends: Vec::new(),
            backend_list_state: ListState::default(),
            add_name_input: TextInput::new("Backend Name"),
            add_transport: TransportChoice::Stdio,
            add_command_input: TextInput::new("Command"),
            add_args_input: TextInput::new("Arguments (space-separated)"),
            add_url_input: TextInput::new("URL"),
            validation_tx,
            validation_rx,
            finished: false,
            should_start: false,
            should_install_service: false,
            error_message: None,
        }
    }

    pub fn handle_event(&mut self, event: &Event) {
        // Check for validation results
        while let Ok(result) = self.validation_rx.try_recv() {
            match result.target {
                ValidationTarget::Gateway => {
                    self.gateway_validation = if result.success {
                        ValidationState::Valid
                    } else {
                        ValidationState::Invalid
                    };
                }
                ValidationTarget::ApiKey => {
                    self.api_key_validation = if result.success {
                        ValidationState::Valid
                    } else {
                        ValidationState::Invalid
                    };
                }
            }
        }

        let Event::Key(key) = event else { return };

        // Global: Ctrl-C to quit
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.finished = true;
            return;
        }

        match self.screen {
            Screen::Welcome => {
                if key.code == KeyCode::Enter {
                    self.screen = Screen::GatewayUrl;
                    self.gateway_input.focused = true;
                }
            }
            Screen::GatewayUrl => {
                if self.gateway_input.focused {
                    match key.code {
                        KeyCode::Enter => {
                            if !self.gateway_input.value.is_empty() {
                                self.validate_gateway();
                                self.gateway_input.focused = false;
                                self.dashboard_input.focused = true;
                            }
                        }
                        KeyCode::Esc => {
                            self.screen = Screen::Welcome;
                            self.gateway_input.focused = false;
                        }
                        KeyCode::Backspace => self.gateway_input.handle_backspace(),
                        KeyCode::Delete => self.gateway_input.handle_delete(),
                        KeyCode::Left => self.gateway_input.handle_left(),
                        KeyCode::Right => self.gateway_input.handle_right(),
                        KeyCode::Home => self.gateway_input.handle_home(),
                        KeyCode::End => self.gateway_input.handle_end(),
                        KeyCode::Char(c) => self.gateway_input.handle_char(c),
                        _ => {}
                    }
                } else if self.dashboard_input.focused {
                    match key.code {
                        KeyCode::Enter => {
                            self.dashboard_input.focused = false;
                            self.screen = Screen::ApiKey;
                            self.api_key_input.focused = true;
                        }
                        KeyCode::Esc => {
                            self.dashboard_input.focused = false;
                            self.gateway_input.focused = true;
                        }
                        KeyCode::Backspace => self.dashboard_input.handle_backspace(),
                        KeyCode::Delete => self.dashboard_input.handle_delete(),
                        KeyCode::Left => self.dashboard_input.handle_left(),
                        KeyCode::Right => self.dashboard_input.handle_right(),
                        KeyCode::Home => self.dashboard_input.handle_home(),
                        KeyCode::End => self.dashboard_input.handle_end(),
                        KeyCode::Char(c) => self.dashboard_input.handle_char(c),
                        _ => {}
                    }
                } else {
                    // TLS toggle area — 't' toggles, Enter continues
                    match key.code {
                        KeyCode::Char('t') | KeyCode::Char(' ') => {
                            self.tls_skip_verify = !self.tls_skip_verify;
                        }
                        KeyCode::Enter => {
                            self.screen = Screen::ApiKey;
                            self.api_key_input.focused = true;
                        }
                        KeyCode::Esc => {
                            self.dashboard_input.focused = true;
                        }
                        _ => {}
                    }
                }
            }
            Screen::ApiKey => {
                match key.code {
                    KeyCode::Enter => {
                        if !self.api_key_input.value.is_empty() {
                            self.validate_api_key();
                            self.api_key_input.focused = false;
                            self.screen = Screen::AgentId;
                            self.agent_id_input.focused = true;
                        }
                    }
                    KeyCode::Esc => {
                        self.screen = Screen::GatewayUrl;
                        self.api_key_input.focused = false;
                        self.gateway_input.focused = true;
                    }
                    KeyCode::Backspace => self.api_key_input.handle_backspace(),
                    KeyCode::Delete => self.api_key_input.handle_delete(),
                    KeyCode::Left => self.api_key_input.handle_left(),
                    KeyCode::Right => self.api_key_input.handle_right(),
                    KeyCode::Home => self.api_key_input.handle_home(),
                    KeyCode::End => self.api_key_input.handle_end(),
                    KeyCode::Char(c) => self.api_key_input.handle_char(c),
                    _ => {}
                }
            }
            Screen::AgentId => {
                match key.code {
                    KeyCode::Enter => {
                        if !self.agent_id_input.value.is_empty() {
                            self.agent_id_input.focused = false;
                            self.screen = Screen::Backends;
                        }
                    }
                    KeyCode::Esc => {
                        self.screen = Screen::ApiKey;
                        self.agent_id_input.focused = false;
                        self.api_key_input.focused = true;
                    }
                    KeyCode::Backspace => self.agent_id_input.handle_backspace(),
                    KeyCode::Delete => self.agent_id_input.handle_delete(),
                    KeyCode::Left => self.agent_id_input.handle_left(),
                    KeyCode::Right => self.agent_id_input.handle_right(),
                    KeyCode::Home => self.agent_id_input.handle_home(),
                    KeyCode::End => self.agent_id_input.handle_end(),
                    KeyCode::Char(c) => self.agent_id_input.handle_char(c),
                    _ => {}
                }
            }
            Screen::Backends => {
                match key.code {
                    KeyCode::Char('a') => {
                        self.add_name_input = TextInput::new("Backend Name");
                        self.add_name_input.focused = true;
                        self.screen = Screen::AddBackendName;
                    }
                    KeyCode::Char('d') => {
                        if let Some(idx) = self.backend_list_state.selected() {
                            if idx < self.backends.len() {
                                self.backends.remove(idx);
                                if !self.backends.is_empty() && idx >= self.backends.len() {
                                    self.backend_list_state.select(Some(self.backends.len() - 1));
                                }
                            }
                        }
                    }
                    KeyCode::Up => {
                        if let Some(idx) = self.backend_list_state.selected() {
                            if idx > 0 {
                                self.backend_list_state.select(Some(idx - 1));
                            }
                        }
                    }
                    KeyCode::Down => {
                        if let Some(idx) = self.backend_list_state.selected() {
                            if idx < self.backends.len().saturating_sub(1) {
                                self.backend_list_state.select(Some(idx + 1));
                            }
                        } else if !self.backends.is_empty() {
                            self.backend_list_state.select(Some(0));
                        }
                    }
                    KeyCode::Enter => {
                        self.screen = Screen::Summary;
                    }
                    KeyCode::Esc => {
                        self.screen = Screen::AgentId;
                        self.agent_id_input.focused = true;
                    }
                    _ => {}
                }
            }
            Screen::AddBackendName => {
                match key.code {
                    KeyCode::Enter => {
                        if !self.add_name_input.value.is_empty() {
                            self.add_name_input.focused = false;
                            self.add_transport = TransportChoice::Stdio;
                            self.screen = Screen::AddBackendTransport;
                        }
                    }
                    KeyCode::Esc => {
                        self.add_name_input.focused = false;
                        self.screen = Screen::Backends;
                    }
                    KeyCode::Backspace => self.add_name_input.handle_backspace(),
                    KeyCode::Char(c) => self.add_name_input.handle_char(c),
                    _ => {}
                }
            }
            Screen::AddBackendTransport => {
                match key.code {
                    KeyCode::Char('1') | KeyCode::Char('s') => {
                        self.add_transport = TransportChoice::Stdio;
                    }
                    KeyCode::Char('2') | KeyCode::Char('h') => {
                        self.add_transport = TransportChoice::Http;
                    }
                    KeyCode::Up | KeyCode::Down => {
                        self.add_transport = match self.add_transport {
                            TransportChoice::Stdio => TransportChoice::Http,
                            TransportChoice::Http => TransportChoice::Stdio,
                        };
                    }
                    KeyCode::Enter => {
                        match self.add_transport {
                            TransportChoice::Stdio => {
                                self.add_command_input = TextInput::new("Command");
                                self.add_command_input.focused = true;
                                self.add_args_input = TextInput::new("Arguments (space-separated)");
                            }
                            TransportChoice::Http => {
                                self.add_url_input = TextInput::new("URL");
                                self.add_url_input.focused = true;
                            }
                        }
                        self.screen = Screen::AddBackendDetail;
                    }
                    KeyCode::Esc => {
                        self.screen = Screen::AddBackendName;
                        self.add_name_input.focused = true;
                    }
                    _ => {}
                }
            }
            Screen::AddBackendDetail => {
                match self.add_transport {
                    TransportChoice::Stdio => self.handle_stdio_detail(key),
                    TransportChoice::Http => self.handle_http_detail(key),
                }
            }
            Screen::Summary => {
                match key.code {
                    KeyCode::Enter => {
                        match self.save_config() {
                            Ok(_) => self.screen = Screen::Done,
                            Err(e) => self.error_message = Some(format!("Failed to save: {}", e)),
                        }
                    }
                    KeyCode::Esc => {
                        self.screen = Screen::Backends;
                    }
                    _ => {}
                }
            }
            Screen::Done => {
                match key.code {
                    KeyCode::Char('r') => {
                        self.should_start = true;
                        self.finished = true;
                    }
                    KeyCode::Char('s') => {
                        self.should_install_service = true;
                        self.finished = true;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => {
                        self.finished = true;
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_stdio_detail(&mut self, key: &crossterm::event::KeyEvent) {
        if self.add_command_input.focused {
            match key.code {
                KeyCode::Enter => {
                    if !self.add_command_input.value.is_empty() {
                        self.add_command_input.focused = false;
                        self.add_args_input.focused = true;
                    }
                }
                KeyCode::Esc => {
                    self.add_command_input.focused = false;
                    self.screen = Screen::AddBackendTransport;
                }
                KeyCode::Backspace => self.add_command_input.handle_backspace(),
                KeyCode::Char(c) => self.add_command_input.handle_char(c),
                _ => {}
            }
        } else if self.add_args_input.focused {
            match key.code {
                KeyCode::Enter => {
                    self.add_args_input.focused = false;
                    self.finalize_add_backend();
                }
                KeyCode::Esc => {
                    self.add_args_input.focused = false;
                    self.add_command_input.focused = true;
                }
                KeyCode::Backspace => self.add_args_input.handle_backspace(),
                KeyCode::Char(c) => self.add_args_input.handle_char(c),
                _ => {}
            }
        }
    }

    fn handle_http_detail(&mut self, key: &crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if !self.add_url_input.value.is_empty() {
                    self.add_url_input.focused = false;
                    self.finalize_add_backend();
                }
            }
            KeyCode::Esc => {
                self.add_url_input.focused = false;
                self.screen = Screen::AddBackendTransport;
            }
            KeyCode::Backspace => self.add_url_input.handle_backspace(),
            KeyCode::Char(c) => self.add_url_input.handle_char(c),
            _ => {}
        }
    }

    fn finalize_add_backend(&mut self) {
        let backend = match self.add_transport {
            TransportChoice::Stdio => {
                let args: Vec<String> = self.add_args_input.value
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                LocalBackendConfig {
                    name: self.add_name_input.value.clone(),
                    transport: "stdio".to_string(),
                    command: Some(self.add_command_input.value.clone()),
                    args,
                    env: HashMap::new(),
                    url: None,
                    headers: HashMap::new(),
                }
            }
            TransportChoice::Http => LocalBackendConfig {
                name: self.add_name_input.value.clone(),
                transport: "streamable-http".to_string(),
                command: None,
                args: Vec::new(),
                env: HashMap::new(),
                url: Some(self.add_url_input.value.clone()),
                headers: HashMap::new(),
            },
        };
        self.backends.push(backend);
        self.screen = Screen::Backends;
        if !self.backends.is_empty() {
            self.backend_list_state.select(Some(self.backends.len() - 1));
        }
    }

    fn validate_gateway(&mut self) {
        self.gateway_validation = ValidationState::Validating;
        let url = self.gateway_input.value.clone();
        let tx = self.validation_tx.clone();
        tokio::spawn(async move {
            let success = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio_tungstenite::connect_async(&url),
            )
            .await
            .is_ok();
            let _ = tx.send(ValidationResult {
                target: ValidationTarget::Gateway,
                success,
            });
        });
    }

    fn validate_api_key(&mut self) {
        self.api_key_validation = ValidationState::Validating;
        let url = format!("{}?token={}", self.gateway_input.value, self.api_key_input.value);
        let tx = self.validation_tx.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio_tungstenite::connect_async(&url),
            )
            .await;
            let success = matches!(result, Ok(Ok(_)));
            let _ = tx.send(ValidationResult {
                target: ValidationTarget::ApiKey,
                success,
            });
        });
    }

    fn build_config(&self) -> Config {
        let dashboard_url = if self.dashboard_input.value.is_empty() {
            None
        } else {
            Some(self.dashboard_input.value.clone())
        };
        Config {
            agent: AgentConfig {
                agent_id: self.agent_id_input.value.clone(),
                gateway_url: self.gateway_input.value.clone(),
                api_key: self.api_key_input.value.clone(),
                dashboard_url,
                tls_skip_verify: self.tls_skip_verify,
            },
            backends: self.backends.clone(),
        }
    }

    fn save_config(&self) -> anyhow::Result<()> {
        crate::config::ensure_dirs()?;
        let config = self.build_config();
        let toml_str = toml::to_string_pretty(&config)?;
        let path = crate::config::default_config_path();
        crate::config::write_config_file(&path, &toml_str)?;
        Ok(())
    }

    pub fn render(&mut self, f: &mut Frame) {
        let area = f.area();

        match self.screen {
            Screen::Welcome => self.render_welcome(f, area),
            Screen::GatewayUrl => self.render_gateway(f, area),
            Screen::ApiKey => self.render_api_key(f, area),
            Screen::AgentId => self.render_agent_id(f, area),
            Screen::Backends => self.render_backends(f, area),
            Screen::AddBackendName => self.render_add_backend_name(f, area),
            Screen::AddBackendTransport => self.render_add_backend_transport(f, area),
            Screen::AddBackendDetail => self.render_add_backend_detail(f, area),
            Screen::Summary => self.render_summary(f, area),
            Screen::Done => self.render_done(f, area),
        }

        if let Some(ref msg) = self.error_message {
            self.render_error_popup(f, area, msg.clone());
        }
    }

    fn render_welcome(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(7),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Fill(1),
        ]).split(area);

        // Center the logo block horizontally but keep lines left-aligned so the art stays intact
        let logo_width: u16 = 60; // width of the widest line in the ASCII art
        let logo_area = if area.width > logo_width {
            let pad = (area.width - logo_width) / 2;
            Rect::new(chunks[1].x + pad, chunks[1].y, logo_width, chunks[1].height)
        } else {
            chunks[1]
        };
        let logo = Paragraph::new(theme::ASCII_LOGO)
            .style(theme::title_style());
        f.render_widget(logo, logo_area);

        let subtitle = Paragraph::new("Agent Setup Wizard")
            .alignment(Alignment::Center)
            .style(theme::highlight());
        f.render_widget(subtitle, chunks[2]);

        let hint = Paragraph::new("Press Enter to begin")
            .alignment(Alignment::Center)
            .style(theme::dim());
        f.render_widget(hint, chunks[3]);
    }

    fn render_step_header(&self, f: &mut Frame, area: Rect, step: u8, title: &str) {
        let header = Line::from(vec![
            Span::styled(
                format!("  Step {}/5  ", step),
                ratatui::style::Style::default()
                    .fg(theme::BRAND_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(title, theme::title_style()),
        ]);
        f.render_widget(Paragraph::new(header), area);
    }

    fn render_gateway(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Fill(1),
        ]).split(area);

        self.render_step_header(f, chunks[0], 1, "Connection");

        let hint = Paragraph::new("Gateway WebSocket URL (e.g., wss://mcp.example.com/agent/ws)")
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        self.gateway_input.render(f, chunks[2]);

        let validation = Line::from(vec![
            super::widgets::validation_indicator(self.gateway_validation),
        ]);
        f.render_widget(Paragraph::new(validation), chunks[3]);

        let hint2 = Paragraph::new("Dashboard URL for updates (leave empty to auto-derive from gateway URL)")
            .style(theme::dim());
        f.render_widget(hint2, chunks[4]);

        self.dashboard_input.render(f, chunks[5]);

        let tls_label = if self.tls_skip_verify {
            "  [x] Skip TLS certificate verification (for self-signed certs)"
        } else {
            "  [ ] Skip TLS certificate verification (for self-signed certs)"
        };
        let tls_hint = Paragraph::new(tls_label)
            .style(if self.tls_skip_verify { theme::status_warn() } else { theme::dim() });
        f.render_widget(tls_hint, chunks[6]);
    }

    fn render_api_key(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Fill(1),
        ]).split(area);

        self.render_step_header(f, chunks[0], 2, "API Key");

        let hint = Paragraph::new("Enter your MCP Gateway API key (starts with mcpgw_)")
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        self.api_key_input.render(f, chunks[2]);

        let validation = Line::from(vec![
            super::widgets::validation_indicator(self.api_key_validation),
        ]);
        f.render_widget(Paragraph::new(validation), chunks[3]);
    }

    fn render_agent_id(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Fill(1),
        ]).split(area);

        self.render_step_header(f, chunks[0], 3, "Agent ID");

        let hint = Paragraph::new("A unique identifier for this agent (pre-filled with hostname)")
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        self.agent_id_input.render(f, chunks[2]);
    }

    fn render_backends(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(2),
        ]).split(area);

        self.render_step_header(f, chunks[0], 4, "Local Backends");

        let hint = Paragraph::new("Configure MCP backends to expose through the gateway")
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        let items: Vec<ListItem> = self.backends.iter().map(|b| {
            let detail = match b.transport.as_str() {
                "stdio" => b.command.as_deref().unwrap_or("?").to_string(),
                _ => b.url.as_deref().unwrap_or("?").to_string(),
            };
            ListItem::new(format!("  {} [{}] - {}", b.name, b.transport, detail))
        }).collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).border_style(theme::border()).title(" Backends "))
            .highlight_style(theme::highlight());

        f.render_stateful_widget(list, chunks[2], &mut self.backend_list_state);

        let hint = Paragraph::new("  a:add  d:delete  Enter:continue  Esc:back")
            .style(theme::dim());
        f.render_widget(hint, chunks[3]);
    }

    fn render_add_backend_name(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Fill(1),
        ]).split(area);

        let header = Paragraph::new("  Add Backend - Name")
            .style(theme::title_style());
        f.render_widget(header, chunks[0]);

        let hint = Paragraph::new("Enter a name for this backend (e.g., serena, playwright)")
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        self.add_name_input.render(f, chunks[2]);
    }

    fn render_add_backend_transport(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Fill(1),
        ]).split(area);

        let header = Paragraph::new("  Add Backend - Transport")
            .style(theme::title_style());
        f.render_widget(header, chunks[0]);

        let hint = Paragraph::new("Select the transport type:")
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        let stdio_style = if self.add_transport == TransportChoice::Stdio {
            theme::highlight()
        } else {
            theme::normal()
        };
        let http_style = if self.add_transport == TransportChoice::Http {
            theme::highlight()
        } else {
            theme::normal()
        };

        let options = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                if self.add_transport == TransportChoice::Stdio { "  > [1] stdio  - Spawn a local process" } else { "    [1] stdio  - Spawn a local process" },
                stdio_style,
            )),
            Line::from(""),
            Line::from(Span::styled(
                if self.add_transport == TransportChoice::Http { "  > [2] http   - Connect to HTTP endpoint" } else { "    [2] http   - Connect to HTTP endpoint" },
                http_style,
            )),
        ]);
        f.render_widget(options, chunks[2]);
    }

    fn render_add_backend_detail(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Fill(1),
        ]).split(area);

        let header = Paragraph::new("  Add Backend - Details")
            .style(theme::title_style());
        f.render_widget(header, chunks[0]);

        match self.add_transport {
            TransportChoice::Stdio => {
                let hint = Paragraph::new("Enter the command and arguments for the stdio backend")
                    .style(theme::dim());
                f.render_widget(hint, chunks[1]);
                self.add_command_input.render(f, chunks[2]);
                self.add_args_input.render(f, chunks[3]);
            }
            TransportChoice::Http => {
                let hint = Paragraph::new("Enter the URL for the HTTP MCP backend")
                    .style(theme::dim());
                f.render_widget(hint, chunks[1]);
                self.add_url_input.render(f, chunks[2]);
            }
        }
    }

    fn render_summary(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(2),
        ]).split(area);

        self.render_step_header(f, chunks[0], 5, "Summary");

        let path = crate::config::default_config_path();
        let hint = Paragraph::new(format!("Config will be saved to: {}", path.display()))
            .style(theme::dim());
        f.render_widget(hint, chunks[1]);

        let config = self.build_config();
        let toml_str = toml::to_string_pretty(&config).unwrap_or_else(|e| format!("Error: {}", e));

        let config_view = Paragraph::new(toml_str)
            .block(Block::default().borders(Borders::ALL).border_style(theme::border()).title(" config.toml "))
            .wrap(Wrap { trim: false });
        f.render_widget(config_view, chunks[2]);

        let hint = Paragraph::new("  Enter:save  Esc:back")
            .style(theme::dim());
        f.render_widget(hint, chunks[3]);
    }

    fn render_done(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(6),
            Constraint::Fill(1),
        ]).split(area);

        let path = crate::config::default_config_path();
        let msg = Paragraph::new(format!("Configuration saved to {}", path.display()))
            .alignment(Alignment::Center)
            .style(theme::status_ok());
        f.render_widget(msg, chunks[1]);

        let subtitle = Paragraph::new("What would you like to do next?")
            .alignment(Alignment::Center)
            .style(theme::normal());
        f.render_widget(subtitle, chunks[2]);

        let options = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  [r] Run the agent now", theme::highlight())),
            Line::from(Span::styled("  [s] Install as a background service (launchd)", theme::normal())),
            Line::from(Span::styled("  [q] Quit", theme::dim())),
        ]);
        f.render_widget(options, chunks[3]);
    }

    fn render_error_popup(&self, f: &mut Frame, area: Rect, msg: String) {
        let popup_area = centered_rect(60, 20, area);
        f.render_widget(Clear, popup_area);

        let popup = Paragraph::new(msg)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme::status_err())
                    .title(" Error "),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(popup, popup_area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ]).split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ]).split(popup_layout[1])[1]
}
