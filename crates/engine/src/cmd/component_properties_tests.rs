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
    for about in [json!("origin"), json!([1, 2]), json!([1, 2, "z"]), json!([f64::MAX, 0, 0]), json!({"x": 1})] {
        assert!(s.execute("component.properties", &json!({"about": about})).is_err(), "{about}");
    }
}

fn tensor(v: &Value) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| v[r][c].as_f64().unwrap_or_else(|| panic!("no [{r}][{c}] in {v}"))))
}

fn tensor_close(actual: &Value, want: [[f64; 3]; 3], rel: f64) {
    let got = tensor(actual);
    let scale = want.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    for (g, w) in got.iter().flatten().zip(want.iter().flatten()) {
        assert!((g - w).abs() <= rel * scale, "{got:?} != {want:?}");
    }
}

/// #8095: a 10 × 20 × 30 mm steel box's inertia tensor about its centre of mass is
/// m (b² + c²) / 12 … on the diagonal, its principal axes are its edges, and turning its
/// placement turns the tensor with it.
#[test]
fn box_inertia_matches_the_closed_form_and_turns_with_its_placement() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Block", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"}));
    run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Steel"}));
    let r = run(&mut s, "component.properties", json!({"component": c["component"]}));
    let total = &r["total"];
    close(&total["bbox"]["max"][1], 20.0);
    let m = total["mass_g"].as_f64().unwrap();
    close(&total["mass_g"], 6000.0 * s.doc.density("Block") / 1000.0);
    let (a, b, h) = (10.0, 20.0, 30.0);
    let (ixx, iyy, izz) = (m * (b * b + h * h) / 12.0, m * (a * a + h * h) / 12.0, m * (a * a + b * b) / 12.0);
    tensor_close(&total["inertia_g_mm2"], [[ixx, 0.0, 0.0], [0.0, iyy, 0.0], [0.0, 0.0, izz]], 1e-9);
    tensor_close(&r["bodies"][0]["inertia_g_mm2"], [[ixx, 0.0, 0.0], [0.0, iyy, 0.0], [0.0, 0.0, izz]], 1e-9);
    // Smallest first: about the longest edge (z), largest about the shortest (x).
    for (k, want) in [izz, iyy, ixx].into_iter().enumerate() {
        assert!((total["principal_moments_g_mm2"][k].as_f64().unwrap() - want).abs() < 1e-9 * ixx, "{total}");
    }
    // Right-handed: z × y is −x.
    assert_eq!(total["principal_axes"], json!([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]));
    for (k, want) in [5.0, 10.0, 15.0].iter().enumerate() {
        close(&total["first_moment_g_mm"][k], m * want);
    }
    // A quarter turn about z swaps the x and y moments; the principal moments stay.
    run(&mut s, "occurrence.move", json!({"occurrence": c["occurrence"], "axis": [0, 0, 1], "angle": "90 deg", "capture": false}));
    let turned = run(&mut s, "component.properties", json!({"component": c["component"]}));
    tensor_close(&turned["total"]["inertia_g_mm2"], [[iyy, 0.0, 0.0], [0.0, ixx, 0.0], [0.0, 0.0, izz]], 1e-9);
    assert_eq!(turned["total"]["principal_moments_g_mm2"], total["principal_moments_g_mm2"]);
    let x_axis = &turned["total"]["principal_axes"][2];
    assert!(x_axis[0].as_f64().unwrap().abs() < 1e-9 && (x_axis[1].as_f64().unwrap().abs() - 1.0).abs() < 1e-9, "{turned}");
}

/// #8095: an assembly's tensor adds each placement's own about the shared centre of mass by
/// parallel axes, and `about` takes it about any point; a cylinder's comes from its
/// tessellation, close to m r² / 2 about its axis.
#[test]
fn assembly_inertia_adds_placements_by_parallel_axes_and_about_any_point() {
    let mut s = Session::default();
    let (component, _, body) = box_in(&mut s, "Block");
    run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20, 0, 0]}));
    let root = run(&mut s, "component.properties", json!({"component": "root", "about": [0, 0, 0]}));
    let total = &root["total"];
    let m = s.doc.density(&body);
    close(&total["mass_g"], 2.0 * m);
    // Each 10 mm cube: m (10² + 10²) / 12 on its diagonal; each sits 10 mm along x from (15, 5, 5).
    let own = m * 200.0 / 12.0;
    let at_centre = [[2.0 * own, 0.0, 0.0], [0.0, 2.0 * own + 2.0 * m * 100.0, 0.0], [0.0, 0.0, 2.0 * own + 2.0 * m * 100.0]];
    tensor_close(&total["inertia_g_mm2"], at_centre, 1e-9);
    // About the origin: the parallel-axis term of the whole mass at (15, 5, 5).
    let (mm, cv) = (2.0 * m, [15.0, 5.0, 5.0]);
    let c2: f64 = cv.iter().map(|x| x * x).sum();
    let at_origin = [0, 1, 2].map(|r| [0, 1, 2].map(|k| at_centre[r][k] + mm * (if r == k { c2 } else { 0.0 } - cv[r] * cv[k])));
    assert_eq!(total["inertia_about"]["point"], json!([0.0, 0.0, 0.0]));
    tensor_close(&total["inertia_about"]["inertia_g_mm2"], at_origin, 1e-9);
    assert!(total.get("inertia_about").is_some() && run(&mut s, "component.properties", json!({}))["total"].get("inertia_about").is_none());

    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 8, "height": 10, "body_name": "Pin"}));
    let r = run(&mut s, "component.properties", json!({}));
    let (m, r2) = (r["total"]["mass_g"].as_f64().unwrap(), 64.0);
    let i = tensor(&r["total"]["inertia_g_mm2"]);
    assert!((i[2][2] - m * r2 / 2.0).abs() < 1e-3 * m * r2 / 2.0, "{i:?}");
    assert!((i[0][0] - m * (3.0 * r2 + 100.0) / 12.0).abs() < 1e-3 * i[0][0], "{i:?}");
}

/// #8095: an open surface has no volume, so no tensor; the closed bodies beside it keep theirs.
#[test]
fn open_bodies_have_no_inertia_and_leave_the_closed_ones_alone() {
    let mut s = Session::default();
    for (command, params) in [
        ("sketch.create", json!({"plane": "XY", "name": "Profile"})),
        ("sketch.line", json!({"points": [[30, 0], [30, 40]], "ids": ["side"]})),
        ("sketch.finish", json!({})),
        ("surface.revolve", json!({"sketch": "Profile", "curves": ["side"], "axis": "y", "angle": "270 deg", "body_name": "Tube"})),
    ] {
        run(&mut s, command, params);
    }
    let open = run(&mut s, "component.properties", json!({"about": [1, 2, 3]}));
    assert_eq!(open["bodies"][0]["inertia_g_mm2"], Value::Null);
    for key in ["inertia_g_mm2", "principal_moments_g_mm2", "principal_axes"] {
        assert_eq!(open["total"][key], Value::Null, "{key}");
    }
    assert_eq!(open["total"]["inertia_about"]["inertia_g_mm2"], Value::Null);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "body_name": "Box"}));
    let both = run(&mut s, "component.properties", json!({}));
    let m = both["total"]["mass_g"].as_f64().unwrap();
    tensor_close(&both["total"]["inertia_g_mm2"], [[m * 200.0 / 12.0, 0.0, 0.0], [0.0, m * 200.0 / 12.0, 0.0], [0.0, 0.0, m * 200.0 / 12.0]], 1e-9);
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

/// #8706: Properties of a large assembly took minutes because every placed copy of a body was
/// transformed and measured again (three fine tessellations each). Every placement of a body now
/// reuses one measurement of it: 41 placements measure no placed copy afresh and tessellate
/// each distinct body's inertia spread (#8095) once, exactly as one placement does, and the
/// answer is the same when measured again.
#[test]
fn repeated_placements_reuse_one_measurement_of_their_body() {
    let mut s = Session::default();
    let (component, _, _) = box_in(&mut s, "Block");
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    for i in 1..40 {
        run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20 * i, 0, 0]}));
    }
    let turned = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [0, 50, 0]}));
    run(&mut s, "occurrence.move", json!({"occurrence": turned["occurrence"], "translate": [0, 0, 0], "axis": [1, 1, 1], "angle": "30 deg"}));
    let first = run(&mut s, "component.properties", json!({"component": "root"}));
    assert_eq!(first["body_count"], 82, "{first}");
    let (again, all) = super::work::capture(|| run(&mut s, "component.properties", json!({"component": "root"})));
    assert_eq!(first, again);
    let (block, one) = super::work::capture(|| run(&mut s, "occurrence.properties", json!({"occurrence": turned["occurrence"]})));
    // Before, every placed copy was transformed and tessellated afresh (82 here); now no copy
    // is, and the work is the same for 41 placements as for one: one spread per distinct body.
    let per_body = super::work::Work { placed_measures: 0, spreads: 2 };
    assert_eq!(all, per_body, "41 placements");
    assert_eq!(one, per_body, "one placement");
    close(&first["total"]["volume_mm3"], 41.0 * block["total"]["volume_mm3"].as_f64().unwrap());
}

/// #8706: a body measured where a rigid placement puts it, from its own measurement, matches its
/// placed copy measured afresh: volume, area, centre and counts, and the box (to the chord of
/// the meshes it is taken from) also when the placement turns it off the axes.
#[test]
fn a_placed_body_measures_as_its_placed_copy() {
    use solidcraft_geom::Vec3;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 5}));
    let state = s.model.state();
    let placements = [
        solidcraft_doc::IDENTITY,
        solidcraft_doc::rigid(Vec3::new(5.0, -3.0, 7.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2),
        solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 1.0, 1.0), 0.5),
    ];
    for body in &state.bodies {
        for m in placements {
            let fast = super::measure_placed(body, m).unwrap();
            let placed = solidcraft_kernel::transform_matrix(&body.body, m).unwrap();
            let slow = solidcraft_kernel::measure(&placed).unwrap();
            let near = |a: f64, b: f64, tol: f64| assert!((a - b).abs() <= tol * b.abs().max(1.0), "{} {a} != {b}", body.name);
            near(fast.volume, slow.volume, 1e-4);
            near(fast.area, slow.area, 1e-4);
            assert!((fast.centroid - slow.centroid).len() < 1e-3, "{} centre {:?} != {:?}", body.name, fast.centroid, slow.centroid);
            let box_near = (fast.bbox.min - slow.bbox.min).len() < 0.05 && (fast.bbox.max - slow.bbox.max).len() < 0.05;
            assert!(box_near, "{} box {:?} != {:?}", body.name, fast.bbox, slow.bbox);
            assert_eq!((fast.faces, fast.edges, fast.vertices), (slow.merged.faces, slow.merged.edges, slow.merged.vertices));
            assert_eq!(fast.closed, crate::cmd::inspect::encloses_volume(&placed));
        }
    }
}

/// #8706: a window waiting for the properties sees how many bodies are measured and can stop the
/// measuring part way.
#[test]
fn a_watcher_sees_the_bodies_measured_and_can_stop_them() {
    use std::sync::Arc;
    let mut s = Session::default();
    let (component, _, _) = box_in(&mut s, "Block");
    for i in 1..4 {
        run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20 * i, 0, 0]}));
    }
    let progress = Arc::new(solidcraft_doc::progress::Progress::default());
    let r = solidcraft_doc::progress::watch(progress.clone(), || s.execute("component.properties", &json!({"component": "root"})));
    assert_eq!(r.unwrap()["body_count"], 4);
    assert_eq!(progress.work(), Some(("measuring", 4, 4)));
    progress.cancel();
    let stopped = solidcraft_doc::progress::watch(progress, || s.execute("component.properties", &json!({"component": "root"})));
    assert!(stopped.unwrap_err().to_string().contains("cancelled"));
}
