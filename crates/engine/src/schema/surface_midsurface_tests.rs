use serde_json::{Value, json};

use super::{MidsurfaceAnalysis, MidsurfaceAnalyzeParams, MidsurfaceParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<MidsurfaceParams>(),
        &[
            "body",
            "body_name",
            "body_names",
            "carry_holes",
            "exclude",
            "include_review_pairs",
            "keep_original",
            "max_thickness",
            "min_aspect",
            "min_thickness",
            "name",
            "pairs",
            "participants",
            "stitch",
            "suppress_holes_below",
        ],
    );
    assert_keys(input_schema_for::<MidsurfaceAnalyzeParams>(), &["body", "max_thickness", "min_aspect", "min_thickness"]);
}

/// A plate 40 x 30 x 2 named `Plate`, at `level`.
fn plate(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 2, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s
}

/// Calls that fail today give the same message in strict: a missing or misnamed body, a body the
/// design lacks, and limits that don't evaluate or have the wrong unit.
#[test]
fn strict_keeps_todays_errors() {
    for cmd in ["surface.midsurface", "surface.midsurface_analyze"] {
        for p in [
            json!({}),
            json!({"body": 5}),
            json!({"body": null}),
            json!({"body": "Nope"}),
            json!({"body": "Plate", "max_thickness": "nonsense"}),
            json!({"body": "Plate", "max_thickness": "30 deg"}),
            json!({"body": "Plate", "min_thickness": "nonsense"}),
        ] {
            let error = |level: ParamCheck| plate(level).execute(cmd, &p).err().map(|e| e.to_string());
            let off = error(ParamCheck::Off);
            assert!(off.is_some(), "{cmd} {p} should fail today");
            assert_eq!(error(ParamCheck::Strict), off, "{cmd} {p}");
        }
    }
}

/// A limit given that is not a number or an expression is refused, not read as absent and
/// defaulted; an empty `pairs` list chooses nothing and is refused, not read as "the confident
/// ones" (which would build them, and with `keep_original: false` remove the solid). Even with
/// the parameter check off, and with nothing added to the design.
#[test]
fn unreadable_limits_and_an_empty_pair_list_are_refused_not_defaulted() {
    for p in [
        json!({"body": "Plate", "max_thickness": ""}),
        json!({"body": "Plate", "max_thickness": "5 mm", "min_thickness": " "}),
        json!({"body": "Plate", "max_thickness": "5 mm", "suppress_holes_below": ""}),
        json!({"body": "Plate", "max_thickness": "5 mm", "suppress_holes_below": true}),
        json!({"body": "Plate", "max_thickness": "5 mm", "pairs": [], "keep_original": false}),
        // A pair of three points, or of one, is refused before its points are read.
        json!({"body": "Plate", "max_thickness": "5 mm", "pairs": [[[0, 0, 0], [0, 0, 1], [0, 0, 2]]]}),
        json!({"body": "Plate", "max_thickness": "5 mm", "pairs": [[[0, 0, 0]]]}),
        // A true/false option given as anything but true or false is refused, not read as absent:
        // "false" would otherwise build one sewn body, or (`keep_original`) keep the solid.
        json!({"body": "Plate", "max_thickness": "5 mm", "stitch": "false"}),
        json!({"body": "Plate", "max_thickness": "5 mm", "carry_holes": 0}),
        json!({"body": "Plate", "max_thickness": "5 mm", "keep_original": "false"}),
        json!({"body": "Plate", "max_thickness": "5 mm", "include_review_pairs": "yes"}),
    ] {
        let mut s = plate(ParamCheck::Off);
        let before = s.doc.features.len();
        assert!(s.execute("surface.midsurface", &p).is_err(), "{p}");
        assert_eq!(s.doc.features.len(), before, "{p}");
        assert!(s.model.state().body("Plate").is_some(), "{p}");
    }
    // The pair list is read before the body is scanned: an empty one is refused as such even
    // where the body could not be analysed.
    let mut s = plate(ParamCheck::Off);
    let error = s.execute("surface.midsurface", &json!({"body": "Nope", "pairs": []})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`pairs` is empty"), "{error}");
    // So are the true/false options.
    let mut s = plate(ParamCheck::Off);
    let error = s.execute("surface.midsurface", &json!({"body": "Nope", "stitch": "false"})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`stitch` must be true or false"), "{error}");
    // Left out or null, the defaults stand: the plate's one pair is built, one surface body, and
    // the plate is kept.
    let mut s = plate(ParamCheck::Off);
    let reply = s
        .execute(
            "surface.midsurface",
            &json!({"body": "Plate", "max_thickness": "5 mm", "min_thickness": null, "pairs": null, "stitch": null, "keep_original": null}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let created: Vec<&str> =
        reply.get("created_bodies").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    assert_eq!(created.len(), 1, "{reply}");
    assert!(created.iter().all(|n| s.model.state().body(n).is_some_and(|b| b.body.is_surface())), "{reply}");
    assert!(s.model.state().body("Plate").is_some(), "{reply}");
}

/// A plate 40 x 30 x 10 spans sqrt(1200) / 10 = 3.46 times its thickness: below the default 5
/// it needs a decision, but with `min_aspect: 0` it is sure and built. The feature keeps the 0,
/// so its rebuild judges by the same limit, and the warning asks for no decision.
#[test]
fn the_aspect_limit_asked_for_is_stored_and_the_warning_asks_no_decision() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    let reply =
        s.execute("surface.midsurface", &json!({"body": "Plate", "max_thickness": "11 mm", "min_aspect": 0})).unwrap_or_else(|e| panic!("{e}"));
    let warning = reply.get("warning").and_then(Value::as_str).unwrap_or("");
    assert!(!warning.contains("need a decision"), "{reply}");
    let stored = s.doc.features.iter().rev().find_map(|f| match &f.kind {
        solidcraft_doc::FeatureKind::SurfaceMidsurface { min_aspect, .. } => Some(*min_aspect),
        _ => None,
    });
    assert_eq!(stored, Some(Some(0.0)), "{reply}");

    // The rebuild's warning judges the pairs left out by the stored limit. Up to 41 mm the
    // block's three pairs of opposite faces are walls: 10 apart over 40 x 30 (aspect 3.46), 30
    // apart over 40 x 10 (sqrt(400) / 30 = 0.67) and 40 apart over 30 x 10 (sqrt(300) / 40 =
    // 0.43). With the 10 mm pair chosen, the other two are below an aspect of 5 and need a
    // decision; with 0 they are sure, and the warning asks for none.
    let analysis = s
        .execute("surface.midsurface_analyze", &json!({"body": "Plate", "max_thickness": "41 mm", "min_aspect": 0}))
        .unwrap_or_else(|e| panic!("{e}"));
    let pairs = analysis.get("pairs").and_then(Value::as_array).cloned().unwrap_or_default();
    assert_eq!(pairs.len(), 3, "{analysis}");
    let wall = pairs
        .iter()
        .find(|p| p.get("thickness").and_then(Value::as_f64).is_some_and(|t| (t - 10.0).abs() < 1e-9))
        .and_then(|p| p.get("points"))
        .cloned()
        .unwrap_or_else(|| panic!("{analysis}"));
    for (aspect, asks) in [(5.0, true), (0.0, false)] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
        let reply = s
            .execute("surface.midsurface", &json!({"body": "Plate", "max_thickness": "41 mm", "min_aspect": aspect, "pairs": [wall]}))
            .unwrap_or_else(|e| panic!("{e}"));
        let warning = reply.get("warning").and_then(Value::as_str).unwrap_or("");
        assert_eq!(warning.contains("2 pairs need a decision"), asks, "min_aspect {aspect}: {reply}");
    }
}

/// A plate placed 100 along x (a body offset) gives its midsurface where the plate is: the sheet
/// is read in the plate's own frame and takes its placement, so in the world it spans the plate's
/// 40 x 30 and lies halfway up its 2, not at the origin.
#[test]
fn a_placed_plates_midsurface_is_where_the_plate_is() {
    use solidcore_record::Vec3;
    use solidcraft_geom::MeshGeom;
    // A body's own placement applies within a component (a part), as an inserted part's does.
    let mut s = Session::default();
    s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 2, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s.doc_mut().body_offsets.insert("Plate".into(), solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    let reply = s.execute("surface.midsurface", &json!({"body": "Plate", "max_thickness": "5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let name = reply.get("created_bodies").and_then(|c| c.get(0)).and_then(Value::as_str).unwrap_or_else(|| panic!("{reply}")).to_string();
    let world = s.world_state().unwrap();
    let bounds = |n: &str| world.bodies.iter().find(|b| b.definition_name() == n).map(|b| b.mesh().bounds()).unwrap_or_else(|| panic!("{n}"));
    let (plate, sheet) = (bounds("Plate"), bounds(&name));
    assert!((plate.max.x - plate.min.x - 40.0).abs() < 1e-6 && plate.min.x > 50.0, "{plate:?}");
    let mid_z = 0.5 * (plate.min.z + plate.max.z);
    for (got, want) in [
        (sheet.min.x, plate.min.x),
        (sheet.max.x, plate.max.x),
        (sheet.min.y, plate.min.y),
        (sheet.max.y, plate.max.y),
        (sheet.min.z, mid_z),
        (sheet.max.z, mid_z),
    ] {
        assert!((got - want).abs() < 1e-6, "{sheet:?} against {plate:?}");
    }
    // The plate moved on to 200 along x (only its offset changes): the midsurface is built again
    // and goes with it, 100 further, not left where the plate was.
    s.doc_mut().body_offsets.insert("Plate".into(), solidcraft_doc::rigid(Vec3::new(200.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    let world = s.world_state().unwrap();
    let bounds = |n: &str| world.bodies.iter().find(|b| b.definition_name() == n).map(|b| b.mesh().bounds()).unwrap_or_else(|| panic!("{n}"));
    let (moved, again) = (bounds("Plate"), bounds(&name));
    assert!((moved.min.x - plate.min.x - 100.0).abs() < 1e-6, "{moved:?} against {plate:?}");
    assert!((again.min.x - moved.min.x).abs() < 1e-6 && (again.max.x - moved.max.x).abs() < 1e-6, "{again:?} against {moved:?}");
    // From the root, whose frame is not the part's, the plate's midsurface is refused, as a
    // reduced mesh is: its sheets would be placed by the root, away from the plate.
    s.execute("component.activate", &json!({"component": "root"})).unwrap_or_else(|e| panic!("{e}"));
    let before = s.doc.features.len();
    let error = s.execute("surface.midsurface", &json!({"body": "Plate", "max_thickness": "5 mm"})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("activate the solid's component"), "{error}");
    assert_eq!(s.doc.features.len(), before);
}

/// With no `max_thickness`, the limit suggested lies past the `min_thickness` given. A plate 2
/// thick suggests 2 x 1.5 = 3 when probed from 0; with a minimum of 3.5 the probe takes no wall
/// under 3.5 and finds none, so the scan looks as far as a fifth of the plate's size (more than
/// 3.5) and is not refused for a limit below its minimum.
#[test]
fn a_suggested_limit_lies_past_the_minimum_given() {
    let mut s = plate(ParamCheck::Off);
    let reply = s.execute("surface.midsurface_analyze", &json!({"body": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    let limit = reply.get("max_thickness").and_then(Value::as_f64);
    assert!(limit.is_some_and(|m| (m - 3.0).abs() < 1e-9), "{reply}");
    let reply = s.execute("surface.midsurface_analyze", &json!({"body": "Plate", "min_thickness": "3.5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let limit = reply.get("max_thickness").and_then(Value::as_f64);
    assert!(limit.is_some_and(|m| m > 3.5), "{reply}");
}

/// A sheet whose stored name another body holds is numbered `BodyN` instead. Built from a plate
/// placed 100 along x, it first takes the plate's placement; given a placement of its own (0 along
/// x, 50 up), it is built again in the plate's frame and placed by that alone, so it spans the
/// plate's 0..40 in x again, not 100..140 from a cached sheet that still holds the plate's 100.
#[test]
fn a_numbered_sheets_own_placement_builds_it_again() {
    use solidcore_record::Vec3;
    use solidcraft_geom::MeshGeom;
    let mut s = Session::default();
    s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 2, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s.doc_mut().body_offsets.insert("Plate".into(), solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    s.execute("surface.midsurface", &json!({"body": "Plate", "max_thickness": "5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    // The stored name is now one the plate holds: the sheet falls back to a numbered name.
    let id = s.doc.features.last().map(|f| f.id).unwrap_or_else(|| panic!("no feature"));
    if let Some(f) = s.doc_mut().features.last_mut() {
        f.body_names = vec!["Plate".into()];
    }
    s.refresh();
    let name = s.model.state().bodies.iter().find(|b| b.feature == id).map(|b| b.name.clone()).unwrap_or_else(|| panic!("no sheet"));
    assert!(solidcraft_doc::is_counted_body_name(&name), "{name}");
    let bounds = |s: &Session, n: &str| {
        let world = s.world_state().unwrap();
        world.bodies.iter().find(|b| b.definition_name() == n).map(|b| b.mesh().bounds()).unwrap_or_else(|| panic!("{n}"))
    };
    let (plate, placed) = (bounds(&s, "Plate"), bounds(&s, &name));
    assert!((placed.min.x - plate.min.x).abs() < 1e-6 && (placed.max.x - plate.max.x).abs() < 1e-6, "{placed:?} against {plate:?}");
    s.doc_mut().body_offsets.insert(name.clone(), solidcraft_doc::rigid(Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    let own = bounds(&s, &name);
    let mid_z = 0.5 * (plate.min.z + plate.max.z);
    for (got, want) in [(own.min.x, plate.min.x - 100.0), (own.max.x, plate.max.x - 100.0), (own.min.z, mid_z + 50.0)] {
        assert!((got - want).abs() < 1e-6, "{own:?} against {plate:?}");
    }
}

/// A session that ran `commands`, each a command and its parameters.
fn design(commands: &[(&str, Value)]) -> Session {
    let mut s = Session::default();
    for (cmd, p) in commands {
        s.execute(cmd, p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"));
    }
    s
}

/// `body`'s midsurface built with walls up to 5 mm (and `extra` parameters): its area, from the
/// kernel's measure of the body made, and the reply's warning.
fn midsurface_area(s: &mut Session, body: &str, extra: Value) -> (f64, String) {
    let mut p = json!({"body": body, "max_thickness": "5 mm", "body_name": "Mid"});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    let reply = s.execute("surface.midsurface", &p).unwrap_or_else(|e| panic!("{e}"));
    let warning = reply.get("warning").and_then(Value::as_str).unwrap_or_default().to_string();
    let state = s.model.state();
    let mid = state.body("Mid").unwrap_or_else(|| panic!("no midsurface: {reply}"));
    (solidcraft_kernel::measure(&mid.body).unwrap_or_else(|e| panic!("{e}")).area, warning)
}

/// The analysis reply's joints of one kind.
fn joints_of(reply: &Value, kind: &str) -> Vec<Value> {
    reply
        .get("joints")
        .and_then(Value::as_array)
        .map(|j| j.iter().filter(|x| x.get("kind").and_then(Value::as_str) == Some(kind)).cloned().collect())
        .unwrap_or_default()
}

/// A tube of radii 8 and 10 standing on a plate 60 x 60 x 4, its bore on through the plate: the
/// tube's sheet (radius 9) runs down to the plate's halfway plane z = 2, and the plate's sheet
/// reaches in to radius 9. 3600 - 81 pi + 2 pi 9 (40 - 2) = 3600 + 603 pi.
#[test]
fn a_tube_on_a_plate_is_joined_on_a_circle() {
    let mut s = design(&[
        ("solid.box", json!({"length": 60, "width": 60, "height": 4, "center": [0, 0, 2], "body_name": "Part"})),
        ("solid.cylinder", json!({"radius": 10, "height": 38, "base": [0, 0, 2], "operation": "join", "targets": ["Part"]})),
        ("solid.cylinder", json!({"radius": 8, "height": 42, "base": [0, 0, -1], "operation": "cut", "targets": ["Part"]})),
    ]);
    let analysis = s.execute("surface.midsurface_analyze", &json!({"body": "Part", "max_thickness": "5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let circles = joints_of(&analysis, "circle");
    assert_eq!(circles.len(), 1, "{analysis}");
    let radius = circles[0].get("radius").and_then(Value::as_f64);
    assert!(radius.is_some_and(|r| (r - 9.0).abs() < 1e-9), "{analysis}");
    let centre_z = circles[0].get("centre").and_then(|c| c.get(2)).and_then(Value::as_f64);
    assert!(centre_z.is_some_and(|z| (z - 2.0).abs() < 1e-9), "{analysis}");
    let (area, warning) = midsurface_area(&mut s, "Part", json!({}));
    let expected = 3600.0 + 603.0 * std::f64::consts::PI;
    assert!((area - expected).abs() < expected * 1e-4, "{area} against {expected}");
    assert!(!warning.contains("a joint") && !warning.contains("filled over"), "{warning}");
}

/// A ring of radii 10 to 30, z 18..22, round a tube of radii 8 and 10, z 0..40: a T. The ring's
/// sheet at z = 20 reaches in to radius 9, pi (900 - 81); the tube's two halves run to z = 20,
/// 2 pi 9 40 together. 1539 pi.
#[test]
fn a_ring_round_a_tube_is_a_t_joined_on_a_circle() {
    let mut s = design(&[
        ("solid.cylinder", json!({"radius": 10, "height": 40, "base": [0, 0, 0], "body_name": "Part"})),
        ("solid.cylinder", json!({"radius": 30, "height": 4, "base": [0, 0, 18], "operation": "join", "targets": ["Part"]})),
        ("solid.cylinder", json!({"radius": 8, "height": 42, "base": [0, 0, -1], "operation": "cut", "targets": ["Part"]})),
    ]);
    let analysis = s.execute("surface.midsurface_analyze", &json!({"body": "Part", "max_thickness": "5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(joints_of(&analysis, "circle").len(), 2, "{analysis}");
    let (area, warning) = midsurface_area(&mut s, "Part", json!({}));
    let expected = 1539.0 * std::f64::consts::PI;
    assert!((area - expected).abs() < expected * 1e-4, "{area} against {expected}");
    assert!(!warning.contains("a joint"), "{warning}");
}

/// A plate 40 x 30 x 2 with a round hole of radius 5 through it: the sheet carries the hole,
/// 1200 - 25 pi. With a bore of radius 3 through (20, 15, 1) tilted 30 degrees, the hole is the
/// ellipse the bore cuts the halfway plane in: 1200 - 9 pi / cos 30.
#[test]
fn a_plates_round_and_slanted_holes_are_carried_exactly() {
    let plate = || ("solid.box", json!({"length": 40, "width": 30, "height": 2, "corner": [0, 0, 0], "body_name": "Plate"}));
    let mut s =
        design(&[plate(), ("solid.cylinder", json!({"radius": 5, "height": 4, "base": [20, 15, -1], "operation": "cut", "targets": ["Plate"]}))]);
    let (area, _) = midsurface_area(&mut s, "Plate", json!({}));
    let expected = 1200.0 - 25.0 * std::f64::consts::PI;
    assert!((area - expected).abs() < 0.05, "{area} against {expected}");
    let (sin, cos) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let mut s = design(&[
        plate(),
        (
            "solid.cylinder",
            json!({"radius": 3, "height": 20, "base": [20.0 - 10.0 * sin, 15, 1.0 - 10.0 * cos], "axis": [sin, 0, cos], "operation": "cut", "targets": ["Plate"]}),
        ),
    ]);
    let analysis = s.execute("surface.midsurface_analyze", &json!({"body": "Plate", "max_thickness": "5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let expected = 1200.0 - 9.0 * std::f64::consts::PI / cos;
    let pair_area = analysis.get("pairs").and_then(|p| p.get(0)).and_then(|p| p.get("area")).and_then(Value::as_f64);
    assert!(pair_area.is_some_and(|a| (a - expected).abs() < 1e-6), "{analysis}");
    let (area, warning) = midsurface_area(&mut s, "Plate", json!({}));
    assert!((area - expected).abs() < expected * 1e-5, "{area} against {expected}");
    assert!(!warning.contains("filled over"), "{warning}");
}

/// A rib 2 thick standing along a tube's length: its halfway plane runs along the tube's axis,
/// a joint not made yet. The analysis says so, and so does the build's warning.
#[test]
fn a_rib_along_a_tube_is_reported_not_joined() {
    let mut s = design(&[
        ("solid.cylinder", json!({"radius": 10, "height": 40, "base": [0, 0, 0], "body_name": "Part"})),
        ("solid.box", json!({"length": 2, "width": 21, "height": 40, "corner": [-1, 9, 0], "operation": "join", "targets": ["Part"]})),
        ("solid.cylinder", json!({"radius": 8, "height": 42, "base": [0, 0, -1], "operation": "cut", "targets": ["Part"]})),
    ]);
    let analysis = s.execute("surface.midsurface_analyze", &json!({"body": "Part", "max_thickness": "5 mm"})).unwrap_or_else(|e| panic!("{e}"));
    let open = joints_of(&analysis, "not_joined");
    assert!(!open.is_empty() && open.iter().all(|j| j.get("why").and_then(Value::as_str).is_some()), "{analysis}");
    assert!(joints_of(&analysis, "circle").is_empty(), "{analysis}");
    let (_, warning) = midsurface_area(&mut s, "Part", json!({"include_review_pairs": true}));
    assert!(warning.contains("a joint with the round wall"), "{warning}");
}

#[test]
fn replies_match_the_output_schemas() {
    let check = |schema: schemars::Schema, reply: &Value| {
        let schema = schema.as_value().clone();
        assert!(conforms(&schema, &schema, reply), "{reply}");
        assert!(reply.get("param_warnings").is_none(), "{reply}");
    };
    let mut s = plate(ParamCheck::Warn);
    let (before, undo) = (s.doc.clone(), s.undo.len());
    let analysis = s.execute("surface.midsurface_analyze", &json!({"body": "Plate", "max_thickness": 5})).unwrap_or_else(|e| panic!("{e}"));
    // Read-only: the analysis adds no feature and no undo step.
    assert_eq!(s.doc, before, "{analysis}");
    assert_eq!(s.undo.len(), undo, "{analysis}");
    // So a caller with read authority alone may run it; building stays an edit.
    let effect = |id: &str| crate::cmd::find_command(id).map(|c| c.effect);
    assert_eq!(effect("surface.midsurface_analyze"), Some(craft_plugin::Effect::Read));
    assert_eq!(effect("surface.midsurface"), Some(craft_plugin::Effect::Edit));
    check(output_schema_for::<MidsurfaceAnalysis>(), &analysis);
    let built = s
        .execute(
            "surface.midsurface",
            &json!({"body": "Plate", "max_thickness": "5 mm", "carry_holes": true, "stitch": false, "keep_original": true}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    check(output_schema_for::<FeatureResult>(), &built);
    let points = analysis.get("pairs").and_then(|p| p.get(0)).and_then(|p| p.get("points")).cloned().unwrap_or(Value::Null);
    let again = s
        .execute("surface.midsurface", &json!({"body": "Plate", "pairs": [points], "exclude": [], "min_aspect": 3}))
        .unwrap_or_else(|e| panic!("{e}"));
    check(output_schema_for::<FeatureResult>(), &again);
}
