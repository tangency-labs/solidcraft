//! The Navigation page's settings as plain data, with their defaults: the mouse mapping and its
//! presets, the wheel and trackpad, orbit and the view. The app reads them through its
//! preferences; the page in [`crate::prefs_navigation_page`] draws them.
use egui::Modifiers;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// The mapping whose right-button mouse gestures are on when the gestures are on.
pub const MOUSE_MODE: &str = "solidworks";
pub const OPENSCAD_NAV: &str = "openscad";
pub const TOUCHPAD_NAV: &str = "touchpad";

/// The wheel's zoom speed, the animation speed and the home view's size, as the page accepts them.
pub const WHEEL_ZOOM_SPEED: RangeInclusive<f32> = 0.25..=4.0;
pub const LOOK_AT_SPEED: RangeInclusive<f32> = 0.25..=4.0;
pub const HOME_FIT_SIZE: RangeInclusive<f32> = 0.1..=1.0;

/// What the view's camera does with a drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Orbit,
    Pan,
    Zoom,
}

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
    const ALT: NavKeys = NavKeys { shift: false, ctrl: false, alt: true };
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
    /// Dragging down zooms in instead of out (independent of the wheel preference).
    pub reverse_drag_zoom: bool,
    /// Choosing the preset sets what a middle double-click does (`None` leaves it as it is).
    pub middle_double_click: Option<&'static str>,
    /// Choosing the preset sets "Right drag orbits" (`None` leaves it as it is).
    pub right_drag_orbits: Option<bool>,
    pub bindings: &'static [NavBinding],
}

impl NavPreset {
    /// What dragging `button` with `mods` held does, if anything.
    pub fn mode(&self, button: NavButton, mods: Modifiers) -> Option<NavMode> {
        self.bindings.iter().find(|b| b.button == button && b.keys.held(mods)).map(|b| b.mode)
    }

    /// Button-free motion has exact modifiers, unlike the additive drag bindings.
    pub fn motion_mode(&self, mods: Modifiers) -> Option<NavMode> {
        if self.id != TOUCHPAD_NAV || mods.ctrl || mods.command || mods.mac_cmd {
            return None;
        }
        match (mods.shift, mods.alt) {
            (true, false) => Some(NavMode::Pan),
            (false, true) => Some(NavMode::Orbit),
            _ => None,
        }
    }

    pub fn translated_label(&self) -> &str {
        match self.id {
            "creo" => solidcraft_ui_text::tr("nav.creo.label"),
            TOUCHPAD_NAV => solidcraft_ui_text::tr("nav.touchpad.label"),
            _ => solidcraft_ui_text::tr_label(self.label),
        }
    }

    pub fn translated_help(&self) -> &str {
        if self.id == "creo" { solidcraft_ui_text::tr("nav.creo.help") } else { solidcraft_ui_text::tr_label(self.help) }
    }
}

use NavButton::{Left, Middle, Right};
use NavMode::{Orbit, Pan, Zoom};

/// The mouse mappings. The first is the default.
pub const NAV_PRESETS: &[NavPreset] = &[
    NavPreset {
        id: "fusion",
        label: "Fusion",
        help: "Middle drag pans, Shift+middle orbits, right drag orbits",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::SHIFT, Orbit), bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "solidworks",
        label: "SolidWorks",
        help: "Middle drag orbits, Ctrl+middle pans, Shift+middle zooms, right drag orbits",
        reverse_zoom: true,
        reverse_drag_zoom: false,
        // Picked here, the right button is left to the context menu and mouse gestures (#6492).
        middle_double_click: Some("fit"),
        right_drag_orbits: Some(false),
        bindings: &[
            bind(Middle, NavKeys::CTRL, Pan),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    // Public Creo Parametric dynamic-viewing bindings; no product code or assets are used.
    // https://support.ptc.com/help/creo/creo_pma/r12/usascii/fundamentals/fundamentals/About_Dynamic_Viewing.html
    NavPreset {
        id: "creo",
        label: "Creo",
        help: "Middle drag orbits, Shift+middle pans, Ctrl+middle drag vertically zooms (down to zoom in); scroll the wheel to zoom",
        reverse_zoom: false,
        reverse_drag_zoom: true,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "nx",
        label: "NX",
        // NX's CAD1 scheme drags middle to rotate, middle + left to zoom and middle + right to
        // pan; the bindings here hold one button at a time, so the two-button drags are the
        // middle drag with Ctrl (zoom) and Shift (pan).
        help: "Middle drag orbits, Shift+middle pans (NX's middle + right), Ctrl+middle zooms (NX's middle + left); right drag leaves the camera",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: Some("fit"),
        right_drag_orbits: Some(false),
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "inventor",
        label: "Inventor",
        help: "Middle drag pans, Shift+middle orbits, Ctrl+middle zooms, right drag orbits",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
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
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
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
        id: "maya",
        label: "Maya",
        help: "Alt+left drag orbits, Alt+middle pans, Alt+right zooms; middle drag also pans",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[
            bind(Left, NavKeys::ALT, Orbit),
            bind(Middle, NavKeys::ALT, Pan),
            bind(Right, NavKeys::ALT, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: "blender",
        label: "Blender",
        help: "Middle drag orbits, Shift+middle pans, Ctrl+middle zooms; right drag leaves the camera alone",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "tinkercad",
        label: "Tinkercad",
        help: "Right drag orbits, middle or Shift+right drag pans",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Right, NavKeys::SHIFT, Pan), bind(Right, NavKeys::NONE, Orbit), bind(Middle, NavKeys::NONE, Pan)],
    },
    NavPreset {
        id: OPENSCAD_NAV,
        label: "OpenSCAD",
        help: "Left drag on empty space orbits, Shift+left drag box-selects, sketch geometry drags edit; right or middle drag pans, Shift+right or Shift+middle drag zooms",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[
            bind(Left, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Right, NavKeys::NONE, Pan),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: "hfss",
        label: "HFSS",
        help: "Alt+left drag pans, Shift+Alt+left zooms, Ctrl+left drag on empty space orbits",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        // Ctrl+left orbit is the drag from empty space every preset has: as a binding it would
        // also take a Ctrl+click on geometry, which adds to the selection.
        bindings: &[bind(Left, NavKeys::SHIFT_ALT, Zoom), bind(Left, NavKeys::ALT, Pan)],
    },
    NavPreset {
        id: TOUCHPAD_NAV,
        label: "Touchpad",
        help: "Hold Shift and move the pointer to pan; hold Alt/Option and move it to orbit. No mouse button is needed. Vertical scroll zooms.",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
];

/// The preset with this id (the first, the default, for an unknown one).
pub fn nav_preset(id: &str) -> &'static NavPreset {
    NAV_PRESETS.iter().find(|p| p.id == id).unwrap_or(&NAV_PRESETS[0])
}

/// The gesture settings, kept with the preferences: whether the right-button ring is on and the
/// tool lists a studio has of its own. The app reads these; the gesture logic is the app's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MouseGestureSettings {
    /// Gestures are on (they also need the SolidWorks mouse mode).
    pub enabled: bool,
    /// A studio's own list of tool ids, clockwise from the top, by studio key. A studio without an
    /// entry uses its default tools.
    pub wheels: BTreeMap<String, Vec<String>>,
}

/// How the navigator figure of the view shows its axes or a custom body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorStyle {
    /// Also what a navigator this version doesn't know reads as (`lenient`).
    #[default]
    Axes,
    /// A body of the user's choice; the signed axes until one is chosen.
    Custom,
}

impl NavigatorStyle {
    pub const ALL: [Self; 2] = [Self::Axes, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Axes => "Signed axes",
            Self::Custom => "Custom",
        }
    }
}

/// How the custom view gizmo colours its body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorTexture {
    /// The navigator's colours. Also what a value this version doesn't know reads as
    /// (`lenient`).
    #[default]
    Themed,
    /// The body's own appearance, as the viewport shows it.
    Modeled,
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

/// The sole modifier that turns vertical scroll into wheel-style zoom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackpadZoomModifier {
    Cmd,
    Alt,
    Ctrl,
    /// The default: scroll keeps its usual meaning until a key is chosen.
    #[default]
    Off,
}

impl TrackpadZoomModifier {
    pub const ALL: [Self; 4] = [Self::Cmd, Self::Alt, Self::Ctrl, Self::Off];

    pub fn modifiers(self) -> Option<Modifiers> {
        match self {
            Self::Cmd => Some(Modifiers::COMMAND),
            Self::Alt => Some(Modifiers::ALT),
            Self::Ctrl => Some(Modifiers::CTRL),
            Self::Off => None,
        }
    }

    /// The message id of the choice's label; `mac` names Cmd on macOS.
    pub fn label_id(self, mac: bool) -> &'static str {
        match self {
            Self::Cmd if mac => "prefs.trackpad_zoom_modifier.cmd",
            Self::Alt => "prefs.trackpad_zoom_modifier.alt",
            Self::Cmd | Self::Ctrl => "prefs.trackpad_zoom_modifier.ctrl",
            Self::Off => "prefs.trackpad_zoom_modifier.off",
        }
    }
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

    /// The setting for `id` (Home for an unknown one).
    pub fn of_id(id: &str) -> MiddleDoubleClick {
        Self::parse(id).unwrap_or(MiddleDoubleClick::Home)
    }

    pub fn parse(id: &str) -> Option<MiddleDoubleClick> {
        Self::ALL.iter().find(|(_, i, _)| *i == id).map(|(m, _, _)| *m)
    }

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(m, _, _)| *m == self).map_or("Home", |(_, _, l)| l)
    }
}

/// The keys the settings are stored under, in the order of the struct. A test keeps this list equal
/// to the struct's serialized keys.
pub const FIELDS: &[&str] = &[
    "nav",
    "touchscreen_navigation",
    "free_orbit",
    "free_orbit_upright",
    "right_drag_orbits",
    "nav_drag_keeps_mode",
    "orbit_cursor_wrap",
    "mark_orbit_point",
    "upright_orbit_unrolls",
    "navigator_style",
    "navigator_texture",
    "navigator_arrows",
    "navigator_home",
    "radial_context_menu",
    "quick_menu_after_selection",
    "mouse_gestures",
    "zoom_reverse",
    "maya_horizontal_zoom",
    "orbit_around",
    "orbit_cursor",
    "browser_arrow_keys",
    "zoom_to_cursor",
    "wheel_zoom_speed",
    "zoom_stop_at_model",
    "smooth_wheel_zoom",
    "keep_hover_while_navigating",
    "trackpad_pan",
    "trackpad_zoom_modifier",
    "pan_reverse_x",
    "pan_reverse_y",
    "orbit_reverse",
    "trackpad_twist_turns",
    "look_at_anim",
    "look_at_speed",
    "middle_double_click",
    "navigator_fit",
    "invert_navigator_arrows",
    "invert_curved_roll_arrows",
    "home_fit_size",
];

/// The Navigation page's settings. Every field's default is the one a fresh install starts from,
/// and the page's Restore Defaults puts them all back. The saved preferences hold these fields at
/// the top level, as they always were, so a file saved before they moved here still loads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationSettings {
    /// Mouse mapping: a preset's id (`fusion`, `solidworks`, ...).
    pub nav: String,
    /// Touchscreen drags orbit/pan; off preserves the mouse mapping's selection behaviour.
    pub touchscreen_navigation: bool,
    /// Opt in to rotation through both poles with roll, without the Shift axis-snap spring.
    pub free_orbit: bool,
    /// With free orbit, mouse drags turn sideways about the world's up axis.
    pub free_orbit_upright: bool,
    /// Off: a right drag that the mouse mapping would orbit leaves the camera alone (#6492).
    pub right_drag_orbits: bool,
    /// A middle or right drag keeps the mode it started with until its button is released (#9557).
    pub nav_drag_keeps_mode: bool,
    /// An orbit drag that reaches the edge of the view wraps to the opposite edge and keeps turning.
    pub orbit_cursor_wrap: bool,
    /// While an orbit turns about a point, a faint ball marks it (#17474).
    pub mark_orbit_point: bool,
    /// The upright orbit stands a rolled view up as the pointer moves (#12458).
    pub upright_orbit_unrolls: bool,
    /// View orientation widget: the signed axes (default) or a custom body.
    pub navigator_style: NavigatorStyle,
    /// The custom view gizmo's colours: the theme's or the body's own.
    pub navigator_texture: NavigatorTexture,
    /// The custom view gizmo has the orbit and roll arrows around it.
    pub navigator_arrows: bool,
    /// The custom view gizmo has the Home button beside it.
    pub navigator_home: bool,
    /// Show the radial ring and enable hold/flick gestures for viewport context menus.
    pub radial_context_menu: bool,
    /// The quick menu also opens by itself shortly after a click in the viewport selects something (#5501).
    pub quick_menu_after_selection: bool,
    /// Mouse gestures: a ring of tools on the right button.
    pub mouse_gestures: MouseGestureSettings,
    /// The wheel zooms out when turned forward.
    pub zoom_reverse: bool,
    /// Maya Alt+right drag uses horizontal motion; off restores the former vertical zoom.
    pub maya_horizontal_zoom: bool,
    /// What an orbit turns around.
    pub orbit_around: OrbitAround,
    /// The cursor preference as releases before the origin option saved it (`true` = the point
    /// under the cursor). The migration folds it into `orbit_around`; this version writes `false`.
    pub orbit_cursor: bool,
    /// Arrow keys browse the document browser.
    pub browser_arrow_keys: bool,
    /// The wheel zooms toward the point under the cursor (off: toward the view's centre).
    pub zoom_to_cursor: bool,
    /// The wheel's zoom speed, 0.25 to 4.
    pub wheel_zoom_speed: f32,
    /// The wheel zoom stops at the model's extent.
    pub zoom_stop_at_model: bool,
    /// Smooth wheel zoom: off applies each wheel event immediately.
    pub smooth_wheel_zoom: bool,
    /// The hover highlight stays on while navigating.
    pub keep_hover_while_navigating: bool,
    /// Two-finger trackpad scroll pans.
    pub trackpad_pan: bool,
    /// The modifier that turns vertical scroll into zoom.
    pub trackpad_zoom_modifier: TrackpadZoomModifier,
    /// Horizontal two-finger scrolling and the thumb wheel pan the other way.
    pub pan_reverse_x: bool,
    /// Vertical two-finger scrolling pans the other way.
    pub pan_reverse_y: bool,
    /// Shift with two-finger scrolling orbits the other way.
    pub orbit_reverse: bool,
    /// A two-finger twist turns the view.
    pub trackpad_twist_turns: bool,
    /// Animate view changes.
    pub look_at_anim: bool,
    /// The view animation's speed, 0.25 to 4.
    pub look_at_speed: f32,
    /// What a middle double-click in the view does: `home`, `fit` or `off`.
    pub middle_double_click: String,
    /// The navigator's fit to the view.
    pub navigator_fit: bool,
    /// Invert the navigator's arrows.
    pub invert_navigator_arrows: bool,
    /// Invert the curved roll arrows.
    pub invert_curved_roll_arrows: bool,
    /// The home view's size as a share of the view, 0.1 to 1.
    pub home_fit_size: f32,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            nav: "fusion".into(),
            touchscreen_navigation: false,
            free_orbit: false,
            free_orbit_upright: false,
            right_drag_orbits: true,
            nav_drag_keeps_mode: true,
            orbit_cursor_wrap: true,
            mark_orbit_point: true,
            upright_orbit_unrolls: true,
            navigator_style: NavigatorStyle::default(),
            navigator_texture: NavigatorTexture::default(),
            navigator_arrows: true,
            navigator_home: true,
            radial_context_menu: false,
            quick_menu_after_selection: false,
            mouse_gestures: MouseGestureSettings::default(),
            zoom_reverse: false,
            maya_horizontal_zoom: true,
            orbit_around: OrbitAround::default(),
            orbit_cursor: false,
            browser_arrow_keys: true,
            zoom_to_cursor: true,
            wheel_zoom_speed: 1.0,
            zoom_stop_at_model: false,
            smooth_wheel_zoom: true,
            keep_hover_while_navigating: true,
            trackpad_pan: true,
            trackpad_zoom_modifier: TrackpadZoomModifier::default(),
            pan_reverse_x: false,
            pan_reverse_y: false,
            orbit_reverse: false,
            trackpad_twist_turns: false,
            look_at_anim: true,
            look_at_speed: 1.0,
            middle_double_click: "home".into(),
            navigator_fit: false,
            invert_navigator_arrows: false,
            invert_curved_roll_arrows: false,
            home_fit_size: 1.0,
        }
    }
}

impl NavigationSettings {
    /// Gestures are on and the mouse mapping allows them.
    pub fn gestures_active(&self) -> bool {
        self.mouse_gestures.enabled && self.nav == MOUSE_MODE
    }

    /// Picks `preset` the way the Preferences page does: its bindings, its wheel direction and,
    /// when the preset names them, middle double-click and "Right drag orbits".
    pub fn choose_preset(&mut self, preset: &NavPreset) {
        self.nav = preset.id.into();
        self.zoom_reverse = preset.reverse_zoom;
        if let Some(double_click) = preset.middle_double_click {
            self.middle_double_click = double_click.into();
        }
        if let Some(orbits) = preset.right_drag_orbits {
            self.right_drag_orbits = orbits;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_the_keys_the_settings_are_stored_under() {
        let value = serde_json::to_value(NavigationSettings::default()).unwrap();
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        let mut fields = FIELDS.to_vec();
        keys.sort_unstable();
        fields.sort_unstable();
        assert_eq!(keys, fields, "every stored key is listed in FIELDS, and only those");
    }
}
