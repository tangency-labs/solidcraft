//! Grouped preferences composed with the shared UI kit. Changes apply immediately; Cancel
//! restores the opening snapshot, Apply keeps editing, and Defaults affects the current page.

use egui::{Modifiers, Pos2};
use serde::{Deserialize, Serialize};
use serde_json::json;
pub use solidcraft_engine::doc::display_units::{format_length, length_number, length_precision, unit_scale};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

/// The key the apps keep the preferences under in eframe's storage.
pub const STORAGE_KEY: &str = "solidcraft.prefs";
/// The key earlier releases used. Compatibility shim (docs/compatibility.md): read when the
/// current key is missing, never written.
pub const LEGACY_STORAGE_KEY: &str = "solvecraft.prefs";

/// The stored preferences, from the current key or else the legacy one.
pub fn stored(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    get(STORAGE_KEY).or_else(|| get(LEGACY_STORAGE_KEY))
}

mod design;
mod general;
mod graphics;
mod interface;
mod navigation;
mod sketch;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::tool_modifiers::ModifierCombo;
use crate::ui_kit;
use crate::viewport::NavMode;

pub const UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];
pub const OPENSCAD_NAV: &str = "openscad";
pub const TOUCHPAD_NAV: &str = "touchpad";
/// A mouse button that can drag the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavButton {
    Left,
    Middle,
    Right,
}

/// Modifier keys a binding needs held (others may be held too).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NavKeys {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl NavKeys {
    const NONE: NavKeys = NavKeys { shift: false, ctrl: false, alt: false };
    const SHIFT: NavKeys = NavKeys { shift: true, ctrl: false, alt: false };
    const CTRL: NavKeys = NavKeys { shift: false, ctrl: true, alt: false };
    const SHIFT_ALT: NavKeys = NavKeys { shift: true, ctrl: false, alt: true };

    /// All of these keys are held (Ctrl is Cmd on macOS).
    fn held(self, m: Modifiers) -> bool {
        (!self.shift || m.shift) && (!self.ctrl || m.ctrl || m.command) && (!self.alt || m.alt)
    }
}

/// One camera binding: dragging `button` with `keys` held does `mode`.
#[derive(Clone, Copy, Debug)]
pub struct NavBinding {
    pub button: NavButton,
    pub keys: NavKeys,
    pub mode: NavMode,
}

const fn bind(button: NavButton, keys: NavKeys, mode: NavMode) -> NavBinding {
    NavBinding { button, keys, mode }
}

/// A mouse mapping: its bindings, most specific first (the first one whose button is dragged
/// with all its keys held wins).
pub struct NavPreset {
    pub id: &'static str,
    pub label: &'static str,
    /// What the mouse does, for the Preferences page.
    pub help: &'static str,
    /// Choosing the preset turns the wheel the other way.
    pub reverse_zoom: bool,
    pub bindings: &'static [NavBinding],
}

impl NavPreset {
    /// What dragging `button` with `mods` held does, if anything.
    pub fn mode(&self, button: NavButton, mods: Modifiers) -> Option<NavMode> {
        self.bindings.iter().find(|b| b.button == button && b.keys.held(mods)).map(|b| b.mode)
    }
}

use NavButton::{Left, Middle, Right};
use NavMode::{Orbit, Pan, Zoom};

/// The mouse mappings. The first is the default.
pub const NAV_PRESETS: [NavPreset; 8] = [
    NavPreset {
        id: "fusion",
        label: "Fusion",
        help: "Middle drag pans, Shift+middle orbits, right drag orbits",
        reverse_zoom: false,
        bindings: &[bind(Middle, NavKeys::SHIFT, Orbit), bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "solidworks",
        label: "SolidWorks",
        help: "Middle drag orbits, Ctrl+middle pans, Shift+middle zooms, right drag orbits",
        reverse_zoom: true,
        bindings: &[
            bind(Middle, NavKeys::CTRL, Pan),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "inventor",
        label: "Inventor",
        help: "Middle drag pans, Shift+middle orbits, Ctrl+middle zooms, right drag orbits",
        reverse_zoom: false,
        bindings: &[
            bind(Middle, NavKeys::SHIFT, Orbit),
            bind(Middle, NavKeys::CTRL, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "alias",
        label: "Alias",
        help: "Shift+Alt+left drag orbits, Shift+Alt+middle pans, Shift+Alt+right zooms; middle drag also pans and right drag orbits",
        reverse_zoom: false,
        bindings: &[
            bind(Left, NavKeys::SHIFT_ALT, Orbit),
            bind(Middle, NavKeys::SHIFT_ALT, Pan),
            bind(Right, NavKeys::SHIFT_ALT, Zoom),
            bind(Middle, NavKeys::CTRL, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "blender",
        label: "Blender",
        help: "Middle drag orbits, Shift+middle pans, Ctrl+middle zooms; right drag leaves the camera alone",
        reverse_zoom: false,
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "tinkercad",
        label: "Tinkercad",
        help: "Right drag orbits, middle or Shift+right drag pans",
        reverse_zoom: false,
        bindings: &[bind(Right, NavKeys::SHIFT, Pan), bind(Right, NavKeys::NONE, Orbit), bind(Middle, NavKeys::NONE, Pan)],
    },
    NavPreset {
        id: OPENSCAD_NAV,
        label: "OpenSCAD",
        help: "Left drag on empty space orbits, Shift+left drag box-selects, sketch geometry drags edit; right or middle drag pans, Shift+right or Shift+middle drag zooms",
        reverse_zoom: false,
        bindings: &[
            bind(Left, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Right, NavKeys::NONE, Pan),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: TOUCHPAD_NAV,
        label: "Touchpad",
        help: "Hold Shift and move the pointer to pan; hold Alt/Option and move it to orbit. No mouse button is needed. Vertical scroll zooms.",
        reverse_zoom: false,
        bindings: &[bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
];

impl NavPreset {
    /// Button-free motion has exact modifiers, unlike the additive drag bindings.
    pub fn motion_mode(&self, mods: Modifiers) -> Option<NavMode> {
        if self.id != TOUCHPAD_NAV || mods.ctrl || mods.command || mods.mac_cmd {
            return None;
        }
        match (mods.shift, mods.alt) {
            (true, false) => Some(Pan),
            (false, true) => Some(Orbit),
            _ => None,
        }
    }

    fn translated_label(&self) -> &str {
        if self.id == TOUCHPAD_NAV { crate::i18n::tr("nav.touchpad.label") } else { crate::i18n::tr_label(self.label) }
    }
}

impl Prefs {
    /// Touchpad scroll zooms without overwriting the user's preference for other presets.
    pub fn trackpad_pans(&self) -> bool {
        self.nav != TOUCHPAD_NAV && self.trackpad_pan
    }
}

/// The preset with this id (the default for an unknown one).
pub fn nav_preset(id: &str) -> &'static NavPreset {
    NAV_PRESETS.iter().find(|p| p.id == id).unwrap_or(&NAV_PRESETS[0])
}
/// Stable control-channel names; presentation labels may be shorter.
pub const PAGES: [&str; 9] = ["General", "Interface", "Design", "Navigation", "Keyboard Shortcuts", "Sketch", "Graphics", "AI", "Plugins"];
pub const PAGE_GENERAL: usize = 0;
pub const PAGE_INTERFACE: usize = 1;
pub const PAGE_DESIGN: usize = 2;
pub const PAGE_NAVIGATION: usize = 3;
pub const PAGE_SHORTCUTS: usize = 4;
pub const PAGE_SKETCH: usize = 5;
pub const PAGE_GRAPHICS: usize = 6;
pub const PAGE_AI: usize = 7;
pub const PAGE_PLUGINS: usize = 8;
const WINDOW_PAGES: [ui_kit::WindowPage<SolveApp>; 9] = [
    ui_kit::WindowPage { icon: "settings", label: "ui.general", group: None, handle: "prefs:page:General", scroll: true, draw: window_page::<0> },
    ui_kit::WindowPage {
        icon: "settings_palette",
        label: "prefs.interface",
        group: None,
        handle: "prefs:page:Interface",
        scroll: true,
        draw: window_page::<1>,
    },
    ui_kit::WindowPage { icon: "ws_design", label: "ui.design", group: None, handle: "prefs:page:Design", scroll: true, draw: window_page::<2> },
    ui_kit::WindowPage {
        icon: "settings_mouse",
        label: "ui.navigation",
        group: Some("prefs.input"),
        handle: "prefs:page:Navigation",
        scroll: true,
        draw: window_page::<3>,
    },
    ui_kit::WindowPage {
        icon: "settings_keyboard",
        label: "prefs.shortcuts",
        group: None,
        handle: "prefs:page:Keyboard Shortcuts",
        scroll: false,
        draw: window_page::<4>,
    },
    ui_kit::WindowPage {
        icon: "sketch",
        label: "prefs.sketch_selection",
        group: None,
        handle: "prefs:page:Sketch",
        scroll: true,
        draw: window_page::<5>,
    },
    ui_kit::WindowPage {
        icon: "settings_monitor",
        label: "ui.graphics",
        group: Some("prefs.display"),
        handle: "prefs:page:Graphics",
        scroll: true,
        draw: window_page::<6>,
    },
    ui_kit::WindowPage {
        icon: "assistant",
        label: "prefs.assistant",
        group: Some("prefs.extend"),
        handle: "prefs:page:AI",
        scroll: true,
        draw: window_page::<7>,
    },
    ui_kit::WindowPage {
        icon: "settings_plug",
        label: "ui.plugins",
        group: None,
        handle: "prefs:page:Plugins",
        scroll: true,
        draw: window_page::<8>,
    },
];

fn search_header(ui: &mut egui::Ui, app: &mut SolveApp) {
    let (field, _) =
        ui_kit::search_actions_row(ui, egui::Id::new("prefs_search"), &mut app.prefs_window.filter, crate::i18n::tr("prefs.search"), &[]);
    crate::scenario::publish_handle("prefs:search", field.rect.center());
    crate::prefs_search::set_force_scroll(ui.ctx(), !app.prefs_window.filter.trim().is_empty());
    ui.add_space(Tokens::get().space_8);
}

fn window_page<const INDEX: usize>(ui: &mut egui::Ui, app: &mut SolveApp) {
    if let Some(rect) = crate::scenario::rect_of("prefs:page:Sketch") {
        crate::scenario::publish_target("prefs:page:Selection", rect);
    }
    if app.prefs_window.section != INDEX {
        crate::keymap::stop_capture(app);
        app.keymap.read_only = false;
        app.prefs_window.section = INDEX;
    }
    page(app, ui, &Tokens::get());
}

pub const THEMES: [(&str, &str); 3] = [("dark", "Dark"), ("light", "Light"), ("system", "System")];
pub const MSAA: [u32; 4] = [1, 2, 4, 8];
/// Supersampling factors (#1288): off, 1.5× and 2× the viewport's pixels each way.
pub const SUPERSAMPLING: [f32; 3] = [1.0, 1.5, 2.0];

/// Original view navigators, independent of the mouse navigation preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorStyle {
    /// Also what a navigator this version doesn't know reads as (`lenient`).
    #[default]
    Axes,
    Cube,
}

impl NavigatorStyle {
    pub const ALL: [Self; 2] = [Self::Axes, Self::Cube];

    pub fn label(self) -> &'static str {
        match self {
            Self::Axes => "Signed axes",
            Self::Cube => "Cube",
        }
    }
}

/// How the History at the left lists sketches (Preferences: "Sketches", #6489). The timeline
/// along the bottom always keeps the creation order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistorySketches {
    /// Every sketch is a row of its own, in creation order (how it always was). Also what a
    /// value this version doesn't know reads as (`lenient`).
    #[default]
    InOrder,
    /// A sketch a feature is built from is listed one level in, under that feature.
    UnderFeature,
}

/// What the interface calls the origin planes (Preferences › Interface). Only the names shown
/// change: the planes' ids (`XY`, `XZ`, `YZ`), commands, scripts and saved designs keep theirs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginPlaneNames {
    /// XY, XZ and YZ. Also what a naming this version doesn't know reads as (`lenient`).
    #[default]
    Xyz,
    /// Top for XY, Front for XZ and Right for YZ (Z up), the engine's aliases for them.
    TopFrontRight,
}

impl OriginPlaneNames {
    pub const ALL: [Self; 2] = [Self::Xyz, Self::TopFrontRight];
    /// The origin planes' ids, in the order the Preferences list them.
    pub const PLANES: [&'static str; 3] = ["XY", "XZ", "YZ"];

    /// The name shown for the plane `name`: an origin plane in this naming, in the interface's
    /// language; any other plane (a construction plane) by its own name.
    pub fn label(self, name: &str) -> &str {
        match (self, name) {
            (Self::TopFrontRight, "XY") => crate::i18n::tr("ui.top"),
            (Self::TopFrontRight, "XZ") => crate::i18n::tr("ui.front"),
            (Self::TopFrontRight, "YZ") => crate::i18n::tr("ui.right"),
            _ => name,
        }
    }
}

/// What an orbit turns around (Preferences: "Orbit around").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbitAround {
    /// The world origin: the view turns about it and keeps it still on screen.
    Origin,
    /// The view's centre: what Pan moves and Fit frames (the default, as it always was).
    #[default]
    ViewCentre,
    /// The model point under the cursor when the drag began.
    Cursor,
}

/// Where the view goes when a sketch ends (Preferences: "When a sketch ends", #7038).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SketchExitView {
    /// Back to the view from before the sketch opened.
    Previous,
    /// The view stays where it is (the default since #4909).
    #[default]
    Keep,
    /// Back to the view from before the sketch, unless the sketch view was turned: a view only
    /// panned or zoomed goes back, a turned one stays.
    UnlessRotated,
}

impl SketchExitView {
    pub const ALL: [Self; 3] = [Self::Previous, Self::Keep, Self::UnlessRotated];

    /// The message id of the choice's label.
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Previous => "prefs.sketch_exit_view.previous",
            Self::Keep => "prefs.sketch_exit_view.keep",
            Self::UnlessRotated => "prefs.sketch_exit_view.unless_rotated",
        }
    }

    /// The view to turn to when a sketch ends with the camera at `now`, or `None` to leave it.
    /// `before` is the view from before the sketch opened and `facing` the one it turned to then.
    /// Going back keeps `now`'s projection, so a perspective switch made in the sketch stays.
    pub fn view_after(self, before: Camera, facing: Camera, now: Camera) -> Option<Camera> {
        let back = Camera { fov: now.fov, ..before };
        match self {
            Self::Previous => Some(back),
            Self::Keep => None,
            Self::UnlessRotated => same_orientation(facing, now).then_some(back),
        }
    }
}

/// Whether two cameras look the same way: pans and zooms move a camera without turning it.
/// Half a degree absorbs rounding in the animation that turned the view to face the sketch.
fn same_orientation(a: Camera, b: Camera) -> bool {
    let near = 0.5_f64.to_radians().cos();
    let ((ar, au, ab), (br, bu, bb)) = (a.basis(), b.basis());
    ar.dot(br) > near && au.dot(bu) > near && ab.dot(bb) > near
}

/// Which program reads the 3D mouse on Linux when both spacenavd and the device itself (hidraw)
/// are there (Preferences: "3D mouse driver"). Read by the app's reader, not by this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMouseDriver {
    /// The device itself (hidraw) when it opens, else spacenavd. Spacenavd has been a known source of
    /// trouble, and a device read directly keeps its settings across a replug.
    #[default]
    Automatic,
    Spacenavd,
    /// The hidraw device, bypassing spacenavd's settings.
    Direct,
}

/// Where the small 3D mouse picture sits in the 3D view (Preferences: "Position").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceMouseGhost {
    /// The lower left corner of the viewport area, right of the browser (how it was first made).
    /// Saved as `current` by the first build, which is still read.
    #[default]
    #[serde(alias = "current")]
    LeftViewport,
    /// The lower right corner of the viewport area. Saved as `right` by the first build.
    #[serde(alias = "right")]
    RightViewport,
    /// The lower left corner of the whole window, under the browser. Saved as `left` by the first build.
    #[serde(alias = "left")]
    LeftWindow,
}

impl SpaceMouseGhost {
    pub const ALL: [Self; 3] = [Self::LeftViewport, Self::RightViewport, Self::LeftWindow];
}

impl SpaceMouseDriver {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::Spacenavd, Self::Direct];

    /// The choice as the reader thread shares it (an `AtomicU8`).
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Back from [`Self::to_u8`]; anything else reads as Automatic.
    pub fn from_u8(value: u8) -> Self {
        Self::ALL.into_iter().find(|driver| driver.to_u8() == value).unwrap_or_default()
    }
}

/// The browser sidebar's first width, and the width [`Prefs::default`] restores; the panel
/// itself clamps what the user drags to its own range.
pub const DEFAULT_BROWSER_WIDTH: f32 = 250.0;

/// The browser's first share of the column it shares with History under it (#7371): the equal
/// split; the column clamps what the user drags so both cards keep their header and two rows.
pub const DEFAULT_BROWSER_SHARE: f32 = 0.5;

// The stored fields, their defaults, page resets and simple form controls share this table.
// Omitted pages keep fields such as grid_follows_view and added agents when restoring defaults.
// Accepted control-channel ranges may differ from slider ranges (look_at_speed).
macro_rules! preference_settings {
    ($( $(#[$doc:meta])* $field:ident: $ty:ty = $default:expr $(, $page:expr)? $(, accept($accepted:expr))? $(=> $control:ident($($args:tt)*))? ;)*) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Prefs { $($(#[$doc])* pub $field: $ty,)* }

        impl Default for Prefs {
            fn default() -> Self { Self { $($field: $default,)* } }
        }

        fn reset_preferences(p: &mut Prefs, page: usize) {
            let d = Prefs::default();
            $($(if page == $page { p.$field = d.$field; })?)*
        }

        mod accepted {
            use super::*;
            $($(pub(super) fn $field(p: &Prefs) -> bool { ($accepted).contains(&p.$field) })?)*
        }

        mod setting {
            use super::*;
            $($(pub(super) fn $field(app: &mut SolveApp, ui: &mut egui::Ui) -> egui::Response {
                preference_settings!(@control app, ui, $field, $control($($args)*))
            })?)*
        }
    };
    (@control $app:ident, $ui:ident, $field:ident, check($label:literal $(, $help:literal)?)) => {
        ui_kit::checkbox($ui, crate::i18n::tr_label($label), &mut $app.preferences.$field)
            $(.on_hover_text(crate::i18n::tr_label($help)))?
    };
    (@control $app:ident, $ui:ident, $field:ident, check_tr($label:literal)) => {
        ui_kit::checkbox($ui, crate::i18n::tr($label), &mut $app.preferences.$field)
    };
    (@control $app:ident, $ui:ident, $field:ident, check_id($label:literal)) => {
        ui_kit::checkbox($ui, $app.tr($label), &mut $app.preferences.$field)
    };
    (@control $app:ident, $ui:ident, $field:ident, number($label:literal, $range:expr)) => {
        ui_kit::row($ui, $app.tr($label), |ui| {
            ui_kit::number_field(ui, &mut $app.preferences.$field, $range, 1.0, "")
        })
    };
    (@control $app:ident, $ui:ident, $field:ident, enabled_slider($enabled:ident, $label:literal, $range:expr, $step:expr, $format:expr)) => {
        ui_kit::row($ui, $app.tr_label($label), |ui| {
            ui.add_enabled_ui($app.preferences.$enabled, |ui| {
                ui_kit::slider(ui, &mut $app.preferences.$field, $range, ui_kit::Slider { step: Some($step), format: $format, ..Default::default() })
            }).inner
        })
    };
    (@control $app:ident, $ui:ident, $field:ident, slider($label:literal, $range:expr, $step:expr, $format:expr, $rect:literal)) => {
        ui_kit::row($ui, $app.tr($label), |ui| {
            let response = ui_kit::slider(ui, &mut $app.preferences.$field, $range, ui_kit::Slider { step: Some($step), format: $format, ..Default::default() });
            crate::scenario::publish_rect($rect, response.rect);
            response
        })
    };
}

preference_settings! {
    /// Units of new designs.
    default_units: String = "mm".into(), PAGE_DESIGN;
    /// Mouse mapping: `fusion`, `solidworks` or `inventor`.
    nav: String = "fusion".into(), PAGE_NAVIGATION;
    /// Opt in to rotation through both poles with roll, without the Shift axis-snap spring.
    /// Independent of mouse bindings; false also preserves existing saved presets' behavior.
    free_orbit: bool = false, PAGE_NAVIGATION => check_tr("nav.free_orbit.label");
    /// With free orbit, mouse drags turn sideways about the world's up axis (a turntable without
    /// the pole limit) instead of the screen's.
    free_orbit_upright: bool = false, PAGE_NAVIGATION => check_tr("nav.free_orbit_upright.label");
    /// View orientation widget: original signed axes (default) or compact cube.
    navigator_style: NavigatorStyle = NavigatorStyle::default(), PAGE_NAVIGATION;
    /// Show the radial ring and enable hold/flick gestures for viewport context menus.
    radial_context_menu: bool = false, PAGE_NAVIGATION => check("Radial context menu");
    /// Work on the model directly in the viewport (pull a face to extrude or cut); off by default.
    direct_interaction: bool = false, PAGE_NAVIGATION => check("Direct interaction", "Work on the model directly in the viewport. Press a flat face or a profile and pull: up extrudes, down into a body cuts, and sideways movement sets the step size");
    /// Mouse gestures: a ring of tools on the right button.
    mouse_gestures: crate::mouse_gesture::Config = Default::default(), PAGE_NAVIGATION;
    /// The wheel zooms out when turned forward.
    zoom_reverse: bool = false, PAGE_NAVIGATION => check("Reverse zoom direction");
    /// What an orbit turns around.
    orbit_around: OrbitAround = OrbitAround::default(), PAGE_NAVIGATION;
    /// The cursor preference as releases before the origin option saved it (`true` = the point
    /// under the cursor). [`migrate`] folds it into `orbit_around` when saved preferences are
    /// read, so an existing choice survives the upgrade; this version only writes `false`.
    orbit_cursor: bool = false, PAGE_NAVIGATION;
    /// Whether a connected 3D mouse moves the view.
    space_mouse_enabled: bool = true, PAGE_NAVIGATION => check("Navigate with the 3D mouse");
    /// Whether a picture of the connected 3D mouse, moving as it does, sits in the 3D view's lower
    /// left corner. Off by default.
    space_mouse_in_view: bool = false, PAGE_NAVIGATION => check_id("ui.3d_mouse_show_in_view");
    /// Speed multiplier for 3D mouse navigation.
    space_mouse_speed: f32 = 1.0, PAGE_NAVIGATION;
    /// Per physical axis of the cap (push X, Y, Z, then tilt X, Y, Z), whether it moves the view
    /// the other way.
    space_mouse_reverse: [bool; 6] = [false; 6], PAGE_NAVIGATION;
    /// Per physical axis of the cap, a multiplier on top of `space_mouse_speed`; 0 switches it off.
    space_mouse_axis_speed: [f32; 6] = [1.0; 6], PAGE_NAVIGATION;
    /// Per physical axis of the cap, which of its motions drives it (`space_mouse::reassigned`),
    /// applied after calibration; each motion drives exactly one axis.
    space_mouse_axis_source: [usize; 6] = crate::space_mouse::SAME_AXES, PAGE_NAVIGATION;
    /// Which program reads the 3D mouse when spacenavd and the device itself are both there (Linux).
    space_mouse_driver: SpaceMouseDriver = SpaceMouseDriver::default(), PAGE_NAVIGATION;
    /// Where the 3D mouse picture sits in the 3D view, when it is shown there (`space_mouse_in_view`).
    space_mouse_ghost_position: SpaceMouseGhost = SpaceMouseGhost::default(), PAGE_NAVIGATION;
    /// The user turned the calibration offer off for good ("Never show again"). Unlike closing the
    /// card, which only records that device, this stops every offer, for every device: it is one flag,
    /// not kept per device key. Like the settings beside it, the dialog's Restore Defaults puts it back
    /// (the 3D mouse section's own restore leaves it).
    space_mouse_offer_never: bool = false, PAGE_NAVIGATION;
    /// Per 3D mouse (`space_mouse::Input::device`), the calibration that puts its axes on
    /// 3Dconnexion's. A device without one is read as it reports. Page resets keep calibrations.
    #[serde(deserialize_with = "calibrations_that_read")]
    space_mouse_calibrations: std::collections::BTreeMap<String, solidcraft_space_mouse::Calibration> = Default::default();
    /// Per 3D mouse, what each of its buttons does, by number from 1, once the user has changed
    /// any of them; until then a measured device has its printed keys' jobs
    /// (`space_mouse::default_button_jobs`). Kept, like calibrations, when settings are reset.
    #[serde(deserialize_with = "button_jobs_that_read")]
    space_mouse_buttons: std::collections::BTreeMap<String, std::collections::BTreeMap<u8, crate::space_mouse::ButtonJob>> = Default::default();
    /// The 3D mice (`space_mouse::Input::device`) whose calibration has been offered and dismissed, so
    /// the offer (`space_mouse_offer`) is made once per device. A saved entry that is not text is dropped.
    /// Kept when settings are reset, like the calibrations and button mappings it goes with: resetting
    /// the page would otherwise bring the offer back for every device.
    #[serde(deserialize_with = "devices_that_read")]
    space_mouse_offered: std::collections::BTreeSet<String> = Default::default();
    /// A device's own app on this computer moves the view (the navigation bridge, docs/3d-mouse.md):
    /// the desktop app connects to it on `nav_bridge_port`. Off unless asked for, so the app
    /// doesn't connect to whatever else listens on that port.
    nav_bridge_enabled: bool = false, PAGE_NAVIGATION;
    /// The loopback port the device app listens on (0: none).
    nav_bridge_port: u16 = 8765, PAGE_NAVIGATION;
    /// Anti-aliasing samples (applies on restart).
    msaa: u32 = 4, PAGE_GRAPHICS;
    /// Linux: the window system the app opens with at its next start (#3083): `auto` (XWayland
    /// whenever there is an X display, native Wayland on niri), `wayland` or `x11`.
    window_system: String = "auto".into(), PAGE_GRAPHICS;
    /// Exact graphics adapter name; empty selects automatically on the next start.
    graphics_adapter: String = String::new(), PAGE_GRAPHICS;
    /// Soft contact shading in the GPU viewport; optional and applied immediately.
    ambient_occlusion: bool = false, PAGE_GRAPHICS;
    /// Optional schematic markings on threaded cylindrical walls; off for new and older preferences.
    cosmetic_threads: bool = false, PAGE_GRAPHICS;
    /// The grid turns to the upright origin plane facing a side view (#1470). Off by default: the
    /// grid stays on the ground plane (XY) whatever the view.
    grid_follows_view: bool = false => check("Grid follows the view", "Side views show the grid on the upright plane facing them instead of the ground");
    /// Draw reference geometry such as inactive sketches and datum planes through shaded bodies.
    /// Off by default to preserve the depth-tested viewport behaviour.
    reference_geometry_through_bodies: bool = false, PAGE_GRAPHICS => check_id("settings.reference_geometry_through_bodies");
    /// How finely the viewport tessellates curved faces and edges: `coarse`, `normal`, `fine` or
    /// `ultra` (#1287). Only the display plan the viewport draws; never the design or exports.
    display_quality: String = "normal".into(), PAGE_GRAPHICS;
    /// Supersampling (#1288): the view drawn at this many times its pixels each way and filtered
    /// down (1, 1.5 or 2; 1 is off).
    supersampling: f32 = 1.0, PAGE_GRAPHICS;
    /// Supersample only while the view holds still (full speed while orbiting).
    supersampling_adaptive: bool = true, PAGE_GRAPHICS => check_tr("ui.supersampling_adaptive");
    /// Show the frame rate in the view's corner (the view then redraws every frame).
    show_frame_rate: bool = false, PAGE_GRAPHICS => check_id("ui.frame_rate");
    /// With the frame rate: a frame-time graph, a histogram and what the GPU drew (#2548).
    frame_rate_details: bool = false, PAGE_GRAPHICS;
    /// Draw a wider rim under finished sketch curves to improve contrast on shaded faces.
    sketch_halo: bool = false, PAGE_GRAPHICS => check("Halo under sketch curves");
    /// Show world X/Y/Z component dimensions beside the total distance in Measure.
    measure_axis_distances: bool = true, PAGE_GRAPHICS => check_id("settings.measure_axis_distances");
    /// Decimals shown for lengths and angles.
    length_decimals: u8 = 2, PAGE_DESIGN;
    angle_decimals: u8 = 1, PAGE_DESIGN;
    /// Up/Down and the wheel over a focused value field step its plain number (`value_step`,
    /// #2320). Off by default: the keys move the text cursor and the wheel scrolls, as before.
    step_values: bool = false, PAGE_DESIGN => check_id("ui.step_values");
    /// Reopen the saved designs that were open when the app was last closed (#5542).
    reopen_designs: bool = true, PAGE_GENERAL;
    /// Those designs' files, in tab order, as the last quit left them.
    open_designs: Vec<String> = Vec::new(), PAGE_GENERAL;
    /// `dark`, `light` or `system` (follow the operating system), or a theme file's id: a
    /// built-in one (`gruvbox-dark`, …) or one of `custom_themes` (#2255).
    theme: String = "dark".into(), PAGE_INTERFACE;
    /// The 3D view's colours: the theme's own, or the Light or Dark theme's whatever the window
    /// uses (#4146).
    viewport_theme: crate::theme::ViewportTheme = crate::theme::ViewportTheme::Theme, PAGE_INTERFACE;
    /// Imported theme files (`theme_files`), kept here so they also work in the browser.
    custom_themes: Vec<crate::theme_files::ThemeFile> = Vec::new();
    /// Desktop: a `solidcraft-theme/1` file another tool writes, followed as the theme and
    /// reloaded when it changes (#7573). Empty (the default): no file is followed.
    theme_follow: String = String::new(), PAGE_INTERFACE;
    /// Interface language (a registered locale ID).
    language: String = "en".into(), 0;
    /// Shared workspace composition; system windows and popups retain their purpose.
    layout: crate::shell::LayoutMode = Default::default(), PAGE_INTERFACE;
    /// Show command names beneath ribbon icons, independently of workspace layout.
    ribbon_labels: bool = true, PAGE_INTERFACE;
    /// Where a dialog's OK and Cancel sit: under its inputs (the default) or in its title row (#6708).
    dialog_buttons: crate::ui_kit::DialogButtons = Default::default(), PAGE_INTERFACE;
    /// Interface scale (1 = 100 %).
    ui_scale: f32 = 1.0, PAGE_INTERFACE, accept(0.5..=2.5);
    /// Interface font: an installed font family (or a font file), or empty for the built-in one
    /// (#1290). Menus, dialogs and labels use it; glyphs it lacks fall back as before.
    ui_font: String = String::new(), PAGE_INTERFACE;
    /// While a dialog or a sketch is open, OK and Cancel in the view's top right corner, with the
    /// axis view moved down below them (#6709). Off by default.
    view_confirm_corner: bool = false, PAGE_INTERFACE => check_id("prefs.view_confirm_corner");
    /// Put the feature history to the left of the browser instead of below the viewport.
    timeline_left: bool = false, PAGE_INTERFACE;
    /// Measured outer width; the contents react to this width without a names mode.
    timeline_left_width: f32 = 130.0, PAGE_INTERFACE;
    /// Share the browser column, vertically, instead of a separate left column.
    timeline_under_browser: bool = false, PAGE_INTERFACE;
    /// Fold the shared-column history to its header.
    timeline_collapsed: bool = false, PAGE_INTERFACE;
    /// The browser's share of the column's height when History sits under it, which the user
    /// drags at the seam between the two cards (#7371).
    browser_share: f32 = DEFAULT_BROWSER_SHARE, PAGE_INTERFACE;
    /// Whether the browser beside the vertical history is collapsed to its rail.
    timeline_left_browser_collapsed: bool = false, PAGE_INTERFACE;
    /// Whether the History at the left lists a sketch under the feature built from it.
    history_sketches: HistorySketches = HistorySketches::default(), PAGE_INTERFACE;
    /// The vertical history (left, or under the browser) starts with the active component's Bodies
    /// folder and the Origin, as the browser shows them (#6490). The bottom strip is unchanged.
    timeline_origin_bodies: bool = false, PAGE_INTERFACE;
    /// Width of the browser sidebar, which
    /// the user drags wider or narrower at its right edge.
    browser_width: f32 = DEFAULT_BROWSER_WIDTH, PAGE_INTERFACE;
    /// Render shell preference, independent of the document and Design history.
    render_shots_strip: bool = true;
    render_inspector_side: String = "right".into();
    render_inspector_width: f32 = 305.0;
    render_browser_side: String = "left".into();
    render_browser_width: f32 = 266.0;
    /// Draw the parts browser on a translucent card over the 3D view instead of a column of its
    /// own, so the workspace keeps the whole window width.
    browser_float: bool = false, PAGE_INTERFACE;
    /// Where command dialogs open: at the view's edge, or in the left column in place of the
    /// browser and the History there (#6707).
    dialog_place: crate::dialog_column::DialogPlace = Default::default(), PAGE_INTERFACE;
    /// The names the browser, dialogs and lists show for the origin planes (#6491).
    origin_plane_names: OriginPlaneNames = OriginPlaneNames::default(), PAGE_INTERFACE;
    /// The wheel zooms toward the point under the cursor (else toward the view's centre).
    zoom_to_cursor: bool = true, PAGE_NAVIGATION => check("Zoom to the cursor", "The wheel zooms toward the point under the cursor (off: toward the view's centre)");
    /// Spread discrete wheel input across frames (off: apply each event immediately).
    smooth_wheel_zoom: bool = true, PAGE_NAVIGATION => check("Smooth wheel zoom", "Turn off to apply each wheel event immediately, without a smoothing tail");
    /// Two-finger trackpad scroll pans (off: it zooms like the wheel); pinch always zooms.
    trackpad_pan: bool = true, PAGE_NAVIGATION;
    /// Horizontal trackpad and thumb-wheel scrolling pans the view the other way (#2007).
    pan_reverse_x: bool = false, PAGE_NAVIGATION => check("Reverse horizontal pan direction", "Horizontal two-finger scrolling and the thumb wheel pan the other way");
    /// View changes (axis view, Look At, Home) animate; `look_at_speed` scales how fast.
    look_at_anim: bool = true, PAGE_NAVIGATION => check("Animate view changes", "Axis view, Look At, Home and Fit");
    look_at_speed: f32 = 1.0, PAGE_NAVIGATION, accept(0.25..=4.0) => enabled_slider(look_at_anim, "Animation speed", 0.5..=3.0, 0.25, |v| format!("{v:.2}×"));
    /// Default modelling orientation: `z` (Z up) or `y` (Y up) (#3).
    up_axis: String = "z".into(), PAGE_DESIGN;
    /// After a STEP or IGES file opens, offer to stand a Y-up part up (`stand_up`).
    offer_stand_up: bool = true, PAGE_DESIGN => check("Offer to stand up Y-up files", "After a STEP or IGES file opens with Z up, offer a quarter turn about X");
    /// Extrude: with a profile picked, a face, vertex or parallel plane picked off its plane
    /// switches the extent to To Object (off: picks only add profiles, as before).
    extrude_auto_to_object: bool = false, PAGE_DESIGN => check("Extrude to a picked target", "In Extrude, with a profile picked, picking a face, vertex or parallel plane off its plane switches the extent to To Object");
    /// Placing a Single hole snaps to the middle of the face and the centres of its round edges
    /// (`hole_place`). Off by default: a hole goes where it is clicked.
    hole_snaps: bool = false, PAGE_DESIGN => check("Snap to face and edge centres");
    /// A placed Single hole has a handle that drags it, and a pick elsewhere on a face that has a
    /// hole moves the hole there (`hole_place`). Off by default: a second pick on its face takes
    /// the hole out, and a hole is moved by taking it out and picking again.
    hole_moves: bool = false, PAGE_DESIGN => check("Move holes by clicking or dragging");
    /// The browser's Origin folder starts hidden in designs that don't keep its eye themselves.
    hide_origin: bool = false, PAGE_DESIGN => check_tr("ui.hide_origin_by_default");
    /// Handles on the preview of a feature placed on a face (Rest, Boss, Snap Fit): a marker that
    /// drags it across the model and a ring that turns it. Off by default: the feature is then placed
    /// by its click only, as before, and its Position input takes faces only.
    placement_handles: bool = false, PAGE_DESIGN => check("Placement handles", "Handles on the preview of a Rest, Boss or Snap Fit: drag the marker to move it across the model, turn it by its ring");
    /// The files a design names (linked designs, font files) may be on any network location, for
    /// organisations that keep their designs on file servers. Off (the default): only on the
    /// servers in `network_hosts`, or one the user allows when asked (`network_prompt`).
    network_locations: bool = false, PAGE_DESIGN => check_tr("prefs.network_locations");
    /// The servers the files a design names may be on ("Always allow this server").
    network_hosts: Vec<String> = Vec::new(), PAGE_DESIGN;
    /// What a middle-button double-click in the view does: `home`, `fit` or `off`.
    middle_double_click: String = "home".into(), PAGE_NAVIGATION;
    /// Colour vision mode (#778): `off`, `protan`, `deutan` or `tritan`.
    color_vision: crate::theme::ColorVision = crate::theme::ColorVision::Off, PAGE_INTERFACE;
    /// Check for updates at start and once a day (docs/updates.md).
    check_updates: bool = true, 0;
    /// When the last update check ran, and the versions skipped or put off (`update::Memo`).
    update: crate::update::Memo = crate::update::Memo::default();
    /// The variant last started in each split tool family (Rectangle, Circle, Arc, Slot, Polygon, Line,
    /// Spline): the family's button and shortcut start it again (`sketch_families`).
    sketch_variants: crate::sketch_families::Choices = crate::sketch_families::Choices::default();
    /// Personal Hole parameter combinations, shared by every design in this profile.
    /// No page reset discards this user-authored library.
    hole_presets: crate::hole_presets::Library = crate::hole_presets::Library::default();
    /// Save as mesh: the last format (`stl` or `3mf`). Older versions also kept a "send to slicer"
    /// choice here (`save_mesh_send`, `save_mesh_slicer`); a settings file that still has them loads,
    /// and they are ignored.
    save_mesh_format: String = "stl".into();
    /// Hidden edges in the current-view DXF export (false: omit, true: dashed).
    view_dxf_dashed: bool = false;
    /// Model-space chord tolerance for current-view DXF curves, in millimetres.
    view_dxf_tolerance: f64 = 0.01;
    /// Send to Slicer: the last slicer used (an installed slicer's id or a program path).
    send_slicer: String = String::new();
    /// Slicers that can load a file into the window that is already open do so (default on).
    slicer_single_instance: bool = true, 0;
    /// The line about sending again replacing the slicer's file has been shown.
    slicer_resend_hint_seen: bool = false;
    /// The user has agreed to Open with EasyPrint uploading a model to Printables' servers.
    easyprint_consent: bool = false;
    /// Save as mesh: the last quality (`standard`, `fine` or `finest`, #4747).
    save_mesh_quality: String = "standard".into();
    /// The programs the user added to the slicer list, newest first.
    custom_slicers: Vec<String> = Vec::new();
    /// AI features (docs/agent-chat.md): the master switch. Off: no AI button, shortcut, menu
    /// entry or panel, and nothing is looked for or started.
    ai_features: bool = true, PAGE_AI;
    /// The Assistant button in the application bar (the shortcut and View menu entry stay).
    ai_toolbar_button: bool = true, PAGE_AI;
    /// An agent's slow commands over the control channel (MCP) compute off the window thread,
    /// so the window stays live while they run (#1989, [`crate::control_job`]). Off: they run on
    /// the window thread, and the window waits for each, as before (the default).
    agent_off_thread: bool = false, PAGE_AI;
    /// Let AI agents connect: the localhost control channel (MCP) opens on the default port at
    /// start-up, unless `--control`/`SOLIDCRAFT_CONTROL_PORT` name one. Read at start-up.
    agent_control: bool = true, PAGE_AI;
    /// Add SolidCraft's MCP server to the AI apps installed on this computer at start-up
    /// ([`crate::ai_connect`]); off takes the entries out again.
    connect_ai_apps: bool = true, PAGE_AI;
    /// Agents the user added (the installed ones are found by themselves). API keys are never
    /// here: they live in the system keychain.
    agents: Vec<crate::agent_chat::agents::AgentConfig> = Vec::new();
    /// The agent a new conversation starts with (an agent's id; empty: the first one available).
    agent_default: String = String::new();
    /// The agent the chat last started (an agent's id; empty: none yet).
    agent_last: String = String::new();
    /// When SolidCraft starts: connect the agent started last (`agent_last`, else the one a new
    /// conversation starts with), and open the panel (#7009). Off by default: an agent starts
    /// only when a message is sent or Connect is pressed, and the panel opens when asked for.
    agent_connect_at_start: bool = false, PAGE_AI;
    agent_chat_open_at_start: bool = false, PAGE_AI;
    /// How much the agent may do without asking (`ask`, `edits`, `all`), and per agent.
    ai_permissions: crate::agent_chat::agents::Mode = crate::agent_chat::agents::Mode::Edits, PAGE_AI;
    agent_permissions: std::collections::BTreeMap<String, crate::agent_chat::agents::Mode> = Default::default(), PAGE_AI;
    /// The model last chosen in the chat, per agent: put back when the agent offers it again.
    agent_models: std::collections::BTreeMap<String, String> = Default::default();
    /// "Always allow" answers: per agent, the tools it may use without asking.
    agent_always_allow: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    /// The side the panel docks on (`left` or `right`), its width, and whether it is folded to a strip.
    agent_chat_side: String = "right".into(), PAGE_AI;
    agent_chat_width: f32 = 380.0, PAGE_AI;
    agent_chat_minimized: bool = false, PAGE_AI;
    /// Whether the chat is in its own window instead of the side panel, and that window's place
    /// and size as the system last reported them (`[x, y, width, height]`, points; `x` can be
    /// negative or past the main screen when the window sits on another one).
    agent_chat_popped: bool = false, PAGE_AI;
    agent_chat_window: Option<[f32; 4]> = None, PAGE_AI;
    /// Changes the agent may make for one message; 0 is no limit.
    agent_chat_turn_limit: u32 = crate::agent_chat::gate::DEFAULT_TURN_LIMIT, PAGE_AI;

    // ---- Sketch / selection modifier bindings (classic defaults; alternate mapping is opt-in) ----
    /// Modifier that suppresses sketch auto-snap while drawing or dragging a sketch point.
    /// Default: Alt/Option. Set to Ctrl/Cmd for the alternate mapping.
    suppress_sketch_snap: ModifierCombo = ModifierCombo::CTRL_ALT, PAGE_SKETCH;
    /// Whether Alt/Option also frees gizmo / canvas manipulator arrows (Primary always does).
    manipulator_free_alt: bool = true, PAGE_SKETCH;
    /// A sketch tool's shortcut pressed again before the first point steps to the tool's next
    /// creation mode (#2459). Off (the default): the key starts the tool again, as it always has.
    shortcut_cycles_modes: bool = false, PAGE_SKETCH;
    /// Where the view goes when a sketch ends (#7038). Keep (the default) leaves it as it is.
    sketch_exit_view: SketchExitView = SketchExitView::default(), PAGE_SKETCH;
    /// When true: Shift+click adds without removing; Primary toggles.
    /// When false (default): Shift and Primary both toggle.
    selection_shift_add_only: bool = false, PAGE_SKETCH;
    /// Prefer movable sketch points on exact selection ties; off keeps point insertion order.
    prefer_free_sketch_points: bool = true, PAGE_SKETCH;
    /// Drive Joints shows the moved parts' old place as a ghost (its Show ghost box, #5905).
    drive_joints_ghost: bool = true, PAGE_DESIGN;
    /// Plugin ids turned off (Preferences › Plugins). An installed plugin whose id is absent is on.
    disabled_plugins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new(), PAGE_PLUGINS;
}

/// What a middle-button double-click in the view does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiddleDoubleClick {
    /// Go Home (the iso view, framing what is shown).
    Home,
    /// Frame what is shown, keeping the orientation.
    Fit,
    Off,
}

impl MiddleDoubleClick {
    pub const ALL: [(MiddleDoubleClick, &'static str, &'static str); 3] =
        [(MiddleDoubleClick::Home, "home", "Home"), (MiddleDoubleClick::Fit, "fit", "Fit"), (MiddleDoubleClick::Off, "off", "Off")];

    /// The preference's setting (Home for an unknown one).
    pub fn of(app: &SolveApp) -> MiddleDoubleClick {
        Self::parse(&app.preferences.middle_double_click).unwrap_or(MiddleDoubleClick::Home)
    }

    pub fn parse(id: &str) -> Option<MiddleDoubleClick> {
        Self::ALL.iter().find(|(_, i, _)| *i == id).map(|(m, _, _)| *m)
    }

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(m, _, _)| *m == self).map_or("Home", |(_, _, l)| l)
    }
}

/// The preferences window.
#[derive(Default)]
pub struct PrefsWindow {
    pub open: bool,
    /// The page shown (index into [`PAGES`]).
    pub section: usize,
    /// Everything as it was when the dialog opened (or at the last Apply), for Cancel.
    snapshot: Option<Snapshot>,
    /// The up axis preference the view follows now (applied when it changes, and at startup).
    applied_up: Option<String>,
    /// The search text of the interface font list.
    font_filter: String,
    /// Search text on the Plugins page.
    pub(crate) plugin_query: String,
    /// The followed theme file's state (#7573): its last good theme and why a read failed.
    pub(crate) follow: crate::theme_files::Follow,
    /// Filters every page. Empty leaves the selected page as it is.
    pub filter: String,
}

/// Following a theme file needs the file system: the desktop app only (#7573).
const FOLLOW_AVAILABLE: bool = cfg!(not(target_arch = "wasm32"));

/// Follow `path` as the theme for this run (the `--theme-file` launch flag), without saving it.
pub fn follow_theme_file_for_run(app: &mut SolveApp, path: &str) -> Result<(), String> {
    if !FOLLOW_AVAILABLE {
        return Err(FOLLOW_UNAVAILABLE.into());
    }
    let path = crate::theme_files::follow_path(path)?;
    app.prefs_window.follow.launch = (!path.is_empty()).then_some(path);
    Ok(())
}

const FOLLOW_UNAVAILABLE: &str = "`theme_follow` is not available in the browser: it follows a file on this computer; import the theme instead";

/// Stop following a theme file: the preference and this run's launch flag. The chosen theme
/// takes over again, with its own dark or light.
fn stop_following(app: &mut SolveApp) {
    let following = !app.prefs_window.follow.path(&app.preferences.theme_follow).is_empty();
    app.preferences.theme_follow.clear();
    app.prefs_window.follow.launch = None;
    if following {
        let theme = app.preferences.theme.clone();
        select_theme(app, &theme);
    }
}

/// Look at the followed theme file (#7573): every second while the app runs and whenever the
/// window gains the focus. A new failure is reported once in the status bar; a recovery too.
fn follow_theme_file(app: &mut SolveApp, ctx: &egui::Context) {
    if !FOLLOW_AVAILABLE {
        return;
    }
    let (now, focused) = ctx.input(|i| (i.time, i.focused));
    let follow = &mut app.prefs_window.follow;
    let gained_focus = focused && !follow.focused;
    follow.focused = focused;
    let path = follow.path(&app.preferences.theme_follow).to_string();
    let had_error = follow.error().is_some();
    match follow.poll(&path, now, gained_focus) {
        crate::theme_files::FollowChange::None => {}
        crate::theme_files::FollowChange::Loaded(name) => {
            if had_error {
                app.set_status(format!("Theme file loaded again: {name}"), false);
            }
        }
        crate::theme_files::FollowChange::Failed(e) => app.set_status(format!("Theme file not applied: {e} (the last good theme stays on)"), true),
    }
    if !path.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(crate::theme_files::FOLLOW_INTERVAL));
    }
}

/// `ui.prefs` `theme_follow`: a path follows that file (read at once, so the reply has any
/// error), `""` stops following.
fn set_theme_follow(app: &mut SolveApp, value: &serde_json::Value, available: bool) -> Result<(), String> {
    if !available {
        return Err(FOLLOW_UNAVAILABLE.into());
    }
    let path = value.as_str().ok_or("`theme_follow` must be a theme file's path, or \"\" to stop following")?;
    let path = crate::theme_files::follow_path(path)?;
    if path.is_empty() {
        stop_following(app);
        return Ok(());
    }
    app.prefs_window.follow.launch = None;
    app.preferences.theme_follow = path.clone();
    // Read now, so the reply has any error; the next frame's look then follows on as usual.
    let _ = app.prefs_window.follow.poll(&path, f64::NEG_INFINITY, true);
    Ok(())
}

/// Open the dialog at a page.
pub fn open_at(app: &mut SolveApp, page: usize) {
    app.prefs_window.open = true;
    app.prefs_window.section = page.min(PAGES.len() - 1);
}

/// Every setting the dialog changes, for Cancel and Restore Defaults.
#[derive(Clone)]
struct Snapshot {
    prefs: Prefs,
    dark: bool,
    layout: crate::shell::LayoutMode,
    grid: bool,
    navigation_bar: bool,
    shadow: bool,
    perspective: bool,
    style: u8,
    pick_bodies: bool,
    agent: crate::agent_cursor::Settings,
    autosave: f64,
    auto_project: bool,
    keys: std::collections::BTreeMap<String, String>,
}

impl Snapshot {
    fn take(app: &SolveApp) -> Snapshot {
        Snapshot {
            prefs: app.preferences.clone(),
            dark: app.ui.dark,
            layout: app.ui.layout,
            grid: app.ui.show_grid,
            navigation_bar: app.ui.show_navigation_bar,
            shadow: app.ui.ground_shadow,
            perspective: app.ui.perspective,
            style: app.ui.visual_style,
            pick_bodies: app.ui.pick_bodies,
            agent: app.ui.agent_cursor,
            autosave: app.autosave_minutes,
            auto_project: app.session.auto_project,
            keys: app.keymap.custom.clone(),
        }
    }
    fn restore(self, app: &mut SolveApp) {
        let graphics = (app.preferences.msaa, app.preferences.window_system.clone(), app.preferences.graphics_adapter.clone());
        let units_changed = app.preferences.default_units != self.prefs.default_units;
        // Update bookkeeping isn't a setting: a check that finished while the dialog was open stays.
        let update = std::mem::take(&mut app.preferences.update);
        app.preferences = self.prefs;
        app.preferences.update = update;
        if units_changed {
            default_units_changed(app);
        }
        app.ui.dark = self.dark;
        app.ui.layout = self.layout;
        app.ui.show_grid = self.grid;
        app.ui.show_navigation_bar = self.navigation_bar;
        app.ui.ground_shadow = self.shadow;
        app.ui.perspective = self.perspective;
        app.ui.visual_style = self.style;
        app.ui.pick_bodies = self.pick_bodies;
        app.ui.agent_cursor = self.agent;
        app.autosave_minutes = self.autosave;
        set_autosave(app);
        app.session.auto_project = self.auto_project;
        app.keymap.custom = self.keys;
        if (app.preferences.msaa, app.preferences.window_system.clone(), app.preferences.graphics_adapter.clone()) != graphics {
            save_graphics(app);
        }
    }
}

/// The colour theme (#2255): Dark, Light, System, the built-in theme files and the imported
/// ones, with Import…, Export… (the theme on now as a complete file, to start a new one from)
/// and Remove (an imported theme). Like the other preferences, a choice applies at once.
fn theme_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut choices = theme_choices(app);
    // While a file is followed (#7573) it is the theme, so no tile shows as chosen.
    let following = !app.prefs_window.follow.path(&app.preferences.theme_follow).is_empty();
    choices.sort_by_key(|(id, _)| match id.as_str() {
        "light" => 0,
        "dark" => 1,
        "system" => 2,
        _ => 3,
    });
    let mut tile_action: Option<(&str, String)> = None;
    let mut action_theme: Option<String> = None;
    let mut chosen: Option<String> = None;
    let mut action: Option<&str> = None;
    ui_kit::theme_rail(
        ui,
        |ui| {
            for (id, label) in &choices {
                let palette = match id.as_str() {
                    "light" | "system" => Tokens::light(),
                    "dark" => Tokens::dark(),
                    _ => crate::theme_files::find(id, &app.preferences.custom_themes).map(crate::theme_files::resolve).unwrap_or_else(Tokens::dark),
                };
                let custom = app.preferences.custom_themes.iter().any(|t| t.id == *id);
                let note = app.tr(if id == "system" {
                    "prefs.follows_os"
                } else if custom {
                    "prefs.imported_file"
                } else {
                    "prefs.built_in"
                });
                let response = ui_kit::option_card(
                    ui,
                    label,
                    Some(note),
                    !following && app.preferences.theme == *id,
                    true,
                    ui_kit::PreferenceFigure::Theme { palette: palette.into(), system: id == "system" },
                );
                crate::scenario::publish_target(&format!("prefs:theme_tile:{id}"), response.rect);
                crate::scenario::publish_target(&format!("prefs:theme:{id}"), response.rect);
                if !following && app.preferences.theme == *id {
                    crate::scenario::publish_target("prefs:theme", response.rect);
                }
                if response.clicked() {
                    chosen = Some(id.clone());
                }
                ui_kit::Menu::context(&response, |ui| {
                    ui_kit::Menu::show(ui, Tokens::get().form_card_width, |ui| {
                        if ui_kit::MenuItem::new("theme_export", app.tr_label("Export…"), "export").show(ui).clicked() {
                            tile_action = Some(("export", id.clone()));
                            ui.close();
                        }
                        if custom {
                            let mut remove = ui_kit::MenuItem::new("theme_remove", app.tr_label("Remove"), "delete");
                            remove.danger = true;
                            if remove.show(ui).clicked() {
                                tile_action = Some(("remove", id.clone()));
                                ui.close();
                            }
                        }
                    });
                });
            }
        },
        |ui| {
            if ui_kit::theme_import_tile(ui, app.tr_label("Import…"), app.tr("prefs.theme_file")).clicked() {
                action = Some("import");
            }
        },
    );
    if let Some((kind, id)) = tile_action {
        action = Some(kind);
        action_theme = Some(id);
    }
    if let Some(id) = chosen {
        // Choosing a theme stops following a file (#7573).
        stop_following(app);
        select_theme(app, &id);
    }
    match action {
        Some("import") => app.pick_open_for(&["json"], |app, path| {
            let Some(path) = path else { return };
            // Through `vfs`, like every other import: in the web app a picked file is there.
            let r = solidcraft_engine::io::vfs::read_to_string(&path, crate::theme_files::MAX_BYTES)
                .map_err(|e| e.to_string())
                .and_then(|text| import_theme(app, &serde_json::Value::String(text)));
            match r {
                Ok(id) => {
                    let name = crate::theme_files::find(&id, &app.preferences.custom_themes).map(|t| t.name.clone()).unwrap_or(id);
                    app.set_status(format!("Theme imported: {name}"), false);
                }
                Err(e) => app.set_status(format!("Theme import: {e}"), true),
            }
        }),
        Some("export") => {
            let system_dark = ui.ctx().system_theme().map_or(app.ui.dark, |theme| theme == egui::Theme::Dark);
            let file = theme_file(app, action_theme.as_deref().unwrap_or(&app.preferences.theme), system_dark);
            let name = file.get("name").and_then(serde_json::Value::as_str).unwrap_or("Theme").to_string();
            let text = serde_json::to_string_pretty(&file).unwrap_or_default();
            app.pick_save(&format!("{name}.json"), &["json"], move |app, path| {
                // Through `vfs`, like every other save: the web app hands it out as a download.
                if let Some(path) = path
                    && let Err(e) = solidcraft_engine::io::vfs::write(&path, text.as_bytes())
                {
                    app.set_status(format!("Theme export: {e}"), true);
                }
            });
        }
        Some("remove") => {
            let id = action_theme.unwrap_or_else(|| app.preferences.theme.clone());
            if let Err(e) = remove_theme(app, &id) {
                app.set_status(e, true);
            }
        }
        _ => {}
    }
}

/// Settings › Interface › Theme: a theme file to follow (#7573), chosen with the file picker,
/// and why its last read was not applied. The desktop app only.
fn follow_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let path = app.prefs_window.follow.path(&app.preferences.theme_follow).to_string();
    let launch = app.prefs_window.follow.launch.is_some();
    let error = app.prefs_window.follow.error().map(str::to_string);
    let (mut choose, mut stop) = (false, false);
    // The setting's name is longer than the label column: it leads the row's own line.
    ui_kit::row(ui, "", |ui| {
        ui.horizontal(|ui| {
            ui_kit::text(ui, app.tr("prefs.theme_follow"), ui_kit::Text::Inline);
            let choose_button = ui_kit::button(ui, app.tr_label("Choose…"), ui_kit::Button::Small);
            crate::scenario::publish_target("prefs:theme_follow", choose_button.rect);
            choose = choose_button.clicked();
            let stop_button = ui_kit::button(ui, app.tr("prefs.theme_follow_stop"), ui_kit::Button::Enabled(!path.is_empty()));
            stop = stop_button.clicked();
            let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string());
            let shown = name.unwrap_or_else(|| app.tr("prefs.theme_follow_off").to_string());
            ui_kit::text(ui, &shown, ui_kit::Text::Inline).on_hover_text(&path);
        });
    });
    if !path.is_empty() {
        ui_kit::text(ui, app.tr(if launch { "prefs.theme_follow_launch" } else { "prefs.theme_follow_note" }), ui_kit::Text::Hint("theme_follow"));
    }
    if let Some(error) = error.filter(|_| !path.is_empty()) {
        ui_kit::text(ui, &crate::i18n::format("prefs.theme_follow_error", &[("error", &error)]), ui_kit::Text::Error("theme_follow"));
    }
    if stop {
        stop_following(app);
    }
    if choose {
        app.pick_open_for(&["json"], |app, path| {
            let Some(path) = path else { return };
            if let Err(e) = set_theme_follow(app, &serde_json::Value::String(path), FOLLOW_AVAILABLE) {
                app.set_status(format!("Follow a theme file: {e}"), true);
            }
        });
    }
}

/// The interface font (#1290): the built-in font first, then the installed families, with a
/// search box. The choice applies at once, like the other preferences.
fn ui_font_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let builtin = app.tr_label("Built-in").to_string();
    let current = if app.preferences.ui_font.trim().is_empty() { builtin.clone() } else { app.preferences.ui_font.clone() };
    let families = crate::sketch_text::families(ui.ctx());
    let mut values = vec![(String::new(), builtin.clone(), "prefs:font_builtin".to_string())];
    if let Some(families) = &families {
        values.extend(families.iter().map(|family| (family.clone(), family.clone(), format!("prefs:font:{family}"))));
    }
    let options: Vec<_> = values.iter().map(|(value, label, handle)| ui_kit::SearchOption { value, label, handle: Some(handle) }).collect();
    let chosen = ui_kit::searchable_dropdown(
        ui,
        "pref_ui_font",
        &current,
        &mut app.prefs_window.font_filter,
        &options,
        None,
        families.is_none().then(|| crate::i18n::tr_label("Listing the installed fonts…")),
    );
    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:widget:{}", crate::i18n::tr("prefs.font"))) {
        crate::scenario::publish_handle("prefs:ui_font", rect.center());
    }
    if let Some(name) = chosen {
        if name.is_empty() || crate::fonts::ui_font(&name).is_some() {
            app.preferences.ui_font = name;
        } else {
            app.set_status(format!("The font `{name}` could not be read"), true);
        }
        app.prefs_window.font_filter.clear();
    }
}

/// Restore Defaults for one page.
fn defaults(app: &mut SolveApp, page: usize) {
    let (msaa, window_system, adapter) = (app.preferences.msaa, app.preferences.window_system.clone(), app.preferences.graphics_adapter.clone());
    let units = app.preferences.default_units.clone();
    reset_preferences(&mut app.preferences, page);
    if app.preferences.default_units != units {
        default_units_changed(app);
    }
    match page {
        0 => {
            app.autosave_minutes = 5.0;
            set_autosave(app);
        }
        PAGE_INTERFACE => {
            app.prefs_window.follow.launch = None;
            app.ui.dark = true;
            app.ui.layout = app.preferences.layout;
            app.ui.show_navigation_bar = true;
        }
        PAGE_AI => app.ui.agent_cursor = Default::default(),
        PAGE_NAVIGATION | PAGE_DESIGN => {}
        PAGE_SHORTCUTS => crate::keymap::reset(app, None),
        PAGE_GRAPHICS => {
            app.ui.visual_style = 0;
            app.ui.show_grid = true;
            app.ui.ground_shadow = true;
            app.ui.perspective = false;
            if app.preferences.msaa != msaa || app.preferences.window_system != window_system || app.preferences.graphics_adapter != adapter {
                save_graphics(app);
            }
        }
        PAGE_SKETCH => {
            app.session.auto_project = true;
            app.ui.pick_bodies = false;
        }
        _ => {}
    }
}

/// Settings that follow the preferences every frame: the theme when it follows the system, and
/// the interface scale.
pub fn apply(app: &mut SolveApp, ctx: &egui::Context) {
    apply_network(&app.preferences);
    // The viewport's display quality (an unknown saved value draws at Normal).
    solidcraft_engine::doc::display_quality::set_current(
        solidcraft_engine::doc::display_quality::DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default(),
    );
    // The view's up axis follows the preference (set in the dialog, by ui.prefs or loaded).
    if app.prefs_window.applied_up.as_deref() != Some(app.preferences.up_axis.as_str()) {
        up_axis_changed(app);
    }
    if app.preferences.theme == "system"
        && let Some(t) = ctx.system_theme()
    {
        app.ui.dark = t == egui::Theme::Dark;
    }
    // A theme file's palette, in place of its base's (dark or light); the visuals follow. A
    // followed file (#7573) wins while it is followed and has read completely once.
    follow_theme_file(app, ctx);
    let file = app.prefs_window.follow.theme().or_else(|| crate::theme_files::find(&app.preferences.theme, &app.preferences.custom_themes));
    if let Some(f) = file {
        app.ui.dark = f.base.is_dark();
    }
    let custom = file.map(|f| crate::theme::Custom { dark: f.base.is_dark(), tokens: crate::theme_files::resolve(f) });
    if crate::theme::set_custom(custom) {
        app.styled = false;
    }
    // egui's end-of-frame shortcuts only change its context; the saved preference would
    // overwrite that change next frame. Consume the desktop shortcuts into the preference.
    if ctx.options(|o| o.zoom_with_keyboard) && !crate::keymap::capturing(app) {
        use egui::gui_zoom::kb_shortcuts::{ZOOM_IN, ZOOM_IN_SECONDARY, ZOOM_OUT, ZOOM_RESET};
        if ctx.input_mut(|i| i.consume_shortcut(&ZOOM_RESET)) {
            app.preferences.ui_scale = 1.0;
        } else {
            let zoom_in = ctx.input_mut(|i| i.consume_shortcut(&ZOOM_IN) || i.consume_shortcut(&ZOOM_IN_SECONDARY));
            let zoom_out = ctx.input_mut(|i| i.consume_shortcut(&ZOOM_OUT));
            if zoom_in != zoom_out {
                app.preferences.ui_scale = (app.preferences.ui_scale + if zoom_in { 0.1 } else { -0.1 }).clamp(0.5, 2.5);
            }
        }
    }
    // Scaling while the pointer is held moves the slider under the pointer, feeding a
    // different value back into it. Commit the layout change after the drag ends.
    let z = app.preferences.ui_scale.clamp(0.5, 2.5);
    if z.is_finite() && !ctx.input(|i| i.pointer.any_down()) && (ctx.zoom_factor() - z).abs() > 1e-3 {
        ctx.set_zoom_factor(z);
    }
}

/// The network locations the files a design names may be on follow the preferences.
pub fn apply_network(p: &Prefs) {
    solidcraft_engine::io::vfs::set_network_policy(solidcraft_engine::io::vfs::NetworkPolicy {
        allow_all: p.network_locations,
        hosts: p.network_hosts.clone(),
    });
}

/// A new, empty design takes the preferred units (not an undo step, not a change to save).
/// Callers: a launch with no file, New on a design that is already empty, `file.new`, and the
/// blank design left when the last tab closes. A design that already has features is unchanged.
pub fn apply_new_design(app: &mut SolveApp) {
    apply_new_design_units(app, &app.preferences.default_units.clone());
}

/// The new-design preference also applies to the blank, unsaved design already open.
fn default_units_changed(app: &mut SolveApp) {
    if crate::documents::active_is_blank(app) {
        apply_new_design(app);
    }
}

/// Explicit units for a new-design tile; the user's default preference stays unchanged.
pub(crate) fn apply_new_design_units(app: &mut SolveApp, units: &str) {
    if app.session.doc.features.is_empty() && app.session.doc.units != units {
        let saved = !app.session.is_dirty();
        if app.session.execute("document.units", &json!({ "units": units })).is_ok() {
            app.session.undo.pop();
            if saved {
                app.session.mark_saved();
            }
        }
    }
}

/// The autosave interval follows `app.autosave_minutes` (0: off).
pub fn set_autosave(app: &mut SolveApp) {
    let m = app.autosave_minutes;
    if let Some(a) = app.autosave.as_mut() {
        a.interval = if m > 0.0 { std::time::Duration::from_secs_f64(m * 60.0) } else { std::time::Duration::from_secs(u64::MAX / 4) };
    }
}

/// Which navigation dragging `button` with `mods` held does under the preset.
pub fn nav_mode(app: &SolveApp, button: NavButton, mods: Modifiers) -> Option<NavMode> {
    // With mouse gestures on, the right button opens the ring of tools instead of orbiting.
    if button == NavButton::Right && crate::mouse_gesture::active(app) {
        return None;
    }
    nav_preset(&app.preferences.nav).mode(button, mods)
}

/// +1, or −1 when horizontal scrolling pans the other way (#2007).
pub fn pan_sign_x(app: &SolveApp) -> f32 {
    if app.preferences.pan_reverse_x { -1.0 } else { 1.0 }
}

/// Whether the configured suppress-snap chord is held.
pub fn suppress_sketch_snap_held(app: &SolveApp, m: Modifiers) -> bool {
    let c = app.preferences.suppress_sketch_snap;
    let held = crate::tool_modifiers::Held::from_modifiers(m);
    // Classic default (`CTRL_ALT`): Ctrl/Cmd **or** Alt/Option each suppress snap.
    // A single-key preference keeps the usual AtLeast match for that key only.
    if c == ModifierCombo::CTRL_ALT {
        return held.primary || held.alt;
    }
    c.held(m)
}

/// Primary always frees manipulator arrows; Alt/Option also frees when `manipulator_free_alt` is on.
pub fn manipulator_free_held(app: &SolveApp, m: Modifiers) -> bool {
    use crate::tool_modifiers::Held;
    let held = Held::from_modifiers(m);
    held.primary || (app.preferences.manipulator_free_alt && held.alt)
}

/// +1, or −1 when the wheel zooms the other way.
pub fn zoom_sign(app: &SolveApp) -> f64 {
    if app.preferences.zoom_reverse { -1.0 } else { 1.0 }
}

/// The model point under a screen position. Over empty space: the centre of the visible bodies
/// and sketches (what Fit frames), or the view's centre when nothing is visible.
pub(crate) fn pivot_under(app: &SolveApp, proj: &crate::viewport::Proj, at: Pos2) -> Vec3 {
    use crate::viewport::Hit;
    for h in crate::viewport::pick(app, proj, at) {
        match h {
            Hit::Face { point, .. } | Hit::Vertex { point, .. } => return point,
            Hit::Edge { mid, .. } => return mid,
            _ => {}
        }
    }
    // Over empty space: the middle of the bodies in view, not of everything shown (#2616).
    if let Some(c) = crate::viewport::in_view_centre(app, proj) {
        return c;
    }
    let b = crate::viewport::view_bounds(app);
    if b.is_empty() { app.cam.target } else { b.center() }
}

/// A plain number typed into a field, with a decimal point or a decimal comma (#1347): "2.5"
/// and "2,5" are both 2.5, as a numeric keypad types either depending on the locale. Spaces are
/// ignored and "−" is a minus, as in egui's own parser. A comma beside a point ("1.234,5") or a
/// second comma is not guessed at: the text is not a number.
pub fn parse_number(text: &str) -> Option<f64> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == '\u{2212}' { '-' } else { c }).collect();
    let t = if !t.contains('.') && t.matches(',').count() == 1 { t.replacen(',', ".", 1) } else { t };
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

// Existing callers retain their numeric-editor API; kit owns the widget.

/// A length typed into a field, in millimetres: a bare number is in the design's units (`scale`
/// units per millimetre); an explicit unit ("10 mm", "0.5 in", "2 cm + 3 mm") is honoured.
pub fn parse_length_mm(text: &str, scale: f64) -> Option<f64> {
    use solidcraft_engine::doc::expr;
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let defaults = expr::Defaults { len: 1.0 / scale, ..expr::Defaults::default() };
    let no_params = |n: &str| Err(solidcraft_engine::doc::DocError::Expr(format!("unknown parameter `{n}`")));
    let value = expr::eval_with_defaults(text.trim(), &no_params, defaults).ok()?.to_kind_in(expr::Kind::Length, defaults).ok()?;
    value.is_finite().then_some(value)
}

/// A length, area (`power` 2) or volume (3) given in millimetres, shown in the design's units
/// with the length precision.
pub fn show_mm(app: &SolveApp, v: f64, power: i32) -> String {
    let (k, u) = unit_scale(&app.session.doc.units);
    let sup = match power {
        2 => "²",
        3 => "³",
        _ => "",
    };
    let value = v * k.powi(power);
    format!("{:.*} {u}{sup}", length_precision(value, &app.session.doc.units, app.preferences.length_decimals), value)
}

/// Numbers in millimetres, in the design's units without the unit name.
pub fn show_mm_bare(app: &SolveApp, v: f64) -> String {
    length_number(v, &app.session.doc.units, app.preferences.length_decimals)
}

/// The window system choices (`Prefs::window_system`, #3083).
pub const WINDOW_SYSTEMS: [&str; 3] = ["auto", "wayland", "x11"];
/// winit's issue for dropping files onto a window on native Wayland, which the note links to.
pub const WAYLAND_DROP_ISSUE: &str = "https://github.com/rust-windowing/winit/issues/1881";

/// Linux: Graphics › Window system (#3083). It applies at the next start; the note under it says
/// which one is running now, and when native Wayland is in use or chosen, that some features are
/// limited there, linking to [`WAYLAND_DROP_ISSUE`].
#[cfg(target_os = "linux")]
fn window_system_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let label = |id: &str| crate::i18n::tr(&format!("ui.window_system_{id}")).to_string();
    let labels = WINDOW_SYSTEMS.map(label);
    // The note below says that it applies after a restart, and which one runs now.
    let response = ui.allocate_ui_with_layout(egui::vec2(t.form_card_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        let mut selected = WINDOW_SYSTEMS.iter().position(|id| *id == app.preferences.window_system).unwrap_or(0);
        if ui_kit::segmented(ui, &mut selected, &labels.each_ref().map(String::as_str), ui_kit::Segments::default())
            && let Some(id) = WINDOW_SYSTEMS.get(selected)
        {
            app.preferences.window_system = (*id).into();
        }
    });
    crate::scenario::publish_handle("prefs:window_system", response.response.rect.center());
    let now = app.services.window_system.map(label);
    let text = match &now {
        Some(now) => crate::i18n::format("settings.window_system_note_now", &[("now", now)]),
        None => app.tr("settings.msaa_applies_on_restart").to_string(),
    };
    ui_kit::text(ui, &text, ui_kit::Text::Hint("window_system"));
    let choice = app.preferences.window_system.as_str();
    if choice == "wayland" || (choice == "auto" && app.services.window_system == Some("wayland")) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(app.tr("settings.window_system_wayland_limited")).size(t.font_small).color(t.warning));
            ui.hyperlink_to(egui::RichText::new(app.tr("settings.window_system_details")).size(t.font_small), WAYLAND_DROP_ISSUE);
        });
    }
}

/// Write the anti-aliasing and window system settings where the host reads them at start (neither
/// can change while the window is open).
fn save_graphics(app: &SolveApp) {
    if let Some(path) = &app.services.graphics_path {
        // Other settings in the file (the graphics `backend`) stay as they are.
        let mut graphics = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| json!({}));
        graphics["msaa"] = json!(app.preferences.msaa);
        graphics["window_system"] = json!(app.preferences.window_system);
        graphics["adapter"] = json!(app.preferences.graphics_adapter);
        let _ = std::fs::write(path, graphics.to_string());
    }
}

/// At start, reflect a confirmed startup fallback in Graphics, or reset an unsupported
/// saved count to the supported default. The compatibility hint keeps higher choices
/// available for another try; a probe result alone does not overwrite the saved choice.
pub fn check_msaa(app: &mut SolveApp, requested: u32) {
    if app.msaa_compatibility {
        app.preferences.msaa = 1;
        return;
    }
    let supported = &app.msaa_supported;
    if requested <= 1 || supported.is_empty() || supported.contains(&requested) {
        return;
    }
    let default = Prefs::default().msaa;
    let reset = supported.iter().copied().filter(|&n| n <= default).max().unwrap_or(1);
    let supported = supported.clone();
    app.preferences.msaa = reset;
    save_graphics(app);
    app.settings_reset.push(SettingReset::Msaa { requested, reset, supported });
}

/// A setting put back at start because it would keep SolidCraft from starting.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingReset {
    /// Anti-aliasing samples the graphics device can't render with.
    Msaa { requested: u32, reset: u32, supported: Vec<u32> },
}

impl SettingReset {
    /// The line the notice shows, in the current interface language.
    pub fn message(&self) -> String {
        match self {
            SettingReset::Msaa { requested, reset, supported } => {
                let setting = format!("{} › {}", crate::i18n::tr("ui.graphics"), crate::i18n::tr("ui.anti_aliasing"));
                let supported = supported.iter().map(|n| format!("{n}")).collect::<Vec<_>>().join("×, ") + "×";
                crate::i18n::format(
                    "settings.msaa_reset",
                    &[("setting", &setting), ("requested", &requested.to_string()), ("reset", &reset.to_string()), ("supported", &supported)],
                )
            }
        }
    }
}

/// The Preferences window can't be folded (`collapsible(false)`), but 0.14 and 0.15 folded it
/// when a font was picked, and egui keeps that in the saved window state: it then opened as its
/// title bar alone, for good (#1800). Unfold it.
fn unfold(ctx: &egui::Context) {
    if let Some(mut body) = egui::collapsing_header::CollapsingState::load(ctx, egui::Id::new("sc_prefs").with("collapsing"))
        && !body.is_open()
    {
        body.set_open(true);
        body.store(ctx);
    }
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    // File › Keyboard Shortcuts and Help › Keyboard Shortcuts open this dialog at that page.
    if app.keymap.open {
        app.keymap.open = false;
        open_at(app, PAGE_SHORTCUTS);
    }
    if !app.prefs_window.open {
        app.prefs_window.filter.clear();
        app.prefs_window.snapshot = None;
        // A question left up when the dialog closed is not still asked when it opens again.
        app.space_mouse.restore_pending = false;
        // Calibration opens from here and ends with it, so the cap moves the view again.
        app.space_mouse.calibrating = None;
        return;
    }
    if app.prefs_window.snapshot.is_none() {
        app.crashes.refresh();
        app.prefs_window.snapshot = Some(Snapshot::take(app));
    }
    unfold(ctx);
    let mut button: Option<&str> = None;
    let before = app.preferences.clone();
    let capturing = crate::keymap::capturing(app);
    let window = ui_kit::Window::new(egui::Id::new("sc_prefs"), "settings", app.tr("ui.preferences"))
        .cards(10.0 / 3.0)
        .page_rows(24.0)
        .page_padding(egui::vec2(Tokens::get().space_16 + Tokens::get().space_4, Tokens::get().space_16))
        .fields()
        .actions("OK", "Cancel", Some("Apply"));
    let window = if capturing { window.keep_open_on_escape() } else { window };
    let mut selected = app.prefs_window.section;
    let (closed, action) = window.show_pages_with_header(ctx, &mut selected, &WINDOW_PAGES, app, search_header, |ui| {
        if ui_kit::button(ui, crate::i18n::tr("ui.restore_defaults"), ui_kit::Button::Normal)
            .on_hover_text(crate::i18n::tr("ui.reset_the_settings_on_this_page"))
            .clicked()
        {
            button = Some("defaults");
        }
    });
    if let Some(rect) = crate::scenario::rect_of("dialog:page_body") {
        crate::scenario::publish_rect("prefs:page_body", rect);
    }
    for name in ["window", "footer", "ok", "apply", "cancel"] {
        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{name}")) {
            crate::scenario::publish_target(&format!("prefs:{name}"), rect);
        }
    }
    crate::space_mouse_calibration::show(app, ctx);
    if let Some(rect) = crate::scenario::rect_of("dialog:footer:actions") {
        crate::scenario::publish_rect("prefs:actions", rect);
    }
    button = match action {
        ui_kit::Action::Ok => Some("ok"),
        ui_kit::Action::Apply => Some("apply"),
        ui_kit::Action::Cancel => Some("cancel"),
        ui_kit::Action::None => button,
    };
    if app.preferences.msaa != before.msaa
        || app.preferences.window_system != before.window_system
        || app.preferences.graphics_adapter != before.graphics_adapter
    {
        save_graphics(app);
    }
    if app.preferences.up_axis != before.up_axis {
        up_axis_changed(app);
    }
    match button {
        Some("defaults") => defaults(app, app.prefs_window.section),
        Some("apply") => app.prefs_window.snapshot = Some(Snapshot::take(app)),
        Some("ok") => close(app, false),
        Some("cancel") => close(app, true),
        _ if closed => close(app, true),
        _ => {}
    }
}

/// Close the dialog; `revert`: put back what was there when it opened (Cancel, Esc, ×).
pub(crate) fn close(app: &mut SolveApp, revert: bool) {
    if let Some(s) = app.prefs_window.snapshot.take()
        && revert
    {
        let up = app.preferences.up_axis.clone();
        s.restore(app);
        if app.preferences.up_axis != up {
            up_axis_changed(app);
        }
    }
    app.prefs_window.open = false;
    app.keymap.read_only = false;
    crate::keymap::stop_capture(app);
}

/// The default orientation changed: the view turns to match.
fn up_axis_changed(app: &mut SolveApp) {
    use solidcraft_engine::render::UpAxis;
    app.prefs_window.applied_up = Some(app.preferences.up_axis.clone());
    let up = if app.preferences.up_axis.eq_ignore_ascii_case("y") { UpAxis::Y } else { UpAxis::Z };
    if app.ui.up_axis != up || app.cam.up != up {
        app.set_up_axis(up);
    }
}

/// One page of the dialog. Inputs share the kit's aligned rows and tokenised controls.
/// A search draws every page and keeps the controls that match.
fn page(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    let _search = crate::prefs_search::begin(ui, &app.prefs_window.filter);
    if crate::prefs_search::querying(ui) {
        for (index, item) in WINDOW_PAGES.iter().enumerate() {
            crate::prefs_search::begin_page(ui, crate::i18n::tr(item.label));
            draw_page(app, ui, index, t);
        }
        if crate::prefs_search::hits(ui) == 0 {
            ui.label(egui::RichText::new(crate::i18n::tr("prefs.search_empty")).size(t.font_body).color(t.form_dim));
        }
    } else {
        let label = WINDOW_PAGES.get(app.prefs_window.section).map(|item| crate::i18n::tr(item.label)).unwrap_or("");
        crate::prefs_search::begin_page(ui, label);
        draw_page(app, ui, app.prefs_window.section, t);
    }
}

fn draw_page(app: &mut SolveApp, ui: &mut egui::Ui, index: usize, t: &Tokens) {
    match index {
        PAGE_SHORTCUTS => crate::keymap::page(app, ui),
        PAGE_AI => crate::agent_chat::settings::page(app, ui),
        PAGE_PLUGINS => crate::plugins::page(app, ui),
        PAGE_GENERAL => general::show(app, ui),
        PAGE_INTERFACE => interface::interface(app, ui, t),
        PAGE_DESIGN => design::show(app, ui),
        PAGE_NAVIGATION => navigation::show(app, ui, t),
        PAGE_GRAPHICS => graphics::show(app, ui),
        PAGE_SKETCH => sketch::show(app, ui),
        _ => {}
    }
}

fn page_index(name: &str) -> Option<usize> {
    match name.to_ascii_lowercase().as_str() {
        "selection" | "sketch & selection" | "sketch and selection" => Some(PAGE_SKETCH),
        "assistant" => Some(PAGE_AI),
        "shortcuts" => Some(PAGE_SHORTCUTS),
        _ => PAGES.iter().position(|x| x.eq_ignore_ascii_case(name)),
    }
}

/// The 3D mouse's settings under Navigation, drawn with the same settings kit as the sections above
/// it: whether it moves the view and shows in it, where the picture goes, its speed, the driver and
/// calibration, each axis, the picture with its dead zone, and what its buttons do. A section like the
/// others, so it folds with its header and stays as the user last left it.
fn space_mouse_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "space_mouse", crate::i18n::tr("ui.3d_mouse_section"), |ui| space_mouse_rows(app, ui));
}

/// The label column the Navigation page's forms share (`prefs/navigation.rs`).
const FORM_LABEL_WIDTH: f32 = 150.0;

fn space_mouse_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // The two checkboxes sit in the two-column grid Wheel and trackpad uses (`navigation.rs`).
    let width = (ui.available_width() - (t.space_16 + t.space_4)) * 0.5;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_16 + t.space_4;
        ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.horizontal_wrapped(|ui| {
                setting::space_mouse_enabled(app, ui);
                let device = app.space_mouse.device.as_ref();
                let saved = device.is_some_and(|device| app.preferences.space_mouse_calibrations.contains_key(device));
                if let Some(device) = device {
                    let state = app.tr(if saved { "ui.3d_mouse_calibrated" } else { "ui.3d_mouse_not_calibrated" });
                    ui_kit::settings_badge(ui, &format!("{state} · {device}"), !saved);
                }
            });
        });
        ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            setting::space_mouse_in_view(app, ui).on_hover_text(crate::i18n::tr("ui.3d_mouse_show_in_view_tip"));
        });
    });
    // Where the picture goes, a choice that waits on the checkbox above.
    let names: Vec<(SpaceMouseGhost, String)> = [
        (SpaceMouseGhost::LeftViewport, "ui.3d_mouse_ghost_left_viewport"),
        (SpaceMouseGhost::RightViewport, "ui.3d_mouse_ghost_right_viewport"),
        (SpaceMouseGhost::LeftWindow, "ui.3d_mouse_ghost_left_window"),
    ]
    .map(|(position, key)| (position, app.tr(key).to_owned()))
    .into();
    ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
        ui_kit::row(ui, crate::i18n::tr("ui.3d_mouse_ghost_position"), |ui| {
            let (in_view, position) = (app.preferences.space_mouse_in_view, &mut app.preferences.space_mouse_ghost_position);
            ui.add_enabled_ui(in_view, |ui| {
                ui_kit::settings_control_width(ui, AXIS_DRIVEN_WIDTH, |ui| {
                    let current = names.iter().find(|(choice, _)| choice == position).map_or("", |(_, name)| name.as_str());
                    ui_kit::dropdown_menu(ui, "sc_space_mouse_ghost", current, |ui| {
                        for (choice, name) in &names {
                            if ui_kit::list_picker(ui, name, name, choice == position).clicked() {
                                *position = *choice;
                            }
                        }
                    });
                });
            });
        });
        ui_kit::row(ui, app.tr("prefs.overall_speed"), |ui| {
            ui.add_enabled_ui(app.preferences.space_mouse_enabled, |ui| {
                ui_kit::slider(
                    ui,
                    &mut app.preferences.space_mouse_speed,
                    0.25..=5.0,
                    ui_kit::Slider { step: Some(0.05), format: |value| format!("{value:.2}×"), ..Default::default() },
                );
            });
        });
        ui_kit::row(ui, app.tr("ui.3d_mouse_calibration"), |ui| {
            let device = app.space_mouse.device.clone();
            let saved = device.as_ref().is_some_and(|device| app.preferences.space_mouse_calibrations.contains_key(device));
            ui.add_enabled_ui(app.preferences.space_mouse_enabled && device.is_some(), |ui| {
                if ui_kit::button(ui, app.tr("ui.3d_mouse_calibrate"), ui_kit::Button::Normal)
                    .on_hover_text(app.tr("ui.3d_mouse_calibrate_tip"))
                    .on_disabled_hover_text(app.tr(if app.space_mouse.device.is_none() {
                        "ui.3d_mouse_calibrate_wait"
                    } else {
                        "ui.3d_mouse_calibrate_tip"
                    }))
                    .clicked()
                {
                    crate::space_mouse_calibration::start(app);
                }
                if saved
                    && ui_kit::button(ui, app.tr("ui.3d_mouse_forget"), ui_kit::Button::Normal).clicked()
                    && let Some(device) = &device
                {
                    app.preferences.space_mouse_calibrations.remove(device);
                }
            });
            let not_allowed = found_but_not_allowed(app.space_mouse.drivers);
            // With a device, the badge beside the toggle above says this.
            if device.is_some() {
                return;
            }
            let status = if not_allowed {
                app.tr("ui.3d_mouse_denied_short")
            } else if waiting_for_a_record(app.space_mouse.drivers) {
                app.tr("ui.3d_mouse_calibrate_wait")
            } else {
                app.tr("ui.3d_mouse_none")
            };
            let status_line = ui_kit::text(ui, status, ui_kit::Text::Report("calibration_status", false));
            // Nothing confirmed yet (none found, or waiting for the first record): a hint to try, as a
            // tooltip here and, while a reader is waiting for the first record, as a line below.
            if device.is_none() && !not_allowed {
                status_line.on_hover_text(app.tr("ui.3d_mouse_wiggle"));
            }
        });
        if app.space_mouse.device.is_none() {
            if found_but_not_allowed(app.space_mouse.drivers) {
                // Wiggling will not help: the device is there and SolidCraft may not read it.
                hint_under_controls(ui, app.tr("ui.3d_mouse_denied"), "space_mouse_denied");
            } else {
                hint_under_controls(ui, app.tr("ui.3d_mouse_wiggle"), "space_mouse_wiggle");
            }
        }
    });
    // A device whose own app drives the view instead (the navigation bridge).
    ui_kit::divider(ui, "nav_bridge");
    navigation::nav_bridge(app, ui);
    // The rarely needed settings sit behind one disclosure, closed until the reader opens it.
    ui_kit::section(ui, "space_mouse_advanced", crate::i18n::tr_label("Advanced"), false, |ui| {
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            // Only where the reader has seen the choice: Linux, with spacenavd and the device both there.
            if let Some(present) = app.space_mouse.drivers.filter(|present| present.spacenavd || (present.direct && !present.denied)) {
                let [label, auto, direct, only_spacenavd, only_direct] =
                    ["driver", "driver_auto", "driver_direct", "driver_only_spacenavd", "driver_only_direct"]
                        .map(|key| app.tr(&format!("ui.3d_mouse_{key}")).to_owned());
                let driver = &mut app.preferences.space_mouse_driver;
                ui_kit::row(ui, &label, |ui| match (present.spacenavd, present.direct) {
                    (true, true) => {
                        let name = |driver| match driver {
                            SpaceMouseDriver::Automatic => auto.as_str(),
                            SpaceMouseDriver::Spacenavd => "spacenavd",
                            SpaceMouseDriver::Direct => direct.as_str(),
                        };
                        ui_kit::settings_control_width(ui, AXIS_DRIVEN_WIDTH, |ui| {
                            ui_kit::dropdown_menu(ui, "sc_space_mouse_driver", name(*driver), |ui| {
                                for choice in SpaceMouseDriver::ALL {
                                    if ui_kit::list_picker(ui, name(choice), name(choice), *driver == choice).clicked() {
                                        *driver = choice;
                                    }
                                }
                            });
                        });
                    }
                    (true, false) => {
                        ui_kit::text(ui, &only_spacenavd, ui_kit::Text::Hint("space_mouse_driver"));
                    }
                    (false, _) => {
                        ui_kit::text(ui, &only_direct, ui_kit::Text::Hint("space_mouse_driver"));
                    }
                });
            }
            // The setting says what was asked for; this says what is read when that is not it: Direct or
            // Automatic with the daemon read because the device itself could not be opened.
            if let Some(present) = app.space_mouse.drivers
                && present.spacenavd
                && present.direct
                && app.preferences.space_mouse_driver != SpaceMouseDriver::Spacenavd
                && app.space_mouse.device.as_deref() == Some("spacenavd")
            {
                hint_under_controls(ui, app.tr("ui.3d_mouse_driver_fallback"), "space_mouse_driver_fallback");
            }
            let enabled = app.preferences.space_mouse_enabled;
            let (label, tip) = (app.tr("ui.3d_mouse_dead_zone").to_owned(), app.tr("ui.3d_mouse_dead_zone_tip").to_owned());
            let device = app.space_mouse.device.clone();
            if let Some(calibration) = device.and_then(|device| app.preferences.space_mouse_calibrations.get_mut(&device)) {
                ui_kit::row(ui, &label, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        ui_kit::slider(
                            ui,
                            &mut calibration.dead_zone_share,
                            0.0..=solidcraft_space_mouse::MAX_DEAD_ZONE_SHARE,
                            ui_kit::Slider { step: Some(0.01), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                        )
                        .on_hover_text(tip);
                    });
                });
            }
        });
        ui_kit::divider(ui, "space_mouse_axes");
        // Rows say what they do to the view, through the mapping navigation uses; the choices
        // are the device's calibrated motions. Neither names a kind of device.
        let mapping = app.space_mouse.mapping;
        let rows: Vec<(String, bool)> = (0..6)
            .map(|row| {
                let label = crate::space_mouse::row_label(mapping, row);
                let text = crate::i18n::Locale::for_language(&app.preferences.language).format(label, &[("n", &(row + 1).to_string())]);
                (text, label == "ui.3d_mouse_view_off")
            })
            .collect();
        let driven_by = app.tr("ui.3d_mouse_driven_by").to_owned();
        let off_tip = app.tr("ui.3d_mouse_view_off_tip").to_owned();
        let motions: Vec<String> = crate::space_mouse::MOTIONS.iter().map(|motion| app.tr(motion).to_owned()).collect();
        // The axes are a table of their own: each row is a live meter of the motion that drives that
        // view axis, then its Invert, Speed and the motion choice.
        let enabled = app.preferences.space_mouse_enabled;
        let raw: [i16; 6] = app.space_mouse.raw.into();
        // Frames come on input, so a reading that moved since the last one gets a frame to show it.
        let seen = ui.id().with("space_mouse_meter_raw");
        let before = ui.data_mut(|data| data.get_temp::<[i16; 6]>(seen));
        ui.data_mut(|data| data.insert_temp(seen, raw));
        if before.is_some_and(|before| before != raw) {
            ui.ctx().request_repaint();
        }
        let calibration = app.space_mouse.device.as_ref().and_then(|device| app.preferences.space_mouse_calibrations.get(device));
        let meters: [_; 6] = std::array::from_fn(|axis| axis_meter(calibration, raw, app.preferences.space_mouse_axis_source, axis));
        ui_kit::settings_axis_header(ui);
        for (axis, (name, off)) in rows.iter().enumerate() {
            ui.push_id(("space_mouse_axis", axis), |ui| {
                let (value, zone) = meters[axis];
                let prefs = &mut app.preferences;
                let (reverse, speed, source) =
                    (prefs.space_mouse_reverse.get_mut(axis), prefs.space_mouse_axis_speed.get_mut(axis), &mut prefs.space_mouse_axis_source);
                ui_kit::settings_axis_row(
                    ui,
                    name,
                    value,
                    zone,
                    // Invert and speed do nothing on a row the driver turns off. The motion choice
                    // stays live, so a motion can still be swapped out of it.
                    |ui| {
                        ui.add_enabled_ui(enabled && !off, |ui| {
                            if let Some(reverse) = reverse {
                                ui_kit::checkbox(ui, crate::i18n::tr_label("Invert"), reverse);
                            }
                        });
                    },
                    |ui| {
                        ui.add_enabled_ui(enabled && !off, |ui| {
                            if let Some(speed) = speed {
                                ui_kit::settings_axis_gain(ui, speed);
                            }
                        });
                    },
                    |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            let current = source.get(axis).copied().unwrap_or(axis);
                            let mut chosen = current;
                            ui_kit::dropdown_menu(ui, ("sc_space_mouse_source", axis), motions.get(current).map_or("", String::as_str), |ui| {
                                for (from, name) in motions.iter().enumerate() {
                                    if ui_kit::list_picker(ui, name, name, chosen == from).clicked() {
                                        chosen = from;
                                    }
                                }
                            })
                            .on_hover_text(if *off { &off_tip } else { &driven_by });
                            if chosen != current {
                                crate::space_mouse::reassign(source, axis, chosen);
                            }
                        });
                    },
                );
            });
        }
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            ui_kit::row(ui, "", |ui| {
                let reassigned = app.preferences.space_mouse_axis_source != crate::space_mouse::SAME_AXES;
                ui.add_enabled_ui(app.preferences.space_mouse_enabled && reassigned, |ui| {
                    if ui_kit::button(ui, app.tr("ui.3d_mouse_reset_axes"), ui_kit::Button::Normal).clicked() {
                        app.preferences.space_mouse_axis_source = crate::space_mouse::SAME_AXES;
                    }
                });
            });
            // Restore this section's settings (after asking); the calibration, the button mapping and the
            // test model are not touched.
            if app.space_mouse.restore_pending {
                // A short question on its own, not in a row (a row has one line's height and cut it off), so
                // that it wraps to its whole height at any width; the buttons are under it.
                hint_under_controls(ui, app.tr("ui.3d_mouse_restore_ask"), "space_mouse_restore_ask");
                ui_kit::row(ui, "", |ui| {
                    if ui_kit::button(ui, app.tr("ui.3d_mouse_restore"), ui_kit::Button::Normal).clicked() {
                        restore_space_mouse_defaults(app);
                        app.space_mouse.restore_pending = false;
                    }
                    if ui_kit::button(ui, app.tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                        app.space_mouse.restore_pending = false;
                    }
                });
            } else {
                ui_kit::row(ui, "", |ui| {
                    if ui_kit::button(ui, app.tr("ui.3d_mouse_restore"), ui_kit::Button::Normal).clicked() {
                        app.space_mouse.restore_pending = true;
                    }
                });
            }
        });
        // The calibrated device moving as it does, with the test model beside it, then its dead zones.
        let (device, raw) = (app.space_mouse.device.clone(), app.space_mouse.raw.into());
        let (boat, motion, reverse) = (&mut app.space_mouse.boat, app.space_mouse.motion, (app.preferences.zoom_reverse, app.preferences.free_orbit));
        if let Some(calibration) = device.as_ref().and_then(|device| app.preferences.space_mouse_calibrations.get_mut(device)) {
            ui_kit::divider(ui, "space_mouse_picture");
            // Under each circle's axes, what the view does with them: the rows' own labels.
            let stories = crate::space_mouse::stories(&app.preferences.language, mapping, app.preferences.space_mouse_axis_source);
            let help = crate::i18n::tr("ui.3d_mouse_circles_help").to_string();
            let pads = |ui: &mut egui::Ui| {
                crate::space_mouse_calibration::dead_zone_pads(ui, calibration, raw, false, &stories);
                ui_kit::text(ui, &help, ui_kit::Text::Hint("space_mouse_circles_help"));
            };
            picture_and_boat(ui, boat, motion, reverse, true, |ui| crate::space_mouse_model::live(ui, calibration, raw), pads);
        } else if device.is_some() {
            // Not calibrated: the test model alone.
            ui_kit::divider(ui, "space_mouse_picture");
            picture_and_boat(ui, boat, motion, reverse, false, |_| {}, |_| {});
        }
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            hint_under_controls(ui, crate::i18n::tr_label("Move and rotate the view together; sketches use pan and zoom only."), "space_mouse");
        });
        ui_kit::divider(ui, "space_mouse_buttons");
        space_mouse_buttons(app, ui);
    });
}

/// A hint in the control column of the forms above and below it, as the form rows' controls are,
/// not at the section's left edge.
fn hint_under_controls(ui: &mut egui::Ui, text: &str, key: &str) {
    let indent = FORM_LABEL_WIDTH.min(ui.available_width() * 0.6) + Tokens::get().space_14;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(indent);
        ui_kit::text(ui, text, ui_kit::Text::Hint(key));
    });
}

/// The width of the "driven by" choice.
const AXIS_DRIVEN_WIDTH: f32 = 140.0;

/// Where the motion that drives view axis `axis` sits, as a share of its full travel (calibrated
/// when the device has a calibration, else the raw reading over full scale), and the share of that
/// travel which reads as no motion on each side, for the live meter in its row.
fn axis_meter(calibration: Option<&solidcraft_space_mouse::Calibration>, raw: [i16; 6], source: [usize; 6], axis: usize) -> (f32, [f32; 2]) {
    let from = source.get(axis).copied().unwrap_or(axis);
    match calibration {
        Some(calibration) => {
            (calibration.travel(raw).get(from).copied().unwrap_or(0.0), calibration.dead_zone_shares().get(from).copied().unwrap_or_default())
        }
        None => (f32::from(raw.get(from).copied().unwrap_or(0)) / f32::from(crate::space_mouse::FULL_SCALE), [0.0; 2]),
    }
}

/// Whether a reader is reachable (spacenavd's socket, or a 3D mouse hidraw lists) that has not yet
/// sent a record: the device is not confirmed until it does, so Preferences asks for a movement
/// instead of saying none was found. Nothing reachable at all is "none found".
fn waiting_for_a_record(drivers: Option<crate::space_mouse::DriversPresent>) -> bool {
    drivers.is_some_and(|present| present.spacenavd || (present.direct && !present.denied))
}

/// A 3D mouse is there but may not be opened, and nothing else is reachable (no spacenavd): asking for
/// a movement would wait forever, so Preferences says what is wrong instead.
fn found_but_not_allowed(drivers: Option<crate::space_mouse::DriversPresent>) -> bool {
    drivers.is_some_and(|present| present.denied && !present.spacenavd)
}

/// Puts this section's settings back: the driver choice, the speed, each axis's invert and speed and
/// assignment, where the picture goes and whether it shows. Not the calibration (it has its own
/// Forget), the button mapping, the offered set, "never show again", "Navigate with the 3D mouse" or
/// the test model.
pub fn restore_space_mouse_defaults(app: &mut SolveApp) {
    let d = Prefs::default();
    let p = &mut app.preferences;
    p.space_mouse_driver = d.space_mouse_driver;
    p.space_mouse_speed = d.space_mouse_speed;
    p.space_mouse_reverse = d.space_mouse_reverse;
    p.space_mouse_axis_speed = d.space_mouse_axis_speed;
    p.space_mouse_axis_source = d.space_mouse_axis_source;
    p.space_mouse_ghost_position = d.space_mouse_ghost_position;
    p.space_mouse_in_view = d.space_mouse_in_view;
}

/// The device's picture with the coordinate circles under it, and a box with a small model that
/// moves with the 3D mouse as the view does and a Reset button: to the right of the picture where
/// there is room, else under the circles. All stay inside the section at any width.
fn picture_and_boat(
    ui: &mut egui::Ui,
    boat: &mut Option<crate::test_boat::Boat>,
    motion: Option<crate::space_mouse::DeviceMotion>,
    (reverse_zoom, free_orbit): (bool, bool),
    has_picture: bool,
    picture: impl FnOnce(&mut egui::Ui),
    pads: impl FnOnce(&mut egui::Ui),
) {
    const PICTURE_WIDTH: f32 = 200.0;
    const BOAT_HEIGHT: f32 = 150.0;
    const BOAT_MIN: f32 = 120.0;
    const BOAT_MAX: f32 = 260.0;
    let gap = Tokens::get().space_12;
    let room = ui.available_width();
    let beside = has_picture && ui_kit::fits_steadily(ui, "boat_beside", PICTURE_WIDTH + gap + BOAT_MIN, room);
    let width = if beside { (room - PICTURE_WIDTH - gap).min(BOAT_MAX) } else { room.min(BOAT_MAX) };
    let mut boat_and_reset = |ui: &mut egui::Ui| {
        ui.vertical(|ui| {
            crate::test_boat::show(ui, boat, motion, reverse_zoom, free_orbit, egui::vec2(width.max(0.0), BOAT_HEIGHT));
            if ui_kit::button(ui, crate::i18n::tr("ui.3d_mouse_reset_view"), ui_kit::Button::Normal).clicked() {
                crate::test_boat::reset(boat);
            }
        });
    };
    // The coordinate circles stay under the device's picture, from its left edge; the test model is
    // beside the picture where there is room, else under the circles.
    if beside {
        ui.horizontal(|ui| {
            picture(ui);
            boat_and_reset(ui);
        });
        pads(ui);
    } else {
        picture(ui);
        pads(ui);
        boat_and_reset(ui);
    }
}

/// The 3D mouse's buttons, each with what it does: those that do something and any pressed since
/// the app started. A pressed one is marked in its row; Preferences doesn't run them.
fn space_mouse_buttons(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(device) = app.space_mouse.device.clone() else {
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            hint_under_controls(ui, crate::i18n::tr("ui.3d_mouse_buttons_no_device"), "space_mouse_buttons");
        });
        return;
    };
    let mut jobs = crate::space_mouse::button_jobs(app, &device);
    let bit = |number: u8| number.checked_sub(1).and_then(|index| 1_u32.checked_shl(u32::from(index))).unwrap_or(0);
    let mut numbers: std::collections::BTreeSet<u8> = jobs.keys().copied().collect();
    numbers.extend((1..=32).filter(|&number| app.space_mouse.seen & bit(number) != 0));
    let mut changed = false;
    ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
        hint_under_controls(ui, crate::i18n::tr("ui.3d_mouse_buttons_hint"), "space_mouse_buttons");
        for number in numbers {
            let mut name = crate::i18n::format("ui.3d_mouse_button", &[("number", &number.to_string())]);
            if app.space_mouse.buttons & bit(number) != 0 {
                name.push_str("  ●");
            }
            let current = jobs.get(&number).cloned();
            if let Some(choice) = ui_kit::row(ui, &name, |ui| button_job_picker(app, ui, number, current.as_ref())) {
                changed = true;
                match choice {
                    Some(job) => jobs.insert(number, job),
                    None => jobs.remove(&number),
                };
            }
        }
    });
    if changed {
        app.preferences.space_mouse_buttons.insert(device.clone(), jobs);
    }
    if app.preferences.space_mouse_buttons.contains_key(&device) {
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            ui_kit::row(ui, "", |ui| {
                if ui_kit::button(ui, crate::i18n::tr("ui.3d_mouse_buttons_reset"), ui_kit::Button::Normal).clicked() {
                    app.preferences.space_mouse_buttons.remove(&device);
                }
            });
        });
    }
}

/// What a button job is called in its picker.
fn button_job_label(app: &SolveApp, job: Option<&crate::space_mouse::ButtonJob>) -> String {
    use crate::space_mouse::{ButtonJob, ButtonKey};
    match job {
        None => crate::i18n::tr("ui.3d_mouse_button_nothing").to_string(),
        Some(ButtonJob::Settings) => crate::i18n::tr("ui.3d_mouse_button_settings").to_string(),
        Some(ButtonJob::View(view)) => {
            let mut name = view.clone();
            if let Some(first) = name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            app.tr_label(&name).to_string()
        }
        Some(ButtonJob::Key(key)) => match key {
            ButtonKey::Esc => "Esc",
            ButtonKey::Shift => "Shift",
            ButtonKey::Ctrl => "Ctrl",
            ButtonKey::Alt => "Alt",
        }
        .to_string(),
        Some(ButtonJob::Command(id)) => solidcraft_engine::find_command(id).map_or_else(|| id.clone(), |c| app.tr_label(c.label).to_string()),
    }
}

/// A job as the picker's value: what it is and, for a view, key or command, which.
fn button_job_value(job: Option<&crate::space_mouse::ButtonJob>) -> String {
    use crate::space_mouse::ButtonJob;
    match job {
        None => "none".into(),
        Some(ButtonJob::Settings) => "settings".into(),
        Some(ButtonJob::View(view)) => format!("view:{view}"),
        Some(ButtonJob::Key(key)) => format!("key:{key:?}"),
        Some(ButtonJob::Command(id)) => format!("cmd:{id}"),
    }
}

/// A button's job, chosen from nothing, the 3D mouse settings, the views, the keys, then any
/// command, in the kit's searchable dropdown. `Some` with the new job when one is picked.
fn button_job_picker(
    app: &SolveApp,
    ui: &mut egui::Ui,
    number: u8,
    current: Option<&crate::space_mouse::ButtonJob>,
) -> Option<Option<crate::space_mouse::ButtonJob>> {
    use crate::space_mouse::{ButtonJob, ButtonKey};
    let views = ["home", "fit", "top", "front", "right", "left", "back", "bottom"].map(|view| ButtonJob::View(view.into()));
    let keys = [ButtonKey::Esc, ButtonKey::Shift, ButtonKey::Ctrl, ButtonKey::Alt].map(ButtonJob::Key);
    let commands = solidcraft_engine::command_specs().into_iter().map(|spec| ButtonJob::Command(spec.id.into()));
    let jobs: Vec<Option<ButtonJob>> =
        [None, Some(ButtonJob::Settings)].into_iter().chain(views.into_iter().chain(keys).chain(commands).map(Some)).collect();
    let entries: Vec<(String, String)> = jobs.iter().map(|job| (button_job_value(job.as_ref()), button_job_label(app, job.as_ref()))).collect();
    let options: Vec<_> = entries.iter().map(|(value, label)| ui_kit::SearchOption { value, label, handle: None }).collect();
    let filter_id = egui::Id::new(("sc_space_mouse_button_filter", number));
    let mut filter: String = ui.data_mut(|data| data.get_temp(filter_id).unwrap_or_default());
    let id = format!("sc_space_mouse_button_{number}");
    let chosen = ui_kit::searchable_dropdown(ui, &id, &button_job_label(app, current), &mut filter, &options, None, None);
    ui.data_mut(|data| data.insert_temp(filter_id, filter));
    let chosen = chosen?;
    jobs.into_iter().find(|job| button_job_value(job.as_ref()) == chosen)
}

/// The control channel's `ui.prefs`: `open` (a page name) opens the dialog there, `button`
/// presses OK, Cancel, Apply or Restore Defaults, `stand_up: true` answers the stand-up offer
/// (`false` leaves the file as it is, like "Leave it"), `theme_import` (a theme file's JSON or
/// text) adds a theme and turns it on, `theme_remove` (an id) removes an imported one and
/// `theme_export: true` returns the theme on now as `theme_file`, `theme_follow` (a path, or
/// `""` to stop) follows a theme file on the desktop (#7573); any other keys set preferences.
/// Returns them all, the open page, the offer, the theme ids and the followed file
/// (`theme_followed`, `theme_follow_error`).
pub fn control(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut rest = p.as_object().cloned().unwrap_or_default();
    if let Some(page) = rest.remove("open") {
        let name = page.as_str().unwrap_or("General");
        let i = page_index(name).ok_or_else(|| format!("no page `{name}` ({})", PAGES.join(", ")))?;
        open_at(app, i);
        if app.prefs_window.snapshot.is_none() {
            app.prefs_window.snapshot = Some(Snapshot::take(app));
        }
    }
    match rest.remove("stand_up").and_then(|v| v.as_bool()) {
        Some(true) => crate::stand_up::stand_up(app)?,
        Some(false) => crate::stand_up::leave(app),
        None => {}
    }
    let imported = match rest.remove("theme_import") {
        Some(v) => Some(import_theme(app, &v)?),
        None => None,
    };
    if let Some(id) = rest.remove("theme_remove") {
        remove_theme(app, id.as_str().unwrap_or_default())?;
    }
    match rest.remove("theme_follow") {
        Some(v) => set_theme_follow(app, &v, FOLLOW_AVAILABLE)?,
        // Choosing a theme stops following a file, as clicking a theme tile does.
        None if rest.contains_key("theme") => stop_following(app),
        None => {}
    }
    let export = rest.remove("theme_export").and_then(|v| v.as_bool()).unwrap_or(false);
    let button = rest.remove("button");
    let mut out = set(app, &serde_json::Value::Object(rest))?;
    match button.as_ref().and_then(|b| b.as_str()) {
        Some("ok") => close(app, false),
        Some("cancel") => close(app, true),
        Some("apply") => app.prefs_window.snapshot = Some(Snapshot::take(app)),
        Some("defaults") => defaults(app, app.prefs_window.section),
        Some(b) => return Err(format!("unknown button `{b}` (ok, cancel, apply, defaults)")),
        None => {}
    }
    if button.is_some() {
        out = serde_json::to_value(&app.preferences).map_err(|e| e.to_string())?;
    }
    if let Some(o) = out.as_object_mut() {
        o.insert("dialog".into(), json!(app.prefs_window.open.then(|| PAGES.get(app.prefs_window.section).copied().unwrap_or_default())));
        o.insert("stand_up_offer".into(), json!(app.stand_up));
        o.insert("dark".into(), json!(app.ui.dark));
        o.insert("viewport_dark".into(), json!(app.viewport_dark()));
        o.insert("theme_ids".into(), json!(theme_choices(app).into_iter().map(|(id, _)| id).collect::<Vec<_>>()));
        let follow = &app.prefs_window.follow;
        let followed = follow.path(&app.preferences.theme_follow);
        o.insert("theme_followed".into(), json!((!followed.is_empty()).then_some(followed)));
        o.insert("theme_follow_error".into(), json!(follow.error().filter(|_| !followed.is_empty())));
        if let Some(id) = imported {
            o.insert("theme_imported".into(), json!(id));
        }
        if export {
            o.insert("theme_file".into(), current_theme_file(app));
        }
    }
    Ok(out)
}

/// The saved 3D mouse calibrations that still read. One this version can't read (a hand edit, a
/// format from another release) is dropped on its own, with a log line, not with all the others.
fn calibrations_that_read<'de, D: serde::Deserializer<'de>>(
    saved: D,
) -> Result<std::collections::BTreeMap<String, solidcraft_space_mouse::Calibration>, D::Error> {
    let saved = std::collections::BTreeMap::<String, serde_json::Value>::deserialize(saved)?;
    Ok(saved
        .into_iter()
        .filter_map(|(device, calibration)| match serde_json::from_value(calibration) {
            Ok(calibration) => Some((device, calibration)),
            Err(error) => {
                log::warn!("preferences: the 3D mouse calibration for `{device}` can't be read ({error}); reading the device as it reports");
                None
            }
        })
        .collect())
}

/// The saved set of devices that read as text; anything else in it is dropped, not the whole set.
fn devices_that_read<'de, D: serde::Deserializer<'de>>(saved: D) -> Result<std::collections::BTreeSet<String>, D::Error> {
    let saved = Vec::<serde_json::Value>::deserialize(saved)?;
    Ok(saved.into_iter().filter_map(|device| device.as_str().map(str::to_owned)).collect())
}

/// The saved button jobs that still read. A job this version doesn't know (from a newer release)
/// drops that button only, with a log line, not its device's other buttons or the other devices.
fn button_jobs_that_read<'de, D: serde::Deserializer<'de>>(
    saved: D,
) -> Result<std::collections::BTreeMap<String, std::collections::BTreeMap<u8, crate::space_mouse::ButtonJob>>, D::Error> {
    let saved = std::collections::BTreeMap::<String, serde_json::Value>::deserialize(saved)?;
    Ok(saved
        .into_iter()
        .filter_map(|(device, jobs)| {
            let jobs = match jobs {
                serde_json::Value::Object(jobs) => jobs,
                _ => {
                    log::warn!("preferences: the 3D mouse buttons for `{device}` can't be read; using the defaults");
                    return None;
                }
            };
            let jobs = jobs
                .into_iter()
                .filter_map(|(number, job)| {
                    let read = number.parse::<u8>().ok().and_then(|number| Some((number, serde_json::from_value(job).ok()?)));
                    if read.is_none() {
                        log::warn!("preferences: button {number} of the 3D mouse `{device}` can't be read; leaving it empty");
                    }
                    read
                })
                .collect();
            Some((device, jobs))
        })
        .collect())
}

/// Every theme the Theme list offers, as (id, name): the modes, the built-in theme files, then
/// the imported ones.
fn theme_choices(app: &SolveApp) -> Vec<(String, String)> {
    let modes = THEMES.iter().map(|(id, label)| ((*id).to_string(), crate::i18n::tr_label(label).to_string()));
    let files = crate::theme_files::builtin().iter().chain(&app.preferences.custom_themes).map(|t| (t.id.clone(), t.name.clone()));
    modes.chain(files).collect()
}

/// Turn a theme on (a mode or a theme file's id); dark follows it.
fn select_theme(app: &mut SolveApp, id: &str) {
    app.preferences.theme = id.to_string();
    match crate::theme_files::find(id, &app.preferences.custom_themes) {
        Some(f) => app.ui.dark = f.base.is_dark(),
        None if id != "system" => app.ui.dark = id == "dark",
        None => {}
    }
}

/// Import a theme file (its text, or its JSON) into the preferences and turn it on; returns
/// the id it is kept under.
pub fn import_theme(app: &mut SolveApp, v: &serde_json::Value) -> Result<String, String> {
    let theme = match v {
        serde_json::Value::String(text) => crate::theme_files::parse(text)?,
        other => crate::theme_files::from_value(other)?,
    };
    let id = crate::theme_files::add(&mut app.preferences.custom_themes, theme)?;
    select_theme(app, &id);
    Ok(id)
}

/// Remove an imported theme; when it is on, its base (dark or light) takes over.
fn remove_theme(app: &mut SolveApp, id: &str) -> Result<(), String> {
    let Some(i) = app.preferences.custom_themes.iter().position(|t| t.id == id) else {
        return Err(format!("no imported theme `{id}`"));
    };
    let gone = app.preferences.custom_themes.remove(i);
    if app.preferences.theme == id {
        select_theme(app, if gone.base.is_dark() { "dark" } else { "light" });
    }
    Ok(())
}

/// The theme on now as a complete theme file, a starting point for a new one.
fn current_theme_file(app: &SolveApp) -> serde_json::Value {
    theme_file(app, &app.preferences.theme, app.ui.dark)
}

/// Export the tile the user chose, without applying it or changing the dialog snapshot.
fn theme_file(app: &SolveApp, id: &str, system_dark: bool) -> serde_json::Value {
    use crate::theme_files::{Base, export, find, resolve};
    match find(id, &app.preferences.custom_themes) {
        Some(f) => export(&f.name, f.base, &resolve(f)),
        None if id == "dark" || (id == "system" && system_dark) => export("Dark", Base::Dark, &crate::theme::Tokens::dark()),
        None => export("Light", Base::Light, &crate::theme::Tokens::light()),
    }
}

/// Fold a setting an older release kept in its own field into the one this version reads.
/// `orbit_cursor` (`true` = the point under the cursor) became `orbit_around`. A 3D mouse
/// calibration that can't be used as it is (a hand edit) is dropped, so the device is read as it
/// reports rather than spinning the view.
pub fn migrate(mut p: Prefs) -> Prefs {
    // Every saved file carries the old default limit; it reads as no limit.
    if p.agent_chat_turn_limit == crate::agent_chat::gate::OLD_DEFAULT_TURN_LIMIT {
        p.agent_chat_turn_limit = 0;
    }
    if p.orbit_cursor {
        p.orbit_around = OrbitAround::Cursor;
        p.orbit_cursor = false;
    }
    if !crate::space_mouse::drives_each_once(p.space_mouse_axis_source) {
        log::warn!("preferences: the 3D mouse axis assignment {:?} doesn't drive each axis once; using the default", p.space_mouse_axis_source);
        p.space_mouse_axis_source = crate::space_mouse::SAME_AXES;
    }
    p.space_mouse_calibrations.retain(|device, calibration| {
        let usable = calibration.is_usable();
        if !usable {
            log::warn!("preferences: the 3D mouse calibration for `{device}` can't be used as it is; reading the device as it reports");
        }
        usable
    });
    p
}

/// Drop plugin ids this page would not show, and keep at most [`crate::plugins::MAX_DISABLED`],
/// so a hand-edited preferences file cannot carry a huge or nonsense list. `set` still refuses
/// those instead of dropping them.
fn limit_disabled_plugins(mut p: Prefs) -> Prefs {
    p.disabled_plugins.retain(|id| crate::plugins::valid_id(id));
    if p.disabled_plugins.len() > crate::plugins::MAX_DISABLED {
        p.disabled_plugins = p.disabled_plugins.into_iter().take(crate::plugins::MAX_DISABLED).collect();
    }
    p
}

/// Saved preferences, keeping every one that still reads. A value this version doesn't
/// understand (a setting a newer or older release saved differently, a hand edit) falls back to
/// its default on its own instead of discarding all of them.
pub fn lenient(saved: &serde_json::Value) -> Prefs {
    if let Ok(p) = serde_json::from_value::<Prefs>(saved.clone()) {
        return limit_disabled_plugins(migrate(p));
    }
    let Ok(mut cur) = serde_json::to_value(Prefs::default()) else { return Prefs::default() };
    if let (Some(o), Some(src)) = (cur.as_object_mut(), saved.as_object()) {
        for (k, v) in src.iter().take(1000) {
            // Keys this version doesn't have are ignored anyway.
            let Some(old) = o.get(k).cloned() else { continue };
            o.insert(k.clone(), v.clone());
            if serde_json::from_value::<Prefs>(serde_json::Value::Object(o.clone())).is_err() {
                log::warn!("preferences: `{k}` is not understood by this version; using its default");
                o.insert(k.clone(), old);
            }
        }
    }
    serde_json::from_value(cur).map(migrate).map(limit_disabled_plugins).unwrap_or_default()
}

/// Set preferences from JSON (control channel); returns them all.
pub fn set(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut cur = serde_json::to_value(&app.preferences).map_err(|e| e.to_string())?;
    if let (Some(o), Some(src)) = (cur.as_object_mut(), p.as_object()) {
        for (k, v) in src {
            if !o.contains_key(k) {
                return Err(format!("unknown preference `{k}`"));
            }
            o.insert(k.clone(), v.clone());
        }
    }
    // Saved preferences read an unknown mode as the default; asked for here, it is an error.
    if let Some(v) = p.get("color_vision")
        && v.as_str().and_then(crate::theme::ColorVision::from_id).is_none()
    {
        return Err("`color_vision` must be off, protan, deutan or tritan".into());
    }
    if let Some(v) = p.get("viewport_theme")
        && v.as_str().and_then(crate::theme::ViewportTheme::from_id).is_none()
    {
        return Err("`viewport_theme` must be theme, light or dark".into());
    }
    let new: Prefs = migrate(serde_json::from_value(cur).map_err(|e| e.to_string())?);
    if !UNITS.contains(&new.default_units.as_str()) {
        return Err(format!("units must be one of {}", UNITS.join(", ")));
    }
    if !WINDOW_SYSTEMS.contains(&new.window_system.as_str()) {
        return Err(format!("`window_system` must be one of {}", WINDOW_SYSTEMS.join(", ")));
    }
    if !NAV_PRESETS.iter().any(|p| p.id == new.nav) || !MSAA.contains(&new.msaa) {
        return Err(format!("`nav` must be one of {} and `msaa` 1, 2, 4 or 8", NAV_PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")));
    }
    if new.msaa != app.preferences.msaa && !app.msaa_supported.is_empty() && !app.msaa_supported.contains(&new.msaa) {
        return Err(format!("this graphics device doesn't support `msaa` {}; it supports {:?}", new.msaa, app.msaa_supported));
    }
    if new.graphics_adapter.len() > 1024 || new.graphics_adapter.chars().any(char::is_control) {
        return Err("`graphics_adapter` must be a name of at most 1024 bytes without control characters".into());
    }
    let file = crate::theme_files::find(&new.theme, &new.custom_themes);
    if (!THEMES.iter().any(|(id, _)| *id == new.theme) && file.is_none()) || !["z", "y"].contains(&new.up_axis.as_str()) {
        return Err("`theme` must be dark, light, system or a theme's id (see `theme_ids`) and `up_axis` z or y".into());
    }
    if new.custom_themes.len() > crate::theme_files::MAX_CUSTOM || new.custom_themes.iter().any(|t| t.colors.len() > crate::theme_files::MAX_COLORS) {
        return Err(format!("at most {} imported themes of {} colours each", crate::theme_files::MAX_CUSTOM, crate::theme_files::MAX_COLORS));
    }
    for theme in &new.custom_themes {
        theme.validate_radii()?;
        crate::theme_files::parse_icon_palette(&theme.icon_palette)?;
        crate::theme_files::parse_borders(&theme.borders)?;
    }
    if new.theme_follow != app.preferences.theme_follow {
        if !FOLLOW_AVAILABLE && !new.theme_follow.trim().is_empty() {
            return Err(FOLLOW_UNAVAILABLE.into());
        }
        crate::theme_files::follow_path(&new.theme_follow)?;
    }
    if !crate::i18n::is_supported(&new.language) {
        return Err(format!("`language` must be one of: {}", crate::i18n::LOCALES.iter().map(|locale| locale.id).collect::<Vec<_>>().join(", ")));
    }
    if new.ui_font != app.preferences.ui_font && !new.ui_font.trim().is_empty() && crate::fonts::ui_font(&new.ui_font).is_none() {
        return Err(format!("`ui_font`: `{}` is not an installed font family or a readable font file", new.ui_font.trim()));
    }
    if new.disabled_plugins.len() > crate::plugins::MAX_DISABLED || new.disabled_plugins.iter().any(|id| !crate::plugins::valid_id(id)) {
        return Err(format!("`disabled_plugins` must be at most {} ids of lowercase letters, digits and hyphens", crate::plugins::MAX_DISABLED));
    }
    if !accepted::ui_scale(&new) || !accepted::look_at_speed(&new) {
        return Err("`ui_scale` must be 0.5…2.5 and `look_at_speed` 0.25…4".into());
    }
    if solidcraft_engine::doc::display_quality::DisplayQuality::parse(&new.display_quality).is_none() {
        return Err("`display_quality` must be coarse, normal, fine or ultra".into());
    }
    if !SUPERSAMPLING.contains(&new.supersampling) {
        return Err("`supersampling` must be 1 (off), 1.5 or 2".into());
    }
    if MiddleDoubleClick::parse(&new.middle_double_click).is_none() {
        return Err("`middle_double_click` must be home, fit or off".into());
    }
    if new.theme != app.preferences.theme && new.theme != "system" {
        app.ui.dark = file.map_or(new.theme == "dark", |f| f.base.is_dark());
    }
    let units_changed = new.default_units != app.preferences.default_units;
    let up_changed = new.up_axis != app.preferences.up_axis;
    let graphics_changed = new.msaa != app.preferences.msaa
        || new.window_system != app.preferences.window_system
        || new.graphics_adapter != app.preferences.graphics_adapter;
    if p.get("layout").is_some() {
        app.ui.layout = new.layout;
    }
    app.preferences = new;
    if units_changed {
        default_units_changed(app);
    }
    if graphics_changed {
        save_graphics(app);
    }
    if up_changed {
        up_axis_changed(app);
    }
    serde_json::to_value(&app.preferences).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) fn reveal_preference(h: &mut crate::scenario::Harness, handle: &str) {
    for _ in 0..16 {
        let page = crate::scenario::rect_of("dialog:page_body").expect("preference body");
        let visible = crate::scenario::rect_of(handle)
            .map(|r| page.contains_rect(r))
            .unwrap_or_else(|| crate::scenario::handle_at(handle).is_some_and(|p| page.shrink(crate::theme::Tokens::get().space_16).contains(p)));
        if visible {
            return;
        }
        h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-100}));
        h.frames(20);
    }
    panic!("preference {handle} was not revealed");
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_ghost_position_defaults_round_trips_and_reads_tolerantly() {
        use super::SpaceMouseGhost::{LeftViewport as Current, LeftWindow as Left, RightViewport as Right};
        assert_eq!(serde_json::from_value::<super::Prefs>(serde_json::json!({})).unwrap().space_mouse_ghost_position, Current);
        for position in [Current, Left, Right] {
            let prefs = super::Prefs { space_mouse_ghost_position: position, ..Default::default() };
            let again: super::Prefs = serde_json::from_value(serde_json::to_value(&prefs).unwrap()).unwrap();
            assert_eq!(again.space_mouse_ghost_position, position);
        }
        // A value from a newer build costs that setting only.
        for (old, position) in [("current", Current), ("left", Left), ("right", Right)] {
            let read = serde_json::from_value::<super::Prefs>(serde_json::json!({ "space_mouse_ghost_position": old })).unwrap();
            assert_eq!(read.space_mouse_ghost_position, position, "{old}");
        }
        assert_eq!(serde_json::to_value(Left).unwrap(), "left_window");
        let read = super::lenient(&serde_json::json!({ "space_mouse_ghost_position": "top", "space_mouse_speed": 2.0 }));
        assert_eq!((read.space_mouse_ghost_position, read.space_mouse_speed), (Current, 2.0));
    }

    #[test]
    fn the_calibration_offer_record_round_trips_and_reads_tolerantly() {
        let prefs: super::Prefs =
            serde_json::from_value(serde_json::json!({ "space_mouse_offered": ["spacenavd", 7, null, "hid 046d:c62b"] })).unwrap();
        assert_eq!(prefs.space_mouse_offered, ["spacenavd".to_string(), "hid 046d:c62b".to_string()].into());
        let again: super::Prefs = serde_json::from_value(serde_json::to_value(&prefs).unwrap()).unwrap();
        assert_eq!(again.space_mouse_offered, prefs.space_mouse_offered);
        // Files from before it existed read as nothing offered yet.
        assert!(serde_json::from_value::<super::Prefs>(serde_json::json!({})).unwrap().space_mouse_offered.is_empty());
    }

    /// The 3D mouse section is laid out inside the panel it is in, at every panel width: nothing it
    /// draws reaches past the right edge (the rows used to run off it and get clipped).
    #[test]
    fn the_3d_mouse_section_stays_inside_the_panel_at_every_width() {
        for width in [420.0_f32, 560.0, 700.0, 900.0, 1200.0] {
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.space_mouse.device = Some("hid 046d:c62b".into());
            app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: true, denied: false });
            app.preferences.space_mouse_in_view = true;
            // Calibrated, so the device's picture and the test model sit side by side where there is room.
            let axes = std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration {
                source,
                reversed: false,
                rest: 0,
                dead_zone: 0,
                full: [350; 2],
            });
            app.preferences.space_mouse_calibrations.insert(
                "hid 046d:c62b".into(),
                solidcraft_space_mouse::Calibration { dead_zone_share: 0.0, kind: solidcraft_space_mouse::Kind::ReturnToCenter, axes },
            );
            let ctx = crate::fonts::test_context();
            let panel = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(width, 1400.0));
            for frame in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width + 40.0, 1500.0))),
                    focused: true,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                        super::space_mouse_section(&mut app, ui);
                        let wide: Vec<_> = crate::scenario::rects("").into_iter().filter(|(_, r)| r.right() > panel.right() + 0.5).collect();
                        assert!(
                            ui.min_rect().right() <= panel.right() + 0.5,
                            "width {width}, frame {frame}: the section reaches {}; past the panel: {wide:?}",
                            ui.min_rect().right()
                        );
                    });
                });
                output.textures_delta.clear();
            }
        }
    }

    /// The calibration window keeps its texts and sliders inside itself at the widths it can be
    /// given, for both kinds of device (the notice card is clamped to the free width by construction).
    #[test]
    fn the_calibration_window_keeps_its_contents_inside() {
        for width in [360.0_f32, 520.0, 900.0] {
            for kind in [solidcraft_space_mouse::Kind::ReturnToCenter, solidcraft_space_mouse::Kind::Trackball] {
                let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
                app.space_mouse.device = Some("hid 046d:c62b".into());
                crate::space_mouse_calibration::start(&mut app);
                app.space_mouse.calibrating.as_mut().unwrap().1.choose(kind);
                let ctx = crate::fonts::test_context();
                for _ in 0..6 {
                    crate::scenario::clear_handles();
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0))),
                        focused: true,
                        ..Default::default()
                    };
                    let mut output = ctx.run_ui(input, |ui| crate::space_mouse_calibration::show(&mut app, ui.ctx()));
                    output.textures_delta.clear();
                }
                let window = crate::scenario::rect_of("dialog:window").expect("the window is drawn");
                for (name, rect) in crate::scenario::rects("dialog:").into_iter().filter(|(name, _)| {
                    name.starts_with("dialog:slider")
                        || name.starts_with("dialog:widget")
                        || name.starts_with("dialog:note")
                        || name.starts_with("dialog:report")
                }) {
                    assert!(
                        rect.right() <= window.right() + 0.5 && rect.left() >= window.left() - 0.5,
                        "width {width} {kind:?}: {name} {rect:?} is outside {window:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn preferred_units_update_only_a_blank_unsaved_design() {
        for design in ["new", "saved", "content"] {
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            match design {
                "saved" => app.session.path = Some("empty.solidcraft".into()),
                "content" => {
                    app.session.execute("sketch.create", &serde_json::json!({"plane": "XY"})).unwrap();
                    app.session.mark_saved();
                }
                _ => {}
            }
            let dirty = app.session.is_dirty();
            let undo = app.session.undo.len();
            super::set(&mut app, &serde_json::json!({"default_units": "in"})).unwrap();
            assert_eq!(app.preferences.default_units, "in");
            assert_eq!(app.session.doc.units, if design == "new" { "in" } else { "mm" }, "{design}");
            assert_eq!(app.session.is_dirty(), dirty, "{design}");
            assert_eq!(app.session.undo.len(), undo, "{design}");
            if design == "new" {
                super::control(&mut app, &serde_json::json!({"open": "Design"})).unwrap();
                super::set(&mut app, &serde_json::json!({"default_units": "ft"})).unwrap();
                assert_eq!(app.session.doc.units, "ft");
                super::control(&mut app, &serde_json::json!({"button": "cancel"})).unwrap();
                assert_eq!(app.session.doc.units, "in");
                super::defaults(&mut app, super::PAGE_DESIGN);
                assert_eq!(app.session.doc.units, "mm");
                assert!(!app.session.is_dirty());
                assert!(app.session.undo.is_empty());
            }
        }
    }

    /// What Preferences says of the 3D mouse before one is confirmed: a reachable reader that has sent
    /// nothing yet asks for a movement; only nothing reachable says none was found.
    #[test]
    fn the_device_line_asks_for_a_movement_when_a_reader_is_reachable_and_only_then_says_none_found() {
        use crate::space_mouse::DriversPresent;
        let none = DriversPresent { spacenavd: false, direct: false, denied: false };
        assert!(!waiting_for_a_record(None) && !waiting_for_a_record(Some(none)));
        assert!(waiting_for_a_record(Some(DriversPresent { spacenavd: true, direct: false, denied: false })), "a daemon, no record yet");
        assert!(waiting_for_a_record(Some(DriversPresent { spacenavd: false, direct: true, denied: false })), "a readable device, no record yet");
        assert!(waiting_for_a_record(Some(DriversPresent { spacenavd: true, direct: true, denied: false })));
        // The texts differ, and the first one is the Calibrate hint.
        assert_ne!(crate::i18n::tr("ui.3d_mouse_none"), crate::i18n::tr("ui.3d_mouse_calibrate_wait"));
        // A confirmed device is neither (the status names it).
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = Some("spacenavd".into());
        h.app.space_mouse.drivers = Some(DriversPresent { spacenavd: true, direct: false, denied: false });
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
        h.frames(3);
        assert!(h.app.prefs_window.open);
    }

    /// Calibration starts while Preferences is open, and its window is on top of it; with no device
    /// known there is nothing to start (the button is off and says why, see the hint key).
    #[test]
    fn calibration_opens_over_an_open_preferences_and_waits_for_a_device() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        // After a driver change and before any reading: no device, so nothing starts.
        h.app.space_mouse.device = None;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
        h.frames(5);
        assert!(crate::i18n::tr("ui.3d_mouse_calibrate_wait").starts_with("Move the 3D mouse"));
        crate::space_mouse_calibration::start(&mut h.app);
        h.frames(3);
        assert!(h.app.space_mouse.calibrating.is_none() && h.app.prefs_window.open);
        // The device is seen: the window opens, above Preferences, which stays open.
        h.app.space_mouse.device = Some("hid 046d:c62b".into());
        h.frames(3);
        crate::space_mouse_calibration::start(&mut h.app);
        h.frames(5);
        assert!(h.app.space_mouse.calibrating.is_some() && h.app.prefs_window.open);
        let window = crate::scenario::rect_of("dialog:window").expect("the calibration window is the last window drawn");
        let top = h.ctx.layer_id_at(window.center());
        assert!(top.is_some_and(|layer| layer.id == egui::Id::new("sc_space_mouse_calibration")), "top is {top:?}");
    }

    /// Scrolls the open page until the handle is well inside the page body (not at its clipped edge), then clicks it (the harness has
    /// no scroll step, so the wheel is sent the way a mouse would).
    fn scroll_to_and_click(h: &mut crate::scenario::Harness, handle: &str) {
        for step in 0..60 {
            let body = crate::scenario::rect_of("prefs:page_body").expect("the page body");
            // Well inside the body while the page can still scroll, at its edge when it cannot.
            let margin = if step < 30 { 40.0 } else { 0.0 };
            if let Some(at) = crate::scenario::handle_at(handle)
                && body.shrink(margin).contains(egui::pos2(at.x, at.y))
            {
                // The wheel scrolls smoothly: let it come to rest, then click where the handle is now.
                h.frames(20);
                let at = crate::scenario::handle_at(handle).unwrap_or(at);
                if body.contains(egui::pos2(at.x, at.y)) {
                    h.step(&serde_json::json!({"click": {"handle": handle}})).unwrap();
                    h.frames(5);
                    return;
                }
            }
            h.app.synthetic.push(egui::Event::PointerMoved(body.center()));
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -60.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            });
            h.frames(2);
        }
        panic!("{handle} never came into view");
    }

    /// The page's rows share one label column, widened by the longest label in the form. The 3D
    /// mouse section brought longer labels, which moved every control (Orbit around's among them) to
    /// the right and turned its three buttons into a dropdown. Measured: each row's control left edge and
    /// width with the section open and folded, at the window sizes the app is used at.
    #[test]
    fn folding_the_3d_mouse_section_does_not_move_the_controls_of_the_rows_above_it() {
        let mut report = Vec::new();
        for width in [900, 1100, 1318, 1600, 1920] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.space_mouse.device = Some("hid 046d:c62b".into());
            h.call("ui.resize", serde_json::json!({"width": width, "height": 900}));
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
            h.frames(6);
            let rows = ["Orbit around", "Middle double-click", "View navigator"];
            let control = |name: &str| crate::scenario::rect_of(&format!("dialog:control:{name}")).map(|r| (r.left(), r.width()));
            let buttons = || crate::scenario::rect_of("dialog:segments:Orbit around").is_some();
            let (open, open_form) = (rows.map(control), buttons());
            scroll_to_and_click(&mut h, "dialog:section:3D mouse");
            // Back to the top, where the rows above are on screen again.
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 5000.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            });
            h.frames(6);
            let (folded, folded_form) = (rows.map(control), buttons());
            eprintln!("window {width}: open {open:?} buttons {open_form}; folded {folded:?} buttons {folded_form}");
            if folded != open || folded_form != open_form {
                report.push(format!("window {width}: open {open:?} buttons {open_form}, folded {folded:?} buttons {folded_form}"));
            }
        }
        assert!(report.is_empty(), "{}", report.join("\n"));
    }

    /// The meter in a view axis row shows the motion chosen in its "driven by" menu, calibrated or not.
    #[test]
    fn an_axis_meter_follows_the_motion_that_drives_it() {
        let raw = [0, 0, 0, 175, 0, 0];
        let full = f32::from(crate::space_mouse::FULL_SCALE);
        let mut source = crate::space_mouse::SAME_AXES;
        assert_eq!(axis_meter(None, raw, source, 2).0, 0.0, "Zoom reads its own motion");
        crate::space_mouse::reassign(&mut source, 2, 3);
        assert_eq!(axis_meter(None, raw, source, 2), (175.0 / full, [0.0; 2]), "Zoom now shows Rotation X");
        assert_eq!(axis_meter(None, raw, source, 3).0, 0.0, "and the row that gave Rotation X up shows what it took in exchange");
        let axes =
            std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] });
        let calibration = solidcraft_space_mouse::Calibration { dead_zone_share: 0.2, kind: solidcraft_space_mouse::Kind::ReturnToCenter, axes };
        assert_eq!(axis_meter(Some(&calibration), raw, source, 2), (0.5, [0.2; 2]), "calibrated: share of travel, with the dead zone");
    }

    /// Restore defaults for the 3D mouse section puts back what it lists, and nothing else.
    #[test]
    fn restoring_the_3d_mouse_section_resets_its_settings_and_keeps_the_rest() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let axes =
            std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] });
        let calibration = solidcraft_space_mouse::Calibration { dead_zone_share: 0.2, kind: solidcraft_space_mouse::Kind::ReturnToCenter, axes };
        let p = &mut h.app.preferences;
        p.space_mouse_calibrations.insert("hid 046d:c62b".into(), calibration);
        p.space_mouse_buttons.insert("hid 046d:c62b".into(), [(1, crate::space_mouse::ButtonJob::Settings)].into());
        p.space_mouse_offered.insert("hid 046d:c62b".into());
        p.space_mouse_offer_never = true;
        p.space_mouse_enabled = false;
        p.space_mouse_driver = SpaceMouseDriver::Direct;
        p.space_mouse_speed = 2.5;
        p.space_mouse_reverse = [true; 6];
        p.space_mouse_axis_speed = [0.25; 6];
        p.space_mouse_axis_source = [1, 0, 2, 3, 4, 5];
        p.space_mouse_ghost_position = SpaceMouseGhost::LeftWindow;
        p.space_mouse_in_view = true;
        let before = h.app.preferences.clone();
        let camera = h.app.cam;
        restore_space_mouse_defaults(&mut h.app);
        let d = Prefs::default();
        let p = &h.app.preferences;
        assert_eq!(
            (p.space_mouse_driver, p.space_mouse_speed, p.space_mouse_reverse),
            (d.space_mouse_driver, d.space_mouse_speed, d.space_mouse_reverse)
        );
        assert_eq!((p.space_mouse_axis_speed, p.space_mouse_axis_source), (d.space_mouse_axis_speed, d.space_mouse_axis_source));
        assert_eq!((p.space_mouse_ghost_position, p.space_mouse_in_view), (d.space_mouse_ghost_position, d.space_mouse_in_view));
        // Kept: the calibration (dead zone too), the buttons, the offered set, never, enabled, the rest of the page.
        assert_eq!(p.space_mouse_calibrations, before.space_mouse_calibrations);
        assert_eq!(p.space_mouse_buttons, before.space_mouse_buttons);
        assert_eq!(p.space_mouse_offered, before.space_mouse_offered);
        assert!(p.space_mouse_offer_never && !p.space_mouse_enabled);
        assert_eq!(p.nav, before.nav);
        assert_eq!(h.app.cam, camera);
        // And it asks first: the real button only arms it (see cancelling_the_restore_question_changes_nothing).
        assert!(!h.app.space_mouse.restore_pending);
    }

    /// Opens Preferences on the Navigation page with the 3D mouse section open and these settings
    /// changed from their defaults, as a person who changed them would find it.
    fn changed_3d_mouse_settings(device: Option<&str>) -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = device.map(str::to_owned);
        let axes =
            std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration { source, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] });
        let calibration = solidcraft_space_mouse::Calibration { dead_zone_share: 0.2, kind: solidcraft_space_mouse::Kind::ReturnToCenter, axes };
        let p = &mut h.app.preferences;
        p.space_mouse_calibrations.insert("hid 046d:c62b".into(), calibration);
        p.space_mouse_driver = SpaceMouseDriver::Direct;
        p.space_mouse_speed = 2.5;
        p.space_mouse_reverse = [true; 6];
        p.space_mouse_axis_speed = [0.25; 6];
        p.space_mouse_axis_source = [1, 0, 2, 3, 4, 5];
        p.space_mouse_ghost_position = SpaceMouseGhost::LeftWindow;
        p.space_mouse_in_view = true;
        h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
        h.frames(36);
        open_advanced(&mut h);
        h
    }

    /// Opens the section's "Advanced" disclosure, which starts closed. Once per harness: it toggles.
    fn open_advanced(h: &mut crate::scenario::Harness) {
        scroll_to_and_click(h, "dialog:section:Advanced");
        h.frames(10);
    }

    /// The driver, dead zone, axis table and buttons are rarely needed, so they start folded away and
    /// are not drawn until the reader opens "Advanced"; the everyday controls are there either way.
    #[test]
    fn the_advanced_3d_mouse_settings_start_folded_away() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = Some("hid 046d:c62b".into());
        h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: true, denied: false });
        h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
        h.frames(36);
        let drawn = |name: &str| crate::scenario::rect_of(&format!("dialog:row:{name}")).is_some();
        let (driver, axes) = (crate::i18n::tr("ui.3d_mouse_driver"), crate::i18n::tr("ui.3d_mouse_button").replace("{number}", "1"));
        assert!(drawn("Calibration") && drawn("Overall speed"), "everyday controls are always there");
        assert!(!drawn(driver) && !drawn(&axes), "closed: no driver row, no buttons");
        assert!(crate::scenario::rect_of("dialog:section:Advanced").is_some());
        open_advanced(&mut h);
        assert!(drawn(driver), "open: the driver row");
        assert!(crate::scenario::rect_of("dialog:button::Restore defaults").is_some(), "open: the restore button");
        open_advanced(&mut h);
        assert!(!drawn(driver), "closed again");
    }

    /// The question is short, says what stays, and exists in every language.
    #[test]
    fn the_restore_question_is_short_and_says_what_stays() {
        let english = crate::i18n::tr("ui.3d_mouse_restore_ask");
        assert_eq!(english, "Set the 3D mouse settings back to their defaults? Your calibration and button mapping stay.");
        for catalog in crate::i18n::LOCALES {
            let text = crate::i18n::Locale::for_language(catalog.id).text("ui.3d_mouse_restore_ask");
            assert!(text.chars().count() > 20 && text.chars().count() < 140 && text != "ui.3d_mouse_restore_ask", "{}", catalog.id);
        }
    }

    /// Cancel on the question leaves every setting as it was, and the question goes away.
    #[test]
    fn cancelling_the_restore_question_changes_nothing() {
        let mut h = changed_3d_mouse_settings(Some("hid 046d:c62b"));
        let before = h.app.preferences.clone();
        scroll_to_and_click(&mut h, "dialog:button::Restore defaults");
        assert!(h.app.space_mouse.restore_pending, "the first click only asks");
        assert_eq!(h.app.preferences, before, "asking changes nothing");
        scroll_to_and_click(&mut h, "dialog:button::Cancel");
        assert!(!h.app.space_mouse.restore_pending);
        assert_eq!(h.app.preferences, before);
    }

    /// Accepting works with no device connected, and a device connecting afterwards does not bring the
    /// old values back (nothing is kept apart from the preferences, and the calibration stays).
    #[test]
    fn restoring_with_no_device_works_and_a_later_connection_brings_nothing_back() {
        let mut h = changed_3d_mouse_settings(None);
        let calibrations = h.app.preferences.space_mouse_calibrations.clone();
        scroll_to_and_click(&mut h, "dialog:button::Restore defaults");
        scroll_to_and_click(&mut h, "dialog:button::Restore defaults");
        assert!(!h.app.space_mouse.restore_pending);
        let d = Prefs::default();
        let restored = |h: &crate::scenario::Harness| {
            let p = &h.app.preferences;
            (p.space_mouse_driver, p.space_mouse_speed, p.space_mouse_reverse, p.space_mouse_axis_speed, p.space_mouse_axis_source)
                == (d.space_mouse_driver, d.space_mouse_speed, d.space_mouse_reverse, d.space_mouse_axis_speed, d.space_mouse_axis_source)
                && (p.space_mouse_ghost_position, p.space_mouse_in_view) == (d.space_mouse_ghost_position, d.space_mouse_in_view)
        };
        assert!(restored(&h), "restored with the device away");
        // The device connects (and its calibration applies): the defaults stay.
        h.app.space_mouse.device = Some("hid 046d:c62b".into());
        h.frames(30);
        assert!(restored(&h), "still the defaults after the device connected");
        assert_eq!(h.app.preferences.space_mouse_calibrations, calibrations, "its calibration is kept");
    }

    /// What a restore sets, and what the prefs file holds of it.
    fn section_is_at_defaults(app: &SolveApp) -> bool {
        let (p, d) = (&app.preferences, Prefs::default());
        (p.space_mouse_driver, p.space_mouse_speed, p.space_mouse_reverse, p.space_mouse_axis_speed, p.space_mouse_axis_source)
            == (d.space_mouse_driver, d.space_mouse_speed, d.space_mouse_reverse, d.space_mouse_axis_speed, d.space_mouse_axis_source)
            && (p.space_mouse_ghost_position, p.space_mouse_in_view) == (d.space_mouse_ghost_position, d.space_mouse_in_view)
    }

    /// Restore, then closing the dialog with its own Cancel while the question is up: the question is not
    /// still asked when the dialog opens again, and nothing was changed.
    #[test]
    fn closing_preferences_with_the_restore_question_up_does_not_leave_it_armed() {
        let mut h = changed_3d_mouse_settings(Some("hid 046d:c62b"));
        let before = h.app.preferences.clone();
        scroll_to_and_click(&mut h, "dialog:button::Restore defaults");
        assert!(h.app.space_mouse.restore_pending);
        h.step(&serde_json::json!({"click": {"handle": "dialog:cancel"}})).unwrap();
        h.frames(6);
        assert!(!h.app.prefs_window.open, "the dialog closed");
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
        h.frames(6);
        assert!(!h.app.space_mouse.restore_pending, "the question is not asked again on its own");
        assert_eq!(h.app.preferences, before);
    }

    /// Confirming with the real buttons is what the prefs file holds afterwards: a new run that loads it
    /// has the defaults for the section and the calibration still there.
    #[test]
    fn a_confirmed_restore_is_what_the_next_run_loads() {
        let mut h = changed_3d_mouse_settings(Some("hid 046d:c62b"));
        let calibrations = h.app.preferences.space_mouse_calibrations.clone();
        assert!(!section_is_at_defaults(&h.app));
        scroll_to_and_click(&mut h, "dialog:button::Restore defaults");
        scroll_to_and_click(&mut h, "dialog:button::Restore defaults");
        assert!(!h.app.space_mouse.restore_pending && section_is_at_defaults(&h.app));
        let saved = h.app.prefs();
        let mut next = crate::scenario::Harness::new();
        next.app.preferences.space_mouse_speed = 2.5;
        assert!(!section_is_at_defaults(&next.app));
        next.app.load_prefs(&saved);
        assert!(section_is_at_defaults(&next.app), "the defaults are what was saved");
        assert_eq!(next.app.preferences.space_mouse_calibrations, calibrations, "and the calibration with them");
        assert!(!next.app.space_mouse.restore_pending);
    }

    /// The visible Calibrate button on the offer's card and the one in Preferences both open the wizard
    /// for the device, at its first screen, with Preferences open.
    #[test]
    fn the_calibrate_buttons_on_the_card_and_in_preferences_start_the_same_wizard() {
        let wizard = |h: &crate::scenario::Harness| {
            h.app.space_mouse.calibrating.as_ref().map(|(device, wizard)| (device.clone(), format!("{:?} {:?}", wizard.stage(), wizard.kind())))
        };
        let harness = || {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.space_mouse.device = Some("spacenavd".into());
            h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
            h
        };
        // The card's.
        let mut card = harness();
        card.frames(120);
        assert!(card.app.space_mouse.offer_since.is_some() && card.app.space_mouse.calibrating.is_none());
        card.step(&serde_json::json!({"widget": "Calibrate…"})).unwrap();
        card.frames(5);
        // Preferences'.
        let mut prefs = harness();
        crate::prefs::open_at(&mut prefs.app, crate::prefs::PAGE_NAVIGATION);
        prefs.frames(6);
        prefs.frames(30);
        assert!(prefs.app.space_mouse.calibrating.is_none());
        scroll_to_and_click(&mut prefs, "dialog:button:Calibration:Calibrate…");
        assert!(card.app.prefs_window.open && prefs.app.prefs_window.open);
        assert!(wizard(&card).is_some(), "the card's button started it");
        assert_eq!(wizard(&card), wizard(&prefs), "and Preferences' starts the same one");
    }

    /// Direct or Automatic chosen with the device present but unopenable: the daemon is read, and Preferences
    /// says so next to the setting instead of leaving "Direct (hidraw)" to imply the device. Spacenavd
    /// chosen, or the device itself read, or no device: no such note.
    #[test]
    fn preferences_says_when_the_daemon_is_read_instead_of_the_device_asked_for() {
        use SpaceMouseDriver::{Automatic, Direct, Spacenavd};
        let note = crate::i18n::tr("ui.3d_mouse_driver_fallback").to_string();
        assert!(note.contains("spacenavd") && note.contains("could not be opened"));
        for catalog in crate::i18n::LOCALES {
            let text = crate::i18n::Locale::for_language(catalog.id).text("ui.3d_mouse_driver_fallback");
            assert!(text.chars().count() > 20 && text != "ui.3d_mouse_driver_fallback", "{}", catalog.id);
        }
        // (driver, device being read, device present) and whether the note shows.
        let cases = [
            (Direct, "spacenavd", true, true),
            (Automatic, "spacenavd", true, true),
            (Spacenavd, "spacenavd", true, false),
            (Direct, "hid 046d:c62b", true, false),
            (Automatic, "hid 046d:c62b", true, false),
            (Direct, "spacenavd", false, false),
        ];
        for (driver, device, direct, shown) in cases {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.space_mouse_driver = driver;
            h.app.space_mouse.device = Some(device.into());
            h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct, denied: false });
            h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
            h.frames(6);
            h.frames(30);
            open_advanced(&mut h);
            let up = h.widget(&note).is_some();
            assert_eq!(up, shown, "{driver:?} reading {device}, device present {direct}");
            // Whatever it says, no calibration moves between keys.
            assert!(h.app.preferences.space_mouse_calibrations.is_empty());
        }
    }

    /// The section's buttons keep inside the page in every language, at the dialog's width (which the
    /// window does not narrow): the arming Restore button, Reset axis assignment, Reset view under the
    /// test model, and the buttons' own reset.
    #[test]
    fn the_3d_mouse_buttons_fit_the_page_in_every_language() {
        let keys = ["ui.3d_mouse_restore", "ui.3d_mouse_reset_axes", "ui.3d_mouse_reset_view", "ui.3d_mouse_buttons_reset"];
        let mut too_wide = Vec::new();
        for catalog in crate::i18n::LOCALES {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.language = catalog.id.to_string();
            h.app.space_mouse.device = Some("hid 046d:c62b".into());
            let axes = std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration {
                source,
                reversed: false,
                rest: 0,
                dead_zone: 0,
                full: [350; 2],
            });
            h.app.preferences.space_mouse_calibrations.insert(
                "hid 046d:c62b".into(),
                solidcraft_space_mouse::Calibration { dead_zone_share: 0.0, kind: solidcraft_space_mouse::Kind::ReturnToCenter, axes },
            );
            h.app.preferences.space_mouse_buttons.insert("hid 046d:c62b".into(), [(1, crate::space_mouse::ButtonJob::Settings)].into());
            // The axis assignment is changed, so its reset button is enabled and drawn.
            h.app.preferences.space_mouse_axis_source = [1, 0, 2, 3, 4, 5];
            h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
            h.frames(6);
            scroll_to_and_click(&mut h, &format!("dialog:section:{}", crate::i18n::Locale::for_language(catalog.id).text("Advanced")));
            h.frames(10);
            // The restore question is up, so its whole text can be checked too.
            h.app.space_mouse.restore_pending = true;
            // The frame's language is only set while it runs, so the texts come from the catalog itself.
            let text = |key: &str| crate::i18n::Locale::for_language(catalog.id).text(key).to_string();
            h.frames(30);
            let body = crate::scenario::rect_of("prefs:page_body").expect("the page body");
            // The question: all of it is laid out (its box is as tall as its wrapped text needs) and the
            // buttons start below it.
            let question = text("ui.3d_mouse_restore_ask");
            match crate::scenario::rect_of("dialog:hint:space_mouse_restore_ask") {
                Some(note) => {
                    let t = crate::theme::Tokens::get();
                    let needed = h.ctx.fonts_mut(|fonts| {
                        fonts.layout(question.clone(), egui::FontId::proportional(t.font_small), egui::Color32::WHITE, note.width()).size().y
                    });
                    if note.height() + 0.5 < needed {
                        too_wide.push(format!("{} question is {:.0} px tall, its text needs {needed:.0}", catalog.id, note.height()));
                    }
                    if note.left() < body.left() - 0.5 || note.right() > body.right() + 0.5 {
                        too_wide.push(format!("{} question {:.0}..{:.0} outside the page", catalog.id, note.left(), note.right()));
                    }
                    let cancel = crate::scenario::rect_of(&format!("dialog:button::{}", text("ui.cancel")));
                    if cancel.is_some_and(|cancel| cancel.top() < note.bottom() - 0.5) {
                        too_wide.push(format!("{} the buttons overlap the question", catalog.id));
                    }
                }
                None => too_wide.push(format!("{} question not drawn", catalog.id)),
            }
            for key in keys {
                let label = text(key);
                let Some(rect) = crate::scenario::rect_of(&format!("dialog:button::{label}")) else {
                    too_wide.push(format!("{} {key}: not drawn", catalog.id));
                    continue;
                };
                if rect.right() > body.right() + 0.5 || rect.left() < body.left() - 0.5 {
                    too_wide.push(format!(
                        "{} {key} \"{label}\": {:.0}..{:.0} outside the page {:.0}..{:.0}",
                        catalog.id,
                        rect.left(),
                        rect.right(),
                        body.left(),
                        body.right()
                    ));
                }
            }
        }
        assert!(too_wide.is_empty(), "{}", too_wide.join("\n"));
    }

    /// The widest of the card's button labels stays well inside the narrowest card (346 px wide at a 760 px
    /// window, less its padding) in every language, so a button never has to be wider than its card.
    #[test]
    fn the_offers_buttons_fit_the_narrowest_card_in_every_language() {
        let ctx = crate::fonts::test_context();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let font = egui::FontId::proportional(crate::theme::Tokens::get().font_label);
        let mut wide = Vec::new();
        for catalog in crate::i18n::LOCALES {
            use crate::space_mouse_offer::Action;
            for key in [Action::Swap, Action::Calibrate, Action::Never, Action::Close].map(|action| action.label()) {
                let label = crate::i18n::Locale::for_language(catalog.id).text(key).to_string();
                let width = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE).size().x);
                if width > 250.0 {
                    wide.push(format!("{} {key} \"{label}\" is {width:.0} px", catalog.id));
                }
            }
        }
        assert!(wide.is_empty(), "{}", wide.join("\n"));
    }

    /// The circles' tooltips come from the one function the rows' labels come from, after a
    /// reassignment, for both kinds of driver mapping (and none), and an inverted axis reads the same.
    #[test]
    fn the_circles_tooltips_say_what_the_rows_say_after_a_reassignment() {
        use crate::space_mouse::{DriverMapping, SAME_AXES, reassign, row_label, stories};
        let shares = [[0.1_f32, 0.2]; 6];
        let mut inverted = DriverMapping::object_mode();
        for rule in &mut inverted.axes {
            rule.reversed = !rule.reversed;
        }
        let mappings = [Some(DriverMapping::default()), Some(DriverMapping::object_mode()), Some(inverted), None];
        for mapping in mappings {
            let mut source = SAME_AXES;
            reassign(&mut source, 2, 1);
            let said = stories("en", mapping, source);
            let say = |row: usize| crate::i18n::Locale::for_language("en").format(row_label(mapping, row), &[("n", &(row + 1).to_string())]);
            // Motion 1 is read by row 2 now, motion 2 by row 1; the others by their own rows.
            assert_eq!(said[1], say(2), "{mapping:?}");
            assert_eq!(said[2], say(1), "{mapping:?}");
            assert_eq!(said[0], say(0));
            let tip = crate::space_mouse_calibration::pad_tooltip(&shares, (0, 1), &said);
            assert_eq!(tip, format!("X −10 % +20 % → {}\nY −10 % +20 % → {}", say(0), say(2)), "{mapping:?}");
            // Invert is not in the story: the same words with every axis inverted.
            let mut flipped = mapping;
            if let Some(mapping) = flipped.as_mut() {
                mapping.axes.iter_mut().for_each(|rule| rule.reversed = !rule.reversed);
            }
            assert_eq!(stories("en", flipped, source), said);
        }
        // A motion nobody reads says nothing, so no arrow.
        let none = stories("en", Some(DriverMapping::default()), [0, 0, 0, 0, 0, 0]);
        assert!(none[3].is_empty());
        assert_eq!(crate::space_mouse_calibration::pad_tooltip(&shares, (3, 4), &none), "Rx −10 % +20 %\nRy −10 % +20 %");
    }

    /// The circles say what the view does with their axes in the rows' own words, and keep saying it
    /// after an axis is reassigned.
    #[test]
    fn the_circles_and_the_rows_tell_the_same_story_after_a_reassignment() {
        use crate::space_mouse::{DriverMapping, SAME_AXES, motion_story, reassign, row_label};
        let mapping = Some(DriverMapping::default());
        // Unchanged: each motion is read by the row of its own number.
        for motion in 0..6 {
            assert_eq!(motion_story(mapping, SAME_AXES, motion), Some(row_label(mapping, motion)));
        }
        // Row 2 (lift) now reads motion 1 (push): the motion's story is that row's label, and row 1 reads motion 2.
        let mut source = SAME_AXES;
        reassign(&mut source, 2, 1);
        assert_eq!(motion_story(mapping, source, 1), Some(row_label(mapping, 2)));
        assert_eq!(motion_story(mapping, source, 2), Some(row_label(mapping, 1)));
        assert_ne!(motion_story(mapping, source, 1), motion_story(mapping, SAME_AXES, 1));
        // Not one motion per row: a motion nobody reads has no story.
        assert_eq!(motion_story(mapping, [0, 0, 0, 0, 0, 0], 3), None);
        assert!(crate::i18n::tr("ui.3d_mouse_circles_help").contains("test model"));
    }

    /// The "wiggle it" hint is a visible line as well as a tooltip, only while a reader is reachable and
    /// silent; the section's own header carries no badge any more.
    #[test]
    fn a_visible_line_says_what_the_3d_mouse_waits_for() {
        assert_eq!(crate::i18n::tr("ui.3d_mouse_waiting"), "Waiting for Movement");
        assert_eq!(crate::i18n::tr("ui.3d_mouse_reset_view"), "Reset view");
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = None;
        h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: false, denied: false });
        h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_NAVIGATION);
        h.frames(5);
        assert!(crate::scenario::rect_of("dialog:section:3D mouse").is_some(), "the section is drawn with the kit's header");
        assert!(crate::scenario::rect_of("dialog:section:3D mouse:count").is_none());
        assert!(crate::scenario::rect_of("dialog:hint:space_mouse_wiggle").is_some());
    }

    /// The boat's Reset view puts the boat back and nothing else: calibration, sensitivity, axis
    /// assignment and the document's camera stay. Clicked as a person would.
    #[test]
    fn reset_view_is_scoped_to_the_test_model() {
        let mut h = changed_3d_mouse_settings(Some("hid 046d:c62b"));
        h.frames(10);
        // The boat has been turned.
        let mut boat = crate::test_boat::Boat::new(30.0);
        boat.cam.yaw += 1.0;
        h.app.space_mouse.boat = Some(boat);
        let before = (h.app.cam, h.app.preferences.clone());
        scroll_to_and_click(&mut h, "dialog:button::Reset view");
        assert!(
            h.app.space_mouse.boat.is_none()
                || h.app.space_mouse.boat.as_ref().is_some_and(|boat| boat.cam.yaw == crate::test_boat::Boat::new(30.0).cam.yaw),
            "the boat is back"
        );
        assert_eq!(h.app.cam, before.0, "the document's camera");
        assert_eq!(h.app.preferences, before.1, "every setting, calibration included");
        assert_eq!(crate::i18n::tr("ui.3d_mouse_reset_view"), "Reset view");
    }

    /// The "wiggle it" hint exists in every catalog and is a visible line while no device is confirmed,
    /// gone once there is one.
    #[test]
    fn the_wiggle_hint_has_text_and_shows_until_a_device_is_confirmed() {
        assert_eq!(crate::i18n::tr("ui.3d_mouse_wiggle"), "If you have one plugged in, wiggle it.");
        for catalog in crate::i18n::LOCALES {
            let text = crate::i18n::Locale::for_language(catalog.id).text("ui.3d_mouse_wiggle");
            assert!(!text.is_empty() && text != "ui.3d_mouse_wiggle", "{}", catalog.id);
        }
        let mut h = changed_3d_mouse_settings(None);
        h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: false, denied: false });
        h.frames(10);
        assert!(crate::scenario::rect_of("dialog:hint:space_mouse_wiggle").is_some(), "no device yet: the hint is a line");
        h.app.space_mouse.device = Some("spacenavd".into());
        h.frames(10);
        assert!(crate::scenario::rect_of("dialog:hint:space_mouse_wiggle").is_none(), "confirmed: no hint");
    }

    /// A 3D mouse that is there but may not be read, with no spacenavd to ask instead: Preferences says so
    /// in plain words and does not ask for a movement; with a daemon reachable it still waits for one.
    #[test]
    fn preferences_says_when_a_device_is_found_but_not_allowed() {
        for catalog in crate::i18n::LOCALES {
            for key in ["ui.3d_mouse_denied", "ui.3d_mouse_denied_short"] {
                let text = crate::i18n::Locale::for_language(catalog.id).text(key);
                assert!(text.chars().count() > 5 && text != key, "{} {key}", catalog.id);
            }
        }
        assert!(crate::i18n::tr("ui.3d_mouse_denied").contains("udev"));
        let cases = [
            // (spacenavd there, device found, denied) and whether the denied line shows.
            (false, true, true, true),
            (false, true, false, false),
            (true, true, true, false),
            (false, false, false, false),
        ];
        for (spacenavd, direct, denied, shown) in cases {
            let mut h = changed_3d_mouse_settings(None);
            h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd, direct, denied });
            h.frames(10);
            let said = crate::scenario::rect_of("dialog:hint:space_mouse_denied").is_some();
            assert_eq!(said, shown, "spacenavd {spacenavd}, found {direct}, denied {denied}");
            let wiggle = crate::scenario::rect_of("dialog:hint:space_mouse_wiggle").is_some();
            assert_eq!(wiggle, !shown, "the hint to wiggle shows only where waiting can help");
            assert_eq!(super::waiting_for_a_record(h.app.space_mouse.drivers), spacenavd || (direct && !denied));
        }
    }

    /// The dialog's Restore Defaults on the page that holds the 3D mouse section resets the page, and the
    /// 3D mice already offered a calibration stay offered (like their calibrations and button mappings):
    /// the offer does not come back for every device.
    #[test]
    fn the_dialogs_restore_defaults_keeps_the_offered_set_with_the_calibrations() {
        let mut h = changed_3d_mouse_settings(Some("hid 046d:c62b"));
        h.app.preferences.space_mouse_offered.insert("hid 046d:c62b".into());
        h.app.preferences.space_mouse_buttons.insert("hid 046d:c62b".into(), [(1, crate::space_mouse::ButtonJob::Settings)].into());
        let (offered, calibrations, buttons) = (
            h.app.preferences.space_mouse_offered.clone(),
            h.app.preferences.space_mouse_calibrations.clone(),
            h.app.preferences.space_mouse_buttons.clone(),
        );
        h.call("ui.prefs", serde_json::json!({"button": "defaults"}));
        h.frames(3);
        assert!(h.app.preferences.space_mouse_speed == Prefs::default().space_mouse_speed, "the page was reset");
        assert_eq!(h.app.preferences.space_mouse_offered, offered);
        assert_eq!(h.app.preferences.space_mouse_calibrations, calibrations);
        assert_eq!(h.app.preferences.space_mouse_buttons, buttons);
    }

    /// The first screen of the calibration (which kind of device) fits at the window's widths:
    /// both pictures are inside it, side by side or one under the other.
    #[test]
    fn the_calibration_choose_screen_fits_at_every_width() {
        for width in [300.0_f32, 360.0, 520.0, 900.0] {
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.space_mouse.device = Some("hid 046d:c62b".into());
            crate::space_mouse_calibration::start(&mut app);
            let ctx = crate::fonts::test_context();
            for _ in 0..6 {
                crate::scenario::clear_handles();
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0))),
                    focused: true,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| crate::space_mouse_calibration::show(&mut app, ui.ctx()));
                output.textures_delta.clear();
            }
            let window = crate::scenario::rect_of("dialog:window").expect("the window is drawn");
            let pictures = crate::scenario::rects("space_mouse:picture");
            assert_eq!(pictures.len(), 2, "width {width}: both kinds are offered");
            for (_, picture) in &pictures {
                assert!(window.contains_rect(*picture), "width {width}: {picture:?} is outside {window:?}");
            }
            assert!(!pictures[0].1.intersects(pictures[1].1), "width {width}: the pictures overlap");
        }
    }

    /// The six readings in the calibration window are a clean grid whatever the values: each name
    /// and its number are in their own column, never wrap or overlap, each number is centred under
    /// its name, and the block is the same height for every value, so the window does not jitter.
    #[test]
    fn the_calibration_readings_are_a_clean_grid_of_constant_height() {
        let sets: [[i16; 6]; 6] = [[0; 6], [-1; 6], [350; 6], [-350; 6], [i16::MAX; 6], [i16::MIN; 6]];
        for width in [360.0_f32, 520.0, 900.0] {
            let mut heights = Vec::new();
            for values in sets {
                let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
                app.space_mouse.device = Some("hid 046d:c62b".into());
                app.space_mouse.raw = values.into();
                crate::space_mouse_calibration::start(&mut app);
                let ctx = crate::fonts::test_context();
                for _ in 0..4 {
                    crate::scenario::clear_handles();
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0))),
                        focused: true,
                        ..Default::default()
                    };
                    let mut output = ctx.run_ui(input, |ui| crate::space_mouse_calibration::show(&mut app, ui.ctx()));
                    output.textures_delta.clear();
                }
                let rect = |kind: &str, i: usize| crate::scenario::rect_of(&format!("space_mouse:reading_{kind}:{i}")).expect("drawn");
                let all: Vec<egui::Rect> = (0..6).flat_map(|i| [rect("name", i), rect("value", i)]).collect();
                for (a, first) in all.iter().enumerate() {
                    for second in &all[a + 1..] {
                        assert!(!first.intersects(*second), "width {width} {values:?}: {first:?} overlaps {second:?}");
                    }
                }
                for i in 0..6 {
                    let (name, value) = (rect("name", i), rect("value", i));
                    assert!((name.center().x - value.center().x).abs() <= 1.0, "width {width} {values:?}: number {i} is not under its name");
                    assert!(value.top() >= name.bottom(), "number {i} is not on the line below");
                    assert_eq!(name.top(), rect("name", 0).top(), "the names share a baseline");
                    assert_eq!(value.top(), rect("value", 0).top(), "the numbers share a baseline");
                    assert!(value.width() <= 48.0, "{values:?}: number {i} is wider than its column");
                }
                heights.push(crate::scenario::rect_of("space_mouse:reading_block").expect("block").height());
            }
            assert!(heights.windows(2).all(|pair| pair[0] == pair[1]), "width {width}: the block's height depends on the values: {heights:?}");
        }
    }

    /// On the last screen, with the pictures and pads, the window is the same size for any live reading.
    #[test]
    fn the_calibration_window_keeps_its_size_while_the_cap_moves() {
        let mut windows = Vec::new();
        for values in [[0_i16; 6], [-1; 6], [350; 6], [-350; 6], [i16::MAX; 6], [i16::MIN; 6], [159, -5, 0, 32, -17, 5]] {
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.space_mouse.device = Some("hid 046d:c62b".into());
            crate::space_mouse_calibration::start(&mut app);
            {
                let wizard = &mut app.space_mouse.calibrating.as_mut().unwrap().1;
                wizard.choose(solidcraft_space_mouse::Kind::ReturnToCenter);
                let hold = |wizard: &mut solidcraft_space_mouse::Wizard, reading: [i16; 6], seconds: f64| {
                    for _ in 0..(seconds / 0.02).round() as usize {
                        wizard.feed(reading, 0.02);
                    }
                };
                hold(wizard, [0; 6], 1.6);
                hold(wizard, [0; 6], 2.6);
                for step in solidcraft_space_mouse::STEPS {
                    let mut raw = [0_i16; 6];
                    raw[step.axis] = if step.positive { 300 } else { -300 };
                    hold(wizard, raw, 0.5);
                    hold(wizard, [0; 6], 0.1);
                }
                assert!(matches!(wizard.result(), Some(Ok(_))), "the calibration reached its result screen");
            }
            app.space_mouse.raw = values.into();
            let ctx = crate::fonts::test_context();
            for _ in 0..4 {
                crate::scenario::clear_handles();
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0))),
                    focused: true,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| crate::space_mouse_calibration::show(&mut app, ui.ctx()));
                output.textures_delta.clear();
            }
            windows.push(crate::scenario::rect_of("dialog:window").expect("the window is drawn"));
        }
        assert!(windows.windows(2).all(|pair| pair[0] == pair[1]), "the window changes with the readings: {windows:?}");
    }

    /// One button job this build can't read (a newer release's) costs that button, not the lot.
    #[test]
    fn one_unreadable_button_job_keeps_the_others() {
        use crate::space_mouse::{ButtonJob, ButtonKey};
        let saved = serde_json::json!({ "space_mouse_buttons": {
            "hid 046d:c62b": { "1": { "key": "esc" }, "2": { "teleport": 3 }, "x": "settings", "3": "settings" },
            "hid 256f:c63a": { "4": { "view": "top" } },
            "spacenavd": 7,
        } });
        let prefs: super::Prefs = serde_json::from_value(saved).expect("reads");
        let jobs = &prefs.space_mouse_buttons;
        assert_eq!(jobs["hid 046d:c62b"], [(1, ButtonJob::Key(ButtonKey::Esc)), (3, ButtonJob::Settings)].into());
        assert_eq!(jobs["hid 256f:c63a"], [(4, ButtonJob::View("top".into()))].into());
        assert!(!jobs.contains_key("spacenavd"));
    }

    /// A calibration as saved: the same text loads and is usable on 32 and 64 bits, and one saved
    /// with the old `usize::MAX` (64-bit native) still reads as having no source on 64 bits.
    #[test]
    fn a_saved_calibration_with_no_source_reads_the_same_on_every_width() {
        use solidcraft_space_mouse::{Calibration, NO_SOURCE};
        let axis = |source: &str, full: u32| format!(r#"{{"source":{source},"reversed":false,"rest":0,"dead_zone":3,"full":[{full},{full}]}}"#);
        let json = |absent: &str| {
            let axes = [axis("0", 300), axis("1", 300), axis("2", 300), axis(absent, 1), axis(absent, 1), axis("3", 300)].join(",");
            format!(r#"{{"dead_zone_share":0.0,"kind":"Trackball","axes":[{axes}]}}"#)
        };
        let fixed: Calibration = serde_json::from_str(&json("65535")).unwrap();
        assert_eq!(fixed.axes.map(|axis| axis.source), [0, 1, 2, NO_SOURCE, NO_SOURCE, 3]);
        assert!(fixed.is_usable());
        assert_eq!(serde_json::to_string(&fixed.axes[3].source).unwrap(), "65535");
        #[cfg(target_pointer_width = "64")]
        assert_eq!(serde_json::from_str::<Calibration>(&json("18446744073709551615")).unwrap(), fixed);
    }

    #[test]
    fn changing_preferences_page_cancels_shortcut_capture() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        h.step(&json!({"click":{"handle":"keymap:filter"}})).unwrap();
        h.step(&json!({"text":"solid.extrude"})).unwrap();
        h.step(&json!({"click":{"handle":"keymap:capture:solid.extrude"}})).unwrap();
        h.step(&json!({"key":"M","pressed":true})).unwrap();
        h.step(&json!({"click":{"handle":"prefs:page:General"}})).unwrap();
        assert!(!crate::keymap::capturing(&h.app));
        assert_eq!(crate::keymap::effective(&h.app, "solid.extrude").as_deref(), Some("E"));
        h.frames(65);
        assert_eq!(crate::keymap::effective(&h.app, "solid.extrude").as_deref(), Some("E"));
    }

    #[test]
    fn display_quality_cards_show_increasing_facet_detail() {
        let mut app = super::SolveApp::new(Default::default(), Default::default());
        super::open_at(&mut app, super::PAGE_GRAPHICS);
        let ctx = crate::fonts::test_context();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() },
                |ui| super::show(&mut app, ui.ctx()),
            );
            shapes = std::mem::take(&mut out.shapes);
            out.textures_delta.clear();
        }
        let mut previous = 0;
        for label in ["Coarse", "Normal", "Fine", "Ultra"] {
            let card = crate::scenario::rect_of(&format!("prefs:option:{label}")).expect("quality card");
            let facets = shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Path(path) = &shape.shape {
                        path.points.iter().all(|p| card.contains(*p)).then_some(path.points.len())
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0);
            assert!(facets > previous, "{label} must visibly add detail to the preceding quality: {facets} <= {previous}");
            previous = facets;
        }
    }

    #[test]
    fn moved_preferences_reset_only_on_their_new_page() {
        use super::*;
        let fields = [
            ("default_units", json!("ft"), PAGE_DESIGN),
            ("length_decimals", json!(5), PAGE_DESIGN),
            ("angle_decimals", json!(4), PAGE_DESIGN),
            ("theme", json!("light"), PAGE_INTERFACE),
            ("ui_scale", json!(1.25), PAGE_INTERFACE),
            ("ui_font", json!("test font"), PAGE_INTERFACE),
            ("layout", json!("compact"), PAGE_INTERFACE),
            ("ribbon_labels", json!(false), PAGE_INTERFACE),
            ("dialog_buttons", json!("top"), PAGE_INTERFACE),
            ("timeline_left", json!(true), PAGE_INTERFACE),
            ("timeline_under_browser", json!(true), PAGE_INTERFACE),
            ("history_sketches", json!("under_feature"), PAGE_INTERFACE),
            ("browser_float", json!(true), PAGE_INTERFACE),
            ("dialog_place", json!("left_column"), PAGE_INTERFACE),
            ("color_vision", json!("protan"), PAGE_INTERFACE),
            ("viewport_theme", json!("light"), PAGE_INTERFACE),
            ("view_confirm_corner", json!(true), PAGE_INTERFACE),
            ("nav", json!("blender"), PAGE_NAVIGATION),
            ("look_at_anim", json!(false), PAGE_NAVIGATION),
            ("selection_shift_add_only", json!(true), PAGE_SKETCH),
            ("suppress_sketch_snap", json!(ModifierCombo::NONE), PAGE_SKETCH),
            ("msaa", json!(8), PAGE_GRAPHICS),
        ];
        let defaults = serde_json::to_value(Prefs::default()).unwrap();
        for (field, value, owner) in fields {
            for page in 0..PAGES.len() {
                let mut data = defaults.clone();
                data[field] = value.clone();
                let mut prefs: Prefs = serde_json::from_value(data).unwrap();
                reset_preferences(&mut prefs, page);
                let result = serde_json::to_value(prefs).unwrap();
                assert_eq!(result[field], if page == owner { defaults[field].clone() } else { value.clone() }, "{field}, page {page}");
            }
        }
    }

    #[test]
    fn page_aliases_and_transient_defaults_follow_the_new_groups() {
        use super::*;
        for name in ["Sketch", "Selection", "Sketch & selection"] {
            assert_eq!(page_index(name), Some(PAGE_SKETCH));
        }
        assert_eq!(page_index("Assistant"), Some(PAGE_AI));
        assert_eq!(page_index("Shortcuts"), Some(PAGE_SHORTCUTS));
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.ui.agent_cursor.show = false;
        app.ui.pick_bodies = true;
        app.session.auto_project = false;
        defaults(&mut app, PAGE_GENERAL);
        assert!(!app.ui.agent_cursor.show);
        assert!(app.ui.pick_bodies);
        assert!(!app.session.auto_project);
        defaults(&mut app, PAGE_AI);
        let expected = SolveApp::new(Default::default(), Default::default()).ui.agent_cursor.show;
        assert_eq!(app.ui.agent_cursor.show, expected);
        defaults(&mut app, PAGE_SKETCH);
        assert!(!app.ui.pick_bodies);
        assert!(app.session.auto_project);
    }

    fn hole_preset_row(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name, "style": "counterbore", "extent": "distance",
            "diameter": "shaft_diameter + 0.2 mm", "depth": "0.5 in",
            "cb_diameter": "10 mm", "cb_depth": "3 mm", "cs_diameter": "10 mm",
            "cs_angle": "90 deg", "tip_angle": "118 deg", "tapped": true, "thread": "M5"
        })
    }

    #[test]
    fn hole_presets_corrupt_rows_and_limits_do_not_discard_other_preferences() {
        use serde_json::json;
        let good = hole_preset_row("Workshop");
        let mut oversized = hole_preset_row("Too long");
        oversized["diameter"] = json!("x".repeat(513));
        let mut duplicate = good.clone();
        duplicate["name"] = json!("WORKSHOP");
        let p = super::lenient(&json!({"length_decimals":4,"hole_presets":[null,good,oversized,duplicate,{"name":"incomplete"}]}));
        assert_eq!(p.length_decimals, 4);
        let rows = serde_json::to_value(&p.hole_presets).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["name"], "Workshop");
        assert_eq!(rows[0]["diameter"], "shaft_diameter + 0.2 mm");
        assert_eq!(rows[0]["depth"], "0.5 in");
        let corrupt = super::lenient(&json!({"length_decimals":3,"hole_presets":{"unexpected":true}}));
        assert_eq!(corrupt.length_decimals, 3);
        assert_eq!(serde_json::to_value(corrupt.hole_presets).unwrap(), json!([]));
        let many: Vec<_> = (0..100).map(|i| hole_preset_row(&format!("Preset {i}"))).collect();
        let capped = super::lenient(&json!({"hole_presets":many}));
        assert_eq!(serde_json::to_value(capped.hole_presets).unwrap().as_array().unwrap().len(), 64);
    }

    #[test]
    fn hole_presets_survive_application_preferences_roundtrip_and_page_resets() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let mut row = hole_preset_row("Fixture");
        row["thread_options"] = json!({"handedness":"left","full_length":false,"depth":"4 mm"});
        super::set(&mut app, &json!({"hole_presets":[row]})).unwrap();
        let expected = serde_json::to_value(&app.preferences.hole_presets).unwrap();
        assert_eq!(expected[0]["thread_options"]["handedness"], "left");
        for page in 0..super::PAGES.len() {
            super::reset_preferences(&mut app.preferences, page);
        }
        assert_eq!(serde_json::to_value(&app.preferences.hole_presets).unwrap(), expected);
        let saved = app.prefs();
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&saved);
        assert_eq!(serde_json::to_value(&other.preferences.hole_presets).unwrap(), expected);
        assert!(other.session.doc.features.is_empty());
    }

    #[test]
    fn hole_presets_keep_unknown_thread_options_for_safe_future_recall() {
        use serde_json::json;
        let mut row = hole_preset_row("Newer version");
        row["thread_options"] = json!({"starts":2});
        let p = super::lenient(&json!({"hole_presets":[row]}));
        let saved = serde_json::to_value(p.hole_presets).unwrap();
        assert_eq!(saved[0]["thread_options"]["starts"], 2, "unknown thread semantics must not silently become the defaults");
    }

    /// The interface font (#1290): a font that can be read, or empty for the built-in one; saved
    /// with the others and reset by Restore Defaults.
    #[test]
    fn ui_font_is_checked_saved_and_restored() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!(app.preferences.ui_font, "");
        let family = solidcraft_engine::sketch::DEFAULT_FONT_FAMILY;
        let out = super::set(&mut app, &json!({ "ui_font": family })).unwrap();
        assert_eq!(out["ui_font"], family);
        let e = super::set(&mut app, &json!({"ui_font": "No Such Font 1290"})).unwrap_err();
        assert!(e.contains("ui_font"), "{e}");
        assert_eq!(app.preferences.ui_font, family, "a refused font leaves the choice as it was");
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.ui_font, family);
        super::defaults(&mut app, super::PAGE_INTERFACE);
        assert_eq!(app.preferences.ui_font, "");
    }

    /// Theme files (#2255): a built-in one and an imported one turn on through `ui.prefs`, the
    /// next frame draws with their colours (dark following the base), an export gives the theme
    /// back complete, imported themes are saved with the preferences, Remove falls back to the
    /// base and Cancel takes an import back.
    #[test]
    fn system_theme_tile_export_uses_os_brightness_without_selecting_it() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.theme = "light".into();
        app.ui.dark = false;
        assert_eq!(theme_file(&app, "system", true)["base"], "dark");
        assert_eq!(theme_file(&app, "system", false)["base"], "light");
        assert_eq!(app.preferences.theme, "light");
        assert!(!app.ui.dark);
    }

    /// The 3D view in the other theme's colours (#4146): `ui.prefs` sets it, the window keeps its
    /// theme and the next frame draws the view from the other side, and it is saved with the
    /// preferences. With a theme file on, the view takes the built-in Light or Dark colours.
    #[test]
    fn the_3d_view_takes_the_light_or_dark_themes_colours() {
        use crate::theme::{ColorVision, Tokens};
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        for (theme, view, dark, view_dark) in
            [("light", "theme", false, false), ("dark", "light", true, false), ("dark", "dark", true, true), ("light", "dark", false, true)]
        {
            let out = h.call("ui.prefs", json!({"theme": theme, "viewport_theme": view}));
            assert_eq!((out["result"]["dark"].as_bool(), out["result"]["viewport_dark"].as_bool()), (Some(dark), Some(view_dark)), "{out}");
            h.frames(2);
            let t = Tokens::get();
            assert_eq!(t.panel, Tokens::themed(dark, ColorVision::Off).panel, "{theme}, {view}: the window");
            assert_eq!(t.viewport_top, Tokens::themed(view_dark, ColorVision::Off).viewport_top, "{theme}, {view}: the view");
        }

        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert_eq!((other.preferences.theme.as_str(), other.ui.dark, other.viewport_dark()), ("light", false, true));

        let out = h.call("ui.prefs", json!({"theme": "gruvbox-dark-hard", "viewport_theme": "light"}));
        assert_eq!((out["result"]["dark"].as_bool(), out["result"]["viewport_dark"].as_bool()), (Some(true), Some(false)), "{out}");
        h.frames(2);
        let hard = crate::theme_files::find("gruvbox-dark-hard", &[]).map(crate::theme_files::resolve).unwrap();
        assert_eq!((Tokens::get().panel, Tokens::get().viewport_top), (hard.panel, Tokens::light().viewport_top));

        // Asked for, an unknown value is refused; read from saved preferences, it is the default.
        assert!(h.call("ui.prefs", json!({"viewport_theme": "dim"}))["error"].is_string());
        let read: crate::theme::ViewportTheme = serde_json::from_value(json!("dim")).unwrap();
        assert_eq!(read, crate::theme::ViewportTheme::Theme);
    }

    /// The theme strip's Import… reads the theme file through `vfs`, like the other imports: in the
    /// web app a picked file is kept there, where `std::fs` can't read it (#7643). On the disk it
    /// is the same file; a folder scope active while it is read refuses a path outside it, which
    /// shows the read goes through `vfs`.
    #[test]
    fn theme_import_reads_through_vfs() {
        use serde_json::json;
        use solidcraft_engine::io::vfs;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"theme": "dark", "open": "Interface"}));
        h.frames(2);
        super::reveal_preference(&mut h, "dialog:Import…");
        let (root, out) = (h.dir.join("root"), h.dir.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let file = json!({"format": "solidcraft-theme/1", "name": "Paper", "base": "light", "colors": {"panel": "#fffdf0"}});
        for (path, inside) in [(out.join("Paper.json"), false), (root.join("Paper.json"), true)] {
            std::fs::write(&path, file.to_string()).unwrap();
            let answer = path.to_string_lossy().to_string();
            h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(Some(answer.clone())))));
            h.app.status = None;
            vfs::scoped(Some(vfs::PathScope::new([root.clone()])), || {
                h.step(&json!({"click":{"handle":"dialog:Import…"}})).unwrap();
                h.frames(4);
            });
            let status = h.app.status.clone().map(|(text, ..)| text);
            if inside {
                assert_eq!(status.as_deref(), Some("Theme imported: Paper"));
                assert_eq!(h.app.preferences.theme, "paper");
            } else {
                assert!(status.as_deref().is_some_and(|s| s.contains("outside the folders this agent may use")), "{status:?}");
                assert!(h.app.preferences.custom_themes.is_empty(), "nothing imported");
            }
        }
    }

    #[test]
    fn a_followed_theme_file_applies_reloads_keeps_the_last_good_theme_and_the_flag_is_not_saved() {
        use crate::theme::Tokens;
        use egui::Color32;
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.app.preferences.theme_follow, "", "off by default");
        let out = h.call("ui.prefs", json!({}));
        assert!(out["result"]["theme_followed"].is_null() && out["result"]["theme_follow_error"].is_null(), "{out}");

        let dir = std::env::temp_dir().join(format!("solidcraft-prefs-follow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("live.json");
        let path = file.to_string_lossy().to_string();
        let write = |panel: &str, extra: &str| {
            let text = format!(r##"{{"format":"solidcraft-theme/1","name":"Live","base":"light","colors":{{"panel":"{panel}"{extra}}}}}"##);
            std::fs::write(&file, text).unwrap();
        };
        write("#102030", "");
        let out = h.call("ui.prefs", json!({"theme_follow": path}));
        assert_eq!(
            (out["result"]["theme_follow"].as_str(), out["result"]["theme_followed"].as_str()),
            (Some(path.as_str()), Some(path.as_str())),
            "{out}"
        );
        assert!(out["result"]["theme_follow_error"].is_null(), "{out}");
        h.frames(2);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x10, 0x20, 0x30));
        assert!(!h.app.ui.dark, "the file's base");

        // Another tool rewrites it: reloaded within the poll interval, without a restart.
        write("#405060", r##","accent":"#aa3300""##);
        h.frames(70);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x40, 0x50, 0x60));

        // Broken or missing: the last good theme stays on and the reply says why.
        std::fs::write(&file, "{\"format\": \"solidcraft-theme/1\", \"na").unwrap();
        h.frames(70);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x40, 0x50, 0x60));
        let out = h.call("ui.prefs", json!({}));
        assert!(out["result"]["theme_follow_error"].as_str().is_some_and(|e| e.contains("not a theme file")), "{out}");
        std::fs::remove_file(&file).unwrap();
        let out = h.call("ui.prefs", json!({"theme_follow": path}));
        assert!(out["result"]["theme_follow_error"].as_str().is_some_and(|e| e.contains("can't read")), "{out}");

        // Choosing a theme, or "", stops following.
        let out = h.call("ui.prefs", json!({"theme": "dark"}));
        assert!(out["result"]["theme_followed"].is_null() && out["result"]["theme_follow"] == "", "{out}");
        h.frames(2);
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);

        // The launch flag follows a file for this run only: the saved preferences never name it.
        write("#708090", "");
        follow_theme_file_for_run(&mut h.app, &path).unwrap();
        h.frames(2);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x70, 0x80, 0x90));
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["theme_followed"].as_str(), Some(path.as_str()));
        assert_eq!(h.app.preferences.theme_follow, "");
        assert!(!h.app.prefs().contains("live.json"));
        h.call("ui.prefs", json!({"theme_follow": ""}));
        h.frames(2);
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);

        // Hostile values are refused; the browser has no files to follow.
        assert!(h.call("ui.prefs", json!({"theme_follow": 5}))["error"].is_string());
        assert!(h.call("ui.prefs", json!({"theme_follow": "a\nb"}))["error"].is_string());
        assert!(h.call("ui.prefs", json!({"theme_follow": "x".repeat(5000)}))["error"].is_string());
        let web = set_theme_follow(&mut h.app, &json!(path), false).unwrap_err();
        assert!(web.contains("not available in the browser"), "{web}");
        assert_eq!(h.app.preferences.theme_follow, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_files_import_apply_export_save_and_remove() {
        use crate::theme::{ColorVision, Tokens};
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let out = h.call("ui.prefs", json!({"theme": "gruvbox-dark-hard"}));
        assert_eq!(out["result"]["theme"], "gruvbox-dark-hard", "{out}");
        assert!(out["result"]["theme_ids"].as_array().is_some_and(|ids| ids.len() == 3 + crate::theme_files::builtin().len()), "{out}");
        h.frames(2);
        let hard = crate::theme_files::find("gruvbox-dark-hard", &[]).map(crate::theme_files::resolve).unwrap();
        assert!(h.app.ui.dark);
        assert_eq!(Tokens::get().panel, hard.panel);
        assert_eq!(
            crate::viewport::scene_palette(true, ColorVision::Off).background.0,
            solidcraft_engine::render::Rgb(hard.viewport_top.r(), hard.viewport_top.g(), hard.viewport_top.b())
        );

        let file = json!({"format": "solidcraft-theme/1", "name": "Paper", "base": "light", "colors": {"panel": "#fffdf0", "accent": "#aa3300"}, "borders":{"primary":{"width":2,"color":"#ffffff"},"tertiary":{"width":0,"color":"#000000"}}, "radii": {"radius_sm": 0, "control_radius": 0, "radius_xl": 16}});
        let out = h.call("ui.prefs", json!({"theme_import": file, "theme_export": true}));
        assert_eq!(out["result"]["theme_imported"], "paper", "{out}");
        assert_eq!((out["result"]["theme"].as_str(), out["result"]["dark"].as_bool()), (Some("paper"), Some(false)));
        let exported = &out["result"]["theme_file"];
        assert_eq!((exported["name"].as_str(), exported["base"].as_str()), (Some("Paper"), Some("light")));
        assert_eq!(exported["colors"]["panel"], "#fffdf0");
        assert_eq!(exported["radii"]["control_radius"], 0);
        assert_eq!(exported["radii"]["radius_xl"], 16);
        assert_eq!(exported["radii"]["radius_md"], 6);
        assert_eq!(exported["borders"]["primary"]["width"], 2.0);
        assert_eq!(exported["colors"]["text"].as_str().map(str::len), Some(7), "an export is complete: {exported}");
        h.frames(2);
        assert_eq!((Tokens::get().panel, Tokens::get().text), (egui::Color32::from_rgb(0xff, 0xfd, 0xf0), Tokens::light().text));
        assert_eq!((Tokens::get().control_radius, Tokens::get().radius_xl), (0, 16));
        let invalid = h.call(
            "ui.prefs",
            json!({"theme_import": {"format": "solidcraft-theme/1", "name": "Bad", "base": "dark", "radii": {"control_radius": -1}}}),
        );
        assert!(invalid["error"].as_str().is_some_and(|e| e.contains("radii.control_radius")));
        let out = h.call(
            "ui.prefs",
            json!({"theme_import": "{\"format\":\"solidcraft-theme/1\",\"name\":\"x\",\"base\":\"dark\",\"colors\":{\"nope\":\"#000000\"}}"}),
        );
        assert!(out["error"].as_str().is_some_and(|e| e.contains("nope")), "{out}");
        assert_eq!(h.app.preferences.theme, "paper", "a refused file changes nothing");
        assert!(h.call("ui.prefs", json!({"theme": "no-such-theme"}))["error"].is_string());

        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert_eq!((other.preferences.theme.as_str(), other.preferences.custom_themes.len()), ("paper", 1));
        assert_eq!(other.preferences.custom_themes[0].radii, h.app.preferences.custom_themes[0].radii);
        assert_eq!(other.preferences.custom_themes[0].borders, h.app.preferences.custom_themes[0].borders);

        let out = h.call("ui.prefs", json!({"theme_remove": "paper"}));
        assert_eq!((out["result"]["theme"].as_str(), out["result"]["dark"].as_bool()), (Some("light"), Some(false)), "{out}");
        assert!(h.app.preferences.custom_themes.is_empty());
        assert!(h.call("ui.prefs", json!({"theme_remove": "paper"}))["error"].is_string());

        h.call("ui.prefs", json!({"theme": "dark"}));
        h.call("ui.prefs", json!({"open": "Interface"}));
        h.call("ui.prefs", json!({"theme_import": file}));
        let out = h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(out["result"]["theme"], "dark", "{out}");
        assert!(h.app.preferences.custom_themes.is_empty(), "Cancel takes the import back");
        h.frames(2);
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);
        assert_eq!((Tokens::get().control_radius, Tokens::get().radius_xl), (6, 10));
    }

    /// A theme tile's Export… writes the theme file through `vfs`, like the other saves, so the
    /// web app hands it out as a download (#7192). On the disk it is the same file; a folder scope
    /// active while it is written refuses a path outside it, which shows the write goes through
    /// `vfs`.
    #[test]
    fn theme_export_writes_through_vfs() {
        use serde_json::json;
        use solidcraft_engine::io::vfs;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"theme": "dark", "open": "Interface"}));
        h.frames(2);
        super::reveal_preference(&mut h, "prefs:theme_tile:dark");
        let (root, out) = (h.dir.join("root"), h.dir.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        for (path, inside) in [(root.join("Dark.json"), true), (out.join("Dark.json"), false)] {
            let answer = path.to_string_lossy().to_string();
            h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(answer.clone())))));
            h.app.status = None;
            vfs::scoped(Some(vfs::PathScope::new([root.clone()])), || {
                h.step(&json!({"click":{"handle":"prefs:theme_tile:dark"},"button":"right"})).unwrap();
                h.step(&json!({"click":{"handle":"menu:theme_export"}})).unwrap();
                h.frames(4);
            });
            let status = h.app.status.clone().map(|(text, ..)| text);
            if inside {
                assert_eq!(status, None);
                let file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                assert_eq!(file, super::theme_file(&h.app, "dark", true));
            } else {
                assert!(status.as_deref().is_some_and(|s| s.contains("outside the folders this agent may use")), "{status:?}");
                assert!(!path.exists(), "nothing written outside");
            }
        }
    }

    /// Picking a font closes the font list, not the Preferences window's body: that folded the
    /// window to its title bar, and egui kept it folded between runs (#1800). A window folded by
    /// an earlier release unfolds when Preferences opens.
    #[test]
    fn picking_a_font_keeps_the_preferences_window_unfolded() {
        use egui::collapsing_header::CollapsingState;
        use serde_json::json;
        let body = egui::Id::new("sc_prefs").with("collapsing");
        let folded = |h: &crate::scenario::Harness| CollapsingState::load(&h.ctx, body).is_some_and(|s| !s.is_open());
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Interface"}));
        super::reveal_preference(&mut h, "prefs:ui_font");
        let combo = crate::scenario::handle_at("prefs:ui_font").unwrap();
        h.call("ui.click", json!({"x":combo.x,"y":combo.y}));
        let builtin = crate::scenario::handle_at("prefs:font_builtin").expect("the font list is open");
        h.call("ui.click", json!({"x":builtin.x,"y":builtin.y}));
        h.frames(2);
        assert!(!folded(&h), "picking a font must not fold the Preferences window");
        assert!(crate::scenario::handle_at("prefs:font_builtin").is_none(), "the font list closes");
        assert!(crate::scenario::rect_of("prefs:ok").is_some(), "the window's body is still drawn");
        h.call("ui.prefs", json!({"button":"cancel"}));

        // The state 0.14 and 0.15 saved: the window's body folded.
        let mut state = CollapsingState::load_with_default_open(&h.ctx, body, true);
        state.set_open(false);
        state.store(&h.ctx);
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.frames(2);
        assert!(!folded(&h), "a folded Preferences window unfolds when it opens");
        assert!(crate::scenario::rect_of("prefs:ok").is_some());
    }

    #[test]
    fn number_fields_take_a_decimal_point_or_a_decimal_comma() {
        // #1347
        assert_eq!(super::parse_number("2.5"), Some(2.5));
        assert_eq!(super::parse_number("2,5"), Some(2.5));
        assert_eq!(super::parse_number(" -0,25 "), Some(-0.25));
        assert_eq!(super::parse_number("\u{2212}1,5"), Some(-1.5));
        assert_eq!(super::parse_number("12"), Some(12.0));
        for bad in ["1.234,5", "1,2,3", ",", "abc", "", "inf", "NaN"] {
            assert_eq!(super::parse_number(bad), None, "{bad}");
        }
    }

    /// Display quality (#1287) is a preference like any other: set by name, checked, saved with
    /// the others, back to Normal with Restore Defaults.
    #[test]
    fn display_quality_is_set_checked_and_restored() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!(app.preferences.display_quality, "normal");
        let out = super::set(&mut app, &json!({"display_quality": "fine"})).unwrap();
        assert_eq!(out["display_quality"], "fine");
        assert!(super::set(&mut app, &json!({"display_quality": "silky"})).unwrap_err().contains("coarse, normal, fine or ultra"));
        assert_eq!(app.preferences.display_quality, "fine");
        assert_eq!(super::lenient(&json!({"display_quality": "ultra"})).display_quality, "ultra");
        super::defaults(&mut app, super::PAGE_GRAPHICS);
        assert_eq!(app.preferences.display_quality, "normal");
    }

    #[test]
    fn sketch_halo_is_off_by_default_saved_and_restored() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["sketch_halo"], false);
        assert_eq!(serde_json::to_value(super::lenient(&json!({}))).unwrap()["sketch_halo"], false);
        let saved = super::set(&mut app, &json!({"sketch_halo": true})).unwrap();
        assert_eq!(saved["sketch_halo"], true);
        assert_eq!(serde_json::to_value(super::lenient(&saved)).unwrap()["sketch_halo"], true);
        super::defaults(&mut app, super::PAGE_GRAPHICS);
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["sketch_halo"], false);
    }

    /// Supersampling (#1288): off by default, adaptive, set by factor and checked.
    #[test]
    fn supersampling_is_off_by_default_and_checked() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!((app.preferences.supersampling, app.preferences.supersampling_adaptive), (1.0, true));
        super::set(&mut app, &json!({"supersampling": 2.0})).unwrap();
        assert_eq!(app.preferences.supersampling, 2.0);
        assert!(super::set(&mut app, &json!({"supersampling": 3.0})).unwrap_err().contains("1 (off), 1.5 or 2"));
        super::set(&mut app, &json!({"supersampling": 1.5, "supersampling_adaptive": false})).unwrap();
        super::defaults(&mut app, super::PAGE_GRAPHICS);
        assert_eq!((app.preferences.supersampling, app.preferences.supersampling_adaptive), (1.0, true));
    }

    #[test]
    fn a_value_this_version_does_not_understand_keeps_the_other_preferences() {
        use serde_json::json;
        // A navigator, a field of the wrong type and a key from another release.
        let saved = json!({"nav": "solidworks", "navigator_style": "hologram", "ui_scale": "big", "zoom_reverse": true, "from_the_future": 3});
        let p = super::lenient(&saved);
        assert_eq!(p.nav, "solidworks");
        assert!(p.zoom_reverse);
        assert_eq!(p.navigator_style, super::NavigatorStyle::Axes);
        assert_eq!(p.ui_scale, super::Prefs::default().ui_scale);
        assert_eq!(super::lenient(&json!("garbage")), super::Prefs::default());
        // Whole-app load: the other saved state is kept too, and nothing panics.
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.load_prefs(&json!({"grid": false, "preferences": saved}).to_string());
        assert!(!app.ui.show_grid && app.preferences.nav == "solidworks" && app.preferences.zoom_reverse);
        for junk in ["", "{", "[]", "null", "{\"preferences\": 7, \"recent\": {}, \"shortcuts\": 1, \"toolbar\": \"x\"}"] {
            crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default()).load_prefs(junk);
        }
    }

    #[test]
    fn stored_prefs_fall_back_to_the_legacy_key() {
        let only_old = |k: &str| (k == super::LEGACY_STORAGE_KEY).then(|| "old".to_string());
        assert_eq!(super::stored(only_old).as_deref(), Some("old"));
        let both = |k: &str| Some(if k == super::STORAGE_KEY { "new" } else { "old" }.to_string());
        assert_eq!(super::stored(both).as_deref(), Some("new"));
        assert_eq!(super::stored(|_| None), None);
    }

    use super::*;
    use crate::navigation::orbit;

    /// Graphics › Window system (#3083): saved in `graphics.json` beside the other start-up
    /// settings, refused when unknown, taken back by Cancel; on Linux the pane says which one runs
    /// now and, for native Wayland, that some features are limited, with the link.
    #[test]
    fn the_window_system_is_saved_for_the_next_start() {
        use serde_json::json;
        let dir = std::env::temp_dir().join(format!("sc_window_system_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"msaa":4,"backend":"vulkan"}"#).unwrap();
        let saved = || -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default()).unwrap_or_default() };
        let mut h = crate::scenario::Harness::new();
        h.app.services.graphics_path = Some(path.to_string_lossy().to_string());
        h.app.services.window_system = Some("x11");
        assert_eq!(h.app.preferences.window_system, "auto");
        let out = h.call("ui.prefs", json!({"window_system": "wayland"}));
        assert_eq!(out["result"]["window_system"], "wayland", "{out}");
        assert_eq!((saved()["window_system"].as_str(), saved()["backend"].as_str()), (Some("wayland"), Some("vulkan")), "the other settings stay");
        assert!(h.call("ui.prefs", json!({"window_system": "mir"}))["error"].as_str().is_some_and(|e| e.contains("window_system")));
        assert_eq!(h.app.preferences.window_system, "wayland", "a refused value changes nothing");

        h.call("ui.prefs", json!({"open": "Graphics"}));
        h.frames(2);
        assert_eq!(crate::scenario::handle_at("prefs:window_system").is_some(), cfg!(target_os = "linux"), "the row is on Linux only");
        h.call("ui.prefs", json!({"window_system": "x11"}));
        let out = h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(out["result"]["window_system"], "wayland", "Cancel takes the change back");
        assert_eq!(saved()["window_system"], "wayland", "and the file with it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn graphics_card_picker_selects_named_cards_and_restores_automatic() {
        let dir = std::env::temp_dir().join(format!("solidcraft-graphics-picker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.services.graphics_path = Some(dir.join("graphics.json").to_string_lossy().into_owned());
        h.app.services.graphics_adapters = vec![("Discrete small".into(), "Discrete small (DiscreteGpu, Vulkan)".into())];
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:graphics_card");
        assert!(h.app.preferences.graphics_adapter.is_empty());
        h.step(&json!({"click":{"handle":"prefs:graphics_card"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:list:graphics_card:0"}})).unwrap();
        assert_eq!(h.app.preferences.graphics_adapter, "Discrete small");
        h.step(&json!({"click":{"handle":"prefs:graphics_card"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:list:graphics_card:auto"}})).unwrap();
        assert!(h.app.preferences.graphics_adapter.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn graphics_card_preference_persists_cancels_and_resets_without_changing_backend() {
        let dir = std::env::temp_dir().join(format!("solidcraft-graphics-card-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"backend":"vulkan","msaa":4}"#).unwrap();
        let services = crate::Services { graphics_path: Some(path.to_string_lossy().into()), ..Default::default() };
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), services);
        let snapshot = Snapshot::take(&app);
        set(&mut app, &json!({"graphics_adapter":"Discrete large"})).unwrap();
        let read = || serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read()["adapter"], "Discrete large");
        assert_eq!(read()["backend"], "vulkan");
        let saved = serde_json::to_value(&app.preferences).unwrap();
        assert_eq!(lenient(&saved).graphics_adapter, "Discrete large");
        snapshot.restore(&mut app);
        assert_eq!(read()["adapter"], "");
        set(&mut app, &json!({"graphics_adapter":"Discrete small"})).unwrap();
        defaults(&mut app, PAGE_GRAPHICS);
        assert_eq!(read()["adapter"], "");
        assert_eq!(read()["backend"], "vulkan");
        assert!(set(&mut app, &json!({"graphics_adapter":"x".repeat(1025)})).is_err());
        assert!(set(&mut app, &json!({"graphics_adapter":"card\nname"})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_msaa_check_keeps_the_default_unless_fallback_is_confirmed() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.msaa_supported = vec![1, 4, 8];
        for requested in [1, 4, 8] {
            app.preferences.msaa = requested;
            app.msaa_compatibility = false;
            check_msaa(&mut app, requested);
            assert_eq!(app.preferences.msaa, requested, "a passing check preserves the choice, including the 4x default");
            assert!(app.settings_reset.is_empty());
        }
        app.preferences.msaa = 4;
        app.msaa_compatibility = true;
        check_msaa(&mut app, 4);
        assert_eq!(app.preferences.msaa, 1, "Graphics must show the confirmed fallback so it can be changed back");
        assert!(app.msaa_supported.contains(&4), "the original choice stays available");
        assert!(app.msaa_compatibility, "Graphics explains the fallback");
        assert!(set(&mut app, &serde_json::json!({ "msaa": 4 })).is_ok());
        assert_eq!(app.preferences.msaa, 4);
    }

    #[test]
    fn unsupported_saved_msaa_is_reset_to_the_default_with_a_notice() {
        let dir = std::env::temp_dir().join(format!("sc_msaa_reset_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("graphics.json");
        let services = crate::Services { graphics_path: Some(path.to_string_lossy().to_string()), ..Default::default() };
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), services);
        // WebGPU's guarantee for the surface format: 8x saved, the device can only do 1x and 4x.
        app.msaa_supported = vec![1, 4];
        app.preferences.msaa = 8;
        std::fs::write(&path, r#"{"msaa":8,"backend":"vulkan"}"#).unwrap();
        check_msaa(&mut app, 8);
        assert_eq!(app.preferences.msaa, 4);
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default()).unwrap_or_default();
        assert_eq!(saved["msaa"], 4);
        assert_eq!(saved["backend"], "vulkan", "the backend preference is kept");
        assert_eq!(app.settings_reset, [SettingReset::Msaa { requested: 8, reset: 4, supported: vec![1, 4] }]);
        assert_eq!(
            app.settings_reset[0].message(),
            "Graphics › Anti-aliasing: 8× isn't supported by this graphics device; it is now 4× (supported: 1×, 4×)."
        );
        {
            let _de = crate::i18n::scope("de");
            assert_eq!(
                app.settings_reset[0].message(),
                "Grafik › Kantenglättung: 8× wird von diesem Grafikgerät nicht unterstützt; jetzt 4× (unterstützt: 1×, 4×)."
            );
        }
        // Below the default when the device can't do that either.
        app.settings_reset.clear();
        app.msaa_supported = vec![1, 2];
        check_msaa(&mut app, 8);
        assert_eq!(app.preferences.msaa, 2);
        // Supported, off, or no GPU: nothing changes (OpenGL's 1x policy isn't a reset either).
        for (supported, requested) in [(vec![1, 2, 4, 8], 8), (vec![1, 4], 1), (vec![], 8), (vec![1, 4], 4)] {
            app.settings_reset.clear();
            app.preferences.msaa = requested;
            app.msaa_supported = supported;
            check_msaa(&mut app, requested);
            assert!(app.settings_reset.is_empty());
            assert_eq!(app.preferences.msaa, requested);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn control_channel_rejects_msaa_the_device_cannot_do_and_bad_saved_msaa_loads_as_default() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.msaa_supported = vec![1, 4];
        assert!(set(&mut app, &serde_json::json!({ "msaa": 8 })).is_err());
        assert!(set(&mut app, &serde_json::json!({ "msaa": 1 })).is_ok());
        app.load_prefs(&serde_json::json!({ "preferences": { "msaa": 16 } }).to_string());
        assert_eq!(app.preferences.msaa, Prefs::default().msaa);
    }

    /// "Let AI agents connect" is on for a new install and for preferences saved before it existed;
    /// an explicit off stays off.
    #[test]
    fn agent_control_is_on_unless_saved_off() {
        assert!(Prefs::default().agent_control);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.preferences.agent_control = false;
        app.load_prefs(&serde_json::json!({ "preferences": { "msaa": 4 } }).to_string());
        assert!(app.preferences.agent_control);
        app.load_prefs(&serde_json::json!({ "preferences": { "agent_control": false } }).to_string());
        assert!(!app.preferences.agent_control);
    }

    #[test]
    fn length_fields_read_bare_numbers_in_design_units_and_honour_explicit_units() {
        let inch = unit_scale("in").0;
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        assert!(close(parse_length_mm("2", inch), 50.8));
        assert!(close(parse_length_mm(" 10 mm ", inch), 10.0));
        assert!(close(parse_length_mm("0.5 in", 1.0), 12.7));
        assert!(close(parse_length_mm("1 in + 5 mm", unit_scale("cm").0), 30.4));
        assert!(close(parse_length_mm("3", unit_scale("cm").0), 30.0));
        assert!(close(parse_length_mm("3", 1.0), 3.0));
        assert_eq!(parse_length_mm("10 deg", inch), None);
        assert_eq!(parse_length_mm("abc", inch), None);
        assert_eq!(parse_length_mm("", inch), None);
        assert_eq!(parse_length_mm("1e400", 1.0), None);
    }

    #[test]
    fn navigator_picker_preserves_axes_and_follows_preferences_transactions() {
        let old: Prefs = serde_json::from_value(json!({"nav":"solidworks"})).unwrap();
        assert_eq!(old.navigator_style, NavigatorStyle::Axes);
        let mut h = crate::scenario::Harness::new();
        assert!(crate::scenario::handle_at("axis_view:z").is_some());
        assert!(crate::scenario::handle_at("navigation_cube:front").is_none());
        let camera = h.app.cam;
        let design = h.app.session.doc.to_json();
        let nav = h.app.preferences.nav.clone();
        h.call("ui.prefs", json!({"open":"Navigation"}));
        super::reveal_preference(&mut h, "prefs:navigator");
        let picker = crate::scenario::rect_of("prefs:navigator").unwrap().center();
        h.call("ui.click", json!({"x":picker.x,"y":picker.y}));
        let cube = crate::scenario::rect_of("prefs:navigator:Cube").unwrap().center();
        assert!(crate::scenario::rect_of("prefs:navigator:Signed axes").is_some());
        h.call("ui.click", json!({"x":cube.x,"y":cube.y}));
        assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Cube);
        assert!(crate::scenario::handle_at("navigation_cube:front").is_some());
        assert_eq!(h.app.preferences.nav, nav);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.doc.to_json(), design);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.navigator_style, NavigatorStyle::Cube);
        let before = h.app.preferences.clone();
        assert!(control(&mut h.app, &json!({"navigator_style":"unknown", "nav":"solidworks"})).is_err());
        assert_eq!(h.app.preferences, before, "invalid settings must fail atomically");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Axes);
        assert!(crate::scenario::handle_at("navigation_cube:front").is_none());
        h.call("ui.prefs", json!({"open":"Navigation","navigator_style":"cube"}));
        h.call("ui.prefs", json!({"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Axes);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Cube);
        h.call("ui.prefs", json!({"navigator_style":"axes", "look_at_anim":false}));
        h.call("ui.view", json!({"view":"front"}));
        let axis = crate::scenario::handle_at("axis_view:-y").unwrap();
        h.call("ui.click", json!({"x":axis.x,"y":axis.y}));
        assert!(h.app.cam.back().dist(Vec3::Y) < 1e-6, "original axes retain click-again reversal");
        assert_eq!(h.app.session.doc.to_json(), design);
    }

    /// The texts one frame paints.
    fn painted_texts(h: &mut crate::scenario::Harness) -> Vec<String> {
        let output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(h.ctx.content_rect()), focused: true, ..Default::default() }, |ui| {
            h.app.logic(ui.ctx());
            h.app.ui(ui);
        });
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Origin plane names (#6491): Top, Front and Right are names shown in the browser, the
    /// dialogs' chips and the plane choices; the planes' ids, the commands and the design keep XY,
    /// XZ and YZ. Saved, refused when unknown, and taken back by Cancel and Restore Defaults.
    #[test]
    fn origin_plane_names_change_only_the_names_shown() {
        use OriginPlaneNames::{TopFrontRight, Xyz};
        let old: Prefs = serde_json::from_value(json!({"nav":"solidworks"})).unwrap();
        assert_eq!(old.origin_plane_names, Xyz, "older preferences keep XY, XZ and YZ");
        assert_eq!(lenient(&json!({"origin_plane_names":"hologram","zoom_reverse":true})).origin_plane_names, Xyz);
        assert_eq!(OriginPlaneNames::PLANES.map(|p| Xyz.label(p)), ["XY", "XZ", "YZ"]);
        assert_eq!(OriginPlaneNames::PLANES.map(|p| TopFrontRight.label(p)), ["Top", "Front", "Right"]);
        assert_eq!(TopFrontRight.label("Plane1"), "Plane1", "construction planes keep their own names");

        let mut h = crate::scenario::Harness::new();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"chevron":"Origin"}})).unwrap();
        let rows = || crate::browser::drawn_rows().into_iter().map(|(label, ..)| label).collect::<Vec<_>>();
        assert!(OriginPlaneNames::PLANES.iter().all(|p| rows().iter().any(|r| r == p)), "{:?}", rows());
        let design = h.app.session.doc.to_json();
        h.call("ui.prefs", json!({"open":"Interface"}));
        super::reveal_preference(&mut h, "prefs:origin_plane_names:top_front_right");
        h.step(&json!({"click":{"handle":"prefs:origin_plane_names:top_front_right"}})).unwrap();
        assert_eq!(h.app.preferences.origin_plane_names, TopFrontRight);
        h.call("ui.prefs", json!({"button":"ok"}));
        h.frames(2);
        for (id, name) in [("XY", "Top"), ("XZ", "Front"), ("YZ", "Right")] {
            assert!(rows().iter().any(|r| r == name) && !rows().iter().any(|r| r == id), "{:?}", rows());
        }
        assert_eq!(h.app.session.doc.to_json(), design, "a name shown changes nothing in the design");

        // Picked from its row, the plane is named in the dialog's chip and runs by its id.
        h.call("ui.start", json!({"command":"construct.plane.offset"}));
        h.step(&json!({"click":{"browser":"Front"}})).unwrap();
        let picked = h.app.dialog.as_ref().and_then(|d| d.inputs.first()).map(|i| i.items.clone());
        assert_eq!(picked, Some(vec![solidcraft_engine::Sel::Plane { name: "XZ".into() }]));
        assert!(painted_texts(&mut h).iter().any(|t| t == "Front"), "the chip shows the plane's name");
        h.step(&json!({"widget":"OK"})).unwrap();
        h.frames(2);
        let made = h.app.session.doc.features.last().map(|f| serde_json::to_value(&f.kind).unwrap_or_default()).unwrap_or_default();
        assert!(made.to_string().contains("\"XZ\""), "the command took the id: {made}");
        // The sketch plane choices of Redefine Sketch Plane: by name, and they run by id.
        crate::browser::redefine_sketch(&mut h.app, sketch);
        h.frames(2);
        let texts = painted_texts(&mut h);
        assert!(["Use Top plane", "Use Front plane", "Use Right plane"].iter().all(|t| texts.iter().any(|x| x == t)), "{texts:?}");
        assert!(!texts.iter().any(|t| t == "Use XY plane"));
        crate::sketch_attachment_ui::clear(&mut h.app);

        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.origin_plane_names, TopFrontRight, "the choice is saved");
        let before = h.app.preferences.clone();
        assert!(control(&mut h.app, &json!({"origin_plane_names":"top", "nav":"solidworks"})).is_err());
        assert_eq!(h.app.preferences, before, "an unknown naming fails atomically");
        h.call("ui.prefs", json!({"open":"Interface","origin_plane_names":"xyz"}));
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.origin_plane_names, TopFrontRight, "Cancel takes it back");
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert_eq!(h.app.preferences.origin_plane_names, Xyz, "Restore Defaults: XY, XZ and YZ");
        h.frames(2);
        assert!(OriginPlaneNames::PLANES.iter().all(|p| rows().iter().any(|r| r == p)), "{:?}", rows());
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.origin_plane_names, TopFrontRight);
    }

    #[test]
    fn history_sketches_defaults_in_order_and_follows_preferences_transactions() {
        let old: Prefs = serde_json::from_value(json!({"timeline_left": true})).unwrap();
        assert_eq!(old.history_sketches, HistorySketches::InOrder, "designs and settings from before keep the creation order");
        assert_eq!(lenient(&json!({"history_sketches": "sideways"})).history_sketches, HistorySketches::InOrder);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"timeline_left": true, "timeline_under_browser": true}));
        let design = h.app.session.doc.to_json();
        h.call("ui.prefs", json!({"open": "Interface"}));
        let row = h.app.tr("prefs.history_sketches").to_string();
        let under = h.app.tr("prefs.history_sketches_under_feature").to_string();
        let handle = format!("dialog:{row}:{under}");
        super::reveal_preference(&mut h, &handle);
        let at = crate::scenario::handle_at(&handle).unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        assert_eq!(h.app.preferences.history_sketches, HistorySketches::UnderFeature);
        assert_eq!(h.app.session.doc.to_json(), design, "the preference changes only the History");
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(h.app.preferences.history_sketches, HistorySketches::InOrder);

        let reply = h.call("ui.prefs", json!({"history_sketches": "under_feature"}));
        assert_eq!(reply["result"]["history_sketches"], "under_feature");
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.history_sketches, HistorySketches::UnderFeature);
        let before = h.app.preferences.clone();
        assert!(control(&mut h.app, &json!({"history_sketches": "sideways"})).is_err());
        assert_eq!(h.app.preferences, before, "an unknown value changes nothing");
        h.call("ui.prefs", json!({"open": "Interface"}));
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(h.app.preferences.history_sketches, HistorySketches::InOrder);
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(h.app.preferences.history_sketches, HistorySketches::UnderFeature);
    }

    #[test]
    fn dialog_buttons_default_bottom_and_follow_preferences_transactions() {
        use crate::ui_kit::DialogButtons;
        let old: Prefs = serde_json::from_value(json!({"ribbon_labels": false})).unwrap();
        assert_eq!(old.dialog_buttons, DialogButtons::Bottom, "settings from before keep the footer");
        assert_eq!(lenient(&json!({"dialog_buttons": "sideways"})).dialog_buttons, DialogButtons::Bottom);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open": "Interface"}));
        super::reveal_preference(&mut h, "prefs:dialog_buttons:top");
        h.step(&json!({"click": {"handle": "prefs:dialog_buttons:top"}})).unwrap();
        assert_eq!(h.app.preferences.dialog_buttons, DialogButtons::Top);
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(h.app.preferences.dialog_buttons, DialogButtons::Bottom);

        let reply = h.call("ui.prefs", json!({"dialog_buttons": "top"}));
        assert_eq!(reply["result"]["dialog_buttons"], "top");
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.dialog_buttons, DialogButtons::Top);
        let before = h.app.preferences.clone();
        assert!(control(&mut h.app, &json!({"dialog_buttons": "sideways"})).is_err());
        assert_eq!(h.app.preferences, before, "an unknown value changes nothing");
        h.call("ui.prefs", json!({"open": "Interface"}));
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(h.app.preferences.dialog_buttons, DialogButtons::Bottom);
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(h.app.preferences.dialog_buttons, DialogButtons::Top);
    }

    #[test]
    fn free_orbit_defaults_off_and_follows_preferences_transactions() {
        let old: Prefs = serde_json::from_value(json!({"nav":"solidworks"})).unwrap();
        assert!(!old.free_orbit);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Navigation"}));
        h.step(&json!({"click":{"handle":"prefs:free_orbit"}})).unwrap();
        assert!(h.app.preferences.free_orbit);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.free_orbit);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.free_orbit);
        h.call("ui.prefs", json!({"open":"Navigation","free_orbit":true,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.free_orbit);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.free_orbit);
    }

    #[test]
    fn radial_context_menu_defaults_off_and_follows_preferences_transactions() {
        assert!(!Prefs::default().radial_context_menu);
        let old: Prefs = serde_json::from_value(json!({"nav":"solidworks","zoom_reverse":true})).unwrap();
        assert!(!old.radial_context_menu && old.zoom_reverse && old.nav == "solidworks");
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Navigation"}));
        for _ in 0..16 {
            let page = crate::scenario::rect_of("dialog:page_body").expect("Navigation page body");
            if h.widget("Radial context menu").is_some_and(|[x, y]| page.contains(egui::pos2(x as f32, y as f32))) {
                break;
            }
            h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-100}));
            h.frames(20);
        }
        h.step(&json!({"widget":"Radial context menu"})).unwrap();
        assert!(h.app.preferences.radial_context_menu, "the actual checkbox enables radial menus");
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.radial_context_menu, "enabled preference survives save/load");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.radial_context_menu, "Cancel restores the default-off setting");
        h.call("ui.prefs", json!({"open":"Navigation","radial_context_menu":true}));
        h.call("ui.prefs", json!({"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.radial_context_menu, "Navigation defaults disable the ring");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.radial_context_menu, "Cancel restores the last Apply");
    }

    #[test]
    fn cosmetic_threads_default_off_and_follow_preferences_transactions() {
        assert!(!Prefs::default().cosmetic_threads);
        let old: Prefs = serde_json::from_value(json!({"theme":"light","msaa":4})).unwrap();
        assert!(!old.cosmetic_threads && old.theme == "light" && old.msaa == 4);
        let malformed = lenient(&json!({"cosmetic_threads":"yes","theme":"light"}));
        assert!(!malformed.cosmetic_threads && malformed.theme == "light");
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:cosmetic_threads");
        let checkbox = crate::scenario::handle_at("prefs:cosmetic_threads").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(h.app.preferences.cosmetic_threads, "the actual checkbox enables the optional markings");
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.cosmetic_threads, "the preference survives saved application settings");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.cosmetic_threads);
        h.call("ui.prefs", json!({"open":"Graphics","cosmetic_threads":true,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.cosmetic_threads, "Graphics defaults restore the default-off policy");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.cosmetic_threads, "Cancel restores the last Apply");
        h.call("ui.prefs", json!({"cosmetic_threads":false}));
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.cosmetic_threads, "an explicitly disabled value also survives reload");
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(h.app.session.undo.is_empty());
    }

    #[test]
    fn tool_modifier_prefs_default_classic_and_persist() {
        let d = Prefs::default();
        assert_eq!(d.suppress_sketch_snap, ModifierCombo::CTRL_ALT);
        assert!(d.manipulator_free_alt);
        assert!(!d.selection_shift_add_only);
        let old: Prefs = serde_json::from_value(json!({"nav": "solidworks"})).unwrap();
        assert_eq!(old.suppress_sketch_snap, ModifierCombo::CTRL_ALT);
        assert!(!old.selection_shift_add_only);
        // Classic default: Ctrl/Cmd or Alt each suppress (`command` is primary on all hosts).
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        let alt = Modifiers { alt: true, ..Default::default() };
        let shift = Modifiers { shift: true, ..Default::default() };
        assert!(suppress_sketch_snap_held(&app, ctrl));
        assert!(suppress_sketch_snap_held(&app, alt));
        assert!(!suppress_sketch_snap_held(&app, shift));
        control(
            &mut app,
            &json!({"open": "Sketch", "suppress_sketch_snap": {"shift": false, "ctrl": true, "alt": false}, "manipulator_free_alt": false}),
        )
        .unwrap();
        control(&mut app, &json!({"button": "apply"})).unwrap();
        control(&mut app, &json!({"open": "Selection", "selection_shift_add_only": true})).unwrap();
        control(&mut app, &json!({"button": "apply"})).unwrap();
        assert_eq!(app.preferences.suppress_sketch_snap, ModifierCombo::CTRL);
        assert!(!app.preferences.manipulator_free_alt);
        assert!(app.preferences.selection_shift_add_only);
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.suppress_sketch_snap, ModifierCombo::CTRL);
        assert!(!other.preferences.manipulator_free_alt);
        assert!(other.preferences.selection_shift_add_only);
        control(&mut app, &json!({"open": "Sketch", "button": "defaults"})).unwrap();
        assert_eq!(app.preferences.suppress_sketch_snap, ModifierCombo::CTRL_ALT);
        assert!(app.preferences.manipulator_free_alt);
        assert!(!app.preferences.selection_shift_add_only, "the merged Sketch & selection page resets both sets of modifiers");
        control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert_eq!(app.preferences.suppress_sketch_snap, ModifierCombo::CTRL);
    }

    #[test]
    fn wheel_smoothing_persists_and_follows_dialog_buttons() {
        assert!(serde_json::from_value::<Prefs>(json!({})).unwrap().smooth_wheel_zoom);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open":"Navigation", "smooth_wheel_zoom":false})).unwrap();
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(app.preferences.smooth_wheel_zoom);
        control(&mut app, &json!({"open":"Navigation", "smooth_wheel_zoom":false})).unwrap();
        control(&mut app, &json!({"button":"apply"})).unwrap();
        control(&mut app, &json!({"smooth_wheel_zoom":true, "button":"cancel"})).unwrap();
        assert!(!app.preferences.smooth_wheel_zoom);
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert!(!other.preferences.smooth_wheel_zoom);
        control(&mut app, &json!({"open":"Navigation", "button":"defaults"})).unwrap();
        assert!(app.preferences.smooth_wheel_zoom);
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(!app.preferences.smooth_wheel_zoom);
    }

    #[test]
    fn touchpad_preset_persists_and_obeys_preference_transactions() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open":"Navigation", "nav":"touchpad"})).unwrap();
        assert_eq!(app.preferences.nav, "touchpad");
        assert!(app.preferences.trackpad_pan, "choosing Touchpad must not overwrite the saved scroll preference");
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        control(&mut app, &json!({"open":"Navigation", "nav":"touchpad", "button":"apply"})).unwrap();
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.nav, "touchpad");
        control(&mut app, &json!({"button":"defaults"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert_eq!(app.preferences.nav, "touchpad");
    }

    #[test]
    fn openscad_preset_maps_navigation_and_persists() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        set(&mut app, &json!({"nav": OPENSCAD_NAV})).unwrap();
        for button in [NavButton::Middle, NavButton::Right] {
            assert_eq!(nav_mode(&app, button, Modifiers::default()), Some(NavMode::Pan));
            assert_eq!(nav_mode(&app, button, Modifiers { shift: true, ..Default::default() }), Some(NavMode::Zoom));
        }
        assert_eq!(nav_mode(&app, NavButton::Left, Modifiers::default()), Some(NavMode::Orbit));
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.nav, OPENSCAD_NAV);
    }

    #[test]
    fn pan_reverse_x_defaults_off_persists_and_flips_the_sign() {
        assert!(!serde_json::from_value::<Prefs>(json!({})).unwrap().pan_reverse_x);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert_eq!(pan_sign_x(&app), 1.0);
        set(&mut app, &json!({"pan_reverse_x": true})).unwrap();
        assert_eq!(pan_sign_x(&app), -1.0);
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert!(other.preferences.pan_reverse_x);
    }

    #[test]
    fn trackpad_pan_defaults_on_persists_and_follows_dialog_buttons() {
        assert!(serde_json::from_value::<Prefs>(json!({})).unwrap().trackpad_pan);
        assert!(Prefs::default().trackpad_pan);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open":"Navigation", "trackpad_pan":false})).unwrap();
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(app.preferences.trackpad_pan);
        control(&mut app, &json!({"open":"Navigation", "trackpad_pan":false})).unwrap();
        control(&mut app, &json!({"button":"apply"})).unwrap();
        assert!(!app.preferences.trackpad_pan);
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert!(!other.preferences.trackpad_pan);
        control(&mut app, &json!({"open":"Navigation", "button":"defaults"})).unwrap();
        assert!(app.preferences.trackpad_pan);
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(!app.preferences.trackpad_pan);
    }

    #[test]
    fn blender_preset_moves_the_camera_with_the_middle_button_only() {
        // #1090: orbit, pan and zoom all on the middle button; no left or right drag binding, so
        // those stay selection and the context menu (Ctrl+left on empty space orbits in every
        // preset, outside the bindings).
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        set(&mut app, &json!({"nav": "blender"})).unwrap();
        let none = Modifiers::default();
        let shift = Modifiers { shift: true, ..Default::default() };
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(nav_mode(&app, NavButton::Middle, none), Some(NavMode::Orbit));
        assert_eq!(nav_mode(&app, NavButton::Middle, shift), Some(NavMode::Pan));
        assert_eq!(nav_mode(&app, NavButton::Middle, ctrl), Some(NavMode::Zoom));
        for mods in [none, shift, ctrl] {
            assert_eq!(nav_mode(&app, NavButton::Left, mods), None);
            assert_eq!(nav_mode(&app, NavButton::Right, mods), None);
        }
        assert_eq!(zoom_sign(&app), 1.0, "the wheel turns the usual way");
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.nav, "blender");
    }

    #[test]
    fn presets_map_the_mouse_and_prefs_round_trip() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let shift = Modifiers { shift: true, ..Default::default() };
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        let none = Modifiers::default();
        assert!(matches!(nav_mode(&app, NavButton::Middle, none), Some(NavMode::Pan)));
        assert!(matches!(nav_mode(&app, NavButton::Middle, shift), Some(NavMode::Orbit)));
        set(&mut app, &json!({"nav": "solidworks"})).unwrap();
        assert!(matches!(nav_mode(&app, NavButton::Middle, none), Some(NavMode::Orbit)));
        assert!(matches!(nav_mode(&app, NavButton::Middle, ctrl), Some(NavMode::Pan)));
        set(&mut app, &json!({"nav": "inventor", "zoom_reverse": true})).unwrap();
        assert!(matches!(nav_mode(&app, NavButton::Middle, ctrl), Some(NavMode::Zoom)));
        assert_eq!(zoom_sign(&app), -1.0);
        assert!(matches!(nav_mode(&app, NavButton::Right, none), Some(NavMode::Orbit)), "right drag orbits everywhere");
        assert_eq!(nav_mode(&app, NavButton::Left, none), None, "left drag selects");
        let shift_alt = Modifiers { shift: true, alt: true, ..Default::default() };
        assert_eq!(nav_mode(&app, NavButton::Left, shift_alt), None, "only Alias navigates with the left button");
        set(&mut app, &json!({"nav": "alias"})).unwrap();
        assert_eq!(nav_mode(&app, NavButton::Left, shift_alt), Some(NavMode::Orbit));
        assert_eq!(nav_mode(&app, NavButton::Middle, shift_alt), Some(NavMode::Pan));
        assert_eq!(nav_mode(&app, NavButton::Right, shift_alt), Some(NavMode::Zoom));
        assert_eq!(nav_mode(&app, NavButton::Left, none), None, "a plain left drag still selects");
        assert_eq!(nav_mode(&app, NavButton::Left, shift), None);
        assert_eq!(nav_mode(&app, NavButton::Right, none), Some(NavMode::Orbit));
        assert!(set(&mut app, &json!({"nav": "no-such-preset"})).is_err());
        assert!(set(&mut app, &json!({"default_units": "furlong"})).is_err());
        assert!(set(&mut app, &json!({"nope": 1})).is_err());
        // Kept between runs.
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences, app.preferences);
        assert_eq!(format_length(25.4, "in", 3), "1.000 in");
    }

    /// The dialog: changes apply at once, Cancel puts back what was there when it opened, Apply
    /// keeps them, Restore Defaults resets one page, and the shortcuts menus open its page.
    #[test]
    fn preferences_dialog_cancel_apply_and_defaults() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open": "general"})).unwrap();
        assert!(app.prefs_window.open && app.prefs_window.section == 0);
        control(&mut app, &json!({"theme": "light", "ui_scale": 1.25, "zoom_to_cursor": false})).unwrap();
        assert!(!app.ui.dark, "light applies at once");
        app.keymap.custom.insert("solid.extrude".into(), "Shift+E".into());
        let v = control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert_eq!(v["dialog"], serde_json::Value::Null);
        assert!(app.ui.dark && app.preferences.theme == "dark" && app.preferences.ui_scale == 1.0 && app.preferences.zoom_to_cursor);
        assert!(app.keymap.custom.is_empty(), "a rebinding made while the dialog was open is undone too");

        control(&mut app, &json!({"open": "Navigation", "nav": "alias", "look_at_anim": false})).unwrap();
        control(&mut app, &json!({"button": "apply"})).unwrap();
        control(&mut app, &json!({"nav": "tinkercad"})).unwrap();
        control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert_eq!(app.preferences.nav, "alias", "Cancel goes back to the last Apply");
        let to = {
            let mut c = app.cam;
            c.set_view(solidcraft_engine::render::StandardView::Top);
            c
        };
        app.animate_to(to);
        assert!(app.cam_anim.is_none() && app.cam == to, "no animation: the view jumps");

        control(&mut app, &json!({"open": "Navigation"})).unwrap();
        control(&mut app, &json!({"button": "defaults"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        assert!(app.preferences.look_at_anim);
        control(&mut app, &json!({"button": "ok"})).unwrap();
        assert!(!app.prefs_window.open);

        // File and Help › Keyboard Shortcuts open the same dialog at its page.
        app.keymap.open = true;
        let ctx = crate::fonts::test_context();
        let mut out = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        out.textures_delta.clear();
        assert!(app.prefs_window.open && app.prefs_window.section == PAGE_SHORTCUTS && !app.keymap.open);
        assert!(control(&mut app, &json!({"open": "NoSuch"})).is_err());
        assert!(set(&mut app, &json!({"up_axis": "x"})).is_err());
        assert!(set(&mut app, &json!({"theme": "sepia"})).is_err());
        assert!(set(&mut app, &json!({"language": "xx"})).is_err());
    }

    #[test]
    fn language_preference_switches_and_persists() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert_eq!(app.preferences.language, "en");
        assert_eq!(app.tr("ui.preferences"), "Preferences");
        assert_eq!(app.tr("ui.extrude"), "Extrude");

        // Switch language to Japanese
        set(&mut app, &json!({"language": "ja", "timeline_left": true})).unwrap();
        app.preferences.timeline_left_browser_collapsed = true;
        assert_eq!(app.preferences.language, "ja");
        assert!(app.preferences.timeline_left);
        assert_eq!(app.tr("ui.preferences"), "環境設定");
        assert_eq!(app.tr("ui.extrude"), "押し出し");

        // Preferences round-trip
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.language, "ja");
        assert!(other.preferences.timeline_left);
        assert!(other.preferences.timeline_left_browser_collapsed);
        assert_eq!(other.tr("ui.preferences"), "環境設定");
    }

    /// A STEP file opened Z-up is offered to be stood up: a quarter turn about X, undoable.
    #[test]
    fn a_y_up_step_file_can_be_stood_up() {
        let dir = std::env::temp_dir().join(format!("solidcraft-standup-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tall_in_y.step").to_string_lossy().to_string();
        let mut src = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        src.run("solid.box", json!({"length": 10, "width": 40, "height": 5})).unwrap();
        src.run("file.export", json!({"path": path})).unwrap();

        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.open_path(&path);
        assert!(app.stand_up.is_none(), "SolidCraft's own exports already have Z up");
        // Another originating system keeps the offer.
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'")).unwrap();
        app.open_path(&path);
        let offered = app.stand_up.clone().unwrap_or_default();
        assert_eq!(offered.len(), 1, "the file's body is offered");
        let size = |app: &SolveApp| solidcraft_engine::view::bounds(&app.session);
        assert!((size(&app).max.y - size(&app).min.y - 40.0).abs() < 1e-6);
        control(&mut app, &json!({"stand_up": true})).unwrap();
        let b = size(&app);
        assert!((b.max.z - b.min.z - 40.0).abs() < 1e-6, "Y became Z: {b:?}");
        assert!(app.stand_up.is_none());
        app.run("edit.undo", json!({})).unwrap();
        assert!((size(&app).max.y - size(&app).min.y - 40.0).abs() < 1e-6, "one undo step");

        // Not offered when turned off, or with Y up as the default orientation.
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.offer_stand_up = false;
        app.open_path(&path);
        assert!(app.stand_up.is_none());
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.up_axis = "y".into();
        app.open_path(&path);
        assert!(app.stand_up.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Orbiting about the world origin keeps the origin where it was drawn, even when Pan has
    /// moved it off to the side.
    #[test]
    fn orbiting_around_the_origin_keeps_it_still() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 40, "width": 40, "height": 40})).unwrap();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.fit_view();
        assert_eq!(app.preferences.orbit_around, OrbitAround::ViewCentre, "the view centre stays the default");
        app.preferences.orbit_around = OrbitAround::Origin;
        app.cam.pan(60.0, -25.0, 600.0);
        let was = (app.cam.yaw, app.cam.pitch);
        let before = crate::viewport::projection(&app, rect).to_screen(Vec3::ZERO).unwrap();
        orbit(&mut app, rect, 40.0, 25.0, None);
        let after = crate::viewport::projection(&app, rect).to_screen(Vec3::ZERO).unwrap();
        assert!(after.distance(before) < 2.0, "{before:?} -> {after:?}");
        assert_ne!((app.cam.yaw, app.cam.pitch), was, "the view must turn");
    }

    /// Preferences saved before the origin option had `orbit_cursor`: that choice survives, and
    /// the control channel still accepts the old key.
    #[test]
    fn saved_cursor_preference_migrates_to_the_origin_option() {
        assert_eq!(lenient(&json!({"orbit_cursor": true})).orbit_around, OrbitAround::Cursor);
        assert_eq!(lenient(&json!({"orbit_cursor": false})).orbit_around, OrbitAround::ViewCentre);
        assert_eq!(lenient(&json!({})).orbit_around, OrbitAround::ViewCentre);
        assert_eq!(lenient(&json!({"orbit_around": "origin"})).orbit_around, OrbitAround::Origin);

        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        set(&mut app, &json!({"orbit_around": "origin"})).unwrap();
        assert_eq!(app.preferences.orbit_around, OrbitAround::Origin);
        set(&mut app, &json!({"orbit_cursor": true})).unwrap();
        assert_eq!(app.preferences.orbit_around, OrbitAround::Cursor, "the old key still moves the choice");
        assert!(!app.preferences.orbit_cursor, "and is folded away");
    }

    #[test]
    fn the_agent_chat_limit_is_off_unless_chosen() {
        assert_eq!(Prefs::default().agent_chat_turn_limit, 0);
        assert_eq!(migrate(lenient(&json!({"agent_chat_turn_limit": 24}))).agent_chat_turn_limit, 0, "the old default reads as no limit");
        assert_eq!(lenient(&json!({"agent_chat_turn_limit": 10})).agent_chat_turn_limit, 10);
    }

    #[test]
    fn orbiting_around_the_cursor_keeps_that_point_still() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 40, "width": 40, "height": 40})).unwrap();
        app.preferences.orbit_around = OrbitAround::Cursor;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.fit_view();
        let at = egui::pos2(500.0, 250.0);
        let pivot = pivot_under(&app, &crate::viewport::projection(&app, rect), at);
        orbit(&mut app, rect, 40.0, 25.0, Some(at));
        let s = crate::viewport::projection(&app, rect).to_screen(pivot).unwrap();
        assert!(s.distance(at) < 2.0, "{s:?}");
    }

    /// A drag that starts over empty space turns around the visible geometry's centre, not a
    /// point far off to the side; with nothing visible it turns around the view's centre.
    #[test]
    fn orbiting_from_empty_space_turns_around_the_visible_geometry() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 40, "width": 40, "height": 40})).unwrap();
        app.preferences.orbit_around = OrbitAround::Cursor;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.fit_view();
        let corner = egui::pos2(10.0, 10.0);
        let proj = crate::viewport::projection(&app, rect);
        let hits = crate::viewport::pick(&app, &proj, corner);
        use crate::viewport::Hit;
        assert!(!hits.iter().any(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })), "the corner is empty space");
        let centre = crate::viewport::view_bounds(&app).center();
        orbit(&mut app, rect, 40.0, 25.0, Some(corner));
        assert_eq!(app.viewport.orbit_pivot, Some(centre));

        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.orbit_around = OrbitAround::Cursor;
        app.cam.target = solidcraft_engine::geom::Vec3::new(5.0, -3.0, 2.0);
        orbit(&mut app, rect, 40.0, 25.0, Some(corner));
        assert_eq!(app.viewport.orbit_pivot, Some(solidcraft_engine::geom::Vec3::new(5.0, -3.0, 2.0)));
    }

    /// The Y up preference turns the view: the camera's up axis follows it, startup included.
    #[test]
    fn up_axis_preference_turns_the_view() {
        use solidcraft_engine::render::UpAxis;
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let ctx = crate::fonts::test_context();
        apply(&mut app, &ctx);
        assert_eq!(app.cam.up, UpAxis::Z);
        app.preferences.up_axis = "y".into();
        apply(&mut app, &ctx);
        assert_eq!((app.ui.up_axis, app.cam.up), (UpAxis::Y, UpAxis::Y));
        // Home looks at the model with Y up: the eye is above the XZ ground.
        assert!(app.cam.back().y > 0.3, "{:?}", app.cam.back());
        app.preferences.up_axis = "z".into();
        apply(&mut app, &ctx);
        assert_eq!(app.cam.up, UpAxis::Z);
    }
}

#[cfg(test)]
mod ui_scale_regression_tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, pos2};

    #[test]
    fn desktop_zoom_shortcuts_update_and_persist_the_scale_preference() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.key", json!({"key":"Minus","cmd":true}));
        assert!((h.app.preferences.ui_scale - 0.9).abs() < 1e-4);
        assert!((h.ctx.zoom_factor() - 0.9).abs() < 1e-4);
        h.frames(12);
        assert!((h.ctx.zoom_factor() - 0.9).abs() < 1e-4, "must not flash back to the old preference");
        h.step(&json!({"restart":"close"})).unwrap();
        assert!((h.app.preferences.ui_scale - 0.9).abs() < 1e-4);
        h.call("ui.key", json!({"key":"Equals","cmd":true}));
        assert!((h.ctx.zoom_factor() - 1.0).abs() < 1e-4);
        h.call("ui.key", json!({"key":"Plus","cmd":true}));
        assert!((h.ctx.zoom_factor() - 1.1).abs() < 1e-4);
        h.call("ui.key", json!({"key":"0","cmd":true}));
        assert!((h.ctx.zoom_factor() - 1.0).abs() < 1e-4);
        h.call("ui.prefs", json!({"ui_scale":0.5}));
        h.call("ui.key", json!({"key":"Minus","cmd":true}));
        assert_eq!(h.app.preferences.ui_scale, 0.5);
        h.call("ui.prefs", json!({"ui_scale":2.5}));
        h.call("ui.key", json!({"key":"Plus","cmd":true}));
        assert_eq!(h.app.preferences.ui_scale, 2.5);
    }

    #[test]
    fn dragging_the_scale_slider_keeps_its_window_still_until_release() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Interface"}));
        let window = crate::scenario::rect_of("prefs:window").unwrap();
        super::reveal_preference(&mut h, "prefs:scale");
        let slider = crate::scenario::rect_of("prefs:scale").unwrap();
        let a = pos2(slider.left() + 20.0, slider.center().y);
        let b = pos2(slider.left() + 90.0, slider.center().y);
        h.app.synthetic.push(Event::PointerMoved(a));
        h.app.synthetic.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.frames(3);
        h.app.synthetic.push(Event::PointerMoved(b));
        h.frames(4);
        assert_ne!(h.app.preferences.ui_scale, 1.0, "the slider changes its value during the drag");
        assert_eq!(h.ctx.zoom_factor(), 1.0, "layout must remain stable until release");
        assert_eq!(crate::scenario::rect_of("prefs:window").unwrap(), window);
        h.app.synthetic.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.frames(5);
        assert!((h.ctx.zoom_factor() - h.app.preferences.ui_scale).abs() < 1e-4);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.ui_scale, 1.0, "Cancel restores the original scale");
        assert_eq!(h.ctx.zoom_factor(), 1.0);
    }
}

/// Both display-setting surfaces dispatch the same programmatic command.
pub(crate) fn ambient_occlusion_checkbox(app: &mut SolveApp, ui: &mut egui::Ui, label: &str) {
    let mut enabled = app.preferences.ambient_occlusion;
    let response = ui_kit::checkbox(ui, crate::i18n::tr_label(label), &mut enabled)
        .on_hover_text(crate::i18n::tr_label("Soft shading where opaque surfaces meet. Requires the GPU viewport."));
    crate::scenario::publish_handle("view:ambient_occlusion", response.rect.center());
    if response.changed() {
        let _ = app.run("view.ambient_occlusion", json!({"enabled": enabled}));
    }
}

#[cfg(test)]
mod ambient_occlusion_tests {
    use super::*;

    #[test]
    fn ambient_occlusion_is_optional_persistent_and_cancelable() {
        let old: Prefs = serde_json::from_value(json!({"msaa": 4})).unwrap();
        assert!(!old.ambient_occlusion);
        let mut h = crate::scenario::Harness::new();
        let camera = h.app.cam;
        let design = h.app.session.doc.to_json();
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "view:ambient_occlusion");
        let checkbox = crate::scenario::handle_at("view:ambient_occlusion").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(h.app.preferences.ambient_occlusion);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.ambient_occlusion);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.ambient_occlusion);
        h.call("ui.prefs", json!({"open":"Graphics", "ambient_occlusion":true}));
        h.call("ui.prefs", json!({"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.ambient_occlusion);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.ambient_occlusion);
        h.call("engine.execute", json!({"command":"view.ambient_occlusion","params":{"enabled":false}}));
        assert!(!h.app.preferences.ambient_occlusion);
        h.call("engine.execute", json!({"command":"view.ambient_occlusion","params":{"enabled":"yes"}}));
        assert!(!h.app.preferences.ambient_occlusion);
        h.app.start("view.ambient_occlusion");
        assert!(h.app.preferences.ambient_occlusion);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(h.app.session.undo.is_empty());
    }
}

/// Manual edits are preserved and shown as Custom rather than claiming to match a preset.
fn graphics_preset(prefs: &Prefs) -> &'static str {
    match (prefs.ambient_occlusion, prefs.display_quality.as_str(), prefs.supersampling, prefs.supersampling_adaptive) {
        (false, "normal", 1.0, _) => "performance",
        (true, "fine", 2.0, true) => "quality",
        _ => "custom",
    }
}

pub(crate) fn graphics_preset_selector(app: &mut SolveApp, ui: &mut egui::Ui) {
    let current = graphics_preset(&app.preferences);
    let label = format!("ui.graphics_{current}");
    let response = ui_kit::dropdown_menu(ui, "graphics_preset", app.tr(&label), |ui| {
        for (preset, label) in [("performance", "ui.graphics_performance"), ("quality", "ui.graphics_quality")] {
            let item = ui_kit::list_picker(ui, preset, app.tr(label), current == preset);
            crate::scenario::publish_handle(&format!("view:graphics_preset:{preset}"), item.rect.center());
            if item.clicked() {
                let _ = app.run("view.graphics_preset", json!({"preset":preset}));
            }
        }
    })
    .on_hover_text(app.tr("ui.graphics_preset_help"));
    crate::scenario::publish_handle("view:graphics_preset", response.rect.center());
}

#[cfg(test)]
mod graphics_preset_tests {
    use super::*;

    #[test]
    fn viewport_preset_popup_accepts_quality_and_performance() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.inspect", json!({}));
        for handle in ["nav:view", "view:graphics_preset", "view:graphics_preset:quality"] {
            let pos = crate::scenario::handle_at(handle).unwrap();
            h.call("ui.click", json!({"x":pos.x,"y":pos.y}));
        }
        assert!(h.app.preferences.ambient_occlusion);
        for handle in ["view:graphics_preset", "view:graphics_preset:performance"] {
            let pos = crate::scenario::handle_at(handle).unwrap();
            h.call("ui.click", json!({"x":pos.x,"y":pos.y}));
        }
        assert!(!h.app.preferences.ambient_occlusion);
    }

    #[test]
    fn quality_selector_enables_ao_and_supports_persistence_and_cancel() {
        let mut h = crate::scenario::Harness::new();
        let camera = h.app.cam;
        let design = h.app.session.doc.to_json();
        let msaa = h.app.preferences.msaa;
        h.call("ui.prefs", json!({"open":"Graphics"}));
        for handle in ["view:graphics_preset", "view:graphics_preset:quality"] {
            let pos = crate::scenario::handle_at(handle).unwrap();
            h.call("ui.click", json!({"x":pos.x,"y":pos.y}));
        }
        assert!(h.app.preferences.ambient_occlusion);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.ambient_occlusion);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(graphics_preset(&h.app.preferences), "performance");
        assert!(!h.app.preferences.ambient_occlusion);
        h.call("engine.execute", json!({"command":"view.graphics_preset","params":{"preset":"quality"}}));
        assert!(h.app.preferences.ambient_occlusion);
        h.call("engine.execute", json!({"command":"view.graphics_preset","params":{"preset":"invalid"}}));
        assert!(h.app.preferences.ambient_occlusion);
        h.call("engine.execute", json!({"command":"view.graphics_preset","params":{"preset":"performance"}}));
        assert!(!h.app.preferences.ambient_occlusion);
        assert_eq!(graphics_preset(&h.app.preferences), "performance");
        assert_eq!(h.app.preferences.display_quality, "normal");
        assert_eq!(h.app.preferences.supersampling, 1.0);
        h.call("engine.execute", json!({"command":"view.graphics_preset","params":{"preset":"quality"}}));
        assert_eq!(graphics_preset(&h.app.preferences), "quality");
        assert_eq!(h.app.preferences.display_quality, "fine");
        assert_eq!(h.app.preferences.supersampling, 2.0);
        assert!(h.app.preferences.supersampling_adaptive);
        for edit in [
            json!({"display_quality":"ultra"}),
            json!({"supersampling":1.5}),
            json!({"supersampling_adaptive":false}),
            json!({"ambient_occlusion":false}),
        ] {
            h.call("engine.execute", json!({"command":"view.graphics_preset","params":{"preset":"quality"}}));
            h.call("ui.prefs", edit);
            assert_eq!(graphics_preset(&h.app.preferences), "custom");
        }
        assert_eq!(graphics_preset(&restored.preferences), "quality");
        h.call("ui.prefs", json!({"open":"Graphics"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert_eq!(graphics_preset(&h.app.preferences), "performance");
        h.call("ui.prefs", json!({"button":"ok"}));
        assert_eq!(h.app.preferences.msaa, msaa);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(h.app.session.undo.is_empty());
    }
}
