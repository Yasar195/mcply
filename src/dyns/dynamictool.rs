use crate::protocoal::http::HttpProtocoal;
use serde_json::json;
use rmcp::model::{ Tool, JsonObject };
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct DynamicToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Option<Vec<ToolParam>>,
    pub action: ActionType,
    pub tool: HttpProtocoal,
}

impl DynamicToolDef {
    pub fn tool_schema(&self) -> Tool {
        let mut properties = serde_json::Map::new();
        let mut required = vec![];

        if let Some(params) = &self.parameters {
            for param in params {
                properties.insert(param.name.clone(), json!({
                    "type": "string",
                    "description": param.description
                }));
                if param.required {
                    required.push(param.name.clone());
                }
            }
        }

        let schema = json!({
            "type": "object",
            "properties": properties,
            "required": required
        });

        Tool {
            name: self.name.clone().into(),
            description: Some(self.description.clone().into()),
            input_schema: Arc::new(
                serde_json::from_value::<JsonObject>(schema).unwrap()
            ),
            title: None,
            output_schema: None,
            annotations: None,
            execution: None,
            icons: None,
            meta: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolParam {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub param_type: String
}

#[derive(Debug, Clone)]
pub enum ActionType {
    http
}