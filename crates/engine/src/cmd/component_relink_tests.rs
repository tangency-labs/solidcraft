use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn folder(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-relink-{case}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn part(path: &Path, height: u32) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": height, "corner": [0,0,0]}));
    run(&mut s, "file.save", json!({"path": path}));
}

fn threaded_part(path: &Path, length: Option<u32>) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 10, "body_name": "Stud"}));
    if let Some(length) = length {
        run(&mut s, "solid.thread", json!({"face": [3,0,5], "designation": "M6", "length": length, "from": [3,0,0]}));
    }
    run(&mut s, "file.save", json!({"path": path}));
    s
}

#[test]
fn linked_cosmetic_threads_keep_their_bodies_placements_and_saved_records() {
    let dir = folder("cosmetic-threads-insert");
    let file = dir.join("part.sc");
    let mut source = threaded_part(&file, Some(5));
    run(&mut source, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [20,0,0], "body_name": "Plate"}));
    run(&mut source, "solid.hole", json!({"position": [30,10,10], "direction": [0,0,-1], "thread": "M6", "depth": 8, "thread_depth": 4}));
    run(&mut source, "file.save", json!({"path": file}));
    let expected = source.world_state().unwrap().threads.clone();
    assert_eq!(expected.len(), 2);
    let mut s = Session::default();
    // A name collision must not attach the imported thread to this unrelated body.
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Stud"}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": file, "translate": [40,0,0]}));
    let feature = inserted["feature"].as_u64().unwrap();
    let component = inserted["component"].as_u64().unwrap();
    assert_eq!(s.model.state().threads.len(), 2, "Insert Design must keep cosmetic threads and threaded holes");
    run(&mut s, "component.insert_design", json!({"path": file, "reuse": true, "translate": [80,0,0]}));
    let check = |s: &Session| {
        let world = s.world_state().unwrap();
        assert_eq!(world.threads.len(), 4);
        for x in [40.0, 80.0] {
            for original in &expected {
                let placed = world
                    .threads
                    .iter()
                    .find(|t| t.internal == original.internal && (t.axis_point.x - original.axis_point.x - x).abs() < 1e-6)
                    .unwrap();
                let body = world.bodies.iter().find(|b| b.name == placed.body).unwrap();
                assert_eq!(s.doc.model_body_component(body), component);
                assert_ne!(placed.body, "Stud");
                let mut wanted = original.clone();
                wanted.feature = feature;
                wanted.body.clone_from(&placed.body);
                wanted.axis_point.x += x;
                assert_eq!(placed, &wanted);
                assert!(!crate::cosmetic_threads::segments(body, &body.mesh(), &world.threads).is_empty());
            }
        }
    };
    check(&s);
    let assembly = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": assembly}));
    std::fs::remove_file(&file).unwrap();
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": assembly}));
    check(&reopened);
    // A fresh evaluation must work without either the source or the saved model cache.
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
    // Older STEP readers may have stored these same two bodies in the opposite order.
    let mut renumbered = (*s.doc).clone();
    let f = renumbered.feature_mut(feature).unwrap();
    assert_eq!(f.body_names.len(), 2);
    f.body_names.reverse();
    f.link_faces.reverse();
    f.link_threads.reverse();
    fn reverse_indices(nodes: &mut [solidcraft_doc::kernel::StepNode]) {
        for node in nodes {
            for index in &mut node.bodies {
                assert!(*index < 2);
                *index = 1 - *index;
            }
            reverse_indices(&mut node.children);
        }
    }
    if let solidcraft_doc::FeatureKind::Import { components, .. } = &mut f.kind {
        assert!(!components.is_empty());
        reverse_indices(components);
    }
    check(&Session::new(renumbered));
    run(&mut reopened, "component.break_link", json!({"component": component}));
    check(&reopened);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_cosmetic_threads_follow_thread_only_updates_cached_reads_and_relink() {
    let dir = folder("cosmetic-threads-update");
    let file = dir.join("part.sc");
    threaded_part(&file, Some(5));
    let mut s = Session::default();
    let first = run(&mut s, "component.insert_design", json!({"path": file}));
    run(&mut s, "component.insert_design", json!({"path": file, "translate": [20,0,0]}));
    assert_eq!(s.model.state().threads.len(), 2);
    for length in [Some(8), None] {
        threaded_part(&file, length);
        let updated = run(&mut s, "component.update_links", json!({}));
        assert_eq!(updated["updated"].as_array().unwrap().len(), 2, "{updated}");
        let threads = s.model.state().threads.clone();
        assert_eq!(threads.len(), if length.is_some() { 2 } else { 0 });
        for t in threads {
            assert!((t.end - t.start - 8.0).abs() < 1e-6);
        }
        assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"].as_array().unwrap().len(), 2);
    }
    let other = dir.join("other.sc");
    threaded_part(&other, Some(3));
    run(&mut s, "component.relink", json!({"component": first["component"], "path": other}));
    assert_eq!(s.model.state().threads.len(), 1);
    let t = &s.model.state().threads[0];
    assert!((t.end - t.start - 3.0).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().threads.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_cosmetic_threads_on_split_bodies_warn_instead_of_duplicating_records() {
    let dir = folder("cosmetic-threads-split");
    let file = dir.join("part.sc");
    let mut source = Session::default();
    run(&mut source, "solid.cylinder", json!({"radius": 3, "height": 10, "body_name": "A"}));
    run(&mut source, "solid.cylinder", json!({"radius": 3, "height": 10, "base": [20,0,0], "body_name": "B"}));
    run(&mut source, "solid.combine", json!({"target": "A", "tools": ["B"], "operation": "join", "touching": "one_body"}));
    run(&mut source, "solid.thread", json!({"face": [3,0,5], "designation": "M6", "length": 5, "from": [3,0,0]}));
    run(&mut source, "file.save", json!({"path": file}));
    assert_eq!(source.model.state().bodies.len(), 1);
    assert_eq!(source.model.state().threads.len(), 1);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": file}));
    assert_eq!(s.model.state().bodies.len(), 2);
    assert!(s.model.state().threads.is_empty());
    let warnings = inserted["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("cosmetic threads") && w.contains("split"))), "{inserted}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_cosmetic_threads_preserve_the_source_occurrence_frame() {
    let dir = folder("cosmetic-threads-source-frame");
    let file = dir.join("part.sc");
    let mut source = Session::default();
    let component = run(&mut source, "component.create", json!({"name": "Stud", "grounded": false}));
    run(&mut source, "solid.cylinder", json!({"radius": 3, "height": 10}));
    run(&mut source, "solid.thread", json!({"face": [3,0,5], "designation": "M6", "length": 5, "from": [3,0,0]}));
    run(&mut source, "occurrence.move", json!({"occurrence": component["occurrence"], "axis": [1,0,0], "angle": "90 deg", "translate": [10,20,30]}));
    run(&mut source, "file.save", json!({"path": file}));
    let original = source.world_state().unwrap().threads[0].clone();
    assert!((original.axis.y + 1.0).abs() < 1e-6);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": file, "translate": [40,0,0]}));
    let world = s.world_state().unwrap();
    assert_eq!(world.threads.len(), 1);
    let thread = &world.threads[0];
    assert!((thread.axis_point.x - original.axis_point.x - 40.0).abs() < 1e-6);
    assert!((thread.axis_point.y - original.axis_point.y).abs() < 1e-6);
    assert!((thread.axis_point.z - original.axis_point.z).abs() < 1e-6);
    for (got, wanted) in [(thread.axis.x, original.axis.x), (thread.axis.y, original.axis.y), (thread.axis.z, original.axis.z)] {
        assert!((got - wanted).abs() < 1e-6);
    }
    let body = world.body(&thread.body).unwrap();
    assert!(!crate::cosmetic_threads::segments(body, &body.mesh(), &world.threads).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_cosmetic_threads_refuse_invalid_saved_records_without_partial_state() {
    let dir = folder("cosmetic-threads-invalid");
    let file = dir.join("part.sc");
    threaded_part(&file, Some(5));
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": file}));
    let feature = inserted["feature"].as_u64().unwrap();
    for case in 0..3 {
        let mut doc = (*s.doc).clone();
        let record = &mut doc.feature_mut(feature).unwrap().link_threads[0][0];
        match case {
            0 => record.radius = -1.0,
            1 => record.end = record.start - 1.0,
            _ => record.starts = 9,
        }
        let opened = Session::new(solidcraft_doc::Document::from_json(&doc.to_json()).unwrap());
        let error = opened.model.result(feature).unwrap().error.as_deref().unwrap();
        assert!(error.contains("invalid cosmetic thread record"), "{error}");
        assert!(opened.model.state().threads.is_empty());
        assert!(opened.model.state().bodies.is_empty());
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A design's canvases and decals come with it through Insert Design: in the linked component,
/// where the design shows them on its bodies, so they move with its placement. One that follows
/// sketch points comes in where they put it, and a name the assembly already uses is told apart.
#[test]
fn linked_designs_bring_their_canvases_and_decals() {
    let png = |w: u32, h: u32| {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        crate::tests::image_file(&b)
    };
    let dir = folder("images-insert");
    let file = dir.join("panel.sc");
    let mut source = Session::default();
    run(&mut source, "solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [0,0,0]}));
    run(&mut source, "canvas.decal", json!({"path": png(20, 10), "face": [10,10,10], "width": 10, "name": "Logo"}));
    let tag = run(&mut source, "canvas.insert", json!({"path": png(10, 10), "plane": "XY", "center": [30,10], "width": 8, "name": "Tag"}))["canvas"]
        .as_u64()
        .unwrap();
    run(&mut source, "canvas.frame_sketch", json!({"canvas": tag}));
    run(&mut source, "file.save", json!({"path": file}));
    let corners = |s: &Session, name: &str| {
        let (vals, _) = s.doc.param_values();
        let c = s.doc.canvases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no image `{name}`"));
        s.doc.canvas_corners(&vals, c, &s.model.state()).unwrap()
    };
    let expected = [corners(&source, "Logo"), corners(&source, "Tag")];
    let mut s = Session::default();
    run(&mut s, "canvas.insert", json!({"path": png(10, 10), "plane": "XY", "width": 5, "name": "Logo"}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": file, "translate": [100,0,0]}));
    let component = inserted["component"].as_u64().unwrap();
    let carried: Vec<_> = s.doc.canvases.iter().filter(|c| c.component == component).cloned().collect();
    assert_eq!(carried.len(), 2, "both images come with the design");
    let logo = carried.iter().find(|c| c.name.starts_with("Logo (")).map(|c| c.name.clone()).expect("the taken name is told apart");
    assert!(carried.iter().all(|c| c.anchor.is_none()), "the copies follow no sketch of this design");
    assert_eq!(carried.iter().find(|c| c.name == logo).unwrap().bytes(), source.doc.canvases.iter().find(|c| c.name == "Logo").unwrap().bytes());
    for (name, want) in [(logo.as_str(), expected[0]), ("Tag", expected[1])] {
        let got = corners(&s, name);
        for (g, e) in got.iter().zip(want) {
            assert!((g.x - e.x - 100.0).abs() < 1e-6 && (g.y - e.y).abs() < 1e-6 && (g.z - e.z).abs() < 1e-6, "{name}: {got:?} vs {want:?}");
        }
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// #16829: a design open in the session that has no file yet can be inserted in another
/// design. The body comes in with the names of its faces, its material and its properties,
/// and the link is recorded with no file: `component.links` reports it as a link with no
/// file, and `component.update_links` leaves it alone (it keeps its bodies).
#[test]
fn a_design_held_in_the_session_is_inserted_without_being_saved_first() {
    // The design the caller holds: a part with a material, a look and a property, never saved.
    let mut source = Session::default();
    run(&mut source, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut source, "material.assign", json!({"bodies": ["Cube"], "material": "Steel"}));
    source.doc_mut().appearances.bodies.insert("Cube".into(), solidcraft_doc::appearance::Look::custom([12, 34, 56], 0.4));
    run(&mut source, "document.metadata.set", json!({"values": {"part_number": "P-1", "revision": "A"}}));
    assert!(source.path.is_none(), "the design has no file yet");
    let faces: Vec<String> = {
        let state = source.world_state().unwrap();
        let body = state.bodies.iter().find(|b| b.name == "Cube").expect("the design has its body");
        solidcraft_doc::naming::face_names(body).as_ref().clone()
    };

    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"document": source.doc.to_json()}));
    let component = inserted["component"].as_u64().unwrap();
    let feature = inserted["feature"].as_u64().unwrap();
    // The body comes in as the design has it: its name, its faces, its material and its look.
    assert_eq!(inserted["bodies"], json!(["Cube"]), "{inserted}");
    let mut carried: Vec<String> = s.doc.feature(feature).unwrap().link_faces[0].iter().filter(|n| !n.is_empty()).cloned().collect();
    let mut want: Vec<String> = faces.iter().filter(|n| !n.is_empty()).cloned().collect();
    carried.sort();
    want.sort();
    assert!(!carried.is_empty(), "the faces come in named");
    assert_eq!(carried, want, "the faces keep the names the design gives them");
    assert_link_properties(&mut s, component, "Steel", 7.85, [12, 34, 56]);
    // The design's properties come in too.
    assert_eq!(part_property(&s, component, "part_number").as_deref(), Some("P-1"));
    assert_eq!(part_property(&s, component, "revision").as_deref(), Some("A"));

    // The link names no file: there is none.
    assert_eq!(s.doc.links[&component].path, "");
    let links = run(&mut s, "component.links", json!({}));
    assert_eq!(links["links"][0]["path"], "", "{links}");
    assert_eq!(links["links"][0]["file"], "", "{links}");
    assert!(links["links"][0]["found"].is_null(), "{links}");
    // Update Links leaves it alone: it has no file to read, and keeps its bodies.
    let updated = run(&mut s, "component.update_links", json!({}));
    assert_eq!(updated["updated"], json!([]), "{updated}");
    assert_eq!(updated["unchanged"], json!([]), "{updated}");
    assert_eq!(updated["failed"], json!([]), "{updated}");
    assert_eq!(run(&mut s, "component.links", json!({}))["links"].as_array().map(Vec::len), Some(1));
}

fn properties_part(path: &Path, material: &str, density: Option<f64>, color: [u8; 3]) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    if let Some(density) = density {
        s.doc_mut().user_materials.push(solidcraft_doc::UserMaterial { name: material.into(), density, appearance: None, physical_card: None });
    }
    run(&mut s, "material.assign", json!({"bodies": ["Cube"], "material": material}));
    s.doc_mut().appearances.bodies.insert("Cube".into(), solidcraft_doc::appearance::Look::custom(color, 0.4));
    run(&mut s, "file.save", json!({"path": path}));
    s
}

fn assert_link_properties(s: &mut Session, component: u64, material: &str, mass: f64, color: [u8; 3]) {
    let p = run(s, "component.properties", json!({"component": component}));
    assert_eq!(p["bodies"][0]["material"], material, "{p}");
    assert!((p["total"]["mass_g"].as_f64().unwrap() - mass).abs() < 1e-6, "{p}");
    let state = s.model.state();
    let b = state.bodies.iter().find(|b| b.component == component).unwrap();
    let look = s.doc.model_body_look(b).unwrap();
    assert_eq!(look.color, color);
    assert!((s.doc.model_body_opacity(b) - 0.4).abs() < 1e-9);
}

#[test]
fn linked_design_properties_keep_material_and_appearance_after_insert_and_reload() {
    let dir = folder("properties-insert");
    let file = dir.join("source.solidcraft");
    properties_part(&file, "Steel", None, [12, 34, 56]);
    let mut s = Session::default();
    // The assembly already owns this body name; properties must follow the new name.
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Cube"}));
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    assert_link_properties(&mut s, c, "Steel", 7.85, [12, 34, 56]);
    assert_eq!(s.doc.body_material("Cube"), "Default");
    let assembly = dir.join("assembly.solidcraft");
    run(&mut s, "file.save", json!({"path": assembly}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": assembly}));
    assert_link_properties(&mut cold, c, "Steel", 7.85, [12, 34, 56]);
    run(&mut cold, "component.insert_design", json!({"path": file, "reuse": true}));
    assert_eq!(cold.doc.links.len(), 1);
    assert_link_properties(&mut cold, c, "Steel", 7.85, [12, 34, 56]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_follow_source_only_edits_and_keep_local_overrides() {
    let dir = folder("properties-update");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Steel", None, [12, 34, 56]);
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    run(&mut part, "material.assign", json!({"bodies": ["Cube"], "material": "Aluminum"}));
    part.doc_mut().appearances.bodies.insert("Cube".into(), solidcraft_doc::appearance::Look::custom([80, 90, 100], 0.4));
    run(&mut part, "file.save", json!({}));
    assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"].as_array().unwrap().len(), 1);
    assert_link_properties(&mut s, c, "Aluminum", 2.7, [80, 90, 100]);
    run(&mut s, "edit.undo", json!({}));
    assert_link_properties(&mut s, c, "Steel", 7.85, [12, 34, 56]);
    run(&mut s, "edit.redo", json!({}));
    assert_link_properties(&mut s, c, "Aluminum", 2.7, [80, 90, 100]);
    run(&mut s, "material.assign", json!({"bodies": ["Cube"], "material": "Brass"}));
    s.doc_mut().appearances.bodies.insert("Cube".into(), solidcraft_doc::appearance::Look::custom([1, 2, 3], 0.4));
    run(&mut part, "material.assign", json!({"bodies": ["Cube"], "material": "Steel"}));
    run(&mut part, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert_link_properties(&mut s, c, "Brass", solidcraft_doc::material_density("Brass").unwrap(), [1, 2, 3]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_do_not_replace_conflicting_custom_materials() {
    let dir = folder("properties-custom");
    let file = dir.join("source.solidcraft");
    properties_part(&file, "Custom alloy", Some(4.2), [12, 34, 56]);
    let mut s = Session::default();
    s.doc_mut().user_materials.push(solidcraft_doc::UserMaterial {
        name: "Custom alloy".into(),
        density: 9.0,
        appearance: None,
        physical_card: None,
    });
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    let p = run(&mut s, "component.properties", json!({"component": c}));
    let assigned = p["bodies"][0]["material"].as_str().unwrap();
    assert_ne!(assigned, "Custom alloy");
    assert!((p["total"]["mass_g"].as_f64().unwrap() - 4.2).abs() < 1e-6, "{p}");
    assert_eq!(s.doc.material_density("Custom alloy"), Some(9.0));
    assert_eq!(s.doc.user_materials.len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

/// #21182: every body that brings the same conflicting custom material shares one `(linked N)`
/// copy, rather than each adding its own until the design runs out of custom materials.
#[test]
fn linked_bodies_with_one_conflicting_material_share_its_copy() {
    let dir = folder("properties-custom-shared");
    let file = dir.join("source.solidcraft");
    properties_part(&file, "Custom alloy", Some(4.2), [12, 34, 56]);
    let mut s = Session::default();
    s.doc_mut().user_materials.push(solidcraft_doc::UserMaterial {
        name: "Custom alloy".into(),
        density: 9.0,
        appearance: None,
        physical_card: None,
    });
    let mut assigned = Vec::new();
    for _ in 0..3 {
        let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
        let p = run(&mut s, "component.properties", json!({"component": c}));
        assert!((p["total"]["mass_g"].as_f64().unwrap() - 4.2).abs() < 1e-6, "{p}");
        assigned.push(p["bodies"][0]["material"].as_str().unwrap().to_string());
    }
    assert_eq!(s.doc.user_materials.len(), 2, "{assigned:?}");
    assert!(assigned.iter().all(|m| *m == assigned[0] && m != "Custom alloy"), "{assigned:?}");
    assert_eq!(s.doc.material_density("Custom alloy"), Some(9.0));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_keep_distinct_names_for_equal_material_properties() {
    let dir = folder("properties-equal-density");
    let file = dir.join("source.solidcraft");
    properties_part(&file, "Source alloy", Some(4.2), [12, 34, 56]);
    let mut s = Session::default();
    s.doc_mut().user_materials.push(solidcraft_doc::UserMaterial {
        name: "Different alloy".into(),
        density: 4.2,
        appearance: None,
        physical_card: None,
    });
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    assert_link_properties(&mut s, c, "Source alloy", 4.2, [12, 34, 56]);
    let count = s.doc.user_materials.len();
    for _ in 0..2 {
        assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"].as_array().unwrap().len(), 1);
        assert_eq!(s.doc.user_materials.len(), count);
    }
    assert_eq!(s.doc.user_material("Different alloy").unwrap().density, 4.2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reused_design_insertion_is_free_even_before_a_link_exists() {
    let dir = folder("reuse-grounding");
    let file = dir.join("part.solidcraft");
    part(&file, 30);
    let mut s = Session::default();
    for count in 1..=2 {
        let inserted = run(&mut s, "component.insert_design", json!({"path": file, "reuse": true}));
        let occurrence = inserted["occurrence"].as_u64().unwrap();
        assert!(!s.doc.occurrences.iter().find(|o| o.id == occurrence).unwrap().grounded);
        assert_eq!(s.doc.links.len(), 1);
        assert_eq!(s.undo.len(), count, "each paste is one undo step");
        run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [10, 0, 0]}));
        run(&mut s, "edit.undo", json!({}));
    }
    let mut ordinary = Session::default();
    let inserted = run(&mut ordinary, "component.insert_design", json!({"path": file}));
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    assert!(ordinary.doc.occurrences.iter().find(|o| o.id == occurrence).unwrap().grounded);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reused_design_insertion_uses_the_requested_position_and_survives_undo_and_reopen() {
    let dir = folder("reuse-position");
    let file = dir.join("part.sc");
    part(&file, 30);
    for first_reuses in [false, true] {
        let mut s = Session::default();
        let first = run(&mut s, "component.insert_design", json!({"path": file, "translate": [20, 0, 0], "reuse": first_reuses}));
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let second = run(&mut s, "component.insert_design", json!({"path": file, "translate": [40, 5, 7], "reuse": true}));
        assert_eq!(first["component"], second["component"]);
        assert_eq!(s.doc.links.len(), 1);
        assert_eq!(s.doc.features.len(), 1);
        assert_eq!(s.undo.len(), undo + 1);
        let occurrence = second["occurrence"].as_u64().unwrap();
        assert_linked_bounds(&mut s, occurrence, [40.0, 5.0, 7.0], [50.0, 25.0, 37.0]);
        assert_linked_bounds(&mut s, first["occurrence"].as_u64().unwrap(), [20.0, 0.0, 0.0], [30.0, 20.0, 30.0]);
        assert!(!s.doc.occurrences.iter().find(|o| o.id == occurrence).unwrap().grounded);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        assert_linked_bounds(&mut s, occurrence, [40.0, 5.0, 7.0], [50.0, 25.0, 37.0]);
        let assembly = dir.join("assembly.sc");
        run(&mut s, "file.save", json!({"path": assembly}));
        let mut cold = Session::default();
        run(&mut cold, "doc.open", json!({"path": assembly}));
        assert_linked_bounds(&mut cold, occurrence, [40.0, 5.0, 7.0], [50.0, 25.0, 37.0]);
        let at_origin = run(&mut cold, "component.insert_design", json!({"path": file, "reuse": true}))["occurrence"].as_u64().unwrap();
        assert_linked_bounds(&mut cold, at_origin, [0.0, 0.0, 0.0], [10.0, 20.0, 30.0]);
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn assert_linked_bounds(s: &mut Session, occurrence: u64, min: [f64; 3], max: [f64; 3]) {
    let p = run(s, "occurrence.properties", json!({"occurrence": occurrence}));
    assert_eq!(p["body_count"], 1);
    assert!((p["total"]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-6, "{p}");
    for (key, expected) in [("min", min), ("max", max)] {
        for (axis, expected) in expected.into_iter().enumerate() {
            assert!((p["total"]["bbox"][key][axis].as_f64().unwrap() - expected).abs() < 1e-6, "{key}[{axis}] != {expected}: {p}");
        }
    }
}

#[test]
fn reused_design_insertion_ignores_existing_moves_and_uses_the_destination_parent() {
    let dir = folder("reuse-parent");
    let file = dir.join("part.sc");
    part(&file, 30);
    for capture in [false, true] {
        let mut s = Session::default();
        let first = run(&mut s, "component.insert_design", json!({"path": file, "translate": [20, 0, 0]}));
        run(
            &mut s,
            "occurrence.move",
            json!({"occurrence": first["occurrence"], "translate": [0, 10, 0], "axis": [0, 0, 1], "angle": "90 deg", "capture": capture, "move_grounded": true}),
        );
        let original = s.doc.occurrences.clone();
        let pending = s.pending_moves.clone();
        let parent = run(&mut s, "component.create", json!({"name": "Parent", "activate": false}));
        run(&mut s, "occurrence.move", json!({"occurrence": parent["occurrence"], "translate": [100, 0, 0], "axis": [0, 0, 1], "angle": "90 deg"}));
        for explicit_parent in [true, false] {
            let mut params = json!({"path": file, "translate": [40, 5, 7], "reuse": true});
            if explicit_parent {
                params["parent"] = json!("Parent");
            } else {
                run(&mut s, "component.activate", json!({"component": parent["component"]}));
            }
            let inserted = run(&mut s, "component.insert_design", params);
            assert_eq!(inserted["component"], first["component"]);
            assert_linked_bounds(&mut s, inserted["occurrence"].as_u64().unwrap(), [75.0, 40.0, 7.0], [95.0, 50.0, 37.0]);
            assert_eq!(s.doc.occurrences.first(), original.first());
            assert_eq!(s.pending_moves, pending);
        }
        let before = s.doc.to_json();
        for parent in [json!("missing"), first["component"].clone()] {
            assert!(s.execute("component.insert_design", &json!({"path": file, "parent": parent, "reuse": true})).is_err());
            assert_eq!(s.doc.to_json(), before);
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_update_density_without_geometry_and_keep_edited_cards() {
    let dir = folder("properties-density");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Custom alloy", Some(4.2), [12, 34, 56]);
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    part.doc_mut().user_materials[0].density = 4.8;
    run(&mut part, "file.save", json!({}));
    assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"].as_array().unwrap().len(), 1);
    let p = run(&mut s, "component.properties", json!({"component": c}));
    assert!((p["total"]["mass_g"].as_f64().unwrap() - 4.8).abs() < 1e-6, "{p}");
    let name = p["bodies"][0]["material"].as_str().unwrap().to_string();
    let count = s.doc.user_materials.len();
    for _ in 0..2 {
        assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"].as_array().unwrap().len(), 1);
        assert_eq!(s.doc.user_materials.len(), count, "unchanged source does not duplicate a disambiguated material");
    }
    s.doc_mut().user_materials.iter_mut().find(|m| m.name == name).unwrap().density = 6.0;
    part.doc_mut().user_materials[0].density = 5.0;
    run(&mut part, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert_link_properties(&mut s, c, &name, 6.0, [12, 34, 56]);
    let assembly = dir.join("assembly.solidcraft");
    run(&mut s, "file.save", json!({"path": assembly}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": assembly}));
    assert_link_properties(&mut cold, c, &name, 6.0, [12, 34, 56]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_keep_opaque_pbr_and_face_color_after_reload() {
    use solidcore_record::Vec3;
    use solidcraft_doc::appearance::{FaceLook, Look};
    let dir = folder("properties-optics");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Steel", None, [12, 34, 56]);
    let mut look = Look::custom([12, 34, 56], 1.0);
    look.pbr = Some(solidcraft_doc::pbr_appearance::PbrParameters::new(0.8, 0.23, 1.5).unwrap());
    part.doc_mut().appearances.bodies.insert("Cube".into(), look.clone());
    let top = Vec3::new(5.0, 5.0, 10.0);
    part.doc_mut().appearances.faces.push(FaceLook {
        body: "Cube".into(),
        point: top,
        look: Look::custom([190, 20, 30], 0.65),
        name: None,
        strict_name: false,
        certificate: None,
    });
    run(&mut part, "file.save", json!({}));
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Cube"}));
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    assert!(!s.doc.appearances.faces.iter().any(|face| face.body == "Cube"), "the assembly's existing body keeps its appearance");
    let check = |s: &Session| {
        let state = s.model.state();
        let body = state.bodies.iter().find(|b| b.component == c).unwrap();
        assert_eq!(s.doc.model_body_look(body), Some(look.clone()));
        let face = solidcraft_doc::appearance::face_index_at(body, top).unwrap();
        let face_looks = s.doc.face_colors(body);
        let face_look = &face_looks.iter().find(|(f, _)| *f == face).unwrap().1;
        assert_eq!(face_look.color, [190, 20, 30]);
        assert!((face_look.opacity - 0.65).abs() < 1e-6);
    };
    check(&s);
    let assembly = dir.join("assembly.solidcraft");
    run(&mut s, "file.save", json!({"path": assembly}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": assembly}));
    check(&cold);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_face_overrides_update_independently_and_preserve_local_edits_and_removals() {
    use solidcore_record::Vec3;
    use solidcraft_doc::appearance::{FaceLook, Look};
    let dir = folder("properties-faces");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Steel", None, [12, 34, 56]);
    let top = Vec3::new(5.0, 5.0, 10.0);
    let side = Vec3::new(10.0, 5.0, 5.0);
    for (point, color) in [(top, [190, 20, 30]), (side, [20, 190, 30])] {
        part.doc_mut().appearances.faces.push(FaceLook {
            body: "Cube".into(),
            point,
            look: Look::custom(color, 1.0),
            name: None,
            strict_name: false,
            certificate: None,
        });
    }
    run(&mut part, "file.save", json!({}));
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    let color = |s: &Session, point| {
        let state = s.model.state();
        let body = state.bodies.iter().find(|b| b.component == c).unwrap();
        let index = solidcraft_doc::appearance::face_index_at(body, point).unwrap();
        s.doc.face_colors(body).into_iter().find(|(face, _)| *face == index).map(|(_, look)| look.color)
    };
    assert_eq!(color(&s, top), Some([190, 20, 30]));
    assert_eq!(color(&s, side), Some([20, 190, 30]));
    part.doc_mut().appearances.faces[0].look = Look::custom([30, 40, 200], 1.0);
    part.doc_mut().appearances.faces.pop();
    run(&mut part, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert_eq!(color(&s, top), Some([30, 40, 200]));
    assert_eq!(color(&s, side), None, "a removed source override reveals the body's look");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(color(&s, side), Some([20, 190, 30]));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(color(&s, side), None);
    let mut local = s.doc.appearances.faces[0].clone();
    local.look = Look::custom([200, 180, 30], 1.0);
    s.doc_mut().appearances.faces.push(local);
    let assembly = dir.join("assembly.solidcraft");
    run(&mut s, "file.save", json!({"path": assembly}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": assembly}));
    part.doc_mut().appearances.faces[0].look = Look::custom([40, 50, 60], 1.0);
    run(&mut part, "file.save", json!({}));
    run(&mut cold, "component.update_links", json!({}));
    assert_eq!(color(&cold, top), Some([200, 180, 30]), "a local face edit survives a source edit after reopening");
    cold.doc_mut().appearances.faces.clear();
    part.doc_mut().appearances.faces[0].look = Look::custom([60, 70, 80], 1.0);
    run(&mut part, "file.save", json!({}));
    run(&mut cold, "component.update_links", json!({}));
    assert_eq!(color(&cold, top), None, "an explicit local removal is not restored by a source update");
    // A link saved by the earlier material-only implementation must be read again,
    // even when the file read cache already knows the current source bytes.
    cold.doc_mut().links.get_mut(&c).unwrap().inherited[0].faces = None;
    run(&mut cold, "component.update_links", json!({}));
    assert_eq!(color(&cold, top), Some([60, 70, 80]));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_properties_update_every_component_and_session_sharing_a_cached_source() {
    let dir = folder("properties-shared-read");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Source alloy", Some(4.2), [12, 34, 56]);
    let mut s = Session::default();
    let a = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    let b = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    let mut other = Session::default();
    let c = run(&mut other, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    part.doc_mut().user_materials[0].density = 4.8;
    run(&mut part, "file.save", json!({}));
    let updated = run(&mut s, "component.update_links", json!({}));
    assert_eq!(updated["updated"].as_array().unwrap().len(), 2, "{updated}");
    for component in [a, b] {
        let p = run(&mut s, "component.properties", json!({"component": component}));
        assert!((p["total"]["mass_g"].as_f64().unwrap() - 4.8).abs() < 1e-6, "{p}");
    }
    run(&mut other, "component.update_links", json!({}));
    let p = run(&mut other, "component.properties", json!({"component": c}));
    assert!((p["total"]["mass_g"].as_f64().unwrap() - 4.8).abs() < 1e-6, "{p}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_face_removal_without_a_body_look_does_not_reveal_baked_source_paint() {
    use solidcraft_doc::appearance::{FaceLook, Look};
    let dir = folder("properties-unpainted-body");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Steel", None, [12, 34, 56]);
    part.doc_mut().appearances.bodies.clear();
    part.doc_mut().appearances.faces.push(FaceLook {
        body: "Cube".into(),
        point: solidcore_record::Vec3::new(5.0, 5.0, 10.0),
        look: Look::custom([190, 20, 30], 0.65),
        name: None,
        strict_name: false,
        certificate: None,
    });
    run(&mut part, "file.save", json!({}));
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": file}));
    assert_eq!(s.doc.face_colors(&s.model.state().bodies[0]).len(), 1);
    assert!(s.doc.appearances.bodies.is_empty());
    s.doc_mut().appearances.faces.clear();
    assert!(s.doc.face_colors(&s.model.state().bodies[0]).is_empty(), "local removal must reveal the body's material, not duplicate STEP paint");
    part.doc_mut().appearances.faces[0].look = Look::custom([20, 190, 30], 0.8);
    run(&mut part, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert!(s.doc.face_colors(&s.model.state().bodies[0]).is_empty(), "source edits must preserve the local removal");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unchanged_linked_face_overrides_do_not_reorder_assignments_or_report_updates() {
    use solidcraft_doc::appearance::{FaceLook, Look};
    let dir = folder("properties-face-cache");
    let file = dir.join("source.solidcraft");
    let mut part = properties_part(&file, "Steel", None, [12, 34, 56]);
    part.doc_mut().appearances.faces.push(FaceLook {
        body: "Cube".into(),
        point: solidcore_record::Vec3::new(5.0, 5.0, 10.0),
        look: Look::custom([190, 20, 30], 0.65),
        name: None,
        strict_name: false,
        certificate: None,
    });
    run(&mut part, "file.save", json!({}));
    let mut s = Session::default();
    for _ in 0..2 {
        run(&mut s, "component.insert_design", json!({"path": file}));
    }
    let before = s.doc.clone();
    for _ in 0..2 {
        let result = run(&mut s, "component.update_links", json!({}));
        assert!(result["updated"].as_array().unwrap().is_empty(), "{result}");
        assert_eq!(result["unchanged"].as_array().unwrap().len(), 2, "{result}");
        assert_eq!(s.doc, before, "unchanged links preserve assignment order and inherited baselines");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_survive_save_as_and_relink_with_undo() {
    let dir = folder("properties-relink");
    let first = dir.join("first.solidcraft");
    let second = dir.join("second.solidcraft");
    properties_part(&first, "Steel", None, [12, 34, 56]);
    properties_part(&second, "Aluminum", None, [80, 90, 100]);
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": first}))["component"].as_u64().unwrap();
    let baseline = s.doc.links[&c].inherited.clone();
    let assembly = dir.join("assembly.solidcraft");
    run(&mut s, "file.save", json!({"path": assembly}));
    let saved_as = dir.join("saved-as.solidcraft");
    run(&mut s, "file.save", json!({"path": saved_as}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": saved_as}));
    assert_eq!(cold.doc.links[&c].inherited, baseline);
    run(&mut cold, "component.relink", json!({"component": c, "path": second}));
    assert_link_properties(&mut cold, c, "Aluminum", 2.7, [80, 90, 100]);
    run(&mut cold, "edit.undo", json!({}));
    assert_link_properties(&mut cold, c, "Steel", 7.85, [12, 34, 56]);
    run(&mut cold, "edit.redo", json!({}));
    assert_link_properties(&mut cold, c, "Aluminum", 2.7, [80, 90, 100]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_design_properties_repair_old_links_without_overwriting_assignments() {
    let dir = folder("properties-legacy");
    let file = dir.join("source.solidcraft");
    properties_part(&file, "Steel", None, [12, 34, 56]);
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    s.doc_mut().links.get_mut(&c).unwrap().inherited.clear();
    s.doc_mut().materials.remove("Cube");
    s.doc_mut().appearances.bodies.remove("Cube");
    run(&mut s, "component.update_links", json!({}));
    assert_link_properties(&mut s, c, "Steel", 7.85, [12, 34, 56]);
    s.doc_mut().links.get_mut(&c).unwrap().inherited.clear();
    run(&mut s, "material.assign", json!({"bodies": ["Cube"], "material": "Brass"}));
    s.doc_mut().appearances.bodies.insert("Cube".into(), solidcraft_doc::appearance::Look::custom([1, 2, 3], 0.4));
    run(&mut s, "component.update_links", json!({}));
    assert_link_properties(&mut s, c, "Brass", solidcraft_doc::material_density("Brass").unwrap(), [1, 2, 3]);
    let _ = std::fs::remove_dir_all(dir);
}

fn numbered_part(path: &Path, values: Value) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut s, "document.metadata.set", json!({"values": values}));
    run(&mut s, "file.save", json!({"path": path}));
    s
}

fn part_property(s: &Session, component: u64, key: &str) -> Option<String> {
    s.doc.components.iter().find(|c| c.id == component).and_then(|c| c.metadata.text(key)).map(str::to_string)
}

#[test]
fn linked_part_properties_follow_the_design_and_keep_values_set_in_the_assembly() {
    let dir = folder("part-properties");
    let file = dir.join("plate.sc");
    let mut part = numbered_part(&file, json!({"part_number": "P-1", "revision": "A", "description": "Plate"}));
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": file}))["component"].as_u64().unwrap();
    // A second placement as its own component reads the changed file from the first one's read.
    let other = run(&mut s, "component.insert_design", json!({"path": file, "translate": [30, 0, 0]}))["component"].as_u64().unwrap();
    assert_eq!(part_property(&s, c, "revision").as_deref(), Some("A"));
    assert_eq!(s.doc.links[&c].inherited_metadata.text("part_number"), Some("P-1"));
    // A bill of materials type is the assembly's own: no update touches it.
    run(&mut s, "component.metadata.set", json!({"component": c, "bom_type": "purchased"}));

    // The design's new revision comes in, through one undo step.
    run(&mut part, "document.metadata.set", json!({"values": {"revision": "B"}}));
    run(&mut part, "file.save", json!({}));
    let r = run(&mut s, "component.update_links", json!({}));
    assert_eq!(r["updated"].as_array().unwrap().len(), 2, "{r}");
    for id in [c, other] {
        assert_eq!(part_property(&s, id, "revision").as_deref(), Some("B"));
        assert_eq!(part_property(&s, id, "part_number").as_deref(), Some("P-1"));
    }
    assert_eq!(part_property(&s, c, "bom_type").as_deref(), Some("purchased"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(part_property(&s, c, "revision").as_deref(), Some("A"));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(part_property(&s, c, "revision").as_deref(), Some("B"));

    // A property changed in the assembly stays, and the update says so when the design moved on.
    run(&mut s, "component.metadata.set", json!({"component": c, "description": "Plate, anodised"}));
    run(&mut part, "document.metadata.set", json!({"values": {"description": "Plate v2", "revision": "C"}}));
    run(&mut part, "file.save", json!({}));
    let r = run(&mut s, "component.update_links", json!({}));
    assert_eq!(part_property(&s, c, "description").as_deref(), Some("Plate, anodised"));
    assert_eq!(part_property(&s, c, "revision").as_deref(), Some("C"));
    assert_eq!(part_property(&s, other, "description").as_deref(), Some("Plate v2"));
    let warnings = r["warnings"].to_string();
    assert!(warnings.contains("keeps its part property `description`") && warnings.contains("Plate v2"), "{r}");

    // A property the design drops goes from the component too, and the record survives a save.
    run(&mut part, "document.metadata.remove", json!({"keys": ["revision"]}));
    run(&mut part, "file.save", json!({}));
    run(&mut s, "component.update_links", json!({}));
    assert_eq!(part_property(&s, c, "revision"), None);
    let assembly = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": assembly}));
    let mut cold = Session::default();
    run(&mut cold, "doc.open", json!({"path": assembly}));
    assert_eq!(cold.doc.links[&c].inherited_metadata, s.doc.links[&c].inherited_metadata);
    let r = run(&mut cold, "component.update_links", json!({}));
    assert_eq!(r["updated"], json!([]), "{r}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_part_properties_of_old_links_fill_gaps_without_overwriting_and_follow_a_relink() {
    let dir = folder("part-properties-legacy");
    let (first, second) = (dir.join("first.sc"), dir.join("second.sc"));
    numbered_part(&first, json!({"part_number": "P-1", "revision": "A"}));
    numbered_part(&second, json!({"part_number": "P-2", "revision": "A", "description": "Second plate"}));
    let mut s = Session::default();
    let c = run(&mut s, "component.insert_design", json!({"path": first}))["component"].as_u64().unwrap();
    // A link made before it kept the properties it took: what the component has is its own.
    s.doc_mut().links.get_mut(&c).unwrap().inherited_metadata = solidcraft_doc::Metadata::default();
    run(&mut s, "component.metadata.set", json!({"component": c, "part_number": "LOCAL", "revision": null}));
    let r = run(&mut s, "component.update_links", json!({}));
    assert_eq!(part_property(&s, c, "part_number").as_deref(), Some("LOCAL"));
    assert_eq!(part_property(&s, c, "revision").as_deref(), Some("A"));
    assert!(r["warnings"].to_string().contains("keeps its part property `part_number`"), "{r}");
    // Relink Source: the new design's properties replace the ones the old design gave.
    run(&mut s, "component.metadata.set", json!({"component": c, "part_number": "P-1"}));
    run(&mut s, "component.update_links", json!({}));
    run(&mut s, "component.relink", json!({"component": c, "path": second}));
    assert_eq!(part_property(&s, c, "part_number").as_deref(), Some("P-2"));
    assert_eq!(part_property(&s, c, "description").as_deref(), Some("Second plate"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(part_property(&s, c, "part_number").as_deref(), Some("P-1"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn relink_repairs_a_moved_source_for_every_instance_in_one_undo_step() {
    let dir = folder("moved");
    let old = dir.join("old.solidcraft");
    let moved = dir.join("moved.solidcraft");
    let assembly = dir.join("assembly.solidcraft");
    part(&old, 30);
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": assembly}));
    let inserted = run(&mut s, "component.insert_design", json!({"path": old}));
    let component = inserted["component"].as_u64().unwrap();
    let first = inserted["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [70,30,10]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [0,12,0], "capture": false}));
    let occurrences = s.doc.occurrences.clone();
    let pending = s.pending_moves.clone();
    let before = serde_json::to_value(&*s.doc).unwrap();
    let undo = s.undo.len();
    std::fs::rename(&old, &moved).unwrap();
    part(&moved, 45);
    let result = run(&mut s, "component.relink", json!({"component": component, "path": moved}));
    assert_eq!(result["previous_path"], "old.solidcraft");
    assert_eq!(result["path"], "moved.solidcraft");
    assert_eq!(result["geometry_changed"], true);
    assert_eq!(result["warnings"], json!([]));
    assert_eq!(s.doc.occurrences, occurrences);
    assert_eq!(s.pending_moves, pending);
    assert_eq!(s.undo.len(), undo + 1);
    assert!((volume(&mut s) - 18000.0).abs() < 1e-5);
    let after = serde_json::to_value(&*s.doc).unwrap();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
    assert_eq!(s.pending_moves, pending);
    assert!((volume(&mut s) - 12000.0).abs() < 1e-5);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), after);
    // A move not captured stops a save (#9736).
    run(&mut s, "component.capture_position", json!({}));
    run(&mut s, "file.save", json!({}));
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": assembly}));
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["path"], "moved.solidcraft");
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["found"], true);
    assert!((volume(&mut reopened) - 18000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn relink_rejects_bad_sources_and_targets_without_touching_the_design_or_undo() {
    let dir = folder("failures");
    let source = dir.join("source.solidcraft");
    let assembly = dir.join("assembly.solidcraft");
    let empty = dir.join("empty.solidcraft");
    let corrupt = dir.join("corrupt.solidcraft");
    part(&source, 30);
    let mut s = Session::default();
    let component = run(&mut s, "component.insert_design", json!({"path": source}))["component"].as_u64().unwrap();
    run(&mut s, "file.save", json!({"path": assembly}));
    run(&mut Session::default(), "file.save", json!({"path": empty}));
    std::fs::write(&corrupt, b"not a design").unwrap();
    let before = serde_json::to_value(&*s.doc).unwrap();
    let undo = s.undo.len();
    for params in [
        json!({}),
        json!({"component": component}),
        json!({"component": false, "path": source}),
        json!({"component": 0, "path": source}),
        json!({"component": "unknown", "path": source}),
        json!({"component": component, "path": "   "}),
        json!({"component": component, "path": false}),
        json!({"component": component, "path": dir.join("missing.solidcraft")}),
        json!({"component": component, "path": dir.join("source.step")}),
        json!({"component": component, "path": empty}),
        json!({"component": component, "path": corrupt}),
        json!({"component": component, "path": assembly}),
    ] {
        assert!(s.execute("component.relink", &params).is_err(), "{params}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before, "{params}");
        assert_eq!(s.undo.len(), undo);
        assert!((volume(&mut s) - 6000.0).abs() < 1e-5);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Relink takes any design Insert Design takes: a plain JSON design too, and back to a container.
#[test]
fn relink_takes_a_json_design_as_insert_design_does() {
    let dir = folder("json");
    let (container, plain) = (dir.join("source.sc"), dir.join("source.json"));
    part(&container, 30);
    part(&plain, 45);
    assert!(!solidcraft_io::is_design_container(&std::fs::read(&plain).unwrap()), "file.save writes a .json design as plain JSON");
    let mut s = Session::default();
    let component = run(&mut s, "component.insert_design", json!({"path": container}))["component"].as_u64().unwrap();
    let to_json = run(&mut s, "component.relink", json!({"component": component, "path": plain}));
    assert_eq!(to_json["geometry_changed"], true);
    assert!((volume(&mut s) - 9000.0).abs() < 1e-5);
    run(&mut s, "component.relink", json!({"component": component, "path": container}));
    assert!((volume(&mut s) - 6000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn relink_keeps_joint_definitions_and_reports_solver_diagnostics() {
    let dir = folder("joints");
    let original = dir.join("original.solidcraft");
    let replacement = dir.join("replacement.solidcraft");
    part(&original, 30);
    part(&replacement, 45);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": original}));
    let component = inserted["component"].as_u64().unwrap();
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
    // Explicit points can keep solving after the source changes, even if the intended
    // connection no longer exists. The source-change warning must still be present.
    run(
        &mut s,
        "joint.create",
        json!({"name": "Seat", "type": "rigid", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": occurrence, "point": [0,0,0]}}),
    );
    let joints = s.doc.assembly.joints.clone();
    let identical = dir.join("identical.solidcraft");
    std::fs::copy(&original, &identical).unwrap();
    let result = run(&mut s, "component.relink", json!({"component": component, "path": identical}));
    assert_eq!(result["geometry_changed"], false, "renaming a source is not a geometry change");
    assert_eq!(result["warnings"], json!([]));
    assert_eq!(s.doc.assembly.joints, joints);
    let result = run(&mut s, "component.relink", json!({"component": component, "path": replacement}));
    assert_eq!(s.doc.assembly.joints, joints);
    assert!(result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("check this component's joints"))));
    // A deliberately lost named face exercises the ordinary solver diagnostics, without
    // requiring relink to guess which face of a different design was intended.
    let joint = s.doc_mut().assembly.joints.first_mut().unwrap();
    joint.b.snap =
        solidcraft_doc::joints::Snap::FaceCenter { pick: solidcore_record::Vec3::ZERO, face: Some("no-such-linked-face".to_string()), x: None };
    s.refresh();
    let result = run(&mut s, "component.relink", json!({"component": component, "path": replacement}));
    assert!(result["joint_errors"].as_array().unwrap().iter().any(|e| e["name"] == "Seat"));
    assert!(result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("joint Seat:"))));
    let _ = std::fs::remove_dir_all(dir);
}

/// #12227: relinking one placement of several leaves the others on the old source. The placement
/// shows a component linked to the new design, made once and reused for the next placement.
#[test]
fn relink_of_one_placement_leaves_the_others_on_the_old_source() {
    let dir = folder("placement");
    let original = dir.join("original.solidcraft");
    let taller = dir.join("taller.solidcraft");
    part(&original, 30);
    part(&taller, 45);
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": original}));
    let component = inserted["component"].as_u64().unwrap();
    let first = inserted["occurrence"].as_u64().unwrap();
    let mut copy = |x: i32| run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [x, 0, 0]}))["occurrence"].as_u64().unwrap();
    let (second, third) = (copy(50), copy(100));
    let placed = |s: &Session, o: u64| s.doc.occurrences.iter().find(|x| x.id == o).cloned().unwrap();
    let was = placed(&s, second);
    let link = s.doc.links[&component].clone();
    let before = serde_json::to_value(&*s.doc).unwrap();
    let undo = s.undo.len();

    let result = run(&mut s, "component.relink", json!({"component": component, "occurrence": second, "path": taller}));
    let linked = result["component"].as_u64().unwrap();
    assert_ne!(linked, component);
    assert_eq!(result["previous_component"], component);
    assert_eq!(result["geometry_changed"], true);
    let now = placed(&s, second);
    assert_eq!((now.component, now.parent, now.transform, now.grounded), (linked, was.parent, was.transform, was.grounded));
    assert_eq!([first, third].map(|o| placed(&s, o).component), [component; 2]);
    assert_eq!(s.doc.links[&component], link);
    assert_eq!(s.doc.occurrences.len(), 3);
    assert_eq!(s.undo.len(), undo + 1);
    assert!((volume(&mut s) - (2.0 * 6000.0 + 9000.0)).abs() < 1e-5);
    let components = s.doc.components.len();

    let again = run(&mut s, "component.relink", json!({"component": component, "occurrence": third, "path": taller}));
    assert_eq!(again["component"], linked);
    assert_eq!(s.doc.components.len(), components);
    assert_ne!(placed(&s, second).name, placed(&s, third).name);
    assert!((volume(&mut s) - (6000.0 + 2.0 * 9000.0)).abs() < 1e-5);

    for (params, refusal) in [
        (json!({"component": linked, "occurrence": second, "path": taller}), "already shows this design"),
        (json!({"component": component, "occurrence": second, "path": original}), "doesn't show that component"),
        (json!({"component": component, "occurrence": "first", "path": taller}), "must be a placement id"),
    ] {
        let err = s.execute("component.relink", &params).unwrap_err().to_string();
        assert!(err.contains(refusal), "{params}: {err}");
    }
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
    let _ = std::fs::remove_dir_all(dir);
}

/// A joint on the relinked placement names a face of the bodies it showed: it is moved to the
/// same face of the bodies it shows now.
#[test]
fn relink_of_one_placement_moves_its_joints_to_the_same_faces() {
    let dir = folder("placement-joints");
    let original = dir.join("original.solidcraft");
    let copy = dir.join("copy.solidcraft");
    part(&original, 30);
    std::fs::copy(&original, &copy).unwrap();
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": original}));
    let component = inserted["component"].as_u64().unwrap();
    let first = inserted["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [50, 0, 0]}))["occurrence"].as_u64().unwrap();
    // Not the first face, so a mapping that always answers the first face fails.
    let (index, face) = {
        let state = s.model.state();
        let body = state.bodies.iter().find(|b| b.component == component).unwrap();
        let names = solidcraft_doc::naming::face_names(body);
        (names.len() - 1, names[names.len() - 1].clone())
    };
    run(
        &mut s,
        "joint.create",
        json!({"name": "Seat", "type": "rigid", "a": {"occurrence": first, "point": [0,0,0]}, "b": {"occurrence": second, "face": face}}),
    );

    let result = run(&mut s, "component.relink", json!({"component": component, "occurrence": second, "path": copy}));
    assert_eq!(result["geometry_changed"], false);
    assert_eq!(result["joint_errors"], json!([]), "{result}");
    let linked = result["component"].as_u64().unwrap();
    let names = {
        let state = s.model.state();
        let body = state.bodies.iter().find(|b| b.component == linked).unwrap();
        solidcraft_doc::naming::face_names(body).to_vec()
    };
    let moved = s.doc.assembly.joints[0].b.snap.face_names().into_iter().next().cloned().unwrap();
    assert_ne!(moved, face);
    // The copy is the same design, so the same face comes at the same place in the new body.
    assert_eq!(moved, names[index], "{moved} is not the face that was {face}: {names:?}");
    let _ = std::fs::remove_dir_all(dir);
}

fn configured_part(path: &Path) -> (Session, u64, u64) {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "25 mm"}));
    run(&mut s, "solid.box", json!({"length": "L", "width": 10, "height": 10, "corner": [0,0,0], "body_name": "Bolt"}));
    run(&mut s, "config.configure", json!({"params": ["L"]}));
    run(&mut s, "config.row", json!({"name": "Default", "rename": "Short"}));
    run(&mut s, "config.row", json!({"name": "Long", "values": {"L": "30 mm"}}));
    let id = |name: &str| s.doc.configs.rows.iter().find(|r| r.name == name).unwrap().id;
    let (short, long) = (id("Short"), id("Long"));
    run(&mut s, "config.activate", json!({"row": short}));
    run(&mut s, "file.save", json!({"path": path}));
    (s, short, long)
}

fn assert_variant_length(s: &mut Session, occurrence: u64, length: f64) {
    let p = run(s, "occurrence.properties", json!({"occurrence": occurrence}));
    assert_eq!(p["body_count"], 1, "{p}");
    assert!((p["total"]["volume_mm3"].as_f64().unwrap() - length * 100.0).abs() < 1e-5, "{p}");
    let bbox = &p["total"]["bbox"];
    assert!((bbox["max"][0].as_f64().unwrap() - bbox["min"][0].as_f64().unwrap() - length).abs() < 1e-6, "{p}");
}

#[test]
fn linked_configurations_keep_distinct_sizes_through_reuse_updates_and_offline_reopen() {
    let dir = folder("configuration-lifecycle");
    let file = dir.join("family.sc");
    let assembly = dir.join("assembly.sc");
    let (mut source, short, long) = configured_part(&file);
    let bytes = std::fs::read(&file).unwrap();
    let mut s = Session::default();
    let small = run(&mut s, "component.insert_design", json!({"path": file, "configuration": "Short", "reuse": true}));
    let large = run(&mut s, "component.insert_design", json!({"path": file, "configuration": long, "reuse": true, "translate": [50,0,0]}));
    let saved = run(&mut s, "component.insert_design", json!({"path": file, "reuse": true, "translate": [100,0,0]}));
    let small_id = small["component"].as_u64().unwrap();
    let large_id = large["component"].as_u64().unwrap();
    let saved_id = saved["component"].as_u64().unwrap();
    assert_ne!(small_id, large_id);
    assert_ne!(small_id, saved_id, "an explicit row differs from following the saved state");
    assert_eq!(s.doc.links[&small_id].configuration, Some(short));
    assert_eq!(s.doc.links[&large_id].configuration, Some(long));
    assert_eq!(s.doc.links[&saved_id].configuration, None);
    let reused = run(&mut s, "component.insert_design", json!({"path": file, "configuration": short, "reuse": true}));
    assert_eq!(reused["component"], small_id, "name and fixed id identify the same row");
    let reused_saved = run(&mut s, "component.insert_design", json!({"path": file, "configuration": null, "reuse": true}));
    assert_eq!(reused_saved["component"], saved_id);
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "selecting a row never saves the source");
    let occurrences = [small["occurrence"].as_u64().unwrap(), large["occurrence"].as_u64().unwrap(), saved["occurrence"].as_u64().unwrap()];
    for (occurrence, length) in occurrences.into_iter().zip([25.0, 30.0, 25.0]) {
        assert_variant_length(&mut s, occurrence, length);
    }
    run(&mut source, "config.row", json!({"name": short, "rename": "Short renamed", "values": {"L": "27 mm"}}));
    run(&mut source, "config.activate", json!({"row": long}));
    run(&mut source, "file.save", json!({"path": file}));
    for _ in 0..2 {
        run(&mut s, "component.update_links", json!({}));
        for (occurrence, length) in occurrences.into_iter().zip([27.0, 30.0, 30.0]) {
            assert_variant_length(&mut s, occurrence, length);
        }
    }
    let links = run(&mut s, "component.links", json!({}));
    assert_eq!(links["links"].as_array().unwrap().iter().find(|l| l["component"] == small_id).unwrap()["configuration"], short);
    run(&mut s, "file.save", json!({"path": assembly}));
    std::fs::remove_file(&file).unwrap();
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": assembly}));
    assert_eq!(reopened.doc.links[&small_id].configuration, Some(short));
    for (occurrence, length) in occurrences.into_iter().zip([27.0, 30.0, 30.0]) {
        assert_variant_length(&mut reopened, occurrence, length);
    }
    let mut uncached = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for (occurrence, length) in occurrences.into_iter().zip([27.0, 30.0, 30.0]) {
        assert_variant_length(&mut uncached, occurrence, length);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_configurations_reject_invalid_or_deleted_rows_without_retargeting() {
    let dir = folder("configuration-errors");
    let file = dir.join("family.sc");
    let (mut source, short, long) = configured_part(&file);
    run(&mut source, "config.row", json!({"name": "Degenerate", "values": {"L": "0 mm"}}));
    run(&mut source, "file.save", json!({"path": file}));
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": file, "configuration": short}));
    let component = inserted["component"].as_u64().unwrap();
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    let valid = run(&mut s, "component.insert_design", json!({"path": file, "configuration": long}));
    let valid_component = valid["component"].as_u64().unwrap();
    let valid_occurrence = valid["occurrence"].as_u64().unwrap();
    let before = s.doc.clone();
    let undo = s.undo.len();
    for configuration in [json!("Missing"), json!(u64::MAX), json!(true), json!({}), json!("Degenerate")] {
        for command in ["component.insert_design", "component.relink"] {
            let mut params = json!({"path": file, "configuration": configuration});
            if command == "component.relink" {
                params["component"] = json!(component);
            }
            assert!(s.execute(command, &params).is_err(), "{command}: {params}");
            assert_eq!(s.doc, before);
            assert_eq!(s.undo.len(), undo);
            assert_variant_length(&mut s, occurrence, 25.0);
        }
    }
    run(&mut source, "config.row", json!({"name": long, "values": {"L": "32 mm"}}));
    run(&mut source, "config.activate", json!({"row": long}));
    run(&mut source, "config.row", json!({"name": short, "delete": true}));
    run(&mut source, "config.row", json!({"name": "Short", "values": {"L": "40 mm"}}));
    run(&mut source, "file.save", json!({"path": file}));
    assert!(s.execute("component.update_links", &json!({"component": component})).is_err());
    assert_eq!(s.doc, before, "a new row with the old name must not take the deleted row's link");
    assert_eq!(s.undo.len(), undo);
    assert!(s.execute("component.update_links", &json!({"components": [component]})).is_err());
    assert_eq!(s.doc, before, "a list in which every link fails also rolls back");
    let result = run(&mut s, "component.update_links", json!({"components": [component, valid_component]}));
    assert_eq!(result["failed"].as_array().unwrap().len(), 1);
    assert_eq!(result["updated"].as_array().unwrap().len(), 1);
    assert_eq!(s.doc.links[&component], before.links[&component], "failed list updates retain the old digest and row");
    assert_variant_length(&mut s, occurrence, 25.0);
    assert_variant_length(&mut s, valid_occurrence, 32.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_configurations_reject_row_ids_recycled_after_source_undo() {
    for legacy in [false, true] {
        let dir = folder(if legacy { "configuration-undo-legacy" } else { "configuration-undo" });
        let file = dir.join("family.sc");
        let assembly = dir.join("assembly.sc");
        let (mut source, _, _) = configured_part(&file);
        run(&mut source, "config.row", json!({"name": "Victim", "values": {"L": "35 mm"}}));
        let row = source.doc.configs.rows.iter().find(|r| r.name == "Victim").unwrap().id;
        if legacy {
            source.doc_mut().configs.row_identities.remove(&row);
        }
        run(&mut source, "file.save", json!({"path": file}));
        let mut s = Session::default();
        let inserted = run(&mut s, "component.insert_design", json!({"path": file, "configuration": row, "reuse": true}));
        let component = inserted["component"].as_u64().unwrap();
        let occurrence = inserted["occurrence"].as_u64().unwrap();
        let identity = s.doc.links[&component].configuration_identity;
        assert_eq!(identity.is_none(), legacy);
        run(&mut s, "file.save", json!({"path": assembly}));
        let mut reopened = Session::default();
        run(&mut reopened, "doc.open", json!({"path": assembly}));
        assert_eq!(reopened.doc.links[&component].configuration_identity, identity);
        s = reopened;

        run(&mut source, "edit.undo", json!({}));
        // The replacement deliberately has identical geometry: another link can warm the
        // shared read cache, but neither that cache nor reuse may bypass the identity check.
        run(&mut source, "config.row", json!({"name": "Replacement", "values": {"L": "35 mm"}}));
        let replacement = source.doc.configs.rows.iter().find(|r| r.name == "Replacement").unwrap().id;
        assert_eq!(replacement, row, "the source counter rewound with undo");
        assert_ne!(source.doc.configs.row_identity(row), identity);
        run(&mut source, "file.save", json!({"path": file}));
        let fresh = run(&mut s, "component.insert_design", json!({"path": file, "configuration": row, "reuse": true}));
        let fresh_component = fresh["component"].as_u64().unwrap();
        assert_ne!(fresh_component, component, "a replacement row must not reuse the stale definition");
        let before = s.doc.clone();
        let undo = s.undo.len();
        for (command, params) in
            [("component.update_links", json!({"component": component})), ("component.relink", json!({"component": component, "path": file}))]
        {
            let error = s.execute(command, &params).unwrap_err().to_string();
            assert!(error.contains("replaced"), "{error}");
            assert_eq!(s.doc, before);
            assert_eq!(s.undo.len(), undo);
            assert_variant_length(&mut s, occurrence, 35.0);
        }
        let result = run(&mut s, "component.update_links", json!({"components": [component, fresh_component]}));
        assert_eq!(result["failed"][0]["component"], component);
        assert_eq!(s.doc.links[&component], before.links[&component]);
        // An explicit selection intentionally accepts the replacement and records its identity.
        run(&mut s, "component.relink", json!({"component": component, "path": file, "configuration": row}));
        assert_eq!(s.doc.links[&component].configuration_identity, source.doc.configs.row_identity(row));
        run(&mut source, "config.row", json!({"name": row, "rename": "Renamed", "values": {"L": "80 mm"}}));
        run(&mut source, "file.save", json!({"path": file}));
        run(&mut s, "component.update_links", json!({"component": component}));
        assert_variant_length(&mut s, occurrence, 80.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn relinking_one_occurrence_selects_a_configuration_and_keeps_other_placements() {
    let dir = folder("configuration-placement");
    let file = dir.join("family.sc");
    let (_, short, long) = configured_part(&file);
    let mut s = Session::default();
    let small = run(&mut s, "component.insert_design", json!({"path": file, "configuration": short, "reuse": true}));
    let component = small["component"].as_u64().unwrap();
    let first = small["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [50,0,0]}))["occurrence"].as_u64().unwrap();
    let was = s.doc.occurrences.iter().find(|o| o.id == second).unwrap().clone();
    let before = s.doc.clone();
    let result = run(&mut s, "component.relink", json!({"component": component, "occurrence": second, "path": file, "configuration": "Long"}));
    let linked = result["component"].as_u64().unwrap();
    assert_ne!(linked, component);
    let now = s.doc.occurrences.iter().find(|o| o.id == second).unwrap();
    assert_eq!((now.parent, now.transform, now.grounded), (was.parent, was.transform, was.grounded));
    assert_eq!(s.doc.links[&linked].configuration, Some(long));
    assert_eq!(s.doc.links[&component].configuration, Some(short));
    assert_variant_length(&mut s, first, 25.0);
    assert_variant_length(&mut s, second, 30.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc, before);
    assert_variant_length(&mut s, second, 25.0);
    run(&mut s, "edit.redo", json!({}));
    assert_variant_length(&mut s, second, 30.0);
    // A single placement can change row in place; omitting the selector preserves that row.
    run(&mut s, "component.relink", json!({"component": linked, "path": file}));
    assert_eq!(s.doc.links[&linked].configuration, Some(long));
    assert_variant_length(&mut s, second, 30.0);
    run(&mut s, "component.relink", json!({"component": linked, "path": file, "configuration": null}));
    assert_eq!(s.doc.links[&linked].configuration, None);
    assert_variant_length(&mut s, second, 25.0);
    run(&mut s, "component.relink", json!({"component": linked, "path": file, "configuration": long}));
    let other = dir.join("other.sc");
    part(&other, 45);
    run(&mut s, "component.relink", json!({"component": linked, "path": other}));
    assert_eq!(s.doc.links[&linked].configuration, None, "a different file does not inherit source-local row ids");
    assert!((volume(&mut s) - (2500.0 + 9000.0)).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn relinking_through_a_source_alias_preserves_the_selected_configuration() {
    let dir = folder("configuration-alias");
    let file = dir.join("family.sc");
    let alias = dir.join("alias.sc");
    let (mut source, short, long) = configured_part(&file);
    run(&mut source, "config.activate", json!({"row": long}));
    run(&mut source, "file.save", json!({"path": file}));
    std::os::unix::fs::symlink(&file, &alias).unwrap();
    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": file, "configuration": short}));
    let component = inserted["component"].as_u64().unwrap();
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    for path in [&alias, &file] {
        let result = run(&mut s, "component.relink", json!({"component": component, "path": path}));
        assert_eq!(result["geometry_changed"], false);
        assert_eq!(s.doc.links[&component].configuration, Some(short));
        assert_variant_length(&mut s, occurrence, 25.0);
    }
    // Repairing a refused old source must still work, without treating it as an alias.
    s.doc_mut().links.get_mut(&component).unwrap().path = r"\\blocked-link-server\parts\family.sc".into();
    run(&mut s, "component.relink", json!({"component": component, "path": file}));
    assert_eq!(s.doc.links[&component].configuration, None);
    run(&mut s, "component.relink", json!({"component": component, "path": file, "configuration": short}));
    run(&mut s, "component.relink", json!({"component": component, "path": alias, "configuration": null}));
    assert_eq!(s.doc.links[&component].configuration, None);
    assert_variant_length(&mut s, occurrence, 30.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linked_configuration_cache_keeps_materials_distinct_for_identical_geometry() {
    let dir = folder("configuration-material-cache");
    let file = dir.join("family.sc");
    let mut source = properties_part(&file, "Steel", None, [12, 34, 56]);
    run(&mut source, "config.configure", json!({"materials": ["Cube"]}));
    run(&mut source, "config.row", json!({"name": "Default", "rename": "Steel row"}));
    run(&mut source, "config.row", json!({"name": "Aluminum row", "values": {"material:Cube": "Aluminum"}}));
    run(&mut source, "file.save", json!({"path": file}));
    let mut s = Session::default();
    let steel = run(&mut s, "component.insert_design", json!({"path": file, "configuration": "Steel row"}))["component"].as_u64().unwrap();
    let aluminum = run(&mut s, "component.insert_design", json!({"path": file, "configuration": "Aluminum row"}))["component"].as_u64().unwrap();
    for _ in 0..2 {
        run(&mut s, "component.update_links", json!({"components": [steel, aluminum]}));
        assert_link_properties(&mut s, steel, "Steel", 7.85, [12, 34, 56]);
        assert_link_properties(&mut s, aluminum, "Aluminum", 2.7, [12, 34, 56]);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn replace_with_link_by_path_does_not_reuse_a_pinned_configuration() {
    let dir = folder("configuration-replacement");
    let file = dir.join("family.sc");
    let (mut source, short, long) = configured_part(&file);
    let mut s = Session::default();
    let pinned = run(&mut s, "component.insert_design", json!({"path": file, "configuration": short}));
    let pinned_component = pinned["component"].as_u64().unwrap();
    let pinned_occurrence = pinned["occurrence"].as_u64().unwrap();
    let local = run(&mut s, "component.create", json!({"name": "Local bolt"}));
    let local_occurrence = local["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 25, "width": 10, "height": 10, "corner": [0,0,0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let replaced = run(&mut s, "component.replace_with_link", json!({"component": local["component"], "path": file}));
    let replacement = replaced["component"].as_u64().unwrap();
    assert_ne!(replacement, pinned_component, "a path alone must follow saved source state even when a pinned row currently matches it");
    assert_eq!(s.doc.links[&replacement].configuration, None);
    assert_variant_length(&mut s, pinned_occurrence, 25.0);
    assert_variant_length(&mut s, local_occurrence, 25.0);
    run(&mut source, "config.activate", json!({"row": long}));
    run(&mut source, "file.save", json!({"path": file}));
    run(&mut s, "component.update_links", json!({}));
    assert_variant_length(&mut s, pinned_occurrence, 25.0);
    assert_variant_length(&mut s, local_occurrence, 30.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn insert_design_accepts_matrix_and_rotation_placement() {
    let dir = folder("placement-test");
    let file = dir.join("part.sc");
    part(&file, 20);

    let mut s = Session::default();

    // 1. Insert with to_matrix (row-major 3x4).
    let to_m = [0.0, 1.0, 0.0, 100.0, -1.0, 0.0, 0.0, 50.0, 0.0, 0.0, 1.0, 25.0];
    let res1 = run(&mut s, "component.insert_design", json!({"path": file, "name": "PlacedMatrix", "to_matrix": to_m}));
    let occ1_id = res1["occurrence"].as_u64().unwrap();
    let occ1 = s.doc.occurrences.iter().find(|o| o.id == occ1_id).unwrap();
    assert!((occ1.transform[1][0] - 1.0).abs() < 1e-6);
    assert!((occ1.transform[0][1] - (-1.0)).abs() < 1e-6);
    assert_eq!(occ1.transform[3], [100.0, 50.0, 25.0, 1.0]);

    // 2. Reuse with to_matrix.
    let to_m2 = [1.0, 0.0, 0.0, 10.0, 0.0, 1.0, 0.0, 20.0, 0.0, 0.0, 1.0, 30.0];
    let res2 = run(&mut s, "component.insert_design", json!({"path": file, "reuse": true, "to_matrix": to_m2}));
    let occ2_id = res2["occurrence"].as_u64().unwrap();
    let occ2 = s.doc.occurrences.iter().find(|o| o.id == occ2_id).unwrap();
    assert_eq!(occ2.transform[3], [10.0, 20.0, 30.0, 1.0]);

    // 3. Insert with axis, angle and translate.
    let res3 = run(
        &mut s,
        "component.insert_design",
        json!({"path": file, "name": "PlacedTurn", "axis": [0, 0, 1], "angle": "90 deg", "translate": [5, 10, 15]}),
    );
    let occ3_id = res3["occurrence"].as_u64().unwrap();
    let occ3 = s.doc.occurrences.iter().find(|o| o.id == occ3_id).unwrap();
    assert!((occ3.transform[0][1] - 1.0).abs() < 1e-6);
    assert!((occ3.transform[1][0] - (-1.0)).abs() < 1e-6);
    assert_eq!(occ3.transform[3], [5.0, 10.0, 15.0, 1.0]);

    // 4. Matrix with length expression in translation column.
    let expr_m = json!([1, 0, 0, "40 mm", 0, 1, 0, "60 mm", 0, 0, 1, 0]);
    let res4 = run(&mut s, "component.insert_design", json!({"path": file, "reuse": true, "matrix": expr_m}));
    let occ4_id = res4["occurrence"].as_u64().unwrap();
    let occ4 = s.doc.occurrences.iter().find(|o| o.id == occ4_id).unwrap();
    assert_eq!(occ4.transform[3], [40.0, 60.0, 0.0, 1.0]);

    // 5. Conflicting parameters refused.
    assert!(s.execute("component.insert_design", &json!({"path": file, "to_matrix": to_m, "matrix": to_m})).is_err());
    assert!(s.execute("component.insert_design", &json!({"path": file, "to_matrix": to_m, "translate": [1, 0, 0]})).is_err());
    assert!(s.execute("component.insert_design", &json!({"path": file, "matrix": to_m, "axis": [0, 0, 1]})).is_err());

    let _ = std::fs::remove_dir_all(dir);
}

/// #20890: deleting a component removes its own placements of a linked part; a link another
/// component still places stays, with its bodies, and goes with the last placement.
#[test]
fn deleting_a_component_keeps_a_linked_part_another_component_still_places() {
    let dir = folder("delete-shared-link");
    let file = dir.join("part.sc");
    part(&file, 30);
    let mut s = Session::default();
    let route1 = run(&mut s, "component.create", json!({"name": "Route 1", "grounded": false}))["component"].as_u64().unwrap();
    let first = run(&mut s, "component.insert_design", json!({"path": file, "name": "Elbow", "reuse": false, "translate": [500, 0, 0]}));
    let elbow = first["component"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": 0}));
    let route2 = run(&mut s, "component.create", json!({"name": "Route 2", "grounded": false}))["component"].as_u64().unwrap();
    let second = run(&mut s, "component.insert_design", json!({"path": file, "name": "Elbow", "reuse": true, "translate": [1000, 0, 0]}));
    assert_eq!(second["component"], first["component"]);
    run(&mut s, "component.activate", json!({"component": 0}));

    let deleted = run(&mut s, "component.delete", json!({"component": "Route 1"}));
    let gone: Vec<u64> = deleted["deleted"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    assert_eq!(gone, vec![route1], "{deleted}");
    assert!(s.doc.components.iter().any(|c| c.id == elbow), "the link another component places stays");
    assert!(s.doc.links.contains_key(&elbow));
    let kept = second["occurrence"].as_u64().unwrap();
    let placed: Vec<_> = s.doc.occurrences.iter().filter(|o| o.component == elbow).collect();
    assert_eq!(placed.len(), 1, "only Route 2's placement is left");
    assert_eq!((placed[0].id, placed[0].parent), (kept, route2));
    assert_linked_bounds(&mut s, kept, [1000.0, 0.0, 0.0], [1010.0, 20.0, 30.0]);
    assert_eq!(s.model.state().bodies.len(), 1, "the part's body is still there");
    let owner = s.doc.components.iter().find(|c| c.id == elbow).map(|c| c.parent);
    assert!(
        owner.is_some_and(|p| p == 0 || p == route2 || s.doc.components.iter().any(|c| c.id == p)),
        "the kept link is not left under a deleted component"
    );

    // The last placement takes the link with it.
    let deleted = run(&mut s, "component.delete", json!({"component": "Route 2"}));
    let gone: Vec<u64> = deleted["deleted"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    assert!(gone.contains(&route2) && gone.contains(&elbow), "{deleted}");
    assert!(s.doc.links.is_empty());
    assert!(s.doc.occurrences.is_empty());
    assert!(s.model.state().bodies.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// #20890: a kept linked part moves up to the component the deleted one sat in, not to the root.
#[test]
fn a_kept_linked_part_moves_up_to_the_component_the_deleted_one_sat_in() {
    let dir = folder("delete-shared-link-nested");
    let file = dir.join("part.sc");
    part(&file, 30);
    let mut s = Session::default();
    let outer = run(&mut s, "component.create", json!({"name": "Outer", "grounded": false}))["component"].as_u64().unwrap();
    let inner = run(&mut s, "component.create", json!({"name": "Inner", "grounded": false}))["component"].as_u64().unwrap();
    assert_eq!(s.doc.components.iter().find(|c| c.id == inner).map(|c| c.parent), Some(outer));
    let first = run(&mut s, "component.insert_design", json!({"path": file, "name": "Elbow", "reuse": false, "translate": [500, 0, 0]}));
    let elbow = first["component"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Other", "grounded": false}));
    run(&mut s, "component.insert_design", json!({"path": file, "name": "Elbow", "reuse": true, "translate": [1000, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));

    run(&mut s, "component.delete", json!({"component": "Inner"}));
    assert!(s.doc.components.iter().all(|c| c.id != inner));
    assert_eq!(s.doc.components.iter().find(|c| c.id == elbow).map(|c| c.parent), Some(outer));
    let _ = std::fs::remove_dir_all(dir);
}

/// What a link stores and what survives a moved job folder (#6892, slice 2 of the separate-files
/// plan). A link keeps its path relative to the folder of the design that links it, written with
/// `/`: a part beside it, a part in a folder below, a sub-assembly that links its own part
/// relative to itself, and a part in a library folder beside the job (`../lib`). Moving the job
/// folder whole keeps every link inside it, the sub-assembly's own included; a library left
/// behind is lost by a link that climbs out of the job (`..`), and found again once it moves
/// along. Update Links meanwhile reads every file it finds and names, by name and file, the one
/// it can't. Nothing here needs the experimental preference: relative links are how links work.
#[test]
fn a_job_folder_moved_whole_keeps_its_links_and_loses_a_library_left_behind() {
    let root = folder("job-moves");
    let (job, lib) = (root.join("job"), root.join("lib"));
    for dir in [job.join("parts"), job.join("sub"), lib.clone()] {
        std::fs::create_dir_all(dir).unwrap();
    }
    part(&job.join("plate.sc"), 20);
    part(&job.join("parts").join("bracket.sc"), 30);
    part(&lib.join("bolt.sc"), 10);
    let mut hinge = Session::default();
    run(&mut hinge, "file.save", json!({"path": job.join("sub").join("hinge.sc")}));
    run(&mut hinge, "component.insert_design", json!({"path": job.join("parts").join("bracket.sc")}));
    run(&mut hinge, "file.save", json!({}));
    let mut top = Session::default();
    run(&mut top, "file.save", json!({"path": job.join("top.sc")}));
    for (file, x) in
        [(job.join("plate.sc"), 0), (job.join("parts").join("bracket.sc"), 20), (lib.join("bolt.sc"), 40), (job.join("sub").join("hinge.sc"), 60)]
    {
        run(&mut top, "component.insert_design", json!({"path": file, "translate": [x, 0, 0]}));
    }
    run(&mut top, "file.save", json!({}));
    let links = |path: &Path| -> Vec<(String, bool)> {
        let mut s = Session::default();
        run(&mut s, "doc.open", json!({"path": path}));
        let links = run(&mut s, "component.links", json!({}));
        let mut found: Vec<(String, bool)> =
            links["links"].as_array().unwrap().iter().map(|l| (l["path"].as_str().unwrap().to_string(), l["found"] == true)).collect();
        found.sort();
        found
    };
    let stored = |found: &[(&str, bool)]| found.iter().map(|(p, f)| ((*p).to_string(), *f)).collect::<Vec<_>>();
    // What the files keep: paths relative to each design's own folder.
    assert_eq!(
        links(&job.join("top.sc")),
        stored(&[("../lib/bolt.sc", true), ("parts/bracket.sc", true), ("plate.sc", true), ("sub/hinge.sc", true)])
    );
    assert_eq!(links(&job.join("sub").join("hinge.sc")), stored(&[("../parts/bracket.sc", true)]));
    // The job folder moves one level down, the library stays: links inside the job are found,
    // the library's is not; the sub-assembly still finds its part, relative to itself.
    let moved = root.join("archive").join("job");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&job, &moved).unwrap();
    assert_eq!(
        links(&moved.join("top.sc")),
        stored(&[("../lib/bolt.sc", false), ("parts/bracket.sc", true), ("plate.sc", true), ("sub/hinge.sc", true)])
    );
    assert_eq!(links(&moved.join("sub").join("hinge.sc")), stored(&[("../parts/bracket.sc", true)]));
    // Update Links reads every file it finds and names the one it can't, with the file it looked
    // for; that part keeps its bodies.
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": moved.join("top.sc")}));
    let bodies = s.model.state().bodies.len();
    let updated = run(&mut s, "component.update_links", json!({}));
    let mut read: Vec<&str> =
        updated["unchanged"].as_array().unwrap().iter().chain(updated["updated"].as_array().unwrap()).filter_map(Value::as_str).collect();
    read.sort_unstable();
    assert_eq!(read, ["bracket", "hinge", "plate"], "{updated}");
    let failed = &updated["failed"][0];
    assert!(failed["name"] == "bolt" && failed["error"].as_str().unwrap().contains("not found"), "{updated}");
    assert!(failed["path"].as_str().unwrap().replace('\\', "/").ends_with("archive/lib/bolt.sc"), "{updated}");
    assert_eq!(s.model.state().bodies.len(), bodies, "the missing part keeps its bodies");
    // The library moves along, beside the job again: found.
    std::fs::rename(&lib, root.join("archive").join("lib")).unwrap();
    assert!(links(&moved.join("top.sc")).iter().all(|(_, found)| *found));
    let _ = std::fs::remove_dir_all(root);
}

/// Library folders (#6892): a linked part file that isn't where its link says is looked for in
/// the library folders, with the file kinds on. Update Links reads it from there and says so; the
/// link keeps naming its own file. Without the folder it isn't found. Part files of
/// a kind (`.scprt`) are linked like any design.
#[test]
fn a_part_moved_into_a_library_folder_is_found_there_and_said_so() {
    crate::set_experimental_file_kinds(true);
    let root = folder("part-library");
    let (job, library) = (root.join("job"), root.join("library"));
    std::fs::create_dir_all(job.join("parts")).unwrap();
    std::fs::create_dir_all(library.join("fasteners")).unwrap();
    let bolt = job.join("parts").join("bolt.scprt");
    part(&bolt, 10);
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": job.join("top.scasm")}));
    run(&mut s, "component.insert_design", json!({"path": bolt}));
    run(&mut s, "file.save", json!({}));
    // The part moves into the library, one folder down, and changes there.
    std::fs::rename(&bolt, library.join("fasteners").join("bolt.scprt")).unwrap();
    part(&library.join("fasteners").join("bolt.scprt"), 25);
    // Without a library folder it isn't found (the only link, so the update is refused).
    let refused = s.execute("component.update_links", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("bolt") && refused.contains("not found"), "{refused}");
    crate::set_part_library_folders(vec![library.to_string_lossy().into_owned()]);
    let updated = run(&mut s, "component.update_links", json!({}));
    crate::set_part_library_folders(Vec::new());
    assert_eq!(updated["updated"], json!(["bolt"]), "{updated}");
    let note = updated["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).find(|w| w.contains("found in the library folder"));
    assert!(note.is_some_and(|n| n.starts_with("bolt: not at") && n.contains("fasteners")), "{updated}");
    assert!((volume(&mut s) - 5000.0).abs() < 1e-6, "the library's part came in: {}", volume(&mut s));
    run(&mut s, "file.save", json!({}));
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": job.join("top.scasm")}));
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["path"], "parts/bolt.scprt", "the link still names its own file");
    let _ = std::fs::remove_dir_all(root);
}

/// A part stored inside a design's file (#6892): `component.make_internal` copies a linked part in,
/// unpacked beside the design, and the component follows that copy. The design's file holds the
/// part, so it opens complete somewhere else with nothing beside it and unpacks the part again. An
/// edit saved in the unpacked part is reported out of date, comes in with Update Links, and is in
/// the design's file after its next save. A read-only open unpacks nothing and still reads the part.
#[test]
fn a_part_stored_inside_a_design_travels_in_its_file_and_is_edited_unpacked() {
    crate::set_experimental_file_kinds(true);
    let root = folder("stored-inside");
    let (job, away) = (root.join("job"), root.join("away"));
    std::fs::create_dir_all(job.join("parts")).unwrap();
    std::fs::create_dir_all(&away).unwrap();
    let bracket = job.join("parts").join("bracket.scprt");
    part(&bracket, 10);
    let mut s = Session::default();
    // An unsaved design has no file to store a part in.
    run(&mut s, "component.insert_design", json!({"path": bracket}));
    let refused = s.execute("component.make_internal", &json!({"component": "bracket"})).unwrap_err().to_string();
    assert!(refused.contains("save this design first"), "{refused}");
    run(&mut s, "file.save", json!({"path": job.join("counter.sc")}));
    let stored = run(&mut s, "component.make_internal", json!({"component": "bracket"}));
    assert_eq!(stored["member"], "bracket.scprt", "{stored}");
    assert_eq!(stored["previous_path"], "parts/bracket.scprt", "{stored}");
    let work = PathBuf::from(stored["file"].as_str().unwrap());
    assert_eq!(work, job.join(".~counter.sc.parts").join("bracket.scprt"));
    // The unpacked part is the linked file with the design's tag in its header and nothing else changed.
    let untagged = |path: &Path| solidcraft_io::with_internal_of(&std::fs::read(path).unwrap(), None).unwrap();
    assert!(
        solidcraft_io::read_internal_of_file(&work.to_string_lossy()).is_some_and(|tag| tag.design == "counter.sc"),
        "the unpacked part is tagged"
    );
    assert!(untagged(&work) == untagged(&bracket), "the unpacked part is the linked file but for its tag");
    let again = s.execute("component.make_internal", &json!({"component": "bracket"})).unwrap_err().to_string();
    assert!(again.contains("already stored"), "{again}");
    let links = run(&mut s, "component.links", json!({}));
    assert_eq!(links["links"][0]["path"], ".~internal/bracket.scprt", "{links}");
    assert!(links["links"][0]["found"] == true && links["links"][0]["changed"] == false, "{links}");
    run(&mut s, "file.save", json!({}));
    let full = volume(&mut s);

    // The design's file alone, somewhere else: it opens complete and unpacks its part there.
    std::fs::copy(job.join("counter.sc"), away.join("counter.sc")).unwrap();
    let mut elsewhere = Session::default();
    let reply = run(&mut elsewhere, "doc.open", json!({"path": away.join("counter.sc")}));
    assert!(reply.get("missing_links").is_none() && reply.get("recovery_components").is_none(), "{reply}");
    let unpacked = away.join(".~counter.sc.parts").join("bracket.scprt");
    assert_eq!(std::fs::read(&unpacked).unwrap(), std::fs::read(&work).unwrap());
    assert!((volume(&mut elsewhere) - full).abs() < 1e-6);
    let updated = run(&mut elsewhere, "component.update_links", json!({}));
    assert_eq!(updated["unchanged"], json!(["bracket"]), "{updated}");

    // The unpacked part is edited and saved: out of date, then taken, then in the design's file.
    part(&unpacked, 25);
    let links = run(&mut elsewhere, "component.links", json!({}));
    assert_eq!(links["links"][0]["changed"], true, "{links}");
    let updated = run(&mut elsewhere, "component.update_links", json!({}));
    assert_eq!(updated["updated"], json!(["bracket"]), "{updated}");
    assert!((volume(&mut elsewhere) - 5000.0).abs() < 1e-6, "{}", volume(&mut elsewhere));
    run(&mut elsewhere, "file.save", json!({}));
    let saved = solidcraft_io::read_design(&std::fs::read(away.join("counter.sc")).unwrap()).unwrap();
    assert_eq!(saved.internal["bracket.scprt"].as_bytes(), std::fs::read(&unpacked).unwrap(), "the save packed the edited part");

    // Opened again with the unpacked part edited but the design not saved, as a crash leaves it: a
    // recovery component. It is named and marked, and until it is settled the design shows, updates
    // from and saves what its file holds.
    part(&unpacked, 40);
    let held = saved.internal["bracket.scprt"].clone();
    let mut crashed = Session::default();
    let reply = run(&mut crashed, "doc.open", json!({"path": away.join("counter.sc")}));
    assert_eq!(reply["recovery_components"], json!(["bracket.scprt"]), "{reply}");
    let link = run(&mut crashed, "component.links", json!({}))["links"][0].clone();
    assert!(link["internal"] == true && link["recovery"] == true && link["changed"] == true, "{link}");
    assert_eq!(run(&mut crashed, "component.update_links", json!({}))["unchanged"], json!(["bracket"]));
    assert!((volume(&mut crashed) - 5000.0).abs() < 1e-6, "{}", volume(&mut crashed));
    run(&mut crashed, "file.save", json!({}));
    let resaved = solidcraft_io::read_design(&std::fs::read(away.join("counter.sc")).unwrap()).unwrap();
    assert_eq!(resaved.internal["bracket.scprt"], held, "a save takes no recovery file unasked");
    // Restored: the unpacked work stays, the component is out of date, and Update Links takes it.
    let restored = run(&mut crashed, "component.restore_recovery", json!({"component": "bracket"}));
    assert_eq!(restored["member"], "bracket.scprt", "{restored}");
    let link = run(&mut crashed, "component.links", json!({}))["links"][0].clone();
    assert!(link.get("recovery").is_none() && link["changed"] == true, "{link}");
    assert_eq!(run(&mut crashed, "component.update_links", json!({}))["updated"], json!(["bracket"]));
    assert!((volume(&mut crashed) - 8000.0).abs() < 1e-6, "{}", volume(&mut crashed));
    let refused = crashed.execute("component.restore_recovery", &json!({"component": "bracket"})).unwrap_err().to_string();
    assert!(refused.contains("not a recovery component"), "{refused}");
    run(&mut crashed, "file.save", json!({}));
    // Discarded: what the design's file holds is unpacked again, and the dropped work is kept beside it.
    let before = std::fs::read(&unpacked).unwrap();
    part(&unpacked, 55);
    let dropped = std::fs::read(&unpacked).unwrap();
    let mut again = Session::default();
    let reply = run(&mut again, "doc.open", json!({"path": away.join("counter.sc")}));
    assert_eq!(reply["recovery_components"], json!(["bracket.scprt"]), "{reply}");
    let discarded = run(&mut again, "component.discard_recovery", json!({"component": "bracket"}));
    assert_eq!(std::fs::read(&unpacked).unwrap(), before, "the design's copy is unpacked again");
    assert_eq!(std::fs::read(discarded["backup"].as_str().unwrap()).unwrap(), dropped, "the dropped work is kept");
    let link = run(&mut again, "component.links", json!({}))["links"][0].clone();
    assert!(link.get("recovery").is_none() && link["changed"] == false, "{link}");
    assert!((volume(&mut again) - 8000.0).abs() < 1e-6);

    // Written back without saving the design: the design's file holds the part's new work, nothing
    // else in it changed, and the design does not take its own write for another program's.
    part(&unpacked, 60);
    let design_before = solidcraft_io::read_design(&std::fs::read(away.join("counter.sc")).unwrap()).unwrap();
    let wrote = run(&mut again, "component.write_back", json!({"component": "bracket"}));
    assert_eq!(wrote["member"], "bracket.scprt", "{wrote}");
    let design_after = solidcraft_io::read_design(&std::fs::read(away.join("counter.sc")).unwrap()).unwrap();
    assert_eq!(design_after.internal["bracket.scprt"].as_bytes(), std::fs::read(&unpacked).unwrap());
    assert_eq!(design_after.features, design_before.features);
    assert_eq!(design_after.links, design_before.links, "the design itself is as it was saved");
    assert!(!again.disk_changed(), "its own write is not another program's change");
    // Opened again: no recovery component (the file holds the work), and the part is out of date.
    let mut reopened = Session::default();
    let reply = run(&mut reopened, "doc.open", json!({"path": away.join("counter.sc")}));
    assert!(reply.get("recovery_components").is_none(), "{reply}");
    let link = run(&mut reopened, "component.links", json!({}))["links"][0].clone();
    assert!(link["changed"] == true && link.get("recovery").is_none(), "{link}");
    assert_eq!(run(&mut reopened, "component.update_links", json!({}))["updated"], json!(["bracket"]));
    run(&mut reopened, "file.save", json!({}));
    let refused = reopened.execute("component.write_back", &json!({"component": 999})).unwrap_err().to_string();
    assert!(!refused.is_empty());

    // Replaced by another file and still internal, as one undo step: one undo brings the first back.
    let spare = away.join("spare.scprt");
    part(&spare, 30);
    let before_replace = volume(&mut reopened);
    let replaced = run(&mut reopened, "component.make_internal", json!({"component": "bracket", "path": spare}));
    assert_eq!(replaced["member"], "spare.scprt", "{replaced}");
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["path"], ".~internal/spare.scprt");
    assert!((volume(&mut reopened) - before_replace).abs() > 1.0, "the component shows the other file's bodies");
    run(&mut reopened, "edit.undo", json!({}));
    let link = run(&mut reopened, "component.links", json!({}))["links"][0].clone();
    assert!(link["path"] == ".~internal/bracket.scprt" && link["found"] == true, "{link}");
    assert!((volume(&mut reopened) - before_replace).abs() < 1e-6, "one undo takes the whole replace back");
    // A file that is not a design replaces nothing.
    std::fs::write(away.join("notes.scprt"), b"not a design").unwrap();
    assert!(reopened.execute("component.make_internal", &json!({"component": "bracket", "path": away.join("notes.scprt")})).is_err());
    assert_eq!(run(&mut reopened, "component.links", json!({}))["links"][0]["path"], ".~internal/bracket.scprt");

    // Read-only, with nothing unpacked beside it: nothing is written, and the part still reads.
    let bare = root.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::copy(away.join("counter.sc"), bare.join("counter.sc")).unwrap();
    let mut read_only = Session::default();
    let reply = run(&mut read_only, "doc.open", json!({"path": bare.join("counter.sc"), "read_only": true}));
    assert!(reply.get("missing_links").is_none(), "{reply}");
    assert!(!bare.join(".~counter.sc.parts").exists(), "a read-only open unpacks nothing");
    let updated = run(&mut read_only, "component.update_links", json!({}));
    assert_eq!(updated["unchanged"], json!(["bracket"]), "{updated}");

    // Save As: the part is unpacked beside the new file too, and the new file holds it.
    std::fs::create_dir_all(root.join("copy")).unwrap();
    run(&mut elsewhere, "file.save_as", json!({"path": root.join("copy").join("line.sc")}));
    assert!(root.join("copy").join(".~line.sc.parts").join("bracket.scprt").is_file());
    assert_eq!(run(&mut elsewhere, "component.links", json!({}))["links"][0]["found"], true);
    let _ = std::fs::remove_dir_all(root);
}

/// Missing files on open (#6892, slice 4b): under Parts and assemblies, an assembly whose part
/// file has gone opens with its last bodies, and `doc.open` names that part with the file it
/// looked for. Relinking it to where the file went brings it back, stored relative to the
/// assembly, and the next open names nothing.
#[test]
fn an_assembly_names_the_parts_it_cannot_find_on_open_and_relink_finds_them() {
    crate::set_experimental_file_kinds(true);
    let root = folder("missing-on-open");
    let (job, elsewhere) = (root.join("job"), root.join("elsewhere"));
    std::fs::create_dir_all(job.join("parts")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (bracket, plate) = (job.join("parts").join("bracket.scprt"), job.join("parts").join("plate.scprt"));
    part(&bracket, 10);
    part(&plate, 20);
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": job.join("frame.scasm")}));
    run(&mut s, "component.insert_design", json!({"path": bracket}));
    run(&mut s, "component.insert_design", json!({"path": plate, "translate": [30, 0, 0]}));
    run(&mut s, "file.save", json!({}));
    let full = volume(&mut s);
    // Both part files move out of the job folder.
    for name in ["bracket.scprt", "plate.scprt"] {
        std::fs::rename(job.join("parts").join(name), elsewhere.join(name)).unwrap();
    }
    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": job.join("frame.scasm")}));
    let missing = reply["missing_links"].as_array().cloned().unwrap_or_default();
    let mut names: Vec<&str> = missing.iter().filter_map(|m| m["name"].as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["bracket", "plate"], "{reply}");
    assert!(missing.iter().all(|m| m["path"].as_str().unwrap().replace('\\', "/").contains("job/parts/")), "{reply}");
    assert!((volume(&mut opened) - full).abs() < 1e-6, "the missing parts keep their last bodies");
    // Pointed at where the files went: each comes back, stored relative to the assembly.
    for m in &missing {
        let name = std::path::Path::new(m["path"].as_str().unwrap()).file_name().unwrap().to_owned();
        run(&mut opened, "component.relink", json!({"component": m["component"], "path": elsewhere.join(name)}));
    }
    run(&mut opened, "file.save", json!({}));
    let mut again = Session::default();
    let reply = run(&mut again, "doc.open", json!({"path": job.join("frame.scasm")}));
    assert!(reply.get("missing_links").is_none(), "{reply}");
    let links = run(&mut again, "component.links", json!({}));
    assert!(links["links"].as_array().unwrap().iter().all(|l| l["path"].as_str().unwrap().starts_with("../elsewhere/")), "{links}");
    let _ = std::fs::remove_dir_all(root);
}

/// A design's drawings as files while it is open (#6892, `design_drawings`): a save writes each
/// drawing to the work folder as `<name>.scdwg`, a drawing file by itself; a renamed drawing's
/// file follows its name; closing removes them; opening writes them again and replaces nothing;
/// a read-only open writes none. Only a top container (`.sc`, `.scasm`): never a part, never an
/// internal component's work file. The design's file holds the drawings throughout.
#[test]
fn a_designs_drawings_are_drawing_files_beside_it_while_it_is_open() {
    crate::file_kinds_on_for_tests();
    let root = folder("drawing-files");
    let design = root.join("job.sc");
    let work = root.join(".~job.sc.parts");
    let listed = |folder: &Path| {
        let mut names: Vec<String> =
            std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 4}));
    run(&mut s, "drawing.create", json!({"name": "Bracket", "standard_views": true}));
    run(&mut s, "drawing.close", json!({}));
    let flat = run(&mut s, "drawing.create", json!({"name": "Flat/pattern"}))["id"].clone();
    run(&mut s, "drawing.close", json!({}));
    assert!(!work.exists(), "a design never saved has no folder to write to");
    run(&mut s, "file.save", json!({"path": design}));
    assert_eq!(listed(&work), ["Bracket.scdwg", "Flat_pattern.scdwg"]);
    let drawing = solidcraft_io::read_drawing_file(&work.join("Bracket.scdwg").to_string_lossy()).unwrap();
    assert_eq!((drawing.name.as_str(), drawing.id), ("Bracket", s.doc.drawings[0].id), "a whole drawing file, the design's drawing");
    let held = solidcraft_io::design_drawings(&std::fs::read(&design).unwrap()).unwrap();
    assert_eq!(held.len(), 2, "and the design's file holds them, as before");

    // Renamed and saved: the file follows the name, and the old one goes.
    run(&mut s, "drawing.open", json!({"drawing": flat}));
    run(&mut s, "drawing.set", json!({"name": "Flat"}));
    run(&mut s, "drawing.close", json!({}));
    assert_eq!(listed(&work), ["Bracket.scdwg", "Flat_pattern.scdwg"], "nothing changes before a save");
    run(&mut s, "file.save", json!({}));
    assert_eq!(listed(&work), ["Bracket.scdwg", "Flat.scdwg"]);

    // Closed: the files the design's file holds go, and the folder with them.
    drop(s);
    assert_eq!(crate::tidy_work_folder(&design.to_string_lossy()), 2);
    assert!(!work.exists());

    // Read-only writes nothing; a normal open writes them again.
    let mut read_only = Session::default();
    run(&mut read_only, "doc.open", json!({"path": design, "read_only": true}));
    assert!(!work.exists(), "a read-only open writes no files");
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": design}));
    assert_eq!(listed(&work), ["Bracket.scdwg", "Flat.scdwg"]);

    // A file that differs is not replaced by an open, and closing leaves it.
    let other = std::fs::read(work.join("Bracket.scdwg")).unwrap();
    std::fs::write(work.join("Flat.scdwg"), &other).unwrap();
    let mut third = Session::default();
    run(&mut third, "doc.open", json!({"path": design}));
    assert_eq!(std::fs::read(work.join("Flat.scdwg")).unwrap(), other, "an open replaces nothing");
    assert_eq!(crate::tidy_work_folder(&design.to_string_lossy()), 1);
    assert_eq!(listed(&work), ["Flat.scdwg"], "the file that differs stays");

    // An internal component's work file keeps its drawings inside it: opened or saved there, it
    // gets no drawing files of its own, however deep it sits.
    let inside = root.join(".~counter.sc.parts");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::copy(&design, inside.join("sub.sc")).unwrap();
    let mut member = Session::default();
    run(&mut member, "doc.open", json!({"path": inside.join("sub.sc")}));
    run(&mut member, "file.save", json!({}));
    assert!(
        listed(&inside).iter().all(|name| !name.ends_with(".scdwg") && !name.starts_with(".~")),
        "one set of drawing files, the top design's: {:?}",
        listed(&inside)
    );

    // A part keeps its drawings inside its file, opened as it is or not; a design without
    // drawings has nothing to write.
    let part = root.join("plate.scprt");
    let mut loose = Session::default();
    run(&mut loose, "solid.box", json!({"length": 4, "width": 2, "height": 1}));
    run(&mut loose, "drawing.create", json!({"name": "Plate"}));
    run(&mut loose, "drawing.close", json!({}));
    run(&mut loose, "file.save", json!({"path": part}));
    run(&mut Session::default(), "doc.open", json!({"path": part}));
    assert!(!root.join(".~plate.scprt.parts").exists(), "a part gets no drawing files");
    let plain = root.join("plain.sc");
    let mut none = Session::default();
    run(&mut none, "solid.box", json!({"length": 4, "width": 2, "height": 1}));
    run(&mut none, "file.save", json!({"path": plain}));
    assert!(!root.join(".~plain.sc.parts").exists(), "no drawings, no folder");
    let _ = std::fs::remove_dir_all(root);
}

/// An internal component's file says which design it belongs to (#6892, `solidcraft_io::InternalOf`):
/// Make Internal tags the copy, the stored copy and the work file are the same tagged bytes, a save
/// of the part keeps the tag, and the owner is found from the file wherever it is. Opened by
/// itself the file reports the tag; `as_copy` opens it as a new single design with no file and no
/// tag, and saving that writes an untagged file. A drawing's file carries the tag too.
#[test]
fn an_internal_components_file_is_tagged_with_the_design_it_belongs_to() {
    crate::file_kinds_on_for_tests();
    let root = folder("tagged-files");
    part(&root.join("bracket.scprt"), 2);
    let design = root.join("counter.sc");
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": design}));
    run(&mut s, "component.insert_design", json!({"path": root.join("bracket.scprt")}));
    assert_eq!(solidcraft_io::read_internal_of_file(&root.join("bracket.scprt").to_string_lossy()), None, "a loose file has no tag");
    let made = run(&mut s, "component.make_internal", json!({"component": "bracket"}));
    run(&mut s, "drawing.create", json!({"name": "Counter"}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "file.save", json!({}));
    let work = PathBuf::from(made["file"].as_str().unwrap());
    let tag = solidcraft_io::read_internal_of_file(&work.to_string_lossy()).expect("the work file is tagged");
    assert_eq!(tag.design, "counter.sc");
    let stored = solidcraft_io::internal_components(&std::fs::read(&design).unwrap()).unwrap();
    assert_eq!(stored["bracket.scprt"], std::fs::read(&work).unwrap(), "stored and unpacked are the same bytes");
    assert_eq!(solidcraft_io::internal_of(&stored["bracket.scprt"]), Some(tag.clone()));
    assert_eq!(crate::internal_owner(&work.to_string_lossy()).map(PathBuf::from), Some(design.clone()));
    let drawing_file = work.with_file_name("Counter.scdwg");
    assert_eq!(solidcraft_io::read_internal_of_file(&drawing_file.to_string_lossy()).map(|tag| tag.design), Some("counter.sc".into()));
    assert_eq!(
        crate::internal_owner(&drawing_file.to_string_lossy()).map(PathBuf::from),
        Some(design.clone()),
        "a drawing's file finds its design too"
    );

    // Opened where it is and saved: the tag stays, so Finish Edit stores a tagged part.
    let mut member = Session::default();
    let reply = run(&mut member, "doc.open", json!({"path": work}));
    assert_eq!(reply["internal_of"], json!({"design": "counter.sc", "key": tag.key}), "{reply}");
    run(&mut member, "solid.box", json!({"length": 1, "width": 1, "height": 9, "corner": [5, 5, 0]}));
    run(&mut member, "file.save", json!({}));
    assert_eq!(solidcraft_io::read_internal_of_file(&work.to_string_lossy()), Some(tag.clone()), "a save where it is keeps the tag");

    // Taken out of the work folder to somewhere else: it still names its design, and the design
    // is found beside it or one folder up, by the key and not by the name alone.
    let loose = root.join("found-on-a-stick.scprt");
    std::fs::copy(&work, &loose).unwrap();
    assert_eq!(crate::internal_owner(&loose.to_string_lossy()).map(PathBuf::from), Some(design.clone()));
    let elsewhere = folder("tagged-files-elsewhere");
    std::fs::copy(&work, elsewhere.join("bracket.scprt")).unwrap();
    std::fs::copy(root.join("bracket.scprt"), elsewhere.join("counter.sc")).unwrap();
    assert_eq!(crate::internal_owner(&elsewhere.join("bracket.scprt").to_string_lossy()), None, "a file that only has the name is not the owner");

    // As a copy: a new, unsaved single design with no file and no tag, and the file untouched.
    let before = std::fs::read(&loose).unwrap();
    let mut copy = Session::default();
    let reply = run(&mut copy, "doc.open", json!({"path": loose, "as_copy": true}));
    assert_eq!((reply["as_copy"].as_bool(), reply.get("read_only")), (Some(true), None), "{reply}");
    assert!(reply["internal_of"]["design"] == "counter.sc", "the reply still says what the file was: {reply}");
    assert!(copy.path.is_none() && copy.internal_of.is_none() && copy.is_dirty() && copy.design_kind == solidcraft_io::DesignKind::Single);
    assert!(copy.execute("file.save", &json!({})).is_err(), "it has no file: a save asks where");
    let own = root.join("bracket-of-its-own.sc");
    run(&mut copy, "file.save", json!({"path": own}));
    assert_eq!(solidcraft_io::read_internal_of_file(&own.to_string_lossy()), None, "saved as a design of its own: no tag");
    assert_eq!(std::fs::read(&loose).unwrap(), before, "the file it came from is as it was");

    // Saved somewhere else from the tagged session: a file of its own there too.
    let moved = root.join("moved.scprt");
    run(&mut member, "file.save", json!({"path": moved}));
    assert_eq!(solidcraft_io::read_internal_of_file(&moved.to_string_lossy()), None);
    assert!(member.internal_of.is_none());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(elsewhere);
}

/// Opening and saving a design do not read work files that are as they were last read (#6892):
/// the work folder's record answers for them. A work file changed since is still found, as a
/// recovery component, and a record too fresh to trust only costs the reads.
#[test]
fn unchanged_work_files_are_not_read_on_open_or_save_and_a_changed_one_is_still_found() {
    use super::super::design_stamps::{READS, settle_all};
    crate::set_experimental_file_kinds(true);
    let root = folder("stamps-open-save");
    for (name, height) in [("bracket.scprt", 10), ("plate.scprt", 15)] {
        part(&root.join(name), height);
    }
    let design = root.join("counter.sc");
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": design}));
    for name in ["bracket", "plate"] {
        run(&mut s, "component.insert_design", json!({"path": root.join(format!("{name}.scprt"))}));
        run(&mut s, "component.make_internal", json!({"component": name}));
    }
    run(&mut s, "file.save", json!({}));
    let work = root.join(".~counter.sc.parts");
    let reads = || READS.with(|reads| reads.replace(0));

    // A record made in the same moment as the files is not trusted: both are read, and nothing differs.
    reads();
    let mut fresh = Session::default();
    let reply = run(&mut fresh, "doc.open", json!({"path": design}));
    assert!(reply.get("recovery_components").is_none(), "{reply}");
    assert_eq!(reads(), 2, "an untrusted record costs the reads");

    // Later, with the record settled: neither an open nor a save reads a work file.
    settle_all(&work);
    let mut later = Session::default();
    let reply = run(&mut later, "doc.open", json!({"path": design}));
    assert!(reply.get("recovery_components").is_none(), "{reply}");
    assert_eq!(reads(), 0, "open: answered from the record");
    let before = std::fs::read(&design).unwrap();
    run(&mut later, "file.save", json!({}));
    assert_eq!(reads(), 0, "save: answered from the record");
    let saved = solidcraft_io::read_design(&std::fs::read(&design).unwrap()).unwrap();
    assert_eq!(saved.internal, solidcraft_io::read_design(&before).unwrap().internal, "the save kept both parts");

    // One work file is saved again by itself: the next open finds it, and reads no more than that one.
    part(&work.join("plate.scprt"), 30);
    settle_all(&work);
    let mut crashed = Session::default();
    let reply = run(&mut crashed, "doc.open", json!({"path": design}));
    assert_eq!(reply["recovery_components"], json!(["plate.scprt"]), "{reply}");
    assert!(reads() <= 1, "only the changed file can need a read");
    let _ = std::fs::remove_dir_all(root);
}

/// A design's work folder deleted while the design is open loses nothing (#6892): the next save
/// still holds each part as this session last had it, whether it was just made internal or
/// written back by Finish Edit, and the next open unpacks it again.
#[test]
fn a_work_folder_deleted_while_the_design_is_open_loses_no_part() {
    crate::set_experimental_file_kinds(true);
    let root = folder("work-folder-deleted");
    part(&root.join("bracket.scprt"), 10);
    let design = root.join("counter.sc");
    let work = root.join(".~counter.sc.parts");
    let held = |file: &Path| solidcraft_io::read_design(&std::fs::read(file).unwrap()).unwrap().internal;
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": design}));
    run(&mut s, "component.insert_design", json!({"path": root.join("bracket.scprt")}));
    run(&mut s, "component.make_internal", json!({"component": "bracket"}));
    // Made internal and not saved yet: the folder goes, and the save still stores the part.
    std::fs::remove_dir_all(&work).unwrap();
    run(&mut s, "file.save", json!({}));
    let first = held(&design);
    assert_eq!(first.keys().map(String::as_str).collect::<Vec<_>>(), ["bracket.scprt"], "the part made internal is in the file");

    // Opened again, the part edited in its work file and written back as Finish Edit does; then
    // the folder is deleted and the design saved.
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": design}));
    part(&work.join("bracket.scprt"), 25);
    run(&mut again, "component.update_links", json!({}));
    run(&mut again, "component.write_back", json!({"component": "bracket"}));
    let edited = held(&design);
    assert_ne!(edited, first, "Finish Edit put the edit in the file");
    std::fs::remove_dir_all(&work).unwrap();
    run(&mut again, "file.save", json!({}));
    assert_eq!(held(&design), edited, "a save without the folder keeps the edit");

    // The same after a save took the edit: edited, saved, the folder deleted, saved again.
    let mut third = Session::default();
    run(&mut third, "doc.open", json!({"path": design}));
    part(&work.join("bracket.scprt"), 40);
    run(&mut third, "component.update_links", json!({}));
    run(&mut third, "file.save", json!({}));
    let saved = held(&design);
    assert_ne!(saved, edited, "the save packed the edit");
    std::fs::remove_dir_all(&work).unwrap();
    run(&mut third, "file.save", json!({}));
    assert_eq!(held(&design), saved, "a second save without the folder keeps it");

    // The next open unpacks the part again, with nothing to ask.
    let mut last = Session::default();
    let reply = run(&mut last, "doc.open", json!({"path": design}));
    assert!(reply.get("missing_links").is_none() && reply.get("recovery_components").is_none(), "{reply}");
    assert_eq!(std::fs::read(work.join("bracket.scprt")).unwrap(), saved["bracket.scprt"].as_bytes());
    let _ = std::fs::remove_dir_all(root);
}

/// A hinge sub-assembly file holding an arm (height 10) and a pin (height 5) as its own internal
/// components, saved at `file`.
fn hinge(dir: &Path, file: &Path) {
    part(&dir.join("arm.scprt"), 10);
    part(&dir.join("pin.scprt"), 5);
    let mut hinge = Session::default();
    run(&mut hinge, "file.new", json!({"kind": "assembly"}));
    run(&mut hinge, "file.save", json!({"path": file}));
    for name in ["arm", "pin"] {
        run(&mut hinge, "component.insert_design", json!({"path": dir.join(format!("{name}.scprt"))}));
        run(&mut hinge, "component.make_internal", json!({"component": name}));
    }
    run(&mut hinge, "file.save", json!({}));
}

/// A sub-assembly is an internal component like a part (#6892): its file is stored inside the
/// design's, with its own parts inside it, so the design travels alone. Opened from the work
/// folder, as Edit in Place does, it has its own components and unpacks its parts beside itself.
/// A change to one of its parts goes up a level at a time: into the sub-assembly's file, then
/// into the design's.
#[test]
fn a_sub_assembly_is_an_internal_component_with_its_own_parts_and_timeline() {
    crate::set_experimental_file_kinds(true);
    let root = folder("sub-assembly-inside");
    let (job, away) = (root.join("job"), root.join("away"));
    std::fs::create_dir_all(&job).unwrap();
    std::fs::create_dir_all(&away).unwrap();
    hinge(&job, &job.join("hinge.scasm"));
    let mut top = Session::default();
    run(&mut top, "file.save", json!({"path": job.join("counter.sc")}));
    run(&mut top, "component.insert_design", json!({"path": job.join("hinge.scasm")}));
    let made = run(&mut top, "component.make_internal", json!({"component": "hinge"}));
    assert_eq!(made["member"], "hinge.scasm", "{made}");
    run(&mut top, "file.save", json!({}));
    let held = |file: &Path| solidcraft_io::internal_components(&std::fs::read(file).unwrap()).unwrap();
    let inside = held(&job.join("counter.sc"));
    assert_eq!(inside.keys().map(String::as_str).collect::<Vec<_>>(), ["hinge.scasm"]);
    let nested = solidcraft_io::internal_components(&inside["hinge.scasm"]).unwrap();
    assert_eq!(nested.keys().map(String::as_str).collect::<Vec<_>>(), ["arm.scprt", "pin.scprt"], "the sub-assembly holds its own parts");

    // The design's file alone, somewhere else: complete, with the sub-assembly unpacked.
    std::fs::copy(job.join("counter.sc"), away.join("counter.sc")).unwrap();
    let mut alone = Session::default();
    let reply = run(&mut alone, "doc.open", json!({"path": away.join("counter.sc")}));
    assert!(reply.get("missing_links").is_none() && reply.get("recovery_components").is_none(), "{reply}");
    assert!((volume(&mut alone) - 3000.0).abs() < 1e-6, "{}", volume(&mut alone));
    let work = away.join(".~counter.sc.parts");
    let unpacked = work.join("hinge.scasm");
    assert!(unpacked.is_file());

    // The sub-assembly opened by itself, as Edit in Place opens it: its own two components, and
    // its parts unpacked beside it.
    let mut sub = Session::default();
    let reply = run(&mut sub, "doc.open", json!({"path": unpacked}));
    assert_eq!(reply["internal_of"]["design"], "counter.sc", "{reply}");
    let links = run(&mut sub, "component.links", json!({}));
    let names: Vec<&str> = links["links"].as_array().unwrap().iter().filter_map(|link| link["name"].as_str()).collect();
    assert_eq!(names, ["arm", "pin"], "{links}");
    assert!(links["links"].as_array().unwrap().iter().all(|link| link["internal"] == true && link["found"] == true), "{links}");
    let parts = work.join(".~hinge.scasm.parts");
    assert!(parts.join("arm.scprt").is_file() && parts.join("pin.scprt").is_file());

    // The arm is edited and saved; the sub-assembly takes it and is saved; the design takes that.
    part(&parts.join("arm.scprt"), 30);
    assert_eq!(run(&mut sub, "component.update_links", json!({}))["updated"], json!(["arm"]));
    run(&mut sub, "file.save", json!({}));
    let link = run(&mut alone, "component.links", json!({}))["links"][0].clone();
    assert_eq!(link["changed"], true, "{link}");
    assert_eq!(run(&mut alone, "component.update_links", json!({}))["updated"], json!(["hinge"]));
    assert!((volume(&mut alone) - 7000.0).abs() < 1e-6, "{}", volume(&mut alone));
    run(&mut alone, "file.save", json!({}));
    assert_eq!(held(&away.join("counter.sc"))["hinge.scasm"], std::fs::read(&unpacked).unwrap(), "the design's file holds the edited sub-assembly");

    // Closed, innermost first: nothing is left beside the design's file but the backup its own
    // save made, the sub-assembly's work folder and the backups in the work folders included.
    assert!(unpacked.with_extension("scasm.bak").is_file(), "the save left a backup beside the work file");
    drop(sub);
    crate::tidy_work_folder(&unpacked.to_string_lossy());
    drop(alone);
    crate::tidy_work_folder(&away.join("counter.sc").to_string_lossy());
    let left: Vec<String> = std::fs::read_dir(&away).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    assert!(!work.exists() && left.iter().all(|name| name.starts_with("counter.sc")), "{left:?}");
    let _ = std::fs::remove_dir_all(root);
}

/// Internal components picked from another design (#6892): `component.design_contents` lists
/// what the design holds, sub-assemblies with their own parts, and `component.insert_from_design`
/// copies the picked ones into this design as internal components of its own, placed where the
/// other design had them. A sub-assembly comes whole. One undo takes the whole insert back, and
/// the design picked from is left as it is.
#[test]
fn internal_components_are_picked_from_another_design_with_their_timelines() {
    crate::set_experimental_file_kinds(true);
    let root = folder("insert-from-design");
    let (src, job) = (root.join("src"), root.join("job"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&job).unwrap();
    hinge(&src, &src.join("hinge.scasm"));
    part(&src.join("plate.scprt"), 20);
    let source = src.join("source.sc");
    let mut made = Session::default();
    run(&mut made, "file.save", json!({"path": source}));
    run(&mut made, "component.insert_design", json!({"path": src.join("plate.scprt"), "translate": [50, 0, 0]}));
    run(&mut made, "component.insert_design", json!({"path": src.join("hinge.scasm")}));
    run(&mut made, "component.make_internal", json!({"component": "plate"}));
    run(&mut made, "component.make_internal", json!({"component": "hinge"}));
    run(&mut made, "file.save", json!({}));
    let before = std::fs::read(&source).unwrap();

    let mut s = Session::default();
    let contents = run(&mut s, "component.design_contents", json!({"path": source}));
    let members = contents["members"].as_array().unwrap();
    let shown: Vec<(&str, &str, &str, u64)> = members
        .iter()
        .map(|m| (m["pick"].as_str().unwrap(), m["name"].as_str().unwrap(), m["kind"].as_str().unwrap(), m["placements"].as_u64().unwrap()))
        .collect();
    assert_eq!(shown, [("hinge.scasm", "hinge", "assembly", 1), ("plate.scprt", "plate", "part", 1)], "{contents}");
    let nested: Vec<&str> = members[0]["members"].as_array().unwrap().iter().map(|m| m["pick"].as_str().unwrap()).collect();
    assert_eq!(nested, ["hinge.scasm/arm.scprt", "hinge.scasm/pin.scprt"], "{contents}");
    assert!(members[1].get("members").is_none(), "a part holds nothing: {contents}");

    // An unsaved design has no file to store them in; a pick the design does not hold is named.
    let picks = json!({"path": source, "members": ["plate.scprt", "hinge.scasm", "hinge.scasm/pin.scprt"]});
    let refused = s.execute("component.insert_from_design", &picks).unwrap_err().to_string();
    assert!(refused.contains("save this design first"), "{refused}");
    run(&mut s, "file.save", json!({"path": job.join("target.sc")}));
    let refused = s.execute("component.insert_from_design", &json!({"path": source, "members": ["bolt.scprt"]})).unwrap_err().to_string();
    assert!(refused.contains("holds no internal component `bolt.scprt`"), "{refused}");
    assert!(s.doc.links.is_empty(), "a refused insert leaves nothing");

    let reply = run(&mut s, "component.insert_from_design", picks);
    let inserted = reply["inserted"].as_array().unwrap();
    let brought: Vec<(&str, &str)> = inserted.iter().map(|m| (m["name"].as_str().unwrap(), m["member"].as_str().unwrap())).collect();
    assert_eq!(brought, [("plate", "plate.scprt"), ("hinge", "hinge.scasm"), ("pin", "pin.scprt")], "{reply}");
    let links = run(&mut s, "component.links", json!({}));
    assert!(links["links"].as_array().unwrap().iter().all(|link| link["internal"] == true && link["found"] == true), "{links}");
    // The plate sits where the other design had it; plate 4000, hinge 3000, pin 1000.
    let plate = inserted[0]["component"].as_u64().unwrap();
    let placed = s.doc.occurrences.iter().find(|o| o.component == plate).unwrap();
    assert_eq!(placed.transform[3], [50.0, 0.0, 0.0, 1.0]);
    assert!((volume(&mut s) - 8000.0).abs() < 1e-6, "{}", volume(&mut s));

    // Saved, the design holds all three, the sub-assembly with its parts, each tagged as its own.
    run(&mut s, "file.save", json!({}));
    let held = solidcraft_io::internal_components(&std::fs::read(job.join("target.sc")).unwrap()).unwrap();
    assert_eq!(held.keys().map(String::as_str).collect::<Vec<_>>(), ["hinge.scasm", "pin.scprt", "plate.scprt"]);
    assert_eq!(solidcraft_io::internal_components(&held["hinge.scasm"]).unwrap().len(), 2, "the sub-assembly came whole");
    assert!(held.values().all(|part| solidcraft_io::internal_of(part).is_some_and(|tag| tag.design == "target.sc")));
    assert_eq!(std::fs::read(&source).unwrap(), before, "the design picked from is untouched");

    // One undo takes the whole insert back.
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.links.is_empty(), "one undo step");
    let _ = std::fs::remove_dir_all(root);
}

/// A part edited in place and not yet saved is the app's autosave to bring back (#6892). The
/// recovered copy is still a part and still belongs to its design, so a save puts it back in
/// its place in the work folder, where the design offers it as a recovery component. The lock
/// the crashed edit left in the work folder goes when the design closes, and the folder with it.
#[test]
fn an_unsaved_internal_part_recovered_from_the_autosave_saves_back_to_its_design() {
    crate::set_experimental_file_kinds(true);
    let root = folder("autosave-internal-part");
    part(&root.join("bracket.scprt"), 10);
    let design = root.join("counter.sc");
    let work = root.join(".~counter.sc.parts");
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": design}));
    run(&mut s, "component.insert_design", json!({"path": root.join("bracket.scprt")}));
    run(&mut s, "component.make_internal", json!({"component": "bracket"}));
    run(&mut s, "file.save", json!({}));

    // The part as Edit in Place has it open, changed and not saved; then the app is gone.
    let file = work.join("bracket.scprt");
    let mut edit = Session::default();
    run(&mut edit, "doc.open", json!({"path": file}));
    run(&mut edit, "solid.box", json!({"length": 3, "width": 3, "height": 3, "corner": [20, 0, 0]}));
    let recovery = root.join("recovery");
    let mut saver = crate::recovery::Autosaver::new(&recovery, std::time::Duration::from_secs(3600)).unwrap();
    assert!(saver.save(&edit).unwrap(), "the unsaved part is autosaved");
    drop(saver);
    let gone = solidcraft_io::design_lock::Lock { pid: u32::MAX - 7, opened: 1, refreshed: 1, ..solidcraft_io::design_lock::Lock::mine(1) };
    solidcraft_io::design_lock::write(&file, &gone).unwrap();
    let lock = solidcraft_io::design_lock::lock_path(&file);
    assert!(lock.is_file());

    // Recovered: a part, of this design, and it saves to where it was.
    let found = crate::recovery::orphans(&recovery);
    assert_eq!(found.len(), 1, "{found:?}");
    let mut back = Session::default();
    run(&mut back, "doc.recover", json!({"id": found[0].id, "dir": recovery}));
    assert_eq!(back.design_kind, solidcraft_io::DesignKind::Part, "a recovered part is still a part");
    assert_eq!(back.internal_of.as_ref().map(|tag| tag.design.as_str()), Some("counter.sc"), "and still belongs to its design");
    run(&mut back, "file.save", json!({}));
    assert_eq!(solidcraft_io::read_internal_of_file(&file.to_string_lossy()).map(|tag| tag.design), Some("counter.sc".to_string()));

    // The design finds the part's newer work and takes it.
    let mut again = Session::default();
    let reply = run(&mut again, "doc.open", json!({"path": design}));
    assert_eq!(reply["recovery_components"], json!(["bracket.scprt"]), "{reply}");
    run(&mut again, "component.restore_recovery", json!({"component": "bracket"}));
    run(&mut again, "file.save", json!({}));
    crate::tidy_work_folder(&design.to_string_lossy());
    assert!(!lock.exists(), "the lock nobody holds is gone");
    assert!(
        !work.exists(),
        "and the work folder with it: {:?}",
        std::fs::read_dir(&work).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
    let _ = std::fs::remove_dir_all(root);
}

/// An assembly of its own 10 x 10 x 10 box and the linked design `part` (a 10 x 20 box), saved
/// as `assembly` in `dir` without the bodies of linked designs.
fn saved_without_linked_bodies(dir: &Path, part: &Path, assembly: &str) -> (Session, PathBuf) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100,0,0]}));
    run(&mut s, "component.insert_design", json!({"path": part}));
    let file = dir.join(assembly);
    let saved = run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert!(saved.get("warnings").is_none(), "leaving the linked bodies out is the request, not a warning: {saved}");
    (s, file)
}

/// The STEP text or records each link's Import feature holds in the design file at `file`.
fn linked_payloads(file: &Path) -> Vec<bool> {
    let doc = solidcraft_io::read_design(&std::fs::read(file).unwrap()).unwrap();
    assert!(!doc.links.is_empty());
    doc.links
        .values()
        .map(|link| match &doc.feature(link.feature).unwrap().kind {
            solidcraft_doc::FeatureKind::Import { step, records, .. } => !step.is_empty() || records.is_some(),
            _ => panic!("a link's feature is an Import"),
        })
        .collect()
}

/// `file.save`'s `linked_bodies: false` (#8170): the file keeps each link without its bodies,
/// opening it reads them from the linked design again, and the design keeps the choice.
#[test]
fn a_design_saved_without_linked_bodies_reads_them_from_the_linked_files_when_it_opens() {
    let dir = folder("left-out");
    for (part_name, assembly) in [("part.json", "assembly.json"), ("part.sc", "assembly.sc")] {
        let source = dir.join(part_name);
        part(&source, 30);
        let mut whole = Session::default();
        run(&mut whole, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100,0,0]}));
        run(&mut whole, "component.insert_design", json!({"path": source}));
        let whole_file = dir.join(format!("whole-{assembly}"));
        run(&mut whole, "file.save", json!({"path": whole_file}));
        assert_eq!(linked_payloads(&whole_file), [true], "a save keeps the linked bodies unless asked not to");

        let (mut s, file) = saved_without_linked_bodies(&dir, &source, assembly);
        assert_eq!(linked_payloads(&file), [false]);
        let lean = std::fs::read(&file).unwrap();
        assert!(lean.len() < std::fs::read(&whole_file).unwrap().len());
        // The session keeps its bodies: 10 x 10 x 10 and 10 x 20 x 30.
        assert!((volume(&mut s) - 7000.0).abs() < 1e-5);
        assert!(!s.is_dirty());

        let mut opened = Session::default();
        let reply = run(&mut opened, "doc.open", json!({"path": file}));
        assert_eq!(reply["errors"], 0);
        assert!(reply.get("warnings").is_none(), "{reply}");
        assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);
        assert!(!opened.is_dirty(), "reading the bodies back is no change to the design");
        assert!(!opened.linked_bodies, "the design keeps the choice its file was saved with");
        assert!(!opened.doc.linked_bodies_left_out, "only the file carries the mark");
        // A later save needs no option, and a plain JSON file is the same bytes again.
        run(&mut opened, "file.save", json!({}));
        assert_eq!(linked_payloads(&file), [false]);
        if assembly.ends_with(".json") {
            assert_eq!(std::fs::read(&file).unwrap(), lean);
        }
        // Save As keeps the choice too; `linked_bodies: true` puts the bodies back in the file.
        let copy = dir.join(format!("copy-{assembly}"));
        run(&mut opened, "file.save_as", json!({"path": copy}));
        assert_eq!(linked_payloads(&copy), [false]);
        run(&mut opened, "file.save", json!({"linked_bodies": true}));
        assert_eq!(linked_payloads(&copy), [true]);
        assert!(opened.linked_bodies);
        let again = Session::opened_at(solidcraft_io::read_design(&std::fs::read(&copy).unwrap()).unwrap(), None, copy.to_str());
        assert!(again.linked_bodies);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A linked design that changed, or is gone, when a design saved without its bodies opens.
#[test]
fn a_design_saved_without_linked_bodies_says_which_linked_files_changed_or_are_missing() {
    let dir = folder("left-out-sources");
    let source = dir.join("part.json");
    part(&source, 30);
    let (_, file) = saved_without_linked_bodies(&dir, &source, "assembly.json");

    // Changed while the design was closed: the bodies are the file's new ones.
    part(&source, 45);
    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": file}));
    assert_eq!(reply["errors"], 0);
    let warnings = reply["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("changed since this design was saved")), "{reply}");
    assert!((volume(&mut opened) - 10_000.0).abs() < 1e-5);
    let listed = run(&mut opened, "component.links", json!({}));
    assert_eq!(listed["links"][0]["changed"], true, "Update Links still has the changed file to take in: {listed}");
    // Until then the link is not what its file gave it: a save keeps its bodies and says so.
    let saved = run(&mut opened, "file.save", json!({}));
    assert!(saved["warnings"][0].as_str().unwrap().contains("changed since this design read it"), "{saved}");
    assert_eq!(linked_payloads(&file), [true]);
    run(&mut opened, "component.update_links", json!({}));
    assert_eq!(run(&mut opened, "component.links", json!({}))["links"][0]["changed"], false);
    assert!(run(&mut opened, "file.save", json!({})).get("warnings").is_none());
    assert_eq!(linked_payloads(&file), [false]);

    // Gone: the design opens, the link's feature says why it has no bodies, and a save keeps
    // the link as it is.
    std::fs::remove_file(&source).unwrap();
    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": file}));
    assert_eq!(reply["errors"], 1);
    assert!(reply["warnings"][0].as_str().unwrap().contains("has no bodies"), "{reply}");
    let error = opened.model.results.iter().find_map(|r| r.error.clone()).unwrap();
    assert!(error.contains("saved without them") && error.contains("Update Links reads them"), "{error}");
    assert!((volume(&mut opened) - 1000.0).abs() < 1e-5);
    run(&mut opened, "file.save", json!({}));
    assert_eq!(linked_payloads(&file), [false]);
    // The file is back: Update Links reads it.
    part(&source, 30);
    run(&mut opened, "component.update_links", json!({}));
    assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

/// `doc.open`'s `linked_bodies: "folder"`: a link to a file outside the design's own folder is
/// not read as the design opens. It has no bodies, is listed as not read, and Update Links
/// reads it; a link inside the folder is read as ever.
#[test]
fn a_design_saved_without_linked_bodies_can_open_reading_only_its_own_folder() {
    let dir = folder("left-out-folder");
    let (inside, outside) = (dir.join("project").join("parts"), dir.join("elsewhere"));
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let (near, far) = (inside.join("near.json"), outside.join("far.json"));
    part(&near, 30);
    part(&far, 45);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": near}));
    let far_component = run(&mut s, "component.insert_design", json!({"path": far, "translate": [50,0,0]}))["component"].as_u64().unwrap();
    let file = dir.join("project").join("assembly.json");
    run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert_eq!(linked_payloads(&file), [false, false]);

    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": file, "linked_bodies": "folder"}));
    assert_eq!(reply["errors"], 1);
    assert!(reply["warnings"][0].as_str().unwrap().contains("in another folder"), "{reply}");
    // Only the 10 x 20 x 30 part beside the design.
    assert!((volume(&mut opened) - 6000.0).abs() < 1e-5);
    let listed = run(&mut opened, "component.links", json!({}));
    let changed: Vec<bool> = listed["links"].as_array().unwrap().iter().map(|l| l["changed"] == true).collect();
    assert_eq!(changed, [false, true], "the link not read is the one Update Links has to read: {listed}");
    run(&mut opened, "component.update_links", json!({"component": far_component}));
    assert!((volume(&mut opened) - 15_000.0).abs() < 1e-5);

    // Without the option every link is read.
    let mut all = Session::default();
    assert_eq!(run(&mut all, "doc.open", json!({"path": file}))["errors"], 0);
    assert!((volume(&mut all) - 15_000.0).abs() < 1e-5);
    assert!(all.execute("doc.open", &json!({"path": file, "linked_bodies": "some"})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// A plain JSON part keeps no model of its own, so the first open of a design saved without
/// linked bodies evaluates it and keeps that evaluation in the local store of reused imports;
/// the next open takes it from there, until the part's file changes.
#[test]
fn a_design_saved_without_linked_bodies_evaluates_an_unchanged_part_once() {
    let dir = folder("left-out-reuse");
    let source = dir.join("part.json");
    part(&source, 30);
    let (_, file) = saved_without_linked_bodies(&dir, &source, "assembly.json");
    let reused = || super::REUSED.with(|n| n.replace(0));
    reused();
    let mut first = Session::default();
    run(&mut first, "doc.open", json!({"path": file}));
    assert_eq!(reused(), 0, "nothing is kept of a part before it is first read");
    let mut second = Session::default();
    assert_eq!(run(&mut second, "doc.open", json!({"path": file}))["errors"], 0);
    assert_eq!(reused(), 1);
    assert!((volume(&mut second) - 7000.0).abs() < 1e-5);
    // Other bytes are another design: evaluated, and then kept in their turn.
    part(&source, 45);
    let mut changed = Session::default();
    run(&mut changed, "doc.open", json!({"path": file}));
    assert_eq!(reused(), 0);
    assert!((volume(&mut changed) - 10_000.0).abs() < 1e-5);
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": file}));
    assert_eq!(reused(), 1);
    assert!((volume(&mut again) - 10_000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

/// Bodies that may be the only copy are never left out: a link whose file cannot be read when
/// the design is saved keeps them, and the save says so.
#[test]
fn a_save_without_linked_bodies_keeps_the_bodies_of_a_link_whose_file_is_gone() {
    let dir = folder("left-out-unreachable");
    let source = dir.join("part.json");
    part(&source, 30);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": source}));
    std::fs::remove_file(&source).unwrap();
    let file = dir.join("assembly.json");
    let saved = run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert!(saved["warnings"][0].as_str().unwrap().contains("keeps its bodies in the file"), "{saved}");
    assert_eq!(linked_payloads(&file), [true]);
    let mut opened = Session::default();
    assert_eq!(run(&mut opened, "doc.open", json!({"path": file}))["errors"], 0);
    assert!((volume(&mut opened) - 6000.0).abs() < 1e-5);
    assert!(!opened.linked_bodies);
    let _ = std::fs::remove_dir_all(dir);
}

/// Two designs saved without linked bodies that link each other: opening one ends, with the
/// link that returns to it reported, instead of reading the pair for ever.
#[test]
fn designs_saved_without_linked_bodies_that_link_each_other_open_with_the_loop_reported() {
    let dir = folder("left-out-loop");
    let (a, b) = (dir.join("a.json"), dir.join("b.json"));
    part(&a, 30);
    let mut second = Session::default();
    run(&mut second, "component.insert_design", json!({"path": a}));
    run(&mut second, "file.save", json!({"path": b, "linked_bodies": false}));
    let mut first = Session::default();
    run(&mut first, "doc.open", json!({"path": a}));
    run(&mut first, "component.insert_design", json!({"path": b}));
    run(&mut first, "file.save", json!({"linked_bodies": false}));
    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": a}));
    assert!(reply["warnings"].as_array().is_some_and(|w| !w.is_empty()), "{reply}");
    // Its own 10 x 20 x 30 box is there whatever became of the link.
    assert!(volume(&mut opened) >= 6000.0 - 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

/// A link made before links recorded which file they read cannot tell whether its file is still
/// that one, so a save without linked bodies keeps its bodies until Update Links makes the record.
#[test]
fn a_save_without_linked_bodies_keeps_the_bodies_of_a_link_with_no_record_of_its_file() {
    let dir = folder("left-out-no-record");
    let source = dir.join("part.json");
    part(&source, 30);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": source}));
    s.doc_mut().links.values_mut().for_each(|l| l.file_sha256 = None);
    let file = dir.join("assembly.json");
    let saved = run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert!(saved["warnings"][0].as_str().is_some_and(|w| w.contains("no record")), "{saved}");
    assert_eq!(linked_payloads(&file), [true]);
    run(&mut s, "component.update_links", json!({}));
    let saved = run(&mut s, "file.save", json!({}));
    assert!(saved.get("warnings").is_none(), "{saved}");
    assert_eq!(linked_payloads(&file), [false]);
    let _ = std::fs::remove_dir_all(dir);
}

/// Two linked designs, both saved without linked bodies, that link the same part: opening the
/// design above them reads that part once, not once for each way to it.
#[test]
fn a_part_that_two_linked_designs_saved_without_bodies_share_is_read_once() {
    let dir = folder("left-out-shared");
    let source = dir.join("part.json");
    part(&source, 30);
    let (_, left) = saved_without_linked_bodies(&dir, &source, "left.json");
    let (_, right) = saved_without_linked_bodies(&dir, &source, "right.json");
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": left}));
    run(&mut s, "component.insert_design", json!({"path": right, "translate": [500, 0, 0]}));
    let file = dir.join("top.json");
    run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert_eq!(linked_payloads(&file), [false, false]);

    let read = || super::EVALUATED.with(|n| n.replace(0));
    read();
    let mut opened = Session::default();
    assert_eq!(run(&mut opened, "doc.open", json!({"path": file}))["errors"], 0);
    assert_eq!(read(), 3, "left, right and the part they share");
    // Each of the two: its own 10 x 10 x 10 box and the 10 x 20 x 30 part.
    assert!((volume(&mut opened) - 14_000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

/// `linked_bodies: folder` holds for the designs a linked design reads in its turn: one saved
/// without the bodies of a link outside the design's folder and its own is not read in part.
#[test]
fn a_linked_design_saved_without_bodies_reads_no_further_than_the_folders_allowed() {
    let dir = folder("left-out-nested-folder");
    let (project, elsewhere) = (dir.join("project"), dir.join("elsewhere"));
    std::fs::create_dir_all(project.join("parts")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (near, far) = (project.join("parts").join("near.json"), elsewhere.join("far.json"));
    part(&near, 30);
    part(&far, 30);
    let top = |middle: &Path, name: &str| {
        let mut s = Session::default();
        let component = run(&mut s, "component.insert_design", json!({"path": middle}))["component"].as_u64().unwrap();
        let file = project.join(name);
        run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
        (file, component)
    };

    // The linked design reaches out of the project folder.
    let (_, reaching) = saved_without_linked_bodies(&project, &far, "reaching.json");
    let (file, component) = top(&reaching, "top-reaching.json");
    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": file, "linked_bodies": "folder"}));
    assert_eq!(reply["errors"], 1, "{reply}");
    assert!(reply["warnings"][0].as_str().is_some_and(|w| w.contains("another folder")), "{reply}");
    assert!(volume(&mut opened).abs() < 1e-5, "none of the linked design comes in, not the part of it in the folder");
    let held = opened.execute("component.update_links", &json!({"components": [component], "linked_bodies": "folder"})).unwrap_err().to_string();
    assert!(held.contains("another folder"), "{held}");
    assert!(volume(&mut opened).abs() < 1e-5);
    // Update Links as the user runs it reads every file.
    run(&mut opened, "component.update_links", json!({}));
    assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);
    let mut all = Session::default();
    assert_eq!(run(&mut all, "doc.open", json!({"path": file}))["errors"], 0);
    assert!((volume(&mut all) - 7000.0).abs() < 1e-5);

    // The linked design stays inside the project folder.
    let (_, staying) = saved_without_linked_bodies(&project, &near, "staying.json");
    let (file, _) = top(&staying, "top-staying.json");
    let mut opened = Session::default();
    assert_eq!(run(&mut opened, "doc.open", json!({"path": file, "linked_bodies": "folder"}))["errors"], 0);
    assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);

    // A linked design in another folder, read because the caller chose it, reads its own folder.
    let (_, away) = saved_without_linked_bodies(&elsewhere, &far, "away.json");
    let (file, component) = top(&away, "top-away.json");
    let mut opened = Session::default();
    assert_eq!(run(&mut opened, "doc.open", json!({"path": file, "linked_bodies": "folder"}))["errors"], 1);
    let read = run(&mut opened, "component.update_links", json!({"components": [component], "linked_bodies": "folder"}));
    assert_eq!(read["failed"].as_array().map_or(0, Vec::len), 0, "{read}");
    assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);
    assert!(opened.execute("component.update_links", &json!({"linked_bodies": "some"})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// Eight designs saved without linked bodies, each linking the next twice and the last linking
/// the first: there are 128 ways round the loop, and opening one ends after a bounded number of
/// reads instead of following each.
#[test]
fn designs_linked_in_a_loop_by_two_links_each_open_after_a_bounded_number_of_reads() {
    let dir = folder("left-out-wide-loop");
    let file = |n: usize| dir.join(format!("d{n}.json"));
    part(&file(1), 30);
    let twice = |s: &mut Session, next: &Path| {
        run(s, "component.insert_design", json!({"path": next}));
        run(s, "component.insert_design", json!({"path": next, "translate": [500, 0, 0]}));
    };
    for n in (2..=8).rev() {
        let mut s = Session::default();
        twice(&mut s, &file(if n == 8 { 1 } else { n + 1 }));
        run(&mut s, "file.save", json!({"path": file(n), "linked_bodies": false}));
    }
    let mut first = Session::default();
    run(&mut first, "doc.open", json!({"path": file(1)}));
    twice(&mut first, &file(2));
    run(&mut first, "file.save", json!({"linked_bodies": false}));
    assert_eq!(linked_payloads(&file(1)), [false, false]);

    let read = || super::EVALUATED.with(|n| n.replace(0));
    read();
    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": file(1)}));
    assert!(reply["warnings"].as_array().is_some_and(|w| !w.is_empty()), "{reply}");
    let reads = read();
    assert!(reads <= super::READ_AGAIN + 8, "{reads} reads");
    // Its own 10 x 20 x 30 box is there whatever became of the links.
    assert!(volume(&mut opened) >= 6000.0 - 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

/// Linked designs that reach out of the folders allowed are each refused for that alone: however
/// many there are, a linked design that stays inside is still read.
#[test]
fn many_linked_designs_reaching_out_of_the_folder_hold_up_no_design_inside_it() {
    let dir = folder("left-out-many-outside");
    let (project, elsewhere) = (dir.join("project"), dir.join("elsewhere"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (near, far) = (project.join("near.json"), elsewhere.join("far.json"));
    part(&near, 30);
    part(&far, 30);
    let (_, reaching) = saved_without_linked_bodies(&project, &far, "reaching-0.json");
    let (_, staying) = saved_without_linked_bodies(&project, &near, "staying.json");
    let count = super::READ_AGAIN + 2;
    let mut s = Session::default();
    for n in 0..count {
        // The same design under another name beside it: its link to the part outside is kept.
        let copy = project.join(format!("reaching-{n}.json"));
        if n > 0 {
            std::fs::copy(&reaching, &copy).unwrap();
        }
        run(&mut s, "component.insert_design", json!({"path": copy}));
    }
    run(&mut s, "component.insert_design", json!({"path": staying, "translate": [500, 0, 0]}));
    let file = project.join("top.json");
    run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));

    let mut opened = Session::default();
    let reply = run(&mut opened, "doc.open", json!({"path": file, "linked_bodies": "folder"}));
    assert_eq!(reply["errors"], count, "{reply}");
    assert!(reply["warnings"].as_array().unwrap().iter().all(|w| w.as_str().is_some_and(|w| w.contains("another folder"))), "{reply}");
    // The design that stays inside: its own 10 x 10 x 10 box and the 10 x 20 x 30 part.
    assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

/// Recovering a design follows the rule its open followed: a link in another folder that was not
/// read before the copy was made is read by `doc.recover` only when the caller reads every file.
#[test]
fn recovering_a_design_saved_without_linked_bodies_reads_only_the_folders_allowed() {
    let dir = folder("left-out-recover");
    let (project, elsewhere, recovery) = (dir.join("project"), dir.join("elsewhere"), dir.join("recovery"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let far = elsewhere.join("far.json");
    part(&far, 30);
    let (_, file) = saved_without_linked_bodies(&project, &far, "assembly.json");
    // Opened as the app opens it: the link outside the folder waits, and the copy is made so.
    let mut s = Session::default();
    assert_eq!(run(&mut s, "doc.open", json!({"path": file, "linked_bodies": "folder"}))["errors"], 1);
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]}));
    let mut autosaver = crate::recovery::Autosaver::new(&recovery, std::time::Duration::ZERO).unwrap();
    assert!(autosaver.save(&s).unwrap());
    drop(autosaver);
    let entry = crate::recovery::orphans(&recovery).pop().unwrap();

    let mut held = Session::default();
    let reply = run(&mut held, "doc.recover", json!({"dir": recovery, "id": entry.id, "linked_bodies": "folder"}));
    assert_eq!(reply["errors"], 1, "{reply}");
    assert!(reply["warnings"][0].as_str().is_some_and(|w| w.contains("another folder")), "{reply}");
    // Its own boxes, 10 x 10 x 10 and 5 x 5 x 5, without the 10 x 20 x 30 part.
    assert!((volume(&mut held) - 1125.0).abs() < 1e-5);
    run(&mut held, "component.update_links", json!({}));
    assert!((volume(&mut held) - 7125.0).abs() < 1e-5);

    let mut all = Session::default();
    let reply = run(&mut all, "doc.recover", json!({"dir": recovery, "id": entry.id}));
    assert_eq!(reply["errors"], 0, "{reply}");
    assert!((volume(&mut all) - 7125.0).abs() < 1e-5);
    assert!(all.execute("doc.recover", &json!({"dir": recovery, "id": entry.id, "linked_bodies": "some"})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// A save without linked bodies leaves out the bodies of linked designs only: a linked STEP file
/// (Insert STEP) keeps its bodies in the design's file, and the save has nothing to say of it.
#[test]
fn a_save_without_linked_bodies_keeps_the_bodies_of_a_linked_step_file() {
    let dir = folder("left-out-step");
    let source = dir.join("part.json");
    part(&source, 30);
    let step = dir.join("block.step");
    let mut block = Session::default();
    run(&mut block, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100,0,0]}));
    run(&mut block, "file.export", json!({"path": step}));

    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": source}));
    run(&mut s, "file.insert_step", json!({"path": step}));
    assert_eq!(s.doc.links.len(), 2, "the design and the STEP file are both linked");
    let file = dir.join("assembly.json");
    let saved = run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert!(saved.get("warnings").is_none(), "{saved}");
    let doc = solidcraft_io::read_design(&std::fs::read(&file).unwrap()).unwrap();
    let kept: Vec<(bool, bool)> = doc
        .links
        .values()
        .map(|link| match &doc.feature(link.feature).unwrap().kind {
            solidcraft_doc::FeatureKind::Import { step, records, .. } => (link.step_names.is_some(), !step.is_empty() || records.is_some()),
            _ => panic!("a link's feature is an Import"),
        })
        .collect();
    assert_eq!(kept, [(false, false), (true, true)], "only the linked design's bodies are left out");

    let mut opened = Session::default();
    assert_eq!(run(&mut opened, "doc.open", json!({"path": file}))["errors"], 0);
    // The 10 x 20 x 30 part and the 10 x 10 x 10 block.
    assert!((volume(&mut opened) - 7000.0).abs() < 1e-5);
    let listed = run(&mut opened, "component.links", json!({}));
    assert!(listed["links"].as_array().unwrap().iter().all(|l| l["changed"] != true), "{listed}");
    let _ = std::fs::remove_dir_all(dir);
}
