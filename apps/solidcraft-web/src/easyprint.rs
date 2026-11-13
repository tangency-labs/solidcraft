//! Open with EasyPrint in the browser: upload the model to Printables' import endpoint with
//! `fetch` (it allows any origin) and answer with the address to open.

use std::sync::mpsc::Sender;

use js_sys::{Array, Uint8Array};
use solidcraft_engine::easyprint::{IMPORT_URL, parse_import_response};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::files::err;

/// Start the upload; the answer arrives on `done`.
pub fn upload(name: &str, bytes: Vec<u8>, done: Sender<Result<String, String>>) {
    let name = name.to_string();
    spawn_local(async move {
        let _ = done.send(post(&name, &bytes).await);
    });
}

async fn post(name: &str, bytes: &[u8]) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let parts = Array::new();
    parts.push(&Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(err)?;
    let form = web_sys::FormData::new().map_err(err)?;
    form.append_with_blob_and_filename("file", &blob, name).map_err(err)?;
    form.append_with_str("file_name", name).map_err(err)?;
    let headers = web_sys::Headers::new().map_err(err)?;
    headers.set("Accept", "application/json").map_err(err)?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&form);
    init.set_headers(&headers);
    let response = JsFuture::from(window.fetch_with_str_and_init(IMPORT_URL, &init)).await.map_err(err)?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "response cast failed".to_string())?;
    let status = response.status();
    let text = JsFuture::from(response.text().map_err(err)?).await.map_err(err)?;
    parse_import_response(status, text.as_string().unwrap_or_default().as_bytes())
}
