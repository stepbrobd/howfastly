pub const MAX_DOWN_BYTES: u64 = 1 << 30;

// single-metric header like "app;dur=12.5"
pub fn parse_server_timing(header: &str) -> Option<f64> {
    let dur = header
        .split(';')
        .find_map(|part| part.trim().strip_prefix("dur="))?;
    dur.parse().ok()
}

// missing param means 0 bytes
// unparsable means bad request
pub fn parse_bytes(param: Option<&str>) -> Option<u64> {
    match param {
        None => Some(0),
        Some(s) => s.parse().map(|n: u64| n.min(MAX_DOWN_BYTES)).ok(),
    }
}

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("css") => "text/css",
        Some("html") => "text/html; charset=utf-8",
        Some("ico") => "image/x-icon",
        Some("js") => "text/javascript",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parse_bytes_clamps(n in 0u64..u64::MAX) {
            let parsed = parse_bytes(Some(&n.to_string())).unwrap();
            prop_assert_eq!(parsed, n.min(MAX_DOWN_BYTES));
        }

        #[test]
        fn server_timing_roundtrip(dur in 0.0f64..1e5) {
            let header = format!("app;dur={dur}");
            prop_assert_eq!(parse_server_timing(&header), Some(dur));
        }
    }

    #[test]
    fn parse_bytes_edges() {
        assert_eq!(parse_bytes(None), Some(0));
        assert_eq!(parse_bytes(Some("abc")), None);
        assert_eq!(parse_bytes(Some("-1")), None);
        assert_eq!(parse_bytes(Some("")), None);
    }

    #[test]
    fn server_timing_edges() {
        assert_eq!(parse_server_timing("app;dur=1.5"), Some(1.5));
        assert_eq!(parse_server_timing("nope"), None);
        assert_eq!(parse_server_timing("app;dur=x"), None);
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("web-abc123.js"), "text/javascript");
        assert_eq!(content_type("web-abc123_bg.wasm"), "application/wasm");
        assert_eq!(content_type("style.css"), "text/css");
        assert_eq!(content_type("junk"), "application/octet-stream");
    }
}
