use std::pin::Pin;

use reqwest::Client;

use crate::model::model::{Model, ChatMessage};
use crate::model::ModelError;

pub struct OllamaModel {
    pub client: Client
}

impl OllamaModel {
    pub fn new() -> Self {
        Self { 
            client: Client::new()
        }
    }
}

impl Model for OllamaModel {
    fn connect(&self, _api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), ModelError>> + Send + '_>> {
        Box::pin(async move {
            self.client
                .get("http://localhost:11434/api/tags")
                .send()
                .await?;
            Ok(())
        })
    }


    fn chat(&self, messages: Vec<ChatMessage>, model: String, tools: Option<serde_json::Value>, tool_choice: Option<String>)
    -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ModelError>> + Send + '_>> {
        Box::pin(async move {
            let mut payload = serde_json::json!({
                "model": model,
                "messages": messages,
                "stream": false
            });

            let suppress_tools = tool_choice.as_deref() == Some("none");

            if !suppress_tools {
                if let Some(t) = tools {
                    if t.as_array().map_or(false, |arr| !arr.is_empty()) {
                        payload.as_object_mut().unwrap().insert("tools".to_string(), t);
                    }
                }
            }

            let response = self.client
                .post("http://localhost:11434/api/chat")
                .timeout(std::time::Duration::from_secs(60))
                .json(&payload)
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;
            if let Some(error_msg) = body.get("error").and_then(|e| e.as_str()) {
                return Err(ModelError::CustomError(error_msg.to_string()));
            }
            Ok(body)
        })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, ModelError>> + Send + '_>> {
        Box::pin(async move {
            let response = self.client
                .get("http://localhost:11434/api/tags")
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;
            let models = body["models"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|tag| tag["name"].as_str().map(|s| s.to_string()))
                .collect();

            Ok(models)
        })
    }
}