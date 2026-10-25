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

fn panel_blank(s: &mut Session, width: f64, height: f64) {
    run(s, "sheet.manage_rules", json!({"name": "Panel 0.8", "thickness": "0.8", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [width, height]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Blank", "body_name": "Panel"}));
}

fn perforated_panel(s: &mut Session) {
    panel_blank(s, 20.0, 10.0);
    run(s, "sheet.perforate", json!({"diameter": "2", "pitch": "10", "margin": "1", "pattern": "square"}));
    run(s, "material.assign", json!({"bodies": ["Panel"], "material": "Steel"}));
}

/// Two radius-1 holes centred at (2, 2) and (12, 2), through a 20 x 10 x 0.8 blank.
fn perforated_panel_expected() -> (f64, f64, [f64; 3]) {
    let removed = 2.0 * std::f64::consts::PI * 0.8;
    let volume = 160.0 - removed;
    let area = 448.0 - 0.8 * std::f64::consts::PI;
    (volume, area, [(160.0 * 10.0 - removed * 7.0) / volume, (160.0 * 5.0 - removed * 2.0) / volume, 0.4])
}

#[test]
fn perforated_sheet_properties_include_holes_and_their_off_centre_volume_moment() {
    let mut s = Session::default();
    perforated_panel(&mut s);
    let (volume, area, center) = perforated_panel_expected();
    let density = s.doc.density("Panel");
    let doc = s.doc.clone();
    let undo = s.undo.len();
    for command in ["component.properties", "inspect.measure"] {
        let result = run(&mut s, command, json!({}));
        close(&result["total"]["volume_mm3"], volume);
        close(&result["total"]["mass_g"], volume * density / 1000.0);
        close(&result["total"]["area_mm2"], area);
        for (axis, expected) in center.iter().enumerate() {
            close(&result["bodies"][0]["center_of_mass"][axis], *expected);
        }
    }
    let inspected = run(&mut s, "document.inspect", json!({"measure": true}));
    close(&inspected["bodies"][0]["volume_mm3"], volume);
    close(&inspected["bodies"][0]["area_mm2"], area);
    let picked = run(&mut s, "inspect.measure", json!({"items": [{"type": "body", "name": "Panel"}]}));
    close(&picked["items"][0]["volume_mm3"], volume);
    assert!(std::sync::Arc::ptr_eq(&s.doc, &doc));
    assert_eq!(s.undo.len(), undo, "measuring must not add an undo step");
}

#[test]
fn perforated_sheet_instances_keep_physical_properties_in_nested_world_placements() {
    let mut s = Session::default();
    let parent = run(&mut s, "component.create", json!({"name": "Subassembly", "grounded": false}));
    let child = run(&mut s, "component.create", json!({"name": "Sheet", "grounded": false}));
    perforated_panel(&mut s);
    let sheet_density = s.doc.density("Panel");
    run(&mut s, "component.activate", json!({"component": "root"}));
    let copy = run(&mut s, "occurrence.copy", json!({"component": parent["component"], "translate": [40, 0, 0]}));
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence": parent["occurrence"], "translate": [20, 0, 10], "axis": [0, 0, 1], "angle": "90 deg", "capture": false}),
    );
    box_in(&mut s, "Ballast");
    let ballast_body = s.model.state().bodies.last().unwrap().name.clone();
    run(&mut s, "material.assign", json!({"bodies": [ballast_body], "material": "Aluminum"}));
    let ballast_mass = s.doc.density(&ballast_body);
    let (volume, area, center) = perforated_panel_expected();
    let sheet_mass = volume * sheet_density / 1000.0;
    let first = [20.0 - center[1], center[0], 10.0 + center[2]];
    let second = [40.0 + center[0], center[1], center[2]];
    let doc = s.doc.clone();
    let moves = s.pending_moves.clone();
    let undo = s.undo.len();
    for (parent_occurrence, expected) in [(&parent["occurrence"], first), (&copy["occurrence"], second)] {
        let properties =
            run(&mut s, "occurrence.properties", json!({"occurrence": child["occurrence"], "path": [parent_occurrence, child["occurrence"]]}));
        close(&properties["total"]["volume_mm3"], volume);
        close(&properties["total"]["mass_g"], sheet_mass);
        close(&properties["total"]["area_mm2"], area);
        for (axis, coordinate) in expected.iter().enumerate() {
            close(&properties["total"]["center_of_mass"][axis], *coordinate);
        }
    }
    let properties = run(&mut s, "component.properties", json!({"component": "root"}));
    let mass = 2.0 * sheet_mass + ballast_mass;
    close(&properties["total"]["volume_mm3"], 2.0 * volume + 1000.0);
    close(&properties["total"]["mass_g"], mass);
    for axis in 0..3 {
        close(&properties["total"]["center_of_mass"][axis], (sheet_mass * (first[axis] + second[axis]) + ballast_mass * 5.0) / mass);
    }
    let measured = run(&mut s, "inspect.measure", json!({}));
    close(&measured["total"]["volume_mm3"], 2.0 * volume + 1000.0);
    close(&measured["total"]["mass_g"], mass);
    assert_eq!(measured["bodies"].as_array().unwrap().iter().filter(|b| b["sheet_metal"] == true).count(), 2);
    assert!(std::sync::Arc::ptr_eq(&s.doc, &doc));
    assert_eq!(s.pending_moves, moves);
    assert_eq!(s.undo.len(), undo);
}

#[test]
fn perforated_sheet_centroid_follows_a_folded_panel_and_unfolds() {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut s = Session::default();
    panel_blank(&mut s, 30.0, 10.0);
    run(
        &mut s,
        "sheet.fold",
        json!({"points": [[10, 0, 0], [10, 10, 0]], "angle": "90 deg", "radius": "0.8", "position": "centerline", "fixed": [2, 5, 0]}),
    );
    let before = run(&mut s, "component.properties", json!({"component": "root"}));
    run(&mut s, "sheet.perforate", json!({"diameter": "2", "pitch": "10", "margin": "0", "pattern": "square"}));
    let sheet = &s.model.state().sheets[0];
    let holes = sheet.perforation_holes(&sheet.perforations[0]).unwrap();
    assert_eq!(holes, vec![solidcraft_geom::Vec2::new(1.0, 1.0), solidcraft_geom::Vec2::new(21.0, 1.0)]);
    // The right panel turns up by 90 degrees about a y-parallel bend axis at
    // z = radius + thickness. Its mid-plane is radius + thickness/2 from that axis.
    let allowance = FRAC_PI_2 * (0.8 + 0.44 * 0.8);
    let bend_start = 10.0 - allowance / 2.0;
    let right_hole = [bend_start + 1.2, 1.0, 1.6 + 21.0 - bend_start - allowance];
    let old_volume = before["total"]["volume_mm3"].as_f64().unwrap();
    let hole_volume = PI * 0.8;
    let volume = old_volume - 2.0 * hole_volume;
    let result = run(&mut s, "component.properties", json!({"component": "root"}));
    close(&result["total"]["volume_mm3"], volume);
    for (axis, left) in [1.0, 1.0, 0.4].iter().enumerate() {
        let old = before["total"]["center_of_mass"][axis].as_f64().unwrap();
        close(&result["total"]["center_of_mass"][axis], old + hole_volume * (2.0 * old - left - right_hole[axis]) / volume);
    }
    run(&mut s, "sheet.unfold", json!({}));
    let result = run(&mut s, "component.properties", json!({"component": "root"}));
    let volume = 240.0 - 2.0 * hole_volume;
    close(&result["total"]["volume_mm3"], volume);
    for (axis, expected) in [(240.0 * 15.0 - 2.0 * hole_volume * 11.0) / volume, (240.0 * 5.0 - 2.0 * hole_volume) / volume, 0.4].iter().enumerate() {
        close(&result["total"]["center_of_mass"][axis], *expected);
    }
}

#[test]
fn perforated_sheet_correction_rejects_invalid_metadata_without_partial_changes() {
    let mut s = Session::default();
    perforated_panel(&mut s);
    let state = s.model.state();
    let measured = solidcraft_kernel::measure(&state.bodies[0].body).unwrap();
    for pitch in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut sheet = state.sheets[0].clone();
        sheet.perforations[0].pitch = pitch;
        let mut actual = measured.clone();
        assert!(sheet.correct_perforation_properties(&mut actual, &solidcraft_doc::IDENTITY).is_err());
        assert_eq!(actual.volume, measured.volume);
        assert_eq!(actual.area, measured.area);
        assert_eq!(actual.centroid, measured.centroid);
    }
}

#[test]
fn perforated_sheet_body_transfer_preserves_world_mass_and_centroid() {
    let mut s = Session::default();
    perforated_panel(&mut s);
    let moved = run(&mut s, "component.create", json!({"name": "MovedSheet", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": moved["occurrence"], "translate": [100, 0, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Panel"], "component": moved["component"]}));
    let (volume, _, center) = perforated_panel_expected();
    let mass = volume * s.doc.density("Panel") / 1000.0;
    for command in ["component.properties", "inspect.measure"] {
        let result = run(&mut s, command, json!({"component": "root"}));
        close(&result["total"]["volume_mm3"], volume);
        close(&result["total"]["mass_g"], mass);
        for (axis, expected) in center.iter().enumerate() {
            close(&result["bodies"][0]["center_of_mass"][axis], *expected);
        }
    }
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
