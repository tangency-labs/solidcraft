//! tangency-labs/solidcore#948 (usd-brep#32): `SmApiBooleanDifference(outer, inner)` of two
//! capped degree-1 skins (`SmApiCreateSkinPrimitive`) through closed cubic sections fails
//! with `SM_ERR` (1001). Both operands are closed manifold solids. The pinned C++ fails in
//! the same place: `SmBrep::MakeFaceTopology`'s radial-sector recovery misses three times
//! (`SmBrep.cpp:14563`; port `topology/brep/p6.rs`). The input is the issue's `min7.txt`
//! (7 + 7 rounded-rectangle sections of 86 points each, a rain cover's outer and cavity
//! lofts), kept as `tests/data/issue_948_min7.txt`. Each section is closed
//! (`InterpolatePoints(.., 3, .., closed, chord length)` with the first point repeated).
//! This test pins upstream's failure.
//!
//! Provenance of `tests/data/issue_948_min7.txt` (third-party data): source
//! <https://github.com/NVIDIA-Omniverse/usd-brep/issues/32>, the inline `min7.txt` block,
//! posted by GitHub user `meros` on 2026-10-25. It is the reporter's posted repro data
//! (model input), copied verbatim, and not part of usd-brep's source. No licence is
//! stated in the post.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::error::SM_ERR;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel, SolidError};

const MIN7: &str = include_str!("data/issue_948_min7.txt");

/// The named groups of sections: a header `name count`, then `count` lines of x y z
/// triples. Leading `#` lines (the provenance note) are skipped.
fn sections(text: &str) -> Vec<(String, Vec<Vec<Point3>>)> {
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let mut out = Vec::new();
    while let Some(header) = lines.next() {
        let mut it = header.split_whitespace();
        let name = it.next().unwrap().to_string();
        let n: usize = it.next().unwrap().parse().unwrap();
        let groups = (0..n)
            .map(|_| {
                let v: Vec<f64> = lines
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .map(|s| s.parse().unwrap())
                    .collect();
                v.chunks(3).map(|c| Point3::new(c[0], c[1], c[2])).collect()
            })
            .collect();
        out.push((name, groups));
    }
    out
}

/// A capped degree-1 skin through closed cubic interpolations of `secs`.
fn ruled_skin(k: &mut Kernel, secs: &[Vec<Point3>]) -> Brep {
    let curves: Vec<Curve> = secs
        .iter()
        .map(|pts| {
            let mut closed = pts.clone();
            closed.push(pts[0]);
            Curve::from(
                BSplineCurve::interpolate_points(
                    &closed,
                    None,
                    3,
                    None,
                    None,
                    true,
                    ParamMethod::ChordLength,
                )
                .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_skin_primitive(k, &vec![1; curves.len()], &refs, 1, true)
        .unwrap()
        .brep
}

#[test]
fn issue_948_difference_of_two_ruled_skins_fails_as_upstream() {
    let data = sections(MIN7);
    assert_eq!(data.len(), 2);
    assert_eq!((data[0].0.as_str(), data[0].1.len()), ("outer", 7));
    assert_eq!((data[1].0.as_str(), data[1].1.len()), ("inner", 7));
    assert!(data.iter().all(|(_, s)| s.iter().all(|p| p.len() == 86)));

    let mut k = Kernel::default();
    let outer = ruled_skin(&mut k, &data[0].1);
    let inner = ruled_skin(&mut k, &data[1].1);
    assert!(is_manifold_solid(&outer).unwrap());
    assert!(is_manifold_solid(&inner).unwrap());

    match boolean_difference(&mut k, outer, inner) {
        Err(SolidError::Upstream { code, .. }) => assert_eq!(code, SM_ERR),
        other => panic!(
            "expected SM_ERR as upstream, got {:?}",
            other.map(|_| "a body")
        ),
    }
}
