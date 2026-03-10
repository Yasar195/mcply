use std::{collections::HashMap, iter::Map};
use std::sync::{ Arc, RwLock };
use rmcp::model::{ CallToolRequestParams, CallToolResult, Content, PaginatedRequestParams, Tool };
use rmcp::service::{ RunningService, ServerInitializeError };
use rmcp::RoleServer;
use crate::dyns::dynamictool::{ DynamicToolDef, ActionType };
use tokio::task::JoinHandle;
use rmcp::{ServerHandler};
use rmcp::model::{
    ServerInfo, ServerCapabilities, ToolsCapability, PromptsCapability,
    ResourcesCapability, ProtocolVersion, Implementation,
    ListToolsResult, CallToolRequestParam,
    ListPromptsResult, GetPromptRequestParam, GetPromptResult,
    ListResourcesResult, ReadResourceRequestParam, ReadResourceResult,
    PaginatedRequestParam,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, 
    StreamableHttpServerConfig,
    session::local::LocalSessionManager,
};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct ServerGenerator {
    name: String,
    version: String,
    tools: Option<Arc<RwLock<HashMap<String, DynamicToolDef>>>>,
    resources: Option<Vec<String>>,
    prompts: Option<Vec<String>>
}

pub struct ServerGeneratorConfig {
    pub name: String,
    pub version: String
}

pub struct Resource {

}

impl ServerGenerator {

    pub fn new(data: &ServerGeneratorConfig) -> Self {
        ServerGenerator {
            name: data.name.clone(),
            version: data.version.clone(),
            tools: Some(Arc::new(RwLock::new(HashMap::new()))),
            prompts: None,
            resources: None
        }
    }

    pub fn add_tools(&mut self, tool: DynamicToolDef) { 
        match &self.tools {
            Some(tools) => {
                tools.write().unwrap().insert(tool.name.clone(), tool);
            },
            None => {
                let mut map = HashMap::new();
                map.insert(tool.name.clone(), tool);
                self.tools = Some(Arc::new(RwLock::new(map)));
            }
        }
    }

    pub fn get_tools_description(&self) -> String {
        match &self.tools {
            Some(t) => {
                let map = t.read().unwrap();
                map.values().map(|def| {
                    let params = def.parameters.as_ref().map(|params| {
                        params.iter().map(|p| {
                            format!("    - {} ({}){}: {}",
                                p.name,
                                p.param_type,
                                if p.required { ", required" } else { "" },
                                p.description
                            )
                        }).collect::<Vec<_>>().join("\n")
                    }).unwrap_or_else(|| "    - no parameters".to_string());

                    format!("- {}: {}\n  parameters:\n{}", def.name, def.description, params)
                }).collect::<Vec<_>>().join("\n\n")
            },
            None => "No tools available".to_string()
        }
    }
    
    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub async fn call_tool(&self, name: &str, params: HashMap<String, String>) -> Option<CallToolResult> {
        let mut def = self.tools.as_ref()?.read().unwrap().get(name).cloned()?;
        
        println!("Calling tool: '{}' with params: {:?}", name, params);
        
        if !params.is_empty() {
            def.tool.path_params = Some(params);
        }

        Some(Self::execute_tool(&def).await)
    }

    async fn execute_tool(def: &DynamicToolDef) -> CallToolResult {
        match def.action {
            ActionType::http => {
                let result = def.tool.request().await;
                match result {
                    Ok(content) => {
                        let json_content = Content::json(content.clone()).unwrap_or_else(|_| Content::text("serialization error"));
                        CallToolResult {
                            is_error: Some(false),
                            structured_content: Some(content),
                            content: vec![json_content],
                            meta: None
                        }
                    },
                    Err(e) => CallToolResult {
                        is_error: Some(true),
                        structured_content: None,
                        content: vec![],
                        meta: None
                    }
                }
            }
        }
    }


    pub fn serve_server_http(self, port: u16) -> JoinHandle<()>{
            tokio::spawn(async move {
                let session_manager = Arc::new(LocalSessionManager::default());
                let config = StreamableHttpServerConfig::default();
                
                // Clone Arc of self to pass into factory
                let server = Arc::new(self);
                
                let service = StreamableHttpService::new(
                    move || Ok((*server).clone()),  // factory called per session
                    session_manager,
                    config,
                );

                let cors = CorsLayer::permissive();

                let router = axum::Router::new()
                    .route("/mcp", axum::routing::any_service(service))
                    .layer(cors);

                let addr = format!("0.0.0.0:{}", port);
                
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
                axum::serve(listener, router).await.unwrap();
            })
    }

    pub fn get_tools(&self) -> Vec<String> {
        match &self.tools {
            Some(t) => {
                let map = t.read().unwrap();
                map.keys().cloned().collect()
            },
            None => vec![]
        }
    }

 
    pub fn get_version(&self) -> &str {
        &self.version
    }
}


impl ServerHandler for ServerGenerator {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: None }),
                ..Default::default()
            },
            server_info: Implementation {
                name: self.name.clone(),
                version: self.version.clone(),
                title: None,
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: None,
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        
        let tools = match &self.tools {
            Some(t) => {
                let map = t.read().unwrap();
                map.values().map(|def| def.tool_schema()).collect()
            },
            None => {
                vec![]
            }
        };

        Ok(ListToolsResult { tools, next_cursor: None, meta: None })
    }


    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let params: HashMap<String, String> = request.arguments
            .as_ref()
            .map(|p: &serde_json::Map<String, serde_json::Value>| {
                p.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        self.call_tool(&request.name, params).await
            .ok_or_else(|| rmcp::ErrorData::invalid_params("tool not found", None))
    }
}