mod config;
mod database;
mod email;
mod github;
mod http;
mod project;
use std::sync::mpsc;

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
            None | Some(0) => 60,
            Some(d) => d,
        });
        eprintln!("[main] using the following poll interval: {poll_interval:?}");

        let notifier: Box<dyn email::Notifier> = match config.email_config {
            None => Box::new(email::NoOpNotifier {}),
            Some(config) => Box::new(email::Client::new(config)),
        };
        let db: Box<dyn database::DB> = match &self.db {
            None => Box::new(database::new_in_memory_db()),
            Some(path) => match database::new_on_disk_db(path.clone()) {
                Ok(db) => Box::new(db),
                Err(err) => {
                    return Err(format!("failed to load DB from {}: {err}", path.display()));
                }
            },
        };
        let github_client = github::Client::new(db.as_ref());
        let project_manager = project::Manager::new(
            db.as_ref(),
            notifier.as_ref(),
            &github_client,
            config.hostname.clone(),
            config.projects,
            poll_interval,
        );
        let http_service =
            http::Service::new(config.hostname.clone(), &github_client, &project_manager);

        std::thread::scope(|s| {
            let stopper_1 = http_service.start(s);
            let stopper_2 = project_manager.start(s);

            // Block until Ctrl+C or similar shutdown signal
            let (tx, rx) = mpsc::channel();
            ctrlc::set_handler(move || {
                eprintln!("[main] received shutdown signal");
                tx.send(()).unwrap();
            })
            .unwrap();
            rx.recv().unwrap();

            eprintln!("[main] starting shutdown sequence");
            stopper_1.stop();
            stopper_2.stop();
        });
        eprintln!("[main] done");
        Ok(())
    }
}
