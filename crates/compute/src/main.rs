mod assets;
mod handlers;

use std::time::Instant;

use fastly::http::Method;
use fastly::{Error, Request};

fn main() -> Result<(), Error> {
    let start = Instant::now();
    let req = Request::from_client();
    let method = req.get_method().clone();
    let path = req.get_path().to_string();

    match (method, path.as_str()) {
        (Method::GET, "/ping") => handlers::ping(start).send_to_client(),
        (Method::GET, "/down") => return handlers::down(req, start),
        (Method::POST, "/up") => handlers::up(req).send_to_client(),
        (Method::GET, "/meta") => handlers::meta(&req, start).send_to_client(),
        (_, "/ping" | "/down" | "/up" | "/meta") => handlers::method_not_allowed().send_to_client(),
        (Method::GET, p) => match assets::serve(p) {
            Some(resp) => resp.send_to_client(),
            None => handlers::not_found().send_to_client(),
        },
        _ => handlers::not_found().send_to_client(),
    }

    Ok(())
}
