use crate::protocoal::http::HttpProtocoal;

pub struct DynamicToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Option<Vec<ToolParam>>,
    pub action: ActionType,
    pub tool: HttpProtocoal,
}

pub struct ToolParam {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub param_type: String
}

enum ActionType {
    http
}