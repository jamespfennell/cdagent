//! A simple database for persisting data across runs of the agent.

use std::collections::HashMap;
use std::sync;

static STATUS_DOT_HTML: &str = include_str!("status.html");

/// A simple database for persisting data across runs of the agent.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Database {
    #[serde(skip)]
    path: Option<String>,
    #[serde(skip)]
    json_data: sync::Arc<sync::Mutex<String>>,
    #[serde(skip)]
    html_data: sync::Arc<sync::Mutex<String>>,
    pub config: crate::config::Config,
    pub github_client: crate::github::Data,
    pub projects: Vec<crate::project::Project>,
}

impl Database {
    /// Create a new in-memory database.
    pub fn new_in_memory(config: crate::config::Config) -> Self {
        Self {
            path: None,
            json_data: Default::default(),
            html_data: Default::default(),
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
        let mut database: Self = match std::fs::read_to_string(path) {
            Ok(json) => {
                let mut database: Self = match serde_json::from_str(&json) {
                    Ok(values) => values,
                    Err(err) => return Err(format!("database file is corrupt: {err}. Consider deleting the file to initialize a new database"))
                };
                database.config = config;
                database
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("Database file {path} doesn't exist; initializing new database");
                    Self::new_in_memory(config)
                } else {
                    return Err(format!("failed to open database file: {err}"));
                }
            }
        };
        database.path = Some(path.to_string());
        let mut existing_projects: Vec<crate::project::Project> = vec![];
        std::mem::swap(&mut existing_projects, &mut database.projects);
        let mut name_to_existing_project: HashMap<String, crate::project::Project> =
            existing_projects
                .into_iter()
                .map(|p| (p.config.name.clone(), p))
                .collect();
        database.projects = database
            .config
            .projects
            .iter()
            .map(|c| match name_to_existing_project.remove(&c.name) {
                None => crate::project::Project::new(c.clone()),
                Some(mut project) => {
                    project.config = c.clone();
                    project
                }
            })
            .collect();
        database.checkpoint()?;
        Ok(database)
    }

    /// Checkpoint the database by writing its full state to disk.
    ///
    /// This is a no-op for in-memory databases.
    pub fn checkpoint(&self) -> Result<(), String> {
        let content =
            serde_json::to_string_pretty(&self).expect("failed to serialize database values");
        if let Some(path) = &self.path {
            match std::fs::write(path, &content) {
                Ok(()) => (),
                Err(err) => return Err(format!("failed to write database: {err}")),
            };
        }
        *self.json_data.lock().unwrap() = content;

       
        let mut tt =handlebars::Handlebars::new();
        tt.register_template_string("status.html", STATUS_DOT_HTML).unwrap();
        let rendered = tt.render("status.html", self).unwrap();
        *self.html_data.lock().unwrap() = rendered;
        Ok(())
    }

    pub fn json_data(&self) -> sync::Arc<sync::Mutex<String>> {
        self.json_data.clone()
    }

    pub fn html_data(&self) -> sync::Arc<sync::Mutex<String>> {
        self.html_data.clone()
    }
}
