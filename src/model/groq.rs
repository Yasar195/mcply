use std::pin::Pin;

use reqwest::{Client, Error};

use crate::model::model::Model;

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
    fn connect(&self, _api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> {
        Box::pin(async move {
            self.client
                .get(format!("{}/models", self.api_url)) // <-- fixed
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;
            Ok(())
        })
    }

    fn chat(&self, message: String, model: String) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        Box::pin(async move {
            let response = self.client
                .post(format!("{}/chat/completions", self.api_url)) // <-- OpenAI format
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": message}] // <-- OpenAI format
                }))
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;
            
            // OpenAI format: choices[0].message.content
            let reply = body["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("No response")
                .to_string();

            Ok(reply)
        })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, Error>> + Send + '_>> {
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


