//! Grouped preferences composed with the shared UI kit. Changes apply immediately; Cancel
//! restores the opening snapshot, Apply keeps editing, and Defaults affects the current page.

use crate::ui_kit::BackingTooltipExt as _;
use solidcraft_engine::geom::prelude::*;
use std::collections::BTreeMap;

use egui::{Modifiers, Pos2};
use serde::{Deserialize, Serialize};
use serde_json::json;
use solidcore_record::Vec3;
pub use solidcraft_engine::doc::display_units::{format_length, length_number, length_precision, unit_scale};
use solidcraft_viewport::Camera;

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
pub(crate) mod figures;
mod general;
mod graphics;
mod interface;
mod mouse3d;
mod navbar;
mod navigation;
mod panel_layout;
mod sketch;
mod theme_picker;
mod tools;

use crate::SolveApp;
use crate::prefs_window;
use crate::tool_modifiers::ModifierCombo;
use crate::ui_kit;
use crate::viewport::NavMode;
use solidcraft_theme::Tokens;

pub const UNITS: &[&str] = &["mm", "cm", "m", "in", "ft"];
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

    pub(crate) fn translated_label(&self) -> &str {
        match self.id {
            "creo" => solidcraft_ui_text::tr("nav.creo.label"),
            TOUCHPAD_NAV => solidcraft_ui_text::tr("nav.touchpad.label"),
            _ => solidcraft_ui_text::tr_label(self.label),
        }
    }

    pub(crate) fn translated_help(&self) -> &str {
        if self.id == "creo" { solidcraft_ui_text::tr("nav.creo.help") } else { solidcraft_ui_text::tr_label(self.help) }
    }
}

/// What a two-finger trackpad scroll does in a navigation preset (#15079).
///
/// Fusion orbits with Shift held and pans without it; Blender is the other way
/// round; the other presets pan either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackpadScroll {
    /// Bare and Shift scrolls both pan.
    Pans,
    /// Shift + two fingers orbits; bare two fingers pan (Fusion).
    ShiftOrbits,
    /// Bare two fingers orbit; Shift + two fingers pans (Blender).
    BareOrbits,
}

impl TrackpadScroll {
    /// Whether the two gestures differ, so a coast ends when the modifier changes.
    pub fn modes_differ(self) -> bool {
        self != TrackpadScroll::Pans
    }

    /// Whether a two-finger scroll with `shift` held orbits.
    pub fn orbits(self, shift: bool) -> bool {
        match self {
            TrackpadScroll::Pans => false,
            TrackpadScroll::ShiftOrbits => shift,
            TrackpadScroll::BareOrbits => !shift,
        }
    }
}

impl Prefs {
    /// Touchpad scroll zooms without overwriting the user's preference for other presets.
    pub fn trackpad_pans(&self) -> bool {
        self.nav != TOUCHPAD_NAV && self.trackpad_pan
    }

    /// What a two-finger trackpad scroll does in the chosen preset (#15079).
    pub fn trackpad_scroll(&self) -> TrackpadScroll {
        match self.nav.as_str() {
            "fusion" => TrackpadScroll::ShiftOrbits,
            "blender" => TrackpadScroll::BareOrbits,
            _ => TrackpadScroll::Pans,
        }
    }
}

/// The preset with this id (the default for an unknown one).
pub fn nav_preset(id: &str) -> &'static NavPreset {
    NAV_PRESETS.iter().find(|p| p.id == id).unwrap_or(&NAV_PRESETS[0])
}

/// Picks `preset` the way the Preferences page does: its bindings, wheel direction and, when the
/// preset names them, middle double-click and "Right drag orbits".
pub fn choose_nav_preset(prefs: &mut Prefs, preset: &NavPreset) {
    prefs.nav = preset.id.into();
    prefs.zoom_reverse = preset.reverse_zoom;
    if let Some(double_click) = preset.middle_double_click {
        prefs.middle_double_click = double_click.into();
    }
    if let Some(orbits) = preset.right_drag_orbits {
        prefs.right_drag_orbits = orbits;
    }
}
/// Stable control-channel names; presentation labels may be shorter.
pub const PAGES: &[&str] =
    &["General", "Appearance", "Layout", "Graphics", "Design", "Sketch", "Tools", "Navigation", "3D mouse", "Shortcuts", "Assistant", "Plugins"];
pub const PAGE_GENERAL: usize = 0;
pub const PAGE_APPEARANCE: usize = 1;
pub const PAGE_LAYOUT: usize = 2;
pub const PAGE_GRAPHICS: usize = 3;
pub const PAGE_DESIGN: usize = 4;
pub const PAGE_SKETCH: usize = 5;
pub const PAGE_TOOLS: usize = 6;
pub const PAGE_NAVIGATION: usize = 7;
pub const PAGE_MOUSE3D: usize = 8;
pub const PAGE_SHORTCUTS: usize = 9;
pub const PAGE_ASSISTANT: usize = 10;
pub const PAGE_PLUGINS: usize = 11;
/// What Preferences accepts for `parameter_list_rows`: `parameters.complete` answers at most 200.
pub const PARAMETER_LIST_ROWS: std::ops::RangeInclusive<u16> = 5..=200;
const WINDOW_PAGES: &[prefs_window::Page<SolveApp>] = &[
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_general"),
        label: "ui.general",
        group: None,
        handle: "prefs:page:General",
        scroll: true,
        draw: page_draw::<PAGE_GENERAL>,
        defaults: page_defaults::<PAGE_GENERAL>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_appearance"),
        label: "prefs.structure.appearance",
        group: Some("prefs.structure.look"),
        handle: "prefs:page:Appearance",
        scroll: true,
        draw: page_draw::<PAGE_APPEARANCE>,
        defaults: page_defaults::<PAGE_APPEARANCE>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_layout"),
        label: "prefs.structure.layout",
        group: None,
        handle: "prefs:page:Layout",
        scroll: true,
        draw: page_draw::<PAGE_LAYOUT>,
        defaults: page_defaults::<PAGE_LAYOUT>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_graphics"),
        label: "ui.graphics",
        group: None,
        handle: "prefs:page:Graphics",
        scroll: true,
        draw: page_draw::<PAGE_GRAPHICS>,
        defaults: page_defaults::<PAGE_GRAPHICS>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_design"),
        label: "ui.design",
        group: Some("prefs.structure.work"),
        handle: "prefs:page:Design",
        scroll: true,
        draw: page_draw::<PAGE_DESIGN>,
        defaults: page_defaults::<PAGE_DESIGN>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_sketch"),
        label: "prefs.structure.sketch",
        group: None,
        handle: "prefs:page:Sketch",
        scroll: true,
        draw: page_draw::<PAGE_SKETCH>,
        defaults: page_defaults::<PAGE_SKETCH>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_tools"),
        label: "prefs.structure.tools",
        group: None,
        handle: "prefs:page:Tools",
        scroll: true,
        draw: page_draw::<PAGE_TOOLS>,
        defaults: page_defaults::<PAGE_TOOLS>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_navigation"),
        label: "ui.navigation",
        group: Some("prefs.input"),
        handle: "prefs:page:Navigation",
        scroll: true,
        draw: page_draw::<PAGE_NAVIGATION>,
        defaults: page_defaults::<PAGE_NAVIGATION>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_mouse3d"),
        label: "prefs.structure.mouse3d",
        group: None,
        handle: "prefs:page:3D mouse",
        scroll: true,
        draw: page_draw::<PAGE_MOUSE3D>,
        defaults: page_defaults::<PAGE_MOUSE3D>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_shortcuts"),
        label: "prefs.shortcuts",
        group: None,
        handle: "prefs:page:Shortcuts",
        scroll: false,
        draw: page_draw::<PAGE_SHORTCUTS>,
        defaults: page_defaults::<PAGE_SHORTCUTS>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_assistant"),
        label: "prefs.assistant",
        group: Some("prefs.extend"),
        handle: "prefs:page:Assistant",
        scroll: true,
        draw: page_draw::<PAGE_ASSISTANT>,
        defaults: page_defaults::<PAGE_ASSISTANT>,
    }),
    prefs_window::Page::Host(prefs_window::HostPage {
        icon: ui_kit::Icon::Kit("prefs_plugins"),
        label: "ui.plugins",
        group: None,
        handle: "prefs:page:Plugins",
        scroll: true,
        draw: page_draw::<PAGE_PLUGINS>,
        defaults: page_defaults::<PAGE_PLUGINS>,
    }),
];

/// Former page names remain usable through both ui.prefs and scenario handles.
const PAGE_ALIASES: &[(&str, usize)] = &[
    ("AI", PAGE_ASSISTANT),
    ("Interface", PAGE_APPEARANCE),
    ("Interface/navigation bar", PAGE_LAYOUT),
    ("Keyboard Shortcuts", PAGE_SHORTCUTS),
    ("Selection", PAGE_SKETCH),
    ("Sketch & selection", PAGE_SKETCH),
    ("Sketch and selection", PAGE_SKETCH),
];

/// A page as the window draws it.
fn page_draw<const PAGE: usize>(ui: &mut egui::Ui, app: &mut SolveApp) {
    draw_page(app, ui, PAGE, &Tokens::get());
}

/// Restore Defaults for a page.
fn page_defaults<const PAGE: usize>(app: &mut SolveApp) {
    defaults(app, PAGE);
}

pub const THEMES: &[(&str, &str)] = &[("dark", "Dark"), ("light", "Light"), ("system", "System")];
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
    /// A body of the user's choice, drawn the cube's size ([`crate::view_gizmo`]); the cube
    /// until one is chosen.
    Custom,
}

impl NavigatorStyle {
    pub const ALL: [Self; 3] = [Self::Axes, Self::Cube, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Axes => "Signed axes",
            Self::Cube => "Cube",
            Self::Custom => "Custom",
        }
    }
}

/// How the custom view gizmo colours its body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorTexture {
    /// The navigation cube's colours. Also what a value this version doesn't know reads as
    /// (`lenient`).
    #[default]
    Themed,
    /// The body's own appearance, as the viewport shows it.
    Modeled,
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

/// How a section hatches the faces it cuts (Preferences: "Section hatch").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionHatch {
    /// Each body in its own hatch (`solidcraft_viewport::hatch`): 45 or 135 degrees in one of the
    /// theme's hatch colours, touching bodies never alike. Also what a value this version doesn't
    /// know reads as (`lenient`).
    #[default]
    ByPart,
    /// Every cut face in the cap colour at 45 degrees (how it was before by part).
    One,
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
            (Self::TopFrontRight, "XY") => solidcraft_ui_text::tr("ui.top"),
            (Self::TopFrontRight, "XZ") => solidcraft_ui_text::tr("ui.front"),
            (Self::TopFrontRight, "YZ") => solidcraft_ui_text::tr("ui.right"),
            _ => name,
        }
    }
}

/// Where a sketch dimension's value is typed (Preferences › Sketch, "Dimension values", #7246).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionInput {
    /// In a box on the dimension (also what a value this version doesn't know reads as).
    #[default]
    View,
    /// In a small window at the dimension, with arrows that step the value (`dim_view::window`).
    Window,
}

/// The lengths (millimetres) and angles (degrees) the dimension window's arrows may step by.
pub const DIMENSION_STEP_MM: std::ops::RangeInclusive<f64> = 0.001..=1000.0;
pub const DIMENSION_STEP_DEG: std::ops::RangeInclusive<f64> = 0.001..=180.0;

/// How far a sketch dimension's text may be scaled (Preferences › Sketch › Dimension text size).
pub const DIMENSION_TEXT_SCALE: std::ops::RangeInclusive<f32> = 0.5..=2.5;

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

    pub fn modifiers(self) -> Option<egui::Modifiers> {
        match self {
            Self::Cmd => Some(egui::Modifiers::COMMAND),
            Self::Alt => Some(egui::Modifiers::ALT),
            Self::Ctrl => Some(egui::Modifiers::CTRL),
            Self::Off => None,
        }
    }

    pub fn label_id(self, platform: crate::tool_modifiers::Platform) -> &'static str {
        match self {
            Self::Cmd if platform == crate::tool_modifiers::Platform::Mac => "prefs.trackpad_zoom_modifier.cmd",
            Self::Alt => "prefs.trackpad_zoom_modifier.alt",
            Self::Cmd | Self::Ctrl => "prefs.trackpad_zoom_modifier.ctrl",
            Self::Off => "prefs.trackpad_zoom_modifier.off",
        }
    }
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

/// How the offer to stand a file up is made when its header names no program known to save Y up
/// (Preferences: "For files from unknown programs", #7717). A file from SolidWorks is asked in the
/// dialog (only the note when it already stands on Z, unless this is Ask); SolidCraft's own files
/// are never offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandUpUnknown {
    /// The "Stand this part up?" dialog for every file from another program, without looking at
    /// how it rests, as before #7717 (a SolidWorks file too).
    Ask,
    /// No offer: the context menu's Turn Up Axis still stands the file up.
    Off,
    /// A small note over the view, only when the file seems to lie on its back, that goes by
    /// itself and never blocks input (the default). A saved value this version doesn't know reads
    /// as this.
    #[default]
    #[serde(other)]
    Note,
}

impl StandUpUnknown {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Note, Self::Off];

    /// The choice as `ui.prefs` and the saved preferences spell it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Note => "note",
            Self::Off => "off",
        }
    }

    /// The message id of the choice's label.
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Ask => "prefs.stand_up_unknown.ask",
            Self::Note => "prefs.stand_up_unknown.note",
            Self::Off => "prefs.stand_up_unknown.off",
        }
    }
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

/// The default largest STEP/import file, in MB (`solidcraft_io::MAX_STEP_BYTES`).
pub const DEFAULT_STEP_MB: u64 = solidcraft_engine::io::limits::FileLimits::DEFAULT.step / solidcraft_engine::io::limits::MB;
/// The default largest design file, in MB (`solidcraft_io::MAX_DESIGN_BYTES`).
pub const DEFAULT_DESIGN_MB: u64 = solidcraft_engine::io::limits::FileLimits::DEFAULT.design / solidcraft_engine::io::limits::MB;

/// The file size limits the preferences ask for (Preferences › General › Largest file to open),
/// brought into range.
pub fn file_limits(p: &Prefs) -> solidcraft_engine::io::limits::FileLimits {
    solidcraft_engine::io::limits::FileLimits::from_mb(p.max_step_file_mb, p.max_design_file_mb).clamped()
}

/// Every read from now on uses the preferred file size limits (unless `--max-file-size` gave
/// this run its own). The browser keeps its defaults.
pub fn apply_file_limits(p: &Prefs) {
    if solidcraft_engine::io::limits::ADJUSTABLE
        && let Err(e) = solidcraft_engine::io::limits::set_preferred(file_limits(p))
    {
        log::warn!("preferences: file size limits: {e}");
    }
}

/// The most points or curves one sketch holds, as the preferences ask (Preferences › Sketch ›
/// Largest sketch, #10874); a value outside the accepted range (a hand edit) reads as the default.
pub fn sketch_limit(p: &Prefs) -> usize {
    if accepted::max_sketch_points(p) { p.max_sketch_points } else { solidcraft_engine::sketch::limits::DEFAULT_POINTS }
}

/// Every sketch built from now on holds up to [`sketch_limit`] points.
pub fn apply_sketch_limit(p: &Prefs) {
    if let Err(e) = solidcraft_engine::sketch::limits::set_max_points(sketch_limit(p)) {
        log::warn!("preferences: largest sketch: {e}");
    }
}

/// Perspective view's default field of view (about 34°), in radians.
const PERSPECTIVE_FOV: f64 = 0.6;

/// The field of view Perspective view draws with: the preference, or the default when a saved
/// value is outside the accepted range (a hand edit).
pub fn perspective_fov(p: &Prefs) -> f64 {
    if accepted::perspective_fov(p) { p.perspective_fov } else { PERSPECTIVE_FOV }
}

/// The factor a sketch dimension's text is scaled by: the preference, or 1 when a saved value is
/// outside the accepted range (a hand edit). Applies in the open sketch and to Show Dimensions.
pub fn dimension_text_scale(p: &Prefs) -> f32 {
    if accepted::dimension_text_scale(p) { p.dimension_text_scale } else { 1.0 }
}

/// The colour a sketch dimension draws its lines, arrows and value in: the user's colour when set,
/// else the theme's colour for a driving dimension or, when `driven`, its driven-dimension colour.
/// Selection, hover and conflicts are drawn by their own colours before this is consulted.
pub fn dimension_colour(p: &Prefs, driven: bool) -> egui::Color32 {
    match p.dimension_colour {
        Some([r, g, b]) => egui::Color32::from_rgb(r, g, b),
        None if driven => Tokens::get().dimension_driven,
        None => Tokens::get().dimension,
    }
}

/// Perspective view keeps the target's size when its angle changes (#5364): the distance scales by
/// tan(old/2) / tan(new/2), so the angle changes the depth, not the zoom. Called every frame and
/// after `ui.prefs`, so every way the angle changes (the slider, Cancel, Restore Defaults, a script)
/// goes through it. A parallel view keeps the last perspective angle, and while Render is open the
/// camera is Render's own, so the change waits until the design view is back.
pub(crate) fn follow_view_fov(app: &mut SolveApp) {
    let fov = app.view_fov();
    if fov <= 0.0 || crate::render_workspace::active(app) {
        return;
    }
    let old = std::mem::replace(&mut app.prefs_window.drawn_fov, fov);
    if old > 0.0 && old != fov {
        let k = (old * 0.5).tan() / (fov * 0.5).tan();
        app.cam.distance *= k;
        if let Some(anim) = &mut app.cam_anim {
            anim.from.distance *= k;
            anim.to.distance *= k;
        }
    }
}

/// The browser sidebar's first width, and the width [`Prefs::default`] restores; the panel
/// itself clamps what the user drags to its own range.
pub const DEFAULT_BROWSER_WIDTH: f32 = 250.0;

/// The browser's first share of the column it shares with History under it (#7371): the equal
/// split; the column clamps what the user drags so both cards keep their header and two rows.
pub const DEFAULT_BROWSER_SHARE: f32 = 0.5;

/// When a component's bodies sit in a Bodies folder of their own in the browser (#4217): each
/// folder is one more level that pushes deep assemblies right. Only folders inside components
/// fold; the design's own Bodies folder, and a folder hidden with its eye (its eye is the way
/// back), always stay (`browser::bodies_folded`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodiesFolders {
    Always,
    /// A component with one body and no groups lists the body straight under it. Also what a
    /// value this version doesn't know reads as (`lenient`).
    #[default]
    Several,
    /// Components list their bodies and groups straight under them.
    Never,
}

impl BodiesFolders {
    pub const ALL: [Self; 3] = [Self::Always, Self::Several, Self::Never];

    /// Its name in the Preferences (a message id).
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Always => "prefs.bodies_folders_always",
            Self::Several => "prefs.bodies_folders_several",
            Self::Never => "prefs.bodies_folders_never",
        }
    }
}

// The stored fields, their defaults, page resets and simple form controls share this table.
// Omitted pages keep fields such as grid_follows_view and added agents when restoring defaults.
// Accepted control-channel ranges may differ from slider ranges (look_at_speed).
macro_rules! preference_settings {
    (legacy { $($legacy:ident: $legacy_ty:ty = $legacy_default:expr;)* } $( $(#[$doc:meta])* $field:ident: $ty:ty = $default:expr $(, $page:expr)? $(, accept($accepted:expr))? $(=> $control:ident($($args:tt)*))? ;)*) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Prefs {
            $($(#[$doc])* pub $field: $ty,)*
            $(#[serde(skip_serializing)] pub $legacy: $legacy_ty,)*
        }

        impl Default for Prefs {
            fn default() -> Self { Self { $($field: $default,)* $($legacy: $legacy_default,)* } }
        }

        const LEGACY_PANEL_FIELDS: &[&str] = &[$(stringify!($legacy),)*];

        impl Prefs {
            fn retain_legacy_panel_fields(&mut self, loaded: &Self) {
                $(self.$legacy.clone_from(&loaded.$legacy);)*
            }
        }

        /// Every stored field and the page whose Defaults resets it (`None`: no page).
        pub(crate) const FIELD_PAGES: &[(&str, Option<usize>)] = &[$((stringify!($field), preference_settings!(@page $($page)?)),)*];

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
    (@page) => { None };
    (@page $page:expr) => { Some($page) };
    (@control $app:ident, $ui:ident, $field:ident, check($label:literal $(, $help:literal)?)) => {
        ui_kit::checkbox($ui, solidcraft_ui_text::tr_label($label), &mut $app.preferences.$field)
            $(.backing_on_hover_text(solidcraft_ui_text::tr_label($help)))?
    };
    (@control $app:ident, $ui:ident, $field:ident, check_tr($label:literal)) => {
        ui_kit::checkbox($ui, solidcraft_ui_text::tr($label), &mut $app.preferences.$field)
    };
    (@control $app:ident, $ui:ident, $field:ident, check_id($label:literal)) => {
        ui_kit::checkbox($ui, $app.tr($label), &mut $app.preferences.$field)
    };
    (@control $app:ident, $ui:ident, $field:ident, check_id_help($label:literal, $help:literal)) => {
        ui_kit::checkbox($ui, $app.tr($label), &mut $app.preferences.$field).backing_on_hover_text($app.tr($help))
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
    // Read old placement once when a saved shared layout is absent.
    legacy {
        timeline_left: bool = false;
        timeline_left_width: f32 = 130.0;
        timeline_under_browser: bool = false;
        timeline_collapsed: bool = false;
        browser_share: f32 = DEFAULT_BROWSER_SHARE;
        timeline_left_browser_collapsed: bool = false;
        browser_width: f32 = DEFAULT_BROWSER_WIDTH;
        render_inspector_side: String = "right".into();
        render_inspector_width: f32 = 305.0;
        render_browser_side: String = "left".into();
        render_browser_width: f32 = 266.0;
        browser_collapsed: bool = false;
        agent_chat_side: String = "right".into();
        agent_chat_width: f32 = 380.0;
        agent_chat_minimized: bool = false;
    }
    /// Units of new designs.
    default_units: String = "mm".into(), PAGE_DESIGN;
    /// Mouse mapping: `fusion`, `solidworks` or `inventor`.
    nav: String = "fusion".into(), PAGE_NAVIGATION;
    /// Opt in to rotation through both poles with roll, without the Shift axis-snap spring.
    /// Independent of mouse bindings; false also preserves existing saved presets' behavior.
    free_orbit: bool = false, PAGE_NAVIGATION;
    /// With free orbit, mouse drags turn sideways about the world's up axis (a turntable without
    /// the pole limit) instead of the screen's.
    free_orbit_upright: bool = false, PAGE_NAVIGATION => check_tr("nav.free_orbit_upright.label");
    /// Off: a right drag that the mouse mapping would orbit leaves the camera alone (#6492).
    right_drag_orbits: bool = true, PAGE_NAVIGATION => check_tr("nav.right_drag_orbits.label");
    /// A middle or right drag keeps the mode it started with until its button is released, as a
    /// left drag with the mapping's keys does: letting go of Shift partway through a Shift+middle
    /// orbit doesn't turn it into a pan (#9557). Off: the keys held decide on every frame, as before.
    nav_drag_keeps_mode: bool = true, PAGE_NAVIGATION => check_tr("nav.drag_keeps_mode.label");
    /// An orbit drag that reaches the edge of the view moves the cursor to the opposite edge and
    /// keeps turning; false stops the drag at the edge of the screen, as before.
    orbit_cursor_wrap: bool = true, PAGE_NAVIGATION => check_tr("nav.orbit_cursor_wrap.label");
    /// A right-click in the view with a dialog open confirms its input: on to the next one, then OK.
    /// Off: it does nothing there, as before (#6159).
    right_click_confirms: bool = false, PAGE_GENERAL => check("Right-click confirms dialog inputs", "With a dialog open, a right-click in the view goes on to its next empty input, and on the last one runs OK");
    /// With a dialog open, typing the first letter of a button's label presses it: O for OK,
    /// C for Cancel (#9841). Off by default: typing does nothing to the buttons, as before.
    /// A letter that fits two buttons does nothing, and a focused text field keeps the keystroke.
    dialog_first_letter_buttons: bool = false, PAGE_GENERAL => check_id("prefs.dialog_first_letter_buttons");
    /// View orientation widget: original signed axes (default) or compact cube.
    navigator_style: NavigatorStyle = NavigatorStyle::default(), PAGE_NAVIGATION;
    /// The custom view gizmo's colours: the cube's (themed) or the body's own (modeled).
    navigator_texture: NavigatorTexture = NavigatorTexture::default(), PAGE_NAVIGATION;
    /// The custom view gizmo has the cube's orbit and roll arrows around it.
    navigator_arrows: bool = true, PAGE_NAVIGATION;
    /// The custom view gizmo has the cube's Home button beside it.
    navigator_home: bool = true, PAGE_NAVIGATION;
    /// Show the radial ring and enable hold/flick gestures for viewport context menus.
    radial_context_menu: bool = false, PAGE_NAVIGATION => check("Radial context menu");
    /// Work on the model directly in the viewport (pull a face to extrude or cut); off by default.
    direct_interaction: bool = false, PAGE_TOOLS;
    /// Mouse gestures: a ring of tools on the right button.
    mouse_gestures: crate::mouse_gesture::Config = Default::default(), PAGE_NAVIGATION;
    /// The wheel zooms out when turned forward.
    zoom_reverse: bool = false, PAGE_NAVIGATION => check("Reverse zoom direction");
    /// Maya Alt+right drag uses horizontal motion; off restores the former vertical zoom.
    maya_horizontal_zoom: bool = true, PAGE_NAVIGATION => check_id("nav.maya_horizontal_zoom.label");
    /// What an orbit turns around.
    orbit_around: OrbitAround = OrbitAround::default(), PAGE_NAVIGATION;
    /// The cursor preference as releases before the origin option saved it (`true` = the point
    /// under the cursor). [`migrate`] folds it into `orbit_around` when saved preferences are
    /// read, so an existing choice survives the upgrade; this version only writes `false`.
    orbit_cursor: bool = false, PAGE_NAVIGATION;
    /// Whether a connected 3D mouse moves the view.
    space_mouse_enabled: bool = true, PAGE_MOUSE3D => check("Navigate with the 3D mouse");
    /// Whether a picture of the connected 3D mouse, moving as it does, sits in the 3D view's lower
    /// left corner. Off by default.
    space_mouse_in_view: bool = false, PAGE_MOUSE3D => check_id("ui.3d_mouse_show_in_view");
    /// 3D mouse orbit: traced (on) turns the view around the model in the middle of the view when
    /// the turn begins (else the middle of the bodies in view), marked by a faint ball; classic
    /// (off, the default) turns it around the view's centre, as it did before.
    space_mouse_model_pivot: bool = false, PAGE_MOUSE3D;
    /// Whether the 3D mouse also turns the view while a sketch is edited and still seen square on
    /// (#2227), as the mouse can. Off (the default): the cap pans and zooms a square-on sketch and
    /// turns it only after the view is tilted away from it.
    space_mouse_sketch_orbit: bool = false, PAGE_MOUSE3D => check_id("ui.3d_mouse_sketch_orbit");
    /// Speed multiplier for 3D mouse navigation.
    space_mouse_speed: f32 = 1.0, PAGE_MOUSE3D;
    /// Per physical axis of the cap (push X, Y, Z, then tilt X, Y, Z), whether it moves the view
    /// the other way.
    space_mouse_reverse: [bool; 6] = [false; 6], PAGE_MOUSE3D;
    /// Per physical axis of the cap, a multiplier on top of `space_mouse_speed`; 0 switches it off.
    space_mouse_axis_speed: [f32; 6] = [1.0; 6], PAGE_MOUSE3D;
    /// Per physical axis of the cap, which of its motions drives it (`space_mouse::reassigned`),
    /// applied after calibration; each motion drives exactly one axis.
    space_mouse_axis_source: [usize; 6] = crate::space_mouse::SAME_AXES, PAGE_MOUSE3D;
    /// Which program reads the 3D mouse when spacenavd and the device itself are both there (Linux).
    space_mouse_driver: SpaceMouseDriver = SpaceMouseDriver::default(), PAGE_MOUSE3D;
    /// Where the 3D mouse picture sits in the 3D view, when it is shown there (`space_mouse_in_view`).
    space_mouse_ghost_position: SpaceMouseGhost = SpaceMouseGhost::default(), PAGE_MOUSE3D;
    /// The user turned the calibration offer off for good ("Never show again"). Unlike closing the
    /// card, which only records that device, this stops every offer, for every device: it is one flag,
    /// not kept per device key. Like the settings beside it, the dialog's Restore Defaults puts it back
    /// (the 3D mouse section's own restore leaves it).
    space_mouse_offer_never: bool = false, PAGE_MOUSE3D;
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
    nav_bridge_enabled: bool = false, PAGE_MOUSE3D;
    /// The loopback port the device app listens on (0: none).
    nav_bridge_port: u16 = 8765, PAGE_MOUSE3D;
    /// Anti-aliasing samples (applies on restart).
    msaa: u32 = 4, PAGE_GRAPHICS;
    /// Share shading and Continuity; false preserves their independent legacy behavior.
    single_surface_analysis: bool = true, PAGE_GRAPHICS => check_id("prefs.graphics_page.single_surface_analysis");
    /// Linux: the window system the app opens with at its next start (#3083): `auto` (XWayland
    /// whenever there is an X display, native Wayland on niri and Hyprland), `wayland` or `x11`.
    window_system: String = "auto".into(), PAGE_GRAPHICS;
    /// Graphics API for the next start; Automatic preserves the platform startup order.
    graphics_backend: String = "auto".into(), PAGE_GRAPHICS;
    /// Exact graphics adapter name; empty selects automatically on the next start.
    graphics_adapter: String = String::new(), PAGE_GRAPHICS;
    /// Soft contact shading in the GPU viewport; optional and applied immediately.
    ambient_occlusion: bool = false, PAGE_GRAPHICS;
    /// Restore the shadow plane at the lowest visible body, including above the origin (#9838).
    ground_shadow_follows_model: bool = false, PAGE_GRAPHICS => check_id("prefs.ground_shadow_follows_model");
    /// Shade the viewport in linear light with a sky fill, so dark parts keep their shape
    /// (#7974). Off by default: the viewport shades as it always has.
    dark_parts_shading: bool = false, PAGE_GRAPHICS => check_id("prefs.dark_parts_shading");
    /// Optional schematic markings on threaded cylindrical walls; off for new and older preferences.
    cosmetic_threads: bool = false, PAGE_GRAPHICS;
    /// A section hatches each cut body its own way (the default), or every one alike, in the cap
    /// colour at 45 degrees, as before.
    section_hatch: SectionHatch = SectionHatch::ByPart, PAGE_GRAPHICS;
    /// The grid turns to the upright origin plane facing a side view (#1470). Off by default: the
    /// grid stays on the ground plane (XY) whatever the view.
    grid_follows_view: bool = false => check_id("ui.grid_follows_view");
    /// Draw reference geometry such as inactive sketches and datum planes through shaded bodies.
    /// Off by default to preserve the depth-tested viewport behaviour.
    reference_geometry_through_bodies: bool = false, PAGE_GRAPHICS => check_id("prefs.graphics_page.reference_geometry_through_bodies");
    /// A small dot at every vertex wherever edges are drawn (#7834). Off by default: the viewport
    /// draws edges only, as it always has.
    vertex_markers: bool = false;
    /// A preview of a feature that changes a body draws the surface it adds in the preview
    /// colour, working out face by face which surface that is. Off: the body is drawn as itself
    /// and nothing is worked out (`preview::Colors::tint_new`).
    preview_tint_new_faces: bool = true, PAGE_GRAPHICS => check_id("prefs.preview_tint_new_faces");
    /// A preview that removes material shows the removed shape as a red see-through ghost. Off:
    /// the ghost is fully transparent and only its outline is drawn (`preview::Colors::cut_tint`).
    cut_preview_tint: bool = true, PAGE_GRAPHICS => check_id("prefs.cut_preview_tint");
    /// Restore the previous platform-specific fixed Render preview geometry budget.
    render_previous_geometry_budget: bool = false, PAGE_GRAPHICS => check_id("prefs.render_previous_geometry_budget");
    /// Restore Raster for new Render setups, fixed CPU sampling and its mode selector.
    /// Refit on entry and keep shot activation camera-neutral, with the previous preview
    /// framing, navigation, picking and selection overlays. Disable automatic live feedback
    /// and automatic result opening; retain the previous result/cancel viewing behavior.
    /// GPU sample edits round to powers of two.
    render_legacy_image_defaults: bool = false, PAGE_GRAPHICS => check_id("prefs.render_legacy_image_defaults");
    /// Restore native software-pool admission without refusing an available-RAM shortage.
    /// Shared/host software limits and device/allocation checks remain in force.
    render_legacy_memory_admission: bool = false, PAGE_GRAPHICS => check_id("prefs.render_legacy_memory_admission");
    /// How finely the viewport tessellates curved faces and edges: `coarse`, `normal`, `fine` or
    /// `ultra` (#1287). Only the display plan the viewport draws; never the design or exports.
    display_quality: String = "normal".into(), PAGE_GRAPHICS;
    /// Supersampling (#1288): the view drawn at this many times its pixels each way and filtered
    /// down (1, 1.5 or 2; 1 is off).
    supersampling: f32 = 1.0, PAGE_GRAPHICS;
    /// Supersample only while the view holds still (full speed while orbiting).
    supersampling_adaptive: bool = true, PAGE_GRAPHICS => check_tr("prefs.graphics_page.supersampling_adaptive");
    /// Show the frame rate in the view's corner (the view then redraws every frame).
    show_frame_rate: bool = false, PAGE_GRAPHICS;
    /// Record where the time goes (SMLib's calls, kernel operations, features, commands, imports)
    /// for `diagnostics.profile` (docs/perf.md). Off by default: nothing is recorded.
    profile_operations: bool = false, PAGE_GRAPHICS => check_tr("prefs.profile_operations");
    /// With the frame rate: a frame-time graph, a histogram and what the GPU drew (#2548).
    frame_rate_details: bool = false, PAGE_GRAPHICS;
    /// Draw a wider rim under finished sketch curves to improve contrast on shaded faces.
    sketch_halo: bool = false, PAGE_GRAPHICS => check("Halo under sketch curves");
    /// How joints look in the viewport: 0 the ring and axis drawn before badges, or a badge with
    /// the joint type's pictogram: 1 white on dark, 2 solid accent, 3 solid amber, 4 outline,
    /// 5 a colour and outline for each family of types, 6 dots (`joint_symbols::STYLES`).
    joint_symbols: u8 = 0, PAGE_GRAPHICS, accept(0..=6);
    /// Joint badges draw quiet unless the joint lets its parts move, which shows what a
    /// construction has fully constrained. Off: every joint draws alike. The ring and axis
    /// style draws every joint alike either way.
    highlight_moving_joints: bool = true, PAGE_GRAPHICS => check_id("prefs.highlight_moving_joints");
    /// Show the view's pitch, yaw and roll and its projection under the orientation gizmo (#6032).
    /// Off by default; View ▸ View angles and `view.angle_readout` turn it on.
    show_view_angles: bool = false, PAGE_GRAPHICS;
    /// Show world X/Y/Z component dimensions beside the total distance in Measure.
    measure_axis_distances: bool = true, PAGE_TOOLS;
    /// Perspective view's vertical field of view in radians (#5364), 15° to 90°. The view reads a
    /// saved value outside that range as the default ([`perspective_fov`]).
    perspective_fov: f64 = PERSPECTIVE_FOV, PAGE_GRAPHICS, accept(15f64.to_radians()..=90f64.to_radians());
    /// Decimals shown for lengths and angles.
    length_decimals: u8 = 2, PAGE_DESIGN;
    angle_decimals: u8 = 1, PAGE_DESIGN;
    /// The most parameters Down arrow lists in an empty expression box (#9246): fewer keeps the
    /// list light in a large design.
    parameter_list_rows: u16 = 50, PAGE_DESIGN;
    /// Up/Down and the wheel over a focused value field step its plain number (`value_step`,
    /// #2320). Off by default: the keys move the text cursor and the wheel scrolls, as before.
    step_values: bool = false, PAGE_DESIGN => check_id("ui.step_values");
    /// When SMLib's full check runs while features are computed (`diagnostics.validation`):
    /// `each_feature` (the default) checks the bodies each feature keeps once; `each_step`, as
    /// before, checks after every sweep, boolean and blend; `off` (experimental) never does.
    validation: solidcraft_engine::kernel::Validation = solidcraft_engine::kernel::Validation::EachFeature, PAGE_DESIGN;
    /// A command's value (Extrude's Distance, Fillet's Radius…) in a box beside its handle in the
    /// view (`canvas::boxes_shown`). Off: it is typed in the command window only, which takes the
    /// keyboard after a pick instead, and nothing covers the model (#9479).
    value_boxes_in_view: bool = true, PAGE_DESIGN => check_id("prefs.value_boxes_in_view");
    /// The Parameters dialog applies an expression as soon as its field is left (#9279). Off: the
    /// edits wait in the dialog until Apply or OK sets them together, in one rebuild and one undo
    /// step. Its checkbox is in the dialog itself.
    parameters_apply_live: bool = true;
    /// A design coming back to its tab reads again the linked designs whose files changed since
    /// it last read them. Off: they wait for Update Links, as before.
    update_changed_links: bool = true, PAGE_DESIGN => check_id("settings.update_changed_links");
    /// Insert STEP keeps its disk source for Update Links and automatic refresh. Off: new
    /// inserts are independent copies, as before; existing links are left alone.
    link_step_inserts: bool = true, PAGE_DESIGN => check_id("settings.link_step_inserts");
    /// A double-click on a component, in the browser or on its face in the view, opens a linked
    /// one's design with the assembly around it (`edit_in_place`) and activates any other. Off:
    /// a double-click in the browser activates every component and one in the view does nothing,
    /// as before.
    double_click_components: bool = true, PAGE_DESIGN => check_id("settings.double_click_components");
    /// Snap points (#5912): wherever a point is picked (Move, Align and Measure so far, #5881),
    /// hovering an edge, face or sketch curve shows its centre, middle and end points to snap to. On by default; off, a pick takes only
    /// what is under the pointer, as before.
    snap_points: bool = true, PAGE_TOOLS => check_id("settings.snap_points");
    /// Insert Design lists the designs open in other tabs, each inserted with one click, then
    /// Browse… for a file (#8570). Off: it opens the file picker at once, as before.
    insert_lists_open_designs: bool = true, PAGE_DESIGN => check_id("settings.insert_lists_open_designs");
    /// Dragging a part a ball joint holds to a part fixed in place turns it like a trackball: by
    /// the smallest turn about the joint, on a sphere at least the part's size (`joint_drag`,
    /// #13262). Off: the grabbed point goes to the pointer and the part may spin about it, as
    /// before.
    joint_drag_trackball: bool = true, PAGE_DESIGN => check_id("prefs.joint_drag_trackball");
    /// A saved design's tab and its Browser top row are named after its file, so a Save As or a
    /// renamed file shows (#7267). Off: both show the name stored in the design, as before.
    names_from_file: bool = true, PAGE_DESIGN => check_id("settings.names_from_file");
    /// Reopen the saved designs that were open when the app was last closed (#5542).
    reopen_designs: bool = true, PAGE_GENERAL;
    /// Ask separately when closing several unsaved designs, as before grouped saving.
    save_prompt_one_at_a_time: bool = false, PAGE_GENERAL => check_id("prefs.save_prompt_one_at_a_time");
    /// Those designs' files, in tab order, as the last quit left them.
    open_designs: Vec<String> = Vec::new(), PAGE_GENERAL;
    /// Web app (#10220): keep each open design's unsaved changes current in the browser's
    /// private storage, so closing or reloading the tab doesn't ask through the browser's own
    /// "leave site?" box; the next visit offers them back. Off: the browser asks, as before.
    keep_unsaved_in_browser: bool = false, PAGE_GENERAL;
    keep_unsaved_also_asks: bool = true, PAGE_GENERAL;
    /// The largest STEP or import file opened, in MB (#4086): from the default (512) up to 16 GB.
    /// `--max-file-size` overrides it for one run. The browser keeps the default.
    max_step_file_mb: u64 = DEFAULT_STEP_MB, PAGE_GENERAL;
    /// The largest design file opened or saved, in MB (#4086): from the default (256) up to 16 GB.
    max_design_file_mb: u64 = DEFAULT_DESIGN_MB, PAGE_GENERAL;
    /// A FreeCAD file opens or inserts as its sketches and features, rebuilt; off brings in only
    /// its solids as one Import feature, as before.
    freecad_history: bool = true, PAGE_GENERAL => check_id("settings.freecad_history");
    /// Explicit assembly Move uses joint handles; off keeps the previous Move panel.
    assembly_move_handles: bool = true, PAGE_TOOLS => check_id("joint_move.use_handles");
    /// Degrees per step near the assembly Move ring.
    joint_rotation_coarse: f64 = 10.0, PAGE_TOOLS;
    /// Degrees per step after pulling the cursor outside the precision guide.
    joint_rotation_fine: f64 = 0.1, PAGE_TOOLS;
    /// `dark`, `light` or `system` (follow the operating system), or a theme file's id: a
    /// built-in one (`gruvbox-dark`, …) or one of `custom_themes` (#2255).
    theme: String = "dark".into(), PAGE_APPEARANCE;
    /// A picture behind the model in place of the view's background; empty for none.
    background_image: String = String::new(), PAGE_APPEARANCE;
    /// Theme used when the operating system is light; unknown ids resolve to Light.
    theme_day: String = "light".into(), PAGE_APPEARANCE;
    /// Theme used when the operating system is dark; unknown ids resolve to Dark.
    theme_night: String = "dark".into(), PAGE_APPEARANCE;
    /// The 3D view's colours: the theme's own, or the Light or Dark theme's whatever the window
    /// uses (#4146).
    viewport_theme: crate::theme::ViewportTheme = crate::theme::ViewportTheme::Theme, PAGE_APPEARANCE;
    /// The 3D view's background, one colour for the whole application (#7688), or `None` (the
    /// default) for the theme's gradient.
    viewport_background: Option<[u8; 3]> = None, PAGE_APPEARANCE;
    /// The application bar's logo in the theme's accent instead of the theme's logo colours.
    logo_accent: bool = false, PAGE_APPEARANCE;
    /// Imported theme files (`theme_files`), kept here so they also work in the browser.
    #[serde(deserialize_with = "themes_that_read")]
    custom_themes: Vec<crate::theme_files::ThemeFile> = Vec::new();
    /// Desktop: a `solidcraft-theme/1` file another tool writes, followed as the theme and
    /// reloaded when it changes (#7573). Empty (the default): no file is followed.
    theme_follow: String = String::new(), PAGE_APPEARANCE;
    /// Per-theme overrides. Raw JSON retains entries from newer or hand-edited settings.
    theme_options: BTreeMap<String, BTreeMap<String, serde_json::Value>> = BTreeMap::new(), PAGE_APPEARANCE;
    /// Interface language: a registered locale ID, or `system` to follow the system's language
    /// on every start (#10138). A saved language keeps winning, so existing users don't change.
    language: String = "system".into(), 0;
    /// Shared workspace composition; system windows and popups retain their purpose.
    layout: crate::ui_kit::LayoutMode = Default::default(), PAGE_LAYOUT;
    /// Preview panel placement and reflow while dragging. Off shows the drop outline.
    live_panel_drag_preview: bool = true, PAGE_LAYOUT;
    /// Ordered view controls and their appearance.
    #[serde(default)]
    navigation_bar: crate::navigation_bar::NavigationBar = Default::default(), PAGE_LAYOUT;
    /// Show command names beneath ribbon icons, independently of workspace layout.
    ribbon_labels: bool = true, PAGE_LAYOUT;
    /// Show engine-derived persistent selection in the ribbon; off retains neutral buttons.
    persistent_ribbon_pressed: bool = true, PAGE_LAYOUT;
    viewport_analysis_status: bool = true, PAGE_GRAPHICS;
    viewport_section_status: bool = true, PAGE_GRAPHICS;
    viewport_status_details: bool = true, PAGE_GRAPHICS;
    viewport_status_summary: bool = true, PAGE_GRAPHICS;
    /// Keep the SKETCH tab in the ribbon, right after MODEL, while no sketch is open; its drawing
    /// tools then start a sketch first (#7904).
    sketch_tab_always: bool = false, PAGE_LAYOUT;
    /// Each design tab keeps its open dialog or tool, and its toolbar tab, while another design is
    /// shown, and has them back when it is shown again (#9360). Off by default: a switch cancels
    /// them and shows the MODEL tab.
    keep_work_on_tab_switch: bool = false, PAGE_LAYOUT;
    /// Hovering a result in Search commands shows the command's place in the ribbon: its tab,
    /// its button or its row in a section's drop-down (#7287). A command started from the search
    /// blinks there, and a running command that sits in a drop-down marks its section label. Off
    /// by default: the ribbon stays as it was.
    ribbon_reveal: bool = false, PAGE_LAYOUT;
    /// Fold the ribbon to its tab row, so the view has the room its panels took (#10112). A tab's
    /// click shows its panels over the view until a command starts or the pointer leaves them.
    /// Off by default: the whole ribbon shows. `view.ribbon_tabs_only` switches it.
    ribbon_tabs_only: bool = false, PAGE_LAYOUT;
    /// A double-click on a design tab renames the design in the tab; off, it maximizes the window
    /// as the rest of the title bar does, as before.
    tab_double_click_renames: bool = true, PAGE_LAYOUT;
    /// Where a dialog's OK and Cancel sit: under its inputs (the default) or in its title row (#6708).
    dialog_buttons: crate::ui_kit::DialogButtons = Default::default(), PAGE_LAYOUT;
    /// Where notices and offers stack: the view's bottom-right corner (the default) or the bottom
    /// centre, as before #7893.
    toast_place: crate::viewport::ToastPlace = Default::default(), PAGE_LAYOUT;
    /// The operations under way, with their progress and Cancel, in a panel in the view's
    /// bottom-right corner. Off: a "Computing" notice and the agent's chip, as before.
    operations_panel: bool = true, PAGE_LAYOUT;
    /// While a file opens, is inserted or its links are read again, the design waits for it
    /// behind a card with the file's progress and Cancel, as it always did. Off (the default): the
    /// operation is a row of the operations panel and the design stays usable meanwhile.
    file_operations_wait: bool = false, PAGE_LAYOUT;
    /// Toolbar tabs hidden by the person. An empty list keeps every existing tab visible.
    hidden_toolbar_tabs: Vec<String> = Vec::new(), PAGE_LAYOUT;
    /// Show the ribbon's panel drop-downs as a grid of large icons, their names in the tooltips,
    /// instead of a list of small icons with names (#4739).
    ribbon_icon_grid: bool = false, PAGE_LAYOUT;
    /// Opt in to entity headings in Construct menus; off keeps the original command order.
    construct_menu_groups: bool = false, PAGE_LAYOUT => check_id_help("prefs.construct_menu_groups", "prefs.construct_menu_groups_help");
    /// After a ribbon panel menu is open, hover another panel to switch to its menu.
    ribbon_menus_follow_hover: bool = false, PAGE_LAYOUT;
    /// Leave the commands the ribbon shows as buttons (pinned ones) out of its panel drop-downs,
    /// which then list only the rest (#4739).
    ribbon_menus_skip_pinned: bool = false, PAGE_LAYOUT;
    /// Interface scale (1 = 100 %).
    ui_scale: f32 = 1.0, PAGE_APPEARANCE, accept(0.5..=2.5);
    /// Interface font: an installed font family (or a font file), or empty for the built-in one
    /// (#1290). Menus, dialogs and labels use it; glyphs it lacks fall back as before.
    ui_font: String = String::new(), PAGE_APPEARANCE;
    /// While a dialog or a sketch is open, OK and Cancel in the view's top right corner, with the
    /// axis view moved down below them (#6709). Off by default.
    view_confirm_corner: bool = false, PAGE_LAYOUT;
    /// A thin bar along the window's bottom edge: what is being edited, the open sketch's state
    /// and the design's units (#7243). Off by default.
    status_bar: bool = false, PAGE_LAYOUT;
    /// The selection's length, area or distance in the view's bottom right corner (#7000). Off by
    /// default.
    quick_measure: bool = false, PAGE_GRAPHICS;
    /// Compact history: the browser lists each component's features in history order with the
    /// marker, and neither timeline is shown.
    history_in_browser: bool = false, PAGE_LAYOUT;
    /// Whether vertical History lists a sketch under the feature built from it.
    history_sketches: HistorySketches = HistorySketches::default(), PAGE_LAYOUT;
    /// A Play History button in the History controls, between Previous and Next (#3688). Off
    /// gives the four controls History had before; right-clicking the controls toggles it too.
    history_play_button: bool = true, PAGE_LAYOUT;
    /// A History folder whose features are all of one kind (Holes, say) shows their icon instead of
    /// the folder icon, in both Histories. Off by default: every folder shows the folder icon.
    history_folder_kind_icon: bool = false, PAGE_LAYOUT;
    /// The History controls (First, Back, Forward, Last) step through the features the bottom bar
    /// shows: one component's while it shows one (#11430). Off: they step through every feature of
    /// the design, as they did before.
    history_steps_component: bool = true, PAGE_LAYOUT;
    /// On, two or more joints show as one button with their count in the History, and a click
    /// shows each one (#8884). Off, the default, shows each joint as the History always has.
    history_group_joints: bool = false, PAGE_LAYOUT;
    /// On, the History bar folds long runs of one component's features into chips and starts
    /// feature folders folded, and a STEP insert that brings more than one item goes into a
    /// `Group #N` folder (#8885). Off, the default, shows each feature, starts folders open and
    /// makes no folder, as the bar always has.
    history_bar_groups: bool = false, PAGE_LAYOUT;
    /// Vertical History starts with the active component's Bodies folder and the Origin (#6490).
    timeline_origin_bodies: bool = false, PAGE_LAYOUT;
    /// Render shell preference, independent of the document and Design history.
    render_shots_strip: bool = true;
    /// Resource identities and history belong to the user, never the design.
    render_library_favorites: std::collections::BTreeSet<(String, String)> = Default::default();
    render_library_recent: std::collections::VecDeque<(String, String)> = Default::default();
    /// The names the browser, dialogs and lists show for the origin planes (#6491).
    origin_plane_names: OriginPlaneNames = OriginPlaneNames::default(), PAGE_DESIGN;
    /// When components' bodies sit in a Bodies folder in the browser (#4217).
    browser_bodies_folders: BodiesFolders = BodiesFolders::Several, PAGE_LAYOUT;
    /// Each tree row's eye between its fold arrow and its radio, as before #8718, instead of in a
    /// column at the rows' right end. Off by default.
    browser_eye_left: bool = false, PAGE_LAYOUT;
    /// Every component placement in the browser starts open, as before #9274, instead of closed
    /// until the user opens it. Off by default.
    browser_components_open: bool = false, PAGE_LAYOUT;
    /// Each body row in the Browser folds open on the timeline features that built it, in
    /// timeline order (#11945). Off by default: body rows have no fold arrow, as before.
    browser_body_features: bool = false, PAGE_LAYOUT;
    /// Folders in the Browser start closed, and placements too whatever "Components start open"
    /// says, and they open only on request (#8768): with their arrow, Expand All or Find in
    /// Browser, which opens just the rows down to its body and closes again the ones the last find
    /// opened. A body picked in the 3D view leaves the Browser as it is, and only the body Find in
    /// Browser found has the rows above it marked, so a large assembly's placements are not named
    /// again after each edit. Off by default: folders start open, placements as "Components start
    /// open" says, and a pick in the 3D view opens the Browser down to its body (#3005), as before.
    browser_rows_start_closed: bool = false, PAGE_LAYOUT;
    /// The wheel zooms toward the point under the cursor (else toward the view's centre).
    zoom_to_cursor: bool = true, PAGE_NAVIGATION => check("Zoom to the cursor", "The wheel zooms toward the point under the cursor (off: toward the view's center)");
    /// Speed multiplier for wheel zoom, independent of the 3D mouse.
    wheel_zoom_speed: f32 = 1.0, PAGE_NAVIGATION, accept(0.25..=4.0) => slider("ui.wheel_zoom_speed", 0.25..=4.0, 0.05, |v| format!("{v:.2}×"), "prefs:wheel_zoom_speed");
    /// Opt-in surface anchoring for the 3D mouse, navigation bridge and drag-zoom. The wheel
    /// and pinch retain their independent `zoom_to_cursor` preference.
    zoom_stop_at_model: bool = false, PAGE_NAVIGATION => check_id("prefs.zoom_stop_at_model");
    /// Spread discrete wheel input across frames (off: apply each event immediately).
    smooth_wheel_zoom: bool = true, PAGE_NAVIGATION => check("Smooth wheel zoom", "Turn off to apply each wheel event immediately, without a smoothing tail");
    /// The face under the pointer keeps its highlight while the view moves under it (#2130).
    keep_hover_while_navigating: bool = true, PAGE_NAVIGATION;
    /// Two-finger trackpad scroll pans (off: it zooms like the wheel); pinch always zooms.
    trackpad_pan: bool = true, PAGE_NAVIGATION;
    /// Hold this modifier alone to zoom with vertical wheel or two-finger scroll.
    trackpad_zoom_modifier: TrackpadZoomModifier = TrackpadZoomModifier::default(), PAGE_NAVIGATION;
    /// Horizontal trackpad and thumb-wheel scrolling pans the view the other way (#2007).
    pan_reverse_x: bool = false, PAGE_NAVIGATION => check("ui.reverse_horizontal_pan_direction", "Horizontal two-finger scrolling and the thumb wheel pan the other way");
    /// Vertical trackpad scrolling pans the view the other way (#5240).
    pan_reverse_y: bool = false, PAGE_NAVIGATION => check("ui.reverse_vertical_pan_direction", "Vertical two-finger scrolling pans the other way");
    /// Shift with two-finger trackpad scrolling orbits the view the other way, both ways at once,
    /// as natural scrolling turns it (#8410).
    orbit_reverse: bool = false, PAGE_NAVIGATION => check("ui.reverse_trackpad_orbit_direction", "Shift with two-finger scrolling orbits the other way");
    /// A two-finger twist on the trackpad turns the view about the world's up axis (#6286). Off
    /// by default: the twist does nothing, as before.
    trackpad_twist_turns: bool = false, PAGE_NAVIGATION => check_id_help("prefs.trackpad_twist_turns", "prefs.trackpad_twist_turns_tip");
    /// View changes (axis view, Look At, Home) animate; `look_at_speed` scales how fast.
    look_at_anim: bool = true, PAGE_NAVIGATION => check("Animate view changes", "ui.look_at_anim_desc");
    look_at_speed: f32 = 1.0, PAGE_NAVIGATION, accept(0.25..=4.0) => enabled_slider(look_at_anim, "Animation speed", 0.5..=3.0, 0.25, |v| format!("{v:.2}×"));
    /// Default modelling orientation: `z` (Z up) or `y` (Y up) (#3).
    up_axis: String = "z".into(), PAGE_DESIGN;
    /// After a STEP or IGES file opens, offer to stand a Y-up part up (`stand_up`).
    offer_stand_up: bool = true, PAGE_DESIGN => check_tr("ui.offer_to_stand_up_y_up_files");
    /// How the offer is made for a file whose header names no program known to save Y up: a note
    /// over the view (the default), the dialog, or nothing (#7717).
    stand_up_unknown: StandUpUnknown = StandUpUnknown::default(), PAGE_DESIGN;
    /// Dragging Extrude's distance arrow stops at flat faces parallel to the profile over it and
    /// outlines them (`extrude_face_snap`, #2398). Off: the drag moves in round steps only.
    extrude_arrow_face_stops: bool = true, PAGE_TOOLS => check_id("prefs.extrude_arrow_face_stops");
    /// Reapply a file's stand-up answer when its original bytes have not changed.
    remember_stand_up: bool = true, PAGE_DESIGN => check_id("prefs.remember_stand_up");
    /// Reopen an unchanged CAD import from the existing saved-design model cache.
    reuse_imports: bool = true, PAGE_DESIGN => check_id("prefs.reuse_imports");
    /// Oldest-first bounded file answers; resetting settings preserves user history.
    stand_up_answers: crate::stand_up::Answers = Default::default();
    /// Extrude: with a profile picked, a face, vertex or parallel plane picked off its plane
    /// switches the extent to To Object (off: picks only add profiles, as before). A click into the
    /// Profiles box makes the next picks profiles again.
    extrude_auto_to_object: bool = true, PAGE_TOOLS => check_tr("ui.extrude_auto_to_object");
    /// Extrude turns the view to isometric each time it opens, so the depth can be seen (#9176).
    /// Off (the default): the view stays as it is.
    extrude_view_isometric: bool = false, PAGE_TOOLS => check_id("prefs.extrude_view_isometric");
    /// With `extrude_view_isometric`: when Extrude closes (OK or Cancel) the view goes back to
    /// where it was before it turned (#9176). Off (the default): it stays isometric.
    extrude_view_return: bool = false, PAGE_TOOLS => check_id("prefs.extrude_view_return");
    /// Feature windows choose Join, New Body or Cut from what their material touches (`op_auto`,
    /// #6901). Off: they keep the operation they open with (Extrude its own Join or Cut), as before.
    auto_operation: bool = true, PAGE_TOOLS => check_id("prefs.auto_operation");
    /// Reuse a command's safe values and options after it succeeds, until the app closes. Off by default: each dialog opens with its usual defaults.
    remember_dialog_values: bool = false, PAGE_TOOLS => check_id_help("prefs.remember_dialog_values", "prefs.remember_dialog_values.help");
    /// A new Solid Fillet opens at the radius the tool ships with, 2 mm, shown in the design's
    /// unit ("0.07874 in" in an inch design): a radius is bound by the faces beside its edges, and
    /// the number read in the design's unit ("2 in" = 50.8 mm) never fits the part (#12418). On: it
    /// opens at the number 2 read in the design's unit, as before.
    fillet_legacy_radius: bool = false, PAGE_TOOLS => check_id_help("prefs.fillet_legacy_radius", "prefs.fillet_legacy_radius.help");
    /// Placing a Single hole snaps to the middle of the face and the centres of its round edges
    /// (`hole_place`). Off by default: a hole goes where it is clicked.
    hole_snaps: bool = false, PAGE_TOOLS => check("Snap to face and edge centres");
    /// A placed Single hole has a handle that drags it, and a pick elsewhere on a face that has a
    /// hole moves the hole there (`hole_place`). Off by default: a second pick on its face takes
    /// the hole out, and a hole is moved by taking it out and picking again.
    hole_moves: bool = false, PAGE_TOOLS => check("Move holes by clicking or dragging");
    /// A face takes several Single holes: a pick elsewhere on a face that has a hole adds another,
    /// and a pick on a hole takes it out (`hole_place`). Off by default: a face takes one Single
    /// hole, and a second pick on it takes the hole out (or moves it, with `hole_moves`).
    hole_several: bool = false, PAGE_TOOLS => check_id("prefs.design_tools.hole_several");
    /// The holes one Hole command makes (two or more) go into a feature folder in History, closed
    /// at first (`hole_place`). Off by default: each hole is its own entry in History, as before.
    hole_group: bool = false, PAGE_TOOLS => check_id("prefs.design_tools.hole_group");
    /// The browser's Origin folder starts hidden in designs that don't keep its eye themselves.
    hide_origin: bool = false, PAGE_DESIGN => check_tr("ui.hide_origin_by_default");
    /// Handles on the preview of a feature placed on a face (Rest, Boss, Snap Fit, and the Box,
    /// Cylinder, Cone, Sphere, Torus and Coil): a marker that drags it across the model and, where
    /// it can turn in the face, a ring that turns it. Off by default: a Rest, Boss or Snap Fit is
    /// then placed by its click only, as before, and its Position input takes faces only; a
    /// primitive by its fields, its inputs or the selection it opened with.
    placement_handles: bool = false, PAGE_TOOLS => check_tr("ui.placement_handles");
    /// The files a design names (linked designs, font files) may be on any network location, for
    /// organisations that keep their designs on file servers. Off (the default): only on the
    /// servers in `network_hosts`, or one the user allows when asked (`network_prompt`).
    network_locations: bool = false, PAGE_GENERAL => check_tr("prefs.network_locations");
    /// The servers the files a design names may be on ("Always allow this server").
    network_hosts: Vec<String> = Vec::new(), PAGE_GENERAL;
    /// What a middle-button double-click in the view does: `home`, `fit` or `off`.
    middle_double_click: String = "home".into(), PAGE_NAVIGATION;
    /// A view chosen on the navigator (a cube face, corner or arrow, an axis tip) also frames the
    /// whole model, as Home does (#10091). Off by default: the view turns in place, keeping its
    /// target and zoom.
    navigator_fit: bool = false, PAGE_NAVIGATION;
    /// The view cube's directional arrows orbit the camera the way the arrow points, as in
    /// Fusion and Inventor (`invert_navigator_arrows`, #10254). Off by default: an arrow rotates
    /// the cube toward the side it sits on, so the arrow on the left swings the camera left.
    invert_navigator_arrows: bool = false, PAGE_NAVIGATION;
    /// The view cube's two curved arrows roll the view the other way: the clockwise arrow rolls
    /// counterclockwise and the counterclockwise arrow rolls clockwise (`invert_curved_roll_arrows`,
    /// #10483). Off by default: each arrow rolls the view the way it curls.
    invert_curved_roll_arrows: bool = false, PAGE_NAVIGATION;
    /// How much of the view the model fills when Home, Fit, an opened design or a navigator
    /// view change frames it (0.1 to 1; #10091): 1, the default, fills the room between the
    /// panels, the compass and the navigation bar, as before.
    home_fit_size: f32 = 1.0, PAGE_NAVIGATION, accept(0.1..=1.0);
    /// Colour vision mode (#778): `off`, `protan`, `deutan` or `tritan`.
    color_vision: solidcraft_theme::ColorVision = solidcraft_theme::ColorVision::Off, PAGE_APPEARANCE;
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
    /// Personal Sheet Metal Rules, offered by the rule manager of every design in this profile
    /// (#9346). No page reset discards this user-authored library.
    personal_sheet_rules: crate::personal_sheet_rules::Library = crate::personal_sheet_rules::Library::default();
    /// The file name File › Export and Save As Mesh offer: `{design}` and `{body}` stand for the
    /// design's and the exported body's names (`export_name`). Empty: each export offers the name
    /// it did before (the design's, or the first body's for a mesh).
    export_name: String = String::new(), PAGE_DESIGN;
    /// Save as mesh: the last format (`stl` or `3mf`). Older versions also kept a "send to slicer"
    /// choice here (`save_mesh_send`, `save_mesh_slicer`); a settings file that still has them loads,
    /// and they are ignored.
    save_mesh_format: String = "stl".into();
    /// Hidden edges in the current-view DXF export (false: omit, true: dashed).
    view_dxf_dashed: bool = false;
    /// Model-space chord tolerance for current-view DXF curves, in millimetres.
    view_dxf_tolerance: f64 = 0.01;
    /// Where the current-view DXF drawing puts its 0,0: "target" (the point the camera looks
    /// at) or "origin" (the model's origin as seen from the camera).
    view_dxf_zero: String = "target".into();
    /// Send to Slicer: the last slicer used (an installed slicer's id or a program path).
    send_slicer: String = String::new();
    /// Slicers that can load a file into the window that is already open do so (default on).
    slicer_single_instance: bool = true, 0;
    /// Export STL, Export 3MF and Send to Slicer start with the design's body in Bodies when it
    /// has just one (#13098). Off: the list starts empty, as before.
    export_selects_only_body: bool = true, 0;
    /// How strongly a print face stands out in the viewport (#8429): the share of the section
    /// colour in its tint, 0 to 1; its outline follows. 0 draws no highlight.
    print_face_highlight: f32 = 0.2, PAGE_GENERAL, accept(0.0..=1.0);
    /// Print faces show in the view at all times and are saved in the design, as before #11512.
    /// Off (the default): they show only while the Send to Slicer window is open, a save leaves
    /// them out, and a design that has them opens without them.
    keep_print_faces: bool = false, PAGE_GENERAL;
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
    ai_features: bool = true, PAGE_ASSISTANT;
    /// The Assistant button in the application bar (the shortcut and View menu entry stay).
    ai_toolbar_button: bool = true, PAGE_ASSISTANT;
    /// An agent's slow commands over the control channel (MCP) compute off the window thread,
    /// so the window stays live while they run (#1989, [`crate::control_job`]), the default. Off:
    /// they run on the window thread, and the window waits for each.
    agent_off_thread: bool = true, PAGE_ASSISTANT;
    /// Let AI agents connect: the localhost control channel (MCP) listens on
    /// `agent_control_port`, unless `--control`/`SOLIDCRAFT_CONTROL_PORT` name one for the run.
    /// Turning it on or off, or changing the port, takes effect at once ([`crate::control_switch`]).
    /// Off until the user turns it on, so a new install listens on no port.
    agent_control: bool = false, PAGE_ASSISTANT;
    /// The control channel's port. The default one takes the next free port when it is taken.
    agent_control_port: u16 = 27878, PAGE_ASSISTANT;
    /// Add SolidCraft's MCP server to the AI apps installed on this computer at start-up
    /// ([`crate::ai_connect`]); off takes the entries out again.
    connect_ai_apps: bool = true, PAGE_ASSISTANT;
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
    agent_connect_at_start: bool = false, PAGE_ASSISTANT;
    agent_chat_open_at_start: bool = false, PAGE_ASSISTANT;
    /// How much the agent may do without asking (`ask`, `edits`, `all`), and per agent.
    ai_permissions: crate::agent_chat::agents::Mode = crate::agent_chat::agents::Mode::Edits, PAGE_ASSISTANT;
    agent_permissions: std::collections::BTreeMap<String, crate::agent_chat::agents::Mode> = Default::default(), PAGE_ASSISTANT;
    /// The model last chosen in the chat, per agent: put back when the agent offers it again.
    agent_models: std::collections::BTreeMap<String, String> = Default::default();
    /// "Always allow" answers: per agent, the tools it may use without asking.
    agent_always_allow: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    /// Whether the chat is in its own window instead of the side panel, and that window's place
    /// and size as the system last reported them (`[x, y, width, height]`, points; `x` can be
    /// negative or past the main screen when the window sits on another one).
    agent_chat_popped: bool = false, PAGE_ASSISTANT;
    agent_chat_window: Option<[f32; 4]> = None, PAGE_ASSISTANT;
    /// Changes the agent may make for one message; 0 is no limit.
    agent_chat_turn_limit: u32 = crate::agent_chat::gate::DEFAULT_TURN_LIMIT, PAGE_ASSISTANT;
    /// Custom instructions (#8220): text the agent chat adds to its system note when a session
    /// starts (`agent_chat::acp::system_note`). No page reset discards this user-authored text.
    agent_instructions: String = String::new();
    /// Compact output for agents connected through the MCP server (`solidcraft-cli mcp`, docs/mcp.md):
    /// `inspect_design` summarises sketches and tool JSON is not pretty-printed. Off: as always.
    /// The server asks once per agent session (`mcp.options`), so a change applies to the next one.
    mcp_compact: bool = false, PAGE_ASSISTANT;

    // ---- Sketch / selection modifier bindings (classic defaults; alternate mapping is opt-in) ----
    /// Modifier that suppresses sketch auto-snap while drawing or dragging a sketch point.
    /// Default: Alt/Option. Set to Ctrl/Cmd for the alternate mapping.
    suppress_sketch_snap: ModifierCombo = ModifierCombo::CTRL_ALT, PAGE_SKETCH;
    /// Temporary horizontal/vertical guides from hovered centres and edge midpoints.
    sketch_tracking_guides: bool = true, PAGE_SKETCH;
    /// While a drawing tool waits for a point, hovered sketch points (ends, corners, free
    /// points) start tracking guides too (#9105). Off: only centres and midpoints, as before.
    sketch_tracking_points: bool = true, PAGE_SKETCH;
    /// Snap a drawing point to where the two most recent centre or midpoint guides cross.
    /// Off: the guides stay visual and the cursor does not move (#8694).
    sketch_recent_snap: bool = false, PAGE_SKETCH;
    /// Drawing lines a point up with the sketch's points on both axes at once, keeping the
    /// horizontal or vertical snap to the previous point, and the click adds the alignments
    /// (#7828). Off by default: only with the previous point, one axis at a time, as before.
    sketch_align_points: bool = false, PAGE_SKETCH;
    /// A point snaps to the crossings of the lines the grid draws, and to the finer lattice
    /// between them. Off: the point snaps to the finer lattice only.
    sketch_snap_to_visible_grid: bool = true, PAGE_SKETCH;
    /// Open Project's options before picking; off restores its previous direct-pick workflow.
    project_options_dialog: bool = true, PAGE_SKETCH;
    /// The most points or curves one sketch holds (#10874): from the default (20000), which
    /// sketches have always had, up to 200000, so a dense SVG or DXF drawing fits in one sketch.
    max_sketch_points: usize = solidcraft_engine::sketch::limits::DEFAULT_POINTS, PAGE_SKETCH,
        accept(solidcraft_engine::sketch::limits::DEFAULT_POINTS..=solidcraft_engine::sketch::limits::CEILING_POINTS);
    /// Whether Alt/Option also frees gizmo / canvas manipulator arrows (Primary always does).
    manipulator_free_alt: bool = true, PAGE_SKETCH;
    /// A sketch's icon in History and the browser takes the colour of its solve state, and its
    /// tooltip names the state (#7245). Off: every sketch has the same icon, as before.
    sketch_state_icons: bool = true, PAGE_SKETCH;
    /// Show the construction-line family in Sketch CREATE; off restores the earlier menu.
    sketch_construction_lines: bool = true, PAGE_SKETCH;
    /// Use the only straight construction line in the preselection as Mirror's reference.
    sketch_mirror_auto_line: bool = true, PAGE_SKETCH;
    /// A sketch tool's shortcut pressed again before the first point steps to the tool's next
    /// creation mode (#2459). Off (the default): the key starts the tool again, as it always has.
    shortcut_cycles_modes: bool = false, PAGE_SKETCH;
    /// A click on the ribbon button of the sketch tool that is running ends the tool, so clicks
    /// in the view select again (#10151). Off: the click starts the tool again, as before.
    ribbon_click_ends_tool: bool = true, PAGE_SKETCH;
    /// Opening or editing a sketch turns the view to face it (the default). Off: the view stays as
    /// it is, and Look At faces the sketch when asked.
    sketch_open_look_at: bool = true, PAGE_SKETCH;
    /// Two quick presses of Esc in a sketch, with nothing left to cancel, finish it (the default,
    /// as before). Off: Esc only cancels the tool, dialog or selection and the sketch stays open;
    /// Finish Sketch ends it (#12069).
    escape_twice_finishes_sketch: bool = true, PAGE_SKETCH;
    /// A sketch started on a face of a body in another component than the active one activates
    /// that component first, so the sketch belongs to it (`sketch.create` `activate_part`,
    /// #10215). Off (the default): the sketch belongs to the active component, as before.
    sketch_face_activates_part: bool = false, PAGE_SKETCH;
    /// While a sketch is open, only sketch keys and keys that work everywhere run; a model
    /// command's key (E, F) does nothing. Off (the default): a model key pressed in a sketch
    /// runs its command, and one that builds a feature finishes the sketch first (#2815).
    sketch_keys_only: bool = false, PAGE_SHORTCUTS;
    /// Where the view goes when a sketch ends (#7038). Keep (the default) leaves it as it is.
    sketch_exit_view: SketchExitView = SketchExitView::default(), PAGE_SKETCH;
    /// A shape drawn in a sketch gets all its dimensions at once: the typed values, and every
    /// other size the shape has at its size as drawn (`sketch.draw.dimensioned` with `as_drawn`).
    /// Shapes `solidcraft_engine::dimension_roles` doesn't list are drawn as before.
    sketch_auto_dimensions: bool = false, PAGE_SKETCH;
    /// With `sketch_auto_dimensions`: new shapes are also placed from the sketch origin until they
    /// are fully constrained (`sketch.draw.dimensioned` with `locate`). A shape already anchored to
    /// the origin or to placed geometry gets nothing more.
    sketch_auto_locate: bool = false, PAGE_SKETCH;
    /// A centre-point arc runs the way the cursor goes round its centre from the start (#6645).
    /// Off: it always runs counter-clockwise from the start, as it did before.
    center_arc_follows_cursor: bool = true, PAGE_SKETCH;
    /// In the Line tool, a press, a drag and the release draw one line from the press to the
    /// release and end there (#7247); clicks still draw a chain. Off (the default): a drag is a
    /// click that moved on the way, placing a point where it is let go (#2448).
    line_drag_draws_one: bool = false, PAGE_SKETCH;
    /// In the Line tool, leaving the chain's last point and resting back on it switches to a
    /// tangent arc, and the same again back to lines (#9489). Off (the default): resting there
    /// does nothing, and tangent arcs are the Arc tool's.
    line_hover_tangent_arc: bool = false, PAGE_SKETCH;
    /// Allow ordinary edge picks through faces in Shaded with hidden edges.
    /// Off preserves the visible-only picking used in v0.30.1.
    select_hidden_edges: bool = false, PAGE_SKETCH;
    /// Include hidden geometry and a small pointer neighbourhood in Select Other.
    select_other_hidden: bool = true, PAGE_SKETCH;
    /// Where a dimension's value is typed: in a box on the dimension (the default), or in a small
    /// window at it whose arrows step the value (#7246, `dim_view::window`).
    dimension_input: DimensionInput = DimensionInput::View, PAGE_SKETCH;
    /// What the dimension window's arrows, Up/Down and the wheel add to a length (millimetres,
    /// converted to the unit the value is written in) and to an angle (degrees).
    dimension_step_mm: f64 = 1.0, PAGE_SKETCH, accept(DIMENSION_STEP_MM);
    dimension_step_deg: f64 = 1.0, PAGE_SKETCH, accept(DIMENSION_STEP_DEG);
    /// The size of a sketch dimension's text as a factor of the theme's size (Preferences › Sketch ›
    /// Dimension text size). 1 draws it as the theme does; it also applies to Show Dimensions
    /// (#14121). A saved value outside the accepted range (a hand edit) reads as 1.
    dimension_text_scale: f32 = 1.0, PAGE_SKETCH, accept(DIMENSION_TEXT_SCALE);
    /// The colour a sketch dimension draws its lines, arrows and value in, or `None` for the
    /// theme's dimension colours (Preferences › Sketch › Dimension color, #14121). Selection, hover
    /// and conflicts keep their own colours.
    dimension_colour: Option<[u8; 3]> = None, PAGE_SKETCH;
    /// The first length dimension of a sketch with nothing sized yet scales the whole drawing to
    /// its value (#1365). Off: it sizes only what it measures (#4109).
    first_dimension_scales: bool = true, PAGE_SKETCH;
    /// With `first_dimension_scales`: the drawing scales about the sketch origin even when nothing
    /// ties it there, so a shape drawn away from the origin moves as it grows (the behaviour
    /// before #11043). Off (the default): such a shape scales about its own middle and stays put.
    first_dimension_scales_about_origin: bool = false, PAGE_SKETCH;
    /// When true: Shift+click adds without removing; Primary toggles.
    /// When false (default): Shift and Primary both toggle.
    selection_shift_add_only: bool = false, PAGE_SKETCH;
    /// Prefer movable sketch points on exact selection ties; off keeps point insertion order.
    prefer_free_sketch_points: bool = true, PAGE_SKETCH;
    /// Opt in to displaying and selecting drawn 3D curve nodes as construction references.
    select_3d_curve_points: bool = false, PAGE_SKETCH;
    /// While the open sketch's constraints conflict, the curves and points a failing constraint
    /// holds, and its glyph or dimension, are drawn in `sketch_conflict` (#7244). Off: only the
    /// glyphs and dimensions are red, in `error`, as before.
    sketch_conflict_colour: bool = true, PAGE_SKETCH;
    /// Hide constraint glyphs when the sketch is zoomed out so far that a millimetre of the plane
    /// is smaller than `sketch_tools::MIN_GLYPH_PX_PER_MM` on screen (#8980) and the sketch spans
    /// less than `sketch_tools::MIN_GLYPH_SPAN_PX` (#10518). Off: glyphs stay at a fixed screen
    /// size at every zoom, as before.
    hide_constraint_glyphs_zoomed_out: bool = true, PAGE_SKETCH;
    /// The Ellipse tool draws the major and minor axes as construction lines with the ellipse, so
    /// its half-axes and axes can be dimensioned. Off: the ellipse alone, and D on it gives its
    /// minor radius, labelled R, as before.
    ellipse_axes: bool = true, PAGE_SKETCH;
    /// In an open sketch with no tool running, the pointer's sketch coordinates beside it (#11070).
    /// Off: only a sketch tool shows them, while it places a point, as before.
    sketch_cursor_coordinates: bool = true, PAGE_SKETCH;
    /// How large constraint glyphs are drawn: a multiple of their 15-pixel badge, letter and icon
    /// included, so they can be read at any zoom (#10794). 1 (the default) is the size they always had.
    constraint_glyph_scale: f32 = 1.0, PAGE_SKETCH, accept(crate::sketch_tools::GLYPH_SCALES) => slider("prefs.constraint_glyph_size", crate::sketch_tools::GLYPH_SCALES, 0.25, |v| format!("{:.0}%", v * 100.0), "prefs:constraint_glyph_size");
    /// Drive Joints shows the moved parts' old place as a ghost (its Show ghost box, #5905).
    drive_joints_ghost: bool = true, PAGE_TOOLS => check_tr("prefs.design_tools.ghost");
    /// While Move's triad drags a component drawn as a layer of its own, what is under the pointer
    /// is still picked and highlighted, as before #7273. Off by default: such a drag picks nothing,
    /// so a step neither meshes the dragged bodies where they stand nor looks up the Browser row a
    /// highlight follows (`drag_layer::holds_pointer`).
    move_drag_highlights: bool = false, PAGE_TOOLS => check_id("prefs.move_drag_highlights");
    /// Plugin ids turned off (Preferences › Plugins). An installed plugin whose id is absent is on.
    disabled_plugins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new(), PAGE_PLUGINS;
    /// User ceilings, not automatic increases to a package's approved execution budget.
    plugin_max_fuel: u64 = solidcraft_engine::plugin::HostLimits::default().max_fuel, PAGE_PLUGINS;
    plugin_max_memory_mib: usize = solidcraft_engine::plugin::HostLimits::default().max_memory_mib, PAGE_PLUGINS;
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
    navbar: navbar::State,
    navbar_scroll: bool,
    /// The window itself: open, the page shown (an index into [`PAGES`]), the search, and
    /// everything as it was when the dialog opened (or at the last Apply), for Cancel.
    pub shell: prefs_window::State<Snapshot>,
    /// The up axis preference the view follows now (applied when it changes, and at startup).
    applied_up: Option<String>,
    /// The Profile operations preference the profiler last followed: applied when it changes,
    /// so `diagnostics.profile` (an agent) and SOLIDCRAFT_PROFILE can switch it in between.
    applied_profile: Option<bool>,
    /// The appearance last asked of the native window ([`sync_window_theme`]).
    window_theme: Option<egui::SystemTheme>,
    /// Last observed OS tone, independent of the selected theme's tone.
    system_theme_dark: Option<bool>,
    /// Import callbacks fill the same slot as a card click; never persisted in preferences.
    theme_picker_night: bool,
    /// The perspective angle the view was last drawn with ([`follow_view_fov`]); 0 before the first.
    drawn_fov: f64,
    /// The search text of the interface font list.
    font_filter: String,
    /// Search text on the Plugins page.
    pub(crate) plugin_query: String,
    pub(crate) plugin_limit_inputs: Option<crate::plugins::LimitInputs>,
    panel_restore_details: bool,
    panel_restore_retry: bool,
    panel_popup_reset: bool,
    /// The followed theme file's state (#7573): its last good theme and why a read failed.
    pub(crate) follow: crate::theme_files::Follow,
    /// Where the theme Import tile was drawn last frame: a file dropped there is a theme (#8226).
    import_tile: Option<egui::Rect>,
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
    if !app.prefs_window.shell.open {
        app.prefs_window.plugin_limit_inputs = None;
    }
    app.prefs_window.shell.open_at(page, PAGES.len());
}

/// The sidebar is drawn before the selected page: defer its Profiles action until that page
/// callback, and leave any search before showing General's existing profile controls.
pub(crate) fn open_profiles(app: &mut SolveApp) {
    app.prefs_window.shell.clear_search();
    open_at(app, PAGE_GENERAL);
}

/// Every setting the dialog changes, for Cancel and Restore Defaults.
#[derive(Clone)]
pub struct Snapshot {
    prefs: Prefs,
    panel_layout: Option<serde_json::Value>,
    panel_layout_state: crate::panel_runtime::PanelLayoutSnapshot,
    panel_visibility: crate::panel_runtime::PanelVisibilitySnapshot,
    /// With no record yet, keep the small deferred legacy import state too.
    panel_layout_deferred: Option<crate::panel_runtime::PanelLayoutPreferences>,
    dark: bool,
    layout: crate::ui_kit::LayoutMode,
    grid: bool,
    navigation_bar: bool,
    tool_cursor: bool,
    shadow: bool,
    perspective: bool,
    style: u8,
    pick_bodies: bool,
    agent: crate::agent_cursor::Settings,
    autosave: f64,
    auto_project: bool,
    project_construction: bool,
    keys: std::collections::BTreeMap<String, Vec<String>>,
    /// What a settings profile also switches: the toolbar, the S box favourites and which profile is active.
    toolbar: serde_json::Value,
    favourites: Vec<String>,
    profile: Option<String>,
}

impl Snapshot {
    fn take(app: &SolveApp) -> Snapshot {
        let panel_layout = crate::panel_runtime::preferences_value(app);
        let panel_layout_deferred = panel_layout.is_none().then(|| app.panel_layout_preferences.clone());
        Snapshot {
            prefs: app.preferences.clone(),
            panel_layout,
            panel_layout_state: crate::panel_runtime::panel_layout_snapshot(app),
            panel_visibility: crate::panel_runtime::PanelVisibilitySnapshot::capture(app),
            panel_layout_deferred,
            dark: app.ui.dark,
            layout: app.ui.layout,
            grid: app.ui.show_grid,
            navigation_bar: app.ui.show_navigation_bar,
            tool_cursor: app.ui.tool_cursor,
            shadow: app.ui.ground_shadow,
            perspective: app.ui.perspective,
            style: app.ui.visual_style,
            pick_bodies: app.ui.pick_bodies,
            agent: app.ui.agent_cursor,
            autosave: app.autosave_minutes,
            auto_project: app.session.auto_project,
            project_construction: app.session.project_construction,
            keys: app.keymap.custom.clone(),
            toolbar: app.toolbar_custom.prefs(),
            favourites: app.sbox.pinned.clone(),
            profile: app.settings_profiles.active.clone(),
        }
    }
    /// Restore ordinary preferences after the panel transaction has succeeded.
    fn restore_other_preferences(self, app: &mut SolveApp) -> Result<(), String> {
        if self.prefs.graphics_backend != app.preferences.graphics_backend && app.services.graphics_path.is_some() {
            save_graphics_backend(app, &self.prefs.graphics_backend)?;
        }
        let graphics = (app.preferences.msaa, app.preferences.window_system.clone(), app.preferences.graphics_adapter.clone());
        let units_changed = app.preferences.default_units != self.prefs.default_units;
        // Update bookkeeping isn't a setting: a check that finished while the dialog was open stays.
        let update = std::mem::take(&mut app.preferences.update);
        let stand_up_answers = std::mem::take(&mut app.preferences.stand_up_answers);
        app.preferences = self.prefs;
        crate::plugins::sync_policy(app);
        self.panel_visibility.restore(app);
        sync_surface_analysis(app);
        crate::render_workspace::apply_memory_policy(app);
        app.preferences.stand_up_answers = stand_up_answers;
        app.preferences.update = update;
        if units_changed {
            default_units_changed(app);
        }
        app.ui.dark = self.dark;
        app.ui.layout = self.layout;
        app.ui.show_grid = self.grid;
        app.ui.show_navigation_bar = self.navigation_bar;
        app.ui.tool_cursor = self.tool_cursor;
        app.ui.ground_shadow = self.shadow;
        app.ui.perspective = self.perspective;
        app.ui.visual_style = self.style;
        app.ui.pick_bodies = self.pick_bodies;
        app.ui.agent_cursor = self.agent;
        app.autosave_minutes = self.autosave;
        set_autosave(app);
        app.session.auto_project = self.auto_project;
        app.session.project_construction = self.project_construction;
        app.keymap.custom = self.keys;
        app.toolbar_custom.load(&self.toolbar);
        app.sbox.pinned = self.favourites;
        app.settings_profiles.active = self.profile.filter(|name| app.settings_profiles.names().contains(name));
        if (app.preferences.msaa, app.preferences.window_system.clone(), app.preferences.graphics_adapter.clone()) != graphics {
            save_graphics(app);
        }
        Ok(())
    }

    /// Whether everything is as in the snapshot: nothing Cancel would put back. Destructured, so
    /// a setting added to the snapshot can't be left out of the comparison.
    fn matches(&self, app: &SolveApp) -> bool {
        let Snapshot {
            prefs,
            panel_layout: _,
            panel_layout_state,
            panel_visibility: _,
            panel_layout_deferred,
            dark,
            layout,
            grid,
            navigation_bar,
            tool_cursor,
            shadow,
            perspective,
            style,
            pick_bodies,
            agent,
            autosave,
            auto_project,
            project_construction,
            keys,
            toolbar,
            favourites,
            profile,
        } = self;
        // Update bookkeeping isn't a setting and Cancel keeps it (`restore_other_preferences`), so it isn't a change.
        let same_prefs = {
            let mut held = prefs.clone();
            held.update.clone_from(&app.preferences.update);
            held.stand_up_answers.clone_from(&app.preferences.stand_up_answers);
            held == app.preferences
        };
        same_prefs
            && crate::panel_runtime::panel_layout_matches(app, panel_layout_state)
            && panel_layout_deferred.as_ref().is_none_or(|deferred| *deferred == app.panel_layout_preferences)
            && *dark == app.ui.dark
            && *layout == app.ui.layout
            && *grid == app.ui.show_grid
            && *navigation_bar == app.ui.show_navigation_bar
            && *tool_cursor == app.ui.tool_cursor
            && *shadow == app.ui.ground_shadow
            && *perspective == app.ui.perspective
            && *style == app.ui.visual_style
            && *pick_bodies == app.ui.pick_bodies
            && *agent == app.ui.agent_cursor
            && *autosave == app.autosave_minutes
            && *auto_project == app.session.auto_project
            && *project_construction == app.session.project_construction
            && *keys == app.keymap.custom
            && *toolbar == app.toolbar_custom.prefs()
            && *favourites == app.sbox.pinned
            && *profile == app.settings_profiles.active
    }
}

/// The saved-settings keys whose values the snapshot holds, as [`SolveApp::prefs`] writes them.
/// While the dialog is open every writer of the settings file saves these instead of the live
/// values, so a change that was never applied can't reach disk through the periodic save, a
/// quit or a window close (#9730). Keys the dialog can't change keep their live values.
pub(crate) fn held_overlay(app: &SolveApp) -> Option<serde_json::Map<String, serde_json::Value>> {
    let s = app.prefs_window.shell.snapshot()?;
    let mut prefs = s.prefs.clone();
    // Same bookkeeping rule as `restore`: a check that finished while the dialog was open stays.
    prefs.update.clone_from(&app.preferences.update);
    prefs.stand_up_answers.clone_from(&app.preferences.stand_up_answers);
    let mut shortcut_box = app.sbox.prefs();
    if let Some(o) = shortcut_box.as_object_mut() {
        o.insert("pinned".into(), serde_json::json!(s.favourites));
    }
    let mut profiles = app.settings_profiles.prefs();
    if let Some(o) = profiles.as_object_mut() {
        o.insert("active".into(), serde_json::json!(s.profile));
    }
    let mut shortcuts = serde_json::Map::new();
    for (id, keys) in &s.keys {
        shortcuts.insert(id.clone(), crate::keymap::keys_value(keys));
    }
    // Bound outside the `json!` macro: the ui-containers guard reads `.shadow` inside a macro
    // as egui's popup shorthand.
    let shadow = s.shadow;
    let mut m = serde_json::Map::new();
    for (key, value) in [
        ("dark", serde_json::json!(s.dark)),
        ("grid", serde_json::json!(s.grid)),
        ("navigation_bar", serde_json::json!(s.navigation_bar)),
        ("tool_cursor", serde_json::json!(s.tool_cursor)),
        ("perspective", serde_json::json!(s.perspective)),
        ("pick_bodies", serde_json::json!(s.pick_bodies)),
        ("auto_project", serde_json::json!(s.auto_project)),
        ("project_construction", serde_json::json!(s.project_construction)),
        ("visual_style", serde_json::json!(s.style)),
        ("shortcut_box", shortcut_box),
        ("shortcuts", serde_json::Value::Object(shortcuts)),
        ("toolbar", s.toolbar.clone()),
        ("settings_profiles", profiles),
        ("autosave_minutes", serde_json::json!(s.autosave)),
        ("ground_shadow", serde_json::json!(shadow)),
        ("agent_cursor", serde_json::json!(s.agent)),
        ("preferences", serde_json::to_value(&prefs).unwrap_or_default()),
    ] {
        m.insert(key.to_string(), value);
    }
    Some(m)
}

/// The outer option means the dialog owns a snapshot; the inner option keeps
/// an absent layout distinct from a present, corrupt JSON null record.
pub(crate) fn held_panel_layout(app: &SolveApp) -> Option<Option<serde_json::Value>> {
    app.prefs_window.shell.snapshot().map(|snapshot| snapshot.panel_layout.clone())
}

/// The colour theme (#2255): Dark, Light, System, the built-in theme files and the imported
/// ones, with Import…, Export… (the theme on now as a complete file, to start a new one from)
/// and Remove (an imported theme). Like the other preferences, a choice applies at once.
fn theme_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    theme_picker::row(app, ui);
}

fn theme_action(app: &mut SolveApp, ui: &mut egui::Ui, action: Option<&str>, action_theme: Option<String>) {
    match action {
        Some("import") => app.pick_open_for(&["json"], |app, path| {
            let Some(path) = path else { return };
            // Through `vfs`, like every other import: in the web app a picked file is there.
            let text = solidcraft_engine::io::vfs::read_to_string(&path, crate::theme_files::MAX_BYTES).map_err(|e| e.to_string());
            import_theme_text(app, text);
        }),
        #[cfg(not(target_arch = "wasm32"))]
        Some("paste") => {
            let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
            if looks_like_theme(&text) {
                import_theme_text(app, Ok(text));
            } else {
                app.set_status(app.tr("prefs.theme_paste_none").to_string(), true);
            }
        }
        Some("export") => {
            let system_dark = system_dark(app, ui.ctx()).unwrap_or_else(|| app.prefs_window.system_theme_dark.unwrap_or(app.ui.dark));
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
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(FOLLOW_AVAILABLE, |ui| {
            let mut on = !path.is_empty();
            let check = ui_kit::checkbox(ui, app.tr("prefs.theme_follow"), &mut on);
            crate::scenario::publish_target("prefs:theme_follow_toggle", check.rect);
            if check.changed() {
                if on {
                    choose = true;
                } else {
                    stop = true;
                }
            }
            ui_kit::help_icon(ui, app.tr("prefs.appearance.follow_help"));
            let button = ui_kit::button(ui, app.tr_label("Choose…"), ui_kit::Button::Small);
            crate::scenario::publish_target("prefs:theme_follow", button.rect);
            choose |= button.clicked();
        });
        ui_kit::settings_state_badge(ui, app.tr("prefs.appearance.desktop_only"), ui_kit::SettingsBadgeTone::Neutral);
        if !path.is_empty() {
            let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui_kit::settings_inline_note(ui, &name);
            ui_kit::help_icon(ui, app.tr(if launch { "prefs.theme_follow_launch" } else { "prefs.theme_follow_note" })).backing_on_hover_text(&path);
        }
    });
    if let Some(error) = error.filter(|_| !path.is_empty()) {
        ui_kit::text(ui, &solidcraft_ui_text::format("prefs.theme_follow_error", &[("error", &error)]), ui_kit::Text::Error("theme_follow"));
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
    let builtin = solidcraft_ui_text::format("sketch_text.builtin_font", &[("family", "IBM Plex Sans")]);
    let current = if app.preferences.ui_font.trim().is_empty() { builtin.clone() } else { app.preferences.ui_font.clone() };
    let families = crate::sketch_text::families(ui.ctx());
    let mut values = vec![(String::new(), builtin.clone(), "prefs:font_builtin".to_string())];
    if let Some(families) = &families {
        values.extend(families.iter().map(|family| (family.clone(), family.clone(), format!("prefs:font:{family}"))));
    }
    let options: Vec<_> = values.iter().map(|(value, label, handle)| ui_kit::SearchOption { value, label, handle: Some(handle) }).collect();
    let chosen = ui_kit::named_control(ui, app.tr_label("Interface font"), |ui| {
        ui_kit::searchable_dropdown(
            ui,
            "pref_ui_font",
            &current,
            &mut app.prefs_window.font_filter,
            &options,
            None,
            families.is_none().then(|| solidcraft_ui_text::tr_label("Listing the installed fonts…")),
        )
    });
    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:widget:{}", solidcraft_ui_text::tr_label("Interface font"))) {
        crate::scenario::publish_target("prefs:ui_font", rect);
        crate::scenario::publish_target(&format!("dialog:{}", app.tr("prefs.font")), rect);
        crate::scenario::publish_rect(&format!("dialog:widget:{}", app.tr("prefs.font")), rect);
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
    if page == PAGE_GRAPHICS
        && app.preferences.graphics_backend != "auto"
        && app.services.graphics_path.is_some()
        && let Err(error) = set_graphics_backend(app, "auto")
    {
        app.set_status(error, true);
        return;
    }
    let (msaa, window_system, adapter) = (app.preferences.msaa, app.preferences.window_system.clone(), app.preferences.graphics_adapter.clone());
    let units = app.preferences.default_units.clone();
    reset_preferences(&mut app.preferences, page);
    if page == PAGE_PLUGINS {
        app.prefs_window.plugin_limit_inputs = None;
    }
    crate::plugins::sync_policy(app);
    sync_surface_analysis(app);
    crate::render_workspace::apply_memory_policy(app);
    if app.preferences.default_units != units {
        default_units_changed(app);
    }
    match page {
        0 => {
            app.autosave_minutes = 5.0;
            set_autosave(app);
        }
        PAGE_APPEARANCE => {
            app.prefs_window.follow.launch = None;
            app.ui.dark = true;
        }
        PAGE_LAYOUT => {
            app.ui.layout = app.preferences.layout;
            app.ui.show_navigation_bar = true;
        }
        PAGE_ASSISTANT => app.ui.agent_cursor = Default::default(),
        PAGE_TOOLS => app.ui.tool_cursor = false,
        PAGE_DESIGN => {}
        PAGE_SHORTCUTS => {
            crate::keymap::stop_capture(app);
            crate::keymap::reset(app, None);
        }
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
            app.session.project_construction = false;
            app.ui.pick_bodies = false;
        }
        _ => {}
    }
}

/// Whether the desktop asks for dark colours, for Theme: System; `None` when nothing says.
/// The host's own reading of the desktop (`Services::desktop_dark`, the freedesktop setting on
/// Linux, #7944) comes first, as the window toolkit does not report it on every desktop
/// (GNOME on Wayland), nor its changes; then the window toolkit's.
pub(crate) fn system_dark(app: &SolveApp, ctx: &egui::Context) -> Option<bool> {
    app.services.desktop_dark.as_ref().and_then(|f| f()).or_else(|| ctx.system_theme().map(|t| t == egui::Theme::Dark))
}

/// The desktop's dark or light preference from the freedesktop settings a Linux host reads
/// (#7944): `appearance` is `org.freedesktop.appearance color-scheme` (1 prefer dark, 2 prefer
/// light, 0 no preference), `gnome` is GNOME's `org.gnome.desktop.interface color-scheme`
/// (`prefer-dark`, `prefer-light`, `default`) and `gtk_theme` its `gtk-theme` name. An explicit
/// preference wins; otherwise a dark-named GTK theme ("Adwaita-dark") means dark, and anything
/// else light, as GTK applications read it (GNOME's Light is `default`). `None` when no setting
/// was read at all.
pub fn desktop_prefers_dark(appearance: Option<u32>, gnome: Option<&str>, gtk_theme: Option<&str>) -> Option<bool> {
    match appearance {
        Some(1) => return Some(true),
        Some(2) => return Some(false),
        _ => {}
    }
    match gnome.map(|s| s.trim().trim_matches('\'')) {
        Some("prefer-dark") => return Some(true),
        Some("prefer-light") => return Some(false),
        _ => {}
    }
    let theme = gtk_theme.map(|s| s.trim().trim_matches('\'').to_ascii_lowercase()).filter(|s| !s.is_empty());
    if theme.as_deref().is_some_and(|t| t.ends_with("-dark") || t.ends_with(":dark")) {
        return Some(true);
    }
    (appearance.is_some() || gnome.is_some() || theme.is_some()).then_some(false)
}

/// Settings that follow the preferences every frame: the theme when it follows the system, and
/// the interface scale.
/// Tell the native window which appearance to take (#8983): the operating system's while the
/// theme follows it, otherwise the theme's own, so the title bar matches. egui's own window sync
/// (`Options::sync_window_theme`) went by the egui theme, which [`solidcraft_theme::apply`] always
/// sets to light or dark. A macOS window told to be light stays light and no longer reports the
/// system appearance changing, so a theme following the system never turned with it.
fn sync_window_theme(app: &mut SolveApp, ctx: &egui::Context, follows_system: bool) {
    let window = if follows_system {
        egui::SystemTheme::SystemDefault
    } else if app.ui.dark {
        egui::SystemTheme::Dark
    } else {
        egui::SystemTheme::Light
    };
    if app.prefs_window.window_theme != Some(window) {
        ctx.options_mut(|o| o.sync_window_theme = false);
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(window));
        app.prefs_window.window_theme = Some(window);
    }
}

pub fn apply(app: &mut SolveApp, ctx: &egui::Context) {
    sync_surface_analysis(app);
    crate::plugins::sync_policy(app);
    apply_network(&app.preferences);
    // The features computed from now on are checked as the preference says.
    app.session.set_validation(app.preferences.validation);
    crate::print_face_ui::sync(&mut app.session, &app.preferences);
    // The profiler records while the preference is on (switched when it changes; at start-up only
    // when on, so SOLIDCRAFT_PROFILE=1 is not undone by the default).
    let profile = app.preferences.profile_operations;
    if app.prefs_window.applied_profile != Some(profile) {
        if profile || app.prefs_window.applied_profile.is_some() {
            solidcraft_engine::profile::set_enabled(profile);
        }
        app.prefs_window.applied_profile = Some(profile);
    }
    apply_file_limits(&app.preferences);
    apply_sketch_limit(&app.preferences);
    // The viewport's display quality (an unknown saved value draws at Normal).
    solidcraft_engine::doc::display_quality::set_current(
        solidcraft_engine::doc::display_quality::DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default(),
    );
    // The view's up axis follows the preference (set in the dialog, by ui.prefs or loaded).
    if app.prefs_window.applied_up.as_deref() != Some(app.preferences.up_axis.as_str()) {
        up_axis_changed(app);
    }
    follow_view_fov(app);
    let os_dark = system_dark(app, ctx).unwrap_or_else(|| app.prefs_window.system_theme_dark.unwrap_or(app.ui.dark));
    app.prefs_window.system_theme_dark = Some(os_dark);
    if app.preferences.theme == "system" {
        let id = theme_picker::active_id(&app.preferences, os_dark);
        app.ui.dark = crate::theme_files::find(id, &app.preferences.custom_themes).map_or(id == "dark", |f| f.base.is_dark());
    }
    // A theme file's palette, in place of its base's (dark or light); the visuals follow. A
    // followed file (#7573) wins while it is followed and has read completely once.
    follow_theme_file(app, ctx);
    let file = active_theme_file(app);
    let custom = crate::theme::prepare_tabs(ctx, file, file.and_then(|f| app.preferences.theme_options.get(&f.id)), || {
        file.map(|f| effective_theme_options(&app.preferences, f).0).unwrap_or_default()
    });
    if let Some(custom) = &custom {
        app.ui.dark = custom.dark;
    }
    // Custom Day/Night palettes still need native OS appearance notifications. Fixing
    // the native window to the slot's tone can stop those notifications on macOS.
    let follows_system = app.preferences.theme == "system" && followed_theme(&app.preferences, &app.prefs_window.follow).is_none();
    if crate::theme::set_file_palette(custom) {
        app.styled = false;
    }
    sync_window_theme(app, ctx, follows_system);
    if solidcraft_theme::set_logo_accent(app.preferences.logo_accent) {
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

/// The user's analysis policy applies immediately to every open design, without document edits.
pub(crate) fn sync_surface_analysis(app: &mut SolveApp) {
    let single = app.preferences.single_surface_analysis;
    app.session.set_single_surface_analysis(single);
    for slot in app.docs.background_mut() {
        slot.session.set_single_surface_analysis(single);
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
    let mode = nav_preset(&app.preferences.nav).mode(button, mods);
    // "Right drag orbits" off leaves the right button to the context menu (#6492).
    if button == NavButton::Right && mode == Some(NavMode::Orbit) && !app.preferences.right_drag_orbits {
        return None;
    }
    mode
}

/// +1, or −1 when horizontal scrolling pans the other way (#2007).
pub fn pan_sign_x(app: &SolveApp) -> f32 {
    if app.preferences.pan_reverse_x { -1.0 } else { 1.0 }
}

/// +1, or −1 when vertical scrolling pans the other way.
pub fn pan_sign_y(app: &SolveApp) -> f32 {
    if app.preferences.pan_reverse_y { -1.0 } else { 1.0 }
}

/// +1, or −1 when Shift with two-finger scrolling orbits the other way (#8410).
pub fn orbit_sign(app: &SolveApp) -> f64 {
    if app.preferences.orbit_reverse { -1.0 } else { 1.0 }
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

/// The model point under a screen position: on a face, vertex, edge or sketch curve, the point
/// the cursor is on (on an edge or a curve too, not its middle, which can be far off along a long
/// one). Over empty space: the centre of the visible bodies and sketches (what Fit frames), or
/// the view's centre when nothing is visible.
pub(crate) fn pivot_under(app: &SolveApp, proj: &crate::viewport::Proj, at: Pos2) -> Vec3 {
    use crate::viewport::Hit;
    for h in crate::viewport::pick(app, proj, at) {
        match h {
            Hit::Face { point, .. } | Hit::Vertex { point, .. } | Hit::Edge { point, .. } | Hit::SketchCurve { point, .. } => {
                return point;
            }
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

/// Graphics API choices offered by the native build; unavailable platform APIs are omitted.
pub const GRAPHICS_BACKENDS: &[&str] = if cfg!(target_os = "windows") {
    &["auto", "dx12", "vulkan", "gl"]
} else if cfg!(target_os = "macos") {
    &["auto", "metal"]
} else {
    &["auto", "vulkan", "gl"]
};

/// Human-readable API names, retaining legacy manually configured choices for display.
pub fn graphics_backend_label(backend: &str) -> String {
    match backend {
        "auto" | "" => solidcraft_ui_text::tr("prefs.graphics_api_auto").to_string(),
        "dx12" => "Direct3D 12".into(),
        "vulkan" => "Vulkan".into(),
        "gl" => "OpenGL".into(),
        "metal" => "Metal".into(),
        other => other.to_string(),
    }
}

/// The window system choices (`Prefs::window_system`, #3083).
pub const WINDOW_SYSTEMS: &[&str] = &["auto", "wayland", "x11"];
/// winit's issue for dropping files onto a window on native Wayland, which the note links to.
pub const WAYLAND_DROP_ISSUE: &str = "https://github.com/rust-windowing/winit/issues/1881";

/// Linux: Graphics › Window system (#3083). It applies at the next start; the note under it says
/// which one is running now, and when native Wayland is in use or chosen, that some features are
/// limited there, linking to [`WAYLAND_DROP_ISSUE`].
#[cfg(target_os = "linux")]
fn window_system_row(app: &mut SolveApp, ui: &mut egui::Ui, label_width: f32) {
    let t = Tokens::get();
    let label = |id: &str| solidcraft_ui_text::tr(&format!("ui.window_system_{id}")).to_string();
    let labels: Vec<_> = WINDOW_SYSTEMS.iter().copied().map(label).collect();
    let labels: Vec<_> = labels.iter().map(String::as_str).collect();
    let now = app.services.window_system.map(label);
    let text = match &now {
        Some(now) => solidcraft_ui_text::format("settings.window_system_note_now", &[("now", now)]),
        None => app.tr("settings.msaa_applies_on_restart").to_string(),
    };
    let row_width = (label_width + t.space_14 + t.space_24 * 10.0).min(ui.available_width());
    ui.horizontal_wrapped(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(row_width, t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui_kit::settings_form(ui, label_width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("ui.window_system"), &text, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2((t.space_24 * 10.0).min(ui.available_width()), t.control_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let mut selected = WINDOW_SYSTEMS.iter().position(|id| *id == app.preferences.window_system).unwrap_or(0);
                            let before = selected;
                            let response = ui_kit::dropdown(ui, "window_system", &mut selected, &labels);
                            crate::scenario::publish_target("prefs:window_system", response.rect);
                            if selected != before
                                && let Some(id) = WINDOW_SYSTEMS.get(selected)
                            {
                                app.preferences.window_system = (*id).into();
                            }
                        },
                    );
                });
            });
        });
        ui_kit::settings_badge(ui, "Linux", false);
    });
    let choice = app.preferences.window_system.as_str();
    if choice == "wayland" || (choice == "auto" && app.services.window_system == Some("wayland")) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(app.tr("settings.window_system_wayland_limited")).size(t.font_small).color(t.warning));
            ui.hyperlink_to(egui::RichText::new(app.tr("settings.window_system_details")).size(t.font_small), WAYLAND_DROP_ISSUE);
        });
    }
}

/// Read startup settings without replacing unreadable data with defaults during a UI edit.
fn read_graphics_settings(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(error) => return Err(error.to_string()),
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("graphics settings must be a JSON object".into());
    }
    Ok(value)
}

/// Save a backend choice and its last working choice together before changing the UI state.
/// Consecutive edits retain the original recovery choice until a window confirms startup.
fn save_graphics_backend(app: &SolveApp, backend: &str) -> Result<(), String> {
    let path = app.services.graphics_path.as_deref().ok_or_else(|| app.tr("prefs.graphics_api_unavailable").to_string())?;
    let result = (|| {
        let path = std::path::Path::new(path);
        let mut graphics = read_graphics_settings(path)?;
        let saved = graphics.get("backend").and_then(serde_json::Value::as_str).unwrap_or("auto").to_string();
        if saved == backend {
            return Ok(());
        }
        let previous = graphics
            .get("backend_trial")
            .filter(|trial| trial.get("requested").and_then(serde_json::Value::as_str) == Some(saved.as_str()))
            .and_then(|trial| trial.get("previous").and_then(serde_json::Value::as_str))
            .unwrap_or(&saved);
        let previous = if previous.len() <= 256 && !previous.chars().any(char::is_control) { previous.to_string() } else { "auto".into() };
        graphics["backend"] = json!(backend);
        if backend == previous {
            if let Some(object) = graphics.as_object_mut() {
                object.remove("backend_trial");
            }
        } else {
            graphics["backend_trial"] = json!({"previous":previous,"requested":backend,"started":false});
        }
        solidcraft_engine::io::safe_write::write_atomic(path, graphics.to_string().as_bytes(), false).map_err(|error| error.to_string())
    })();
    result.map_err(|error: String| solidcraft_ui_text::format("prefs.graphics_api_save_failed", &[("error", &error)]))
}

/// Select the graphics API for the next launch without a terminal or environment override.
/// A failed write leaves the current preference and any pending restart unchanged.
pub fn set_graphics_backend(app: &mut SolveApp, backend: &str) -> Result<(), String> {
    if !GRAPHICS_BACKENDS.contains(&backend) {
        return Err(format!("`graphics_backend` must be one of {}", GRAPHICS_BACKENDS.join(", ")));
    }
    save_graphics_backend(app, backend)?;
    app.preferences.graphics_backend = backend.into();
    Ok(())
}

/// Check restart availability before saving a choice or applying a combined control request.
pub fn check_graphics_restart(app: &SolveApp) -> Result<(), String> {
    if app.services.graphics_backend_override {
        return Err(app.tr("prefs.graphics_api_override").to_string());
    }
    if app.services.graphics_restart.is_none() {
        return Err(app.tr("prefs.graphics_api_restart_unavailable").to_string());
    }
    Ok(())
}

/// Restart through the normal unsaved-design prompts using the host's restart command.
pub fn request_graphics_restart(app: &mut SolveApp) -> Result<(), String> {
    check_graphics_restart(app)?;
    crate::update::restart_graphics(app);
    Ok(())
}

/// Write ordinary startup settings without changing a backend choice or its recovery record.
fn save_graphics(app: &SolveApp) {
    if let Some(path) = &app.services.graphics_path {
        let path = std::path::Path::new(path);
        let result = (|| {
            let mut graphics = read_graphics_settings(path)?;
            graphics["msaa"] = json!(app.preferences.msaa);
            graphics["window_system"] = json!(app.preferences.window_system);
            graphics["adapter"] = json!(app.preferences.graphics_adapter);
            solidcraft_engine::io::safe_write::write_atomic(path, graphics.to_string().as_bytes(), false).map_err(|error| error.to_string())
        })();
        if let Err(error) = result {
            log::warn!("graphics preferences were not saved: {error}");
        }
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
                let setting = format!("{} › {}", solidcraft_ui_text::tr("ui.graphics"), solidcraft_ui_text::tr("ui.anti_aliasing"));
                let supported = supported.iter().map(|n| format!("{n}")).collect::<Vec<_>>().join("×, ") + "×";
                solidcraft_ui_text::format(
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
    if let Some(mut body) = egui::collapsing_header::CollapsingState::load(ctx, window_id().with("collapsing"))
        && !body.is_open()
    {
        body.set_open(true);
        body.store(ctx);
    }
}

/// The Preferences window's id.
pub(crate) fn window_id() -> egui::Id {
    egui::Id::unique("sc_prefs")
}

/// Its search field's id.
pub(crate) fn search_id() -> egui::Id {
    prefs_window::search_id(window_id())
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if std::mem::take(&mut app.prefs_window.panel_popup_reset)
        || (app.prefs_window.shell.section != PAGE_LAYOUT && app.prefs_window.shell.filter.trim().is_empty())
    {
        panel_layout::closed(ctx);
    }
    crate::settings_profiles::rename_popup(app, ctx);
    // File › Keyboard Shortcuts and Help › Keyboard Shortcuts open this dialog at that page.
    if app.keymap.open {
        app.keymap.open = false;
        open_at(app, PAGE_SHORTCUTS);
    }
    app.prefs_window.import_tile = None;
    if !app.prefs_window.shell.open {
        panel_layout::closed(ctx);
        // A question left up when the dialog closed is not still asked when it opens again.
        app.space_mouse.restore_pending = false;
        // Calibration opens from here and ends with it, so the cap moves the view again.
        app.space_mouse.calibrating = None;
    } else {
        if app.prefs_window.shell.snapshot().is_none() {
            app.crashes.refresh();
        }
        unfold(ctx);
        panel_layout::reserve_escape(ctx);
    }
    let before = app.preferences.clone();
    let layout_page = app.prefs_window.shell.section == PAGE_LAYOUT;
    let options = prefs_window::Options {
        id: window_id(),
        prefix: "prefs",
        labels: prefs_window::Labels {
            title: app.tr("ui.preferences"),
            icon: "settings",
            ok: "OK",
            cancel: "Cancel",
            apply: "Apply",
            search: solidcraft_ui_text::tr("prefs.search"),
            no_results: solidcraft_ui_text::tr("prefs.search_empty"),
            restore_defaults: solidcraft_ui_text::tr("prefs.restore_page_defaults"),
            restore_defaults_hint: solidcraft_ui_text::tr(if layout_page {
                "ui.panel_layout.page_defaults_hint"
            } else {
                "ui.reset_the_settings_on_this_page"
            }),
        },
        sidebar_footer: Some((Tokens::get().space_24 * 2.0, crate::settings_profiles::sidebar)),
        above_page: Some(|ui, app| panel_restore_banner(app, ui, &Tokens::get())),
    };
    prefs_window::show(ctx, app, WINDOW_PAGES, &options, |app| {
        // Former page names remain usable as scenario handles.
        for (alias, index) in PAGE_ALIASES {
            if let Some(page) = WINDOW_PAGES.get(*index)
                && let prefs_window::Page::Host(page) = page
                && let Some(rect) = crate::scenario::rect_of(page.handle)
            {
                crate::scenario::publish_target(&format!("prefs:page:{alias}"), rect);
            }
        }
        crate::space_mouse_calibration::show(app, ctx);
        if app.preferences.msaa != before.msaa
            || app.preferences.window_system != before.window_system
            || app.preferences.graphics_adapter != before.graphics_adapter
        {
            save_graphics(app);
        }
        if app.preferences.up_axis != before.up_axis {
            up_axis_changed(app);
        }
        if std::mem::take(&mut app.prefs_window.panel_restore_retry) {
            prefs_window::close(app, true);
        }
    });
}

impl prefs_window::Host for SolveApp {
    type Snapshot = Snapshot;
    fn window(&mut self) -> &mut prefs_window::State<Snapshot> {
        &mut self.prefs_window.shell
    }
    fn window_ref(&self) -> &prefs_window::State<Snapshot> {
        &self.prefs_window.shell
    }
    fn take_snapshot(&self) -> Snapshot {
        Snapshot::take(self)
    }
    fn matches(&self, snapshot: &Snapshot) -> bool {
        snapshot.matches(self) && !crate::plugins::resource_pending(self)
    }
    /// The panel layout goes back first, through the same transaction as its commands; only when
    /// that succeeds do the other settings.
    fn restore(&mut self, snapshot: &Snapshot) -> Result<(), String> {
        crate::panel_runtime::restore_preferences(self, snapshot.panel_layout.clone()).map_err(solidcraft_engine::panels::commands::error_text)?;
        if let Some(deferred) = &snapshot.panel_layout_deferred {
            self.panel_layout_preferences.clone_from(deferred);
        }
        let up = self.preferences.up_axis.clone();
        snapshot.clone().restore_other_preferences(self)?;
        if self.preferences.up_axis != up {
            up_axis_changed(self);
        }
        Ok(())
    }
    fn confirm(&mut self) -> Result<(), String> {
        crate::plugins::commit_resource_inputs(self).inspect_err(|error| self.set_status(error.clone(), true))
    }
    fn keys_captured(&self) -> bool {
        crate::keymap::capturing(self)
    }
    fn page_left(&mut self) {
        crate::keymap::stop_capture(self);
        self.keymap.read_only = false;
    }
    fn result_opened(&mut self, page: usize) {
        if page == PAGE_SHORTCUTS {
            crate::keymap::reveal(self);
        }
        if page == PAGE_PLUGINS {
            self.prefs_window.plugin_query.clear();
        }
    }
    fn closed(&mut self) {
        self.prefs_window.plugin_limit_inputs = None;
        self.prefs_window.panel_restore_details = false;
        self.prefs_window.panel_popup_reset = true;
        self.keymap.read_only = false;
        crate::keymap::stop_capture(self);
    }
}

/// The default orientation changed: the view turns to match.
fn up_axis_changed(app: &mut SolveApp) {
    use solidcraft_viewport::UpAxis;
    app.prefs_window.applied_up = Some(app.preferences.up_axis.clone());
    let up = if app.preferences.up_axis.eq_ignore_ascii_case("y") { UpAxis::Y } else { UpAxis::Z };
    if app.ui.up_axis != up || app.cam.up != up {
        app.set_up_axis(up);
    }
}

/// One page of the dialog. Inputs share the kit's aligned rows and tokenised controls.
/// A search draws every page and keeps the controls that match.
fn panel_restore_banner(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some(reason) = app.prefs_window.shell.restore_error().map(str::to_owned) else { return };
    let shown = ui_kit::Container::show_backing(
        ui,
        ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Navigation)
            .edge_frame(t.form_error_fill)
            .stroke(egui::Stroke::new(1.0, t.form_error))
            .corner_radius(t.control_radius)
            .inner_margin(t.space_8),
        |ui| {
            ui.set_width((ui.available_width() - t.space_2).max(0.0));
            ui.horizontal_wrapped(|ui| {
                let (icon, _) = ui.allocate_exact_size(egui::vec2(t.space_16, t.space_16), egui::Sense::hover());
                crate::icons::paint_ink(ui.painter(), icon, "status_warning", t.form_error);
                ui.label(
                    egui::RichText::new(app.tr("ui.panel_layout.restore_failed"))
                        .font(crate::ui_kit::type_face::semibold(t.font_label))
                        .color(t.form_error),
                );
                let retry = ui_kit::button(ui, app.tr("ui.panel_layout.try_again"), ui_kit::Button::Primary);
                crate::scenario::publish_target("prefs:panel_restore_retry", retry.rect);
                if retry.clicked() {
                    app.prefs_window.panel_restore_retry = true;
                }
                let details = ui_kit::button(ui, app.tr("ui.panel_layout.details"), ui_kit::Button::Normal);
                crate::scenario::publish_target("prefs:panel_restore_details", details.rect);
                if details.clicked() {
                    app.prefs_window.panel_restore_details = !app.prefs_window.panel_restore_details;
                }
            });
            if app.prefs_window.panel_restore_details {
                ui.label(egui::RichText::new(&reason).color(t.form_ink).size(t.font_small));
                if ui_kit::button(ui, app.tr("ui.panel_layout.copy"), ui_kit::Button::Ghost).clicked() {
                    ui.ctx().copy_text(reason.clone());
                }
            }
        },
    );
    shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Alert, true, app.tr("ui.panel_layout.restore_failed")));
    crate::scenario::publish_target("prefs:panel_restore_error", shown.response.rect);
    ui.add_space(t.space_16);
}

fn draw_page(app: &mut SolveApp, ui: &mut egui::Ui, index: usize, t: &Tokens) {
    match index {
        PAGE_SHORTCUTS => {
            let read_only = app.keymap.read_only;
            let mut show = |ui: &mut egui::Ui| {
                ui_kit::settings_heading_with_tag(
                    ui,
                    app.tr("prefs.shortcuts"),
                    app.tr("prefs.shortcuts_intro"),
                    Some(crate::settings_profiles::tag),
                );
                crate::keymap::page(app, ui);
            };
            if read_only {
                show(ui);
            } else {
                ui_kit::settings_page(ui, show);
            }
        }
        PAGE_ASSISTANT => crate::agent_chat::settings::page(app, ui),
        PAGE_PLUGINS => crate::plugins::page(app, ui),
        PAGE_GENERAL => general::show(app, ui),
        PAGE_APPEARANCE => interface::appearance(app, ui, t),
        PAGE_LAYOUT => interface::layout(app, ui),
        PAGE_TOOLS => tools::show(app, ui),
        PAGE_MOUSE3D => {
            space_mouse_section(app, ui);
        }
        PAGE_DESIGN => design::show(app, ui),
        PAGE_NAVIGATION => navigation::show(app, ui, t),
        PAGE_GRAPHICS => graphics::show(app, ui),
        PAGE_SKETCH => sketch::show(app, ui),
        _ => {}
    }
}

fn page_index(name: &str) -> Option<usize> {
    PAGES
        .iter()
        .position(|page| page.eq_ignore_ascii_case(name))
        .or_else(|| PAGE_ALIASES.iter().find_map(|(alias, page)| alias.eq_ignore_ascii_case(name).then_some(*page)))
}

/// The 3D mouse page's settings, drawn with the same settings kit as the sections above
/// it: whether it moves the view and shows in it, where the picture goes, its speed, the driver and
/// calibration, each axis, the picture with its dead zone, and what its buttons do. A section like the
/// others, with independent, searchable sections.
fn space_mouse_section(app: &mut SolveApp, ui: &mut egui::Ui) {
    mouse3d::show(app, ui);
}

/// A device's own app on this computer driving the view (the navigation bridge): on or off, its
/// port, and whether an app is connected. Part of the 3D mouse Advanced section.
fn nav_bridge(app: &mut SolveApp, ui: &mut egui::Ui) {
    let (label, tip, port) = (app.tr("ui.nav_bridge").to_owned(), app.tr("ui.nav_bridge_tip").to_owned(), app.tr("ui.nav_bridge_port").to_owned());
    let status = match (&app.space_mouse.bridge, app.preferences.nav_bridge_enabled) {
        (_, false) => None,
        (Some(name), true) => Some((format!("{} · {name}", app.tr("ui.nav_bridge_connected")), false)),
        (None, true) => Some((app.tr("ui.nav_bridge_waiting").to_owned(), true)),
    };
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        ui_kit::checkbox(ui, &label, &mut app.preferences.nav_bridge_enabled).backing_on_hover_text(&tip);
        if let Some((status, waiting)) = &status {
            ui_kit::settings_state_badge(ui, status, if *waiting { ui_kit::SettingsBadgeTone::Warning } else { ui_kit::SettingsBadgeTone::Success });
        }
    });
    ui_kit::settings_form(ui, ui_kit::ContentWidth::label(ui, &port), |ui| {
        ui_kit::row(ui, &port, |ui| {
            ui.add_enabled_ui(app.preferences.nav_bridge_enabled, |ui| {
                ui_kit::settings_control_width(ui, Tokens::get().space_24 * 3.0 + Tokens::get().space_8, |ui| {
                    ui_kit::number_field(ui, &mut app.preferences.nav_bridge_port, 1..=u16::MAX, 1.0, "");
                });
            });
        });
    });
}

/// Legacy label alignment for the 3D mouse's supplemental controls.
const FORM_LABEL_WIDTH: f32 = 150.0;

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
/// assignment, where the picture goes and whether it shows, and what the view turns around. Not
/// the calibration (it has its own Forget), the button mapping, the offered set, "never show
/// again", "Navigate with the 3D mouse" or the test model.
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
    p.space_mouse_model_pivot = d.space_mouse_model_pivot;
    p.space_mouse_sketch_orbit = d.space_mouse_sketch_orbit;
}

/// The device's picture with the coordinate circles under it, and a box with a small model that
/// moves with the 3D mouse as the view does and a Reset button: to the right of the picture where
/// there is room, else under the circles. All stay inside the section at any width.
fn picture_and_boat(
    ui: &mut egui::Ui,
    boat: &mut Option<crate::test_boat::Boat>,
    motion: Option<crate::space_mouse::DeviceMotion>,
    free_orbit: bool,
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
            crate::test_boat::show(ui, boat, motion, free_orbit, egui::vec2(width.max(0.0), BOAT_HEIGHT));
            if ui_kit::button(ui, solidcraft_ui_text::tr("ui.3d_mouse_reset_view"), ui_kit::Button::Normal).clicked() {
                crate::test_boat::reset(boat);
            }
        });
    };
    // The coordinate circles stay under the device's picture, from its left edge; the test model is
    // beside the picture where there is room, else under the circles.
    if beside {
        ui.horizontal_top(|ui| {
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
            hint_under_controls(ui, solidcraft_ui_text::tr("ui.3d_mouse_buttons_no_device"), "space_mouse_buttons");
        });
        return;
    };
    let mut jobs = crate::space_mouse::button_jobs(app, &device);
    let bit = |number: u8| number.checked_sub(1).and_then(|index| 1_u32.checked_shl(u32::from(index))).unwrap_or(0);
    let mut numbers: std::collections::BTreeSet<u8> = jobs.keys().copied().collect();
    numbers.extend((1..=32).filter(|&number| app.space_mouse.seen & bit(number) != 0));
    let mut changed = false;
    ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
        hint_under_controls(ui, solidcraft_ui_text::tr("ui.3d_mouse_buttons_hint"), "space_mouse_buttons");
        for number in numbers {
            let mut name = solidcraft_ui_text::format("ui.3d_mouse_button", &[("number", &number.to_string())]);
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
                if ui_kit::button(ui, solidcraft_ui_text::tr("ui.3d_mouse_buttons_reset"), ui_kit::Button::Normal).clicked() {
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
        None => solidcraft_ui_text::tr("ui.3d_mouse_button_nothing").to_string(),
        Some(ButtonJob::Settings) => solidcraft_ui_text::tr("ui.3d_mouse_button_settings").to_string(),
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
    let filter_id = egui::Id::unique(("sc_space_mouse_button_filter", number));
    let mut filter: String = ui.data_mut(|data| data.get_temp(filter_id).unwrap_or_default());
    let id = format!("sc_space_mouse_button_{number}");
    let chosen = ui_kit::searchable_dropdown(ui, &id, &button_job_label(app, current), &mut filter, &options, None, None);
    ui.data_mut(|data| data.insert_temp(filter_id, filter));
    let chosen = chosen?;
    jobs.into_iter().find(|job| button_job_value(job.as_ref()) == chosen)
}

/// The control channel's `ui.prefs`: `open` (a page name) opens the dialog there, `button`
/// presses OK, Cancel, Apply or Restore Defaults, `stand_up: true` answers the stand-up offer,
/// dialog or note (`false` leaves the file as it is, like "Leave it"), `theme_import` (a theme file's JSON or
/// text) adds a theme and turns it on, `theme_remove` (an id) removes an imported one and
/// `theme_export: true` returns the theme on now as `theme_file`, `theme_follow` (a path, or
/// `""` to stop) follows a theme file on the desktop (#7573); any other keys set preferences.
/// `open: "Interface/Navigation bar"` opens Interface and scrolls to its bar editor.
/// Returns them all, the open page, the offer (and whether it is the note), the theme ids and the
/// followed file (`theme_followed`, `theme_follow_error`).
pub fn control(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    // Stage every fallible preference operation before dialogs, document actions or writes.
    let mut rest = p.as_object().cloned().ok_or("preferences must be an object")?;
    let page = rest
        .remove("open")
        .map(|page| {
            let name = page.as_str().unwrap_or("General");
            page_index(name)
                .map(|i| (i, name.eq_ignore_ascii_case("Interface/Navigation bar")))
                .ok_or_else(|| format!("no page `{name}` ({})", PAGES.join(", ")))
        })
        .transpose()?;
    let button = rest.remove("button");
    if let Some(b) = button.as_ref().and_then(|v| v.as_str())
        && !["ok", "cancel", "apply", "defaults", "restart_graphics"].contains(&b)
    {
        return Err(format!("unknown button `{b}` (ok, cancel, apply, defaults, restart_graphics)"));
    }
    if button.as_ref().and_then(serde_json::Value::as_str) == Some("restart_graphics") {
        check_graphics_restart(app)?;
    }
    let stand_up = rest.remove("stand_up").and_then(|v| v.as_bool());
    let forget = match rest.remove("forget_stand_up") {
        None => false,
        Some(serde_json::Value::Bool(true)) => true,
        Some(_) => return Err("`forget_stand_up` must be true".into()),
    };
    let mut staged = app.preferences.clone();
    let mut follow = app.prefs_window.follow.clone();
    let imported = rest
        .remove("theme_import")
        .map(|value| {
            let theme = match value {
                serde_json::Value::String(text) => crate::theme_files::parse(&text)?,
                other => crate::theme_files::from_value(&other)?,
            };
            let id = crate::theme_files::add(&mut staged.custom_themes, theme)?;
            staged.theme = id.clone();
            Ok::<_, String>(id)
        })
        .transpose()?;
    if let Some(id) = rest.remove("theme_remove") {
        let id = id.as_str().unwrap_or_default();
        let i = staged.custom_themes.iter().position(|t| t.id == id).ok_or_else(|| format!("no imported theme `{id}`"))?;
        let gone = staged.custom_themes.remove(i);
        staged.theme_options.remove(id);
        if staged.theme == id {
            staged.theme = if gone.base.is_dark() { "dark" } else { "light" }.into();
        }
    }
    match rest.remove("theme_follow") {
        Some(value) => {
            if !FOLLOW_AVAILABLE {
                return Err(FOLLOW_UNAVAILABLE.into());
            }
            let path = value.as_str().ok_or("`theme_follow` must be a theme file's path, or an empty string to stop following")?;
            staged.theme_follow = crate::theme_files::follow_path(path)?;
            follow.launch = None;
            let _ = follow.poll(&staged.theme_follow, f64::NEG_INFINITY, true);
        }
        None if rest.contains_key("theme") => {
            staged.theme_follow.clear();
            follow.launch = None;
            let _ = follow.poll("", f64::NEG_INFINITY, true);
        }
        None => {}
    }
    let export = rest.remove("theme_export").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut new = prepare_preferences(app, &staged, followed_theme(&staged, &follow), &serde_json::Value::Object(rest.clone()))?;
    let stopped_following = !app.prefs_window.follow.path(&app.preferences.theme_follow).is_empty() && follow.path(&new.theme_follow).is_empty();
    if stand_up == Some(true) && app.stand_up.is_none() {
        return Err("no file is waiting to be stood up".into());
    }
    if let Some((i, navbar)) = page {
        open_at(app, i);
        app.prefs_window.navbar_scroll = navbar;
        prefs_window::hold(app);
    }
    match stand_up {
        Some(true) => crate::stand_up::stand_up(app)?,
        Some(false) => crate::stand_up::leave(app),
        None => {}
    }
    // Answering the offer records history after preferences were staged. Keep that answer
    // when applying the staged settings; forgetting is an explicit action that takes precedence.
    if stand_up.is_some() {
        new.stand_up_answers = app.preferences.stand_up_answers.clone();
    }
    if forget {
        new.stand_up_answers.clear();
    }
    if stopped_following {
        // Restore the selected theme's dark/light mode even when its saved id did not change.
        stop_following(app);
    }
    app.prefs_window.follow = follow;
    let mut out = apply_preferences(app, new, &serde_json::Value::Object(rest))?;
    match button.as_ref().and_then(|b| b.as_str()) {
        Some("ok") => prefs_window::press(app, WINDOW_PAGES, prefs_window::Button::Ok)?,
        Some("cancel") => prefs_window::press(app, WINDOW_PAGES, prefs_window::Button::Cancel)?,
        Some("apply") => prefs_window::press(app, WINDOW_PAGES, prefs_window::Button::Apply)?,
        Some("defaults") => prefs_window::press(app, WINDOW_PAGES, prefs_window::Button::Defaults)?,
        Some("restart_graphics") => request_graphics_restart(app)?,
        Some(b) => return Err(format!("unknown button `{b}` (ok, cancel, apply, defaults, restart_graphics)")),
        None => {}
    }
    if button.is_some() {
        // Cancel and Restore Defaults can change the angle too.
        follow_view_fov(app);
        out = serde_json::to_value(&app.preferences).map_err(|e| e.to_string())?;
    }
    if let Some(o) = out.as_object_mut() {
        if let serde_json::Value::Object(options) = serde_json::to_value(theme_options_reply(app)).map_err(|e| e.to_string())? {
            o.extend(options);
        }
        o.insert("dialog".into(), json!(app.prefs_window.shell.open.then(|| PAGES.get(app.prefs_window.shell.section).copied().unwrap_or_default())));
        o.insert("apply_enabled".into(), json!(prefs_window::apply_pending(app)));
        o.insert("stand_up_offer".into(), json!(app.stand_up));
        o.insert("stand_up_note".into(), json!(app.stand_up.is_some() && app.stand_up_note));
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

/// Read stored theme identity/base tables with the existing serde contract, then normalize only
/// option tables using the file-import parser. A stale declaration must not lose the base palette.
fn theme_from_preferences(value: &serde_json::Value, strict: bool) -> Result<crate::theme_files::ThemeFile, String> {
    let mut base = value.clone();
    if let Some(object) = base.as_object_mut() {
        object.remove("options");
        object.remove("overlays");
        object.remove("tabs");
    }
    let mut theme: crate::theme_files::ThemeFile = serde_json::from_value(base).map_err(|e| e.to_string())?;
    if let Err(error) = theme.set_tabs(value) {
        if strict {
            return Err(error);
        }
        log::warn!("preferences: theme `{}` has invalid tab surfaces ({error}); keeping its base palette", theme.id);
    }
    if let Err(error) = theme.set_option_tables(value) {
        if strict {
            return Err(error);
        }
        log::warn!("preferences: theme `{}` has invalid option tables ({error}); keeping its base palette", theme.id);
    }
    Ok(theme)
}

fn themes_that_read<'de, D: serde::Deserializer<'de>>(saved: D) -> Result<Vec<crate::theme_files::ThemeFile>, D::Error> {
    Vec::<serde_json::Value>::deserialize(saved)?.iter().map(|value| theme_from_preferences(value, false).map_err(serde::de::Error::custom)).collect()
}

/// Shared action for the Preferences button and `ui.prefs`.
pub(super) fn forget_stand_up(app: &mut SolveApp) {
    app.preferences.stand_up_answers.clear();
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
    let modes =
        THEMES.iter().filter(|(id, _)| *id != "system").map(|(id, label)| ((*id).to_string(), solidcraft_ui_text::tr_label(label).to_string()));
    let files = crate::theme_files::builtin().iter().chain(&app.preferences.custom_themes).map(|t| (t.id.clone(), t.name.clone()));
    let mut choices: Vec<_> = modes.chain(files).collect();
    choices.sort_by_key(|(id, _)| match id.as_str() {
        "light" => 0,
        "dark" => 1,
        _ => 2,
    });
    choices
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
/// Import a theme file's text, from a file, a paste or a drop, and say what happened in the status bar.
fn import_theme_text(app: &mut SolveApp, text: Result<String, String>) {
    match text.and_then(|text| import_theme(app, &serde_json::Value::String(text))) {
        Ok(id) => {
            let name = crate::theme_files::find(&id, &app.preferences.custom_themes).map(|t| t.name.clone()).unwrap_or(id);
            app.set_status(format!("Theme imported: {name}"), false);
        }
        Err(e) => app.set_status(format!("Theme import: {e}"), true),
    }
}

/// Text that is meant as a theme file: a JSON object naming the theme format. Anything else
/// pasted while Preferences is open keeps its usual meaning.
fn looks_like_theme(text: &str) -> bool {
    text.trim_start().starts_with('{') && text.contains(crate::theme_files::FORMAT)
}

/// Ctrl/Cmd+V on the Appearance page with a theme file's text on the clipboard imports it, like
/// Import… with a file (#8226). Called before the global Copy and Paste shortcuts, which would
/// otherwise paste copied components; the paste is taken out of the input once it is a theme's.
pub fn take_theme_paste(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.prefs_window.shell.open || app.prefs_window.shell.section != PAGE_APPEARANCE {
        return;
    }
    let Some(text) = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None })) else {
        return;
    };
    if !looks_like_theme(&text) {
        return;
    }
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            !matches!(e, egui::Event::Paste(_) | egui::Event::Key { key: egui::Key::V, .. } | egui::Event::Key { key: egui::Key::Insert, .. })
        })
    });
    import_theme_text(app, Ok(text));
}

/// Files dropped on the theme Import tile are themes to import, not designs to open (#8226).
pub fn take_theme_drops(app: &mut SolveApp, ctx: &egui::Context) {
    if !theme_drop_target(app, ctx) || !ctx.input(|i| !i.raw.dropped_files.is_empty()) {
        return;
    }
    let files = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
    for f in files.iter().take(8) {
        // The browser reads dropped files only asynchronously: the web shell takes them first and
        // hands a theme's bytes to `import_theme_bytes` (#8226), so none reach this on wasm.
        #[cfg(not(target_arch = "wasm32"))]
        match f.bytes() {
            Ok(bytes) => import_theme_bytes(app, bytes),
            Err(e) => import_theme_text(app, Err(e.to_string())),
        }
        #[cfg(target_arch = "wasm32")]
        let _ = f;
    }
}

/// Whether files dropped now land on the Import tile: the Preferences window is open and the
/// pointer was last seen over the tile.
pub fn theme_drop_target(app: &SolveApp, ctx: &egui::Context) -> bool {
    app.prefs_window
        .import_tile
        .filter(|_| app.prefs_window.shell.open)
        .is_some_and(|rect| ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p)))
}

/// A file dropped on the Import tile, as bytes: imported when it is a theme, refused with the reason
/// otherwise.
pub fn import_theme_bytes(app: &mut SolveApp, bytes: Vec<u8>) {
    let text = if bytes.len() > crate::theme_files::MAX_BYTES {
        Err("the file is too large for a theme".to_string())
    } else {
        String::from_utf8(bytes).map_err(|_| "not a theme file".to_string())
    };
    import_theme_text(app, text);
}

pub fn import_theme(app: &mut SolveApp, v: &serde_json::Value) -> Result<String, String> {
    let theme = match v {
        serde_json::Value::String(text) => crate::theme_files::parse(text)?,
        other => crate::theme_files::from_value(other)?,
    };
    let id = crate::theme_files::add(&mut app.preferences.custom_themes, theme)?;
    if app.prefs_window.shell.open && app.prefs_window.shell.section == PAGE_APPEARANCE {
        theme_picker::choose(app, app.prefs_window.theme_picker_night, &id);
    } else {
        select_theme(app, &id);
    }
    Ok(id)
}

/// Remove an imported theme; when it is on, its base (dark or light) takes over.
fn remove_theme(app: &mut SolveApp, id: &str) -> Result<(), String> {
    let Some(i) = app.preferences.custom_themes.iter().position(|t| t.id == id) else {
        return Err(format!("no imported theme `{id}`"));
    };
    let gone = app.preferences.custom_themes.remove(i);
    app.preferences.theme_options.remove(id);
    if app.preferences.theme == id {
        select_theme(app, if gone.base.is_dark() { "dark" } else { "light" });
    }
    Ok(())
}

/// The theme on now as a complete theme file, a starting point for a new one.
fn current_theme_file(app: &SolveApp) -> serde_json::Value {
    match active_theme_file(app) {
        Some(f) => export_theme_file(f),
        None => theme_file(app, &app.preferences.theme, app.prefs_window.system_theme_dark.unwrap_or(app.ui.dark)),
    }
}

fn active_theme_file(app: &SolveApp) -> Option<&crate::theme_files::ThemeFile> {
    followed_theme(&app.preferences, &app.prefs_window.follow).or_else(|| {
        crate::theme_files::find(
            theme_picker::active_id(&app.preferences, app.prefs_window.system_theme_dark.unwrap_or(app.ui.dark)),
            &app.preferences.custom_themes,
        )
    })
}

/// The single runtime decision used by controls, ui.prefs and renderer activation. It never
/// changes saved intent or caches accessibility/hardware state in palette tokens.
pub(crate) fn blur_availability(app: &SolveApp) -> crate::theme_options::Availability {
    blur_availability_for(app, active_theme_file(app).is_some_and(crate::theme_options::declares_blur))
}

fn blur_availability_for(app: &SolveApp, offered: bool) -> crate::theme_options::Availability {
    use crate::theme_options::Availability::{Available, Unavailable};
    let reduced = app.services.reduce_transparency.as_ref().and_then(|read| read(offered || app.universal_panel_host_enabled()));
    if !offered && app.blur_session.harness_radius.is_none() {
        return Available;
    }
    if let Some(reason) = app.blur_session.harness_unavailable {
        return Unavailable(reason);
    }
    // State owns CPU policy, including the explicit cfg(test) rendering fixture. The
    // adapter identity stays real; production CPU capability can never be Available.
    let capability = app.blur.capability();
    if app.blur.software_adapter() && capability != crate::blur::Capability::Available {
        return Unavailable("prefs.blur_software");
    }
    if cfg!(target_arch = "wasm32") {
        return Unavailable("prefs.blur_web");
    }
    if reduced == Some(true) {
        return Unavailable("prefs.blur_reduce_transparency");
    }
    if app.home.open || crate::render_workspace::result_active(app) {
        return Unavailable("prefs.blur_source");
    }
    match capability {
        crate::blur::Capability::Available => Available,
        crate::blur::Capability::Unavailable(_) => Unavailable("prefs.blur_device"),
    }
}

pub(crate) fn prepare_blur(app: &mut SolveApp, ctx: &egui::Context) {
    if ctx.viewport_id() != egui::ViewportId::ROOT {
        return;
    }
    let intent = crate::theme::blur_intent(ctx);
    let radius = app.blur_session.harness_radius.unwrap_or(intent.radius);
    // Disabled frames need only cached intent and atomic OS observation. Explicit UI/API
    // queries still report capabilities at radius zero; painting need not probe the GPU.
    let availability = if radius > 0.0 {
        blur_availability_for(app, intent.offered)
    } else {
        if let Some(read) = &app.services.reduce_transparency {
            read(intent.offered || app.universal_panel_host_enabled());
        }
        crate::theme_options::Availability::Available
    };
    // Resolve raw/default intent separately so temporary unavailability never drives the
    // renderer through radius=0 and clears a latched device/allocation failure.
    let available = availability == crate::theme_options::Availability::Available;
    app.blur_session.sample(ctx, intent.revision, radius, available, app.blur.painted_frame());
    app.blur.set_available(available);
    app.blur.set_tier_cap(app.blur_session.guard.cap);
    if let Err(error) = app.blur.set_radius(radius) {
        log::warn!("theme blur: {error}");
    }
}

fn followed_theme<'a>(prefs: &Prefs, follow: &'a crate::theme_files::Follow) -> Option<&'a crate::theme_files::ThemeFile> {
    (!follow.path(&prefs.theme_follow).is_empty()).then(|| follow.theme()).flatten()
}

/// Preserve authored definitions; themes predating options retain their complete snapshot export.
fn export_theme_file(file: &crate::theme_files::ThemeFile) -> serde_json::Value {
    if !file.options.is_empty() || !file.overlays.is_empty() || !file.tabs.is_empty() || file.needs_newer() {
        crate::theme_files::export_file(file)
    } else {
        crate::theme_files::export(
            &file.name,
            file.base,
            &crate::theme_files::resolve(file),
            crate::theme_files::resolve_icon_palette(file, &Default::default()),
        )
    }
}

fn effective_theme_options(
    prefs: &Prefs,
    file: &crate::theme_files::ThemeFile,
) -> (BTreeMap<String, crate::theme_files::OptionValue>, Vec<solidcraft_engine::schema::ui_prefs::ThemeOptionNotice>) {
    use crate::theme_files::{OptionKind, OptionNoticeKind, OptionValue};
    use solidcraft_engine::schema::ui_prefs::{ThemeOptionNotice, ThemeOptionNoticeKind as Kind};
    let mut saved = BTreeMap::new();
    let mut notices = Vec::new();
    let definitions = crate::theme_files::ordered_options(file);
    for (id, value) in prefs.theme_options.get(&file.id).into_iter().flatten() {
        let definition = definitions.iter().find(|o| &o.id == id);
        if let Some(definition) = definition
            && let OptionKind::Range { min, max, .. } = &definition.kind
            && value.as_f64().is_some_and(|n| n < f64::from(*min) || n > f64::from(*max))
        {
            notices.push(ThemeOptionNotice { id: id.clone(), kind: Kind::OutOfRange });
            continue;
        }
        match serde_json::from_value::<OptionValue>(value.clone()) {
            Ok(value) => {
                saved.insert(id.clone(), value);
            }
            Err(_) => {
                notices.push(ThemeOptionNotice { id: id.clone(), kind: if definition.is_some() { Kind::WrongType } else { Kind::UnknownOption } })
            }
        }
    }
    let (values, problems) = crate::theme_files::effective_values(file, &saved);
    notices.extend(problems.into_iter().map(|notice| ThemeOptionNotice {
        id: notice.id,
        kind: match notice.kind {
            OptionNoticeKind::UnknownOption => Kind::UnknownOption,
            OptionNoticeKind::WrongType => Kind::WrongType,
            OptionNoticeKind::UnknownChoice => Kind::UnknownChoice,
            OptionNoticeKind::NonFinite => Kind::NonFinite,
            OptionNoticeKind::Clamped => Kind::OutOfRange,
        },
    }));
    notices.sort_by(|a, b| a.id.cmp(&b.id));
    (values, notices)
}

fn resolved_theme(prefs: &Prefs, file: &crate::theme_files::ThemeFile) -> Tokens {
    crate::theme_files::resolve_with(file, &effective_theme_options(prefs, file).0)
}

fn theme_option_text(text: &crate::theme_files::OptionText, language: &str, fallback: &str) -> String {
    text.get(language).or_else(|| text.get("en")).map(String::as_str).unwrap_or(fallback).to_owned()
}

/// SolidCraft's name and help (message ids) for the theme system's standard option `id`.
fn standard_option_text(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "blur" => Some(("prefs.theme_option_blur", "prefs.theme_option_blur_help")),
        "opacity" => Some(("prefs.theme_option_opacity", "prefs.theme_option_opacity_help")),
        "tab_colours" => Some(("prefs.theme_option_tab_colours", "prefs.theme_option_tab_colours_help")),
        "tab_fade_ms" => Some(("prefs.theme_option_tab_fade_ms", "prefs.theme_option_tab_fade_ms_help")),
        _ => None,
    }
}

/// Why a theme option is off (a message id), or `None` when it is available.
pub(crate) fn theme_option_reason(availability: crate::theme_options::Availability) -> Option<&'static str> {
    match availability {
        crate::theme_options::Availability::Available => None,
        crate::theme_options::Availability::Unavailable(reason) => Some(reason),
        crate::theme_options::Availability::Unsupported => Some("prefs.theme_options_unavailable"),
    }
}

fn theme_option_rows<'a>(
    file: &'a crate::theme_files::ThemeFile,
    language: &str,
    providers: &[crate::theme_options::Provider],
    names: fn(&str) -> Option<(&'static str, &'static str)>,
    blur: crate::theme_options::Availability,
) -> Vec<ui_kit::ThemeOptionRow<'a>> {
    use crate::theme_options::{offered, provider};
    let locale = solidcraft_ui_text::Locale::for_language(language);
    offered(file, providers)
        .into_iter()
        .map(|definition| {
            let text = provider(&definition.id, providers).and_then(|p| names(p.id));
            ui_kit::ThemeOptionRow {
                definition,
                blur: definition.id == "blur",
                label: text
                    .map(|(label, _)| locale.text(label).to_owned())
                    .unwrap_or_else(|| theme_option_text(&definition.label, language, &definition.id)),
                help: text.map(|(_, help)| locale.text(help).to_owned()).unwrap_or_else(|| theme_option_text(&definition.help, language, "")),
                choices: match &definition.kind {
                    crate::theme_files::OptionKind::Choice { values, .. } => {
                        values.iter().map(|v| theme_option_text(&v.label, language, &v.id)).collect()
                    }
                    _ => Vec::new(),
                },
                reason: theme_option_reason(crate::theme_options::availability(file, &definition.id, providers, blur))
                    .map(|reason| locale.text(reason).to_owned()),
            }
        })
        .collect()
}

/// What the theme options list says, from SolidCraft's catalogs.
fn theme_options_text() -> ui_kit::ThemeOptionsText<'static> {
    ui_kit::ThemeOptionsText {
        count: |count| {
            let id = if count == 1 { "prefs.appearance.option_count_one" } else { "prefs.appearance.option_count" };
            solidcraft_ui_text::format(id, &[("count", &count.to_string())])
        },
        more: |count| solidcraft_ui_text::format("prefs.theme_options_more", &[("count", &count.to_string())]),
        reset: solidcraft_ui_text::tr("prefs.theme_options_reset"),
        blur_off: solidcraft_ui_text::tr("prefs.blur_off"),
        blur_cost: solidcraft_ui_text::tr("prefs.blur_cost"),
    }
}

fn theme_options_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let availability = blur_availability(app);
    let Some(file) = active_theme_file(app).cloned() else { return };
    let (mut values, notices) = effective_theme_options(&app.preferences, &file);
    // Notices precede the empty-control return: newer requirements intentionally have no rows.
    let mut problems: Vec<_> = notices
        .iter()
        .map(|notice| {
            let raw = app.preferences.theme_options.get(&file.id).and_then(|v| v.get(&notice.id));
            (
                notice.id.clone(),
                format!("{:?}:{raw:?}", notice.kind),
                solidcraft_ui_text::format("prefs.theme_option_invalid", &[("option", &notice.id)]),
            )
        })
        .collect();
    if file.needs_newer() {
        problems.push(("newer".into(), file.requires.to_string(), app.tr("prefs.theme_options_newer").to_owned()));
    }
    for (id, identity, text) in problems {
        let key = theme_notice_key(&file.id, &id, &identity);
        if !app.blur_session.dismissed(key)
            && ui_kit::theme_option_notice(ui, &id, &text, solidcraft_ui_text::tr("prefs.theme_notice_dismiss"), false).1
        {
            app.blur_session.dismiss(key);
        }
    }
    if app.blur_session.guard.notice {
        let (undo, dismiss) =
            ui_kit::theme_option_notice(ui, "guard", app.tr("prefs.blur_guard"), solidcraft_ui_text::tr("prefs.theme_notice_dismiss"), true);
        if undo {
            app.blur_session.guard.undo();
            ui.ctx().request_repaint();
        }
        if dismiss {
            app.blur_session.guard.notice = false;
        }
    }
    let rows = theme_option_rows(
        &file,
        solidcraft_ui_text::effective_language(&app.preferences.language),
        crate::theme_options::PROVIDERS,
        standard_option_text,
        availability,
    );
    if rows.is_empty() {
        return;
    }
    let before = values.clone();
    let reset = ui_kit::settings_card(ui, |ui| ui_kit::theme_options_list(ui, &file.id, &file.name, &rows, &mut values, &theme_options_text())).inner;
    let changes: BTreeMap<_, _> = values.into_iter().filter(|(id, value)| before.get(id) != Some(value)).collect();
    if reset || !changes.is_empty() {
        let update = if reset { json!({}) } else { json!(changes) };
        let params = json!({"theme_options": {(file.id.clone()): update}});
        // Same atomic validation/merge and followed-file semantics as the control channel.
        // No button, page selection or dialog is added by this live preference update.
        if let Err(error) = control(app, &params) {
            app.set_status(error, true);
        }
    }
}

fn theme_notice_key(theme: &str, problem: &str, raw: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (theme, problem, raw).hash(&mut hash);
    hash.finish()
}

/// Only explicitly supplied entries are checked. Stale saved entries survive unrelated changes.
fn merge_theme_options(prefs: &mut Prefs, followed: Option<&crate::theme_files::ThemeFile>, update: &serde_json::Value) -> Result<(), String> {
    use crate::theme_files::{OptionKind, OptionValue};
    let themes = update.as_object().ok_or("`theme_options` must be a map of theme ids to option maps")?;
    // Apply legacy keys first so a canonical entry in the same request always wins.
    let aliases = themes.iter().filter(|(id, _)| crate::theme_files::legacy_gruvbox(id).is_some());
    let canonical = themes.iter().filter(|(id, _)| crate::theme_files::legacy_gruvbox(id).is_none());
    for (requested_id, update) in aliases.chain(canonical) {
        let theme_id = crate::theme_files::legacy_gruvbox(requested_id).map_or(requested_id.as_str(), |(family, _)| family);
        let key = format!("theme_options.{theme_id}");
        let file = followed.filter(|f| f.id == theme_id).or_else(|| crate::theme_files::find(theme_id, &prefs.custom_themes));
        let mode = THEMES.iter().any(|(id, _)| *id == theme_id);
        if file.is_none() && !mode {
            return Err(format!("`{key}`: unknown theme"));
        }
        let options = update.as_object().ok_or_else(|| format!("`{key}` must be an option map"))?;
        if options.is_empty() {
            prefs.theme_options.remove(theme_id);
            continue;
        }
        for (id, value) in options {
            let key = format!("{key}.{id}");
            let definition = file
                .and_then(|file| crate::theme_files::ordered_options(file).into_iter().find(|o| &o.id == id))
                .ok_or_else(|| format!("`{key}`: unknown option"))?;
            if value.is_null() {
                if let Some(saved) = prefs.theme_options.get_mut(theme_id) {
                    saved.remove(id);
                }
                continue;
            }
            let parsed: OptionValue = serde_json::from_value(value.clone()).map_err(|_| format!("`{key}`: wrong type"))?;
            let valid = match (&definition.kind, &parsed) {
                (OptionKind::Bool { .. }, OptionValue::Bool(_)) => true,
                (OptionKind::Choice { values, .. }, OptionValue::Choice(id)) => values.iter().any(|v| &v.id == id),
                (OptionKind::Range { min, max, .. }, OptionValue::Num(n)) => {
                    // Check the original JSON number too, before f32 conversion rounds at a bound.
                    n.is_finite() && value.as_f64().is_some_and(|n| n.is_finite() && n >= f64::from(*min) && n <= f64::from(*max))
                }
                _ => false,
            };
            if !valid {
                return Err(format!("`{key}`: wrong type, unknown choice or out of range"));
            }
            prefs.theme_options.entry(theme_id.to_owned()).or_default().insert(id.clone(), value.clone());
        }
        if prefs.theme_options.get(theme_id).is_some_and(BTreeMap::is_empty) {
            prefs.theme_options.remove(theme_id);
        }
    }
    Ok(())
}

fn theme_options_reply(app: &SolveApp) -> solidcraft_engine::schema::ui_prefs::ThemeOptionsReply {
    use crate::theme_files::{OptionKind, OptionValue};
    use solidcraft_engine::schema::ui_prefs::{ThemeChoice, ThemeOptionDef, ThemeOptionKind, ThemeOptionValue, ThemeOptionsReply};
    let Some(file) = active_theme_file(app) else { return ThemeOptionsReply::default() };
    let text = |texts: &crate::theme_files::OptionText, fallback: &str| {
        texts
            .get(solidcraft_ui_text::effective_language(&app.preferences.language))
            .or_else(|| texts.get("en"))
            .map(String::as_str)
            .unwrap_or(fallback)
            .to_string()
    };
    let (values, notices) = effective_theme_options(&app.preferences, file);
    let blur = blur_availability(app);
    ThemeOptionsReply {
        theme_option_defs: crate::theme_files::ordered_options(file)
            .into_iter()
            .map(|o| {
                let standard = crate::theme_options::standard(&o.id);
                let named = crate::theme_options::provider(&o.id, crate::theme_options::PROVIDERS).and_then(|p| standard_option_text(p.id));
                let availability = crate::theme_options::availability(file, &o.id, crate::theme_options::PROVIDERS, blur);
                ThemeOptionDef {
                    id: o.id.clone(),
                    kind: match &o.kind {
                        OptionKind::Bool { default } => ThemeOptionKind::Bool { default: *default },
                        OptionKind::Choice { default, values } => ThemeOptionKind::Choice {
                            default: default.clone(),
                            values: values.iter().map(|v| ThemeChoice { id: v.id.clone(), label: text(&v.label, &v.id) }).collect(),
                        },
                        OptionKind::Range { default, min, max, step, unit } => {
                            ThemeOptionKind::Range { default: *default, min: *min, max: *max, step: *step, unit: unit.clone() }
                        }
                    },
                    label: named.map(|(label, _)| app.tr(label).to_owned()).unwrap_or_else(|| text(&o.label, &o.id)),
                    help: named.map(|(_, help)| app.tr(help).to_owned()).unwrap_or_else(|| text(&o.help, "")),
                    parent: o.parent.clone(),
                    advanced: o.advanced,
                    section: o.section.clone(),
                    standard,
                    available: availability == crate::theme_options::Availability::Available,
                    reason: theme_option_reason(availability).map(|reason| app.tr(reason).to_owned()),
                }
            })
            .collect(),
        theme_option_values: values
            .into_iter()
            .map(|(id, value)| {
                (
                    id,
                    match value {
                        OptionValue::Bool(v) => ThemeOptionValue::Bool(v),
                        OptionValue::Choice(v) => ThemeOptionValue::Choice(v),
                        OptionValue::Num(v) => ThemeOptionValue::Num(v),
                    },
                )
            })
            .collect(),
        theme_option_notices: notices,
    }
}

/// Export the tile the user chose, without applying it or changing the dialog snapshot.
fn theme_file(app: &SolveApp, id: &str, system_dark: bool) -> serde_json::Value {
    use crate::theme_files::{Base, export, find};
    let id = if id == "system" { theme_picker::slot_id(&app.preferences, system_dark) } else { id };
    match find(id, &app.preferences.custom_themes) {
        Some(f) => export_theme_file(f),
        None if id == "dark" => export("Dark", Base::Dark, &solidcraft_theme::Tokens::dark(), None),
        None => export("Light", Base::Light, &solidcraft_theme::Tokens::light(), None),
    }
}

/// Fold a setting an older release kept in its own field into the one this version reads.
/// `orbit_cursor` (`true` = the point under the cursor) became `orbit_around`. A 3D mouse
/// calibration that can't be used as it is (a hand edit) is dropped, so the device is read as it
/// reports rather than spinning the view.
pub fn migrate(mut p: Prefs) -> Prefs {
    migrate_gruvbox(&mut p);
    // Every saved file carries the old default limit; it reads as no limit.
    if p.agent_chat_turn_limit == crate::agent_chat::gate::OLD_DEFAULT_TURN_LIMIT {
        p.agent_chat_turn_limit = 0;
    }
    let valid_resource = |(kind, id): &(String, String)| !kind.is_empty() && kind.len() <= 64 && !id.is_empty() && id.len() <= 4096;
    p.render_library_favorites = p.render_library_favorites.into_iter().filter(valid_resource).take(256).collect();
    p.render_library_recent = p.render_library_recent.into_iter().filter(valid_resource).take(32).collect();
    if !p.joint_rotation_coarse.is_finite() || !(0.001..=180.0).contains(&p.joint_rotation_coarse) {
        p.joint_rotation_coarse = 10.0;
    }
    if !p.joint_rotation_fine.is_finite() || !(0.001..=180.0).contains(&p.joint_rotation_fine) {
        p.joint_rotation_fine = 0.1;
    }
    if !accepted::wheel_zoom_speed(&p) {
        p.wheel_zoom_speed = Prefs::default().wheel_zoom_speed;
    }
    if !accepted::home_fit_size(&p) {
        p.home_fit_size = Prefs::default().home_fit_size;
    }
    if !accepted::constraint_glyph_scale(&p) {
        p.constraint_glyph_scale = Prefs::default().constraint_glyph_scale;
    }
    if !accepted::dimension_text_scale(&p) {
        p.dimension_text_scale = Prefs::default().dimension_text_scale;
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

/// Fold old contrast cards into their family without replacing unrelated saved options.
/// An active old card still names the selected appearance; explicit control updates run after
/// this migration, so their contrast choice (including a reset) takes precedence.
pub(crate) fn migrate_gruvbox(p: &mut Prefs) {
    let selected = crate::theme_files::legacy_gruvbox(&p.theme);
    for (old, family, _) in crate::theme_files::GRUVBOX_ALIASES {
        if let Some(options) = p.theme_options.remove(old) {
            let canonical = p.theme_options.entry(family.into()).or_default();
            for (id, value) in options {
                canonical.entry(id).or_insert(value);
            }
        }
    }
    if let Some((family, contrast)) = selected {
        p.theme = family.into();
        p.theme_options.entry(family.into()).or_default().insert("contrast".into(), json!(contrast));
    }
}

/// Drop plugin ids this page would not show, and keep at most [`crate::plugins::MAX_DISABLED`],
/// so a hand-edited preferences file cannot carry a huge or nonsense list. `set` still refuses
/// those instead of dropping them.
fn limit_disabled_plugins(mut p: Prefs) -> Prefs {
    let defaults = solidcraft_engine::plugin::HostLimits::default();
    if p.plugin_max_fuel == 0 {
        p.plugin_max_fuel = defaults.max_fuel;
    }
    if !(1..=solidcraft_engine::plugin::MAX_MEMORY_MIB).contains(&p.plugin_max_memory_mib) {
        p.plugin_max_memory_mib = defaults.max_memory_mib;
    }
    // The file size limits, likewise, are brought into range (#4086).
    let limits = file_limits(&p);
    let mb = solidcraft_engine::io::limits::MB;
    (p.max_step_file_mb, p.max_design_file_mb) =
        if solidcraft_engine::io::limits::ADJUSTABLE { (limits.step / mb, limits.design / mb) } else { (DEFAULT_STEP_MB, DEFAULT_DESIGN_MB) };
    // A hand-edited largest sketch out of range reads as the default (#10874).
    if !accepted::max_sketch_points(&p) {
        p.max_sketch_points = Prefs::default().max_sketch_points;
    }
    p.disabled_plugins.retain(|id| crate::plugins::valid_id(id));
    if p.disabled_plugins.len() > crate::plugins::MAX_DISABLED {
        p.disabled_plugins = p.disabled_plugins.into_iter().take(crate::plugins::MAX_DISABLED).collect();
    }
    p
}

/// Keep only known hidden tabs, each once, so a hand-edited preferences
/// file cannot carry a huge or nonsense list. `set` still refuses those instead of dropping them.
fn limit_toolbar_tabs(mut p: Prefs) -> Prefs {
    fn known_once(list: Vec<String>, allowed: &[&str]) -> Vec<String> {
        let mut kept: Vec<String> = Vec::new();
        for tab in list {
            if allowed.contains(&tab.as_str()) && !kept.contains(&tab) {
                kept.push(tab);
            }
        }
        kept
    }
    let allowed: Vec<&str> = crate::workspace::choosable_tabs().collect();
    p.hidden_toolbar_tabs = known_once(std::mem::take(&mut p.hidden_toolbar_tabs), &allowed);
    p
}

/// Saved preferences, keeping every one that still reads. A value this version doesn't
/// understand (a setting a newer or older release saved differently, a hand edit) falls back to
/// its default on its own instead of discarding all of them.
pub fn lenient(saved: &serde_json::Value) -> Prefs {
    if let Ok(p) = serde_json::from_value::<Prefs>(saved.clone()) {
        return limit_toolbar_tabs(limit_disabled_plugins(migrate(p)));
    }
    let Ok(mut cur) = serde_json::to_value(Prefs::default()) else { return Prefs::default() };
    if let (Some(o), Some(src)) = (cur.as_object_mut(), saved.as_object()) {
        for (k, v) in src.iter().take(1000) {
            // Keys this version doesn't have are ignored anyway.
            if !o.contains_key(k) && !LEGACY_PANEL_FIELDS.contains(&k.as_str()) {
                continue;
            }
            let old = o.insert(k.clone(), v.clone());
            if serde_json::from_value::<Prefs>(serde_json::Value::Object(o.clone())).is_err() {
                log::warn!("preferences: `{k}` is not understood by this version; using its default");
                if let Some(old) = old {
                    o.insert(k.clone(), old);
                } else {
                    o.remove(k);
                }
            }
        }
    }
    serde_json::from_value(cur).map(migrate).map(limit_disabled_plugins).map(limit_toolbar_tabs).unwrap_or_default()
}

/// Validate a proposed preference state without applying it or performing writes.
fn prepare_preferences(
    app: &SolveApp,
    base: &Prefs,
    followed: Option<&crate::theme_files::ThemeFile>,
    p: &serde_json::Value,
) -> Result<Prefs, String> {
    for key in ["joint_rotation_coarse", "joint_rotation_fine"] {
        if let Some(value) = p.get(key)
            && value.as_f64().is_none_or(|v| !v.is_finite() || !(0.001..=180.0).contains(&v))
        {
            return Err(format!("`{key}` must be 0.001…180 degrees"));
        }
    }
    let mut cur = serde_json::to_value(base).map_err(|e| e.to_string())?;
    if let (Some(o), Some(src)) = (cur.as_object_mut(), p.as_object()) {
        for (k, v) in src {
            if !o.contains_key(k) {
                return Err(format!("unknown preference `{k}`"));
            }
            if k == "custom_themes" {
                let values = v
                    .as_array()
                    .filter(|values| values.len() <= crate::theme_files::MAX_CUSTOM)
                    .ok_or_else(|| format!("`custom_themes` must be an array of at most {} themes", crate::theme_files::MAX_CUSTOM))?;
                let themes: Vec<_> = values
                    .iter()
                    .enumerate()
                    .map(|(i, value)| theme_from_preferences(value, true).map_err(|error| format!("`custom_themes[{i}]`: {error}")))
                    .collect::<Result<_, _>>()?;
                o.insert(k.clone(), serde_json::to_value(themes).map_err(|e| e.to_string())?);
            } else if k != "theme_options" {
                o.insert(k.clone(), v.clone());
            }
        }
    }
    // Saved preferences read an unknown mode as the default; asked for here, it is an error.
    if let Some(v) = p.get("color_vision")
        && v.as_str().and_then(solidcraft_theme::ColorVision::from_id).is_none()
    {
        return Err("`color_vision` must be off, protan, deutan or tritan".into());
    }
    if let Some(v) = p.get("viewport_theme")
        && v.as_str().and_then(crate::theme::ViewportTheme::from_id).is_none()
    {
        return Err("`viewport_theme` must be theme, light or dark".into());
    }
    if let Some(v) = p.get("validation")
        && v.as_str().and_then(solidcraft_engine::kernel::Validation::parse).is_none()
    {
        return Err("`validation` must be each_step, each_feature or off".into());
    }
    if let Some(value) = p.get("navigation_bar") {
        crate::navigation_bar::validate(value)?;
    }
    if let Some(v) = p.get("stand_up_unknown")
        && !StandUpUnknown::ALL.iter().any(|c| v.as_str() == Some(c.id()))
    {
        return Err("`stand_up_unknown` must be ask, note or off".into());
    }
    if let Some(v) = p.get("dimension_input")
        && !matches!(v.as_str(), Some("view" | "window"))
    {
        return Err("`dimension_input` must be view or window".into());
    }
    if let Some(value) = p.get("graphics_backend")
        && !value.as_str().is_some_and(|id| GRAPHICS_BACKENDS.contains(&id) || id == app.preferences.graphics_backend)
    {
        return Err(format!("`graphics_backend` must be one of {}", GRAPHICS_BACKENDS.join(", ")));
    }
    if let Some(v) = p.get("dimension_colour")
        && !v.is_null()
        && serde_json::from_value::<[u8; 3]>(v.clone()).is_err()
    {
        return Err("`dimension_colour` must be null or three numbers 0…255".into());
    }
    let mut new: Prefs = serde_json::from_value(cur).map_err(|e| e.to_string())?;
    new.retain_legacy_panel_fields(base);
    migrate_gruvbox(&mut new);
    if let Some(update) = p.get("theme_options") {
        merge_theme_options(&mut new, followed, update)?;
    }
    if !accepted::wheel_zoom_speed(&new) {
        return Err("`wheel_zoom_speed` must be 0.25…4".into());
    }
    if !accepted::home_fit_size(&new) {
        return Err("`home_fit_size` must be 0.1…1".into());
    }
    if !accepted::constraint_glyph_scale(&new) {
        return Err("`constraint_glyph_scale` must be 1…3".into());
    }
    if !accepted::dimension_text_scale(&new) {
        return Err("`dimension_text_scale` must be 0.5…2.5".into());
    }
    let new = migrate(new);
    crate::plugins::host_limits(&new).validate().map_err(|e| format!("plugin_max_fuel / plugin_max_memory_mib: {e}"))?;
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
    if new.language != solidcraft_ui_text::SYSTEM && !solidcraft_ui_text::is_supported(&new.language) {
        return Err(format!(
            "`language` must be `{}` or one of: {}",
            solidcraft_ui_text::SYSTEM,
            crate::i18n::LOCALES.iter().map(|locale| locale.id).collect::<Vec<_>>().join(", ")
        ));
    }
    if new.ui_font != app.preferences.ui_font && !new.ui_font.trim().is_empty() && crate::fonts::ui_font(&new.ui_font).is_none() {
        return Err(format!("`ui_font`: `{}` is not an installed font family or a readable font file", new.ui_font.trim()));
    }
    if new.disabled_plugins.len() > crate::plugins::MAX_DISABLED || new.disabled_plugins.iter().any(|id| !crate::plugins::valid_id(id)) {
        return Err(format!("`disabled_plugins` must be at most {} ids of lowercase letters, digits and hyphens", crate::plugins::MAX_DISABLED));
    }
    if new.max_step_file_mb != app.preferences.max_step_file_mb || new.max_design_file_mb != app.preferences.max_design_file_mb {
        let mb = solidcraft_engine::io::limits::MB;
        let wanted = solidcraft_engine::io::limits::FileLimits {
            step: new.max_step_file_mb.saturating_mul(mb),
            design: new.max_design_file_mb.saturating_mul(mb),
        };
        if !solidcraft_engine::io::limits::ADJUSTABLE && wanted != solidcraft_engine::io::limits::FileLimits::DEFAULT {
            return Err("`max_step_file_mb` and `max_design_file_mb` can't change in the web app: a browser tab can't hold larger files".into());
        }
        wanted.validate().map_err(|e| {
            format!(
                "`max_step_file_mb` must be {DEFAULT_STEP_MB}…{max} and `max_design_file_mb` {DEFAULT_DESIGN_MB}…{max}: {e}",
                max = solidcraft_engine::io::limits::ceiling() / mb
            )
        })?;
    }
    let once = |list: &[String], allowed: &[&str]| {
        list.len() <= allowed.len() && list.iter().all(|tab| allowed.contains(&tab.as_str()) && list.iter().filter(|t| *t == tab).count() == 1)
    };
    let allowed: Vec<&str> = crate::workspace::choosable_tabs().collect();
    if !once(&new.hidden_toolbar_tabs, &allowed) {
        return Err(format!("`hidden_toolbar_tabs` lists toolbar tabs, each once: {}", allowed.join(", ")));
    }
    if !accepted::ui_scale(&new) || !accepted::look_at_speed(&new) {
        return Err("`ui_scale` must be 0.5…2.5 and `look_at_speed` 0.25…4".into());
    }
    if !accepted::print_face_highlight(&new) {
        return Err("`print_face_highlight` must be 0…1".into());
    }
    if !accepted::joint_symbols(&new) {
        return Err("`joint_symbols` must be 0 (ring and axis), 1 (white on dark), 2 (solid accent), 3 (solid amber), 4 (outline), 5 (colour by type) or 6 (dots)".into());
    }
    if !accepted::home_fit_size(&new) {
        return Err("`home_fit_size` must be 0.1…1".into());
    }
    if !accepted::perspective_fov(&new) {
        return Err("`perspective_fov` must be 15°…90° in radians (0.2618…1.5708)".into());
    }
    if !accepted::dimension_step_mm(&new) || !accepted::dimension_step_deg(&new) {
        return Err("`dimension_step_mm` must be 0.001…1000 and `dimension_step_deg` 0.001…180".into());
    }
    if !accepted::max_sketch_points(&new) {
        use solidcraft_engine::sketch::limits::{CEILING_POINTS, DEFAULT_POINTS};
        return Err(format!("`max_sketch_points` must be {DEFAULT_POINTS}…{CEILING_POINTS}"));
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
    Ok(new)
}

/// Set preferences from JSON (control channel); returns them all.
pub fn set(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    let new = prepare_preferences(app, &app.preferences, followed_theme(&app.preferences, &app.prefs_window.follow), p)?;
    apply_preferences(app, new, p)
}

/// All fallible preference checks finish before any application or file write.
fn apply_preferences(app: &mut SolveApp, new: Prefs, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    if new.graphics_backend != app.preferences.graphics_backend {
        save_graphics_backend(app, &new.graphics_backend)?;
    }
    let file = crate::theme_files::find(&new.theme, &new.custom_themes);
    if (new.theme != app.preferences.theme || new.custom_themes != app.preferences.custom_themes) && new.theme != "system" {
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
    if new.select_other_hidden != app.preferences.select_other_hidden {
        crate::select_other::close(app);
    }
    app.preferences = new;
    crate::plugins::sync_policy(app);
    sync_surface_analysis(app);
    crate::render_workspace::apply_memory_policy(app);
    apply_file_limits(&app.preferences);
    apply_sketch_limit(&app.preferences);
    if units_changed {
        default_units_changed(app);
    }
    if graphics_changed {
        save_graphics(app);
    }
    if up_changed {
        up_axis_changed(app);
    }
    follow_view_fov(app);
    serde_json::to_value(&app.preferences).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) fn keep_search_reveal_visible(app: &mut SolveApp) -> bool {
    app.prefs_window.shell.keep_reveal_visible()
}

#[cfg(test)]
pub(crate) fn reveal_preference(h: &mut crate::scenario::Harness, handle: &str) {
    // Keep small scroll steps so a target cannot be skipped on a long Preferences page.
    for _ in 0..32 {
        let page = crate::scenario::rect_of("dialog:page_body").expect("preference body");
        // Scroll offsets are pixel-rounded: the final item can extend a fraction
        // of a point beyond the viewport even at the maximum scroll offset.
        // Match the tolerance used by the colour-vision sample geometry checks.
        let visible = crate::scenario::rect_of(handle)
            .map(|r| page.expand(1.0).contains_rect(r))
            .unwrap_or_else(|| crate::scenario::handle_at(handle).is_some_and(|p| page.shrink(solidcraft_theme::Tokens::get().space_16).contains(p)));
        if visible {
            return;
        }
        h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-100}));
        h.frames(20);
    }
    panic!(
        "preference {handle} was not revealed: theme={} language={} section={} filter={:?} screen={:?} window={:?} body={:?} target_rect={:?} target_point={:?}",
        h.app.preferences.theme,
        h.app.preferences.language,
        h.app.prefs_window.shell.section,
        h.app.prefs_window.shell.filter,
        h.ctx.content_rect(),
        crate::scenario::rect_of("prefs:window"),
        crate::scenario::rect_of("dialog:page_body"),
        crate::scenario::rect_of(handle),
        crate::scenario::handle_at(handle)
    );
}

#[cfg(test)]
mod tests {
    fn host_page(page: &prefs_window::Page<SolveApp>) -> &prefs_window::HostPage<SolveApp> {
        match page {
            prefs_window::Page::Host(page) => page,
        }
    }

    #[test]
    fn invalid_legacy_panel_preferences_recover_before_shared_host_setup() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.load_prefs(
            &json!({"preferences":{"shared_panel_layout":false,"language":42,"browser_width":0.0,"timeline_left_width":-1.0,
                "agent_chat_width":1e9,"render_browser_width":0.0,"browser_share":2.0}})
            .to_string(),
        );
        let defaults = Prefs::default();
        assert_eq!(app.preferences.browser_width, 0.0);
        crate::panel_runtime::tick_app(&mut app, &egui::Context::default());
        assert!(app.panel_runtime().is_some(), "malformed legacy sizes must not leave setup retrying each frame");
        let imported = crate::panel_runtime::preferences_value(&app).unwrap();
        for (name, expected) in [
            ("history_width", defaults.timeline_left_width),
            ("assistant_width", defaults.agent_chat_width),
            ("render_width", defaults.render_browser_width),
        ] {
            assert_eq!(imported["legacy"][name], json!(expected));
        }
        let saved = serde_json::to_value(&app.preferences).unwrap();
        for key in ["shared_panel_layout", "browser_width", "timeline_left_width", "agent_chat_width", "render_browser_width", "browser_share"] {
            assert!(saved.get(key).is_none(), "{key} is no longer written");
            assert!(set(&mut app, &json!({(key): false})).is_err(), "{key} cannot be set after cutover");
        }
    }

    #[test]
    fn theme_traits_use_resolved_background_and_maximal_contrast() {
        crate::test_support::host_data();
        use theme_picker::{Classification, theme_traits};
        let light = Tokens::light();
        let dark = Tokens::dark();
        assert_eq!(theme_traits(&light), Classification { dark: false, high: false });
        assert_eq!(theme_traits(&dark), Classification { dark: true, high: false });
        for (ground, ink, night) in [(egui::Color32::WHITE, egui::Color32::BLACK, false), (egui::Color32::BLACK, egui::Color32::WHITE, true)] {
            let mut palette = light;
            palette.dialog_footer = ground;
            palette.form_ink = ink;
            assert_eq!(theme_traits(&palette), Classification { dark: night, high: true });
        }
        let mut palette = light;
        for (gray, night) in [(187, true), (188, false)] {
            palette.dialog_footer = egui::Color32::from_gray(gray);
            assert_eq!(theme_traits(&palette).dark, night, "linear luminance straddles 0.5");
        }
        let maximal: Vec<_> = crate::theme_files::builtin()
            .iter()
            .filter(|f| theme_traits(&resolved_theme(&Prefs::default(), f)).high)
            .map(|f| f.id.as_str())
            .collect();
        assert_eq!(maximal.len(), 4, "ordinary palettes must stay Standard: {maximal:?}");
        assert!(maximal.contains(&"1-bit") && maximal.contains(&"1-bit-dark"));
    }

    #[test]
    fn theme_slots_default_persist_allow_command_access_and_restore_with_appearance() {
        let mut h = crate::scenario::Harness::new();
        assert_eq!((&h.app.preferences.theme_day[..], &h.app.preferences.theme_night[..]), ("light", "dark"));
        let out = super::set(&mut h.app, &json!({"theme_day":"akari", "theme_night":"onyx"})).unwrap();
        assert_eq!((out["theme_day"].as_str(), out["theme_night"].as_str()), (Some("akari"), Some("onyx")));
        let out = h.call("ui.prefs", json!({}));
        assert_eq!(out["result"]["theme_day"], "akari");
        assert_eq!(out["result"]["theme_night"], "onyx");
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!((restored.preferences.theme_day.as_str(), restored.preferences.theme_night.as_str()), ("akari", "onyx"));
        h.call("ui.prefs", json!({"theme_day":"clay", "theme_night":"clay-dark", "open":"Appearance"}));
        assert_eq!(h.app.preferences.theme_day, "clay");
        defaults(&mut h.app, PAGE_GENERAL);
        assert_eq!(h.app.preferences.theme_day, "clay", "another page's reset does not clear slots");
        defaults(&mut h.app, PAGE_APPEARANCE);
        assert_eq!((h.app.preferences.theme_day.as_str(), h.app.preferences.theme_night.as_str()), ("light", "dark"));
    }

    #[test]
    fn theme_slots_resolve_unknown_and_recursive_ids_without_changing_saved_intent() {
        let mut prefs = Prefs { theme: "system".into(), theme_day: "missing".into(), theme_night: "system".into(), ..Default::default() };
        assert_eq!(theme_picker::active_id(&prefs, false), "light");
        assert_eq!(theme_picker::active_id(&prefs, true), "dark");
        assert_eq!((&prefs.theme_day[..], &prefs.theme_night[..]), ("missing", "system"));
        prefs.theme = "onyx".into();
        assert_eq!(theme_picker::active_id(&prefs, false), "onyx");
        assert_eq!(theme_picker::active_id(&prefs, true), "onyx");
        let mut h = crate::scenario::Harness::new();
        h.app.services.desktop_dark = Some(Box::new(|| Some(false)));
        let out = h.call("ui.prefs", json!({"theme":"system", "theme_day":"missing", "theme_night":"system"}));
        assert!(out["error"].is_null(), "{out}");
        h.frames(2);
        assert!(!h.app.ui.dark);
        assert_eq!(h.app.preferences.theme_day, "missing");
        h.app.services.desktop_dark = Some(Box::new(|| Some(true)));
        h.frames(2);
        assert!(h.app.ui.dark);
        assert_eq!(h.app.preferences.theme_night, "system");
    }

    #[test]
    fn theme_onyx_is_reachable_selectable_and_shows_its_options_in_both_views() {
        for grid in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1467,"height":1146}));
            h.call("ui.prefs", json!({"language":"en","theme":"light","open":"Appearance"}));
            h.frames(5);
            if grid {
                h.step(&json!({"click":{"handle":"prefs:theme_view:1"}})).unwrap();
                h.frames(3);
                super::reveal_preference(&mut h, "prefs:theme_tile:onyx");
            } else {
                for _ in 0..theme_choices(&h.app).len() {
                    if crate::scenario::rect_of("prefs:theme_tile:onyx").is_some() {
                        break;
                    }
                    h.step(&json!({"click":{"handle":"prefs:theme_next"}})).unwrap();
                    h.frames(3);
                }
            }
            h.step(&json!({"click":{"handle":"prefs:theme_tile:onyx"}})).unwrap();
            h.frames(4);
            assert_eq!(h.app.preferences.theme, "onyx");
            assert!(crate::scenario::rect_of("prefs:theme_options_badge:onyx").is_some());
            assert!(crate::scenario::rect_of("prefs:theme_options_header").is_some());
            let out = h.call("ui.prefs", json!({}));
            assert_eq!(out["result"]["theme_option_values"]["opacity"], 80.0);
        }
    }

    #[test]
    fn theme_system_resolves_custom_day_and_night_by_os_including_cross_tone_slots() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let desktop = Arc::new(AtomicBool::new(false));
        let reading = desktop.clone();
        let mut h = crate::scenario::Harness::new();
        h.app.services.desktop_dark = Some(Box::new(move || Some(reading.load(Ordering::Relaxed))));
        h.call("ui.prefs", json!({"theme":"system", "theme_day":"onyx", "theme_night":"akari"}));
        h.frames(3);
        assert!(h.app.ui.dark, "the OS is light, but the Day slot may hold a dark theme");
        assert_eq!(h.app.prefs_window.window_theme, Some(egui::SystemTheme::SystemDefault));
        assert_eq!(active_theme_file(&h.app).unwrap().id, "onyx");
        assert_eq!(current_theme_file(&h.app)["name"], "Onyx");
        desktop.store(true, Ordering::Relaxed);
        h.frames(3);
        assert!(!h.app.ui.dark);
        assert_eq!(active_theme_file(&h.app).unwrap().id, "akari");
        h.call("ui.prefs", json!({"theme_day":"dark", "theme_night":"light"}));
        h.frames(2);
        assert_eq!(current_theme_file(&h.app)["base"], "light");
        desktop.store(false, Ordering::Relaxed);
        h.frames(2);
        assert_eq!(current_theme_file(&h.app)["base"], "dark");
        // If OS observation temporarily disappears, the mode button must use the same
        // remembered OS tone as rendering, rather than the cross-tone slot's palette.
        h.app.services.desktop_dark = Some(Box::new(|| None));
        h.app.home.open = false;
        h.call("ui.prefs", json!({"open":"Appearance"}));
        h.frames(3);
        h.step(&json!({"click":{"handle":"prefs:theme_mode:0"}})).unwrap();
        assert_eq!(h.app.preferences.theme, "dark", "leaving System preserves the theme actually in effect");
    }

    #[test]
    fn theme_mode_switches_preserve_the_effective_theme_and_fill_only_the_aimed_slot() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        select_theme(&mut app, "onyx");
        theme_picker::switch_mode(&mut app, true, true);
        assert_eq!((&app.preferences.theme[..], &app.preferences.theme_day[..], &app.preferences.theme_night[..]), ("system", "light", "onyx"));
        theme_picker::choose(&mut app, false, "akari");
        assert_eq!((&app.preferences.theme[..], &app.preferences.theme_day[..], &app.preferences.theme_night[..]), ("system", "akari", "onyx"));
        theme_picker::switch_mode(&mut app, false, false);
        assert_eq!(app.preferences.theme, "akari", "leaving system picks the effective Day theme, not its tone's default");
        theme_picker::switch_mode(&mut app, true, false);
        theme_picker::choose(&mut app, true, "clay-dark");
        theme_picker::switch_mode(&mut app, false, true);
        assert_eq!(app.preferences.theme, "clay-dark");
        let mut state = theme_picker::State::default();
        assert!(!state.night && !state.grid && state.tone.is_none());
        state.aim(true);
        assert!(state.night && state.tone == Some(true));
        state.aim(false);
        assert!(!state.night && state.tone == Some(false));
    }

    #[test]
    fn theme_filter_counts_ignore_only_their_own_group() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let file = crate::theme_files::from_value(&json!({"format":"solidcraft-theme/1", "name":"Imported maximal", "base":"dark", "colors":{"dialog_footer":"#000000", "form_ink":"#ffffff"}})).unwrap();
        crate::theme_files::add(&mut app.preferences.custom_themes, file).unwrap();
        let entries = theme_picker::entries(&app);
        let state = theme_picker::State { tone: Some(true), high: Some(false), imported: false, ..Default::default() };
        assert_eq!((state.count(&entries, "tone", false), state.count(&entries, "tone", true)), (8, 15));
        assert_eq!((state.count(&entries, "contrast", false), state.count(&entries, "contrast", true)), (15, 4));
        assert_eq!(state.count(&entries, "imported", true), 0);
        let state = theme_picker::State { high: Some(true), imported: true, ..state };
        assert_eq!(state.count(&entries, "tone", true), 1);
        assert_eq!(state.count(&entries, "contrast", true), 1);
        assert_eq!(state.count(&entries, "contrast", false), 0);
        assert_eq!(state.count(&entries, "imported", true), 1);
    }

    #[test]
    fn theme_carousel_wheel_passes_vertical_to_page_and_handles_horizontal_and_shift() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1467,"height":1146}));
        h.call("ui.prefs", json!({"language":"en","theme":"light","open":"Appearance"}));
        h.frames(6);
        for (delta, modifiers, horizontal) in [
            (egui::vec2(1.0, -96.0), egui::Modifiers::NONE, false),
            (egui::vec2(-180.0, 0.0), egui::Modifiers::NONE, true),
            (egui::vec2(0.0, -180.0), egui::Modifiers::SHIFT, true),
        ] {
            let before = crate::scenario::rect_of("prefs:card:Light").unwrap();
            let at = crate::scenario::rect_of("prefs:theme_viewport").unwrap().center();
            h.frame_with(vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    source: egui::MouseWheelSource::Unknown,
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    modifiers,
                    phase: egui::TouchPhase::Move,
                },
            ]);
            h.frames(60);
            let after = crate::scenario::rect_of("prefs:card:Light").unwrap();
            if horizontal {
                assert!(after.left() < before.left() - 100.0, "horizontal wheel must move a card: {before:?} -> {after:?}");
                assert!((after.top() - before.top()).abs() < 1.0, "horizontal wheel must leave the page in place");
            } else {
                assert!((after.left() - before.left()).abs() < 1.0, "vertical wheel must not move carousel");
                assert!(after.top() < before.top() - 50.0, "vertical wheel must scroll the page");
            }
        }
    }

    /// Profile operations switches the profiler when it changes, and only then: an agent's
    /// diagnostics.profile in between is not undone next frame.
    #[test]
    fn the_profile_preference_switches_the_profiler_when_it_changes() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.frames(1);
        h.call("ui.prefs", json!({"profile_operations": true}));
        h.frames(2);
        assert!(solidcraft_engine::profile::enabled());
        h.app.session.execute("diagnostics.profile", &json!({"enabled": false})).unwrap();
        h.frames(2);
        assert!(!solidcraft_engine::profile::enabled(), "the command's choice stands");
        h.call("ui.prefs", json!({"profile_operations": false}));
        h.frames(2);
        assert!(!solidcraft_engine::profile::enabled());
    }

    /// Preferences saved with a setting this version no longer has still load, every other
    /// setting kept: the removed key is ignored like any unknown one.
    #[test]
    fn saved_preferences_with_a_removed_setting_still_load() {
        use serde_json::json;
        for (removed, value) in [
            ("geometry_kernel", json!("smlib")),
            ("geometry_kernel", json!("solidcore")),
            ("metric_tap_drill_major_less_pitch", json!(true)),
            ("surface_extrude_symmetric_per_side", json!(true)),
        ] {
            let saved = json!({"length_decimals": 4, removed: value});
            let p = serde_json::from_value::<super::Prefs>(saved.clone()).unwrap();
            assert_eq!(p.length_decimals, 4, "{removed}");
            assert_eq!(super::lenient(&saved).length_decimals, 4, "{removed}");
            assert!(serde_json::to_value(&p).unwrap().get(removed).is_none(), "{removed} is not written back");
        }
    }

    /// #10254: the invert toggle is off by default, persists, and the Navigation page's
    /// Defaults resets it.
    #[test]
    fn invert_navigator_arrows_defaults_off_persists_and_resets() {
        assert!(!Prefs::default().invert_navigator_arrows);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(!app.preferences.invert_navigator_arrows);
        app.preferences.invert_navigator_arrows = true;
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["invert_navigator_arrows"], true);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert!(restored.preferences.invert_navigator_arrows);
        super::reset_preferences(&mut app.preferences, super::PAGE_NAVIGATION);
        assert!(!app.preferences.invert_navigator_arrows);
    }

    /// #10483: the curved-roll invert toggle is off by default, persists, and the Navigation
    /// page's Defaults resets it.
    #[test]
    fn invert_curved_roll_arrows_defaults_off_persists_and_resets() {
        assert!(!Prefs::default().invert_curved_roll_arrows);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(!app.preferences.invert_curved_roll_arrows);
        app.preferences.invert_curved_roll_arrows = true;
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["invert_curved_roll_arrows"], true);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert!(restored.preferences.invert_curved_roll_arrows);
        super::reset_preferences(&mut app.preferences, super::PAGE_NAVIGATION);
        assert!(!app.preferences.invert_curved_roll_arrows);
    }

    /// #10220: off by default, offered on Preferences › General only where designs are kept in
    /// a browser (the web app), set through `ui.prefs` and saved with the settings.
    #[test]
    fn keep_unsaved_in_browser_is_off_and_only_the_web_app_offers_it() {
        crate::i18n::install();
        use serde_json::json;
        assert!(!crate::prefs::Prefs::default().keep_unsaved_in_browser);
        let label = solidcraft_ui_text::tr("prefs.keep_unsaved_in_browser").to_string();
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"General"}));
        h.frames(5);
        assert!(h.widget(&label).is_none(), "the desktop app keeps no designs in a browser");
        h.call("ui.prefs", json!({"button":"cancel"}));
        h.app.services.browser_files = Some(Box::new(|| {}));
        h.call("ui.prefs", json!({"open":"General"}));
        h.frames(5);
        assert!(h.widget(&label).is_some(), "the web app offers it beside the other saving options");
        let ask = solidcraft_ui_text::tr("prefs.keep_unsaved_also_asks").to_string();
        assert!(h.widget(&ask).is_none(), "still asking only means something once work is kept");
        let reply = h.call("ui.prefs", json!({"keep_unsaved_in_browser": true}));
        assert_eq!(reply["ok"], true, "{reply}");
        h.frames(5);
        assert!(h.widget(&ask).is_some(), "keeping work offers whether to still ask");
        h.call("ui.prefs", json!({"button":"ok"}));
        assert!(h.app.preferences.keep_unsaved_in_browser);
        assert!(h.app.preferences.keep_unsaved_also_asks, "keeping work still asks unless told not to");
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.keep_unsaved_in_browser, "it is saved with the settings");
    }

    #[test]
    fn trackpad_zoom_modifier_roundtrips_and_validates_control_values() {
        assert_eq!(Prefs::default().trackpad_zoom_modifier, TrackpadZoomModifier::Off);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        for (id, value) in [
            ("cmd", TrackpadZoomModifier::Cmd),
            ("alt", TrackpadZoomModifier::Alt),
            ("ctrl", TrackpadZoomModifier::Ctrl),
            ("off", TrackpadZoomModifier::Off),
        ] {
            set(&mut app, &json!({"trackpad_zoom_modifier": id})).unwrap();
            assert_eq!(app.preferences.trackpad_zoom_modifier, value);
            let saved = serde_json::to_value(&app.preferences).unwrap();
            assert_eq!(saved["trackpad_zoom_modifier"], id);
            assert_eq!(lenient(&saved).trackpad_zoom_modifier, value);
            let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            restored.load_prefs(&app.prefs());
            assert_eq!(restored.preferences.trackpad_zoom_modifier, value);
        }

        for invalid in [json!("super"), json!(""), json!(42), json!(null)] {
            assert!(set(&mut app, &json!({"trackpad_zoom_modifier": invalid})).is_err());
            assert_eq!(app.preferences.trackpad_zoom_modifier, TrackpadZoomModifier::Off);
            assert_eq!(lenient(&json!({"trackpad_zoom_modifier": invalid})).trackpad_zoom_modifier, TrackpadZoomModifier::Off);
        }
        app.load_prefs(&json!({"preferences": {"trackpad_zoom_modifier": "future"}}).to_string());
        assert_eq!(app.preferences.trackpad_zoom_modifier, TrackpadZoomModifier::Off);
        app.preferences.trackpad_zoom_modifier = TrackpadZoomModifier::Cmd;
        reset_preferences(&mut app.preferences, PAGE_NAVIGATION);
        assert_eq!(app.preferences.trackpad_zoom_modifier, TrackpadZoomModifier::Off);
    }

    #[test]
    fn command_dialog_value_memory_defaults_off_persists_and_resets() {
        assert!(!Prefs::default().remember_dialog_values);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(!app.preferences.remember_dialog_values);
        app.preferences.remember_dialog_values = true;
        assert_eq!(serde_json::to_value(&app.preferences).unwrap()["remember_dialog_values"], true);

        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert!(restored.preferences.remember_dialog_values);
        reset_preferences(&mut app.preferences, PAGE_TOOLS);
        assert!(!app.preferences.remember_dialog_values);
    }

    #[test]
    fn navigation_bar_preferences_round_trip_and_recover_hostile_fields() {
        use crate::navigation_bar::{Labels, NavigationBar, Position};
        use crate::ui_kit::{NavigationEdge, NavigationIdle, NavigationSize};
        use serde_json::json;
        assert_eq!(super::lenient(&json!({})).navigation_bar, NavigationBar::default());
        let p = super::lenient(&json!({"navigation_bar": {
            "slots":["pan", "future_slot", "pan", "separator", "separator", "separator", "separator", "separator", 7, "fit"],
            "position":"bottom_left", "edge":"top", "size":"regular", "labels":"labels", "idle":"hide"
        }}));
        assert_eq!(p.navigation_bar.slots, ["pan", "future_slot", "separator", "separator", "separator", "separator", "fit"]);
        assert_eq!(
            (p.navigation_bar.position, p.navigation_bar.edge, p.navigation_bar.size, p.navigation_bar.labels, p.navigation_bar.idle),
            (Position::Left, NavigationEdge::Top, NavigationSize::Regular, Labels::Labels, NavigationIdle::Hide)
        );
        let saved = serde_json::to_value(&p).unwrap();
        assert_eq!(serde_json::from_value::<super::Prefs>(saved).unwrap(), p);
        for field in ["position", "edge", "size", "labels", "idle"] {
            let mut value = serde_json::to_value(&p).unwrap();
            value["navigation_bar"][field] = json!({"wrong":"type"});
            let mut expected = p.clone();
            match field {
                "position" => expected.navigation_bar.position = Position::default(),
                "edge" => expected.navigation_bar.edge = NavigationEdge::default(),
                "size" => expected.navigation_bar.size = NavigationSize::default(),
                "labels" => expected.navigation_bar.labels = Labels::default(),
                _ => expected.navigation_bar.idle = NavigationIdle::default(),
            }
            assert_eq!(super::lenient(&value), expected, "{field}");
            value["navigation_bar"][field] = json!("unknown");
            assert_eq!(super::lenient(&value), expected, "{field}");
        }
        for value in [json!(null), json!(7), json!([]), json!({"slots":false})] {
            assert_eq!(super::lenient(&json!({"navigation_bar":value})).navigation_bar, NavigationBar::default());
        }
        let ids: Vec<_> = (0..100).map(|i| format!("future_{i}")).collect();
        let limited = super::lenient(&json!({"navigation_bar":{"slots":ids}}));
        assert_eq!(limited.navigation_bar.slots.len(), crate::navigation_bar::MAX_SLOTS);
        assert_eq!(limited.navigation_bar.slots.last().map(String::as_str), Some("future_15"));
        let ids: Vec<_> = ["pan".to_owned(), "pan".to_owned()].into_iter().chain((0..20).map(|i| format!("future_{i}"))).collect();
        let limited = super::lenient(&json!({"navigation_bar":{"slots":ids}}));
        assert_eq!(limited.navigation_bar.slots.len(), crate::navigation_bar::MAX_SLOTS);
        assert_eq!(limited.navigation_bar.slots.last().map(String::as_str), Some("future_14"));
    }

    #[test]
    fn navigation_bar_control_reads_writes_validates_and_restores() {
        use crate::navigation_bar::NavigationBar;
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let initial = h.call("ui.prefs", json!({}));
        assert_eq!(initial["result"]["navigation_bar"], serde_json::to_value(NavigationBar::default()).unwrap());
        let edit = json!({"navigation_bar":{"slots":["zoom", "future", "zoom", "display"], "position":"bottom_right", "size":"regular", "labels":"labels", "idle":"always"}});
        h.call("ui.prefs", json!({"open":"Layout"}));
        let reply = h.call("ui.prefs", edit);
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["navigation_bar"]["slots"], json!(["zoom", "future", "display"]));
        let before = h.app.preferences.clone();
        for bad in [
            json!(null),
            json!({"slots":"fit"}),
            json!({"slots":[7]}),
            json!({"position":"top"}),
            json!({"size":true}),
            json!({"labels":"text"}),
            json!({"idle":10}),
            json!({"unknown":true}),
        ] {
            assert!(super::control(&mut h.app, &json!({"navigation_bar":bad,"zoom_reverse":true})).is_err());
            assert_eq!(h.app.preferences, before, "failed updates are atomic");
        }
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.navigation_bar, NavigationBar::default(), "a change that was not applied is not saved (#9730)");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.navigation_bar, NavigationBar::default());
        h.call("ui.prefs", json!({"open":"Layout","navigation_bar":{"slots":[]}}));
        assert!(h.app.preferences.navigation_bar.slots.is_empty());
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert_eq!(h.app.preferences.navigation_bar, NavigationBar::default());
    }

    fn navbar_click(h: &mut crate::scenario::Harness, handle: &str) {
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar"}));
        h.frames(20);
        scroll_to_and_click(h, handle);
    }

    #[test]
    fn navigation_bar_editor_buttons_and_appearance_update_preferences() {
        use crate::navigation_bar::{Labels, MenuDisplay, Position};
        use crate::ui_kit::{NavigationIdle, NavigationSize};
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar", "navigation_bar":{"slots":["pan","fit","visual_style","display"]}}));
        navbar_click(&mut h, "prefs:navbar:add:spin");
        assert_eq!(h.app.preferences.navigation_bar.slots, ["pan", "fit", "visual_style", "display", "spin"]);
        navbar_click(&mut h, "prefs:navbar:slot:pan");
        navbar_click(&mut h, "prefs:navbar:right");
        assert_eq!(h.app.preferences.navigation_bar.slots, ["fit", "pan", "visual_style", "display", "spin"]);
        navbar_click(&mut h, "prefs:navbar:remove");
        assert_eq!(h.app.preferences.navigation_bar.slots, ["fit", "visual_style", "display", "spin"]);
        navbar_click(&mut h, "prefs:navbar:slot:visual_style");
        navbar_click(&mut h, "prefs:navbar:shows:0");
        assert_eq!(h.app.preferences.navigation_bar.menu_display("visual_style"), MenuDisplay::IconAndValue);
        navbar_click(&mut h, "prefs:navbar:anchor:0");
        assert_eq!(h.app.preferences.navigation_bar.position, Position::Left);
        assert_eq!(h.app.preferences.navigation_bar.edge, crate::ui_kit::NavigationEdge::Top);
        navbar_click(&mut h, "prefs:navbar:anchor:5");
        assert_eq!(h.app.preferences.navigation_bar.position, Position::Right);
        assert_eq!(h.app.preferences.navigation_bar.edge, crate::ui_kit::NavigationEdge::Bottom);
        navbar_click(&mut h, "prefs:navbar:size:0");
        assert_eq!(h.app.preferences.navigation_bar.size, NavigationSize::Compact);
        navbar_click(&mut h, "prefs:navbar:size:1");
        assert_eq!(h.app.preferences.navigation_bar.size, NavigationSize::Regular);
        navbar_click(&mut h, "prefs:navbar:buttons:1");
        assert_eq!(h.app.preferences.navigation_bar.labels, Labels::Labels);
        navbar_click(&mut h, "prefs:navbar:idle:0");
        assert_eq!(h.app.preferences.navigation_bar.idle, NavigationIdle::Always);
        navbar_click(&mut h, "prefs:navbar:idle:2");
        assert_eq!(h.app.preferences.navigation_bar.idle, NavigationIdle::Hide);
        let before = h.app.preferences.navigation_bar.clone();
        navbar_click(&mut h, "prefs:navbar:preview_as:1");
        assert_eq!(h.app.preferences.navigation_bar, before, "canvas preview is transient");
        navbar_click(&mut h, "prefs:navbar:add:zoom");
        assert_eq!(h.app.preferences.navigation_bar.slots.last().map(String::as_str), Some("zoom"));
        let shared = h.app.preferences.navigation_bar.clone();
        navbar_click(&mut h, "prefs:navbar:preview_as:0");
        assert_eq!(h.app.preferences.navigation_bar, shared, "Render edits persist in the shared Design preview");
        navbar_click(&mut h, "prefs:navbar:show");
        assert!(!h.app.ui.show_navigation_bar);
        navbar_click(&mut h, "prefs:navbar:show");
        assert!(h.app.ui.show_navigation_bar);
    }

    #[test]
    fn layout_page_checkboxes_keep_their_labels_at_every_width() {
        let checks = [
            ("history_play_button", "prefs.layout_page.play_history"),
            ("history_group_joints", "prefs.history_group_joints"),
            ("history_bar_groups", "prefs.history_bar_groups"),
            ("history_origin_bodies", "prefs.layout_page.origin_bodies"),
            ("browser_components_open", "prefs.browser_components_open"),
            ("browser_rows_start_closed", "prefs.browser_rows_start_closed"),
            ("browser_eye_left", "prefs.browser_eye_left"),
            ("browser_body_features", "prefs.browser_body_features"),
        ];
        for width in [1600, 1100, 960, 800, 640] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.language = "en".into();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":width,"height":1100}));
            h.call("ui.prefs", json!({"open":"Layout"}));
            h.frames(20);
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            for (key, label) in checks {
                let label = solidcraft_ui_text::tr(label);
                let check = crate::scenario::rect_of(&format!("prefs:{key}")).unwrap();
                assert!(check.left() >= body.left() - 0.5 && check.right() <= body.right() + 0.5, "{key} at {width}: {check:?} outside {body:?}");
                let mut text_found = false;
                for shape in &h.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| {
                        if let egui::Shape::Text(text) = shape
                            && text.galley.job.text == label
                        {
                            let bounds = text.galley.rect.translate(text.pos.to_vec2());
                            if check.contains(bounds.center()) {
                                assert!(
                                    !text.galley.elided && check.expand(0.5).contains_rect(bounds),
                                    "{key} label cut at {width}: {bounds:?} in {check:?}"
                                );
                                text_found = true;
                            }
                        }
                    });
                }
                assert!(text_found, "{key} shows its label \"{label}\" at {width}");
            }
        }
    }

    #[test]
    fn navigation_bar_appearance_segments_stay_inside_the_page_at_narrow_widths() {
        for width in [1600, 880, 640, 600, 480] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.language = "en".into();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":width,"height":1100}));
            h.call("ui.prefs", json!({"open":"Interface/Navigation bar", "navigation_bar":{"slots":["fit","visual_style","display","spin"]}}));
            h.frames(20);
            let header = crate::scenario::rect_of("dialog:header").unwrap();
            let search = crate::scenario::rect_of("prefs:search:frame").unwrap();
            assert!(header.contains_rect(search), "titlebar search stays inside the window at {width}");
            assert!(search.width() <= 230.0 && search.width() >= 100.0, "usable bounded search at {width}");
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            for index in 0..6 {
                let cell = crate::scenario::rect_of(&format!("prefs:navbar:anchor:{index}")).unwrap();
                assert!(cell.left() >= body.left() && cell.right() <= body.right() + 0.5, "anchor {index} at {width}: {cell:?} outside {body:?}");
                assert!(cell.width() > 20.0 && cell.height() > 20.0, "anchor {index} stays a usable target at {width}");
            }
            for (key, label, labels) in [
                ("size", "Size", &["Compact", "Regular"][..]),
                ("buttons", "Buttons", &["Icons", "Icons + labels"][..]),
                ("idle", "When idle", &["Always", "Fade", "Hover"][..]),
            ] {
                let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                let row = crate::scenario::rect_of(&format!("prefs:navbar:{key}")).unwrap();
                assert!(row.left() >= body.left() && row.right() <= body.right() + 0.5, "{key} at {width}: {row:?} outside {body:?}");
                assert_eq!(row.height(), crate::ui_kit::row_height(), "standard control height");
                let dropdown = crate::scenario::rect_of(&format!("dialog:segments:{label}")).is_none();
                if !dropdown {
                    let control = crate::scenario::rect_of(&format!("dialog:control:{label}")).unwrap();
                    assert!((row.width() - control.width()).abs() < 0.5, "{key} fills its property column at {width}");
                }
                if dropdown {
                    // Standard Segments fall back to the kit dropdown when their labels cannot fit.
                    navbar_click(&mut h, &format!("prefs:navbar:{key}"));
                    h.frames(2);
                }
                for (index, option) in labels.iter().enumerate() {
                    let cell = crate::scenario::rect_of(&format!("prefs:navbar:{key}:{index}")).unwrap();
                    let at = crate::scenario::handle_at(&format!("prefs:navbar:{key}:{index}")).unwrap();
                    assert!(cell.contains(at), "the scenario target is the actual choice");
                    let mut text_found = false;
                    for shape in &h.shapes {
                        crate::test_support::each_shape(&shape.shape, &mut |shape| {
                            if let egui::Shape::Text(text) = shape
                                && text.galley.job.text == *option
                            {
                                let bounds = text.galley.rect.translate(text.pos.to_vec2());
                                if cell.contains(bounds.center()) {
                                    assert!(
                                        !text.galley.elided && cell.contains_rect(bounds),
                                        "{key}:{index} text clips at {width}: {bounds:?} in {cell:?}"
                                    );
                                    text_found = true;
                                }
                            }
                        });
                    }
                    assert!(text_found, "{key}:{index} has painted, readable text at {width}");
                }
                if dropdown {
                    h.step(&json!({"click":{"handle":format!("prefs:navbar:{key}:{}", labels.len() - 1)}})).unwrap();
                    h.frames(2);
                }
            }
        }
    }

    #[test]
    fn navigation_bar_inspector_header_reserves_one_line_for_actions() {
        let t = solidcraft_theme::Tokens::light();
        for width in [0.0, 80.0, 200.0, 520.0, 640.0, 760.0, 880.0] {
            for remove_width in [75.0, 160.0] {
                let bounds = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(width, t.control_height));
                let (name, buttons, compact) = super::navbar::inspector_header_layout(bounds, remove_width, &t);
                assert!(bounds.contains_rect(name));
                for rect in buttons {
                    assert!(bounds.contains_rect(rect), "{width}: {rect:?} outside {bounds:?}");
                    assert_eq!(rect.top(), bounds.top());
                    assert_eq!(rect.bottom(), bounds.bottom());
                    assert!(name.right() <= rect.left());
                }
                assert!((buttons.last().unwrap().right() - bounds.right()).abs() < 0.01);
                for pair in buttons.windows(2) {
                    assert!(pair[0].right() <= pair[1].left());
                }
                if width >= 760.0 {
                    assert!(!compact, "wide rows keep Remove's label");
                } else if width <= 80.0 {
                    assert!(compact, "narrow rows keep Remove as an icon");
                }
                if !compact {
                    assert_eq!(buttons[0].width(), t.control_height, "Move left is always icon-only");
                    assert_eq!(buttons[1].width(), t.control_height, "Move right is always icon-only");
                    assert_eq!(buttons[2].width(), remove_width);
                }
            }
        }
    }

    #[test]
    fn navigation_bar_inspector_and_status_keep_full_width_for_every_selection() {
        for width in [480, 600, 880, 1030, 1200, 1600] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":width,"height":1100}));
            h.call(
                "ui.prefs",
                json!({"open":"Interface/Navigation bar", "navigation_bar":{"slots":["pan","fit","visual_style","display","separator"]}}),
            );
            h.frames(20);
            for slot in [None, Some("pan"), Some("fit"), Some("visual_style"), Some("display"), Some("separator")] {
                if let Some(slot) = slot {
                    navbar_click(&mut h, &format!("prefs:navbar:slot:{slot}"));
                    h.frames(3);
                }
                let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                if slot.is_some() {
                    let card = crate::scenario::rect_of("prefs:navbar:inspector").unwrap();
                    assert!(
                        (card.left() - body.left()).abs() < 0.5 && (card.right() - body.right()).abs() < 0.5,
                        "{slot:?} at {width}: {card:?} vs {body:?}"
                    );
                } else {
                    assert!(crate::scenario::rect_of("prefs:navbar:inspector").is_none(), "inspector belongs to a selected slot");
                }
                let status = crate::scenario::rect_of("prefs:navbar:status").unwrap();
                let reset = crate::scenario::rect_of("prefs:navbar:reset").unwrap();
                let show = crate::scenario::rect_of("prefs:navbar:show").unwrap();
                assert!(status.left() >= body.left() && status.right() <= body.right() + 0.5, "header actions stay inside the page");
                let stage = crate::scenario::rect_of("prefs:navbar:stage").unwrap();
                let count = crate::scenario::rect_of("prefs:navbar:count").unwrap();
                assert!(status.bottom() <= stage.top(), "Show and Reset belong above the shared stage");
                let bar = crate::scenario::rect_of("prefs:navbar:bar").unwrap();
                assert!(stage.contains_rect(count), "the real slot count sits in the stage");
                assert!(!count.intersects(bar), "the count never covers the bar");
                let properties = crate::scenario::rect_of("dialog:row:Position").unwrap();
                assert!(stage.bottom() <= properties.top(), "the stage stays before properties");
                assert!(status.contains_rect(reset) && status.contains_rect(show));
                assert!(show.right() <= reset.left());
                assert_eq!(reset.center().y, show.center().y);
                if slot.is_some() {
                    let header = crate::scenario::rect_of("prefs:navbar:inspector:header").unwrap();
                    let name = crate::scenario::rect_of("prefs:navbar:inspector:name").unwrap();
                    let actions: Vec<_> =
                        ["left", "right", "remove"].iter().map(|key| crate::scenario::rect_of(&format!("prefs:navbar:{key}")).unwrap()).collect();
                    for rect in &actions {
                        assert!(header.contains_rect(*rect), "{slot:?} at {width}: {rect:?} outside {header:?}");
                        assert_eq!(rect.center().y, header.center().y);
                        assert!(name.right() <= rect.left());
                    }
                    assert_eq!(actions[0].width(), header.height(), "Move left stays icon-only");
                    assert_eq!(actions[1].width(), header.height(), "Move right stays icon-only");
                    assert_eq!(actions.last().unwrap().right(), header.right());
                }
                for (_, at) in crate::scenario::handles("prefs:navbar:add:") {
                    assert!(at.x >= body.left() && at.x <= body.right(), "tray actions stay in their tiles");
                }
            }
        }
    }

    #[test]
    fn interface_sections_share_the_page_content_width_before_and_after_navigation() {
        for width in [600, 880, 1030, 1200, 1496, 1600] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":width,"height":1100}));
            for (page, sections, rows) in [
                ("Appearance", &["theme", "scale", "colour_vision"][..], &["dialog:divider:scale_font", "dialog:row:Font"][..]),
                ("Layout", &["layout", "navigation_bar"][..], &["prefs:navbar:stage"][..]),
            ] {
                h.call("ui.prefs", json!({"open":page}));
                h.frames(20);
                for _ in 0..2 {
                    let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                    let window = crate::scenario::rect_of("prefs:window").unwrap();
                    let t = solidcraft_theme::Tokens::get();
                    let inset = t.space_24 - t.space_2 + crate::ui_kit::navigation_popup_frame().stroke.width;
                    assert!((body.right() - (window.right() - inset)).abs() < 0.5, "{width}: {page}");
                    for key in sections {
                        let section = crate::scenario::rect_of(&format!("prefs:section:{key}")).unwrap();
                        assert!(
                            (section.left() - body.left()).abs() < 0.5 && (section.right() - body.right()).abs() < 0.5,
                            "{key} at {width}: {section:?} vs {body:?}"
                        );
                    }
                    for key in rows {
                        let row = crate::scenario::rect_of(key).unwrap();
                        assert!((row.right() - body.right()).abs() < 0.5, "{key} at {width}: {row:?} vs {body:?}");
                    }
                    h.call("ui.scroll", json!({"x":body.center().x,"y":body.center().y,"delta":-600}));
                    h.frames(20);
                }
            }
        }
    }

    #[test]
    fn navigation_bar_preview_stage_follows_anchor_live() {
        use crate::navigation_bar::Position;
        use crate::ui_kit::NavigationEdge;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar","navigation_bar":{"slots":["pan","fit","display"]}}));
        for (index, edge, position) in [
            (0, NavigationEdge::Top, Position::Left),
            (1, NavigationEdge::Top, Position::Centre),
            (2, NavigationEdge::Top, Position::Right),
            (3, NavigationEdge::Bottom, Position::Left),
            (4, NavigationEdge::Bottom, Position::Centre),
            (5, NavigationEdge::Bottom, Position::Right),
        ] {
            navbar_click(&mut h, &format!("prefs:navbar:anchor:{index}"));
            h.frames(3);
            let stage = crate::scenario::rect_of("prefs:navbar:stage").unwrap();
            let bar = crate::scenario::rect_of("prefs:navbar:bar").unwrap();
            let count = crate::scenario::rect_of("prefs:navbar:count").unwrap();
            assert_eq!((h.app.preferences.navigation_bar.edge, h.app.preferences.navigation_bar.position), (edge, position));
            assert_eq!(stage.height(), crate::ui_kit::NAV_STAGE_HEIGHT);
            let inset = crate::ui_kit::NAV_BORDER + crate::ui_kit::NAV_STAGE_INSET;
            match edge {
                NavigationEdge::Top => {
                    assert_eq!(bar.top(), stage.top() + inset);
                    assert!(count.top() >= bar.bottom(), "the count sits below a top bar");
                }
                NavigationEdge::Bottom => {
                    assert_eq!(bar.bottom(), stage.bottom() - inset);
                    assert!(count.bottom() <= bar.top(), "the count sits above a bottom bar");
                }
            }
            match position {
                Position::Left => assert_eq!(bar.left(), stage.left() + inset),
                Position::Centre => assert_eq!(bar.center().x, stage.center().x),
                Position::Right => assert_eq!(bar.right(), stage.right() - inset),
            }
        }
    }

    #[test]
    fn navigation_bar_editor_drag_adds_reorders_and_removes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar", "navigation_bar":{"slots":["pan","fit","display"]}}));
        h.frames(20);
        let preview = crate::scenario::rect_of("prefs:navbar:preview").unwrap();
        h.step(&json!({"drag":[{"handle":"prefs:navbar:tile:spin"}, [preview.right()-8.0,preview.center().y]],"steps":12})).unwrap();
        h.frames(4);
        assert_eq!(h.app.preferences.navigation_bar.slots, ["pan", "fit", "display", "spin"]);
        let pan = crate::scenario::rect_of("prefs:navbar:slot:pan").unwrap();
        h.step(&json!({"drag":[{"handle":"prefs:navbar:slot:spin"}, [pan.left()+1.0,pan.center().y]],"steps":12})).unwrap();
        h.frames(4);
        assert_eq!(h.app.preferences.navigation_bar.slots, ["spin", "pan", "fit", "display"]);
        let preview = crate::scenario::rect_of("prefs:navbar:preview").unwrap();
        h.step(&json!({"drag":[{"handle":"prefs:navbar:slot:spin"}, [preview.center().x,preview.bottom()+80.0]],"steps":12})).unwrap();
        h.frames(4);
        assert_eq!(h.app.preferences.navigation_bar.slots, ["pan", "fit", "display"]);
        // Empty stage space is off the bar too; it must remove rather than append a moved slot.
        h.step(&json!({"drag":[{"handle":"prefs:navbar:slot:pan"}, [preview.right()-4.0,preview.center().y]],"steps":12})).unwrap();
        h.frames(4);
        assert_eq!(h.app.preferences.navigation_bar.slots, ["fit", "display"]);
    }

    #[test]
    fn navigation_bar_menu_options_cancel_defaults_and_section_reset() {
        use crate::navigation_bar::NavigationBar;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let before = h.app.preferences.navigation_bar.clone();
        let options = json!({"slots":["visual_style","pan"],"slot_options":{"visual_style":{"shows":"icon_and_value"}},"size":"regular"});
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar","navigation_bar":options}));
        h.app.run("view.navigation_bar", json!({"visible":false})).unwrap();
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.navigation_bar, before);
        assert!(h.app.ui.show_navigation_bar);
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar","navigation_bar":options}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert_eq!(h.app.preferences.navigation_bar, NavigationBar::default());
        h.call("ui.prefs", json!({"navigation_bar":options}));
        navbar_click(&mut h, "prefs:navbar:reset");
        assert_eq!(h.app.preferences.navigation_bar, NavigationBar::default());
        navbar_click(&mut h, "dialog:section:Navigation bar");
        assert!(crate::scenario::handle_at("prefs:navbar:slot:pan").is_none(), "the section collapsed");
        h.call("ui.prefs", json!({"open":"Interface/Navigation bar"}));
        h.frames(20);
        let preview = crate::scenario::rect_of("prefs:navbar:preview").unwrap();
        let body = crate::scenario::rect_of("prefs:page_body").unwrap();
        assert!(body.contains(preview.center()), "the route reopens and scrolls to the section");
    }

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
        assert_ne!(solidcraft_ui_text::tr("ui.3d_mouse_none"), solidcraft_ui_text::tr("ui.3d_mouse_calibrate_wait"));
        // A confirmed device is neither (the status names it).
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = Some("spacenavd".into());
        h.app.space_mouse.drivers = Some(DriversPresent { spacenavd: true, direct: false, denied: false });
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
        h.frames(3);
        assert!(h.app.prefs_window.shell.open);
    }

    /// Calibration starts while Preferences is open, and its window is on top of it; with no device
    /// known there is nothing to start (the button is off and says why, see the hint key).
    #[test]
    fn calibration_opens_over_an_open_preferences_and_waits_for_a_device() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        // After a driver change and before any reading: no device, so nothing starts.
        h.app.space_mouse.device = None;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
        h.frames(5);
        assert!(solidcraft_ui_text::tr("ui.3d_mouse_calibrate_wait").starts_with("Move the 3D mouse"));
        crate::space_mouse_calibration::start(&mut h.app);
        h.frames(3);
        assert!(h.app.space_mouse.calibrating.is_none() && h.app.prefs_window.shell.open);
        // The device is seen: the window opens, above Preferences, which stays open.
        h.app.space_mouse.device = Some("hid 046d:c62b".into());
        h.frames(3);
        crate::space_mouse_calibration::start(&mut h.app);
        h.frames(5);
        assert!(h.app.space_mouse.calibrating.is_some() && h.app.prefs_window.shell.open);
        let window = crate::scenario::rect_of("dialog:window").expect("the calibration window is the last window drawn");
        let top = h.ctx.layer_id_at(window.center());
        assert!(top.is_some_and(|layer| layer.id == egui::Id::unique("sc_space_mouse_calibration")), "top is {top:?}");
    }

    /// Scrolls vertically to the handle, then clicks after the wheel animation settles.
    /// Edge-aligned controls are valid targets; scrolling cannot move them inward horizontally.
    fn scroll_to_and_click(h: &mut crate::scenario::Harness, handle: &str) {
        for _ in 0..60 {
            let body = crate::scenario::rect_of("prefs:page_body").expect("the page body");
            let target = crate::scenario::handle_at(handle);
            if let Some(at) = target
                && body.contains(at)
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
                source: egui::MouseWheelSource::Unknown,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, if target.is_some_and(|at| at.y < body.top()) { 60.0 } else { -60.0 }),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            });
            h.frames(2);
        }
        panic!("{handle} never came into view");
    }

    #[test]
    fn folding_a_3d_mouse_section_does_not_move_navigation_controls() {
        for width in [900, 1100, 1318, 1600, 1920] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":width,"height":900}));
            open_at(&mut h.app, PAGE_NAVIGATION);
            h.frames(6);
            let rows = ["Middle double-click", "View navigator"];
            let controls = || rows.map(|name| crate::scenario::rect_of(&format!("dialog:control:{name}")));
            let before = controls();
            assert!(before.iter().all(Option::is_some));
            open_at(&mut h.app, PAGE_MOUSE3D);
            h.frames(6);
            assert!(h.widget("Middle double-click").is_none());
            scroll_to_and_click(&mut h, "dialog:section:Device");
            open_at(&mut h.app, PAGE_NAVIGATION);
            h.frames(6);
            assert_eq!(controls(), before);
        }
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
        p.space_mouse_model_pivot = true;
        p.space_mouse_sketch_orbit = true;
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
        assert_eq!((p.space_mouse_model_pivot, p.space_mouse_sketch_orbit), (d.space_mouse_model_pivot, d.space_mouse_sketch_orbit));
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

    /// The 3D mouse's orbit is a choice of two: Classic turns around the view's centre, Traced
    /// around the model in the middle of the view.
    #[test]
    fn the_3d_mouse_orbit_is_traced_or_classic() {
        let mut h = changed_3d_mouse_settings(None);
        assert!(!h.app.preferences.space_mouse_model_pivot, "classic is the default");
        scroll_to_and_click(&mut h, "prefs:3d_mouse_orbit_traced");
        assert!(h.app.preferences.space_mouse_model_pivot, "traced is opt-in");
        scroll_to_and_click(&mut h, "prefs:3d_mouse_orbit_classic");
        assert!(!h.app.preferences.space_mouse_model_pivot, "classic again");
        open_at(&mut h.app, PAGE_NAVIGATION);
        h.frames(4);
        assert!(!h.app.preferences.zoom_stop_at_model, "stop-at-model zoom is off by default");
        scroll_to_and_click(&mut h, "prefs:zoom_stop_at_model");
        assert!(h.app.preferences.zoom_stop_at_model, "stop-at-model zoom is opt-in");
        assert!(h.app.preferences.zoom_to_cursor, "the wheel preference is unchanged");
    }

    #[test]
    fn model_navigation_is_opt_in_and_persists() {
        for to_cursor in [false, true] {
            let old = serde_json::from_value::<Prefs>(json!({"zoom_to_cursor": to_cursor})).unwrap();
            assert!(!old.zoom_stop_at_model && !old.space_mouse_model_pivot, "older saved preferences keep legacy navigation");
            assert_eq!(old.zoom_to_cursor, to_cursor);
        }
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(!app.preferences.zoom_stop_at_model && !app.preferences.space_mouse_model_pivot);
        let enabled = json!({"open": "Navigation", "zoom_stop_at_model": true, "space_mouse_model_pivot": true});
        control(&mut app, &enabled).unwrap();
        control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert!(!app.preferences.zoom_stop_at_model && !app.preferences.space_mouse_model_pivot);
        let reply = control(&mut app, &enabled).unwrap();
        assert_eq!(reply["zoom_stop_at_model"], true);
        assert_eq!(reply["space_mouse_model_pivot"], true);
        control(&mut app, &json!({"button": "apply"})).unwrap();
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert!(other.preferences.zoom_stop_at_model && other.preferences.space_mouse_model_pivot);
        assert!(other.preferences.zoom_to_cursor, "enabling model navigation leaves wheel zoom alone");
        control(&mut app, &json!({"button": "defaults"})).unwrap();
        assert!(!app.preferences.zoom_stop_at_model);
        assert!(app.preferences.space_mouse_model_pivot, "Navigation defaults leave 3D mouse settings alone");
        control(&mut app, &json!({"open":"3D mouse", "button":"defaults"})).unwrap();
        assert!(!app.preferences.space_mouse_model_pivot);
        assert!(app.preferences.zoom_to_cursor, "Navigation defaults still anchor the wheel");
        control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert!(app.preferences.zoom_stop_at_model && app.preferences.space_mouse_model_pivot, "Cancel restores the last Apply");
    }

    /// Opens Preferences on the 3D mouse page with its section open and these settings
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
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
        h.frames(36);
        h
    }

    /// Axes and buttons have independent search homes and stay available when Advanced folds.
    #[test]
    fn the_3d_mouse_sections_keep_axes_and_buttons_outside_advanced() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = Some("hid 046d:c62b".into());
        h.app.space_mouse.seen = 1;
        h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: true, denied: false });
        h.call("ui.resize", json!({"width":1600,"height":4000}));
        open_at(&mut h.app, PAGE_MOUSE3D);
        h.frames(36);
        let sections = ["space_mouse_device", "space_mouse_orbit", "space_mouse_axes", "space_mouse_buttons", "space_mouse_advanced"];
        let bodies = sections.map(|key| crate::scenario::rect_of(&format!("prefs:section:{key}:body")).unwrap());
        assert!(bodies.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()));
        assert!(crate::scenario::rect_of("dialog:row:Button 1").is_some());
        assert!(crate::scenario::rect_of("dialog:row:Driver").is_some());
        scroll_to_and_click(&mut h, "dialog:section:Advanced");
        assert!(crate::scenario::rect_of("dialog:row:Driver").is_none());
        assert!(crate::scenario::rect_of("dialog:row:Button 1").is_some());
        assert!(crate::scenario::rect_of("prefs:space_mouse_reset_axes").is_some());
    }

    #[test]
    fn mouse3d_device_card_uses_content_height_and_paints_status_and_actions_at_its_top() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let t = Tokens::get();
            for width in [280.0, 646.0] {
                for (drivers, status_id) in [
                    (None, "ui.3d_mouse_none"),
                    (Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: false, denied: false }), "ui.3d_mouse_calibrate_wait"),
                    (Some(crate::space_mouse::DriversPresent { spacenavd: false, direct: true, denied: true }), "ui.3d_mouse_denied_short"),
                ] {
                    let mut heights = Vec::new();
                    for viewport_height in [900.0, 1800.0] {
                        let ctx = crate::fonts::test_context();
                        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
                        app.preferences.language = "en".into();
                        app.space_mouse.drivers = drivers;
                        let before = app.preferences.clone();
                        crate::scenario::clear_handles();
                        let output = crate::test_support::frame(
                            &ctx,
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, viewport_height))),
                                ..Default::default()
                            },
                            |ui| {
                                ui_kit::scope(ui, |ui| {
                                    ui.set_width(width);
                                    super::space_mouse_section(&mut app, ui);
                                });
                            },
                        );
                        let card = crate::scenario::rect_of("prefs:space_mouse_device_card").unwrap();
                        let status = crate::scenario::rect_of("prefs:space_mouse_status").unwrap();
                        let calibrate = crate::scenario::rect_of("prefs:space_mouse_calibrate").unwrap();
                        assert!(card.height() < t.control_height * 4.0 + t.space_24 * 2.0, "{width}/{viewport_height}: compact card {card:?}");
                        assert!(card.contains_rect(status) && card.contains_rect(calibrate));
                        assert!(status.top() - card.top() < t.control_height, "the status belongs at the card's top");
                        assert!(calibrate.top() - status.bottom() <= t.space_24, "actions follow the status without viewport-sized space");
                        for label in [app.tr(status_id), app.tr("ui.3d_mouse_calibrate")] {
                            let mut painted = 0;
                            for clipped in &output.shapes {
                                crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                                    if let egui::Shape::Text(text) = shape
                                        && text.galley.text() == label
                                    {
                                        painted += 1;
                                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                                        assert!(!text.galley.elided && card.expand(1.0).contains_rect(rect));
                                        assert!(clipped.clip_rect.expand(1.0).contains_rect(rect), "{label} must be fully painted");
                                    }
                                });
                            }
                            assert_eq!(painted, 1, "one actual {label} is visible");
                        }
                        assert_eq!(app.preferences, before);
                        assert!(app.space_mouse.device.is_none() && app.space_mouse.calibrating.is_none());
                        heights.push(card.height());
                    }
                    assert!((heights[0] - heights[1]).abs() < 0.5, "viewport height must not stretch the device card");
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn reset_axis_assignment_preserves_gains_inversion_and_every_other_setting() {
        for theme in ["light", "dark"] {
            let mut h = changed_3d_mouse_settings(Some("hid 046d:c62b"));
            h.call("ui.prefs", json!({"theme":theme}));
            let mut expected = h.app.preferences.clone();
            expected.space_mouse_axis_source = crate::space_mouse::SAME_AXES;
            scroll_to_and_click(&mut h, "prefs:space_mouse_reset_axes");
            assert_eq!(h.app.preferences, expected);
            h.app.preferences.space_mouse_axis_source = [1, 0, 2, 3, 4, 5];
            h.app.preferences.space_mouse_enabled = false;
            h.frames(3);
            let before = h.app.preferences.clone();
            scroll_to_and_click(&mut h, "prefs:space_mouse_reset_axes");
            assert_eq!(h.app.preferences, before, "disabled navigation cannot reset assignments");
        }
    }
    /// The question is short, says what stays, and exists in every language.
    #[test]
    fn the_restore_question_is_short_and_says_what_stays() {
        crate::i18n::install();
        let english = solidcraft_ui_text::tr("ui.3d_mouse_restore_ask");
        assert_eq!(english, "Set the 3D mouse settings back to their defaults? Your calibration and button mapping stay.");
        for catalog in crate::i18n::LOCALES {
            let text = solidcraft_ui_text::Locale::for_language(catalog.id).text("ui.3d_mouse_restore_ask");
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
        assert!(!h.app.prefs_window.shell.open, "the dialog closed");
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
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
        h.call("ui.prefs", json!({"button":"apply"}));
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
        crate::prefs::open_at(&mut prefs.app, crate::prefs::PAGE_MOUSE3D);
        prefs.frames(6);
        prefs.frames(30);
        assert!(prefs.app.space_mouse.calibrating.is_none());
        scroll_to_and_click(&mut prefs, "dialog:button:Calibration:Calibrate…");
        assert!(card.app.prefs_window.shell.open && prefs.app.prefs_window.shell.open);
        assert!(wizard(&card).is_some(), "the card's button started it");
        assert_eq!(wizard(&card), wizard(&prefs), "and Preferences' starts the same one");
    }

    /// Direct or Automatic chosen with the device present but unopenable: the daemon is read, and Preferences
    /// says so next to the setting instead of leaving "Direct (hidraw)" to imply the device. Spacenavd
    /// chosen, or the device itself read, or no device: no such note.
    #[test]
    fn preferences_says_when_the_daemon_is_read_instead_of_the_device_asked_for() {
        crate::i18n::install();
        use SpaceMouseDriver::{Automatic, Direct, Spacenavd};
        let note = solidcraft_ui_text::tr("ui.3d_mouse_driver_fallback").to_string();
        assert!(note.contains("spacenavd") && note.contains("could not be opened"));
        for catalog in crate::i18n::LOCALES {
            let text = solidcraft_ui_text::Locale::for_language(catalog.id).text("ui.3d_mouse_driver_fallback");
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
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
            h.frames(6);
            h.frames(30);
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
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
            h.frames(6);
            // The redesigned Advanced section starts expanded.
            // The restore question is up, so its whole text can be checked too.
            h.app.space_mouse.restore_pending = true;
            // The frame's language is only set while it runs, so the texts come from the catalog itself.
            let text = |key: &str| solidcraft_ui_text::Locale::for_language(catalog.id).text(key).to_string();
            h.frames(30);
            let body = crate::scenario::rect_of("prefs:page_body").expect("the page body");
            // The question: all of it is laid out (its box is as tall as its wrapped text needs) and the
            // buttons start below it.
            let question = text("ui.3d_mouse_restore_ask");
            match crate::scenario::rect_of("dialog:hint:space_mouse_restore_ask") {
                Some(note) => {
                    let t = solidcraft_theme::Tokens::get();
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
        let font = egui::FontId::proportional(solidcraft_theme::Tokens::get().font_label);
        let mut wide = Vec::new();
        for catalog in crate::i18n::LOCALES {
            use crate::space_mouse_offer::Action;
            for key in [Action::Swap, Action::Calibrate, Action::Never, Action::Close].map(|action| action.label()) {
                let label = solidcraft_ui_text::Locale::for_language(catalog.id).text(key).to_string();
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
            let say = |row: usize| solidcraft_ui_text::Locale::for_language("en").format(row_label(mapping, row), &[("n", &(row + 1).to_string())]);
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
        crate::i18n::install();
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
        assert!(solidcraft_ui_text::tr("ui.3d_mouse_circles_help").contains("test model"));
    }

    /// The "wiggle it" hint is a visible line as well as a tooltip, only while a reader is reachable and
    /// silent; the section's own header carries no badge any more.
    #[test]
    fn a_visible_line_says_what_the_3d_mouse_waits_for() {
        crate::i18n::install();
        assert_eq!(solidcraft_ui_text::tr("ui.3d_mouse_waiting"), "Waiting for Movement");
        assert_eq!(solidcraft_ui_text::tr("ui.3d_mouse_reset_view"), "Reset view");
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.space_mouse.device = None;
        h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: false, denied: false });
        h.call("ui.resize", serde_json::json!({"width": 1600, "height": 4000}));
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_MOUSE3D);
        h.frames(5);
        assert!(crate::scenario::rect_of("dialog:section:Device").is_some(), "the section is drawn with the kit's header");
        assert!(crate::scenario::rect_of("dialog:section:Device:count").is_none());
        assert!(h.widget(solidcraft_ui_text::tr("ui.3d_mouse_calibrate_wait")).is_some());
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
        assert_eq!(solidcraft_ui_text::tr("ui.3d_mouse_reset_view"), "Reset view");
    }

    /// The "wiggle it" hint exists in every catalog and is a visible line while no device is confirmed,
    /// gone once there is one.
    #[test]
    fn the_wiggle_hint_has_text_and_shows_until_a_device_is_confirmed() {
        crate::i18n::install();
        assert_eq!(solidcraft_ui_text::tr("ui.3d_mouse_wiggle"), "If you have one plugged in, wiggle it.");
        for catalog in crate::i18n::LOCALES {
            let text = solidcraft_ui_text::Locale::for_language(catalog.id).text("ui.3d_mouse_wiggle");
            assert!(!text.is_empty() && text != "ui.3d_mouse_wiggle", "{}", catalog.id);
        }
        let mut h = changed_3d_mouse_settings(None);
        h.app.space_mouse.drivers = Some(crate::space_mouse::DriversPresent { spacenavd: true, direct: false, denied: false });
        h.frames(10);
        assert!(h.widget(solidcraft_ui_text::tr("ui.3d_mouse_calibrate_wait")).is_some(), "the reader waits for a real record");
        h.app.space_mouse.device = Some("spacenavd".into());
        h.frames(10);
        assert!(crate::scenario::rect_of("prefs:space_mouse_status").is_none(), "confirmed: the real device replaces the waiting status");
    }

    /// A 3D mouse that is there but may not be read, with no spacenavd to ask instead: Preferences says so
    /// in plain words and does not ask for a movement; with a daemon reachable it still waits for one.
    #[test]
    fn preferences_says_when_a_device_is_found_but_not_allowed() {
        crate::i18n::install();
        for catalog in crate::i18n::LOCALES {
            for key in ["ui.3d_mouse_denied", "ui.3d_mouse_denied_short"] {
                let text = solidcraft_ui_text::Locale::for_language(catalog.id).text(key);
                assert!(text.chars().count() > 5 && text != key, "{} {key}", catalog.id);
            }
        }
        assert!(solidcraft_ui_text::tr("ui.3d_mouse_denied").contains("udev"));
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
            let said = h.widget(solidcraft_ui_text::tr("ui.3d_mouse_denied_short")).is_some();
            assert_eq!(said, shown, "spacenavd {spacenavd}, found {direct}, denied {denied}");
            let wiggle = h
                .widget(solidcraft_ui_text::tr(if super::waiting_for_a_record(h.app.space_mouse.drivers) {
                    "ui.3d_mouse_calibrate_wait"
                } else {
                    "ui.3d_mouse_none"
                }))
                .is_some();
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
        for (label, segments) in [("Coarse", 8), ("Normal", 16), ("Fine", 32), ("Ultra", 64)] {
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
            assert_eq!(facets, segments + 1, "{label} uses the canonical faceting caption, with the closing vertex");
            previous = facets;
        }
    }

    #[test]
    fn design_precision_controls_use_preferred_units_and_keep_document_and_transactions() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", json!({"language":"en", "theme":theme}));
            // A nonblank document keeps its units while the new-design defaults are edited.
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            let doc = h.app.session.doc.to_json();
            let original = h.app.preferences.clone();
            h.call("ui.prefs", json!({"open":"General"}));
            h.step(&json!({"click":{"handle":"prefs:page:Design"}})).unwrap();
            h.app.prefs_window.shell.filter = "Units and precision".into();
            h.frames(5);
            for unit in UNITS {
                h.step(&json!({"click":{"handle":format!("prefs:default_units:{unit}")}})).unwrap();
                h.frames(2);
                let sample = format_length(12.5 * unit_scale(unit).0, unit, 2);
                assert!(h.widget(&sample).is_some(), "{theme}: {unit} uses the real unit-aware sample {sample}");
                assert_eq!(h.app.session.doc.to_json(), doc);
            }
            for (key, text) in [("length_decimals", "4"), ("angle_decimals", "3"), ("parameter_list_rows", "75")] {
                let handle = format!("prefs:{key}");
                super::reveal_preference(&mut h, &handle);
                h.step(&json!({"click":{"handle":handle},"double":true})).unwrap();
                h.step(&json!({"key":"A","cmd":true})).unwrap();
                h.step(&json!({"text":text})).unwrap();
                h.step(&json!({"key":"Enter"})).unwrap();
                h.frames(2);
            }
            assert_eq!((h.app.preferences.length_decimals, h.app.preferences.angle_decimals, h.app.preferences.parameter_list_rows), (4, 3, 75));
            assert!(h.widget(&format_length(12.5 * unit_scale("ft").0, "ft", 4)).is_some());
            assert!(h.widget("45.000°").is_some());
            assert_eq!(saved_prefs(&h.app)["preferences"]["length_decimals"], original.length_decimals);
            h.call("ui.prefs", json!({"button":"apply"}));
            let applied = h.app.preferences.clone();
            let mut restarted = SolveApp::new(Default::default(), Default::default());
            restarted.load_prefs(&h.app.prefs());
            assert_eq!(
                (
                    restarted.preferences.default_units.as_str(),
                    restarted.preferences.length_decimals,
                    restarted.preferences.angle_decimals,
                    restarted.preferences.parameter_list_rows
                ),
                ("ft", 4, 3, 75)
            );
            h.call("ui.prefs", json!({"button":"defaults"}));
            assert_eq!((h.app.preferences.length_decimals, h.app.preferences.angle_decimals, h.app.preferences.parameter_list_rows), (2, 1, 50));
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences, applied);
            assert_eq!(h.app.session.doc.to_json(), doc);
        }
    }

    #[test]
    fn design_unknown_program_dropdown_keeps_note_default_and_stand_up_dependency() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"language":"en","open":"Design"}));
        assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Note);
        h.app.prefs_window.shell.filter = "Files from unknown programs".into();
        h.frames(5);
        super::reveal_preference(&mut h, "prefs:stand_up_unknown");
        h.step(&json!({"click":{"handle":"prefs:stand_up_unknown"}})).unwrap();
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:list:prefs.stand_up_unknown.ask"}})).unwrap();
        assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Ask);
        h.app.prefs_window.shell.filter = "Offer to stand up Y-up files".into();
        h.frames(5);
        super::reveal_preference(&mut h, "prefs:offer_stand_up");
        h.step(&json!({"click":{"handle":"prefs:offer_stand_up"}})).unwrap();
        assert!(!h.app.preferences.offer_stand_up);
        h.app.prefs_window.shell.filter = "Files from unknown programs".into();
        h.frames(5);
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let dropdown =
            tree.nodes.iter().find(|(_, node)| node.role() == egui::accesskit::Role::ComboBox && node.label() == Some("Files from unknown programs"));
        assert!(dropdown.expect("unknown-program dropdown").1.is_disabled());
        assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Ask, "disabling the offer keeps the stored choice");
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.offer_stand_up);
        assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Note);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Note);
    }

    #[test]
    fn tools_ghost_and_pointer_tiles_edit_only_their_state_and_preserve_transactions() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", json!({"language":"en","theme":theme,"quick_measure":true}));
            let original = h.app.preferences.clone();
            h.call("ui.prefs", json!({"open":"General"}));
            h.step(&json!({"click":{"handle":"prefs:page:Tools"}})).unwrap();
            let mut expected = serde_json::to_value(&original).unwrap();
            for (query, field) in [
                ("Show moved parts' old place as a ghost", "drive_joints_ghost"),
                ("Axis distances in Measure", "measure_axis_distances"),
                ("Direct interaction", "direct_interaction"),
            ] {
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let handle = format!("prefs:{field}");
                super::reveal_preference(&mut h, &handle);
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                expected[field] = json!(!expected[field].as_bool().unwrap());
                assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected, "{theme}: {field} edits only its own preference");
            }
            h.app.prefs_window.shell.filter = "Tool at the cursor".into();
            h.frames(5);
            super::reveal_preference(&mut h, "view:tool_cursor");
            h.step(&json!({"click":{"handle":"view:tool_cursor"}})).unwrap();
            assert!(h.app.ui.tool_cursor);
            assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected, "cursor stays app UI state");
            assert_eq!(saved_prefs(&h.app)["preferences"], serde_json::to_value(&original).unwrap());
            h.call("ui.prefs", json!({"button":"apply"}));
            let applied = h.app.preferences.clone();
            let mut restarted = SolveApp::new(Default::default(), Default::default());
            restarted.load_prefs(&h.app.prefs());
            assert_eq!(restarted.preferences, applied);
            assert!(restarted.ui.tool_cursor);
            h.call("ui.prefs", json!({"button":"defaults"}));
            assert!(h.app.preferences.drive_joints_ghost && h.app.preferences.measure_axis_distances);
            assert!(!h.app.preferences.direct_interaction && !h.app.ui.tool_cursor);
            assert!(h.app.preferences.quick_measure, "Tools defaults leave Graphics quick measurement alone");
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences, applied);
            assert!(h.app.ui.tool_cursor);
        }
    }

    #[test]
    fn tools_extrude_view_controls_keep_their_dependency_without_clearing_the_saved_choice() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"language":"en","open":"Tools"}));
        h.app.prefs_window.shell.filter = "Extrude".into();
        h.frames(5);
        for (key, isometric, returns) in [
            ("extrude_view_return", false, false),
            ("extrude_view_isometric", true, false),
            ("extrude_view_return", true, true),
            ("extrude_view_isometric", false, true),
            ("extrude_view_return", false, true),
        ] {
            let handle = format!("prefs:{key}");
            super::reveal_preference(&mut h, &handle);
            h.step(&json!({"click":{"handle":handle}})).unwrap();
            h.frames(2);
            assert_eq!((h.app.preferences.extrude_view_isometric, h.app.preferences.extrude_view_return), (isometric, returns));
            assert!(h.app.preferences.auto_operation);
            assert!(h.app.preferences.extrude_auto_to_object);
        }
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.extrude_view_isometric && !h.app.preferences.extrude_view_return);
    }

    #[test]
    fn moved_preferences_reset_only_on_their_new_page() {
        use super::*;
        let fields = [
            ("link_step_inserts", json!(false), PAGE_DESIGN),
            ("default_units", json!("ft"), PAGE_DESIGN),
            ("length_decimals", json!(5), PAGE_DESIGN),
            ("angle_decimals", json!(4), PAGE_DESIGN),
            ("parameter_list_rows", json!(75), PAGE_DESIGN),
            ("step_values", json!(true), PAGE_DESIGN),
            ("value_boxes_in_view", json!(false), PAGE_DESIGN),
            ("up_axis", json!("y"), PAGE_DESIGN),
            ("offer_stand_up", json!(false), PAGE_DESIGN),
            ("stand_up_unknown", json!("ask"), PAGE_DESIGN),
            ("hide_origin", json!(true), PAGE_DESIGN),
            ("update_changed_links", json!(false), PAGE_DESIGN),
            ("double_click_components", json!(false), PAGE_DESIGN),
            ("insert_lists_open_designs", json!(false), PAGE_DESIGN),
            ("theme", json!("light"), PAGE_APPEARANCE),
            ("ui_scale", json!(1.25), PAGE_APPEARANCE),
            ("ui_font", json!("test font"), PAGE_APPEARANCE),
            ("layout", json!("compact"), PAGE_LAYOUT),
            ("live_panel_drag_preview", json!(false), PAGE_LAYOUT),
            ("ribbon_labels", json!(false), PAGE_LAYOUT),
            ("persistent_ribbon_pressed", json!(false), PAGE_LAYOUT),
            ("viewport_analysis_status", json!(false), PAGE_GRAPHICS),
            ("viewport_section_status", json!(false), PAGE_GRAPHICS),
            ("viewport_status_details", json!(false), PAGE_GRAPHICS),
            ("viewport_status_summary", json!(false), PAGE_GRAPHICS),
            ("sketch_tab_always", json!(true), PAGE_LAYOUT),
            ("keep_work_on_tab_switch", json!(true), PAGE_LAYOUT),
            ("ribbon_reveal", json!(true), PAGE_LAYOUT),
            ("tab_double_click_renames", json!(false), PAGE_LAYOUT),
            ("dialog_buttons", json!("top"), PAGE_LAYOUT),
            ("toast_place", json!("centre"), PAGE_LAYOUT),
            ("operations_panel", json!(false), PAGE_LAYOUT),
            ("file_operations_wait", json!(true), PAGE_LAYOUT),
            ("ribbon_icon_grid", json!(true), PAGE_LAYOUT),
            ("construct_menu_groups", json!(true), PAGE_LAYOUT),
            ("ribbon_menus_follow_hover", json!(true), PAGE_LAYOUT),
            ("ribbon_menus_skip_pinned", json!(true), PAGE_LAYOUT),
            ("history_sketches", json!("under_feature"), PAGE_LAYOUT),
            ("history_play_button", json!(false), PAGE_LAYOUT),
            ("history_folder_kind_icon", json!(true), PAGE_LAYOUT),
            ("history_group_joints", json!(true), PAGE_LAYOUT),
            ("history_bar_groups", json!(true), PAGE_LAYOUT),
            ("color_vision", json!("protan"), PAGE_APPEARANCE),
            ("viewport_theme", json!("light"), PAGE_APPEARANCE),
            ("viewport_background", json!([255, 255, 255]), PAGE_APPEARANCE),
            ("view_confirm_corner", json!(true), PAGE_LAYOUT),
            ("status_bar", json!(true), PAGE_LAYOUT),
            ("browser_eye_left", json!(true), PAGE_LAYOUT),
            ("browser_components_open", json!(true), PAGE_LAYOUT),
            ("browser_rows_start_closed", json!(true), PAGE_LAYOUT),
            ("quick_measure", json!(true), PAGE_GRAPHICS),
            ("origin_plane_names", json!("top_front_right"), PAGE_DESIGN),
            ("direct_interaction", json!(true), PAGE_TOOLS),
            ("hole_snaps", json!(true), PAGE_TOOLS),
            ("hole_moves", json!(true), PAGE_TOOLS),
            ("hole_several", json!(true), PAGE_TOOLS),
            ("hole_group", json!(true), PAGE_TOOLS),
            ("extrude_arrow_face_stops", json!(false), PAGE_TOOLS),
            ("extrude_auto_to_object", json!(false), PAGE_TOOLS),
            ("extrude_view_isometric", json!(true), PAGE_TOOLS),
            ("extrude_view_return", json!(true), PAGE_TOOLS),
            ("auto_operation", json!(false), PAGE_TOOLS),
            ("placement_handles", json!(true), PAGE_TOOLS),
            ("assembly_move_handles", json!(false), PAGE_TOOLS),
            ("joint_rotation_coarse", json!(12.0), PAGE_TOOLS),
            ("joint_rotation_fine", json!(0.2), PAGE_TOOLS),
            ("drive_joints_ghost", json!(false), PAGE_TOOLS),
            ("measure_axis_distances", json!(false), PAGE_TOOLS),
            ("network_locations", json!(true), PAGE_GENERAL),
            ("network_hosts", json!(["files.studio"]), PAGE_GENERAL),
            ("nav_bridge_enabled", json!(true), PAGE_MOUSE3D),
            ("nav_bridge_port", json!(9876), PAGE_MOUSE3D),
            ("space_mouse_speed", json!(2.0), PAGE_MOUSE3D),
            ("nav", json!("blender"), PAGE_NAVIGATION),
            ("look_at_anim", json!(false), PAGE_NAVIGATION),
            ("keep_hover_while_navigating", json!(false), PAGE_NAVIGATION),
            ("selection_shift_add_only", json!(true), PAGE_SKETCH),
            ("dimension_text_scale", json!(1.5), PAGE_SKETCH),
            ("dimension_colour", json!([18, 52, 86]), PAGE_SKETCH),
            ("select_other_hidden", json!(false), PAGE_SKETCH),
            ("select_hidden_edges", json!(true), PAGE_SKETCH),
            ("sketch_conflict_colour", json!(false), PAGE_SKETCH),
            ("hide_constraint_glyphs_zoomed_out", json!(false), PAGE_SKETCH),
            ("constraint_glyph_scale", json!(2.0), PAGE_SKETCH),
            ("suppress_sketch_snap", json!(ModifierCombo::NONE), PAGE_SKETCH),
            ("sketch_state_icons", json!(false), PAGE_SKETCH),
            ("sketch_mirror_auto_line", json!(false), PAGE_SKETCH),
            ("sketch_tracking_guides", json!(false), PAGE_SKETCH),
            ("sketch_tracking_points", json!(false), PAGE_SKETCH),
            ("sketch_align_points", json!(true), PAGE_SKETCH),
            ("sketch_recent_snap", json!(true), PAGE_SKETCH),
            ("sketch_snap_to_visible_grid", json!(false), PAGE_SKETCH),
            ("line_hover_tangent_arc", json!(true), PAGE_SKETCH),
            ("ribbon_click_ends_tool", json!(false), PAGE_SKETCH),
            ("sketch_cursor_coordinates", json!(false), PAGE_SKETCH),
            ("msaa", json!(8), PAGE_GRAPHICS),
            ("single_surface_analysis", json!(false), PAGE_GRAPHICS),
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
        assert_eq!(page_index("Assistant"), Some(PAGE_ASSISTANT));
        assert_eq!(page_index("Shortcuts"), Some(PAGE_SHORTCUTS));
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.ui.agent_cursor.show = false;
        app.ui.pick_bodies = true;
        app.session.auto_project = false;
        app.session.project_construction = true;
        defaults(&mut app, PAGE_GENERAL);
        assert!(!app.ui.agent_cursor.show);
        assert!(app.ui.pick_bodies);
        assert!(!app.session.auto_project);
        assert!(app.session.project_construction);
        defaults(&mut app, PAGE_ASSISTANT);
        let expected = SolveApp::new(Default::default(), Default::default()).ui.agent_cursor.show;
        assert_eq!(app.ui.agent_cursor.show, expected);
        defaults(&mut app, PAGE_SKETCH);
        assert!(!app.ui.pick_bodies);
        assert!(app.session.auto_project);
        assert!(!app.session.project_construction, "off by default (#13142)");
    }

    #[test]
    fn preferences_structure_order_groups_and_unique_field_homes() {
        crate::i18n::install();
        assert_eq!(
            PAGES,
            [
                "General",
                "Appearance",
                "Layout",
                "Graphics",
                "Design",
                "Sketch",
                "Tools",
                "Navigation",
                "3D mouse",
                "Shortcuts",
                "Assistant",
                "Plugins"
            ]
        );
        assert_eq!(WINDOW_PAGES.len(), PAGES.len());
        let groups: Vec<_> =
            WINDOW_PAGES.iter().map(host_page).enumerate().filter_map(|(i, p)| p.group.map(|g| (i, solidcraft_ui_text::tr(g)))).collect();
        assert_eq!(groups, [(PAGE_APPEARANCE, "Look"), (PAGE_DESIGN, "Work"), (PAGE_NAVIGATION, "Input"), (PAGE_ASSISTANT, "Extend")]);
        let mut fields = std::collections::BTreeSet::new();
        for (field, page) in FIELD_PAGES {
            assert!(fields.insert(field), "duplicate home for {field}");
            assert!(page.is_none_or(|p| p < PAGES.len()), "invalid page for {field}");
        }
        let saved = serde_json::to_value(Prefs::default()).unwrap();
        assert_eq!(fields.len(), saved.as_object().unwrap().len(), "every stored field has exactly one metadata entry");
        for (i, name) in PAGES.iter().enumerate() {
            assert_eq!(solidcraft_ui_text::tr(host_page(&WINDOW_PAGES[i]).label), *name);
            assert_eq!(host_page(&WINDOW_PAGES[i]).handle, format!("prefs:page:{name}"));
        }
    }

    #[test]
    fn preferences_all_canonical_and_legacy_pages_open_and_publish_real_alias_handles() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        for (name, index) in PAGES.iter().enumerate().map(|(i, n)| (*n, i)).chain(PAGE_ALIASES.iter().copied()) {
            assert_eq!(page_index(name), Some(index));
            let reply = h.call("ui.prefs", json!({"open":name}));
            assert!(reply["error"].is_null(), "{name}: {reply}");
            assert_eq!(reply["result"]["dialog"], PAGES[index]);
            open_at(&mut h.app, index);
            h.frames(4);
            assert_eq!(h.app.prefs_window.shell.section, index);
            assert_eq!(crate::scenario::rect_of(&format!("prefs:page:{name}")), crate::scenario::rect_of(host_page(&WINDOW_PAGES[index]).handle));
            assert!(crate::scenario::rect_of(&format!("prefs:page:{name}")).is_some());
        }
        for (alias, index) in PAGE_ALIASES {
            open_at(&mut h.app, *index);
            h.frames(4);
            h.step(&json!({"click":{"handle":format!("prefs:page:{alias}")}})).unwrap();
            assert_eq!(h.app.prefs_window.shell.section, *index, "{alias} is clickable");
        }
    }

    #[test]
    fn preferences_moved_controls_have_one_drawn_home() {
        let cases = [
            ("Tool at the cursor", PAGE_TOOLS),
            ("Direct interaction", PAGE_TOOLS),
            ("Axis distances in Measure", PAGE_TOOLS),
            ("Move by click or drag", PAGE_TOOLS),
            ("Snap to centers", PAGE_TOOLS),
            ("Show moved parts' old place as a ghost", PAGE_TOOLS),
            ("Extrude to a picked target", PAGE_TOOLS),
            ("Turn the view to isometric when Extrude opens", PAGE_TOOLS),
            ("Go back to the earlier view when Extrude closes", PAGE_TOOLS),
            ("Placement handles", PAGE_TOOLS),
            ("Navigate with the 3D mouse", PAGE_MOUSE3D),
            ("Origin plane names", PAGE_DESIGN),
            ("Status bar", PAGE_LAYOUT),
            ("Color vision", PAGE_APPEARANCE),
            ("Quick measurement", PAGE_GRAPHICS),
            ("Analysis", PAGE_GRAPHICS),
            ("Section", PAGE_GRAPHICS),
            ("Live details", PAGE_GRAPHICS),
            ("Fold items", PAGE_GRAPHICS),
            ("Allow network locations in designs", PAGE_GENERAL),
            ("Print face highlight", PAGE_GENERAL),
            ("Keep print faces", PAGE_GENERAL),
        ];
        let mut found = vec![Vec::new(); cases.len()];
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        for page in 0..PAGES.len() {
            open_at(&mut h.app, page);
            h.frames(4);
            for (i, (label, _)) in cases.iter().enumerate() {
                if h.widget(label).is_some() {
                    found[i].push(page);
                }
            }
        }
        for ((label, page), actual) in cases.iter().zip(found) {
            assert_eq!(actual, [*page], "{label} must have exactly one Preferences home");
        }
    }

    #[test]
    fn save_prompt_preference_defaults_persists_and_resets_on_general() {
        let mut a = SolveApp::new(Default::default(), Default::default());
        assert!(!a.preferences.save_prompt_one_at_a_time);
        control(&mut a, &serde_json::json!({"save_prompt_one_at_a_time":true})).unwrap();
        let mut b = SolveApp::new(Default::default(), Default::default());
        b.load_prefs(&a.prefs());
        assert!(b.preferences.save_prompt_one_at_a_time);
        defaults(&mut b, PAGE_DESIGN);
        assert!(b.preferences.save_prompt_one_at_a_time);
        defaults(&mut b, PAGE_GENERAL);
        assert!(!b.preferences.save_prompt_one_at_a_time);
    }

    #[test]
    fn preferences_session_resets_follow_their_new_pages_only() {
        for page in 0..PAGES.len() {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.autosave_minutes = 9.0;
            app.ui.dark = false;
            app.prefs_window.follow.launch = Some("theme.json".into());
            app.ui.layout = crate::ui_kit::LayoutMode::Docked;
            app.ui.show_navigation_bar = false;
            app.ui.tool_cursor = true;
            app.ui.visual_style = 2;
            app.ui.show_grid = false;
            app.ui.ground_shadow = false;
            app.ui.perspective = true;
            app.session.auto_project = false;
            app.ui.pick_bodies = true;
            app.ui.agent_cursor.show = true;
            app.keymap.custom.insert("solid.extrude".into(), vec!["X".into()]);
            defaults(&mut app, page);
            assert_eq!(app.autosave_minutes, if page == PAGE_GENERAL { 5.0 } else { 9.0 });
            assert_eq!(app.ui.dark, page == PAGE_APPEARANCE);
            assert_eq!(app.prefs_window.follow.launch.is_none(), page == PAGE_APPEARANCE);
            assert_eq!(app.ui.layout == crate::ui_kit::LayoutMode::Floating, page == PAGE_LAYOUT);
            assert_eq!(app.ui.show_navigation_bar, page == PAGE_LAYOUT);
            assert_eq!(app.ui.tool_cursor, page != PAGE_TOOLS);
            assert_eq!(app.ui.visual_style, if page == PAGE_GRAPHICS { 0 } else { 2 });
            assert_eq!(app.ui.show_grid, page == PAGE_GRAPHICS);
            assert_eq!(app.ui.ground_shadow, page == PAGE_GRAPHICS);
            assert_eq!(app.ui.perspective, page != PAGE_GRAPHICS);
            assert_eq!(app.session.auto_project, page == PAGE_SKETCH);
            assert_eq!(app.ui.pick_bodies, page != PAGE_SKETCH);
            assert_eq!(app.ui.agent_cursor.show, page != PAGE_ASSISTANT);
            assert_eq!(app.keymap.custom.is_empty(), page == PAGE_SHORTCUTS);
        }
    }

    /// The Ellipse tool draws its axes with "Draw ellipses with their axes" on (the default),
    /// and the ellipse alone, as before, with it off.
    #[test]
    fn the_ellipse_tool_draws_its_axes_as_the_preference_says() {
        for (on, curves) in [(None, 3), (Some(false), 1), (Some(true), 3)] {
            let mut h = crate::scenario::Harness::new();
            if let Some(on) = on {
                set(&mut h.app, &serde_json::json!({"ellipse_axes": on})).unwrap();
            }
            assert_eq!(h.app.preferences.ellipse_axes, on.unwrap_or(true));
            h.app.run("sketch.create", serde_json::json!({"plane": "XY"})).unwrap();
            h.call("ui.view", serde_json::json!({"view": "top"}));
            h.step(&serde_json::json!({"start": "sketch.ellipse"})).unwrap();
            for at in [[0.0, 0.0], [20.0, 0.0], [0.0, 8.0]] {
                h.step(&serde_json::json!({"click": {"sketch": at}})).unwrap();
            }
            h.settle();
            let sid = h.app.session.active_sketch.unwrap();
            let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
            assert_eq!(sk.curves.len(), curves, "preference {on:?}: {:?}", sk.curves);
        }
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
        row["thread_options"] = json!({"full_length":false,"depth":"4 mm"});
        super::set(&mut app, &json!({"hole_presets":[row]})).unwrap();
        let expected = serde_json::to_value(&app.preferences.hole_presets).unwrap();
        assert_eq!(expected[0]["thread_options"]["depth"], "4 mm");
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

    fn personal_sheet_rule_row(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name, "thickness": "1.6 mm", "k_factor": "0.42", "bend_radius": "Thickness",
            "relief_width": "Thickness", "relief_depth": "Thickness * 0.5", "corner_relief": "Thickness * 4.0",
            "corner_relief_shape": "round", "hem_gap": "0.01 mm", "gap": "Thickness", "angle_display": "included"
        })
    }

    #[test]
    fn personal_sheet_rules_corrupt_rows_and_limits_do_not_discard_other_preferences() {
        use serde_json::json;
        let good = personal_sheet_rule_row("CRS 1.6");
        let mut oversized = personal_sheet_rule_row("Too long");
        oversized["thickness"] = json!("x".repeat(513));
        let mut duplicate = good.clone();
        duplicate["name"] = json!("crs 1.6");
        let p = super::lenient(&json!({"length_decimals":4,"personal_sheet_rules":[null,good,oversized,duplicate,{"name":"incomplete"}]}));
        assert_eq!(p.length_decimals, 4);
        assert_eq!(serde_json::to_value(&p.personal_sheet_rules).unwrap(), json!([personal_sheet_rule_row("CRS 1.6")]));
        let corrupt = super::lenient(&json!({"length_decimals":3,"personal_sheet_rules":{"unexpected":true}}));
        assert_eq!(corrupt.length_decimals, 3);
        assert_eq!(serde_json::to_value(corrupt.personal_sheet_rules).unwrap(), json!([]));
        let many: Vec<_> = (0..100).map(|i| personal_sheet_rule_row(&format!("Rule {i}"))).collect();
        let capped = super::lenient(&json!({"personal_sheet_rules":many}));
        assert_eq!(serde_json::to_value(capped.personal_sheet_rules).unwrap().as_array().unwrap().len(), 64);
    }

    #[test]
    fn personal_sheet_rules_survive_application_preferences_roundtrip_and_page_resets() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        super::set(&mut app, &json!({"personal_sheet_rules":[personal_sheet_rule_row("CRS 1.6")]})).unwrap();
        let expected = serde_json::to_value(&app.preferences.personal_sheet_rules).unwrap();
        assert_eq!(expected[0]["angle_display"], "included");
        for page in 0..super::PAGES.len() {
            super::reset_preferences(&mut app.preferences, page);
        }
        assert_eq!(serde_json::to_value(&app.preferences.personal_sheet_rules).unwrap(), expected);
        let saved = app.prefs();
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&saved);
        assert_eq!(serde_json::to_value(&other.preferences.personal_sheet_rules).unwrap(), expected);
        assert!(other.session.doc.sheet.rules.is_empty(), "a personal rule is not a rule of the design");
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
        super::defaults(&mut app, super::PAGE_APPEARANCE);
        assert_eq!(app.preferences.ui_font, "");
    }

    /// The web app's open tabs share their settings (#8644): a running tab loads what another
    /// tab wrote over settings of its own. Everything it had changed is replaced, a pin or a
    /// shortcut the other tab doesn't have included, so both tabs then save the same settings.
    #[test]
    fn settings_loaded_over_changed_ones_replace_them_all() {
        let new_app = || crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let mut writer = new_app();
        writer.sbox.toggle_pin("sketch.line");
        writer.ui.dark = !writer.ui.dark;
        writer.keymap.custom.insert("view.fit".into(), vec!["Shift+F".into()]);
        let mut reader = new_app();
        reader.sbox.toggle_pin("feature.extrude");
        reader.keymap.custom.insert("view.home".into(), vec!["H".into()]);
        reader.toolbar_custom.panels.insert(format!("{}/SOLID/CREATE", crate::toolbar_custom::WORKSPACE), vec!["feature.extrude".into()]);
        reader.tree.collapsed.insert("Bodies".into());
        reader.home.recent.push("part.solidcraft".into());
        reader.preferences.ui_scale = 1.5;
        reader.load_prefs(&writer.prefs());
        assert_eq!(reader.sbox.pinned, ["sketch.line"], "the other tab's pins, not this tab's");
        assert!(!reader.keymap.custom.contains_key("view.home"));
        assert_eq!(reader.ui.dark, writer.ui.dark);
        assert_eq!(reader.prefs(), writer.prefs(), "both tabs save the same settings");
    }

    /// Bodies folders (#4217): Several bodies only by default, also in preferences saved before it
    /// existed. Each value is saved and read back; `ui.prefs` refuses anything else and changes
    /// nothing, and saved preferences read such a value as the default and keep the rest. The
    /// Interface page's segments set it, Cancel puts it back and the page's Restore Defaults resets it.
    #[test]
    fn bodies_folders_is_saved_restored_and_refuses_unknown_values() {
        use super::*;
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.app.preferences.browser_bodies_folders, BodiesFolders::Several);
        assert_eq!(lenient(&json!({"theme": "light"})).browser_bodies_folders, BodiesFolders::Several, "saved before it existed");
        for (mode, id) in [(BodiesFolders::Always, "always"), (BodiesFolders::Several, "several"), (BodiesFolders::Never, "never")] {
            assert_eq!(control(&mut h.app, &json!({"browser_bodies_folders": id})).unwrap()["browser_bodies_folders"], id);
            let mut restored = SolveApp::new(Default::default(), Default::default());
            restored.load_prefs(&h.app.prefs());
            assert_eq!(restored.preferences.browser_bodies_folders, mode, "{id}");
        }
        for bad in [json!("sometimes"), json!("Never"), json!(""), json!(2), json!(null), json!(["never"])] {
            let before = h.app.preferences.clone();
            assert!(set(&mut h.app, &json!({"browser_bodies_folders": bad})).is_err(), "{bad}");
            assert_eq!(h.app.preferences, before, "{bad}: a refused value changes nothing");
            let saved = lenient(&json!({"browser_bodies_folders": bad, "theme": "light"}));
            assert_eq!((saved.browser_bodies_folders, saved.theme.as_str()), (BodiesFolders::Several, "light"), "{bad}");
        }
        h.call("ui.prefs", json!({"open": "Layout"}));
        reveal_preference(&mut h, "dialog:Bodies folders:Always");
        h.step(&json!({"click": {"handle": "dialog:Bodies folders:Always"}})).unwrap();
        assert_eq!(h.app.preferences.browser_bodies_folders, BodiesFolders::Always);
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(h.app.preferences.browser_bodies_folders, BodiesFolders::Never, "Cancel puts it back");
        defaults(&mut h.app, PAGE_LAYOUT);
        assert_eq!(h.app.preferences.browser_bodies_folders, BodiesFolders::Several, "Interface's Restore Defaults");
    }

    /// Theme: System on a Linux desktop (#7944): the freedesktop preference wins, GNOME's
    /// settings decide when it has none (its Light is `default`, so a plain theme name is
    /// light), and nothing read leaves the window toolkit to say.
    #[test]
    fn the_desktops_colour_scheme_setting_decides_dark_or_light() {
        use super::desktop_prefers_dark as d;
        assert_eq!(d(Some(1), Some("prefer-light"), Some("Adwaita")), Some(true), "freedesktop: prefer dark");
        assert_eq!(d(Some(2), Some("prefer-dark"), Some("Adwaita-dark")), Some(false), "freedesktop: prefer light");
        assert_eq!(d(Some(0), Some("prefer-dark"), None), Some(true), "no preference: GNOME's");
        assert_eq!(d(Some(0), Some("'prefer-light'"), Some("Adwaita-dark")), Some(false), "gsettings quotes its answer");
        assert_eq!(d(Some(0), Some("default"), Some("Adwaita-dark")), Some(true), "a dark GTK theme");
        assert_eq!(d(None, None, Some("Yaru:dark")), Some(true));
        assert_eq!(d(Some(0), Some("default"), Some("Adwaita")), Some(false), "GNOME's Light");
        assert_eq!(d(Some(0), None, None), Some(false), "no preference at all: light, as GTK applications");
        assert_eq!(d(Some(u32::MAX), None, None), Some(false), "an unknown value is no preference");
        assert_eq!(d(None, None, Some("  ")), None, "nothing read");
        assert_eq!(d(None, None, None), None);
    }

    /// The host's reading of the desktop comes before the window toolkit's theme, which egui
    /// reports as None in tests, and Theme: System follows it every frame (#7944).
    #[test]
    fn theme_system_follows_the_hosts_desktop_setting() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU8, Ordering};
        let desktop = Arc::new(AtomicU8::new(0));
        let reading = desktop.clone();
        let services = crate::Services {
            desktop_dark: Some(Box::new(move || match reading.load(Ordering::Relaxed) {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            })),
            ..Default::default()
        };
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), services);
        app.preferences.theme = "system".into();
        app.ui.dark = true;
        let ctx = egui::Context::default();
        super::apply(&mut app, &ctx);
        assert!(app.ui.dark, "nothing known: the colours stay");
        desktop.store(2, Ordering::Relaxed);
        super::apply(&mut app, &ctx);
        assert!(!app.ui.dark, "the desktop turned light");
        desktop.store(1, Ordering::Relaxed);
        super::apply(&mut app, &ctx);
        assert!(app.ui.dark, "and dark again");
        app.preferences.theme = "light".into();
        app.ui.dark = false;
        super::apply(&mut app, &ctx);
        assert!(!app.ui.dark, "an explicit theme does not follow the desktop");
    }

    /// Compact output for MCP agents: off by default, a checkbox on the AI page (there with the
    /// chat off too), reported to the bridged MCP server as `mcp.options`, reset by Restore Defaults.
    #[test]
    fn mcp_compact_output_is_an_ai_preference_the_server_reads() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.call("mcp.options", json!({}))["result"], json!({"compact": false}));
        h.call("ui.prefs", json!({"ai_features": false, "open": "AI"}));
        h.frames(3);
        assert!(crate::scenario::handle_at("prefs:mcp_compact").is_some(), "the checkbox is on the AI page with the chat off");
        // The page scrolls, so the setting is turned on as an agent would, not by a click.
        h.call("ui.prefs", json!({"mcp_compact": true}));
        h.frames(2);
        assert!(h.app.preferences.mcp_compact);
        assert_eq!(h.call("mcp.options", json!({}))["result"], json!({"compact": true}));
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(h.call("mcp.options", json!({}))["result"], json!({"compact": false}));
    }

    /// Theme files (#2255): a built-in one and an imported one turn on through `ui.prefs`, the
    /// next frame draws with their colours (dark following the base), an export gives the theme
    /// back complete, imported themes are saved with the preferences, Remove falls back to the
    /// base and Cancel takes an import back.
    #[test]
    fn system_theme_export_uses_os_slot_without_selecting_it() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.theme = "light".into();
        app.ui.dark = false;
        assert_eq!(theme_file(&app, "system", true)["base"], "dark");
        assert_eq!(theme_file(&app, "system", false)["base"], "light");
        app.preferences.theme_day = "akari".into();
        app.preferences.theme_night = "onyx".into();
        assert_eq!(theme_file(&app, "system", false)["name"], "Akari");
        assert_eq!(theme_file(&app, "system", true)["name"], "Onyx");
        assert_eq!(app.preferences.theme, "light");
        assert!(!app.ui.dark);
    }

    /// The System theme leaves the native window to follow the operating system (#8983). A macOS
    /// window told to be light or dark keeps that appearance and no longer reports the system
    /// appearance changing, so the app stayed light after the system turned dark. Dark and Light
    /// still give the window their own appearance, for its title bar.
    #[test]
    fn the_system_theme_leaves_the_window_to_follow_the_system() {
        let mut h = crate::scenario::Harness::new();
        // A frame with the system appearance `system`: the window themes the frame asked for.
        let frame = |h: &mut crate::scenario::Harness, system: egui::Theme| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                system_theme: Some(system),
                time: Some(h.ctx.input(|input| input.time)),
                ..Default::default()
            };
            let app = &mut h.app;
            let mut out = h.ctx.run_ui(raw, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            out.textures_delta.clear();
            let commands = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default();
            commands.into_iter().filter_map(|c| if let egui::ViewportCommand::SetTheme(t) = c { Some(t) } else { None }).collect::<Vec<_>>()
        };
        let frames = |h: &mut crate::scenario::Harness, system: egui::Theme| (0..3).flat_map(|_| frame(h, system)).collect::<Vec<_>>();
        let out = h.call("ui.prefs", json!({"theme": "system"}));
        assert_eq!(out["result"]["theme"], "system", "{out}");
        frames(&mut h, egui::Theme::Light);
        assert_eq!(h.app.prefs_window.window_theme, Some(egui::SystemTheme::SystemDefault));
        assert!(!h.ctx.options(|o| o.sync_window_theme), "egui would tell the window light or dark");
        assert!(!h.app.ui.dark);
        // The system turns dark: the app follows, and nothing is asked of the window.
        assert_eq!(frames(&mut h, egui::Theme::Dark), vec![]);
        assert!(h.app.ui.dark);
        assert_eq!(frames(&mut h, egui::Theme::Light), vec![]);
        assert!(!h.app.ui.dark);
        // An explicit theme gives the window its appearance, whatever the system's.
        let out = h.call("ui.prefs", json!({"theme": "light"}));
        assert_eq!(out["result"]["theme"], "light", "{out}");
        assert_eq!(frames(&mut h, egui::Theme::Dark), vec![]);
        assert_eq!(h.app.prefs_window.window_theme, Some(egui::SystemTheme::Light));
        assert!(!h.app.ui.dark);
    }

    /// The 3D view in the other theme's colours (#4146): `ui.prefs` sets it, the window keeps its
    /// theme and the next frame draws the view from the other side, and it is saved with the
    /// preferences. With a theme file on, the view takes the built-in Light or Dark colours.
    #[test]
    fn the_3d_view_takes_the_light_or_dark_themes_colours() {
        use serde_json::json;
        use solidcraft_theme::{ColorVision, Tokens};
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
        let hard = resolved_theme(&h.app.preferences, active_theme_file(&h.app).unwrap());
        assert_eq!((Tokens::get().panel, Tokens::get().viewport_top), (hard.panel, Tokens::light().viewport_top));

        // Asked for, an unknown value is refused; read from saved preferences, it is the default.
        assert!(h.call("ui.prefs", json!({"viewport_theme": "dim"}))["error"].is_string());
        let read: crate::theme::ViewportTheme = serde_json::from_value(json!("dim")).unwrap();
        assert_eq!(read, crate::theme::ViewportTheme::Theme);
    }

    /// The 3D view's background in one colour of the user's (#7688): `ui.prefs` sets it, the next
    /// frame fills the view with it in either theme while the window keeps its colours, a picture
    /// of the view has it too, and it is saved with the preferences. `null` gives back the theme's
    /// gradient; a value that is not a colour is refused, and read from saved preferences it is
    /// the gradient.
    #[test]
    fn the_3d_view_takes_a_background_colour_of_its_own() {
        use serde_json::json;
        use solidcraft_theme::{ColorVision, Tokens};
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.app.preferences.viewport_background, None);
        for (theme, view_dark) in [("light", false), ("dark", true)] {
            let out = h.call("ui.prefs", json!({"theme": theme, "viewport_background": [255, 255, 255]}));
            assert!(out["error"].is_null(), "{out}");
            h.frames(2);
            let (t, white) = (Tokens::get(), egui::Color32::WHITE);
            assert_eq!((t.viewport_top, t.viewport_bottom, Tokens::view().viewport_top), (white, white, white), "{theme}");
            assert_eq!(t.panel, Tokens::themed(view_dark, ColorVision::Off).panel, "{theme}: the window");
            let scene = crate::control::view_scene(&h.app, solidcraft_engine::view::Guides::default()).unwrap();
            let white = solidcraft_viewport::Rgb(255, 255, 255);
            assert_eq!(scene.background, Some((white, white)), "{theme}: a picture of the view");
        }

        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert_eq!(other.preferences.viewport_background, Some([255, 255, 255]));

        h.call("ui.prefs", json!({"viewport_background": null}));
        h.frames(2);
        let (t, dark) = (Tokens::get(), Tokens::dark());
        assert_eq!((t.viewport_top, t.viewport_bottom), (dark.viewport_top, dark.viewport_bottom), "the theme's gradient again");

        for bad in [json!([256, 0, 0]), json!("white"), json!([1, 2])] {
            assert!(h.call("ui.prefs", json!({"viewport_background": bad}))["error"].is_string(), "{bad}");
            assert_eq!(lenient(&json!({"viewport_background": bad})).viewport_background, None, "{bad}");
        }
    }

    /// #14121: the size and colour of sketch dimension text are saved preferences of their own.
    /// An out-of-range scale and a value that is not an RGB colour are refused; a saved bad value
    /// falls back to its default without discarding the other preferences.
    #[test]
    fn dimension_text_size_and_colour_preferences_validate_and_migrate() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let out = h.call("ui.prefs", json!({"dimension_text_scale":1.5,"dimension_colour":[18,52,86]}));
        assert!(out["error"].is_null(), "{out}");
        assert_eq!((h.app.preferences.dimension_text_scale, h.app.preferences.dimension_colour), (1.5, Some([18, 52, 86])));
        h.call("ui.prefs", json!({"dimension_colour":null}));
        assert_eq!(h.app.preferences.dimension_colour, None);
        for bad in [json!(0.4), json!(2.6)] {
            assert!(h.call("ui.prefs", json!({"dimension_text_scale":bad}))["error"].is_string(), "{bad}");
            assert_eq!(lenient(&json!({"dimension_text_scale":bad})).dimension_text_scale, 1.0, "{bad}");
        }
        for bad in [json!([256, 0, 0]), json!("red"), json!([1, 2])] {
            assert!(h.call("ui.prefs", json!({"dimension_colour":bad}))["error"].is_string(), "{bad}");
            assert_eq!(lenient(&json!({"dimension_colour":bad})).dimension_colour, None, "{bad}");
        }
        let read = lenient(&json!({"dimension_text_scale":9.0,"dimension_colour":[256,0,0]}));
        assert_eq!((read.dimension_text_scale, read.dimension_colour), (1.0, None), "bad values cost only their settings");
    }

    /// The theme strip's Import… reads the theme file through `vfs`, like the other imports: in the
    /// web app a picked file is kept there, where `std::fs` can't read it (#7643). On the disk it
    /// is the same file; a folder scope active while it is read refuses a path outside it, which
    /// shows the read goes through `vfs`.
    #[test]
    fn a_theme_pasted_or_dropped_on_the_interface_page_imports_and_other_pastes_pass_through() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let theme = |name: &str| format!(r##"{{"format":"solidcraft-theme/1","name":"{name}","base":"dark","colors":{{"panel":"#102030"}}}}"##);
        let has = |h: &crate::scenario::Harness, id: &str| h.app.preferences.custom_themes.iter().any(|t| t.id == id);

        // Another page: a theme's text is left to the usual paste.
        h.call("ui.prefs", json!({"open":"General"}));
        h.frames(2);
        h.frame_with(vec![egui::Event::Paste(theme("Elsewhere"))]);
        assert!(!has(&h, "elsewhere"));

        // The Interface page: plain text is not a theme and changes nothing.
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.frames(2);
        let before = h.app.preferences.clone();
        h.frame_with(vec![egui::Event::Paste("just some words".into())]);
        assert_eq!(h.app.preferences.custom_themes, before.custom_themes);

        // A theme's text imports and turns on, as Import… does with a file.
        h.frame_with(vec![egui::Event::Paste(theme("Pasted"))]);
        assert!(has(&h, "pasted"), "{:?}", h.app.preferences.custom_themes.iter().map(|t| &t.id).collect::<Vec<_>>());
        assert_eq!(h.app.preferences.theme, "pasted");

        // An invalid theme says why, and imports nothing.
        h.frame_with(vec![egui::Event::Paste(r#"{"format":"solidcraft-theme/1","name":"Bad","base":"sideways"}"#.into())]);
        assert!(!has(&h, "bad"));

        // A file dropped on the Import tile is a theme to import, not a design to open.
        h.frames(2);
        let tile = h.app.prefs_window.import_tile.expect("the Import tile is drawn on the Interface page");
        let dir = std::env::temp_dir().join(format!("solidcraft-theme-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dropped.json");
        std::fs::write(&file, theme("Dropped")).unwrap();
        #[derive(Debug)]
        struct File(std::path::PathBuf);
        impl egui::DroppedFile for File {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|e| e.to_string())
            }
        }
        let tabs = h.app.docs.count();
        h.frame_with(vec![egui::Event::PointerMoved(tile.center())]);
        h.drop_files(tile.center(), vec![std::sync::Arc::new(File(file)) as egui::DroppedFileHandle]);
        h.frames(2);
        assert!(has(&h, "dropped"));
        assert_eq!(h.app.docs.count(), tabs, "the drop opened no design");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_import_reads_through_vfs() {
        use serde_json::json;
        use solidcraft_engine::io::vfs;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"theme": "dark", "open": "Interface"}));
        h.frames(2);
        assert!(h.app.prefs_window.import_tile.is_some(), "Import is always visible");
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
                h.step(&json!({"click":{"handle":"prefs:theme_import"}})).unwrap();
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
        use egui::Color32;
        use serde_json::json;
        use solidcraft_theme::Tokens;
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
    fn appearance_sections_and_carousel_use_real_data_and_stay_in_the_page() {
        for base in ["light", "dark"] {
            for screen in [740, 1600] {
                let mut h = crate::scenario::Harness::new();
                h.app.home.open = false;
                h.call("ui.resize", json!({"width":screen,"height":1100}));
                h.call("ui.prefs", json!({"theme":base,"language":"en","open":"Appearance"}));
                h.frames(4);
                let page = crate::scenario::rect_of("prefs:page_body").unwrap();
                let mut previous: Option<f32> = None;
                for key in ["theme", "viewport_logo", "scale", "colour_vision"] {
                    let section = crate::scenario::rect_of(&format!("prefs:section:{key}")).unwrap();
                    assert!(section.left() >= page.left() - 1.0 && section.right() <= page.right() + 1.0);
                    if let Some(bottom) = previous {
                        assert!((section.top() - bottom - 38.0).abs() <= 1.0, "{base} width={screen}: {key}");
                    }
                    previous = Some(crate::scenario::rect_of(&format!("prefs:section:{key}:body")).unwrap().bottom());
                }
                let choices = theme_choices(&h.app);
                let count = format!("{} themes", choices.len());
                let mut painted_count = false;
                for shape in &h.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| {
                        if let egui::Shape::Text(text) = shape {
                            painted_count |= text.galley.job.text == count;
                        }
                    });
                }
                assert!(painted_count, "count comes from actual themes");
                while crate::scenario::rect_of("prefs:theme_previous").is_some() {
                    h.step(&json!({"click":{"handle":"prefs:theme_previous"}})).unwrap();
                    h.frames(3);
                }
                let mut found = std::collections::BTreeSet::new();
                for _ in 0..choices.len() + 1 {
                    for (id, _) in &choices {
                        if let Some(rect) = crate::scenario::rect_of(&format!("prefs:theme:{id}")) {
                            assert!(page.expand(1.0).contains_rect(rect), "{base} width={screen}: {id} {rect:?}");
                            found.insert(id.clone());
                        }
                    }
                    if crate::scenario::rect_of("prefs:theme_next").is_none() {
                        break;
                    }
                    h.step(&json!({"click":{"handle":"prefs:theme_next"}})).unwrap();
                    h.frames(3);
                }
                assert_eq!(found, choices.into_iter().map(|(id, _)| id).collect());
                assert!(h.app.prefs_window.import_tile.is_some(), "Import stays visible while scrolling");
                assert_eq!(h.app.preferences.theme, base, "scrolling does not select a theme");
                assert_eq!(h.app.preferences.ui_scale, 1.0);
                assert_eq!(h.app.preferences.viewport_background, None);
                assert_eq!(h.app.preferences.viewport_theme, crate::theme::ViewportTheme::Theme);
                assert!(!h.app.preferences.logo_accent);
                assert!(h.app.preferences.theme_follow.is_empty() && h.app.preferences.ui_font.is_empty());
                assert_eq!(h.app.preferences.color_vision, solidcraft_theme::ColorVision::Off);
                h.call("ui.prefs", json!({"button":"cancel"}));
            }
        }
    }

    /// #11445: the theme row opens on the theme in use each time Preferences or its page shows it,
    /// and the arrows still move it freely while it stays in view.
    #[test]
    fn theme_row_opens_on_the_theme_in_use_each_time_it_comes_into_view() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"theme":"dark","language":"en","open":"Appearance"}));
        h.frames(4);
        let count = theme_choices(&h.app).len();
        let page_back = |h: &mut crate::scenario::Harness| {
            for _ in 0..=count {
                if crate::scenario::rect_of("prefs:theme_previous").is_none() {
                    return;
                }
                h.step(&json!({"click":{"handle":"prefs:theme_previous"}})).unwrap();
                h.frames(3);
            }
            panic!("the previous arrow never reached the start: the row is held in place");
        };
        page_back(&mut h);
        // A theme the row does not show at its start.
        let far =
            theme_choices(&h.app).into_iter().map(|(id, _)| id).find(|id| crate::scenario::rect_of(&format!("prefs:theme:{id}")).is_none()).unwrap();
        let shows = |id: &str| {
            let viewport = crate::scenario::rect_of("prefs:theme_viewport").unwrap();
            crate::scenario::rect_of(&format!("prefs:theme:{id}")).is_some_and(|rect| viewport.expand(1.0).contains_rect(rect))
        };
        h.call("ui.prefs", json!({"theme":far}));
        h.frames(4);
        assert!(shows(&far), "a theme chosen elsewhere comes into view: {far}");
        page_back(&mut h);
        assert!(!shows(&far), "the arrows move the row away from the theme in use");
        h.call("ui.prefs", json!({"button":"ok"}));
        h.frames(3);
        h.call("ui.prefs", json!({"open":"Appearance"}));
        h.frames(4);
        assert!(shows(&far), "reopening Preferences shows the theme in use: {far}");
        page_back(&mut h);
        h.frames(4);
        assert!(!shows(&far), "the row stays where the arrows leave it");
        h.call("ui.prefs", json!({"open":"General"}));
        h.frames(3);
        h.call("ui.prefs", json!({"open":"Appearance"}));
        h.frames(4);
        assert!(shows(&far), "coming back to the page shows the theme in use: {far}");
        page_back(&mut h);
        h.app.prefs_window.shell.filter = "Onyx".to_owned();
        h.frames(5);
        h.app.prefs_window.shell.filter.clear();
        h.frames(5);
        assert!(shows(&far), "clearing a search shows the theme in use: {far}");
        assert_eq!(h.app.preferences.theme, far, "showing a theme never chooses one");
        h.call("ui.prefs", json!({"button":"cancel"}));
    }

    #[test]
    fn appearance_controls_edit_real_fields_and_cancel_restores_them() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"language":"en"}));
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.frames(3);
        assert_eq!(h.app.prefs_window.shell.section, PAGE_APPEARANCE, "legacy alias remains supported");
        let before = h.app.preferences.clone();
        for (handle, expected) in
            [("prefs:viewport_theme:light", crate::theme::ViewportTheme::Light), ("prefs:viewport_theme:dark", crate::theme::ViewportTheme::Dark)]
        {
            super::reveal_preference(&mut h, handle);
            h.step(&json!({"click":{"handle":handle}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.preferences.viewport_theme, expected);
        }
        super::reveal_preference(&mut h, "dialog:Background:Color");
        h.step(&json!({"click":{"handle":"dialog:Background:Color"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.viewport_background, Some([255, 255, 255]));
        super::reveal_preference(&mut h, "prefs:viewport_background_hex");
        h.step(&json!({"click":{"handle":"prefs:viewport_background_hex"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"#123456"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.viewport_background, Some([0x12, 0x34, 0x56]));
        super::reveal_preference(&mut h, "prefs:viewport_background_picker");
        h.step(&json!({"click":{"handle":"prefs:viewport_background_picker"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("color:popover").is_some(), "the standard picker opens beside the hex editor");
        h.step(&json!({"click":{"handle":"color:spin:R:up"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.viewport_background, Some([0x13, 0x34, 0x56]), "picker edits the same RGB as hex");
        h.step(&json!({"click":{"handle":"prefs:viewport_background_hex"}})).unwrap();
        h.frames(2);
        assert!(h.widget("#133456").is_some(), "a picker edit refreshes the inline hex field");
        super::reveal_preference(&mut h, "dialog:Logo:Accent");
        h.step(&json!({"click":{"handle":"dialog:Logo:Accent"}})).unwrap();
        h.frames(2);
        assert!(h.app.preferences.logo_accent);
        for (percent, scale) in [(75, 0.75), (100, 1.0), (110, 1.1), (125, 1.25), (150, 1.5), (200, 2.0)] {
            let handle = format!("prefs:scale_preset:{percent}");
            // Rescaling may leave the presets above the current vertical scroll position.
            let page = crate::scenario::rect_of("prefs:page_body").unwrap();
            h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":5000}));
            h.frames(20);
            super::reveal_preference(&mut h, &handle);
            h.step(&json!({"click":{"handle":handle}})).unwrap();
            h.frames(4);
            assert!((h.app.preferences.ui_scale - scale).abs() < 0.001);
        }
        h.call("ui.prefs", json!({"button":"cancel"}));
        h.frames(2);
        assert_eq!(h.app.preferences, before);
    }

    /// #14121: the Sketch page's size slider and Theme/Custom colour controls edit the real
    /// preferences, and Cancel restores the opening snapshot.
    #[test]
    fn sketch_dimension_text_controls_edit_size_and_colour() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"language":"en","theme":"dark"}));
        h.call("ui.prefs", json!({"open":"Sketch"}));
        h.frames(3);
        let original = h.app.preferences.clone();
        h.app.prefs_window.shell.filter = h.app.tr("prefs.dimension_text_size").into();
        h.frames(5);
        super::reveal_preference(&mut h, "prefs:dimension_text_scale");
        let track = crate::scenario::rect_of("dialog:slider_track:Dimension text size").expect("the dimension text size slider");
        h.step(&json!({"click":[track.center().x,track.center().y]})).unwrap();
        h.frames(2);
        assert!((h.app.preferences.dimension_text_scale - 1.5).abs() < 0.06, "{}", h.app.preferences.dimension_text_scale);
        h.app.prefs_window.shell.filter = h.app.tr("prefs.dimension_colour").into();
        h.frames(5);
        super::reveal_preference(&mut h, "prefs:dimension_colour:custom");
        assert!(crate::scenario::rect_of("prefs:dimension_colour_picker").is_some(), "the colour controls are shown");
        h.step(&json!({"click":{"handle":"prefs:dimension_colour:custom"}})).unwrap();
        h.frames(2);
        let theme = Tokens::get().dimension.to_array();
        assert_eq!(h.app.preferences.dimension_colour, Some([theme[0], theme[1], theme[2]]), "Custom starts at the theme's colour");
        super::reveal_preference(&mut h, "prefs:dimension_colour_hex");
        h.step(&json!({"click":{"handle":"prefs:dimension_colour_hex"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"#123456"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.dimension_colour, Some([0x12, 0x34, 0x56]));
        h.step(&json!({"click":{"handle":"prefs:dimension_colour:theme"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.dimension_colour, None);
        h.call("ui.prefs", json!({"button":"cancel"}));
        h.frames(2);
        assert_eq!(h.app.preferences, original);
    }

    fn theme_options_fixture(base: &str, count: usize) -> serde_json::Value {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/theme-options.json")).unwrap();
        fixtures[format!("{base}-{count}")].clone()
    }

    #[test]
    fn theme_options_ui_counts_badges_more_and_reset_in_light_and_dark() {
        for base in ["light", "dark"] {
            for count in [0, 1, 3, 6, 12] {
                let mut h = crate::scenario::Harness::new();
                h.app.home.open = false;
                h.call("ui.resize", json!({"width":1600,"height":1100}));
                let fixture = theme_options_fixture(base, count);
                // The 12-cell fixture also declares a composite child, counted in the header.
                let option_count = fixture["options"].as_array().expect("fixture option definitions").len();
                let out = h.call("ui.prefs", json!({"theme_import":fixture,"open":"Interface"}));
                assert!(out["error"].is_null(), "{out}");
                h.frames(3);
                assert_eq!(crate::scenario::rect_of("prefs:theme_options_header").is_some(), count > 0, "{base} {count}");
                let caption = if option_count == 1 { "1 option".to_owned() } else { format!("{option_count} options") };
                let mut painted_count = 0;
                for shape in &h.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| {
                        if let egui::Shape::Text(text) = shape {
                            painted_count += usize::from(text.galley.job.text == caption);
                        }
                    });
                }
                assert_eq!(painted_count, usize::from(count > 0), "{base}: the real option count paints the exact singular/plural caption once");
                let id = format!("options-{base}-{count}");
                assert_eq!(crate::scenario::rect_of(&format!("prefs:theme_options_badge:{id}")).is_some(), count > 0);
                assert_eq!(crate::scenario::handle_at("prefs:theme_options_more").is_some(), count == 12);
                let cells: std::collections::BTreeSet<_> = crate::scenario::rects("prefs:theme_option_cell:").into_iter().map(|(id, _)| id).collect();
                assert_eq!(cells.len(), count.min(10), "only ten top-level cells before More");
                assert_eq!(
                    crate::scenario::rect_of("dialog:hint:theme_option_section").is_some(),
                    count > 6,
                    "dim section labels only above six entries"
                );
                assert!(crate::scenario::handle_at("prefs:theme_options_reset").is_none());
                if count == 0 {
                    let viewport = crate::scenario::rect_of("prefs:section:viewport_logo").unwrap();
                    let mut unknown = theme_options_fixture(base, count);
                    unknown["options"] = json!([]);
                    h.call("ui.prefs", json!({"theme_import":unknown}));
                    h.frames(2);
                    assert!(crate::scenario::rect_of("prefs:theme_options_header").is_none());
                    assert!(crate::scenario::rect_of(&format!("prefs:theme_options_badge:{id}")).is_none());
                    assert_eq!(crate::scenario::rect_of("prefs:section:viewport_logo").unwrap(), viewport, "empty declarations add no gap or notice");
                }
                if count == 12 {
                    assert!(crate::scenario::rect_of("prefs:theme_option:outline").is_none());
                    assert!(crate::scenario::rect_of("prefs:theme_option:extra").is_none(), "advanced is always under More");
                    super::reveal_preference(&mut h, "prefs:theme_options_more");
                    h.step(&json!({"click":{"handle":"prefs:theme_options_more"}})).unwrap();
                    h.frames(2);
                    assert!(crate::scenario::rect_of("prefs:theme_option:outline").is_some());
                    assert!(crate::scenario::rect_of("prefs:theme_option:extra").is_some());
                }
                if count >= 3 {
                    super::reveal_preference(&mut h, "prefs:theme_option_switch:Warm accent");
                    h.step(&json!({"click":{"handle":"prefs:theme_option_switch:Warm accent"}})).unwrap();
                    h.frames(2);
                    assert_eq!(h.app.preferences.theme_options[&id]["warm"], true, "UI edits go through preference merging");
                    assert!(crate::scenario::handle_at("prefs:theme_options_reset").is_some());
                    super::reveal_preference(&mut h, "prefs:theme_options_reset");
                    h.step(&json!({"click":{"handle":"prefs:theme_options_reset"}})).unwrap();
                    h.frames(2);
                    assert!(!h.app.preferences.theme_options.contains_key(&id));
                    assert!(crate::scenario::handle_at("prefs:theme_options_reset").is_none());
                }
            }
        }
    }

    #[test]
    fn theme_options_ui_short_composite_shares_row_with_narrow_control() {
        for base in ["light", "dark"] {
            for language in ["en", "de"] {
                for width in [1600, 520] {
                    let mut h = crate::scenario::Harness::new();
                    h.app.home.open = false;
                    h.call("ui.resize", json!({"width":width,"height":1100}));
                    let file = json!({"format":"solidcraft-theme/1","name":"Compact options","base":base,"options":[
                        {"id":"shadow","type":"bool","default":true,"label":{"en":"Shadow","de":"Schatten"}},
                        {"id":"softness","type":"range","default":5.5,"min":0,"max":10,"step":0.5,"unit":"px","parent":"shadow",
                            "label":{"en":"Softness","de":"Weichheit"}},
                        {"id":"edges","type":"range","default":5.5,"min":0,"max":10,"step":0.5,"unit":"px",
                            "label":{"en":"Edges","de":"Kanten"}}
                    ]});
                    let out = h.call("ui.prefs", json!({"theme_import":file,"open":"Interface","language":language}));
                    assert!(out["error"].is_null(), "{out}");
                    h.frames(3);
                    let rect = |name: &str| crate::scenario::rect_of(name).unwrap();
                    let parent = rect("prefs:theme_option:shadow");
                    let child = rect("prefs:theme_option:softness");
                    let composite = rect("prefs:theme_option_cell:shadow");
                    let neighbour = rect("prefs:theme_option_cell:edges");
                    let body = rect("prefs:page_body").expand(1.0);
                    assert!(body.contains_rect(composite) && body.contains_rect(neighbour), "{base} {language} width={width}");
                    assert!(composite.contains_rect(parent) && composite.contains_rect(child));
                    assert!(!composite.intersects(neighbour));
                    if width == 1600 {
                        assert!(
                            (composite.top() - neighbour.top()).abs() < 1.0,
                            "short composite must share a normal Preferences row: {composite:?} {neighbour:?}"
                        );
                        assert!(composite.right() < neighbour.left(), "declared left-to-right order");
                        assert!((parent.top() - child.top()).abs() < 1.0);
                        assert!(parent.right() < child.left(), "parent and child share one compact cell");
                    } else {
                        assert!(neighbour.top() > composite.bottom(), "a genuinely narrow page falls back to a full composite row");
                    }
                    let labels = if language == "de" { ["Schatten", "Weichheit", "Kanten"] } else { ["Shadow", "Softness", "Edges"] };
                    let parts = [
                        rect(&format!("dialog:label:{}", labels[0])),
                        rect(&format!("prefs:theme_option_switch:{}", labels[0])),
                        rect(&format!("dialog:label:{}", labels[1])),
                        rect(&format!("dialog:slider:{}", labels[1])),
                        rect("prefs:theme_option_readout:softness"),
                        rect(&format!("dialog:label:{}", labels[2])),
                        rect(&format!("dialog:slider:{}", labels[2])),
                        rect("prefs:theme_option_readout:edges"),
                    ];
                    for (i, part) in parts.iter().enumerate() {
                        assert!(body.contains_rect(*part), "{base} {language} width={width}: {part:?} outside {body:?}");
                        for other in parts.iter().skip(i + 1) {
                            assert!(!part.intersects(*other), "{base} {language} width={width}: {part:?} overlaps {other:?}");
                        }
                    }
                    assert!(rect(&format!("dialog:slider:{}", labels[1])).width() >= Tokens::get().space_24 * 2.0, "retain a usable slider track");
                    let control = rect(&format!("dialog:control:{}", labels[2]));
                    let track = rect(&format!("dialog:slider_track:{}", labels[2]));
                    let readout = rect("prefs:theme_option_readout:edges");
                    let t = Tokens::get();
                    let natural_width = |text: &str| {
                        h.ctx.fonts_mut(|fonts| {
                            fonts.layout_no_wrap(text.to_owned(), egui::FontId::proportional(t.font_label), t.form_ink).size().x.ceil()
                        })
                    };
                    assert!((rect(&format!("dialog:label:{}", labels[2])).width() - natural_width(labels[2]) - t.space_4).abs() <= 1.0);
                    assert!((readout.width() - natural_width("5.5px")).abs() <= 1.0, "readout must retain its natural width");
                    let reserved_readout = ["0px", "10px", "5.5px", "0.5px", "9.5px"].into_iter().map(natural_width).fold(0.0, f32::max);
                    assert!(neighbour.expand(1.0).contains_rect(control));
                    assert!(control.contains_rect(track) && control.contains_rect(readout));
                    assert!(
                        (track.width() - (control.width() - reserved_readout - t.space_8 - t.space_4)).abs() <= 1.0,
                        "standalone slider must fill the width left by its natural label and readout: {base} {language} width={width}: {control:?} {track:?} {readout:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn theme_options_ui_wide_composite_dropdown_and_advanced_keep_order() {
        for base in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1100}));
            h.call("ui.prefs", json!({"theme_import":theme_options_fixture(base, 12),"open":"Interface"}));
            h.frames(3);
            let rect = |id: &str| crate::scenario::rect_of(&format!("prefs:theme_option:{id}")).unwrap();
            assert!(rect("opacity").center().x < rect("warm").center().x);
            assert!((rect("opacity").top() - rect("warm").top()).abs() < 1.0);
            assert!(rect("palette").top() > rect("warm").top());
            assert!(rect("radius").top() > rect("palette").top(), "four choices occupy a full row");
            let composite = crate::scenario::rect_of("prefs:theme_option_cell:shadow").unwrap();
            assert!(composite.contains_rect(rect("shadow")) && composite.contains_rect(rect("softness")));
            assert!(crate::scenario::rect_of("prefs:theme_option_cell:softness").is_none(), "child is not an eleventh cell");
            super::reveal_preference(&mut h, "dialog:Palette:Warm");
            h.step(&json!({"click":{"handle":"dialog:Palette:Warm"}})).unwrap();
            h.frames(2);
            let id = format!("options-{base}-12");
            assert_eq!(h.app.preferences.theme_options[&id]["palette"], "v2");
            super::reveal_preference(&mut h, "prefs:theme_option_switch:Shadow");
            h.step(&json!({"click":{"handle":"prefs:theme_option_switch:Shadow"}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.preferences.theme_options[&id]["shadow"], false);
            let saved = h.app.preferences.theme_options[&id].clone();
            let track = crate::scenario::rect_of("dialog:slider_track:Softness").unwrap();
            h.step(&json!({"click":[track.center().x,track.center().y]})).unwrap();
            h.frames(2);
            assert_eq!(h.app.preferences.theme_options[&id], saved, "inactive child is inert and retains its value");
            super::reveal_preference(&mut h, "prefs:theme_option:pattern");
            h.step(&json!({"click":{"handle":"dialog:Pattern"}})).unwrap();
            h.frames(2);
            assert!(crate::scenario::handle_at("dialog:Pattern:Plain").is_some(), "six choices use a dropdown");
            h.step(&json!({"click":{"handle":"dialog:Pattern:Plain"}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.preferences.theme_options[&id]["pattern"], "v6");
            assert!(crate::scenario::rect_of("prefs:theme_option:extra").is_none());
            super::reveal_preference(&mut h, "prefs:theme_options_more");
            h.step(&json!({"click":{"handle":"prefs:theme_options_more"}})).unwrap();
            h.frames(2);
            assert!(rect("extra").top() > rect("compact").top());
            assert!((rect("extra").top() - rect("outline").top()).abs() < 1.0);
        }
    }

    #[test]
    fn theme_options_ui_opacity_live_reset_and_cancel_use_the_existing_path() {
        for base in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            control(&mut h.app, &json!({"theme_import":theme_options_fixture(base, 1)})).unwrap();
            h.call("ui.prefs", json!({"open":"Interface"}));
            h.frames(3);
            let full = Tokens::get();
            let file = active_theme_file(&h.app).unwrap();
            let mut invalid_saved = h.app.preferences.clone();
            invalid_saved.theme_options.entry(file.id.clone()).or_default().insert("opacity".into(), json!(20));
            assert_eq!(resolved_theme(&invalid_saved, file), full, "saved preference fallback stays at the theme default");
            assert_eq!(invalid_saved.theme_options[&file.id]["opacity"], 20, "fallback must retain the raw saved value");
            super::reveal_preference(&mut h, "dialog:slider:Opacity");
            let slider = crate::scenario::rect_of("dialog:slider_track:Opacity").unwrap();
            h.step(&json!({"click":[slider.center().x,slider.center().y]})).unwrap();
            h.frames(2);
            let id = format!("options-{base}-1");
            let value = h.app.preferences.theme_options[&id]["opacity"].as_f64().unwrap();
            assert!((40.0..100.0).contains(&value));
            assert!(Tokens::get().panel.a() < full.panel.a());
            assert_eq!(Tokens::get().text, full.text);
            assert!(h.app.prefs_window.shell.open, "editing must keep the same Preferences window");
            h.step(&json!({"click":{"handle":"prefs:theme_options_reset"}})).unwrap();
            h.frames(2);
            assert_eq!(Tokens::get(), full);
            h.call("ui.prefs", json!({"theme_options":{(id.clone()):{"opacity":60}}}));
            h.frames(2);
            assert!(Tokens::get().panel.a() < full.panel.a());
            h.call("ui.prefs", json!({"button":"cancel"}));
            h.frames(2);
            assert_eq!(Tokens::get(), full);
            assert!(!h.app.preferences.theme_options.contains_key(&id));
        }
    }

    #[test]
    fn theme_options_provider_registry_filters_unknown_and_retains_unavailable() {
        crate::i18n::install();
        use crate::theme_options::{Availability, Provider};
        let file = crate::theme_files::from_value(&json!({"format":"solidcraft-theme/1","name":"Providers","base":"dark","options":[
            {"id":"blur","type":"range","default":0},
            {"id":"tab_colours","type":"bool","default":true},
            {"id":"tab_fade_ms","type":"range","default":100},
            {"id":"dependent","type":"bool","default":true,"parent":"tab_colours"},
            {"id":"custom","type":"bool","default":false,"label":{"en":"Custom","de":"Eigene"}},
            {"id":"opacity","type":"range","default":100}
        ]}))
        .unwrap_err();
        assert!(file.contains("more than one child"));
        let file = crate::theme_files::from_value(&json!({"format":"solidcraft-theme/1","name":"Providers","base":"dark","options":[
            {"id":"blur","type":"range","default":0}, {"id":"tab_colours","type":"bool","default":true},
            {"id":"tab_fade_ms","type":"range","default":100},
            {"id":"custom","type":"bool","default":false,"label":{"en":"Custom","de":"Eigene"}},
            {"id":"opacity","type":"range","default":100}
        ]}))
        .unwrap();
        let rows = theme_option_rows(&file, "de", crate::theme_options::PROVIDERS, standard_option_text, Availability::Available);
        assert_eq!(rows.iter().map(|r| r.definition.id.as_str()).collect::<Vec<_>>(), ["opacity", "blur", "tab_colours", "tab_fade_ms", "custom"]);
        assert_eq!(rows[0].label, "Deckkraft");
        assert_eq!(rows[4].label, "Eigene");
        assert_eq!(
            theme_option_rows(&file, "ja", crate::theme_options::PROVIDERS, standard_option_text, Availability::Available)[4].label,
            "Custom",
            "English fallback"
        );
        let providers = [Provider { id: "blur", availability: || Availability::Unavailable("prefs.theme_options_unavailable"), apply: |_, _| {} }];
        // The test provider is named with the opacity text.
        let opacity_text = |_: &str| Some(("prefs.theme_option_opacity", "prefs.theme_option_opacity_help"));
        let rows = theme_option_rows(&file, "en", &providers, opacity_text, Availability::Available);
        assert_eq!(rows.iter().map(|r| r.definition.id.as_str()).collect::<Vec<_>>(), ["blur", "custom"]);
        assert_eq!(rows[0].reason.as_deref(), Some("Unavailable on this device"));
        // The internal fixture goes through the real kit. No fake provider is shipped.
        for dark in [false, true] {
            crate::theme::set_file_palette(None);
            solidcraft_theme::set_dark(dark);
            let mut values = crate::theme_files::effective_values(&file, &BTreeMap::new()).0;
            let before = values.clone();
            let ctx = crate::fonts::test_context();
            let mut draw = |events| {
                crate::scenario::clear_handles();
                crate::test_support::frame(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        ui_kit::scope(ui, |ui| {
                            ui_kit::theme_options_list(ui, &file.id, &file.name, &rows, &mut values, &super::theme_options_text());
                        });
                    },
                );
            };
            draw(vec![]);
            assert!(crate::scenario::rect_of("prefs:theme_option_reason:blur").is_some());
            let at = crate::scenario::handle_at("dialog:slider:Opacity").unwrap();
            draw(crate::test_support::pointer(at, true));
            draw(crate::test_support::pointer(at, false));
            assert_eq!(values, before, "unavailable controls stay visible and inert");
        }
    }

    #[test]
    fn blur_slider_reserves_readout_meter_and_disabled_reason() {
        crate::test_support::host_data();
        crate::i18n::install();
        use crate::theme_files::OptionValue;
        use crate::theme_options::Availability;
        let source: serde_json::Value = serde_json::from_str(include_str!("../tests/fixtures/theme-blur.json")).unwrap();
        let file = crate::theme_files::from_value(&source).unwrap();
        for dark in [false, true] {
            crate::theme::set_file_palette(None);
            solidcraft_theme::set_dark(dark);
            for available in [Availability::Available, Availability::Unavailable("prefs.theme_options_unavailable")] {
                let rows = theme_option_rows(&file, "en", crate::theme_options::PROVIDERS, standard_option_text, available);
                for radius in [0.0, 1.0, 8.0, 9.0, 18.0, 33.0, 40.0, 18.25] {
                    let ctx = crate::fonts::test_context();
                    let mut values = crate::theme_files::effective_values(&file, &BTreeMap::new()).0;
                    values.insert("blur".into(), OptionValue::Num(radius));
                    let before = values.clone();
                    let mut output = egui::FullOutput::default();
                    for _ in 0..2 {
                        crate::scenario::clear_handles();
                        output = crate::test_support::frame(
                            &ctx,
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
                                ..Default::default()
                            },
                            |ui| {
                                ui_kit::scope(ui, |ui| {
                                    ui_kit::theme_options_list(ui, &file.id, &file.name, &rows, &mut values, &super::theme_options_text());
                                })
                            },
                        );
                    }
                    assert_eq!(values, before, "drawing must preserve fractional intent");
                    let meter = crate::scenario::rect_of("prefs:blur_cost").unwrap();
                    let readout = crate::scenario::rect_of("prefs:theme_option_readout:blur").unwrap();
                    let slider = crate::scenario::rect_of("dialog:slider_track:Blur behind panels").unwrap();
                    assert!(slider.width() > 300.0, "standalone blur fills remaining row width");
                    assert!(slider.right() <= readout.left() && readout.right() <= meter.left());
                    assert_eq!(crate::scenario::rect_of("prefs:theme_option_reason:blur").is_some(), available != Availability::Available);
                    let mut heights = Vec::new();
                    for clipped in &output.shapes {
                        crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                            if let egui::Shape::Rect(rect) = shape
                                && meter.contains_rect(rect.rect)
                            {
                                heights.push(rect.rect.height());
                            }
                        });
                    }
                    assert_eq!(heights, if radius == 0.0 { vec![2.0] } else { vec![4.0, 6.0, 8.0, 11.0, 14.0] });
                }
            }
        }
    }

    #[test]
    fn blur_availability_preserves_saved_intent_and_parent_state() {
        use crate::theme_options::{Availability, Provider};
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let source = json!({"format":"solidcraft-theme/1","name":"Blur parent","base":"dark","options":[
            {"id":"enabled","type":"bool","default":true}, {"id":"blur","type":"range","default":0,"parent":"enabled"},
            {"id":"amount","type":"range","default":5,"min":0,"max":10,"step":1,"parent":"enabled"}
        ]});
        control(&mut h.app, &json!({"theme_import":source,"theme_options":{"blur-parent":{"blur":18.25,"amount":7.5}}})).unwrap();
        let file = active_theme_file(&h.app).unwrap().clone();
        assert_eq!(file.options.iter().find(|o| o.id == "blur").unwrap().parent, None, "reserved blur ignores authored parenting");
        assert_eq!(file.options.iter().find(|o| o.id == "amount").unwrap().parent.as_deref(), Some("enabled"));
        let saved = h.app.preferences.theme_options.clone();
        let values = effective_theme_options(&h.app.preferences, &file).0;
        for reason in ["prefs.blur_software", "prefs.blur_web", "prefs.blur_reduce_transparency"] {
            assert_eq!(crate::theme_options::blur_radius(&file, &values, Availability::Unavailable(reason)), 0.0);
            assert_eq!(crate::theme_options::blur_radius(&file, &values, Availability::Available), 18.25);
        }
        assert_eq!(h.app.preferences.theme_options, saved);
        let providers = [
            Provider { id: "enabled", availability: || Availability::Unavailable("prefs.blur_software"), apply: |_, _| {} },
            Provider { id: "blur", availability: || Availability::Available, apply: |_, _| {} },
        ];
        assert_eq!(
            crate::theme_options::availability(&file, "amount", &providers, Availability::Available),
            Availability::Unavailable("prefs.blur_software")
        );
        assert_eq!(
            crate::theme_options::availability(&file, "blur", &providers, Availability::Available),
            Availability::Available,
            "standard blur remains independent of the unavailable custom parent"
        );
        let rows = theme_option_rows(&file, "en", &providers, standard_option_text, Availability::Available);
        let parent = rows.iter().find(|r| r.definition.id == "enabled").unwrap();
        let child = rows.iter().find(|r| r.definition.id == "amount").unwrap();
        let blur = rows.iter().find(|r| r.definition.id == "blur").unwrap();
        assert!(parent.reason.is_some());
        assert_eq!(child.reason, parent.reason, "the genuine custom child inherits its parent's unavailable reason");
        assert!(blur.reason.is_none(), "the reserved blur control is unaffected by custom-parent availability");
        let reduced = std::rc::Rc::new(std::cell::Cell::new(true));
        let reading = reduced.clone();
        h.app.services.reduce_transparency = Some(Box::new(move |_| Some(reading.get())));
        assert_eq!(blur_availability(&h.app), Availability::Unavailable("prefs.blur_reduce_transparency"));
        let reply = control(&mut h.app, &json!({})).unwrap();
        assert_eq!(reply["theme_option_values"]["blur"], 18.25);
        h.frames(2);
        assert_eq!(h.app.blur.radius(), 18.25, "OS unavailability preserves requested intent");
        assert_eq!(h.app.preferences.theme_options, saved);
        assert_eq!(h.app.blur.counters().textures_created, 0);
        assert_eq!(h.app.blur.counters().markers_emitted, 0);
        reduced.set(false);
        assert_ne!(blur_availability(&h.app), Availability::Unavailable("prefs.blur_reduce_transparency"));
        h.frames(2);
        assert_eq!(h.app.blur.radius(), 18.25, "renderer retains raw intent even without a GPU");
        assert_eq!(h.app.blur.counters().textures_created, 0);
        control(&mut h.app, &json!({"theme_options":{"blur-parent":{"enabled":false}}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.blur.radius(), 18.25, "an inactive custom parent cannot turn independent standard blur off");
        assert_eq!(crate::theme::blur_intent(&h.ctx).radius, 18.25);
        let reply = control(&mut h.app, &json!({})).unwrap();
        assert_eq!(reply["theme_option_values"]["enabled"], false);
        assert_eq!(reply["theme_option_values"]["amount"], 7.5, "an inactive custom child retains its saved value");
        assert_eq!(h.app.preferences.theme_options["blur-parent"]["blur"], 18.25);
        control(&mut h.app, &json!({"theme_options":{"blur-parent":{"enabled":true}}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.blur.radius(), 18.25);
        assert_eq!(h.app.preferences.theme_options["blur-parent"]["amount"], 7.5);
        let counts = h.app.blur.counters();
        assert_eq!((counts.textures_created, counts.markers_emitted, counts.markers_drawn, counts.live_bytes), (0, 0, 0, 0));
    }

    #[test]
    fn blur_notices_dismiss_by_problem_identity_and_preserve_raw_json() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.prefs", json!({"theme_import":option_theme_file("Notice blur"),"open":"Interface"}));
        h.app.preferences.theme_options.insert("notice-blur".into(), BTreeMap::from([("blur".into(), json!({"future":18}))]));
        h.frames(3);
        assert!(crate::scenario::rect_of("prefs:theme_notice:blur").is_some());
        h.step(&json!({"click":{"handle":"prefs:theme_notice_dismiss:blur"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:theme_notice:blur").is_none());
        assert_eq!(h.app.preferences.theme_options["notice-blur"]["blur"], json!({"future":18}));
        let reply = control(&mut h.app, &json!({})).unwrap();
        assert_eq!(reply["theme_option_notices"], json!([{"id":"blur","kind":"wrong_type"}]));
        h.app.preferences.theme_options.get_mut("notice-blur").unwrap().insert("blur".into(), json!({"future":19}));
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:theme_notice:blur").is_some(), "changed raw value must reappear");
        h.call(
            "ui.prefs",
            json!({"theme_import":{"format":"solidcraft-theme/1","name":"Future blur","base":"dark","requires":crate::theme_files::REQUIREMENTS_LEVEL + 1,
            "options":[{"id":"blur","type":"range","default":18}]}}),
        );
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:theme_notice:newer").is_some());
        assert!(crate::scenario::rect_of("prefs:theme_options_header").is_none());
        h.step(&json!({"click":{"handle":"prefs:theme_notice_dismiss:newer"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:theme_notice:newer").is_none());
        assert!(active_theme_file(&h.app).unwrap().options.is_empty(), "unsupported tables were discarded, never auto-restored");
        h.app.preferences.custom_themes.iter_mut().find(|f| f.id == "future-blur").unwrap().requires = crate::theme_files::REQUIREMENTS_LEVEL + 2;
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:theme_notice:newer").is_some());
        for identity in 0..80 {
            h.app.blur_session.dismiss(identity);
        }
        assert!(!h.app.blur_session.dismissed(0), "dismissal storage is bounded");
        assert!(h.app.blur_session.dismissed(79));
    }

    fn option_theme_file(name: &str) -> serde_json::Value {
        json!({
            "format": "solidcraft-theme/1", "name": name, "base": "dark", "colors": {"panel": "#112233"},
            "options": [
                {"id":"enabled", "type":"bool", "default":false, "label":{"en":"Enabled", "de":"Aktiv"}, "help":{"en":"Use the overlay", "de":"Farben verwenden"}},
                {"id":"amount", "type":"range", "default":5, "min":0, "max":10, "step":1, "unit":"px", "advanced":true, "section":"Details"},
                {"id":"style", "type":"choice", "default":"warm", "parent":"enabled", "values":[{"id":"warm", "label":{"en":"Warm", "de":"Warmton"}}, {"id":"cool", "label":{"en":"Cool"}}]},
                {"id":"blur", "type":"range", "default":0}
            ],
            "overlays": [{"when":{"opt":"enabled", "eq":true}, "colors":{"panel":"#abcdef"}}]
        })
    }

    #[test]
    fn sketch_checkbox_bindings_edit_only_their_own_fields() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", json!({"language":"en","theme":theme}));
            h.call("ui.prefs", json!({"open":"Sketch"}));
            let original = h.app.preferences.clone();
            let session = (h.app.session.auto_project, h.app.ui.pick_bodies);
            let mut expected = serde_json::to_value(&original).unwrap();
            for (field, label) in [
                ("sketch_tracking_points", "prefs.sketch.tracking_points"),
                ("sketch_tracking_guides", "prefs.sketch.tracking_guides"),
                ("sketch_recent_snap", "prefs.sketch.recent_snap"),
                ("sketch_align_points", "prefs.sketch.align_points"),
                ("sketch_snap_to_visible_grid", "prefs.sketch.snap_to_visible_grid"),
                ("line_drag_draws_one", "prefs.line_drag_draws_one"),
                ("line_hover_tangent_arc", "prefs.line_hover_tangent_arc"),
                ("center_arc_follows_cursor", "prefs.sketch.center_arc"),
                ("sketch_construction_lines", "prefs.sketch_construction_lines"),
                ("sketch_auto_dimensions", "prefs.sketch.auto_dimensions"),
                ("sketch_auto_locate", "prefs.sketch.auto_locate"),
                ("shortcut_cycles_modes", "prefs.sketch.shortcut_cycles_modes"),
                ("ribbon_click_ends_tool", "prefs.ribbon_click_ends_tool"),
                ("project_options_dialog", "prefs.sketch.project_options"),
                ("sketch_cursor_coordinates", "prefs.sketch.cursor_coordinates"),
                ("sketch_face_activates_part", "prefs.sketch_face_activates_part"),
                ("sketch_open_look_at", "prefs.sketch_open_look_at"),
                ("escape_twice_finishes_sketch", "prefs.sketch.escape_twice_finishes"),
                ("selection_shift_add_only", "prefs.shift_add_ctrl_toggle"),
                ("prefer_free_sketch_points", "prefs.sketch.prefer_free_points"),
                ("select_3d_curve_points", "prefs.sketch.select_3d_curve_points"),
                ("select_other_hidden", "prefs.sketch.select_other_hidden"),
                ("manipulator_free_alt", "prefs.alt_frees_manipulators"),
            ] {
                h.app.prefs_window.shell.filter = h.app.tr(label).into();
                h.frames(5);
                let handle = format!("prefs:{field}");
                super::reveal_preference(&mut h, &handle);
                let legacy = match field {
                    "sketch_auto_dimensions" => Some("Dimension new shapes"),
                    "sketch_auto_locate" => Some("Also place them from the origin (fully constrained)"),
                    "select_other_hidden" => Some("Select hidden geometry with Select Other"),
                    _ => None,
                };
                if let Some(legacy) = legacy {
                    assert_eq!(crate::scenario::rect_of(&format!("dialog:{legacy}")), crate::scenario::rect_of(&handle));
                }
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                expected[field] = json!(!expected[field].as_bool().unwrap());
                assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected, "{theme}: {field}");
                assert_eq!((h.app.session.auto_project, h.app.ui.pick_bodies), session, "{field} leaves session values alone");
            }
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences, original);
        }
    }

    #[test]
    fn sketch_cards_tiles_and_session_controls_preserve_apply_defaults_and_cancel() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", json!({"language":"en","theme":theme,"quick_measure":true}));
            // Opening the page must retain a non-default session, too.
            h.app.session.auto_project = false;
            h.app.ui.pick_bodies = true;
            let original = Snapshot::take(&h.app);
            h.call("ui.prefs", json!({"open":"Sketch"}));
            assert!(original.matches(&h.app));
            let mut expected = serde_json::to_value(&h.app.preferences).unwrap();
            for (key, choice) in
                [("previous", SketchExitView::Previous), ("keep", SketchExitView::Keep), ("unless_rotated", SketchExitView::UnlessRotated)]
            {
                h.app.prefs_window.shell.filter = h.app.tr("prefs.sketch_exit_view").into();
                h.frames(5);
                let handle = format!("prefs:sketch_exit_view:{key}");
                super::reveal_preference(&mut h, &handle);
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                expected["sketch_exit_view"] = serde_json::to_value(choice).unwrap();
                assert_eq!(h.app.preferences.sketch_exit_view, choice);
                assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected, "only the exit enum changes");
            }
            for (key, label) in [
                ("sketch_state_icons", "prefs.sketch.state_icons"),
                ("sketch_conflict_colour", "prefs.sketch.conflict_colour"),
                ("hide_constraint_glyphs_zoomed_out", "prefs.sketch.hide_glyphs"),
            ] {
                h.app.prefs_window.shell.filter = h.app.tr(label).into();
                h.frames(5);
                let handle = format!("prefs:{key}");
                super::reveal_preference(&mut h, &handle);
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                expected[key] = json!(false);
                assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected, "{key} changes only its field");
            }
            for (key, bodies) in [("faces_edges", false), ("bodies", true), ("faces_edges", false)] {
                h.app.prefs_window.shell.filter = h.app.tr("prefs.sketch.click_selects").into();
                h.frames(5);
                let handle = format!("prefs:pick_bodies:{key}");
                super::reveal_preference(&mut h, &handle);
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                assert_eq!(h.app.ui.pick_bodies, bodies);
                assert!(!h.app.session.auto_project);
                assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected);
            }
            h.app.prefs_window.shell.filter = h.app.tr("ui.project_face_edges_sketch").into();
            h.frames(5);
            super::reveal_preference(&mut h, "prefs:auto_project");
            h.step(&json!({"click":{"handle":"prefs:auto_project"}})).unwrap();
            assert!(h.app.session.auto_project);
            assert!(!h.app.ui.pick_bodies);
            assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), expected);
            assert_eq!(saved_prefs(&h.app)["preferences"], serde_json::to_value(&original.prefs).unwrap(), "unapplied tile choices are held back");
            h.call("ui.prefs", json!({"button":"apply"}));
            let applied = Snapshot::take(&h.app);
            let mut reloaded = SolveApp::new(Default::default(), Default::default());
            reloaded.load_prefs(&h.app.prefs());
            assert_eq!(reloaded.preferences, h.app.preferences, "card and tile values survive the existing saved representation");

            h.call("ui.prefs", json!({"button":"defaults"}));
            let reset = serde_json::to_value(&h.app.preferences).unwrap();
            let defaults = serde_json::to_value(Prefs::default()).unwrap();
            for (field, owner) in FIELD_PAGES {
                assert_eq!(
                    &reset[*field],
                    if *owner == Some(PAGE_SKETCH) { &defaults[*field] } else { &expected[*field] },
                    "{field}: only Sketch resets"
                );
            }
            assert_eq!((h.app.session.auto_project, h.app.ui.pick_bodies), (true, false));
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert!(applied.matches(&h.app), "Cancel after Defaults returns to Apply");

            // The session fields also roll back when the page was opened with the opposite values.
            original.clone().restore_other_preferences(&mut h.app).unwrap();
            h.call("ui.prefs", json!({"open":"Sketch","button":"defaults"}));
            assert_eq!((h.app.session.auto_project, h.app.ui.pick_bodies), (true, false));
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert!(original.matches(&h.app));
        }
    }

    #[test]
    fn sketch_disabled_dependencies_keep_their_stored_choices() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"language":"en","open":"Sketch","sketch_auto_locate":true}));
        for (query, parent, child) in [
            ("prefs.sketch.drawing", "sketch_auto_dimensions", "sketch_auto_locate"),
            ("prefs.sketch.snapping_guides", "sketch_tracking_guides", "sketch_tracking_points"),
        ] {
            h.app.prefs_window.shell.filter = h.app.tr(query).into();
            h.frames(5);
            for enabled in [false, true, false] {
                set(&mut h.app, &json!({(parent):enabled, (child):true})).unwrap();
                h.frames(5);
                let handle = format!("prefs:{child}");
                super::reveal_preference(&mut h, &handle);
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                let prefs = serde_json::to_value(&h.app.preferences).unwrap();
                assert_eq!(prefs[parent], enabled);
                assert_eq!(prefs[child], !enabled, "a disabled {child} retains true; an enabled one toggles");
            }
        }
    }

    #[test]
    fn sketch_shift_legend_updates_in_the_checkbox_release_frame() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", json!({"language":"en","theme":theme,"open":"Sketch"}));
            h.app.prefs_window.shell.filter = h.app.tr("prefs.selecting").into();
            h.frames(5);
            super::reveal_preference(&mut h, "prefs:selection_shift_add_only");
            for enabled in [true, false] {
                let at = crate::scenario::rect_of("prefs:selection_shift_add_only").unwrap().center();
                h.frame_with(crate::test_support::pointer(at, true));
                h.frame_with(crate::test_support::pointer(at, false));
                assert_eq!(h.app.preferences.selection_shift_add_only, enabled);
                let mut actions = Vec::new();
                for shape in &h.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| {
                        if let egui::Shape::Text(text) = shape {
                            actions.push(text.galley.job.text.clone());
                        }
                    });
                }
                assert_eq!(actions.iter().filter(|text| text.as_str() == h.app.tr("prefs.add_item")).count(), usize::from(enabled));
                assert_eq!(actions.iter().filter(|text| text.as_str() == h.app.tr("prefs.toggle_item")).count(), if enabled { 1 } else { 2 });
                let primary = crate::tool_modifiers::ctrl_name_for(crate::tool_modifiers::Platform::of(&h.ctx));
                assert!(actions.iter().any(|text| text == primary), "the live platform keycap remains visible");
            }
        }
    }

    #[test]
    fn theme_options_raw_values_default_while_theme_api_clamps() {
        use crate::theme_files::{OptionNotice, OptionNoticeKind, OptionValue};
        use solidcraft_engine::schema::ui_prefs::ThemeOptionNoticeKind;
        let file = crate::theme_files::from_value(&option_theme_file("Options A")).unwrap();
        for (raw, clamped) in [(-1.0, 0.0), (20.0, 10.0)] {
            let (values, notices) = crate::theme_files::effective_values(&file, &BTreeMap::from([("amount".into(), OptionValue::Num(raw))]));
            assert_eq!(values["amount"], OptionValue::Num(clamped));
            assert_eq!(notices, [OptionNotice { id: "amount".into(), kind: OptionNoticeKind::Clamped }]);

            let mut prefs = Prefs::default();
            prefs.theme_options.insert(file.id.clone(), BTreeMap::from([("amount".into(), json!(raw))]));
            let saved = prefs.theme_options.clone();
            let (values, notices) = effective_theme_options(&prefs, &file);
            assert_eq!(values["amount"], OptionValue::Num(5.0));
            assert_eq!(notices.len(), 1);
            assert_eq!((notices[0].id.as_str(), notices[0].kind), ("amount", ThemeOptionNoticeKind::OutOfRange));
            assert_eq!(prefs.theme_options, saved);
        }
    }

    #[test]
    fn tabs_preferences_validate_atomically_restore_safely_and_preserve_export_defaults() {
        crate::test_support::host_data();
        for base in ["dark", "light"] {
            let file = crate::test_support::tab_fixture(base);
            let source = serde_json::to_value(&file).unwrap();
            let mut h = crate::scenario::Harness::new();
            for invalid in [json!(false), json!({"sketch":{}}), json!({"SKETCH":{"error":"#ffffff"}}), json!({"SKETCH":{"panel":"bad"}})] {
                let mut damaged = source.clone();
                damaged["tabs"] = invalid;
                let before = h.app.preferences.clone();
                let error = control(&mut h.app, &json!({"custom_themes":[damaged],"theme":file.id,"ui_scale":1.5,"open":"Interface"})).unwrap_err();
                assert!(error.contains("tabs"), "{error}");
                assert_eq!(h.app.preferences, before);
                assert!(!h.app.prefs_window.shell.open);
                h.app.load_prefs(&json!({"preferences":{"custom_themes":[damaged,source],"theme":file.id,"zoom_reverse":true}}).to_string());
                assert_eq!(h.app.preferences.custom_themes.len(), 2);
                assert!(h.app.preferences.custom_themes[0].tabs.is_empty());
                assert_eq!(h.app.preferences.custom_themes[1].tabs, file.tabs);
                assert!(h.app.preferences.zoom_reverse);
            }
            let out = control(&mut h.app, &json!({"custom_themes":[source],"theme":file.id,"theme_options":{&file.id:{"tab_colours":false,"tab_fade_ms":123}},"theme_export":true})).unwrap();
            assert_eq!(out["theme_file"]["tabs"], serde_json::to_value(&file.tabs).unwrap());
            assert_eq!(out["theme_file"]["options"][0]["default"], true);
            assert_eq!(out["theme_file"]["options"][1]["default"], 240.0);
            assert_eq!(out["theme_option_values"]["tab_fade_ms"], 123.0, "step is a control increment, never rounding");
            assert_eq!(out["theme_option_defs"].as_array().unwrap().iter().find(|o| o["id"] == "tab_fade_ms").unwrap()["step"], 20.0);
            let mut restored = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            restored.load_prefs(&h.app.prefs());
            assert_eq!(restored.preferences.custom_themes, h.app.preferences.custom_themes);
            assert_eq!(restored.preferences.theme_options, h.app.preferences.theme_options);
        }
    }

    #[test]
    fn tabs_followed_reload_updates_surfaces_without_losing_saved_options() {
        let mut h = crate::scenario::Harness::new();
        let mut source = crate::theme_files::export_file(&crate::test_support::tab_fixture("dark"));
        let path = h.dir.join("tabs.json");
        std::fs::write(&path, source.to_string()).unwrap();
        control(&mut h.app, &json!({"theme_follow":path,"theme_options":{"file-tabs-dark":{"tab_fade_ms":0}}})).unwrap();
        assert!(h.call("engine.execute", json!({"command":"sketch.create","params":{"plane":"XY"}}))["ok"].as_bool().unwrap());
        h.frames(2);
        assert_eq!(Tokens::get().viewport_top, egui::Color32::from_rgb(0x10, 0x31, 0x28));
        source["tabs"]["SKETCH"]["viewport_top"] = json!("#1a4232");
        std::fs::write(&path, source.to_string()).unwrap();
        control(&mut h.app, &json!({"theme_follow":path})).unwrap();
        h.frames(2);
        assert_eq!(Tokens::get().viewport_top, egui::Color32::from_rgb(0x1a, 0x42, 0x32));
        assert_eq!(h.app.preferences.theme_options["file-tabs-dark"]["tab_fade_ms"], 0);
        source["tabs"]["SKETCH"]["warning"] = json!("#ffffff");
        std::fs::write(&path, source.to_string()).unwrap();
        assert!(control(&mut h.app, &json!({"theme_follow":path})).unwrap()["theme_follow_error"].as_str().unwrap().contains("warning"));
        h.frames(2);
        assert_eq!(Tokens::get().viewport_top, egui::Color32::from_rgb(0x1a, 0x42, 0x32));
        control(&mut h.app, &json!({"theme_follow":""})).unwrap();
        h.frames(2);
        assert_eq!(Tokens::get(), Tokens::dark());
    }

    #[test]
    fn theme_options_direct_definitions_validate_and_restore_safely() {
        let mut source = option_theme_file("Options A");
        source["id"] = json!("options-a");
        let mut other = source.clone();
        other["id"] = json!("options-b");
        other["name"] = json!("Options B");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let invalid_tables = [
            ("options", json!((0..17).map(|i| json!({"id":format!("option_{i}"),"type":"bool","default":true})).collect::<Vec<_>>())),
            ("options", json!([{ "id":"amount","type":"range","default":5,"min":10,"max":0,"step":1 }])),
            ("options", json!([{ "id":"amount","type":"range","default":11,"min":0,"max":10,"step":1 }])),
            ("options", json!([{ "id":"amount","type":"range","default":5,"min":0,"max":10,"step":0 }])),
            ("options", json!([{ "id":"amount","type":"range","default":"bad","min":0,"max":10,"step":1 }])),
            ("options", json!([{ "id":"enabled","type":"unknown","default":true }])),
            ("options", json!([{ "id":"enabled","type":"bool","default":true,"parent":"missing" }])),
            ("options", json!([{ "id":"a","type":"bool","default":true,"parent":"b" },{ "id":"b","type":"bool","default":true,"parent":"a" }])),
            ("options", json!([{ "id":"style","type":"choice","default":"missing","values":[{"id":"a","label":{}}] }])),
            ("overlays", json!((0..33).map(|_| json!({"when":{"opt":"enabled","eq":true},"colors":{"panel":"#abcdef"}})).collect::<Vec<_>>())),
            ("overlays", json!([{ "when":{"opt":"enabled","eq":1},"colors":{"panel":"#abcdef"} }])),
            ("overlays", json!([{ "when":{"opt":"unknown","eq":true},"colors":{"panel":"#abcdef"} }])),
            ("overlays", json!([{ "when":{"opt":"enabled","eq":true},"radii":{"radius_sm":-1} }])),
        ];
        for (table, invalid) in invalid_tables {
            let mut malformed = source.clone();
            malformed[table] = invalid;
            assert!(crate::theme_files::from_value(&malformed).is_err(), "{malformed}");
            let before = app.preferences.clone();
            let error = control(&mut app, &json!({"custom_themes":[malformed],"ui_scale":1.5,"open":"Interface"})).unwrap_err();
            assert!(error.contains("custom_themes[0]"), "{error}");
            assert_eq!(app.preferences, before);
            assert!(!app.prefs_window.shell.open);

            let mut restored = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            restored.load_prefs(
                &json!({"preferences":{
                    "custom_themes":[malformed,other],"theme":"options-a","zoom_reverse":true,
                    "theme_options":{"options-a":{"amount":99}}
                }})
                .to_string(),
            );
            assert_eq!(restored.preferences.custom_themes.len(), 2);
            assert!(restored.preferences.custom_themes[0].options.is_empty());
            assert!(restored.preferences.custom_themes[0].overlays.is_empty());
            assert_eq!(restored.preferences.custom_themes[0].colors, restored.preferences.custom_themes[1].colors);
            assert!(!restored.preferences.custom_themes[1].options.is_empty());
            assert!(restored.preferences.zoom_reverse);
            apply(&mut restored, &egui::Context::default());
            assert_eq!(
                resolved_theme(&restored.preferences, &restored.preferences.custom_themes[0]).panel,
                egui::Color32::from_rgb(0x11, 0x22, 0x33)
            );
            let out = control(&mut restored, &json!({"ui_scale":1.25})).unwrap();
            assert_eq!(out["theme_option_defs"], json!([]));
            assert_eq!(out["theme_option_values"], json!({}));
            assert_eq!(out["theme_options"]["options-a"]["amount"], 99);
            assert!(restored.preferences.zoom_reverse);
        }

        // Reserved fields ignored by import must also be ignored before serde sees an update.
        source["options"][3] = json!({"id":"blur","type":"range","default":999,"step":null,"unit":false,"label":null,"advanced":"ignored"});
        let expected = crate::theme_files::from_value(&source).unwrap();
        control(&mut app, &json!({"custom_themes":[source],"theme":"options-a"})).unwrap();
        assert_eq!(app.preferences.custom_themes, [expected]);
        app.preferences.custom_themes[0].options[1].kind =
            crate::theme_files::OptionKind::Range { default: 5.0, min: 10.0, max: 0.0, step: 1.0, unit: None };
        let out = control(&mut app, &json!({"ui_scale":1.25})).unwrap();
        assert_eq!(out["theme_option_defs"], json!([]));
        assert_eq!(app.preferences.custom_themes[0].colors["panel"], "#112233");
    }

    #[test]
    fn theme_options_persist_isolate_reset_remove_and_export_definitions() {
        let mut h = crate::scenario::Harness::new();
        let out =
            control(&mut h.app, &json!({"theme_import":option_theme_file("Options A"), "theme_options":{"options-a":{"enabled":true,"amount":7.5}}}))
                .unwrap();
        assert_eq!(out["theme_option_values"]["enabled"], true);
        h.frames(2);
        assert_eq!(Tokens::get().panel, egui::Color32::from_rgb(0xab, 0xcd, 0xef));
        control(&mut h.app, &json!({"theme_import":option_theme_file("Options B"), "theme_options":{"options-b":{"amount":3}}})).unwrap();
        assert_eq!(control(&mut h.app, &json!({})).unwrap()["theme_option_values"]["enabled"], false);
        let out = control(&mut h.app, &json!({"theme":"options-a", "theme_export":true})).unwrap();
        assert_eq!(out["theme_option_values"]["amount"], 7.5);
        assert_eq!(out["theme_file"]["options"][0]["default"], false);
        assert_eq!(out["theme_file"]["colors"]["panel"], "#112233");
        assert_eq!(out["theme_file"]["overlays"][0]["colors"]["panel"], "#abcdef");
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert_eq!(other.preferences.theme_options, h.app.preferences.theme_options);
        control(&mut other, &json!({"theme_options":{"options-a":{"amount":null}}})).unwrap();
        assert_eq!(control(&mut other, &json!({})).unwrap()["theme_option_values"]["amount"], 5.0);
        assert!(other.preferences.theme_options["options-a"].contains_key("enabled"));
        control(&mut other, &json!({"theme_options":{"options-a":{}}})).unwrap();
        assert!(!other.preferences.theme_options.contains_key("options-a"));
        assert!(other.preferences.theme_options.contains_key("options-b"));
        control(&mut h.app, &json!({"theme_remove":"options-a"})).unwrap();
        assert!(!h.app.preferences.theme_options.contains_key("options-a"));
        assert!(h.app.preferences.theme_options.contains_key("options-b"));
        control(&mut other, &json!({"open":"Interface", "button":"defaults"})).unwrap();
        assert!(other.preferences.theme_options.is_empty());
        h.frames(2);
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);
        control(
            &mut h.app,
            &json!({"theme_import":option_theme_file("Options A"),"theme_options":{"options-a":{"enabled":true}},"open":"Interface"}),
        )
        .unwrap();
        control(&mut h.app, &json!({"button":"apply"})).unwrap();
        control(&mut h.app, &json!({"button":"defaults"})).unwrap();
        h.frames(2);
        assert!(h.app.preferences.theme_options.is_empty());
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);
        control(&mut h.app, &json!({"button":"cancel"})).unwrap();
        h.frames(2);
        assert_eq!(Tokens::get().panel, egui::Color32::from_rgb(0xab, 0xcd, 0xef));
    }

    #[test]
    fn theme_options_saved_invalid_values_survive_with_defaults_and_notices() {
        let mut h = crate::scenario::Harness::new();
        control(&mut h.app, &json!({"theme_import":option_theme_file("Options A")})).unwrap();
        let mut saved = serde_json::to_value(&h.app.preferences).unwrap();
        saved["theme_options"] =
            json!({"options-a":{"enabled":[], "amount":99, "style":"dropped", "removed":{"future":true}}, "unknown-theme":{"anything":null}});
        saved["zoom_reverse"] = json!(true);
        saved["ui_scale"] = json!("bad saved field");
        h.app.preferences = lenient(&saved);
        assert!(h.app.preferences.zoom_reverse);
        let out = control(&mut h.app, &json!({"look_at_anim":false})).unwrap();
        assert_eq!(out["theme_options"], saved["theme_options"]);
        assert_eq!(out["theme_option_values"], json!({"enabled":false,"amount":5.0,"style":"warm","blur":0.0}));
        assert_eq!(
            out["theme_option_notices"],
            json!([
                {"id":"amount","kind":"out_of_range"}, {"id":"enabled","kind":"wrong_type"},
                {"id":"removed","kind":"unknown_option"}, {"id":"style","kind":"unknown_choice"}
            ])
        );
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert_eq!(other.preferences.theme_options, h.app.preferences.theme_options);
        assert!(other.preferences.zoom_reverse);
    }

    #[test]
    fn theme_options_control_refuses_entire_mixed_call_before_side_effects() {
        let mut h = crate::scenario::Harness::new();
        control(&mut h.app, &json!({"theme_import":option_theme_file("Options A")})).unwrap();
        h.app.stand_up = Some(vec!["Offered".into()]);
        h.app.stand_up_note = true;
        let before = h.app.prefs();
        for (options, key) in [
            (json!({"unknown":{"enabled":true}}), "theme_options.unknown"),
            (json!({"options-a":{"unknown":true}}), "theme_options.options-a.unknown"),
            (json!({"options-a":{"enabled":1}}), "theme_options.options-a.enabled"),
            (json!({"options-a":{"amount":10.0000001}}), "theme_options.options-a.amount"),
            (json!({"options-a":{"amount":-1}}), "theme_options.options-a.amount"),
            (json!({"options-a":{"style":"missing"}}), "theme_options.options-a.style"),
            (json!({"options-a":{"amount":8,"enabled":{}},"options-b":{"enabled":true}}), "theme_options.options-a.enabled"),
        ] {
            let err = control(
                &mut h.app,
                &json!({
                    "theme_import":option_theme_file("Options B"), "theme_options":options,
                    "theme":"options-b", "theme_follow":"", "open":"Interface", "button":"defaults", "stand_up":false,
                    "msaa":4, "default_units":"inch", "up_axis":"y", "ui_scale":1.5
                }),
            )
            .unwrap_err();
            assert!(err.contains(key), "{err}");
            assert_eq!(h.app.prefs(), before, "{key}");
            assert!(!h.app.prefs_window.shell.open && h.app.prefs_window.shell.snapshot().is_none());
            assert_eq!(h.app.stand_up.as_deref(), Some(["Offered".to_string()].as_slice()));
            assert!(h.app.stand_up_note);
        }
        let err = control(&mut h.app, &json!({"theme_remove":"options-a", "theme_options":{"options-a":{}}})).unwrap_err();
        assert!(err.contains("theme_options.options-a"));
        assert_eq!(h.app.prefs(), before);
        assert!(control(&mut h.app, &json!({"theme_options":{"options-a":{"enabled":true}},"ui_scale":9})).is_err());
        assert_eq!(h.app.prefs(), before);
        assert!(control(&mut h.app, &json!({"theme_import":option_theme_file("Options B"),"button":"invalid"})).is_err());
        assert_eq!(h.app.prefs(), before);
        assert!(control(&mut h.app, &json!({"theme_options":null})).is_err());
    }

    #[test]
    fn theme_options_active_reply_is_localized_ordered_and_matches_schema_types() {
        use solidcraft_engine::schema::ui_prefs::{PrefsReply, ThemeOptionKind};
        let mut h = crate::scenario::Harness::new();
        let out = control(&mut h.app, &json!({"theme_import":option_theme_file("Options A"), "language":"de"})).unwrap();
        let reply: PrefsReply = serde_json::from_value(out.clone()).unwrap();
        assert_eq!(reply.options.theme_option_defs.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["blur", "enabled", "amount", "style"]);
        let defs = &reply.options.theme_option_defs;
        assert!(defs[0].standard && !defs[0].available);
        let crate::theme_options::Availability::Unavailable(reason) = blur_availability(&h.app) else { panic!("headless fixture has no GPU") };
        assert_eq!(defs[0].reason.as_deref(), Some(solidcraft_ui_text::Locale::for_language("de").text(reason)));
        assert_eq!((&defs[1].label, &defs[1].help), (&"Aktiv".to_string(), &"Farben verwenden".to_string()));
        assert!(defs[1].available && defs[1].reason.is_none());
        assert!(defs[2].advanced && defs[2].section.as_deref() == Some("Details"));
        let ThemeOptionKind::Choice { values, .. } = &defs[3].kind else { panic!("choice definition") };
        assert_eq!(values.iter().map(|v| v.label.as_str()).collect::<Vec<_>>(), ["Warmton", "Cool"]);
        assert_eq!(serde_json::to_value(reply).unwrap(), out);
        let out = control(&mut h.app, &json!({"theme":"dark"})).unwrap();
        assert_eq!(out["theme_option_defs"], json!([]));
        assert_eq!(out["theme_option_values"], json!({}));
        assert_eq!(out["theme_option_notices"], json!([]));
    }

    #[test]
    fn theme_options_followed_file_uses_its_id_and_reload_keeps_saved_entries() {
        let mut h = crate::scenario::Harness::new();
        let path = h.dir.join("follow-options.json");
        let mut file = option_theme_file("Follow Options");
        file["base"] = json!("light");
        std::fs::write(&path, file.to_string()).unwrap();
        let out = control(&mut h.app, &json!({"theme_follow":path, "theme_options":{"file-follow-options":{"enabled":true,"amount":8}}})).unwrap();
        assert_eq!(out["theme"], "dark");
        assert_eq!(out["theme_option_values"]["enabled"], true);
        h.frames(2);
        assert_eq!(Tokens::get().panel, egui::Color32::from_rgb(0xab, 0xcd, 0xef));
        assert!(!h.app.ui.dark);
        let saved = h.app.prefs();
        let err = control(&mut h.app, &json!({"theme":"light", "theme_options":{"missing":{}}, "stand_up":false})).unwrap_err();
        assert!(err.contains("theme_options.missing"));
        assert_eq!(h.app.prefs(), saved);
        assert_eq!(active_theme_file(&h.app).unwrap().id, "file-follow-options");
        let other_path = h.dir.join("other-options.json");
        std::fs::write(&other_path, option_theme_file("Other Options").to_string()).unwrap();
        let err = control(&mut h.app, &json!({"theme_follow":other_path,"theme_options":{"file-other-options":{"enabled":42}}})).unwrap_err();
        assert!(err.contains("theme_options.file-other-options.enabled"), "{err}");
        assert_eq!(h.app.prefs(), saved);
        assert_eq!(active_theme_file(&h.app).unwrap().id, "file-follow-options");
        file["options"][1]["max"] = json!(6);
        file["options"][1]["default"] = json!(2);
        std::fs::write(&path, file.to_string()).unwrap();
        let out = control(&mut h.app, &json!({"theme_follow":path, "theme_export":true})).unwrap();
        assert_eq!(out["theme_option_values"]["amount"], 2.0);
        assert_eq!(out["theme_options"]["file-follow-options"]["amount"], 8);
        assert_eq!(out["theme_option_notices"], json!([{"id":"amount","kind":"out_of_range"}]));
        assert_eq!(out["theme_file"]["name"], "Follow Options");
        assert_eq!(out["theme_file"]["options"][1]["default"], 2.0);
        std::fs::write(&path, "incomplete").unwrap();
        let out = control(&mut h.app, &json!({"theme_follow":path})).unwrap();
        assert!(out["theme_follow_error"].is_string());
        assert_eq!(out["theme_option_values"]["enabled"], true);
        let out = control(&mut h.app, &json!({"theme_follow":""})).unwrap();
        assert_eq!(out["dark"], true);
        assert!(active_theme_file(&h.app).is_none());
        h.frames(2);
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);
        let out = control(&mut h.app, &json!({"theme_follow":path})).unwrap();
        assert_eq!(out["theme_option_values"], json!({}), "stopping clears the last good file");
        control(&mut h.app, &json!({"theme_follow":""})).unwrap();
        std::fs::write(&path, file.to_string()).unwrap();
        follow_theme_file_for_run(&mut h.app, path.to_str().unwrap()).unwrap();
        h.frames(2);
        let out = control(&mut h.app, &json!({})).unwrap();
        assert_eq!(out["theme_follow"], "");
        assert_eq!(out["theme_option_values"]["enabled"], true);
        assert_eq!(Tokens::get().panel, egui::Color32::from_rgb(0xab, 0xcd, 0xef));
        assert!(!h.app.ui.dark);
        let saved = h.app.prefs();
        let launch = h.app.prefs_window.follow.launch.clone();
        assert!(control(&mut h.app, &json!({"theme":"dark","theme_options":{"missing":{}}})).is_err());
        assert_eq!(h.app.prefs(), saved);
        assert_eq!(h.app.prefs_window.follow.launch, launch);
        assert_eq!(active_theme_file(&h.app).unwrap().id, "file-follow-options");
        let out = control(&mut h.app, &json!({"theme":"dark"})).unwrap();
        assert_eq!(out["dark"], true);
        assert!(out["theme_followed"].is_null());
        assert!(h.app.prefs_window.follow.theme().is_none());
        h.frames(2);
        assert_eq!(Tokens::get().panel, Tokens::dark().panel);
    }

    #[test]
    fn gruvbox_legacy_selection_migrates_on_load_and_preserves_saved_values() {
        for (old, family, contrast) in crate::theme_files::GRUVBOX_ALIASES {
            let saved = json!({"theme":old,"theme_options":{
                family:{"contrast":"standard","unrelated":{"keep":true}},
                old:{"contrast":"standard","future":42}
            }});
            let migrated = lenient(&saved);
            assert_eq!(migrated.theme, family);
            assert_eq!(migrated.theme_options[family]["contrast"], contrast);
            assert_eq!(migrated.theme_options[family]["unrelated"], json!({"keep":true}));
            assert_eq!(migrated.theme_options[family]["future"], 42);
            assert!(!migrated.theme_options.contains_key(old));
            assert_eq!(serde_json::to_value(migrate(migrated.clone())).unwrap(), serde_json::to_value(&migrated).unwrap());
            let mut h = crate::scenario::Harness::new();
            h.app.load_prefs(&json!({"preferences":saved}).to_string());
            assert_eq!(h.app.preferences.theme, family);
            assert_eq!(h.app.preferences.theme_options[family]["contrast"], contrast);
            let output: serde_json::Value = serde_json::from_str(&h.app.prefs()).unwrap();
            assert_eq!(output["preferences"]["theme"], family);
            assert_eq!(output["preferences"]["theme_options"][family]["contrast"], contrast);
        }
        // An unselected legacy map must not replace an existing canonical contrast.
        let migrated = lenient(&json!({"theme":"dark","theme_options":{
            "gruvbox-dark":{"contrast":"soft"},"gruvbox-dark-hard":{"contrast":"hard"}
        }}));
        assert_eq!(migrated.theme_options["gruvbox-dark"]["contrast"], "soft");
    }

    #[test]
    fn gruvbox_control_legacy_ids_allow_explicit_choices_resets_and_atomic_errors() {
        let mut h = crate::scenario::Harness::new();
        for (old, family, contrast) in crate::theme_files::GRUVBOX_ALIASES {
            let out = control(&mut h.app, &json!({"theme":old})).unwrap();
            assert_eq!(out["theme"], family);
            assert_eq!(out["theme_option_values"]["contrast"], contrast);
            let ids = out["theme_ids"].as_array().unwrap();
            assert_eq!(ids.iter().filter(|id| id.as_str().is_some_and(|id| id.starts_with("gruvbox-"))).count(), 2);
            let out = control(&mut h.app, &json!({"theme":old,"theme_options":{family:{"contrast":"standard"}}})).unwrap();
            assert_eq!(out["theme_option_values"]["contrast"], "standard");
            let out = control(&mut h.app, &json!({"theme":old,"theme_options":{family:{"contrast":null}}})).unwrap();
            assert_eq!(out["theme_option_values"]["contrast"], "standard");
            let out = control(&mut h.app, &json!({"theme":old,"theme_options":{family:{}}})).unwrap();
            assert_eq!(out["theme_option_values"]["contrast"], "standard");
            let out = control(&mut h.app, &json!({"theme":old,"theme_options":{old:{"contrast":"hard"},family:{"contrast":"soft"}}})).unwrap();
            assert_eq!(out["theme_option_values"]["contrast"], "soft", "canonical explicit values win over alias entries");
            let before = h.app.prefs();
            assert!(control(&mut h.app, &json!({"theme":old,"open":"Interface","theme_options":{family:{"contrast":"invalid"}}})).is_err());
            assert_eq!(h.app.prefs(), before, "failed changes do not migrate or open the dialog");
            assert!(!h.app.prefs_window.shell.open);
        }
    }

    #[test]
    fn theme_files_import_apply_export_save_and_remove() {
        use serde_json::json;
        use solidcraft_theme::{ColorVision, Tokens};
        let mut h = crate::scenario::Harness::new();
        let out = h.call("ui.prefs", json!({"theme": "gruvbox-dark-hard"}));
        assert_eq!(out["result"]["theme"], "gruvbox-dark", "{out}");
        assert_eq!(out["result"]["theme_option_values"]["contrast"], "hard");
        assert!(out["result"]["theme_ids"].as_array().is_some_and(|ids| ids.len() == 2 + crate::theme_files::builtin().len()), "{out}");
        h.frames(2);
        let hard = resolved_theme(&h.app.preferences, active_theme_file(&h.app).unwrap());
        assert!(h.app.ui.dark);
        assert_eq!(Tokens::get().panel, hard.panel);
        assert_eq!(
            crate::viewport::scene_palette(true, ColorVision::Off).background.0,
            solidcraft_viewport::Rgb(hard.viewport_top.r(), hard.viewport_top.g(), hard.viewport_top.b())
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
        let body = egui::Id::unique("sc_prefs").with("collapsing");
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
    fn appearance_popups_own_escape_restore_focus_and_leave_cancel_to_the_next_escape() {
        for font in [true, false] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1100}));
            h.call("ui.prefs", json!({"language":"en","theme":"dark"}));
            let before = h.app.preferences.clone();
            h.call("ui.prefs", json!({"open":"Appearance"}));
            h.frames(3);
            reveal_preference(&mut h, "prefs:viewport_theme:light");
            h.step(&json!({"click":{"handle":"prefs:viewport_theme:light"}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.preferences.viewport_theme, crate::theme::ViewportTheme::Light);
            let (target, popup) = if font {
                ("prefs:ui_font", "dialog:Interface font search")
            } else {
                reveal_preference(&mut h, "dialog:Background:Color");
                h.step(&json!({"click":{"handle":"dialog:Background:Color"}})).unwrap();
                h.frames(2);
                ("prefs:viewport_background_picker", "color:popover")
            };
            reveal_preference(&mut h, target);
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            let rect = crate::scenario::rect_of(target).unwrap();
            assert!(body.contains_rect(rect), "{target} is visible before clicking");
            let edited = h.app.preferences.clone();
            assert_ne!(edited, before);
            h.step(&json!({"click":{"handle":target}})).unwrap();
            h.frames(2);
            assert!(crate::scenario::handle_at(popup).is_some() || crate::scenario::rect_of(popup).is_some(), "{target} opens its real popup");
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.prefs_window.shell.open && h.app.prefs_window.shell.section == PAGE_APPEARANCE);
            assert!(h.app.prefs_window.shell.snapshot().is_some());
            assert_eq!(h.app.preferences, edited, "dismissing {target} retains live changes");
            assert!(crate::scenario::handle_at(popup).is_none() && crate::scenario::rect_of(popup).is_none());
            // No pointer input: the actual combo/swatch must have regained keyboard focus.
            h.step(&json!({"key":"Enter"})).unwrap();
            h.frames(2);
            assert!(
                crate::scenario::handle_at(popup).is_some() || crate::scenario::rect_of(popup).is_some(),
                "{target} reopens from its restored focus"
            );
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.prefs_window.shell.open && h.app.prefs_window.shell.snapshot().is_some());
            assert_eq!(h.app.preferences, edited);
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(!h.app.prefs_window.shell.open && h.app.prefs_window.shell.snapshot().is_none());
            assert_eq!(h.app.preferences, before, "Escape with no popup cancels and restores the original snapshot");
        }
    }

    /// Letters typed in the font list's own Search box narrow the list. Preferences' search takes
    /// typing only when no text field has the keyboard, so it must not take these.
    #[test]
    fn typing_in_the_font_lists_search_narrows_the_list_not_the_page() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Interface"}));
        // Found by searching, as the setting is far down the page.
        h.step(&json!({"text": "font"})).expect("search");
        h.frames(3);
        let combo = crate::scenario::handle_at("prefs:ui_font").expect("the search finds the font row");
        let body = crate::scenario::rect_of("prefs:page_body").expect("the Preferences page body");
        let picker = crate::scenario::rect_of("prefs:ui_font").expect("the font picker bounds");
        assert!(body.contains_rect(picker) && picker.contains(combo), "the font picker is inside the visible page before clicking");
        h.call("ui.click", json!({"x":combo.x,"y":combo.y}));
        h.frames(2);
        let search = format!("dialog:{} search", solidcraft_ui_text::tr_label("Interface font"));
        let field = crate::scenario::handle_at(&search).expect("the font list is open");
        h.call("ui.click", json!({"x":field.x,"y":field.y}));
        h.frames(2);
        h.step(&json!({"text": "Comic"})).expect("type");
        h.frames(2);
        assert_eq!(h.app.prefs_window.shell.filter, "font", "the Preferences search took the letters");
        assert_eq!(h.app.prefs_window.font_filter, "Comic", "the font list's search keeps them");
        assert!(crate::scenario::handle_at(&search).is_some(), "the font list stays open");
        h.call("ui.prefs", json!({"button":"cancel"}));
    }

    #[test]
    fn number_fields_take_a_decimal_point_or_a_decimal_comma() {
        // #1347
        assert_eq!(crate::ui_kit::parse_number("2.5"), Some(2.5));
        assert_eq!(crate::ui_kit::parse_number("2,5"), Some(2.5));
        assert_eq!(crate::ui_kit::parse_number(" -0,25 "), Some(-0.25));
        assert_eq!(crate::ui_kit::parse_number("\u{2212}1,5"), Some(-1.5));
        assert_eq!(crate::ui_kit::parse_number("12"), Some(12.0));
        for bad in ["1.234,5", "1,2,3", ",", "abc", "", "inf", "NaN"] {
            assert_eq!(crate::ui_kit::parse_number(bad), None, "{bad}");
        }
    }

    #[test]
    fn render_legacy_image_defaults_round_trips_and_resets() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        assert!(!app.preferences.render_legacy_image_defaults);
        let out = super::set(&mut app, &json!({"render_legacy_image_defaults":true})).unwrap();
        assert_eq!(out["render_legacy_image_defaults"], true);
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert!(restored.preferences.render_legacy_image_defaults);
        super::defaults(&mut restored, super::PAGE_GRAPHICS);
        assert!(!restored.preferences.render_legacy_image_defaults);
    }

    #[test]
    fn render_legacy_memory_admission_applies_to_both_hosts_and_roundtrips() {
        use serde_json::json;
        use solidcraft_engine::render::job::{CpuHost, NativeMemoryPolicy};
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        let cpu = app.session.render_job_host().unwrap().clone();
        let gpu = CpuHost::cooperative();
        app.render_workspace.path_host = crate::render_workspace::PathHost::Ready(gpu.clone());
        assert!(!app.preferences.render_legacy_memory_admission);
        assert_eq!(cpu.native_memory_policy(), NativeMemoryPolicy::AvailableRam);
        assert_eq!(gpu.native_memory_policy(), NativeMemoryPolicy::AvailableRam);
        super::control(&mut app, &json!({"open":"Graphics"})).unwrap();
        assert!(app.prefs_window.shell.open && app.prefs_window.shell.snapshot().is_some());
        let out = super::set(&mut app, &json!({"render_legacy_memory_admission":true})).unwrap();
        assert_eq!(out["render_legacy_memory_admission"], true);
        assert_eq!(cpu.native_memory_policy(), NativeMemoryPolicy::SoftwarePoolOnly);
        assert_eq!(gpu.native_memory_policy(), NativeMemoryPolicy::SoftwarePoolOnly);
        assert!(super::set(&mut app, &json!({"render_legacy_memory_admission":"yes"})).is_err());
        assert!(app.preferences.render_legacy_memory_admission);
        crate::prefs_window::close(&mut app, true);
        assert!(!app.prefs_window.shell.open && app.prefs_window.shell.snapshot().is_none());
        assert!(!app.preferences.render_legacy_memory_admission);
        // Cancel must restore both reused hosts before any frame/backend sync.
        assert_eq!(cpu.native_memory_policy(), NativeMemoryPolicy::AvailableRam);
        assert_eq!(gpu.native_memory_policy(), NativeMemoryPolicy::AvailableRam);

        super::set(&mut app, &json!({"render_legacy_memory_admission":true})).unwrap();
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert!(restored.preferences.render_legacy_memory_admission);
        let restored_host = restored.session.render_job_host().unwrap().clone();
        assert_eq!(restored_host.native_memory_policy(), NativeMemoryPolicy::SoftwarePoolOnly);
        super::defaults(&mut restored, super::PAGE_GRAPHICS);
        assert!(!restored.preferences.render_legacy_memory_admission);
        assert_eq!(restored_host.native_memory_policy(), NativeMemoryPolicy::AvailableRam);
    }

    #[test]
    fn render_previous_geometry_budget_round_trips_and_resets() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        assert!(!app.preferences.render_previous_geometry_budget);
        let out = super::set(&mut app, &json!({"render_previous_geometry_budget":true})).unwrap();
        assert_eq!(out["render_previous_geometry_budget"], true);
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert!(restored.preferences.render_previous_geometry_budget);
        super::defaults(&mut restored, super::PAGE_GRAPHICS);
        assert!(!restored.preferences.render_previous_geometry_budget);
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

    /// The validation mode is once per feature by default; set by name or by the command, the
    /// two stay one setting, the session computes under it, and Restore Defaults on Design brings
    /// each_feature back. A saved unknown mode reads as the default.
    #[test]
    fn validation_defaults_to_each_feature_and_follows_the_command() {
        use serde_json::json;
        use solidcraft_engine::kernel::Validation;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!(app.preferences.validation, Validation::EachFeature);
        assert_eq!(super::lenient(&json!({"validation": "sometimes"})).validation, Validation::EachFeature);
        assert!(super::set(&mut app, &json!({"validation": "never"})).unwrap_err().contains("each_step, each_feature or off"));
        let out = super::set(&mut app, &json!({"validation": "each_step"})).unwrap();
        assert_eq!(out["validation"], "each_step");
        assert_eq!(super::lenient(&out).validation, Validation::EachStep);
        // A command runs under the preference.
        app.run("diagnostics.validation", json!({})).unwrap();
        assert_eq!(app.session.validation(), Validation::EachStep);
        // The command sets the preference too.
        app.run("diagnostics.validation", json!({"mode": "off"})).unwrap();
        assert_eq!((app.preferences.validation, app.session.validation()), (Validation::Off, Validation::Off));
        super::defaults(&mut app, super::PAGE_DESIGN);
        assert_eq!(app.preferences.validation, Validation::EachFeature);
    }

    #[test]
    fn joint_symbols_default_to_the_ring_and_axis_and_badges_highlight_moving_joints() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!((app.preferences.joint_symbols, app.preferences.highlight_moving_joints), (crate::joint_symbols::RING_AND_AXIS, true));
        let saved = super::set(&mut app, &json!({"joint_symbols": 6, "highlight_moving_joints": false})).unwrap();
        assert_eq!((saved["joint_symbols"].as_u64(), saved["highlight_moving_joints"].as_bool()), (Some(6), Some(false)));
        assert!(super::set(&mut app, &json!({"joint_symbols": 7})).unwrap_err().contains("joint_symbols"));
        assert_eq!(app.preferences.joint_symbols, 6);
        super::defaults(&mut app, super::PAGE_GRAPHICS);
        assert_eq!((app.preferences.joint_symbols, app.preferences.highlight_moving_joints), (crate::joint_symbols::RING_AND_AXIS, true));
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
        // Hand-edited toolbar tab lists keep only the tabs each can hold, each once (#5717).
        let mut hidden = vec![json!("VIEW"), json!("SHEET METAL"), json!("VIEW"), json!("SURFACE")];
        hidden.extend(std::iter::repeat_n(json!("NOT A TAB"), 10_000));
        let tabs = super::lenient(&json!({"hidden_toolbar_tabs": hidden}));
        assert_eq!(tabs.hidden_toolbar_tabs, ["VIEW", "SHEET METAL", "SURFACE"]);
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

    /// Preferences › General › Largest file to open (#4086): the defaults are the limits there always
    /// were; `ui.prefs` raises both, up to 16 GB and never below the defaults; a saved value out of
    /// range reads back in range, and General's Restore Defaults puts them back.
    #[test]
    fn the_largest_file_to_open_defaults_raises_and_refuses_out_of_range() {
        use serde_json::json;
        use solidcraft_engine::io::limits::{FileLimits, GB};
        let mut h = crate::scenario::Harness::new();
        assert_eq!((h.app.preferences.max_step_file_mb, h.app.preferences.max_design_file_mb), (512, 256));
        assert_eq!(super::file_limits(&h.app.preferences), FileLimits::DEFAULT);
        let out = h.call("ui.prefs", json!({"max_step_file_mb": 4096, "max_design_file_mb": 2048}));
        assert_eq!((out["result"]["max_step_file_mb"].as_u64(), out["result"]["max_design_file_mb"].as_u64()), (Some(4096), Some(2048)), "{out}");
        assert_eq!(super::file_limits(&h.app.preferences), FileLimits { step: 4 * GB, design: 2 * GB });
        for bad in [
            json!({"max_step_file_mb": 16385}),
            json!({"max_design_file_mb": 100}),
            json!({"max_step_file_mb": -1}),
            json!({"max_design_file_mb": "2GB"}),
        ] {
            assert!(h.call("ui.prefs", bad.clone())["error"].is_string(), "{bad}");
        }
        assert_eq!(super::file_limits(&h.app.preferences), FileLimits { step: 4 * GB, design: 2 * GB }, "a refused value changes nothing");
        let read = super::lenient(&json!({"max_step_file_mb": 999_999, "max_design_file_mb": 3}));
        assert_eq!((read.max_step_file_mb, read.max_design_file_mb), (16384, 256));

        h.call("ui.prefs", json!({"open": "General"}));
        h.frames(2);
        super::reveal_preference(&mut h, "prefs:max_step_file_mb");
        assert!(crate::scenario::rect_of("prefs:max_design_file_mb").is_some());
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(super::file_limits(&h.app.preferences), FileLimits::DEFAULT);
        h.call("ui.prefs", json!({"button": "cancel"}));
    }

    /// #10874: Preferences › Sketch › Largest sketch starts at the limit sketches have always had,
    /// raises the limit every sketch is built to, refuses a value out of range, reads a hand-edited
    /// one back as the default, and Sketch's Restore Defaults puts it back.
    #[test]
    fn the_largest_sketch_defaults_raises_and_refuses_out_of_range() {
        use serde_json::json;
        use solidcraft_engine::sketch::limits::{CEILING_POINTS, DEFAULT_POINTS};
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.app.preferences.max_sketch_points, DEFAULT_POINTS);
        let out = h.call("ui.prefs", json!({"max_sketch_points": 50_000}));
        assert_eq!(out["result"]["max_sketch_points"].as_u64(), Some(50_000), "{out}");
        assert_eq!(super::sketch_limit(&h.app.preferences), 50_000);
        for bad in
            [json!({"max_sketch_points": DEFAULT_POINTS - 1}), json!({"max_sketch_points": CEILING_POINTS + 1}), json!({"max_sketch_points": "lots"})]
        {
            assert!(h.call("ui.prefs", bad.clone())["error"].is_string(), "{bad}");
        }
        assert_eq!(super::sketch_limit(&h.app.preferences), 50_000, "a refused value changes nothing");
        assert_eq!(super::lenient(&json!({"max_sketch_points": 5})).max_sketch_points, DEFAULT_POINTS);

        h.call("ui.prefs", json!({"open": "Sketch"}));
        h.frames(2);
        super::reveal_preference(&mut h, "prefs:max_sketch_points");
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(h.app.preferences.max_sketch_points, DEFAULT_POINTS);
        h.call("ui.prefs", json!({"button": "cancel"}));
    }

    #[test]
    fn general_files_controls_edit_real_limits_and_hosts_in_both_themes() {
        for theme in ["light", "dark"] {
            for width in [1468, 480] {
                let mut h = crate::scenario::Harness::new();
                h.app.home.open = false;
                h.call("ui.resize", json!({"width":width, "height":1100}));
                let configured = h.call(
                    "ui.prefs",
                    json!({"language":"en", "theme":theme, "network_locations":true, "network_hosts":["files.example", "nas.example"]}),
                );
                assert!(configured["error"].is_null(), "{theme} {width}: setting up Preferences failed: {configured}");
                let opened = h.call("ui.prefs", json!({"open":"General"}));
                assert!(opened["error"].is_null(), "{theme} {width}: opening General failed: {opened}");
                let initial_body = crate::scenario::rect_of("prefs:page_body").unwrap();
                for target in [
                    "dialog:widget:Profile name",
                    "prefs:profile:save",
                    "prefs:profile:import",
                    "prefs:profiles:held:row",
                    "prefs:profiles:computer:row",
                ] {
                    let rect = crate::scenario::rect_of(target).unwrap();
                    assert!(initial_body.contains_rect(rect), "{theme} {width}: {target} {rect:?} leaves {initial_body:?}");
                }
                if width == 480 {
                    let name = crate::scenario::rect_of("dialog:widget:Profile name").unwrap();
                    let save = crate::scenario::rect_of("prefs:profile:save").unwrap();
                    let import = crate::scenario::rect_of("prefs:profile:import").unwrap();
                    assert!(save.top() >= name.bottom(), "narrow Save wraps after the independent name entry");
                    assert!((save.top() - import.top()).abs() < 0.5);
                }
                let painted = |h: &crate::scenario::Harness, label: &str| {
                    let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                    h.shapes
                        .iter()
                        .find_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape
                                && text.galley.job.text == label
                            {
                                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                                if body.contains_rect(rect) && shape.clip_rect.contains_rect(rect) {
                                    return Some(rect);
                                }
                            }
                            None
                        })
                        .unwrap_or_else(|| panic!("{theme} {width}: {label} is not fully painted in the page"))
                };
                // Reveal the live dropdown, not the full-width layout row's allocation rectangle.
                super::reveal_preference(&mut h, "dialog:widget:Interface language");
                let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                let language = painted(&h, "Interface language");
                assert!((language.left() - body.left()).abs() < 0.75);
                assert!((crate::scenario::rect_of("dialog:control:Interface language").unwrap().left() - body.left() - 164.0).abs() < 0.5);
                super::reveal_preference(&mut h, "prefs:autosave_minutes");
                let autosave = painted(&h, "Autosave every");
                let autosave_help = crate::scenario::rect_of("dialog:row_help:Autosave every").unwrap();
                assert!((autosave.left() - body.left()).abs() < 0.75, "the painted autosave label shares the language label's left edge");
                assert!((autosave_help.left() - autosave.right() - 4.0).abs() < 1.0, "help follows the actual text, not the whole label column");
                assert!((crate::scenario::rect_of("prefs:autosave_minutes").unwrap().left() - body.left() - 164.0).abs() < 0.5);
                super::reveal_preference(&mut h, "prefs:autosave_timeline");
                let timeline = crate::scenario::rect_of("prefs:autosave_timeline").unwrap();
                let autosave_control = crate::scenario::rect_of("prefs:autosave_minutes").unwrap();
                assert!((timeline.left() - autosave_control.left()).abs() < 0.5);
                assert!((timeline.width() - 260.0_f32.min(body.right() - autosave_control.left())).abs() < 0.5);
                assert!(body.contains_rect(timeline));
                super::reveal_preference(&mut h, "prefs:max_step_file_mb");
                super::reveal_preference(&mut h, "dialog:row:Largest file to open");
                let field = crate::scenario::rect_of("prefs:max_step_file_mb").unwrap();
                let design = crate::scenario::rect_of("prefs:max_design_file_mb").unwrap();
                assert!(
                    (field.width() - 72.0).abs() < 0.5 && (design.width() - 72.0).abs() < 0.5,
                    "{theme}: both compound file editors retain their width"
                );
                let step_group = crate::scenario::rect_of("dialog:row:Largest file to open: STEP and import").unwrap();
                let design_group = crate::scenario::rect_of("dialog:row:Largest file to open: Design").unwrap();
                let step_caption = painted(&h, "STEP and import");
                let design_caption = painted(&h, "Design");
                let largest = painted(&h, "Largest file to open");
                assert!((step_caption.left() - field.left()).abs() < 0.75 && (design_caption.left() - design.left()).abs() < 0.75);
                assert!((largest.left() - body.left()).abs() < 0.75);
                assert!((largest.top() - step_caption.top() - 6.0).abs() <= 1.0, "the left label stays in its top-aligned 28 px cell");
                assert!(step_group.width() >= 90.0 && step_group.width() < 130.0, "{theme}: the subgroup follows its field, unit and caption");
                if width == 1468 {
                    assert!((step_caption.top() - design_caption.top()).abs() < 0.5, "{theme}: the actual painted captions share a top baseline");
                    assert!(
                        (field.top() - design.top()).abs() < 0.5 && field.right() < design.left(),
                        "{theme}: independent editors share one visual row"
                    );
                    assert!((design_group.left() - step_group.right() - 20.0).abs() < 0.5, "{theme}: compact pair gap");
                } else {
                    assert!(design_group.top() > step_group.bottom(), "{theme}: the pair wraps at narrow widths");
                    assert!((design_group.left() - step_group.left()).abs() < 0.5);
                }
                let group = crate::scenario::rect_of("dialog:row:Largest file to open").unwrap();
                assert!(group.contains_rect(field) && group.contains_rect(design));
                assert!(crate::scenario::rect_of("prefs:page_body").unwrap().contains_rect(group));
                h.step(&json!({"click":[field.center().x, field.center().y], "double":true})).unwrap();
                h.step(&json!({"key":"A", "cmd":true})).unwrap();
                h.step(&json!({"text":"4096"})).unwrap();
                h.step(&json!({"key":"Enter"})).unwrap();
                assert_eq!(h.app.preferences.max_step_file_mb, 4096);
                super::reveal_preference(&mut h, "prefs:max_design_file_mb");
                let field = crate::scenario::rect_of("prefs:max_design_file_mb").unwrap();
                h.step(&json!({"click":[field.center().x, field.center().y], "double":true})).unwrap();
                h.step(&json!({"key":"A", "cmd":true})).unwrap();
                h.step(&json!({"text":"2048"})).unwrap();
                h.step(&json!({"key":"Enter"})).unwrap();
                assert_eq!(h.app.preferences.max_design_file_mb, 2048);
                assert_eq!(super::file_limits(&h.app.preferences).step, 4096 * solidcraft_engine::io::limits::MB);
                let before = h.app.preferences.clone();
                for bad in [json!({"max_step_file_mb":16385}), json!({"max_design_file_mb":255})] {
                    assert!(h.call("ui.prefs", bad)["error"].is_string());
                    assert_eq!(h.app.preferences, before, "invalid file limits fail atomically");
                }
                super::reveal_preference(&mut h, "prefs:network_host:files.example:remove");
                h.step(&json!({"click":{"handle":"prefs:network_host:files.example:remove"}})).unwrap();
                assert_eq!(h.app.preferences.network_hosts, ["nas.example"]);
                h.step(&json!({"widget":"Allow network locations in designs"})).unwrap();
                h.frames(3);
                assert!(!h.app.preferences.network_locations);
                assert_eq!(h.app.preferences.network_hosts, ["nas.example"], "hiding the chips retains the hosts");
                assert!(crate::scenario::rect_of("prefs:network_host:nas.example:remove").is_none());
                super::reveal_preference(&mut h, "prefs:print_face_highlight");
                let highlight = painted(&h, "Print face highlight");
                let highlight_help = crate::scenario::rect_of("dialog:row_help:Print face highlight").unwrap();
                assert!((highlight.left() - body.left()).abs() < 0.75, "the painted highlight label shares the language/autosave left edge");
                assert!((highlight_help.left() - highlight.right() - 4.0).abs() < 1.0);
                assert!((crate::scenario::rect_of("prefs:print_face_highlight").unwrap().left() - body.left() - 164.0).abs() < 0.5);
                let track = crate::scenario::rect_of("dialog:slider_track:Print face highlight").unwrap();
                // The published rectangle includes the circular handle's endpoint insets.
                let input_range = track.x_range().shrink(track.height() / 2.5);
                h.step(&json!({"click":[egui::lerp(input_range, 0.75), track.center().y]})).unwrap();
                assert!(
                    (h.app.preferences.print_face_highlight - 0.75).abs() < 0.06,
                    "{theme} {width}: highlight={} track={track:?} input_range={input_range:?}",
                    h.app.preferences.print_face_highlight
                );
                super::reveal_preference(&mut h, "prefs:check_now");
                assert!(body.contains_rect(crate::scenario::rect_of("prefs:check_now").unwrap()));
                assert!(h.app.prefs_window.shell.open);
                h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
                assert_eq!((h.app.preferences.max_step_file_mb, h.app.preferences.max_design_file_mb), (DEFAULT_STEP_MB, DEFAULT_DESIGN_MB));
                assert_eq!(h.app.preferences.network_hosts, ["files.example", "nas.example"]);
                assert!(h.app.preferences.network_locations);
                assert_eq!(h.app.preferences.print_face_highlight, 0.2);
            }
        }
    }

    #[test]
    fn general_autosave_rows_apply_cancel_and_defaults_keep_the_effective_interval() {
        use std::time::Duration;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.step(&json!({"autosave":true})).unwrap();
        h.app.autosave_minutes = 7.0;
        super::set_autosave(&mut h.app);
        h.app.preferences.language = "en".into();
        h.app.preferences.nav = "tinkercad".into();
        h.app.preferences.reopen_designs = false;
        h.app.run("settings.profile.save", json!({"name":"Working"})).unwrap();
        let profiles = h.app.settings_profiles.list.clone();
        h.call("ui.prefs", json!({"open":"General"}));
        super::reveal_preference(&mut h, "prefs:autosave_minutes");
        super::reveal_preference(&mut h, "prefs:autosave_timeline");
        let number = crate::scenario::rect_of("prefs:autosave_minutes").unwrap();
        let timeline = crate::scenario::rect_of("prefs:autosave_timeline").unwrap();
        assert!((timeline.width() - 260.0).abs() < 0.5, "the timeline stays bounded in the control column");
        assert!((timeline.left() - number.left()).abs() < 0.5);
        assert!(timeline.top() >= number.bottom());
        assert!(crate::scenario::rect_of("prefs:page_body").unwrap().contains_rect(timeline));
        h.step(&json!({"click":{"handle":"prefs:autosave_minutes"}, "double":true})).unwrap();
        h.step(&json!({"key":"A", "cmd":true})).unwrap();
        h.step(&json!({"text":"2.5"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.autosave_minutes, 2.5);
        assert_eq!(h.app.autosave.as_ref().unwrap().interval, Duration::from_secs(150));
        assert_eq!(saved_prefs(&h.app)["autosave_minutes"], 7.0, "an unapplied value is not persisted");
        h.step(&json!({"click":{"handle":"prefs:apply"}})).unwrap();
        assert_eq!(saved_prefs(&h.app)["autosave_minutes"], 2.5);
        super::reveal_preference(&mut h, "prefs:autosave_minutes");
        h.step(&json!({"click":{"handle":"prefs:autosave_minutes"}, "double":true})).unwrap();
        h.step(&json!({"key":"A", "cmd":true})).unwrap();
        h.step(&json!({"text":"0"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.autosave_minutes, 0.0);
        assert!(h.app.autosave.as_ref().unwrap().interval > Duration::from_secs(365 * 24 * 60 * 60));
        h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
        assert_eq!(h.app.autosave_minutes, 2.5);
        assert_eq!(h.app.autosave.as_ref().unwrap().interval, Duration::from_secs(150));
        h.call("ui.prefs", json!({"open":"General"}));
        assert!(h.widget(h.app.tr("prefs.restore_page_defaults")).is_some(), "the translated Preferences caption is visible");
        assert!(h.widget(h.app.tr("ui.restore_defaults")).is_none(), "the generic caption is absent from Preferences");
        h.step(&json!({"widget":"Restore page defaults"})).unwrap();
        assert_eq!(h.app.autosave_minutes, 5.0);
        assert_eq!(h.app.autosave.as_ref().unwrap().interval, Duration::from_secs(300));
        assert_eq!(h.app.preferences.language, solidcraft_ui_text::SYSTEM);
        assert!(h.app.preferences.reopen_designs);
        assert_eq!(h.app.preferences.nav, "tinkercad", "General's reset leaves other pages alone");
        assert_eq!(h.app.settings_profiles.list, profiles);
        assert_eq!(h.app.settings_profiles.active.as_deref(), Some("Working"));
        h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
        assert_eq!(h.app.preferences.language, "en");
        assert!(!h.app.preferences.reopen_designs);
        assert_eq!(h.app.autosave_minutes, 2.5);
        assert_eq!(h.app.autosave.as_ref().unwrap().interval, Duration::from_secs(150));
    }

    /// Backend edits are saved before restart, with one original recovery choice across edits.
    #[test]
    fn graphics_backend_selection_preserves_recovery_and_other_settings() {
        let dir = std::env::temp_dir().join(format!("solidcraft-backend-choice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"backend":"auto","msaa":4,"adapter":"Named GPU","future":{"keep":true}}"#).unwrap();
        let services = crate::Services { graphics_path: Some(path.to_string_lossy().into()), ..Default::default() };
        let mut app = SolveApp::new(Default::default(), services);
        let read = || serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let snapshot = Snapshot::take(&app);
        let wanted = if cfg!(target_os = "macos") { "metal" } else { "vulkan" };
        set_graphics_backend(&mut app, wanted).unwrap();
        assert_eq!(app.preferences.graphics_backend, wanted);
        assert_eq!(read()["backend_trial"], json!({"previous":"auto","requested":wanted,"started":false}));
        assert_eq!(read()["future"], json!({"keep":true}));
        assert_eq!(read()["adapter"], "Named GPU");
        assert_eq!(read()["msaa"], 4);
        // A second untried choice must recover to Automatic, not to the first untried API.
        save_graphics_backend(&app, "gl").unwrap();
        app.preferences.graphics_backend = "gl".into();
        set(&mut app, &json!({"msaa":2})).unwrap();
        assert_eq!(read()["backend_trial"], json!({"previous":"auto","requested":"gl","started":false}));
        assert_eq!(read()["backend"], "gl");
        assert_eq!(read()["msaa"], 2);
        snapshot.restore_other_preferences(&mut app).unwrap();
        assert_eq!(app.preferences.graphics_backend, "auto");
        assert_eq!(read()["backend"], "auto");
        assert!(read().get("backend_trial").is_none());
        set_graphics_backend(&mut app, wanted).unwrap();
        defaults(&mut app, PAGE_GRAPHICS);
        assert_eq!(app.preferences.graphics_backend, "auto");
        assert_eq!(read()["backend"], "auto");
        assert!(read().get("backend_trial").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A manual legacy backend list survives unrelated edits and remains the recovery choice.
    #[test]
    fn graphics_backend_legacy_choice_is_preserved_until_explicitly_changed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-backend-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graphics.json");
        std::fs::write(&path, r#"{"backend":"dx12,vulkan","msaa":4}"#).unwrap();
        let services = crate::Services { graphics_path: Some(path.to_string_lossy().into()), ..Default::default() };
        let mut app = SolveApp::new(Default::default(), services);
        app.preferences.graphics_backend = "dx12,vulkan".into();
        let read = || serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        set(&mut app, &json!({"graphics_adapter":"Named GPU"})).unwrap();
        assert_eq!(read()["backend"], "dx12,vulkan");
        assert!(read().get("backend_trial").is_none());
        let snapshot = Snapshot::take(&app);
        set_graphics_backend(&mut app, "auto").unwrap();
        assert_eq!(read()["backend_trial"]["previous"], "dx12,vulkan");
        snapshot.restore_other_preferences(&mut app).unwrap();
        assert_eq!(read()["backend"], "dx12,vulkan");
        assert!(read().get("backend_trial").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Failed writes or invalid API choices cannot update the UI or discard existing settings.
    #[test]
    fn graphics_backend_save_failure_and_invalid_choice_leave_preferences_unchanged() {
        let dir = std::env::temp_dir().join(format!("solidcraft-backend-write-failure-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graphics.json");
        std::fs::create_dir_all(&path).unwrap();
        let services = crate::Services { graphics_path: Some(path.to_string_lossy().into()), ..Default::default() };
        let mut app = SolveApp::new(Default::default(), services);
        let before = app.preferences.clone();
        let wanted = if cfg!(target_os = "macos") { "metal" } else { "vulkan" };
        assert!(set_graphics_backend(&mut app, wanted).is_err());
        assert!(set(&mut app, &json!({"graphics_backend":wanted,"theme":"light"})).is_err());
        assert_eq!(app.preferences, before);
        assert!(!app.quit_requested);
        std::fs::remove_dir(&path).unwrap();
        let damaged = b"{broken graphics settings";
        std::fs::write(&path, damaged).unwrap();
        assert!(set_graphics_backend(&mut app, wanted).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), damaged);
        assert_eq!(app.preferences, before);
        std::fs::write(&path, b"{}").unwrap();
        for bad in [json!("glide"), json!("dx12,vulkan"), json!(""), json!(null), json!(7)] {
            assert!(set(&mut app, &json!({"graphics_backend":bad})).is_err());
            assert_eq!(app.preferences, before);
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The visible picker uses the same saved preference as ui.prefs and Cancel restores it.
    #[test]
    fn graphics_backend_picker_selects_and_cancels_without_requiring_commands() {
        let dir = std::env::temp_dir().join(format!("solidcraft-backend-picker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graphics.json");
        let mut h = crate::scenario::Harness::new();
        h.app.services.graphics_path = Some(path.to_string_lossy().into());
        h.app.services.graphics_backend = Some(if cfg!(target_os = "macos") { "metal" } else { "vulkan" });
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:graphics_backend");
        assert_eq!(h.app.preferences.graphics_backend, "auto");
        h.step(&json!({"click":{"handle":"prefs:graphics_backend"}})).unwrap();
        let wanted = if cfg!(target_os = "macos") { "metal" } else { "vulkan" };
        h.step(&json!({"click":{"handle":format!("dialog:list:graphics_api:{wanted}")}})).unwrap();
        assert_eq!(h.app.preferences.graphics_backend, wanted);
        let saved = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["backend"], wanted);
        assert_eq!(saved["backend_trial"]["previous"], "auto");
        assert_eq!(lenient(&serde_json::to_value(&h.app.preferences).unwrap()).graphics_backend, wanted);
        assert_eq!(h.call("ui.prefs", json!({"button":"cancel"}))["ok"], true);
        assert_eq!(h.app.preferences.graphics_backend, "auto");
        let saved = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["backend"], "auto");
        assert!(saved.get("backend_trial").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn graphics_backend_restart_control_refuses_an_override_or_missing_host_command() {
        let dir = std::env::temp_dir().join(format!("solidcraft-backend-restart-refusal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graphics.json");
        let original = br#"{"backend":"auto","future":{"keep":true}}"#;
        std::fs::write(&path, original).unwrap();
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.services.graphics_path = Some(path.to_string_lossy().into_owned());
        let wanted = if cfg!(target_os = "macos") { "metal" } else { "vulkan" };
        let before = app.preferences.clone();
        let combined = json!({"graphics_backend":wanted,"theme":"light","button":"restart_graphics"});
        assert!(control(&mut app, &combined).is_err());
        assert_eq!(app.preferences, before, "a rejected combined request changes no preferences");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!app.quit_requested);
        app.services.graphics_restart = Some(("solidcraft".into(), Vec::new()));
        app.services.graphics_backend_override = true;
        assert!(control(&mut app, &combined).is_err());
        assert_eq!(app.preferences, before, "an override refuses before saving the new API");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!app.quit_requested);
        assert!(crate::update::take_launch(&mut app).is_none());
        assert_eq!(Prefs::default().graphics_backend, "auto");
        assert_eq!(lenient(&json!({"msaa":4})).graphics_backend, "auto");
        std::fs::remove_dir_all(dir).unwrap();
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
        snapshot.restore_other_preferences(&mut app).unwrap();
        assert_eq!(read()["adapter"], "");
        set(&mut app, &json!({"graphics_adapter":"Discrete small"})).unwrap();
        defaults(&mut app, PAGE_GRAPHICS);
        assert_eq!(read()["adapter"], "");
        assert_eq!(read()["backend"], "vulkan");
        assert!(set(&mut app, &json!({"graphics_adapter":"x".repeat(1025)})).is_err());
        assert!(set(&mut app, &json!({"graphics_adapter":"card\nname"})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The settings `SolveApp::prefs` would write right now, parsed.
    fn saved_prefs(app: &SolveApp) -> serde_json::Value {
        serde_json::from_str(&app.prefs()).unwrap()
    }

    #[test]
    fn refused_portable_layout_still_saves_other_settings_without_replacing_stored_recovery() {
        use solidcraft_engine::panels::{FIXTURE_LAYOUT_V1, MAX_ENVELOPE_BYTES, PanelRegistry, commands::CatalogueState};

        // A maximum-sized original v1 record is readable, but projecting its native
        // window to portable v2 can exceed the envelope cap. Keep that real source.
        let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
        let native = json!({"type":"window","host":"legacy.window","origin":{"x":1900.0,"y":900.0},
            "return_to":{"zone":"left","index":0,"size":{"width":280.0,"height":240.0},"collapsed":false}});
        raw["layout"]["sections"][0]["location"] = native.clone();
        raw["layout"]["instances"][0]["memory"]["location"] = native;
        raw["retained_data"]["padding"] = json!("");
        let overhead = serde_json::to_vec(&raw).unwrap().len();
        raw["retained_data"]["padding"] = json!("x".repeat(MAX_ENVELOPE_BYTES - overhead));
        let envelope = serde_json::to_vec(&raw).unwrap();
        assert_eq!(envelope.len(), MAX_ENVELOPE_BYTES);
        fn bytes(out: &mut Vec<u8>, value: &[u8]) {
            out.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
            out.extend_from_slice(value);
        }
        let mut catalogue = b"SCPCAT01".to_vec();
        catalogue.extend_from_slice(&1_u32.to_le_bytes()); // One immutable preset.
        bytes(&mut catalogue, b"Legacy");
        bytes(&mut catalogue, &envelope);
        catalogue.push(0); // No rejected envelope.
        catalogue.extend_from_slice(&0_u32.to_le_bytes()); // No user presets.
        catalogue.extend_from_slice(&1_u32.to_le_bytes());
        for text in ["fixture.workspace", "ui.panel", "Legacy"] {
            bytes(&mut catalogue, text.as_bytes());
        }
        catalogue.extend_from_slice(&1_u32.to_le_bytes());
        bytes(&mut catalogue, b"fixture.workspace");
        bytes(&mut catalogue, &envelope);
        catalogue.push(0);
        CatalogueState::hydrate(&catalogue, &PanelRegistry::default()).unwrap();
        let refused = json!({"version":1,"selected":"fixture.workspace",
            "catalogue":solidcraft_engine::doc::canvas::base64_encode(&catalogue)});
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.load_prefs(&json!({"panel_layout":refused}).to_string());
        app.ui.show_grid = false;
        assert!(app.try_prefs().is_err());
        let recovery = json!({"version":99,"library":["saved layout"],"rejected":{"source":"exact recovery"}});
        let previous = json!({"grid":true,"panel_layout":recovery}).to_string();
        let fallback: serde_json::Value = serde_json::from_str(&app.prefs_with_stored_panel_layout(Some(&previous)).unwrap()).unwrap();
        assert_eq!(fallback["grid"], false);
        assert_eq!(fallback["panel_layout"], recovery);
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(refused.clone()));
        control(&mut app, &json!({"open":"Layout"})).unwrap();
        let held = held_panel_layout(&app);
        app.prefs_with_stored_panel_layout(Some(&previous)).unwrap();
        assert_eq!(held_panel_layout(&app), held, "fallback cannot replace the Cancel baseline");
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(refused));
    }

    #[test]
    fn stored_panel_fallback_preserves_null_absence_and_refuses_malformed_previous_settings() {
        let app = SolveApp::new(Default::default(), Default::default());
        for previous in [None, Some("{}"), Some("{\"grid\":false}")] {
            let saved: serde_json::Value = serde_json::from_str(&app.prefs_with_stored_panel_layout(previous).unwrap()).unwrap();
            assert!(saved.get("panel_layout").is_none());
        }
        let saved: serde_json::Value = serde_json::from_str(&app.prefs_with_stored_panel_layout(Some("{\"panel_layout\":null}")).unwrap()).unwrap();
        assert_eq!(saved.get("panel_layout"), Some(&serde_json::Value::Null));
        for previous in ["{", "null", "[]", "42"] {
            assert!(app.prefs_with_stored_panel_layout(Some(previous)).is_err());
        }
    }

    #[test]
    fn deferred_panel_layout_records_follow_preferences_apply_cancel_and_held_writes() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let original = json!({"version":99,"future":{"layout":"keep these bytes"}});
        let changed = json!({"version":100,"future":{"layout":"another record"}});
        app.load_prefs(&json!({"panel_layout":original}).to_string());
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(original.clone()));
        control(&mut app, &json!({"open":"Layout"})).unwrap();
        app.load_prefs(&json!({"panel_layout":changed}).to_string());
        assert!(prefs_window::apply_pending(&app));
        assert_eq!(saved_prefs(&app)["panel_layout"], original, "unapplied recovery data cannot replace the saved record");
        prefs_window::close(&mut app, true);
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(original));
        assert!(!app.prefs_window.shell.open);

        control(&mut app, &json!({"open":"Layout"})).unwrap();
        app.load_prefs(&json!({"panel_layout":changed}).to_string());
        control(&mut app, &json!({"button":"apply"})).unwrap();
        assert_eq!(saved_prefs(&app)["panel_layout"], changed, "Apply establishes the next saved baseline");
        assert!(!prefs_window::apply_pending(&app));
        defaults(&mut app, PAGE_LAYOUT);
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(changed), "a deferred future layout is not erased by Classic page reset");
    }

    #[test]
    fn layout_page_defaults_preserve_arrangements_and_cancel_restores_the_library() {
        use crate::panel_runtime;
        use solidcraft_engine::panels::{LayoutAction, Mutation};

        let mut h = crate::scenario::Harness::new();
        h.call("ui.panel_fixture", json!({"preset":"docks","remember":true}));
        h.frames(2);
        h.app.run_panel_command("layout.save", &json!({"name":"Original"})).unwrap();
        let baseline = panel_runtime::preferences_value(&h.app);
        control(&mut h.app, &json!({"open":"Layout"})).unwrap();
        let section = h.app.panel_runtime().unwrap().layout().layout().sections.first().unwrap().id.clone();
        h.app
            .dispatch_panel_layout(LayoutAction::Apply { expected_revision: None, mutations: vec![Mutation::Collapse { section, collapsed: true }] })
            .unwrap();
        h.app.run_panel_command("layout.save", &json!({"name":"Original","replace":true})).unwrap();
        h.app.run_panel_command("layout.save", &json!({"name":"Temporary"})).unwrap();
        h.app.run_panel_command("layout.workspace.select", &json!({"workspace":"panel_fixture.workspace.review"})).unwrap();
        let edited = panel_runtime::preferences_value(&h.app);
        defaults(&mut h.app, PAGE_LAYOUT);
        assert_eq!(panel_runtime::preferences_value(&h.app), edited, "page defaults leave all panel arrangements and saved layouts alone");
        prefs_window::close(&mut h.app, true);
        assert_eq!(
            panel_runtime::preferences_value(&h.app),
            baseline,
            "Cancel restores selection, every arrangement, and both new and replaced layouts"
        );
        assert!(!h.app.prefs_window.shell.open);
    }

    #[test]
    fn failed_layout_cancel_keeps_the_baseline_until_the_host_can_restore_it() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.panel_fixture", json!({"preset":"overflow","remember":true}));
        h.frames(2);
        control(&mut h.app, &json!({"open":"Layout"})).unwrap();
        let baseline = held_panel_layout(&h.app);
        let original_host = h.app.panel_runtime.take();
        let mut smaller = crate::scenario::Harness::new();
        smaller.call("ui.panel_fixture", json!({"preset":"docks","remember":true}));
        smaller.frames(2);
        h.app.panel_runtime = smaller.app.panel_runtime.take();
        let current = crate::panel_runtime::preferences_value(&h.app);
        for button in ["cancel", "apply", "ok"] {
            control(&mut h.app, &json!({"button":button})).unwrap();
            assert!(h.app.prefs_window.shell.open);
            assert!(h.app.prefs_window.shell.restore_error().is_some());
            assert_eq!(held_panel_layout(&h.app), baseline, "a refused restore cannot replace the baseline");
            assert_eq!(crate::panel_runtime::preferences_value(&h.app), current, "failed admission changes no live state");
        }
        h.app.panel_runtime = original_host;
        prefs_window::close(&mut h.app, true);
        assert!(!h.app.prefs_window.shell.open);
        assert!(h.app.prefs_window.shell.snapshot().is_none());
        assert!(h.app.prefs_window.shell.restore_error().is_none());
    }

    #[test]
    fn panel_workspace_arrows_follow_a_mouse_click_without_turning_the_view() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.panel_fixture", json!({"preset":"docks","remember":true}));
        h.call("ui.prefs", json!({"open":"Layout","language":"en"}));
        h.frames(3);
        let camera = h.app.cam;
        h.step(&json!({"click":{"handle":"dialog:Layout workspace:Modeling"}})).unwrap();
        h.step(&json!({"key":"ArrowRight"})).unwrap();
        h.frames(2);
        assert_eq!(crate::panel_runtime::layout_preferences_view(&h.app).unwrap().selected_workspace.0, "panel_fixture.workspace.sketch");
        assert_eq!(h.app.cam, camera, "workspace arrows belong to Preferences after a pointer click");
    }

    #[test]
    fn panel_save_popover_requires_explicit_replace_and_owns_the_first_escape() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.panel_fixture", json!({"preset":"docks","remember":true}));
        h.call("ui.prefs", json!({"open":"Layout","language":"en"}));
        h.frames(3);
        h.step(&json!({"click":{"handle":"prefs:panel_layout:save_as"}})).unwrap();
        h.frames(2);
        h.step(&json!({"text":"Bench setup"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        assert_eq!(crate::panel_runtime::user_preset_names(&h.app), ["Bench setup"]);
        h.step(&json!({"click":{"handle":"prefs:panel_layout:save_as"}})).unwrap();
        h.frames(2);
        h.step(&json!({"text":"bench setup"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        assert_eq!(crate::panel_runtime::user_preset_names(&h.app), ["Bench setup"], "duplicate Enter never silently replaces or adds a layout");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.prefs_window.shell.open, "the first Escape belongs to the save popover");
        assert_eq!(crate::panel_runtime::user_preset_names(&h.app), ["Bench setup"]);
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(!h.app.prefs_window.shell.open);
        assert!(crate::panel_runtime::user_preset_names(&h.app).is_empty(), "Preferences Cancel undoes the saved layout");
    }

    #[test]
    fn full_panel_library_wraps_and_keyboard_reaches_the_save_tile() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1100}));
        h.call("ui.panel_fixture", json!({"preset":"docks","remember":true}));
        for index in 0..64 {
            h.app.run_panel_command("layout.save", &json!({"name":format!("Saved {index:02}")})).unwrap();
        }
        h.call("ui.prefs", json!({"open":"Layout","language":"en"}));
        h.frames(3);
        let first = crate::scenario::rect_of("prefs:panel_layout:tile:Saved 00").unwrap();
        let later = crate::scenario::rect_of("prefs:panel_layout:tile:Saved 10").unwrap();
        assert!(later.top() > first.top(), "the grid wraps into rows rather than placing every row horizontally");
        h.step(&json!({"click":{"handle":"prefs:panel_layout:tile:Saved 00"}})).unwrap();
        h.step(&json!({"key":"End"})).unwrap();
        h.frames(20);
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:panel_layout:save_popover").is_some());
        h.step(&json!({"click":{"handle":"prefs:panel_layout:replace"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("prefs:panel_layout:replace_confirm").is_some(), "a full library still requires explicit confirmation");
        assert_eq!(crate::panel_runtime::user_preset_names(&h.app).len(), 64);
    }

    #[test]
    fn absent_panel_layout_is_held_as_absent_and_cancel_restores_absence() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        control(&mut app, &json!({"open":"Layout"})).unwrap();
        app.load_prefs(&json!({"panel_layout":{"version":99,"future":"retained"}}).to_string());
        assert!(crate::panel_runtime::preferences_value(&app).is_some());
        assert!(saved_prefs(&app).get("panel_layout").is_none(), "the held overlay must remove a newly introduced record");
        prefs_window::close(&mut app, true);
        assert!(crate::panel_runtime::preferences_value(&app).is_none());
        assert!(saved_prefs(&app).get("panel_layout").is_none());
    }

    #[test]
    fn present_null_panel_record_stays_present_during_preferences_transaction() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.load_prefs(&json!({"panel_layout":null}).to_string());
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(serde_json::Value::Null));
        control(&mut app, &json!({"open":"Layout"})).unwrap();
        app.load_prefs(&json!({"panel_layout":{"version":99,"future":"retained"}}).to_string());
        let held = saved_prefs(&app);
        assert_eq!(
            held.get("panel_layout"),
            Some(&serde_json::Value::Null),
            "a corrupt present record is not absence or permission for legacy import"
        );
        prefs_window::close(&mut app, true);
        assert_eq!(crate::panel_runtime::preferences_value(&app), Some(serde_json::Value::Null));
    }

    #[test]
    fn cancel_keeps_the_original_deferred_legacy_import_with_or_without_a_temporary_catalogue() {
        use crate::panel_runtime;
        use solidcraft_engine::panels::{InstanceId, LogicalRect, Point, Size};

        for enroll in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.load_prefs(&json!({"preferences":{"browser_width":333.0}}).to_string());
            control(&mut app, &json!({"open":"Layout"})).unwrap();
            app.load_prefs(&json!({"preferences":{"browser_width":777.0}}).to_string());
            app.preferences.browser_width = 333.0;
            assert!(prefs_window::apply_pending(&app), "deferred import changes count even when ordinary preferences match");

            let capacity = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1600.0, height: 1000.0 } };
            if enroll {
                panel_runtime::enable_host_specimen(&mut app, "docks").unwrap();
                panel_runtime::enable_preferences(&mut app).unwrap();
                app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, capacity).unwrap();
            }
            prefs_window::close(&mut app, true);
            assert!(!app.prefs_window.shell.open);
            assert!(panel_runtime::preferences_value(&app).is_none());
            assert!(!panel_runtime::preferences_enabled(&app));

            if !enroll {
                panel_runtime::enable_host_specimen(&mut app, "docks").unwrap();
            }
            panel_runtime::enable_preferences(&mut app).unwrap();
            app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, capacity).unwrap();
            assert_eq!(
                panel_runtime::preferences_value(&app).unwrap()["legacy"]["browser_width"],
                json!(333.0),
                "Cancel retains the original legacy sidecar before migration"
            );
            let browser = InstanceId::new("specimen-0").unwrap();
            panel_runtime::migrate_legacy_placements(
                &mut app,
                &[panel_runtime::LegacyPanelBinding { role: panel_runtime::LegacyPanelRole::Browser, instance: browser.clone() }],
            )
            .unwrap();
            let layout = app.panel_runtime().unwrap().layout().layout();
            let section = layout.containing(&browser).unwrap();
            assert_eq!(layout.column(section.column.as_ref().unwrap()).unwrap().width, 333.0, "Cancel restores the original placement import");
        }
    }

    #[test]
    fn unapplied_changes_are_not_saved_while_the_dialog_is_open() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert_eq!(saved_prefs(&app)["preferences"]["theme"], "dark");
        assert_eq!(saved_prefs(&app)["preferences"]["sketch_tab_always"], false);
        // Open the dialog: the snapshot is taken as `show` and the control channel take it.
        app.prefs_window.shell.open = true;
        prefs_window::hold(&mut app);
        // The Light tile and the Sketch tab box, clicked without Apply.
        set(&mut app, &json!({"theme": "light", "sketch_tab_always": true})).unwrap();
        assert_eq!(app.preferences.theme, "light", "the live app has the change");
        assert!(!app.ui.dark);
        let held = saved_prefs(&app);
        assert_eq!(held["preferences"]["theme"], "dark", "a change that was not applied must not be saved");
        assert_eq!(held["preferences"]["sketch_tab_always"], false);
        assert_eq!(held["dark"], true, "the Light tile's dark flag is not saved either");
        // Apply makes the change the baseline: from now on it is saved.
        prefs_window::press(&mut app, WINDOW_PAGES, prefs_window::Button::Apply).unwrap();
        let applied = saved_prefs(&app);
        assert_eq!(applied["preferences"]["theme"], "light");
        assert_eq!(applied["dark"], false);
        // A second unapplied change is held back too, and Cancel puts it back.
        set(&mut app, &json!({"theme": "dark"})).unwrap();
        assert_eq!(saved_prefs(&app)["preferences"]["theme"], "light", "the second change waits for Apply too");
        prefs_window::close(&mut app, true);
        let after = saved_prefs(&app);
        assert_eq!(after["preferences"]["theme"], "light", "Cancel restores the applied state");
        assert_eq!(after["dark"], false);
        assert_eq!(after["preferences"]["sketch_tab_always"], true, "Apply's change stays");
    }

    #[test]
    fn settings_the_dialog_cannot_change_keep_saving_while_it_is_open() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.prefs_window.shell.open = true;
        prefs_window::hold(&mut app);
        // A recent file joins while the dialog is open: not a dialog setting, so it is saved.
        app.home.recent.insert(0, "bracket.solid".to_string());
        assert_eq!(saved_prefs(&app)["recent"][0], "bracket.solid");
    }

    #[test]
    fn shortcut_changes_are_not_saved_while_the_dialog_is_open() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.keymap.custom.insert("file.new".into(), vec!["Ctrl+N".into()]);
        app.prefs_window.shell.open = true;
        prefs_window::hold(&mut app);
        app.keymap.custom.insert("file.save".into(), vec!["Ctrl+S".into()]);
        let saved = saved_prefs(&app);
        assert_eq!(saved["shortcuts"]["file.new"], "Ctrl+N", "the shortcut from before the dialog is saved");
        assert!(saved["shortcuts"].get("file.save").is_none(), "a shortcut rebound without Apply is not saved");
    }

    #[test]
    fn control_channel_holds_unapplied_changes_out_of_the_saved_settings() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open": "Layout"}));
        h.call("ui.prefs", json!({"sketch_tab_always": true}));
        let saved = saved_prefs(&h.app);
        assert!(h.app.preferences.sketch_tab_always);
        assert_eq!(saved["preferences"]["sketch_tab_always"], false, "the control channel's change waits for Apply");
        h.call("ui.prefs", json!({"button": "apply"}));
        assert_eq!(saved_prefs(&h.app)["preferences"]["sketch_tab_always"], true, "Apply makes it the saved state");
        h.call("ui.prefs", json!({"sketch_tab_always": false}));
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(saved_prefs(&h.app)["preferences"]["sketch_tab_always"], true, "Cancel puts back what Apply left");
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
            let _de = solidcraft_ui_text::scope("de");
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

    /// "Let AI agents connect" is off for a new install and for preferences saved before it existed;
    /// an explicit on stays on.
    #[test]
    fn agent_control_is_off_unless_saved_on() {
        assert!(!Prefs::default().agent_control);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.preferences.agent_control = true;
        app.load_prefs(&serde_json::json!({ "preferences": { "msaa": 4 } }).to_string());
        assert!(!app.preferences.agent_control);
        app.load_prefs(&serde_json::json!({ "preferences": { "agent_control": true } }).to_string());
        assert!(app.preferences.agent_control);
    }

    /// "Let AI agents connect" and its port sit in Agents through MCP, there with the chat off
    /// too; the port is 27878 unless saved otherwise, and Restore Defaults puts it back.
    #[test]
    fn agent_control_and_its_port_are_in_agents_through_mcp() {
        use serde_json::json;
        assert_eq!(Prefs::default().agent_control_port, 27878);
        let mut h = crate::scenario::Harness::new();
        h.app.load_prefs(&json!({ "preferences": { "agent_control_port": 7999 } }).to_string());
        assert_eq!(h.app.preferences.agent_control_port, 7999);
        h.call("ui.prefs", json!({"ai_features": false, "open": "AI"}));
        h.frames(3);
        assert!(crate::scenario::handle_at("prefs:agent_control").is_some(), "the checkbox is on the AI page with the chat off");
        assert!(crate::scenario::handle_at("prefs:agent_control_port").is_some(), "so is the port");
        h.call("ui.prefs", json!({"agent_control_port": 8123}));
        assert_eq!(h.app.preferences.agent_control_port, 8123);
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(h.app.preferences.agent_control_port, 27878);
        assert!(!h.app.preferences.agent_control, "Restore Defaults turns it off");
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
        assert_eq!(restored.preferences.navigator_style, NavigatorStyle::Axes, "a change that was not applied is not saved (#9730)");
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
        // Use the harness clock and consume its texture updates like every other frame.
        h.frame();
        h.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The shipped ribbon uses the same localized tab names as workspace metadata.
    #[test]
    fn ribbon_names_the_tabs_as_workspace_metadata_does() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":2400,"height":1000}));
        for (lang, want) in [
            ("en", ["Model", "Surface", "Mesh", "Form", "Sheet Metal", "Render"]),
            ("de", ["Modell", "Fläche", "Mesh", "Form", "Blech", "Rendern"]),
            ("fr", ["Modèle", "Surface", "Maillage", "Forme", "Tôlerie", "Rendu"]),
        ] {
            h.app.preferences.language = lang.into();
            h.frames(3);
            let _scope = solidcraft_ui_text::scope(lang);
            for (id, name) in ["MODEL", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER"].into_iter().zip(want) {
                assert_eq!(crate::workspace::tab_name(id), name, "{lang}");
                assert_eq!(crate::toolbar::ribbon_label(&h.app, id), name, "{lang}");
            }
            let mut painted = Vec::new();
            for shape in &h.shapes {
                crate::test_support::each_shape(&shape.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        painted.push(text.galley.job.text.clone());
                    }
                });
            }
            for id in crate::workspace::visible_tabs(&h.app) {
                let name = crate::toolbar::ribbon_label(&h.app, id);
                assert!(crate::scenario::handle_at(&format!("tab:{id}")).is_some(), "{lang}: real ribbon tab {id}");
                assert!(painted.contains(&name), "{lang}: the ribbon paints {painted:?}, not {name}");
            }
        }
    }

    /// Origin plane names (#6491): Top, Front and Right are names shown in the browser, the
    /// dialogs' chips and the plane choices; the planes' ids, the commands and the design keep XY,
    /// XZ and YZ. Saved, refused when unknown, and taken back by Cancel and Restore Defaults.
    #[test]
    fn origin_plane_names_change_only_the_names_shown() {
        crate::i18n::install();
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
        h.call("ui.prefs", json!({"open":"Design"}));
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
        h.call("ui.prefs", json!({"open":"Design","origin_plane_names":"xyz"}));
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.origin_plane_names, TopFrontRight, "Cancel takes it back");
        h.call("ui.prefs", json!({"open":"Design"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert_eq!(h.app.preferences.origin_plane_names, Xyz, "Restore Defaults: XY, XZ and YZ");
        h.frames(2);
        assert!(OriginPlaneNames::PLANES.iter().all(|p| rows().iter().any(|r| r == p)), "{:?}", rows());
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences.origin_plane_names, TopFrontRight);
    }

    /// The snap-suppress choice and Direct interaction read in the user's language: the classic
    /// choice's "or" was English in every language, and Direct interaction had no message.
    #[test]
    fn modifier_choice_and_direct_interaction_are_translated() {
        crate::i18n::install();
        let _japanese = solidcraft_ui_text::scope("ja");
        let either = crate::tool_modifiers::ModifierCombo::CTRL_ALT.shown_label();
        assert!(either.contains(" または ") && !either.contains(" or "), "{either}");
        assert_ne!(crate::tool_modifiers::ModifierCombo::NONE.shown_label(), "None");
        for english in [
            "Direct interaction",
            "Work on the model directly in the viewport. Press a flat face or a profile and pull: up extrudes, down into a body cuts, and sideways movement sets the step size",
        ] {
            assert_ne!(solidcraft_ui_text::tr_label(english), english);
        }
    }

    #[test]
    fn panel_content_and_toolbar_choices_preserve_transactions_in_both_themes() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1200,"height":1100}));
            h.call("ui.prefs", json!({"theme":theme,"quick_measure":true,"invert_curved_roll_arrows":true}));
            h.call("ui.prefs", json!({"open":"Layout"}));
            let before = h.app.preferences.clone();
            let doc = h.app.session.doc.to_json();
            assert!(before.live_panel_drag_preview);
            scroll_to_and_click(&mut h, "prefs:live_panel_drag_preview");
            assert!(!h.app.preferences.live_panel_drag_preview);
            scroll_to_and_click(&mut h, "prefs:history_in_browser");
            assert!(h.app.preferences.history_in_browser);
            for tab in crate::workspace::choosable_tabs() {
                scroll_to_and_click(&mut h, &format!("prefs:toolbar_tab:{tab}"));
                assert!(!crate::workspace::tab_turned_on(&h.app, tab));
                scroll_to_and_click(&mut h, &format!("prefs:toolbar_tab:{tab}"));
                assert!(crate::workspace::tab_turned_on(&h.app, tab));
            }
            assert_eq!(h.app.session.doc.to_json(), doc);
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences, before, "Cancel restores every Layout edit");
            h.call("ui.prefs", json!({"open":"Layout"}));
            scroll_to_and_click(&mut h, "prefs:live_panel_drag_preview");
            scroll_to_and_click(&mut h, "prefs:history_in_browser");
            h.call("ui.prefs", json!({"button":"apply"}));
            let mut restored = SolveApp::new(Default::default(), Default::default());
            restored.load_prefs(&h.app.prefs());
            assert!(!restored.preferences.live_panel_drag_preview);
            h.call("ui.prefs", json!({"button":"defaults"}));
            assert!(h.app.preferences.live_panel_drag_preview);
            assert!(!h.app.preferences.history_in_browser);
            assert!(h.app.preferences.quick_measure && h.app.preferences.invert_curved_roll_arrows, "Layout reset retains Graphics and Navigation");
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert!(h.app.preferences.history_in_browser, "Cancel after Apply returns to the applied state");
            assert!(!h.app.preferences.live_panel_drag_preview);
        }
    }

    #[test]
    fn history_preferences_widget_click_reveals_scrolled_content_and_keeps_folds_closed() {
        let steps: serde_json::Value = serde_json::from_str(include_str!("../tests/scenarios/history_sketches_under_feature.json")).unwrap();
        let steps = steps.as_array().unwrap();
        let transition = steps.iter().position(|step| step["params"]["history_sketches"] == "under_feature").unwrap();
        let mut h = crate::scenario::Harness::new();
        for step in steps.iter().take(transition) {
            h.step(step).unwrap();
        }
        h.call("ui.prefs", json!({"open":"Layout"}));
        let page = crate::scenario::rect_of("prefs:page_body").unwrap();
        h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-10000}));
        h.frames(60);
        let at = h.widget("Under their feature").unwrap();
        let page = crate::scenario::rect_of("prefs:page_body").unwrap();
        assert!(!page.contains(egui::pos2(at[0] as f32, at[1] as f32)), "the real control begins scrolled out of view");
        assert!((at[1] as f32) < page.top(), "scrolling to the bottom puts the target above the viewport");
        assert_eq!(h.app.preferences.history_sketches, HistorySketches::InOrder);
        h.step(&json!({"widget":"Under their feature", "near":"Sketches in History"})).unwrap();
        assert_eq!(h.app.preferences.history_sketches, HistorySketches::UnderFeature);
        let at = h.widget("Under their feature").unwrap();
        assert!(crate::scenario::rect_of("prefs:page_body").unwrap().contains(egui::pos2(at[0] as f32, at[1] as f32)));
        assert!(h.app.tree.expanded.is_empty(), "revealing a preference does not open history folds");
        let page_before = crate::scenario::rect_of("prefs:page_body");
        h.step(&json!({"widget":"Apply"})).unwrap();
        assert!(h.app.prefs_window.shell.open);
        assert_eq!(crate::scenario::rect_of("prefs:page_body"), page_before);
        h.step(&json!({"widget":"OK"})).unwrap();
        h.step(&steps[transition + 1]).unwrap();
        assert!(!h.app.prefs_window.shell.open);
        for step in steps.iter().skip(transition + 2) {
            h.step(step).unwrap();
        }
        h.call("ui.prefs", json!({"open":"Layout"}));
        h.step(&json!({"widget":"Navigation"})).unwrap();
        assert_eq!(h.app.prefs_window.shell.section, PAGE_NAVIGATION, "sidebar navigation remains a real click");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(!h.app.prefs_window.shell.open);
    }

    #[test]
    fn history_sketches_defaults_in_order_and_follows_preferences_transactions() {
        let old: Prefs = serde_json::from_value(json!({"timeline_left": true})).unwrap();
        assert_eq!(old.history_sketches, HistorySketches::InOrder, "designs and settings from before keep the creation order");
        assert_eq!(lenient(&json!({"history_sketches": "sideways"})).history_sketches, HistorySketches::InOrder);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        h.call("ui.prefs", json!({"open": "Layout"}));
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
        h.call("ui.prefs", json!({"open": "Layout"}));
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
        h.call("ui.prefs", json!({"open": "Layout"}));
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
        h.call("ui.prefs", json!({"open": "Layout"}));
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert_eq!(h.app.preferences.dialog_buttons, DialogButtons::Bottom);
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert_eq!(h.app.preferences.dialog_buttons, DialogButtons::Top);
    }

    #[test]
    fn navigation_legend_follows_all_presets_and_runtime_options_without_mutating_them() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        for preset in NAV_PRESETS {
            choose_nav_preset(&mut app.preferences, preset);
            let before = app.preferences.clone();
            let rows = navigation::mapping_rows(&app);
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|(keys, action)| !keys.is_empty() && !action.is_empty()));
            assert_eq!(app.preferences, before);
            if preset.id == "creo" {
                assert!(preset.reverse_drag_zoom);
                assert_eq!(preset.mode(NavButton::Middle, Modifiers::NONE), Some(NavMode::Orbit));
                assert!(
                    rows.iter()
                        .any(|(keys, action)| keys.iter().any(|key| key == "Ctrl/Cmd") && action == solidcraft_ui_text::tr("prefs.gesture_zoom"))
                );
            }
            if preset.id == TOUCHPAD_NAV {
                assert!(rows.iter().any(|(keys, action)| keys == &[solidcraft_ui_text::tr("prefs.shift").to_owned()]
                    && action == solidcraft_ui_text::tr("prefs.gesture_pan")));
                assert!(
                    rows.iter().any(|(keys, action)| keys == &["Alt/Option".to_owned()] && action == solidcraft_ui_text::tr("prefs.gesture_orbit"))
                );
                assert!(!app.preferences.trackpad_pans());
            }
        }
        choose_nav_preset(&mut app.preferences, nav_preset(crate::mouse_gesture::MOUSE_MODE));
        app.preferences.mouse_gestures.enabled = true;
        let rows = navigation::mapping_rows(&app);
        assert!(rows.iter().any(|(_, action)| action == solidcraft_ui_text::tr("prefs.context_menu_ring")));
        assert!(!rows.iter().any(|(keys, action)| keys.contains(&solidcraft_ui_text::tr("prefs.button_right").to_owned())
            && action == solidcraft_ui_text::tr("prefs.gesture_orbit")));
    }

    #[test]
    fn navigation_dropdown_escape_retains_live_edits_then_cancels_the_snapshot() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1100}));
            h.call("ui.prefs", json!({"language":"en","theme":theme}));
            let before = h.app.preferences.clone();
            h.call("ui.prefs", json!({"open":"Navigation","zoom_reverse":true}));
            let edited = h.app.preferences.clone();
            scroll_to_and_click(&mut h, "prefs:nav");
            let popup = "dialog:list:creo";
            assert!(crate::scenario::handle_at(popup).is_some());
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.prefs_window.shell.open && h.app.prefs_window.shell.snapshot().is_some());
            assert_eq!(h.app.preferences, edited);
            assert!(crate::scenario::handle_at(popup).is_none());
            h.step(&json!({"key":"Enter"})).unwrap();
            h.frames(2);
            assert!(crate::scenario::handle_at(popup).is_some(), "Escape restored focus to the mapping trigger");
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(!h.app.prefs_window.shell.open);
            assert_eq!(h.app.preferences, before);
        }
    }

    #[test]
    fn orbit_picture_cards_keep_legacy_migration_and_preferences_transactions() {
        let legacy = migrate(serde_json::from_value::<Prefs>(json!({"orbit_cursor":true})).unwrap());
        assert_eq!(legacy.orbit_around, OrbitAround::Cursor);
        assert!(!legacy.orbit_cursor);
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1100}));
            h.call("ui.prefs", json!({"language":"en","theme":theme,"open":"Navigation"}));
            let camera = h.app.cam;
            for (key, value) in [("Origin", OrbitAround::Origin), ("View center", OrbitAround::ViewCentre), ("Under the cursor", OrbitAround::Cursor)]
            {
                scroll_to_and_click(&mut h, &format!("prefs:orbit_around:{key}"));
                assert_eq!(h.app.preferences.orbit_around, value);
                assert_eq!(h.app.cam, camera, "choosing the pivot does not turn the camera");
                assert!(!h.app.preferences.orbit_cursor);
            }
            h.call("ui.prefs", json!({"button":"apply"}));
            h.call("ui.prefs", json!({"button":"defaults"}));
            assert_eq!(h.app.preferences.orbit_around, OrbitAround::ViewCentre);
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences.orbit_around, OrbitAround::Cursor);
        }
    }

    #[test]
    fn orbit_cursor_wrap_defaults_on_and_can_be_turned_off() {
        let old: Prefs = serde_json::from_value(json!({"nav":"solidworks"})).unwrap();
        assert!(old.orbit_cursor_wrap, "saved preferences from before the option wrap");
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Navigation"}));
        h.step(&json!({"click":{"handle":"prefs:orbit_cursor_wrap"}})).unwrap();
        assert!(!h.app.preferences.orbit_cursor_wrap);
        h.call("ui.prefs", json!({"button":"apply"}));
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.orbit_cursor_wrap);
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.orbit_cursor_wrap);
        h.call("ui.prefs", json!({"orbit_cursor_wrap":false,"button":"apply"}));
        assert!(!h.app.preferences.orbit_cursor_wrap);
    }

    /// An agent's slow commands compute off the window thread unless a person turned that off:
    /// a settings file without the setting follows the default, one that has it keeps its value.
    #[test]
    fn agent_commands_compute_off_the_window_thread_by_default() {
        assert!(Prefs::default().agent_off_thread);
        let old: Prefs = serde_json::from_value(json!({"nav": "solidworks"})).unwrap();
        assert!(old.agent_off_thread, "a file from before the setting follows the default");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.agent_off_thread = false;
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.agent_off_thread, "a saved off stays off");
    }

    #[test]
    fn free_orbit_defaults_off_and_follows_preferences_transactions() {
        let old: Prefs = serde_json::from_value(json!({"nav":"solidworks"})).unwrap();
        assert!(!old.free_orbit);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Navigation"}));
        h.app.preferences.free_orbit_upright = true;
        h.frames(2);
        scroll_to_and_click(&mut h, "prefs:free_orbit_upright");
        assert!(h.app.preferences.free_orbit_upright, "the disabled upright control preserves its saved value");
        assert!(!h.app.preferences.free_orbit);
        scroll_to_and_click(&mut h, "prefs:free_orbit");
        assert!(h.app.preferences.free_orbit);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.free_orbit, "a change that was not applied is not saved (#9730)");
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
        assert!(!restored.preferences.radial_context_menu, "a change that was not applied is not saved (#9730)");
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
    fn single_surface_analysis_live_cancel_restores_policy_without_restarting_effects() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        super::control(&mut app, &json!({"single_surface_analysis":false})).unwrap();
        for (command, params) in [
            ("solid.box", json!({"length":10,"width":10,"height":10})),
            ("inspect.zebra", json!({"stripes":14.5})),
            ("inspect.section", json!({"plane":"XY"})),
            ("inspect.continuity", json!({"edges":[[5,0,0]],"samples":5})),
        ] {
            app.run(command, params).unwrap();
        }
        app.session.mark_saved();
        let doc = app.session.doc.clone();
        let undo = app.session.undo.len();
        assert_eq!(app.session.status_items().len(), 3);
        super::control(&mut app, &json!({"open":"Graphics"})).unwrap();
        super::control(&mut app, &json!({"single_surface_analysis":true})).unwrap();
        assert!(app.session.single_surface_analysis());
        assert_eq!(app.session.analysis, None, "turning on applies immediately and ends older shading");
        assert_eq!(app.session.status_items()[0].command, "inspect.continuity");
        super::control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(!app.preferences.single_surface_analysis && !app.session.single_surface_analysis());
        assert_eq!(app.session.analysis, None, "Cancel restores the policy, not ended view effects");
        assert_eq!(app.session.continuity.len(), 1);
        assert_eq!(app.session.status_items().len(), 2);
        app.run("inspect.zebra", json!({"stripes":14.5})).unwrap();
        assert!(app.session.analysis.is_some() && !app.session.continuity.is_empty());
        assert_eq!(app.session.status_items().len(), 3, "the restored legacy policy permits reactivation");
        assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
        assert!(!app.session.is_dirty());
        assert_eq!(app.session.undo.len(), undo);
    }

    /// Construct grouping is opt-in, survives Apply, and follows Layout Cancel and Defaults.
    #[test]
    fn construct_menu_groups_preference_persists_cancels_and_resets() {
        assert!(!Prefs::default().construct_menu_groups);
        assert!(!lenient(&json!({"theme":"light"})).construct_menu_groups);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Layout"}));
        super::reveal_preference(&mut h, "prefs:construct_menu_groups");
        let checkbox = crate::scenario::handle_at("prefs:construct_menu_groups").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(h.app.preferences.construct_menu_groups);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.construct_menu_groups);
        h.call("ui.prefs", json!({"open":"Layout","construct_menu_groups":true,"button":"apply"}));
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.construct_menu_groups);
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.construct_menu_groups);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.construct_menu_groups);
    }

    #[test]
    fn persistent_ribbon_pressed_preference_checkbox_control_persists_cancels_and_resets() {
        assert!(Prefs::default().persistent_ribbon_pressed);
        assert!(lenient(&json!({"theme":"light"})).persistent_ribbon_pressed);
        assert!(lenient(&json!({"persistent_ribbon_pressed":"no"})).persistent_ribbon_pressed);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        let revision = h.app.session.revision;
        h.call("ui.prefs", json!({"open":"Layout"}));
        super::reveal_preference(&mut h, "prefs:persistent_ribbon_pressed");
        let checkbox = crate::scenario::handle_at("prefs:persistent_ribbon_pressed").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(!h.app.preferences.persistent_ribbon_pressed);
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["persistent_ribbon_pressed"], false);
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.persistent_ribbon_pressed, "a change that was not applied is not saved (#9730)");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.persistent_ribbon_pressed);
        h.call("ui.prefs", json!({"open":"Layout","persistent_ribbon_pressed":false,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.persistent_ribbon_pressed);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.persistent_ribbon_pressed);
        for malformed in [json!("false"), json!(null), json!(0), json!([])] {
            assert!(super::set(&mut h.app, &json!({"persistent_ribbon_pressed":malformed})).is_err());
            assert!(!h.app.preferences.persistent_ribbon_pressed);
        }
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.doc.to_json(), design);
    }

    #[test]
    fn viewport_analysis_status_preference_checkbox_control_persists_cancels_and_resets() {
        assert!(Prefs::default().viewport_analysis_status);
        assert!(lenient(&json!({"theme":"light"})).viewport_analysis_status);
        assert!(lenient(&json!({"viewport_analysis_status":"no"})).viewport_analysis_status);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        let revision = h.app.session.revision;
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:viewport_analysis_status");
        let checkbox = crate::scenario::handle_at("prefs:viewport_analysis_status").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(!h.app.preferences.viewport_analysis_status);
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["viewport_analysis_status"], false);
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.viewport_analysis_status, "a change that was not applied is not saved (#9730)");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.viewport_analysis_status);
        h.call("ui.prefs", json!({"open":"Graphics","viewport_analysis_status":false,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.viewport_analysis_status);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.viewport_analysis_status);
        for malformed in [json!("false"), json!(null), json!(0), json!([])] {
            assert!(super::set(&mut h.app, &json!({"viewport_analysis_status":malformed})).is_err());
            assert!(!h.app.preferences.viewport_analysis_status);
        }
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.doc.to_json(), design);
    }

    #[test]
    fn viewport_analysis_status_is_independent_of_session_policy_in_background_documents() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        super::set(&mut app, &json!({"single_surface_analysis":false,"persistent_ribbon_pressed":false})).unwrap();
        app.run("inspect.zebra", json!({})).unwrap();
        let mut other = solidcraft_engine::Session::default();
        other.execute("inspect.draft", &json!({})).unwrap();
        crate::documents::open_tab(&mut app, other).unwrap();
        let before: Vec<_> =
            std::iter::once(&app.session).chain(app.docs.background()).map(|s| (s.status_items(), s.revision, s.undo.len())).collect();
        super::control(&mut app, &json!({"open":"Graphics","viewport_analysis_status":false})).unwrap();
        assert!(!app.preferences.viewport_analysis_status);
        assert!(!app.preferences.persistent_ribbon_pressed);
        assert!(std::iter::once(&app.session).chain(app.docs.background()).all(|s| !s.single_surface_analysis()));
        super::control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(app.preferences.viewport_analysis_status);
        let after: Vec<_> =
            std::iter::once(&app.session).chain(app.docs.background()).map(|s| (s.status_items(), s.revision, s.undo.len())).collect();
        assert_eq!(before, after, "a presentation preference never changes effects in any document");
    }

    #[test]
    fn viewport_section_status_preference_checkbox_control_persists_cancels_and_resets() {
        assert!(Prefs::default().viewport_section_status);
        assert!(lenient(&json!({"theme":"light"})).viewport_section_status);
        assert!(lenient(&json!({"viewport_section_status":"no"})).viewport_section_status);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        let revision = h.app.session.revision;
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:viewport_section_status");
        let checkbox = crate::scenario::handle_at("prefs:viewport_section_status").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(!h.app.preferences.viewport_section_status);
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["viewport_section_status"], false);
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.viewport_section_status, "a change that was not applied is not saved (#9730)");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.viewport_section_status);
        h.call("ui.prefs", json!({"open":"Graphics","viewport_section_status":false,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.viewport_section_status);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.viewport_section_status);
        for malformed in [json!("false"), json!(null), json!(0), json!([])] {
            assert!(super::set(&mut h.app, &json!({"viewport_section_status":malformed})).is_err());
            assert!(!h.app.preferences.viewport_section_status);
        }
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.doc.to_json(), design);
    }

    #[test]
    fn viewport_status_details_preference_checkbox_control_persists_cancels_and_resets() {
        assert!(Prefs::default().viewport_status_details);
        assert!(lenient(&json!({"theme":"light"})).viewport_status_details);
        assert!(lenient(&json!({"viewport_status_details":"no"})).viewport_status_details);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        let revision = h.app.session.revision;
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:viewport_status_details");
        let checkbox = crate::scenario::handle_at("prefs:viewport_status_details").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(!h.app.preferences.viewport_status_details);
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["viewport_status_details"], false);
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.viewport_status_details, "a change that was not applied is not saved (#9730)");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.viewport_status_details);
        h.call("ui.prefs", json!({"open":"Graphics","viewport_status_details":false,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.viewport_status_details);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.viewport_status_details);
        for malformed in [json!("false"), json!(null), json!(0), json!([])] {
            assert!(super::set(&mut h.app, &json!({"viewport_status_details":malformed})).is_err());
            assert!(!h.app.preferences.viewport_status_details);
        }
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.doc.to_json(), design);
    }

    #[test]
    fn viewport_status_summary_preference_checkbox_control_persists_cancels_and_resets() {
        assert!(Prefs::default().viewport_status_summary);
        assert!(lenient(&json!({"theme":"light"})).viewport_status_summary);
        assert!(lenient(&json!({"viewport_status_summary":"no"})).viewport_status_summary);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        let revision = h.app.session.revision;
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:viewport_status_summary");
        let checkbox = crate::scenario::handle_at("prefs:viewport_status_summary").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(!h.app.preferences.viewport_status_summary);
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["viewport_status_summary"], false);
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.viewport_status_summary, "a change that was not applied is not saved (#9730)");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.viewport_status_summary);
        h.call("ui.prefs", json!({"open":"Graphics","viewport_status_summary":false,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.viewport_status_summary);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.viewport_status_summary);
        for malformed in [json!("false"), json!(null), json!(0), json!([])] {
            assert!(super::set(&mut h.app, &json!({"viewport_status_summary":malformed})).is_err());
            assert!(!h.app.preferences.viewport_status_summary);
        }
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.doc.to_json(), design);
    }

    #[test]
    fn viewport_section_status_is_independent_of_session_policy_in_background_documents() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        super::set(&mut app, &json!({"single_surface_analysis":false,"persistent_ribbon_pressed":false})).unwrap();
        app.run("inspect.zebra", json!({})).unwrap();
        app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
        let mut other = solidcraft_engine::Session::default();
        other.execute("inspect.draft", &json!({})).unwrap();
        other.execute("inspect.section", &json!({"plane":"XZ","at":-3})).unwrap();
        crate::documents::open_tab(&mut app, other).unwrap();
        let before: Vec<_> =
            std::iter::once(&app.session).chain(app.docs.background()).map(|s| (s.status_items(), s.revision, s.undo.len())).collect();
        super::control(&mut app, &json!({"open":"Graphics","viewport_section_status":false})).unwrap();
        assert!(!app.preferences.viewport_section_status);
        assert!(!app.preferences.persistent_ribbon_pressed);
        assert!(std::iter::once(&app.session).chain(app.docs.background()).all(|s| !s.single_surface_analysis()));
        super::control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert!(app.preferences.viewport_section_status);
        let after: Vec<_> =
            std::iter::once(&app.session).chain(app.docs.background()).map(|s| (s.status_items(), s.revision, s.undo.len())).collect();
        assert_eq!(before, after, "a presentation preference never changes effects in any document");
    }

    #[test]
    fn single_surface_analysis_preference_checkbox_persists_cancels_and_resets() {
        assert!(Prefs::default().single_surface_analysis);
        let old: Prefs = serde_json::from_value(json!({"theme":"light"})).unwrap();
        assert!(old.single_surface_analysis);
        assert!(lenient(&json!({"single_surface_analysis":"no"})).single_surface_analysis);
        let mut h = crate::scenario::Harness::new();
        let design = h.app.session.doc.to_json();
        let revision = h.app.session.revision;
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:single_surface_analysis");
        let checkbox = crate::scenario::handle_at("prefs:single_surface_analysis").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(!h.app.preferences.single_surface_analysis);
        assert!(!h.app.session.single_surface_analysis());
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.single_surface_analysis, "a change that was not applied is not saved (#9730)");
        assert!(restored.session.single_surface_analysis(), "loaded settings apply before any command");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.single_surface_analysis && h.app.session.single_surface_analysis());
        h.call("ui.prefs", json!({"open":"Graphics","single_surface_analysis":false,"button":"apply"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.single_surface_analysis && h.app.session.single_surface_analysis());
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.single_surface_analysis && !h.app.session.single_surface_analysis());
        for malformed in [json!("false"), json!(null), json!(0), json!([])] {
            assert!(super::set(&mut h.app, &json!({"single_surface_analysis":malformed})).is_err());
            assert!(!h.app.preferences.single_surface_analysis && !h.app.session.single_surface_analysis());
        }
        assert_eq!(h.app.session.revision, revision, "policy-only edits with no effects need no view revision");
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(!h.app.session.is_dirty());
        assert!(h.app.session.undo.is_empty());
    }

    #[test]
    fn single_surface_analysis_preference_syncs_background_designs_and_resets_effects() {
        fn legacy_session() -> solidcraft_engine::Session {
            let mut s = solidcraft_engine::Session::default();
            s.set_single_surface_analysis(false);
            s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
            s.execute("inspect.zebra", &json!({"stripes":14.5})).unwrap();
            s.execute("inspect.section", &json!({"plane":"XY"})).unwrap();
            s.execute("inspect.continuity", &json!({"edges":[[5,0,0]],"samples":5})).unwrap();
            s.mark_saved();
            s
        }
        let mut app = SolveApp::new(Default::default(), Default::default());
        super::set(&mut app, &json!({"single_surface_analysis":false})).unwrap();
        app.session = legacy_session();
        crate::documents::open_tab(&mut app, legacy_session()).unwrap();
        assert_eq!(app.docs.count(), 2);
        assert!(!app.session.single_surface_analysis());
        assert_eq!(app.session.status_items().len(), 3);
        assert!(app.docs.background().all(|s| !s.single_surface_analysis() && s.status_items().len() == 3));
        let doc = app.session.doc.clone();
        let undo = app.session.undo.len();
        let declarations = app.session.status_items();
        let revision = app.session.revision;
        app.preferences.single_surface_analysis = true;
        app.run("view.status_items", json!({})).unwrap();
        assert!(!app.session.single_surface_analysis(), "a raw query cannot apply a desynchronised preference");
        assert_eq!(app.session.status_items(), declarations);
        assert_eq!(app.session.revision, revision);
        assert!(app.session.analysis.is_some() && !app.session.continuity.is_empty());
        assert!(app.docs.background().all(|s| !s.single_surface_analysis() && s.status_items().len() == 3));
        app.preferences.single_surface_analysis = false;
        super::defaults(&mut app, super::PAGE_GENERAL);
        assert!(!app.session.single_surface_analysis(), "another page leaves the policy alone");
        super::defaults(&mut app, super::PAGE_GRAPHICS);
        for s in std::iter::once(&app.session).chain(app.docs.background()) {
            assert!(s.single_surface_analysis());
            assert_eq!(s.analysis, None, "latest Continuity wins on reset");
            assert_eq!(s.continuity.len(), 1);
            assert_eq!(s.status_items().len(), 2);
            assert_eq!(s.status_items()[0].command, "inspect.continuity");
            assert!(s.section.is_some());
            assert!(!s.is_dirty());
        }
        assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
        assert_eq!(app.session.undo.len(), undo);
        // Query entry never applies a pending policy change; the explicit frame hook does.
        app.preferences.single_surface_analysis = false;
        app.run("view.status_items", json!({})).unwrap();
        assert!(app.session.single_surface_analysis());
        super::apply(&mut app, &egui::Context::default());
        assert!(!app.session.single_surface_analysis());
        assert!(app.docs.background().all(|s| !s.single_surface_analysis()));
        app.preferences.single_surface_analysis = true;
        super::apply(&mut app, &egui::Context::default());
        assert!(app.session.single_surface_analysis());
        assert!(app.docs.background().all(|s| s.single_surface_analysis()));
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
        assert!(!restored.preferences.cosmetic_threads, "a change that was not applied is not saved (#9730)");
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
    fn hidden_edge_preference_defaults_persists_and_uses_real_checkbox() {
        let mut h = crate::scenario::Harness::new();
        assert!(!Prefs::default().select_hidden_edges);
        assert!(!serde_json::from_value::<Prefs>(json!({})).unwrap().select_hidden_edges);
        h.call("ui.prefs", json!({"open":"Sketch"}));
        super::reveal_preference(&mut h, "prefs:select_hidden_edges");
        h.step(&json!({"click":{"handle":"prefs:select_hidden_edges"}})).unwrap();
        assert!(h.app.preferences.select_hidden_edges);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.select_hidden_edges);
        h.call("ui.prefs", json!({"open":"Sketch","select_hidden_edges":true,"button":"apply"}));
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.select_hidden_edges);
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.select_hidden_edges);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.select_hidden_edges);
    }

    #[test]
    fn ground_shadow_placement_follows_preferences_transactions() {
        assert!(!Prefs::default().ground_shadow_follows_model);
        let old: Prefs = serde_json::from_value(json!({"msaa":4})).unwrap();
        assert!(!old.ground_shadow_follows_model);
        assert!(!lenient(&json!({"ground_shadow_follows_model":"yes"})).ground_shadow_follows_model);
        let mut h = crate::scenario::Harness::new();
        h.app.ui.ground_shadow = true;
        let camera = h.app.cam;
        let design = h.app.session.doc.to_json();
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:ground_shadow_follows_model");
        let checkbox = crate::scenario::handle_at("prefs:ground_shadow_follows_model").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(h.app.preferences.ground_shadow_follows_model);
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.ground_shadow_follows_model, "an unapplied change is not saved");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.ground_shadow_follows_model);
        h.call("ui.prefs", json!({"ground_shadow_follows_model":true}));
        assert!(!h.app.prefs_window.shell.open, "programmatic changes do not open Preferences");
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.ground_shadow_follows_model);
        h.call("ui.prefs", json!({"open":"Graphics", "button":"defaults"}));
        assert!(!h.app.preferences.ground_shadow_follows_model);
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(h.app.preferences.ground_shadow_follows_model, "Cancel restores the saved choice");
        h.call("ui.prefs", json!({"open":"Graphics", "button":"defaults"}));
        h.call("ui.prefs", json!({"button":"apply"}));
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.ground_shadow_follows_model, "Graphics defaults can be saved");
        assert_eq!(h.app.cam, camera);
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
    fn choosing_the_solidworks_preset_frees_the_right_button_and_fits_on_middle_double_click() {
        let none = Modifiers::default();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.middle_double_click = "off".into();
        set(&mut app, &json!({"nav": "solidworks"})).unwrap();
        assert!(app.preferences.right_drag_orbits, "a saved SolidWorks preset keeps its right-drag orbit");
        assert_eq!(nav_mode(&app, NavButton::Right, none), Some(NavMode::Orbit));
        choose_nav_preset(&mut app.preferences, nav_preset("fusion"));
        let p = &app.preferences;
        assert_eq!((p.nav.as_str(), p.zoom_reverse, p.middle_double_click.as_str(), p.right_drag_orbits), ("fusion", false, "off", true));
        choose_nav_preset(&mut app.preferences, nav_preset("solidworks"));
        let p = &app.preferences;
        assert_eq!((p.nav.as_str(), p.zoom_reverse, p.middle_double_click.as_str(), p.right_drag_orbits), ("solidworks", true, "fit", false));
        assert_eq!(nav_mode(&app, NavButton::Right, none), None, "right drag is left to the context menu (#6492)");
        assert_eq!(nav_mode(&app, NavButton::Middle, none), Some(NavMode::Orbit));
        set(&mut app, &json!({"nav": "openscad"})).unwrap();
        assert_eq!(nav_mode(&app, NavButton::Right, none), Some(NavMode::Pan), "only an orbiting right drag is affected");
        set(&mut app, &json!({"right_drag_orbits": true, "nav": "solidworks"})).unwrap();
        assert_eq!(nav_mode(&app, NavButton::Right, none), Some(NavMode::Orbit), "turned back on, the right drag orbits again");
    }

    #[test]
    fn creo_preset_maps_navigation_and_persists() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert_eq!(app.preferences.nav, "fusion");
        choose_nav_preset(&mut app.preferences, nav_preset("creo"));
        assert_eq!(app.preferences.nav, "creo");
        assert!(!app.preferences.zoom_reverse);
        for (mods, mode) in [
            (Modifiers::NONE, NavMode::Orbit),
            (Modifiers::SHIFT, NavMode::Pan),
            (Modifiers::CTRL, NavMode::Zoom),
            (Modifiers::COMMAND, NavMode::Zoom),
        ] {
            assert_eq!(nav_mode(&app, NavButton::Middle, mods), Some(mode));
        }
        for button in [NavButton::Left, NavButton::Right] {
            assert_eq!(nav_mode(&app, button, Modifiers::NONE), None);
        }
        assert!(nav_preset("creo").reverse_drag_zoom);
        for preset in NAV_PRESETS.iter().filter(|p| p.id != "creo") {
            assert!(!preset.reverse_drag_zoom, "{} keeps its drag zoom direction", preset.id);
        }
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.nav, "creo");
        assert_eq!(nav_mode(&other, NavButton::Middle, Modifiers::NONE), Some(NavMode::Orbit));
    }

    #[test]
    fn creo_navigation_settings_show_localised_bindings() {
        crate::i18n::install();
        let _language = solidcraft_ui_text::scope("de");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open":"Navigation", "nav":"creo", "language":"de"})).unwrap();
        let ctx = crate::fonts::test_context();
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 1000.0));
        let mut textures = std::collections::HashMap::new();
        let mut draw = || {
            let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| show(&mut app, ui.ctx()));
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            output
        };
        for _ in 0..3 {
            draw();
        }
        let output = draw();
        for label in [
            "Creo",
            solidcraft_ui_text::tr("prefs.gesture_orbit"),
            solidcraft_ui_text::tr("prefs.gesture_pan"),
            solidcraft_ui_text::tr("prefs.gesture_zoom"),
        ] {
            assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text().contains(label)), "missing binding label: {label}");
        }
        assert_ne!(nav_preset("creo").translated_help(), nav_preset("creo").help);
        if let Ok(dir) = std::env::var("SOLIDCRAFT_CREO_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, screen, &textures)
                .save(std::path::Path::new(&dir).join("creo-navigation-de.png"))
                .unwrap();
        }
    }

    #[test]
    fn creo_preset_obeys_preference_transactions() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open":"Navigation", "nav":"creo"})).unwrap();
        assert_eq!(app.preferences.nav, "creo");
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        control(&mut app, &json!({"open":"Navigation", "nav":"creo", "button":"apply"})).unwrap();
        control(&mut app, &json!({"button":"defaults"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        control(&mut app, &json!({"button":"cancel"})).unwrap();
        assert_eq!(app.preferences.nav, "creo");
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
    fn orbit_reverse_defaults_off_persists_and_flips_the_sign() {
        assert!(!serde_json::from_value::<Prefs>(json!({})).unwrap().orbit_reverse);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert_eq!(orbit_sign(&app), 1.0);
        set(&mut app, &json!({"orbit_reverse": true})).unwrap();
        assert_eq!(orbit_sign(&app), -1.0);
        assert_eq!((pan_sign_x(&app), pan_sign_y(&app)), (1.0, 1.0));
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert!(other.preferences.orbit_reverse);
    }

    #[test]
    fn pan_reverse_y_defaults_off_persists_and_flips_the_sign() {
        assert!(!serde_json::from_value::<Prefs>(json!({})).unwrap().pan_reverse_y);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert_eq!(pan_sign_y(&app), 1.0);
        set(&mut app, &json!({"pan_reverse_y": true})).unwrap();
        assert_eq!(pan_sign_y(&app), -1.0);
        assert_eq!(pan_sign_x(&app), 1.0);
        set(&mut app, &json!({"pan_reverse_x": true, "pan_reverse_y": false})).unwrap();
        assert_eq!(pan_sign_x(&app), -1.0);
        assert_eq!(pan_sign_y(&app), 1.0);
        set(&mut app, &json!({"pan_reverse_y": true})).unwrap();
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert!(other.preferences.pan_reverse_y);
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

    /// #6474, #2742: Maya moves the camera with Alt and any button, HFSS with Alt or Shift+Alt and
    /// the left button. A plain left drag still selects, and HFSS leaves Ctrl+left to the drag
    /// from empty space every preset has, so a Ctrl+click on geometry still adds to the selection.
    #[test]
    fn maya_and_hfss_presets_navigate_with_alt() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let none = Modifiers::default();
        let alt = Modifiers { alt: true, ..Default::default() };
        let shift_alt = Modifiers { shift: true, alt: true, ..Default::default() };
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        set(&mut app, &json!({"nav": "maya"})).unwrap();
        assert_eq!(nav_mode(&app, NavButton::Left, alt), Some(NavMode::Orbit));
        assert_eq!(nav_mode(&app, NavButton::Middle, alt), Some(NavMode::Pan));
        assert_eq!(nav_mode(&app, NavButton::Right, alt), Some(NavMode::Zoom));
        assert_eq!(nav_mode(&app, NavButton::Middle, none), Some(NavMode::Pan));
        assert_eq!(nav_mode(&app, NavButton::Left, none), None, "a plain left drag selects");
        assert_eq!(nav_mode(&app, NavButton::Right, none), None, "a plain right click opens the context menu");
        set(&mut app, &json!({"nav": "hfss"})).unwrap();
        assert_eq!(nav_mode(&app, NavButton::Left, alt), Some(NavMode::Pan));
        assert_eq!(nav_mode(&app, NavButton::Left, shift_alt), Some(NavMode::Zoom));
        assert_eq!(nav_mode(&app, NavButton::Left, none), None);
        assert_eq!(nav_mode(&app, NavButton::Left, ctrl), None);
        assert_eq!(zoom_sign(&app), 1.0);
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.nav, "hfss");
    }

    #[test]
    fn maya_zoom_axis_preference_is_saved_and_reset_with_navigation() {
        let mut h = crate::scenario::Harness::new();
        assert!(h.app.preferences.maya_horizontal_zoom);
        control(&mut h.app, &json!({"open":"Navigation","nav":"maya","maya_horizontal_zoom":false})).unwrap();
        control(&mut h.app, &json!({"button":"apply"})).unwrap();
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&h.app.prefs());
        assert_eq!(other.preferences.nav, "maya");
        assert!(!other.preferences.maya_horizontal_zoom, "the legacy direction survives reopening");
        control(&mut h.app, &json!({"button":"defaults"})).unwrap();
        assert!(h.app.preferences.maya_horizontal_zoom);
        control(&mut h.app, &json!({"button":"cancel"})).unwrap();
        assert!(!h.app.preferences.maya_horizontal_zoom, "Cancel restores the applied legacy choice");
        assert!(set(&mut h.app, &json!({"maya_horizontal_zoom":"sideways"})).is_err());
    }

    /// The dialog: changes apply at once, Cancel puts back what was there when it opened, Apply
    /// keeps them, Restore Defaults resets one page, and the shortcuts menus open its page.
    #[test]
    fn preferences_dialog_cancel_apply_and_defaults() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open": "general"})).unwrap();
        assert!(app.prefs_window.shell.open && app.prefs_window.shell.section == 0);
        control(&mut app, &json!({"theme": "light", "ui_scale": 1.25, "zoom_to_cursor": false})).unwrap();
        assert!(!app.ui.dark, "light applies at once");
        app.keymap.custom.insert("solid.extrude".into(), vec!["Shift+E".into()]);
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
            c.set_view(solidcraft_viewport::StandardView::Top);
            c
        };
        app.animate_to(to);
        assert!(app.cam_anim.is_none() && app.cam == to, "no animation: the view jumps");

        control(&mut app, &json!({"open": "Navigation"})).unwrap();
        control(&mut app, &json!({"button": "defaults"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        assert!(app.preferences.look_at_anim);
        control(&mut app, &json!({"button": "ok"})).unwrap();
        assert!(!app.prefs_window.shell.open);

        // File and Help › Keyboard Shortcuts open the same dialog at its page.
        app.keymap.open = true;
        let ctx = crate::fonts::test_context();
        let mut out = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        out.textures_delta.clear();
        assert!(app.prefs_window.shell.open && app.prefs_window.shell.section == PAGE_SHORTCUTS && !app.keymap.open);
        assert!(control(&mut app, &json!({"open": "NoSuch"})).is_err());
        assert!(set(&mut app, &json!({"up_axis": "x"})).is_err());
        assert!(set(&mut app, &json!({"theme": "sepia"})).is_err());
        assert!(set(&mut app, &json!({"language": "xx"})).is_err());
    }

    /// Apply is disabled while nothing changed since the dialog opened or since the last Apply,
    /// enabled by a change, and disabled again by Apply (#8561).
    #[test]
    fn apply_is_enabled_only_while_something_changed() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let ctx = crate::fonts::test_context();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            focused: true,
            ..Default::default()
        };
        let draw = |app: &mut SolveApp, events: Vec<egui::Event>| {
            crate::test_support::frame(&ctx, egui::RawInput { events, ..input() }, |ui| show(app, ui.ctx()));
            crate::scenario::count_of("dialog:apply:enabled")
        };
        let v = control(&mut app, &json!({"open": "Navigation"})).unwrap();
        assert_eq!(v["apply_enabled"], false, "nothing changed yet");
        assert_eq!(draw(&mut app, vec![]), Some(0.0), "just opened: Apply is disabled");

        let v = control(&mut app, &json!({"zoom_to_cursor": false})).unwrap();
        assert_eq!(v["apply_enabled"], true);
        assert_eq!(draw(&mut app, vec![]), Some(1.0), "after a change: Apply is enabled");
        let v = control(&mut app, &json!({"zoom_to_cursor": true})).unwrap();
        assert_eq!(v["apply_enabled"], false, "a change put back by hand leaves nothing to apply");
        control(&mut app, &json!({"zoom_to_cursor": false})).unwrap();

        // Clicking Apply keeps the change and leaves nothing to apply.
        draw(&mut app, vec![]);
        let apply = crate::scenario::handle_at("dialog:apply").unwrap();
        draw(&mut app, crate::test_support::pointer(apply, true));
        draw(&mut app, crate::test_support::pointer(apply, false));
        assert!(app.prefs_window.shell.open && !app.preferences.zoom_to_cursor, "Apply keeps the dialog and the change");
        assert_eq!(draw(&mut app, vec![]), Some(0.0), "after Apply: Apply is disabled");
        // A disabled Apply does nothing when clicked.
        draw(&mut app, crate::test_support::pointer(apply, true));
        draw(&mut app, crate::test_support::pointer(apply, false));
        assert_eq!(draw(&mut app, vec![]), Some(0.0));

        // Every setting Cancel would put back counts, the shortcuts and view settings included;
        // update bookkeeping (not a setting, and Cancel keeps it) does not.
        app.preferences.update.last_check += 1.0;
        assert_eq!(draw(&mut app, vec![]), Some(0.0), "update bookkeeping is not a change");
        app.preferences.stand_up_answers.remember("a".repeat(64), crate::stand_up::Answer::YToZ);
        assert_eq!(draw(&mut app, vec![]), Some(0.0), "a recorded stand-up answer is not a change either");
        assert_eq!(control(&mut app, &json!({})).unwrap()["apply_enabled"], false);
        app.keymap.custom.insert("solid.extrude".into(), vec!["Shift+E".into()]);
        assert_eq!(draw(&mut app, vec![]), Some(1.0), "a rebinding is a change");
        let v = control(&mut app, &json!({"button": "apply"})).unwrap();
        assert_eq!(v["apply_enabled"], false);
        app.ui.show_grid = !app.ui.show_grid;
        assert_eq!(draw(&mut app, vec![]), Some(1.0), "a view setting is a change");

        // Cancel puts it back and closes; opened again, there is nothing to apply.
        let v = control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert_eq!(v["apply_enabled"], false);
        let v = control(&mut app, &json!({"open": "General"})).unwrap();
        assert_eq!(v["apply_enabled"], false);
    }

    #[test]
    fn language_preference_switches_and_persists() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        // A first start follows the system (#10138), which is English in a test.
        assert_eq!(app.preferences.language, solidcraft_ui_text::SYSTEM);
        assert_eq!(app.tr("ui.preferences"), "Preferences");
        assert_eq!(app.tr("ui.extrude"), "Extrude");

        // Switch language to Japanese
        set(&mut app, &json!({"language": "ja", "history_in_browser": true})).unwrap();
        assert_eq!(app.preferences.language, "ja");
        assert!(app.preferences.history_in_browser);
        assert_eq!(app.tr("ui.preferences"), "環境設定");
        assert_eq!(app.tr("ui.extrude"), "押し出し");

        // Preferences round-trip
        let mut other = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences.language, "ja");
        assert!(other.preferences.history_in_browser);
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
        // Another originating system is offered (asked in the dialog, without looking at how the
        // flat box rests).
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'")).unwrap();
        app.preferences.stand_up_unknown = StandUpUnknown::Ask;
        app.open_path(&path);
        let offered = app.stand_up.clone().unwrap_or_default();
        assert_eq!(offered.len(), 1, "the file's body is offered");
        let size = |app: &SolveApp| solidcraft_engine::view::bounds(&app.session).unwrap();
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

    #[test]
    fn stand_up_memory_reopens_changed_files_and_can_be_disabled_or_forgotten() {
        use crate::stand_up::{Answer, Answers};
        let dir = std::env::temp_dir().join(format!("solidcraft-standup-memory-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.step").to_string_lossy().to_string();
        let mut src = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        src.run("solid.box", json!({"length": 10, "width": 40, "height": 5})).unwrap();
        src.run("file.export", json!({"path": path})).unwrap();
        let text = std::fs::read_to_string(&path).unwrap().replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'");
        std::fs::write(&path, &text).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.stand_up_unknown = StandUpUnknown::Ask;
        assert!(app.preferences.remember_stand_up);
        app.open_path(&path);
        assert!(app.stand_up.is_some());
        let digest = app.stand_up_digest.clone().unwrap();
        control(&mut app, &json!({"stand_up": true})).unwrap();
        assert_eq!(app.preferences.stand_up_answers.get(&digest), Some(Answer::YToZ));
        let stood = solidcraft_engine::view::bounds(&app.session).unwrap();
        // Across app restarts, too: the user history round-trips through the real settings API.
        let mut reopened = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        reopened.load_prefs(&app.prefs());
        assert!(reopened.preferences.reuse_imports);
        let hit = reopened.run("doc.open", json!({"path": path})).unwrap();
        assert_eq!(hit["import_reused"], true, "remembered stand-up applies on a cache hit");
        crate::stand_up::offer(&mut reopened, &path, &[], Some(&hit));
        assert!(reopened.stand_up.is_none());
        assert_eq!(solidcraft_engine::view::bounds(&reopened.session).unwrap(), stood);
        reopened.run("edit.undo", json!({})).unwrap();
        let b = solidcraft_engine::view::bounds(&reopened.session).unwrap();
        assert!((b.max.y - b.min.y - 40.0).abs() < 1e-6, "remembered rotation is undoable");

        control(&mut reopened, &json!({"reuse_imports": false})).unwrap();
        assert_eq!(reopened.run("doc.open", json!({"path": path})).unwrap()["import_reused"], false);
        let mut settings = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        settings.load_prefs(&reopened.prefs());
        assert!(!settings.preferences.reuse_imports, "reuse preference survives restart");
        control(&mut reopened, &json!({"reuse_imports": true})).unwrap();
        let answers = reopened.preferences.stand_up_answers.clone();
        reopened.run("file.import_cache_clear", json!({})).unwrap();
        assert_eq!(reopened.preferences.stand_up_answers, answers, "clearing imports keeps stand-up answers");
        assert_eq!(reopened.run("doc.open", json!({"path": path})).unwrap()["import_reused"], false);
        assert_eq!(reopened.run("doc.open", json!({"path": path})).unwrap()["import_reused"], true);

        // A byte change, even one that leaves the geometry alone, asks again.
        std::fs::write(&path, format!("{text}\n")).unwrap();
        reopened.open_path(&path);
        assert!(reopened.stand_up.is_some());
        assert_ne!(reopened.stand_up_digest.as_ref(), Some(&digest));
        control(&mut reopened, &json!({"stand_up": false})).unwrap();
        let left = solidcraft_engine::view::bounds(&reopened.session).unwrap();
        reopened.open_path(&path);
        assert!(reopened.stand_up.is_none());
        assert_eq!(solidcraft_engine::view::bounds(&reopened.session).unwrap(), left);

        let remembered = reopened.preferences.stand_up_answers.clone();
        control(&mut reopened, &json!({"remember_stand_up": false})).unwrap();
        for answer in [true, false] {
            reopened.open_path(&path);
            assert!(reopened.stand_up.is_some(), "memory off always offers");
            control(&mut reopened, &json!({"stand_up": answer})).unwrap();
            assert_eq!(reopened.preferences.stand_up_answers, remembered, "memory off stores no answers");
        }
        // Turned back on before the window opens, so Cancel below does not undo it with the rest.
        control(&mut reopened, &json!({"remember_stand_up": true})).unwrap();
        control(&mut reopened, &json!({"open": "Design"})).unwrap();
        let before = reopened.preferences.clone();
        for invalid in [json!({"forget_stand_up": false}), json!({"forget_stand_up": true, "theme": "missing-theme"})] {
            assert!(control(&mut reopened, &invalid).is_err());
            assert_eq!(reopened.preferences, before, "invalid requests cannot forget history or change settings");
        }
        control(&mut reopened, &json!({"forget_stand_up": true})).unwrap();
        control(&mut reopened, &json!({"button": "cancel"})).unwrap();
        assert_eq!(reopened.preferences.stand_up_answers, Answers::default(), "Cancel cannot restore forgotten history");
        reopened.open_path(&path);
        assert!(reopened.stand_up.is_some());
        // A note's timeout is also a Leave it answer.
        reopened.stand_up_note = true;
        reopened.stand_up_note_since = Some(-crate::stand_up::NOTE_SECONDS);
        let ctx = crate::fonts::test_context();
        assert!(crate::stand_up::note_due(&mut reopened, &ctx).is_none());
        reopened.open_path(&path);
        assert!(reopened.stand_up.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stand_up_memory_is_bounded_and_ignores_corrupt_entries() {
        use crate::stand_up::{Answer, Answers, MAX_ANSWERS};
        let key = |i: usize| format!("{i:064x}");
        let mut entries = Vec::new();
        for i in 0..=MAX_ANSWERS {
            entries.push(json!({"digest": key(i), "answer": "y_to_z"}));
        }
        entries.extend([
            json!({"digest": "bad", "answer": "leave"}),
            json!({"digest": "g".repeat(64), "answer": "leave"}),
            json!({"digest": key(MAX_ANSWERS + 1), "answer": "rotate_infinity"}),
            json!({"digest": key(MAX_ANSWERS + 2), "answer": {"angle": 90}}),
            json!(null),
        ]);
        let mut prefs: Prefs = serde_json::from_value(json!({"stand_up_answers": entries, "remember_stand_up": false})).unwrap();
        assert!(!prefs.remember_stand_up);
        assert_eq!(prefs.stand_up_answers.get(&key(0)), None, "oldest was evicted");
        assert_eq!(prefs.stand_up_answers.get(&key(1)), Some(Answer::YToZ));
        assert_eq!(serde_json::to_value(&prefs.stand_up_answers).unwrap().as_array().unwrap().len(), MAX_ANSWERS);
        prefs.stand_up_answers.remember(key(1), Answer::Leave);
        prefs.stand_up_answers.remember(key(MAX_ANSWERS + 1), Answer::Leave);
        assert_eq!(prefs.stand_up_answers.get(&key(1)), Some(Answer::Leave));
        assert_eq!(prefs.stand_up_answers.get(&key(2)), None, "replacing an answer makes it newest");
        let roundtrip: Prefs = serde_json::from_value(serde_json::to_value(&prefs).unwrap()).unwrap();
        assert_eq!(roundtrip, prefs);
        for bad in [json!(null), json!(5), json!({"bad": true})] {
            let read: Prefs = serde_json::from_value(json!({"stand_up_answers": bad, "remember_stand_up": false})).unwrap();
            assert_eq!(read.stand_up_answers, Answers::default());
            assert!(!read.remember_stand_up);
        }
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

    /// Constraint glyphs keep their size unless a larger one is chosen (#10794); `ui.prefs` takes
    /// only the offered range, and a hand-edited file out of it reads as the default.
    #[test]
    fn constraint_glyph_size_defaults_to_the_old_size_and_stays_in_range() {
        assert_eq!(Prefs::default().constraint_glyph_scale, 1.0);
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        set(&mut app, &json!({"constraint_glyph_scale": 2.0})).unwrap();
        assert_eq!(app.preferences.constraint_glyph_scale, 2.0);
        for bad in [0.5, 3.5, -1.0] {
            assert!(set(&mut app, &json!({"constraint_glyph_scale": bad})).is_err(), "{bad} is refused");
        }
        assert_eq!(app.preferences.constraint_glyph_scale, 2.0, "a refused value changes nothing");
        assert_eq!(migrate(lenient(&json!({"constraint_glyph_scale": 40.0}))).constraint_glyph_scale, 1.0);
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

    /// A drag that starts on an edge turns around the edge's point under the cursor, not its
    /// middle, and that point stays still on screen.
    #[test]
    fn orbiting_from_an_edge_turns_around_the_point_under_the_cursor() {
        use crate::viewport::Hit;
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 200, "width": 40, "height": 40})).unwrap();
        app.preferences.orbit_around = OrbitAround::Cursor;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.fit_view();
        let proj = crate::viewport::projection(&app, rect);
        let st = crate::open_meshing::session_world(&app.session);
        let m = st.bodies[0].mesh();
        // A spot a fifth of the way along a visible edge, well away from its middle on screen.
        let found = m.edges.iter().filter(|e| e.len() >= 2).find_map(|e| {
            let (a, b) = (e[0], e[e.len() - 1]);
            let at = proj.to_screen(a + (b - a) * 0.2)?;
            let mid = proj.to_screen(crate::viewport::polyline_mid(e))?;
            let first =
                crate::viewport::pick(&app, &proj, at).into_iter().find(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. }));
            (rect.contains(at) && at.distance(mid) > 100.0 && matches!(first, Some(Hit::Edge { .. }))).then_some((at, mid))
        });
        let (at, mid) = found.unwrap();
        let pivot = pivot_under(&app, &proj, at);
        let on_screen = proj.to_screen(pivot).unwrap();
        assert!(on_screen.distance(at) < 6.0, "the pivot is under the cursor: {on_screen:?} vs {at:?}");
        assert!(on_screen.distance(mid) > 90.0, "not the edge's middle");
        orbit(&mut app, rect, 40.0, 25.0, Some(at));
        assert_eq!(app.viewport.orbit_pivot, Some(pivot));
        let after = crate::viewport::projection(&app, rect).to_screen(pivot).unwrap();
        assert!(after.distance(on_screen) < 2.0, "{on_screen:?} -> {after:?}");
    }

    /// On a sketch line too: the line's point under the cursor, not the view's middle.
    #[test]
    fn orbiting_from_a_sketch_line_turns_around_the_point_under_the_cursor() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        app.run("sketch.line", json!({"points": [[-100, 0], [100, 0]]})).unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        app.preferences.orbit_around = OrbitAround::Cursor;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.fit_view();
        let proj = crate::viewport::projection(&app, rect);
        let on_line = Vec3::new(-60.0, 0.0, 0.0);
        let at = proj.to_screen(on_line).unwrap();
        let pivot = pivot_under(&app, &proj, at);
        assert!(pivot.dist(on_line) < 2.0, "{pivot:?}");
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
        app.cam.target = solidcore_record::Vec3::new(5.0, -3.0, 2.0);
        orbit(&mut app, rect, 40.0, 25.0, Some(corner));
        assert_eq!(app.viewport.orbit_pivot, Some(solidcore_record::Vec3::new(5.0, -3.0, 2.0)));
    }

    /// The Y up preference turns the view: the camera's up axis follows it, startup included.
    #[test]
    fn up_axis_preference_turns_the_view() {
        use solidcraft_viewport::UpAxis;
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
        assert!(!restored.preferences.ambient_occlusion, "a change that was not applied is not saved (#9730)");
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

    /// #7974: shading dark parts in linear light with a sky fill is off for older and new
    /// preferences, turned on from the Graphics page, kept, and sent to the shader with the world's
    /// up direction; Restore Defaults turns it off again.
    #[test]
    fn dark_parts_shading_is_optional_persistent_and_follows_the_up_axis() {
        use solidcraft_viewport::UpAxis;
        let old: Prefs = serde_json::from_value(json!({"msaa": 4})).unwrap();
        assert!(!old.dark_parts_shading && !Prefs::default().dark_parts_shading);
        assert_eq!(crate::viewport::sky_uniform(false, UpAxis::Z), [0.0; 4], "off: shaded as before");
        assert_eq!(crate::viewport::sky_uniform(true, UpAxis::Z), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(crate::viewport::sky_uniform(true, UpAxis::Y), [0.0, 1.0, 0.0, 1.0]);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"open":"Graphics"}));
        super::reveal_preference(&mut h, "prefs:dark_parts_shading");
        let checkbox = crate::scenario::handle_at("prefs:dark_parts_shading").unwrap();
        h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
        assert!(h.app.preferences.dark_parts_shading);
        h.call("ui.prefs", json!({"button":"apply"}));
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.dark_parts_shading);
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.dark_parts_shading);
    }
}

/// Manual edits are preserved and shown as Custom rather than claiming to match a preset.
pub(crate) fn graphics_preset(prefs: &Prefs) -> &'static str {
    match (prefs.ambient_occlusion, prefs.display_quality.as_str(), prefs.supersampling, prefs.supersampling_adaptive) {
        (false, "normal", 1.0, _) => "performance",
        (true, "fine", 2.0, true) => "quality",
        _ => "custom",
    }
}

#[cfg(test)]
mod graphics_preset_tests {
    use super::*;

    fn click_graphics(h: &mut crate::scenario::Harness, query: &str, handle: &str) {
        h.app.prefs_window.shell.filter = query.into();
        h.frames(5);
        reveal_preference(h, handle);
        h.step(&json!({"click":{"handle":handle}})).unwrap();
        h.frames(2);
    }

    #[test]
    fn graphics_cards_follow_manual_changes_and_preserve_non_preset_settings_in_both_themes() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.prefs", json!({"theme":theme,"language":"en","msaa":2,"dark_parts_shading":true,"frame_rate_details":true}));
            h.call("ui.prefs", json!({"open":"Graphics"}));
            let camera = h.app.cam;
            let design = h.app.session.doc.to_json();
            click_graphics(&mut h, "Quality", "view:graphics_preset:quality");
            assert_eq!(graphics_preset(&h.app.preferences), "quality");
            assert_eq!(
                (h.app.preferences.display_quality.as_str(), h.app.preferences.supersampling, h.app.preferences.supersampling_adaptive),
                ("fine", 2.0, true)
            );
            assert!(h.app.preferences.ambient_occlusion);
            assert_eq!(h.app.preferences.msaa, 2);
            assert!(h.app.preferences.dark_parts_shading && h.app.preferences.frame_rate_details);
            let quality = h.app.preferences.clone();
            click_graphics(&mut h, "Custom", "view:graphics_preset:custom");
            assert_eq!(h.app.preferences, quality, "Custom represents the mixture and sets nothing");
            click_graphics(&mut h, "Ultra", "prefs:display_quality:ultra");
            assert_eq!(h.app.preferences.display_quality, "ultra");
            assert_eq!(graphics_preset(&h.app.preferences), "custom");
            click_graphics(&mut h, "Performance", "view:graphics_preset:performance");
            assert_eq!(graphics_preset(&h.app.preferences), "performance");
            assert_eq!(
                (h.app.preferences.ambient_occlusion, h.app.preferences.supersampling, h.app.preferences.display_quality.as_str()),
                (false, 1.0, "normal")
            );
            h.call("ui.prefs", json!({"supersampling_adaptive":false}));
            assert_eq!(graphics_preset(&h.app.preferences), "performance", "Performance ignores the stored adaptive flag");
            assert_eq!(h.app.preferences.msaa, 2);
            assert!(h.app.preferences.dark_parts_shading && h.app.preferences.frame_rate_details);
            assert_eq!(h.app.cam, camera);
            assert_eq!(h.app.session.doc.to_json(), design);
            assert!(h.app.session.undo.is_empty());
        }
    }

    #[test]
    fn graphics_tiles_and_status_chips_edit_real_values_and_cancel_in_both_themes() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.prefs", json!({"theme":theme,"language":"en"}));
            h.call("ui.prefs", json!({"open":"Graphics"}));
            let original = h.app.preferences.clone();
            let session = (h.app.ui.show_grid, h.app.ui.ground_shadow, h.app.ui.perspective);
            for (query, handle) in [("Grid", "prefs:grid"), ("Ground shadow", "prefs:ground_shadow"), ("Perspective", "prefs:perspective")] {
                click_graphics(&mut h, query, handle);
            }
            assert_eq!((h.app.ui.show_grid, h.app.ui.ground_shadow, h.app.ui.perspective), (!session.0, !session.1, !session.2));
            for (query, field) in [
                ("Ambient occlusion", "ambient_occlusion"),
                ("View angles", "show_view_angles"),
                ("Vertex markers", "vertex_markers"),
                ("Quick measurement", "quick_measure"),
                ("Frame rate", "show_frame_rate"),
                ("Analysis", "viewport_analysis_status"),
                ("Section", "viewport_section_status"),
                ("Live details", "viewport_status_details"),
                ("Fold items", "viewport_status_summary"),
            ] {
                let before = serde_json::to_value(&h.app.preferences).unwrap();
                click_graphics(&mut h, query, &format!("prefs:{field}"));
                let after = serde_json::to_value(&h.app.preferences).unwrap();
                let mut expected = before.clone();
                expected[field] = json!(!before[field].as_bool().unwrap());
                assert_eq!(after, expected, "{theme}: {field} changes precisely its own setting");
            }
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences, original);
            assert_eq!((h.app.ui.show_grid, h.app.ui.ground_shadow, h.app.ui.perspective), session);
        }
    }

    #[test]
    fn graphics_overlay_cards_apply_persist_and_reset_without_adding_unpaged_fields() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"language":"en"}));
        h.call("ui.prefs", json!({"open":"Graphics"}));
        for (query, handle) in [
            ("View angles", "prefs:show_view_angles"),
            ("Vertex markers", "prefs:vertex_markers"),
            ("Quick measurement", "prefs:quick_measure"),
            ("Frame rate", "prefs:show_frame_rate"),
            ("Frame rate details", "prefs:frame_rate_details"),
            ("Analysis", "prefs:viewport_analysis_status"),
            ("Section", "prefs:viewport_section_status"),
            ("Live details", "prefs:viewport_status_details"),
            ("Fold items", "prefs:viewport_status_summary"),
        ] {
            click_graphics(&mut h, query, handle);
        }
        h.call("ui.prefs", json!({"button":"apply"}));
        let applied = h.app.preferences.clone();
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.show_view_angles && restored.preferences.vertex_markers && restored.preferences.quick_measure);
        assert!(restored.preferences.show_frame_rate && restored.preferences.frame_rate_details);
        assert!(!restored.preferences.viewport_analysis_status && !restored.preferences.viewport_section_status);
        assert!(!restored.preferences.viewport_status_details && !restored.preferences.viewport_status_summary);
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(!h.app.preferences.show_view_angles && !h.app.preferences.quick_measure);
        assert!(!h.app.preferences.show_frame_rate && !h.app.preferences.frame_rate_details);
        assert!(h.app.preferences.viewport_analysis_status && h.app.preferences.viewport_section_status);
        assert!(h.app.preferences.viewport_status_details && h.app.preferences.viewport_status_summary);
        assert!(h.app.preferences.vertex_markers, "the visible unpaged control remains excluded from page defaults");
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert_eq!(h.app.preferences, applied, "Cancel after Apply restores the applied overlay state");
    }

    #[test]
    fn graphics_redesign_keeps_newer_main_controls_and_their_transactions() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", json!({"theme":theme,"language":"en","open":"Graphics"}));
            let original = h.app.preferences.clone();
            for (query, handle) in [
                ("Tint new surface in a preview", "prefs:preview_tint_new_faces"),
                ("Show removed material in red", "prefs:cut_preview_tint"),
                ("Profile operations", "prefs:profile_operations"),
            ] {
                click_graphics(&mut h, query, handle);
            }
            h.app.prefs_window.shell.filter = "Section hatch".into();
            h.frames(5);
            reveal_preference(&mut h, "dialog:Section hatch:One style");
            h.step(&json!({"click":{"handle":"dialog:Section hatch:One style"}})).unwrap();
            h.frames(2);
            assert!(!h.app.preferences.preview_tint_new_faces && !h.app.preferences.cut_preview_tint);
            assert!(h.app.preferences.profile_operations);
            assert_eq!(h.app.preferences.section_hatch, SectionHatch::One);
            h.call("ui.prefs", json!({"button":"apply"}));
            let applied = h.app.preferences.clone();
            h.call("ui.prefs", json!({"button":"defaults"}));
            assert!(h.app.preferences.preview_tint_new_faces && h.app.preferences.cut_preview_tint);
            assert!(!h.app.preferences.profile_operations);
            assert_eq!(h.app.preferences.section_hatch, SectionHatch::ByPart);
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences, applied);
            h.call("ui.prefs", json!({"profile_operations":false}));
            assert_eq!(h.app.preferences.persistent_ribbon_pressed, original.persistent_ribbon_pressed);
        }
    }

    #[test]
    fn graphics_style_dropdown_uses_the_real_indices_and_icon_masters() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.prefs", json!({"theme":theme,"language":"en"}));
            h.call("ui.prefs", json!({"open":"Graphics"}));
            for (index, (_, label)) in crate::navigation_slots::VISUAL_STYLES.into_iter().enumerate() {
                click_graphics(&mut h, "Visual style", "prefs:visual_style");
                h.step(&json!({"click":{"handle":format!("dialog:Visual style:{label}")}})).unwrap();
                h.frames(3);
                assert_eq!(h.app.ui.visual_style, index as u8);
                let icon = crate::navigation_slots::visual_style_icon(index as u8);
                let painted = crate::scenario::rect_of(&format!("dialog:dropdown_icon:{icon}")).unwrap();
                assert!(crate::scenario::rect_of("prefs:visual_style").unwrap().contains_rect(painted));
            }
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.ui.visual_style, 0);
        }
    }

    #[test]
    fn graphics_style_popup_owns_escape_and_leaves_snapshot_cancel_to_the_next_escape() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1100}));
            h.call("ui.prefs", json!({"language":"en","theme":theme}));
            let before = h.app.preferences.clone();
            h.call("ui.prefs", json!({"open":"Graphics","dark_parts_shading":true}));
            let edited = h.app.preferences.clone();
            assert_ne!(before, edited);
            reveal_preference(&mut h, "prefs:visual_style");
            h.step(&json!({"click":{"handle":"prefs:visual_style"}})).unwrap();
            h.frames(2);
            let popup = "dialog:Visual style:Wireframe";
            assert!(crate::scenario::handle_at(popup).is_some());
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.prefs_window.shell.open && h.app.prefs_window.shell.section == PAGE_GRAPHICS);
            assert!(h.app.prefs_window.shell.snapshot().is_some());
            assert_eq!(h.app.preferences, edited, "first Escape retains unapplied Graphics edits");
            assert!(crate::scenario::handle_at(popup).is_none());
            h.step(&json!({"key":"Enter"})).unwrap();
            h.frames(2);
            assert!(h.app.prefs_window.shell.open && crate::scenario::handle_at(popup).is_some(), "Enter reopens the focused style trigger");
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.prefs_window.shell.open && h.app.prefs_window.shell.snapshot().is_some());
            assert!(crate::scenario::handle_at(popup).is_none());
            assert_eq!(h.app.preferences, edited);
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(!h.app.prefs_window.shell.open && h.app.prefs_window.shell.snapshot().is_none());
            assert_eq!(h.app.preferences, before, "Escape without a popup performs normal Preferences Cancel");
        }
    }

    #[test]
    fn graphics_disabled_controls_keep_stored_values_and_msaa_waits_for_restart() {
        use egui::accesskit::Role;
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            let dir = std::env::temp_dir().join(format!("solidcraft-graphics-cards-{}-{theme}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("graphics.json");
            std::fs::write(&path, r#"{"backend":"vulkan","msaa":4}"#).unwrap();
            h.app.services.graphics_path = Some(path.to_string_lossy().into_owned());
            h.app.msaa_supported = vec![1, 2, 4];
            let active_samples = h.app.viewport.gpu.as_ref().map(|gpu| gpu.samples);
            h.call("ui.prefs", json!({"theme":theme,"language":"en","frame_rate_details":true}));
            h.call("ui.prefs", json!({"open":"Graphics"}));
            click_graphics(&mut h, "Anti-aliasing", "prefs:msaa:8");
            assert_eq!(h.app.preferences.msaa, 4, "an unavailable card cannot coerce MSAA");
            assert!(h.widget("Restart needed").is_some());
            let tree = h.output.accesskit_update.as_ref().unwrap();
            assert!(tree.nodes.iter().any(|(_, node)| node.role() == Role::RadioButton && node.label() == Some("8×") && node.is_disabled()));
            click_graphics(&mut h, "Anti-aliasing", "prefs:msaa:2");
            assert_eq!(h.app.preferences.msaa, 2);
            let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved["msaa"], 2);
            assert_eq!(saved["backend"], "vulkan");
            assert_eq!(
                h.app.viewport.gpu.as_ref().map(|gpu| gpu.samples),
                active_samples,
                "changing the saved level leaves the running renderer alone"
            );
            click_graphics(&mut h, "Only when the view is still", "prefs:supersampling_adaptive");
            assert!(h.app.preferences.supersampling_adaptive, "disabled adaptive control retains its saved value");
            click_graphics(&mut h, "Frame rate details", "prefs:frame_rate_details");
            assert!(h.app.preferences.frame_rate_details, "visible disabled details control retains its saved value");
            let tree = h.output.accesskit_update.as_ref().unwrap();
            assert!(
                tree.nodes.iter().any(|(_, node)| node.role() == Role::CheckBox && node.label() == Some("Frame rate details") && node.is_disabled())
            );
            click_graphics(&mut h, "Frame rate", "prefs:show_frame_rate");
            click_graphics(&mut h, "Frame rate details", "prefs:frame_rate_details");
            assert!(!h.app.preferences.frame_rate_details, "enabling Frame rate enables its details checkbox");
            h.call("ui.prefs", json!({"button":"cancel"}));
            assert_eq!(h.app.preferences.msaa, 4);
            assert!(h.app.preferences.frame_rate_details);
            let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved["msaa"], 4, "Cancel restores startup persistence too");
            assert_eq!(saved["backend"], "vulkan");
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn graphics_page_keeps_the_untouched_radian_default_and_unpaged_reset_exclusions() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.prefs", json!({"theme":theme,"language":"en","vertex_markers":true,"grid_follows_view":true}));
            h.call("ui.prefs", json!({"open":"Graphics"}));
            for (perspective, auto_ortho) in [(false, false), (true, false), (false, true)] {
                h.app.ui.perspective = perspective;
                h.app.ui.auto_ortho_faces = auto_ortho;
                h.app.prefs_window.shell.filter = "Field of view".into();
                h.frames(5);
                assert_eq!(h.app.preferences.perspective_fov, 0.6, "drawing a whole-degree readout never rounds the default");
            }
            for page in 0..PAGES.len() {
                defaults(&mut h.app, page);
                assert!(h.app.preferences.vertex_markers && h.app.preferences.grid_follows_view, "{page}: neither unpaged field joins resets");
            }
            assert_eq!(h.app.preferences.perspective_fov, 0.6);
        }
    }

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
        assert!(!restored.preferences.ambient_occlusion, "a change that was not applied is not saved (#9730)");
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
        // The dialog is closed now: what is saved is the live state, quality included (#9730).
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
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

#[cfg(test)]
mod perspective_fov_tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, pos2};

    fn press(h: &mut crate::scenario::Harness, at: egui::Pos2, pressed: bool) {
        h.app.synthetic.push(Event::PointerMoved(at));
        h.app.synthetic.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.frames(3);
    }

    fn graphics_slider(h: &mut crate::scenario::Harness) -> egui::Rect {
        h.call("ui.prefs", json!({"open": "Graphics"}));
        let label = solidcraft_ui_text::tr("prefs.perspective_fov");
        reveal_preference(h, &format!("dialog:slider:{label}"));
        crate::scenario::rect_of(&format!("dialog:slider_track:{label}")).expect("the Field of view slider")
    }

    /// Graphics › Field of view (#5364): the kit slider in whole degrees, dragged or typed, written
    /// back in radians. While the view can't draw in perspective (Perspective view off, and no
    /// perspective-while-orbiting mode, #2682) it changes nothing, and Cancel puts it back.
    #[test]
    fn the_field_of_view_slider_drags_and_types_whole_degrees() {
        let mut h = crate::scenario::Harness::new();
        for (perspective, auto_ortho) in [(false, false), (true, false), (false, true)] {
            h.call("ui.set", json!({"perspective": perspective}));
            h.app.ui.auto_ortho_faces = auto_ortho;
            let track = graphics_slider(&mut h);
            press(&mut h, pos2(track.left() + 4.0, track.center().y), true);
            h.app.synthetic.push(Event::PointerMoved(pos2(track.left() + track.width() * 0.75, track.center().y)));
            h.frames(3);
            press(&mut h, pos2(track.left() + track.width() * 0.75, track.center().y), false);
            let degrees = h.app.preferences.perspective_fov.to_degrees();
            if perspective || auto_ortho {
                assert!((degrees - degrees.round()).abs() < 1e-9 && (35.0..=90.0).contains(&degrees), "{degrees}");
            } else {
                assert_eq!(h.app.preferences.perspective_fov, PERSPECTIVE_FOV, "disabled while Perspective view is off");
            }
            h.call("ui.prefs", json!({"button": "cancel"}));
            assert_eq!(h.app.preferences.perspective_fov, PERSPECTIVE_FOV, "Cancel restores the angle");
        }
        let track = graphics_slider(&mut h);
        press(&mut h, pos2(track.right() + 30.0, track.center().y), true);
        press(&mut h, pos2(track.right() + 30.0, track.center().y), false);
        h.app.synthetic.push(Event::Text("40.4".into()));
        h.app.synthetic.push(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.frames(3);
        assert!(
            (h.app.preferences.perspective_fov - 40f64.to_radians()).abs() < 1e-9,
            "typed 40.4 keeps to 40°: {}",
            h.app.preferences.perspective_fov
        );
        h.call("ui.prefs", json!({"button": "ok"}));
    }

    /// `ui.prefs {perspective_fov}` takes 15° to 90° in radians and refuses anything else, changing
    /// nothing; a saved value the view can't use (a hand edit) draws at the default, never a broken view.
    #[test]
    fn hostile_angles_are_refused_or_drawn_at_the_default() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"perspective": true}));
        for fine in [15f64.to_radians(), 90f64.to_radians(), 1.0] {
            assert_eq!(h.call("ui.prefs", json!({"perspective_fov": fine}))["ok"], true, "{fine}");
            assert_eq!(h.app.view_fov(), fine);
        }
        let before = (h.app.preferences.clone(), h.app.cam);
        for bad in [
            json!(-1.0),
            json!(0.0),
            json!(14.9f64.to_radians()),
            json!(90.1f64.to_radians()),
            json!(std::f64::consts::PI),
            json!(1e308),
            json!(-1e308),
            json!("x"),
            json!(null),
            json!([1]),
        ] {
            let r = h.call("ui.prefs", json!({"perspective_fov": bad}));
            assert_eq!(r["ok"], false, "{bad}: {r}");
            assert_eq!((h.app.preferences.clone(), h.app.cam), before, "{bad} changes nothing");
        }
        for saved in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 0.1, 2.0, std::f64::consts::PI, 1e308] {
            h.app.preferences.perspective_fov = saved;
            h.frames(2);
            assert_eq!(h.app.view_fov(), PERSPECTIVE_FOV, "{saved} draws at the default");
            assert!(h.app.cam.is_valid(), "{saved}: {:?}", h.app.cam);
        }
        // A fuzz pass: any bit pattern, and values spread over -1…3 rad, through `ui.prefs` and saved.
        let range = 15f64.to_radians()..=90f64.to_radians();
        let mut x: u64 = 0x5364;
        for i in 0..2000 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let v = if i % 2 == 0 { f64::from_bits(x) } else { (x >> 11) as f64 / (1u64 << 53) as f64 * 4.0 - 1.0 };
            let accepted = set(&mut h.app, &json!({"perspective_fov": v})).is_ok();
            assert_eq!(accepted, range.contains(&v), "{v}");
            h.app.preferences.perspective_fov = v;
            follow_view_fov(&mut h.app);
            assert!(range.contains(&h.app.view_fov()) && h.app.cam.is_valid(), "{v}: {:?}", h.app.cam);
        }
        let read = lenient(&json!({"perspective_fov": "wide", "ui_scale": 1.5}));
        assert_eq!((read.perspective_fov, read.ui_scale), (PERSPECTIVE_FOV, 1.5), "an unreadable angle costs that setting only");
    }

    /// The target keeps its size when the angle changes, by every path (`ui.prefs`, Cancel, Restore
    /// Defaults), and a parallel view or a sketch keeps the last perspective angle until it's back.
    #[test]
    fn the_view_keeps_its_size_when_the_angle_changes() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.call("ui.set", json!({"perspective": true}));
        h.frames(3);
        let half = |h: &crate::scenario::Harness| {
            let mut cam = h.app.cam;
            cam.fov = h.app.view_fov();
            cam.half_height()
        };
        let (size, distance) = (half(&h), h.app.cam.distance);
        h.call("ui.prefs", json!({"open": "Graphics"}));
        for fov in [15f64.to_radians(), 1.0, 90f64.to_radians()] {
            h.call("ui.prefs", json!({"perspective_fov": fov}));
            assert!((half(&h) / size - 1.0).abs() < 1e-9, "{fov}: {} vs {size}", half(&h));
        }
        h.call("ui.prefs", json!({"button": "cancel"}));
        assert!((h.app.cam.distance / distance - 1.0).abs() < 1e-9, "Cancel restores the angle and the size");
        h.call("ui.prefs", json!({"perspective_fov": 1.2}));
        h.call("ui.prefs", json!({"open": "Graphics", "button": "defaults"}));
        h.call("ui.prefs", json!({"button": "ok"}));
        assert_eq!(h.app.preferences.perspective_fov, PERSPECTIVE_FOV);
        assert!(!h.app.ui.perspective, "the page's Restore Defaults turns Perspective view off too");
        // A parallel view keeps the last perspective angle: back in perspective, the default angle
        // draws at the first size, and an angle changed while it's off is followed once it's on.
        h.call("ui.set", json!({"perspective": true}));
        h.frames(2);
        assert!((h.app.cam.distance / distance - 1.0).abs() < 1e-9, "Restore Defaults restores the size");
        h.call("ui.set", json!({"perspective": false}));
        h.frames(2);
        let parallel = h.app.cam.distance;
        h.call("ui.prefs", json!({"perspective_fov": 1.2}));
        assert_eq!(h.app.cam.distance, parallel, "parallel: nothing to rescale yet");
        h.call("ui.set", json!({"perspective": true}));
        h.frames(2);
        assert!((half(&h) / size - 1.0).abs() < 1e-9);
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.call("ui.prefs", json!({"perspective_fov": 0.6}));
        h.app.finish_sketch();
        h.frames(2);
        assert!((half(&h) / size - 1.0).abs() < 1e-9);
    }

    /// A view change that is animating when the angle changes ends at the same size too: both its
    /// ends are rescaled with the camera.
    #[test]
    fn an_animating_view_change_is_rescaled_too() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"perspective": true}));
        h.frames(2);
        let to = solidcraft_viewport::Camera { distance: 300.0, ..h.app.cam };
        h.app.cam_anim = Some(solidcraft_viewport::CameraAnim::new(h.app.cam, to, h.app.now));
        let from = h.app.cam;
        set(&mut h.app, &json!({"perspective_fov": 1.0})).unwrap();
        let k = (0.6f64 * 0.5).tan() / 0.5f64.tan();
        let anim = h.app.cam_anim.as_ref().expect("still animating");
        assert!((anim.to.distance - 300.0 * k).abs() < 1e-9 && (anim.from.distance - from.distance * k).abs() < 1e-9, "{:?}", (anim.from, anim.to));
    }

    /// A script's Cancel puts the view's size back at once, before any frame runs, so a `ui.camera`
    /// read straight after it gives the restored view.
    #[test]
    fn cancel_restores_the_size_before_the_next_frame() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"perspective": true}));
        h.frames(2);
        let distance = h.app.cam.distance;
        let mut raw = |method: &str, params: serde_json::Value| {
            let (req, _rx) = crate::control::ControlRequest::new(method, params);
            match crate::control::handle(&mut h.app, &h.ctx, &req) {
                crate::control::Outcome::Done(v) => v,
                _ => serde_json::Value::Null,
            }
        };
        raw("ui.prefs", json!({"open": "Graphics", "perspective_fov": 1.2}));
        raw("ui.prefs", json!({"button": "cancel"}));
        let cam = raw("ui.camera", serde_json::Value::Null);
        assert!((cam["result"]["distance"].as_f64().unwrap() / distance - 1.0).abs() < 1e-9, "{cam}");
    }

    /// The angle is kept between runs.
    #[test]
    fn the_angle_is_kept_across_a_restart() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"perspective_fov": 1.1}));
        h.step(&json!({"restart": "close"})).unwrap();
        assert_eq!(h.app.preferences.perspective_fov, 1.1);
    }
}

// SolidCraft's Preferences window drives the kit's settings search; these run the whole app.
#[cfg(test)]
mod settings_search_tests {
    use crate::ui_kit::settings_search::*;

    #[test]
    fn search_matches_a_label_a_subsequence_and_the_english_source() {
        crate::i18n::install();
        assert!(matches_text("zoom", &["Reverse zoom direction"]));
        assert!(matches_text("zo", &["Reverse zoom direction"]));
        assert!(!matches_text("rzd", &["Reverse zoom direction"]));
        assert!(!matches_text("q", &["Units of new designs"]));
        assert!(!matches_text("qqqq", &["Units of new designs"]));
        let _language = solidcraft_ui_text::scope("fr");
        assert!(matches_text("units", &["Unités des nouvelles conceptions"]));
        assert!(matches_text("preferences", &["préférences"]));
        assert!(matches_text("dark", &["Theme"]));
        assert!(matches_text("antialiasing", &["Anti-aliasing"]));
        assert!(!matches_text("dark", &["Units of new designs"]));
    }

    /// A multi-word right-to-left label reaches the search in display order while the query is
    /// typed in reading order: typing the label's words as written must find the row (#10471).
    #[test]
    fn search_finds_right_to_left_labels_typed_in_reading_order() {
        crate::i18n::install();
        let _language = solidcraft_ui_text::scope("he");
        let label = solidcraft_ui_text::tr("prefs.bodies_folders");
        assert!(matches_text("תיקיות גופים", &[label]), "the label as it is typed");
        assert!(matches_text("גופים תיקיות", &[label]), "the words swapped still match");
        assert!(matches_text("תיקיות", &[label]), "one word still matches");
        let _language = solidcraft_ui_text::scope("ar");
        let label = solidcraft_ui_text::tr("prefs.bodies_folders");
        assert!(matches_text("مجلدات الأجسام", &[label]), "the label as it is typed");
        assert!(matches_text("الأجسام مجلدات", &[label]), "the words swapped still match");
        assert!(matches_text("مجلدات", &[label]), "one word still matches");
    }

    #[test]
    fn the_search_field_names_the_window_it_searches() {
        crate::i18n::install();
        let name = solidcraft_ui_text::tr("ui.preferences").to_lowercase();
        for id in ["prefs.search", "prefs.search_empty"] {
            let text = solidcraft_ui_text::tr(id).to_lowercase();
            assert!(text.contains(&name), "`{text}` should name the window `{name}`");
        }
    }

    #[test]
    fn sketch_short_labels_keep_the_legacy_search_terms_in_every_complete_locale() {
        crate::i18n::install();
        let cases = [
            ("prefs.sketch.tracking_guides", "Center and midpoint alignment guides"),
            ("prefs.sketch.tracking_points", "Alignment guides from end points and corners while drawing"),
            ("prefs.sketch.recent_snap", "Auto snap to recent centers and midpoints"),
            ("prefs.sketch.align_points", "Line up with other sketch points while drawing"),
            ("prefs.sketch.center_arc", "Center point arc follows the cursor's direction"),
            ("prefs.sketch.auto_locate", "Also place them from the origin (fully constrained)"),
            ("prefs.sketch.shortcut_cycles_modes", "Pressing a tool's shortcut again switches its mode"),
            ("prefs.sketch.project_options", "Project options dialog"),
            ("prefs.sketch.exit_previous", "Return to the previous view"),
            ("prefs.sketch.exit_keep", "Keep the current view"),
            ("prefs.sketch.exit_unless_rotated", "Return unless rotated"),
            ("prefs.sketch.state_icons", "Sketch icons in the color of their state"),
            ("prefs.sketch.conflict_colour", "Conflicting geometry in the conflict color"),
            ("prefs.sketch.hide_glyphs", "Hide constraint glyphs when zoomed out"),
            ("prefs.sketch.prefer_free_points", "Prefer movable sketch points when points overlap"),
            ("prefs.sketch.select_other_hidden", "Select hidden geometry with Select Other"),
            ("prefs.sketch.whole_bodies", "Click selects whole bodies"),
        ];
        for lang in ["en", "en-GB", "ar", "de", "he", "ja", "ko", "vi", "zh"] {
            let _language = solidcraft_ui_text::scope(lang);
            for (id, legacy) in cases {
                assert!(matches_text(legacy, &[solidcraft_ui_text::tr(id)]), "{lang}: {legacy} still finds {id}");
            }
        }
    }

    #[test]
    fn theme_picker_names_and_controls_are_searchable_and_theme_results_reveal_real_cards() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", serde_json::json!({"width":1467,"height":1146}));
        h.call("ui.prefs", serde_json::json!({"language":"en","theme":"system","open":"Appearance"}));
        for label in
            ["Onyx", "Follow system", "One theme", "Carousel", "Grid", "Day theme", "Night theme", "High contrast", "Import theme file…", "Imported"]
        {
            h.app.prefs_window.shell.filter = label.to_owned();
            h.frames(5);
            let (_, _, hits) = nav(&h.ctx);
            let matches: Vec<_> =
                hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_APPEARANCE && hit.label == label).collect();
            assert_eq!(
                matches.len(),
                1,
                "{label} has one searchable Appearance control: {:?}",
                hits.iter().map(|hit| (&hit.label, hit.page)).collect::<Vec<_>>()
            );
            if label == "Onyx" {
                set_cursor(&h.ctx, matches[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                h.frames(25);
                let page = crate::scenario::rect_of("prefs:page_body").unwrap();
                let target = crate::scenario::rect_of("prefs:theme:onyx").unwrap();
                assert!(page.contains_rect(target));
                assert_eq!(h.app.preferences.theme, "system", "revealing a theme never selects it");
            }
        }
    }

    #[test]
    fn graphics_cards_tiles_and_short_status_labels_keep_search_paths_and_reveal() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", serde_json::json!({"theme":theme,"language":"en"}));
            h.call("ui.prefs", serde_json::json!({"open":"Graphics"}));
            for (query, label, handle) in [
                ("Display quality", "Curved surfaces", "prefs:display_quality:normal"),
                ("MSAA", "Anti-aliasing", "prefs:msaa:4"),
                ("Ambient occlusion", "Ambient occlusion", "prefs:ambient_occlusion"),
                ("Profile operations", "Profile operations", "prefs:profile_operations"),
                ("Quick measurement in the view's corner", "Quick measurement", "prefs:quick_measure"),
                ("Show viewport analysis status", "Analysis", "prefs:viewport_analysis_status"),
                ("Show viewport section status", "Section", "prefs:viewport_section_status"),
                ("Live viewport details", "Live details", "prefs:viewport_status_details"),
                ("Fold viewport status items", "Fold items", "prefs:viewport_status_summary"),
            ] {
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> =
                    hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_GRAPHICS && hit.label == label).collect();
                assert_eq!(matching.len(), 1, "{theme}: {query} keeps one real Graphics search target");
                let index = matching[0].0;
                set_cursor(&h.ctx, index);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                // Enter restores the full page; let its normal scroll animation settle.
                h.frames(20);
                assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_GRAPHICS);
                assert!(h.app.prefs_window.shell.filter.is_empty());
                let page = crate::scenario::rect_of("dialog:page_body").unwrap();
                let target = crate::scenario::rect_of(handle).unwrap();
                assert!(page.contains_rect(target), "{query} reveals the real control: page={page:?}, target={target:?}");
            }
        }
    }

    #[test]
    fn appearance_controls_keep_one_search_result_and_their_page() {
        use egui::accesskit::Role;
        for width in [520, 1600] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":width,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","open":"Appearance"}));
            for (query, label, row_target, native) in [
                ("3D view colors", "3D view colors", "dialog:row:3D view colors", None),
                ("Interface scale", "Interface scale", "dialog:row:Interface scale", Some((Role::Slider, "Interface scale", "prefs:scale"))),
                ("Follow a theme file", "Follow a theme file", "prefs:theme_follow_toggle", None),
                ("Background", "Background", "dialog:row:Background", Some((Role::Button, "Color", "dialog:Background:Color"))),
                ("Font", "Font", "dialog:row:Font", Some((Role::ComboBox, "Interface font", "prefs:ui_font"))),
            ] {
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
                assert_eq!(matching.len(), 1, "width={width}: {query} needs exactly one real navigation hit");
                assert_eq!(matching[0].page, crate::prefs::PAGE_APPEARANCE);
                // The search field also contains the query as its accessible value. Use the
                // indexed row so this check measures the result rather than the search field.
                let row = crate::scenario::rect_of(row_target).unwrap();
                let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                assert!(body.contains(row.center()), "width={width}: {query} row {row:?} outside {body:?}");
                if let Some((role, name, target)) = native {
                    let tree = h.output.accesskit_update.as_ref().unwrap();
                    let controls: Vec<_> =
                        tree.nodes.iter().filter(|(_, node)| !node.is_disabled() && node.role() == role && node.label() == Some(name)).collect();
                    assert_eq!(controls.len(), 1, "width={width}: {query} must expose one enabled native {name} control");
                    let bounds = controls[0].1.bounds().unwrap();
                    let centre = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
                    let rect = crate::scenario::rect_of(target).unwrap();
                    assert!(body.contains_rect(rect) && rect.contains(centre), "width={width}: {name} is visible at its real target");
                    if name == "Interface scale" {
                        let row = crate::scenario::rect_of("dialog:row:Interface scale").unwrap();
                        assert!((rect.left() - row.left()).abs() <= 1.0, "accessible-only name reserves no label column");
                    }
                    if name == "Color" {
                        h.step(&serde_json::json!({"click":{"handle":target}})).unwrap();
                        h.frames(2);
                        assert_eq!(h.app.preferences.viewport_background, Some([255, 255, 255]), "the search hit edits the real setting");
                    }
                    if name == "Interface font" {
                        h.step(&serde_json::json!({"click":{"handle":target}})).unwrap();
                        h.frames(2);
                        assert!(crate::scenario::handle_at("prefs:font_builtin").is_some(), "real searchable font picker opens at either width");
                    }
                }
            }
        }
    }

    #[test]
    fn layout_controls_keep_one_real_search_target_and_native_names() {
        use egui::accesskit::Role;
        for width in [600, 1600] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":width,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","open":"Layout"}));
            for (query, native) in [
                ("Components start open", Some((Role::CheckBox, "prefs:browser_components_open"))),
                ("Preview panel placement while dragging", Some((Role::CheckBox, "prefs:live_panel_drag_preview"))),
                ("Menus", None),
                ("Highlight active commands", Some((Role::CheckBox, "prefs:persistent_ribbon_pressed"))),
                ("Show navigation bar", Some((Role::CheckBox, "prefs:navbar:show"))),
                ("Position", None),
            ] {
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> = hits.iter().filter(|hit| hit.page == crate::prefs::PAGE_LAYOUT && hit.label == query).collect();
                assert_eq!(matching.len(), 1, "{width}: {query} has one real Layout target");
                if let Some((role, target)) = native {
                    let tree = h.output.accesskit_update.as_ref().unwrap();
                    let nodes: Vec<_> =
                        tree.nodes.iter().filter(|(_, node)| node.role() == role && node.label() == Some(query) && !node.is_disabled()).collect();
                    assert_eq!(nodes.len(), 1, "{query} retains its native accessible name");
                    let bounds = nodes[0].1.bounds().unwrap();
                    let centre = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
                    assert!(crate::scenario::rect_of(target).unwrap().contains(centre));
                }
            }
        }
    }

    /// A labelled row whose control reports no match of its own (a plain number field, as
    /// Sketch › Window steps has) drew when its label matched, but never counted as a result: the
    /// page heading stayed away and the window said "No matching preferences". It is a result now,
    /// with one stop for the arrow keys.
    #[test]
    fn a_matching_row_with_a_plain_number_field_is_a_result() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_SKETCH);
        for (query, label) in [("window steps", "Window steps"), ("dimension values", "Dimension values")] {
            h.app.prefs_window.shell.filter = query.into();
            h.frames(4);
            assert!(h.widget(h.app.tr("prefs.search_empty")).is_none(), "{query} has a result instead of the empty-results message");
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            let at = h.widget(label).unwrap_or_else(|| panic!("{label} must match {query}"));
            assert!(body.contains(egui::pos2(at[0] as f32, at[1] as f32)), "{label} is drawn in the results");
            assert!(h.widget("Sketch").is_some(), "{query}: the page heading names where it is");
            // The Drawing section's hidden checkbox columns leave no gap between heading and row.
            let heading = h.widget("DRAWING").unwrap_or_else(|| panic!("{query}: the section heading is drawn"));
            assert!(at[1] - heading[1] < 80.0, "{label} sits under its section heading, not {} points below", at[1] - heading[1]);
            let (_, _, hits) = nav(&h.ctx);
            let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
            assert_eq!(matching.len(), 1, "{label} must have one stop for the arrow keys: {:?}", hits.iter().map(|h| &h.label).collect::<Vec<_>>());
            assert_eq!(matching[0].page, crate::prefs::PAGE_SKETCH);
        }
    }

    /// Enter on such a row opens its page, scrolls to the row and outlines it.
    #[test]
    fn enter_on_a_matching_row_with_a_plain_number_field_reveals_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_SKETCH);
        h.app.prefs_window.shell.filter = "window steps".into();
        h.frames(4);
        let (_, _, hits) = nav(&h.ctx);
        let at = hits.iter().position(|hit| hit.label == "Window steps").expect("Window steps is a stop");
        set_cursor(&h.ctx, at);
        h.frames(2);
        h.step(&serde_json::json!({"key":"Enter"})).unwrap();
        h.frames(30);
        assert!(h.app.prefs_window.shell.filter.is_empty(), "Enter clears the search");
        let row = crate::scenario::rect_of("dialog:row:Window steps").expect("the row is drawn");
        let revealed = crate::scenario::handle_at("prefs:revealed").expect("Enter outlines the row");
        assert!(revealed.distance(row.center()) <= 1.0, "the outline is on Window steps");
    }

    /// A row found only by its hint is a stop for the arrow keys too, named by its label.
    #[test]
    fn a_row_found_only_by_its_hint_has_a_stop() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_SKETCH);
        // Only Dimension values' hint says this; its label and options do not.
        h.app.prefs_window.shell.filter = "arrows that step".into();
        h.frames(4);
        let (_, _, hits) = nav(&h.ctx);
        let matching: Vec<_> = hits.iter().filter(|hit| hit.label == "Dimension values").collect();
        assert_eq!(matching.len(), 1, "one stop: {:?}", hits.iter().map(|h| &h.label).collect::<Vec<_>>());
        assert_eq!(matching[0].page, crate::prefs::PAGE_SKETCH);
    }

    #[test]
    fn general_redesigned_controls_remain_searchable_without_duplicate_settings() {
        use egui::accesskit::Role;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        for (query, label) in [
            ("reopen the designs", "Reopen the designs that were open last time"),
            ("autosave every", "Autosave every"),
            ("largest file to open: step", "Largest file to open: STEP and import"),
            ("largest file to open: design", "Largest file to open: Design"),
            ("allow network locations", "Allow network locations in designs"),
            ("slicer that is already running", "Open files in a slicer that is already running"),
            ("print face highlight", "Print face highlight"),
            ("keep print faces", "Keep print faces"),
            ("interface language", "Interface language"),
        ] {
            h.app.prefs_window.shell.filter = query.into();
            h.frames(4);
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            let at = h.widget(label).unwrap_or_else(|| panic!("{label} must match {query}"));
            assert!(body.contains(egui::pos2(at[0] as f32, at[1] as f32)), "{label} is drawn in the results");
            assert!(h.widget(h.app.tr("prefs.search_empty")).is_none(), "{query} has a real result instead of the empty-results message");
            let (_, _, hits) = nav(&h.ctx);
            let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
            assert_eq!(matching.len(), 1, "{label} must have one real search result");
            assert_eq!(matching[0].page, crate::prefs::PAGE_GENERAL);
            let control = match label {
                "Autosave every" => Some(("prefs:autosave_minutes", Role::SpinButton, "Minutes", h.app.autosave_minutes)),
                "Largest file to open: STEP and import" => {
                    Some(("prefs:max_step_file_mb", Role::SpinButton, label, h.app.preferences.max_step_file_mb as f64))
                }
                "Largest file to open: Design" => {
                    Some(("prefs:max_design_file_mb", Role::SpinButton, label, h.app.preferences.max_design_file_mb as f64))
                }
                "Print face highlight" => {
                    Some(("prefs:print_face_highlight", Role::Slider, label, f64::from(h.app.preferences.print_face_highlight)))
                }
                _ => None,
            };
            if let Some((identity, role, accessible_label, value)) = control {
                let field = crate::scenario::rect_of(identity).expect("the matching field retains its stable identity");
                assert!(body.contains_rect(field), "{identity} is drawn in the results");
                let tree = h.output.accesskit_update.as_ref().unwrap();
                // Search also measures invisible, disabled probe UIs. Count the live control.
                let controls: Vec<_> = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| !node.is_disabled() && node.role() == role && node.label() == Some(accessible_label))
                    .collect();
                assert_eq!(controls.len(), 1, "{identity} must have one real searchable control");
                let node = &controls[0].1;
                assert_eq!(node.numeric_value(), Some(value), "{identity} exposes its actual setting value");
                let bounds = node.bounds().unwrap();
                assert!(field.contains(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)));
            }
        }
        // An unavailable control is still a real result. Invisible search probes are also
        // disabled, so require the live navigation hit and painted label as well as metadata.
        h.app.updater.kind = Some(crate::update::InstallKind::Dev);
        assert!(!crate::update::available(&mut h.app));
        let label = "Check at start and once a day";
        h.app.prefs_window.shell.filter = label.into();
        for checked in [false, true] {
            h.app.preferences.check_updates = checked;
            h.frames(4);
            let (_, _, hits) = nav(&h.ctx);
            let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
            assert_eq!(matching.len(), 1, "the disabled setting has one live result");
            assert_eq!(matching[0].page, crate::prefs::PAGE_GENERAL);
            assert!(h.widget(h.app.tr("prefs.search_empty")).is_none());
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            let field = crate::scenario::rect_of("prefs:check_updates").expect("the real scheduling control");
            assert!(body.contains_rect(field));
            let tree = h.output.accesskit_update.as_ref().unwrap();
            let (_, node) = tree
                .nodes
                .iter()
                .rev()
                .find(|(_, node)| {
                    node.role() == Role::CheckBox
                        && node.label() == Some(label)
                        && node.bounds().is_some_and(|bounds| {
                            field.contains(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
                        })
                })
                .expect("the disabled checkbox exposes its real state at the rendered control");
            assert!(node.is_disabled());
            assert_eq!(node.toggled(), Some(if checked { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }));
            let painted = h
                .shapes
                .iter()
                .filter(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label && body.contains(text.pos) && shape.clip_rect.contains(text.pos))
                })
                .count();
            assert_eq!(painted, 1, "one visible disabled control is painted, beyond probe metadata");
            h.step(&serde_json::json!({"click":{"handle":"prefs:check_updates"}})).unwrap();
            assert_eq!(h.app.preferences.check_updates, checked, "the search result cannot edit an unavailable setting");
        }
    }

    /// Typing focuses the field, Enter opens that setting, and Escape returns to the page
    /// that was up when the search started. Recording a shortcut keeps the key.
    #[test]
    fn preferences_search_types_opens_and_returns() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GRAPHICS);
        h.frames(2);
        h.step(&serde_json::json!({"key": "F", "cmd": true})).expect("find");
        h.frames(1);
        assert!(h.ctx.memory(|memory| memory.has_focus(crate::prefs::search_id())), "Cmd+F focuses search");
        h.step(&serde_json::json!({"text": "zo"})).expect("type");
        h.frames(20);
        assert!(h.widget("Reverse zoom direction").is_some(), "typed letters search from the open page");
        assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_GRAPHICS);
        let first = crate::scenario::handle_at("prefs:search:current");
        h.step(&serde_json::json!({"key": "ArrowDown"})).expect("next match");
        h.frames(1);
        let second = crate::scenario::handle_at("prefs:search:current");
        assert!(first.is_some() && second.is_some() && first != second, "Down moves to the next match");
        h.step(&serde_json::json!({"key": "ArrowUp"})).expect("previous match");
        h.frames(1);
        assert_eq!(crate::scenario::handle_at("prefs:search:current"), first, "Up moves back");
        let (_, _, hits) = nav(&h.ctx);
        let cursor = hits
            .iter()
            .position(|hit| hit.page == crate::prefs::PAGE_NAVIGATION && hit.label == "Reverse zoom direction")
            .expect("the Navigation setting remains keyboard reachable");
        set_cursor(&h.ctx, cursor);
        h.frames(2);
        h.step(&serde_json::json!({"key": "Enter"})).expect("open");
        h.frames(2);
        assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_NAVIGATION, "Enter opens the match's page");
        assert!(h.app.prefs_window.shell.filter.is_empty());
        assert!(h.widget("Reverse zoom direction").is_some());
        assert!(crate::scenario::handle_at("prefs:revealed").is_some(), "the opened row is highlighted");
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GRAPHICS);
        h.frames(2);
        h.step(&serde_json::json!({"text": "zo"})).expect("search again");
        h.frames(2);
        h.step(&serde_json::json!({"key": "Escape"})).expect("clear");
        h.frames(1);
        assert!(h.app.prefs_window.shell.filter.is_empty());
        assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_GRAPHICS, "Escape returns to the page from before the search");
    }

    #[test]
    fn sketch_picture_groups_and_options_have_one_real_search_hit_and_enter_reveals_the_cards() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":1600,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","theme":theme}));
            let original = (h.app.preferences.sketch_exit_view, h.app.ui.pick_bodies);
            for (query, target, option) in [
                ("When a sketch ends", "prefs:sketch_exit_view:choices", "Keep"),
                ("Return to the view before", "prefs:sketch_exit_view:previous", "Return to the view before"),
                ("Keep", "prefs:sketch_exit_view:keep", "Keep"),
                ("Unless turned", "prefs:sketch_exit_view:unless_rotated", "Unless turned"),
                ("Click selects", "prefs:pick_bodies:choices", "Whole bodies"),
                ("Faces and edges", "prefs:pick_bodies:faces_edges", "Faces and edges"),
                ("Whole bodies", "prefs:pick_bodies:bodies", "Whole bodies"),
            ] {
                h.call("ui.prefs", serde_json::json!({"open":"General"}));
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> =
                    hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_SKETCH && hit.label == query).collect();
                assert_eq!(matching.len(), 1, "{theme}: {query} has exactly one real Sketch setting hit");
                assert_eq!(matching[0].1.label, query);
                let tree = h.output.accesskit_update.as_ref().unwrap();
                assert!(
                    tree.nodes.iter().any(|(_, node)| {
                        node.role() == egui::accesskit::Role::RadioButton && node.label() == Some(option) && !node.is_disabled()
                    }),
                    "{query} exposes the enabled native card"
                );
                set_cursor(&h.ctx, matching[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                // Enter restores the full page; allow its scroll animation to settle.
                h.frames(20);
                assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_SKETCH);
                assert!(h.app.prefs_window.shell.filter.is_empty());
                let body = crate::scenario::rect_of("dialog:page_body").unwrap();
                let actual = crate::scenario::rect_of(target).unwrap();
                let revealed = crate::scenario::handle_at("prefs:revealed").expect("Enter publishes an actual reveal target");
                assert!(body.contains_rect(actual), "{theme}: {query} reveals the actual cards inside the body");
                assert!(revealed.distance(actual.center()) <= 1.0, "{theme}: {query} outlines its real group/card");
                if target.ends_with(":choices") {
                    let group = crate::scenario::rect_of(&format!("dialog:row:{query}")).unwrap();
                    assert!((actual.left() - group.left()).abs() <= 1.0, "{theme}: {query} has no phantom label column beside its cards");
                }
                assert_eq!((h.app.preferences.sketch_exit_view, h.app.ui.pick_bodies), original, "navigation does not change a choice");
            }
        }
    }

    /// Typing in a number field (the largest STEP file) edits that field instead of starting
    /// a search.
    #[test]
    fn preferences_number_fields_keep_typed_text() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(2);
        crate::prefs::reveal_preference(&mut h, "prefs:max_step_file_mb");
        let field = crate::scenario::rect_of("prefs:max_step_file_mb").expect("STEP limit field");
        assert!(crate::scenario::rect_of("prefs:page_body").expect("Preferences body").contains_rect(field));
        h.step(&serde_json::json!({"click": [field.center().x, field.center().y]})).expect("click the field");
        h.frames(2);
        let focused = h.ctx.memory(|memory| memory.focused()).expect("the number editor has focus");
        assert!(field.contains_rect(h.ctx.read_response(focused).expect("focused editor response").rect));
        h.step(&serde_json::json!({"text": "7"})).expect("type");
        h.frames(2);
        assert!(h.app.prefs_window.shell.filter.is_empty(), "typed `{}` into the search", h.app.prefs_window.shell.filter);
    }

    /// The selected match is scrolled to when the arrows move it, not on every frame, so the
    /// wheel can scroll the results away from it.
    #[test]
    fn navigation_and_mouse3d_search_hits_reveal_the_actual_cards_and_controls() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":1600,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","theme":theme}));
            for (query, page, target) in [
                ("Orbit around", crate::prefs::PAGE_NAVIGATION, "prefs:orbit_around:View center"),
                ("Under the cursor", crate::prefs::PAGE_NAVIGATION, "prefs:orbit_around:Under the cursor"),
                ("Origin", crate::prefs::PAGE_NAVIGATION, "prefs:orbit_around:Origin"),
                ("View navigator", crate::prefs::PAGE_NAVIGATION, "prefs:navigator:Signed axes"),
                ("Axes", crate::prefs::PAGE_NAVIGATION, "prefs:navigator:Signed axes"),
                ("Cube", crate::prefs::PAGE_NAVIGATION, "prefs:navigator:Cube"),
                ("Turntable", crate::prefs::PAGE_NAVIGATION, "prefs:turntable"),
                ("Wheel zoom speed", crate::prefs::PAGE_NAVIGATION, "prefs:wheel_zoom_speed"),
                ("Zoom with two-finger scroll while holding", crate::prefs::PAGE_NAVIGATION, "prefs:trackpad_zoom_modifier"),
                ("Overall speed", crate::prefs::PAGE_MOUSE3D, "prefs:space_mouse_speed"),
                ("Traced", crate::prefs::PAGE_MOUSE3D, "prefs:3d_mouse_orbit_traced"),
                ("Position", crate::prefs::PAGE_MOUSE3D, "prefs:space_mouse_ghost_position"),
            ] {
                h.call("ui.prefs", serde_json::json!({"open":"General"}));
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> =
                    hits.iter().enumerate().filter(|(_, hit)| hit.page == page && (query != "Cube" || hit.label == "Cube")).collect();
                assert_eq!(matching.len(), 1, "{theme}: {query} has one indexed setting");
                set_cursor(&h.ctx, matching[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                h.frames(20);
                assert_eq!(h.app.prefs_window.shell.section, page);
                assert!(h.app.prefs_window.shell.filter.is_empty());
                let body = crate::scenario::rect_of("dialog:page_body").unwrap();
                assert!(body.contains_rect(crate::scenario::rect_of(target).unwrap()), "{query} reveals its live control");
            }
        }
    }

    #[test]
    fn preferences_search_results_scroll_freely() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(2);
        h.app.prefs_window.shell.filter = "view".into();
        h.frames(20);
        let first = crate::scenario::handle_at("prefs:search:current").expect("first match");
        let wheel = egui::Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        // The wheel scrolls smoothly: give it time to come to rest before each check.
        h.frame_with(vec![egui::Event::PointerMoved(first), wheel.clone()]);
        h.frames(60);
        let scrolled = crate::scenario::handle_at("prefs:search:current").expect("match still drawn");
        assert!(scrolled.y < first.y - 100.0, "the wheel scrolls past the first match ({} -> {})", first.y, scrolled.y);
        h.step(&serde_json::json!({"key": "ArrowDown"})).expect("next match");
        h.frames(60);
        let next = crate::scenario::handle_at("prefs:search:current").expect("next match");
        let body = crate::scenario::rect_of("dialog:page_body").expect("page body");
        assert!(body.contains(next), "Down brings the next match into view ({next:?} in {body:?})");
        h.frame_with(vec![egui::Event::PointerMoved(next), wheel]);
        h.frames(60);
        let after = crate::scenario::handle_at("prefs:search:current").expect("match still drawn");
        assert!(after.y < next.y - 100.0, "the wheel still scrolls after Down ({} -> {})", next.y, after.y);
    }

    #[test]
    fn preferences_search_filters_live_controls_across_pages() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(2);
        assert!(h.widget("Units of new designs").is_none(), "another page stays put until a search");
        h.app.prefs_window.shell.filter = "units".into();
        h.frames(20);
        assert!(h.widget("Units of new designs").is_some(), "a row is found from its label");
        assert!(h.widget("Reverse zoom direction").is_none());
        h.app.prefs_window.shell.filter = "zoom".into();
        h.frames(20);
        assert!(h.widget("Reverse zoom direction").is_some());
        assert!(h.widget("Units of new designs").is_none());
        assert!(h.widget("Grid").is_none(), "a checkbox on another page stays hidden");
        assert!(h.widget("Interface language").is_none(), "the page around a match stays hidden");
        assert!(h.widget("Advanced").is_none(), "a folded heading is not a match");
        let search = crate::scenario::handle_at("prefs:search").expect("search field");
        let hit = h.widget("Reverse zoom direction").expect("zoom row");
        let gap = hit[1] - f64::from(search.y);
        assert!(gap < 520.0, "search results leave a {gap:.0}px gap under the field");
        assert!(h.widget("Translation X").is_none(), "unmatched pages do not stack on the result");
        h.app.prefs_window.shell.filter = "extrude".into();
        h.frames(20);
        assert!(crate::scenario::rect_of("prefs:search:shortcuts").is_some(), "shortcut matches collapse to one row");
        assert!(crate::scenario::rect_of("dialog:cell:Extrude").is_none(), "the shortcut list stays closed");
        h.step(&serde_json::json!({"click": {"handle": "prefs:search:clear"}})).expect("clear");
        h.frames(2);
        assert!(h.app.prefs_window.shell.filter.is_empty(), "the search X clears the field");
        h.app.prefs_window.shell.filter = "fusion".into();
        h.frames(20);
        assert!(h.widget("Mouse mapping").is_some(), "the dropdown's current value counts");
        h.app.prefs_window.shell.filter = "zzzz-no-setting".into();
        h.frames(2);
        assert!(h.widget("No matching preferences").is_some());
        h.app.prefs_window.shell.filter = "units".into();
        h.frames(2);
        h.step(&serde_json::json!({"click": {"handle": "prefs:page:Design"}})).expect("design page");
        h.frames(2);
        assert!(h.app.prefs_window.shell.filter.is_empty(), "a page tab clears the search");
        assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_DESIGN);
        assert!(h.widget("Units of new designs").is_some(), "the tab shows its own page");
        assert!(crate::scenario::rect_of("prefs:search:shortcuts").is_none(), "shortcut matches leave with the search");
        h.app.prefs_window.shell.filter = "zoom".into();
        h.frames(2);
        h.step(&serde_json::json!({"click": {"handle": "prefs:page:Design"}})).expect("same page");
        h.frames(2);
        assert!(h.app.prefs_window.shell.filter.is_empty(), "the current tab also clears the search");
        assert!(h.widget("Reverse zoom direction").is_none());
        assert!(h.widget("Units of new designs").is_some());
    }

    /// Settings drawn in a row with no label of their own are found by the words they show (#9265).
    #[test]
    fn preferences_search_finds_settings_in_rows_without_a_label() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_LAYOUT);
        h.frames(2);
        // Drawn, not merely present: a hidden row's controls still exist, far off screen.
        let drawn = |h: &crate::scenario::Harness, label: &str| {
            let body = crate::scenario::rect_of("dialog:page_body");
            h.widget(label).is_some_and(|[x, y]| body.is_none_or(|body| body.contains(egui::pos2(x as f32, y as f32))))
        };
        let rows = [
            ("status bar", "Status bar"),
            ("sketch tab shown", "Keep the Sketch tab shown"),
            ("search results", "Show search results in the ribbon"),
            ("play history", "Play History button"),
            ("view's corner", "Also in the view's corner"),
            ("highlight active", "Highlight active commands"),
        ];
        for (query, label) in rows {
            h.app.prefs_window.shell.filter = query.into();
            h.frames(4);
            assert!(drawn(&h, label), "searching '{query}' should show '{label}'");
        }
        h.app.prefs_window.shell.filter = "status bar".into();
        h.frames(4);
        assert!(!drawn(&h, "Keep the Sketch tab shown"), "a row that does not match stays out of the result");
        // Three unlabelled rows sit under one labelled row: each is found on its own words only.
        h.app.prefs_window.shell.filter = "sketch tab shown".into();
        h.frames(4);
        assert!(drawn(&h, "Keep the Sketch tab shown"));
        assert!(!drawn(&h, "Highlight active commands"), "its neighbour is not a match");
        assert!(!drawn(&h, "Show search results in the ribbon"), "its neighbour is not a match");
        h.app.prefs_window.shell.filter = "zzzz-no-setting".into();
        h.frames(4);
        assert!(!drawn(&h, "Status bar"), "a miss shows nothing");
    }

    /// The Placed features row is found by the name of every feature that has the marker, not
    /// only by its own label.
    #[test]
    fn preferences_search_finds_placement_handles_by_each_placed_feature() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_TOOLS);
        h.frames(2);
        let drawn = |h: &crate::scenario::Harness, label: &str| {
            let body = crate::scenario::rect_of("dialog:page_body");
            h.widget(label).is_some_and(|[x, y]| body.is_none_or(|body| body.contains(egui::pos2(x as f32, y as f32))))
        };
        for query in ["Placed features", "Rest", "Boss", "Snap Fit", "Box", "Cylinder", "Cone", "Sphere", "Torus", "Coil"] {
            h.app.prefs_window.shell.filter = query.into();
            h.frames(4);
            assert!(drawn(&h, "Placement handles"), "searching '{query}' should show Placement handles");
        }
        h.app.prefs_window.shell.filter = "zzzz-no-setting".into();
        h.frames(4);
        assert!(!drawn(&h, "Placement handles"), "a miss shows nothing");
    }

    #[test]
    fn design_and_tools_short_labels_keep_legacy_search_and_reveal_the_real_control() {
        for language in ["en", "ja"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", serde_json::json!({"language":language,"open":"General"}));
            for (query, id, page, handle) in [
                ("Step values with the arrow keys and the wheel", "ui.step_values", crate::prefs::PAGE_DESIGN, "prefs:step_values"),
                ("For files from unknown programs", "prefs.stand_up_unknown", crate::prefs::PAGE_DESIGN, "prefs:stand_up_unknown"),
                ("Hide origin by default", "ui.hide_origin_by_default", crate::prefs::PAGE_DESIGN, "prefs:hide_origin"),
                ("Snap to face and edge centres", "Snap to face and edge centres", crate::prefs::PAGE_TOOLS, "prefs:hole_snaps"),
                ("Move holes by clicking or dragging", "Move holes by clicking or dragging", crate::prefs::PAGE_TOOLS, "prefs:hole_moves"),
                ("Several holes on one face", "prefs.design_tools.hole_several", crate::prefs::PAGE_TOOLS, "prefs:hole_several"),
                ("Group holes made together", "prefs.design_tools.hole_group", crate::prefs::PAGE_TOOLS, "prefs:hole_group"),
                ("Show moved parts' old place as a ghost", "prefs.design_tools.ghost", crate::prefs::PAGE_TOOLS, "prefs:drive_joints_ghost"),
                ("Near-axis rotation step (°)", "joint_move.coarse", crate::prefs::PAGE_TOOLS, "prefs:joint_rotation_coarse"),
                ("Far-axis rotation step (°)", "joint_move.fine", crate::prefs::PAGE_TOOLS, "prefs:joint_rotation_fine"),
                ("Show X/Y/Z distances", "settings.measure_axis_distances", crate::prefs::PAGE_TOOLS, "prefs:measure_axis_distances"),
                ("Show tool at cursor", "Show tool at cursor", crate::prefs::PAGE_TOOLS, "view:tool_cursor"),
            ] {
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let label = h.app.tr(id);
                let matches: Vec<_> = hits.iter().enumerate().filter(|(_, hit)| hit.page == page && hit.label == label).collect();
                assert_eq!(matches.len(), 1, "{language}: {query} has one real setting");
                set_cursor(&h.ctx, matches[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                // Let the normal scroll animation settle after opening the longer Design page.
                h.frames(20);
                assert_eq!(h.app.prefs_window.shell.section, page);
                assert!(h.app.prefs_window.shell.filter.is_empty());
                let body = crate::scenario::rect_of("dialog:page_body").unwrap();
                let target = crate::scenario::rect_of(handle).unwrap();
                assert!(body.contains_rect(target), "{language}: {query} reveals {handle}: body={body:?}, target={target:?}");
            }
        }
    }

    #[test]
    fn moved_preferences_search_paths_and_enter_follow_window_groups() {
        for (query, path, page) in [
            ("Direct interaction", "Work > Tools", crate::prefs::PAGE_TOOLS),
            ("Navigate with the 3D mouse", "Input > 3D mouse", crate::prefs::PAGE_MOUSE3D),
            ("Origin plane names", "Work > Design", crate::prefs::PAGE_DESIGN),
            ("Status bar", "Look > Layout", crate::prefs::PAGE_LAYOUT),
        ] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
            h.app.prefs_window.shell.filter = query.into();
            h.frames(4);
            assert!(h.widget(path).is_some(), "{query} shows {path}");
            let (_, _, hits) = nav(&h.ctx);
            let index = hits.iter().position(|hit| hit.page == page && hit.label == query).expect("moved control is keyboard reachable");
            set_cursor(&h.ctx, index);
            h.frames(2);
            h.step(&serde_json::json!({"key":"Enter"})).unwrap();
            h.frames(2);
            assert_eq!(h.app.prefs_window.shell.section, page);
            assert!(h.app.prefs_window.shell.filter.is_empty());
            assert!(h.widget(query).is_some());
        }
    }

    // These tests check navigation and scrolling, independent of machine load and the
    // highlight's wall-clock deadline. Keep the real reveal alive while frames settle.
    fn keep_reveal_visible(h: &mut crate::scenario::Harness) {
        assert!(crate::prefs::keep_search_reveal_visible(&mut h.app));
    }

    #[test]
    fn collapsed_shortcut_results_enter_reveals_the_command_despite_an_old_page_filter() {
        use serde_json::json;
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.ai_features = false;
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            h.call("ui.prefs", json!({"open":"Shortcuts","theme":theme}));
            h.step(&json!({"click":{"handle":"keymap:filter"}})).unwrap();
            h.step(&json!({"text":"solid.extrude"})).unwrap();
            let command = solidcraft_engine::command_specs().last().unwrap().id;
            let original = crate::toolbar_custom::export(&h.app);
            h.app.prefs_window.shell.filter = command.into();
            let find = |h: &crate::scenario::Harness| {
                let (_, _, hits) = nav(&h.ctx);
                hits.iter()
                    .enumerate()
                    .filter(|(_, hit)| hit.page == crate::prefs::PAGE_SHORTCUTS && hit.label.contains(command))
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            };
            // Wait for the results to appear rather than assuming four frames on a loaded machine.
            for _ in 0..600 {
                h.frames(1);
                if !find(&h).is_empty() {
                    break;
                }
            }
            let matches: Vec<_> = find(&h).into_iter().map(|i| (i, ())).collect();
            assert_eq!(matches.len(), 1, "a collapsed result owns one real keyboard target");
            set_cursor(&h.ctx, matches[0].0);
            h.frames(2);
            h.step(&json!({"key":"Enter"})).unwrap();
            keep_reveal_visible(&mut h);
            h.frames(30);
            // The reveal scrolls over later frames; wait until the target is in view.
            for _ in 0..600 {
                let in_view = match (crate::scenario::rect_of(&format!("keymap:capture:{command}")), crate::scenario::rect_of("dialog:page_body")) {
                    (Some(target), Some(body)) => body.contains_rect(target),
                    _ => false,
                };
                if h.app.prefs_window.shell.filter.is_empty() && in_view {
                    break;
                }
                keep_reveal_visible(&mut h);
                h.frames(1);
            }
            assert!(h.app.prefs_window.shell.filter.is_empty());
            assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_SHORTCUTS);
            let target = crate::scenario::rect_of(&format!("keymap:capture:{command}")).unwrap();
            let body = crate::scenario::rect_of("dialog:page_body").unwrap();
            assert!(body.contains_rect(target), "Enter reveals the actual off-screen command, clearing the old local filter");
            assert!(crate::scenario::handle_at("prefs:revealed").is_some());
            assert_eq!(crate::toolbar_custom::export(&h.app), original);
        }
    }

    #[test]
    fn assistant_permission_options_and_limited_value_have_real_enter_targets_while_ai_is_off() {
        use serde_json::json;
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.ai_features = false;
            h.call("ui.resize", json!({"width":1600,"height":1200}));
            h.call("ui.prefs", json!({"open":"General","theme":theme}));
            let original = (h.app.preferences.ai_permissions, h.app.preferences.agent_chat_turn_limit);
            for (query, target) in [
                ("Ask first", "prefs:ai_mode:ask"),
                ("Make edits", "prefs:ai_mode:edits"),
                ("Do everything", "prefs:ai_mode:all"),
                ("Limited", "prefs:agent_chat_turn_limit:mode"),
            ] {
                h.app.prefs_window.shell.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matches: Vec<_> =
                    hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_ASSISTANT && hit.label == query).collect();
                assert_eq!(matches.len(), 1, "{query} owns a single real setting target");
                set_cursor(&h.ctx, matches[0].0);
                h.frames(2);
                h.step(&json!({"key":"Enter"})).unwrap();
                keep_reveal_visible(&mut h);
                h.frames(30);
                assert_eq!(h.app.prefs_window.shell.section, crate::prefs::PAGE_ASSISTANT);
                let rect = crate::scenario::rect_of(target).unwrap();
                let revealed = crate::scenario::handle_at("prefs:revealed").unwrap();
                assert!(revealed.distance(rect.center()) <= 1.0, "{query} reveals its real control");
                assert_eq!(
                    (h.app.preferences.ai_permissions, h.app.preferences.agent_chat_turn_limit),
                    original,
                    "navigation never selects an option"
                );
            }
        }
    }
}

// SolidCraft's Preferences window on the kit's window, footer and colour components.
#[cfg(test)]
mod kit_use_tests {
    use crate::scenario;
    use crate::ui_kit::recent_colors::{MAX_RECENT_COLORS, remember_color};

    #[test]
    fn preferences_preserves_every_page_and_footer_in_both_themes() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.app.home.open = false;
            for (index, page) in crate::prefs::PAGES.iter().enumerate() {
                crate::prefs::open_at(&mut h.app, index);
                h.frames(3);
                assert_eq!(h.app.prefs_window.shell.section, index);
                let nav = scenario::rect_of("dialog:pages").unwrap();
                let row = scenario::rect_of(&format!("prefs:page:{page}")).unwrap();
                assert!(nav.contains_rect(row), "{dark}: {page} stays visible");
                let window = scenario::rect_of("prefs:window").unwrap();
                let header = scenario::rect_of("dialog:header").unwrap();
                let search = scenario::rect_of("prefs:search:frame").unwrap();
                assert!((header.height() - 40.0).abs() < 0.5);
                assert!(header.contains_rect(search), "{dark}: {page}: search stays inside the titlebar");
                assert!((search.width() - 230.0).abs() < 0.5 && (search.height() - 26.0).abs() < 0.5);
                assert!(search.left() > header.center().x);
                for action in ["prefs:ok", "prefs:apply", "prefs:cancel"] {
                    assert!(window.contains_rect(scenario::rect_of(action).unwrap()), "{dark}: {page}: {action}");
                }
                assert!(scenario::rect_of("prefs:page_body").is_some());
                assert!(h.widget(h.app.tr("prefs.restore_page_defaults")).is_some());
                let footer = scenario::rect_of("dialog:pages_footer").expect("pinned profile footer");
                assert!(nav.contains_rect(footer));
                let profile_card = scenario::rect_of("prefs:profile:sidebar_card").expect("bordered profile card");
                assert!(footer.contains_rect(profile_card));
                assert!((profile_card.height() - 48.0).abs() < 0.5);
                assert_eq!(profile_card, scenario::rect_of("prefs:profile").expect("the whole card is the real switcher"));
                let icon = scenario::rect_of("prefs:profile:icon").unwrap();
                let label = scenario::rect_of("prefs:profile:label").unwrap();
                let name = scenario::rect_of("prefs:profile:name").unwrap();
                let chevron = scenario::rect_of("prefs:profile:chevron").unwrap();
                for part in [icon, label, name, chevron] {
                    assert!(profile_card.contains_rect(part));
                }
                assert!(icon.right() < label.left() && label.bottom() <= name.top() && name.right() < chevron.left());
                assert!((label.left() - name.left()).abs() < 0.5);
                assert!((icon.center().y - profile_card.center().y).abs() < 0.5);
                let tagged = [
                    crate::prefs::PAGE_GENERAL,
                    crate::prefs::PAGE_APPEARANCE,
                    crate::prefs::PAGE_LAYOUT,
                    crate::prefs::PAGE_SKETCH,
                    crate::prefs::PAGE_NAVIGATION,
                    crate::prefs::PAGE_SHORTCUTS,
                ]
                .contains(&index);
                assert_eq!(scenario::rect_of("prefs:in_profile").is_some(), tagged, "{page}");
                if index == crate::prefs::PAGE_GENERAL {
                    let tag = scenario::rect_of("prefs:in_profile").unwrap();
                    let body = scenario::rect_of("prefs:page_body").unwrap();
                    assert!((tag.height() - 22.0).abs() < 0.5);
                    assert!((tag.right() - body.right()).abs() < 0.5, "{dark}: the profile tag aligns to the page's right edge");
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn page_sidebar_footer_stays_visible_when_the_page_list_scrolls() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.theme = theme.into();
            h.call("ui.resize", serde_json::json!({"width":1200,"height":620}));
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
            h.frames(6);
            let footer = scenario::rect_of("dialog:pages_footer").unwrap();
            let nav = scenario::rect_of("dialog:pages").unwrap();
            h.frame_with(vec![
                egui::Event::PointerMoved(egui::pos2(nav.center().x, nav.top() + 30.0)),
                egui::Event::MouseWheel {
                    source: egui::MouseWheelSource::Unknown,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -1000.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
            ]);
            h.frames(60);
            assert_eq!(scenario::rect_of("dialog:pages_footer"), Some(footer));
            assert!(footer.contains_rect(scenario::rect_of("prefs:profile").unwrap()));
            assert!(nav.contains_rect(scenario::rect_of("prefs:page:Plugins").unwrap()), "last page is reachable in {theme}");
        }
    }

    #[test]
    fn recent_colours_are_saved_with_the_preferences_and_hostile_entries_dropped() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let mut list = vec![json!([1, 2, 3]), json!([300, 0, 0]), json!("red"), json!([1, 2]), json!(null)];
        list.extend((0..50).map(|i| json!([i, i, i])));
        app.load_prefs(&json!({ "recent_colors": list }).to_string());
        assert_eq!(app.ui.recent_colors.len(), MAX_RECENT_COLORS);
        assert_eq!(app.ui.recent_colors.first(), Some(&[1, 2, 3]));
        let ctx = crate::fonts::test_context();
        crate::ui_kit::recent_colors::share(&ctx, &mut app.ui.recent_colors);
        remember_color(&ctx, [9, 8, 7], None);
        crate::ui_kit::recent_colors::share(&ctx, &mut app.ui.recent_colors);
        let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&app.prefs());
        assert_eq!(restored.ui.recent_colors.first(), Some(&[9, 8, 7]));
        assert_eq!(restored.ui.recent_colors.get(1), Some(&[1, 2, 3]));
        assert_eq!(restored.ui.recent_colors.len(), MAX_RECENT_COLORS);
    }
}
