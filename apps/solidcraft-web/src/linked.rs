//! Bounded HTTPS or same-origin downloads, handed to the normal design-open command as unsaved copies.
use std::{cell::RefCell, rc::Rc};

use js_sys::{Reflect, Uint8Array};
use solidcraft_engine::io::vfs;
use solidcraft_ui_egui::{SolveApp, ui_kit};
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::{check_download_size, design_url};

enum Source {
    Copy(String, Vec<u8>),
    Server(String),
}
type Reply = Result<Source, String>;

#[derive(Default)]
pub struct Download {
    reply: Rc<RefCell<Option<Reply>>>,
    pending: Option<Source>,
}

impl Download {
    pub fn start(url: Option<&str>, ctx: &egui::Context, files: &Rc<crate::files::Files>) -> Self {
        let download = Self::default();
        if let Some(url) = url {
            let url = url.to_owned();
            let slot = download.reply.clone();
            let ctx = ctx.clone();
            let files = files.clone();
            spawn_local(async move {
                let origin = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
                let server_path = crate::server::link_path(&url, &origin);
                let result = if server_path.is_some() && crate::server::available().await {
                    files.server.enabled.set(true);
                    crate::server::load(&files, server_path.as_deref().unwrap_or("")).await.map(Source::Server)
                } else {
                    fetch(&url).await.map(|(name, bytes)| Source::Copy(name, bytes))
                };
                *slot.borrow_mut() = Some(result);
                ctx.request_repaint();
            });
        }
        download
    }

    pub fn tick(&mut self, app: &mut SolveApp, ctx: &egui::Context) {
        if let Some(reply) = self.reply.borrow_mut().take() {
            match reply {
                Ok(file) => self.pending = Some(file),
                Err(error) => app.set_status(format!("Could not open design link: {error}"), true),
            }
        }
        if self.pending.is_none() || app.file_picker_busy() {
            return;
        }
        if solidcraft_ui_egui::documents::any_dirty(app) {
            let mut answer = None;
            let closed = ui_kit::Window::new(egui::Id::new("open_link"), "status_warning", solidcraft_ui_egui::i18n::tr("web.open_link"))
                .required()
                .show_body(ctx, |ui| {
                    ui.label(solidcraft_ui_egui::i18n::tr("web.open_link_confirm"));
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, solidcraft_ui_egui::i18n::tr("ui.open"), ui_kit::Button::Normal).clicked() {
                            answer = Some(true);
                        }
                        if ui_kit::button(ui, solidcraft_ui_egui::i18n::tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                            answer = Some(false);
                        }
                    });
                });
            match answer {
                Some(true) => {}
                Some(false) => {
                    self.pending = None;
                    return;
                }
                None if closed => {
                    self.pending = None;
                    return;
                }
                None => return,
            }
        }
        let Some(source) = self.pending.take() else { return };
        let (name, bytes) = match source {
            Source::Server(path) => {
                app.open_path(&path);
                return;
            }
            Source::Copy(name, bytes) => (name, bytes),
        };
        // The extension selects the design loader even when the URL has no extension.
        let path = &format!("/linked/design.{}", solidcraft_engine::io::DESIGN_EXT);
        vfs::insert(path, bytes);
        app.open_path(path);
        if app.session.path.as_deref() == Some(path.as_str()) {
            app.session.path = None;
            app.session.doc_mut().name = name;
            app.session.mark_unsaved();
        } else {
            app.set_status("Could not open design link: the downloaded file is not a valid SolidCraft design", true);
        }
        let _ = vfs::remove(path);
    }
}

async fn fetch(value: &str) -> Result<(String, Vec<u8>), String> {
    let window = web_sys::window().ok_or("No browser window")?;
    let page_origin = window.location().origin().map_err(|_| "Cannot read page origin")?;
    let url = design_url(value, &page_origin)?;
    let name = url.path_segments().and_then(|mut segments| segments.next_back()).filter(|name| !name.is_empty()).unwrap_or("Design");
    let decoded = js_sys::decode_uri_component(name).ok().and_then(|name| name.as_string()).unwrap_or_else(|| name.to_owned());
    // `arm.sc` (or a legacy `arm.solidcraft`) is named `arm`.
    let stem = match decoded.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && solidcraft_engine::io::is_design_ext(ext) => stem,
        _ => decoded.as_str(),
    };
    let name: String = stem.chars().take(255).collect();
    let response = JsFuture::from(window.fetch_with_str(url.as_str())).await.map_err(|_| "Network request failed or the host refused CORS access")?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "Invalid download response")?;
    design_url(&response.url(), &page_origin).map_err(|_| "Design redirected to a URL that does not use HTTPS or the page's own origin")?;
    if !response.ok() {
        return Err(format!("HTTP {} {}", response.status(), response.status_text()));
    }
    let length = response
        .headers()
        .get("Content-Length")
        .map_err(|_| "Cannot read download size")?
        .map(|value| value.parse::<u64>().map_err(|_| "Invalid Content-Length"))
        .transpose()?;
    if let Some(length) = length {
        check_download_size(0, length)?;
    }
    let stream = response.body().ok_or("Download has no body")?;
    let reader: web_sys::ReadableStreamDefaultReader = stream.get_reader().dyn_into().map_err(|_| "Cannot read download stream")?;
    let result = read(&reader, solidcraft_engine::io::MAX_DESIGN_BYTES).await;
    if result.is_err() {
        let _ = JsFuture::from(reader.cancel()).await;
    }
    // Fetch rejects interrupted HTTP bodies and decodes Content-Encoding. Content-Length
    // can describe compressed bytes, so the streamed limit also checks the decoded size.
    let bytes = result?;
    Ok((name, bytes))
}

pub(crate) async fn read(reader: &web_sys::ReadableStreamDefaultReader, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    // Bound even a stream which keeps returning empty chunks.
    for _ in 0..1_000_000 {
        let chunk = JsFuture::from(reader.read()).await.map_err(|_| "Download interrupted")?;
        if Reflect::get(&chunk, &JsValue::from_str("done")).map_err(|_| "Invalid download stream")?.as_bool() == Some(true) {
            return Ok(bytes);
        }
        let value = Reflect::get(&chunk, &JsValue::from_str("value")).map_err(|_| "Invalid download chunk")?;
        let chunk: Uint8Array = value.dyn_into().map_err(|_| "Invalid download bytes")?;
        if u64::from(chunk.length()) > cap.saturating_sub(bytes.len()) as u64 {
            return Err("Download exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk.to_vec());
    }
    Err("Download returned too many chunks".into())
}
