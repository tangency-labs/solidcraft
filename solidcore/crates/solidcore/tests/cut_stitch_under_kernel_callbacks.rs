//! `SmApiCut` ends with `StitchAndOrient`. The port's `StitchAndOrient` edits through edits
//! of its own on the kernel context; a cut run under `with_kernel_callbacks` has lent that
//! context's callbacks to its own edit, so those edits were refused as nested, and the
//! refusal was ignored with `StitchAndOrient`'s status: the cut's pieces stayed
//! unstitched and unoriented. The cut now stitches and orients through its own
//! environment there.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::math::Point2;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::cutting::{cut, cut_notified};
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::{new_brep, with_kernel_callbacks};
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, EntityRef, Kernel, Point3, Vec3};

/// The six faces of the cube -5..5, each a separate face of the infinite region with
/// edges and vertices of its own (nothing stitched).
fn loose_cube(k: &mut Kernel) -> Brep {
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let h = 5.0;
    for a in [Vec3::X, Vec3::Y, Vec3::Z] {
        for s in [1.0, -1.0] {
            let n = a * s;
            let surface = Surface::from(Plane::from_normal(n * h, n));
            let mut state = ArenaNotifyState::new(&b);
            let mut env = state.env(None);
            let mut marks = Marks::default();
            let this_face = |face| arena_identity(CacheObject::Face(face));
            let mut fenv = FaceBuildEnv {
                env: &mut env,
                marks: &mut marks,
                this_face: &this_face,
            };
            let domain = Extent2d::from_points(Point2::new(-h, -h), Point2::new(h, h));
            create_face_from_surface(k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
        }
    }
    b
}

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    )
}

/// The loose cube cut at z = 1, keeping the part below, by the plain `cut` and by
/// `cut_notified` inside a kernel-callback edit: the same stitched pieces.
#[test]
fn cut_under_kernel_callbacks_stitches_as_the_plain_cut_does() {
    let (point, normal) = (Point3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
    let mut k = Kernel::default();

    let mut plain = loose_cube(&mut k);
    cut(&mut k, &mut plain, point, normal).unwrap();

    let mut nested = loose_cube(&mut k);
    with_kernel_callbacks(&mut k, &mut nested, |k, b, env| {
        let key_of = env.key_of;
        let curve_ref = move |e, _type_code: i64| key_of(EntityRef::Edge(e));
        cut_notified(k, b, env, &curve_ref, point, normal)
    })
    .unwrap();

    // the plain cut's stitch glued the loose faces' edges (each had four of its own)
    let (faces, edges, _) = counts(&plain);
    assert!(
        edges < 4 * faces,
        "the plain cut stitched nothing: {faces} faces, {edges} edges"
    );
    assert_eq!(counts(&nested), counts(&plain));
    let (a_plain, a_nested) = (
        brep_area(&k, &mut plain, 1e-9).unwrap(),
        brep_area(&k, &mut nested, 1e-9).unwrap(),
    );
    assert!(
        (a_nested - a_plain).abs() <= 1e-9 * a_plain,
        "area {a_nested} vs {a_plain}"
    );
}
