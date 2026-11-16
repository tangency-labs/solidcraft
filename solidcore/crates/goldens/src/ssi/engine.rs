//! Runs one SSI corpus case through the Rust port and returns its [`RawOutput`]: the
//! case's surfaces built with solidcore's own types and intersected by
//! [`Surface::global_surface_intersect`], every curve sampled at
//! [`N_SAMPLES`](super::raw::N_SAMPLES) parameters.
//! Every [`SolidError::Unported`] on the way makes the case deferred.

use std::time::Instant;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::kernel::Kernel;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;

use super::case::{SsiCase, SurfaceSpec};
use super::kernel::{deferred_output, rust_surface, write_intersection};
use super::raw::{RawOutput, SM_SUCCESS};

/// `SM_ERR`.
const SM_ERR: i64 = 1001;
/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i64 = 1007;
/// Upstream `NL_MAXDEG`.
const MAX_DEGREE: usize = 17;
/// Largest control net accepted (`nu * nv`).
const MAX_NET: usize = 1 << 22;

pub use super::EngineFailure;

fn finite<'a>(v: impl IntoIterator<Item = &'a f64>) -> Result<(), String> {
    if v.into_iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err("ssi_corpus: non-finite value".into())
    }
}

/// The input contract of a case's surface (ported from the reference's corpus wrapper):
/// every number finite; for a NURBS surface degrees in `1..=NL_MAXDEG`, at least `p + 1`
/// by `q + 1` control points and at most [`MAX_NET`], `nu * nv` points and weights,
/// `n + p + 1` knots per direction, knots non-decreasing and weights positive.
fn check_input(spec: &SurfaceSpec, domain: Option<[f64; 4]>) -> Result<(), String> {
    let bad = |m: &str| Err(format!("ssi_corpus: {m}"));
    match spec {
        SurfaceSpec::Plane {
            origin,
            xaxis,
            yaxis,
            u,
            v,
        } => finite(origin.iter().chain(xaxis).chain(yaxis).chain(u).chain(v))?,
        SurfaceSpec::Cylinder {
            origin,
            xaxis,
            yaxis,
            radius,
            angle,
            height,
        } => finite(
            origin
                .iter()
                .chain(xaxis)
                .chain(yaxis)
                .chain([radius])
                .chain(angle)
                .chain(height),
        )?,
        SurfaceSpec::Cone {
            apex,
            xaxis,
            yaxis,
            half_angle_deg,
            height,
        } => finite(
            apex.iter()
                .chain(xaxis)
                .chain(yaxis)
                .chain([half_angle_deg])
                .chain(height),
        )?,
        SurfaceSpec::Sphere {
            center,
            xaxis,
            yaxis,
            radius,
            lon,
            lat,
        } => finite(
            center
                .iter()
                .chain(xaxis)
                .chain(yaxis)
                .chain([radius])
                .chain(lon)
                .chain(lat),
        )?,
        SurfaceSpec::Torus {
            center,
            xaxis,
            yaxis,
            major_radius,
            minor_radius,
            major,
            minor,
        } => finite(
            center
                .iter()
                .chain(xaxis)
                .chain(yaxis)
                .chain([major_radius, minor_radius])
                .chain(major)
                .chain(minor),
        )?,
        SurfaceSpec::Nurbs {
            degree,
            knots_u,
            knots_v,
            nu,
            nv,
            points,
            weights,
        } => {
            let [p, q] = *degree;
            if !(1..=MAX_DEGREE).contains(&p) || !(1..=MAX_DEGREE).contains(&q) {
                return bad("degree outside 1..=NL_MAXDEG");
            }
            if *nu < p + 1 || *nv < q + 1 {
                return bad("too few control points for the degree");
            }
            let Some(n) = nu.checked_mul(*nv) else {
                return bad("net too large");
            };
            if n > MAX_NET || points.len() != n || weights.len() != n {
                return bad("net size does not match nu * nv");
            }
            if knots_u.len() != nu + p + 1 || knots_v.len() != nv + q + 1 {
                return bad("knot count does not match n + p + 1");
            }
            finite(knots_u.iter().chain(knots_v))?;
            if knots_u
                .windows(2)
                .chain(knots_v.windows(2))
                .any(|w| matches!(w, [a, b] if a > b))
            {
                return bad("decreasing knots");
            }
            if weights.iter().any(|w| !(w.is_finite() && *w > 0.0)) {
                return bad("non-positive weight");
            }
            finite(points.iter().flatten())?;
        }
    }
    finite(domain.iter().flatten())
}

fn rust_status(e: &SolidError) -> (i64, Option<String>) {
    match e {
        SolidError::Upstream { code, .. } => (i64::from(*code), None),
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => (
            SM_ERR_INVALID_INPUT,
            Some(format!("SM_ERR_INVALID_INPUT ({e})")),
        ),
        _ => (SM_ERR, Some(format!("SM_ERR ({e})"))),
    }
}

/// The ported `Surface::global_surface_intersect` on the case, every curve sampled at
/// [`N_SAMPLES`](super::raw::N_SAMPLES) parameters.
pub fn run_rust(case: &SsiCase, tol: Option<f64>) -> Result<RawOutput, EngineFailure> {
    let k = Kernel::default();
    let [(a, da), (b, db)] = &case.surfaces;
    let mut built = Vec::new();
    for (spec, dom) in [(a, da), (b, db)] {
        check_input(spec, *dom).map_err(EngineFailure::Build)?;
        match rust_surface(&k, spec).map_err(EngineFailure::Build)? {
            Ok(s) => {
                let d = match dom {
                    Some(d) => Extent2d::from_bounds(d[0], d[2], d[1], d[3]),
                    None => s
                        .natural_uv_domain_with_nurb(&k)
                        .map_err(|e| EngineFailure::Build(e.to_string()))?,
                };
                built.push((s, d));
            }
            Err(d) => return Ok(deferred_output(case, tol, "rust", &d.unit, &d.symbol)),
        }
    }
    let [(s1, d1), (s2, d2)] = <[(Surface, Extent2d); 2]>::try_from(built)
        .map_err(|_| EngineFailure::Other("two surfaces expected".into()))?;
    let dom4 = |d: &Extent2d| [d.min().x, d.max().x, d.min().y, d.max().y];
    let approx = tol.map(|t| approx_tol3d(&k, t));
    let t0 = Instant::now();
    let res = s1.global_surface_intersect(&k, &d1, &s2, &d2, [true, true], approx, None);
    let time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut out = RawOutput {
        case: case.id.clone(),
        engine: "rust".into(),
        status: SM_SUCCESS,
        domain1: dom4(&d1),
        domain2: dom4(&d2),
        approx_tol: tol,
        time_ms,
        ..RawOutput::default()
    };
    let x = match res {
        Ok(x) => x,
        Err(SolidError::Unported { upstream, unit }) => {
            return Ok(deferred_output(case, tol, "rust", unit, upstream))
        }
        Err(e) => {
            let (status, name) = rust_status(&e);
            out.status = status;
            out.status_name = name;
            return Ok(out);
        }
    };
    match write_intersection(&k, &x, [&s1, &s2], &mut out)? {
        Ok(()) => Ok(out),
        Err(d) => Ok(deferred_output(case, tol, "rust", &d.unit, &d.symbol)),
    }
}
