//! Runner for unit `nurbs.b_spline_surface_n_lib_p1`: the `SmBSplineSurface` methods
//! implemented with NLib (offset approximation, Coons, n-sided, ruled, sweep and swung
//! surfaces, extension, joining, reversal, reparametrisation, boundary span expansion,
//! the evaluators) and the file-static helpers `sm_CreateNonRationalCurve`,
//! `sm_MakeKnots`, `sm_CheckTrimBoundaries` and `sm_check_zero_surface_derivs`.
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_surface_n_lib_p1` (upstream
//! `source/SMLib/src/SmBSplineSurfaceNLib.cpp`). Rust:
//! `solidcore::nurbs::b_spline_surface_n_lib::p1`.
//!
//! A case is `this` (the surface a member op runs on: `empty` for a surface without
//! NURB, `oob`, `form` and an inline or `.sms` surface), absent for the static ops,
//! plus `op` and its parameters. Both sides produce the same flat record (layout in the
//! FFI module docs), split into keys by [`to_output`]: `status` (`0` = `SM_SUCCESS`,
//! else the `SmStatus`: `1001` `SM_ERR`, `1006` `SM_ERR_NULL_POINTER`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce) and the op's outputs on success: surface states (`new_*`, `this_*`,
//! `patch<i>_*`: `flags`, `dims`, `cpts`, `u`, `v`), curve states, points,
//! derivatives, knots.
//!
//! Cases whose Rust side reaches an unported `SmSurface` method are deferred (none
//! now: `CreateExtendedSurface` and `sm_CreateNonRationalCurve`'s `DropCurve`, unit
//! `intersect.surface_drop_lib_p3`, are ported). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p1::{
    check_trim_boundaries, check_zero_surface_derivs, create_non_rational_curve, make_knots,
    OffsetParamType,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_surface_n_lib_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `nurbs.b_spline_surface_n_lib_p1` runner.
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
    Offset {
        dist: f64,
        tol: f64,
        #[serde(default)]
        param: u8,
    },
    Extend {
        dir_overload: bool,
        dir: u8,
        dist: f64,
        cont: u8,
    },
    Join {
        other: SrfIn,
        dir: u8,
        tol: f64,
    },
    ExtEval {
        uv: [f64; 2],
    },
    Reparam {
        dom: [f64; 4],
    },
    Reverse {
        dir: u8,
    },
    Expand {
        param: f64,
        dir: u8,
    },
    EvalSimple {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        u_left: bool,
        v_left: bool,
        upper: bool,
    },
    EvalPoint {
        uv: [f64; 2],
    },
    Coons {
        tol: f64,
        bilinear: bool,
        bdry: Vec<CrvIn>,
        #[serde(default)]
        ders: Vec<CrvIn>,
    },
    NSided {
        bdry: Vec<CrvIn>,
        ders: Vec<Option<CrvIn>>,
        eps: f64,
        #[serde(default)]
        center: Option<[[f64; 3]; 2]>,
    },
    Ruled {
        c1: CrvIn,
        c2: CrvIn,
        dir: u8,
    },
    Sweep {
        section: CrvIn,
        traj: CrvIn,
        #[serde(default)]
        scale: Option<CrvIn>,
        trans: bool,
        tol: f64,
    },
    Swung {
        section: CrvIn,
        traj: CrvIn,
        scale: f64,
    },
    NonRat {
        curve: CrvIn,
        #[serde(default)]
        uv: Option<CrvIn>,
        #[serde(default)]
        base: Option<SrfIn>,
        tol: f64,
    },
    MakeKnots {
        n: usize,
        p: usize,
        u: Vec<f64>,
    },
    TrimBounds {
        knots: Vec<f64>,
        tmin: f64,
        min_idx: Option<usize>,
        tmax: f64,
        max_idx: Option<usize>,
        dom: [f64; 4],
        dir: u8,
    },
    ZeroDerivs {
        uv: [f64; 2],
        dom: [f64; 4],
        pt: [f64; 3],
        su: [f64; 3],
        sv: [f64; 3],
        suu: [f64; 3],
        suv: [f64; 3],
        svv: [f64; 3],
        sz: f64,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Offset { .. } => Op::Offset,
            OpIn::Extend { .. } => Op::Extend,
            OpIn::Join { .. } => Op::Join,
            OpIn::ExtEval { .. } => Op::ExtEval,
            OpIn::Reparam { .. } => Op::Reparam,
            OpIn::Reverse { .. } => Op::Reverse,
            OpIn::Expand { .. } => Op::Expand,
            OpIn::EvalSimple { .. } => Op::EvalSimple,
            OpIn::EvalPoint { .. } => Op::EvalPoint,
            OpIn::Coons { .. } => Op::Coons,
            OpIn::NSided { .. } => Op::NSided,
            OpIn::Ruled { .. } => Op::Ruled,
            OpIn::Sweep { .. } => Op::Sweep,
            OpIn::Swung { .. } => Op::Swung,
            OpIn::NonRat { .. } => Op::NonRat,
            OpIn::MakeKnots { .. } => Op::MakeKnots,
            OpIn::TrimBounds { .. } => Op::TrimBounds,
            OpIn::ZeroDerivs { .. } => Op::ZeroDerivs,
        }
    }

    /// Whether the op outputs a new surface (`has` + state).
    fn creates(&self) -> bool {
        matches!(
            self,
            OpIn::Offset { .. }
                | OpIn::Extend { .. }
                | OpIn::Join { .. }
                | OpIn::Coons { .. }
                | OpIn::Ruled { .. }
                | OpIn::Sweep { .. }
                | OpIn::Swung { .. }
        )
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

#[path = "fixtures/nurbs_b_spline_surface_n_lib_p1_sms.rs"]
mod sms;

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let sms::ParsedSrf { net, p, q, u, v } = sms::decode(&text, path)?;
    Ok(InlineSrf { net, p, q, u, v })
}

fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let c = crate::fixtures::text_curve::read_3d(path)?;
    Ok(InlineCrv {
        cpts: c.cpts,
        p: c.p,
        u: c.u,
    })
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
    let n = s
        .net
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

// ---- C++ side -----------------------------------------------------------------------

fn push_srf(o: &mut Vec<f64>, s: &InlineSrf) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend(
        [
            n,
            m,
            s.p,
            s.q,
            s.u.len().saturating_sub(1),
            s.v.len().saturating_sub(1),
        ]
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
    o.extend([
        3.0,
        n as f64,
        c.p as f64,
        c.u.len().saturating_sub(1) as f64,
    ]);
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
        OpIn::Offset { dist, tol, param } => o.extend([*dist, *tol, f64::from(*param)]),
        OpIn::Extend {
            dir_overload,
            dir,
            dist,
            cont,
        } => o.extend([
            flag(*dir_overload),
            f64::from(*dir),
            *dist,
            f64::from(*cont),
        ]),
        OpIn::Join { other, dir, tol } => {
            push_srf(&mut o, &srf(other)?)?;
            o.extend([f64::from(*dir), *tol]);
        }
        OpIn::ExtEval { uv } | OpIn::EvalPoint { uv } => o.extend_from_slice(uv),
        OpIn::Reparam { dom } => o.extend_from_slice(dom),
        OpIn::Reverse { dir } => o.push(f64::from(*dir)),
        OpIn::Expand { param, dir } => o.extend([*param, f64::from(*dir)]),
        OpIn::EvalSimple {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
        } => {
            o.extend_from_slice(uv);
            o.extend([
                *hu as f64,
                *hv as f64,
                flag(*u_left),
                flag(*v_left),
                flag(*upper),
            ]);
        }
        OpIn::Coons {
            tol,
            bilinear,
            bdry,
            ders,
        } => {
            o.extend([*tol, flag(*bilinear)]);
            for c in bdry {
                push_crv(&mut o, &crv(c)?);
            }
            o.push(ders.len() as f64);
            for c in ders {
                push_crv(&mut o, &crv(c)?);
            }
        }
        OpIn::NSided {
            bdry,
            ders,
            eps,
            center,
        } => {
            o.push(bdry.len() as f64);
            for c in bdry {
                push_crv(&mut o, &crv(c)?);
            }
            for d in ders {
                o.push(flag(d.is_some()));
                if let Some(c) = d {
                    push_crv(&mut o, &crv(c)?);
                }
            }
            o.push(*eps);
            o.push(flag(center.is_some()));
            let [c, nrm] = center.unwrap_or([[0.0; 3]; 2]);
            o.extend_from_slice(&c);
            o.extend_from_slice(&nrm);
        }
        OpIn::Ruled { c1, c2, dir } => {
            push_crv(&mut o, &crv(c1)?);
            push_crv(&mut o, &crv(c2)?);
            o.push(f64::from(*dir));
        }
        OpIn::Sweep {
            section,
            traj,
            scale,
            trans,
            tol,
        } => {
            push_crv(&mut o, &crv(section)?);
            push_crv(&mut o, &crv(traj)?);
            o.push(flag(scale.is_some()));
            if let Some(c) = scale {
                push_crv(&mut o, &crv(c)?);
            }
            o.extend([flag(*trans), *tol]);
        }
        OpIn::Swung {
            section,
            traj,
            scale,
        } => {
            push_crv(&mut o, &crv(section)?);
            push_crv(&mut o, &crv(traj)?);
            o.push(*scale);
        }
        OpIn::NonRat {
            curve,
            uv,
            base,
            tol,
        } => {
            push_crv(&mut o, &crv(curve)?);
            o.push(flag(uv.is_some()));
            if let Some(c) = uv {
                push_crv(&mut o, &crv(c)?);
            }
            o.push(flag(base.is_some()));
            if let Some(s) = base {
                push_srf(&mut o, &srf(s)?)?;
            }
            o.push(*tol);
        }
        OpIn::MakeKnots { n, p, u } => {
            o.extend([*n as f64, *p as f64]);
            o.extend_from_slice(u);
        }
        OpIn::TrimBounds {
            knots,
            tmin,
            min_idx,
            tmax,
            max_idx,
            dom,
            dir,
        } => {
            o.push(knots.len() as f64);
            o.extend_from_slice(knots);
            o.extend([
                *tmin,
                min_idx.map_or(0.0, |k| (k + 1) as f64),
                *tmax,
                max_idx.map_or(0.0, |k| (k + 1) as f64),
            ]);
            o.extend_from_slice(dom);
            o.push(f64::from(*dir));
        }
        OpIn::ZeroDerivs {
            uv,
            dom,
            pt,
            su,
            sv,
            suu,
            suv,
            svv,
            sz,
        } => {
            o.extend_from_slice(uv);
            o.extend_from_slice(dom);
            for v in [pt, su, sv, suu, suv, svv] {
                o.extend_from_slice(v);
            }
            o.push(*sz);
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
            self.vals(
                &format!("{pfx}cpts"),
                4 * (n as usize + 1) * (m as usize + 1),
            )?;
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
            self.vals(&format!("{pfx}u"), m as usize + 1)?;
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

    /// `has` then, when set, a curve state.
    fn new_curve(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
            self.curve_state(pfx)?;
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
    if ok {
        match &i.op {
            _ if i.op.creates() => s.new_state("new_")?,
            OpIn::ExtEval { .. } | OpIn::EvalPoint { .. } => s.vals("point", 3)?,
            OpIn::Reparam { .. } | OpIn::Reverse { .. } => s.state("this_")?,
            OpIn::EvalSimple { hu, hv, .. } => s.vals("derivs", 3 * (hu + 1) * (hv + 1))?,
            OpIn::NSided { .. } => {
                let k = s.count()?;
                s.put("patch_count", vec![k as f64]);
                for j in 0..k {
                    s.state(&format!("patch{j}_"))?;
                }
                s.vals("normal", 3)?;
            }
            OpIn::NonRat { .. } => {
                s.new_curve("curve_")?;
                s.new_curve("uv_")?;
            }
            OpIn::MakeKnots { n, p, .. } => s.vals("knots", n + p + 2)?,
            OpIn::TrimBounds { .. } => s.vals("bounds", 2)?,
            OpIn::ZeroDerivs { .. } => s.vals("result", 7)?,
            _ => {}
        }
    }
    if let OpIn::Expand { .. } = i.op {
        s.state("this_")?;
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
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
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

fn continuity_of(v: u8) -> Result<ContinuityType, HarnessError> {
    Ok(match v {
        0 => ContinuityType::Undefined,
        1 => ContinuityType::Discontinuous,
        2 => ContinuityType::C0,
        3 => ContinuityType::G1,
        4 => ContinuityType::G1R,
        5 => ContinuityType::G1G2,
        6 => ContinuityType::G1G2G3,
        7 => ContinuityType::C1,
        8 => ContinuityType::C1G2,
        9 => ContinuityType::C1G2G3,
        10 => ContinuityType::C1C2,
        11 => ContinuityType::C1C2G3,
        12 => ContinuityType::C1C2C3,
        13 => ContinuityType::CInfinity,
        _ => return Err(harness(format!("bad SmContinuityType {v}"))),
    })
}

fn offset_param_of(v: u8) -> Result<OffsetParamType, HarnessError> {
    Ok(match v {
        0 => OffsetParamType::Inherited,
        1 => OffsetParamType::ChordLength,
        2 => OffsetParamType::Centripetal,
        3 => OffsetParamType::Uniform,
        _ => return Err(harness(format!("bad offset parametrisation {v}"))),
    })
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A Rust error as a record, or the deferral of an unported call.
enum Fail {
    Status(f64),
    Deferred(Box<Output>),
}

fn fail(e: &SolidError) -> Fail {
    match Output::unported(e) {
        Some(o) => Fail::Deferred(Box::new(o)),
        None => Fail::Status(status(e)),
    }
}

/// Runs a member op; returns the flat record.
fn rust_member(k: &solidcore::kernel::Kernel, this: &mut BSplineSurface, op: &OpIn) -> Result<Result<Vec<f64>, Fail>, HarnessError> {
    let mut o = vec![0.0];
    let new = |o: &mut Vec<f64>, r: solidcore::Result<BSplineSurface>| match r {
        Ok(s) => {
            o.push(1.0);
            dump_state(o, &s);
            Ok(())
        }
        Err(e) => Err(fail(&e)),
    };
    let res: Result<(), Fail> = match op {
        OpIn::Offset { dist, tol, param } => new(
            &mut o,
            this.approximate_offset_surface(*dist, *tol, offset_param_of(*param)?),
        ),
        OpIn::Extend {
            dir_overload,
            dir,
            dist,
            cont,
        } => {
            let cont = continuity_of(*cont)?;
            let r = if *dir_overload {
                this.create_extended_surface_dir(k, param_of(*dir)?, *dist, cont)
            } else {
                this.create_extended_surface(k, *dist, cont)
            };
            new(&mut o, r)
        }
        OpIn::Join { other, dir, tol } => {
            let mut other = BSplineSurface::from_nurb(surface_of(&srf(other)?)?, false);
            new(&mut o, this.join_surface(&mut other, param_of(*dir)?, *tol))
        }
        OpIn::ExtEval { uv } => {
            match this.evaluate_point_on_extended_surface(Point2::new(uv[0], uv[1])) {
                Ok(p) => {
                    push3(&mut o, p);
                    Ok(())
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::EvalPoint { uv } => match this.evaluate_point(Point2::new(uv[0], uv[1])) {
            Ok(p) => {
                push3(&mut o, p);
                Ok(())
            }
            Err(e) => Err(fail(&e)),
        },
        OpIn::Reparam { dom } => {
            match this.reparameterize(&Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3])) {
                Ok(()) => {
                    dump_state(&mut o, this);
                    Ok(())
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::Reverse { dir } => match this.reverse(param_of(*dir)?) {
            Ok(()) => {
                dump_state(&mut o, this);
                Ok(())
            }
            Err(e) => Err(fail(&e)),
        },
        OpIn::Expand { param, dir } => {
            let r = this.expand_boundary_span(*param, param_of(*dir)?);
            match r {
                Ok(()) => {
                    dump_state(&mut o, this);
                    Ok(())
                }
                Err(SolidError::Upstream { code, .. }) => {
                    let mut f = vec![f64::from(code)];
                    dump_state(&mut f, this);
                    return Ok(Ok(f));
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::EvalSimple {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
        } => match this.evaluate_simple(
            Point2::new(uv[0], uv[1]),
            *hu,
            *hv,
            *u_left,
            *v_left,
            *upper,
        ) {
            Ok(d) => {
                for i in 0..=*hu {
                    for j in 0..=*hv {
                        let v = d
                            .get(i)
                            .and_then(|r| r.get(j))
                            .copied()
                            .ok_or_else(|| harness("derivative matrix too small"))?;
                        push3(&mut o, v);
                    }
                }
                Ok(())
            }
            Err(e) => Err(fail(&e)),
        },
        _ => return Err(harness("not a member op")),
    };
    Ok(res.map(|()| o))
}

/// Runs a static op; returns the flat record.
fn rust_static(op: &OpIn) -> Result<Result<Vec<f64>, Fail>, HarnessError> {
    let mut o = vec![0.0];
    let new = |o: &mut Vec<f64>, r: solidcore::Result<BSplineSurface>| match r {
        Ok(s) => {
            o.push(1.0);
            dump_state(o, &s);
            Ok(())
        }
        Err(e) => Err(fail(&e)),
    };
    let res: Result<(), Fail> = match op {
        OpIn::Coons {
            tol,
            bilinear,
            bdry,
            ders,
        } => {
            let mut b = bdry
                .iter()
                .map(|c| curve_of(&crv(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            let mut d = ders
                .iter()
                .map(|c| curve_of(&crv(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            new(
                &mut o,
                BSplineSurface::create_coons_patch(*tol, *bilinear, &mut b, &mut d),
            )
        }
        OpIn::NSided {
            bdry,
            ders,
            eps,
            center,
        } => {
            let mut b = bdry
                .iter()
                .map(|c| curve_of(&crv(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            let mut d = ders
                .iter()
                .map(|c| c.as_ref().map(|c| curve_of(&crv(c)?)).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            let mut normal = center.map_or(Vec3::new(0.0, 0.0, 0.0), |c| v3(c[1]));
            let ctr = center.map(|c| (v3(c[0]), &mut normal));
            match BSplineSurface::create_n_sided_patch(&mut b, &mut d, *eps, ctr) {
                Ok(patches) => {
                    o.push(patches.len() as f64);
                    for p in &patches {
                        dump_state(&mut o, p);
                    }
                    push3(&mut o, normal);
                    Ok(())
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::Ruled { c1, c2, dir } => new(
            &mut o,
            BSplineSurface::create_ruled_surface(
                &curve_of(&crv(c1)?)?,
                &curve_of(&crv(c2)?)?,
                param_of(*dir)?,
            ),
        ),
        OpIn::Sweep {
            section,
            traj,
            scale,
            trans,
            tol,
        } => {
            let scale = scale.as_ref().map(|c| curve_of(&crv(c)?)).transpose()?;
            new(
                &mut o,
                BSplineSurface::create_sweep_surface(
                    &curve_of(&crv(section)?)?,
                    &curve_of(&crv(traj)?)?,
                    scale.as_ref(),
                    *trans,
                    *tol,
                ),
            )
        }
        OpIn::Swung {
            section,
            traj,
            scale,
        } => new(
            &mut o,
            BSplineSurface::create_swung_surface(
                &curve_of(&crv(section)?)?,
                &curve_of(&crv(traj)?)?,
                *scale,
            ),
        ),
        OpIn::NonRat {
            curve,
            uv,
            base,
            tol,
        } => {
            let c = curve_of(&crv(curve)?)?;
            let uv = uv.as_ref().map(|c| curve_of(&crv(c)?)).transpose()?;
            let base = base
                .as_ref()
                .map(
                    |s| -> Result<solidcore::surface::surface::Surface, HarnessError> {
                        Ok(BSplineSurface::from_nurb(surface_of(&srf(s)?)?, false).into())
                    },
                )
                .transpose()?;
            let k = solidcore::kernel::Kernel::default();
            match create_non_rational_curve(&k, &c, uv.as_ref(), base.as_ref(), *tol) {
                Ok((nc, nuv)) => {
                    o.push(1.0);
                    dump_curve(&mut o, &nc);
                    o.push(flag(nuv.is_some()));
                    if let Some(u) = &nuv {
                        dump_curve(&mut o, u);
                    }
                    Ok(())
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::MakeKnots { n, p, u } => {
            // the caller's knot array; absurd counts (fuzz input the guard rejects)
            // are a harness error instead of an allocation abort
            let len = n
                .checked_add(*p)
                .and_then(|l| l.checked_add(2))
                .filter(|&l| l <= u.len().saturating_add(64).max(4096))
                .ok_or_else(|| harness("make_knots: knot count too large"))?;
            let mut k = vec![0.0; len];
            match make_knots(u, *n, *p, &mut k) {
                Ok(()) => {
                    o.extend_from_slice(&k);
                    Ok(())
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::TrimBounds {
            knots,
            tmin,
            min_idx,
            tmax,
            max_idx,
            dom,
            dir,
        } => {
            let d = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
            match check_trim_boundaries(
                knots,
                *tmin,
                *min_idx,
                *tmax,
                *max_idx,
                &d,
                param_of(*dir)?,
            ) {
                Ok((a, b)) => {
                    o.extend([a, b]);
                    Ok(())
                }
                Err(e) => Err(fail(&e)),
            }
        }
        OpIn::ZeroDerivs {
            uv,
            dom,
            su,
            sv,
            suu,
            suv,
            svv,
            sz,
            ..
        } => {
            let d = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
            let (mut a, mut b) = (v3(*su), v3(*sv));
            let ok = check_zero_surface_derivs(
                Point2::new(uv[0], uv[1]),
                &d,
                &mut a,
                &mut b,
                v3(*suu),
                v3(*suv),
                v3(*svv),
                *sz,
            );
            o.push(flag(ok));
            push3(&mut o, a);
            push3(&mut o, b);
            Ok(())
        }
        _ => return Err(harness("not a static op")),
    };
    Ok(res.map(|()| o))
}

fn rust(k: &solidcore::kernel::Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let res = match &i.this {
        Some(t) => {
            let mut this = this_of(t)?;
            rust_member(k, &mut this, &i.op)?
        }
        None => rust_static(&i.op)?,
    };
    match res {
        Ok(flat) => to_output(i, &flat),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Status(s)) => {
            let mut flat = vec![s];
            if let (OpIn::Expand { .. }, Some(t)) = (&i.op, &i.this) {
                dump_state(&mut flat, &this_of(t)?);
            }
            to_output(i, &flat)
        }
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
        inner = (1..=interior)
            .map(|i| i as f64 / (interior + 1) as f64)
            .collect();
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

/// A 3D curve from `a` to `b` with bulged interior points, optionally rational.
fn gen_crv_between(rng: &mut Rng, a: [f64; 3], b: [f64; 3], rational: bool) -> InlineCrv {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let bulge = rng.uniform(0.0, 0.5);
    let cpts = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let inner = i != 0 && i != n;
            let mut x = [0.0; 3];
            for ((xk, ak), bk) in x.iter_mut().zip(a).zip(b) {
                *xk = ak + (bk - ak) * t;
                if inner {
                    *xk += bulge * rng.uniform(-1.0, 1.0);
                }
            }
            let [x0, x1, x2] = x;
            if rational && inner {
                let w = rng.uniform(0.5, 2.0);
                [w * x0, w * x1, w * x2, w]
            } else if rational {
                [x0, x1, x2, 1.0]
            } else {
                [x0, x1, x2, NOW]
            }
        })
        .collect();
    let (lo, hi) = gen_domain(rng);
    InlineCrv {
        cpts,
        p,
        u: gen_knots(rng, n, p, lo, hi),
    }
}

fn gen_crv(rng: &mut Rng, rational: bool) -> InlineCrv {
    let a = rng.point3(-3.0, 3.0);
    let b = rng.point3(-3.0, 3.0);
    gen_crv_between(rng, a, b, rational)
}

/// [`gen_crv`], rational with probability `prob`.
fn gen_crv_p(rng: &mut Rng, prob: f64) -> InlineCrv {
    let rational = rng.chance(prob);
    gen_crv(rng, rational)
}

fn inline(c: InlineCrv) -> CrvIn {
    CrvIn::Inline(c)
}

/// Row `i` (`u` fixed) of a non-rational net as a curve in v with knots `v`.
fn row_curve(s: &InlineSrf, i: usize) -> InlineCrv {
    InlineCrv {
        cpts: s.net.get(i).cloned().unwrap_or_default(),
        p: s.q,
        u: s.v.clone(),
    }
}

/// Column `j` (`v` fixed) as a curve in u with knots `u`.
fn col_curve(s: &InlineSrf, j: usize) -> InlineCrv {
    InlineCrv {
        cpts: (0..s.net.len()).map(|i| cp(s, i, j)).collect(),
        p: s.p,
        u: s.u.clone(),
    }
}

fn diff_pts(a: [f64; 4], b: [f64; 4], f: f64) -> [f64; 4] {
    let ([ax, ay, az, _], [bx, by, bz, _]) = (a, b);
    [f * (ax - bx), f * (ay - by), f * (az - bz), NOW]
}

/// The boundaries and cross-boundary derivatives of a non-rational surface:
/// `[L, R, B, T]` and `[dL, dR, dB, dT]`.
fn coons_data(s: &InlineSrf) -> (Vec<InlineCrv>, Vec<InlineCrv>) {
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    let (p, q) = (s.p as f64, s.q as f64);
    let (u, v) = (&s.u, &s.v);
    let fu0 = p / (kat(u, s.p + 1) - kat(u, 0));
    let fu1 = p / (kat(u, n + s.p + 1) - kat(u, n));
    let fv0 = q / (kat(v, s.q + 1) - kat(v, 0));
    let fv1 = q / (kat(v, m + s.q + 1) - kat(v, m));
    let d_l = InlineCrv {
        cpts: (0..=m)
            .map(|j| diff_pts(cp(s, 1, j), cp(s, 0, j), fu0))
            .collect(),
        p: s.q,
        u: s.v.clone(),
    };
    let d_r = InlineCrv {
        cpts: (0..=m)
            .map(|j| diff_pts(cp(s, n, j), cp(s, n - 1, j), fu1))
            .collect(),
        p: s.q,
        u: s.v.clone(),
    };
    let d_b = InlineCrv {
        cpts: (0..=n)
            .map(|i| diff_pts(cp(s, i, 1), cp(s, i, 0), fv0))
            .collect(),
        p: s.p,
        u: s.u.clone(),
    };
    let d_t = InlineCrv {
        cpts: (0..=n)
            .map(|i| diff_pts(cp(s, i, m), cp(s, i, m - 1), fv1))
            .collect(),
        p: s.p,
        u: s.u.clone(),
    };
    (
        vec![
            row_curve(s, 0),
            row_curve(s, n),
            col_curve(s, 0),
            col_curve(s, m),
        ],
        vec![d_l, d_r, d_b, d_t],
    )
}

fn scale_crv(c: &mut InlineCrv, f: f64) {
    for p in &mut c.cpts {
        for x in p.iter_mut().take(3) {
            *x *= f;
        }
    }
}

fn gen_coons(rng: &mut Rng) -> OpIn {
    let bilinear = rng.chance(0.4);
    // every generated surface has at least two control points per direction
    let s = gen_srf(rng, false, 3);
    let (mut bdry, mut ders) = coons_data(&s);
    // mismatched magnitudes (rescaling path) or perturbed derivatives (warnings)
    if rng.chance(0.4) {
        let k = rng.below(4) as usize;
        let f = rng.uniform(0.3, 3.0);
        if let Some(d) = ders.get_mut(k) {
            scale_crv(d, f);
        }
    }
    if rng.chance(0.2) {
        let k = rng.below(4) as usize;
        let e = rng.uniform(-0.5, 0.5);
        if let Some(d) = ders.get_mut(k) {
            let j = rng.below(d.cpts.len() as u64) as usize;
            if let Some(z) = d.cpts.get_mut(j).and_then(|p| p.get_mut(2)) {
                *z += e;
            }
        }
    }
    // a rational boundary (bilinear handles it, bicubic fails)
    if rng.chance(0.1) {
        let k = rng.below(4) as usize;
        if let Some(b) = bdry.get_mut(k) {
            for p in &mut b.cpts {
                p[3] = 1.0;
            }
        }
    }
    let no_derivs = if bilinear {
        rng.chance(0.5)
    } else {
        rng.chance(0.05)
    };
    let nd = if no_derivs { 0 } else { 4 };
    OpIn::Coons {
        tol: *rng.choose(&[1e-6, 1e-3, 1e-1]).unwrap_or(&1e-3),
        bilinear,
        bdry: bdry.into_iter().map(inline).collect(),
        ders: ders.into_iter().take(nd).map(inline).collect(),
    }
}

fn gen_nsided(rng: &mut Rng) -> OpIn {
    let k = 3 + rng.below(4) as usize;
    let r = rng.uniform(1.0, 3.0);
    let corners: Vec<[f64; 3]> = (0..k)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / k as f64 + rng.uniform(-0.1, 0.1);
            [r * a.cos(), r * a.sin(), rng.uniform(-0.3, 0.3)]
        })
        .collect();
    let corner = |i: usize| corners.get(i % k).copied().unwrap_or([0.0; 3]);
    let bdry: Vec<InlineCrv> = (0..k)
        .map(|i| {
            let a = corner(i);
            let b = corner(i + 1);
            let mut c = gen_crv_between(rng, a, b, false);
            c.u = gen_knots(rng, c.cpts.len() - 1, c.p, 0.0, 1.0);
            c
        })
        .collect();
    let ders = bdry.iter().map(|_| None).collect();
    let center = if rng.chance(0.3) {
        Some([
            [0.0, 0.0, rng.uniform(-0.2, 0.2)],
            [
                rng.uniform(-0.2, 0.2),
                rng.uniform(-0.2, 0.2),
                rng.uniform(0.5, 2.0),
            ],
        ])
    } else {
        None
    };
    OpIn::NSided {
        bdry: bdry.into_iter().map(inline).collect(),
        ders,
        eps: *rng.choose(&[1.0, 5.0, 10.0]).unwrap_or(&5.0),
        center,
    }
}

fn gen_static(rng: &mut Rng) -> OpIn {
    match rng.below(10) {
        0 | 1 => gen_coons(rng),
        2 => gen_nsided(rng),
        3 => OpIn::Ruled {
            c1: inline(gen_crv_p(rng, 0.3)),
            c2: inline(gen_crv_p(rng, 0.3)),
            dir: rng.below(5) as u8,
        },
        4 => {
            let section = gen_crv_p(rng, 0.2);
            let traj = gen_crv_p(rng, 0.2);
            let scale = if rng.chance(0.3) {
                let mut s = gen_crv_between(rng, [1.0, 1.0, 1.0], [1.5, 0.8, 1.2], false);
                s.u = gen_knots(rng, s.cpts.len() - 1, s.p, kat(&traj.u, 0), klast(&traj.u));
                Some(inline(s))
            } else {
                None
            };
            OpIn::Sweep {
                section: inline(section),
                traj: inline(traj),
                scale,
                trans: rng.chance(0.4),
                tol: *rng.choose(&[1e-2, 1e-3]).unwrap_or(&1e-2),
            }
        }
        5 => {
            let mut section = gen_crv_p(rng, 0.2);
            for p in &mut section.cpts {
                p[1] = 0.0; // [x, z] plane
            }
            let mut traj = gen_crv_p(rng, 0.2);
            for p in &mut traj.cpts {
                p[2] = 0.0;
            }
            OpIn::Swung {
                section: inline(section),
                traj: inline(traj),
                scale: rng.uniform(0.5, 2.0),
            }
        }
        6 => {
            let rational = rng.chance(0.7);
            OpIn::NonRat {
                curve: inline(gen_crv(rng, rational)),
                uv: if rng.chance(0.5) {
                    Some(inline(gen_crv(rng, false)))
                } else {
                    None
                },
                // with a base surface upstream drops the curve with SmSurface::DropCurve,
                // which crashes for a standalone surface (plan/findings.md)
                base: None,
                tol: *rng.choose(&[1e-2, 1e-3, 1e-4]).unwrap_or(&1e-3),
            }
        }
        7 => {
            let n = 1 + rng.below(10) as usize;
            let p = 1 + rng.below(n.min(5) as u64 + 1) as usize;
            let mut u: Vec<f64> = (0..=n).map(|_| rng.unit()).collect();
            u.sort_by(f64::total_cmp);
            if rng.chance(0.4) {
                // clustered parameters: snapping and triple-knot branches
                let mut prev = kat(&u, 0);
                for t in u.iter_mut().skip(1) {
                    if rng.chance(0.5) {
                        *t = prev + rng.uniform(0.0, 1e-3);
                    }
                    prev = *t;
                }
            }
            OpIn::MakeKnots { n, p, u }
        }
        8 => {
            let nk = 1 + rng.below(12) as usize;
            let mut knots: Vec<f64> = (0..nk).map(|_| rng.unit()).collect();
            knots.sort_by(f64::total_cmp);
            let a = rng.unit();
            let b = a + rng.uniform(0.0, 0.5);
            let pick = |rng: &mut Rng| {
                if rng.chance(0.2) {
                    None
                } else {
                    Some(rng.below(nk as u64 + 1).min(nk as u64 - 1) as usize)
                }
            };
            let min_idx = pick(rng);
            let max_idx = if rng.chance(0.2) {
                None
            } else {
                Some(rng.below(nk as u64 + 3) as usize)
            };
            OpIn::TrimBounds {
                knots,
                tmin: rng.unit(),
                min_idx,
                tmax: rng.unit(),
                max_idx,
                dom: [a, a, b, b],
                dir: rng.below(3) as u8,
            }
        }
        _ => {
            let vec3 = |rng: &mut Rng, zero: f64| -> [f64; 3] {
                if rng.chance(zero) {
                    let e = if rng.chance(0.5) { 0.0 } else { 1e-12 };
                    [e, 0.0, 0.0]
                } else {
                    rng.point3(-2.0, 2.0)
                }
            };
            OpIn::ZeroDerivs {
                uv: [rng.unit(), rng.unit()],
                dom: [0.0, 0.0, 1.0, 1.0],
                pt: rng.point3(-5.0, 5.0),
                su: vec3(rng, 0.6),
                sv: vec3(rng, 0.6),
                suu: vec3(rng, 0.3),
                suv: vec3(rng, 0.4),
                svv: vec3(rng, 0.3),
                sz: *rng.choose(&[1e-11, 1e-6]).unwrap_or(&1e-11),
            }
        }
    }
}

fn uv_in(rng: &mut Rng, s: &InlineSrf, out: f64) -> [f64; 2] {
    let (a, b) = (kat(&s.u, 0), klast(&s.u));
    let (c, d) = (kat(&s.v, 0), klast(&s.v));
    let pick = |rng: &mut Rng, lo: f64, hi: f64| {
        if rng.chance(0.15) {
            // exactly on a knot or boundary
            *rng.choose(&[lo, hi]).unwrap_or(&lo)
        } else {
            let e = out * (hi - lo);
            rng.uniform(lo - e, hi + e)
        }
    };
    [pick(rng, a, b), pick(rng, c, d)]
}

/// A second surface sharing the v-max (`along_v`) or u-max boundary of `s`.
fn join_partner(rng: &mut Rng, s: &InlineSrf, along_v: bool) -> InlineSrf {
    let mut o = gen_srf(rng, false, 3);
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    if along_v {
        // same number of rows; o[i][0] = s[i][m]
        let cols = o.net.first().map_or(0, Vec::len);
        o.net = (0..=n)
            .map(|i| {
                (0..cols)
                    .map(|j| {
                        let b = cp(s, i, m);
                        if j == 0 {
                            b
                        } else {
                            [b[0] + rng.uniform(-0.1, 0.1), b[1] + j as f64, b[2], b[3]]
                        }
                    })
                    .collect()
            })
            .collect();
        o.p = s.p;
        o.u.clone_from(&s.u);
    } else {
        let rows = o.net.len();
        o.net = (0..rows)
            .map(|i| {
                (0..=m)
                    .map(|j| {
                        let b = cp(s, n, j);
                        if i == 0 {
                            b
                        } else {
                            [b[0] + i as f64, b[1] + rng.uniform(-0.1, 0.1), b[2], b[3]]
                        }
                    })
                    .collect()
            })
            .collect();
        o.q = s.q;
        o.v.clone_from(&s.v);
    }
    o
}

fn gen_member(rng: &mut Rng) -> Inputs {
    let rational = rng.chance(0.2);
    let polynomial = rng.chance(0.1);
    let s = gen_srf(rng, rational && !polynomial, if polynomial { 2 } else { 4 });
    let form = if polynomial {
        BSplineSurfaceForm::Polynomial as u8
    } else if rng.chance(0.1) {
        rng.below(10) as u8
    } else {
        default_form()
    };
    let op = match rng.below(12) {
        0 => OpIn::Offset {
            dist: rng.uniform(-0.5, 0.5),
            tol: *rng.choose(&[1e-2, 3e-2]).unwrap_or(&1e-2),
            param: rng.below(4) as u8,
        },
        1 => OpIn::Extend {
            dir_overload: rng.chance(0.7),
            dir: rng.below(9) as u8,
            dist: rng.uniform(0.05, 1.0),
            cont: *rng.choose(&[3, 4, 5, 13, 2]).unwrap_or(&3),
        },
        2 => {
            let along_v = rng.chance(0.5);
            let s_nr = InlineSrf {
                net: s
                    .net
                    .iter()
                    .map(|r| r.iter().map(|p| [p[0], p[1], p[2], NOW]).collect())
                    .collect(),
                ..s.clone()
            };
            let other = join_partner(rng, &s_nr, along_v);
            let dir = if rng.chance(0.8) {
                u8::from(!along_v)
            } else {
                u8::from(along_v)
            };
            return Inputs {
                this: Some(This {
                    empty: false,
                    oob: false,
                    form: default_form(),
                    srf: Some(SrfIn::Inline(s_nr)),
                }),
                op: OpIn::Join {
                    other: SrfIn::Inline(other),
                    dir,
                    tol: *rng.choose(&[1e-6, 1e-3, 1e-1]).unwrap_or(&1e-3),
                },
            };
        }
        3 => OpIn::ExtEval {
            uv: uv_in(rng, &s, 0.5),
        },
        4 => {
            let a = rng.uniform(-3.0, 3.0);
            let c = rng.uniform(-3.0, 3.0);
            let (b, d) = if rng.chance(0.05) {
                (a, c + 1.0)
            } else {
                (a + rng.uniform(0.1, 4.0), c + rng.uniform(0.1, 4.0))
            };
            OpIn::Reparam { dom: [a, c, b, d] }
        }
        5 => OpIn::Reverse {
            dir: rng.below(9) as u8,
        },
        6 => {
            let dir = if rng.chance(0.9) {
                rng.below(2) as u8
            } else {
                rng.below(9) as u8
            };
            let k = if dir == 0 { &s.u } else { &s.v };
            let (lo, hi) = (kat(k, 0), klast(k));
            let len = hi - lo;
            let param = match rng.below(3) {
                0 => rng.uniform(lo, hi),
                1 => hi + rng.uniform(0.01, 0.5) * len,
                _ => lo - rng.uniform(0.01, 0.5) * len,
            };
            OpIn::Expand { param, dir }
        }
        7..=9 => {
            let big = rng.chance(0.03);
            OpIn::EvalSimple {
                uv: uv_in(rng, &s, 0.2),
                hu: if big { 18 } else { rng.below(5) as usize },
                hv: if rng.chance(0.03) {
                    5
                } else {
                    rng.below(5) as usize
                },
                u_left: rng.chance(0.5),
                v_left: rng.chance(0.5),
                upper: rng.chance(0.3),
            }
        }
        _ => OpIn::EvalPoint {
            uv: uv_in(rng, &s, 0.2),
        },
    };
    let empty = matches!(op, OpIn::Offset { .. } | OpIn::Reparam { .. }) && rng.chance(0.05);
    Inputs {
        this: Some(This {
            empty,
            oob: rng.chance(0.2),
            form,
            srf: if empty { None } else { Some(SrfIn::Inline(s)) },
        }),
        op,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.45) {
        Inputs {
            this: None,
            op: gen_static(rng),
        }
    } else {
        gen_member(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.b_spline_surface_n_lib_p1"
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
