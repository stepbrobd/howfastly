#[cfg(target_arch = "wasm32")]
mod assets;
#[cfg(target_arch = "wasm32")]
mod handlers;
#[cfg(target_arch = "wasm32")]
mod plausible;

#[cfg(target_arch = "wasm32")]
fn main() {
    use std::time::Instant;

    use fastly::Request;
    use fastly::http::{Method, header};
    use howfastly::http::parse_bytes;
    use howfastly::types::size_label;

    let start = Instant::now();
    let mut req = Request::from_client();
    let method = req.get_method().clone();
    let path = req.get_path().to_string();

    match (&method, path.as_str()) {
        (&Method::GET, "/ping") => handlers::ping(start).send_to_client(),
        (&Method::GET, "/down") => handlers::down(&req, start),
        (&Method::POST, "/up") => handlers::up(&mut req).send_to_client(),
        (&Method::GET, "/meta") => handlers::meta(&req, start).send_to_client(),
        (_, "/ping" | "/down" | "/up" | "/meta") => handlers::method_not_allowed().send_to_client(),
        (&Method::GET, p) => match assets::serve(p) {
            Some(resp) => resp.send_to_client(),
            None => handlers::not_found().send_to_client(),
        },
        _ => handlers::not_found().send_to_client(),
    }

    // counted only once the response is on the wire, so no measurement
    // includes it, pings are too chatty to count at all
    let label = |bytes: Option<u64>| bytes.map(size_label).unwrap_or_default();
    match (method, path.as_str()) {
        (Method::GET, "/") => plausible::event(&req, "pageview", &[]),
        (Method::GET, "/meta") => plausible::event(&req, "Meta", &[]),
        (Method::GET, "/down") => {
            let bytes = parse_bytes(req.get_query_parameter("bytes"));
            plausible::event(&req, "Download", &[("Bytes", label(bytes))])
        }
        (Method::POST, "/up") => {
            let bytes = req
                .get_header(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok());
            plausible::event(&req, "Upload", &[("Bytes", label(bytes))])
        }
        _ => {}
    }
}

// hostcalls in the fastly crate cannot link natively
// the stub lets cargo build host integration tests for this package
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
