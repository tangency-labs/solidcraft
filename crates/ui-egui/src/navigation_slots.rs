//! SolidCraft's navigation bar slots: what each slot shows, says and runs, and the bar a new
//! preferences file starts with. The bar component (`navigation_bar`) reads this catalogue once
//! [`install`] has run.
use crate::navigation_bar::{Catalogue, Slot};
use crate::ui_kit::NavigationKind;

macro_rules! slot {
    ($id:literal, $kind:ident, $icon:literal, $command:expr) => {
        slot!($id, $kind, $icon, $command, orbit: false, pick: false)
    };
    ($id:literal, $kind:ident, $icon:literal, $command:expr, orbit: $orbit:literal, pick: $pick:literal) => {
        slot!($id, $kind, $icon, $command, orbit: $orbit, pick: $pick, icons: &[])
    };
    ($id:literal, $kind:ident, $icon:literal, $command:expr, orbit: $orbit:literal, pick: $pick:literal, icons: $icons:expr) => {
        Slot {
            id: $id,
            kind: NavigationKind::$kind,
            icon: $icon,
            label: concat!("navbar.", $id),
            description: concat!("navbar.description.", $id),
            command: $command,
            needs_orbit: $orbit,
            needs_pick: $pick,
            value_icons: $icons,
        }
    };
}

/// The engine's style ids in index order (`ui.visual_style` 0..3), with the English menu label.
pub(crate) const VISUAL_STYLES: [(&str, &str); 4] = [
    ("shaded_with_edges", "Shaded with edges"),
    ("shaded", "Shaded"),
    ("wireframe", "Wireframe"),
    ("shaded_with_hidden_edges", "Shaded with hidden edges"),
];

/// Each visual style's icon, in [`VISUAL_STYLES`] order.
const VISUAL_STYLE_ICONS: [&str; 4] = ["style_shaded_edges", "style_shaded", "style_wireframe", "style_hidden_edges"];

/// The icon of visual style `style`; the first style's for an unknown one.
pub(crate) fn visual_style_icon(style: u8) -> &'static str {
    let [first, ..] = VISUAL_STYLE_ICONS;
    VISUAL_STYLE_ICONS.get(usize::from(style)).copied().unwrap_or(first)
}

// Reserved for the selection-mode slot in #6326; no runtime consumer yet.
#[cfg(test)]
pub(crate) const SELECTION_MODE_ICONS: &[(&str, &str)] = &[
    ("faces_edges", "select_faces_edges"),
    ("bodies", "select_bodies"),
    ("features", "select_features"),
    ("parts", "select_parts"),
    ("components", "select_components"),
    ("sketch", "select_sketch"),
];

/// The zoom slot's command: the drag-zoom tool, which a key held down also starts.
pub(crate) const ZOOM_TOOL: &str = "view.tool.zoom";

/// The slots, in the order the customiser lists them. Orbiting slots leave flat canvases;
/// looking at a face and the visual style also need something to pick.
pub(crate) const CATALOGUE: &[Slot] = &[
    slot!("orbit", Tool, "orbit", Some("view.tool"), orbit: true, pick: false),
    slot!("pan", Tool, "pan", Some("view.tool")),
    slot!("zoom", Tool, "zoom", Some(ZOOM_TOOL)),
    slot!("spin", Toggle, "spin", Some("view.spin"), orbit: true, pick: false),
    slot!("fit", Action, "fit", Some("view.fit")),
    slot!("look_at", Action, "lookat", Some("view.normal_to"), orbit: true, pick: true),
    slot!("home", Action, "home", Some("view.home")),
    slot!("visual_style", Menu, "style_shaded_edges", Some("view.visual_style"), orbit: true, pick: true, icons: &VISUAL_STYLE_ICONS),
    slot!("projection", Toggle, "perspective", None, orbit: true, pick: false),
    slot!("display", Menu, "display", None),
    slot!("grid", Toggle, "grid", Some("view.grid")),
    slot!("ground_shadow", Toggle, "ground_shadow", Some("view.ground_shadow")),
    slot!("ambient_occlusion", Toggle, "ambient_occlusion", Some("view.ambient_occlusion")),
    slot!("separator", Separator, "", None),
];

/// SolidCraft's bar: its slots, the bar a new preferences file holds, the menus that show their
/// value until the user chooses (the selection and render modes, once they are slots) and the
/// View menu, which only ever shows its icon.
static HOST: Catalogue = Catalogue {
    slots: CATALOGUE,
    default_bar: &["orbit", "pan", "zoom", "separator", "fit", "look_at", "home", "separator", "visual_style", "display"],
    value_menus: &["selection_mode", "render_mode"],
    icon_menus: &["display"],
};

/// Installs SolidCraft's catalogue, once per process, before preferences are read or the first
/// frame; later calls find it installed.
pub fn install() {
    crate::navigation_bar::install_catalogue(&HOST);
}
