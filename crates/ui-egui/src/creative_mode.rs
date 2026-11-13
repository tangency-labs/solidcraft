//! Creative Mode: a lighter way to shape a model in the Design workspace, off by default
//! (Preferences › Tools). With it on:
//!
//! - Pulling a flat face or a profile extrudes or cuts it, as direct interaction does, and the
//!   release applies the change: no window waits for OK (`drag_extrude::end`).
//! - A pill floats above the selection with the commands that fit it, single or several items:
//!   the features whose window takes the selection come first ([`crate::creative_tools`]: Sketch
//!   and Press Pull on a face, Fillet on an edge, Loft on two profiles, Sweep on a profile and a
//!   path, Mirror and patterns on a body, one-click Join, Cut and Intersect on overlapping
//!   bodies), then the context menu's Move, Appearance, Hide and Delete. Its name chip opens the
//!   full context menu. Every action stays an ordinary command on the timeline.
//! - Dragging an edge across itself rounds it when pulled out of the material and cuts it flat
//!   when pushed in (`drag_fillet`).
//! - Double-clicking a body picks it up: Move opens on it with its triad, and releasing a
//!   triad drag applies the move (`move_body`).
//!
//! The Creative switch in the application bar turns it on and off without opening Preferences.

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

/// The most actions the pill shows beside its name chip; the name chip opens the full menu.
const MAX_ACTIONS: usize = 8;

/// Creative Mode is on.
pub fn on(app: &SolveApp) -> bool {
    app.preferences.creative_mode
}

/// Turn Creative Mode on or off, as the application bar's switch does. The preferences writer
/// saves the change with the other settings.
pub fn toggle(app: &mut SolveApp) {
    app.preferences.creative_mode = !app.preferences.creative_mode;
}

/// In Creative Mode, select the whole of `body` and open Move on it, its triad ready to drag.
/// False (nothing done) when Creative Mode is off or Move does not start.
pub fn move_body(app: &mut SolveApp, body: &str) -> bool {
    if !on(app) || crate::render_workspace::active(app) {
        return false;
    }
    if app.run("select.set", serde_json::json!({"items": [{"type": "body", "name": body}]})).is_err() {
        return false;
    }
    app.start("solid.move");
    matches!(app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Move { .. }))
}

/// The Creative switch at the right of `span` (the application bar's free space): filled with
/// the accent while Creative Mode is on, outlined while it is off. Its label is dropped when the
/// bar is too narrow, and it is left out when even the icon does not fit. Returns its rectangle.
pub(crate) fn bar_switch(app: &mut SolveApp, ui: &mut egui::Ui, span: egui::Rect) -> Option<egui::Rect> {
    let t = solidcraft_theme::Tokens::get();
    let height = crate::toolbar::PILL_HEIGHT;
    let font = egui::FontId::proportional(t.font_label);
    let label = app.tr("creative_mode.switch").to_string();
    let (icon_x, text_x, right_pad) = (16.0, 29.0, 11.0);
    let galley_w = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), t.app_bar_text).size().x);
    // Keep room for the document tabs before showing the label.
    let labeled = text_x + galley_w + right_pad + 160.0 <= span.width();
    let width = if labeled { text_x + galley_w + right_pad } else { height + 4.0 };
    if width > span.width() {
        return None;
    }
    let rect = egui::Rect::from_min_size(egui::pos2(span.right() - width, span.center().y - height / 2.0), egui::vec2(width, height));
    let response = ui.interact(rect, ui.scope_id().with("sc_creative_switch"), egui::Sense::click());
    let on = on(app);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), on, &label));
    crate::scenario::publish_target("titlebar:creative", rect);
    let hot = response.hovered() || response.has_focus();
    let radius = height / 2.0 * f32::from(t.radius_xl) / 10.0;
    let fill = match (on, hot) {
        (true, false) => t.accent,
        (_, true) => t.app_bar_hover_button,
        (false, false) => egui::Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, radius, fill);
    if !on {
        ui.painter().rect_stroke(rect, radius, egui::Stroke::new(1.0, t.app_bar_text.gamma_multiply(0.45)), egui::StrokeKind::Inside);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            solidcraft_theme::Tokens::outset_radius(radius, 2.0),
            egui::Stroke::new(1.0, t.app_bar_text),
            egui::StrokeKind::Outside,
        );
    }
    let glyph_x = if labeled { rect.left() + icon_x } else { rect.center().x };
    let glyph = egui::Rect::from_center_size(egui::pos2(glyph_x, rect.center().y), egui::vec2(16.0, 16.0));
    crate::icons::paint_on(ui.painter(), glyph, "style", Some(true), 1.0);
    if labeled {
        ui.painter().text(egui::pos2(rect.left() + text_x, rect.center().y), egui::Align2::LEFT_CENTER, &label, font, t.app_bar_text);
    }
    let tip = app.tr(if on { "creative_mode.switch_on_tip" } else { "creative_mode.switch_off_tip" }).to_string();
    use crate::ui_kit::BackingTooltipExt as _;
    if response.backing_on_hover_text(tip).clicked() {
        toggle(app);
    }
    Some(rect)
}

/// The pill's actions for the current selection: the features that fit it
/// ([`crate::creative_tools`]: Join, Cut and Intersect for overlapping bodies, Loft for two
/// profiles, Sweep for a profile and a path, and so on), then the context menu's own commands in
/// [`PILL`] order that are not already there.
pub(crate) fn pill_items(app: &SolveApp) -> Vec<Item> {
    let mut v = crate::creative_tools::items(app);
    let all = crate::context_menu::items(app, &Target::Viewport);
    for id in PILL {
        if !v.iter().any(|i| i.id == id)
            && let Some(item) = all.iter().find(|i| i.id == id && i.enabled)
        {
            v.push(item.clone());
        }
    }
    v.truncate(MAX_ACTIONS);
    v
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
    // A modal window or the recovery prompt owns the view: the pill must not sit over it.
    let modal = ui.ctx().memory(|m| m.top_modal_layer().is_some()) || (!app.recoverable.is_empty() && !app.home.open);
    if modal || !wanted(app) {
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
        ui_kit::selection_tools(ui, &name, &actions)
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
    fn the_bar_switch_turns_creative_mode_on_and_off() {
        let mut h = boxed();
        h.frames(2);
        h.step(&json!({"click": {"handle": "titlebar:creative"}})).unwrap();
        h.frames(2);
        assert!(on(&h.app), "the first click turns it on");
        h.step(&json!({"click": {"handle": "titlebar:creative"}})).unwrap();
        h.frames(2);
        assert!(!on(&h.app), "the second turns it off");
    }

    #[test]
    fn double_clicking_a_body_picks_it_up_and_a_triad_drag_moves_it() {
        let mut h = boxed();
        h.app.preferences.creative_mode = true;
        h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "fit"}})).unwrap();
        h.settle();
        h.step(&json!({"click": {"world": [10, 10, 10]}, "double": true})).unwrap();
        h.frames(3);
        let Some(d) = h.app.dialog.as_ref() else { panic!("Move opened") };
        assert!(matches!(d.kind, crate::dialogs::Kind::Move { .. }), "{:?}", d.kind);
        assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Body1".into() }], "Move holds the body");
        let features = h.app.session.doc.features.len();
        let before = h.app.session.execute("inspect.measure", &json!({})).unwrap()["bodies"][0]["bbox"].clone();
        h.step(&json!({"drag": [{"gizmo": "x"}, {"gizmo": "x", "by": [10.0, 0.0, 0.0]}]})).unwrap();
        h.frames(6);
        h.settle();
        assert!(h.app.dialog.is_none(), "the release applied the move");
        assert_eq!(h.app.session.doc.features.len(), features + 1);
        let after = h.app.session.execute("inspect.measure", &json!({})).unwrap()["bodies"][0]["bbox"].clone();
        assert_ne!(before, after, "the body moved");
    }

    #[test]
    fn without_creative_mode_a_double_click_does_not_move() {
        let mut h = boxed();
        h.step(&json!({"click": {"world": [10, 10, 10]}, "double": true})).unwrap();
        h.frames(3);
        assert!(!h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Move { .. })));
    }

    #[test]
    fn nothing_selected_offers_nothing_and_the_pill_stays_away() {
        let mut h = boxed();
        h.app.preferences.creative_mode = true;
        h.app.run("select.clear", json!({})).unwrap();
        assert!(!wanted(&h.app));
    }
}
