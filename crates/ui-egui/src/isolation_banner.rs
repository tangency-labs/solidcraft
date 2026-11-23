//! The note at the top of the viewport while Isolate has layers: its depth, and the ways out
//! (Exit Isolation: one layer back, Ctrl+Shift+I; Exit All: Ctrl+Alt+I). The layers live in the
//! session (`Session::isolation_depth`); Ctrl+I isolates the selection as a new layer, through
//! `browser.visibility` with `isolate`, which is what the menus run too.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit::{Container, Surface};
use solidcraft_engine::Sel;

/// Isolate the selection as a new layer. An empty selection does nothing (so a stray Ctrl+I is
/// harmless, and a second press is not a toggle that isolates again).
pub(crate) fn isolate_selection(app: &mut SolveApp) {
    let paths: Vec<Vec<u64>> =
        app.session.selection.iter().filter_map(|s| if let Sel::Occurrence { path } = s { Some(path.clone()) } else { None }).collect();
    if !paths.is_empty() {
        isolate(app, json!({ "paths": paths, "visible": true, "isolate": true }));
        return;
    }
    let mut bodies = crate::context_menu::touched_bodies(app);
    for c in app.tree.picked_components.clone() {
        for b in crate::context_menu::component_bodies(app, c) {
            if !bodies.contains(&b) {
                bodies.push(b);
            }
        }
    }
    if !bodies.is_empty() {
        isolate(app, json!({ "items": bodies, "visible": true, "isolate": true }));
    }
}

/// Run an isolate (`browser.visibility`): a press that hides nothing more adds no layer, and says so.
pub(crate) fn isolate(app: &mut SolveApp, params: Value) {
    let depth = app.session.isolation_depth();
    match app.run("browser.visibility", params) {
        Ok(_) if app.session.isolation_depth() == depth => app.set_status(solidcraft_ui_text::tr("ui.nothingToIsolate").to_string(), false),
        Ok(_) => {}
        Err(e) => app.set_status(e, true),
    }
}

/// One layer back (false: not isolated).
pub(crate) fn exit_one(app: &mut SolveApp) -> bool {
    app.session.isolation_depth() > 0 && app.run("display.unisolate", json!({})).is_ok()
}

/// Every layer (false: not isolated).
pub(crate) fn exit_all(app: &mut SolveApp) -> bool {
    app.session.isolation_depth() > 0 && app.run("display.unisolate_all", json!({})).is_ok()
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let depth = app.session.isolation_depth();
    let Some(view) = app.viewport.rect.filter(|_| depth > 0 && crate::toolbar::design_shown(app)) else { return };
    // The floating shell lays the ribbon over the viewport: sit inside the content area, below it.
    let view = crate::shell::content_rect(app, view);
    let t = solidcraft_theme::Tokens::get();
    let (mut one, mut all) = (false, false);
    Container::new(ctx, Surface::Toast)
        .area(egui::Id::unique("isolation_banner"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(egui::pos2(view.center().x, view.top() + 10.0))
        .show(ctx, |ui| {
            Container::popup_frame(ui.style()).fill(t.panel).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    // A row height set before the label, so the label is centred on the buttons' text.
                    ui.set_min_height(ui.spacing().interact_size.y.max(26.0));
                    // The buttons look like the search field's border and the Browser rows' hover.
                    let w = &mut ui.visuals_mut().widgets;
                    for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                        state.bg_stroke = egui::Stroke::new(1.0, t.control_border);
                    }
                    w.hovered.weak_bg_fill = t.hover;
                    w.hovered.bg_fill = t.hover;
                    // One layer reads as plain "Isolated"; layers (levels) only show once there are two.
                    let note = if depth > 1 {
                        solidcraft_ui_text::locale().format("ui.isolatedLevel", &[("level", &depth.to_string())])
                    } else {
                        solidcraft_ui_text::tr("ui.isolated").to_string()
                    };
                    ui.label(egui::RichText::new(note).color(t.text).strong());
                    let back = if depth > 1 {
                        solidcraft_ui_text::locale().format("ui.backToLevel", &[("level", &(depth - 1).to_string())])
                    } else {
                        solidcraft_ui_text::tr("ui.exitIsolation").to_string()
                    };
                    one = crate::ui_kit::button(ui, &back, crate::ui_kit::Button::Enabled(true)).on_hover_text("Ctrl+Shift+I").clicked();
                    if depth > 1 {
                        all = crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.exitAll"), crate::ui_kit::Button::Enabled(true))
                            .on_hover_text("Ctrl+Alt+I")
                            .clicked();
                    }
                });
            });
        });
    if all {
        exit_all(app);
    } else if one {
        exit_one(app);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::Sel;

    use crate::scenario::Harness;

    fn harness() -> Harness {
        let mut h = Harness::new();
        for (i, n) in ["A", "B", "C"].iter().enumerate() {
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30 * i, 0, 0], "body_name": n})).unwrap();
        }
        h.frames(3);
        h
    }

    fn select(h: &mut Harness, names: &[&str]) {
        let items: Vec<Sel> = names.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
        h.app.run("select.set", json!({ "items": items })).unwrap();
    }

    fn key(h: &mut Harness, k: &str, shift: bool, alt: bool) {
        h.step(&json!({"key": k, "cmd": true, "shift": shift, "alt": alt})).unwrap();
        h.frames(3);
    }

    fn iso(h: &mut Harness, names: &[&str]) {
        h.app.run("browser.visibility", json!({"items": names, "visible": true, "isolate": true})).unwrap();
    }

    fn hidden(h: &Harness) -> Vec<String> {
        let mut v = h.app.session.visibility.hidden_bodies.clone();
        v.sort();
        v
    }

    #[test]
    fn ctrl_i_isolates_the_selection_and_the_other_keys_leave_the_layers() {
        let mut h = harness();
        // Nothing selected: Ctrl+I does nothing, and does not become Isolate-everything.
        h.app.run("select.clear", json!({})).unwrap();
        key(&mut h, "I", false, false);
        assert_eq!((h.app.session.isolation_depth(), hidden(&h)), (0, Vec::<String>::new()));
        // A layer, then another on top.
        select(&mut h, &["A", "B"]);
        key(&mut h, "I", false, false);
        assert_eq!((h.app.session.isolation_depth(), hidden(&h)), (1, vec!["C".to_string()]));
        select(&mut h, &["A"]);
        key(&mut h, "I", false, false);
        assert_eq!((h.app.session.isolation_depth(), hidden(&h)), (2, vec!["B".to_string(), "C".to_string()]));
        // Ctrl+I again with the same selection hides nothing more, so it is no new layer.
        key(&mut h, "I", false, false);
        assert_eq!(h.app.session.isolation_depth(), 2);
        // Ctrl+Shift+I: one layer back; Ctrl+Alt+I: all of them.
        key(&mut h, "I", true, false);
        assert_eq!(h.app.session.isolation_depth(), 1);
        key(&mut h, "I", false, true);
        assert_eq!((h.app.session.isolation_depth(), hidden(&h)), (0, Vec::<String>::new()));
        // Leaving when not isolated changes nothing.
        key(&mut h, "I", true, false);
        key(&mut h, "I", false, true);
        assert_eq!(hidden(&h), Vec::<String>::new());
    }

    #[test]
    fn the_viewport_menu_leaves_isolation_and_the_banner_shows_while_isolated() {
        let mut h = harness();
        h.app.run("select.clear", json!({})).unwrap();
        let before = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
        let exit = before.iter().find(|i| i.label == "Exit Isolation").unwrap();
        assert!(!exit.enabled, "nothing to exit yet");
        iso(&mut h, &["A", "B"]);
        iso(&mut h, &["A"]);
        h.frames(3);
        // The banner's buttons are on screen; "Back to level 1" steps back one layer.
        let exit_one = crate::scenario::handles("dialog:button").into_iter().find(|(k, _)| k.ends_with(":Back to level 1"));
        let (_, at) = exit_one.expect("the banner shows while isolated");
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.isolation_depth(), 1);
        iso(&mut h, &["A"]);
        h.frames(3);
        let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
        assert!(items.iter().find(|i| i.label == "Exit Isolation").unwrap().enabled);
        let all = items.iter().find(|i| i.label == "Exit All Isolations").expect("two layers offer Exit All");
        crate::context_menu::run_item(&mut h.app, all, egui::pos2(0.0, 0.0));
        assert_eq!(h.app.session.isolation_depth(), 0);
        // The body menu's Isolate runs the same layer command.
        let body = crate::context_menu::items(&h.app, &crate::context_menu::Target::Body { name: "B".into() });
        let isolate = body.iter().find(|i| i.label == "Isolate").unwrap();
        assert_eq!(isolate.shortcut, "Ctrl+I");
        crate::context_menu::run_item(&mut h.app, isolate, egui::pos2(0.0, 0.0));
        assert_eq!(h.app.session.isolation_depth(), 1);
        assert_eq!(hidden(&h), vec!["A", "C"]);
        // One layer: the banner offers a plain Exit Isolation, no levels.
        h.frames(3);
        let (_, at) = crate::scenario::handles("dialog:button").into_iter().find(|(k, _)| k.ends_with(":Exit Isolation")).unwrap();
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.isolation_depth(), 0);
        assert!(hidden(&h).is_empty());
        // And no banner without layers.
        assert!(crate::scenario::handles("dialog:button").iter().all(|(k, _)| !k.ends_with(":Exit All")));
    }
}
