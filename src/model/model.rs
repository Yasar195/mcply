use std::pin::Pin;

use reqwest::Error;

pub trait Model {
    fn connect(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>>;
    fn chat(&self, message: String, model: String) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>;
}