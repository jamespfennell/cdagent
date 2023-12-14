mod config;
mod github;
use std::{collections::HashMap, time};

use github::WorkflowRun;

fn main() {
    if let Err(err) = run() {
        eprintln!("Failed to run agent: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().into_iter().collect();
    let config_file_path = match args.get(1) {
        None => {
            return Err(
                "the path to the configuration file must be provided as a CLI argument".to_string(),
            )
        }
        Some(s) => s,
    };
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

    let mut github_client: github::Client = Default::default();
    // TODO: it would be nice to persist this cache, and the GitHub client's etag cache,
    // and logs from running CD commands. We could just persist it all as json on disk.
    let mut cache: HashMap<usize, github::WorkflowRun> = Default::default();
    let poll_interval = time::Duration::from_secs(match config.poll_interval_seconds {
        None | Some(0) => 300,
        Some(d) => d,
    });
    eprintln!("Using the following poll interval: {poll_interval:?}");
    loop {
        let start = time::SystemTime::now();

        for (i, project) in config.projects.iter().enumerate() {
            match run_for_project(project, &mut github_client, cache.get(&i)) {
                Ok(workflow_run) => {
                    cache.insert(i, workflow_run);
                }
                Err(err) => {
                    eprintln!("Failed to poll for project {}: {err}", project.name)
                }
            }
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
    github_client: &mut github::Client,
    old_workflow_run: Option<&WorkflowRun>,
) -> Result<WorkflowRun, String> {
    let new_workflow_run = github_client.get_latest_successful_workflow_run(
        &project.github_user,
        &project.repo,
        &project.mainline_branch,
        match &project.auth_token {
            None => "",
            Some(s) => s,
        },
    )?;
    let old_workflow_run = match old_workflow_run {
        None => return Ok(new_workflow_run),
        Some(w) => w,
    };
    if old_workflow_run.id == new_workflow_run.id {
        return Ok(new_workflow_run);
    }
    eprintln!("[{}] New successful workflow run found: {new_workflow_run:#?}", project.name);
    // TODO: run the command
    Ok(new_workflow_run)
}
