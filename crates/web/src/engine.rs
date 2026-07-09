use std::cell::RefCell;
use std::rc::Rc;

use common::http::parse_server_timing;
use common::stats;
use common::types::MetaResponse;
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    ProgressEvent, ReadableStreamDefaultReader, RequestInit, Response, Window, XmlHttpRequest,
};

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

async fn drain(resp: &Response, on_progress: &mut impl FnMut(f64, u64)) -> Result<(), JsValue> {
    let stream = resp.body().ok_or_else(|| JsValue::from_str("no body"))?;
    let reader: ReadableStreamDefaultReader = stream.get_reader().dyn_into()?;
    let mut total = 0u64;
    loop {
        let chunk = JsFuture::from(reader.read()).await?;
        if Reflect::get(&chunk, &"done".into())?.is_truthy() {
            return Ok(());
        }
        let value: Uint8Array = Reflect::get(&chunk, &"value".into())?.dyn_into()?;
        total += u64::from(value.length());
        on_progress(now_ms(), total);
    }
}

pub async fn download(bytes: u64, mut on_progress: impl FnMut(f64, u64)) -> Result<f64, JsValue> {
    let start = now_ms();
    let resp = fetch("GET", &format!("/down?bytes={bytes}"), None).await?;
    drain(&resp, &mut on_progress).await?;
    let secs = ((now_ms() - start - server_dur_ms(&resp)) / 1e3).max(1e-9);
    Ok(stats::mbps(bytes, secs))
}

pub async fn upload(
    bytes: u64,
    mut on_progress: impl FnMut(f64, u64) + 'static,
) -> Result<f64, JsValue> {
    let xhr = XmlHttpRequest::new()?;
    xhr.open("POST", "/up")?;

    let resolve = Rc::new(RefCell::new(None::<js_sys::Function>));
    let promise = js_sys::Promise::new(&mut |res, _| {
        *resolve.borrow_mut() = Some(res);
    });

    let onprogress = Closure::<dyn FnMut(ProgressEvent)>::new(move |e: ProgressEvent| {
        on_progress(now_ms(), e.loaded() as u64);
    });
    xhr.upload()?
        .set_onprogress(Some(onprogress.as_ref().unchecked_ref()));

    let onloadend = Closure::<dyn FnMut(ProgressEvent)>::new({
        let resolve = resolve.clone();
        move |_: ProgressEvent| {
            if let Some(f) = resolve.borrow_mut().take() {
                let _ = f.call0(&JsValue::NULL);
            }
        }
    });
    xhr.set_onloadend(Some(onloadend.as_ref().unchecked_ref()));

    let body = Uint8Array::new_with_length(bytes as u32);
    let start = now_ms();
    xhr.send_with_opt_buffer_source(Some(&body))?;
    JsFuture::from(promise).await?;

    let status = xhr.status().unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(JsValue::from_str(&format!(
            "upload failed with status {status}"
        )));
    }
    let server_ms = xhr
        .get_response_header("server-timing")
        .ok()
        .flatten()
        .and_then(|h| parse_server_timing(&h))
        .unwrap_or(0.0);
    let secs = ((now_ms() - start - server_ms) / 1e3).max(1e-9);
    Ok(stats::mbps(bytes, secs))
}

pub async fn meta() -> Result<MetaResponse, JsValue> {
    let resp = fetch("GET", "/meta", None).await?;
    let text = JsFuture::from(resp.text()?).await?;
    serde_json::from_str(&text.as_string().unwrap_or_default())
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
