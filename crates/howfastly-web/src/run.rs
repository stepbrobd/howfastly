use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use howfastly::types::{
    Direction, DirectionSummary, LOADED_PING_INTERVAL_MS, LatencySummary, MetaResponse, SizePlan,
    SizeSamples, SpeedtestResults, TestConfig, summarize_direction, summarize_latency,
};
use howfastly_map::chart::throughput_points;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;
use web_sys::AbortController;

use crate::engine;

const WINDOW_MS: f64 = 500.0;
const EMIT_MS: f64 = 100.0;

// the live state of one transfer direction
#[derive(Clone, Copy)]
pub struct Lane {
    pub dir: Direction,
    pub running: RwSignal<bool>,
    pub points: RwSignal<Vec<(f64, f64)>>,
    pub sizes: RwSignal<Vec<SizeSamples>>,
    pub summary: RwSignal<Option<DirectionSummary>>,
}

impl Lane {
    pub fn new(dir: Direction) -> Self {
        Self {
            dir,
            running: RwSignal::new(false),
            points: RwSignal::new(Vec::new()),
            sizes: RwSignal::new(Vec::new()),
            summary: RwSignal::new(None),
        }
    }

    fn reset(self) {
        self.running.set(false);
        self.points.set(Vec::new());
        self.sizes.set(Vec::new());
        self.summary.set(None);
    }
}

// what a run is doing, the controls and the loop both follow it
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    Paused,
    Cancelled,
}

#[derive(Clone, Copy)]
pub struct State {
    pub phase: RwSignal<Phase>,
    // the transfer in flight, pause and cancel abort it
    pub abort: StoredValue<Option<AbortController>, LocalStorage>,
    pub error: RwSignal<Option<String>>,
    pub notice: RwSignal<Option<String>>,
    pub meta: RwSignal<Option<MetaResponse>>,
    pub latency: RwSignal<Option<LatencySummary>>,
    pub down: Lane,
    pub up: Lane,
}

// per direction bookkeeping that survives across interleaved segments
struct DirRun {
    lane: Lane,
    plans: Vec<SizePlan>,
    events: Rc<RefCell<Vec<(f64, u64)>>>,
    active_ms: f64,
    out: Vec<SizeSamples>,
    loaded: Vec<f64>,
}

impl DirRun {
    fn new(lane: Lane, plans: Vec<SizePlan>) -> Self {
        Self {
            lane,
            plans,
            events: Rc::new(RefCell::new(Vec::new())),
            active_ms: 0.0,
            out: Vec::new(),
            loaded: Vec::new(),
        }
    }
}

// why a run stopped short of a finish
enum Interrupt {
    Cancelled,
    Failed(JsValue),
}

// one full run bracketed by the start and finish markers
// a cancelled run sends no finish and counts as abandoned
pub fn launch(state: State) {
    if state.phase.get_untracked() != Phase::Idle {
        return;
    }
    state.phase.set(Phase::Running);
    state.error.set(None);
    state.latency.set(None);
    state.down.reset();
    state.up.reset();
    spawn_local(async move {
        engine::start().await;
        let outcome = run_all(state).await;
        state.down.running.set(false);
        state.up.running.set(false);
        state.abort.set_value(None);
        state.phase.set(Phase::Idle);
        match outcome {
            Ok(()) => {
                engine::finish(&SpeedtestResults {
                    meta: state.meta.get_untracked(),
                    latency: state.latency.get_untracked(),
                    download: state.down.summary.get_untracked(),
                    upload: state.up.summary.get_untracked(),
                })
                .await
            }
            Err(Interrupt::Cancelled) => {}
            Err(Interrupt::Failed(e)) => state.error.set(Some(format!("{e:?}"))),
        }
    });
}

fn abort(state: State) {
    state.abort.with_value(|a| {
        if let Some(a) = a {
            a.abort();
        }
    });
}

// the transfer in flight is dropped and tried again after the resume
pub fn pause(state: State) {
    if state.phase.get_untracked() == Phase::Running {
        state.phase.set(Phase::Paused);
        abort(state);
    }
}

pub fn resume(state: State) {
    if state.phase.get_untracked() == Phase::Paused {
        state.phase.set(Phase::Running);
    }
}

pub fn cancel(state: State) {
    if matches!(state.phase.get_untracked(), Phase::Running | Phase::Paused) {
        state.phase.set(Phase::Cancelled);
        abort(state);
    }
}

// waits out a pause, the run only moves on while running
async fn hold(state: State) -> Result<(), Interrupt> {
    loop {
        match state.phase.get_untracked() {
            Phase::Running => return Ok(()),
            Phase::Paused => TimeoutFuture::new(100).await,
            Phase::Idle | Phase::Cancelled => return Err(Interrupt::Cancelled),
        }
    }
}

async fn run_all(state: State) -> Result<(), Interrupt> {
    let cfg = TestConfig::default();

    let mut pings = Vec::new();
    for _ in 0..cfg.latency_samples {
        hold(state).await?;
        pings.push(engine::ping().await.map_err(Interrupt::Failed)?);
    }
    state.latency.set(summarize_latency(&pings));

    // alternate size classes so both directions estimate early
    let mut down = DirRun::new(state.down, cfg.plans(Direction::Download).to_vec());
    let mut up = DirRun::new(state.up, cfg.plans(Direction::Upload).to_vec());
    for i in 0..down.plans.len().max(up.plans.len()) {
        for run in [&mut down, &mut up] {
            if let Some(&plan) = run.plans.get(i) {
                segment(state, run, plan, cfg.time_budget_secs).await?;
            }
        }
    }
    Ok(())
}

// one size class for one direction with its own loaded latency pinger
async fn segment(
    state: State,
    run: &mut DirRun,
    plan: SizePlan,
    budget_secs: f64,
) -> Result<(), Interrupt> {
    run.lane.running.set(true);
    let stop = Rc::new(Cell::new(false));
    let seg_loaded = Rc::new(RefCell::new(Vec::new()));

    spawn_local({
        let stop = stop.clone();
        let seg_loaded = seg_loaded.clone();
        async move {
            while !stop.get() {
                if state.phase.get_untracked() == Phase::Paused {
                    TimeoutFuture::new(100).await;
                    continue;
                }
                if let Ok(ms) = engine::ping().await {
                    seg_loaded.borrow_mut().push(ms);
                }
                TimeoutFuture::new(LOADED_PING_INTERVAL_MS).await;
            }
        }
    });

    let mut s = SizeSamples {
        bytes: plan.bytes,
        mbps: Vec::new(),
        skipped: false,
    };
    let outcome = transfers(state, run, plan, budget_secs, &mut s).await;
    stop.set(true);
    if let Err(e) = outcome {
        run.lane.running.set(false);
        return Err(e);
    }

    run.out.push(s);
    run.loaded.extend(seg_loaded.borrow().iter().copied());
    run.lane
        .points
        .set(throughput_points(&run.events.borrow(), WINDOW_MS, EMIT_MS));
    run.lane.sizes.set(run.out.clone());
    run.lane
        .summary
        .set(Some(summarize_direction(&run.out, &run.loaded)));
    run.lane.running.set(false);
    Ok(())
}

// the transfers of one size class, each one only counts once it completes
// a transfer aborted by a pause is wiped from the timeline and tried again
async fn transfers(
    state: State,
    run: &mut DirRun,
    plan: SizePlan,
    budget_secs: f64,
    s: &mut SizeSamples,
) -> Result<(), Interrupt> {
    let mut done = 0;
    while done < plan.iterations {
        hold(state).await?;
        if run.active_ms / 1e3 > budget_secs {
            s.skipped = true;
            return Ok(());
        }
        let abort = AbortController::new().map_err(Interrupt::Failed)?;
        state.abort.set_value(Some(abort.clone()));
        let start = engine::now_ms();
        let mark = run.events.borrow().len();
        let progress = recorder(run.lane, run.active_ms, start, run.events.clone());
        let sample = match run.lane.dir {
            Direction::Download => engine::download(plan.bytes, progress, &abort.signal()).await,
            Direction::Upload => engine::upload(plan.bytes, progress, &abort.signal()).await,
        };
        state.abort.set_value(None);
        match sample {
            Ok(mbps) => {
                run.active_ms += engine::now_ms() - start;
                s.mbps.push(mbps);
                done += 1;
                let mut live = run.out.clone();
                live.push(s.clone());
                run.lane.sizes.set(live);
            }
            Err(e) => {
                run.events.borrow_mut().truncate(mark);
                run.lane
                    .points
                    .set(throughput_points(&run.events.borrow(), WINDOW_MS, EMIT_MS));
                if state.phase.get_untracked() == Phase::Running {
                    return Err(Interrupt::Failed(e));
                }
            }
        }
    }
    Ok(())
}

// per transfer closure turning cumulative bytes into active timeline deltas
// the x axis counts only this direction's own transfer time
fn recorder(
    lane: Lane,
    base_ms: f64,
    seg_start: f64,
    events: Rc<RefCell<Vec<(f64, u64)>>>,
) -> impl FnMut(f64, u64) + 'static {
    let mut last_bytes = 0u64;
    let mut last_set = 0.0f64;
    move |now, cumulative| {
        let t = base_ms + (now - seg_start);
        let delta = cumulative.saturating_sub(last_bytes);
        last_bytes = cumulative;
        events.borrow_mut().push((t, delta));
        if t - last_set >= EMIT_MS {
            last_set = t;
            lane.points
                .set(throughput_points(&events.borrow(), WINDOW_MS, EMIT_MS));
        }
    }
}
