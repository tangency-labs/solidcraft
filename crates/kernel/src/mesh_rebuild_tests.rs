//! A plan is checked against parts whose answer is known by construction: a solid is built, frozen
//! to triangles, recognized, and planned, and the plan has to say what the solid was made from.

use solidcore_record::Vec3;

use super::*;
use crate::mesh_recognize::recognize_mesh;
use crate::{Body, BoolOp, MeshTolerance, mesh_body};

/// The same shape as an imported STL: triangles, with no faces left.
fn frozen(body: &Body) -> Body {
    let mesh = body.mesh(&MeshTolerance::chord(0.005)).unwrap();
    mesh_body(&mesh.positions, &mesh.triangles).unwrap()
}

fn plan_of(body: &Body, axis: Option<Vec3>) -> Prismatic {
    let found = recognize_mesh(&frozen(body), 0.05, None).unwrap();
    prismatic_plan(&found, axis, 2f64.to_radians()).unwrap()
}

fn refusal(body: &Body) -> String {
    let found = recognize_mesh(&frozen(body), 0.05, None).unwrap();
    match prismatic_plan(&found, None, 2f64.to_radians()) {
        Ok(plan) => panic!("expected a refusal, got {:?} with {} bores", plan.outline, plan.bores.len()),
        Err(err) => err.to_string(),
    }
}

fn corners(plan: &Prismatic) -> Vec<Corner> {
    match &plan.outline {
        Outline::Polygon(corners) => corners.clone(),
        other => panic!("expected a polygon, got {other:?}"),
    }
}

/// The width and height the profile covers, which is independent of where the sketch origin
/// landed on the end face.
fn span(plan: &Prismatic) -> [f64; 2] {
    let corners = corners(plan);
    let axis = |i: usize| {
        let lo = corners.iter().map(|c| c.at[i]).fold(f64::INFINITY, f64::min);
        let hi = corners.iter().map(|c| c.at[i]).fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    };
    [axis(0), axis(1)]
}

fn drilled(plate: &Body, holes: &[(Vec3, f64)]) -> Body {
    let mut body = plate.clone();
    for &(at, radius) in holes {
        let tool = crate::cylinder(at - Vec3::Z, Vec3::Z, radius, 100.).unwrap();
        body = crate::boolean(&body, &tool, BoolOp::Cut).unwrap().unwrap();
    }
    body
}

#[test]
fn a_drilled_plate_comes_back_as_a_profile_a_thickness_and_its_holes() {
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let plan = plan_of(&drilled(&plate, &[(Vec3::new(10., 10., 0.), 3.), (Vec3::new(30., 20., 0.), 2.5)]), None);
    // The axis was not given: the plan takes the direction with the most area square to it, which
    // on a plate is the one between its two faces.
    assert!(plan.axis.z.abs() > 0.999, "{:?}", plan.axis);
    assert!((plan.thickness - 10.).abs() < 0.05, "{}", plan.thickness);
    assert_eq!(corners(&plan).len(), 4);
    let [width, height] = span(&plan);
    assert!((width - 40.).abs() < 0.05 && (height - 30.).abs() < 0.05, "{width} x {height}");
    assert_eq!(plan.bores.len(), 2, "{:?}", plan.bores);
    assert!(plan.bores.iter().all(|b| b.through), "{:?}", plan.bores);
    assert!(plan.bores.iter().any(|b| (b.radius - 3.).abs() < 0.01));
    assert!(plan.bores.iter().any(|b| (b.radius - 2.5).abs() < 0.01));
    assert!(plan.bosses.is_empty());
    // Six faces and two bores, and the plan uses every one of them.
    assert!(plan.accounted > 0.99, "{} with {:?}", plan.accounted, plan.unused);
}

#[test]
fn a_round_part_comes_back_as_one_circle() {
    let plan = plan_of(&crate::cylinder(Vec3::ZERO, Vec3::Z, 12., 8.).unwrap(), None);
    assert!((plan.thickness - 8.).abs() < 0.05, "{}", plan.thickness);
    match plan.outline {
        Outline::Circle { radius, .. } => assert!((radius - 12.).abs() < 0.05, "{radius}"),
        ref other => panic!("expected a circle, got {other:?}"),
    }
    assert!(plan.bores.is_empty() && plan.bosses.is_empty());
    assert!(plan.accounted > 0.99, "{} with {:?}", plan.accounted, plan.unused);
}

#[test]
fn a_bore_that_stops_short_comes_back_as_a_depth_from_the_face_it_was_drilled_from() {
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(30., 30., 12.)).unwrap();
    let pocket = crate::cylinder(Vec3::new(15., 15., 7.), Vec3::Z, 4., 6.).unwrap();
    let blind = crate::boolean(&plate, &pocket, BoolOp::Cut).unwrap().unwrap();
    let plan = plan_of(&blind, None);
    assert!((plan.thickness - 12.).abs() < 0.05, "{}", plan.thickness);
    let [bore] = plan.bores.as_slice() else { panic!("expected one bore, got {:?}", plan.bores) };
    assert!(!bore.through && bore.from_top, "{bore:?}");
    assert!((bore.depth - 5.).abs() < 0.05, "{bore:?}");
    assert!((bore.radius - 4.).abs() < 0.01, "{bore:?}");
    // The hole's flat floor is the bore's own, and it is neither taken for an end face nor
    // reported as something the rebuild left out.
    assert!((bore.at - 12.).abs() < 0.05, "{bore:?}");
    assert!(plan.unused.is_empty(), "{:?}", plan.unused);
    assert!(plan.accounted > 0.99, "{} with {:?}", plan.accounted, plan.unused);
}

#[test]
fn a_rounded_corner_becomes_that_corner_s_radius() {
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let edges = [Vec3::new(0., 0., 5.), Vec3::new(40., 0., 5.), Vec3::new(40., 30., 5.), Vec3::new(0., 30., 5.)];
    let rounded = crate::fillet(&plate, &edges, 5.).unwrap();
    let plan = plan_of(&rounded, None);
    let corners = corners(&plan);
    assert_eq!(corners.len(), 4, "{corners:?}");
    assert!(corners.iter().all(|c| (c.radius - 5.).abs() < 0.01), "{corners:?}");
    // The corner rounds are the outline, not four bosses standing in the middle of it.
    assert!(plan.bores.is_empty() && plan.bosses.is_empty(), "{:?} {:?}", plan.bores, plan.bosses);
    let [width, height] = span(&plan);
    assert!((width - 40.).abs() < 0.05 && (height - 30.).abs() < 0.05, "{width} x {height}");
}

#[test]
fn a_boss_standing_on_the_far_face_comes_back_as_a_boss() {
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let post = crate::cylinder(Vec3::new(20., 15., 10.), Vec3::Z, 6., 8.).unwrap();
    let plan = plan_of(&crate::boolean(&plate, &post, BoolOp::Union).unwrap().unwrap(), None);
    // The thickness is between the plate's own faces, not up to the top of the post.
    assert!((plan.thickness - 10.).abs() < 0.05, "{}", plan.thickness);
    let [boss] = plan.bosses.as_slice() else { panic!("expected one boss, got {:?}", plan.bosses) };
    assert!((boss.radius - 6.).abs() < 0.01 && (boss.height - 8.).abs() < 0.05, "{boss:?}");
    assert!(plan.bores.is_empty(), "{:?}", plan.bores);
}

#[test]
fn an_outline_that_turns_back_on_itself_is_refused_and_not_rebuilt_as_a_smaller_rectangle() {
    let arm = crate::box_solid(Vec3::ZERO, Vec3::new(20., 30., 10.)).unwrap();
    let foot = crate::box_solid(Vec3::ZERO, Vec3::new(40., 15., 10.)).unwrap();
    let ell = crate::boolean(&arm, &foot, BoolOp::Union).unwrap().unwrap();
    let message = refusal(&ell);
    assert!(message.contains("turns back on itself"), "{message}");
}

#[test]
fn a_part_with_nothing_to_extrude_between_is_refused_and_hostile_input_never_panics() {
    let empty = Recognition { triangles: 0, vertices: 0, closed: false, area: 0., tolerance: 0.05, patches: Vec::new(), axes: Vec::new() };
    let align = 2f64.to_radians();
    assert!(prismatic_plan(&empty, None, align).is_err());
    assert!(prismatic_plan(&empty, Some(Vec3::Z), align).is_err());
    assert!(prismatic_plan(&empty, Some(Vec3::ZERO), align).is_err());
    assert!(prismatic_plan(&empty, Some(Vec3::new(f64::NAN, 0., 0.)), align).is_err());
    for bad in [0., -1., 1e9, f64::NAN, f64::INFINITY] {
        assert!(prismatic_plan(&empty, Some(Vec3::Z), bad).is_err(), "align {bad}");
    }
    // A sphere has faces to recognize and no two ends, which is a refusal and not a crash.
    let ball = crate::sphere(Vec3::ZERO, 10.).unwrap();
    let found = recognize_mesh(&frozen(&ball), 0.05, None).unwrap();
    assert!(prismatic_plan(&found, Some(Vec3::Z), align).is_err());
}

#[test]
fn a_bore_through_a_boss_is_drilled_from_the_top_of_that_boss() {
    // The order matters when the two stand over each other: the boss is built first and the hole
    // goes through both, or the boss fills the hole in again and the part comes out solid there.
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let post = crate::cylinder(Vec3::new(20., 15., 10.), Vec3::Z, 12., 14.).unwrap();
    let standing = crate::boolean(&plate, &post, BoolOp::Union).unwrap().unwrap();
    let drill = crate::cylinder(Vec3::new(20., 15., -1.), Vec3::Z, 6., 30.).unwrap();
    let part = crate::boolean(&standing, &drill, BoolOp::Cut).unwrap().unwrap();
    let plan = plan_of(&part, None);
    assert!((plan.thickness - 10.).abs() < 0.05, "{}", plan.thickness);
    let [boss] = plan.bosses.as_slice() else { panic!("expected one boss, got {:?}", plan.bosses) };
    assert!((boss.radius - 12.).abs() < 0.05 && (boss.height - 14.).abs() < 0.05, "{boss:?}");
    let [bore] = plan.bores.as_slice() else { panic!("expected one bore, got {:?}", plan.bores) };
    assert!(bore.through && bore.from_top, "{bore:?}");
    assert!((bore.radius - 6.).abs() < 0.05, "{bore:?}");
    // It is drilled from the top of the boss, 24 above the sketch, and goes all the way down.
    assert!((bore.at - 24.).abs() < 0.05, "{bore:?}");
    assert!((bore.depth - 24.).abs() < 0.05, "{bore:?}");
}
