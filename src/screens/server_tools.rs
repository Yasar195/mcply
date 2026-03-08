use crate::dyns::dynamictool::{ActionType, DynamicToolDef};
use crate::persistence::persistence::{McpServerTool, Persistence};
use crate::protocoal::http::{HttpMethod, HttpProtocoal};
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

#[derive(Debug, PartialEq)]
enum ScreenMode {
    List,
    Form,
    ConfirmDelete,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum FormField {
    Name,
    Description,
    Url,
    Method,
    Params, // A simple comma separated list of param names for now
}

pub struct ServerToolsScreen {
    pub title: String,
    pub server_id: i64,
    pub server_name: String,
    pub list: NavigatableList,
    pub tools: Vec<McpServerTool>,
    pub persistence: Persistence,

    mode: ScreenMode,

    editing_index: Option<usize>,
    form_field: FormField,
    
    // Form fields
    form_name: String,
    form_desc: String,
    form_url: String,
    form_method: String,
    form_params: String,

    status_message: Option<String>,
}

impl ServerToolsScreen {
    pub fn new(server_id: i64, server_name: String) -> Self {
        let persistence = Persistence::new();
        let tools: Vec<McpServerTool> = persistence.get_by_parent(server_id);
        let list_options = Self::build_list_options(&tools);

        let mut list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: list_options,
        };
        if !list.options.is_empty() {
            list.state.select(Some(0));
        }

        ServerToolsScreen {
            title: format!("Tools for: {}", server_name),
            server_id,
            server_name,
            list,
            tools,
            persistence,
            mode: ScreenMode::List,
            editing_index: None,
            form_field: FormField::Name,
            form_name: String::new(),
            form_desc: String::new(),
            form_url: String::new(),
            form_method: "GET".to_string(),
            form_params: String::new(),
            status_message: None,
        }
    }

    fn build_list_options(tools: &[McpServerTool]) -> Vec<String> {
        let mut opts: Vec<String> = tools
            .iter()
            .map(|t| t.tool_name.clone())
            .collect();
        opts.push("[ + Add Tool ]".to_string());
        opts
    }

    fn refresh_list(&mut self) {
        self.tools = self.persistence.get_by_parent(self.server_id);
        let opts = Self::build_list_options(&self.tools);
        let prev = self.list.state.selected().unwrap_or(0);
        self.list.options = opts;
        let clamped = prev.min(self.list.options.len().saturating_sub(1));
        self.list.state.select(Some(clamped));
    }

    fn open_add_form(&mut self) {
        self.editing_index = None;
        self.form_name = String::new();
        self.form_desc = String::new();
        self.form_url = String::new();
        self.form_method = "GET".to_string();
        self.form_params = String::new();
        self.form_field = FormField::Name;
        self.mode = ScreenMode::Form;
        self.status_message = None;
    }

    fn open_edit_form(&mut self, idx: usize) {
        let tool_record = &self.tools[idx];
        self.editing_index = Some(idx);
        self.form_name = tool_record.tool_name.clone();
        
        // Parse the definition to populate form
        if let Ok(def) = serde_json::from_str::<DynamicToolDef>(&tool_record.tool_def) {
            self.form_desc = def.description;
            self.form_url = def.tool.url;
            self.form_method = match def.tool.method {
                HttpMethod::GET => "GET",
                HttpMethod::POST => "POST",
                HttpMethod::PUT => "PUT",
                HttpMethod::PATCH => "PATCH",
                HttpMethod::DELETE => "DELETE",
            }.to_string();
            
            if let Some(params) = def.parameters {
                let p_names: Vec<String> = params.into_iter().map(|p| p.name).collect();
                self.form_params = p_names.join(", ");
            } else {
                self.form_params = String::new();
            }
        }

        self.form_field = FormField::Name;
        self.mode = ScreenMode::Form;
        self.status_message = None;
    }

    fn save_form(&mut self) {
        let name = self.form_name.trim().to_string();
        if name.is_empty() || self.form_url.trim().is_empty() {
            self.status_message = Some("✗ Name and URL required".to_string());
            return;
        }

        let method = match self.form_method.trim().to_uppercase().as_str() {
            "POST" => HttpMethod::POST,
            "PUT" => HttpMethod::PUT,
            "PATCH" => HttpMethod::PATCH,
            "DELETE" => HttpMethod::DELETE,
            _ => HttpMethod::GET,
        };

        let mut params = Vec::new();
        if !self.form_params.trim().is_empty() {
            for p in self.form_params.split(',') {
                let p_name = p.trim().to_string();
                if !p_name.is_empty() {
                    params.push(crate::dyns::dynamictool::ToolParam {
                        name: p_name.clone(),
                        description: format!("Parameter {}", p_name),
                        required: true,
                        param_type: "string".to_string(),
                    });
                }
            }
        }

        let def = DynamicToolDef {
            name: name.clone(),
            description: self.form_desc.trim().to_string(),
            parameters: if params.is_empty() { None } else { Some(params) },
            action: ActionType::http,
            tool: HttpProtocoal {
                method,
                url: self.form_url.trim().to_string(),
                query_params: None, // Can be extended later
                path_params: None,
                body: None,
                request_headers: None,
            },
        };

        let def_json = serde_json::to_string(&def).unwrap_or_default();

        match self.editing_index {
            None => {
                let tool = McpServerTool {
                    id: None,
                    server_id: self.server_id,
                    tool_name: name.clone(),
                    tool_def: def_json,
                };
                self.persistence.save(&tool);
                self.status_message = Some(format!("✓ Added tool {}", name));
            }
            Some(idx) => {
                let existing_id = self.tools[idx].id;
                let tool = McpServerTool {
                    id: existing_id,
                    server_id: self.server_id,
                    tool_name: name.clone(),
                    tool_def: def_json,
                };
                self.persistence.update(&tool);
                self.status_message = Some(format!("✓ Updated tool {}", name));
            }
        }

        self.refresh_list();
        self.mode = ScreenMode::List;
    }

    fn delete_selected(&mut self) {
        if let Some(idx) = self.list.state.selected() {
            if idx < self.tools.len() {
                if let Some(id) = self.tools[idx].id {
                    self.persistence.delete::<McpServerTool>(id);
                    self.status_message = Some(format!("✓ Deleted tool {}", self.tools[idx].tool_name));
                    self.refresh_list();
                }
            }
        }
        self.mode = ScreenMode::List;
    }

    // ─── Input Handlers ──────────────────────────────────────────────────────

    fn handle_list_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        self.status_message = None;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => { self.list.next(); None }
            KeyCode::Up | KeyCode::Char('k') => { self.list.previous(); None }
            KeyCode::Enter => {
                let selected = self.list.state.selected().unwrap_or(0);
                if selected >= self.tools.len() {
                    self.open_add_form();
                }
                None
            }
            KeyCode::Char('a') => { self.open_add_form(); None }
            KeyCode::Char('e') => {
                if let Some(idx) = self.list.state.selected() {
                    if idx < self.tools.len() { self.open_edit_form(idx); }
                }
                None
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(idx) = self.list.state.selected() {
                    if idx < self.tools.len() { self.mode = ScreenMode::ConfirmDelete; }
                }
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(ScreenAction::Switch(Box::new(crate::screens::mcp_servers::McpServersScreen::new()))),
            _ => None,
        }
    }

    fn handle_form_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        let (target_str, next_field) = match self.form_field {
            FormField::Name => (&mut self.form_name, FormField::Description),
            FormField::Description => (&mut self.form_desc, FormField::Url),
            FormField::Url => (&mut self.form_url, FormField::Method),
            FormField::Method => (&mut self.form_method, FormField::Params),
            FormField::Params => (&mut self.form_params, FormField::Name),
        };

        match key.code {
            KeyCode::Char(c) => { target_str.push(c); None }
            KeyCode::Backspace => { target_str.pop(); None }
            KeyCode::Tab => { self.form_field = next_field; None }
            KeyCode::BackTab => {
                self.form_field = match self.form_field {
                    FormField::Name => FormField::Params,
                    FormField::Description => FormField::Name,
                    FormField::Url => FormField::Description,
                    FormField::Method => FormField::Url,
                    FormField::Params => FormField::Method,
                };
                None
            }
            KeyCode::Enter => { self.save_form(); None }
            KeyCode::Esc => { self.mode = ScreenMode::List; None }
            _ => None,
        }
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        let items: Vec<ListItem> = self.list.options.iter().enumerate().map(|(i, label)| {
            if i < self.tools.len() {
                let tool = &self.tools[i];
                let method = if let Ok(def) = serde_json::from_str::<DynamicToolDef>(&tool.tool_def) {
                    match def.tool.method {
                        HttpMethod::GET => "GET", HttpMethod::POST => "POST",
                        HttpMethod::PUT => "PUT", HttpMethod::PATCH => "PATCH", HttpMethod::DELETE => "DEL"
                    }
                } else { "???" };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {:<6} ", method), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                    Span::styled(tool.tool_name.clone(), Style::default().fg(Color::Cyan)),
                ]))
            } else {
                ListItem::new(Line::from(Span::styled(format!("  {}", label), Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC))))
            }
        }).collect();

        let list = List::new(items)
            .block(Block::default().title(format!(" {} ", self.title)).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)).style(Style::default().bg(Color::Black)))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, chunks[0], &mut self.list.state);

        let status = self.status_message.as_deref().unwrap_or("");
        let keys = vec![(" ↑↓ ", "Nav"), (" a ", "Add"), (" e ", "Edit"), (" d ", "Del"), (" q ", "Back")];
        let mut spans = Vec::new();
        for (k, desc) in keys {
            spans.push(Span::styled(k, Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" {}  ", desc), Style::default().fg(Color::Gray)));
        }
        spans.push(Span::styled(status, Style::default().fg(if status.starts_with('✓') { Color::LightGreen } else { Color::Yellow })));

        frame.render_widget(Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray))), chunks[1]);
    }

    fn render_form(&mut self, frame: &mut Frame, area: Rect) {
        self.render_list(frame, area);
        let popup_area = centered_rect(70, 80, area);
        frame.render_widget(Clear, popup_area);

        let chunks = Layout::default().direction(Direction::Vertical).constraints([
            Constraint::Length(3), // Name
            Constraint::Length(3), // Description
            Constraint::Length(3), // URL
            Constraint::Length(3), // Method
            Constraint::Length(3), // Params
            Constraint::Length(1), // Footer
            Constraint::Length(1), // Status
        ]).split(Block::default().title(" Tool ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).style(Style::default().bg(Color::Black)).inner(popup_area));

        frame.render_widget(Block::default().title(" Tool Form ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).style(Style::default().bg(Color::Black)), popup_area);

        let fields = [
            (&self.form_name, "Name", FormField::Name, chunks[0]),
            (&self.form_desc, "Description", FormField::Description, chunks[1]),
            (&self.form_url, "Endpoint URL", FormField::Url, chunks[2]),
            (&self.form_method, "Method (GET/POST...)", FormField::Method, chunks[3]),
            (&self.form_params, "Params (comma-separated)", FormField::Params, chunks[4]),
        ];

        for (val, title, field, chunk) in fields {
            let color = if self.form_field == field { Color::Yellow } else { Color::DarkGray };
            frame.render_widget(Paragraph::new(format!("  {}", val)).block(Block::default().title(format!(" {} ", title)).borders(Borders::ALL).border_style(Style::default().fg(color))), chunk);
        }

        let footer = vec![
            Span::styled(" Tab ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(" Next  ", Style::default().fg(Color::Gray)),
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(" Save  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ];
        frame.render_widget(Paragraph::new(Line::from(footer)), chunks[5]);

        if let Some(msg) = &self.status_message {
            let color = if msg.contains('✓') { Color::LightGreen } else { Color::LightRed };
            frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(format!(" {}", msg), Style::default().fg(color).add_modifier(Modifier::BOLD))])), chunks[6]);
        }
    }
}

impl Screen for ServerToolsScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match self.mode {
            ScreenMode::List => self.handle_list_input(key),
            ScreenMode::Form => self.handle_form_input(key),
            ScreenMode::ConfirmDelete => {
                if key.code == KeyCode::Char('y') || key.code == KeyCode::Enter { self.delete_selected(); }
                else if key.code == KeyCode::Char('n') || key.code == KeyCode::Esc { self.mode = ScreenMode::List; }
                None
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self.mode {
            ScreenMode::List => self.render_list(frame, area),
            ScreenMode::Form => self.render_form(frame, area),
            ScreenMode::ConfirmDelete => {
                self.render_list(frame, area);
                let popup = centered_rect(40, 20, area);
                frame.render_widget(Clear, popup);
                frame.render_widget(Paragraph::new("  Confirm Delete Tool? (y/n)")
                    .block(Block::default().title(" ⚠ Delete ").borders(Borders::ALL).border_style(Style::default().fg(Color::Red)).style(Style::default().bg(Color::Black)))
                    .style(Style::default().fg(Color::White)), popup);
            }
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
