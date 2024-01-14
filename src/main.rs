mod config;
mod database;
mod github;
use std::process::Command;
use std::time;

fn main() {
    if let Err(err) = run() {
        eprintln!("Failed to run agent: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
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
        None => database::Database::new_in_memory(),
        Some(path) => database::Database::new_on_disk(&path)?,
    };
    let mut github_client = github::Client::new(&database);
    let poll_interval = time::Duration::from_secs(match config.poll_interval_seconds {
        None | Some(0) => 300,
        Some(d) => d,
    });
    eprintln!("Using the following poll interval: {poll_interval:?}");
    loop {
        let start = time::SystemTime::now();

        for project in config.projects.iter() {
            if let Err(err) = run_for_project(project, &mut database, &mut github_client) {
                eprintln!(
                    "Failed to run one iteration for project {}: {err}",
                    project.name
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
                std::thread::sleep(remaining);
            }
            None => {
                eprintln!("Time to poll all projects ({loop_duration:?}) was longer than the poll interval ({poll_interval:?}). Will poll again immediately");
            }
        }
    }
}

fn run_for_project(
    project: &config::Project,
    database: &mut database::Database,
    github_client: &mut github::Client,
) -> Result<(), String> {
    let workflow_db_key = format!("last_successful_workflow/{}", project.name);
    let old_workflow_run = database.get::<github::WorkflowRun>(&workflow_db_key);
    let new_workflow_run = github_client.get_latest_successful_workflow_run(
        &project.github_user,
        &project.repo,
        &project.mainline_branch,
        match &project.auth_token {
            None => "",
            Some(s) => s,
        },
    )?;
    if let Some(old_workflow_run) = old_workflow_run {
        if old_workflow_run.id == new_workflow_run.id {
            return Ok(());
        }
    }
    eprintln!(
        "[{}] New successful workflow run found: {new_workflow_run:#?}",
        project.name
    );
    database.set::<github::WorkflowRun>(&workflow_db_key, &new_workflow_run);
    for step in &project.steps {
        let pieces = match shlex::split(&step.run) {
            None => return Err(format!("invalid run command {}", step.run)),
            Some(pieces) => pieces,
        };
        let program = match pieces.first() {
            None => return Err("empty run command".into()),
            Some(command) => command,
        };
        eprintln!("Running program {program} with args {:?}", &pieces[1..]);
        let output = Command::new(program)
            .args(&pieces[1..])
            .output()
            .expect("failed to wait for subprocess");
        eprintln!("Result: {output:?}");
        if !output.status.success() {
            return Err("failed to run command".into());
        }
    }
    Ok(())
}
