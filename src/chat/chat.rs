use crate::generators::server::ServerGenerator;
use crate::generators::client::ClientGenerator;
use crate::model::model::Model;

pub struct Chat {
    pub conversations: Vec<String>,
    pub mcp_server: ServerGenerator,
    pub mcp_client: ClientGenerator,
    pub chat_model: Box<dyn Model>,
}

impl Chat {

    pub fn new(mcp_client: ClientGenerator, mcp_server: ServerGenerator, chat_model: Box<dyn Model>) -> Self {
        Chat { conversations: Vec::new(), mcp_server, mcp_client, chat_model }
    }
    
    pub fn send_chat(&mut self, chat: String, prompt: Option<String>) {
        self.conversations.push(chat);
    }

}
