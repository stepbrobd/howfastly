use fastly::Request;
use fastly::http::{Url, header};
use howfastly::stats::{latency_bucket, speed_bucket};
use howfastly::types::{Direction, Outcome, Run};
use serde_json::{Map, json};

use crate::handlers;

const BACKEND: &str = "plausible";
const ENDPOINT: &str = "https://stats.ysun.co/api/event";
const DOMAIN: &str = "speed.edgecompute.app";

// what a request counts as, a run is bracketed by start and the way it ended
// the run is boxed so the enum stays as small as its unit variants
pub enum Event {
    Pageview,
    Start,
    Finish(Box<Run>),
    // a first publication of a result, a reuse counts nothing
    Share(String),
    // a view of a shared result page, json reads and heads count nothing
    View(String),
}

impl Event {
    fn name(&self) -> &'static str {
        match self {
            Self::Pageview => "pageview",
            Self::Start => "Start",
            Self::Finish(run) => match run.outcome {
                Outcome::Completed => "Finish",
                Outcome::Canceled { .. } => "Cancel",
                Outcome::Failed { .. } => "Fail",
                Outcome::Left { .. } => "Leave",
            },
            Self::Share(_) => "Share",
            Self::View(_) => "pageview",
        }
    }

    // measurements reach plausible as coarse buckets, never raw numbers
    // shared pages all count under one url and carry their id here instead
    fn props(&self) -> Vec<(&'static str, String)> {
        let mut props = Vec::new();
        match self {
            Self::Finish(run) => {
                if let Some(stage) = run.outcome.stage() {
                    props.push(("Stage", stage.label()));
                }
                for dir in Direction::ALL {
                    if let Some(mbps) = run.results.direction(dir).and_then(|d| d.p90) {
                        props.push((dir.name(), speed_bucket(mbps).to_string()));
                    }
                }
                if let Some(latency) = &run.results.latency {
                    props.push(("Latency", latency_bucket(latency.median).to_string()));
                }
            }
            Self::Share(id) | Self::View(id) => props.push(("Share", id.clone())),
            Self::Pageview | Self::Start => {}
        }
        props
    }

    fn url(&self, req: &Request) -> String {
        match self {
            Self::Share(_) | Self::View(_) => handlers::url_at(req.get_url(), "/share"),
            Self::Pageview | Self::Start | Self::Finish(_) => req.get_url_str().to_string(),
        }
    }
}

// the visitor's user agent and ip go along so plausible counts them, not the edge
// x-plausible-ip outranks x-forwarded-for, which caddy rewrites
// the dropped pending request keeps sending after the program exits
pub fn send(req: &Request, event: &Event) {
    let Some(agent) = req.get_header(header::USER_AGENT) else {
        return;
    };
    let Ok(agent_str) = agent.to_str() else {
        return;
    };
    let mut props = Map::new();
    props.insert("Client".into(), client(agent_str).into());
    props.insert("POP".into(), fastly::compute_runtime::pop().into());
    props.insert("Protocol".into(), crate::handlers::protocol(req).into());
    for (key, value) in event.props() {
        props.insert(key.into(), value.into());
    }
    let body = json!({
        "domain": DOMAIN,
        "name": event.name(),
        "url": event.url(req),
        "referrer": referrer(req),
        "props": props,
    });
    let Ok(mut event) = Request::post(ENDPOINT)
        .with_header(header::USER_AGENT, agent.clone())
        .with_header(header::CONTENT_TYPE, "application/json")
        .with_body_json(&body)
    else {
        return;
    };
    if let Some(ip) = req.get_client_ip_addr() {
        event.set_header("x-plausible-ip", ip.to_string());
        event.set_header("x-forwarded-for", ip.to_string());
    }
    let _ = event.send_async(BACKEND);
}

// the cli announces itself, everything else counts as a browser
fn client(agent: &str) -> &'static str {
    if agent.starts_with("HowFastly/") {
        "CLI"
    } else {
        "Web"
    }
}

// a referrer that is a shared page collapses the same way, whichever host served it
// so a click through to a new run does not carry the id either
fn referrer(req: &Request) -> String {
    let raw = req
        .get_header(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match Url::parse(raw) {
        Ok(url) if url.path().starts_with("/share/") => handlers::url_at(&url, "/share"),
        _ => raw.to_string(),
    }
}
