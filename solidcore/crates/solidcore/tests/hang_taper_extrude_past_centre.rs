//! `create_taper_extrude` of a regular polygon whose inward offset passes the profile's
//! centre (panic fuzz of #2490, case `body_queries:15857720670000042863`; #2756): a
//! 64-gon of radius 6.29 extruded to 9.19 at 72 degrees, so the offset,
//! `9.19 * tan 72° = 28.3`, is four times the radius and the trimmed offset is empty.
//! The taper then takes its surface intersection method: the untrimmed walls are merged
//! piecewise, which splits them into a number of faces that grows as the square of the
//! sides (464 for 16 sides), and the faces that do not reach their generating curve's
//! ends are deleted one by one. Each deletion walks the shells of the whole merged
//! B-rep, and the loop never checked the kernel's cancel token: cancelled at 240 s, the
//! 64-gon went on deleting for another 100 s. The loop now checks the token before each
//! face (module deviations of `primitive::primitive_creation::p2::taper`); results are
//! unchanged.
//!
//! The cancel is raised from the kernel's notify callback at the first face the loop
//! removes, so it lands in the loop on every run. The deletions after it are counted:
//! with the check none follows; without it the loop removes all the rest and the call
//! is only stopped by the Boolean union after it. Each case runs on its own thread, and
//! fails at the time budget instead of hanging the suite.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::context::NotifyCallback;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::primitive::api_curves as curves;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::face::p2::TYPE as FACE;
use solidcore::{CancelToken, Kernel, Point3};

/// The fuzz case's height and radius (angle 72 degrees, no caps, linear corners).
const HEIGHT: f64 = 9.188_299_504_644_123;
const RADIUS: f64 = 6.290_578_747_600_081;

/// Runs `f` on its own thread and fails the test past `budget` (or with `f`'s panic).
fn within_budget(budget: std::time::Duration, f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    let case = std::thread::spawn(move || {
        f();
        tx.send(()).unwrap();
    });
    match rx.recv_timeout(budget) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("the case did not return within the budget (hang)")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(case.join().unwrap_err())
        }
    }
}

/// Counts the faces removed from a B-rep, and cancels `token` at the first.
#[derive(Debug)]
struct CancelAtFirstFaceRemoval {
    token: CancelToken,
    removed: Arc<AtomicUsize>,
}

impl NotifyCallback for CancelAtFirstFaceRemoval {
    fn execute(
        &mut self,
        operation: NotifyOperation,
        _: ObjectRef,
        removed: Option<ObjectRef>,
        _: Option<ObjectRef>,
        _: Option<ObjectRef>,
    ) -> solidcore::Result<()> {
        if operation == NotifyOperation::RmFromBrep && removed.is_some_and(|o| o.type_code == FACE)
        {
            self.removed.fetch_add(1, Ordering::SeqCst);
            self.token.cancel();
        }
        Ok(())
    }
}

/// The taper of the `sides`-gon, cancelled at the first face it removes: the result and
/// the number of faces removed.
fn cancelled_taper(sides: u64) -> (solidcore::Result<usize>, usize) {
    let mut k = Kernel::default();
    let token = CancelToken::new();
    k.set_cancel_token(token.clone());
    let removed = Arc::new(AtomicUsize::new(0));
    k.context
        .set_user_notify_callback(Some(Box::new(CancelAtFirstFaceRemoval {
            token,
            removed: Arc::clone(&removed),
        })));
    let polygon: Vec<Curve> =
        curves::create_regular_polygon(Point3::new(0.0, 0.0, 0.0), sides, RADIUS)
            .unwrap()
            .into_iter()
            .map(Curve::from)
            .collect();
    let refs: Vec<Option<&Curve>> = polygon.iter().map(Some).collect();
    let r = sweeps::create_taper_extrude(&mut k, &refs, HEIGHT, 72.0, 0, false)
        .map(|b| b.faces().len());
    (r, removed.load(Ordering::SeqCst))
}

fn assert_stops_at_the_first_removal(sides: u64) {
    let (r, removed) = cancelled_taper(sides);
    assert!(
        matches!(
            &r,
            Err(SolidError::Cancelled { operation })
                if *operation == "primitive::primitive_creation::p2::taper::surface_intersection_method"
        ),
        "{r:?}"
    );
    assert_eq!(removed, 1, "faces removed after the cancel");
}

/// The fuzz case's taper with 16 sides (0.8 s in release): 464 faces are removed
/// without the check, one with it.
#[test]
fn taper_extrude_past_the_centre_stops_in_its_face_deletions() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_stops_at_the_first_removal(16);
    });
}

/// The fuzz case itself, the 64-gon: about 100 s in release to reach the deletions,
/// which then take over 250 s without the check.
#[test]
#[ignore = "slow: about 100 s in release before the cancel, far longer in a dev build"]
fn fuzz_case_64_gon_stops_in_its_face_deletions() {
    within_budget(std::time::Duration::from_secs(1200), || {
        assert_stops_at_the_first_removal(64);
    });
}

/// Uncancelled, the taper builds as before the check, and its result is right: the
/// walls meet before the height, so the open taper is the pyramid over the polygon, one
/// wall per side, its apex on the axis where the walls' inward offset reaches the
/// inradius, at `height * r cos(pi / n) / (height * tan 72°)`.
#[test]
fn taper_extrude_past_the_centre_still_builds_its_pyramid() {
    within_budget(std::time::Duration::from_secs(300), || {
        let sides = 8;
        let mut k = Kernel::default();
        let polygon: Vec<Curve> =
            curves::create_regular_polygon(Point3::new(0.0, 0.0, 0.0), sides, RADIUS)
                .unwrap()
                .into_iter()
                .map(Curve::from)
                .collect();
        let refs: Vec<Option<&Curve>> = polygon.iter().map(Some).collect();
        let b = sweeps::create_taper_extrude(&mut k, &refs, HEIGHT, 72.0, 0, false).unwrap();
        let n = usize::try_from(sides).unwrap();
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            (n, 2 * n, n + 1)
        );
        let inradius = RADIUS * (std::f64::consts::PI / sides as f64).cos();
        let apex_z = HEIGHT * inradius / (HEIGHT * 72.0_f64.to_radians().tan());
        let points: Vec<Point3> = b
            .vertices()
            .ids()
            .map(|v| b.vertex(v).unwrap().point)
            .collect();
        let apex = points
            .iter()
            .copied()
            .fold(points[0], |a, p| if p.z > a.z { p } else { a });
        assert!(
            apex.x.abs() < 1e-9 && apex.y.abs() < 1e-9 && (apex.z - apex_z).abs() < 1e-9 * apex_z,
            "apex {apex:?}, expected z {apex_z}"
        );
        // the other vertices are the polygon's corners
        for p in points.iter().filter(|p| **p != apex) {
            let r = p.x.hypot(p.y);
            assert!(
                p.z.abs() < 1e-12 && (r - RADIUS).abs() < 1e-9 * RADIUS,
                "{p:?}"
            );
        }
    });
}
