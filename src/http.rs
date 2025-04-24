//! Module http contains a HTTP service that exposes status information about the agent.

use std::sync;
use std::thread;

use crate::{github, project};

pub struct Service<'a> {
    github_client: &'a github::Client<'a>,
    project_manager: &'a project::Manager<'a>,
    templates: handlebars::Handlebars<'static>,
}

impl<'a> Service<'a> {
    pub fn new(github_client: &'a github::Client, project_manager: &'a project::Manager) -> Self {
        let mut templates = handlebars::Handlebars::new();
        templates.set_strict_mode(true);
        templates
            .register_template_string("status.html", STATUS_DOT_HTML)
            .unwrap();
        templates.register_helper("time_diff", Box::new(helper::time_diff));
        templates.register_helper("json_pretty", Box::new(helper::json_pretty));
        Self {
            github_client,
            project_manager,
            templates,
        }
    }
    pub fn start<'scope>(&'a self, scope: &'scope thread::Scope<'scope, 'a>) -> Stopper {
        let server = sync::Arc::new(tiny_http::Server::http("0.0.0.0:8000").unwrap());
        let server_cloned = server.clone();
        scope.spawn(move || {
            eprintln!("[http_service] listening for requests");
            for request in server_cloned.incoming_requests() {
                match request.method() {
                    tiny_http::Method::Get => {}
                    _ => {
                        let response = tiny_http::Response::empty(tiny_http::StatusCode(405));
                        request.respond(response).unwrap();
                        continue;
                    }
                }
                let (data, content_type) = match request.url() {
                    "/" | "/index.html" => (self.index_html(), "text/html; charset=UTF-8"),
                    "/status.json" => (self.status_json(), "application/json; charset=UTF-8"),
                    _ => {
                        let response = tiny_http::Response::empty(tiny_http::StatusCode(404));
                        request.respond(response).unwrap();
                        continue;
                    }
                };
                let header = tiny_http::Header::from_bytes("Content-Type", content_type).unwrap();
                let response = tiny_http::Response::from_string(data).with_header(header);
                request.respond(response).unwrap();
            }
        });
        Stopper { server }
    }
    fn index_html(&self) -> String {
        let data = self.data();
        self.templates.render("status.html", &data).unwrap()
    }
    fn status_json(&self) -> String {
        let data = self.data();
        serde_json::to_string_pretty(&data).unwrap()
    }
    fn data(&self) -> Data {
        Data {
            projects: self.project_manager.projects(),
            rate_limit_info: self.github_client.rate_limit_info(),
        }
    }
}

pub struct Stopper {
    server: sync::Arc<tiny_http::Server>,
}

impl Stopper {
    pub fn stop(self) {
        eprintln!("[http_service] shutdown signal received");
        self.server.unblock();
        eprintln!("[http_service] unblocked listening thread; waiting to stop");
    }
}

static STATUS_DOT_HTML: &str = include_str!("status.html");

#[derive(Debug, serde::Serialize)]
struct Data {
    projects: Vec<project::Project>,
    rate_limit_info: github::RateLimiter,
}

mod helper {
    use handlebars::*;
    pub fn json_pretty(
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let param = h.param(0).unwrap();
        let value = param.value();
        let pretty = serde_json::to_string_pretty(value).unwrap();
        write!(out, "{}", pretty)?;
        Ok(())
    }

    pub fn time_diff(
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let param = h.param(0).unwrap();
        let i = param.value().render();
        if i.is_empty() {
            out.write("at an unknown time")?;
            return Ok(());
        }
        let i = format!("\"{i}\"");
        let t: chrono::DateTime<chrono::Utc> = serde_json::from_str(&i).unwrap();
        let now = chrono::Utc::now();
        if t < now {
            let d = now - t;
            let (i, s) = if d.num_minutes() < 60 {
                (d.num_minutes(), "minute")
            } else if d.num_hours() < 24 {
                (d.num_hours(), "hour")
            } else if d.num_days() < 15 {
                (d.num_days(), "day")
            } else {
                (d.num_weeks(), "week")
            };
            let p = if i == 1 { "" } else { "s" };
            write!(out, "{i} {s}{p} ago")?;
        } else {
            let d = t - now;
            write!(out, "in {} minutes", d.num_minutes())?;
        }
        Ok(())
    }
}
