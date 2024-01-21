use crate::config;
use crate::github;
use std::process::Command;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub config: crate::config::ProjectConfig,
    last_workflow_run: Option<crate::github::WorkflowRun>,
    run_results: Vec<RunResult>,
}

impl Project {
    pub fn new(config: crate::config::ProjectConfig) -> Self {
        Self {
            config,
            last_workflow_run: None,
            run_results: Default::default(),
        }
    }

    pub fn run(&mut self, github_client: &mut github::Client) -> Result<(), String> {
        if self.config.paused {
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
                return Ok(());
            }
        }
        eprintln!(
            "[{}] New successful workflow run found: {new_workflow_run:#?}",
            self.config.name
        );
        self.last_workflow_run = Some(new_workflow_run.clone());

        let mut result = RunResult {
            project: self.config.clone(),
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
            if !output.status.success() {
                eprintln!("failed to run command: {:?}", result);
                break;
            }
            result.steps.push(step_result);
        }

        self.run_results.push(result);
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RunResult {
    project: config::ProjectConfig,
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
