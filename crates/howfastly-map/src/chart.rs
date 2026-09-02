pub fn format_speed(bps: f64) -> (f64, &'static str) {
    let bps = if bps.is_finite() { bps.max(0.0) } else { 0.0 };
    if bps < 1e3 {
        (bps, "bps")
    } else if bps < 1e6 {
        (bps / 1e3, "kbps")
    } else if bps < 1e9 {
        (bps / 1e6, "Mbps")
    } else {
        (bps / 1e9, "Gbps")
    }
}

// the largest sample, floored so an empty or flat series still scales
pub fn peak(points: &[(f64, f64)]) -> f64 {
    points.iter().map(|&(_, y)| y).fold(f64::EPSILON, f64::max)
}

// svg y grows downwards, larger values sit higher
// a zero ceiling would divide by zero, so it is floored like peak
pub fn chart_y(value: f64, max: f64, height: f64) -> f64 {
    height - value.max(0.0) / max.max(f64::EPSILON) * height
}

// map samples into svg space
// x spans the input range
// y spans 0 to max, which is at least the peak so every sample fits
fn chart_coords(points: &[(f64, f64)], width: f64, height: f64, max: f64) -> Vec<(f64, f64)> {
    let (x0, x1) = match (points.first(), points.last()) {
        (Some(&(a, _)), Some(&(b, _))) => (a, b),
        _ => return Vec::new(),
    };
    let span = (x1 - x0).max(f64::EPSILON);
    points
        .iter()
        .map(|&(x, y)| ((x - x0) / span * width, chart_y(y, max, height)))
        .collect()
}

pub fn svg_path(points: &[(f64, f64)], width: f64, height: f64, max: f64) -> String {
    let coords = chart_coords(points, width, height, max);
    if coords.len() < 2 {
        return String::new();
    }
    coords
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            let cmd = if i == 0 { 'M' } else { 'L' };
            format!("{cmd}{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// events are (elapsed ms, bytes since previous event) in time order
// emit at most one point per emit_ms
// speed is measured over the trailing window_ms
pub fn throughput_points(events: &[(f64, u64)], window_ms: f64, emit_ms: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut next_emit = emit_ms;
    for (i, &(t, _)) in events.iter().enumerate() {
        if t < next_emit {
            continue;
        }
        let from = t - window_ms;
        let bytes: u64 = events[..=i]
            .iter()
            .rev()
            .take_while(|&&(tt, _)| tt > from)
            .map(|&(_, b)| b)
            .sum();
        let secs = (window_ms.min(t).max(f64::EPSILON)) / 1e3;
        out.push((t / 1e3, bytes as f64 * 8.0 / secs));
        next_emit = t + emit_ms;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
    #[test]
    fn chart_coords_within_bounds(
        raw in prop::collection::vec((0.0f64..1e4, 0.0f64..1e10), 2..200),
    ) {
        let mut pts = raw;
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (x, y) in chart_coords(&pts, 300.0, 80.0, peak(&pts)) {
            prop_assert!((-1e-9..=300.0 + 1e-9).contains(&x));
            prop_assert!((-1e-9..=80.0 + 1e-9).contains(&y));
        }
    }

    #[test]
    fn chart_y_within_frame_below_ceiling(
        raw in prop::collection::vec((0.0f64..1e4, 0.0f64..1e10), 1..50),
        headroom in 1.0f64..10.0,
    ) {
        let max = peak(&raw) * headroom;
        for &(_, y) in &raw {
            prop_assert!((0.0..=80.0).contains(&chart_y(y, max, 80.0)));
        }
    }

    #[test]
    fn chart_coords_x_monotone(
        raw in prop::collection::vec((0.0f64..1e4, 0.0f64..1e10), 2..200),
    ) {
        let mut pts = raw;
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let coords = chart_coords(&pts, 300.0, 80.0, peak(&pts));
        for w in coords.windows(2) {
            prop_assert!(w[1].0 >= w[0].0);
        }
    }

    #[test]
    fn format_speed_value_in_display_range(bps in 1.0f64..1e12) {
        let (v, _) = format_speed(bps);
        prop_assert!((1.0..1000.0).contains(&v));
    }    }

    #[test]
    fn format_speed_exact() {
        assert_eq!(format_speed(0.0), (0.0, "bps"));
        assert_eq!(format_speed(999.0), (999.0, "bps"));
        assert_eq!(format_speed(1_000.0), (1.0, "kbps"));
        assert_eq!(format_speed(1e6), (1.0, "Mbps"));
        assert_eq!(format_speed(1e9), (1.0, "Gbps"));
        assert_eq!(format_speed(2.5e9), (2.5, "Gbps"));
        assert_eq!(format_speed(f64::NAN), (0.0, "bps"));
        assert_eq!(format_speed(-5.0), (0.0, "bps"));
    }

    #[test]
    fn svg_path_exact() {
        assert_eq!(svg_path(&[], 300.0, 80.0, 1.0), "");
        assert_eq!(svg_path(&[(0.0, 1.0)], 300.0, 80.0, 1.0), "");
        let pts = [(0.0, 0.0), (1.0, 10.0)];
        assert_eq!(
            svg_path(&pts, 300.0, 80.0, peak(&pts)),
            "M0.0,80.0 L300.0,0.0"
        );
        assert_eq!(svg_path(&pts, 300.0, 80.0, 20.0), "M0.0,80.0 L300.0,40.0");
    }
    #[test]
    fn peak_exact() {
        assert_eq!(peak(&[]), f64::EPSILON);
        assert_eq!(peak(&[(0.0, 3.0), (1.0, 7.0), (2.0, 5.0)]), 7.0);
        assert_eq!(chart_y(7.0, 7.0, 80.0), 0.0);
        assert_eq!(chart_y(0.0, 7.0, 80.0), 80.0);
        assert_eq!(chart_y(-1.0, 7.0, 80.0), 80.0);
        assert!(chart_y(1.0, 0.0, 80.0).is_finite());
    }

    proptest! {
        #[test]
        fn throughput_points_monotone_finite(
            deltas in prop::collection::vec((0.1f64..500.0, 0u64..10_000_000), 1..300),
        ) {
            let mut t = 0.0;
            let events: Vec<(f64, u64)> = deltas
                .into_iter()
                .map(|(dt, b)| {
                    t += dt;
                    (t, b)
                })
                .collect();
            let pts = throughput_points(&events, 500.0, 100.0);
            prop_assert!(pts.len() <= events.len());
            for w in pts.windows(2) {
                prop_assert!(w[1].0 > w[0].0);
            }
            for &(_, bps) in &pts {
                prop_assert!(bps.is_finite() && bps >= 0.0);
            }
        }
    }

    #[test]
    fn throughput_points_exact() {
        assert!(throughput_points(&[], 500.0, 100.0).is_empty());
        // one event of 1000 bytes at 200ms
        // the window truncates to elapsed time
        // 1000 bytes * 8 bits over 0.2s = 40_000 bps at t 0.2s
        let pts = throughput_points(&[(200.0, 1_000)], 500.0, 100.0);
        assert_eq!(pts, vec![(0.2, 40_000.0)]);
    }
}
