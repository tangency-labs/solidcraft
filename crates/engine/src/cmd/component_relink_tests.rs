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

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
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
    use solidcraft_doc::appearance::{FaceLook, Look};
    use solidcraft_geom::Vec3;
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
    use solidcraft_doc::appearance::{FaceLook, Look};
    use solidcraft_geom::Vec3;
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
        point: solidcraft_geom::Vec3::new(5.0, 5.0, 10.0),
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
        point: solidcraft_geom::Vec3::new(5.0, 5.0, 10.0),
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
        solidcraft_doc::joints::Snap::FaceCenter { pick: solidcraft_geom::Vec3::ZERO, face: Some("no-such-linked-face".to_string()), x: None };
    s.refresh();
    let result = run(&mut s, "component.relink", json!({"component": component, "path": replacement}));
    assert!(result["joint_errors"].as_array().unwrap().iter().any(|e| e["name"] == "Seat"));
    assert!(result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("joint Seat:"))));
    let _ = std::fs::remove_dir_all(dir);
}
