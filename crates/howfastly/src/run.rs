use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, ensure};
use howfastly::http;
use howfastly::share;
use howfastly::stats;
use howfastly::types::{
    Direction, DirectionSummary, LOADED_PING_INTERVAL_MS, MetaResponse, Outcome, Run, SizePlan,
    SizeSamples, SpeedtestResults, Stage, TestConfig, parse_meta, size_label, summarize_direction,
    summarize_latency,
};
use reqwest::{Client, ClientBuilder, Method, RequestBuilder, Response, StatusCode, Version};
use tokio::sync::mpsc;

pub struct Options {
    pub base: String,
    pub local: Option<IpAddr>,
    pub forced: Option<Version>,
    // one direction alone, or both in order
    pub only: Option<Direction>,
    pub verbose: bool,
    // publish a summary through the same connection once the run succeeds
    pub share: bool,
    pub cfg: TestConfig,
}

pub async fn run(opts: &Options) -> Result<SpeedtestResults> {
    let (client, pinned) = connect(&opts.base, opts.local, opts.forced).await?;
    let runner = Runner {
        client,
        pinned,
        base: opts.base.clone(),
        verbose: opts.verbose,
    };

    let (meta, version) = runner.meta().await?;
    if let Some(warning) = meta.mismatch() {
        eprintln!("Warning: {warning}");
    }
    let pop = if meta.pop.name.is_empty() {
        meta.pop.code.clone()
    } else {
        format!("{} {}", meta.pop.code, meta.pop.name)
    };
    eprintln!(
        "Server: POP {pop} ({version:?}) | Client: {} | AS{} {} | {}, {}",
        meta.ip, meta.asn, meta.org, meta.city, meta.country,
    );

    runner.start().await;
    let mut results = SpeedtestResults {
        meta: Some(meta),
        ..Default::default()
    };
    let mut stage = Stage::Latency;
    let outcome = measure(&runner, opts, &mut results, &mut stage).await;
    let finished_at = SystemTime::now();
    let run = Run {
        outcome: match &outcome {
            Ok(()) => Outcome::Completed,
            Err(_) => Outcome::Failed { stage },
        },
        results,
    };
    runner.finish(&run).await;
    outcome?;

    // a failed publication warns and keeps the results
    if opts.share {
        match runner.share(&opts.cfg, &run.results, finished_at).await {
            Ok(shared) => eprintln!(
                "Shared: {}\nExpires: {}",
                shared.url,
                share::utc(shared.expires_at),
            ),
            Err(e) => eprintln!("Warning: sharing failed: {e:#}"),
        }
    }
    Ok(run.results)
}

// the latency phase then each direction, stage follows along for the report
// a direction fails only after every one of its sizes, so the stage is its last
async fn measure(
    runner: &Runner,
    opts: &Options,
    results: &mut SpeedtestResults,
    stage: &mut Stage,
) -> Result<()> {
    let mut pings = Vec::new();
    for i in 0..opts.cfg.latency_samples {
        match runner.ping().await {
            Ok(ms) => pings.push(ms),
            Err(e) => eprintln!("Warning: latency sample {i}: {e}"),
        }
    }
    ensure!(!pings.is_empty(), "All latency samples failed");
    results.latency = summarize_latency(&pings);
    if let Some(l) = &results.latency {
        eprintln!(
            "Latency: Min {:.1} / Median {:.1} / Avg {:.1} / Jitter {:.1} ms",
            l.min, l.median, l.avg, l.jitter,
        );
    }

    let dirs: &[Direction] = match &opts.only {
        Some(dir) => std::slice::from_ref(dir),
        None => &Direction::ALL,
    };
    for &dir in dirs {
        let last = opts
            .cfg
            .plans(dir)
            .last()
            .expect("the cap keeps the smallest size");
        *stage = Stage::Transfer {
            direction: dir,
            bytes: last.bytes,
        };
        let summary = runner.direction(dir, &opts.cfg).await?;
        results.record(dir, summary);
    }
    Ok(())
}

// an unexplained status, such as a proxy page, speaks for itself
fn share_error(status: StatusCode, body: &str) -> anyhow::Error {
    match share::error_message(body) {
        Some(msg) => anyhow!("{status}: {msg}"),
        None => anyhow!("{status}"),
    }
}

// a forced version either connects or fails the run
// with no flag h3 gets probed first since tcp alpn cannot discover it
// probe failure falls back to a default client that negotiates h2 or h1
async fn connect(
    base: &str,
    local: Option<IpAddr>,
    forced: Option<Version>,
) -> Result<(Client, Option<Version>)> {
    // quic carries tls itself, so a plaintext url can never speak h3
    ensure!(
        forced != Some(Version::HTTP_3) || !base.starts_with("http://"),
        "HTTP/3 requires an https URL"
    );

    // only h3 needs per request pinning
    // alpn settles h2 and h1 at the builder
    if let Some(version) = forced {
        let pinned = (version == Version::HTTP_3).then_some(version);
        let client = builder(version).local_address(local).build()?;
        probe(&client, base, pinned)
            .await
            .with_context(|| format!("{version:?} unreachable at {base}"))?;
        return Ok((client, pinned));
    }

    if !base.starts_with("http://")
        && let Ok(client) = builder(Version::HTTP_3).local_address(local).build()
        && probe(&client, base, Some(Version::HTTP_3)).await.is_ok()
    {
        return Ok((client, Some(Version::HTTP_3)));
    }
    Ok((
        Client::builder()
            .user_agent(agent())
            .local_address(local)
            .build()?,
        None,
    ))
}

// the server tells cli runs apart from browsers by this string
fn agent() -> String {
    format!("HowFastly/{}", howfastly::VERSION)
}

fn builder(version: Version) -> ClientBuilder {
    let b = Client::builder().user_agent(agent());
    if version == Version::HTTP_3 {
        b.http3_prior_knowledge()
    } else if version == Version::HTTP_2 {
        b.http2_prior_knowledge()
    } else {
        b.http1_only()
    }
}

// the short timeout turns a blackholed path into a fast failure
// instead of a hang inside the quic handshake
async fn probe(client: &Client, base: &str, pinned: Option<Version>) -> reqwest::Result<Response> {
    let req = client
        .get(format!("{base}/ping"))
        .timeout(Duration::from_secs(2));
    match pinned {
        Some(v) => req.version(v),
        None => req,
    }
    .send()
    .await
}

#[derive(Clone)]
struct Runner {
    client: Client,
    // h3 needs every request pinned, alpn settles the rest
    pinned: Option<Version>,
    base: String,
    verbose: bool,
}

fn server_dur_ms(resp: &Response) -> f64 {
    http::server_dur_ms(
        resp.headers()
            .get("server-timing")
            .and_then(|v| v.to_str().ok()),
    )
}

impl Runner {
    fn req(&self, method: Method, path: &str) -> RequestBuilder {
        let req = self.client.request(method, format!("{}{path}", self.base));
        match self.pinned {
            Some(v) => req.version(v),
            None => req,
        }
    }

    async fn meta(&self) -> Result<(MetaResponse, Version)> {
        let resp = self
            .req(Method::GET, "/meta")
            .send()
            .await
            .context("Service unreachable")?;
        let version = resp.version();
        let body = resp.error_for_status()?.text().await?;
        Ok((parse_meta(&body)?, version))
    }

    // run markers for the edge side counting, the outcome is ignored
    async fn start(&self) {
        let _ = self.req(Method::POST, "/start").send().await;
    }

    async fn finish(&self, run: &Run) {
        let _ = self.req(Method::POST, "/finish").json(run).send().await;
    }

    // summary only, the raw samples stay local
    async fn share(
        &self,
        cfg: &TestConfig,
        results: &SpeedtestResults,
        finished_at: SystemTime,
    ) -> Result<share::ShareResponse> {
        let finished_at = finished_at
            .duration_since(UNIX_EPOCH)
            .context("System clock before the UNIX epoch")?
            .as_secs();
        let payload =
            share::Payload::from_results(share::Client::Cli, finished_at, cfg.clone(), results);
        let resp = self
            .req(Method::POST, "/share")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        match status {
            StatusCode::OK | StatusCode::CREATED => {
                serde_json::from_str(&body).context("Invalid share response")
            }
            _ => Err(share_error(status, &body)),
        }
    }

    async fn ping(&self) -> Result<f64> {
        let start = Instant::now();
        let resp = self.req(Method::GET, "/ping").send().await?;
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        resp.error_for_status_ref()?;
        Ok((elapsed - server_dur_ms(&resp)).max(0.0))
    }

    async fn download(&self, bytes: u64) -> Result<f64> {
        let start = Instant::now();
        let path = format!("/down?bytes={bytes}");
        let mut resp = self
            .req(Method::GET, &path)
            .send()
            .await?
            .error_for_status()?;
        let dur = server_dur_ms(&resp);
        while resp.chunk().await?.is_some() {}
        let secs = (start.elapsed().as_secs_f64() - dur / 1e3).max(1e-9);
        Ok(stats::mbps(bytes, secs))
    }

    async fn upload(&self, bytes: u64) -> Result<f64> {
        let body = vec![0u8; bytes as usize];
        let start = Instant::now();
        let resp = self
            .req(Method::POST, "/up")
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let secs = (start.elapsed().as_secs_f64() - server_dur_ms(&resp) / 1e3).max(1e-9);
        Ok(stats::mbps(bytes, secs))
    }

    async fn sample(&self, dir: Direction, bytes: u64) -> Result<f64> {
        match dir {
            Direction::Download => self.download(bytes).await,
            Direction::Upload => self.upload(bytes).await,
        }
    }

    async fn direction(&self, dir: Direction, cfg: &TestConfig) -> Result<DirectionSummary> {
        let name = dir.name();

        // the pinger sends every sample over a channel until it is aborted
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pinger = tokio::spawn({
            let runner = self.clone();
            async move {
                loop {
                    if let Ok(ms) = runner.ping().await
                        && tx.send(ms).is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(u64::from(LOADED_PING_INTERVAL_MS)))
                        .await;
                }
            }
        });

        // the budget counts completed transfers only, as the web does
        let mut active = Duration::ZERO;
        let mut out = Vec::new();
        for &SizePlan { bytes, iterations } in cfg.plans(dir) {
            let mut s = SizeSamples {
                bytes,
                mbps: Vec::new(),
                skipped: false,
            };
            for i in 0..iterations {
                if active.as_secs_f64() > cfg.time_budget_secs {
                    s.skipped = true;
                    break;
                }
                let start = Instant::now();
                match self.sample(dir, bytes).await {
                    Ok(mbps) => {
                        active += start.elapsed();
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

        pinger.abort();
        let mut loaded_ms = Vec::new();
        while let Ok(ms) = rx.try_recv() {
            loaded_ms.push(ms);
        }

        ensure!(
            out.iter().any(|s| !s.mbps.is_empty()),
            "All {} samples failed",
            name.to_lowercase(),
        );
        Ok(summarize_direction(&out, &loaded_ms))
    }
}
