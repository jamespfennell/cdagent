mod config;
mod database;
mod github;
mod project;
use std::process::Command;
use std::sync::mpsc;
use std::time;

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
            if let Err(err) = run_for_project(project, &mut github_client) {
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

fn run_for_project(
    project: &mut project::Project,
    github_client: &mut github::Client,
) -> Result<(), String> {
    if project.config.paused {
        return Ok(());
    }
    let old_workflow_run = &project.last_workflow_run;
    let new_workflow_run = github_client.get_latest_successful_workflow_run(
        &project.config.github_user,
        &project.config.repo,
        &project.config.mainline_branch,
        &project.config.auth_token,
    )?;
    if let Some(old_workflow_run) = old_workflow_run {
        if old_workflow_run.id == new_workflow_run.id {
            return Ok(());
        }
    }
    eprintln!(
        "[{}] New successful workflow run found: {new_workflow_run:#?}",
        project.config.name
    );
    project.last_workflow_run = Some(new_workflow_run.clone());

    let mut result = RunResult {
        project: project.config.clone(),
        workflow_run: new_workflow_run,
        steps: vec![],
    };
    for step in &project.config.steps {
        let pieces = match shlex::split(&step.run) {
            None => return Err(format!("invalid run command {}", step.run)),
            Some(pieces) => pieces,
        };
        let program = match pieces.first() {
            None => return Err("empty run command".into()),
            Some(command) => command,
        };
        eprintln!("Running program {program} with args {:?}", &pieces[1..]);
        let mut command = Command::new(program);
        command.args(&pieces[1..]);
        if let Some(working_directory) = &project.config.working_directory {
            command.current_dir(working_directory);
        }
        let output = command.output().expect("failed to wait for subprocess");
        let step_result = StepResult::new(step, &output);
        if !output.status.success() {
            eprintln!("failed to run command: {:?}", result);
            break;
        }
        result.steps.push(step_result);
    }

    project.run_results.push(result);
    Ok(())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RunResult {
    project: config::Project,
    workflow_run: github::WorkflowRun,
    steps: Vec<StepResult>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct StepResult {
    step: config::Step,
    success: bool,
    stdout: String,
    stderr: String,
}

impl StepResult {
    fn new(step: &config::Step, output: &std::process::Output) -> Self {
        Self {
            step: step.clone(),
            success: output.status.success(),
            stdout: vec_to_string(&output.stdout),
            stderr: vec_to_string(&output.stderr),
        }
    }
}

fn vec_to_string(v: &[u8]) -> String {
    match std::str::from_utf8(v) {
        Ok(s) => s.into(),
        Err(_) => v
            .iter()
            .map(|b| if b.is_ascii() { *b as char } else { '#' })
            .collect(),
    }
}
