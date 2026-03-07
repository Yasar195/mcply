use std::{collections::HashMap, iter::Map};
use std::sync::{ Arc, RwLock };
use rmcp::model::{ Tool, Content, CallToolResult };
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

// #[derive(Debug)]
// pub struct Tool {
//     pub tool_name: String,
//     pub tool_url: String,
//     pub tool_url_params: Option<Vec<HashMap<String, String>>>,
//     pub tool_url_query: Option<Vec<Map<String, String>>>
// }

pub struct Resource {

}

impl ServerGenerator {

    pub fn new(data: &ServerGeneratorConfig) -> Self {
        ServerGenerator {
            name: data.name.clone(),
            version: data.version.clone(),
            tools: None,
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
    
    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub async fn call_tool(&self, name: &str) -> Option<CallToolResult> {
        let def = self.tools.as_ref()?.read().unwrap().get(name).cloned()?;
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
                eprintln!("MCP HTTP server listening on http://{}/mcp", addr);
                
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
                axum::serve(listener, router).await.unwrap();
            })
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
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::Error> {
        eprintln!("list_tools called!"); // add this to confirm it's being hit
        
        let tools = match &self.tools {
            Some(t) => {
                let map = t.read().unwrap();
                eprintln!("tool count: {}", map.len()); // confirm tools exist
                map.values().map(|def| def.tool_schema()).collect()
            },
            None => {
                eprintln!("tools is None!"); // diagnose the problem
                vec![]
            }
        };

        Ok(ListToolsResult { tools, next_cursor: None, meta: None })
    }


    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::Error> {
        self.call_tool(&request.name).await
            .ok_or_else(|| rmcp::Error::invalid_params("tool not found", None))
    }
}