//! The `solidcore-ssi` engine: the case's surfaces built with solidcore's own types, as
//! the `rust` engine builds them ([`rust_surface`]), intersected by the `solidcore-ssi`
//! crate (the `ssi` crate behind the port's inputs and output), and written as the
//! `rust` engine writes the port's output ([`write_intersection`]). So it grades the
//! path a kernel caller would take, the adapters into and out of `ssi` included, by the
//! port's own sampling: its curves are B-spline curves sampled at 4001 parameters, not
//! the marched points the `ssi` engine writes. Its points are the singular points the
//! crate returns beside the curves (the `ssi` engine writes the same). The timer covers
//! the whole call (patches in, `ssi`, filling in, curves out). No C++ is involved.

use std::time::Instant;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::kernel::Kernel;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ssi::Error;

use super::case::SsiCase;
use super::kernel::{deferred_output, rust_surface, write_intersection};
use super::raw::{RawOutput, SM_SUCCESS};
use super::EngineFailure;

/// The engine's name in reports.
pub const ENGINE: &str = "solidcore-ssi";

/// `SM_ERR`: the status of a refusal or failure, whose code and detail are in
/// `status_name`.
const SM_ERR: i64 = 1001;

/// The `solidcore-ssi` crate's intersection of the case's two surfaces. A refusal (a
/// contact `ssi` has not built, `BOOLEAN_TANGENT`), a surface it does not take, or a
/// failure is status `SM_ERR` with its code and detail as `status_name`; an unported
/// step on the way defers the case, as for the port.
pub fn run_solidcore_ssi(case: &SsiCase, tol: Option<f64>) -> Result<RawOutput, EngineFailure> {
    let k = Kernel::default();
    let [(a, da), (b, db)] = &case.surfaces;
    let mut built = Vec::new();
    for (spec, dom) in [(a, da), (b, db)] {
        match rust_surface(&k, spec).map_err(EngineFailure::Build)? {
            Ok(s) => {
                let d = match dom {
                    Some(d) => Extent2d::from_bounds(d[0], d[2], d[1], d[3]),
                    None => s.natural_uv_domain(),
                };
                built.push((s, d));
            }
            Err(d) => return Ok(deferred_output(case, tol, ENGINE, &d.unit, &d.symbol)),
        }
    }
    let [(s1, d1), (s2, d2)] = <[(Surface, Extent2d); 2]>::try_from(built)
        .map_err(|_| EngineFailure::Other("two surfaces expected".into()))?;
    let dom4 = |d: &Extent2d| [d.min().x, d.max().x, d.min().y, d.max().y];
    let approx = tol.map(|t| approx_tol3d(&k, t));
    let t0 = Instant::now();
    let res = solidcore_ssi::global_surface_intersect(&k, &s1, &d1, &s2, &d2, approx, None);
    let time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut out = RawOutput {
        case: case.id.clone(),
        engine: ENGINE.into(),
        status: SM_SUCCESS,
        domain1: dom4(&d1),
        domain2: dom4(&d2),
        approx_tol: tol,
        time_ms,
        ..RawOutput::default()
    };
    let x = match res {
        Ok(x) => x,
        Err(Error::Kernel(SolidError::Unported { upstream, unit })) => {
            return Ok(deferred_output(case, tol, ENGINE, unit, upstream))
        }
        Err(e) => {
            out.status = SM_ERR;
            out.status_name = Some(match &e {
                Error::Ssi(_) => e.to_string(),
                Error::Unsupported(_) => format!("UNSUPPORTED ({e})"),
                Error::Kernel(_) => format!("SM_ERR ({e})"),
            });
            return Ok(out);
        }
    };
    // the singular points on the curves, as the `ssi` engine writes them
    out.points = x.vertices.iter().map(|v| [v.x, v.y, v.z]).collect();
    match write_intersection(&k, &x.curves, [&s1, &s2], &mut out)? {
        Ok(()) => Ok(out),
        Err(d) => Ok(deferred_output(case, tol, ENGINE, &d.unit, &d.symbol)),
    }
}
