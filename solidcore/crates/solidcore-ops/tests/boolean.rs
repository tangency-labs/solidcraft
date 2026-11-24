//! The boolean family on bodies whose answers are known.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

mod common;

use std::f64::consts::{PI, TAU};
use std::io::Read as _;

use serde_json::Value;
use solidcore::topology::vertex::corner;
use solidcore_ops::boolean::{boolean, imprint, union_all, union_copies};
use solidcore_ops::prism::extrude_planar_regions;
use solidcore_ops::sweep::{extrude, join_arcs, revolve, zone_tolerance, Curve, DraftCorner, Join};
use solidcore_ops::{
    blend, make_box, make_cone, make_sphere, session, Blend, Body, BooleanOp, CheckLevel, Corners,
    Error, PointClass,
};
use solidcore_record::brep::Brep as Record;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-7 * b.abs().max(1.0)
}

#[test]
fn two_overlapping_boxes() {
    all_fixes();
    let a = || make_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap();
    let b = || make_box([1.0, 1.0, 1.0], [2.0, 2.0, 2.0]).unwrap();
    let u = boolean(a(), b(), BooleanOp::Union).unwrap().unwrap();
    assert!(near(volume(&u), 15.0), "{}", volume(&u));
    assert!(u.is_manifold_solid().unwrap());
    let d = boolean(a(), b(), BooleanOp::Subtract).unwrap().unwrap();
    assert!(near(volume(&d), 7.0), "{}", volume(&d));
    let i = boolean(a(), b(), BooleanOp::Intersect).unwrap().unwrap();
    assert!(near(volume(&i), 1.0), "{}", volume(&i));
}

/// The public repro in solidcraft#20408 joins two swept parts along their matching collars.
#[test]
fn a_union_of_overlapping_collars_keeps_both_input_bodies() {
    all_fixes();
    let a = Body::from_smb(include_bytes!("data/issue_20408_hoop.smb").to_vec(), false).unwrap();
    let b = Body::from_smb(include_bytes!("data/issue_20408_back.smb").to_vec(), false).unwrap();
    let before = [volume(&a), volume(&b)];
    let joined = union_copies(&a, &b).unwrap().unwrap();
    assert_eq!(joined.solid_count().unwrap(), 1);
    assert!(joined.check(CheckLevel::Geometry).unwrap().is_valid());
    // The operands overlap in their coaxial 1.3 mm-radius collars over exactly 2 mm.
    let overlap = PI * 1.3 * 1.3 * 2.0;
    assert!(near(volume(&joined), before[0] + before[1] - overlap));
    assert_eq!(joined.counts().unwrap(), [25, 70, 46]);
    // The outside of the surviving collar at x=4.75 + 1.3 is 0.1 mm from the probe.
    let (_, order) = joined.to_record().unwrap();
    let faces: Vec<usize> = (0..25).collect();
    let closest = joined
        .closest_to_point(&order, &faces, &[], [6.15, 15.25, 18.375])
        .unwrap()
        .unwrap();
    assert!(near(closest.distance, 0.1));
    assert!(near(volume(&a), before[0]));
    assert!(near(volume(&b), before[1]));
}

#[test]
fn apart_boxes_intersect_in_nothing() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    assert!(boolean(a, b, BooleanOp::Intersect).unwrap().is_none());
}

#[test]
fn a_list_unions_level_by_level() {
    all_fixes();
    let boxes = (0..5)
        .map(|i| make_box([f64::from(i), 0.0, 0.0], [1.5, 1.0, 1.0]).unwrap())
        .collect();
    let u = union_all(boxes).unwrap();
    assert!(near(volume(&u), 5.5), "{}", volume(&u));
    assert_eq!(union_all(Vec::new()).unwrap_err(), Error::Argument);
}

#[test]
fn boxes_touching_along_an_edge_union_to_a_closed_body() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([1.0, 1.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
    assert!(near(volume(&u), 2.0), "{}", volume(&u));
    assert_eq!(u.solid_count().unwrap(), 2);
}

/// A 20 mm box with a closed void at its middle: a 10 mm cube void, or a sphere void of radius 5.
fn box_with_void(cube: bool) -> Body {
    let tool = if cube {
        make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap()
    } else {
        make_sphere([10.0, 10.0, 10.0], 5.0).unwrap()
    };
    let b = make_box([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]).unwrap();
    boolean(b, tool, BooleanOp::Subtract).unwrap().unwrap()
}

#[test]
fn a_union_that_fills_a_void_is_the_whole_box() {
    all_fixes();
    // Every face of the void is deleted, and the region they joined is material. The tool's
    // faces lie on the void's, or inside the box's material, so none of them is copied in.
    let fillers = [
        (true, make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap()),
        (true, make_box([4.0, 4.0, 4.0], [12.0, 12.0, 12.0]).unwrap()),
        (true, make_box([5.0, 4.0, 4.0], [12.0, 12.0, 12.0]).unwrap()),
        (false, make_sphere([10.0, 10.0, 10.0], 5.0).unwrap()),
        (false, make_sphere([10.0, 10.0, 10.0], 6.0).unwrap()),
        (
            false,
            make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap(),
        ),
    ];
    for (n, (cube, filler)) in fillers.into_iter().enumerate() {
        let u = boolean(box_with_void(cube), filler, BooleanOp::Union)
            .unwrap()
            .unwrap();
        assert!(near(volume(&u), 8000.0), "filler {n}: {}", volume(&u));
        assert_eq!(u.counts().unwrap(), [6, 12, 8], "filler {n}");
        assert!(u.is_manifold_solid().unwrap(), "filler {n}");
        assert!(u.to_record().is_ok(), "filler {n}");
    }
    // A filler that leaves part of the void keeps that part: 20³ − 10 · 10 · 1.
    let part = make_box([6.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
    let u = boolean(box_with_void(true), part, BooleanOp::Union)
        .unwrap()
        .unwrap();
    assert!(near(volume(&u), 7900.0), "{}", volume(&u));
}

/// A cylinder cavity cut into a box whose side touches the box's y = 5 wall along a line (and
/// the x = 25 wall too, in the second cut), inside the box from z = 22.5 to 32.5. The contact
/// line leaves a slit loop in the wall, whose removal splits the box's shell from the cavity's.
/// The cavity's shell lies inside the box's, so both bound the one solid: its outer shell and
/// a void, as when the cavity is clear of the wall. The record has the exterior, the material
/// and the cavity, and two shells.
#[test]
fn a_cavity_touching_the_outside_along_a_line_is_one_solid_with_a_void() {
    for fixes in [false, true] {
        if fixes {
            all_fixes();
        }
        for base in [[30.0, 7.5, 22.5], [27.5, 7.5, 22.5]] {
            let b = make_box([25.0, 5.0, 12.5], [20.0, 30.0, 22.5]).unwrap();
            let cavity = solidcore_ops::make_cylinder(base, 2.5, 10.0).unwrap();
            let d = boolean(b, cavity, BooleanOp::Subtract).unwrap().unwrap();
            let expected = 20.0 * 30.0 * 22.5 - PI * 2.5 * 2.5 * 10.0;
            assert!(
                near(volume(&d), expected),
                "{base:?} fixes {fixes}: {}",
                volume(&d)
            );
            assert_eq!(d.solid_count().unwrap(), 1, "{base:?} fixes {fixes}");
            let (record, _) = d.to_record().unwrap();
            assert_eq!(
                (record.regions.len(), record.shells.len()),
                (3, 2),
                "{base:?} fixes {fixes}"
            );
        }
    }
}

#[test]
fn a_cube_on_a_void_is_kept_out_by_a_difference_and_shares_nothing_with_it() {
    all_fixes();
    let cube = || make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
    // The faces bound material on opposite sides: a difference keeps them, so the box is
    // unchanged, and an intersection deletes them and leaves nothing.
    let d = boolean(box_with_void(true), cube(), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    assert!(near(volume(&d), 7000.0), "{}", volume(&d));
    assert_eq!(d.counts().unwrap(), [12, 24, 16]);
    assert!(boolean(box_with_void(true), cube(), BooleanOp::Intersect)
        .unwrap()
        .is_none());
}

/// A cylinder lying along a box's edge, tangent there to the box's face. The two faces leave the
/// edge together and part as the square of the distance over the cylinder's diameter, by less
/// the larger the cylinder, so the fixed steps off the edge took a large one's faces for
/// parallel (a zero sector, from 10 times the size) and for one surface (coincident faces,
/// from 100 times). The union is the two solids touching along that line, at 1 to 1000 times
/// the size, in either order.
#[test]
fn a_large_cylinder_resting_on_a_box_edge_unions() {
    all_fixes();
    for k in [1.0, 10.0, 40.0, 100.0, 1000.0] {
        let cylinder =
            || solidcore_ops::make_cylinder([0.0, 15.0 * k, 2.5 * k], 7.5 * k, 25.0 * k).unwrap();
        let block = || make_box([0.0, 0.0, 0.0], [5.0 * k, 7.5 * k, 5.0 * k]).unwrap();
        let want = (PI * 7.5 * 7.5 * 25.0 + 5.0 * 7.5 * 5.0) * (k * k * k);
        for (a, b) in [(cylinder(), block()), (block(), cylinder())] {
            let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
            let report = u.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{k}: {}", report.text);
            assert_eq!(u.solid_count().unwrap(), 2, "{k}");
            assert!(
                near(volume(&u), want),
                "{k}: {}, closed form {want}",
                volume(&u)
            );
            u.to_record().unwrap();
        }
    }
}

/// A cylinder r 9 lying along x on a box's top face (z = 0), sunk `d` into it, joined. Where
/// the end caps (x = ±15) meet the top face, the line x = ±15, z = 0 touches each cap's
/// circle at the circle's vertex, a hair inside it. The sector test at that vertex read the
/// line inside the cap from secants a tolerance long, while the samples were still within
/// tolerance of each other, so the line was kept as two edges across the cap and the union
/// failed (status 1001) for d from about 7e-7 to 1e-5. Up to the contact tolerance, at it
/// too, the two solids touch along the line; past it (2e-5) they are one solid less a sliver
/// of (4/3)·√(2r)·d^1.5 per unit length (1.5e-5 mm³ here, inside the volume check's 2.4e-4).
#[test]
fn a_cylinder_sunk_a_hair_into_a_flat_face_joins_it() {
    let (r, len) = (9.0, 30.0);
    let whole = 40.0 * 40.0 * 10.0 + PI * r * r * len;
    for fixes in [false, true] {
        if fixes {
            all_fixes();
        }
        for (d, solids) in [(1e-6, 2), (5e-6, 2), (1e-5, 2), (2e-5, 1)] {
            let b = make_box([-20.0, -20.0, -10.0], [40.0, 40.0, 10.0]).unwrap();
            let c = solidcore_ops::make_cylinder([0.0; 3], r, len).unwrap();
            // rows: the images of x, y, z; row 3 the translation (p' = p·A + t)
            c.transform(&[
                [0.0, 0.0, -1.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [-15.0, 0.0, r - d, 1.0],
            ])
            .unwrap();
            let u = boolean(b, c, BooleanOp::Union)
                .unwrap_or_else(|e| panic!("d {d} fixes {fixes}: {e:?}"))
                .unwrap();
            assert!(
                (volume(&u) - whole).abs() <= 1e-8 * whole,
                "d {d} fixes {fixes}: {}",
                volume(&u)
            );
            assert_eq!(u.solid_count().unwrap(), solids, "d {d} fixes {fixes}");
            assert!(u.to_record().is_ok(), "d {d} fixes {fixes}");
        }
    }
}

/// A 0.68 mm sphere and a box whose edge runs 1.5e-4 inside it, 4.2e-6 deep, across the
/// sphere's seam, with a corner 9.8e-6 outside the sphere (`ub175` of the kernel's golden unit
/// `boolean.api_brep`). The vertex on the seam stands for three points up to 2.17e-5 apart and
/// the corner's for two 9.97e-6 apart, and each kept a tolerance short of them, so the union,
/// the intersection and the difference failed their checks. Each is now one valid solid, with
/// or without the fixes, and their volumes add up: union + common = sphere + box and
/// difference + common = sphere, within 1e-6 of the sphere (measured 2e-7; the lens the edge
/// cuts off is 3e-6 of it at most).
#[test]
fn a_box_edge_grazing_a_tiny_sphere_across_its_seam_gives_valid_solids() {
    let c = [
        0.000_620_018_136_026_583_4,
        -0.000_460_304_429_701_722_37,
        0.000_076_983_630_377_661_13,
    ];
    let r = 0.000_681_902_795_490_716_8_f64;
    let o = [
        0.000_510_378_840_547_897,
        -0.000_598_367_716_621_174,
        0.000_197_124_463_185_099_34,
    ];
    let d = [
        0.000_776_710_634_559_774_7,
        0.000_535_188_654_593_634_2,
        0.000_719_601_710_449_030_5,
    ];
    let sphere = 4.0 / 3.0 * PI * r * r * r;
    let block = d[0] * d[1] * d[2];
    for fixes in [false, true] {
        if fixes {
            all_fixes();
        }
        let run = |op: BooleanOp| {
            let a = make_sphere(c, r).unwrap();
            let b = make_box(o, d).unwrap();
            let body = boolean(a, b, op)
                .unwrap_or_else(|e| panic!("{op:?} fixes {fixes}: {e:?}"))
                .unwrap();
            assert!(
                body.is_manifold_solid().unwrap(),
                "{op:?} fixes {fixes}: not a manifold solid"
            );
            assert_eq!(body.solid_count().unwrap(), 1, "{op:?} fixes {fixes}");
            body.volume(1e-9).unwrap()
        };
        let union = run(BooleanOp::Union);
        let common = run(BooleanOp::Intersect);
        let difference = run(BooleanOp::Subtract);
        let off = [
            union + common - sphere - block,
            difference + common - sphere,
        ];
        // Written to accept: a NaN volume fails.
        assert!(
            off.iter().all(|v| v.abs() <= 1e-6 * sphere),
            "fixes {fixes}: union {union:e} common {common:e} difference {difference:e} against sphere {sphere:e} and box {block:e}"
        );
    }
}

/// Two equal cylinders side by side, touching along a line. Their faces leave the line
/// together and part as the square of the distance over the radius, so the fixed steps off
/// the line took two large ones for one surface, and the union failed (100 times the size)
/// or the intersection was a whole cylinder (1000 times). The union is the two solids
/// touching along the line and nothing is common, in either order.
#[test]
fn two_large_cylinders_touching_along_a_line_union() {
    all_fixes();
    for k in [1.0, 100.0, 1000.0] {
        let left = || solidcore_ops::make_cylinder([0.0; 3], 7.5 * k, 25.0 * k).unwrap();
        let right =
            || solidcore_ops::make_cylinder([15.0 * k, 0.0, 2.5 * k], 7.5 * k, 25.0 * k).unwrap();
        let want = 2.0 * PI * 7.5 * 7.5 * 25.0 * (k * k * k);
        for (a, b) in [(left(), right()), (right(), left())] {
            let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
            let report = u.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{k}: {}", report.text);
            assert_eq!(u.solid_count().unwrap(), 2, "{k}");
            assert!(
                near(volume(&u), want),
                "{k}: {}, closed form {want}",
                volume(&u)
            );
        }
        assert!(
            boolean(left(), right(), BooleanOp::Intersect)
                .unwrap()
                .is_none(),
            "{k}"
        );
    }
}

/// The area two circles of radius `r` whose centres are `d` apart have in common.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn lens_area(r: f64, d: f64) -> f64 {
    2.0 * r * r * (d / (2.0 * r)).acos() - 0.5 * d * (4.0 * r * r - d * d).sqrt()
}

/// A drill of radius 2.5 into the box face y = 0 along +y, centred at (x, z): its wall `depth`
/// deep, then a 118° point; it starts 1 outside the face.
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn drill(x: f64, z: f64, depth: f64) -> Body {
    let (r, top) = (2.5, -1.0);
    let tip = r / 59f64.to_radians().tan();
    let p = |u: f64, y: f64| [x, y, z + u];
    let profile = [
        Curve::Line {
            start: p(0.0, top),
            end: p(r, top),
        },
        Curve::Line {
            start: p(r, top),
            end: p(r, depth),
        },
        Curve::Line {
            start: p(r, depth),
            end: p(0.0, depth + tip),
        },
        Curve::Line {
            start: p(0.0, depth + tip),
            end: p(0.0, top),
        },
    ];
    revolve(&profile, [x, 0.0, z], [0.0, 1.0, 0.0], TAU, true).unwrap()
}

/// Two parallel holes of radius 2.5 whose axes are 5 − ε apart, a hair less than tangent: their
/// walls cross in two lines 2√(2.5 ε) apart and lie within ε of each other between them, and
/// so do their rims on a face square to the axes. The sketch solver of
/// tangency-labs/solidcraft#15429 left two drill centres 2.3e-9 closer than their diameter, the
/// upper one with its wall on the plate's top face, and the second cut failed in the merge;
/// through holes, and the two cylinders' union, failed as not a solid from ε = 1e-9. The walls
/// and the rims touch within the contact tolerance, as for holes a hair apart (ε = −1e-9):
/// each cut is one valid solid of the counts of the holes a hair apart and the volume less the
/// overlap (the lens of the two circles; the points' overlap is below 1e-12), in either order,
/// also for ε up to the contact tolerance (the zone tolerance, 1e-5), past the wall
/// intersection's search band (5e-6), where the walls and the rims must still agree; past the
/// intersection tolerance (2e-5) the walls cross in two lines, as the circles do, and the cut is
/// one valid solid of the volume. Between the two (ε = 1.5e-5) the cut may be refused, but never
/// a valid solid of another volume (a contact tolerance of 2e-5 kept the second tool whole, as a
/// void, 39.27 short). The two cylinders unite to two solids touching along the
/// line (to ε = 1e-7: beyond it their coplanar caps are not decided here). The drills keep the
/// counts of drills a hair apart to the issue's ε; from 1e-8 their points, which only the
/// marching intersection meets, add edges, and they are one valid solid of the volume.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn parallel_holes_a_hair_less_than_tangent_cut_as_touching() {
    all_fixes();
    let plate = || make_box([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]).unwrap();
    let cylinder = |y: f64| solidcore_ops::make_cylinder([10.0, y, -1.0], 2.5, 12.0).unwrap();
    let cut_twice = |one: Body, two: Body| {
        let first = boolean(plate(), one, BooleanOp::Subtract).unwrap().unwrap();
        boolean(first, two, BooleanOp::Subtract).unwrap().unwrap()
    };
    let (r, depth) = (2.5, 5.0);
    let tip = r / 59f64.to_radians().tan();
    let lens = |eps: f64| {
        if eps > 0.0 {
            lens_area(r, 5.0 - eps)
        } else {
            0.0
        }
    };
    // either order: the holes at a and b, or at b and a
    let orders = |a: f64, b: f64| [(a, b), (b, a)];
    for eps in [-1e-9, 1e-9, 2.3e-9, 1e-8, 1e-7, 3e-6, 9.9e-6, 2.5e-5] {
        let through = 4000.0 - (2.0 * PI * r * r - lens(eps)) * 10.0;
        // touching along one line, or past the intersection tolerance crossing in two
        let counts = if eps < 1e-5 {
            [8, 22, 14]
        } else {
            [10, 24, 16]
        };
        for (y1, y2) in orders(10.0, 5.0 + eps) {
            let c = cut_twice(cylinder(y1), cylinder(y2));
            one_valid_solid(&c, counts, through);
        }
    }
    let eps = 1.5e-5;
    let through = 4000.0 - (2.0 * PI * r * r - lens(eps)) * 10.0;
    for (y1, y2) in orders(10.0, 5.0 + eps) {
        let first = boolean(plate(), cylinder(y1), BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        // a refusal is allowed here; an empty result is not
        if let Ok(c) = boolean(first, cylinder(y2), BooleanOp::Subtract) {
            let c = c.unwrap();
            let report = c.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{eps}: {}", report.text);
            assert!(
                near(volume(&c), through),
                "{eps}: {}, closed form {through}",
                volume(&c)
            );
        }
    }
    for eps in [-1e-9, 1e-9, 2.3e-9, 1e-8, 1e-7] {
        let u = boolean(cylinder(10.0), cylinder(5.0 + eps), BooleanOp::Union)
            .unwrap()
            .unwrap();
        let report = u.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{eps}: {}", report.text);
        assert_eq!(u.solid_count().unwrap(), 2, "{eps}");
        assert_eq!(u.counts().unwrap(), [6, 10, 6], "{eps}");
        let want = (2.0 * PI * r * r - lens(eps)) * 12.0;
        assert!(
            near(volume(&u), want),
            "{eps}: {}, closed form {want}",
            volume(&u)
        );
    }
    for eps in [-1e-9, 1e-9, 2.3e-9, 1e-8, 1e-7] {
        let drilled = 4000.0 - 2.0 * PI * r * r * (depth + tip / 3.0) + lens(eps) * depth;
        for (z1, z2) in orders(7.5, 2.5 + eps) {
            let c = cut_twice(drill(5.0, z1, depth), drill(5.0, z2, depth));
            if eps < 1e-8 {
                one_valid_solid(&c, [12, 27, 16], drilled);
            } else {
                // the points' extra edges are not decided here: one valid solid, its volume
                let report = c.check(CheckLevel::All).unwrap();
                assert!(report.is_valid(), "{eps}: {}", report.text);
                assert_eq!(c.solid_count().unwrap(), 1, "{eps}");
                assert!(
                    near(volume(&c), drilled),
                    "{eps}: {}, closed form {drilled}",
                    volume(&c)
                );
            }
        }
    }
}

#[test]
fn an_imprint_cuts_the_faces_the_tool_crosses() {
    all_fixes();
    let body = make_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap();
    let tool = make_box([1.0, -1.0, -1.0], [4.0, 4.0, 4.0]).unwrap();
    let (r, changed) = imprint(body, tool).unwrap();
    assert!(changed);
    assert!(r.counts().unwrap()[0] > 6, "{:?}", r.counts());
    assert!(r.is_manifold_solid().unwrap());
    // a tool apart from the body cuts nothing: the body comes back, not dropped
    let body = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let tool = make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let (r, changed) = imprint(body, tool).unwrap();
    assert!(!changed);
    assert_eq!(r.counts().unwrap(), [6, 12, 8]);
    assert!(near(volume(&r), 1.0), "{}", volume(&r));
}

/// The kernel rig's case `booleans/<case>`: its entry, and its operand bodies by name, read
/// under the corpus's fix set.
fn corpus_case(case: &str) -> (Value, impl Fn(&str) -> Body) {
    let corpus = format!(
        "{}/../solidcore/tests/kernel_cases",
        env!("CARGO_MANIFEST_DIR")
    );
    session::set_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    let entry: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{corpus}/booleans/{case}.json")).unwrap(),
    )
    .unwrap();
    let gz = std::fs::File::open(format!("{corpus}/booleans/{case}.bodies.json.gz")).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let records: Value = serde_json::from_str(&text).unwrap();
    let trusted = entry["bodies"].clone();
    let body = move |name: &str| {
        let record: Record = serde_json::from_value(records[name].clone()).unwrap();
        Body::from_record(&record, trusted[name]["trusted"].as_bool().unwrap())
            .unwrap()
            .0
    };
    (entry, body)
}

/// The case `booleans/13462-rod-cut-by-two-counter-wound-channel-coils`: a rod cut by two
/// one-turn screw sweeps of one channel profile, one of each hand, from its top face. The
/// profile's radial walls meet the rod's cylinder, so each sweep's rail there is a helix on it,
/// and the two rails cross. The crossing is found more than once, a few 1e-12 apart in the
/// rail's parameter; the new edge along the second rail took the interval its UV curve's trim
/// had snapped onto a knot of its own, 2.4e-12 before the 3D curve's end (`SmEdge` test 2).
/// Its operands, and the result graded: one valid solid of the case's closed form (its
/// `expected.method`). The volume is measured to 1e-7, ten times finer than the case's
/// tolerance.
fn counter_wound_sweeps() -> (impl Fn(&str) -> Body, impl Fn(&str, &Body)) {
    let (entry, body) = corpus_case("13462-rod-cut-by-two-counter-wound-channel-coils");
    let want = entry["expected"]["volume_mm3"].as_f64().unwrap();
    let rel = entry["expected"]["tolerance"]["rel"].as_f64().unwrap();
    let graded = move |how: &str, r: &Body| {
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "cut {how}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "cut {how}");
        let v = r.volume(1e-7).unwrap();
        assert!(
            (v - want).abs() <= rel * want,
            "cut {how}: volume {v}, closed form {want}"
        );
    };
    (body, graded)
}

/// The second sweep cut from the rod the first one cut, as SolidCraft does it and the case
/// records it. b14 is the rod cut by the right-handed sweep as the case's run read it back from
/// its record, b5 the left-handed sweep.
#[test]
fn a_rod_cut_by_two_counter_wound_channel_sweeps_is_valid() {
    let (body, graded) = counter_wound_sweeps();
    let r = boolean(body("b14"), body("b5"), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    graded("in turn", &r);
}

/// The same rod cut by the union of the two sweeps (b4 and b5) at once: the same edge, through
/// the union first. b6 is the rod.
#[test]
#[ignore = "the sweeps' union and the cut by it take about 80 s unoptimised; the cut in turn \
            covers the same edge"]
fn a_rod_cut_by_the_union_of_two_counter_wound_channel_sweeps_is_valid() {
    let (body, graded) = counter_wound_sweeps();
    let both = union_all(vec![body("b4"), body("b5")]).unwrap();
    let r = boolean(body("b6"), both, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    graded("by the union", &r);
}

/// A dome turned about y from a quarter disc whose arc SolidCraft drew through a point written to
/// eight decimals (#13713): the arc's circle has radius 10 + 1.01e-8, and the arc starts 1.01e-9
/// rad above the x axis, so the dome's base circle lies 1.01e-8 above y = 0. Its seam lies in
/// z = 0 on +x.
fn rounded_dome() -> Body {
    let r = 10.000_000_010_132_878;
    let profile = [
        Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start: 1.013_287_798_059_198_8e-9,
            end: 1.570_796_325_781_608_9,
        },
        Curve::Line {
            start: [0.0, r, 0.0],
            end: [0.0; 3],
        },
        Curve::Line {
            start: [0.0; 3],
            end: [r, 1.013_287_799_085_951e-8, 0.0],
        },
    ];
    revolve(&profile, [0.0; 3], [0.0, -1.0, 0.0], TAU, true).unwrap()
}

/// One valid solid of `counts` faces, edges and vertices and `want` mm³ (the dome's 1e-8 off
/// its circle moves the volume by about 1e-9 relative).
fn one_valid_solid(r: &Body, counts: [usize; 3], want: f64) {
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    assert_eq!(r.counts().unwrap(), counts);
    assert!(near(volume(r), want), "{}, closed form {want}", volume(r));
}

/// The rounded dome joined flat to a cylinder of radius 10 below it, its seam on the dome's. The
/// two seams end 1.43e-8 apart at the vertex they share, 1.01e-8 radially and 1.01e-8 along the
/// seam, and not at all across it: the result is valid (no face test 36, "Face Near missing
/// Seam"), V = 2/3·π·10³ + π·10²·15.
#[test]
fn a_dome_1e_8_off_its_circle_joins_flat_to_a_cylinder() {
    all_fixes();
    let circle = [Curve::Arc {
        center: [0.0, -15.0, 0.0],
        axis: [0.0, 1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: 10.0,
        start: 0.0,
        end: TAU,
    }];
    let cylinder = extrude(
        &circle,
        [0.0, 1.0, 0.0],
        15.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let r = boolean(cylinder, rounded_dome(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    one_valid_solid(&r, [3, 4, 3], 2.0 / 3.0 * PI * 1e3 + PI * 1e2 * 15.0);
}

/// The rounded dome joined to its mirror image about y = 0: the two seams end 2.03e-8 apart, along
/// the seam only. The result is a valid sphere, V = 4/3·π·10³.
#[test]
fn a_dome_1e_8_off_its_circle_joins_to_its_mirror() {
    all_fixes();
    let mirror = rounded_dome();
    mirror
        .transform(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap();
    let r = boolean(rounded_dome(), mirror, BooleanOp::Union)
        .unwrap()
        .unwrap();
    one_valid_solid(&r, [2, 3, 3], 4.0 / 3.0 * PI * 1e3);
}

/// Cancel stops the full check of a long coil cut (solidcore#3247): a 20-turn square coil cut
/// from a cylinder, the case of tangency-labs/solidcraft#3962, whose check takes seconds (most
/// of the cut's time, in the edges' curve analyses). Measured in the token's polls, not in
/// time: a counting token (`CancelToken::after_polls`) first counts the polls of the whole
/// check, `whole`; a token cancelled from a quarter of them then stops a second check there.
/// It returns `Cancelled`, and the token is polled exactly once after the poll that cancelled
/// it: by the session as the call returns, asking whether it was cancelled (`run_with`, after
/// the kernel's call). Any further poll would be work done after the cancel.
#[test]
fn the_check_of_a_long_coil_cut_stops_on_cancel() {
    use solidcore_ops::cancel::{within, CancelToken};
    use solidcore_ops::sweep::{screw_sweep, Curve};
    let corners = [
        [19.0, 0.0, 12.0],
        [32.0, 0.0, 12.0],
        [32.0, 0.0, 25.0],
        [19.0, 0.0, 25.0],
    ];
    let square: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let coil = screw_sweep(
        &square,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        15.24,
        20.0,
        None,
        15.24e-4,
    )
    .unwrap();
    let stock = solidcore_ops::make_cylinder([0.0; 3], 20.0, 304.5).unwrap();
    let cut = boolean(stock, coil, BooleanOp::Subtract).unwrap().unwrap();
    let probe = CancelToken::after_polls(u64::MAX);
    within(&probe, || cut.check(CheckLevel::All)).unwrap();
    let whole = probe.polls().unwrap();
    let n = whole / 4;
    // cancelled well inside the check, not at its last poll
    assert!(n >= 1 && whole >= 2 * n, "the check polls {whole} times");
    let token = CancelToken::after_polls(n);
    let r = within(&token, || cut.check(CheckLevel::All));
    eprintln!(
        "the check polls {whole} times; cancelled at {n}, polled {:?}",
        token.polls()
    );
    assert_eq!(r.map(|_| ()), Err(Error::Cancelled));
    assert_eq!(token.polls(), Some(n + 1));
}

/// #13295: a bar of L section (100 long; its step's top edge along x at y 12.527, z 19.567)
/// cut by a circle of radius 4.717 sketched on the plane at 45 degrees through that edge and
/// extruded into the bar. The sketch puts the circle's centre 7.9e-6 off the edge, so the cut
/// wall's corner on the edge, where its seam edge ends, lies that far across the wall's
/// seam: inside the vertex's 1e-5 tolerance, with no sliver beside the seam. The check
/// refused it (face test 36) for centres 1e-7 to 1e-5 off. Every offset now gives one valid
/// solid, its volume moving with the offset by no more than the wall's swept area allows.
///
/// The seam's corner on the bar's edge `d` off (the seam vertex, made one with its drop on the
/// edge and placed midway) is `d` from where the seam crosses the step face beside the edge
/// and 1.118 `d` from the midway point: for `d` from 1e-5 to 1.3e-5 the crossing lies within
/// the seam vertex's tolerance of it but farther than that from the midway point, so it is
/// one point only by the vertex's own ball, which its proto vertex holds (docs/booleans.md,
/// "One point or two"). Without it the cut was refused (`MergedVertexReaches`) at 1e-5, 1.1e-5
/// and 1.3e-5 once two points are one only within the larger of their tolerances. From 2.05e-5
/// to 2.4e-5 the step face's curve leaves the seam where the seam's end vertex is past both
/// tolerances but within their sum: it no longer runs "along" the seam to there (docs/booleans.md,
/// "Seams and poles"), and the cut, refused (not a solid) or empty before, is valid.
#[test]
fn a_cut_whose_seam_edge_ends_within_tolerance_across_the_seam_is_valid() {
    all_fixes();
    let (y0, z1, yc, z2) = (
        16.423877938511588,
        13.06859205815335,
        12.527075811882755,
        19.566787029154483,
    );
    let p = [
        [0.0, 0.0, 0.0],
        [0.0, y0, 0.0],
        [0.0, y0, z1],
        [0.0, yc, z1],
        [0.0, yc, z2],
        [0.0, 0.0, z2],
    ];
    let profile: Vec<Curve> = (0..6)
        .map(|i| Curve::Line {
            start: p[i],
            end: p[(i + 1) % 6],
        })
        .collect();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let radius = 4.716_990_566_008_734;
    let cut = |d: f64| {
        let bar = extrude(
            &profile,
            [1.0, 0.0, 0.0],
            100.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        // the circle's centre `d` off the edge, in the sketch plane (normal (0, 1, 1) / sqrt 2)
        let centre = [20.4216146277246, yc + d * s, z2 - d * s];
        let circle = [Curve::Arc {
            center: centre,
            axis: [0.0, s, s],
            reference: [1.0, 0.0, 0.0],
            radius,
            start: 0.0,
            end: TAU,
        }];
        let tool = extrude(
            &circle,
            [0.0, -s, -s],
            60.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        boolean(bar, tool, BooleanOp::Subtract)
            .unwrap_or_else(|e| panic!("d {d}: {e:?}"))
            .unwrap()
    };
    let exact = volume(&cut(0.0));
    // (1e-6 off, the cut is no solid, before this change too: #14840)
    for d in [
        1e-9,
        1e-7,
        3e-7,
        2e-6,
        7.868_858_162e-6,
        1e-5,
        1.1e-5,
        1.3e-5,
        1.5e-5,
        2e-5,
        2.2e-5,
        2.4e-5,
        3e-5,
        1e-4,
    ] {
        let r = cut(d);
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "d {d}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "d {d}");
        // the wall moves by d: the volume by at most its perimeter times its length times d
        let bound = TAU * radius * 60.0 * d + 1e-7 * exact;
        assert!(
            (volume(&r) - exact).abs() <= bound,
            "d {d}: {} vs {exact}",
            volume(&r)
        );
    }
}

/// A 120 degree sector of height 119 with its apex at the origin, its legs ending at the
/// sketch point (±74.0452, 42.75) (85.5 at 30 and 150 degrees, rounded to four decimals, so
/// each leg turns 1.636e-7 rad outward), its arc about the origin, turned `turn` degrees
/// about z.
#[allow(clippy::disallowed_methods)] // the test's operands and closed form, not ops geometry
fn sector_of_rounded_sketch(turn: f64) -> Body {
    let (x, y) = (74.0452, 42.75);
    let profile = [
        Curve::Line {
            start: [0.0; 3],
            end: [x, y, 0.0],
        },
        Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: x.hypot(y),
            start: y.atan2(x),
            end: y.atan2(-x),
        },
        Curve::Line {
            start: [-x, y, 0.0],
            end: [0.0; 3],
        },
    ];
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        119.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let (s, c) = turn.to_radians().sin_cos();
    b.transform(&[
        [c, -s, 0.0, 0.0],
        [s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    b
}

/// Two copies of the sector of a sketch whose leg points were rounded, turned 120 degrees
/// apart (SolidCraft, a case joined from three such sectors): the legs that should coincide
/// cross at 3.3e-7 rad along the z axis, so the sectors overlap in a sliver, and the arc of
/// one runs 2.8e-5 past the other's leg. The imprint splits that arc there; once the leg is
/// gone the split vertex is redundant and its two arc pieces are joined back. The join kept
/// the split as a knot, a span of 1.6e-7 of the arc whose control points coincide, and the
/// cap faces' trim curves made from it failed the check (`SmBSplineCurve` test 15, the
/// polar conversion test): the union was refused as not a solid. Joined into one span, the
/// arc is the operand's again and the union is one valid solid of 6 faces, 12 edges and 8
/// vertices, V = (2α - 2d)/2 · r² · 119 for the sector angle α = 120° + 2d and the overlap
/// 2d, d = 30° - atan2(42.75, 74.0452).
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn sectors_of_a_rounded_sketch_turned_120_degrees_union() {
    session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    let r = 74.0452_f64.hypot(42.75);
    let d = 30_f64.to_radians() - 42.75_f64.atan2(74.0452);
    let alpha = 2.0 * PI / 3.0 + 2.0 * d;
    let want = (2.0 * alpha - 2.0 * d) / 2.0 * r * r * 119.0;
    let u = boolean(
        sector_of_rounded_sketch(0.0),
        sector_of_rounded_sketch(120.0),
        BooleanOp::Union,
    )
    .unwrap()
    .unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_eq!(u.counts().unwrap(), [6, 12, 8]);
    let v = volume(&u);
    let off = (v - want).abs() / want;
    assert!(!(off.is_nan() || off > 1e-9), "{v}, closed form {want}");
}

/// A 120° pie slice of height 119 symmetric about +y, turned by `turn` about z: its legs run
/// from the origin at 90° ∓ (60° + `widen`) to corners `rho` away, its arc is the circle about
/// (0, `c`) through the corners (SolidCraft draws the arc through its rounded end points, so
/// its centre can miss the apex).
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn sector(widen: f64, rho: f64, c: f64, turn: f64) -> Body {
    let half = PI / 3.0 + widen;
    let p0 = [rho * (PI / 2.0 - half).cos(), rho * (PI / 2.0 - half).sin()];
    let p1 = [-p0[0], p0[1]];
    let (ct, st) = (turn.cos(), turn.sin());
    let at = |p: [f64; 2]| [ct * p[0] - st * p[1], st * p[0] + ct * p[1], 0.0];
    let profile = [
        Curve::Line {
            start: [0.0; 3],
            end: at(p0),
        },
        Curve::Arc {
            center: at([0.0, c]),
            axis: [0.0, 0.0, 1.0],
            reference: at([1.0, 0.0]),
            radius: (p0[0].powi(2) + (p0[1] - c).powi(2)).sqrt(),
            start: (p0[1] - c).atan2(p0[0]),
            end: (p1[1] - c).atan2(p1[0]),
        },
        Curve::Line {
            start: at(p1),
            end: [0.0; 3],
        },
    ];
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        119.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

/// Two pie slices side by side, the second turned by 120° + `apart` about the apex: their
/// facing walls meet at the apex edge and part linearly, `apart` · 85.5 at the arc. Within
/// the 1e-5 tolerance across the wall (`apart` <= 1e-7) the walls are one and the slices join
/// into one solid; 3.3e-7 rad (2.8e-5 at the arc) and 1e-6 rad apart they are two walls, and
/// the union is the two slices touching along the apex edge, V = 2 · r²/2 · 2π/3 · h exactly.
/// Before, the radial sector read the two walls (closer than 0.001°) as coincident and merged
/// their corner edges 2.8e-5 apart: not a closed solid. Overlapping slices (`apart` < 0) join
/// into one solid of V = r²/2 · (4π/3 + apart) · h.
#[test]
fn pie_slices_whose_walls_part_beyond_tolerance_join_along_the_apex() {
    all_fixes();
    let (r, h) = (85.5, 119.0);
    let slice = r * r / 2.0 * (2.0 * PI / 3.0) * h;
    for apart in [0.0, 1e-8, 1e-7, 3.3e-7, 1e-6, -1e-8, -1e-7, -3.3e-7, -1e-6] {
        let a = sector(0.0, r, 0.0, 0.0);
        let b = sector(0.0, r, 0.0, 2.0 * PI / 3.0 + apart);
        let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
        let report = u.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{apart}: {}", report.text);
        let v = volume(&u);
        // the wedge between the walls, which a join within tolerance may absorb or keep
        let wedge = r * r * h * apart.abs();
        if apart > 2e-7 {
            assert_eq!(u.solid_count().unwrap(), 2, "{apart}");
            assert_eq!(u.counts().unwrap(), [10, 17, 10], "{apart}");
            assert!(near(v, 2.0 * slice), "{apart}: {v}");
        } else {
            assert_eq!(u.solid_count().unwrap(), 1, "{apart}");
            assert_eq!(u.counts().unwrap(), [6, 12, 8], "{apart}");
            let want = 2.0 * slice + r * r / 2.0 * apart.min(0.0) * h;
            let err = (v - want).abs();
            assert!(
                !(err.is_nan() || err > wedge + 1e-7 * want),
                "{apart}: {v}, closed form {want}"
            );
        }
    }
}

/// The half roll r 48..50 about the z axis, z 0..40, on the y > 0 side, its walls extruded from
/// a profile whose arcs are framed by `frame % 4` (counter-clockwise from +x or clockwise from
/// -x, about +z or -z), then mirrored onto itself in x (`frame & 4`) and in z (`frame & 8`):
/// sixteen frames of the walls' cylinders, one solid.
fn half_roll(frame: u32) -> Body {
    let (axis, reference, start, end) = match frame % 4 {
        0 => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.0, PI),
        1 => ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], 0.0, PI),
        2 => ([0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], -PI, 0.0),
        _ => ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], -PI, 0.0),
    };
    let arc = |radius| Curve::Arc {
        center: [0.0; 3],
        axis,
        reference,
        radius,
        start,
        end,
    };
    let profile = [
        arc(50.0),
        Curve::Line {
            start: [-50.0, 0.0, 0.0],
            end: [-48.0, 0.0, 0.0],
        },
        arc(48.0),
        Curve::Line {
            start: [48.0, 0.0, 0.0],
            end: [50.0, 0.0, 0.0],
        },
    ];
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        40.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let sx = if frame & 4 == 0 { 1.0 } else { -1.0 };
    let (sz, tz) = if frame & 8 == 0 {
        (1.0, 0.0)
    } else {
        (-1.0, 40.0)
    };
    if frame >= 4 {
        b.transform(&[
            [sx, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, sz, 0.0],
            [0.0, 0.0, tz, 1.0],
        ])
        .unwrap();
    }
    b
}

/// The area of the disc of radius `rho` centred `c` from the origin inside the circle of radius
/// `r` about the origin (the lens of two circles).
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn lens(r: f64, rho: f64, c: f64) -> f64 {
    if rho <= 0.0 || c >= r + rho {
        return 0.0;
    }
    if c <= (r - rho).abs() {
        return PI * r.min(rho).powi(2);
    }
    let a1 = ((c * c + r * r - rho * rho) / (2.0 * c * r))
        .clamp(-1.0, 1.0)
        .acos();
    let a2 = ((c * c + rho * rho - r * r) / (2.0 * c * rho))
        .clamp(-1.0, 1.0)
        .acos();
    let k = ((-c + r + rho) * (c + r - rho) * (c - r + rho) * (c + r + rho))
        .max(0.0)
        .sqrt();
    r * r * a1 + rho * rho * a2 - 0.5 * k
}

/// `f` integrated over `[a, b]` by tanh-sinh quadrature at step `h` (its error falls doubly
/// exponentially in `1/h`, the endpoint singularities of `f` included).
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn tanh_sinh(f: impl Fn(f64) -> f64, a: f64, b: f64, h: f64) -> f64 {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let mut sum = 0.0;
    for k in 0..10_000 {
        let t = f64::from(k) * h;
        let u = PI / 2.0 * t.sinh();
        let (x, w) = (u.tanh(), PI / 2.0 * t.cosh() / u.cosh().powi(2));
        if w < 1e-300 || 1.0 - x < 1e-17 {
            break;
        }
        sum += w * f(mid + half * x);
        if k > 0 {
            sum += w * f(mid - half * x);
        }
    }
    sum * h * half
}

/// The r 6 ball about `center`, its seam meridian turned `turn` radians about its axis from +x.
#[allow(clippy::disallowed_methods)] // the test's operand, not ops computing geometry
fn ball(center: [f64; 3], turn: f64) -> Body {
    let b = make_sphere([0.0; 3], 6.0).unwrap();
    let (s, c) = turn.sin_cos();
    let [x, y, z] = center;
    b.transform(&[
        [c, s, 0.0, 0.0],
        [-s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [x, y, z, 1.0],
    ])
    .unwrap();
    b
}

/// The volume of the r 6 ball about (0, `c`, 20) inside the radius `radius(z)` about the z axis:
/// the integral over z of the lens areas of the ball's section (radius √(36 - (z-20)²), centred
/// `c` from the axis) within that radius, by tanh-sinh quadrature over the pieces between the
/// heights where a lens turns whole or empty (the section's radius is |radius(z) - c|, found by
/// bisection). Asserts that steps 1/32 and 1/64 agree to 1e-13 relative.
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn ball_inside(radius: impl Fn(f64) -> f64, c: f64) -> f64 {
    let section = |z: f64| (36.0 - (z - 20.0) * (z - 20.0)).max(0.0).sqrt();
    let kink = |z: f64| section(z) - (radius(z) - c).abs();
    let mut cuts = vec![14.0];
    let n = 1200;
    for i in 0..n {
        let (mut a, mut b) = (
            14.0 + 12.0 * f64::from(i) / f64::from(n),
            14.0 + 12.0 * f64::from(i + 1) / f64::from(n),
        );
        if (kink(a) > 0.0) != (kink(b) > 0.0) {
            for _ in 0..200 {
                let m = (a + b) / 2.0;
                if (kink(a) > 0.0) == (kink(m) > 0.0) {
                    a = m;
                } else {
                    b = m;
                }
            }
            cuts.push((a + b) / 2.0);
        }
    }
    cuts.push(26.0);
    let v = |h: f64| {
        cuts.windows(2)
            .map(|w| tanh_sinh(|z| lens(radius(z), section(z), c), w[0], w[1], h))
            .sum::<f64>()
    };
    let (coarse, fine) = (v(1.0 / 32.0), v(1.0 / 64.0));
    assert!(
        (coarse - fine).abs() <= 1e-13 * fine.abs().max(1.0),
        "{coarse} {fine}"
    );
    fine
}

/// `a` less `b` is one solid that checks valid at every level, of `want` mm³ within 1e-8
/// relative (`what` names the case in the failure).
fn cut_is_valid_solid(a: Body, b: Body, want: f64, what: &str) -> Option<String> {
    match boolean(a, b, BooleanOp::Subtract) {
        Ok(Some(r)) => {
            let report = r.check(CheckLevel::All).unwrap();
            let v = volume(&r);
            let off = (v - want).abs() / want;
            (!report.is_valid() || r.solid_count().unwrap() != 1 || off.is_nan() || off > 1e-8)
                .then(|| format!("{what}: {v} mm³ ({off:e} off {want}), {}", report.text))
        }
        other => Some(format!("{what}: {other:?}")),
    }
}

/// The half roll less the r 6 ball about (0, 50, 20) (SolidCraft's sheet metal, a ball cut
/// centred on a roll's face): the ball's poles (0, 50, 14) and (0, 50, 26) lie on the roll's
/// outside, and the line where the ball crosses that wall runs into each pole along the ball's
/// seam meridian, so on the ball the line's trim ends at a corner of its domain, the pole on the
/// seam. The trace's UV tangent line met the seam a little before the pole, the step was cut
/// short there though the curve does not cross the seam, and a second step 1e-4 or less went
/// on to the pole: stepped back 1 % of that, the pole's UV tangent and singular parameter were
/// below the coordinates' precision, and 12 of the 16 frames gave trims with coincident control
/// points, cusps, jumps across the seam or ends outside the face, refused as not a solid. Each
/// frame is one valid solid of V = 3920π less the ball between radii 48 and 50
/// ([`ball_inside`]). The volume the kernel measures is 2.1e-9 to 2.5e-9 relative above it at
/// every accuracy asked (the traced trims' deviation), inside the 1e-8 bound.
#[test]
fn a_ball_centred_on_a_rolls_face_with_its_poles_on_the_face_is_cut_in_every_frame() {
    all_fixes();
    let want = 3920.0 * PI - (ball_inside(|_| 50.0, 50.0) - ball_inside(|_| 48.0, 50.0));
    let failed: Vec<String> = (0..16)
        .filter_map(|frame| {
            let b = ball([0.0, 50.0, 20.0], 0.0);
            cut_is_valid_solid(half_roll(frame), b, want, &format!("frame {frame}"))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Four frames of the cut above that failed, the ball's seam turned 1e-8, 1e-10 and -1e-8 rad
/// off the meridian the line runs into the poles along: the trim's end is then that far off the
/// corner of the ball's domain, and these were refused too. Each is one valid solid of the same
/// volume as above (turning the ball about its axis moves no point of it).
#[test]
fn a_ball_with_its_poles_on_a_rolls_face_and_its_seam_turned_slightly_off_is_cut() {
    all_fixes();
    let want = 3920.0 * PI - (ball_inside(|_| 50.0, 50.0) - ball_inside(|_| 48.0, 50.0));
    let mut failed = Vec::new();
    for turn in [1e-8, 1e-10, -1e-8] {
        for frame in [2, 5, 8, 14] {
            let what = format!("turned {turn:e}, frame {frame}");
            let b = ball([0.0, 50.0, 20.0], turn);
            failed.extend(cut_is_valid_solid(half_roll(frame), b, want, &what));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The ball's poles on a cylinder's seam and on a face's boundary edge, its own seam on the
/// line's meridian there or square to it. A solid cylinder r 50, z 0..40, extruded from a whole
/// circle starting at +x (its seam on +x), less the ball about (50, 0, 20): its poles are on
/// that seam, V = π·50²·40 less the ball inside radius 50. The half roll (frame 0) less the
/// same ball: its poles are on the edge between the roll's outside and its end at y = 0, and
/// the ball is symmetric about that end's plane, so V = 3920π less half the ball between radii
/// 48 and 50.
#[test]
fn a_ball_with_its_poles_on_a_cylinders_seam_or_on_a_faces_edge_is_cut() {
    all_fixes();
    let inside_50 = ball_inside(|_| 50.0, 50.0);
    let between = inside_50 - ball_inside(|_| 48.0, 50.0);
    let mut failed = Vec::new();
    for turn in [0.0, PI / 2.0] {
        let circle = [Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 50.0,
            start: 0.0,
            end: TAU,
        }];
        let cylinder = extrude(
            &circle,
            [0.0, 0.0, 1.0],
            40.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        failed.extend(cut_is_valid_solid(
            cylinder,
            ball([50.0, 0.0, 20.0], turn),
            PI * 2500.0 * 40.0 - inside_50,
            &format!("cylinder's seam, turned {turn}"),
        ));
        failed.extend(cut_is_valid_solid(
            half_roll(0),
            ball([50.0, 0.0, 20.0], turn),
            3920.0 * PI - between / 2.0,
            &format!("roll's end edge, turned {turn}"),
        ));
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The ball's poles on a plane and on a cone. A box on one side of the plane y = 0 (which holds
/// the ball's seam meridian) or of x = 0 (square to it) less the ball about the origin: the
/// plane cuts the ball in a great circle through both poles, V = 20·10·20 less half the ball,
/// 2/3·π·6³. A frustum about +z, radius 60 at z = 0 to 40 at z = 40, less the ball about
/// (0, 47, 20): its top pole (0, 47, 26) is on the cone (radius 60 - 26/2 = 47 there) and the
/// line runs into it along the ball's seam meridian; V = π·40/3·(60² + 60·40 + 40²) less the
/// ball inside radius 60 - z/2.
#[test]
fn a_ball_with_its_poles_on_a_plane_or_a_cone_is_cut() {
    all_fixes();
    let half_ball = 2.0 / 3.0 * PI * 216.0;
    let mut failed = Vec::new();
    failed.extend(cut_is_valid_solid(
        make_box([-10.0, 0.0, -10.0], [20.0, 10.0, 20.0]).unwrap(),
        ball([0.0; 3], 0.0),
        4000.0 - half_ball,
        "plane y = 0",
    ));
    failed.extend(cut_is_valid_solid(
        make_box([0.0, -10.0, -10.0], [10.0, 20.0, 20.0]).unwrap(),
        ball([0.0; 3], 0.0),
        4000.0 - half_ball,
        "plane x = 0",
    ));
    let frustum = PI * 40.0 / 3.0 * (3600.0 + 2400.0 + 1600.0);
    failed.extend(cut_is_valid_solid(
        make_cone([0.0; 3], 60.0, 40.0, 40.0).unwrap(),
        ball([0.0, 47.0, 20.0], 0.0),
        frustum - ball_inside(|z| 60.0 - z / 2.0, 47.0),
        "cone",
    ));
    assert!(failed.is_empty(), "{failed:#?}");
}

/// SolidCraft's cone (#18546): a frustum revolved about +z, radius 3 at z = 0 to 0.5 at
/// z = 1000, its seam in the half plane y = 0, x > 0. A round notch, r 0.06, centred on the top
/// rim and extruded from z = 999.5 to the top, crosses the rim 9.2e-5 rad past the seam. The
/// curve where the notch's wall meets the cone crosses the seam at a slant of about 2e-4 rad,
/// so the two lie within tolerance of each other along a short piece of the seam: two proto
/// edges overlapping there, running opposite ways. The boolean combined them and then
/// removed the second at proto vertices that no longer listed it, refused as "the proto edge
/// is not listed". It is one valid solid of 5 faces (side, base, top, notch wall, notch
/// floor), 10 edges and 7 vertices: the base circle, the seam below the overlap, the overlap
/// shared by the seam and the wall's curve, the wall's curve before and after it, the floor's
/// arcs on the cone and on the wall, the wall's curve on the cone's far side, the rim and the
/// top's arc on the wall; a vertex at the seam's foot, at each end of the overlap, and where
/// the floor and the top each meet the cone on either side. V = the frustum less the notch,
/// the notch the integral over z of the area the r 0.06 disc shares with the cone's circle
/// there (two circles' lens, in closed form), by Simpson's rule.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_round_notch_overlapping_a_cones_seam_backwards_is_cut() {
    all_fixes();
    let (r_base, r_top, height, r, depth) = (3.0, 0.5, 1000.0_f64, 0.06, 0.5);
    let section = [
        [0.0, 0.0, 0.0],
        [r_base, 0.0, 0.0],
        [r_top, 0.0, height],
        [0.0, 0.0, height],
    ];
    let profile: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: section[i],
            end: section[(i + 1) % 4],
        })
        .collect();
    let cone = || revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap();
    let notch = |c: [f64; 2]| {
        let circle = [Curve::Arc {
            center: [c[0], c[1], height - depth],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start: 0.0,
            end: TAU,
        }];
        extrude(
            &circle,
            [0.0, 0.0, 1.0],
            depth,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    };
    let c: [f64; 2] = [0.496_405_514_95, 0.059_846_175_546_1];
    let d = c[0].hypot(c[1]);
    // the area the disc of radius `r` about `c` shares with the disc of radius `big` about the
    // axis, `big` - r < d < `big` + r
    let lens = |big: f64| {
        let a = big * big * ((d * d + big * big - r * r) / (2.0 * d * big)).acos();
        let b = r * r * ((d * d + r * r - big * big) / (2.0 * d * r)).acos();
        let k = (-d + big + r) * (d + big - r) * (d - big + r) * (d + big + r);
        a + b - 0.5 * k.sqrt()
    };
    let radius_at = |z: f64| r_top + (r_base - r_top) * (height - z) / height;
    let n = 2000;
    let h = depth / f64::from(n);
    let removed = (0..=n)
        .map(|i| {
            let w = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            w * lens(radius_at(height - depth + f64::from(i) * h))
        })
        .sum::<f64>()
        * h
        / 3.0;
    let frustum = PI * height / 3.0 * (r_base * r_base + r_base * r_top + r_top * r_top);
    let cut = boolean(cone(), notch(c), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&cut, [5, 10, 7], frustum - removed, 1e-9);
}

/// The half roll less the r 6 ball about (0, 50 + δ, 20), its poles δ off the roll's outside:
/// 1e-6 inside it and 1e-6 outside, within the tolerance the trace refines with, in the four
/// frames of the walls' cylinders that failed above. The line where the ball crosses the wall
/// passes the poles δ from them, and the trace takes each pole as a point of it (within that
/// tolerance) and ends two curves there. `StepBackFromSingularity` took the pole point's
/// singular parameter 1 % of the last span back from the pole, where the curve's direction
/// seen from the pole is set by the pole's offset (an angle of about δ over the distance), not
/// by the curve: every span into the pole failed the span test by about 100 δ, the steps
/// shrank to the tolerance, and the pole points got singular parameters a quarter turn off
/// (0.73 for 0.5) and UV tangents of 3e5, refused as not a solid. Each frame is one valid
/// solid of V = 3920π less the ball between radii 48 and 50 at that distance ([`ball_inside`]).
#[test]
fn a_ball_with_its_poles_within_tolerance_of_a_rolls_face_is_cut() {
    all_fixes();
    let mut failed = Vec::new();
    for delta in [-1e-6, 1e-6] {
        let d = 50.0 + delta;
        let want = 3920.0 * PI - (ball_inside(|_| 50.0, d) - ball_inside(|_| 48.0, d));
        for frame in [2, 5, 8, 14] {
            let what = format!("δ {delta:e}, frame {frame}");
            failed.extend(cut_is_valid_solid(
                half_roll(frame),
                ball([0.0, d, 20.0], 0.0),
                want,
                &what,
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The half roll less the r 6 ball about (0, 50 - 1e-6, 20), its poles 1e-6 inside the roll's
/// outside (within the tolerance the trace refines with), its seam turned 0.01, 0.02 and 0.03
/// rad off the meridian the line runs into the poles along (#16637). The line passes each pole
/// 1e-6 away and crosses the seam δ/sin(turn) from it, 1e-4 to 3.3e-5: farther than the summed
/// tolerances (2e-5), within upstream's combine radius (1e-4). The crossing's vertex
/// definition was combined into the pole's and deleted, the proto vertex stayed at the average
/// of the two, and the curve that started at the crossing was continued past its start to that
/// average, out of the ball's UV domain (`SmBrep` test 70); every frame was refused as not a
/// solid. The merged vertex is now the pole, its tolerance the certified distance to the
/// crossing it absorbed, and that curve's end is moved onto the pole inside the domain. Each
/// frame is one valid solid of V = 3920π less the ball between radii 48 and 50 at that
/// distance ([`ball_inside`]), with the face, edge and vertex counts of the same frame with
/// the seam turned 0.1 (the crossing within the summed tolerances of the pole, one vertex
/// there before and after #16637): no vertex or sliver edge is left at the crossing.
#[test]
fn a_ball_with_its_poles_within_tolerance_and_its_seam_turned_off_the_line_is_cut() {
    all_fixes();
    let d = 50.0 - 1e-6;
    let want = 3920.0 * PI - (ball_inside(|_| 50.0, d) - ball_inside(|_| 48.0, d));
    let counts = |frame, turn: f64| {
        boolean(
            half_roll(frame),
            ball([0.0, d, 20.0], turn),
            BooleanOp::Subtract,
        )
        .ok()
        .flatten()
        .and_then(|r| r.counts().ok())
    };
    let mut failed = Vec::new();
    for frame in [2, 5, 8, 14] {
        let control = counts(frame, 0.1);
        for turn in [0.01, 0.02, 0.03] {
            let what = format!("turned {turn}, frame {frame}");
            failed.extend(cut_is_valid_solid(
                half_roll(frame),
                ball([0.0, d, 20.0], turn),
                want,
                &what,
            ));
            let got = counts(frame, turn);
            if got.is_none() || got != control {
                failed.push(format!("{what}: counts {got:?}, turned 0.1 {control:?}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The controls of the cut above, valid before and after #16637: the seam turned ±1e-3 rad
/// (the crossing 1e-3 from the pole, beyond the combine radius) and ±0.1 rad (1e-5, within
/// the summed tolerances) with the poles 1e-6 inside the roll's outside, and every turn of the
/// test above and these with the poles 1e-6 outside it. Each frame is one valid solid of the
/// closed-form volume.
#[test]
fn a_ball_with_its_poles_within_tolerance_and_its_seam_turned_is_cut_in_the_controls() {
    all_fixes();
    let mut failed = Vec::new();
    for (delta, turns) in [
        (-1e-6, &[-1e-3, 1e-3, -0.1, 0.1][..]),
        (1e-6, &[0.01, 0.02, 0.03, -1e-3, 1e-3, -0.1, 0.1][..]),
    ] {
        let d = 50.0 + delta;
        let want = 3920.0 * PI - (ball_inside(|_| 50.0, d) - ball_inside(|_| 48.0, d));
        for &turn in turns {
            for frame in [2, 5, 8, 14] {
                let what = format!("δ {delta:e}, turned {turn}, frame {frame}");
                failed.extend(cut_is_valid_solid(
                    half_roll(frame),
                    ball([0.0, d, 20.0], turn),
                    want,
                    &what,
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The same cut with the poles 1e-4 inside and outside the roll's outside, past the
/// tolerance: each of the four frames is one valid solid of the closed-form volume, as it
/// was before the trace placed its points at poles.
#[test]
fn a_ball_with_its_poles_past_tolerance_of_a_rolls_face_is_cut() {
    all_fixes();
    let mut failed = Vec::new();
    for delta in [-1e-4, 1e-4] {
        let d = 50.0 + delta;
        let want = 3920.0 * PI - (ball_inside(|_| 50.0, d) - ball_inside(|_| 48.0, d));
        for frame in [2, 5, 8, 14] {
            let what = format!("δ {delta:e}, frame {frame}");
            failed.extend(cut_is_valid_solid(
                half_roll(frame),
                ball([0.0, d, 20.0], 0.0),
                want,
                &what,
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A box read from its record and mirrored across x = 0 into one of two openings cut into the
/// side of a loft through two closed cubic splines, 0.2 mm clear of the loft on every side
/// (#15862). The mirror keeps each of the box's straight edges a line: before, each became the
/// degree 1 B-spline of the whole line, its control points some 1.8e7 mm out, and the loft
/// minus the box was nothing, the loft and the box intersected in the whole loft, and their union
/// was refused (1001). The loft is SolidCraft's, as its record (`data/`).
#[test]
fn a_box_mirrored_into_an_opening_of_a_spline_loft_stays_clear_of_it() {
    all_fixes();
    let record: Record = serde_json::from_str(include_str!(
        "data/issue_15862_spline_loft_with_openings.record.json"
    ))
    .unwrap();
    let loft = || Body::from_record(&record, true).unwrap().0;
    let lofted = volume(&loft());
    // the box as SolidCraft makes it: its rectangle extruded, read back from its record
    let mirrored = || {
        let corners = [[-24.8, 12.0], [-10.2, 12.0], [-10.2, 25.0], [-24.8, 25.0]];
        let rectangle: Vec<Curve> = (0..4)
            .map(|i| {
                let ([x0, y0], [x1, y1]) = (corners[i], corners[(i + 1) % 4]);
                Curve::Line {
                    start: [x0, y0, 16.2],
                    end: [x1, y1, 16.2],
                }
            })
            .collect();
        let made = extrude(
            &rectangle,
            [0.0, 0.0, 1.0],
            7.6,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        let b = Body::from_record(&made.to_record().unwrap().0, true)
            .unwrap()
            .0;
        b.transform(&[
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap();
        b
    };
    let boxed = 14.6 * 13.0 * 7.6;
    let cut = boolean(loft(), mirrored(), BooleanOp::Subtract).unwrap();
    assert!(cut.is_some(), "the loft minus the box is nothing");
    let cut = cut.unwrap();
    let report = cut.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(cut.solid_count().unwrap(), 1);
    assert_eq!(cut.counts().unwrap(), [13, 27, 18]);
    assert!(
        near(volume(&cut), lofted),
        "{}, the loft {lofted}",
        volume(&cut)
    );

    for (a, b) in [(loft(), mirrored()), (mirrored(), loft())] {
        let common = boolean(a, b, BooleanOp::Intersect).unwrap();
        assert!(
            common.is_none(),
            "the loft and the box share {:?} mm3",
            common.map(|c| volume(&c))
        );
    }

    let u = boolean(loft(), mirrored(), BooleanOp::Union);
    assert!(
        matches!(u, Ok(Some(_))),
        "{:?}",
        u.as_ref().map(Option::is_some)
    );
    let u = u.unwrap().unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 2);
    assert_eq!(u.counts().unwrap(), [19, 39, 26]);
    assert!(
        near(volume(&u), lofted + boxed),
        "{}, the loft and the box {}",
        volume(&u),
        lofted + boxed
    );

    let (mirrored_record, _) = mirrored().to_record().unwrap();
    for e in &mirrored_record.edges {
        assert!(
            matches!(e.curve, solidcore_record::brep::Curve::Line { .. }),
            "{:?}",
            e.curve
        );
    }
    assert!(near(volume(&mirrored()), boxed), "{}", volume(&mirrored()));
}

/// Sets the calling thread's fixes: none, or every one (SolidCraft's set).
fn with_fixes(all: bool) {
    if all {
        all_fixes();
    } else {
        session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    }
}

/// `body` as SolidCraft hands it to the next feature: through its record.
fn round_tripped(body: &Body) -> Body {
    let (record, _) = body.to_record().unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// solidcraft#12785 (solidcore #13650): a block with a hexagonal pocket through it and a
/// round hole drilled from an end face along the pocket's axis plane, ending exactly at a
/// corner of the pocket, so the hole's bottom meets the two pocket walls along a segment of
/// the corner edge (a solid touching itself along a line). The cut leaves that segment as one
/// edge of four faces; a record round trip (`to_record`, `from_record`: SolidCraft's body
/// between features) writes it as two edges of two faces each, and filling the pocket with
/// the same hexagon as a join failed there (`topology_intersector::p1::relate`, status 1001,
/// or a result that is not a closed solid). Without the hole, or without the round trip, the
/// same join works. Carried from bherbruck's
/// `boolean_join_fills_pocket_with_hole_at_its_corner.rs`.
#[allow(clippy::expect_used, clippy::panic)] // test helpers
mod pocket_with_hole_at_its_corner {
    use std::f64::consts::TAU;

    use solidcore_ops::boolean::{boolean, BooleanOp};
    use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
    use solidcore_ops::{Body, CheckLevel};

    use super::{round_tripped, with_fixes};

    /// The hexagon's circumradius.
    const R: f64 = 6.3685;
    const Z: [f64; 3] = [0.0, 0.0, 1.0];

    fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
        Curve::Line { start, end }
    }

    /// The regular hexagon of circumradius `R` about the origin with a corner on +X, at z = 0
    /// (the reporter's sketch: the corner at (R, 0), the next at (R/2, R sqrt(3)/2)).
    fn hexagon() -> Vec<Curve> {
        const H: f64 = 5.515_282_784_001_197;
        let corners = [
            [R, 0.0, 0.0],
            [R / 2.0, H, 0.0],
            [-R / 2.0, H, 0.0],
            [-R, 0.0, 0.0],
            [-R / 2.0, -H, 0.0],
            [R / 2.0, -H, 0.0],
        ];
        (0..6)
            .map(|i| line(corners[i], corners[(i + 1) % 6]))
            .collect()
    }

    /// The block 100.29 x 32 x 10.
    fn block() -> Body {
        extrude(
            &[
                line([-50.145, -16.0, 0.0], [50.145, -16.0, 0.0]),
                line([50.145, -16.0, 0.0], [50.145, 16.0, 0.0]),
                line([50.145, 16.0, 0.0], [-50.145, 16.0, 0.0]),
                line([-50.145, 16.0, 0.0], [-50.145, -16.0, 0.0]),
            ],
            Z,
            10.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    }

    /// The hexagon extruded through the block's height: the pocket's cutter and the join's
    /// tool.
    fn tool() -> Body {
        extrude(&hexagon(), Z, 10.0, 0.0, DraftCorner::Curvature, true).unwrap()
    }

    /// The block with the pocket through it and the hole: 2.5 mm across, along -X from the
    /// end face x = 50.145, 43.7765 deep, so it ends at the pocket's corner (R, 0).
    fn target() -> Body {
        let pocketed = boolean(block(), tool(), BooleanOp::Subtract)
            .unwrap()
            .expect("the pocketed block");
        let hole = extrude(
            &[Curve::Arc {
                center: [50.145, 0.0, 5.0],
                axis: [-1.0, 0.0, 0.0],
                reference: [0.0, 1.0, 0.0],
                radius: 1.25,
                start: 0.0,
                end: TAU,
            }],
            [-1.0, 0.0, 0.0],
            50.145 - R,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        boolean(pocketed, hole, BooleanOp::Subtract)
            .unwrap()
            .expect("the drilled block")
    }

    /// Joins `target` and the hexagon, checks the result and compares its volume with the
    /// sum.
    fn join_fills(target: Body, what: &str) {
        let want = target.volume(1e-9).unwrap() + tool().volume(1e-9).unwrap();
        let b = match boolean(target, tool(), BooleanOp::Union) {
            Ok(Some(b)) => b,
            Ok(None) => panic!("{what}: no body"),
            Err(e) => panic!("{what}: {e}"),
        };
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{what}:\n{}", report.text);
        let got = b.volume(1e-9).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "{what}: volume {got} vs {want}"
        );
    }

    /// The cut's own body: the corner segment is one edge of four faces, and the join works.
    #[test]
    fn filling_a_pocket_whose_corner_a_hole_reaches_joins() {
        let target = target();
        assert_eq!(target.counts().unwrap(), [15, 37, 24]);
        join_fills(target, "the cut's body");
    }

    /// The same body after its record round trip: the corner segment is two coincident edges
    /// of two faces each, and the join works as well, with no fixes and with every fix.
    #[test]
    fn filling_a_pocket_whose_corner_a_hole_reaches_joins_after_a_record_round_trip() {
        for all in [false, true] {
            with_fixes(all);
            let target = round_tripped(&target());
            assert_eq!(target.counts().unwrap(), [15, 38, 24]);
            assert!(target.check(CheckLevel::All).unwrap().is_valid());
            join_fills(target, &format!("the round-tripped body, all fixes {all}"));
        }
    }

    /// The control: without the hole the same join works, round trip or not.
    #[test]
    fn filling_a_pocket_without_the_hole_joins() {
        let pocketed = boolean(block(), tool(), BooleanOp::Subtract)
            .unwrap()
            .expect("the pocketed block");
        join_fills(round_tripped(&pocketed), "the pocketed block");
    }
}

/// Two unit boxes touching along the edge x = y = 1 in one body (two solids), after a record
/// round trip (`split_shared` writes the edge once per solid, so the operand carries it as two
/// coincident edges, which the boolean glues back into the one edge the kernel's union shares).
/// Booleans on it give the exact volumes, whether the tool reaches the shared edge or not,
/// with no fixes and with every fix: subtract `[0.5, 1.5]^2 x [0, 0.5]` (an eighth of a unit
/// from each box, across the edge) leaves 1.75; intersect `[0.5, 1.5]^2 x [0, 1]` keeps 0.5;
/// union with a far unit box gives 3.
#[test]
fn two_solids_touching_along_an_edge_survive_a_round_trip_and_booleans() {
    for all in [false, true] {
        with_fixes(all);
        let touching = || {
            let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
            let b = make_box([1.0, 1.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
            round_tripped(&boolean(a, b, BooleanOp::Union).unwrap().unwrap())
        };
        let cases = [
            (
                BooleanOp::Subtract,
                make_box([0.5, 0.5, 0.0], [1.0, 1.0, 0.5]).unwrap(),
                1.75,
            ),
            (
                BooleanOp::Intersect,
                make_box([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]).unwrap(),
                0.5,
            ),
            (
                BooleanOp::Union,
                make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap(),
                3.0,
            ),
        ];
        for (op, tool, want) in cases {
            let r = boolean(touching(), tool, op).unwrap().unwrap();
            let report = r.check(CheckLevel::All).unwrap();
            assert!(
                report.is_valid(),
                "all fixes {all}, {op:?}:\n{}",
                report.text
            );
            assert!(
                near(volume(&r), want),
                "all fixes {all}, {op:?}: {} vs {want}",
                volume(&r)
            );
        }
    }
}

/// An 80 x 75 x 48 block (x from 0, y from -75, z from 0) less a stadium through it along y:
/// two half circles of radius 16 centred at x = 21 and 59, joined by flat sides 32 apart, its
/// bottom side at z = `bottom`. The stadium's flat bottom meets each half circle at a vertex
/// where the two are tangent.
fn block_less_stadium(bottom: f64) -> Body {
    let (low, high, centre) = (bottom, bottom + 32.0, bottom + 16.0);
    let arc = |x: f64, start: f64| Curve::Arc {
        center: [x, 5.0, centre],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: 16.0,
        start,
        end: start + PI,
    };
    let profile = [
        arc(21.0, PI / 2.0),
        Curve::Line {
            start: [21.0, 5.0, low],
            end: [59.0, 5.0, low],
        },
        arc(59.0, -PI / 2.0),
        Curve::Line {
            start: [59.0, 5.0, high],
            end: [21.0, 5.0, high],
        },
    ];
    let stadium = extrude(
        &profile,
        [0.0, -1.0, 0.0],
        85.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let block = make_box([0.0, -75.0, 0.0], [80.0, 75.0, 48.0]).unwrap();
    boolean(block, stadium, BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// The stadium's area, 38 x 32 + pi 16^2.
fn stadium_area() -> f64 {
    38.0 * 32.0 + PI * 16.0 * 16.0
}

/// The block less the stadium whose bottom lies `h` below z = 12, less the rest of the block
/// from z = 12 up over y from -65 to 0: the stadium's part above z = 12 is its area less the
/// strip 38 h of its flat side and the two half segments of depth h of its circles, one whole
/// segment r^2 acos((r - h) / r) - (r - h) sqrt(2 r h - h^2).
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn block_less_stadium_and_cut(h: f64) -> f64 {
    let r = 16.0;
    let segment = r * r * ((r - h) / r).acos() - (r - h) * (2.0 * r * h - h * h).sqrt();
    let above = stadium_area() - 38.0 * h - segment;
    80.0 * 75.0 * 48.0 - 75.0 * stadium_area() - 65.0 * (80.0 * 36.0 - above)
}

fn one_valid_solid_of_volume(r: &Body, counts: [usize; 3], want: f64, rel: f64) {
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    assert_eq!(r.counts().unwrap(), counts);
    let v = r.volume(1e-11).unwrap();
    assert!(
        (v - want).abs() <= rel * want,
        "volume {v}, closed form {want}"
    );
}

/// A box cut from z = 12 up, its bottom face on the stadium's flat bottom, which lies 3.756e-11
/// below it (as a sketch solve leaves it, #16237), and its back face flush with the block's. The
/// line where the box's bottom meets the block's back face runs along the stadium's bottom edge
/// and leaves it at the vertices where the edge and the half circles are tangent, into the back
/// face. The classification of that line against the back face stepped along it to settle the
/// tangency and read the stepped point against the stadium's loop, a hole: a point outside a
/// hole was taken for outside the face, the line was not imprinted there, and the merge failed
/// ("no radial edge use classifies the face's edge use"). A box past every side of the block
/// cut nothing instead, a valid body of the wrong volume. Both leave the closed form.
#[test]
fn a_cut_whose_bottom_is_a_hair_above_a_stadium_holes_flat_side_removes_the_rest() {
    all_fixes();
    let h = 3.756e-11;
    let want = block_less_stadium_and_cut(h);
    let flush = make_box([0.0, -65.0, 12.0], [80.0, 65.0, 36.0]).unwrap();
    let r = boolean(block_less_stadium(12.0 - h), flush, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&r, [11, 30, 20], want, 1e-9);
    let past = make_box([-5.0, -65.0, 12.0], [90.0, 70.0, 40.0]).unwrap();
    let r = boolean(block_less_stadium(12.0 - h), past, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&r, [11, 30, 20], want, 1e-9);
}

/// The real defect beside it: a cylinder through the stadium whose circle is tangent to the
/// stadium's flat bottom and its half circle at their common vertex, of a smaller radius, so it
/// bends into the hole there. It lies in the hole and cuts nothing: the classification still
/// takes the curves leaving that vertex into the hole for outside the back face.
#[test]
fn a_cylinder_in_a_stadium_hole_tangent_at_its_vertex_cuts_nothing() {
    all_fixes();
    let h = 3.756e-11;
    let want = 80.0 * 75.0 * 48.0 - 75.0 * stadium_area();
    for r in [15.0, 15.9, 15.99] {
        let circle = [Curve::Arc {
            center: [59.0, 0.0, 12.0 - h + r],
            axis: [0.0, -1.0, 0.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start: 0.0,
            end: TAU,
        }];
        let cylinder = extrude(
            &circle,
            [0.0, -1.0, 0.0],
            65.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        let cut = boolean(block_less_stadium(12.0 - h), cylinder, BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        one_valid_solid_of_volume(&cut, [10, 24, 16], want, 1e-9);
    }
}

/// The operand records of one of SolidCraft's failing cuts (`data/<name>.bodies.json.gz`), by
/// name.
fn traced_operands(name: &str) -> impl Fn(&str) -> Body {
    let path = format!(
        "{}/tests/data/{name}.bodies.json.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: Value = serde_json::from_str(&text).unwrap();
    move |key: &str| {
        let record: Record = serde_json::from_value(records[key].clone()).unwrap();
        Body::from_record(&record, true).unwrap().0
    }
}

/// #16237's cut as SolidCraft passed it: the block less a stadium sketched with its bottom
/// 3.756e-11 below z = 12, less the rectangle cut from z = 12 up, flush with the block's top and
/// back. It failed in the merge; it is the closed form of the block, the stadium and the cut.
#[test]
fn a_traced_cut_flush_with_an_earlier_stadium_cut_is_its_closed_form() {
    all_fixes();
    let body = traced_operands("issue_16237_cut_flush_with_earlier_cut");
    let r = boolean(body("target"), body("tool"), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(
        &r,
        [11, 30, 20],
        block_less_stadium_and_cut(3.756e-11),
        1e-9,
    );
}

/// #15700's operands from the reporter's files: `cavity` (folds over itself at its nose),
/// `front` (the cavity cut back to x < 20) and `tool` (overlaps the nose).
fn folded_cavity() -> impl Fn(&str) -> Body {
    traced_operands("folded_cavity_and_nose_tool")
}

/// The kernel misses where the tool meets the cavity's fold and answered empty, so the cavity
/// vanished without an error. It is refused, or keeps at least the cavity less the tool.
#[test]
fn a_cut_answered_empty_while_the_body_has_a_vertex_outside_the_tool_is_refused() {
    all_fixes();
    let body = folded_cavity();
    let (vc, vt) = (volume(&body("cavity")), volume(&body("tool")));
    match boolean(body("cavity"), body("tool"), BooleanOp::Subtract) {
        Err(e) => assert_eq!(e.name(), "EmptyRefuted", "{e}"),
        Ok(Some(r)) => {
            let v = volume(&r);
            assert!(v >= vc - vt && v <= vc, "{v} of {vc} less at most {vt}");
        }
        other => panic!("the cavity less the tool: {:?}", other.map(|r| r.is_some())),
    }
}

/// The same missed crossing made a real overlap "empty". It is refused, or is no larger than
/// the tool.
#[test]
fn an_intersection_answered_empty_while_a_vertex_lies_in_the_other_operand_is_refused() {
    all_fixes();
    let body = folded_cavity();
    let tool = body("tool");
    match boolean(body("front"), body("tool"), BooleanOp::Intersect) {
        Err(e) => assert_eq!(e.name(), "EmptyRefuted", "{e}"),
        Ok(Some(r)) => {
            let (v, vt) = (volume(&r), volume(&tool));
            assert!(v > 0.0 && v <= vt, "{v} of at most {vt}");
        }
        other => panic!(
            "the front of the cavity and the tool: {:?}",
            other.map(|r| r.is_some())
        ),
    }
}

#[test]
fn a_refused_empty_cut_names_a_vertex_of_the_body_outside_the_tool() {
    all_fixes();
    let body = folded_cavity();
    let (vc, vt) = (volume(&body("cavity")), volume(&body("tool")));
    match boolean(body("cavity"), body("tool"), BooleanOp::Subtract) {
        Err(Error::EmptyRefuted { at }) => {
            assert_eq!(
                body("cavity").classify_point(at).unwrap(),
                PointClass::OnBoundary
            );
            assert_eq!(
                body("tool").classify_point(at).unwrap(),
                PointClass::Outside
            );
        }
        Ok(Some(r)) => {
            let v = volume(&r);
            assert!(v >= vc - vt && v <= vc, "{v} of {vc} less at most {vt}");
        }
        other => panic!("{:?}", other.map(|r| r.is_some())),
    }
}

#[test]
fn truly_empty_differences_and_intersections_are_not_refuted() {
    all_fixes();
    let inner = || make_box([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]).unwrap();
    let outer = || make_box([0.0, 0.0, 0.0], [3.0, 3.0, 3.0]).unwrap();
    let apart = || make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    assert!(boolean(inner(), outer(), BooleanOp::Subtract)
        .unwrap()
        .is_none());
    assert!(boolean(inner(), apart(), BooleanOp::Intersect)
        .unwrap()
        .is_none());
}

/// #15247's second cut as SolidCraft passed it: a plate already cut by one half of a 0.8 mm
/// outline wall around an arrow, less the other half, which meets the faces the first cut left.
/// It failed in the merge. The tool lies inside the plate, so the cut removes exactly its volume.
#[test]
fn a_traced_cut_meeting_an_earlier_cut_removes_its_tool() {
    all_fixes();
    let body = traced_operands("issue_15247_cut_arrow_outline");
    let want = body("target").volume(1e-11).unwrap() - body("tool").volume(1e-11).unwrap();
    let r = boolean(body("target"), body("tool"), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&r, [46, 132, 88], want, 1e-9);
}

/// #14599's cut as SolidCraft passed it: a 0.3 x 6 x 5 rib less a tool as long as the rib, whose
/// profile is the rib's top, its side x = -19.1 down to z = 2.5 and a round of radius 10.5667
/// tangent to that side at z = 2.5. The rib's side runs along the round's tangent line at the
/// front and lies 5.8e-10 inside the round at the back, where the two crossed 2.2e-4 apart; the
/// cut was not a solid. The tool lies inside the rib, so the cut removes exactly its volume and
/// leaves the rib's six faces with the round in place of its top.
#[test]
fn a_traced_cut_whose_round_is_tangent_to_a_thin_ribs_side_removes_its_tool() {
    all_fixes();
    let body = traced_operands("issue_14599_cut_flush_with_a_thin_rib");
    let want = body("target").volume(1e-11).unwrap() - body("tool").volume(1e-11).unwrap();
    let r = boolean(body("target"), body("tool"), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&r, [6, 12, 8], want, 1e-9);
    // the vertices are the rib's bottom corners, its top corners on the far side and the
    // round's tangent points at z = 2.5, none at the second crossing 1.1e-4 above them
    let (_, brep) = common::kernel_copy(&r);
    for &v in brep.vertex_list() {
        let z = solidcore::query::api_queries::vertex_get_point(&brep, v)
            .unwrap()
            .z;
        assert!(
            [0.0, 2.5, 5.0].iter().any(|h| (z - h).abs() < 1e-6),
            "a vertex at z = {z}"
        );
    }
}

/// #16935's intersect as SolidCraft passed it. Both bodies are a decagon of side 10, 10 tall, with
/// a 6 tall top tapered at 45°. The target has a 6 mm groove down the middle of each slope, its
/// edges rounded at 1 mm; the tool has the ten edges between the slopes rounded at 5 mm. Near the
/// top each 5 mm round's cylinder runs within the tolerance of a 1 mm round and the corner patch
/// beside it, so each of the twenty gaps it leaves in the intersection loop is closed along the
/// target's edge between them. Ten passes of one closure each left ten of them open, and the
/// first closure taken at a corner ran up the round's tangent line: the faces were classified
/// across the gaps, the intersect lost eight of the ten 5 mm rounds and said nothing.
///
/// The intersection is the target less the ten wedges between the tool's sharp edges and its
/// rounds, `V(sharp tool) - V(tool)`, plus what the target's 1 mm rounds and their corner
/// patches already took out of those wedges near the top: under 1e-3 (the wedges classified
/// against the target, column by column along the slopes' normals). Before, the intersect gave
/// 6.38 more: eight wedges kept.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_traced_intersect_keeps_every_5_mm_round_beside_the_grooves_1_mm_rounds() {
    all_fixes();
    let body = traced_operands("issue_16935_hip_rounds_over_groove_rounds");
    // the decagon's apothem and area, the top's apothem 6 less (45°), the frustum on the prism
    let apothem = 10.0 / (2.0 * 18_f64.to_radians().tan());
    let base = 10.0 * apothem * apothem * 18_f64.to_radians().tan();
    let top = base * ((apothem - 6.0) / apothem).powi(2);
    let sharp_tool = 10.0 * base + 6.0 / 3.0 * (base + top + (base * top).sqrt());
    let wedges = sharp_tool - body("tool").volume(1e-11).unwrap();
    let want = body("target").volume(1e-11).unwrap() - wedges;
    let r = boolean(body("target"), body("tool"), BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    // the target's 92 faces and the ten rounds
    assert_eq!(r.counts().unwrap(), [102, 320, 220]);
    let v = r.volume(1e-11).unwrap();
    assert!(
        v >= want && v <= want + 1e-3,
        "volume {v}, target less the wedges {want}"
    );
}

/// #14477's join as SolidCraft passed it: a 14 x 20 x 35 block with a 3.2 mm hole whose rim on
/// the y = -20 face is chamfered 0.5 mm, joined with that face extruded 10 mm (a slab with a
/// 4.2 mm hole, the chamfer's outer rim). The two touch on the face; the join is their sum.
#[test]
fn a_traced_join_onto_a_chamfered_holes_face_is_their_sum() {
    all_fixes();
    let body = traced_operands("issue_14477_join_over_chamfered_hole");
    // the block less the 1.6 hole and the chamfer's ring (Pappus), the slab less its 2.1 hole
    let block = 14.0 * 20.0 * 35.0 - PI * 1.6 * 1.6 * 20.0 - TAU * (1.6 + 0.5 / 3.0) * 0.125;
    let slab = 14.0 * 10.0 * 35.0 - PI * 2.1 * 2.1 * 10.0;
    let r = boolean(body("target"), body("tool"), BooleanOp::Union)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&r, [9, 19, 12], block + slab, 1e-9);
}

/// #17118's cut as SolidCraft passed it: the target of #16935 (its 1 mm groove rounds) less one
/// of the wedges between the plain decagon's sharp sloping edges and their 5 mm rounds. The
/// wedge's two faces are tangent at its knife edges, so within about 0.01 of them it is thinner
/// than the tolerance. Near the top, a vertex of the target where a groove round meets its corner
/// patch lies 0.0115 from a knife edge, on the wedge's plane and 1.3e-5 off its cylinder: the
/// round's edge on the plane and the patch's curve on the cylinder each imprinted a vertex for it
/// in the wedge, 2.4e-5 apart, and relating the target's vertex to the second failed with
/// "another entity is already related". Once the two are one vertex, a sliver of the round
/// between that vertex and the knife edge, within the tolerance of both of the wedge's faces,
/// was left to a ray and kept, so the result was not closed.
///
/// The cut removes the wedge less what the target's rounds and corner patches had already taken
/// out of it near the top: under 1e-4, a tenth of the 1e-3 bound for all ten wedges in the
/// intersect above.
#[test]
fn a_traced_cut_by_a_wedge_grazing_the_grooves_1_mm_rounds_removes_the_wedge() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    let want = body("target").volume(1e-11).unwrap() - body("tool").volume(1e-11).unwrap();
    let r = boolean(body("target"), body("tool"), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    // the target's 92 faces and the wedge's cylinder
    assert_eq!(r.counts().unwrap(), [93, 266, 175]);
    let v = r.volume(1e-11).unwrap();
    assert!(
        v >= want && v <= want + 1e-4,
        "volume {v}, target less the wedge {want}"
    );
}

/// #17118's cut repeated with all ten wedges, one at a time in the order SolidCraft cut them
/// (`wedge0` to `wedge9`), each cut on what the earlier ones left. The wedges are rotated copies,
/// but the faces are visited in a different order at each: where the sliver of a groove's round
/// in the wedge's thin edge first meets the wedge's plane before that plane is classified, the
/// two faces were not paired (they lie on surfaces of different kinds), the sliver was left to a
/// ray and kept: the fifth cut here, and the sixth wedge alone on the target, left bodies that
/// were not closed. The result is
/// the intersect above: the target less the ten wedges, plus what the target's rounds had already
/// taken out of them, under 1e-3.
#[test]
fn a_traced_cut_by_each_of_ten_wedges_in_turn_is_the_target_less_the_wedges() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    let mut r = body("target");
    let mut want = r.volume(1e-11).unwrap();
    for i in 0..10 {
        let wedge = body(&format!("wedge{i}"));
        want -= wedge.volume(1e-11).unwrap();
        r = boolean(r, wedge, BooleanOp::Subtract).unwrap().unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "after wedge {i}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "after wedge {i}");
    }
    // the target's 92 faces and the ten cylinders, as the intersect gives
    assert_eq!(r.counts().unwrap(), [102, 320, 220]);
    let v = r.volume(1e-11).unwrap();
    assert!(
        v >= want && v <= want + 1e-3,
        "volume {v}, target less the wedges {want}"
    );
}

/// The ten wedges of the test above joined to the target one at a time. Each wedge reaches out
/// of the target only where the target's groove rounds and corner patches dip below the slopes,
/// so each join adds the part the cut leaves behind, 6.8e-5. The sliver of a groove's round in
/// a wedge's thin edge lies on the wedge's plane within the tolerance; the plane is kept, so
/// the sliver goes. Classified as a coincident face the join keeps, it was kept beside the
/// plane, and the first join left a body that was not closed; the face count and the check below
/// catch that.
///
/// The volume bound is the precision of the volume of these traced bodies, not a tolerance on the
/// joins. The bodies' edges may end anywhere within the tolerance of their vertices: the sixth
/// wedge as traced (`wedge5`) has an arc on the top face that ends 4.5e-7 short of its vertex,
/// where `wedge0`'s ends on it. The join keeps that arc, so the result's top face is open by that
/// gap, and its integrated area is 2.1e-6 larger than in the first join (half the cross product of
/// the arc's end with the gap); the volume integration over the open loops then depends on the
/// point it is taken about. Moved 40 above the centre of the bounding box, where `Body::volume`
/// takes it, that point changes the target's own volume by 1.3e-4 and what the sixth join adds by
/// 1.8e-4. So nine joins add 7.3e-5 each, the sixth 1.0e-4 alone and 2.0e-4 after the other five,
/// and the bound allows 3e-5 a wedge. The test after this one checks each join's vertices
/// against the first join's, which the volume cannot do to better than that.
#[test]
fn a_traced_join_of_each_of_ten_wedges_in_turn_adds_what_lies_outside_the_target() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    let target = body("target").volume(1e-11).unwrap();
    // what the first wedge leaves when it is cut from the target: the part outside it
    let wedge = body("wedge0");
    let inside = target
        - boolean(body("target"), body("wedge0"), BooleanOp::Subtract)
            .unwrap()
            .unwrap()
            .volume(1e-11)
            .unwrap();
    let outside = wedge.volume(1e-11).unwrap() - inside;
    let mut r = body("target");
    for i in 0..10 {
        r = boolean(r, body(&format!("wedge{i}")), BooleanOp::Union)
            .unwrap()
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "after wedge {i}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "after wedge {i}");
    }
    // the target's 92 faces and two faces of each wedge, its cylinder and its plane's piece
    assert_eq!(r.counts().unwrap()[0], 112);
    let added = r.volume(1e-11).unwrap() - target;
    assert!(
        (added - 10.0 * outside).abs() <= 10.0 * 3e-5,
        "added {added}, ten times the part of a wedge outside the target {outside}"
    );
}

/// The ten wedges of the test above, each joined alone to the target. The wedges and the target
/// are ten turns of one part about the z axis, so each join, turned back by its wedge's multiple
/// of 36 degrees, must have the first join's counts and its vertices, each within 1e-6 (a tenth of
/// the tolerance) of one of the first join's and the other way round. This checks the geometry of
/// every join directly, where the volume bound above only measures it as precisely as the traced
/// bodies' open loops allow.
#[test]
#[allow(clippy::disallowed_methods)] // the test's comparison, not ops computing geometry
fn a_traced_join_of_each_wedge_alone_is_the_first_join_turned_into_place() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    let angle = |b: &Body| {
        let [x, y, _] = b.mass_properties(1e-11).unwrap().centroid;
        y.atan2(x)
    };
    let joined = |i: usize| {
        let wedge = body(&format!("wedge{i}"));
        let turn = angle(&wedge);
        let r = boolean(body("target"), wedge, BooleanOp::Union)
            .unwrap()
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "wedge {i}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "wedge {i}");
        (r, turn)
    };
    // each vertex of the join, turned about z by `turn`
    let vertices = |r: &Body, turn: f64| -> Vec<[f64; 3]> {
        let (_, brep) = common::kernel_copy(r);
        let (s, c) = turn.sin_cos();
        brep.vertices()
            .ids()
            .map(|v| {
                let p = brep.vertex(v).unwrap().point;
                [c * p.x - s * p.y, s * p.x + c * p.y, p.z]
            })
            .collect()
    };
    let nearest = |p: &[f64; 3], set: &[[f64; 3]]| {
        set.iter()
            .map(|q| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt())
            .fold(f64::INFINITY, f64::min)
    };
    let (first, first_turn) = joined(0);
    let counts = first.counts().unwrap();
    let want = vertices(&first, 0.0);
    for i in 1..10 {
        let (r, turn) = joined(i);
        assert_eq!(r.counts().unwrap(), counts, "wedge {i}");
        // the wedge's whole number of tenths of a turn from the first
        let steps = ((turn - first_turn) / (TAU / 10.0)).round();
        let got = vertices(&r, -steps * TAU / 10.0);
        for (a, b, which) in [
            (&got, &want, "of this join"),
            (&want, &got, "of the first join"),
        ] {
            for p in a {
                let d = nearest(p, b);
                assert!(
                    d <= 1e-6,
                    "wedge {i}: a vertex {which} at {p:?} is {d} from the other"
                );
            }
        }
    }
}

/// The ten wedges joined to the target one at a time, compared with each joined alone. Each
/// wedge changes the target only near itself, so the vertices of the joint result are the
/// target's, less those each join alone removed and plus those each join alone added, and the
/// same holds for the midpoints of the edges. Every point must be within 1e-6 (a tenth of the
/// tolerance) of one expected and the other way round, which checks the faces' trims in the
/// sequential result without its volume.
#[test]
#[allow(clippy::disallowed_methods)] // the test's comparison, not ops computing geometry
fn a_traced_join_of_the_ten_wedges_in_turn_has_the_vertices_and_edges_of_each_join_alone() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    // the body's vertices, then the midpoints of its edges
    let points = |r: &Body| -> [Vec<[f64; 3]>; 2] {
        let (k, brep) = common::kernel_copy(r);
        let vertices = brep
            .vertices()
            .ids()
            .map(|v| {
                let p = brep.vertex(v).unwrap().point;
                [p.x, p.y, p.z]
            })
            .collect();
        let midpoints = brep
            .edges()
            .ids()
            .filter_map(|e| {
                let edge = brep.edge(e).unwrap();
                let p = edge
                    .curve
                    .as_ref()?
                    .evaluate_point(&k, edge.interval.evaluate(0.5))
                    .unwrap();
                Some([p.x, p.y, p.z])
            })
            .collect();
        [vertices, midpoints]
    };
    let near = |p: &[f64; 3], set: &[[f64; 3]]| {
        set.iter().any(|q| {
            ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() <= 1e-6
        })
    };
    let target = points(&body("target"));
    let mut want = target.clone();
    let mut r = body("target");
    for i in 0..10 {
        let wedge = || body(&format!("wedge{i}"));
        let alone = points(
            &boolean(body("target"), wedge(), BooleanOp::Union)
                .unwrap()
                .unwrap(),
        );
        for kind in 0..2 {
            let removed: Vec<_> = target[kind]
                .iter()
                .filter(|p| !near(p, &alone[kind]))
                .copied()
                .collect();
            want[kind].retain(|p| !near(p, &removed));
            want[kind].extend(alone[kind].iter().filter(|p| !near(p, &target[kind])));
        }
        r = boolean(r, wedge(), BooleanOp::Union).unwrap().unwrap();
    }
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let got = points(&r);
    for (kind, name) in [(0, "vertex"), (1, "edge midpoint")] {
        assert_eq!(got[kind].len(), want[kind].len(), "{name}s");
        for (a, b, which) in [
            (&got[kind], &want[kind], "of the result is not expected"),
            (&want[kind], &got[kind], "expected is not in the result"),
        ] {
            for p in a {
                assert!(near(p, b), "a {name} {which}: {p:?}");
            }
        }
    }
}

/// The ten wedges joined to the target one at a time, checked against the operands alone, with
/// nothing taken from a join. The boundary of a union lies on the boundaries of its operands and
/// inside none of them. So every point sampled from the result, nine along each edge's curve and
/// one on each face (the face's point closest to the centroid of its area), must classify on the
/// boundary of the target or of a wedge, and inside none of the eleven. A face kept on the wrong
/// surface, a trim curve that leaves its surfaces, or a piece kept where another operand covers it
/// fails this, whatever the volume and the counts say.
#[test]
#[allow(clippy::disallowed_methods)] // sampling the result to check it, not ops computing geometry
fn a_traced_join_of_the_ten_wedges_in_turn_lies_on_its_operands_and_inside_none() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    let operands: Vec<(String, Body)> = std::iter::once("target".to_string())
        .chain((0..10).map(|i| format!("wedge{i}")))
        .map(|name| {
            let b = body(&name);
            (name, b)
        })
        .collect();
    let mut r = body("target");
    for i in 0..10 {
        r = boolean(r, body(&format!("wedge{i}")), BooleanOp::Union)
            .unwrap()
            .unwrap();
    }
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);

    // nine points along each edge's curve, over its interval
    let (k, brep) = common::kernel_copy(&r);
    let mut points: Vec<([f64; 3], String)> = Vec::new();
    for e in brep.edges().ids() {
        let edge = brep.edge(e).unwrap();
        let Some(curve) = edge.curve.as_ref() else {
            continue;
        };
        for j in 1..10 {
            let p = curve
                .evaluate_point(&k, edge.interval.evaluate(f64::from(j) / 10.0))
                .unwrap();
            points.push(([p.x, p.y, p.z], format!("edge {e:?} at {j}/10")));
        }
    }
    // one point on each face: its point closest to the centroid of its area
    let (record, order) = r.to_record().unwrap();
    for f in 0..record.faces.len() {
        let c = r.face_centroid(&order, f, 1e-6).unwrap().centroid;
        let on = r
            .closest_to_point(&order, &[f], &[], c)
            .unwrap()
            .unwrap()
            .to;
        points.push((on, format!("face {f}")));
    }
    assert!(points.len() > 1000, "{} points", points.len());

    let classes: Vec<Vec<PointClass>> = operands
        .iter()
        .map(|(_, b)| {
            b.classify_points(&points.iter().map(|(p, _)| *p).collect::<Vec<_>>())
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect()
        })
        .collect();
    for (n, (p, from)) in points.iter().enumerate() {
        let inside: Vec<_> = operands
            .iter()
            .zip(&classes)
            .filter(|(_, c)| c[n] == PointClass::Inside)
            .map(|((name, _), _)| name.as_str())
            .collect();
        assert!(inside.is_empty(), "{from} at {p:?} is inside {inside:?}");
        assert!(
            classes.iter().any(|c| c[n] == PointClass::OnBoundary),
            "{from} at {p:?} is on no operand's boundary"
        );
    }
}

/// A 10 mm cube with the vertical edge at x = 0, y = 0 rounded to radius 2.
fn filleted_cube() -> Body {
    let block = make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
    let edge = common::edge_at(&block, [0.0, 0.0, 5.0]);
    blend(&block, Blend::Round, &[edge], &[2.0], &[], Corners::Filled).unwrap();
    block
}

/// A 6 x 10 x 5 slab through the cube's front, its near face `dx` past the fillet spring at x = 2.
fn slab_past_spring(dx: f64) -> Body {
    make_box([2.0 + dx, -5.0, 0.0], [6.0, 10.0, 5.0]).unwrap()
}

/// Every vertex under the intersection tolerance of each of its faces, none wider than `max_tol`.
fn vertex_gaps_within_xsect(body: &Body, max_tol: f64) {
    let (k, mut brep) = common::kernel_copy(body);
    let xsect = corner::xsect_tol_3d_of_brep(&k, &brep);
    for v in brep.vertices().ids().collect::<Vec<_>>() {
        let (_, gap, _) = corner::is_within_xsect_tol_3d_of_faces(&k, &mut brep, v, true).unwrap();
        assert!(gap < xsect, "vertex gap {gap} vs {xsect}");
        let tol = brep.vertex(v).unwrap().tolerance;
        assert!(tol <= max_tol, "vertex tolerance {tol}");
    }
}

/// #16532: a slab whose near face is `dx` past a fillet spring. The cube less one rounded edge is
/// `960 + 10 PI`; the overlap is the slab's 6 by 5 section over the 5 mm both bodies share, 150
/// (the offset keeps both of the slab's x faces inside the cube). The result must check valid and
/// no vertex may grow past 5e-5.
///
/// The volume bound is derived from the move. The join places the vertex between the round and
/// the slab face within `dx` of the slab face (the round meets the flat face at x = 2, the slab
/// face stands at x = 2 + dx), so the notch's near face is displaced by at most `dx`, and
/// displacing a face by `d` changes the volume by at most `d` times its area. That face is the
/// slab's x face inside the cube: `width` (the slab's y extent -5..5 cut to the cube's 0..10,
/// so 5) by `height` (z 0..5, so 5). The bound is that, plus the file's `near` tolerance for the
/// volume integral itself.
fn slab_past_spring_case(dx: f64, op: BooleanOp) {
    all_fixes();
    let block = filleted_cube();
    let report = block.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let fillet = 960.0 + 10.0 * PI;
    let expected = match op {
        BooleanOp::Union => fillet + 300.0 - 150.0,
        BooleanOp::Subtract => fillet - 150.0,
        BooleanOp::Intersect => 150.0,
    };
    let built = boolean(filleted_cube(), slab_past_spring(dx), op);
    assert!(built.is_ok(), "{op:?} dx {dx}: {:?}", built.as_ref().err());
    let result = built.ok().flatten();
    assert!(result.is_some(), "{op:?} dx {dx}: the boolean left no body");
    let result = result.unwrap();
    let report = result.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{op:?} dx {dx}: {}", report.text);
    let got = volume(&result);
    let (width, height) = (5.0, 5.0);
    let moved_volume = dx * width * height;
    assert!(
        near(got, expected) || (got - expected).abs() <= moved_volume + 1e-7 * expected.abs(),
        "{op:?} dx {dx}: {got} vs {expected}, bound {moved_volume}"
    );
    vertex_gaps_within_xsect(&result, 5e-5 + 1e-12);
}

macro_rules! slab_past_spring_cases {
    ($($name:ident: $dx:expr, $op:ident;)*) => {$(
        #[test]
        fn $name() {
            slab_past_spring_case($dx, BooleanOp::$op);
        }
    )*};
}

slab_past_spring_cases! {
    issue_16532_union_slab_on_the_spring: 0.0, Union;
    issue_16532_union_slab_2e5_past_the_spring: 2e-5, Union;
    issue_16532_union_slab_2_5e5_past_the_spring: 2.5e-5, Union;
    issue_16532_subtract_slab_on_the_spring: 0.0, Subtract;
    issue_16532_subtract_slab_2e5_past_the_spring: 2e-5, Subtract;
    issue_16532_subtract_slab_2_5e5_past_the_spring: 2.5e-5, Subtract;
    issue_16532_intersect_slab_on_the_spring: 0.0, Intersect;
    issue_16532_intersect_slab_2e5_past_the_spring: 2e-5, Intersect;
    issue_16532_intersect_slab_2_5e5_past_the_spring: 2.5e-5, Intersect;
}

/// A rectangle `size` from the origin at height `z`, as four lines.
fn rectangle_at(z: f64, size: [f64; 2]) -> Vec<Curve> {
    let [w, h] = size;
    let corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
    (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            Curve::Line {
                start: [a[0], a[1], z],
                end: [b[0], b[1], z],
            }
        })
        .collect()
}

/// #17285: a 40 x 30 x 6 plate with a 4 mm hole at (10, 10) cut through it, joined to a 10 mm
/// plate on its top whose outline is the same rectangle and the same hole, drawn as two half
/// arcs split at `split` degrees and joined into one circle clockwise from +X, as SolidCraft
/// sweeps a circle drawn in pieces. The two bores' rims at z = 6 are one circle run opposite
/// ways. The join's face of the top bore took the trim of its rim from the top plate's own
/// rim, which the points at its ends and middle (the same either way round) took for a copy of
/// the bottom bore's rim: the trim ran backwards along the use, the loop jumped a period on the
/// cylinder (SMLib's UVEdge/UVEdge gap) and the two bores stayed two faces. The join is the 40 x
/// 30 x 16 block less the hole, its bores one face: 7 faces, 15 edges, 10 vertices.
fn join_on_a_holed_plate(split: f64) -> Body {
    let hole = extrude(
        &[Curve::Arc {
            center: [10.0, 10.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 2.0,
            start: 0.0,
            end: TAU,
        }],
        [0.0, 0.0, 1.0],
        6.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let plate = extrude(
        &rectangle_at(0.0, [40.0, 30.0]),
        [0.0, 0.0, 1.0],
        6.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let holed = boolean(plate, hole, BooleanOp::Subtract).unwrap().unwrap();
    let a = split * PI / 180.0;
    let half = |start: f64| Curve::Arc {
        center: [10.0, 10.0, 6.0],
        axis: [0.0, 0.0, -1.0],
        reference: [1.0, 0.0, 0.0],
        radius: 2.0,
        start,
        end: start + PI,
    };
    let circle = join_arcs(
        &[half(-a), half(-a - PI)],
        Join::ClosedAtFrameStart,
        zone_tolerance().unwrap(),
    )
    .unwrap();
    let mut outline = rectangle_at(6.0, [40.0, 30.0]);
    outline.push(circle);
    let top = extrude(
        &outline,
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    boolean(holed, top, BooleanOp::Union).unwrap().unwrap()
}

#[test]
fn a_join_whose_hole_lines_up_with_a_bore_run_the_other_way_is_one_bore() {
    all_fixes();
    let want = 40.0 * 30.0 * 16.0 - PI * 4.0 * 16.0;
    for split in [0.0, 60.0, 135.0] {
        one_valid_solid_of_volume(&join_on_a_holed_plate(split), [7, 15, 10], want, 1e-9);
    }
}

/// A counter-clockwise arc from `a` to `b` about `c`, of radius |a − c|.
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn arc_through(a: Vec2, b: Vec2, c: Vec2) -> Seg2 {
    let start = (a.y - c.y).atan2(a.x - c.x);
    let mut end = (b.y - c.y).atan2(b.x - c.x);
    while end <= start {
        end += TAU;
    }
    Seg2::Arc {
        center: c,
        radius: ((a.x - c.x).powi(2) + (a.y - c.y).powi(2)).sqrt(),
        start,
        sweep: end - start,
    }
}

/// `l` run the other way round.
fn reversed(l: Loop2) -> Loop2 {
    let segs = l.segs.into_iter().rev().map(|s| match s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => Seg2::Arc {
            center,
            radius,
            start: start + sweep,
            sweep: -sweep,
        },
        s => s,
    });
    Loop2 {
        segs: segs.collect(),
    }
}

/// The two bodies of tangency-labs/solidcraft#11749. A half pipe along x from -40 to 40: the
/// region 23 ≤ r ≤ 25, z ≥ 0 of the YZ plane, closed by two half discs below z = 0 whose centres
/// sit at (±24, -0.005) (the sketch's numbers), so their radius is 1.0000125 and they bulge
/// 1.25e-5 past y = ±25. And a ring from z = -h to h: an 84 × 54 rectangle less a rounded
/// rectangle, its sides y = ±25 and x = ±40 joined by corner arcs of radius 24 about (±16, ±1).
fn half_pipe_and_ring(h: f64) -> (Body, Body) {
    let v = Vec2::new;
    let dz = -0.005_000_000_000_023_874;
    let inner = reversed(Loop2 {
        segs: vec![arc_through(v(23.0, 0.0), v(-23.0, 0.0), v(0.0, 0.0))],
    });
    let pipe = Loop2 {
        segs: [
            vec![
                arc_through(v(25.0, 0.0), v(-25.0, 0.0), v(0.0, 0.0)),
                arc_through(v(-25.0, 0.0), v(-23.0, 0.0), v(-24.0, dz)),
            ],
            inner.segs,
            vec![arc_through(v(23.0, 0.0), v(25.0, 0.0), v(24.0, dz))],
        ]
        .concat(),
    };
    let pipe = Region2 {
        outer: pipe,
        holes: vec![],
    };
    let arc = |c: Vec2, start: f64| Seg2::Arc {
        center: c,
        radius: 24.0,
        start,
        sweep: PI / 2.0,
    };
    let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
    let hole = Loop2 {
        segs: vec![
            line(v(-16.0, -25.0), v(16.0, -25.0)),
            arc(v(16.0, -1.0), -PI / 2.0),
            line(v(40.0, -1.0), v(40.0, 1.0)),
            arc(v(16.0, 1.0), 0.0),
            line(v(16.0, 25.0), v(-16.0, 25.0)),
            arc(v(-16.0, 1.0), PI / 2.0),
            line(v(-40.0, 1.0), v(-40.0, -1.0)),
            arc(v(-16.0, -1.0), PI),
        ],
    };
    let ring = Region2 {
        outer: Loop2::polygon(&[
            v(-42.0, -27.0),
            v(42.0, -27.0),
            v(42.0, 27.0),
            v(-42.0, 27.0),
        ]),
        holes: vec![reversed(hole)],
    };
    (
        extrude_planar_regions(&Plane::YZ, &[pipe], -40.0, 40.0).unwrap(),
        extrude_planar_regions(&Plane::XY, &[ring], -h, h).unwrap(),
    )
}

/// A symmetric cut through the half pipe gives the same body for every length past the pipe
/// (tangency-labs/solidcraft#11749). At each corner (±16, ±25, 0) the ring's corner cylinder
/// only touches a cap: along x the cap is straight and the corner cylinder curves away from it.
/// A longer ring once read one of those touches as a 5e-4 crossing curve, which split the
/// pipe's edge there and kept that corner of the pipe whole (+575.19 mm³ and one face less per
/// corner: 9 faces at h = 36.75, 8 at 250).
#[test]
fn a_symmetric_cut_through_a_half_pipe_is_the_same_for_every_length_past_it() {
    all_fixes();
    for h in [30.0, 36.65, 36.75, 50.0, 250.0] {
        let (pipe, ring) = half_pipe_and_ring(h);
        let cut = boolean(pipe, ring, BooleanOp::Subtract).unwrap().unwrap();
        let report = cut.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "h {h}: {}", report.text);
        assert_eq!(cut.solid_count().unwrap(), 1, "h {h}");
        assert_eq!(cut.counts().unwrap(), [10, 28, 20], "h {h}");
        assert!(
            near(volume(&cut), 10_015.883_451_427),
            "h {h}: {}",
            volume(&cut)
        );
    }
}

/// A 6 deep, 11 high slab whose long wall lies in the plane of the wall of #17595's join,
/// moved `shift` into the slab (at 0 it is that wall), from `from` to `to` along the wall's
/// 120 long line, standing on z = `z0`.
fn slab_on_the_join_wall(from: f64, to: f64, shift: f64, z0: f64) -> Body {
    let start = [-77.110_635_008_193_54, 233.891_523_985_808_33];
    let end = [-137.110_634_991_806_45, 129.968_476_014_191_62];
    let inward = [0.866_025_403_784_438_6, -0.5];
    let at = |along: f64, deep: f64| {
        [
            start[0] + along / 120.0 * (end[0] - start[0]) + deep * inward[0],
            start[1] + along / 120.0 * (end[1] - start[1]) + deep * inward[1],
            z0,
        ]
    };
    let corners = [
        at(from, shift),
        at(to, shift),
        at(to, shift + 6.0),
        at(from, shift + 6.0),
    ];
    let profile: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        11.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

/// The union of `corner` and `slab` (and of `slab` and `corner` with `either_order`) is one
/// valid solid of `counts`, on the default kernel and with every fix on, whose volume is the
/// two less what they share (as the default kernel intersects them), within `rel`.
fn joins_as_both_less_the_shared(
    corner: &dyn Fn() -> Body,
    slab: &dyn Fn() -> Body,
    either_order: bool,
    counts: [usize; 3],
    rel: f64,
) {
    let no_fixes: &[&str] = &[];
    session::set_thread_fixes(no_fixes.iter().copied()).unwrap();
    let shared = boolean(corner(), slab(), BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    let both = corner().volume(1e-11).unwrap() + slab().volume(1e-11).unwrap()
        - shared.volume(1e-11).unwrap();
    for fixes in [no_fixes, solidcore::kernel::UpstreamFixes::NAMES] {
        session::set_thread_fixes(fixes.iter().copied()).unwrap();
        let joined = boolean(corner(), slab(), BooleanOp::Union)
            .unwrap()
            .unwrap();
        one_valid_solid_of_volume(&joined, counts, both, rel);
        if either_order {
            let joined = boolean(slab(), corner(), BooleanOp::Union)
                .unwrap()
                .unwrap();
            one_valid_solid_of_volume(&joined, counts, both, rel);
        }
    }
}

/// #17595: a block joined onto an imported part came back as the part alone. In the part two
/// spline faces meet in an edge of tolerance 4.0e-3, which the join's wall crosses at 1.3° to
/// 1.8° (measured there). Each face's curve ends where its own surface meets the wall, so the
/// two ends lie 0.057 apart along the edge; they were two vertices and the intersection loop
/// stayed open between them. The fixture is a 3 mm cube of the part around that crossing
/// (`data/issue_17595_imported_corner.smb`: 9 faces, 13.57 mm³; the edge's tolerance there is
/// 9.1e-3). The wall's face was not cut through, the flood crossed the gap, and every face of
/// the slab was taken for inside the corner: the union was the corner alone (11 faces,
/// 13.57 mm³, the slab's 792 missing). The same with the wall 0.05 out. The two ends are one
/// crossing of the edge with the wall: the union is the corner and the slab less what they
/// share. (The edge's tolerance over the cube's 3 x 3 section lets two results differ by
/// 0.08, 1e-4 of the volume; measured 0.0085 at most.)
///
/// With the slab first the same two unions are refused on this cube as not a solid (the
/// part itself joins in either order): open, and not asserted here.
///
/// A wall that misses the edge is untouched: 1 mm in, the same sum in either order; 1 mm out
/// the slab holds the corner and the union is the slab, 12 x 6 x 11.
#[test]
fn a_join_whose_wall_crosses_a_loose_spline_edge_at_a_shallow_angle_keeps_the_tool() {
    let corner = || {
        Body::from_smb(
            include_bytes!("data/issue_17595_imported_corner.smb").to_vec(),
            false,
        )
        .unwrap()
    };
    assert_eq!(corner().counts().unwrap(), [9, 20, 13]);
    for (shift, either_order, counts) in [
        (0.0, false, [10, 21, 14]),
        (-0.05, false, [10, 21, 14]),
        (1.0, true, [15, 35, 23]),
    ] {
        let slab = || slab_on_the_join_wall(80.35, 92.35, shift, -46.0);
        joins_as_both_less_the_shared(&corner, &slab, either_order, counts, 1e-4);
    }
    let slab = boolean(
        corner(),
        slab_on_the_join_wall(80.35, 92.35, -1.0, -46.0),
        BooleanOp::Union,
    )
    .unwrap()
    .unwrap();
    one_valid_solid_of_volume(&slab, [6, 12, 8], 12.0 * 6.0 * 11.0, 1e-8);
}

/// #17595, the same join farther along its wall, where the wall leaves a spline face through
/// an edge whose curve lies 3.3e-3 outside the face's surface, past the edge's tolerance of
/// 2.8e-3 (as the STEP import left it; the fixture is a 3 mm cube of the part there, 7 faces,
/// with that edge's tolerance:
/// `data/issue_17595_imported_edge_off_its_face.record.json`). The wall's curve on the face
/// ends on the surface's natural boundary, 3.3e-3 from the edge's curve, where the curve/edge
/// intersection finds no crossing: the end was taken for a point inside the face, became a
/// vertex 8.8e-3 short of the one where the next face's curve starts on the edge, and the
/// intersection loop stayed open between the two. The face's trims put that end on the edge.
/// The union, in either order, is the corner and the slab less what they share (within 1e-4
/// of the volume, as above; measured 0.0098 of 799).
#[test]
fn a_join_past_an_imported_edge_that_lies_off_its_face_keeps_the_tool() {
    let record: Record = serde_json::from_str(include_str!(
        "data/issue_17595_imported_edge_off_its_face.record.json"
    ))
    .unwrap();
    let corner = || Body::from_record(&record, true).unwrap().0;
    let slab = || slab_on_the_join_wall(49.8, 61.8, 0.0, -50.0);
    assert_eq!(corner().counts().unwrap(), [7, 15, 10]);
    joins_as_both_less_the_shared(&corner, &slab, true, [12, 27, 18], 1e-4);
}

/// `b` written to a record and read back, as the app hands its operands to the kernel.
fn through_record(b: &Body) -> Body {
    Body::from_record(&b.to_record().unwrap().0, true)
        .unwrap()
        .0
}

/// #17596: a block with a bore of radius 149 up to z = 100 and one of radius 143 on through the
/// top, the bore's ceiling edge rounded 2 mm, less a cylinder of radius 155 up to z = 100. The
/// tool's top merges with the ceiling, an annulus with the 143 bore as its inner loop, and the
/// merge turns the surviving face over: the inner loop must move onto the face use that keeps its
/// side. It landed on the other one, and the result was refused as no solid (in the app: "Face
/// Loop has bad orientation", "Edgeuses share same region"). The round lies inside the tool (r 147
/// to 149, z 98 to 100), so the result is the block less the r 155 cylinder up to z = 100 and the
/// r 143 one above it: 400 · 400 · 108 − π (155² · 100 + 143² · 8).
#[test]
fn a_cylinder_cut_flush_with_a_rounded_bore_ceiling_is_one_valid_solid() {
    all_fixes();
    let block = make_box([-200.0, -200.0, 0.0], [400.0, 400.0, 108.0]).unwrap();
    let bore = solidcore_ops::make_cylinder([0.0, 0.0, -1.0], 149.0, 101.0).unwrap();
    let block = boolean(block, bore, BooleanOp::Subtract).unwrap().unwrap();
    let upper = solidcore_ops::make_cylinder([0.0, 0.0, 99.0], 143.0, 10.0).unwrap();
    let block = boolean(block, upper, BooleanOp::Subtract).unwrap().unwrap();
    let ceiling = common::edge_at(&block, [0.0, 149.0, 100.0]);
    blend(
        &block,
        Blend::Round,
        &[ceiling],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(block.check(CheckLevel::All).unwrap().is_valid());
    let tool = solidcore_ops::make_cylinder([0.0, 0.0, -11.0], 155.0, 111.0).unwrap();
    let r = boolean(
        through_record(&block),
        through_record(&tool),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let want = 400.0 * 400.0 * 108.0 - PI * (155.0 * 155.0 * 100.0 + 143.0 * 143.0 * 8.0);
    one_valid_solid_of_volume(&r, [9, 18, 12], want, 1e-9);
}

/// The closed drum of #16133: radius 105.5, 82 tall, walls 3 thick all round (a void of radius
/// 102.5 from z = 3 to 79), a radius 9 hole through its axis.
fn drum_with_a_hole() -> Body {
    let drum = solidcore_ops::make_cylinder([0.0; 3], 105.5, 82.0).unwrap();
    let void = solidcore_ops::make_cylinder([0.0, 0.0, 3.0], 102.5, 76.0).unwrap();
    let drum = boolean(drum, void, BooleanOp::Subtract).unwrap().unwrap();
    let hole = solidcore_ops::make_cylinder([0.0, 0.0, -1.0], 9.0, 97.0).unwrap();
    boolean(drum, hole, BooleanOp::Subtract).unwrap().unwrap()
}

/// #16133: Split Body keeps what lies on each side of a plane by intersecting with a box whose
/// face lies in it, here the plane of the drum's void ceiling, z = 79. Above it, the box's bottom
/// merges with the ceiling, an annulus whose inner loop is the hole, and the merge turns the
/// surviving face over: the hole's loop must move onto the face use that keeps its side. It
/// landed on the other one and the part was refused as no solid (in the app: "Face Loop has bad
/// orientation", "Edgeuses share same region"). Operands go through the record as the app's do.
/// Below: π (105.5² · 79 − 102.5² · 76 − 9² · 3); above, the 3 mm lid: π (105.5² − 9²) · 3.
#[test]
fn a_drum_cut_on_its_void_ceiling_with_a_hole_through_both_is_two_valid_solids() {
    all_fixes();
    let drum = drum_with_a_hole();
    assert!(drum.check(CheckLevel::All).unwrap().is_valid());
    let below = make_box([-200.0, -200.0, -10.0], [400.0, 400.0, 89.0]).unwrap();
    let above = make_box([-200.0, -200.0, 79.0], [400.0, 400.0, 20.0]).unwrap();
    for (tool, counts, want) in [
        (
            below,
            [6, 9, 6],
            PI * (105.5 * 105.5 * 79.0 - 102.5 * 102.5 * 76.0 - 9.0 * 9.0 * 3.0),
        ),
        (above, [4, 6, 4], PI * (105.5 * 105.5 - 9.0 * 9.0) * 3.0),
    ] {
        let r = boolean(
            through_record(&drum_with_a_hole()),
            through_record(&tool),
            BooleanOp::Intersect,
        )
        .unwrap()
        .unwrap();
        one_valid_solid_of_volume(&r, counts, want, 1e-9);
    }
}

/// #18064's bin: a 168 x 252 x 35 block with a pocket from z = 7 up, its walls x = 2.85 and
/// 165.15, y = 2.85 and 249.15, its corners rounds of radius 1.15 about (4, 4), (4, 248),
/// (164, 4) and (164, 248).
fn pocketed_bin() -> Body {
    let (x0, x1, y0, y1, r, z) = (2.85, 165.15, 2.85, 249.15, 1.15, 7.0);
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], a[1], z],
        end: [b[0], b[1], z],
    };
    let round = |c: [f64; 2], start: f64| Curve::Arc {
        center: [c[0], c[1], z],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start,
        end: start + PI / 2.0,
    };
    let outline = [
        line([x0 + r, y0], [x1 - r, y0]),
        round([x1 - r, y0 + r], -PI / 2.0),
        line([x1, y0 + r], [x1, y1 - r]),
        round([x1 - r, y1 - r], 0.0),
        line([x1 - r, y1], [x0 + r, y1]),
        round([x0 + r, y1 - r], PI / 2.0),
        line([x0, y1 - r], [x0, y0 + r]),
        round([x0 + r, y0 + r], PI),
    ];
    let pocket = extrude(
        &outline,
        [0.0, 0.0, 1.0],
        35.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let block = make_box([0.0, 0.0, 0.0], [168.0, 252.0, 35.0]).unwrap();
    boolean(block, pocket, BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// The top of #18064's insert: the quadratic spline from (y, z) = (2.85, 28) through the
/// control point (`ym`, 7) to (249.15, 7), tangent there to the pocket's floor z = 7, as
/// (y, height above the floor, dy/dt) at `t`. The issue's insert has `ym` = 200.
fn insert_top(ym: f64, t: f64) -> [f64; 3] {
    let (y0, y1) = (2.85, 249.15);
    let u = 1.0 - t;
    let y = u * u * y0 + 2.0 * t * u * ym + t * t * y1;
    let dy = 2.0 * (1.0 - t) * (ym - y0) + 2.0 * t * (y1 - ym);
    [y, 21.0 * u * u, dy]
}

/// The parameter of `insert_top` at `y`.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing it
fn insert_top_at(ym: f64, y: f64) -> f64 {
    let (y0, y1) = (2.85_f64, 249.15_f64);
    let (a, b, c) = (y0 - 2.0 * ym + y1, 2.0 * (ym - y0), y0 - y);
    (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)
}

/// #18064's insert against the pocket's wall x = 2.85: a prism along y from 2.85 to 249.15
/// whose section is the quadrilateral of the wall from z = 7 to 28, the floor out to
/// x = 26.868841 and a side up to x = 9.247749 at z = 28, cut through along x by the region
/// above `insert_top` with the control point at `ym`. Its top meets the floor tangentially
/// along the line y = 249.15, which touches the corner round there at its end.
fn insert_flush_with_a_wall(ym: f64) -> Body {
    let (f, top) = (7.0, 28.0);
    let q = [[2.85, f], [26.868841, f], [9.247749, top], [2.85, top]];
    let section: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            Curve::Line {
                start: [a[0], 2.85, a[1]],
                end: [b[0], 2.85, b[1]],
            }
        })
        .collect();
    let prism = extrude(
        &section,
        [0.0, 1.0, 0.0],
        246.3,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let x = -5.0;
    let above = [
        Curve::Nurbs {
            degree: 2,
            points: vec![[x, 2.85, top, 1.0], [x, ym, f, 1.0], [x, 249.15, f, 1.0]],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        Curve::Line {
            start: [x, 249.15, f],
            end: [x, 249.15, top],
        },
        Curve::Line {
            start: [x, 249.15, top],
            end: [x, 2.85, top],
        },
    ];
    let cut = extrude(
        &above,
        [1.0, 0.0, 0.0],
        200.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    boolean(prism, cut, BooleanOp::Subtract).unwrap().unwrap()
}

/// `f` integrated over [a, b] by Simpson's rule on `n` (even) intervals.
fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: u32) -> f64 {
    let h = (b - a) / f64::from(n);
    let inner: f64 = (1..n)
        .map(|i| f(a + f64::from(i) * h) * if i % 2 == 1 { 4.0 } else { 2.0 })
        .sum();
    (f(a) + f(b) + inner) * h / 3.0
}

/// #18064: a Gridfinity bin's insert joined into it, flush with the pocket's wall x = 2.85,
/// its floor and its end walls, its spline top meeting the floor tangentially along the far
/// wall y = 249.15. That line is tangent to the corner round about (4, 248) at its end
/// (4, 249.15), and the curve where the top crosses the round runs within 1e-10 of the
/// round's floor edge for 0.027 before it ends there, touching it to fourth order. The
/// curve/edge intersection took the last 0.027 for a coincident stretch (both its ends
/// within the scaled zero) and split the curve there, which the insert's own topology did
/// not follow: the join was refused (`SM_ERR`; on the issue's insert, "not a closed solid"),
/// while the mirror image against x = 165.15, whose subdivision found the touching point,
/// joined. The range is the flank of the tangency at the touching point and is dropped. The
/// join is the bin and the insert less what they share: the insert's corners
/// inside the walls outside the two rounds, its full section there, of height the top's.
#[test]
fn an_insert_whose_top_is_tangent_to_the_floor_at_a_corner_round_joins_its_bin() {
    insert_with_its_top_tangent_to_the_floor_joins_its_bin(200.0, true);
}

/// #18064 without `simple_coincidence_crossing_is_point` (WR-647 part A): the curve where the
/// top crosses the corner round reaches the round's floor edge only at its end vertex
/// (4, 249.15, 7), so the curve/edge range there is that one contact. The top's other
/// boundary, the floor's line y = 249.15, meets the round's floor arc only at that point, as
/// its tangent; the face/face tangency witness proves the range is the point from those two
/// carriers, and the join is the same as with the flag.
#[test]
fn an_insert_whose_top_is_tangent_to_the_floor_at_a_corner_round_joins_its_bin_by_the_witness() {
    insert_with_its_top_tangent_to_the_floor_joins_its_bin(200.0, false);
}

/// #18064's insert with its top's control point at y = 100 instead of 200: the top is ten
/// times flatter where it meets the floor, so the curve where it crosses the corner round
/// stays within the scaled zero of the round's floor edge for longer before the touch.
#[test]
fn an_insert_whose_flatter_top_is_tangent_to_the_floor_at_a_corner_round_joins_its_bin() {
    insert_with_its_top_tangent_to_the_floor_joins_its_bin(100.0, true);
}

/// The flatter insert of #18064 without `simple_coincidence_crossing_is_point`: the witness
/// makes the longer range at the round's end vertex the one contact there (WR-647 part A).
#[test]
fn an_insert_whose_flatter_top_is_tangent_to_the_floor_at_a_corner_round_joins_its_bin_by_the_witness(
) {
    insert_with_its_top_tangent_to_the_floor_joins_its_bin(100.0, false);
}

/// The join of `insert_flush_with_a_wall(ym)` into `pocketed_bin`: the bin and the insert
/// less what they share, the insert's corners inside the walls outside the two rounds, its
/// full section there, of height the top's. `crossing_is_point` keeps
/// `simple_coincidence_crossing_is_point` on; without it every other fix is on.
fn insert_with_its_top_tangent_to_the_floor_joins_its_bin(ym: f64, crossing_is_point: bool) {
    if crossing_is_point {
        all_fixes();
    } else {
        session::set_thread_fixes(
            solidcore::kernel::UpstreamFixes::NAMES
                .iter()
                .copied()
                .filter(|n| *n != "simple_coincidence_crossing_is_point"),
        )
        .unwrap();
    }
    let r = 1.15_f64;
    // what the insert shares with the bin: over each corner square outside its round, the
    // insert's height above the floor
    #[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing it
    let width = |dy: f64| 4.0 - (r * r - dy * dy).max(0.0).sqrt() - 2.85;
    let shared_over = |ya: f64, yb: f64, centre: f64| {
        simpson(
            |t| {
                let [y, h, dy] = insert_top(ym, t);
                width(y - centre) * h * dy
            },
            insert_top_at(ym, ya),
            insert_top_at(ym, yb),
            200_000,
        )
    };
    let shared = shared_over(2.85, 4.0, 4.0) + shared_over(248.0, 249.15, 248.0);
    let bin = pocketed_bin();
    let insert = insert_flush_with_a_wall(ym);
    let want = bin.volume(1e-11).unwrap() + insert.volume(1e-11).unwrap() - shared;
    let joined = boolean(bin, insert, BooleanOp::Union).unwrap().unwrap();
    // the bin's 15 faces with the insert's top and slanted side
    one_valid_solid_of_volume(&joined, [17, 41, 27], want, 1e-9);
}

/// The upper half (z >= 0) of the torus of radii 1.8 and 0.6 about z, revolved from its
/// profile circle starting at the circle's outer point (2.4, 0, 0) as SolidCraft revolves it:
/// the seam meridian lies in the xz plane on +x and ends at (2.4, 0, 0) on the flat face's
/// outer edge, which is the profile circle's own seam.
fn half_torus() -> Body {
    let profile = [Curve::Arc {
        center: [1.8, 0.0, 0.0],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: 0.6,
        start: 0.0,
        end: TAU,
    }];
    let torus = revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap();
    let below = make_box([-3.4, -3.4, -1.6], [6.8, 6.8, 1.6]).unwrap();
    boolean(torus, below, BooleanOp::Subtract).unwrap().unwrap()
}

/// A half torus of any radii. Kept for the notch case moved to its own issue (see the
/// finding `heal-face-props-seam-review-18804.md`); no test calls it until that returns.
#[allow(dead_code)]
fn half_torus_of_radii(major: f64, tube: f64) -> Body {
    torus_above(major, tube, 0.0)
}

fn torus_above(major: f64, tube: f64, bottom: f64) -> Body {
    let profile = [Curve::Arc {
        center: [major, 0.0, 0.0],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: tube,
        start: 0.0,
        end: TAU,
    }];
    let torus = revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap();
    let outside = major + tube + 1.0;
    let below = make_box(
        [-outside, -outside, -tube - 1.0],
        [2.0 * outside, 2.0 * outside, tube + 1.0 + bottom],
    )
    .unwrap();
    boolean(torus, below, BooleanOp::Subtract).unwrap().unwrap()
}

/// A cylinder of `radius` about z from `base` up `height`, its seam at the angle SolidCraft
/// gives a cylinder's (0.6131 rad from +x).
#[allow(clippy::disallowed_methods)] // the test's operand, not ops computing geometry
fn rod(base: [f64; 3], radius: f64, height: f64) -> Body {
    let (s, c) = 0.6131_f64.sin_cos();
    let circle = [Curve::Arc {
        center: base,
        axis: [0.0, 0.0, 1.0],
        reference: [c, s, 0.0],
        radius,
        start: 0.0,
        end: TAU,
    }];
    extrude(
        &circle,
        [0.0, 0.0, 1.0],
        height,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

/// The record's vertex nearest `corner`, its distance from the seam (`off_seam` of its
/// point) and the record's tolerance.
#[allow(clippy::disallowed_methods)] // the test's own measure, not ops geometry
fn seam_vertex_gap(r: &Body, corner: [f64; 3], off_seam: impl Fn([f64; 3]) -> f64) -> (f64, f64) {
    let (rec, _) = r.to_record().unwrap();
    let dist = |p: [f64; 3]| (0..3).map(|i| (p[i] - corner[i]).powi(2)).sum::<f64>();
    let v = rec
        .vertices
        .iter()
        .min_by(|a, b| dist(a.point).total_cmp(&dist(b.point)))
        .unwrap();
    (off_seam(v.point), rec.tolerance)
}

/// #18274: a boss of radius 0.07 and height 0.14 joined under the flat face of a half
/// torus, centred on the torus's seam meridian, its side `d` beyond the flat face's outer
/// edge, both through their records as SolidCraft hands them on. The boss's top crosses
/// that edge 1.2e-3 either side of the seam at 1 degree, a crossing fixed only to the
/// tolerance over that sine, so the two crossings and the seam's vertex between them are one
/// point. The vertex the merge kept was a crossing's, 1.2e-3 off the seam, so the seam edge
/// ended off the seam and the check refused the join (face test 36) for d 1e-5 and 2e-5. The
/// seam's vertex now stays. Below the contact tolerance (d up to 9e-6) the boss's top circle
/// and the edge touch at one point (docs/booleans.md §6, "Circles a hair less than
/// tangent"), the seam's vertex; the surface intersection of the torus with the boss's wall
/// met the lens at its two crossings as well, and the flat face's plane cut the boss's
/// circle at the side of its domain, 8.4e-4 off the seam for d 5e-6: for d from 3e-6 to
/// 9.9e-6 the check refused the join ("partially coincident edges", face test 34), and below
/// it one of the crossings (1.2e-4 off the seam for d 1e-7) was kept as a vertex of its
/// own, an edge and a vertex more. The join is one
/// valid solid of the two volumes added, π²·1.8·0.6² + π·0.07²·0.14: they share only part of
/// a face.
#[test]
#[allow(clippy::disallowed_methods)] // the test's own measure, not ops geometry
fn a_boss_overhanging_a_half_torus_flat_face_by_a_tolerance_at_its_seam_joins() {
    all_fixes();
    let want = PI * PI * 1.8 * 0.36 + PI * 0.07 * 0.07 * 0.14;
    for d in [1e-7, 1e-6, 3e-6, 5e-6, 7e-6, 9e-6, 1e-5, 1.5e-5, 2e-5] {
        let boss = round_tripped(&rod([2.33 + d, 0.0, -0.14], 0.07, 0.14));
        let r = boolean(round_tripped(&half_torus()), boss, BooleanOp::Union)
            .unwrap_or_else(|e| panic!("d {d}: {e:?}"))
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "d {d}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "d {d}");
        // the torus, its flat face, the boss's side and bottom
        assert_eq!(r.counts().unwrap(), [4, 7, 4], "d {d}");
        // the seam meridian: radius 0.6 about (1.8, 0, 0) in the xz plane
        let (gap, tol) = seam_vertex_gap(&r, [2.4, 0.0, 0.0], |[x, y, z]| {
            y.hypot((x - 1.8).hypot(z) - 0.6)
        });
        assert!(gap <= tol, "d {d}: the seam's vertex is {gap} off the seam");
        let v = volume(&r);
        assert!((v - want).abs() <= 1e-8 * want, "d {d}: {v} vs {want}");
    }
}

/// Stress corpus S2170: a boss of radius 0.12 and height 0.24 joined on the top face (z = 3,
/// a rim of radius 1) of a frustum of radii 2 and 1, at x = 0.88001, so that it overhangs the
/// rim by exactly the contact tolerance (1e-5); the operands through their records, as
/// SolidCraft hands them on, the boss's seam where SolidCraft puts a cylinder's (`rod`).
/// The boolean met the rim and the boss's bottom circle as the edges' own circles and as
/// the faces' intersection curves, a few roundings apart, and read the overlap below the
/// tolerance at some sites (a touch) and above it at others (two crossings 0.0033 apart),
/// and the join failed (status 1001; relating the operands, in the corpus item). The
/// contact is now read once, at the tolerance within it, and the boss touches the rim at
/// one point. The join is one
/// valid solid (Level 2) of the volumes added: the frustum π·3·(2² + 2·1 + 1²)/3 and the
/// boss π·0.12²·0.24, which stands on the top face, overhanging it only in the air.
#[test]
#[allow(clippy::disallowed_methods)] // the test's operands and closed form, not ops geometry
fn a_boss_overhanging_a_frustums_rim_by_exactly_the_tolerance_joins() {
    all_fixes();
    let frustum = round_tripped(&make_cone([0.0; 3], 2.0, 1.0, 3.0).unwrap());
    let boss = round_tripped(&rod([0.88001, 0.0, 3.0], 0.12, 0.24));
    let joined = boolean(frustum, boss, BooleanOp::Union)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap();
    let report = joined.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(joined.solid_count().unwrap(), 1);
    let want = PI * 3.0 * (4.0 + 2.0 + 1.0) / 3.0 + PI * 0.12 * 0.12 * 0.24;
    assert!(
        near(volume(&joined), want),
        "{}, closed form {want}",
        volume(&joined)
    );
}

/// #18274: the r 2 ball cut flat at z 1.2 (a cap edge of radius 1.6), less a notch of radius
/// 0.19 from z -0.4 up to the cap, centred on the cap's edge just off the ball's seam meridian
/// so that its side passes `d` outside the corner (1.6, 0, 1.2) where the seam meets the cap's
/// edge, the operands through their records. The corner lies on the notch's top circle within
/// tolerance, and the vertex definition there moved it halfway to the circle, 5e-6 off the
/// seam: the seam edge's trim left the seam and the check refused the cut (face test 36). The
/// seam's corner now stays. The cut is one valid solid: the ball less its cap, less the
/// notch's share of it, which is the same wherever the notch's centre lies on the cap's edge:
/// over each radius ρ about z, the notch's arc at ρ times the ball's height there between the
/// bottom and the cap.
#[test]
#[allow(clippy::disallowed_methods)] // the test's operands and closed form, not ops geometry
fn a_notch_passing_within_a_tolerance_outside_a_balls_seam_corner_cuts_it() {
    all_fixes();
    let (big, cap, edge, r, bottom) = (2.0_f64, 1.2_f64, 1.6_f64, 0.19_f64, -0.4_f64);
    let h = big - cap;
    let capped = 4.0 / 3.0 * PI * big.powi(3) - PI * h * h * (3.0 * big - h) / 3.0;
    let height = |rho: f64| (big * big - rho * rho).max(0.0).sqrt().min(cap) - bottom;
    let arc = |rho: f64| {
        2.0 * rho
            * ((rho * rho + edge * edge - r * r) / (2.0 * rho * edge))
                .clamp(-1.0, 1.0)
                .acos()
    };
    // (split at ρ = 1.6, where the height turns from the cap's to the ball's)
    let share = |step: f64| {
        tanh_sinh(|rho| height(rho) * arc(rho), edge - r, edge, step)
            + tanh_sinh(|rho| height(rho) * arc(rho), edge, edge + r, step)
    };
    let (coarse, fine) = (share(1.0 / 32.0), share(1.0 / 64.0));
    assert!((coarse - fine).abs() <= 1e-13 * fine, "{coarse} {fine}");
    let want = capped - fine;
    for d in [5e-6, 1e-5, 2e-5] {
        // the centre on the cap's edge at the angle that puts the corner r + d from it
        let angle = 2.0 * ((r + d) / (2.0 * edge)).asin();
        let notch = rod(
            [edge * angle.cos(), edge * angle.sin(), bottom],
            r,
            cap - bottom,
        );
        let ball = make_sphere([0.0; 3], big).unwrap();
        let top = make_box([-3.0, -3.0, cap], [6.0, 6.0, 2.0]).unwrap();
        let capped_ball = boolean(ball, top, BooleanOp::Subtract).unwrap().unwrap();
        let res = boolean(
            round_tripped(&capped_ball),
            round_tripped(&notch),
            BooleanOp::Subtract,
        )
        .unwrap_or_else(|e| panic!("d {d}: {e:?}"))
        .unwrap_or_else(|| panic!("d {d}: the cut removed everything"));
        let report = res.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "d {d}: {}", report.text);
        assert_eq!(res.solid_count().unwrap(), 1, "d {d}");
        // the ball, the cap, the notch's side and bottom
        assert_eq!(res.counts().unwrap(), [4, 8, 6], "d {d}");
        // the seam meridian: radius 2 about the origin in the xz plane, on +x
        let (gap, tol) = seam_vertex_gap(&res, [edge, 0.0, cap], |[x, y, z]| {
            y.hypot(x.hypot(z) - big)
        });
        assert!(gap <= tol, "d {d}: the seam's corner is {gap} off the seam");
        let v = volume(&res);
        assert!((v - want).abs() <= 1e-8 * want, "d {d}: {v} vs {want}");
    }
}

/// #18689: a ring revolved a whole turn about z from a profile in the yz plane (the r 6
/// ball's own arc from its equator down to z -2, the line out to r 6 and up to the equator),
/// joined to that ball, both through their records as SolidCraft hands them on. The revolve
/// axis is `off` along y from the ball's centre: the arc's centre is that far from the axis,
/// so the revolve turns the arc into a sphere centred on the axis, the ball's own sphere
/// moved by `off`. With the axis 3e-9 to 1e-7 off, within tolerance, the join was refused
/// ("Face Near missing Seam", face test 36); at 0 and 1e-6 it joined. Every offset is one
/// valid solid with the faces, edges and vertices of the join about the centre. Its volume is
/// the ball's plus the ring's outside it: the ring's wall has radius 6 + `off` about the axis
/// and holds every section of the ball between z -2 and 0, so the join adds
/// ∫₋₂⁰ π((6 + off)² - (36 - z²)) dz to 4/3·π·6³: 288π + 8π/3 + 2π·off·(12 + off).
#[test]
#[allow(clippy::disallowed_methods)] // the test's operands and closed form, not ops geometry
fn a_ring_turned_about_an_axis_within_tolerance_of_a_balls_centre_joins_it() {
    all_fixes();
    let end = (1.0_f64 / 3.0).asin();
    let (rim, corner) = ([0.0, -6.0 * end.cos(), -2.0], [0.0, -6.0, -2.0]);
    let profile = [
        // from (0, -6, 0) down the ball to the rim at z -2
        Curve::Arc {
            center: [0.0; 3],
            axis: [1.0, 0.0, 0.0],
            reference: [0.0, -1.0, 0.0],
            radius: 6.0,
            start: 0.0,
            end,
        },
        Curve::Line {
            start: rim,
            end: corner,
        },
        Curve::Line {
            start: corner,
            end: [0.0, -6.0, 0.0],
        },
    ];
    let mut about_the_centre = None;
    for off in [0.0, 3e-9, 1e-8, 1e-7, 1e-6] {
        let ball = make_sphere([0.0; 3], 6.0).unwrap();
        let ring = revolve(&profile, [0.0, off, 0.0], [0.0, 0.0, 1.0], TAU, true)
            .unwrap_or_else(|e| panic!("off {off}: {e:?}"));
        let r = boolean(round_tripped(&ball), round_tripped(&ring), BooleanOp::Union)
            .unwrap_or_else(|e| panic!("off {off}: {e:?}"))
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "off {off}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "off {off}");
        let counts = r.counts().unwrap();
        assert_eq!(counts, *about_the_centre.get_or_insert(counts), "off {off}");
        let want = 288.0 * PI + 8.0 * PI / 3.0 + 2.0 * PI * off * (12.0 + off);
        let v = volume(&r);
        assert!((v - want).abs() <= 1e-9 * want, "off {off}: {v} vs {want}");
    }
}

/// `b` as a record whose tolerances (the body's, every edge's and vertex's) are all
/// `tolerance`, as a file read at that tolerance gives it.
fn read_at(b: &Body, tolerance: f64) -> Body {
    fn set(v: &mut Value, t: f64) {
        match v {
            Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    if k == "tolerance" {
                        *x = serde_json::json!(t);
                    } else {
                        set(x, t);
                    }
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| set(x, t)),
            _ => {}
        }
    }
    let (r, _) = b.to_record().unwrap();
    let mut v = serde_json::to_value(&r).unwrap();
    set(&mut v, tolerance);
    let r: Record = serde_json::from_value(v).unwrap();
    Body::from_record(&r, true).unwrap().0
}

/// solidcore#3292: a body read at a file's round tolerance (0.01 mm) joined by one with an
/// edge of 3 um, as a nameplate's plate and a letter read finer for its own short edges. The
/// result took the plate's tolerance, the short edge's ends merged and its side face's edges
/// ran together (`SmFace` test #34, partially coincident edges). The plate's geometry meets
/// the letter's tolerance, so the boolean reads it at that one: the result is valid and keeps
/// the short edge.
///
/// The plate 20 x 20 x 5; on it, from z = 5 to 10, the prism over (5, 5), (15, 5), (15, 12),
/// (10.0015, 15), (9.9985, 15), (5, 12): a 10 x 7 rectangle and, above it, a trapezoid 3 high
/// between widths 10 and 0.003, 70 + 3 (10 + 0.003) / 2 = 85.0045 in area. Volume
/// 2000 + 5 * 85.0045 = 2425.0225.
#[test]
fn a_short_edge_joined_onto_a_body_read_coarser_is_kept() {
    let v = |x: f64, y: f64| Vec2 { x, y };
    let corners = [
        v(5.0, 5.0),
        v(15.0, 5.0),
        v(15.0, 12.0),
        v(10.0015, 15.0),
        v(9.9985, 15.0),
        v(5.0, 12.0),
    ];
    let region = Region2 {
        outer: Loop2::polygon(&corners),
        holes: vec![],
    };
    let letter = read_at(
        &extrude_planar_regions(&Plane::XY, &[region], 5.0, 10.0).unwrap(),
        0.25 * 0.003,
    );
    for plate_tolerance in [0.01, 0.0001] {
        let plate = read_at(
            &make_box([0.0, 0.0, 0.0], [20.0, 20.0, 5.0]).unwrap(),
            plate_tolerance,
        );
        for (a, b) in [
            (letter.copy().unwrap(), plate.copy().unwrap()),
            (plate.copy().unwrap(), letter.copy().unwrap()),
        ] {
            let r = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
            let report = r.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{plate_tolerance}: {}", report.text);
            // the short edge, at the top and at the foot: from (9.9985, 15) to (10.0015, 15)
            let (rec, _) = r.to_record().unwrap();
            let at = |p: [f64; 3], x: f64| (p[0] - x).abs() < 1e-9 && (p[1] - 15.0).abs() < 1e-9;
            let short = rec
                .edges
                .iter()
                .filter(|e| {
                    let [p, q] = e.vertices;
                    let (p, q) = (
                        rec.vertices[p as usize].point,
                        rec.vertices[q as usize].point,
                    );
                    (at(p, 9.9985) && at(q, 10.0015)) || (at(p, 10.0015) && at(q, 9.9985))
                })
                .count();
            assert_eq!(short, 2, "{plate_tolerance}");
            let want = 2000.0 + 5.0 * 85.0045;
            let got = volume(&r);
            assert!(near(got, want), "{plate_tolerance}: {got} vs {want}");
        }
    }
}

/// The area two circles of radii `a` and `b` whose centres are `d` apart have in common.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn circles_common_area(a: f64, b: f64, d: f64) -> f64 {
    if d >= a + b {
        return 0.0;
    }
    if d <= (a - b).abs() {
        return PI * a.min(b).powi(2);
    }
    let ta = ((d * d + a * a - b * b) / (2.0 * d * a)).acos();
    let tb = ((d * d + b * b - a * a) / (2.0 * d * b)).acos();
    a * a * ta + b * b * tb - 0.5 * ((-d + a + b) * (d + a - b) * (d - a + b) * (d + a + b)).sqrt()
}

/// A ball of radius 2 cut flat at z = 1.2, then drilled by a cylinder of radius 0.19 from
/// z = -0.4 up to the flat, whose wall passes `d` outside the flat rim's vertex (1.6, 0, 1.2)
/// on the sphere's seam: the vertex is `d` inside the tool (tangency-labs/solidcore#3339, the
/// Dutchman stress item S0314). For d within the tolerance the wall/sphere curve runs from the
/// rim, within d of the seam, down to where it crosses the seam (2.8e-4 below the vertex at
/// d = 1e-5): that run was taken for a curve mislabelled as on the seam and removed, and
/// nothing else meets the sphere there, since the seam has the sphere on both sides. The
/// wall was left open there and the cut came back empty or not closed for d = 5e-6 to
/// 1.5e-5. Each cut is one valid solid of the volume of the ball, less its cap of height 0.8
/// (π h² (3R − h) / 3), less the part of the cylinder inside the ball, the integral over z of
/// the area the drill's circle has in common with the ball's section of radius √(4 − z²): the
/// cylinder's whole area below z0, where √(4 − z0²) = ρ + 0.19 for its axis ρ from the ball's,
/// and a lens above, integrated by Simpson's rule on each side of z0 (20000 steps each: its
/// error is far inside the check's 1e-7). d = 1e-6 and 2e-5 are not covered: the run is then
/// within a tolerance of the vertex, or starts just past it, and those still fail `SmFace`
/// tests 29 and 31.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_drill_grazing_the_seam_vertex_of_a_cut_balls_rim_cuts_it() {
    all_fixes();
    let (big, r, flat, bottom): (f64, f64, f64, f64) = (2.0, 0.19, 1.2, -0.4);
    let vertex = [(big * big - flat * flat).sqrt(), 0.0];
    // the direction from the vertex to the drill's axis (the issue's placement)
    let (ux, uy): (f64, f64) = (-0.011_280_0, 0.189_655_0);
    let n = (ux * ux + uy * uy).sqrt();
    let (ux, uy) = (ux / n, uy / n);
    let simpson = |f: &dyn Fn(f64) -> f64, a: f64, b: f64| {
        let n = 20_000;
        let h = (b - a) / f64::from(n);
        let mut s = f(a) + f(b);
        for i in 1..n {
            s += f(a + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        s * h / 3.0
    };
    let cap = PI * (big - flat).powi(2) * (3.0 * big - (big - flat)) / 3.0;
    for d in [-1e-5, 0.0, 5e-6, 1e-5, 1.5e-5, 4e-5, 1e-4] {
        let axis = [vertex[0] + (r - d) * ux, vertex[1] + (r - d) * uy];
        let rho = axis[0].hypot(axis[1]);
        let section = |z: f64| circles_common_area(r, (big * big - z * z).sqrt(), rho);
        let z0 = (big * big - (rho + r).powi(2)).sqrt();
        let drilled = PI * r * r * (z0 - bottom) + simpson(&section, z0, flat);
        let want = 4.0 / 3.0 * PI * big.powi(3) - cap - drilled;
        let ball = make_sphere([0.0; 3], big).unwrap();
        let block = make_box([-3.0, -3.0, flat], [6.0, 6.0, 2.0]).unwrap();
        let cut = boolean(ball, block, BooleanOp::Subtract).unwrap().unwrap();
        let drill =
            solidcore_ops::make_cylinder([axis[0], axis[1], bottom], r, flat - bottom).unwrap();
        let got = boolean(cut, drill, BooleanOp::Subtract)
            .unwrap_or_else(|e| panic!("d {d}: {e:?}"))
            .unwrap_or_else(|| panic!("d {d}: empty"));
        let report = got.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "d {d}: {}", report.text);
        assert_eq!(got.solid_count().unwrap(), 1, "d {d}");
        assert!(got.is_manifold_solid().unwrap(), "d {d}");
        let v = volume(&got);
        assert!((v - want).abs() <= 1e-7 * want, "d {d}: {v} vs {want}");
    }
}

/// #17527's cut as SolidCraft passed it: a part with a 2.1 mm hole, less a revolved counterbore
/// (a 4.25 mm bore, a 45° cone down to the hole's radius, a disc closing it). The sketch's revolve
/// axis is 1.19e-5 off the hole's axis, so the tool's rim (radius 2.1000073) lies 4.6e-6 to
/// 1.9e-5 from the hole's wall: in it, within the rim's tolerance, but nowhere within the proto
/// topology's. The cone and the disc each meet the wall along the rim in a curve of their own.
/// Their overlapping proto edges were replaced by the rim classified within half the zone
/// tolerance, which found it nowhere in the wall, so the cut failed in `make_faceuse`; with the
/// rim classified within its own tolerance, a piece of the disc's circle outside the overlap
/// stayed beside its replacement and was related twice ("another entity is already related").
/// The cut is one valid solid, the same as the cut by the tool moved onto the hole's axis.
#[test]
fn a_traced_revolved_counterbore_a_hair_off_the_holes_axis_cuts_the_part() {
    all_fixes();
    let body = traced_operands("issue_17527_revolve_cut_round_edge");
    let cut = |dy: f64| {
        let tool = body("tool");
        tool.transform(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, dy, 0.0, 1.0],
        ])
        .unwrap();
        boolean(body("target"), tool, BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    };
    // the tool moved by the offset of its axis from the hole's: no rim off the wall
    let want = cut(1.1866e-5).volume(1e-9).unwrap();
    let r = cut(0.0);
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    // the part's 41 faces (the hole's wall now ends at the rim), the bore and the cone
    assert_eq!(r.counts().unwrap(), [43, 104, 65]);
    let v = r.volume(1e-9).unwrap();
    assert!(
        (v - want).abs() < 1e-3,
        "volume {v}, on the hole's axis {want}"
    );
}

/// #20422: two curved spline pockets meeting diagonally at the crossing (4, 5) leave a vertical
/// self-touching pinch edge along (4, 5, 3)..(4, 5, 4) whose curve is a degree-1 `Record::Spline`.
/// Through a record round trip, `split_touching` writes two copies of that edge; before the fix,
/// `curve::from_parameters` marked clamped polynomial splines rational (`w = 1.0` instead of
/// `NOW`), so `glue_self_touching_edges` and `edges_coincide` both refused them (`certified_hausdorff`
/// rejects rational curves) and cutting the third adjacent pocket N failed in `relate`.
///
/// Closed-form areas (depth 1 on the [0, 10] × [0, 10] × [0, 4] box of volume 400):
/// - L (x ∈ [3, 4], between y = 2 + 4u - u² and y = 8 - 4u + u²): ∫₀¹ (6 - 8u + 2u²) du = 8/3
/// - M (x ∈ [4, 6], between y = 5 - 4t(1-t) and y = 5 + 4t(1-t), dx/dt = 2): 16/6 = 8/3
/// - N (x ∈ [3, 5], up to y = 8 above L's top and M's left half-top): 5/3 + 7/3 = 4
///
/// Remaining volume: 400 - 28/3 = 1172/3.
#[test]
fn curved_pockets_touching_along_a_vertical_spline_edge_cut_after_round_trip() {
    all_fixes();
    let z = 3.0;
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], a[1], z],
        end: [b[0], b[1], z],
    };
    let quad = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| Curve::Nurbs {
        degree: 2,
        points: vec![
            [a[0], a[1], z, 1.0],
            [b[0], b[1], z, 1.0],
            [c[0], c[1], z, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    };
    let tool = |curves: &[Curve]| {
        extrude(
            curves,
            [0.0, 0.0, 1.0],
            1.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    };
    let region_l = tool(&[
        line([3.0, 8.0], [3.0, 2.0]),
        quad([3.0, 2.0], [3.5, 4.0], [4.0, 5.0]),
        quad([4.0, 5.0], [3.5, 6.0], [3.0, 8.0]),
    ]);
    let region_m = tool(&[
        quad([4.0, 5.0], [5.0, 3.0], [6.0, 5.0]),
        quad([6.0, 5.0], [5.0, 7.0], [4.0, 5.0]),
    ]);
    let region_n = tool(&[
        quad([3.0, 8.0], [3.5, 6.0], [4.0, 5.0]),
        quad([4.0, 5.0], [4.5, 6.0], [5.0, 6.0]),
        line([5.0, 6.0], [5.0, 8.0]),
        line([5.0, 8.0], [3.0, 8.0]),
    ]);

    let block = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 4.0]).unwrap();
    let after_l = round_tripped(
        &boolean(block, region_l, BooleanOp::Subtract)
            .unwrap()
            .unwrap(),
    );
    let after_m_raw = boolean(after_l, region_m, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    // Verify the exported record actually has the two duplicate vertical `Spline` edges at (4, 5).
    let (rec, _) = after_m_raw.to_record().unwrap();
    let at_pinch = |p: [f64; 3]| (p[0] - 4.0).abs() < 1e-9 && (p[1] - 5.0).abs() < 1e-9;
    let pinch_splines = rec
        .edges
        .iter()
        .filter(|e| {
            let [a, b] = e.vertices;
            at_pinch(rec.vertices[a as usize].point)
                && at_pinch(rec.vertices[b as usize].point)
                && matches!(e.curve, solidcore_record::brep::Curve::Spline { .. })
        })
        .count();
    assert_eq!(
        pinch_splines, 2,
        "expected 2 duplicate vertical Spline pinch edges at (4, 5)"
    );

    let after_m = Body::from_record(&rec, true).unwrap().0;
    let r = boolean(after_m, region_n, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    assert!(r.is_manifold_solid().unwrap());
    let want = 400.0 - 28.0 / 3.0;
    let got = volume(&r);
    assert!(near(got, want), "volume {got} vs closed form {want}");
}
