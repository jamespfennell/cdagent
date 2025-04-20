mod config;
mod database;
mod github;
mod http;
mod project;
use std::sync::{mpsc, Arc};

fn main() {
    let cli = Cli::parse();

    if let Err(err) = cli.run() {
        eprintln!("Failed to run agent: {err}");
        std::process::exit(1);
    }
}

use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to the configuration file.
    config: std::path::PathBuf,

    /// Path to the database file.
    ///
    /// If not specified, an in-memory database will be used.
    #[arg(long)]
    db: Option<std::path::PathBuf>,

    /// How often to poll the GitHub API to check for new successful CI runs.
    ///
    /// The default is 60 seconds (1 minute).
    ///
    /// With a smaller number, the agent will notice new pushes faster.
    /// However, GitHub imposes rate limits on API requests.
    /// If this number is too small and too many requests are made,
    ///     these rate limits may be reached.
    /// If this happens the agent with stop polling GitHub until the cooling off period elapses.
    /// The cooling off period is up to one hour.
    ///
    /// The rate limits are 60 non-cached requests per-hour if no auth token is provided,
    ///     or 5000 non-cached requests per-hour per-GitHub-user if an auth token is provided.
    /// Note that if there is no new information from the API (i.e., no new CI runs on mainline),
    ///     GitHub returns a cached response that does not count towards the limit.
    #[arg(long)]
    pub poll_interval_secs: Option<i64>,
}

impl Cli {
    fn run(&self) -> Result<(), String> {
        let raw_config = match std::fs::read_to_string(&self.config) {
            Ok(s) => s,
            Err(err) => {
                return Err(format!(
                    "failed to read configuration file {}: {err}",
                    self.config.display()
                ))
            }
        };
        let config: config::Config = match serde_yaml::from_str(&raw_config) {
            Ok(config) => config,
            Err(err) => return Err(format!("failed to parse YAML configuration file: {err}")),
        };
        eprintln!(
            "[main] loaded the configuration containing {} projects",
            config.projects.len()
        );

        let poll_interval = chrono::Duration::seconds(match self.poll_interval_secs {
            None | Some(0) => 300,
            Some(d) => d,
        });
        eprintln!("[main] using the following poll interval: {poll_interval:?}");

        let db: Arc<dyn database::DB> = match &self.db {
            None => Arc::new(database::new_in_memory_db()),
            Some(path) => match database::new_on_disk_db(path.clone().into()) {
                Ok(db) => Arc::new(db),
                Err(err) => {
                    return Err(format!("failed to load DB from {}: {err}", path.display()));
                }
            },
        };
        let github_client = Arc::new(github::Client::new(db.clone()));
        let project_manager = Arc::new(project::Manager::create_and_start(
            db.clone(),
            github_client.clone(),
            config.projects,
            poll_interval,
        ));
        let http_service =
            http::Service::create_and_start(github_client.clone(), project_manager.clone());

        // Block until Ctrl+C or similar shutdown signal
        let (tx, rx) = mpsc::channel();
        ctrlc::set_handler(move || {
            eprintln!("[main] received shutdown signal");
            tx.send(()).unwrap();
        })
        .unwrap();
        rx.recv().unwrap();
        eprintln!("[main] starting shutdown sequence");
        http_service.shutdown();
        Arc::into_inner(project_manager).unwrap().shutdown();
        eprintln!("[main] done");
        Ok(())
    }
}
