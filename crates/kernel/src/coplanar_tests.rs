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

/// A wall joined round a part, flush with its sides: a ring whose hole is the part's outline,
/// rounded corners and all (a box's walls round its gridded floor), standing on the part's
/// bottom or on a level below it. The bodies only touch, back to back on planes and cylinders.
mod wall_join {
    use std::f64::consts::{FRAC_PI_2, PI};

    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

    use crate::*;

    /// The rectangle from `lo` to `hi` with its corners rounded to `r`, counter-clockwise.
    fn rounded(lo: Vec2, hi: Vec2, r: f64) -> Loop2 {
        let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
        let arc = |center: Vec2, start: f64| Seg2::Arc { center, radius: r, start, sweep: FRAC_PI_2 };
        Loop2 {
            segs: vec![
                line(Vec2::new(lo.x + r, lo.y), Vec2::new(hi.x - r, lo.y)),
                arc(Vec2::new(hi.x - r, lo.y + r), -FRAC_PI_2),
                line(Vec2::new(hi.x, lo.y + r), Vec2::new(hi.x, hi.y - r)),
                arc(Vec2::new(hi.x - r, hi.y - r), 0.0),
                line(Vec2::new(hi.x - r, hi.y), Vec2::new(lo.x + r, hi.y)),
                arc(Vec2::new(lo.x + r, hi.y - r), FRAC_PI_2),
                line(Vec2::new(lo.x, hi.y - r), Vec2::new(lo.x, lo.y + r)),
                arc(Vec2::new(lo.x + r, lo.y + r), PI),
            ],
        }
    }

    /// The area of [`rounded`].
    fn rounded_area(lo: Vec2, hi: Vec2, r: f64) -> f64 {
        (hi.x - lo.x) * (hi.y - lo.y) - (4.0 - PI) * r * r
    }

    const LO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    const HI: Vec2 = Vec2 { x: 60.0, y: 50.0 };

    /// A 60 × 50 plate 3.25 thick with r4 corners, a pocket with sloping sides in its top and a
    /// socket under it (so that it is no prism, and nothing below its bottom is clear).
    fn part() -> Body {
        let plate = extrude(&Plane::XY, &[Region2 { outer: rounded(LO, HI, 4.0), holes: vec![] }], 0.0, 3.25).unwrap().pop().unwrap();
        let pocket = Region2 { outer: rounded(Vec2::new(10.0, 10.0), Vec2::new(50.0, 40.0), 3.0), holes: vec![] };
        let top = Plane::from_normal(Vec3::new(0.0, 0.0, 4.0), Vec3::Z).unwrap();
        let pocket = extrude_tapered(&top, &pocket, 3.0, -1.0, -0.3).unwrap();
        let plate = boolean(&plate, &pocket, BoolOp::Cut).unwrap().unwrap();
        let socket = box_solid(Vec3::new(12.0, 12.0, -4.75), Vec3::new(48.0, 38.0, 0.0)).unwrap();
        boolean(&plate, &socket, BoolOp::Union).unwrap().unwrap()
    }

    /// The 10 mm wall round the part, r14 outside, from `z0` to `z1`.
    fn wall(z0: f64, z1: f64) -> (Body, f64) {
        let (outer, inner) = ((Vec2::new(-10.0, -10.0), Vec2::new(70.0, 60.0), 14.0), (LO, HI, 4.0));
        let ring = Region2 { outer: rounded(outer.0, outer.1, outer.2), holes: vec![rounded(inner.0, inner.1, inner.2).reversed()] };
        let area = rounded_area(outer.0, outer.1, outer.2) - rounded_area(inner.0, inner.1, inner.2);
        (extrude(&Plane::XY, &[ring], z0, z1).unwrap().pop().unwrap(), area * (z1 - z0))
    }

    /// From the part's bottom, and from the socket's floor past its top: joined exactly (glued where
    /// they touch, no tool nudged), one solid of both volumes, with no face left where they touched.
    #[test]
    fn a_wall_joins_flush_round_a_part_with_rounded_corners() {
        let part = part();
        let v_part = measure(&part).unwrap().volume;
        for (z0, z1) in [(0.0, 40.0), (-4.75, 5.25)] {
            let (wall, v_wall) = wall(z0, z1);
            for (a, b) in [(&part, &wall), (&wall, &part)] {
                let (r, d) = capture_boolean_diagnostics(|| boolean(a, b, BoolOp::Union));
                let r = r.unwrap().unwrap();
                assert!(!d.events.iter().any(|e| e.strategy.alters_result()), "{z0}..{z1}: an exact join: {d:?}");
                assert_eq!(r.shell_count(), 1, "{z0}..{z1}");
                assert!(r.validity().is_empty(), "{z0}..{z1}: {:?}", r.validity());
                let v = measure(&r).unwrap().volume;
                // Measured to 2e-5 where faces are curved (as the drain pads are).
                assert!((v - v_part - v_wall).abs() < 2e-5 * (v_part + v_wall), "{z0}..{z1}: {v} vs {}", v_part + v_wall);
                let size = a.size().max(b.size());
                assert!(crate::coplanar::slit_free(&r, a, b, size * 1e-4), "{z0}..{z1}: a face left where the bodies touched");
                lineage_holds(&r, [a, b]);
            }
        }
    }

    /// Do two face surfaces coincide (planes facing the same way, cylinders, cones)? `None` for
    /// surfaces this doesn't compare.
    fn same_surface(x: &FaceSurface, y: &FaceSurface) -> Option<bool> {
        let tol = 1e-6;
        Some(match (x, y) {
            (FaceSurface::Plane { normal: n, point: p }, FaceSurface::Plane { normal: m, point: q }) => {
                n.dot(**m) > 1.0 - 1e-9 && n.dot(*q - *p).abs() < tol
            }
            (FaceSurface::Cylinder { origin: o, axis: a, radius: r }, FaceSurface::Cylinder { origin: p, axis: b, radius: s }) => {
                let off = *p - *o;
                a.dot(**b).abs() > 1.0 - 1e-9 && (r - s).abs() < tol && (off - **a * off.dot(**a)).len() < tol
            }
            (FaceSurface::Cone { apex: v, axis: a, half_angle: h }, FaceSurface::Cone { apex: w, axis: b, half_angle: k }) => {
                v.dist(*w) < tol && a.dot(**b) > 1.0 - 1e-9 && (h - k).abs() < 1e-9
            }
            _ => return None,
        })
    }

    /// Every face of `r` that its history says comes from a face of an operand lies on that face's
    /// surface: the history counts each step once.
    fn lineage_holds(r: &Body, operands: [&Body; 2]) {
        let h = r.history().expect("the join's history");
        assert_eq!(h.faces.len(), r.face_count());
        for (i, f) in h.faces.iter().enumerate() {
            let Some(here) = face_surface(r, i) else { continue };
            for o in &f.origins {
                if let crate::history::FaceOrigin::Face { operand, face } = o
                    && let Some(there) = operands.get(*operand).and_then(|b| face_surface(b, *face))
                {
                    assert_ne!(same_surface(&here, &there), Some(false), "face {i} ({here:?}) is said to come from {o:?} ({there:?})");
                }
            }
        }
    }

    /// A pin standing on a base, through a plate's hole, the plate's top edges chamfered (no
    /// prism); the base as wide as the plate or wider. Glued, the round faces must go with the planar
    /// ones: one of them left behind is a slit, which the checks on a glued pair of operands don't see.
    #[test]
    fn a_pin_through_a_hole_with_its_base_under_the_plate_has_no_slit() {
        let hole = cylinder(Vec3::new(20.0, 20.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
        let plate = boolean(&box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 10.0)).unwrap(), &hole, BoolOp::Cut).unwrap().unwrap();
        let tops = [Vec3::new(20.0, 0.0, 10.0), Vec3::new(40.0, 20.0, 10.0), Vec3::new(20.0, 40.0, 10.0), Vec3::new(0.0, 20.0, 10.0)];
        let plate = chamfer(&plate, &tops, 2.0).unwrap();
        let pin = cylinder(Vec3::new(20.0, 20.0, 0.0), Vec3::Z, 5.0, 10.0).unwrap();
        for (lo, hi) in [(-10.0, 50.0), (0.0, 40.0)] {
            let base = box_solid(Vec3::new(lo, lo, -5.0), Vec3::new(hi, hi, 0.0)).unwrap();
            let post = boolean(&base, &pin, BoolOp::Union).unwrap().unwrap();
            let want = measure(&plate).unwrap().volume + measure(&post).unwrap().volume;
            for (a, b) in [(&plate, &post), (&post, &plate)] {
                let r = boolean(a, b, BoolOp::Union).unwrap().unwrap();
                assert_eq!(r.shell_count(), 1, "base {lo}..{hi}");
                assert!(r.validity().is_empty(), "base {lo}..{hi}: {:?}", r.validity());
                let v = measure(&r).unwrap().volume;
                assert!((v - want).abs() < 2e-5 * want, "base {lo}..{hi}: {v} vs {want}");
                assert!(crate::coplanar::slit_free(&r, a, b, a.size().max(b.size()) * 1e-4), "base {lo}..{hi}: a slit where the bodies touched");
            }
        }
    }

    /// A bracket standing on a large plate, its arm reaching back over the plate 0.4 mm above it: a
    /// real gap in front of a face the bodies touch on is no slit.
    #[test]
    fn a_narrow_gap_over_a_contact_face_is_no_slit() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(300.0, 300.0, 10.0)).unwrap();
        let foot = box_solid(Vec3::new(100.0, 100.0, 10.0), Vec3::new(120.0, 120.0, 30.0)).unwrap();
        let arm = box_solid(Vec3::new(20.0, 100.0, 10.4), Vec3::new(110.0, 120.0, 12.0)).unwrap();
        let bracket = boolean(&foot, &arm, BoolOp::Union).unwrap().unwrap();
        let r = boolean(&plate, &bracket, BoolOp::Union).unwrap().unwrap();
        assert!(r.validity().is_empty(), "{:?}", r.validity());
        let size = plate.size().max(bracket.size());
        assert!(crate::coplanar::slit_free(&r, &plate, &bracket, size * 5e-4));
    }

    /// A contact far smaller than the face it lies on (a tall wall beside a thin plate) is seen
    /// whichever body is asked about first: the wall's face meshes into triangles none of whose
    /// samples fall on the plate.
    #[test]
    fn a_contact_much_smaller_than_its_face_is_seen() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 1.0)).unwrap();
        let wall = box_solid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(12.0, 10.0, 40.0)).unwrap();
        assert!(crate::coplanar::faces_touch(&plate, &wall));
        assert!(crate::coplanar::faces_touch(&wall, &plate));
        let apart = box_solid(Vec3::new(10.5, 0.0, 0.0), Vec3::new(12.0, 10.0, 40.0)).unwrap();
        assert!(!crate::coplanar::faces_touch(&plate, &apart));
    }

    /// Two blocks side by side in one body, not fused, have a slit where they touch; the same blocks
    /// joined have none.
    #[test]
    fn a_face_left_where_bodies_touched_is_a_slit() {
        let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let b = box_solid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)).unwrap();
        let shells: Vec<_> = a.solid.boundaries().iter().chain(b.solid.boundaries()).cloned().collect();
        let side_by_side = Body::new(crate::body::Solid::try_new(shells).unwrap()).unwrap();
        assert!(!crate::coplanar::slit_free(&side_by_side, &a, &b, 0.01));
        let joined = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
        assert!(crate::coplanar::slit_free(&joined, &a, &b, 0.01));
        // Faces on one side of a contact surface only, however finely meshed, are no slit.
        let wide = cylinder(Vec3::ZERO, Vec3::Z, 50.0, 10.0).unwrap();
        let bore = Region2 { outer: Loop2::circle(Vec2::ZERO, 60.0), holes: vec![Loop2::circle(Vec2::ZERO, 50.0).reversed()] };
        let tall = extrude(&Plane::XY, &[bore], 0.0, 20.0).unwrap().pop().unwrap();
        assert!(crate::coplanar::slit_free(&tall, &wide, &tall, 1e-6));
        // A disc filling a ring's bore, the two side by side: a slit on their common cylinder.
        let disc = cylinder(Vec3::ZERO, Vec3::Z, 5.0, 10.0).unwrap();
        let bore = Region2 { outer: Loop2::circle(Vec2::ZERO, 12.0), holes: vec![Loop2::circle(Vec2::ZERO, 5.0).reversed()] };
        let ring = extrude(&Plane::XY, &[bore], 0.0, 10.0).unwrap().pop().unwrap();
        let shells: Vec<_> = disc.solid.boundaries().iter().chain(ring.solid.boundaries()).cloned().collect();
        let filled = Body::new(crate::body::Solid::try_new(shells).unwrap()).unwrap();
        assert!(!crate::coplanar::slit_free(&filled, &disc, &ring, 0.01));
    }

    /// The faces of `body` on the cylinder about Z through `c` of radius `r`, split by the plane
    /// z = `level` along exact arcs of its circle, the pieces covering what the faces did.
    fn splits_on_its_circle(body: &Body, c: Vec3, r: f64, level: f64) {
        let on = |s: &Option<FaceSurface>| match s {
            Some(FaceSurface::Cylinder { origin, axis, radius }) => {
                let off = c - *origin;
                (radius - r).abs() < 1e-6 && axis.dot(Vec3::Z).abs() > 1.0 - 1e-9 && (off - Vec3::Z * off.z).len() < 1e-6
            }
            _ => false,
        };
        let sides: Vec<usize> = (0..body.face_count()).filter(|i| on(&face_surface(body, *i))).collect();
        assert!(!sides.is_empty());
        let plane = Plane::from_normal(Vec3::new(0.0, 0.0, level), Vec3::Z).unwrap();
        let (split, _) = crate::splitface::split_across(body, &sides, plane, (c, Vec3::Z, r)).unwrap();
        assert!(split.validity().is_empty(), "{:?}", split.validity());
        let (before, after) = (measure(body).unwrap(), measure(&split).unwrap());
        assert!((before.area - after.area).abs() < 1e-6 * before.area, "{} vs {}", after.area, before.area);
        use truck_modeling::{BoundedCurve, ParametricCurve};
        let at = |p: truck_modeling::Point3| crate::body::from_p3(p);
        let off = |p: Vec3| Vec3::new(p.x - c.x, p.y - c.y, 0.0).len();
        let cuts: Vec<_> = split
            .solid
            .edge_iter()
            .filter(|e| [e.front().point(), e.back().point()].iter().all(|p| (at(*p).z - level).abs() < 1e-9 && (off(at(*p)) - r).abs() < 1e-6))
            .collect();
        assert!(!cuts.is_empty());
        for e in cuts {
            let curve = e.curve();
            let (t0, t1) = curve.range_tuple();
            for k in 0..=16 {
                let p = at(curve.subs(t0 + (t1 - t0) * f64::from(k) / 16.0));
                assert!((off(p) - r).abs() < 1e-9 && (p.z - level).abs() < 1e-9, "{p:?} is {} off the circle", off(p) - r);
            }
        }
    }

    /// A plane square to a round side's axis splits it along an exact arc of its circle, where a
    /// curve fitted to the face's mesh would stray by the mesh's sag: half turns, a small round side
    /// on a large body (whose traced chain keeps only its ends), and a revolved one.
    #[test]
    fn a_round_side_is_split_across_its_axis_on_its_circle() {
        let c = Vec3::new(5.0, -3.0, 0.0);
        splits_on_its_circle(&cylinder(c, Vec3::Z, 7.0, 12.0).unwrap(), c, 7.0, 4.5);
        let plate = box_solid(Vec3::ZERO, Vec3::new(300.0, 300.0, 10.0)).unwrap();
        let boss = cylinder(Vec3::new(150.0, 150.0, 10.0), Vec3::Z, 1.0, 5.0).unwrap();
        let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
        splits_on_its_circle(&bossed, Vec3::new(150.0, 150.0, 0.0), 1.0, 12.5);
        // Three quarters of a turn on one face, a 1 mm boss on a 300 mm plate.
        let o = Vec2::new(150.0, 150.0);
        let pie = Loop2 {
            segs: vec![
                Seg2::Line { a: o, b: Vec2::new(151.0, 150.0) },
                Seg2::Arc { center: o, radius: 1.0, start: 0.0, sweep: 1.5 * PI },
                Seg2::Line { a: Vec2::new(150.0, 149.0), b: o },
            ],
        };
        let key = extrude(&Plane::XY, &[Region2 { outer: pie.clone(), holes: vec![] }], 10.0, 15.0).unwrap().pop().unwrap();
        let keyed = boolean(&plate, &key, BoolOp::Union).unwrap().unwrap();
        splits_on_its_circle(&keyed, Vec3::new(150.0, 150.0, 0.0), 1.0, 12.5);
        // The same key taller, its first or last quarter turn cut away above 20: split at 22 it is
        // half a turn, on the side away from the notch, though its edges below reach the other side.
        let key = extrude(&Plane::XY, &[Region2 { outer: pie.clone(), holes: vec![] }], 10.0, 25.0).unwrap().pop().unwrap();
        let tall = boolean(&plate, &key, BoolOp::Union).unwrap().unwrap();
        for (lo, hi) in
            [(Vec3::new(150.0, 150.0, 20.0), Vec3::new(152.0, 152.0, 26.0)), (Vec3::new(148.0, 148.0, 20.0), Vec3::new(150.0, 150.0, 26.0))]
        {
            let notched = boolean(&tall, &box_solid(lo, hi).unwrap(), BoolOp::Cut).unwrap().unwrap();
            splits_on_its_circle(&notched, Vec3::new(150.0, 150.0, 0.0), 1.0, 22.0);
        }
        // A small hole drilled into the key's round side at 60 degrees: split through it, the face
        // reaches the other side at that very level.
        let dir = Vec3::new(0.5, 0.75f64.sqrt(), 0.0);
        let drilled = boolean(&tall, &cylinder(Vec3::new(150.0, 150.0, 17.5), dir, 0.2, 3.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
        splits_on_its_circle(&drilled, Vec3::new(150.0, 150.0, 0.0), 1.0, 17.5);
        let section = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
        let ring =
            Region2 { outer: Loop2::polygon(&[Vec2::new(3.0, 0.0), Vec2::new(7.0, 0.0), Vec2::new(7.0, 12.0), Vec2::new(3.0, 12.0)]), holes: vec![] };
        let tube = revolve(&section, &[ring], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 2.0 * PI).unwrap().pop().unwrap();
        splits_on_its_circle(&tube, Vec3::ZERO, 7.0, 4.5);
    }

    /// An arc counts as on a round face where it runs over the face, and not where it runs the
    /// other way round the circle: of a half turn between the face's side edges, every point or none.
    #[test]
    fn an_arc_is_on_a_round_face_only_the_way_the_face_runs() {
        let (c, r) = (Vec3::new(5.0, -3.0, 0.0), 7.0);
        let body = cylinder(c, Vec3::Z, r, 12.0).unwrap();
        let side = (0..body.face_count()).find(|i| matches!(face_surface(&body, *i), Some(FaceSurface::Cylinder { .. }))).unwrap();
        let face = body.solid.face_iter().nth(side).unwrap();
        let off_axis = |p: Vec3| Vec3::new(p.x - c.x, p.y - c.y, 0.0).normalized().unwrap();
        let mut ends: Vec<Vec3> = face.vertex_iter().map(|v| off_axis(crate::body::from_p3(v.point()))).collect();
        ends.dedup_by(|x, y| x.dist(*y) < 1e-9);
        let (ua, ub) = (ends[0], *ends.iter().find(|u| u.dist(ends[0]) > 1e-6).unwrap());
        let middle = Vec3::Z.cross(ua);
        let meshed = crate::splitface::face_triangles(face, r * 1e-2);
        let centre = Vec3::new(c.x, c.y, 6.0);
        let mut counts = [middle, -middle].map(|m| crate::splitface::arc_on_face(&meshed, (centre, r), [ua, m, ub]));
        counts.sort_unstable();
        assert_eq!(counts, [0, 2 * crate::splitface::ARC_STEPS - 1], "{ua:?} {ub:?}");
    }
}
