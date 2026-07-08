mod output;
mod run;

use clap::{Parser, ValueEnum};
use common::types::{self, TestConfig};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
    JsonPretty,
    Csv,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PayloadSize {
    #[value(name = "100k")]
    K100,
    #[value(name = "1m")]
    M1,
    #[value(name = "10m")]
    M10,
    #[value(name = "25m")]
    M25,
    #[value(name = "100m")]
    M100,
}

impl PayloadSize {
    pub fn bytes(self) -> u64 {
        match self {
            Self::K100 => 100_000,
            Self::M1 => 1_000_000,
            Self::M10 => 10_000_000,
            Self::M25 => 25_000_000,
            Self::M100 => 100_000_000,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "howfastly",
    about = "HowFastly: speed test for the Fastly network"
)]
pub struct Args {
    #[arg(long, env = "HOWFASTLY_URL", default_value = "http://localhost:7676")]
    pub url: String,

    #[arg(long, default_value_t = types::ITERATIONS)]
    pub nr_tests: usize,

    #[arg(long, default_value_t = types::LATENCY_SAMPLES)]
    pub nr_latency_tests: usize,

    #[arg(long, value_enum, default_value = "100m")]
    pub max_payload_size: PayloadSize,

    #[arg(long, conflicts_with = "upload_only")]
    pub download_only: bool,

    #[arg(long)]
    pub upload_only: bool,

    #[arg(long, value_enum, default_value = "human")]
    pub output_format: OutputFormat,

    #[arg(long, short)]
    pub verbose: bool,
}

impl Args {
    pub fn config(&self) -> TestConfig {
        let cap = self.max_payload_size.bytes();
        let keep = |sizes: &[u64]| sizes.iter().copied().filter(|b| *b <= cap).collect();
        TestConfig {
            latency_samples: self.nr_latency_tests,
            iterations: self.nr_tests,
            download_sizes: keep(&types::DOWNLOAD_SIZES),
            upload_sizes: keep(&types::UPLOAD_SIZES),
            time_budget_secs: types::TIME_BUDGET_SECS,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let results = run::run(&args).await?;
    print!("{}", output::render(&results, args.output_format)?);
    Ok(())
}
