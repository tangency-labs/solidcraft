//! Bringing the window forward when a close stops at "save changes?".
//!
//! A close from the taskbar or a window switcher reaches the window while another app has the
//! focus. The prompt it opens is only seen once the window comes forward, so the window asks for
//! the focus then. A window the user is already in is left alone.
//!
//! winit's Focus asks as an application, which a window manager with focus-stealing prevention
//! (KWin's default) refuses for a window used before the switch away: it only marks the taskbar
//! entry. On X11 the request is sent again with the source EWMH gives "direct user actions"
//! (`_NET_ACTIVE_WINDOW`, source 2), which is what this is: the prompt is the rest of the user's
//! close. Native Wayland has no way for a window to raise itself, so there the attention request
//! (xdg-activation) lets the compositor raise it or mark its taskbar entry.
//!
//! When: not at once. KWin raises the window, then the taskbar's closing context menu hands the
//! focus back to the app that had it, a few milliseconds later, and the window would flicker
//! forward and back. So the window asks once the menu is gone, `FIRST_AFTER` the close. It asks
//! again only if the focus is taken back right after it came (a slow menu) or never came; a later
//! switch is the user's own, and the window stays where the user put it.

use std::time::{Duration, Instant};

/// From the close to the first request: past the taskbar menu's focus hand-back (2 to 11 ms on
/// KDE Plasma 6), too short to notice.
const FIRST_AFTER: Duration = Duration::from_millis(120);
/// After the first request, how long the window asks again when it loses the focus: short of
/// anything the user does next.
const KEEP_ASKING: Duration = Duration::from_millis(400);
/// The least time between two requests.
const AGAIN_AFTER: Duration = Duration::from_millis(100);
/// Focus lost this soon after a request was taken back by the taskbar menu; later, by the user.
const TAKEN_BACK_WITHIN: Duration = Duration::from_millis(50);

/// The window coming forward after a close that asks about unsaved designs.
#[derive(Default)]
pub struct ComeForward {
    /// The first request and the end of asking; `None`: not asking.
    window: Option<(Instant, Instant)>,
    /// The last request.
    last: Option<Instant>,
    /// The window has had the focus since the last request.
    held: bool,
}

/// What a frame does while the window is coming forward.
#[derive(Debug, PartialEq)]
enum Step {
    /// Ask for the focus now.
    Ask,
    /// Look again after this long.
    Wait(Duration),
    /// Done asking.
    Done,
}

impl ComeForward {
    /// A close stopped at the prompt: ask for the focus shortly.
    pub fn start(&mut self, now: Instant) {
        let first = now + FIRST_AFTER;
        self.window = Some((first, first + KEEP_ASKING));
        self.last = None;
        self.held = false;
    }

    /// Every frame: ask once the taskbar menu is gone, and again while the focus is taken back.
    pub fn tick(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.window.is_none() {
            return;
        }
        let (focused, minimized) = ctx.input(|i| (i.viewport().focused, i.viewport().minimized));
        // The app may be idle: a frame to look again.
        match self.step(Instant::now(), focused) {
            Step::Ask => {
                bring(ctx, frame, minimized);
                ctx.request_repaint_after(AGAIN_AFTER);
            }
            Step::Wait(after) => ctx.request_repaint_after(after),
            Step::Done => {}
        }
    }

    fn step(&mut self, now: Instant, focused: Option<bool>) -> Step {
        let Some((first, until)) = self.window else { return Step::Done };
        if now >= until {
            self.window = None;
            return Step::Done;
        }
        if now < first {
            return Step::Wait(first - now);
        }
        let Some(last) = self.last else {
            if focused == Some(true) {
                // The user went back to the window already.
                self.window = None;
                return Step::Done;
            }
            self.last = Some(now);
            return Step::Ask;
        };
        if focused == Some(true) {
            self.held = true;
            return Step::Wait(until - now);
        }
        if self.held {
            if now.duration_since(last) >= TAKEN_BACK_WITHIN {
                // The user switched away: leave the window there.
                self.window = None;
                return Step::Done;
            }
            self.held = false;
        }
        let next = last + AGAIN_AFTER;
        if now < next {
            return Step::Wait(next - now);
        }
        self.last = Some(now);
        Step::Ask
    }
}

/// Ask for the focus once.
fn bring(ctx: &egui::Context, frame: &eframe::Frame, minimized: Option<bool>) {
    #[cfg(all(unix, not(target_os = "macos")))]
    let raised = match x11_activate(frame) {
        Ok(sent) => sent,
        Err(e) => {
            log::warn!("bringing the window forward through X11: {e}");
            false
        }
    };
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let raised = {
        let _ = frame;
        false
    };
    for c in commands(minimized, raised) {
        ctx.send_viewport_cmd(c);
    }
}

/// The viewport commands for a window in the background; `raised`: the X11 request went out,
/// which also restores a minimized window.
fn commands(minimized: Option<bool>, raised: bool) -> Vec<egui::ViewportCommand> {
    if raised {
        return vec![egui::ViewportCommand::Focus];
    }
    let mut commands = Vec::new();
    if minimized == Some(true) {
        commands.push(egui::ViewportCommand::Minimized(false));
    }
    commands.push(egui::ViewportCommand::Focus);
    commands.push(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
    commands
}

/// Ask the X11 window manager to activate the window as a user action. `Ok(false)`: the window
/// isn't an X11 window (native Wayland).
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_activate(frame: &eframe::Frame) -> Result<bool, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};

    let handle = frame.window_handle().map_err(|e| e.to_string())?;
    let window = match handle.as_raw() {
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).map_err(|e| e.to_string())?,
        RawWindowHandle::Xcb(h) => h.window.get(),
        _ => return Ok(false),
    };
    // A connection of its own: winit's isn't reachable, and a close that asks is rare.
    let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let root = conn.setup().roots.get(screen).ok_or("no X11 screen")?.root;
    let atom = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
    // Source 2: a direct user action; the time is the server's current time.
    let event = ClientMessageEvent::new(32, window, atom, [2, x11rb::CURRENT_TIME, 0, 0, 0]);
    conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{AGAIN_AFTER, ComeForward, FIRST_AFTER, KEEP_ASKING, Step};
    use eframe::egui::{UserAttentionType, ViewportCommand as C};
    use std::time::{Duration, Instant};

    /// The first request waits until the taskbar menu has handed the focus back. The window asks
    /// again when the focus is taken back right after it came, or never came, no more often than
    /// every `AGAIN_AFTER` and only for `KEEP_ASKING`; a later switch away is the user's.
    #[test]
    fn the_window_asks_once_the_taskbar_menu_is_gone() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let at = |n: u64| t0 + ms(n);
        let (first, again, keep) = (FIRST_AFTER.as_millis() as u64, AGAIN_AFTER.as_millis() as u64, KEEP_ASKING.as_millis() as u64);
        let mut f = ComeForward::default();
        assert_eq!(f.step(t0, Some(false)), Step::Done, "not started");
        f.start(t0);
        assert_eq!(f.step(t0, Some(false)), Step::Wait(FIRST_AFTER), "the menu is still closing");
        assert_eq!(f.step(at(5), Some(false)), Step::Wait(ms(first - 5)));
        assert_eq!(f.step(at(first), Some(false)), Step::Ask);
        assert_eq!(f.step(at(first + 3), Some(true)), Step::Wait(ms(keep - 3)), "raised");
        // A slow menu takes the focus back 10 ms later: ask again, not before AGAIN_AFTER.
        assert_eq!(f.step(at(first + 10), Some(false)), Step::Wait(ms(again - 10)));
        assert_eq!(f.step(at(first + again), Some(false)), Step::Ask);
        assert_eq!(f.step(at(first + again + 3), Some(true)), Step::Wait(ms(keep - again - 3)));
        // The user clicks another app 200 ms later: the window stays behind.
        assert_eq!(f.step(at(first + again + 200), Some(false)), Step::Done);
        assert_eq!(f.step(at(first + again + 300), Some(false)), Step::Done);

        // Never raised (the request was refused): ask again every AGAIN_AFTER, for KEEP_ASKING.
        f.start(t0);
        assert_eq!(f.step(at(first), None), Step::Ask, "unknown focus counts as out of focus");
        assert_eq!(f.step(at(first + again), Some(false)), Step::Ask);
        assert_eq!(f.step(at(first + keep), Some(false)), Step::Done);

        // Back in the window before the first request: nothing to do.
        f.start(t0);
        assert_eq!(f.step(at(first), Some(true)), Step::Done);
    }

    /// Without the X11 request: focus, and the attention mark where focus is refused; a minimized
    /// window is restored first.
    #[test]
    fn a_window_closed_from_the_background_comes_forward() {
        let forward = [C::Focus, C::RequestUserAttention(UserAttentionType::Informational)];
        assert_eq!(super::commands(Some(false), false), forward);
        assert_eq!(super::commands(None, false), forward, "unknown counts as not minimized");
        let minimized = super::commands(Some(true), false);
        assert_eq!(minimized.first(), Some(&C::Minimized(false)));
        assert_eq!(minimized.get(1..), Some(&forward[..]));
        // The window manager took the X11 request: nothing left to mark.
        assert_eq!(super::commands(Some(true), true), [C::Focus]);
    }
}
