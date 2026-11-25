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
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries as q;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p1::swap_uv;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::FaceuseId;
use solidcore::{Brep, CancelToken, FaceId, Kernel, Point3};

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
        for samples in [0, 4, 64] {
            // an owner-keeping copy (`Brep`'s `Clone` drops every surface's owner); the
            // copy numbers its own ids, so its start face is taken from it
            let mut c = brep_copy(&mut k, &b).unwrap();
            let start = q::get_faces(&c).unwrap()[0];
            assert!(api_unify_normals(&mut k, Some(&mut c), Some(start), samples).is_ok());
        }
    });
}

/// Each face of `b` with its upward face use.
fn upward_uses(b: &Brep) -> Vec<(FaceId, Option<FaceuseId>)> {
    q::get_faces(b)
        .unwrap()
        .into_iter()
        .map(|f| (f, fp2::upward_faceuse(b, f).unwrap()))
        .collect()
}

/// The manifold edges of `b` across which the faces' orientations disagree, as
/// `(face, edge, face across)`. They agree when the edge use in one face's upward use
/// has its radial in the other face's upward use and the two run in opposite
/// directions. Also returns the number of (face, manifold edge) pairs looked at.
fn disagreements(b: &Brep) -> (Vec<(FaceId, solidcore::EdgeId, FaceId)>, usize) {
    let mut bad = Vec::new();
    let mut checked = 0;
    for f in q::get_faces(b).unwrap() {
        let fu = fp2::upward_faceuse(b, f).unwrap().unwrap();
        for eu in eu1::faceuse_edgeuses(b, fu, false, None).unwrap() {
            let e = eu1::edge(b, eu).unwrap().unwrap();
            if !edge::is_manifold(b, e).unwrap() {
                continue;
            }
            checked += 1;
            let rad = eu1::radial(b, eu).unwrap().unwrap();
            let rad_fu = eu1::faceuse(b, rad).unwrap().unwrap();
            let g = eu1::faceuse_face(b, rad_fu).unwrap().unwrap();
            assert_ne!(f, g, "edge {e:?} bounds face {f:?} twice");
            let opposite = matches!(
                (
                    eu1::orientation(b, eu).unwrap(),
                    eu1::orientation(b, rad).unwrap()
                ),
                (OrientType::Same, OrientType::Opposite) | (OrientType::Opposite, OrientType::Same)
            );
            if fp2::upward_faceuse(b, g).unwrap() != Some(rad_fu) || !opposite {
                bad.push((f, e, g));
            }
        }
    }
    (bad, checked)
}

/// `UnifyNormals` by value (#2709 follow-up). The drafted rectangle (four walls and the
/// offset cap, open at the other end) is consistently oriented, so nothing flips and no
/// face use changes. With one face (the second) reversed by `SwapUV` and the call
/// started from the first, exactly the reversed face flips back and every manifold edge
/// agrees again. Its neighbours are reached across manifold edges, which take no samples, so
/// the result is the same for every sample count (the samples only look for lamina
/// edges of other faces near each open edge, and this body's open edges meet only at
/// their ends, which the loop skips).
#[test]
fn unify_normals_flips_exactly_the_reversed_face() {
    within_budget(|| {
        let mut k = Kernel::default();
        for samples in [0, 1, 4, 64] {
            // consistent body: an empty list, unchanged orientations
            let mut b = drafted_rectangle(&mut k);
            let faces = q::get_faces(&b).unwrap();
            let before = upward_uses(&b);
            let (bad, n) = disagreements(&b);
            assert!(bad.is_empty() && n > 0, "{bad:?} of {n}");
            let flipped = api_unify_normals(&mut k, Some(&mut b), Some(faces[0]), samples).unwrap();
            assert_eq!(flipped, Vec::<FaceId>::new(), "samples {samples}");
            assert_eq!(upward_uses(&b), before, "samples {samples}");
            assert_eq!(disagreements(&b), (Vec::new(), n), "samples {samples}");

            // one face reversed: it disagrees with its neighbours, and flips back
            let mut b = drafted_rectangle(&mut k);
            let reversed = faces[1];
            swap_uv(&k, &mut b, reversed).unwrap();
            let (bad, _) = disagreements(&b);
            assert!(
                !bad.is_empty() && bad.iter().all(|&(f, _, g)| f == reversed || g == reversed),
                "{bad:?}"
            );
            let reversed_uses = upward_uses(&b);
            let flipped = api_unify_normals(&mut k, Some(&mut b), Some(faces[0]), samples).unwrap();
            assert_eq!(flipped, vec![reversed], "samples {samples}");
            assert_eq!(disagreements(&b), (Vec::new(), n), "samples {samples}");
            // the reversed face's upward use changed back; no other face's did
            for ((f, was), (g, now)) in reversed_uses.iter().zip(&upward_uses(&b)) {
                assert_eq!(f, g);
                assert_eq!(was != now, *f == reversed, "face {f:?}, samples {samples}");
            }
            assert_eq!(upward_uses(&b), before, "samples {samples}");
        }
    });
}
