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

/// A public-safe analogue of #3456, whose original design is unavailable.
fn drilled_round_taper() -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let disc = Region2 { outer: Loop2::circle(Vec2::ZERO, 20.), holes: vec![] };
    let mut target = extrude_tapered(&Plane::XY, &disc, 100., 1., 5_f64.to_radians()).unwrap();
    for x in [-8., 0., 8.] {
        for y in [-8., 0., 8.] {
            let bore = cylinder(Vec3::new(x, y, -1.), Vec3::Z, 2., 102.).unwrap();
            target = boolean(&target, &bore, BoolOp::Cut).unwrap().unwrap();
        }
    }
    target
}

fn round_taper_tool(radius: f64, hole_radius: f64, lo: f64, hi: f64) -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let centers = [Vec2::ZERO, Vec2::new(8., 0.), Vec2::new(-8., 0.), Vec2::new(0., 8.), Vec2::new(0., -8.)];
    let profile = Region2 {
        outer: Loop2::circle(Vec2::ZERO, radius),
        holes: centers.into_iter().map(|center| Loop2::circle(center, hole_radius).reversed()).collect(),
    };
    extrude(&Plane::XY, &[profile], lo, hi).unwrap().pop().unwrap()
}

fn taper_volume_to(z: f64) -> f64 {
    let slope = 5_f64.to_radians().tan();
    // Integral pi*(20 + slope*z)^2, less nine straight R2 bores.
    std::f64::consts::PI * (400. * z + 20. * slope * z * z + slope * slope * z * z * z / 3. - 36. * z)
}

/// A deep multi-loop cutter, coincident bores, and a conical rim. Numerical
/// empty candidates must not swallow the 2 mm base or force tool displacement.
#[test]
fn multi_loop_cut_of_a_drilled_cone_retains_the_base() {
    let target = drilled_round_taper();
    let radius = 20. + 100. * 5_f64.to_radians().tan();
    let tool = round_taper_tool(radius, 2., 2., 100.);
    assert!(crate::round_cap_cut::cut(&target, &tool).is_some(), "certify the complete current boundary");
    let (result, report) = capture_boolean_diagnostics(|| boolean(&target, &tool, BoolOp::Cut));
    let result = result.unwrap().unwrap();
    let mesh = result.tessellate_with(1e-6, true).unwrap();
    let expected = taper_volume_to(2.);
    assert!((mesh.measure().volume - expected).abs() < 0.03, "{} vs {expected}; {report:?}", mesh.measure().volume);
    assert!((mesh.bounds().max.z - 2.).abs() < 1e-7, "{:?}; {report:?}", mesh.bounds());
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert!(report.warning().is_none(), "{report:?}");
    result.history().unwrap().check(&result, &[&target, &tool]).unwrap();
    assert!(!mesh.contains(Vec3::new(0., 0., 1.)));
    assert!(mesh.contains(Vec3::new(15., 0., 1.)));
    assert_eq!(measure(&result).unwrap().merged.face_types.get("cone"), Some(&1));
    assert_eq!(measure(&result).unwrap().merged.face_types.get("cylinder"), Some(&9));
}

#[test]
fn round_cap_cut_preserves_two_sides_and_refuses_a_cutting_wall() {
    let target = drilled_round_taper();
    let radius = 20. + 100. * 5_f64.to_radians().tan();
    let tool = round_taper_tool(radius, 2., 25., 75.);
    let result = crate::round_cap_cut::cut(&target, &tool).unwrap().unwrap().unwrap();
    let expected = taper_volume_to(25.) + taper_volume_to(100.) - taper_volume_to(75.);
    let mesh = result.tessellate_with(1e-6, true).unwrap();
    assert!((mesh.measure().volume - expected).abs() < 0.2, "{} vs {expected}", mesh.measure().volume);
    assert_eq!(result.lumps().unwrap().len(), 2);
    assert!(!mesh.contains(Vec3::new(15., 0., 50.)));
    assert!(mesh.contains(Vec3::new(15., 0., 10.)));
    assert!(mesh.contains(Vec3::new(15., 0., 90.)));
    assert!(crate::round_cap_cut::cut(&target, &round_taper_tool(radius - 0.01, 2., 2., 100.)).is_none());
    assert!(crate::round_cap_cut::cut(&target, &round_taper_tool(radius, 2.01, 2., 100.)).is_none());
    let tool = round_taper_tool(radius, 2., 2., 100.);
    let moved = transform(&tool, Vec3::X * 0.01, Vec3::ZERO, Vec3::Z, 0.).unwrap();
    assert!(crate::round_cap_cut::cut(&target, &moved).is_none());
    let scaled = transform_matrix(&tool, [[0.9, 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]).unwrap();
    assert!(crate::round_cap_cut::cut(&target, &scaled).is_none(), "old extrusion metadata must not certify an elliptic tool");
}

#[test]
fn round_cap_cut_keeps_a_single_thin_bore_and_every_piece() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let frames = [Plane::XY, Plane::from_normal(Vec3::new(120., -40., 15.), Vec3::new(1., 2., 3.)).unwrap()];
    for frame in frames {
        for turn in [0., 0.39] {
            let region = Region2 {
                outer: Loop2::circle_from(Vec2::ZERO, 3.02, turn),
                holes: vec![Loop2::circle_from(Vec2::ZERO, 3., turn + 0.17).reversed()],
            };
            let target = crate::build::extrude_in_frame(&frame, &[region], 0., 9.65).unwrap().pop().unwrap();
            for (lo, hi, pieces, height) in [(6., 18., 1, 6.), (3., 6., 2, 6.65), (-1., 12., 0, 0.)] {
                let tool = crate::cylinder(frame.offset(lo).origin, frame.normal(), 10., hi - lo).unwrap();
                let result = crate::round_cap_cut::cut(&target, &tool).expect("certify the current thin tube").unwrap();
                if pieces == 0 {
                    assert!(result.is_none());
                    continue;
                }
                let body = result.unwrap();
                assert!(!body.is_mesh());
                assert!(body.validity().is_empty(), "{:?}", body.validity());
                assert_eq!(body.lumps().unwrap().len(), pieces);
                body.history().unwrap().check(&body, &[&target, &tool]).unwrap();
                let want = std::f64::consts::PI * (3.02f64.powi(2) - 9.) * height;
                let fine = body.tessellate_with(1e-5, true).unwrap().measure().volume;
                assert!((fine - want).abs() < 0.002 * want, "{turn}: {fine} vs {want}");
                let measured = measure(&body).unwrap().volume;
                assert!((measured - want).abs() < 0.05 * want, "{turn}: {measured} vs {want}");
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SurfaceCase {
    Flat,
    Convex,
    Concave,
}

const SURFACE_CASES: [SurfaceCase; 3] = [SurfaceCase::Flat, SurfaceCase::Convex, SurfaceCase::Concave];

fn section_solid(kind: SurfaceCase, radius: f64, center: f64, hole_center: f64, hole: f64, lo: f64, hi: f64) -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    if kind == SurfaceCase::Flat {
        return box_solid(Vec3::new(center - radius, -radius, lo), Vec3::new(center + radius, radius, hi)).unwrap();
    }
    let region = Region2 {
        outer: Loop2::circle(Vec2::new(center, 0.), radius),
        holes: if kind == SurfaceCase::Concave { vec![Loop2::circle(Vec2::new(hole_center, 0.), hole).reversed()] } else { vec![] },
    };
    extrude(&Plane::XY, &[region], lo, hi).unwrap().pop().unwrap()
}

fn section_area(kind: SurfaceCase, radius: f64, hole: f64) -> f64 {
    if kind == SurfaceCase::Flat {
        4. * radius * radius
    } else {
        std::f64::consts::PI * (radius * radius - if kind == SurfaceCase::Concave { hole * hole } else { 0. })
    }
}

fn section_inside(kind: SurfaceCase, r: f64, center: f64, hole_center: f64, hole: f64, lo: f64, hi: f64, p: Vec3) -> Option<bool> {
    let outer = if kind == SurfaceCase::Flat { r - (p.x - center).abs().max(p.y.abs()) } else { r - (p.x - center).hypot(p.y) };
    let inner = if kind == SurfaceCase::Concave { (p.x - hole_center).hypot(p.y) - hole } else { 1e10 };
    if [outer, inner, p.z - lo, hi - p.z].iter().any(|x| x.abs() < 1e-5) {
        return None;
    }
    Some(outer > 0. && inner > 0. && p.z > lo && p.z < hi)
}

fn circle_lens(r: f64, s: f64, d: f64) -> f64 {
    use std::f64::consts::PI;
    if d >= r + s {
        return 0.;
    }
    if d <= (r - s).abs() {
        return PI * r.min(s).powi(2);
    }
    let a = ((d * d + r * r - s * s) / (2. * d * r)).clamp(-1., 1.).acos();
    let b = ((d * d + s * s - r * r) / (2. * d * s)).clamp(-1., 1.).acos();
    r * r * a + s * s * b - 0.5 * ((-d + r + s) * (d + r - s) * (d - r + s) * (d + r + s)).sqrt()
}

/// Ordered flat/convex/concave permutations, not just one favorable operand order.
/// Full containment, internal voids, through bores, separated halves, and a bore
/// crossing an outer wall. Concave bore tools retain a disconnected central island.
#[test]
fn surface_permutation_boolean_matrix_preserves_material_and_all_lumps() {
    use std::f64::consts::PI;
    for a_kind in SURFACE_CASES {
        let a = section_solid(a_kind, 10., 0., 0., 4., 0., 10.);
        let area_a = section_area(a_kind, 10., 4.);
        for b_kind in SURFACE_CASES {
            for mode in ["contains_target", "internal_void", "through_bore", "separating_slab", "crossing_bore", "partial_cap_contact"] {
                let (r, center, hc, hole, lo, hi) = match mode {
                    "contains_target" => (20., 0., 16., 1., -1., 11.),
                    "internal_void" => (1., 7., 7., 0.3, 4., 6.),
                    "through_bore" => (1., 7., 7., 0.3, -1., 11.),
                    "separating_slab" => (20., 0., 16., 1., 2., 8.),
                    "partial_cap_contact" => (4., 9., 9., 0.7, 10., 14.),
                    _ => (4., 9., 9., 0.7, -1., 11.),
                };
                let b = section_solid(b_kind, r, center, hc, hole, lo, hi);
                let area_b = section_area(b_kind, r, hole);
                let overlap_area = match mode {
                    "contains_target" | "separating_slab" => area_a,
                    "internal_void" | "through_bore" => area_b,
                    _ => {
                        let outer = if b_kind == SurfaceCase::Flat {
                            if a_kind == SurfaceCase::Flat { 40. } else { 4. * 84_f64.sqrt() + 100. * 0.4_f64.asin() - 40. }
                        } else if a_kind == SurfaceCase::Flat {
                            // The right half of the R4 disc, x >= 10, lies outside the box.
                            let cap = 16. * (0.25_f64.acos()) - 15_f64.sqrt();
                            16. * PI - cap
                        } else {
                            circle_lens(10., 4., 9.)
                        };
                        outer - if b_kind == SurfaceCase::Concave { PI * hole * hole } else { 0. }
                    }
                };
                let va = area_a * 10.;
                let vb = area_b * (hi - lo);
                let vi = overlap_area * hi.min(10.) - overlap_area * lo.max(0.);
                for op in [BoolOp::Cut, BoolOp::Union, BoolOp::Intersect] {
                    let tag = format!("{a_kind:?}/{b_kind:?}/{mode}/{op:?}");
                    let (result, diagnostics) = capture_boolean_diagnostics(|| boolean(&a, &b, op));
                    let result = result.unwrap_or_else(|e| panic!("{tag}: {e}"));
                    let expected = match op {
                        BoolOp::Cut => va - vi,
                        BoolOp::Union => va + vb - vi,
                        BoolOp::Intersect => vi,
                    };
                    let expected_lumps = if expected == 0. {
                        0
                    } else if op == BoolOp::Cut {
                        match mode {
                            "contains_target" => 0,
                            "separating_slab" => 2,
                            "through_bore" | "crossing_bore" if b_kind == SurfaceCase::Concave => 2,
                            _ => 1,
                        }
                    } else {
                        1
                    };
                    if expected_lumps == 0 {
                        assert!(result.is_none(), "{tag}");
                        continue;
                    }
                    let result = result.unwrap_or_else(|| panic!("{tag}: lost every body"));
                    assert!(result.validity().is_empty(), "{tag}: {:?}", result.validity());
                    let lumps = result.lumps().unwrap_or_else(|e| panic!("{tag}: {e}"));
                    assert_eq!(lumps.len(), expected_lumps, "{tag}");
                    let mesh = result.tessellate(1e-4).unwrap();
                    assert!((mesh.measure().volume - expected).abs() < 0.2, "{tag}: {} vs {expected}", mesh.measure().volume);
                    let inside = mesh.inside_index();
                    for x in [-11., -5., 0., 7., 9., 11., 18., 25.] {
                        for y in [-11., -3., 0., 3., 11.] {
                            for z in [-0.5, 1., 5., 9., 10.5] {
                                let p = Vec3::new(x, y, z);
                                let (Some(ia), Some(ib)) =
                                    (section_inside(a_kind, 10., 0., 0., 4., 0., 10., p), section_inside(b_kind, r, center, hc, hole, lo, hi, p))
                                else {
                                    continue;
                                };
                                let want = match op {
                                    BoolOp::Cut => ia && !ib,
                                    BoolOp::Union => ia || ib,
                                    BoolOp::Intersect => ia && ib,
                                };
                                assert_eq!(inside.contains(p), want, "{tag}: {p:?}");
                            }
                        }
                    }
                    assert!(diagnostics.warning().is_none(), "{tag}: {diagnostics:?}");
                    if let Some(history) = result.history() {
                        history.check(&result, &[&a, &b]).unwrap();
                    }
                }
            }
        }
    }
}

fn circular_sector(inner: f64, outer: f64, lo: f64, hi: f64) -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    use std::f64::consts::FRAC_PI_2;
    let region = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(inner, 0.), b: Vec2::new(outer, 0.) },
                Seg2::Arc { center: Vec2::ZERO, radius: outer, start: 0., sweep: FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(0., outer), b: Vec2::new(0., inner) },
            ]
            .into_iter()
            .chain((inner > 0.).then_some(Seg2::Arc { center: Vec2::ZERO, radius: inner, start: FRAC_PI_2, sweep: -FRAC_PI_2 }))
            .collect(),
        },
        holes: vec![],
    };
    extrude(&Plane::XY, &[region], lo, hi).unwrap().pop().unwrap()
}

/// Partial coincidence both axially and circumferentially: convex/concave,
/// convex/convex and concave/concave walls, and the planar faces at their ends.
#[test]
fn partially_coincident_curved_sectors_work_in_both_operand_orders() {
    use std::f64::consts::PI;
    let tube = section_solid(SurfaceCase::Concave, 10., 0., 0., 4., 0., 10.);
    let pin = section_solid(SurfaceCase::Convex, 4., 0., 0., 0., 0., 10.);
    let plug = circular_sector(0., 4., 2., 8.);
    let ring = circular_sector(4., 8., 2., 8.);
    for (a, b, va, vb, vi) in
        [(&tube, &plug, 840. * PI, 24. * PI, 0.), (&pin, &plug, 160. * PI, 24. * PI, 24. * PI), (&tube, &ring, 840. * PI, 72. * PI, 72. * PI)]
    {
        for rotated in [false, true] {
            let place = |body: &Body| {
                if rotated { transform(body, Vec3::new(123.5, -47., 31.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.67).unwrap() } else { body.clone() }
            };
            let (a, b) = (place(a), place(b));
            for (a, b, va, vb) in [(&a, &b, va, vb), (&b, &a, vb, va)] {
                for op in [BoolOp::Cut, BoolOp::Union, BoolOp::Intersect] {
                    let expected = match op {
                        BoolOp::Cut => va - vi,
                        BoolOp::Union => va + vb - vi,
                        BoolOp::Intersect => vi,
                    };
                    let (result, diagnostics) = capture_boolean_diagnostics(|| boolean(a, b, op));
                    let result = result.unwrap_or_else(|e| panic!("{op:?} rotated={rotated}: {e}"));
                    if expected == 0. {
                        assert!(result.is_none());
                        continue;
                    }
                    let result = result.unwrap();
                    assert!(result.validity().is_empty(), "{:?}", result.validity());
                    assert_eq!(result.lumps().unwrap().len(), 1);
                    let got = result.tessellate_with(1e-5, true).unwrap().measure().volume;
                    assert!((got - expected).abs() < 0.1, "{op:?} rotated={rotated}: {got} vs {expected}");
                    assert!(diagnostics.warning().is_none(), "{diagnostics:?}");
                }
            }
        }
    }
}

/// A thin closed shell can have several boundary shells but still be one body.
/// Cutting through its equator leaves two bodies; an annular axial bore leaves
/// the main shell and two separate small caps. No cavity may be filled or dropped.
#[test]
fn thin_shells_and_internal_voids_keep_every_disconnected_piece() {
    use std::f64::consts::PI;
    let cube = box_solid(Vec3::new(-10., -10., -10.), Vec3::new(10., 10., 10.)).unwrap();
    let inner = box_solid(Vec3::new(-9.8, -9.8, -9.8), Vec3::new(9.8, 9.8, 9.8)).unwrap();
    let cube = boolean(&cube, &inner, BoolOp::Cut).unwrap().unwrap();
    let ball = sphere(Vec3::ZERO, 10.).unwrap();
    let inner = sphere(Vec3::ZERO, 9.8).unwrap();
    let ball = crate::guard("analytic thin spherical shell fixture", || {
        let mut inward = inner.deep_copy();
        inward.not();
        let shells = ball.solid.boundaries().iter().chain(inward.boundaries()).cloned().collect();
        let solid = crate::body::Solid::try_new(shells).map_err(|e| KernelError::Failed(e.to_string()))?;
        Body::new(solid)
    })
    .unwrap();
    let tube = section_solid(SurfaceCase::Concave, 10., 0., 0., 9.8, -10., 10.);
    let band = 4. * PI * (100. - 9.8_f64.powi(2));
    for (name, target, whole, removed) in [
        ("planar closed shell", &cube, 8000. - 19.6_f64.powi(3), 4. * (400. - 19.6_f64.powi(2))),
        ("spherical closed shell", &ball, 4. * PI / 3. * (1000. - 9.8_f64.powi(3)), band),
        ("open thin tube", &tube, 20. * PI * (100. - 9.8_f64.powi(2)), band),
    ] {
        for kind in SURFACE_CASES {
            let tool = section_solid(kind, 20., 0., 0., 1., -2., 2.);
            let result = boolean(target, &tool, BoolOp::Cut).unwrap_or_else(|e| panic!("{name}/{kind:?}: {e}")).unwrap();
            assert_eq!(result.lumps().unwrap().len(), 2, "{name}/{kind:?}");
            assert!(result.validity().is_empty(), "{name}: {:?}", result.validity());
            let mesh = result.tessellate_with(1e-5, true).unwrap();
            assert!((mesh.measure().volume - (whole - removed)).abs() < 0.2, "{name}/{kind:?}: {} vs {}", mesh.measure().volume, whole - removed);
            assert!(!mesh.contains(Vec3::ZERO), "{name}: internal void refilled");
            let read = step_import(&step_export(&[&result], "thin separated shell").unwrap()).unwrap();
            assert!(read.warnings.is_empty(), "{:?}", read.warnings);
            assert_eq!(read.bodies.len(), 2, "{name}: STEP lost a solid");
        }
    }
    // A through annular bore isolates the material in its central hole at both poles.
    let annulus = section_solid(SurfaceCase::Concave, 1., 0., 0., 0.3, -11., 11.);
    let result = boolean(&ball, &annulus, BoolOp::Cut).unwrap().unwrap();
    assert_eq!(result.lumps().unwrap().len(), 3);
    let removed_by_disc = |r: f64| 4. * PI / 3. * (1000. - (100. - r * r).powf(1.5) - 9.8_f64.powi(3) + (9.8_f64.powi(2) - r * r).powf(1.5));
    let expected = 4. * PI / 3. * (1000. - 9.8_f64.powi(3)) - removed_by_disc(1.) + removed_by_disc(0.3);
    let mesh = result.tessellate_with(1e-5, true).unwrap();
    assert!((mesh.measure().volume - expected).abs() < 0.2, "{} vs {expected}", mesh.measure().volume);
    assert!(mesh.contains(Vec3::new(0., 0., 9.9)));
    assert!(!mesh.contains(Vec3::new(0.6, 0., 9.9)));
    assert!(!mesh.contains(Vec3::ZERO));
}

/// The same certified geometry after placement/scaling, with either cap doing
/// the cut, a reversed extrusion, an interior slab, and complete removal.
#[test]
fn round_cap_cut_is_independent_of_placement_and_cut_direction() {
    let target = drilled_round_taper();
    let radius = 20. + 100. * 5_f64.to_radians().tan();
    for scale in [0.1, 1., 10.] {
        let place = |body: &Body| {
            let scaled = transform_matrix(body, [[scale, 0., 0., 0.], [0., scale, 0., 0.], [0., 0., scale, 0.], [0., 0., 0., 1.]]).unwrap();
            transform(&scaled, Vec3::new(13., -17., 23.), Vec3::ZERO, Vec3::new(1., 2., 3.), 2.17).unwrap()
        };
        let a = place(&target);
        for (lo, hi, expected, count) in [
            (-10., 25., taper_volume_to(100.) - taper_volume_to(25.), 1),
            (75., 110., taper_volume_to(75.), 1),
            (75., 25., taper_volume_to(25.) + taper_volume_to(100.) - taper_volume_to(75.), 2),
            (-10., 110., 0., 0),
        ] {
            // Slightly smaller excluded cylinders lie entirely within the old
            // bores, so only the caps remove material even without coincidence.
            let b = place(&round_taper_tool(radius + 0.01, 1.99, lo, hi));
            let result = crate::round_cap_cut::cut(&a, &b).expect("certified placed stock").unwrap();
            if count == 0 {
                assert!(result.is_none());
                continue;
            }
            let result = result.unwrap();
            assert_eq!(result.lumps().unwrap().len(), count);
            assert!(result.validity().is_empty(), "{:?}", result.validity());
            result.history().unwrap().check(&result, &[&a, &b]).unwrap();
            let volume = result.tessellate_with(1e-6 * scale, true).unwrap().measure().volume;
            assert!(
                (volume - expected * scale.powi(3)).abs() < 0.2 * scale.powi(3),
                "scale {scale} {lo}..{hi}: {volume} vs {}",
                expected * scale.powi(3)
            );
        }
    }
}

/// A tiny positive overlap, exact tangency, and a tiny real gap must remain
/// distinct. Both operand shapes and both curved orientations participate.
#[test]
fn near_tangent_walls_do_not_invent_or_erase_material() {
    for a_kind in SURFACE_CASES {
        let a = section_solid(a_kind, 2., 0., 0., 0.5, 0., 3.);
        let va = section_area(a_kind, 2., 0.5) * 3.;
        for b_kind in SURFACE_CASES {
            for gap in [-0.001, 0., 0.001] {
                let d = 4. + gap;
                let b = section_solid(b_kind, 2., d, d, 0.5, 0., 3.);
                let vb = section_area(b_kind, 2., 0.5) * 3.;
                let area = if gap >= 0. {
                    0.
                } else if a_kind == SurfaceCase::Flat && b_kind == SurfaceCase::Flat {
                    -4. * gap
                } else if a_kind == SurfaceCase::Flat || b_kind == SurfaceCase::Flat {
                    let x = 2. + gap;
                    4. * (x / 2.).acos() - x * (4. - x * x).sqrt()
                } else {
                    circle_lens(2., 2., d)
                };
                for op in [BoolOp::Cut, BoolOp::Union, BoolOp::Intersect] {
                    let tag = format!("{a_kind:?}/{b_kind:?}/gap {gap}/{op:?}");
                    let expected = match op {
                        BoolOp::Cut => va - 3. * area,
                        BoolOp::Union => va + vb - 3. * area,
                        BoolOp::Intersect => 3. * area,
                    };
                    let result = boolean(&a, &b, op).unwrap_or_else(|e| panic!("{tag}: {e}"));
                    if expected == 0. {
                        assert!(result.is_none(), "{tag}");
                        continue;
                    }
                    let result = result.unwrap_or_else(|| panic!("{tag}: lost positive material"));
                    assert!(result.validity().is_empty(), "{tag}: {:?}", result.validity());
                    let pieces = if op == BoolOp::Union && (gap > 0. || (gap == 0. && (a_kind != SurfaceCase::Flat || b_kind != SurfaceCase::Flat))) {
                        2
                    } else {
                        1
                    };
                    match result.lumps() {
                        Ok(lumps) => assert_eq!(lumps.len(), pieces, "{tag}"),
                        Err(error) if op == BoolOp::Union && gap == 0. && pieces == 2 => {
                            // Line-only contact is a non-manifold union. The
                            // splitter may conservatively refuse touching shell
                            // ownership; it must never discard either solid.
                            assert_eq!(result.shell_count(), 2, "{tag}");
                            assert!(error.to_string().contains("shell boundaries intersect or touch"), "{tag}: {error}");
                        }
                        Err(error) => panic!("{tag}: {error}"),
                    }
                    // The public reference mesher floors chord tolerance at
                    // size*1e-6, too coarse for the volume of this tiny lens.
                    // Use the independent display planner at the stated chord
                    // tolerance rather than relaxing the analytic expectation.
                    let mesh =
                        crate::guard("near contact reference mesh", || Ok(crate::tess::plan(&result.solid, 1e-7, None, None)?.0.evaluate())).unwrap();
                    let got = mesh.measure().volume;
                    assert!((got - expected).abs() < 1e-4, "{tag}: {got} vs {expected}");
                    if gap != 0. {
                        let in_overlap = gap < 0. && op != BoolOp::Cut;
                        assert_eq!(mesh.contains(Vec3::new(d / 2., 0., 1.5)), in_overlap, "{tag}: material in the tiny overlap/gap");
                    }
                }
            }
        }
    }
}

/// Cross the existing bore wall itself, rather than only the convex outside of
/// a body that happens to contain a hole. The tool's own hole also crosses that
/// concave wall, leaving a separate material island in a through cut.
#[test]
fn crossing_concave_bore_walls_preserves_the_crescent_and_island() {
    use std::f64::consts::PI;
    let a = section_solid(SurfaceCase::Concave, 10., 0., 0., 4., 0., 10.);
    let va = 840. * PI;
    for kind in SURFACE_CASES {
        let b = section_solid(kind, 2., 4., 4., 0.3, -1., 11.);
        let vb = 12. * section_area(kind, 2., 0.3);
        let in_void = if kind == SurfaceCase::Flat {
            2. * 12_f64.sqrt() - 8. + 8. * PI / 3.
        } else {
            circle_lens(4., 2., 4.) - if kind == SurfaceCase::Concave { circle_lens(4., 0.3, 4.) } else { 0. }
        };
        let vi = 10. * (section_area(kind, 2., 0.3) - in_void);
        for reversed in [false, true] {
            let (left, right, vl, vr) = if reversed { (&b, &a, vb, va) } else { (&a, &b, va, vb) };
            for op in [BoolOp::Cut, BoolOp::Union, BoolOp::Intersect] {
                let tag = format!("concave/{kind:?}/reversed {reversed}/{op:?}");
                let expected = match op {
                    BoolOp::Cut => vl - vi,
                    BoolOp::Union => vl + vr - vi,
                    BoolOp::Intersect => vi,
                };
                let result = boolean(left, right, op).unwrap_or_else(|e| panic!("{tag}: {e}")).unwrap();
                assert!(result.validity().is_empty(), "{tag}: {:?}", result.validity());
                let count = if op == BoolOp::Cut && !reversed && kind == SurfaceCase::Concave { 2 } else { 1 };
                assert_eq!(result.lumps().unwrap().len(), count, "{tag}");
                let mesh = result.tessellate_with(1e-5, true).unwrap();
                assert!((mesh.measure().volume - expected).abs() < 0.03, "{tag}: {} vs {expected}", mesh.measure().volume);
                for x in [2.5, 3.5, 4.1, 4.7, 5.5, 8.] {
                    for y in [0., 0.4, 1.5] {
                        for z in [-0.5, 5., 10.5] {
                            let p = Vec3::new(x, y, z);
                            let (Some(ia), Some(ib)) = (
                                section_inside(SurfaceCase::Concave, 10., 0., 0., 4., 0., 10., p),
                                section_inside(kind, 2., 4., 4., 0.3, -1., 11., p),
                            ) else {
                                continue;
                            };
                            let (il, ir) = if reversed { (ib, ia) } else { (ia, ib) };
                            let want = match op {
                                BoolOp::Cut => il && !ir,
                                BoolOp::Union => il || ir,
                                BoolOp::Intersect => il && ir,
                            };
                            assert_eq!(mesh.contains(p), want, "{tag}: {p:?}");
                        }
                    }
                }
            }
        }
    }
}
