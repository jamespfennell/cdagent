//! Configuration for the agent.

use crate::{email, github};

/// Configuration for the agent.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Hostname for the agent is running on; e.g. rollouts.example.com.
    ///
    /// This is used when sending emails and on the status page.
    pub hostname: String,

    /// List of projects to run the agent for.
    pub projects: Vec<ProjectConfig>,

    pub email_config: Option<email::Config>,
}

/// A project to run the agent for.
///
/// Each project corresponds to a distinct deployment and generally a distinct GitHub repository.
/// Whenever there is a new successful CI run on the specified GitHub repository branch,
///     the agent will run the specified command.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectConfig {
    /// Name of the project. Used for debugging.
    pub name: String,

    /// If the project is paused; defaults to false.
    #[serde(default)]
    pub paused: bool,

    /// GitHub repository to watch.
    ///
    /// This field as the form `github.com/$USER/$NAME`.
    pub repo: github::Repo,

    /// Branch of repo to watch.
    pub branch: String,

    /// Auth token to use for making GitHub API requests.
    ///
    /// The auth token can be empty, in which case GitHub will use per-IP-address rate limiting.
    /// This allows up to 60 non-cached requests an hour.
    /// Using an auth token increases the rate limit to 5000 non-cached requests an hour.
    /// In general a non-cached request is only made when there is a new successful CI run.
    ///
    /// If provided, the auth token must have GitHub actions read permission
    ///     on the repository.
    #[serde(default)]
    pub auth_token: String,

    /// Working directory in which to run the redeployment steps.
    ///
    /// Defaults to the working directory in which the agent was started.
    pub working_directory: Option<String>,

    /// Steps to perform during a redeployment.
    #[serde(default)]
    pub steps: Vec<Step>,

    /// Number of prior deployments to retain in the internal database and show on
    /// the HTML status page.
    #[serde(default = "ten")]
    pub retention: usize,

    /// Minutes to wait after a successful CI run before performing the redeployment.
    ///
    /// This can be used to perform staggered redeployments.
    /// It can also be used to update the agent itself by having a second agent and performing
    /// staggered redeployments of the pair.
    #[serde(default)]
    pub wait_minutes: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Step {
    /// Name of the step.
    pub name: String,

    /// Command to run.
    pub run: String,
}

fn ten() -> usize {
    10
}
