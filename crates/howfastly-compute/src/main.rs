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
    use plausible::Event;

    let start = Instant::now();
    let mut req = Request::from_client();
    let method = req.get_method().clone();
    let path = req.get_path().to_string();

    // each arm answers the client and says what the request counts as
    let event = match (&method, path.as_str()) {
        (&Method::GET, "/ping") => {
            handlers::ack(start).send_to_client();
            None
        }
        (&Method::POST, "/start") => {
            handlers::ack(start).send_to_client();
            Some(Event::Start)
        }
        (&Method::GET, "/down") => {
            handlers::down(&req, start);
            None
        }
        (&Method::POST, "/up") => {
            handlers::up(&mut req).send_to_client();
            None
        }
        (&Method::GET, "/meta") => {
            handlers::meta(&req, start).send_to_client();
            None
        }
        (&Method::POST, "/finish") => {
            let (resp, results) = handlers::finish(&mut req, start);
            resp.send_to_client();
            results.map(|r| Event::Finish(Box::new(r)))
        }
        (_, "/ping" | "/down" | "/up" | "/meta" | "/start" | "/finish") => {
            handlers::method_not_allowed().send_to_client();
            None
        }
        (&Method::GET, p) => match assets::serve(p) {
            Some(resp) => {
                resp.send_to_client();
                (p == "/").then_some(Event::Pageview)
            }
            None => {
                handlers::not_found().send_to_client();
                None
            }
        },
        _ => {
            handlers::not_found().send_to_client();
            None
        }
    };

    // counted after the response is on the wire so no measurement includes it
    if let Some(event) = event {
        plausible::send(&req, &event);
    }
}

// hostcalls in the fastly crate cannot link natively
// the stub lets cargo build host integration tests for this package
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
