use crate::Session;
use serde_json::{Value, json};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap()
}
fn close(actual: &Value, expected: f64) {
    assert!((actual.as_f64().unwrap() - expected).abs() < 1e-6, "{actual} != {expected}");
}
fn box_in(s: &mut Session, name: &str) -> (u64, u64, String) {
    let c = run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let body = s.model.state().bodies.last().unwrap().name.clone();
    (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap(), body)
}

#[test]
fn assembly_counts_instances_and_resolves_original_materials() {
    let mut s = Session::default();
    let (component, occurrence, body) = box_in(&mut s, "Block");
    run(&mut s, "material.assign", json!({"bodies": [body], "material": "Steel"}));
    let copy = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20, 0, 0]}));
    let rho = s.doc.density(&body);
    let root = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(root["body_count"], 2);
    close(&root["total"]["mass_g"], 2.0 * rho);
    close(&root["total"]["volume_mm3"], 2000.0);
    close(&root["total"]["center_of_mass"][0], 15.0);
    let first = run(&mut s, "component.properties", json!({"component": component}));
    assert_eq!(first["body_count"], 1);
    close(&first["total"]["center_of_mass"][0], 5.0);
    let second = run(&mut s, "occurrence.properties", json!({"occurrence": copy["occurrence"]}));
    close(&second["total"]["center_of_mass"][0], 25.0);
    assert_eq!(second["bodies"][0]["material"], "Steel");
    assert_eq!(first["path"], json!([occurrence]));
}

#[test]
fn mass_centroid_is_density_weighted_and_pending_moves_are_read_only() {
    let mut s = Session::default();
    let (a, oa, steel) = box_in(&mut s, "Steel block");
    run(&mut s, "material.assign", json!({"bodies": [steel], "material": "Steel"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let (_, ob, plastic) = box_in(&mut s, "Plastic block");
    run(&mut s, "material.assign", json!({"bodies": [plastic], "material": "ABS Plastic"}));
    run(&mut s, "occurrence.move", json!({"occurrence": ob, "translate": [30, 0, 0], "capture": false}));
    let doc = s.doc.clone();
    let moves = s.pending_moves.clone();
    let r = run(&mut s, "component.properties", json!({"component": 0}));
    let ra = s.doc.density(&steel);
    let rb = s.doc.density(&plastic);
    close(&r["total"]["center_of_mass"][0], (5.0 * ra + 35.0 * rb) / (ra + rb));
    close(&r["total"]["bbox"]["max"][0], 40.0);
    assert!(std::sync::Arc::ptr_eq(&s.doc, &doc));
    assert_eq!(s.pending_moves, moves);
    run(&mut s, "occurrence.move", json!({"occurrence": oa, "translate": [10, 0, 0], "axis": [0, 0, 1], "angle": "90 deg"}));
    let r = run(&mut s, "component.properties", json!({"component": a}));
    close(&r["total"]["center_of_mass"][0], 5.0);
    close(&r["total"]["center_of_mass"][1], 5.0);
}

#[test]
fn nested_instances_require_a_path_and_can_exclude_descendants() {
    let mut s = Session::default();
    let parent = run(&mut s, "component.create", json!({"name": "Assembly"}));
    let (_, child_occurrence, _) = box_in(&mut s, "Child");
    let copy = run(&mut s, "occurrence.copy", json!({"component": parent["component"], "translate": [100, 0, 0]}));
    assert!(s.execute("occurrence.properties", &json!({"occurrence": child_occurrence})).is_err());
    let r = run(&mut s, "occurrence.properties", json!({"occurrence": child_occurrence, "path": [copy["occurrence"], child_occurrence]}));
    close(&r["total"]["center_of_mass"][0], 105.0);
    let r = run(&mut s, "component.properties", json!({"component": parent["component"]}));
    assert_eq!(r["body_count"], 1);
    let r = run(&mut s, "component.properties", json!({"component": parent["component"], "descendants": false}));
    assert_eq!(r["body_count"], 0);
    assert!(r["total"]["center_of_mass"].is_null());
    assert!(r["total"]["bbox"].is_null());
}

#[test]
fn body_reassignment_offsets_preserve_world_properties() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let body = s.model.state().bodies.first().unwrap().name.clone();
    let c = run(&mut s, "component.create", json!({"name": "Moved", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": c["occurrence"], "translate": [100, 0, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": [body], "component": c["component"]}));
    let r = run(&mut s, "component.properties", json!({"component": c["component"]}));
    close(&r["total"]["center_of_mass"][0], 5.0);
}

#[test]
fn properties_reject_hostile_identifiers_and_options() {
    let mut s = Session::default();
    let (_, occurrence, _) = box_in(&mut s, "Block");
    for p in [json!({"component": []}), json!({"component": "missing"}), json!({"descendants": 1})] {
        assert!(s.execute("component.properties", &p).is_err());
    }
    for path in [json!([]), json!(["bad"]), json!([0]), json!(vec![occurrence; 65])] {
        assert!(s.execute("occurrence.properties", &json!({"occurrence": occurrence, "path": path})).is_err());
    }
}

#[test]
fn compact_exponentially_instanced_assembly_refuses_partial_totals() {
    let mut s = Session::default();
    let mut last_occurrence = 0;
    // Only 17 component definitions and 34 occurrences, but over 100000 placements.
    for _ in 0..17 {
        let parent = s.active_component;
        let component = s.doc_mut().add_component(None, parent).unwrap();
        last_occurrence = s.doc_mut().add_occurrence(component, parent, solidcraft_doc::IDENTITY).unwrap();
        s.active_component = component;
    }
    for command in ["component.properties", "occurrence.properties"] {
        let error = s.execute(command, &json!({"component": 0, "occurrence": last_occurrence})).unwrap_err();
        assert!(error.to_string().contains("traversal limits"), "{error}");
    }
}

#[test]
fn excessively_nested_assembly_refuses_partial_totals() {
    let mut s = Session::default();
    for _ in 0..65 {
        let parent = s.active_component;
        s.active_component = s.doc_mut().add_component(None, parent).unwrap();
    }
    let error = s.execute("component.properties", &json!({"component": 0})).unwrap_err();
    assert!(error.to_string().contains("traversal limits"), "{error}");
}

/// #532: a product BOM needs materials outside the built-in library (walnut, polypropylene,
/// rubber, leather). A user-defined material with its density is assigned like a built-in one
/// and sets the body's mass: a 10 mm walnut cube at 0.65 g/cm³ weighs 0.65 g.
#[test]
fn user_defined_material_sets_the_body_mass() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Slat"}));
    run(&mut s, "material.create", json!({"name": "Walnut", "density_g_cm3": 0.65}));
    run(&mut s, "material.assign", json!({"bodies": ["Slat"], "material": "Walnut"}));
    let props = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(props["bodies"][0]["material"], "Walnut", "{props}");
    close(&props["bodies"][0]["density_g_cm3"], 0.65);
    close(&props["total"]["mass_g"], 0.65);
}

/// A design file path in a fresh temporary folder (removed by the caller).
fn temp_design(tag: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-user-material-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("design.solidcraft").to_string_lossy().to_string();
    (dir, path)
}

fn mass(s: &mut Session) -> Value {
    run(s, "component.properties", json!({"component": "root"}))["total"]["mass_g"].clone()
}

/// #532: a design's own material is listed after the library, gives its bodies its look, and
/// can be edited (density and name: the bodies follow) and deleted; each step is undoable.
#[test]
fn user_defined_material_is_edited_renamed_and_deleted() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Pad"}));
    assert!(s.execute("material.edit", &json!({"material": "Rubber", "density_g_cm3": 1.2})).is_err(), "nothing to edit yet");
    let created = run(&mut s, "material.create", json!({"name": "Rubber", "density_g_cm3": 1.5, "color": "#1c1c1c"}));
    assert_eq!(created["appearance"]["color"], "#1c1c1c", "{created}");
    let list = run(&mut s, "material.list", json!({}));
    let materials = list["materials"].as_array().unwrap();
    assert_eq!(materials.len(), solidcraft_doc::MATERIALS.len() + 1);
    let rubber = materials.last().unwrap();
    assert_eq!((rubber["name"].as_str(), rubber["user"].as_bool()), (Some("Rubber"), Some(true)));

    // Assigned (in any case), it sets the mass and the body's colour.
    run(&mut s, "material.assign", json!({"bodies": ["Pad"], "material": "rubber"}));
    close(&mass(&mut s), 1.5);
    let pad = s.model.state().body("Pad").unwrap().clone();
    let grey = 28.0 / 255.0;
    assert_eq!(s.doc.body_color(&pad), Some([grey, grey, grey]));

    // A new density changes the mass; a new name carries the body with it.
    run(&mut s, "material.edit", json!({"material": "Rubber", "density_g_cm3": 1.2, "name": "Foot Rubber"}));
    close(&mass(&mut s), 1.2);
    let props = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(props["bodies"][0]["material"], "Foot Rubber");
    run(&mut s, "edit.undo", json!({}));
    close(&mass(&mut s), 1.5);
    assert_eq!(s.doc.materials.get("Pad").map(String::as_str), Some("Rubber"));
    run(&mut s, "edit.redo", json!({}));

    // A material in use is not deleted unless its bodies get another one.
    let refused = s.execute("material.delete", &json!({"material": "Foot Rubber"})).unwrap_err().to_string();
    assert!(refused.contains("body `Pad`"), "{refused}");
    run(&mut s, "material.delete", json!({"material": "Foot Rubber", "replace": "Oak"}));
    close(&mass(&mut s), 0.75);
    assert!(s.doc.user_materials.is_empty());
    assert!(s.execute("material.assign", &json!({"bodies": ["Pad"], "material": "Foot Rubber"})).is_err());
    run(&mut s, "edit.undo", json!({}));
    close(&mass(&mut s), 1.2);
}

/// #532: names and densities are checked before anything changes.
#[test]
fn user_defined_material_refuses_bad_names_and_densities() {
    let mut s = Session::default();
    run(&mut s, "material.create", json!({"name": "Walnut", "density_g_cm3": 0.65}));
    let doc = s.doc.clone();
    for p in [
        json!({"name": "Steel", "density_g_cm3": 1.0}),
        json!({"name": "walnut", "density_g_cm3": 1.0}),
        json!({"name": "  ", "density_g_cm3": 1.0}),
        json!({"name": "x".repeat(129), "density_g_cm3": 1.0}),
        json!({"name": "Leather", "density_g_cm3": 0}),
        json!({"name": "Leather", "density_g_cm3": -1}),
        json!({"name": "Leather", "density_g_cm3": 1e9}),
        json!({"name": "Leather", "density_g_cm3": "heavy"}),
        json!({"name": "Leather"}),
        json!({"name": "Leather", "density_g_cm3": 0.9, "color": "#zzzzzz"}),
        json!({"name": "Leather", "density_g_cm3": 0.9, "appearance": "No Such Look"}),
        json!({"name": "Leather", "density_g_cm3": 0.9, "opacity": 0.5}),
    ] {
        assert!(s.execute("material.create", &p).is_err(), "{p}");
    }
    for p in [
        json!({"material": "Steel", "density_g_cm3": 1.0}),
        json!({"material": "Walnut", "name": "Oak"}),
        json!({"material": "Walnut", "density_g_cm3": 0}),
    ] {
        assert!(s.execute("material.edit", &p).is_err(), "{p}");
    }
    assert_eq!(*s.doc, *doc);
}

/// #532: a design's materials are saved with it: after reopening, the bodies keep their
/// material, its density and its look.
#[test]
fn user_defined_material_survives_save_and_reopen() {
    let (dir, path) = temp_design("saved");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Tab"}));
    run(&mut s, "material.create", json!({"name": "Leather", "density_g_cm3": 0.86, "appearance": "Rubber - Black"}));
    run(&mut s, "material.assign", json!({"bodies": ["Tab"], "material": "Leather"}));
    run(&mut s, "file.save", json!({"path": path}));
    assert!(s.doc.to_json().contains(&format!("\"{}\"", solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT))));

    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let props = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(props["bodies"][0]["material"], "Leather", "{props}");
    close(&props["total"]["mass_g"], 0.86);
    assert_eq!(s.doc.material_look("Leather").map(|l| l.name), Some("Rubber - Black".to_string()));
    run(&mut s, "material.edit", json!({"material": "Leather", "density_g_cm3": 0.9}));
    close(&mass(&mut s), 0.9);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #532: designs from before user materials open unchanged: built-in assignments keep their
/// densities and a design without its own materials saves no `user_materials`. A damaged
/// file's unusable density falls back to the default instead of producing nonsense masses, and
/// the body reads as the default material.
#[test]
fn designs_without_user_materials_open_unchanged() {
    let (dir, path) = temp_design("legacy");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Plate"}));
    run(&mut s, "material.assign", json!({"bodies": ["Plate"], "material": "Steel"}));
    let mut legacy: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert!(legacy.get("user_materials").is_none());
    legacy["format"] = json!("solidcraft/16");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    close(&mass(&mut s), 7.85);
    assert!(s.doc.user_materials.is_empty());
    assert!(s.execute("material.delete", &json!({"material": "Steel"})).is_err(), "no design materials: nothing to delete");

    legacy["user_materials"] = json!([{"name": "Broken", "density": -3.0, "appearance": {"name": "Broken", "color": [1, 2, 3], "opacity": 9.0}}]);
    legacy["materials"] = json!({"Plate": "Broken"});
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    run(&mut s, "doc.open", json!({"path": path}));
    close(&mass(&mut s), solidcraft_doc::DEFAULT_DENSITY);
    // #4783: it reads as the material it weighs; the file's assignment is kept as it is.
    let props = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(props["bodies"][0]["material"], "Default", "{props}");
    let measured = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(measured["bodies"][0]["material"], "Default", "{measured}");
    close(&measured["bodies"][0]["density_g_cm3"], solidcraft_doc::DEFAULT_DENSITY);
    assert_eq!(s.doc.materials.get("Plate").map(String::as_str), Some("Broken"));
    assert_eq!(s.doc.material_look("Broken").map(|l| l.opacity), Some(1.0));
    // Assigning the default material, in any case, clears the damaged assignment.
    run(&mut s, "material.assign", json!({"bodies": ["Plate"], "material": "default"}));
    assert!(!s.doc.materials.contains_key("Plate"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// #532: configuration material cells take a design's own material and follow its rename.
#[test]
fn configurations_switch_to_a_user_defined_material() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Slat"}));
    run(&mut s, "material.create", json!({"name": "Walnut", "density_g_cm3": 0.65}));
    run(&mut s, "config.configure", json!({"materials": ["Slat"]}));
    run(&mut s, "config.row", json!({"name": "Walnut", "values": {"material:Slat": "walnut"}}));
    run(&mut s, "config.activate", json!({"row": "Walnut"}));
    close(&mass(&mut s), 0.65);
    // A row of the default material, in any case, clears the assignment.
    run(&mut s, "config.row", json!({"name": "Plain", "values": {"material:Slat": "default"}}));
    run(&mut s, "config.activate", json!({"row": "Plain"}));
    assert!(!s.doc.materials.contains_key("Slat"));
    run(&mut s, "material.edit", json!({"material": "Walnut", "name": "Black Walnut"}));
    run(&mut s, "config.activate", json!({"row": "Default"}));
    run(&mut s, "config.activate", json!({"row": "Walnut"}));
    assert_eq!(s.doc.materials.get("Slat").map(String::as_str), Some("Black Walnut"));
    run(&mut s, "material.delete", json!({"material": "Black Walnut", "replace": "Default"}));
    assert!(!s.doc.materials.contains_key("Slat"));
}
