use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::chart::{format_speed, svg_path, throughput_points};
use common::stats;
use common::types::{
    DOWNLOAD_PLAN, DirectionSummary, LOADED_PING_INTERVAL_MS, LatencySummary, MetaResponse,
    SizePlan, SizeSamples, TestConfig, UPLOAD_PLAN, size_label, summarize_direction,
    summarize_latency,
};
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

use crate::engine;

const WINDOW_MS: f64 = 500.0;
const EMIT_MS: f64 = 100.0;

#[derive(Clone, Copy)]
struct Direction {
    upload: bool,
    running: RwSignal<bool>,
    points: RwSignal<Vec<(f64, f64)>>,
    sizes: RwSignal<Vec<SizeSamples>>,
    summary: RwSignal<Option<DirectionSummary>>,
}

impl Direction {
    fn new(upload: bool) -> Self {
        Self {
            upload,
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

#[derive(Clone, Copy)]
struct State {
    running: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    latency: RwSignal<Option<LatencySummary>>,
    down: Direction,
    up: Direction,
}

#[component]
pub fn App() -> impl IntoView {
    let meta = RwSignal::new(None::<MetaResponse>);
    let state = State {
        running: RwSignal::new(true),
        error: RwSignal::new(None),
        latency: RwSignal::new(None),
        down: Direction::new(false),
        up: Direction::new(true),
    };

    spawn_local(async move {
        if let Ok(m) = engine::meta().await {
            meta.set(Some(m));
        }
    });

    // start measuring as soon as the page loads
    spawn_local(async move {
        if let Err(e) = run_all(state).await {
            state.error.set(Some(format!("{e:?}")));
        }
        state.down.running.set(false);
        state.up.running.set(false);
        state.running.set(false);
    });

    view! {
        <main class="mx-auto flex min-h-screen w-full max-w-[65ch] flex-col gap-8 p-4 lg:max-w-6xl">
            <header>
                <h1 class="text-2xl font-black">HowFastly</h1>
                {move || meta.get().map(|m| view! {
                    <p><small>
                        <a href=format!("https://bgp.tools/prefix/{}", m.client_ip)
                            target="_blank" rel="noopener">{m.client_ip.clone()}</a>
                        " | "
                        <a href=format!("https://bgp.tools/as/{}", m.asn)
                            target="_blank" rel="noopener">{format!("AS{}", m.asn)}</a>
                        {format!(" {} | {}, {} -> POP {}",
                            m.as_org, m.city, m.country, m.pop)}
                    </small></p>
                })}
            </header>

            <div class="grid gap-8 lg:grid-cols-2">
                <section class="flex flex-col gap-4">
                    <Headline label="Download" dir=state.down state=state/>
                    <SizeTable title="Download" dir=state.down/>
                </section>
                <section class="flex flex-col gap-4">
                    <Headline label="Upload" dir=state.up state=state/>
                    <SizeTable title="Upload" dir=state.up/>
                </section>
            </div>

            <section class="rounded bg-nord-1 p-4">
                <h2 class="font-semibold">Latency</h2>
                <div class="mt-2 grid gap-4 sm:grid-cols-3">
                    <LatencyCard label="Unloaded" summary=state.latency.into()/>
                    <LatencyCard
                        label="Download loaded"
                        summary=Signal::derive(move || {
                            state.down.summary.get().and_then(|d| d.loaded_latency)
                        })
                    />
                    <LatencyCard
                        label="Upload loaded"
                        summary=Signal::derive(move || {
                            state.up.summary.get().and_then(|d| d.loaded_latency)
                        })
                    />
                </div>
            </section>

            {move || state.error.get().map(|e| view! {
                <div class="rounded border border-nord-11 bg-nord-1 p-4 text-nord-11">{e}</div>
            })}

            <p class="text-nord-3"><small>
                "Tests run automatically and transfer up to ~640 MB in total. "
                "Close the page early to spend less. "
                "Tap a speed card to run that test again."
            </small></p>
        </main>
    }
}

const CHART_W: f64 = 300.0;
const CHART_H: f64 = 80.0;

#[component]
fn Waiting(class: &'static str) -> impl IntoView {
    view! {
        <div class=format!("flex items-center justify-center rounded text-nord-3 {class}")>
            <small>"Waiting for measurements..."</small>
        </div>
    }
}

#[component]
fn SpeedChart(dir: Direction) -> impl IntoView {
    let (stroke, fill) = if dir.upload {
        ("stroke-nord-12", "fill-nord-12")
    } else {
        ("stroke-nord-8", "fill-nord-8")
    };
    view! {
        <div class="mt-2 h-24 w-full">
            {move || {
                let pts = dir.points.get();
                let line = svg_path(&pts, CHART_W, CHART_H);
                if line.is_empty() {
                    return view! { <Waiting class="h-full bg-nord-0"/> }.into_any();
                }
                let area = format!("{line} L{CHART_W:.1},{CHART_H:.1} L0.0,{CHART_H:.1} Z");
                view! {
                    <svg
                        class="h-full w-full"
                        viewBox=format!("0 0 {CHART_W} {CHART_H}")
                        preserveAspectRatio="none"
                    >
                        <path d=area class=fill fill-opacity="0.15" stroke="none"/>
                        <path
                            d=line
                            class=stroke
                            fill="none"
                            stroke-width="2"
                            stroke-linejoin="round"
                            stroke-linecap="round"
                            vector-effect="non-scaling-stroke"
                        />
                    </svg>
                }
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn Headline(label: &'static str, dir: Direction, state: State) -> impl IntoView {
    // live estimate while this direction transfers, p90 once summarized
    let speed = move || {
        let live = dir.points.get().last().map(|&(_, bps)| bps);
        let bps = if dir.running.get() {
            live
        } else {
            dir.summary
                .get()
                .and_then(|d| d.p90_mbps)
                .map(|p90| p90 * 1e6)
                .or(live)
        };
        bps.map(format_speed)
    };
    let rerun = move |_| {
        if state.running.get() {
            return;
        }
        state.running.set(true);
        state.error.set(None);
        dir.reset();
        spawn_local(async move {
            if let Err(e) = run_one(dir).await {
                state.error.set(Some(format!("{e:?}")));
            }
            dir.running.set(false);
            state.running.set(false);
        });
    };
    view! {
        <div
            class=move || {
                let cursor = if state.running.get() {
                    "cursor-wait"
                } else {
                    "cursor-pointer hover:ring-1 hover:ring-nord-3"
                };
                format!("flex-1 rounded bg-nord-1 p-4 {cursor}")
            }
            title="Run this test again"
            on:click=rerun
        >
            <div class="font-mono text-4xl text-nord-6">
                {move || match speed() {
                    Some((v, unit)) => view! {
                        {format!("{v:.1}")}
                        <span class="pl-1 text-base text-nord-4">{unit}</span>
                    }
                        .into_any(),
                    None => view! { "-" }.into_any(),
                }}
            </div>
            <div class="flex justify-between">
                <span>{label}</span>
                <span class="text-nord-3">
                    <small>
                        {move || {
                            let peak = dir
                                .points
                                .get()
                                .iter()
                                .map(|&(_, b)| b)
                                .fold(0.0, f64::max);
                            if peak > 0.0 {
                                let (v, unit) = format_speed(peak);
                                format!("Peak {v:.1} {unit}")
                            } else {
                                String::new()
                            }
                        }}
                    </small>
                </span>
            </div>
            <SpeedChart dir=dir/>
        </div>
    }
}

#[component]
fn LatencyCard(label: &'static str, summary: Signal<Option<LatencySummary>>) -> impl IntoView {
    view! {
        <div>
            <div><small>{label}</small></div>
            {move || match summary.get() {
                Some(s) => view! {
                    <div class="text-nord-6">
                        {format!("Median {:.1} ms / Jitter {:.1} ms", s.median_ms, s.jitter_ms)}
                    </div>
                    <div><small>{format!("Min {:.1} / Avg {:.1}", s.min_ms, s.avg_ms)}</small></div>
                }
                    .into_any(),
                None => view! {
                    <div class="text-nord-3">"Median - / Jitter -"</div>
                    <div class="text-nord-3"><small>"Min - / Avg -"</small></div>
                }
                    .into_any(),
            }}
        </div>
    }
}

#[component]
fn BoxPlot(samples: Vec<f64>, max: f64, upload: bool) -> impl IntoView {
    let (stroke, fill) = if upload {
        ("stroke-nord-12", "fill-nord-12")
    } else {
        ("stroke-nord-8", "fill-nord-8")
    };
    if samples.is_empty() {
        return view! { <svg class="block h-4 w-full"></svg> }.into_any();
    }
    let x = move |v: f64| (v / max * 100.0).clamp(0.0, 100.0);
    let q = |p: f64| stats::percentile(&samples, p).unwrap_or(0.0);
    let (lo, q1, med, q3, hi) = (q(0.0), q(25.0), q(50.0), q(75.0), q(100.0));
    let ticks = samples
        .iter()
        .map(|&s| {
            view! {
                <line
                    x1=format!("{:.1}", x(s))
                    x2=format!("{:.1}", x(s))
                    y1="12"
                    y2="16"
                    class=stroke
                    stroke-opacity="0.5"
                    vector-effect="non-scaling-stroke"
                />
            }
        })
        .collect_view();
    view! {
        <svg class="block h-4 w-full" viewBox="0 0 100 16" preserveAspectRatio="none">
            <line
                x1=format!("{:.1}", x(lo))
                x2=format!("{:.1}", x(hi))
                y1="6"
                y2="6"
                class=stroke
                stroke-opacity="0.6"
                vector-effect="non-scaling-stroke"
            />
            <rect
                x=format!("{:.1}", x(q1))
                width=format!("{:.1}", (x(q3) - x(q1)).max(0.5))
                y="2"
                height="8"
                class=fill
                fill-opacity="0.4"
            />
            <line
                x1=format!("{:.1}", x(med))
                x2=format!("{:.1}", x(med))
                y1="1"
                y2="11"
                class=stroke
                stroke-width="2"
                vector-effect="non-scaling-stroke"
            />
            {ticks}
        </svg>
    }
    .into_any()
}

#[component]
fn SizeTable(title: &'static str, dir: Direction) -> impl IntoView {
    // render every planned size from the start so the height never changes
    let plans: &'static [SizePlan] = if dir.upload {
        &UPLOAD_PLAN
    } else {
        &DOWNLOAD_PLAN
    };
    view! {
        {move || {
            let live = dir.sizes.get();
            let sizes: Vec<(SizeSamples, usize)> = plans
                .iter()
                .map(|&SizePlan { bytes, iterations }| {
                    let s = live
                        .iter()
                        .find(|s| s.bytes == bytes)
                        .cloned()
                        .unwrap_or(SizeSamples {
                            bytes,
                            mbps: Vec::new(),
                            skipped: false,
                        });
                    (s, iterations)
                })
                .collect();
            let max = sizes
                .iter()
                .flat_map(|(s, _)| s.mbps.iter().copied())
                .fold(f64::EPSILON, f64::max);
            view! {
                <table class="w-full border-separate border-spacing-0 overflow-hidden rounded border border-nord-3">
                    <thead>
                        <tr>
                            <th class="border-b border-nord-3 bg-nord-0 px-4 py-2 text-left font-semibold text-nord-6">
                                {title}
                            </th>
                            <th class="border-b border-nord-3 bg-nord-0 px-4 py-2 text-left font-semibold text-nord-6">
                                "Median"
                            </th>
                            <th class="w-1/2 border-b border-nord-3 bg-nord-0 px-4 py-2"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {sizes
                            .into_iter()
                            .map(|(s, iterations)| {
                                let label = format!(
                                    "{} ({}/{})",
                                    size_label(s.bytes),
                                    s.mbps.len(),
                                    iterations,
                                );
                                let text = match (stats::median(&s.mbps), s.skipped) {
                                    (Some(m), _) => {
                                        let (v, unit) = format_speed(m * 1e6);
                                        format!("{v:.1} {unit}")
                                    }
                                    (None, true) => "Skipped".to_string(),
                                    (None, false) => "-".to_string(),
                                };
                                view! {
                                    <tr class="odd:bg-nord-1">
                                        <td class="px-4 py-2">{label}</td>
                                        <td class="px-4 py-2 font-mono">{text}</td>
                                        <td class="px-4 py-2">
                                            <BoxPlot samples=s.mbps max=max upload=dir.upload/>
                                        </td>
                                    </tr>
                                }
                            })
                            .collect_view()}
                    </tbody>
                </table>
            }
        }}
    }
}

// per direction bookkeeping that survives across interleaved segments
struct DirRun {
    dir: Direction,
    plans: Vec<SizePlan>,
    events: Rc<RefCell<Vec<(f64, u64)>>>,
    active_ms: f64,
    out: Vec<SizeSamples>,
    loaded: Vec<f64>,
}

impl DirRun {
    fn new(dir: Direction, plans: Vec<SizePlan>) -> Self {
        Self {
            dir,
            plans,
            events: Rc::new(RefCell::new(Vec::new())),
            active_ms: 0.0,
            out: Vec::new(),
            loaded: Vec::new(),
        }
    }
}

async fn run_all(state: State) -> Result<(), JsValue> {
    let cfg = TestConfig::default();

    let mut pings = Vec::new();
    for _ in 0..cfg.latency_samples {
        pings.push(engine::ping().await?);
    }
    state.latency.set(summarize_latency(&pings));

    // alternate size classes so both directions estimate early
    let mut down = DirRun::new(state.down, cfg.download.clone());
    let mut up = DirRun::new(state.up, cfg.upload.clone());
    for i in 0..down.plans.len().max(up.plans.len()) {
        for run in [&mut down, &mut up] {
            if let Some(&plan) = run.plans.get(i) {
                segment(run, plan, cfg.time_budget_secs).await?;
            }
        }
    }
    Ok(())
}

async fn run_one(dir: Direction) -> Result<(), JsValue> {
    let cfg = TestConfig::default();
    let plans = if dir.upload { cfg.upload } else { cfg.download };
    let mut run = DirRun::new(dir, plans.clone());
    for plan in plans {
        segment(&mut run, plan, cfg.time_budget_secs).await?;
    }
    Ok(())
}

// one size class for one direction with its own loaded latency pinger
async fn segment(run: &mut DirRun, plan: SizePlan, budget_secs: f64) -> Result<(), JsValue> {
    run.dir.running.set(true);
    let stop = Rc::new(Cell::new(false));
    let seg_loaded = Rc::new(RefCell::new(Vec::new()));

    spawn_local({
        let stop = stop.clone();
        let seg_loaded = seg_loaded.clone();
        async move {
            while !stop.get() {
                if let Ok(ms) = engine::ping().await {
                    seg_loaded.borrow_mut().push(ms);
                }
                TimeoutFuture::new(LOADED_PING_INTERVAL_MS as u32).await;
            }
        }
    });

    let seg_start = engine::now_ms();
    let mut s = SizeSamples {
        bytes: plan.bytes,
        mbps: Vec::new(),
        skipped: false,
    };
    for _ in 0..plan.iterations {
        if (run.active_ms + engine::now_ms() - seg_start) / 1e3 > budget_secs {
            s.skipped = true;
            break;
        }
        let progress = recorder(run.dir, run.active_ms, seg_start, run.events.clone());
        let sample = if run.dir.upload {
            engine::upload(plan.bytes, progress).await
        } else {
            engine::download(plan.bytes, progress).await
        };
        let mbps = match sample {
            Ok(mbps) => mbps,
            Err(e) => {
                stop.set(true);
                run.dir.running.set(false);
                return Err(e);
            }
        };
        s.mbps.push(mbps);
        let mut live = run.out.clone();
        live.push(s.clone());
        run.dir.sizes.set(live);
    }
    stop.set(true);

    run.active_ms += engine::now_ms() - seg_start;
    run.out.push(s);
    run.loaded.extend(seg_loaded.borrow().iter().copied());
    run.dir
        .points
        .set(throughput_points(&run.events.borrow(), WINDOW_MS, EMIT_MS));
    run.dir.sizes.set(run.out.clone());
    run.dir
        .summary
        .set(Some(summarize_direction(&run.out, &run.loaded)));
    run.dir.running.set(false);
    Ok(())
}

// per transfer closure turning cumulative bytes into active timeline deltas
// the x axis counts only this direction's own transfer time
fn recorder(
    dir: Direction,
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
            dir.points
                .set(throughput_points(&events.borrow(), WINDOW_MS, EMIT_MS));
        }
    }
}
