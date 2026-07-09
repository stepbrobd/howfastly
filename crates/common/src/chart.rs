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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn format_speed_value_in_display_range(bps in 1.0f64..1e12) {
            let (v, _) = format_speed(bps);
            prop_assert!((1.0..1000.0).contains(&v));
        }

        #[test]
        fn format_speed_never_negative(bps in -1e12f64..1e12) {
            prop_assert!(format_speed(bps).0 >= 0.0);
        }
    }

    #[test]
    fn format_speed_exact() {
        assert_eq!(format_speed(0.0), (0.0, "bps"));
        assert_eq!(format_speed(999.0), (999.0, "bps"));
        assert_eq!(format_speed(1_000.0), (1.0, "kbps"));
        assert_eq!(format_speed(1e6), (1.0, "Mbps"));
        assert_eq!(format_speed(2.5e9), (2.5, "Gbps"));
        assert_eq!(format_speed(f64::NAN), (0.0, "bps"));
        assert_eq!(format_speed(-5.0), (0.0, "bps"));
    }
}
