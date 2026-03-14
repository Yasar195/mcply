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
    ModelSwitching,
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
    selected_provider_model: String, // The actual model name to use from the provider
    chat_history: Vec<(String, String)>, // (Role, Message) used for UI display
    input_text: String,
    
    // Model switcher state
    available_models: Vec<String>, // Models from the provider
    switching_is_loading: bool,
    
    // Async worker channel
    tx: Option<mpsc::Sender<(String, String, String)>>, // To send user inputs to the bg thread (role, message, model_name)
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
            selected_provider_model: String::new(),
            chat_history: Vec::new(),
            input_text: String::new(),
            available_models: Vec::new(),
            switching_is_loading: false,
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

        // Initialize the provider model name
        if let Some(model) = &self.selected_model {
            let model_type = model.model_type.clone();
            let api_key = model.api_key.clone();
            
            // Create tokio runtime and fetch available models
            let rt = tokio::runtime::Runtime::new().unwrap();
            let models = rt.block_on(Self::fetch_available_models(&model_type, api_key));
            
            if let Some(first_model) = models.first() {
                self.selected_provider_model = first_model.clone();
            } else {
                // Fallback to a default model
                self.selected_provider_model = match model_type.as_str() {
                    "Groq" => "llama3-8b-8192".to_string(),
                    _ => "llama3.1".to_string(),
                };
            }
        }

        let (ui_tx, ui_rx) = mpsc::channel(); // Updates sent TO the UI
        let (bg_tx, bg_rx) = mpsc::channel(); // Inputs sent TO the background thread

        self.rx = Some(ui_rx);
        self.tx = Some(bg_tx);

        let model_config = self.selected_model.as_ref().unwrap().clone();
        let server_config = self.selected_server.clone();
        let selected_model_name = self.selected_provider_model.clone();

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

                // Add system message separately (don't store in history to avoid duplication)
                let system_message = if mcp_tools.is_some() {
                    Some(ChatMessage {
                        role: "system".to_string(),
                        content: Some("You are a helpful assistant with access to tools. You MUST use the available tools to answer user questions. 

CRITICAL TOOL INSTRUCTIONS:
1. ALWAYS use the native JSON tool-calling format. NEVER write out plain text explaining what you will do.
2. DO NOT hallucinate, guess, or make up placeholder data (e.g. fake todo lists). You must fetch real data using the tools.
3. If a tool has optional parameters and the user didn't specify them (e.g., they just said 'fetch todos'), DO NOT ASK for more information. YOU MUST CALL THE TOOL IMMEDIATELY with an empty arguments object `{}` or by omitting the optional fields.
4. Calling a tool with no arguments is completely valid and expected when parameters are optional. Do not hesitate.".to_string()),
                        tool_call_id: None,
                        tool_calls: None,
                    })
                } else {
                    None
                };

                // 2. Chat Processing Loop
                // Wait for a user message from the UI
                while let Ok((role, content, current_model_name)) = bg_rx.recv() {
                    if role == "EXIT" { break; }
                    
                    // Update the model name if a new one was sent
                    let active_model_name = if current_model_name.is_empty() { 
                        selected_model_name.clone() 
                    } else { 
                        current_model_name 
                    };
                    
                    history.push(ChatMessage {
                        role: "user".to_string(),
                        content: Some(content.clone()),
                        tool_call_id: None,
                        tool_calls: None,
                    });

                    // Recursive loop to process tool choices
                    let mut retry_without_tools = false;
                    let mut last_tool_calls: Vec<String> = Vec::new();
                    let mut tool_call_repeat_count = 0;
                    loop {
                        // Build messages to send: system message + optimized history
                        // When retrying without tools, send ONLY the current user message to avoid format errors
                        let messages_to_send = if retry_without_tools {
                            // Retry mode: just the current user message for models that don't support tools
                            vec![ChatMessage {
                                role: "user".to_string(),
                                content: Some(content.clone()),
                                tool_call_id: None,
                                tool_calls: None,
                            }]
                        } else {
                            // Normal mode: system + optimized history
                            // First build with system + full history
                            let mut msgs = Vec::new();
                            if let Some(ref sys_msg) = system_message {
                                msgs.push(sys_msg.clone());
                            }
                            msgs.extend(history.clone());
                            
                            // Then optimize to keep only recent context
                            ChatMessage::optimize_messages(msgs, 15)
                        };
                        
                        // Log token estimate (rough: ~4 chars per token)
                        let estimated_tokens = messages_to_send.iter()
                            .map(|m| m.estimate_tokens())
                            .sum::<usize>();
                        if estimated_tokens > 4000 {
                            let _ = ui_tx.send(("System".to_string(), format!("⚠️  Large request (~{} tokens). Sending may be slow or fail with rate limits.", estimated_tokens)));
                        }
                        
                        let result: Result<Value, _> = match model_config.model_type.as_str() {
                            "Groq" => {
                                let m = GroqModel::new("https://api.groq.com/openai/v1".to_string(), model_config.api_key.clone().unwrap_or_default());
                                m.chat(messages_to_send, active_model_name.clone(), mcp_tools.clone()).await
                            }
                            "Ollama" | _ => {
                                let m = OllamaModel::new();
                                m.chat(messages_to_send, active_model_name.clone(), mcp_tools.clone()).await
                            }
                        };

                        match result {
                            Ok(reply_val) => {
                                // Extract tool calls and message
                                // OpenAI/Groq response parsing
                                if let Some(choices) = reply_val.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(msg) = choices.get(0).and_then(|c| c.get("message")) {
                                        // Update history with Assistants intermediate response
                                        // content is null in Groq response when tool_calls is set — preserve that as None
                                        let content_opt = msg.get("content").and_then(|c| c.as_str()).map(|s| s.to_string());
                                        let tool_calls = msg.get("tool_calls").cloned();
                                        // Use the text content for display; send None to API when tool_calls is set
                                        let content_str = content_opt.clone().unwrap_or_default();
                                        
                                        history.push(ChatMessage {
                                            role: "assistant".to_string(),
                                            // Groq requires null (not "") when tool_calls is present
                                            content: if tool_calls.is_some() { None } else { content_opt },
                                            tool_calls: tool_calls.clone(),
                                            tool_call_id: None,
                                        });

                                        if let Some(calls) = tool_calls.as_ref().and_then(|t| t.as_array()) {
                                            // Check for repeated tool calls (safety against infinite loops)
                                            // Include arguments in the signature to detect same-tool-different-params loops
                                            let current_tool_calls: Vec<String> = calls.iter()
                                                .filter_map(|c| {
                                                    let name = c.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str())?;
                                                    let args = c.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("{}");
                                                    Some(format!("{}:{}", name, args))
                                                })
                                                .collect();

                                            eprintln!("{:?}", current_tool_calls);
                                            
                                            if current_tool_calls == last_tool_calls {
                                                tool_call_repeat_count += 1;
                                                if tool_call_repeat_count >= 1 {
                                                    // Same tool+args called again - break to prevent infinite loop
                                                    let _ = ui_tx.send(("System".to_string(), "⚠️  Stopped: Tool calling loop detected. Model is repeating same tool calls.".to_string()));
                                                    break;
                                                }
                                            } else {
                                                tool_call_repeat_count = 0;
                                            }
                                            last_tool_calls = current_tool_calls;
                                            
                                            for call in calls {
                                                if let Some(func) = call.get("function") {
                                                    // use a fallback ID if the API omits it to ensure tool execution proceeds
                                                    let id = call.get("id").and_then(|i| i.as_str()).unwrap_or("call_fallback");
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

                                                    // Truncate large tool outputs before storing to keep token usage low
                                                    let truncated_output = if output.len() > 2000 {
                                                        format!("(truncated) {}", &output[..2000])
                                                    } else {
                                                        output.clone()
                                                    };
                                                    // Push truncated result
                                                    history.push(ChatMessage {
                                                        role: "tool".to_string(),
                                                        content: Some(truncated_output),
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
                                    let content_opt = msg.get("content").and_then(|c| c.as_str()).map(|s| s.to_string());
                                    let tool_calls = msg.get("tool_calls").cloned();
                                    let content_str = content_opt.clone().unwrap_or_default();

                                    history.push(ChatMessage {
                                        role: "assistant".to_string(),
                                        content: if tool_calls.is_some() { None } else { content_opt },
                                        tool_calls: tool_calls.clone(),
                                        tool_call_id: None,
                                    });

                                    if let Some(calls) = tool_calls.as_ref().and_then(|t| t.as_array()) {
                                        // Check for repeated tool calls (safety against infinite loops)
                                        // Include arguments in the signature to detect same-tool-different-params loops
                                        let current_tool_calls: Vec<String> = calls.iter()
                                            .filter_map(|c| {
                                                let func = c.get("function")?;
                                                let name = func.get("name").and_then(|n| n.as_str())?;
                                                // Ollama args come as an object, serialize for comparison
                                                let args = func.get("arguments").map(|a| a.to_string()).unwrap_or_default();
                                                Some(format!("{}:{}", name, args))
                                            })
                                            .collect();
                                        
                                        if current_tool_calls == last_tool_calls {
                                            tool_call_repeat_count += 1;
                                            if tool_call_repeat_count >= 1 {
                                                // Same tool+args called again - break to prevent infinite loop
                                                let _ = ui_tx.send(("System".to_string(), "⚠️  Stopped: Tool calling loop detected. Model is repeating same tool calls.".to_string()));
                                                break;
                                            }
                                        } else {
                                            tool_call_repeat_count = 0;
                                        }
                                        last_tool_calls = current_tool_calls;
                                        
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
                                                    content: Some(output),
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
                                let error_msg = e.to_string();
                                eprintln!("Chat error: {}", error_msg); // Debug logging
                                
                                // If error is about tool calling, retry without tools
                                if (error_msg.contains("tool") || error_msg.contains("Tool")) && mcp_tools.is_some() && !retry_without_tools {
                                    let _ = ui_tx.send(("System".to_string(), "Model doesn't support tools, retrying without MCP...".to_string()));
                                    mcp_tools = None;
                                    retry_without_tools = true;
                                    // Don't break - continue the loop to retry without tools
                                    continue;
                                }
                                
                                let _ = ui_tx.send(("System".to_string(), format!("API Error: {}", error_msg)));
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
                        let _ = tx.send(("EXIT".to_string(), "".to_string(), String::new()));
                    }
                    self.mode = ScreenMode::ModelSelection;
                    return None;
                }
                _ => return None,
            }
        }

        match key.code {
            KeyCode::Tab => {
                // Open model switcher
                self.mode = ScreenMode::ModelSwitching;
                self.switching_is_loading = true;
                self.available_models.clear();
                self.model_list.options.clear();
                self.model_list.state.select(Some(0));
                
                if let Some(model) = &self.selected_model {
                    let model_type = model.model_type.clone();
                    let api_key = model.api_key.clone();
                    
                    // Create tokio runtime and fetch models
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let models = rt.block_on(Self::fetch_available_models(&model_type, api_key));
                    
                    self.available_models = models.clone();
                    self.model_list.options = models;
                    if !self.model_list.options.is_empty() {
                        self.model_list.state.select(Some(0));
                    }
                }
                
                self.switching_is_loading = false;
                None
            }
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
                        let _ = tx.send(("user".to_string(), msg, self.selected_provider_model.clone()));
                    }
                }
                None
            }
            KeyCode::Esc => {
                if let Some(tx) = &self.tx {
                    let _ = tx.send(("EXIT".to_string(), "".to_string(), String::new()));
                }
                return Some(ScreenAction::Switch(Box::new(MenuScreen::new())));
            }
            _ => None,
        }
    }

    fn handle_model_switching_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
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
                    if idx < self.available_models.len() {
                        let selected_model_name = self.available_models[idx].clone();
                        self.selected_provider_model = selected_model_name.clone();
                        self.chat_history.push(("System".to_string(), format!("Switched to model: {}", selected_model_name)));
                    }
                }
                self.mode = ScreenMode::Chatting;
                None
            }
            KeyCode::Esc => {
                self.mode = ScreenMode::Chatting;
                None
            }
            _ => None,
        }
    }

    async fn fetch_available_models(model_type: &str, api_key: Option<String>) -> Vec<String> {
        match model_type {
            "Groq" => {
                let groq = GroqModel::new(
                    "https://api.groq.com/openai/v1".to_string(),
                    api_key.unwrap_or_default()
                );
                let models = groq.list_models().await.unwrap_or_default();
                // Filter out non-chat models: audio, embedding, vision-only, safety/moderation models
                models.into_iter()
                    .filter(|m| {
                        !m.contains("whisper") 
                            && !m.contains("embed") 
                            && !m.contains("vision")
                            && !m.contains("guard")
                            && !m.contains("safeguard")
                            && !m.contains("classify")
                    })
                    .collect()
            }
            "Ollama" | _ => {
                let ollama = OllamaModel::new();
                let models = ollama.list_models().await.unwrap_or_default();
                // Filter out non-chat models
                models.into_iter()
                    .filter(|m| !m.contains("embed") && !m.contains("vision"))
                    .collect()
            }
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

        let input_title = if self.is_loading { " Waiting for AI... " } else { " Type a message (Enter to send, Tab to switch model) " };
        let input_color = if self.is_loading { Color::DarkGray } else { Color::Yellow };
        let input_text = if self.is_loading { String::new() } else { format!("> {}", self.input_text) };

        let input_block = Paragraph::new(input_text)
            .block(Block::default().title(input_title).borders(Borders::ALL).border_style(Style::default().fg(input_color)));

        frame.render_widget(input_block, chunks[1]);
    }

    fn render_model_switcher(&mut self, frame: &mut Frame, area: Rect) {
        // Render the chat in the background
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

        // Render model switcher overlay on top
        let popup_width = 50;
        let popup_height = (self.available_models.len() as u16).min(15) + 4;
        
        let popup_left = (area.width.saturating_sub(popup_width)) / 2;
        let popup_top = (area.height.saturating_sub(popup_height)) / 2;
        
        let popup_area = Rect {
            x: popup_left,
            y: popup_top,
            width: popup_width,
            height: popup_height,
        };

        // Render background overlay (semi-transparent effect via darkened background)
        let overlay_block = Block::default()
            .style(Style::default().bg(Color::Black).fg(Color::Gray))
            .title(" Switch Model ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let items: Vec<ListItem> = self.available_models.iter().enumerate().map(|(i, model)| {
            let content = if Some(i) == self.model_list.state.selected() {
                Line::from(vec![Span::styled(format!(">> {} ", model), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))])
            } else {
                Line::from(vec![Span::raw(format!("   {} ", model))])
            };
            ListItem::new(content)
        }).collect();

        let list_widget = List::new(items)
            .block(overlay_block)
            .style(Style::default().bg(Color::Black));

        frame.render_stateful_widget(list_widget, popup_area, &mut self.model_list.state);

        // Render footer
        let footer_area = Rect {
            x: popup_left,
            y: popup_top + popup_height.saturating_sub(1),
            width: popup_width,
            height: 1,
        };

        let footer = Line::from(vec![
            Span::styled(" Enter ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Select  ", Style::default().fg(Color::Gray)),
            Span::styled(" Esc ", Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel ", Style::default().fg(Color::Gray)),
        ]);
        frame.render_widget(Paragraph::new(footer).style(Style::default().bg(Color::Black)), footer_area);
    }
}

impl Screen for ChatScreen {
    fn handle_input(&mut self, key: KeyEvent) -> Option<ScreenAction> {
        match self.mode {
            ScreenMode::ModelSelection => self.handle_model_selection_input(key),
            ScreenMode::ServerSelection => self.handle_server_selection_input(key),
            ScreenMode::Chatting => self.handle_chat_input(key),
            ScreenMode::ModelSwitching => self.handle_model_switching_input(key),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self.mode {
            ScreenMode::ModelSelection => Self::render_list_view(frame, area, "Select AI Model for Chat", &mut self.model_list),
            ScreenMode::ServerSelection => Self::render_list_view(frame, area, "Select MCP Server", &mut self.server_list),
            ScreenMode::Chatting => self.render_chat(frame, area),
            ScreenMode::ModelSwitching => self.render_model_switcher(frame, area),
        }
    }
}
