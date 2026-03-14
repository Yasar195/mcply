use crate::dyns::dynamictool::{ActionType, DynamicToolDef, ToolParam};
use crate::persistence::persistence::{McpServerTool, Persistence};
use crate::protocol::http::{HttpMethod, HttpProtocoal};
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

// ─── Param type ──────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ParamKind {
    Path,
    Query,
    Body,
}

impl ParamKind {
    fn label(&self) -> &'static str {
        match self {
            ParamKind::Path => "Path",
            ParamKind::Query => "Query",
            ParamKind::Body => "Body",
        }
    }
    fn color(&self) -> Color {
        match self {
            ParamKind::Path => Color::LightRed,
            ParamKind::Query => Color::LightBlue,
            ParamKind::Body => Color::LightGreen,
        }
    }
    fn cycle_next(&self) -> Self {
        match self {
            ParamKind::Path => ParamKind::Query,
            ParamKind::Query => ParamKind::Body,
            ParamKind::Body => ParamKind::Path,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParamEntry {
    pub name: String,
    pub description: String,
    pub kind: ParamKind,
    pub required: bool,
}

impl ParamEntry {
    fn new() -> Self {
        ParamEntry {
            name: String::new(),
            description: String::new(),
            kind: ParamKind::Query,
            required: true,
        }
    }
}

// ─── Form focus ──────────────────────────────────────────────────────────────

/// Which top-level form field is focused
#[derive(Debug, PartialEq, Clone, Copy)]
enum FormSection {
    Name,
    Description,
    Url,
    Method,
    /// Editing param `idx`, sub-field `ParamSubField`
    Param(usize, ParamSubField),
    AddParam, // Button to add a new param row
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ParamSubField {
    Name,
    Description,
    Kind,
    Required,
}

// ─── Screen modes ────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum ScreenMode {
    List,
    Form,
    ConfirmDelete,
}

// ─── Main screen struct ──────────────────────────────────────────────────────

pub struct ServerToolsScreen {
    pub title: String,
    pub server_id: i64,
    pub server_name: String,
    pub list: NavigatableList,
    pub tools: Vec<McpServerTool>,
    pub persistence: Persistence,

    mode: ScreenMode,

    editing_index: Option<usize>,
    form_section: FormSection,

    // Basic form fields
    form_name: String,
    form_desc: String,
    form_url: String,
    form_method: String,

    // Param rows
    form_params: Vec<ParamEntry>,

    status_message: Option<String>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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
            form_section: FormSection::Name,
            form_name: String::new(),
            form_desc: String::new(),
            form_url: String::new(),
            form_method: "GET".to_string(),
            form_params: Vec::new(),
            status_message: None,
        }
    }

    fn build_list_options(tools: &[McpServerTool]) -> Vec<String> {
        let mut opts: Vec<String> = tools.iter().map(|t| t.tool_name.clone()).collect();
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
        self.form_params = Vec::new();
        self.form_section = FormSection::Name;
        self.mode = ScreenMode::Form;
        self.status_message = None;
    }

    fn open_edit_form(&mut self, idx: usize) {
        let tool_record = &self.tools[idx];
        self.editing_index = Some(idx);
        self.form_name = tool_record.tool_name.clone();
        self.form_params = Vec::new();

        if let Ok(def) = serde_json::from_str::<DynamicToolDef>(&tool_record.tool_def) {
            self.form_desc = def.description;
            self.form_url = def.tool.url.clone();
            self.form_method = match def.tool.method {
                HttpMethod::GET => "GET",
                HttpMethod::POST => "POST",
                HttpMethod::PUT => "PUT",
                HttpMethod::PATCH => "PATCH",
                HttpMethod::DELETE => "DELETE",
            }
            .to_string();

            // Re-hydrate params from ToolParam list, using name-based kind heuristics from URL
            if let Some(params) = def.parameters {
                for p in params {
                    let kind = Self::infer_kind(&def.tool.url, &p.name);
                    self.form_params.push(ParamEntry {
                        name: p.name,
                        description: p.description,
                        kind,
                        required: p.required,
                    });
                }
            }
        }

        self.form_section = FormSection::Name;
        self.mode = ScreenMode::Form;
        self.status_message = None;
    }

    /// Guess param kind: if name appears as `{name}` in the URL it's a path param, else query.
    fn infer_kind(url: &str, name: &str) -> ParamKind {
        if url.contains(&format!("{{{}}}", name)) {
            ParamKind::Path
        } else {
            ParamKind::Query
        }
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

        // Convert form params to ToolParam
        let tool_params: Vec<ToolParam> = self
            .form_params
            .iter()
            .filter(|p| !p.name.trim().is_empty())
            .map(|p| ToolParam {
                name: p.name.trim().to_string(),
                description: if p.description.trim().is_empty() {
                    format!("{} parameter: {}", p.kind.label(), p.name.trim())
                } else {
                    p.description.trim().to_string()
                },
                required: p.required,
                param_type: "string".to_string(),
            })
            .collect();

        // Build separate param maps for HttpProtocoal
        let mut path_params: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut query_params: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut body_keys: Vec<String> = Vec::new();

        for p in &self.form_params {
            let pname = p.name.trim().to_string();
            if pname.is_empty() { continue; }
            match p.kind {
                ParamKind::Path => { path_params.insert(pname.clone(), format!("{{{}}}", pname)); }
                ParamKind::Query => { query_params.insert(pname.clone(), String::new()); }
                ParamKind::Body => { body_keys.push(pname); }
            }
        }

        // Build body template if body params exist
        let body = if body_keys.is_empty() {
            None
        } else {
            let mut body_map = serde_json::Map::new();
            for k in &body_keys {
                body_map.insert(k.clone(), serde_json::Value::String(format!("{{{}}}", k)));
            }
            Some(serde_json::Value::Object(body_map))
        };

        let def = DynamicToolDef {
            name: name.clone(),
            description: self.form_desc.trim().to_string(),
            parameters: if tool_params.is_empty() { None } else { Some(tool_params) },
            action: ActionType::Http,
            tool: HttpProtocoal {
                method,
                url: self.form_url.trim().to_string(),
                query_params: if query_params.is_empty() { None } else { Some(query_params) },
                path_params: if path_params.is_empty() { None } else { Some(path_params) },
                body,
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

    fn add_param(&mut self) {
        let idx = self.form_params.len();
        self.form_params.push(ParamEntry::new());
        self.form_section = FormSection::Param(idx, ParamSubField::Name);
    }

    fn remove_param(&mut self, idx: usize) {
        if idx < self.form_params.len() {
            self.form_params.remove(idx);
            // Move focus back
            if self.form_params.is_empty() {
                self.form_section = FormSection::AddParam;
            } else {
                let new_idx = idx.min(self.form_params.len() - 1);
                self.form_section = FormSection::Param(new_idx, ParamSubField::Name);
            }
        }
    }

    // ─── Navigation helpers ───────────────────────────────────────────────────

    /// Total ordered sections for Tab navigation
    fn next_section(&self) -> FormSection {
        match self.form_section {
            FormSection::Name => FormSection::Description,
            FormSection::Description => FormSection::Url,
            FormSection::Url => FormSection::Method,
            FormSection::Method => {
                if self.form_params.is_empty() {
                    FormSection::AddParam
                } else {
                    FormSection::Param(0, ParamSubField::Name)
                }
            }
            FormSection::Param(idx, sub) => match sub {
                ParamSubField::Name => FormSection::Param(idx, ParamSubField::Description),
                ParamSubField::Description => FormSection::Param(idx, ParamSubField::Kind),
                ParamSubField::Kind => FormSection::Param(idx, ParamSubField::Required),
                ParamSubField::Required => {
                    if idx + 1 < self.form_params.len() {
                        FormSection::Param(idx + 1, ParamSubField::Name)
                    } else {
                        FormSection::AddParam
                    }
                }
            },
            FormSection::AddParam => FormSection::Name,
        }
    }

    fn prev_section(&self) -> FormSection {
        match self.form_section {
            FormSection::Name => FormSection::AddParam,
            FormSection::Description => FormSection::Name,
            FormSection::Url => FormSection::Description,
            FormSection::Method => FormSection::Url,
            FormSection::AddParam => {
                if self.form_params.is_empty() {
                    FormSection::Method
                } else {
                    let last = self.form_params.len() - 1;
                    FormSection::Param(last, ParamSubField::Required)
                }
            }
            FormSection::Param(idx, sub) => match sub {
                ParamSubField::Name => {
                    if idx == 0 {
                        FormSection::Method
                    } else {
                        FormSection::Param(idx - 1, ParamSubField::Required)
                    }
                }
                ParamSubField::Description => FormSection::Param(idx, ParamSubField::Name),
                ParamSubField::Kind => FormSection::Param(idx, ParamSubField::Description),
                ParamSubField::Required => FormSection::Param(idx, ParamSubField::Kind),
            },
        }
    }

    // ─── Input handlers ───────────────────────────────────────────────────────

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
        match key.code {
            // Global navigation
            KeyCode::Tab => { self.form_section = self.next_section(); return None; }
            KeyCode::BackTab => { self.form_section = self.prev_section(); return None; }
            KeyCode::Esc => { self.mode = ScreenMode::List; return None; }
            KeyCode::Enter => {
                match self.form_section {
                    FormSection::AddParam => { self.add_param(); return None; }
                    _ => { self.save_form(); return None; }
                }
            }
            _ => {}
        }

        match self.form_section {
            FormSection::Name => self.handle_text(&key, true, false),
            FormSection::Description => self.handle_text(&key, false, false),
            FormSection::Url => self.handle_text(&key, false, false),
            FormSection::Method => self.handle_text(&key, true, false),
            FormSection::AddParam => {} // Enter handled above
            FormSection::Param(idx, sub) => {
                if idx < self.form_params.len() {
                    match sub {
                        ParamSubField::Name => {
                            match key.code {
                                KeyCode::Char(c) => { self.form_params[idx].name.push(c); }
                                KeyCode::Backspace => { self.form_params[idx].name.pop(); }
                                KeyCode::Delete => { self.remove_param(idx); }
                                _ => {}
                            }
                        }
                        ParamSubField::Description => {
                            match key.code {
                                KeyCode::Char(c) => { self.form_params[idx].description.push(c); }
                                KeyCode::Backspace => { self.form_params[idx].description.pop(); }
                                _ => {}
                            }
                        }
                        ParamSubField::Kind => {
                            match key.code {
                                // Space / left-right cycles the kind
                                KeyCode::Char(' ') | KeyCode::Right | KeyCode::Left => {
                                    self.form_params[idx].kind = self.form_params[idx].kind.cycle_next();
                                }
                                _ => {}
                            }
                        }
                        ParamSubField::Required => {
                            match key.code {
                                KeyCode::Char(' ') | KeyCode::Enter => {
                                    self.form_params[idx].required = !self.form_params[idx].required;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Route a key event to the currently active top-level text field.
    fn handle_text(&mut self, key: &KeyEvent, _uppercase_hint: bool, _multiline: bool) {
        let target = match self.form_section {
            FormSection::Name => &mut self.form_name,
            FormSection::Description => &mut self.form_desc,
            FormSection::Url => &mut self.form_url,
            FormSection::Method => &mut self.form_method,
            _ => return,
        };
        match key.code {
            KeyCode::Char(c) => target.push(c),
            KeyCode::Backspace => { target.pop(); }
            _ => {}
        }
    }

    // ─── Rendering ────────────────────────────────────────────────────────────

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
        let popup_area = centered_rect(78, 90, area);
        frame.render_widget(Clear, popup_area);
        frame.render_widget(
            Block::default()
                .title(" ✏  Tool Form  [Tab] Next  [Enter] Save  [Esc] Cancel ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black)),
            popup_area,
        );

        let inner = Block::default()
            .title("")
            .borders(Borders::NONE)
            .inner(popup_area);

        // Shrink inner by 1 on all sides for padding
        let inner = Rect {
            x: inner.x + 1,
            y: inner.y + 1,
            width: inner.width.saturating_sub(2),
            height: inner.height.saturating_sub(2),
        };

        // --- Top fixed section (Name / Desc / URL / Method) ---
        let top_rows = 4u16; // each is 3 tall
        let top_height = top_rows * 3;

        // --- Param section header + rows + add button ---
        let param_count = self.form_params.len() as u16;
        let param_rows_height = param_count * 3; // each param row is 3 lines
        let param_header_height = 1u16;
        let add_btn_height = 1u16;
        let status_height = 1u16;

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(top_height),
                Constraint::Length(param_header_height),
                Constraint::Length(param_rows_height),
                Constraint::Length(add_btn_height),
                Constraint::Length(status_height),
                Constraint::Min(0),
            ])
            .split(inner);

        // ── Top fields ──────────────────────────────────────────────────────
        let top_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(chunks[0]);

        let fixed_fields: &[(&str, &str, FormSection)] = &[
            (&self.form_name, "Name", FormSection::Name),
            (&self.form_desc, "Description", FormSection::Description),
            (&self.form_url, "Endpoint URL", FormSection::Url),
            (&self.form_method, "Method  (GET / POST / PUT / PATCH / DELETE)", FormSection::Method),
        ];
        for (i, (val, title, section)) in fixed_fields.iter().enumerate() {
            let focused = self.form_section == *section;
            let color = if focused { Color::Yellow } else { Color::DarkGray };
            let prefix = if focused { "▶ " } else { "  " };
            frame.render_widget(
                Paragraph::new(format!("{}{}", prefix, val))
                    .block(Block::default().title(format!(" {} ", title)).borders(Borders::ALL).border_style(Style::default().fg(color))),
                top_chunks[i],
            );
        }

        // ── Params header ────────────────────────────────────────────────────
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Parameters ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("  [PATH] ", Style::default().fg(Color::LightRed)),
                Span::styled("[QUERY] ", Style::default().fg(Color::LightBlue)),
                Span::styled("[BODY] ", Style::default().fg(Color::LightGreen)),
                Span::styled("  ← Space to cycle type  ·  Del to remove", Style::default().fg(Color::DarkGray)),
            ])),
            chunks[1],
        );

        // ── Param rows ───────────────────────────────────────────────────────
        if !self.form_params.is_empty() {
            let param_constraints: Vec<Constraint> = self.form_params.iter().map(|_| Constraint::Length(3)).collect();
            let param_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(param_constraints)
                .split(chunks[2]);

            for (idx, param) in self.form_params.iter().enumerate() {
                let row = param_chunks[idx];

                // Split each row: [Type badge] [Name field] [Description field] [Req toggle]
                let row_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(9),  // Type badge
                        Constraint::Length(18), // Name
                        Constraint::Min(20),    // Description
                        Constraint::Length(10), // Required
                    ])
                    .split(row);

                // Type badge
                let kind_focused = self.form_section == FormSection::Param(idx, ParamSubField::Kind);
                let kind_color = if kind_focused { Color::White } else { param.kind.color() };
                frame.render_widget(
                    Paragraph::new(format!(" {}", param.kind.label()))
                        .block(Block::default()
                            .title(" Type ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if kind_focused { Color::Yellow } else { param.kind.color() })))
                        .style(Style::default().fg(kind_color).add_modifier(if kind_focused { Modifier::BOLD } else { Modifier::empty() })),
                    row_chunks[0],
                );

                // Name subfield
                let name_focused = self.form_section == FormSection::Param(idx, ParamSubField::Name);
                let name_prefix = if name_focused { "▶ " } else { "  " };
                frame.render_widget(
                    Paragraph::new(format!("{}{}", name_prefix, param.name))
                        .block(Block::default()
                            .title(" Name ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if name_focused { Color::Yellow } else { Color::DarkGray }))),
                    row_chunks[1],
                );

                // Description subfield
                let desc_focused = self.form_section == FormSection::Param(idx, ParamSubField::Description);
                let desc_prefix = if desc_focused { "▶ " } else { "  " };
                frame.render_widget(
                    Paragraph::new(format!("{}{}", desc_prefix, param.description))
                        .block(Block::default()
                            .title(" Description ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if desc_focused { Color::Yellow } else { Color::DarkGray }))),
                    row_chunks[2],
                );

                // Required toggle
                let req_focused = self.form_section == FormSection::Param(idx, ParamSubField::Required);
                let req_label = if param.required { "✔ Yes" } else { "✘ No " };
                let req_color = if param.required { Color::LightGreen } else { Color::DarkGray };
                frame.render_widget(
                    Paragraph::new(format!(" {}", req_label))
                        .block(Block::default()
                            .title(" Req? ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if req_focused { Color::Yellow } else { req_color })))
                        .style(Style::default().fg(req_color).add_modifier(if req_focused { Modifier::BOLD } else { Modifier::empty() })),
                    row_chunks[3],
                );
            }
        }

        // ── Add param button ─────────────────────────────────────────────────
        let add_focused = self.form_section == FormSection::AddParam;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                if add_focused {
                    Span::styled(" ▶ [ + Add Parameter ]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("   [ + Add Parameter ]", Style::default().fg(Color::Gray))
                },
            ])),
            chunks[3],
        );

        // ── Status message ───────────────────────────────────────────────────
        if let Some(msg) = &self.status_message {
            let color = if msg.contains('✓') { Color::LightGreen } else { Color::LightRed };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(format!(" {}", msg), Style::default().fg(color).add_modifier(Modifier::BOLD)))),
                chunks[4],
            );
        }
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
}

// ─── Screen trait impl ────────────────────────────────────────────────────────

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
                frame.render_widget(
                    Paragraph::new("  Confirm Delete Tool? (y/n)")
                        .block(Block::default().title(" ⚠ Delete ").borders(Borders::ALL).border_style(Style::default().fg(Color::Red)).style(Style::default().bg(Color::Black)))
                        .style(Style::default().fg(Color::White)),
                    popup,
                );
            }
        }
    }
}

// ─── Utility ──────────────────────────────────────────────────────────────────

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
