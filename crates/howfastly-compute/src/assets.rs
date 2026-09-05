use fastly::Response;
use fastly::http::{StatusCode, header};
use include_dir::{Dir, include_dir};

static DIST: Dir<'_> = include_dir!("$WEB_DIST");

pub fn serve(path: &str) -> Option<Response> {
    let (file, cache) = match path {
        "/" => (DIST.get_file("index.html")?, "no-cache"),
        _ => {
            let name = path.strip_prefix("/assets/")?;
            // the shell lives at the root only, a long lifetime would pin an old build
            if name == "index.html" {
                return None;
            }
            (DIST.get_file(name)?, "public, max-age=31536000, immutable")
        }
    };

    let name = file.path().to_str().unwrap_or_default();
    Some(
        headed(StatusCode::OK, howfastly::http::content_type(name), cache)
            .with_body(file.contents()),
    )
}

// the shell as text, the shared page rewrites its head
pub fn shell() -> Option<&'static str> {
    DIST.get_file("index.html")?.contents_utf8()
}

pub fn headed(status: StatusCode, content_type: &'static str, cache: &str) -> Response {
    Response::from_status(status)
        .with_header(header::CONTENT_TYPE, content_type)
        .with_header(header::CACHE_CONTROL, cache)
        .with_header("alt-svc", "h3=\":443\"; ma=86400")
        .with_header("x-compress-hint", "on")
}
