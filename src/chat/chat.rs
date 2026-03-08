use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::generators::client::ClientGenerator;
use crate::generators::server::ServerGenerator;
use crate::model::model::Model;

pub struct Chat {
    pub conversations: Arc<Mutex<Vec<String>>>,
    // pub mcp_server: ServerGenerator,
    pub mcp_client: ClientGenerator,
    pub chat_model: Arc<dyn Model + Send + Sync>,
}

fn parse_tool_call(response: &str) -> Option<(String, HashMap<String, String>)> {
    // Strip markdown code fences if present
    let cleaned = response
        .replace("```json", "")
        .replace("```", "")
        .trim()
        .to_string();

    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')? + 1;
    let parsed: serde_json::Value = serde_json::from_str(&cleaned[start..end]).ok()?;

    let name = parsed["tool_call"]["name"].as_str()?.trim().to_string();
    let params = parsed["tool_call"]["parameters"]
        .as_object()
        .map(|p| p.iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect::<HashMap<String, String>>())
        .unwrap_or_default();

    println!("Parsed tool: '{}', params: {:?}", name, params);
    Some((name, params))
}
impl Chat {

    pub fn new(mcp_client: ClientGenerator, chat_model: Arc<dyn Model + Send + Sync>) -> Self {
        Chat { 
            conversations: Arc::new(Mutex::new(Vec::new())), 
            mcp_client,
            // mcp_server, 
            chat_model 
        }
    }
    
    // chat.rs - connect ONCE before spawning, not inside the spawn
    pub fn send_chat(&mut self, chat: String, _prompt: Option<String>, model_name: String) -> tokio::task::JoinHandle<()> {
        self.conversations.lock().unwrap().push(chat.clone());

        let model = Arc::clone(&self.chat_model);
        let conversations = Arc::clone(&self.conversations);
        let mcp_client = self.mcp_client.clone();
        let chat_clone = chat.clone();
        let model_clone = model_name.clone();

        tokio::spawn(async move {
            // Connect once and reuse for both list and call
            let Ok(mcp_connection) = mcp_client.connect().await else {
                eprintln!("Failed to connect to MCP server");
                return;
            };

            let tool_descriptions = mcp_client.get_descriptions(&mcp_connection).await;

            let system_prompt = format!(
                r#"You are a helpful assistant with access to these tools:

    {tool_descriptions}

    RULES:
    - Respond ONLY with raw JSON, no markdown, no code fences.
    - Always include required parameters.
    - Exact format: {{"tool_call": {{"name": "<name>", "parameters": {{"<param>": "<value>"}}}}}}

    Otherwise respond normally."#
            );

            let full_message = format!("{}\n\nUser: {}", system_prompt, chat);

            match model.chat(full_message, model_name).await {
                Ok(response) => {
                    println!("LLM response: {}", response);

                    if let Some((tool_name, params)) = parse_tool_call(&response) {
                        println!("Calling tool via MCP: {} {:?}", tool_name, params);

                        // Reuse same connection
                        match mcp_client.call_with(&mcp_connection, &tool_name, params).await {
                            Some(tool_output) => {
                                let followup = format!(
                                    "User asked: '{}'\nTool '{}' returned:\n{}\n\nAnswer directly and concisely.",
                                    chat_clone, tool_name, tool_output
                                );
                                match model.chat(followup, model_clone).await {
                                    Ok(final_response) => {
                                        conversations.lock().unwrap().push(final_response);
                                    }
                                    Err(e) => eprintln!("LLM followup error: {:#?}", e),
                                }
                            }
                            None => eprintln!("Tool call failed"),
                        }
                    } else {
                        conversations.lock().unwrap().push(response);
                    }
                }
                Err(e) => eprintln!("Chat error: {:#?}", e),
            }
        })
    }

    pub fn get_conversations(&self) -> Vec<String> {
        self.conversations.lock().unwrap().clone()
    }
}