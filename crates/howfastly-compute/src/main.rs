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
    use fastly::http::Method;
    use howfastly::stats::{latency_bucket, speed_bucket};

    let start = Instant::now();
    let mut req = Request::from_client();
    let method = req.get_method().clone();
    let path = req.get_path().to_string();

    let mut results = None;
    match (&method, path.as_str()) {
        (&Method::GET, "/ping") | (&Method::POST, "/start") => {
            handlers::ack(start).send_to_client()
        }
        (&Method::GET, "/down") => handlers::down(&req, start),
        (&Method::POST, "/up") => handlers::up(&mut req).send_to_client(),
        (&Method::GET, "/meta") => handlers::meta(&req, start).send_to_client(),
        (&Method::POST, "/finish") => {
            let (resp, parsed) = handlers::finish(&mut req, start);
            results = parsed;
            resp.send_to_client()
        }
        (_, "/ping" | "/down" | "/up" | "/meta" | "/start" | "/finish") => {
            handlers::method_not_allowed().send_to_client()
        }
        (&Method::GET, p) => match assets::serve(p) {
            Some(resp) => resp.send_to_client(),
            None => handlers::not_found().send_to_client(),
        },
        _ => handlers::not_found().send_to_client(),
    }

    // counted after the response is on the wire so no measurement includes it
    // a run is bracketed by start and finish
    match (method, path.as_str()) {
        (Method::GET, "/") => plausible::event(&req, "pageview", &[]),
        (Method::POST, "/start") => plausible::event(&req, "Start", &[]),
        (Method::POST, "/finish") => {
            let Some(results) = results else {
                return;
            };
            let mut props = Vec::new();
            if let Some(mbps) = results.download.and_then(|d| d.p90) {
                props.push(("Download", speed_bucket(mbps).to_string()));
            }
            if let Some(mbps) = results.upload.and_then(|d| d.p90) {
                props.push(("Upload", speed_bucket(mbps).to_string()));
            }
            if let Some(latency) = results.latency {
                props.push(("Latency", latency_bucket(latency.median).to_string()));
            }
            plausible::event(&req, "Finish", &props)
        }
        _ => {}
    }
}

// hostcalls in the fastly crate cannot link natively
// the stub lets cargo build host integration tests for this package
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
