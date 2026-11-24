//! Problem reports from the browser (docs/problem-reports.md): requests to the ticket server with
//! `fetch` (no cookies; the token goes as a bearer header) and the sign-in kept in the browser's
//! local storage, the web's place for it.

use std::sync::mpsc::Sender;

use js_sys::Uint8Array;
use solidcraft_ui_egui::agent_chat::backend::{HttpRequest, Secrets};
use solidcraft_ui_egui::tickets::Reply;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::files::err;

/// Send `r`; the answer arrives on `done`.
pub fn http(r: HttpRequest, done: Sender<Reply>) {
    spawn_local(async move {
        let _ = done.send(fetch(&r).await);
    });
}

async fn fetch(r: &HttpRequest) -> Reply {
    if !r.url.starts_with("https://") {
        return Err("only HTTPS is allowed".into());
    }
    let window = web_sys::window().ok_or("no window")?;
    let headers = web_sys::Headers::new().map_err(err)?;
    for (k, v) in &r.headers {
        headers.set(k, v).map_err(err)?;
    }
    let init = web_sys::RequestInit::new();
    init.set_method(r.method);
    init.set_headers(&headers);
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_redirect(web_sys::RequestRedirect::Error);
    if let Some(body) = &r.body {
        init.set_body(&Uint8Array::from(body.as_slice()));
    }
    let response = JsFuture::from(window.fetch_with_str_and_init(&r.url, &init)).await.map_err(err)?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "response cast failed".to_string())?;
    let status = response.status();
    let buffer = JsFuture::from(response.array_buffer().map_err(err)?).await.map_err(err)?;
    let bytes = Uint8Array::new(&buffer).to_vec();
    if bytes.len() > 16 << 20 {
        return Err("the answer was too large".into());
    }
    Ok((status, bytes))
}

/// The ticket server's sign-in in local storage, under `solidcraft.secret.<id>`.
pub struct BrowserSecrets;

fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or_else(|| "this browser keeps no local storage".to_string())
}

fn key(id: &str) -> String {
    format!("solidcraft.secret.{id}")
}

impl Secrets for BrowserSecrets {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        storage()?.get_item(&key(id)).map_err(err)
    }
    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        storage()?.set_item(&key(id), secret).map_err(err)
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        storage()?.remove_item(&key(id)).map_err(err)
    }
}
