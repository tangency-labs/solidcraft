//! A host page that keeps the files: `?host=parent` (or a page the `?host_origin=` names) frames
//! the app and owns what crosses, like the other Craft apps' web bridges. The page hands a file in
//! with `solidcraft:open {name, bytes}`, and every save and export goes back as
//! `solidcraft:save {name, bytes, id, export?}` instead of a download, acknowledged with
//! `solidcraft:saved {id}`. The page also hears `solidcraft:ready {version}` once the app
//! started, `solidcraft:dirty {dirty}` when unsaved changes appear or go away, and
//! `solidcraft:failed {error}` when the app can't start or a host save was not kept. Unframed
//! pages are unchanged.
//!
//! Only the parent window is heard or answered, and only at the origin decided at start: the
//! page's own, or the exact origin a validated `?host_origin=` names.

#[cfg(any(target_arch = "wasm32", test))]
mod bridge {
    use std::collections::HashMap;

    /// A file the parent sends larger than this is refused, never copied (`solidcraft:failed`).
    /// The same limit the engine applies to its other file reads.
    pub const MAX_OPEN_BYTES: usize = 256 * 1024 * 1024;
    /// An unacknowledged save is reported as failed after this long (`solidcraft:failed`).
    pub const SAVE_TIMEOUT_MS: f64 = 30_000.0;

    /// Messages the host sends.
    pub const OPEN: &str = "solidcraft:open";
    pub const SAVED: &str = "solidcraft:saved";
    /// Messages the app sends.
    pub const READY: &str = "solidcraft:ready";
    pub const SAVE: &str = "solidcraft:save";
    pub const DIRTY: &str = "solidcraft:dirty";
    pub const FAILED: &str = "solidcraft:failed";

    /// Lowercase scheme and host, drop the default port, as browsers serialise origins, so the
    /// string a `?host_origin=` names compares equal to what `message` events report.
    pub fn canonical_origin(value: &str) -> Option<String> {
        let (scheme, rest) = value.trim().split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https") {
            return None;
        }
        let (host, port) = match rest.split_once(':') {
            Some((h, p)) => {
                if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                (h.to_ascii_lowercase(), p.parse::<u16>().ok())
            }
            None => (rest.to_ascii_lowercase(), None),
        };
        if host.is_empty() {
            return None;
        }
        Some(match (scheme.as_str(), port) {
            ("http", Some(80)) | ("https", Some(443)) | (_, None) => format!("{scheme}://{host}"),
            (_, Some(p)) => format!("{scheme}://{host}:{p}"),
        })
    }

    /// The origin the bridge answers: the page's own (a same-origin parent, `own`), else the
    /// exact origin `?host_origin=` names, canonicalised.
    pub fn target_origin(own: Option<&str>, host_origin: Option<&str>) -> Option<String> {
        if own.is_some() {
            return own.map(str::to_string);
        }
        canonical_origin(host_origin?)
    }

    /// An incoming message the bridge understands, decided from the plain fields; the bytes are
    /// checked where they live (the wasm layer).
    #[derive(Debug, PartialEq)]
    pub enum FromHost {
        /// `solidcraft:open {name}`: open the bytes like File > Open.
        Open(String),
        /// `solidcraft:saved {id}`: the parent kept the save with this id.
        Saved(String),
        /// Anything else: not ours, ignored.
        Other,
    }

    pub fn parse_message(kind: &str, name: Option<&str>, id: Option<&str>) -> FromHost {
        match kind {
            OPEN => match name {
                Some(name) if acceptable_name(name) => FromHost::Open(name.to_string()),
                _ => FromHost::Other,
            },
            SAVED => match id {
                Some(id) if !id.trim().is_empty() && id.len() <= 64 => FromHost::Saved(id.to_string()),
                _ => FromHost::Other,
            },
            _ => FromHost::Other,
        }
    }

    /// A file name the parent may open: present, short, a file (no path parts).
    pub fn acceptable_name(name: &str) -> bool {
        !name.trim().is_empty() && name.len() <= 200 && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
    }

    /// The saves posted to the parent and not yet acknowledged: their ids and deadlines.
    #[derive(Default)]
    pub struct Saves {
        next: u64,
        pending: HashMap<String, f64>,
    }

    impl Saves {
        /// Post a save: its id and the deadline the parent must answer by.
        pub fn offer(&mut self, now: f64) -> String {
            self.next += 1;
            let id = format!("{:x}", self.next);
            self.pending.insert(id.clone(), now + SAVE_TIMEOUT_MS);
            id
        }

        /// The parent kept a save: false for an id that is not pending.
        pub fn ack(&mut self, id: &str) -> bool {
            self.pending.remove(id).is_some()
        }

        /// Saves past their deadline, oldest id first; they are no longer pending.
        pub fn expire(&mut self, now: f64) -> Vec<String> {
            let mut late: Vec<(f64, String)> =
                self.pending.iter().filter(|(_, deadline)| **deadline <= now).map(|(id, deadline)| (*deadline, id.clone())).collect();
            late.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let ids: Vec<String> = late.into_iter().map(|(_, id)| id).collect();
            for id in &ids {
                self.pending.remove(id);
            }
            ids
        }

        pub fn is_pending(&self) -> bool {
            !self.pending.is_empty()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn target_origin_prefers_same_origin_and_canonicalises_host_origin() {
            assert_eq!(target_origin(Some("https://files.example:443"), None).as_deref(), Some("https://files.example:443"));
            assert_eq!(
                target_origin(None, Some("https://Files.example")).as_deref(),
                Some("https://files.example"),
                "host origin is canonicalised, so replies reach a parent the browser reports in canonical form"
            );
            assert_eq!(target_origin(None, Some("https://files.example:443")).as_deref(), Some("https://files.example"));
            assert_eq!(target_origin(None, Some("http://files.example:0080")).as_deref(), Some("http://files.example:80"));
            assert_eq!(target_origin(None, None), None, "no ?host_origin=, no cross-origin parent");
            assert_eq!(target_origin(None, Some("https://files.example/path")), None);
            assert_eq!(target_origin(None, Some("javascript:alert(1)")), None);
        }

        #[test]
        fn parse_message_reads_open_and_saved_and_ignores_the_rest() {
            assert_eq!(parse_message(OPEN, Some("bracket.step"), None), FromHost::Open("bracket.step".into()));
            assert_eq!(parse_message(SAVED, None, Some("1")), FromHost::Saved("1".into()));
            assert_eq!(parse_message(OPEN, None, None), FromHost::Other, "no name, no open");
            assert_eq!(parse_message(OPEN, Some("a/b"), None), FromHost::Other, "a path is not a name");
            assert_eq!(parse_message(SAVED, None, Some("")), FromHost::Other);
            assert_eq!(parse_message("solidcraft:other", None, None), FromHost::Other);
        }

        #[test]
        fn saves_offer_ack_and_expire_in_order() {
            let mut saves = Saves::default();
            assert_eq!(saves.offer(100.0), "1");
            assert_eq!(saves.offer(200.0), "2");
            assert!(saves.is_pending());
            assert!(!saves.ack("9"));
            assert!(saves.ack("1"));
            assert_eq!(saves.expire(300.0 - 1.0), Vec::<String>::new(), "nothing is due before its deadline");
            assert_eq!(saves.expire(300.0), vec!["2".to_string()]);
            assert!(!saves.is_pending());
            assert_eq!(saves.offer(300.0), "3", "ids keep counting, so an old ack cannot confirm a new save");
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod live {
    use std::cell::RefCell;
    use std::rc::Rc;

    use js_sys::{ArrayBuffer, JsCast as _, JsValue, Uint8Array};
    use serde_json::json;
    use wasm_bindgen::{JsError, closure::Closure};
    use web_sys::{MessageEvent, Window};

    use super::bridge::{self, FromHost, MAX_OPEN_BYTES, Saves};
    use crate::files::{Files, Intent};

    struct Host {
        /// The only origin heard and answered.
        origin: String,
        files: Rc<Files>,
        saves: Saves,
        /// The dirty state last reported, so `solidcraft:dirty` is posted on changes only.
        dirty: Option<bool>,
    }

    thread_local! {
        static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    }

    fn post(window: &Window, origin: &str, message: &serde_json::Value) {
        if let Ok(text) = js_sys::JSON::stringify(&JsValue::from_str(&message.to_string())) {
            let _ = window.post_message(&text.into(), origin);
        }
    }

    fn post_failed(origin: &str, error: &str) {
        with_window(|window| post(window, origin, &json!({"type": bridge::FAILED, "error": error})));
    }

    fn with_window(f: impl FnOnce(&Window)) {
        if let Some(window) = web_sys::window() {
            f(&window);
        }
    }

    fn with_host(f: impl FnOnce(&mut Host, &Window)) {
        HOST.with(|h| {
            if let Some(host) = h.borrow_mut().as_mut() {
                if let Some(window) = web_sys::window() {
                    f(host, &window);
                }
            }
        });
    }

    /// Take up the bridge when the page runs as `?host=parent` in a frame it may talk to: true
    /// when the app started bridged. `files` receives what the parent opens.
    pub fn start(files: &Rc<Files>) -> Result<(), JsError> {
        let window = web_sys::window().ok_or_else(|| JsError::new("no window"))?;
        // `parent` on a top-level page is the page itself: not framed, no bridge.
        let framed = window.parent().ok().map(|parent| JsValue::from(parent) != JsValue::from(window.clone())).unwrap_or(false);
        let host_origin = query_value("host_origin");
        let bridged = query_value("host").as_deref() == Some("parent") && (framed || host_origin.is_some());
        if !bridged {
            return Ok(());
        }
        let own = window.location().origin().ok();
        let Some(origin) = bridge::target_origin(own.as_deref(), host_origin.as_deref()) else {
            return Err(JsError::new("?host=parent with no ?host_origin= outside a same-origin frame"));
        };
        let files = files.clone();
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
            let Ok(event) = event.dyn_into::<MessageEvent>() else { return };
            hear(&event);
        });
        window.add_event_listener_with_callback("message", handler.into_js_value().unchecked_ref())?;
        *HOST.borrow_mut() = Some(Host { origin, files: files.clone(), saves: Saves::default(), dirty: None });
        Ok(())
    }

    /// The query's value for `name`, decoded once.
    fn query_value(name: &str) -> Option<String> {
        let query = web_sys::window()?.location().search().ok()?;
        url::form_urlencoded::parse(query.trim_start_matches('?').as_bytes()).find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
    }

    /// A message from the parent: open what it sends, note what it kept.
    fn hear(event: &MessageEvent) {
        HOST.with(|h| {
            let Some(mut host) = h.borrow_mut().as_mut() else { return };
            if event.origin() != host.origin {
                return;
            }
            let Ok(object) = event.data().dyn_into::<js_sys::Object>() else { return };
            let kind = js_sys::Reflect::get(&object, &"type".into()).ok().and_then(|v| v.as_string());
            let Some(kind) = kind else { return };
            let name = js_sys::Reflect::get(&object, &"name".into()).ok().and_then(|v| v.as_string());
            let id = js_sys::Reflect::get(&object, &"id".into()).ok().and_then(|v| v.as_string());
            match bridge::parse_message(&kind, name.as_deref(), id.as_deref()) {
                FromHost::Open(name) => open(&mut host, name, &js_sys::Reflect::get(&object, &"bytes".into()).unwrap_or(JsValue::UNDEFINED)),
                FromHost::Saved(id) => {
                    let acked = host.saves.ack(&id);
                    if acked {
                        host.files.say("Saved to the host page.", false);
                    }
                }
                FromHost::Other => {}
            }
        });
    }

    /// Open a file the parent sent: its bytes must be an `ArrayBuffer` or `Uint8Array`, and at
    /// most `MAX_OPEN_BYTES`; it is queued like a file brought in from the computer. Refusals go
    /// to the parent and the status line without touching the bridge's queue again.
    fn open(host: &mut Host, name: String, bytes: &JsValue) {
        let refused = |why: &str| {
            post_failed(&host.origin, why);
            host.files.say(format!("{name}: {why}"), true);
        };
        if !bridge::acceptable_name(&name) {
            return refused("the file name is not a plain file name");
        }
        let data: Uint8Array = if let Ok(buffer) = bytes.clone().dyn_into::<ArrayBuffer>() {
            if buffer.byte_length() > MAX_OPEN_BYTES as u32 {
                return refused(&format!("the file is larger than {} MiB", MAX_OPEN_BYTES >> 20));
            }
            Uint8Array::new(&buffer)
        } else if let Ok(array) = bytes.clone().dyn_into::<Uint8Array>() {
            if array.byte_length() > MAX_OPEN_BYTES as u32 {
                return refused(&format!("the file is larger than {} MiB", MAX_OPEN_BYTES >> 20));
            }
            if array.length() == 0 {
                return refused("the file is empty");
            }
            array
        } else {
            return refused("the bytes must be an ArrayBuffer or a Uint8Array");
        };
        host.files.incoming.borrow_mut().push((name, data.to_vec(), Intent::Open));
        host.files.wake();
    }

    /// Every save and export goes to the parent instead of a download: true when handed over.
    pub fn offer_save(name: &str, bytes: &[u8], export: bool) -> bool {
        let active = HOST.with(|h| h.borrow().is_some());
        if !active {
            return false;
        }
        let id = HOST.with(|h| h.borrow_mut().as_mut().map(|host| host.saves.offer(js_sys::Date::now())));
        let Some(id) = id else { return false };
        with_host(|host, window| {
            let array = Uint8Array::from(bytes);
            let object = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&object, &"type".into(), &JsValue::from_str(bridge::SAVE));
            let _ = js_sys::Reflect::set(&object, &"name".into(), &JsValue::from_str(name));
            let _ = js_sys::Reflect::set(&object, &"id".into(), &JsValue::from_str(&id));
            let _ = js_sys::Reflect::set(&object, &"bytes".into(), &array);
            if export {
                let _ = js_sys::Reflect::set(&object, &"export".into(), &JsValue::from_bool(true));
            }
            let _ = window.parent().map(|p| p.post_message(&object.into(), &host.origin));
        });
        true
    }

    /// `solidcraft:ready {version}`: posted once, after the app started.
    pub fn ready() {
        with_host(|host, window| {
            post(window, &host.origin, &json!({"type": bridge::READY, "version": env!("CARGO_PKG_VERSION")}));
        });
    }

    /// `solidcraft:failed {error}`: the app cannot do what the bridge promised.
    pub fn failed(error: &str) {
        with_host(|host, window| post(window, &host.origin, &json!({"type": bridge::FAILED, "error": error})));
    }

    /// `solidcraft:dirty {dirty}` when unsaved changes appear or go away in the app.
    pub fn set_dirty(dirty: bool) {
        let changed = HOST.with(|h| {
            h.borrow_mut().as_mut().map(|host| {
                let changed = host.dirty != Some(dirty);
                host.dirty = Some(dirty);
                changed
            })
        });
        if changed == Some(true) {
            with_host(|host, window| post(window, &host.origin, &json!({"type": bridge::DIRTY, "dirty": dirty})));
        }
    }

    /// Saves the parent never acknowledged are reported as failed once their deadline passes.
    pub fn tick() {
        let late = HOST.with(|h| h.borrow_mut().as_mut().map(|host| host.saves.expire(js_sys::Date::now())));
        let Some(late) = late.filter(|late| !late.is_empty()) else { return };
        with_host(|host, window| {
            for _ in late {
                post(window, &host.origin, &json!({"type": bridge::FAILED, "error": "the host page did not keep the save"}));
            }
        });
        // The app says so on its status line too.
        HOST.with(|h| {
            if let Some(host) = h.borrow().as_ref() {
                host.files.say("Not saved to the host page: it did not answer in time.", true);
            }
        });
    }

    /// The app is bridged: `?sample` is left to the host.
    pub fn active() -> bool {
        HOST.with(|h| h.borrow().is_some())
    }

    /// Run a frame.
    pub fn poll() {
        tick();
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub use bridge::{FromHost, MAX_OPEN_BYTES, SAVE_TIMEOUT_MS, Saves, acceptable_name, canonical_origin, parse_message, target_origin};

#[cfg(target_arch = "wasm32")]
pub use live::{active, failed, offer_save, poll, ready, set_dirty, start};
