//! Runner for unit `blend.fillet_custom_solver`:
//! `SmEiffelTowerCrossSectionFSG::CreateSurface`, `SmEiffelTowerFS::LoadJacobian` /
//! `LoadInitialValues` and `SmSecantConstantRadiusFS::LoadJacobian` /
//! `LoadInitialValues`.
//!
//! C++: `solidcore_ffi::units::blend_fillet_custom_solver` (upstream
//! `source/SMLib/src/SmFilletCustomSolver.cpp`). Rust:
//! `solidcore::blend::fillet_custom_solver`.
//!
//! A case is one op (`create`, `jac` or `init`) with its inputs. Both sides produce the
//! same flat record (layout in the FFI module docs), split into keys by [`to_output`]:
//! `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001` `SM_ERR`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot produce),
//! then for `create` the final rails (`rail1_*`, `rail2_*`) and the fillet surface
//! (`new_*`); for `jac` `counts` (`ne np`), `found`, `f`, `jac` and `offsets`; for
//! `init` `surface2` (rail of `rpSurface2`, `rlSurface2Index`), `intervals`, `per` and
//! `guess`.
//!
//! The generated `create` cases build "Eiffel tower" sections exactly (a first arc of
//! radius `R1` swept `alpha` from the rail 1 point, a second of radius `R2` swept `beta`
//! more ending on rail 2) along line, arc and free-form rails, with jittered fillet
//! points, touching rails, degenerate sweeps (`beta` 0 or below `SM_EFF_ZERO_SQRT`),
//! mismatched point counts and knot vectors, tolerances from 0 to above the rail gap
//! and scales from `1e-6` to `1e6`. `jac` and `init` cases build their offset surfaces
//! with the solver's `sing` (`lSingularities`): upstream's default `SM_SS_UNKNOWN`
//! (fixtures, and half the generated cases) asks the base surface's `GetSingularities`
//! and is deferred until `surface.surface_p3`; `SM_SS_NONE` cases compare the offset
//! evaluations now. `init` cases with `rbDoSurf2Calcs` also wait on the point solves
//! (`surface.surface_p4`, `surface.surface_p5`). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{
    ConstantRadiusAssistedFs, EiffelTowerCrossSectionFsg, EiffelTowerFs, JacobianLoad,
    OffsetSurface, SecantConstantRadiusFs, SurfaceId,
};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::matrix::Matrix;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::NurbCircleParam;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_custom_solver::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `blend.fillet_custom_solver` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline 3D curve: control points as stored (`[x, y, z, w]`), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

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

/// The solver set-up: rail 0 offsets `a`, rail 1 offsets `b` (or `a` when absent).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    a: InlineSrf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<InlineSrf>,
    r: [f64; 2],
    o: [f64; 2],
    angle: f64,
    #[serde(default = "default_ext")]
    ext: f64,
    /// `lSingularities` of the offset surfaces (upstream's default `SM_SS_UNKNOWN`
    /// when absent).
    #[serde(default = "default_sing")]
    sing: u32,
}

fn default_ext() -> f64 {
    0.5
}

fn default_sing() -> u32 {
    SS_UNKNOWN
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Eiffel,
    Secant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Create {
        tol: f64,
        rail1: InlineCrv,
        rail2: InlineCrv,
        /// `[SrfPos(0), SrfPos(1)]` per fillet point.
        points: Vec<[[f64; 3]; 2]>,
    },
    Jac {
        kind: Kind,
        solver: SolverIn,
        rail: usize,
        ne: usize,
        np: usize,
        sel1: usize,
        off1: usize,
        sel2: usize,
        off2: usize,
        x: Vec<f64>,
        f: Vec<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        jac: Option<[usize; 2]>,
    },
    Init {
        kind: Kind,
        solver: SolverIn,
        do2: bool,
        rail: usize,
        sel1: usize,
        s1_index: usize,
        intervals: Vec<[f64; 2]>,
        per: Vec<bool>,
        guess: Vec<f64>,
    },
}

impl Inputs {
    fn ffi(&self) -> Op {
        match self {
            Inputs::Create { .. } => Op::Create,
            Inputs::Jac {
                kind: Kind::Eiffel, ..
            } => Op::JacEiffel,
            Inputs::Jac {
                kind: Kind::Secant, ..
            } => Op::JacSecant,
            Inputs::Init {
                kind: Kind::Eiffel, ..
            } => Op::InitEiffel,
            Inputs::Init {
                kind: Kind::Secant, ..
            } => Op::InitSecant,
        }
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
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

fn push_solver(o: &mut Vec<f64>, s: &SolverIn) -> Result<(), HarnessError> {
    o.push(flag(s.b.is_none()));
    push_srf(o, &s.a)?;
    if let Some(b) = &s.b {
        push_srf(o, b)?;
    }
    o.extend([s.r[0], s.r[1], s.o[0], s.o[1], s.angle, s.ext]);
    o.push(f64::from(s.sing));
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Create {
            tol,
            rail1,
            rail2,
            points,
        } => {
            o.push(*tol);
            push_crv(&mut o, rail1);
            push_crv(&mut o, rail2);
            o.push(points.len() as f64);
            for [p1, p2] in points {
                o.extend_from_slice(p1);
                o.extend_from_slice(p2);
            }
        }
        Inputs::Jac {
            solver,
            rail,
            ne,
            np,
            sel1,
            off1,
            sel2,
            off2,
            x,
            f,
            jac,
            ..
        } => {
            push_solver(&mut o, solver)?;
            o.extend([*rail, *ne, *np, *sel1, *off1, *sel2, *off2].map(|v| v as f64));
            o.push(x.len() as f64);
            o.extend_from_slice(x);
            o.push(f.len() as f64);
            o.extend_from_slice(f);
            let [rows, cols] = jac.unwrap_or([0, 0]);
            o.extend([flag(jac.is_some()), rows as f64, cols as f64]);
        }
        Inputs::Init {
            solver,
            do2,
            rail,
            sel1,
            s1_index,
            intervals,
            per,
            guess,
            ..
        } => {
            push_solver(&mut o, solver)?;
            o.extend([flag(*do2), *rail as f64, *sel1 as f64, *s1_index as f64]);
            o.push(intervals.len() as f64);
            for iv in intervals {
                o.extend_from_slice(iv);
            }
            o.push(per.len() as f64);
            o.extend(per.iter().map(|&b| flag(b)));
            o.push(guess.len() as f64);
            o.extend_from_slice(guess);
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    let flat = ffi::eval(i.ffi(), &input).map_err(|e| harness(e.to_string()))?;
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
        if x.fract() != 0.0 || !(0.0..=1e6).contains(&x) {
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

    fn curve(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
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

    fn surface(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
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
    match i {
        Inputs::Create { .. } => {
            s.curve("rail1_")?;
            s.curve("rail2_")?;
            if ok {
                let has = s.take(1)?;
                let set = has.first().copied() == Some(1.0);
                if set {
                    s.surface("new_")?;
                } else {
                    s.put("new_has", has);
                }
            }
        }
        Inputs::Jac { f, jac, .. } => {
            s.vals("counts", 2)?;
            s.vals("found", 1)?;
            s.vals("f", f.len())?;
            if let Some([rows, cols]) = jac {
                s.vals("jac", rows * cols)?;
            }
            s.vals("offsets", 2)?;
        }
        Inputs::Init { intervals, .. } => {
            s.vals("surface2", 2)?;
            s.vals("intervals", 2 * intervals.len())?;
            let np = s.count()?;
            s.vals("per", np)?;
            let ng = s.count()?;
            s.vals("guess", ng)?;
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

fn curve_of(c: &InlineCrv) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur =
        NurbCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, 3, false))
}

fn surface_of(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
        .collect();
    let nurb = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    Ok(Surface::from(BSplineSurface::from_nurb(nurb, false)))
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn dump_curve(o: &mut Vec<f64>, c: &BSplineCurve) {
    let Some(cur) = c.gw_nurb() else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.extend([
        cur.n() as f64,
        cur.degree() as f64,
        cur.knots().len().saturating_sub(1) as f64,
    ]);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(cur.knots());
}

fn dump_surface(o: &mut Vec<f64>, s: &BSplineSurface) {
    let Some(sur) = s.gw_nurb() else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
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

/// A Rust result as the case's outcome: `Err(deferred)` for an unported callee.
type Built<T> = Result<T, Output>;

/// `r`, or the deferred output of an `Unported` error (others are harness errors:
/// the C++ constructions cannot fail).
fn built<T>(r: Result<T, SolidError>) -> Result<Built<T>, HarnessError> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => Output::unported(&e)
            .map(Err)
            .ok_or_else(|| harness(format!("offset surface: {e}"))),
    }
}

/// An offset surface of the solver's rails (`SmOffsetSurface(d, base)` with the
/// case's `lSingularities`).
fn offset_of(
    k: &Kernel,
    s: &SolverIn,
    d: f64,
    base: Surface,
    id: SurfaceId,
) -> Result<Built<OffsetSurface>, HarnessError> {
    built(OffsetSurface::new(k, d, base, id, s.sing))
}

/// The solver's base: rail 0 offsets A (`Free(0)`), rail 1 B (`Free(1)`) or A.
fn base_of(k: &Kernel, s: &SolverIn) -> Result<Built<ConstantRadiusAssistedFs>, HarnessError> {
    let a = surface_of(&s.a)?;
    let (b, b_id) = match &s.b {
        Some(b) => (surface_of(b)?, SurfaceId::Free(1)),
        None => (a.clone(), SurfaceId::Free(0)),
    };
    let mut s0 = match offset_of(k, s, s.r[0] * s.o[0], a, SurfaceId::Free(0))? {
        Ok(o) => o,
        Err(out) => return Ok(Err(out)),
    };
    let mut s1 = match offset_of(k, s, s.r[1] * s.o[1], b, b_id)? {
        Ok(o) => o,
        Err(out) => return Ok(Err(out)),
    };
    s0.set_allow_extension(true);
    s1.set_allow_extension(true);
    let mut fs = ConstantRadiusAssistedFs::from_parts(
        1.0e-6,
        30.0 * std::f64::consts::PI / 180.0,
        2.0 * std::f64::consts::PI / 180.0,
        s.r,
        [Some(s0), Some(s1)],
        s.o,
        [None, None],
    );
    fs.solver.surface_extension_factor = s.ext;
    Ok(Ok(fs))
}

/// The base identity of a selector (`None` for `NULL`).
fn selector_id(sel: usize, s: &SolverIn) -> Option<SurfaceId> {
    match sel {
        1 | 3 => Some(SurfaceId::Free(0)),
        2 => Some(if s.b.is_some() {
            SurfaceId::Free(1)
        } else {
            SurfaceId::Free(0)
        }),
        4 => Some(SurfaceId::Free(2)),
        _ => None,
    }
}

/// The offset surface of a (non-`NULL`) selector.
fn selector_surface(
    k: &Kernel,
    sel: usize,
    s: &SolverIn,
    fs: &ConstantRadiusAssistedFs,
) -> Result<Built<OffsetSurface>, HarnessError> {
    let rust = |e: SolidError| harness(e.to_string());
    match sel {
        1 => fs.surface(0).cloned().map(Ok).map_err(rust),
        2 => fs.surface(1).cloned().map(Ok).map_err(rust),
        3 | 4 => {
            let id = if sel == 3 {
                SurfaceId::Free(0)
            } else {
                SurfaceId::Free(2)
            };
            offset_of(k, s, s.r[0] * s.o[0], surface_of(&s.a)?, id)
        }
        _ => Err(harness("NULL first surface")),
    }
}

fn rust_create(
    tol: f64,
    rail1: &InlineCrv,
    rail2: &InlineCrv,
    points: &[[[f64; 3]; 2]],
) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut r1 = curve_of(rail1)?;
    let mut r2 = curve_of(rail2)?;
    let pts: Vec<TsectPnt> = points
        .iter()
        .map(|[a, b]| {
            let mut t = TsectPnt::new();
            *t.srf_pos_mut(SrfIndex::First) = Vec3::new(a[0], a[1], a[2]);
            *t.srf_pos_mut(SrfIndex::Second) = Vec3::new(b[0], b[1], b[2]);
            t
        })
        .collect();
    let res = EiffelTowerCrossSectionFsg::new().create_surface(&k, tol, &mut r1, &mut r2, &pts);
    let mut o = Vec::new();
    match &res {
        Ok(_) => o.push(0.0),
        Err(e) => {
            if let Some(out) = Output::unported(e) {
                return Ok(out);
            }
            o.push(status(e));
        }
    }
    dump_curve(&mut o, &r1);
    dump_curve(&mut o, &r2);
    if let Ok(s) = &res {
        o.push(1.0);
        dump_surface(&mut o, s);
    }
    Ok(Output::new().values("flat", o))
}

#[allow(clippy::too_many_arguments)]
fn rust_jac(
    kind: Kind,
    solver: &SolverIn,
    rail: usize,
    ne: usize,
    np: usize,
    sel: [(usize, usize); 2],
    x: &[f64],
    f: &[f64],
    jac: Option<[usize; 2]>,
) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let base = match base_of(&k, solver)? {
        Ok(b) => b,
        Err(out) => return Ok(out),
    };
    let mut load = JacobianLoad {
        rail_index: rail,
        surface1: selector_id(sel[0].0, solver),
        surf1_offset: sel[0].1,
        surface2: selector_id(sel[1].0, solver),
        surf2_offset: sel[1].1,
        x,
    };
    let (mut ne, mut np) = (ne, np);
    let mut f = f.to_vec();
    let mut j = jac.map(|[r, c]| Matrix::new(r, c));
    let (res, offsets) = match kind {
        Kind::Eiffel => {
            let mut fs = EiffelTowerFs::new(base, solver.angle);
            let r = fs.load_jacobian(&k, &mut ne, &mut np, &mut load, &mut f, j.as_mut());
            (r, offsets_of(&fs.base))
        }
        Kind::Secant => {
            let mut fs = SecantConstantRadiusFs::new(base, solver.angle);
            let r = fs.load_jacobian(&k, &mut ne, &mut np, &mut load, &mut f, j.as_mut());
            (r, offsets_of(&fs.base))
        }
    };
    let mut o = Vec::new();
    let found = match &res {
        Ok(b) => {
            o.push(0.0);
            *b
        }
        Err(e) => {
            if let Some(out) = Output::unported(e) {
                return Ok(out);
            }
            o.push(status(e));
            false
        }
    };
    o.extend([ne as f64, np as f64, flag(found)]);
    o.extend_from_slice(&f);
    if let (Some(m), Some([rows, cols])) = (&j, jac) {
        for r in 0..rows {
            for c in 0..cols {
                o.push(m.at(r, c).map_err(|e| harness(e.to_string()))?);
            }
        }
    }
    o.extend(offsets);
    Ok(Output::new().values("flat", o))
}

fn offsets_of(fs: &ConstantRadiusAssistedFs) -> [f64; 2] {
    [0, 1].map(|i| fs.surface(i).map_or(f64::NAN, OffsetSurface::offset_distance))
}

#[allow(clippy::too_many_arguments)]
fn rust_init(
    kind: Kind,
    solver: &SolverIn,
    do2: bool,
    rail: usize,
    sel1: usize,
    s1_index: usize,
    intervals: &[[f64; 2]],
    per: &[bool],
    guess: &[f64],
) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let base = match base_of(&k, solver)? {
        Ok(b) => b,
        Err(out) => return Ok(out),
    };
    let mut surface1 = match selector_surface(&k, sel1, solver, &base)? {
        Ok(s) => s,
        Err(out) => return Ok(out),
    };
    let mut ivls = ExtentNd::new(intervals.len());
    for (i, [a, b]) in intervals.iter().enumerate() {
        *ivls
            .get_mut(i)
            .ok_or_else(|| harness("interval index"))? = Extent1d::from_min_max(*a, *b);
    }
    let mut per = per.to_vec();
    let mut guess = guess.to_vec();
    let res = match kind {
        Kind::Eiffel => EiffelTowerFs::new(base, solver.angle).load_initial_values(
            &k,
            None,
            do2,
            rail,
            &mut surface1,
            s1_index,
            &mut ivls,
            &mut per,
            &mut guess,
        ),
        Kind::Secant => SecantConstantRadiusFs::new(base, solver.angle).load_initial_values(
            &k,
            None,
            do2,
            rail,
            &mut surface1,
            s1_index,
            &mut ivls,
            &mut per,
            &mut guess,
        ),
    };
    let mut o = Vec::new();
    match &res {
        Ok(Some(iv)) => o.extend([0.0, iv.surface2_rail as f64, iv.surface2_index as f64]),
        Ok(None) => o.extend([0.0, -1.0, 999.0]),
        Err(e) => {
            if let Some(out) = Output::unported(e) {
                return Ok(out);
            }
            o.extend([status(e), -1.0, 999.0]);
        }
    }
    for i in 0..intervals.len() {
        let iv = ivls.get(i).ok_or_else(|| harness("interval index"))?;
        o.extend([iv.min(), iv.max()]);
    }
    o.push(per.len() as f64);
    o.extend(per.iter().map(|&b| flag(b)));
    o.push(guess.len() as f64);
    o.extend_from_slice(&guess);
    Ok(Output::new().values("flat", o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let out = match i {
        Inputs::Create {
            tol,
            rail1,
            rail2,
            points,
        } => rust_create(*tol, rail1, rail2, points)?,
        Inputs::Jac {
            kind,
            solver,
            rail,
            ne,
            np,
            sel1,
            off1,
            sel2,
            off2,
            x,
            f,
            jac,
        } => rust_jac(
            *kind,
            solver,
            *rail,
            *ne,
            *np,
            [(*sel1, *off1), (*sel2, *off2)],
            x,
            f,
            *jac,
        )?,
        Inputs::Init {
            kind,
            solver,
            do2,
            rail,
            sel1,
            s1_index,
            intervals,
            per,
            guess,
        } => rust_init(
            *kind, solver, *do2, *rail, *sel1, *s1_index, intervals, per, guess,
        )?,
    };
    if out.deferred.is_some() {
        return Ok(out);
    }
    let flat = out
        .values
        .get("flat")
        .cloned()
        .ok_or_else(|| harness("no record"))?;
    to_output(i, &flat)
}

// ---- generators ---------------------------------------------------------------------

fn v3(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn unit_vec(rng: &mut Rng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        );
        let l = v.length();
        if l > 0.2 && l <= 1.0 {
            return v / l;
        }
    }
}

/// A unit vector perpendicular to `d`.
fn perp(d: Vec3, rng: &mut Rng) -> Vec3 {
    loop {
        let w = unit_vec(rng);
        let p = w - d * w.dot(d);
        let l = p.length();
        if l > 0.2 {
            return p / l;
        }
    }
}

/// Clamped uniform knots for `n + 1` control points of degree `p` over `[a, b]`.
fn uniform_knots(n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let spans = n + 1 - p;
    let mut u = vec![a; p + 1];
    for i in 1..spans {
        u.push(a + (b - a) * i as f64 / spans as f64);
    }
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn crv_of(c: &BSplineCurve) -> Option<InlineCrv> {
    let cur = c.gw_nurb()?;
    Some(InlineCrv {
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    })
}

fn translate(c: &InlineCrv, d: Vec3) -> InlineCrv {
    let mut out = c.clone();
    for p in &mut out.cpts {
        if p[3] == NOW {
            p[0] += d.x;
            p[1] += d.y;
            p[2] += d.z;
        } else {
            // weighted coordinates
            p[0] += d.x * p[3];
            p[1] += d.y * p[3];
            p[2] += d.z * p[3];
        }
    }
    out
}

/// The tower section in its plane: `(P1, C2, S2)` relative to `S1` at the origin
/// (`e_b` up): arc 1 of radius `r1` about `P1 = (0, r1)` swept `alpha`, arc 2 of
/// radius `r2` swept `beta` more.
fn tower_2d(r1: f64, r2: f64, alpha: f64, beta: f64) -> [[f64; 2]; 3] {
    let u = |t: f64| [t.sin(), -t.cos()];
    let p1 = [0.0, r1];
    let ua = u(alpha);
    let c2 = [p1[0] + (r1 - r2) * ua[0], p1[1] + (r1 - r2) * ua[1]];
    let ub = u(alpha + beta);
    let s2 = [c2[0] + r2 * ub[0], c2[1] + r2 * ub[1]];
    [p1, c2, s2]
}

fn unique_knots(c: &InlineCrv) -> Vec<f64> {
    let mut k: Vec<f64> = Vec::new();
    for &t in &c.u {
        if k.last() != Some(&t) {
            k.push(t);
        }
    }
    k
}

#[allow(clippy::too_many_lines)]
fn gen_create(rng: &mut Rng) -> Inputs {
    let scale = *rng
        .choose(&[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3, 1e-6, 1e6])
        .unwrap_or(&1.0);
    let r1 = rng.uniform(0.5, 2.0);
    let mut r2 = r1 * rng.uniform(0.2, 0.95);
    if rng.chance(0.05) {
        r2 = r1;
    }
    let alpha = rng.uniform(0.2, 1.3);
    let beta = match rng.below(20) {
        0 => 0.0,
        1 => 1e-7,
        _ => rng.uniform(0.2, 1.3),
    };
    let [p1_2d, c2_2d, s2_2d] = tower_2d(r1, r2, alpha, beta);
    let kind = rng.below(10);

    let fail = |rng: &mut Rng| -> Inputs { gen_create_simple(rng) };

    // rail 1 and the section frame at a rail point: (e_a, e_b)
    let (rail1, rail2, frames): (InlineCrv, InlineCrv, Vec<(Vec3, Vec3, Vec3)>) = match kind {
        // straight rails
        0..=2 => {
            let o = Vec3::new(rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0), 0.0);
            let d = unit_vec(rng);
            let len = rng.uniform(2.0, 10.0);
            let p = 1 + rng.below(3) as usize;
            let n = p + rng.below(3) as usize;
            let (a, b) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                (0.0, len)
            };
            let u = uniform_knots(n, p, a, b);
            // control points at the Greville abscissae: a linear parametrisation
            let cpts = (0..=n)
                .map(|i| {
                    let g: f64 = u.iter().skip(i + 1).take(p).sum::<f64>() / p as f64;
                    let q = o + d * (len * (g - a) / (b - a));
                    [q.x, q.y, q.z, NOW]
                })
                .collect();
            let rail1 = InlineCrv { cpts, p, u };
            let e_a = perp(d, rng);
            let e_b = d.cross(e_a);
            let off = e_a * s2_2d[0] + e_b * s2_2d[1];
            let mut rail2 = translate(&rail1, off);
            if rng.chance(0.2) {
                // not parallel
                if let Some(last) = rail2.cpts.last_mut() {
                    last[0] += 0.3;
                }
            }
            let Some(c1) = curve_of(&rail1).ok() else {
                return fail(rng);
            };
            let mut frames = Vec::new();
            for t in unique_knots(&rail1) {
                let Ok(s1) = c1.evaluate_point(t) else {
                    return fail(rng);
                };
                frames.push((s1, e_a, e_b));
            }
            (rail1, rail2, frames)
        }
        // arcs
        3..=4 => {
            let c = Vec3::new(rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0), 0.0);
            let rho = rng.uniform(3.0, 8.0);
            let span = rng.uniform(30.0, 300.0);
            let x = unit_vec(rng);
            let y = perp(x, rng);
            let z = x.cross(y);
            let f1 = Axis2Placement::new(c, x, y);
            let f2 = Axis2Placement::new(c + z * s2_2d[1], x, y);
            let rho2 = rho + s2_2d[0];
            let (Ok(a1), Ok(a2)) = (
                BSplineCurve::create_circle_segment(3, &f1, rho, 0.0, span, NurbCircleParam::Quadratic),
                BSplineCurve::create_circle_segment(
                    3,
                    &f2,
                    rho2,
                    0.0,
                    span,
                    NurbCircleParam::Quadratic,
                ),
            ) else {
                return fail(rng);
            };
            let (Some(rail1), Some(rail2)) = (crv_of(&a1), crv_of(&a2)) else {
                return fail(rng);
            };
            let mut frames = Vec::new();
            for t in unique_knots(&rail1) {
                let Ok(s1) = a1.evaluate_point(t) else {
                    return fail(rng);
                };
                let radial = s1 - c;
                let radial = radial / radial.length();
                frames.push((s1, radial, z));
            }
            (rail1, rail2, frames)
        }
        // free-form rails (5..=9)
        _ => {
            let p = 2 + rng.below(2) as usize;
            let n = p + 1 + rng.below(4) as usize;
            let len = rng.uniform(4.0, 10.0);
            let cpts: Vec<[f64; 4]> = (0..=n)
                .map(|i| {
                    let x = len * i as f64 / n as f64;
                    [x, rng.uniform(-0.6, 0.6), rng.uniform(-0.6, 0.6), NOW]
                })
                .collect();
            let (a, b) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                (rng.uniform(-2.0, 0.0), rng.uniform(1.0, 5.0))
            };
            let u = uniform_knots(n, p, a, b);
            let rail1 = InlineCrv { cpts, p, u };
            let e_a = Vec3::new(0.0, 1.0, 0.0);
            let e_b = Vec3::new(0.0, 0.0, 1.0);
            let off = e_a * s2_2d[0] + e_b * s2_2d[1];
            let mut rail2 = translate(&rail1, off);
            match rng.below(10) {
                // varying gap
                0 => {
                    for q in &mut rail2.cpts {
                        q[1] += rng.uniform(-0.05, 0.05);
                        q[2] += rng.uniform(-0.05, 0.05);
                    }
                }
                // rails meet at the start
                1 => {
                    if let (Some(q2), Some(q1)) = (rail2.cpts.first_mut(), rail1.cpts.first()) {
                        *q2 = *q1;
                    }
                }
                // different interior knots (same interval)
                2 if rail2.u.len() > 2 * p + 2 => {
                    let i = p + 1;
                    let lo = rail2.u.get(i - 1).copied().unwrap_or(a);
                    let hi = rail2.u.get(i + 1).copied().unwrap_or(b);
                    if let Some(k) = rail2.u.get_mut(i) {
                        *k = lo + (hi - lo) * rng.uniform(0.2, 0.8);
                    }
                }
                _ => {}
            }
            let Some(c1) = curve_of(&rail1).ok() else {
                return fail(rng);
            };
            let mut frames = Vec::new();
            for t in unique_knots(&rail1) {
                let Ok(s1) = c1.evaluate_point(t) else {
                    return fail(rng);
                };
                frames.push((s1, e_a, e_b));
            }
            (rail1, rail2, frames)
        }
    };

    let jitter = rng.chance(0.3);
    let mut points: Vec<[[f64; 3]; 2]> = frames
        .iter()
        .map(|&(s1, e_a, e_b)| {
            let mut p1 = s1 + e_a * p1_2d[0] + e_b * p1_2d[1];
            let mut p2 = s1 + e_a * c2_2d[0] + e_b * c2_2d[1];
            if jitter {
                p1 += unit_vec(rng) * (1e-3 * r1);
                p2 += unit_vec(rng) * (1e-3 * r1);
            }
            [v3(p1), v3(p2)]
        })
        .collect();
    if rng.chance(0.05) {
        points.pop();
    }
    if rng.chance(0.03) {
        if let Some(q) = points.first().copied() {
            points.push(q);
        }
    }
    let tol = match rng.below(10) {
        0 => 0.0,
        1 => 1e-3,
        2 => 10.0,
        _ => 1e-6,
    } * scale;
    scale_create(tol, rail1, rail2, points, scale)
}

fn scale_create(
    tol: f64,
    mut rail1: InlineCrv,
    mut rail2: InlineCrv,
    mut points: Vec<[[f64; 3]; 2]>,
    scale: f64,
) -> Inputs {
    for c in [&mut rail1, &mut rail2] {
        for p in &mut c.cpts {
            p[0] *= scale;
            p[1] *= scale;
            p[2] *= scale;
        }
    }
    for pp in &mut points {
        for p in pp.iter_mut() {
            for x in p.iter_mut() {
                *x *= scale;
            }
        }
    }
    Inputs::Create {
        tol,
        rail1,
        rail2,
        points,
    }
}

/// A two-section tower between straight rails (the fallback generator).
fn gen_create_simple(rng: &mut Rng) -> Inputs {
    let r1 = rng.uniform(0.5, 2.0);
    let r2 = r1 * rng.uniform(0.2, 0.9);
    let [p1, c2, s2] = tower_2d(r1, r2, rng.uniform(0.2, 1.2), rng.uniform(0.2, 1.2));
    let len = rng.uniform(1.0, 5.0);
    let rail1 = InlineCrv {
        cpts: vec![[0.0, 0.0, 0.0, NOW], [len, 0.0, 0.0, NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    };
    let rail2 = translate(&rail1, Vec3::new(0.0, s2[0], s2[1]));
    let points = [0.0, len]
        .iter()
        .map(|&x| [[x, p1[0], p1[1]], [x, c2[0], c2[1]]])
        .collect();
    Inputs::Create {
        tol: 1e-6,
        rail1,
        rail2,
        points,
    }
}

fn gen_surface(rng: &mut Rng, z0: f64) -> InlineSrf {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(2) as usize;
    let m = q + rng.below(2) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        4.0 * i as f64 / n as f64,
                        4.0 * j as f64 / m as f64,
                        z0 + rng.uniform(-0.3, 0.3),
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    InlineSrf {
        net,
        p,
        q,
        u: uniform_knots(n, p, 0.0, 1.0),
        v: uniform_knots(m, q, 0.0, 1.0),
    }
}

fn gen_solver(rng: &mut Rng) -> SolverIn {
    let a = gen_surface(rng, 0.0);
    let b = if rng.chance(0.15) {
        None
    } else {
        // tilted against A
        let mut b = gen_surface(rng, 0.0);
        for row in &mut b.net {
            for pt in row.iter_mut() {
                let (y, z) = (pt[1], pt[2]);
                pt[1] = z;
                pt[2] = y;
            }
        }
        Some(b)
    };
    let r0 = rng.uniform(0.3, 1.5);
    SolverIn {
        a,
        b,
        r: [r0, r0 * rng.uniform(0.3, 1.0)],
        o: [
            if rng.chance(0.5) { 1.0 } else { -1.0 },
            if rng.chance(0.5) { 1.0 } else { -1.0 },
        ],
        angle: rng.uniform(0.1, 1.4),
        ext: if rng.chance(0.8) { 0.5 } else { 1.0 },
        // upstream's default, or none given (compared before surface.surface_p3)
        sing: if rng.chance(0.5) { SS_UNKNOWN } else { 0 },
    }
}

fn gen_jac(rng: &mut Rng) -> Inputs {
    let kind = if rng.chance(0.5) {
        Kind::Eiffel
    } else {
        Kind::Secant
    };
    let solver = gen_solver(rng);
    let ne = rng.below(2) as usize;
    let np = rng.below(3) as usize;
    let nx = 10;
    let x = (0..nx).map(|_| rng.uniform(-0.1, 1.1)).collect();
    let nf = ne + 3 + rng.below(2) as usize;
    let f = (0..nf).map(|_| rng.uniform(-1.0, 1.0)).collect();
    Inputs::Jac {
        kind,
        solver,
        rail: rng.below(2) as usize,
        ne,
        np,
        sel1: rng.below(5) as usize,
        off1: rng.below(7) as usize,
        sel2: rng.below(5) as usize,
        off2: rng.below(7) as usize,
        x,
        f,
        jac: if rng.chance(0.7) {
            Some([ne + 3, nx])
        } else {
            None
        },
    }
}

fn gen_init(rng: &mut Rng) -> Inputs {
    let kind = if rng.chance(0.5) {
        Kind::Eiffel
    } else {
        Kind::Secant
    };
    let solver = gen_solver(rng);
    let rail = rng.below(2) as usize;
    let do2 = rng.chance(0.6);
    let sel1 = if rng.chance(0.8) {
        rail + 1
    } else {
        1 + rng.below(4) as usize
    };
    let mut intervals = vec![[0.0, 1.0], [0.0, 1.0], [0.0, 0.0], [0.0, 0.0]];
    if rng.chance(0.3) {
        intervals.push([rng.uniform(-1.0, 0.0), rng.uniform(1.0, 2.0)]);
    }
    let per = if rng.chance(0.5) {
        vec![false, false]
    } else {
        vec![rng.chance(0.5), rng.chance(0.5)]
    };
    let guess = vec![rng.uniform(0.05, 0.95), rng.uniform(0.05, 0.95)];
    Inputs::Init {
        kind,
        solver,
        do2,
        rail,
        sel1,
        s1_index: 0,
        intervals,
        per,
        guess,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0..=6 => gen_create(rng),
        7 => gen_jac(rng),
        _ => gen_init(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_custom_solver"
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

    /// The generators reach success and failure of `create` on the Rust side, and
    /// mostly real values.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..400u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let op = format!("{:?}", i.ffi());
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((op, kind)).or_default() += 1;
        }
        for ((op, kind), n) in &tally {
            println!("{op:12} {kind:60} {n}");
        }
        let ok = tally
            .get(&("Create".to_string(), "status:0".to_string()))
            .copied()
            .unwrap_or(0);
        assert!(ok > 150, "too few successful create cases: {ok}");
    }
}
