use common::http::parse_server_timing;
use common::stats;
use common::types::MetaResponse;
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStreamDefaultReader, RequestInit, Response, Window};

fn window() -> Window {
    web_sys::window().expect("no window")
}

pub fn now_ms() -> f64 {
    window().performance().expect("no performance").now()
}

async fn fetch(method: &str, url: &str, body: Option<Uint8Array>) -> Result<Response, JsValue> {
    let init = RequestInit::new();
    init.set_method(method);
    if let Some(body) = body {
        init.set_body(&JsValue::from(body));
    }
    let resp = JsFuture::from(window().fetch_with_str_and_init(url, &init)).await?;
    resp.dyn_into()
}

fn server_dur_ms(resp: &Response) -> f64 {
    resp.headers()
        .get("server-timing")
        .ok()
        .flatten()
        .and_then(|h| parse_server_timing(&h))
        .unwrap_or(0.0)
}

pub async fn ping() -> Result<f64, JsValue> {
    let start = now_ms();
    let resp = fetch("GET", "/ping", None).await?;
    Ok((now_ms() - start - server_dur_ms(&resp)).max(0.0))
}

async fn drain(resp: &Response) -> Result<(), JsValue> {
    let stream = resp.body().ok_or_else(|| JsValue::from_str("no body"))?;
    let reader: ReadableStreamDefaultReader = stream.get_reader().dyn_into()?;
    loop {
        let chunk = JsFuture::from(reader.read()).await?;
        if Reflect::get(&chunk, &"done".into())?.is_truthy() {
            return Ok(());
        }
    }
}

pub async fn download(bytes: u64) -> Result<f64, JsValue> {
    let start = now_ms();
    let resp = fetch("GET", &format!("/down?bytes={bytes}"), None).await?;
    drain(&resp).await?;
    let secs = ((now_ms() - start - server_dur_ms(&resp)) / 1e3).max(1e-9);
    Ok(stats::mbps(bytes, secs))
}

pub async fn upload(bytes: u64) -> Result<f64, JsValue> {
    let body = Uint8Array::new_with_length(bytes as u32);
    let start = now_ms();
    let resp = fetch("POST", "/up", Some(body)).await?;
    let secs = ((now_ms() - start - server_dur_ms(&resp)) / 1e3).max(1e-9);
    Ok(stats::mbps(bytes, secs))
}

pub async fn meta() -> Result<MetaResponse, JsValue> {
    let resp = fetch("GET", "/meta", None).await?;
    let text = JsFuture::from(resp.text()?).await?;
    serde_json::from_str(&text.as_string().unwrap_or_default())
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
