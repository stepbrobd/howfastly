mod output;
mod run;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use clap::{Parser, ValueEnum};
use howfastly::types::{self, Direction, SizePlan, TestConfig};
use reqwest::Version;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
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
    about = concat!(
        "HowFastly ",
        env!("CARGO_PKG_VERSION"),
        "\nHow fast is your connection to the Fastly network?"
    )
)]
pub struct Args {
    /// Service to test against
    #[arg(
        long,
        short = 'U',
        env = "HOWFASTLY_URL",
        default_value = "https://speed.edgecompute.app"
    )]
    pub url: String,

    // flat override for the per size iteration plan
    /// Transfers per size in place of the per size plan
    #[arg(long, short)]
    pub nr_tests: Option<usize>,

    /// Unloaded latency samples before the transfers
    #[arg(long, short = 'l', default_value_t = types::LATENCY_SAMPLES)]
    pub nr_latency_tests: usize,

    /// Largest transfer size, larger sizes leave the plan
    #[arg(long, short, value_enum, default_value = "100m")]
    pub max_payload_size: PayloadSize,

    /// Measure the download direction only
    #[arg(long, short, conflicts_with = "upload_only")]
    pub download_only: bool,

    /// Measure the upload direction only
    #[arg(long, short = 'u')]
    pub upload_only: bool,

    // bare flag binds the family's unspecified address
    // the kernel then picks the default outbound address at connect time
    /// Connect over IPv4, from a given local address or any
    #[arg(long, value_name = "ADDR", num_args = 0..=1, default_missing_value = "0.0.0.0", conflicts_with = "ipv6")]
    pub ipv4: Option<Ipv4Addr>,

    /// Connect over IPv6, from a given local address or any
    #[arg(long, value_name = "ADDR", num_args = 0..=1, default_missing_value = "::")]
    pub ipv6: Option<Ipv6Addr>,

    // force one protocol instead of probing h3 then negotiating
    // an unreachable forced version fails the run rather than falling back
    /// Force HTTP/1.1, an unreachable version fails the run
    #[arg(long, group = "http_version")]
    pub http1: bool,

    /// Force HTTP/2, an unreachable version fails the run
    #[arg(long, group = "http_version")]
    pub http2: bool,

    /// Force HTTP/3, an unreachable version fails the run
    #[arg(long, group = "http_version")]
    pub http3: bool,

    /// Output format for the results on stdout
    #[arg(long, short, value_enum, default_value = "human")]
    pub format: OutputFormat,

    /// Print every transfer sample as it completes
    #[arg(long, short)]
    pub verbose: bool,

    /// Publish the result summary and print the link on stderr
    #[arg(long, short)]
    pub share: bool,
}

impl Args {
    pub fn local_addr(&self) -> Option<IpAddr> {
        self.ipv4.map(IpAddr::V4).or(self.ipv6.map(IpAddr::V6))
    }

    // clap rejects both flags together
    pub fn only(&self) -> Option<Direction> {
        if self.download_only {
            Some(Direction::Download)
        } else if self.upload_only {
            Some(Direction::Upload)
        } else {
            None
        }
    }

    pub fn options(&self) -> run::Options {
        run::Options {
            base: self.url.trim_end_matches('/').to_string(),
            local: self.local_addr(),
            forced: self.http_version(),
            only: self.only(),
            verbose: self.verbose,
            share: self.share,
            cfg: self.config(),
        }
    }

    // clap rejects more than one flag in the group
    pub fn http_version(&self) -> Option<Version> {
        if self.http1 {
            Some(Version::HTTP_11)
        } else if self.http2 {
            Some(Version::HTTP_2)
        } else if self.http3 {
            Some(Version::HTTP_3)
        } else {
            None
        }
    }

    pub fn config(&self) -> TestConfig {
        let cap = self.max_payload_size.bytes();
        let keep = |plan: &[SizePlan]| {
            plan.iter()
                .copied()
                .filter(|p| p.bytes <= cap)
                .map(|p| SizePlan {
                    iterations: self.nr_tests.unwrap_or(p.iterations),
                    ..p
                })
                .collect()
        };
        TestConfig {
            latency_samples: self.nr_latency_tests,
            download: keep(&types::DOWNLOAD_PLAN),
            upload: keep(&types::UPLOAD_PLAN),
            time_budget_secs: types::TIME_BUDGET_SECS,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let results = run::run(&args.options()).await?;
    print!("{}", output::render(&results, args.format)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    // catches duplicate shorts and malformed arg definitions
    #[test]
    fn cli() {
        Args::command().debug_assert();
    }

    #[test]
    fn shorts() {
        let a = Args::try_parse_from([
            "howfastly",
            "-U",
            "http://x",
            "-n",
            "3",
            "-l",
            "5",
            "-m",
            "1m",
            "-d",
            "-f",
            "json",
            "-s",
        ])
        .unwrap();
        assert_eq!(a.url, "http://x");
        assert!(a.share);
        assert_eq!(a.nr_tests, Some(3));
        assert_eq!(a.nr_latency_tests, 5);
        assert!(matches!(a.max_payload_size, PayloadSize::M1));
        assert_eq!(a.only(), Some(Direction::Download));
        assert!(matches!(a.format, OutputFormat::Json));

        let a = Args::try_parse_from(["howfastly", "-u"]).unwrap();
        assert_eq!(a.only(), Some(Direction::Upload));
        assert_eq!(Args::try_parse_from(["howfastly"]).unwrap().only(), None);
        assert!(Args::try_parse_from(["howfastly", "-d", "-u"]).is_err());
        assert!(Args::try_parse_from(["howfastly", "-f", "json-pretty"]).is_err());
    }

    // the flag values name sizes the download plan really has
    #[test]
    fn payload_sizes_follow_the_plan() {
        for size in PayloadSize::value_variants() {
            assert!(types::DOWNLOAD_PLAN.iter().any(|p| p.bytes == size.bytes()));
        }
    }

    #[test]
    fn http_flags() {
        let a = Args::try_parse_from(["howfastly"]).unwrap();
        assert_eq!(a.http_version(), None);

        let a = Args::try_parse_from(["howfastly", "--http1"]).unwrap();
        assert_eq!(a.http_version(), Some(Version::HTTP_11));
        let a = Args::try_parse_from(["howfastly", "--http2"]).unwrap();
        assert_eq!(a.http_version(), Some(Version::HTTP_2));
        let a = Args::try_parse_from(["howfastly", "--http3"]).unwrap();
        assert_eq!(a.http_version(), Some(Version::HTTP_3));

        assert!(Args::try_parse_from(["howfastly", "--http1", "--http2"]).is_err());
        assert!(Args::try_parse_from(["howfastly", "--http1", "--http3"]).is_err());
        assert!(Args::try_parse_from(["howfastly", "--http2", "--http3"]).is_err());
    }

    #[test]
    fn ip_binding() {
        let a = Args::try_parse_from(["howfastly"]).unwrap();
        assert_eq!(a.local_addr(), None);

        let a = Args::try_parse_from(["howfastly", "--ipv4"]).unwrap();
        assert_eq!(a.local_addr(), Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        let a = Args::try_parse_from(["howfastly", "--ipv4", "192.0.2.7"]).unwrap();
        assert_eq!(
            a.local_addr(),
            Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)))
        );

        let a = Args::try_parse_from(["howfastly", "--ipv6"]).unwrap();
        assert_eq!(a.local_addr(), Some(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        let a = Args::try_parse_from(["howfastly", "--ipv6", "2001:db8::1"]).unwrap();
        assert_eq!(a.local_addr(), Some("2001:db8::1".parse().unwrap()));

        assert!(Args::try_parse_from(["howfastly", "--ipv4", "--ipv6"]).is_err());
        assert!(Args::try_parse_from(["howfastly", "--ipv4", "::1"]).is_err());
        assert!(Args::try_parse_from(["howfastly", "--ipv6", "127.0.0.1"]).is_err());
    }
}
