//! Native feature rows in the component browser, separate from timeline layout and order.

use super::*;
use crate::history_rows::{HistoryFeature, HistoryRows};

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
    let here = Here { component, path, scope: &scope };
    // The feature that made what is under the cursor or picked in the viewport, as the history
    // lights it, and in the history layout what a selected history row lights.
    let from_view = [crate::timeline::hovered_feature(app), crate::timeline::picked_feature(app)];
    let lit = if compact { lit_by_selection(app) } else { BTreeSet::new() };
    let mark = |app: &SolveApp, index: usize| {
        let id = app.session.doc.features.get(index).map(|f| f.id);
        if id.is_some_and(|id| lit.contains(&id) || from_view.contains(&Some(id))) { Mark::Here } else { Mark::None }
    };
    if !compact {
        for index in indices {
            let _ = feature_row(app, ui, acts, &here, index, depth + 1, None, mark(app, index));
        }
        return;
    }
    let n = app.session.doc.features.len();
    let marker = app.session.doc.marker.unwrap_or(n);
    let plan = history_plan(app, &indices, marker);
    // `plan` counts in the component's own features; a timeline position is a global index.
    let global = |at: usize| indices.get(at).copied().unwrap_or_else(|| indices.last().map_or(n, |last| last + 1));
    // Where the marker can be dropped: the top of each row (it and the sketches listed under it
    // first rolled back) and below the last.
    let mut slots: Vec<(f32, usize)> = Vec::new();
    let mut marker_resp = None;
    for row in &plan.rows {
        let index = global(row.index);
        if marker_resp.is_none() && index >= marker {
            marker_resp = Some(marker_row(app, ui, depth + 1));
        }
        slots.push((ui.cursor().min.y, global(row.slot)));
        // The same fold as in the vertical History, so it stays open when the layout changes.
        let key = crate::timeline_left::sketches_key(row.feature);
        let open = is_open(app, &key, false);
        let listed: Vec<usize> = row.listed.iter().filter(|listed| !listed.reference).map(|listed| global(listed.index)).collect();
        let fold = (!listed.is_empty()).then_some(open);
        if feature_row(app, ui, acts, &here, index, depth + 1, fold, mark(app, index)) {
            toggle(app, &key, false);
        }
        if open {
            for index in listed {
                let _ = feature_row(app, ui, acts, &here, index, depth + 2, None, mark(app, index));
            }
        }
    }
    let resp = marker_resp.unwrap_or_else(|| marker_row(app, ui, depth + 1));
    if !plan.rows.is_empty() {
        slots.push((ui.cursor().min.y, global(indices.len())));
    }
    drag_marker(app, ui, &resp, &slots, marker);
}

/// Where a history row is drawn: its component and placement.
struct Here<'a> {
    component: u64,
    path: &'a [u64],
    scope: &'a str,
}

/// The component's history rows, `indices` its features' timeline positions: with Sketches:
/// Under their feature, each sketch listed under the feature built from it as the vertical
/// History lists it (`history_rows`, without reference rows); otherwise in creation order.
fn history_plan(app: &SolveApp, indices: &[usize], marker: usize) -> HistoryRows {
    let doc = &app.session.doc;
    let features: Vec<HistoryFeature> = indices.iter().filter_map(|index| doc.features.get(*index)).map(HistoryFeature::of).collect();
    if !crate::timeline_left::nests(app) {
        return HistoryRows::in_order(features.iter().map(|feature| (feature.id, feature.sketch)));
    }
    let local = indices.iter().take_while(|index| **index < marker).count();
    crate::history_rows::rows(&features, &doc.timeline_groups, (local < features.len()).then_some(local), false)
}

/// What a selected history row lights up: a feature's sketches, and the features built from a
/// selected sketch.
fn lit_by_selection(app: &SolveApp) -> BTreeSet<u64> {
    let mut lit = BTreeSet::new();
    let doc = &app.session.doc;
    for selection in &app.session.selection {
        let Sel::Feature { id } = selection else { continue };
        let Some(feature) = doc.feature(*id) else { continue };
        if matches!(feature.kind, FeatureKind::Sketch { .. }) {
            lit.extend(doc.features.iter().filter(|f| f.kind.input_sketches().contains(id)).map(|f| f.id));
        } else {
            lit.extend(feature.kind.input_sketches());
        }
    }
    lit
}

/// One feature or sketch row; returns whether its fold arrow was clicked.
#[allow(clippy::too_many_arguments)]
fn feature_row(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    acts: &mut Actions,
    here: &Here,
    index: usize,
    depth: usize,
    fold: Option<bool>,
    mark: Mark,
) -> bool {
    let Some(feature) = app.session.doc.features.get(index) else { return false };
    let (component, path) = (here.component, here.path);
    let (id, label, icon) = (feature.id, feature.name.clone(), crate::timeline::icon_of(&feature.kind));
    let is_sketch = matches!(feature.kind, FeatureKind::Sketch { .. });
    let sketch = crate::timeline::TimelineKind::of(&feature.kind).profile_sketch();
    // Behind the history marker or suppressed: not built, drawn faint as in the timeline.
    let dim = feature.suppressed || app.session.doc.marker.is_some_and(|marker| index >= marker);
    let selected = app.session.selection.contains(&Sel::Feature { id });
    app.tree.remember(&format!("f:{id}"));
    let action = sketch.map(|_| ("sketch", "Edit Profile Sketch"));
    // A sketch row has the eye its Sketches folder row has. A feature's eye shows and hides the
    // bodies it made (#5099); a component listed on its own stands for all of its placements.
    let placement = (component == 0 || !path.is_empty()).then_some((component, path));
    let bodies = if is_sketch { Vec::new() } else { app.session.feature_bodies(&[id], placement) };
    let visible =
        if is_sketch { crate::browser::sketch_visible(app, id) } else { bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b)) };
    let eye = (is_sketch || !bodies.is_empty()).then_some(visible);
    let color = matches!(feature.kind, FeatureKind::ImportPlaceholder { .. }).then_some(Tokens::get().warning);
    let r = draw_row(
        ui,
        ui.id().with(("browser_feature", id)),
        &Row {
            depth,
            fold,
            eye,
            icon,
            label: &label,
            selected,
            dim: dim || (!is_sketch && eye == Some(false)),
            action,
            mark,
            color,
            ..Default::default()
        },
    );
    if r.eye && is_sketch {
        crate::browser::set_sketch_visible(app, id, !visible);
    } else if r.eye {
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
        return false;
    }
    let scope = here.scope;
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
        acts.menu = Some((at, if is_sketch { Target::Sketch { id } } else { Target::Feature { id } }));
    }
    r.fold
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

    #[test]
    fn missing_import_operation_is_visible_and_opens_its_explanation_without_rolling_history() {
        let mut h = Harness::new();
        let kind = FeatureKind::ImportPlaceholder {
            source: "Fillet003".into(),
            source_type: "PartDesign::Fillet".into(),
            detail: "Edge references could not be imported.".into(),
        };
        let id = h.app.session.doc_mut().add_feature(kind, Some("[Not imported] Fillet003")).unwrap();
        h.app.session.refresh();
        for cache in ["current", "missing", "stale"] {
            assert!(feature_rows(&mut h, cache).iter().any(|label| label == "[Not imported] Fillet003"));
        }
        let marker = h.app.session.doc.marker;
        let undo = h.app.session.undo.len();
        h.app.edit_feature(id);
        let detail = h.app.freecad_operation_detail.as_ref().unwrap();
        assert!(detail.summary.contains("Fillet003"));
        assert!(detail.detail.contains("Edge references"));
        assert_eq!(h.app.session.doc.marker, marker);
        assert_eq!(h.app.session.undo.len(), undo);
    }

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

    fn settle_heavy(h: &mut Harness) {
        for _ in 0..500 {
            if h.app.deferred.is_none() && h.app.deferred_queue.is_empty() {
                break;
            }
            h.frame();
        }
        h.frames(2);
    }

    fn count(label: &str) -> usize {
        drawn_rows().iter().filter(|(l, ..)| l == label).count()
    }

    fn fold_of(label: &str) -> Option<bool> {
        drawn_rows().into_iter().find(|(l, ..)| l == label).and_then(|(.., open)| open)
    }

    /// History in the browser: no timeline; with Sketches: Under their feature a sketch sits
    /// under its extrusion, folded; dragging the marker above that extrusion rolls both back with
    /// the timeline's command.
    #[test]
    fn browser_history_mode_nests_a_sketch_under_its_feature_and_drags_the_marker() {
        let mut h = Harness::new();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let pad = h.app.run("solid.extrude", json!({"distance":10,"name":"Pad","body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"name":"Block","body_name":"Block"})).unwrap();
        h.app.run("view.history_layout", json!({"dock":"browser"})).unwrap();
        assert!(h.app.preferences.history_in_browser && !h.app.preferences.timeline_left);
        h.frames(3);
        assert!(h.app.shell.history.is_none(), "no timeline beside the browser history");
        assert_eq!(count("Profile"), 1, "the Sketches folder starts closed: only the sketch's history row");
        assert_eq!(fold_of("Pad"), None, "in creation order nothing is listed under the extrusion");

        h.app.preferences.history_sketches = crate::prefs::HistorySketches::UnderFeature;
        h.frames(2);
        assert_eq!(fold_of("Pad"), Some(false), "the extrusion folds over its sketch");
        assert_eq!(count("Profile"), 0, "folded: the sketch is not a row of its own");
        h.step(&json!({"click": {"handle": "fold:Pad"}})).unwrap();
        h.frames(2);
        assert_eq!(count("Profile"), 1, "unfolded: the sketch is under the extrusion");
        assert!(h.app.tree.expanded.contains(&crate::timeline_left::sketches_key(pad)), "the same fold as the vertical History");
        let rows = drawn_rows();
        let (sketch_row, pad_row) = (rows.iter().find(|(l, ..)| l == "Profile").unwrap().1, rows.iter().find(|(l, ..)| l == "Pad").unwrap().1);
        assert!(sketch_row.top() > pad_row.top(), "the sketch is below its feature");

        let row = crate::scenario::handle_at(&format!("row:browser-feature:c0:{pad}")).unwrap();
        h.step(&json!({"drag":[{"handle":"browser:rollback"},[row.x, row.y - 8.0]]})).unwrap();
        settle_heavy(&mut h);
        let sketch_index = h.app.session.doc.feature_index(sketch).unwrap();
        assert_eq!(h.app.session.doc.marker, Some(sketch_index), "dropped above the extrusion: it and its sketch are rolled back");
        assert_eq!(fold_of("Pad"), Some(true), "rolled back together, the sketch stays under it");

        h.app.run("view.history_layout", json!({"dock":"bottom"})).unwrap();
        assert!(!h.app.preferences.history_in_browser);
    }

    /// The browser lists sketches as the vertical History does (`history_rows`): a shared sketch
    /// under its first feature, an unused one and one the marker separates from its feature as
    /// rows of their own; selecting a feature lights its sketch.
    #[test]
    fn browser_history_mode_lists_sketches_like_the_vertical_history() {
        let mut h = Harness::new();
        let two = h.app.run("sketch.create", json!({"plane":"XY","name":"Two"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[20,0],"p1":[30,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch":"Two","profiles":[0],"distance":5,"name":"Low","body_name":"A"})).unwrap();
        let high = h.app.run("solid.extrude", json!({"sketch":"Two","profiles":[1],"distance":15,"name":"High","body_name":"B"})).unwrap()["feature"]
            .as_u64()
            .unwrap();
        let post = h.app.run("sketch.create", json!({"plane":"XY","name":"PostSketch"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.circle.center", json!({"center":[50,5],"radius":3})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":8,"name":"Post","body_name":"C"})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XZ","name":"Unused"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":3})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.preferences.history_sketches = crate::prefs::HistorySketches::UnderFeature;
        h.app.run("view.history_layout", json!({"dock":"browser"})).unwrap();
        h.frames(3);
        assert_eq!(fold_of("Low"), Some(false), "a shared sketch is listed under its first feature");
        assert_eq!(fold_of("High"), None, "no reference rows in the browser");
        assert_eq!(count("Unused"), 1, "an unused sketch is its own row");
        assert_eq!(fold_of("Post"), Some(false), "the post's own sketch is under it");

        // Selecting a feature lights the row of the sketch it is built from.
        h.app.run("select.set", json!({"items":[{"type":"feature","id":high}]})).unwrap();
        h.frames(2);
        assert!(lit_by_selection(&h.app).contains(&two));

        // The marker between the post's sketch and the post: the sketch is built, the post is
        // not, so the sketch shows on its own above the marker.
        let index = h.app.session.doc.feature_index(post).unwrap();
        h.app.run("timeline.roll_to", json!({"position": index + 1})).unwrap();
        h.frames(2);
        assert_eq!(fold_of("Post"), None, "split by the marker: nothing under the post");
        assert_eq!(count("PostSketch"), 1, "the sketch is its own row");
    }

    /// A sketch row in the browser's history has the eye its Sketches folder row has.
    #[test]
    fn browser_history_sketch_rows_have_an_eye() {
        let mut h = Harness::new();
        h.app.preferences.history_sketches = crate::prefs::HistorySketches::UnderFeature;
        h.app.run("view.history_layout", json!({"dock":"browser"})).unwrap();
        let two = h.app.run("sketch.create", json!({"plane":"XY","name":"Two"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch":"Two","profiles":[0],"distance":5,"name":"Low","body_name":"A"})).unwrap();
        let shown = |h: &Harness| solidcraft_engine::view::sketch_shown(&h.app.session, two);
        h.frames(2);
        assert!(!shown(&h), "a used sketch is hidden");
        h.step(&json!({"click": {"handle": "fold:Low"}})).unwrap();
        h.frames(2);
        let row = crate::scenario::handle_at(&format!("row:browser-feature:c0:{two}")).unwrap();
        let eye = crate::scenario::handle_at("eye:Two").unwrap();
        assert!((eye.y - row.y).abs() < 2.0, "the eye is on the sketch's history row");
        h.step(&json!({"click": {"handle": "eye:Two"}})).unwrap();
        h.frames(2);
        assert!(shown(&h), "its eye shows it");
        h.step(&json!({"click": {"handle": "eye:Two"}})).unwrap();
        h.frames(2);
        assert!(!shown(&h), "and hides it again");
    }
}
