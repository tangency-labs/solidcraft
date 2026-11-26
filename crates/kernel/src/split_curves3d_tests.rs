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
