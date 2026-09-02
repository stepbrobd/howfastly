use gloo_timers::future::TimeoutFuture;
use howfastly::types::{Coordinates, MetaResponse};
use howfastly_map::map::{self, View};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::engine;

// the frame is twice as wide as tall
const ASPECT: f64 = 2.0;
// fraction of the route extent kept clear on each side
const PAD: f64 = 0.35;
// narrowest viewport in map units so a short route keeps its surroundings
const MIN_W: f64 = 24.0;
const FLY_MS: f64 = 1500.0;
const FRAME_MS: u32 = 16;
const ARC_STEPS: usize = 64;
const MAX_LABELS: usize = 24;

// map points of the visitor and the pop joined by the great circle
// the arc is traced from the visitor so the pop lands on its nearest copy
#[derive(Clone, PartialEq)]
struct Route {
    client: Option<(f64, f64)>,
    pop: Option<(f64, f64)>,
    arc: Vec<(f64, f64)>,
}

impl Route {
    fn points(&self) -> Vec<(f64, f64)> {
        if self.arc.is_empty() {
            self.client.into_iter().chain(self.pop).collect()
        } else {
            self.arc.clone()
        }
    }

    // the viewport the flight ends in
    fn target(&self) -> Option<View> {
        let (lo, hi) = map::bounds(&self.points())?;
        Some(map::fit(lo, hi, ASPECT, PAD, MIN_W))
    }
}

fn route(m: &MetaResponse) -> Route {
    let lonlat = |c: Coordinates| (c.longitude, c.latitude);
    match (m.coordinates.map(lonlat), m.pop.coordinates.map(lonlat)) {
        (Some(c), Some(p)) => {
            let arc = map::trace(&map::arc(c, p, ARC_STEPS));
            Route {
                client: arc.first().copied(),
                pop: arc.last().copied(),
                arc,
            }
        }
        (c, p) => Route {
            client: c.map(|(lon, lat)| map::project(lon, lat)),
            pop: p.map(|(lon, lat)| map::project(lon, lat)),
            arc: Vec::new(),
        },
    }
}

// a town label laid out for the target viewport
#[derive(Clone, PartialEq)]
struct Town {
    name: String,
    at: (f64, f64),
}

// glide the viewport to its target, a newer flight takes over mid-air
async fn fly(view: RwSignal<View>, flight: RwSignal<u32>, id: u32, to: View) {
    let from = view.get_untracked();
    let start = engine::now_ms();
    loop {
        if flight.get_untracked() != id {
            return;
        }
        let t = ((engine::now_ms() - start) / FLY_MS).min(1.0);
        view.set(from.toward(&to, map::ease(t)));
        if t >= 1.0 {
            return;
        }
        TimeoutFuture::new(FRAME_MS).await;
    }
}

#[component]
pub fn Map(meta: Signal<Option<MetaResponse>>) -> impl IntoView {
    let land = map::land(map::LAND).expect("land outline");
    let borders = map::borders(map::BORDERS).expect("borders");
    let places = StoredValue::new(map::places(map::PLACES).expect("places"));
    let view = RwSignal::new(map::world(ASPECT));
    let route = Memo::new(move |_| meta.with(|m| m.as_ref().map(route)));
    let names = Memo::new(move |_| {
        meta.with(|m| {
            m.as_ref().map(|m| {
                let you = if m.city.is_empty() { "You" } else { &m.city };
                (you.to_string(), m.pop.code.clone())
            })
        })
    });
    let flight = RwSignal::new(0u32);

    // towns are laid out once for the viewport the flight ends in
    // the route labels are placed first, towns fill the space left
    let towns = Memo::new(move |_| {
        let Some((r, target)) = route.get().and_then(|r| r.target().map(|t| (r, t))) else {
            return Vec::new();
        };
        let taken: Vec<(f64, f64)> = r
            .client
            .into_iter()
            .chain(r.pop)
            .map(|p| target.frac(p))
            .collect();
        places.with_value(|p| {
            map::labels(p, &target, &taken, map::GAP, MAX_LABELS)
                .into_iter()
                .map(|l| Town {
                    name: l.place.name.clone(),
                    at: l.at,
                })
                .collect::<Vec<Town>>()
        })
    });

    Effect::new(move |_| {
        let Some(to) = route.get().and_then(|r| r.target()) else {
            return;
        };
        let id = flight.get_untracked() + 1;
        flight.set(id);
        spawn_local(fly(view, flight, id, to));
    });

    view! {
        <div class="relative aspect-[2/1] w-full overflow-hidden rounded border border-nord-3 bg-nord-0">
            <svg
                class="h-full w-full"
                viewBox=move || view.get().view_box()
                preserveAspectRatio="xMidYMid slice"
            >
                <defs>
                    <g id="tile">
                        <path
                            d=land
                            class="fill-nord-2 stroke-nord-3"
                            fill-rule="evenodd"
                            stroke-width="1"
                            vector-effect="non-scaling-stroke"
                        />
                        <path
                            d=borders
                            fill="none"
                            class="stroke-nord-3"
                            stroke-width="1"
                            vector-effect="non-scaling-stroke"
                        />
                    </g>
                </defs>
                // copies on both sides so a route over the antimeridian keeps its land
                <use href="#tile" x=format!("{}", -map::WORLD)/>
                <use href="#tile"/>
                <use href="#tile" x=format!("{}", map::WORLD)/>
                <For
                    each=move || towns.get()
                    key=|t| (t.at.0.to_bits(), t.at.1.to_bits())
                    children=move |t| view! {
                        <path
                            d=format!("M{:.1},{:.1}h0", t.at.0, t.at.1)
                            class="stroke-nord-4"
                            stroke-width="4"
                            stroke-linecap="round"
                            vector-effect="non-scaling-stroke"
                        />
                    }
                />
                {move || route.get().map(|r| view! {
                    <path
                        d=map::path(&r.arc)
                        fill="none"
                        class="stroke-nord-4"
                        stroke-opacity="0.7"
                        stroke-dasharray="4 4"
                        vector-effect="non-scaling-stroke"
                    />
                    {r.pop.map(|p| view! { <Dot at=p class="stroke-nord-8"/> })}
                    {r.client.map(|p| view! { <Dot at=p class="stroke-nord-13"/> })}
                })}
            </svg>
            <For
                each=move || towns.get()
                key=|t| (t.at.0.to_bits(), t.at.1.to_bits())
                children=move |t| view! {
                    <Label at=t.at view=view text=t.name class="text-xs text-nord-4"/>
                }
            />
            {move || route.get().zip(names.get()).map(|(r, (you, pop))| view! {
                {r.client.map(|p| view! { <Label at=p view=view text=you class="text-nord-6"/> })}
                {r.pop.map(|p| view! { <Label at=p view=view text=pop class="text-nord-6"/> })}
            })}
            <details class="absolute right-1 bottom-1">
                <summary class="flex h-5 w-5 cursor-pointer list-none items-center justify-center rounded-full bg-nord-1 font-serif text-xs text-nord-4 [&::-webkit-details-marker]:hidden">
                    "i"
                </summary>
                <small class="absolute right-6 bottom-0 rounded bg-nord-1 px-2 py-1 whitespace-nowrap text-nord-4">
                    "Made with "
                    <a href="https://www.naturalearthdata.com" target="_blank" rel="noopener">"Natural Earth"</a>
                </small>
            </details>
        </div>
    }
}

// a zero length stroke with round caps is a dot of fixed screen size
// the dark halo separates it from the land
#[component]
fn Dot(at: (f64, f64), class: &'static str) -> impl IntoView {
    let d = format!("M{:.1},{:.1}h0", at.0, at.1);
    view! {
        <path
            d=d.clone()
            class="stroke-nord-0"
            stroke-width="14"
            stroke-linecap="round"
            vector-effect="non-scaling-stroke"
        />
        <path
            d=d
            class=class
            stroke-width="10"
            stroke-linecap="round"
            vector-effect="non-scaling-stroke"
        />
    }
}

// html text pinned to a map point, it follows the viewport without a rebuild
#[component]
fn Label(at: (f64, f64), view: RwSignal<View>, text: String, class: &'static str) -> impl IntoView {
    view! {
        <small
            class=format!("absolute -translate-y-1/2 translate-x-2 whitespace-nowrap [text-shadow:0_0_4px_var(--color-nord-0)] {class}")
            style=move || {
                let (fx, fy) = view.get().frac(at);
                format!("left:{:.2}%;top:{:.2}%", fx * 100.0, fy * 100.0)
            }
        >
            {text}
        </small>
    }
}
