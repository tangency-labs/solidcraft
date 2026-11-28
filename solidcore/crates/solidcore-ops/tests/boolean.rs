//! The boolean family on bodies whose answers are known.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

mod common;

use std::f64::consts::{PI, TAU};
use std::io::Read as _;

use serde_json::Value;
use solidcore::topology::vertex::corner;
use solidcore_ops::boolean::{boolean, imprint, union_all, union_all_with, union_copies};
use solidcore_ops::prism::extrude_planar_regions;
use solidcore_ops::sweep::{extrude, join_arcs, revolve, zone_tolerance, Curve, DraftCorner, Join};
use solidcore_ops::{
    blend, make_box, make_cone, make_sphere, session, Blend, Body, BooleanOp, CheckLevel, Contract,
    Corners, Error, PointClass,
};
use solidcore_record::brep::{Brep as Record, Surface};
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

/// Two 2° wedges of a ring (radii 4 to 10, 4 high) grooved by a whole turn of a torus (radii
/// 7 and 1, its centre circle in the top face), the second the first's mirror image in the
/// plane at 2°, both through their records: they meet in that plane only, and their union is
/// 4/360 of the grooved ring, V = (4/360)(π(10² − 4²)·4 − π²·7·1²) (a half torus of the
/// groove: π²Rr²). The groove's face is a sliver of a whole turn, which the face's box from its
/// surface (the whole control net) boxes as the whole ring, and the mirror image's faces come
/// back from its record over their surfaces' whole domains. So the first wedge and its own copy
/// a quarter turn on, apart from it, paired every face of each with the other's groove; boxed
/// by what the faces hold, they pair none.
#[test]
#[allow(clippy::disallowed_methods)] // the test's operands and checks, not ops computing geometry
fn wedges_of_a_grooved_ring_unite_and_wedges_apart_pair_no_faces() {
    all_fixes();
    let step = 2f64.to_radians();
    let p = |x: f64, z: f64| [x, 0.0, z];
    let line = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let section = [
        line(p(4.0, 0.0), p(10.0, 0.0)),
        line(p(10.0, 0.0), p(10.0, 4.0)),
        line(p(10.0, 4.0), p(4.0, 4.0)),
        line(p(4.0, 4.0), p(4.0, 0.0)),
    ];
    let wedge = revolve(&section, [0.0; 3], [0.0, 0.0, 1.0], step, true).unwrap();
    let tube = [Curve::Arc {
        center: [7.0, 0.0, 4.0],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: 1.0,
        start: 0.0,
        end: TAU,
    }];
    let torus = revolve(&tube, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap();
    let a = boolean(wedge, torus, BooleanOp::Subtract).unwrap().unwrap();
    let b = a.copy().unwrap();
    // the reflection in the plane through z at 2°: I - 2nn', n = (-sin, cos, 0)
    let (s, c) = step.sin_cos();
    let (nx, ny) = (-s, c);
    b.transform(&[
        [1.0 - 2.0 * nx * nx, -2.0 * nx * ny, 0.0, 0.0],
        [-2.0 * nx * ny, 1.0 - 2.0 * ny * ny, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    // the first wedge a quarter turn on, apart from the first: no face of one meets the other
    let far = a.copy().unwrap();
    far.transform(&[
        [0.0, 1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    let (ra, rb) = (a.to_record().unwrap().0, b.to_record().unwrap().0);

    let mut k = solidcore::Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..solidcore::Kernel::default()
    };
    let mut pairs_of = |x: &Record, y: &Record| {
        let (kx, _) = solidcore_convert::brep::to_kernel(&mut k, x, true).unwrap();
        let (ky, _) = solidcore_convert::brep::to_kernel(&mut k, y, true).unwrap();
        let before = k.work_counters.face_pairs();
        solidcore::boolean::api_brep::boolean_union(&mut k, kx, ky).unwrap();
        k.work_counters.face_pairs() - before
    };
    let apart = pairs_of(&ra, &far.to_record().unwrap().0);
    assert_eq!(
        apart, 0,
        "the wedges a quarter turn apart paired {apart} faces"
    );

    let (ua, _) = Body::from_record(&ra, true).unwrap();
    let (ub, _) = Body::from_record(&rb, true).unwrap();
    let u = union_all(vec![ua, ub]).unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    let want = 4.0 / 360.0 * (PI * (100.0 - 16.0) * 4.0 - PI * PI * 7.0);
    assert!(near(volume(&u), want), "{}, closed form {want}", volume(&u));
}

/// A plate 42 x 42 x 5 at `x0` along x with a pocket 2 deep cut into its top: a 41 x 41 square
/// with corners rounded to 4, 0.5 in from each side (tangency-labs/solidcraft#22034). Its
/// volume is 42² · 5 − 2 (41² − (4 − π) 4²) = 5586 − 32π.
fn pocketed_plate(x0: f64) -> Body {
    let v = |x: f64, y: f64| Vec2::new(x0 + x, y);
    let (r, a, b) = (4.0, 0.5, 41.5);
    let arc = |c: Vec2, start: f64| Seg2::Arc {
        center: c,
        radius: r,
        start,
        sweep: PI / 2.0,
    };
    let line = |p: Vec2, q: Vec2| Seg2::Line { a: p, b: q };
    let pocket = Loop2 {
        segs: vec![
            line(v(a + r, a), v(b - r, a)),
            arc(v(b - r, a + r), -PI / 2.0),
            line(v(b, a + r), v(b, b - r)),
            arc(v(b - r, b - r), 0.0),
            line(v(b - r, b), v(a + r, b)),
            arc(v(a + r, b - r), PI / 2.0),
            line(v(a, b - r), v(a, a + r)),
            arc(v(a + r, a + r), PI),
        ],
    };
    let region = Region2 {
        outer: pocket,
        holes: vec![],
    };
    let tool = extrude_planar_regions(&Plane::XY, &[region], 3.0, 5.0).unwrap();
    let plate = make_box([x0, 0.0, 0.0], [42.0, 42.0, 5.0]).unwrap();
    boolean(plate, tool, BooleanOp::Subtract).unwrap().unwrap()
}

/// tangency-labs/solidcraft#22034: a join of copies of a pocketed plate side by side. Each union
/// carries the trims of the faces it copies from the other operand, and proves each carried
/// trim within its edge's tolerance at equal parameters first (`copy_uv_trim_curves_of_other_face`).
/// Every such trim is an exact copy with its edge's own structure: an arc of the pocket's
/// floor (a trim on a plane with the arc's knots and weights), an arc of a rounded corner along
/// its cylinder's first or last row, a line of a corner along its first or last column. So
/// each is proved from its control points at once, and none is bisected
/// (`same_parameter_bisections`): bisected, each was cut down to pieces whose bound came just
/// under its tolerance, and a join of 16 copies gated 768 such trims. The join is the four
/// plates' volumes, `4 (5586 − 32π)`, as one solid.
///
/// Its counts. One plate is a box less a pocket open at the top. The box has its 6 faces, 12
/// edges and 8 vertices, its top face holding the pocket's opening as an inner loop. The
/// pocket adds 9 faces (its floor, 4 straight walls, 4 quarter-cylinder corners, none closed
/// round so none with a seam), 24 edges (8 round its opening, 8 round its floor, 8 vertical
/// where a wall meets a corner) and 16 vertices (8 at the opening, 8 at the floor): 15 faces,
/// 36 edges, 24 vertices (`V − E + F − (L − F) = 24 − 36 + 15 − 1 = 2`). Side by side, each pair
/// of neighbours touches over a whole 42 x 5 face: both go, and the boolean merges faces
/// on one surface across an edge both sides of which it keeps, so each plane of the row's
/// outside (top, bottom, front, back) is one face, each of its edges along the row one
/// edge and the vertices where the plates met are gone (as the issue's 16 plates give
/// `6 + 16 · 9 = 150` faces). The row is one box of 6 faces, 12 edges and 8 vertices, its top
/// holding the 4 openings, with 4 pockets: `6 + 4 · 9 = 42` faces, `12 + 4 · 24 = 108` edges,
/// `8 + 4 · 16 = 72` vertices (`72 − 108 + 42 − 4 = 2`).
///
/// Its trims against their surfaces. Every trim the join holds (each face's trim of an edge,
/// where the unions carried or made one) is within its edge's tolerance of the edge curve at
/// equal parameters: `sup |C(t) − S(W(t))| <= tol` over the edge's interval, by the proved
/// bound that gates a carried trim (`trim_same_parameter_gap_bound_within`), here on the join
/// itself. A UV line on a plane under a line edge has no exact data for that bound (a UV
/// line's rounding moves its parameters); there `C(t) − S(W(t))` is affine in `t`, so its
/// length is largest at an end of the interval, and the gap is the larger end gap plus each
/// line's evaluation slack (`line_evaluation_slack`).
#[test]
#[allow(clippy::disallowed_methods)] // the test's check of its trims, not ops computing geometry
fn a_join_of_pocketed_plates_proves_its_carried_trims_without_bisecting_them() {
    use solidcore::curve::curve::CurveKind;
    use solidcore::query::curve_surface_gap::{
        line_evaluation_slack, trim_same_parameter_gap_bound_within,
    };
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::edgeuse;
    use solidcore::Point2;

    all_fixes();
    let plates: Vec<Body> = (0..4)
        .map(|i| pocketed_plate(42.0 * f64::from(i)))
        .collect();
    let one = 5586.0 - 32.0 * PI;
    assert!(near(volume(&plates[0]), one), "{}", volume(&plates[0]));

    let mut k = solidcore::Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..solidcore::Kernel::default()
    };
    let mut held: Vec<_> = plates
        .iter()
        .map(|p| {
            let record = p.to_record().unwrap().0;
            solidcore_convert::brep::to_kernel(&mut k, &record, true)
                .unwrap()
                .0
        })
        .collect();
    k.work_counters.reset();
    // (the union of each pair of neighbours, then of the two pairs, as `union_all` pairs them)
    while held.len() > 1 {
        let mut next = Vec::new();
        let mut it = held.into_iter();
        while let (Some(a), Some(b)) = (it.next(), it.next()) {
            next.push(solidcore::boolean::api_brep::boolean_union(&mut k, a, b).unwrap());
        }
        held = next;
    }
    assert!(
        k.work_counters.trim_deviation_bounds() > 0,
        "no carried trim was gated"
    );
    assert_eq!(
        k.work_counters.same_parameter_bisections(),
        0,
        "of {} carried trims gated",
        k.work_counters.trim_deviation_bounds()
    );

    // the join's counts, as derived above
    let joined = &held[0];
    assert_eq!(
        [
            joined.face_list().len(),
            joined.edge_list().len(),
            joined.vertex_list().len()
        ],
        [42, 108, 72]
    );
    // every trim of the join within its edge's tolerance of the edge curve at equal parameters
    // (each trim once: an edge use without its own reads its mate's)
    let mut held_by = Vec::new();
    let mut ends_only = 0;
    for &e in joined.edge_list() {
        let rec = joined.edge(e).unwrap();
        let (tol, ivl) = (rec.tolerance, rec.interval);
        let range = [ivl.min(), ivl.max()];
        let curve = rec.curve.as_ref().unwrap();
        for &eu in &rec.edgeuses {
            let (Some(trim), Some(holder)) = edgeuse::p1::uv_trim_curve(joined, eu).unwrap() else {
                continue;
            };
            if held_by.contains(&holder) {
                continue;
            }
            held_by.push(holder);
            let f = edgeuse::p1::face(joined, holder).unwrap().unwrap();
            let surface = joined.face(f).unwrap().surface.as_ref().unwrap();
            let gap = trim_same_parameter_gap_bound_within(
                &k,
                surface,
                curve,
                trim,
                range,
                1e-3 * tol,
                [tol, tol],
                None,
            )
            .unwrap();
            // A UV line on a plane, under a line edge: the bound has no exact data for a UV
            // line at equal parameters (its rounding moves its parameters). The plane is
            // affine in (u, v) and both lines are affine in t, so C(t) − S(W(t)) is affine in
            // t and its length is largest at an end of the interval: the gap is the larger
            // end gap, plus each line's evaluation slack.
            let gap = if let Some(g) = gap {
                g
            } else {
                assert!(
                    matches!(surface.kind(), SurfaceKind::Plane(_)),
                    "edge {e:?}: an unbounded trim off a plane"
                );
                let (CurveKind::Line(c), CurveKind::Line(w)) = (curve.kind(), trim.kind()) else {
                    panic!("edge {e:?}: an unbounded trim that is not a line under a line");
                };
                ends_only += 1;
                let end_gap = |t: f64| {
                    let uv = trim.evaluate_point(&k, t).unwrap();
                    let on = surface.evaluate_point(&k, Point2::new(uv.x, uv.y)).unwrap();
                    (curve.evaluate_point(&k, t).unwrap() - on).length()
                };
                end_gap(range[0]).max(end_gap(range[1]))
                    + line_evaluation_slack(c, false).unwrap()
                    + line_evaluation_slack(w, true).unwrap()
            };
            assert!(
                gap <= tol,
                "edge {e:?}: a trim {gap} from its curve at tolerance {tol}"
            );
        }
    }
    // (the join holds a trim where an operation made or carried one; the others are built
    // when read)
    assert!(
        ends_only < held_by.len(),
        "{} trims, {ends_only} lines bounded by their ends",
        held_by.len()
    );

    let u = union_all(plates).unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_eq!(u.counts().unwrap(), [42, 108, 72]);
    assert!(near(volume(&u), 4.0 * one), "{}", volume(&u));
}

/// tangency-labs/solidcraft#22273: a slat, a box 9 x 200 x 3 joined with a cylinder of radius 2
/// and length 200 lying along the middle of its top face, its axis in that face, as the issue's
/// `slat.json` makes it (the cylinder an extruded circle whose seam is 0.6131 rad round from
/// `-x`, above the top). The union carries the cylinder's trims of its end circles, which it
/// split where they cross the top: on the cylinder's side each piece's trim runs along a piece
/// of its net's end row, with the piece's structure, not the whole row's. Each is now proved
/// from the control points of the row's restriction to the piece at once; before, each of the
/// four was bisected down to just under its tolerance, and the four took about 80% of the join
/// (`same_parameter_bisections`).
///
/// Its counts and volume. The cylinder's lower half lies inside the box (`|x| <= 2 < 4.5`,
/// `1 <= z <= 3`), so the union is the box and the upper half cylinder:
/// `V = 9 · 200 · 3 + π 2² · 200 / 2 = 5400 + 400π`. The cylinder's seam line (at 0.6131 rad
/// from `-x`, `z = 3 + 2 sin 0.6131 > 3`) is on the upper half, and the boolean keeps it as an
/// edge, so the half cylinder is two faces either side of it. Faces: the box's bottom and two
/// long sides, its top in two strips either side of the cylinder, its two ends each one face
/// with the half disc of the cylinder's cap on it (one plane), and the two faces of the half
/// cylinder: 9. Vertices: the box's 8 corners, the 4 where the cylinder meets the top at its
/// ends and the seam's 2 ends: 14. Edges: the box's 4 bottom and 4 vertical edges, its 2 long
/// top edges, 4 short top edges (from a corner to the cylinder at each end), the 2 lines where
/// the cylinder meets the top, the 2 half circles each in 2 arcs either side of the seam, and
/// the seam: 21 (`14 − 21 + 9 = 2`).
///
/// The four split arcs' trims. After the join, each of the four arcs (in the end planes
/// `y = ±100`, from where the circle meets the top, `z = 3`, to the seam's end above it) has
/// a trim on the cylinder's side, and each is asked the gate's question again on its own
/// (`trim_same_parameter_gap_bound_within` over the edge's interval, at the edge's tolerance,
/// the floor a thousandth of it): each is proved within the tolerance with no bisection, so
/// the union's zero bisections are not those trims going ungated.
#[test]
#[allow(clippy::disallowed_methods)] // the test's check of its trims, not ops computing geometry
fn a_slat_joined_with_a_cylinder_along_its_top_proves_the_split_circles_trims_without_bisecting() {
    use solidcore::query::curve_surface_gap::trim_same_parameter_gap_bound_within;
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::edgeuse;

    all_fixes();
    let slat = || make_box([-4.5, -100.0, 0.0], [9.0, 200.0, 3.0]).unwrap();
    let knuckle = || {
        let circle = [Curve::Arc {
            center: [0.0, -100.0, 3.0],
            axis: [0.0, 1.0, 0.0],
            reference: [-1.0, 0.0, 0.0],
            radius: 2.0,
            start: 0.6131,
            end: 0.6131 + TAU,
        }];
        extrude(
            &circle,
            [0.0, 1.0, 0.0],
            200.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    };

    let mut k = solidcore::Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..solidcore::Kernel::default()
    };
    let mut held = |b: Body| {
        let record = b.to_record().unwrap().0;
        solidcore_convert::brep::to_kernel(&mut k, &record, true)
            .unwrap()
            .0
    };
    let (a, b) = (held(slat()), held(knuckle()));
    k.work_counters.reset();
    let joined = solidcore::boolean::api_brep::boolean_union(&mut k, a, b).unwrap();
    assert!(
        k.work_counters.trim_deviation_bounds() > 0,
        "no carried trim was gated"
    );
    assert_eq!(
        k.work_counters.same_parameter_bisections(),
        0,
        "of {} carried trims gated",
        k.work_counters.trim_deviation_bounds()
    );

    // (the four split arcs' trims on the cylinder's side, each bounded on its own)
    let (mut arcs, mut held_by) = (0, Vec::new());
    for &e in joined.edge_list() {
        let rec = joined.edge(e).unwrap();
        let Some(curve) = rec.curve.as_ref() else {
            continue;
        };
        let (tol, ivl) = (rec.tolerance, rec.interval);
        let range = [ivl.min(), ivl.max()];
        let ends = range.map(|t| curve.evaluate_point(&k, t).unwrap());
        let in_an_end_plane = ends.iter().all(|p| (p.y.abs() - 100.0).abs() <= 1e-9);
        let (low, high) = (ends[0].z.min(ends[1].z), ends[0].z.max(ends[1].z));
        if !(in_an_end_plane && (low - 3.0).abs() <= 1e-9 && high > 3.1) {
            continue;
        }
        for &eu in &rec.edgeuses {
            let Some(f) = edgeuse::p1::face(&joined, eu).unwrap() else {
                continue;
            };
            let surface = joined.face(f).unwrap().surface.as_ref().unwrap();
            if matches!(surface.kind(), SurfaceKind::Plane(_)) {
                continue;
            }
            // (each trim once: an edge use without its own reads its mate's)
            let (Some(trim), Some(holder)) = edgeuse::p1::uv_trim_curve(&joined, eu).unwrap()
            else {
                continue;
            };
            if held_by.contains(&holder) {
                continue;
            }
            held_by.push(holder);
            let before = k.work_counters.same_parameter_bisections();
            let gap = trim_same_parameter_gap_bound_within(
                &k,
                surface,
                curve,
                trim,
                range,
                1e-3 * tol,
                [tol, tol],
                None,
            )
            .unwrap();
            assert_eq!(
                k.work_counters.same_parameter_bisections(),
                before,
                "edge {e:?}: the split arc's trim was bisected"
            );
            assert!(
                gap.is_some_and(|g| g <= tol),
                "edge {e:?}: {gap:?} at tolerance {tol}"
            );
            arcs += 1;
        }
    }
    assert_eq!(arcs, 4, "the split arcs' trims on the cylinder's side");

    let u = boolean(slat(), knuckle(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_eq!(u.counts().unwrap(), [9, 21, 14]);
    let want = 5400.0 + 400.0 * PI;
    assert!(near(volume(&u), want), "{}, closed form {want}", volume(&u));
}

/// The box the boolean pairs faces by holds the face (`proved_bounding_box`): it holds every
/// vertex of the face and the face's tight box (`SmFace::CalculateTightBoundingBox`, the face's
/// extreme points inside its trim), on faces whose trims do not bound them alone: a ball's
/// faces over its poles, a whole turn of a torus, a cylinder's band round its seam, a ball
/// with a hole through it (a face of inner loops only), a ball capped across its pole and the
/// grooved ring of the wedges above.
#[test]
#[allow(clippy::disallowed_methods)] // the test's operands and checks, not ops computing geometry
fn the_box_a_face_is_paired_by_holds_the_face() {
    all_fixes();
    let ring = || {
        let tube = [Curve::Arc {
            center: [7.0, 0.0, 0.0],
            axis: [0.0, -1.0, 0.0],
            reference: [1.0, 0.0, 0.0],
            radius: 1.0,
            start: 0.0,
            end: TAU,
        }];
        revolve(&tube, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap()
    };
    let ball = || make_sphere([0.0; 3], 5.0).unwrap();
    let rod = || solidcore_ops::make_cylinder([0.0, 0.0, -8.0], 1.5, 16.0).unwrap();
    let pierced = boolean(ball(), rod(), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let capped = boolean(
        ball(),
        make_box([-6.0, -6.0, 3.0], [12.0, 12.0, 4.0]).unwrap(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let grooved = boolean(
        solidcore_ops::make_cylinder([0.0, 0.0, -4.0], 10.0, 4.0).unwrap(),
        ring(),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    let mut k = solidcore::Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..solidcore::Kernel::default()
    };
    let mut proved = 0;
    for (name, body) in [
        ("ball", ball()),
        ("ring", ring()),
        ("rod", rod()),
        ("pierced", pierced),
        ("capped", capped),
        ("grooved", grooved),
    ] {
        let record = body.to_record().unwrap().0;
        let (mut b, _) = solidcore_convert::brep::to_kernel(&mut k, &record, true).unwrap();
        let faces: Vec<_> = solidcore::topology::brep::p1::faces(&b, None).unwrap();
        for f in faces {
            // every trim made, so that the narrowing to the trims is what is checked
            for lu in solidcore::topology::face::p4::upward_loopuses(&b, f, false, None).unwrap() {
                for eu in solidcore::topology::edgeuse::p1::loopuse_edgeuses(&b, lu).unwrap() {
                    let _ = solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve(
                        &k, &mut b, eu, false, false, true,
                    );
                }
            }
            let Some(bx) =
                solidcore::query::face::p1::proved_bounding_box(&k, &b, f, &mut None).unwrap()
            else {
                continue;
            };
            proved += 1;
            let tight =
                solidcore::query::face::p3::calculate_tight_bounding_box(&k, &b, f).unwrap();
            let slack = 1e-7;
            let inside = |p: solidcore::math::Point3| {
                [p.x, p.y, p.z]
                    .iter()
                    .enumerate()
                    .all(|(i, &x)| x >= bx.min_at(i) - slack && x <= bx.max_at(i) + slack)
            };
            for c in tight.corners() {
                assert!(
                    inside(c),
                    "{name} face {f:?}: tight box {tight:?} leaves {bx:?}"
                );
            }
            for v in solidcore::query::face::p2::face_vertices(&b, f, false, None).unwrap() {
                let p = solidcore::topology::vertex::point(&b, v).unwrap();
                assert!(inside(p), "{name} face {f:?}: vertex {p:?} outside {bx:?}");
            }
        }
    }
    assert!(proved >= 6, "only {proved} faces had a proved box");
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

/// Two equal holes of radius 0.9 crossing at right angles in a 25 x 20.9 x 4.5 block, one
/// along z, the other along y with its axis `d` off the first's along x
/// (tangency-labs/solidcraft#18475). At d = 0 the walls cross in two ellipses; for nonzero d
/// they cross in one loop. For negative `d`, its crossings of the y cylinder's +x seam are
/// 2√(2 · 0.9 |d| − d²) apart, and the seam cuts the loop into two arcs with the same two
/// ends whose midpoints are 2√(2 · 0.9 |d| − d²) apart on the first hole's far side. The second
/// cut was refused for d = −1e-5 and −2e-5, where those two arcs were taken for one edge. Each
/// cut is one valid solid of the block less both holes plus what they share, 16/3 · 0.9³ (the
/// miss changes that by under 3e-5).
#[test]
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn a_hole_whose_axis_misses_an_equal_crossing_holes_by_a_hair_cuts() {
    all_fixes();
    let r = 0.9;
    let want = 25.0 * 20.9 * 4.5 - PI * r * r * (4.5 + 20.9) + 16.0 / 3.0 * r * r * r;
    for d in [0.0, -1e-7, -1e-6, -5e-6, -1e-5, -2e-5, 1e-5, -1e-4, 1e-3] {
        let block = make_box([0.0, -10.45, 0.0], [25.0, 20.9, 4.5]).unwrap();
        let along_z = solidcore_ops::make_cylinder([12.5, 0.0, -1.0], r, 6.5).unwrap();
        let first = boolean(block, along_z, BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let rim = [Curve::Arc {
            center: [12.5 + d, -11.45, 2.25],
            axis: [0.0, 1.0, 0.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start: 0.0,
            end: TAU,
        }];
        let along_y = extrude(
            &rim,
            [0.0, 1.0, 0.0],
            22.9,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        let cut = boolean(first, along_y, BooleanOp::Subtract);
        let cut = cut.unwrap_or_else(|e| panic!("{d}: {e:?}")).unwrap();
        let report = cut.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{d}: {}", report.text);
        assert_eq!(cut.solid_count().unwrap(), 1, "{d}");
        assert!(
            near(volume(&cut), want),
            "{d}: {}, closed form {want}",
            volume(&cut)
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

// ---- #15049: a cut whose profile has a cusp, swept onto a face its line sweeps onto -------

/// The arc of radius `radius` about the x-parallel axis through `center`, from angle `start`
/// to `end` measured from +y towards +z.
fn cusp_arc_x(center: [f64; 3], radius: f64, start: f64, end: f64) -> Curve {
    Curve::Arc {
        center,
        axis: [1.0, 0.0, 0.0],
        reference: [0.0, 1.0, 0.0],
        radius,
        start,
        end,
    }
}

/// The block of #15049's shape, cut twice, and the frame of its third cut's sketch.
///
/// A block 30 wide along x whose section is a front quarter circle (radius 30), a long top
/// arc (radius 86 about a centre 56 below the floor) and a small back arc (radius 18),
/// standing on the floor z = 0. Cut 1 rounds its front to a ball. Cut 2 sweeps two curved
/// triangles along the top arc; each has an arc of the front ball's radius running
/// tangentially into its top line at x = 0, where the two triangles touch: a cusp. Cut 2
/// ends at the top of the back arc in a plane face holding the two triangles, and cut 3
/// starts there (`sketch_point` maps the sketch's (u, v) onto that plane).
struct CuspBlock {
    body: Body,
    /// The sketch plane's origin, its u and v directions and its normal.
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    normal: [f64; 3],
    /// The back arc's centre (y), radius and start angle: cut 3's path.
    back: (f64, f64, f64),
}

impl CuspBlock {
    fn sketch_point(&self, (a, b): (f64, f64)) -> [f64; 3] {
        let (o, u, v) = (self.origin, self.u, self.v);
        [
            o[0] + u[0] * a + v[0] * b,
            o[1] + u[1] * a + v[1] * b,
            o[2] + u[2] * a + v[2] * b,
        ]
    }
}

const CUSP_W: f64 = 15.0;
const CUSP_R0: f64 = 30.0;
const CUSP_C: f64 = 56.0;
const CUSP_R2: f64 = 18.0;

#[allow(clippy::disallowed_methods)] // the test's operands, not ops computing geometry
fn cusp_block() -> CuspBlock {
    use solidcore_ops::sweep::{sweep_placed, SweepFrame};
    let (w, r0, c, r2) = (CUSP_W, CUSP_R0, CUSP_C, CUSP_R2);
    let rb = r0 + c;
    let q = ((rb - r2).powi(2) - c * c).sqrt();
    let phi_t = c.atan2(-q);
    let x = [1.0, 0.0, 0.0];
    let profile = [
        cusp_arc_x([-w, 0.0, 0.0], r0, 0.0, PI / 2.0),
        cusp_arc_x([-w, 0.0, -c], rb, PI / 2.0, phi_t),
        cusp_arc_x([-w, -q, 0.0], r2, phi_t, PI),
        Curve::Line {
            start: [-w, -q - r2, 0.0],
            end: [-w, r0, 0.0],
        },
    ];
    let mut body = extrude(&profile, x, 2.0 * w, 0.0, DraftCorner::Curvature, true).unwrap();
    // cut 1: the front rounded to a ball
    let h = r0 + 15.0;
    let ya = |x: f64| (r0 * r0 - x * x).sqrt();
    let tool1 = [
        Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: x,
            radius: r0,
            start: ya(w).atan2(w),
            end: ya(w).atan2(-w),
        },
        Curve::Line {
            start: [-w, ya(w), 0.0],
            end: [-w, h, 0.0],
        },
        Curve::Line {
            start: [-w, h, 0.0],
            end: [w + 0.2, h, 0.0],
        },
        Curve::Line {
            start: [w + 0.2, h, 0.0],
            end: [w, ya(w), 0.0],
        },
    ];
    let (s1, _, _) = sweep_placed(
        &tool1,
        &[cusp_arc_x([0.0; 3], r0, 0.0, PI / 2.0)],
        SweepFrame::Follow,
        true,
    )
    .unwrap();
    body = boolean(body, s1, BooleanOp::Subtract).unwrap().unwrap();
    // cut 2: two curved triangles touching at their cusps, swept along the top arc
    let path2 = [cusp_arc_x([0.0, 0.0, -c], rb, PI / 2.0, phi_t)];
    for s in [1.0_f64, -1.0] {
        let za = ya(s * w);
        let (a, b, cc) = ([s * w, 0.0, za], [0.0, 0.0, r0], [s * w, 0.0, r0]);
        let ta = za.atan2(s * w);
        let arc = |start: f64, end: f64| Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, -1.0, 0.0],
            reference: x,
            radius: r0,
            start,
            end,
        };
        let tri = if s > 0.0 {
            [
                arc(ta, PI / 2.0),
                Curve::Line { start: b, end: cc },
                Curve::Line { start: cc, end: a },
            ]
        } else {
            [
                arc(PI / 2.0, ta),
                Curve::Line { start: a, end: cc },
                Curve::Line { start: cc, end: b },
            ]
        };
        let (s2, _, _) = sweep_placed(&tri, &path2, SweepFrame::Follow, true).unwrap();
        body = boolean(body, s2, BooleanOp::Subtract).unwrap().unwrap();
    }
    assert_eq!(body.counts().unwrap(), [9, 19, 12], "the block after cut 2");
    // cut 3's sketch plane: cut 2's end face
    let alpha = phi_t - PI / 2.0;
    let v = [0.0, -alpha.sin(), alpha.cos()];
    let u = [-1.0, 0.0, 0.0];
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    CuspBlock {
        body,
        origin: [0.0, 0.0, -c],
        u,
        v,
        normal,
        back: (-q, r2, phi_t),
    }
}

/// Cut 3's two tools: the end face's triangles as a sketch on that face reads them back,
/// each point moved by a few billionths of a millimetre (`noise`, seeded by `seed`) and each
/// arc's radius taken from its start point, swept along the back arc to the floor.
#[allow(clippy::disallowed_methods)] // the test's operands, not ops computing geometry
fn cusp_tools(block: &CuspBlock, seed: u32, noise: f64) -> Vec<Body> {
    use solidcore_ops::sweep::{sweep_placed, SweepFrame};
    let (w, r0, c) = (CUSP_W, CUSP_R0, CUSP_C);
    let rnd = |j: u32| {
        let t = (f64::from(seed) * 12.9898 + f64::from(j) * 78.233).sin() * 43758.5453;
        (t - t.floor()) * 2.0 - 1.0
    };
    let pt = |j: u32, a: f64, b: f64| (a + noise * rnd(2 * j), b + noise * rnd(2 * j + 1));
    let (side, top) = (c + (r0 * r0 - w * w).sqrt(), c + r0);
    let p1 = pt(1, w, side);
    let p2 = pt(2, w, top);
    let p3 = pt(3, 0.0, c);
    let p4 = pt(4, 0.0, top);
    let p5 = pt(5, -w, side);
    let p6 = pt(6, -w, top);
    // an arc about `cen` from `a` to `b`, counter-clockwise, its radius to `a`
    let arc = |cen: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        let start = (a.1 - cen.1).atan2(a.0 - cen.0);
        let mut sweep = (b.1 - cen.1).atan2(b.0 - cen.0) - start;
        while sweep <= 1e-12 {
            sweep += 2.0 * PI;
        }
        Curve::Arc {
            center: block.sketch_point(cen),
            axis: block.normal,
            reference: block.u,
            radius: ((a.0 - cen.0).powi(2) + (a.1 - cen.1).powi(2)).sqrt(),
            start,
            end: start + sweep,
        }
    };
    let line = |a, b| Curve::Line {
        start: block.sketch_point(a),
        end: block.sketch_point(b),
    };
    let (yc, r2, phi_t) = block.back;
    let path = [cusp_arc_x([0.0, yc, 0.0], r2, phi_t, PI)];
    [
        vec![line(p1, p2), arc(p3, p1, p4), line(p2, p4)],
        vec![arc(p3, p4, p5), line(p4, p6), line(p5, p6)],
    ]
    .iter()
    .map(|region| {
        sweep_placed(region, &path, SweepFrame::Follow, true)
            .unwrap()
            .0
    })
    .collect()
}

/// #15049: a sweep cut whose profile has a cusp, an arc running tangentially into a line,
/// swept round the axis of a cylinder that line sweeps onto. Within a few hundredths of a
/// millimetre of the cusp both of the tool's edges there, and its faces swept from them,
/// lie within the tolerance of each other and of the block, so where the rounding of the
/// sketch's points puts a crossing there decided the topology: the coaxial surfaces crossed
/// 3e-5 to 2.5e-4 from the cusp, and the touch of the tool's line with the block's arc
/// was a crossing as far off, and the imprint failed (no vertex at an edge definition's end)
/// or the merge did. With each contact read as one touch at the cusp, the cut builds
/// whatever the rounding. Seeds whose points were 5e-10 and 1e-9 off; each failed before.
/// Then seeds that failed at the other two contacts, 5e-10 to 3e-9 off: where the cut ends
/// on the floor, the floor's section of the tool's arc face crossed the floor's side once
/// 2.5e-4 from the cusp and the result was not a solid (seeds 32, 3, 4); and where it
/// starts, the touch of the block's arc face and the tool's line face was found 1.5e-4 from
/// the cusp and split the tool's line edge alone, and the merge failed (seeds 2, 14).
#[test]
#[allow(clippy::disallowed_methods)] // the oracle points' closed form, not ops computing geometry
fn a_cusp_swept_onto_a_coaxial_cylinder_cuts_whatever_the_sketch_rounding() {
    all_fixes();
    let block = cusp_block();
    for (seed, noise) in [
        (0, 0.0),
        (7, 5e-10),
        (12, 5e-10),
        (22, 5e-10),
        (8, 1e-9),
        (28, 1e-9),
        (33, 1e-9),
        (2, 5e-10),
        (32, 5e-10),
        (3, 1e-9),
        (4, 3e-9),
        (14, 3e-9),
    ] {
        let tools = cusp_tools(&block, seed, noise);
        // the tools lie inside the block, touching its back face and each other: the cut
        // removes their volumes, no more and no less
        let removed: f64 = tools.iter().map(volume).sum();
        let expected = volume(&block.body) - removed;
        // points of the tools' faces halfway along the back arc, each turned there from the
        // sketch: the middle of each triangle's arc, which bounds the result (its side line
        // sweeps onto the block's side, which the cut opens); and a point inside each
        // triangle, which the cut removes
        let (yc, _, phi_t) = block.back;
        let beta = (PI - phi_t) / 2.0;
        let swept = |(a, b): (f64, f64)| {
            let p = block.sketch_point((a, b));
            let (y, z) = (p[1] - yc, p[2]);
            [
                p[0],
                yc + y * beta.cos() - z * beta.sin(),
                y * beta.sin() + z * beta.cos(),
            ]
        };
        let (w, r0, c) = (CUSP_W, CUSP_R0, CUSP_C);
        let top = c + r0;
        let mid_arc = (r0 * r0 - w * w).sqrt().atan2(w) / 2.0 + PI / 4.0;
        let on_faces: Vec<[f64; 3]> = [1.0, -1.0]
            .iter()
            .map(|&sgn: &f64| swept((sgn * r0 * mid_arc.cos(), c + r0 * mid_arc.sin())))
            .collect();
        let removed_points: Vec<[f64; 3]> = [1.0, -1.0]
            .iter()
            .map(|&sgn: &f64| swept((sgn * 12.0, top - 0.3)))
            .collect();
        for order in [[1usize, 0], [0, 1]] {
            let mut body = block.body.copy().unwrap();
            for k in order {
                body = boolean(body, tools[k].copy().unwrap(), BooleanOp::Subtract)
                    .unwrap_or_else(|e| panic!("seed {seed}, tools {order:?}: {e:?}"))
                    .unwrap_or_else(|| panic!("seed {seed}, tools {order:?}: empty"));
            }
            assert_eq!(
                body.counts().unwrap(),
                [8, 17, 11],
                "seed {seed}, tools {order:?}"
            );
            let report = body.check(CheckLevel::All).unwrap();
            assert!(
                report.is_valid(),
                "seed {seed}, tools {order:?}: {}",
                report.text
            );
            let (_, faces_order) = body.to_record().unwrap();
            let faces: Vec<usize> = (0..8).collect();
            for &at in &on_faces {
                let closest = body
                    .closest_to_point(&faces_order, &faces, &[], at)
                    .unwrap()
                    .unwrap();
                // written to accept: a NaN distance fails
                assert!(
                    closest.distance <= 1e-6,
                    "seed {seed}, tools {order:?}: {at:?} is {} off the result",
                    closest.distance
                );
            }
            for &at in &removed_points {
                assert_eq!(
                    body.classify_point(at).unwrap(),
                    PointClass::Outside,
                    "seed {seed}, tools {order:?}: {at:?} is not cut away"
                );
            }
            // written to accept: a NaN volume fails
            let v = volume(&body);
            assert!(
                (v - expected).abs() <= 1e-6 * expected,
                "seed {seed}, tools {order:?}: volume {v} against {expected} ({removed} removed)"
            );
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
/// check, `whole`; a token cancelled from a quarter of them then stops the check of the same
/// cut moved up by 1 there. It returns `Cancelled`, and the token is polled exactly once after
/// the poll that cancelled it: by the session as the call returns, asking whether it was
/// cancelled (`run_with`, after the kernel's call). Any further poll would be work done after
/// the cancel. (The second check is of the moved cut, not of the same one: a check skips the
/// geometric tests of neighbourhoods an earlier check on the kernel found clean,
/// `solidcore::geom::check_memo`, so the same cut checked again polls far fewer times. The
/// moved cut has no neighbourhood in common with it, so its check is as whole as the first.)
#[test]
fn the_check_of_a_long_coil_cut_stops_on_cancel() {
    use solidcore_ops::cancel::{within, CancelToken};
    use solidcore_ops::sweep::{screw_sweep, Curve};
    // (the cut, moved up by `dz`)
    let cut_at = |dz: f64| {
        let corners = [
            [19.0, 0.0, 12.0 + dz],
            [32.0, 0.0, 12.0 + dz],
            [32.0, 0.0, 25.0 + dz],
            [19.0, 0.0, 25.0 + dz],
        ];
        let square: Vec<Curve> = (0..4)
            .map(|i| Curve::Line {
                start: corners[i],
                end: corners[(i + 1) % 4],
            })
            .collect();
        let coil = screw_sweep(
            &square,
            [0.0, 0.0, dz],
            [0.0, 0.0, 1.0],
            15.24,
            20.0,
            None,
            15.24e-4,
        )
        .unwrap();
        let stock = solidcore_ops::make_cylinder([0.0, 0.0, dz], 20.0, 304.5).unwrap();
        boolean(stock, coil, BooleanOp::Subtract).unwrap().unwrap()
    };
    let cut = cut_at(0.0);
    let probe = CancelToken::after_polls(u64::MAX);
    within(&probe, || cut.check(CheckLevel::All)).unwrap();
    let whole = probe.polls().unwrap();
    let n = whole / 4;
    // cancelled well inside the check, not at its last poll
    assert!(n >= 1 && whole >= 2 * n, "the check polls {whole} times");
    let moved = cut_at(1.0);
    let token = CancelToken::after_polls(n);
    let r = within(&token, || moved.check(CheckLevel::All));
    eprintln!(
        "the check polls {whole} times; cancelled at {n}, polled {:?}",
        token.polls()
    );
    assert_eq!(r.map(|_| ()), Err(Error::Cancelled));
    assert_eq!(token.polls(), Some(n + 1));
}

/// tangency-labs/solidcraft#22578: a rod along x (0 to 72, radius 4, chamfered ends, relief
/// grooves of radius 3.25 at x 25 to 25.5 and 46.5 to 47) and a thread cut of pitch 1.25 and
/// 21.25 turns from x = -1.25 along +x, a 60 degree V with a rounded root, its top on the rod's
/// radius, so the coil's outer wall is a cylinder on the rod's own. The cut was refused by the
/// check (a loop's edges one whole period apart in u) where its twin, the same cut from
/// x = 73.25 along -x, builds. The twin is this design turned half a turn about the vertical
/// line through x = 36, which maps the rod, its seam half-plane (y 0, z above the axis), the
/// coil's profile and its half turns onto themselves: both cuts are one valid solid with the
/// same face, edge and vertex counts. The coil comes to the cut through its record, as
/// SolidCraft's does: its walls are their certified nets, without the screw definitions the
/// record has no field for (built and cut in memory, with them, it built before too).
///
/// The volume is the rod's (Pappus over its polygon) less the thread's part inside it,
/// integrated over the screw motion (volume element r dr du dphi; the chord of the profile at
/// each radius, an exact integral along x, adaptive in r): 3430.968886633128 mm³, to 1e-12.
/// The coil's walls lie within the sweep's tolerance of their screw surfaces, so the cut's
/// volume is within the coil's area times that tolerance of it. Both end discs of the rod stay
/// whole (the cut lost the one at x = 0).
#[test]
fn a_right_handed_coil_cut_on_a_rod_builds_like_its_twin() {
    use solidcore_ops::sweep::screw_sweep;
    all_fixes();
    let rod = [
        [0.0, 0.0],
        [72.0, 0.0],
        [72.0, 3.375],
        [71.375, 4.0],
        [47.75, 4.0],
        [46.99999999999999, 3.2500000000000027],
        [46.49999999999999, 3.250000000000003],
        [45.750000000000014, 4.0],
        [26.25, 4.0],
        [25.500000000000004, 3.2500000000000013],
        [25.0, 3.2500000000000013],
        [24.250000000000007, 4.0],
        [0.6250000000000021, 4.0],
        [0.0, 3.375],
    ];
    // (x, z) to (x, 0, z); `flip` turns it half a turn about the line x = 36 along z, which
    // leaves the rod as it is (its polygon is its own image, to the last bits as sketched)
    let at = |[x, z]: [f64; 2], flip: bool| {
        if flip {
            [72.0 - x, 0.0, z]
        } else {
            [x, 0.0, z]
        }
    };
    let (pitch, turns, tolerance) = (1.25, 21.25, 1.25e-4);
    let exact = 3_430.968_886_633_128;
    let cut = |flip: bool| {
        let n = rod.len();
        let profile: Vec<Curve> = (0..n)
            .map(|i| Curve::Line {
                start: at(rod[i], false),
                end: at(rod[(i + 1) % n], false),
            })
            .collect();
        let stock = revolve(&profile, [0.0; 3], [1.0, 0.0, 0.0], TAU, true).unwrap();
        // the V: its root an arc of radius 0.1 about (-1.25, 3.475), its flanks up to radius 4
        let root = (-1.250_000_000_000_000_2, 3.474_999_999_999_999_6);
        let (a, b) = (
            [-1.336_602_540_378_443_4, 3.425],
            [-1.163_397_459_621_557, 3.425],
        );
        let (c, d) = (
            [-0.831_421_054_837_520_5, 4.0],
            [-1.668_578_945_162_48, 4.0],
        );
        // the arc counter-clockwise about -y from a to b, angle 0 along +x (both turned too)
        let s = if flip { -1.0 } else { 1.0 };
        let section = vec![
            Curve::Arc {
                center: at([root.0, root.1], flip),
                axis: [0.0, -s, 0.0],
                reference: [s, 0.0, 0.0],
                radius: 0.099_999_999_999_999_31,
                start: -2.617_993_877_991_492_6,
                end: -0.523_598_775_598_300_9,
            },
            Curve::Line {
                start: at(b, flip),
                end: at(c, flip),
            },
            Curve::Line {
                start: at(c, flip),
                end: at(d, flip),
            },
            Curve::Line {
                start: at(d, flip),
                end: at(a, flip),
            },
        ];
        let origin = at([root.0, 0.0], flip);
        let swept = screw_sweep(
            &section,
            origin,
            [s, 0.0, 0.0],
            pitch,
            turns,
            None,
            tolerance,
        );
        // as SolidCraft hands it to the cut: through its record, its walls their nets alone
        let coil = Body::from_record(&swept.unwrap().to_record().unwrap().0, true)
            .unwrap()
            .0;
        // only the coil's walls stray from their surfaces, by the tolerance at most, and the
        // cut keeps no more of them than the coil has
        let bound = coil.area(1e-8).unwrap() * tolerance;
        let r = boolean(stock, coil, BooleanOp::Subtract)
            .unwrap_or_else(|e| panic!("flip {flip}: {e:?}"))
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        let v = volume(&r);
        let counts = r.counts().unwrap();
        eprintln!(
            "flip {flip}: volume {v} bound {bound} counts {counts:?} {}",
            report.text
        );
        assert!(report.is_valid(), "flip {flip}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "flip {flip}");
        assert!((v - exact).abs() <= bound, "flip {flip}: {v} vs {exact}");
        // the rod's end discs, both whole: the coil's caps lie outside the rod (the start
        // cap past its end, the end cap over the groove, whose radius is under the root's)
        let (record, order) = r.to_record().unwrap();
        let discs: Vec<(f64, f64)> = (0..record.faces.len())
            .filter_map(|i| match &record.faces[i].surface {
                Surface::Plane { origin, .. } | Surface::PlaneThrough { origin, .. } => {
                    Some((origin[0], r.face_area(&order, i, 1e-10).unwrap()))
                }
                _ => None,
            })
            .collect();
        let disc = PI * 3.375 * 3.375;
        assert_eq!(discs.len(), 2, "flip {flip}: {discs:?}");
        for (x, a) in &discs {
            assert!(*x == 0.0 || *x == 72.0, "flip {flip}: {discs:?}");
            assert!((a - disc).abs() <= 1e-9 * disc, "flip {flip}: {discs:?}");
        }
        counts
    };
    // the twin first: it built before this change too
    let twin = cut(true);
    assert_eq!(cut(false), twin);
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
/// volume as above (turning the ball about its axis moves no point of it). The coincidence
/// crossing is found as the section curve (WR-647, #21306 parts C and D): the section's
/// meridian on the ball's seam runs into its pole, where the drop's pole adjustment once
/// moved its start across the seam (u 1 to u 1.8e-9), out of the ball face's domain
/// (Test #70, frame 2); it now keeps the seam side the drop point is on.
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
    let removed = notch_in_the_seam_cones_rim(c);
    let frustum = PI * height / 3.0 * (r_base * r_base + r_base * r_top + r_top * r_top);
    let cut = boolean(cone(), notch(c), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    one_valid_solid_of_volume(&cut, [5, 10, 7], frustum - removed, 1e-9);
}

/// What a round notch, r 0.06 about `c` and from z = 999.5 to the top, takes from the frustum
/// of radius 3 at z = 0 to 0.5 at z = 1000 about +z: the integral over z of the area the disc
/// shares with the frustum's circle there (two circles' lens, in closed form), by Simpson's
/// rule.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn notch_in_the_seam_cones_rim(c: [f64; 2]) -> f64 {
    let (r_base, r_top, height, r, depth) = (3.0, 0.5, 1000.0_f64, 0.06, 0.5);
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
    (0..=n)
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
        / 3.0
}

/// SolidCraft's cone and notch of #22813, as its `solid.cone` and `solid.cylinder` build them
/// (the operands its boolean was handed, `data/issue_22813_*.smb`): the frustum of
/// [`a_round_notch_overlapping_a_cones_seam_backwards_is_cut`], revolved so that its side is
/// parametrised from the top rim down and clockwise, and the r 0.06 notch whose circle reaches
/// d = 1e-5 past the seam (centre at the angle (0.06 + d) / 0.5 on the rim), then turned
/// 1e-5 rad about +z (d = 5e-6). The wall's curve crosses the seam at a slant of about 2e-4
/// rad and comes out of the surface intersection in two pieces, one each side of the seam,
/// that overlap along it within tolerance. The top one was cut at its crossing, and its rest
/// ran on along the seam to its end, back along the floor's piece from the proto vertex at the
/// crossing, which the floor's piece ends at once the two pieces' overlap above it is
/// combined. Proto edges that meet at a proto vertex were never looked at for an overlap, so
/// the floor's piece kept its own run beside the seam's, the wall's loop stayed open between
/// the crossing and where the floor's piece meets the seam, the side face and the notch's
/// piece of it were one, and the cut came out empty. Each is one valid solid of the five
/// faces, ten edges and seven vertices of that test, V = the frustum less
/// [`notch_in_the_seam_cones_rim`].
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_round_notch_a_tolerance_over_a_revolved_cones_seam_is_cut() {
    all_fixes();
    let read = |smb: &[u8]| Body::from_smb(smb.to_vec(), false).unwrap();
    let cone = include_bytes!("data/issue_22813_cone.smb");
    let frustum = PI * 1000.0 / 3.0 * (9.0 + 1.5 + 0.25);
    let c = [0.496_405_514_949_725_07, 0.059_846_175_546_130_59];
    for turn in [0.0, 1e-5_f64] {
        let notch = read(include_bytes!("data/issue_22813_notch.smb"));
        let (s, co) = turn.sin_cos();
        notch
            .transform(&[
                [co, s, 0.0, 0.0],
                [-s, co, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ])
            .unwrap();
        let at = [c[0] * co - c[1] * s, c[0] * s + c[1] * co];
        let cut = boolean(read(cone), notch, BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        one_valid_solid_of_volume(
            &cut,
            [5, 10, 7],
            frustum - notch_in_the_seam_cones_rim(at),
            1e-9,
        );
    }
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

/// #21581: a 15 × 1.7 × 20 mm box crosses one side of a loft between two
/// identical rounded spline profiles. Integrating (x + 20) dz along the
/// cubic profile spans inside x <= -20 and z <= 0 gives 32.675328686804
/// mm². The two straight clipping boundaries contribute zero to that
/// integral, and the same section runs through the box's 1.7 mm width.
#[test]
fn a_box_notches_a_periodic_spline_loft_and_leaves_valid_trim_curves() {
    all_fixes();
    let body = traced_operands("issue_21581_loft_box_cut");
    let (loft, tool) = (body("loft"), body("box"));
    let before = volume(&loft);
    let cut = boolean(loft, tool, BooleanOp::Subtract).unwrap().unwrap();
    assert_eq!(cut.solid_count().unwrap(), 1);
    let removed = before - volume(&cut);
    assert!(near(removed, 32.675328686804 * 1.7), "{removed}");
    let full_check = cut.check(CheckLevel::All).unwrap();
    assert!(full_check.is_valid(), "{}", full_check.text);
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

/// #21931: #17118's cut the other way round, each of the ten wedges less the target. A wedge
/// reaches out of the target only where the target's groove rounds dip below its slopes near the
/// top, so what is left is the wedge less its part inside the target, which the intersect of the
/// target with the wedge gives. The sliver of a groove's round in the wedge's thin edge lies within
/// the tolerance of the wedge's plane, which the cut keeps (it lies outside the target), so the
/// sliver must be kept to close the leftover beside it. Deleted, the result was open; kept, it was
/// made in the wedge's outside, so no face split the wedge's inside and the result had no solid
/// region. Both came back as "not a closed solid".
///
/// The leftover is two pieces, at the two groove rounds the wedge's round passes into, each of
/// three faces of the wedge and two of the target's round (the sliver one of them): 5 faces,
/// 9 edges and 6 vertices (6 - 9 + 5 = 2). The cut
/// and the intersect split the wedge along the same faces of the target, the sliver among them
/// (the film between the sliver and the wedge's plane goes with the cut), so their volumes add
/// up to the wedge's exactly. The bound is the accuracy the integration is asked for, 1e-11 of
/// each volume, about 1.6e-11 over the wedge and the intersect, with a margin: 1e-10, against a
/// leftover of 6.7e-5.
#[test]
fn each_traced_wedge_less_the_grooved_target_leaves_the_part_of_the_wedge_outside_it() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    for i in 0..10 {
        let w = format!("wedge{i}");
        let inside = boolean(body("target"), body(&w), BooleanOp::Intersect)
            .unwrap()
            .unwrap()
            .volume(1e-11)
            .unwrap();
        let want = body(&w).volume(1e-11).unwrap() - inside;
        let r = boolean(body(&w), body("target"), BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{w}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 2, "{w}");
        assert_eq!(r.counts().unwrap(), [10, 18, 12], "{w}");
        let v = r.volume(1e-11).unwrap();
        assert!(
            (v - want).abs() <= 1e-10,
            "{w}: volume {v}, the wedge less its part inside the target {want}"
        );
    }
}

/// #21931: the intersect of each of the ten wedges with the target, the wedge first, is the
/// intersect the other way round, which builds. The same sliver of a groove's round, kept here
/// as the copy of the wedge's plane, was made in the wedge's outside, so the faces of the target
/// that bound the intersect split no region of the wedge and the result had no solid region.
/// Both orders keep the same faces, so the counts agree and the volumes differ by no more than
/// ten times the accuracy the integration is asked for.
#[test]
fn each_traced_wedge_intersected_with_the_grooved_target_is_the_intersect_either_way_round() {
    all_fixes();
    let body = traced_operands("issue_17118_wedge_cut_over_groove_rounds");
    for i in 0..10 {
        let w = format!("wedge{i}");
        let want = boolean(body("target"), body(&w), BooleanOp::Intersect)
            .unwrap()
            .unwrap();
        let r = boolean(body(&w), body("target"), BooleanOp::Intersect)
            .unwrap()
            .unwrap();
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{w}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "{w}");
        assert_eq!(r.counts().unwrap(), want.counts().unwrap(), "{w}");
        let (v, u) = (r.volume(1e-11).unwrap(), want.volume(1e-11).unwrap());
        assert!(
            (v - u).abs() <= 1e-10,
            "{w}: volume {v}, the other way round {u}"
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
///
/// WR-647 part A: the curve where the top crosses the corner round reaches the round's floor
/// edge only at its end vertex (4, 249.15, 7), so the curve/edge range there is that one
/// contact. The top's other boundary, the floor's line y = 249.15, meets the round's floor
/// arc only at that point, as its tangent; the face/face tangency witness proves the range
/// is the point from those two carriers.
#[test]
fn an_insert_whose_top_is_tangent_to_the_floor_at_a_corner_round_joins_its_bin() {
    insert_with_its_top_tangent_to_the_floor_joins_its_bin(200.0);
}

/// #18064's insert with its top's control point at y = 100 instead of 200: the top is ten
/// times flatter where it meets the floor, so the curve where it crosses the corner round
/// stays within the scaled zero of the round's floor edge for longer before the touch. The
/// witness makes the longer range at the round's end vertex the one contact there (WR-647
/// part A).
#[test]
fn an_insert_whose_flatter_top_is_tangent_to_the_floor_at_a_corner_round_joins_its_bin() {
    insert_with_its_top_tangent_to_the_floor_joins_its_bin(100.0);
}

/// The join of `insert_flush_with_a_wall(ym)` into `pocketed_bin`: the bin and the insert
/// less what they share, the insert's corners inside the walls outside the two rounds, its
/// full section there, of height the top's.
fn insert_with_its_top_tangent_to_the_floor_joins_its_bin(ym: f64) {
    all_fixes();
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

/// The lower rim of the clipped sphere has radius sqrt(10² − 6²) = 8. A bore starting
/// in that face opens to the exterior even when its rim is only one tolerance inside.
/// The volume alone cannot distinguish the opening from a sealed cylindrical void.
/// Once the rims are farther apart than their coincidence tolerance, an inner bore
/// leaves an annulus and an outer bore meets the sphere at z = −sqrt(10² − radius²).
#[test]
fn a_bore_starting_at_a_clipped_spheres_rim_opens_the_bottom() {
    use std::f64::consts::FRAC_1_SQRT_2;

    let sphere = make_sphere([0.0; 3], 10.0).unwrap();
    let above = make_box([-11.0, -11.0, 7.0], [22.0, 22.0, 11.0]).unwrap();
    let upper = boolean(sphere, above, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    check_clipped_sphere_bores(
        &upper,
        -6.0,
        &[
            -1e-5, -1e-4, -2e-5, -1.5e-5, -1.25e-5, -5e-6, 0.0, 1.01e-5, 1.1e-5, 1.25e-5, 1.5e-5,
            2e-5,
        ],
        [1.0, 0.0, 0.0],
    );
    // Rotating the seam rotates the rational tangent hull outside the face box.
    // The bottom face still has the analytic annular area checked below.
    check_clipped_sphere_bores(&upper, -6.0, &[-2e-5], [FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0]);
    // At z = −8 the true section is within a tolerance of the cutter's cap,
    // while the old rim is more than a tolerance from the cutter's wall.
    check_clipped_sphere_bores(&upper, -8.0, &[1.25e-5], [1.0, 0.0, 0.0]);
}

#[allow(clippy::disallowed_methods)] // the test's closed forms
fn check_clipped_sphere_bores(upper: &Body, base_z: f64, offsets: &[f64], reference: [f64; 3]) {
    let z = [0.0, 0.0, 1.0];
    let below = make_box([-11.0, -11.0, base_z - 11.0], [22.0, 22.0, 11.0]).unwrap();
    let target = boolean(upper.copy().unwrap(), below, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let target_volume = volume(&target);
    let rim = (100.0 - base_z * base_z).sqrt();
    for &offset in offsets {
        let radius = rim + offset;
        let circle = Curve::Arc {
            center: [0.0, 0.0, base_z],
            axis: z,
            reference,
            radius,
            start: 0.0,
            end: TAU,
        };
        let bore = extrude(&[circle], z, -base_z, 0.0, DraftCorner::default(), true).unwrap();
        let cut = boolean(target.copy().unwrap(), bore, BooleanOp::Subtract)
            .map_err(|error| (base_z, offset, error))
            .unwrap()
            .unwrap();
        let (record, order) = cut
            .to_record()
            .map_err(|error| (base_z, offset, error))
            .unwrap();
        if offset.abs() > 1e-5 {
            let bottom: Vec<_> = record
                .faces
                .iter()
                .enumerate()
                .filter(|(_, face)| match &face.surface {
                    Surface::Plane { origin, .. } | Surface::PlaneThrough { origin, .. } => {
                        (origin[2] - base_z).abs() < 1e-8
                    }
                    _ => false,
                })
                .map(|(i, _)| i)
                .collect();
            if offset < 0.0 {
                let area: f64 = bottom
                    .iter()
                    .map(|&face| cut.face_area(&order, face, 1e-10).unwrap())
                    .sum();
                let want = PI * (rim * rim - radius * radius);
                assert!(
                    (area - want).abs() < 1e-8,
                    "z={base_z}, offset {offset}: bottom area {area}, expected {want}"
                );
            } else {
                assert!(
                    bottom.is_empty(),
                    "z={base_z}, offset {offset}: bottom faces {bottom:?}"
                );
                let lowest = record
                    .vertices
                    .iter()
                    .map(|v| v.point[2])
                    .fold(f64::INFINITY, f64::min);
                let height = -(100.0 - radius * radius).sqrt();
                assert!(
                    (lowest - height).abs() < 1e-8,
                    "z={base_z}, offset {offset}: lowest vertex {lowest}, expected {height}"
                );
            }
        }
        // Both rays see the ceiling except where the wider inset leaves material at the rim.
        let near_rim = rim - 5e-5;
        let rim_hit_z = if near_rim > radius { base_z } else { 0.0 };
        for (x, height) in [(0.0, 0.0), (near_rim, rim_hit_z)] {
            let hit = cut
                .face_hit_by_ray(&order, [x, 0.0, base_z - 1.0], z)
                .unwrap()
                .unwrap();
            assert!(
                (hit.point[2] - height).abs() < 1e-8,
                "z={base_z}, offset {offset}, x {x}: {:?}",
                hit.point
            );
        }
        assert_eq!(record.shells.len(), 1, "z={base_z}, offset {offset}");
        // Beyond the rim the tool includes a thin slice outside the sphere.
        let z0 = (100.0 - radius * radius).sqrt().min(-base_z);
        let h = -base_z - z0;
        let want = target_volume + PI * radius * radius * base_z + PI * h * h * (z0 + h / 3.0);
        assert!(near(volume(&cut), want), "z={base_z}, offset {offset}");
        let check = cut.check(CheckLevel::All).unwrap();
        assert!(
            check.is_valid(),
            "z={base_z}, offset {offset}: {}",
            check.text
        );
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

#[test]
fn union_all_mixed_touching_and_apart_preserves_volume_solids_and_validity() {
    let mut bodies = vec![
        make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap(),
        make_box([5.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap(),
    ];
    for i in 0..20_u32 {
        let x = 50.0 + f64::from(i) * 20.0;
        bodies.push(make_box([x, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap());
    }
    let u = union_all_with(bodies, Contract::Proved).unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 21);
    assert_eq!(u.counts().unwrap(), [126, 252, 168]);
    let want = 1500.0 + 20.0 * 1000.0;
    let got = volume(&u);
    assert!(near(got, want), "volume {got} vs {want}");
}

#[test]
fn union_all_shuffled_chain_and_hub_spokes_preserve_volume_and_validity() {
    // Shuffled overlapping box chain
    let n = 16_u32;
    let order: [u32; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
    let chain: Vec<Body> = order
        .iter()
        .map(|&i| make_box([f64::from(i) * 5.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap())
        .collect();
    let u = union_all_with(chain, Contract::Proved).unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_eq!(u.counts().unwrap(), [6, 12, 8]);
    let want = (10.0 + f64::from(n - 1) * 5.0) * 100.0;
    assert!(near(volume(&u), want), "chain {} vs {want}", volume(&u));

    // Hub (plate) + 16 mutually disjoint spokes (bosses)
    let mut hub_spokes = vec![make_box([0.0, 0.0, 0.0], [100.0, 100.0, 5.0]).unwrap()];
    for r in 0..4_u32 {
        for c in 0..4_u32 {
            hub_spokes.push(
                make_box(
                    [10.0 + f64::from(c) * 20.0, 10.0 + f64::from(r) * 20.0, 5.0],
                    [6.0, 6.0, 8.0],
                )
                .unwrap(),
            );
        }
    }
    let u = union_all_with(hub_spokes, Contract::Proved).unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_eq!(u.counts().unwrap(), [86, 204, 136]);
    let want = 100.0 * 100.0 * 5.0 + 16.0 * (6.0 * 6.0 * 8.0);
    assert!(near(volume(&u), want), "hub {} vs {want}", volume(&u));

    // Nested cube inside a hollow box's cavity: overlapping AABBs must NOT take disjoint path.
    let outer = make_box([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]).unwrap();
    let cavity = make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
    let hollow = boolean(outer, cavity, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let inner = make_box([7.0, 7.0, 7.0], [6.0, 6.0, 6.0]).unwrap();
    let nested = union_all_with(vec![hollow, inner], Contract::Proved).unwrap();
    let report = nested.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(nested.counts().unwrap(), [18, 36, 24]);
    let want = (8000.0 - 1000.0) + 216.0;
    assert!(
        near(volume(&nested), want),
        "nested {} vs {want}",
        volume(&nested)
    );
}

/// acid-10's near miss: a 50 mm block, joined (or not) by a Ø20 cylinder along x lying 2e-5
/// deep in its top face, then joined by an r 15 ball centred 14.99998 from its +x face, so
/// that the ball dips 2e-5 into it, twice the zone tolerance. They meet in a circle of radius
/// √(2 · 15 · 2e-5 − 4e-10) = 0.0245, at 0.094° to each other. At that angle the radial sector
/// steps off the circle until the ball's face and the block's are more than the tolerance
/// apart; upstream's secant step aimed at the tolerance itself, settled 0.009 from the
/// circle with the faces exactly that far apart, and read the unchanging distance as a
/// coincidence. Taken for a copy of the block face, the ball's face was put in the block's
/// material, and the union deleting the disc the ball covers merged the block into the
/// outside: refused. The step now aims at twice the tolerance, the faces part, and the ball's
/// face is outside the block.
///
/// The volume is the closed form: block, cylinder and ball, less the cap the ball dips in,
/// π d² (3R − d) / 3 with d = 2e-5, and the cylinder's segment under the top face over its
/// 50 mm, (r² acos((r − d) / r) − (r − d) √(2 r d − d²)) · 50; the ball and cylinder are
/// apart (the ball's top is at z = 40, the cylinder's bottom at 49.99998).
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_ball_dipped_twice_the_tolerance_into_a_block_face_joins_it() {
    all_fixes();
    let (d, rb, rc): (f64, f64, f64) = (2e-5, 15.0, 10.0);
    let cap = PI * d * d * (3.0 * rb - d) / 3.0;
    let segment =
        (rc * rc * ((rc - d) / rc).acos() - (rc - d) * (2.0 * rc * d - d * d).sqrt()) * 50.0;
    for with_cylinder in [false, true] {
        let mut part = make_box([0.0; 3], [50.0; 3]).unwrap();
        let mut want = 125_000.0 + 4.0 / 3.0 * PI * rb.powi(3) - cap;
        if with_cylinder {
            let v = solidcore_record::Vec3::new;
            let plane = Plane {
                origin: v(0.0, 25.0, 60.0 - d),
                x: v(0.0, 1.0, 0.0),
                y: v(0.0, 0.0, 1.0),
            };
            let region = Region2 {
                outer: Loop2::circle_from(Vec2::new(0.0, 0.0), rc, 0.6131),
                holes: vec![],
            };
            let cylinder = extrude_planar_regions(&plane, &[region], 0.0, 50.0).unwrap();
            part = boolean(part, cylinder, BooleanOp::Union).unwrap().unwrap();
            want += PI * rc * rc * 50.0 - segment;
        }
        let ball = make_sphere([65.0 - d, 25.0, 25.0], rb).unwrap();
        let u = boolean(part, ball, BooleanOp::Union)
            .unwrap_or_else(|e| panic!("cylinder {with_cylinder}: {e:?}"))
            .unwrap();
        assert_eq!(u.solid_count().unwrap(), 1, "cylinder {with_cylinder}");
        assert!(u.is_manifold_solid().unwrap(), "cylinder {with_cylinder}");
        assert!(
            near(volume(&u), want),
            "cylinder {with_cylinder}: {} vs {want}",
            volume(&u)
        );
        // In the ball beyond the block face, at its middle and where the ball dips in.
        for p in [
            [79.0, 25.0, 25.0],
            [50.000_005, 25.0, 25.0],
            [49.999_99, 25.0, 25.0],
        ] {
            assert_eq!(
                u.classify_point(p).unwrap(),
                PointClass::Inside,
                "cylinder {with_cylinder}: {p:?}"
            );
        }
    }
}

/// The same ball on the block SolidCraft's Box builds: a 50 mm square swept 50 along +z, its
/// four sides lines, rather than `make_box`'s block (solidcraft#20878, a 0.50 nightly built
/// before the parting step above). The ball, r 15 centred at x = 64.99998, dips d = 2e-5 into
/// the +x face, twice the zone tolerance, so the union is one solid: the block and the ball
/// less the cap the ball dips in, 125000 + 4500 π − π d² (3R − d) / 3 = 139137.166941.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_ball_dipped_twice_the_tolerance_into_a_swept_block_face_joins_it() {
    all_fixes();
    let (d, rb): (f64, f64) = (2e-5, 15.0);
    let corners = [
        [0.0, 0.0, 0.0],
        [50.0, 0.0, 0.0],
        [50.0, 50.0, 0.0],
        [0.0, 50.0, 0.0],
    ];
    let square: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let block = extrude(
        &square,
        [0.0, 0.0, 1.0],
        50.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    let ball = make_sphere([64.999_98, 25.0, 25.0], rb).unwrap();
    let u = boolean(block, ball, BooleanOp::Union).unwrap().unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert!(u.is_manifold_solid().unwrap());
    let want = 125_000.0 + 4.0 / 3.0 * PI * rb.powi(3) - PI * d * d * (3.0 * rb - d) / 3.0;
    assert!(near(volume(&u), want), "{} vs {want}", volume(&u));
    // In the ball beyond the block face, at its middle and where the ball dips in.
    for p in [
        [79.0, 25.0, 25.0],
        [50.000_005, 25.0, 25.0],
        [49.999_99, 25.0, 25.0],
    ] {
        assert_eq!(u.classify_point(p).unwrap(), PointClass::Inside, "{p:?}");
    }
}

/// A 40 × 30 × 10 box and an r 5 cylinder lying along its top face, 50 long so that it runs
/// past both ends, sunk d into it just past the zone tolerance (1e-5 per edge, so t = 2e-5 for
/// two edges), its seam at five angles from its lowest line (solidcraft#21194's sweep). The
/// cylinder meets the top face in two lines h = √(2 r d − d²) either side of y = 15 (0.0297
/// apart at d = 2.2e-5), leaving it at asin(h / r) = 0.17°. Between them the plane and the
/// cylinder's lowest strip part to d at the middle and close again, and the sector search,
/// stepping off a line until the two are more than t apart, must stop before the middle: past
/// it the cylinder turns back up and is read on the other side of the plane. Aimed at 2t the
/// steps went past it (union refused, the difference left the box whole), and upstream's step,
/// aimed at t through 0, refused d = 2e-5 to 2.05e-5.
///
/// The top face keeps the two strips outside the lines in both operations, of area
/// 40 (30 − 2h). The difference takes out the groove, the circular segment of depth d,
/// r² · 2 asin(√(d / 2r)) − (r − d) h, over the box's 40, and its floor is the cylinder's arc
/// between the lines, 2 r asin(h / r) wide; the union adds the cylinder less that groove.
/// The faces cross at the small angle θ = asin(h / r), so each line is fixed along them only
/// to the edge tolerance over sin θ (`booleans.md`, near-coincident strips): the areas are
/// checked within 40 · 2 · 1e-5 / sin θ = 0.28 of the closed form, a quarter of the strip
/// (1.15 to 1.19) a refused cut would leave.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_cylinder_sunk_just_past_the_tolerance_into_a_box_face_joins_and_cuts_it() {
    all_fixes();
    let r: f64 = 5.0;
    for d in [2.05e-5, 2.1e-5, 2.2e-5] {
        let h = (2.0 * r * d - d * d).sqrt();
        let groove = (r * r * 2.0 * (d / (2.0 * r)).sqrt().asin() - (r - d) * h) * 40.0;
        let top = 40.0 * (30.0 - 2.0 * h);
        let floor = 40.0 * 2.0 * r * (h / r).asin();
        let slack = 40.0 * 2.0 * 1e-5 / (h / r);
        for seam in [0.0, 0.01, 0.6131, PI / 2.0, PI] {
            let (sin, cos) = seam.sin_cos();
            for op in [BooleanOp::Union, BooleanOp::Subtract] {
                let case = format!("d {d}, seam {seam}, {op:?}");
                let block = make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap();
                // Its seam (the local x axis) at `seam` from straight down, its axis along x.
                let cylinder = solidcore_ops::make_cylinder([0.0; 3], r, 50.0).unwrap();
                cylinder
                    .transform(&[
                        [0.0, sin, -cos, 0.0],
                        [0.0, cos, sin, 0.0],
                        [1.0, 0.0, 0.0, 0.0],
                        [-5.0, 15.0, 15.0 - d, 1.0],
                    ])
                    .unwrap();
                let got = boolean(block, cylinder, op)
                    .unwrap_or_else(|e| panic!("{case}: {e:?}"))
                    .unwrap();
                assert_eq!(got.solid_count().unwrap(), 1, "{case}");
                assert!(got.is_manifold_solid().unwrap(), "{case}");
                assert!(got.check(CheckLevel::All).unwrap().is_valid(), "{case}");
                let want = match op {
                    BooleanOp::Union => 12_000.0 + PI * r * r * 50.0 - groove,
                    _ => 12_000.0 - groove,
                };
                assert!(
                    near(volume(&got), want),
                    "{case}: {} vs {want}",
                    volume(&got)
                );
                let (record, order) = got.to_record().unwrap();
                let area_on = |on: &dyn Fn(&Surface) -> bool| -> f64 {
                    (0..record.faces.len())
                        .filter(|&f| on(&record.faces[f].surface))
                        .map(|f| got.face_area(&order, f, 1e-10).unwrap())
                        .sum()
                };
                let on_top = area_on(&|s| match s {
                    Surface::Plane {
                        origin,
                        u_axis,
                        v_axis,
                    } => (origin[2] - 10.0).abs() < 1e-12 && u_axis[2] == 0.0 && v_axis[2] == 0.0,
                    Surface::PlaneThrough {
                        origin,
                        u_point,
                        v_point,
                    } => [origin, u_point, v_point]
                        .iter()
                        .all(|p| (p[2] - 10.0).abs() < 1e-12),
                    _ => false,
                });
                assert!(
                    (on_top - top).abs() <= slack,
                    "{case}: top {on_top} vs {top}"
                );
                if op == BooleanOp::Subtract {
                    let on_cylinder = area_on(&|s| matches!(s, Surface::Cylinder { .. }));
                    assert!(
                        (on_cylinder - floor).abs() <= slack,
                        "{case}: floor {on_cylinder} vs {floor}"
                    );
                }
            }
        }
    }
}

/// The length of the line through (x, z) parallel to y that lies in the cube −20..20 less a
/// Ø18 hole along each axis through its centre and Ø8 holes along x at y = −10 and y = 10
/// (z = 0): 40 less the union of the y intervals the holes take. A hole along y (x² + z² < 81)
/// takes the whole line.
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn drilled_cube_chord(x: f64, z: f64) -> f64 {
    if x * x + z * z < 81.0 {
        return 0.0;
    }
    let mut taken = [
        (0.0, 81.0 - z * z),
        (0.0, 81.0 - x * x),
        (-10.0, 16.0 - z * z),
        (10.0, 16.0 - z * z),
    ]
    .map(|(centre, r2): (f64, f64)| {
        let h = r2.max(0.0).sqrt();
        ((centre - h).max(-20.0), (centre + h).min(20.0))
    });
    taken.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut length, mut lo, mut hi) = (0.0, taken[0].0, taken[0].1);
    for &(a, b) in &taken[1..] {
        if a <= hi {
            hi = hi.max(b);
        } else {
            length += hi - lo;
            (lo, hi) = (a, b);
        }
    }
    40.0 - (length + hi - lo)
}

/// acid-4's crossfire: a 40 mm cube about the origin drilled by Ø18 holes along x, y and z
/// and by Ø8 holes along x at y = ±10, then cut by a Ø8 hole along y at x = −10 whose axis
/// passes 1e-5 above the two Ø8 x holes' axes. Equal holes whose axes cross a hair apart meet
/// in curves that nearly pinch: each hole's lens in the other's wall ends in a tip 0.009 mm
/// across, and the two lenses' tips are 0.018 mm apart, the walls within 1e-5 of each other
/// between them. A face of the new hole runs through that neck, and the midpoint of its edge
/// round a lens tip, where the radial sector classified it first, lies in the neck: the walls
/// leave the edge there 0.13 degrees apart, the steps off the edge ran across the neck and on
/// past the other lens's tip, where the walls have crossed back, and read the face as lying
/// in the holes' space. Copied into the outside, the face's edges found no face use in the
/// solid, and the cut was refused. A point of the edge nearer its ends, where the walls cross
/// at a clear angle (36 degrees at the ends), now decides at the edge, before any point is
/// stepped off.
///
/// The cubes and holes are built as SolidCraft builds them: each hole an extrusion of a circle
/// starting 0.6131 rad round from its plane's x axis (its seam). The volume the cut removes is
/// the integral over the hole's section of [`drilled_cube_chord`], by the midpoint rule on a
/// polar grid (2000 rings of 8000 cells); the rule gives 471.3501 on half that grid each way
/// and 471.3519 on this one, so 0.01 bounds its error with room.
#[test]
#[allow(clippy::disallowed_methods)] // the test's reference integral, not ops computing geometry
fn a_hole_crossing_equal_holes_a_hair_off_their_axes_cuts_a_drilled_cube() {
    all_fixes();
    let seam = 0.6131;
    let hole = |origin: [f64; 3], x: [f64; 3], y: [f64; 3], r: f64| {
        let v = |p: [f64; 3]| solidcore_record::Vec3::new(p[0], p[1], p[2]);
        let plane = Plane {
            origin: v(origin),
            x: v(x),
            y: v(y),
        };
        let region = Region2 {
            outer: Loop2::circle_from(Vec2::new(0.0, 0.0), r, seam),
            holes: vec![],
        };
        extrude_planar_regions(&plane, &[region], 0.0, 42.0).unwrap()
    };
    let (ex, ey, ez, mx) = (
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    );
    let mut part = make_box([-20.0; 3], [40.0; 3]).unwrap();
    for tool in [
        hole([-21.0, 0.0, 0.0], ey, ez, 9.0),
        hole([0.0, -21.0, 0.0], mx, ez, 9.0),
        hole([0.0, 0.0, -21.0], ex, ey, 9.0),
        hole([-21.0, -10.0, 0.0], ey, ez, 4.0),
        hole([-21.0, 10.0, 0.0], ey, ez, 4.0),
    ] {
        part = boolean(part, tool, BooleanOp::Subtract).unwrap().unwrap();
    }
    let before = volume(&part);
    let cut = boolean(
        part,
        hole([-10.0, -21.0, 1e-5], mx, ez, 4.0),
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    assert!(cut.is_manifold_solid().unwrap());
    assert_eq!(cut.solid_count().unwrap(), 1);
    let (rings, cells) = (2000_u32, 8000_u32);
    let mut want = 0.0;
    for i in 0..rings {
        let rho = 4.0 * (f64::from(i) + 0.5) / f64::from(rings);
        for j in 0..cells {
            let t = TAU * (f64::from(j) + 0.5) / f64::from(cells);
            want += drilled_cube_chord(-10.0 + rho * t.cos(), 1e-5 + rho * t.sin()) * rho;
        }
    }
    want *= (4.0 / f64::from(rings)) * (TAU / f64::from(cells));
    let removed = before - volume(&cut);
    assert!(
        (removed - want).abs() <= 0.01,
        "removed {removed}, the integral gives {want}"
    );
    // Inside the new hole, where it crosses the x hole at y = 10 and where it runs through
    // material on its own; material beside it.
    assert_eq!(
        cut.classify_point([-10.0, 10.0, 3.0]).unwrap(),
        PointClass::Outside
    );
    assert_eq!(
        cut.classify_point([-10.0, 16.0, 0.0]).unwrap(),
        PointClass::Outside
    );
    assert_eq!(
        cut.classify_point([-15.0, 16.0, 0.0]).unwrap(),
        PointClass::Inside
    );
}

/// #21801's insert: a prism over the quadrilateral (2.85, 7), (26.868841, 7), (9.247749, 28),
/// (2.85, 28) in x-z from y = 2.85 to 249.15, less a loft through three sections at x = -5,
/// 95, 195 whose quadratic runs from (y, z) = (2.85, 28) to (249.15, 7) with its middle
/// control point at y = ym, ym - 50, ym and z = 7. The loft meets z = 7 tangentially at
/// y = 249.15, so the insert ends in a knife along the bin's floor there.
fn fitted_loft_insert(ym: f64) -> Body {
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
    let above = |x: f64, ym: f64| {
        vec![
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
        ]
    };
    let cut = solidcore_ops::sweep::loft(
        &[above(-5.0, ym), above(95.0, ym - 50.0), above(195.0, ym)],
        2,
        true,
    )
    .unwrap();
    boolean(prism, cut, BooleanOp::Subtract).unwrap().unwrap()
}

/// #21801: with `simple_coincidence_crossing_is_point` off, the union of #18064's bin with
/// the fitted-loft insert failed with `NotSolid`: imprinting the vertex where the loft's
/// section curve leaves the bin's floor arc at (4, 248) split the arc, and the vertex
/// definition that split added to the same proto vertex was never imprinted, so the insert's
/// bottom face was not split along the arc. With that imprinted, it failed with
/// `RelateConflict`: the insert's bottom-face arc edge and its top-face section edge along the
/// same arc had no certified coincidence bound (an ellipse against a B-spline).
///
/// The union now gets through the imprint and the relations. It still refuses in
/// `manifold_boolean`'s leak check (#21822: the bin's wall is classified against only one of
/// the insert's two coincident edges); once #21822 is fixed this test requires the joined
/// solid. The volume of that solid lies in a certified enclosure: the bin and the insert
/// share no volume except in the two pocket corners the insert covers, outside the rounds
/// about (4, 4) and (4, 248), each of area 1.15^2 (1 - pi / 4), between z = 7 and the
/// insert's top, at most z = 28; their sum is at most `share`.
#[test]
fn a_fitted_loft_insert_with_a_knife_on_the_floor_gets_through_imprint_and_relate() {
    let no_crossing = solidcore::kernel::UpstreamFixes::NAMES
        .iter()
        .copied()
        .filter(|n| *n != "simple_coincidence_crossing_is_point");
    session::set_thread_fixes(no_crossing).unwrap();
    let bin = pocketed_bin();
    let insert = fitted_loft_insert(200.0);
    let (vb, vi) = (volume(&bin), volume(&insert));
    let share = 2.0 * 1.15 * 1.15 * (1.0 - PI / 4.0) * 21.0;
    let _ = (session::take_failure_code(), session::take_failure_detail());
    let joined = boolean(bin, insert, BooleanOp::Union);
    let (code, detail) = (session::take_failure_code(), session::take_failure_detail());
    match joined {
        Ok(Some(r)) => {
            let report = r.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{}", report.text);
            assert!(r.is_manifold_solid().unwrap());
            let got = volume(&r);
            // each of the three volumes is computed to `1e-8` relative accuracy (`volume`)
            let slack = 1e-8 * (vb + vi + got);
            assert!(
                got >= vb + vi - share - slack && got <= vb + vi + slack,
                "volume {got} outside [{}, {}]",
                vb + vi - share,
                vb + vi
            );
        }
        Ok(None) => panic!("the union of two solids is empty"),
        Err(e) => {
            let code = code.unwrap_or_else(|| panic!("{e:?} without a failure code"));
            assert_eq!(
                code.operation, "boolean::merge::p4::manifold_boolean",
                "{e:?} ({code}): refused before the leak check (#21822)"
            );
            // the leak check's own cause (`BooleanFailure::SolidLeaks`), not another refusal there
            let detail = detail.unwrap_or_default();
            assert!(
                detail.contains("the solid it bounded merged into the outside"),
                "{e:?}: {detail}"
            );
        }
    }
}

#[test]
fn make_other_faces_handles_many_disjoint_touching_and_nested_solids() {
    all_fixes();
    // Primary: 12 apart unit cubes plus one small cube inside what will become a cavity of `other`.
    let mut primary_parts = Vec::new();
    for i in 0..12 {
        primary_parts.push(make_box([f64::from(i) * 3.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap());
    }
    // Small primary cube centered at (100, 5, 5) — will lie strictly inside `other`'s outer hollow box.
    primary_parts.push(make_box([99.5, 4.5, 4.5], [1.0, 1.0, 1.0]).unwrap());
    let primary = union_all(primary_parts).unwrap();

    // Other: 12 apart unit cubes (disjoint from primary), 4 boxes overlapping primary cubes 0..4,
    // and a hollow box [95..105, 0..10, 0..10] with cavity [97..103, 2..8, 2..8] enclosing primary's
    // small cube at (100, 5, 5).
    let mut other_parts = Vec::new();
    for i in 0..12 {
        other_parts.push(make_box([f64::from(i) * 3.0, 5.0, 0.0], [1.0, 1.0, 1.0]).unwrap());
    }
    for i in 0..4 {
        other_parts.push(make_box([f64::from(i) * 3.0 + 0.5, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap());
    }
    let outer = make_box([95.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
    let inner = make_box([97.0, 2.0, 2.0], [6.0, 6.0, 6.0]).unwrap();
    let hollow = boolean(outer, inner, BooleanOp::Subtract).unwrap().unwrap();
    other_parts.push(hollow);
    let other = union_all(other_parts).unwrap();

    let joined = boolean(primary, other, BooleanOp::Union).unwrap().unwrap();
    let report = joined.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // Expected volume:
    // - 8 untouched primary cubes: 8.0
    // - 4 overlapped pairs (each 1.5 x 1 x 1): 6.0
    // - 12 disjoint other cubes: 12.0
    // - hollow shell (10^3 - 6^3 = 784) + enclosed small primary cube (1.0): 785.0
    let expected = 8.0 + 6.0 + 12.0 + 784.0 + 1.0;
    let got = volume(&joined);
    assert!(near(got, expected), "got {got}, expected {expected}");
    assert_eq!(joined.solid_count().unwrap(), 8 + 4 + 12 + 2);
}

/// Build a closed faceted UV sphere (`nlat` bands × `2*nlat` longitudes, so `2*nlat^2` planar
/// faces: triangles at poles and planar trapezoids in between) matching #18211's reproducer.
#[allow(clippy::disallowed_methods)] // test fixture construction, not ops computing geometry
fn faceted_uv_sphere(nlat: usize, radius: f64) -> Body {
    use solidcore_record::brep::{
        Brep, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace, Region,
        Shell, Surface, Vertex, VERSION,
    };
    let nlon = 2 * nlat;
    let off = PI / nlon as f64;
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let north = 0u32;
    pts.push([0.0, 0.0, radius]);
    for i in 1..nlat {
        let theta = PI * i as f64 / nlat as f64;
        let st = theta.sin();
        let ct = theta.cos();
        for j in 0..nlon {
            let phi = 2.0 * PI * j as f64 / nlon as f64 + off;
            pts.push([
                radius * st * phi.cos(),
                radius * st * phi.sin(),
                radius * ct,
            ]);
        }
    }
    let south = pts.len() as u32;
    pts.push([0.0, 0.0, -radius]);

    let ring = |i: usize, j: usize| -> u32 { 1 + ((i - 1) * nlon + (j % nlon)) as u32 };
    let mut polys: Vec<Vec<u32>> = Vec::with_capacity(nlat * nlon);
    for j in 0..nlon {
        // Outward CCW viewed from outside (+normal): (north, ring(1, j), ring(1, j+1))
        polys.push(vec![north, ring(1, j), ring(1, j + 1)]);
    }
    for i in 1..nlat - 1 {
        for j in 0..nlon {
            polys.push(vec![
                ring(i, j),
                ring(i + 1, j),
                ring(i + 1, j + 1),
                ring(i, j + 1),
            ]);
        }
    }
    for j in 0..nlon {
        polys.push(vec![ring(nlat - 1, j), south, ring(nlat - 1, j + 1)]);
    }

    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let norm = |a: [f64; 3]| {
        let l = dot(a, a).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };

    let tol = 1e-6;
    let mut edge_map: std::collections::BTreeMap<(u32, u32), u32> =
        std::collections::BTreeMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut faces: Vec<Face> = Vec::with_capacity(polys.len());

    for (fi, poly) in polys.iter().enumerate() {
        let p0 = pts[poly[0] as usize];
        let p1 = pts[poly[1] as usize];
        let p2 = pts[poly[2] as usize];
        let u_axis = norm(sub(p1, p0));
        let n = norm(cross(sub(p1, p0), sub(p2, p0)));
        let v_axis = norm(cross(n, u_axis));
        let uv = |p: [f64; 3]| {
            let d = sub(p, p0);
            [dot(d, u_axis), dot(d, v_axis)]
        };
        let (mut umin, mut umax, mut vmin, mut vmax) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for &vi in poly {
            let [u, v] = uv(pts[vi as usize]);
            umin = umin.min(u);
            umax = umax.max(u);
            vmin = vmin.min(v);
            vmax = vmax.max(v);
        }
        let pad = 1e-3;
        let mut oedges = Vec::with_capacity(poly.len());
        for (pos, &a) in poly.iter().enumerate() {
            let b = poly[(pos + 1) % poly.len()];
            let (lo, hi, forward) = if a < b { (a, b, true) } else { (b, a, false) };
            let ei = *edge_map.entry((lo, hi)).or_insert_with(|| {
                let idx = edges.len() as u32;
                edges.push(Edge {
                    curve: Curve::Line {
                        start: pts[lo as usize],
                        end: pts[hi as usize],
                    },
                    nurbs: None,
                    interval: [0.0, 1.0],
                    vertices: [lo, hi],
                    tolerance: tol,
                    uses: Vec::new(),
                });
                idx
            });
            edges[ei as usize].uses.push(EdgeUseRef {
                face: fi as u32,
                r#loop: 0,
                position: pos as u32,
                upper: !forward,
            });
            oedges.push(OrientedEdge {
                edge: ei,
                forward,
                pcurve: Curve2::Spline {
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    points: vec![uv(pts[lo as usize]), uv(pts[hi as usize])],
                },
            });
        }
        faces.push(Face {
            surface: Surface::Plane {
                origin: p0,
                u_axis,
                v_axis,
            },
            nurbs: None,
            domain: [[umin - pad, umax + pad], [vmin - pad, vmax + pad]],
            loops: vec![Loop::Edges { edges: oedges }],
        });
    }

    let oriented_faces = (0..faces.len() as u32)
        .map(|face| OrientedFace {
            face,
            forward: true,
        })
        .collect();
    let vertices = pts
        .into_iter()
        .map(|point| Vertex {
            point,
            tolerance: tol,
        })
        .collect();
    let record = Brep {
        version: VERSION,
        tolerance: tol,
        regions: vec![
            Region {
                shells: vec![0],
                void: false,
            },
            Region {
                shells: vec![0],
                void: false,
            },
        ],
        shells: vec![Shell::Faces {
            faces: oriented_faces,
            wires: Vec::new(),
        }],
        faces,
        edges,
        vertices,
    };
    Body::from_record(&record, true).unwrap().0
}

/// Issue #18211: splitting a closed planar-faceted UV sphere at the XZ plane produces
/// both closed hemispheres with exact face/edge/vertex counts and volume summing to the
/// original sphere.
#[test]
fn faceted_sphere_plane_split_produces_both_halves() {
    all_fixes();
    let nlat = 16usize;
    let sphere_a = faceted_uv_sphere(nlat, 10.0);
    let sphere_b = faceted_uv_sphere(nlat, 10.0);
    let full_vol = volume(&sphere_a);
    let pos_box = make_box([-60.0, 0.0, -60.0], [120.0, 60.0, 120.0]).unwrap();
    let neg_box = make_box([-60.0, -60.0, -60.0], [120.0, 60.0, 120.0]).unwrap();

    let pos_half = boolean(sphere_a, pos_box, BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    let neg_half = boolean(sphere_b, neg_box, BooleanOp::Intersect)
        .unwrap()
        .unwrap();

    assert_eq!(pos_half.counts().unwrap(), [273, 543, 272]);
    assert_eq!(neg_half.counts().unwrap(), [273, 543, 272]);
    let pos_vol = volume(&pos_half);
    let neg_vol = volume(&neg_half);
    assert!(
        (pos_vol - neg_vol).abs() < 1e-6,
        "halves differ: {pos_vol} vs {neg_vol}"
    );
    assert!(
        (pos_vol + neg_vol - full_vol).abs() < 1e-6,
        "sum {} != full {full_vol}",
        pos_vol + neg_vol
    );
}
