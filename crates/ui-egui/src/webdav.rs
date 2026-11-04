//! Optional WebDAV storage. Hosts provide HTTP; the picker and save transaction are shared.
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    rc::Rc,
};

use quick_xml::{events::Event, name::ResolveResult};
use solidcraft_engine::io::{self, vfs};
use url::Url;

use crate::{SolveApp, file_picker::Completion, i18n::tr, ui_kit};

pub const MAX_LIST_BYTES: usize = 4 << 20;
pub const MAX_ENTRIES: usize = 10_000;
pub const PROPFIND: &[u8] =
    br#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/></d:prop></d:propfind>"#;

pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub user: String,
    pub password: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub cap: usize,
}
pub struct Response {
    pub status: u16,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}
pub type Reply = Pin<Box<dyn Future<Output = Result<Response, String>>>>;
pub type Http = Rc<dyn Fn(Request) -> Reply>;
type Task<T> = Pin<Box<dyn Future<Output = Result<T, String>>>>;

fn work<T: Send + 'static>(run: impl FnOnce() -> Result<T, String> + Send + 'static) -> Task<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let language = crate::i18n::language();
        let started = std::thread::Builder::new().name("webdav-file".into()).spawn(move || {
            let _language = crate::i18n::scope(language);
            let _ = tx.send(run());
        });
        Box::pin(async move {
            if started.is_err() {
                return Err(tr("webdav.error.worker").into());
            }
            std::future::poll_fn(move |_| match rx.try_recv() {
                Ok(result) => std::task::Poll::Ready(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => std::task::Poll::Pending,
                Err(_) => std::task::Poll::Ready(Err(tr("webdav.error.worker").into())),
            })
            .await
        })
    }
    #[cfg(target_arch = "wasm32")]
    Box::pin(async move { run() })
}

/// Refuse insecure remote authentication, URL credentials, and ambiguous paths before parsing.
pub fn connection(value: &str) -> Result<Url, String> {
    if value.len() > 4096 || value.chars().any(|c| c.is_control() || c == '\\') {
        return Err(tr("webdav.error.url").into());
    }
    let mut url = Url::parse(value).map_err(|_| tr("webdav.error.url"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err(tr("webdav.error.https").into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() || url.host_str().is_none() {
        return Err(tr("webdav.error.url").into());
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

pub fn name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '%' | '?' | '#' | ':'))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
}

/// A namespace-aware, non-expanding streaming parser, bounded before allocating entries.
pub fn listing(xml: &[u8], directory: &Url) -> Result<Vec<Entry>, String> {
    let bad = || tr("webdav.error.listing").to_string();
    if xml.len() > MAX_LIST_BYTES {
        return Err(tr("webdav.error.listing_size").into());
    }
    let mut reader = quick_xml::NsReader::from_reader(xml);
    reader.config_mut().check_end_names = true;
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut entries = BTreeMap::new();
    let (mut href, mut status, mut size) = (String::new(), String::new(), String::new());
    let (mut folder, mut prop_folder, mut prop_size, mut successful, mut responses) = (false, false, String::new(), false, 0usize);
    let mut root = false;
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|_| bad())?;
        let dav = matches!(ns, ResolveResult::Bound(n) if n.as_ref() == b"DAV:");
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let tag = if dav { e.local_name().as_ref().to_vec() } else { Vec::new() };
                if stack.is_empty() {
                    if root || tag != b"multistatus" {
                        return Err(bad());
                    }
                    root = true;
                }
                if stack.len() >= 32 {
                    return Err(bad());
                }
                let parent = stack.last().map(Vec::as_slice);
                let valid_parent = match tag.as_slice() {
                    b"multistatus" => stack.is_empty(),
                    b"response" => stack.len() == 1 && parent == Some(b"multistatus"),
                    b"href" | b"propstat" => parent == Some(b"response"),
                    b"prop" => parent == Some(b"propstat"),
                    b"status" => matches!(parent, Some(b"propstat" | b"response")),
                    b"resourcetype" | b"getcontentlength" => parent == Some(b"prop"),
                    b"collection" => parent == Some(b"resourcetype"),
                    _ => true,
                };
                if !valid_parent {
                    return Err(bad());
                }
                if tag == b"response" {
                    if stack.last().map(Vec::as_slice) != Some(b"multistatus") {
                        return Err(bad());
                    }
                    responses += 1;
                    if responses > MAX_ENTRIES + 1 {
                        return Err(tr("webdav.error.listing_size").into());
                    }
                    href.clear();
                    folder = false;
                    successful = false;
                }
                if tag == b"propstat" {
                    status.clear();
                    prop_folder = false;
                    prop_size.clear();
                }
                if tag == b"collection" && stack.last().map(Vec::as_slice) == Some(b"resourcetype") {
                    prop_folder = true;
                }
                stack.push(tag);
            }
            Event::Text(e) => {
                let text = e.decode().map_err(|_| bad())?;
                let text = quick_xml::escape::unescape(&text).map_err(|_| bad())?;
                if stack.is_empty() && !text.trim().is_empty() {
                    return Err(bad());
                }
                let field = match stack.last().map(Vec::as_slice) {
                    Some(b"href") => Some(&mut href),
                    Some(b"status") => Some(&mut status),
                    Some(b"getcontentlength") => Some(&mut prop_size),
                    _ => None,
                };
                if let Some(field) = field {
                    if field.len().saturating_add(text.len()) > 8192 {
                        return Err(bad());
                    }
                    field.push_str(&text);
                }
            }
            Event::End(_) => {
                let tag = stack.pop().ok_or_else(bad)?;
                if tag == b"propstat" && status.split_whitespace().nth(1) == Some("200") {
                    successful = true;
                    folder |= prop_folder;
                    size.clone_from(&prop_size);
                    if !size.is_empty() && size.trim().parse::<u64>().map_err(|_| bad())? > io::MAX_DESIGN_BYTES as u64 && !folder {
                        return Err(tr("webdav.error.design_size").into());
                    }
                }
                if tag == b"response" && successful {
                    let href = href.trim();
                    if href.is_empty() || href.len() > 6144 || href.contains(['\\', '?', '#']) {
                        return Err(bad());
                    }
                    let decoded_href = percent_encoding::percent_decode_str(href).decode_utf8().map_err(|_| bad())?;
                    if decoded_href.split('/').any(|part| matches!(part, "." | "..")) {
                        return Err(bad());
                    }
                    let url = directory.join(href).map_err(|_| bad())?;
                    if url.origin() != directory.origin() || !url.username().is_empty() || url.password().is_some() {
                        return Err(bad());
                    }
                    if url.path().trim_end_matches('/') == directory.path().trim_end_matches('/') {
                        continue;
                    }
                    let child = url.path().strip_prefix(directory.path()).ok_or_else(bad)?.trim_end_matches('/');
                    let decoded = percent_encoding::percent_decode_str(child).decode_utf8().map_err(|_| bad())?;
                    if !name_valid(&decoded) || child.contains('/') {
                        return Err(bad());
                    }
                    if entries.insert(decoded.to_string(), folder).is_some() || entries.len() > MAX_ENTRIES {
                        return Err(bad());
                    }
                }
            }
            Event::Eof => break,
            Event::GeneralRef(e) => {
                if stack.is_empty() {
                    return Err(bad());
                }
                let value = e
                    .resolve_char_ref()
                    .map_err(|_| bad())?
                    .or_else(|| match &*e {
                        b"amp" => Some('&'),
                        b"lt" => Some('<'),
                        b"gt" => Some('>'),
                        b"quot" => Some('"'),
                        b"apos" => Some('\''),
                        _ => None,
                    })
                    .ok_or_else(bad)?;
                let field = match stack.last().map(Vec::as_slice) {
                    Some(b"href") => Some(&mut href),
                    Some(b"status") => Some(&mut status),
                    Some(b"getcontentlength") => Some(&mut prop_size),
                    _ => None,
                };
                if let Some(field) = field {
                    if field.len() >= 8192 {
                        return Err(bad());
                    }
                    field.push(value);
                }
            }
            Event::DocType(_) | Event::CData(_) => return Err(bad()),
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
        }
    }
    if !root || !stack.is_empty() {
        return Err(bad());
    }
    Ok(entries.into_iter().map(|(name, directory)| Entry { name, directory }).collect())
}

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) base: Url,
    pub(crate) user: String,
    pub(crate) password: String,
}
impl Config {
    fn request(&self, method: &'static str, url: &Url, body: Vec<u8>, headers: Vec<(String, String)>, cap: usize) -> Request {
        Request { method, url: url.to_string(), user: self.user.clone(), password: self.password.clone(), headers, body, cap }
    }
}
#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) config: Config,
    pub(crate) url: Url,
    pub(crate) etag: Option<String>,
}
#[derive(Default)]
pub struct State {
    picker: Option<Picker>,
    pub(crate) password: String,
    pub(crate) targets: Rc<RefCell<BTreeMap<String, Target>>>,
    pub(crate) conflict: Rc<RefCell<bool>>,
    pub(crate) overwrite: bool,
    pub(crate) interactive: Rc<Cell<bool>>,
    serial: u64,
    #[cfg(not(target_arch = "wasm32"))]
    roots: Vec<std::path::PathBuf>,
}
struct Picker {
    config: Config,
    directory: Url,
    entries: Vec<Entry>,
    name: Option<String>,
    complete: Option<Completion>,
    pending: Option<Reply>,
    prepared: Option<(String, Target, Task<()>)>,
    opening: Option<Url>,
    error: Option<String>,
    extensions: Vec<String>,
}

pub(crate) fn busy(app: &SolveApp) -> bool {
    app.webdav.picker.is_some() || *app.webdav.conflict.borrow()
}

pub fn settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.services.webdav_http.is_none() {
        return;
    }
    ui_kit::settings_section(ui, "webdav", tr("webdav.title"), |ui| {
        ui_kit::checkbox(ui, tr("webdav.enable"), &mut app.preferences.webdav_enabled);
        ui_kit::row(ui, tr("webdav.url"), |ui| {
            ui_kit::bounded_cell_editor(ui, ui.id().with("webdav_url"), &mut app.preferences.webdav_url, tr("webdav.url"), false, 4096);
        });
        ui_kit::row(ui, tr("webdav.user"), |ui| {
            ui_kit::bounded_cell_editor(ui, ui.id().with("webdav_user"), &mut app.preferences.webdav_user, tr("webdav.user"), false, 1024);
        });
        ui_kit::row(ui, tr("webdav.password"), |ui| {
            ui_kit::password_field(ui, ui.id().with("webdav_password"), &mut app.webdav.password, tr("webdav.password"));
        });
        ui_kit::text(ui, tr("webdav.credentials"), ui_kit::Text::Hint("webdav_credentials"));
    });
}

/// Use the existing host picker when disabled or when the caller requests another file type.
pub(crate) fn pick(app: &mut SolveApp, name: Option<&str>, extensions: &[&str], complete: Completion) -> Result<(), Completion> {
    let _language = crate::i18n::scope(&app.preferences.language);
    if !app.preferences.webdav_enabled || app.services.webdav_http.is_none() || !(extensions.is_empty() || extensions.contains(&"solidcraft")) {
        return Err(complete);
    }
    if app.preferences.webdav_user.len() > 1024 || app.preferences.webdav_user.contains(':') || app.webdav.password.len() > 4096 {
        app.set_status(tr("webdav.error.credentials"), true);
        complete(app, None);
        return Ok(());
    }
    let base = match connection(&app.preferences.webdav_url) {
        Ok(base) => base,
        Err(e) => {
            app.set_status(e, true);
            complete(app, None);
            return Ok(());
        }
    };
    let config = Config { base: base.clone(), user: app.preferences.webdav_user.clone(), password: app.webdav.password.clone() };
    app.webdav.overwrite = false;
    app.webdav.picker = Some(Picker {
        config,
        directory: base,
        entries: Vec::new(),
        name: name.map(str::to_string),
        complete: Some(complete),
        pending: None,
        prepared: None,
        opening: None,
        error: None,
        extensions: extensions.iter().map(|s| s.to_string()).collect(),
    });
    if let (Some(p), Some(http)) = (app.webdav.picker.as_mut(), app.services.webdav_http.as_ref()) {
        p.pending = Some(http(list_request(p)));
    }
    Ok(())
}
fn list_request(p: &Picker) -> Request {
    p.config.request(
        "PROPFIND",
        &p.directory,
        PROPFIND.to_vec(),
        vec![("Depth".into(), "1".into()), ("Content-Type".into(), "application/xml; charset=utf-8".into())],
        MAX_LIST_BYTES,
    )
}
fn child(directory: &Url, name: &str, folder: bool) -> Result<Url, String> {
    if !name_valid(name) {
        return Err(tr("webdav.error.url").into());
    }
    let mut url = directory.clone();
    url.path_segments_mut().map_err(|_| tr("webdav.error.url"))?.pop_if_empty().push(name);
    if folder {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn cache(state: &mut State, name: &str, data: Option<Vec<u8>>) -> Result<(String, Task<()>), String> {
    if state.serial >= 256 {
        return Err(tr("webdav.error.limit").into());
    }
    state.serial = state.serial.checked_add(1).ok_or(tr("webdav.error.limit"))?;
    #[cfg(not(target_arch = "wasm32"))]
    let path = {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| tr("webdav.error.cache"))?.as_nanos();
        let root = io::vfs::temp_dir().join(format!("solidcraft-webdav-{}-{now}-{}", std::process::id(), state.serial));
        std::fs::create_dir(&root).map_err(|_| tr("webdav.error.cache"))?;
        state.roots.push(root.clone());
        root.join(name).to_string_lossy().into_owned()
    };
    #[cfg(target_arch = "wasm32")]
    let path = format!("/webdav/{}/{name}", state.serial);
    let local = path.clone();
    let task = work(move || {
        if let Some(data) = data {
            #[cfg(not(target_arch = "wasm32"))]
            io::safe_write::write_atomic(std::path::Path::new(&local), &data, false).map_err(|_| tr("webdav.error.cache"))?;
            #[cfg(target_arch = "wasm32")]
            vfs::insert(&local, data);
        }
        Ok(())
    });
    Ok((path, task))
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if std::mem::take(&mut *app.webdav.conflict.borrow_mut()) {
        app.disk_prompt = true;
        app.session.mark_unsaved();
    }
    let Some(mut p) = app.webdav.picker.take() else { return };
    if let Some((_, _, task)) = p.prepared.as_mut() {
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match task.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(result) => {
                if let Some((path, target, _)) = p.prepared.take() {
                    match result {
                        Ok(()) => {
                            app.webdav.targets.borrow_mut().insert(path.clone(), target);
                            if let Some(done) = p.complete.take() {
                                done(app, Some(path));
                            }
                            return;
                        }
                        Err(error) => p.error = Some(error),
                    }
                }
            }
            std::task::Poll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
    }
    let Some(http) = app.services.webdav_http.clone() else { return };
    let mut selected = None;
    if let Some(pending) = p.pending.as_mut() {
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        if let std::task::Poll::Ready(result) = pending.as_mut().poll(&mut cx) {
            p.pending = None;
            let opening = p.opening.take();
            match result.and_then(|r| {
                if !(200..300).contains(&r.status) {
                    return Err(crate::i18n::format("webdav.error.http", &[("status", &r.status.to_string())]));
                }
                if let Some(url) = opening {
                    let tag = strong_etag(r.etag)?;
                    selected = Some((url, Some((r.body, tag))));
                } else {
                    if r.status != 207 {
                        return Err(tr("webdav.error.listing").into());
                    }
                    p.entries = listing(&r.body, &p.directory)?;
                }
                Ok(())
            }) {
                Ok(()) => {}
                Err(e) => p.error = Some(e),
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    let mut computer = false;
    let mut refresh = false;
    let mut navigate = None;
    let closed = ui_kit::Window::new(egui::Id::new("webdav_picker"), "folder", tr("webdav.title")).cards(3.0).show_body(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui_kit::button(ui, tr("webdav.other"), ui_kit::Button::Small).clicked() {
                computer = true;
            }
            if ui_kit::button(ui, tr("web.server.place"), ui_kit::Button::SmallSelected).clicked() {
                refresh = true;
            }
            if ui_kit::button(ui, tr("web.server.refresh"), ui_kit::Button::Small).clicked() {
                refresh = true;
            }
            if ui_kit::button(ui, tr("web.server.up"), ui_kit::Button::Small).clicked() && p.directory != p.config.base {
                navigate = p.directory.join("../").ok().filter(|u| u.path().starts_with(p.config.base.path()));
            }
        });
        ui.label(p.directory.path());
        if let Some(error) = &p.error {
            ui_kit::text(ui, error, ui_kit::Text::Report("webdav_error", true));
        }
        ui.add_enabled_ui(p.pending.is_none() && p.prepared.is_none(), |ui| {
            egui::ScrollArea::vertical().max_height(crate::theme::Tokens::get().space_24 * 12.0).show(ui, |ui| {
                for entry in &p.entries {
                    if !entry.directory && !io::is_design_path(&entry.name) {
                        continue;
                    }
                    let label = if entry.directory { format!("{}/", entry.name) } else { entry.name.clone() };
                    if ui_kit::button(ui, label, ui_kit::Button::Normal).clicked()
                        && let Ok(url) = child(&p.directory, &entry.name, entry.directory)
                    {
                        if entry.directory {
                            navigate = Some(url);
                        } else if let Some(name) = &mut p.name {
                            name.clone_from(&entry.name);
                        } else {
                            p.opening = Some(url.clone());
                            p.pending = Some(http(p.config.request("GET", &url, Vec::new(), Vec::new(), io::MAX_DESIGN_BYTES)));
                        }
                    }
                }
            });
            if let Some(name) = &mut p.name {
                ui_kit::bounded_cell_editor(ui, ui.id().with("webdav_name"), name, tr("web.server.name"), false, 255);
                if ui_kit::button(ui, tr("ui.save"), ui_kit::Button::Normal).clicked() && name_valid(name) {
                    let name = if io::is_design_path(name) { name.clone() } else { format!("{name}.solidcraft") };
                    match child(&p.directory, &name, false) {
                        Ok(url) => selected = Some((url, None)),
                        Err(error) => p.error = Some(error),
                    }
                }
            }
        });
    });
    if closed {
        if let Some(done) = p.complete.take() {
            done(app, None);
        }
        return;
    }
    if computer {
        let extensions: Vec<&str> = p.extensions.iter().map(String::as_str).collect();
        let future = if let Some(name) = &p.name {
            app.services.pick_save.as_ref().map(|f| f(name, &extensions))
        } else {
            app.services.pick_open.as_ref().map(|f| f(&extensions))
        };
        if let Some(done) = p.complete.take() {
            app.begin_webdav_fallback(future, done);
        }
        return;
    }
    if let Some((url, data)) = selected {
        let name =
            percent_encoding::percent_decode_str(url.path().rsplit('/').next().unwrap_or("design.solidcraft")).decode_utf8_lossy().into_owned();
        let (bytes, etag) = data.map_or((None, None), |(bytes, tag)| (Some(bytes), Some(tag)));
        match cache(&mut app.webdav, &name, bytes) {
            Ok((path, task)) => {
                let target = Target { config: p.config.clone(), url, etag };
                p.prepared = Some((path, target, task));
                ctx.request_repaint();
            }
            Err(e) => p.error = Some(e),
        }
    }

    if let Some(directory) = navigate.filter(|_| p.pending.is_none() && p.prepared.is_none()) {
        p.directory = directory;
        refresh = true;
    }
    if refresh && p.pending.is_none() && p.prepared.is_none() {
        p.error = None;
        p.pending = Some(http(list_request(&p)));
    }
    app.webdav.picker = Some(p);
}

pub fn strong_etag(tag: Option<String>) -> Result<String, String> {
    tag.filter(|t| {
        t.len() <= 1024
            && t.strip_prefix('"').and_then(|v| v.strip_suffix('"')).is_some_and(|v| v.bytes().all(|b| b >= 0x21 && b != b'"' && b != 0x7f))
    })
    .ok_or_else(|| tr("webdav.error.etag").into())
}
pub fn condition(tag: Option<&str>, overwrite: bool) -> Vec<(String, String)> {
    if overwrite { Vec::new() } else { vec![tag.map_or_else(|| ("If-None-Match".into(), "*".into()), |t| ("If-Match".into(), t.into()))] }
}

pub(crate) fn persist(app: &mut SolveApp, path: &str) -> Option<crate::save_job::PersistFuture> {
    let target = app.webdav.targets.borrow().get(path)?.clone();
    let http = app.services.webdav_http.clone()?;
    let local = path.to_string();
    let read = work(move || vfs::read(&local, io::MAX_DESIGN_BYTES).map_err(|_| tr("webdav.error.cache").into()));
    let headers = condition(target.etag.as_deref(), std::mem::take(&mut app.webdav.overwrite));
    let interactive = app.webdav.interactive.clone();
    let (targets, conflict, path) = (app.webdav.targets.clone(), app.webdav.conflict.clone(), path.to_string());
    Some(Box::pin(async move {
        let bytes = read.await?;
        let response = http(target.config.request("PUT", &target.url, bytes, headers, 64 << 10)).await?;
        if response.status == 412 {
            if interactive.get() {
                *conflict.borrow_mut() = true;
            }
            return Err(tr("webdav.error.conflict").into());
        }
        if !(200..300).contains(&response.status) {
            return Err(crate::i18n::format("webdav.error.http", &[("status", &response.status.to_string())]));
        }
        let tag = strong_etag(response.etag)?;
        if let Some(target) = targets.borrow_mut().get_mut(&path) {
            target.etag = Some(tag);
        }
        Ok(())
    }))
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for State {
    fn drop(&mut self) {
        for root in &self.roots {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

pub(crate) fn managed(app: &SolveApp) -> bool {
    app.session.path.as_ref().is_some_and(|path| app.webdav.targets.borrow().contains_key(path))
}
