//! #3066: a round on supports read from a record with rounding in their frames keeps its exact
//! definition. The class is decided within the body's tolerance; the round lies on a torus of
//! its own, its net certified there at bound 0, and that torus's distance from the class's
//! torus from the supports is held to the body's tolerance, so the round answers
//! `procedural_blend()`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test

use std::f64::consts::PI;

use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore_record::brep::Brep as Record;

use crate::blend::{blend, Blend, Corners};
use crate::boolean::boolean;
use crate::{make_box, make_cylinder, session, Body, BooleanOp, CheckLevel};

fn fixes() {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
}

/// The faces of `b` that are no plane, as (with a definition, without one).
fn definitions(b: &Body) -> (usize, usize) {
    let brep = b.lock();
    let (mut with, mut without) = (0, 0);
    for &f in brep.face_list() {
        let s = face::surface(&brep, f).unwrap().unwrap();
        if matches!(s.kind(), SurfaceKind::Plane(_)) {
            continue;
        }
        if s.procedural_blend().is_some() {
            with += 1;
        } else {
            without += 1;
        }
    }
    (with, without)
}

/// The edge of `b` through `p`.
fn edge_through(b: &Body, p: [f64; 3]) -> usize {
    let k = solidcore::Kernel::default();
    let brep = b.lock();
    let q = solidcore::Vec3::new(p[0], p[1], p[2]);
    brep.edge_list()
        .iter()
        .position(|&e| {
            solidcore::query::api_queries::edge_closest_point(&k, &brep, e, q)
                .unwrap()
                .closest
                .distance
                < 1e-6
        })
        .unwrap()
}

/// A boss R 8 standing 10 out of a 40 x 40 x 10 block, its axis turned 1e-10 off square to the
/// block's top, its root rounded 2: a circle-class round on supports only that close to the
/// class (the error the 1e-10 turn makes over the part is far inside the body's tolerance, and
/// far outside the rounding the net is certified to). The round is concave: the corner between
/// the wall and the top, less the quarter disc, turned about the axis. By Pappus, it adds
/// 2 pi (R + x) r^2 (1 - pi / 4), x = r (10 - 3 pi) / (12 - 3 pi) the corner's centroid from the
/// wall.
#[test]
fn a_round_on_a_boss_tilted_off_the_class_keeps_its_definition() {
    fixes();
    let block = make_box([0.0; 3], [40.0, 40.0, 10.0]).unwrap();
    // the boss from z = 5 to 20, its axis turned 1e-10 about x off square to the block's top
    let boss = make_cylinder([0.0; 3], 8.0, 15.0).unwrap();
    let (c, s) = (1e-10_f64.cos(), 1e-10_f64.sin());
    boss.transform(&[
        [1.0, 0.0, 0.0, 0.0],
        [0.0, c, s, 0.0],
        [0.0, -s, c, 0.0],
        [20.0, 20.0, 5.0, 1.0],
    ])
    .unwrap();
    let b = boolean(block, boss, BooleanOp::Union).unwrap().unwrap();
    let e = edge_through(&b, [28.0, 20.0, 10.0]);
    let (r, big_r) = (2.0, 8.0);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // the boss's wall and the round: the round keeps its definition
    assert_eq!(definitions(&b), (1, 1));
    let x = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want =
        16000.0 + PI * big_r * big_r * 10.0 + 2.0 * PI * (big_r + x) * r * r * (1.0 - PI / 4.0);
    let got = b.volume(1e-10).unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} {want}");
}

/// SolidCraft's raked wedge (`tests/data/raked_wedge_noisy_cylinders.json`): its end outline's
/// rounds keep their definitions, the two circle-class rounds on the read-back cylinders among
/// them; the operand's own faces, read from the record, have none.
#[test]
fn the_raked_wedges_rounds_keep_their_definitions() {
    fixes();
    let record: Record = serde_json::from_str(include_str!(
        "../tests/data/raked_wedge_noisy_cylinders.json"
    ))
    .unwrap();
    let (b, _) = Body::from_record(&record, true).unwrap();
    let (_, before) = definitions(&b);
    let top = 60.0 - 60.0 * 10f64.to_radians().tan();
    let mut seeds = Vec::new();
    for x in [-69.0, 69.0] {
        for p in [
            [x, -41.0, 22.0],
            [x, 8.0, 41.0],
            [x, (38.0 + top) / 2.0, 60.0],
        ] {
            seeds.push(edge_through(&b, p));
        }
    }
    let chain = crate::blend::tangent_chain(&b, &seeds, 0.5_f64.to_radians()).unwrap();
    blend(&b, Blend::Round, &chain, &[0.854], &[], Corners::Filled).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let (with, without) = definitions(&b);
    // as many of the new rounds keep their definitions as on the same wedge with exact
    // cylinders, and the rest are as bare there
    let (exact_with, exact_bare) = exact_wedge_rounds();
    assert_eq!((with, without - before), (exact_with, exact_bare));
    assert_eq!(with + without - before, chain.len());
}

/// The same wedge built here, its cylinders exact, its end outline rounded as SolidCraft's: of the
/// rounds that last call makes, how many keep a definition and how many don't.
fn exact_wedge_rounds() -> (usize, usize) {
    use crate::sweep::{extrude, Curve, DraftCorner};
    let top = 60.0 - 60.0 * 10f64.to_radians().tan();
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
    let b = extrude(
        &curves,
        [1.0, 0.0, 0.0],
        138.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let round = |pts: &[[f64; 3]], r: f64, chain: bool| {
        let seeds: Vec<usize> = pts.iter().map(|&p| edge_through(&b, p)).collect();
        let edges = if chain {
            crate::blend::tangent_chain(&b, &seeds, 0.5_f64.to_radians()).unwrap()
        } else {
            seeds
        };
        blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
    };
    round(&[[0.0, -60.0, 22.0]], 1.2, true);
    round(&[[0.0, -22.0, 22.0]], 1.5, true);
    round(&[[0.0, 38.0, 60.0]], 1.5, true);
    let back = (60.0 + top) / 2.0;
    let four = [
        [-69.0, -60.0, 12.0],
        [69.0, -60.0, 12.0],
        [-69.0, back, 30.0],
        [69.0, back, 30.0],
    ];
    round(&four, 4.0, false);
    let (with_before, bare_before) = definitions(&b);
    let mut ends = Vec::new();
    for x in [-69.0, 69.0] {
        ends.extend([
            [x, -41.0, 22.0],
            [x, 8.0, 41.0],
            [x, (38.0 + top) / 2.0, 60.0],
        ]);
    }
    round(&ends, 0.854, true);
    let (with, bare) = definitions(&b);
    (with - with_before, bare - bare_before)
}
