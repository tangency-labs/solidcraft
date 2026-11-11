//! Creative Mode: a lighter way to shape a model in the Design workspace, off by default
//! (Preferences › Tools). With it on:
//!
//! - Pulling a flat face or a profile extrudes or cuts it, as direct interaction does, and the
//!   release applies the change: no window waits for OK (`drag_extrude::end`).
//! - A pill floats above the selection with the few commands that fit it: on a face Sketch and
//!   Press Pull, on an edge Fillet and Chamfer, on a body Move, Appearance and Delete. Its name
//!   chip opens the full context menu. The commands come from the context menu's own list for
//!   the selection ([`crate::context_menu::items`]), so the pill offers nothing the menu would
//!   refuse, and every action stays an ordinary command on the timeline.

use crate::SolveApp;
use crate::context_menu::Target;
use crate::ui_kit::{self, Item};
use solidcraft_engine::Sel;

/// The commands the pill may offer, most useful first. What the selection's context menu does
/// not list, or lists disabled, is left out.
const PILL: [&str; 10] = [
    "sketch.create",
    "solid.press_pull",
    "solid.fillet",
    "solid.chamfer",
    "ui.moveBodies",
    "solid.shell",
    "ui.appearance",
    "ui.hide",
    "ui.delete",
    "inspect.measure",
];

/// The most actions the pill shows beside its name chip.
const MAX_ACTIONS: usize = 6;

/// Creative Mode is on.
pub fn on(app: &SolveApp) -> bool {
    app.preferences.creative_mode
}

/// The pill's actions for the current selection, in [`PILL`] order.
pub(crate) fn pill_items(app: &SolveApp) -> Vec<Item> {
    let all = crate::context_menu::items(app, &Target::Viewport);
    PILL.iter().filter_map(|id| all.iter().find(|i| i.id == *id && i.enabled)).take(MAX_ACTIONS).cloned().collect()
}

/// What the name chip says: the body the selection is on, or how many things are selected.
fn selection_name(app: &SolveApp) -> String {
    match app.session.selection.as_slice() {
        [Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. }] | [Sel::Body { name: body }] => body.clone(),
        many => format!("{} selected", many.len()),
    }
}

/// Whether the pill has a place this frame: Creative Mode on, the Design workspace, something
/// selected, and nothing else in progress (a tool, a sketch, a window, a menu or a pull).
fn wanted(app: &SolveApp) -> bool {
    on(app)
        && !crate::render_workspace::active(app)
        && !app.session.selection.is_empty()
        && app.session.active_sketch.is_none()
        && app.tool.is_none()
        && app.dialog.is_none()
        && app.viewport.context_menu.is_none()
        && !crate::drag_extrude::started(app)
}

/// Draw the selection pill and run what it is clicked for.
pub(crate) fn overlays(app: &mut SolveApp, ui: &mut egui::Ui) {
    if !wanted(app) {
        crate::shell::register_panel(app, "creative:selection", None, None);
        return;
    }
    let (Some(bounds), Some(viewport)) = (crate::viewport::selection_bounds(app), app.viewport.rect) else {
        crate::shell::register_panel(app, "creative:selection", None, None);
        return;
    };
    let t = solidcraft_theme::Tokens::get();
    // The view less the ribbon, browser and other panels over it, as the Render pill uses.
    let free = crate::shell::content_rect(app, viewport).shrink(t.space_8);
    let proj = crate::viewport::projection(app, viewport);
    let mut screen = egui::Rect::NOTHING;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                if let Some(p) = proj.to_screen(solidcore_record::Vec3::new(x, y, z)).filter(|p| p.is_finite()) {
                    screen.extend_with(p);
                }
            }
        }
    }
    let items = pill_items(app);
    if !screen.is_positive() || !screen.intersects(free) || items.is_empty() {
        crate::shell::register_panel(app, "creative:selection", None, None);
        return;
    }
    let actions: Vec<_> = items
        .iter()
        .map(|i| ui_kit::StageAction { icon: Some(&*i.icon), label: &i.label, enabled: i.enabled, primary: false, reason: i.tip.as_deref() })
        .collect();
    let anchor = egui::pos2(
        ui_kit::within(screen.center().x, free.left(), free.right()),
        ui_kit::within(screen.top() - t.space_12, free.top(), free.bottom()),
    );
    let name = selection_name(app);
    let ctx = ui.ctx().clone();
    let pill = ui_kit::Container::selection_card(&ctx, egui::Id::unique("creative_selection"), anchor, free, |ui| {
        ui_kit::selection_actions(ui, &name, &actions)
    });
    crate::scenario::publish_rect("creative:selection", pill.response.rect);
    crate::shell::register_panel(app, "creative:selection", Some(pill.response.rect), None);
    let at = pill.response.rect.left_bottom();
    match pill.inner {
        Some(index) if index < items.len() => crate::context_menu::run_item(app, &items[index], at),
        Some(_) => crate::context_menu::open_for(app, at, Target::Viewport),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use serde_json::json;

    fn boxed() -> Harness {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
        h.settle();
        h
    }

    fn ids(h: &Harness) -> Vec<String> {
        pill_items(&h.app).into_iter().map(|i| i.id).collect()
    }

    #[test]
    fn creative_mode_is_off_until_the_setting_is_on() {
        let h = Harness::new();
        assert!(!on(&h.app));
    }

    #[test]
    fn a_flat_face_offers_sketch_and_press_pull_first() {
        let mut h = boxed();
        h.app.run("select.set", json!({"items":[{"type":"face","body":"Body1","point":[10,10,10]}]})).unwrap();
        let ids = ids(&h);
        assert_eq!(ids.first().map(String::as_str), Some("sketch.create"), "{ids:?}");
        assert!(ids.iter().any(|i| i == "solid.press_pull"), "{ids:?}");
        assert!(ids.len() <= MAX_ACTIONS);
    }

    #[test]
    fn an_edge_offers_fillet_and_chamfer() {
        let mut h = boxed();
        h.app.run("select.set", json!({"items":[{"type":"edge","body":"Body1","point":[10,0,10]}]})).unwrap();
        let ids = ids(&h);
        assert_eq!(&ids[..2], ["solid.fillet", "solid.chamfer"], "{ids:?}");
    }

    #[test]
    fn nothing_selected_offers_nothing_and_the_pill_stays_away() {
        let mut h = boxed();
        h.app.preferences.creative_mode = true;
        h.app.run("select.clear", json!({})).unwrap();
        assert!(!wanted(&h.app));
    }
}
