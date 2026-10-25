//! Browser transport and picker for the optional server folder.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::task::Waker;

use solidcraft_engine::io::vfs;
use solidcraft_ui_egui::i18n::tr;
use solidcraft_ui_egui::{FilePickerFuture, SolveApp, open_elsewhere::DiskAnswer, ui_kit};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::{ENDPOINT, MAX_LIST_BYTES, MAX_LISTING, PREFIX, endpoint, parse, save_conflict, valid};
use crate::files::{Files, err};

type Answer = Rc<RefCell<(Option<Option<String>>, Option<Waker>)>>;
#[derive(Default)]
pub struct State {
    pub enabled: Cell<bool>,
    directory: RefCell<String>,
    entries: RefCell<Vec<(String, bool)>>,
    versions: RefCell<HashMap<String, String>>,
    picker: RefCell<Option<Picker>>,
    queue: RefCell<VecDeque<QueuedWrite>>,
    interactive: RefCell<HashSet<String>>,
    busy: Cell<bool>,
    conflict: RefCell<Option<String>>,
    pub conflicts: RefCell<Vec<String>>,
    force: RefCell<Option<String>>,
    outcomes: RefCell<HashMap<String, Result<(), String>>>,
}
struct QueuedWrite {
    path: String,
    bytes: Vec<u8>,
    overwrite: bool,
    interactive: bool,
}
struct Picker {
    answer: Answer,
    save: Option<String>,
    folder: String,
    place: u8,
}

async fn request(method: &str, path: &str, bytes: Option<&[u8]>, condition: Option<(&str, &str)>) -> Result<web_sys::Response, String> {
    let options = web_sys::RequestInit::new();
    options.set_method(method);
    options.set_cache(web_sys::RequestCache::NoStore);
    options.set_redirect(web_sys::RequestRedirect::Error);
    if let Some(bytes) = bytes {
        options.set_body(&js_sys::Uint8Array::from(bytes).into());
    }
    let headers = web_sys::Headers::new().map_err(err)?;
    if let Some((key, value)) = condition {
        headers.set(key, value).map_err(err)?;
    }
    options.set_headers(&headers);
    let window = web_sys::window().ok_or("No browser window")?;
    JsFuture::from(window.fetch_with_str_and_init(path, &options)).await.map_err(err)?.dyn_into().map_err(|_| "Invalid server response".into())
}

async fn bytes(response: &web_sys::Response, cap: usize) -> Result<Vec<u8>, String> {
    if let Some(length) = response.headers().get("Content-Length").map_err(err)? {
        let size = length.parse::<u64>().map_err(|_| "Invalid server response size")?;
        if size > cap as u64 {
            return Err("Server response is too large".into());
        }
    }
    let reader: web_sys::ReadableStreamDefaultReader =
        response.body().ok_or("Missing server response")?.get_reader().dyn_into().map_err(|_| "Invalid response stream")?;
    let result = crate::linked::read(&reader, cap).await;
    if result.is_err() {
        let _ = JsFuture::from(reader.cancel()).await;
    }
    result
}

pub fn probe(files: &Rc<Files>) {
    let files = files.clone();
    spawn_local(async move {
        if let Ok(response) = request("OPTIONS", ENDPOINT, None, None).await
            && response.ok()
            && response.headers().get("X-SolidCraft-Folder").ok().flatten().as_deref() == Some("1")
        {
            files.server.enabled.set(true);
            list(&files, "");
        }
        files.wake();
    });
}

fn list(files: &Rc<Files>, directory: &str) {
    let Some(mut url) = endpoint(directory) else { return };
    if !url.ends_with('/') {
        url.push('/');
    }
    let files = files.clone();
    let directory = directory.to_string();
    spawn_local(async move {
        let result = async {
            let response = request("GET", &url, None, None).await?;
            if !response.ok() {
                return Err(format!("Server folder: HTTP {}", response.status()));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes(&response, MAX_LIST_BYTES).await?).map_err(|e| e.to_string())?;
            let entries = value.as_array().ok_or("Invalid server listing")?;
            if entries.len() > MAX_LISTING {
                return Err("Server listing is too long".into());
            }
            let mut out = Vec::new();
            for entry in entries {
                let name = entry["name"].as_str().ok_or("Invalid server entry")?;
                if !valid(name) || name.contains('/') {
                    return Err("Invalid server entry name".into());
                }
                out.push((name.to_string(), entry["directory"].as_bool().ok_or("Invalid server entry type")?));
            }
            if *files.server.directory.borrow() == directory {
                *files.server.entries.borrow_mut() = out;
            }
            Ok::<_, String>(())
        }
        .await;
        if let Err(e) = result {
            files.say(e, true);
        }
        files.wake();
    });
}

pub async fn load(files: &Files, path: &str) -> Result<String, String> {
    let url = endpoint(path).ok_or("Invalid server path")?;
    let response = request("GET", &url, None, None).await?;
    if !response.ok() {
        return Err(format!("Server folder: HTTP {}", response.status()));
    }
    let tag = response.headers().get("ETag").map_err(err)?.ok_or("Server folder did not supply an ETag")?;
    let data = bytes(&response, solidcraft_engine::io::MAX_DESIGN_BYTES).await?;
    let engine = super::engine_path(path).ok_or("Invalid server path")?;
    files.server.versions.borrow_mut().insert(engine.clone(), tag);
    vfs::insert(&engine, data);
    Ok(engine)
}

pub fn restore(files: &Rc<Files>, engine: &str) {
    let Some(path) = parse(engine) else { return };
    let (files, path) = (files.clone(), path.to_string());
    spawn_local(async move {
        match load(&files, &path).await {
            Ok(path) => files.opened.borrow_mut().push(path),
            Err(e) => files.say(e, true),
        }
        files.wake();
    });
}

pub async fn available() -> bool {
    request("OPTIONS", ENDPOINT, None, None)
        .await
        .is_ok_and(|response| response.ok() && response.headers().get("X-SolidCraft-Folder").ok().flatten().as_deref() == Some("1"))
}

pub fn pick(files: &Rc<Files>, save: Option<&str>) -> FilePickerFuture {
    let answer: Answer = Rc::default();
    *files.server.picker.borrow_mut() = Some(Picker { answer: answer.clone(), save: save.map(str::to_string), folder: String::new(), place: 2 });
    files.wake();
    Box::pin(std::future::poll_fn(move |cx| {
        let mut answer = answer.borrow_mut();
        match answer.0.take() {
            Some(value) => std::task::Poll::Ready(value),
            None => {
                answer.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }))
}
fn finish(answer: &Answer, path: Option<String>) {
    let mut answer = answer.borrow_mut();
    answer.0 = Some(path);
    if let Some(waker) = answer.1.take() {
        waker.wake();
    }
}

/// Feed the existing changed-elsewhere question rather than silently replacing server bytes.
pub fn answer_conflict(files: &Rc<Files>, answer: DiskAnswer) {
    let Some(path) = files.server.conflict.borrow_mut().take() else { return };
    if answer == DiskAnswer::Overwrite {
        *files.server.force.borrow_mut() = Some(path);
    }
}

pub fn persist(files: &Rc<Files>, path: &str) -> Option<solidcraft_ui_egui::save_job::PersistFuture> {
    parse(path)?;
    files.server.outcomes.borrow_mut().remove(path);
    files.server.interactive.borrow_mut().insert(path.to_string());
    crate::files::flush(files);
    let files = files.clone();
    let path = path.to_string();
    Some(Box::pin(std::future::poll_fn(move |_| match files.server.outcomes.borrow_mut().remove(&path) {
        Some(result) => std::task::Poll::Ready(result),
        None => std::task::Poll::Pending,
    })))
}

pub fn enqueue(files: &Rc<Files>, path: &str, bytes: Vec<u8>) {
    let overwrite = files.server.force.borrow_mut().take().as_deref() == Some(path);
    enqueue_inner(files, path.to_string(), bytes, overwrite);
}
fn enqueue_inner(files: &Rc<Files>, path: String, bytes: Vec<u8>, overwrite: bool) {
    let interactive = files.server.interactive.borrow_mut().remove(&path);
    if files.server.queue.borrow().len() >= 16
        || files.server.queue.borrow().iter().fold(bytes.len(), |total, write| total.saturating_add(write.bytes.len()))
            > solidcraft_engine::io::MAX_DESIGN_BYTES
    {
        if interactive {
            files.server.outcomes.borrow_mut().insert(path.clone(), Err("Too many pending server writes".into()));
        }
        files.unsaved.borrow_mut().push(path);
        files.say("Too many pending server writes", true);
        return;
    }
    files.server.queue.borrow_mut().retain(|write| write.path != path);
    files.server.queue.borrow_mut().push_back(QueuedWrite { path, bytes, overwrite, interactive });
    if files.server.busy.replace(true) {
        return;
    }
    let files = files.clone();
    spawn_local(async move {
        loop {
            let next = files.server.queue.borrow_mut().pop_front();
            let Some(QueuedWrite { path, bytes: data, overwrite, interactive }) = next else { break };
            let result = async {
                let relative = parse(&path).ok_or("Invalid server path")?;
                let url = endpoint(relative).ok_or("Invalid server path")?;
                crate::check_download_size(0, data.len() as u64)?;
                let tag = files.server.versions.borrow().get(&path).cloned();
                let condition = super::save_condition(tag.as_deref(), overwrite);
                let response = request("PUT", &url, Some(&data), condition).await?;
                if response.status() == 412 {
                    if save_conflict(response.status(), interactive) {
                        files.server.conflicts.borrow_mut().push(path.clone());
                        *files.server.conflict.borrow_mut() = Some(path.clone());
                    }
                    return Err("The server file changed elsewhere. Choose Save a copy, Overwrite, or Cancel.".into());
                }
                if !response.ok() {
                    return Err(format!("Server save: HTTP {}", response.status()));
                }
                let tag = response.headers().get("ETag").map_err(err)?.ok_or("Server save did not supply an ETag")?;
                files.server.versions.borrow_mut().insert(path.clone(), tag);
                files.unsaved.borrow_mut().retain(|failed| failed != &path);
                Ok::<_, String>(())
            }
            .await;
            if interactive {
                files.server.outcomes.borrow_mut().insert(path.clone(), result.clone());
            }
            if let Err(e) = result {
                files.unsaved.borrow_mut().push(path);
                files.say(e, true);
            }
            files.wake();
        }
        files.server.busy.set(false);
    });
}

pub fn show(files: &Rc<Files>, ctx: &egui::Context) {
    let mut slot = files.server.picker.borrow_mut();
    let Some(picker) = slot.as_mut() else { return };
    let mut selected = None;
    let mut computer = false;
    let directory = files.server.directory.borrow().clone();
    let join = |name: &str| if directory.is_empty() { name.to_string() } else { format!("{directory}/{name}") };
    let title = if picker.save.is_some() { tr("web.server.save") } else { tr("web.server.open") };
    let closed = ui_kit::Window::new(egui::Id::new("server_folder_picker"), "folder", title).cards(3.0).show_body(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut picker.place, 0, tr("web.server.computer"));
            ui.selectable_value(&mut picker.place, 1, tr("web.server.browser"));
            ui.selectable_value(&mut picker.place, 2, tr("web.server.place"));
        });
        ui.separator();
        if picker.place == 0 {
            if ui_kit::button(ui, tr("web.server.choose_computer"), ui_kit::Button::Normal).clicked() {
                computer = true;
            }
            return;
        }
        if picker.place == 1 {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for entry in files.entries.borrow().iter().filter(|e| solidcraft_engine::io::is_design_path(&e.name)) {
                    if ui_kit::button(ui, &entry.name, ui_kit::Button::Normal).clicked() {
                        if let Some(name) = &mut picker.save {
                            *name = entry.name.clone();
                        } else {
                            selected = Some(format!("{}{}", crate::files::OPFS, entry.name));
                        }
                    }
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(solidcraft_ui_egui::i18n::format("web.server.location", &[("path", &directory)]));
                if ui_kit::button(ui, tr("web.server.up"), ui_kit::Button::Small).clicked() {
                    let parent = directory.rsplit_once('/').map_or("", |(p, _)| p);
                    *files.server.directory.borrow_mut() = parent.to_string();
                    list(files, parent);
                }
                if ui_kit::button(ui, tr("web.server.refresh"), ui_kit::Button::Small).clicked() {
                    list(files, &directory);
                }
            });
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (name, folder) in files.server.entries.borrow().iter() {
                    if !folder && !solidcraft_engine::io::is_design_path(name) {
                        continue;
                    }
                    let label = if *folder { format!("{name}/") } else { name.clone() };
                    if ui_kit::button(ui, &label, ui_kit::Button::Normal).clicked() {
                        if *folder {
                            let path = join(name);
                            *files.server.directory.borrow_mut() = path.clone();
                            list(files, &path);
                        } else if let Some(save) = &mut picker.save {
                            *save = name.clone();
                        } else {
                            selected = super::engine_path(&join(name));
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui_kit::cell_editor(ui, ui.make_persistent_id("server_new_folder"), &mut picker.folder, tr("web.server.new_folder"), false);
                if ui_kit::button(ui, tr("web.server.new_folder"), ui_kit::Button::Small).clicked()
                    && valid(&picker.folder)
                    && !picker.folder.contains('/')
                {
                    let path = join(&picker.folder);
                    let files = files.clone();
                    let directory = directory.clone();
                    spawn_local(async move {
                        if let Some(mut url) = endpoint(&path) {
                            url.push('/');
                            match request("MKCOL", &url, None, None).await {
                                Ok(r) if r.ok() => list(&files, &directory),
                                Ok(r) => files.say(format!("New folder: HTTP {}", r.status()), true),
                                Err(e) => files.say(e, true),
                            }
                        }
                    });
                    picker.folder.clear();
                }
            });
        }
        if let Some(name) = &mut picker.save {
            ui.horizontal(|ui| {
                ui_kit::cell_editor(ui, ui.make_persistent_id("server_save_name"), name, tr("web.server.name"), false);
                if ui_kit::button(ui, tr("ui.save"), ui_kit::Button::Normal).clicked() && valid(name) && !name.contains('/') {
                    let name = if solidcraft_engine::io::is_design_path(name) { name.clone() } else { format!("{name}.solidcraft") };
                    selected = if picker.place == 1 { Some(format!("{}{name}", crate::files::OPFS)) } else { super::engine_path(&join(&name)) };
                }
            });
        }
    });
    if closed {
        finish(&picker.answer, None);
        *slot = None;
        return;
    }
    if computer {
        let answer = picker.answer.clone();
        let save = picker.save.clone();
        let files = files.clone();
        spawn_local(async move {
            let path = if crate::disk::supported() {
                if let Some(name) = save { crate::disk::pick_save(&files, &name).await } else { crate::disk::pick_open(&files).await }
            } else if let Some(name) = save {
                Some(format!("{}{name}", crate::files::DOWNLOADS))
            } else {
                crate::files::pick_from_disk(&files, crate::files::Intent::Open);
                None
            };
            finish(&answer, path);
        });
        *slot = None;
    } else if let Some(path) = selected {
        let answer = picker.answer.clone();
        if picker.save.is_some() && parse(&path).is_some() {
            // Save As must ask before replacing an existing target, even one opened earlier.
            files.server.versions.borrow_mut().remove(&path);
        }
        if picker.save.is_none() && parse(&path).is_some() {
            let files = files.clone();
            spawn_local(async move {
                match load(&files, path.strip_prefix(PREFIX).unwrap_or("")).await {
                    Ok(path) => finish(&answer, Some(path)),
                    Err(e) => {
                        files.say(e, true);
                        finish(&answer, None);
                    }
                }
            });
        } else {
            finish(&answer, Some(path));
        }
        *slot = None;
    }
}

pub fn conflict_prompt(files: &Rc<Files>, app: &mut SolveApp) {
    let path = files.server.conflicts.borrow_mut().pop();
    if let Some(path) = path
        && app.session.path.as_deref() == Some(&path)
    {
        app.session.mark_unsaved();
        app.disk_prompt = true;
    }
}
