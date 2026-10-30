//! Booleans that failed, panicked or hung, replayed from the scripts in
//! `examples/regressions/` (each script's description says what went wrong).

use serde_json::{Value, json};

use crate::Session;

/// Runs in PR CI as well as the heavier fixed-seed suite. Two pattern copies are
/// disconnected, and one cylinder overlaps the block by half its volume.
#[test]
fn partial_face_contact_pattern_has_analytic_material() {
    // Repeated construction exposes re-imprinting a cylinder through both of its
    // side faces, which can create a redundant arc along the first imprint.
    for _ in 0..8 {
        check_partial_face_contact_pattern();
    }
}

fn check_partial_face_contact_pattern() {
    let s = replay(include_str!("../../../examples/regressions/partial_face_contact_pattern.json"));
    let (mut volume, mut area, mut shells) = (0.0, 0.0, 0);
    for b in &s.model.state().bodies {
        assert!(b.body.validity().is_empty(), "{:?}", b.body.validity());
        let m = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure();
        volume += m.volume;
        area += m.area;
        shells += b.body.shell_count();
    }
    let expected_volume = 25.0 * 47.5 * 37.5 + 3.5 * std::f64::consts::PI * 25.0 * 2.5;
    let expected_area = 2.0 * (25.0 * 47.5 + 25.0 * 37.5 + 47.5 * 37.5) + 237.5 * std::f64::consts::PI - 25.0;
    assert!((volume - expected_volume).abs() < 0.02, "{volume} expected {expected_volume}");
    assert!((area - expected_area).abs() < 0.02, "{area} expected {expected_area}");
    assert_eq!(shells, 3);
    println!("#674 volume {volume:.6} / {expected_volume:.6} mm³; area {area:.6} / {expected_area:.6} mm²; {shells} shells");
}

/// A cylinder intersected with a disc carrying a tooth whose corners stand 0.27 mm out of it
/// (radius 16.92 for 16.65): the general boolean can't intersect them, and the sampled answer
/// (the disc is inside the cylinder) used to be kept with a warning, corners and all. The
/// intersection must trim them or fail and leave the body as it was (#3244).
#[test]
fn intersection_never_keeps_material_outside_the_trimming_cylinder() {
    let mut s = Session::default();
    let (a, b) = ([-0.1706475844770042, 7.63270030627277], [0.1706475844770042, 7.63270030627277]);
    for (command, params) in [
        ("solid.cylinder", json!({"radius": 16.65, "height": 5, "body_name": "Base"})),
        ("sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [-1, 0, 0], "y_dir": [0, 0, 1]}})),
        ("sketch.line", json!({"points": [a, [-3, 4.99], [3, 4.99], b]})),
        ("sketch.arc.three_point", json!({"start": b, "through": [0, 7.7], "end": a})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 1.6875953835063608, "start_offset": 14.962404616493638, "operation": "join", "targets": ["Base"]})),
    ] {
        s.execute(command, &params).unwrap();
    }
    let before = volume(&s, "Base");
    let r = s.execute("solid.cylinder", &json!({"radius": 16.65, "height": 14.59, "operation": "intersect", "targets": ["Base"]}));
    let state = s.model.state();
    let reach = state
        .body("Base")
        .unwrap()
        .body
        .mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3))
        .unwrap()
        .positions
        .iter()
        .map(|p| p.x.hypot(p.y))
        .fold(0.0, f64::max);
    match r {
        Ok(_) => assert!(reach < 16.65 + 1e-2, "kept material out to radius {reach}"),
        Err(e) => assert!((volume(&s, "Base") - before).abs() < 1e-9 && reach > 16.9, "{e}"),
    }
}

fn replay(script: &str) -> Session {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(script).unwrap();
    s.run_script(&script).unwrap();
    s
}

fn volume(s: &Session, body: &str) -> f64 {
    s.model.state().body(body).map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0)
}

/// #5562: a quarter disc in a box's side face, at its corner, revolved 360° about the box's
/// edge through the corner and cut: a hemisphere centred on the corner, its pole on that edge.
/// Dividing one of the cut's intersection edges met a point that projects onto neither surface,
/// and the fallback unwrapped that same failed projection: a kernel panic, recorded as a crash.
/// Nothing panics now; the box loses the eighth of the ball inside it, or the cut is refused
/// and the box is left whole. The side face is picked by its point (the report's face index
/// was truck's order): `p3` is its corner (15, -5, 0) and `l4` the box's upright edge there.
#[test]
fn revolve_cut_round_a_box_corner_does_not_panic() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut s = Session::default();
    for (command, params) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.rectangle.two_point", json!({"p0": [-5.4, 7.0], "p1": [15.0, -5.0]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"direction": "positive", "distance": "10 mm", "operation": "new", "profiles": [0], "sketch": 1})),
        ("sketch.create", json!({"plane": {"face": [12.721266146083186, -5.0, 7.034827173055135]}})),
        ("sketch.circle.center", json!({"center": "p3", "radius": 4.664761515876241})),
        ("sketch.options", json!({"slice": true})),
        ("sketch.finish", json!({})),
    ] {
        s.execute(command, &params).unwrap();
    }
    let before = volume(&s, "Body1");
    let me = std::thread::current().id();
    let count = Arc::new(AtomicUsize::new(0));
    let c2 = count.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |_| {
        if std::thread::current().id() == me {
            c2.fetch_add(1, Ordering::SeqCst);
        }
    }));
    let result = s.execute("solid.revolve", &json!({"angle": "360 deg", "axis": "l4", "operation": "cut", "profiles": [2], "sketch": 3}));
    std::panic::set_hook(prev);
    assert_eq!(count.load(Ordering::SeqCst), 0, "kernel panics during the cut");
    let removed = before - volume(&s, "Body1");
    let eighth = std::f64::consts::PI * 4.664761515876241_f64.powi(3) / 6.0;
    match result {
        Ok(_) => assert!((removed - eighth).abs() < 0.02 * eighth, "removed {removed}, an eighth of the ball is {eighth}"),
        Err(e) => assert!(removed.abs() < 1e-9, "{e}, yet the box lost {removed}"),
    }
}

/// #5543: a grid of 121 holes cut through a plate, four of its circles drawn a second time on
/// top of themselves. Each of those was two profiles of one region and so two coincident tools:
/// the cut retried with the tool nudged and took fifty times as long. The profiles drawn twice
/// are cut once: no retry, and the plate is the one the 121 circles drawn once make.
#[test]
fn circles_drawn_twice_are_cut_once() {
    let cut = |repeats: usize| {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [-35, -35], "p1": [35, 35]})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 2, "operation": "new"})).unwrap();
        s.execute("sketch.create", &json!({"plane": "XY", "offset": 2})).unwrap();
        let mut centres = Vec::new();
        for i in -7i32..8 {
            for j in -8i32..9 {
                let (x, y) = (f64::from(j) * 3.45 + if i % 2 != 0 { 1.725 } else { 0.0 }, f64::from(i) * 3.0);
                if x.hypot(y) <= 20.7 {
                    centres.push([x, y]);
                }
            }
        }
        assert_eq!(centres.len(), 121);
        for c in centres.iter().chain(centres.iter().take(repeats)) {
            s.execute("sketch.circle.center", &json!({"center": c, "radius": 1.15})).unwrap();
        }
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("solid.extrude", &json!({"operation": "cut", "direction": "negative", "distance": 2, "targets": ["Body1"]})).unwrap();
        assert!(r["warning"].is_null(), "{}", r["warning"]);
        let state = s.model.state();
        let body = &state.body("Body1").unwrap().body;
        (body.volume_at(1e-4).unwrap(), body.face_count())
    };
    let (once, faces_once) = cut(0);
    let (twice, faces_twice) = cut(4);
    let analytic = 70.0 * 70.0 * 2.0 - 121.0 * std::f64::consts::PI * 1.15 * 1.15 * 2.0;
    assert!((once - analytic).abs() < 0.2, "{once} vs {analytic}");
    assert!((twice - once).abs() < 1e-9 * once, "{twice} vs {once}");
    assert_eq!(faces_twice, faces_once);
}

/// #1985: the STEP export of a small enclosure never finished. The export reads its own file
/// back, and meshing the screw hole's seam cylinder never ended: the boundary walk took
/// projected angles millions of turns apart. The file is written, reads back cleanly, and holds
/// the same solid.
#[test]
fn step_export_of_an_enclosure_with_a_screw_hole_finishes_and_reads_back() {
    let s = replay(include_str!("../../../examples/regressions/step_export_screw_hole_enclosure.json"));
    let state = s.model.state();
    let body = &state.body("Unterteil").unwrap().body;
    // 27 faces, as the file holds them: each whole turn (the hole, two standoffs) is one face.
    assert_eq!(body.face_count(), 27);
    let (text, file_faces) = read_back(&s, body);
    assert_eq!(text.matches("CYLINDRICAL_SURFACE(").count(), 7, "the seven round faces stay cylinders");
    assert_eq!(file_faces, 27);
}

/// #1985, the reporter's second reproduction: the same walk on a standoff of a flanged
/// enclosure without a screw hole.
#[test]
fn step_export_of_a_flanged_enclosure_finishes_and_reads_back() {
    let s = replay(include_str!("../../../examples/regressions/step_export_flanged_enclosure.json"));
    let state = s.model.state();
    let (_, file_faces) = read_back(&s, &state.body("Unterteil").unwrap().body);
    assert_eq!(file_faces, 29);
}

/// The body's STEP text, which reads back without warnings as one closed solid of the same
/// volume and area, and the number of faces the file holds.
fn read_back(s: &Session, body: &solidcraft_kernel::Body) -> (String, usize) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let text = solidcraft_kernel::step_export(&[body], "SolidCraft").unwrap();
    let imp = solidcraft_kernel::step_import(&text).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 1);
    let back = &imp.bodies[0];
    assert!(back.closed);
    let (a, b) = (solidcraft_kernel::measure(body).unwrap(), solidcraft_kernel::measure(&back.body).unwrap());
    assert!((a.volume - b.volume).abs() < 1e-3 * a.volume, "volume {} read back as {}", a.volume, b.volume);
    assert!((a.area - b.area).abs() < 1e-3 * a.area, "area {} read back as {}", a.area, b.area);
    println!("#1985 volume {:.3} / {:.3} mm³; area {:.3} / {:.3} mm²", a.volume, b.volume, a.area, b.area);
    (text, back.file_faces)
}

/// The timeline insertion from #1765 on a small part with analytic material. The kernel test
/// separately forces the empty numerical attempts, without needing the reporter's design.
#[test]
fn shallow_cuts_rebuild_after_inserting_an_earlier_pocket() {
    let s = replay(include_str!("../../../examples/regressions/shallow_cuts_after_earlier_pocket.json"));
    let rounded_plate = 40.0 * 30.0 * 10.0 - (4.0 - std::f64::consts::PI) * 40.0;
    // Half of each circle overlaps the existing 0.75 mm pocket.
    let expected = rounded_plate - 30.0 * 10.0 * 0.75 - 2.0 * std::f64::consts::PI * 25.0 * (1.5 - 0.75 / 2.0);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [s, reopened] {
        assert!(session.model.results.iter().all(|r| r.error.is_none()));
        assert_eq!(session.model.state().bodies.len(), 1);
        let body = &session.model.state().bodies[0].body;
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        let actual = volume(&session, "Plate");
        assert!((actual - expected).abs() < 0.02, "{actual} vs {expected}");
        let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.001)).unwrap();
        assert!(!mesh.contains(solidcraft_geom::Vec3::new(12.0, 14.0, 9.0)));
        assert!(mesh.contains(solidcraft_geom::Vec3::new(12.0, 14.0, 8.0)));
        assert!(!mesh.contains(solidcraft_geom::Vec3::new(20.0, 20.0, 9.5)));
        assert!(mesh.contains(solidcraft_geom::Vec3::new(20.0, 20.0, 9.0)));
    }
}

/// #2059: tabs exactly as wide as their supporting ribs share the ribs' side
/// planes. The base's remote conical chamfer must not prevent their join.
#[test]
fn eighteen_tabs_join_the_ribs_of_a_chamfered_base() {
    let s = replay(include_str!("../../../examples/regressions/tabs_on_chamfered_ribbed_base.json"));
    let check = |s: &Session| {
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
        assert_eq!(s.model.state().bodies.len(), 1);
        let state = s.model.state();
        let body = &state.body("Base").unwrap().body;
        assert_eq!(body.shell_count(), 1);
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap();
        // R40 x 4 disc less its 1 x 1 chamfer, six 32 x 2 ribs
        // standing 4 above it, and eighteen 1 x 2 tabs another 8 higher.
        let expected = std::f64::consts::PI * (6400.0 - 40.0 + 1.0 / 3.0) + 1536.0 + 288.0;
        let got = mesh.measure().volume;
        assert!((got - expected).abs() < 0.1, "{got} vs {expected}");
        println!("#2059 volume {got:.6} / {expected:.6} mm³; one valid shell, eighteen tabs");
        for k in 0..6 {
            let angle = k as f64 * std::f64::consts::TAU / 6.0;
            let p = |x: f64, y: f64, z| solidcraft_geom::Vec3::new(x * angle.cos() - y * angle.sin(), x * angle.sin() + y * angle.cos(), z);
            for x in [15.5, 25.5, 35.5] {
                assert!(mesh.contains(p(x, 0.0, 12.0)), "tab {k} at {x}");
                assert!(!mesh.contains(p(x, 1.2, 12.0)), "tab {k} grew sideways at {x}");
            }
            assert!(mesh.contains(p(20.0, 0.0, 6.0)), "rib {k}");
            assert!(mesh.contains(p(39.5, 0.0, 2.0)), "rim {k}");
            assert!(!mesh.contains(p(39.75, 0.0, 3.75)), "chamfer {k}");
        }
        assert!((mesh.bounds().max.z - 16.0).abs() < 1e-8);
    };
    check(&s);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened);
}

#[test]
fn buried_join_end_ignores_bodies_in_other_components() {
    for own_body in [false, true] {
        let mut s = Session::default();
        let run = |s: &mut Session, command: &str, params: Value| s.execute(command, &params).unwrap();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 8, "body_name": "Other"}));
        run(&mut s, "component.create", json!({"name": "Child"}));
        if own_body {
            run(&mut s, "solid.box", json!({"corner": [50, 0, 0], "length": 10, "width": 10, "height": 8, "body_name": "Child"}));
        }
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [4, 4]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 16, "operation": "join", "body_name": "Tab"}));
        let got = volume(&s, if own_body { "Child" } else { "Tab" });
        let expected = 64.0 + if own_body { 800.0 } else { 0.0 };
        assert!((got - expected).abs() < 1e-6, "{got}");
        assert!((volume(&s, "Other") - 800.0).abs() < 1e-6);
    }
}

/// Slab less a 557.68 × 100 × 120 pocket and the loft below its floor (556.68 × 99 down 4 mm to
/// 80 × 80; prismatoid: h/6 · (A_top + A_bottom + 4 A_mid)).
fn slab_pocket_loft() -> (f64, f64) {
    let slab = 900.0 * 480.0 * 160.0;
    let loft = 4.0 / 6.0 * (556.68 * 99.0 + 80.0 * 80.0 + 4.0 * (556.68 + 80.0) / 2.0 * (99.0 + 80.0) / 2.0);
    (slab - 557.68 * 100.0 * 120.0, loft)
}

#[test]
fn loft_cut_beside_a_round_hole() {
    let s = replay(include_str!("../../../examples/regressions/loft_cut_beside_round_hole.json"));
    let (pocketed, loft) = slab_pocket_loft();
    let hole = std::f64::consts::PI * 17.5 * 17.5 * 25.0;
    let want = pocketed - loft - hole;
    let got = volume(&s, "Body1");
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
}

#[test]
fn shallow_loft_cut_from_the_top_face() {
    let s = replay(include_str!("../../../examples/regressions/shallow_loft_cut_from_top_face.json"));
    let (_, loft) = slab_pocket_loft();
    let want = 900.0 * 480.0 * 160.0 - loft;
    let got = volume(&s, "Body1");
    assert!((got - want).abs() < 1e-9 * want, "{got} vs {want}");
}

#[test]
fn oblique_cut_through_a_small_arc_matches_a_split() {
    let scripts = [
        include_str!("../../../examples/regressions/oblique_cut_of_cylinder.json"),
        include_str!("../../../examples/regressions/oblique_cut_of_cylinder_hang.json"),
    ];
    // The same bar split by the prism's tilted top: the piece above it is the cut's result.
    let mut split = Session::default();
    let script: Value = serde_json::from_str(scripts[0]).unwrap();
    let steps = script.get("commands").and_then(Value::as_array).unwrap();
    split.run_script(&Value::Array(steps[..5].to_vec())).unwrap();
    let whole = volume(&split, "Tool");
    split.execute("solid.split_body", &json!({"body": "Tool", "plane": {"origin": [0, -472, 853.91], "normal": [0, 15.44, 40]}})).unwrap();
    let above = volume(&split, "Tool");
    assert!(above < whole - 10.0, "{above} of {whole}");
    for script in scripts {
        let s = replay(script);
        let got = volume(&s, "Tool");
        assert!((got - above).abs() < 1e-6 * above, "{got} vs {above}");
    }
}

/// Joining struts that end on a bore wall reached a truck-topology panic (#268). The guard caught it,
/// but no kernel call should panic at all (wasm aborts on a panic).
#[test]
fn struts_ending_on_a_bore_wall_join_without_a_panic() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let me = std::thread::current().id();
    let count = Arc::new(AtomicUsize::new(0));
    let c2 = count.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |_| {
        if std::thread::current().id() == me {
            c2.fetch_add(1, Ordering::SeqCst);
        }
    }));
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/regressions/join_struts_on_bore_wall.json")).unwrap();
    let result = s.run_script(&script);
    std::panic::set_hook(prev);
    assert_eq!(count.load(Ordering::SeqCst), 0, "kernel panics during the join");
    result.unwrap();
    assert_eq!(s.model.state().bodies.len(), 1);
}

/// A blind Ø2 hole into the flat back of a long D-profile bar ends 2 mm short of the round front,
/// yet the cut was refused (#544). It removes π · 1² · 10 mm³ (within 5 %: the measure tessellates
/// the small bore) and adds the bore's wall and floor to the bar's six faces.
#[test]
fn blind_hole_into_a_long_d_profile_bar_stops_short_of_the_round_front() {
    let script: Value = serde_json::from_str(include_str!("../../../examples/regressions/blind_hole_in_long_d_profile.json")).unwrap();
    let steps = script.get("commands").and_then(Value::as_array).unwrap();
    let mut s = Session::default();
    s.run_script(&Value::Array(steps[..5].to_vec())).unwrap();
    let bar = volume(&s, "Slat");
    s.run_script(&Value::Array(steps[5..].to_vec())).unwrap();
    let hole = std::f64::consts::PI * 10.0;
    let removed = bar - volume(&s, "Slat");
    assert!((removed - hole).abs() < 0.05 * hole, "removed {removed} vs {hole}");
    let faces = s.model.state().body("Slat").map(|b| solidcraft_kernel::measure(&b.body).unwrap().merged.faces);
    assert_eq!(faces, Some(8));
}

/// A ring pocket cut from below a recess leaves a 0.5 mm floor joining the outer wall to the inner
/// block: one connected part, yet the cut left two bodies and lost volume (#545).
#[test]
fn ring_cut_below_a_recess_keeps_the_part_in_one_body() {
    let s = replay(include_str!("../../../examples/regressions/ring_cut_below_recess_thin_floor.json"));
    let bodies: Vec<(String, f64)> = s.model.state().bodies.iter().map(|b| (b.name.clone(), volume(&s, &b.name))).collect();
    let want = 100.0 * 100.0 * 30.0 - 88.0 * 88.0 * 12.0 - (95.0 * 95.0 - 68.0 * 68.0) * 17.5;
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    let got = volume(&s, "Frame");
    assert!((got - want).abs() < 1e-9 * want, "{got} vs {want}");
}

/// Four ribs joined on a rounded lid, standing on the plate over the skirt and flush with the
/// window's sides: each adds exactly its own material and nothing changes away from it. The
/// joins displaced the tool and one left a slit through the far side of the skirt and plate
/// (#616), which no volume shows: the area does. The reply of each join carries its warning, as
/// the timeline does (an agent placing parts over MCP saw none).
#[test]
fn ribs_flush_with_a_window_change_nothing_away_from_them() {
    let script: Value = serde_json::from_str(include_str!("../../../examples/regressions/lid_ribs_flush_with_window.json")).unwrap();
    let steps = script.get("commands").and_then(Value::as_array).unwrap();
    let (lid, ribs) = steps.split_at(steps.len() - 4);
    let mut s = Session::default();
    s.run_script(&Value::Array(lid.to_vec())).unwrap();
    let measure = |s: &Session| s.model.state().body("Lid").map(|b| solidcraft_kernel::measure(&b.body).unwrap()).unwrap();
    let (v0, a0) = (measure(&s).volume, measure(&s).area);
    // The plate with its rounded corners, the skirt, less the window, the slot and the notch
    // through the skirt: (76.8 × 89.3 - (4 - π) × 9) × 2.4 + (71.5 × 84 - 68.5 × 81) × 3
    // - 68 × 61 × 2.4 - 44 × 10 × 2.4 - 22 × 1.5 × 3.
    let lid = (76.8 * 89.3 - (4.0 - std::f64::consts::PI) * 9.0) * 2.4 + (71.5 * 84.0 - 68.5 * 81.0) * 3.0
        - 68.0 * 61.0 * 2.4
        - 44.0 * 10.0 * 2.4
        - 22.0 * 1.5 * 3.0;
    assert!((v0 - lid).abs() < 1e-3, "{v0} vs {lid}");
    for (i, rib) in ribs.iter().enumerate() {
        let reply = s.execute(rib["command"].as_str().unwrap(), &rib["params"]).unwrap();
        let id = reply["feature"].as_u64().unwrap();
        let result = s.model.result(id).unwrap();
        assert_eq!(reply["warning"], json!(result.warning), "the reply says what the timeline says");
        assert!(!result.warning.as_deref().unwrap_or_default().contains("tool displacement"), "{:?}", result.warning);
        let m = measure(&s);
        let n = (i + 1) as f64;
        // The measure's accuracy: SMLib's mass properties come within 2e-7 of a closed form.
        let (dv, da) = (2e-7 * m.volume, 2e-7 * m.area);
        // 1.6 x 6 x 11.7 less the 1.35 x 6 x 3 inside the skirt.
        assert!((m.volume - (v0 + n * 88.02)).abs() < dv, "rib {}: {}", i + 1, m.volume - v0);
        // Top 9.6, outer side 6 x 8.7 above the skirt, inner side 6 x 11.7 (flush with the
        // window's side), ends 2 x (1.6 x 11.7 - 1.35 x 3); the skirt top 8.1, its inner side
        // 18 and 1.5 of the plate go under the rib.
        let area = 9.6 + 52.2 + 70.2 + 29.34 - 8.1 - 18.0 - 1.5;
        assert!((m.area - (a0 + n * area)).abs() < da, "rib {}: area {} (no slit adds any)", i + 1, m.area - a0);
    }
    let bounds = s.model.state().body("Lid").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap().bounds();
    assert!((bounds.max.z - 14.1).abs() < 1e-9, "{}", bounds.max.z);
}

/// #679: one sketch of a 2 × 2 grid of 10 mm squares; three cells extruded up 10 mm (XO/XX) and
/// three down 10 mm, joined (XX/OX). The two L's meet back to back over two whole cells, along
/// an edge and at the centre: one solid of 6 cells × 10 mm × 100 mm².
#[test]
fn ls_extruded_up_and_down_from_one_grid_join() {
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Grid"}));
    for (p0, p1) in [([-10, -10], [0, 0]), ([0, -10], [10, 0]), ([-10, 0], [0, 10]), ([0, 0], [10, 10])] {
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let cells = |pts: &[[f64; 2]]| Value::Array(pts.iter().map(|p| json!({"point": p})).collect());
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Grid", "profiles": cells(&[[-5., 5.], [-5., -5.], [5., -5.]]), "distance": 10, "body_name": "Logo"}),
    );
    let reply = run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Grid", "profiles": cells(&[[-5., 5.], [5., 5.], [5., -5.]]), "distance": "10 mm", "direction": "negative", "operation": "join"}),
    );
    assert!(reply["warning"].as_str().is_none_or(|w| !w.contains("tool displacement")), "{reply}");
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1);
    let body = &st.body("Logo").unwrap().body;
    assert_eq!(body.shell_count(), 1);
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let v = volume(&s, "Logo");
    assert!((v - 6000.0).abs() < 1e-6, "{v}");
}

/// This process's CPU time (all threads) and peak resident memory, where the system says
/// (Linux: /proc/self); `None` elsewhere.
fn cpu_and_peak_memory() -> Option<(std::time::Duration, u64)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // utime and stime, in clock ticks (100 a second on Linux), after the command's name.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks: u64 = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = status.lines().find_map(|l| l.strip_prefix("VmHWM:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some((std::time::Duration::from_millis(ticks * 10), kb * 1024))
}

/// Press/pull on the sensor enclosure's lid top: joining the pulled face's prism imprints the
/// contact (#825), and boxing the lid's faces by dividing their curved edges down to the
/// contact tolerance ran away (over 24 GB and many minutes). It takes about 9 s of CPU here and
/// no memory beyond the design's. The budgets are work, not wall time, which a loaded runner
/// stretches (61-87 s for the same work on CI).
#[test]
fn press_pull_on_the_enclosure_lid_is_prompt() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    let total = |s: &Session| -> f64 { s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum() };
    let before = total(&s);
    let start = cpu_and_peak_memory();
    s.execute("solid.press_pull", &json!({"face": [-12.0, 27.4, 23.0], "distance": 5.0})).unwrap();
    if let (Some((cpu0, peak0)), Some((cpu1, peak1))) = (start, cpu_and_peak_memory()) {
        let grew = peak1.saturating_sub(peak0);
        assert!(grew < 1 << 30, "the press/pull grew the peak memory by {} MB", grew >> 20);
        // Other tests in this process (cargo test) count too, so the bound is generous.
        assert!(cpu1 - cpu0 < std::time::Duration::from_secs(300), "the press/pull took {:?} of CPU", cpu1 - cpu0);
    }
    assert!(total(&s) > before, "{before} -> {}", total(&s));
}

/// A lip swept round a block's rim, flush with its sides and round its corners, joins it as one
/// solid of both volumes: the block (50 × 40 with R4.5 corners, 10 high) and the lip (1 × 1 along
/// 41 + 31 + 41 mm of straight rim and two quarter turns between radii 3.5 and 4.5).
#[test]
fn lip_swept_flush_round_a_corner_joins() {
    let s = replay(include_str!("../../../examples/regressions/lip_swept_flush_round_corner.json"));
    let bodies = &s.model.state().bodies;
    assert_eq!(bodies.len(), 1);
    let b = &bodies[0];
    assert!(b.body.validity().is_empty(), "{:?}", b.body.validity());
    assert_eq!(b.body.shell_count(), 1);
    let block = (50.0 * 40.0 - (4.0 - std::f64::consts::PI) * 4.5 * 4.5) * 10.0;
    let lip = 41.0 + 31.0 + 41.0 + 2.0 * std::f64::consts::PI * (4.5 * 4.5 - 3.5 * 3.5) / 4.0;
    let volume = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().volume;
    assert!((volume - block - lip).abs() < 0.02, "{volume} expected {}", block + lip);
}

/// #1275: an Extrude Cut of a whole face of a rounded-rectangle block with a bottom chamfer. The
/// tool's corner cylinders lay on the block's; they are grown off them now, as straight edges
/// were, and the cut takes 5 mm off the top.
#[test]
fn cut_of_a_whole_face_beside_round_corners_after_a_chamfer() {
    let s = replay(include_str!("../../../examples/regressions/cut_whole_face_after_bottom_chamfer.json"));
    let bodies = &s.model.state().bodies;
    assert_eq!(bodies.len(), 1);
    let b = &bodies[0];
    assert!(b.body.validity().is_empty(), "{:?}", b.body.validity());
    let pi = std::f64::consts::PI;
    let block = (40.0 * 40.0 - (4.0 - pi) * 8.0 * 8.0) * 15.0;
    // The chamfer's 2 × 2 triangle swept along the loop (Pappus: its centroid is 2/3 mm in).
    let chamfer = 2.0 * (4.0 * 24.0 + 2.0 * pi * (8.0 - 2.0 / 3.0));
    let mesh = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap();
    let volume = mesh.measure().volume;
    assert!((volume - (block - chamfer)).abs() < 0.05, "{volume} expected {}", block - chamfer);
    let top = mesh.bounds().max.z;
    assert!((top - 15.0).abs() < 1e-6, "top at {top}");
}

/// The display tessellation of loft bodies joined into a bike frame plans at every display
/// quality and every refinement step. The edge nodes on an intersection of two lofts once
/// stepped back along the curve (the edge's fit rang where truck's parameter changes speed, and
/// took a leader point off the curve past a vertex), so a face's boundary crossed itself and
/// the body fell back to the reference mesher, which drew a sheet across the down tube.
#[test]
fn joined_loft_frames_plan_their_display_at_every_quality() {
    use solidcraft_doc::display_quality::{DisplayQuality, MAX_REFINE};
    for (script, name) in [
        (include_str!("../../../examples/regressions/loft_join_bracket_hull_triangle.json"), "DT2"),
        (include_str!("../../../examples/regressions/front_triangle_hull_triangle.json"), "FrontTriangle"),
    ] {
        let s = replay(script);
        let state = s.model.state();
        let body = &state.body(name).unwrap().body;
        for q in DisplayQuality::ALL {
            for k in 0..=MAX_REFINE {
                let plan =
                    body.mesh_plan(&solidcraft_kernel::MeshTolerance { chord: q.refined_chord(body.size(), k), angle: Some(q.refined_angle(k)) });
                assert!(plan.is_ok(), "{name} at {} + {k}: {:?}", q.id(), plan.err());
            }
        }
    }
}

/// A chamfered 240° ring cut by two prisms whose outlines have a corner exactly on its bore and
/// one in its end face, then notched into its top outer chamfer: each cut is exact (no retry
/// moved the tool), the two mirrored cuts take the same material, the notch takes what its
/// integral says, and the result meshes to its volume at a coarse tolerance too. The prism cuts
/// went to the last-resort nudge (seconds each, a face of no width left at each corner) and the
/// notch was refused for its volume: its hole in the chamfer meshed several mm² too large.
#[test]
fn oblique_cuts_through_a_chamfered_ring_are_exact() {
    let script: Value = serde_json::from_str(include_str!("../../../examples/regressions/oblique_cuts_through_chamfered_ring.json")).unwrap();
    let steps = script.get("commands").and_then(Value::as_array).unwrap();
    let mut s = Session::default();
    // The ring after the move and after each cut, and what each cut took away (the ring before it
    // less the ring after it: a small body that measures to its own accuracy, where the difference
    // of the whole ring's volumes is lost in the integration's 1e-6 of 1149 mm³).
    let mut rings: Vec<solidcraft_kernel::Body> = Vec::new();
    for step in steps {
        let reply = s.execute(step["command"].as_str().unwrap(), &step["params"]).unwrap();
        if step["command"] == "solid.extrude" {
            assert!(reply["warning"].is_null(), "{reply}");
        }
        if matches!(step["command"].as_str(), Some("solid.move" | "solid.extrude")) {
            rings.push(s.model.state().body("R").unwrap().body.clone());
        }
    }
    let taken = |k: usize| -> f64 {
        let piece = solidcraft_kernel::boolean(&rings[k], &rings[k + 1], solidcraft_kernel::BoolOp::Cut).unwrap().unwrap();
        solidcraft_kernel::measure(&piece).unwrap().volume
    };
    let volumes: Vec<f64> = rings.iter().map(|b| b.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().volume).collect();
    // Pappus: the profile's moment about the axis turned through 240°.
    let profile = [(16.6, 0.0), (17.5, 0.0), (18.5, 1.0), (18.5, 7.0), (17.5, 8.0), (16.6, 8.0), (16.4, 7.8), (16.4, 0.2)];
    let moment: f64 = (0..profile.len())
        .map(|i| {
            let ((x0, y0), (x1, y1)) = (profile[i], profile[(i + 1) % profile.len()]);
            (x0 * y1 - x1 * y0) * (x0 + x1) / 6.0
        })
        .sum();
    let ring = moment.abs() * 4.0 * std::f64::consts::PI / 3.0;
    let [whole, _, _, _] = volumes[..] else { panic!("{volumes:?}") };
    // Volumes from meshes at 1e-5 mm: within a few millionths of the ring's.
    assert!((whole - ring).abs() < 1e-5 * ring, "{whole} vs {ring}");
    // The ring and the two prisms are mirror images across XZ.
    let (cut1, cut2) = (taken(0), taken(1));
    assert!(cut1 > 5.0 && (cut1 - cut2).abs() < 1e-6 * cut1, "{cut1} vs {cut2}");
    let (notched, notch) = (taken(2), ring_notch());
    assert!((notched - notch).abs() < 1e-5 * notch, "{notched} vs {notch}");
    let state = s.model.state();
    let body = &state.body("R").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    // Faces meshed coarsely keep to their surfaces: chords cut a little area off round faces,
    // and nothing adds any (triangles fanning across a chamfer from the notch's hole or from the
    // cuts' trims, folded over each other, added a few mm² each).
    let (coarse, fine) = (
        body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.05)).unwrap().measure().area,
        body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().area,
    );
    assert!(coarse < fine && coarse > fine - 2.0, "area meshed at 0.05: {coarse} vs {fine}");
    println!(
        "ring {whole:.6} / {ring:.6} mm³; cuts {cut1:.6}, {cut2:.6}; notch {notched:.6} / {notch:.6}; area meshed at 0.05 {coarse:.4} / {fine:.4}"
    );
}

/// The ring's notch alone: its hole in the top outer chamfer meshed several mm² too large, and
/// the cut was refused for an implausible volume after more than a minute; it takes what its
/// integral says.
#[test]
fn notch_into_a_ring_chamfer_is_cut_exactly() {
    let script: Value = serde_json::from_str(include_str!("../../../examples/regressions/oblique_cuts_through_chamfered_ring.json")).unwrap();
    let steps = script.get("commands").and_then(Value::as_array).unwrap();
    let mut s = Session::default();
    s.run_script(&Value::Array(steps[..5].to_vec())).unwrap();
    let before = s.model.state().body("R").unwrap().body.clone();
    // The notch's sketch and its cut.
    for step in &steps[steps.len() - 4..] {
        let reply = s.execute(step["command"].as_str().unwrap(), &step["params"]).unwrap();
        assert!(reply["warning"].is_null(), "{reply}");
    }
    // What the cut took: the ring before it less the ring after it, measured as a body of its own
    // (the difference of the two rings' volumes is lost in the integration's 1e-6 of 1149 mm³).
    let piece = solidcraft_kernel::boolean(&before, &s.model.state().body("R").unwrap().body, solidcraft_kernel::BoolOp::Cut).unwrap().unwrap();
    let (got, want) = (solidcraft_kernel::measure(&piece).unwrap().volume, ring_notch());
    assert!((got - want).abs() < 1e-5 * want, "{got} vs {want}");
}

/// What the ring's notch takes: a band 0.9 wide about a radius, from 17.6 out along it, and from
/// z 7.1 up to the top outer chamfer (r = 25.5 - z). Across the band (s), along it from 17.6 to
/// where r = 18.4, the height 18.4 - r: along it in closed form, across by Simpson's rule.
fn ring_notch() -> f64 {
    let along = |s: f64| {
        let x1 = (18.4f64 * 18.4 - s * s).sqrt();
        let g = |x: f64| {
            let r = (x * x + s * s).sqrt();
            18.4 * x - 0.5 * (x * r + s * s * (x + r).ln())
        };
        g(x1) - g(17.6)
    };
    let n = 2000;
    let h = 0.9 / n as f64;
    let weight = |i: usize| {
        if i == 0 || i == n {
            1.0
        } else if i % 2 == 1 {
            4.0
        } else {
            2.0
        }
    };
    (0..=n).map(|i| weight(i) * along(-0.45 + i as f64 * h)).sum::<f64>() * h / 3.0
}

/// The wedge of #1617: a side profile in (y, z) extruded 138 mm along x, its back face leaning
/// by `lean` (the tangent of its angle), with the issue's rounds, each on the edges it picked. The last one runs along the
/// outline of both ends. The issue asked for R1 with `fit: reduce`, which reduces it to 0.854 mm;
/// that radius is asked for here directly, which builds the same body without the fit's search
/// (about a minute).
fn raked_wedge(lean: f64) -> Session {
    let mut s = Session::default();
    let top = 60.0 - 60.0 * lean;
    let profile = [[-60.0, 0.0], [60.0, 0.0], [top, 60.0], [38.0, 60.0], [-22.0, 22.0], [-60.0, 22.0]];
    let back = (60.0 + top) / 2.0;
    let ends: Vec<[f64; 3]> = [-69.0, 69.0].iter().flat_map(|&x| [[x, -41.0, 22.0], [x, 8.0, 41.0], [x, (38.0 + top) / 2.0, 60.0]]).collect();
    let steps = [
        ("sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "Profile"})),
        ("sketch.line", json!({"points": profile, "closed": true})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 138, "direction": "symmetric", "operation": "new", "body_name": "Wedge"})),
        ("solid.fillet", json!({"body": "Wedge", "radius": 1.2, "edges": [[0, -60, 22]]})),
        ("solid.fillet", json!({"body": "Wedge", "radius": 1.5, "edges": [[0, -22, 22]]})),
        ("solid.fillet", json!({"body": "Wedge", "radius": 1.5, "edges": [[0, 38, 60]]})),
        // The issue's R4 rounds these four end edges alone: carried on round the end outline
        // (tangent propagation, the feature's default since 84bd2daad, after this test), it would
        // run onto the R1.2 round's end arc, where its rail on the end face would be an arc of
        // radius 1.2 - 4 < 0.
        (
            "solid.fillet",
            json!({"body": "Wedge", "radius": 4, "tangent_chain": false, "edges": [[-69, -60, 12], [69, -60, 12], [-69, back, 30], [69, back, 30]]}),
        ),
        ("solid.fillet", json!({"body": "Wedge", "radius": 0.854, "edges": ends})),
    ];
    for (command, params) in steps {
        s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
    }
    s
}

/// A box intersect taking the end slice x >= 55 off the wedge of #1617 failed with "no result"
/// once its ends were rounded, when its back face leaned 10 degrees (a vertical one worked).
/// The leaning back shortens the top edge the end round runs along to 11.4 mm. Its strip there
/// borrowed sections from that short neighbour and folded back on itself past the joint, and
/// the boolean could not intersect the folded faces (fixed with #1667). The slice and the
/// rest, cut off by the same box, are sound and add up to the wedge, with the cut plane where
/// the issue had it and moved by 0.02 mm, as it also tried.
#[test]
fn box_intersect_splits_the_end_off_a_raked_wedge_after_its_end_rounds() {
    for lean in [10f64.to_radians().tan(), 0.0] {
        let mut s = raked_wedge(lean);
        let whole = volume(&s, "Wedge");
        for (i, x0) in [55.0, 55.02].into_iter().enumerate() {
            let mut piece = |op: &str| -> (f64, solidcraft_geom::Aabb3) {
                let copy = s.execute("solid.move", &json!({"bodies": ["Wedge"], "translate": [0, 0, 0], "copy": true})).unwrap();
                let copy = copy["created_bodies"][0].as_str().unwrap().to_string();
                let cap = format!("Cap{i}{op}");
                let length = 80.0 - x0;
                let cap_box = json!({"center": [x0 + length / 2.0, 0, 40], "length": length, "width": 160, "height": 120, "operation": "new", "body_name": cap});
                s.execute("solid.box", &cap_box).unwrap();
                let (target, tool) = if op == "intersect" { (cap, copy) } else { (copy, cap) };
                s.execute("solid.combine", &json!({"target": target, "tools": [tool], "operation": op}))
                    .unwrap_or_else(|e| panic!("lean {lean}, x >= {x0}, {op}: {e}"));
                let state = s.model.state();
                let body = &state.body(&target).unwrap().body;
                assert!(body.validity().is_empty(), "lean {lean}, x >= {x0}, {op}: {:?}", body.validity());
                (solidcraft_kernel::measure(body).unwrap().volume, body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3)).unwrap().bounds())
            };
            let (slice, at) = piece("intersect");
            let (rest, _) = piece("cut");
            println!("#1617 lean {lean:.4}, x >= {x0}: slice {slice:.6} + rest {rest:.6} = {:.6}, wedge {whole:.6} mm³", slice + rest);
            assert!((slice + rest - whole).abs() < 1e-6 * whole, "{slice} + {rest} vs {whole}");
            assert!((at.min.x - x0).abs() < 1e-6 && (at.max.x - 69.0).abs() < 1e-3, "slice spans x {} to {}", at.min.x, at.max.x);
        }
    }
}

/// #1853: half a slot drawn on a plate's top face, cut along its centreline, revolved about a
/// construction line on that centreline as a cut. Half a turn into the plate and a full turn
/// both failed with "no result": the revolve came out wrong where its arcs end on the axis (a
/// fifth too much or too little), and the cut could not intersect it there. Both now take the
/// half of the capsule below the face.
#[test]
fn revolve_cut_of_a_half_slot_about_its_centreline_in_the_face() {
    let pi = std::f64::consts::PI;
    let inside = (pi * 9.0 * 14.0 + 4.0 / 3.0 * pi * 27.0) / 2.0;
    for angle in ["-180 deg", "360 deg"] {
        let mut s = Session::default();
        let steps = [
            ("solid.box", json!({"corner": [0, 0, 0], "length": 40, "width": 20, "height": 10, "body_name": "Plate"})),
            ("sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Half"})),
            ("sketch.line", json!({"points": [[10, 10], [30, 10]]})),
            ("sketch.line", json!({"points": [[27, 13], [13, 13]]})),
            ("sketch.arc.center_point", json!({"center": [13, 10], "start": [13, 13], "end": [10, 10]})),
            ("sketch.arc.center_point", json!({"center": [27, 10], "start": [30, 10], "end": [27, 13]})),
            ("sketch.line", json!({"points": [[5, 10], [35, 10]], "construction": true, "ids": ["axis"]})),
            ("sketch.finish", json!({})),
        ];
        for (command, params) in steps {
            s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
        }
        let whole = volume(&s, "Plate");
        s.execute("solid.revolve", &json!({"sketch": "Half", "axis": "axis", "angle": angle, "operation": "cut"}))
            .unwrap_or_else(|e| panic!("revolve {angle}: {e}"));
        let bad = s.model.state().body("Plate").map(|b| b.body.validity()).unwrap_or_default();
        assert!(bad.is_empty(), "{angle}: {bad:?}");
        let removed = whole - volume(&s, "Plate");
        println!("#1853 revolve {angle}: removed {removed:.6} mm³ of the {inside:.6} below the face");
        assert!((removed - inside).abs() < 1e-3 * inside, "{angle}: removed {removed} instead of {inside}");
    }
}

/// #2005: changing a circular sweep from a separate body to a half-turn cut must create the
/// internal channel, rather than failing the Boolean on coincident end caps.
#[test]
fn closed_sweep_redefines_to_a_half_turn_torus_cut() {
    let s = replay(include_str!("../../../examples/regressions/closed_sweep_torus_cut.json"));
    assert_eq!(s.model.state().bodies.len(), 1);
    let body = &s.model.state().bodies[0].body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.02)).unwrap();
    let expected = (std::f64::consts::PI * 64.0 - 64.0) * std::f64::consts::TAU * 30.0;
    assert!((mesh.measure().volume - expected).abs() < expected * 0.015);
    for (point, material) in [(solidcraft_geom::Vec3::new(30.0, 0.0, 0.0), false), (solidcraft_geom::Vec3::new(30.0, 0.0, 7.0), true)] {
        assert_eq!(mesh.contains(point), material, "material at {point:?}");
    }
}

#[test]
fn many_extrusion_profiles_preserve_material_and_body_identity() {
    for operation in ["join", "cut", "new"] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "body_name": "Stock"})).unwrap();
        s.execute("sketch.create", &json!({"plane": {"origin": [0, 0, 20], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})).unwrap();
        for i in 0..6 {
            for j in 0..4 {
                let (x, y) = (2 + i * 6, 2 + j * 7);
                s.execute("sketch.rectangle.two_point", &json!({"p0": [x, y], "p1": [x + 4, y + 4]})).unwrap();
            }
        }
        s.execute("sketch.finish", &json!({})).unwrap();
        let distance = if operation == "cut" { -5 } else { 5 };
        let before = s.revision;
        s.execute("solid.extrude", &json!({"distance": distance, "operation": operation})).unwrap();
        let state = s.model.state();
        assert_eq!(state.bodies.len(), if operation == "new" { 25 } else { 1 });
        let expected = 24_000.0
            + match operation {
                "join" => 1920.0,
                "cut" => -1920.0,
                _ => 0.0,
            };
        assert!((volume(&s, "Stock") - expected).abs() < 1e-6);
        let stock = state.body("Stock").unwrap();
        for (point, present) in [([3.0, 3.0, 22.0], operation == "join"), ([3.0, 3.0, 18.0], operation != "cut"), ([7.0, 3.0, 18.0], true)] {
            assert_eq!(stock.mesh().contains(solidcraft_geom::Vec3::from(point)), present, "{operation}: {point:?}");
        }
        assert!(stock.body.validity().is_empty());
        assert!(s.revision > before);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!((volume(&s, "Stock") - 24_000.0).abs() < 1e-6);
        s.execute("edit.redo", &json!({})).unwrap();
        assert!((volume(&s, "Stock") - expected).abs() < 1e-6);
    }
}

#[test]
fn many_circular_extrusion_profiles_cut_a_curved_target() {
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius": 20, "height": 20, "body_name": "Stock"})).unwrap();
    s.execute("sketch.create", &json!({"plane": {"origin": [0, 0, -1], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})).unwrap();
    for x in [-9, -3, 3, 9] {
        for y in [-9, -3, 3, 9] {
            s.execute("sketch.circle.center", &json!({"center": [x, y], "radius": 1})).unwrap();
        }
    }
    s.execute("sketch.finish", &json!({})).unwrap();
    s.execute("solid.extrude", &json!({"distance": 22, "operation": "cut", "targets": ["Stock"]})).unwrap();
    // The rebuilt caps mesh more finely than the original cylinder. Compare with the
    // analytic material volume, rather than subtracting two differently faceted meshes.
    let expected = (400.0 - 16.0) * std::f64::consts::PI * 20.0;
    let actual = volume(&s, "Stock");
    assert!((actual - expected).abs() < expected * 1e-5, "{actual} vs {expected}");
    let state = s.model.state();
    assert_eq!(state.bodies.len(), 1);
    let stock = state.body("Stock").unwrap();
    assert!(stock.body.validity().is_empty());
    let mesh = stock.mesh();
    assert!(mesh.contains(solidcraft_geom::Vec3::new(0.0, 0.0, 10.0)));
    for x in [-9.0, -3.0, 3.0, 9.0] {
        for y in [-9.0, -3.0, 3.0, 9.0] {
            assert!(!mesh.contains(solidcraft_geom::Vec3::new(x, y, 10.0)), "hole at {x}, {y}");
        }
    }
}

#[test]
fn coplanar_body_faces_extrude_together_and_replay_their_links() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":64,"width":64,"height":20,"body_name":"Plate"})).unwrap();
    s.execute("sketch.create", &json!({"plane":{"origin":[0,0,20],"x_dir":[1,0,0],"y_dir":[0,1,0]}})).unwrap();
    let mut faces = Vec::new();
    for i in 0..8 {
        for j in 0..8 {
            let (x, y) = (2 + i * 7, 2 + j * 7);
            s.execute("sketch.rectangle.two_point", &json!({"p0":[x,y],"p1":[x+4,y+4]})).unwrap();
            faces.push(json!([x + 2, y + 2, 26]));
        }
    }
    s.execute("sketch.finish", &json!({})).unwrap();
    s.execute("solid.extrude", &json!({"distance":6,"operation":"join"})).unwrap();
    let (features, undo) = (s.doc.features.len(), s.undo.len());
    s.execute("solid.extrude", &json!({"faces":faces,"distance":-7,"operation":"cut"})).unwrap();
    assert_eq!(s.doc.features.len(), features + 2, "one linked sketch and one extrusion");
    assert_eq!(s.undo.len(), undo + 1);
    let actual = volume(&s, "Plate");
    let state = s.model.state();
    let sketch = state.sketch(4).unwrap();
    assert!((actual - 80_896.0).abs() < 1e-5, "volume {actual}; plane {:?}; profiles {}", sketch.plane, sketch.profiles.len());
    assert_eq!(s.model.state().bodies.len(), 1);
    let document = serde_json::from_value(serde_json::to_value(s.doc.as_ref()).unwrap()).unwrap();
    let mut reopened = Session { doc: std::sync::Arc::new(document), ..Session::default() };
    reopened.refresh();
    assert!((volume(&reopened, "Plate") - 80_896.0).abs() < 1e-5);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.features.len(), features);
    s.execute("edit.redo", &json!({})).unwrap();
    assert!((volume(&s, "Plate") - 80_896.0).abs() < 1e-5);
}

#[test]
fn unsupported_face_groups_are_atomic() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let before = serde_json::to_value(s.doc.as_ref()).unwrap();
    let (revision, undo) = (s.revision, s.undo.len());
    for faces in [
        json!([]),
        json!([[5, 5, 10], [5, 5, 0]]),
        json!([[5, 5, 10], [5, 0, 5]]),
        json!([[5, 5, 10], [1000, 1000, 1000]]),
        json!(vec![[5, 5, 10]; 1025]),
    ] {
        assert!(s.execute("solid.extrude", &json!({"faces":faces,"distance":5,"operation":"join"})).is_err());
        assert_eq!(serde_json::to_value(s.doc.as_ref()).unwrap(), before);
        assert_eq!((s.revision, s.undo.len()), (revision, undo));
    }
}

#[test]
fn imported_rotated_face_extrusions_preserve_other_operations_and_bodies() {
    use solidcraft_geom::Vec3;
    let mut source = Session::default();
    source.execute("solid.box", &json!({"corner":[-16,-16,-3],"length":32,"width":32,"height":1,"body_name":"Other"})).unwrap();
    source.execute("solid.cylinder", &json!({"radius":14,"height":8,"body_name":"Stock"})).unwrap();
    let mut outlines = Vec::new();
    let mut centers = Vec::new();
    for i in 0..12 {
        let angle = i as f64 * std::f64::consts::TAU / 12.;
        let (c, s) = (angle.cos(), angle.sin());
        let outline: Vec<_> =
            [(-2., -2.), (2., -2.), (2., 2.), (-2., 2.)].into_iter().map(|(x, y)| [10.7 * c + x * c - y * s, 10.7 * s + x * s + y * c]).collect();
        outlines.push(outline);
        centers.push([10.7 * c, 10.7 * s, 12.]);
    }
    let draw = |session: &mut Session, height: f64| {
        session.execute("sketch.create", &json!({"plane":{"origin":[0,0,height],"x_dir":[1,0,0],"y_dir":[0,1,0]}})).unwrap();
        for outline in &outlines {
            session.execute("sketch.line", &json!({"points":outline,"closed":true,"infer":false})).unwrap();
        }
        session.execute("sketch.finish", &json!({})).unwrap();
    };
    draw(&mut source, 8.);
    source.execute("solid.extrude", &json!({"distance":4,"operation":"join"})).unwrap();
    // An original public-safe STEP example, with rotated full-face seams and a
    // curved target. No vendor/user design is part of this regression.
    let path = std::env::temp_dir().join(format!("solidcraft-face-extrusion-{}.step", std::process::id()));
    source.execute("file.export", &json!({"path":path.to_string_lossy()})).unwrap();
    let mut imported = Session::default();
    imported.execute("doc.open", &json!({"path":path.to_string_lossy()})).unwrap();
    std::fs::remove_file(path).unwrap();
    let original = imported.model.state();
    let other = original.body("Other").unwrap().body.clone();
    for (operation, distance) in [("join", 2.), ("cut", -1.), ("new", 2.), ("intersect", -1.)] {
        let mut grouped = imported.scratch();
        grouped.execute("solid.extrude", &json!({"faces":centers,"distance":distance,"operation":operation})).unwrap();
        let mut reference = imported.scratch();
        draw(&mut reference, 12.);
        if matches!(operation, "join" | "cut") {
            // Individual tools retain the original Boolean and participant path.
            for index in 0..12 {
                reference
                    .execute("solid.extrude", &json!({"sketch":"Sketch1","profiles":[index],"distance":distance,"operation":operation}))
                    .unwrap();
            }
        } else {
            reference.execute("solid.extrude", &json!({"distance":distance,"operation":operation})).unwrap();
        }
        let actual = grouped.model.state();
        let expected = reference.model.state();
        assert_eq!(actual.bodies.len(), expected.bodies.len(), "{operation}");
        for body in &actual.bodies {
            let reference = expected.body(&body.name).unwrap();
            // Fixed fine chord tolerance: cap subdivision can change the default
            // adaptive mesh's cylinder faceting without changing its analytic surface.
            let (a, b) = (
                body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().volume,
                reference.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().volume,
            );
            assert!((a - b).abs() < b.max(1.) * 1e-6, "{operation}: {} {a} vs {b}", body.name);
            assert!(body.body.validity().is_empty(), "{operation}: {}", body.name);
            let (a, b) = (body.mesh(), reference.mesh());
            for center in &centers {
                for z in [7., 11.5, 13.] {
                    let point = Vec3::new(center[0], center[1], z);
                    assert_eq!(a.contains(point), b.contains(point), "{operation}: {} {point:?}", body.name);
                }
            }
        }
        if operation != "intersect" {
            let unchanged = &actual.body("Other").unwrap().body;
            assert_eq!(unchanged.face_count(), other.face_count());
            assert_eq!(unchanged.color(), other.color());
            assert_eq!(
                unchanged.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap().positions,
                other.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap().positions,
                "unaffected imported body"
            );
        }
    }
}

#[test]
fn multi_target_join_keeps_nearby_bodies_and_original_absorption_order() {
    for (second_x, expected_count) in [(10.0015, 2), (9., 1)] {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Stock"})).unwrap();
        session.execute("solid.box", &json!({"corner":[second_x,8,10],"length":1,"width":1,"height":2,"body_name":"Other"})).unwrap();
        session.execute("sketch.create", &json!({"plane":{"origin":[0,0,10],"x_dir":[1,0,0],"y_dir":[0,1,0]}})).unwrap();
        for origin in [[0., 0.], [8., 8.]] {
            session.execute("sketch.rectangle.two_point", &json!({"p0":origin,"p1":[origin[0]+2.,origin[1]+2.]})).unwrap();
        }
        session.execute("sketch.finish", &json!({})).unwrap();
        let mut reference = session.scratch();
        for index in 0..2 {
            reference.execute("solid.extrude", &json!({"sketch":"Sketch1","profiles":[index],"distance":2,"operation":"join"})).unwrap();
        }
        session.execute("solid.extrude", &json!({"distance":2,"operation":"join"})).unwrap();
        assert_eq!(session.model.state().bodies.len(), expected_count);
        assert_eq!(
            session.model.state().bodies.iter().map(|body| body.name.clone()).collect::<Vec<_>>(),
            reference.model.state().bodies.iter().map(|body| body.name.clone()).collect::<Vec<_>>()
        );
        for body in &session.model.state().bodies {
            assert!((volume(&session, &body.name) - volume(&reference, &body.name)).abs() < 1e-6);
        }
    }
}

/// Two posts joined into one body while apart, trimmed by a cut that splits neither, then joined
/// to a case they stand on. The cut split the joined posts into "Posts" and "Posts (1)" without
/// saying so (`created_bodies` was empty), and the join then took only "Posts", leaving the other
/// post behind as a body of its own. A cut that does cut a body in two still makes two bodies,
/// and its reply names the new one.
#[test]
fn cut_keeps_posts_joined_while_apart_in_one_body() {
    let mut s = Session::default();
    let post = |name: &str, x: f64| json!({"length": 10, "width": 10, "height": 30, "center": [x, 0, 25], "operation": "new", "body_name": name});
    s.execute("solid.box", &json!({"length": 60, "width": 40, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Case"})).unwrap();
    s.execute("solid.box", &post("Posts", -20.0)).unwrap();
    s.execute("solid.box", &post("Post2", 20.0)).unwrap();
    s.execute("solid.combine", &json!({"target": "Posts", "tools": ["Post2"], "operation": "join"})).unwrap();
    s.execute("solid.box", &json!({"length": 80, "width": 20, "height": 10, "center": [0, 0, 40], "operation": "new", "body_name": "Trim"})).unwrap();
    let cut = s.execute("solid.combine", &json!({"target": "Posts", "tools": ["Trim"], "operation": "cut"})).unwrap();
    // Each post loses its top 5 mm: 10 x 10 x 25 left of each.
    let posts = 2.0 * 10.0 * 10.0 * 25.0;
    assert_eq!(cut["bodies"], json!(["Case", "Posts"]), "{cut}");
    assert_eq!(cut["created_bodies"], json!([]), "{cut}");
    assert_eq!(s.model.state().body("Posts").map(|b| b.body.shell_count()), Some(2));
    assert!((volume(&s, "Posts") - posts).abs() < 1e-6 * posts, "{}", volume(&s, "Posts"));
    let join = s.execute("solid.combine", &json!({"target": "Case", "tools": ["Posts"], "operation": "join"})).unwrap();
    assert_eq!(join["bodies"], json!(["Case"]), "{join}");
    assert_eq!(join["removed_bodies"], json!(["Posts"]), "{join}");
    let want = 60.0 * 40.0 * 10.0 + posts;
    let got = volume(&s, "Case");
    println!("posts joined while apart, cut, joined: Case {got:.6} mm³, expected {want:.6}");
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");

    // A cut through the middle of the case still leaves two bodies, and says it made the second.
    s.execute("solid.box", &json!({"length": 2, "width": 80, "height": 80, "center": [0, 0, 20], "operation": "new", "body_name": "Saw"})).unwrap();
    let saw = s.execute("solid.combine", &json!({"target": "Case", "tools": ["Saw"], "operation": "cut"})).unwrap();
    // `bodies` lists them in the kernel's order. The piece whose least face name sorts first keeps
    // the name (`lineage::order_lumps`): the +x half, whose `F1:+x` sorts before the -x half's
    // least, its piece of the case's `F1:+y`.
    let mut bodies: Vec<&str> = saw["bodies"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    bodies.sort_unstable();
    assert_eq!(bodies, ["Case", "Case (1)"], "{saw}");
    assert_eq!(saw["created_bodies"], json!(["Case (1)"]), "{saw}");
    let low_x = |name: &str| s.model.state().body(name).unwrap().mesh().bounds().min.x;
    assert!((low_x("Case") - 1.0).abs() < 1e-9 && (low_x("Case (1)") + 30.0).abs() < 1e-9);
    let half = (60.0 - 2.0) / 2.0 * 40.0 * 10.0 + posts / 2.0;
    for name in ["Case", "Case (1)"] {
        assert!((volume(&s, name) - half).abs() < 1e-6 * half, "{name}: {} vs {half}", volume(&s, name));
    }
}

/// Designs saved before that fix combined the pieces the cut split off: tools "Posts" and
/// "Posts (1)", or "Posts (1)" joined back into "Posts". The cut keeps them in "Posts" now, so
/// the combine takes "Posts" and says the piece is part of it, instead of failing on
/// "Posts (1)" and leaving both posts off the case. Any other missing body still fails.
#[test]
fn combine_of_a_piece_a_cut_used_to_split_off_takes_the_body_it_is_part_of() {
    let posts_cut = || {
        let mut s = Session::default();
        let post = |name: &str, x: f64| json!({"length": 10, "width": 10, "height": 30, "center": [x, 0, 25], "operation": "new", "body_name": name});
        s.execute("solid.box", &json!({"length": 60, "width": 40, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Case"}))
            .unwrap();
        s.execute("solid.box", &post("Posts", -20.0)).unwrap();
        s.execute("solid.box", &post("Post2", 20.0)).unwrap();
        s.execute("solid.combine", &json!({"target": "Posts", "tools": ["Post2"], "operation": "join"})).unwrap();
        s.execute("solid.box", &json!({"length": 80, "width": 20, "height": 10, "center": [0, 0, 40], "operation": "new", "body_name": "Trim"}))
            .unwrap();
        s.execute("solid.combine", &json!({"target": "Posts", "tools": ["Trim"], "operation": "cut"})).unwrap();
        s
    };
    let (case, posts) = (60.0 * 40.0 * 10.0, 2.0 * 10.0 * 10.0 * 25.0);
    let close = |got: f64, want: f64| assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
    let piece = Some("`Posts (1)` is part of `Posts` now");

    let mut s = posts_cut();
    s.execute("solid.combine", &json!({"target": "Case", "tools": ["Posts", "Posts (1)"], "operation": "join"})).unwrap();
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let join = reopened.model.results.last().unwrap();
    assert_eq!((join.error.as_deref(), join.warning.as_deref()), (None, piece));
    assert_eq!(reopened.model.state().bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Case"]);
    close(volume(&reopened, "Case"), case + posts);

    let mut s = posts_cut();
    s.execute("solid.combine", &json!({"target": "Posts", "tools": ["Posts (1)"], "operation": "join"})).unwrap();
    assert_eq!(s.model.results.last().unwrap().warning.as_deref(), piece);
    close(volume(&s, "Posts"), posts);

    // A slab through both posts, 10 mm of each, cut by both pieces.
    let mut s = posts_cut();
    s.execute("solid.box", &json!({"length": 80, "width": 20, "height": 10, "center": [0, 0, 25], "operation": "new", "body_name": "Slab"})).unwrap();
    s.execute("solid.combine", &json!({"target": "Slab", "tools": ["Posts", "Posts (1)"], "operation": "cut"})).unwrap();
    assert_eq!(s.model.results.last().unwrap().warning.as_deref(), piece);
    close(volume(&s, "Slab"), 80.0 * 20.0 * 10.0 - 2.0 * 10.0 * 10.0 * 10.0);

    for (target, tools, operation) in [
        ("Case", json!(["Posts (1)"]), "join"),
        ("Case", json!(["Posts", "Ghost (1)"]), "join"),
        ("Case", json!(["Posts", "Posts (0)"]), "join"),
        ("Posts", json!(["Posts (1)"]), "cut"),
    ] {
        let r = posts_cut().execute("solid.combine", &json!({"target": target, "tools": tools, "operation": operation}));
        assert!(r.is_err(), "{target} {operation} {tools}: {r:?}");
    }
}

/// A triangular prism and a thin slab sharing their bottom plane, the prism's edge in that plane
/// lying in the slab's end face, the slab's corners rounded nearly to half its width (so its flat
/// end is narrower than the prism).
#[test]
fn prism_edge_in_a_round_cornered_slab_end_face_joins() {
    for r in [4.5, 4.9] {
        let mut s = Session::default();
        let mut run = |c: &str, p: Value| s.execute(c, &p).unwrap();
        run("sketch.create", json!({"plane": {"origin": [-45.6, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}}));
        run("sketch.line", json!({"points": [[-63, 0], [-3, 0], [-63, 60]], "closed": true}));
        run("sketch.finish", json!({}));
        run("solid.extrude", json!({"distance": 1.2, "operation": "new", "body_name": "Prism"}));
        run("sketch.create", json!({"plane": "XY"}));
        run("sketch.rectangle.rounded", json!({"p0": [-50, -67], "p1": [-40, -3], "radius": r}));
        run("sketch.finish", json!({}));
        run("solid.extrude", json!({"distance": 0.4, "operation": "new", "body_name": "Slab"}));
        let (prism, slab) = (volume(&s, "Prism"), volume(&s, "Slab"));
        s.execute("solid.combine", &json!({"target": "Prism", "tools": ["Slab"], "operation": "join"})).unwrap();
        let join = s.model.results.last().unwrap();
        assert_eq!(join.error.as_deref(), None, "r {r}");
        // The prism inside the slab: 1.2 wide, 0.4 high and 60 long, less the 0.4 x 0.4 triangle the
        // sloped face leaves empty at the slab's end.
        let shared = 1.2 * 0.4 * 60.0 - 1.2 * 0.4 * 0.4 / 2.0;
        let got = volume(&s, "Prism");
        assert!((got - (prism + slab - shared)).abs() < 1e-3 * got, "r {r}: {got} vs {}", prism + slab - shared);
    }
}

/// A cut from a face up to the plate's top face, its tool's end lying in that face: the boolean
/// of the bodies as they lie left the hole closed by a skin of no thickness (the plate's top face
/// whole over the hole, the tool's end face under it), which no volume shows and which was kept.
/// The hole is open, and the cut takes the whole tool: the circle of radius 3.04 the two
/// profiles make, from the barrel cut's floor at z 1.6 up to the plate's top at z 2.8.
#[test]
fn cut_up_to_the_plate_top_face_opens_the_hole() {
    let mut script: Value = serde_json::from_str(include_str!("../../../examples/regressions/cut_up_to_face_leaves_skin.json")).unwrap();
    let s = replay_value(&script);
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    let body = &state.body("Body7").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let skin: Vec<f64> = body
        .faces(1e-4)
        .unwrap()
        .into_iter()
        .filter(|f| f.plane_normal.is_some_and(|n| n.z < -0.999) && (f.centroid.z - 2.8).abs() < 1e-6)
        .map(|f| f.area)
        .collect();
    assert!(skin.is_empty(), "faces at z 2.8 facing down, areas {skin:?} mm²");
    // The same design without the cut.
    script["commands"].as_array_mut().unwrap().pop();
    let before = replay_value(&script);
    let fine =
        |s: &Session| s.model.state().body("Body7").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().volume;
    let (removed, want) = (fine(&before) - fine(&s), std::f64::consts::PI * 3.04 * 3.04 * 1.2);
    assert!((removed - want).abs() < 0.01, "removed {removed} mm³, want {want}");
}

fn replay_value(script: &Value) -> Session {
    let mut s = Session::default();
    s.run_script(script).unwrap();
    s
}

/// A revolved cylinder (r 24) with a sealed spherical void (r 2) on its axis, cut over its bottom
/// edge. The cut came out empty: refused for a box tool, and taken silently, the body gone, for a
/// large rotated box whose bounding box covers the target. Both keep the body and its void, and an
/// intersection with the box keeps the segment.
#[test]
fn cut_beside_a_sealed_void_keeps_the_body() {
    use std::f64::consts::PI;
    let r: f64 = 24.0;
    // The circle's segment beyond a chord at `d` from the centre, and its integral over `d`.
    let segment = |d: f64| r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let wedge = |d: f64| r * r * (d * (d / r).acos() - (r * r - d * d).sqrt()) + (r * r - d * d).powf(1.5) / 3.0;
    let sphere = 4.0 / 3.0 * PI * 8.0;
    let combine = |z: [f64; 2], tool: &[(&str, Value)], operation: &str| {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "YZ"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, z[0]], "p1": [r, z[1]]})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.revolve", &json!({"axis": "y", "operation": "new", "body_name": "B"})).unwrap();
        let mid = (z[0] + z[1]) / 2.0;
        s.execute("solid.sphere", &json!({"radius": 2, "center": [0, 0, mid], "operation": "new", "body_name": "V"})).unwrap();
        s.execute("solid.combine", &json!({"target": "B", "tools": ["V"], "operation": "cut"})).unwrap();
        for (command, params) in tool {
            s.execute(command, params).unwrap();
        }
        s.execute("solid.combine", &json!({"target": "B", "tools": ["K"], "operation": operation})).unwrap_or_else(|e| panic!("{e}"));
        let state = s.model.state();
        let body = &state.body("B").unwrap().body;
        assert_eq!(body.shell_count(), if operation == "cut" { 2 } else { 1 });
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        // The revolved side's mesh stays 0.3 mm³ under its cylinder at any finer tolerance.
        body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-5)).unwrap().measure().volume
    };
    // A box over the bottom edge: the segment beyond x = 15, 5 high.
    let edge_box = [("solid.box", json!({"length": 15, "width": 60, "height": 10, "corner": [15, -30, -5], "operation": "new", "body_name": "K"}))];
    let got = combine([0.0, 60.0], &edge_box, "cut");
    let want = PI * r * r * 60.0 - sphere - 5.0 * segment(15.0);
    println!("box beside a void: {got:.6} mm³, expected {want:.6}");
    assert!((got - want).abs() < 1e-5 * want, "{got} vs {want}");
    let got = combine([0.0, 60.0], &edge_box, "intersect");
    let want = 5.0 * segment(15.0);
    println!("box beside a void, intersected: {got:.6} mm³, expected {want:.6}");
    assert!((got - want).abs() < 1e-5 * want, "{got} vs {want}");
    // A 600 mm box rotated 45° about Y, its face on x - z = 116: the wedge beyond chords from
    // x = 10 (z = -106) to x = 24 (z = -92).
    let shift = (300.0 + 116.0 / 2f64.sqrt()) / 2f64.sqrt();
    let got = combine(
        [-106.0, -43.5],
        &[
            ("solid.box", json!({"length": 600, "width": 600, "height": 600, "center": [0, 0, 0], "operation": "new", "body_name": "K"})),
            ("solid.move", json!({"bodies": ["K"], "axis": [0, 1, 0], "angle": -45})),
            ("solid.move", json!({"bodies": ["K"], "translate": [shift, 0, -shift]})),
        ],
        "cut",
    );
    let want = PI * r * r * 62.5 - sphere - (wedge(r) - wedge(10.0));
    println!("rotated box beside a void: {got:.6} mm³, expected {want:.6}");
    assert!((got - want).abs() < 1e-5 * want, "{got} vs {want}");
}

/// The reference mesh of loft bodies joined into a bike frame covers each face once: within 1%
/// of the area of a mesh at a tenth of the tolerance. The trimmed loft sides were meshed with a
/// flat sheet through the down tube (Delaunay in raw parameters on a strip stretched twentyfold:
/// two and a half times the area), and the seat tube's side with slivers across it (a boundary
/// polyline hooking back past its vertex dropped a trimming curve's constraints).
#[test]
fn joined_loft_frames_reference_mesh_covers_each_face_once() {
    for (script, name) in [
        (include_str!("../../../examples/regressions/loft_join_bracket_hull_triangle.json"), "DT2"),
        (include_str!("../../../examples/regressions/front_triangle_hull_triangle.json"), "FrontTriangle"),
    ] {
        let s = replay(script);
        let state = s.model.state();
        let body = &state.body(name).unwrap().body;
        let per_face = |m: &solidcraft_geom::Mesh| {
            let mut a = vec![0.0; body.face_count()];
            for (t, f) in m.triangles.iter().zip(&m.tri_face) {
                let [p0, p1, p2] = t.map(|i| m.positions[i as usize]);
                a[*f as usize] += (p1 - p0).cross(p2 - p0).len() / 2.0;
            }
            a
        };
        let size = body.size();
        let mesh = per_face(&body.mesh(&solidcraft_kernel::MeshTolerance { chord: size * 3e-4, angle: Some(12f64.to_radians()) }).unwrap());
        let fine = per_face(&body.mesh(&solidcraft_kernel::MeshTolerance { chord: size * 3e-5, angle: Some(3f64.to_radians()) }).unwrap());
        for (fi, (m, f)) in mesh.iter().zip(&fine).enumerate() {
            assert!((m - f).abs() <= 0.01 * f.max(1.0), "{name} face {fi}: mesh {m:.0} mm², finer mesh {f:.0} mm²");
        }
    }
}

/// The block of #2040 with `extra` run after it.
fn rounded_block_and(extra: Value) -> Session {
    let mut script: Value = serde_json::from_str(include_str!("../../../examples/regressions/extrude_face_rounded_by_fillets.json")).unwrap();
    script["commands"].as_array_mut().unwrap().push(extra);
    replay(&script.to_string())
}

/// #2040: the slanted face of a block whose ends fillets round, extruded as a join. Its outline's
/// quarter-ellipses come into the face's sketch as splines a hair off the body's edges, and the
/// join failed with "boolean Union: no result"; it now sweeps the body's own face. It adds the
/// face's area times its height: a 10 x 12 face less four corners of (1 - pi/4) r * r*sqrt(2).
#[test]
fn extrude_join_from_a_face_rounded_by_fillets() {
    let at = json!([16.191269086108846, -6.702792508916929, 4.676550460347439]);
    let before = volume(&replay(include_str!("../../../examples/regressions/extrude_face_rounded_by_fillets.json")), "Body1");
    let r = 3.8_f64;
    let area = 10.0 * 12.0 - 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 2f64.sqrt();
    let joined = rounded_block_and(json!({"command": "solid.extrude", "params": {"face": at, "distance": 5, "operation": "join"}}));
    let bodies = &joined.model.state().bodies;
    assert_eq!(bodies.len(), 1, "the join makes no second body");
    assert!(bodies[0].body.validity().is_empty(), "{:?}", bodies[0].body.validity());
    let v = volume(&joined, "Body1");
    assert!((v - (before + area * 5.0)).abs() < 0.05, "join {v} expected {}", before + area * 5.0);
}

/// #2040: a cut 3 mm into the slanted face of the block rounded along its top only (its bottom
/// edge left sharp). The walls from the bottom edge go into the body and the others only touch
/// it, along edges with curved faces beside them: the cut failed with "boolean Cut: no result".
/// In the face's plane the body's section at depth t is the block's section (10 wide, h high, its
/// top corners rounded) moved down the face by t: the cut takes
/// √2 ∫₀³ (10 (h − t/√2) − 2 (1 − π/4) r²) dt.
#[test]
fn extrude_cut_into_a_face_beside_rounds() {
    let at = json!([16.191269086108846, -6.702792508916929, 4.676550460347439]);
    let top_only = |extra: Option<Value>| {
        let mut script: Value = serde_json::from_str(include_str!("../../../examples/regressions/extrude_face_rounded_by_fillets.json")).unwrap();
        let commands = script["commands"].as_array_mut().unwrap();
        let fillet = commands.last_mut().unwrap();
        fillet["params"]["edges"] = json!([[10, -10, 8.485281374238571], [10, 0, 8.485281374238571]]);
        commands.extend(extra);
        replay(&script.to_string())
    };
    let before = volume(&top_only(None), "Body1");
    let cut = top_only(Some(json!({"command": "solid.extrude", "params": {"face": at, "distance": 3, "direction": "negative", "operation": "cut"}})));
    let bodies = &cut.model.state().bodies;
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].body.validity().is_empty(), "{:?}", bodies[0].body.validity());
    let (h, r, d, w) = (8.485281374238571_f64, 3.8_f64, 3.0_f64, 10.0_f64);
    let corner = (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
    let taken = 2f64.sqrt() * w * h * d - w * d * d / 2.0 - 2.0 * 2f64.sqrt() * corner * d;
    let v = before - volume(&cut, "Body1");
    assert!((v - taken).abs() < 0.05, "took {v}, expected {taken}");
}

/// #2040: a cut 2 mm into the top face of the block rounded at its ends, the strip 10 − 2r wide
/// between the rounds. Below the face the rounds stand out past its outline, so a slab over the
/// whole body would take them too: the cut takes only what lies over the face, at height z from
/// x = 0 to the slanted face at x = x0 + z.
#[test]
fn extrude_cut_into_a_rounded_block_top() {
    let (h, r, x0, d) = (8.485281374238571_f64, 3.8_f64, 11.514718625761429_f64, 2.0_f64);
    let before = volume(&replay(include_str!("../../../examples/regressions/extrude_face_rounded_by_fillets.json")), "Body1");
    let cut = rounded_block_and(
        json!({"command": "solid.extrude", "params": {"face": [5, -5, h], "distance": d, "direction": "negative", "operation": "cut"}}),
    );
    let bodies = &cut.model.state().bodies;
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].body.validity().is_empty(), "{:?}", bodies[0].body.validity());
    let taken = (10.0 - 2.0 * r) * (x0 * d + (h * h - (h - d) * (h - d)) / 2.0);
    let v = before - volume(&cut, "Body1");
    assert!((v - taken).abs() < 0.05, "took {v}, expected {taken}");
}

#[test]
fn empty_intersection_is_refused_without_changing_the_design() -> crate::Result<()> {
    for command in ["solid.extrude", "solid.box", "solid.combine"] {
        for x in [5, 20] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Base"}))?;
            s.execute("solid.box", &json!({"corner":[x,0,0],"length":10,"width":10,"height":10,"body_name":"Tool","operation":"new"}))?;
            s.execute("sketch.create", &json!({"plane":"XY"}))?;
            s.execute("sketch.rectangle.two_point", &json!({"p0":[x,0],"p1":[x+10,10]}))?;
            s.execute("sketch.finish", &json!({}))?;
            let before = s.doc.to_json();
            let params = match command {
                "solid.combine" => json!({"target":"Base","tools":["Tool"],"operation":"intersect"}),
                "solid.box" => json!({"corner":[x,0,0],"length":10,"width":10,"height":10,"operation":"intersect","targets":["Base"]}),
                _ => json!({"distance":10,"operation":"intersect","targets":["Base"]}),
            };
            let result = s.execute(command, &params);
            if x == 20 {
                let error = result.err().ok_or_else(|| crate::EngineError::Other(format!("{command} accepted an empty intersection")))?;
                assert!(
                    error.to_string().contains("the intersection is empty: nothing of `Base` would remain; check that the profile overlaps the body"),
                    "{error}"
                );
                assert_eq!(s.doc.to_json(), before);
                assert_eq!(s.model.state().bodies.len(), 2);
                assert!((volume(&s, "Base") - 1000.0).abs() < 1e-6);
                assert!((volume(&s, "Tool") - 1000.0).abs() < 1e-6);
            } else {
                result?;
                assert!((volume(&s, "Base") - 500.0).abs() < 1e-6);
            }
        }
    }
    Ok(())
}

#[test]
fn empty_intersection_in_a_saved_design_fails_and_keeps_the_target() -> crate::Result<()> {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Base"}))?;
    s.execute("solid.box", &json!({"length":10,"width":10,"height":40,"body_name":"Tall","operation":"new"}))?;
    s.execute("sketch.create", &json!({"plane":"XY","offset":5}))?;
    s.execute("sketch.rectangle.two_point", &json!({"p0":[0,0],"p1":[10,10]}))?;
    s.execute("sketch.finish", &json!({}))?;
    let result = s.execute("solid.extrude", &json!({"distance":10,"operation":"intersect","targets":["Tall","Base"]}))?;
    let id = result["feature"].as_u64().ok_or_else(|| crate::EngineError::Other("missing feature id".into()))?;
    assert!((volume(&s, "Base") - 500.0).abs() < 1e-6);
    let mut doc = solidcraft_doc::Document::from_json(&s.doc.to_json())?;
    if let Some(f) = doc.feature_mut(id)
        && let solidcraft_doc::FeatureKind::Extrude { extent, .. } = &mut f.kind
    {
        extent.start_offset = Some("20 mm".into());
    }
    let reopened = Session::new(solidcraft_doc::Document::from_json(&doc.to_json())?);
    assert!(reopened.model.result(id).and_then(|r| r.error.as_ref()).is_some_and(|e| e.contains("the intersection is empty")));
    assert_eq!(reopened.model.state().bodies.len(), 2);
    assert!((volume(&reopened, "Tall") - 4000.0).abs() < 1e-6);
    assert!((volume(&reopened, "Base") - 1000.0).abs() < 1e-6);
    Ok(())
}

#[test]
fn intersect_with_every_body_still_drops_the_ones_it_misses() -> crate::Result<()> {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Near"}))?;
    s.execute("solid.box", &json!({"corner":[30,0,0],"length":10,"width":10,"height":10,"body_name":"Far","operation":"new"}))?;
    s.execute("sketch.create", &json!({"plane":"XY"}))?;
    s.execute("sketch.rectangle.two_point", &json!({"p0":[0,0],"p1":[5,10]}))?;
    s.execute("sketch.finish", &json!({}))?;
    s.execute("solid.extrude", &json!({"distance":10,"operation":"intersect"}))?;
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(&s, "Near") - 500.0).abs() < 1e-6);
    Ok(())
}

/// #451: a spherical shell (r 18.5 to 40, revolved on XY) cut from a block, or joined with it.
/// Swept with its two radial lines, the shell had zero-area faces on the axis joining the outer
/// sphere to the inner one, and a cut through the inner sphere's pole failed. As a solid with a
/// void, the boolean took its shells one at a time: a cut through both spheres kept only the piece
/// in the hollow, and a join failed.
#[test]
fn spherical_shell_cuts_and_joins_a_block_crossing_its_hollow() {
    let r: f64 = 18.5;
    // ∫∫ √(R² − y² − z²) dy dz over the blocks' 8 × 8 section, the inner integral in closed form,
    // the outer by Simpson's rule.
    let under = |big: f64| {
        let inner = |y: f64| {
            let a2 = big * big - y * y;
            let f = |z: f64| 0.5 * (z * (a2 - z * z).sqrt() + a2 * (z / a2.sqrt()).asin());
            f(4.0) - f(-4.0)
        };
        let n = 200;
        let h = 8.0 / n as f64;
        (0..n / 2).map(|i| -4.0 + 2.0 * i as f64 * h).map(|y| inner(y) + 4.0 * inner(y + h) + inner(y + 2.0 * h)).sum::<f64>() * h / 3.0
    };
    // From x = 8 to the inner sphere, and from the outer sphere to x = 50.
    let (hollow, beyond) = (under(r) - 8.0 * 64.0, 50.0 * 64.0 - under(40.0));
    let shell = 4.0 / 3.0 * std::f64::consts::PI * (40.0f64.powi(3) - r.powi(3));
    for (x0, length, operation, want) in [
        (-4.0, 8.0, "cut", 512.0),
        (8.0, 12.0, "cut", hollow),
        (8.0, 42.0, "cut", hollow + beyond),
        (8.0, 12.0, "join", shell + hollow),
        (8.0, 42.0, "join", shell + hollow + beyond),
    ] {
        let mut s = Session::default();
        let mut run = |c: &str, p: Value| s.execute(c, &p).unwrap();
        run("solid.box", json!({"corner": [x0, -4, -4], "length": length, "width": 8, "height": 8, "body_name": "Block"}));
        run("sketch.create", json!({"plane": "XY", "name": "Ring"}));
        run("sketch.line", json!({"points": [[-40, 0], [40, 0]]}));
        run("sketch.arc.center_point", json!({"center": [0, 0], "start": [40, 0], "end": [-40, 0]}));
        run("sketch.arc.center_point", json!({"center": [0, 0], "start": [r, 0], "end": [-r, 0]}));
        run("sketch.finish", json!({}));
        run("solid.revolve", json!({"sketch": "Ring", "profiles": [{"point": [0, 30]}], "axis": "l1", "operation": operation, "targets": ["Block"]}));
        // A cut through both spheres leaves two pieces, one each side of the shell.
        let state = s.model.state();
        let got: f64 = state.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum();
        println!("#451 {operation} of the block from x = {x0} to {}: {got:.6} mm³ in {} bodies, expected {want:.6}", x0 + length, state.bodies.len());
        assert!((got - want).abs() < 1e-4 * want, "{operation} from x = {x0} to {}: {got} vs {want}", x0 + length);
        assert!(state.bodies.iter().all(|b| b.body.validity().is_empty()));
    }
}

/// #4434: a Join whose profile stands apart from the body still joins it, as a second piece of
/// `Body1`, and says so; scripts read it from the reply as History shows it. A join that meets
/// the body, one that closes a cavity (a void is not a piece) and a Combine of bodies chosen
/// apart say nothing.
#[test]
fn join_that_meets_no_body_keeps_the_piece_and_says_so() {
    let apart = "`Body1` is now in 2 separate pieces";
    let two_squares = || {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY", "name": "Sketch1"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [20, 20]})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [40, 0], "p1": [60, 20]})).unwrap();
        s.execute(
            "solid.extrude",
            &json!({"sketch": "Sketch1", "profiles": [{"point": [10, 10]}], "distance": 10, "operation": "new", "name": "Extrude1"}),
        )
        .unwrap();
        s
    };
    let mut s = two_squares();
    let join = s
        .execute(
            "solid.extrude",
            &json!({"sketch": "Sketch1", "profiles": [{"point": [50, 10]}], "distance": 10, "operation": "join", "name": "Extrude2"}),
        )
        .unwrap();
    assert!(join["warning"].as_str().is_some_and(|w| w.contains(apart)), "{join}");
    assert_eq!(s.model.results.last().and_then(|r| r.warning.as_deref()), join["warning"].as_str());
    assert_eq!(s.model.state().bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Body1"]);
    assert!((volume(&s, "Body1") - 8000.0).abs() < 1e-6, "{}", volume(&s, "Body1"));

    // Both profiles at once, one on the body and one apart: joined as one tool, still told.
    let mut s = two_squares();
    let both = s
        .execute("solid.extrude", &json!({"sketch": "Sketch1", "profiles": [{"point": [10, 10]}, {"point": [50, 10]}], "distance": 5, "start_offset": 10, "operation": "join"}))
        .unwrap();
    assert!(both["warning"].as_str().is_some_and(|w| w.contains(apart)), "{both}");

    // A box joined apart from the body, then one bridging both pieces: only the first is told.
    let mut s = two_squares();
    let box_apart = s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "center": [10, 40, 5], "operation": "join"})).unwrap();
    assert!(box_apart["warning"].as_str().is_some_and(|w| w.contains(apart)), "{box_apart}");
    let bridge = s.execute("solid.box", &json!({"length": 10, "width": 30, "height": 10, "center": [10, 25, 5], "operation": "join"})).unwrap();
    assert!(bridge["warning"].is_null(), "{bridge}");

    // A lid joined on a cup closes a cavity: one piece with a void, nothing to say.
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "center": [0, 0, 5], "operation": "new"})).unwrap();
    s.execute("solid.box", &json!({"length": 16, "width": 16, "height": 10, "center": [0, 0, 6], "operation": "cut"})).unwrap();
    let lid = s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 2, "center": [0, 0, 11], "operation": "join"})).unwrap();
    assert_eq!(s.model.state().body("Body1").map(|b| b.body.shell_count()), Some(2));
    assert!(lid["warning"].is_null(), "{lid}");

    // Combine joins the bodies it is given, apart or not.
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "A"})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "center": [30, 0, 5], "operation": "new", "body_name": "B"})).unwrap();
    let combine = s.execute("solid.combine", &json!({"target": "A", "tools": ["B"], "operation": "join"})).unwrap();
    assert!(combine["warning"].is_null(), "{combine}");
}
