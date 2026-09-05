use std::cell::RefCell;
use std::rc::Rc;

use howfastly::http;
use howfastly::stats;
use howfastly::types::{MetaResponse, SpeedtestResults, parse_meta};
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortSignal, Headers, ProgressEvent, ReadableStreamDefaultReader, ReadableStreamReadResult,
    RequestInit, Response, Window, XmlHttpRequest,
};

fn window() -> Window {
    web_sys::window().expect("no window")
}

pub fn now_ms() -> f64 {
    window().performance().expect("no performance").now()
}

const AUTOSTART_KEY: &str = "howfastly-autostart";

pub fn autostart_saved() -> bool {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item(AUTOSTART_KEY).ok().flatten())
        .is_some()
}

pub fn save_autostart() {
    if let Ok(Some(s)) = window().local_storage() {
        let _ = s.set_item(AUTOSTART_KEY, "1");
    }
}

fn request(
    method: &str,
    json: Option<&str>,
    signal: Option<&AbortSignal>,
) -> Result<RequestInit, JsValue> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_signal(signal);
    if let Some(json) = json {
        let headers = Headers::new()?;
        headers.set("content-type", "application/json")?;
        init.set_headers_headers(&headers);
        init.set_body_opt_str(Some(json));
    }
    Ok(init)
}

async fn send(url: &str, init: &RequestInit) -> Result<Response, JsValue> {
    let resp = JsFuture::from(window().fetch_with_str_and_init(url, init)).await?;
    resp.dyn_into()
}

async fn fetch(
    method: &str,
    url: &str,
    json: Option<&str>,
    signal: Option<&AbortSignal>,
) -> Result<Response, JsValue> {
    send(url, &request(method, json, signal)?).await
}

// status and body of an exchange whose answer the caller reads either way
async fn exchange(method: &str, url: &str, json: Option<&str>) -> Result<(u16, String), JsValue> {
    let resp = fetch(method, url, json, None).await?;
    let status = resp.status();
    let text = JsFuture::from(resp.text()?).await?;
    Ok((status, text.as_string().unwrap_or_default()))
}

// run markers for the edge side counting, the outcome is ignored
pub async fn start() {
    let _ = fetch("POST", "/start", None, None).await;
}

pub async fn finish(results: &SpeedtestResults) {
    let Ok(json) = serde_json::to_string(results) else {
        return;
    };
    let Ok(init) = request("POST", Some(&json), None) else {
        return;
    };
    // a closing tab must still deliver the report
    let _ = Reflect::set(&init, &"keepalive".into(), &JsValue::TRUE);
    let _ = send("/finish", &init).await;
}

pub async fn share(json: &str) -> Result<(u16, String), JsValue> {
    exchange("POST", "/share", Some(json)).await
}

pub async fn report(id: &str) -> Result<(u16, String), JsValue> {
    exchange("GET", &format!("/share/{id}.json"), None).await
}

pub fn describe(e: JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

pub fn unix_secs() -> u64 {
    (js_sys::Date::now() / 1e3) as u64
}

pub fn pathname() -> String {
    window().location().pathname().unwrap_or_default()
}

pub fn embedded(id: &str) -> Option<String> {
    window().document()?.get_element_by_id(id)?.text_content()
}

// the write is issued before the future is awaited
// navigator.clipboard is absent outside secure contexts and the typed getter would trap there
pub fn copy(text: &str) -> JsFuture {
    let clipboard = Reflect::get(&window().navigator(), &"clipboard".into())
        .and_then(|value| value.dyn_into::<web_sys::Clipboard>());
    JsFuture::from(match clipboard {
        Ok(clipboard) => clipboard.write_text(text),
        Err(error) => js_sys::Promise::reject(&error),
    })
}

fn server_dur_ms(resp: &Response) -> f64 {
    http::server_dur_ms(
        resp.headers()
            .get("server-timing")
            .ok()
            .flatten()
            .as_deref(),
    )
}

pub async fn ping() -> Result<f64, JsValue> {
    let start = now_ms();
    let resp = fetch("GET", "/ping", None, None).await?;
    Ok((now_ms() - start - server_dur_ms(&resp)).max(0.0))
}

async fn drain(resp: &Response, on_progress: &mut impl FnMut(f64, u64)) -> Result<(), JsValue> {
    let stream = resp.body().ok_or_else(|| JsValue::from_str("no body"))?;
    let reader: ReadableStreamDefaultReader = stream.get_reader().dyn_into()?;
    let mut total = 0u64;
    loop {
        let chunk: ReadableStreamReadResult = JsFuture::from(reader.read()).await?.unchecked_into();
        if chunk.get_done() == Some(true) {
            return Ok(());
        }
        let value: Uint8Array = chunk.get_value().dyn_into()?;
        total += u64::from(value.length());
        on_progress(now_ms(), total);
    }
}

// the signal aborts the transfer, which then returns an error
pub async fn download(
    bytes: u64,
    mut on_progress: impl FnMut(f64, u64),
    signal: &AbortSignal,
) -> Result<f64, JsValue> {
    let start = now_ms();
    let resp = fetch("GET", &format!("/down?bytes={bytes}"), None, Some(signal)).await?;
    drain(&resp, &mut on_progress).await?;
    let secs = ((now_ms() - start - server_dur_ms(&resp)) / 1e3).max(1e-9);
    Ok(stats::mbps(bytes, secs))
}

pub async fn upload(
    bytes: u64,
    mut on_progress: impl FnMut(f64, u64) + 'static,
    signal: &AbortSignal,
) -> Result<f64, JsValue> {
    let xhr = XmlHttpRequest::new()?;
    xhr.open("POST", "/up")?;

    let onabort = Closure::<dyn FnMut()>::new({
        let xhr = xhr.clone();
        move || {
            let _ = xhr.abort();
        }
    });
    signal.set_onabort(Some(onabort.as_ref().unchecked_ref()));

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

    let len = u32::try_from(bytes).map_err(|_| JsValue::from_str("upload size exceeds u32"))?;
    let body = Uint8Array::new_with_length(len);
    let start = now_ms();
    xhr.send_with_opt_buffer_source(Some(&body))?;
    JsFuture::from(promise).await?;
    signal.set_onabort(None);

    let status = xhr.status().unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(JsValue::from_str(&format!(
            "upload failed with status {status}"
        )));
    }
    let server_ms = http::server_dur_ms(
        xhr.get_response_header("server-timing")
            .ok()
            .flatten()
            .as_deref(),
    );
    let secs = ((now_ms() - start - server_ms) / 1e3).max(1e-9);
    Ok(stats::mbps(bytes, secs))
}

pub async fn meta() -> Result<MetaResponse, JsValue> {
    let resp = fetch("GET", "/meta", None, None).await?;
    let text = JsFuture::from(resp.text()?).await?;
    parse_meta(&text.as_string().unwrap_or_default()).map_err(|e| JsValue::from_str(&e.to_string()))
}
