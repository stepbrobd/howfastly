use serde::{Deserialize, Serialize};

use crate::stats;

// larger transfers have lower relative variance and need fewer repeats
pub const DOWNLOAD_PLAN: [SizePlan; 5] = [
    SizePlan::new(100_000, 8),
    SizePlan::new(1_000_000, 8),
    SizePlan::new(10_000_000, 6),
    SizePlan::new(25_000_000, 4),
    SizePlan::new(100_000_000, 2),
];
pub const UPLOAD_PLAN: [SizePlan; 5] = [
    SizePlan::new(100_000, 8),
    SizePlan::new(1_000_000, 8),
    SizePlan::new(10_000_000, 6),
    SizePlan::new(25_000_000, 4),
    SizePlan::new(50_000_000, 2),
];
pub const LATENCY_SAMPLES: usize = 25;
pub const TIME_BUDGET_SECS: f64 = 30.0;
pub const LOADED_PING_INTERVAL_MS: u64 = 400;

#[derive(Clone, Copy, Debug)]
pub struct SizePlan {
    pub bytes: u64,
    pub iterations: usize,
}

impl SizePlan {
    pub const fn new(bytes: u64, iterations: usize) -> Self {
        Self { bytes, iterations }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MetaResponse {
    pub client_ip: String,
    pub asn: u32,
    pub as_org: String,
    pub city: String,
    pub country: String,
    pub pop: String,
    #[serde(default)]
    pub protocol: String,
    pub service_version: String,
}

#[derive(Clone, Debug)]
pub struct TestConfig {
    pub latency_samples: usize,
    pub download: Vec<SizePlan>,
    pub upload: Vec<SizePlan>,
    pub time_budget_secs: f64,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            latency_samples: LATENCY_SAMPLES,
            download: DOWNLOAD_PLAN.to_vec(),
            upload: UPLOAD_PLAN.to_vec(),
            time_budget_secs: TIME_BUDGET_SECS,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SizeSamples {
    pub bytes: u64,
    pub mbps: Vec<f64>,
    pub skipped: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LatencySummary {
    pub min_ms: f64,
    pub avg_ms: f64,
    pub median_ms: f64,
    pub jitter_ms: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SizeSummary {
    pub bytes: u64,
    pub samples: usize,
    pub median_mbps: Option<f64>,
    pub skipped: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DirectionSummary {
    pub p90_mbps: Option<f64>,
    pub sizes: Vec<SizeSummary>,
    pub loaded_latency: Option<LatencySummary>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SpeedtestResults {
    pub meta: Option<MetaResponse>,
    pub latency: Option<LatencySummary>,
    pub download: Option<DirectionSummary>,
    pub upload: Option<DirectionSummary>,
}

// fastly geo data arrives lowercased
// capitalize at word boundaries and leave already-cased characters alone
pub fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut boundary = true;
    for c in s.chars() {
        if boundary {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        boundary = matches!(c, ' ' | '-' | '.');
    }
    out
}

pub fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{} MB", bytes / 1_000_000)
    } else {
        format!("{} kB", bytes / 1_000)
    }
}

pub fn summarize_latency(samples_ms: &[f64]) -> Option<LatencySummary> {
    let median_ms = stats::median(samples_ms)?;
    Some(LatencySummary {
        min_ms: samples_ms.iter().copied().fold(f64::INFINITY, f64::min),
        avg_ms: samples_ms.iter().sum::<f64>() / samples_ms.len() as f64,
        median_ms,
        jitter_ms: stats::jitter(samples_ms).unwrap_or(0.0),
    })
}

pub fn summarize_direction(sizes: &[SizeSamples], loaded_ms: &[f64]) -> DirectionSummary {
    let all: Vec<f64> = sizes.iter().flat_map(|s| s.mbps.iter().copied()).collect();
    DirectionSummary {
        p90_mbps: stats::percentile(&all, 90.0),
        sizes: sizes
            .iter()
            .map(|s| SizeSummary {
                bytes: s.bytes,
                samples: s.mbps.len(),
                median_mbps: stats::median(&s.mbps),
                skipped: s.skipped,
            })
            .collect(),
        loaded_latency: summarize_latency(loaded_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_summary() {
        assert!(summarize_latency(&[]).is_none());
        let s = summarize_latency(&[2.0, 4.0, 3.0]).unwrap();
        assert_eq!(s.min_ms, 2.0);
        assert_eq!(s.avg_ms, 3.0);
        assert_eq!(s.median_ms, 3.0);
        assert_eq!(s.jitter_ms, 1.5);
    }

    #[test]
    fn direction_summary() {
        let sizes = [
            SizeSamples {
                bytes: 100,
                mbps: vec![10.0, 20.0],
                skipped: false,
            },
            SizeSamples {
                bytes: 200,
                mbps: vec![],
                skipped: true,
            },
        ];
        let d = summarize_direction(&sizes, &[]);
        assert!(d.p90_mbps.unwrap() > 10.0);
        assert_eq!(d.sizes.len(), 2);
        assert_eq!(d.sizes[0].median_mbps, Some(15.0));
        assert!(d.sizes[1].skipped);
        assert!(d.loaded_latency.is_none());
    }

    #[test]
    fn title_cases() {
        assert_eq!(title_case("nantes"), "Nantes");
        assert_eq!(title_case("linkt sas"), "Linkt Sas");
        assert_eq!(title_case("stoke-on-trent"), "Stoke-On-Trent");
        assert_eq!(title_case("San Francisco"), "San Francisco");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn size_labels() {
        assert_eq!(size_label(100_000), "100 kB");
        assert_eq!(size_label(1_000_000), "1 MB");
        assert_eq!(size_label(25_000_000), "25 MB");
    }

    #[test]
    fn results_roundtrip() {
        let r = SpeedtestResults {
            meta: Some(MetaResponse::default()),
            latency: summarize_latency(&[1.0, 2.0]),
            download: Some(summarize_direction(&[], &[])),
            upload: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: SpeedtestResults = serde_json::from_str(&json).unwrap();
        assert_eq!(back.latency.unwrap().min_ms, 1.0);
        assert!(back.upload.is_none());
    }
}
