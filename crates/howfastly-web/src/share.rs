use std::rc::Rc;

use howfastly::share::{
    Client, FORMAT, Payload, Report, ShareResponse, SharedDirection, Timeline, valid_id,
};
use howfastly::types::{SpeedtestResults, TestConfig};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::engine;
use crate::run::{Lane, State};

// where the completed snapshot stands with the server, the next run resets it
#[derive(Clone, PartialEq, Eq)]
pub enum Share {
    Ready,
    Publishing,
    Published {
        url: String,
        expires_at: u64,
        clip: Clip,
    },
    Failed(String),
}

// what the clipboard did with the link, the link stays on screen either way
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    Pending,
    Copied,
    Failed,
}

// the payload of a run that just completed, frozen with its raw samples and chart
pub fn snapshot(state: State, config: TestConfig, results: &SpeedtestResults) {
    let mut payload = Payload::from_results(Client::Web, engine::unix_secs(), config, results);
    attach(&mut payload.download, state.down);
    attach(&mut payload.upload, state.up);
    state.snapshot.set(Some(Rc::new(payload)));
}

fn attach(dir: &mut Option<SharedDirection>, lane: Lane) {
    if let Some(d) = dir {
        d.samples = Some(lane.sizes.get_untracked());
        d.timeline = Timeline::from_points(&lane.points.get_untracked());
    }
}

// one publication in flight at a time, a retry posts the same snapshot
// a published link is copied again rather than posted again
pub fn publish(state: State) {
    let Some(payload) = state.snapshot.get_untracked() else {
        return;
    };
    match state.share.get_untracked() {
        Share::Publishing => return,
        Share::Published {
            url, expires_at, ..
        } => {
            copy(state, payload, url, expires_at);
            return;
        }
        Share::Ready | Share::Failed(_) => {}
    }
    state.share.set(Share::Publishing);
    spawn_local(async move {
        let outcome = post(&payload).await;
        // a run that started meanwhile owns the state now
        if !current(state, &payload) {
            return;
        }
        match outcome {
            Ok(r) => copy(state, payload, r.url, r.expires_at),
            Err(e) => state.share.set(Share::Failed(e)),
        }
    });
}

// the snapshot a response belongs to is the one still on screen
fn current(state: State, payload: &Rc<Payload>) -> bool {
    state
        .snapshot
        .with_untracked(|s| s.as_ref().is_some_and(|s| Rc::ptr_eq(s, payload)))
}

// the link shows before the clipboard answers and stays when it refuses
// the write starts right here so a click still counts as the gesture some browsers require
fn copy(state: State, payload: Rc<Payload>, url: String, expires_at: u64) {
    state.share.set(Share::Published {
        url: url.clone(),
        expires_at,
        clip: Clip::Pending,
    });
    let write = engine::copy(&url);
    spawn_local(async move {
        let clip = match write.await {
            Ok(_) => Clip::Copied,
            Err(_) => Clip::Failed,
        };
        if current(state, &payload) {
            state.share.set(Share::Published {
                url,
                expires_at,
                clip,
            });
        }
    });
}

// a 200 or 201 carries the link, anything else explains itself
async fn post(payload: &Payload) -> Result<ShareResponse, String> {
    let json = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let (status, body) = engine::share(&json).await.map_err(engine::describe)?;
    match status {
        200 | 201 => serde_json::from_str(&body)
            .map_err(|e| format!("The share response could not be read, {e}.")),
        _ => Err(explain(status, &body)),
    }
}

// the server explains errors as {"error": "..."}, the status stands in when it does not
fn explain(status: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error")?.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("The server answered {status}."))
}

// the id under a shared route, any path under /share stays out of the live app
pub fn route() -> Option<String> {
    let path = engine::pathname();
    match path.strip_prefix("/share") {
        Some("") => Some(String::new()),
        Some(rest) => rest.strip_prefix('/').map(str::to_string),
        None => None,
    }
}

// the element the server embeds the report in
const EMBED: &str = "howfastly-report";

// why a shared result cannot be shown, each one gets its own page
#[derive(Clone, PartialEq, Eq)]
pub enum Problem {
    // the path or the record is not something this build reads
    Invalid(String),
    // the server has no live record, its explanation says whether it expired
    Missing(String),
    Unsupported(String),
    // the server or the network failed, not the record
    Unavailable(String),
}

// the embedded report of a shared page, or its json twin when the shell came without one
pub async fn load(id: String) -> Result<Report, Problem> {
    if !valid_id(&id) {
        return Err(Problem::Invalid(
            "This is not a link to a shared result.".into(),
        ));
    }
    if let Some(text) = engine::embedded(EMBED) {
        return decode(&text);
    }
    match engine::report(&id).await {
        Ok((200, body)) => decode(&body),
        Ok((404, body)) => Err(Problem::Missing(explain(404, &body))),
        Ok((422, body)) => Err(Problem::Unsupported(explain(422, &body))),
        Ok((status, body)) => Err(Problem::Unavailable(explain(status, &body))),
        Err(e) => Err(Problem::Unavailable(format!(
            "The shared result could not be loaded. {}",
            engine::describe(e)
        ))),
    }
}

// the format is checked before the shape so a newer record says so instead of failing to parse
fn decode(text: &str) -> Result<Report, Problem> {
    let unreadable = |e: serde_json::Error| {
        Problem::Invalid(format!("The shared result could not be read, {e}."))
    };
    let value: serde_json::Value = serde_json::from_str(text).map_err(unreadable)?;
    match value.get("format").and_then(serde_json::Value::as_u64) {
        Some(f) if f == u64::from(FORMAT) => {}
        Some(f) => {
            return Err(Problem::Unsupported(format!(
                "Format {f} is unknown to this build ({}), reload to update.",
                howfastly::VERSION
            )));
        }
        None => {
            return Err(Problem::Invalid(
                "The shared result names no format.".into(),
            ));
        }
    }
    serde_json::from_value(value).map_err(unreadable)
}
