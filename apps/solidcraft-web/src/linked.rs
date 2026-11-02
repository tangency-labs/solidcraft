//! Bounded HTTPS or same-origin downloads, handed to the normal design-open command as unsaved copies.
use std::{cell::RefCell, rc::Rc};

use js_sys::{Reflect, Uint8Array};
use solidcraft_engine::io::vfs;
use solidcraft_ui_egui::{SolveApp, ui_kit};
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::{check_download_size, design_url};

type Reply = Result<(String, Vec<u8>), String>;

#[derive(Default)]
pub struct Download {
    reply: Rc<RefCell<Option<Reply>>>,
    pending: Option<(String, Vec<u8>)>,
    /// The downloaded design being opened (a few features a frame, #10198): the answer to come,
    /// the name it takes and the file it was read from.
    opening: Option<(std::sync::mpsc::Receiver<serde_json::Value>, String, String)>,
}

impl Download {
    pub fn start(url: Option<&str>, ctx: &egui::Context) -> Self {
        let download = Self::default();
        if let Some(url) = url {
            let url = url.to_owned();
            let slot = download.reply.clone();
            let ctx = ctx.clone();
            spawn_local(async move {
                *slot.borrow_mut() = Some(fetch(&url).await);
                ctx.request_repaint();
            });
        }
        download
    }

    pub fn tick(&mut self, app: &mut SolveApp, ctx: &egui::Context) {
        self.opened(app);
        if let Some(reply) = self.reply.borrow_mut().take() {
            match reply {
                Ok(file) => self.pending = Some(file),
                Err(error) => app.set_status(format!("Could not open design link: {error}"), true),
            }
        }
        if self.pending.is_none() || self.opening.is_some() || app.file_picker_busy() {
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
        let Some((name, bytes)) = self.pending.take() else { return };
        // The extension selects the design loader even when the URL has no extension. Named
        // after the link, as the progress card shows the file's name.
        let file: String = name.chars().map(|c| if matches!(c, '/' | '\\') { '_' } else { c }).collect();
        let path = format!("/linked/{}.{}", if file.is_empty() { "design" } else { &file }, solidcraft_engine::io::DESIGN_EXT);
        vfs::insert(&path, bytes);
        // Opened as any design is, with its progress and Cancel; finished in `opened`.
        self.opening = Some((app.open_path_answered(&path), name, path));
        self.opened(app);
    }

    /// The downloaded design is in: it is an unsaved copy under the link's name.
    fn opened(&mut self, app: &mut SolveApp) {
        let Some((rx, _, _)) = &self.opening else { return };
        let (ok, cancelled) = match rx.try_recv() {
            Ok(reply) => (reply["ok"].as_bool() == Some(true), reply["error"].as_str().is_some_and(|e| e.contains("cancelled"))),
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => (false, false),
        };
        let Some((_, name, path)) = self.opening.take() else { return };
        if ok && app.session.path.as_deref() == Some(path.as_str()) {
            app.session.path = None;
            app.session.doc_mut().name = name;
            app.session.mark_unsaved();
        } else if !ok && !cancelled {
            app.set_status("Could not open design link: the downloaded file is not a valid SolidCraft design", true);
        }
        let _ = vfs::remove(&path);
    }
}

async fn fetch(value: &str) -> Reply {
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
    let result = read(&reader).await;
    if result.is_err() {
        let _ = JsFuture::from(reader.cancel()).await;
    }
    // Fetch rejects interrupted HTTP bodies and decodes Content-Encoding. Content-Length
    // can describe compressed bytes, so the streamed limit also checks the decoded size.
    let bytes = result?;
    Ok((name, bytes))
}

async fn read(reader: &web_sys::ReadableStreamDefaultReader) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    // Bound even a stream which keeps returning empty chunks.
    for _ in 0..1_000_000 {
        let chunk = JsFuture::from(reader.read()).await.map_err(|_| "Download interrupted")?;
        if Reflect::get(&chunk, &JsValue::from_str("done")).map_err(|_| "Invalid download stream")?.as_bool() == Some(true) {
            return Ok(bytes);
        }
        let value = Reflect::get(&chunk, &JsValue::from_str("value")).map_err(|_| "Invalid download chunk")?;
        let chunk: Uint8Array = value.dyn_into().map_err(|_| "Invalid download bytes")?;
        check_download_size(bytes.len(), u64::from(chunk.length()))?;
        bytes.extend_from_slice(&chunk.to_vec());
    }
    Err("Download returned too many chunks".into())
}
