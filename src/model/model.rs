use std::pin::Pin;
use reqwest::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

pub trait Model {
    fn connect(&self, api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>>;
    fn chat(&self, messages: Vec<ChatMessage>, model: String, tools: Option<serde_json::Value>) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, Error>> + Send + '_>>;
    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, Error>> + Send + '_>> {
        Box::pin(async move {
            Ok(vec!["default".to_string()])
        })
    }
}