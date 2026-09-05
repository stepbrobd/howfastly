pub mod http;
pub mod share;
pub mod stats;
pub mod types;

// the workspace version, shared by compute, cli, and web builds
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
