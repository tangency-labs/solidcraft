//! Runner for unit `nurbs.b_spline_surface_p2`: the `SmBSplineSurface` members of
//! `SmBSplineSurface.cpp` part 2 (analytic copies, iso curves, spheres from arcs,
//! surfaces of revolution, trimmed planes from ruled surfaces, degeneracy and bilinear
//! tests, control point access and editing, `SetCanonical`, sub-patches, splitting,
//! `SwapUV`, the maximum analytic domain and `AdjustSTEPUVDomain`).
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_surface_p2` (upstream
//! `source/SMLib/src/SmBSplineSurface.cpp`). Rust:
//! `solidcore::nurbs::b_spline_surface::p2`.
//!
//! A case is `this` (the surface a member op runs on: `empty` for a surface without
//! NURB, `oob`, `form` and an inline or `.sms` surface), absent for the static ops,
//! plus `op` and its parameters. Both sides produce the same flat record (layout in the
//! FFI module docs), split into keys by [`to_output`]: `status` (`0` = `SM_SUCCESS`,
//! else the `SmStatus`: `1001` `SM_ERR`, `1006` `SM_ERR_NULL_POINTER`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce) and the op's outputs.
//!
//! Cases whose Rust side reaches an unported callee are deferred: every
//! `CopyAndAddAnalytics` (`SmPlane::IsNurbSurfacePlane`, `surface.plane_p2`),
//! `IsDegeneratePoint` (`SmSurface::IsKindOf`, `surface.surface_p2`) and
//! `GetMaxAnalyticDomain` (`SmSurface::IsPlanar`, `surface.surface_p7`), spheres
//! from three arcs
//! (`SmCurve::IsArc`, `curve.curve_p5`) and trimmed planes of non-rectangular ruled
//! surfaces (`SmSurface::Evaluate1stDerivatives`, `surface.surface_p1`). Every run has
//! at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, ControlPointForm, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, ControlPointBlock,
    IsoCurveOptions, SurfParamType,
};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_surface_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 700;

/// `nurbs.b_spline_surface_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// An inline 3D curve: control points as stored, degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream 3D `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Inline data.
    Inline(InlineCrv),
}

/// The surface a member op runs on.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct This {
    #[serde(default)]
    empty: bool,
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    srf: Option<SrfIn>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Analytics {},
    Iso {
        param: u8,
        iso: f64,
        #[serde(default)]
        dom: Option<[f64; 4]>,
        #[serde(default)]
        want_uv: bool,
    },
    DegenPt {
        tol: f64,
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    Bilinear {},
    GetCp {
        form: u8,
        u: usize,
        v: usize,
    },
    Expert {
        form: u8,
        su: usize,
        eu: usize,
        sv: usize,
        ev: usize,
        us: usize,
        vs: usize,
    },
    MaxDomain {},
    SetCanon {
        ud: usize,
        vd: usize,
        pts: Vec<[f64; 3]>,
        form: u8,
        uk: Vec<f64>,
        um: Vec<usize>,
        vk: Vec<f64>,
        vm: Vec<usize>,
        kt: u8,
        #[serde(default)]
        weights: Option<Vec<f64>>,
    },
    SetCp {
        form: u8,
        u: usize,
        v: usize,
        pt: [f64; 3],
        w: f64,
    },
    SubPatch {
        dom: [f64; 4],
    },
    Split {
        t: f64,
        param: u8,
    },
    Swap {},
    Adjust {
        dom: [f64; 4],
    },
    Sphere {
        arcs: Vec<CrvIn>,
        tol: f64,
    },
    Revolution {
        curve: CrvIn,
        origin: [f64; 3],
        axis: [f64; 3],
        angle: f64,
        #[serde(default)]
        swap: bool,
    },
    TrimPlane {
        c1: CrvIn,
        c2: CrvIn,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Analytics {} => Op::Analytics,
            OpIn::Iso { .. } => Op::Iso,
            OpIn::DegenPt { .. } => Op::DegenPt,
            OpIn::Bilinear {} => Op::Bilinear,
            OpIn::GetCp { .. } => Op::GetCp,
            OpIn::Expert { .. } => Op::Expert,
            OpIn::MaxDomain {} => Op::MaxDomain,
            OpIn::SetCanon { .. } => Op::SetCanon,
            OpIn::SetCp { .. } => Op::SetCp,
            OpIn::SubPatch { .. } => Op::SubPatch,
            OpIn::Split { .. } => Op::Split,
            OpIn::Swap {} => Op::Swap,
            OpIn::Adjust { .. } => Op::Adjust,
            OpIn::Sphere { .. } => Op::Sphere,
            OpIn::Revolution { .. } => Op::Revolution,
            OpIn::TrimPlane { .. } => Op::TrimPlane,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    this: Option<This>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- file readers -------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| harness(format!("{path}: {e}"))))
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

/// Reads an upstream 3D `.smc` text file: `n`, `p`, rational flag, dimension (3),
/// `n+1` points (`x y z`, or `wx wy wz w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let p = int(path, num()?)?;
    let rat = num()? != 0.0;
    if int(path, num()?)? != 3 {
        return Err(harness(format!("{path}: not a 3D curve")));
    }
    let mut cpts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let (x, y, z) = (num()?, num()?, num()?);
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineCrv { cpts, p, u })
}

fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

fn crv(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(i) => Ok(i.clone()),
    }
}

fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- C++ side -----------------------------------------------------------------------

fn push_srf(o: &mut Vec<f64>, s: &InlineSrf) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend(
        [n, m, s.p, s.q, s.u.len().saturating_sub(1), s.v.len().saturating_sub(1)]
            .map(|x| x as f64),
    );
    for row in &s.net {
        for p in row {
            o.extend_from_slice(p);
        }
    }
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn push_crv(o: &mut Vec<f64>, c: &InlineCrv) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([n as f64, c.p as f64, c.u.len().saturating_sub(1) as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn push_dom(o: &mut Vec<f64>, dom: Option<&[f64; 4]>) {
    o.push(flag(dom.is_some()));
    o.extend_from_slice(dom.unwrap_or(&[0.0, 0.0, 0.0, 0.0]));
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match &i.this {
        None => o.push(0.0),
        Some(t) => {
            o.push(if t.empty { 1.0 } else { 2.0 });
            o.push(flag(t.oob));
            o.push(f64::from(t.form));
            if !t.empty {
                let s = t.srf.as_ref().ok_or_else(|| harness("this: missing srf"))?;
                push_srf(&mut o, &srf(s)?)?;
            }
        }
    }
    match &i.op {
        OpIn::Analytics {} | OpIn::Bilinear {} | OpIn::MaxDomain {} | OpIn::Swap {} => {}
        OpIn::Iso {
            param,
            iso,
            dom,
            want_uv,
        } => {
            o.extend([f64::from(*param), *iso]);
            push_dom(&mut o, dom.as_ref());
            o.push(flag(*want_uv));
        }
        OpIn::DegenPt { tol, dom } => {
            o.push(*tol);
            push_dom(&mut o, dom.as_ref());
        }
        OpIn::GetCp { form, u, v } => o.extend([f64::from(*form), *u as f64, *v as f64]),
        OpIn::Expert {
            form,
            su,
            eu,
            sv,
            ev,
            us,
            vs,
        } => {
            o.push(f64::from(*form));
            o.extend([su, eu, sv, ev, us, vs].map(|x| *x as f64));
        }
        OpIn::SetCanon {
            ud,
            vd,
            pts,
            form,
            uk,
            um,
            vk,
            vm,
            kt,
            weights,
        } => {
            o.extend([*ud as f64, *vd as f64, pts.len() as f64]);
            for p in pts {
                o.extend_from_slice(p);
            }
            o.push(f64::from(*form));
            o.push(uk.len() as f64);
            o.extend_from_slice(uk);
            o.extend(um.iter().map(|&x| x as f64));
            o.push(vk.len() as f64);
            o.extend_from_slice(vk);
            o.extend(vm.iter().map(|&x| x as f64));
            o.push(f64::from(*kt));
            o.push(flag(weights.is_some()));
            if let Some(w) = weights {
                o.push(w.len() as f64);
                o.extend_from_slice(w);
            }
        }
        OpIn::SetCp { form, u, v, pt, w } => {
            o.extend([f64::from(*form), *u as f64, *v as f64]);
            o.extend_from_slice(pt);
            o.push(*w);
        }
        OpIn::SubPatch { dom } | OpIn::Adjust { dom } => o.extend_from_slice(dom),
        OpIn::Split { t, param } => o.extend([*t, f64::from(*param)]),
        OpIn::Sphere { arcs, tol } => {
            o.push(arcs.len() as f64);
            for c in arcs {
                push_crv(&mut o, &crv(c)?);
            }
            o.push(*tol);
        }
        OpIn::Revolution {
            curve,
            origin,
            axis,
            angle,
            swap,
        } => {
            push_crv(&mut o, &crv(curve)?);
            o.extend_from_slice(origin);
            o.extend_from_slice(axis);
            o.extend([*angle, flag(*swap)]);
        }
        OpIn::TrimPlane { c1, c2 } => {
            push_crv(&mut o, &crv(c1)?);
            push_crv(&mut o, &crv(c2)?);
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    let flat = ffi::eval(i.op.ffi(), &input).map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

// ---- output splitting ---------------------------------------------------------------

struct Split<'a> {
    flat: &'a [f64],
    at: usize,
    out: Output,
}

impl Split<'_> {
    fn take(&mut self, k: usize) -> Result<Vec<f64>, HarnessError> {
        let part = self
            .at
            .checked_add(k)
            .and_then(|end| self.flat.get(self.at..end))
            .ok_or_else(|| harness(format!("record too short ({} values)", self.flat.len())))?;
        self.at += k;
        Ok(part.to_vec())
    }

    fn count(&mut self) -> Result<usize, HarnessError> {
        let x = self.take(1)?.first().copied().unwrap_or(f64::NAN);
        if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
            return Err(harness(format!("bad count {x} in record")));
        }
        Ok(x as usize)
    }

    fn put(&mut self, key: &str, v: Vec<f64>) {
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
    }

    fn vals(&mut self, key: &str, k: usize) -> Result<(), HarnessError> {
        let v = self.take(k)?;
        self.put(key, v);
        Ok(())
    }

    fn flag(&mut self, key: &str) -> Result<bool, HarnessError> {
        let v = self.take(1)?;
        let set = v.first().copied() == Some(1.0);
        self.put(key, v);
        Ok(set)
    }

    fn state(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let flags = self.take(5)?;
        let has = flags.first().copied() == Some(1.0);
        self.put(&format!("{pfx}flags"), flags);
        if has {
            let dims = self.take(6)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let ok = |x: f64| x.fract() == 0.0 && (0.0..=1e6).contains(&x);
            let (n, m, r, s) = (cnt(0), cnt(1), cnt(4), cnt(5));
            if !(ok(n) && ok(m) && ok(r) && ok(s)) {
                return Err(harness(format!("bad surface dims {dims:?}")));
            }
            self.put(&format!("{pfx}dims"), dims);
            self.vals(&format!("{pfx}cpts"), 4 * (n as usize + 1) * (m as usize + 1))?;
            self.vals(&format!("{pfx}u"), r as usize + 1)?;
            self.vals(&format!("{pfx}v"), s as usize + 1)?;
        }
        Ok(())
    }

    fn curve_state(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let flags = self.take(6)?;
        let has = flags.get(5).copied() == Some(1.0);
        self.put(&format!("{pfx}flags"), flags);
        if has {
            let dims = self.take(3)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let ok = |x: f64| x.fract() == 0.0 && (0.0..=1e6).contains(&x);
            let (n, m) = (cnt(0), cnt(2));
            if !(ok(n) && ok(m)) {
                return Err(harness(format!("bad curve dims {dims:?}")));
            }
            self.put(&format!("{pfx}dims"), dims);
            self.vals(&format!("{pfx}cpts"), 4 * (n as usize + 1))?;
            self.vals(&format!("{pfx}knots"), m as usize + 1)?;
        }
        Ok(())
    }

    /// `has` then, when set, a surface state.
    fn new_state(&mut self, pfx: &str) -> Result<(), HarnessError> {
        if self.flag(&format!("{pfx}has"))? {
            self.state(pfx)?;
        }
        Ok(())
    }
}

/// Splits a flat record into keys (layout in the FFI module docs).
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut s = Split {
        flat,
        at: 0,
        out: Output::new(),
    };
    let status = s.take(1)?;
    let ok = status.first().copied() == Some(0.0);
    s.put("status", status);
    match &i.op {
        OpIn::Analytics {} => {
            if ok {
                s.vals("type", 1)?;
            }
        }
        OpIn::Iso { .. } => {
            if ok {
                s.vals("max_gap", 1)?;
                s.flag("has_uv")?;
                if s.flag("has_curve")? {
                    s.curve_state("curve_")?;
                }
            }
        }
        OpIn::DegenPt { .. } | OpIn::Bilinear {} => s.vals("flag", 1)?,
        OpIn::GetCp { .. } => {
            if ok {
                s.vals("point", 4)?;
            }
        }
        OpIn::Expert { .. } => {
            if ok {
                let k = s.count()?;
                s.put("len", vec![k as f64]);
                s.vals("values", k)?;
            }
        }
        OpIn::MaxDomain {} => s.vals("domain", 4)?,
        OpIn::SetCanon { .. } | OpIn::SetCp { .. } | OpIn::Swap {} | OpIn::Adjust { .. } => {
            s.state("this_")?;
        }
        OpIn::SubPatch { .. } | OpIn::Revolution { .. } => {
            if ok {
                s.new_state("new_")?;
            }
        }
        OpIn::Split { .. } => {
            if ok && s.flag("has")? {
                s.state("left_")?;
                s.state("right_")?;
            }
        }
        OpIn::Sphere { .. } => {
            if ok {
                let k = s.count()?;
                s.put("arc_count", vec![k as f64]);
                s.vals("ccw", k)?;
                s.vals("orients", k)?;
                s.new_state("new_")?;
            }
        }
        OpIn::TrimPlane { .. } => {
            if ok {
                s.vals("trim_count", 1)?;
                s.new_state("new_")?;
            }
        }
    }
    if s.at != flat.len() {
        return Err(harness(format!(
            "record has {} trailing values",
            flat.len() - s.at
        )));
    }
    Ok(s.out)
}

// ---- Rust side ----------------------------------------------------------------------

fn surface_of(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
        .collect();
    Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))
}

fn curve_of(c: &InlineCrv) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, 3, false))
}

fn bsc_of(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    curve_of(&crv(c)?)
}

fn this_of(t: &This) -> Result<BSplineSurface, HarnessError> {
    let mut s = if t.empty {
        BSplineSurface::new()
    } else {
        let si = t.srf.as_ref().ok_or_else(|| harness("this: missing srf"))?;
        BSplineSurface::from_nurb(surface_of(&srf(si)?)?, false)
    };
    *s.out_of_bounds_enabled_mut() = t.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(t.form))
        .ok_or_else(|| harness(format!("bad form {}", t.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    Ok(match v {
        0 => SurfParamType::U,
        1 => SurfParamType::V,
        2 => SurfParamType::Unknown,
        3 => SurfParamType::Both,
        4 => SurfParamType::Neither,
        5 => SurfParamType::UMin,
        6 => SurfParamType::VMin,
        7 => SurfParamType::UMax,
        8 => SurfParamType::VMax,
        9 => SurfParamType::Out,
        _ => return Err(harness(format!("bad SmSurfParamType {v}"))),
    })
}

fn cp_form_of(v: u8) -> Result<ControlPointForm, HarnessError> {
    Ok(match v {
        0 => ControlPointForm::NonRational,
        1 => ControlPointForm::HomogeneousRational,
        2 => ControlPointForm::EuclideanRational,
        _ => return Err(harness(format!("bad SmControlPointFormType {v}"))),
    })
}

fn knot_type_of(v: u8) -> Result<KnotType, HarnessError> {
    Ok(match v {
        0 => KnotType::UniformKnots,
        1 => KnotType::Unspecified,
        2 => KnotType::QuasiUniformKnots,
        3 => KnotType::PiecewiseBezierKnots,
        4 => KnotType::Unknown,
        _ => return Err(harness(format!("bad SmKnotType {v}"))),
    })
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

fn dom_of(d: &[f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn dump_state(o: &mut Vec<f64>, s: &BSplineSurface) {
    o.extend([
        flag(s.gw_nurb().is_some()),
        flag(s.is_out_of_bounds_enabled()),
        flag(s.is_nurb_borrowed()),
        f64::from(s.knot_type() as i32),
        f64::from(s.form() as i32),
    ]);
    if let Some(sur) = s.gw_nurb() {
        let ix = sur.indices();
        o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
        for row in sur.rows() {
            for p in row {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        o.extend_from_slice(sur.u_knots());
        o.extend_from_slice(sur.v_knots());
    }
}

fn dump_curve(o: &mut Vec<f64>, c: &BSplineCurve) {
    o.extend([
        c.dim() as f64,
        flag(c.is_out_of_bounds_enabled()),
        flag(c.is_nurb_borrowed()),
        f64::from(c.knot_type() as i32),
        f64::from(c.form() as i32),
        flag(c.gw_nurb().is_some()),
    ]);
    if let Some(cur) = c.gw_nurb() {
        o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
        for p in cur.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.extend_from_slice(cur.knots());
    }
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// Runs a mutating op on `this` and records status and state (the state also on
/// failure, as the shim does).
fn mutate(
    o: &mut Vec<f64>,
    t: &This,
    f: impl FnOnce(&mut BSplineSurface) -> solidcore::Result<()>,
) -> Result<Option<Output>, HarnessError> {
    let mut this = this_of(t)?;
    if let Err(e) = f(&mut this) {
        if let Some(out) = Output::unported(&e) {
            return Ok(Some(out));
        }
        *o = vec![status(&e)];
    }
    dump_state(o, &this);
    Ok(None)
}

#[allow(clippy::too_many_lines)] // one arm per op
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = vec![0.0];
    let res: solidcore::Result<()> = match (&i.op, &i.this) {
        (OpIn::Analytics {}, Some(t)) => this_of(t)?
            .copy_and_add_analytics(&k)
            .map(|s| o.push(s.type_code() as f64)),
        (
            OpIn::Iso {
                param,
                iso,
                dom,
                want_uv,
            },
            Some(t),
        ) => {
            let this = this_of(t)?;
            let opts = IsoCurveOptions {
                domain: dom.as_ref().map(dom_of),
                uv_iso_curve: *want_uv,
            };
            this.create_iso_parametric_curve(&k, param_of(*param)?, *iso, opts)
                .map(|r| {
                    o.extend([r.max_gap_3d, flag(r.uv_iso_curve.is_some()), 1.0]);
                    dump_curve(&mut o, &r.curve);
                })
        }
        (OpIn::DegenPt { tol, dom }, Some(t)) => {
            let d = dom.as_ref().map(dom_of);
            this_of(t)?
                .is_degenerate_point(&k, *tol, d.as_ref())
                .map(|b| o.push(flag(b)))
        }
        (OpIn::Bilinear {}, Some(t)) => {
            o.push(flag(this_of(t)?.is_bilinear()));
            Ok(())
        }
        (OpIn::GetCp { form, u, v }, Some(t)) => this_of(t)?
            .control_point(cp_form_of(*form)?, *u, *v)
            .map(|(p, w)| o.extend([p.x, p.y, p.z, w])),
        (
            OpIn::Expert {
                form,
                su,
                eu,
                sv,
                ev,
                us,
                vs,
            },
            Some(t),
        ) => {
            let block = ControlPointBlock {
                start_u: *su,
                end_u: *eu,
                start_v: *sv,
                end_v: *ev,
                u_stride: *us,
                v_stride: *vs,
            };
            this_of(t)?
                .control_points_expert(cp_form_of(*form)?, &block)
                .map(|v| {
                    o.push(v.len() as f64);
                    o.extend_from_slice(&v);
                })
        }
        (OpIn::MaxDomain {}, Some(t)) => this_of(t)?.max_analytic_domain(&k).map(|d| {
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
        }),
        (
            OpIn::SetCanon {
                ud,
                vd,
                pts,
                form,
                uk,
                um,
                vk,
                vm,
                kt,
                weights,
            },
            Some(t),
        ) => {
            let pts: Vec<Point3> = pts.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect();
            let form = BSplineSurfaceForm::from_code(u64::from(*form))
                .ok_or_else(|| harness(format!("bad form {form}")))?;
            let kt = knot_type_of(*kt)?;
            let data = CanonicalSurfaceData {
                u_degree: *ud,
                v_degree: *vd,
                control_points: &pts,
                form,
                u_multiplicities: um,
                v_multiplicities: vm,
                u_knots: uk,
                v_knots: vk,
                knot_type: kt,
                weights: weights.as_deref(),
                uv_domain: None,
            };
            if let Some(out) = mutate(&mut o, t, |s| s.set_canonical(&data))? {
                return Ok(out);
            }
            return to_output(i, &o);
        }
        (OpIn::SetCp { form, u, v, pt, w }, Some(t)) => {
            let form = cp_form_of(*form)?;
            let p = Point3::new(pt[0], pt[1], pt[2]);
            if let Some(out) = mutate(&mut o, t, |s| s.set_control_point(form, *u, *v, p, *w))? {
                return Ok(out);
            }
            return to_output(i, &o);
        }
        (OpIn::Swap {}, Some(t)) => {
            if let Some(out) = mutate(&mut o, t, BSplineSurface::swap_uv)? {
                return Ok(out);
            }
            return to_output(i, &o);
        }
        (OpIn::Adjust { dom }, Some(t)) => {
            let d = dom_of(dom);
            if let Some(out) = mutate(&mut o, t, |s| s.adjust_step_uv_domain(&d))? {
                return Ok(out);
            }
            return to_output(i, &o);
        }
        (OpIn::SubPatch { dom }, Some(t)) => {
            this_of(t)?.copy_sub_patch(&dom_of(dom)).map(|s| {
                o.push(1.0);
                dump_state(&mut o, &s);
            })
        }
        (OpIn::Split { t: par, param }, Some(t)) => this_of(t)?
            .split_at(*par, param_of(*param)?)
            .map(|(l, r)| {
                o.push(1.0);
                dump_state(&mut o, &l);
                dump_state(&mut o, &r);
            }),
        (OpIn::Sphere { arcs, tol }, None) => {
            let cs = arcs
                .iter()
                .map(|a| bsc_of(a).map(SmCurve::from))
                .collect::<Result<Vec<_>, _>>()?;
            BSplineSurface::create_sphere_from_arcs(&k, &cs, *tol).map(|r| {
                o.push(r.ccw_arcs.len() as f64);
                o.extend(r.ccw_arcs.iter().map(|&j| j as f64));
                o.extend(r.orients.iter().map(|&x| orient_code(x)));
                o.push(1.0);
                dump_state(&mut o, &r.sphere);
            })
        }
        (
            OpIn::Revolution {
                curve,
                origin,
                axis,
                angle,
                swap,
            },
            None,
        ) => {
            let c = bsc_of(curve)?;
            BSplineSurface::create_surf_of_revolution(
                &Kernel::default(),
                &c,
                Point3::new(origin[0], origin[1], origin[2]),
                Vec3::new(axis[0], axis[1], axis[2]),
                *angle,
                *swap,
                None,
            )
            .map(|s| {
                o.push(1.0);
                dump_state(&mut o, &s);
            })
        }
        (OpIn::TrimPlane { c1, c2 }, None) => {
            let (a, b) = (bsc_of(c1)?, bsc_of(c2)?);
            BSplineSurface::create_trimmed_plane_from_ruled(&Kernel::default(), &a, &b).map(|r| {
                o.push(r.trimming_curves.len() as f64);
                o.push(1.0);
                dump_state(&mut o, &r.plane);
            })
        }
        _ => return Err(harness("op and `this` do not match")),
    };
    match res {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(out) => Ok(out),
            None => to_output(i, &[status(&e)]),
        },
    }
}

// ---- generators ---------------------------------------------------------------------

/// `v[i]` (`0` past the end).
fn kat(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

/// The last value of `v` (`0` when empty).
fn klast(v: &[f64]) -> f64 {
    v.last().copied().unwrap_or(0.0)
}

/// A clamped knot vector of degree `p` for `n + 1` control points over `[a, b]`,
/// sometimes with a repeated interior knot.
fn gen_knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    if rng.chance(0.4) {
        inner = (1..=interior).map(|i| i as f64 / (interior + 1) as f64).collect();
    }
    inner.sort_by(f64::total_cmp);
    let mut prev = f64::NEG_INFINITY;
    for t in &mut inner {
        if *t <= prev + 1e-3 {
            *t = prev + 1e-3;
        }
        prev = *t;
    }
    // a double interior knot (multiplicity at most the degree)
    if p >= 2 && inner.len() >= 2 && rng.chance(0.15) {
        let j = 1 + rng.below(inner.len() as u64 - 1) as usize;
        let prev = kat(&inner, j - 1);
        if let Some(t) = inner.get_mut(j) {
            *t = prev;
        }
    }
    let mut k = vec![a; p + 1];
    k.extend(inner.iter().map(|t| a + (b - a) * t.min(0.999)));
    k.extend(vec![b; p + 1]);
    k
}

fn gen_domain(rng: &mut Rng) -> (f64, f64) {
    if rng.chance(0.5) {
        return (0.0, 1.0);
    }
    let a = rng.uniform(-2.0, 2.0);
    (a, a + rng.uniform(0.5, 3.0))
}

/// A wavy 3D surface over the grid `[0, n] x [0, m]`, optionally rational.
fn gen_srf(rng: &mut Rng, rational: bool, max_deg: usize) -> InlineSrf {
    let p = 1 + rng.below(max_deg as u64) as usize;
    let q = 1 + rng.below(max_deg as u64) as usize;
    let n = p + rng.below(4) as usize;
    let m = q + rng.below(4) as usize;
    let amp = rng.uniform(0.0, 1.0);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let x = i as f64 + rng.uniform(-0.2, 0.2);
                    let y = j as f64 + rng.uniform(-0.2, 0.2);
                    let z = amp * rng.uniform(-1.0, 1.0);
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [w * x, w * y, w * z, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    let (a, b) = gen_domain(rng);
    let (c, d) = gen_domain(rng);
    InlineSrf {
        net,
        p,
        q,
        u: gen_knots(rng, n, p, a, b),
        v: gen_knots(rng, m, q, c, d),
    }
}

/// A surface for the member ops: mostly generated, sometimes collapsed to a point
/// or with a 2 x 2 net.
fn gen_this(rng: &mut Rng) -> InlineSrf {
    let rational = rng.chance(0.25);
    let mut s = gen_srf(rng, rational, 4);
    if rng.chance(0.08) {
        // point-sized surface
        let pt = s.net.first().and_then(|r| r.first()).copied().unwrap_or([0.0; 4]);
        for row in &mut s.net {
            for p in row.iter_mut() {
                *p = pt;
            }
        }
    }
    s
}

fn this(s: InlineSrf, rng: &mut Rng) -> This {
    This {
        empty: false,
        oob: rng.chance(0.1),
        form: if rng.chance(0.1) {
            rng.below(12) as u8
        } else {
            default_form()
        },
        srf: Some(SrfIn::Inline(s)),
    }
}

fn empty_this() -> This {
    This {
        empty: true,
        oob: false,
        form: default_form(),
        srf: None,
    }
}

/// A parameter near `[a, b]` with knots `k`: inside, on a knot, a hair off a knot or
/// an end, or outside.
fn gen_param(rng: &mut Rng, k: &[f64]) -> f64 {
    let (a, b) = (kat(k, 0), klast(k));
    match rng.below(8) {
        0 | 1 => *rng.choose(k).unwrap_or(&a),
        2 => {
            let kn = *rng.choose(k).unwrap_or(&a);
            let eps = *rng
                .choose(&[1e-13, -1e-13, 1e-11, -1e-11, 1e-9, -1e-9, 1e-7, -1e-7])
                .unwrap_or(&0.0);
            kn + eps * (1.0 + kn.abs())
        }
        3 => {
            let e = *rng.choose(&[1e-10, 1e-9, 2e-8, 1e-6, 0.1]).unwrap_or(&0.1);
            if rng.chance(0.5) {
                a - e
            } else {
                b + e
            }
        }
        _ => rng.uniform(a, b),
    }
}

/// A sub-domain of `[k0, k1]`, sometimes reaching outside or snapped near knots.
fn gen_sub(rng: &mut Rng, k: &[f64]) -> (f64, f64) {
    let (a, b) = (kat(k, 0), klast(k));
    let len = b - a;
    let mut lo = gen_param(rng, k).clamp(a - 0.2 * len, b + 0.2 * len);
    let mut hi = gen_param(rng, k).clamp(a - 0.2 * len, b + 0.2 * len);
    if lo > hi {
        core::mem::swap(&mut lo, &mut hi);
    }
    match rng.below(10) {
        0 => (a, b),
        1 => (a - 0.5 * len, b + 0.5 * len),
        2 => (lo, lo),
        _ => (lo, hi),
    }
}

fn gen_dom(rng: &mut Rng, s: &InlineSrf) -> [f64; 4] {
    let (u0, u1) = gen_sub(rng, &s.u);
    let (v0, v1) = gen_sub(rng, &s.v);
    [u0, v0, u1, v1]
}

fn gen_iso(rng: &mut Rng) -> Inputs {
    let s = gen_this(rng);
    let param = if rng.chance(0.1) {
        rng.below(10) as u8
    } else {
        rng.below(2) as u8
    };
    let k = if param == 0 { &s.u } else { &s.v };
    let iso = gen_param(rng, k);
    let dom = if rng.chance(0.4) {
        let mut d = gen_dom(rng, &s);
        if rng.chance(0.3) {
            // keep the iso parameter inside
            if param == 0 {
                d[0] = d[0].min(iso);
                d[2] = d[2].max(iso);
            } else {
                d[1] = d[1].min(iso);
                d[3] = d[3].max(iso);
            }
        }
        Some(d)
    } else {
        None
    };
    let empty = rng.chance(0.03);
    Inputs {
        this: Some(if empty { empty_this() } else { this(s, rng) }),
        op: OpIn::Iso {
            param,
            iso,
            dom,
            want_uv: rng.chance(0.15),
        },
    }
}

fn gen_cp_ops(rng: &mut Rng) -> Inputs {
    let s = gen_this(rng);
    let n = s.net.len() - 1;
    let m = s.net.first().map_or(0, Vec::len) - 1;
    let form = rng.below(3) as u8;
    let idx = |rng: &mut Rng, hi: usize| rng.below(hi as u64 + 1) as usize;
    let op = match rng.below(4) {
        0 => OpIn::GetCp {
            form,
            u: idx(rng, n),
            v: idx(rng, m),
        },
        1 => {
            let su = idx(rng, n);
            let sv = idx(rng, m);
            let mut eu = su + idx(rng, n - su);
            let mut ev = sv + idx(rng, m - sv);
            if rng.chance(0.1) {
                eu += 1 + rng.below(2) as usize; // outside the net
            }
            if rng.chance(0.05) {
                ev = sv.saturating_sub(1); // inverted
            }
            let pt = if form == 0 { 3 } else { 4 };
            let vs = pt + rng.below(3) as usize;
            let rows = (ev + 1).saturating_sub(sv).max(1);
            let mut us = vs * rows + rng.below(3) as usize;
            if rng.chance(0.08) {
                us = vs; // too small for more than one v point
            }
            let vs = if rng.chance(0.05) { pt - 1 } else { vs };
            OpIn::Expert {
                form,
                su,
                eu,
                sv,
                ev,
                us,
                vs,
            }
        }
        _ => {
            let (mut u, mut v) = (idx(rng, n), idx(rng, m));
            if rng.chance(0.05) {
                u = n + 1;
            }
            if rng.chance(0.05) {
                v = m + 1 + rng.below(3) as usize;
            }
            OpIn::SetCp {
                form,
                u,
                v,
                pt: rng.point3(-5.0, 5.0),
                w: rng.uniform(0.25, 3.0),
            }
        }
    };
    Inputs {
        this: Some(this(s, rng)),
        op,
    }
}

/// Unique knots with multiplicities for `count` control points of degree `deg`.
fn canon_knots(rng: &mut Rng, count: usize, deg: usize) -> (Vec<f64>, Vec<usize>) {
    let (a, b) = gen_domain(rng);
    let mut uk = vec![a];
    let mut um = vec![deg + 1];
    let mut left = count - (deg + 1);
    let mut t = 0.0;
    while left > 0 {
        let mult = (1 + rng.below(deg as u64) as usize).min(left);
        t += rng.uniform(0.2, 1.0);
        uk.push(t);
        um.push(mult);
        left -= mult;
    }
    let total = t + rng.uniform(0.2, 1.0);
    for x in uk.iter_mut().skip(1) {
        *x = a + (b - a) * *x / total;
    }
    uk.push(b);
    um.push(deg + 1);
    (uk, um)
}

fn gen_set_canon(rng: &mut Rng) -> Inputs {
    let ud = 1 + rng.below(3) as usize;
    let vd = 1 + rng.below(3) as usize;
    let nu = ud + 1 + rng.below(3) as usize;
    let nv = vd + 1 + rng.below(3) as usize;
    let (uk, mut um) = canon_knots(rng, nu, ud);
    let (vk, vm) = canon_knots(rng, nv, vd);
    let mut pts: Vec<[f64; 3]> = (0..nu * nv)
        .map(|k| {
            let (i, j) = (k / nv, k % nv);
            [i as f64, j as f64, rng.uniform(-1.0, 1.0)]
        })
        .collect();
    let mut weights = match rng.below(4) {
        0 => None,
        1 => Some(Vec::new()),
        2 => Some(vec![1.0; nu * nv]),
        _ => Some((0..nu * nv).map(|_| rng.uniform(0.5, 2.0)).collect()),
    };
    match rng.below(12) {
        0 => {
            pts.pop(); // point count mismatch
        }
        1 => {
            if let Some(w) = weights.as_mut() {
                w.push(1.0); // weight count mismatch
            }
        }
        2 => {
            if let Some(x) = um.get_mut(1) {
                *x += 1; // knot / point count mismatch
            }
        }
        _ => {}
    }
    let empty = rng.chance(0.1);
    let rational = rng.chance(0.3);
    let s = gen_srf(rng, rational, 3);
    Inputs {
        this: Some(if empty { empty_this() } else { this(s, rng) }),
        op: OpIn::SetCanon {
            ud,
            vd,
            pts,
            form: rng.below(12) as u8,
            uk,
            um,
            vk,
            vm,
            kt: rng.below(5) as u8,
            weights,
        },
    }
}

fn gen_domain_ops(rng: &mut Rng) -> Inputs {
    let s = gen_this(rng);
    let op = match rng.below(5) {
        0 | 1 => OpIn::SubPatch {
            dom: if rng.chance(0.05) {
                [10.0, 10.0, 11.0, 11.0]
            } else {
                gen_dom(rng, &s)
            },
        },
        2 | 3 => {
            let param = if rng.chance(0.1) {
                rng.below(10) as u8
            } else {
                rng.below(2) as u8
            };
            let k = if param == 0 { &s.u } else { &s.v };
            OpIn::Split {
                t: gen_param(rng, k),
                param,
            }
        }
        _ => {
            // AdjustSTEPUVDomain: grow or shrink each side a little
            let (u0, u1) = (kat(&s.u, 0), klast(&s.u));
            let (v0, v1) = (kat(&s.v, 0), klast(&s.v));
            let side = |rng: &mut Rng, x: f64, len: f64, dir: f64| match rng.below(4) {
                0 => x,
                1 => x + dir * rng.uniform(0.0, 0.3) * len,
                _ => x - dir * rng.uniform(0.0, 0.3) * len,
            };
            let (lu, lv) = (u1 - u0, v1 - v0);
            OpIn::Adjust {
                dom: [
                    side(rng, u0, lu, -1.0),
                    side(rng, v0, lv, -1.0),
                    side(rng, u1, lu, 1.0),
                    side(rng, v1, lv, 1.0),
                ],
            }
        }
    };
    Inputs {
        this: Some(this(s, rng)),
        op,
    }
}

fn gen_misc_member(rng: &mut Rng) -> Inputs {
    let s = gen_this(rng);
    let (op, empty) = match rng.below(10) {
        0..=3 => (OpIn::Swap {}, rng.chance(0.05)),
        4 | 5 => (OpIn::Bilinear {}, rng.chance(0.1)),
        6 => (OpIn::Analytics {}, false),
        7 => (OpIn::MaxDomain {}, false),
        _ => {
            let dom = if rng.chance(0.4) {
                Some(gen_dom(rng, &s))
            } else {
                None
            };
            (
                OpIn::DegenPt {
                    tol: *rng.choose(&[1e-6, 1e-3, 10.0]).unwrap_or(&1e-6),
                    dom,
                },
                false,
            )
        }
    };
    let s = if matches!(op, OpIn::Bilinear {}) && rng.chance(0.4) {
        // a 2 x 2 net
        let mut t = gen_srf(rng, false, 1);
        t.net.truncate(2);
        for r in &mut t.net {
            r.truncate(2);
        }
        t.u = vec![0.0, 0.0, 1.0, 1.0];
        t.v = vec![0.0, 0.0, 1.0, 1.0];
        t
    } else {
        s
    };
    Inputs {
        this: Some(if empty { empty_this() } else { this(s, rng) }),
        op,
    }
}

/// A 3D generator curve for revolutions: degree 1 to 3, sometimes rational, sometimes
/// starting or ending on the z axis.
fn gen_gen_curve(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.25);
    let mut cpts: Vec<[f64; 4]> = (0..=n)
        .map(|i| {
            let x = rng.uniform(0.5, 3.0);
            let z = i as f64 + rng.uniform(-0.3, 0.3);
            let y = rng.uniform(-0.5, 0.5);
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [w * x, w * y, w * z, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    if rng.chance(0.25) {
        // first point on the axis
        if let Some(c) = cpts.first_mut() {
            c[0] = 0.0;
            c[1] = 0.0;
        }
    }
    if rng.chance(0.1) {
        if let Some(c) = cpts.last_mut() {
            c[0] = 0.0;
            c[1] = 0.0;
        }
    }
    if rng.chance(0.03) {
        // everything on the axis
        for c in &mut cpts {
            c[0] = 0.0;
            c[1] = 0.0;
        }
    }
    let (a, b) = gen_domain(rng);
    InlineCrv {
        cpts,
        p,
        u: gen_knots(rng, n, p, a, b),
    }
}

fn gen_revolution(rng: &mut Rng) -> Inputs {
    let curve = gen_gen_curve(rng);
    let axis = match rng.below(6) {
        0 => [0.0, 0.0, 0.0],
        1 | 2 => [0.0, 0.0, rng.uniform(0.5, 3.0)],
        _ => {
            let a = rng.point3(-0.2, 0.2);
            [a[0], a[1], 1.0 + a[2]]
        }
    };
    let origin = if rng.chance(0.5) {
        [0.0, 0.0, 0.0]
    } else {
        rng.point3(-0.3, 0.3)
    };
    let angle = match rng.below(10) {
        0 => 360.0,
        1 => 90.0,
        2 => 180.0,
        3 => 270.0,
        4 => rng.uniform(-360.0, 0.0),
        5 => 400.0,
        _ => rng.uniform(1.0, 360.0),
    };
    Inputs {
        this: None,
        op: OpIn::Revolution {
            curve: CrvIn::Inline(curve),
            origin,
            axis,
            angle,
            swap: rng.chance(0.5),
        },
    }
}

/// A degree-1 or -2 line from `a` to `b` over `[0, 1]`.
fn line(rng: &mut Rng, a: [f64; 3], b: [f64; 3]) -> InlineCrv {
    let mid = [
        0.5 * (a[0] + b[0]),
        0.5 * (a[1] + b[1]),
        0.5 * (a[2] + b[2]),
    ];
    if rng.chance(0.6) {
        InlineCrv {
            cpts: vec![[a[0], a[1], a[2], NOW], [b[0], b[1], b[2], NOW]],
            p: 1,
            u: vec![0.0, 0.0, 1.0, 1.0],
        }
    } else {
        InlineCrv {
            cpts: vec![
                [a[0], a[1], a[2], NOW],
                [mid[0], mid[1], mid[2], NOW],
                [b[0], b[1], b[2], NOW],
            ],
            p: 2,
            u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        }
    }
}

fn gen_trim_plane(rng: &mut Rng) -> Inputs {
    let a = rng.point3(-2.0, 2.0);
    let d = rng.point3(-2.0, 2.0);
    let b = [a[0] + d[0] + 1.0, a[1] + d[1], a[2] + d[2]];
    // offset perpendicular to the line (exactly so for an axis-aligned pair)
    let off = if rng.chance(0.3) {
        let (a, b) = (rng.uniform(0.5, 2.0), 0.0);
        let mut a_off = [0.0, a, b];
        if rng.chance(0.5) {
            a_off = [0.0, 0.0, a];
        }
        a_off
    } else {
        let dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let r = rng.point3(-1.5, 1.5);
        let dd = dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2];
        let t = (r[0] * dir[0] + r[1] * dir[1] + r[2] * dir[2]) / dd;
        [r[0] - t * dir[0], r[1] - t * dir[1], r[2] - t * dir[2]]
    };
    let (a, b) = if matches!(off, [0.0, _, _]) && rng.chance(0.5) {
        // axis-aligned line along x
        ([a[0], a[1], a[2]], [a[0] + 2.0, a[1], a[2]])
    } else {
        (a, b)
    };
    let shift = |p: [f64; 3]| [p[0] + off[0], p[1] + off[1], p[2] + off[2]];
    let c1 = line(rng, a, b);
    let c2 = match rng.below(5) {
        // parallel and the same way: a rectangle (param1 = 0, param2 = 1)
        0 | 1 => line(rng, shift(a), shift(b)),
        // the other way (param1 = 1, param2 = 0)
        2 => line(rng, shift(b), shift(a)),
        // not a rectangle: a shorter parallel line
        3 => line(
            rng,
            shift(a),
            [
                shift(a)[0] + 0.5 * (b[0] - a[0]),
                shift(a)[1] + 0.5 * (b[1] - a[1]),
                shift(a)[2] + 0.5 * (b[2] - a[2]),
            ],
        ),
        // a curved second boundary
        _ => {
            let mut c = line(rng, shift(a), shift(b));
            c.p = 2;
            let m = [
                0.5 * (shift(a)[0] + shift(b)[0]),
                0.5 * (shift(a)[1] + shift(b)[1]) + 0.5,
                0.5 * (shift(a)[2] + shift(b)[2]),
            ];
            c.cpts = vec![
                [shift(a)[0], shift(a)[1], shift(a)[2], NOW],
                [m[0], m[1], m[2], NOW],
                [shift(b)[0], shift(b)[1], shift(b)[2], NOW],
            ];
            c.u = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            c
        }
    };
    Inputs {
        this: None,
        op: OpIn::TrimPlane {
            c1: CrvIn::Inline(c1),
            c2: CrvIn::Inline(c2),
        },
    }
}

fn gen_sphere(rng: &mut Rng) -> Inputs {
    let k = match rng.below(6) {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 3,
    };
    let arcs = (0..k)
        .map(|_| CrvIn::Inline(gen_gen_curve(rng)))
        .collect();
    Inputs {
        this: None,
        op: OpIn::Sphere {
            arcs,
            tol: *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-6),
        },
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(20) {
        0..=3 => gen_iso(rng),
        4..=7 => gen_cp_ops(rng),
        8 | 9 => gen_set_canon(rng),
        10..=12 => gen_domain_ops(rng),
        13 | 14 => gen_misc_member(rng),
        15..=17 => gen_revolution(rng),
        18 => gen_trim_plane(rng),
        _ => gen_sphere(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.b_spline_surface_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach success and failure of every op on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..1500u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let op = format!("{:?}", i.op.ffi());
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((op, kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        for op in [
            "Iso",
            "Bilinear",
            "GetCp",
            "Expert",
            "SetCanon",
            "SetCp",
            "SubPatch",
            "Split",
            "Swap",
            "Adjust",
            "Revolution",
            "TrimPlane",
        ] {
            let ok = tally
                .iter()
                .any(|((o, k), _)| o == op && k == "status:0");
            assert!(ok, "{op} never succeeds");
        }
    }
}
