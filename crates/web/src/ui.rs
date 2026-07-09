use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::chart::{format_speed, svg_path, throughput_points};
use common::stats;
use common::types::{
    DirectionSummary, ITERATIONS, LOADED_PING_INTERVAL_MS, LatencySummary, MetaResponse,
    SizeSamples, TestConfig, size_label, summarize_direction, summarize_latency,
};
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

use crate::engine;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Latency,
    Download,
    Upload,
    Done,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Latency => "Measuring latency...",
            Self::Download => "Measuring download...",
            Self::Upload => "Measuring upload...",
            Self::Done => "Done",
        }
    }

    fn running(self) -> bool {
        !matches!(self, Self::Idle | Self::Done)
    }
}

const WINDOW_MS: f64 = 500.0;
const EMIT_MS: f64 = 100.0;

#[derive(Clone, Copy)]
struct Direction {
    upload: bool,
    points: RwSignal<Vec<(f64, f64)>>,
    sizes: RwSignal<Vec<SizeSamples>>,
    summary: RwSignal<Option<DirectionSummary>>,
}

impl Direction {
    fn new(upload: bool) -> Self {
        Self {
            upload,
            points: RwSignal::new(Vec::new()),
            sizes: RwSignal::new(Vec::new()),
            summary: RwSignal::new(None),
        }
    }

    fn reset(self) {
        self.points.set(Vec::new());
        self.sizes.set(Vec::new());
        self.summary.set(None);
    }
}

#[derive(Clone, Copy)]
struct State {
    phase: RwSignal<Phase>,
    error: RwSignal<Option<String>>,
    latency: RwSignal<Option<LatencySummary>>,
    down: Direction,
    up: Direction,
}

#[component]
pub fn App() -> impl IntoView {
    let meta = RwSignal::new(None::<MetaResponse>);
    let state = State {
        phase: RwSignal::new(Phase::Idle),
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

    let start = move |_| {
        if state.phase.get().running() {
            return;
        }
        state.error.set(None);
        state.latency.set(None);
        state.down.reset();
        state.up.reset();
        spawn_local(async move {
            if let Err(e) = run_test(state).await {
                state.error.set(Some(format!("{e:?}")));
            }
            state.phase.set(Phase::Done);
        });
    };

    view! {
        <main class="mx-auto flex min-h-screen w-full max-w-[65ch] flex-col gap-8 p-4">
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

            <section class="flex flex-col gap-4 sm:flex-row">
                <Headline label="Download" dir=state.down/>
                <Headline label="Upload" dir=state.up/>
            </section>

            <section class="flex flex-wrap gap-4">
                <LatencyCard label="Latency (unloaded)" summary=state.latency.into()/>
                <LatencyCard
                    label="Latency (download loaded)"
                    summary=Signal::derive(move || {
                        state.down.summary.get().and_then(|d| d.loaded_latency)
                    })
                />
                <LatencyCard
                    label="Latency (upload loaded)"
                    summary=Signal::derive(move || {
                        state.up.summary.get().and_then(|d| d.loaded_latency)
                    })
                />
            </section>

            <SizeTable title="Download" dir=state.down/>
            <SizeTable title="Upload" dir=state.up/>

            {move || state.error.get().map(|e| view! {
                <div class="rounded border border-nord-11 bg-nord-1 p-4 text-nord-11">{e}</div>
            })}
            <p>{move || state.phase.get().label()}</p>

            <button
                class="w-fit cursor-pointer rounded bg-nord-10 px-8 py-3 text-nord-6 hover:bg-nord-9 disabled:cursor-wait disabled:bg-nord-3"
                on:click=start
                disabled=move || state.phase.get().running()
            >
                {move || if state.phase.get() == Phase::Idle { "Start" } else { "Run again" }}
            </button>
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
                let peak = pts.iter().map(|&(_, b)| b).fold(0.0, f64::max);
                let (v, unit) = format_speed(peak);
                view! {
                    <div class="relative h-full">
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
                        <div class="absolute top-0 left-0 text-nord-3">
                            <small>{format!("Peak {v:.1} {unit}")}</small>
                        </div>
                    </div>
                }
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn Headline(label: &'static str, dir: Direction) -> impl IntoView {
    let speed = move || {
        let bps = match dir.summary.get().and_then(|d| d.p90_mbps) {
            Some(p90) => Some(p90 * 1e6),
            None => dir.points.get().last().map(|&(_, bps)| bps),
        };
        bps.map(format_speed)
    };
    view! {
        <div class="flex-1 rounded bg-nord-1 p-4">
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
            <div>{label}</div>
            <SpeedChart dir=dir/>
        </div>
    }
}

#[component]
fn LatencyCard(label: &'static str, summary: Signal<Option<LatencySummary>>) -> impl IntoView {
    view! {
        <div class="flex-1 basis-48 rounded bg-nord-1 p-4">
            <div><small>{label}</small></div>
            {move || match summary.get() {
                Some(s) => view! {
                    <div class="text-nord-6">
                        {format!("Median {:.1} ms / jitter {:.1} ms", s.median_ms, s.jitter_ms)}
                    </div>
                    <div><small>{format!("Min {:.1} / avg {:.1}", s.min_ms, s.avg_ms)}</small></div>
                }
                    .into_any(),
                None => view! { <div class="text-nord-3">"-"</div> }.into_any(),
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
    view! {
        {move || {
            let sizes = dir.sizes.get();
            if sizes.is_empty() {
                return view! { <Waiting class="h-24 bg-nord-1"/> }.into_any();
            }
            let max = sizes
                .iter()
                .flat_map(|s| s.mbps.iter().copied())
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
                            .map(|s| {
                                let label = format!(
                                    "{} ({}/{})",
                                    size_label(s.bytes),
                                    s.mbps.len(),
                                    ITERATIONS,
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
                .into_any()
        }}
    }
}

async fn run_test(state: State) -> Result<(), JsValue> {
    let cfg = TestConfig::default();

    state.phase.set(Phase::Latency);
    let mut pings = Vec::new();
    for _ in 0..cfg.latency_samples {
        pings.push(engine::ping().await?);
    }
    state.latency.set(summarize_latency(&pings));

    for dir in [state.down, state.up] {
        state.phase.set(if dir.upload {
            Phase::Upload
        } else {
            Phase::Download
        });
        let (sizes, loaded) = run_direction(dir, &cfg).await?;
        dir.summary.set(Some(summarize_direction(&sizes, &loaded)));
    }
    Ok(())
}

async fn run_direction(
    dir: Direction,
    cfg: &TestConfig,
) -> Result<(Vec<SizeSamples>, Vec<f64>), JsValue> {
    let stop = Rc::new(Cell::new(false));
    let loaded = Rc::new(RefCell::new(Vec::new()));

    spawn_local({
        let stop = stop.clone();
        let loaded = loaded.clone();
        async move {
            while !stop.get() {
                if let Ok(ms) = engine::ping().await {
                    loaded.borrow_mut().push(ms);
                }
                TimeoutFuture::new(LOADED_PING_INTERVAL_MS as u32).await;
            }
        }
    });

    let result = transfers(dir, cfg).await;
    stop.set(true);
    let loaded_ms = loaded.borrow().clone();
    Ok((result?, loaded_ms))
}

async fn transfers(dir: Direction, cfg: &TestConfig) -> Result<Vec<SizeSamples>, JsValue> {
    let phase_start = engine::now_ms();
    let events: Rc<RefCell<Vec<(f64, u64)>>> = Rc::new(RefCell::new(Vec::new()));
    let sizes = if dir.upload {
        &cfg.upload_sizes
    } else {
        &cfg.download_sizes
    };
    let mut out = Vec::new();
    for &bytes in sizes {
        let mut s = SizeSamples {
            bytes,
            mbps: Vec::new(),
            skipped: false,
        };
        for _ in 0..cfg.iterations {
            if (engine::now_ms() - phase_start) / 1e3 > cfg.time_budget_secs {
                s.skipped = true;
                break;
            }
            let progress = recorder(dir, phase_start, events.clone());
            let mbps = if dir.upload {
                engine::upload(bytes, progress).await?
            } else {
                engine::download(bytes, progress).await?
            };
            s.mbps.push(mbps);
            let mut live = out.clone();
            live.push(s.clone());
            dir.sizes.set(live);
        }
        out.push(s);
    }
    dir.points
        .set(throughput_points(&events.borrow(), WINDOW_MS, EMIT_MS));
    dir.sizes.set(out.clone());
    Ok(out)
}

// per transfer closure turning cumulative bytes into phase timeline deltas
fn recorder(
    dir: Direction,
    phase_start: f64,
    events: Rc<RefCell<Vec<(f64, u64)>>>,
) -> impl FnMut(f64, u64) + 'static {
    let mut last_bytes = 0u64;
    let mut last_set = 0.0f64;
    move |now, cumulative| {
        let t = now - phase_start;
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
