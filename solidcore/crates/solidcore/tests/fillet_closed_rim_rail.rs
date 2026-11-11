//! A closed rim fillet whose surface does not meet itself at the seam: SMLib's
//! `SmFilletEdge::CalcCrossSection` first makes the two ends match
//! (`sm_ExamineNFixFilletGeoms` copies the start row of control points onto the end row
//! of the fillet geometry's own surface) and then builds the corner's UV curves on that
//! fixed surface, so `sm_CreateUVCurve`'s closure check passes. The port built them on a
//! copy of the surface taken before the fix, so the check failed on the unfixed surface
//! and the executive recorded a corner error (code 3).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::edge;
use solidcore::Kernel;

/// The top rim of a cylinder of radius 10 and height 20 chamfered (`xsect` 0) by 10,
/// continuity 1, thumb weight 1: the chamfer surface collapses onto the axis and its
/// rail, a single cubic, ends away from its start until the ends are matched. SMLib
/// then still fails the fillet (`SM_ERR`), but with no error recorded in the executive
/// (`ec=0`); the unfixed port failed earlier, in the corner edges, with code 3.
#[test]
fn top_rim_chamfer_passes_the_corner_edge_closure_check() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    let rim = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let p = c.evaluate_point(&k, ivl.min()).unwrap();
            (p.z - 20.0).abs() < 1e-6 && (p.x.hypot(p.y) - 10.0).abs() < 1e-6
        })
        .unwrap();
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[rim], 10.0, 0, 1, 1.0)
        .unwrap();
    assert!(session.do_fillet(&mut k).is_err());
    assert_eq!(session.error_info().code, 0);
}
