use crate::config;
use crate::github;
use std::process::Command;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub config: crate::config::ProjectConfig,
    last_workflow_run: Option<crate::github::WorkflowRun>,
    pending_workflow_run: Option<crate::github::WorkflowRun>,
    run_results: Vec<RunResult>,
}

impl Project {
    pub fn new(config: crate::config::ProjectConfig) -> Self {
        Self {
            config,
            last_workflow_run: None,
            pending_workflow_run: None,
            run_results: Default::default(),
        }
    }

    pub fn run(&mut self, github_client: &mut github::Client) -> Result<(), String> {
        let started = chrono::offset::Utc::now();
        if self.config.paused {
            self.pending_workflow_run = None;
            return Ok(());
        }
        let old_workflow_run = &self.last_workflow_run;
        let new_workflow_run = github_client.get_latest_successful_workflow_run(
            &self.config.github_user,
            &self.config.repo,
            &self.config.mainline_branch,
            &self.config.auth_token,
        )?;
        if let Some(old_workflow_run) = old_workflow_run {
            if old_workflow_run.id == new_workflow_run.id {
                self.pending_workflow_run = None;
                return Ok(());
            }
        }
        let elapsed_mins = (started - new_workflow_run.updated_at).num_minutes();
        if elapsed_mins < self.config.wait_minutes {
            if let Some(pending_workflow_run) = &self.pending_workflow_run {
                if pending_workflow_run.id == new_workflow_run.id {
                    return Ok(());
                }
                eprintln!(
                    "[{}] Abandoning workflow run {pending_workflow_run:#?} in favor of new workflow run.",
                    self.config.name,
                );
            }
            eprintln!(
                "[{}] New successful workflow run found: {new_workflow_run:#?}; waiting {} minutes to redeploy.",
                self.config.name,
                self.config.wait_minutes - elapsed_mins,
            );
            self.pending_workflow_run = Some(new_workflow_run);
            return Ok(());
        }
        eprintln!(
            "[{}] New successful workflow run found: {new_workflow_run:#?}; redeploying",
            self.config.name
        );
        self.last_workflow_run = Some(new_workflow_run.clone());
        self.pending_workflow_run = None;

        let mut result = RunResult {
            config: self.config.clone(),
            started: started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            finished: "".to_string(),
            success: true,
            workflow_run: new_workflow_run,
            steps: vec![],
        };
        for step in &self.config.steps {
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
            if let Some(working_directory) = &self.config.working_directory {
                command.current_dir(working_directory);
            }
            let output = command.output().expect("failed to wait for subprocess");
            let step_result = StepResult::new(step, &output);
            result.steps.push(step_result);
            if !output.status.success() {
                result.success = false;
                eprintln!("failed to run command: {:?}", result);
                break;
            }
        }
        let finished = chrono::offset::Utc::now();
        result.finished = finished.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.run_results.push(result);
        while self.run_results.len() >= self.config.retention {
            self.run_results.remove(0);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RunResult {
    config: config::ProjectConfig,
    #[serde(default)]
    started: String,
    #[serde(default)]
    finished: String,
    success: bool,
    workflow_run: github::WorkflowRun,
    steps: Vec<StepResult>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct StepResult {
    config: config::Step,
    success: bool,
    stdout: String,
    stderr: String,
}

impl StepResult {
    fn new(step: &config::Step, output: &std::process::Output) -> Self {
        Self {
            config: step.clone(),
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
