mod config;
mod database;
mod github;
mod project;
use std::sync::mpsc;
use std::{thread, time};

fn main() {
    let (tx, rx) = mpsc::channel();

    ctrlc::set_handler(move || {
        eprintln!("received shut down signal");
        tx.send(()).expect("Could not send signal on channel.");
    })
    .expect("Error setting Ctrl-C handler");

    if let Err(err) = run(rx) {
        eprintln!("Failed to run agent: {err}");
        std::process::exit(1);
    }
}

fn run(shutdown: mpsc::Receiver<()>) -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let config_file_path = match args.get(1) {
        None => {
            return Err(
                "the path to the configuration file must be provided as a CLI argument".to_string(),
            )
        }
        Some(s) => s,
    };
    let database_path = args.get(2).cloned();
    let config_file = match std::fs::read_to_string(config_file_path) {
        Ok(s) => s,
        Err(err) => {
            return Err(format!(
                "failed to read configuration file {config_file_path}: {err}"
            ))
        }
    };
    let config: config::Config = match serde_yaml::from_str(&config_file) {
        Ok(config) => config,
        Err(err) => return Err(format!("failed to parse YAML configuration file: {err}")),
    };
    eprintln!("Using the following config: {config:#?}");

    let mut database = match database_path {
        None => database::Database::new_in_memory(config),
        Some(path) => database::Database::new_on_disk(config, &path)?,
    };
    let mut github_client = github::Client::new(&database);
    let poll_interval = time::Duration::from_secs(match database.config.poll_interval_seconds {
        None | Some(0) => 300,
        Some(d) => d,
    });
    eprintln!("Using the following poll interval: {poll_interval:?}");

    let latest_checkpoint = database.latest_checkpoint();
    thread::spawn(move || {
        let server = tiny_http::Server::http("0.0.0.0:8000").unwrap();
        for request in server.incoming_requests() {
            match request.method() {
                tiny_http::Method::Get => {}
                _ => {
                    let response = tiny_http::Response::empty(tiny_http::StatusCode(405));
                    request.respond(response).unwrap();
                    continue;
                }
            }
            let _is_html = match request.url() {
                "/" | "/index.html" => true,
                "/data.json" => false,
                _ => {
                    let response = tiny_http::Response::empty(tiny_http::StatusCode(404));
                    request.respond(response).unwrap();
                    continue;
                }
            };
            let json_data = { latest_checkpoint.lock().unwrap().clone() };
            let response = tiny_http::Response::from_string(json_data);
            request.respond(response).unwrap();
        }
    });

    loop {
        let start = time::SystemTime::now();

        for project in database.projects.iter_mut() {
            if shutdown.try_recv().is_ok() {
                eprintln!(
                    "running project {} interrupted because of shut down signal",
                    project.config.name
                );
                // We don't return immediately but instead try to persist progress in the database
                // before exiting.
                break;
            }
            if let Err(err) = project.run(&mut github_client) {
                eprintln!(
                    "Failed to run one iteration for project {}: {err}",
                    project.config.name
                )
            }
        }
        github_client.persist(&mut database);
        if let Err(err) = database.checkpoint() {
            eprintln!("Failed to checkpoint database: {err}");
        }

        let end = time::SystemTime::now();
        let loop_duration = match end.duration_since(start) {
            Ok(d) => d,
            Err(_) => time::Duration::ZERO,
        };
        match poll_interval.checked_sub(loop_duration) {
            Some(remaining) => {
                if shutdown.recv_timeout(remaining).is_ok() {
                    eprintln!("sleep interrupted because of shut down signal");
                    break;
                }
            }
            None => {
                eprintln!("Time to poll all projects ({loop_duration:?}) was longer than the poll interval ({poll_interval:?}). Will poll again immediately");
            }
        }
    }
    Ok(())
}
