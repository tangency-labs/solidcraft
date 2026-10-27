//! solidcore surfaces as `ssi` patches.
//!
//! A surface goes to `ssi` as its own NURB's net, so a patch's (u, v) are the surface's
//! (u, v): solidcore evaluates its analytic surfaces at their NURB's parameters too
//! (`Cone::evaluate_point` converts them to STEP parameters first). A plane, cylinder,
//! cone, sphere or torus also carries its analytic surface, whose implicit equation
//! `ssi` traces the other surface against; it is attached only when the net's points
//! lie on it. A face's domain inside the surface's is cut out of the net by knot
//! insertion (`split_srf`), which keeps the parameters.

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::nurbs_srf::split_srf;
use solidcore::nurbs::srf_basic::{Direction, Surface as NurbSurface};
use solidcore::surface::surface::{Surface, SurfaceKind};
use ssi::geom3::{Frame, V3};
use ssi::surface3::Surf;
use ssi::Patch;

use crate::Error;

/// How far, as a fraction of the net's size, the net's points may lie from the
/// analytic surface it is given with.
const ON_ANALYTIC: f64 = 1e-9;

/// Samples per direction when checking the analytic surface against the net.
const CHECK_SAMPLES: u32 = 7;

/// A domain edge this close to the natural one (as a fraction of the domain's width)
/// is the natural one: the net is not cut there.
const SAME_EDGE: f64 = 1e-12;

/// The NURB of a surface, made on a copy when the surface has none yet.
fn nurb(k: &Kernel, surface: &Surface) -> Result<NurbSurface, Error> {
    let made = match surface.kind() {
        SurfaceKind::BSpline(s) => s.gw_nurb().cloned(),
        SurfaceKind::Plane(p) => p.base_with_nurb()?.gw_nurb().cloned(),
        SurfaceKind::Cone(c) => {
            if let Some(n) = c.rev().base().gw_nurb() {
                Some(n.clone())
            } else {
                let mut c = c.as_ref().clone();
                c.make_nurb(k)?;
                c.rev().base().gw_nurb().cloned()
            }
        }
        SurfaceKind::Sphere(s) => {
            if let Some(n) = s.base().base().gw_nurb() {
                Some(n.clone())
            } else {
                let mut s = s.copy();
                s.make_nurb(k)?;
                s.base().base().gw_nurb().cloned()
            }
        }
        SurfaceKind::Torus(t) => {
            if let Some(n) = t.base().base().gw_nurb() {
                Some(n.clone())
            } else {
                let mut t = t.copy();
                t.make_nurb(k)?;
                t.base().base().gw_nurb().cloned()
            }
        }
        SurfaceKind::SurfOfRevolution(s) => {
            if let Some(n) = s.base().gw_nurb() {
                Some(n.clone())
            } else {
                let mut s = s.copy();
                s.make_nurb(k)?;
                s.base().gw_nurb().cloned()
            }
        }
        SurfaceKind::SurfOfExtrusion(s) => s.base().gw_nurb().cloned(),
        SurfaceKind::CurveBounded(_) => return Err(Error::Unsupported("curve-bounded surface")),
        SurfaceKind::Step(_) => return Err(Error::Unsupported("STEP surface")),
        SurfaceKind::OffsetSurface(_) => return Err(Error::Unsupported("offset surface")),
        SurfaceKind::SrfInVolume(_) => return Err(Error::Unsupported("surface in a volume")),
    };
    made.ok_or(Error::Unsupported("a surface without its NURB"))
}

/// The net cut down to `domain` (clamped to the net's own domain) by knot insertion.
fn restrict(net: NurbSurface, domain: &Extent2d) -> Result<NurbSurface, Error> {
    let natural = net.parameter_bounds();
    let (lo, hi) = (domain.min(), domain.max());
    let u = [lo.x.max(natural.ul), hi.x.min(natural.ur)];
    let v = [lo.y.max(natural.vb), hi.y.min(natural.vt)];
    if !(u[0] < u[1] && v[0] < v[1]) {
        return Err(Error::Unsupported("a domain outside its surface"));
    }
    let mut net = net;
    for (dir, [a, b], [n0, n1]) in [
        (Direction::U, u, [natural.ul, natural.ur]),
        (Direction::V, v, [natural.vb, natural.vt]),
    ] {
        let eps = SAME_EDGE * (n1 - n0);
        if a > n0 + eps {
            net = split_srf(&net, a, dir)?.1;
        }
        if b < n1 - eps {
            net = split_srf(&net, b, dir)?.0;
        }
    }
    Ok(net)
}

/// The net as an `ssi` patch: Cartesian poles, weights when rational, full knots.
fn patch_of(net: &NurbSurface) -> Result<Patch, Error> {
    let rational = net.is_rational();
    let (u_degree, v_degree) = net.degrees();
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for row in net.rows() {
        for c in row {
            let w = if rational && c.w != NOW { c.w } else { 1.0 };
            if !(w > 0.0) {
                return Err(Error::Unsupported(
                    "a net with a weight that is not positive",
                ));
            }
            poles.push([c.x / w, c.y / w, c.z / w]);
            weights.push(w);
        }
    }
    Ok(Patch {
        u_degree,
        v_degree,
        u_count: net.n() + 1,
        v_count: net.m() + 1,
        poles,
        weights: rational.then_some(weights),
        u_knots: net.u_knots().to_vec(),
        v_knots: net.v_knots().to_vec(),
        analytic: None,
    })
}

fn v3(p: Vec3) -> V3 {
    V3::new(p.x, p.y, p.z)
}

/// The analytic surface of a plane, cylinder, cone, sphere or torus.
fn analytic(surface: &Surface) -> Option<Surf> {
    let frame = |o: Vec3, x: Vec3, z: Vec3| Frame {
        o: v3(o),
        z: v3(z),
        x: v3(x),
    };
    Some(match surface.kind() {
        SurfaceKind::Plane(p) => {
            let at = p.position();
            Surf::Plane {
                f: frame(at.origin(), at.x_axis(), at.z_axis()),
            }
        }
        SurfaceKind::Cone(c) => {
            let at = c.rev().position();
            let f = frame(at.origin(), at.x_axis(), at.z_axis());
            if c.is_cylinder() {
                Surf::Cylinder {
                    f,
                    r: c.base_radius(),
                }
            } else {
                Surf::Cone {
                    f,
                    r: c.base_radius(),
                    a: c.semi_angle_deg().to_radians(),
                }
            }
        }
        SurfaceKind::Sphere(s) => {
            let at = s.base().position();
            Surf::Sphere {
                f: frame(at.origin(), at.x_axis(), at.z_axis()),
                r: s.radius(),
            }
        }
        SurfaceKind::Torus(t) => {
            let at = t.base().position();
            Surf::Torus {
                f: frame(at.origin(), at.x_axis(), at.z_axis()),
                major: t.major_radius(),
                minor: t.minor_radius(),
            }
        }
        _ => return None,
    })
}

/// Whether the patch's points lie on `surf`, at a grid of parameters over its domain.
fn lies_on(patch: &Patch, surf: &Surf) -> bool {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for pole in &patch.poles {
        for ((l, h), &x) in lo.iter_mut().zip(&mut hi).zip(pole) {
            *l = l.min(x);
            *h = h.max(x);
        }
    }
    let size = hi
        .iter()
        .zip(&lo)
        .map(|(h, l)| (h - l) * (h - l))
        .sum::<f64>()
        .sqrt();
    let Some([u0, u1, v0, v1]) = patch.domain() else {
        return false;
    };
    let n = f64::from(CHECK_SAMPLES - 1);
    (0..CHECK_SAMPLES).all(|i| {
        (0..CHECK_SAMPLES).all(|j| {
            let (s, t) = (f64::from(i) / n, f64::from(j) / n);
            patch
                .evaluate(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t)
                .is_some_and(|[x, y, z]| surf.dist(V3::new(x, y, z)) <= ON_ANALYTIC * size)
        })
    })
}

/// `surface` over `domain` as an `ssi` patch whose (u, v) are the surface's own: its
/// NURB's net cut down to the domain, with the analytic surface when it has one.
///
/// # Errors
///
/// [`Error::Unsupported`] for a kind with no NURB to give (curve-bounded, STEP,
/// offset, in a volume) or a domain outside the surface; [`Error::Kernel`] when
/// making the NURB or cutting the net fails.
pub fn patch(k: &Kernel, surface: &Surface, domain: &Extent2d) -> Result<Patch, Error> {
    let net = restrict(nurb(k, surface)?, domain)?;
    let mut patch = patch_of(&net)?;
    patch.analytic = analytic(surface).filter(|surf| lies_on(&patch, surf));
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use solidcore::geom::vector2d::{Point2, Vec2};
    use solidcore::surface::cone::Cone;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::sphere::{Sphere, SphereOptions};
    use solidcore::surface::torus::{Torus, TorusOptions};

    use super::*;

    fn x() -> Vec3 {
        Vec3::new(1.0, 0.0, 0.0)
    }

    fn y() -> Vec3 {
        Vec3::new(0.0, 1.0, 0.0)
    }

    fn surfaces(k: &Kernel) -> Vec<(&'static str, Surface)> {
        let o = Vec3::new(0.3, -0.2, 0.1);
        let mut plane = Plane::new(
            o,
            x(),
            y(),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-2.0, -1.0, 2.0, 3.0),
        );
        plane.make_nurb().unwrap();
        let mut cyl =
            Cone::new_cylinder_bounded(o, x(), y(), 1.5, 0.0, 360.0, 2.0, false, false, false);
        cyl.make_nurb(k).unwrap();
        let mut sphere = Sphere::new(
            k,
            o,
            x(),
            y(),
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            1.25,
            SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(k).unwrap();
        let mut torus = Torus::new(
            k,
            o,
            x(),
            y(),
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            2.0,
            0.5,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(k).unwrap();
        vec![
            ("plane", Surface::from(plane)),
            ("cylinder", Surface::from(cyl)),
            ("sphere", Surface::from(sphere)),
            ("torus", Surface::from(torus)),
        ]
    }

    /// The patch and the surface are one map from (u, v) to space, and the
    /// analytic surface is attached.
    #[test]
    fn patch_parameters_are_the_surfaces() {
        let k = Kernel::default();
        for (name, s) in surfaces(&k) {
            let d = s.natural_uv_domain_if_built();
            let p = patch(&k, &s, &d).unwrap();
            assert!(
                p.analytic.is_some(),
                "{name}: analytic surface not attached"
            );
            let (lo, hi) = (d.min(), d.max());
            for i in 0..=8 {
                for j in 0..=8 {
                    let u = lo.x + (hi.x - lo.x) * f64::from(i) / 8.0;
                    let v = lo.y + (hi.y - lo.y) * f64::from(j) / 8.0;
                    let a = p.evaluate(u, v).unwrap();
                    let b = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
                    let e =
                        ((a[0] - b.x).powi(2) + (a[1] - b.y).powi(2) + (a[2] - b.z).powi(2)).sqrt();
                    assert!(e < 1e-12, "{name} at ({u}, {v}): {e:e} apart");
                }
            }
        }
    }

    /// A face domain inside the surface's cuts the net and keeps its parameters.
    #[test]
    fn a_face_domain_keeps_the_parameters() {
        let k = Kernel::default();
        for (name, s) in surfaces(&k) {
            let d = s.natural_uv_domain_if_built();
            let (lo, hi) = (d.min(), d.max());
            let (w, h) = (hi.x - lo.x, hi.y - lo.y);
            let inner = Extent2d::from_bounds(
                lo.x + 0.2 * w,
                lo.y + 0.3 * h,
                lo.x + 0.7 * w,
                lo.y + 0.9 * h,
            );
            let p = patch(&k, &s, &inner).unwrap();
            let got = p.domain().unwrap();
            let want = [
                lo.x + 0.2 * w,
                lo.x + 0.7 * w,
                lo.y + 0.3 * h,
                lo.y + 0.9 * h,
            ];
            for (g, e) in got.iter().zip(want) {
                assert!(
                    (g - e).abs() < 1e-12 * (w + h),
                    "{name}: domain {got:?}, want {want:?}"
                );
            }
            let (u, v) = (lo.x + 0.45 * w, lo.y + 0.6 * h);
            let a = p.evaluate(u, v).unwrap();
            let b = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
            let e = ((a[0] - b.x).powi(2) + (a[1] - b.y).powi(2) + (a[2] - b.z).powi(2)).sqrt();
            assert!(e < 1e-12, "{name}: {e:e} apart");
        }
    }
}
