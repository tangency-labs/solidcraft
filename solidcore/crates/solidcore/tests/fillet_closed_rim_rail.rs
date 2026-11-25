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
/// continuity 1, thumb weight 1. SMLib traces this chamfer as a B-spline whose rail, a single
/// cubic, ends away from its start until the ends are matched, and then fails the fillet
/// (`SM_ERR`, `ec=0`). A chamfer turned round a circular edge is the `SmCone` it is (WR-421,
/// the only path since #18913), so here no rail is traced: the chamfer is the cone from the
/// side at z 10 to its apex on the axis at z 20, and the body is the cylinder below z 10 and
/// that cone, 1000 pi + 1000 pi / 3, valid at Level 2.
#[test]
fn top_rim_chamfer_to_the_axis_is_its_cone() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use solidcore::math::PI;
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
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
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[rim], 10.0, 0, 1, 1.0)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
        assert_eq!(session.error_info().code, 0);
    }
    b.refresh_cached_topology();
    let cones = b
        .face_list()
        .iter()
        .filter(|&&f| {
            matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Cone(_)
            )
        })
        .count();
    // the side, an `SmCone` of half-angle 0, and the chamfer's cone; the top is gone
    assert_eq!(cones, 2, "the chamfer is the cone");
    assert_eq!(b.face_list().len(), 3, "the side, the bottom and the cone");
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    let want = 1000.0 * PI + 1000.0 * PI / 3.0;
    assert!((v - want).abs() <= 1e-9 * want, "volume {v}, want {want}");
}
