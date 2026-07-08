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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
        }

        #[test]
        fn mbps_linear_in_bytes(bytes in 1u64..1_000_000_000, secs in 0.001f64..100.0) {
            let one = mbps(bytes, secs);
            let two = mbps(bytes * 2, secs);
            prop_assert!((two - one * 2.0).abs() < 1e-6 * two.max(1.0));
        }
    }

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
