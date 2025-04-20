//! Module http contains a HTTP service that exposes status information about the agent.

use std::sync;
use std::sync::Arc;
use std::thread;

use crate::{github, project};

pub struct Service {
    server: sync::Arc<tiny_http::Server>,
    listening_thread: thread::JoinHandle<()>,
}

impl Service {
    pub fn create_and_start(
        github_client: Arc<github::Client>,
        project_manager: Arc<project::Manager>,
    ) -> Self {
        let server = sync::Arc::new(tiny_http::Server::http("0.0.0.0:8000").unwrap());
        let server_cloned = server.clone();
        let handlers = Handlers {
            github_client,
            project_manager,
        };
        let listening_thread = thread::spawn(move || {
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
                    "/" | "/index.html" => (handlers.index_html(), "text/html; charset=UTF-8"),
                    "/data.json" => (handlers.status_json(), "application/json; charset=UTF-8"),
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
        Self {
            server,
            listening_thread,
        }
    }

    pub fn shutdown(self) {
        eprintln!("[http_service] shutdown signal received");
        self.server.unblock();
        eprintln!("[http_service] unblocked listening thread; waiting to stop");
        self.listening_thread.join().unwrap();
        eprintln!("[http_service] shutdown complete");
    }
}

struct Handlers {
    github_client: Arc<github::Client>,
    project_manager: Arc<project::Manager>,
}

static STATUS_DOT_HTML: &str = include_str!("status.html");

#[derive(Debug, serde::Serialize)]
struct Data {
    projects: Vec<project::Project>,
    rate_limit_info: github::RateLimiter,
}

impl Handlers {
    fn index_html(&self) -> String {
        let data = Data {
            projects: self.project_manager.projects(),
            rate_limit_info: self.github_client.rate_limit_info(),
        };
        let mut tt = handlebars::Handlebars::new();
        tt.register_template_string("status.html", STATUS_DOT_HTML)
            .unwrap();
        tt.register_helper("time_diff", Box::new(helper::time_diff));
        let rendered = tt.render("status.html", &data).unwrap();
        rendered
    }
    fn status_json(&self) -> String {
        "".into()
    }
}

mod helper {
    use handlebars::*;
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
            if d.num_minutes() < 60 {
                write!(out, "{} minutes ago", d.num_minutes())?;
            } else if d.num_hours() < 24 {
                write!(out, "{} hours ago", d.num_hours())?;
            } else if d.num_days() < 15 {
                write!(out, "{} days ago", d.num_days())?;
            } else {
                write!(out, "{} weeks ago", d.num_weeks())?;
            }
        } else {
            let d = t - now;
            write!(out, "in {} minutes", d.num_minutes())?;
        }
        Ok(())
    }
}
