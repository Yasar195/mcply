pub mod model;
pub mod ollama;
pub mod groq;

use std::fmt;

#[derive(Debug)]
pub enum ModelError {
    RequestError(reqwest::Error),
    CustomError(String),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::RequestError(e) => write!(f, "Request error: {}", e),
            ModelError::CustomError(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<reqwest::Error> for ModelError {
    fn from(err: reqwest::Error) -> Self {
        ModelError::RequestError(err)
    }
}