//! SolidCraft's navigation bar slots: what each slot shows, says and runs. The bar component
//! (`navigation_bar`) checks stored bars against this catalogue once [`install`] has run.
use crate::navigation_bar::Slot;
use crate::ui_kit::NavigationKind;

macro_rules! slot {
    ($id:literal, $kind:ident, $icon:literal, $command:expr) => {
        slot!($id, $kind, $icon, $command, orbit: false, pick: false)
    };
    ($id:literal, $kind:ident, $icon:literal, $command:expr, orbit: $orbit:literal, pick: $pick:literal) => {
        Slot {
            id: $id,
            kind: NavigationKind::$kind,
            icon: $icon,
            label: concat!("navbar.", $id),
            description: concat!("navbar.description.", $id),
            command: $command,
            needs_orbit: $orbit,
            needs_pick: $pick,
        }
    };
}

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
    slot!("visual_style", Menu, "style_shaded_edges", Some("view.visual_style"), orbit: true, pick: true),
    slot!("projection", Toggle, "perspective", None, orbit: true, pick: false),
    slot!("display", Menu, "display", None),
    slot!("grid", Toggle, "grid", Some("view.grid")),
    slot!("ground_shadow", Toggle, "ground_shadow", Some("view.ground_shadow")),
    slot!("ambient_occlusion", Toggle, "ambient_occlusion", Some("view.ambient_occlusion")),
    slot!("separator", Separator, "", None),
];

/// Installs SolidCraft's catalogue, once per process, before preferences are read or the first
/// frame; later calls find it installed.
pub fn install() {
    crate::navigation_bar::install_catalogue(CATALOGUE);
}
