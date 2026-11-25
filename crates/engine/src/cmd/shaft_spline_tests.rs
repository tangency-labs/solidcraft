//! Tests of `shaft_spline.insert`, which adds one Shaft Spline feature to the History.
//!
//! The bodies are measured against the kernel's own tooth outlines, worked out here through the same plan the
//! feature uses: a new part's volume is its outline's area times the length, with one face for each piece of the
//! outline; a picked shaft keeps all of its blank but the ring outside the outline over the splined length; a
//! bore loses the opening less its own circle over that length. Two ways of making one solid (a blank chamfered
//! first, the spline's own chamfer) are compared by volume. Faces are checked by their lasting names, edits go
//! through `timeline.redefine` and undo, refusals are checked word for word with the design left as it was, and
//! hostile parameters never panic. Most tests use six straight-sided teeth, which build quickly; involute teeth
//! appear where a test is about them.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::shaft_spline::{
    OutlinePiece as Piece, SplineForm, SplineMaster, SplineNumbers, SplinePlan, ToothOutline as Outline, bezier_pieces, plan,
};
use solidcraft_kernel::PointClass;

use super::{SplinePart, shaft_spline_preview};
use crate::{EngineError, Session};

const CMD: &str = "shaft_spline.insert";

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn with(mut p: Value, changes: Value) -> Value {
    for (k, v) in changes.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    p
}

fn without(mut p: Value, key: &str) -> Value {
    if let Some(o) = p.as_object_mut() {
        o.remove(key);
    }
    p
}

/// The words of the command's refusal of `p`, after checking that the refusal left the design as it was.
fn refused(s: &mut Session, p: Value) -> String {
    let (doc, undo, features) = (s.doc.clone(), s.undo.len(), s.doc.features.len());
    let words = match s.execute(CMD, &p) {
        Err(EngineError::BadParams { msg, .. } | EngineError::Other(msg)) => msg,
        other => panic!("{p}: expected a refusal, got {other:?}"),
    };
    assert!(Arc::ptr_eq(&s.doc, &doc), "{p}: the refusal changed the design");
    assert_eq!((s.undo.len(), s.doc.features.len()), (undo, features), "{p}");
    assert!(!words.trim().is_empty(), "{p}: a refusal without words");
    words
}

/// [`refused`] on an empty design.
fn refusal(p: Value) -> String {
    refused(&mut Session::default(), p)
}

/// The id of the feature a feature command's reply names.
fn feature_of(r: &Value) -> u64 {
    r["feature"].as_u64().unwrap_or_else(|| panic!("no feature in {r}"))
}

/// The design's last feature, which must be a Shaft Spline.
fn last_spline(s: &Session) -> &FeatureKind {
    let f = s.doc.features.last().unwrap_or_else(|| panic!("the design has no feature"));
    assert!(matches!(f.kind, FeatureKind::ShaftSpline { .. }), "the last feature is not a Shaft Spline: {:?}", f.kind);
    &f.kind
}

/// The errors and warnings the rebuild left on the features, each with its feature.
fn notes(s: &Session) -> Vec<String> {
    s.model.results.iter().filter_map(|r| r.error.clone().or_else(|| r.warning.clone()).map(|w| format!("{}: {w}", r.name))).collect()
}

/// The body called `name` as `inspect.measure` gives it.
fn measure(s: &mut Session, name: &str) -> Value {
    let m = run(s, "inspect.measure", json!({}));
    m["bodies"].as_array().and_then(|all| all.iter().find(|b| b["name"] == name)).cloned().unwrap_or_else(|| panic!("no body {name}: {m}"))
}

/// The volume of the body called `name`, which must be closed.
fn volume(s: &mut Session, name: &str) -> f64 {
    let b = measure(s, name);
    assert_eq!(b["closed"], json!(true), "{name} is not closed: {b}");
    b["volume_mm3"].as_f64().unwrap_or_else(|| panic!("{name} has no volume: {b}"))
}

/// The names of the bodies of the design.
fn body_names(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

fn assert_close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() <= 1e-6 * want.abs().max(1.0), "{what}: {got}, expected {want}");
}

/// One face of a body as `model.faces` lists it.
#[derive(Debug)]
struct Face {
    name: String,
    centroid: [f64; 3],
    /// The radius of a round face.
    radius: Option<f64>,
}

fn faces(s: &mut Session, body: &str) -> Vec<Face> {
    let r = run(s, "model.faces", json!({"body": body}));
    r["faces"]
        .as_array()
        .map(|all| {
            all.iter()
                .map(|f| Face {
                    name: f["name"].as_str().unwrap_or_default().to_string(),
                    centroid: [0usize, 1, 2].map(|i| f["centroid"][i].as_f64().unwrap_or(f64::NAN)),
                    radius: f["cylinder"]["radius"].as_f64(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A face's own name, before any `#` that tells apart the pieces of one face.
fn stem(name: &str) -> &str {
    name.split('#').next().unwrap_or(name)
}

fn has(faces: &[Face], name: &str) -> bool {
    faces.iter().any(|f| stem(&f.name) == name)
}

/// Every face has a name of its own: none is missing, none is a running number, none is used twice.
fn assert_lasting_names(faces: &[Face]) {
    let names: Vec<&str> = faces.iter().map(|f| f.name.as_str()).collect();
    assert!(names.iter().all(|n| !n.is_empty() && !n.contains(":face:")), "{names:?}");
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "a name is used twice: {names:?}");
}

/// The count of the spline's chamfer faces at the end the teeth start from, or at the far end (`far`).
fn chamfers(faces: &[Face], far: bool) -> usize {
    faces
        .iter()
        .filter(|f| {
            let s = stem(&f.name);
            s.contains("chamfer") && s.ends_with(":far") == far
        })
        .count()
}

/// The angle of a face's centroid about Z, in degrees counter-clockwise from +X.
fn angle_of(f: &Face) -> f64 {
    f.centroid[1].atan2(f.centroid[0]).to_degrees()
}

/// Whether each point at `radius` and at each of `angles` (degrees from +X), at height `z`, is in the body's
/// material: classified exactly on the body's faces, no mesh. A point on a face fails the test.
fn material_at(s: &Session, name: &str, radius: f64, z: f64, angles: &[f64]) -> Vec<bool> {
    let st = s.model.state();
    let body = st.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}"));
    let points: Vec<Vec3> = angles.iter().map(|a| Vec3::new(radius * a.to_radians().cos(), radius * a.to_radians().sin(), z)).collect();
    let classes = body.body.classify_points(&points).unwrap();
    classes
        .iter()
        .zip(angles)
        .map(|(c, a)| match c {
            PointClass::Inside => true,
            PointClass::Outside => false,
            PointClass::OnBoundary => panic!("{name}: the point at {a} degrees lies on a face"),
        })
        .collect()
}

/// The area an outline runs around, worked out piece by piece: exactly for lines and arcs, and for a spline from its
/// cubic Bezier pieces by Gauss-Legendre quadrature, which is exact for the polynomial `x y' - y x'` of degree 5.
fn area(outline: &Outline) -> f64 {
    let mut doubled = 0.0;
    for piece in &outline.tooth {
        doubled += match piece {
            Piece::Line { from, to, .. } => from[0] * to[1] - to[0] * from[1],
            Piece::Arc { centre, radius, start, sweep, .. } => {
                let (a0, a1) = (*start, start + sweep);
                radius * (centre[0] * (a1.sin() - a0.sin()) - centre[1] * (a1.cos() - a0.cos())) + radius * radius * sweep
            }
            Piece::Spline { degree, knots, control, .. } => {
                let nodes = [(-0.861_136_311_594_052_6, 0.347_854_845_137_453_9), (-0.339_981_043_584_856_3, 0.652_145_154_862_546_1)];
                let nodes = [nodes[0], nodes[1], (-nodes[1].0, nodes[1].1), (-nodes[0].0, nodes[0].1)];
                let mut sum = 0.0;
                for b in bezier_pieces(*degree, knots, control).unwrap() {
                    let n = b.len() - 1;
                    let at = |t: f64| -> ([f64; 2], [f64; 2]) {
                        // De Casteljau for the point; the hodograph for the derivative.
                        let mut p = b.clone();
                        for r in 1..=n {
                            for i in 0..=n - r {
                                p[i] = [p[i][0] * (1.0 - t) + p[i + 1][0] * t, p[i][1] * (1.0 - t) + p[i + 1][1] * t];
                            }
                        }
                        let mut d: Vec<[f64; 2]> = b.windows(2).map(|w| [(w[1][0] - w[0][0]) * n as f64, (w[1][1] - w[0][1]) * n as f64]).collect();
                        for r in 1..n {
                            for i in 0..n - r {
                                d[i] = [d[i][0] * (1.0 - t) + d[i + 1][0] * t, d[i][1] * (1.0 - t) + d[i + 1][1] * t];
                            }
                        }
                        (p[0], d[0])
                    };
                    for (x, w) in nodes {
                        let (p, d) = at(0.5 + 0.5 * x);
                        sum += 0.5 * w * (p[0] * d[1] - p[1] * d[0]);
                    }
                }
                sum
            }
            Piece::Curve { points, .. } => points.windows(2).map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1]).sum(),
        };
    }
    doubled / 2.0 * f64::from(outline.teeth)
}

/// The straight-sided spline most tests use: 6 teeth, 28 over 23 mm, 6 mm wide, plain tips, 20 mm long.
fn straight() -> Value {
    json!({
        "type": "straight", "teeth": 6, "length": 20, "major_diameter": 28, "minor_diameter": 23, "tooth_width": 6,
        "side_clearance": 0.05, "major_clearance": 0.3, "minor_clearance": 0.2
    })
}

/// [`straight`]'s numbers as the plan reads them, with `teeth` teeth `width` wide over `major`.
fn numbers(teeth: u32, major: f64, width: f64) -> SplineNumbers {
    SplineNumbers {
        form: SplineForm::Straight,
        teeth,
        length: 20.0,
        major_diameter: Some(major),
        minor_diameter: Some(23.0),
        tooth_width: Some(width),
        side_clearance: Some(0.05),
        major_clearance: Some(0.3),
        minor_clearance: Some(0.2),
        with_shaft: true,
        ..SplineNumbers::default()
    }
}

/// The involute spline of the involute tests: 20 teeth of module 1.5, the plan's own values for the rest.
fn involute_numbers(teeth: u32, module: f64) -> SplineNumbers {
    SplineNumbers {
        form: SplineForm::Involute,
        teeth,
        length: 10.0,
        module: Some(module),
        side_clearance: Some(0.06),
        tip_clearance: Some(0.2),
        with_shaft: true,
        ..SplineNumbers::default()
    }
}

fn planned(n: &SplineNumbers) -> SplinePlan {
    plan(n).unwrap_or_else(|r| panic!("{r}"))
}

/// The area of the shaft's outline of `n`.
fn shaft_area(n: &SplineNumbers) -> f64 {
    area(&planned(n).shaft)
}

/// What a round blank of `radius` and `height` keeps when a spline whose shaft outline encloses `outline_area` is
/// cut into it for `length`: all of it but the ring outside the outline over the splined length.
fn cut_shaft_volume(radius: f64, height: f64, outline_area: f64, length: f64) -> f64 {
    PI * radius * radius * height - (PI * radius * radius - outline_area) * length
}

/// A round shaft about Z, from z = 0 up, called Shaft. Returns its feature.
fn shaft(s: &mut Session, radius: f64, height: f64) -> u64 {
    feature_of(&run(s, "solid.cylinder", json!({"radius": radius, "height": height, "body_name": "Shaft"})))
}

/// A ring about Z with a round bore, from `start` up for `thick`, called Hub.
fn ring(s: &mut Session, outer: f64, bore: f64, start: f64, thick: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": start, "name": "Hub section"}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": outer}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": bore}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": thick, "operation": "new", "body_name": "Hub"}));
    assert_close("the ring before the spline", volume(s, "Hub"), PI * (outer * outer - bore * bore) * thick);
}

fn redefine(s: &mut Session, feature: u64, p: Value) -> Value {
    run(s, "timeline.redefine", json!({"feature": feature, "command": CMD, "params": p}))
}

// ---------------------------------------------------------------------------------------------------------------
// New parts
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_new_straight_shaft_is_one_feature_whose_body_is_its_outline_extruded() {
    let mut s = Session::default();
    let r = run(&mut s, CMD, straight());
    assert_eq!(s.doc.features.len(), 1, "one History item");
    last_spline(&s);
    assert_eq!(r["created_bodies"], json!(["Spline Shaft"]), "{r}");
    let n = numbers(6, 28.0, 6.0);
    let planned = planned(&n);
    let outline = &planned.shaft;
    assert_close("the shaft's volume, its outline's area times the length", volume(&mut s, "Spline Shaft"), area(outline) * 20.0);
    // One face for each piece of the outline, and the two ends, each with a lasting name.
    let fs = faces(&mut s, "Spline Shaft");
    assert_eq!(fs.len(), outline.tooth.len() * 6 + 2, "{fs:?}");
    assert_lasting_names(&fs);
    let f = feature_of(&r);
    for end in ["top", "bottom"] {
        assert!(has(&fs, &format!("F{f}:{end}")), "{end}: {fs:?}");
    }
    for k in 0..6 {
        for part in [format!("tip{k}"), format!("space{k}:flank:0"), format!("space{k}:flank:1"), format!("space{k}:root")] {
            assert!(has(&fs, &format!("F{f}:{part}")), "{part}: {fs:?}");
        }
    }
    // The engine's numbers.
    for (key, want) in [
        ("shaft_major_diameter", 28.0),
        ("shaft_minor_diameter", 23.0),
        ("hub_major_diameter", 28.0 + 2.0 * 0.3),
        ("hub_minor_diameter", 23.0 + 2.0 * 0.2),
        ("tip_gap", 0.3),
        ("root_gap", 0.2),
        ("turning_play", planned.made.turning_play),
    ] {
        assert!((r[key].as_f64().unwrap_or(f64::NAN) - want).abs() < 1e-12, "{key}: {r}");
    }
    assert_eq!(r["positions"], 6, "{r}");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    // One undo step takes the feature and its body away.
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features.is_empty() && body_names(&s).is_empty());
}

#[test]
fn a_new_hub_is_a_ring_around_the_opening_and_fits_with_the_clearance_asked_for() {
    let mut s = Session::default();
    let r = run(&mut s, CMD, with(straight(), json!({"make": "both", "hub_diameter": 40})));
    assert_eq!(r["created_bodies"], json!(["Spline Shaft", "Spline Hub"]), "{r}");
    assert_eq!(s.doc.features.len(), 1, "one History item for both parts");
    let opening = area(&planned(&numbers(6, 28.0, 6.0)).hub);
    assert_close("the hub's volume, the ring's area times the length", volume(&mut s, "Spline Hub"), (PI * 20.0 * 20.0 - opening) * 20.0);
    assert_lasting_names(&faces(&mut s, "Spline Hub"));
    // Measured on the exact faces: nothing overlaps, and the nearest the two come is the side clearance, the smallest
    // of the three.
    let c = run(&mut s, "inspect.clearance", json!({"clearance": 1.0}));
    assert_eq!(c["overlaps"], json!([]), "{c}");
    let near = c["near"].as_array().cloned().unwrap_or_default();
    assert_eq!(near.len(), 1, "{c}");
    let d = near[0]["distance_mm"].as_f64().unwrap_or(f64::NAN);
    assert!((d - 0.05).abs() < 1e-6, "least gap {d}: {c}");
}

#[test]
fn more_teeth_make_more_faces_on_a_new_shaft() {
    for (teeth, width) in [(3, 6.0), (6, 6.0), (10, 3.0)] {
        let mut s = Session::default();
        let r = run(&mut s, CMD, with(straight(), json!({"teeth": teeth, "tooth_width": width})));
        let outline_pieces = planned(&numbers(teeth, 28.0, width)).shaft.tooth.len();
        let fs = faces(&mut s, "Spline Shaft");
        assert_eq!(fs.len(), outline_pieces * teeth as usize + 2, "{teeth} teeth: {fs:?}");
        let f = feature_of(&r);
        let tips = fs.iter().filter(|x| stem(&x.name).starts_with(&format!("F{f}:tip"))).count();
        assert_eq!(tips, teeth as usize, "{teeth} teeth: {fs:?}");
        assert!(!has(&fs, &format!("F{f}:space{teeth}:root")), "a space numbered past the last tooth");
    }
}

#[test]
fn an_involute_shaft_is_its_fitted_outline_extruded() {
    let mut s = Session::default();
    let p = json!({"type": "involute", "teeth": 20, "length": 5, "module": 1.5, "side_clearance": 0.06, "tip_clearance": 0.2});
    let r = run(&mut s, CMD, p);
    let n = SplineNumbers { length: 5.0, ..involute_numbers(20, 1.5) };
    let planned = planned(&n);
    let outline = &planned.shaft;
    assert_close("the shaft's volume", volume(&mut s, "Spline Shaft"), area(outline) * 5.0);
    let fs = faces(&mut s, "Spline Shaft");
    assert_eq!(fs.len(), outline.tooth.len() * 20 + 2, "one face for each piece of the outline: {fs:?}");
    assert_lasting_names(&fs);
    let f = feature_of(&r);
    for part in ["space0:flank:0", "space0:flank:1", "space0:fillet:0", "space0:fillet:1", "space0:root", "tip0", "space19:root"] {
        assert!(has(&fs, &format!("F{f}:{part}")), "{part}: {fs:?}");
    }
    // The pitch circle is module times teeth; the flanks are fitted within a two-thousandth of the module.
    assert!((r["pitch_diameter"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9, "{r}");
    assert!((r["flank_tolerance"].as_f64().unwrap_or(f64::NAN) - 1.5 * 5e-4).abs() < 1e-15, "{r}");
}

#[test]
fn a_serration_with_rounded_tips_and_many_teeth_is_made() {
    let mut s = Session::default();
    let p = json!({
        "type": "serration", "teeth": 36, "length": 10, "major_diameter": 20, "minor_diameter": 18.8, "flank_angle": 90,
        "root_round": 0.03, "tip_corner": "rounded", "tip_size": 0.1, "side_clearance": 0.02, "major_clearance": 0.05,
        "minor_clearance": 0.05, "hub_root_round": 0.03, "hub_tip_round": 0.05, "make": "both", "hub_diameter": 30,
        "master_spline": "wide_tooth", "master_angle": 90
    });
    let r = run(&mut s, CMD, p);
    assert_eq!(r["created_bodies"], json!(["Spline Shaft", "Spline Hub"]), "{r}");
    assert!(r["sharp_tip_diameter"].as_f64().is_some(), "{r}");
    let c = run(&mut s, "inspect.clearance", json!({"clearance": 1.0}));
    assert_eq!(c["overlaps"], json!([]), "{c}");
}

#[test]
fn a_wide_tooth_is_made_on_tips_rounded_all_across() {
    // Rounds of half the tooth's width meet on the major circle with no flat between them; the corner where they
    // meet is enough for the wide tooth.
    let mut s = Session::default();
    let p = with(
        straight(),
        json!({"tooth_width": 2, "tip_corner": "rounded", "tip_size": 1, "make": "both", "hub_diameter": 40, "master_spline": "wide_tooth"}),
    );
    let r = run(&mut s, CMD, p);
    assert_eq!((r["created_bodies"].clone(), r["positions"].clone()), (json!(["Spline Shaft", "Spline Hub"]), json!(1)), "{r}");
    let c = run(&mut s, "inspect.clearance", json!({"clearance": 1.0}));
    assert_eq!(c["overlaps"], json!([]), "{c}");
}

#[test]
fn large_and_small_splines_are_closed_bodies_of_their_outline() {
    // The dialog's straight-sided spline 50 times larger and at a hundredth of its size.
    for scale in [50.0, 0.01] {
        let mut s = Session::default();
        let p = json!({
            "type": "straight", "teeth": 6, "length": 20.0 * scale, "major_diameter": 28.0 * scale, "minor_diameter": 23.0 * scale,
            "tooth_width": 6.0 * scale, "side_clearance": 0.05 * scale, "major_clearance": 0.3 * scale, "minor_clearance": 0.2 * scale
        });
        run(&mut s, CMD, p);
        let n = SplineNumbers {
            length: 20.0 * scale,
            major_diameter: Some(28.0 * scale),
            minor_diameter: Some(23.0 * scale),
            tooth_width: Some(6.0 * scale),
            side_clearance: Some(0.05 * scale),
            major_clearance: Some(0.3 * scale),
            minor_clearance: Some(0.2 * scale),
            ..numbers(6, 28.0, 6.0)
        };
        let outline = &planned(&n).shaft;
        assert_close(&format!("{scale}: the shaft's volume"), volume(&mut s, "Spline Shaft"), area(outline) * 20.0 * scale);
        assert_eq!(faces(&mut s, "Spline Shaft").len(), outline.tooth.len() * 6 + 2, "{scale}");
    }
}

#[test]
fn a_new_part_takes_its_own_chamfer_on_every_tooth_at_both_ends() {
    let mut s = Session::default();
    let plain = area(&planned(&numbers(6, 28.0, 6.0)).shaft) * 20.0;
    let r = run(&mut s, CMD, with(straight(), json!({"make": "both", "hub_diameter": 50, "end_chamfer": 0.8, "entry_chamfer": 0.8})));
    assert_eq!(r["created_bodies"], json!(["Spline Shaft", "Spline Hub"]), "{r}");
    for body in ["Spline Shaft", "Spline Hub"] {
        let fs = faces(&mut s, body);
        assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 6), "{body}: one chamfer face on each tooth at each end: {fs:?}");
        assert_lasting_names(&fs);
    }
    assert!(volume(&mut s, "Spline Shaft") < plain - 1e-3, "the chamfers take metal off the shaft");
    let c = run(&mut s, "inspect.clearance", json!({"clearance": 1.0}));
    assert_eq!(c["overlaps"], json!([]), "{c}");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    // An Entry chamfer with only a shaft made has no bore to be cut at: it would have no effect, so it is refused
    // and nothing is added.
    assert_eq!(
        refusal(with(straight(), json!({"entry_chamfer": 1}))),
        "`entry_chamfer` is the chamfer where the teeth run out of the hub's bore: give it only when a hub is cut or made"
    );
}

#[test]
fn a_new_straight_shaft_has_the_volume_worked_out_by_hand_from_its_teeth() {
    // This number is worked out by hand from the tooth's shape, not from the engine's outline. A straight-sided tooth
    // with plain tips and no root round is the part of a strip of width w (centred on a radius) between the minor
    // circle (radius r) and the major circle (radius R). The area of the strip inside a circle of radius rho, on the
    // tooth's side of the centre, is S(rho) = (w/2) sqrt(rho^2 - (w/2)^2) + rho^2 asin(w / (2 rho)), so one tooth is
    // S(R) - S(r), and the shaft is the minor circle and its teeth. Six teeth 6 mm wide do not meet at the 23 mm minor
    // circle (each is asin(3 / 11.5), about 15 degrees, either side of its middle; the pitch is 60 degrees).
    let mut s = Session::default();
    run(&mut s, CMD, straight());
    let (teeth, w, r, big_r, length) = (6.0, 6.0, 23.0 / 2.0, 28.0 / 2.0, 20.0);
    let strip = |rho: f64| (w / 2.0) * (rho * rho - (w / 2.0) * (w / 2.0)).sqrt() + rho * rho * (w / (2.0 * rho)).asin();
    let by_hand = (PI * r * r + teeth * (strip(big_r) - strip(r))) * length;
    assert_close("the shaft's volume worked out by hand", volume(&mut s, "Spline Shaft"), by_hand);
}

// ---------------------------------------------------------------------------------------------------------------
// The master spline
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_master_spline_leaves_the_hub_one_position() {
    for (form, master) in [("tooth_out", SplineMaster::ToothOut), ("wide_tooth", SplineMaster::WideTooth)] {
        let mut s = Session::default();
        let p = with(straight(), json!({"make": "both", "hub_diameter": 40, "master_spline": form, "master_angle": 0}));
        let r = run(&mut s, CMD, p.clone());
        assert_eq!(r["positions"], 1, "{r}");
        // A tooth stands on +X; the middle of a wide tooth is half a pitch (30 degrees) on, so the spline turns back by that.
        assert_eq!(r["master_spline"]["tooth"].as_u64(), Some(0), "{r}");
        let turn = r["master_spline"]["turn"].as_f64().unwrap_or(f64::NAN);
        assert!((turn - if form == "tooth_out" { 0.0 } else { -30.0 }).abs() < 1e-12, "{r}");
        // The shaft is the master spline's outline extruded.
        let keyed = SplineNumbers { master, master_angle: Some(0.0), ..numbers(6, 28.0, 6.0) };
        assert_close(form, volume(&mut s, "Spline Shaft"), area(&planned(&keyed).shaft) * 20.0);
        let c = run(&mut s, "inspect.clearance", json!({"clearance": 1.0}));
        assert_eq!(c["overlaps"], json!([]), "{form}: {c}");
        // The hub turned on by a pitch does not go on: it overlaps the shaft.
        run(&mut s, "solid.move", json!({"bodies": ["Spline Hub"], "axis": [0, 0, 1], "angle": "60 deg"}));
        let c = run(&mut s, "inspect.clearance", json!({"bodies": ["Spline Shaft"], "against": ["Spline Hub"]}));
        assert_eq!(c["overlaps"].as_array().map(Vec::len), Some(1), "{form}: {c}");
    }
    // Without a master spline the same turn of a pitch fits.
    let mut s = Session::default();
    run(&mut s, CMD, with(straight(), json!({"make": "both", "hub_diameter": 40})));
    run(&mut s, "solid.move", json!({"bodies": ["Spline Hub"], "axis": [0, 0, 1], "angle": "60 deg"}));
    let c = run(&mut s, "inspect.clearance", json!({"bodies": ["Spline Shaft"], "against": ["Spline Hub"]}));
    assert_eq!(c["overlaps"], json!([]), "{c}");
}

/// The turn of the whole spline that puts the master spline at `angle` degrees, worked out on its own: a tooth's
/// middle is a whole count of pitches from +X, a space's a half more; the nearest is taken, and just between two,
/// the one further on.
fn expected_turn(teeth: u32, form: &str, angle: f64) -> f64 {
    let pitch = 360.0 / f64::from(teeth);
    let half = if form == "wide_tooth" { 0.5 } else { 0.0 };
    angle - ((angle / pitch - half + 0.5).floor() + half) * pitch
}

#[test]
fn the_teeth_and_the_master_spline_are_where_the_angles_say() {
    // Straight-sided teeth of width `w` on 28 over 23 mm, probed on the circle of radius 13, halfway up: a tooth's
    // flank crosses it asin(w / 26) from the tooth's middle. Each probe is 1.5 degrees (0.34 mm) inside or outside
    // a flank, more than the 0.05 mm side clearance, so there the hub's material is exactly where the shaft's is not.
    let flank = |w: f64| (w / 26.0).asin().to_degrees();
    let both = with(straight(), json!({"make": "both", "hub_diameter": 40}));
    // No master spline: tooth 0 is on +X.
    let mut s = Session::default();
    run(&mut s, CMD, both.clone());
    let (f, edge) = (flank(6.0), 1.5);
    let angles = [0.0, f - edge, -(f - edge), f + edge, -(f + edge), 30.0, 60.0, 300.0];
    let shaft = material_at(&s, "Spline Shaft", 13.0, 10.0, &angles);
    assert_eq!(shaft, [true, true, true, false, false, false, true, true], "{angles:?}");
    let hub = material_at(&s, "Spline Hub", 13.0, 10.0, &angles);
    assert_eq!(hub, shaft.iter().map(|m| !m).collect::<Vec<_>>(), "{angles:?}");
    // The master spline at its angle: for a tooth left out the gap runs from flank to flank of the two neighbours,
    // a pitch less a flank either way; for a wide tooth the material runs from the outer flanks of the two teeth
    // either side of the filled space, half a pitch and a flank either way.
    for (teeth, width, form, angle) in [
        (6, 6.0, "tooth_out", 0.0),
        (6, 6.0, "tooth_out", 10.0),
        (6, 6.0, "tooth_out", -150.0),
        (6, 6.0, "tooth_out", 271.3),
        (6, 6.0, "wide_tooth", 0.0),
        (6, 6.0, "wide_tooth", -45.0),
        (3, 4.0, "tooth_out", 90.0),
        (3, 4.0, "wide_tooth", 90.0),
        (2, 4.0, "tooth_out", 45.0),
    ] {
        let mut s = Session::default();
        let p = with(both.clone(), json!({"teeth": teeth, "tooth_width": width, "master_spline": form, "master_angle": angle}));
        let r = run(&mut s, CMD, p);
        let turn = r["master_spline"]["turn"].as_f64().unwrap_or(f64::NAN);
        assert!((turn - expected_turn(teeth, form, angle)).abs() < 1e-9, "{teeth} {form} {angle}: {r}");
        let pitch = 360.0 / f64::from(teeth);
        let f = flank(width);
        let (reach, master) = if form == "tooth_out" { (pitch - f, false) } else { (pitch / 2.0 + f, true) };
        let angles = [angle, angle + reach - edge, angle - reach + edge, angle + reach + edge, angle - reach - edge];
        let shaft = material_at(&s, "Spline Shaft", 13.0, 10.0, &angles);
        assert_eq!(shaft, [master, master, master, !master, !master], "{teeth} {form} {angle}: {angles:?}");
        let hub = material_at(&s, "Spline Hub", 13.0, 10.0, &angles);
        assert_eq!(hub, shaft.iter().map(|m| !m).collect::<Vec<_>>(), "{teeth} {form} {angle}: the hub at {angles:?}");
    }
}

#[test]
fn a_master_spline_is_cut_into_a_picked_shaft() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    let r = run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 55], "master_spline": "tooth_out", "master_angle": 0})));
    assert_eq!(r["positions"], 1, "{r}");
    let keyed = SplineNumbers { master: SplineMaster::ToothOut, master_angle: Some(0.0), ..numbers(6, 28.0, 6.0) };
    assert_close("the keyed shaft", volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 60.0, shaft_area(&keyed), 20.0));
    // The tooth on +X is left out; its neighbours stand.
    assert_eq!(material_at(&s, "Shaft", 13.0, 50.0, &[0.0, 60.0, -60.0]), [false, true, true]);
}

// ---------------------------------------------------------------------------------------------------------------
// A picked shaft
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_picked_shaft_is_cut_from_its_nearer_end_for_the_length() {
    let outline = shaft_area(&numbers(6, 28.0, 6.0));
    // Picked near the top the spaces run down from z = 60; picked near the bottom, up from z = 0.
    for (pick_z, in_spaces, below) in [(55.0, 50.0, 30.0), (5.0, 10.0, 30.0)] {
        let mut s = Session::default();
        shaft(&mut s, 14.0, 60.0);
        let r = run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, pick_z]})));
        assert_eq!(s.doc.features.len(), 2, "one History item added");
        last_spline(&s);
        assert_eq!(r["bodies"], json!(["Shaft"]), "nothing new is made: {r}");
        assert_close(&format!("picked at z = {pick_z}"), volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 60.0, outline, 20.0));
        // Tooth 0 stands on +X and a space is half a pitch on: cut where the teeth are, whole past the length.
        assert_eq!(material_at(&s, "Shaft", 13.0, in_spaces, &[0.0, 30.0, 60.0]), [true, false, true], "z = {pick_z}");
        assert_eq!(material_at(&s, "Shaft", 13.0, below, &[30.0]), [true], "z = {pick_z}");
        assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    }
}

#[test]
fn the_start_face_says_which_end_the_teeth_start_from() {
    let outline = shaft_area(&numbers(6, 28.0, 6.0));
    // The shaft is picked near its top both times: the face the teeth start from decides the end, the bottom
    // (z = 0) or the top (z = 60), and the spaces run from it for the 20 mm.
    for (start_z, in_spaces, past) in [(0.0, 10.0, 50.0), (60.0, 50.0, 10.0)] {
        let mut s = Session::default();
        shaft(&mut s, 14.0, 60.0);
        run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 55], "shaft_start": [5, 0, start_z]})));
        assert_close(&format!("started at z = {start_z}"), volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 60.0, outline, 20.0));
        // Tooth 0 stands on +X and a space is half a pitch on: cut where the teeth are, whole past the length.
        assert_eq!(material_at(&s, "Shaft", 13.0, in_spaces, &[0.0, 30.0]), [true, false], "started at z = {start_z}");
        assert_eq!(material_at(&s, "Shaft", 13.0, past, &[30.0]), [true], "started at z = {start_z}");
        assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    }
    // A shoulder is a start face too: the neck of a stepped shaft, picked near its free end, is cut from the
    // shoulder at z = 40 up for the 8 mm.
    let mut s = Session::default();
    stepped(&mut s);
    let p = json!({
        "type": "straight", "teeth": 6, "length": 8, "minor_diameter": 17, "tooth_width": 4, "side_clearance": 0.05,
        "major_clearance": 0.3, "minor_clearance": 0.2, "shaft_face": [10, 0, 54], "shaft_start": [12.5, 0, 40]
    });
    run(&mut s, CMD, p);
    assert_eq!(material_at(&s, "Shaft", 9.5, 44.0, &[0.0, 30.0]), [true, false], "cut from the shoulder up");
    assert_eq!(material_at(&s, "Shaft", 9.5, 52.0, &[30.0]), [true], "whole past the length");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    // And in a bore: picked near its top, started from the hub's bottom face, the spaces run up from z = 0.
    let mut s = Session::default();
    ring(&mut s, 20.0, 11.0, 0.0, 30.0);
    run(&mut s, CMD, with(straight(), json!({"bore_face": [11, 0, 25], "bore_start": [15, 0, 0]})));
    assert_eq!(material_at(&s, "Hub", 12.5, 10.0, &[0.0, 30.0]), [false, true], "cut from the bottom up");
    assert_eq!(material_at(&s, "Hub", 12.5, 25.0, &[0.0]), [true], "whole past the length");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn a_spline_given_no_start_face_stores_its_end_and_keeps_it_when_the_shaft_grows() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "60 mm"}));
    run(&mut s, "solid.cylinder", json!({"radius": 14, "height": "tall", "body_name": "Shaft"}));
    run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 50]})));
    // The end nearest the pick, the top, is stored as the start: a point of the top face, half the radius in.
    let FeatureKind::ShaftSpline { shaft_start: Some(start), bore_start: None, .. } = last_spline(&s) else {
        panic!("no start stored: {:?}", last_spline(&s))
    };
    let at = start.0;
    assert!((at.z - 60.0).abs() < 1e-9 && (at.x.hypot(at.y) - 7.0).abs() < 1e-9, "{at:?}");
    assert_eq!(material_at(&s, "Shaft", 13.0, 50.0, &[0.0, 30.0]), [true, false], "the teeth are at the top");
    // The shaft made 200 mm long: the pick at z = 50 is now nearer the bottom, and the spline stays at the top.
    run(&mut s, "parameters.change", json!({"name": "tall", "expression": "200 mm"}));
    assert_close("the long shaft", volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 200.0, shaft_area(&numbers(6, 28.0, 6.0)), 20.0));
    assert_eq!(material_at(&s, "Shaft", 13.0, 190.0, &[0.0, 30.0]), [true, false], "the teeth are still at the top");
    assert_eq!(material_at(&s, "Shaft", 13.0, 50.0, &[30.0]), [true], "nothing is cut where the pick was");
    assert_eq!(material_at(&s, "Shaft", 13.0, 10.0, &[30.0]), [true], "nor at the bottom");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn a_start_face_for_a_part_not_picked_is_refused() {
    let shaft_words = "`shaft_start` is the face the teeth of a picked shaft start from: give it only with `shaft_face`";
    let bore_words = "`bore_start` is the face the teeth of a picked bore start from: give it only with `bore_face`";
    assert_eq!(refusal(with(straight(), json!({"shaft_start": [0, 0, 0]}))), shaft_words);
    assert_eq!(refusal(with(straight(), json!({"make": "hub", "hub_diameter": 40, "bore_start": [0, 0, 0]}))), bore_words);
    let mut s = Session::default();
    ring(&mut s, 20.0, 11.0, 0.0, 30.0);
    assert_eq!(refused(&mut s, with(straight(), json!({"bore_face": [11, 0, 25], "shaft_start": [15, 0, 0]}))), shaft_words);
    let not_a_point = refused(&mut s, with(straight(), json!({"bore_face": [11, 0, 25], "bore_start": "top"})));
    assert!(not_a_point.contains("bore_start"), "{not_a_point}");
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    assert_eq!(refused(&mut s, with(straight(), json!({"shaft_face": [14, 0, 55], "bore_start": [5, 0, 60]}))), bore_words);
    assert_eq!(s.doc.features.len(), 1, "nothing was added");
}

#[test]
fn a_chamfer_is_a_start_face_and_the_spline_keeps_its_end_when_the_chamfer_changes_or_goes() {
    // A blank chamfered first: its round face ends 1 mm short of the top, where the chamfer begins.
    let chamfered = |s: &mut Session| -> u64 {
        shaft(s, 14.0, 60.0);
        feature_of(&run(s, "solid.chamfer", json!({"edges": [[14, 0, 60]], "distance": 1, "tangent_chain": false})))
    };
    // The same blank cut by a pick near its top and no start face, to compare with.
    let mut by_pick = Session::default();
    chamfered(&mut by_pick);
    run(&mut by_pick, CMD, with(straight(), json!({"shaft_face": [14, 0, 50]})));
    let want = volume(&mut by_pick, "Shaft");
    // The shaft is picked near its BOTTOM now, and the start face puts the teeth at the top: the chamfer itself
    // (a point of its cone), or the flat end beyond it.
    for (what, start) in [("the chamfer", json!([13.5, 0, 59.5])), ("the flat end", json!([5, 0, 60]))] {
        let mut s = Session::default();
        let chamfer = chamfered(&mut s);
        run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 5], "shaft_start": start})));
        assert_close(&format!("started from {what}"), volume(&mut s, "Shaft"), want);
        assert_eq!(material_at(&s, "Shaft", 13.0, 50.0, &[0.0, 30.0]), [true, false], "{what}: the teeth are at the top");
        assert_eq!(material_at(&s, "Shaft", 13.0, 10.0, &[30.0]), [true], "{what}: nothing is cut at the bottom");
        assert!(notes(&s).is_empty(), "{what}: {:?}", notes(&s));
        // The chamfer made larger: the start face is found again, and the teeth start where it now ends.
        let larger = json!({"edges": [[14, 0, 60]], "distance": 2, "tangent_chain": false});
        run(&mut s, "timeline.redefine", json!({"feature": chamfer, "command": "solid.chamfer", "params": larger}));
        assert_eq!(material_at(&s, "Shaft", 13.0, 50.0, &[0.0, 30.0]), [true, false], "{what}: at the top with the chamfer larger");
        assert_eq!(material_at(&s, "Shaft", 13.0, 10.0, &[30.0]), [true], "{what}: the bottom whole with the chamfer larger");
        assert!(notes(&s).is_empty(), "{what}, the chamfer larger: {:?}", notes(&s));
        // The chamfer taken away: the flat end is still there; the chamfer picked as the start is gone, and the
        // place saved for it still says the top, with a note on the feature and no refusal.
        run(&mut s, "timeline.suppress", json!({"feature": chamfer, "suppressed": true}));
        assert_eq!(material_at(&s, "Shaft", 13.0, 50.0, &[0.0, 30.0]), [true, false], "{what}: at the top without the chamfer");
        assert_eq!(material_at(&s, "Shaft", 13.0, 10.0, &[30.0]), [true], "{what}: the bottom whole without the chamfer");
        let left = notes(&s);
        if what == "the chamfer" {
            assert!(left.len() == 1 && left[0].contains("was not found again"), "{left:?}");
        } else {
            assert!(left.is_empty(), "{left:?}");
        }
    }
}

#[test]
fn the_spline_own_chamfers_are_cut_at_the_end_the_start_face_says() {
    // A shaft picked near its top with its bottom end as the start: the End chamfer is on the teeth at the bottom.
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 55], "shaft_start": [5, 0, 0], "end_chamfer": 1})));
    let fs = faces(&mut s, "Shaft");
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 0), "one chamfer face on each tooth at the start: {fs:?}");
    assert!(fs.iter().filter(|f| stem(&f.name).contains("chamfer")).all(|f| f.centroid[2] < 2.0), "they are at the bottom: {fs:?}");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    // A bore picked near its top with the hub's bottom face as the start: the Entry chamfer is at the bottom.
    let mut s = Session::default();
    ring(&mut s, 20.0, 11.0, 0.0, 30.0);
    run(&mut s, CMD, with(straight(), json!({"bore_face": [11, 0, 25], "bore_start": [15, 0, 0], "entry_chamfer": 1})));
    let fs = faces(&mut s, "Hub");
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 0), "one chamfer face on each tooth at the start: {fs:?}");
    assert!(fs.iter().filter(|f| stem(&f.name).contains("chamfer")).all(|f| f.centroid[2] < 2.0), "they are at the bottom: {fs:?}");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn the_shaft_is_turned_down_or_built_up_to_the_spline_and_keeps_its_name() {
    let want = cut_shaft_volume(14.0, 60.0, shaft_area(&numbers(6, 28.0, 6.0)), 20.0);
    for radius in [12.0, 14.0, 16.0] {
        let mut s = Session::default();
        let c = shaft(&mut s, radius, 60.0);
        run(&mut s, CMD, with(straight(), json!({"shaft_face": [radius, 0, 55]})));
        // The whole round face follows the major diameter, so each is the 28 mm shaft cut.
        assert_close(&format!("a {radius} mm shaft"), volume(&mut s, "Shaft"), want);
        let fs = faces(&mut s, "Shaft");
        let side: Vec<f64> = fs.iter().filter(|f| stem(&f.name) == format!("F{c}:side")).filter_map(|f| f.radius).collect();
        assert!(!side.is_empty() && side.iter().all(|r| (r - 14.0).abs() < 1e-9), "{radius}: the surface keeps the shaft's name: {fs:?}");
        assert!(fs.iter().filter_map(|f| f.radius).all(|r| r <= 14.0 + 1e-9), "{radius}: nothing is left of the old surface: {fs:?}");
        assert!(has(&fs, &format!("F{c}:top")) && has(&fs, &format!("F{c}:bottom")), "{fs:?}");
        assert_lasting_names(&fs);
        assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    }
}

/// A hole across the shaft, a keyway and a flat, all clear of the 20 mm the teeth of [`straight`] take from the top
/// of a 60 mm shaft, and each cut right through whatever radius the shaft has.
fn openings(s: &mut Session) {
    run(s, "solid.cylinder", json!({"radius": 2, "height": 80, "axis": "Y", "base": [0, -40, 20], "operation": "cut", "targets": ["Shaft"]}));
    run(s, "solid.box", json!({"length": 40, "width": 4, "height": 10, "corner": [9, -2, 26], "operation": "cut", "targets": ["Shaft"]}));
    run(s, "solid.box", json!({"length": 40, "width": 80, "height": 8, "corner": [-49, -40, 4], "operation": "cut", "targets": ["Shaft"]}));
}

/// What opens in a shaft's round face stays open when the spline resizes the shaft: the hole, the keyway and the
/// flat are carried to the new surface along their own walls. Cutting them before the spline gives the very solid
/// that cutting them after it gives, whether the shaft is built up or turned down.
#[test]
fn what_opens_in_a_shaft_stays_open_when_the_spline_resizes_it() {
    for radius in [12.0, 16.0] {
        let spline = with(straight(), json!({"shaft_face": [radius, 0, 55]}));
        let mut before = Session::default();
        let c = shaft(&mut before, radius, 60.0);
        openings(&mut before);
        run(&mut before, CMD, spline.clone());
        assert!(notes(&before).is_empty(), "{radius}: {:?}", notes(&before));
        let mut after = Session::default();
        shaft(&mut after, radius, 60.0);
        run(&mut after, CMD, spline);
        openings(&mut after);
        assert!(notes(&after).is_empty(), "{radius}: {:?}", notes(&after));
        assert_close(&format!("a {radius} mm shaft, cut before the spline and after it"), volume(&mut before, "Shaft"), volume(&mut after, "Shaft"));
        // Just under the new 14 mm surface: open in the hole, the keyway and the flat, whole beside them.
        assert_eq!(material_at(&before, "Shaft", 13.5, 20.0, &[90.0, 270.0]), [false, false], "{radius}: the hole across");
        assert_eq!(material_at(&before, "Shaft", 13.5, 31.0, &[0.0]), [false], "{radius}: the keyway");
        assert_eq!(material_at(&before, "Shaft", 13.5, 8.0, &[180.0]), [false], "{radius}: the flat");
        assert_eq!(material_at(&before, "Shaft", 13.5, 20.0, &[0.0, 180.0]), [true, true], "{radius}: the shaft beside the hole");
        assert_eq!(material_at(&before, "Shaft", 14.5, 20.0, &[0.0]), [false], "{radius}: the shaft is the spline's 28 mm");
        // The surface is still the shaft's own.
        let fs = faces(&mut before, "Shaft");
        assert!(
            fs.iter().any(|f| stem(&f.name) == format!("F{c}:side") && f.radius.is_some_and(|r| (r - 14.0).abs() < 1e-9)),
            "{radius}: the shaft's surface keeps its name at the new radius: {fs:?}"
        );
    }
}

/// A chamfer or a round the shaft has on its far end keeps its own size when the shaft is built up, with a hole
/// across the shaft and without one: the same solid as a chamfer or a round of that size made on the larger shaft
/// after the spline.
#[test]
fn a_chamfer_or_a_round_on_the_end_of_a_shaft_keeps_its_size_when_the_shaft_is_built_up() {
    let hole = |s: &mut Session| {
        run(s, "solid.cylinder", json!({"radius": 2, "height": 80, "axis": "Y", "base": [0, -40, 20], "operation": "cut", "targets": ["Shaft"]}));
    };
    for (tool, size) in [("solid.chamfer", "distance"), ("solid.fillet", "radius")] {
        let on_the_end = |s: &mut Session, radius: f64| {
            let mut p = json!({"edges": [[radius, 0, 0]], size: 1});
            if tool == "solid.chamfer" {
                p = with(p, json!({"tangent_chain": false}));
            }
            run(s, tool, p);
        };
        for holed in [true, false] {
            let mut before = Session::default();
            shaft(&mut before, 12.0, 60.0);
            if holed {
                hole(&mut before);
            }
            on_the_end(&mut before, 12.0);
            run(&mut before, CMD, with(straight(), json!({"shaft_face": [12, 0, 55]})));
            assert!(notes(&before).is_empty(), "{tool}, hole {holed}: {:?}", notes(&before));
            let mut after = Session::default();
            shaft(&mut after, 12.0, 60.0);
            run(&mut after, CMD, with(straight(), json!({"shaft_face": [12, 0, 55]})));
            if holed {
                hole(&mut after);
            }
            on_the_end(&mut after, 14.0);
            assert!(notes(&after).is_empty(), "{tool}, hole {holed}: {:?}", notes(&after));
            assert_close(
                &format!("{tool} on the far end (hole {holed}), made before the spline and after it"),
                volume(&mut before, "Shaft"),
                volume(&mut after, "Shaft"),
            );
            assert_eq!(material_at(&before, "Shaft", 13.5, 20.0, &[90.0, 270.0]), [!holed, !holed], "{tool}: the hole across, where there is one");
        }
    }
}

/// A shaft whose spline is larger than the collar next to its round face grows past the collar, and what opens in
/// the round face stays open there too: the same solid as cutting the openings after the spline.
#[test]
fn a_shaft_grows_past_the_collar_next_to_it_and_what_opens_in_it_stays_open() {
    let collar = |s: &mut Session| {
        run(s, "solid.cylinder", json!({"radius": 16, "height": 10, "base": [0, 0, -10], "operation": "join", "targets": ["Shaft"]}));
    };
    // 34 mm across over a 32 mm collar.
    let large = with(straight(), json!({"shaft_face": [12, 0, 55], "major_diameter": 34, "minor_diameter": 29, "tooth_width": 7}));
    let mut plain = Session::default();
    shaft(&mut plain, 12.0, 60.0);
    collar(&mut plain);
    run(&mut plain, CMD, large.clone());
    assert!(notes(&plain).is_empty(), "{:?}", notes(&plain));
    assert_eq!(material_at(&plain, "Shaft", 16.5, 20.0, &[0.0, 90.0]), [true, true], "the shaft is larger than its collar now");
    assert_eq!(material_at(&plain, "Shaft", 16.5, -5.0, &[0.0]), [false], "the collar keeps its own size");
    let mut before = Session::default();
    shaft(&mut before, 12.0, 60.0);
    collar(&mut before);
    openings(&mut before);
    run(&mut before, CMD, large.clone());
    assert!(notes(&before).is_empty(), "{:?}", notes(&before));
    let mut after = Session::default();
    shaft(&mut after, 12.0, 60.0);
    collar(&mut after);
    run(&mut after, CMD, large);
    openings(&mut after);
    assert!(notes(&after).is_empty(), "{:?}", notes(&after));
    assert_close("a shaft next to a collar, cut before the spline and after it", volume(&mut before, "Shaft"), volume(&mut after, "Shaft"));
    assert_eq!(material_at(&before, "Shaft", 16.5, 20.0, &[90.0, 270.0]), [false, false], "the hole across");
    assert_eq!(material_at(&before, "Shaft", 16.5, 20.0, &[0.0, 180.0]), [true, true], "the shaft beside the hole");
}

/// What opens in a hub's bore stays open when the bore is narrowed to the tops of the hub's teeth: a set screw's
/// hole cut before the spline gives the very solid that cutting it after the spline gives, where the teeth run
/// the whole bore and where they stop short of its far end (the bore then keeps its own size past them).
#[test]
fn what_opens_in_a_bore_stays_open_when_the_spline_narrows_it() {
    let screw_holes = |s: &mut Session, thick: f64| {
        // One within the 20 mm of the teeth, and one past them where the hub is long enough.
        run(s, "solid.cylinder", json!({"radius": 1.5, "height": 80, "axis": "Y", "base": [0, -40, 10], "operation": "cut", "targets": ["Hub"]}));
        if thick > 26.0 {
            run(s, "solid.cylinder", json!({"radius": 1.5, "height": 80, "axis": "X", "base": [-40, 0, 25], "operation": "cut", "targets": ["Hub"]}));
        }
    };
    let minor = planned(&numbers(6, 28.0, 6.0)).hub.least_radius;
    assert!(minor < 12.5, "the test's bore of 13 mm must be wider than the tops of the hub's teeth at {minor}");
    for thick in [20.0, 30.0] {
        let spline = with(straight(), json!({"bore_face": [13, 0, 5], "bore_start": [15, 0, 0]}));
        let mut before = Session::default();
        ring(&mut before, 22.0, 13.0, 0.0, thick);
        screw_holes(&mut before, thick);
        run(&mut before, CMD, spline.clone());
        assert!(notes(&before).is_empty(), "{thick}: {:?}", notes(&before));
        let mut after = Session::default();
        ring(&mut after, 22.0, 13.0, 0.0, thick);
        run(&mut after, CMD, spline);
        screw_holes(&mut after, thick);
        assert!(notes(&after).is_empty(), "{thick}: {:?}", notes(&after));
        assert_close(
            &format!("a hub {thick} mm long, drilled before the spline and after it"),
            volume(&mut before, "Hub"),
            volume(&mut after, "Hub"),
        );
        if thick > 26.0 {
            // Past the teeth the bore keeps its own 13 mm.
            assert_eq!(material_at(&before, "Hub", 12.5, 27.0, &[45.0]), [false], "{thick}: the bore past the teeth");
        }
    }
}

#[test]
fn a_size_left_out_or_given_to_its_last_places_is_the_shaft_own() {
    let want = cut_shaft_volume(15.0, 60.0, shaft_area(&numbers(6, 30.0, 6.0)), 20.0);
    // 30.02 mm is within a thousandth of the 15 mm radius of the shaft's own 30 mm: it is the shaft's own.
    for size in [None, Some(30.0), Some(30.02)] {
        let mut s = Session::default();
        shaft(&mut s, 15.0, 60.0);
        let p = without(with(straight(), json!({"shaft_face": [15, 0, 55]})), "major_diameter");
        let p = match size {
            Some(d) => with(p, json!({"major_diameter": d})),
            None => p,
        };
        let r = run(&mut s, CMD, p);
        assert!((r["shaft_major_diameter"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9, "{size:?}: {r}");
        assert_close(&format!("{size:?}"), volume(&mut s, "Shaft"), want);
        let FeatureKind::ShaftSpline { major_diameter, .. } = last_spline(&s) else { panic!("not a Shaft Spline") };
        assert_eq!(major_diameter.is_some(), size.is_some(), "a size left out stays left out, so the rebuild takes the shaft's");
    }
}

#[test]
fn involute_teeth_of_one_size_grow_the_shaft_with_their_count() {
    // A module of 30/21 puts the tips of 20 teeth on a 30 mm shaft (the tips' circle is module × (teeth + 1));
    // 24 teeth of that module grow it to 25 × 30/21 mm, 16 shrink it to 17 × 30/21 mm.
    let module = 30.0 / 21.0;
    for (teeth, given) in [(20, false), (24, true), (16, true)] {
        let mut s = Session::default();
        let c = shaft(&mut s, 15.0, 60.0);
        let mut p =
            json!({"type": "involute", "teeth": teeth, "length": 10, "side_clearance": 0.06, "tip_clearance": 0.2, "shaft_face": [15, 0, 55]});
        if given {
            p = with(p, json!({"module": module}));
        }
        let r = run(&mut s, CMD, p);
        let top = module * f64::from(teeth + 1) / 2.0;
        assert!((r["shaft_major_diameter"].as_f64().unwrap_or(f64::NAN) - 2.0 * top).abs() < 1e-9, "{teeth}: {r}");
        let fs = faces(&mut s, "Shaft");
        assert!(
            fs.iter().any(|f| stem(&f.name) == format!("F{c}:side") && f.radius.is_some_and(|r| (r - top).abs() < 1e-9)),
            "{teeth}: the shaft's surface at the tips' radius {top}: {fs:?}"
        );
        let outline = shaft_area(&involute_numbers(teeth, module));
        let got = volume(&mut s, "Shaft");
        let want = cut_shaft_volume(top, 60.0, outline, 10.0);
        assert!((got - want).abs() < 1e-6 * want, "{teeth}: {got}, expected {want}");
        assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    }
}

#[test]
fn a_shaft_along_x_is_cut_as_one_along_z() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Shaft section"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 14}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 60, "operation": "new", "body_name": "Shaft"}));
    let b = measure(&mut s, "Shaft");
    let (lo, hi) = (b["bbox"]["min"][0].as_f64().unwrap_or(f64::NAN), b["bbox"]["max"][0].as_f64().unwrap_or(f64::NAN));
    assert!((hi - lo - 60.0).abs() < 1e-6, "the shaft runs 60 mm along X: {b}");
    // Picked 5 mm from its end away from the origin.
    let end = if hi.abs() >= lo.abs() { hi - 5.0 } else { lo + 5.0 };
    run(&mut s, CMD, with(straight(), json!({"shaft_face": [end, 14, 0]})));
    assert_close("along X", volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 60.0, shaft_area(&numbers(6, 28.0, 6.0)), 20.0));
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn spaces_cut_with_a_cutter_end_in_its_curve() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    let outline_area = shaft_area(&numbers(6, 28.0, 6.0));
    let square = cut_shaft_volume(14.0, 60.0, outline_area, 20.0);
    let r = run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 55], "cutter_diameter": 40})));
    let f = feature_of(&r);
    let got = volume(&mut s, "Shaft");
    // The straight part of each space runs the length, as a square-ended cut does; the cutter's curve then turns out
    // of the shaft beyond the length, away from the free end. So the rounded ends take metal past the length that a
    // square end leaves: less is left than after the square-ended cut of the same length.
    assert!(got < square - 1.0, "the rounded ends cut past the length: {got} against {square} square");
    // The curve is a circle of the cutter's radius rc about an axis rc above the bottom of the space at the end of the
    // length: in the middle of a space it reaches the tops of the teeth (depth = 14 - 11.5 mm above the bottom) the
    // run-out sqrt(rc^2 - (rc - depth)^2) past the length, and below the curve it leaves metal a square end would cut.
    // A square-ended cut longer by the run-out therefore takes more. (Off the middle of the space, at the corners
    // where the bottom meets the sides, the bottom lies a little lower and the curve reaches a few tenths of a mm past
    // the run-out; that sliver is far smaller than the metal left under the curve, some tens of mm^3 a space.)
    let (rc, depth): (f64, f64) = (40.0 / 2.0, 14.0 - 11.5);
    let run_out = (rc * rc - (rc - depth) * (rc - depth)).sqrt();
    let longer = cut_shaft_volume(14.0, 60.0, outline_area, 20.0 + run_out);
    assert!(got > longer, "the rounded ends leave metal under the cutter's curve: {got} against {longer} square over {run_out} mm more");
    let fs = faces(&mut s, "Shaft");
    assert_lasting_names(&fs);
    for k in 0..6 {
        for part in ["flank:0", "flank:1", "root", "end:flank:0", "end:flank:1", "end:root"] {
            assert!(has(&fs, &format!("F{f}:space{k}:{part}")), "space {k} {part}: {fs:?}");
        }
    }
    // Teeth of the same spline the whole length of a shaft run right through: no space ends in the cutter's curve.
    let mut s = Session::default();
    shaft(&mut s, 14.0, 30.0);
    run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 25], "length": 30, "cutter_diameter": 40})));
    let fs = faces(&mut s, "Shaft");
    assert!(!fs.iter().any(|x| stem(&x.name).contains(":end:")), "{fs:?}");
    assert_close("teeth the whole length", volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 30.0, shaft_area(&numbers(6, 28.0, 6.0)), 30.0));
}

#[test]
fn a_space_too_wide_for_the_cutter_ends_square_and_the_feature_says_so() {
    // Two teeth 1.5 mm wide leave two spaces of more than 170 degrees each: more than one cutter's curve reaches
    // round. Nothing is refused: those spaces end square, exactly as a spline cut without a cutter ends, and the
    // feature says so in a warning.
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    let wide = with(straight(), json!({"teeth": 2, "tooth_width": 1.5, "shaft_face": [14, 0, 55]}));
    let r = run(&mut s, CMD, with(wide.clone(), json!({"cutter_diameter": 40})));
    let f = feature_of(&r);
    let got = volume(&mut s, "Shaft");
    let told = notes(&s);
    assert!(told.iter().any(|w| w.contains("2 of the 2 tooth spaces end square")), "{told:?}");
    let fs = faces(&mut s, "Shaft");
    assert_lasting_names(&fs);
    assert!(!fs.iter().any(|x| stem(&x.name).contains(":end:")), "no space ends in the cutter's curve: {fs:?}");
    for k in 0..2 {
        assert!(has(&fs, &format!("F{f}:space{k}:root")), "space {k}: {fs:?}");
    }
    // The same spline without a cutter: the same solid.
    let mut plain = Session::default();
    shaft(&mut plain, 14.0, 60.0);
    run(&mut plain, CMD, wide);
    assert!(notes(&plain).is_empty(), "{:?}", notes(&plain));
    assert_close("two wide spaces end square", got, volume(&mut plain, "Shaft"));
}

/// The spline of a neck, cut from the neck's free end for its whole length, with a cutter: `shaft_face` near the
/// top of the 20 mm neck of [`stepped`], which stands on a 30 mm step from z = 40.
fn neck_to_the_step(cutter_diameter: f64) -> Value {
    json!({
        "type": "straight", "teeth": 6, "length": 15, "minor_diameter": 17, "tooth_width": 4, "side_clearance": 0.05,
        "major_clearance": 0.3, "minor_clearance": 0.2, "shaft_face": [10, 0, 54], "shaft_start": [3, 0, 55],
        "cutter_diameter": cutter_diameter
    })
}

#[test]
fn the_cutter_runs_out_through_a_larger_step_as_an_open_slot() {
    // The teeth run the whole neck, down to the step at z = 40, so the cutter's curve runs on into the step. A
    // cutter cuts everything inside its circle: each space comes out through the step as a slot open to the
    // outside, and no ring of the step is left over the spaces.
    let mut s = Session::default();
    stepped(&mut s);
    run(&mut s, CMD, neck_to_the_step(30.0));
    volume(&mut s, "Shaft");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    // Tooth 0 stands on +X and a space is half a pitch on (30 degrees). In the neck: the space is cut.
    assert_eq!(material_at(&s, "Shaft", 9.5, 47.0, &[0.0, 30.0]), [true, false]);
    // In the step, between the tops of the teeth (10 mm) and the step's surface (15 mm): the cutter's circle has
    // its centre 8.5 + 15 = 23.5 mm from the axis in the plane z = 40, so at the radius r it reaches
    // sqrt(15^2 - (23.5 - r)^2) below that plane: 10.2 mm at r = 12.5, 12 mm at r = 14.5.
    for (radius, cut_z, whole_z) in [(12.5, 39.0, 28.5), (12.5, 31.0, 28.5), (14.5, 29.0, 27.0)] {
        assert_eq!(
            material_at(&s, "Shaft", radius, cut_z, &[30.0]),
            [false],
            "the slot is open at r = {radius}, z = {cut_z}: no ring over the space"
        );
        assert_eq!(material_at(&s, "Shaft", radius, whole_z, &[30.0]), [true], "below the cutter's circle at r = {radius}, z = {whole_z}");
        assert_eq!(material_at(&s, "Shaft", radius, cut_z, &[0.0]), [true], "the step is whole between the slots at r = {radius}, z = {cut_z}");
    }
    // The bottom of a space rises along the cutter's circle: 8.5 mm from the axis at the step's top, and
    // 23.5 - sqrt(15^2 - 5^2) = 9.36 mm from it 5 mm further down.
    assert_eq!(material_at(&s, "Shaft", 9.0, 35.0, &[30.0]), [true]);
    assert_eq!(material_at(&s, "Shaft", 9.7, 35.0, &[30.0]), [false]);
}

#[test]
fn a_cutter_too_small_to_run_out_through_the_step_is_refused_in_words() {
    // The step stands 5 mm above the teeth: the cutter's centre must lie above it, with the tools' margin
    // (0.15 mm, a tenth of the 1.5 mm depth) twice: a radius of more than 15.15 - 8.5 + 0.15 = 6.8 mm.
    let mut s = Session::default();
    stepped(&mut s);
    let words = refused(&mut s, neck_to_the_step(12.0));
    assert!(words.starts_with("`cutter_diameter` must be more than 13.6 mm to run out through the step past the end of the teeth"), "{words}");
    assert_eq!(s.doc.features.len(), 3, "nothing was added");
    // A little more is enough.
    run(&mut s, CMD, neck_to_the_step(14.0));
    volume(&mut s, "Shaft");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

// ---------------------------------------------------------------------------------------------------------------
// A picked bore
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_picked_bore_gets_the_hub_spaces_cut_from_its_nearer_end() {
    let mut s = Session::default();
    ring(&mut s, 20.0, 11.0, 0.0, 30.0);
    let features = s.doc.features.len();
    let r = run(&mut s, CMD, with(straight(), json!({"bore_face": [11, 0, 25]})));
    assert_eq!((s.doc.features.len(), r["bodies"].clone()), (features + 1, json!(["Hub"])), "{r}");
    last_spline(&s);
    // The hub's opening holds the 22 mm bore (its teeth's tips lie on 23.4 mm): the cut takes the opening less the
    // bore over the length.
    let opening = area(&planned(&numbers(6, 28.0, 6.0)).hub);
    let blank = PI * (20.0 * 20.0 - 11.0 * 11.0) * 30.0;
    assert_close("the hub", volume(&mut s, "Hub"), blank - (opening - PI * 11.0 * 11.0) * 20.0);
    assert_lasting_names(&faces(&mut s, "Hub"));
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn a_shaft_and_its_bore_cut_together_fit() {
    // A hub bored to the shaft's own diameter sits on it: the bore's point lies on the shaft's surface too.
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    ring(&mut s, 22.0, 14.0, 40.0, 20.0);
    let features = s.doc.features.len();
    run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 35], "bore_face": [14, 0, 52]})));
    assert_eq!(s.doc.features.len(), features + 1, "one History item for both");
    volume(&mut s, "Shaft");
    volume(&mut s, "Hub");
    let c = run(&mut s, "inspect.clearance", json!({"bodies": ["Shaft"], "against": ["Hub"]}));
    assert_eq!(c["overlaps"], json!([]), "{c}");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

// ---------------------------------------------------------------------------------------------------------------
// Chamfers on a picked face
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_blank_chamfered_first_is_the_same_solid_as_the_spline_end_chamfer() {
    // The 15 mm shaft is turned down to the 28 mm spline: the chamfer it had keeps its size on the smaller shaft.
    for radius in [14.0, 15.0] {
        let mut first = Session::default();
        shaft(&mut first, radius, 60.0);
        run(&mut first, "solid.chamfer", json!({"edges": [[radius, 0, 60]], "distance": 1, "tangent_chain": false}));
        run(&mut first, CMD, with(straight(), json!({"shaft_face": [radius, 0, 50]})));
        // The chamfered blank's round face ends 1 mm short of the end, and its spline runs from there: the spline's
        // own chamfer on a plain blank runs 1 mm further to end at the same place.
        let mut own = Session::default();
        shaft(&mut own, radius, 60.0);
        run(&mut own, CMD, with(straight(), json!({"shaft_face": [radius, 0, 50], "end_chamfer": 1, "length": 21})));
        let (a, b) = (volume(&mut first, "Shaft"), volume(&mut own, "Shaft"));
        assert!((a - b).abs() < 0.02, "{radius}: chamfered first {a}, the spline's own chamfer {b}");
        let fs = faces(&mut own, "Shaft");
        assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 0), "{radius}: one chamfer face on each tooth at the free end: {fs:?}");
        assert!(notes(&first).is_empty() && notes(&own).is_empty(), "{:?} {:?}", notes(&first), notes(&own));
    }
}

#[test]
fn teeth_the_whole_length_take_the_end_chamfer_at_both_ends_and_follow_the_count() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 30.0);
    let p = with(straight(), json!({"shaft_face": [14, 0, 25], "length": 30, "end_chamfer": 1}));
    let f = feature_of(&run(&mut s, CMD, p.clone()));
    let fs = faces(&mut s, "Shaft");
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 6), "{fs:?}");
    assert!(volume(&mut s, "Shaft") < cut_shaft_volume(14.0, 30.0, shaft_area(&numbers(6, 28.0, 6.0)), 30.0) - 1e-3);
    // Eight narrower teeth: the chamfers are made again on each of them.
    redefine(&mut s, f, with(p, json!({"teeth": 8, "tooth_width": 4})));
    let fs = faces(&mut s, "Shaft");
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (8, 8), "{fs:?}");
    volume(&mut s, "Shaft");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn an_entry_chamfer_is_cut_only_where_the_teeth_run_out_of_the_bore() {
    let mut s = Session::default();
    ring(&mut s, 20.0, 11.0, 0.0, 30.0);
    run(&mut s, CMD, with(straight(), json!({"bore_face": [11, 0, 25], "entry_chamfer": 1})));
    let fs = faces(&mut s, "Hub");
    // The teeth start at the top and stop 10 mm short of the bottom: a chamfer on each tooth at the top only.
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 0), "{fs:?}");
    let opening = area(&planned(&numbers(6, 28.0, 6.0)).hub);
    let plain = PI * (20.0 * 20.0 - 11.0 * 11.0) * 30.0 - (opening - PI * 11.0 * 11.0) * 20.0;
    assert!(volume(&mut s, "Hub") < plain - 1e-3, "the chamfer takes metal off the hub");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

/// The volume of the new parts `p` makes, after checking each has its chamfer face on every tooth at both ends
/// when `chamfered`, and none otherwise.
fn chamfered_volumes(p: Value, chamfered: bool) -> Vec<f64> {
    let mut s = Session::default();
    let r = run(&mut s, CMD, p);
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    let bodies: Vec<String> =
        r["created_bodies"].as_array().map(|b| b.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let each = if chamfered { 6 } else { 0 };
    bodies
        .iter()
        .map(|body| {
            let fs = faces(&mut s, body);
            assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (each, each), "{body}: {fs:?}");
            volume(&mut s, body)
        })
        .collect()
}

/// A chamfer given a second distance, or an angle, runs as far across the end as they say: a second distance equal
/// to the first and an angle of 45 degrees are the chamfer alone, and an angle is the second distance its tangent
/// gives. The shaft's End chamfer runs in from the major diameter and the hub's Entry chamfer out from the tops of
/// its teeth, so a shorter reach across takes less metal off either part.
#[test]
fn a_chamfer_runs_across_the_end_as_far_as_its_second_distance_or_its_angle_says() {
    let both = with(straight(), json!({"make": "both", "hub_diameter": 50}));
    let made = |sizes: Value| chamfered_volumes(with(both.clone(), sizes), true);
    let alone = made(json!({"end_chamfer": 1, "entry_chamfer": 1}));
    let same = [
        made(json!({"end_chamfer": 1, "end_chamfer_distance2": 1, "entry_chamfer": 1, "entry_chamfer_distance2": 1})),
        made(json!({"end_chamfer": 1, "end_chamfer_angle": 45, "entry_chamfer": 1, "entry_chamfer_angle": 45})),
        made(json!({"end_chamfer": "1 mm", "end_chamfer_angle": "45 deg", "entry_chamfer": 1, "entry_chamfer_angle": "90 deg / 2"})),
    ];
    for volumes in &same {
        for ((part, got), want) in ["shaft", "hub"].iter().zip(volumes).zip(&alone) {
            assert_close(&format!("the {part} at 45 degrees"), *got, *want);
        }
    }
    let half = made(json!({"end_chamfer": 1, "end_chamfer_distance2": 0.5, "entry_chamfer": 1, "entry_chamfer_distance2": 0.5}));
    let slope = 0.5_f64.atan().to_degrees();
    let sloped = made(json!({"end_chamfer": 1, "end_chamfer_angle": slope, "entry_chamfer": 1, "entry_chamfer_angle": slope}));
    for (((part, half), sloped), alone) in ["shaft", "hub"].iter().zip(&half).zip(&sloped).zip(&alone) {
        assert!(*half > alone + 1e-3, "the {part}: half as far across takes less metal off: {half} against {alone}");
        assert_close(&format!("the {part} at the angle whose tangent is a half"), *sloped, *half);
    }
    // A chamfer of 0 is no chamfer, with a second distance or an angle given or not.
    let zero = json!({"end_chamfer": 0, "end_chamfer_angle": 30, "entry_chamfer": 0, "entry_chamfer_distance2": 1});
    let none = chamfered_volumes(with(both.clone(), zero), false);
    let plain = chamfered_volumes(both, false);
    for ((part, got), want) in ["shaft", "hub"].iter().zip(&none).zip(&plain) {
        assert_close(&format!("the {part} with chamfers of 0"), *got, *want);
    }
}

/// Which of the four optional chamfer values the design's last Shaft Spline keeps: the End chamfer's second
/// distance and angle, then the Entry chamfer's.
fn chamfer_values(s: &Session) -> [bool; 4] {
    match last_spline(s) {
        FeatureKind::ShaftSpline { end_chamfer_distance2, end_chamfer_angle, entry_chamfer_distance2, entry_chamfer_angle, .. } => {
            [end_chamfer_distance2.is_some(), end_chamfer_angle.is_some(), entry_chamfer_distance2.is_some(), entry_chamfer_angle.is_some()]
        }
        _ => [false; 4],
    }
}

/// The second distance and the angle are kept on the feature as given, and an edit through `timeline.redefine`
/// changes a chamfer from one type to the other.
#[test]
fn a_chamfer_second_distance_or_angle_is_kept_on_the_feature_and_changed_by_an_edit() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 30.0);
    let p = with(straight(), json!({"shaft_face": [14, 0, 25], "length": 30, "end_chamfer": 1, "end_chamfer_distance2": 0.5}));
    let f = feature_of(&run(&mut s, CMD, p.clone()));
    assert_eq!(chamfer_values(&s), [true, false, false, false], "{:?}", last_spline(&s));
    let fs = faces(&mut s, "Shaft");
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 6), "{fs:?}");
    let two_distances = volume(&mut s, "Shaft");
    // The same chamfer as a distance and an angle: the angle whose tangent is a half.
    let edited = with(without(p, "end_chamfer_distance2"), json!({"end_chamfer_angle": 0.5_f64.atan().to_degrees()}));
    redefine(&mut s, f, edited);
    assert_eq!(chamfer_values(&s), [false, true, false, false], "{:?}", last_spline(&s));
    let fs = faces(&mut s, "Shaft");
    assert_eq!((chamfers(&fs, false), chamfers(&fs, true)), (6, 6), "{fs:?}");
    assert_close("the same chamfer by its angle", volume(&mut s, "Shaft"), two_distances);
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

/// A second distance or an angle without its chamfer, both for one chamfer, or a value out of its range is refused
/// by the plan before anything is added, naming a parameter to change; the dialog's drawing says the same.
#[test]
fn a_chamfer_second_distance_or_angle_is_refused_without_its_chamfer_together_or_out_of_range() {
    let both = with(straight(), json!({"make": "both", "hub_diameter": 50}));
    let end = ["end_chamfer", "end_chamfer_distance2", "end_chamfer_angle"];
    let entry = ["entry_chamfer", "entry_chamfer_distance2", "entry_chamfer_angle"];
    for (changes, names) in [
        (json!({"end_chamfer_distance2": 0.5}), &end),
        (json!({"end_chamfer_angle": 30}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_distance2": 0.5, "end_chamfer_angle": 30}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_angle": 90}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_angle": 0}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_angle": -30}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_angle": 120}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_distance2": 0}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_distance2": -1}), &end),
        (json!({"end_chamfer": 1, "end_chamfer_distance2": 1e6}), &end),
        (json!({"entry_chamfer_distance2": 0.5}), &entry),
        (json!({"entry_chamfer_angle": 30}), &entry),
        (json!({"entry_chamfer": 1, "entry_chamfer_distance2": 0.5, "entry_chamfer_angle": 30}), &entry),
        (json!({"entry_chamfer": 1, "entry_chamfer_angle": 90}), &entry),
        (json!({"entry_chamfer": 1, "entry_chamfer_distance2": -1}), &entry),
    ] {
        let p = with(both.clone(), changes);
        let words = refusal(p.clone());
        let drawn = shaft_spline_preview(&p).expect_err("the preview refuses it");
        assert_eq!(drawn.words, words, "{p}");
        let key = drawn.key.as_deref().unwrap_or_else(|| panic!("{p}: the refusal names no parameter: {words}"));
        assert!(names.contains(&key), "{p}: {words}");
        assert!(words.contains(&format!("`{key}`")), "{p}: {words}");
    }
    // The second distance or the angle that is the one to change is named, not only its chamfer.
    for (changes, key) in [
        (json!({"end_chamfer": 1, "end_chamfer_angle": 90}), "end_chamfer_angle"),
        (json!({"end_chamfer": 1, "end_chamfer_distance2": -1}), "end_chamfer_distance2"),
        (json!({"entry_chamfer": 1, "entry_chamfer_angle": 0}), "entry_chamfer_angle"),
        (json!({"entry_chamfer": 1, "entry_chamfer_distance2": 0}), "entry_chamfer_distance2"),
    ] {
        let p = with(both.clone(), changes);
        assert_eq!(shaft_spline_preview(&p).expect_err("the preview refuses it").key.as_deref(), Some(key), "{p}");
    }
}

/// A stepped shaft as one body called Shaft: 30 mm across up to z = 40, then a neck 20 mm across up to z = 55.
fn stepped(s: &mut Session) {
    shaft(s, 15.0, 40.0);
    run(s, "sketch.create", json!({"plane": "XY", "offset": 40, "name": "Neck section"}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 15, "operation": "join"}));
}

#[test]
fn on_a_stepped_shaft_the_end_chamfer_is_cut_at_a_step_and_left_out_at_a_shoulder() {
    // The teeth start at the step down to the neck: a chamfer on each tooth there, the neck untouched.
    let mut s = Session::default();
    stepped(&mut s);
    let p = without(with(straight(), json!({"shaft_face": [15, 0, 38], "cutter_diameter": 60, "end_chamfer": 1})), "major_diameter");
    run(&mut s, CMD, p);
    let fs = faces(&mut s, "Shaft");
    let cones: Vec<&Face> = fs.iter().filter(|f| stem(&f.name).contains("chamfer")).collect();
    assert_eq!(cones.len(), 6, "{fs:?}");
    assert!(cones.iter().all(|f| (39.0 - 0.05..=40.0 + 0.05).contains(&f.centroid[2])), "{cones:?}");
    assert_eq!(fs.iter().filter(|f| f.radius.is_some_and(|r| (r - 10.0).abs() < 1e-6)).count(), 1, "the neck: {fs:?}");
    volume(&mut s, "Shaft");
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    // The neck picked by its shoulder: the teeth start at the shoulder and stop short of the far end, so no chamfer
    // is cut, and the feature says so.
    let mut s = Session::default();
    stepped(&mut s);
    let neck = json!({
        "type": "straight", "teeth": 6, "length": 8, "minor_diameter": 17, "tooth_width": 4, "side_clearance": 0.05,
        "major_clearance": 0.3, "minor_clearance": 0.2, "shaft_face": [10, 0, 41], "cutter_diameter": 40, "end_chamfer": 0.5
    });
    let r = run(&mut s, CMD, neck);
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("end chamfer was not cut")), "{r}");
    volume(&mut s, "Shaft");
}

#[test]
fn teeth_that_start_at_a_shoulder_cut_nothing_of_the_shoulder() {
    let mut s = Session::default();
    stepped(&mut s);
    let blank = PI * 15.0 * 15.0 * 40.0 + PI * 10.0 * 10.0 * 15.0;
    assert_close("the stepped blank", volume(&mut s, "Shaft"), blank);
    // The neck picked by its shoulder, the size left out so the spline fits the neck: the teeth run from z = 40 up
    // for 8 mm, and the spaces start exactly at the shoulder.
    let p = json!({
        "type": "straight", "teeth": 6, "length": 8, "minor_diameter": 17, "tooth_width": 4, "side_clearance": 0.05,
        "major_clearance": 0.3, "minor_clearance": 0.2, "shaft_face": [10, 0, 41]
    });
    let r = run(&mut s, CMD, p);
    assert!((r["shaft_major_diameter"].as_f64().unwrap_or(f64::NAN) - 20.0).abs() < 1e-6, "the neck's own size: {r}");
    // The blank less exactly the tooth spaces over the length: had the cuts reached into the shoulder, more would go.
    let neck = SplineNumbers { minor_diameter: Some(17.0), ..numbers(6, 20.0, 4.0) };
    assert_close("the neck splined from the shoulder", volume(&mut s, "Shaft"), blank - (PI * 10.0 * 10.0 - shaft_area(&neck)) * 8.0);
    // Tooth 0 stands on +X and a space is half a pitch on: cut in the neck, whole in the shoulder just below it and
    // in the neck past the length.
    assert_eq!(material_at(&s, "Shaft", 9.5, 44.0, &[0.0, 30.0]), [true, false]);
    assert_eq!(material_at(&s, "Shaft", 9.5, 39.5, &[30.0]), [true]);
    assert_eq!(material_at(&s, "Shaft", 9.5, 52.0, &[30.0]), [true]);
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn teeth_that_start_at_the_blind_bottom_of_a_bore_cut_exactly_their_length() {
    // A ring 40 mm high whose 22 mm bore is plugged from the bottom up to z = 15: a blind bore from z = 15 to 40.
    let blank = PI * (20.0 * 20.0 - 11.0 * 11.0) * 40.0 + PI * 11.0 * 11.0 * 15.0;
    // The cut takes the hub's opening less the bore over the length, whichever end the teeth start from: at the
    // bore's mouth the cuts run out into the air, at its bottom they start on the bottom's own plane.
    let opening = area(&planned(&numbers(6, 28.0, 6.0)).hub);
    for (pick_z, length) in [(16.0, 10.0), (39.0, 10.0), (16.0, 25.0)] {
        let mut s = Session::default();
        ring(&mut s, 20.0, 11.0, 0.0, 40.0);
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 0, "name": "Plug section"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 11}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 15, "operation": "join"}));
        assert_close("the blind blank", volume(&mut s, "Hub"), blank);
        run(&mut s, CMD, with(straight(), json!({"bore_face": [11, 0, pick_z], "length": length})));
        let what = format!("the hub cut from z = {pick_z} for {length} mm");
        assert_close(&what, volume(&mut s, "Hub"), blank - (opening - PI * 11.0 * 11.0) * length);
        assert!(notes(&s).is_empty(), "{what}: {:?}", notes(&s));
        if pick_z < 20.0 && length < 20.0 {
            // From the bottom for 10 mm: the shaft's tooth 0 stands on +X, so the hub is cut there and whole half
            // a pitch on; past the length the wall is whole, and so is the plug just below the bottom.
            assert_eq!(material_at(&s, "Hub", 12.5, 20.0, &[0.0, 30.0]), [false, true], "{what}");
            assert_eq!(material_at(&s, "Hub", 12.5, 30.0, &[0.0]), [true], "{what}");
            assert_eq!(material_at(&s, "Hub", 12.5, 14.5, &[0.0]), [true], "{what}");
            assert_eq!(material_at(&s, "Hub", 5.0, 14.5, &[0.0]), [true], "{what}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Lasting names through edits
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_chamfer_on_a_flank_edge_stays_on_that_flank_through_edits() {
    let mut s = Session::default();
    let c = shaft(&mut s, 14.0, 60.0);
    let p = with(straight(), json!({"shaft_face": [14, 0, 55]}));
    let f = feature_of(&run(&mut s, CMD, p.clone()));
    // Space 0's flank beside tooth 0 lies just after angle 0 (tooth 0 stands on +X, 3 mm either side of it).
    let flank = format!("F{f}:space0:flank:1");
    let fs = faces(&mut s, "Shaft");
    let at = fs.iter().find(|x| stem(&x.name) == flank).unwrap_or_else(|| panic!("no {flank}: {fs:?}"));
    assert!((0.0..30.0).contains(&angle_of(at)), "{at:?}");
    // The flank is flat and runs along Z: its centroid's place across is on it, at every height.
    let edge = [at.centroid[0], at.centroid[1], 60.0];
    let chamfer = feature_of(&run(&mut s, "solid.chamfer", json!({"edges": [edge], "distance": 0.2, "tangent_chain": false})));
    // The chamfer keeps its edge by the names of the two faces it joins: the shaft's end and the flank.
    let blend = format!("blend(F{c}:top|{flank})");
    let check = |s: &mut Session, what: &str| {
        let fs = faces(s, "Shaft");
        let made: Vec<&Face> = fs.iter().filter(|x| x.name.starts_with(&format!("F{chamfer}:"))).collect();
        assert!(!made.is_empty() && made.iter().all(|x| x.name.contains(&blend)), "{what}: {fs:?}");
        let flank_at = fs.iter().find(|x| stem(&x.name) == flank).map(angle_of).unwrap_or_else(|| panic!("{what}: no {flank}"));
        assert!(made.iter().all(|x| (angle_of(x) - flank_at).abs() < 6.0), "{what}: the chamfer is by the flank at {flank_at}: {made:?}");
        assert!(notes(s).is_empty(), "{what}: {:?}", notes(s));
        volume(s, "Shaft");
    };
    check(&mut s, "as made");
    let mut edited = p;
    for change in [json!({"teeth": 8, "tooth_width": 4}), json!({"minor_diameter": 22}), json!({"length": 15}), json!({"teeth": 5})] {
        edited = with(edited, change.clone());
        redefine(&mut s, f, edited.clone());
        check(&mut s, &change.to_string());
    }
}

#[test]
fn a_chamfer_on_the_far_end_survives_the_shaft_being_resized() {
    let mut s = Session::default();
    let c = shaft(&mut s, 14.0, 60.0);
    let p = with(straight(), json!({"shaft_face": [14, 0, 55]}));
    let f = feature_of(&run(&mut s, CMD, p.clone()));
    let chamfer = feature_of(&run(&mut s, "solid.chamfer", json!({"edges": [[14, 0, 0]], "distance": 1, "tangent_chain": false})));
    // Turned down, built up, and back: the round face keeps its name, so the chamfer finds its edge each time.
    for major in [26.0, 30.0, 28.0] {
        redefine(&mut s, f, with(p.clone(), json!({"major_diameter": major})));
        let fs = faces(&mut s, "Shaft");
        let made: Vec<&Face> = fs.iter().filter(|x| x.name.starts_with(&format!("F{chamfer}:"))).collect();
        assert!(!made.is_empty() && made.iter().all(|x| x.centroid[2] < 1.0 + 1e-6), "{major}: the chamfer at the far end: {fs:?}");
        assert!(
            fs.iter().any(|x| stem(&x.name) == format!("F{c}:side") && x.radius.is_some_and(|r| (r - major / 2.0).abs() < 1e-9)),
            "{major}: {fs:?}"
        );
        assert!(notes(&s).is_empty(), "{major}: {:?}", notes(&s));
        volume(&mut s, "Shaft");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Edits, expressions and undo
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn an_edit_through_timeline_redefine_rebuilds_the_one_feature_and_undo_restores_it() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    let p = with(straight(), json!({"shaft_face": [14, 0, 55]}));
    let f = feature_of(&run(&mut s, CMD, p.clone()));
    let before = volume(&mut s, "Shaft");
    let (features, undo) = (s.doc.features.len(), s.undo.len());
    redefine(&mut s, f, with(p, json!({"teeth": 8, "tooth_width": 4, "length": 30})));
    assert_eq!((s.doc.features.len(), s.undo.len()), (features, undo + 1), "the same feature, one undo step");
    last_spline(&s);
    let after = volume(&mut s, "Shaft");
    assert_close("eight teeth 30 mm long", after, cut_shaft_volume(14.0, 60.0, shaft_area(&numbers(8, 28.0, 4.0)), 30.0));
    run(&mut s, "edit.undo", json!({}));
    assert_close("undone", volume(&mut s, "Shaft"), before);
    run(&mut s, "edit.redo", json!({}));
    assert_close("redone", volume(&mut s, "Shaft"), after);
}

#[test]
fn numbers_given_as_expressions_stay_parameters_of_the_feature() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "3 + 3", "unit": ""}));
    let p = with(straight(), json!({"shaft_face": [14, 0, 55], "teeth": "n", "length": "25 mm", "tooth_width": "2 * 3 mm"}));
    run(&mut s, CMD, p);
    assert_close("n = 3 + 3", volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 60.0, shaft_area(&numbers(6, 28.0, 6.0)), 25.0));
    // The parameter changed, the spline follows it.
    run(&mut s, "parameters.change", json!({"name": "n", "expression": "8", "unit": ""}));
    assert_close("n = 8", volume(&mut s, "Shaft"), cut_shaft_volume(14.0, 60.0, shaft_area(&numbers(8, 28.0, 6.0)), 25.0));
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn a_suppressed_spline_gives_the_plain_shaft_back_and_a_deleted_one_leaves_it() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    let plain = PI * 14.0 * 14.0 * 60.0;
    let cut = cut_shaft_volume(14.0, 60.0, shaft_area(&numbers(6, 28.0, 6.0)), 20.0);
    let f = feature_of(&run(&mut s, CMD, with(straight(), json!({"shaft_face": [14, 0, 55]}))));
    assert_close("splined", volume(&mut s, "Shaft"), cut);
    run(&mut s, "timeline.suppress", json!({"feature": f, "suppressed": true}));
    assert_eq!(s.doc.feature(f).map(|x| x.suppressed), Some(true));
    assert_close("the spline suppressed", volume(&mut s, "Shaft"), plain);
    run(&mut s, "timeline.suppress", json!({"feature": f, "suppressed": false}));
    assert_eq!(s.doc.feature(f).map(|x| x.suppressed), Some(false));
    assert_close("the spline back", volume(&mut s, "Shaft"), cut);
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    let features = s.doc.features.len();
    run(&mut s, "timeline.delete", json!({"feature": f}));
    assert_eq!(s.doc.features.len(), features - 1);
    assert!(!s.doc.features.iter().any(|x| matches!(x.kind, FeatureKind::ShaftSpline { .. })), "the spline is gone");
    assert_close("the spline deleted", volume(&mut s, "Shaft"), plain);
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
}

#[test]
fn a_saved_design_opens_again_to_the_same_spline() {
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    let p = with(
        straight(),
        json!({
            "shaft_face": [14, 0, 55], "master_spline": "tooth_out", "master_angle": 0, "cutter_diameter": 40, "end_chamfer": 0.5,
            "end_chamfer_angle": 30
        }),
    );
    run(&mut s, CMD, p);
    assert!(notes(&s).is_empty(), "{:?}", notes(&s));
    let (before, face_count) = (volume(&mut s, "Shaft"), faces(&mut s, "Shaft").len());
    let kind = last_spline(&s).clone();
    // Saved to the design file's text and opened in a new session, which rebuilds the History from it.
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert_eq!(reopened.doc.features.len(), s.doc.features.len());
    assert_eq!(reopened.doc.features.iter().filter(|x| matches!(x.kind, FeatureKind::ShaftSpline { .. })).count(), 1);
    let again = last_spline(&reopened);
    assert_eq!(again, &kind, "the feature keeps its values");
    let FeatureKind::ShaftSpline { shaft: picked, master, master_angle, cutter_diameter, end_chamfer, end_chamfer_angle, .. } = again else {
        panic!("not a Shaft Spline")
    };
    assert!(
        picked.is_some()
            && *master == SplineMaster::ToothOut
            && master_angle.is_some()
            && cutter_diameter.is_some()
            && end_chamfer.is_some()
            && end_chamfer_angle.is_some(),
        "{again:?}"
    );
    assert!(notes(&reopened).is_empty(), "{:?}", notes(&reopened));
    assert_close("reopened", volume(&mut reopened, "Shaft"), before);
    assert_eq!(faces(&mut reopened, "Shaft").len(), face_count);
}

#[test]
fn a_refusal_changes_nothing_a_success_is_one_undo_step_and_an_open_sketch_is_refused() {
    let mut s = Session::default();
    run(&mut s, CMD, straight());
    let names = body_names(&s);
    let words = refused(&mut s, with(straight(), json!({"make": "both", "hub_diameter": 30})));
    assert!(words.starts_with("`hub_diameter` leaves no rim"), "{words}");
    let (undo, features) = (s.undo.len(), s.doc.features.len());
    let r = run(&mut s, CMD, with(straight(), json!({"make": "both", "hub_diameter": 40})));
    let made: Vec<String> =
        r["created_bodies"].as_array().map(|b| b.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default();
    assert!(made.len() == 2 && made.iter().all(|n| !names.contains(n)), "new names for the new parts: {r}");
    assert_eq!((s.undo.len(), s.doc.features.len()), (undo + 1, features + 1));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((s.doc.features.len(), body_names(&s)), (features, names.clone()));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(body_names(&s), [names, made].concat());
    // While a sketch is being edited, the spline is refused rather than added inside it.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert_eq!(
        s.execute(CMD, &straight()).map_err(|e| e.to_string()).err().as_deref(),
        Some("invalid parameters for `shaft_spline.insert`: a sketch is being edited: finish it first (sketch.finish)")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn refusals_name_the_parameter_in_plain_words() {
    let straight = json!({
        "type": "straight", "teeth": 6, "length": 20, "major_diameter": 28, "minor_diameter": 23, "tooth_width": 6,
        "root_round": 0.3, "tip_corner": "chamfered", "tip_size": 0.3, "side_clearance": 0.05, "major_clearance": 0.3,
        "minor_clearance": 0.2, "hub_root_round": 0.2, "hub_tip_round": 0.3
    });
    let serration = json!({
        "type": "serration", "teeth": 36, "length": 10, "major_diameter": 20, "minor_diameter": 18.8, "flank_angle": 90,
        "root_round": 0.03, "tip_corner": "rounded", "tip_size": 0.1, "side_clearance": 0.02, "major_clearance": 0.05,
        "minor_clearance": 0.05, "hub_root_round": 0.03, "hub_tip_round": 0.05
    });
    let involute = json!({"type": "involute", "teeth": 20, "length": 15, "module": 1.5, "side_clearance": 0.06, "tip_clearance": 0.2});
    for (p, text) in [
        (json!({"teeth": 6, "length": 20, "side_clearance": 0.1}), "`type` is required: involute, straight or serration"),
        (with(straight.clone(), json!({"type": "helical"})), "`type` must be involute, straight or serration"),
        (with(straight.clone(), json!({"teeth": 2.5})), "`teeth` must be a whole number"),
        (with(straight.clone(), json!({"teeth": 1})), "`teeth` must be more than 1, not 1"),
        (with(straight.clone(), json!({"teeth": 5000})), "`teeth` must be at most 2000, not 5000"),
        (with(straight.clone(), json!({"length": 0})), "`length` must be from 0.001 to 100000 mm, not 0"),
        (with(straight.clone(), json!({"length": 1e-5})), "`length` must be from 0.001 to 100000 mm, not 0.00001"),
        (with(straight.clone(), json!({"length": 1e-9})), "`length` must be from 0.001 to 100000 mm, not 1e-9"),
        (with(straight.clone(), json!({"make": "pulley"})), "`make` must be shaft, hub or both"),
        (with(straight.clone(), json!({"module": 2})), "`module` does not apply to straight-sided teeth (it is for involute teeth)"),
        (with(straight.clone(), json!({"flank_angle": 60})), "`flank_angle` does not apply to straight-sided teeth (it is for serrations)"),
        (with(serration.clone(), json!({"tooth_width": 3})), "`tooth_width` does not apply to serrations (it is for straight-sided teeth)"),
        (with(serration.clone(), json!({"hub_addendum": 0.5})), "`hub_addendum` does not apply to serrations (it is for involute teeth)"),
        (with(involute.clone(), json!({"tooth_width": 3})), "`tooth_width` does not apply to involute teeth (it is for straight-sided teeth)"),
        (
            with(involute.clone(), json!({"major_diameter": 30})),
            "`major_diameter` does not apply to involute teeth (it is for straight-sided teeth and serrations)",
        ),
        (with(straight.clone(), json!({"tooth_width": null})), "`tooth_width` is required (mm)"),
        (with(straight.clone(), json!({"minor_diameter": 30})), "`minor_diameter`: the minor diameter must be smaller than the major diameter"),
        (
            with(straight.clone(), json!({"tooth_width": 20})),
            "`tooth_width`: neighbouring teeth would meet at the root: no space is left between them",
        ),
        (
            with(straight.clone(), json!({"tip_corner": "chamfered", "tip_size": null})),
            "`tip_size` is required (mm, the chamfer's length down the flank)",
        ),
        (with(straight.clone(), json!({"side_clearance": -0.1})), "`side_clearance` cannot be negative, not -0.1"),
        (with(straight.clone(), json!({"side_clearance": -1e-9})), "`side_clearance` cannot be negative, not -1e-9"),
        (
            with(straight.clone(), json!({"minor_clearance": 3})),
            "`minor_clearance`: the minor clearance would take the hub's teeth out beyond the shaft's tips: nothing would drive",
        ),
        (with(straight.clone(), json!({"make": "both"})), "`hub_diameter` is required to make the hub (mm, its outside diameter)"),
        (
            with(straight.clone(), json!({"make": "hub", "hub_diameter": 30})),
            "`hub_diameter` leaves no rim around the hub's teeth: it must be more than 31.4 mm",
        ),
        (with(straight.clone(), json!({"make": "hub", "hub_diameter": 1e6})), "`hub_diameter` must be at most 100000 mm, not 1000000"),
        // A key that would be ignored is refused, saying what it belongs to.
        (
            with(straight.clone(), json!({"hub_diameter": 40})),
            "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub or both",
        ),
        (
            with(straight.clone(), json!({"bore_face": [11, 0, 25], "hub_diameter": 40})),
            "`hub_diameter` is the outside diameter of a new hub: leave it out when a shaft or bore face is picked",
        ),
        (
            with(straight.clone(), json!({"master_angle": 60})),
            "`master_angle` is where the master spline sits: give it only with `master_spline` tooth_out or wide_tooth",
        ),
        (
            with(straight.clone(), json!({"master_spline": "tooth_out", "master_angle": 400})),
            "`master_angle` must be from -360 to 360 (degrees), not 400",
        ),
        (with(straight.clone(), json!({"master_spline": "sideways"})), "`master_spline` must be none, tooth_out or wide_tooth"),
        (with(straight.clone(), json!({"end_chamfer": -1})), "`end_chamfer` must be from 0 to 100000 mm, not -1"),
        (
            with(straight.clone(), json!({"make": "hub", "hub_diameter": 40, "entry_chamfer": 1e6})),
            "`entry_chamfer` must be from 0 to 100000 mm, not 1000000",
        ),
        (with(straight.clone(), json!({"shaft_face": [14, 0]})), "`shaft_face` must be a point [x, y, z]"),
        // Serrations: their own names for the reasons the straight-sided teeth share.
        (with(serration.clone(), json!({"flank_angle": 175})), "`flank_angle` must be at most 170, not 175"),
        (with(serration.clone(), json!({"flank_angle": 170})), "`flank_angle`: the flank does not cross both circles: the teeth are too wide"),
        (with(serration.clone(), json!({"tip_size": 0.3})), "`tip_size`: the corners of a tooth would meet: no tip is left on the major circle"),
        (with(serration.clone(), json!({"minor_diameter": 21})), "`minor_diameter`: the minor diameter must be smaller than the major diameter"),
        (with(involute.clone(), json!({"module": null})), "`module` is required (mm)"),
        (with(involute.clone(), json!({"module": -1})), "`module` cannot be negative, not -1"),
        (with(involute.clone(), json!({"pressure_angle": 60})), "`pressure_angle` must be at most 50, not 60"),
        (with(involute.clone(), json!({"teeth": 2})), "`teeth` must be more than 2, not 2"),
        (with(involute.clone(), json!({"tip_clearance": null})), "`tip_clearance` is required (mm)"),
        (with(involute.clone(), json!({"hub_addendum": 0.85})), "`hub_addendum`: the hub's teeth would reach into the shaft's root fillet"),
        (with(involute.clone(), json!({"root_fillet": 3})), "`root_fillet`: the root must lie below the pitch circle by more than the root fillet"),
    ] {
        assert_eq!(refusal(p.clone()), text, "{p}");
    }
}

/// What the rebuild finds wrong with a pick or with the room it has is refused in words, and nothing is added.
#[test]
fn a_pick_the_spline_cannot_be_cut_into_is_refused_and_nothing_is_added() {
    let on_shaft = |radius: f64, p: Value| {
        let mut s = Session::default();
        shaft(&mut s, radius, 60.0);
        refused(&mut s, with(straight(), p))
    };
    let in_ring = |p: Value| {
        let mut s = Session::default();
        ring(&mut s, 20.0, 11.0, 0.0, 30.0);
        refused(&mut s, with(straight(), p))
    };
    for (words, part) in [
        (on_shaft(14.0, json!({"shaft_face": [80, 0, 55]})), "no round face where the shaft was picked: pick the outside of a round shaft"),
        (on_shaft(14.0, json!({"bore_face": [14, 0, 55]})), "the face picked as the bore is the outside of a shaft: pick the inside of a bore"),
        (in_ring(json!({"shaft_face": [11, 0, 25]})), "the face picked as the shaft is the inside of a bore: pick the outside of a shaft"),
        (on_shaft(14.0, json!({"shaft_face": [14, 0, 55], "length": 70})), "`length` is longer than the shaft's round face"),
        (on_shaft(14.0, json!({"shaft_face": [14, 0, 55], "cutter_diameter": 2})), "`cutter_diameter` must be more than"),
        (on_shaft(15.0, json!({"shaft_face": [15, 0, 55], "end_chamfer": 20})), "`end_chamfer`"),
    ] {
        assert!(words.contains(part), "{words:?} should say {part:?}");
    }
    // A pick on the flat end of the shaft is no round face either.
    on_shaft(14.0, json!({"shaft_face": [3, 0, 60]}));
}

/// The cutter and the two chamfers belong to a part: given for a part that is neither cut nor made they would have
/// no effect, so they are refused before anything is added ([`refused`] checks the design is left as it was).
#[test]
fn a_cutter_or_a_chamfer_for_a_part_not_cut_or_made_is_refused() {
    let cutter = "`cutter_diameter` is the cutter that ends the tooth spaces cut into a picked shaft: give it only with `shaft_face`";
    let end = "`end_chamfer` is the chamfer on the ends of the shaft's teeth: give it only when a shaft is cut or made";
    let entry = "`entry_chamfer` is the chamfer where the teeth run out of the hub's bore: give it only when a hub is cut or made";
    let end_distance2 = "`end_chamfer_distance2` is the distance in from the major diameter of the chamfer on the ends of the shaft's teeth: \
                         give it only when a shaft is cut or made";
    let end_angle = "`end_chamfer_angle` is the angle of the chamfer on the ends of the shaft's teeth: give it only when a shaft is cut or made";
    let entry_distance2 = "`entry_chamfer_distance2` is the distance out from the tops of the hub's teeth of the chamfer where the teeth run \
                           out of the hub's bore: give it only when a hub is cut or made";
    let entry_angle =
        "`entry_chamfer_angle` is the angle of the chamfer where the teeth run out of the hub's bore: give it only when a hub is cut or made";
    for (p, words) in [
        (json!({"cutter_diameter": 40}), cutter),
        (json!({"make": "both", "hub_diameter": 40, "cutter_diameter": 40}), cutter),
        (json!({"make": "hub", "hub_diameter": 40, "end_chamfer": 1}), end),
        (json!({"entry_chamfer": 1}), entry),
        (json!({"make": "hub", "hub_diameter": 40, "end_chamfer_distance2": 0.5}), end_distance2),
        (json!({"make": "hub", "hub_diameter": 40, "end_chamfer_angle": 30}), end_angle),
        (json!({"entry_chamfer_distance2": 0.5}), entry_distance2),
        (json!({"entry_chamfer_angle": 30}), entry_angle),
    ] {
        assert_eq!(refusal(with(straight(), p.clone())), words, "{p}");
    }
    // The same with a face picked: a bore has no shaft to round the spaces of or chamfer, a shaft no bore.
    let mut s = Session::default();
    ring(&mut s, 20.0, 11.0, 0.0, 30.0);
    let features = s.doc.features.len();
    assert_eq!(refused(&mut s, with(straight(), json!({"bore_face": [11, 0, 25], "cutter_diameter": 40}))), cutter);
    assert_eq!(refused(&mut s, with(straight(), json!({"bore_face": [11, 0, 25], "end_chamfer": 1}))), end);
    assert_eq!(s.doc.features.len(), features, "nothing was added");
    let mut s = Session::default();
    shaft(&mut s, 14.0, 60.0);
    assert_eq!(refused(&mut s, with(straight(), json!({"shaft_face": [14, 0, 55], "entry_chamfer": 1}))), entry);
    assert_eq!(s.doc.features.len(), 1, "nothing was added");
}

/// A cutter diameter is checked with the numbers by the command itself, before the feature is added: one whose
/// radius does not reach past the depth of the teeth, or one past the limit, is refused whatever the length, also
/// where the teeth run the whole face and no space ends in the cutter's curve.
#[test]
fn a_cutter_too_small_or_too_large_is_refused_by_the_command_whatever_the_length() {
    // Teeth 2.5 mm deep (14 over 11.5 mm): the tools reach a tenth of that, 0.25 mm, past the tops and past the
    // cutter's axis, so the radius must be more than 2.5 + 2 * 0.25 = 3 mm.
    let small = "`cutter_diameter` must be more than 6 mm: the cutter's radius must reach past the depth of the teeth";
    let large = "`cutter_diameter` must be a number of at most 100000 mm";
    for length in [20.0, 60.0] {
        let mut s = Session::default();
        shaft(&mut s, 14.0, 60.0);
        for (diameter, words) in [(2.0, small), (5.0, small), (1e6, large)] {
            let p = with(straight(), json!({"shaft_face": [14, 0, 55], "length": length, "cutter_diameter": diameter}));
            assert_eq!(refused(&mut s, p), words, "{length} mm long, a {diameter} mm cutter");
            assert_eq!(s.doc.features.len(), 1, "nothing was added");
            assert!(notes(&s).is_empty(), "{:?}", notes(&s));
        }
    }
    // The command and the dialog's drawing say the same.
    let p = with(straight(), json!({"shaft_face": [14, 0, 55], "cutter_diameter": 5}));
    let drawn = shaft_spline_preview(&p).expect_err("the preview refuses it");
    assert_eq!((drawn.words.as_str(), drawn.key.as_deref()), (small, Some("cutter_diameter")));
}

/// The dialog's drawing asks the same checks as the command: what one refuses, the other refuses in the same words
/// and names the same key; what one makes, the other draws.
#[test]
fn the_preview_refuses_what_the_command_refuses_in_the_same_words() {
    for (p, key) in [
        (with(straight(), json!({"minor_diameter": 30})), "minor_diameter"),
        (with(straight(), json!({"teeth": 1})), "teeth"),
        (with(straight(), json!({"end_chamfer": -1})), "end_chamfer"),
        (with(straight(), json!({"make": "hub", "hub_diameter": 40, "end_chamfer_angle": 30})), "end_chamfer_angle"),
        (with(straight(), json!({"entry_chamfer_distance2": 0.5})), "entry_chamfer_distance2"),
        (with(straight(), json!({"make": "both"})), "hub_diameter"),
        (with(straight(), json!({"module": 2})), "module"),
    ] {
        let refused = shaft_spline_preview(&p).expect_err("the preview refuses it");
        assert_eq!(refused.words, refusal(p.clone()), "{p}");
        assert_eq!(refused.key.as_deref(), Some(key), "{p}");
    }
    let preview = shaft_spline_preview(&with(straight(), json!({"make": "both", "hub_diameter": 40}))).expect("the preview draws it");
    assert!((preview.pitch - TAU / 6.0).abs() < 1e-12);
    let parts: Vec<(SplinePart, bool)> = preview.outlines.iter().map(|o| (o.part, o.made)).collect();
    assert_eq!(parts, [(SplinePart::Shaft, true), (SplinePart::Hub, true)]);
    // A shaft alone: the hub is drawn for what it fits, and not made.
    let preview = shaft_spline_preview(&straight()).expect("the preview draws it");
    assert_eq!(preview.outlines.iter().map(|o| o.made).collect::<Vec<_>>(), [true, false]);
    // With a master spline the outline is one period of the whole ring, but the drawing's pitch stays the angle
    // from one tooth to the next, and the tooth its close-up shows is a whole number of pitches from the turn.
    for form in ["tooth_out", "wide_tooth"] {
        let keyed = with(straight(), json!({"make": "both", "hub_diameter": 40, "master_spline": form, "master_angle": 0}));
        let preview = shaft_spline_preview(&keyed).expect("the preview draws it");
        assert!((preview.pitch - TAU / 6.0).abs() < 1e-12, "{form}: pitch {}", preview.pitch);
        let turn = preview.master.as_ref().expect("a master spline").turn.to_radians();
        let pitches = (preview.tooth_at - turn) / preview.pitch;
        assert!((pitches - pitches.round()).abs() < 1e-9 && (0.0..6.0).contains(&pitches.round()), "{form}: {pitches} pitches from the turn");
    }
}

/// The drawing's pitch, the tooth its close-up shows and the master spline's arcs are the kernel plan's own numbers,
/// copied bit for bit: the engine works out no angle of its own.
#[test]
fn the_preview_takes_the_pitch_the_tooth_and_the_arcs_from_the_kernel_plan() {
    let plain = planned(&numbers(6, 28.0, 6.0));
    assert_eq!((plain.tooth_pitch.to_bits(), plain.tooth_at.to_bits()), ((TAU / 6.0).to_bits(), 0.0_f64.to_bits()));
    let preview = shaft_spline_preview(&straight()).expect("the preview draws it");
    assert_eq!((preview.pitch.to_bits(), preview.tooth_at.to_bits()), (plain.tooth_pitch.to_bits(), plain.tooth_at.to_bits()));
    for (form, master) in [("tooth_out", SplineMaster::ToothOut), ("wide_tooth", SplineMaster::WideTooth)] {
        for angle in [0.0, 47.0, -100.0] {
            let p = with(straight(), json!({"make": "both", "hub_diameter": 40, "master_spline": form, "master_angle": angle}));
            let preview = shaft_spline_preview(&p).expect("the preview draws it");
            let made = planned(&SplineNumbers { master, master_angle: Some(angle), ..numbers(6, 28.0, 6.0) });
            let m = made.master.as_ref().expect("a master spline");
            // The plan's numbers are the ones the engine used to work out itself.
            assert_eq!(made.tooth_pitch.to_bits(), (TAU / 6.0).to_bits(), "{form} {angle}");
            assert_eq!(made.tooth_at.to_bits(), (m.turn + made.tooth_pitch * f64::from((m.tooth + 3) % 6)).to_bits(), "{form} {angle}");
            assert_eq!(m.hub_arc_on_shaft.start.to_bits(), (m.hub_arc.start + made.hub_start).to_bits(), "{form} {angle}");
            assert_eq!(m.hub_arc_on_shaft.end.to_bits(), (m.hub_arc.end + made.hub_start).to_bits(), "{form} {angle}");
            // And the preview copies them.
            assert_eq!(
                (preview.pitch.to_bits(), preview.tooth_at.to_bits()),
                (made.tooth_pitch.to_bits(), made.tooth_at.to_bits()),
                "{form} {angle}"
            );
            let drawn = preview.master.as_ref().expect("a master spline");
            for (got, want) in [(drawn.shaft_arc, m.shaft_arc), (drawn.hub_arc, m.hub_arc_on_shaft)] {
                assert_eq!(
                    [got.radius, got.start, got.end].map(f64::to_bits),
                    [want.radius, want.start, want.end].map(f64::to_bits),
                    "{form} {angle}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Hostile input
// ---------------------------------------------------------------------------------------------------------------

/// Every key the command reads, each made hostile in turn on a call that works otherwise, on new parts and on a
/// picked shaft: refused or made, never a panic. Values that make no sense are refused, never made.
#[test]
fn hostile_parameters_never_panic_and_absurd_values_are_refused() {
    let keys = [
        "type",
        "teeth",
        "length",
        "make",
        "hub_diameter",
        "module",
        "pressure_angle",
        "addendum",
        "dedendum",
        "root_fillet",
        "profile_shift",
        "thinning",
        "hub_addendum",
        "tip_clearance",
        "form_clearance",
        "major_diameter",
        "minor_diameter",
        "tooth_width",
        "flank_angle",
        "root_round",
        "tip_corner",
        "tip_size",
        "major_clearance",
        "minor_clearance",
        "hub_tip_round",
        "side_clearance",
        "hub_root_round",
        "master_spline",
        "master_angle",
        "shaft_face",
        "bore_face",
        "cutter_diameter",
        "end_chamfer",
        "entry_chamfer",
        "end_chamfer_distance2",
        "end_chamfer_angle",
        "entry_chamfer_distance2",
        "entry_chamfer_angle",
        "name",
        "body_name",
        "body_names",
        "participants",
    ];
    let fresh = Session::default();
    let mut picked = Session::default();
    shaft(&mut picked, 14.0, 60.0);
    let on_picked = with(straight(), json!({"shaft_face": [14, 0, 55], "end_chamfer": 0.5}));
    let bases = [
        (
            &fresh,
            with(
                straight(),
                json!({"teeth": 4, "make": "both", "hub_diameter": 40, "master_spline": "tooth_out", "end_chamfer": 0.5, "entry_chamfer": 0.5}),
            ),
        ),
        (
            &fresh,
            json!({"type": "serration", "teeth": 12, "length": 5, "major_diameter": 10, "minor_diameter": 9, "side_clearance": 0.02, "major_clearance": 0.05, "minor_clearance": 0.05, "make": "hub", "hub_diameter": 16, "master_spline": "wide_tooth"}),
        ),
        (&picked, on_picked.clone()),
    ];
    let mut values = crate::tests::hostile_values();
    values.extend([
        json!("NaN"),
        json!("inf"),
        json!("-inf"),
        json!("1e400"),
        json!(1e12),
        json!(-1e-12),
        json!(2001),
        json!([14, 0]),
        json!([1e308, 0, 0]),
        json!({"x": 14, "y": 0, "z": 55}),
    ]);
    let mut panicked = Vec::new();
    for (start, base) in &bases {
        for key in keys {
            for v in &values {
                let p = with(base.clone(), json!({ key: v }));
                if let Err(EngineError::Internal(id, msg)) = start.scratch().execute(CMD, &p) {
                    panicked.push(format!("{id} {p}: {msg}"));
                }
            }
        }
    }
    for v in &values {
        if let Err(EngineError::Internal(id, msg)) = fresh.scratch().execute(CMD, v) {
            panicked.push(format!("{id} {v}: {msg}"));
        }
    }
    assert!(panicked.is_empty(), "{}", panicked.join("\n"));
    // Absurd values on the picked shaft: each refused, and the design left as it was.
    for (key, v) in [
        ("teeth", json!(0)),
        ("teeth", json!(-6)),
        ("teeth", json!(1e12)),
        ("teeth", json!("NaN")),
        ("teeth", json!(true)),
        ("length", json!(0)),
        ("length", json!(-20)),
        ("length", json!(1e12)),
        ("length", json!("inf")),
        ("major_diameter", json!(-28)),
        ("tooth_width", json!(1e12)),
        ("side_clearance", json!(-1)),
        ("end_chamfer", json!(-1)),
        ("end_chamfer", json!(1e12)),
        ("end_chamfer_distance2", json!(0)),
        ("end_chamfer_distance2", json!(-1)),
        ("end_chamfer_distance2", json!(1e12)),
        ("end_chamfer_angle", json!(0)),
        ("end_chamfer_angle", json!(90)),
        ("end_chamfer_angle", json!(-45)),
        ("end_chamfer_angle", json!("inf")),
        ("entry_chamfer_angle", json!(30)),
        ("cutter_diameter", json!(1e12)),
        ("shaft_face", json!([0, 0, 0])),
        ("shaft_face", json!([14, 0])),
        ("shaft_face", json!("nope")),
        ("shaft_face", json!([1e308, 0, 0])),
        ("type", json!(7)),
    ] {
        refused(&mut picked.scratch(), with(on_picked.clone(), json!({ key: v })));
    }
}
