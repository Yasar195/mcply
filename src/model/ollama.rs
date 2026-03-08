use std::pin::Pin;

use reqwest::{Client, Error};

use crate::model::model::Model;



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
    fn connect(&self, api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> {
        Box::pin(async move {
            self.client
                .get("http://localhost:11434/api/tags")
                .send()
                .await?;
            Ok(())
        })
    }


    fn chat(&self, message: String, model: String) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
       Box::pin(async move {
            let response = self.client
                .post("http://localhost:11434/api/generate")
                .timeout(std::time::Duration::from_secs(60))
                .json(&serde_json::json!({
                    "model": model,
                    "prompt": message,
                    "stream": false   // <-- back to false, simpler and reliable
                }))
                .send()
                .await?;

            let body: serde_json::Value = response.json().await?;
            
            let reply = body["response"]
                .as_str()
                .unwrap_or("No response")
                .to_string();

            Ok(reply)
        })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, Error>> + Send + '_>> {
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