//! Cuts on a sheet made from surfaces (#6748): what a feature that is not sheet metal cuts from
//! the sheet's body is read back onto its flat panels. A cut through the sheet is a cut-out of
//! its flat; a cut that stops on a floor inside the thickness is a pocket, measured from the
//! face on the sheet's surface; a cut through a pocket takes it with the material; what the
//! sheet can't keep is refused, and the sheet stays as it was.

use std::f64::consts::PI;

use super::*;

const T: f64 = 2.0;

/// A plate 100 x 60 x 2 thickened from its top (the sheet's surface, z = 2; the material below).
fn plate() -> Session {
    let mut s = Session::default();
    rule(&mut s, "Test", T, 0.44, 2.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": T, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Plate", "face": [50, 30, T]}));
    s
}

/// A rectangle from `p0` to `p1` sketched `at` mm up the z axis, cut `depth` from there into
/// the plate, down when `down`, else up.
fn engrave(s: &mut Session, name: &str, at: f64, p0: [f64; 2], p1: [f64; 2], depth: f64, down: bool) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": at, "name": name}));
    run(s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
    run(s, "sketch.finish", json!({}));
    let direction = if down { "negative" } else { "positive" };
    run(s, "solid.extrude", json!({"sketch": name, "distance": depth, "operation": "cut", "direction": direction}));
}

fn pockets(s: &mut Session) -> Vec<Value> {
    flat(s)["pockets"].as_array().cloned().unwrap_or_default()
}

fn exact_volume(s: &Session) -> f64 {
    s.model
        .state()
        .bodies
        .iter()
        .find(|b| b.name == "Plate")
        .map_or(f64::NAN, |b| solidcraft_kernel::measure_mass(&b.body).map_or(f64::NAN, |m| m.volume))
}

/// A hole drilled through a folded sheet, in its base or in a flange, is a cut-out of the flat
/// where the panel puts it; the folded part keeps it.
#[test]
fn a_hole_drilled_through_a_folded_sheet_is_a_cut_out_of_its_flat() {
    let mut s = plate();
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    let v0 = exact_volume(&s);
    run(&mut s, "solid.hole", json!({"position": [50, 30, T], "diameter": 10}));
    run(&mut s, "solid.hole", json!({"position": [50, 0, 12], "diameter": 6}));
    let mut radii: Vec<f64> = flat_circles(&mut s).iter().map(|c| c.0).collect();
    radii.sort_by(f64::total_cmp);
    assert!(radii.len() == 2 && (radii[0] - 3.0).abs() < 1e-9 && (radii[1] - 5.0).abs() < 1e-9, "{radii:?}");
    let want = v0 - PI * (25.0 + 9.0) * T;
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
    // Unfolded and folded again, the holes stay.
    run(&mut s, "sheet.unfold", json!({}));
    run(&mut s, "sheet.refold", json!({}));
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// An engraving cut 0.5 into the sheet's surface is a pocket from the flat's face on the
/// surface (z 0…0.5), on its DXF layer; a hole drilled through it is a cut-out of the flat and a
/// hole in the pocket. Cut into the other face, an engraving is a pocket from z 1.5 to 2.
#[test]
fn an_engraving_is_a_pocket_and_a_hole_through_it_takes_it_with_the_material() {
    let mut s = plate();
    engrave(&mut s, "Label", T, [10.0, 10.0], [30.0, 20.0], 0.5, true);
    let v1 = 100.0 * 60.0 * T - 200.0 * 0.5;
    assert!((exact_volume(&s) - v1).abs() < 1e-6 * v1, "{}", exact_volume(&s));
    let p = pockets(&mut s);
    assert!(p.len() == 1 && (p[0]["depth_mm"].as_f64().unwrap_or(0.0) - 0.5).abs() < 1e-9 && p[0]["z0_mm"].as_f64() == Some(0.0), "{p:?}");
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or("").to_string();
    assert_eq!((lines_on(&dxf, "OUTLINE"), lines_on(&dxf, "POCKET_BOTTOM_Z0.000000_TO_0.500000_MM")), (4, 4), "{dxf}");
    // Through the middle of it.
    run(&mut s, "solid.hole", json!({"position": [20, 15, T - 0.5], "diameter": 4}));
    let v2 = v1 - PI * 4.0 * (T - 0.5);
    assert!((exact_volume(&s) - v2).abs() < 1e-6 * v2, "{} vs {v2}", exact_volume(&s));
    assert_eq!(flat_circles(&mut s).len(), 1);
    let p = pockets(&mut s);
    assert!(p.len() == 1 && p[0]["cutouts"].as_array().map(Vec::len) == Some(1), "{p:?}");
    // The other face.
    engrave(&mut s, "Under", 0.0, [60.0, 10.0], [80.0, 20.0], 0.5, false);
    let p = pockets(&mut s);
    assert!(p.iter().any(|x| x["z0_mm"].as_f64().is_some_and(|z| (z - 1.5).abs() < 1e-9) && x["side"] == "top"), "{p:?}");
}

/// Engravings from both faces that meet in the thickness cut through where they overlap: the
/// overlap is a cut-out of the flat, the rest of each a pocket from its own face.
#[test]
fn engravings_from_both_faces_that_meet_cut_through_where_they_overlap() {
    let mut s = plate();
    engrave(&mut s, "Top", T, [10.0, 10.0], [30.0, 30.0], 1.2, true);
    engrave(&mut s, "Bottom", 0.0, [20.0, 20.0], [40.0, 40.0], 1.0, false);
    let f = flat(&mut s);
    assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(1), "{f}");
    let p = pockets(&mut s);
    assert_eq!(p.len(), 2, "{p:?}");
    let want = 100.0 * 60.0 * T - 300.0 * 1.2 - 300.0 * 1.0 - 100.0 * T;
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// A counterbored hole is a cut-out of the flat and a ring-shaped pocket round it.
#[test]
fn a_counterbore_is_a_cut_out_and_a_ring_pocket() {
    let mut s = plate();
    run(&mut s, "solid.hole", json!({"position": [50, 30, T], "type": "counterbore", "diameter": 6, "cb_diameter": 10, "cb_depth": 1}));
    assert_eq!(flat_circles(&mut s).len(), 1);
    let p = pockets(&mut s);
    assert!(
        p.len() == 1 && p[0]["cutouts"].as_array().map(Vec::len) == Some(1) && (p[0]["depth_mm"].as_f64().unwrap_or(0.0) - 1.0).abs() < 1e-9,
        "{p:?}"
    );
    let want = 100.0 * 60.0 * T - PI * 9.0 * T - PI * (25.0 - 9.0) * 1.0;
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// A corner rounded through the thickness is an arc of the flat's outline.
#[test]
fn a_corner_rounded_through_the_sheet_is_an_arc_of_its_outline() {
    let mut s = plate();
    run(&mut s, "solid.fillet", json!({"edges": [[100, 0, 1]], "radius": 5}));
    assert_eq!(dxf_entities(&mut s), (4, 1, 0));
    let want = 100.0 * 60.0 * T - (25.0 - PI * 25.0 / 4.0) * T;
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// What a sheet can't keep is refused, and the sheet stays as it was: a countersink and a
/// chamfered face edge (walls not square to the sheet), a cut across a bend.
#[test]
fn cuts_a_sheet_cannot_keep_are_refused_with_the_reason() {
    let refused = |cmd: &str, p: Value| {
        let mut s = plate();
        run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
        let v = exact_volume(&s);
        let e = s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "{cmd}: the sheet changed");
        e
    };
    let e = refused("solid.hole", json!({"position": [50, 30, T], "type": "countersink", "diameter": 6, "cs_diameter": 8, "cs_angle": 90}));
    assert!(e.contains("not square to the sheet"), "{e}");
    let e = refused("solid.chamfer", json!({"edges": [[50, 60, T]], "distance": 0.5}));
    assert!(e.contains("not square to the sheet"), "{e}");
    let e = refused("solid.box", json!({"corner": [45, -10, -10], "length": 10, "width": 15, "height": 50, "operation": "cut"}));
    assert!(e.contains("reaches a bend"), "{e}");
}

/// A hole drilled through a plate at a slant is cut from the flat as its shadow, as a shop cuts
/// it: everything whose fiber through the thickness meets the drill, the hull of the drill's
/// ellipses on the plate's two faces. The folded part has the hole as made, square through.
#[test]
fn a_hole_at_a_slant_is_cut_from_the_flat_as_its_shadow() {
    let mut s = plate();
    let (r, tilt) = (4.0, 30f64.to_radians());
    let axis = [tilt.sin(), 0.0, tilt.cos()];
    let base = [50.0 - 10.0 * axis[0], 30.0, 1.0 - 10.0 * axis[2]];
    run(&mut s, "solid.cylinder", json!({"radius": r, "height": 20, "base": base, "axis": axis, "operation": "cut"}));
    let hole = PI * r * r / tilt.cos() + 2.0 * r * T * tilt.tan();
    let want = (100.0 * 60.0 - hole) * T;
    // The outline is a curve through exact points, within the model's tolerance of them.
    assert!((exact_volume(&s) - want).abs() < 1e-5 * hole * T, "{} vs {want}", exact_volume(&s));
}

/// A half roll of radius 50, 40 long, drawn as a surface about the z axis from +x round to -x
/// and converted: its metal lies 48 to 50 from the axis.
fn half_roll() -> Session {
    let mut s = Session::default();
    rule(&mut s, "Test", T, 0.44, 2.0, 0.5);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
    let arc = run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [50, 0], "sweep": 180}));
    let curve = arc["created"]["curves"][0].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Arc", "curves": [curve], "distance": "40 mm", "name": "Half", "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"body": "Plate"}));
    let v = PI * (50.0 * 50.0 - 48.0 * 48.0) / 2.0 * 40.0;
    assert!((exact_volume(&s) - v).abs() < 1e-6 * v, "{} vs {v}", exact_volume(&s));
    s
}

/// Simpson's rule for `f` over `a` to `b` in `n` (even) steps.
fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    let inner: f64 = (1..n).map(|i| f(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 }).sum();
    (f(a) + f(b) + inner) * h / 3.0
}

/// The metal a prism of the roll's parameters `area` (angle x length) holds: 48 to 50 from the
/// axis.
fn roll_prism(area: f64) -> f64 {
    area * (50.0 * 50.0 - 48.0 * 48.0) / 2.0
}

/// A hole drilled square to the roll's axis goes through the metal widest on the inside, where
/// the fibers are closest: the flat loses what lies within the drill's rim on the inside face,
/// in angle and length, and the roll the metal of that, square through.
#[test]
fn a_hole_drilled_through_a_roll_is_cut_as_its_shadow() {
    let mut s = half_roll();
    let v0 = exact_volume(&s);
    let r = 5.0;
    run(&mut s, "solid.cylinder", json!({"radius": r, "height": 20, "base": [0, 40, 20], "axis": "Y", "operation": "cut"}));
    // sin(angle) = (r / 48) sin(phi): the rim's length r cos(phi) either side, at each angle.
    let k = r / 48.0;
    let area = simpson(|phi: f64| 2.0 * r * phi.cos() * k * phi.cos() / (1.0 - (k * phi.sin()).powi(2)).sqrt(), -PI / 2.0, PI / 2.0, 2000);
    let want = v0 - roll_prism(area);
    assert!((exact_volume(&s) - want).abs() < 1e-5 * roll_prism(area), "{} vs {want}", exact_volume(&s));
}

/// A ball through a roll's metal is cut where it turns edge-on to the fibers: at each height
/// its round section seen from the axis, wholly within the metal, so the flat loses twice the
/// angle it spans there, up the ball's height, and the roll the metal of that.
#[test]
fn a_ball_through_a_roll_is_cut_where_it_turns_edge_on() {
    let mut s = half_roll();
    let v0 = exact_volume(&s);
    run(&mut s, "solid.sphere", json!({"radius": 6, "center": [0, 49, 20], "operation": "cut"}));
    // At height 6 sin(phi) from its middle, its section 6 cos(phi) across, 49 from the axis.
    let area = simpson(|phi: f64| 2.0 * (6.0 * phi.cos() / 49.0).asin() * 6.0 * phi.cos(), -PI / 2.0, PI / 2.0, 2000);
    let want = v0 - roll_prism(area);
    assert!((exact_volume(&s) - want).abs() < 1e-5 * roll_prism(area), "{} vs {want}", exact_volume(&s));
}

/// A ball centred on the roll's outside face turns edge-on to the fibers on a curve that touches
/// that face at the ball's top and bottom: there the outline is that curve and nothing else,
/// the angle it spans at each height seen from the axis 50 away.
#[test]
fn a_ball_centred_on_a_rolls_face_is_cut_where_it_turns_edge_on() {
    let mut s = half_roll();
    let v0 = exact_volume(&s);
    run(&mut s, "solid.sphere", json!({"radius": 6, "center": [0, 50, 20], "operation": "cut"}));
    let area = simpson(|phi: f64| 2.0 * (6.0 * phi.cos() / 50.0).asin() * 6.0 * phi.cos(), -PI / 2.0, PI / 2.0, 2000);
    let want = v0 - roll_prism(area);
    assert!((exact_volume(&s) - want).abs() < 1e-5 * roll_prism(area), "{} vs {want}", exact_volume(&s));
}

/// The angle either side of its middle that a round section `s` across, its centre `d` from the
/// axis, spans within the roll's metal (48 to 50 from the axis), seen from the axis: where it
/// turns edge-on to the fibers if that lies in the metal, else where it crosses the face nearer
/// that.
fn spread(s: f64, d: f64) -> f64 {
    if d - s >= 50.0 || d + s <= 48.0 {
        return 0.0;
    }
    let edge_on = (d * d - s * s).sqrt();
    let rho = if edge_on > 50.0 {
        50.0
    } else if edge_on < 48.0 {
        48.0
    } else {
        return (s / d).asin();
    };
    ((d * d + rho * rho - s * s) / (2.0 * d * rho)).clamp(-1.0, 1.0).acos()
}

/// Across the tangency: a ball's centre stepped through the roll's outside face, where its fold
/// comes to touch that face, is cut each time as the closed form says. (A step 1e-4 past the
/// face SMLib's own boolean refuses; 2e-4 and 1e-3 past it are read within 1.5e-5 of the cut,
/// short of this test's 1e-5: the curve through the fold's points guesses its ends' tangents.)
#[test]
fn a_ball_stepped_through_a_rolls_face_is_cut_alike_either_side() {
    for off in [-2e-4, -1e-4, 0.0, 1e-2] {
        let d = 50.0 + off;
        let mut s = half_roll();
        let v0 = exact_volume(&s);
        run(&mut s, "solid.sphere", json!({"radius": 6, "center": [0, d, 20], "operation": "cut"}));
        let area = simpson(|phi: f64| 2.0 * spread(6.0 * phi.cos(), d) * 6.0 * phi.cos(), -PI / 2.0, PI / 2.0, 20000);
        let want = v0 - roll_prism(area);
        assert!((exact_volume(&s) - want).abs() < 1e-5 * roll_prism(area), "{d}: {} vs {want}", exact_volume(&s));
    }
}

/// What a roll can't keep is refused, and the roll stays as it was: a dimple that stops inside
/// the metal, and a drill that runs on into a bend.
#[test]
fn shadows_a_sheet_cannot_keep_are_refused_with_the_reason() {
    let mut s = half_roll();
    let v = exact_volume(&s);
    let e = s
        .execute("solid.cylinder", &json!({"radius": 3, "height": 20, "base": [0, 49, 20], "axis": "Y", "operation": "cut"}))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(e.contains("don't run through"), "{e}");
    assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "the roll changed");
    let mut s = plate();
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    let v = exact_volume(&s);
    let tilt = 30f64.to_radians();
    let e = s
        .execute("solid.cylinder", &json!({"radius": 5, "height": 40, "base": [50.0, 20.0 * tilt.sin(), -20.0 * tilt.cos()], "axis": [0.0, -tilt.sin(), tilt.cos()], "operation": "cut"}))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(e.contains("reaches a bend"), "{e}");
    assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "the sheet changed");
}
