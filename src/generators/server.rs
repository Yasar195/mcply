pub struct ServerGenerator {
    name: String,
    version: String,
}

pub struct ServerGeneratorConfig {
    pub name: String,
    pub version: String
}

impl ServerGenerator {

    pub fn new(data: &ServerGeneratorConfig) -> Self {
        ServerGenerator {
            name: data.name.clone(),
            version: data.version.clone()
        }
    }
    
    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_version(&self) -> &str {
        &self.version
    }
}