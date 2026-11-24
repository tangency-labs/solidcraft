//! SolidCraft's navigation bar slots: what each slot shows, says and runs, and the bar a new
//! preferences file starts with. The bar component (`navigation_bar`) reads this catalogue once
//! [`install`] has run.
use crate::navigation_bar::{Catalogue, Slot};
use crate::ui_kit::NavigationKind;
use solidcraft_engine::PickMode;

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

/// Each selection mode's icon, in [`PickMode::ALL`] order (the selection-mode menu's).
pub(crate) const SELECTION_MODE_ICONS: [&str; 6] =
    ["select_faces_edges", "select_bodies", "select_features", "select_parts", "select_components", "select_sketch"];

/// The selection modes with their icons, in the menu's order.
pub(crate) fn selection_mode_icons() -> impl Iterator<Item = (PickMode, &'static str)> {
    PickMode::ALL.into_iter().zip(SELECTION_MODE_ICONS)
}

/// The value a menu slot shows its icon by ([`Slot::current_icon`]): the visual style, or the
/// selection mode's place in [`PickMode::ALL`]; 0 for a slot that holds no value.
pub(crate) fn value(ui: &crate::UiState, slot: &Slot) -> u8 {
    match slot.id {
        "visual_style" => ui.visual_style,
        "selection_mode" => PickMode::ALL.iter().position(|mode| *mode == ui.pick_mode).and_then(|i| u8::try_from(i).ok()).unwrap_or(0),
        _ => 0,
    }
}

/// The zoom slot's command: the drag-zoom tool, which a key held down also starts.
pub(crate) const ZOOM_TOOL: &str = "view.tool.zoom";

/// The slots, in the order the customiser lists them. Orbiting slots leave flat canvases;
/// looking at a face, the selection mode and the visual style also need something to pick.
pub(crate) const CATALOGUE: &[Slot] = &[
    slot!("orbit", Tool, "orbit", Some("view.tool"), orbit: true, pick: false),
    slot!("pan", Tool, "pan", Some("view.tool")),
    slot!("zoom", Tool, "zoom", Some(ZOOM_TOOL)),
    Slot {
        id: "sheet_zoom",
        kind: NavigationKind::Menu,
        icon: "zoom",
        label: "ui.sheet_page.zoom",
        description: "navbar.description.sheet_zoom",
        command: None,
        needs_orbit: false,
        needs_pick: false,
        value_icons: &[],
    },
    slot!("spin", Toggle, "spin", Some("view.spin"), orbit: true, pick: false),
    slot!("fit", Action, "fit", Some("view.fit")),
    slot!("look_at", Action, "lookat", Some("view.normal_to"), orbit: true, pick: true),
    slot!("home", Action, "home", Some("view.home")),
    slot!("selection_mode", Menu, "select_faces_edges", Some("select.mode"), orbit: false, pick: true, icons: &SELECTION_MODE_ICONS),
    slot!("visual_style", Menu, "style_shaded_edges", Some("view.visual_style"), orbit: true, pick: true, icons: &VISUAL_STYLE_ICONS),
    slot!("projection", Toggle, "perspective", None, orbit: true, pick: false),
    slot!("display", Menu, "display", None),
    slot!("grid", Toggle, "grid", Some("view.grid")),
    slot!("ground_shadow", Toggle, "ground_shadow", Some("view.ground_shadow")),
    slot!("ambient_occlusion", Toggle, "ambient_occlusion", Some("view.ambient_occlusion")),
    // Shown on its own while an Isolate is in effect (#8120); the bar view runs it.
    slot!("unisolate", Toggle, "unisolate", None, orbit: true, pick: true),
    slot!("separator", Separator, "", None),
];

/// SolidCraft's bar: its slots, the bar a new preferences file holds, the menus that show their
/// value until the user chooses (the selection mode, the render mode once it is a slot, and the
/// sheet zoom), the View menu, which only ever shows its icon, and Unisolate, which stands out
/// while the rest of the model is hidden.
static HOST: Catalogue = Catalogue {
    slots: CATALOGUE,
    default_bar: &["orbit", "pan", "zoom", "separator", "fit", "look_at", "home", "separator", "selection_mode", "visual_style", "display"],
    value_menus: &["selection_mode", "render_mode", "sheet_zoom"],
    icon_menus: &["display"],
    strong: &["unisolate"],
};

/// Installs SolidCraft's catalogue, once per process, before preferences are read or the first
/// frame; later calls find it installed.
pub fn install() {
    crate::navigation_bar::install_catalogue(&HOST);
}
