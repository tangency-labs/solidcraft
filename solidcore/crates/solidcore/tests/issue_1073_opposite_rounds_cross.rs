//! tangency-labs/solidcore#1073 (WR-460): rounds on the four vertical edges of a
//! 14 x 6 x 10 post, each 6 mm face carrying two of them. Past r = 3 the two rails on a
//! face have crossed and SMLib builds an open body; the default kernel refuses that result,
//! as it does any fillet of a closed manifold solid whose result is none (WR-212), and puts
//! the post back. With SolidCraft's fix list the band
//! test (`fixes.fillet_refuse_overlapping_bands`, WR-542) refuses it before the post is
//! edited, naming the largest radius that builds. (The crossing-rails refusal
//! `fillet_refuse_crossing_rails`, which refused it with a bare 1001 after the fillet's
//! B-rep was built, is retired: the band test is the one test of two bands on a face.)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::error::blend::BlendFailure;
use solidcore::error::ErrorKind;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel, Result, SolidError};

/// SolidCraft's fix list (`tests/kernel_cases/fixes.json`), or the pinned SMLib's
/// (`Kernel::default`).
fn kernel(fixed: bool) -> Kernel {
    if !fixed {
        return Kernel::default();
    }
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in &names {
        if let Some(flag) = f.flag_mut(name) {
            *flag = true;
        }
    }
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

/// Fillet the post's vertical edges with `r`; the post and whether it is a solid after.
fn round(fixed: bool, r: f64) -> (Brep, Result<()>, bool) {
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
    let result = fillet_edges(&mut k, &mut b, &vertical, r, 1, 1, 1.0);
    if result.is_err() {
        assert_eq!(b.face_list().len(), faces_before, "post left edited");
    }
    let solid = result.is_ok() && is_manifold_solid(&b).unwrap();
    (b, result, solid)
}

/// The default kernel (no band test) builds the r 3.5 rounds whose rails cross on each 6 mm
/// face, which leaves no closed manifold solid: the result is refused as a failed blend
/// naming no entity, and the post comes back a closed manifold solid with its six faces.
#[test]
fn issue_1073_the_default_kernels_open_body_is_refused() {
    let (b, result, _) = round(false, 3.5);
    let Err(error) = result else {
        let solid = is_manifold_solid(&b);
        panic!(
            "built: {} faces, manifold solid {solid:?}",
            b.face_list().len()
        );
    };
    assert_eq!(error.kind(), ErrorKind::Failed, "{error:?}");
    assert!(
        matches!(error.blend_failure(), Some(BlendFailure::Failed(_))),
        "{error:?}"
    );
    assert_eq!(error.entity(), None, "{error:?}");
    assert!(is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 6);
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
