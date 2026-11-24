//! tangency-labs/solidcore#1073 (WR-460): rounds on the four vertical edges of a
//! 14 x 6 x 10 post, each 6 mm face carrying two of them. Past r = 3 the two rails on a
//! face have crossed and SMLib builds an open body; the default kernel refuses that result,
//! as it does any fillet of a closed manifold solid whose result is none (WR-212), and puts
//! the post back. The band test (WR-542, no longer a flag) refuses it before the post is
//! edited, naming the largest radius that builds. (The crossing-rails refusal
//! `fillet_refuse_crossing_rails`, which refused it with a bare 1001 after the fillet's
//! B-rep was built, is retired: the band test is the one test of two bands on a face.)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

#[path = "support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

use solidcore::blend::fillet_executive::api::{
    fillet_edges, fillet_edges_per_edge, variable_radius_fillet,
};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel, Result, SolidError};

/// SolidCraft's fix list (`support/solidcraft_fixes.rs`), or the pinned SMLib's
/// (`Kernel::default`).
fn kernel(fixed: bool) -> Kernel {
    if !fixed {
        return Kernel::default();
    }
    let f = solidcraft_fixes::solidcraft_fixes();
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

/// Fillet the post's vertical edges with `r`; the post and whether it is a solid after.
fn round(fixed: bool, r: f64) -> (Brep, Result<()>, bool) {
    round_with(fixed, |k, b, vertical| {
        fillet_edges(k, b, vertical, r, 1, 1, 1.0)
    })
}

/// Round the post's vertical edges with `fillet`; the post, the result and whether the post
/// is a solid after.
fn round_with(
    fixed: bool,
    fillet: impl FnOnce(&mut Kernel, &mut Brep, &[EdgeId]) -> Result<()>,
) -> (Brep, Result<()>, bool) {
    let mut k = kernel(fixed);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 14.0, 6.0, 10.0).unwrap();
    let vertical: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            let (p, q) = (
                vertex::point(&b, s.unwrap()).unwrap(),
                vertex::point(&b, t.unwrap()).unwrap(),
            );
            (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9
        })
        .collect();
    assert_eq!(vertical.len(), 4);
    let faces_before = b.face_list().len();
    let result = fillet(&mut k, &mut b, &vertical);
    if result.is_err() {
        assert_eq!(b.face_list().len(), faces_before, "post left edited");
    }
    let solid = result.is_ok() && is_manifold_solid(&b).unwrap();
    (b, result, solid)
}

/// A round of the post's vertical edges by one of the public wrappers.
type Round = Box<dyn FnOnce(&mut Kernel, &mut Brep, &[EdgeId]) -> Result<()>>;

/// The default kernel refuses the r 3.5 rounds before it edits the post, for each wrapper
/// that rounds a solid: one radius, a radius per edge, and a radius law. Each round's band
/// on a 6 mm end face runs from its edge to its rail, r in from the edge, so past r = 3 (half
/// the face) the two bands on the face overlap: the bands' check (WR-542, no longer a flag)
/// refuses them as no room, naming 3 for a constant radius, and the post comes back a closed manifold solid with
/// its six faces. (Before the check ran by default, the kernel built the rounds, whose rails
/// cross on each end face, and refused the open body it left as a defect, status 1001.)
#[test]
fn issue_1073_the_default_kernels_overlapping_bands_are_refused() {
    let circular = [FilletSurfaceGeneratorType::Circular; 4];
    let wrappers: [(&str, Round); 3] = [
        (
            "fillet_edges",
            Box::new(|k, b, v| fillet_edges(k, b, v, 3.5, 1, 1, 1.0)),
        ),
        (
            "fillet_edges_per_edge",
            Box::new(move |k, b, v| fillet_edges_per_edge(k, b, v, &[3.5; 4], &circular, 1, 1.0)),
        ),
        (
            "variable_radius_fillet",
            Box::new(|k, b, v| variable_radius_fillet(k, b, v, 3.5, 3.5, 1, 1)),
        ),
    ];
    for (name, fillet) in wrappers {
        let (b, result, _) = round_with(false, fillet);
        match result {
            Err(SolidError::FilletRefused {
                reason: Some(NoRoomReason::BandsOverlap),
                fit,
                ..
            }) if name != "variable_radius_fillet" => assert!(
                (fit.size - 3.0).abs() <= 1e-9 * 3.0,
                "{name}: fit {}",
                fit.size
            ),
            // the executive's fit size is for constant radii, not radius laws (this law
            // is constant, but the wrapper still runs a law): refused with the
            // executive's status as a stage failure, not main's defect (the session's error
            // info notes the overlap, below)
            Err(error) if name == "variable_radius_fillet" => {
                assert_eq!(
                    solidcore::error::upstream_status(&error),
                    Some(1001),
                    "{name}: {error:?}"
                );
                assert!(!error.is_defect(), "{name}: {error:?}");
            }
            other => panic!("{name}: expected BandsOverlap, got {other:?}"),
        }
        assert!(is_manifold_solid(&b).unwrap(), "{name}");
        assert_eq!(b.face_list().len(), 6, "{name}");
    }
}

/// The radius law's refusal above is the bands' check: the session notes `BandsOverlap`.
#[test]
fn issue_1073_the_default_kernels_radius_law_notes_the_overlap() {
    let (_, result, _) = round_with(false, |k, b, vertical| {
        let mut session = FilletSession::new(k, b).unwrap();
        session
            .prepare_variable_radius_edges(
                k,
                vertical,
                3.5,
                FilletLaw::linear(3.5, 3.5, None),
                1,
                1,
            )
            .unwrap();
        let result = session.do_fillet(k);
        assert_eq!(
            session.error_info().no_room_reason(),
            Some(NoRoomReason::BandsOverlap),
            "{result:?}"
        );
        result
    });
    assert!(result.is_err());
}

#[test]
fn issue_1073_rounds_that_fit_are_kept() {
    for r in [2.9, 3.0] {
        let (_, result, solid) = round(true, r);
        result.unwrap();
        assert!(solid, "r = {r}");
    }
}

/// Past r = 3 the two bands on each 6 mm face overlap: refused for that (the band test,
/// right after the trace), naming 3, where the bands meet at the face's middle; the
/// named radius builds a solid.
#[test]
fn issue_1073_rounds_past_the_face_are_refused() {
    for r in [3.03, 3.0625, 3.5, 4.0] {
        let (_, result, _) = round(true, r);
        let fit = match result {
            Err(SolidError::FilletRefused {
                reason: Some(NoRoomReason::BandsOverlap),
                fit,
                ..
            }) => fit,
            other => panic!("r = {r}: expected BandsOverlap, got {other:?}"),
        };
        assert_eq!(fit.kind, FitKind::Radius, "r = {r}");
        assert!(
            (fit.size - 3.0).abs() <= 1e-9 * 3.0,
            "r = {r}: fit {}",
            fit.size
        );
        let (_, built, solid) = round(true, fit.size);
        built.unwrap();
        assert!(solid, "r = {r}: the fit {} is not a solid", fit.size);
    }
}

#[test]
fn issue_1073_all_edges_that_fit_are_kept() {
    let mut k = kernel(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 14.0, 6.0, 10.0).unwrap();
    let all: Vec<_> = b.edges().ids().collect();
    fillet_edges(&mut k, &mut b, &all, 2.5, 1, 1, 1.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
}
