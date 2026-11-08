//! The fork's tangency-labs/smlib@701da4b, under WR-547's
//! `Kernel::fixes.corner_blend_coons_patch_tangent_to_sides`: `sm_CoonsSideAngle` makes the
//! Coons corner patch's normal unit by its own length, not by `Unitize`, whose absolute floor
//! (1e-12) refused the normal of a station near a collapsing side of a patch 0.01 across.
//!
//! The fork's case: the three edges at a corner of a box of 0.01 rounded together by a
//! variable-radius round failed (`SM_ERR_INVALID_INPUT`) at every radius, while the same
//! corner on a box of 10 built; with the fix both build bodies of 10 faces, the small one
//! removing the big one's volume scaled by 1e9 to SMLib's fitting tolerances (the fork:
//! 0.5%). Here the round's law is constant: a variable-radius corner is never analytic
//! (`MakeAnalyticCornerSurface` returns `FALSE` for an `SM_FS_VARIABLE_RADIUS` solver), so
//! it is the Coons patch at every size.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::{FilletLaw, FilletLawKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, Result};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edges of `b` with an end at `p`.
fn edges_at(k: &Kernel, b: &Brep, p: Point3, tol: f64) -> Vec<EdgeId> {
    b.edges()
        .ids()
        .filter(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            s.distance(p) < tol || t.distance(p) < tol
        })
        .collect()
}

/// A box of side `size` with the three edges at its origin corner rounded by the constant
/// variable-radius law `radius`, with the fix as given.
fn round_corner(k: &mut Kernel, fixed: bool, size: f64, radius: f64) -> Result<Brep> {
    k.fixes.corner_blend_coons_patch_tangent_to_sides = fixed;
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), size, size, size)?;
    let edges = edges_at(k, &b, Point3::new(0.0, 0.0, 0.0), 1e-3 * size);
    assert_eq!(edges.len(), 3);
    let law = FilletLaw::new(FilletLawKind::Linear {
        start_radius: radius,
        end_radius: radius,
    });
    {
        let mut session = FilletSession::new(k, &mut b)?;
        session.prepare_variable_radius_edges(k, &edges, radius, law, 1, 1)?;
        session.do_fillet(k)?;
    }
    Ok(b)
}

/// The box's volume less the rounded body's.
fn removed(fixed: bool, size: f64, radius: f64) -> Result<(usize, f64)> {
    let mut k = Kernel::default();
    let mut b = round_corner(&mut k, fixed, size, radius)?;
    Ok((
        b.faces().ids().count(),
        size * size * size - volume(&k, &mut b),
    ))
}

/// With the fix the corner of the 0.01 box builds as the box of 10's does: at r 0.001 the
/// body is the box of 10's at r 1 scaled by 1e-3, and removes its volume times 1e-9 (to
/// 1%, the fork's 0.5% being SMLib's fitting tolerances, which are lengths); at r 2e-4 it
/// builds too. Before 701da4b `Unitize` refused the patch's normal near its collapsed side
/// (|Du x Dv| down to 1e-14 there) and the round failed with `SM_ERR_INVALID_INPUT`.
#[test]
fn fork_701da4b_small_box_corner_builds_with_the_fix() {
    let (faces, big) = removed(true, 10.0, 1.0).unwrap();
    assert_eq!(faces, 10);
    let (faces, small) = removed(true, 0.01, 1e-3).unwrap();
    assert_eq!(faces, 10);
    let want = big * 1e-9;
    assert!(
        (small - want).abs() <= 0.01 * want,
        "removed {small:e}, want {want:e}"
    );
    let (faces, _) = removed(true, 0.01, 2e-4).unwrap();
    assert_eq!(faces, 10);
}
