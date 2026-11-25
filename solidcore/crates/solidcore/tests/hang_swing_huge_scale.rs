//! A swung primitive scaled by `f64::MAX` (panic fuzz of #2490, case
//! `booleans:11053777690231679143`) has a surface with control points near `1e307`.
//! Merging it with itself (`piecewise_merge`, or `boolean` with `Merge`) ran past 600 s
//! in release, as did the C++ reference past 120 s: every coincidence check of two of
//! its boundary curves splits the curves (`GlobalCoincidenceChecker`), whose polar boxes
//! overflow to unbounded on every piece, until the call cap, and the merge makes many
//! such checks. The coincidence check now fails for a curve with a box coordinate past
//! `limits::MAX_COORDINATE_MAGNITUDE` (`1e150`, the HANG-29 bound), which a scale-1 body
//! scaled up afterwards reaches too (module deviations of `curve::curve::p6`). The swung
//! primitive itself still accepts any scale, as upstream does.
//!
//! A sibling with a count from the input: the capped repeated linear sweep moved its
//! end cap profile once per repetition before the sweep engine's 4096 repetition cap,
//! so `u64::MAX` repetitions did not end; the cap is now checked first.
//!
//! Each case runs on its own thread: without the fix it does not end, and the test fails
//! at the time budget instead of hanging the suite.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as prim;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::proto_topology::p1::api_transform::{api_scale, TransformObject};
use solidcore::{Brep, Kernel, Point3};

/// Generous for the debug build on a loaded machine (each case takes milliseconds in
/// release once fixed).
const BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

/// Runs `f` on its own thread and fails the test past [`BUDGET`].
fn within_budget(f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        f();
        tx.send(()).unwrap();
    });
    rx.recv_timeout(BUDGET)
        .expect("the case did not return within the budget (hang)");
}

/// The fuzz case's curves: an XY path arc of 288 degrees and an XZ profile circle.
fn curves_of_case() -> (Curve, Curve) {
    let path = curves::create_arc(
        Point3::new(
            1.902_500_661_646_435_3,
            0.035_779_313_426_832_715,
            4.084_046_386_820_813,
        ),
        1.472_662_398_908_621_8,
        288.044_840_130_223_24,
        -0.0,
    )
    .unwrap();
    let profile =
        curves::create_circle(Point3::new(0.0, 0.0, 0.0), 0.753_624_163_768_833_1).unwrap();
    (Curve::from(path), Curve::from(profile))
}

fn swung(k: &mut Kernel, scale: f64) -> solidcore::Result<Brep> {
    let (path, profile) = curves_of_case();
    prim::create_swung_primitive(k, &[path], &[profile], scale)
}

/// The fuzz case: the swung primitive, an offset profile that fails (its `-1e300` point
/// only makes it fail), then the piecewise merge and the `Merge` boolean of the one body
/// with itself, each step's error ignored as the fuzz does.
#[test]
fn fuzz_case_returns() {
    within_budget(|| {
        let mut k = Kernel::default();
        let mut bodies = Vec::new();
        bodies.extend(swung(&mut k, f64::MAX).ok());
        let s = 6.187_296_518_882_389;
        let points = [
            [0.0, 0.0, 0.0],
            [0.0, s, 0.0],
            [s, s, -1e300],
            [s, 0.0, 0.0],
            [0.0, s, 0.0],
        ];
        let lines: Vec<Curve> = points
            .windows(2)
            .map(|w| {
                let p = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
                Curve::from(curves::create_line_segment(p(w[0]), p(w[1])).unwrap())
            })
            .collect();
        let refs: Vec<Option<&Curve>> = lines.iter().map(Some).collect();
        bodies.extend(
            prim::create_offset_profile(&mut k, &refs, 0.459_475_522_325_903_94, 3, false, true)
                .ok(),
        );
        // the fuzz's body indices 5, 5 and 6, 3 taken modulo the bodies made
        let n = bodies.len();
        if n > 0 {
            let x = brep_copy(&mut k, &bodies[5 % n]).unwrap();
            let y = brep_copy(&mut k, &bodies[5 % n]).unwrap();
            let _ = boolean::piecewise_merge(&mut k, x, y, false);
            let x = brep_copy(&mut k, &bodies[6 % n]).unwrap();
            let y = brep_copy(&mut k, &bodies[3 % n]).unwrap();
            let _ = boolean::boolean(&mut k, x, y, BooleanOperation::Merge);
        }
    });
}

#[test]
fn merge_of_a_swung_body_scaled_past_the_coincidence_bound_returns() {
    within_budget(|| {
        let mut k = Kernel::default();
        let mut body = swung(&mut k, 1.0).unwrap();
        // a scale-1 body scaled afterwards (nine valid scales of 1e18, to near 1e162)
        // reaches overflowing geometry too
        let s = 1e18;
        for _ in 0..9 {
            api_scale(
                &mut k,
                Some(TransformObject::Brep(&mut body)),
                Point3::new(s, s, s),
            )
            .unwrap();
        }
        let (x, y) = (
            brep_copy(&mut k, &body).unwrap(),
            brep_copy(&mut k, &body).unwrap(),
        );
        let _ = boolean::piecewise_merge(&mut k, x, y, false);
        let x = brep_copy(&mut k, &body).unwrap();
        let _ = boolean::boolean(&mut k, x, body, BooleanOperation::Merge);
    });
}

#[test]
fn capped_linear_sweep_with_u64_max_repetitions_returns() {
    within_budget(|| {
        let mut k = Kernel::default();
        let profile = Curve::from(curves::create_circle(Point3::new(3.0, 0.0, 0.0), 1.0).unwrap());
        let sweep = |k: &mut Kernel, repetitions| {
            sweeps::create_linear_sweep_with_repetitions(
                k,
                &[Some(&profile)],
                Point3::new(0.0, 0.0, 1.0),
                1.0,
                repetitions,
                true,
            )
        };
        let r = sweep(&mut k, u64::MAX);
        assert!(
            matches!(r, Err(SolidError::InvalidInput { .. })),
            "{:?}",
            r.map(|_| ())
        );
    });
}
