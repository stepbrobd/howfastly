use serde::{Deserialize, Serialize};

use crate::stats;

pub const DOWNLOAD_SIZES: [u64; 5] = [100_000, 1_000_000, 10_000_000, 25_000_000, 100_000_000];
pub const UPLOAD_SIZES: [u64; 5] = [100_000, 1_000_000, 10_000_000, 25_000_000, 50_000_000];
pub const LATENCY_SAMPLES: usize = 25;
pub const ITERATIONS: usize = 8;
pub const TIME_BUDGET_SECS: f64 = 30.0;
pub const LOADED_PING_INTERVAL_MS: u64 = 400;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MetaResponse {
    pub client_ip: String,
    pub asn: u32,
    pub as_org: String,
    pub city: String,
    pub country: String,
    pub pop: String,
    pub service_version: String,
}

#[derive(Clone, Debug)]
pub struct TestConfig {
    pub latency_samples: usize,
    pub iterations: usize,
    pub download_sizes: Vec<u64>,
    pub upload_sizes: Vec<u64>,
    pub time_budget_secs: f64,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            latency_samples: LATENCY_SAMPLES,
            iterations: ITERATIONS,
            download_sizes: DOWNLOAD_SIZES.to_vec(),
            upload_sizes: UPLOAD_SIZES.to_vec(),
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

pub fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{}mb", bytes / 1_000_000)
    } else {
        format!("{}kb", bytes / 1_000)
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
    fn size_labels() {
        assert_eq!(size_label(100_000), "100kb");
        assert_eq!(size_label(1_000_000), "1mb");
        assert_eq!(size_label(25_000_000), "25mb");
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
