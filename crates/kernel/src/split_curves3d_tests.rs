//! Split Face by curves in space (3D sketch curves, #22414): on the scratch sheet (a circular arc
//! 40 mm wide with an 8 mm rise, radius 29 mm, extruded 30 mm) and on a box's top, each piece's
//! area against its closed form.
use crate::*;
use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::bspline::HPoint;
use solidcraft_geom::prelude::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

const R: f64 = 29.0;
const LENGTH: f64 = 30.0;

/// The half angle of the sheet's arc: asin(20 / 29).
fn half() -> f64 {
    (20.0 / R).asin()
}

/// The sheet's height above its chord at `x`: z = sqrt(29² − x²) − 21.
fn z(x: f64) -> f64 {
    (R * R - x * x).sqrt() - 21.0
}

/// The scratch sheet: x from -20 to 20, y from 0 to 30, its top at z = 8 over x = 0; one face.
fn sheet() -> Body {
    // Sketch x along X, sketch y along Z: the normal X × Z is -Y, so heights -30..0 are y 30..0.
    let plane = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
    let start = (21.0f64).atan2(20.0);
    let arc = Seg2::Arc { center: Vec2::new(0.0, -21.0), radius: R, start, sweep: PI - 2.0 * start };
    let s = extrude_sheet(&plane, &[vec![arc]], -LENGTH, 0.0).unwrap();
    assert_eq!(s.face_count(), 1);
    s
}

/// Each face's area by SMLib's integration, smallest first.
fn areas(b: &Body) -> Vec<f64> {
    let form = b.ops_body().unwrap();
    let mut a: Vec<f64> = (0..b.face_count()).map(|f| form.0.face_area(&form.1, f, 1e-9).unwrap()).collect();
    a.sort_by(f64::total_cmp);
    a
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-9)
}

/// The whole sheet: the arc's length 29 · 2 asin(20/29) times 30.
fn whole() -> f64 {
    R * 2.0 * half() * LENGTH
}

fn split_by(body: &Body, faces: &[usize], wires: Vec<Vec<PathSeg>>, along: Option<Vec3>) -> Result<Body> {
    let tool = curves3d_tool(body, faces, wires, along)?;
    split_faces(body, faces, &tool)
}

#[test]
fn the_scratch_sheet_is_what_the_tests_say() {
    let a = areas(&sheet());
    assert!(rel(a[0], whole()) < 1e-9, "{a:?} against {}", whole());
}

/// A straight 3D line lying on the sheet along its length, ending on its two straight edges,
/// swept along the sheet's normal there (the default): the radial plane through the sheet's
/// axis, which meets the sheet in the line itself. The piece past it spans the angle from
/// asin(10/29) to asin(20/29).
#[test]
fn a_straight_line_on_the_sheet_splits_along_itself() {
    let s = sheet();
    let line = vec![PathSeg::Line { a: Vec3::new(10.0, 0.0, z(10.0)), b: Vec3::new(10.0, LENGTH, z(10.0)) }];
    let tool = curves3d_tool(&s, &[0], vec![line.clone()], None).unwrap();
    let SplitTool::Curves3d { wires } = &tool else { panic!("a 3D curve tool") };
    let normal = Vec3::new(10.0, 0.0, z(10.0) + 21.0) * (1.0 / R);
    assert!((wires[0].along - normal).len() < 1e-9, "the sweep is the sheet's normal at the line: {:?}", wires[0].along);
    let split = split_faces(&s, &[0], &tool).unwrap();
    assert_eq!(split.face_count(), 2);
    let a = areas(&split);
    let side = R * (half() - (10.0 / R).asin()) * LENGTH;
    assert!(rel(a[0], side) < 1e-6, "{} against {side}", a[0]);
    assert!(rel(a[1], whole() - side) < 1e-6, "{} against {}", a[1], whole() - side);
    assert!(rel(a[0] + a[1], whole()) < 1e-9);
}

/// A 3D spline across the sheet (a quadratic B-spline from edge to edge whose plan view is the
/// parabola y = 5 + x²/80), swept along +Z: the piece on the y = 0 side has the area
/// ∫ (5 + x²/80) · 29 / sqrt(29² − x²) dx over -20..20
/// = 29 · (10 asin(20/29) + (29² asin(20/29) − 20 · 21) / 80).
#[test]
fn a_spline_across_the_sheet_splits_it_in_two() {
    let s = sheet();
    let ctrl = [
        HPoint::weighted(Vec3::new(-20.0, 10.0, 0.0), 1.0),
        HPoint::weighted(Vec3::new(0.0, 0.0, 16.0), 1.0),
        HPoint::weighted(Vec3::new(20.0, 10.0, 0.0), 1.0),
    ];
    let spline = vec![PathSeg::Bezier(SpaceBezier::new(&ctrl).unwrap())];
    let split = split_by(&s, &[0], vec![spline], Some(Vec3::Z)).unwrap();
    assert_eq!(split.face_count(), 2);
    let a = areas(&split);
    let near = R * (10.0 * half() + (R * R * half() - 20.0 * 21.0) / 80.0);
    assert!((near - 300.448).abs() < 1e-3, "{near}");
    assert!(rel(a[0], near) < 1e-6, "{} against {near}", a[0]);
    assert!(rel(a[1], whole() - near) < 1e-6, "{} against {}", a[1], whole() - near);
}

/// The sheet's own cross-section at y = 15, an exact 3D arc from edge to edge over the top:
/// swept along the mean normal (in the arc's plane), it is that plane, which halves the sheet.
#[test]
fn an_arc_on_the_sheet_halves_it() {
    let s = sheet();
    let arc = vec![PathSeg::Arc { a: Vec3::new(-20.0, 15.0, 0.0), center: Vec3::new(0.0, 15.0, -21.0), axis: Vec3::Y, angle: 2.0 * half() }];
    let split = split_by(&s, &[0], vec![arc], None).unwrap();
    assert_eq!(split.face_count(), 2);
    let a = areas(&split);
    assert!(rel(a[0], whole() / 2.0) < 1e-6 && rel(a[1], whole() / 2.0) < 1e-6, "{a:?}");
}

/// A closed 3D circle of radius 5 on a box's top, swept along the top's normal (the default):
/// the disc and the rest, the disc named inside the loop.
#[test]
fn a_closed_circle_cuts_a_disc_out_of_a_top() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let top = b.face_within(Vec3::new(20.0, 15.0, 20.0), 1e-6).unwrap().unwrap();
    let circle = vec![PathSeg::Arc { a: Vec3::new(25.0, 15.0, 20.0), center: Vec3::new(20.0, 15.0, 20.0), axis: Vec3::Z, angle: TAU }];
    let tool = curves3d_tool(&b, &[top], vec![circle], None).unwrap();
    let (split, from) = split_faces_with_map(&b, &[top], &tool).unwrap();
    assert_eq!(split.face_count(), 7);
    let pieces: Vec<usize> = (0..split.face_count()).filter(|f| from.get(*f) == Some(&top)).collect();
    assert_eq!(pieces.len(), 2);
    let form = split.ops_body().unwrap();
    let mut a: Vec<(f64, usize)> = pieces.iter().map(|f| (form.0.face_area(&form.1, *f, 1e-9).unwrap(), *f)).collect();
    a.sort_by(|x, y| x.0.total_cmp(&y.0));
    assert!(rel(a[0].0, 25.0 * PI) < 1e-6 && rel(a[1].0, 1200.0 - 25.0 * PI) < 1e-6, "{a:?}");
    let sides = split_piece_sides(&split, &[a[0].1, a[1].1], &tool).unwrap();
    assert_eq!(sides.fields, vec![FieldKind::Loop]);
    assert_eq!(sides.sides, vec![vec![Some(true)], vec![Some(false)]], "the disc is inside the loop");
}

/// A closed 3D circle of radius 0.005 (under the 0.01 a loop used to be sampled to, which made it
/// one chord and no turn) on the top of a 0.04 × 0.03 × 0.02 box cuts a disc of 0.005² π out of
/// it, named inside the loop whichever way round the circle is drawn.
#[test]
fn a_small_closed_circle_is_inside_either_way_round() {
    let b = box_solid(Vec3::ZERO, Vec3::new(0.04, 0.03, 0.02)).unwrap();
    let top = b.face_within(Vec3::new(0.02, 0.015, 0.02), 1e-9).unwrap().unwrap();
    for angle in [TAU, -TAU] {
        let circle = vec![PathSeg::Arc { a: Vec3::new(0.025, 0.015, 0.02), center: Vec3::new(0.02, 0.015, 0.02), axis: Vec3::Z, angle }];
        let tool = curves3d_tool(&b, &[top], vec![circle], None).unwrap();
        let (split, from) = split_faces_with_map(&b, &[top], &tool).unwrap();
        let pieces: Vec<usize> = (0..split.face_count()).filter(|f| from.get(*f) == Some(&top)).collect();
        let form = split.ops_body().unwrap();
        let mut a: Vec<(f64, usize)> = pieces.iter().map(|f| (form.0.face_area(&form.1, *f, 1e-12).unwrap(), *f)).collect();
        a.sort_by(|x, y| x.0.total_cmp(&y.0));
        assert_eq!(a.len(), 2, "{angle}: {a:?}");
        assert!(rel(a[0].0, 0.005 * 0.005 * PI) < 1e-6, "{angle}: {a:?}");
        let sides = split_piece_sides(&split, &[a[0].1, a[1].1], &tool).unwrap();
        assert_eq!(sides.sides, vec![vec![Some(true)], vec![Some(false)]], "{angle}: the disc is inside the loop");
    }
}

/// A closed 3D loop (10, 5, 20), (30, 25, 20), (10, 25, 22), (30, 5, 22) swept along +Z: seen
/// along the sweep a bow tie crossing itself at (20, 15), its lobes the triangles of area
/// ½ · 20 · 10 = 100 above and below that point, turning opposite ways (the total turn is 0). The
/// 40 × 30 top splits into the two lobes and the rest, 1200 − 200 = 1000, and no piece is named
/// inside or outside the loop, since no one side of it is inside for both lobes.
#[test]
fn a_loop_crossing_itself_along_the_sweep_names_no_inside() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let top = b.face_within(Vec3::new(20.0, 15.0, 20.0), 1e-6).unwrap().unwrap();
    let p = [Vec3::new(10.0, 5.0, 20.0), Vec3::new(30.0, 25.0, 20.0), Vec3::new(10.0, 25.0, 22.0), Vec3::new(30.0, 5.0, 22.0)];
    let bow: Vec<PathSeg> = (0..4).map(|i| PathSeg::Line { a: p[i], b: p[(i + 1) % 4] }).collect();
    let tool = curves3d_tool(&b, &[top], vec![bow], Some(Vec3::Z)).unwrap();
    let (split, from) = split_faces_with_map(&b, &[top], &tool).unwrap();
    let pieces: Vec<usize> = (0..split.face_count()).filter(|f| from.get(*f) == Some(&top)).collect();
    let form = split.ops_body().unwrap();
    let mut a: Vec<(f64, usize)> = pieces.iter().map(|f| (form.0.face_area(&form.1, *f, 1e-9).unwrap(), *f)).collect();
    a.sort_by(|x, y| x.0.total_cmp(&y.0));
    assert_eq!(a.len(), 3, "{a:?}");
    assert!(rel(a[0].0, 100.0) < 1e-6 && rel(a[1].0, 100.0) < 1e-6 && rel(a[2].0, 1000.0) < 1e-6, "{a:?}");
    let faces: Vec<usize> = a.iter().map(|x| x.1).collect();
    let sides = split_piece_sides(&split, &faces, &tool).unwrap();
    assert_eq!(sides.fields, vec![FieldKind::Loop]);
    assert_eq!(sides.sides, vec![vec![None], vec![None], vec![None]], "neither lobe is called inside");
}

/// A 3D line rising steeply over a box's top, from (20, 15, 20) to (20.01, 15, 30), swept along +Z:
/// its sheet is the plane y = 15, and its ends, run on until they have passed the top across the
/// sweep (not only 0.001 of that), halve the 40 × 30 top: 600 and 600.
#[test]
fn a_steep_line_runs_on_across_the_sweep() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let top = b.face_within(Vec3::new(20.0, 15.0, 20.0), 1e-6).unwrap().unwrap();
    let steep = vec![PathSeg::Line { a: Vec3::new(20.0, 15.0, 20.0), b: Vec3::new(20.01, 15.0, 30.0) }];
    let tool = curves3d_tool(&b, &[top], vec![steep], Some(Vec3::Z)).unwrap();
    let (split, from) = split_faces_with_map(&b, &[top], &tool).unwrap();
    assert_eq!(split.face_count(), 7);
    let form = split.ops_body().unwrap();
    let a: Vec<f64> = (0..split.face_count()).filter(|f| from.get(*f) == Some(&top)).map(|f| form.0.face_area(&form.1, f, 1e-9).unwrap()).collect();
    assert_eq!(a.len(), 2, "{a:?}");
    assert!(a.iter().all(|x| rel(*x, 600.0) < 1e-6), "{a:?}");
}

/// An open clockwise quarter arc on a box's top (radius 5 about (20, 15), from (25, 15) to
/// (20, 10), turning -90° about +Z) is continued along its own travel: before its start toward +Y
/// to the edge at y = 30, after its end toward -X to the edge at x = 0. The piece holding the
/// corner (0, 30) is the rectangle 25 × 20 less the 5 × 5 corner the arc rounds, plus the
/// quarter disc: 500 − 25 + 25π/4; the other is the rest of 1200.
#[test]
fn a_clockwise_open_arc_runs_on_along_its_travel() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let top = b.face_within(Vec3::new(20.0, 15.0, 20.0), 1e-6).unwrap().unwrap();
    let arc = vec![PathSeg::Arc { a: Vec3::new(25.0, 15.0, 20.0), center: Vec3::new(20.0, 15.0, 20.0), axis: Vec3::Z, angle: -FRAC_PI_2 }];
    let tool = curves3d_tool(&b, &[top], vec![arc], None).unwrap();
    let (split, from) = split_faces_with_map(&b, &[top], &tool).unwrap();
    assert_eq!(split.face_count(), 7);
    let form = split.ops_body().unwrap();
    let mut a: Vec<f64> =
        (0..split.face_count()).filter(|f| from.get(*f) == Some(&top)).map(|f| form.0.face_area(&form.1, f, 1e-9).unwrap()).collect();
    a.sort_by(f64::total_cmp);
    let corner = 475.0 + 25.0 * PI / 4.0;
    assert_eq!(a.len(), 2, "{a:?}");
    assert!(rel(a[0], corner) < 1e-6 && rel(a[1], 1200.0 - corner) < 1e-6, "{a:?} against {corner}");
}

/// A sweep lying in the face's tangent plane all along the curve is refused, and so is a curve
/// whose sheet misses the face; a zero direction is refused too.
#[test]
fn a_sweep_along_the_face_and_a_curve_that_misses_are_refused() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
    let top = b.face_within(Vec3::new(20.0, 15.0, 20.0), 1e-6).unwrap().unwrap();
    let along_x = vec![PathSeg::Line { a: Vec3::new(5.0, 15.0, 20.0), b: Vec3::new(35.0, 15.0, 20.0) }];
    let e = curves3d_tool(&b, &[top], vec![along_x.clone()], Some(Vec3::Y)).unwrap_err().to_string();
    assert!(e.contains("tangent"), "{e}");
    let e = curves3d_tool(&b, &[top], vec![along_x.clone()], Some(Vec3::ZERO)).unwrap_err().to_string();
    assert!(e.contains("nonzero"), "{e}");
    // Along the line itself (here also along the top): refused as running along the face.
    let e = split_by(&b, &[top], vec![along_x], Some(Vec3::X)).unwrap_err().to_string();
    assert!(e.contains("tangent"), "{e}");
    // A line standing up through the top swept along itself: its sweep has no area.
    let upright = vec![PathSeg::Line { a: Vec3::new(20.0, 15.0, 10.0), b: Vec3::new(20.0, 15.0, 30.0) }];
    let e = split_by(&b, &[top], vec![upright], Some(Vec3::Z)).unwrap_err().to_string();
    assert!(e.contains("along the split direction"), "{e}");
    // Beside the sheet: it lies over no point of the face, so there is no normal to default to;
    // along +Z its sheet stands beside the face and misses it.
    let s = sheet();
    let beside = vec![PathSeg::Line { a: Vec3::new(30.0, 0.0, 0.0), b: Vec3::new(30.0, LENGTH, 0.0) }];
    let e = split_by(&s, &[0], vec![beside.clone()], None).unwrap_err().to_string();
    assert!(e.contains("give a direction"), "{e}");
    let e = split_by(&s, &[0], vec![beside], Some(Vec3::Z)).unwrap_err().to_string();
    assert!(e.contains("does not cross"), "{e}");
    assert!(curves3d_tool(&s, &[0], vec![], None).is_err());
    assert!(curves3d_tool(&s, &[0], vec![vec![]], None).is_err());
    let quarter = vec![PathSeg::Arc { a: Vec3::new(-20.0, 15.0, 0.0), center: Vec3::new(0.0, 15.0, -21.0), axis: Vec3::Y, angle: FRAC_PI_2 * 5.0 }];
    assert!(split_by(&s, &[0], vec![quarter], None).is_err(), "more than a turn");
}
