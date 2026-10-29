//! Runs one SSI corpus case through each engine and returns its [`RawOutput`]:
//!
//! * `cpp`: upstream SMLib through `solidcore_ffi::units::ssi_corpus` (shim
//!   `harness/cpp/shim/ssi_corpus.cpp`, which mirrors the corpus runner's `gsi` mode);
//! * `rust`: the same surfaces built with solidcore's own types and
//!   [`Surface::global_surface_intersect`], sampled the same way. Every
//!   [`SolidError::Unported`] on the way makes the case deferred for Rust.

use std::time::Instant;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::kernel::Kernel;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::ssi_corpus::{self as ffi, SsiSurface, SsiSurfaceIn};
use solidcore_ffi::FfiError;

use super::case::{SsiCase, SurfaceSpec};
use super::geom::P3;
use super::kernel::{deferred_output, rust_surface, write_intersection};
use super::raw::{type_name, RawCurve, RawOutput, N_SAMPLES, SM_SUCCESS};

/// `SM_ERR`.
const SM_ERR: i64 = 1001;
/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i64 = 1007;

pub use super::EngineFailure;

fn ffi_surface(spec: &SurfaceSpec, domain: Option<[f64; 4]>) -> SsiSurfaceIn {
    let surface = match spec.clone() {
        SurfaceSpec::Plane {
            origin,
            xaxis,
            yaxis,
            u,
            v,
        } => SsiSurface::Plane {
            origin,
            xaxis,
            yaxis,
            u,
            v,
        },
        SurfaceSpec::Cylinder {
            origin,
            xaxis,
            yaxis,
            radius,
            angle,
            height,
        } => SsiSurface::Cylinder {
            origin,
            xaxis,
            yaxis,
            radius,
            angle,
            height,
        },
        SurfaceSpec::Cone {
            apex,
            xaxis,
            yaxis,
            half_angle_deg,
            height,
        } => SsiSurface::Cone {
            apex,
            xaxis,
            yaxis,
            half_angle_deg,
            height,
        },
        SurfaceSpec::Sphere {
            center,
            xaxis,
            yaxis,
            radius,
            lon,
            lat,
        } => SsiSurface::Sphere {
            center,
            xaxis,
            yaxis,
            radius,
            lon,
            lat,
        },
        SurfaceSpec::Torus {
            center,
            xaxis,
            yaxis,
            major_radius,
            minor_radius,
            major,
            minor,
        } => SsiSurface::Torus {
            center,
            xaxis,
            yaxis,
            major_radius,
            minor_radius,
            major,
            minor,
        },
        SurfaceSpec::Nurbs {
            degree,
            knots_u,
            knots_v,
            nu,
            nv,
            points,
            weights,
        } => SsiSurface::Nurbs {
            degree,
            knots_u,
            knots_v,
            nu,
            nv,
            points,
            weights,
        },
    };
    SsiSurfaceIn { surface, domain }
}

/// Sequential reader over the shim's record.
struct Rec<'a> {
    v: std::slice::Iter<'a, f64>,
}

impl Rec<'_> {
    fn num(&mut self) -> Result<f64, String> {
        self.v
            .next()
            .copied()
            .ok_or_else(|| "ssi_corpus: short record".to_string())
    }
    fn count(&mut self) -> Result<usize, String> {
        let x = self.num()?;
        if x >= 0.0 && x.fract() == 0.0 && x < 1e9 {
            Ok(x as usize)
        } else {
            Err(format!("ssi_corpus: bad count {x}"))
        }
    }
    fn p3(&mut self) -> Result<P3, String> {
        Ok([self.num()?, self.num()?, self.num()?])
    }
    fn dom(&mut self) -> Result<[f64; 4], String> {
        Ok([self.num()?, self.num()?, self.num()?, self.num()?])
    }
}

/// Upstream `SmSurface::GlobalSurfaceIntersect` on the case (C++ via the shim).
pub fn run_cpp(case: &SsiCase, tol: Option<f64>) -> Result<RawOutput, EngineFailure> {
    let [(a, da), (b, db)] = &case.surfaces;
    let s1 = ffi_surface(a, *da);
    let s2 = ffi_surface(b, *db);
    let t0 = Instant::now();
    let rec = match ffi::intersect(&s1, &s2, tol, N_SAMPLES) {
        Ok(r) => r,
        Err(FfiError::Status(ffi::SC_ERR_BUILD)) => {
            return Err(EngineFailure::Build("surface build error".into()))
        }
        Err(e @ FfiError::InvalidArgument(_)) => return Err(EngineFailure::Build(e.to_string())),
        Err(e) => return Err(EngineFailure::Other(e.to_string())),
    };
    let time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    decode(case, tol, &rec, time_ms).map_err(EngineFailure::Other)
}

fn decode(
    case: &SsiCase,
    tol: Option<f64>,
    rec: &[f64],
    time_ms: f64,
) -> Result<RawOutput, String> {
    let mut r = Rec { v: rec.iter() };
    let status = r.num()? as i64;
    let domain1 = r.dom()?;
    let domain2 = r.dom()?;
    let n = r.count()?;
    let mut curves = Vec::new();
    for _ in 0..n {
        let ty = r.num()? as i64;
        let deviation = r.num()?;
        let has3 = r.num()? != 0.0;
        let (t0, t1) = (r.num()?, r.num()?);
        let class = r.num()? as i64;
        let nx = r.count()?;
        let xyz = (0..nx).map(|_| r.p3()).collect::<Result<Vec<_>, _>>()?;
        let mut uv = [Vec::new(), Vec::new()];
        let mut at = [Vec::new(), Vec::new()];
        for k in 0..2 {
            let _has_uv = r.num()?;
            let m = r.count()?;
            let pts = (0..m)
                .map(|_| Ok([r.num()?, r.num()?]))
                .collect::<Result<Vec<[f64; 2]>, String>>()?;
            let s = (0..m).map(|_| r.p3()).collect::<Result<Vec<_>, _>>()?;
            if let (Some(u), Some(a)) = (uv.get_mut(k), at.get_mut(k)) {
                *u = pts;
                *a = s;
            }
        }
        let [uv1, uv2] = uv;
        let [s1_at_uv, s2_at_uv] = at;
        curves.push(RawCurve {
            ty: type_name(ty).to_string(),
            deviation,
            interval: has3.then_some([t0, t1]),
            class_type: has3.then_some(class),
            xyz,
            uv1,
            s1_at_uv,
            uv2,
            s2_at_uv,
        });
    }
    let np = r.count()?;
    let points = (0..np).map(|_| r.p3()).collect::<Result<Vec<_>, _>>()?;
    if r.v.next().is_some() {
        return Err("ssi_corpus: trailing record data".into());
    }
    Ok(RawOutput {
        case: case.id.clone(),
        engine: "cpp".into(),
        status,
        status_name: None,
        domain1,
        domain2,
        approx_tol: tol,
        time_ms,
        curves,
        points,
        deferred: None,
    })
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

/// The ported `Surface::global_surface_intersect` on the case, sampled like the C++ side.
pub fn run_rust(case: &SsiCase, tol: Option<f64>) -> Result<RawOutput, EngineFailure> {
    let k = Kernel::default();
    let [(a, da), (b, db)] = &case.surfaces;
    let mut built = Vec::new();
    for (spec, dom) in [(a, da), (b, db)] {
        // reject exactly what the C++ side's encoder rejects
        ffi_surface(spec, *dom)
            .encode()
            .map_err(|e| EngineFailure::Build(e.to_string()))?;
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
