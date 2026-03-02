use std::{collections::HashMap, iter::Map};

pub struct ServerGenerator {
    name: String,
    version: String,
    tools: Option<Vec<Tool>>,
    resources: Option<Vec<String>>,
    prompts: Option<Vec<String>>
}

pub struct ServerGeneratorConfig {
    pub name: String,
    pub version: String
}

#[derive(Debug)]
pub struct Tool {
    pub tool_name: String,
    pub tool_url: String,
    pub tool_url_params: Option<Vec<HashMap<String, String>>>,
    pub tool_url_query: Option<Vec<Map<String, String>>>
}

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

    pub fn add_tools(&mut self, tool: Tool) { 
        let _ = &self.tools.get_or_insert(Vec::new()).push(tool);
    }
    
    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn display(&self) {
        println!("{:?}", self.tools);
    }
 
    pub fn get_version(&self) -> &str {
        &self.version
    }
}