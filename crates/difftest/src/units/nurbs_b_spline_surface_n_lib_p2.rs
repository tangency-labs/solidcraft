//! Runner for unit `nurbs.b_spline_surface_n_lib_p2`: `SmBSplineSurface::Evaluate`,
//! `TrimWithDomain`, `CreateSkinnedSurface` and `CreateCornerBlend`.
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_surface_n_lib_p2` (upstream
//! `source/SMLib/src/SmBSplineSurfaceNLib.cpp`). Rust:
//! `solidcore::nurbs::b_spline_surface_n_lib::p2`.
//!
//! A case is `this` (the surface a member op runs on: `empty` for a surface without
//! NURB, `oob`, `form` and an inline or `.sms` surface), absent for the static ops,
//! plus `op` and its parameters. Both sides produce the same flat record (layout in the
//! FFI module docs), split into keys by [`to_output`]: `status` (`0` = `SM_SUCCESS`,
//! else the `SmStatus`: `1001` `SM_ERR`, `1006` `SM_ERR_NULL_POINTER`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce) and the op's outputs: `derivs`, `domain` and `this_*` surface state,
//! `new_*` and `patch<i>_*` surface states (`flags`, `dims`, `cpts`, `u`, `v`).
//!
//! Cases whose Rust side reaches an unported callee are deferred: G1 corner blends
//! (the tangent field's `SmSurface::GetNaturalUVDomain`, `surface.surface_p2`;
//! `SmSurface::EvaluateNormal`, `surface.surface_p7`) and skins whose two rails change
//! their domains when synchronized (`SmCurve::LocalCurveIntersect`, `curve.curve_p3`). Every run has at
//! least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::{
    EvaluateFlags, OrientType, SkinnedSurfaceOptions,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_surface_n_lib_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `nurbs.b_spline_surface_n_lib_p2` runner.
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

fn default_degree() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Eval {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        u_left: bool,
        v_left: bool,
        upper: bool,
        nzt: bool,
    },
    Trim {
        dom: [f64; 4],
    },
    Skin {
        sections: Vec<CrvIn>,
        sync: bool,
        dir: u8,
        tol: f64,
        #[serde(default)]
        rail1: Option<Box<CrvIn>>,
        #[serde(default)]
        rail2: Option<Box<CrvIn>>,
        #[serde(default)]
        spine: bool,
        #[serde(default)]
        params: Option<Vec<f64>>,
        #[serde(default)]
        derivs: Option<Box<[Option<SrfIn>; 2]>>,
        #[serde(default = "default_degree")]
        degree: usize,
    },
    Corner {
        curves: Vec<CrvIn>,
        orients: Vec<u8>,
        uvs: Vec<Option<CrvIn>>,
        srfs: Vec<Option<SrfIn>>,
        tol: f64,
        ang: f64,
        bilinear: bool,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Eval { .. } => Op::Eval,
            OpIn::Trim { .. } => Op::Trim,
            OpIn::Skin { .. } => Op::Skin,
            OpIn::Corner { .. } => Op::Corner,
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
    o.extend([3.0, n as f64, c.p as f64, c.u.len().saturating_sub(1) as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
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
        OpIn::Eval {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            nzt,
        } => {
            o.extend_from_slice(uv);
            o.extend([
                *hu as f64,
                *hv as f64,
                flag(*u_left),
                flag(*v_left),
                flag(*upper),
                flag(*nzt),
            ]);
        }
        OpIn::Trim { dom } => o.extend_from_slice(dom),
        OpIn::Skin {
            sections,
            sync,
            dir,
            tol,
            rail1,
            rail2,
            spine,
            params,
            derivs,
            degree,
        } => {
            o.push(sections.len() as f64);
            for c in sections {
                push_crv(&mut o, &crv(c)?);
            }
            o.extend([flag(*sync), f64::from(*dir), *tol]);
            for r in [rail1, rail2] {
                o.push(flag(r.is_some()));
                if let Some(c) = r {
                    push_crv(&mut o, &crv(c)?);
                }
            }
            o.push(flag(*spine));
            o.push(flag(params.is_some()));
            if let Some(p) = params {
                o.push(p.len() as f64);
                o.extend_from_slice(p);
            }
            o.push(flag(derivs.is_some()));
            if let Some(ds) = derivs {
                for d in ds.iter() {
                    o.push(flag(d.is_some()));
                    if let Some(s) = d {
                        push_srf(&mut o, &srf(s)?)?;
                    }
                }
            }
            o.push(*degree as f64);
        }
        OpIn::Corner {
            curves,
            orients,
            uvs,
            srfs,
            tol,
            ang,
            bilinear,
        } => {
            o.push(curves.len() as f64);
            for c in curves {
                push_crv(&mut o, &crv(c)?);
            }
            for &k in orients {
                o.push(f64::from(k));
            }
            for u in uvs {
                o.push(flag(u.is_some()));
                if let Some(c) = u {
                    push_crv(&mut o, &crv(c)?);
                }
            }
            for s in srfs {
                o.push(flag(s.is_some()));
                if let Some(s) = s {
                    push_srf(&mut o, &srf(s)?)?;
                }
            }
            o.extend([*tol, *ang, flag(*bilinear)]);
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

    /// `has` then, when set, a surface state.
    fn new_state(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
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
        OpIn::Eval { hu, hv, .. } => {
            if ok {
                s.vals("derivs", 3 * (hu + 1) * (hv + 1))?;
            }
        }
        OpIn::Trim { .. } => {
            s.vals("domain", 4)?;
            s.state("this_")?;
        }
        OpIn::Skin { .. } => {
            if ok {
                s.new_state("new_")?;
            }
        }
        OpIn::Corner { .. } => {
            if ok {
                let k = s.count()?;
                s.put("patch_count", vec![k as f64]);
                for j in 0..k {
                    s.state(&format!("patch{j}_"))?;
                }
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

fn bss_of(s: &SrfIn) -> Result<BSplineSurface, HarnessError> {
    Ok(BSplineSurface::from_nurb(surface_of(&srf(s)?)?, false))
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
        bss_of(si)?
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

fn orient_of(v: u8) -> Result<OrientType, HarnessError> {
    Ok(match v {
        0 => OrientType::Unknown,
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => return Err(harness(format!("bad SmOrientType {v}"))),
    })
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

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

fn rust(k: &solidcore::kernel::Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut o = vec![0.0];
    let res: solidcore::Result<()> = match (&i.op, &i.this) {
        (
            OpIn::Eval {
                uv,
                hu,
                hv,
                u_left,
                v_left,
                upper,
                nzt,
            },
            Some(t),
        ) => {
            let this = this_of(t)?;
            let flags = EvaluateFlags {
                u_side: side(*u_left),
                v_side: side(*v_left),
                only_upper_half: *upper,
                non_zero_tangents: *nzt,
            };
            this.evaluate(Point2::new(uv[0], uv[1]), *hu, *hv, flags)
                .and_then(|d| {
                    for i in 0..=*hu {
                        for j in 0..=*hv {
                            let v = d
                                .get(i)
                                .and_then(|r| r.get(j))
                                .copied()
                                .ok_or_else(|| SolidError::sm_err("difftest: short matrix"))?;
                            o.extend([v.x, v.y, v.z]);
                        }
                    }
                    Ok(())
                })
        }
        (OpIn::Trim { dom }, Some(t)) => {
            let mut this = this_of(t)?;
            let mut d = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
            let r = this.trim_with_domain(&mut d);
            if let Err(e) = &r {
                if let Some(out) = Output::unported(e) {
                    return Ok(out);
                }
                o = vec![status(e)];
            }
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
            dump_state(&mut o, &this);
            return to_output(i, &o);
        }
        (
            OpIn::Skin {
                sections,
                sync,
                dir,
                tol,
                rail1,
                rail2,
                spine,
                params,
                derivs,
                degree,
            },
            None,
        ) => {
            let secs = sections
                .iter()
                .map(bsc_of)
                .collect::<Result<Vec<_>, _>>()?;
            let mut r1 = rail1.as_deref().map(bsc_of).transpose()?;
            let mut r2 = rail2.as_deref().map(bsc_of).transpose()?;
            let ds = match derivs.as_deref() {
                Some([a, b]) => Some([
                    a.as_ref().map(bss_of).transpose()?,
                    b.as_ref().map(bss_of).transpose()?,
                ]),
                None => None,
            };
            let opts = SkinnedSurfaceOptions {
                synchronized: *sync,
                cross_section_surf_param: param_of(*dir)?,
                approx_tol: *tol,
                rail1: r1.as_mut(),
                rail2: r2.as_mut(),
                rail1_used_as_spine_curve: *spine,
                intersections_with_rails: params.as_deref(),
                deriv_surfs: ds.as_ref().map(|[a, b]| [a.as_ref(), b.as_ref()]),
                degree: *degree,
            };
            BSplineSurface::create_skinned_surface(k, &secs, opts).map(|s| {
                o.push(1.0);
                dump_state(&mut o, &s);
            })
        }
        (
            OpIn::Corner {
                curves,
                orients,
                uvs,
                srfs,
                tol,
                ang,
                bilinear,
            },
            None,
        ) => {
            let c3 = curves.iter().map(bsc_of).collect::<Result<Vec<_>, _>>()?;
            let ors = orients
                .iter()
                .map(|&k| orient_of(k))
                .collect::<Result<Vec<_>, _>>()?;
            let uv = uvs
                .iter()
                .map(|u| u.as_ref().map(bsc_of).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            let ss = srfs
                .iter()
                .map(|s| s.as_ref().map(bss_of).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            let sr: Vec<Option<&BSplineSurface>> = ss.iter().map(Option::as_ref).collect();
            BSplineSurface::create_corner_blend(k, &c3, &ors, &uv, &sr, *tol, *ang, *bilinear).map(
                |patches| {
                    o.push(patches.len() as f64);
                    for p in &patches {
                        dump_state(&mut o, p);
                    }
                },
            )
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

/// Control point `[i][j]` of `s` (the origin past the end).
fn cp(s: &InlineSrf, i: usize, j: usize) -> [f64; 4] {
    s.net
        .get(i)
        .and_then(|r| r.get(j))
        .copied()
        .unwrap_or([0.0, 0.0, 0.0, NOW])
}

/// A clamped knot vector of degree `p` for `n + 1` control points over `[a, b]`.
fn gen_knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    if rng.chance(0.4) {
        inner = (1..=interior).map(|i| i as f64 / (interior + 1) as f64).collect();
    }
    inner.sort_by(f64::total_cmp);
    // keep interior knots distinct (multiplicity 1)
    let mut prev = f64::NEG_INFINITY;
    for t in &mut inner {
        if *t <= prev + 1e-3 {
            *t = prev + 1e-3;
        }
        prev = *t;
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

/// Collapses a side (or two) of `s` to a point: zero first derivatives there.
fn degenerate(rng: &mut Rng, s: &mut InlineSrf) {
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    let mode = rng.below(5);
    let collapse_row = |s: &mut InlineSrf, i: usize| {
        let pt = cp(s, i, 0);
        if let Some(row) = s.net.get_mut(i) {
            for p in row.iter_mut() {
                *p = pt;
            }
        }
    };
    let collapse_col = |s: &mut InlineSrf, j: usize| {
        let pt = cp(s, 0, j);
        for row in &mut s.net {
            if let Some(p) = row.get_mut(j) {
                *p = pt;
            }
        }
    };
    match mode {
        0 => collapse_row(s, 0),
        1 => collapse_row(s, n),
        2 => collapse_col(s, 0),
        3 => collapse_col(s, m),
        _ => {
            collapse_row(s, 0);
            collapse_col(s, 0);
        }
    }
    // a corner collapse makes everything around it shrink
    if rng.chance(0.2) {
        let pt = cp(s, 0, 0);
        for row in &mut s.net {
            for p in row.iter_mut() {
                *p = pt;
            }
        }
    }
}

fn uv_in(rng: &mut Rng, s: &InlineSrf, out: f64) -> [f64; 2] {
    let (a, b) = (kat(&s.u, 0), klast(&s.u));
    let (c, d) = (kat(&s.v, 0), klast(&s.v));
    let pick = |rng: &mut Rng, lo: f64, hi: f64, k: &[f64]| {
        if rng.chance(0.2) {
            // exactly on a knot or boundary
            *rng.choose(k).unwrap_or(&lo)
        } else {
            let e = out * (hi - lo);
            rng.uniform(lo - e, hi + e)
        }
    };
    [pick(rng, a, b, &s.u), pick(rng, c, d, &s.v)]
}

fn gen_eval(rng: &mut Rng) -> Inputs {
    let rational = rng.chance(0.2);
    let polynomial = rng.chance(0.1);
    let mut s = gen_srf(rng, rational && !polynomial, if polynomial { 2 } else { 4 });
    if !polynomial && rng.chance(0.4) {
        degenerate(rng, &mut s);
    }
    let form = if polynomial {
        BSplineSurfaceForm::Polynomial as u8
    } else if rng.chance(0.1) {
        rng.below(10) as u8
    } else {
        default_form()
    };
    let upper = rng.chance(0.25);
    let hu = if rng.chance(0.03) {
        5
    } else {
        rng.below(5) as usize
    };
    let hv = if upper && rng.chance(0.9) {
        hu
    } else {
        rng.below(5) as usize
    };
    let op = OpIn::Eval {
        uv: uv_in(rng, &s, 0.2),
        hu,
        hv,
        u_left: rng.chance(0.5),
        v_left: rng.chance(0.5),
        upper,
        nzt: rng.chance(0.8),
    };
    Inputs {
        this: Some(This {
            empty: false,
            oob: rng.chance(0.2),
            form,
            srf: Some(SrfIn::Inline(s)),
        }),
        op,
    }
}

fn gen_trim(rng: &mut Rng) -> Inputs {
    let rational = rng.chance(0.2);
    let s = gen_srf(rng, rational, 4);
    let pick = |rng: &mut Rng, k: &[f64]| -> (f64, f64) {
        let (a, b) = (kat(k, 0), klast(k));
        let len = b - a;
        let near = |rng: &mut Rng| {
            let kn = *rng.choose(k).unwrap_or(&a);
            let eps = *rng.choose(&[0.0, 1e-12, -1e-12, 1e-9, -1e-9, 3e-8, -3e-8]).unwrap_or(&0.0);
            (kn + eps).clamp(a, b)
        };
        let mut lo = if rng.chance(0.4) {
            near(rng)
        } else {
            rng.uniform(a, b)
        };
        let mut hi = if rng.chance(0.4) {
            near(rng)
        } else {
            rng.uniform(a, b)
        };
        if lo > hi {
            core::mem::swap(&mut lo, &mut hi);
        }
        match rng.below(12) {
            0 => (a, b),
            1 => (lo, lo + 1e-9 * len),
            2 => (a - 0.1 * len, hi),
            _ => (lo, hi),
        }
    };
    let (ul, ur) = pick(rng, &s.u);
    let (vl, vr) = pick(rng, &s.v);
    let empty = rng.chance(0.03);
    Inputs {
        this: Some(This {
            empty,
            oob: rng.chance(0.1),
            form: default_form(),
            srf: if empty {
                None
            } else {
                Some(SrfIn::Inline(s))
            },
        }),
        op: OpIn::Trim {
            dom: [ul, vl, ur, vr],
        },
    }
}

/// Row `i` of a net as a curve with knots `v`.
fn row_curve(s: &InlineSrf, i: usize) -> InlineCrv {
    InlineCrv {
        cpts: s.net.get(i).cloned().unwrap_or_default(),
        p: s.q,
        u: s.v.clone(),
    }
}

/// A degree-1 curve through `pts` with knots at `ts` (interpolating).
fn polyline(pts: &[[f64; 4]], ts: &[f64]) -> InlineCrv {
    let mut u = vec![kat(ts, 0)];
    u.extend_from_slice(ts);
    u.push(klast(ts));
    InlineCrv {
        cpts: pts.to_vec(),
        p: 1,
        u,
    }
}

/// Increasing parameters `t0 < t1 < ...` over `[a, b]`.
fn gen_params(rng: &mut Rng, k: usize, a: f64, b: f64) -> Vec<f64> {
    if k == 1 {
        return vec![a];
    }
    let mut t: Vec<f64> = (0..k)
        .map(|i| {
            let base = i as f64 / (k - 1) as f64;
            let jitter = if i == 0 || i == k - 1 {
                0.0
            } else {
                rng.uniform(-0.2, 0.2) / (k - 1) as f64
            };
            a + (b - a) * (base + jitter)
        })
        .collect();
    t.sort_by(f64::total_cmp);
    t
}

fn gen_skin(rng: &mut Rng) -> Inputs {
    let mut s = gen_srf(rng, false, 3);
    // cross sections: the rows of the net (compatible curves in v)
    let n = s.net.len() - 1;
    let k = (1 + rng.below(n as u64 + 1) as usize).max(if rng.chance(0.9) { 2 } else { 1 });
    let k = k.min(n + 1);
    if rng.chance(0.15) {
        for row in &mut s.net {
            for p in row.iter_mut() {
                let w = rng.uniform(0.5, 2.0);
                *p = [w * p[0], w * p[1], w * p[2], w];
            }
        }
    }
    let mut sections: Vec<InlineCrv> = (0..k).map(|i| row_curve(&s, i)).collect();
    let mut sync = rng.chance(0.6);
    if !sync && rng.chance(0.5) {
        // incompatible sections: other knots / degrees
        for c in sections.iter_mut().skip(1) {
            if rng.chance(0.5) {
                let nn = c.cpts.len() - 1;
                c.u = gen_knots(rng, nn, c.p, kat(&c.u, 0), klast(&c.u));
            }
        }
    }
    let (a, b) = gen_domain(rng);
    let ts = gen_params(rng, k, a, b);
    let m = s.net.first().map_or(0, Vec::len) - 1;
    let start_pts: Vec<[f64; 4]> = sections.iter().map(|c| *c.cpts.first().unwrap_or(&[0.0; 4])).collect();
    let end_pts: Vec<[f64; 4]> = sections.iter().map(|c| *c.cpts.get(m).unwrap_or(&[0.0; 4])).collect();
    let rational = start_pts.first().is_some_and(|p| p[3] != NOW);
    let mode = rng.below(10);
    let mut rail1 = None;
    let mut rail2 = None;
    let mut spine = false;
    let mut params = None;
    let mut derivs = None;
    if !rational && k >= 2 {
        match mode {
            0 | 1 => {
                rail1 = Some(polyline(&start_pts, &ts));
                params = Some(ts.clone());
            }
            2 | 3 => {
                rail1 = Some(polyline(&start_pts, &ts));
                // the second rail on another domain makes the sync reparametrise
                let ts2 = if rng.chance(0.3) {
                    ts.iter().map(|t| 2.0 * t + 1.0).collect()
                } else {
                    ts.clone()
                };
                rail2 = Some(polyline(&end_pts, &ts2));
                let mut all = ts.clone();
                all.extend_from_slice(&ts2);
                params = Some(all);
            }
            4 => {
                rail1 = Some(polyline(&start_pts, &ts));
                spine = true;
                params = Some(ts.clone());
            }
            5 | 6 => {
                // derivative surfaces: two rows of derivative vectors over the
                // sections' knots
                sync = false;
                let mk = |rng: &mut Rng, c: &InlineCrv| InlineSrf {
                    net: c
                        .cpts
                        .iter()
                        .map(|_| {
                            let d = rng.point3(-1.0, 1.0);
                            vec![[d[0], d[1], d[2] + 1.0, NOW], [d[0], d[1], d[2] + 1.0, NOW]]
                        })
                        .collect(),
                    p: c.p,
                    q: 1,
                    u: c.u.clone(),
                    v: vec![0.0, 0.0, 1.0, 1.0],
                };
                let d0 = if rng.chance(0.8) {
                    sections.first().map(|c| SrfIn::Inline(mk(rng, c)))
                } else {
                    None
                };
                let d1 = if rng.chance(0.6) {
                    sections.last().map(|c| SrfIn::Inline(mk(rng, c)))
                } else {
                    None
                };
                derivs = Some([d0, d1]);
                if rng.chance(0.3) {
                    params = Some(ts.clone());
                }
            }
            7 => {
                params = Some(ts.clone());
            }
            _ => {}
        }
    }
    Inputs {
        this: None,
        op: OpIn::Skin {
            sections: sections.into_iter().map(CrvIn::Inline).collect(),
            sync,
            dir: u8::from(!rng.chance(0.5)),
            tol: *rng.choose(&[0.0, 0.0, 1e-3, 1e-2]).unwrap_or(&0.0),
            rail1: rail1.map(|c| Box::new(CrvIn::Inline(c))),
            rail2: rail2.map(|c| Box::new(CrvIn::Inline(c))),
            spine,
            params,
            derivs: derivs.map(Box::new),
            degree: 1 + rng.below(4) as usize,
        },
    }
}

/// Reverses a curve's direction (control points and knots).
fn reversed(c: &InlineCrv) -> InlineCrv {
    let (a, b) = (kat(&c.u, 0), klast(&c.u));
    let mut u: Vec<f64> = c.u.iter().rev().map(|t| a + b - t).collect();
    for t in &mut u {
        *t = t.clamp(a, b);
    }
    InlineCrv {
        cpts: c.cpts.iter().rev().copied().collect(),
        p: c.p,
        u,
    }
}

fn col_curve(s: &InlineSrf, j: usize) -> InlineCrv {
    InlineCrv {
        cpts: (0..s.net.len()).map(|i| cp(s, i, j)).collect(),
        p: s.p,
        u: s.u.clone(),
    }
}

/// A plane over the unit square (degree 1 in both directions).
fn unit_plane(rng: &mut Rng) -> InlineSrf {
    let z = rng.uniform(-0.5, 0.5);
    let tilt = rng.uniform(-0.3, 0.3);
    InlineSrf {
        net: vec![
            vec![[0.0, 0.0, z, NOW], [0.0, 4.0, z + 4.0 * tilt, NOW]],
            vec![[4.0, 0.0, z, NOW], [4.0, 4.0, z + 4.0 * tilt, NOW]],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A line from `a` to `b` (degree 1 or 2) over `[0, 1]`.
fn line(rng: &mut Rng, a: [f64; 3], b: [f64; 3]) -> InlineCrv {
    let mid = [
        0.5 * (a[0] + b[0]),
        0.5 * (a[1] + b[1]),
        0.5 * (a[2] + b[2]),
    ];
    if rng.chance(0.5) {
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

/// A loop of `k` lines on a plane (3D curves, UV curves, the plane), CCW.
fn plane_loop(rng: &mut Rng, k: usize) -> (Vec<InlineCrv>, Vec<InlineCrv>, InlineSrf) {
    let pl = unit_plane(rng);
    let z0 = cp(&pl, 0, 0)[2];
    let tilt = (cp(&pl, 0, 1)[2] - z0) / 4.0;
    let uvs: Vec<[f64; 2]> = (0..k)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / k as f64 + std::f64::consts::PI;
            [0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()]
        })
        .collect();
    let to3 = |uv: [f64; 2]| [4.0 * uv[0], 4.0 * uv[1], z0 + 4.0 * uv[1] * tilt];
    let mut c3 = Vec::with_capacity(k);
    let mut c2 = Vec::with_capacity(k);
    for i in 0..k {
        let a = uvs.get(i).copied().unwrap_or([0.0; 2]);
        let b = uvs.get((i + 1) % k).copied().unwrap_or([0.0; 2]);
        let l3 = line(rng, to3(a), to3(b));
        let l2 = InlineCrv {
            cpts: l3
                .cpts
                .iter()
                .map(|p| [p[0] / 4.0, p[1] / 4.0, 0.0, NOW])
                .collect(),
            p: l3.p,
            u: l3.u.clone(),
        };
        c3.push(l3);
        c2.push(l2);
    }
    (c3, c2, pl)
}

fn gen_corner(rng: &mut Rng) -> Inputs {
    let mode = rng.below(10);
    let tol = *rng.choose(&[1e-3, 1e-2]).unwrap_or(&1e-3);
    let ang = *rng.choose(&[0.01, 0.05]).unwrap_or(&0.01);
    if mode <= 5 {
        // bilinear Coons patch through the boundaries of a surface
        let s = gen_srf(rng, false, 3);
        let n = s.net.len() - 1;
        let m = s.net.first().map_or(0, Vec::len) - 1;
        // [left, bottom, right, top] in natural parametrisation: left (u = min,
        // along v) and top (v = max, along u) run against the CCW order
        let mut curves = vec![row_curve(&s, 0), col_curve(&s, 0), row_curve(&s, n), col_curve(&s, m)];
        let mut orients = vec![2u8, 1, 1, 2];
        for (c, o) in curves.iter_mut().zip(orients.iter_mut()) {
            if rng.chance(0.3) {
                *c = reversed(c);
                *o = 3 - *o;
            }
            if rng.chance(0.05) {
                *o = 0;
            }
        }
        if rng.chance(0.15) {
            // a rational boundary (approximated by a non-rational one)
            if let Some(c) = curves.get_mut(rng.below(4) as usize) {
                for p in &mut c.cpts {
                    p[3] = 1.0;
                }
                if let Some(p) = c.cpts.get_mut(1) {
                    let w = 1.3;
                    *p = [w * p[0], w * p[1], w * p[2], w];
                }
            }
        }
        let k = if rng.chance(0.05) { 3 } else { 4 };
        curves.truncate(k);
        orients.truncate(k);
        let bilinear = rng.chance(0.85);
        return Inputs {
            this: None,
            op: OpIn::Corner {
                curves: curves.into_iter().map(CrvIn::Inline).collect(),
                orients,
                uvs: vec![None; k],
                srfs: vec![None; k],
                tol,
                ang,
                bilinear,
            },
        };
    }
    // G1 blends on a plane: 4 (Coons) or 5/6 (n-sided) sides
    let k = if mode <= 7 { 4 } else { 5 + rng.below(2) as usize };
    let (c3, c2, pl) = plane_loop(rng, k);
    let mut uvs: Vec<Option<CrvIn>> = c2.into_iter().map(|c| Some(CrvIn::Inline(c))).collect();
    let mut srfs: Vec<Option<SrfIn>> = (0..k).map(|_| Some(SrfIn::Inline(pl.clone()))).collect();
    // drop some neighbours (an NER error, or one triangle side)
    let drops = rng.below(3) as usize;
    for _ in 0..drops {
        let i = rng.below(k as u64) as usize;
        if let (Some(u), Some(s)) = (uvs.get_mut(i), srfs.get_mut(i)) {
            *u = None;
            *s = None;
        }
    }
    Inputs {
        this: None,
        op: OpIn::Corner {
            curves: c3.into_iter().map(CrvIn::Inline).collect(),
            orients: vec![1; k],
            uvs,
            srfs,
            tol,
            ang,
            bilinear: false,
        },
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0..=3 => gen_eval(rng),
        4 | 5 => gen_trim(rng),
        6 | 7 => gen_skin(rng),
        _ => gen_corner(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.b_spline_surface_n_lib_p2"
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
let k = &solidcore::Kernel::default();
        rust(k, &case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach success and failure of every op on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
let k = &solidcore::Kernel::default();
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..600u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let op = match &i.op {
                OpIn::Skin {
                    rail1,
                    rail2,
                    spine,
                    derivs,
                    ..
                } => format!(
                    "Skin r{}{} s{} d{}",
                    u8::from(rail1.is_some()),
                    u8::from(rail2.is_some()),
                    u8::from(*spine),
                    u8::from(derivs.is_some())
                ),
                _ => format!("{:?}", i.op.ffi()),
            };
            let out = rust(k, &i).expect("harness");
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
        for op in ["Eval", "Trim", "Skin", "Corner"] {
            let ok = tally
                .iter()
                .any(|((o, k), _)| o.starts_with(op) && k == "status:0");
            assert!(ok, "{op} never succeeds");
        }
    }
}
