//! Sends a usage statistics request with `fetch` (`solidcraft_ui_egui::usage_stats`): to its
//! endpoints only (`allowed_url`), without cookies or credentials. The answer comes on the channel; the page never
//! waits on it.

use std::sync::mpsc::Sender;

use js_sys::Uint8Array;
use solidcraft_ui_egui::agent_chat::backend::HttpRequest;
use solidcraft_ui_egui::tickets::Reply;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::files::err;

pub fn post(r: HttpRequest, done: Sender<Reply>) {
    spawn_local(async move {
        let reply =
            if solidcraft_ui_egui::usage_stats::allowed_url(&r.url) { send(&r).await } else { Err("not the usage statistics endpoint".into()) };
        let _ = done.send(reply);
    });
}

async fn send(r: &HttpRequest) -> Reply {
    let window = web_sys::window().ok_or("no window")?;
    let headers = web_sys::Headers::new().map_err(err)?;
    for (name, value) in &r.headers {
        headers.set(name, value).map_err(err)?;
    }
    let init = web_sys::RequestInit::new();
    init.set_method(r.method);
    init.set_headers(&headers);
    init.set_credentials(web_sys::RequestCredentials::Omit);
    if let Some(body) = &r.body {
        init.set_body(&Uint8Array::from(body.as_slice()));
    }
    let response = JsFuture::from(window.fetch_with_str_and_init(&r.url, &init)).await.map_err(err)?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "response cast failed".to_string())?;
    Ok((response.status(), Vec::new()))
}
