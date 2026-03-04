use std::{collections::HashMap, iter::Map};
use std::sync::{ Arc, RwLock };
use rmcp::model::{ Tool, Content, CallToolResult };
use crate::dyns::dynamictool::{ DynamicToolDef, ActionType };

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

    // pub fn display(&self) {
    //     println!("{:?}", self.tools);
    // }
 
    pub fn get_version(&self) -> &str {
        &self.version
    }
}