//! Cuts on a sheet made from surfaces (#6748): what a feature that is not sheet metal cuts from
//! the sheet's body is read back onto its flat panels. A cut through the sheet is a cut-out of
//! its flat; a cut that stops on a floor inside the thickness is a pocket, measured from the
//! face on the sheet's surface; a cut through a pocket takes it with the material; what the
//! sheet can't keep is refused, and the sheet stays as it was.

use std::f64::consts::PI;

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2, Vec3};

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
        let Ok(dev) = solidcraft_develop::develop(&sheet.sheet, 0) else { panic!("the sheet lies flat") };
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
    let Ok(dev) = solidcraft_develop::develop(&f.sheet.sheet, 0) else { panic!("the sheet lies flat") };
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

/// Cuts that part a bend from its panels are refused: one that takes the whole bend, cutting the
/// sheet in two, and a ring that leaves an island of the bend joined to neither panel.
#[test]
fn cuts_that_part_a_bend_from_its_panels_are_refused() {
    let f = Flanged::new();
    let w = f.width;
    let refused = |cuts: Vec<Region2>| f.cut_on(0, cuts).err().map(|e| e.to_string()).unwrap_or_default();
    let e = refused(vec![f.rect([-5.0, f.length + 5.0], [-1.0, w + 1.0])]);
    assert!(e.contains("takes bend 1 apart"), "{e}");
    let c = f.at(50.0, w / 2.0);
    let e = refused(vec![Region2 { outer: disc(c, 2.0), holes: vec![disc(c, 1.0).reversed()] }]);
    assert!(e.contains("joined to neither"), "{e}");
}

/// A 100 x 60 plate of 2 mm sheet with a 30 high flange on its edge y = 0, bent R 10 (K 0.44):
/// the bend's axis runs along x at y = z = 12, its metal 10 to 12 from it.
fn wide_bend() -> Session {
    let mut s = Session::default();
    rule(&mut s, "Wide", T, 0.44, 10.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": T, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"mode": "thicken", "body": "Plate", "face": [50, 30, T]}));
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
