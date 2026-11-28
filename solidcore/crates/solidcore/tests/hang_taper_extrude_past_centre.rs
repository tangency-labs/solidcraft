//! `create_taper_extrude` of a regular polygon whose inward offset passes the profile's
//! centre (panic fuzz of #2490, case `body_queries:15857720670000042863`; #2756): a
//! 64-gon of radius 6.29 extruded to 9.19 at 72 degrees, so the offset,
//! `9.19 * tan 72° = 28.3`, is four times the radius and the trimmed offset is empty.
//! Its sides as lines, or as B-spline segments (#16639), now take the tangent corner rule,
//! which builds the pyramid directly (#17265); bowed a little, the cancel cases below, they
//! take the offset method. The taper then takes its surface intersection method: the
//! untrimmed walls are merged piecewise, which splits them into a number of faces that
//! grows as the square of the
//! sides, and the faces that do not reach their generating curve's
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
use solidcore::math::det;
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_curves as curves;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::face::p2::TYPE as FACE;
use solidcore::{CancelToken, Kernel, Point3, Vec3};

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

/// The same `sides`-gon's sides as B-spline segments: straight, so the tangent rule takes
/// them as lines (#16639).
fn bspline_polygon(sides: u64) -> Vec<Curve> {
    curves::create_regular_polygon(Point3::new(0.0, 0.0, 0.0), sides, RADIUS)
        .unwrap()
        .into_iter()
        .map(|l| Curve::from(l.base_with_nurb().unwrap().into_owned()))
        .collect()
}

/// How far each side of [`bowed_polygon`] bows out at its middle: its radius of curvature,
/// about `side^2 / (8 BOW)` (750 for 16 sides, 48 for 64), stays over the offset (28.3), so
/// its offset is a curve.
const BOW: f64 = 0.001;

/// The `sides`-gon with each side a quadratic B-spline bowed out by [`BOW`] at its middle:
/// not a line or an arc, so the tangent rule leaves it to the offset method and from there
/// the surface intersection method, as the lines' did before the rule became the only path.
fn bowed_polygon(sides: u64) -> Vec<Curve> {
    let corner = |i: u64| {
        let a = 2.0 * std::f64::consts::PI * i as f64 / sides as f64;
        Vec3::new(RADIUS * det::cos(a), RADIUS * det::sin(a), 0.0)
    };
    (0..sides)
        .map(|i| {
            let (a, b) = (corner(i), corner(i + 1));
            let mid = (a + b) * 0.5;
            // a quadratic's middle is half its middle control point's offset away
            let control = [a, mid + mid.unitize().unwrap() * (2.0 * BOW), b];
            Curve::from(
                BSplineCurve::create_canonical(&CanonicalData {
                    dim: 3,
                    degree: 2,
                    control_points: &control,
                    form: BSplineCurveForm::default(),
                    knot_multiplicities: &[3, 3],
                    knots: &[0.0, 1.0],
                    knot_type: KnotType::default(),
                    weights: None,
                    trim: None,
                })
                .unwrap(),
            )
        })
        .collect()
}

/// The taper of the `sides`-gon (its sides bowed B-splines), cancelled at the first face it
/// removes: the result and the number of faces removed.
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
    let polygon = bowed_polygon(sides);
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

/// The fuzz case's taper with 16 sides: without the check every face to delete is
/// removed, with it one.
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
        let inradius = RADIUS * det::cos(std::f64::consts::PI / sides as f64);
        let apex_z = HEIGHT * inradius / (HEIGHT * det::tan(72.0_f64.to_radians()));
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
            let r = det::hypot(p.x, p.y);
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
    RADIUS * det::cos(std::f64::consts::PI / sides as f64) / det::tan(72.0_f64.to_radians())
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
        let r = det::hypot(p.x, p.y);
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
        sides as f64 * RADIUS * RADIUS * det::sin(2.0 * std::f64::consts::PI / sides as f64) / 2.0;
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
        let r = det::hypot(refusal.corner.x, refusal.corner.y);
        assert!(
            (r - RADIUS).abs() < 1e-9 * RADIUS && refusal.corner.z.abs() < 1e-12,
            "{refusal:?}"
        );
    });
}

/// The 16-gon of B-spline segments is its lines' pyramid: straight B-splines take the
/// tangent rule as lines (#16639).
#[test]
fn the_bspline_16_gon_still_builds_its_pyramid() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_open_pyramid_of(16, &bspline_polygon(16));
    });
}

// #22149: a circle drafted inward past its centre. A circle has no corner, so the walls
// above never see it; the taper's offset method moved it past its centre and built no
// closed solid ("SMLib's result is not a closed solid" in SolidCraft's drill-point cut).

/// The drill point's circle radius and draft (degrees, inward).
const DRILL_R: f64 = 1.2;
const DRILL_DEG: f64 = 60.0;

/// Where the drill point's wall reaches the circle's centre: its inward offset at height
/// `z`, `z tan 60°`, is the radius at `z = r / tan 60°` (0.6928 mm).
fn drill_apex() -> f64 {
    DRILL_R / det::tan(DRILL_DEG.to_radians())
}

/// The circle of radius [`DRILL_R`] about `(0, 0, z0)` in a plane parallel to XY, one full
/// turn, counter-clockwise about +Z.
fn drill_circle(k: &Kernel, z0: f64) -> Curve {
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::geom::axis2_placement::Axis2Placement;
    let frame = Axis2Placement::new(Point3::new(0.0, 0.0, z0), Vec3::X, Vec3::Y);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, DRILL_R, None, None).unwrap())
}

/// The circle about `(0, 0, z0)` drafted [`DRILL_DEG`] inward to `height` (negative: down,
/// as SolidCraft's cut into a top face passes it) under `corner`, capped at both ends.
fn drill(height: f64, z0: f64, corner: sweeps::OffsetCornerType) -> sweeps::TaperExtrude {
    let mut k = Kernel::default();
    let circle = drill_circle(&k, z0);
    sweeps::create_taper_extrude_with_corner(&mut k, &[Some(&circle)], height, DRILL_DEG, 3, corner)
        .unwrap()
}

/// The drafted circle is the cone from it to the apex at [`drill_apex`] past `z0` the way
/// `height` points, however far past the apex `height` reaches: valid at Level 2, a manifold
/// solid of two faces (the cone and the base disc; nothing is left of the far end to cap),
/// the edges and vertices of the kernel's own cone of base radius 1.2 and top radius 0, its
/// apex on the axis and every other vertex on the circle, its volume `pi r^2 z / 3` with
/// `z` the apex height (integrated to `1e-10` relative, the apex within `1e-9` relative:
/// within `2e-9` relative).
fn assert_drill_point(height: f64, z0: f64, corner: sweeps::OffsetCornerType) {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use solidcore::primitive::api_primitives::create_cone;
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let result = drill(height, z0, corner);
    let sweeps::TaperExtrude::Solid(mut b) = result else {
        panic!("the drill point to {height} is refused: {result:?}");
    };
    let mut k = Kernel::default();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
    let want_z = drill_apex();
    let cone = create_cone(&mut k, Point3::ZERO, DRILL_R, 0.0, want_z).unwrap();
    assert_eq!(
        cone.faces().len(),
        2,
        "the kernel's cone: its side and its base"
    );
    assert_eq!(
        (
            b.faces().len(),
            b.edges().len(),
            b.vertices().len(),
            b.shells().len()
        ),
        (
            2,
            cone.edges().len(),
            cone.vertices().len(),
            cone.shells().len()
        )
    );
    let points: Vec<Point3> = b
        .vertices()
        .ids()
        .map(|v| b.vertex(v).unwrap().point)
        .collect();
    let along = height.signum();
    let apex = points.iter().copied().fold(points[0], |a, p| {
        if (p.z - z0) * along > (a.z - z0) * along {
            p
        } else {
            a
        }
    });
    let apex_z = (apex.z - z0) * along;
    assert!(
        apex.x.abs() < 1e-9 && apex.y.abs() < 1e-9 && (apex_z - want_z).abs() < 1e-9 * want_z,
        "apex {apex:?}, expected {want_z} past z0 {z0}"
    );
    for p in points.iter().filter(|p| **p != apex) {
        let r = det::hypot(p.x, p.y);
        assert!(
            (p.z - z0).abs() < 1e-12 && (r - DRILL_R).abs() < 1e-9 * DRILL_R,
            "{p:?}"
        );
    }
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(&k, &mut b, 1e-10, &attributes).unwrap();
    let want = std::f64::consts::PI * DRILL_R * DRILL_R * want_z / 3.0;
    assert!(
        (v - want).abs() <= 2e-9 * want,
        "volume {v}, expected {want}"
    );
}

/// #22149's blind cut: 3 mm down from the block's top face at z 5, the apex 0.6928 mm in;
/// under every corner rule (a circle has no corner, so they agree).
#[test]
fn a_circle_drafted_past_its_centre_ends_at_its_apex() {
    within_budget(std::time::Duration::from_secs(300), || {
        for corner in [
            sweeps::OffsetCornerType::CurveExtension,
            sweeps::OffsetCornerType::LinearExtension,
            sweeps::OffsetCornerType::FilletCorner,
        ] {
            assert_drill_point(-3.0, 5.0, corner);
        }
        // and up from z 0
        assert_drill_point(3.0, 0.0, sweeps::OffsetCornerType::CurveExtension);
    });
}

/// #22149's cut through all: the same cone, however far the height reaches.
#[test]
fn a_circle_drafted_far_past_its_centre_is_the_same_cone() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_drill_point(-100.0, 5.0, sweeps::OffsetCornerType::CurveExtension);
    });
}

/// The height exactly to the apex: the far end is the apex, the same cone.
#[test]
fn a_circle_drafted_exactly_to_its_centre_is_its_cone() {
    within_budget(std::time::Duration::from_secs(300), || {
        assert_drill_point(-drill_apex(), 5.0, sweeps::OffsetCornerType::CurveExtension);
    });
}

/// An open arc of the drill point's circle, a quarter turn, drafted the same past its centre
/// and uncapped, is no circle: it is not built as the whole cone, whatever the draft makes of
/// it (no vertex at the cone's apex).
#[test]
fn an_open_arc_drafted_past_its_centre_is_not_the_whole_cone() {
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::extent1d::Extent1d;
    within_budget(std::time::Duration::from_secs(300), || {
        let mut k = Kernel::default();
        let frame = Axis2Placement::new(Point3::new(0.0, 0.0, 5.0), Vec3::X, Vec3::Y);
        let quarter = Extent1d::from_min_max(0.0, 90.0);
        let arc = Curve::from(
            Ellipse::create_canonical_circle(&k, &frame, DRILL_R, Some(&quarter), None).unwrap(),
        );
        let result = sweeps::create_taper_extrude_with_corner(
            &mut k,
            &[Some(&arc)],
            -3.0,
            DRILL_DEG,
            0,
            sweeps::OffsetCornerType::CurveExtension,
        );
        if let Ok(sweeps::TaperExtrude::Solid(b)) = result {
            let apex = Point3::new(0.0, 0.0, 5.0 - drill_apex());
            for v in b.vertices().ids() {
                let p = b.vertex(v).unwrap().point;
                assert!(p.distance(apex) > 1e-6, "a vertex at the apex: {p:?}");
            }
        }
    });
}
