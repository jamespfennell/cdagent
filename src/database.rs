//! A simple database for persisting data across runs of the agent.

use std::collections::HashMap;

/// A simple database for persisting data across runs of the agent.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Database {
    path: Option<String>,
    pub config: crate::config::Config,
    pub github_client: crate::github::Data,
    pub projects: Vec<crate::project::Project>,
}

impl Database {
    /// Create a new in-memory database.
    pub fn new_in_memory(config: crate::config::Config) -> Self {
        Self {
            path: None,
            config,
            github_client: Default::default(),
            projects: Default::default(),
        }
    }

    /// Create a new on-disk database.
    ///
    /// If there is not file at the provided path, a new database will be provisioned.
    ///
    /// This constructor fails if there is an IO error when reading the path,
    ///     or if the file is not valid JSON.
    pub fn new_on_disk(config: crate::config::Config, path: &str) -> Result<Self, String> {
        let json = match std::fs::read_to_string(path) {
            Ok(json) => json,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("Database file {path} doesn't exist; initializing new database");
                    return Ok(Self {
                        path: Some(path.into()),
                        ..Self::new_in_memory(config)
                    });
                }
                return Err(format!("failed to open database file: {err}"));
            }
        };
        let mut database: Self = match serde_json::from_str(&json) {
            Ok(values) => values,
            Err(err) => return Err(format!("database file is corrupt: {err}. Consider deleting the file to initialize a new database"))
        };
        let mut existing_projects: Vec<crate::project::Project> = vec![];
        std::mem::swap(&mut existing_projects, &mut database.projects);
        let mut name_to_existing_project: HashMap<String, crate::project::Project> = existing_projects.into_iter().map(|p| (p.config.name.clone(), p)).collect();
        database.projects = database.config.projects.iter().map(|c| {
            match name_to_existing_project.remove(&c.name) {
                None => crate::project::Project::new(c.clone()),
                Some(mut project) => {
                    project.config = c.clone();
                    project
                },
            }
        }).collect();
        database.config = config;
        Ok(database)
    }

    /// Checkpoint the database by writing its full state to disk.
    ///
    /// This is a no-op for in-memory databases.
    pub fn checkpoint(&self) -> Result<(), String> {
        let path = match &self.path {
            None => return Ok(()),
            Some(path) => path,
        };
        let content =
            serde_json::to_string_pretty(&self).expect("failed to serialize database values");
        match std::fs::write(path, content) {
            Ok(()) => Ok(()),
            Err(err) => Err(format!("failed to write database: {err}")),
        }
    }
}
