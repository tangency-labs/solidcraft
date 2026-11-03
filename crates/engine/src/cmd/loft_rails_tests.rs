//! Guide rails through the loft commands (#201): solid.loft and surface.loft with a rail,
//! scaling or stretching, checked against analytic values; editing, naming, saving and hostile
//! values.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

/// The arc rail's radius (chord 40, bulging 5) and where its centre sits off the Z axis: the
/// rail is at x = r(z) = C + √(R² − (z − 20)²), from 10 at both ends to 15 halfway. (The sketch
/// hands the arc on as chords within 0.01 mm, hence the 2e-3 on the measures.)
const R: f64 = 42.5;
const C: f64 = 15.0 - R;

/// ∫ √(R² − u²) du over the rail's height (u from −20 to 20).
fn root_integral() -> f64 {
    20.0 * (R * R - 400.0).sqrt() + R * R * (20.0 / R).asin()
}

/// Circles `Base` (z = 0) and `Top` (z = 40) of radius 10, lines `Low` and `High` across them
/// from x = −10 to 10, rings `RingLow` and `RingHigh` (radius 10 round a hole of 5) there, and
/// in `Rail` on XZ the arc `arc` from (10, 0, 0) to (10, 0, 40) bulging out to x = 15 halfway,
/// and a straight `short` one that stops at z = 30.
fn setup() -> Session {
    let mut s = Session::default();
    for (name, z) in [("Base", 0), ("Top", 40)] {
        run(&mut s, "sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10, "id": "c"}));
        run(&mut s, "sketch.finish", json!({}));
    }
    for (name, z) in [("Low", 0), ("High", 40)] {
        run(&mut s, "sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.line", json!({"points": [[-10, 0], [10, 0]], "ids": ["l"]}));
        run(&mut s, "sketch.finish", json!({}));
    }
    for (name, z) in [("RingLow", 0), ("RingHigh", 40)] {
        run(&mut s, "sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
    }
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Rail"}));
    // The XZ sketch's y runs down world Z.
    run(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [10, -40], "through": [15, -20], "id": "arc"}));
    run(&mut s, "sketch.line", json!({"points": [[-10, 0], [-10, -30]], "ids": ["short"]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn measure(s: &Session, body: &str) -> solidcraft_kernel::BodyMeasure {
    solidcraft_kernel::measure(&s.world_state().unwrap().body(body).unwrap().body).unwrap()
}

fn loft(s: &mut Session, body: &str, extra: Value) -> Result<Value, String> {
    let mut p = json!({"sections": [{"sketch": "Base"}, {"sketch": "Top"}], "operation": "new", "body_name": body});
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    s.execute("solid.loft", &p).map_err(|e| e.to_string())
}

fn face_names(s: &mut Session, body: &str) -> Vec<String> {
    let f = run(s, "model.faces", json!({ "body": body }));
    let mut n: Vec<String> = f["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x["name"].as_str())
        .map(|x| x.split_once(':').map_or(x, |(_, rest)| rest).to_string())
        .collect();
    n.sort();
    n
}

/// The arc rail turns the loft between the circles into the arc's solid of revolution
/// (π∫r² dz); its faces keep the plain loft's names; the rail is saved with the feature; and
/// switching the fit to stretch is an edit (the circles then widen only toward the rail, into
/// ellipses π · 10 · r).
#[test]
fn a_rail_turns_a_loft_into_its_solid_of_revolution_and_stretching_is_an_edit() {
    let mut s = setup();
    let rail = json!([{"sketch": "Rail", "curves": ["arc"]}]);
    loft(&mut s, "Plain", json!({})).unwrap();
    loft(&mut s, "Vase", json!({"rails": rail, "name": "Railed"})).unwrap();
    let volume = PI * (40.0 * C * C + 2.0 * C * root_integral() + 40.0 * R * R - 2.0 * 8000.0 / 3.0);
    let m = measure(&s, "Vase");
    assert!((m.volume - volume).abs() < 2e-3 * volume, "volume {} vs {volume}", m.volume);
    assert!((m.bbox.max.x - 15.0).abs() < 1e-2, "{:?}", m.bbox);
    assert_eq!(face_names(&mut s, "Plain"), face_names(&mut s, "Vase"));

    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let reopened = Session::new(doc);
    assert!((measure(&reopened, "Vase").volume - m.volume).abs() < 1e-9 * volume);

    run(&mut s, "timeline.edit", json!({"feature": "Railed", "set": {"rail_fit": "stretch"}}));
    let stretched = measure(&s, "Vase");
    let elliptic = 10.0 * PI * (40.0 * C + root_integral());
    assert!((stretched.volume - elliptic).abs() < 2e-3 * elliptic, "stretched {} vs {elliptic}", stretched.volume);
    assert!((stretched.bbox.max.y - 10.0).abs() < 1e-3, "{:?}", stretched.bbox);
}

/// A sheet lofted between the two lines with the arc rail from one end: the lines between grow
/// about their middles, 2r long; area 2∫r dz.
#[test]
fn a_surface_loft_follows_its_rail() {
    let mut s = setup();
    run(
        &mut s,
        "surface.loft",
        json!({"sections": [{"sketch": "Low", "curves": ["l"]}, {"sketch": "High", "curves": ["l"]}], "rails": [{"sketch": "Rail", "curves": ["arc"]}], "body_name": "Sail"}),
    );
    let m = measure(&s, "Sail");
    let area = 2.0 * (40.0 * C + root_integral());
    assert!((m.area - area).abs() < 2e-3 * area, "area {} vs {area}", m.area);
    assert!((m.bbox.min.x + 15.0).abs() < 1e-2, "{:?}", m.bbox);
}

/// Hostile rails are refused with an error and leave the design as it was.
#[test]
fn hostile_rails_are_refused() {
    let mut s = setup();
    let before = s.doc.features.len();
    let rail = json!([{"sketch": "Rail", "curves": ["arc"]}]);
    for bad in [
        json!({"rails": "arc"}),
        json!({"rails": []}),
        json!({"rails": [{"sketch": "Rail", "curves": ["arc"]}, {"sketch": "Rail", "curves": ["arc"]}, {"sketch": "Rail", "curves": ["arc"]}]}),
        json!({"rails": [{"sketch": "Rail", "curves": []}]}),
        // A loft has no path whose sketch a rail could default to.
        json!({"rails": [{"curves": ["arc"]}]}),
        json!({"rails": [{"sketch": "Rail", "curves": ["nope"]}]}),
        json!({"rails": [{"sketch": "Nowhere", "curves": ["arc"]}]}),
        json!({"rails": rail, "rail_fit": "wobble"}),
        // A rail that stops short of the top section.
        json!({"rails": [{"sketch": "Rail", "curves": ["short"]}]}),
        // Two rails meeting the sections at one point.
        json!({"rails": [{"sketch": "Rail", "curves": ["arc"]}, {"sketch": "Rail", "curves": ["arc"]}]}),
    ] {
        assert!(loft(&mut s, "Bad", bad.clone()).is_err(), "{bad}");
        let mut p = json!({"sections": [{"sketch": "Low", "curves": ["l"]}, {"sketch": "High", "curves": ["l"]}]});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(s.execute("surface.loft", &p).is_err(), "surface {bad}");
    }
    // Rails on what the railed loft does not build yet.
    let apex = json!({"sections": [{"sketch": "Base"}, {"sketch": "Rail", "point": "arc.start"}], "rails": rail});
    assert!(s.execute("solid.loft", &apex).is_err());
    // Rings (a circle with a hole) loft without rails, not with them yet.
    let ring = |name: &str| json!({"sketch": name, "profiles": [{"point": [7.5, 0]}]});
    let rings = json!({"sections": [ring("RingLow"), ring("RingHigh")], "operation": "new", "rails": rail});
    let refused = s.execute("solid.loft", &rings).unwrap_err().to_string();
    assert!(refused.contains("not supported yet") && refused.contains("holes"), "{refused}");
    let g1 =
        json!({"sections": [{"sketch": "Low", "curves": ["l"]}, {"sketch": "High", "curves": ["l"]}], "rails": rail, "continuity": ["g1", "g0"]});
    let refused = s.execute("surface.loft", &g1).unwrap_err().to_string();
    assert!(refused.contains("not supported yet"), "{refused}");
    assert_eq!(s.doc.features.len(), before, "nothing was added");
}
