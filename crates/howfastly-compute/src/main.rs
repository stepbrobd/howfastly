#[cfg(target_arch = "wasm32")]
mod assets;
#[cfg(target_arch = "wasm32")]
mod handlers;
#[cfg(target_arch = "wasm32")]
mod plausible;

#[cfg(target_arch = "wasm32")]
fn main() {
    use std::time::Instant;

    use fastly::http::Method;
    use fastly::{Request, Response};
    use plausible::Event;

    let start = Instant::now();
    let mut req = Request::from_client();
    let path = req.get_path().to_string();
    // a head request takes the get path and its answer keeps the body home
    let head = req.get_method() == Method::HEAD;
    let method = if head {
        Method::GET
    } else {
        req.get_method().clone()
    };
    let send = |resp: Response| {
        if head {
            resp.with_body("").send_to_client();
        } else {
            resp.send_to_client();
        }
    };

    // each arm answers the client and says what the request counts as
    let event = match (&method, path.as_str()) {
        (&Method::GET, "/ping") => {
            send(handlers::ack(start));
            None
        }
        (&Method::POST, "/start") => {
            send(handlers::ack(start));
            Some(Event::Start)
        }
        (&Method::GET, "/down") => {
            handlers::down(&req, start, head);
            None
        }
        (&Method::POST, "/up") => {
            send(handlers::up(&mut req));
            None
        }
        (&Method::GET, "/meta") => {
            send(handlers::meta(&req, start));
            None
        }
        (&Method::POST, "/finish") => {
            let (resp, results) = handlers::finish(&mut req, start);
            send(resp);
            results.map(|r| Event::Finish(Box::new(r)))
        }
        (_, "/ping" | "/down" | "/meta") => {
            send(handlers::method_not_allowed("GET, HEAD"));
            None
        }
        (_, "/start" | "/up" | "/finish") => {
            send(handlers::method_not_allowed("POST"));
            None
        }
        (&Method::GET, p) => match assets::serve(p) {
            Some(resp) => {
                send(resp);
                (p == "/" && !head).then_some(Event::Pageview)
            }
            None => {
                send(handlers::not_found());
                None
            }
        },
        _ => {
            send(handlers::not_found());
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
