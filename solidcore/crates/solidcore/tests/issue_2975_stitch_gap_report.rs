//! tangency-labs/solidcore#2975: the stitch reports, for every gap it closes, a
//! certified bound on the gap and the tolerance growth its closure applied; an exact
//! match's closure applies no growth.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::math::{Point2, Vec3};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p5::{stitch_and_orient_reported, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, Kernel};

/// Adds one square planar face on the plane `z = 0` over the 1 x 1 domain whose low
/// corner is `x` on x and `y` on y.
fn add_square(k: &mut Kernel, b: &mut Brep, x: f64, y: f64) {
    let surface = Surface::from(Plane::from_normal(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ));
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let domain = Extent2d::from_points(Point2::new(x, y), Point2::new(x + 1.0, y + 1.0));
    create_face_from_surface(k, b, Some(surface), &domain, &mut fenv).unwrap();
}

#[test]
fn a_known_gap_reports_its_bound_and_the_growth_its_closure_applied() {
    let gap = 1e-5;
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_square(&mut k, &mut b, 0.0, 0.0);
    add_square(&mut k, &mut b, 1.0 + gap, 0.0);

    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("the stitch runs");
    // The two corners glue: each reports the gap it held (the known offset the two
    // squares put between them) and the growth the glue applied — the survivor's
    // tolerance rose by the same amount, the glue's rule keeping the closed distance
    // inside the surviving tolerance (solidcore#2975).
    let corners: Vec<_> = gaps
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                solidcore::heal::stitch::p2::ClosedGapKind::Vertex { .. }
            )
        })
        .collect();
    assert_eq!(corners.len(), 2, "{gaps:?}");
    for entry in corners {
        assert!(
            (entry.gap - gap).abs() <= 1e-9 * gap.max(1.0),
            "reported {entry:?} against the known {gap:.1e}"
        );
        assert!(
            (entry.growth - gap).abs() <= 1e-9 * gap.max(1.0),
            "growth {entry:?} against the known {gap:.1e}"
        );
    }
    // The edge between them glues with no gap of its own.
    assert!(
        gaps.iter().any(|entry| matches!(
            entry.kind,
            solidcore::heal::stitch::p2::ClosedGapKind::Edge { .. }
        )),
        "{gaps:?}"
    );
}

#[test]
fn an_exact_match_reports_no_growth() {
    let mut k = Kernel::default();
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    add_square(&mut k, &mut b, 0.0, 0.0);
    add_square(&mut k, &mut b, 0.0, 0.0);

    let gaps = stitch_and_orient_reported(&mut k, &mut b, &StitchAndOrientOptions::default())
        .expect("the stitch runs");
    for gap in gaps {
        assert!(gap.gap.is_finite(), "gap {gap:?}");
        assert!(
            gap.growth == 0.0,
            "the exact match's closure applied no growth: {gap:?}"
        );
    }
}
