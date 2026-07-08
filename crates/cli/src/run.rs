use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use common::http::parse_server_timing;
use common::stats;
use common::types::{
    DirectionSummary, LOADED_PING_INTERVAL_MS, MetaResponse, SizeSamples, SpeedtestResults,
    TestConfig, size_label, summarize_direction, summarize_latency,
};
use reqwest::{Client, Response};

use crate::Args;

pub async fn run(args: &Args) -> Result<SpeedtestResults> {
    let runner = Runner {
        client: Client::new(),
        base: args.url.trim_end_matches('/').to_string(),
        verbose: args.verbose,
    };
    let cfg = args.config();

    let meta = runner.meta().await.context("Service unreachable")?;
    eprintln!(
        "Server: POP {} | Client: {} | AS{} {} | {}, {}",
        meta.pop, meta.client_ip, meta.asn, meta.as_org, meta.city, meta.country,
    );

    let mut results = SpeedtestResults {
        meta: Some(meta),
        ..Default::default()
    };

    let mut pings = Vec::new();
    for i in 0..cfg.latency_samples {
        match runner.ping().await {
            Ok(ms) => pings.push(ms),
            Err(e) => eprintln!("Warning: latency sample {i}: {e}"),
        }
    }
    ensure!(!pings.is_empty(), "All latency samples failed");
    results.latency = summarize_latency(&pings);
    if let Some(l) = &results.latency {
        eprintln!(
            "Latency: min {:.1} / med {:.1} / avg {:.1} / jitter {:.1} ms",
            l.min_ms, l.median_ms, l.avg_ms, l.jitter_ms,
        );
    }

    if !args.upload_only {
        results.download = Some(runner.direction(false, &cfg).await?);
    }
    if !args.download_only {
        results.upload = Some(runner.direction(true, &cfg).await?);
    }
    Ok(results)
}

#[derive(Clone)]
struct Runner {
    client: Client,
    base: String,
    verbose: bool,
}

fn server_dur_ms(resp: &Response) -> f64 {
    resp.headers()
        .get("server-timing")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_server_timing)
        .unwrap_or(0.0)
}

impl Runner {
    async fn meta(&self) -> Result<MetaResponse> {
        let resp = self
            .client
            .get(format!("{}/meta", self.base))
            .send()
            .await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    async fn ping(&self) -> Result<f64> {
        let start = Instant::now();
        let resp = self
            .client
            .get(format!("{}/ping", self.base))
            .send()
            .await?;
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        resp.error_for_status_ref()?;
        Ok((elapsed - server_dur_ms(&resp)).max(0.0))
    }

    async fn download(&self, bytes: u64) -> Result<f64> {
        let start = Instant::now();
        let url = format!("{}/down?bytes={bytes}", self.base);
        let mut resp = self.client.get(url).send().await?.error_for_status()?;
        let dur = server_dur_ms(&resp);
        while resp.chunk().await?.is_some() {}
        let secs = (start.elapsed().as_secs_f64() - dur / 1e3).max(1e-9);
        Ok(stats::mbps(bytes, secs))
    }

    async fn upload(&self, bytes: u64) -> Result<f64> {
        let body = vec![0u8; bytes as usize];
        let start = Instant::now();
        let url = format!("{}/up", self.base);
        let resp = self
            .client
            .post(url)
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let secs = (start.elapsed().as_secs_f64() - server_dur_ms(&resp) / 1e3).max(1e-9);
        Ok(stats::mbps(bytes, secs))
    }

    async fn sample(&self, upload: bool, bytes: u64) -> Result<f64> {
        if upload {
            self.upload(bytes).await
        } else {
            self.download(bytes).await
        }
    }

    async fn direction(&self, upload: bool, cfg: &TestConfig) -> Result<DirectionSummary> {
        let name = if upload { "Upload" } else { "Download" };
        let stop = Arc::new(AtomicBool::new(false));
        let loaded = Arc::new(Mutex::new(Vec::new()));

        let pinger = tokio::spawn({
            let runner = self.clone();
            let stop = stop.clone();
            let loaded = loaded.clone();
            async move {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(ms) = runner.ping().await {
                        loaded.lock().unwrap().push(ms);
                    }
                    tokio::time::sleep(Duration::from_millis(LOADED_PING_INTERVAL_MS)).await;
                }
            }
        });

        let phase_start = Instant::now();
        let sizes = if upload {
            &cfg.upload_sizes
        } else {
            &cfg.download_sizes
        };
        let mut out = Vec::new();
        for &bytes in sizes {
            let mut s = SizeSamples {
                bytes,
                mbps: Vec::new(),
                skipped: false,
            };
            for i in 0..cfg.iterations {
                if phase_start.elapsed().as_secs_f64() > cfg.time_budget_secs {
                    s.skipped = true;
                    break;
                }
                match self.sample(upload, bytes).await {
                    Ok(mbps) => {
                        if self.verbose {
                            eprintln!("{name} {} sample {i}: {mbps:.2} Mbps", size_label(bytes));
                        }
                        s.mbps.push(mbps);
                    }
                    Err(e) => eprintln!("Warning: {name} {} sample {i}: {e}", size_label(bytes)),
                }
            }
            eprintln!(
                "{name} {}: {} Mbps ({} samples{})",
                size_label(bytes),
                stats::median(&s.mbps)
                    .map(|m| format!("{m:.2}"))
                    .unwrap_or_else(|| "-".into()),
                s.mbps.len(),
                if s.skipped { ", budget hit" } else { "" },
            );
            out.push(s);
        }

        stop.store(true, Ordering::Relaxed);
        let _ = pinger.await;
        let loaded_ms = loaded.lock().unwrap().clone();

        ensure!(
            out.iter().any(|s| !s.mbps.is_empty()),
            "All {} samples failed",
            name.to_lowercase(),
        );
        Ok(summarize_direction(&out, &loaded_ms))
    }
}
