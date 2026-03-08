use crate::persistence::persistence::{McpServer, McpServerTool, Persistence, Persistable};
use crate::screens::menu::MenuScreen;
use crate::screens::server_tools::ServerToolsScreen;
use crate::dyns::dynamictool::DynamicToolDef;
use crate::generators::server::{ServerGenerator, ServerGeneratorConfig};
use crate::ui::navigation::NavigatableList;
use crate::ui::screen::{Screen, ScreenAction};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

fn published_ports() -> &'static RwLock<HashMap<i64, u16>> {
    static PORTS: OnceLock<RwLock<HashMap<i64, u16>>> = OnceLock::new();
    PORTS.get_or_init(|| RwLock::new(HashMap::new()))
}

// ─── Mode ────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum ScreenMode {
    List,
    Form,
    ConfirmDelete,
    PublishForm,
}

// ─── Form field index ────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy)]
enum FormField {
    Name,
    Version,
}

// ─── McpServersScreen ────────────────────────────────────────────────────────

pub struct McpServersScreen {
    pub title: String,
    pub list: NavigatableList,
    pub servers: Vec<McpServer>,
    pub persistence: Persistence,

    mode: ScreenMode,

    // --- form sub-state ---
    editing_index: Option<usize>,
    form_field: FormField,
    form_name: String,
    form_version: String,
    
    // --- publish sub-state ---
    publish_port: String,

    // --- status bar message ---
    status_message: Option<String>,
}

impl McpServersScreen {
    pub fn new() -> Self {
        let persistence = Persistence::new();
        persistence.sync_schema();

        let servers: Vec<McpServer> = persistence.get_all();
        let list_options = Self::build_list_options(&servers);

        let mut list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: list_options,
        };
        if !list.options.is_empty() {
            list.state.select(Some(0));
        }

        McpServersScreen {
            title: "MCP Servers".to_string(),
            list,
            servers,
            persistence,
            mode: ScreenMode::List,
            editing_index: None,
            form_field: FormField::Name,
            form_name: String::new(),
            form_version: String::new(),
            publish_port: String::new(),
            status_message: None,
        }
    }

    fn build_list_options(servers: &[McpServer]) -> Vec<String> {
        let mut opts: Vec<String> = servers
            .iter()
            .map(|s| format!("{} (v{})", s.name, s.version))
            .collect();
        opts.push("[ + Add Server ]".to_string());
        opts
    }

    fn refresh_list(&mut self) {
        self.servers = self.persistence.get_all();
        let opts = Self::build_list_options(&self.servers);
        let prev = self.list.state.selected().unwrap_or(0);
        self.list.options = opts;
        let clamped = prev.min(self.list.options.len().saturating_sub(1));
        self.list.state.select(Some(clamped));
    }

    fn selected_server_index(&self) -> Option<usize> {
        let idx = self.list.state.selected()?;
        if idx < self.servers.len() {
            Some(idx)
        } else {
            None
        }
    }

    fn open_add_form(&mut self) {
        self.editing_index = None;
        self.form_name = String::new();
        self.form_version = "1.0.0".to_string();
        self.form_field = FormField::Name;
        self.mode = ScreenMode::Form;
        self.status_message = None;
    }

    fn open_edit_form(&mut self, idx: usize) {
        let server = self.servers[idx].clone();
        self.editing_index = Some(idx);
        self.form_name = server.name;
        self.form_version = server.version;
        self.form_field = FormField::Name;
        self.mode = ScreenMode::Form;
        self.status_message = None;
    }

    fn open_publish_form(&mut self, idx: usize) {
        self.editing_index = Some(idx);
        self.publish_port = "8000".to_string();
        self.mode = ScreenMode::PublishForm;
        self.status_message = None;
    }

    fn publish_server(&mut self) {
        if let Some(idx) = self.editing_index {
            if let Ok(port) = self.publish_port.trim().parse::<u16>() {
                let server = &self.servers[idx];
                let server_name = server.name.clone();
                let server_version = server.version.clone();
                
                let tools_data = if let Some(id) = server.id {
                    self.persistence.get_by_parent::<McpServerTool>(id)
                } else {
                    vec![]
                };

                let tools: Vec<DynamicToolDef> = tools_data.into_iter()
                    .filter_map(|t| serde_json::from_str(&t.tool_def).ok())
                    .collect();

                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
                    rt.block_on(async {
                        let config = ServerGeneratorConfig {
                            name: server_name,
                            version: server_version,
                        };
                        let mut generator = ServerGenerator::new(&config);
                        for t in tools {
                            generator.add_tools(t);
                        }
                        generator.serve_server_http(port);
                        std::future::pending::<()>().await;
                    });
                });
                
                if let Some(id) = server.id {
                    if let Ok(mut ports) = published_ports().write() {
                        ports.insert(id, port);
                    }
                }
                
                self.status_message = Some(format!("✓ Publishing on {}...", port));
            } else {
                self.status_message = Some("✗ Invalid port number".to_string());
                return;
            }
        }
        self.mode = ScreenMode::List;
    }

    fn save_form(&mut self) {
        let name = self.form_name.trim().to_string();
        let version = self.form_version.trim().to_string();

        if name.is_empty() {
            self.status_message = Some("✗ Name cannot be empty".to_string());
            return;
        }

        match self.editing_index {
            None => {
                let server = McpServer {
                    id: None,
                    name: name.clone(),
                    version,
                };
                self.persistence.save(&server);
                self.status_message = Some(format!("✓ Added server {}", name));
            }
            Some(idx) => {
                let existing_id = self.servers[idx].id;
                let server = McpServer {
                    id: existing_id,
                    name: name.clone(),
                    version,
                };
                self.persistence.update(&server);
                self.status_message = Some(format!("✓ Updated server {}", name));
            }
        }

        self.refresh_list();
        self.mode = ScreenMode::List;
    }

    fn delete_selected(&mut self) {
        if let Some(idx) = self.selected_server_index() {
            if let Some(id) = self.servers[idx].id {
                self.persistence.delete::<McpServer>(id);
                self.status_message = Some(format!("✓ Deleted server {}", self.servers[idx].name));
                self.refresh_list();
            }
        }
        self.mode = ScreenMode::List;
    }

    // ─── Input Handlers ──────────────────────────────────────────────────────

    fn handle_list_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        self.status_message = None;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.list.next();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.previous();
                None
            }
            KeyCode::Enter => {
                let selected = self.list.state.selected().unwrap_or(0);
                if selected >= self.servers.len() {
                    self.open_add_form();
                    None
                } else {
                    let server = &self.servers[selected];
                    if let Some(id) = server.id {
                        Some(ScreenAction::Switch(Box::new(ServerToolsScreen::new(id, server.name.clone()))))
                    } else {
                        None
                    }
                }
            }
            KeyCode::Char('a') => {
                self.open_add_form();
                None
            }
            KeyCode::Char('e') => {
                if let Some(idx) = self.selected_server_index() {
                    self.open_edit_form(idx);
                }
                None
            }
            KeyCode::Char('p') => {
                if let Some(idx) = self.selected_server_index() {
                    self.open_publish_form(idx);
                }
                None
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if self.selected_server_index().is_some() {
                    self.mode = ScreenMode::ConfirmDelete;
                }
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(ScreenAction::Switch(Box::new(MenuScreen::new()))),
            _ => None,
        }
    }

    fn handle_form_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        let target_str = match self.form_field {
            FormField::Name => &mut self.form_name,
            FormField::Version => &mut self.form_version,
        };

        match key.code {
            KeyCode::Char(c) => {
                target_str.push(c);
                None
            }
            KeyCode::Backspace => {
                target_str.pop();
                None
            }
            KeyCode::Tab => {
                self.form_field = match self.form_field {
                    FormField::Name => FormField::Version,
                    FormField::Version => FormField::Name,
                };
                None
            }
            KeyCode::Enter => {
                self.save_form();
                None
            }
            KeyCode::Esc => {
                self.mode = ScreenMode::List;
                None
            }
            _ => None,
        }
    }

    fn handle_confirm_delete_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                self.delete_selected();
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = ScreenMode::List;
                None
            }
            _ => None,
        }
    }

    fn handle_publish_form_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.publish_port.push(c);
                None
            }
            KeyCode::Backspace => {
                self.publish_port.pop();
                None
            }
            KeyCode::Enter => {
                self.publish_server();
                None
            }
            KeyCode::Esc => {
                self.mode = ScreenMode::List;
                None
            }
            _ => None,
        }
    }

    // ─── Rendering ───────────────────────────────────────────────────────────

    fn render_list(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        let items: Vec<ListItem> = self
            .list
            .options
            .iter()
            .enumerate()
            .map(|(i, label)| {
                if i < self.servers.len() {
                    let server = &self.servers[i];
                    let tool_count = server.id.map(|id| self.persistence.get_by_parent::<McpServerTool>(id).len()).unwrap_or(0);
                    
                    let mut spans = vec![
                        Span::styled(format!("  {:<20} ", server.name), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("v{:<8} ", server.version), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} tools", tool_count), Style::default().fg(Color::Blue)),
                    ];

                    if let Some(id) = server.id {
                        if let Ok(ports) = published_ports().read() {
                            if let Some(port) = ports.get(&id) {
                                spans.push(Span::styled("   ● ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)));
                                spans.push(Span::styled(format!("Published on port {}", port), Style::default().fg(Color::LightGreen)));
                            }
                        }
                    }
                    
                    let line = Line::from(spans);
                    ListItem::new(line)
                } else {
                    ListItem::new(Line::from(Span::styled(
                        format!("  {}", label),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
                    )))
                }
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(Span::styled(format!(" {} ", self.title), Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::Black)),
            )
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, chunks[0], &mut self.list.state);

        // Footer
        let status = self.status_message.as_deref().unwrap_or("");
        let keys = vec![
            (" ↑↓ ", "Navigate"),
            (" Enter ", "Manage Tools"),
            (" a ", "Add"),
            (" e ", "Edit"),
            (" p ", "Publish"),
            (" d ", "Delete"),
            (" q ", "Menu"),
        ];

        let mut spans = Vec::new();
        for (key, desc) in &keys {
            spans.push(Span::styled(*key, Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" {} ", desc), Style::default().fg(Color::Gray)));
            spans.push(Span::raw("  "));
        }

        let status_color = if status.starts_with('✓') { Color::LightGreen } else { Color::Yellow };
        spans.push(Span::styled(format!("  {}", status), Style::default().fg(status_color).add_modifier(Modifier::BOLD)));

        let footer = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)).style(Style::default().bg(Color::Black)));
        frame.render_widget(footer, chunks[1]);
    }

    fn render_form(&mut self, frame: &mut Frame, area: Rect) {
        self.render_list(frame, area);

        let popup_area = centered_rect(50, 60, area);
        frame.render_widget(Clear, popup_area);

        let title = if self.editing_index.is_none() { " Add MCP Server " } else { " Edit MCP Server " };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Name input
                Constraint::Length(3), // Version input
                Constraint::Length(1), // Footer keys
                Constraint::Length(1), // Status
            ])
            .split(
                Block::default()
                    .title(Span::styled(title, Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .style(Style::default().bg(Color::Black))
                    .inner(popup_area),
            );

        frame.render_widget(Block::default().title(Span::styled(title, Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).style(Style::default().bg(Color::Black)), popup_area);

        // Name field
        let name_border = if self.form_field == FormField::Name { Color::Yellow } else { Color::DarkGray };
        let name_block = Paragraph::new(format!("  {}", self.form_name)).block(Block::default().title(Span::styled(" Name ", Style::default().fg(name_border))).borders(Borders::ALL).border_style(Style::default().fg(name_border)));
        frame.render_widget(name_block, chunks[0]);

        // Version field
        let ver_border = if self.form_field == FormField::Version { Color::Yellow } else { Color::DarkGray };
        let ver_block = Paragraph::new(format!("  {}", self.form_version)).block(Block::default().title(Span::styled(" Version ", Style::default().fg(ver_border))).borders(Borders::ALL).border_style(Style::default().fg(ver_border)));
        frame.render_widget(ver_block, chunks[1]);

        // Footer keys
        let footer_spans = vec![
            Span::styled(" Tab ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Next Field  ", Style::default().fg(Color::Gray)),
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Save  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(footer_spans)).style(Style::default().bg(Color::Black)), chunks[2]);

        // Status
        if let Some(msg) = &self.status_message {
            let color = if msg.contains('✓') { Color::LightGreen } else { Color::LightRed };
            frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(format!(" {}", msg), Style::default().fg(color).add_modifier(Modifier::BOLD))])), chunks[3]);
        }
    }

    fn render_confirm_delete(&mut self, frame: &mut Frame, area: Rect) {
        self.render_list(frame, area);

        let popup_area = centered_rect(50, 30, area);
        frame.render_widget(Clear, popup_area);

        let server_name = self.selected_server_index().map(|i| self.servers[i].name.clone()).unwrap_or_default();
        let text = format!("  Delete MCP Server \"{}\"?\n  (This will also delete tools associated with it)", server_name);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(3)])
            .split(Block::default().title(Span::styled(" ⚠ Confirm Delete ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))).borders(Borders::ALL).border_style(Style::default().fg(Color::Red)).style(Style::default().bg(Color::Black)).inner(popup_area));

        frame.render_widget(Block::default().title(Span::styled(" ⚠ Confirm Delete ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))).borders(Borders::ALL).border_style(Style::default().fg(Color::Red)).style(Style::default().bg(Color::Black)), popup_area);
        frame.render_widget(Paragraph::new(text).style(Style::default().fg(Color::White).bg(Color::Black)).wrap(Wrap { trim: false }), chunks[0]);
        frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(" y ", Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(" Yes, delete  ", Style::default().fg(Color::Gray)), Span::styled(" n ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(" Cancel", Style::default().fg(Color::Gray))])).style(Style::default().bg(Color::Black)), chunks[1]);
    }

    fn render_publish_form(&mut self, frame: &mut Frame, area: Rect) {
        self.render_list(frame, area);

        let popup_area = centered_rect(40, 30, area);
        frame.render_widget(Clear, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Port input
                Constraint::Length(1), // Footer keys
            ])
            .split(
                Block::default()
                    .title(Span::styled(" Publish MCP Server ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta))
                    .style(Style::default().bg(Color::Black))
                    .inner(popup_area),
            );

        frame.render_widget(Block::default().title(Span::styled(" Publish MCP Server ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD))).borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)).style(Style::default().bg(Color::Black)), popup_area);

        // Port field
        let port_block = Paragraph::new(format!("  {}", self.publish_port)).block(Block::default().title(" Port ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
        frame.render_widget(port_block, chunks[0]);

        // Footer keys
        let footer_spans = vec![
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Publish  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(footer_spans)).style(Style::default().bg(Color::Black)), chunks[1]);
    }
}

impl Screen for McpServersScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match self.mode {
            ScreenMode::List => self.handle_list_input(key),
            ScreenMode::Form => self.handle_form_input(key),
            ScreenMode::ConfirmDelete => self.handle_confirm_delete_input(key),
            ScreenMode::PublishForm => self.handle_publish_form_input(key),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self.mode {
            ScreenMode::List => self.render_list(frame, area),
            ScreenMode::Form => self.render_form(frame, area),
            ScreenMode::ConfirmDelete => self.render_confirm_delete(frame, area),
            ScreenMode::PublishForm => self.render_publish_form(frame, area),
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2)])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2)])
        .split(popup_layout[1])[1]
}
