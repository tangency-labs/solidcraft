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

pub(super) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, component: u64, path: &[u64], depth: usize, acts: &mut Actions) {
    let has_features = match app.tree.model.as_ref().filter(|m| m.matches(&app.session)) {
        Some(model) => model.features.get(&component).is_some_and(|features| !features.is_empty()),
        None => app.session.doc.features.iter().any(|feature| feature.component == component && !matches!(feature.kind, FeatureKind::Sketch { .. })),
    };
    if !has_features {
        return;
    }
    let scope = if path.is_empty() { format!("c{component}") } else { occurrence_key(path) };
    let key = format!("{scope}/features");
    let open = is_open(app, &key, false);
    let r = draw_row(ui, ui.id().with(&key), &Row { depth, fold: Some(open), icon: "folder", label: "Features", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, &key, false);
    }
    if !open {
        return;
    }
    let indices = match app.tree.model.as_ref().filter(|m| m.matches(&app.session)) {
        Some(model) => model.features.get(&component).cloned().unwrap_or_default(),
        None => app
            .session
            .doc
            .features
            .iter()
            .enumerate()
            .filter_map(|(i, f)| (f.component == component && !matches!(f.kind, FeatureKind::Sketch { .. })).then_some(i))
            .collect(),
    };
    for index in indices {
        let Some(feature) = app.session.doc.features.get(index) else { continue };
        let (id, label, icon) = (feature.id, feature.name.clone(), crate::timeline::icon_of(&feature.kind));
        let selected = app.session.selection.contains(&Sel::Feature { id });
        app.tree.order.push(format!("f:{id}"));
        let r = draw_row(ui, ui.id().with(("browser_feature", id)), &Row { depth: depth + 1, icon, label: &label, selected, ..Default::default() });
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
        if r.hovered {
            acts.hover_feature = Some(id);
        }
        if let Some(at) = r.secondary {
            acts.menu = Some((at, Target::Feature { id }));
        }
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

    #[test]
    fn browser_feature_hover_lights_what_its_timeline_item_lights() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        let round = h.app.run("solid.fillet", json!({"edges":[[0,0,10]],"radius":2,"name":"Round"})).unwrap()["feature"].as_u64().unwrap();
        h.app.tree.expanded.insert("c0/features".into());
        h.frames(2);
        let row = crate::scenario::handle_at(&format!("row:browser-feature:c0:{round}")).unwrap();
        h.step(&json!({"move":[row.x,row.y]})).unwrap();
        assert_eq!(h.app.viewport.hover_feature, Some(round));
        assert!(h.app.session.selection.is_empty(), "hovering doesn't select");
        h.step(&json!({"move":[5,5]})).unwrap();
        assert_eq!(h.app.viewport.hover_feature, None);
        // The timeline's own hover still lights it: the browser clears only what it set.
        let item = crate::scenario::handle_at("timeline:Round").unwrap();
        h.step(&json!({"move":[item.x,item.y]})).unwrap();
        assert_eq!(h.app.viewport.hover_feature, Some(round));
        // With the timeline hidden, leaving the row still clears the browser's hover.
        h.app.ui.show_timeline = false;
        h.frames(2);
        let row = crate::scenario::handle_at(&format!("row:browser-feature:c0:{round}")).unwrap();
        h.step(&json!({"move":[row.x,row.y]})).unwrap();
        assert_eq!(h.app.viewport.hover_feature, Some(round));
        h.step(&json!({"move":[5,5]})).unwrap();
        assert_eq!(h.app.viewport.hover_feature, None);
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
}
