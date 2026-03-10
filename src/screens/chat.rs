use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use serde_json::Value;

use crate::dyns::dynamictool::DynamicToolDef;
use crate::generators::client::ClientGenerator;
use crate::model::groq::GroqModel;
use crate::model::model::{ChatMessage, Model as AIModel};
use crate::model::ollama::OllamaModel;
use crate::persistence::persistence::{McpServer, Model as PersistentModel, Persistence};
use crate::screens::mcp_servers::published_ports;
use crate::screens::menu::MenuScreen;
use crate::ui::navigation::NavigatableList;
use crate::ui::screen::{Screen, ScreenAction};

#[derive(Debug, PartialEq)]
enum ScreenMode {
    ModelSelection,
    ServerSelection,
    Chatting,
}

pub struct ChatScreen {
    mode: ScreenMode,
    
    // UI Lists
    models: Vec<PersistentModel>,
    model_list: NavigatableList,

    servers: Vec<(McpServer, u16)>, // (Server, Port)
    server_list: NavigatableList,

    // Chat session state
    selected_model: Option<PersistentModel>,
    selected_server: Option<(McpServer, u16)>,
    chat_history: Vec<(String, String)>, // (Role, Message) used for UI display
    input_text: String,
    
    // Async worker channel
    tx: Option<mpsc::Sender<(String, String)>>, // To send user inputs to the bg thread
    rx: Option<mpsc::Receiver<(String, String)>>, // To receive UI updates (Role, Message) from bg thread
    is_loading: bool,

    background_thread: Option<thread::JoinHandle<()>>,
}

impl ChatScreen {
    pub fn new() -> Self {
        let persistence = Persistence::new();
        let models: Vec<PersistentModel> = persistence.get_all();
        
        let list_options: Vec<String> = models.iter().map(|m| {
            format!("{} {}", m.model_type, if m.api_key.is_some() { "[AUTHED]" } else { "" })
        }).collect();

        let mut model_list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options: list_options,
        };
        if !model_list.options.is_empty() {
            model_list.state.select(Some(0));
        }

        ChatScreen {
            mode: ScreenMode::ModelSelection,
            models,
            model_list,
            servers: Vec::new(),
            server_list: NavigatableList { state: ratatui::widgets::ListState::default(), options: vec![] },
            selected_model: None,
            selected_server: None,
            chat_history: Vec::new(),
            input_text: String::new(),
            tx: None,
            rx: None,
            is_loading: false,
            background_thread: None,
        }
    }

    fn prepare_server_selection(&mut self) {
        let persistence = Persistence::new();
        let all_servers: Vec<McpServer> = persistence.get_all();
        
        self.servers.clear();
        let mut options = vec!["[ Skip - No Server ]".to_string()];

        if let Ok(ports) = published_ports().read() {
            for server in all_servers {
                if let Some(id) = server.id {
                    if let Some(port) = ports.get(&id) {
                        options.push(format!("{} (Port: {})", server.name, port));
                        self.servers.push((server, *port));
                    }
                }
            }
        }

        self.server_list = NavigatableList {
            state: ratatui::widgets::ListState::default(),
            options,
        };
        self.server_list.state.select(Some(0));
        self.mode = ScreenMode::ServerSelection;
    }

    fn start_chat_session(&mut self) {
        self.mode = ScreenMode::Chatting;
        self.chat_history.push(("System".to_string(), format!("Chat started with {}", self.selected_model.as_ref().unwrap().model_type)));
        if let Some((srv, port)) = &self.selected_server {
            self.chat_history.push(("System".to_string(), format!("Connected to MCP Server: {} on port {}", srv.name, port)));
        }

        let (ui_tx, ui_rx) = mpsc::channel(); // Updates sent TO the UI
        let (bg_tx, bg_rx) = mpsc::channel(); // Inputs sent TO the background thread

        self.rx = Some(ui_rx);
        self.tx = Some(bg_tx);

        let model_config = self.selected_model.as_ref().unwrap().clone();
        let server_config = self.selected_server.clone();

        self.background_thread = Some(thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let mut history: Vec<ChatMessage> = Vec::new();
                let mut mcp_client: Option<ClientGenerator> = None;
                let mut mcp_tools: Option<Value> = None;
                let mut tool_name_map = HashMap::new();

                // 1. Initialize MCP Client if selected
                if let Some((ref srv, port)) = server_config {
                    let client = ClientGenerator::new(&format!("http://localhost:{}/mcp", port));
                    if let Ok(running_client) = client.connect().await {
                        let _ = ui_tx.send(("System".to_string(), "Fetched tools from MCP Server...".to_string()));
                        
                        // Let's get tools using rmcp's list_tools
                        if let Ok(list) = running_client.list_tools(None).await {
                           let mut groq_tools = Vec::new();
                           for t in list.tools {
                               let safe_name = t.name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
                               // OpenAI / Groq tool schema
                               let schema = serde_json::json!({
                                   "type": "function",
                                   "function": {
                                       "name": safe_name.clone(),
                                       "description": t.description.unwrap_or_default(),
                                       "parameters": t.input_schema
                                   }
                               });
                               groq_tools.push(schema);
                               tool_name_map.insert(safe_name.clone(), t.name.clone());
                           }
                           
                           if model_config.model_type == "Ollama" {
                               // Ollama expects slightly different payload or bare tools usually, but OpenAI format is mostly supported in edge tools payload as well.
                               // Let's use the standard openAI wrapper for both.
                               mcp_tools = Some(serde_json::Value::Array(groq_tools));
                           } else {
                               mcp_tools = Some(serde_json::Value::Array(groq_tools));
                           }
                        }
                    } else {
                        let _ = ui_tx.send(("System".to_string(), "Failed to connect to MCP Server! Continuing without tools.".to_string()));
                    }
                    mcp_client = Some(client);
                }

                // 2. Chat Processing Loop
                // Wait for a user message from the UI
                while let Ok((role, content)) = bg_rx.recv() {
                    if role == "EXIT" { break; }
                    
                    history.push(ChatMessage {
                        role: "user".to_string(),
                        content: content.clone(),
                        tool_call_id: None,
                        tool_calls: None,
                    });

                    // Recursive loop to process tool choices
                    loop {
                        let result: Result<Value, _> = match model_config.model_type.as_str() {
                            "Groq" => {
                                let m = GroqModel::new("https://api.groq.com/openai/v1".to_string(), model_config.api_key.clone().unwrap_or_default());
                                let models = m.list_models().await.unwrap_or_default();
                                let target_model = models.into_iter().next().unwrap_or("llama3-8b-8192".to_string());
                                m.chat(history.clone(), target_model, mcp_tools.clone()).await
                            }
                            "Ollama" | _ => {
                                let m = OllamaModel::new();
                                let models = m.list_models().await.unwrap_or_default();
                                let target_model = models.into_iter().next().unwrap_or("llama3.1".to_string());
                                m.chat(history.clone(), target_model, mcp_tools.clone()).await
                            }
                        };

                        match result {
                            Ok(reply_val) => {
                                // Extract tool calls and message
                                // OpenAI/Groq response parsing
                                if let Some(choices) = reply_val.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(msg) = choices.get(0).and_then(|c| c.get("message")) {
                                        // Update history with Assistants intermediate response
                                        let content_str = msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                                        let tool_calls = msg.get("tool_calls").cloned();
                                        
                                        history.push(ChatMessage {
                                            role: "assistant".to_string(),
                                            content: content_str.clone(),
                                            tool_calls: tool_calls.clone(),
                                            tool_call_id: None,
                                        });

                                        if let Some(calls) = tool_calls.as_ref().and_then(|t| t.as_array()) {
                                            for call in calls {
                                                if let (Some(id), Some(func)) = (call.get("id").and_then(|i| i.as_str()), call.get("function")) {
                                                    let func_name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                                    let mut actual_name = func_name.to_string();
                                                    if let Some(mapped) = tool_name_map.get(func_name) {
                                                        actual_name = mapped.to_string();
                                                    }
                                                    let args_str = func.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                                                    let _ = ui_tx.send(("System".to_string(), format!("⚙️ Executing tool: {}", actual_name)));
                                                    
                                                    // Execute
                                                    let mut output = "Tool execution failed".to_string();
                                                    if let Some(ref client) = mcp_client {
                                                        if let Ok(args_map) = serde_json::from_str::<HashMap<String, serde_json::Value>>(args_str) {
                                                            let str_args = args_map.into_iter().map(|(k, v)| (k, v.as_str().unwrap_or("").to_string())).collect();
                                                            if let Some(res) = client.call_tool(&actual_name, str_args).await {
                                                                output = res;
                                                            }
                                                        }
                                                    }

                                                    // Push result
                                                    history.push(ChatMessage {
                                                        role: "tool".to_string(),
                                                        content: output,
                                                        tool_call_id: Some(id.to_string()),
                                                        tool_calls: None,
                                                    });
                                                }
                                            }
                                            // Loop back around to let the model generate the final response
                                            continue;
                                        } else {
                                            // Final Text Response
                                            let _ = ui_tx.send(("AI".to_string(), content_str));
                                        }
                                    }
                                } else if let Some(msg) = reply_val.get("message") {
                                    // Ollama response parsing
                                    let content_str = msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                                    let tool_calls = msg.get("tool_calls").cloned();

                                    history.push(ChatMessage {
                                        role: "assistant".to_string(),
                                        content: content_str.clone(),
                                        tool_calls: tool_calls.clone(),
                                        tool_call_id: None,
                                    });

                                    if let Some(calls) = tool_calls.as_ref().and_then(|t| t.as_array()) {
                                        for call in calls {
                                            if let Some(func) = call.get("function") {
                                                let func_name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                                let mut actual_name = func_name.to_string();
                                                if let Some(mapped) = tool_name_map.get(func_name) {
                                                    actual_name = mapped.to_string();
                                                }
                                                let args_map = func.get("arguments").and_then(|a| a.as_object());
                                                let _ = ui_tx.send(("System".to_string(), format!("⚙️ Executing tool: {}", actual_name)));
                                                
                                                let mut output = "Tool execution failed".to_string();
                                                if let Some(ref client) = mcp_client {
                                                    if let Some(args) = args_map {
                                                        let str_args = args.into_iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
                                                        if let Some(res) = client.call_tool(&actual_name, str_args).await {
                                                            output = res;
                                                        }
                                                    }
                                                }

                                                history.push(ChatMessage {
                                                    role: "tool".to_string(),
                                                    content: output,
                                                    tool_call_id: None,
                                                    tool_calls: None,
                                                });
                                            }
                                        }
                                        // Iterate
                                        continue;
                                    } else {
                                        let _ = ui_tx.send(("AI".to_string(), content_str));
                                    }
                                } else {
                                    let _ = ui_tx.send(("System".to_string(), format!("Failed to parse response: {}", reply_val)));
                                }
                            }
                            Err(e) => {
                                let _ = ui_tx.send(("System".to_string(), format!("API Error: {}", e)));
                            }
                        }
                        // Break out of the recursive tool loop if complete
                        break;
                    }
                    
                    // Signal the UI that generation is done
                    let _ = ui_tx.send(("DONE".to_string(), "".to_string()));
                }
            });
        }));
    }

    fn handle_model_selection_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.model_list.next();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.model_list.previous();
                None
            }
            KeyCode::Enter => {
                if let Some(idx) = self.model_list.state.selected() {
                    if idx < self.models.len() {
                        self.selected_model = Some(self.models[idx].clone());
                        self.prepare_server_selection();
                    }
                }
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(ScreenAction::Switch(Box::new(MenuScreen::new()))),
            _ => None,
        }
    }

    fn handle_server_selection_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.server_list.next();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.server_list.previous();
                None
            }
            KeyCode::Enter => {
                if let Some(idx) = self.server_list.state.selected() {
                    if idx == 0 {
                        // Skip
                        self.selected_server = None;
                    } else if idx - 1 < self.servers.len() {
                        self.selected_server = Some(self.servers[idx - 1].clone());
                    }
                    self.start_chat_session();
                }
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.mode = ScreenMode::ModelSelection;
                None
            }
            _ => None,
        }
    }


    fn handle_chat_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        if self.is_loading {
            // Drop input while waiting for LLM, except Esc to exit chat
            match key.code {
                KeyCode::Esc => {
                    if let Some(tx) = &self.tx {
                        let _ = tx.send(("EXIT".to_string(), "".to_string()));
                    }
                    self.mode = ScreenMode::ModelSelection;
                    return None;
                }
                _ => return None,
            }
        }

        match key.code {
            KeyCode::Char(c) => {
                self.input_text.push(c);
                None
            }
            KeyCode::Backspace => {
                self.input_text.pop();
                None
            }
            KeyCode::Enter => {
                let msg = self.input_text.trim().to_string();
                if !msg.is_empty() {
                    self.chat_history.push(("You".to_string(), msg.clone()));
                    self.input_text.clear();
                    self.is_loading = true;
                    if let Some(tx) = &self.tx {
                        let _ = tx.send(("user".to_string(), msg));
                    }
                }
                None
            }
            KeyCode::Esc => {
                if let Some(tx) = &self.tx {
                    let _ = tx.send(("EXIT".to_string(), "".to_string()));
                }
                return Some(ScreenAction::Switch(Box::new(MenuScreen::new())));
            }
            _ => None,
        }
    }

    fn pump_events(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok((role, reply)) = rx.try_recv() {
                if role == "DONE" {
                    self.is_loading = false;
                } else {
                    self.chat_history.push((role, reply));
                }
            }
        }
    }

    fn render_list_view(frame: &mut Frame, area: Rect, title: &str, list: &mut NavigatableList) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        let items: Vec<ListItem> = list.options.iter().enumerate().map(|(i, t)| {
            let content = if Some(i) == list.state.selected() {
                Line::from(vec![Span::styled(format!(">> {} ", t), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))])
            } else {
                Line::from(vec![Span::raw(format!("   {} ", t))])
            };
            ListItem::new(content)
        }).collect();

        let widget = List::new(items)
            .block(Block::default().title(format!(" {} ", title)).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));

        frame.render_stateful_widget(widget, chunks[0], &mut list.state);

        let footer = Line::from(vec![
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Select  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc/q ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Back ", Style::default().fg(Color::Gray)),
        ]);
        frame.render_widget(Paragraph::new(footer).style(Style::default().bg(Color::Black)), chunks[1]);
    }

    fn render_chat(&mut self, frame: &mut Frame, area: Rect) {
        self.pump_events();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(area);

        let mut log_lines = Vec::new();
        for (role, msg) in &self.chat_history {
            let color = match role.as_str() {
                "You" => Color::Green,
                "AI" => Color::Magenta,
                "System" => Color::DarkGray,
                _ => Color::DarkGray,
            };
            
            if !msg.is_empty() {
                log_lines.push(Line::from(vec![Span::styled(format!("{}: ", role), Style::default().fg(color).add_modifier(Modifier::BOLD))]));
                for line in msg.lines() {
                    log_lines.push(Line::from(vec![Span::raw(line)]));
                }
                log_lines.push(Line::from(vec![Span::raw("")]));
            }
        }

        if self.is_loading {
            log_lines.push(Line::from(vec![Span::styled("AI is thinking...", Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC))]));
        }

        let log_height = log_lines.len() as u16;
        let view_height = chunks[0].height.saturating_sub(2);
        let scroll_offset = if log_height > view_height { log_height - view_height } else { 0 };

        let server_title = self.selected_server.as_ref().map(|s| format!(" [+{} MCP]", s.0.name)).unwrap_or_default();
        let log_block = Paragraph::new(log_lines)
            .block(Block::default().title(format!(" Chat with {}{} ", self.selected_model.as_ref().unwrap().model_type, server_title)).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
            .scroll((scroll_offset, 0));

        frame.render_widget(log_block, chunks[0]);

        let input_title = if self.is_loading { " Waiting for AI... " } else { " Type a message (Enter to send) " };
        let input_color = if self.is_loading { Color::DarkGray } else { Color::Yellow };
        let input_text = if self.is_loading { String::new() } else { format!("> {}", self.input_text) };

        let input_block = Paragraph::new(input_text)
            .block(Block::default().title(input_title).borders(Borders::ALL).border_style(Style::default().fg(input_color)));

        frame.render_widget(input_block, chunks[1]);
    }
}

impl Screen for ChatScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match self.mode {
            ScreenMode::ModelSelection => self.handle_model_selection_input(key),
            ScreenMode::ServerSelection => self.handle_server_selection_input(key),
            ScreenMode::Chatting => self.handle_chat_input(key),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self.mode {
            ScreenMode::ModelSelection => Self::render_list_view(frame, area, "Select AI Model for Chat", &mut self.model_list),
            ScreenMode::ServerSelection => Self::render_list_view(frame, area, "Select MCP Server", &mut self.server_list),
            ScreenMode::Chatting => self.render_chat(frame, area),
        }
    }
}
