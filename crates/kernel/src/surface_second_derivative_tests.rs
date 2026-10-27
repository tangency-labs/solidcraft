//! The vendored truck-geometry's second derivatives of B-spline and NURBS surfaces (`uuder`,
//! `vvder`) against finite differences of the surface itself, for degrees 2 to 5. Upstream's
//! were short of the factor p − 1, so every curvature read from a cubic surface came out half its
//! value (#763: a G2 boundary surface measured half the cylinder's curvature it does carry).
//! These pin the vendored fix, so a refresh of the crate can't quietly bring the error back.

use solidcraft_geom::Vec3;
use truck_modeling as mt;
use truck_modeling::ParametricSurface;

use crate::body::from_p3;

/// A degree-`p` × degree-`q` surface on a 5 × 6 net with an interior knot each way, bent in z.
fn surface(p: usize, q: usize) -> mt::BSplineSurface<mt::Point3> {
    let knots = |d: usize, n: usize| {
        let inner: Vec<f64> = (1..n - d).map(|k| k as f64 / (n - d) as f64).collect();
        let mut k = vec![0.0; d + 1];
        k.extend(inner);
        k.extend(vec![1.0; d + 1]);
        mt::KnotVec::from(k)
    };
    let (nu, nv) = (p + 3, q + 3);
    let rows: Vec<Vec<mt::Point3>> = (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    let (x, y) = (i as f64 * 2.0, j as f64 * 1.5);
                    mt::Point3::new(x + 0.3 * y, y, 0.4 * x * x - 0.2 * y * y + 0.1 * x * y)
                })
                .collect()
        })
        .collect();
    mt::BSplineSurface::new((knots(p, nu), knots(q, nv)), rows)
}

/// Second differences of `f` along the parameter line, central inside and one-sided at the ends.
fn second(f: impl Fn(f64) -> Vec3, t: f64) -> Vec3 {
    let h = 1e-4;
    if t < 2.0 * h {
        (f(t) * 2.0 - f(t + h) * 5.0 + f(t + 2.0 * h) * 4.0 - f(t + 3.0 * h)) / (h * h)
    } else if t > 1.0 - 2.0 * h {
        (f(t) * 2.0 - f(t - h) * 5.0 + f(t - 2.0 * h) * 4.0 - f(t - 3.0 * h)) / (h * h)
    } else {
        (f(t - h) - f(t) * 2.0 + f(t + h)) / (h * h)
    }
}

fn v3(x: mt::Vector3) -> Vec3 {
    Vec3::new(x.x, x.y, x.z)
}

#[test]
fn bspline_surface_second_derivatives_match_finite_differences() {
    for p in 2..=5 {
        for q in 2..=5 {
            let s = surface(p, q);
            for (u, v) in [(0.0, 0.0), (0.37, 0.0), (0.5, 0.61), (1.0, 0.2), (0.13, 1.0)] {
                let uu = second(|t| from_p3(s.subs(t, v)), u);
                let vv = second(|t| from_p3(s.subs(u, t)), v);
                let scale = uu.len().max(vv.len()).max(1.0);
                assert!(v3(s.uuder(u, v)).dist(uu) < 1e-4 * scale, "degree ({p}, {q}) uuder at ({u}, {v}): {:?} vs {uu:?}", s.uuder(u, v));
                assert!(v3(s.vvder(u, v)).dist(vv) < 1e-4 * scale, "degree ({p}, {q}) vvder at ({u}, {v}): {:?} vs {vv:?}", s.vvder(u, v));
            }
        }
    }
}

/// A rational surface's second derivatives come from its homogeneous B-spline's: they are right
/// too.
#[test]
fn nurbs_surface_second_derivatives_match_finite_differences() {
    let b = surface(3, 3);
    let (ku, kv) = b.knot_vecs().clone();
    let rows: Vec<Vec<mt::Vector4>> = b
        .control_points()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.iter()
                .enumerate()
                .map(|(j, p)| {
                    let w = 1.0 + 0.3 * ((i + 2 * j) % 3) as f64;
                    mt::Vector4::new(p.x * w, p.y * w, p.z * w, w)
                })
                .collect()
        })
        .collect();
    let s = mt::NurbsSurface::new(mt::BSplineSurface::new((ku, kv), rows));
    for (u, v) in [(0.0, 0.0), (0.37, 0.0), (0.5, 0.61), (1.0, 0.2)] {
        let uu = second(|t| from_p3(s.subs(t, v)), u);
        let vv = second(|t| from_p3(s.subs(u, t)), v);
        let scale = uu.len().max(vv.len()).max(1.0);
        assert!(v3(s.uuder(u, v)).dist(uu) < 1e-4 * scale, "uuder at ({u}, {v}): {:?} vs {uu:?}", s.uuder(u, v));
        assert!(v3(s.vvder(u, v)).dist(vv) < 1e-4 * scale, "vvder at ({u}, {v}): {:?} vs {vv:?}", s.vvder(u, v));
    }
}

/// Dense basis sums independently check that skipping exactly zero coefficients
/// preserves long curves and strips, their ends, extrapolation and all derivatives.
#[test]
fn compact_support_spline_evaluation_matches_dense_sums() {
    use mt::ParametricCurve;
    fn knots(count: usize) -> mt::KnotVec {
        let mut k = vec![0.0; 4];
        k.extend((1..count - 3).map(|i| i as f64 / (count - 3) as f64));
        k.extend([1.0; 4]);
        mt::KnotVec::from(k)
    }
    fn basis(k: &mt::KnotVec, degree: usize, t: f64, derivative: usize) -> Vec<f64> {
        if derivative == 0 {
            return k.bspline_basis_functions(degree, t);
        }
        let lower = basis(k, degree - 1, t, derivative - 1);
        (0..k.len() - degree - 1)
            .map(|i| {
                let a = k[i + degree] - k[i];
                let b = k[i + degree + 1] - k[i + 1];
                degree as f64 * (if a == 0.0 { 0.0 } else { lower[i] / a } - if b == 0.0 { 0.0 } else { lower[i + 1] / b })
            })
            .collect()
    }
    let (nu, nv) = (9, 385);
    let (ku, kv) = (knots(nu), knots(nv));
    let rows: Vec<Vec<mt::Point3>> = (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    let (x, y) = (i as f64 / (nu - 1) as f64, j as f64 / (nv - 1) as f64);
                    mt::Point3::new(x, 30.0 * y, (x * 2.0).sin() + (y * 3.0).cos() + x * y)
                })
                .collect()
        })
        .collect();
    let surface = mt::BSplineSurface::new((ku.clone(), kv.clone()), rows.clone());
    let curve = mt::BSplineCurve::new(kv.clone(), rows[3].clone());
    for (u, v) in [(-0.1, 0.23), (0.0, 0.0), (0.01, 0.87), (0.37, 0.5), (0.7, 0.001), (1.0, 1.0), (1.1, 0.83)] {
        for (du, dv) in [(0, 0), (1, 0), (0, 1), (2, 0), (0, 2), (1, 1)] {
            let (bu, bv) = (basis(&ku, 3, u, du), basis(&kv, 3, v, dv));
            let mut expected = Vec3::ZERO;
            for (i, row) in rows.iter().enumerate() {
                for (j, p) in row.iter().enumerate() {
                    expected += from_p3(*p) * (bu[i] * bv[j]);
                }
            }
            let actual = match (du, dv) {
                (0, 0) => from_p3(surface.subs(u, v)),
                (1, 0) => v3(surface.uder(u, v)),
                (0, 1) => v3(surface.vder(u, v)),
                (2, 0) => v3(surface.uuder(u, v)),
                (0, 2) => v3(surface.vvder(u, v)),
                _ => v3(surface.uvder(u, v)),
            };
            assert!(actual.dist(expected) < 1e-9 * expected.len().max(1.0), "({u},{v}) derivative ({du},{dv}): {actual:?} vs {expected:?}");
        }
        for d in 0..=2 {
            let weights = basis(&kv, 3, v, d);
            let expected = rows[3].iter().zip(weights).fold(Vec3::ZERO, |sum, (p, w)| sum + from_p3(*p) * w);
            let actual = match d {
                0 => from_p3(curve.subs(v)),
                1 => v3(curve.der(v)),
                _ => v3(curve.der2(v)),
            };
            assert!(actual.dist(expected) < 1e-9 * expected.len().max(1.0), "curve {v}, derivative {d}: {actual:?} vs {expected:?}");
        }
    }
}
