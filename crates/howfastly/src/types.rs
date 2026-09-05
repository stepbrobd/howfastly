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
pub const LOADED_PING_INTERVAL_MS: u32 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Download,
    Upload,
}

impl Direction {
    pub const ALL: [Direction; 2] = [Direction::Download, Direction::Upload];

    pub fn name(self) -> &'static str {
        match self {
            Self::Download => "Download",
            Self::Upload => "Upload",
        }
    }

    pub fn plan(self) -> &'static [SizePlan] {
        match self {
            Self::Download => &DOWNLOAD_PLAN,
            Self::Upload => &UPLOAD_PLAN,
        }
    }
}

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
    // the nix store path of the serving build, absent outside nix
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
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

impl TestConfig {
    pub fn plans(&self, dir: Direction) -> &[SizePlan] {
        match dir {
            Direction::Download => &self.download,
            Direction::Upload => &self.upload,
        }
    }
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

impl SpeedtestResults {
    pub fn direction(&self, dir: Direction) -> Option<&DirectionSummary> {
        match dir {
            Direction::Download => self.download.as_ref(),
            Direction::Upload => self.upload.as_ref(),
        }
    }

    pub fn record(&mut self, dir: Direction, summary: DirectionSummary) {
        match dir {
            Direction::Download => self.download = Some(summary),
            Direction::Upload => self.upload = Some(summary),
        }
    }
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
    use proptest::prelude::*;

    // serde_json parses floats best effort, a nanodegree is well under a millimeter
    fn close(a: Option<Coordinates>, b: Option<Coordinates>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => {
                (a.latitude - b.latitude).abs() < 1e-9 && (a.longitude - b.longitude).abs() < 1e-9
            }
            (None, None) => true,
            _ => false,
        }
    }

    proptest! {
        #[test]
        fn title_case_keeps_length_and_settles(s in "[a-z .-]{0,20}") {
            let once = title_case(&s);
            prop_assert_eq!(once.chars().count(), s.chars().count());
            prop_assert_eq!(title_case(&once), once.clone());
            prop_assert_eq!(once.to_lowercase(), s);
        }

        #[test]
        fn latency_summary_orders(samples in prop::collection::vec(0.0f64..1e4, 1..50)) {
            let s = summarize_latency(&samples).unwrap();
            let max = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            prop_assert!(s.min <= s.median && s.median <= max);
            prop_assert!(s.min <= s.avg && s.avg <= max + 1e-9);
            prop_assert!(s.jitter >= 0.0);
        }

        #[test]
        fn direction_summary_keeps_shape(
            sizes in prop::collection::vec(
                (1u64..1_000_000, prop::collection::vec(0.0f64..1e4, 0..8), any::<bool>()),
                0..6,
            ),
            loaded in prop::collection::vec(0.0f64..1e4, 0..20),
        ) {
            let samples: Vec<SizeSamples> = sizes
                .iter()
                .map(|(bytes, mbps, skipped)| SizeSamples {
                    bytes: *bytes,
                    mbps: mbps.clone(),
                    skipped: *skipped,
                })
                .collect();
            let d = summarize_direction(&samples, &loaded);
            prop_assert_eq!(d.sizes.len(), samples.len());
            prop_assert_eq!(d.loaded.is_some(), !loaded.is_empty());
            let all: Vec<f64> = samples.iter().flat_map(|s| s.mbps.iter().copied()).collect();
            match d.p90 {
                Some(p) => {
                    let lo = all.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = all.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    prop_assert!(lo <= p && p <= hi);
                }
                None => prop_assert!(all.is_empty()),
            }
            for (s, out) in samples.iter().zip(&d.sizes) {
                prop_assert_eq!(out.bytes, s.bytes);
                prop_assert_eq!(out.samples, s.mbps.len());
                prop_assert_eq!(out.skipped, s.skipped);
                prop_assert_eq!(out.median.is_some(), !s.mbps.is_empty());
            }
        }

        #[test]
        fn meta_roundtrips(
            ip in "[0-9a-f:.]{1,40}",
            asn in any::<u32>(),
            city in "[A-Za-z ]{0,20}",
            lat in -90.0f64..90.0,
            lon in -180.0f64..180.0,
            code in "[A-Z]{3}",
        ) {
            let meta = MetaResponse {
                ip,
                asn,
                city,
                coordinates: Some(Coordinates { latitude: lat, longitude: lon }),
                pop: Pop {
                    code,
                    coordinates: Some(Coordinates { latitude: -lat, longitude: -lon }),
                    ..Default::default()
                },
                cargo: crate::VERSION.to_string(),
                ..Default::default()
            };
            let back = parse_meta(&serde_json::to_string(&meta).unwrap()).unwrap();
            prop_assert!(back.mismatch().is_none());
            prop_assert_eq!(back.ip, meta.ip);
            prop_assert_eq!(back.asn, meta.asn);
            prop_assert_eq!(back.city, meta.city);
            prop_assert!(close(back.coordinates, meta.coordinates));
            prop_assert_eq!(back.pop.code, meta.pop.code);
            prop_assert!(close(back.pop.coordinates, meta.pop.coordinates));
        }
    }

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
        // the 90th percentile of 10 and 20 interpolates to 19
        assert!((d.p90.unwrap() - 19.0).abs() < 1e-9);
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
    fn directions() {
        assert_eq!(Direction::Download.plan()[0].bytes, DOWNLOAD_PLAN[0].bytes);
        assert_eq!(Direction::Upload.plan().len(), UPLOAD_PLAN.len());
        let cfg = TestConfig::default();
        assert_eq!(cfg.plans(Direction::Upload).len(), cfg.upload.len());
        let mut r = SpeedtestResults::default();
        assert!(r.direction(Direction::Upload).is_none());
        r.record(Direction::Upload, summarize_direction(&[], &[]));
        assert!(r.direction(Direction::Upload).is_some());
        assert!(r.direction(Direction::Download).is_none());
        assert_eq!(Direction::ALL.map(Direction::name), ["Download", "Upload"]);
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
