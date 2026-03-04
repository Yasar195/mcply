use crate::protocoal::http::HttpProtocoal;

#[derive(Debug, Clone)]
pub struct DynamicToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Option<Vec<ToolParam>>,
    pub action: ActionType,
    pub tool: HttpProtocoal,
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