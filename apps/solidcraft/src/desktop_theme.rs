//! Linux: the desktop's dark or light preference, for Theme: System (#7944).
//!
//! The window toolkit does not pass it on everywhere: on GNOME (Wayland, or X11 through XWayland)
//! the window never learns it, nor when it changes, so the app kept its colours. This reads the
//! freedesktop setting through the XDG Desktop Portal's Settings interface on the session bus
//! (`org.freedesktop.appearance color-scheme`, then GNOME's `color-scheme` and `gtk-theme`), the
//! same setting other applications follow, and inside a Flatpak too. It runs on a thread of its
//! own: the first reading, then the portal's `SettingChanged` signal; where the signal can't be
//! had, a reading every few seconds. Without a portal it asks `gsettings` instead, on the same
//! thread. The window reads the last answer with an atomic load
//! (`solidcraft_ui_egui::Services::desktop_dark`), and the decision from the raw values is
//! `solidcraft_ui_egui::prefs::desktop_prefers_dark`.

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use zbus::zvariant::{OwnedValue, Value};

const UNKNOWN: u8 = 0;
const DARK: u8 = 1;
const LIGHT: u8 = 2;

/// How often to read again where the portal's change signal is not available.
const POLL: Duration = Duration::from_secs(5);
/// How long one `gsettings` reading may take before it is given up.
const GSETTINGS_TIMEOUT: Duration = Duration::from_secs(2);

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";
const GNOME: &str = "org.gnome.desktop.interface";

/// Start following the desktop's preference; the closure answers the latest reading (`None`
/// until there is one, or when the desktop says nothing).
pub fn start(ctx: &egui::Context) -> Box<dyn Fn() -> Option<bool>> {
    let state = Arc::new(AtomicU8::new(UNKNOWN));
    let (thread_state, ctx) = (state.clone(), ctx.clone());
    if let Err(e) = std::thread::Builder::new().name("desktop-theme".into()).spawn(move || follow(&thread_state, &ctx)) {
        log::warn!("desktop theme: no thread to follow it: {e}");
    }
    Box::new(move || match state.load(Ordering::Relaxed) {
        DARK => Some(true),
        LIGHT => Some(false),
        _ => None,
    })
}

/// Keep `state` on the desktop's preference, asking for a frame whenever it changes.
fn follow(state: &AtomicU8, ctx: &egui::Context) {
    let publish = |dark: Option<bool>| {
        let code = match dark {
            Some(true) => DARK,
            Some(false) => LIGHT,
            None => UNKNOWN,
        };
        if state.swap(code, Ordering::Relaxed) != code {
            ctx.request_repaint();
        }
    };
    let portal = match portal() {
        Ok(p) => Some(p),
        Err(e) => {
            log::info!("desktop theme: no settings portal ({e}); asking gsettings");
            None
        }
    };
    // The portal first; gsettings where there is none or it says nothing (its backend missing).
    let read = |portal: Option<&zbus::blocking::Proxy<'static>>| portal.and_then(read_portal).or_else(read_gsettings);
    let from_portal = portal.as_ref().and_then(read_portal);
    let first = from_portal.or_else(read_gsettings);
    publish(first);
    // Wait for the portal's signal only when the portal answers: else the reading is gsettings',
    // which only a new reading notices.
    if let Some(p) = portal.as_ref().filter(|_| from_portal.is_some()) {
        match p.receive_signal("SettingChanged") {
            Ok(changes) => {
                for message in changes {
                    if relevant(&message) {
                        publish(read(Some(p)));
                    }
                }
                log::info!("desktop theme: the portal's change signal ended; reading every few seconds");
            }
            Err(e) => log::info!("desktop theme: no change signal ({e}); reading every few seconds"),
        }
    } else if portal.is_none() && first.is_none() && !gsettings_available() {
        // Neither the portal nor gsettings: nothing to follow; the window toolkit's theme stays.
        return;
    }
    loop {
        std::thread::sleep(POLL);
        publish(read(portal.as_ref()));
    }
}

/// The portal's Settings interface on the session bus.
fn portal() -> zbus::Result<zbus::blocking::Proxy<'static>> {
    let connection = zbus::blocking::Connection::session()?;
    zbus::blocking::Proxy::new(&connection, PORTAL, PORTAL_PATH, SETTINGS)
}

/// One reading through the portal.
fn read_portal(portal: &zbus::blocking::Proxy<'static>) -> Option<bool> {
    let appearance = read_setting(portal, APPEARANCE, "color-scheme").and_then(|v| as_u32(&v));
    // The GNOME settings only matter when the freedesktop one has no preference.
    if matches!(appearance, Some(1 | 2)) {
        return solidcraft_ui_egui::prefs::desktop_prefers_dark(appearance, None, None);
    }
    let gnome = read_setting(portal, GNOME, "color-scheme").and_then(|v| as_string(&v));
    let gtk_theme = read_setting(portal, GNOME, "gtk-theme").and_then(|v| as_string(&v));
    solidcraft_ui_egui::prefs::desktop_prefers_dark(appearance, gnome.as_deref(), gtk_theme.as_deref())
}

/// A setting through `ReadOne` (portal version 2), else the older `Read`.
fn read_setting(portal: &zbus::blocking::Proxy<'static>, namespace: &str, key: &str) -> Option<OwnedValue> {
    let one: zbus::Result<OwnedValue> = portal.call("ReadOne", &(namespace, key));
    one.or_else(|_| -> zbus::Result<OwnedValue> { portal.call("Read", &(namespace, key)) }).ok()
}

/// Whether a `SettingChanged` signal is about a setting the preference is read from.
fn relevant(message: &zbus::Message) -> bool {
    let body = message.body();
    let Ok((namespace, key, _)) = body.deserialize::<(String, String, OwnedValue)>() else { return false };
    matches!((namespace.as_str(), key.as_str()), (APPEARANCE, "color-scheme") | (GNOME, "color-scheme" | "gtk-theme"))
}

/// The value inside the variants `Read` wraps it in (a portal answers one or two deep).
fn unwrap_variants<'v, 'a>(value: &'v Value<'a>) -> &'v Value<'a> {
    let mut value = value;
    for _ in 0..4 {
        match value {
            Value::Value(inner) => value = &**inner,
            _ => break,
        }
    }
    value
}

fn as_u32(value: &Value<'_>) -> Option<u32> {
    match unwrap_variants(value) {
        Value::U32(n) => Some(*n),
        Value::I32(n) => u32::try_from(*n).ok(),
        Value::U8(n) => Some(u32::from(*n)),
        _ => None,
    }
}

fn as_string(value: &Value<'_>) -> Option<String> {
    match unwrap_variants(value) {
        Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    }
}

/// Without a portal: GNOME's settings through `gsettings` (`None` when it is not there).
fn read_gsettings() -> Option<bool> {
    let gnome = gsettings("color-scheme");
    let gtk_theme = gsettings("gtk-theme");
    solidcraft_ui_egui::prefs::desktop_prefers_dark(None, gnome.as_deref(), gtk_theme.as_deref())
}

fn gsettings_available() -> bool {
    gsettings("gtk-theme").is_some()
}

/// `gsettings get org.gnome.desktop.interface <key>`, its quoted answer, or `None` when the
/// program is missing, fails or does not answer in time.
fn gsettings(key: &str) -> Option<String> {
    let mut child = std::process::Command::new("gsettings")
        .args(["get", GNOME, key])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if started.elapsed() < GSETTINGS_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.take(4096).read_to_string(&mut out).ok()?;
    let value = out.trim().trim_matches('\'').to_string();
    (!value.is_empty()).then_some(value)
}
