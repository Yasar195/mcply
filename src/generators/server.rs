use std::{collections::HashMap, iter::Map};
use std::sync::{ Arc, RwLock };
use rmcp::transport::DynamicTransportError;
use crate::dyns::dynamictool::DynamicToolDef;

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

pub trait DinamicToolRegistry {
    fn register_tool(&self, def: DynamicTransportError);
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

    // pub fn display(&self) {
    //     println!("{:?}", self.tools);
    // }
 
    pub fn get_version(&self) -> &str {
        &self.version
    }
}