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

}



pub trait Persistable: Sized {
    fn insert_sql(&self) -> String;
    fn params(&self) -> Vec<&dyn rusqlite::ToSql>;
    fn update_sql() -> String;
    fn update_params(&self) -> Vec<&dyn rusqlite::ToSql>;
    fn get_all_sql() -> String;
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