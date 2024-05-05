//! Configuration for the agent.

/// Configuration for the agent.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// List of projects to run the agent for.
    pub projects: Vec<ProjectConfig>,

    /// How often to poll the GitHub API to check for new successful CI runs.
    ///
    /// The default is 300 seconds (5 minutes).
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
    pub poll_interval_seconds: Option<u64>,
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

    /// Name of GitHub user that owns the GitHub repository.
    pub github_user: String,

    /// Name of the GitHub repository.
    pub repo: String,

    /// Mainline branch which will be watched for new successful CI runs.
    /// Will generally be 'main' or 'master' but there are no restrictions.
    pub mainline_branch: String,

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
    /// Defaults to the working directory of the cdagent invocation.
    pub working_directory: Option<String>,

    /// Steps to perform during a redeployment.
    #[serde(default)]
    pub steps: Vec<Step>,

    /// Number of prior deployments to retain in the internal database and show on
    /// the HTML status page.
    #[serde(default="ten")]
    pub retention: usize,
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
