//! Is the file chooser that opens the desktop's own? (#10159)
//!
//! `rfd` opens the file chooser through the XDG desktop portal; without the portal it falls
//! back to zenity, a window that does not belong to the app and that the app never mentions.
//! The desktop's own chooser is the portal one, so the app has to know when it is not the one
//! that opens, and say so in the session log.
//!
//! The session bus is asked once, on a thread of its own: a bus that answers late must not
//! hold the window. What it answered is kept, and read when a file is opened.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// The portal's bus name: the desktop's own file chooser lives behind it.
const PORTAL: &str = "org.freedesktop.portal.Desktop";

/// The bus has not answered yet.
const UNKNOWN: u8 = 0;
/// There is no session bus to reach the portal on.
const NO_SESSION_BUS: u8 = 1;
/// The bus is there but the portal does not answer on it.
const NO_PORTAL_SERVICE: u8 = 2;
/// The bus is there and the portal answers on it.
const PORTAL_THERE: u8 = 3;

/// What the session bus said about the portal.
static BUS: AtomicU8 = AtomicU8::new(UNKNOWN);
/// A file chooser is opening: whether it is the desktop's own is to be said.
static OPENING: AtomicBool = AtomicBool::new(false);

/// Ask the session bus, on a thread of its own, whether the portal is there.
pub fn start(ctx: &egui::Context) {
    let thread_ctx = ctx.clone();
    let probe = move || {
        let answer = match session_bus() {
            None => NO_SESSION_BUS,
            Some(connection) => match portal_on(&connection) {
                true => PORTAL_THERE,
                false => NO_PORTAL_SERVICE,
            },
        };
        BUS.store(answer, Ordering::Relaxed);
        // The answer is in: a file chooser waiting for it can be told now.
        thread_ctx.request_repaint();
    };
    if let Err(e) = std::thread::Builder::new().name("desktop-chooser".into()).spawn(probe) {
        log::warn!("file chooser: no thread to ask the session bus: {e}");
        // Nothing will answer: the desktop's own chooser cannot be told apart from the start.
        BUS.store(PORTAL_THERE, Ordering::Relaxed);
    }
}

/// The session bus, when there is one to talk to.
fn session_bus() -> Option<zbus::blocking::Connection> {
    zbus::blocking::Connection::session().ok()
}

/// Whether the portal answers on the bus: it is either running on it or waiting to be started.
fn portal_on(connection: &zbus::blocking::Connection) -> bool {
    let Ok(bus) = zbus::blocking::fdo::DBusProxy::new(connection) else { return false };
    let named = |names: Vec<zbus::names::OwnedBusName>| names.iter().any(|name| name.as_str() == PORTAL);
    // A portal that is running is on the bus; one that is not running yet is still there when
    // the bus can start it, which is what the portal's service file is for.
    match (bus.list_names().map(named), bus.list_activatable_names().map(named)) {
        (Ok(running), Ok(startable)) => running || startable,
        (Ok(running), Err(_)) => running,
        (Err(_), Ok(startable)) => startable,
        // The bus does not answer: nothing answers on it.
        (Err(_), Err(_)) => false,
    }
}

/// A file chooser is opening: ask for a frame, so that the reason it is not the desktop's own
/// is said (#10159).
pub fn note_opening(ctx: &egui::Context) {
    OPENING.store(true, Ordering::Relaxed);
    ctx.request_repaint();
}

/// The reason the file chooser that is opening is not the desktop's own, if it is not.
///
/// The decision is the one the test states: without a session bus there is no bus to reach the
/// portal on; with a bus but no portal on it, `rfd` falls back to zenity; and under XWayland the
/// window is an X11 one while the session is a Wayland one, which the portal refuses to parent
/// its chooser to.
pub fn take_reason() -> Option<&'static str> {
    if !OPENING.load(Ordering::Relaxed) {
        return None;
    }
    let (dbus_session, portal_service) = match BUS.load(Ordering::Relaxed) {
        NO_SESSION_BUS => (false, false),
        NO_PORTAL_SERVICE => (true, false),
        PORTAL_THERE => (true, true),
        // The bus has not answered yet: wait for it, and say nothing yet.
        _ => return None,
    };
    OPENING.store(false, Ordering::Relaxed);
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11_display = super::window_system_now() == Some("x11");
    super::desktop_chooser_fallback(dbus_session, portal_service, wayland_display, x11_display)
}
