//! A UV trim curve made on a B-rep under the default naming scheme is owned by the edge
//! use holding it, as upstream `SetUVTrimCurve` (`pUVTrimCurve->SetOwner(eu)`) always
//! does (`plan/findings/topology.edgeuse_p1--uv-trim-owner-under-unknown-naming.md`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p3::create_uv_trim_curves;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, EdgeuseId, NamingScheme};

#[test]
fn new_uv_trim_curves_are_owned_by_their_holder_under_the_default_naming() {
    let mut k = Kernel::default();
    // `new SmBrep()`: no naming scheme recorded
    let mut b = Brep::new_with_infinite_region().unwrap();
    assert_eq!(b.naming(), NamingScheme::Unknown);

    let face = {
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let this_face = |f| arena_identity(CacheObject::Face(f));
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        let domain = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
        let plane = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z));
        create_face_from_surface(&mut k, &mut b, Some(plane), &domain, &mut fenv).unwrap()
    };

    // drop the face's UV trim curves and make them again: every new one goes through
    // `SetUVTrimCurve`
    let eus: Vec<EdgeuseId> = b.edgeuses().ids().collect();
    for &eu in &eus {
        eu1::delete_uv_trim_curve(&mut b, eu).unwrap();
    }
    assert!(b.edgeuses().iter().all(|(_, r)| r.uv_trim_curve.is_none()));
    create_uv_trim_curves(&k, &mut b, face, false, None).unwrap();

    let mut owned = 0;
    for (eu, r) in b.edgeuses().iter() {
        if let Some(c) = r.uv_trim_curve.as_ref() {
            let owner = c.owner().expect("a UV trim curve without owner");
            // the edge use's identity under the arena (and notify) scheme: its
            // notification identity `(SmEdgeuse_TYPE, 1 + (7 << 32) + index)`
            assert_eq!(owner.type_code, eu1::TYPE);
            assert_eq!(owner.key.0, 1 + (7u64 << 32) + u64::from(eu.index()));
            owned += 1;
        }
    }
    assert!(owned > 0, "the face got no UV trim curves");
}
