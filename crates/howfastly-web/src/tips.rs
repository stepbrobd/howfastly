// one sentence per metric, shown as a tooltip
pub const HEADLINE: &str = "90th percentile of throughput over every transfer in this direction";
pub const PEAK: &str = "Highest throughput over any 500ms window";
pub const MEDIAN: &str = "Median throughput of the transfers of one size";
pub const COUNT: &str = "Transfers completed out of those planned, a size stops early once the 30s budget of its direction is spent";
pub const PLOT: &str =
    "The box spans the quartiles, the bar is the median, the ticks are single transfers";
pub const UNLOADED: &str = "Round trip of a small request minus the time the server spent on it";
pub const LOADED: &str =
    "Round trips taken while the transfers ran, the rise over unloaded is bufferbloat";
pub const JITTER: &str = "Mean absolute difference between consecutive round trips";
pub const ROUTE: &str = "Your address and network as Fastly sees them, then the datacenter that served the test and the HTTP version";
