//! Booleans of bodies with faces in a common plane: a face is pushed out of the plane only where
//! that leaves the result as it is. Every expected volume is what the two bodies define.
#![allow(clippy::unwrap_used, clippy::panic)]
use crate::*;
use solidcraft_geom::Vec3;

/// An l x w x h block with two bottom edges chamfered by 1 mm: no prism along any axis, so
/// its booleans are not done on sections.
fn block(l: f64, w: f64, h: f64) -> Body {
    let b = box_solid(Vec3::ZERO, Vec3::new(l, w, h)).unwrap();
    let b = chamfer(&b, &[Vec3::new(l / 2.0, w, 0.0)], 1.0).unwrap();
    chamfer(&b, &[Vec3::new(l, w / 2.0, 0.0)], 1.0).unwrap()
}

/// A 200 x 100 x 20 block with a rib across it: 20 wide over the block's 100, from the top
/// face z = 20 up to z = 30.
fn ribbed() -> Body {
    let rib = box_solid(Vec3::new(90.0, 0.0, 20.0), Vec3::new(110.0, 100.0, 30.0)).unwrap();
    boolean(&block(200.0, 100.0, 20.0), &rib, BoolOp::Union).unwrap().unwrap()
}

/// A channel along that block, longer than its 200: y 30 to 70 and z 10 to 20, so its top
/// lies in the plane of the face the rib stands on.
fn channel() -> Body {
    box_solid(Vec3::new(-20.0, 30.0, 10.0), Vec3::new(220.0, 70.0, 20.0)).unwrap()
}

/// What the boolean of `a` and `b` adds to a's volume, the result's lumps, and how it was made.
fn change(a: &Body, b: &Body, op: BoolOp) -> (f64, usize, BooleanDiagnostics) {
    let (r, how) = capture_boolean_diagnostics(|| boolean(a, b, op));
    let r = r.unwrap().unwrap();
    (measure(&r).unwrap().volume - measure(a).unwrap().volume, r.lumps().unwrap().len(), how)
}

fn used(how: &BooleanDiagnostics, strategy: BooleanStrategy) -> bool {
    how.events.iter().any(|e| e.strategy == strategy)
}

/// A channel cut along a block, up to the face a rib stands on. The channel's top lies in
/// that face's plane and the rib bridges it: pushed up out of the plane (2.29 % of the
/// body's size, 5.6 mm), the top would cut a notch 20 x 40 into the rib.
#[test]
fn a_cut_up_to_a_face_leaves_what_stands_on_the_face() {
    let (added, lumps, how) = change(&ribbed(), &channel(), BoolOp::Cut);
    // It takes 200 x 40 x 10 of the block and nothing of the rib.
    assert!((added + 80_000.0).abs() < 1e-6 && lumps == 1, "{added}, {lumps} lumps, {how:?}");
    assert!(!used(&how, BooleanStrategy::CoplanarSeparation), "{how:?}");
}

/// The same with a round hole through the block beside the channel (r 4 at 30, 15: y 11 to
/// 19, the channel from 30 on). The body no longer has planar faces only, but the hole lies
/// clear of the tool, and the cut stays what the channel is.
#[test]
fn a_round_hole_beside_the_cut_changes_nothing() {
    let hole = cylinder(Vec3::new(30.0, 15.0, -1.0), Vec3::Z, 4.0, 22.0).unwrap();
    let drilled = boolean(&ribbed(), &hole, BoolOp::Cut).unwrap().unwrap();
    let (added, lumps, how) = change(&drilled, &channel(), BoolOp::Cut);
    // 200 x 40 x 10 again (the hole is measured on its mesh, before and after).
    assert!((added + 80_000.0).abs() < 1e-3 && lumps == 1, "{added}, {lumps} lumps, {how:?}");
    assert!(!used(&how, BooleanStrategy::CoplanarSeparation), "{how:?}");
}

/// With nothing in their way the faces are pushed as before: a pocket's top out of the plane
/// of the block's top face, a pad's foot into the block. (This holds without the check too:
/// it keeps the check from refusing what is harmless.)
#[test]
fn faces_with_room_are_pushed_apart() {
    let block = block(200.0, 100.0, 20.0);
    // A pocket 50 x 60 x 5 from the top face: 15 000 less.
    let pocket = box_solid(Vec3::new(75.0, 20.0, 15.0), Vec3::new(125.0, 80.0, 20.0)).unwrap();
    let (added, _, how) = change(&block, &pocket, BoolOp::Cut);
    assert!((added + 15_000.0).abs() < 1e-6 && used(&how, BooleanStrategy::CoplanarSeparation), "{added}, {how:?}");
    // A pad 50 x 60 x 10 on the top face: 30 000 more.
    let pad = box_solid(Vec3::new(75.0, 20.0, 20.0), Vec3::new(125.0, 80.0, 30.0)).unwrap();
    let (added, lumps, how) = change(&block, &pad, BoolOp::Union);
    assert!((added - 30_000.0).abs() < 1e-6 && lumps == 1 && used(&how, BooleanStrategy::CoplanarSeparation), "{added}, {how:?}");
}

/// A disc joined into a round hole it fills (issue: "boolean Union: no result" for a 1 mm
/// floor extruded into a through hole of a part that isn't a prism). The disc's wall lies on
/// the hole's, and at either end its face lies in the block's bottom or top face: it adds its
/// own volume, the block stays one piece, and the hole is closed where the disc is.
#[test]
fn a_disc_joined_into_the_hole_it_fills() {
    let hole = cylinder(Vec3::new(150.0, 50.0, -1.0), Vec3::Z, 20.0, 22.0).unwrap();
    let drilled = boolean(&block(200.0, 100.0, 20.0), &hole, BoolOp::Cut).unwrap().unwrap();
    for (z0, z1) in [(0.0, 1.0), (5.0, 8.0), (19.0, 20.0)] {
        let disc = cylinder(Vec3::new(150.0, 50.0, z0), Vec3::Z, 20.0, z1 - z0).unwrap();
        let want = measure(&disc).unwrap().volume;
        let (r, how) = capture_boolean_diagnostics(|| boolean(&drilled, &disc, BoolOp::Union));
        let r = r.unwrap().unwrap();
        let added = measure(&r).unwrap().volume - measure(&drilled).unwrap().volume;
        assert!((added - want).abs() < want * 1e-3, "{z0}..{z1}: {added} vs {want}, {how:?}");
        assert!(r.lumps().unwrap().len() == 1 && r.validity().is_empty(), "{z0}..{z1}: {:?}", r.validity());
        // Exact: nothing of the tool is moved.
        assert!(used(&how, BooleanStrategy::GrownBoss) && !used(&how, BooleanStrategy::ToolNudge), "{z0}..{z1}: {how:?}");
    }
}

/// A hemisphere of radius `r` standing on the plane z = 0.
fn hemisphere(r: f64) -> Body {
    let ball = sphere(Vec3::ZERO, r).unwrap();
    let below = box_solid(Vec3::new(-2.0 * r, -2.0 * r, -2.0 * r), Vec3::new(2.0 * r, 2.0 * r, 0.0)).unwrap();
    boolean(&ball, &below, BoolOp::Cut).unwrap().unwrap()
}

/// A hemisphere (r 20) and a block (x 5 to 35, y -30 to 30, z 0 to 25) that both stand on z = 0, the block's foot over most of the
/// hemisphere's flat face. The neighbour of that face is the sphere, which cannot be stretched, so its face is grown instead (a
/// strip along its rim) and what the join leaves below the plane is cut off by editing faces. The block takes the part of the dome
/// beyond x = 5, half of a cap 15 high: the union is the dome and the block less that part.
#[test]
fn a_block_flush_with_the_flat_face_of_a_hemisphere_joins_exactly() {
    let (r, x0) = (20.0, 5.0);
    let block = box_solid(Vec3::new(x0, -30.0, 0.0), Vec3::new(35.0, 30.0, 25.0)).unwrap();
    let dome = hemisphere(r);
    let h = r - x0;
    let expected = 2.0 / 3.0 * std::f64::consts::PI * r.powi(3) - std::f64::consts::PI * h * h * (3.0 * r - h) / 6.0 + 30.0 * 60.0 * 25.0;
    for (a, b) in [(&dome, &block), (&block, &dome)] {
        let joined = crate::coplanar::boolean_grown(a, b, BoolOp::Union).expect("the route applies").unwrap().unwrap();
        let v = measure(&joined).unwrap().volume;
        assert!((v - expected).abs() < expected * 1e-4, "{v} against {expected}");
        assert_eq!(joined.lumps().unwrap().len(), 1);
        assert!(joined.validity().is_empty(), "{:?}", joined.validity());
        // The flat faces are one plane again: nothing of the join stays below z = 0.
        let mesh = joined.tessellate(0.05).unwrap();
        let lowest = mesh.positions.iter().map(|p| p.z).fold(f64::MAX, f64::min);
        assert!(lowest > -1e-6, "the join reaches down to z = {lowest}");
    }
}

/// Growing the flat face of a dome past its plane and cutting what that adds off again gives the dome back: the same volume, the same
/// faces, nothing below the plane.
#[test]
fn what_growing_a_face_adds_is_cut_off_again_exactly() {
    let dome = hemisphere(20.0);
    let measured = measure(&dome).unwrap().volume;
    let grown = crate::coplanar::grow_faces_exact(&dome, -Vec3::Z, 0.0, 3.0).unwrap();
    assert!(measure(&grown).unwrap().volume > measured + 3.0 * 300.0, "the stub adds volume");
    let back = crate::coplanar::trim_exact(&grown, -Vec3::Z, 0.0).unwrap();
    let v = measure(&back).unwrap().volume;
    assert!((v - measured).abs() < measured * 1e-6, "{v} against {measured}");
    assert_eq!(back.face_count(), dome.face_count());
    assert!(back.validity().is_empty(), "{:?}", back.validity());
    let lowest = back.tessellate(0.05).unwrap().positions.iter().map(|p| p.z).fold(f64::MAX, f64::min);
    assert!(lowest > -1e-6, "the body reaches down to z = {lowest}");
}

/// A plane the body stands on has nothing beyond it, and one clear of the body is refused (nothing is cut off).
#[test]
fn trimming_at_a_plane_with_nothing_beyond_it() {
    let dome = hemisphere(20.0);
    let measured = measure(&dome).unwrap().volume;
    let same = crate::coplanar::trim_exact(&dome, -Vec3::Z, 0.0).unwrap();
    assert!((measure(&same).unwrap().volume - measured).abs() < measured * 1e-9);
    assert!(crate::coplanar::trim_exact(&dome, -Vec3::Z, 5.0).is_err());
}

/// A tapered pocket-shaped tool (49.5 x 40 at its top, its sides leaning in by 30 degrees down to
/// 15 below) whose top lies in the face the rib stands on, 0.5 short of the rib. Pushed up out of
/// that plane, its leaning sides stay on their planes and carry it past its top's outline, into
/// the rib: the intersection used to keep a wedge of the rib above the tool (#423). It is the
/// tool, all of it below the plane, and the guard refuses the wedge.
#[test]
fn an_intersection_keeps_nothing_beyond_the_face_its_tool_was_pushed_out_of() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let target = ribbed();
    let plane = Plane::new(Vec3::new(0.0, 0.0, 20.0), Vec3::X, Vec3::Y).unwrap();
    let outline = Loop2::polygon(&[Vec2::new(40.0, 30.0), Vec2::new(89.5, 30.0), Vec2::new(89.5, 70.0), Vec2::new(40.0, 70.0)]);
    let (h, t) = (15.0, 30f64.to_radians().tan());
    let tool = extrude_tapered(&plane, &Region2 { outer: outline, holes: vec![] }, h, -1.0, -30f64.to_radians()).unwrap();
    let expected = 49.5 * 40.0 * h - (49.5 + 40.0) * t * h * h + 4.0 * t * t * h.powi(3) / 3.0;
    let r = crate::coplanar::boolean_apart(&target, &tool, BoolOp::Intersect).expect("the faces are pushed").unwrap().unwrap();
    let v = measure(&r).unwrap().volume;
    assert!((v - expected).abs() < 1e-2, "{v} against {expected}");
    assert_eq!(r.lumps().unwrap().len(), 1);
    assert!(r.validity().is_empty(), "{:?}", r.validity());
    let top = r.tessellate(0.05).unwrap().positions.iter().map(|p| p.z).fold(f64::MIN, f64::max);
    assert!(top < 20.0 + 1e-6, "the intersection reaches up to z = {top}");
    // What the pushed tool alone takes: the wedge of the rib too, which the guard refuses.
    let pushed = crate::coplanar::push_faces(&tool, Vec3::Z, 20.0, target.size().max(tool.size()) * 0.0229).unwrap();
    let wedged = boolean(&target, &pushed, BoolOp::Intersect).unwrap().unwrap();
    assert!(crate::boolean_guard::check_retry(&target, &tool, BoolOp::Intersect, Some(&wedged)).is_err());
    assert!(crate::boolean_guard::check_retry(&target, &tool, BoolOp::Intersect, Some(&r)).is_ok());
}
