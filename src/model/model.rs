use std::pin::Pin;
use std::future::Future;
use serde::{Deserialize, Serialize};
use crate::model::ModelError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    /// Content may be null when the assistant returns only tool_calls.
    /// Groq rejects an empty-string "" alongside tool_calls — it must be JSON null.
    #[serde(default, serialize_with = "serialize_opt_str")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Serialise `Option<String>` as either a JSON string or `null`.
fn serialize_opt_str<S>(val: &Option<String>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match val {
        Some(v) => s.serialize_str(v),
        None => s.serialize_none(),
    }
}

pub trait Model {
    fn connect(&self, api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), ModelError>> + Send + '_>>;
    fn chat(&self, messages: Vec<ChatMessage>, model: String, tools: Option<serde_json::Value>, tool_choice: Option<String>) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ModelError>> + Send + '_>>;
    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, ModelError>> + Send + '_>> {
        Box::pin(async move {
            Ok(vec!["default".to_string()])
        })
    }
}

impl ChatMessage {
    /// Estimate token count (rough approximation: ~4 characters per token)
    pub fn estimate_tokens(&self) -> usize {
        (self.content.as_deref().unwrap_or("").len() / 4).max(1)
    }
    
    /// Optimize messages to minimize token usage while preserving tool context.
    /// Keeps: system messages + the last user message + any assistant/tool pairs after it.
    /// This ensures the model sees the complete tool call→result chain and won't repeat tool calls.
    pub fn optimize_messages(messages: Vec<ChatMessage>, _max_messages: usize) -> Vec<ChatMessage> {
        if messages.is_empty() {
            return messages;
        }

        let (mut system_msgs, other_msgs): (Vec<_>, Vec<_>) =
            messages.into_iter().partition(|m| m.role == "system");

        if other_msgs.is_empty() {
            return system_msgs;
        }

        let last_user_idx = other_msgs.iter().rposition(|m| m.role == "user");

        let recent_messages: Vec<ChatMessage> = if let Some(idx) = last_user_idx {
            other_msgs.into_iter().skip(idx).collect()
        } else {
            other_msgs
        };

        system_msgs.extend(recent_messages);
        system_msgs
    }
    
    /// Calculate total token estimate for a batch of messages
    pub fn batch_token_estimate(messages: &[ChatMessage]) -> usize {
        messages.iter().map(|m| m.estimate_tokens()).sum()
    }
}