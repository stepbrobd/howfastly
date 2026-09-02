use anyhow::Result;
use howfastly::types::{Direction, DirectionSummary, SpeedtestResults};

use crate::OutputFormat;

pub fn render(results: &SpeedtestResults, format: OutputFormat) -> Result<String> {
    Ok(match format {
        OutputFormat::Json => serde_json::to_string_pretty(results)? + "\n",
        OutputFormat::Csv => csv(results),
        OutputFormat::Human => human(results),
    })
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.2}")).unwrap_or_default()
}

fn csv(r: &SpeedtestResults) -> String {
    let mut out = String::from("direction,bytes,samples,median,p90\n");
    for dir in Direction::ALL {
        let Some(d) = r.direction(dir) else { continue };
        let name = dir.name().to_lowercase();
        for s in &d.sizes {
            out += &format!(
                "{name},{},{},{},{}\n",
                s.bytes,
                s.samples,
                fmt_opt(s.median),
                fmt_opt(d.p90),
            );
        }
    }
    out
}

// progress already streamed each measurement to stderr
// only summarize what is new (p90 headline and loaded latency)
fn direction_block(name: &str, d: &DirectionSummary) -> String {
    let mut out = format!("{name}: {} Mbps (p90)\n", fmt_opt(d.p90));
    if let Some(l) = &d.loaded {
        out += &format!(
            "{name} loaded latency: Median {:.1} ms / Jitter {:.1} ms\n",
            l.median, l.jitter,
        );
    }
    out
}

fn human(r: &SpeedtestResults) -> String {
    let mut out = String::from("\n");
    for dir in Direction::ALL {
        if let Some(d) = r.direction(dir) {
            out += &direction_block(dir.name(), d);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use howfastly::types::*;

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
