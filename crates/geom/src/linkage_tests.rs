use super::*;
use crate::brep::{Frame, Surface};

fn frame() -> Frame {
    Frame { origin: [1.0, -2.0, 0.5], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }
}

/// A sphere of radius `r` as a height over its tangent plane along the outward normal:
/// `H(x) = √(r² − |x|²) − r`, so `H₂(a) = −|a|²/r`, `H₃ = 0`, `H₄(a) = −3|a|⁴/r³`.
#[test]
fn a_sphere_is_a_height_whose_forms_are_its_radius_s() {
    let r = 7.0;
    let s = SurfaceEval::new(&Surface::Sphere { frame: frame(), radius: r }).unwrap();
    for uv in [[0.3, 0.2], [2.0, -0.9], [4.5, 1.1]] {
        let (_, su, sv) = s.derivs(uv[0], uv[1]).unwrap();
        let n = su.cross(sv).normalized().unwrap();
        let w = (su + sv * 0.5).normalized().unwrap();
        let jet = surface_jet(&s, uv, n, w, 4).unwrap();
        for (a, len2) in [([1.0, 0.0], 1.0), ([0.0, 1.0], 1.0), ([1.0, 1.0], 2.0), ([0.6, -0.8], 1.0)] {
            assert!((jet.form(2, a).unwrap() + len2 / r).abs() < 1e-12, "H2 at {uv:?} along {a:?}");
            assert!(jet.form(3, a).unwrap().abs() < 1e-11, "H3 at {uv:?} along {a:?}");
            assert!((jet.form(4, a).unwrap() + 3.0 * len2 * len2 / r.powi(3)).abs() < 1e-11, "H4 at {uv:?} along {a:?}");
        }
    }
}

/// A cylinder bends across its axis only: the height depends on the component round it, as the
/// circle's `√(r² − x²) − r` does; along its axis it is straight.
#[test]
fn a_cylinder_bends_round_its_axis_and_not_along_it() {
    let r = 4.0;
    let s = SurfaceEval::new(&Surface::Cylinder { frame: frame(), radius: r }).unwrap();
    let uv = [0.7, 3.0];
    let (_, su, sv) = s.derivs(uv[0], uv[1]).unwrap();
    let n = su.cross(sv).normalized().unwrap();
    // w along the axis: t = n × w runs round the cylinder.
    let jet = surface_jet(&s, uv, n, sv.normalized().unwrap(), 4).unwrap();
    let around = [1.0, 0.0];
    assert!((jet.form(2, around).unwrap().abs() - 1.0 / r).abs() < 1e-12);
    assert!((jet.form(4, around).unwrap().abs() - 3.0 / r.powi(3)).abs() < 1e-11);
    for m in 2..=4 {
        assert!(jet.form(m, [0.0, 1.0]).unwrap().abs() < 1e-12, "straight along the axis: H{m}");
    }
}

/// The same cylinder on its NURBS form (rational quadratic arcs, as SMLib holds it) has the same
/// jet: the height is the surface's, whatever its parameterisation.
#[test]
fn a_cylinder_and_its_nurbs_form_have_one_jet() {
    let label = Surface::Cylinder { frame: frame(), radius: 4.0 };
    let (form, _) = crate::brep::form::surface(&label, [[0.0, 2.0], [-1.0, 5.0]]).unwrap();
    let (a, b) = (SurfaceEval::new(&label).unwrap(), SurfaceEval::new(&form).unwrap());
    let p = a.point([0.9, 2.0]).unwrap();
    let uv = b.project(p, None, 1e-9).unwrap();
    let (_, su, sv) = a.derivs(0.9, 2.0).unwrap();
    let n = su.cross(sv).normalized().unwrap();
    let w = (su.normalized().unwrap() * 0.6 + sv * 0.8).normalized().unwrap();
    let (ja, jb) = (surface_jet(&a, [0.9, 2.0], n, w, 4).unwrap(), surface_jet(&b, uv, n, w, 4).unwrap());
    for m in 2..=4 {
        for x in [[1.0, 0.0], [0.0, 1.0], [0.3, 0.7]] {
            let (fa, fb) = (ja.form(m, x).unwrap(), jb.form(m, x).unwrap());
            assert!((fa - fb).abs() < 1e-9 * fa.abs().max(1.0), "H{m}({x:?}): {fa} vs {fb}");
        }
    }
}

/// The targets are the linkage conditions: order 2 the form in the curve's direction, order 3
/// `3·H₂(x₁, x₂) + H₃(x₁)`.
#[test]
fn the_targets_are_the_height_s_taylor_terms_along_the_curve() {
    let mut forms = [[0.0; MAX_CONTINUITY + 1]; MAX_CONTINUITY - 1];
    forms[0] = [2.0, 0.5, -1.0, 0.0, 0.0];
    forms[1] = [0.3, -0.2, 0.1, 0.7, 0.0];
    let jet = Jet { order: 3, forms };
    let (x1, x2) = ([0.8, 0.4], [0.1, -0.5]);
    let got = jet.targets(&[x1, x2]).unwrap();
    let h2 = |a: [f64; 2], b: [f64; 2]| 2.0 * a[0] * b[0] + 0.25 * (a[0] * b[1] + a[1] * b[0]) - a[1] * b[1];
    assert!((got[0] - h2(x1, x1)).abs() < 1e-14);
    assert!((got[1] - (3.0 * h2(x1, x2) + jet.form(3, x1).unwrap())).abs() < 1e-14);
    assert!(jet.targets(&[x1]).is_none(), "order 3 reads c″");
}

#[test]
fn hostile_orders_and_degenerate_points_are_refused() {
    let s = SurfaceEval::new(&Surface::Sphere { frame: frame(), radius: 2.0 }).unwrap();
    let (n, w) = (Vec3::Z, Vec3::X);
    assert!(surface_jet(&s, [0.0, 0.0], Vec3::X, Vec3::Y, 5).is_none());
    assert!(surface_jet(&s, [0.0, 0.0], Vec3::X, Vec3::Y, 1).is_none());
    // At the pole the latitude's parameterisation has no tangent plane.
    assert!(surface_jet(&s, [0.0, std::f64::consts::FRAC_PI_2], n, w, 2).is_none());
    assert!(surface_jet(&s, [f64::NAN, 0.0], n, w, 2).is_none());
    let jet = Jet { order: 2, forms: [[f64::NAN; MAX_CONTINUITY + 1]; MAX_CONTINUITY - 1] };
    assert!(!jet.is_finite());
    assert!(jet.form(3, [1.0, 0.0]).is_none(), "beyond its order");
}
