//! Stored preferences on the 3D mouse page (`PAGE_MOUSE3D`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! mouse3d_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// A device's own app on this computer moves the view (the navigation bridge, docs/3d-mouse.md):
            /// the desktop app connects to it on `nav_bridge_port`. Off unless asked for, so the app
            /// doesn't connect to whatever else listens on that port.
            nav_bridge_enabled: bool = false, PAGE_MOUSE3D;
            /// The loopback port the device app listens on (0: none).
            nav_bridge_port: u16 = 8765, PAGE_MOUSE3D;
            /// Per physical axis of the cap, which of its motions drives it (`space_mouse::reassigned`),
            /// applied after calibration; each motion drives exactly one axis.
            space_mouse_axis_source: [usize; 6] = crate::space_mouse::SAME_AXES, PAGE_MOUSE3D;
            /// Per physical axis of the cap, a multiplier on top of `space_mouse_speed`; 0 switches it off.
            space_mouse_axis_speed: [f32; 6] = [1.0; 6], PAGE_MOUSE3D;
            /// Which program reads the 3D mouse when spacenavd and the device itself are both there (Linux).
            space_mouse_driver: SpaceMouseDriver = SpaceMouseDriver::default(), PAGE_MOUSE3D;
            /// Whether a connected 3D mouse moves the view.
            space_mouse_enabled: bool = true, PAGE_MOUSE3D => check("Navigate with the 3D mouse");
            /// Where the 3D mouse picture sits in the 3D view, when it is shown there (`space_mouse_in_view`).
            space_mouse_ghost_position: SpaceMouseGhost = SpaceMouseGhost::default(), PAGE_MOUSE3D;
            /// Whether a picture of the connected 3D mouse, moving as it does, sits in the 3D view's lower
            /// left corner. Off by default.
            space_mouse_in_view: bool = false, PAGE_MOUSE3D => check_id("ui.3d_mouse_show_in_view");
            /// 3D mouse orbit: traced (on) turns the view around the model in the middle of the view when
            /// the turn begins (else the middle of the bodies in view), marked by a faint ball; classic
            /// (off, the default) turns it around the view's centre, as it did before.
            space_mouse_model_pivot: bool = false, PAGE_MOUSE3D;
            /// The user turned the calibration offer off for good ("Never show again"). Unlike closing the
            /// card, which only records that device, this stops every offer, for every device: it is one flag,
            /// not kept per device key. Like the settings beside it, the dialog's Restore Defaults puts it back
            /// (the 3D mouse section's own restore leaves it).
            space_mouse_offer_never: bool = false, PAGE_MOUSE3D;
            /// Per physical axis of the cap (push X, Y, Z, then tilt X, Y, Z), whether it moves the view
            /// the other way.
            space_mouse_reverse: [bool; 6] = [false; 6], PAGE_MOUSE3D;
            /// Whether the 3D mouse also turns the view while a sketch is edited and still seen square on
            /// (#2227), as the mouse can. Off (the default): the cap pans and zooms a square-on sketch and
            /// turns it only after the view is tilted away from it.
            space_mouse_sketch_orbit: bool = false, PAGE_MOUSE3D => check_id("ui.3d_mouse_sketch_orbit");
            /// Speed multiplier for 3D mouse navigation.
            space_mouse_speed: f32 = 1.0, PAGE_MOUSE3D;
        }
    };
}
