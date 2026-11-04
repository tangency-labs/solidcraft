//! `SmApiUnifyNormals` with `u64::MAX` samples (panic fuzz of #2490, case
//! `heal:9663500891460284416`, and four more `heal` cases like it): `UnifyNormals`
//! evaluates and drops `num_samples` points of each lamina edge use, so on an open body
//! the call runs for as long as the count says, and it never checked the kernel's
//! cancel token, so it could not be stopped. The C++ reference loops over the same
//! `ULONG` count. The sample loop now checks the token on every sample (module
//! deviations of `heal::stitch::p1`); results are unchanged.
//!
//! Each case runs on its own thread: without the fix the cancelled call does not end,
//! and the test fails at the time budget instead of hanging the suite.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::heal::stitch::p1::api_unify_normals;
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries as q;
use solidcore::sweep::api_brep as sweeps;
use solidcore::{Brep, CancelToken, Kernel, Point3};

/// Generous for the debug build on a loaded machine (a cancelled call returns within
/// one sample once fixed).
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

/// The fuzz case's body: a rectangle drafted to 2.53 at 13.4 degrees, capped at the
/// offset end only, so its other end's edges are lamina edges.
fn drafted_rectangle(k: &mut Kernel) -> Brep {
    let rect: Vec<Curve> = curves::create_rectangle(
        Point3::new(0.0, 0.0, 0.0),
        7.491_523_346_209_184,
        4.634_538_442_301_228,
    )
    .unwrap()
    .into_iter()
    .map(Curve::from)
    .collect();
    let refs: Vec<Option<&Curve>> = rect.iter().map(Some).collect();
    sweeps::create_draft_sweep(
        k,
        &refs,
        2.525_817_411_448_431_7,
        13.408_127_241_652_39,
        false,
        2,
    )
    .unwrap()
}

#[test]
fn unify_normals_with_u64_max_samples_can_be_cancelled() {
    within_budget(|| {
        let mut k = Kernel::default();
        let token = CancelToken::new();
        k.set_cancel_token(token.clone());
        let mut b = drafted_rectangle(&mut k);
        let faces = q::get_faces(&b).unwrap();
        // the fuzz's face pick 43, modulo the faces
        let start = faces[43 % faces.len()];
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1));
            token.cancel();
        });
        let r = api_unify_normals(&mut k, Some(&mut b), Some(start), u64::MAX);
        canceller.join().unwrap();
        assert!(
            matches!(r, Err(SolidError::Cancelled { .. })),
            "{:?}",
            r.map(|f| f.len())
        );
    });
}

#[test]
fn unify_normals_with_small_counts_still_runs() {
    within_budget(|| {
        let mut k = Kernel::default();
        let b = drafted_rectangle(&mut k);
        let start = q::get_faces(&b).unwrap()[0];
        for samples in [0, 4, 64] {
            assert!(api_unify_normals(&mut k, Some(&mut b.clone()), Some(start), samples).is_ok());
        }
    });
}
