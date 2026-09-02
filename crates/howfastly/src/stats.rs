pub fn mbps(bytes: u64, seconds: f64) -> f64 {
    bytes as f64 * 8.0 / seconds / 1e6
}

pub fn median(samples: &[f64]) -> Option<f64> {
    percentile(samples, 50.0)
}

// linear interpolation between closest ranks
pub fn percentile(samples: &[f64], p: f64) -> Option<f64> {
    if samples.is_empty() || !(0.0..=100.0).contains(&p) {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rank = p / 100.0 * (sorted.len() - 1) as f64;
    let lo = sorted[rank.floor() as usize];
    let hi = sorted[rank.ceil() as usize];
    Some(lo + (hi - lo) * rank.fract())
}

// mean absolute difference of consecutive samples
pub fn jitter(samples: &[f64]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let sum: f64 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    Some(sum / (samples.len() - 1) as f64)
}

// plausible breaks props down per distinct value
// measurements reach it as coarse buckets
const SPEED_BUCKETS: [(f64, &str); 7] = [
    (10.0, "< 10 Mbps"),
    (50.0, "10-50 Mbps"),
    (100.0, "50-100 Mbps"),
    (250.0, "100-250 Mbps"),
    (500.0, "250-500 Mbps"),
    (1000.0, "500-1000 Mbps"),
    (f64::INFINITY, "1+ Gbps"),
];
const LATENCY_BUCKETS: [(f64, &str); 5] = [
    (10.0, "< 10 ms"),
    (25.0, "10-25 ms"),
    (50.0, "25-50 ms"),
    (100.0, "50-100 ms"),
    (f64::INFINITY, "100+ ms"),
];

fn bucket(value: f64, buckets: &[(f64, &'static str)]) -> &'static str {
    buckets
        .iter()
        .find(|(limit, _)| value < *limit)
        .map_or(buckets[buckets.len() - 1].1, |(_, name)| name)
}

pub fn speed_bucket(mbps: f64) -> &'static str {
    bucket(mbps, &SPEED_BUCKETS)
}

pub fn latency_bucket(ms: f64) -> &'static str {
    bucket(ms, &LATENCY_BUCKETS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn index(buckets: &[(f64, &str)], name: &str) -> usize {
        buckets.iter().position(|(_, n)| *n == name).unwrap()
    }

    proptest! {
        #[test]
        fn buckets_monotone(a in 0.0f64..5000.0, b in 0.0f64..5000.0) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(index(&SPEED_BUCKETS, speed_bucket(lo)) <= index(&SPEED_BUCKETS, speed_bucket(hi)));
            prop_assert!(index(&LATENCY_BUCKETS, latency_bucket(lo)) <= index(&LATENCY_BUCKETS, latency_bucket(hi)));
        }
    }

    #[test]
    fn bucket_edges() {
        assert_eq!(speed_bucket(9.99), "< 10 Mbps");
        assert_eq!(speed_bucket(10.0), "10-50 Mbps");
        assert_eq!(speed_bucket(1000.0), "1+ Gbps");
        assert_eq!(latency_bucket(0.0), "< 10 ms");
        assert_eq!(latency_bucket(100.0), "100+ ms");
        assert_eq!(latency_bucket(f64::NAN), "100+ ms");
    }

    proptest! {
    #[test]
    fn percentile_within_bounds(
        samples in prop::collection::vec(0.0f64..1e6, 1..100),
        p in 0.0f64..=100.0,
    ) {
        let v = percentile(&samples, p).unwrap();
        let min = samples.iter().copied().fold(f64::INFINITY, f64::min);
        let max = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        prop_assert!(v >= min && v <= max);
    }

    #[test]
    fn percentile_monotone_in_p(
        samples in prop::collection::vec(0.0f64..1e6, 1..100),
        p in 0.0f64..=99.0,
    ) {
        prop_assert!(percentile(&samples, p).unwrap() <= percentile(&samples, p + 1.0).unwrap());
    }

    #[test]
    fn jitter_of_constant_is_zero(x in 0.0f64..1e6, n in 2usize..50) {
        prop_assert_eq!(jitter(&vec![x; n]), Some(0.0));
    }    }

    #[test]
    fn percentile_exact() {
        let s = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&s, 0.0), Some(1.0));
        assert_eq!(percentile(&s, 50.0), Some(2.5));
        assert_eq!(percentile(&s, 100.0), Some(4.0));
        assert_eq!(percentile(&[], 50.0), None);
        assert_eq!(median(&s), Some(2.5));
    }

    #[test]
    fn jitter_exact() {
        assert_eq!(jitter(&[1.0]), None);
        assert_eq!(jitter(&[1.0, 3.0, 2.0]), Some(1.5));
    }

    #[test]
    fn mbps_exact() {
        assert_eq!(mbps(1_000_000, 8.0), 1.0);
    }
}
