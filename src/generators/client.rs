use std::collections::HashMap;
use rmcp::{ServiceExt, RoleClient};
use rmcp::service::RunningService;
use rmcp::model::{CallToolRequestParams, PaginatedRequestParams};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::{ StreamableHttpClientWorker };
use reqwest::Client;

#[derive(Clone)]
pub struct ClientGenerator {
    server_url: String,
}

impl ClientGenerator {
    pub fn new(server_url: &str) -> Self {
        ClientGenerator { server_url: server_url.to_string() }
    }

    pub async fn connect(&self) -> Result<RunningService<RoleClient, ()>, Box<dyn std::error::Error + Send + Sync>> {
        let worker = StreamableHttpClientWorker::<Client>::new_simple(self.server_url.as_str());
        let transport = StreamableHttpClientTransport::spawn(worker);
        let client = ().serve(transport).await?;
        Ok(client)
    }

    pub async fn list_tools(&self) -> Vec<String> {
        let Ok(client) = self.connect().await else { return vec![] };
        client.list_tools(None).await
            .map(|r| r.tools.iter().map(|t| t.name.to_string()).collect())
            .unwrap_or_default()
    }

    pub async fn call_tool(&self, name: &str, params: HashMap<String, String>) -> Option<String> {
        let client = self.connect().await.ok()?;

        let arguments: serde_json::Map<String, serde_json::Value> = params.iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();

        let result = client.call_tool(CallToolRequestParams {
            name: name.to_string().into(),
            arguments: Some(arguments),
            meta: None,
            task: None,
        }).await.ok()?;

        // Keep client alive until call completes
        let output = result.content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        
        drop(client);
        Some(output)
    }
    pub async fn list_tools_description(&self) -> String {
        let Ok(client) = self.connect().await else { return String::new() };
        let result = client.list_tools(None).await
            .map(|r| r.tools.iter().map(|t| {
                let params = t.input_schema.get("properties")
                    .and_then(|p| p.as_object())
                    .map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                format!("- {} (params: {}): {}", t.name, params, t.description.as_deref().unwrap_or(""))
            }).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();

        drop(client);
        result

    }

    pub async fn get_descriptions(&self, client: &RunningService<RoleClient, ()>) -> String {
        client.list_tools(None).await
            .map(|r| r.tools.iter().map(|t| {
                let params = t.input_schema.get("properties")
                    .and_then(|p| p.as_object())
                    .map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                format!("- {} (params: {}): {}",
                    t.name,
                    params,
                    t.description.as_deref().unwrap_or(""))
            }).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    pub async fn call_with(
        &self,
        client: &RunningService<RoleClient, ()>,
        name: &str,
        params: HashMap<String, String>
    ) -> Option<String> {
        let arguments: serde_json::Map<String, serde_json::Value> = params.iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();

        let result = client.call_tool(CallToolRequestParams {
            name: name.to_string().into(),
            arguments: Some(arguments),
            meta: None,
            task: None,
        }).await.ok()?;

        let mut parts: Vec<String> = result.content
            .iter()
            .filter_map(|c| {
                if let Some(t) = c.as_text() {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .collect();

        if let Some(json) = &result.structured_content {
            if let Ok(s) = serde_json::to_string_pretty(json) {
                parts.push(s);
            }
        }

        Some(parts.join("\n"))
    }

}