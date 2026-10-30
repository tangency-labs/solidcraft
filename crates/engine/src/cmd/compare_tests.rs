use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn regions(r: &Value) -> Vec<Value> {
    r["regions"].as_array().cloned().unwrap_or_default()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn close(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

/// A 40 x 30 x 10 box `A` and its copy `A (2)`, in the same place.
fn two_boxes() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.move", json!({"bodies": ["A"], "translate": [0, 0, 0], "copy": true}));
    s
}

#[test]
fn a_box_and_its_copy_are_identical() {
    let mut s = two_boxes();
    let r = run(&mut s, "inspect.compare", json!({"a": "A", "b": "A (2)"}));
    assert_eq!(r["identical"], true, "{r}");
    assert_eq!(r["warnings"], json!([]), "{r}");
    assert!(regions(&r).is_empty());
    assert!(f(&r["deviation"]["max_mm"]) < 1e-9, "{}", r["deviation"]);
    assert_eq!(f(&r["difference"]["volume_mm3"]), 0.0);
    assert!(close(f(&r["a"]["volume_mm3"]), 12_000.0, 1e-9));
    assert!(r["verdict"].as_str().unwrap_or_default().starts_with("identical within 0.01 mm"), "{}", r["verdict"]);
    // Without names: the design's only two bodies. Comparing changes nothing.
    let before = s.doc.clone();
    let r = run(&mut s, "inspect.compare", json!({}));
    assert_eq!(r["identical"], true);
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc));
}

#[test]
fn a_pocket_is_material_only_the_plain_box_has() {
    // b has a 10 x 8 x 0.5 pocket in its top: 40 mm³ that only a has.
    let mut s = two_boxes();
    run(&mut s, "solid.box", json!({"length": 10, "width": 8, "height": 0.5, "corner": [10, 10, 9.5], "operation": "cut", "targets": ["A (2)"]}));
    let r = run(&mut s, "inspect.compare", json!({"a": "A", "b": "A (2)"}));
    let rs = regions(&r);
    assert_eq!(rs.len(), 1, "{r}");
    let g = &rs[0];
    assert_eq!(g["side"], "only_in_a");
    assert_eq!(g["material_of"], "A");
    // Measured exactly, on the cut a − b: the pocket's own volume, box and centre.
    assert_eq!(g["measured_by"], "boolean", "{g}");
    assert!(close(f(&g["volume_mm3"]), 40.0, 1e-9), "{g}");
    assert!((f(&g["max_deviation_mm"]) - 0.5).abs() < 1e-6, "{g}");
    let (lo, hi) = (&g["bbox"]["min"], &g["bbox"]["max"]);
    for (k, (want_lo, want_hi)) in [(10.0, 20.0), (10.0, 18.0), (9.5, 10.0)].into_iter().enumerate() {
        assert!((f(&lo[k]) - want_lo).abs() < 1e-6 && (f(&hi[k]) - want_hi).abs() < 1e-6, "{g}");
    }
    let c = &g["center"];
    assert!((f(&c[0]) - 15.0).abs() < 1e-9 && (f(&c[1]) - 14.0).abs() < 1e-9 && (f(&c[2]) - 9.75).abs() < 1e-9, "{g}");
    // The pocket's floor and walls, and a's top.
    assert_eq!(g["faces_b"].as_array().map(Vec::len), Some(5), "{g}");
    assert_eq!(g["faces_a"].as_array().map(Vec::len), Some(1), "{g}");
    assert!(close(f(&r["difference"]["volume_mm3"]), -40.0, 1e-6), "{}", r["difference"]);
    assert!(close(f(&r["only_in_a"]["volume_mm3"]), 40.0, 1e-9));
    assert_eq!(r["only_in_b"]["regions"], 0);
    // The other way round, the same material is only in b.
    let r = run(&mut s, "inspect.compare", json!({"a": "A (2)", "b": "A"}));
    assert_eq!(regions(&r).len(), 1);
    assert_eq!(regions(&r)[0]["side"], "only_in_b");
    assert_eq!(regions(&r)[0]["material_of"], "A");
}

#[test]
fn a_boss_is_material_only_the_box_with_it_has() {
    // b has a 6 x 5 x 2 boss on its top: 60 mm³ that only b has.
    let mut s = two_boxes();
    run(&mut s, "solid.box", json!({"length": 6, "width": 5, "height": 2, "corner": [5, 5, 10], "operation": "join", "targets": ["A (2)"]}));
    let r = run(&mut s, "inspect.compare", json!({"a": "A", "b": "A (2)", "color": true}));
    let rs = regions(&r);
    assert_eq!(rs.len(), 1, "{r}");
    assert_eq!(rs[0]["side"], "only_in_b");
    assert!(close(f(&rs[0]["volume_mm3"]), 60.0, 1e-9), "{}", rs[0]);
    assert!((f(&rs[0]["max_deviation_mm"]) - 2.0).abs() < 1e-6);
    assert!(close(f(&r["difference"]["volume_mm3"]), 60.0, 1e-6));
    // Coloured green: the boss's top and four sides, all of them beyond the tolerance; not a's top,
    // of which only the boss's footprint is. One undo step.
    assert_eq!(r["colored_faces"], 5, "{r}");
    let green = |s: &Session, body: &str| s.doc.appearances.faces.iter().filter(|x| x.body == body && x.look.color == [0x2f, 0x9e, 0x44]).count();
    assert_eq!((green(&s, "A (2)"), green(&s, "A")), (5, 0));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.appearances.faces.len(), 0);
    // Own colours.
    let r = run(&mut s, "inspect.compare", json!({"color": {"only_in_b": "#0000ff"}}));
    assert_eq!(r["colored_faces"], 5);
    assert_eq!(s.doc.appearances.faces.iter().filter(|x| x.look.color == [0, 0, 255]).count(), 5);
}

#[test]
fn a_taller_box_has_a_slab_only_it_has() {
    // b is 5 mm taller on the same footprint: b − a is the slab z 20..25, 40 x 30 x 5 = 6000 mm³
    // at (20, 15, 22.5), sharing four side faces and the bottom with a. Exact, from the cut.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 25, "body_name": "B"}));
    let r = run(&mut s, "inspect.compare", json!({"a": "A", "b": "B"}));
    let rs = regions(&r);
    assert_eq!(rs.len(), 1, "{r}");
    let g = &rs[0];
    assert_eq!((g["side"].as_str(), g["measured_by"].as_str()), (Some("only_in_b"), Some("boolean")), "{g}");
    assert!(close(f(&g["volume_mm3"]), 6000.0, 1e-9), "{g}");
    let c = &g["center"];
    assert!((f(&c[0]) - 20.0).abs() < 1e-9 && (f(&c[1]) - 15.0).abs() < 1e-9 && (f(&c[2]) - 22.5).abs() < 1e-9, "{g}");
    assert!((f(&g["bbox"]["min"][2]) - 20.0).abs() < 1e-6 && (f(&g["bbox"]["max"][2]) - 25.0).abs() < 1e-6, "{g}");
    assert!(close(f(&r["only_in_b"]["volume_mm3"]), 6000.0, 1e-9), "{r}");
    assert_eq!(f(&r["only_in_a"]["volume_mm3"]), 0.0);
    assert_eq!(r["warnings"], json!([]), "{r}");
}

/// A 100 x 60 sheet-metal plate with a 20 mm flange along y = 0 and a 5 mm hole at (x, 40).
fn bracket(s: &mut Session, name: &str, x: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "name": format!("{name} base")}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": format!("{name} base"), "body_name": name}));
    run(s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "body": name}));
    run(s, "sketch.create", json!({"plane": "XY", "name": format!("{name} hole")}));
    run(s, "sketch.circle.center", json!({"center": [x, 40], "diameter": 5}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": format!("{name} hole"), "through_all": true, "operation": "cut", "targets": [name]}));
}

#[test]
fn a_hole_moved_2_mm_leaves_two_crescents() {
    let mut s = Session::default();
    bracket(&mut s, "Before", 50.0);
    bracket(&mut s, "After", 52.0);
    let r = run(&mut s, "inspect.compare", json!({"a": "Before", "b": "After"}));
    let rs = regions(&r);
    assert_eq!(rs.len(), 2, "{r}");
    // Each is a disk of 2.5 mm radius less its overlap with the other, centres 2 mm apart, through
    // the 2.5 mm sheet: (π r² − 2 r² acos(d / 2r) + (d / 2) √(4 r² − d²)) · t.
    let (rr, d, t) = (2.5f64, 2.0f64, 2.5);
    let crescent = (std::f64::consts::PI * rr * rr - 2.0 * rr * rr * (d / (2.0 * rr)).acos() + d / 2.0 * (4.0 * rr * rr - d * d).sqrt()) * t;
    assert!((crescent - 24.316_34).abs() < 1e-4);
    for g in &rs {
        assert_eq!(g["measured_by"], "boolean", "{g}");
        assert!(close(f(&g["volume_mm3"]), crescent, 1e-6), "{g}");
        let c = &g["center"];
        assert!((f(&c[1]) - 40.0).abs() < 1e-6 && (f(&c[2]) - 1.25).abs() < 1e-6, "{g}");
        // Material only Before has is where After's hole is (beyond x = 51), and the reverse.
        match g["side"].as_str() {
            Some("only_in_a") => assert!(f(&c[0]) > 51.5, "{g}"),
            Some("only_in_b") => assert!(f(&c[0]) < 50.5, "{g}"),
            _ => panic!("{g}"),
        }
    }
    assert_ne!(rs[0]["side"], rs[1]["side"]);
    assert!(f(&r["difference"]["volume_mm3"]).abs() < 1e-3, "{}", r["difference"]);
    // Coloured: the hole walls, not the plate's faces, of which only the crescents deviate.
    let r = run(&mut s, "inspect.compare", json!({"a": "Before", "b": "After", "color": true}));
    let n = r["colored_faces"].as_u64().unwrap_or(0) as usize;
    assert!((2..=8).contains(&n), "{r}");
    let looks = &s.doc.appearances.faces;
    assert_eq!(looks.len(), n);
    for l in looks {
        assert!(((l.point.x - 51.0).powi(2) + (l.point.y - 40.0).powi(2)).sqrt() < 4.0, "{:?}", l.point);
    }
}

#[test]
fn a_part_and_its_step_round_trips_are_identical() {
    // Curved faces too: a hole and a rounded edge.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 8, "height": 10, "base": [20, 15, 0], "operation": "cut", "targets": ["Part"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 10]], "radius": 3}));
    let dir = std::env::temp_dir().join(format!("solidcraft-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.step");
    run(&mut s, "file.export", json!({"path": path, "bodies": ["Part"]}));
    let mut inserted = Vec::new();
    for _ in 0..2 {
        let r = run(&mut s, "file.insert_step", json!({"path": path}));
        inserted.push(r["created_bodies"][0].as_str().unwrap().to_string());
    }
    let _ = std::fs::remove_dir_all(&dir);
    for (a, b) in [("Part", inserted[0].as_str()), (inserted[0].as_str(), inserted[1].as_str())] {
        let r = run(&mut s, "inspect.compare", json!({"a": a, "b": b}));
        assert_eq!(r["identical"], true, "{a} vs {b}: {r}");
        assert!(f(&r["deviation"]["max_mm"]) < 0.005, "{a} vs {b}: {}", r["deviation"]);
        assert!(f(&r["difference"]["volume_mm3"]).abs() < 0.05, "{a} vs {b}: {}", r["difference"]);
    }
}

#[test]
fn bad_parameters_are_refused() {
    let mut s = two_boxes();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "C"}));
    for p in [
        json!({}),
        json!({"a": "A"}),
        json!({"a": "A", "b": "A"}),
        json!({"a": "A", "b": "nope"}),
        json!({"a": "A", "b": 3}),
        json!({"a": "A", "b": "C", "tolerance": 0}),
        json!({"a": "A", "b": "C", "tolerance": -1}),
        json!({"a": "A", "b": "C", "tolerance": "a lot"}),
        json!({"a": "A", "b": "C", "tolerance": 1e300}),
        json!({"a": "A", "b": "C", "spacing": 0}),
        json!({"a": "A", "b": "C", "max_regions": 0}),
        json!({"a": "A", "b": "C", "max_points": 10}),
        json!({"a": "A", "b": "C", "color": "blue"}),
        json!({"a": "A", "b": "C", "color": {"only_in_a": "#12345"}}),
    ] {
        assert!(s.execute("inspect.compare", &p).is_err(), "{p}");
    }
    // Two selected bodies are the pair.
    s.selection = vec![crate::Sel::Body { name: "A".into() }, crate::Sel::Body { name: "C".into() }];
    let r = run(&mut s, "inspect.compare", json!({}));
    assert_eq!(r["a"]["name"], "A");
    assert_eq!(r["b"]["name"], "C");
    assert_eq!(r["only_in_a"]["regions"], 1, "{r}");
    assert!(close(f(&r["only_in_a"]["volume_mm3"]), 12_000.0 - 125.0, 1e-9), "{r}");
    assert!(r["verdict"].as_str().unwrap_or_default().starts_with("1 region differs by more than 0.01 mm"), "{}", r["verdict"]);
}
