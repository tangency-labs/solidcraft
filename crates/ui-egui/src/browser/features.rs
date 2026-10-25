//! Native feature rows in the component browser, separate from timeline layout and order.

use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Reveal {
    pub id: u64,
    pub path: Vec<u64>,
}

/// Open the Features folder that lists this feature and scroll to its row, without changing the
/// active component or history marker. The folder is the one of the placement `body` belongs to:
/// without a body there is no telling which copy of a component was meant.
pub(crate) fn reveal(app: &mut SolveApp, id: u64, body: &str) {
    let Some(feature) = app.session.doc.feature(id) else { return };
    if matches!(feature.kind, FeatureKind::Sketch { .. }) {
        return; // Sketches are listed in Sketches, not Features.
    }
    let component = feature.component;
    let state = app.session.model.state();
    let world = app.session.world_state();
    let Some(source) = world.body(body).map(|selected| selected.definition_name()) else { return };
    let Some((_, path, _)) = app
        .session
        .doc
        .placements()
        .into_iter()
        .find(|(owner, path, _)| *owner == component && app.session.doc.world_body_name(&state, source, component, path) == body)
    else {
        return;
    };
    app.ui.show_browser = true;
    app.preferences.timeline_left_browser_collapsed = false;
    app.tree.reveal = None;
    app.tree.collapsed.remove("c0");
    for depth in 1..=path.len() {
        app.tree.collapsed.remove(&occurrence_key(&path[..depth]));
    }
    let scope = if path.is_empty() { format!("c{component}") } else { occurrence_key(&path) };
    app.tree.collapsed.remove(&scope);
    app.tree.expanded.insert(format!("{scope}/features"));
    app.tree.reveal_feature = Some(Reveal { id, path });
}

/// With the history in the browser: open the feature's component and history, and scroll to it.
pub(crate) fn reveal_in_history(app: &mut SolveApp, id: u64) {
    let Some(component) = app.session.doc.feature(id).map(|feature| feature.component) else { return };
    app.ui.show_browser = true;
    app.tree.collapsed.remove("c0");
    app.tree.collapsed.remove(&format!("c{component}"));
    app.tree.collapsed.remove(&format!("c{component}/history"));
    app.tree.reveal_feature = Some(Reveal { id, path: Vec::new() });
}

pub(super) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, component: u64, path: &[u64], depth: usize, acts: &mut Actions) {
    // Compact history: every feature of the component, sketches too, in history order with the
    // marker, in place of the timeline.
    let compact = app.preferences.history_in_browser;
    let has_features = match app.tree.model.as_ref().filter(|m| m.matches(&app.session) && !compact) {
        Some(model) => model.features.get(&component).is_some_and(|features| !features.is_empty()),
        None => app
            .session
            .doc
            .features
            .iter()
            .any(|feature| feature.component == component && (compact || !matches!(feature.kind, FeatureKind::Sketch { .. }))),
    };
    if !has_features {
        return;
    }
    let scope = if path.is_empty() { format!("c{component}") } else { occurrence_key(path) };
    // The history opens by default; the Features folder stays closed until opened.
    let (key, label) =
        if compact { (format!("{scope}/history"), crate::i18n::tr("ui.history_title")) } else { (format!("{scope}/features"), "Features") };
    let open = is_open(app, &key, compact);
    let r = draw_row(ui, ui.id().with(&key), &Row { depth, fold: Some(open), icon: "folder", label, ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, &key, compact);
    }
    if !open {
        return;
    }
    let indices = match app.tree.model.as_ref().filter(|m| m.matches(&app.session) && !compact) {
        Some(model) => model.features.get(&component).cloned().unwrap_or_default(),
        None => app
            .session
            .doc
            .features
            .iter()
            .enumerate()
            .filter_map(|(i, f)| (f.component == component && (compact || !matches!(f.kind, FeatureKind::Sketch { .. }))).then_some(i))
            .collect(),
    };
    // The feature that made what is under the cursor or picked in the viewport, as the history
    // lights it.
    let from_view = [crate::timeline::hovered_feature(app), crate::timeline::picked_feature(app)];
    let marker = app.session.doc.marker.unwrap_or(app.session.doc.features.len());
    // Where the marker can be dropped: the top of each row (that feature first rolled back) and
    // below the last.
    let mut slots: Vec<(f32, usize)> = Vec::new();
    let mut marker_resp = None;
    for index in indices {
        if compact && marker_resp.is_none() && index >= marker {
            marker_resp = Some(marker_row(app, ui, depth + 1));
        }
        let Some(feature) = app.session.doc.features.get(index) else { continue };
        if compact {
            slots.push((ui.cursor().min.y, index));
        }
        let (id, label, icon) = (feature.id, feature.name.clone(), crate::timeline::icon_of(&feature.kind));
        let sketch = crate::timeline::TimelineKind::of(&feature.kind).profile_sketch();
        // Behind the history marker or suppressed: not built, drawn faint as in the timeline.
        let dim = feature.suppressed || app.session.doc.marker.is_some_and(|marker| index >= marker);
        let selected = app.session.selection.contains(&Sel::Feature { id });
        let mark = if from_view.contains(&Some(id)) { Mark::Here } else { Mark::None };
        app.tree.remember(&format!("f:{id}"));
        let action = sketch.map(|_| ("sketch", "Edit Profile Sketch"));
        // The bodies it made show and hide with its eye (#5099); a component listed on its own
        // stands for all of its placements.
        let placement = (component == 0 || !path.is_empty()).then_some((component, path));
        let bodies = app.session.feature_bodies(&[id], placement);
        let visible = bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b));
        let eye = (!bodies.is_empty()).then_some(visible);
        let r = draw_row(
            ui,
            ui.id().with(("browser_feature", id)),
            &Row { depth: depth + 1, eye, icon, label: &label, selected, dim: dim || eye == Some(false), mark, action, ..Default::default() },
        );
        if r.eye {
            let mut p = json!({"visible": !visible, "features": [id]});
            if !path.is_empty() {
                p["path"] = json!(path);
            }
            let _ = app.run("browser.visibility", p);
        }
        if r.action
            && let Some(sketch) = sketch
        {
            app.edit_sketch(sketch.0);
            continue;
        }
        if let Some(rect) = r.rect {
            crate::scenario::publish_handle(&format!("row:browser-feature:{scope}:{id}"), rect.center());
            if app.tree.reveal_feature.as_ref().is_some_and(|target| target.id == id && target.path == path) {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                app.tree.reveal_feature = None;
            }
        }
        if r.double {
            if !path.is_empty() {
                let _ = app.run("component.activate", json!({"component":component,"occurrence":path.last(),"path":path}));
            }
            app.edit_feature(id);
        } else if r.clicked {
            click_select(app, ui, format!("f:{id}"), Some(Sel::Feature { id }));
        }
        if let Some(at) = r.secondary {
            let is_sketch = app.session.doc.feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::Sketch { .. }));
            acts.menu = Some((at, if is_sketch { Target::Sketch { id } } else { Target::Feature { id } }));
        }
    }
    if compact {
        let end = slots.last().map(|&(_, index)| index + 1);
        let resp = marker_resp.unwrap_or_else(|| marker_row(app, ui, depth + 1));
        if let Some(end) = end {
            slots.push((ui.cursor().min.y, end));
        }
        drag_marker(app, ui, &resp, &slots, marker);
    }
}

/// The history marker between the rows, as in the vertical timeline.
fn marker_row(app: &SolveApp, ui: &mut egui::Ui, depth: usize) -> egui::Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_12), Sense::drag());
    let line = Rect::from_min_max(pos2(rect.left() + 2.0 + depth as f32 * INDENT, rect.top()), rect.max);
    crate::ui_kit::history_marker(ui.painter(), line, true, crate::theme::TabAccent::for_tab(&t, &app.ui.tab));
    crate::scenario::publish_handle("browser:rollback", rect.center());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response.on_hover_text(crate::i18n::tr("ui.history_marker_tip"))
}

/// Dragging the marker shows where it lands; letting go rolls the history there, through the
/// timeline's own command.
fn drag_marker(app: &mut SolveApp, ui: &egui::Ui, resp: &egui::Response, slots: &[(f32, usize)], marker: usize) {
    if !resp.dragged() && !resp.drag_stopped() {
        return;
    }
    let Some(y) = ui.ctx().pointer_latest_pos().map(|p| p.y) else { return };
    let Some(&(slot_y, position)) = slots.iter().min_by(|a, b| (a.0 - y).abs().total_cmp(&(b.0 - y).abs())) else { return };
    if resp.dragged() && position != marker {
        let accent = crate::theme::TabAccent::current(ui.ctx()).color;
        ui.painter().hline(resp.rect.left() + 4.0..=resp.rect.right() - 4.0, slot_y, Stroke::new(2.0, accent));
    }
    if resp.drag_stopped() && position != marker {
        let n = app.session.doc.features.len();
        let _ = app.run_heavy("timeline.roll_to", if position >= n { json!({}) } else { json!({ "position": position }) });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn feature_rows(h: &mut Harness, cache: &str) -> Vec<String> {
        h.app.tree.model = match cache {
            "current" => Some(model::Model::new(&h.app.session)),
            "stale" => Some(model::Model::new(&solidcraft_engine::Session::default())),
            _ => None,
        };
        h.app.tree.expanded.insert("c0/features".into());
        DRAWN.with(|d| d.borrow_mut().clear());
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        h.ctx.run_ui(raw, |ui| rows(&mut h.app, ui, 0, &[], 0, &mut Actions::default())).textures_delta.clear();
        drawn_rows().into_iter().map(|(label, ..)| label).collect()
    }

    /// #5099: a feature row's eye shows and hides the bodies the feature made.
    #[test]
    fn a_feature_rows_eye_hides_the_bodies_it_made() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap();
        h.app.tree.expanded.insert("c0/features".into());
        h.settle();
        h.step(&json!({"click":{"handle":"eye:Box1"}})).unwrap();
        assert_eq!(h.app.session.visibility.hidden_bodies, ["Plate"]);
        h.step(&json!({"click":{"handle":"eye:Box1"}})).unwrap();
        assert!(h.app.session.visibility.hidden_bodies.is_empty());
    }

    #[test]
    fn browser_feature_sketch_only_has_one_sketch_row_and_no_features_folder() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY","name":"Sketch1"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.tree.expanded.insert("c0/features".into());
        let saved = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.frames(2);
        let labels: Vec<_> = drawn_rows().into_iter().map(|(label, ..)| label).collect();
        assert_eq!(labels.iter().filter(|label| *label == "Sketch1").count(), 1, "{labels:?}");
        assert!(!labels.iter().any(|label| label == "Features"), "{labels:?}");
        for cache in ["current", "missing", "stale"] {
            assert!(feature_rows(&mut h, cache).is_empty(), "{cache}: sketch-only document has no Features folder");
        }
        assert_eq!(h.app.session.doc.to_json(), saved);
        assert_eq!(h.app.session.undo.len(), undo);
    }

    #[test]
    fn browser_feature_mixed_history_filters_sketches_with_current_missing_and_stale_cache() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let extrude = h.app.run("solid.extrude", json!({"distance":10,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
        let name = h.app.session.doc.feature(extrude).unwrap().name.clone();
        let saved = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        for cache in ["current", "missing", "stale"] {
            assert_eq!(feature_rows(&mut h, cache), ["Features".to_owned(), name.clone()], "{cache}");
        }
        h.frames(2);
        let labels: Vec<_> = drawn_rows().into_iter().map(|(label, ..)| label).collect();
        assert_eq!(labels.iter().filter(|label| *label == "Profile").count(), 1, "{labels:?}");
        assert!(labels.contains(&name), "{labels:?}");
        find_in_timeline(&mut h.app, extrude, "Plate");
        h.frames(2);
        assert!(h.app.tree.reveal_feature.is_none(), "the creator row was drawn and scrolled to");
        let at = h.call("ui.at", json!({"handle":format!("row:browser-feature:c0:{extrude}")}));
        assert_eq!(at["ok"], true, "{at}");
        assert_eq!(h.app.session.doc.to_json(), saved);
        assert_eq!(h.app.session.undo.len(), undo);
    }

    #[test]
    fn browser_feature_shift_click_selects_the_range_up_and_down() {
        let mut h = Harness::new();
        let ids: Vec<u64> = (0..5)
            .map(|i| {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":format!("B{i}")})).unwrap()["feature"].as_u64().unwrap()
            })
            .collect();
        h.app.tree.expanded.insert("c0/features".into());
        h.frames(2);
        let row = |id: u64| json!({"handle": format!("row:browser-feature:c0:{id}")});
        let all: Vec<Sel> = ids.iter().map(|&id| Sel::Feature { id }).collect();
        let sorted = |s: &[Sel]| {
            let mut v = s.to_vec();
            v.sort_by_key(|x| format!("{x:?}"));
            v
        };
        // Up: anchor on the last row, Shift+click the first.
        h.step(&json!({"click": row(ids[4])})).unwrap();
        h.frames(2);
        h.step(&json!({"click": row(ids[0]), "shift": true})).unwrap();
        h.frames(2);
        assert_eq!(sorted(&h.app.session.selection), sorted(&all), "Shift+click above the anchor");
        // Down: anchor on the first row, Shift+click the last.
        h.step(&json!({"click": row(ids[0])})).unwrap();
        h.frames(2);
        h.step(&json!({"click": row(ids[4]), "shift": true})).unwrap();
        h.frames(2);
        assert_eq!(sorted(&h.app.session.selection), sorted(&all), "Shift+click below the anchor");
    }

    fn find_in_timeline(app: &mut SolveApp, feature: u64, body: &str) {
        let state = app.session.world_state();
        let model_body = state.body(body).unwrap();
        let names = solidcraft_engine::doc::naming::face_names(model_body);
        let index = names.iter().position(|name| name.starts_with(&format!("F{feature}:"))).unwrap();
        app.session.selection = vec![Sel::Face { body: body.into(), index, point: solidcraft_engine::geom::Vec3::ZERO }];
        let item = crate::context_menu::items(app, &Target::Viewport).into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
        assert!(item.enabled);
        crate::context_menu::run_item(app, &item, pos2(0.0, 0.0));
        assert_eq!(app.session.selection, [Sel::Feature { id: feature }]);
    }

    #[test]
    fn find_in_timeline_from_a_sketch_or_plane_row_leaves_the_browser_alone() {
        let mut h = Harness::new();
        let assembly = h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
        h.app.run("component.create", json!({"name":"Part"})).unwrap();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Plate"})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XZ","name":"PartSketch"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[5,25],"p1":[15,32]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let sketch = h.app.session.doc.find_feature("PartSketch").unwrap().id;
        let plane = h.app.run("construct.plane.offset", json!({"base":"XY","offset":30,"name":"PartPlane"})).unwrap()["feature"].as_u64().unwrap();
        h.app.run("occurrence.copy", json!({"occurrence":assembly["occurrence"],"translate":[90,0,0]})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.frames(2);
        let (collapsed, expanded) = (h.app.tree.collapsed.clone(), h.app.tree.expanded.clone());
        // Neither row says which copy it was in: revealing in "the first placement" picked the other copy.
        for (target, id) in [(Target::Sketch { id: sketch }, sketch), (Target::Plane { id: plane }, plane)] {
            let item = crate::context_menu::items(&h.app, &target).into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
            crate::context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
            assert_eq!(h.app.session.selection, [Sel::Feature { id }]);
            assert!(h.app.tree.reveal_feature.is_none(), "{target:?}");
            assert_eq!((&h.app.tree.collapsed, &h.app.tree.expanded), (&collapsed, &expanded), "{target:?}");
        }
    }

    #[test]
    fn find_in_timeline_opens_the_picked_placements_features_folder_and_survives_reopen() {
        let mut h = Harness::new();
        let assembly = h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
        let component = h.app.run("component.create", json!({"name":"Part"})).unwrap();
        let base = h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
        let hole = h.app.run("solid.hole", json!({"position":[20,15,20],"diameter":8})).unwrap()["feature"].as_u64().unwrap();
        let fillet = h.app.run("solid.fillet", json!({"edges":[[0,0,10]],"radius":2})).unwrap()["feature"].as_u64().unwrap();
        h.app.run("solid.hole", json!({"position":[32,20,20],"diameter":4})).unwrap();
        let copy = h.app.run("occurrence.copy", json!({"occurrence":assembly["occurrence"],"translate":[70,25,10]})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence":copy["occurrence"],"axis":[0,0,1],"angle":"35 deg","capture":true})).unwrap();
        let path = vec![copy["occurrence"].as_u64().unwrap(), component["occurrence"].as_u64().unwrap()];
        h.app.run("component.activate", json!({"component":0})).unwrap();
        let world_body = h.app.session.occurrence_bodies(&path, false).unwrap()[0].clone();
        let saved = h.app.session.doc.to_json();
        h.app.session = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::from_json(&saved).unwrap());
        h.app.tree.collapsed.insert("c0".into());
        h.app.tree.collapsed.insert(occurrence_key(&path));
        h.app.preferences.timeline_left_browser_collapsed = true;
        let undo = h.app.session.undo.len();
        for id in [base, hole, fillet] {
            find_in_timeline(&mut h.app, id, &world_body);
            assert_eq!(h.app.tree.reveal_feature.as_ref().unwrap().path, path);
            assert!(!h.app.tree.collapsed.contains("c0"));
            assert!(!h.app.tree.collapsed.contains(&occurrence_key(&path)));
            assert!(!h.app.preferences.timeline_left_browser_collapsed);
            h.frames(2);
            assert!(h.app.tree.reveal_feature.is_none(), "the actual feature row was drawn and scrolled to");
            let at = h.call("ui.at", json!({"handle":format!("row:browser-feature:{}:{id}",occurrence_key(&path))}));
            assert!(at["ok"].as_bool().unwrap_or(false), "{at}");
        }
        assert_eq!(h.app.session.doc.to_json(), saved);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.active_component, 0);
    }

    #[test]
    fn find_in_timeline_from_a_root_body_opens_the_root_features_folder() {
        let mut h = Harness::new();
        let base = h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
        h.frames(2);
        assert!(
            h.call("ui.at", json!({"handle":format!("row:browser-feature:c0:{base}")}))["ok"].as_bool() != Some(true),
            "the folder starts closed"
        );
        let item =
            crate::context_menu::items(&h.app, &Target::Body { name: "Plate".into() }).into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
        crate::context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
        assert!(h.app.tree.expanded.contains("c0/features"));
        h.frames(2);
        let at = h.call("ui.at", json!({"handle":format!("row:browser-feature:c0:{base}")}));
        assert!(at["ok"].as_bool().unwrap_or(false), "{at}");
        assert_eq!(h.app.session.selection, [Sel::Feature { id: base }]);
    }

    #[test]
    fn browser_find_from_a_body_or_its_face_reveals_the_body() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap();
        for pick in [Sel::Body { name: "Plate".into() }, Sel::Face { body: "Plate".into(), index: 0, point: solidcraft_engine::geom::Vec3::ZERO }] {
            h.app.tree.reveal = None;
            h.app.session.selection = vec![pick.clone()];
            let item = crate::context_menu::items(&h.app, &Target::Viewport).into_iter().find(|item| item.id == "ui.findBrowser").unwrap();
            assert!(item.enabled, "{pick:?}");
            crate::context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
            assert_eq!(h.app.tree.reveal.as_deref(), Some("Plate"), "{pick:?}");
            assert_eq!(h.app.session.selection, [Sel::Body { name: "Plate".into() }], "{pick:?}");
        }
    }

    fn find_in_browser(app: &mut SolveApp, pick: Sel) {
        app.session.selection = vec![pick];
        let item = crate::context_menu::items(app, &Target::Viewport).into_iter().find(|item| item.id == "ui.findBrowser").unwrap();
        crate::context_menu::run_item(app, &item, pos2(0.0, 0.0));
    }

    fn face(body: &str) -> Sel {
        Sel::Face { body: body.into(), index: 0, point: solidcraft_engine::geom::Vec3::ZERO }
    }

    #[test]
    fn browser_find_opens_the_collapsed_rows_above_the_body() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap();
        let group = h.app.run("browser.group", json!({"component":0,"folder":"bodies","items":["Plate"]})).unwrap()["group"].as_u64().unwrap();
        for key in ["c0".to_string(), "c0/bodies".to_string(), format!("c0/g{group}")] {
            h.app.tree.collapsed.insert(key);
        }
        h.frames(2);
        assert!(!drawn_rows().iter().any(|(label, ..)| label == "Plate"), "the body starts hidden");
        find_in_browser(&mut h.app, face("Plate"));
        h.frames(2);
        assert!(drawn_rows().iter().any(|(label, ..)| label == "Plate"), "{:?}", drawn_rows());
        assert!(h.app.tree.reveal.is_none(), "the body row was drawn and scrolled to");
        assert_eq!(h.app.session.selection, [Sel::Body { name: "Plate".into() }]);
    }

    #[test]
    fn browser_find_reveals_a_body_in_its_own_nested_placement() {
        let mut h = Harness::new();
        let assembly = h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
        let component = h.app.run("component.create", json!({"name":"Part"})).unwrap();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Plate"})).unwrap();
        let copy = h.app.run("occurrence.copy", json!({"occurrence":assembly["occurrence"],"translate":[70,25,10]})).unwrap();
        let first = vec![assembly["occurrence"].as_u64().unwrap(), component["occurrence"].as_u64().unwrap()];
        let later = vec![copy["occurrence"].as_u64().unwrap(), component["occurrence"].as_u64().unwrap()];
        h.app.run("component.activate", json!({"component":0})).unwrap();
        // The first placement's body keeps the definition's name, which every component's first
        // placement (the root's too) would also give; a later one has its own world name.
        for (path, named_plate) in [(first, true), (later, false)] {
            let world_body = h.app.session.occurrence_bodies(&path, false).unwrap()[0].clone();
            assert_eq!(world_body == "Plate", named_plate);
            h.app.tree.collapsed.insert("c0".into());
            for depth in 1..=path.len() {
                h.app.tree.collapsed.insert(occurrence_key(&path[..depth]));
            }
            h.app.tree.collapsed.insert(format!("{}/bodies", occurrence_key(&path)));
            find_in_browser(&mut h.app, face(&world_body));
            for depth in 1..=path.len() {
                assert!(!h.app.tree.collapsed.contains(&occurrence_key(&path[..depth])), "{world_body}: {:?}", &path[..depth]);
            }
            assert!(!h.app.tree.collapsed.contains(&format!("{}/bodies", occurrence_key(&path))), "{world_body}");
            h.frames(2);
            assert!(h.app.tree.reveal.is_none(), "{world_body}: the placed body row was drawn and scrolled to");
            assert_eq!(h.app.session.selection, [Sel::Body { name: world_body }]);
        }
    }

    #[test]
    fn browser_find_reveals_a_placed_body_whose_row_is_off_screen() {
        let mut h = Harness::new();
        let component = h.app.run("component.create", json!({"name":"Part"})).unwrap();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        let first = component["occurrence"].as_u64().unwrap();
        // Enough placements that the last one's row sits below the Browser's visible height.
        let mut last = first;
        for i in 1..=80 {
            last = h.app.run("occurrence.copy", json!({"occurrence":first,"translate":[20.0 * f64::from(i),0,0]})).unwrap()["occurrence"]
                .as_u64()
                .unwrap();
        }
        h.app.run("browser.collapse_all", json!({})).unwrap();
        h.frames(2);
        let world_body = h.app.session.occurrence_bodies(&[last], false).unwrap()[0].clone();
        // A later placement's body has its own world name; the row's label stays the local one.
        assert_ne!(world_body, "Plate");
        let plate_rows = || drawn_rows().into_iter().filter(|(label, ..)| label == "Plate").count();
        assert_eq!(plate_rows(), 0, "the body starts off screen and collapsed");
        find_in_browser(&mut h.app, face(&world_body));
        // The row is off screen, so the reveal only gives it a rect to scroll to; the scroll then
        // animates over several frames until the row is on screen and drawn.
        let arrived = (0..240).any(|_| {
            h.frames(1);
            plate_rows() == 1
        });
        assert!(arrived, "the off-screen placed body row was never scrolled into view");
        assert_eq!(h.app.session.selection, [Sel::Body { name: world_body }]);
    }

    #[test]
    fn browser_feature_imported_source_ids_never_reveal_an_unrelated_native_feature() {
        struct Source(std::path::PathBuf);
        impl Drop for Source {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let source = Source(std::env::temp_dir().join(format!("solidcraft-browser-import-{}-{stamp}.solidcraft", std::process::id())));
        let mut saved = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        saved.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Imported"})).unwrap();
        saved.run("file.save", json!({"path":source.0.to_string_lossy()})).unwrap();
        let mut h = Harness::new();
        let base = h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Native"})).unwrap()["feature"].as_u64().unwrap();
        let inserted = h.app.run("component.insert_design", json!({"path":source.0.to_string_lossy()})).unwrap();
        let path = vec![inserted["occurrence"].as_u64().unwrap()];
        let name = h.app.session.occurrence_bodies(&path, false).unwrap()[0].clone();
        let state = h.app.session.world_state();
        let owner = state.body(&name).unwrap().feature;
        assert_ne!(owner, base);
        h.app.session.selection = vec![Sel::Face { body: name, index: 0, point: solidcraft_engine::geom::Vec3::ZERO }];
        for id in ["ui.findTimeline", "ui.editFeature"] {
            let item = crate::context_menu::items(&h.app, &Target::Viewport).into_iter().find(|item| item.id == id).unwrap();
            assert!(!item.enabled || item.params.as_ref().unwrap()["feature"] == json!(owner), "{id}: {item:?}");
        }
    }

    /// A feature's browser row has the timeline's actions: its sketch button and menu edit the
    /// profile sketch, and the menu suppresses and rolls back; rolled-back rows stay listed.
    #[test]
    fn browser_feature_row_edits_its_sketch_suppresses_and_rolls_back() {
        let mut h = Harness::new();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let pad = h.app.run("solid.extrude", json!({"distance":10,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
        let block = h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Block"})).unwrap()["feature"].as_u64().unwrap();
        let (pad_name, block_name) = (h.app.session.doc.feature(pad).unwrap().name.clone(), h.app.session.doc.feature(block).unwrap().name.clone());
        h.app.tree.expanded.insert("c0/features".into());
        h.frames(2);
        // Only a feature with a profile sketch gets the button.
        assert!(crate::scenario::handle_at(&format!("action:{block_name}")).is_none());
        // The button sits right after the name, not at the far edge of a wide browser.
        let name = crate::scenario::rect_of(&format!("row_label:{pad_name}")).unwrap();
        let button = crate::scenario::handle_at(&format!("action:{pad_name}")).unwrap();
        assert!(button.x > name.right() && button.x < name.right() + 20.0, "{name:?} {button:?}");
        h.step(&json!({"click": {"handle": format!("action:{pad_name}")}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.active_sketch, Some(sketch), "the button opens the profile sketch");
        h.app.finish_sketch();
        h.frames(2);

        let menu = |h: &Harness, id: u64| crate::context_menu::items(&h.app, &Target::Feature { id });
        let edit = menu(&h, pad).into_iter().find(|item| item.id == "ui.editSketch").unwrap();
        assert!(edit.enabled && edit.params.as_ref().unwrap()["sketch"] == json!(sketch), "{edit:?}");
        assert!(!menu(&h, block).into_iter().find(|item| item.id == "ui.editSketch").unwrap().enabled);
        let run = |h: &mut Harness, id: u64, command: &str| {
            let item = menu(h, id).into_iter().find(|item| item.id == command).unwrap();
            crate::context_menu::run_item(&mut h.app, &item, Pos2::ZERO);
            for _ in 0..500 {
                if h.app.deferred.is_none() && h.app.deferred_queue.is_empty() {
                    break;
                }
                h.frame();
            }
            h.frames(2);
        };
        run(&mut h, block, "timeline.suppress");
        assert!(h.app.session.doc.feature(block).unwrap().suppressed);
        run(&mut h, block, "timeline.suppress");
        assert!(!h.app.session.doc.feature(block).unwrap().suppressed, "the second click unsuppresses");
        run(&mut h, pad, "timeline.roll_to");
        let pad_index = h.app.session.doc.feature_index(pad).unwrap();
        assert_eq!(h.app.session.doc.marker, Some(pad_index + 1), "the marker goes right after the feature");
        let labels: Vec<_> = drawn_rows().into_iter().map(|(label, ..)| label).collect();
        assert!(labels.contains(&block_name), "a rolled-back feature stays listed: {labels:?}");
    }

    /// History in the browser: no timeline, the component lists its sketches and features in
    /// history order, and dragging the marker rolls the history back with the timeline's command.
    #[test]
    fn browser_history_mode_lists_the_history_and_drags_the_marker() {
        let mut h = Harness::new();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let pad = h.app.run("solid.extrude", json!({"distance":10,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Block"})).unwrap();
        h.app.run("view.history_layout", json!({"dock":"browser"})).unwrap();
        assert!(h.app.preferences.history_in_browser && !h.app.preferences.timeline_left);
        h.frames(3);
        assert!(h.app.shell.history.is_none(), "no timeline beside the browser history");
        assert!(crate::scenario::handle_at(&format!("row:browser-feature:c0:{sketch}")).is_some(), "the sketch is in the history");
        assert!(crate::scenario::handle_at("browser:rollback").is_some(), "the marker is drawn");

        let row = crate::scenario::handle_at(&format!("row:browser-feature:c0:{pad}")).unwrap();
        h.step(&json!({"drag":[{"handle":"browser:rollback"},[row.x, row.y - 8.0]]})).unwrap();
        for _ in 0..500 {
            if h.app.deferred.is_none() && h.app.deferred_queue.is_empty() {
                break;
            }
            h.frame();
        }
        let pad_index = h.app.session.doc.feature_index(pad).unwrap();
        assert_eq!(h.app.session.doc.marker, Some(pad_index), "dropped above the extrusion, which is rolled back");

        h.app.run("view.history_layout", json!({"dock":"bottom"})).unwrap();
        assert!(!h.app.preferences.history_in_browser);
    }
}
