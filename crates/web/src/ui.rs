use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::types::{
    DirectionSummary, LOADED_PING_INTERVAL_MS, LatencySummary, MetaResponse, SizeSamples,
    TestConfig, size_label, summarize_direction, summarize_latency,
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
            Self::Latency => "measuring latency...",
            Self::Download => "measuring download...",
            Self::Upload => "measuring upload...",
            Self::Done => "done",
        }
    }

    fn running(self) -> bool {
        !matches!(self, Self::Idle | Self::Done)
    }
}

#[derive(Clone, Copy)]
struct State {
    phase: RwSignal<Phase>,
    error: RwSignal<Option<String>>,
    latency: RwSignal<Option<LatencySummary>>,
    download: RwSignal<Option<DirectionSummary>>,
    upload: RwSignal<Option<DirectionSummary>>,
    live: RwSignal<Option<(bool, f64)>>,
}

#[component]
pub fn App() -> impl IntoView {
    let meta = RwSignal::new(None::<MetaResponse>);
    let state = State {
        phase: RwSignal::new(Phase::Idle),
        error: RwSignal::new(None),
        latency: RwSignal::new(None),
        download: RwSignal::new(None),
        upload: RwSignal::new(None),
        live: RwSignal::new(None),
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
        state.download.set(None);
        state.upload.set(None);
        state.live.set(None);
        spawn_local(async move {
            if let Err(e) = run_test(state).await {
                state.error.set(Some(format!("{e:?}")));
            }
            state.phase.set(Phase::Done);
        });
    };

    view! {
        <main>
            <header>
                <h1>howfastly</h1>
                {move || meta.get().map(|m| view! {
                    <p class="meta">
                        {format!("{} | as{} {} | {}, {} -> pop {}",
                            m.client_ip, m.asn, m.as_org, m.city, m.country, m.pop)}
                    </p>
                })}
            </header>

            <section class="headline">
                <Headline label="download" upload=false state=state/>
                <Headline label="upload" upload=true state=state/>
            </section>

            <section class="cards">
                {move || state.latency.get().map(|l| view! {
                    <LatencyCard label="latency (unloaded)" summary=l/>
                })}
                {move || state.download.get().and_then(|d| d.loaded_latency).map(|l| view! {
                    <LatencyCard label="latency (down loaded)" summary=l/>
                })}
                {move || state.upload.get().and_then(|d| d.loaded_latency).map(|l| view! {
                    <LatencyCard label="latency (up loaded)" summary=l/>
                })}
            </section>

            {move || state.download.get().map(|d| view! { <SizeTable title="download" dir=d/> })}
            {move || state.upload.get().map(|d| view! { <SizeTable title="upload" dir=d/> })}

            {move || state.error.get().map(|e| view! { <div class="error">{e}</div> })}
            <p class="phase">{move || state.phase.get().label()}</p>

            <button on:click=start disabled=move || state.phase.get().running()>
                {move || if state.phase.get() == Phase::Idle { "start" } else { "run again" }}
            </button>
        </main>
    }
}

#[component]
fn Headline(label: &'static str, upload: bool, state: State) -> impl IntoView {
    let value = move || {
        let done = if upload { state.upload } else { state.download };
        if let Some(p90) = done.get().and_then(|d| d.p90_mbps) {
            return format!("{p90:.1}");
        }
        match state.live.get() {
            Some((up, mbps)) if up == upload => format!("{mbps:.1}"),
            _ => "-".to_string(),
        }
    };
    view! {
        <div class="big">
            <div class="value">{value}</div>
            <div class="label">{label} " mbps"</div>
        </div>
    }
}

#[component]
fn LatencyCard(label: &'static str, summary: LatencySummary) -> impl IntoView {
    view! {
        <div class="card">
            <div class="label">{label}</div>
            <div>{format!("med {:.1} ms | jitter {:.1} ms", summary.median_ms, summary.jitter_ms)}</div>
            <div class="label">{format!("min {:.1} / avg {:.1}", summary.min_ms, summary.avg_ms)}</div>
        </div>
    }
}

#[component]
fn SizeTable(title: &'static str, dir: DirectionSummary) -> impl IntoView {
    let max = dir
        .sizes
        .iter()
        .filter_map(|s| s.median_mbps)
        .fold(f64::EPSILON, f64::max);
    view! {
        <table>
            <thead>
                <tr>
                    <th>{title}</th>
                    <th>"mbps (median)"</th>
                    <th></th>
                </tr>
            </thead>
            <tbody>
                {dir.sizes.into_iter().map(|s| {
                    let label = size_label(s.bytes);
                    let text = match (s.median_mbps, s.skipped) {
                        (Some(m), _) => format!("{m:.1}"),
                        (None, true) => "skipped".to_string(),
                        (None, false) => "-".to_string(),
                    };
                    let width = s.median_mbps.map(|m| m / max * 100.0).unwrap_or(0.0);
                    view! {
                        <tr>
                            <td>{label}</td>
                            <td>{text}</td>
                            <td>
                                <svg class="bar" viewBox="0 0 100 8" preserveAspectRatio="none">
                                    <rect width=format!("{width:.1}") height="8"/>
                                </svg>
                            </td>
                        </tr>
                    }
                }).collect_view()}
            </tbody>
        </table>
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

    state.phase.set(Phase::Download);
    let (sizes, loaded) = run_direction(false, &cfg, state).await?;
    state
        .download
        .set(Some(summarize_direction(&sizes, &loaded)));

    state.phase.set(Phase::Upload);
    let (sizes, loaded) = run_direction(true, &cfg, state).await?;
    state.upload.set(Some(summarize_direction(&sizes, &loaded)));
    Ok(())
}

async fn run_direction(
    upload: bool,
    cfg: &TestConfig,
    state: State,
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

    let result = transfers(upload, cfg, state).await;
    stop.set(true);
    let loaded_ms = loaded.borrow().clone();
    Ok((result?, loaded_ms))
}

async fn transfers(
    upload: bool,
    cfg: &TestConfig,
    state: State,
) -> Result<Vec<SizeSamples>, JsValue> {
    let phase_start = engine::now_ms();
    let sizes = if upload {
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
            let mbps = if upload {
                engine::upload(bytes).await?
            } else {
                engine::download(bytes).await?
            };
            state.live.set(Some((upload, mbps)));
            s.mbps.push(mbps);
        }
        out.push(s);
    }
    Ok(out)
}
