//! A simple key-value database for persisting data across runs of the agent.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;

/// A simple key-value database for persisting data across runs of the agent.
pub struct Database {
    path: Option<String>,
    values: HashMap<String, serde_json::Value>,
}

impl Database {
    /// Create a new in-memory database.
    pub fn new_in_memory() -> Self {
        Self {
            path: None,
            values: Default::default(),
        }
    }

    /// Create a new on-disk database.
    ///
    /// If there is not file at the provided path, a new database will be provisioned.
    ///
    /// This constructor fails if there is an IO error when reading the path,
    ///     or if the file is not valid JSON.
    pub fn new_on_disk(path: &str) -> Result<Self, String> {
        let json = match std::fs::read_to_string(path) {
            Ok(json) => json,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("Database file {path} doesn't exist; initializing new database");
                    return Ok(Self {
                        path: Some(path.into()),
                        values: Default::default(),
                    });
                }
                return Err(format!("failed to open database file: {err}"));
            }
        };
        let values = match serde_json::from_str(&json) {
            Ok(values) => values,
            Err(err) => return Err(format!("database file is corrupt: {err}. Consider deleting the file to initialize a new database"))
        };
        Ok(Self {
            path: Some(path.into()),
            values,
        })
    }

    /// Get a value from the database.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let value = match self.values.get(key) {
            None => return None,
            Some(value) => value,
        };
        match serde_json::from_value::<T>(value.clone()) {
            Ok(t) => Some(t),
            Err(err) => {
                eprintln!("Failed to convert json value: {err}");
                None
            }
        }
    }

    /// Set a value in the database.
    pub fn set<T: Serialize>(&mut self, key: &str, value: &T) {
        self.values.insert(
            key.to_string(),
            serde_json::to_value(value).expect("failed to serialize value"),
        );
    }

    /// Checkpoint the database by writing its full state to disk.
    ///
    /// This is a no-op for in-memory databases.
    pub fn checkpoint(&self) -> Result<(), String> {
        let path = match &self.path {
            None => return Ok(()),
            Some(path) => path,
        };
        let content = serde_json::to_string_pretty(&self.values)
            .expect("failed to serialize database values");
        match std::fs::write(path, content) {
            Ok(()) => Ok(()),
            Err(err) => Err(format!("failed to write database: {err}")),
        }
    }
}
