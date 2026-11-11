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

/// Six 3D-sketch spline rails around the circles of `Base` and `Top`, each bulging out to
/// radius 15 halfway up: solid.loft follows all six (the rails' solid of revolution, volume
/// 4500π + 8000 to the splines' fit of the sine), and seventeen are refused.
#[test]
fn six_spline3d_rails_guide_a_solid_loft_and_seventeen_are_refused() {
    let mut s = setup();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rails3d"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let rail_at = |s: &mut Session, a: f64| -> String {
        let pts: Vec<[f64; 3]> = (0..=16)
            .map(|i| {
                let z = f64::from(i) * 2.5;
                let r = 10.0 + 5.0 * (PI * z / 40.0).sin();
                [r * a.cos(), r * a.sin(), z]
            })
            .collect();
        run(s, "sketch.spline3d", json!({ "points": pts }))["curves"][0].as_str().unwrap().to_string()
    };
    let ids: Vec<String> = (0..17).map(|k| rail_at(&mut s, f64::from(k) * 2.0 * PI / 17.0)).collect();
    let six: Vec<String> = (0..6).map(|k| rail_at(&mut s, f64::from(k) * PI / 3.0)).collect();
    run(&mut s, "sketch.finish", json!({}));
    let rails = |ids: &[String]| json!({"rails": ids.iter().map(|c| json!({"sketch": "Rails3d", "curves": [c]})).collect::<Vec<_>>()});
    loft(&mut s, "Six", rails(&six)).unwrap();
    let m = measure(&s, "Six");
    let want = 4500.0 * PI + 8000.0;
    assert!((m.volume - want).abs() < 2e-3 * want, "volume {} vs {want}", m.volume);
    assert!((m.bbox.max.x - 15.0).abs() < 0.05 && (m.bbox.max.z - 40.0).abs() < 1e-6, "{:?}", m.bbox);
    let e = loft(&mut s, "Many", rails(&ids)).unwrap_err();
    assert!(e.contains("1…16"), "{e}");
}

/// A 10 mm square (x 500…510, y 0…10, z = 0) down to a 6 mm one 20 mm below, joined corner to
/// corner by four straight rails: the frustum (h/3 · (A₁ + A₂ + √(A₁A₂))). Built with one rail
/// and edited to all four through timeline.edit; seventeen are refused there too.
#[test]
fn four_corner_rails_make_a_frustum_and_are_an_edit() {
    let mut s = Session::default();
    for (name, z, lo, hi) in [("Big", 0, [500, 0], [510, 10]), ("Small", -20, [502, 2], [508, 8])] {
        run(&mut s, "sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": lo, "p1": hi}));
        run(&mut s, "sketch.finish", json!({}));
    }
    let made = run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Corners"}));
    let corners_id = made["sketch"].as_u64().unwrap_or_else(|| panic!("{made}"));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let corners = [([500.0, 0.0], [502.0, 2.0]), ([510.0, 0.0], [508.0, 2.0]), ([510.0, 10.0], [508.0, 8.0]), ([500.0, 10.0], [502.0, 8.0])];
    let ids: Vec<String> = corners
        .iter()
        .map(|([x0, y0], [x1, y1])| {
            let pts: Vec<[f64; 3]> = (0..=2).map(|i| f64::from(i) / 2.0).map(|f| [x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, -20.0 * f]).collect();
            run(&mut s, "sketch.spline3d", json!({ "points": pts }))["curves"][0].as_str().unwrap().to_string()
        })
        .collect();
    run(&mut s, "sketch.finish", json!({}));
    let rails = |ids: &[String]| json!(ids.iter().map(|c| json!({"sketch": "Corners", "curves": [c]})).collect::<Vec<_>>());
    let p = json!({"sections": [{"sketch": "Big"}, {"sketch": "Small"}], "operation": "new", "body_name": "Frustum", "name": "Railed", "rails": rails(&ids[..1])});
    run(&mut s, "solid.loft", p);
    // timeline.edit takes the rails as they are stored: each sketch by its id.
    let stored = |ids: &[String]| json!(ids.iter().map(|c| json!({"sketch": corners_id, "curves": [c]})).collect::<Vec<_>>());
    run(&mut s, "timeline.edit", json!({"feature": "Railed", "set": {"rails": stored(&ids)}}));
    let want = 20.0 / 3.0 * (100.0 + 36.0 + 60.0);
    let m = measure(&s, "Frustum");
    assert!((m.volume - want).abs() < 1e-3 * want, "volume {} vs {want}", m.volume);
    assert!((m.bbox.min.x - 500.0).abs() < 1e-6 && (m.bbox.max.x - 510.0).abs() < 1e-6 && (m.bbox.min.z + 20.0).abs() < 1e-6, "{:?}", m.bbox);
    let seventeen: Vec<String> = ids.iter().cycle().take(17).cloned().collect();
    assert!(s.execute("timeline.edit", &json!({"feature": "Railed", "set": {"rails": stored(&seventeen)}})).is_err());
}
