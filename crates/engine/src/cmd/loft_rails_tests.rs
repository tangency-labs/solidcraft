//! Guide rails through the loft commands (#201): solid.loft and surface.loft with a rail,
//! scaling or stretching, checked against analytic values; editing, naming, saving and hostile
//! values.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_geom::Vec3Geom;

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

/// A hull sheet from a keel (vertical) to a sheer (level) along a transom and a stem (#12031): the
/// keel's sketch facing −Y or +Y draws the same curve, and the loft is the same either way. The
/// stations between the two sections, at 90° to each other, used to turn with the keel's sketch.
#[test]
fn a_sections_sketch_facing_does_not_change_a_railed_loft() {
    let hull = |keel_plane: Value, dip: f64| {
        let mut s = Session::default();
        let sketch = |s: &mut Session, name: &str, plane: Value, cmd: &str, p: Value| {
            run(s, "sketch.create", json!({"name": name, "plane": plane}));
            run(s, cmd, p);
            run(s, "sketch.finish", json!({}));
        };
        let at = |o: [i32; 3], x: [i32; 3], y: [i32; 3]| json!({"origin": o, "x_dir": x, "y_dir": y});
        sketch(&mut s, "Keel", keel_plane, "sketch.spline.fit_point", json!({"points": [[20, 0], [131, dip], [250, 0]]}));
        sketch(&mut s, "Sheer", at([0, 0, 20], [1, 0, 0], [0, 1, 0]), "sketch.spline.fit_point", json!({"points": [[250, 42], [177, 50], [20, 0]]}));
        sketch(
            &mut s,
            "Transom",
            at([250, 0, 0], [0, 1, 0], [0, 0, 1]),
            "sketch.spline.fit_point",
            json!({"points": [[0, 0], [40.5, 12.3], [42, 20]]}),
        );
        // The XZ sketch's y runs down world Z.
        sketch(&mut s, "Stem", json!("XZ"), "sketch.line", json!({"points": [[20, 0], [20, -20]]}));
        let p = json!({"sections": [{"sketch": "Keel", "curves": ["s1"]}, {"sketch": "Sheer", "curves": ["s1"]}],
            "rails": [{"sketch": "Transom", "curves": ["s1"]}, {"sketch": "Stem", "curves": ["l1"]}], "body_name": "Hull"});
        s.execute("surface.loft", &p).map_err(|e| e.to_string())?;
        Ok::<_, String>(measure(&s, "Hull"))
    };
    let up = hull(json!("XZ"), 19.0).unwrap();
    let down = hull(json!({"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}), -19.0).unwrap();
    assert!((up.area - down.area).abs() < 1e-6 * up.area, "area {} vs {}", down.area, up.area);
    assert!((up.bbox.min.z + 19.0).abs() < 0.1 && (up.bbox.max.z - 20.0).abs() < 1e-6, "{:?}", up.bbox);
    assert!(up.bbox.min.dist(down.bbox.min) < 1e-6 && up.bbox.max.dist(down.bbox.max) < 1e-6, "{:?} vs {:?}", down.bbox, up.bbox);
}

/// A quarter turn about the Z axis (#17580): a quadrilateral beside the axis and the same one
/// turned a quarter turn round it, its sketch facing back against the turn. The second section
/// used to be wound the other way round from the first, so the stations twisted through
/// themselves (two rails "came together", one left the loft without volume); and a rail of
/// curves that close into a ring was followed the long way round from one section to the next.
/// Two rails, round the axis through the quadrilateral's two nearest corners, the outer one three
/// arcs listed out of order, now stretch the stations into the solid of revolution: π/2 · the
/// quadrilateral's first moment about the axis (Pappus), to the skin's accuracy through its
/// stations. The closed outline of the face its bottom edge sweeps (the arcs and the two edges)
/// is followed from where it leaves the first section, along the inner arc.
#[test]
fn a_loft_through_a_quarter_turn_follows_rails_of_several_curves() {
    let mut s = Session::default();
    // The same quadrilateral either side, its x the distance from the Z axis.
    let start = [[2, 0], [5, 0], [25, 8], [25, 10]];
    let end = [[-2, 0], [-5, 0], [-25, 8], [-25, 10]];
    for (name, x_dir, points) in [("Start", [-1, 0, 0], start), ("End", [0, -1, 0], end)] {
        run(&mut s, "sketch.create", json!({"name": name, "plane": {"origin": [0, 0, 0], "x_dir": x_dir, "y_dir": [0, 0, 1]}}));
        run(&mut s, "sketch.line", json!({"points": points, "infer": false, "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
    }
    run(&mut s, "sketch.create", json!({"name": "Rail", "plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    // Arcs round the origin: radius 5 from 90° to 180° in three pieces of 30° (cos 30° · 5 =
    // 4.330127018922193), and radius 2 whole.
    let h = 4.330127018922193;
    for (id, start, end) in
        [("o1", [0.0, 5.0], [-2.5, h]), ("o2", [-2.5, h], [-h, 2.5]), ("o3", [-h, 2.5], [-5.0, 0.0]), ("i", [0.0, 2.0], [-2.0, 0.0])]
    {
        run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": start, "end": end, "id": id}));
    }
    run(&mut s, "sketch.line", json!({"points": [[-5, 0], [-2, 0]], "infer": false, "ids": ["s"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 2], [0, 5]], "infer": false, "ids": ["e"]}));
    run(&mut s, "sketch.finish", json!({}));
    let rail = |curves: &[&str]| json!({"sketch": "Rail", "curves": curves});
    let mut loft = |body: &str, rails: Value, fit: &str| {
        let p = json!({"sections": [{"sketch": "Start"}, {"sketch": "End"}], "rails": rails, "rail_fit": fit, "operation": "new", "body_name": body});
        s.execute("solid.loft", &p).unwrap_or_else(|e| panic!("{body}: {e}"));
    };
    loft("Turned", json!([rail(&["o2", "o1", "o3"]), rail(&["i"])]), "stretch");
    loft("Inner", json!([rail(&["i"])]), "scale");
    loft("Outline", json!([rail(&["o1", "o2", "o3", "s", "i", "e"])]), "scale");
    // Pappus: the quadrilateral's area is 35 and its centroid 316/21 from the axis, so its
    // first moment is 1580/3 and a quarter turn sweeps π/2 · 1580/3 = 790π/3.
    let revolved = 790.0 * PI / 3.0;
    let turned = measure(&s, "Turned");
    assert!((turned.volume - revolved).abs() < 2e-3 * revolved, "volume {} vs {revolved}", turned.volume);
    assert!((turned.bbox.max.y - 25.0).abs() < 1e-6 && (turned.bbox.min.x + 25.0).abs() < 1e-6, "{:?}", turned.bbox);
    let (inner, outline) = (measure(&s, "Inner").volume, measure(&s, "Outline").volume);
    assert!(inner > 0.0 && (outline - inner).abs() < 1e-9 * inner, "the outline's loft {outline} vs the inner arc's {inner}");
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
    // A loft to a point whose rail stops short of the apex (the arc's top end, 40 up).
    let apex =
        json!({"sections": [{"sketch": "Base"}, {"sketch": "Rail", "point": "arc.start"}], "rails": [{"sketch": "Rail", "curves": ["short"]}]});
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

/// To [`setup`]'s sketches, `Tip`: the point `apex` 40 above the circles' centre; and `Spine` on
/// XZ: the line `side` from the circle at x = 10 straight to the apex, and the arc `bow` from the
/// circle at x = −10 to the apex bulging out through (−9, 0, 20).
fn apex_sketches(s: &mut Session) {
    run(s, "sketch.create", json!({"name": "Tip", "plane": {"origin": [0, 0, 40], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    run(s, "sketch.point", json!({"point": [0, 0], "id": "apex"}));
    run(s, "sketch.finish", json!({}));
    run(s, "sketch.create", json!({"plane": "XZ", "name": "Spine"}));
    // The XZ sketch's y runs down world Z.
    run(s, "sketch.line", json!({"points": [[10, 0], [0, -40]], "ids": ["side"]}));
    run(s, "sketch.arc.three_point", json!({"start": [-10, 0], "end": [0, -40], "through": [-9, -20], "id": "bow"}));
    run(s, "sketch.finish", json!({}));
}

/// A circle lofted to a sketch point along a rail (#20985): a straight rail from the circle to the
/// apex gives the cone, π · 10² · 40 / 3, named as the loft to the point without the rail, and the
/// rail is saved with it; an arc rail bulging out makes a fuller solid that still ends at the apex.
#[test]
fn a_loft_to_a_point_follows_its_rail() {
    let mut s = setup();
    apex_sketches(&mut s);
    let to_point = |rails: Value, body: &str| json!({"sections": [{"sketch": "Base"}, {"sketch": "Tip", "point": "apex"}], "operation": "new", "body_name": body, "rails": rails});
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Base"}, {"sketch": "Tip", "point": "apex"}], "operation": "new", "body_name": "Cone"}));
    run(&mut s, "solid.loft", to_point(json!([{"sketch": "Spine", "curves": ["side"]}]), "Railed"));
    let cone = PI * 100.0 * 40.0 / 3.0;
    let m = measure(&s, "Railed");
    assert!((m.volume - cone).abs() < 1e-4 * cone, "volume {} vs {cone}", m.volume);
    assert!((m.bbox.max.z - 40.0).abs() < 1e-6, "{:?}", m.bbox);
    assert_eq!(face_names(&mut s, "Cone"), face_names(&mut s, "Railed"));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((measure(&reopened, "Railed").volume - m.volume).abs() < 1e-9 * cone);

    run(&mut s, "solid.loft", to_point(json!([{"sketch": "Spine", "curves": ["bow"]}]), "Bowed"));
    let bowed = measure(&s, "Bowed");
    assert!(bowed.volume > cone && bowed.volume < PI * 100.0 * 40.0, "volume {} between {cone} and the cylinder", bowed.volume);
    assert!((bowed.bbox.max.z - 40.0).abs() < 1e-6 && bowed.bbox.min.x < -10.0 + 1e-3, "{:?}", bowed.bbox);
}

/// The railed loft to a point (#20985) given apex first is the same cone; as a cut it takes the
/// cone out of a box round it (30 × 30 × 45 less π · 10² · 40 / 3); joined to a block under its
/// base it adds the cone to it (30 × 30 × 5 plus the cone).
#[test]
fn a_railed_loft_to_a_point_cuts_joins_and_takes_its_sections_either_way_round() {
    let cone = PI * 100.0 * 40.0 / 3.0;
    let rails = json!([{"sketch": "Spine", "curves": ["side"]}]);
    let apex_first = json!([{"sketch": "Tip", "point": "apex"}, {"sketch": "Base"}]);
    let mut s = setup();
    apex_sketches(&mut s);
    run(&mut s, "solid.loft", json!({"sections": apex_first, "operation": "new", "body_name": "Flipped", "rails": rails}));
    let v = measure(&s, "Flipped").volume;
    assert!((v - cone).abs() < 1e-4 * cone, "apex first: volume {v} vs {cone}");

    for (op, corner, height, want) in [("cut", 0.0, 45.0, 30.0 * 30.0 * 45.0 - cone), ("join", -5.0, 5.0, 30.0 * 30.0 * 5.0 + cone)] {
        let mut s = setup();
        apex_sketches(&mut s);
        run(&mut s, "solid.box", json!({"corner": [-15, -15, corner], "length": 30, "width": 30, "height": height, "body_name": "Stock"}));
        run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Base"}, {"sketch": "Tip", "point": "apex"}], "operation": op, "rails": rails}));
        let v = measure(&s, "Stock").volume;
        assert!((v - want).abs() < 1e-4 * want, "{op}: volume {v} vs {want}");
    }
}
