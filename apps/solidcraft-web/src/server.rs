//! Designs in the folder of the SolidCraft server the page came from (`solidcraft-cli serve`,
//! docs/server-edition.md). When the page's origin answers `api/v1/health`, a folder browser
//! ("Files on this server") shows that folder one level at a time: open a design, make a folder,
//! and pick where Save As puts one, like a desktop file dialog backed by the server's folder.
//! To the engine such a design is the file `/server/<path>`; [`flush`](crate::files::flush)
//! forwards what it wrote there to the server, refused (412) when someone saved that file since it
//! was read, so a save never silently overwrites another. Without a server nothing changes.
//!
//! The token is asked once and kept in this browser's local storage for the page's origin. It is
//! only ever sent to that origin, and only over HTTPS or to this computer (loopback).
//!
//! The path logic is plain Rust and tested natively; the browser calls are wasm-only.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

/// Where a design on the server appears to the engine.
pub const PREFIX: &str = "/server/";
/// The local storage key of the server's token.
pub const TOKEN_KEY: &str = "solidcraft.server.token";

/// The server path of an engine path on the server.
pub fn parse(path: &str) -> Option<&str> {
    path.strip_prefix(PREFIX).filter(|p| !p.is_empty())
}

/// The engine path of the server file `rel`.
pub fn path(rel: &str) -> String {
    format!("{PREFIX}{rel}")
}

/// `rel` percent-encoded name by name, the `/` between them kept.
fn encode(rel: &str) -> String {
    let parts: Vec<String> =
        rel.split('/').map(|s| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()).collect();
    parts.join("/")
}

/// The API address of the server file `rel`, relative to the page.
pub fn file_url(rel: &str) -> String {
    format!("api/v1/files/{}", encode(rel))
}

/// The API address of the server folder `rel` (`""`: the top), relative to the page.
pub fn folder_url(rel: &str) -> String {
    if rel.is_empty() { "api/v1/folder".into() } else { format!("api/v1/folder/{}", encode(rel)) }
}

/// `name` inside the folder `dir` (`""`: the top).
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }
}

/// The folder above `dir` (`""` stays the top).
pub fn parent(dir: &str) -> String {
    dir.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default()
}

/// The path bar: each folder from the top down to `dir`, as (name, path); the top is "".
pub fn crumbs(dir: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut at = String::new();
    for name in dir.split('/').filter(|n| !n.is_empty()) {
        at = join(&at, name);
        out.push((name.to_string(), at.clone()));
    }
    out
}

/// Whether the token may be sent to a page served over `protocol` from `hostname`: over HTTPS,
/// or to this computer. Anything else would send it in the clear.
pub fn may_send_token(protocol: &str, hostname: &str) -> bool {
    protocol == "https:" || matches!(hostname, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// A design file in a server folder, as the folder browser lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub etag: String,
}

/// A folder's subfolders (name, path) and design files, from a `GET api/v1/folder` reply;
/// anything malformed is left out.
pub fn listing(reply: &serde_json::Value) -> (Vec<(String, String)>, Vec<Entry>) {
    let list = |key: &str| reply.get(key).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_str).map(String::from);
    let folders = list("folders").iter().filter_map(|f| Some((text(f, "name")?, text(f, "path")?))).collect();
    let files = list("files")
        .iter()
        .filter_map(|f| Some(Entry { name: text(f, "name")?, path: text(f, "path")?, size: f.get("size")?.as_u64()?, etag: text(f, "etag")? }))
        .collect();
    (folders, files)
}

/// What a failed request says to the user, by its status.
pub fn status_text(status: u16, rel: &str, server_said: &str) -> String {
    match status {
        401 => "The server refused the token: enter it again in Files on this server.".into(),
        404 => format!("{rel} is not on the server any more."),
        412 => format!("{rel} was saved on the server by someone else since you opened it. Save As under another name to keep your changes."),
        413 => format!("{rel} is larger than the server accepts."),
        _ if server_said.is_empty() => format!("{rel}: the server answered {status}."),
        _ => format!("{rel}: {server_said}"),
    }
}

/// A file or folder name typed in the browser, as the server takes it: no folder parts, no
/// leading dots (hidden), no `:` or control characters. `None`: nothing usable is left.
pub fn clean_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim().trim_start_matches('.');
    let clean: String = base.chars().filter(|c| !c.is_control() && *c != ':').collect();
    let clean = clean.trim_end_matches([' ', '.']).to_string();
    (!clean.is_empty()).then_some(clean)
}

/// The name a design gets: `name`, cleaned, with the design extension unless it has one.
pub fn design_name(name: &str) -> String {
    let ext = solidcraft_engine::io::DESIGN_EXT;
    match clean_name(name) {
        Some(n) if solidcraft_engine::io::is_design_path(&n) => n,
        Some(n) => format!("{n}.{ext}"),
        None => format!("Untitled.{ext}"),
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, VecDeque};
    use std::rc::Rc;

    use js_sys::{Function, Promise, Uint8Array};
    use solidcraft_engine::io::vfs;
    use wasm_bindgen::{JsCast as _, JsValue};
    use wasm_bindgen_futures::{JsFuture, spawn_local};

    use super::{Entry, PREFIX, TOKEN_KEY, file_url, folder_url, listing, may_send_token, parse, path, status_text};
    use crate::files::{Files, err};

    thread_local! {
        /// Writes waiting for their turn, latest per path: one at a time, so a save's `If-Match`
        /// is always the version the previous save made.
        static QUEUE: RefCell<VecDeque<(String, Vec<u8>)>> = const { RefCell::new(VecDeque::new()) };
        static BUSY: Cell<bool> = const { Cell::new(false) };
    }

    /// A Save As waiting for the folder browser: the name to save under, and how to answer.
    pub struct SaveAs {
        pub name: String,
        /// The name in the field already names a file there; the next click replaces it.
        pub confirm: bool,
        resolve: Function,
    }

    /// The server, as the page knows it.
    #[derive(Default)]
    pub struct Server {
        /// The page's origin is a SolidCraft server (`None`: not asked yet).
        pub present: Cell<Option<bool>>,
        /// The server refused the token, or there is none yet.
        pub needs_token: Cell<bool>,
        /// The folder shown (`""`: the top of the server's folder).
        pub dir: RefCell<String>,
        /// Its subfolders (name, path) and design files, as last listed.
        pub folders: RefCell<Vec<(String, String)>>,
        pub entries: RefCell<Vec<Entry>>,
        /// A listing is under way.
        pub listing: Cell<bool>,
        /// The window is open.
        pub panel: Cell<bool>,
        /// What the token field holds.
        pub token_input: RefCell<String>,
        /// What the new folder field holds (`None`: not shown).
        pub new_folder: RefCell<Option<String>>,
        /// A Save As choosing its place here.
        pub save_as: RefCell<Option<SaveAs>>,
        /// The version of each server file this page read or wrote, by its server path.
        pub etags: RefCell<HashMap<String, String>>,
        /// The design `serve --dir DESIGN.sc` named was asked for (once per page).
        pub opened_start: Cell<bool>,
    }

    impl Server {
        pub fn available(&self) -> bool {
            self.present.get() == Some(true)
        }
    }

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    fn token() -> Option<String> {
        storage()?.get_item(TOKEN_KEY).ok().flatten().filter(|t| !t.is_empty())
    }

    /// Keep `token` for this origin, or forget it (`None`).
    pub fn set_token(token: Option<&str>) {
        if let Some(s) = storage() {
            let _ = match token {
                Some(t) => s.set_item(TOKEN_KEY, t.trim()),
                None => s.remove_item(TOKEN_KEY),
            };
        }
    }

    /// The token may go to this page's origin.
    pub fn secure_enough() -> bool {
        let Some(location) = web_sys::window().map(|w| w.location()) else { return false };
        may_send_token(&location.protocol().unwrap_or_default(), &location.hostname().unwrap_or_default())
    }

    struct Reply {
        status: u16,
        etag: Option<String>,
        bytes: Vec<u8>,
    }

    impl Reply {
        fn json(&self) -> serde_json::Value {
            serde_json::from_slice(&self.bytes).unwrap_or_default()
        }
        fn said(&self) -> String {
            self.json().get("error").and_then(|e| e.as_str()).map(String::from).unwrap_or_default()
        }
    }

    async fn request(method: &str, url: &str, body: Option<&[u8]>, extra: &[(&str, &str)]) -> Result<Reply, String> {
        let window = web_sys::window().ok_or("no window")?;
        let headers = web_sys::Headers::new().map_err(err)?;
        if url != "api/v1/health" {
            if !secure_enough() {
                return Err("This server is reached over plain HTTP, so the page won't send it the token. Reach it over HTTPS.".into());
            }
            // Without one the server answers 401 (or serves it, started with --no-token).
            if let Some(token) = token() {
                headers.set("Authorization", &format!("Bearer {token}")).map_err(err)?;
            }
        }
        for (n, v) in extra {
            headers.set(n, v).map_err(err)?;
        }
        let init = web_sys::RequestInit::new();
        init.set_method(method);
        init.set_headers(&headers);
        init.set_cache(web_sys::RequestCache::NoStore);
        init.set_credentials(web_sys::RequestCredentials::Omit);
        let array;
        if let Some(b) = body {
            array = Uint8Array::from(b);
            init.set_body(&array);
        }
        let response = JsFuture::from(window.fetch_with_str_and_init(url, &init)).await.map_err(|_| "the server could not be reached".to_string())?;
        let response: web_sys::Response = response.dyn_into().map_err(|_| "no response".to_string())?;
        let etag = response.headers().get("ETag").ok().flatten();
        let buffer = JsFuture::from(response.array_buffer().map_err(err)?).await.map_err(err)?;
        Ok(Reply { status: response.status(), etag, bytes: Uint8Array::new(&buffer).to_vec() })
    }

    /// Ask the page's origin whether it is a SolidCraft server; on one, show its folder.
    pub fn detect(files: &Rc<Files>) {
        let files = files.clone();
        spawn_local(async move {
            let present = matches!(request("GET", "api/v1/health", None, &[]).await, Ok(r) if r.status == 200
                && r.json().get("ok").and_then(serde_json::Value::as_bool) == Some(true));
            files.server.present.set(Some(present));
            if present {
                files.server.panel.set(true);
                refresh(&files);
            }
            files.wake();
        });
    }

    /// Show the folder `dir`.
    pub fn go(files: &Rc<Files>, dir: &str) {
        *files.server.dir.borrow_mut() = dir.to_string();
        if let Some(save) = files.server.save_as.borrow_mut().as_mut() {
            save.confirm = false;
        }
        files.server.folders.borrow_mut().clear();
        files.server.entries.borrow_mut().clear();
        refresh(files);
    }

    /// List the folder shown again; the first time the token works, open the design
    /// `serve --dir DESIGN.sc` named.
    pub fn refresh(files: &Rc<Files>) {
        if files.server.listing.replace(true) {
            return;
        }
        let files = files.clone();
        spawn_local(async move {
            let dir = files.server.dir.borrow().clone();
            match request("GET", &folder_url(&dir), None, &[]).await {
                Ok(r) if r.status == 200 => {
                    let (folders, entries) = listing(&r.json());
                    *files.server.folders.borrow_mut() = folders;
                    *files.server.entries.borrow_mut() = entries;
                    files.server.needs_token.set(false);
                    if !files.server.opened_start.replace(true)
                        && let Ok(info) = request("GET", "api/v1/info", None, &[]).await
                        && let Some(start) = info.json().get("open").and_then(serde_json::Value::as_str).map(String::from)
                    {
                        open(&files, &start);
                    }
                }
                Ok(r) if r.status == 401 => files.server.needs_token.set(true),
                // The folder went away: back to the top.
                Ok(r) if r.status == 404 && !dir.is_empty() => {
                    files.server.listing.set(false);
                    go(&files, "");
                    return;
                }
                Ok(r) => files.say(status_text(r.status, "The folder", &r.said()), true),
                Err(e) => files.say(e, true),
            }
            files.server.listing.set(false);
            files.wake();
        });
    }

    /// Make the folder `name` in the folder shown.
    pub fn make_folder(files: &Rc<Files>, name: &str) {
        let Some(name) = super::clean_name(name) else { return };
        let rel = super::join(&files.server.dir.borrow(), &name);
        let files = files.clone();
        spawn_local(async move {
            match request("PUT", &folder_url(&rel), None, &[]).await {
                Ok(r) if r.status == 200 || r.status == 201 => go(&files, &rel),
                Ok(r) => files.say(status_text(r.status, &rel, &r.said()), true),
                Err(e) => files.say(e, true),
            }
            files.wake();
        });
    }

    /// Read the server file `rel` into the engine's files and open it.
    pub fn open(files: &Rc<Files>, rel: &str) {
        let (files, rel) = (files.clone(), rel.to_string());
        spawn_local(async move {
            match request("GET", &file_url(&rel), None, &[]).await {
                Ok(r) if r.status == 200 => {
                    let p = path(&rel);
                    if let Some(tag) = r.etag {
                        files.server.etags.borrow_mut().insert(rel.clone(), tag);
                    }
                    vfs::insert(&p, r.bytes);
                    files.opened.borrow_mut().push(p);
                    files.server.panel.set(false);
                }
                Ok(r) => {
                    if r.status == 401 {
                        files.server.needs_token.set(true);
                    }
                    files.say(status_text(r.status, &rel, &r.said()), true);
                }
                Err(e) => files.say(e, true),
            }
            files.wake();
        });
    }

    /// Save As, step one, on a server: show the folder browser to choose the folder and the name.
    /// The answer is the engine path to save to, `None` when the window was closed.
    pub fn pick_save(files: &Rc<Files>, name: &str) -> impl std::future::Future<Output = Option<String>> + use<> {
        let mut resolve = None;
        let promise = Promise::new(&mut |ok, _| resolve = Some(ok));
        if let Some(resolve) = resolve {
            // A Save As still waiting is answered "closed" first.
            if let Some(old) = files.server.save_as.borrow_mut().replace(SaveAs { name: super::design_name(name), confirm: false, resolve }) {
                let _ = old.resolve.call1(&JsValue::NULL, &JsValue::NULL);
            }
        }
        files.server.panel.set(true);
        refresh(files);
        files.wake();
        async move { JsFuture::from(promise).await.ok().and_then(|v| v.as_string()) }
    }

    /// Answer the Save As waiting here: with the server file `rel`, or `None` (closed).
    pub fn finish_save(files: &Rc<Files>, rel: Option<&str>) {
        let Some(waiting) = files.server.save_as.borrow_mut().take() else { return };
        let answer = rel.map_or(JsValue::NULL, |r| JsValue::from_str(&path(r)));
        let _ = waiting.resolve.call1(&JsValue::NULL, &answer);
        if rel.is_some() {
            files.server.panel.set(false);
        }
    }

    /// Queue what a command wrote to a server path. Writes go one at a time; a newer write of the
    /// same file replaces one still waiting.
    pub fn enqueue(files: &Rc<Files>, engine_path: &str, bytes: Vec<u8>) {
        QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            q.retain(|(p, _)| p != engine_path);
            q.push_back((engine_path.to_string(), bytes));
        });
        if BUSY.with(|b| b.replace(true)) {
            return;
        }
        let files = files.clone();
        spawn_local(async move {
            while let Some((engine_path, bytes)) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
                let Some(rel) = parse(&engine_path).map(String::from) else { continue };
                // The version this page last saw, or, for a new name, only if no file has it.
                let known = files.server.etags.borrow().get(&rel).cloned();
                let condition = match &known {
                    Some(tag) => ("If-Match", tag.clone()),
                    None => ("If-None-Match", "*".to_string()),
                };
                match request("PUT", &file_url(&rel), Some(&bytes), &[(condition.0, &condition.1)]).await {
                    Ok(r) if r.status == 200 => {
                        if let Some(tag) = r.etag {
                            files.server.etags.borrow_mut().insert(rel.clone(), tag);
                        }
                        files.say(format!("Saved {rel} on the server."), false);
                        refresh(&files);
                    }
                    Ok(r) => {
                        if r.status == 401 {
                            files.server.needs_token.set(true);
                        }
                        let text = if r.status == 412 && known.is_none() {
                            format!("{rel} already exists on the server. Save As and choose it to replace it, or use another name.")
                        } else {
                            status_text(r.status, &rel, &r.said())
                        };
                        files.failed_server(&engine_path, &text);
                    }
                    Err(e) => files.failed_server(&engine_path, &e),
                }
            }
            BUSY.with(|b| b.set(false));
            files.wake();
        });
    }

    /// Whether an engine path is a server file.
    pub fn is_server_path(p: &str) -> bool {
        p.starts_with(PREFIX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_paths_and_urls() {
        assert_eq!(parse("/server/Parts/a b.sc"), Some("Parts/a b.sc"));
        assert_eq!(parse("/server/"), None);
        assert_eq!(parse("/opfs/a.sc"), None);
        assert_eq!(path("a.sc"), "/server/a.sc");
        assert_eq!(file_url("Parts/a b#1.sc"), "api/v1/files/Parts/a%20b%231%2Esc");
        assert_eq!(folder_url(""), "api/v1/folder");
        assert_eq!(folder_url("mechanical/old"), "api/v1/folder/mechanical/old");
        assert_eq!(join("", "a.sc"), "a.sc");
        assert_eq!(join("m", "a.sc"), "m/a.sc");
        assert_eq!(parent("m/old"), "m");
        assert_eq!(parent("m"), "");
        assert_eq!(crumbs("m/old"), vec![("m".to_string(), "m".to_string()), ("old".to_string(), "m/old".to_string())]);
        assert!(crumbs("").is_empty());
    }

    #[test]
    fn the_token_goes_only_over_https_or_to_this_computer() {
        assert!(may_send_token("https:", "cad.example"));
        assert!(may_send_token("http:", "localhost"));
        assert!(may_send_token("http:", "127.0.0.1"));
        assert!(!may_send_token("http:", "cad.example"));
        assert!(!may_send_token("http:", "100.101.102.103"));
    }

    #[test]
    fn listings_and_names() {
        let reply = serde_json::json!({
            "folders": [{"name": "old", "path": "m/old"}, {"name": 1}],
            "files": [{"name": "a.sc", "path": "m/a.sc", "size": 10, "modified": 5, "etag": "\"a-1\""}, {"path": 3}],
        });
        let (folders, files) = listing(&reply);
        assert_eq!(folders, vec![("old".to_string(), "m/old".to_string())]);
        assert_eq!(files, vec![Entry { name: "a.sc".into(), path: "m/a.sc".into(), size: 10, etag: "\"a-1\"".into() }]);
        assert_eq!(listing(&serde_json::json!({})), (vec![], vec![]));
        assert_eq!(design_name("Bracket"), "Bracket.sc");
        assert_eq!(design_name("Bracket.sc"), "Bracket.sc");
        assert_eq!(design_name("../x/.hidden.sc"), "hidden.sc");
        assert_eq!(design_name(" "), "Untitled.sc");
        assert_eq!(clean_name("v2/"), None);
        assert_eq!(clean_name("New folder. "), Some("New folder".into()));
        assert!(status_text(412, "a.sc", "").contains("someone else"));
    }
}
