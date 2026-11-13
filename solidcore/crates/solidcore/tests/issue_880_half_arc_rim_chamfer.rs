//! tangency-labs/solidcore#880 (WR-582): a chamfer of one edge of a smooth rim chain. The
//! fork refused SolidCraft's chamfer of half the elliptical rim a flat cut leaves on its
//! tilted cylinder with "a rail leaves the face it rolls on" (`NO_ROOM`); pinned SMLib
//! refuses it with the generic 1001.
//!
//! SolidCraft's `build::cylinder` (at 6c60f1a9) extrudes `Loop2::circle_from(0, r,
//! SEAM_ANGLE)`: two half-circle arcs from `SEAM_ANGLE = 0.6131` rad, on
//! `Plane::from_normal(base, axis)`, whose x axis is `Z × n` projected. Here the same two
//! arcs (radius 5, from 35.128 to 215.128 degrees and from -144.872 to 35.128) are swept
//! 40 along +Z and capped (`create_linear_sweep`), then placed from (0, 0, -10) with
//! x = (0, 1, 0) and y = n × x for n = (1, 0, 2) / `√5`, and cut by the box above
//! z = 10. The wall is two faces and the rim two half-ellipse edges meeting tangentially
//! at the seam lines. The chamfers use a constant radius solver given a second radius, a
//! linear cross section and bevel corners, at the default fixes.
//!
//! One half edge alone (the issue's pick: offsets from setbacks 1 on the cut face and 2
//! down the wall, as the fork's `sides_offsets` derives them) builds a closed solid with a
//! clean check (4 faces to 7), since the generic blend's sections and checks are the
//! default (WR-685 addendum 2026-11-06); main's default kernel refused it with 1001. The
//! whole chain, both halves in one session, builds a closed solid (4 faces to 6).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, transform, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId, EdgeuseId};
use solidcore::{Brep, Kernel};

/// SolidCraft's `SEAM_ANGLE`, in radians.
const SEAM_ANGLE: f64 = 0.6131;

/// SolidCraft's tilted cylinder, cut flat at z = 10.
fn cut_cylinder(k: &mut Kernel) -> Brep {
    let seam = SEAM_ANGLE.to_degrees();
    // (`create_arc` takes degrees within [-360, 360]: the second half circle, from
    // seam + 180 to seam + 360, is given as seam - 180 to seam)
    let a = Curve::from(create_arc(Point3::ZERO, 5.0, seam, seam + 180.0).unwrap());
    let b = Curve::from(create_arc(Point3::ZERO, 5.0, seam - 180.0, seam).unwrap());
    let up = Vec3::new(0.0, 0.0, 1.0);
    let mut cylinder = create_linear_sweep(k, &[Some(&a), Some(&b)], up, 40.0, true).unwrap();
    let s = 5.0_f64.sqrt();
    let placement = Axis2Placement::new(
        Point3::new(0.0, 0.0, -10.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-2.0 / s, 0.0, 1.0 / s),
    );
    transform(k, &mut cylinder, &placement, None, false).unwrap();
    let cut = create_box(k, Point3::new(-50.0, -50.0, 10.0), 100.0, 100.0, 40.0).unwrap();
    boolean_difference(k, cylinder, cut).unwrap()
}

/// The edges whose middle lies at z = 10, in edge list order.
fn rims(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let Some(curve) = edge::curve(b, e).unwrap() else {
                return false;
            };
            let mid = edge::interval(b, e).unwrap().evaluate(0.5);
            (curve.evaluate_point(k, mid).unwrap().z - 10.0).abs() < 1e-6
        })
        .collect()
}

/// The chamfer of `edges` (each with its blend edge use) by the offsets `r0` (on each
/// edge use's face) and `r1`: one constant radius solver per edge given a second radius at
/// `SetFilletParameters`' defaults (approximation tolerance 4 times the edge's, 20 and 10
/// degrees), a linear cross section, every corner a bevel; the session is gone before the
/// caller's `StitchAndOrient`.
fn chamfer(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[(EdgeId, EdgeuseId)],
    (r0, r1): (f64, f64),
) -> solidcore::Result<()> {
    let tolerances = edges
        .iter()
        .map(|&(e, _)| edge::tolerance(b, e))
        .collect::<solidcore::Result<Vec<f64>>>()?;
    let mut session = FilletSession::new(k, b)?;
    let generator = session.add_surface_generator(FilletSurfaceGenerator::Linear)?;
    for (&(_, u0), tolerance) in edges.iter().zip(tolerances) {
        let solver = session.new_constant_radius_solver_with_second(
            k,
            u0,
            r0,
            r1,
            4.0 * tolerance,
            20.0 * PI / 180.0,
            10.0 * PI / 180.0,
        )?;
        session.load_solver(k, solver)?;
        session.set_solver_generator(solver, generator)?;
    }
    session.create_corners(k, None)?;
    for corner in session.fillet_corners()? {
        session.set_corner_bevel(corner, true)?;
    }
    session.do_filleting(k, None)
}

/// The face counts before and after and whether the result is a manifold solid.
type Outcome = (usize, usize, bool);

/// `StitchAndOrient` (its status ignored, as `SmApiSetBevelCorners` ignores it) and the
/// outcome.
fn finish(k: &mut Kernel, b: &mut Brep, before: usize) -> Outcome {
    let _ = stitch_and_orient(k, b, &StitchAndOrientOptions::default());
    let after = p1::faces(b, None).unwrap().len();
    (before, after, is_manifold_solid(b).unwrap())
}

/// Both rim halves (the whole smooth rim) chamfered in one session by `offsets`.
fn chamfer_whole(offsets: (f64, f64)) -> solidcore::Result<Outcome> {
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = p1::faces(&b, None).unwrap().len();
    let mut edges = Vec::new();
    for e in rims(&k, &b) {
        edges.push((e, edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap()));
    }
    chamfer(&mut k, &mut b, &edges, offsets)?;
    Ok(finish(&mut k, &mut b, before))
}

/// The whole tangent chain, both halves in one session, builds a closed solid with one
/// chamfer face per half (4 faces to 6), as the C++ cases
/// `issue-880-half-arc-rim-whole-offsets-*` compare.
#[test]
fn issue_880_whole_rim_chamfers() {
    for offsets in [
        (1.0, 1.0),
        (1.443_730_684_954_035_7, 1.443_730_684_954_035_7),
        (1.0, 2.0),
    ] {
        let outcome = chamfer_whole(offsets);
        assert!(
            matches!(outcome, Ok((4, 6, true))),
            "offsets {offsets:?}: {outcome:?}"
        );
    }
}

/// The issue's own pick: one rim half chamfered by the offsets of setbacks 1 on the cut face
/// and 2 down the wall builds a closed solid (4 faces to 7) that SMLib's check passes.
#[test]
fn issue_880_one_half_chamfers() {
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = p1::faces(&b, None).unwrap().len();
    let e = rims(&k, &b)[0];
    let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
    chamfer(&mut k, &mut b, &[(e, u)], (1.0, 2.0)).unwrap();
    assert_eq!(finish(&mut k, &mut b, before), (4, 7, true));
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// The volume of `b`, by the mass integrator.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The first rim half chamfered by the equal offsets the setbacks 1 and 1 give
/// (1.4437..., the goldens' `issue-880-half-arc-rim-0-setbacks-1-1`) builds a closed solid
/// (4 faces to 7) that the check passes, as the setbacks 1 and 2 do. It was refused with
/// 1001: at the half's End on the seam line through (6.78, 4.09, 10), the rail on the wall
/// crossed the End's plane off the wall by more than its zone and less than the rail's
/// tolerance, so the rail end read as off its face (#15906). The End is capped there; at
/// the other seam vertex the chamfer runs on past the vertex and leaves the part through the
/// rim (the global merge cuts it). The material removed lies between two wedges of the same
/// chamfer (constant offsets d from the cut face and the wall), integrated independently in
/// the planes square to the chamfer's spine (converged to 2e-6): the half's wedge between the
/// End planes at both seam vertices (11.609075), which the chamfer contains, and the wedge
/// from the capped End to the plane square to the rim where the run-on leaves the part
/// (19.620616), which contains it, its run-on rising from the chamfer to the rim.
#[test]
fn issue_880_one_half_chamfers_at_equal_setbacks() {
    let d = 1.443_730_684_954_035_7;
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = volume(&k, &mut b);
    let faces = p1::faces(&b, None).unwrap().len();
    let e = rims(&k, &b)[0];
    let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
    chamfer(&mut k, &mut b, &[(e, u)], (d, d)).unwrap();
    assert_eq!(finish(&mut k, &mut b, faces), (4, 7, true));
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let removed = before - volume(&k, &mut b);
    assert!(
        removed > 11.609_075 && removed < 19.620_616,
        "removed {removed}"
    );
}
