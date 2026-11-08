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
use solidcore::geom::tol;
use solidcore::mass::integrator::properties::{brep_volume, face_properties, PropertyAttributes};
use solidcore::mass::integrator::{OdeIntegrator, OdeOptions, StepCounts};
use solidcore::math::Vec3;
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
