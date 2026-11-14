//! The `?host=parent` postMessage bridge (#16476). A host page (NextCraft) frames the web app and
//! keeps the user's files itself. The app says `solidcraft:ready` when it can take a document,
//! the host answers `solidcraft:open` with `{name, bytes}`, and the app hands saves back with
//! `solidcraft:save` (`saveAs: false` when Save writes the design under its name, `true` for a
//! Save As or an export), unsaved changes with `solidcraft:dirty`, and failures with
//! `solidcraft:failed`. Messages go to and are accepted from the page's own origin only, and
//! without `?host=parent` nothing is installed.
//!
//! A design the host opens is brought in like one from the computer (a copy in the browser's own
//! store, then opened), so the rest of the app is unchanged. A save in host mode is handed to the
//! host instead of becoming a download: the host writes it back.

/// The app has loaded and can take a document (app → host).
pub const READY: &str = "solidcraft:ready";
/// Open this file, `bytes` as an `ArrayBuffer` and `name` carrying the extension (host → app).
pub const OPEN: &str = "solidcraft:open";
/// Save instead of downloading (app → host).
pub const SAVE: &str = "solidcraft:save";
/// Unsaved changes, so the host can warn before the page closes (app → host).
pub const DIRTY: &str = "solidcraft:dirty";
/// The open (or a save) failed, with a message the host can show (app → host).
pub const FAILED: &str = "solidcraft:failed";

/// A parent may drive the bridge only when the app is framed and the parent is this page's own
/// origin: `?host=parent` at the top level, or in a cross-origin frame, does nothing.
#[cfg(any(target_arch = "wasm32", test))]
fn framed_same_origin(page: Option<&str>, parent: Option<&str>) -> bool {
    page.is_some() && parent.is_some() && parent == page
}

thread_local! {
    /// The bridge is up: `?host=parent`, framed, and the parent is this page's own origin.
    static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The last `dirty` sent, so the host hears a change once, not every frame.
    static LAST_DIRTY: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Is the bridge up? When it is, saves go to the host instead of the browser's downloads.
#[cfg(target_arch = "wasm32")]
pub fn active() -> bool {
    ACTIVE.get()
}

#[cfg(target_arch = "wasm32")]
pub fn install(host: bool, files: &std::rc::Rc<crate::files::Files>) -> Result<(), String> {
    let Some(window) = web_sys::window() else {
        return Ok(());
    };
    let page = window.location().origin().ok();
    let parent_origin = window.parent().ok().and_then(|p| p.location().origin().ok());
    if !host || !framed_same_origin(page.as_deref(), parent_origin.as_deref()) {
        return Ok(());
    }
    ACTIVE.set(true);
    let files = files.clone();
    let expected = page.clone();
    let listener = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
        move |e: web_sys::MessageEvent| {
            // Same origin only: a frame from anywhere else is not the host.
            if Some(e.origin().as_str()) != expected.as_deref() {
                return;
            }
            if let Err(error) = take(&files, e.data()) {
                failed(&error);
            }
        },
    );
    window
        .add_event_listener_with_callback("message", listener.as_ref().unchecked_ref())
        .map_err(|e| format!("{e:?}"))?;
    // The listener lives as long as the page.
    listener.forget();
    Ok(())
}

/// One message from the host: `solidcraft:open` brings the file in, everything else is ignored.
#[cfg(target_arch = "wasm32")]
fn take(files: &std::rc::Rc<crate::files::Files>, data: wasm_bindgen::JsValue) -> Result<(), String> {
    use wasm_bindgen::JsCast as _;
    let Ok(data) = data.dyn_into::<js_sys::Object>() else {
        return Ok(());
    };
    let kind = js_sys::Reflect::get(&data, &js_sys::JsValue::from("type"))
        .ok()
        .and_then(|v| v.as_string());
    if kind.as_deref() != Some(OPEN) {
        return Ok(());
    }
    let name = js_sys::Reflect::get(&data, &js_sys::JsValue::from("name"))
        .ok()
        .and_then(|v| v.as_string())
        .ok_or("solidcraft:open without a name")?;
    if name.is_empty() {
        return Err("solidcraft:open without a name".into());
    }
    let raw = js_sys::Reflect::get(&data, &js_sys::JsValue::from("bytes")).map_err(|_| "solidcraft:open without bytes")?;
    let bytes = if let Some(buffer) = raw.dyn_ref::<js_sys::ArrayBuffer>() {
        js_sys::Uint8Array::new(buffer).to_vec()
    } else if let Some(array) = raw.dyn_ref::<js_sys::Uint8Array>() {
        array.to_vec()
    } else {
        return Err(format!("solidcraft:open: {name} did not come as bytes"));
    };
    if bytes.is_empty() {
        return Err(format!("solidcraft:open: {name} is empty"));
    }
    files.open_from_host(name, bytes);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn post(fields: &[(&str, wasm_bindgen::JsValue)]) {
    use wasm_bindgen::JsCast as _;
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(target) = window.location().origin() else {
        return;
    };
    let message = js_sys::Object::new();
    for (key, value) in fields {
        let _ = js_sys::Reflect::set(&message, &js_sys::JsValue::from(*key), value);
    }
    let _ = window.post_message(&message, &target);
}

/// The app has loaded and can take a document.
#[cfg(target_arch = "wasm32")]
pub fn announce_ready() {
    if ACTIVE.get() {
        post(&[("type", READY.into())]);
    }
}

/// Hand `bytes` to the host to save as `name` (`save_as` for a Save As or an export).
#[cfg(target_arch = "wasm32")]
pub fn save(name: &str, bytes: &[u8], save_as: bool) {
    if !ACTIVE.get() {
        return;
    }
    post(&[
        ("type", SAVE.into()),
        ("name", name.into()),
        ("bytes", js_sys::Uint8Array::from(bytes).into()),
        ("saveAs", save_as.into()),
    ]);
}

/// Unsaved changes changed: tell the host once, so it can warn before the page closes.
#[cfg(target_arch = "wasm32")]
pub fn dirty(dirty: bool) {
    if !ACTIVE.get() || LAST_DIRTY.get() == Some(dirty) {
        return;
    }
    LAST_DIRTY.set(Some(dirty));
    post(&[("type", DIRTY.into()), ("dirty", dirty.into())]);
}

/// The open (or a save) failed, with a message the host can show.
#[cfg(target_arch = "wasm32")]
pub fn failed(error: &str) {
    if !ACTIVE.get() {
        return;
    }
    post(&[("type", FAILED.into()), ("error", error.into())]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_framed_same_origin_parent_drives_the_bridge() {
        assert!(framed_same_origin(Some("https://cloud.example.org"), Some("https://cloud.example.org")));
        assert!(!framed_same_origin(Some("https://cloud.example.org"), Some("https://other.example.org")));
        assert!(!framed_same_origin(Some("https://cloud.example.org"), None), "not framed");
        assert!(!framed_same_origin(None, Some("https://cloud.example.org")), "no page origin");
    }
}
