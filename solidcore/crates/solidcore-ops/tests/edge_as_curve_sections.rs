//! A skin's section edge taken back as a free curve is the section: two curves through
//! points (chord-length knots) of different point counts, skinned, carry merged knots; each
//! section edge, asked for at its own degree and span count, comes back as itself, not as a
//! refit on uniform knots (which misses such an edge by millimetres).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::curve_deviation;
use solidcore_ops::curve_fit::{edge_as_curve, ep_curve};
use solidcore_ops::sweep::{loft, Curve as Path};
use solidcore_record::brep::Curve;

/// The shoulder: five points at y = 0, rising and falling.
const SHOULDER: [[f64; 3]; 5] = [
    [0.0, 0.0, 10.0],
    [7.0, 0.0, 13.0],
    [20.0, 0.0, 14.0],
    [31.0, 0.0, 12.5],
    [40.0, 0.0, 10.0],
];
/// The sill: four points at y = 12, lower and flatter.
const SILL: [[f64; 3]; 4] = [
    [0.0, 12.0, 0.0],
    [12.0, 12.0, 2.0],
    [29.0, 12.0, 1.0],
    [40.0, 12.0, 0.0],
];

/// A record spline as a section.
fn section(c: &Curve) -> Path {
    let Curve::Spline { knots, points } = c else {
        panic!("a spline: {c:?}")
    };
    Path::Nurbs {
        degree: knots.len() - points.len() - 1,
        points: points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
        knots: knots.clone(),
    }
}

/// The spans of a spline: its knot intervals of some length.
fn spans(c: &Curve) -> usize {
    let Curve::Spline { knots, .. } = c else {
        panic!("a spline: {c:?}")
    };
    knots.windows(2).filter(|w| w[1] > w[0]).count()
}

#[test]
fn each_section_edge_of_a_skin_of_ep_curves_comes_back_as_its_section() {
    let (shoulder, _) = ep_curve(&SHOULDER, 3).unwrap();
    let (sill, _) = ep_curve(&SILL, 3).unwrap();
    assert_ne!(spans(&shoulder), spans(&sill), "the sections' knots differ");
    let body = loft(&[vec![section(&shoulder)], vec![section(&sill)]], 1, false).unwrap();
    let (record, order) = body.to_record().unwrap();
    let mut found = 0;
    for (i, e) in record.edges.iter().enumerate() {
        if !matches!(e.curve, Curve::Spline { .. }) || spans(&e.curve) < 2 {
            continue;
        }
        let (curve, bound) = edge_as_curve(&body, &order, i, 3, spans(&e.curve), 1e-9)
            .unwrap_or_else(|err| panic!("edge {i}: {err:?}"));
        assert!(bound <= 1e-9, "edge {i}: bound {bound}");
        let off = [&shoulder, &sill]
            .map(|s| curve_deviation(&curve, s, 200).unwrap().max)
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        assert!(off <= 1e-9, "edge {i} is {off} mm off its section");
        found += 1;
    }
    assert_eq!(found, 2, "the two section edges: {:?}", record.edges);
}

/// The car walk's side slab as it was drawn: its shoulder (five points) and sill (four) in one
/// side-view plane, y = -900, their fits as the client saved them. Skinned, the slab is one
/// plane face whose boundary the kernel builds from those curves and C0 piecewise cubics (triple
/// knots). Every edge, asked for as the client asks (degree 3, the edge record curve's own span
/// count, Preferences' position tolerance of 0.01 mm), comes back within that tolerance; the
/// uniform refit missed one of them by 6.8 mm.
#[test]
fn every_edge_of_the_walks_flat_slab_comes_back_within_the_position_tolerance() {
    let shoulder = Path::Nurbs {
        degree: 3,
        points: [
            [77.3, -900.0, 407.9],
            [656.191280742273, -900.0, 1193.7305549918165],
            [1919.9752541558773, -900.0, 363.5317439831184],
            [3188.4434025060154, -900.0, 938.1288117582549],
            [3770.0, -900.0, 975.8],
        ]
        .map(|[x, y, z]| [x, y, z, 1.0])
        .to_vec(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5318694394883184, 1.0, 1.0, 1.0, 1.0],
    };
    let sill = Path::Nurbs {
        degree: 3,
        points: [
            [1885.5, -900.0, 484.3],
            [2447.07720037759, -900.0, 315.9471768903195],
            [3134.5038779137312, -900.0, 973.7758752568442],
            [3668.2, -900.0, 547.9],
        ]
        .map(|[x, y, z]| [x, y, z, 1.0])
        .to_vec(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    };
    let body = loft(&[vec![shoulder], vec![sill]], 1, false).unwrap();
    let (record, order) = body.to_record().unwrap();
    for (i, e) in record.edges.iter().enumerate() {
        // the client's span count: the record curve's knot intervals, a line one
        let spans = match &e.curve {
            Curve::Spline { .. } => spans(&e.curve),
            _ => 1,
        };
        let made = edge_as_curve(&body, &order, i, 3, spans, 0.01);
        assert!(
            matches!(made, Ok((_, bound)) if bound <= 0.01),
            "edge {i} ({spans} spans): {made:?}"
        );
    }
}
