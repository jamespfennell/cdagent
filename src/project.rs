#[derive(serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub config: crate::config::Project,
    pub last_workflow_run: Option<crate::github::WorkflowRun>,
    pub run_results: Vec<crate::RunResult>,
}

impl Project {
    pub fn new(config: crate::config::Project) -> Self {
        Self { config, last_workflow_run: None,  run_results: Default::default(), }
    }
}
