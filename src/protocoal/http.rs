use serde_json::Value;
use std::collections::HashMap;
use reqwest::Client;

#[derive(Debug, Clone)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
}

#[derive(Debug, Clone)]
pub struct HttpProtocoal {
    pub method: HttpMethod,
    pub query_params: Option<HashMap<String, String>>,
    pub path_params: Option<HashMap<String, String>>,
    pub body: Option<Value>,
    pub url: String,
    pub request_headers: Option<HashMap<String, String>>
}

impl HttpProtocoal {
    

    pub async fn request(&self) -> Result<Value, reqwest::Error>{
        let client: Client = reqwest::Client::new();

        let mut final_url: String = self.url.clone();

        if let Some(path_params) = &self.path_params {
            for(key, value) in path_params {
                final_url = final_url.replace(&format!("{{{}}}", key), value)
            }
        }

        let mut request_builder = match self.method {
            HttpMethod::GET => client.get(&final_url),
            HttpMethod::PATCH => client.patch(&final_url),
            HttpMethod::DELETE => client.delete(&final_url),
            HttpMethod::POST => client.post(&final_url),
            HttpMethod::PUT => client.put(&final_url)
        };

        if let Some(query_params) = &self.query_params {
            request_builder = request_builder.query(query_params);
        }

        if let Some(request_headers) = &self.request_headers {
            for(key, value) in request_headers {
                request_builder = request_builder.header(key, value);
            }
        }

        if let Some(body) = &self.body {
            request_builder = request_builder.json(body);
        }

        let response = request_builder.send().await?;
        
        let json = response.json::<Value>().await?;

        Ok(json)
    }

}