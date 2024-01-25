//! A GitHub client.

use std::time;
use std::{collections::HashMap, time::Duration};

use crate::database;

/// A GitHub client.
///
/// This is a "good citizen" client that honors rate limiting information,
///     and tries to cache requests using the HTTP etag header.
pub struct Client {
    agent: ureq::Agent,
    data: Data,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Data {
    cache: HashMap<String, (String, WorkflowRun)>,
    rate_limit_resource_to_infos: HashMap<String, RateLimitInfo>,
    auth_token_to_rate_limit_resource: HashMap<String, String>,
}

impl Client {
    pub fn new(database: &database::Database) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(1000))
            .build();
        let data = database.github_client.clone();
        Self { agent, data }
    }

    /// Get the latest successful workflow run for the provided repo in branch.
    ///
    /// Returns an error if there have been no successful workflow runs for the branch.
    ///
    /// The provided auth token can be empty.
    /// See the commands on the auth token config for more information about this.
    pub fn get_latest_successful_workflow_run(
        &mut self,
        user: &str,
        repo: &str,
        branch: &str,
        auth_token: &str,
    ) -> Result<WorkflowRun, String> {
        self.check_for_rate_limiting(auth_token)?;

        let url = format!["https://api.github.com/repos/{user}/{repo}/actions/runs?branch={branch}&event=push&status=success&per_page=1&exclude_pull_requests=true"];
        let mut request = self
            .agent
            .get(&url)
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", "2022-11-28");
        if !auth_token.is_empty() {
            request = request.set("Authorization", &format!["Bearer {auth_token}"]);
        }
        if let Some((etag, _)) = self.data.cache.get(&url) {
            request = request.set("if-none-match", etag)
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(err) => return Err(format!("failed to make GitHub API request: {err}")),
        };
        if let Some(rate_limit_info) = RateLimitInfo::build(&response) {
            self.data
                .auth_token_to_rate_limit_resource
                .insert(auth_token.to_string(), rate_limit_info.resource.clone());
            self.data
                .rate_limit_resource_to_infos
                .insert(rate_limit_info.resource.clone(), rate_limit_info);
        }

        if response.status() == 304 {
            if let Some((_, workflow_run)) = self.data.cache.get(&url) {
                return Ok(workflow_run.clone());
            }
        }

        let etag = response.header("etag").map(str::to_string);
        let body: String = match response.into_string() {
            Ok(body) => body,
            Err(err) => return Err(format!("failed to read GitHub API response: {err}")),
        };
        let mut build: Build = match serde_json::from_str(&body) {
            Ok(build) => build,
            Err(err) => {
                return Err(format!(
                    "failed to deserialize GitHub API json response: {err}"
                ))
            }
        };
        let workflow_run = match build.workflow_runs.pop() {
            Some(workflow_run) => workflow_run,
            None => return Err("GitHub actions has no successful runs".to_string()),
        };
        if let Some((old_etag, cached_workflow_run)) = self.data.cache.get(&url) {
            if workflow_run.created_at < cached_workflow_run.created_at {
                return Err(format!["GitHub returned a stale workflow run! old_etag={old_etag}, new_etag={etag:?},\ncached_workflow={cached_workflow_run:#?}\nbody=<begin>\n{body}\n<end>"]);
            }
        }

        if let Some(etag) = etag {
            self.data.cache.insert(url, (etag, workflow_run.clone()));
        }
        Ok(workflow_run)
    }

    fn check_for_rate_limiting(&self, auth_token: &str) -> Result<(), String> {
        let resource = match self.data.auth_token_to_rate_limit_resource.get(auth_token) {
            None => return Ok(()),
            Some(resource) => resource,
        };
        let info = match self.data.rate_limit_resource_to_infos.get(resource) {
            None => return Ok(()),
            Some(info) => info,
        };
        if info.remaining > 0 {
            return Ok(());
        }
        let current_timestamp = time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .expect("current time should be after the Unix epoch")
            .as_secs();
        let seconds_to_reset = match info.reset.checked_sub(current_timestamp) {
            None => return Ok(()),
            Some(s) => s,
        };
        Err(format!("reached GitHub API rate limit for this auth token; resource={resource}, limit={}, seconds_to_reset={seconds_to_reset}", info.limit))
    }

    pub fn persist(&self, database: &mut database::Database) {
        database.github_client = self.data.clone();
    }
}

#[derive(Debug, serde::Deserialize)]
struct Build {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub display_title: String,
    pub run_number: u64,
    pub head_sha: String,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RateLimitInfo {
    pub limit: u64,
    pub remaining: u64,
    pub used: u64,
    pub reset: u64,
    pub resource: String,
}

impl RateLimitInfo {
    fn build(response: &ureq::Response) -> Option<RateLimitInfo> {
        let resource = match response.header("x-ratelimit-resource") {
            None => return None,
            Some(s) => s.to_string(),
        };
        let mut info = RateLimitInfo {
            limit: 0,
            remaining: 0,
            used: 0,
            reset: 0,
            resource,
        };
        for (u, header_name) in [
            (&mut info.limit, "x-ratelimit-limit"),
            (&mut info.remaining, "x-ratelimit-remaining"),
            (&mut info.used, "x-ratelimit-used"),
            (&mut info.reset, "x-ratelimit-reset"),
        ] {
            *u = match response.header(header_name) {
                None => return None,
                Some(s) => match s.parse::<u64>() {
                    Ok(u) => u,
                    Err(_) => return None,
                },
            };
        }
        Some(info)
    }
}
