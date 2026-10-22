//! The record evaluator's second derivatives of B-spline and NURBS surfaces
//! (`SurfaceEval::second`) against finite differences of the surface itself, for degrees 2 to 5:
//! every curvature the kernel reads (continuity, the G2 fill's frames) comes from them (#763: a G2
//! boundary surface once measured half the cylinder's curvature it does carry).

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{Surface, eval::SurfaceEval};

/// A degree-`p` × degree-`q` surface on a net of `p + 3` × `q + 3` points with interior knots each
/// way, bent in z: its knots and points (`rows[i][j]`, i along u).
fn net(p: usize, q: usize) -> (Vec<f64>, Vec<f64>, Vec<Vec<[f64; 3]>>) {
    let knots = |d: usize, n: usize| {
        let inner: Vec<f64> = (1..n - d).map(|k| k as f64 / (n - d) as f64).collect();
        let mut k = vec![0.0; d + 1];
        k.extend(inner);
        k.extend(vec![1.0; d + 1]);
        k
    };
    let (nu, nv) = (p + 3, q + 3);
    let rows = (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    let (x, y) = (i as f64 * 2.0, j as f64 * 1.5);
                    [x + 0.3 * y, y, 0.4 * x * x - 0.2 * y * y + 0.1 * x * y]
                })
                .collect()
        })
        .collect();
    (knots(p, nu), knots(q, nv), rows)
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

/// The surface's second derivatives along u and v at each `(u, v)` against finite differences.
fn check(s: &SurfaceEval, at: &[(f64, f64)], what: &str) {
    for &(u, v) in at {
        let uu = second(|t| s.point([t, v]).unwrap(), u);
        let vv = second(|t| s.point([u, t]).unwrap(), v);
        let (suu, _, svv) = s.second(u, v).unwrap();
        let scale = uu.len().max(vv.len()).max(1.0);
        assert!(suu.dist(uu) < 1e-4 * scale, "{what}: S_uu at ({u}, {v}): {suu:?} vs {uu:?}");
        assert!(svv.dist(vv) < 1e-4 * scale, "{what}: S_vv at ({u}, {v}): {svv:?} vs {vv:?}");
    }
}

#[test]
fn bspline_surface_second_derivatives_match_finite_differences() {
    for p in 2..=5 {
        for q in 2..=5 {
            let (u_knots, v_knots, points) = net(p, q);
            let s = SurfaceEval::new(&Surface::Spline { u_knots, v_knots, points }).unwrap();
            check(&s, &[(0.0, 0.0), (0.37, 0.0), (0.5, 0.61), (1.0, 0.2), (0.13, 1.0)], &format!("degree ({p}, {q})"));
        }
    }
}

/// A rational surface's second derivatives come from its homogeneous B-spline's by the quotient
/// rule: they are right too.
#[test]
fn nurbs_surface_second_derivatives_match_finite_differences() {
    let (u_knots, v_knots, points) = net(3, 3);
    let homogeneous = points
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.iter()
                .enumerate()
                .map(|(j, p)| {
                    let w = 1.0 + 0.3 * ((i + 2 * j) % 3) as f64;
                    [p[0] * w, p[1] * w, p[2] * w, w]
                })
                .collect()
        })
        .collect();
    let s = SurfaceEval::new(&Surface::RationalSpline { u_knots, v_knots, homogeneous }).unwrap();
    check(&s, &[(0.0, 0.0), (0.37, 0.0), (0.5, 0.61), (1.0, 0.2)], "rational");
}
