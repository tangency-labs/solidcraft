//! Thin Extrude (#721): walls along sketch curves, their volumes worked out by hand (a wall's
//! area times its height).

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

fn body_count(s: &mut Session) -> u64 {
    run(s, "inspect.measure", json!({}))["body_count"].as_u64().unwrap_or(0)
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{a} != {b}");
}

/// A sketch on XY holding a 40 × 20 rectangle from (5, 5); returns its side ids.
fn rectangle(s: &mut Session) -> Vec<String> {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    let r = run(s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [45, 25]}));
    run(s, "sketch.finish", json!({}));
    ["bottom", "right", "top", "left"].iter().map(|k| r["sides"][k].as_str().unwrap_or_default().to_string()).collect()
}

#[test]
fn a_rectangle_makes_a_ring_wall_on_either_side() {
    for (side, extra, area) in [
        ("center", json!({}), 42.0 * 22.0 - 38.0 * 18.0),
        ("inside", json!({}), 40.0 * 20.0 - 36.0 * 16.0),
        ("outside", json!({}), 44.0 * 24.0 - 40.0 * 20.0),
        ("two_sides", json!({"thickness2": 1.5}), 43.0 * 23.0 - 39.0 * 19.0),
    ] {
        let mut s = Session::default();
        rectangle(&mut s);
        let mut p = json!({"sketch": "Outline", "thin": true, "thickness": 2, "side": side, "distance": 10});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        if side == "two_sides" {
            p["thickness"] = json!(0.5);
        }
        run(&mut s, "solid.extrude", p);
        close(volume(&mut s), area * 10.0);
        assert_eq!(body_count(&mut s), 1, "{side}");
    }
}

#[test]
fn the_thickness_is_a_parameter_and_the_wall_survives_reopening() {
    let mut s = Session::default();
    rectangle(&mut s);
    let r = run(&mut s, "solid.extrude", json!({"sketch": "Outline", "thin": true, "thickness": 2, "distance": 10, "name": "Wall"}));
    assert!(r["created_bodies"].as_array().is_some_and(|b| b.len() == 1));
    close(volume(&mut s), 240.0 * 10.0);
    let f = s.doc.features.iter().find(|f| f.name == "Wall").cloned().unwrap();
    let t = f.param_names.iter().zip(f.kind.inputs()).find(|(_, (label, _, _))| *label == "Thickness").map(|(n, _)| n.clone()).unwrap();
    run(&mut s, "parameters.change", json!({"name": t, "expression": "4"}));
    close(volume(&mut s), (44.0 * 24.0 - 36.0 * 16.0) * 10.0);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    close(volume(&mut reopened), (44.0 * 24.0 - 36.0 * 16.0) * 10.0);
    // A plain extrude of the same sketch fills the rectangle.
    let mut s = Session::default();
    rectangle(&mut s);
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "distance": 10}));
    close(volume(&mut s), 800.0 * 10.0);
}

#[test]
fn chosen_curves_make_an_open_wall_that_joins_and_cuts() {
    // Three sides of the rectangle: a U, inside toward the open side's middle.
    let mut s = Session::default();
    let sides = rectangle(&mut s);
    let u = [&sides[0], &sides[1], &sides[2]];
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "thin": true, "curves": u, "thickness": 2, "side": "inside", "distance": 5}));
    // 40 × 2 twice and 20 × 2, less the two 2 × 2 corners they share.
    close(volume(&mut s), (80.0 + 40.0 + 80.0 - 8.0) * 5.0);
    // Cut a groove 3 deep along the whole outline into a block under it.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 30, "height": 10, "corner": [0, 0, -10], "body_name": "Block"}));
    rectangle(&mut s);
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Outline", "thin": true, "thickness": 2, "distance": 3, "direction": "negative", "operation": "cut"}),
    );
    close(volume(&mut s), 15000.0 - 240.0 * 3.0);
    assert_eq!(body_count(&mut s), 1);
    // And join a rim on top of it.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 30, "height": 10, "corner": [0, 0, -10], "body_name": "Block"}));
    rectangle(&mut s);
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "thin": true, "thickness": 2, "distance": 4, "operation": "auto"}));
    close(volume(&mut s), 15000.0 + 240.0 * 4.0);
    assert_eq!(body_count(&mut s), 1);
}

#[test]
fn a_ring_and_a_cross_of_walls_are_one_body() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Target"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(&mut s, "sketch.line", json!({"points": [[-20, 0], [20, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, -20], [0, 20]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Target", "thin": true, "thickness": 1, "distance": 5}));
    assert_eq!(body_count(&mut s), 1);
    // The ring, plus the bars inside it less where they cross and where they run into it.
    let ring = std::f64::consts::PI * (20.5f64.powi(2) - 19.5f64.powi(2));
    let v = volume(&mut s);
    assert!(v > (ring + 2.0 * 38.0 - 1.0) * 5.0 && v < (ring + 2.0 * 40.0) * 5.0, "{v}");
}

#[test]
fn bad_thin_walls_are_refused_and_change_nothing() {
    let mut s = Session::default();
    rectangle(&mut s);
    let before = s.doc.to_json();
    for (p, says) in [
        (json!({"thin": true, "thickness": 2, "distance": 5, "curves": ["nope"]}), "no curve"),
        (json!({"thin": true, "distance": 5}), "thickness"),
        (json!({"thin": true, "thickness": 2, "distance": 5, "side": "two_sides"}), "thickness2"),
        (json!({"thin": true, "thickness": 2, "thickness2": 1, "distance": 5}), "two_sides"),
        (json!({"thin": true, "thickness": 2, "distance": 5, "side": "sideways"}), "unknown side"),
        (json!({"thin": true, "thickness": 2, "distance": 5, "profiles": [0]}), "profiles"),
        (json!({"thin": true, "thickness": 0, "distance": 5}), "thickness"),
        (json!({"thin": true, "thickness": -1, "distance": 5}), "thickness"),
        (json!({"thin": "yes", "thickness": 1, "distance": 5}), "`thin`"),
    ] {
        let mut p = p;
        p["sketch"] = json!("Outline");
        let e = s.execute("solid.extrude", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(s.doc.to_json(), before);
    }
    // A wall thicker than an arc it follows bends.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Small"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 1}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.extrude", &json!({"sketch": "Small", "thin": true, "thickness": 3, "side": "inside", "distance": 5}));
    assert!(e.is_err_and(|e| e.to_string().contains("thicker")));
    run(&mut s, "solid.extrude", json!({"sketch": "Small", "thin": true, "thickness": 3, "side": "outside", "distance": 5}));
    // Measured from the tessellation, within its 1e-4.
    let (v, exact) = (volume(&mut s), std::f64::consts::PI * (16.0 - 1.0) * 5.0);
    assert!((v - exact).abs() < 1e-4 * exact, "{v} vs {exact}");
}

#[test]
fn hostile_thin_params_never_panic() {
    let keys = ["curves", "thickness", "thickness2", "side", "distance", "taper", "direction", "to", "operation"];
    for v in crate::tests::hostile_values().into_iter().chain([json!(1e9), json!(1e-9), json!("1e6 m"), json!(["l1", "l1", "l1"])]) {
        for k in keys {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
            rectangle(&mut s);
            let mut p = json!({"sketch": "Outline", "thin": true, "thickness": 2, "side": "two_sides", "thickness2": 1, "distance": 5});
            p[k] = v.clone();
            if let Err(crate::EngineError::Internal(id, msg)) = s.execute("solid.extrude", &p) {
                panic!("{id} {p}: {msg}");
            }
        }
    }
}

// #1519: sketches on a thin extrude's caps. A thin wall's caps are bounded by offset curves the
// sketch doesn't have, so they are certified by the curves the wall follows: the signature the
// solid caps had when the wall follows the whole profile.

fn attachment(s: &Session, sketch: &str) -> solidcraft_doc::FaceAttachmentStatus {
    let id = s.doc.find_feature(sketch).map(|f| f.id).unwrap_or_default();
    s.placed_document().sketch_face_attachment(id, &s.model)
}

fn attached(s: &Session, sketch: &str) -> (solidcraft_geom::Plane, Option<solidcraft_doc::config::FaceCertificate>) {
    match attachment(s, sketch) {
        solidcraft_doc::FaceAttachmentStatus::Healthy { plane, target, .. } => (plane, target.certificate),
        other => panic!("{sketch}: {other:?}"),
    }
}

fn same_plane(a: &solidcraft_geom::Plane, b: &solidcraft_geom::Plane) {
    let near = |p: solidcraft_geom::Vec3, q: solidcraft_geom::Vec3| (p - q).len() < 1e-9;
    assert!(near(a.origin, b.origin) && near(a.x, b.x) && near(a.y, b.y), "{a:?} != {b:?}");
}

fn sketch_on(s: &mut Session, at: [f64; 3], name: &str) {
    run(s, "sketch.create", json!({"plane": {"face": at}, "name": name}));
    run(s, "sketch.finish", json!({}));
}

fn is_extrude_cap(certificate: &Option<solidcraft_doc::config::FaceCertificate>) -> bool {
    matches!(certificate, Some(solidcraft_doc::config::FaceCertificate::ExtrudeCap { .. }))
}

#[test]
fn sketches_on_a_solid_extrudes_caps_keep_their_faces_when_it_is_made_thin_and_solid_again() {
    for side in ["center", "inside", "outside"] {
        let mut s = Session::default();
        let sides = rectangle(&mut s);
        run(&mut s, "solid.extrude", json!({"sketch": "Outline", "distance": 10, "name": "Wall"}));
        // On the rim of the wall whichever side it lies on (the rectangle starts at (5, 5)).
        sketch_on(&mut s, [5.5, 5.5, 10.0], "Top");
        sketch_on(&mut s, [5.5, 5.5, 0.0], "Bottom");
        let (top, _) = attached(&s, "Top");
        let (bottom, _) = attached(&s, "Bottom");
        run(&mut s, "timeline.edit", json!({"feature": "Wall", "set": {"thin": {"curves": sides, "thickness": "1", "side": side}}}));
        no_errors(&s, side);
        let (thin_top, certificate) = attached(&s, "Top");
        assert!(is_extrude_cap(&certificate), "{side}: {certificate:?}");
        same_plane(&thin_top, &top);
        same_plane(&attached(&s, "Bottom").0, &bottom);
        // The sketch follows the thin wall's height, and keeps its face through a save.
        run(&mut s, "timeline.edit", json!({"feature": "Wall", "set": {"extent": {"distance": "25"}}}));
        assert!((attached(&s, "Top").0.origin.z - 25.0).abs() < 1e-9, "{side}");
        let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
        reopened.refresh();
        no_errors(&reopened, side);
        assert!((attached(&reopened, "Top").0.origin.z - 25.0).abs() < 1e-9, "{side}");
        // And solid again.
        run(&mut s, "timeline.edit", json!({"feature": "Wall", "set": {"thin": null, "extent": {"distance": "10"}}}));
        no_errors(&s, side);
        same_plane(&attached(&s, "Top").0, &top);
        same_plane(&attached(&s, "Bottom").0, &bottom);
    }
}

#[test]
fn a_sketch_on_a_thin_cap_is_certified_and_keeps_its_face_when_the_extrude_is_made_solid() {
    let mut s = Session::default();
    rectangle(&mut s);
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "thin": true, "thickness": 2, "side": "inside", "distance": 10, "name": "Wall"}));
    sketch_on(&mut s, [6.0, 6.0, 10.0], "Top");
    let (top, certificate) = attached(&s, "Top");
    assert!(is_extrude_cap(&certificate), "{certificate:?}");
    run(&mut s, "timeline.edit", json!({"feature": "Wall", "set": {"thin": null}}));
    no_errors(&s, "solid");
    same_plane(&attached(&s, "Top").0, &top);
}

#[test]
fn a_thin_wall_along_part_of_the_profile_does_not_take_over_its_cap() {
    // A wall along two sides is another wall than the rectangle's: the sketch reports its lost
    // face instead of moving onto the new one.
    let mut s = Session::default();
    let sides = rectangle(&mut s);
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "distance": 10, "name": "Wall"}));
    sketch_on(&mut s, [6.0, 6.0, 10.0], "Top");
    let _ = s.execute("timeline.edit", &json!({"feature": "Wall", "set": {"thin": {"curves": [&sides[0], &sides[1]], "thickness": "2"}}}));
    assert!(matches!(attachment(&s, "Top"), solidcraft_doc::FaceAttachmentStatus::Lost { .. }), "{:?}", attachment(&s, "Top"));
}

#[test]
fn the_caps_of_walls_apart_attach_by_their_history_names() {
    // Two rectangles make two walls: their caps aren't certified (one signature would name
    // both), so a sketch on one attaches by its exact history name, as before.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [30, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "thin": true, "thickness": 1, "side": "inside", "distance": 5}));
    sketch_on(&mut s, [0.5, 0.5, 5.0], "Top");
    let (_, certificate) = attached(&s, "Top");
    assert!(matches!(certificate, Some(solidcraft_doc::config::FaceCertificate::Lineage)), "{certificate:?}");
}

fn no_errors(s: &Session, context: &str) {
    assert!(
        s.model.results.iter().all(|r| r.error.is_none()),
        "{context}: {:?}",
        s.model.results.iter().filter_map(|r| r.error.as_ref()).collect::<Vec<_>>()
    );
}

/// How far a face's plane lies from the origin along its own normal.
fn plane_offset(plane: &solidcraft_geom::Plane) -> f64 {
    plane.normal().dot(plane.origin)
}

#[test]
fn the_walls_of_an_open_thin_extrude_take_sketches_that_follow_their_thickness() {
    // Three lines make an open wall; its sides lie off the sketch lines (a hair either side of
    // each), so no sketch curve is under them. Each side still has a name its history can tell
    // apart, so a sketch on it attaches and follows the wall when the thickness changes.
    for side in ["center", "inside", "outside"] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
        let lines = run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 30], [0, 30]], "closed": false}));
        run(&mut s, "sketch.finish", json!({}));
        let curves = lines["created"]["curves"].clone();
        let thin = |t: &str| json!({"feature": "Wall", "set": {"thin": {"curves": curves, "thickness": t, "side": side}}});
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch": "Outline", "thin": true, "curves": curves, "thickness": "2", "side": side, "distance": 20, "name": "Wall"}),
        );
        no_errors(&s, side);
        let picks = flat_side_faces(&s);
        assert!(picks.len() >= 6, "{side}: {picks:?}");
        let mut moved_by_one = 0;
        for (k, (label, at)) in picks.iter().enumerate() {
            let name = format!("S{k}");
            let mut t = s.scratch();
            sketch_on(&mut t, *at, &name);
            let (plane, _) = attached(&t, &name);
            // Twice as thick, then back: the sketch stays on its face.
            run(&mut t, "timeline.edit", thin("4"));
            no_errors(&t, &format!("{side} {label}"));
            let (thick, _) = attached(&t, &name);
            if side == "center" {
                // A side of a centred wall is half the thickness off its line: 1 -> 2 mm. The
                // wall's two ends stay where they are.
                let moved = (plane_offset(&thick).abs() - plane_offset(&plane).abs()).abs();
                assert!(moved < 1e-6 || (moved - 1.0).abs() < 1e-6, "{label}: moved {moved}");
                moved_by_one += usize::from(moved > 0.5);
            }
            let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&t.doc.to_json()).unwrap().into(), ..Session::default() };
            reopened.refresh();
            no_errors(&reopened, side);
            same_plane(&attached(&reopened, &name).0, &thick);
        }
        if side == "center" {
            assert_eq!(moved_by_one, 6, "two sides of each of the three lines move with the thickness");
        }
    }
}

/// A point inside each flat, vertical face of the first body: (what it is, point).
fn flat_side_faces(s: &Session) -> Vec<(String, [f64; 3])> {
    let world = s.world_state();
    let mut out = Vec::new();
    let Some(body) = world.bodies.first() else { return out };
    let mesh = body.mesh();
    let mut best: std::collections::BTreeMap<usize, (solidcraft_geom::Vec3, f64)> = std::collections::BTreeMap::new();
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if let Some([a, b, c]) = mesh.tri(triangle) {
            let area = (b - a).cross(c - a).len();
            let entry = best.entry(*face as usize).or_insert(((a + b + c) * (1. / 3.), -1.));
            if area > entry.1 {
                *entry = ((a + b + c) * (1. / 3.), area);
            }
        }
    }
    for (index, (point, _)) in best {
        if let Some(plane) = body.body.planar_face(index)
            && plane.normal().z.abs() < 1e-6
        {
            out.push((format!("face {index} normal {:?}", plane.normal()), [point.x, point.y, point.z]));
        }
    }
    out
}

#[test]
fn the_round_sides_of_a_thin_ring_are_named_by_the_circle_they_run_beside() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Outline", "thin": true, "thickness": 2, "distance": 5, "name": "Ring"}));
    let world = s.world_state();
    let names = solidcraft_doc::naming::face_names(world.bodies.first().unwrap());
    // Each round side is two half cylinders, told apart by their numbers.
    let walls: Vec<_> = names.iter().filter(|name| name.contains(":wall:")).collect();
    assert_eq!(walls.len(), 4, "{names:?}");
    for side in ["left", "right"] {
        assert_eq!(walls.iter().filter(|name| name.contains(&format!(":wall:c1:{side}#"))).count(), 2, "{walls:?}");
    }
}
