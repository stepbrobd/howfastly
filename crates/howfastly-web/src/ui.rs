use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use howfastly::chart::{chart_y, format_speed, peak, svg_path, throughput_points};
use howfastly::stats;
use howfastly::types::{
    DOWNLOAD_PLAN, DirectionSummary, LOADED_PING_INTERVAL_MS, LatencySummary, MetaResponse,
    SizePlan, SizeSamples, SpeedtestResults, TestConfig, UPLOAD_PLAN, size_label,
    summarize_direction, summarize_latency,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

use crate::engine;
use crate::map::Map;

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
    notice: RwSignal<Option<String>>,
    meta: RwSignal<Option<MetaResponse>>,
    latency: RwSignal<Option<LatencySummary>>,
    down: Direction,
    up: Direction,
}

#[component]
pub fn App() -> impl IntoView {
    let state = State {
        running: RwSignal::new(false),
        error: RwSignal::new(None),
        notice: RwSignal::new(None),
        meta: RwSignal::new(None),
        latency: RwSignal::new(None),
        down: Direction::new(false),
        up: Direction::new(true),
    };

    spawn_local(async move {
        match engine::meta().await {
            Ok(m) => {
                state
                    .notice
                    .set(m.mismatch().map(|w| format!("{w}, reload to update")));
                state.meta.set(Some(m));
            }
            Err(e) => state
                .notice
                .set(Some(e.as_string().unwrap_or_else(|| format!("{e:?}")))),
        }
    });

    // first visit gates behind the popup, later visits start right away
    let gate = RwSignal::new(!engine::autostart_saved());
    if !gate.get_untracked() {
        launch(state);
    }
    let begin = move |_| {
        engine::save_autostart();
        gate.set(false);
        launch(state);
    };

    view! {
        <main class="mx-auto flex min-h-screen w-full max-w-[65ch] flex-col gap-8 p-4 lg:max-w-6xl">
            <section class="overflow-x-auto rounded bg-nord-1 p-4">
                <div class="mx-auto w-max whitespace-nowrap font-mono">
                    {move || match state.meta.get() {
                        Some(m) => view! {
                            <a href=format!("https://bgp.tools/prefix/{}", m.ip)
                                target="_blank" rel="noopener">{m.ip.clone()}</a>
                            " ("
                            <a href=format!("https://bgp.tools/as/{}", m.asn)
                                target="_blank" rel="noopener">{format!("AS{}", m.asn)}</a>
                            ") @ "
                            {format!("{}, {}", m.city, m.country)}
                            " "
                            <svg
                                class="inline h-[1lh] w-[1em] align-bottom"
                                viewBox="0 0 16 16"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.5"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M2.5 8h11M9.5 4l4 4-4 4"/>
                            </svg>
                            " "
                            {m.pop.code.clone()}
                            {(!m.pop.name.is_empty()).then(|| {
                                if m.pop.group.is_empty() {
                                    format!(" ({})", m.pop.name)
                                } else {
                                    format!(" ({}, {})", m.pop.name, m.pop.group)
                                }
                            })}
                            {(!m.protocol.is_empty()).then(|| format!(" via {}", m.protocol))}
                        }
                            .into_any(),
                        None => view! { "-" }.into_any(),
                    }}
                </div>
            </section>

            <section class="rounded bg-nord-1 p-4">
                <Map meta=state.meta.into()/>
            </section>

            {move || state.notice.get().map(|n| view! {
                <div class="rounded border border-nord-13 bg-nord-1 p-4 text-nord-13">{n}</div>
            })}

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
                            state.down.summary.get().and_then(|d| d.loaded)
                        })
                    />
                    <LatencyCard
                        label="Upload loaded"
                        summary=Signal::derive(move || {
                            state.up.summary.get().and_then(|d| d.loaded)
                        })
                    />
                </div>
            </section>

            {move || state.error.get().map(|e| view! {
                <div class="rounded border border-nord-11 bg-nord-1 p-4 text-nord-11">{e}</div>
            })}

            <footer class="text-center">
                <p><small>
                    "Not an official "
                    <a href="https://www.fastly.com/" target="_blank" rel="noopener" referrerpolicy="origin">"Fastly"</a>
                    " product."
                </small></p>
                <p><small>
                    "Made by "
                    <a href="https://ysun.co" target="_blank" rel="noopener">"Yifei Sun"</a>
                    " aka "
                    <a href="https://github.com/stepbrobd" target="_blank" rel="noopener">"StepBroBD"</a>
                    ", source on "
                    <a href="https://github.com/stepbrobd/howfastly" target="_blank" rel="noopener">"GitHub"</a>
                    "."
                </small></p>
                <p><small>
                    <a href="https://crates.io/crates/howfastly" target="_blank" rel="noopener">"HowFastly"</a>
                    " "
                    <a
                        href=concat!(
                            "https://github.com/stepbrobd/howfastly/releases/tag/",
                            env!("CARGO_PKG_VERSION"),
                        )
                        target="_blank"
                        rel="noopener"
                    >
                        {env!("CARGO_PKG_VERSION")}
                    </a>
                </small></p>
            </footer>

            {move || gate.get().then(|| view! {
                <div class="fixed inset-0 z-10 flex items-center justify-center bg-nord-0/80 p-4">
                    <div class="w-full max-w-md rounded bg-nord-1 p-6">
                        <h2 class="text-lg font-semibold">HowFastly</h2>
                        <p class="mt-2">
                            "Tests run automatically and transfer up to ~640 MB in total. "
                            "Close the page early to spend less. "
                            "Tap a speed card to run the test again."
                        </p>
                        <button
                            class="mt-4 w-full cursor-pointer rounded bg-nord-10 px-8 py-3 text-nord-6 hover:bg-nord-9"
                            on:click=begin
                        >
                            "Start"
                        </button>
                    </div>
                </div>
            })}
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
        <div class="relative mt-2 h-24 w-full">
            {move || {
                let pts = dir.points.get();
                let p90 = dir.summary.get().and_then(|d| d.p90).map(|mbps| mbps * 1e6);
                // the scale grows to keep the reference line inside the frame
                let max = peak(&pts).max(p90.unwrap_or(0.0));
                let line = svg_path(&pts, CHART_W, CHART_H, max);
                if line.is_empty() {
                    return view! { <Waiting class="h-full bg-nord-0"/> }.into_any();
                }
                let area = format!("{line} L{CHART_W:.1},{CHART_H:.1} L0.0,{CHART_H:.1} Z");
                let mark = p90.map(|v| chart_y(v, max, CHART_H).max(1.0));
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
                        {mark.map(|y| view! {
                            <line
                                x1="0"
                                x2=format!("{CHART_W:.1}")
                                y1=format!("{y:.1}")
                                y2=format!("{y:.1}")
                                class="stroke-nord-4"
                                stroke-opacity="0.7"
                                stroke-dasharray="4 3"
                                vector-effect="non-scaling-stroke"
                            />
                        })}
                    </svg>
                    {mark.map(|y| {
                        // the label sits under a high line and above a low one
                        let side = if y < CHART_H / 2.0 { "" } else { "-translate-y-full" };
                        view! {
                            <small
                                class=format!("absolute right-1 text-nord-4 {side}")
                                style=format!("top:{:.1}%", y / CHART_H * 100.0)
                            >
                                "p90"
                            </small>
                        }
                    })}
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
                .and_then(|d| d.p90)
                .map(|p90| p90 * 1e6)
                .or(live)
        };
        bps.map(format_speed)
    };
    let rerun = move |_| launch(state);
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
            title="Run the test again"
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
                <span class="text-nord-4">
                    <small>
                        {move || {
                            let pts = dir.points.get();
                            if pts.is_empty() {
                                return String::new();
                            }
                            let (v, unit) = format_speed(peak(&pts));
                            format!("Peak {v:.1} {unit}")
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
                        {format!("Median {:.1} ms / Jitter {:.1} ms", s.median, s.jitter)}
                    </div>
                    <div><small>{format!("Min {:.1} / Avg {:.1}", s.min, s.avg)}</small></div>
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

// one full run bracketed by the start and finish markers
// a click during a run is ignored
fn launch(state: State) {
    if state.running.get_untracked() {
        return;
    }
    state.running.set(true);
    state.error.set(None);
    state.latency.set(None);
    state.down.reset();
    state.up.reset();
    spawn_local(async move {
        engine::start().await;
        let outcome = run_all(state).await;
        state.down.running.set(false);
        state.up.running.set(false);
        state.running.set(false);
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
            Err(e) => state.error.set(Some(format!("{e:?}"))),
        }
    });
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
                TimeoutFuture::new(LOADED_PING_INTERVAL_MS).await;
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
