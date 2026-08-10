#[cfg(target_arch = "wasm32")]
mod assets;
#[cfg(target_arch = "wasm32")]
mod handlers;

#[cfg(target_arch = "wasm32")]
fn main() {
    use std::time::Instant;

    use fastly::Request;
    use fastly::http::Method;

    let start = Instant::now();
    let req = Request::from_client();
    let method = req.get_method().clone();
    let path = req.get_path().to_string();

    match (method, path.as_str()) {
        (Method::GET, "/ping") => handlers::ping(start).send_to_client(),
        (Method::GET, "/down") => handlers::down(req, start),
        (Method::POST, "/up") => handlers::up(req).send_to_client(),
        (Method::GET, "/meta") => handlers::meta(&req, start).send_to_client(),
        (_, "/ping" | "/down" | "/up" | "/meta") => handlers::method_not_allowed().send_to_client(),
        (Method::GET, p) => match assets::serve(p) {
            Some(resp) => resp.send_to_client(),
            None => handlers::not_found().send_to_client(),
        },
        _ => handlers::not_found().send_to_client(),
    }
}

// hostcalls in the fastly crate cannot link natively
// the stub lets cargo build host integration tests for this package
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
