use fastly::Request;
use fastly::http::header;
use howfastly::stats::{latency_bucket, speed_bucket};
use howfastly::types::{Direction, SpeedtestResults};
use serde_json::{Map, json};

const BACKEND: &str = "plausible";
const ENDPOINT: &str = "https://stats.ysun.co/api/event";
const DOMAIN: &str = "speed.edgecompute.app";

// what a request counts as, a run is bracketed by start and finish
// the report is boxed so the enum stays as small as its unit variants
pub enum Event {
    Pageview,
    Start,
    Finish(Box<SpeedtestResults>),
}

impl Event {
    fn name(&self) -> &'static str {
        match self {
            Self::Pageview => "pageview",
            Self::Start => "Start",
            Self::Finish(_) => "Finish",
        }
    }

    // measurements reach plausible as coarse buckets, never raw numbers
    fn props(&self) -> Vec<(&'static str, String)> {
        let Self::Finish(results) = self else {
            return Vec::new();
        };
        let mut props = Vec::new();
        for dir in Direction::ALL {
            if let Some(mbps) = results.direction(dir).and_then(|d| d.p90) {
                props.push((dir.name(), speed_bucket(mbps).to_string()));
            }
        }
        if let Some(latency) = &results.latency {
            props.push(("Latency", latency_bucket(latency.median).to_string()));
        }
        props
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
    let referrer = req
        .get_header(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let body = json!({
        "domain": DOMAIN,
        "name": event.name(),
        "url": req.get_url_str(),
        "referrer": referrer,
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
