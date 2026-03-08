use std::pin::Pin;

use reqwest::Error;

pub trait Model {
    fn connect(&self, api_key: Option<String>) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>>;
    fn chat(&self, message: String, model: String) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>;
    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>, Error>> + Send + '_>> {
        Box::pin(async move {
            Ok(vec!["default".to_string()])
        })
    }
}