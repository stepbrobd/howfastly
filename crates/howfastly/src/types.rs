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

// wgs 84 degrees, the shape of the datacenters api coordinates object
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MetaResponse {
    pub ip: String,
    pub asn: u32,
    pub org: String,
    pub city: String,
    pub country: String,
    pub coordinates: Option<Coordinates>,
    pub pop: Pop,
    pub protocol: String,
    pub version: String,
    pub cargo: String,
}

impl MetaResponse {
    // a differing build that still decodes is a warning, not a failure
    pub fn mismatch(&self) -> Option<MetaError> {
        (self.cargo != crate::VERSION).then(|| MetaError::Mismatch(self.cargo.clone()))
    }
}

#[derive(Debug)]
pub enum MetaError {
    Mismatch(String),
    Missing,
    Invalid(serde_json::Error),
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let build = crate::VERSION;
        match self {
            Self::Mismatch(server) => {
                write!(
                    f,
                    "Server runs HowFastly {server} but this build is {build}"
                )
            }
            Self::Missing => write!(
                f,
                "Server predates version reporting, this build is {build}"
            ),
            Self::Invalid(e) => write!(f, "Invalid meta response: {e}"),
        }
    }
}

impl std::error::Error for MetaError {}

// a shape mismatch names the server version so an old build knows to upgrade
pub fn parse_meta(body: &str) -> Result<MetaResponse, MetaError> {
    let err = match serde_json::from_str(body) {
        Ok(meta) => return Ok(meta),
        Err(e) => e,
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Err(MetaError::Invalid(err));
    };
    Err(match value.get("cargo").and_then(|c| c.as_str()) {
        Some(server) if server != crate::VERSION => MetaError::Mismatch(server.to_string()),
        Some(_) => MetaError::Invalid(err),
        None => MetaError::Missing,
    })
}

// also one entry of the fastly datacenters api response
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Pop {
    pub code: String,
    pub name: String,
    pub group: String,
    pub coordinates: Option<Coordinates>,
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
    pub min: f64,
    pub avg: f64,
    pub median: f64,
    pub jitter: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SizeSummary {
    pub bytes: u64,
    pub samples: usize,
    pub median: Option<f64>,
    pub skipped: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DirectionSummary {
    pub p90: Option<f64>,
    pub sizes: Vec<SizeSummary>,
    pub loaded: Option<LatencySummary>,
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
    let median = stats::median(samples_ms)?;
    Some(LatencySummary {
        min: samples_ms.iter().copied().fold(f64::INFINITY, f64::min),
        avg: samples_ms.iter().sum::<f64>() / samples_ms.len() as f64,
        median,
        jitter: stats::jitter(samples_ms).unwrap_or(0.0),
    })
}

pub fn summarize_direction(sizes: &[SizeSamples], loaded_ms: &[f64]) -> DirectionSummary {
    let all: Vec<f64> = sizes.iter().flat_map(|s| s.mbps.iter().copied()).collect();
    DirectionSummary {
        p90: stats::percentile(&all, 90.0),
        sizes: sizes
            .iter()
            .map(|s| SizeSummary {
                bytes: s.bytes,
                samples: s.mbps.len(),
                median: stats::median(&s.mbps),
                skipped: s.skipped,
            })
            .collect(),
        loaded: summarize_latency(loaded_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_fallback() {
        let ok = serde_json::to_string(&MetaResponse {
            cargo: crate::VERSION.to_string(),
            ..Default::default()
        })
        .unwrap();
        assert!(parse_meta(&ok).unwrap().mismatch().is_none());
        assert!(matches!(
            parse_meta(r#"{"cargo":"0.0.0","pop":"BRU"}"#),
            Err(MetaError::Mismatch(v)) if v == "0.0.0"
        ));
        assert!(matches!(
            parse_meta(r#"{"pop":"BRU"}"#),
            Err(MetaError::Missing)
        ));
        assert!(matches!(parse_meta("nope"), Err(MetaError::Invalid(_))));
    }

    #[test]
    fn pop_from_datacenters_entry() {
        let entry = r#"{"code":"BRU","name":"Brussels","group":"Europe","region":"EU-Central",
            "coordinates":{"x":0,"y":0,"latitude":50.871,"longitude":4.476},"shield":"bru-brussels-be"}"#;
        let pop: Pop = serde_json::from_str(entry).unwrap();
        assert_eq!(pop.code, "BRU");
        assert_eq!(
            pop.coordinates,
            Some(Coordinates {
                latitude: 50.871,
                longitude: 4.476
            })
        );
        let bare: Pop = serde_json::from_str(r#"{"code":"XXX","name":"","group":""}"#).unwrap();
        assert_eq!(bare.coordinates, None);
    }

    #[test]
    fn latency_summary() {
        assert!(summarize_latency(&[]).is_none());
        let s = summarize_latency(&[2.0, 4.0, 3.0]).unwrap();
        assert_eq!(s.min, 2.0);
        assert_eq!(s.avg, 3.0);
        assert_eq!(s.median, 3.0);
        assert_eq!(s.jitter, 1.5);
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
        assert!(d.p90.unwrap() > 10.0);
        assert_eq!(d.sizes.len(), 2);
        assert_eq!(d.sizes[0].median, Some(15.0));
        assert!(d.sizes[1].skipped);
        assert!(d.loaded.is_none());
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
        assert_eq!(back.latency.unwrap().min, 1.0);
        assert!(back.upload.is_none());
    }
}
