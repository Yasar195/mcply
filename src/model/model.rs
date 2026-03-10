use std::pin::Pin;
use std::future::Future;
use serde::{Deserialize, Serialize};
use crate::model::ModelError;

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
    fn connect(&self, api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), ModelError>> + Send + '_>>;
    fn chat(&self, messages: Vec<ChatMessage>, model: String, tools: Option<serde_json::Value>) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ModelError>> + Send + '_>>;
    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, ModelError>> + Send + '_>> {
        Box::pin(async move {
            Ok(vec!["default".to_string()])
        })
    }
}

impl ChatMessage {
    /// Estimate token count (rough approximation: ~4 characters per token)
    pub fn estimate_tokens(&self) -> usize {
        (self.content.len() / 4).max(1)
    }
    
    /// Aggressively optimize messages to minimize token usage while preserving tool context
    /// Keeps: system messages + most recent tool result (if any) + current user message
    /// This is minimal but ensures tools work and tokens stay low
    pub fn optimize_messages(messages: Vec<ChatMessage>, _max_messages: usize) -> Vec<ChatMessage> {
        if messages.is_empty() {
            return messages;
        }
        
        // Separate system messages from others
        let (mut system_msgs, other_msgs): (Vec<_>, Vec<_>) = 
            messages.into_iter().partition(|m| m.role == "system");
        
        if other_msgs.is_empty() {
            return system_msgs;
        }
        
        // Keep only the most recent: tool result (if exists) + current user message
        // This is the minimal context needed to avoid infinite loops while staying under token limits
        let mut recent_messages = Vec::new();
        
        // Find the last tool result (indicating a tool was just executed)
        let last_tool_result = other_msgs.iter().rev().find(|m| m.role == "tool");
        if let Some(tool_msg) = last_tool_result {
            recent_messages.push(tool_msg.clone());
        }
        
        // Find the last user message (current input)
        let last_user = other_msgs.iter().rev().find(|m| m.role == "user");
        if let Some(user_msg) = last_user {
            recent_messages.push(user_msg.clone());
        }
        
        // Build final message list: system + minimal context
        system_msgs.extend(recent_messages);
        system_msgs
    }
    
    /// Calculate total token estimate for a batch of messages
    pub fn batch_token_estimate(messages: &[ChatMessage]) -> usize {
        messages.iter().map(|m| m.estimate_tokens()).sum()
    }
}