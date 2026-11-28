//! Where a preference can take effect (#22848). An option whose effect the platform can't
//! deliver is not drawn there, so nobody turns on a setting that silently does nothing. The saved
//! value is kept either way, so a profile moved between machines doesn't lose it.
//!
//! Each such option names what it needs ([`Needs`]); [`available`] answers from a few facts
//! about where the app runs ([`Facts`]): the build target, and at run time the Linux window
//! system and whether the browser is Safari. A fact that isn't known keeps the option shown.

use crate::SolveApp;

/// What an option needs from the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    /// A two-finger twist on the trackpad: winit reports one on macOS and on Wayland, eframe in
    /// the browser only from Safari's gesture event; Windows and X11 never send one.
    TrackpadTwist,
    /// Moving the pointer to the other edge of the screen during an orbit drag: Wayland and the
    /// browser don't let an app move the pointer.
    CursorWarp,
}

/// The system the app was built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
    Browser,
    Other,
}

impl Os {
    /// This build's system.
    pub const fn this() -> Self {
        if cfg!(target_arch = "wasm32") {
            Os::Browser
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        }
    }
}

/// What [`available`] decides from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub os: Os,
    /// Linux: the window system this run uses, `wayland` or `x11`; None: not known.
    pub window_system: Option<&'static str>,
    /// Browser: the page runs in Safari.
    pub safari: bool,
}

impl Facts {
    /// The facts for this run.
    pub fn of(app: &SolveApp) -> Self {
        Facts { os: Os::this(), window_system: app.services.window_system, safari: app.services.browser_safari }
    }
}

/// Whether an option that `needs` this can take effect where `facts` say the app runs.
pub fn available(needs: Needs, facts: Facts) -> bool {
    match needs {
        Needs::TrackpadTwist => match facts.os {
            Os::Windows => false,
            Os::Linux => facts.window_system != Some("x11"),
            Os::Browser => facts.safari,
            Os::Mac | Os::Other => true,
        },
        Needs::CursorWarp => match facts.os {
            Os::Browser => false,
            Os::Linux => facts.window_system != Some("wayland"),
            Os::Windows | Os::Mac | Os::Other => true,
        },
    }
}

/// Whether Preferences draws an option that `needs` this, in this run.
pub fn shown(app: &SolveApp, needs: Needs) -> bool {
    available(needs, Facts::of(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(os: Os, window_system: Option<&'static str>, safari: bool) -> Facts {
        Facts { os, window_system, safari }
    }

    #[test]
    fn the_twist_is_offered_only_where_a_twist_arrives() {
        let twist = |f| available(Needs::TrackpadTwist, f);
        assert!(!twist(facts(Os::Windows, None, false)), "winit reports no trackpad rotation on Windows");
        assert!(twist(facts(Os::Mac, None, false)));
        assert!(twist(facts(Os::Linux, Some("wayland"), false)));
        assert!(!twist(facts(Os::Linux, Some("x11"), false)), "X11 has no pointer gestures");
        assert!(twist(facts(Os::Linux, None, false)), "an unknown window system keeps the option");
        assert!(twist(facts(Os::Browser, None, true)), "Safari sends its gesture event");
        assert!(!twist(facts(Os::Browser, None, false)));
    }

    #[test]
    fn the_cursor_wrap_is_offered_only_where_the_pointer_can_be_moved() {
        let wrap = |f| available(Needs::CursorWarp, f);
        assert!(wrap(facts(Os::Windows, None, false)));
        assert!(wrap(facts(Os::Mac, None, false)));
        assert!(wrap(facts(Os::Linux, Some("x11"), false)));
        assert!(!wrap(facts(Os::Linux, Some("wayland"), false)), "Wayland doesn't let an app move the pointer");
        assert!(wrap(facts(Os::Linux, None, false)), "an unknown window system keeps the option");
        assert!(!wrap(facts(Os::Browser, None, true)));
        assert!(!wrap(facts(Os::Browser, None, false)));
    }

    #[test]
    fn this_build_knows_its_system() {
        let os = Os::this();
        if cfg!(target_os = "windows") {
            assert_eq!(os, Os::Windows);
        }
        assert_eq!(os == Os::Browser, cfg!(target_arch = "wasm32"));
    }
}
