//! #2013: `BrepCheck` counted SMLib's warning as a failure, and `check_brep` checked a
//! body of unknown naming under identities no maker used.
//!
//! 1. Two disjoint sheet faces in one B-rep (an L whose sharp edge is pulled apart: a
//!    square on z = 0 and one on x = 1.5), each in its own shell. `SmBrep::AssertValid`
//!    reports test 21 ("Suspicious Shell Count", one shell per face) without failing
//!    (`SmBrep.cpp`: "this is a warning, so don't change the bRtn value"), so it returns
//!    `TRUE`; the capi leaves the report out of its lines. Expected: `is_valid()`,
//!    `failure_count() == 0`, the one warning in `warnings()`, the raw report list
//!    unchanged. Before the fix `failure_count()` was 1.
//! 2. A B-rep of `NamingScheme::Unknown` (`Brep::new_with_infinite_region` plus a face):
//!    `check_brep` returns `InvalidInput` instead of failing every edge's curve owner
//!    (`SmBrep::ValidatePointers` test 61) under arena identities.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::assert_array::labels;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector2d::Point2;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, check_brep_with_keys, CheckLevel};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId, NamingScheme};
use solidcore::{EntityRef, Kernel, SolidError, Vec3};

/// Adds the square `-1..1` of the plane through `origin` with normal `normal` to `b` as
/// a separate face of the infinite region, its surface owned by `this_face`.
fn add_square(
    k: &mut Kernel,
    b: &mut Brep,
    origin: Vec3,
    normal: Vec3,
    this_face: &dyn Fn(FaceId) -> ObjectRef,
) -> FaceId {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face,
    };
    let domain = Extent2d::from_points(Point2::new(-1.0, -1.0), Point2::new(1.0, 1.0));
    let plane = Surface::from(Plane::from_normal(origin, normal));
    create_face_from_surface(k, b, Some(plane), &domain, &mut fenv).unwrap()
}

/// The issue's sheet: `new_brep` (notification naming) with two disjoint squares.
fn two_sheets(k: &mut Kernel) -> Brep {
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    assert_eq!(b.naming(), NamingScheme::Notify);
    let this_face = |f: FaceId| notify_ref(EntityRef::Face(f));
    add_square(k, &mut b, Vec3::new(0.0, 0.0, 0.0), Vec3::Z, &this_face);
    add_square(k, &mut b, Vec3::new(1.5, 0.0, 1.5), Vec3::X, &this_face);
    b
}

#[test]
fn suspicious_shell_count_is_a_warning_not_a_failure() {
    let mut k = Kernel::default();
    let mut b = two_sheets(&mut k);
    assert_eq!(b.faces().ids().count(), 2);
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();

    // upstream's AssertValid passes, and the raw list keeps its one report
    assert_eq!(c.assert_valid, Some(true));
    assert!(c.pointer_reports.is_empty());
    let raw: Vec<_> = c.reports.reports().iter().filter(|r| !r.ok).collect();
    assert_eq!(raw.len(), 1);
    assert_eq!(
        (raw[0].reporting_type, raw[0].list_index, raw[0].test_index),
        (labels::BREP_TYPE, 0, 21)
    );
    assert_eq!(raw[0].name, "Suspicious Shell Count");

    // the summary leaves it out, and it stays reachable as a warning
    assert!(c.is_valid(), "{:?}", c.lines());
    assert_eq!(c.failure_count(), 0);
    assert!(c.lines().is_empty());
    assert_eq!(c.failures().count(), 0);
    let warnings: Vec<_> = c
        .warnings()
        .map(|r| (r.reporting_type, r.list_index, r.test_index))
        .collect();
    assert_eq!(warnings, [(labels::BREP_TYPE, 0, 21)]);
}

/// A `NamingScheme::Unknown` B-rep with one square, its surface owned by the arena
/// identity (as `uv_trim_owner_default_naming.rs` builds it).
fn unknown_naming_face(k: &mut Kernel) -> Brep {
    let mut b = Brep::new_with_infinite_region().unwrap();
    let this_face = |f: FaceId| arena_identity(CacheObject::Face(f));
    add_square(k, &mut b, Vec3::ZERO, Vec3::Z, &this_face);
    assert_eq!(b.naming(), NamingScheme::Unknown);
    b
}

#[test]
fn check_brep_refuses_an_unknown_naming() {
    let mut k = Kernel::default();
    let mut b = unknown_naming_face(&mut k);
    let r = check_brep(&mut k, &mut b, CheckLevel::Level2);
    assert!(
        matches!(r, Err(SolidError::InvalidInput { .. })),
        "{:?}",
        r.map(|c| c.lines())
    );

    // what check_brep ran before: the arena identities, which the builder did not name
    // the edges' curves by, so every edge failed `ValidatePointers` test 61
    let arena = |e: EntityRef| match e {
        EntityRef::Edge(x) => arena_identity(CacheObject::Edge(x)),
        EntityRef::Face(x) => arena_identity(CacheObject::Face(x)),
        EntityRef::Vertex(x) => arena_identity(CacheObject::Vertex(x)),
        EntityRef::Brep(_) => arena_identity(CacheObject::Brep),
        other => notify_ref(other),
    };
    let c = check_brep_with_keys(&mut k, &mut b, CheckLevel::Level2, &arena).unwrap();
    assert!(!c.is_valid());
    let edges = b.edges().ids().count();
    let tests: Vec<_> = c.failures().map(|r| (r.list_index, r.test_index)).collect();
    assert_eq!(tests, vec![(1, 61); edges], "{:?}", c.lines());

    // the builder's own identities still check it
    let keys = |e: EntityRef| match e {
        EntityRef::Face(f) => arena_identity(CacheObject::Face(f)),
        other => notify_ref(other),
    };
    let c = check_brep_with_keys(&mut k, &mut b, CheckLevel::Level2, &keys).unwrap();
    assert!(c.is_valid(), "{:?}", c.lines());

    // and once a scheme is recorded, check_brep runs
    b.use_arena_naming().unwrap();
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(c.is_valid(), "{:?}", c.lines());
}
