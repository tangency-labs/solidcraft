//! #2091 (WR-670): each iso-u integral of `SmODEAreaIFEO` runs knot span by knot span
//! (WR-494), but every piece's last point was evaluated in the knot span above it (upstream's
//! `bVFromLeft = TRUE`). Where the surface's derivatives jump at a v knot (a G2 but
//! not C2 section, merged into one face under `geometric_g2_continuity`), the integrand
//! jumped at each piece's end and the Runge-Kutta steps shrank toward every knot; at the
//! C0 double knot of #1435's multi-span patch they underflowed at accuracies 1e-9 and
//! tighter. Each piece's end is now evaluated from the piece's own span (the one-sided
//! limit from inside it), the only path.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use solidcore::blend::fillet_executive::api::create_face_from_surface;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tol;
use solidcore::mass::integrator::properties::{
    boundary_properties, brep_volume, face_properties, PropertyAttributes,
};
use solidcore::mass::integrator::{OdeIntegrator, OdeOptions, StepCounts};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::face::p2::{OdeIsoCurveLength, PrecisePropertyLayout, PPF_ALL};
use solidcore::topology::Brep;
use solidcore::Kernel;
use std::f64::consts::PI;

const RADIUS: f64 = 5.0;
const HEIGHT: f64 = 10.0;
/// Unequal arcs on unit knot spans: the section's speed jumps at every join (C0 in its
/// parameter), its tangent and curvature do not (G2).
const ARCS_DEG: [f64; 6] = [30.0, 50.0, 70.0, 40.0, 80.0, 90.0];

/// The circle of `RADIUS` at height `z`, as one rational quadratic arc a unit knot span.
fn section(z: f64) -> Vec<Curve> {
    let mut start = 0.0_f64;
    let mut out = Vec::new();
    for (i, deg) in ARCS_DEG.iter().enumerate() {
        let half = deg.to_radians() / 2.0;
        let mid = start + half;
        let end = start + 2.0 * half;
        let w = half.cos();
        let p0 = Vec3::new(z, RADIUS * start.cos(), RADIUS * start.sin());
        let p1 = Vec3::new(z, RADIUS * mid.cos() / w, RADIUS * mid.sin() / w);
        let p2 = Vec3::new(z, RADIUS * end.cos(), RADIUS * end.sin());
        let cpw = [
            CPoint::new(p0.x, p0.y, p0.z, 1.0),
            CPoint::new(p1.x * w, p1.y * w, p1.z * w, w),
            CPoint::new(p2.x, p2.y, p2.z, 1.0),
        ];
        let k0 = i as f64;
        let k1 = k0 + 1.0;
        let knots = [k0, k0, k0, k1, k1, k1];
        out.push(Curve::from(
            BSplineCurve::from_data(3, 2, 5, 2, &cpw, &knots).unwrap(),
        ));
        start = end;
    }
    out
}

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.geometric_g2_continuity = true;
    k
}

/// The capped prism lofted between the two sections: one side face across all six
/// spans under `geometric_g2_continuity`.
fn prism(k: &mut Kernel) -> Brep {
    let curves: Vec<Curve> = section(0.0).into_iter().chain(section(HEIGHT)).collect();
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let n = ARCS_DEG.len();
    let mut b = create_skin_primitive(k, &[n, n], &refs, 1, true)
        .unwrap()
        .brep;
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

/// The volume of the prism spanning six knot spans matches π r² h.
#[test]
fn multi_span_prism_volume_matches_closed_form() {
    let exact = PI * RADIUS * RADIUS * HEIGHT;
    let mut k = kernel();
    let mut b = prism(&mut k);
    assert_eq!(b.face_list().len(), 3, "one side face and two caps");
    let v = volume(&k, &mut b, 1e-8);
    let err = (v - exact).abs() / exact;
    eprintln!("volume {v} vs {exact} ({err:e})");
    assert!(err <= 1e-9, "volume {v} vs {exact} ({err:e} relative)");
}

/// The side face's B-spline surface and its v knots (ends included).
fn side_surface(b: &Brep) -> (Surface, Vec<f64>) {
    for &f in b.face_list() {
        let s = b.face(f).unwrap().surface.clone().unwrap();
        let k = Kernel::default();
        let knots = solidcore::topology::face::p2::surface_knots_inside(
            &k,
            &s,
            solidcore::nurbs::b_spline_surface::p1::SurfParamType::V,
            f64::MIN,
            f64::MAX,
        )
        .unwrap();
        if knots.len() == ARCS_DEG.len() + 1 {
            return (s, knots);
        }
    }
    panic!("no multi-span face");
}

/// One iso-u piece from a v knot to the next, as `SmODEAreaIFEO` integrates it.
fn piece_steps(eval: &OdeIsoCurveLength<'_>, a: f64, b: f64, n: usize) -> (Vec<f64>, StepCounts) {
    let mut y = vec![0.0; n];
    let scale = vec![1.0; n];
    let mut scratch = vec![0.0; n + 1];
    let counts = OdeIntegrator::default()
        .integrate(
            &mut y,
            a,
            b,
            OdeOptions {
                accuracy: 1e-9,
                initial_step: (b - a) / 2.0,
                minimum_step: 0.0,
                scale: Some(&scale),
                quadrature: true,
                rounding: None,
                rate: None,
            },
            |t, _, dy| {
                eval.evaluate(t, &mut scratch)?;
                for (d, v) in dy.iter_mut().zip(scratch.iter().skip(1)) {
                    *d = *v;
                }
                Ok(())
            },
        )
        .unwrap();
    (y, counts)
}

/// At a piece's end knot the integrand evaluated from below is the limit from inside
/// the piece, and the piece takes fewer steps; evaluated from above (upstream's side)
/// it is the next span's value there.
#[test]
fn piece_end_is_evaluated_in_its_own_span() {
    let mut k = kernel();
    let b = prism(&mut k);
    let (surface, knots) = side_surface(&b);
    let flags = solidcore::topology::face::p2::PPF_AREA;
    let layout = PrecisePropertyLayout::new(flags);
    let n = layout.num_components;
    let (lo, hi) = (knots[0], knots[1]);
    let u = 0.5;
    let upstream = OdeIsoCurveLength::new(&k, &surface, u, Vec3::ZERO, 0.0, &layout);
    let fixed = upstream.with_v_end_from_below(hi);

    let mut at_end_up = vec![0.0; n + 1];
    let mut at_end_fix = vec![0.0; n + 1];
    let mut inside = vec![0.0; n + 1];
    upstream.evaluate(hi, &mut at_end_up).unwrap();
    fixed.evaluate(hi, &mut at_end_fix).unwrap();
    upstream.evaluate(hi - 1e-9, &mut inside).unwrap();
    let area = |d: &[f64]| d[1];
    assert!(
        (area(&at_end_fix) - area(&inside)).abs() <= 1e-6 * area(&inside),
        "fixed end {} vs inside {}",
        area(&at_end_fix),
        area(&inside)
    );
    assert!(
        (area(&at_end_up) - area(&inside)).abs() > 1e-3 * area(&inside),
        "upstream end {} should be the next span's, not {}",
        area(&at_end_up),
        area(&inside)
    );

    let (y_up, c_up) = piece_steps(&upstream, lo, hi, n);
    let (y_fix, c_fix) = piece_steps(&fixed, lo, hi, n);
    eprintln!("steps upstream {c_up:?} fixed {c_fix:?}; {y_up:?} vs {y_fix:?}");
    assert!(
        c_fix.good + c_fix.reduced < c_up.good + c_up.reduced,
        "fixed {c_fix:?} vs upstream {c_up:?}"
    );
    assert!((y_fix[0] - y_up[0]).abs() <= 1e-8 * y_fix[0].abs());
}

/// The #1435 multi-span patch (`mass.integrator`'s face kind 5, `issue1435_multispan`):
/// a rational quadratic B-spline over `[0, 1]²` with a simple u knot at 0.4 and a double
/// v knot at 0.5, where its v derivatives, so the iso-u integrand, jump (C0).
fn multispan_surface() -> Surface {
    let (p0, size) = (Vec3::new(1.0, 2.0, 3.0), (2.0, 3.0));
    let cp: Vec<Vec<_>> = (0..4)
        .map(|i: i32| {
            (0..5)
                .map(|j: i32| {
                    CPoint::new(
                        p0.x + f64::from(i) * size.0 / 3.0,
                        p0.y + f64::from(j) * size.1 / 4.0,
                        p0.z + f64::from(i * (3 - i) * j * (4 - j)) * 0.125,
                        if j == 2 { 0.8 } else { 1.0 },
                    )
                })
                .collect()
        })
        .collect();
    Surface::from(
        BSplineSurface::from_data(
            3,
            4,
            2,
            2,
            6,
            7,
            &cp,
            &[0., 0., 0., 0.4, 1., 1., 1.],
            &[0., 0., 0., 0.5, 0.5, 1., 1., 1.],
        )
        .unwrap(),
    )
}

/// Gauss-Legendre nodes and weights on `[-1, 1]` (Newton on `P_n`).
fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    (1..=n)
        .map(|i| {
            let mut x = (PI * (i as f64 - 0.25) / (n as f64 + 0.5)).cos();
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, x);
                for k in 2..=n {
                    let kf = k as f64;
                    let p2 = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
                    p0 = p1;
                    p1 = p2;
                }
                let dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
                let dx = p1 / dp;
                x -= dx;
                if dx.abs() < 1e-16 {
                    break;
                }
            }
            let (mut p0, mut p1) = (1.0, x);
            for k in 2..=n {
                let kf = k as f64;
                let p2 = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
                p0 = p1;
                p1 = p2;
            }
            let dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
            (x, 2.0 / ((1.0 - x * x) * dp * dp))
        })
        .collect()
}

/// Every precise-property component of the whole patch by tensor Gauss-Legendre with
/// `n` points a knot span in each direction: the integrand is analytic inside each span,
/// and no node lies on a knot. Indexed as the layout's components (`[0]` unused).
fn multispan_reference(
    k: &Kernel,
    surface: &Surface,
    origin: Vec3,
    layout: &PrecisePropertyLayout,
    n: usize,
) -> Vec<f64> {
    let gl = gauss_legendre(n);
    let mut total = vec![0.0; layout.num_components + 1];
    let mut dydx = vec![0.0; layout.num_components + 1];
    for (u0, u1) in [(0.0, 0.4), (0.4, 1.0)] {
        for (v0, v1) in [(0.0, 0.5), (0.5, 1.0)] {
            let (hu, hv) = ((u1 - u0) / 2.0, (v1 - v0) / 2.0);
            for &(xu, wu) in &gl {
                let u = u0 + hu * (xu + 1.0);
                let eval = OdeIsoCurveLength::new(k, surface, u, origin, 0.0, layout);
                for &(xv, wv) in &gl {
                    eval.evaluate(v0 + hv * (xv + 1.0), &mut dydx).unwrap();
                    for (t, d) in total.iter_mut().zip(dydx.iter()) {
                        *t += wu * hu * wv * hv * d;
                    }
                }
            }
        }
    }
    total
}

/// The #1435 multi-span face's every precise property (flags 63) holds requested
/// accuracies 1e-9 to 1e-11 against an independent reference. Each iso-u piece below
/// the double v knot 0.5 ends there; evaluated at its end from the span above, its last
/// sample was the next span's value, the steps underflowed approaching 0.5 and the
/// face's properties failed at 1e-9 and tighter.
#[test]
fn multispan_face_holds_tight_accuracies() {
    let mut k = Kernel::default();
    let surface = multispan_surface();
    let origin = Vec3::new(2.37, 3.21, 3.41);
    let layout = PrecisePropertyLayout::new(PPF_ALL);
    let reference = multispan_reference(&k, &surface, origin, &layout, 24);
    let check = multispan_reference(&k, &surface, origin, &layout, 32);
    for (r, c) in reference.iter().zip(check.iter()) {
        assert!((r - c).abs() <= 1e-14 * r.abs().max(1.0), "{r} vs {c}");
    }
    let mut b = solidcore::topology::brep::p3::new_brep(&mut k, tol::USE_DEFAULT, tol::USE_DEFAULT)
        .unwrap();
    let f = create_face_from_surface(&mut k, &mut b, &surface).unwrap();
    let bbox = solidcore::query::face::p1::calculate_bounding_box(&k, &b, f, false).unwrap();
    let size = bbox.size();
    let d = size.z.max(size.x.max(size.y));
    let r = |i: usize| reference[i];
    for accuracy in [1e-9, 1e-10, 1e-11] {
        let p = face_properties(
            &k,
            &mut b,
            f,
            OrientType::Same,
            accuracy,
            origin,
            d * d / 100.0,
            0.0,
            PPF_ALL,
        )
        .unwrap_or_else(|e| panic!("accuracy {accuracy:e}: {e}"));
        // (area; volume; area first, second (3 groups); volume first, second (3 groups))
        // (the volume integrals take the face's side: -1 for SAME without thickness)
        let mut groups = vec![(vec![p.area], vec![r(layout.area)])];
        groups.push((vec![p.volume], vec![-r(layout.volume)]));
        let moments = |m: &[Vec3], idx: &[usize], sign: f64| {
            let got: Vec<f64> = m.iter().flat_map(|v| [v.x, v.y, v.z]).collect();
            let want: Vec<f64> = idx
                .iter()
                .flat_map(|&i| [r(i), r(i + 1), r(i + 2)].map(|w| w * sign))
                .collect();
            (got, want)
        };
        groups.push(moments(&p.moments[0..1], &layout.moments[0..1], 1.0));
        groups.push(moments(&p.moments[1..4], &layout.moments[1..4], 1.0));
        groups.push(moments(&p.moments[4..5], &layout.moments[4..5], -1.0));
        groups.push(moments(&p.moments[5..8], &layout.moments[5..8], -1.0));
        for (gi, (got, want)) in groups.iter().enumerate() {
            let size = want.iter().fold(0.0_f64, |m, w| m.max(w.abs()));
            let err = got
                .iter()
                .zip(want.iter())
                .fold(0.0_f64, |m, (g, w)| m.max((g - w).abs()))
                / size;
            eprintln!("accuracy {accuracy:e} group {gi}: {err:e} ({got:?} vs {want:?})");
            assert!(
                err <= accuracy,
                "accuracy {accuracy:e}, group {gi}: {err:e} off"
            );
        }
    }
}

/// A strip `[0, 1] x [0, n]`, quadratic across (one span in u) and cubic along (`n` unit
/// knot spans in v), rational and wavy, as a screw wall is long in v.
fn long_strip(n: usize) -> Surface {
    let cp: Vec<Vec<_>> = (0..3)
        .map(|i: i32| {
            (0..n + 3)
                .map(|j| {
                    let (x, y) = (0.5 * f64::from(i), j as f64);
                    let z = 0.3 * (0.7 * y).sin() + 0.1 * x * x + 0.05 * x * y.cos();
                    let w = if i == 1 { 0.9 } else { 1.0 };
                    CPoint::new(x * w, y * w, z * w, w)
                })
                .collect()
        })
        .collect();
    let mut v = vec![0.0; 4];
    v.extend((1..n).map(|j| j as f64));
    v.extend([n as f64; 4]);
    Surface::from(
        BSplineSurface::from_data(2, n + 2, 2, 3, 5, n + 6, &cp, &[0., 0., 0., 1., 1., 1.], &v)
            .unwrap(),
    )
}

/// The strip's slanted parallelogram: u in `[l(v), l(v) + 0.5]`, `l(v) = 0.2 + 0.05 v / n`,
/// v in `[0.3, n - 0.3]`.
fn strip_left(v: f64, n: usize) -> f64 {
    0.2 + 0.05 * v / n as f64
}

/// The parallelogram's four sides, counterclockwise in uv. The long sides are polylines
/// with a corner (collinear) at every half-integer v, so they have a knot in every span of
/// the strip, as a screw wall's long trims do.
fn strip_trims(n: usize) -> Vec<Curve> {
    let (v0, v1) = (0.3, n as f64 - 0.3);
    let side = |du: f64, up: bool| {
        let mut vs = vec![v0];
        vs.extend((0..n).map(|j| j as f64 + 0.5));
        vs.push(v1);
        if !up {
            vs.reverse();
        }
        let m = vs.len();
        let cp: Vec<CPoint> = vs
            .iter()
            .map(|&v| CPoint::new(strip_left(v, n) + du, v, 0.0, 1.0))
            .collect();
        // chord-length knots of the polyline: its parameter is v (or -v) itself
        let mut knots = vec![vs[0].abs()];
        knots.extend(vs.iter().map(|v| if up { *v } else { -*v }));
        knots.push(*knots.last().unwrap());
        knots[0] = knots[1];
        Curve::from(BSplineCurve::from_data(3, m - 1, m + 1, 1, &cp, &knots).unwrap())
    };
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            solidcore::primitive::api_curves::create_line_segment(
                Point3::new(a.0, a.1, 0.0),
                Point3::new(b.0, b.1, 0.0),
            )
            .unwrap(),
        )
    };
    vec![
        line((strip_left(v0, n), v0), (strip_left(v0, n) + 0.5, v0)),
        side(0.5, true),
        line((strip_left(v1, n) + 0.5, v1), (strip_left(v1, n), v1)),
        side(0.0, false),
    ]
}

/// Every precise-property component over the parallelogram by Gauss-Legendre with `g`
/// points a unit of v (each knot span on its own, the ends' partial spans included) and
/// `g` across u (one span): the integrand is analytic in each piece.
fn strip_reference(
    k: &Kernel,
    surface: &Surface,
    n: usize,
    origin: Vec3,
    layout: &PrecisePropertyLayout,
    g: usize,
) -> Vec<f64> {
    let gl = gauss_legendre(g);
    let mut total = vec![0.0; layout.num_components + 1];
    let mut dydx = vec![0.0; layout.num_components + 1];
    for j in 0..n {
        let (v0, v1) = ((j as f64).max(0.3), ((j + 1) as f64).min(n as f64 - 0.3));
        let hv = (v1 - v0) / 2.0;
        for &(xv, wv) in &gl {
            let v = v0 + hv * (xv + 1.0);
            let (u0, hu) = (strip_left(v, n), 0.25);
            for &(xu, wu) in &gl {
                let eval =
                    OdeIsoCurveLength::new(k, surface, u0 + hu * (xu + 1.0), origin, 0.0, layout);
                eval.evaluate(v, &mut dydx).unwrap();
                for (t, d) in total.iter_mut().zip(dydx.iter()) {
                    *t += wu * hu * wv * hv * d;
                }
            }
        }
    }
    total
}

/// #14460: a face long in v (a thread flank over every turn) was integrated with every
/// trim sample's iso-u line running from the trims' lowest v up to the sample, span by
/// span, so its cost grew with the square of the spans it crosses. Banded at its v knots,
/// the iso-curve integrand's evaluations per span stay flat as the strip grows (they
/// grew 1.9x from 8 to 64 spans before), and every property still holds its accuracy
/// against an independent quadrature.
#[test]
fn a_long_face_costs_linearly_in_its_knot_spans() {
    let k = Kernel::default();
    let origin = Vec3::new(0.3, -0.2, -1.0);
    let layout = PrecisePropertyLayout::new(PPF_ALL);
    let accuracy = 1e-9;
    let mut per_span = Vec::new();
    for n in [16_usize, 64] {
        let surface = long_strip(n);
        let reference = strip_reference(&k, &surface, n, origin, &layout, 16);
        let check = strip_reference(&k, &surface, n, origin, &layout, 24);
        for (r, c) in reference.iter().zip(check.iter()) {
            assert!((r - c).abs() <= 1e-13 * r.abs().max(1.0), "{r} vs {c}");
        }
        let trims = strip_trims(n);
        let face_box = Extent3d::from_points(
            Point3::new(-1.0, -1.0, -1.0),
            Point3::new(2.0, n as f64 + 1.0, 1.0),
        );
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, n as f64);
        k.work_counters.reset();
        let p = boundary_properties(
            &k,
            &surface,
            &trims,
            &domain,
            &face_box,
            OrientType::Opposite,
            accuracy,
            origin,
            0.5 * n as f64 / 10.0,
            0.0,
            PPF_ALL,
        )
        .unwrap();
        let evaluations = k.work_counters.mass_iso_evaluations();
        per_span.push(evaluations as f64 / n as f64);
        let r = |i: usize| reference[i];
        let mut groups = vec![(vec![p.area], vec![r(layout.area)])];
        groups.push((vec![p.volume], vec![r(layout.volume)]));
        let moments = |m: &[Vec3], idx: &[usize]| {
            let got: Vec<f64> = m.iter().flat_map(|v| [v.x, v.y, v.z]).collect();
            let want: Vec<f64> = idx
                .iter()
                .flat_map(|&i| [r(i), r(i + 1), r(i + 2)])
                .collect();
            (got, want)
        };
        groups.push(moments(&p.moments[0..1], &layout.moments[0..1]));
        groups.push(moments(&p.moments[1..4], &layout.moments[1..4]));
        groups.push(moments(&p.moments[4..5], &layout.moments[4..5]));
        groups.push(moments(&p.moments[5..8], &layout.moments[5..8]));
        for (gi, (got, want)) in groups.iter().enumerate() {
            let size = want.iter().fold(0.0_f64, |m, w| m.max(w.abs()));
            let err = got
                .iter()
                .zip(want.iter())
                .fold(0.0_f64, |m, (g, w)| m.max((g - w).abs()))
                / size;
            eprintln!("{n} spans, group {gi}: {err:e}");
            assert!(err <= accuracy, "{n} spans, group {gi}: {err:e} off");
        }
        eprintln!("{n} spans: {evaluations} iso evaluations");
    }
    assert!(
        per_span[1] <= 1.3 * per_span[0],
        "iso evaluations per span {per_span:?} grow with the spans"
    );
}

/// Each property group's error against `reference` (indexed by `layout`), over the
/// largest reference value of the group: area, volume, then the moment groups.
fn group_errors(
    p: &solidcore::mass::integrator::properties::PreciseProperties,
    reference: &[f64],
    layout: &PrecisePropertyLayout,
) -> Vec<f64> {
    let r = |i: usize| reference[i];
    let mut groups = vec![(vec![p.area], vec![r(layout.area)])];
    groups.push((vec![p.volume], vec![r(layout.volume)]));
    let moments = |m: &[Vec3], idx: &[usize]| {
        let got: Vec<f64> = m.iter().flat_map(|v| [v.x, v.y, v.z]).collect();
        let want: Vec<f64> = idx
            .iter()
            .flat_map(|&i| [r(i), r(i + 1), r(i + 2)])
            .collect();
        (got, want)
    };
    for range in [0..1, 1..4, 4..5, 5..8] {
        groups.push(moments(&p.moments[range.clone()], &layout.moments[range]));
    }
    groups
        .iter()
        .map(|(got, want)| {
            let size = want.iter().fold(0.0_f64, |m, w| m.max(w.abs()));
            got.iter()
                .zip(want.iter())
                .fold(0.0_f64, |m, (g, w)| m.max((g - w).abs()))
                / size
        })
        .collect()
}

/// #14460: a face whose trims cross fewer knot lines than banding needs keeps one iso-u
/// base, so each trim sample's iso-u line runs the face's height in v, span by span (a
/// thread's run-out cylinder, whose iso-u lines are quarter turns). Its trim pieces were
/// integrated by nested Runge-Kutta, at least 144 iso-curve evaluations a piece and many
/// steps a line: this 8-span strip cost 964,934 evaluations at accuracy 1e-9, with errors
/// up to 3.5e-12 against an independent Gauss-Legendre quadrature. With nested 7-point
/// Gauss-Kronrod, as the bands are, it costs 534,807, and every error is below 3e-15.
#[test]
fn an_unbanded_long_face_integrates_its_trim_pieces_by_gauss_kronrod() {
    let k = Kernel::default();
    let origin = Vec3::new(0.3, -0.2, -1.0);
    let layout = PrecisePropertyLayout::new(PPF_ALL);
    let n = 8_usize;
    let surface = long_strip(n);
    let reference = strip_reference(&k, &surface, n, origin, &layout, 16);
    let check = strip_reference(&k, &surface, n, origin, &layout, 24);
    for (r, c) in reference.iter().zip(check.iter()) {
        assert!((r - c).abs() <= 1e-13 * r.abs().max(1.0), "{r} vs {c}");
    }
    let face_box = Extent3d::from_points(
        Point3::new(-1.0, -1.0, -1.0),
        Point3::new(2.0, n as f64 + 1.0, 1.0),
    );
    let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, n as f64);
    for accuracy in [1e-9, 1e-11] {
        k.work_counters.reset();
        let p = boundary_properties(
            &k,
            &surface,
            &strip_trims(n),
            &domain,
            &face_box,
            OrientType::Opposite,
            accuracy,
            origin,
            0.5 * n as f64 / 10.0,
            0.0,
            PPF_ALL,
        )
        .unwrap();
        let evaluations = k.work_counters.mass_iso_evaluations();
        let errors = group_errors(&p, &reference, &layout);
        eprintln!("accuracy {accuracy:e}: {evaluations} iso evaluations, errors {errors:?}");
        for (gi, err) in errors.iter().enumerate() {
            assert!(
                *err <= 1e-12,
                "accuracy {accuracy:e}, group {gi}: {err:e} off"
            );
        }
        if accuracy == 1e-9 {
            assert!(evaluations <= 700_000, "{evaluations} iso evaluations");
        }
    }
}

/// A trim along the trims' lowest v, the base of every iso-u integral, adds nothing: each
/// of its points' iso-u integrals runs from the base to itself. It evaluates nothing
/// either; before, each of its trim samples took the inner quadrature's seven samples of
/// that zero-width line, at least 49 iso evaluations a piece.
#[test]
fn a_trim_along_the_base_evaluates_no_iso_line() {
    let k = Kernel::default();
    let n = 1_usize;
    let surface = long_strip(n);
    let base_trim = strip_trims(n).into_iter().next().unwrap();
    let face_box = Extent3d::from_points(Point3::new(-1.0, -1.0, -1.0), Point3::new(2.0, 2.0, 1.0));
    let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, n as f64);
    k.work_counters.reset();
    let p = boundary_properties(
        &k,
        &surface,
        &[base_trim],
        &domain,
        &face_box,
        OrientType::Opposite,
        1e-9,
        Vec3::new(0.3, -0.2, -1.0),
        0.05,
        0.0,
        PPF_ALL,
    )
    .unwrap();
    assert_eq!(k.work_counters.mass_iso_evaluations(), 0);
    assert_eq!(p.area, 0.0, "{p:?}");
    assert_eq!(p.volume, 0.0, "{p:?}");
}

/// A non-finite accuracy is refused, typed, before any integration: a NaN one passed
/// every quadrature piece's error test (`estimate > NaN` is false), so a trimmed face's
/// properties came back unchecked, and an infinite one holds no piece to any accuracy.
#[test]
fn a_non_finite_accuracy_is_refused() {
    let k = Kernel::default();
    let n = 2_usize;
    let surface = long_strip(n);
    let trims = strip_trims(n);
    let face_box = Extent3d::from_points(
        Point3::new(-1.0, -1.0, -1.0),
        Point3::new(2.0, n as f64 + 1.0, 1.0),
    );
    let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, n as f64);
    for accuracy in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let r = boundary_properties(
            &k,
            &surface,
            &trims,
            &domain,
            &face_box,
            OrientType::Opposite,
            accuracy,
            Vec3::new(0.3, -0.2, -1.0),
            0.5 * n as f64 / 10.0,
            0.0,
            PPF_ALL,
        );
        assert!(
            matches!(r, Err(solidcore::SolidError::InvalidInput { .. })),
            "accuracy {accuracy}: {r:?}"
        );
    }
    let mut k = Kernel::default();
    let mut b = solidcore::topology::brep::p3::new_brep(&mut k, tol::USE_DEFAULT, tol::USE_DEFAULT)
        .unwrap();
    let f = create_face_from_surface(&mut k, &mut b, &multispan_surface()).unwrap();
    for accuracy in [f64::NAN, f64::INFINITY] {
        let r = face_properties(
            &k,
            &mut b,
            f,
            OrientType::Opposite,
            accuracy,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            0.0,
            PPF_ALL,
        );
        assert!(
            matches!(r, Err(solidcore::SolidError::InvalidInput { .. })),
            "face_properties, accuracy {accuracy}: {r:?}"
        );
    }
}

/// A bicubic patch `[0, n]²` with `n` unit knot spans each way, wavy, as a G2 corner patch
/// is a dense spline grid in both directions.
fn dense_grid(n: usize) -> Surface {
    let cp: Vec<Vec<_>> = (0..n + 3)
        .map(|i| {
            (0..n + 3)
                .map(|j| {
                    let (x, y) = (i as f64, j as f64);
                    let z = 3.0 * (x / 8.0).sin() * (y / 10.0).cos() + 0.02 * x * y;
                    CPoint::new(x, y, z, 1.0)
                })
                .collect()
        })
        .collect();
    let mut kv = vec![0.0; 4];
    kv.extend((1..n).map(|j| j as f64));
    kv.extend([n as f64; 4]);
    Surface::from(
        BSplineSurface::from_data(n + 2, n + 2, 3, 3, n + 6, n + 6, &cp, &kv, &kv).unwrap(),
    )
}

/// The square `[m, n - m]²`, counterclockwise in uv.
fn grid_trims(n: usize, m: f64) -> Vec<Curve> {
    let (lo, hi) = (m, n as f64 - m);
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            solidcore::primitive::api_curves::create_line_segment(
                Point3::new(a.0, a.1, 0.0),
                Point3::new(b.0, b.1, 0.0),
            )
            .unwrap(),
        )
    };
    vec![
        line((lo, lo), (hi, lo)),
        line((hi, lo), (hi, hi)),
        line((hi, hi), (lo, hi)),
        line((lo, hi), (lo, lo)),
    ]
}

/// Every component over the square by tensor Gauss-Legendre with `g` points a knot span
/// (or partial span) each way.
fn grid_reference(
    k: &Kernel,
    surface: &Surface,
    n: usize,
    m: f64,
    origin: Vec3,
    layout: &PrecisePropertyLayout,
    g: usize,
) -> Vec<f64> {
    let gl = gauss_legendre(g);
    let mut total = vec![0.0; layout.num_components + 1];
    let mut dydx = vec![0.0; layout.num_components + 1];
    let span = |j: usize| ((j as f64).max(m), ((j + 1) as f64).min(n as f64 - m));
    for i in 0..n {
        let (u0, u1) = span(i);
        let hu = (u1 - u0) / 2.0;
        for &(xu, wu) in &gl {
            let eval =
                OdeIsoCurveLength::new(k, surface, u0 + hu * (xu + 1.0), origin, 0.0, layout);
            for j in 0..n {
                let (v0, v1) = span(j);
                let hv = (v1 - v0) / 2.0;
                for &(xv, wv) in &gl {
                    eval.evaluate(v0 + hv * (xv + 1.0), &mut dydx).unwrap();
                    for (t, d) in total.iter_mut().zip(dydx.iter()) {
                        *t += wu * hu * wv * hv * d;
                    }
                }
            }
        }
    }
    total
}

/// #14460: a G2 corner patch is a dense spline grid in both directions, so its banded
/// integral has a cell per band and u knot span, each integrated along u and, at each u
/// sample, along v. With nested Runge-Kutta steps (two of six samples each at the least)
/// this grid cost 826 iso-curve evaluations a cell at the app's accuracy 1e-8; with
/// nested 7-point Gauss-Kronrod (49 a cell when no piece is halved) it costs 321, and 258
/// with its trim pieces integrated the same way. Every
/// property still holds its accuracy against an independent tensor Gauss-Legendre
/// quadrature, more tightly than before.
#[test]
fn a_dense_grid_face_costs_few_evaluations_per_cell() {
    let k = Kernel::default();
    let origin = Vec3::new(0.3, -0.2, -1.0);
    let layout = PrecisePropertyLayout::new(PPF_ALL);
    let n = 24_usize;
    let m = 0.3;
    let surface = dense_grid(n);
    let reference = grid_reference(&k, &surface, n, m, origin, &layout, 8);
    let check = grid_reference(&k, &surface, n, m, origin, &layout, 12);
    for (r, c) in reference.iter().zip(check.iter()) {
        assert!((r - c).abs() <= 1e-13 * r.abs().max(1.0), "{r} vs {c}");
    }
    let trims = grid_trims(n, m);
    let face_box = Extent3d::from_points(
        Point3::new(-1.0, -1.0, -4.0),
        Point3::new(n as f64 + 1.0, n as f64 + 1.0, 4.0 + 0.02 * (n * n) as f64),
    );
    let domain = Extent2d::from_bounds(0.0, 0.0, n as f64, n as f64);
    // (`brep_properties`' estimate: the box's largest side squared over 100)
    let size = face_box.size();
    let d = size.z.max(size.x.max(size.y));
    let mut per_cell = Vec::new();
    for accuracy in [1e-8, 1e-11] {
        k.work_counters.reset();
        let p = boundary_properties(
            &k,
            &surface,
            &trims,
            &domain,
            &face_box,
            OrientType::Opposite,
            accuracy,
            origin,
            d * d / 100.0,
            0.0,
            PPF_ALL,
        )
        .unwrap();
        let evaluations = k.work_counters.mass_iso_evaluations();
        per_cell.push(evaluations as f64 / (n * n) as f64);
        let r = |i: usize| reference[i];
        let mut groups = vec![(vec![p.area], vec![r(layout.area)])];
        groups.push((vec![p.volume], vec![r(layout.volume)]));
        let moments = |m: &[Vec3], idx: &[usize]| {
            let got: Vec<f64> = m.iter().flat_map(|v| [v.x, v.y, v.z]).collect();
            let want: Vec<f64> = idx
                .iter()
                .flat_map(|&i| [r(i), r(i + 1), r(i + 2)])
                .collect();
            (got, want)
        };
        groups.push(moments(&p.moments[0..1], &layout.moments[0..1]));
        groups.push(moments(&p.moments[1..4], &layout.moments[1..4]));
        groups.push(moments(&p.moments[4..5], &layout.moments[4..5]));
        groups.push(moments(&p.moments[5..8], &layout.moments[5..8]));
        for (gi, (got, want)) in groups.iter().enumerate() {
            let size = want.iter().fold(0.0_f64, |m, w| m.max(w.abs()));
            let err = got
                .iter()
                .zip(want.iter())
                .fold(0.0_f64, |m, (g, w)| m.max((g - w).abs()))
                / size;
            eprintln!("accuracy {accuracy:e}, group {gi}: {err:e}");
            assert!(
                err <= accuracy,
                "accuracy {accuracy:e}, group {gi}: {err:e} off"
            );
        }
        eprintln!(
            "accuracy {accuracy:e}: {evaluations} iso evaluations, {:.1} a cell",
            per_cell.last().unwrap()
        );
    }
    assert!(per_cell[0] <= 450.0, "iso evaluations a cell {per_cell:?}");
}
