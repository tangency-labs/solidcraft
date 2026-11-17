//! #18325: the surface drop of an edge cut from a B-spline surface along a side of the
//! face's domain returned UV ends a few ulps off the side (`v = 0.8999999999999999` for the
//! side `v = 0.9`). The iso-line drop (`Surface::drop_iso_curve`) does not invert the end
//! points: its start is the Newton point solve's and its end extrapolates the start by the
//! measured speed ratio `|S_v| / |C'|`. A side of the domain is a known parameter, so an
//! end on it now takes that side's exact value when the surface and curve points agree
//! within their certified evaluation error bounds.
//!
//! The input is the adversarial golden case `adversarial_b102_eu0_binormal_3d`
//! (`fixtures/topology.edgeuse_p3/adversarial.json`): a biquadratic 4 x 4 surface whose
//! `u = 0` row collapses to a point, cut to the face domain `[0.1, 0.8] x [0.2, 0.9]`. Each
//! of the four natural trim curves drops to a UV line between two exact domain corners.
//! A curve that stops short of a side by more than rounding keeps its computed end.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::nurbs::b_spline_surface::p1::BSplineSurface;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

#[derive(Clone, Copy, PartialEq)]
struct Obj;
impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

/// The edge tolerance the face's edges carry, the drop's approximation tolerance.
const TOL: f64 = 1e-5;

/// The case's surface: control point `(0.01 i, 0.01 j, z[i][j])`, row 0 collapsed to the
/// point `(0, 0)` in x and y.
fn surface() -> Surface {
    let z = [
        [
            -0.018312366950226835,
            0.01071154283020613,
            0.0025677038194224047,
            -0.012671751225806162,
        ],
        [
            0.007209397218335374,
            -0.0012793880455714746,
            0.01683268638338247,
            0.0070781513750238605,
        ],
        [
            0.003377609487654132,
            0.002398042956638,
            -0.00011398683205153581,
            -0.0052018205137808506,
        ],
        [
            -0.0036062392242838114,
            -0.016842755072325893,
            -0.012347925951995312,
            -0.013455795300022402,
        ],
    ];
    let pw: Vec<Vec<CPoint>> = (0..4)
        .map(|i| {
            (0..4)
                .map(|j| {
                    let (x, y) = if i == 0 {
                        (0.0, 0.0)
                    } else {
                        (0.01 * i as f64, 0.01 * j as f64)
                    };
                    CPoint::new(x, y, z[i][j], NOW)
                })
                .collect()
        })
        .collect();
    Surface::from(
        BSplineSurface::from_data(
            3,
            3,
            2,
            2,
            6,
            6,
            &pw,
            &[0.0, 0.0, 0.0, 0.1116114584111405, 1.0, 1.0, 1.0],
            &[0.0, 0.0, 0.0, 0.48093325471811293, 1.0, 1.0, 1.0],
        )
        .unwrap(),
    )
}

fn domain() -> Extent2d {
    Extent2d::from_bounds(0.1, 0.2, 0.8, 0.9)
}

/// The UV ends of the one line `curve` over `interval` drops to.
fn drop_ends(
    k: &Kernel,
    s: &Surface,
    curve_index: usize,
    interval: Option<Extent1d>,
) -> [[f64; 2]; 2] {
    let natural = s
        .create_natural_uv_trim_curves(k, &domain(), SurfParamType::U, false)
        .unwrap();
    let curve = &natural.curves_3d[curve_index];
    let interval = interval.unwrap_or_else(|| curve.natural_interval());
    let out = s
        .drop_curve_base::<Obj>(k, &domain(), curve, &interval, TOL, false, None)
        .unwrap();
    assert_eq!(out.uv_curves.len(), 1, "one UV curve");
    let cp = out.uv_curves[0].control_points().unwrap();
    let (a, b) = (cp.first().unwrap(), cp.last().unwrap());
    [[a.x, a.y], [b.x, b.y]]
}

#[test]
fn natural_trim_curves_drop_to_exact_domain_corners() {
    let k = Kernel::default();
    let s = surface();
    let natural = s
        .create_natural_uv_trim_curves(&k, &domain(), SurfParamType::U, false)
        .unwrap();
    assert_eq!(natural.curves_3d.len(), 4);
    let mut iso_v_end = false;
    for ii in 0..4 {
        let ends = drop_ends(&k, &s, ii, None);
        for [u, v] in ends {
            assert!(
                u == 0.1 || u == 0.8,
                "side {ii}: u {u:?} is not a domain side"
            );
            assert!(
                v == 0.2 || v == 0.9,
                "side {ii}: v {v:?} is not a domain side"
            );
        }
        // the issue's edge: u = 0.1 from v = 0.2 to v = 0.9
        iso_v_end |= ends == [[0.1, 0.2], [0.1, 0.9]];
    }
    assert!(
        iso_v_end,
        "the u = 0.1 side drops to (0.1, 0.2) -> (0.1, 0.9)"
    );
}

#[test]
fn a_curve_short_of_the_side_keeps_its_end() {
    let k = Kernel::default();
    let s = surface();
    let natural = s
        .create_natural_uv_trim_curves(&k, &domain(), SurfParamType::U, false)
        .unwrap();
    for ii in 0..4 {
        let full = natural.curves_3d[ii].natural_interval();
        // stop 1e-9 of the side's length short of its far end: far above rounding
        let short =
            Extent1d::from_min_max(full.min(), full.max() - 1e-9 * (full.max() - full.min()));
        let [start, end] = drop_ends(&k, &s, ii, Some(short));
        for x in start {
            assert!(
                [0.1, 0.8, 0.2, 0.9].contains(&x),
                "side {ii}: start {start:?}"
            );
        }
        // the coordinate along the side stops short; the fixed one stays on its side
        let off: Vec<f64> = end
            .iter()
            .copied()
            .filter(|x| ![0.1, 0.8, 0.2, 0.9].contains(x))
            .collect();
        assert_eq!(
            off.len(),
            1,
            "side {ii}: end {end:?} has one coordinate off the sides"
        );
        let gap = [0.1, 0.8, 0.2, 0.9]
            .iter()
            .map(|b| (off[0] - b).abs())
            .fold(f64::INFINITY, f64::min);
        assert!(
            gap > 1e-11 && gap < 1e-8,
            "side {ii}: end {end:?} gap {gap:e}"
        );
    }
}
