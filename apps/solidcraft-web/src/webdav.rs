//! Origin-local, in-memory WebDAV credentials; requests never follow redirects.
use solidcraft_ui_egui::i18n::tr;
use solidcraft_ui_egui::webdav::{Request, Response};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::JsFuture;

pub fn service() -> solidcraft_ui_egui::webdav::Http {
    std::rc::Rc::new(|request| Box::pin(send(request)))
}
struct Abort(web_sys::AbortController);
impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn send(r: Request) -> Result<Response, String> {
    solidcraft_ui_egui::webdav::connection(&r.url)?;
    if r.user.contains(':') || r.user.len() > 1024 || r.password.len() > 4096 || r.body.len() > solidcraft_engine::io::MAX_DESIGN_BYTES {
        return Err(tr("webdav.error.credentials").into());
    }
    let failure = |_| tr("webdav.error.transport").to_string();
    let abort = Abort(web_sys::AbortController::new().map_err(failure)?);
    let options = web_sys::RequestInit::new();
    options.set_signal(Some(&abort.0.signal()));
    options.set_method(r.method);
    options.set_cache(web_sys::RequestCache::NoStore);
    options.set_credentials(web_sys::RequestCredentials::Omit);
    options.set_redirect(web_sys::RequestRedirect::Error);
    let headers = web_sys::Headers::new().map_err(failure)?;
    let auth = solidcraft_ui_egui::agent_chat::backend::base64(format!("{}:{}", r.user, r.password).as_bytes());
    headers.set("Authorization", &format!("Basic {auth}")).map_err(failure)?;
    for (key, value) in &r.headers {
        headers.set(key, value).map_err(failure)?;
    }
    options.set_headers(&headers);
    if r.method != "GET" {
        options.set_body(&js_sys::Uint8Array::from(r.body.as_slice()).into());
    }
    let window = web_sys::window().ok_or(tr("webdav.error.transport"))?;
    let response: web_sys::Response =
        JsFuture::from(window.fetch_with_str_and_init(&r.url, &options)).await.map_err(failure)?.dyn_into().map_err(failure)?;
    let status = response.status();
    let etag = response.headers().get("ETag").map_err(failure)?;
    let body = if let Some(stream) = response.body() {
        let reader: web_sys::ReadableStreamDefaultReader = stream.get_reader().dyn_into().map_err(|_| tr("webdav.error.transport").to_string())?;
        if r.method == "PUT" || !(200..300).contains(&status) {
            let _ = JsFuture::from(reader.cancel()).await;
            Vec::new()
        } else {
            let result = crate::linked::read(&reader, r.cap.min(solidcraft_engine::io::MAX_DESIGN_BYTES)).await;
            if result.is_err() {
                let _ = JsFuture::from(reader.cancel()).await;
            }
            result?
        }
    } else {
        Vec::new()
    };
    Ok(Response { status, etag, body })
}
