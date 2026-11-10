//! SolidCraft's raked wedge (#1617 there): `data/raked_wedge_noisy_cylinders.json` is the body it
//! hands the last round of `box_intersect_splits_the_end_off_a_raked_wedge_after_its_end_rounds`,
//! its earlier rounds read back as cylinders with rounding in their frames (radius
//! 1.500000000000035, an axis 1.3e-13 off square to the end plane). The exact class is read within
//! the body's tolerance and the certificate to rounding, so a round of those is the class's
//! surface but not provably: it keeps no definition, and builds. It failed with status 1001.
//!
//! The same wedge built here has exact cylinders and takes the same volume.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test

mod common;

use common::{kernel_copy, rel, volume};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::Vec3;
use solidcore_ops::blend::{blend, tangent_chain, Blend, Corners};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{session, Body, CheckLevel};
use solidcore_record::brep::Brep as Record;

const TANGENT: f64 = 0.5 * std::f64::consts::PI / 180.0;

fn lean() -> f64 {
    10f64.to_radians().tan()
}

/// The edge nearest `p`.
fn pick(b: &Body, p: [f64; 3]) -> usize {
    let (k, brep) = kernel_copy(b);
    let q = Vec3::new(p[0], p[1], p[2]);
    let distance = |i: usize| {
        edge_closest_point(&k, &brep, brep.edge_list()[i], q)
            .unwrap()
            .closest
            .distance
    };
    (0..brep.edge_list().len())
        .min_by(|&x, &y| distance(x).total_cmp(&distance(y)))
        .unwrap()
}

fn round(b: &Body, points: &[[f64; 3]], r: f64, chain: bool) {
    let seeds: Vec<usize> = points.iter().map(|&p| pick(b, p)).collect();
    let edges = if chain {
        tangent_chain(b, &seeds, TANGENT).unwrap()
    } else {
        seeds
    };
    blend(b, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
}

/// The last round: the outline of both ends.
fn round_the_ends(b: &Body) {
    let top = 60.0 - 60.0 * lean();
    let mut ends = Vec::new();
    for x in [-69.0, 69.0] {
        ends.extend([
            [x, -41.0, 22.0],
            [x, 8.0, 41.0],
            [x, (38.0 + top) / 2.0, 60.0],
        ]);
    }
    round(b, &ends, 0.854, true);
}

/// The wedge with its first four rounds, its cylinders exact.
fn exact_wedge() -> Body {
    let top = 60.0 - 60.0 * lean();
    let profile = [
        [-60.0, 0.0],
        [60.0, 0.0],
        [top, 60.0],
        [38.0, 60.0],
        [-22.0, 22.0],
        [-60.0, 22.0],
    ];
    let at = |p: [f64; 2]| [-69.0, p[0], p[1]];
    let curves: Vec<Curve> = (0..6)
        .map(|i| Curve::Line {
            start: at(profile[i]),
            end: at(profile[(i + 1) % 6]),
        })
        .collect();
    let dir = [1.0, 0.0, 0.0];
    let b = extrude(&curves, dir, 138.0, 0.0, DraftCorner::Curvature, true).unwrap();
    round(&b, &[[0.0, -60.0, 22.0]], 1.2, true);
    round(&b, &[[0.0, -22.0, 22.0]], 1.5, true);
    round(&b, &[[0.0, 38.0, 60.0]], 1.5, true);
    let back = (60.0 + top) / 2.0;
    let four = [
        [-69.0, -60.0, 12.0],
        [69.0, -60.0, 12.0],
        [-69.0, back, 30.0],
        [69.0, back, 30.0],
    ];
    round(&b, &four, 4.0, false);
    b
}

#[test]
fn rounds_as_the_exact_wedge_does() {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let record: Record =
        serde_json::from_str(include_str!("data/raked_wedge_noisy_cylinders.json")).unwrap();
    let (noisy, _) = Body::from_record(&record, true).unwrap();
    round_the_ends(&noisy);
    let report = noisy.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(noisy.counts().unwrap()[0], 32);

    let exact = exact_wedge();
    round_the_ends(&exact);
    assert!(
        rel(volume(&noisy), volume(&exact)) < 1e-7,
        "{} {}",
        volume(&noisy),
        volume(&exact)
    );
}
