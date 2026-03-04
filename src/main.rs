mod generators;
mod protocoal;
mod dyns;

use std::collections::HashMap;

use generators::server::{ ServerGenerator, ServerGeneratorConfig };

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
}
