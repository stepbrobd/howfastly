use howfastly::types::TIME_BUDGET_SECS;

// one sentence per metric, shown as a tooltip
pub const HEADLINE: &str = "90th percentile of throughput over every transfer in this direction";
pub const PEAK: &str = "Highest throughput over any 500ms window";
pub const MEDIAN: &str = "Median throughput of the transfers of one size";
pub const PLOT: &str =
    "The box spans the quartiles, the bar is the median, the ticks are single transfers";
pub const UNLOADED: &str = "Round trip of a small request minus the time the server spent on it";
pub const LOADED: &str =
    "Round trips taken while the transfers ran, the rise over unloaded is bufferbloat";
pub const JITTER: &str = "Mean absolute difference between consecutive round trips";
pub const ROUTE: &str = "Your address and network as Fastly sees them, then the datacenter that served the test and the HTTP version";
pub const PUBLICATION: &str = "The network and datacenter Fastly saw when this result was published, which can differ from the connection that was measured";

pub fn count() -> String {
    format!(
        "Transfers completed out of those planned, a size stops early once the {TIME_BUDGET_SECS} s budget of its direction is spent"
    )
}
