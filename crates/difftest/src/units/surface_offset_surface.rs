//! Runner for unit `surface.offset_surface`. C++: `solidcore_ffi::units::surface_offset_surface`
//! (shim `harness/cpp/shim/surface_offset_surface.cpp`). Rust: `solidcore::surface::offset_surface`.
//!
//! A case builds an `SmOffsetSurface` over a base surface (a B-spline, an upstream `.sms`
//! file, a plane, a curve-bounded or STEP surface, a surface of revolution, or a nested
//! offset) and runs a script of ops on that one object, so that the extended surface
//! upstream's `const` evaluators store is seen by the later ops (the Rust side runs the
//! storing `*_extending` evaluators; an op marked `via` runs the `&self` evaluators through
//! the `Surface` interface instead and is only generated where no extension can be
//! stored). A `read_db` case reads the C++ `WriteToDB` record of an offset on both sides.
//!
//! Output `out`: the construction status, then each op's status and outputs (see the
//! FFI module); a failure is the upstream `SmStatus` (`InvalidInput` is
//! `SM_ERR_INVALID_INPUT`, the zero-vector status of `Unitize`). `Unported` on the Rust
//! side defers the case.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::OFFSET_SURFACE_TYPE;
use solidcore::surface::surface::{StepSurface, Surface, SurfaceKind};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::SolidError;
use solidcore_ffi::units::surface_offset_surface as ffi;

use super::surface_surf_of_revolution_p1 as rev;
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `surface.offset_surface` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Bspline(BssIn),
    /// An upstream `.sms` file (absolute path after fixture expansion).
    Sms(String),
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    Bounded {
        basis: Box<SrfIn>,
        implicit: bool,
    },
    Step {
        base: Box<SrfIn>,
    },
    Revolution(rev::SurfIn),
    Offset(Box<OffIn>),
}

const fn yes() -> bool {
    true
}

const fn unknown() -> u32 {
    SS_UNKNOWN
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffIn {
    d: f64,
    #[serde(default = "yes")]
    owns: bool,
    #[serde(default = "unknown")]
    sing: u32,
    #[serde(default)]
    allow: bool,
    base: SrfIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum StepIn {
    State,
    Eval {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default = "yes")]
        ul: bool,
        #[serde(default = "yes")]
        vl: bool,
        #[serde(default)]
        upper: bool,
        #[serde(default = "yes")]
        nzt: bool,
        #[serde(default)]
        via: bool,
    },
    EvalSimple {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default = "yes")]
        ul: bool,
        #[serde(default = "yes")]
        vl: bool,
        #[serde(default)]
        upper: bool,
        #[serde(default)]
        via: bool,
    },
    EvalPt {
        uv: [f64; 2],
        #[serde(default)]
        via: bool,
    },
    Normal {
        uv: [f64; 2],
        #[serde(default = "yes")]
        ul: bool,
        #[serde(default = "yes")]
        vl: bool,
        #[serde(default)]
        via: bool,
    },
    Knots {
        param: u8,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    MaxAnal,
    #[serde(rename = "bbox")]
    BBox {
        dom: [f64; 4],
        want: [bool; 3],
        #[serde(default = "yes")]
        expand: bool,
    },
    StepInv {
        dom: [f64; 4],
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        guess: Option<[f64; 2]>,
    },
    Reverse {
        param: u8,
    },
    Swap,
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    Trim {
        dom: [f64; 4],
    },
    Split {
        t: f64,
        param: u8,
    },
    CreateOffset {
        d: f64,
    },
    Extend {
        dist: f64,
        cont: u8,
    },
    ExtendDir {
        param: u8,
        dist: f64,
        cont: u8,
    },
    Copy,
    Eq {
        other: OffIn,
    },
    SetDist {
        d: f64,
    },
    SetAllow {
        b: bool,
    },
    IsPlanar {
        tol: f64,
    },
    IsKindOf {
        t: i64,
    },
    BaseOffset {
        d: f64,
        tol: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inputs {
    Script {
        off: OffIn,
        steps: Vec<StepIn>,
    },
    /// `bytes` absent in a fixture: filled with the C++ `WriteToDB` record of `off`.
    ReadDb {
        off: OffIn,
        binary: bool,
        version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<Vec<u8>>,
    },
}

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| harness(format!("{path}: {e}")))
        })
        .collect()
}

fn count(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)` points
/// (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots, `m+q+2` v
/// knots.
fn read_sms(path: &str) -> Result<BssIn, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
    let n = count(path, num()?)?;
    let m = count(path, num()?)?;
    let p = count(path, num()?)?;
    let q = count(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BssIn {
        oob: false,
        form: default_form(),
        net,
        p,
        q,
        u,
        v,
    })
}

// ---- C++ side -------------------------------------------------------------------------------

fn ffi_srf(s: &SrfIn) -> Result<ffi::Srf, HarnessError> {
    Ok(match s {
        SrfIn::Bspline(b) => ffi::Srf::BSpline(ffi_bss(b)),
        SrfIn::Sms(path) => ffi::Srf::BSpline(ffi_bss(&read_sms(path)?)),
        SrfIn::Plane { o, n } => ffi::Srf::Plane { o: *o, n: *n },
        SrfIn::Bounded { basis, implicit } => ffi::Srf::Bounded {
            basis: Box::new(ffi_srf(basis)?),
            implicit: *implicit,
        },
        SrfIn::Step { base } => ffi::Srf::Step {
            base: Box::new(ffi_srf(base)?),
        },
        SrfIn::Revolution(r) => ffi::Srf::Revolution(rev::ffi_surf(r)?),
        SrfIn::Offset(o) => ffi::Srf::Offset(Box::new(ffi_off(o)?)),
    })
}

fn ffi_bss(b: &BssIn) -> ffi::Bss {
    ffi::Bss {
        oob: b.oob,
        form: b.form,
        net: b.net.clone(),
        p: b.p,
        q: b.q,
        u: b.u.clone(),
        v: b.v.clone(),
    }
}

fn ffi_off(o: &OffIn) -> Result<ffi::Off, HarnessError> {
    Ok(ffi::Off {
        d: o.d,
        owns: o.owns,
        sing: o.sing,
        allow: o.allow,
        base: ffi_srf(&o.base)?,
    })
}

fn ffi_step(s: &StepIn) -> Result<ffi::Step, HarnessError> {
    use ffi::Step as F;
    Ok(match s {
        StepIn::State => F::State,
        StepIn::Eval {
            uv,
            hu,
            hv,
            ul,
            vl,
            upper,
            nzt,
            ..
        } => F::Eval {
            uv: *uv,
            hu: *hu,
            hv: *hv,
            ul: *ul,
            vl: *vl,
            upper: *upper,
            nzt: *nzt,
        },
        StepIn::EvalSimple {
            uv,
            hu,
            hv,
            ul,
            vl,
            upper,
            ..
        } => F::EvalSimple {
            uv: *uv,
            hu: *hu,
            hv: *hv,
            ul: *ul,
            vl: *vl,
            upper: *upper,
        },
        StepIn::EvalPt { uv, .. } => F::EvalPt { uv: *uv },
        StepIn::Normal { uv, ul, vl, .. } => F::Normal {
            uv: *uv,
            ul: *ul,
            vl: *vl,
        },
        StepIn::Knots { param, ivl } => F::Knots {
            param: *param,
            ivl: *ivl,
        },
        StepIn::MaxAnal => F::MaxAnal,
        StepIn::BBox { dom, want, expand } => F::BBox {
            dom: *dom,
            want: *want,
            expand: *expand,
        },
        StepIn::StepInv { dom, p, tol, guess } => F::StepInv {
            dom: *dom,
            p: *p,
            tol: *tol,
            guess: *guess,
        },
        StepIn::Reverse { param } => F::Reverse { param: *param },
        StepIn::Swap => F::Swap,
        StepIn::Transform {
            origin,
            x,
            y,
            scale,
        } => F::Transform {
            origin: *origin,
            x: *x,
            y: *y,
            scale: *scale,
        },
        StepIn::Trim { dom } => F::Trim { dom: *dom },
        StepIn::Split { t, param } => F::Split {
            t: *t,
            param: *param,
        },
        StepIn::CreateOffset { d } => F::CreateOffset { d: *d },
        StepIn::Extend { dist, cont } => F::Extend {
            dist: *dist,
            cont: *cont,
        },
        StepIn::ExtendDir { param, dist, cont } => F::ExtendDir {
            param: *param,
            dist: *dist,
            cont: *cont,
        },
        StepIn::Copy => F::Copy,
        StepIn::Eq { other } => F::Eq {
            other: Box::new(ffi_off(other)?),
        },
        StepIn::SetDist { d } => F::SetDist { d: *d },
        StepIn::SetAllow { b } => F::SetAllow { b: *b },
        StepIn::IsPlanar { tol } => F::IsPlanar { tol: *tol },
        StepIn::IsKindOf { t } => F::IsKindOf { t: *t },
        StepIn::BaseOffset { d, tol } => F::BaseOffset { d: *d, tol: *tol },
    })
}

fn ffi_steps(steps: &[StepIn]) -> Result<Vec<ffi::Step>, HarnessError> {
    steps.iter().map(ffi_step).collect()
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let rec = match i {
        Inputs::Script { off, steps } => ffi::script(&ffi_off(off)?, &ffi_steps(steps)?),
        Inputs::ReadDb {
            off,
            binary,
            version,
            bytes,
        } => {
            let bytes = bytes.as_ref().ok_or_else(|| harness("read_db: no bytes"))?;
            ffi::read_db(&ffi_off(off)?, *binary, *version, bytes)
        }
    }
    .map_err(|e| harness(e.to_string()))?;
    Ok(Output::new().values("out", rec))
}

// ---- Rust side ------------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    SurfParamType::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad param {v}")))
}

fn continuity_of(v: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match v {
        0 => C::Undefined,
        1 => C::Discontinuous,
        2 => C::C0,
        3 => C::G1,
        4 => C::G1R,
        5 => C::G1G2,
        6 => C::G1G2G3,
        7 => C::C1,
        8 => C::C1G2,
        9 => C::C1G2G3,
        10 => C::C1C2,
        11 => C::C1C2G3,
        12 => C::C1C2C3,
        13 => C::CInfinity,
        _ => return Err(harness(format!("bad continuity {v}"))),
    })
}

/// The `SmStatus` of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        // SM_ERR_INVALID_INPUT (the zero-vector status of Unitize)
        SolidError::InvalidInput { .. } => 1007.0,
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, else a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

fn defer(e: &SolidError) -> Option<Fail> {
    Output::unported(e).map(|o| Fail::Deferred(Box::new(o)))
}

/// Pushes `0` and the outputs of `v` on success, the status on failure.
fn push_res<T>(
    o: &mut Vec<f64>,
    r: solidcore::Result<T>,
    f: impl FnOnce(&mut Vec<f64>, T) -> Result<(), Fail>,
) -> Result<(), Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            f(o, v)
        }
        Err(e) => {
            if let Some(d) = defer(&e) {
                Err(d)
            } else {
                o.push(status(&e));
                Ok(())
            }
        }
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn bss_of(b: &BssIn) -> Result<BSplineSurface, HarnessError> {
    let net = b
        .net
        .iter()
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
        .collect();
    let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    let mut s = BSplineSurface::from_nurb(nl, false);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

/// Builds a base; `Ok(Err(e))` when a construction fails (as upstream's does).
fn rust_srf(k: &Kernel, s: &SrfIn) -> Result<solidcore::Result<Surface>, HarnessError> {
    Ok(match s {
        SrfIn::Bspline(b) => Ok(Surface::from(bss_of(b)?)),
        SrfIn::Sms(path) => Ok(Surface::from(bss_of(&read_sms(path)?)?)),
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Bounded { basis, implicit } => rust_srf(k, basis)?.map(|b| {
            Surface::from(CurveBoundedSurface::new(
                b,
                Vec::new(),
                Vec::new(),
                *implicit,
            ))
        }),
        SrfIn::Step { base } => {
            rust_srf(k, base)?.map(|b| Surface::from(StepSurface::new(b, true)))
        }
        SrfIn::Revolution(r) => rev::rust_surf(k, r)?.map(Surface::from),
        SrfIn::Offset(o) => rust_off(k, o)?.map(Surface::from),
    })
}

/// Builds an offset (`SmOffsetSurface(d, base, owns, sing)` then `SetAllowExtension`).
fn rust_off(k: &Kernel, o: &OffIn) -> Result<solidcore::Result<OffsetSurface>, HarnessError> {
    Ok(match rust_srf(k, &o.base)? {
        Ok(base) => OffsetSurface::new(k, o.d, base, o.owns, o.sing).map(|mut s| {
            s.set_allow_extension(o.allow);
            s
        }),
        Err(e) => Err(e),
    })
}

/// The 3x3 `EvaluatePoint` grid over the natural domain.
fn put_grid(o: &mut Vec<f64>, s: &Surface) -> Result<(), Fail> {
    let d = s.natural_uv_domain();
    for i in 0..3 {
        for j in 0..3 {
            let uv = d.evaluate(f64::from(i) / 2.0, f64::from(j) / 2.0);
            push_res(o, s.evaluate_point(uv), |o, p| {
                pv(o, p);
                Ok(())
            })?;
        }
    }
    Ok(())
}

fn put_state(o: &mut Vec<f64>, s: &OffsetSurface) -> Result<(), Fail> {
    put_dom(o, &s.natural_uv_domain());
    o.push(s.offset_distance());
    o.push(flag(s.allow_extension()));
    o.push(flag(s.owns_surface()));
    let [pu, pv_] = s.is_periodic();
    o.push(f64::from(pu));
    o.push(f64::from(pv_));
    o.push(f64::from(s.singularities()));
    match s.extended_base_surface() {
        Some(e) => {
            o.push(1.0);
            put_dom(o, &e.natural_uv_domain());
            o.push(e.type_code() as f64);
        }
        None => o.push(0.0),
    }
    let b = s.base_surface();
    o.push(b.type_code() as f64);
    put_dom(o, &b.natural_uv_domain());
    o.push(s.type_code() as f64);
    put_grid(o, &Surface::from(s.clone()))
}

fn put_result(o: &mut Vec<f64>, r: &Surface) -> Result<(), Fail> {
    o.push(r.type_code() as f64);
    if let SurfaceKind::OffsetSurface(off) = r.kind() {
        put_state(o, off)
    } else {
        put_dom(o, &r.natural_uv_domain());
        put_grid(o, r)
    }
}

/// The entries upstream writes: all, or with `bOnlyUpperHalf` those with `i + j` up to
/// the larger order (the others are untouched caller memory).
fn put_matrix(
    o: &mut Vec<f64>,
    m: &[Vec<Vec3>],
    hu: usize,
    hv: usize,
    upper: bool,
) -> Result<(), Fail> {
    for i in 0..=hu {
        for j in 0..=hv {
            if !upper || i + j <= hu.max(hv) {
                let v = m
                    .get(i)
                    .and_then(|r| r.get(j))
                    .copied()
                    .ok_or_else(|| harness("evaluation matrix too small"))?;
                pv(o, v);
            }
        }
    }
    Ok(())
}

fn flags([ul, vl, upper, nzt]: [bool; 4]) -> EvaluateFlags {
    EvaluateFlags {
        u_side: side(ul),
        v_side: side(vl),
        only_upper_half: upper,
        non_zero_tangents: nzt,
    }
}

fn run_step(
    k: &Kernel,
    s: &mut OffsetSurface,
    step: &StepIn,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match step {
        StepIn::State => {
            o.push(0.0);
            put_state(o, s)
        }
        StepIn::Eval {
            uv,
            hu,
            hv,
            ul,
            vl,
            upper,
            nzt,
            via,
        } => {
            let f = flags([*ul, *vl, *upper, *nzt]);
            let r = if *via {
                Surface::from(s.clone()).evaluate(p2(*uv), *hu, *hv, f)
            } else {
                s.evaluate_extending(p2(*uv), *hu, *hv, f)
            };
            push_res(o, r, |o, m| put_matrix(o, &m, *hu, *hv, *upper))
        }
        StepIn::EvalSimple {
            uv,
            hu,
            hv,
            ul,
            vl,
            upper,
            via,
        } => {
            let f = flags([*ul, *vl, *upper, false]);
            let r = if *via {
                Surface::from(s.clone()).evaluate_simple(p2(*uv), *hu, *hv, f)
            } else {
                s.evaluate_simple_extending(p2(*uv), *hu, *hv, f)
            };
            push_res(o, r, |o, m| put_matrix(o, &m, *hu, *hv, *upper))
        }
        StepIn::EvalPt { uv, via } => {
            let r = if *via {
                Surface::from(s.clone()).evaluate_point(p2(*uv))
            } else {
                s.evaluate_point_extending(p2(*uv))
            };
            push_res(o, r, |o, p| {
                pv(o, p);
                Ok(())
            })
        }
        StepIn::Normal { uv, ul, vl, via } => {
            let r = if *via {
                Surface::from(s.clone()).evaluate_normal(p2(*uv), side(*ul), side(*vl))
            } else {
                s.evaluate_normal_extending(p2(*uv), side(*ul), side(*vl))
            };
            push_res(o, r, |o, n| {
                pv(o, n);
                Ok(())
            })
        }
        StepIn::Knots { param, ivl } => {
            let ivl = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            push_res(
                o,
                s.knots(param_of(*param)?, ivl.as_ref()),
                |o, (knots, mults)| {
                    o.push(knots.len() as f64);
                    o.extend(knots);
                    o.push(mults.len() as f64);
                    o.extend(mults.into_iter().map(|m| m as f64));
                    Ok(())
                },
            )
        }
        StepIn::MaxAnal => push_res(o, s.max_analytic_domain(), |o, d| {
            put_dom(o, &d);
            Ok(())
        }),
        StepIn::BBox { dom, want, expand } => {
            let w = Surface::from(s.clone());
            push_res(
                o,
                w.calculate_bounding_box(k, &dom_of(*dom), *want, *expand),
                |o, b| {
                    if want[0] {
                        let nb = b.normal_box.ok_or_else(|| harness("no normal box"))?;
                        pv(o, nb.min());
                        pv(o, nb.max());
                    }
                    if want[1] {
                        let pb = b.pseudo_box.ok_or_else(|| harness("no pseudo box"))?;
                        for v in pb.basis() {
                            pv(o, *v);
                        }
                        for i in pb.intervals() {
                            o.extend([i.min(), i.max()]);
                        }
                    }
                    if want[2] {
                        let qb = b.polar_box.ok_or_else(|| harness("no polar box"))?;
                        o.push(f64::from(qb.state().code()));
                        for v in qb.basis() {
                            pv(o, *v);
                        }
                        put_dom(o, &qb.domain());
                    }
                    Ok(())
                },
            )
        }
        StepIn::StepInv { dom, p, tol, guess } => push_res(
            o,
            s.step_inversion_extending(k, &dom_of(*dom), v3(*p), *tol, guess.map(p2)),
            |o, (uv, loc)| {
                o.extend([uv.x, uv.y, f64::from(loc as i32)]);
                Ok(())
            },
        ),
        StepIn::Reverse { param } => {
            let p = param_of(*param)?;
            push_res(o, s.reverse(k, p), |_, ()| Ok(()))
        }
        StepIn::Swap => push_res(o, s.swap_uv(k), |_, ()| Ok(())),
        StepIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            push_res(o, s.transform(k, &ax, scale.map(v3)), |_, ()| Ok(()))
        }
        StepIn::Trim { dom } => {
            let mut d = dom_of(*dom);
            let r = s.trim_with_domain(k, &mut d);
            push_res(o, r, |o, ()| {
                put_dom(o, &d);
                Ok(())
            })
        }
        StepIn::Split { t, param } => {
            let p = param_of(*param)?;
            push_res(o, s.split_at(k, *t, p), |o, (l, r)| {
                put_result(o, &l)?;
                put_result(o, &r)
            })
        }
        StepIn::CreateOffset { d } => {
            push_res(o, s.create_offset_surface(k, *d), |o, r| put_result(o, &r))
        }
        StepIn::Extend { dist, cont } => {
            let c = continuity_of(*cont)?;
            push_res(o, s.create_extended_surface(k, *dist, c), |o, r| {
                put_result(o, &r)
            })
        }
        StepIn::ExtendDir { param, dist, cont } => {
            let (p, c) = (param_of(*param)?, continuity_of(*cont)?);
            push_res(o, s.create_extended_surface_dir(k, p, *dist, c), |o, r| {
                put_result(o, &r)
            })
        }
        StepIn::Copy => push_res(o, s.copy(k), |o, c| put_result(o, &Surface::from(c))),
        StepIn::Eq { other } => match rust_off(k, other)? {
            Ok(other) => {
                o.push(0.0);
                let eq = s.approx_eq(&Surface::from(other));
                let w = Surface::from(s.clone());
                let self_eq = w.approx_eq(&w);
                for r in [eq, self_eq] {
                    match r {
                        Ok(b) => o.push(flag(b)),
                        Err(e) => {
                            return Err(defer(&e).unwrap_or_else(|| {
                                Fail::Harness(harness(format!("operator== failed: {e}")))
                            }))
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                if let Some(d) = defer(&e) {
                    Err(d)
                } else {
                    o.push(status(&e));
                    Ok(())
                }
            }
        },
        StepIn::SetDist { d } => {
            s.set_offset_distance(*d);
            o.push(0.0);
            Ok(())
        }
        StepIn::SetAllow { b } => {
            s.set_allow_extension(*b);
            o.push(0.0);
            Ok(())
        }
        StepIn::IsPlanar { tol } => {
            let w = Surface::from(s.clone());
            match w.is_planar(k, *tol) {
                Ok(pl) => {
                    o.extend([0.0, flag(pl.is_planar)]);
                    pv(o, pl.point);
                    pv(o, pl.normal);
                    o.push(pl.deviation);
                    Ok(())
                }
                Err(e) => Err(defer(&e)
                    .unwrap_or_else(|| Fail::Harness(harness(format!("IsPlanar failed: {e}"))))),
            }
        }
        StepIn::IsKindOf { t } => {
            o.extend([0.0, flag(OffsetSurface::is_kind_of(*t))]);
            Ok(())
        }
        StepIn::BaseOffset { d, tol } => push_res(
            o,
            s.base_surface().create_offset_surface(k, *d, *tol),
            |o, r| put_result(o, &r),
        ),
    }
}

fn rust_script(off: &OffIn, steps: &[StepIn]) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let mut s = match rust_off(&k, off)? {
        Ok(s) => s,
        Err(e) => {
            if let Some(d) = defer(&e) {
                return Err(d);
            }
            o.push(status(&e));
            return Ok(o);
        }
    };
    o.push(0.0);
    for step in steps {
        run_step(&k, &mut s, step, &mut o)?;
    }
    Ok(o)
}

fn rust_read(binary: bool, version: u32, bytes: &[u8]) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let ft = if binary {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    let mut db = DatabaseReader::with_stream(ft, InputStream::new(bytes.to_vec()));
    let mut o = Vec::new();
    push_res(
        &mut o,
        OffsetSurface::read_from_db(&k, &mut db, u64::from(version)),
        |o, s| put_state(o, &s),
    )?;
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let r = match i {
        Inputs::Script { off, steps } => rust_script(off, steps),
        Inputs::ReadDb {
            binary,
            version,
            bytes,
            ..
        } => {
            let bytes = bytes.as_ref().ok_or_else(|| harness("read_db: no bytes"))?;
            rust_read(*binary, *version, bytes)
        }
    };
    match r {
        Ok(rec) => Ok(Output::new().values("out", rec)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generated cases --------------------------------------------------------------------------

fn gen_knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let mut inner: Vec<f64> = (0..n - p).map(|_| rng.uniform(a, b)).collect();
    inner.sort_by(f64::total_cmp);
    // keep interior knots apart from the ends and from each other (or repeat one)
    let span = b - a;
    for k in &mut inner {
        *k = k.clamp(a + 1e-3 * span, b - 1e-3 * span);
    }
    if p >= 2 && inner.len() >= 2 && rng.chance(0.15) {
        let i = rng.below(inner.len() as u64 - 1) as usize;
        if let Some(&x) = inner.get(i) {
            if let Some(y) = inner.get_mut(i + 1) {
                *y = x;
            }
        }
    }
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(vec![b; p + 1]);
    u
}

type Frame3 = [[f64; 3]; 3];

fn frame(rng: &mut Rng) -> Frame3 {
    let e1 = rev::unit_vec(rng);
    let mut t = rev::unit_vec(rng);
    while rev::cross3(e1, t).iter().map(|c| c * c).sum::<f64>() < 0.1 {
        t = rev::unit_vec(rng);
    }
    let e3 = rev::normalize(rev::cross3(e1, t));
    let e2 = rev::cross3(e3, e1);
    [e1, e2, e3]
}

fn at(c: [f64; 3], f: &Frame3, a: f64, b: f64, h: f64) -> [f64; 3] {
    let [e1, e2, e3] = f;
    rev::add(rev::add(rev::add(c, *e1, a), *e2, b), *e3, h)
}

/// The kinds of generated B-spline bases.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Smooth,
    /// Row `i = 0` collapsed to a point (a pole on the `u = UMin` side).
    PoleU,
    /// Column `j = m` collapsed (a pole on the `v = VMax` side).
    PoleV,
    /// Closed in u with matching end tangents (periodic upstream).
    ClosedU,
    Flat,
    /// All control points on a line: no normal anywhere.
    Collinear,
}

fn gen_bss(rng: &mut Rng, shape: Shape, scale: f64) -> BssIn {
    let flat = shape == Shape::Flat;
    let p = if flat { 1 } else { 1 + rng.below(3) as usize };
    let q = if flat { 1 } else { 1 + rng.below(3) as usize };
    let mut n = p + rng.below(4) as usize;
    let m = q + rng.below(4) as usize;
    if shape == Shape::ClosedU {
        n = n.max(p + 2).max(3);
    }
    let f = frame(rng);
    let c = rng.point3(-2.0 * scale, 2.0 * scale);
    let (amp, f1, f2, ph) = (
        rng.uniform(0.0, 0.6) * scale,
        rng.uniform(0.5, 3.0),
        rng.uniform(0.5, 3.0),
        rng.uniform(0.0, 3.0),
    );
    let rational = !flat && rng.chance(0.2);
    let mut net = Vec::new();
    for i in 0..=n {
        let mut row = Vec::new();
        for j in 0..=m {
            let (s, t) = (i as f64 / n as f64, j as f64 / m as f64);
            let pt = match shape {
                Shape::Smooth | Shape::Flat => {
                    let h = if flat {
                        0.0
                    } else {
                        amp * (f1 * s + ph).sin() * (f2 * t).cos()
                    };
                    at(c, &f, 2.0 * scale * s, 1.5 * scale * t, h)
                }
                Shape::PoleU => {
                    // a dome: radius growing from 0 at i = 0
                    let r = scale * s;
                    let th = std::f64::consts::PI * t;
                    at(c, &f, r * th.cos(), r * th.sin(), scale * (1.0 - s * s))
                }
                Shape::PoleV => {
                    let r = scale * (1.0 - t);
                    let th = 1.5 * s;
                    at(c, &f, r * th.cos(), r * th.sin(), 0.8 * scale * t)
                }
                Shape::ClosedU => {
                    // a polygon around a circle, the last row equal to the first, so
                    // the end tangents match
                    let k = if i == n { 0 } else { i };
                    let th = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                    at(c, &f, scale * th.cos(), scale * th.sin(), 1.5 * scale * t)
                }
                Shape::Collinear => at(c, &f, scale * (s + 0.5 * t), 0.0, 0.0),
            };
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            row.push([pt[0], pt[1], pt[2], w]);
        }
        net.push(row);
    }
    if shape == Shape::PoleU {
        if let Some(first) = net.first().and_then(|r| r.first()).copied() {
            if let Some(row) = net.first_mut() {
                for pt in row.iter_mut() {
                    *pt = [first[0], first[1], first[2], pt[3]];
                }
            }
        }
    }
    if shape == Shape::PoleV {
        // every row ends at the apex
        let apex = at(c, &f, 0.0, 0.0, 0.8 * scale);
        for row in &mut net {
            if let Some(last) = row.last_mut() {
                *last = [apex[0], apex[1], apex[2], last[3]];
            }
        }
    }
    let (a, len) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-3.0, 3.0), rng.uniform(0.5, 4.0))
    };
    let u = gen_knots(rng, n, p, a, a + len);
    let (a2, len2) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-3.0, 3.0), rng.uniform(0.5, 4.0))
    };
    let v = gen_knots(rng, m, q, a2, a2 + len2);
    // uniform knots keep the closed polygon's end tangents equal
    let u = if shape == Shape::ClosedU {
        let mut u = vec![a; p + 1];
        for i in 1..=(n - p) {
            u.push(a + len * i as f64 / (n - p + 1) as f64);
        }
        u.extend(vec![a + len; p + 1]);
        u
    } else {
        u
    };
    BssIn {
        oob: rng.chance(0.1),
        form: default_form(),
        net,
        p,
        q,
        u,
        v,
    }
}

fn gen_shape(rng: &mut Rng) -> Shape {
    match rng.below(20) {
        0..=8 => Shape::Smooth,
        9..=11 => Shape::PoleU,
        12..=13 => Shape::PoleV,
        14..=15 => Shape::ClosedU,
        16..=18 => Shape::Flat,
        _ => Shape::Collinear,
    }
}

fn gen_bss_any(rng: &mut Rng, scale: f64) -> BssIn {
    let shape = gen_shape(rng);
    gen_bss(rng, shape, scale)
}

fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => 1e-3,
        1 => 1e3,
        2 => 10.0,
        // extreme magnitudes (no coordinate cap on the C++ side)
        3 => 1e-9,
        4 => 1e9,
        5 => 1e150,
        _ => 1.0,
    }
}

/// A surface of revolution whose line or circle generator is shrunk to `1e-9` .. `1e-12`
/// of its size (no minimum size on the offset's C++ side).
fn tiny_revolution(rng: &mut Rng) -> rev::SurfIn {
    let mut s = rev::gen_surf(rng);
    let f = *rng.choose(&[1e-9, 1e-12]).unwrap_or(&1e-9);
    match &mut s.gen {
        rev::GenIn::Circle { r, .. } => *r *= f,
        rev::GenIn::Line { a, b } => {
            for (bi, ai) in b.iter_mut().zip(a.iter()) {
                *bi = *ai + (*bi - *ai) * f;
            }
        }
        rev::GenIn::BSpline(_) => {}
    }
    s
}

fn gen_base(rng: &mut Rng, scale: f64, depth: usize) -> SrfIn {
    match rng.below(20) {
        0..=8 => SrfIn::Bspline(gen_bss_any(rng, scale)),
        9..=10 => SrfIn::Plane {
            o: rng.point3(-2.0 * scale, 2.0 * scale),
            n: rev::unit_vec(rng),
        },
        11..=13 => SrfIn::Revolution(rev::gen_surf(rng)),
        14 => SrfIn::Revolution(tiny_revolution(rng)),
        15 => SrfIn::Bounded {
            basis: Box::new(SrfIn::Bspline(gen_bss(rng, Shape::Smooth, scale))),
            implicit: rng.chance(0.5),
        },
        16 => SrfIn::Step {
            base: Box::new(SrfIn::Bspline(gen_bss_any(rng, scale))),
        },
        _ if depth < 2 => SrfIn::Offset(Box::new(gen_off(rng, scale, depth + 1))),
        _ => SrfIn::Bspline(gen_bss_any(rng, scale)),
    }
}

fn gen_dist(rng: &mut Rng, scale: f64) -> f64 {
    match rng.below(20) {
        0..=1 => 0.0,
        2 => rng.uniform(-1e-9, 1e-9),
        3..=4 => rng.uniform(2.0, 5.0) * scale * if rng.chance(0.5) { 1.0 } else { -1.0 },
        _ => rng.uniform(-1.0, 1.0) * scale,
    }
}

fn gen_off(rng: &mut Rng, scale: f64, depth: usize) -> OffIn {
    let base = gen_base(rng, scale, depth);
    let sing = match rng.below(10) {
        0..=1 => SS_UNKNOWN,
        2..=6 => 0,
        _ => rng.below(16) as u32,
    };
    OffIn {
        d: gen_dist(rng, scale),
        owns: rng.chance(0.85),
        sing,
        allow: rng.chance(0.3),
        base,
    }
}

/// The base's natural domain and size (the distance between its domain corners'
/// points, at least `1e-9`), built on the Rust side.
fn base_info(k: &Kernel, off: &OffIn) -> Option<([f64; 4], f64)> {
    let s = rust_srf(k, &off.base).ok()?.ok()?;
    let d = s.natural_uv_domain();
    let r = [d.min().x, d.min().y, d.max().x, d.max().y];
    if !(r.iter().all(|c| c.is_finite() && c.abs() < 1e5) && r[0] < r[2] && r[1] < r[3]) {
        return None;
    }
    let size = match (s.evaluate_point(d.min()), s.evaluate_point(d.max())) {
        (Ok(a), Ok(b)) => a.distance(b),
        _ => 0.0,
    };
    Some((r, size.max(1e-9)))
}

fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let [u0, v0, u1, v1] = d;
    let (lu, lv) = (u1 - u0, v1 - v0);
    let pick = |rng: &mut Rng, a: f64, b: f64, len: f64| match rng.below(20) {
        0..=10 => rng.uniform(a, b),
        11..=12 => a,
        13..=14 => b,
        15..=17 => {
            if rng.chance(0.5) {
                a - rng.uniform(0.0, 1.5) * len
            } else {
                b + rng.uniform(0.0, 1.5) * len
            }
        }
        _ => a + len * f64::from(rng.below(5) as u32) / 4.0,
    };
    [pick(rng, u0, u1, lu), pick(rng, v0, v1, lv)]
}

fn gen_inside(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    [rng.uniform(d[0], d[2]), rng.uniform(d[1], d[3])]
}

fn gen_subdomain(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    let [a, b] = gen_inside(rng, d);
    let [c, e] = gen_inside(rng, d);
    [
        a.min(c),
        b.min(e),
        a.max(c).max(a.min(c) + 1e-3 * (d[2] - d[0])),
        b.max(e).max(b.min(e) + 1e-3 * (d[3] - d[1])),
    ]
}

fn gen_orders(rng: &mut Rng) -> (usize, usize, bool) {
    let upper = rng.chance(0.35);
    if rng.chance(0.05) {
        // more than MAX_DERIVS: SM_ERR before anything is written (orders up to 8)
        return (
            rng.below(3) as usize * 3 + 2,
            rng.below(3) as usize * 3 + 2,
            false,
        );
    }
    let pairs: &[(usize, usize)] = if upper {
        &[
            (0, 0),
            (1, 1),
            (2, 2),
            (3, 3),
            (1, 0),
            (2, 0),
            (3, 0),
            (2, 1),
            (3, 1),
            (3, 2),
            (2, 3),
            (1, 2),
        ]
    } else {
        &[
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 1),
            (2, 0),
            (0, 2),
            (2, 1),
            (1, 2),
            (3, 0),
            (0, 3),
        ]
    };
    let &(hu, hv) = rng.choose(pairs).unwrap_or(&(1, 1));
    (hu, hv, upper)
}

/// One op; extensions are a fraction of the base `size` (tiny relative distances make
/// upstream's G1/G2 extension search loop forever, HANG-6).
fn gen_step(rng: &mut Rng, d: [f64; 4], scale: f64, size: f64, can_via: bool) -> StepIn {
    let via = can_via && rng.chance(0.3);
    match rng.below(100) {
        0..=24 => {
            let (hu, hv, upper) = gen_orders(rng);
            StepIn::Eval {
                uv: gen_uv(rng, d),
                hu,
                hv,
                ul: rng.chance(0.8),
                vl: rng.chance(0.8),
                upper,
                nzt: rng.chance(0.75),
                via,
            }
        }
        25..=34 => {
            let (hu, hv, upper) = gen_orders(rng);
            StepIn::EvalSimple {
                uv: gen_uv(rng, d),
                hu,
                hv,
                ul: rng.chance(0.8),
                vl: rng.chance(0.8),
                upper,
                via,
            }
        }
        35..=46 => StepIn::EvalPt {
            uv: gen_uv(rng, d),
            via,
        },
        47..=54 => StepIn::Normal {
            uv: gen_uv(rng, d),
            ul: rng.chance(0.8),
            vl: rng.chance(0.8),
            via,
        },
        55..=57 => StepIn::Knots {
            param: rng.below(2) as u8,
            ivl: rng.chance(0.3).then(|| {
                let [a, b] = gen_inside(rng, d);
                [a.min(b), a.max(b)]
            }),
        },
        58..=59 => StepIn::MaxAnal,
        60..=66 => StepIn::BBox {
            dom: if rng.chance(0.85) {
                d
            } else {
                gen_subdomain(rng, d)
            },
            want: [rng.chance(0.7), rng.chance(0.6), rng.chance(0.5)],
            expand: rng.chance(0.7),
        },
        67..=70 => StepIn::StepInv {
            dom: d,
            p: rng.point3(-3.0 * scale, 3.0 * scale),
            tol: if rng.chance(0.5) {
                1e-6
            } else {
                rng.uniform(1e-4, 0.5)
            },
            guess: rng.chance(0.4).then(|| gen_inside(rng, d)),
        },
        71..=72 => StepIn::Reverse {
            param: rng.below(2) as u8,
        },
        73..=74 => StepIn::Swap,
        75..=76 => {
            let x = rev::unit_vec(rng);
            let mut y = rev::unit_vec(rng);
            while rev::cross3(x, y).iter().map(|c| c * c).sum::<f64>() < 0.01 {
                y = rev::unit_vec(rng);
            }
            StepIn::Transform {
                origin: rng.point3(-2.0, 2.0),
                x,
                y,
                scale: match rng.below(4) {
                    0 => None,
                    1 => {
                        let s = rng.uniform(0.2, 3.0);
                        Some([s, s, s])
                    }
                    // unit, tiny, zero and negative uniform scales
                    2 => {
                        let s = *rng.choose(&[1.0, 1e-6, 0.0, -1e-3]).unwrap_or(&1.0);
                        Some([s, s, s])
                    }
                    _ => Some([
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                    ]),
                },
            }
        }
        77..=79 => StepIn::Trim {
            dom: if rng.chance(0.7) {
                gen_subdomain(rng, d)
            } else {
                let lu = d[2] - d[0];
                let lv = d[3] - d[1];
                [
                    d[0] - rng.uniform(0.0, 0.5) * lu,
                    d[1],
                    d[2] + rng.uniform(0.0, 0.5) * lu,
                    d[3] + rng.uniform(0.0, 0.2) * lv,
                ]
            },
        },
        80..=82 => {
            let param = rng.below(2) as u8;
            let (a, b) = if param == 0 {
                (d[0], d[2])
            } else {
                (d[1], d[3])
            };
            StepIn::Split {
                t: rng.uniform(a + 0.05 * (b - a), b - 0.05 * (b - a)),
                param,
            }
        }
        83..=85 => StepIn::CreateOffset {
            d: if rng.chance(0.2) {
                0.0
            } else {
                rng.uniform(-1.0, 1.0) * scale
            },
        },
        // (`ffi::check` refuses `CreateExtendedSurface` in process, UB-107/HANG-6/HANG-22,
        // so `gen_case` drops a script with one of these)
        86..=87 => StepIn::Extend {
            dist: rng.uniform(0.05, 0.4) * size,
            cont: *rng.choose(&[3, 4, 5, 13]).unwrap_or(&3),
        },
        88 => StepIn::ExtendDir {
            param: *rng.choose(&[0, 1, 3, 5, 6, 7, 8]).unwrap_or(&0),
            dist: rng.uniform(0.05, 0.4) * size,
            cont: *rng.choose(&[3, 4, 5, 13]).unwrap_or(&3),
        },
        89..=91 => StepIn::Copy,
        92..=93 => StepIn::Eq {
            other: gen_off(rng, scale, 2),
        },
        94 => StepIn::SetDist {
            d: gen_dist(rng, scale),
        },
        95 => StepIn::SetAllow { b: rng.chance(0.5) },
        96..=97 => StepIn::IsPlanar {
            tol: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
        },
        // (relative to the base's size, as in surface.surf_of_revolution_p2: an offset
        // far larger than a revolution's generator degenerates, and upstream's
        // `CreateSimpleOffset` then copies an uninitialised `dMaxGap3d`)
        98 => StepIn::BaseOffset {
            d: if rng.chance(0.1) {
                0.0
            } else {
                rng.uniform(-1.0, 1.0) * size
            },
            tol: *rng.choose(&[0.0, 1e-6, 1e-3]).unwrap_or(&1e-3) * scale,
        },
        _ => StepIn::IsKindOf {
            t: *rng
                .choose(&[OFFSET_SURFACE_TYPE, 15000, 0, 15001, 15400, 15100])
                .unwrap_or(&0),
        },
    }
}

/// A script on `off` whose `via` steps run only while extension is off (no extension
/// can then be stored).
fn gen_steps(rng: &mut Rng, off: &OffIn, d: [f64; 4], scale: f64, size: f64) -> Vec<StepIn> {
    let mut steps = Vec::new();
    if rng.chance(0.5) {
        steps.push(StepIn::State);
    }
    let mut allow = off.allow;
    let n = 1 + rng.below(5) as usize;
    for _ in 0..n {
        let step = gen_step(rng, d, scale, size, !allow);
        if let StepIn::SetAllow { b } = step {
            allow = b;
        }
        steps.push(step);
    }
    if rng.chance(0.4) {
        steps.push(StepIn::State);
    }
    steps
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let scale = gen_scale(rng);
    let off = gen_off(rng, scale, 0);
    let (d, size) = base_info(&k, &off)?;
    let foff = ffi_off(&off).ok()?;
    if rng.chance(0.08) {
        let binary = rng.chance(0.5);
        let version = *rng.choose(&[30, 33])?;
        let bytes = ffi::write_db_bytes(&foff, binary, version).ok()??;
        return Some(Inputs::ReadDb {
            off,
            binary,
            version,
            bytes: Some(bytes),
        });
    }
    let mut steps = gen_steps(rng, &off, d, scale, size);
    if let Some(degree_1) = degree_1_directions(&k, &off) {
        avoid_reflection_extensions(&mut steps, degree_1);
    }
    let fsteps = ffi_steps(&steps).ok()?;
    ffi::check(&foff, &fsteps).ok()?;
    // Domain/zero-offset guards depend on the actual C++ state after preceding
    // edits. Preflight candidates requiring those checks, dropping only an explicit
    // refusal: any other C++ error is kept so difftest reports it.
    let dynamic_guard = fsteps.iter().any(|s| {
        matches!(
            s,
            ffi::Step::BBox { .. }
                | ffi::Step::IsPlanar { .. }
                | ffi::Step::Eval { upper: true, .. }
                | ffi::Step::EvalSimple { upper: true, .. }
        )
    });
    if dynamic_guard
        && matches!(
            ffi::script(&foff, &fsteps),
            Err(solidcore_ffi::FfiError::InvalidArgument(_))
        )
    {
        return None;
    }
    Some(Inputs::Script { off, steps })
}

/// Which directions (`[u, v]`) of the B-spline NLib extends for this offset (its base,
/// looking through nested offsets, curve-bounded and STEP wrappers) have degree 1;
/// `None` when neither does.
fn degree_1_directions(k: &Kernel, off: &OffIn) -> Option<[bool; 2]> {
    let Ok(Ok(base)) = rust_srf(k, &off.base) else {
        return None;
    };
    let mut s = &base;
    for _ in 0..=solidcore::surface::offset_surface::MAX_SURFACE_NESTING {
        s = match s.kind() {
            SurfaceKind::OffsetSurface(o) => o.base_surface(),
            SurfaceKind::CurveBounded(c) => c.basis(),
            SurfaceKind::Step(st) => st.surface()?,
            SurfaceKind::BSpline(_) | SurfaceKind::Plane(_) | SurfaceKind::SurfOfRevolution(_) => {
                let d = [
                    s.degree(SurfParamType::U) == 1,
                    s.degree(SurfParamType::V) == 1,
                ];
                return (d[0] || d[1]).then_some(d);
            }
            SurfaceKind::SrfInVolume(_) => return None,
        };
    }
    None
}

/// UB-32 (`plan/upstream-issues.md`): a reflection extension (`G1`, `G1G2`) in a
/// degree-1 direction can make `ST_surxsdr`'s reflection-plane fallback read past its
/// scratch row; the port reports `InvalidInput` there. As in `nlib.srf_adv_p1`, keep
/// such extensions to C-max, only for the extensions that reach a degree-1 direction
/// (`degree_1` is `[u, v]` of the base; `SwapUV` exchanges them as the script runs).
/// (Applied after generation, so the random stream and the other cases are unchanged;
/// the evaluation-triggered extension is ruled and safe.)
fn avoid_reflection_extensions(steps: &mut [StepIn], degree_1: [bool; 2]) {
    use SurfParamType as P;
    let [mut du, mut dv] = degree_1;
    for step in steps {
        let (cont, reaches) = match step {
            StepIn::Swap => {
                core::mem::swap(&mut du, &mut dv);
                continue;
            }
            StepIn::Extend { cont, .. } => (cont, du || dv),
            StepIn::ExtendDir { param, cont, .. } => {
                let reaches = match param_of(*param) {
                    Ok(P::U | P::UMin | P::UMax) => du,
                    Ok(P::V | P::VMin | P::VMax) => dv,
                    Ok(P::Neither) => false,
                    _ => du || dv,
                };
                (cont, reaches)
            }
            _ => continue,
        };
        if reaches && matches!(*cont, 3 | 5) {
            *cont = 13;
        }
    }
}

/// Fills a fixture's placeholders from the base built on the Rust side: a `dom` of
/// `"NAT"` (the base's natural domain), `"SUB"` (its middle half) or `"OUT"` (a quarter
/// larger on every side); a `uv` of `"MID"`, `"VMIN"` or `"VMAX"` (the middle of the
/// domain, of its `v = VMin` side, of its `v = VMax` side); a `p` of `"ON"` (the offset's
/// point at `(0.4, 0.6)` of that domain, built without singularities).
fn fill_placeholders(case: &mut Case) -> Result<(), HarnessError> {
    let keys = ["dom", "uv", "p"];
    let Some(steps) = case.inputs.get("steps").and_then(Value::as_array) else {
        return Ok(());
    };
    let wanted = steps
        .iter()
        .any(|s| keys.iter().any(|k| s.get(k).is_some_and(Value::is_string)));
    if !wanted {
        return Ok(());
    }
    let bad = |what: &str| harness(format!("case {}: {what}", case.name));
    let off: OffIn = serde_json::from_value(
        case.inputs
            .get("off")
            .cloned()
            .ok_or_else(|| bad("no off"))?,
    )
    .map_err(|e| bad(&e.to_string()))?;
    let k = Kernel::default();
    let base = rust_srf(&k, &off.base)?.map_err(|e| bad(&format!("base: {e}")))?;
    let d = base.natural_uv_domain();
    let (u0, v0, u1, v1) = (d.min().x, d.min().y, d.max().x, d.max().y);
    let (lu, lv) = (u1 - u0, v1 - v0);
    let on = || -> Result<[f64; 3], HarnessError> {
        let plain = OffIn {
            sing: 0,
            ..off.clone()
        };
        let s = rust_off(&k, &plain)?.map_err(|e| bad(&format!("offset: {e}")))?;
        let p = s
            .evaluate_point(Point2::new(u0 + 0.4 * lu, v0 + 0.6 * lv))
            .map_err(|e| bad(&format!("point: {e}")))?;
        Ok([p.x, p.y, p.z])
    };
    let mut steps = steps.clone();
    for step in &mut steps {
        for key in keys {
            let Some(tag) = step.get(key).and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let v = match tag.as_str() {
                "NAT" => serde_json::json!([u0, v0, u1, v1]),
                "SUB" => serde_json::json!([
                    u0 + 0.25 * lu,
                    v0 + 0.25 * lv,
                    u1 - 0.25 * lu,
                    v1 - 0.25 * lv
                ]),
                "OUT" => serde_json::json!([
                    u0 - 0.25 * lu,
                    v0 - 0.25 * lv,
                    u1 + 0.25 * lu,
                    v1 + 0.25 * lv
                ]),
                "MID" => serde_json::json!([u0 + 0.5 * lu, v0 + 0.5 * lv]),
                "VMIN" => serde_json::json!([u0 + 0.5 * lu, v0]),
                "VMAX" => serde_json::json!([u0 + 0.5 * lu, v1]),
                "ON" => serde_json::json!(on()?),
                other => return Err(bad(&format!("unknown placeholder {other}"))),
            };
            if let Some(slot) = step.get_mut(key) {
                *slot = v;
            }
        }
    }
    if let Some(slot) = case.inputs.get_mut("steps") {
        *slot = Value::Array(steps);
    }
    Ok(())
}

/// Fills the bytes of a fixture `read_db` case with the C++ `WriteToDB` record.
fn fill_read_bytes(case: &mut Case) -> Result<(), HarnessError> {
    let mut i: Inputs = case.parse()?;
    let Inputs::ReadDb {
        off,
        binary,
        version,
        bytes,
    } = &mut i
    else {
        return Ok(());
    };
    if bytes.is_some() {
        return Ok(());
    }
    *bytes = Some(
        ffi::write_db_bytes(&ffi_off(off)?, *binary, *version)
            .map_err(|e| harness(e.to_string()))?
            .ok_or_else(|| harness(format!("case {}: WriteToDB failed", case.name)))?,
    );
    case.inputs = serde_json::to_value(i).map_err(|e| harness(e.to_string()))?;
    Ok(())
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.offset_surface"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..16 {
            if let Some(i) = gen_case(rng) {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for case in &mut cases {
            fill_placeholders(case)?;
            fill_read_bytes(case)?;
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
