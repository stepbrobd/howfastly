use howfastly::share::{Client, Report, SharedDirection, iso_utc};
use howfastly::stats;
use howfastly::types::{
    Direction, LatencySummary, MetaResponse, SizePlan, SizeSummary, Stage, planned_bytes,
    size_label,
};
use howfastly_map::chart::{chart_y, format_speed, peak, svg_path};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::engine;
use crate::map::Map;
use crate::run::{self, Lane, Phase, State};
use crate::share::{self, Clip, Problem, Share};
use crate::tips;

// the ci pushes every build here, the footer points at the served one
const CACHE: &str = "https://cache.ysun.co";

#[component]
pub fn App() -> impl IntoView {
    let state = State {
        phase: RwSignal::new(Phase::Idle),
        abort: StoredValue::new_local(None),
        error: RwSignal::new(None),
        notice: RwSignal::new(None),
        meta: RwSignal::new(None),
        latency: RwSignal::new(None),
        down: Lane::new(Direction::Download),
        up: Lane::new(Direction::Upload),
        stage: StoredValue::new(Stage::Latency),
        snapshot: RwSignal::new_local(None),
        share: RwSignal::new(Share::Ready),
        reported: StoredValue::new(false),
    };
    window_event_listener(leptos::ev::pagehide, move |_| run::leave(state));

    spawn_local(async move {
        match engine::meta().await {
            Ok(m) => {
                state
                    .notice
                    .set(m.mismatch().map(|w| format!("{w}, reload to update.")));
                state.meta.set(Some(m));
            }
            Err(e) => state.notice.set(Some(engine::describe(e))),
        }
    });

    // the plan total to the nearest ten megabytes
    let total = format!("~{} MB", (planned_bytes() as f64 / 1e7).round() * 10.0);
    // first visit gates behind the popup, later visits start right away
    let gate = RwSignal::new(!engine::autostart_saved());
    if !gate.get_untracked() {
        run::launch(state);
    }
    let begin = move |_| {
        engine::save_autostart();
        gate.set(false);
        run::launch(state);
    };

    view! {
        <main class="mx-auto flex min-h-screen w-full max-w-[65ch] flex-col gap-8 p-4 lg:max-w-6xl">
            <section class="rounded bg-nord-1 p-4">
                <div class="text-center font-mono [overflow-wrap:anywhere]" title=tips::ROUTE>
                    {move || match state.meta.get() {
                        Some(m) => hops(&m),
                        None => view! { "-" }.into_any(),
                    }}
                </div>
            </section>

            <section class="rounded bg-nord-1 p-4">
                <Map meta=state.meta.into() active=Signal::derive(move || !gate.get())>
                    <Controls state=state/>
                </Map>
            </section>

            // the link of the completed run, or why there is none
            {move || match state.share.get() {
                Share::Published { url, expires_at, clip } => {
                    let href = url.clone();
                    let note = match clip {
                        Clip::Pending => "",
                        Clip::Copied => "Copied to the clipboard.",
                        Clip::Failed => "Copy failed, select the link or press Share again.",
                    };
                    let until = format!("Valid until {}.", iso_utc(expires_at));
                    Some(view! {
                        <div class="rounded border border-nord-8 bg-nord-1 p-4">
                            <div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
                                <a class="font-mono [overflow-wrap:anywhere]" href=href>{url}</a>
                                <small class="text-nord-4">{note}</small>
                            </div>
                            <div class="text-nord-4"><small>{until}</small></div>
                        </div>
                    }
                        .into_any())
                }
                Share::Failed(e) => Some(view! {
                    <div class="rounded border border-nord-11 bg-nord-1 p-4 text-nord-11">
                        {format!("Sharing failed. {e}")}
                    </div>
                }
                    .into_any()),
                Share::Ready | Share::Publishing => None,
            }}

            {move || state.notice.get().map(|n| view! {
                <div class="rounded border border-nord-13 bg-nord-1 p-4 text-nord-13">{n}</div>
            })}

            <div class="grid gap-8 lg:grid-cols-2">
                <section class="flex flex-col gap-4">
                    <Headline lane=state.down/>
                    <SizeTable lane=state.down plans=Direction::Download.plan().to_vec()/>
                </section>
                <section class="flex flex-col gap-4">
                    <Headline lane=state.up/>
                    <SizeTable lane=state.up plans=Direction::Upload.plan().to_vec()/>
                </section>
            </div>

            <section class="rounded bg-nord-1 p-4">
                <h2 class="font-semibold">Latency</h2>
                <div class="mt-2 grid gap-4 sm:grid-cols-3">
                    <LatencyCard label="Unloaded" tip=tips::UNLOADED summary=state.latency.into()/>
                    <LatencyCard
                        label="Download loaded"
                        tip=tips::LOADED
                        summary=Signal::derive(move || {
                            state.down.summary.get().and_then(|d| d.loaded)
                        })
                    />
                    <LatencyCard
                        label="Upload loaded"
                        tip=tips::LOADED
                        summary=Signal::derive(move || {
                            state.up.summary.get().and_then(|d| d.loaded)
                        })
                    />
                </div>
            </section>

            {move || state.error.get().map(|e| view! {
                <div class="rounded border border-nord-11 bg-nord-1 p-4 text-nord-11">{e}</div>
            })}

            // the build that served the page, checkable against the cache
            {move || state.meta.get().and_then(|m| m.store).map(|path| {
                let href = narinfo(&path);
                view! {
                    <pre class="overflow-x-auto rounded bg-nord-1 p-4 text-center text-sm text-nord-13"><code>
                        "nix path-info "
                        <a href=href target="_blank" rel="noopener">{path}</a>
                        " --store "
                        <a href=CACHE target="_blank" rel="noopener">{CACHE}</a>
                        " --json-format 2 --json"
                    </code></pre>
                }
            })}

            <Footer/>

            {move || gate.get().then(|| view! {
                <div class="fixed inset-0 z-10 flex items-center justify-center bg-nord-0/80 p-4">
                    <div class="w-full max-w-md rounded bg-nord-1 p-6">
                        <h2 class="text-lg font-semibold">HowFastly</h2>
                        <p class="mt-2">
                            {format!("Tests run automatically and transfer up to {total} in total. ")}
                            "Pause or cancel at any time, retest once it is done."
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

// the read-only page of a published result, nothing here measures or reports
#[component]
pub fn Shared(id: String) -> impl IntoView {
    let loaded = RwSignal::new(None::<Result<Report, Problem>>);
    spawn_local(async move { loaded.set(Some(share::load(id).await)) });
    view! {
        <main class="mx-auto flex min-h-screen w-full max-w-[65ch] flex-col gap-8 p-4 lg:max-w-6xl">
            {move || match loaded.get() {
                None => view! {
                    <section class="rounded bg-nord-1 p-4 text-nord-4">"Loading the shared result..."</section>
                }
                    .into_any(),
                Some(Ok(report)) => view! { <Viewer report=report/> }.into_any(),
                Some(Err(problem)) => view! { <Failure problem=problem/> }.into_any(),
            }}
            <Footer/>
        </main>
    }
}

// a lane filled from a stored direction, the presentation reads it like a finished run
fn stored(dir: Direction, shared: Option<&SharedDirection>) -> Lane {
    let lane = Lane::new(dir);
    if let Some(d) = shared {
        lane.summary.set(Some(d.summary.clone()));
        if let Some(s) = &d.samples {
            lane.sizes.set(s.clone());
        }
        if let Some(t) = &d.timeline {
            lane.points.set(t.points());
        }
    }
    lane
}

// what the chart says in place of a timeline
fn absent(shared: Option<&SharedDirection>) -> &'static str {
    match shared {
        None => "Not measured",
        Some(_) => "No timeline in this share",
    }
}

#[component]
fn Viewer(report: Report) -> impl IntoView {
    let payload = report.payload;
    let publication = report.publication;
    let (published_at, expires_at) = (report.published_at, report.expires_at);
    let meta = publication.to_meta();
    let bar = hops(&meta);
    let down = stored(Direction::Download, payload.download.as_ref());
    let up = stored(Direction::Upload, payload.upload.as_ref());
    let down_empty = absent(payload.download.as_ref());
    let up_empty = absent(payload.upload.as_ref());
    let down_plans = payload.config.plans(Direction::Download).to_vec();
    let up_plans = payload.config.plans(Direction::Upload).to_vec();
    let latency: Signal<Option<LatencySummary>> = RwSignal::new(payload.latency.clone()).into();

    let client = match payload.client {
        Client::Web => "in the browser",
        Client::Cli => "from the command line",
    };
    let measured = format!(
        "{} with HowFastly {} {client}",
        iso_utc(payload.finished_at),
        payload.build
    );
    // the publication context, what fastly saw of the request that published
    let network = {
        let mut parts = Vec::new();
        if publication.asn != 0 {
            parts.push(format!("AS{}", publication.asn));
        }
        for part in [&publication.org, &publication.city, &publication.country] {
            if !part.is_empty() {
                parts.push(part.clone());
            }
        }
        if parts.is_empty() {
            "an unknown network".to_string()
        } else {
            parts.join(", ")
        }
    };
    let service = if publication.version.is_empty() {
        String::new()
    } else {
        format!(" (service version {})", publication.version)
    };
    let published = format!(
        "{} from {network} by HowFastly {}{service}",
        iso_utc(published_at),
        publication.cargo
    );
    let expires = iso_utc(expires_at);
    let store = publication.store.clone();
    let meta: Signal<Option<MetaResponse>> = RwSignal::new(Some(meta)).into();

    view! {
        <section class="rounded bg-nord-1 p-4">
            <div class="text-center font-mono [overflow-wrap:anywhere]" title=tips::PUBLICATION>
                {bar}
            </div>
        </section>

        <section class="rounded bg-nord-1 p-4">
            <Map meta=meta active=RwSignal::new(true).into()>
                <a
                    class="absolute bottom-1 left-1 rounded bg-nord-10 px-3 py-1 text-sm text-nord-6 hover:bg-nord-9"
                    href="/"
                >
                    "Run your own test"
                </a>
            </Map>
        </section>

        <section class="rounded bg-nord-1 p-4">
            <h2 class="font-semibold">Shared result</h2>
            <dl class="mt-2 grid gap-x-4 gap-y-1 sm:grid-cols-[max-content_1fr]">
                <dt class="text-nord-4">Measured</dt>
                <dd class="text-nord-6">{measured}</dd>
                <dt class="text-nord-4">Published</dt>
                <dd class="text-nord-6">{published}</dd>
                <dt class="text-nord-4">Expires</dt>
                <dd class="text-nord-6">{expires}</dd>
                {store.map(|path| {
                    let href = narinfo(&path);
                    view! {
                        <dt class="text-nord-4">Build</dt>
                        <dd class="text-nord-6 [overflow-wrap:anywhere]">
                            <a href=href target="_blank" rel="noopener">{path}</a>
                        </dd>
                    }
                })}
            </dl>
            <p class="mt-2 text-nord-4"><small>
                "The network and datacenter are those of the connection that published this result, "
                "which can differ from the one measured. Measurements come from the client and are not verified."
            </small></p>
        </section>

        <div class="grid gap-8 lg:grid-cols-2">
            <section class="flex flex-col gap-4">
                <Headline lane=down empty=down_empty/>
                <SizeTable lane=down plans=down_plans/>
            </section>
            <section class="flex flex-col gap-4">
                <Headline lane=up empty=up_empty/>
                <SizeTable lane=up plans=up_plans/>
            </section>
        </div>

        <section class="rounded bg-nord-1 p-4">
            <h2 class="font-semibold">Latency</h2>
            <div class="mt-2 grid gap-4 sm:grid-cols-3">
                <LatencyCard label="Unloaded" tip=tips::UNLOADED summary=latency/>
                <LatencyCard
                    label="Download loaded"
                    tip=tips::LOADED
                    summary=Signal::derive(move || down.summary.get().and_then(|d| d.loaded))
                />
                <LatencyCard
                    label="Upload loaded"
                    tip=tips::LOADED
                    summary=Signal::derive(move || up.summary.get().and_then(|d| d.loaded))
                />
            </div>
        </section>
    }
}

// each way a shared result is out of reach gets its own words and a way to run a test
#[component]
fn Failure(problem: Problem) -> impl IntoView {
    let (title, text) = match problem {
        Problem::Invalid(e) => ("Not a shared result", e),
        Problem::Missing(e) => ("No such shared result", e),
        Problem::Unsupported(e) => ("Unsupported shared result", e),
        Problem::Unavailable(e) => ("Shared results unavailable", e),
    };
    view! {
        <section class="rounded bg-nord-1 p-4">
            <h2 class="text-lg font-semibold">{title}</h2>
            <p class="mt-2">{text}</p>
            <p class="mt-4"><a href="/">"Run your own test"</a></p>
        </section>
    }
}

// the network and the datacenter of a meta, each part shown when the server gave it
// erased so the view owns its strings and outlives the borrow
fn hops(m: &MetaResponse) -> AnyView {
    let ip = (!m.ip.is_empty()).then(|| m.ip.clone());
    let asn = (m.asn != 0).then_some(m.asn);
    let paired = ip.is_some() && asn.is_some();
    let unknown = ip.is_none() && asn.is_none();
    let place = [m.city.as_str(), m.country.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let at = (!place.is_empty()).then(|| format!(" @ {place}"));
    let code = m.pop.code.clone();
    let name = (!m.pop.name.is_empty()).then(|| {
        if m.pop.group.is_empty() {
            format!(" ({})", m.pop.name)
        } else {
            format!(" ({}, {})", m.pop.name, m.pop.group)
        }
    });
    let via = (!m.protocol.is_empty()).then(|| format!(" via {}", m.protocol));
    view! {
        {ip.map(|ip| view! {
            <a href=format!("https://bgp.tools/prefix/{ip}") target="_blank" rel="noopener">{ip.clone()}</a>
        })}
        {paired.then_some(" (")}
        {asn.map(|asn| view! {
            <a href=format!("https://bgp.tools/as/{asn}") target="_blank" rel="noopener">{format!("AS{asn}")}</a>
        })}
        {paired.then_some(")")}
        {unknown.then_some("-")}
        {at}
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
        {code}
        {name}
        {via}
    }
    .into_any()
}

// the narinfo sits under the hash that opens the store name
fn narinfo(path: &str) -> String {
    let hash = path
        .rsplit('/')
        .next()
        .and_then(|name| name.split('-').next())
        .unwrap_or_default();
    format!("{CACHE}/{hash}.narinfo")
}

#[component]
fn Footer() -> impl IntoView {
    view! {
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
            <p class="mt-8"><small>
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
    }
}

const CHART_W: f64 = 300.0;
const CHART_H: f64 = 80.0;
const WAITING: &str = "Waiting for measurements...";

#[component]
fn Waiting(class: &'static str, text: &'static str) -> impl IntoView {
    view! {
        <div class=format!("flex items-center justify-center rounded text-nord-3 {class}")>
            <small>{text}</small>
        </div>
    }
}

// stroke and fill classes of a direction
fn palette(dir: Direction) -> (&'static str, &'static str) {
    match dir {
        Direction::Download => ("stroke-nord-8", "fill-nord-8"),
        Direction::Upload => ("stroke-nord-12", "fill-nord-12"),
    }
}

#[component]
fn SpeedChart(lane: Lane, empty: &'static str) -> impl IntoView {
    let (stroke, fill) = palette(lane.dir);
    view! {
        <div class="relative mt-2 h-24 w-full">
            {move || {
                let pts = lane.points.get();
                let p90 = lane.summary.get().and_then(|d| d.p90).map(|mbps| mbps * 1e6);
                // the scale grows to keep the reference line inside the frame
                let max = peak(&pts).max(p90.unwrap_or(0.0));
                let line = svg_path(&pts, CHART_W, CHART_H, max);
                if line.is_empty() {
                    return view! { <Waiting class="h-full bg-nord-0" text=empty/> }.into_any();
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
                                class=format!("absolute left-1 text-nord-4 {side}")
                                style=format!("top:{:.1}%", y / CHART_H * 100.0)
                                title=tips::HEADLINE
                            >
                                "90th percentile"
                            </small>
                        }
                    })}
                }
                    .into_any()
            }}
        </div>
    }
}

// empty is what the chart says while it has no line, the live run waits, a share explains
#[component]
fn Headline(lane: Lane, #[prop(default = WAITING)] empty: &'static str) -> impl IntoView {
    let label = lane.dir.name();
    // live estimate while this direction transfers, p90 once summarized
    let speed = move || {
        let live = lane.points.get().last().map(|&(_, bps)| bps);
        let bps = if lane.running.get() {
            live
        } else {
            lane.summary
                .get()
                .and_then(|d| d.p90)
                .map(|p90| p90 * 1e6)
                .or(live)
        };
        bps.map(format_speed)
    };
    view! {
        <div class="flex-1 rounded bg-nord-1 p-4">
            <div class="font-mono text-4xl text-nord-6" title=tips::HEADLINE>
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
                <span class="text-nord-4" title=tips::PEAK>
                    <small>
                        {move || {
                            let pts = lane.points.get();
                            if pts.is_empty() {
                                return String::new();
                            }
                            let (v, unit) = format_speed(peak(&pts));
                            format!("Peak {v:.1} {unit}")
                        }}
                    </small>
                </span>
            </div>
            <SpeedChart lane=lane empty=empty/>
        </div>
    }
}

#[component]
fn LatencyCard(
    label: &'static str,
    tip: &'static str,
    summary: Signal<Option<LatencySummary>>,
) -> impl IntoView {
    view! {
        <div>
            <div title=tip><small>{label}</small></div>
            {move || match summary.get() {
                Some(s) => view! {
                    <div class="text-nord-6">
                        {format!("Median {:.1} ms / ", s.median)}
                        <span title=tips::JITTER>{format!("Jitter {:.1} ms", s.jitter)}</span>
                    </div>
                    <div><small>{format!("Min {:.1} / Avg {:.1}", s.min, s.avg)}</small></div>
                }
                    .into_any(),
                None => view! {
                    <div class="text-nord-3">
                        "Median - / "
                        <span title=tips::JITTER>"Jitter -"</span>
                    </div>
                    <div class="text-nord-3"><small>"Min - / Avg -"</small></div>
                }
                    .into_any(),
            }}
        </div>
    }
}

#[component]
fn BoxPlot(samples: Vec<f64>, max: f64, dir: Direction) -> impl IntoView {
    let (stroke, fill) = palette(dir);
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

// plans are the sizes the run was configured with, a share carries its own
#[component]
fn SizeTable(lane: Lane, plans: Vec<SizePlan>) -> impl IntoView {
    let title = lane.dir.name();
    // render every planned size from the start so the height never changes
    view! {
        {move || {
            let live = lane.sizes.get();
            let summary = lane.summary.get();
            // raw samples when the run kept them, a summary still knows its counts and medians
            let rows: Vec<(SizeSummary, usize, Vec<f64>)> = plans
                .iter()
                .map(|&SizePlan { bytes, iterations }| {
                    let (s, mbps) = match live.iter().find(|s| s.bytes == bytes) {
                        Some(s) => (
                            SizeSummary {
                                bytes,
                                samples: s.mbps.len(),
                                median: stats::median(&s.mbps),
                                skipped: s.skipped,
                            },
                            s.mbps.clone(),
                        ),
                        None => (
                            summary
                                .as_ref()
                                .and_then(|d| d.sizes.iter().find(|s| s.bytes == bytes).cloned())
                                .unwrap_or(SizeSummary {
                                    bytes,
                                    ..Default::default()
                                }),
                            Vec::new(),
                        ),
                    };
                    (s, iterations, mbps)
                })
                .collect();
            let max = rows
                .iter()
                .flat_map(|(_, _, mbps)| mbps.iter().copied())
                .fold(f64::EPSILON, f64::max);
            view! {
                <table class="w-full border-separate border-spacing-0 overflow-hidden rounded border border-nord-3">
                    <thead>
                        <tr>
                            <th class="border-b border-nord-3 bg-nord-0 px-2 py-2 text-left font-semibold text-nord-6 sm:px-4">
                                {title}
                            </th>
                            <th
                                class="border-b border-nord-3 bg-nord-0 px-2 py-2 text-left font-semibold text-nord-6 sm:px-4"
                                title=tips::MEDIAN
                            >
                                "Median"
                            </th>
                            <th class="w-1/3 border-b border-nord-3 bg-nord-0 px-2 py-2 sm:w-1/2 sm:px-4"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {rows
                            .into_iter()
                            .map(|(s, iterations, mbps)| {
                                let label = format!(
                                    "{} ({}/{})",
                                    size_label(s.bytes),
                                    s.samples,
                                    iterations,
                                );
                                let text = match (s.median, s.skipped) {
                                    (Some(m), _) => {
                                        let (v, unit) = format_speed(m * 1e6);
                                        format!("{v:.1} {unit}")
                                    }
                                    (None, true) => "Skipped".to_string(),
                                    (None, false) => "-".to_string(),
                                };
                                view! {
                                    <tr class="odd:bg-nord-1">
                                        <td class="px-2 py-2 whitespace-nowrap sm:px-4" title=tips::count()>{label}</td>
                                        <td class="px-2 py-2 font-mono whitespace-nowrap sm:px-4" title=tips::MEDIAN>{text}</td>
                                        <td class="px-2 py-2 sm:px-4" title=tips::PLOT>
                                            <BoxPlot samples=mbps max=max dir=lane.dir/>
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

#[component]
fn Controls(state: State) -> impl IntoView {
    let primary = "cursor-pointer rounded bg-nord-10 px-3 py-1 text-sm text-nord-6 hover:bg-nord-9";
    let plain = "cursor-pointer rounded bg-nord-1 px-3 py-1 text-sm text-nord-4 hover:bg-nord-2";
    let quiet = "rounded bg-nord-1 px-3 py-1 text-sm text-nord-4";
    view! {
        <div class="absolute bottom-1 left-1 flex gap-1">
            {move || match state.phase.get() {
                Phase::Idle => view! {
                    <button class=primary on:click=move |_| run::launch(state)>"Retest"</button>
                    // only a completed run has something to share, the chip follows the publication
                    {move || state.snapshot.with(|s| s.is_some()).then(|| match state.share.get() {
                        Share::Publishing => view! { <span class=quiet>"Sharing"</span> }.into_any(),
                        Share::Ready | Share::Published { .. } | Share::Failed(_) => view! {
                            <button class=plain on:click=move |_| share::publish(state)>"Share"</button>
                        }
                            .into_any(),
                    })}
                }
                    .into_any(),
                Phase::Running => view! {
                    <button class=plain on:click=move |_| run::pause(state)>"Pause"</button>
                    <button class=plain on:click=move |_| run::cancel(state)>"Cancel"</button>
                }
                    .into_any(),
                Phase::Paused => view! {
                    <button class=primary on:click=move |_| run::resume(state)>"Resume"</button>
                    <button class=plain on:click=move |_| run::cancel(state)>"Cancel"</button>
                }
                    .into_any(),
                Phase::Canceled => view! {
                    <span class=quiet>"Stopping"</span>
                }
                    .into_any(),
            }}
        </div>
    }
}
