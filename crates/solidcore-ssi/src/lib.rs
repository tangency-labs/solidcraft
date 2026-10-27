//! solidcore's surfaces through the `ssi` crate's surface/surface intersection.
//!
//! The port's `Surface::global_surface_intersect` is SMLib's method, kept faithful for
//! parity. This crate offers `ssi` beside it, behind the same inputs and the same
//! output ([`SurfaceIntersection`]), so a caller can choose per call ([`Engine`]):
//!
//! * in: each surface over its domain becomes an `ssi` patch whose (u, v) are the
//!   surface's own, with its analytic surface when it is a plane, cylinder, cone,
//!   sphere or torus ([`patch`]);
//! * out: each `ssi` curve becomes a 3D curve and a UV curve on each surface, built
//!   together on one parameter from the intersection's own points, split at seams and
//!   poles, with touch points as the port marks them ([`curves`]); the singular points
//!   where branches cross or meet come beside them ([`Intersection::vertices`]), where
//!   a B-rep builder splits its edges.
//!
//! Nothing in `crates/solidcore` calls this yet: it is the seam for an opt-in SSI, to
//! be measured against the port on the corpus and on real models first.

// Comparisons are written `!(x > 0.0)` where NaN must fail the test, which
// `x <= 0.0` would pass (as in `ssi`).
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod curves;
pub mod patch;

use std::fmt;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use ssi::{densify, intersect_bodies, shapes, Options, Patch};

/// The most a filled-in curve turns between points (radians) when the caller gives no
/// angular tolerance, and the most it may turn when the caller gives a larger one: the
/// curves are cubic Hermite interpolants, whose error grows as the fourth power of
/// the turn.
pub const DENSIFY_TURN: f64 = 0.02;

/// Why the `ssi` path did not give an intersection.
#[derive(Debug)]
pub enum Error {
    /// A surface or domain this crate does not give `ssi` (yet).
    Unsupported(&'static str),
    /// solidcore failed: making a NURB, cutting a net, evaluating a surface, making a
    /// curve.
    Kernel(SolidError),
    /// `ssi` refused (a contact it has not built, named) or failed.
    Ssi(ssi::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(what) => write!(f, "not given to ssi: {what}"),
            Error::Kernel(e) => write!(f, "{e}"),
            Error::Ssi(e) => write!(f, "{} ({e})", e.code()),
        }
    }
}

impl std::error::Error for Error {}

impl From<SolidError> for Error {
    fn from(e: SolidError) -> Self {
        Error::Kernel(e)
    }
}

impl From<ssi::Error> for Error {
    fn from(e: ssi::Error) -> Self {
        Error::Ssi(e)
    }
}

/// The intersection curves in the port's form, and the singular points on them.
#[derive(Debug, Clone, Default)]
pub struct Intersection {
    /// The curves, as the port's `Surface::global_surface_intersect` returns them.
    pub curves: SurfaceIntersection,
    /// Where branches cross or meet (`ssi`'s vertices). The curves pass through them, as
    /// the corpus's branches do; a B-rep builder splits its edges there (upstream issue
    /// #393). Empty from the port, which does not find them.
    pub vertices: Vec<Vec3>,
}

/// Which surface/surface intersection runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Engine {
    /// The port's `Surface::global_surface_intersect` (SMLib's method).
    #[default]
    Port,
    /// `ssi` alone: a refusal or an unsupported surface is an error.
    Ssi,
    /// `ssi`, and the port wherever `ssi` gives no intersection.
    SsiThenPort,
}

/// The absolute distance within which `ssi` takes two points as one, for `options`
/// on these patches (as `ssi` scales it: its options are fractions of the bodies'
/// size, floored at the accuracy of a grazing point).
fn link(options: &Options, patches: [&Patch; 2]) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for pole in patches.iter().flat_map(|p| &p.poles) {
        for ((l, h), &x) in lo.iter_mut().zip(&mut hi).zip(pole) {
            *l = l.min(x);
            *h = h.max(x);
        }
    }
    let size = match hi
        .iter()
        .zip(&lo)
        .map(|(h, l)| (h - l) * (h - l))
        .sum::<f64>()
        .sqrt()
    {
        s if s > 0.0 => s,
        _ => 1.0,
    };
    let magnitude = lo.iter().chain(&hi).fold(0.0_f64, |m, x| m.max(x.abs()));
    (options.link * size).max(8.0 * (f64::EPSILON * magnitude).sqrt())
}

/// The intersection curves of `surface1` over `domain1` with `surface2` over `domain2`
/// by `ssi`, in the port's form: `approx_tol_3d` is the smallest feature to resolve
/// (`ssi`'s default when `None`) and `ang_tol_rad` the most a curve turns between its
/// points (at most [`DENSIFY_TURN`]).
///
/// # Errors
///
/// [`Error::Unsupported`] for a surface kind or domain not given to `ssi`;
/// [`Error::Ssi`] where `ssi` refuses (a tangency it has not built, by name) or fails;
/// [`Error::Kernel`] where solidcore fails on the way in or out.
pub fn global_surface_intersect(
    k: &Kernel,
    surface1: &Surface,
    domain1: &Extent2d,
    surface2: &Surface,
    domain2: &Extent2d,
    approx_tol_3d: Option<f64>,
    ang_tol_rad: Option<f64>,
) -> Result<Intersection, Error> {
    let a = patch::patch(k, surface1, domain1)?;
    let b = patch::patch(k, surface2, domain2)?;
    let options = approx_tol_3d.map_or_else(Options::default, |t| shapes::resolving(t, &[&a, &b]));
    let (body_a, body_b) = (std::slice::from_ref(&a), std::slice::from_ref(&b));
    let mut found = intersect_bodies(body_a, body_b, &options)?;
    let turn = ang_tol_rad.map_or(DENSIFY_TURN, |t| t.min(DENSIFY_TURN));
    densify(&mut found, body_a, body_b, turn)?;
    let link = link(&options, [&a, &b]);
    // the curves are refined to the caller's tolerance, else to ten times the link
    let tol = approx_tol_3d.unwrap_or(10.0 * link);
    Ok(Intersection {
        curves: curves::build(k, [surface1, surface2], [&a, &b], &found, link, tol)?,
        vertices: found
            .vertices
            .iter()
            .map(|&[x, y, z]| Vec3::new(x, y, z))
            .collect(),
    })
}

/// The intersection curves of `surface1` over `domain1` with `surface2` over `domain2`
/// by `engine`, with the port's arguments (`use_surface_edges` is the port's alone).
///
/// # Errors
///
/// Those of the port's `Surface::global_surface_intersect` (as [`Error::Kernel`]) or of
/// [`global_surface_intersect`]; for [`Engine::SsiThenPort`], the port's.
#[allow(clippy::too_many_arguments)]
pub fn intersect(
    engine: Engine,
    k: &Kernel,
    surface1: &Surface,
    domain1: &Extent2d,
    surface2: &Surface,
    domain2: &Extent2d,
    use_surface_edges: [bool; 2],
    approx_tol_3d: Option<f64>,
    ang_tol_rad: Option<f64>,
) -> Result<Intersection, Error> {
    let port = || {
        surface1
            .global_surface_intersect(
                k,
                domain1,
                surface2,
                domain2,
                use_surface_edges,
                approx_tol_3d,
                ang_tol_rad,
            )
            .map(|curves| Intersection {
                curves,
                vertices: Vec::new(),
            })
            .map_err(Error::Kernel)
    };
    match engine {
        Engine::Port => port(),
        Engine::Ssi => global_surface_intersect(
            k,
            surface1,
            domain1,
            surface2,
            domain2,
            approx_tol_3d,
            ang_tol_rad,
        ),
        Engine::SsiThenPort => global_surface_intersect(
            k,
            surface1,
            domain1,
            surface2,
            domain2,
            approx_tol_3d,
            ang_tol_rad,
        )
        .or_else(|_| port()),
    }
}

#[cfg(test)]
mod tests {
    use solidcore::curve::curve::Curve;
    use solidcore::geom::vector2d::{Point2, Vec2};
    use solidcore::geom::vector3d::Vec3;
    use solidcore::surface::cone::Cone;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::sphere::{Sphere, SphereOptions};
    use solidcore::surface::surface::p2::TsectCurveType;

    use super::*;

    fn plane(origin: Vec3, x: Vec3, y: Vec3) -> Surface {
        let mut p = Plane::new(
            origin,
            x,
            y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-3.0, -3.0, 3.0, 3.0),
        );
        p.make_nurb().unwrap();
        Surface::from(p)
    }

    fn sphere(k: &Kernel, r: f64) -> Surface {
        let mut s = Sphere::new(
            k,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            r,
            SphereOptions::default(),
        )
        .unwrap();
        s.make_nurb(k).unwrap();
        Surface::from(s)
    }

    fn cylinder(k: &Kernel, r: f64) -> Surface {
        let mut c = Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            r,
            0.0,
            360.0,
            4.0,
            false,
            false,
            false,
        );
        c.make_nurb(k).unwrap();
        Surface::from(c)
    }

    fn run(k: &Kernel, s1: &Surface, s2: &Surface) -> SurfaceIntersection {
        global_surface_intersect(
            k,
            s1,
            &s1.natural_uv_domain_if_built(),
            s2,
            &s2.natural_uv_domain_if_built(),
            Some(1e-6),
            None,
        )
        .unwrap()
        .curves
    }

    /// Samples a curve at `n` + 1 parameters over its interval.
    fn samples(k: &Kernel, c: &Curve, n: u32) -> Vec<Vec3> {
        let ivl = c.natural_interval();
        (0..=n)
            .map(|i| {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / f64::from(n);
                c.evaluate_point(k, t).unwrap()
            })
            .collect()
    }

    /// Every curve's 3D samples, and each surface at its UV samples, lie on the true
    /// intersection within `tol`; the deviation reported is that small too.
    fn check(
        k: &Kernel,
        x: &SurfaceIntersection,
        s: [&Surface; 2],
        on: impl Fn(Vec3) -> f64,
        tol: f64,
    ) {
        for (i, c3) in x.curves_3d.iter().enumerate() {
            for p in samples(k, c3, 400) {
                assert!(on(p) < tol, "curve {i}: 3D point {:e} off", on(p));
            }
            for (uvs, surface) in [&x.surface1_uv_curves, &x.surface2_uv_curves].iter().zip(s) {
                let uv = uvs[i].as_ref().unwrap();
                let a = samples(k, c3, 400);
                let b = samples(k, uv, 400);
                for (p, q) in a.iter().zip(&b) {
                    let on_surface = surface.evaluate_point(k, Point2::new(q.x, q.y)).unwrap();
                    let e = (*p - on_surface).length();
                    assert!(e < tol, "curve {i}: UV curve {e:e} from the 3D curve");
                }
            }
            assert!(
                x.deviations[i] < tol,
                "curve {i}: deviation {:e}",
                x.deviations[i]
            );
        }
    }

    #[test]
    fn plane_cuts_a_sphere_in_one_circle() {
        let k = Kernel::default();
        let (s, p) = (
            sphere(&k, 1.0),
            plane(
                Vec3::new(0.0, 0.0, 0.5),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ),
        );
        let x = run(&k, &p, &s);
        assert!(!x.curves_3d.is_empty());
        assert!(x.curve_types.iter().all(|&t| t == TsectCurveType::Crossing));
        let r = 0.75f64.sqrt();
        check(
            &k,
            &x,
            [&p, &s],
            |q| {
                ((q.x * q.x + q.y * q.y).sqrt() - r)
                    .abs()
                    .max((q.z - 0.5).abs())
            },
            1e-7,
        );
    }

    /// A tilted plane cuts the cylinder in an ellipse that crosses the seam: each UV
    /// curve on the cylinder stays in its domain.
    #[test]
    fn an_ellipse_across_the_seam() {
        let k = Kernel::default();
        let c = cylinder(&k, 1.0);
        let n = Vec3::new(0.3, 0.1, 1.0);
        let p = plane(
            Vec3::new(0.0, 0.0, 0.2),
            Vec3::new(1.0, 0.0, -0.3),
            Vec3::new(0.0, 1.0, -0.1),
        );
        let x = run(&k, &c, &p);
        assert!(!x.curves_3d.is_empty());
        let d = c.natural_uv_domain_if_built();
        for uv in x.surface1_uv_curves.iter().flatten() {
            for q in samples(&k, uv, 200) {
                assert!(
                    q.x >= d.min().x - 1e-9 && q.x <= d.max().x + 1e-9,
                    "u {} outside the domain",
                    q.x
                );
            }
        }
        let on = |q: Vec3| {
            let radial = ((q.x * q.x + q.y * q.y).sqrt() - 1.0).abs();
            let planar = ((q.x * n.x + q.y * n.y + (q.z - 0.2) * n.z) / n.length()).abs();
            radial.max(planar)
        };
        check(&k, &x, [&c, &p], on, 1e-7);
    }

    /// A plane through the sphere's poles cuts it in a great circle that `ssi` steps
    /// across each pole: the curves are split there, and each UV curve stays on its
    /// meridian and in its domain.
    #[test]
    fn a_great_circle_through_the_poles() {
        let k = Kernel::default();
        let s = sphere(&k, 1.0);
        let p = plane(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let x = run(&k, &p, &s);
        assert!(x.curves_3d.len() >= 2, "{} curves", x.curves_3d.len());
        let d = s.natural_uv_domain_if_built();
        for uv in x.surface2_uv_curves.iter().flatten() {
            for q in samples(&k, uv, 400) {
                assert!(
                    q.x >= d.min().x && q.x <= d.max().x && q.y >= d.min().y && q.y <= d.max().y,
                    "UV ({}, {}) outside the domain",
                    q.x,
                    q.y
                );
            }
        }
        let on = |q: Vec3| q.x.abs().max(((q.y * q.y + q.z * q.z).sqrt() - 1.0).abs());
        check(&k, &x, [&p, &s], on, 1e-6);
    }

    /// A plane touching a sphere gives one touch point, marked as the port marks it.
    #[test]
    fn a_plane_touching_a_sphere() {
        let k = Kernel::default();
        let (s, p) = (
            sphere(&k, 1.0),
            plane(
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ),
        );
        let x = run(&k, &p, &s);
        assert_eq!(x.curve_types, vec![TsectCurveType::Touching], "{x:?}");
        let q = samples(&k, &x.curves_3d[0], 1)[0];
        assert!((q - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-7, "{q:?}");
    }

    /// The engines answer through one entry point, the port's when `ssi` gives none.
    #[test]
    fn engines_share_one_entry_point() {
        let k = Kernel::default();
        let (s, p) = (
            sphere(&k, 1.0),
            plane(
                Vec3::new(0.0, 0.0, 0.5),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ),
        );
        let (d1, d2) = (
            p.natural_uv_domain_if_built(),
            s.natural_uv_domain_if_built(),
        );
        for engine in [Engine::Ssi, Engine::SsiThenPort] {
            let x =
                intersect(engine, &k, &p, &d1, &s, &d2, [true, true], Some(1e-6), None).unwrap();
            assert!(!x.curves.curves_3d.is_empty(), "{engine:?}");
        }
    }
}
