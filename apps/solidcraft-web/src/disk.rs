//! Designs on the computer's own disk, through the File System Access API (Chrome, Edge, Opera):
//! Save As and Open show the browser's file dialogs, and the chosen file's handle is kept (in
//! memory, and in IndexedDB so Save still reaches the file after a reload) so Save writes back to
//! it. To the engine such a design is the file `/disk/<id>/<name>`: it saves there like anywhere,
//! and [`flush`](crate::files::flush) forwards what it wrote to the handle. Browsers without the
//! API (Firefox, Safari) keep designs in the origin private file system (`files.rs`).
//!
//! The path logic is plain Rust and tested natively; the browser calls are wasm-only.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

/// Where a design on the computer's disk appears to the engine.
pub const PREFIX: &str = "/disk/";

/// Where designs kept in the browser's private storage (OPFS) appear to the engine.
pub const OPFS_PREFIX: &str = "/opfs/";
/// Paths written here are handed to the user as downloads.
pub const DOWNLOADS_PREFIX: &str = "/downloads/";

/// Where what a command wrote to an engine path goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteTo {
    /// The browser's private storage: the design (or recovery file) keeps its name there.
    Opfs,
    /// A file on the computer, through its handle.
    Disk,
    /// A download.
    Download,
    /// Kept only in memory (uploads).
    Memory,
}

/// Route a write by its path. OPFS paths always go to OPFS, whatever the browser can do: a design
/// opened from the browser's storage keeps saving there until the user picks Save As.
pub fn route(path: &str) -> WriteTo {
    if path.starts_with(OPFS_PREFIX) {
        WriteTo::Opfs
    } else if parse(path).is_some() {
        WriteTo::Disk
    } else if path.starts_with(DOWNLOADS_PREFIX) {
        WriteTo::Download
    } else {
        WriteTo::Memory
    }
}

/// The engine path of the design `name` kept under handle `id`.
pub fn path(id: &str, name: &str) -> String {
    format!("{PREFIX}{id}/{name}")
}

/// The handle id and file name of an engine path made by [`path`].
pub fn parse(path: &str) -> Option<(&str, &str)> {
    let (id, name) = path.strip_prefix(PREFIX)?.split_once('/')?;
    (!id.is_empty() && !name.is_empty() && !name.contains('/')).then_some((id, name))
}

/// A handle id from the time and a random number.
pub fn make_id(ms: u64, random: u64) -> String {
    format!("{ms:x}-{random:x}")
}

/// How long a remembered file may go unused, and unreferenced by the recent designs and the
/// autosave, before the browser's copy of its handle is dropped.
pub const HANDLE_MAX_AGE_MS: f64 = 90.0 * 24.0 * 3600.0 * 1000.0;

/// What to do with the remembered handles.
#[derive(Debug, Default, PartialEq)]
pub struct PrunePlan {
    /// Handles to forget.
    pub delete: Vec<String>,
    /// Handles seen unused for the first time: start their clock.
    pub stamp: Vec<String>,
}

/// Decide which of the remembered handles `ids` (id, when it was last stamped) to forget: those
/// that no recent design or autosave (`keep`) names and that have gone unused for `max_age`. One
/// never stamped starts its clock now, so a file another tab has just saved is never dropped.
pub fn plan_prune(ids: &[(String, Option<f64>)], keep: &std::collections::HashSet<String>, now: f64, max_age: f64) -> PrunePlan {
    let mut plan = PrunePlan::default();
    for (id, stamped) in ids.iter().filter(|(id, _)| !keep.contains(id)) {
        match stamped {
            None => plan.stamp.push(id.clone()),
            Some(at) if now - at > max_age => plan.delete.push(id.clone()),
            Some(_) => {}
        }
    }
    plan
}

/// Where a design saved with Save As goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTarget {
    /// A file the user picks on their computer.
    Disk,
    /// The browser's private storage (OPFS): what browsers without the API have always done.
    Browser,
    /// Not a design (an export): handed over as a download.
    Download,
}

/// Choose where a Save As goes: designs to the disk when the browser can pick a file, else into
/// the browser's storage; anything else is a download.
pub fn save_target(api: bool, design: bool) -> SaveTarget {
    match (design, api) {
        (false, _) => SaveTarget::Download,
        (true, true) => SaveTarget::Disk,
        (true, false) => SaveTarget::Browser,
    }
}

/// Does a picker for `extensions` (what the command reads; `&[]`: anything) belong to a design?
/// Only those go through the disk dialog; inserting a STEP or mesh keeps using the file window.
pub fn opens_design(extensions: &[&str]) -> bool {
    extensions.is_empty() || extensions.iter().any(|e| solidcraft_engine::io::DESIGN_EXTS.contains(e))
}

/// Where an Open for `extensions` (what the command reads; `&[]`: anything) is answered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenRoute {
    /// The browser's open dialog for a design: the file keeps its handle, so Save writes back.
    Disk,
    /// The "Files in this browser" window, where the browser can't open a design from the disk.
    Panel,
    /// A plain file input that offers these types: the file is read into the engine's files and
    /// the command runs on it (Insert DXF or SVG, images, meshes, imported settings).
    Upload,
}

/// Choose how an Open for `extensions` asks for its file; `api`: the browser can open a file
/// from the disk and keep its handle.
pub fn open_route(api: bool, extensions: &[&str]) -> OpenRoute {
    match (opens_design(extensions), api) {
        (true, true) => OpenRoute::Disk,
        (true, false) => OpenRoute::Panel,
        (false, _) => OpenRoute::Upload,
    }
}

/// The `accept` list of a file input for `extensions` (with or without their dot): ".dxf,.svg".
pub fn accept_list(extensions: &[&str]) -> String {
    let list: Vec<String> = extensions
        .iter()
        .map(|e| e.trim().trim_start_matches('.'))
        .filter(|e| !e.is_empty())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .collect();
    list.join(",")
}

/// The `accept` of a file input for `extensions` on this device (`apple_touch`: an iPhone or
/// iPad). The picker there offers only files of a type the system knows from the list and greys
/// out the rest, `.solidcraft` designs among them, since no system type has that extension
/// (#6977). So it offers every file there, and the opener checks what it gets.
pub fn input_accept(extensions: &[&str], apple_touch: bool) -> String {
    if apple_touch { String::new() } else { accept_list(extensions) }
}

/// The browser runs on an iPhone or iPad (every browser there is WebKit), from its
/// `navigator.userAgent` and `navigator.maxTouchPoints`. iPadOS presents itself as a Mac; only its
/// touch screen tells them apart.
pub fn apple_touch(user_agent: &str, touch_points: i32) -> bool {
    ["iPhone", "iPad", "iPod"].iter().any(|d| user_agent.contains(d)) || (user_agent.contains("Macintosh") && touch_points > 1)
}

/// The largest file an upload for `extensions` reads: images go into a canvas, which has a size
/// limit, so a bigger one is refused before it is read. `None`: the importer checks the file.
pub fn upload_limit(extensions: &[&str]) -> Option<usize> {
    let image = |e: &&str| ["png", "jpg", "jpeg"].contains(&e.trim_start_matches('.').to_ascii_lowercase().as_str());
    (!extensions.is_empty() && extensions.iter().all(image)).then_some(solidcraft_engine::doc::canvas::MAX_CANVAS_BYTES)
}

/// The file type a Save As dialog offers for `name`: (description, MIME type, extension).
pub fn save_type(name: &str) -> (&'static str, &'static str, &'static str) {
    let json = std::path::Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if json { ("Plain JSON design", "application/json", ".json") } else { ("SolidCraft design", "application/octet-stream", ".solidcraft") }
}

/// Is this a cancelled dialog (the browser's `AbortError`)? Not an error to show.
pub fn is_abort(error_name: &str) -> bool {
    error_name == "AbortError"
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, VecDeque};
    use std::rc::Rc;

    use js_sys::{Array, Function, Object, Promise, Reflect};
    use solidcraft_engine::io::vfs;
    use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{FileSystemFileHandle, IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransactionMode};

    use super::{HANDLE_MAX_AGE_MS, is_abort, make_id, parse, path, plan_prune, save_type};
    use crate::files::{Files, err, read_handle};

    const DB: &str = "solidcraft-disk-handles";
    const STORE: &str = "handles";

    thread_local! {
        /// Handles picked or looked up in this session, by id.
        static HANDLES: RefCell<HashMap<String, FileSystemFileHandle>> = RefCell::new(HashMap::new());
        /// Writes waiting for their turn (a file takes one writer at a time), latest per id.
        static QUEUE: RefCell<VecDeque<(String, Vec<u8>)>> = const { RefCell::new(VecDeque::new()) };
        static BUSY: Cell<bool> = const { Cell::new(false) };
    }

    /// Why a dialog gave no file.
    enum Pick {
        Cancelled,
        Failed(String),
    }

    fn failed(e: JsValue) -> Pick {
        let name = Reflect::get(&e, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
        if is_abort(&name) { Pick::Cancelled } else { Pick::Failed(err(e)) }
    }

    /// Can this page show the browser's file dialogs and write to the files they pick?
    pub fn supported() -> bool {
        let Some(window) = web_sys::window() else { return false };
        let has = |name: &str| Reflect::get(&window, &name.into()).is_ok_and(|f| f.is_function());
        window.is_secure_context() && has("showSaveFilePicker") && has("showOpenFilePicker")
    }

    fn types(description: &str, mime: &str, extension: &str) -> Array {
        let accept = Object::new();
        let _ = Reflect::set(&accept, &mime.into(), &Array::of1(&extension.into()));
        let entry = Object::new();
        let _ = Reflect::set(&entry, &"description".into(), &description.into());
        let _ = Reflect::set(&entry, &"accept".into(), &accept);
        Array::of1(&entry)
    }

    /// Start a dialog. This must run in the click that asked for it (the browser wants a user
    /// gesture), so it is called before anything is awaited.
    fn start_picker(function: &str, options: &Object) -> Result<Promise, Pick> {
        let window = web_sys::window().ok_or_else(|| Pick::Failed("no window".into()))?;
        let f: Function = Reflect::get(&window, &function.into())
            .map_err(failed)?
            .dyn_into()
            .map_err(|_| Pick::Failed("this browser can't open file dialogs".into()))?;
        f.call1(&window, options).map_err(failed)?.dyn_into().map_err(|_| Pick::Failed("no answer from the file dialog".into()))
    }

    /// Remember a picked file: in memory now, in IndexedDB for the next session.
    fn register(handle: &FileSystemFileHandle) -> String {
        let id = make_id(js_sys::Date::now() as u64, (js_sys::Math::random() * 1e15) as u64);
        HANDLES.with(|h| h.borrow_mut().insert(id.clone(), handle.clone()));
        let (key, kept, name) = (id.clone(), handle.clone(), handle.name());
        spawn_local(async move {
            if let Err(e) = remember(&key, &kept).await {
                log::warn!("the file's handle isn't kept for the next session: {e}");
            }
        });
        path(&id, &name)
    }

    fn save_options(name: &str) -> Object {
        let options = Object::new();
        let _ = Reflect::set(&options, &"suggestedName".into(), &name.into());
        let (description, mime, extension) = save_type(name);
        let _ = Reflect::set(&options, &"types".into(), &types(description, mime, extension));
        options
    }

    /// "Save to computer…" for a design kept in the browser: write a copy of `bytes` to a file the
    /// user picks (call in the click), or download it where the browser can't pick one. The design
    /// itself stays where it is.
    pub fn save_copy(files: &Rc<Files>, name: &str, bytes: Vec<u8>) {
        if !supported() {
            if let Err(e) = crate::files::download(name, &bytes) {
                files.say(format!("{name}: {e}"), true);
            }
            return;
        }
        let promise = start_picker("showSaveFilePicker", &save_options(name));
        let files = files.clone();
        spawn_local(async move {
            let r = async {
                let h: FileSystemFileHandle =
                    JsFuture::from(promise?).await.map_err(failed)?.dyn_into().map_err(|_| Pick::Failed("not a file".into()))?;
                write_handle(&h, &bytes).await.map_err(Pick::Failed)?;
                Ok::<String, Pick>(h.name())
            };
            match r.await {
                Ok(saved) => files.say(format!("Saved a copy to {saved}"), false),
                Err(Pick::Cancelled) => {}
                Err(Pick::Failed(e)) => files.say(format!("save to computer: {e}"), true),
            }
        });
    }

    /// Save As, step one: show the browser's save dialog (call in the click). The answer is the
    /// engine path the design is saved to, `None` when the dialog was closed.
    pub fn pick_save(files: &Rc<Files>, name: &str) -> impl std::future::Future<Output = Option<String>> + use<> {
        let promise = start_picker("showSaveFilePicker", &save_options(name));
        let files = files.clone();
        async move {
            let handle = match promise {
                Ok(p) => JsFuture::from(p)
                    .await
                    .map_err(failed)
                    .and_then(|h| h.dyn_into::<FileSystemFileHandle>().map_err(|_| Pick::Failed("not a file".into()))),
                Err(e) => Err(e),
            };
            match handle {
                Ok(h) => Some(register(&h)),
                Err(Pick::Cancelled) => None,
                Err(Pick::Failed(e)) => {
                    files.say(format!("save: {e}"), true);
                    None
                }
            }
        }
    }

    /// Open, step one: show the browser's open dialog for a design (call in the click). The
    /// answer is the engine path of the file, read into the engine's files; a design keeps its
    /// handle, so Save writes back to the file. `None`: closed, or it could not be read.
    pub fn pick_open(files: &Rc<Files>) -> impl std::future::Future<Output = Option<String>> + use<> {
        let promise = start_picker("showOpenFilePicker", &open_options());
        let files = files.clone();
        async move {
            let r = async {
                let p = promise?;
                let handles: Array = JsFuture::from(p).await.map_err(failed)?.dyn_into().map_err(|_| Pick::Failed("no file".into()))?;
                let h: FileSystemFileHandle = handles.get(0).dyn_into().map_err(|_| Pick::Cancelled)?;
                let bytes = read_handle(&h).await.map_err(Pick::Failed)?;
                Ok((h, bytes))
            };
            match r.await {
                Ok((h, bytes)) => Some(arrive(&h, bytes)),
                Err(Pick::Cancelled) => None,
                Err(Pick::Failed(e)) => {
                    files.say(format!("open: {e}"), true);
                    None
                }
            }
        }
    }

    /// The options of the open dialog: a single file, designs and CAD files listed first.
    pub fn open_options() -> Object {
        let options = Object::new();
        let _ = Reflect::set(&options, &"multiple".into(), &JsValue::FALSE);
        let extensions = Array::new();
        for e in [".solidcraft", ".solvecraft", ".json", ".step", ".stp", ".3mf", ".stl", ".obj", ".glb", ".gltf", ".fcstd", ".3dm", ".dxf", ".svg"] {
            extensions.push(&e.into());
        }
        let accept = Object::new();
        let _ = Reflect::set(&accept, &"application/octet-stream".into(), &extensions);
        let entry = Object::new();
        let _ = Reflect::set(&entry, &"description".into(), &"Designs and CAD files".into());
        let _ = Reflect::set(&entry, &"accept".into(), &accept);
        let _ = Reflect::set(&options, &"types".into(), &Array::of1(&entry));
        options
    }

    /// A file read from a dialog's handle: a design lives at its disk path from now on (and
    /// keeps the handle); anything else is an upload to open or insert.
    pub fn arrive(handle: &FileSystemFileHandle, bytes: Vec<u8>) -> String {
        let name = handle.name();
        let p = if solidcraft_engine::io::is_design_path(&name) { register(handle) } else { format!("{}{name}", crate::files::UPLOAD) };
        vfs::insert(&p, bytes);
        p
    }

    // ---- writing ------------------------------------------------------------------------

    /// Queue what a command wrote to a disk path. Writes go one at a time; a newer write of the
    /// same file replaces one still waiting.
    pub fn enqueue(files: &Rc<Files>, path: &str, bytes: Vec<u8>) {
        QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            q.retain(|(p, _)| p != path);
            q.push_back((path.to_string(), bytes));
        });
        if BUSY.with(|b| b.replace(true)) {
            return;
        }
        let files = files.clone();
        spawn_local(async move {
            while let Some((path, bytes)) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
                if let Err(e) = write(&path, &bytes).await {
                    files.failed_disk(&path, &e);
                }
            }
            BUSY.with(|b| b.set(false));
        });
    }

    async fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
        let (id, name) = parse(path).ok_or("not a file on the computer")?;
        let handle = handle_for(id).await.map_err(|e| format!("{name}: {e}"))?;
        write_handle(&handle, bytes).await.map_err(|e| format!("{name}: {e}"))
    }

    async fn write_handle(handle: &FileSystemFileHandle, bytes: &[u8]) -> Result<(), String> {
        ensure_readwrite(handle).await?;
        let w: web_sys::FileSystemWritableFileStream =
            JsFuture::from(handle.create_writable()).await.map_err(err)?.dyn_into().map_err(|_| "no writable stream".to_string())?;
        JsFuture::from(w.write_with_u8_array(bytes).map_err(err)?).await.map_err(err)?;
        JsFuture::from(w.close()).await.map_err(err)?;
        Ok(())
    }

    async fn handle_for(id: &str) -> Result<FileSystemFileHandle, String> {
        if let Some(h) = HANDLES.with(|h| h.borrow().get(id).cloned()) {
            return Ok(h);
        }
        let h: FileSystemFileHandle = recall(id)
            .await?
            .and_then(|v| v.dyn_into().ok())
            .ok_or("the browser no longer remembers where this design was saved: use Save As to choose the file again")?;
        HANDLES.with(|m| m.borrow_mut().insert(id.to_string(), h.clone()));
        Ok(h)
    }

    /// Ask for write access if the browser doesn't hold it any more (after a reload): one prompt,
    /// from the click that asked for the save.
    async fn ensure_readwrite(handle: &FileSystemFileHandle) -> Result<(), String> {
        let options = Object::new();
        let _ = Reflect::set(&options, &"mode".into(), &"readwrite".into());
        for method in ["queryPermission", "requestPermission"] {
            let Some(f) = Reflect::get(handle, &method.into()).ok().and_then(|f| f.dyn_into::<Function>().ok()) else {
                // A browser without the permission calls hands out writable handles as they are.
                return Ok(());
            };
            let answer = f.call1(handle, &options).map_err(err)?;
            let state = JsFuture::from(Promise::resolve(&answer)).await.map_err(err)?;
            if state.as_string().as_deref() == Some("granted") {
                return Ok(());
            }
        }
        Err("the browser wasn't given permission to write the file: use Save As to choose it again".into())
    }

    // ---- IndexedDB: the handles, by id --------------------------------------------------

    /// The result of an IndexedDB request.
    async fn result(req: &IdbRequest) -> Result<JsValue, String> {
        let promise = Promise::new(&mut |resolve, reject| {
            let r = req.clone();
            let ok = Closure::once_into_js(move || {
                let _ = resolve.call1(&JsValue::NULL, &r.result().unwrap_or(JsValue::UNDEFINED));
            });
            let bad = Closure::once_into_js(move || {
                let _ = reject.call1(&JsValue::NULL, &"IndexedDB failed".into());
            });
            req.set_onsuccess(Some(ok.unchecked_ref()));
            req.set_onerror(Some(bad.unchecked_ref()));
        });
        JsFuture::from(promise).await.map_err(err)
    }

    async fn open_db() -> Result<IdbDatabase, String> {
        let factory = web_sys::window().ok_or("no window")?.indexed_db().map_err(err)?.ok_or("this browser has no IndexedDB")?;
        let request: IdbOpenDbRequest = factory.open_with_u32(DB, 1).map_err(err)?;
        let target = request.clone();
        let upgrade = Closure::once_into_js(move || {
            if let Ok(db) = target.result().and_then(|r| r.dyn_into::<IdbDatabase>()) {
                let _ = db.create_object_store(STORE);
            }
        });
        request.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
        result(&request).await?.dyn_into().map_err(|_| "no database".to_string())
    }

    fn store(db: &IdbDatabase, mode: IdbTransactionMode) -> Result<IdbObjectStore, String> {
        db.transaction_with_str_and_mode(STORE, mode).map_err(err)?.object_store(STORE).map_err(err)
    }

    /// Key of the time a handle was last used (the handle's own key is its id).
    fn stamp_key(id: &str) -> String {
        format!("at:{id}")
    }

    async fn remember(id: &str, handle: &FileSystemFileHandle) -> Result<(), String> {
        let db = open_db().await?;
        let store = store(&db, IdbTransactionMode::Readwrite)?;
        result(&store.put_with_key(handle, &id.into()).map_err(err)?).await?;
        result(&store.put_with_key(&js_sys::Date::now().into(), &stamp_key(id).into()).map_err(err)?).await.map(|_| ())
    }

    /// Note that the file was used now.
    async fn touch(id: &str) {
        let Ok(db) = open_db().await else { return };
        if let Ok(store) = store(&db, IdbTransactionMode::Readwrite)
            && let Ok(req) = store.put_with_key(&js_sys::Date::now().into(), &stamp_key(id).into())
        {
            let _ = result(&req).await;
        }
    }

    /// The ids of every remembered file.
    pub async fn known_ids() -> Result<Vec<String>, String> {
        let db = open_db().await?;
        let req = store(&db, IdbTransactionMode::Readonly)?.get_all_keys().map_err(err)?;
        let keys: Array = result(&req).await?.dyn_into().map_err(|_| "no keys".to_string())?;
        Ok(keys.iter().filter_map(|k| k.as_string()).filter(|k| !k.starts_with("at:")).collect())
    }

    async fn forget(ids: &[String]) -> Result<(), String> {
        let db = open_db().await?;
        let store = store(&db, IdbTransactionMode::Readwrite)?;
        for id in ids {
            result(&store.delete(&id.as_str().into()).map_err(err)?).await?;
            result(&store.delete(&stamp_key(id).into()).map_err(err)?).await?;
            HANDLES.with(|h| h.borrow_mut().remove(id));
        }
        Ok(())
    }

    /// Forget the remembered files nothing refers to any more (`keep`: the ids the recent designs
    /// and the autosave name), once they have gone unused for a long while.
    pub fn prune(keep: std::collections::HashSet<String>) {
        spawn_local(async move {
            let r = async {
                // A transaction ends when the page awaits anything but its own requests, so each
                // step opens its own.
                let known = known_ids().await?;
                let db = open_db().await?;
                let reading = store(&db, IdbTransactionMode::Readonly)?;
                let mut ids = Vec::new();
                for id in known {
                    let at = result(&reading.get(&stamp_key(&id).into()).map_err(err)?).await?.as_f64();
                    ids.push((id, at));
                }
                let plan = plan_prune(&ids, &keep, js_sys::Date::now(), HANDLE_MAX_AGE_MS);
                for id in &plan.stamp {
                    touch(id).await;
                }
                forget(&plan.delete).await
            };
            if let Err(e) = r.await {
                log::warn!("remembered files weren't tidied: {e}");
            }
        });
    }

    /// Open a design remembered from an earlier session (a recent one, after a reload): ask once
    /// for access to the file (call in the click), read it into the engine's files and queue it
    /// to open. A file that is gone is forgotten and dropped from the recent designs.
    pub fn restore(files: &Rc<Files>, path: &str) {
        let Some((id, name)) = parse(path) else { return };
        let (files, path, id, name) = (files.clone(), path.to_string(), id.to_string(), name.to_string());
        spawn_local(async move {
            let r = async {
                let handle = handle_for(&id).await?;
                ensure_readwrite(&handle).await?;
                read_handle(&handle).await
            };
            match r.await {
                Ok(bytes) => {
                    vfs::insert(&path, bytes);
                    files.opened.borrow_mut().push(path);
                    files.wake();
                    touch(&id).await;
                }
                Err(e) => {
                    let gone = e.contains("NotFoundError") || e.contains("could not be found") || e.contains("no longer remembers");
                    if gone {
                        let _ = forget(std::slice::from_ref(&id)).await;
                        files.disk_known.borrow_mut().remove(&id);
                        files.gone.borrow_mut().push(path);
                        files.say(format!("{name} isn't where it was saved any more"), true);
                    } else {
                        files.say(format!("{name}: {e}"), true);
                    }
                }
            }
        });
    }

    async fn recall(id: &str) -> Result<Option<JsValue>, String> {
        let db = open_db().await?;
        let req = store(&db, IdbTransactionMode::Readonly)?.get(&id.into()).map_err(err)?;
        Ok(Some(result(&req).await?).filter(|v| !v.is_undefined()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_path_round_trips_its_handle_id_and_name() {
        let p = path("18f-2a", "Bracket v2.solidcraft");
        assert_eq!(p, "/disk/18f-2a/Bracket v2.solidcraft");
        assert_eq!(parse(&p), Some(("18f-2a", "Bracket v2.solidcraft")));
    }

    #[test]
    fn only_disk_paths_parse() {
        for not in [
            "/opfs/a.solidcraft",
            "/downloads/a.step",
            "/upload/a.step",
            "/disk/",
            "/disk/id",
            "/disk/id/",
            "/disk//a",
            "/disk/id/x/y",
            "a.solidcraft",
        ] {
            assert_eq!(parse(not), None, "{not}");
        }
    }

    /// Designs already stored in OPFS (and the recovery files) keep saving to OPFS under their own
    /// names, whether or not the browser can write to the disk.
    #[test]
    fn writes_to_opfs_paths_still_go_to_opfs() {
        for p in
            ["/opfs/Bracket.solidcraft", "/opfs/Bracket (2).solidcraft", "/opfs/recovery.solidcraft", "/opfs/recovery.json", "/opfs/old.solvecraft"]
        {
            assert_eq!(route(p), WriteTo::Opfs, "{p}");
        }
        // A design opened from OPFS saves to its own path; only a Save As can pick the disk.
        assert_eq!(save_target(false, true), SaveTarget::Browser);
        assert_eq!(route(&format!("{OPFS_PREFIX}Bracket.solidcraft")), WriteTo::Opfs);
        assert_eq!(route("/disk/18f-2a/Bracket.solidcraft"), WriteTo::Disk);
        assert_eq!(route("/downloads/part.step"), WriteTo::Download);
        assert_eq!(route("/upload/part.step"), WriteTo::Memory);
    }

    #[test]
    fn only_unreferenced_handles_unused_for_long_are_forgotten() {
        let keep: std::collections::HashSet<String> = ["recent".to_string()].into();
        let day = 24.0 * 3600.0 * 1000.0;
        let now = 1000.0 * day;
        let ids = vec![
            ("recent".to_string(), Some(now - 200.0 * day)),
            ("old".to_string(), Some(now - 100.0 * day)),
            ("fresh".to_string(), Some(now - 5.0 * day)),
            ("unstamped".to_string(), None),
        ];
        let plan = plan_prune(&ids, &keep, now, HANDLE_MAX_AGE_MS);
        assert_eq!(plan, PrunePlan { delete: vec!["old".to_string()], stamp: vec!["unstamped".to_string()] });
        assert_eq!(plan_prune(&[], &keep, now, HANDLE_MAX_AGE_MS), PrunePlan::default());
    }

    #[test]
    fn handle_ids_differ_by_time_and_by_chance() {
        assert_ne!(make_id(1, 5), make_id(2, 5));
        assert_ne!(make_id(1, 5), make_id(1, 6));
        assert!(!make_id(0, 0).contains('/'));
    }

    /// Save As: a design goes to the disk where the browser can pick files and to the browser's
    /// storage where it cannot (today's behaviour); an export is a download either way.
    #[test]
    fn save_as_chooses_between_the_disk_the_browsers_storage_and_a_download() {
        assert_eq!(save_target(true, true), SaveTarget::Disk);
        assert_eq!(save_target(false, true), SaveTarget::Browser);
        assert_eq!(save_target(true, false), SaveTarget::Download);
        assert_eq!(save_target(false, false), SaveTarget::Download);
    }

    #[test]
    fn only_design_opens_use_the_disk_dialog() {
        assert!(opens_design(&[]));
        assert!(opens_design(&["solidcraft"]));
        assert!(opens_design(&["solvecraft"]));
        assert!(!opens_design(&["step", "stp"]));
        assert!(!opens_design(&["json"]));
    }

    /// Insert DXF or SVG, images and other non-design files get a file input that offers their
    /// types in every browser (#2052); designs keep the disk dialog or the browser's files.
    #[test]
    fn opens_of_other_files_use_a_file_input_for_their_types() {
        assert_eq!(open_route(true, &["dxf"]), OpenRoute::Upload);
        assert_eq!(open_route(false, &["png", "jpg", "jpeg"]), OpenRoute::Upload);
        assert_eq!(open_route(true, &[]), OpenRoute::Disk);
        assert_eq!(open_route(false, &["solidcraft"]), OpenRoute::Panel);
        assert_eq!(accept_list(&["dxf"]), ".dxf");
        assert_eq!(accept_list(&["png", ".JPG", "", "jpeg"]), ".png,.jpg,.jpeg");
        assert_eq!(accept_list(&[]), "");
        assert!(upload_limit(&["png", "jpg", "jpeg"]).is_some());
        assert_eq!(upload_limit(&["dxf"]), None);
    }

    /// An iPhone or iPad greys out a `.solidcraft` design the `accept` list names, as it knows no
    /// type for that extension, so its inputs offer every file; other devices keep the list (#6977).
    #[test]
    fn file_inputs_on_an_iphone_or_ipad_offer_every_file() {
        let iphone =
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1";
        let mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";
        let chrome_ios = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/138.0.7204.156 Mobile/15E148 Safari/604.1";
        let chrome = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
        assert!(apple_touch(iphone, 5));
        assert!(apple_touch(chrome_ios, 5), "every browser on iOS has the same picker");
        assert!(apple_touch(mac, 5), "an iPad presents itself as a Mac");
        assert!(!apple_touch(mac, 0), "a Mac has no touch screen");
        assert!(!apple_touch(chrome, 10), "a touch screen elsewhere filters as before");
        assert!(!apple_touch("", 0), "hostile input is just not an iPhone");
        let designs = ["solidcraft", "json", "step"];
        assert_eq!(input_accept(&designs, true), "");
        assert_eq!(input_accept(&designs, false), ".solidcraft,.json,.step");
    }

    #[test]
    fn the_save_dialog_offers_the_type_of_the_name() {
        assert_eq!(save_type("Plate.solidcraft").2, ".solidcraft");
        assert_eq!(save_type("Plate.JSON").2, ".json");
        assert_eq!(save_type("Plate").2, ".solidcraft");
    }

    #[test]
    fn closing_a_dialog_is_not_an_error() {
        assert!(is_abort("AbortError"));
        assert!(!is_abort("NotAllowedError"));
        assert!(!is_abort("SecurityError"));
    }
}
