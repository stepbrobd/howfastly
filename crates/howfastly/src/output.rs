use anyhow::Result;
use common::types::{DirectionSummary, SpeedtestResults};

use crate::OutputFormat;

pub fn render(results: &SpeedtestResults, format: OutputFormat) -> Result<String> {
    Ok(match format {
        OutputFormat::Json => serde_json::to_string(results)? + "\n",
        OutputFormat::JsonPretty => serde_json::to_string_pretty(results)? + "\n",
        OutputFormat::Csv => csv(results),
        OutputFormat::Human => human(results),
    })
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.2}")).unwrap_or_default()
}

fn csv(r: &SpeedtestResults) -> String {
    let mut out = String::from("direction,size_bytes,samples,median_mbps,p90_mbps\n");
    for (name, dir) in [("download", &r.download), ("upload", &r.upload)] {
        let Some(d) = dir else { continue };
        for s in &d.sizes {
            out += &format!(
                "{name},{},{},{},{}\n",
                s.bytes,
                s.samples,
                fmt_opt(s.median_mbps),
                fmt_opt(d.p90_mbps),
            );
        }
    }
    out
}

// progress already streamed each measurement to stderr
// only summarize what is new (p90 headline and loaded latency)
fn direction_block(name: &str, d: &DirectionSummary) -> String {
    let mut out = format!("{name}: {} Mbps (p90)\n", fmt_opt(d.p90_mbps));
    if let Some(l) = &d.loaded_latency {
        out += &format!(
            "{name} loaded latency: Median {:.1} ms / Jitter {:.1} ms\n",
            l.median_ms, l.jitter_ms,
        );
    }
    out
}

fn human(r: &SpeedtestResults) -> String {
    let mut out = String::from("\n");
    for (name, dir) in [("Download", &r.download), ("Upload", &r.upload)] {
        if let Some(d) = dir {
            out += &direction_block(name, d);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::*;

    fn results() -> SpeedtestResults {
        SpeedtestResults {
            meta: Some(MetaResponse::default()),
            latency: summarize_latency(&[1.0, 2.0, 3.0]),
            download: Some(summarize_direction(
                &[SizeSamples {
                    bytes: 100_000,
                    mbps: vec![50.0],
                    skipped: false,
                }],
                &[5.0, 6.0],
            )),
            upload: None,
        }
    }

    #[test]
    fn json_roundtrips() {
        let s = render(&results(), crate::OutputFormat::Json).unwrap();
        let back: SpeedtestResults = serde_json::from_str(&s).unwrap();
        assert_eq!(back.download.unwrap().sizes.len(), 1);
    }

    #[test]
    fn csv_has_header_and_rows() {
        let s = render(&results(), crate::OutputFormat::Csv).unwrap();
        let lines: Vec<_> = s.lines().collect();
        assert!(lines[0].starts_with("direction,"));
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn human_mentions_sections() {
        let s = render(&results(), crate::OutputFormat::Human).unwrap();
        assert!(s.contains("latency"));
        assert!(s.contains("Download"));
        assert!(!s.contains("Upload:"));
    }
}
