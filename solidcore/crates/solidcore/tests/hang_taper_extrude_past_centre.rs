//! `create_taper_extrude` of a regular polygon whose inward offset passes the profile's
//! centre (panic fuzz of #2490, case `body_queries:15857720670000042863`; #2756): a
//! 64-gon of radius 6.29 extruded to 9.19 at 72 degrees, so the offset,
//! `9.19 * tan 72° = 28.3`, is four times the radius and the trimmed offset is empty.
//! Its sides as lines now take the tangent corner rule, which builds the pyramid directly
//! (#17265); as B-spline segments, the cancel cases below, they take the offset method.
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

/// The `sides`-gon's sides as lines.
fn polygon(sides: u64) -> Vec<Curve> {
    curves::create_regular_polygon(Point3::new(0.0, 0.0, 0.0), sides, RADIUS)
        .unwrap()
        .into_iter()
        .map(Curve::from)
        .collect()
}

/// The same `sides`-gon's sides as B-spline segments: the tangent rule builds exact walls
/// only for lines and arcs, so its taper takes the offset method and from there the
/// surface intersection method, as the lines' did before the rule became the only path.
fn bspline_polygon(sides: u64) -> Vec<Curve> {
    curves::create_regular_polygon(Point3::new(0.0, 0.0, 0.0), sides, RADIUS)
        .unwrap()
        .into_iter()
        .map(|l| Curve::from(l.base_with_nurb().unwrap().into_owned()))
        .collect()
}

/// The taper of the `sides`-gon (its sides B-spline segments), cancelled at the first face
/// it removes: the result and the number of faces removed.
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
    let polygon = bspline_polygon(sides);
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

/// Uncancelled, the taper's result is right: the walls meet before the height, so the
/// open taper is the pyramid over the polygon, one wall per side, its apex on the axis
/// where the walls' inward offset reaches the inradius, at
/// `height * r cos(pi / n) / (height * tan 72°)`. The tangent rule builds it: every wall's
/// section shrinks to that apex at one height, where the section closes (#17265).
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

/// The apex height of the `sides`-gon's pyramid drafted 72 degrees: where the walls' inward
/// offset, `z tan 72°`, reaches the inradius `r cos(pi / n)`.
fn apex_z(sides: u64) -> f64 {
    RADIUS * (std::f64::consts::PI / sides as f64).cos() / 72.0_f64.to_radians().tan()
}

/// The `sides`-gon tapered to `height` at 72 degrees with linear (tangent) corners and the
/// end caps `caps`.
fn tangent_taper(sides: u64, height: f64, caps: i32) -> sweeps::TaperExtrude {
    taper_of(&polygon(sides), height, caps)
}

/// `polygon` tapered to `height` at 72 degrees with linear corners and the end caps `caps`.
fn taper_of(polygon: &[Curve], height: f64, caps: i32) -> sweeps::TaperExtrude {
    let mut k = Kernel::default();
    let refs: Vec<Option<&Curve>> = polygon.iter().map(Some).collect();
    sweeps::create_taper_extrude_with_corner(
        &mut k,
        &refs,
        height,
        72.0,
        caps,
        sweeps::OffsetCornerType::LinearExtension,
    )
    .unwrap()
}

/// The open taper is the pyramid: `n` walls, `2n` edges, `n + 1` vertices, its apex on the
/// axis at [`apex_z`] and the rest the polygon's corners (as
/// [`taper_extrude_past_the_centre_still_builds_its_pyramid`] checks the octagon).
fn assert_open_pyramid(sides: u64) {
    assert_open_pyramid_of(sides, &polygon(sides));
}

/// [`assert_open_pyramid`] of the `sides`-gon given as `curves`.
fn assert_open_pyramid_of(sides: u64, curves: &[Curve]) {
    let sweeps::TaperExtrude::Solid(b) = taper_of(curves, HEIGHT, 0) else {
        panic!("the {sides}-gon's pyramid is refused");
    };
    let n = usize::try_from(sides).unwrap();
    assert_eq!(
        (b.faces().len(), b.edges().len(), b.vertices().len()),
        (n, 2 * n, n + 1)
    );
    let want = apex_z(sides);
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
        apex.x.abs() < 1e-9 && apex.y.abs() < 1e-9 && (apex.z - want).abs() < 1e-9 * want,
        "apex {apex:?}, expected z {want}"
    );
    for p in points.iter().filter(|p| **p != apex) {
        let r = p.x.hypot(p.y);
        assert!(
            p.z.abs() < 1e-12 && (r - RADIUS).abs() < 1e-9 * RADIUS,
            "{p:?}"
        );
    }
}

/// The closed taper (capped at the polygon; the far end has no section left to cap) is the
/// solid pyramid: valid at Level 2, a manifold solid of `n + 1` faces, its volume the
/// pyramid's `A z / 3` with `A = n R^2 sin(2 pi / n) / 2`. The volume is integrated to a
/// relative accuracy of `1e-10`, and the apex height is within `1e-9` relative (checked
/// above), so the volume is within `2e-9` relative of the closed form.
fn assert_closed_pyramid(sides: u64) {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let sweeps::TaperExtrude::Solid(mut b) = tangent_taper(sides, HEIGHT, 1) else {
        panic!("the {sides}-gon's pyramid is refused");
    };
    let mut k = Kernel::default();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
    let n = usize::try_from(sides).unwrap();
    assert_eq!(b.faces().len(), n + 1);
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(&k, &mut b, 1e-10, &attributes).unwrap();
    let area =
        sides as f64 * RADIUS * RADIUS * (2.0 * std::f64::consts::PI / sides as f64).sin() / 2.0;
    let want = area * apex_z(sides) / 3.0;
    assert!(
        (v - want).abs() <= 2e-9 * want,
        "volume {v}, expected {want}"
    );
}

/// #17265: the tangent rule builds the octagon's pyramid (it refused it, "draft corner
/// refused", as `WallCloses`, closing one wall at a time until fewer than three were left).
#[test]
fn the_tangent_rule_closes_the_octagon_at_its_apex() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_open_pyramid(8);
        assert_closed_pyramid(8);
    });
}

/// #17265: and the cancel case's 16-gon, its sides lines.
#[test]
fn the_tangent_rule_closes_the_16_gon_at_its_apex() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_open_pyramid(16);
        assert_closed_pyramid(16);
    });
}

/// A pyramid whose apex lands on the far end would pinch the far end to a point: still
/// refused where the walls close, at a corner of the polygon.
#[test]
fn an_apex_on_the_far_end_is_still_refused() {
    within_budget(std::time::Duration::from_secs(300), || {
        let result = tangent_taper(8, apex_z(8), 3);
        let sweeps::TaperExtrude::Refused(refusal) = result else {
            panic!("built: {result:?}");
        };
        assert_eq!(refusal.reason, sweeps::TaperCornerRefusal::WallCloses);
        let r = refusal.corner.x.hypot(refusal.corner.y);
        assert!(
            (r - RADIUS).abs() < 1e-9 * RADIUS && refusal.corner.z.abs() < 1e-12,
            "{refusal:?}"
        );
    });
}

/// Uncancelled, the 16-gon of B-spline segments (the cancel case's input) still builds its
/// pyramid through the surface intersection method, as its lines did before the tangent
/// rule took them.
#[test]
fn the_bspline_16_gon_still_builds_its_pyramid() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_open_pyramid_of(16, &bspline_polygon(16));
    });
}
