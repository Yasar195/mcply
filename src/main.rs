mod generators;
mod protocoal;

use std::collections::HashMap;

use generators::server::{ ServerGenerator, ServerGeneratorConfig, Tool };

fn main() {
    let mut config: ServerGeneratorConfig = ServerGeneratorConfig {
        name: "Gmail server".to_string(),
        version: "1.0".to_string()
    };

    let gmail_server: ServerGenerator = ServerGenerator::new(&config);
    println!("Server Name: {}, Version: {}", gmail_server.get_name(), gmail_server.get_version());

    config.name = "Yahoo server".to_string();
    config.version = "2.0".to_string();

    let mut yahoo_server: ServerGenerator = ServerGenerator::new(&config);  
    println!("Server Name: {}, Version: {}", yahoo_server.get_name(), yahoo_server.get_version());

    config.name = "Outlook server".to_string();
    config.version = "3.0".to_string();

    let outlook_server: ServerGenerator = ServerGenerator::new(&config);
    println!("Server Name: {}, Version: {}", outlook_server.get_name(), outlook_server.get_version());

    let mut test_params = HashMap::new();
    
    test_params.insert("page".to_string(), "5".to_string());

    let test_tool = Tool {
        tool_name: "get_test_url".to_string(),
        tool_url: "https://test.org/get/items".to_string(),
        tool_url_params: Some(vec![test_params]),
        tool_url_query: None
    };

    yahoo_server.add_tools(test_tool);

    yahoo_server.display();
    gmail_server.display();
}
