use crate::config;
use crate::database;
use crate::github;
use std::process::Command;
use std::sync;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub struct Manager {
    tx: mpsc::Sender<()>,
    main_thread: thread::JoinHandle<()>,
    projects: Arc<sync::Mutex<Vec<Project>>>,
}

impl Manager {
    pub fn create_and_start(
        db: Arc<dyn database::DB>,
        github_client: Arc<github::Client>,
        projects: Vec<config::ProjectConfig>,
        poll_interval: chrono::Duration,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let projects: Vec<Project> = projects
            .into_iter()
            .map(|project| {
                database::get_typed(
                    db.as_ref(),
                    &format!["project_manager/projects/{}", project.name],
                )
                .unwrap()
                .unwrap_or_else(|| Project::new(project))
            })
            .collect();
        let projects = Arc::new(sync::Mutex::new(projects));
        let worker = Worker {
            db,
            github_client,
            projects: projects.clone(),
            poll_interval,
            rx,
        };
        let main_thread = thread::spawn(move || {
            worker.run();
        });
        Self {
            tx,
            main_thread,
            projects,
        }
    }

    pub fn shutdown(self) {
        eprintln!("[project_manager] shutdown signal received");
        self.tx.send(()).unwrap();
        eprintln!("[project_manager] signalled to work thread; waiting to stop");
        self.main_thread.join().unwrap();
        eprintln!("[project_manager] shutdown complete");
    }

    pub fn projects(&self) -> Vec<Project> {
        (*self.projects.lock().unwrap()).clone()
    }
}

pub struct Worker {
    db: Arc<dyn database::DB>,
    github_client: Arc<github::Client>,
    projects: Arc<sync::Mutex<Vec<Project>>>,
    poll_interval: chrono::Duration,
    rx: mpsc::Receiver<()>,
}

impl Worker {
    fn run(self) {
        eprintln!("[project_manager] work thread started");
        loop {
            let start = chrono::Utc::now();

            let mut projects = (*self.projects.lock().unwrap()).clone();
            for project in &mut projects {
                if self.rx.try_recv().is_ok() {
                    eprintln!(
                        "[project_manager] running project {} interrupted because of shut down signal",
                        project.config.name,
                    );
                    return;
                }
                if let Err(err) = project.run(self.github_client.as_ref()) {
                    eprintln!(
                        "Failed to run one iteration for project {}: {err}",
                        project.config.name
                    );
                }
                database::set_typed(
                    self.db.as_ref(),
                    format!("project_manager/projects/{}", project.config.name),
                    project,
                )
                .unwrap();
            }
            *self.projects.lock().unwrap() = projects;
            let loop_duration = chrono::Utc::now() - start;
            match self.poll_interval.checked_sub(&loop_duration) {
                Some(remaining) => {
                    if self.rx.recv_timeout(remaining.to_std().unwrap()).is_ok() {
                        eprintln!(
                            "[project_manager] sleep interrupted because of shut down signal"
                        );
                        return;
                    }
                }
                None => {
                    eprintln!("[project_manager] time to poll all projects ({loop_duration:?}) was longer than the poll interval ({:?}). Will poll again immediately", self.poll_interval);
                }
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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

    pub fn run(&mut self, github_client: &github::Client) -> Result<(), String> {
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
            started,
            finished: started,
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
            let step_result = match command.output() {
                Ok(output) => {
                    StepResult::new(step, &output)
                },
                Err(err) => {
                    StepResult{
                        config:step.clone(),
                        success:false,
                        stderr: format!("Failed to start command.\nThis is probably an error in the project configuration.\nError: {err}"),
                    }
                },
            };
            let success = step_result.success;
            result.steps.push(step_result);
            if !success {
                result.success = false;
                eprintln!("failed to run command: {:?}", result);
                break;
            }
        }
        result.finished = chrono::offset::Utc::now();
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
    started: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    finished: chrono::DateTime<chrono::Utc>,
    success: bool,
    workflow_run: github::WorkflowRun,
    steps: Vec<StepResult>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct StepResult {
    config: config::Step,
    success: bool,
    stderr: String,
}

impl StepResult {
    fn new(step: &config::Step, output: &std::process::Output) -> Self {
        Self {
            config: step.clone(),
            success: output.status.success(),
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
