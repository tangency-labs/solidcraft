//! #2048: `check_brep` of a body with a face on an `OffsetSurface` returned
//! `Unported { upstream: "SmFace handle of a surface's face", .. }`.
//! `SmOffsetSurface::AssertValid` test 2 runs `GlobalSurfaceIntersect` of the surface
//! with itself, and upstream's `smsurf_GetSurfaceCache` reaches the surface's face
//! through the surface (`GetFace()`); the port's face check now hands the face-aware
//! cache context to that solve.
//!
//! The input: a quarter cylinder (radius 10, length 20) as a rational B-spline face,
//! offset by 1 to a quarter cylinder of radius 11 on one `OffsetSurface` face, of area
//! `11 * pi / 2 * 20`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::nurbs::b_spline_surface::p1::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, Kernel};

/// The issue's quarter cylinder: u degree 2 (rational quarter circle), v degree 1.
fn quarter_cylinder(k: &mut Kernel) -> Brep {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let pw: Vec<Vec<CPoint>> = [(10.0, 0.0, 1.0), (10.0, 10.0, c), (0.0, 10.0, 1.0)]
        .iter()
        .map(|&(x, y, w)| {
            vec![
                CPoint::new(x * w, y * w, 0.0, w),
                CPoint::new(x * w, y * w, 20.0 * w, w),
            ]
        })
        .collect();
    let s = Surface::from(
        BSplineSurface::from_data(
            2,
            1,
            2,
            1,
            5,
            3,
            &pw,
            &[0., 0., 0., 1., 1., 1.],
            &[0., 0., 1., 1.],
        )
        .unwrap(),
    );
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    {
        let this_face = |f: FaceId| -> ObjectRef { notify_ref(EntityRef::Face(f)) };
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        create_face_from_surface(k, &mut b, Some(s), &d, &mut fenv).unwrap();
    }
    b.refresh_cached_topology();
    b
}

#[test]
fn check_brep_answers_on_an_offset_surface_face() {
    let mut k = Kernel::default();
    let mut b = quarter_cylinder(&mut k);
    assert_eq!(
        check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .assert_valid,
        Some(true)
    );

    let mut r = offset_brep_simple(
        &mut k,
        &mut b,
        OffsetOptions {
            distance: 1.0,
            extend_convex_edges: true,
            merge_results: true,
        },
    )
    .unwrap();
    r.refresh_cached_topology();

    let faces: Vec<FaceId> = r.faces().ids().collect();
    assert_eq!(faces.len(), 1);
    let surface = r.face(faces[0]).unwrap().surface.as_ref().unwrap();
    assert!(matches!(surface.kind(), SurfaceKind::OffsetSurface(_)));
    let area = brep_area(&k, &mut r, 1e-6).unwrap();
    let expected = 11.0 * std::f64::consts::FRAC_PI_2 * 20.0;
    assert!((area - expected).abs() < 1e-3, "area {area} vs {expected}");

    let c = check_brep(&mut k, &mut r, CheckLevel::Level2).expect("check_brep answers");
    assert_eq!(c.assert_valid, Some(true), "{:?}", c.reports);
    assert_eq!(c.failures().count(), 0);
}
