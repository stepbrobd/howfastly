// timestamps are unix seconds, speeds mbps and latencies milliseconds

use serde::{Deserialize, Serialize};

use crate::http::MAX_DOWN_BYTES;
use crate::types::{
    Coordinates, Direction, DirectionSummary, LatencySummary, MetaResponse, Pop, SizePlan,
    SizeSamples, SpeedtestResults, TestConfig, size_label, summarize_direction,
};

pub const FORMAT: u8 = 1;
pub const RETENTION_SECS: u64 = 604_800;
pub const MAX_BYTES: usize = 64 * 1024;
pub const MAX_POINTS: usize = 512;

// the finish time a payload may claim, 2001 to 2100
const FINISHED_AT: std::ops::RangeInclusive<u64> = 1_000_000_000..=4_102_444_800;
const MAX_BUILD_LEN: usize = 64;
const MAX_LATENCY_SAMPLES: usize = 10_000;
const MAX_PLAN_SIZES: usize = 16;
const MAX_ITERATIONS: usize = 1000;
const MAX_BUDGET_SECS: f64 = 3600.0;
// a terabit per second in the unit each field carries
const MAX_MBPS: f64 = 1e6;
const MAX_KBPS: u64 = 1_000_000_000;
// an hour of latency and a day of timeline
const MAX_MS: f64 = 3_600_000.0;
const MAX_TIME_MS: u32 = 86_400_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Web,
    Cli,
}

// time_ms counts the direction's own transfer time and not the wall clock
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Timeline {
    pub time_ms: Vec<u32>,
    pub kbps: Vec<u64>,
}

impl Timeline {
    // chart points of (seconds, bits per second) reduced to at most MAX_POINTS
    pub fn from_points(points: &[(f64, f64)]) -> Option<Timeline> {
        let mut kept: Vec<(u32, u64)> = Vec::new();
        for &(secs, bps) in points {
            if !secs.is_finite() || !bps.is_finite() || secs < 0.0 || bps < 0.0 {
                continue;
            }
            let ms = (secs * 1e3).round();
            let kbps = (bps / 1e3).round();
            if ms > f64::from(MAX_TIME_MS) || kbps > MAX_KBPS as f64 {
                continue;
            }
            let ms = ms as u32;
            if kept.last().is_some_and(|&(last, _)| ms <= last) {
                continue;
            }
            kept.push((ms, kbps as u64));
        }
        if kept.is_empty() {
            return None;
        }
        let (time_ms, kbps) = downsample(kept).into_iter().unzip();
        Some(Timeline { time_ms, kbps })
    }

    // chart points of (seconds, bits per second)
    pub fn points(&self) -> Vec<(f64, f64)> {
        self.time_ms
            .iter()
            .zip(&self.kbps)
            .map(|(&t, &k)| (f64::from(t) / 1e3, k as f64 * 1e3))
            .collect()
    }
}

// keep the first and last points and the peak of each bucket in between
// buckets are index ranges so the order survives and the global peak is one of them
fn downsample(points: Vec<(u32, u64)>) -> Vec<(u32, u64)> {
    let n = points.len();
    if n <= MAX_POINTS {
        return points;
    }
    let inner = &points[1..n - 1];
    let buckets = MAX_POINTS - 2;
    let mut out = Vec::with_capacity(MAX_POINTS);
    out.push(points[0]);
    for b in 0..buckets {
        let from = b * inner.len() / buckets;
        let to = (b + 1) * inner.len() / buckets;
        // the first of equal peaks, so ties resolve the same way every time
        if let Some(&peak) = inner[from..to]
            .iter()
            .min_by_key(|&&(_, kbps)| std::cmp::Reverse(kbps))
        {
            out.push(peak);
        }
    }
    out.push(points[n - 1]);
    out
}

// a summary alone is what the cli sends, the browser adds its samples and timeline
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SharedDirection {
    pub summary: DirectionSummary,
    pub samples: Option<Vec<SizeSamples>>,
    pub timeline: Option<Timeline>,
}

// what a client publishes, finished_at is when the client says the run ended
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Payload {
    pub format: u8,
    pub client: Client,
    pub build: String,
    pub finished_at: u64,
    pub config: TestConfig,
    pub latency: Option<LatencySummary>,
    pub download: Option<SharedDirection>,
    pub upload: Option<SharedDirection>,
}

impl Payload {
    // the meta stays behind, it holds the ip
    pub fn from_results(
        client: Client,
        finished_at: u64,
        config: TestConfig,
        results: &SpeedtestResults,
    ) -> Payload {
        let direction = |summary: &Option<DirectionSummary>| {
            summary.clone().map(|summary| SharedDirection {
                summary,
                samples: None,
                timeline: None,
            })
        };
        Payload {
            format: FORMAT,
            client,
            build: crate::VERSION.to_string(),
            finished_at,
            config,
            latency: results.latency.clone(),
            download: direction(&results.download),
            upload: direction(&results.upload),
        }
    }

    // speeds carry three decimals and latencies one, zero is always positive
    // supplied samples rebuild their summary, the loaded latency stays as sent
    // the error is shown to the client, wall clock checks belong to the server
    pub fn normalize(&mut self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("Result format {} is not supported.", self.format));
        }
        if self.build.is_empty()
            || self.build.len() > MAX_BUILD_LEN
            || !self.build.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(format!(
                "Build must be 1 to {MAX_BUILD_LEN} printable ASCII characters."
            ));
        }
        if !FINISHED_AT.contains(&self.finished_at) {
            return Err("Finish time is out of range.".into());
        }
        normalize_config(&mut self.config)?;
        if let Some(latency) = &mut self.latency {
            normalize_latency(latency, "Latency")?;
        }
        if self.download.is_none() && self.upload.is_none() {
            return Err("Result has neither a download nor an upload.".into());
        }
        let config = &self.config;
        for dir in Direction::ALL {
            let shared = match dir {
                Direction::Download => &mut self.download,
                Direction::Upload => &mut self.upload,
            };
            if let Some(shared) = shared {
                normalize_direction(shared, dir, config.plans(dir))?;
            }
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|e| format!("Result does not serialize: {e}."))?
            .len();
        if bytes > MAX_BYTES {
            return Err(format!(
                "Result is {bytes} bytes, the limit is {} KiB.",
                MAX_BYTES / 1024
            ));
        }
        Ok(())
    }
}

fn check(value: f64, max: f64, what: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 || value > max {
        return Err(format!("{what} is out of range."));
    }
    Ok(())
}

// a negative zero becomes the canonical positive zero
fn round(value: f64, scale: f64) -> f64 {
    let r = (value * scale).round() / scale;
    if r == 0.0 { 0.0 } else { r }
}

fn normalize_config(config: &mut TestConfig) -> Result<(), String> {
    if config.latency_samples > MAX_LATENCY_SAMPLES {
        return Err(format!(
            "Latency sample count exceeds {MAX_LATENCY_SAMPLES}."
        ));
    }
    check(config.time_budget_secs, MAX_BUDGET_SECS, "Time budget")?;
    config.time_budget_secs = round(config.time_budget_secs, 1e3);
    if config.time_budget_secs <= 0.0 {
        return Err("Time budget must be positive.".into());
    }
    for dir in Direction::ALL {
        let name = dir.name();
        let plan = config.plans(dir);
        if plan.len() > MAX_PLAN_SIZES {
            return Err(format!("{name} plan has more than {MAX_PLAN_SIZES} sizes."));
        }
        let mut last = 0;
        for p in plan {
            if p.bytes == 0 || p.bytes > MAX_DOWN_BYTES {
                return Err(format!("{name} plan size {} is out of range.", p.bytes));
            }
            if p.bytes <= last {
                return Err(format!("{name} plan sizes must increase."));
            }
            last = p.bytes;
            if p.iterations == 0 || p.iterations > MAX_ITERATIONS {
                return Err(format!(
                    "{name} plan iterations must be 1 to {MAX_ITERATIONS}."
                ));
            }
        }
    }
    Ok(())
}

fn normalize_latency(latency: &mut LatencySummary, what: &str) -> Result<(), String> {
    for (name, value) in [
        ("minimum", &mut latency.min),
        ("average", &mut latency.avg),
        ("median", &mut latency.median),
        ("jitter", &mut latency.jitter),
    ] {
        check(*value, MAX_MS, &format!("{what} {name}"))?;
        *value = round(*value, 10.0);
    }
    if latency.min > latency.median || latency.min > latency.avg {
        return Err(format!("{what} minimum exceeds its median or average."));
    }
    Ok(())
}

fn normalize_direction(
    shared: &mut SharedDirection,
    dir: Direction,
    plan: &[SizePlan],
) -> Result<(), String> {
    let name = dir.name();
    if plan.is_empty() {
        return Err(format!("{name} has no planned sizes."));
    }
    if let Some(loaded) = &mut shared.summary.loaded {
        normalize_latency(loaded, &format!("{name} loaded latency"))?;
    }
    match &mut shared.samples {
        Some(samples) => {
            normalize_samples(samples, name, plan)?;
            // the summary follows the samples, only the loaded latency is taken as sent
            let loaded = shared.summary.loaded.take();
            let mut summary = summarize_direction(samples, &[]);
            summary.loaded = loaded;
            round_summary(&mut summary);
            shared.summary = summary;
        }
        None => normalize_summary(&mut shared.summary, name, plan)?,
    }
    if shared.summary.p90.is_none() {
        return Err(format!("{name} has no samples."));
    }
    if let Some(timeline) = &shared.timeline {
        check_timeline(timeline, name)?;
    }
    Ok(())
}

// transfers fail and the budget stops a size midway, so fewer samples than planned pass
fn check_count(samples: usize, skipped: bool, plan: &SizePlan, what: &str) -> Result<(), String> {
    if samples > plan.iterations {
        return Err(format!("{what} has more samples than planned."));
    }
    if skipped && samples == plan.iterations {
        return Err(format!("{what} is marked skipped but completed its plan."));
    }
    Ok(())
}

fn normalize_samples(
    samples: &mut [SizeSamples],
    name: &str,
    plan: &[SizePlan],
) -> Result<(), String> {
    if samples.len() != plan.len() {
        return Err(format!(
            "{name} samples cover {} sizes but the plan has {}.",
            samples.len(),
            plan.len()
        ));
    }
    for (s, p) in samples.iter_mut().zip(plan) {
        let what = format!("{name} {}", size_label(p.bytes));
        if s.bytes != p.bytes {
            return Err(format!(
                "{what} samples carry {} instead.",
                size_label(s.bytes)
            ));
        }
        check_count(s.mbps.len(), s.skipped, p, &what)?;
        for v in &mut s.mbps {
            check(*v, MAX_MBPS, &format!("{what} sample"))?;
            *v = round(*v, 1e3);
        }
    }
    Ok(())
}

fn normalize_summary(
    summary: &mut DirectionSummary,
    name: &str,
    plan: &[SizePlan],
) -> Result<(), String> {
    if summary.sizes.len() != plan.len() {
        return Err(format!(
            "{name} summary covers {} sizes but the plan has {}.",
            summary.sizes.len(),
            plan.len()
        ));
    }
    let mut any = false;
    for (s, p) in summary.sizes.iter_mut().zip(plan) {
        let what = format!("{name} {}", size_label(p.bytes));
        if s.bytes != p.bytes {
            return Err(format!(
                "{what} summary carries {} instead.",
                size_label(s.bytes)
            ));
        }
        check_count(s.samples, s.skipped, p, &what)?;
        match &mut s.median {
            Some(median) => {
                if s.samples == 0 {
                    return Err(format!("{what} has a median without samples."));
                }
                check(*median, MAX_MBPS, &format!("{what} median"))?;
                *median = round(*median, 1e3);
            }
            None if s.samples > 0 => return Err(format!("{what} median is missing.")),
            None => {}
        }
        any |= s.samples > 0;
    }
    match &mut summary.p90 {
        Some(p90) => {
            if !any {
                return Err(format!("{name} has a p90 without samples."));
            }
            check(*p90, MAX_MBPS, &format!("{name} p90"))?;
            *p90 = round(*p90, 1e3);
        }
        None if any => return Err(format!("{name} p90 is missing.")),
        None => {}
    }
    Ok(())
}

// percentiles interpolate between rounded samples, so they round once more
fn round_summary(summary: &mut DirectionSummary) {
    if let Some(p90) = &mut summary.p90 {
        *p90 = round(*p90, 1e3);
    }
    for s in &mut summary.sizes {
        if let Some(median) = &mut s.median {
            *median = round(*median, 1e3);
        }
    }
}

fn check_timeline(timeline: &Timeline, name: &str) -> Result<(), String> {
    if timeline.time_ms.len() != timeline.kbps.len() {
        return Err(format!("{name} timeline arrays differ in length."));
    }
    if timeline.time_ms.is_empty() {
        return Err(format!("{name} timeline is empty."));
    }
    if timeline.time_ms.len() > MAX_POINTS {
        return Err(format!(
            "{name} timeline has more than {MAX_POINTS} points."
        ));
    }
    if timeline.time_ms.windows(2).any(|w| w[1] <= w[0]) {
        return Err(format!("{name} timeline times must increase."));
    }
    if timeline.time_ms.iter().any(|&t| t > MAX_TIME_MS) {
        return Err(format!("{name} timeline runs past a day."));
    }
    if timeline.kbps.iter().any(|&k| k > MAX_KBPS) {
        return Err(format!("{name} timeline speed is out of range."));
    }
    Ok(())
}

// what the edge saw of the request that published, not of the connection that measured
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PublicMeta {
    pub asn: u32,
    pub org: String,
    pub city: String,
    pub country: String,
    pub coordinates: Option<Coordinates>,
    pub pop: Pop,
    pub protocol: String,
    pub version: String,
    pub cargo: String,
    // the nix store path of the publishing build, absent outside nix
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
}

impl PublicMeta {
    pub fn from_meta(meta: &MetaResponse) -> PublicMeta {
        PublicMeta {
            asn: meta.asn,
            org: meta.org.clone(),
            city: meta.city.clone(),
            country: meta.country.clone(),
            coordinates: meta.coordinates.map(coarsen),
            pop: meta.pop.clone(),
            protocol: meta.protocol.clone(),
            version: meta.version.clone(),
            cargo: meta.cargo.clone(),
            store: meta.store.clone(),
        }
    }

    // the meta shape the map renders with an empty ip, for presentation and never storage
    pub fn to_meta(&self) -> MetaResponse {
        MetaResponse {
            ip: String::new(),
            asn: self.asn,
            org: self.org.clone(),
            city: self.city.clone(),
            country: self.country.clone(),
            coordinates: self.coordinates,
            pop: self.pop.clone(),
            protocol: self.protocol.clone(),
            version: self.version.clone(),
            cargo: self.cargo.clone(),
            store: self.store.clone(),
        }
    }
}

// a tenth of a degree is about eleven kilometers, a bounded map position and no street
fn coarsen(c: Coordinates) -> Coordinates {
    Coordinates {
        latitude: round(c.latitude, 10.0),
        longitude: round(c.longitude, 10.0),
    }
}

// a stored publication, the server sets published_at and expires_at once at first publication
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub payload: Payload,
    pub publication: PublicMeta,
    pub published_at: u64,
    pub expires_at: u64,
}

impl Report {
    pub fn remaining(&self, now: u64) -> u64 {
        self.expires_at.saturating_sub(now).min(RETENTION_SECS)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShareResponse {
    pub id: String,
    pub url: String,
    pub expires_at: u64,
}

// a share id is the full sha-256 digest as 64 lowercase hex characters
pub fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

// unix seconds as an iso 8601 utc timestamp, 2026-09-05T10:22:07Z
pub fn iso_utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // civil date from days since 1970-01-01, after howard hinnant
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

// the replacements are json string escapes, the text content parses back to the original
// a closing script tag inside a string can no longer end the element
pub fn embed_json(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::summarize_latency;

    fn samples(plan: &[SizePlan]) -> Vec<SizeSamples> {
        plan.iter()
            .map(|p| SizeSamples {
                bytes: p.bytes,
                mbps: vec![100.0; p.iterations],
                skipped: false,
            })
            .collect()
    }

    fn payload() -> Payload {
        let cfg = TestConfig::default();
        let down = samples(&cfg.download);
        let up = samples(&cfg.upload);
        let results = SpeedtestResults {
            meta: Some(MetaResponse {
                ip: "203.0.113.9".into(),
                ..Default::default()
            }),
            latency: summarize_latency(&[1.0, 2.0, 3.0]),
            download: Some(summarize_direction(&down, &[5.0, 6.0])),
            upload: Some(summarize_direction(&up, &[])),
        };
        Payload::from_results(Client::Web, 1_700_000_000, cfg, &results)
    }

    fn canonical(p: &Payload) -> Vec<u8> {
        serde_json::to_vec(p).unwrap()
    }

    fn report() -> Report {
        Report {
            payload: payload(),
            publication: PublicMeta::default(),
            published_at: 100,
            expires_at: 100 + RETENTION_SECS,
        }
    }

    #[test]
    fn from_results_drops_meta_and_sets_format() {
        let p = payload();
        assert_eq!(p.format, FORMAT);
        assert_eq!(p.build, crate::VERSION);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("203.0.113.9"));
        assert!(!json.contains("\"ip\""));
        assert!(json.contains("\"client\":\"web\""));
        assert!(p.download.as_ref().unwrap().samples.is_none());
        assert!(p.download.as_ref().unwrap().timeline.is_none());
    }

    #[test]
    fn rejects_budget_below_wire_precision() {
        let mut p = payload();
        p.config.time_budget_secs = 0.0001;
        assert!(p.normalize().is_err());
    }

    #[test]
    fn normalize_is_idempotent_and_canonicalizes_zero() {
        let mut p = payload();
        p.latency.as_mut().unwrap().jitter = -0.0;
        p.download.as_mut().unwrap().summary.sizes[0].median = Some(12.34567);
        p.normalize().unwrap();
        let once = canonical(&p);
        assert!(!String::from_utf8(once.clone()).unwrap().contains("-0.0"));
        assert_eq!(
            p.download.as_ref().unwrap().summary.sizes[0].median,
            Some(12.346)
        );
        p.normalize().unwrap();
        assert_eq!(canonical(&p), once);
        // a json roundtrip keeps the canonical bytes
        let mut back: Payload = serde_json::from_slice(&once).unwrap();
        assert_eq!(canonical(&back), once);
        back.normalize().unwrap();
        assert_eq!(canonical(&back), once);
    }

    #[test]
    fn samples_rebuild_the_summary_and_keep_loaded_latency() {
        let mut p = payload();
        let plan = p.config.download.clone();
        let mut s = samples(&plan);
        s[0].mbps = vec![10.0, 30.0];
        // the budget can stop a size after some of its transfers
        s[4].mbps = vec![50.0];
        s[4].skipped = true;
        let down = p.download.as_mut().unwrap();
        down.summary.p90 = Some(999.0);
        down.summary.sizes[0].median = Some(999.0);
        down.samples = Some(s);
        p.normalize().unwrap();
        let down = p.download.as_ref().unwrap();
        assert_eq!(down.summary.sizes[0].median, Some(20.0));
        assert_eq!(down.summary.sizes[0].samples, 2);
        assert!(down.summary.sizes[4].skipped);
        assert_eq!(down.summary.sizes[4].samples, 1);
        assert_ne!(down.summary.p90, Some(999.0));
        assert_eq!(down.summary.loaded.as_ref().unwrap().min, 5.0);
    }

    #[test]
    fn rejects_samples_beyond_the_plan() {
        let plan = TestConfig::default().download;

        let mut p = payload();
        let mut s = samples(&plan);
        s[1].mbps.push(1.0);
        p.download.as_mut().unwrap().samples = Some(s);
        assert!(p.normalize().is_err());

        let mut p = payload();
        let mut s = samples(&plan);
        s[1].skipped = true;
        p.download.as_mut().unwrap().samples = Some(s);
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.upload.as_mut().unwrap().summary.sizes.pop();
        assert!(p.normalize().is_err());
    }

    #[test]
    fn cli_subsets_and_custom_iterations_pass() {
        let mut cfg = TestConfig::default();
        cfg.download.truncate(2);
        cfg.upload.truncate(2);
        for p in cfg.download.iter_mut().chain(cfg.upload.iter_mut()) {
            p.iterations = 3;
        }
        let down = samples(&cfg.download);
        let results = SpeedtestResults {
            meta: None,
            latency: summarize_latency(&[4.0]),
            download: Some(summarize_direction(&down, &[])),
            upload: None,
        };
        let mut p = Payload::from_results(Client::Cli, 1_700_000_000, cfg, &results);
        p.normalize().unwrap();
        assert_eq!(p.download.unwrap().summary.sizes.len(), 2);
        assert_eq!(serde_json::to_string(&p.client).unwrap(), "\"cli\"");
    }

    #[test]
    fn rejects_bad_values() {
        let mut p = payload();
        p.format = 2;
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.latency.as_mut().unwrap().min = f64::NAN;
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.download.as_mut().unwrap().summary.p90 = Some(-1.0);
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.build = "a b".into();
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.finished_at = 0;
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.download = None;
        p.upload = None;
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.download.as_mut().unwrap().timeline = Some(Timeline {
            time_ms: vec![0, 100, 100],
            kbps: vec![1, 2, 3],
        });
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.download.as_mut().unwrap().timeline = Some(Timeline::default());
        assert!(p.normalize().is_err());

        let mut p = payload();
        p.config.download[0].iterations = 0;
        assert!(p.normalize().is_err());
    }

    #[test]
    fn timeline_filters_dedupes_and_keeps_peaks() {
        assert!(Timeline::from_points(&[]).is_none());
        assert!(
            Timeline::from_points(&[(f64::NAN, 1.0), (-1.0, 1.0), (1.0, f64::INFINITY)]).is_none()
        );
        let t = Timeline::from_points(&[(0.1, 1e6), (0.1, 2e6), (0.2, 3e6), (0.15, 4e6)]).unwrap();
        assert_eq!(t.time_ms, vec![100, 200]);
        assert_eq!(t.kbps, vec![1000, 3000]);
        assert_eq!(t.points(), vec![(0.1, 1e6), (0.2, 3e6)]);

        let points: Vec<(f64, f64)> = (0..2000)
            .map(|i| (i as f64 * 0.1, if i == 1234 { 9e9 } else { 1e6 }))
            .collect();
        let t = Timeline::from_points(&points).unwrap();
        assert_eq!(t.time_ms.len(), MAX_POINTS);
        assert_eq!(t.kbps.len(), MAX_POINTS);
        assert_eq!(t.time_ms[0], 0);
        assert_eq!(*t.time_ms.last().unwrap(), 199_900);
        assert!(t.kbps.contains(&9_000_000));
        assert!(t.time_ms.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn report_flattens_the_payload() {
        let r = report();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.starts_with("{\"format\":1,"));
        assert!(!json.contains("\"payload\""));
        assert!(!json.contains("\"store\""));
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back.expires_at, r.expires_at);
        assert_eq!(canonical(&back.payload), canonical(&r.payload));
    }

    #[test]
    fn public_meta_coarsens_and_drops_ip() {
        let meta = MetaResponse {
            ip: "203.0.113.9".into(),
            asn: 64496,
            city: "Lyon".into(),
            coordinates: Some(Coordinates {
                latitude: 45.7640,
                longitude: 4.8357,
            }),
            ..Default::default()
        };
        let public = PublicMeta::from_meta(&meta);
        let c = public.coordinates.unwrap();
        assert!((c.latitude - 45.8).abs() < 1e-9);
        assert!((c.longitude - 4.8).abs() < 1e-9);
        assert!(
            !serde_json::to_string(&public)
                .unwrap()
                .contains("203.0.113.9")
        );
        let back = public.to_meta();
        assert_eq!(back.ip, "");
        assert_eq!(back.asn, 64496);
        assert_eq!(back.city, "Lyon");
    }

    #[test]
    fn utc_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_600_000_000), "2020-09-13T12:26:40Z");
        assert_eq!(iso_utc(4_102_444_800), "2100-01-01T00:00:00Z");
    }

    #[test]
    fn embedding_keeps_json_and_closes_no_script() {
        let json = serde_json::to_string(&serde_json::json!({ "org": "</script><b>&" })).unwrap();
        let safe = embed_json(&json);
        assert!(!safe.contains("</script"));
        assert!(!safe.contains('<') && !safe.contains('>') && !safe.contains('&'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&safe).unwrap()["org"],
            "</script><b>&"
        );
        assert_eq!(escape_html("a<b>&\"c'"), "a&lt;b&gt;&amp;&quot;c&#39;");
    }
}
