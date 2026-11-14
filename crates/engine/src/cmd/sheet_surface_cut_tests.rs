//! Cuts on a sheet made from surfaces (#6748): what a feature that is not sheet metal cuts from
//! the sheet's body is read back onto its flat panels. A cut through the sheet is a cut-out of
//! its flat; a cut that stops on a floor inside the thickness is a pocket, measured from the
//! face on the sheet's surface; a cut through a pocket takes it with the material; what the
//! sheet can't keep is refused, and the sheet stays as it was.

use solidcraft_develop::prelude::*;
use std::f64::consts::{PI, SQRT_2};

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};

use super::*;

const T: f64 = 2.0;

/// A session checking each step: each cut is also weighed against what the sheet reads of it.
fn session() -> Session {
    let mut s = Session::default();
    s.set_validation(solidcraft_kernel::Validation::EachStep);
    s
}

/// A plate 100 x 60 x 2 thickened from its top (the sheet's surface, z = 2; the material below).
fn plate() -> Session {
    let mut s = session();
    rule(&mut s, "Test", T, 0.44, 2.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": T, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [50, 30, T]}));
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

/// An engraving cut 0.5 into the sheet's surface is a pocket down from the flat's face on the
/// surface, its top (z 1.5…2), on its DXF layer; a hole drilled through it is a cut-out of the
/// flat and a hole in the pocket. Cut into the other face, an engraving is a pocket from z 0 to 0.5.
#[test]
fn an_engraving_is_a_pocket_and_a_hole_through_it_takes_it_with_the_material() {
    let mut s = plate();
    engrave(&mut s, "Label", T, [10.0, 10.0], [30.0, 20.0], 0.5, true);
    let v1 = 100.0 * 60.0 * T - 200.0 * 0.5;
    assert!((exact_volume(&s) - v1).abs() < 1e-6 * v1, "{}", exact_volume(&s));
    let p = pockets(&mut s);
    assert!(p.len() == 1 && (p[0]["depth_mm"].as_f64().unwrap_or(0.0) - 0.5).abs() < 1e-9 && p[0]["z1_mm"].as_f64() == Some(T), "{p:?}");
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or("").to_string();
    assert_eq!((lines_on(&dxf, "OUTLINE"), lines_on(&dxf, "POCKET_TOP_Z1.500000_TO_2.000000_MM")), (4, 4), "{dxf}");
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
    assert!(p.iter().any(|x| x["z0_mm"].as_f64() == Some(0.0) && x["side"] == "bottom"), "{p:?}");
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
/// chamfered face edge (walls not square to the sheet), a notch through a bend from the edge.
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
    assert!(e.contains("reaches the edge of") && e.contains("across a bend"), "{e}");
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

/// A 100 x 60 base flange on XY of the default rule (T 2.5), its metal z 0 to 2.5:
/// 100 · 60 · 2.5 = 15000 mm³.
fn default_plate() -> Session {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"type": "base", "sketch": "Base", "body_name": "Plate"}));
    assert!((exact_volume(&s) - 15000.0).abs() < 1e-9 * 15000.0, "{}", exact_volume(&s));
    s
}

/// A bore of radius `r`, 20 long, lying along x in the default plate from x 40 to 60 about
/// y = 30, its axis `z` up, cut from it: what the cut says.
fn bore(s: &mut Session, r: f64, z: f64) -> std::result::Result<Value, String> {
    s.execute("solid.cylinder", &json!({"radius": r, "height": 20, "base": [40, 30, z], "axis": "X", "operation": "cut"})).map_err(|e| e.to_string())
}

/// Each hole of the sheet's panels: its extent along its panel's chart's x and y, from its
/// segments' ends, and whether its segments are all lines.
fn panel_holes(s: &Session) -> Vec<(f64, f64, bool)> {
    let Some(sk) = s.model.state().surface_sheets.first().cloned() else { panic!("the plate is a sheet") };
    sk.sheet
        .panels
        .iter()
        .flat_map(|p| p.region.holes.iter())
        .map(|h| {
            let ends: Vec<Vec2> = h.segs.iter().map(|g| g.start()).collect();
            let spread =
                |k: fn(&Vec2) -> f64| ends.iter().map(k).fold(f64::NEG_INFINITY, f64::max) - ends.iter().map(k).fold(f64::INFINITY, f64::min);
            (spread(|p| p.x), spread(|p| p.y), h.segs.iter().all(|g| matches!(g, Seg2::Line { .. })))
        })
        .collect()
}

/// The default plate less a bore R 2 lying in it (see [`bore`]), its axis at `z` within the metal
/// and reaching both faces: its shadow is a slot 20 x 4 through the plate, four lines, 80 of the
/// flat's 6000 mm², and the plate loses 20 · 4 · 2.5 = 200 of its 15000 mm³, 14800 left. Checking
/// each step, the sheet built again weighs no more than the body the bore cut.
fn a_bore_cuts_its_shadow(z: f64) {
    let mut s = default_plate();
    if let Err(e) = bore(&mut s, 2.0, z) {
        panic!("axis at z {z}: {e}");
    }
    let want = 15000.0 - 20.0 * 4.0 * 2.5;
    assert!((exact_volume(&s) - want).abs() < 1e-9 * want, "axis at z {z}: {} vs {want}", exact_volume(&s));
    let (material, outlines, cutouts, cuts, _) = flat_exact(&s);
    assert!(
        (material - 5920.0).abs() < 1e-9 * 6000.0 && (outlines - 6000.0).abs() < 1e-9 * 6000.0 && (cutouts, cuts) == (1, 0),
        "axis at z {z}: the flat holds {material} mm² within {outlines}, {cutouts} cut-outs, {cuts} cuts of the sheet's"
    );
    let holes = panel_holes(&s);
    assert!(holes.len() == 1 && same_size((holes[0].0, holes[0].1), (20.0, 4.0), 1e-9) && holes[0].2, "axis at z {z}: {holes:?}");
}

/// A bore lying in a plate that runs through both its faces is cut from the flat as its shadow,
/// as a shop cuts it square through: every fiber through the thickness that meets the bore. R 2
/// along x from x 40 to 60, its axis at mid-thickness (z 1.25) in the default plate (T 2.5), it
/// reaches z −0.75 to 3.25. The plate's faces, 1.25 from its axis, show it 2 √(2² − 1.25²) =
/// 3.1225 across, but at its axis it is 2 · 2 = 4 across, and every fiber within 2 of the axis
/// meets it there: the flat loses a slot 20 x 4, and the plate 20 · 4 · 2.5 = 200 mm³, 14800
/// left. Read from what its faces show, the plate kept 15000 − 20 · 2.5 · 3.1225 = 14843.875 mm³:
/// the metal the bore took at its axis came back.
#[test]
fn a_bore_lying_in_a_plate_through_both_faces_is_cut_as_its_shadow() {
    a_bore_cuts_its_shadow(1.25);
}

/// The same bore with its axis at z 1 reaches z −1 to 3, both faces still. The faces show it
/// 2 √(2² − 1²) = 3.4641 across at z 0 and 2 √(2² − 1.5²) = 2.6458 across at z 2.5, each narrower
/// than it is at its axis, which lies within the metal (0 < 1 < 2.5): there it is 2 · 2 = 4
/// across, so its shadow is the same slot 20 x 4, 14800 mm³ left.
#[test]
fn a_bore_lying_off_the_middle_of_a_plate_is_cut_as_its_shadow() {
    a_bore_cuts_its_shadow(1.0);
}

/// A bore lying in a plate that doesn't run through it is no cut through the sheet: refused, and
/// the plate stays as it was, 15000 mm³. R 2 with its axis at z −1 reaches z −3 to 1: the face
/// z 0 only, 1.5 short of z 2.5. R 1 at mid-thickness reaches z 0.25 to 2.25: neither face, a
/// hollow inside the metal.
#[test]
fn a_bore_lying_in_a_plate_short_of_either_face_is_refused() {
    for (r, z) in [(2.0, -1.0), (1.0, 1.25)] {
        let mut s = default_plate();
        let e = bore(&mut s, r, z).err().unwrap_or_default();
        assert!(e.contains("don't run through"), "R {r} at z {z}: {e}");
        assert!((exact_volume(&s) - 15000.0).abs() < 1e-9 * 15000.0, "R {r} at z {z}: the plate changed to {}", exact_volume(&s));
    }
}

/// A half roll of radius 50, 40 long, drawn as a surface about the z axis from +x round to -x
/// and converted: its metal lies 48 to 50 from the axis.
fn half_roll() -> Session {
    let mut s = session();
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

/// A roll's flat has its lines, as a bend's has: along its rulings where it starts and ends, and
/// halfway between, each the roll's length; the DXF draws them on a layer of their own.
#[test]
fn a_rolls_flat_has_lines_where_it_starts_and_ends_and_halfway_between() {
    let mut s = half_roll();
    let flat = run(&mut s, "sheet.flat_pattern", json!({}));
    let lines: Vec<(String, [f64; 2], [f64; 2])> = flat["roll_lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            let p = |k: &str| [l[k][0].as_f64().unwrap_or(f64::NAN), l[k][1].as_f64().unwrap_or(f64::NAN)];
            (l["line"].as_str().unwrap_or_default().to_string(), p("start"), p("end"))
        })
        .collect();
    let mut kinds: Vec<&str> = lines.iter().map(|l| l.0.as_str()).collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["end", "middle", "start"], "{flat}");
    let length = |l: &(String, [f64; 2], [f64; 2])| (l.2[0] - l.1[0]).hypot(l.2[1] - l.1[1]);
    assert!(lines.iter().all(|l| (length(l) - 40.0).abs() < 1e-6), "{lines:?}");
    // The middle line lies halfway across the roll's flat, which is half a turn round its neutral
    // surface wide.
    let line = |k: &str| lines.iter().find(|l| l.0 == k).map(|l| Vec2::new(0.5 * (l.1[0] + l.2[0]), 0.5 * (l.1[1] + l.2[1]))).unwrap_or_default();
    let (a, m, b) = (line("start"), line("middle"), line("end"));
    let wide = PI * (48.0 + 0.44 * T);
    assert!((a.dist(b) - wide).abs() < 1e-6 && (a.dist(m) - wide / 2.0).abs() < 1e-6, "{a:?} {m:?} {b:?}");
    // Its row in the report, as a bend has one: inside radius, turn, width and length.
    let row = &flat["roll_table"][0];
    let near = |k: &str, v: f64| row[k].as_f64().is_some_and(|x| (x - v).abs() < 1e-6);
    assert!(flat["roll_table"].as_array().map(Vec::len) == Some(1) && near("radius", 48.0) && near("angle_deg", 180.0), "{flat}");
    assert!(near("width", wide) && near("length", 40.0), "{flat}");
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or_default().to_string();
    assert_eq!(
        dxf.matches(
            "
ROLL
"
        )
        .count()
            - 1,
        3,
        "the layer and its three lines"
    );
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
#[ignore = "solidcore: a sphere whose pole lies on a cylinder's face leaves a trim with coincident control points at the pole since main's kernel (reproduced by crates/kernel/tests/ball_on_a_rolls_face.rs); #6748 follow-up"]
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
#[ignore = "solidcore: a sphere whose pole lies on a cylinder's face leaves a trim with coincident control points at the pole since main's kernel (reproduced by crates/kernel/tests/ball_on_a_rolls_face.rs); #6748 follow-up"]
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
/// the metal; and on a plate, a drill at a slant through its edge that scoops into its bend and
/// flange without going through them.
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
    assert!(e.contains("leaves a recess") && e.contains("in a bend"), "{e}");
    assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "the sheet changed");
}

/// A plate with a 20 high flange on its edge y = 0, as its sheet keeps it, and its bend's zone in
/// the flat (the plate's chart): where the plate's tangent line starts, the unit vectors along the
/// bend and across it toward the flange, and the zone's width and length.
struct Flanged {
    sheet: solidcraft_doc::SurfaceSheet,
    start: Vec2,
    along: Vec2,
    across: Vec2,
    width: f64,
    length: f64,
}

impl Flanged {
    fn new() -> Flanged {
        let mut s = plate();
        run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
        let Some(sheet) = s.model.state().surface_sheets.first().cloned() else { panic!("the plate is a sheet") };
        let Ok(dev) = sheet.develop() else { panic!("the sheet lies flat") };
        let Some(z) = dev.zones.first() else { panic!("the sheet has a bend") };
        let (start, along) = (z.near[0], (z.near[1] - z.near[0]).normalized().unwrap_or_default());
        let left = Vec2::new(-along.y, along.x);
        let across = if (z.far[0] - start).dot(left) < 0.0 { -left } else { left };
        Flanged { width: (z.far[0] - start).dot(across), length: z.near[0].dist(z.near[1]), sheet, start, along, across }
    }

    /// The point `a` along the bend from its start and `b` across it from the plate's tangent
    /// line.
    fn at(&self, a: f64, b: f64) -> Vec2 {
        self.start + self.along * a + self.across * b
    }

    fn polygon(&self, pts: &[(f64, f64)]) -> Region2 {
        let pts: Vec<Vec2> = pts.iter().map(|(a, b)| self.at(*a, *b)).collect();
        Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() }
    }

    fn rect(&self, a: [f64; 2], b: [f64; 2]) -> Region2 {
        self.polygon(&[(a[0], b[0]), (a[1], b[0]), (a[1], b[1]), (a[0], b[1])])
    }

    /// The sheet with `cuts` drawn on `panel` built: its volume, and the sheet.
    fn cut_on(&self, panel: usize, cuts: Vec<Region2>) -> solidcraft_doc::Result<(f64, solidcraft_doc::SurfaceSheet)> {
        let mut sheet = self.sheet.clone();
        sheet.sheet.cuts.extend(cuts.into_iter().map(|region| solidcraft_develop::Cut { panel, region }));
        let body = sheet.build()?;
        Ok((solidcraft_kernel::measure_mass(&body).map_or(f64::NAN, |m| m.volume), sheet))
    }

    /// The sheet with `cuts` drawn on the plate, whose chart is the flat's frame.
    fn cut(&self, cuts: Vec<Region2>) -> (f64, solidcraft_doc::SurfaceSheet) {
        match self.cut_on(0, cuts) {
            Ok(out) => out,
            Err(e) => panic!("{e}"),
        }
    }
}

/// A circle of radius `r` round `c`.
fn disc(c: Vec2, r: f64) -> Loop2 {
    Loop2 { segs: vec![Seg2::Arc { center: c, radius: r, start: 0.0, sweep: PI }, Seg2::Arc { center: c, radius: r, start: PI, sweep: PI }] }.ccw()
}

/// What a bend of the test rule (T 2, R 2, K 0.44) keeps per area of its zone in the flat: the
/// zone's neutral width wraps round the bend, its thickness from R to R + T.
fn bent_per_area() -> f64 {
    T * (2.0 + T / 2.0) / (2.0 + 0.44 * T)
}

/// A slot cut across a bend from the plate into the flange leaves two stretches of the bend, each
/// still joining both panels: both bend, and the bend's line stops at the slot. Drawn on the
/// flange in its own chart instead, the slot is the same cut.
#[test]
fn a_slot_across_a_bend_leaves_two_stretches_that_bend() {
    let f = Flanged::new();
    let w = f.width;
    let (v0, _) = f.cut(Vec::new());
    let slot = f.rect([40.0, 60.0], [-3.0, w + 3.0]);
    let (v, sheet) = f.cut(vec![slot.clone()]);
    let want = v0 - 2.0 * 20.0 * 3.0 * T - 20.0 * w * bent_per_area();
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
    let lines: Vec<f64> = sheet.bend_infos().iter().map(|b| b.length).collect();
    assert!(lines.len() == 2 && (lines.iter().sum::<f64>() - (f.length - 20.0)).abs() < 1e-6, "{lines:?} of {}", f.length);
    assert!(sheet.bends.iter().all(|b| b.tabs.is_empty()), "no tabs");
    let Ok(dev) = f.sheet.develop() else { panic!("the sheet lies flat") };
    let Some(place) = dev.placements.get(1).copied().flatten() else { panic!("the flange has a place in the flat") };
    match f.cut_on(1, vec![place.inverse().region(&slot)]) {
        Ok((on_flange, _)) => assert!((on_flange - v).abs() < 1e-9 * v, "{on_flange} vs {v}"),
        Err(e) => panic!("{e}"),
    }
}

/// A U cut into a bend from the plate's side frees the strip of the bend inside it from the
/// flange: the strip is a tab, and stays flat with the plate, its volume its flat area through
/// the thickness, while the rest of the bend bends round it. The bend's line stops at the tab.
#[test]
fn a_strip_of_a_bend_held_by_one_panel_stays_flat_with_it_as_a_tab() {
    let f = Flanged::new();
    let (w, (v0, _)) = (f.width, f.cut(Vec::new()));
    let b = 0.6 * w;
    // Its sides 0.3 wide from 1 inside the plate, its bottom 0.3 wide across the bend at b.
    let u = f.polygon(&[(40.0, -1.0), (40.3, -1.0), (40.3, b), (60.0, b), (60.0, -1.0), (60.3, -1.0), (60.3, b + 0.3), (40.0, b + 0.3)]);
    let (v, sheet) = f.cut(vec![u]);
    let whole = 20.3 * (b + 1.3) - 19.7 * (b + 1.0);
    let in_plate = 20.3 - 19.7;
    let tab = 19.7 * b;
    let want = v0 - in_plate * T - (whole - in_plate) * bent_per_area() - tab * bent_per_area() + tab * T;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want} (bent, it would be {})", want - tab * (T - bent_per_area()));
    assert!(sheet.bends.first().is_some_and(|l| l.tabs.len() == 1), "one tab");
    let lines: Vec<f64> = sheet.bend_infos().iter().map(|b| b.length).collect();
    assert!(lines.len() == 2 && (lines.iter().sum::<f64>() - (f.length - 20.3)).abs() < 1e-6, "{lines:?} of {}", f.length);
}

/// A hole across the plate's tangent line is cut from the plate and from the bend, each its
/// share of the circle; a hole inside the bend bends with it.
#[test]
fn a_hole_across_a_tangent_line_is_cut_from_the_plate_and_the_bend() {
    let f = Flanged::new();
    let (w, (v0, _)) = (f.width, f.cut(Vec::new()));
    // Its center 1 into the plate: the cap past the tangent line lies in the bend.
    let (r, d): (f64, f64) = (2.5, 1.0);
    let cap = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let across = Region2 { outer: disc(f.at(70.0, -d), r), holes: Vec::new() };
    let inside = Region2 { outer: disc(f.at(20.0, w / 2.0), 1.0), holes: Vec::new() };
    let (v, _) = f.cut(vec![across, inside]);
    let want = v0 - (PI * r * r - cap) * T - cap * bent_per_area() - PI * bent_per_area();
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

/// A cut that takes a whole bend, 1 mm into the plate and the flange beside it, cuts the sheet in
/// two there: the plate and the flange each lose a strip `length` × 1 and are pieces of their own,
/// each where the bend held it. A ring that leaves an island of the bend joined to neither panel
/// is refused: nothing says where it lies.
#[test]
fn a_cut_that_takes_a_whole_bend_leaves_two_pieces_and_an_island_of_a_bend_is_refused() {
    let f = Flanged::new();
    let (w, (v0, _)) = (f.width, f.cut(Vec::new()));
    let (v, sheet) = f.cut(vec![f.rect([-5.0, f.length + 5.0], [-1.0, w + 1.0])]);
    let want = v0 - 2.0 * f.length * T - f.length * w * bent_per_area();
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
    assert_eq!(sheet.regions.len(), 2, "two pieces");
    let refused = |cuts: Vec<Region2>| f.cut_on(0, cuts).err().map(|e| e.to_string()).unwrap_or_default();
    let c = f.at(50.0, w / 2.0);
    let e = refused(vec![Region2 { outer: disc(c, 2.0), holes: vec![disc(c, 1.0).reversed()] }]);
    assert!(e.contains("joined to neither"), "{e}");
}

/// A 100 x 60 plate of 2 mm sheet with a 30 high flange on its edge y = 0, bent R 10 (K 0.44):
/// the bend's axis runs along x at y = z = 12, its metal 10 to 12 from it.
fn wide_bend() -> Session {
    let mut s = session();
    rule(&mut s, "Wide", T, 0.44, 10.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": T, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [50, 30, T]}));
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 30}));
    s
}

/// A drill through the middle of a bend, square to it, is cut from the bend's flat zone as its
/// shadow: every fiber its round meets, so that each section across the drill loses the angle
/// within which the inside of the bend comes nearer its axis than its radius.
#[test]
fn a_drill_through_a_bend_is_cut_as_its_shadow() {
    let mut s = wide_bend();
    let v0 = exact_volume(&s);
    let (r, inside) = (2.0, 10.0);
    let d = std::f64::consts::FRAC_1_SQRT_2;
    run(&mut s, "solid.cylinder", json!({"radius": r, "height": 20, "base": [50, 12, 12], "axis": [0, -d, -d], "operation": "cut"}));
    let removed = simpson(|x: f64| ((r * r - x * x).max(0.0).sqrt() / inside).asin(), -r, r, 20000) * T * (2.0 * inside + T);
    let want = v0 - removed;
    assert!((exact_volume(&s) - want).abs() < 1e-5 * removed, "{} vs {want}", exact_volume(&s));
}

/// A hole drilled square through the plate beside its bend runs on into the bend: one outline
/// across the tangent line, square through the plate where the drill is, and in the bend every
/// fiber the drill meets.
#[test]
fn a_hole_through_a_plate_that_runs_into_its_bend_is_cut_from_both() {
    let mut s = wide_bend();
    let v0 = exact_volume(&s);
    // Its center 1 into the plate from the tangent line y = 12.
    let (r, d, inside): (f64, f64, f64) = (3.0, 1.0, 10.0);
    run(&mut s, "solid.cylinder", json!({"radius": r, "height": 30, "base": [50, 12.0 + d, -5], "axis": "Z", "operation": "cut"}));
    let cap = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    // In the bend, the fiber turned phi meets the drill where its inside end comes within the
    // drill's half chord s of its center: 10 sin phi <= s - 1.
    let reach = (r * r - d * d).sqrt();
    let bend =
        simpson(|x: f64| (((r * r - x * x).max(0.0).sqrt() - d) / inside).clamp(0.0, 1.0).asin(), -reach, reach, 20000) * T * (2.0 * inside + T)
            / 2.0;
    let removed = (PI * r * r - cap) * T + bend;
    let want = v0 - removed;
    assert!((exact_volume(&s) - want).abs() < 1e-5 * removed, "{} vs {want}", exact_volume(&s));
}

/// A drill that stops inside a bend is refused, and the sheet stays as it was.
#[test]
fn a_drill_that_stops_inside_a_bend_is_refused() {
    let mut s = wide_bend();
    let v = exact_volume(&s);
    let d = std::f64::consts::FRAC_1_SQRT_2;
    let e = s
        .execute(
            "solid.cylinder",
            &json!({"radius": 2, "height": 9, "base": [50.0, 12.0 - 20.0 * d, 12.0 - 20.0 * d], "axis": [0, d, d], "operation": "cut"}),
        )
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(e.contains("don't run through"), "{e}");
    assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "the sheet changed");
}

/// A slot cut through the flat across a bend while the sheet is unfolded is a cut of the sheet's,
/// as if drawn on the flat: folded again, both stretches of the bend bend, as for the same slot
/// cut folded.
#[test]
fn a_slot_cut_across_a_bend_in_the_flat_folds_with_the_sheet() {
    let f = Flanged::new();
    let (v0, _) = f.cut(Vec::new());
    let w = f.width;
    let mut s = plate();
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    run(&mut s, "sheet.unfold", json!({}));
    // The slot's corners in space: the flat lies in the plane of the plate's face.
    let Some(place) = f.sheet.places.first().copied().flatten() else { panic!("the plate has a place") };
    let corners: Vec<Vec3> = [(40.0, -3.0), (60.0, -3.0), (60.0, w + 3.0), (40.0, w + 3.0)]
        .iter()
        .map(|(a, b)| {
            let p = f.at(*a, *b);
            place.point(Vec3::new(p.x, p.y, 0.0))
        })
        .collect();
    let (lo, hi) = corners.iter().fold(
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    assert!((hi.x - lo.x - 20.0).abs() < 1e-9 || (hi.y - lo.y - 20.0).abs() < 1e-9, "the flat runs along x or y: {lo:?} {hi:?}");
    run(&mut s, "solid.box", json!({"corner": [lo.x, lo.y, -10], "length": hi.x - lo.x, "width": hi.y - lo.y, "height": 20, "operation": "cut"}));
    run(&mut s, "sheet.refold", json!({}));
    let want = v0 - 2.0 * 20.0 * 3.0 * T - 20.0 * w * bent_per_area();
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// A U cut into a bend while the sheet is unfolded frees the strip inside it as a tab, as the
/// same U drawn on the flat does: folded again, the sheet weighs the same. Unfolded, the tab lay
/// flat before the cut as after it, so nothing of its weight changes there.
#[test]
fn a_tab_cut_while_unfolded_is_the_tab_the_flat_makes() {
    let f = Flanged::new();
    let b = 0.6 * f.width;
    let u = [(40.0, -1.0), (40.3, -1.0), (40.3, b), (60.0, b), (60.0, -1.0), (60.3, -1.0), (60.3, b + 0.3), (40.0, b + 0.3)];
    let (want, _) = f.cut(vec![f.polygon(&u)]);
    let mut s = plate();
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    run(&mut s, "sheet.unfold", json!({}));
    // The U's corners in space: the flat lies in the plane of the plate's face.
    let Some(place) = f.sheet.places.first().copied().flatten() else { panic!("the plate has a place") };
    let mut points: Vec<[f64; 2]> = u
        .iter()
        .map(|(a, b)| {
            let p = place.point(Vec3::new(f.at(*a, *b).x, f.at(*a, *b).y, 0.0));
            [p.x, p.y]
        })
        .collect();
    points.push(points[0]);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": -5, "name": "U"}));
    run(&mut s, "sketch.line", json!({"points": points}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "U", "distance": 20, "operation": "cut", "direction": "positive"}));
    run(&mut s, "sheet.refold", json!({}));
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// A notch 20 along the bend from its end: from 3 into the plate to 3 into the flange, and on
/// past the flat's edge. Its box in space, the flat lying in the plane of the plate's face.
fn edge_notch(f: &Flanged) -> (Vec3, Vec3) {
    let Some(place) = f.sheet.places.first().copied().flatten() else { panic!("the plate has a place") };
    [(-10.0, -3.0), (20.0, -3.0), (20.0, f.width + 3.0), (-10.0, f.width + 3.0)]
        .iter()
        .map(|(a, b)| place.point(Vec3::new(f.at(*a, *b).x, f.at(*a, *b).y, 0.0)))
        .fold(
            (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
}

/// A notch cut across a bend from the flat's edge while the sheet is unfolded is a cut of the
/// sheet's: folded again, the sheet weighs what it does with the same notch drawn on its flat,
/// less what the notch takes from the plate, the bend and the flange.
#[test]
fn a_notch_cut_across_a_bend_from_the_flats_edge_folds_with_the_sheet() {
    let f = Flanged::new();
    let w = f.width;
    let (v0, _) = f.cut(Vec::new());
    let want = v0 - 2.0 * 20.0 * 3.0 * T - 20.0 * w * bent_per_area();
    let (drawn, _) = f.cut(vec![f.rect([-10.0, 20.0], [-3.0, w + 3.0])]);
    assert!((drawn - want).abs() < 1e-6 * want, "drawn on the flat: {drawn} vs {want}");
    let mut s = plate();
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    run(&mut s, "sheet.unfold", json!({}));
    let (lo, hi) = edge_notch(&f);
    run(&mut s, "solid.box", json!({"corner": [lo.x, lo.y, -10], "length": hi.x - lo.x, "width": hi.y - lo.y, "height": 20, "operation": "cut"}));
    run(&mut s, "sheet.refold", json!({}));
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "{} vs {want}", exact_volume(&s));
}

/// What such a cut from the flat's edge can't be is refused, and the sheet stays as it was: a
/// drill at a slant through the bend's end, its walls leaning, and the notch stopping 1 inside
/// the thickness, cut by a box or by Combine.
#[test]
fn a_cut_across_a_bend_from_the_flats_edge_at_a_slant_or_short_of_through_is_refused() {
    let f = Flanged::new();
    let Some(place) = f.sheet.places.first().copied().flatten() else { panic!("the plate has a place") };
    // Ø6 across the bend's zone (about 4.5 wide), 1 in from its end, leaning 30° along the bend.
    let c = place.point(Vec3::new(f.at(1.0, f.width / 2.0).x, f.at(1.0, f.width / 2.0).y, 0.0));
    let tilt = 30f64.to_radians();
    let along = place.vector(Vec3::new(f.along.x, f.along.y, 0.0));
    let axis = Vec3::new(0.0, 0.0, tilt.cos()) + along * tilt.sin();
    let base = c - axis * 10.0;
    let drill = json!({"radius": 3, "height": 20, "base": [base.x, base.y, base.z], "axis": [axis.x, axis.y, axis.z], "operation": "cut"});
    let (lo, hi) = edge_notch(&f);
    let pocket = json!({"corner": [lo.x, lo.y, T - 1.0], "length": hi.x - lo.x, "width": hi.y - lo.y, "height": 5, "operation": "cut"});
    let cases: [(&str, &str, Value, fn(&str) -> bool); 2] = [
        ("at a slant", "solid.cylinder", drill, |e| e.contains("reaches the edge of") && e.contains("at a slant")),
        ("short of through", "solid.box", pocket, |e| e.contains("reaches a bend or a seam")),
    ];
    for (what, cmd, p, refused) in cases {
        let mut s = plate();
        run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
        run(&mut s, "sheet.unfold", json!({}));
        let v = exact_volume(&s);
        let e = s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(refused(&e), "{what}: {e}");
        assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "{what}: the sheet changed");
    }
    // The same notch as a body of its own, cut from the sheet by Combine, takes material away as
    // the box cut does: refused alike, not kept on the body with a warning.
    let mut s = plate();
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, T]], "height": 20}));
    run(&mut s, "sheet.unfold", json!({}));
    let v = exact_volume(&s);
    let (size, corner) = ([hi.x - lo.x, hi.y - lo.y, 5.0], [lo.x, lo.y, T - 1.0]);
    let notch = json!({"corner": corner, "length": size[0], "width": size[1], "height": size[2], "body_name": "Notch", "operation": "new"});
    run(&mut s, "solid.box", notch);
    let combine = json!({"target": "Plate", "tools": ["Notch"], "operation": "cut"});
    let e = s.execute("solid.combine", &combine).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("reaches a bend or a seam"), "combine: {e}");
    assert!((exact_volume(&s) - v).abs() < 1e-9 * v, "combine: the sheet changed");
}

/// What a cut takes from a sheet doesn't depend on the cuts before it, their order, or whether
/// the sheet was folded or laid out flat when each was made. On the R 10 bend: two slots through
/// the bend from the plate into the flange (their ends on the same planes), a drill through the
/// bend, a hole that runs into it, one at a slant and one square through the plate. In any order,
/// and with the slots cut on the flat, the sheet loses what each cut takes from it alone.
#[test]
fn cuts_take_what_each_takes_alone_in_any_order_folded_or_flat() {
    let d = std::f64::consts::FRAC_1_SQRT_2;
    let tilt = 30f64.to_radians();
    let axis = [tilt.sin(), 0.0, tilt.cos()];
    // Through the plate to y = 20, the bend's quarter (its axis at y = z = 12) and the flange
    // to z = 20.
    let slot = |x: f64| ("solid.box", json!({"corner": [x, -10, -10], "length": 4, "width": 30, "height": 30, "operation": "cut"}));
    let cuts: Vec<(&str, Value)> = vec![
        slot(10.0),
        slot(30.0),
        ("solid.cylinder", json!({"radius": 2, "height": 20, "base": [50, 12, 12], "axis": [0, -d, -d], "operation": "cut"})),
        ("solid.cylinder", json!({"radius": 3, "height": 30, "base": [70, 13, -5], "axis": "Z", "operation": "cut"})),
        (
            "solid.cylinder",
            json!({"radius": 4, "height": 20, "base": [60.0 - 10.0 * axis[0], 40, 1.0 - 10.0 * axis[2]], "axis": axis, "operation": "cut"}),
        ),
        ("solid.cylinder", json!({"radius": 3, "height": 10, "base": [85, 45, -5], "axis": "Z", "operation": "cut"})),
    ];
    let v0 = exact_volume(&wide_bend());
    let alone: Vec<f64> = cuts
        .iter()
        .map(|(c, p)| {
            let mut s = wide_bend();
            run(&mut s, c, p.clone());
            v0 - exact_volume(&s)
        })
        .collect();
    let want = v0 - alone.iter().sum::<f64>();
    let check = |s: &Session, what: &str| assert!((exact_volume(s) - want).abs() < 1e-6 * want, "{what}: {} vs {want}", exact_volume(s));
    for order in [[0, 1, 2, 3, 4, 5], [5, 4, 3, 2, 1, 0], [2, 0, 4, 1, 5, 3], [3, 5, 1, 4, 0, 2]] {
        let mut s = wide_bend();
        for k in order {
            run(&mut s, cuts[k].0, cuts[k].1.clone());
        }
        check(&s, &format!("folded, in the order {order:?}"));
    }
    // The slots cut on the flat: the plate's tangent line lies along y = 12 there, and the flange
    // starts the bend's allowance short of it.
    let allowance = (10.0 + 0.44 * T) * std::f64::consts::FRAC_PI_2;
    let flat_slot =
        |x: f64| json!({"corner": [x, 12.0 - allowance - 8.0, -10], "length": 4, "width": allowance + 16.0, "height": 20, "operation": "cut"});
    for slots_first in [true, false] {
        let mut s = wide_bend();
        let rest = |s: &mut Session| {
            for (c, p) in &cuts[2..] {
                run(s, c, p.clone());
            }
        };
        if !slots_first {
            rest(&mut s);
        }
        run(&mut s, "sheet.unfold", json!({}));
        run(&mut s, "solid.box", flat_slot(30.0));
        run(&mut s, "solid.box", flat_slot(10.0));
        run(&mut s, "sheet.refold", json!({}));
        if slots_first {
            rest(&mut s);
        }
        check(&s, &format!("slots cut on the flat {}", if slots_first { "first" } else { "last" }));
    }
}

/// On the flat, a hole across the plate's tangent line, then one inside the bend's zone. The
/// first is cut as the circle it is on both sides of the line, and the sheet built after it still
/// shows each panel both its faces, so the second is read too: refolded, the sheet loses what each
/// takes on the flat alone.
#[test]
fn a_hole_inside_a_bend_on_the_flat_after_one_across_its_tangent_line() {
    let hole = |r: f64, x: f64, y: f64| json!({"radius": r, "height": 30, "base": [x, y, -10], "axis": "Z", "operation": "cut"});
    let holes = [hole(3.042, 87.84, 10.926), hole(3.783, 69.982, 5.788)];
    let flat = |holes: &[Value]| {
        let mut s = wide_bend();
        run(&mut s, "sheet.unfold", json!({}));
        for h in holes {
            run(&mut s, "solid.cylinder", h.clone());
        }
        run(&mut s, "sheet.refold", json!({}));
        exact_volume(&s)
    };
    let v0 = exact_volume(&wide_bend());
    let want = v0 - (v0 - flat(&holes[..1])) - (v0 - flat(&holes[1..]));
    let got = flat(&holes);
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
}

/// The flat pattern's outlines' area, its cut-outs not taken off, and how many cut-outs it has.
fn flat_outline(s: &mut Session) -> (f64, usize) {
    let f = flat(s);
    let area = |l: &Value| {
        let pts: Vec<(f64, f64)> =
            l.as_array().into_iter().flatten().map(|p| (p[0].as_f64().unwrap_or(f64::NAN), p[1].as_f64().unwrap_or(f64::NAN))).collect();
        pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum::<f64>().abs() / 2.0
    };
    (f["outline"].as_array().map_or(0.0, |o| o.iter().map(area).sum()), f["cutouts"].as_array().map_or(0, Vec::len))
}

/// A 120 x 80 base (T 1, R 1, K 0.44, the rule's gap `gap`) with walls 35 high on its front
/// (y = 0) and right (x = 120) edges, made by one flange feature, so that the gap closes their
/// corner.
fn corner_sheet(gap: f64) -> Session {
    let mut s = session();
    rule(&mut s, "Corner", 1.0, 0.44, 1.0, gap);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [120, 80]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"type": "base", "sketch": "Base", "body_name": "Plate"}));
    run(&mut s, "sheet.flange", json!({"edges": [[60, 0, 1], [120, 40, 1]], "height": 35}));
    s
}

/// What `corner_sheet` holds, by hand (T 1, R 1), and how far short of the base's far edge each
/// wall ends. The base lies at z 0..1; each wall's bend turns 90° round an axis R + T = 2 in from
/// the base's edge and 2 up, so the base keeps 118 x 78 (x 0..118, y 2..80) and each bend is a
/// quarter annulus of radii 1 and 2, (π/4)(2² − 1²) = 3π/4 mm² across, 118 long on the front and
/// 78 on the right. Each wall stands 33 high, z 2..35. The walls' inside faces are y = 1 and
/// x = 119; the gap g lies between their inside corners, across the corner's 45° line, each set
/// back g/√2 from the other's inside face: the front wall runs x 0..119 − g/√2 and the right wall
/// y 1 + g/√2..80, each c = T + g/√2 short of the base's far edge (as `tray_volume` has it). At
/// g 0.25, c = 1.1767767 and the sheet holds 9204 + 33 · 197.6464466 + 147π = 16188.1469 mm³.
fn corner_volume(gap: f64) -> (f64, f64) {
    let t = 1.0;
    let c = t + gap / SQRT_2;
    (t * (118.0 * 78.0 + 33.0 * ((120.0 - c) + (80.0 - c))) + 0.75 * PI * (118.0 + 78.0), c)
}

/// The area of the sheet's panels' own outlines, in their charts, and how many cuts of its own the
/// sheet keeps.
fn panel_outlines(s: &Session) -> (f64, usize) {
    let Some(sk) = s.model.state().surface_sheets.first().cloned() else { panic!("the plate is a sheet") };
    (sk.sheet.panels.iter().map(|p| p.region.area()).sum(), sk.sheet.cuts.len())
}

/// The corner sheet (see `corner_sheet`) with a rectangle on the XZ plane from (70, `z0`) to
/// (119, `z1`) cut 3 either side of it, through the front wall up to the right wall's inside
/// face: past the front wall's end. The folded sheet loses what the front wall had of the
/// rectangle, and its flat loses that from its outline, a notch open at the wall's end, with no
/// new cut-out. The front wall's own outline runs to the rip's line at x = 119, where the walls'
/// inside faces meet, past its material (the rip's lap, g/√2 wide, which the rip cuts back): it
/// loses the rectangle as drawn, x 70..119 by z z0 up to the wall's top (35) at most, and the
/// sheet keeps no cut of its own for it.
fn cut_past_a_walls_end(gap: f64, z0: f64, z1: f64) {
    let mut s = corner_sheet(gap);
    let t = 1.0;
    let (whole, c) = corner_volume(gap);
    let v0 = exact_volume(&s);
    assert!((v0 - whole).abs() < 1e-6 * whole, "gap {gap}: {v0} vs {whole}");
    let (outline0, cutouts0) = flat_outline(&mut s);
    let (panels0, cuts0) = panel_outlines(&s);
    run(&mut s, "sketch.create", json!({"name": "Cut", "plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [70.0, z0], "p1": [119, z1]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Cut", "distance": 6, "direction": "symmetric", "operation": "cut"}));
    // The box x 70..119, y −3..3, z z0..z1 takes, of the sheet, the front wall's material (y 0..1)
    // over x 70..120 − c, to the wall's end, and z z0 up to the wall's top (35) at most: T times
    // that. Nothing else: the right wall lies at x 119..120, where the box ends, and the base and
    // the bends below z = 2.
    let notch = (120.0 - c - 70.0) * (z1.min(35.0) - z0);
    let want = whole - t * notch;
    assert!((exact_volume(&s) - want).abs() < 1e-6 * want, "gap {gap}, z {z0}..{z1}: {} vs {want}", exact_volume(&s));
    // The front wall's flat part lies in the flat as it is, so the flat's outline loses the notch,
    // open at the wall's end; a window below the top is no cut-out.
    let (outline, cutouts) = flat_outline(&mut s);
    assert_eq!(cutouts, cutouts0, "gap {gap}, z {z0}..{z1}: no new cut-out");
    assert!((outline0 - outline - notch).abs() < 1e-6 * outline0, "gap {gap}, z {z0}..{z1}: the outline lost {} of {notch}", outline0 - outline);
    let (panels, cuts) = panel_outlines(&s);
    let drawn = (119.0 - 70.0) * (z1.min(35.0) - z0);
    assert!((panels0 - panels - drawn).abs() < 1e-9 * panels0, "gap {gap}, z {z0}..{z1}: the panels lost {} of {drawn}", panels0 - panels);
    assert_eq!(cuts, cuts0, "gap {gap}, z {z0}..{z1}: no cut of the sheet's");
}

/// A notch down through a wall's top, from x 70 to the next wall's inside face, past the wall's
/// end where one flange feature closed their corner with the rule's gap: the wall loses it to its
/// end. At gaps 0.25 and 0.4 its reading failed in the kernel ("SMLib failed (status 1001)").
/// By hand (see `corner_volume`), z 15..35: at g 0.25, c = 1.1767767, the sheet holds
/// 9204 + 33 · 197.6464466 + 147π = 16188.1469 and the notch is 48.8232233 x 20 = 976.4645,
/// leaving 15211.6824 mm³; at g 0.4, c = 1.2828427, 16181.1465 less 48.7171573 x 20 = 974.3431,
/// 15206.8034 mm³.
#[test]
fn a_notch_past_a_walls_end_at_a_closed_corner_takes_it_to_the_end() {
    for gap in [0.25, 0.4] {
        cut_past_a_walls_end(gap, 15.0, 40.0);
    }
}

/// A window through a wall below its top, from x 70 to the next wall's inside face, past the
/// wall's end where one flange feature closed their corner with the rule's gap: a notch open at
/// the wall's end, not a hole. It failed at every gap ("a cut-out of the flat lost its piece").
/// By hand (see `corner_volume`), z 12..25: at g 0.25, 16188.1469 less 48.8232233 x 13 =
/// 634.7019, 15553.4450 mm³; at g 0.8, c = 1.5656854, 9204 + 33 · 196.8686292 + 147π =
/// 16162.4789 less 48.4343146 x 13 = 629.6461, 15532.8328 mm³.
#[test]
fn a_window_past_a_walls_end_at_a_closed_corner_is_a_notch_open_at_the_end() {
    for gap in [0.25, 0.8] {
        cut_past_a_walls_end(gap, 12.0, 25.0);
    }
}

/// The sheet's flat as the sheet keeps it, exactly (its curves, not points along them): its
/// material's area, its outlines' area and how many cut-outs it has; and how many cuts of its own
/// the sheet keeps, and how many holes its panels' outlines have.
fn flat_exact(s: &Session) -> (f64, f64, usize, usize, usize) {
    let Some(sk) = s.model.state().surface_sheets.first().cloned() else { panic!("the plate is a sheet") };
    let material: f64 = sk.regions.iter().map(|r| r.area()).sum();
    let outlines: f64 = sk.regions.iter().map(|r| r.outer.signed_area().abs()).sum();
    let cutouts: usize = sk.regions.iter().map(|r| r.holes.len()).sum();
    (material, outlines, cutouts, sk.sheet.cuts.len(), sk.sheet.panels.iter().map(|p| p.region.holes.len()).sum())
}

/// The corner sheet at the rule's gap 0.25 (see `corner_sheet`) with a round hole of radius 4
/// drilled square through its front wall (y −3..3), its center 20 up and `inset` in from the
/// wall's end at x = 119 − g/√2: the sheet's volume after it, and its flat before and after it
/// (see `flat_exact`). By hand, either way: the hole lies all in the wall's flat part (x within
/// 4 + `inset` of its end, z 16..24), so the sheet loses the disc, 16π = 50.2655 mm² through T 1,
/// and holds 16188.1469 − 50.2655 = 16137.8813 mm³ (see `corner_volume`), and the flat's material
/// loses 16π.
fn round_hole_by_a_walls_end(inset: f64) -> (f64, (f64, f64, usize, usize, usize), (f64, f64, usize, usize, usize)) {
    let gap = 0.25;
    let mut s = corner_sheet(gap);
    let before = flat_exact(&s);
    let x = 119.0 - gap / SQRT_2 - inset;
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 6, "base": [x, -3, 20], "axis": "Y", "operation": "cut"}));
    let (whole, _) = corner_volume(gap);
    let (v, want) = (exact_volume(&s), whole - 16.0 * PI);
    assert!((v - want).abs() < 1e-6 * want, "inset {inset}: {v} vs {want}");
    let after = flat_exact(&s);
    let lost = before.0 - after.0;
    assert!((lost - 16.0 * PI).abs() < 1e-6 * before.0, "inset {inset}: the flat's material lost {lost} of {}", 16.0 * PI);
    (v, before, after)
}

/// A round hole of radius 4 through a wall, its center 4 in from the wall's end where one flange
/// feature closed the corner with the rule's gap 0.25: it touches the end, the edge the rip left,
/// at one point only, so nothing of it runs on across the rip's lap, and it is a hole of the
/// wall's own outline (by hand, see `round_hole_by_a_walls_end`). Once the rip cuts the wall
/// back, the flat's outline takes it in at that point, or keeps it as a cut-out touching it
/// there: either way the material loses 16π.
#[test]
fn a_round_hole_touching_a_walls_end_at_a_closed_corner_stays_a_hole_of_the_wall() {
    let (_, before, after) = round_hole_by_a_walls_end(4.0);
    assert_eq!((after.3, after.4), (before.3, before.4 + 1), "a hole of the wall, not a cut of the sheet's");
    let into_outline = (before.1 - after.1 - 16.0 * PI).abs() < 1e-6 * before.1 && after.2 == before.2;
    let cut_out = (before.1 - after.1).abs() < 1e-6 * before.1 && after.2 == before.2 + 1;
    assert!(into_outline || cut_out, "the outline lost {} of {}, with {} cut-outs from {}", before.1 - after.1, 16.0 * PI, after.2, before.2);
}

/// The same hole 0.01 farther in, its center 4.01 from the wall's end: clear of it by 0.01, real
/// metal, it is a hole of the wall's own outline, as any hole clear inside a panel (by hand, see
/// `round_hole_by_a_walls_end`). The flat keeps its outline and has one more cut-out.
#[test]
fn a_round_hole_just_short_of_a_walls_end_at_a_closed_corner_is_a_hole_of_the_wall() {
    let (_, before, after) = round_hole_by_a_walls_end(4.01);
    assert_eq!((after.3, after.4), (before.3, before.4 + 1), "a hole of the wall, not a cut of the sheet's");
    assert!((before.1 - after.1).abs() < 1e-9 * before.1, "the outline changed by {}", before.1 - after.1);
    assert_eq!(after.2, before.2 + 1, "one more cut-out");
}

/// #1095: a 100 x 60 base flange on XY (T 2, R 2, K 0.5) with a 20 high flange on its edge
/// y = 60, unfolded, and a triangle cut through the flat whose corner lies on the flat's edge
/// x = 0 at the middle of the bend's zone: it touches the sheet's outline at that point only, so
/// it bounds the material with the outline there, never a hole clear inside it. It failed at the
/// cut ("a cut-out of the flat lost its piece"). By hand: the bend's setback is R + T = 4, so the
/// base keeps x 0..100, y 0..56 and the flange its 20 less 4, 16; the zone is the allowance
/// (π/2)(R + K T) = 3π/2 = 4.7123890 wide, y 56..60.7123890, its middle at y = 56 + 3π/4 =
/// 58.3561945. The flat holds 100 · (56 + 3π/2 + 16) = 7200 + 150π = 7671.2389 mm². The triangle
/// (0, yc), (3, yc − 1), (3, yc + 1) is 2 across and 3 deep, 3 mm², all in the zone (x 0..3,
/// y yc ± 1); the flat keeps 7668.2389 mm². At K 0.5 a bend keeps what its zone does flat: across
/// it, (π/4)((R + T)² − R²) = 3π = T · 3π/2 mm², and so does what is cut from it. Flat or folded,
/// the sheet holds T times its flat, 2 · 7668.2389 = 15336.4778 mm³.
#[test]
fn a_cut_out_with_a_corner_on_the_sheets_edge_at_a_bend_folds_with_the_sheet() {
    let mut s = session();
    rule(&mut s, "Rule", T, 0.5, 2.0, 0.5);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"type": "base", "sketch": "Base", "body_name": "Plate"}));
    run(&mut s, "sheet.flange", json!({"type": "edge", "edges": [[50, 60, T]], "height": 20, "angle": 90}));
    run(&mut s, "sheet.unfold", json!({}));
    let whole = 7200.0 + 150.0 * PI;
    let check = |s: &Session, want: f64, what: &str| {
        let (v, area) = (exact_volume(s), flat_exact(s).0);
        assert!((area - want).abs() < 1e-6 * want, "{what}: the flat holds {area} of {want}");
        assert!((v - T * want).abs() < 1e-6 * T * want, "{what}: {v} vs {}", T * want);
    };
    check(&s, whole, "unfolded");
    let yc = 56.0 + 0.75 * PI;
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cutout"}));
    run(&mut s, "sketch.line", json!({"points": [[0.0, yc], [3.0, yc - 1.0], [3.0, yc + 1.0]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Cutout", "through_all": true, "direction": "symmetric", "operation": "cut"}));
    check(&s, whole - 3.0, "cut on the flat");
    run(&mut s, "sheet.refold", json!({}));
    check(&s, whole - 3.0, "folded again");
}
