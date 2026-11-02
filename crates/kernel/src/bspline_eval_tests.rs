//! truck-geometry's B-spline surface point and derivatives sum only the control points whose
//! basis functions are non-zero (vendor/README.md). They must still equal the full sums: the point
//! against every control point weighted by its basis functions, the derivatives against the
//! derived surfaces.
#![allow(clippy::unwrap_used, clippy::panic)]
use truck_modeling::*;

#[test]
fn surface_point_and_derivatives_skip_zero_basis_functions() {
    let uknot = KnotVec::from(vec![0.0, 0.0, 0.0, 0.0, 0.1, 0.25, 0.3, 0.5, 0.7, 0.75, 0.9, 1.0, 1.0, 1.0, 1.0]);
    let vknot = KnotVec::from(vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.45, 0.6, 1.0, 1.0, 1.0, 1.0]);
    let pts: Vec<Vec<Point3>> = (0..11)
        .map(|i| {
            (0..7)
                .map(|j| {
                    let (x, y) = (f64::from(i), f64::from(j));
                    Point3::new(x + 0.1 * y, y - 0.2 * x, (0.7 * x).sin() * (1.3 * y).cos())
                })
                .collect()
        })
        .collect();
    let s = BSplineSurface::new((uknot.clone(), vknot.clone()), pts.clone());
    let (du, dv) = (s.uderivation(), s.vderivation());
    let (duu, dvv, duv) = (du.uderivation(), dv.vderivation(), du.vderivation());
    let near = |a: Vector3, b: Vector3, what: &str, u: f64, v: f64| {
        assert!((a - b).magnitude() <= 1e-9 * (1.0 + b.magnitude()), "{what} at ({u}, {v}): {a:?} vs {b:?}");
    };
    for k in 0..=40 {
        for l in 0..=40 {
            let (u, v) = (f64::from(k) / 40.0, f64::from(l) / 40.0);
            let (b0, b1) = (uknot.bspline_basis_functions(3, u), vknot.bspline_basis_functions(3, v));
            let mut full = Vector3::new(0.0, 0.0, 0.0);
            for (row, a) in pts.iter().zip(&b0) {
                for (p, b) in row.iter().zip(&b1) {
                    full += p.to_vec() * (a * b);
                }
            }
            near(s.subs(u, v).to_vec(), full, "point", u, v);
            near(s.uder(u, v), du.subs(u, v), "uder", u, v);
            near(s.vder(u, v), dv.subs(u, v), "vder", u, v);
            near(s.uuder(u, v), duu.subs(u, v), "uuder", u, v);
            near(s.vvder(u, v), dvv.subs(u, v), "vvder", u, v);
            near(s.uvder(u, v), duv.subs(u, v), "uvder", u, v);
        }
    }
}
