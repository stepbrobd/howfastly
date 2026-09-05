use std::rc::Rc;

use howfastly::share::{
    Client, FORMAT, Payload, Report, ShareResponse, SharedDirection, Timeline, error_message,
    valid_id,
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

// the status stands in when the server did not explain
fn explain(status: u16, body: &str) -> String {
    error_message(body).unwrap_or_else(|| format!("The server answered {status}."))
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

// the embedded report of a shared page, or its json twin when the shell came without one
// none for anything that cannot be shown, the server explained every miss on its own page
pub async fn load(id: String) -> Option<Report> {
    if !valid_id(&id) {
        return None;
    }
    if let Some(text) = engine::embedded(EMBED) {
        return decode(&text);
    }
    match engine::report(&id).await {
        Ok((200, body)) => decode(&body),
        _ => None,
    }
}

// a record of another format is not read, whatever its shape
fn decode(text: &str) -> Option<Report> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.get("format")?.as_u64()? != u64::from(FORMAT) {
        return None;
    }
    serde_json::from_value(value).ok()
}
