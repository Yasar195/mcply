use std::pin::Pin;

use reqwest::Client;
use std::future::Future;

use crate::model::model::{Model, ChatMessage};
use crate::model::ModelError;

pub struct GroqModel {
    pub client: Client,
    pub api_url: String,
    pub api_key: String,
}

impl GroqModel {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self { 
            client: Client::new(),
            api_url,
            api_key,
        }
    }
}

impl Model for GroqModel {
    fn connect(&self, _api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), ModelError>> + Send + '_>> {
        Box::pin(async move {
            self.client
                .get(format!("{}/models", self.api_url)) // <-- fixed
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;
            Ok(())
        })
    }

    fn chat(&self, messages: Vec<ChatMessage>, model: String, tools: Option<serde_json::Value>, tool_choice: Option<String>) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ModelError>> + Send + '_>> {
        Box::pin(async move {
            let mut payload = serde_json::json!({
                "model": model,
                "messages": messages
            });
            
            if let Some(t) = tools {
                if let Some(arr) = t.as_array() {
                    if !arr.is_empty() {
                        payload.as_object_mut().unwrap().insert("tools".to_string(), t);
                        // Let Groq API use default tool_choice ("auto") natively rather than explicitly injecting it,
                        // which some models (like llama-3 on Groq) can misinterpret when parameters are empty.
                        let choice = tool_choice.unwrap_or_else(|| "auto".to_string());
                            payload.as_object_mut().unwrap().insert(
                                "tool_choice".to_string(), 
                                serde_json::Value::String(choice)
                            );
                        }
                }
            }

            let response = self.client
                .post(format!("{}/chat/completions", self.api_url)) // <-- OpenAI format
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&payload)
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;
            
            // Check if the API returned an error in the response body
            if let Some(error_obj) = body.get("error") {
                if let Some(error_msg) = error_obj.get("message").and_then(|m| m.as_str()) {
                    // Create a custom error that the caller can catch
                    return Err(ModelError::CustomError(error_msg.to_string()));
                }
            }
            
            Ok(body)
        })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, ModelError>> + Send + '_>> {
        Box::pin(async move {
            let response = self.client
                .get(format!("{}/models", self.api_url)) // <-- fixed URL
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;
            
            // Groq uses OpenAI format: { "data": [{ "id": "..." }] }
            let models = body["data"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string())) // <-- "id" not "name"
                .collect();

            Ok(models)
        })
    }
}


