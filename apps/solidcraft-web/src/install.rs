//! Install as an app (`solidcraft_ui_egui::install`). The browser offers it with a
//! `beforeinstallprompt` event, which can come before this code runs: index.html keeps the event
//! as `window.solidcraftInstall` (and drops it once the app is installed). The offer is good for
//! one prompt; a browser that is still willing makes a new one later. Safari makes no offer: it
//! gets its steps, unless the app is already running installed.

use solidcraft_ui_egui::install::{InstallApp, Offer, safari_steps};
use wasm_bindgen::{JsCast as _, JsValue};

/// Where index.html keeps the browser's offer.
const OFFER: &str = "solidcraftInstall";

/// The browser's offer, kept by index.html (None: none, or it was used).
fn offer() -> Option<js_sys::Object> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &OFFER.into()).ok()?.dyn_into().ok()
}

/// Show the browser's install prompt (from the File menu click, so the page may).
fn prompt() {
    let Some(offer) = offer() else { return };
    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(&window, &OFFER.into(), &JsValue::NULL);
    }
    let shown = js_sys::Reflect::get(&offer, &"prompt".into())
        .and_then(|f| f.dyn_into::<js_sys::Function>())
        .and_then(|f| f.call0(&offer))
        .and_then(|p| p.dyn_into::<js_sys::Promise>());
    match shown {
        // Accepted or dismissed, the browser handles it; a refusal is only logged.
        Ok(p) => wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(p).await {
                log::warn!("install prompt: {e:?}");
            }
        }),
        Err(e) => log::warn!("install prompt: {e:?}"),
    }
}

/// The page runs as the installed app (its own window, or from the Home Screen on iOS).
fn running_installed(window: &web_sys::Window) -> bool {
    let standalone = window.match_media("(display-mode: standalone)").ok().flatten().is_some_and(|m| m.matches());
    let home_screen = js_sys::Reflect::get(&window.navigator(), &"standalone".into()).ok().and_then(|v| v.as_bool()).unwrap_or(false);
    standalone || home_screen
}

/// The host service for File ▸ Install app…; the UI is drawn again whenever the browser
/// makes or withdraws its offer (even with nothing else happening).
pub fn service(ctx: &egui::Context) -> InstallApp {
    // Safari's steps, decided once: the browser and the way the page runs don't change.
    let steps = web_sys::window().filter(|w| !running_installed(w)).and_then(|w| {
        let nav = w.navigator();
        let vendor = js_sys::Reflect::get(&nav, &"vendor".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        safari_steps(&nav.user_agent().unwrap_or_default(), &vendor, nav.max_touch_points())
    });
    if let Some(window) = web_sys::window() {
        for event in ["beforeinstallprompt", "appinstalled"] {
            let ctx = ctx.clone();
            let redraw = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || ctx.request_repaint());
            let _ = window.add_event_listener_with_callback(event, redraw.as_ref().unchecked_ref());
            // The listener lives as long as the page.
            redraw.forget();
        }
    }
    let offered = move || if offer().is_some() { Some(Offer::Prompt) } else { steps.map(Offer::Steps) };
    InstallApp { offer: Box::new(offered), prompt: Box::new(prompt) }
}
