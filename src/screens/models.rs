use crate::model::groq::GroqModel;
use crate::model::model::Model as ModelTrait;
use crate::model::ollama::OllamaModel;
use crate::persistence::persistence::{Model, Persistence, Persistable};
use crate::screens::menu::MenuScreen;
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
use tokio::runtime::Builder as RuntimeBuilder;

// ─── Mode ────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum ScreenMode {
    /// Browsing the list of models
    List,
    /// Choosing the model type for a new model
    SelectType,
    /// Editing fields of a model (add or edit)
    Form,
    /// Confirming deletion of the selected model
    ConfirmDelete,
    /// Blocking connection test in progress (shown for one frame before save)
    TestingConnection,
}

// ─── Form field index ─────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone)]
enum FormField {
    ModelType,
    ApiKey,
}

// ─── ModelsScreen ────────────────────────────────────────────────────────────

pub struct ModelsScreen {
    pub title: String,
    pub list: NavigatableList,
    pub models: Vec<Model>,
    pub persistence: Persistence,

    mode: ScreenMode,

    // --- type-selection sub-state ---
    type_list: NavigatableList,

    // --- form sub-state ---
    /// Model currently being added / edited (None = adding new)
    editing_index: Option<usize>,
    form_field: FormField,
    form_model_type: String,
    form_api_key: String,

    // --- status bar message ---
    status_message: Option<String>,
}

// ─── Available model types ────────────────────────────────────────────────────

const MODEL_TYPES: &[&str] = &["Ollama", "Groq"];

impl ModelsScreen {
    pub fn new() -> Self {
        let persistence = Persistence::new();
        persistence.sync_schema();

        let models: Vec<Model> = persistence.get_all();
        let list_options = Self::build_list_options(&models);

        let mut list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: list_options,
        };
        if !list.options.is_empty() {
            list.state.select(Some(0));
        }

        let mut type_list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: MODEL_TYPES.iter().map(|s| s.to_string()).collect(),
        };
        type_list.state.select(Some(0));

        ModelsScreen {
            title: "Models".to_string(),
            list,
            models,
            persistence,
            mode: ScreenMode::List,
            type_list,
            editing_index: None,
            form_field: FormField::ModelType,
            form_model_type: String::new(),
            form_api_key: String::new(),
            status_message: None,
        }
    }

    // ── helpers ──────────────────────────────────────────────────────────────

    fn build_list_options(models: &[Model]) -> Vec<String> {
        let mut opts: Vec<String> = models
            .iter()
            .map(|m| {
                let key_hint = if m.api_key.as_deref().unwrap_or("").is_empty() {
                    " (no key)".to_string()
                } else {
                    " (key set)".to_string()
                };
                format!("{}{}", m.model_type, key_hint)
            })
            .collect();
        opts.push("[ + Add Model ]".to_string());
        opts
    }

    fn refresh_list(&mut self) {
        self.models = self.persistence.get_all();
        let opts = Self::build_list_options(&self.models);
        let prev = self.list.state.selected().unwrap_or(0);
        self.list.options = opts;
        let clamped = prev.min(self.list.options.len().saturating_sub(1));
        self.list.state.select(Some(clamped));
    }

    fn selected_model_index(&self) -> Option<usize> {
        let idx = self.list.state.selected()?;
        if idx < self.models.len() {
            Some(idx)
        } else {
            None
        }
    }

    fn open_add_form(&mut self) {
        self.editing_index = None;
        self.form_model_type = MODEL_TYPES[0].to_string();
        self.form_api_key = String::new();
        self.form_field = FormField::ModelType;
        self.type_list.state.select(Some(0));
        self.mode = ScreenMode::SelectType;
    }

    fn open_edit_form(&mut self, idx: usize) {
        let model = self.models[idx].clone();
        self.editing_index = Some(idx);
        self.form_model_type = model.model_type.clone();
        self.form_api_key = model.api_key.clone().unwrap_or_default();

        // Pre-select the type in the type list
        let type_idx = MODEL_TYPES
            .iter()
            .position(|t| *t == model.model_type.as_str())
            .unwrap_or(0);
        self.type_list.state.select(Some(type_idx));
        self.form_field = FormField::ApiKey; // skip type selection for edit, go to key
        self.mode = ScreenMode::Form;
    }

    /// Run the model's connect() in a blocking tokio runtime and return Ok/Err.
    fn test_connection_blocking(model_type: &str, api_key: &str) -> Result<(), String> {
        let rt = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Runtime error: {}", e))?;

        let result = match model_type {
            "Groq" => {
                let m = GroqModel::new(
                    "https://api.groq.com/openai/v1".to_string(),
                    api_key.to_string(),
                );
                rt.block_on(m.connect(Some(api_key.to_string())))
            }
            "Ollama" => {
                let m = OllamaModel::new();
                rt.block_on(m.connect(None))
            }
            other => return Err(format!("Unknown model type: {}", other)),
        };

        result.map_err(|e| e.to_string())
    }

    fn save_form(&mut self) {
        let api_key_str = self.form_api_key.trim().to_string();
        let api_key_opt = if api_key_str.is_empty() {
            None
        } else {
            Some(api_key_str.clone())
        };

        // ── Test connection first ──────────────────────────────────────────
        self.status_message = Some(" Testing connection… ".to_string());
        match Self::test_connection_blocking(&self.form_model_type, &api_key_str) {
            Ok(()) => {
                // Connection OK → persist
                match self.editing_index {
                    None => {
                        let model = Model {
                            id: None,
                            model_type: self.form_model_type.clone(),
                            api_key: api_key_opt,
                        };
                        self.persistence.save(&model);
                        self.status_message =
                            Some(format!("✓ Connected & saved {} model", self.form_model_type));
                    }
                    Some(idx) => {
                        let existing_id = self.models[idx].id;
                        let model = Model {
                            id: existing_id,
                            model_type: self.form_model_type.clone(),
                            api_key: api_key_opt,
                        };
                        self.persistence.update(&model);
                        self.status_message =
                            Some(format!("✓ Connected & updated {} model", self.form_model_type));
                    }
                }
                self.refresh_list();
                self.mode = ScreenMode::List;
            }
            Err(err) => {
                // Connection failed → stay in form, show error
                let short = if err.len() > 80 { format!("{}…", &err[..80]) } else { err };
                self.status_message = Some(format!("✗ Connection failed: {}", short));
                // remain in Form mode so user can fix the key
            }
        }
    }

    fn delete_selected(&mut self) {
        if let Some(idx) = self.selected_model_index() {
            if let Some(id) = self.models[idx].id {
                self.persistence.delete::<Model>(id);
                self.status_message =
                    Some(format!("✓ Deleted {} model", self.models[idx].model_type));
                self.refresh_list();
            }
        }
        self.mode = ScreenMode::List;
    }

    // ── input handlers per mode ───────────────────────────────────────────────

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
                if selected >= self.models.len() {
                    // "+ Add Model" row
                    self.open_add_form();
                }
                None
            }
            KeyCode::Char('a') => {
                self.open_add_form();
                None
            }
            KeyCode::Char('e') => {
                if let Some(idx) = self.selected_model_index() {
                    self.open_edit_form(idx);
                }
                None
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if self.selected_model_index().is_some() {
                    self.mode = ScreenMode::ConfirmDelete;
                }
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(ScreenAction::Switch(Box::new(MenuScreen::new()))),
            _ => None,
        }
    }

    fn handle_type_select_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.type_list.next();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.type_list.previous();
                None
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Confirm type selection, move to form
                let idx = self.type_list.state.selected().unwrap_or(0);
                self.form_model_type = MODEL_TYPES[idx].to_string();
                self.form_field = FormField::ApiKey;
                self.mode = ScreenMode::Form;
                None
            }
            KeyCode::Esc => {
                self.mode = ScreenMode::List;
                None
            }
            _ => None,
        }
    }

    fn handle_form_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match &self.form_field {
            FormField::ApiKey => match key.code {
                KeyCode::Char(c) => {
                    self.form_api_key.push(c);
                    None
                }
                KeyCode::Backspace => {
                    self.form_api_key.pop();
                    None
                }
                KeyCode::Enter | KeyCode::F(10) => {
                    self.save_form();
                    None
                }
                KeyCode::Tab => {
                    // Cycle back to type (only for add)
                    if self.editing_index.is_none() {
                        self.form_field = FormField::ModelType;
                        self.mode = ScreenMode::SelectType;
                    }
                    None
                }
                KeyCode::Esc => {
                    self.mode = ScreenMode::List;
                    None
                }
                _ => None,
            },
            FormField::ModelType => None, // handled in SelectType mode
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

    // ── render helpers ────────────────────────────────────────────────────────

    fn render_list(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        // Model list
        let items: Vec<ListItem> = self
            .list
            .options
            .iter()
            .enumerate()
            .map(|(i, label)| {
                if i < self.models.len() {
                    let model = &self.models[i];
                    let type_color = match model.model_type.as_str() {
                        "Groq" => Color::Cyan,
                        "Ollama" => Color::Green,
                        _ => Color::White,
                    };
                    let key_icon = if model.api_key.as_deref().unwrap_or("").is_empty() {
                        Span::styled(" ✗ key", Style::default().fg(Color::DarkGray))
                    } else {
                        Span::styled(" ✓ key", Style::default().fg(Color::Green))
                    };

                    let line = Line::from(vec![
                        Span::styled(
                            format!("  {:<8}", model.model_type),
                            Style::default().fg(type_color).add_modifier(Modifier::BOLD),
                        ),
                        key_icon,
                    ]);
                    ListItem::new(line)
                } else {
                    // "+ Add" row
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
                    .title(Span::styled(
                        format!(" {} ", self.title),
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .style(Style::default().bg(Color::Black)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, chunks[0], &mut self.list.state);

        // Footer
        self.render_list_footer(frame, chunks[1]);
    }

    fn render_list_footer(&self, frame: &mut Frame, area: Rect) {
        let status = self.status_message.as_deref().unwrap_or("");

        let keys = vec![
            (" ↑↓ ", "Navigate"),
            (" a ", "Add"),
            (" e ", "Edit"),
            (" d ", "Delete"),
            (" q ", "Back"),
        ];

        let mut spans: Vec<Span> = Vec::new();
        for (key, desc) in &keys {
            spans.push(Span::styled(
                *key,
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {} ", desc),
                Style::default().fg(Color::Gray),
            ));
            spans.push(Span::raw("  "));
        }

        let status_span = if status.starts_with('✓') {
            Span::styled(
                format!("  {}", status),
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!("  {}", status),
                Style::default().fg(Color::Yellow),
            )
        };
        spans.push(status_span);

        let footer = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .style(Style::default().bg(Color::Black)),
        );
        frame.render_widget(footer, area);
    }

    fn render_type_select(&mut self, frame: &mut Frame, area: Rect) {
        // Dim the background by rendering the list behind
        self.render_list(frame, area);

        // Popup
        let popup_area = centered_rect(40, 50, area);
        frame.render_widget(Clear, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(popup_area);

        let items: Vec<ListItem> = self
            .type_list
            .options
            .iter()
            .map(|t| ListItem::new(format!("  {}", t)))
            .collect();

        let type_list_widget = List::new(items)
            .block(
                Block::default()
                    .title(Span::styled(
                        " Select Model Type ",
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::Black)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(type_list_widget, chunks[0], &mut self.type_list.state);

        // Footer
        let footer = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Navigate  ", Style::default().fg(Color::Gray)),
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Select  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .style(Style::default().bg(Color::Black)),
        );
        frame.render_widget(footer, chunks[1]);
    }

    fn render_form(&mut self, frame: &mut Frame, area: Rect) {
        // Dim background
        self.render_list(frame, area);

        let popup_area = centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);

        let title = if self.editing_index.is_none() {
            " Add Model "
        } else {
            " Edit Model "
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // model type display
                Constraint::Length(3), // api key input
                Constraint::Length(1), // key hints
                Constraint::Length(1), // status message
            ])
            .split(
                Block::default()
                    .title(Span::styled(
                        title,
                        Style::default()
                            .fg(Color::LightYellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .style(Style::default().bg(Color::Black))
                    .inner(popup_area),
            );

        // Outer border
        frame.render_widget(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black)),
            popup_area,
        );

        // Model type row
        let type_block = Paragraph::new(Span::styled(
            format!("  {}", self.form_model_type),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .block(
            Block::default()
                .title(Span::styled(" Type ", Style::default().fg(Color::DarkGray)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .style(Style::default().bg(Color::Black)),
        );
        frame.render_widget(type_block, chunks[0]);

        // API key input row
        let api_key_display = if self.form_api_key.is_empty() {
            Span::styled(
                "  (leave blank for local models)",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )
        } else {
            // mask the key, show last 4 chars
            let masked = if self.form_api_key.len() > 4 {
                format!(
                    "  {}{}",
                    "•".repeat(self.form_api_key.len() - 4),
                    &self.form_api_key[self.form_api_key.len() - 4..]
                )
            } else {
                format!("  {}", "•".repeat(self.form_api_key.len()))
            };
            Span::styled(masked, Style::default().fg(Color::LightYellow))
        };

        let api_border_color = if self.form_field == FormField::ApiKey {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let api_block = Paragraph::new(Line::from(vec![api_key_display]))
            .block(
                Block::default()
                    .title(Span::styled(
                        " API Key ",
                        Style::default().fg(api_border_color),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(api_border_color))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(api_block, chunks[1]);

        // Key hints row
        let tab_hint = if self.editing_index.is_none() {
            vec![
                Span::styled(" Tab ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" Type  ", Style::default().fg(Color::Gray)),
            ]
        } else {
            vec![]
        };

        let mut footer_spans = tab_hint;
        footer_spans.extend(vec![
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Save  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ]);

        let hints = Paragraph::new(Line::from(footer_spans))
            .style(Style::default().bg(Color::Black).fg(Color::Gray));
        frame.render_widget(hints, chunks[2]);

        // Status / connection-test result row
        if let Some(msg) = &self.status_message {
            let (icon, color) = if msg.contains('✓') {
                ("", Color::LightGreen)
            } else if msg.contains('✗') {
                ("", Color::LightRed)
            } else {
                ("", Color::Yellow)
            };
            let status = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} {}", icon, msg.trim()),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]))
            .style(Style::default().bg(Color::Black));
            frame.render_widget(status, chunks[3]);
        }
    }

    fn render_confirm_delete(&mut self, frame: &mut Frame, area: Rect) {
        self.render_list(frame, area);

        let popup_area = centered_rect(50, 30, area);
        frame.render_widget(Clear, popup_area);

        let model_name = self
            .selected_model_index()
            .map(|i| self.models[i].model_type.clone())
            .unwrap_or_default();

        let text = format!("  Delete \"{}\" model?\n  This cannot be undone.", model_name);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(3)])
            .split(
                Block::default()
                    .title(Span::styled(
                        " ⚠ Confirm Delete ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red))
                    .style(Style::default().bg(Color::Black))
                    .inner(popup_area),
            );

        // outer border
        frame.render_widget(
            Block::default()
                .title(Span::styled(
                    " ⚠ Confirm Delete ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .style(Style::default().bg(Color::Black)),
            popup_area,
        );

        let confirm_paragraph = Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(Wrap { trim: false });
        frame.render_widget(confirm_paragraph, chunks[0]);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled(" y ", Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Yes, delete  ", Style::default().fg(Color::Gray)),
            Span::styled(" n ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ]))
        .style(Style::default().bg(Color::Black));
        frame.render_widget(footer, chunks[1]);
    }
}

// ─── Screen impl ─────────────────────────────────────────────────────────────

impl Screen for ModelsScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match self.mode {
            ScreenMode::List => self.handle_list_input(key),
            ScreenMode::SelectType => self.handle_type_select_input(key),
            ScreenMode::Form | ScreenMode::TestingConnection => self.handle_form_input(key),
            ScreenMode::ConfirmDelete => self.handle_confirm_delete_input(key),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self.mode {
            ScreenMode::List => self.render_list(frame, area),
            ScreenMode::SelectType => self.render_type_select(frame, area),
            ScreenMode::Form | ScreenMode::TestingConnection => self.render_form(frame, area),
            ScreenMode::ConfirmDelete => self.render_confirm_delete(frame, area),
        }
    }
}

// ─── Layout utility ──────────────────────────────────────────────────────────

/// Returns a centered `Rect` of the given percentage of the parent area.
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