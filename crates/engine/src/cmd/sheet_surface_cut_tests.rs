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
    let e = refused("solid.hole", json!({"position": [50, 30, T], "type": "countersink", "diameter": 6, "cs_diameter": 12, "cs_angle": 90}));
    assert!(e.contains("not square to the sheet"), "{e}");
    let e = refused("solid.chamfer", json!({"edges": [[50, 60, T]], "distance": 0.5}));
    assert!(e.contains("not square to the sheet"), "{e}");
    let e = refused("solid.box", json!({"corner": [45, -10, -10], "length": 10, "width": 15, "height": 50, "operation": "cut"}));
    assert!(e.contains("reaches a bend"), "{e}");
}
