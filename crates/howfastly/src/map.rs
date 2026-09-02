use std::f64::consts::PI;

// the world square edge in map units
pub const WORLD: f64 = 1000.0;
// web mercator cuts off before the poles
const MAX_LAT: f64 = 85.051_129;
// natural earth land rings, country borders and populated places
// regenerate with assets/gen.nu
pub const LAND: &str = include_str!("../assets/land.txt");
pub const BORDERS: &str = include_str!("../assets/borders.txt");
pub const PLACES: &str = include_str!("../assets/places.txt");
// labels this close in viewport fractions would overlap
pub const GAP: (f64, f64) = (0.12, 0.05);

// a viewport in map units
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl View {
    pub fn view_box(&self) -> String {
        format!("{:.2} {:.2} {:.2} {:.2}", self.x, self.y, self.w, self.h)
    }

    // where a map point sits as fractions of the viewport
    pub fn frac(&self, (px, py): (f64, f64)) -> (f64, f64) {
        ((px - self.x) / self.w, (py - self.y) / self.h)
    }

    // t of 0 is self and 1 is to
    // the size moves geometrically so a deep zoom feels even
    pub fn toward(&self, to: &View, t: f64) -> View {
        let t = t.clamp(0.0, 1.0);
        let cx = self.x + self.w / 2.0 + (to.x + to.w / 2.0 - self.x - self.w / 2.0) * t;
        let cy = self.y + self.h / 2.0 + (to.y + to.h / 2.0 - self.y - self.h / 2.0) * t;
        let w = self.w * (to.w / self.w).powf(t);
        let h = self.h * (to.h / self.h).powf(t);
        View {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        }
    }
}

// web mercator onto the world square, x grows east and y grows south
pub fn project(lon: f64, lat: f64) -> (f64, f64) {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.0) / 360.0 * WORLD;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * WORLD;
    // rounding at the cutoff would otherwise leak a hair past the edge
    (x, y.clamp(0.0, WORLD))
}

fn unit(lon: f64, lat: f64) -> (f64, f64, f64) {
    let (lon, lat) = (lon.to_radians(), lat.to_radians());
    (lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin())
}

// samples along the great circle from a to b in degrees, both ends included
// antipodes have no single shortest path and fall back to a straight run
pub fn arc(a: (f64, f64), b: (f64, f64), steps: usize) -> Vec<(f64, f64)> {
    let (ua, ub) = (unit(a.0, a.1), unit(b.0, b.1));
    let omega = (ua.0 * ub.0 + ua.1 * ub.1 + ua.2 * ub.2)
        .clamp(-1.0, 1.0)
        .acos();
    let n = steps.max(1);
    let inner = (1..n).map(|i| {
        let t = i as f64 / n as f64;
        if omega.sin() < 1e-9 {
            return (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        }
        let wa = ((1.0 - t) * omega).sin() / omega.sin();
        let wb = (t * omega).sin() / omega.sin();
        let (x, y, z) = (
            wa * ua.0 + wb * ub.0,
            wa * ua.1 + wb * ub.1,
            wa * ua.2 + wb * ub.2,
        );
        (y.atan2(x).to_degrees(), z.atan2(x.hypot(y)).to_degrees())
    });
    std::iter::once(a)
        .chain(inner)
        .chain(std::iter::once(b))
        .collect()
}

// project a polyline keeping x continuous across the antimeridian
pub fn trace(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for &(lon, lat) in points {
        let (x, y) = project(lon, lat);
        let x = out.last().map_or(x, |&(px, _)| nearest(px, x));
        out.push((x, y));
    }
    out
}

pub fn path(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| format!("{}{x:.1},{y:.1}", if i == 0 { 'M' } else { 'L' }))
        .collect()
}

// the copy of x within half a world of anchor
// a route then crosses the antimeridian the short way
pub fn nearest(anchor: f64, x: f64) -> f64 {
    x - ((x - anchor) / WORLD).round() * WORLD
}

pub fn bounds(points: &[(f64, f64)]) -> Option<((f64, f64), (f64, f64))> {
    let (first, rest) = points.split_first()?;
    Some(
        rest.iter()
            .fold((*first, *first), |((x0, y0), (x1, y1)), &(x, y)| {
                ((x0.min(x), y0.min(y)), (x1.max(x), y1.max(y)))
            }),
    )
}

// the smallest viewport of the given aspect around a box
// pad is a fraction of the extent added on each side
// the width never drops under min_w so nearby points keep their surroundings
// a lone point with no min_w still gets a positive width, frac and toward divide by it
pub fn fit((x0, y0): (f64, f64), (x1, y1): (f64, f64), aspect: f64, pad: f64, min_w: f64) -> View {
    let w = ((x1 - x0) * (1.0 + 2.0 * pad))
        .max((y1 - y0) * (1.0 + 2.0 * pad) * aspect)
        .max(min_w)
        .max(f64::EPSILON);
    let h = w / aspect;
    View {
        x: (x0 + x1) / 2.0 - w / 2.0,
        y: (y0 + y1) / 2.0 - h / 2.0,
        w,
        h,
    }
}

// the inhabited world, the poles cut where the mercator stretch says nothing
pub fn world(aspect: f64) -> View {
    fit(
        project(-180.0, 78.0),
        project(180.0, -58.0),
        aspect,
        0.0,
        0.0,
    )
}

// smooth in and out
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub at: (f64, f64),
    // the natural earth min_zoom, smaller is more prominent
    pub zoom: f64,
}

// zoom, lon, lat and name per line, tab separated, kept in file order
// None on any line that does not parse
pub fn places(text: &str) -> Option<Vec<Place>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let mut f = line.splitn(4, '\t');
            let zoom = f.next()?.parse().ok()?;
            let lon = f.next()?.parse().ok()?;
            let lat = f.next()?.parse().ok()?;
            Some(Place {
                name: f.next()?.to_string(),
                at: project(lon, lat),
                zoom,
            })
        })
        .collect()
}

// the web mercator zoom a viewport width amounts to on a screen about a thousand pixels wide
fn zoom(w: f64) -> f64 {
    (WORLD / w).log2() + 2.0
}

// a place placed in a viewport, at is the map point of the copy in view
#[derive(Clone, Debug, PartialEq)]
pub struct Label<'a> {
    pub place: &'a Place,
    pub at: (f64, f64),
    pub frac: (f64, f64),
}

// places worth a label in the viewport, in order of prominence
// none within gap of another, of a taken spot, or of the frame edge where text would clip
// a place is tried on its copy nearest the viewport so wrapped views keep their labels
pub fn labels<'a>(
    places: &'a [Place],
    view: &View,
    taken: &[(f64, f64)],
    gap: (f64, f64),
    limit: usize,
) -> Vec<Label<'a>> {
    let zoom = zoom(view.w);
    let mut out: Vec<Label> = Vec::new();
    let mut spots: Vec<(f64, f64)> = taken.to_vec();
    let crowded = |spots: &[(f64, f64)], (fx, fy): (f64, f64)| {
        spots
            .iter()
            .any(|&(sx, sy)| (sx - fx).abs() < gap.0 && (sy - fy).abs() < gap.1)
    };
    let inside = |(fx, fy): (f64, f64)| {
        (0.0..1.0 - gap.0).contains(&fx) && (gap.1..1.0 - gap.1).contains(&fy)
    };
    for place in places.iter().filter(|p| p.zoom <= zoom) {
        if out.len() >= limit {
            break;
        }
        let at = (nearest(view.x + view.w / 2.0, place.at.0), place.at.1);
        let frac = view.frac(at);
        if !inside(frac) || crowded(&spots, frac) {
            continue;
        }
        spots.push(frac);
        out.push(Label { place, at, frac });
    }
    out
}

// one polyline per line as lon,lat pairs, projected
// None on any pair that does not parse
fn polylines(text: &str) -> Option<Vec<Vec<(f64, f64)>>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            line.split_whitespace()
                .map(|pair| {
                    let (lon, lat) = pair.split_once(',')?;
                    Some(project(lon.parse().ok()?, lat.parse().ok()?))
                })
                .collect()
        })
        .collect()
}

// land rings close into filled shapes
pub fn land(text: &str) -> Option<String> {
    Some(
        polylines(text)?
            .iter()
            .map(|ring| path(ring) + "Z")
            .collect(),
    )
}

// border segments stay open strokes
pub fn borders(text: &str) -> Option<String> {
    Some(polylines(text)?.iter().map(|seg| path(seg)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn project_within_world(lon in -180.0f64..=180.0, lat in -90.0f64..=90.0) {
            let (x, y) = project(lon, lat);
            prop_assert!((0.0..=WORLD).contains(&x));
            prop_assert!((0.0..=WORLD).contains(&y));
        }

        #[test]
        fn project_monotone(a in -180.0f64..=180.0, b in -180.0f64..=180.0, c in -85.0f64..=85.0, d in -85.0f64..=85.0) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(project(lo, 0.0).0 <= project(hi, 0.0).0);
            let (south, north) = if c <= d { (c, d) } else { (d, c) };
            prop_assert!(project(0.0, south).1 >= project(0.0, north).1);
        }

        #[test]
        fn nearest_within_half_world(anchor in 0.0f64..WORLD, x in 0.0f64..WORLD) {
            let n = nearest(anchor, x);
            prop_assert!((n - anchor).abs() <= WORLD / 2.0 + 1e-9);
            let turns = ((n - x) / WORLD).round();
            prop_assert!(turns.abs() <= 1.0);
            prop_assert!((n - x - turns * WORLD).abs() < 1e-6);
        }

        #[test]
        fn fit_holds_every_point(
            pts in prop::collection::vec((0.0f64..WORLD, 0.0f64..WORLD), 1..20),
            aspect in 0.5f64..4.0,
            pad in 0.0f64..1.0,
            min_w in 0.0f64..WORLD,
        ) {
            let (lo, hi) = bounds(&pts).unwrap();
            let v = fit(lo, hi, aspect, pad, min_w);
            for &p in &pts {
                prop_assert!(holds(&v, p));
            }
            prop_assert!((v.w / v.h - aspect).abs() < 1e-9);
            prop_assert!(v.w >= min_w);
        }

        #[test]
        fn toward_endpoints(
            a in (0.0f64..WORLD, 0.0f64..WORLD, 1.0f64..WORLD, 1.0f64..WORLD),
            b in (0.0f64..WORLD, 0.0f64..WORLD, 1.0f64..WORLD, 1.0f64..WORLD),
            t in 0.0f64..=1.0,
        ) {
            let from = View { x: a.0, y: a.1, w: a.2, h: a.3 };
            let to = View { x: b.0, y: b.1, w: b.2, h: b.3 };
            prop_assert!(close(&from.toward(&to, 0.0), &from));
            prop_assert!(close(&from.toward(&to, 1.0), &to));
            let mid = from.toward(&to, t);
            prop_assert!(mid.w >= from.w.min(to.w) - 1e-9 && mid.w <= from.w.max(to.w) + 1e-9);
            prop_assert!(ease(t) >= 0.0 && ease(t) <= 1.0);
        }
    }

    fn close(a: &View, b: &View) -> bool {
        [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
            .iter()
            .all(|d| d.abs() < 1e-6)
    }

    fn holds(v: &View, p: (f64, f64)) -> bool {
        let (fx, fy) = v.frac(p);
        (0.0..=1.0).contains(&fx) && (0.0..=1.0).contains(&fy)
    }

    fn angle(a: (f64, f64), b: (f64, f64)) -> f64 {
        let (ua, ub) = (unit(a.0, a.1), unit(b.0, b.1));
        (ua.0 * ub.0 + ua.1 * ub.1 + ua.2 * ub.2)
            .clamp(-1.0, 1.0)
            .acos()
    }

    proptest! {
        #[test]
        fn arc_samples_split_the_angle_evenly_in_one_plane(
            a in (-180.0f64..180.0, -89.0f64..89.0),
            b in (-180.0f64..180.0, -89.0f64..89.0),
            steps in 1usize..40,
        ) {
            prop_assume!(angle(a, b) < 3.0);
            let pts = arc(a, b, steps);
            prop_assert_eq!(pts.len(), steps + 1);
            prop_assert_eq!(pts[0], a);
            prop_assert_eq!(pts[steps], b);
            let (ua, ub) = (unit(a.0, a.1), unit(b.0, b.1));
            let normal = (
                ua.1 * ub.2 - ua.2 * ub.1,
                ua.2 * ub.0 - ua.0 * ub.2,
                ua.0 * ub.1 - ua.1 * ub.0,
            );
            for (i, &p) in pts.iter().enumerate() {
                let want = i as f64 / steps as f64 * angle(a, b);
                prop_assert!((angle(a, p) - want).abs() < 1e-6);
                let u = unit(p.0, p.1);
                prop_assert!((normal.0 * u.0 + normal.1 * u.1 + normal.2 * u.2).abs() < 1e-9);
            }
        }

        #[test]
        fn arc_midpoint_splits_the_angle(
            a in (-180.0f64..180.0, -89.0f64..89.0),
            b in (-180.0f64..180.0, -89.0f64..89.0),
        ) {
            prop_assume!(angle(a, b) < 3.0);
            let mid = arc(a, b, 2)[1];
            prop_assert!((angle(a, mid) - angle(mid, b)).abs() < 1e-6);
            prop_assert!((angle(a, mid) + angle(mid, b) - angle(a, b)).abs() < 1e-6);
        }

        #[test]
        fn trace_never_jumps(
            pts in prop::collection::vec((-180.0f64..180.0, -85.0f64..85.0), 1..30),
        ) {
            let t = trace(&pts);
            prop_assert_eq!(t.len(), pts.len());
            for w in t.windows(2) {
                prop_assert!((w[1].0 - w[0].0).abs() <= WORLD / 2.0 + 1e-9);
            }
        }
    }

    #[test]
    fn arc_exact() {
        let equator = arc((0.0, 0.0), (90.0, 0.0), 3);
        assert!(equator.iter().all(|&(_, lat)| lat.abs() < 1e-9));
        assert!(equator.windows(2).all(|w| w[1].0 > w[0].0));
        let meridian = arc((10.0, 0.0), (10.0, 60.0), 4);
        assert!(meridian.iter().all(|&(lon, _)| (lon - 10.0).abs() < 1e-9));
        // paris to san francisco bends over the north atlantic
        let top = arc((2.35, 48.86), (-122.42, 37.77), 32)
            .iter()
            .map(|&(_, lat)| lat)
            .fold(0.0, f64::max);
        assert!(top > 52.0);
        assert_eq!(arc((5.0, 5.0), (5.0, 5.0), 2), vec![(5.0, 5.0); 3]);
    }

    #[test]
    fn trace_crosses_the_pacific_the_short_way() {
        let t = trace(&[(-122.4, 37.8), (139.7, 35.7)]);
        assert!(t[1].0 < t[0].0);
        assert!((t[0].0 - t[1].0) < WORLD / 2.0);
        assert_eq!(path(&[(1.0, 2.0), (3.04, 4.06)]), "M1.0,2.0L3.0,4.1");
        assert_eq!(path(&[]), "");
    }

    #[test]
    fn project_exact() {
        assert_eq!(project(0.0, 0.0), (500.0, 500.0));
        assert_eq!(project(180.0, 0.0), (1000.0, 500.0));
        assert_eq!(project(-180.0, 0.0), (0.0, 500.0));
        assert!(project(0.0, 90.0).1 < 1e-3);
        assert!(project(0.0, -90.0).1 > WORLD - 1e-3);
    }

    #[test]
    fn fit_never_collapses() {
        let v = fit((5.0, 5.0), (5.0, 5.0), 2.0, 0.0, 0.0);
        assert!(v.w > 0.0 && v.h > 0.0);
        assert!(v.frac((5.0, 5.0)).0.is_finite());
        assert!(v.toward(&world(2.0), 0.5).w.is_finite());
    }

    #[test]
    fn world_holds_the_cities() {
        let v = world(2.0);
        assert!(holds(&v, project(4.5, 50.9)));
        assert!(holds(&v, project(-122.4, 37.8)));
        assert!(holds(&v, project(139.7, 35.7)));
        assert!((v.w / v.h - 2.0).abs() < 1e-9);
        assert_eq!(v.frac((v.x, v.y)), (0.0, 0.0));
    }

    #[test]
    fn nearest_exact() {
        assert_eq!(nearest(100.0, 900.0), -100.0);
        assert_eq!(nearest(900.0, 100.0), 1100.0);
        assert_eq!(nearest(400.0, 600.0), 600.0);
    }

    #[test]
    fn ease_exact() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(0.5), 0.5);
        assert_eq!(ease(1.0), 1.0);
        assert_eq!(ease(2.0), 1.0);
    }

    #[test]
    fn land_exact() {
        assert_eq!(land(""), Some(String::new()));
        assert_eq!(
            land("0,0 180,0 0,90\n-180,0\n"),
            Some("M500.0,500.0L1000.0,500.0L500.0,0.0ZM0.0,500.0Z".to_string())
        );
        assert_eq!(land("0,0 nope"), None);
        assert_eq!(land("0;0"), None);
        assert_eq!(
            borders("0,0 180,0\n-180,0 0,0\n"),
            Some("M500.0,500.0L1000.0,500.0M0.0,500.0L500.0,500.0".to_string())
        );
        assert_eq!(borders("1,1 x"), None);
    }

    #[test]
    fn borders_parse() {
        let d = borders(BORDERS).unwrap();
        assert!(d.starts_with('M') && !d.contains('Z'));
        assert_eq!(d.matches('M').count(), BORDERS.lines().count());
        assert!(BORDERS.lines().count() > 100);
    }

    fn town(name: &str, lon: f64, lat: f64, zoom: f64) -> Place {
        Place {
            name: name.to_string(),
            at: project(lon, lat),
            zoom,
        }
    }

    proptest! {
        #[test]
        fn labels_keep_their_distance(
            raw in prop::collection::vec((-180.0f64..180.0, -80.0f64..80.0, 0.0f64..9.0), 0..80),
            taken in prop::collection::vec((0.0f64..1.0, 0.0f64..1.0), 0..3),
            w in 24.0f64..WORLD,
            limit in 0usize..20,
        ) {
            let mut towns: Vec<Place> = raw
                .iter()
                .enumerate()
                .map(|(i, &(lon, lat, z))| town(&i.to_string(), lon, lat, z))
                .collect();
            towns.sort_by(|a, b| a.zoom.total_cmp(&b.zoom));
            let v = fit((400.0, 300.0), (400.0 + w / 2.0, 300.0 + w / 4.0), 2.0, 0.0, w);
            let out = labels(&towns, &v, &taken, GAP, limit);
            prop_assert!(out.len() <= limit);
            let spots: Vec<(f64, f64)> = taken.iter().copied().chain(out.iter().map(|o| o.frac)).collect();
            for (i, l) in out.iter().enumerate() {
                let (fx, fy) = l.frac;
                prop_assert!((0.0..1.0 - GAP.0).contains(&fx) && (GAP.1..1.0 - GAP.1).contains(&fy));
                prop_assert!(l.place.zoom <= zoom(v.w));
                prop_assert!(holds(&v, l.at));
                prop_assert!((v.frac(l.at).0 - fx).abs() < 1e-9);
                for (j, &(sx, sy)) in spots.iter().enumerate() {
                    if j != taken.len() + i {
                        prop_assert!((sx - fx).abs() >= GAP.0 || (sy - fy).abs() >= GAP.1);
                    }
                }
            }
            for w in out.windows(2) {
                prop_assert!(w[0].place.zoom <= w[1].place.zoom);
            }
        }

        #[test]
        fn zoom_falls_as_the_view_widens(a in 1.0f64..WORLD, b in 1.0f64..WORLD) {
            let (narrow, wide) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(zoom(narrow) >= zoom(wide));
        }
    }

    #[test]
    fn labels_exact() {
        let towns = [
            town("Lyon", 4.83, 45.77, 4.7),
            town("Grenoble", 5.72, 45.18, 6.1),
            town("Vienne", 4.87, 45.53, 8.0),
        ];
        let v = fit(project(3.0, 44.0), project(7.0, 47.0), 2.0, 0.2, 24.0);
        let shown: Vec<&str> = labels(&towns, &v, &[], GAP, 10)
            .iter()
            .map(|l| l.place.name.as_str())
            .collect();
        assert_eq!(shown, vec!["Lyon", "Grenoble"]);
        let lyon = labels(&towns, &v, &[], GAP, 10)[0].frac;
        assert!(
            labels(&towns, &v, &[lyon], GAP, 10)
                .iter()
                .all(|l| l.place.name != "Lyon")
        );
        assert!(labels(&towns, &world(2.0), &[], GAP, 10).is_empty());
        assert_eq!(labels(&towns, &v, &[], GAP, 0).len(), 0);
        // a town on the right edge would clip and stays out
        let edge = View {
            x: towns[0].at.0 - 95.0,
            y: towns[0].at.1 - 25.0,
            w: 100.0,
            h: 50.0,
        };
        assert!(labels(&towns, &edge, &[], GAP, 10).is_empty());
    }

    #[test]
    fn labels_follow_a_wrapped_view() {
        let towns = [town("Tokyo", 139.69, 35.68, 1.7)];
        let mut v = fit(project(-170.0, 30.0), project(-120.0, 40.0), 2.0, 0.0, 24.0);
        assert!(labels(&towns, &v, &[], GAP, 10).is_empty());
        // stretch west past the antimeridian, tokyo's western copy is now in view
        v.x -= 200.0;
        v.w += 200.0;
        v.h = v.w / 2.0;
        let shown = labels(&towns, &v, &[], GAP, 10);
        assert_eq!(shown.len(), 1);
        assert!(shown[0].frac.0 < 0.5);
        assert!(shown[0].at.0 < 0.0);
    }

    #[test]
    fn places_exact() {
        let p = places("1.7\t139.69\t35.68\tTokyo\n6.1\t5.72\t45.18\tGrenoble\n").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].name, "Grenoble");
        assert_eq!(p[1].zoom, 6.1);
        assert_eq!(p[1].at, project(5.72, 45.18));
        assert_eq!(places("1.7\t139.69\tTokyo\n"), None);
        assert_eq!(places("x\t1\t2\tName\n"), None);
        assert_eq!(places(""), Some(Vec::new()));
    }

    #[test]
    fn places_parse_sorted() {
        let p = places(PLACES).unwrap();
        assert!(p.len() > 1000);
        assert!(p.windows(2).all(|w| w[0].zoom <= w[1].zoom));
        assert!(p.iter().any(|t| t.name == "Grenoble"));
    }

    #[test]
    fn land_outline_parses() {
        let d = land(LAND).unwrap();
        assert!(d.starts_with('M') && d.ends_with('Z'));
        assert_eq!(d.matches('M').count(), LAND.lines().count());
        assert!(LAND.lines().count() > 100);
    }
}
