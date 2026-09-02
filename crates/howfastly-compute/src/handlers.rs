use std::io::{Read, Write};
use std::time::{Duration, Instant};

use fastly::cache::simple::{self, CacheEntry};
use fastly::http::{StatusCode, Version, header};
use fastly::{Request, Response};

static CHUNK: [u8; 64 * 1024] = [0x55; 64 * 1024];

const SECRET_STORE: &str = "secretstore";
const API_KEY: &str = "fastly-api-key";
const API_BACKEND: &str = "fastly";

// resolve the serving pop against the datacenters api
// the simple cache keeps the response body in this pop for a day
// the error names the step that failed so the log says why meta degraded
fn pop_info(code: &str) -> Result<howfastly::types::Pop, &'static str> {
    if code.is_empty() {
        return Err("pop code empty");
    }
    let store = fastly::secret_store::SecretStore::open(SECRET_STORE)
        .map_err(|_| "secret store missing")?;
    let secret = store
        .try_get(API_KEY)
        .map_err(|_| "secret store unreadable")?
        .ok_or("api key missing")?;
    let plaintext = secret.try_plaintext().map_err(|_| "api key unreadable")?;
    let api_key = std::str::from_utf8(&plaintext)
        .map_err(|_| "api key not utf8")?
        .trim()
        .to_string();

    let body = simple::get_or_set_with("datacenters", || {
        let resp = Request::get("https://api.fastly.com/datacenters")
            .with_header("fastly-key", api_key)
            .send(API_BACKEND)?;
        if resp.get_status() != StatusCode::OK {
            return Err(fastly::Error::msg("datacenters request failed"));
        }
        Ok(CacheEntry {
            value: resp.into_body(),
            ttl: Duration::from_secs(86_400),
        })
    })
    .map_err(|_| "datacenters unreachable")?
    .ok_or("datacenters cache empty")?;

    let pops: Vec<howfastly::types::Pop> =
        serde_json::from_reader(body).map_err(|_| "datacenters body invalid")?;
    pops.into_iter()
        .find(|pop| pop.code.eq_ignore_ascii_case(code))
        .ok_or("pop unknown to the api")
}

fn base(status: StatusCode, start: Instant) -> Response {
    let dur = start.elapsed().as_secs_f64() * 1e3;
    Response::from_status(status)
        .with_header(header::CACHE_CONTROL, "no-store")
        .with_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .with_header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "server-timing")
        .with_header("timing-allow-origin", "*")
        .with_header("server-timing", format!("app;dur={dur:.3}"))
}

pub fn ack(start: Instant) -> Response {
    base(StatusCode::NO_CONTENT, start)
}

// a finish report is analytics only
// anything unparsable or oversized is a bad request and counts nothing
pub fn finish(
    req: &mut Request,
    start: Instant,
) -> (Response, Option<howfastly::types::SpeedtestResults>) {
    let mut buf = Vec::new();
    let read = req
        .take_body()
        .take(64 * 1024)
        .read_to_end(&mut buf)
        .is_ok();
    let results = read.then(|| serde_json::from_slice(&buf).ok()).flatten();
    let status = match results {
        Some(_) => StatusCode::NO_CONTENT,
        None => StatusCode::BAD_REQUEST,
    };
    (base(status, start), results)
}

pub fn down(req: &Request, start: Instant) {
    let Some(n) = howfastly::http::parse_bytes(req.get_query_parameter("bytes")) else {
        base(StatusCode::BAD_REQUEST, start).send_to_client();
        return;
    };

    let resp = base(StatusCode::OK, start)
        .with_header(header::CONTENT_TYPE, "application/octet-stream")
        .with_header(header::CONTENT_LENGTH, n.to_string());

    let mut body = resp.stream_to_client();
    let mut left = n;
    while left > 0 {
        let take = left.min(CHUNK.len() as u64) as usize;
        if body.write_all(&CHUNK[..take]).is_err() {
            // a client hanging up mid-transfer is normal for a speed test
            return;
        }
        left -= take as u64;
    }
    let _ = body.finish();
}

pub fn up(req: &mut Request) -> Response {
    // drain with large reads
    // viceroy rebuffers the unread remainder on every read
    // small buffers therefore make big uploads quadratic
    let mut body = req.take_body();
    let mut buf = vec![0u8; 2 * 1024 * 1024];
    let mut received: u64 = 0;
    loop {
        match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => received += n as u64,
            Err(_) => return base(StatusCode::BAD_REQUEST, Instant::now()),
        }
    }
    // dur starts after the drain
    // receive time belongs to the client's upload measurement
    base(StatusCode::OK, Instant::now())
        .with_header(header::CONTENT_TYPE, "text/plain")
        .with_body(received.to_string())
}

// the wire name of the version, empty for one this build does not know
pub fn protocol(req: &Request) -> &'static str {
    match req.get_version() {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "",
    }
}

pub fn meta(req: &Request, start: Instant) -> Response {
    let ip = req.get_client_ip_addr();
    let geo = ip.and_then(fastly::geo::geo_lookup);
    let code = fastly::compute_runtime::pop();
    let pop = pop_info(code).unwrap_or_else(|cause| {
        eprintln!("pop lookup degraded to the bare code: {cause}");
        howfastly::types::Pop {
            code: code.to_string(),
            ..Default::default()
        }
    });

    let meta = howfastly::types::MetaResponse {
        ip: ip.map(|ip| ip.to_string()).unwrap_or_default(),
        asn: geo.as_ref().map(|g| g.as_number()).unwrap_or_default(),
        org: geo
            .as_ref()
            .map(|g| howfastly::types::title_case(g.as_name()))
            .unwrap_or_default(),
        city: geo
            .as_ref()
            .map(|g| howfastly::types::title_case(g.city()))
            .unwrap_or_default(),
        country: geo
            .as_ref()
            .map(|g| g.country_code().to_string())
            .unwrap_or_default(),
        // an unknown position reads as the null island
        coordinates: geo
            .as_ref()
            .map(|g| howfastly::types::Coordinates {
                latitude: g.latitude(),
                longitude: g.longitude(),
            })
            .filter(|c| c.latitude != 0.0 || c.longitude != 0.0),
        pop,
        protocol: protocol(req).to_string(),
        version: std::env::var("FASTLY_SERVICE_VERSION").unwrap_or_default(),
        cargo: howfastly::VERSION.to_string(),
    };

    base(StatusCode::OK, start)
        .with_header(header::CONTENT_TYPE, "application/json")
        .with_body(serde_json::to_string(&meta).expect("meta serializes"))
}

pub fn not_found() -> Response {
    Response::from_status(StatusCode::NOT_FOUND)
}

pub fn method_not_allowed() -> Response {
    Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
}
