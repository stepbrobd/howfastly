use std::io::{self, Write};
use std::time::Instant;

use fastly::http::{StatusCode, header};
use fastly::{Request, Response};

static CHUNK: [u8; 64 * 1024] = [0x55; 64 * 1024];

fn base(status: StatusCode, start: Instant) -> Response {
    let dur = start.elapsed().as_secs_f64() * 1e3;
    Response::from_status(status)
        .with_header(header::CACHE_CONTROL, "no-store")
        .with_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .with_header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "server-timing")
        .with_header("timing-allow-origin", "*")
        .with_header("server-timing", format!("app;dur={dur:.3}"))
}

pub fn ping(start: Instant) -> Response {
    base(StatusCode::NO_CONTENT, start)
}

pub fn down(req: Request, start: Instant) -> Result<(), fastly::Error> {
    let Some(n) = common::http::parse_bytes(req.get_query_parameter("bytes")) else {
        base(StatusCode::BAD_REQUEST, start).send_to_client();
        return Ok(());
    };

    let resp = base(StatusCode::OK, start)
        .with_header(header::CONTENT_TYPE, "application/octet-stream")
        .with_header(header::CONTENT_LENGTH, n.to_string());

    let mut body = resp.stream_to_client();
    let mut left = n;
    while left > 0 {
        let take = left.min(CHUNK.len() as u64) as usize;
        body.write_all(&CHUNK[..take])?;
        left -= take as u64;
    }
    body.finish()?;
    Ok(())
}

pub fn up(req: Request) -> Response {
    let mut body = req.into_body();
    let received = io::copy(&mut body, &mut io::sink()).unwrap_or(0);
    // dur starts after the drain: receive time is the client's upload
    // measurement, not server overhead
    base(StatusCode::OK, Instant::now()).with_body(received.to_string())
}

pub fn meta(req: &Request, start: Instant) -> Response {
    let ip = req.get_client_ip_addr();
    let geo = ip.and_then(fastly::geo::geo_lookup);

    let meta = common::types::MetaResponse {
        client_ip: ip.map(|ip| ip.to_string()).unwrap_or_default(),
        asn: geo.as_ref().map(|g| g.as_number()).unwrap_or_default(),
        as_org: geo
            .as_ref()
            .map(|g| g.as_name().to_string())
            .unwrap_or_default(),
        city: geo
            .as_ref()
            .map(|g| g.city().to_string())
            .unwrap_or_default(),
        country: geo
            .as_ref()
            .map(|g| g.country_code().to_string())
            .unwrap_or_default(),
        pop: std::env::var("FASTLY_POP").unwrap_or_default(),
        service_version: std::env::var("FASTLY_SERVICE_VERSION").unwrap_or_default(),
    };

    base(StatusCode::OK, start)
        .with_header(header::CONTENT_TYPE, "application/json")
        .with_body(serde_json::to_string(&meta).unwrap_or_default())
}

pub fn not_found() -> Response {
    Response::from_status(StatusCode::NOT_FOUND)
}

pub fn method_not_allowed() -> Response {
    Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
}
