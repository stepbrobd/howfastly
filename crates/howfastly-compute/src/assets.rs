use fastly::Response;
use fastly::http::{StatusCode, header};
use include_dir::{Dir, include_dir};

static DIST: Dir<'_> = include_dir!("$WEB_DIST");

pub fn serve(path: &str) -> Option<Response> {
    let (file, cache) = match path {
        "/" => (DIST.get_file("index.html")?, "no-cache"),
        _ => (
            DIST.get_file(path.strip_prefix("/assets/")?)?,
            "public, max-age=31536000, immutable",
        ),
    };

    let name = file.path().to_str().unwrap_or_default();
    Some(
        Response::from_status(StatusCode::OK)
            .with_header(header::CONTENT_TYPE, howfastly::http::content_type(name))
            .with_header(header::CACHE_CONTROL, cache)
            .with_header("alt-svc", "h3=\":443\"; ma=86400")
            .with_header("x-compress-hint", "on")
            .with_body(file.contents()),
    )
}
