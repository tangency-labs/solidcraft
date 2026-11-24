//! #18325: the surface drop of an edge cut from a B-spline surface along a side of the
//! face's domain returned UV ends a few ulps off the side (`v = 0.8999999999999999` for the
//! side `v = 0.9`). The iso-line drop (`Surface::drop_iso_curve`) does not invert the end
//! points: its start is the Newton point solve's and its end extrapolates the start by the
//! measured speed ratio `|S_v| / |C'|`. A curve that is, by construction, the surface's iso
//! curve at a side `f` of the domain (equal to the trimmed `CreateIsoParametricCurve`, as a
//! face's natural edges are) is `t -> S(f, t)`, so its UV ends are now exactly `(f, t0)` and
//! `(f, t1)`. A curve made any other way keeps the computed ends.
//! The edge/face gap of a curve on the face's own surface (`CrvOnSurf`) is seeded at the
//! curve's own uv, the exact foot, rather than at the previous sample's foot.
//!
//! The input is the adversarial golden case `adversarial_b102_eu0_binormal_3d`
//! (`fixtures/topology.edgeuse_p3/adversarial.json`): a biquadratic 4 x 4 surface whose
//! `u = 0` row collapses to a point, cut to the face domain `[0.1, 0.8] x [0.2, 0.9]`. Each
//! of the four natural trim curves drops to a UV line between two exact domain corners.

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
fn a_sub_interval_of_an_iso_edge_drops_to_its_exact_parameters() {
    let k = Kernel::default();
    let s = surface();
    let natural = s
        .create_natural_uv_trim_curves(&k, &domain(), SurfParamType::U, false)
        .unwrap();
    for ii in 0..4 {
        let full = natural.curves_3d[ii].natural_interval();
        let short = Extent1d::from_min_max(full.min() + 0.1, full.max() - 1e-9);
        let [start, end] = drop_ends(&k, &s, ii, Some(short));
        // the side is the fixed coordinate; the other is the curve parameter itself
        let fixed = if ii % 2 == 0 { start[0] } else { start[1] };
        let (t0, t1) = if ii % 2 == 0 {
            (start[1], end[1])
        } else {
            (start[0], end[0])
        };
        assert!(
            [0.1, 0.8, 0.2, 0.9].contains(&fixed),
            "side {ii}: {start:?}"
        );
        assert_eq!(
            (t0, t1),
            (short.min(), short.max()),
            "side {ii}: {start:?} -> {end:?}"
        );
    }
}

#[test]
fn a_curve_that_is_not_the_iso_curve_keeps_the_computed_ends() {
    let k = Kernel::default();
    let s = surface();
    let natural = s
        .create_natural_uv_trim_curves(&k, &domain(), SurfParamType::U, false)
        .unwrap();
    // the u = 0.1 side, reparameterised: no longer the surface's iso curve
    let mut curve = natural.curves_3d[0].clone();
    let ivl = curve.natural_interval();
    let moved = Extent1d::from_min_max(ivl.min() + 1.0, ivl.max() + 1.0);
    curve.edit_parameterization(&k, &moved).unwrap();
    let out = s
        .drop_curve_base::<Obj>(&k, &domain(), &curve, &moved, TOL, false, None)
        .unwrap();
    assert_eq!(out.uv_curves.len(), 1, "one UV curve");
    let cp = out.uv_curves[0].control_points().unwrap();
    let end = cp.last().unwrap();
    // not the exact iso line: that path would return the curve's own parameter as the
    // moving coordinate (`moved.max()`, 1.9); the computed inversion finds the geometric
    // end (it may land on 0.9 exactly, or a few ulps off, as in the issue)
    assert_ne!(end.y, moved.max(), "end {end:?}");
    assert!((end.y - 0.9).abs() < 1e-12, "end {end:?}");
    assert!((end.x - 0.1).abs() < 1e-12, "end {end:?}");
}

/// The drop quality of an exact iso drop is measured on the line it returns, `(f, t)`,
/// not on the line extrapolated from a start a few ulps off the side (review r1 B1).
///
/// The start passed here is off the side by a few ulps in both coordinates, as the Newton
/// solve's start is. The expected deviation and drop are recomputed station by station
/// at `(f, param)`, with the drop's own station rule (the curve's knots in the interval,
/// plus its ends; station `i` at `i / n` of the interval).
#[test]
fn an_exact_iso_drop_measures_its_quality_on_the_exact_line() {
    use solidcore::geom::geom_utility::p1::line_point_distance;
    use solidcore::geom::tol::are_same;
    use solidcore::geom::vector2d::Point2;
    use solidcore::math::EFF_ZERO_SQRT;
    use solidcore::nurbs::knots::Side;

    let k = Kernel::default();
    let s = surface();
    let natural = s
        .create_natural_uv_trim_curves(&k, &domain(), SurfParamType::U, false)
        .unwrap();
    // side 0 is the issue's edge, u = 0.1 from v = 0.2 to v = 0.9
    let curve = &natural.curves_3d[0];
    let ivl = curve.natural_interval();
    let (f, t0) = (0.1, ivl.min());
    assert_eq!((t0, ivl.max()), (0.2, 0.9));
    let off = Point2::new(f + 4.0 * f64::EPSILON, t0 - 4.0 * f64::EPSILON);
    let iso = s
        .drop_iso_curve(&k, &domain(), curve, &ivl, off, TOL)
        .unwrap();
    assert!(iso.successful);
    let cp = iso.iso_uv_curve.as_ref().unwrap().control_points().unwrap();
    let (a, b) = (cp.first().unwrap(), cp.last().unwrap());
    assert_eq!([a.x, a.y, b.x, b.y], [0.1, 0.2, 0.1, 0.9]);

    let (mut knots, _) = curve.knots(&k, None).unwrap();
    knots.retain(|&t| t >= ivl.min() - EFF_ZERO_SQRT && t <= ivl.max() + EFF_ZERO_SQRT);
    if knots.first().is_none_or(|&t| !are_same(t, ivl.min())) {
        knots.insert(0, ivl.min());
    }
    if knots.len() == 1 || knots.last().is_some_and(|&t| !are_same(t, ivl.max())) {
        knots.push(ivl.max());
    }
    let n = knots.len();
    let size = ivl.max() - ivl.min();
    let (mut dev, mut drop) = (0.0f64, 0.0f64);
    for i in 1..=n {
        let step = if i < n {
            (i as f64 / n as f64) * size
        } else {
            size
        };
        let param = ivl.clamp_value(ivl.min() + step);
        let uv = Point2::new(f, param);
        let c = curve.evaluate_point(&k, param).unwrap();
        let p = s.evaluate_point(&k, uv).unwrap();
        let nrm = s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap();
        let (d, lp) = line_point_distance(p, nrm, c);
        dev = dev.max(d);
        drop = drop.max(lp.abs());
    }
    assert_eq!(iso.max_approx_dev.to_bits(), dev.to_bits(), "deviation");
    assert_eq!(iso.max_drop_to_surf.to_bits(), drop.to_bits(), "drop");
}

/// The edge/face gap of a `CrvOnSurf` over a natural UV trim on the face's own surface
/// (the goc cases' cached edge/face gap): every point of the curve is the surface's point
/// at the curve's own uv, so the closest-point solve finds that uv, the exact foot, bit
/// for bit. Seeded at the previous sample's foot, it stopped within its tolerance of the
/// foot (about `1e-13` in uv) and the gap came out about `1e-12`.
#[test]
fn a_curve_on_the_face_surface_has_its_own_uv_as_foot() {
    use solidcore::curve::crv_on_surf::CrvOnSurf;
    use solidcore::curve::curve::Curve;
    use solidcore::heal::gap::GapFunction;

    let k = Kernel::default();
    let s = surface();
    let natural = s
        .create_natural_uv_trim_curves(&k, &domain(), SurfParamType::U, false)
        .unwrap();
    for (i, uv) in natural.uv_curves.iter().enumerate() {
        let cos = CrvOnSurf::new(&k, Curve::from(uv.clone()), s.clone(), None, 0).unwrap();
        let curve = Curve::from(cos.clone());
        let ivl = curve.natural_interval();
        let mut gf = GapFunction::crv_srf(TOL, &curve, &ivl, &s, 20);
        let g = gf.max_gap_sample(&k, false).unwrap();
        let own = cos.evaluate_uv_point(&k, g.this_param().x).unwrap();
        let foot = g.other_param();
        assert_eq!(
            [foot.x, foot.y].map(f64::to_bits),
            [own.x, own.y].map(f64::to_bits),
            "side {i}: foot {foot:?}, own uv {own:?}"
        );
        let gap = (g.this_pos() - g.other_pos()).length();
        assert!(gap < 1e-15, "side {i}: gap {gap}");
    }
}
