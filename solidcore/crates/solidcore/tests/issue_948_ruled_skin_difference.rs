//! tangency-labs/solidcore#948 (usd-brep#32): `SmApiBooleanDifference(outer, inner)` of two
//! capped degree-1 skins (`SmApiCreateSkinPrimitive`) through closed cubic sections fails
//! with `SM_ERR` (1001). Both operands are closed manifold solids. The pinned C++ fails in
//! the same place: `SmBrep::MakeFaceTopology`'s radial-sector recovery misses three times
//! (`SmBrep.cpp:14563`; port `topology/brep/p6.rs`). The input is the issue's `min7.txt`
//! (7 + 7 rounded-rectangle sections of 86 points each, a rain cover's outer and cavity
//! lofts), kept as `tests/data/issue_948_min7.txt`. Each section is closed
//! (`InterpolatePoints(.., 3, .., closed, chord length)` with the first point repeated).
//! The pinned C++ still fails; since the loft seam alignment (#15263, WR-778) this port
//! builds the difference, and these tests pin that solid.
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
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel};

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

/// The difference of the two skins, on both query paths (`query_statuses_pass_on` off and on).
/// The pinned C++ fails here with `SM_ERR` in `SmBrep::MakeFaceTopology`. Since each closed
/// section's seam is aligned with the previous section's (the loft fix for #15263, WR-778), the
/// skins' side faces no longer twist between sections, and the difference is one closed
/// manifold solid smaller than the outer skin.
fn difference_volume(query_statuses_pass_on: bool) -> f64 {
    use solidcore::mass::integrator::properties::brep_volume_default;
    let data = sections(MIN7);
    let mut k = Kernel::default();
    k.fixes.query_statuses_pass_on = query_statuses_pass_on;
    let mut outer = ruled_skin(&mut k, &data[0].1);
    let inner = ruled_skin(&mut k, &data[1].1);
    assert!(is_manifold_solid(&outer).unwrap());
    assert!(is_manifold_solid(&inner).unwrap());
    let outer_volume = brep_volume_default(&k, &mut outer, 1e-3).unwrap();
    let mut body = match boolean_difference(&mut k, outer, inner) {
        Ok(body) => body,
        Err(e) => panic!("expected a solid, got {e}"),
    };
    assert!(is_manifold_solid(&body).unwrap());
    let volume = brep_volume_default(&k, &mut body, 1e-3).unwrap();
    assert!(
        volume > 0.0 && volume < outer_volume,
        "{volume} of {outer_volume}"
    );
    volume
}

#[test]
fn issue_948_difference_of_two_ruled_skins_is_a_solid() {
    let data = sections(MIN7);
    assert_eq!(data.len(), 2);
    assert_eq!((data[0].0.as_str(), data[0].1.len()), ("outer", 7));
    assert_eq!((data[1].0.as_str(), data[1].1.len()), ("inner", 7));
    assert!(data.iter().all(|(_, s)| s.iter().all(|p| p.len() == 86)));
    let volume = difference_volume(false);
    assert!((volume - 74_838.2).abs() < 1.0, "{volume}");
}

/// WR-663 with the fork's tangency-labs/smlib@8ea6a20: a refined end past a curve/curve range
/// does not abort the intersection, and the query path gives the same solid as the default one.
#[test]
fn a_refined_parameter_outside_the_range_does_not_abort_the_intersection() {
    let default_path = difference_volume(false);
    let query_path = difference_volume(true);
    assert!(
        (default_path - query_path).abs() < 1e-3,
        "{default_path} vs {query_path}"
    );
}
