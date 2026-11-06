use std::f64::consts::{FRAC_PI_2, PI};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn flat(s: &mut Session) -> Value {
    run(s, "sheet.flat_pattern", json!({}))
}

/// A w x h base flange on XY.
fn base(s: &mut Session, w: f64, h: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [w, h]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
}

fn rectangle_tool(s: &mut Session, name: &str, p0: [f64; 2], p1: [f64; 2]) {
    run(s, "sketch.create", json!({"plane":"XY","name":name}));
    run(s, "sketch.rectangle.two_point", json!({"p0":p0,"p1":p1}));
    run(s, "sketch.finish", json!({}));
}

#[test]
fn shallow_sheet_pocket_preserves_material_depth_export_undo_and_reopen() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Pocket", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Pocket","distance":1,"operation":"cut"}));
    assert!((volume(&mut s) - 14900.0).abs() < 1e-6);
    let f = flat(&mut s);
    assert_eq!(f["cutouts"], json!([]));
    assert_eq!(f["pockets"][0]["side"], "bottom");
    assert_eq!(f["pockets"][0]["depth_mm"], 1.0);
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    assert!(dxf.contains("POCKET_BOTTOM_Z0.000000_TO_1.000000_MM"));
    assert_eq!(solvecraft_io::read_dxf(&dxf).unwrap().len(), 8);
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - 15000.0).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    for command in ["sheet.unfold", "sheet.refold"] {
        run(&mut s, command, json!({}));
        assert!((volume(&mut s) - 14900.0).abs() < 1e-6);
        let state = s.model.state();
        let body = &state.bodies[0].body;
        assert!(!body.contains(solvecraft_geom::Vec3::new(25.0, 25.0, 0.5)));
        assert!(body.contains(solvecraft_geom::Vec3::new(25.0, 25.0, 1.5)));
    }
    let mut reopened = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((volume(&mut reopened) - 14900.0).abs() < 1e-6);
    assert_eq!(flat(&mut reopened)["pockets"], f["pockets"]);
}

#[test]
fn planar_sheet_intersection_retains_exact_outline_and_islands() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Retain", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Retain","through_all":true,"direction":"symmetric","operation":"intersect"}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        assert!((volume(&mut s) - 250.0).abs() < 1e-6);
        assert_eq!(flat(&mut s)["flat_size_mm"], json!([10.0, 10.0, 2.5]));
    }
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    assert_eq!(solvecraft_io::read_dxf(&dxf).unwrap().len(), 4);
    let mut imported = Session::default();
    run(&mut imported, "sketch.insert_dxf", json!({"name":"Retained","text":dxf}));
    run(&mut imported, "sketch.finish", json!({}));
    run(&mut imported, "solid.extrude", json!({"sketch":"Retained","distance":2.5}));
    assert!((volume(&mut imported) - 250.0).abs() < 1e-6);
    // An annular retained region keeps its central hole, with no first-solid shortcut.
    let mut annulus = Session::default();
    base(&mut annulus, 100.0, 60.0);
    run(&mut annulus, "sketch.create", json!({"plane":"XY","name":"Ring"}));
    run(&mut annulus, "sketch.circle.center", json!({"center":[50,30],"radius":10}));
    run(&mut annulus, "sketch.circle.center", json!({"center":[50,30],"radius":5}));
    run(&mut annulus, "sketch.finish", json!({}));
    run(
        &mut annulus,
        "solid.extrude",
        json!({"sketch":"Ring","profiles":[{"point":[57,30]}],"through_all":true,"direction":"symmetric","operation":"intersect"}),
    );
    run(&mut annulus, "sheet.unfold", json!({}));
    let area: f64 = annulus.model.state().sheets[0].flat_regions().unwrap().iter().map(|r| r.area()).sum();
    assert!((area - PI * 75.0).abs() < 1e-8, "exact retained area: {area}");
    // inspect.measure integrates a tessellation; exact arc area is checked above.
    let measured = volume(&mut annulus);
    assert!((measured - PI * 75.0 * 2.5).abs() < 0.1, "measured={measured}");
    assert_eq!(flat(&mut annulus)["cutouts"].as_array().unwrap().len(), 1);
}

#[test]
fn overlapping_opposite_side_pockets_become_true_through_material() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Bottom", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Bottom","distance":1.5,"operation":"cut"}));
    rectangle_tool(&mut s, "Top", [25.0, 20.0], [35.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Top","start_offset":2.5,"direction":"negative","distance":1.5,"operation":"cut"}));
    let expected = 15000.0 - 100.0 * 1.5 * 2.0 + 50.0 * 0.5;
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        assert!((volume(&mut s) - expected).abs() < 1e-6);
        assert_eq!(flat(&mut s)["cutouts"].as_array().unwrap().len(), 1);
        assert_eq!(flat(&mut s)["pockets"].as_array().unwrap().len(), 2);
    }
}

#[test]
fn pockets_survive_forming_away_from_bends_and_refuse_bend_overlap_atomically() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Pocket", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Pocket","distance":1,"operation":"cut"}));
    run(&mut s, "sheet.flange", json!({"edges":[[50,0,2.5]],"height":20}));
    assert!((volume(&mut s) - (18972.62196081845 - 100.0)).abs() < 0.2);
    run(&mut s, "sheet.unfold", json!({}));
    let unfolded = volume(&mut s);
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - (18972.62196081845 - 100.0)).abs() < 0.2);
    assert!(unfolded > 14900.0);
    let mut cross = Session::default();
    base(&mut cross, 100.0, 60.0);
    rectangle_tool(&mut cross, "Pocket", [45.0, 20.0], [55.0, 30.0]);
    run(&mut cross, "solid.extrude", json!({"sketch":"Pocket","distance":1,"operation":"cut"}));
    let before = cross.doc.to_json();
    let undo = cross.undo.len();
    let error = cross.execute("sheet.fold", &json!({"points":[[50,0,2.5],[50,60,2.5]]})).unwrap_err().to_string();
    assert!(error.contains("pocket") && error.contains("bend"), "{error}");
    assert_eq!(cross.doc.to_json(), before);
    assert_eq!(cross.undo.len(), undo);
    assert!((volume(&mut cross) - 14900.0).abs() < 1e-6);
}

#[test]
fn unsupported_sheet_machining_is_atomic_but_true_no_change_is_allowed() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Taper", [20.0, 20.0], [30.0, 30.0]);
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let error = s.execute("solid.extrude", &json!({"sketch":"Taper","distance":1,"operation":"cut","taper":"5 deg"})).unwrap_err().to_string();
    assert!(error.contains("sheet") && error.contains("preserv"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), undo);
    rectangle_tool(&mut s, "Outside", [120.0, 20.0], [130.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Outside","distance":1,"operation":"cut","taper":"5 deg"}));
    assert!((volume(&mut s) - 15000.0).abs() < 1e-6);
    rectangle_tool(&mut s, "All", [-20.0, -20.0], [120.0, 80.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"All","through_all":true,"direction":"symmetric","operation":"intersect"}));
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - 15000.0).abs() < 1e-6);
}

#[test]
fn empty_sheet_intersection_removes_geometry_and_metadata_together() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Outside", [120.0, 20.0], [130.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Outside","through_all":true,"direction":"symmetric","operation":"intersect"}));
    assert!(s.model.state().bodies.is_empty());
    assert!(s.model.state().sheets.is_empty());
    assert!(s.execute("sheet.unfold", &json!({})).is_err());
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - 15000.0).abs() < 1e-6);
}

#[test]
fn intersect_preserves_both_material_regions_of_a_logical_ring_sheet() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Ring"}));
    for radius in [10.0, 5.0] {
        run(&mut s, "sketch.circle.center", json!({"center":[50,30],"radius":radius}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":"Ring","profiles":[{"point":[57,30]}],"through_all":true,"direction":"symmetric","operation":"cut"}),
    );
    rectangle_tool(&mut s, "Crop", [35.0, 15.0], [65.0, 45.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Crop","through_all":true,"direction":"symmetric","operation":"intersect"}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        let state = s.model.state();
        let regions = state.sheets[0].flat_regions().unwrap();
        assert_eq!(regions.len(), 2);
        let area: f64 = regions.iter().map(|r| r.area()).sum();
        assert!((area - (900.0 - PI * 75.0)).abs() < 1e-8, "area={area}");
        let body = &state.bodies[0].body;
        assert!(body.contains(solvecraft_geom::Vec3::new(50.0, 30.0, 1.0)));
        assert!(!body.contains(solvecraft_geom::Vec3::new(57.0, 30.0, 1.0)));
        assert!(body.contains(solvecraft_geom::Vec3::new(64.0, 30.0, 1.0)));
        assert!(!body.contains(solvecraft_geom::Vec3::new(70.0, 30.0, 1.0)));
        assert!((volume(&mut s) - (900.0 - PI * 75.0) * 2.5).abs() < 0.2);
    }
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    assert_eq!(solvecraft_io::read_dxf(&dxf).unwrap().len(), 6);
}

#[test]
fn machining_a_folded_parallel_panel_preserves_its_pocket_on_unfold() {
    use solvecraft_doc::{apply_point, apply_vector};
    use solvecraft_geom::Vec3;
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sheet.flange", json!({"edges":[[50,0,2.5]],"height":20}));
    let sh = s.model.state().sheets[0].clone();
    let points = sh.panel(&sh.flanges[0]).outer.polyline(1e-3);
    let c = points.iter().copied().fold(solvecraft_geom::Vec2::ZERO, |sum, p| sum + p) / (points.len() as f64);
    let w = sh.world(Some(0));
    let origin = apply_point(&w, Vec3::ZERO);
    let x = apply_vector(&w, Vec3::X);
    let y = apply_vector(&w, Vec3::Y);
    run(
        &mut s,
        "sketch.create",
        json!({"name":"PanelPocket","plane":{"origin":[origin.x,origin.y,origin.z],"x_dir":[x.x,x.y,x.z],"y_dir":[y.x,y.y,y.z]}}),
    );
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[c.x-2.0,c.y-2.0],"p1":[c.x+2.0,c.y+2.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"PanelPocket","distance":1,"operation":"cut"}));
    assert!((volume(&mut s) - (18972.62196081845 - 16.0)).abs() < 0.2);
    run(&mut s, "sheet.unfold", json!({}));
    let state = s.model.state();
    let body = &state.bodies[0].body;
    assert!(!body.contains(Vec3::new(c.x, c.y, 0.5)));
    assert!(body.contains(Vec3::new(c.x, c.y, 1.5)));
    assert_eq!(flat(&mut s)["pockets"][0]["depth_mm"], 1.0);
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - (18972.62196081845 - 16.0)).abs() < 0.2);
}

#[test]
fn flipped_sheet_pocket_keeps_depth_in_the_local_thickness_direction() {
    let mut s = Session::default();
    rectangle_tool(&mut s, "Base", [0.0, 0.0], [100.0, 60.0]);
    run(&mut s, "sheet.flange", json!({"sketch":"Base","flip":true}));
    rectangle_tool(&mut s, "Pocket", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Pocket","distance":1,"direction":"negative","operation":"cut"}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        assert!((volume(&mut s) - 14900.0).abs() < 1e-6);
        let state = s.model.state();
        assert!(!state.bodies[0].body.contains(solvecraft_geom::Vec3::new(25.0, 25.0, -0.5)));
        assert!(state.bodies[0].body.contains(solvecraft_geom::Vec3::new(25.0, 25.0, -1.5)));
    }
}

#[test]
fn multiple_profile_intersect_keeps_tool_union_for_solids_and_sheet_groups() {
    for sheet in [false, true] {
        let mut s = Session::default();
        if sheet {
            base(&mut s, 100.0, 60.0);
        } else {
            run(&mut s, "solid.box", json!({"length":100,"width":60,"height":2.5,"body_name":"Blank"}));
        }
        let target = s.model.state().bodies[0].name.clone();
        run(&mut s, "solid.box", json!({"corner":[200,0,0],"length":10,"width":10,"height":10,"body_name":"Control"}));
        run(&mut s, "sketch.create", json!({"plane":"XY","name":"TwoIslands"}));
        for x in [20.0, 60.0] {
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[x,20],"p1":[x+10.0,30]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch":"TwoIslands","operation":"intersect","distance":3,"targets":[target]}));
        assert!((volume(&mut s) - 1500.0).abs() < 1e-6);
        if sheet {
            for command in ["sheet.unfold", "sheet.refold"] {
                run(&mut s, command, json!({"body":target}));
                assert!((volume(&mut s) - 1500.0).abs() < 1e-6);
                assert_eq!(flat(&mut s)["outline"].as_array().unwrap().len(), 2);
            }
        }
        let state = s.model.state();
        for x in [25.0, 65.0] {
            assert!(state.bodies.iter().any(|b| b.body.contains(solvecraft_geom::Vec3::new(x, 25.0, 1.0))));
        }
        assert!(!state.bodies.iter().any(|b| b.body.contains(solvecraft_geom::Vec3::new(50.0, 25.0, 1.0))));
        assert!(state.bodies.iter().any(|b| b.name == "Control" && b.body.contains(solvecraft_geom::Vec3::new(205.0, 5.0, 5.0))));
    }
}

#[test]
fn deleting_sheet_material_prunes_cosmetic_threads_and_undo_restores_them() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"ThreadPlate"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[100,60]}));
    run(&mut s, "sketch.circle.center", json!({"center":[50,30],"radius":5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch":"ThreadPlate","profiles":[{"point":[10,10]}]}));
    run(&mut s, "solid.thread", json!({"face":[55,30,1],"designation":"M10"}));
    assert_eq!(s.model.state().threads.len(), 1);
    rectangle_tool(&mut s, "Outside", [120.0, 20.0], [130.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Outside","through_all":true,"direction":"symmetric","operation":"intersect"}));
    assert!(s.model.state().bodies.is_empty());
    assert!(s.model.state().sheets.is_empty());
    assert!(s.model.state().threads.is_empty());
    assert!(solvecraft_doc::world_state(&s.doc, &s.model.state()).threads.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.model.state().threads.len(), 1);
}

#[test]
fn sheet_pocket_transfer_keeps_local_depth_world_pose_and_history() {
    use solvecraft_geom::Vec3;
    fn check(s: &Session, name: &str, owner: u64, local_origin: Vec3, local_y: Vec3, transferred: bool) {
        let state = s.model.state();
        let mb = state.body(name).unwrap();
        assert_eq!(mb.component, owner);
        assert_eq!(mb.transferred, transferred);
        let sh = state.sheets.iter().find(|sh| sh.body == name).unwrap();
        assert!(sh.frame.origin.dist(local_origin) < 1e-8, "{:?}", sh.frame);
        assert!(sh.frame.x.dist(Vec3::X) < 1e-8);
        assert!(sh.frame.y.dist(local_y) < 1e-8);
        assert_eq!(sh.pockets.len(), 1);
        assert_eq!((sh.pockets[0].z0, sh.pockets[0].z1), (0.0, 1.0));
        let cut = sh.frame.to_world(solvecraft_geom::Vec2::new(25.0, 25.0)) + sh.frame.normal() * 0.5;
        let floor = sh.frame.to_world(solvecraft_geom::Vec2::new(25.0, 25.0)) + sh.frame.normal() * 1.5;
        assert!(!mb.body.contains(cut));
        assert!(mb.body.contains(floor));
        let world = s.world_state();
        let body = &world.body(name).unwrap().body;
        let bounds = body.tessellate(1e-3).unwrap().bounds();
        assert!(bounds.min.dist(Vec3::ZERO) < 1e-6, "{bounds:?}");
        assert!(bounds.max.dist(Vec3::new(100.0, 60.0, 2.5)) < 1e-6, "{bounds:?}");
        assert!(!body.contains(Vec3::new(25.0, 25.0, 0.5)));
        assert!(body.contains(Vec3::new(25.0, 25.0, 1.5)));
        assert!((solvecraft_kernel::measure(body).unwrap().volume - 14900.0).abs() < 1e-6);
    }
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Pocket", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Pocket","distance":1,"operation":"cut"}));
    let name = s.model.state().sheets[0].body.clone();
    check(&s, &name, 0, Vec3::ZERO, Vec3::Y, false);
    let destination = run(&mut s, "component.create", json!({"name":"Machined","parent":0}));
    let component = destination["component"].as_u64().unwrap();
    let occurrence = destination["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,20,10],"axis":[1,0,0],"angle":"90 deg","capture":true}));
    run(&mut s, "component.move_bodies", json!({"component":component,"bodies":[name]}));
    assert!(!s.doc.body_offsets.contains_key(&name), "recorded transfer must not leave a legacy placement offset");
    let origin = Vec3::new(-30.0, -10.0, 20.0);
    let y = Vec3::new(0.0, 0.0, -1.0);
    check(&s, &name, component, origin, y, true);
    run(&mut s, "edit.undo", json!({}));
    check(&s, &name, 0, Vec3::ZERO, Vec3::Y, false);
    run(&mut s, "edit.redo", json!({}));
    check(&s, &name, component, origin, y, true);
    run(&mut s, "component.activate", json!({"component":component}));
    for command in ["sheet.unfold", "sheet.refold"] {
        run(&mut s, command, json!({"body":name}));
        check(&s, &name, component, origin, y, true);
        let f = run(&mut s, "sheet.flat_pattern", json!({"body":name}));
        assert_eq!(f["pockets"][0]["side"], "bottom");
        assert_eq!(f["pockets"][0]["depth_mm"], 1.0);
    }
    let mut reopened = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, &name, component, origin, y, true);
    run(&mut reopened, "component.activate", json!({"component":component}));
    run(&mut reopened, "sheet.unfold", json!({"body":name}));
    check(&reopened, &name, component, origin, y, true);
}

#[test]
fn base_and_edge_flange_follow_the_k_factor() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    assert!(rel(volume(&mut s), 15000.0) < 1e-9);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    // Oracle 68: flat = 60 − 5 + (π/2)(2.5 + 0.44·2.5) + 20 − 5.
    let want = 60.0 - 5.0 + FRAC_PI_2 * (2.5 + 0.44 * 2.5) + 15.0;
    let f = flat(&mut s);
    assert!((f["flat_size_mm"][1].as_f64().unwrap_or(0.0) - want).abs() < 1e-6, "{f}");
    assert_eq!(f["bends"][0]["direction"], "up");
    assert!(rel(volume(&mut s), 18972.622) < 1e-4, "{}", volume(&mut s));
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["total"]["faces"], 14, "{m}");
    // The angle is a parameter: 45° (oracle 70 uses height 15).
    let names = s.doc.features.last().map(|f| f.param_names.clone()).unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": names[0], "expression": "15"}));
    run(&mut s, "parameters.change", json!({"name": names[1], "expression": "45 deg"}));
    assert!(rel(volume(&mut s), 18450.777) < 1e-4, "{}", volume(&mut s));
    assert!((flat(&mut s)["flat_size_mm"][1].as_f64().unwrap_or(0.0) - 73.685298).abs() < 1e-5);
    // DXF: outline and an up bend line.
    let d = run(&mut s, "sheet.export_dxf", json!({}));
    let text = d["dxf"].as_str().unwrap_or_default();
    assert!(text.contains("BEND_UP") && text.contains("OUTLINE"), "{text}");
    assert!(s.execute("sheet.flange", &json!({"edges": [[50, 30, 40]], "height": 20})).is_err(), "not on an edge");
}

#[test]
fn rules_are_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "Thick", "expression": "1.5 mm"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "Custom", "thickness": "Thick", "k_factor": 0.33, "bend_radius": "3 mm"}));
    base(&mut s, 80.0, 50.0);
    run(&mut s, "sheet.flange", json!({"edges": [[40, 50, 1.5], [40, 0, 1.5]], "height": 20}));
    // Oracle 71: 50 − 9 + 2·(π/2)(3 + 0.495) + 2·15.5 = 82.980.
    let want = 50.0 - 2.0 * 4.5 + 2.0 * FRAC_PI_2 * (3.0 + 0.33 * 1.5) + 2.0 * 15.5;
    assert!((flat(&mut s)["flat_size_mm"][1].as_f64().unwrap_or(0.0) - want).abs() < 1e-6);
    assert!(rel(volume(&mut s), 10053.717) < 1e-4, "{}", volume(&mut s));
    // Thicker: everything follows.
    run(&mut s, "parameters.change", json!({"name": "Thick", "expression": "2 mm"}));
    let want = 50.0 - 2.0 * 5.0 + 2.0 * FRAC_PI_2 * (3.0 + 0.33 * 2.0) + 2.0 * 15.0;
    assert!((flat(&mut s)["flat_size_mm"][1].as_f64().unwrap_or(0.0) - want).abs() < 1e-6);
    let l = run(&mut s, "sheet.rules", json!({}));
    assert_eq!(l["active"], "Custom", "{l}");
    assert!(s.execute("sheet.manage_rules", &json!({"name": "Bad", "k_factor": 3})).is_err());
}

#[test]
fn contour_flange_hem_and_tray() {
    // Oracle 72.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}, "name": "Contour"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, -30], [70, -30]], "ids": ["l1", "l2", "l3"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Contour", "curves": ["l1", "l2", "l3"], "distance": 50}));
    assert!(rel(volume(&mut s), 12097.622) < 1e-4, "{}", volume(&mut s));
    assert!((flat(&mut s)["flat_size_mm"][0].as_f64().unwrap_or(0.0) - 96.309734).abs() < 1e-5);
    // Oracle 73: flange, then a flat hem of 8.
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    run(&mut s, "sheet.hem", json!({"edges": [[50, 0, 20]], "length": 8}));
    assert!(rel(volume(&mut s), 21334.723) < 1e-4, "{}", volume(&mut s));
    let b = flat(&mut s)["bends"].clone();
    assert!(b.as_array().is_some_and(|a| a.iter().any(|x| (x["angle_deg"].as_f64().unwrap_or(0.0) - 180.0).abs() < 1e-9)), "{b}");
    // A tray: four flanges at once stop at each other's bends (square corner relief).
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Sq", "thickness": "2.5 mm", "k_factor": "0.44", "bend_radius": "Thickness", "corner_relief_shape": "square", "active": true}));
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 60, 2.5], [100, 30, 2.5], [50, 0, 2.5], [0, 30, 2.5]], "height": 25}));
    let f = flat(&mut s);
    // Oracle 69: 100 − 10 + 2·5.655 + 2·20 = 141.310 across.
    assert!((f["flat_size_mm"][0].as_f64().unwrap_or(0.0) - (90.0 + 2.0 * FRAC_PI_2 * 3.6 + 40.0)).abs() < 1e-6, "{f}");
    let v = volume(&mut s);
    // Each flange runs on past its bend toward the corner until the inner corners are the
    // rule's gap apart (diagonally): 5 − (2.5 + 2.5/√2) = 0.732 at each of 8 ends.
    let section = PI / 4.0 * (25.0 - 6.25) + 2.5 * 20.0;
    let ext = 5.0 - (2.5 + 2.5 / std::f64::consts::SQRT_2);
    let want = 90.0 * 50.0 * 2.5 + section * (2.0 * 90.0 + 2.0 * 50.0) + 8.0 * ext * 2.5 * 20.0;
    assert!(rel(v, want) < 1e-4 && rel(v, 29666.234) < 1e-4, "{v} {want}");
    // The same tray with laser-weld corner relief adds the V extensions at each of the 8 ends.
    let mut s2 = Session::default();
    run(&mut s2, "sheet.manage_rules", json!({"name": "LW", "thickness": "2.5 mm", "k_factor": "0.44", "bend_radius": "Thickness", "corner_relief_shape": "laser_weld", "active": true}));
    base(&mut s2, 100.0, 60.0);
    run(&mut s2, "sheet.flange", json!({"edges": [[50, 60, 2.5], [100, 30, 2.5], [50, 0, 2.5], [0, 30, 2.5]], "height": 25}));
    assert!(volume(&mut s2) > v, "laser-weld relief adds V extensions: {} vs {}", volume(&mut s2), v);
}

#[test]
fn unfold_cut_refold_and_convert() {
    // Oracle 74: two flanges, unfold, a slot across the bend, refold.
    let mut s = Session::default();
    base(&mut s, 80.0, 50.0);
    run(&mut s, "sheet.flange", json!({"edges": [[40, 50, 2.5], [40, 0, 2.5]], "height": 20}));
    let folded = volume(&mut s);
    run(&mut s, "sheet.unfold", json!({}));
    let m = run(&mut s, "inspect.measure", json!({}));
    assert!((m["bodies"][0]["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 2.5).abs() < 1e-6, "{m}");
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [35, -12], "p1": [45, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    run(&mut s, "sheet.refold", json!({}));
    let v = volume(&mut s);
    assert!(v < folded - 300.0, "{v} {folded}");
    assert!(rel(v, 15800.304) < 1e-4, "{v}");
    assert_eq!(flat(&mut s)["cutouts"].as_array().map(Vec::len), Some(1));
    // Convert a plate.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 2.5, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [50, 30, 2.5]}));
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    assert!(rel(volume(&mut s), 18972.622) < 1e-4, "{}", volume(&mut s));
    let mut t = Session::default();
    run(&mut t, "solid.cylinder", json!({"radius": 10, "height": 30, "body_name": "Rod"}));
    assert!(t.execute("sheet.convert", &json!({"body": "Rod", "face": [0, 0, 30]})).is_err());
}

/// A sketch line across a 100 x 60 plate's top face at x = 40.
fn fold_line(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": 2.5, "name": "FoldLine"}));
    run(s, "sketch.line", json!({"points": [[40, 0], [40, 60]], "ids": ["fl"]}));
    run(s, "sketch.finish", json!({}));
}

fn bbox(s: &mut Session) -> Value {
    run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"].clone()
}

#[test]
fn fold_takes_the_bend_out_of_the_flat_sheet() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    fold_line(&mut s);
    run(&mut s, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl"}));
    // The flat pattern keeps its size; the bend's centre line is the sketch line.
    let f = flat(&mut s);
    assert!((f["flat_size_mm"][0].as_f64().unwrap_or(0.0) - 100.0).abs() < 1e-6, "{f}");
    assert!((f["flat_size_mm"][1].as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-6, "{f}");
    assert!((f["bends"][0]["allowance"].as_f64().unwrap_or(0.0) - ba).abs() < 1e-9, "{f}");
    // Folded: the zone's flat volume BA·T·w becomes the bend's θ·T·(R + T/2)·w.
    let want = 15000.0 - ba * t * 60.0 + FRAC_PI_2 * t * (r + t / 2.0) * 60.0;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    // The smaller side (x < 40) stands up: outer face at the zone start less R + T, leg 40 − BA/2
    // tall above the bend.
    let b = bbox(&mut s);
    assert!((b["min"][0].as_f64().unwrap_or(0.0) - (40.0 + ba / 2.0 - r - t)).abs() < 1e-6, "{b}");
    assert!((b["max"][2].as_f64().unwrap_or(0.0) - (t + r + 40.0 - ba / 2.0)).abs() < 1e-6, "{b}");
    assert!((b["max"][0].as_f64().unwrap_or(0.0) - 100.0).abs() < 1e-6, "{b}");
}

#[test]
fn fold_positions_fixed_side_and_flip() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let case = |p: Value| {
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        fold_line(&mut s);
        let mut q = json!({"sketch": "FoldLine", "curve": "fl"});
        for (k, v) in p.as_object().into_iter().flatten() {
            q[k] = v.clone();
        }
        run(&mut s, "sheet.fold", q);
        bbox(&mut s)
    };
    let x0 = |b: &Value| b["min"][0].as_f64().unwrap_or(f64::NAN);
    // Start: the bend starts at the line (on the fixed side), so the folded outer face is R + T
    // short of it. End: the bend ends there. Mould: the outer face lands on the line.
    assert!((x0(&case(json!({"position": "start"}))) - (40.0 - r - t)).abs() < 1e-6);
    assert!((x0(&case(json!({"position": "end"}))) - (40.0 + ba - r - t)).abs() < 1e-6);
    assert!((x0(&case(json!({"position": "mould"}))) - 40.0).abs() < 1e-6);
    // Keep the small side: the large one (x > 40) stands up instead.
    let b = case(json!({"fixed": [10, 30, 2.5]}));
    assert!((b["max"][0].as_f64().unwrap_or(0.0) - (40.0 - ba / 2.0 + r + t)).abs() < 1e-6, "{b}");
    assert!((b["max"][2].as_f64().unwrap_or(0.0) - (t + r + 60.0 - ba / 2.0)).abs() < 1e-6, "{b}");
    // Flipped: folds down, below the sheet.
    let b = case(json!({"flip": true}));
    assert!((b["min"][2].as_f64().unwrap_or(0.0) + (r + 40.0 - ba / 2.0)).abs() < 1e-6, "{b}");
    // A 45° fold: the leg leans.
    let b = case(json!({"angle": "45 deg"}));
    assert!(b["max"][2].as_f64().unwrap_or(0.0) < t + r + 40.0 - ba / 2.0);
}

#[test]
fn fold_carries_flanges_on_the_moving_side() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    // A flange on the left edge, then a fold between it and the rest.
    run(&mut s, "sheet.flange", json!({"edges": [[0, 30, 2.5]], "height": 20}));
    let v0 = volume(&mut s);
    fold_line(&mut s);
    run(&mut s, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl", "fixed": [90, 30, 2.5]}));
    // The flange rode along: it is above the plate now, nothing left at z < 0.
    let b = bbox(&mut s);
    assert!(b["min"][2].as_f64().unwrap_or(-1.0) > -1e-6, "{b}");
    let ba = FRAC_PI_2 * (2.5 + 0.44 * 2.5);
    let want = v0 - ba * 2.5 * 60.0 + FRAC_PI_2 * 2.5 * (2.5 + 1.25) * 60.0;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    assert!(flat(&mut s)["bends"].as_array().map(Vec::len) == Some(2));
    // Errors: off the sheet, through the flange's bend.
    let mut e = Session::default();
    base(&mut e, 100.0, 60.0);
    assert!(e.execute("sheet.fold", &json!({"points": [[40, 0, 30], [40, 60, 30]]})).is_err());
    assert!(e.execute("sheet.fold", &json!({"points": [[40, 0, 2.5], [40, 60, 2.5]], "position": "sideways"})).is_err());
    assert!(e.execute("sheet.fold", &json!({"points": [[0.5, 0, 2.5], [0.5, 60, 2.5]]})).is_err(), "no material past the bend");
}

#[test]
fn fold_on_a_flange_panel() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 30}));
    let flat0 = flat(&mut s)["flat_size_mm"].clone();
    let v0 = volume(&mut s);
    // The flange stands up at the plate's y = 0 edge: its outer face is the body's min y.
    let y = bbox(&mut s)["min"][1].as_f64().unwrap_or(f64::NAN);
    run(&mut s, "sheet.fold", json!({"points": [[10, y, 18], [90, y, 18]]}));
    let f = flat(&mut s);
    for i in 0..2 {
        assert!(
            (f["flat_size_mm"][i].as_f64().unwrap_or(0.0) - flat0[i].as_f64().unwrap_or(1.0)).abs() < 1e-6,
            "the flat pattern keeps its size: {f}"
        );
    }
    assert_eq!(f["bends"].as_array().map(Vec::len), Some(2));
    let len = f["bends"][1]["length"].as_f64().unwrap_or(0.0);
    assert!(len > 90.0, "{f}");
    let want = v0 - ba * t * len + FRAC_PI_2 * t * (r + t / 2.0) * len;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    // The top part now leans over the plate (folded up, toward its top face).
    let b = bbox(&mut s);
    assert!(b["max"][2].as_f64().unwrap_or(99.0) < 25.0, "{b}");
    // Folding across the flange's own bend is refused.
    assert!(s.execute("sheet.fold", &json!({"points": [[50, y, 2], [50, y, 25]]})).is_err());
}

#[test]
fn fold_through_cut_outs() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    // A slot across the fold line (edges along and across the bend).
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 20], "p1": [50, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    let mut plain = Session::default();
    base(&mut plain, 100.0, 60.0);
    fold_line(&mut plain);
    run(&mut plain, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl"}));
    fold_line(&mut s);
    run(&mut s, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl"}));
    let f = flat(&mut s);
    assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(1), "{f}");
    let (v, vp) = (volume(&mut s), volume(&mut plain));
    // The slot takes 20 x 20 of the flat sheet; across the bend it removes the bend's share.
    assert!(v < vp - 900.0 && v > vp - 1100.0, "{v} {vp}");
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - (15000.0 - 1000.0)).abs() < 1e-3, "{}", volume(&mut s));
    // A round hole across the bend is not supported yet, and says so.
    let mut c = Session::default();
    base(&mut c, 100.0, 60.0);
    run(&mut c, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
    run(&mut c, "sketch.circle.center", json!({"center": [40, 30], "radius": 5}));
    run(&mut c, "sketch.finish", json!({}));
    run(&mut c, "solid.extrude", json!({"sketch": "Hole", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    fold_line(&mut c);
    let e = c.execute("sheet.fold", &json!({"sketch": "FoldLine", "curve": "fl"})).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("not supported yet"), "{e}");
}

#[test]
fn flange_edges_are_named_and_follow_the_base_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 100}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    run(&mut s, "sheet.flange", json!({"edges": [[100, 30, 2.5]], "height": 20, "name": "Side"}));
    let side = s.doc.find_feature("Side").cloned().unwrap();
    assert_eq!(side.edge_names, vec!["base:l2:top".to_string()]);
    let v0 = volume(&mut s);
    // Narrower base: the flange stays on the right edge (now at x = 40).
    let name = d["param"].as_str().or_else(|| d["name"].as_str()).map(str::to_string).unwrap_or_else(|| "d1".into());
    run(&mut s, "parameters.change", json!({"name": name, "expression": "40"}));
    let b = run(&mut s, "inspect.measure", json!({}))["bodies"][0]["bbox"].clone();
    let w = b["max"][0].as_f64().unwrap_or(0.0) - b["min"][0].as_f64().unwrap_or(0.0);
    assert!((w - 40.0).abs() < 1e-3 && b["max"][2].as_f64().unwrap_or(0.0) > 19.0, "{b}");
    assert!(volume(&mut s) < v0);
    let id = side.id;
    assert_eq!(s.model.result(id).and_then(|r| r.warning.clone()), None);
    // A hem on the flange's far edge is named after the flange.
    let top = b["max"][2].as_f64().unwrap_or(0.0);
    let x = b["max"][0].as_f64().unwrap_or(0.0);
    run(&mut s, "sheet.hem", json!({"edges": [[x, 30, top]], "length": 6, "name": "H"}));
    let h = s.doc.find_feature("H").cloned().unwrap();
    assert!(h.edge_names[0].starts_with(&format!("F{id}.0:far")), "{:?}", h.edge_names);
}

/// With no fixed side, the larger side stays whichever side of the line it is on (QA s22).
#[test]
fn fold_keeps_the_larger_side_either_way() {
    for (w, line, moving_right) in [(60.0, 40.0, true), (100.0, 40.0, false)] {
        let mut s = Session::default();
        base(&mut s, w, 40.0);
        run(&mut s, "sheet.fold", json!({"points": [[line, 0, 2.5], [line, 40, 2.5]]}));
        let b = bbox(&mut s);
        let (x0, x1) = (b["min"][0].as_f64().unwrap_or(0.0), b["max"][0].as_f64().unwrap_or(0.0));
        if moving_right {
            assert!(x0.abs() < 1e-6 && x1 < line + 5.0, "the left (larger) part stays: {b}");
        } else {
            assert!((x1 - w).abs() < 1e-6 && x0 > line - 5.0, "the right (larger) part stays: {b}");
        }
    }
}

#[test]
fn bend_table_exports_shop_dimensions_and_tracks_parameter_changes() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Shop, \"steel\"", "thickness": 2, "bend_radius": 3, "k_factor": 0.4}));
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20}));
    let undo = s.undo.len();
    let csv = run(&mut s, "sheet.export_bend_table", json!({}));
    let text = csv["text"].as_str().unwrap();
    assert!(text.starts_with("body,rule,thickness_mm,k_factor,bend,angle_deg,direction,"));
    assert!(text.contains("\"Shop, \"\"steel\"\"\""), "{text}");
    assert_eq!(text.lines().count(), 2);
    assert_eq!(csv["bends"], 1);
    let exported = run(&mut s, "sheet.export_bend_table", json!({"format": "JSON"}));
    let table: Value = serde_json::from_str(exported["text"].as_str().unwrap()).unwrap();
    assert_eq!(table["thickness"], 2.0);
    assert_eq!(table["bends"][0]["direction"], "up");
    assert!((table["bends"][0]["allowance"].as_f64().unwrap() - FRAC_PI_2 * 3.8).abs() < 1e-9);
    assert_eq!(s.undo.len(), undo, "export does not create an undo step");
    run(&mut s, "sheet.manage_rules", json!({"name": "Shop, \"steel\"", "k_factor": 0.5}));
    let updated = run(&mut s, "sheet.export_bend_table", json!({"format": "json"}));
    let table: Value = serde_json::from_str(updated["text"].as_str().unwrap()).unwrap();
    assert!((table["bends"][0]["allowance"].as_f64().unwrap() - FRAC_PI_2 * 4.0).abs() < 1e-9);
    assert!(s.execute("sheet.export_bend_table", &json!({"format": "xml"})).is_err());
    assert!(s.execute("sheet.export_bend_table", &json!({"body": "Missing"})).is_err());
    let mut empty = Session::default();
    assert!(empty.execute("sheet.export_bend_table", &json!({})).is_err());
}

#[test]
fn malformed_fold_points_are_rejected_atomically() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    let before = serde_json::to_value(&s.doc).unwrap();
    for points in [
        json!([[40, 0, 2.5], null, [40, 60, 2.5]]),
        json!([[40, 0, 2.5], [40, 60, 2.5], "junk"]),
        json!([[40, 0, 2.5], [40, 60]]),
        json!([[40, 0, 2.5]]),
        json!("not points"),
    ] {
        assert!(s.execute("sheet.fold", &json!({"points": points})).is_err());
        assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
    }
}

#[test]
fn flat_dxf_preserves_exact_cutout_entities() {
    use solvecraft_geom::{Loop2, Region2, Seg2, Vec2};
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    let mut sh = s.model.state().sheets[0].clone();
    sh.holes = vec![Region2 {
        outer: Loop2 { segs: vec![Seg2::Arc { center: Vec2::new(40.0, 30.0), radius: 5.0, start: -PI, sweep: -2.0 * PI }] },
        holes: Vec::new(),
    }];
    let dxf = super::flat_dxf(&sh).unwrap();
    assert_eq!(dxf.matches("0\nCIRCLE\n").count(), 1, "{dxf}");
    assert!(!dxf.contains("0\nARC\n"), "a full circle must not become a zero-length ARC");
    let read = solvecraft_io::read_dxf(&dxf).unwrap();
    assert_eq!(read.len(), 5);
    assert!(read.iter().any(|g| matches!(g, solvecraft_io::Geom2::Circle(c, r) if *c == Vec2::new(40.0, 30.0) && (*r - 5.0).abs() < 1e-9)));
    sh.holes[0].outer = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(10.0, 10.0), b: Vec2::new(30.0, 10.0) },
            Seg2::Arc { center: Vec2::new(20.0, 10.0), radius: 10.0, start: 0.0, sweep: PI },
        ],
    };
    let dxf = super::flat_dxf(&sh).unwrap();
    assert_eq!(dxf.matches("0\nLINE\n").count(), 5, "four outline lines and one exact cutout line");
    assert_eq!(dxf.matches("0\nARC\n").count(), 1);
    assert!(dxf.contains("50\n0.000000\n51\n180.000000\n"));
    let read = solvecraft_io::read_dxf(&dxf).unwrap();
    assert_eq!(read.len(), 6);
    assert!(read.iter().any(|g| matches!(g, solvecraft_io::Geom2::Arc { c, a, b } if *c == Vec2::new(20.0, 10.0) && a.dist(Vec2::new(30.0, 10.0)) < 1e-9 && b.dist(Vec2::new(10.0, 10.0)) < 1e-9)));
}

// A profile chosen by a point outside its inner loop must keep that inner loop through
// sheet construction, flattening and manufacturing export.
#[test]
fn inherited_base_profile_holes_survive_every_sheet_state() {
    for flip in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Perforated"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 20], "p1": [60, 40]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Perforated", "profiles": [{"point": [10, 10]}], "flip": flip}));
        assert!((volume(&mut s) - 14000.0).abs() < 1e-6);
        let f = flat(&mut s);
        assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(1));
        let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
        assert_eq!(solvecraft_io::read_dxf(&dxf).unwrap().len(), 8);
        let mut reopened = Session::default();
        run(&mut reopened, "sketch.insert_dxf", json!({"text": dxf, "name": "Blank"}));
        run(&mut reopened, "sketch.finish", json!({}));
        let pick = if flip { [10, -10] } else { [10, 10] };
        run(&mut reopened, "solid.extrude", json!({"sketch": "Blank", "profiles": [{"point": pick}], "distance": 2.5}));
        assert!((volume(&mut reopened) - 14000.0).abs() < 1e-5);
        run(&mut s, "sheet.unfold", json!({}));
        assert!((volume(&mut s) - 14000.0).abs() < 1e-6);
        run(&mut s, "sheet.refold", json!({}));
        assert!((volume(&mut s) - 14000.0).abs() < 1e-6);
    }
}

#[test]
fn edge_notch_uses_the_same_material_boundary_in_solid_flat_and_dxf() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Notch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-5, 20], "p1": [10, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Notch", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    assert!((volume(&mut s) - 14500.0).abs() < 1e-6);
    let f = flat(&mut s);
    assert_eq!(f["outline"].as_array().map(Vec::len), Some(1));
    assert_eq!(f["outline"][0].as_array().map(Vec::len), Some(8));
    assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(0));
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    assert_eq!(solvecraft_io::read_dxf(&dxf).unwrap().len(), 8);
    let mut reopened = Session::default();
    run(&mut reopened, "sketch.insert_dxf", json!({"text": dxf, "name": "Blank"}));
    run(&mut reopened, "sketch.finish", json!({}));
    run(&mut reopened, "solid.extrude", json!({"sketch": "Blank", "distance": 2.5}));
    assert!((volume(&mut reopened) - 14500.0).abs() < 1e-5);
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - 14500.0).abs() < 1e-6);
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - 14500.0).abs() < 1e-6);
}

#[test]
fn inherited_round_hole_survives_flange_and_unfold_in_flipped_frames() {
    for flip in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Perforated"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut s, "sketch.circle.center", json!({"center": [50, 30], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Perforated", "profiles": [{"point": [10, 10]}], "flip": flip}));
        run(&mut s, "sheet.flange", json!({"edges": [[50, 0, if flip { -2.5 } else { 2.5 }]], "height": 20}));
        let folded = volume(&mut s);
        assert!((folded - (18972.62196081845 - PI * 25.0 * 2.5)).abs() < 0.2, "{folded}");
        let f = flat(&mut s);
        assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(1));
        let sh = s.model.state().sheets[0].clone();
        let regions = sh.flat_regions().unwrap();
        assert_eq!(regions[0].holes[0].segs.len(), 2);
        assert!(regions[0].holes[0].segs.iter().all(|s| matches!(s, solvecraft_geom::Seg2::Arc { radius, .. } if (*radius - 5.0).abs() < 1e-9)));
        let expected = regions.iter().map(solvecraft_geom::Region2::area).sum::<f64>() * 2.5;
        run(&mut s, "sheet.unfold", json!({}));
        assert!((volume(&mut s) - expected).abs() < 0.1);
        run(&mut s, "sheet.refold", json!({}));
        assert!((volume(&mut s) - folded).abs() < 0.1);
    }
}

#[test]
fn crossing_only_disconnecting_cut_preserves_all_material_states() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Split"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [45, -5], "p1": [55, 65]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Split", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    assert!((volume(&mut s) - 13500.0).abs() < 1e-6);
    assert_eq!(flat(&mut s)["outline"].as_array().map(Vec::len), Some(2));
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    let mut reopened = Session::default();
    run(&mut reopened, "sketch.insert_dxf", json!({"text": dxf, "name": "SplitBlank"}));
    run(&mut reopened, "sketch.finish", json!({}));
    run(&mut reopened, "solid.extrude", json!({"sketch": "SplitBlank", "distance": 2.5}));
    assert!((volume(&mut reopened) - 13500.0).abs() < 1e-6);
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - 13500.0).abs() < 1e-6);
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - 13500.0).abs() < 1e-6);
}

#[test]
fn splitting_an_unfolded_flanged_sheet_preserves_both_bent_pieces() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sheet.flange", json!({"edges": [[50,0,2.5]], "height": 20}));
    let folded = volume(&mut s);
    run(&mut s, "sheet.unfold", json!({}));
    let unfolded = volume(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "BlankSplit"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [45,-100], "p1": [55,100]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "BlankSplit", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    assert!((volume(&mut s) - unfolded * 0.9).abs() < 0.1);
    assert_eq!(flat(&mut s)["outline"].as_array().map(Vec::len), Some(2));
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - folded * 0.9).abs() < 0.2);
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - unfolded * 0.9).abs() < 0.1);
}

#[test]
fn repeated_and_nested_sheet_cuts_are_redundant() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    for (i, radius) in [3.0, 5.0, 5.0].into_iter().enumerate() {
        let name = format!("Hole{i}");
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(&mut s, "sketch.circle.center", json!({"center": [20, 30], "radius": radius}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": name, "through_all": true, "operation": "cut", "direction": "symmetric"}));
    }
    assert!((volume(&mut s) - (6000.0 - PI * 25.0) * 2.5).abs() < 0.1);
    assert_eq!(flat(&mut s)["cutouts"].as_array().map(Vec::len), Some(1));
}

#[test]
fn overlapping_round_cuts_keep_exact_union_in_export_and_roundtrip() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    for (i, x) in [20.0, 22.0].into_iter().enumerate() {
        let name = format!("Overlap{i}");
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(&mut s, "sketch.circle.center", json!({"center": [x, 30], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": name, "through_all": true, "operation": "cut", "direction": "symmetric"}));
    }
    let intersection = 50.0 * (0.2f64).acos() - (96.0f64).sqrt();
    let expected = (6000.0 - (50.0 * PI - intersection)) * 2.5;
    assert!((volume(&mut s) - expected).abs() < 0.1);
    assert_eq!(flat(&mut s)["cutouts"].as_array().map(Vec::len), Some(1));
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    assert!(dxf.contains("0\nARC\n"));
    let mut reopened = Session::default();
    run(&mut reopened, "sketch.insert_dxf", json!({"text": dxf, "name": "Blank"}));
    run(&mut reopened, "sketch.finish", json!({}));
    run(&mut reopened, "solid.extrude", json!({"sketch": "Blank", "profiles": [{"point": [10, 10]}], "distance": 2.5}));
    assert!((volume(&mut reopened) - expected).abs() < 0.1);
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - expected).abs() < 0.1);
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - expected).abs() < 0.1);
}

#[test]
fn cut_across_a_concave_notch_uses_crossings_not_just_contained_corners() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "U"}));
    run(&mut s, "sketch.line", json!({"points": [[0,0],[100,0],[100,60],[70,60],[70,20],[30,20],[30,60],[0,60],[0,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "U"}));
    assert!((volume(&mut s) - 11000.0).abs() < 1e-6);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "ArmNotches"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20,30], "p1": [80,40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "ArmNotches", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    assert!((volume(&mut s) - 10500.0).abs() < 1e-6);
    let f = flat(&mut s);
    assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(0));
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - 10500.0).abs() < 1e-6);
}

#[test]
fn tiny_cut_inside_exact_circle_survives_unfold_refold_and_dxf() {
    for initially_flat in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Disc"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0,0], "radius": 10}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Disc"}));
        let circle = s.model.state().sheets[0].base.outer.clone();
        let displayed = circle.polyline(1e-3);
        let at = (displayed[0] + displayed[1]).normalized().unwrap() * 9.9998;
        assert!(!circle.contains(at));
        if initially_flat {
            run(&mut s, "sheet.unfold", json!({}));
        }
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "TinyCut"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [at.x-0.000025,at.y-0.000025], "p1": [at.x+0.000025,at.y+0.000025]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "TinyCut", "through_all": true, "operation": "cut", "direction": "symmetric"}));
        for stage in ["sheet.unfold", "sheet.refold"] {
            let regions = s.model.state().sheets[0].flat_regions().unwrap();
            assert_eq!(regions[0].holes.len(), 1);
            assert!((regions[0].holes[0].signed_area().abs() - 2.5e-9).abs() < 1e-13);
            assert_eq!(s.model.state().bodies.len(), 1);
            let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
            assert_eq!(solvecraft_io::read_dxf(&dxf).unwrap().len(), 5);
            if stage == "sheet.unfold" && initially_flat {
                run(&mut s, "sheet.refold", json!({}));
            }
            run(&mut s, stage, json!({}));
        }
        assert_eq!(s.model.state().sheets[0].flat_regions().unwrap()[0].holes.len(), 1);
    }
}

#[test]
fn unrelated_box_cut_preserves_logical_ring_sheet_compound() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    let sheet_name = s.model.state().sheets[0].body.clone();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ring"}));
    for radius in [10, 5] {
        run(&mut s, "sketch.circle.center", json!({"center": [50,30], "radius": radius}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Ring", "profiles": [{"point": [57,30]}], "through_all": true, "operation": "cut", "direction": "symmetric"}),
    );
    let material = s.model.state().sheets[0].flat_regions().unwrap();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [200,0,0], "body_name": "SeparateBox"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "BoxCut"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [205,5], "p1": [210,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "BoxCut", "participants": ["SeparateBox"], "through_all": true, "operation": "cut", "direction": "symmetric"}),
    );
    assert_eq!(s.model.state().bodies.len(), 2);
    assert_eq!(s.model.state().sheets[0].flat_regions().unwrap(), material);
    for stage in ["sheet.unfold", "sheet.refold"] {
        run(&mut s, stage, json!({"body": sheet_name}));
        assert_eq!(s.model.state().bodies.len(), 2);
        let state = s.model.state();
        let body = &state.bodies.iter().find(|b| b.name == sheet_name).unwrap().body;
        assert!(body.contains(solvecraft_geom::Vec3::new(50.0, 30.0, 1.25)));
        assert!(!body.contains(solvecraft_geom::Vec3::new(57.0, 30.0, 1.25)));
    }
}

#[test]
fn specialized_sheet_cuts_respect_component_and_explicit_participants() {
    for explicit in [false, true] {
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        let name = s.model.state().sheets[0].body.clone();
        run(&mut s, "component.create", json!({"name": "Other"}));
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "OtherBox"}));
        if explicit {
            run(&mut s, "component.activate", json!({"component": "root"}));
        }
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20,20], "p1": [30,30]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut params = json!({"sketch": "Cut", "through_all": true, "operation": "cut", "direction": "symmetric"});
        if explicit {
            params["participants"] = json!(["OtherBox"]);
        }
        run(&mut s, "solid.extrude", params);
        let m = run(&mut s, "inspect.measure", json!({"bodies": [name]}));
        assert!((m["total"]["volume_mm3"].as_f64().unwrap() - 15000.0).abs() < 1e-6);
        assert!(s.model.state().sheets[0].holes.is_empty());
        let m = run(&mut s, "inspect.measure", json!({"bodies": ["OtherBox"]}));
        assert!((m["total"]["volume_mm3"].as_f64().unwrap() - 59000.0).abs() < 1e-6);
    }
}

#[test]
fn ring_cut_preserves_retained_island_through_unfold_and_refold() {
    fn material_probes(s: &Session) {
        let body = &s.model.state().bodies[0].body;
        assert!(body.contains(solvecraft_geom::Vec3::new(50.0, 30.0, 1.25)));
        assert!(!body.contains(solvecraft_geom::Vec3::new(57.0, 30.0, 1.25)));
        assert!(body.contains(solvecraft_geom::Vec3::new(65.0, 30.0, 1.25)));
    }
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ring"}));
    run(&mut s, "sketch.circle.center", json!({"center": [50,30], "radius": 10}));
    run(&mut s, "sketch.circle.center", json!({"center": [50,30], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Ring", "profiles": [{"point": [57,30]}], "through_all": true, "operation": "cut", "direction": "symmetric"}),
    );
    let expected = (6000.0 - PI * 75.0) * 2.5;
    assert!((volume(&mut s) - expected).abs() < 0.2);
    material_probes(&s);
    assert_eq!(flat(&mut s)["outline"].as_array().map(Vec::len), Some(2));
    let regions = s.model.state().sheets[0].flat_regions().unwrap();
    assert!(regions.iter().any(|r| r.contains(solvecraft_geom::Vec2::new(50.0, 30.0))));
    assert!(!regions.iter().any(|r| r.contains(solvecraft_geom::Vec2::new(57.0, 30.0))));
    assert!(regions.iter().any(|r| r.contains(solvecraft_geom::Vec2::new(65.0, 30.0))));
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    let mut reopened = Session::default();
    run(&mut reopened, "sketch.insert_dxf", json!({"text": dxf, "name": "RingBlank"}));
    run(&mut reopened, "sketch.finish", json!({}));
    run(&mut reopened, "solid.extrude", json!({"sketch": "RingBlank", "profiles": [{"point": [10,10]}, {"point": [50,30]}], "distance": 2.5}));
    assert!((volume(&mut reopened) - expected).abs() < 0.2);
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - expected).abs() < 0.2);
    material_probes(&s);
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - expected).abs() < 0.2);
    material_probes(&s);
}

fn retained_depth_check(s: &mut Session, want: f64, z0: f64, z1: f64) {
    use solvecraft_geom::Vec3;
    assert!((volume(s) - want).abs() < 1e-6, "{} vs {want}", volume(s));
    let state = s.model.state();
    assert_eq!(state.sheets.len(), 1);
    assert_eq!(state.sheets[0].t, 2.5, "rule stock gauge is preserved");
    assert!(state.bodies.iter().any(|b| b.body.contains(Vec3::new(25.0, 25.0, (z0 + z1) * 0.5))));
    assert!(!state.bodies.iter().any(|b| b.body.contains(Vec3::new(25.0, 25.0, z0 - 0.1))));
    assert!(!state.bodies.iter().any(|b| b.body.contains(Vec3::new(25.0, 25.0, z1 + 0.1))));
    assert!(!state.bodies.iter().any(|b| b.body.contains(Vec3::new(10.0, 25.0, (z0 + z1) * 0.5))));
    let bounds = state.bodies[0].mesh().bounds();
    assert!((bounds.min.z - z0).abs() < 1e-8 && (bounds.max.z - z1).abs() < 1e-8, "{bounds:?}");
}

fn depth_step_reopen(s: &mut Session, suffix: &str, want: f64, inside: solvecraft_geom::Vec3, outside: solvecraft_geom::Vec3) {
    let path = std::env::temp_dir().join(format!("solvecraft-depth-intersect-{suffix}-{}.step", std::process::id()));
    run(s, "file.export", json!({"path":path.to_string_lossy()}));
    let mut imported = Session::default();
    run(&mut imported, "doc.open", json!({"path":path.to_string_lossy()}));
    assert!((volume(&mut imported) - want).abs() < 1e-6);
    assert!(imported.model.state().bodies.iter().any(|b| b.body.contains(inside)));
    assert!(!imported.model.state().bodies.iter().any(|b| b.body.contains(outside)));
    std::fs::remove_file(path).unwrap();
}

/// The generic sketch importer intentionally drops DXF layers/depth. Reimport only the
/// material outline at a separately known depth, never pretend all entities reconstruct3D.
fn dxf_outline_only(text: &str) -> String {
    let lines: Vec<_> = text.lines().collect();
    let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().filter_map(|p| Some((p.first()?.trim(), *p.get(1)?))).collect();
    let mut out = String::from("0\nSECTION\n2\nENTITIES\n");
    let mut start = 0;
    for end in 1..=pairs.len() {
        if end == pairs.len() || pairs.get(end).is_some_and(|p| p.0 == "0") {
            let entity = &pairs[start..end];
            if entity.first().is_some_and(|p| matches!(p.1, "LINE" | "ARC" | "CIRCLE")) && entity.iter().any(|p| p.0 == "8" && p.1 == "OUTLINE") {
                for (code, value) in entity {
                    out.push_str(&format!("{code}\n{value}\n"));
                }
            }
            start = end;
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    out
}

#[test]
fn partial_depth_intersect_retains_bottom_top_and_middle_material_through_history_and_exports() {
    use solvecraft_geom::Vec3;
    for (offset, z0, z1, layers) in [
        (0.0, 0.0, 1.0, vec![("top", 1.0, 2.5)]),
        (1.5, 1.5, 2.5, vec![("bottom", 0.0, 1.5)]),
        (0.5, 0.5, 1.5, vec![("bottom", 0.0, 0.5), ("top", 1.5, 2.5)]),
    ] {
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        rectangle_tool(&mut s, "RetainDepth", [20.0, 20.0], [30.0, 30.0]);
        run(&mut s, "solid.extrude", json!({"sketch":"RetainDepth","distance":1,"start_offset":offset,"operation":"intersect","name":"RetainSlab"}));
        retained_depth_check(&mut s, 100.0, z0, z1);
        let exported = flat(&mut s);
        assert_eq!(exported["thickness"], 2.5);
        assert_eq!(exported["pockets"].as_array().unwrap().len(), layers.len());
        let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
        for (side, a, b) in &layers {
            assert!(
                exported["pockets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["side"] == *side && p["z0_mm"] == *a && p["z1_mm"] == *b && p["depth_mm"] == b - a)
            );
            assert!(dxf.contains(&format!("POCKET_{}_Z{a:.6}_TO_{b:.6}_MM", side.to_uppercase())));
        }
        let outline = dxf_outline_only(&dxf);
        assert_eq!(solvecraft_io::read_dxf(&outline).unwrap().len(), 4);
        let mut outline_import = Session::default();
        run(&mut outline_import, "sketch.insert_dxf", json!({"text":outline,"name":"Outline"}));
        run(&mut outline_import, "sketch.finish", json!({}));
        run(&mut outline_import, "solid.extrude", json!({"distance":1}));
        assert!((volume(&mut outline_import) - 100.0).abs() < 1e-6);
        run(&mut s, "edit.undo", json!({}));
        assert!((volume(&mut s) - 15000.0).abs() < 1e-6);
        run(&mut s, "edit.redo", json!({}));
        for command in ["sheet.unfold", "sheet.refold"] {
            run(&mut s, command, json!({}));
            retained_depth_check(&mut s, 100.0, z0, z1);
            assert_eq!(flat(&mut s)["pockets"], exported["pockets"]);
        }
        let mut reopened = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        retained_depth_check(&mut reopened, 100.0, z0, z1);
        depth_step_reopen(&mut s, &format!("offset{offset}"), 100.0, Vec3::new(25.0, 25.0, (z0 + z1) * 0.5), Vec3::new(25.0, 25.0, z1 + 0.1));
    }
}

#[test]
fn partial_depth_intersect_preserves_prior_pockets_and_actual_interval_intersections() {
    use solvecraft_geom::Vec3;
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "OldPocket", [20.0, 20.0], [25.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"OldPocket","distance":1,"operation":"cut"}));
    rectangle_tool(&mut s, "RetainDepth", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"RetainDepth","distance":1,"start_offset":0.5,"operation":"intersect"}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        assert!((volume(&mut s) - 75.0).abs() < 1e-6);
        let state = s.model.state();
        let body = &state.bodies[0].body;
        assert!(!body.contains(Vec3::new(22.0, 25.0, 0.75)));
        assert!(body.contains(Vec3::new(22.0, 25.0, 1.25)));
        assert!(body.contains(Vec3::new(27.0, 25.0, 0.75)));
        assert!(!body.contains(Vec3::new(27.0, 25.0, 0.25)));
    }
    let records = s.model.state().sheets[0].pockets.clone();
    run(&mut s, "solid.extrude", json!({"sketch":"RetainDepth","distance":1.5,"start_offset":0.25,"operation":"intersect"}));
    assert!((volume(&mut s) - 75.0).abs() < 1e-6);
    assert_eq!(s.model.state().sheets[0].pockets, records, "no-change leaves replay work unchanged");
    run(&mut s, "solid.extrude", json!({"sketch":"RetainDepth","distance":0.25,"start_offset":1.0,"operation":"intersect"}));
    assert!((volume(&mut s) - 25.0).abs() < 1e-6);
    run(&mut s, "sheet.unfold", json!({}));
    assert!((volume(&mut s) - 25.0).abs() < 1e-6);
    let mut cold = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((volume(&mut cold) - 25.0).abs() < 1e-6);
    depth_step_reopen(&mut s, "prior", 25.0, Vec3::new(22.0, 25.0, 1.125), Vec3::new(22.0, 25.0, 0.75));
}

#[test]
fn partial_depth_intersect_keeps_disconnected_tool_islands_and_annular_curves() {
    use solvecraft_geom::Vec3;
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Islands", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "sketch.edit", json!({"sketch":"Islands"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[60,20],"p1":[70,30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Islands","distance":1,"operation":"intersect"}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        assert!((volume(&mut s) - 200.0).abs() < 1e-6);
        assert_eq!(s.model.state().sheets[0].flat_regions().unwrap().len(), 2);
        for x in [25.0, 65.0] {
            assert!(s.model.state().bodies[0].body.contains(Vec3::new(x, 25.0, 0.5)));
        }
        assert!(!s.model.state().bodies[0].body.contains(Vec3::new(45.0, 25.0, 0.5)));
    }
    depth_step_reopen(&mut s, "islands", 200.0, Vec3::new(65.0, 25.0, 0.5), Vec3::new(45.0, 25.0, 0.5));
    let mut ring = Session::default();
    base(&mut ring, 100.0, 60.0);
    run(&mut ring, "sketch.create", json!({"plane":"XY","name":"Ring"}));
    for radius in [10.0, 5.0] {
        run(&mut ring, "sketch.circle.center", json!({"center":[50,30],"radius":radius}));
    }
    run(&mut ring, "sketch.finish", json!({}));
    run(&mut ring, "solid.extrude", json!({"sketch":"Ring","profiles":[{"point":[57,30]}],"distance":1,"operation":"intersect"}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut ring, command, json!({}));
        }
        let state = ring.model.state();
        let area: f64 = state.sheets[0].flat_regions().unwrap().iter().map(|r| r.area()).sum();
        assert!((area - 75.0 * PI).abs() < 1e-8);
        assert!((volume(&mut ring) - 75.0 * PI).abs() < 0.1);
        assert!(state.bodies[0].body.contains(Vec3::new(57.0, 30.0, 0.5)));
        assert!(!state.bodies[0].body.contains(Vec3::new(50.0, 30.0, 0.5)));
        assert!(!state.bodies[0].body.contains(Vec3::new(57.0, 30.0, 1.5)));
    }
    let dxf = run(&mut ring, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
    assert!(dxf.contains("POCKET_TOP_Z1.000000_TO_2.500000_MM"));
    assert_eq!(solvecraft_io::read_dxf(&dxf_outline_only(&dxf)).unwrap().len(), 2);
}

#[test]
fn partial_depth_intersect_deletes_material_already_removed_by_a_pocket_atomically() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "Removed", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Removed","distance":1,"operation":"cut"}));
    run(&mut s, "solid.extrude", json!({"sketch":"Removed","distance":1,"operation":"intersect"}));
    assert!(s.model.state().bodies.is_empty());
    assert!(s.model.state().sheets.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - 14900.0).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert!(s.model.state().bodies.is_empty());
}

#[test]
fn partial_depth_intersect_flipped_tool_and_transferred_sheet_keep_depth_side_and_world_pose() {
    use solvecraft_geom::Vec3;
    let mut flipped = Session::default();
    base(&mut flipped, 100.0, 60.0);
    run(&mut flipped, "sketch.create", json!({"name":"FlippedTool","plane":{"origin":[0,0,2.5],"x_dir":[1,0,0],"y_dir":[0,-1,0]}}));
    run(&mut flipped, "sketch.rectangle.two_point", json!({"p0":[20,-30],"p1":[30,-20]}));
    run(&mut flipped, "sketch.finish", json!({}));
    run(&mut flipped, "solid.extrude", json!({"sketch":"FlippedTool","distance":1,"operation":"intersect"}));
    retained_depth_check(&mut flipped, 100.0, 1.5, 2.5);
    assert_eq!(flat(&mut flipped)["pockets"][0]["side"], "bottom");
    assert_eq!(flat(&mut flipped)["pockets"][0]["depth_mm"], 1.5);
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    rectangle_tool(&mut s, "RetainDepth", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"RetainDepth","distance":1,"start_offset":0.5,"operation":"intersect","name":"RetainSlab"}));
    let before = s.world_state().bodies[0].mesh().bounds();
    let name = s.model.state().sheets[0].body.clone();
    let destination = run(&mut s, "component.create", json!({"name":"Destination","parent":0}));
    let owner = destination["component"].as_u64().unwrap();
    let occurrence = destination["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,20,10],"axis":[1,0,0],"angle":"90 deg","capture":true}));
    run(&mut s, "component.move_bodies", json!({"component":owner,"bodies":[name]}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({}));
        }
        let state = s.model.state();
        let sh = &state.sheets[0];
        assert_eq!(sh.t, 2.5);
        assert!(sh.frame.origin.dist(Vec3::new(-30.0, -10.0, 20.0)) < 1e-8);
        assert!(sh.frame.normal().dist(Vec3::Y) < 1e-8);
        assert_eq!(sh.pockets.len(), 2);
        assert_eq!(state.body(&name).unwrap().component, owner);
        assert!(state.body(&name).unwrap().transferred);
        let world = s.world_state();
        let body = &world.body(&name).unwrap().body;
        assert!(body.contains(Vec3::new(25.0, 25.0, 1.0)));
        assert!(!body.contains(Vec3::new(25.0, 25.0, 0.25)));
        assert!(!body.contains(Vec3::new(25.0, 25.0, 2.0)));
        let bounds = world.body(&name).unwrap().mesh().bounds();
        assert!(before.min.dist(bounds.min) < 1e-8 && before.max.dist(bounds.max) < 1e-8);
        assert!((solvecraft_kernel::measure(body).unwrap().volume - 100.0).abs() < 1e-6);
    }
    let mut reopened = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let reopened_world = reopened.world_state();
    let body = &reopened_world.body(&name).unwrap().body;
    assert!(body.contains(Vec3::new(25.0, 25.0, 1.0)));
    assert!((solvecraft_kernel::measure(body).unwrap().volume - 100.0).abs() < 1e-6);
    run(&mut reopened, "timeline.edit", json!({"feature":"RetainSlab","set":{"extent":{"distance":"1.5"}}}));
    let world = reopened.world_state();
    let body = &world.body(&name).unwrap().body;
    assert!((solvecraft_kernel::measure(body).unwrap().volume - 150.0).abs() < 1e-6);
    assert!(body.contains(Vec3::new(25.0, 25.0, 1.75)));
    assert!(!body.contains(Vec3::new(25.0, 25.0, 2.25)));
}

#[test]
fn partial_depth_intersect_full_gauge_in_rotated_frames_does_not_invent_pockets() {
    use solvecraft_geom::Vec3;
    for (angle, translate) in [(37.0, [30.0, 20.0, 10.0]), (121.0, [12345.678, 98765.432, 4321.123])] {
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        let name = s.model.state().sheets[0].body.clone();
        let destination = run(&mut s, "component.create", json!({"name":"Destination","parent":0}));
        let owner = destination["component"].as_u64().unwrap();
        run(
            &mut s,
            "occurrence.move",
            json!({"occurrence":destination["occurrence"],"translate":translate,"axis":[1,2,3],"angle":format!("{angle} deg"),"capture":true}),
        );
        run(&mut s, "component.move_bodies", json!({"component":owner,"bodies":[name]}));
        run(&mut s, "component.activate", json!({"component":owner}));
        run(&mut s, "sketch.create", json!({"name":"WorldTool","plane":{"origin":[0,0,0],"x_dir":[1,0,0],"y_dir":[0,1,0]}}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[20,20],"p1":[30,30]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut tool_session = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        run(&mut tool_session, "solid.extrude", json!({"sketch":"WorldTool","distance":2.5,"body_name":"CapWitness"}));
        let witness_state = tool_session.model.state();
        let witness = &witness_state.body("CapWitness").unwrap().body;
        let tool_plane = witness_state.sketches.last().unwrap().plane;
        let sources = witness.face_sources().unwrap();
        assert_eq!(sources.len(), witness.face_count());
        let mut cap_count = 0;
        for (index, source) in sources.iter().enumerate() {
            if let Some(solvecraft_kernel::FaceSource::CertifiedExtrudeCap { high, plane, .. }) = source {
                cap_count += 1;
                let current = witness.planar_face(index).unwrap();
                assert!(current.normal().cross(tool_plane.normal()).len() < 1e-10);
                assert!((current.origin - plane.origin).dot(tool_plane.normal()).abs() < 1e-8);
                assert!(((plane.origin - tool_plane.origin).dot(tool_plane.normal()) - if *high { 2.5 } else { 0.0 }).abs() < 1e-8);
            }
        }
        assert_eq!(cap_count, 2);
        run(&mut s, "solid.extrude", json!({"sketch":"WorldTool","distance":2.5,"operation":"intersect","name":"PostTransferRetain"}));
        for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
            if let Some(command) = command {
                run(&mut s, command, json!({}));
            }
            assert!(s.model.state().sheets[0].pockets.is_empty(), "full gauge must not acquire roundoff-depth pockets");
            let world = s.world_state();
            let body = &world.body(&name).unwrap().body;
            assert!((solvecraft_kernel::measure(body).unwrap().volume - 250.0).abs() < 1e-5);
            assert!(body.contains(Vec3::new(25.0, 25.0, 0.1)));
            assert!(body.contains(Vec3::new(25.0, 25.0, 2.4)));
            assert!(!body.contains(Vec3::new(10.0, 25.0, 1.0)));
        }
        let mut cold = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!(cold.model.state().sheets[0].pockets.is_empty());
        assert!(flat(&mut cold)["pockets"].as_array().unwrap().is_empty());
        run(&mut s, "timeline.edit", json!({"feature":"PostTransferRetain","set":{"extent":{"distance":"1","start_offset":"0.5"}}}));
        for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
            if let Some(command) = command {
                run(&mut s, command, json!({}));
            }
            assert_eq!(s.model.state().sheets[0].pockets.len(), 2);
            let world = s.world_state();
            let body = &world.body(&name).unwrap().body;
            let measured = solvecraft_kernel::measure(body).unwrap().volume;
            assert!((measured - 100.0).abs() < 1e-5, "retained volume {measured}");
            assert!(body.contains(Vec3::new(25.0, 25.0, 1.0)));
            assert!(!body.contains(Vec3::new(25.0, 25.0, 0.1)));
            assert!(!body.contains(Vec3::new(25.0, 25.0, 2.4)));
        }
        let cold = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        let world = cold.world_state();
        let body = &world.body(&name).unwrap().body;
        assert!((solvecraft_kernel::measure(body).unwrap().volume - 100.0).abs() < 1e-5);
        assert!(body.contains(Vec3::new(25.0, 25.0, 1.0)));
        assert!(!body.contains(Vec3::new(25.0, 25.0, 0.1)));
    }
}

#[test]
fn partial_depth_intersect_planar_to_object_retains_current_target_depth() {
    use solvecraft_geom::Vec3;
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    let name = s.model.state().sheets[0].body.clone();
    run(&mut s, "solid.box", json!({"corner":[200,0,1.5],"length":10,"width":10,"height":1,"body_name":"Roof"}));
    rectangle_tool(&mut s, "Tool", [20.0, 20.0], [30.0, 30.0]);
    run(&mut s, "solid.extrude", json!({"sketch":"Tool","to":[205,5,1.5],"start_offset":0.5,"operation":"intersect","participants":[name]}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({"body":name}));
        }
        let state = s.model.state();
        let body = &state.body(&name).unwrap().body;
        assert!((solvecraft_kernel::measure(body).unwrap().volume - 100.0).abs() < 1e-6);
        assert!(body.contains(Vec3::new(25.0, 25.0, 1.0)));
        assert!(!body.contains(Vec3::new(25.0, 25.0, 0.1)));
        assert!(!body.contains(Vec3::new(25.0, 25.0, 2.0)));
        assert_eq!(state.sheets[0].pockets.len(), 2);
        assert!((solvecraft_kernel::measure(&state.body("Roof").unwrap().body).unwrap().volume - 100.0).abs() < 1e-6);
    }
    let cold = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((solvecraft_kernel::measure(&cold.model.state().body(&name).unwrap().body).unwrap().volume - 100.0).abs() < 1e-6);
}

#[test]
fn partial_depth_intersect_curved_to_object_refuses_unrepresentable_depth_atomically() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    let name = s.model.state().sheets[0].body.clone();
    run(&mut s, "solid.cylinder", json!({"base":[25,20,12],"axis":[0,1,0],"radius":11,"height":10,"body_name":"CurvedRoof"}));
    rectangle_tool(&mut s, "Tool", [24.0, 23.0], [26.0, 27.0]);
    let before = s.doc.to_json();
    let error =
        s.execute("solid.extrude", &json!({"sketch":"Tool","to":[36,25,12],"operation":"intersect","participants":[name]})).unwrap_err().to_string();
    assert!(error.contains("sheet machining"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({"body":name}));
        }
        let state = s.model.state();
        assert!((solvecraft_kernel::measure(&state.body(&name).unwrap().body).unwrap().volume - 15000.0).abs() < 1e-6);
        assert!(state.sheets[0].pockets.is_empty());
    }
}

#[test]
fn partial_depth_intersect_curved_to_object_preserves_current_trimmed_cap_footprint() {
    use solvecraft_geom::Vec3;
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    let name = s.model.state().sheets[0].body.clone();
    run(&mut s, "solid.cylinder", json!({"base":[25,25,0],"axis":[0,0,1],"radius":2,"height":20,"body_name":"CurvedRoof"}));
    run(&mut s, "solid.box", json!({"corner":[19,19,1],"length":12,"width":12,"height":1,"body_name":"StopPlate"}));
    run(&mut s, "solid.combine", json!({"target":"CurvedRoof","tools":["StopPlate"],"operation":"join"}));
    rectangle_tool(&mut s, "Tool", [20.0, 20.0], [30.0, 30.0]);
    let mut tool_session = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut tool_session, "solid.extrude", json!({"sketch":"Tool","to":[27,25,10],"body_name":"TrimWitness"}));
    let ts = tool_session.model.state();
    let witness = &ts.body("TrimWitness").unwrap().body;
    assert!((solvecraft_kernel::measure(witness).unwrap().volume - (100.0 - 4.0 * std::f64::consts::PI)).abs() < 0.01);
    assert!(witness.contains(Vec3::new(21.0, 21.0, 0.5)));
    assert!(!witness.contains(Vec3::new(25.0, 25.0, 0.5)));
    run(&mut s, "solid.extrude", json!({"sketch":"Tool","to":[27,25,10],"operation":"intersect","participants":[name]}));
    for command in [None, Some("sheet.unfold"), Some("sheet.refold")] {
        if let Some(command) = command {
            run(&mut s, command, json!({"body":name}));
        }
        let state = s.model.state();
        let body = &state.body(&name).unwrap().body;
        let measured = solvecraft_kernel::measure(body).unwrap().volume;
        assert!((measured - (100.0 - 4.0 * std::f64::consts::PI)).abs() < 0.01);
        let area: f64 = state.sheets[0].flat_regions().unwrap().iter().map(|r| r.area()).sum();
        assert!((area - (100.0 - 4.0 * std::f64::consts::PI)).abs() < 1e-8);
        assert!(body.contains(Vec3::new(21.0, 21.0, 0.5)));
        assert!(!body.contains(Vec3::new(25.0, 25.0, 0.5)));
        assert!(!body.contains(Vec3::new(21.0, 21.0, 1.5)));
    }
    let cold = Session::new(solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(!cold.model.state().body(&name).unwrap().body.contains(Vec3::new(25.0, 25.0, 0.5)));
    let dxf = run(&mut s, "sheet.export_dxf", json!({"body":name}))["dxf"].as_str().unwrap().to_string();
    assert!(dxf.contains("POCKET_TOP_Z1.000000_TO_2.500000_MM"));
    let outline = dxf_outline_only(&dxf);
    assert_eq!(solvecraft_io::read_dxf(&outline).unwrap().len(), 5);
    let mut outline_import = Session::default();
    run(&mut outline_import, "sketch.insert_dxf", json!({"text":outline,"name":"Outline"}));
    run(&mut outline_import, "sketch.finish", json!({}));
    run(&mut outline_import, "solid.extrude", json!({"distance":1,"profiles":[{"point":[21,21]}]}));
    assert!((volume(&mut outline_import) - (100.0 - 4.0 * std::f64::consts::PI)).abs() < 0.01);
    assert!(!outline_import.model.state().bodies[0].body.contains(Vec3::new(25.0, 25.0, 0.5)));
    let path = std::env::temp_dir().join(format!("solvecraft-current-trim-cap-{}.step", std::process::id()));
    run(&mut s, "file.export", json!({"path":path.to_string_lossy(),"bodies":[name]}));
    let mut imported = Session::default();
    run(&mut imported, "doc.open", json!({"path":path.to_string_lossy()}));
    assert!((volume(&mut imported) - (100.0 - 4.0 * std::f64::consts::PI)).abs() < 0.01);
    assert!(!imported.model.state().bodies[0].body.contains(Vec3::new(25.0, 25.0, 0.5)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn partial_depth_intersect_sub_extrusion_resolution_layers_refuse_atomically() {
    for (distance, offset) in [(2.5 - 1e-8, 1e-8), (2.5 - 1e-8, 0.0), (1e-8, 0.0)] {
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        let name = s.model.state().sheets[0].body.clone();
        let destination = run(&mut s, "component.create", json!({"name":"Far","parent":0}));
        let owner = destination["component"].as_u64().unwrap();
        run(
            &mut s,
            "occurrence.move",
            json!({"occurrence":destination["occurrence"],"translate":[12345.678,98765.432,4321.123],"axis":[1,2,3],"angle":"121 deg","capture":true}),
        );
        run(&mut s, "component.move_bodies", json!({"component":owner,"bodies":[name]}));
        run(&mut s, "component.activate", json!({"component":owner}));
        run(&mut s, "sketch.create", json!({"name":"Tool","plane":{"origin":[0,0,0],"x_dir":[1,0,0],"y_dir":[0,1,0]}}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[20,20],"p1":[30,30]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = s.doc.to_json();
        let error = s
            .execute("solid.extrude", &json!({"sketch":"Tool","distance":distance,"start_offset":offset,"operation":"intersect"}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("resolution") || error.contains("distance is zero") || error.contains("sheet machining"), "{error}");
        assert_eq!(s.doc.to_json(), before);
        assert!(s.model.state().sheets[0].pockets.is_empty());
        assert!((solvecraft_kernel::measure(&s.world_state().body(&name).unwrap().body).unwrap().volume - 15000.0).abs() < 1e-5);
    }
}

/// A 0.8 mm steel rule and a base flange from a closed outline on XY (the sheet spans z 0..0.8).
fn outline_blank(s: &mut Session, outline: Value) {
    run(s, "sheet.manage_rules", json!({"name": "Steel 0.8", "thickness": "0.8", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(s, "sketch.line", json!({"points": outline, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Blank"}));
}

/// A 100 x 40 blank with two 15 mm tabs on its right edge (y 5..12 and y 28..35), on one bend line.
fn two_tab_blank(s: &mut Session) {
    outline_blank(
        s,
        json!([[0, 0], [100, 0], [100, 5], [115, 5], [115, 12], [100, 12], [100, 28], [115, 28], [115, 35], [100, 35], [100, 40], [0, 40]]),
    );
}

/// Folding one tab must not be refused because another tab lies on the same line (#264).
#[test]
fn fold_one_of_two_collinear_tabs() {
    let mut s = Session::default();
    two_tab_blank(&mut s);
    let flat = volume(&mut s);
    run(&mut s, "sheet.fold", json!({"points": [[107, 5, 0.8], [107, 12, 0.8]], "fixed": [50, 20, 0.8]}));
    assert!(rel(volume(&mut s), flat) < 1e-3, "a fold keeps the material");
}

/// One fold line across both tabs bends both (#264).
#[test]
fn fold_across_two_collinear_tabs() {
    let mut s = Session::default();
    two_tab_blank(&mut s);
    let flat = volume(&mut s);
    run(&mut s, "sheet.fold", json!({"points": [[107, 5, 0.8], [107, 35, 0.8]], "fixed": [50, 20, 0.8]}));
    assert!(rel(volume(&mut s), flat) < 1e-3);
    let bb = &run(&mut s, "inspect.measure", json!({}))["bodies"][0]["bbox"];
    assert!(bb["max"][2].as_f64().unwrap() > 7.0, "both tabs stand up: {bb}");
}

/// Edge flanges on both tab ends in one feature (two separate features already work) (#264).
#[test]
fn edge_flanges_on_two_collinear_tab_ends_in_one_feature() {
    let mut s = Session::default();
    two_tab_blank(&mut s);
    run(&mut s, "sheet.flange", json!({"edges": [[115, 8, 0.8], [115, 31, 0.8]], "height": "10"}));
    let mut separate = Session::default();
    two_tab_blank(&mut separate);
    run(&mut separate, "sheet.flange", json!({"edges": [[115, 8, 0.8]], "height": "10"}));
    run(&mut separate, "sheet.flange", json!({"edges": [[115, 31, 0.8]], "height": "10"}));
    assert!(rel(volume(&mut s), volume(&mut separate)) < 1e-6);
}

/// `fixed` inside a cut-out touches no material: the fold must refuse it or say which side it kept (#263).
#[test]
fn fold_fixed_point_off_the_sheet_is_not_silently_ignored() {
    let mut s = Session::default();
    outline_blank(&mut s, json!([[0, 0], [100, 0], [100, 60], [0, 60]]));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "2", "name": "Window"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 15], "p1": [40, 45]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Window", "distance": "4", "direction": "negative", "operation": "cut"}));
    match s.execute("sheet.fold", &json!({"points": [[80, 0, 0.8], [80, 60, 0.8]], "fixed": [25, 30, 0.8]})) {
        Err(e) => assert!(e.to_string().contains("fixed"), "{e}"),
        Ok(r) => {
            let warned =
                ["warning", "warnings"].iter().any(|k| r.get(*k).is_some_and(|w| !w.is_null() && w != &json!([]) && w.to_string().contains("fixed")));
            assert!(warned, "fixed [25, 30] is inside the window, yet the fold succeeded silently: {r}");
        }
    }
}

/// A tab whose fold line sits exactly at its root: the bend zone reaches the blank outline beside
/// the tab. Every edge is straight, so the error must not blame a slanted or curved edge (#262).
#[test]
fn fold_at_a_tab_root_names_the_real_cause() {
    let mut s = Session::default();
    outline_blank(&mut s, json!([[0, 0], [100, 0], [100, 50], [60, 50], [60, 70], [40, 70], [40, 50], [0, 50]]));
    if let Err(e) = s.execute("sheet.fold", &json!({"points": [[40, 50, 0.8], [60, 50, 0.8]], "fixed": [50, 25, 0.8]})) {
        let e = e.to_string();
        assert!(!e.contains("slanted or curved"), "all edges are straight: {e}");
        assert!(e.contains("bend zone"), "name the bend zone reaching the outline: {e}");
    }
    // 1 mm into the tab the same fold works.
    let mut ok = Session::default();
    outline_blank(&mut ok, json!([[0, 0], [100, 0], [100, 50], [60, 50], [60, 70], [40, 70], [40, 50], [0, 50]]));
    run(&mut ok, "sheet.fold", json!({"points": [[40, 51, 0.8], [60, 51, 0.8]], "fixed": [50, 25, 0.8]}));
}
