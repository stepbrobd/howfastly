use std::io::Read;
use std::net::{IpAddr, Ipv6Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fastly::erl::{ERLError, Penaltybox};
use fastly::http::{StatusCode, header};
use fastly::kv_store::{InsertMode, KVStore, KVStoreError};
use fastly::{Request, Response};
use howfastly::share::{
    self, Client, FORMAT, MAX_BYTES, Payload, PublicMeta, RETENTION_SECS, Report, ShareResponse,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{assets, handlers};

// the key under the store is the id itself
const STORE: &str = "kvstore";
// hashed ahead of the canonical payload, changes with the format
const DOMAIN: &[u8] = b"howfastly-share-v1\n";
const CREATION_WINDOW_SECS: u64 = 86_400;
// every publication holds one entry of this penalty box for the slot ttl
// the box is the memory of one pop and bounds one source, a fleet of addresses is not its job
const BOX: &str = "share";
const SLOTS: u32 = 3;
const SLOT_TTL: Duration = Duration::from_secs(15 * 60);

// a status and the explanation the client shows
type Reject = (StatusCode, String);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_secs()
}

// a span of whole days in words, 1 day or 7 days
fn days(secs: u64) -> String {
    match secs / 86_400 {
        1 => "1 day".to_string(),
        n => format!("{n} days"),
    }
}

// the full digest as 64 lowercase hex characters, never truncated
fn id_of(canonical: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(canonical);
    format!("{:x}", hasher.finalize())
}

fn unavailable(cause: impl std::fmt::Display) -> Reject {
    eprintln!("sharing unavailable, {cause}");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Sharing is unavailable right now, try again later.".into(),
    )
}

fn open() -> Result<KVStore, String> {
    match KVStore::open(STORE) {
        Ok(Some(store)) => Ok(store),
        Ok(None) => Err(format!("kv store {STORE} is not linked")),
        Err(e) => Err(format!("kv store {STORE} failed to open, {e}")),
    }
}

fn json_body(req: &Request) -> bool {
    req.get_header(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|ct| ct.split(';').next())
        .is_some_and(|ct| ct.trim().eq_ignore_ascii_case("application/json"))
}

// no cors or timing headers, those serve the measurement routes
fn reply(status: StatusCode, cache: &str) -> Response {
    Response::from_status(status)
        .with_header(header::CONTENT_TYPE, "application/json")
        .with_header(header::CACHE_CONTROL, cache)
}

fn error(status: StatusCode, message: &str) -> Response {
    let resp = reply(status, "no-store").with_body(json!({ "error": message }).to_string());
    if status != StatusCode::TOO_MANY_REQUESTS {
        return resp;
    }
    // the box frees a slot on the minute after its ttl, so the wait is one minute past it
    resp.with_header(header::RETRY_AFTER, (SLOT_TTL.as_secs() + 60).to_string())
}

// the address as the limiter keys it, a v4 address whole and a v6 address by its /64
fn source(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => format!("{}/64", Ipv6Addr::from(u128::from(v6) & (u128::MAX << 64))),
        },
    }
}

fn take(pb: &Penaltybox, key: &str) -> Result<bool, ERLError> {
    for slot in 0..SLOTS {
        let entry = format!("{key}#{slot}");
        if !pb.has(&entry)? {
            pb.add(&entry, SLOT_TTL)?;
            return Ok(true);
        }
    }
    Ok(false)
}

// a slot is never given back, a rejected body consumed one too
// the limiter is a defense and not a dependency, so a failed hostcall admits and says so
fn admit(req: &Request) -> Result<(), Reject> {
    let Some(ip) = req.get_client_ip_addr() else {
        return Ok(());
    };
    match take(&Penaltybox::open(BOX), &source(ip)) {
        Ok(true) => Ok(()),
        Ok(false) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "Your address shared {SLOTS} results in the last {} min, try again later.",
                SLOT_TTL.as_secs() / 60
            ),
        )),
        Err(e) => {
            eprintln!("rate limit unavailable, {e}");
            Ok(())
        }
    }
}

// the stored record under this id when it is readable, unexpired and hashes to the id
// missing or expired is none, a record that hashes elsewhere or cannot be read rejects
fn existing(store: &KVStore, id: &str, now: u64) -> Result<Option<Report>, Reject> {
    let mut found = match store.lookup(id) {
        Ok(found) => found,
        Err(KVStoreError::ItemNotFound) => return Ok(None),
        Err(e) => return Err(unavailable(e)),
    };
    let report: Report = serde_json::from_slice(&found.take_body_bytes())
        .map_err(|e| unavailable(format!("record {id} unreadable, {e}")))?;
    if report.expires_at <= now {
        return Ok(None);
    }
    let canonical = serde_json::to_vec(&report.payload).map_err(unavailable)?;
    if id_of(&canonical) != id {
        return Err((
            StatusCode::CONFLICT,
            "A different result already holds this link.".into(),
        ));
    }
    Ok(Some(report))
}

// a repeated payload answers 200 with its first publication, a new one 201
fn create(req: &mut Request) -> Result<(StatusCode, ShareResponse), Reject> {
    admit(req)?;
    if !json_body(req) {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Publish a result as application/json.".into(),
        ));
    }
    // one byte past the cap tells an oversized body from one that fits exactly
    let mut buf = Vec::new();
    req.take_body()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "The result body could not be read.".to_string(),
            )
        })?;
    if buf.len() > MAX_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("A result may not exceed {} KiB.", MAX_BYTES / 1024),
        ));
    }
    let mut payload: Payload = serde_json::from_slice(&buf).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("The result JSON is invalid: {e}."),
        )
    })?;
    if payload.format != FORMAT {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "Result format {} is not supported, this build publishes format {FORMAT}.",
                payload.format
            ),
        ));
    }
    payload
        .normalize()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let canonical = serde_json::to_vec(&payload).map_err(unavailable)?;
    let id = id_of(&canonical);
    let url = handlers::url_at(req.get_url(), &format!("/share/{id}"));
    let now = now();
    let store = open().map_err(unavailable)?;

    // a retry reads the first publication without another write to the same key
    if let Some(report) = existing(&store, &id, now)? {
        return Ok((
            StatusCode::OK,
            ShareResponse {
                id,
                url,
                expires_at: report.expires_at,
            },
        ));
    }
    if now.abs_diff(payload.finished_at) > CREATION_WINDOW_SECS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "A result can only be published within {} of finishing.",
                days(CREATION_WINDOW_SECS)
            ),
        ));
    }

    let report = Report {
        payload,
        publication: PublicMeta::from_meta(&handlers::lookup_meta(req)),
        published_at: now,
        expires_at: now + RETENTION_SECS,
    };
    let record = serde_json::to_vec(&report).map_err(unavailable)?;
    if record.len() > MAX_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("A stored result may not exceed {} KiB.", MAX_BYTES / 1024),
        ));
    }
    // add never touches an existing key, so the first publication keeps its context and expiry
    let insert = store
        .build_insert()
        .mode(InsertMode::Add)
        .time_to_live(Duration::from_secs(RETENTION_SECS))
        .execute(&id, record);
    match insert {
        Ok(()) => Ok((
            StatusCode::CREATED,
            ShareResponse {
                id,
                url,
                expires_at: report.expires_at,
            },
        )),
        Err(KVStoreError::ItemPreconditionFailed) => match existing(&store, &id, now)? {
            Some(found) => Ok((
                StatusCode::OK,
                ShareResponse {
                    id,
                    url,
                    expires_at: found.expires_at,
                },
            )),
            // the key exists but its record is not readable yet, no success to claim
            None => Err(unavailable(format!(
                "record {id} missing after an add conflict"
            ))),
        },
        Err(KVStoreError::TooManyRequests) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Sharing is busy right now, try again later.".into(),
        )),
        Err(e) => Err(unavailable(e)),
    }
}

// the id comes back when a record was created, a reuse counts nothing
pub fn publish(req: &mut Request) -> (Response, Option<String>) {
    match create(req) {
        Ok((status, share)) => {
            let body = serde_json::to_string(&share).expect("share response serializes");
            let created = status == StatusCode::CREATED;
            (
                reply(status, "no-store").with_body(body),
                created.then_some(share.id),
            )
        }
        Err((status, message)) => (error(status, &message), None),
    }
}

// why a read has nothing to show
enum Miss {
    Unknown,
    Expired,
    Unsupported(u64),
    Unavailable,
}

impl Miss {
    fn status(&self) -> StatusCode {
        match self {
            Self::Unknown | Self::Expired => StatusCode::NOT_FOUND,
            Self::Unsupported(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::Unknown => "Result not found",
            Self::Expired => "Result expired",
            Self::Unsupported(_) => "Result format not supported",
            Self::Unavailable => "Sharing unavailable",
        }
    }

    fn explain(&self) -> String {
        let kept = days(RETENTION_SECS);
        match self {
            Self::Unknown => format!(
                "No shared result lives at this link. Shared results stay available for {kept} after publication."
            ),
            Self::Expired => format!(
                "This shared result expired. Shared results stay available for {kept} after publication."
            ),
            Self::Unsupported(format) => format!(
                "This result was published in format {format}, which this build cannot show."
            ),
            Self::Unavailable => {
                "The shared result could not be read right now, try again later.".into()
            }
        }
    }
}

// a store or read failure never reads as a missing record
fn load(id: &str, now: u64) -> Result<Report, Miss> {
    if !share::valid_id(id) {
        return Err(Miss::Unknown);
    }
    let store = open().map_err(|cause| {
        eprintln!("sharing unavailable, {cause}");
        Miss::Unavailable
    })?;
    let mut found = match store.lookup(id) {
        Ok(found) => found,
        Err(KVStoreError::ItemNotFound) => return Err(Miss::Unknown),
        Err(e) => {
            eprintln!("record {id} lookup failed, {e}");
            return Err(Miss::Unavailable);
        }
    };
    let bytes = found.take_body_bytes();
    let report: Report = match serde_json::from_slice(&bytes) {
        Ok(report) => report,
        Err(e) => {
            // a record of another format is a version gap, anything else is corrupt
            let format = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("format")?.as_u64());
            return Err(match format {
                Some(format) if format != u64::from(FORMAT) => Miss::Unsupported(format),
                _ => {
                    eprintln!("record {id} unreadable, {e}");
                    Miss::Unavailable
                }
            });
        }
    };
    if report.payload.format != FORMAT {
        return Err(Miss::Unsupported(u64::from(report.payload.format)));
    }
    if report.expires_at <= now {
        return Err(Miss::Expired);
    }
    Ok(report)
}

pub fn json(id: &str) -> Response {
    let now = now();
    match load(id, now) {
        Ok(report) => {
            let fresh = format!(
                "public, max-age={}, immutable, must-revalidate",
                report.remaining(now)
            );
            reply(StatusCode::OK, &fresh)
                .with_header("x-robots-tag", "noindex")
                .with_body(serde_json::to_string(&report).expect("report serializes"))
        }
        Err(miss) => error(miss.status(), &miss.explain()),
    }
}

// the shell around a live record, a plain page for anything else
pub fn page(req: &Request, id: &str) -> Response {
    let now = now();
    let miss = match load(id, now) {
        Ok(report) => match shell(
            &report,
            &handlers::url_at(req.get_url(), &format!("/share/{id}")),
        ) {
            Some(html) => return document(StatusCode::OK, "no-cache").with_body(html),
            None => {
                eprintln!("app shell has no head, the shared page cannot render");
                Miss::Unavailable
            }
        },
        Err(miss) => miss,
    };
    document(miss.status(), "no-store").with_body(plain_page(miss.title(), &miss.explain()))
}

// a share is reached by its link and stays out of search indexes
fn document(status: StatusCode, cache: &str) -> Response {
    assets::headed(status, "text/html; charset=utf-8", cache).with_header("x-robots-tag", "noindex")
}

// the embedded json is what the app renders, the tags are what link previews read
// the tags close the head so the shell's charset meta stays first
fn shell(report: &Report, url: &str) -> Option<String> {
    let html = assets::shell()?;
    let at = html.find("</head>")?;
    let title = share::escape_html(&headline(report));
    let description = share::escape_html(&summary(report));
    let url = share::escape_html(url);
    let json = share::embed_json(&serde_json::to_string(report).expect("report serializes"));
    let tags = [
        format!("<title>{title}</title>"),
        format!("<meta name=\"description\" content=\"{description}\" />"),
        format!("<meta property=\"og:title\" content=\"{title}\" />"),
        format!("<meta property=\"og:description\" content=\"{description}\" />"),
        "<meta property=\"og:type\" content=\"website\" />".to_string(),
        format!("<meta property=\"og:url\" content=\"{url}\" />"),
        "<meta name=\"twitter:card\" content=\"summary\" />".to_string(),
        format!("<script id=\"howfastly-report\" type=\"application/json\">{json}</script>"),
    ];
    let (before, after) = html.split_at(at);
    let before = strip(
        &strip(before, "<title>", "<title>", "</title>"),
        "name=\"description\"",
        "<meta",
        ">",
    );
    let mut out = String::with_capacity(html.len() + json.len() + 1024);
    out.push_str(before.trim_end());
    for tag in &tags {
        out.push_str("\n    ");
        out.push_str(tag);
    }
    out.push_str("\n  ");
    out.push_str(after);
    Some(out)
}

// the text without the element around the first marker, open precedes it and close follows
fn strip(html: &str, marker: &str, open: &str, close: &str) -> String {
    let Some(at) = html.find(marker) else {
        return html.to_string();
    };
    let Some(from) = html[..at + marker.len()].rfind(open) else {
        return html.to_string();
    };
    let Some(len) = html[at..].find(close) else {
        return html.to_string();
    };
    format!("{}{}", &html[..from], &html[at + len + close.len()..])
}

fn speed(mbps: f64) -> String {
    if mbps >= 1000.0 {
        format!("{:.2} Gbps", mbps / 1000.0)
    } else {
        format!("{mbps:.1} Mbps")
    }
}

fn headline(report: &Report) -> String {
    let p = &report.payload;
    let mut parts = Vec::new();
    if let Some(mbps) = p.download.as_ref().and_then(|d| d.summary.p90) {
        parts.push(format!("{} down", speed(mbps)));
    }
    if let Some(mbps) = p.upload.as_ref().and_then(|d| d.summary.p90) {
        parts.push(format!("{} up", speed(mbps)));
    }
    if let Some(latency) = &p.latency {
        parts.push(format!("{:.1} ms latency", latency.median));
    }
    if parts.is_empty() {
        "HowFastly shared result".into()
    } else {
        format!("HowFastly: {}", parts.join(", "))
    }
}

// the publication context in words, what was observed when the link was made
fn summary(report: &Report) -> String {
    let p = &report.payload;
    let m = &report.publication;
    let client = match p.client {
        Client::Web => "web",
        Client::Cli => "CLI",
    };
    let place = [m.city.as_str(), m.country.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let network = if m.asn == 0 {
        String::new()
    } else if m.org.is_empty() {
        format!("AS{}", m.asn)
    } else {
        format!("AS{} {}", m.asn, m.org)
    };
    let from = match (place.is_empty(), network.is_empty()) {
        (false, false) => format!(" from {place} ({network})"),
        (false, true) => format!(" from {place}"),
        (true, false) => format!(" from {network}"),
        (true, true) => String::new(),
    };
    let through = if m.pop.code.is_empty() {
        String::new()
    } else if m.pop.name.is_empty() {
        format!(" through Fastly POP {}", m.pop.code)
    } else {
        format!(" through Fastly POP {} {}", m.pop.code, m.pop.name)
    };
    let over = if m.protocol.is_empty() {
        String::new()
    } else {
        format!(" over {}", m.protocol)
    };
    format!(
        "Measured {} with the HowFastly {client} client {}. Published {}{from}{through}{over}, the link expires {}.",
        share::utc(p.finished_at),
        p.build,
        share::utc(report.published_at),
        share::utc(report.expires_at),
    )
}

// a small standalone page, the app never boots on an error
fn plain_page(title: &str, message: &str) -> String {
    let title = share::escape_html(title);
    let message = share::escape_html(message);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en-US\">\n<head>\n<meta charset=\"utf-8\" />\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n<title>HowFastly: {title}</title>\n</head>\n<body>\n<main>\n<h1>{title}</h1>\n<p>{message}</p>\n<p><a href=\"/\">Run your own test</a></p>\n</main>\n</body>\n</html>\n"
    )
}
