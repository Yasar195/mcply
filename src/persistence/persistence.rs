use std::{fs::File, path::PathBuf, env};
use rusqlite::{ Connection };
pub struct Persistence {
    pub connection: Option<Connection>,
}

impl Persistence {

    pub fn new() -> Self {
        let db_path = Self::get_database_path();
        Self::create_database();

        Persistence { connection: Some(Connection::open(&db_path).unwrap()) }
    }


    pub fn sync_schema(&self) {
        if let Some(conn) = &self.connection {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL,
                    description TEXT,
                    completed BOOLEAN NOT NULL
                )",
                [],
            ).expect("Failed to create tasks table");
            
            conn.execute(
                "CREATE TABLE IF NOT EXISTS models (
                    id INTEGER PRIMARY KEY,
                    model_type TEXT NOT NULL,
                    api_key TEXT
                )",
                [],
            ).expect("Failed to create models table");

            conn.execute(
                "CREATE TABLE IF NOT EXISTS mcp_servers (
                    id      INTEGER PRIMARY KEY,
                    name    TEXT NOT NULL,
                    version TEXT NOT NULL DEFAULT '1.0.0'
                )",
                [],
            ).expect("Failed to create mcp_servers table");

            conn.execute(
                "CREATE TABLE IF NOT EXISTS mcp_server_tools (
                    id        INTEGER PRIMARY KEY,
                    server_id INTEGER NOT NULL
                        REFERENCES mcp_servers(id) ON DELETE CASCADE,
                    tool_name TEXT NOT NULL,
                    tool_def  TEXT NOT NULL
                )",
                [],
            ).expect("Failed to create mcp_server_tools table");

            // Migration: Add tool_def column if it doesn't exist. Ignore error if it does.
            let _ = conn.execute(
                "ALTER TABLE mcp_server_tools ADD COLUMN tool_def TEXT NOT NULL DEFAULT '{}'",
                [],
            );
        }
    }

    fn get_database_path() -> PathBuf {
        let data_dir = if cfg!(target_os = "windows") {
            let appdata = env::var("APPDATA").expect("Failed to get APPDATA");
            PathBuf::from(appdata).join("mcply")
        } else {
            let home = env::var("HOME").expect("Failed to get HOME");
            let xdg_data = env::var("XDG_DATA_HOME")
                .unwrap_or_else(|_| format!("{}/.local/share", home));
            PathBuf::from(xdg_data).join("mcply")
        };
        
        std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");
        
        data_dir.join("mcply.db")
    }

    fn create_database() {
        let db_path = Self::get_database_path();
        let file: Result<File, std::io::Error> = File::open(&db_path);
        match file {
            Ok(_) => (),
            Err(_) => {
                match File::create(&db_path) {
                    Ok(_) => (),
                    Err(e) => eprintln!("Failed to create database: {}", e),
                }
            }
        }
    }

    pub fn save<T: Persistable>(&self, item: &T) {
        if let Some(conn) = &self.connection {
            conn.execute(
                item.insert_sql().as_str(),
                item.params().as_slice()
            ).expect("Failed to insert item");
        }   
    }

    pub fn get_all<T: Persistable>(&self) -> Vec<T> {
        let mut items = Vec::new();
        if let Some(conn) = &self.connection {
            let mut stmt = conn.prepare(T::get_all_sql().as_str()).expect("Failed to prepare statement");
            let rows = stmt.query_map([], |row| T::from_row(row)).expect("Failed to query items");

            for item in rows {
                items.push(item.expect("Failed to map item"));
            }
        }
        items
    }

    pub fn update<T: Persistable>(&self, item: &T) {
        if let Some(conn) = &self.connection {
            conn.execute(
                T::update_sql().as_str(),
                item.update_params().as_slice()
            ).expect("Failed to update item");
        }
    }

    pub fn delete<T: Persistable>(&self, id: i64) {
        if let Some(conn) = &self.connection {
            conn.execute(T::delete_sql().as_str(), [id])
                .expect("Failed to delete item");
        }
    }

    /// Fetch all rows where `parent_id` matches a foreign-key column.
    /// The SQL must be `SELECT ... WHERE fk_col = ?1 ORDER BY ...`
    pub fn get_by_parent<T: Persistable>(&self, parent_id: i64) -> Vec<T> {
        let mut items = Vec::new();
        if let Some(conn) = &self.connection {
            let sql = T::get_by_parent_sql();
            let mut stmt = conn.prepare(sql.as_str()).expect("Failed to prepare statement");
            let rows = stmt
                .query_map([parent_id], |row| T::from_row(row))
                .expect("Failed to query items");
            for item in rows {
                items.push(item.expect("Failed to map item"));
            }
        }
        items
    }

}



pub trait Persistable: Sized {
    fn insert_sql(&self) -> String;
    fn params(&self) -> Vec<&dyn rusqlite::ToSql>;
    fn update_sql() -> String;
    fn update_params(&self) -> Vec<&dyn rusqlite::ToSql>;
    fn get_all_sql() -> String;
    fn get_by_parent_sql() -> String {
        panic!("get_by_parent_sql not implemented for this type")
    }
    fn delete_sql() -> String;
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

#[derive(Debug)]
pub struct Task {
    pub id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub completed: bool,
}

impl Persistable for Task {
    fn insert_sql(&self) -> String {
        "INSERT INTO tasks (title, description, completed) VALUES (?1, ?2, ?3)".to_string()
    }

    fn params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.title, &self.description, &self.completed]
    }

    fn update_sql() -> String {
        "UPDATE tasks SET title = ?1, description = ?2, completed = ?3 WHERE id = ?4".to_string()
    }

    fn update_params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.title, &self.description, &self.completed, &self.id]
    }

    fn get_all_sql() -> String {
        "SELECT id, title, description, completed FROM tasks ORDER BY id DESC".to_string()
    }

    fn delete_sql() -> String {
        "DELETE FROM tasks WHERE id = ?1".to_string()
    }

    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Task {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            completed: row.get(3)?,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ModelType {
    Ollama,
    Groq,
}

impl ModelType {
    pub fn as_str(&self) -> &str {
        match self {
            ModelType::Ollama => "Ollama",
            ModelType::Groq => "Groq",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Ollama" => Some(ModelType::Ollama),
            "Groq" => Some(ModelType::Groq),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    pub id: Option<i64>,
    pub model_type: String,
    pub api_key: Option<String>,
}

impl Model {
    pub fn get_model_type(&self) -> ModelType {
        ModelType::from_str(&self.model_type).unwrap_or(ModelType::Ollama)
    }
}

impl Persistable for Model {
    fn insert_sql(&self) -> String {
        "INSERT INTO models (model_type, api_key) VALUES (?1, ?2)".to_string()
    }

    fn params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.model_type, &self.api_key]
    }

    fn update_sql() -> String {
        "UPDATE models SET model_type = ?1, api_key = ?2 WHERE id = ?3".to_string()
    }

    fn update_params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.model_type, &self.api_key, &self.id]
    }

    fn get_all_sql() -> String {
        "SELECT id, model_type, api_key FROM models ORDER BY id DESC".to_string()
    }

    fn delete_sql() -> String {
        "DELETE FROM models WHERE id = ?1".to_string()
    }

    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Model {
            id: row.get(0)?,
            model_type: row.get(1)?,
            api_key: row.get(2)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct McpServer {
    pub id: Option<i64>,
    pub name: String,
    pub version: String,
}

impl Persistable for McpServer {
    fn insert_sql(&self) -> String {
        "INSERT INTO mcp_servers (name, version) VALUES (?1, ?2)".to_string()
    }

    fn params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.name, &self.version]
    }

    fn update_sql() -> String {
        "UPDATE mcp_servers SET name = ?1, version = ?2 WHERE id = ?3".to_string()
    }

    fn update_params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.name, &self.version, &self.id]
    }

    fn get_all_sql() -> String {
        "SELECT id, name, version FROM mcp_servers ORDER BY id DESC".to_string()
    }

    fn delete_sql() -> String {
        "DELETE FROM mcp_servers WHERE id = ?1".to_string()
    }

    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(McpServer {
            id: row.get(0)?,
            name: row.get(1)?,
            version: row.get(2)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct McpServerTool {
    pub id: Option<i64>,
    pub server_id: i64,
    pub tool_name: String,
    pub tool_def: String, // Stored as JSON string
}

impl Persistable for McpServerTool {
    fn insert_sql(&self) -> String {
        "INSERT INTO mcp_server_tools (server_id, tool_name, tool_def) VALUES (?1, ?2, ?3)".to_string()
    }

    fn params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.server_id, &self.tool_name, &self.tool_def]
    }

    fn update_sql() -> String {
        "UPDATE mcp_server_tools SET server_id = ?1, tool_name = ?2, tool_def = ?3 WHERE id = ?4".to_string()
    }

    fn update_params(&self) -> Vec<&dyn rusqlite::ToSql> {
        vec![&self.server_id, &self.tool_name, &self.tool_def, &self.id]
    }

    fn get_all_sql() -> String {
        "SELECT id, server_id, tool_name, tool_def FROM mcp_server_tools ORDER BY id ASC".to_string()
    }

    fn get_by_parent_sql() -> String {
        "SELECT id, server_id, tool_name, tool_def FROM mcp_server_tools WHERE server_id = ?1 ORDER BY id ASC".to_string()
    }

    fn delete_sql() -> String {
        "DELETE FROM mcp_server_tools WHERE id = ?1".to_string()
    }

    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(McpServerTool {
            id: row.get(0)?,
            server_id: row.get(1)?,
            tool_name: row.get(2)?,
            tool_def: row.get(3)?,
        })
    }
}