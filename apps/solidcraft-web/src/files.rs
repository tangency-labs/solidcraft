//! Files in the browser. Designs are kept in the origin private file system (OPFS): read into
//! the engine's in-memory files (`solidcraft_engine::io::vfs`) at start, written back whenever a
//! command writes one. Files come in from the computer through a file picker (the File System
//! Access API where the browser has it, an `<input type=file>` otherwise) or a drop on the page,
//! and go out as downloads (exports, and "Download design"). Where the File System Access API
//! exists a design also lives on the computer's disk, saved and opened through the browser's file
//! dialogs (`disk.rs`). An autosave of unsaved work is kept in OPFS and offered again after a
//! reload or a crash.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use solidcraft_engine::io::vfs;

use crate::disk;
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions, FileSystemWritableFileStream};

/// Where OPFS files appear to the engine.
pub const OPFS: &str = disk::OPFS_PREFIX;
/// Paths written here are handed to the user as downloads.
pub const DOWNLOADS: &str = disk::DOWNLOADS_PREFIX;
/// Files brought in from the computer.
pub const UPLOAD: &str = "/upload/";
/// The autosave of unsaved work (in OPFS: `recovery.sc` and `recovery.json`), named with
/// `solidcraft_engine::io::DESIGN_EXT`.
const RECOVERY: &str = "recovery.sc";
/// The autosaves earlier releases kept, newest first, one per entry of
/// `solidcraft_engine::io::LEGACY_DESIGN_EXTS` (compatibility shim, docs/compatibility.md): the
/// newest is recovered when there is no current one.
const LEGACY_RECOVERY: [&str; 2] = ["recovery.solidcraft", "recovery.solvecraft"];
const RECOVERY_META: &str = "recovery.json";

/// What to do with a file brought in from the computer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intent {
    /// Open it (a design, or STEP/mesh as a new design).
    Open,
    /// Add its bodies to the open design (STEP, 3MF, STL).
    Insert,
    /// Import it as a theme: it was dropped on Preferences' Import tile (#8226).
    Theme,
}

/// A stored design, as the file panel lists it.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    /// Last written (ms since 1970; 0 when unknown).
    pub modified: f64,
}

/// State shared between the async browser calls and the app's frames.
#[derive(Default)]
pub struct Files {
    pub server: crate::server::State,
    pub entries: RefCell<Vec<Entry>>,
    /// Files read from the computer: (name, bytes, what to do).
    pub incoming: RefCell<Vec<(String, Vec<u8>, Intent)>>,
    /// Designs opened from the computer's disk: their paths in the engine's files.
    pub opened: RefCell<Vec<String>>,
    /// The ids of the files remembered from earlier sessions (see `disk::restore`).
    pub disk_known: RefCell<std::collections::HashSet<String>>,
    /// Recent designs whose file is gone: to drop from the recent list.
    pub gone: RefCell<Vec<String>>,
    /// Disk designs whose last write failed: they are not saved.
    pub unsaved: RefCell<Vec<String>>,
    /// Messages for the status line (errors and confirmations).
    pub messages: RefCell<Vec<(String, bool)>>,
    /// The stored designs are loaded (the app may open them).
    pub ready: Cell<bool>,
    /// The file panel is open.
    pub panel: Cell<bool>,
    /// An autosave left by an earlier session: its design's name and file.
    pub recovery: RefCell<Option<(String, Option<String>)>>,
    /// The app's context, to wake it when a browser call finishes.
    pub ctx: RefCell<Option<egui::Context>>,
}

impl Files {
    pub fn say(&self, text: impl Into<String>, error: bool) {
        self.messages.borrow_mut().push((text.into(), error));
        self.wake();
    }

    /// Writing the design at disk `path` failed: say so, and take its "saved" back.
    pub fn failed_disk(&self, path: &str, error: &str) {
        self.unsaved.borrow_mut().push(path.to_string());
        self.say(format!("Not saved to your computer: {error}"), true);
    }

    /// A file arrived from the computer.
    fn arrive(&self, name: String, bytes: Vec<u8>, intent: Intent) {
        self.incoming.borrow_mut().push((name, bytes, intent));
        self.wake();
    }

    /// Run a frame (the app sleeps until something happens).
    pub fn wake(&self) {
        if let Some(c) = self.ctx.borrow().as_ref() {
            c.request_repaint();
        }
    }
}

thread_local! {
    /// The private file system isn't there (an insecure page: http on a network address, or an
    /// old browser): designs are kept in local storage instead.
    static LOCAL: Cell<bool> = const { Cell::new(false) };
}

/// Local storage keys of stored files.
const LOCAL_PREFIX: &str = "solidcraft.file.";
/// Local-storage files kept by earlier releases (compatibility shim, docs/compatibility.md):
/// still listed and opened; removing one removes either key.
const LEGACY_LOCAL_PREFIX: &str = "solvecraft.file.";

fn local() -> Result<web_sys::Storage, String> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or_else(|| "this browser keeps no local storage".to_string())
}

/// Text as is, anything else as hex (local storage holds strings).
fn encode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(t) => format!("t{t}"),
        Err(_) => {
            let mut s = String::with_capacity(bytes.len() * 2 + 1);
            s.push('x');
            for b in bytes {
                s.push_str(&format!("{b:02x}"));
            }
            s
        }
    }
}

fn decode(s: &str) -> Vec<u8> {
    if let Some(t) = s.strip_prefix('t') {
        return t.as_bytes().to_vec();
    }
    let h = s.strip_prefix('x').unwrap_or("");
    (0..h.len() / 2).filter_map(|i| h.get(2 * i..2 * i + 2).and_then(|x| u8::from_str_radix(x, 16).ok())).collect()
}

/// Is the browser's storage local storage (no OPFS here)?
pub fn local_only() -> bool {
    LOCAL.with(Cell::get)
}

pub(crate) fn err(e: JsValue) -> String {
    e.as_string().or_else(|| Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string())).unwrap_or_else(|| format!("{e:?}"))
}

/// The browser's storage manager: missing on insecure pages (http on a network address) and in
/// old browsers, where calling into it would throw.
fn storage_manager() -> Option<web_sys::StorageManager> {
    let nav = web_sys::window()?.navigator();
    let sm = Reflect::get(&nav, &"storage".into()).ok().filter(|v| v.is_object())?;
    Some(sm.unchecked_into())
}

/// Does the storage manager have `method`?
fn has(sm: &web_sys::StorageManager, method: &str) -> bool {
    Reflect::get(sm, &method.into()).is_ok_and(|f| f.is_function())
}

async fn root() -> Result<FileSystemDirectoryHandle, String> {
    let sm = storage_manager().filter(|sm| has(sm, "getDirectory")).ok_or("this page has no private file storage (it needs https or localhost)")?;
    let dir = JsFuture::from(sm.get_directory()).await.map_err(err)?;
    dir.dyn_into::<FileSystemDirectoryHandle>().map_err(|_| "this browser has no private file storage (OPFS)".to_string())
}

pub(crate) async fn read_handle(h: &FileSystemFileHandle) -> Result<Vec<u8>, String> {
    let file: web_sys::File = JsFuture::from(h.get_file()).await.map_err(err)?.dyn_into().map_err(|_| "not a file".to_string())?;
    blob_bytes(&file).await
}

async fn blob_bytes(b: &web_sys::Blob) -> Result<Vec<u8>, String> {
    let buf = JsFuture::from(b.array_buffer()).await.map_err(err)?;
    Ok(Uint8Array::new(&buf).to_vec())
}

async fn file_handle(dir: &FileSystemDirectoryHandle, name: &str, create: bool) -> Result<FileSystemFileHandle, String> {
    let opts = FileSystemGetFileOptions::new();
    opts.set_create(create);
    JsFuture::from(dir.get_file_handle_with_options(name, &opts)).await.map_err(err)?.dyn_into().map_err(|_| "not a file".to_string())
}

async fn write_file(name: &str, bytes: &[u8]) -> Result<(), String> {
    if local_only() {
        return local()?
            .set_item(&format!("{LOCAL_PREFIX}{name}"), &encode(bytes))
            .map_err(|e| format!("local storage is full or blocked: {}", err(e)));
    }
    let dir = root().await?;
    let h = file_handle(&dir, name, true).await?;
    let w: FileSystemWritableFileStream =
        JsFuture::from(h.create_writable()).await.map_err(err)?.dyn_into().map_err(|_| "no writable stream".to_string())?;
    JsFuture::from(w.write_with_u8_array(bytes).map_err(err)?).await.map_err(err)?;
    JsFuture::from(w.close()).await.map_err(err)?;
    Ok(())
}

async fn remove_file(name: &str) -> Result<(), String> {
    if local_only() {
        let st = local()?;
        st.remove_item(&format!("{LEGACY_LOCAL_PREFIX}{name}")).map_err(err)?;
        return st.remove_item(&format!("{LOCAL_PREFIX}{name}")).map_err(err);
    }
    let dir = root().await?;
    JsFuture::from(dir.remove_entry(name)).await.map_err(err)?;
    Ok(())
}

/// Every stored file in local storage.
fn read_local() -> Result<Vec<(String, Vec<u8>, f64)>, String> {
    let st = local()?;
    let mut out = Vec::new();
    for i in 0..st.length().unwrap_or(0).min(10_000) {
        let Some(k) = st.key(i).ok().flatten() else { continue };
        let Some(name) = k.strip_prefix(LOCAL_PREFIX).or_else(|| k.strip_prefix(LEGACY_LOCAL_PREFIX)) else { continue };
        if let Some(v) = st.get_item(&k).ok().flatten() {
            out.push((name.to_string(), decode(&v), 0.0));
        }
    }
    Ok(out)
}

/// Every stored file: (name, bytes). Without OPFS, local storage takes over from here on.
async fn read_all() -> Result<Vec<(String, Vec<u8>, f64)>, String> {
    let dir = match root().await {
        Ok(d) => d,
        Err(e) => {
            log::warn!("no OPFS ({e}): designs go to local storage");
            LOCAL.with(|l| l.set(true));
            return read_local();
        }
    };
    let iter: JsValue = dir.values().into();
    let next: Function = Reflect::get(&iter, &"next".into()).map_err(err)?.dyn_into().map_err(|_| "no iterator".to_string())?;
    let mut out = Vec::new();
    for _ in 0..10_000 {
        let step = JsFuture::from(js_sys::Promise::resolve(&next.call0(&iter).map_err(err)?)).await.map_err(err)?;
        if Reflect::get(&step, &"done".into()).ok().and_then(|d| d.as_bool()).unwrap_or(true) {
            break;
        }
        let Ok(h) = Reflect::get(&step, &"value".into()).map_err(err)?.dyn_into::<FileSystemFileHandle>() else { continue };
        let file =
            match JsFuture::from(h.get_file()).await.map_err(err).and_then(|f| f.dyn_into::<web_sys::File>().map_err(|_| "not a file".to_string())) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("{}: {e}", h.name());
                    continue;
                }
            };
        match blob_bytes(&file).await {
            Ok(b) => out.push((h.name(), b, file.last_modified())),
            Err(e) => log::warn!("{}: {e}", h.name()),
        }
    }
    Ok(out)
}

/// Ask the browser to keep the storage (not evict it under pressure), then load every stored
/// file into the engine's files.
pub fn start(files: &Rc<Files>) {
    if let Some(sm) = storage_manager().filter(|sm| has(sm, "persist"))
        && let Ok(p) = sm.persist()
    {
        spawn_local(async move {
            let _ = JsFuture::from(p).await;
        });
    }
    let files = files.clone();
    spawn_local(async move {
        match disk::known_ids().await {
            Ok(ids) => files.disk_known.borrow_mut().extend(ids),
            Err(e) => log::info!("no remembered files: {e}"),
        }
        match read_all().await {
            Ok(all) => {
                if local_only() {
                    files.say("This page can't use the browser's file storage (it needs https or localhost): designs are kept in local storage, a few MB at most.", false);
                }
                let mut entries = Vec::new();
                let has_recovery = all.iter().any(|(n, _, _)| n == RECOVERY);
                let legacy = (!has_recovery).then(|| LEGACY_RECOVERY.into_iter().find(|l| all.iter().any(|(n, _, _)| n == l))).flatten();
                for (name, bytes, modified) in all {
                    if LEGACY_RECOVERY.contains(&name.as_str()) {
                        if legacy == Some(name.as_str()) {
                            vfs::insert(&format!("{OPFS}{RECOVERY}"), bytes);
                        }
                        continue;
                    }
                    if name == RECOVERY_META {
                        let meta: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
                        let n = meta["name"].as_str().unwrap_or("Untitled").to_string();
                        *files.recovery.borrow_mut() = Some((n, meta["path"].as_str().map(str::to_string)));
                        continue;
                    }
                    if name != RECOVERY {
                        entries.push(Entry { name: name.clone(), size: bytes.len() as u64, modified });
                    }
                    vfs::insert(&format!("{OPFS}{name}"), bytes);
                }
                entries.sort_by_key(|a| a.name.to_lowercase());
                *files.entries.borrow_mut() = entries;
                // An autosave without its design file is nothing to recover.
                if !vfs::exists(&format!("{OPFS}{RECOVERY}")) {
                    *files.recovery.borrow_mut() = None;
                }
            }
            Err(e) => files.say(format!("browser storage: {e}"), true),
        }
        files.ready.set(true);
        files.wake();
    });
}

/// Store what commands wrote: OPFS paths into OPFS, download paths as downloads.
pub fn flush(files: &Rc<Files>) {
    for (path, bytes) in vfs::take_writes() {
        match disk::route(&path) {
            disk::WriteTo::Opfs => {
                let name = path.strip_prefix(OPFS).unwrap_or(&path).to_string();
                {
                    let mut e = files.entries.borrow_mut();
                    e.retain(|x| x.name != name);
                    if name != RECOVERY && name != RECOVERY_META {
                        e.push(Entry { name: name.clone(), size: bytes.len() as u64, modified: js_sys::Date::now() });
                        e.sort_by_key(|a| a.name.to_lowercase());
                    }
                }
                let files = files.clone();
                spawn_local(async move {
                    if let Err(e) = write_file(&name, &bytes).await {
                        files.say(format!("{name}: {e}"), true);
                    }
                });
            }
            disk::WriteTo::Server => crate::server::enqueue(files, &path, bytes),
            disk::WriteTo::Disk => disk::enqueue(files, &path, bytes),
            disk::WriteTo::Download => {
                let name = path.strip_prefix(DOWNLOADS).unwrap_or(&path);
                if let Err(e) = download(name, &bytes) {
                    files.say(format!("{name}: {e}"), true);
                }
            }
            disk::WriteTo::Memory => {}
        }
    }
}

/// Hand `bytes` to the user as a download named `name`.
pub fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let parts = Array::new();
    parts.push(&Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(err)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(err)?;
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").map_err(err)?.dyn_into().map_err(|_| "no anchor".to_string())?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    // Let the browser start the download before the URL goes.
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
    }
    Ok(())
}

/// Delete a stored design.
pub fn delete(files: &Rc<Files>, name: &str) {
    let _ = vfs::remove(&format!("{OPFS}{name}"));
    files.entries.borrow_mut().retain(|e| e.name != name);
    let (files, name) = (files.clone(), name.to_string());
    spawn_local(async move {
        if let Err(e) = remove_file(&name).await {
            files.say(format!("{name}: {e}"), true);
        }
    });
}

/// Rename a stored design (a copy under the new name, then the old one goes: not every browser
/// can move OPFS files).
pub fn rename(files: &Rc<Files>, from: &str, to: &str) -> Result<(), String> {
    let to = to.trim();
    if to.is_empty() || to.contains('/') || to.contains('\\') || to.len() > 200 {
        return Err("give a file name without slashes".into());
    }
    if files.entries.borrow().iter().any(|e| e.name == to) {
        return Err(format!("there is already a file named {to}"));
    }
    let bytes = vfs::read(&format!("{OPFS}{from}"), solidcraft_engine::io::MAX_DESIGN_BYTES).map_err(|e| e.to_string())?;
    vfs::write(&format!("{OPFS}{to}"), &bytes).map_err(|e| e.to_string())?;
    delete(files, from);
    flush(files);
    Ok(())
}

/// Keep (or drop) the autosave of the open design.
pub fn autosave(files: &Rc<Files>, doc: Option<(&solidcraft_engine::doc::Document, Option<&str>)>) {
    let files = files.clone();
    match doc {
        Some((d, path)) => {
            let bytes = match solidcraft_engine::io::write_design_container(d) {
                Ok(bytes) => bytes,
                Err(e) => {
                    files.say(format!("autosave: {e}"), true);
                    return;
                }
            };
            let meta = serde_json::json!({"name": d.name, "path": path, "saved_at": js_sys::Date::now()}).to_string();
            spawn_local(async move {
                let r = async {
                    write_file(RECOVERY, &bytes).await?;
                    write_file(RECOVERY_META, meta.as_bytes()).await
                };
                if let Err(e) = r.await {
                    files.say(format!("autosave: {e}"), true);
                }
            });
        }
        None => spawn_local(async move {
            let _ = remove_file(RECOVERY).await;
            for legacy in LEGACY_RECOVERY {
                let _ = remove_file(legacy).await;
            }
            let _ = remove_file(RECOVERY_META).await;
        }),
    }
}

/// A stored path for `name` that no stored file has: `name`, else `name (2)`, `name (3)`…
pub fn free_name(name: &str) -> String {
    let (stem, ext) = name.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((name.to_string(), String::new()));
    for i in 1..1000 {
        let n = if i == 1 { name.to_string() } else { format!("{stem} ({i}){ext}") };
        let p = format!("{OPFS}{n}");
        if !vfs::exists(&p) {
            return p;
        }
    }
    format!("{OPFS}{stem} ({}){ext}", js_sys::Date::now() as u64)
}

/// The autosaved design's path (in the engine's files) for opening it.
pub fn recovery_path() -> String {
    format!("{OPFS}{RECOVERY}")
}

/// A recovery file lives in OPFS, but its relative links belong to the original design's
/// folder (which can be on disk). Resolve them before Open sees the temporary file location.
pub fn prepare_recovery(original_path: Option<&str>) -> Result<String, String> {
    let path = recovery_path();
    let bytes = vfs::read(&path, solidcraft_engine::io::MAX_DESIGN_BYTES).map_err(|e| e.to_string())?;
    let mut doc = solidcraft_engine::io::read_design(&bytes).map_err(|e| e.to_string())?;
    solidcraft_engine::resolve_design_links(&mut doc, original_path);
    let bytes = solidcraft_engine::io::write_design_container(&doc).map_err(|e| e.to_string())?;
    vfs::insert(&path, bytes);
    Ok(path)
}

/// Pick files on the computer: the File System Access picker where there is one, else an
/// `<input type=file>`. Read files arrive in `files.incoming`.
pub fn pick_from_disk(files: &Rc<Files>, intent: Intent) {
    let Some(window) = web_sys::window() else { return };
    let picker = Reflect::get(&window, &"showOpenFilePicker".into()).ok().and_then(|f| f.dyn_into::<Function>().ok());
    match picker {
        Some(f) => {
            // A design from the computer's disk keeps its file, so Save writes back to it.
            let on_disk = intent == Intent::Open && disk::supported();
            let opts = if on_disk {
                disk::open_options()
            } else {
                let opts = Object::new();
                let _ = Reflect::set(&opts, &"multiple".into(), &JsValue::FALSE);
                opts
            };
            let promise = f.call1(&window, &opts);
            let files = files.clone();
            spawn_local(async move {
                let r = async {
                    let p: js_sys::Promise = promise.map_err(err)?.dyn_into().map_err(|_| "no promise".to_string())?;
                    let handles: Array = JsFuture::from(p).await.map_err(err)?.dyn_into().map_err(|_| "no files".to_string())?;
                    for h in handles.iter() {
                        let h: FileSystemFileHandle = h.dyn_into().map_err(|_| "not a file".to_string())?;
                        let bytes = read_handle(&h).await?;
                        if on_disk && solidcraft_engine::io::is_design_path(&h.name()) {
                            let path = disk::arrive(&h, bytes);
                            files.opened.borrow_mut().push(path);
                            files.wake();
                        } else {
                            files.arrive(h.name(), bytes, intent);
                        }
                    }
                    Ok::<(), String>(())
                };
                match r.await {
                    Ok(()) => {}
                    // Closing the picker is no error.
                    Err(e) if e.contains("abort") || e.contains("Abort") => {}
                    Err(e) => files.say(format!("open: {e}"), true),
                }
            });
        }
        None => input_picker(files, intent),
    }
}

fn input_picker(files: &Rc<Files>, intent: Intent) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(input) = doc.create_element("input").map(|e| e.unchecked_into::<web_sys::HtmlInputElement>()) else { return };
    input.set_type("file");
    // Everything the browser build imports, from the format registry: cadmpeg is pure Rust, so
    // FreeCAD and Rhino files are part of it; plus the DXF and SVG drawings Open and Insert take.
    let mut extensions = solidcraft_engine::io::formats::open_extensions(true);
    extensions.extend(["dxf", "svg"]);
    input.set_accept(&disk::input_accept(&extensions, apple_touch()));
    // Attached (hidden) while it is open: some browsers ignore a click on a detached input.
    let _ = input.set_attribute("style", "display:none");
    if let Some(body) = doc.body() {
        let _ = body.append_child(&input);
    }
    let (files, el) = (files.clone(), input.clone());
    let on_change = Closure::once_into_js(move || {
        el.remove();
        let Some(list) = el.files() else { return };
        for i in 0..list.length() {
            let Some(f) = list.get(i) else { continue };
            let files = files.clone();
            spawn_local(async move {
                match blob_bytes(&f).await {
                    Ok(b) => files.arrive(f.name(), b, intent),
                    Err(e) => files.say(format!("{}: {e}", f.name()), true),
                }
            });
        }
    });
    input.set_onchange(Some(on_change.unchecked_ref()));
    input.click();
}

/// Ask for one file of these types (`extensions`, e.g. `["dxf"]`) with a plain file input, the
/// dialog every browser has. The answer is the file's path in the engine's files (under
/// [`UPLOAD`], as for a file picked in the disk dialog), so the command that asked runs on it;
/// `None` when the dialog was closed or the file could not be read.
pub fn pick_upload(files: &Rc<Files>, extensions: &[&str]) -> impl std::future::Future<Output = Option<String>> + use<> {
    // Shown now, in the click: a file dialog needs the user's gesture.
    let chosen = choose_file(&disk::input_accept(extensions, apple_touch()));
    let limit = disk::upload_limit(extensions);
    let files = files.clone();
    async move {
        let read = async {
            let value = JsFuture::from(chosen?).await.map_err(err)?;
            // Closing the dialog answers no file: nothing to say.
            let Ok(file) = value.dyn_into::<web_sys::File>() else { return Ok(None) };
            let name = file.name();
            if limit.is_some_and(|max| !file.size().is_finite() || file.size() > max as f64) {
                return Err(format!("{name}: the file is too large"));
            }
            let bytes = blob_bytes(&file).await.map_err(|e| format!("{name}: {e}"))?;
            let path = format!("{UPLOAD}{name}");
            vfs::insert(&path, bytes);
            Ok(Some(path))
        };
        match read.await {
            Ok(path) => path,
            Err(e) => {
                files.say(format!("open: {e}"), true);
                None
            }
        }
    }
}

/// The page runs on an iPhone or iPad, whose file inputs offer every file (`disk::input_accept`).
fn apple_touch() -> bool {
    web_sys::window().is_some_and(|w| {
        let nav = w.navigator();
        disk::apple_touch(&nav.user_agent().unwrap_or_default(), nav.max_touch_points())
    })
}

/// Show a hidden `<input type=file>` offering `accept` (call in the click). The promise answers
/// the picked `File`, or `null` when the dialog is closed (the input's `cancel` event).
fn choose_file(accept: &str) -> Result<js_sys::Promise, String> {
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("this page can't show a file dialog")?;
    let input = doc.create_element("input").map_err(err)?.dyn_into::<web_sys::HtmlInputElement>().map_err(|_| "no file input".to_string())?;
    input.set_type("file");
    input.set_accept(accept);
    // Attached (hidden) while it is open: some browsers ignore a click on a detached input.
    input.set_attribute("style", "display:none").map_err(err)?;
    doc.body().ok_or("this page has no body")?.append_child(&input).map_err(err)?;
    let promise = js_sys::Promise::new(&mut |resolve: Function, _reject: Function| {
        // One answer for a pick or a cancel; read from the event, so it does not keep the input.
        let done = Closure::once_into_js(move |event: web_sys::Event| {
            let file = event.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).and_then(|input| {
                input.set_onchange(None);
                input.set_oncancel(None);
                input.remove();
                input.files().and_then(|list| list.get(0))
            });
            let _ = resolve.call1(&JsValue::UNDEFINED, &file.map_or(JsValue::NULL, JsValue::from));
        });
        input.set_onchange(Some(done.unchecked_ref()));
        input.set_oncancel(Some(done.unchecked_ref()));
    });
    input.click();
    Ok(promise)
}

/// Read files dropped on the page.
/// `theme`: the pointer is over Preferences' Import tile, so the files are themes.
pub fn take_drops(files: &Rc<Files>, raw: &mut egui::RawInput, theme: bool) {
    let intent = if theme { Intent::Theme } else { Intent::Insert };
    for f in std::mem::take(&mut raw.dropped_files) {
        let files = files.clone();
        spawn_local(async move {
            let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped".into());
            match f.bytes_async().await {
                Ok(b) => files.arrive(name, b, intent),
                Err(e) => files.say(format!("{name}: {e}"), true),
            }
        });
    }
}
