mod generators;
mod protocoal;
mod dyns;
mod chat;
mod model;

use std::{collections::HashMap};

use dyns::dynamictool::{ DynamicToolDef, ActionType };
use generators::server::{ ServerGenerator, ServerGeneratorConfig };
use protocoal::http::{ HttpProtocoal, HttpMethod };

#[tokio::main]
async fn main() {
    let mut config: ServerGeneratorConfig = ServerGeneratorConfig {
        name: "Gmail server".to_string(),
        version: "1.0".to_string()
    };

    let mut yahoo_config: ServerGeneratorConfig = ServerGeneratorConfig {
        name: "Yahoo server".to_string(),
        version: "1.0".to_string()
    };

    let mut yahoo_server: ServerGenerator = ServerGenerator::new(&yahoo_config);

    let mut gmail_server: ServerGenerator = ServerGenerator::new(&config);
    // println!("Server Name: {}, Version: {}", gmail_server.get_name(), gmail_server.get_version());

    let tool = DynamicToolDef {
        name: "Send Email".to_string(),
        description: "Sends an email to a specified recipient.".to_string(),
        parameters: None,
        action: ActionType::http,
        tool: HttpProtocoal {
            method: HttpMethod::GET,
            url: "https://dummyjson.com/carts/{id}".to_string(),
            body: None,
            path_params: Some(HashMap::from([
                ("id".to_string(), "4".to_string())  // replaces {id} with 1
            ])),
            query_params: None,
            request_headers: Some(HashMap::from([
                ("Accept".to_string(), "application/json".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]))        
        }
    };
    gmail_server.add_tools(tool);
    let handle = gmail_server.serve_server_http(4000);
    let yahoo_handle = yahoo_server.serve_server_http(4001);

    match handle.await {
        Err(e) => eprintln!("gmail server panicked: {:?}", e),
        Ok(_) => eprintln!("gmail server exited"),
    }
    
    match yahoo_handle.await {
        Err(err) => println!("error: {:?}", err),
        Ok(_) => ()
    }
    // let result = gmail_server.call_tool("Send Email").await;
    // if let Some(tool_result) = result {
    //     if let Some(structured) = tool_result.structured_content {
    //         println!("{}", serde_json::to_string_pretty(&structured).unwrap());
    //     }
    // }
    
    // let ollama = OllamaModel::new();

    // match ollama.connect().await {
    //     Err(e) => println!("Error connnection: {:?}", e),
    //     Ok(_) => println!("Connected successfully")
    // }

    // match ollama.chat("what is the best pen name for abhishek?".to_string(), "ministral-3:3b".to_string()).await {
    //     Err(e) => println!("Error chating: {:?}", e),
    //     Ok(reply) => println!("Response: {:?}", reply)
    // }


    // let chat = Chat::new(mcp_client, gmail_server, Box::new(ollama));
}