//! Runner for unit `nurbs.b_spline_curve_p1`: the `SmBSplineCurve` object (an NLib
//! curve with dimension and STEP flags), its constructors and copies, the canonical,
//! circle, arc, degenerate and line factories, mirror and plane projection copies,
//! evaluation, end point / parameterisation / knot editing, transformation, queries
//! (knots, control polygon, ends, counts, Greville abscissae, continuities) and the
//! fair-blend tangent scaling.
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_curve_p1` (upstream
//! `source/SMLib/src/SmBSplineCurve.cpp`). Rust: `solidcore::nurbs::b_spline_curve`.
//!
//! A case is `this` (how the curve the op runs on is built: `ctor` `empty`,
//! `fast_owned`, `fast_borrowed`, `data`, `gw_copy` or `copy`, with `dim`, `oob`
//! (out-of-bounds evaluation) and an inline or `.smc` curve), absent for the static
//! factories, plus `op` and its parameters. Both sides produce the same flat record,
//! split into keys by [`to_output`]: `status` (`0` = `SM_SUCCESS`, else the
//! `SmStatus`: `1001` `SM_ERR`, `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side
//! for an error upstream cannot produce), the op's outputs on success, the state of a
//! curve the op created (`new_*`) and the state of `this` after the op (`this_*`). A
//! state is `flags` (`dim oob borrowed knot_type form has_nurb`) and, with a NURB,
//! `dims` (`n p m`), `cpts` (x y z w as stored) and `u`.
//!
//! Cases whose Rust side reaches an unported `SmCurve` method (`EvaluateContinuity`
//! for curves with interior knots, `IsKindOf` for other types, `ApproximateLength` /
//! `TessellateByBisection`) are deferred. Every run has at least [`MIN_GENERATED`]
//! generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType, NurbCircleParam, ProjectionType,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_curve_p1::{self as ffi, Ctor, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nurbs.b_spline_curve_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline curve: control points as stored (`[x, y, z, w]`), degree, knots.
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
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Inline data.
    Inline(InlineCrv),
}

/// How `this` is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CtorIn {
    Empty,
    FastOwned,
    FastBorrowed,
    Data,
    GwCopy,
    Copy,
}

impl CtorIn {
    fn ffi(self) -> Ctor {
        match self {
            CtorIn::Empty => Ctor::Empty,
            CtorIn::FastOwned => Ctor::FastOwned,
            CtorIn::FastBorrowed => Ctor::FastBorrowed,
            CtorIn::Data => Ctor::Data,
            CtorIn::GwCopy => Ctor::GwCopy,
            CtorIn::Copy => Ctor::Copy,
        }
    }
}

/// A curve block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct This {
    ctor: CtorIn,
    dim: u64,
    #[serde(default)]
    oob: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    crv: Option<CrvIn>,
}

/// A frame `[origin, x axis, y axis]` (`SetCanonical`).
type Frame = [[f64; 3]; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
    Evaluate {
        t: f64,
        nd: usize,
        from_left: bool,
        non_zero: bool,
    },
    EvaluatePoint {
        t: f64,
    },
    Intervals,
    Knots {
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    ControlPolygon,
    ControlPoints,
    Ends,
    Counts,
    Greville {
        index: usize,
    },
    Convert2d,
    Convert3d,
    EditParam {
        ivl: [f64; 2],
        notify: bool,
    },
    EditEnd {
        p: [f64; 3],
        start: bool,
        #[serde(default)]
        w: Option<f64>,
    },
    Fixup {
        tol: f64,
    },
    RemoveExtra {
        tol: f64,
    },
    SetFromGw {
        #[serde(default)]
        crv: Option<CrvIn>,
    },
    Transform {
        frame: Frame,
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    Mirror {
        frame: Frame,
    },
    Projection {
        kind: u8,
        point: [f64; 3],
        normal: [f64; 3],
        dir: [f64; 3],
    },
    Eq {
        other: This,
    },
    IsKindOf {
        t: i64,
    },
    Type,
    Continuities {
        tol: f64,
    },
    Tessellate {
        ivl: [f64; 2],
        chord: f64,
        angle: f64,
        min_segments: usize,
    },
    MakeNurb,
    Copy,
    Canonical {
        dim: u64,
        deg: usize,
        cps: Vec<[f64; 3]>,
        form: u8,
        knots: Vec<f64>,
        mults: Vec<usize>,
        kt: u8,
        #[serde(default)]
        weights: Option<Vec<f64>>,
        #[serde(default)]
        trim: Option<[f64; 2]>,
    },
    Circle {
        dim: u64,
        frame: Frame,
        r: f64,
        a0: f64,
        a1: f64,
        quintic: bool,
    },
    Arc {
        center: [f64; 3],
        p1: [f64; 3],
        p2: [f64; 3],
        quintic: bool,
    },
    Degenerate {
        dim: u64,
        p: [f64; 3],
    },
    Line {
        dim: u64,
        p0: [f64; 3],
        p1: [f64; 3],
    },
    Fair {
        pts: Vec<[f64; 3]>,
        tans: Vec<[f64; 3]>,
        #[serde(default)]
        hi: Option<Vec<[f64; 3]>>,
        prevent: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    this: Option<This>,
    #[serde(flatten)]
    op: OpIn,
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::State => Op::State,
            OpIn::Evaluate { .. } => Op::Evaluate,
            OpIn::EvaluatePoint { .. } => Op::EvaluatePoint,
            OpIn::Intervals => Op::Intervals,
            OpIn::Knots { .. } => Op::Knots,
            OpIn::ControlPolygon => Op::ControlPolygon,
            OpIn::ControlPoints => Op::ControlPoints,
            OpIn::Ends => Op::Ends,
            OpIn::Counts => Op::Counts,
            OpIn::Greville { .. } => Op::Greville,
            OpIn::Convert2d => Op::Convert2d,
            OpIn::Convert3d => Op::Convert3d,
            OpIn::EditParam { .. } => Op::EditParam,
            OpIn::EditEnd { .. } => Op::EditEnd,
            OpIn::Fixup { .. } => Op::Fixup,
            OpIn::RemoveExtra { .. } => Op::RemoveExtra,
            OpIn::SetFromGw { .. } => Op::SetFromGw,
            OpIn::Transform { .. } => Op::Transform,
            OpIn::Mirror { .. } => Op::Mirror,
            OpIn::Projection { .. } => Op::Projection,
            OpIn::Eq { .. } => Op::Eq,
            OpIn::IsKindOf { .. } => Op::IsKindOf,
            OpIn::Type => Op::Type,
            OpIn::Continuities { .. } => Op::Continuities,
            OpIn::Tessellate { .. } => Op::Tessellate,
            OpIn::MakeNurb => Op::MakeNurb,
            OpIn::Copy => Op::Copy,
            OpIn::Canonical { .. } => Op::Canonical,
            OpIn::Circle { .. } => Op::Circle,
            OpIn::Arc { .. } => Op::Arc,
            OpIn::Degenerate { .. } => Op::Degenerate,
            OpIn::Line { .. } => Op::Line,
            OpIn::Fair { .. } => Op::Fair,
        }
    }

    /// Whether a successful op is followed by the state of a new curve.
    fn creates(&self) -> bool {
        matches!(
            self,
            OpIn::Mirror { .. }
                | OpIn::Projection { .. }
                | OpIn::Copy
                | OpIn::Canonical { .. }
                | OpIn::Circle { .. }
                | OpIn::Arc { .. }
                | OpIn::Degenerate { .. }
                | OpIn::Line { .. }
        )
    }
}

/// Whitespace-separated numbers of an upstream text file.
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

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let p = int(path, num()?)?;
    let rat = num()? != 0.0;
    let dim = int(path, num()?)?;
    let mut cpts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let (x, y) = (num()?, num()?);
        let z = if dim == 3 { num()? } else { NOZ };
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineCrv { cpts, p, u })
}

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

// ---- C++ side ---------------------------------------------------------------------

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([n as f64, c.p as f64, c.u.len().saturating_sub(1) as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn encode_this(t: &This, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    o.extend([t.ctor.ffi().code(), t.dim as f64, f64::from(u8::from(t.oob))]);
    if t.ctor != CtorIn::Empty {
        let c = t
            .crv
            .as_ref()
            .ok_or_else(|| harness("this: ctor needs a curve"))?;
        encode_crv(&resolve(c)?, o);
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn encode_frame(f: &Frame, o: &mut Vec<f64>) {
    for v in f {
        o.extend_from_slice(v);
    }
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match &i.this {
        Some(t) => encode_this(t, &mut o)?,
        None => o.push(Ctor::None.code()),
    }
    match &i.op {
        OpIn::State
        | OpIn::Intervals
        | OpIn::ControlPolygon
        | OpIn::ControlPoints
        | OpIn::Ends
        | OpIn::Counts
        | OpIn::Convert2d
        | OpIn::Convert3d
        | OpIn::Type
        | OpIn::MakeNurb
        | OpIn::Copy => {}
        OpIn::Evaluate {
            t,
            nd,
            from_left,
            non_zero,
        } => o.extend([*t, *nd as f64, flag(*from_left), flag(*non_zero)]),
        OpIn::EvaluatePoint { t } => o.push(*t),
        OpIn::Knots { ivl } => {
            let [a, b] = ivl.unwrap_or([0.0, 0.0]);
            o.extend([flag(ivl.is_some()), a, b]);
        }
        OpIn::Greville { index } => o.push(*index as f64),
        OpIn::EditParam { ivl, notify } => o.extend([ivl[0], ivl[1], flag(*notify)]),
        OpIn::EditEnd { p, start, w } => {
            o.extend_from_slice(p);
            o.extend([flag(*start), flag(w.is_some()), w.unwrap_or(0.0)]);
        }
        OpIn::Fixup { tol } | OpIn::RemoveExtra { tol } | OpIn::Continuities { tol } => o.push(*tol),
        OpIn::SetFromGw { crv } => {
            o.push(flag(crv.is_some()));
            if let Some(c) = crv {
                encode_crv(&resolve(c)?, &mut o);
            }
        }
        OpIn::Transform { frame, scale } => {
            encode_frame(frame, &mut o);
            o.push(flag(scale.is_some()));
            o.extend_from_slice(&scale.unwrap_or([1.0, 1.0, 1.0]));
        }
        OpIn::Mirror { frame } => encode_frame(frame, &mut o),
        OpIn::Projection {
            kind,
            point,
            normal,
            dir,
        } => {
            o.push(f64::from(*kind));
            o.extend_from_slice(point);
            o.extend_from_slice(normal);
            o.extend_from_slice(dir);
        }
        OpIn::Eq { other } => encode_this(other, &mut o)?,
        OpIn::IsKindOf { t } => o.push(*t as f64),
        OpIn::Tessellate {
            ivl,
            chord,
            angle,
            min_segments,
        } => o.extend([ivl[0], ivl[1], *chord, *angle, *min_segments as f64]),
        OpIn::Canonical {
            dim,
            deg,
            cps,
            form,
            knots,
            mults,
            kt,
            weights,
            trim,
        } => {
            o.extend([*dim as f64, *deg as f64, cps.len() as f64]);
            for p in cps {
                o.extend_from_slice(p);
            }
            o.extend([f64::from(*form), knots.len() as f64]);
            o.extend_from_slice(knots);
            if mults.len() != knots.len() {
                return Err(harness("canonical: one multiplicity per knot"));
            }
            o.extend(mults.iter().map(|&m| m as f64));
            o.push(f64::from(*kt));
            o.push(flag(weights.is_some()));
            if let Some(w) = weights {
                o.push(w.len() as f64);
                o.extend_from_slice(w);
            }
            let [a, b] = trim.unwrap_or([0.0, 0.0]);
            o.extend([flag(trim.is_some()), a, b]);
        }
        OpIn::Circle {
            dim,
            frame,
            r,
            a0,
            a1,
            quintic,
        } => {
            o.push(*dim as f64);
            encode_frame(frame, &mut o);
            o.extend([*r, *a0, *a1, flag(*quintic)]);
        }
        OpIn::Arc {
            center,
            p1,
            p2,
            quintic,
        } => {
            o.extend_from_slice(center);
            o.extend_from_slice(p1);
            o.extend_from_slice(p2);
            o.push(flag(*quintic));
        }
        OpIn::Degenerate { dim, p } => {
            o.push(*dim as f64);
            o.extend_from_slice(p);
        }
        OpIn::Line { dim, p0, p1 } => {
            o.push(*dim as f64);
            o.extend_from_slice(p0);
            o.extend_from_slice(p1);
        }
        OpIn::Fair {
            pts,
            tans,
            hi,
            prevent,
        } => {
            o.push(pts.len() as f64);
            for p in pts {
                o.extend_from_slice(p);
            }
            o.push(tans.len() as f64);
            for t in tans {
                o.extend_from_slice(t);
            }
            let h = hi.as_deref().unwrap_or(&[]);
            o.extend([flag(hi.is_some()), h.len() as f64]);
            for t in h {
                o.extend_from_slice(t);
            }
            o.push(flag(*prevent));
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    match ffi::eval(i.op.ffi(), &input) {
        Ok(flat) => to_output(i, &flat),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- records ----------------------------------------------------------------------

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

    fn vals(&mut self, key: &str, k: usize) -> Result<(), HarnessError> {
        let v = self.take(k)?;
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
        Ok(())
    }

    /// A counted array: `count`, then `width * count` values.
    fn counted(&mut self, key: &str, width: usize) -> Result<(), HarnessError> {
        let k = self.count()?;
        let out = std::mem::take(&mut self.out);
        self.out = out.int(&format!("{key}_count"), k as i64);
        self.vals(key, width * k)
    }

    fn state(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let flags = self.take(6)?;
        let has = flags.get(5).copied() == Some(1.0);
        let out = std::mem::take(&mut self.out);
        self.out = out.values(&format!("{pfx}flags"), flags);
        if has {
            let dims = self.take(3)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let (n, m) = (cnt(0), cnt(2));
            if n.fract() != 0.0 || m.fract() != 0.0 || !(0.0..=1e6).contains(&n) || !(0.0..=1e6).contains(&m) {
                return Err(harness(format!("bad curve dims {dims:?}")));
            }
            let out = std::mem::take(&mut self.out);
            self.out = out.values(&format!("{pfx}dims"), dims);
            self.vals(&format!("{pfx}cpts"), 4 * (n as usize + 1))?;
            self.vals(&format!("{pfx}u"), m as usize + 1)?;
        }
        Ok(())
    }
}

/// Splits a flat record into keys (layout in the module docs).
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut s = Split {
        flat,
        at: 0,
        out: Output::new(),
    };
    let status = s.take(1)?;
    let ok = status.first().copied() == Some(0.0);
    s.out = s.out.values("status", status);
    if ok {
        match &i.op {
            OpIn::Evaluate { nd, .. } => s.vals("pv", 3 * (nd + 1))?,
            OpIn::EvaluatePoint { .. } => s.vals("p", 3)?,
            OpIn::Intervals => s.vals("ivl", 4)?,
            OpIn::Knots { .. } => {
                let k = s.count()?;
                s.vals("knots", k)?;
                s.vals("mults", k)?;
            }
            OpIn::ControlPolygon => {
                s.counted("pts", 3)?;
                s.counted("w", 1)?;
            }
            OpIn::ControlPoints => s.counted("cp", 4)?,
            OpIn::Ends => s.vals("ends", 8)?,
            OpIn::Counts => s.vals("counts", 6)?,
            OpIn::Greville { .. } | OpIn::IsKindOf { .. } => s.vals("value", 1)?,
            OpIn::Eq { .. } | OpIn::Type => s.vals("value", 2)?,
            OpIn::Continuities { .. } => {
                s.vals("min", 1)?;
                s.counted("cont", 1)?;
            }
            OpIn::Tessellate { .. } => {
                s.counted("prm", 1)?;
                s.counted("pts", 3)?;
                s.counted("tans", 3)?;
            }
            OpIn::Fair { hi, .. } => {
                s.vals("tans", 6)?;
                s.vals("hi", 3 * hi.as_ref().map_or(0, Vec::len))?;
            }
            _ => {}
        }
        if i.op.creates() {
            s.state("new_")?;
        }
    }
    if i.this.is_some() {
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

// ---- Rust side --------------------------------------------------------------------

fn to_curve(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    let mut cur = allocate_nurb_curve(n, c.p, m).map_err(|e| harness(format!("curve: {e}")))?;
    for (d, s) in cur.cpts_mut().iter_mut().zip(&c.cpts) {
        d.x = s[0];
        d.y = s[1];
        d.z = s[2];
        d.w = s[3];
    }
    cur.knots_mut().copy_from_slice(&c.u);
    Ok(cur)
}

fn build_this(t: &This) -> Result<BSplineCurve, HarnessError> {
    let crv = || {
        t.crv
            .as_ref()
            .ok_or_else(|| harness("this: ctor needs a curve"))
            .and_then(resolve)
    };
    let mut c = match t.ctor {
        CtorIn::Empty => BSplineCurve::new(t.dim),
        CtorIn::FastOwned => BSplineCurve::from_nurb(to_curve(&crv()?)?, t.dim, false),
        CtorIn::FastBorrowed => BSplineCurve::from_nurb(to_curve(&crv()?)?, t.dim, true),
        CtorIn::Data => {
            let ic = crv()?;
            let src = to_curve(&ic)?;
            BSplineCurve::from_data(t.dim, src.n(), src.m(), src.degree(), src.cpts(), src.knots())
                .map_err(|e| harness(format!("from_data: {e}")))?
        }
        CtorIn::GwCopy => BSplineCurve::from_gw_nurb(t.dim, &to_curve(&crv()?)?),
        CtorIn::Copy => {
            let mut orig = BSplineCurve::from_nurb(to_curve(&crv()?)?, t.dim, false);
            orig.set_out_of_bounds_enabled(t.oob);
            orig.copy()
        }
    };
    c.set_out_of_bounds_enabled(t.oob);
    Ok(c)
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn dump_state(o: &mut Vec<f64>, c: &BSplineCurve) {
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

fn frame_of(f: &Frame) -> Result<Axis2Placement, SolidError> {
    let mut a = Axis2Placement::default();
    a.set_canonical(v3(f[0]), v3(f[1]), v3(f[2]))?;
    Ok(a)
}

fn form_of(v: u8) -> Result<BSplineCurveForm, HarnessError> {
    Ok(match v {
        0 => BSplineCurveForm::PolylineForm,
        1 => BSplineCurveForm::CircularArc,
        2 => BSplineCurveForm::EllipticArc,
        3 => BSplineCurveForm::ParabolicArc,
        4 => BSplineCurveForm::HyperbolicArc,
        5 => BSplineCurveForm::HelicalArc,
        6 => BSplineCurveForm::Unspecified,
        _ => return Err(harness(format!("bad form {v}"))),
    })
}

fn knot_type_of(v: u8) -> Result<KnotType, HarnessError> {
    Ok(match v {
        0 => KnotType::UniformKnots,
        1 => KnotType::Unspecified,
        2 => KnotType::QuasiUniformKnots,
        3 => KnotType::PiecewiseBezierKnots,
        4 => KnotType::Unknown,
        _ => return Err(harness(format!("bad knot type {v}"))),
    })
}

fn projection_of(v: u8) -> Result<ProjectionType, HarnessError> {
    Ok(match v {
        0 => ProjectionType::Parallel,
        1 => ProjectionType::Perspective,
        2 => ProjectionType::Rotation,
        3 => ProjectionType::Unknown,
        _ => return Err(harness(format!("bad projection type {v}"))),
    })
}

fn circle_param(quintic: bool) -> NurbCircleParam {
    if quintic {
        NurbCircleParam::Quintic
    } else {
        NurbCircleParam::Quadratic
    }
}

/// The outcome of a Rust op: success (outputs pushed), an upstream status, or a
/// harness problem.
enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Fail::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// Runs a static op; pushes its outputs (after the status) on success.
fn rust_static(op: &OpIn, o: &mut Vec<f64>) -> Result<(), Fail> {
    let created = match op {
        OpIn::Canonical {
            dim,
            deg,
            cps,
            form,
            knots,
            mults,
            kt,
            weights,
            trim,
        } => {
            let pts: Vec<Point3> = cps.iter().map(|&p| v3(p)).collect();
            BSplineCurve::create_canonical(&CanonicalData {
                dim: *dim,
                degree: *deg,
                control_points: &pts,
                form: form_of(*form)?,
                knot_multiplicities: mults,
                knots,
                knot_type: knot_type_of(*kt)?,
                weights: weights.as_deref(),
                trim: trim.map(|[a, b]| Extent1d::from_min_max(a, b)),
            })?
        }
        OpIn::Circle {
            dim,
            frame,
            r,
            a0,
            a1,
            quintic,
        } => {
            let f = frame_of(frame)?;
            BSplineCurve::create_circle_segment(*dim, &f, *r, *a0, *a1, circle_param(*quintic))?
        }
        OpIn::Arc {
            center,
            p1,
            p2,
            quintic,
        } => BSplineCurve::create_arc_from_points(v3(*center), v3(*p1), v3(*p2), circle_param(*quintic))?,
        OpIn::Degenerate { dim, p } => BSplineCurve::create_degenerate_curve(*dim, v3(*p))?,
        OpIn::Line { dim, p0, p1 } => BSplineCurve::create_line_segment(*dim, v3(*p0), v3(*p1))?,
        OpIn::Fair {
            pts,
            tans,
            hi,
            prevent,
        } => {
            let pts: Vec<Point3> = pts.iter().map(|&p| v3(p)).collect();
            let mut t: Vec<Vec3> = tans.iter().map(|&p| v3(p)).collect();
            let mut h: Option<Vec<Vec3>> = hi.as_ref().map(|h| h.iter().map(|&p| v3(p)).collect());
            BSplineCurve::fair_blend_curve_derivatives(&pts, &mut t, h.as_deref_mut(), *prevent)?;
            for v in t {
                push3(o, v);
            }
            for v in h.unwrap_or_default() {
                push3(o, v);
            }
            return Ok(());
        }
        _ => return Err(harness("not a static op").into()),
    };
    dump_state(o, &created);
    Ok(())
}

/// Runs a curve op on `c`; pushes its outputs (after the status) on success.
fn rust_curve(k: &solidcore::kernel::Kernel, op: &OpIn, c: &mut BSplineCurve, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::State | OpIn::Convert2d | OpIn::Convert3d | OpIn::MakeNurb => match op {
            OpIn::Convert2d => c.convert_to_2d()?,
            OpIn::Convert3d => c.convert_to_3d()?,
            OpIn::MakeNurb => c.make_nurb()?,
            _ => {}
        },
        OpIn::Evaluate {
            t,
            nd,
            from_left,
            non_zero,
        } => {
            let pv = c.evaluate(*t, *nd, *from_left, *non_zero)?;
            if pv.len() != nd + 1 {
                return Err(harness("evaluate: wrong output count").into());
            }
            for v in pv {
                push3(o, v);
            }
        }
        OpIn::EvaluatePoint { t } => push3(o, c.evaluate_point(*t)?),
        OpIn::Intervals => {
            let a = c.natural_interval();
            let b = c.step_interval();
            o.extend([a.min(), a.max(), b.min(), b.max()]);
        }
        OpIn::Knots { ivl } => {
            let e = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            let (k, m) = c.knots(e.as_ref())?;
            o.push(k.len() as f64);
            o.extend_from_slice(&k);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::ControlPolygon => {
            let (pts, w) = c.control_polygon()?;
            o.push(pts.len() as f64);
            for p in pts {
                push3(o, p);
            }
            o.push(w.len() as f64);
            o.extend_from_slice(&w);
        }
        OpIn::ControlPoints => {
            let cp = c.control_points()?;
            o.push(cp.len() as f64);
            for p in cp {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        OpIn::Ends => {
            let e = c.ends()?;
            push3(o, e.start);
            push3(o, e.end);
            o.extend([e.start_weight, e.end_weight]);
        }
        OpIn::Counts => {
            o.extend([
                c.degree()? as f64,
                c.number_natural_knots()? as f64,
                c.number_control_points()? as f64,
                flag(c.is_rational()?),
                flag(c.inside_out()),
                flag(*c.out_of_bounds_enabled_mut()),
            ]);
        }
        OpIn::Greville { index } => o.push(c.greville_abscissa(*index)?),
        // `notify` only reaches the C++ side (the Rust port has no notifications).
        OpIn::EditParam { ivl, .. } => {
            c.edit_parameterization(&Extent1d::from_min_max(ivl[0], ivl[1]))?;
        }
        OpIn::EditEnd { p, start, w } => c.edit_end_point(v3(*p), *start, *w)?,
        OpIn::Fixup { tol } => c.fixup_knot_vector(*tol)?,
        OpIn::RemoveExtra { tol } => c.remove_extra_knots(*tol)?,
        OpIn::SetFromGw { crv } => {
            let src = crv.as_ref().map(resolve).transpose()?;
            let cur = src.as_ref().map(to_curve).transpose()?;
            c.set_from_gw_nurb(cur.as_ref())?;
        }
        OpIn::Transform { frame, scale } => {
            let f = frame_of(frame)?;
            c.transform(&f, scale.map(v3))?;
        }
        OpIn::Mirror { frame } => {
            let f = frame_of(frame)?;
            let m = c.create_mirror_curve(&f)?;
            dump_state(o, &m);
        }
        OpIn::Projection {
            kind,
            point,
            normal,
            dir,
        } => {
            let r = c.create_plane_projection(projection_of(*kind)?, v3(*point), v3(*normal), v3(*dir))?;
            dump_state(o, &r);
        }
        OpIn::Eq { other } => {
            let d = build_this(other)?;
            o.extend([flag(c.approx_eq(&d)), flag(c.approx_eq(c))]);
        }
        OpIn::IsKindOf { t } => o.push(flag(c.is_kind_of(*t)?)),
        OpIn::Type => o.extend([c.type_code() as f64, BSplineCurve::TYPE as f64]),
        OpIn::Continuities { tol } => {
            let (mn, all) = c.calculate_continuities(k, *tol)?;
            o.push(f64::from(mn as i32));
            o.push(all.len() as f64);
            o.extend(all.iter().map(|&x| f64::from(x as i32)));
        }
        OpIn::Tessellate {
            ivl,
            chord,
            angle,
            min_segments,
        } => {
            let t = c.tessellate(k, &Extent1d::from_min_max(ivl[0], ivl[1]), *chord, *angle, *min_segments)?;
            o.push(t.parameters.len() as f64);
            o.extend_from_slice(&t.parameters);
            o.push(t.points.len() as f64);
            for p in t.points {
                push3(o, p);
            }
            o.push(t.tangents.len() as f64);
            for p in t.tangents {
                push3(o, p);
            }
        }
        OpIn::Copy => dump_state(o, &c.copy()),
        _ => return Err(harness("not a curve op").into()),
    }
    Ok(())
}

fn rust(k: &solidcore::kernel::Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut this = i.this.as_ref().map(build_this).transpose()?;
    let mut o = vec![0.0];
    let res = match this.as_mut() {
        Some(c) => rust_curve(k, &i.op, c, &mut o),
        None => rust_static(&i.op, &mut o),
    };
    match res {
        Ok(()) => {}
        Err(Fail::Harness(e)) => return Err(e),
        Err(Fail::Solid(e)) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            o = vec![status(&e)];
        }
    }
    if let Some(c) = &this {
        dump_state(&mut o, c);
    }
    to_output(i, &o)
}

// ---- generated cases --------------------------------------------------------------

/// `v[i]`, or the default when out of range (generator inputs only).
fn at<T: Copy + Default>(v: &[T], i: usize) -> T {
    v.get(i).copied().unwrap_or_default()
}

/// A random clamped curve: degree `p`, `n - p` interior knots (fewer when they
/// collide) with multiplicities up to `p`, 2D or 3D, rational or not.
fn gen_crv_deg(rng: &mut Rng, p: usize, extra: usize) -> InlineCrv {
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let integral = rng.chance(0.3);
    let mut inner: Vec<f64> = Vec::new();
    let mut used: Vec<f64> = Vec::new();
    let mut remaining = extra;
    while remaining > 0 {
        let mut v = a + 0.5 * len;
        for attempt in 0..32 {
            v = if integral && attempt < 16 {
                a + len * (1.0 + rng.below(15) as f64) / 16.0
            } else {
                rng.uniform(a, b)
            };
            if v > a && v < b && !used.contains(&v) {
                break;
            }
        }
        if v <= a || v >= b || used.contains(&v) {
            break;
        }
        used.push(v);
        let mult = if rng.chance(0.25) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    let n = p + inner.len();
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));
    let three_d = rng.chance(0.75);
    let rational = rng.chance(0.3);
    let degenerate = rng.chance(0.03);
    let base = rng.point3(-10.0, 10.0);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = if degenerate { base } else { rng.point3(-10.0, 10.0) };
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let p = if rng.chance(0.08) {
        6 + rng.below(3) as usize
    } else {
        1 + rng.below(5) as usize
    };
    let extra = rng.below(9) as usize;
    gen_crv_deg(rng, p, extra)
}

/// `alpha * a + beta * b` on homogeneous points, keeping the 2D / non-rational markers.
fn mix(alpha: f64, a: [f64; 4], beta: f64, b: [f64; 4]) -> [f64; 4] {
    let z = if a[2] == NOZ { NOZ } else { alpha * a[2] + beta * b[2] };
    let w = if a[3] == NOW { NOW } else { alpha * a[3] + beta * b[3] };
    [alpha * a[0] + beta * b[0], alpha * a[1] + beta * b[1], z, w]
}

/// Inserts `v` once (Boehm), so the curve has a removable knot; unchanged when `v`
/// is outside the interior or already has multiplicity `p`.
fn insert_knot(c: &InlineCrv, v: f64) -> InlineCrv {
    let p = c.p;
    let m = c.u.len() - 1;
    let (Some(&a), Some(&b)) = (c.u.get(p), c.u.get(m - p)) else {
        return c.clone();
    };
    if v <= a || v >= b {
        return c.clone();
    }
    let k = (p..m - p).rfind(|&j| at(&c.u, j) <= v).unwrap_or(p);
    let s = c.u.iter().filter(|&&x| x == v).count();
    if s >= p {
        return c.clone();
    }
    let n = c.cpts.len() - 1;
    let mut q = Vec::with_capacity(n + 2);
    for i in 0..=n + 1 {
        let pt = if i + p <= k {
            at(&c.cpts, i)
        } else if i > k - s {
            at(&c.cpts, i - 1)
        } else {
            let al = (v - at(&c.u, i)) / (at(&c.u, i + p) - at(&c.u, i));
            mix(al, at(&c.cpts, i), 1.0 - al, at(&c.cpts, i - 1))
        };
        q.push(pt);
    }
    let mut u = c.u.clone();
    u.insert(k + 1, v);
    InlineCrv { cpts: q, p, u }
}

fn ends(c: &InlineCrv) -> (f64, f64) {
    (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    )
}

/// A parameter in or near the domain: interior, a knot, an end, or outside.
fn gen_t(rng: &mut Rng, c: &InlineCrv) -> f64 {
    let (a, b) = ends(c);
    match rng.below(10) {
        0..=4 => rng.uniform(a, b),
        5..=6 => at(&c.u, rng.below(c.u.len() as u64) as usize),
        7 => {
            if rng.chance(0.5) {
                a
            } else {
                b
            }
        }
        8 => 0.5 * (a + b),
        _ => rng.uniform(a - 0.2 * (b - a), b + 0.2 * (b - a)),
    }
}

fn gen_vec(rng: &mut Rng, lo: f64, hi: f64) -> [f64; 3] {
    rng.point3(lo, hi)
}

/// A frame: random (orthogonalised by `SetCanonical`), axis-aligned, identity, or
/// (rarely) degenerate.
fn gen_frame(rng: &mut Rng) -> Frame {
    match rng.below(10) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        1 => [gen_vec(rng, -5.0, 5.0), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        2 if rng.chance(0.3) => [gen_vec(rng, -5.0, 5.0), [1.0, 1.0, 0.0], [2.0, 2.0, 0.0]],
        _ => [
            gen_vec(rng, -5.0, 5.0),
            gen_vec(rng, -1.0, 1.0),
            gen_vec(rng, -1.0, 1.0),
        ],
    }
}

fn gen_ctor(rng: &mut Rng) -> CtorIn {
    match rng.below(10) {
        0..=3 => CtorIn::FastOwned,
        4 => CtorIn::FastBorrowed,
        5 | 6 => CtorIn::Data,
        7 | 8 => CtorIn::GwCopy,
        _ => CtorIn::Copy,
    }
}

fn this_of(rng: &mut Rng, c: &InlineCrv) -> This {
    let three_d = c.cpts.first().is_some_and(|p| p[2] != NOZ);
    let dim = if rng.chance(0.1) {
        1 + rng.below(3)
    } else if three_d {
        3
    } else {
        2
    };
    This {
        ctor: gen_ctor(rng),
        dim,
        oob: rng.chance(0.3),
        crv: Some(CrvIn::Inline(c.clone())),
    }
}

/// Perturbs one control point coordinate of `c` by `eps`.
fn perturb(rng: &mut Rng, c: &InlineCrv, eps: f64) -> InlineCrv {
    let mut d = c.clone();
    let i = rng.below(d.cpts.len() as u64) as usize;
    if let Some(p) = d.cpts.get_mut(i) {
        p[0] += eps;
    }
    d
}

fn gen_curve_op(rng: &mut Rng, c: &InlineCrv, this: &This) -> OpIn {
    let (a, b) = ends(c);
    let rational = c.cpts.first().is_some_and(|p| p[3] != NOW);
    match rng.below(26) {
        0 => OpIn::State,
        1..=4 => OpIn::Evaluate {
            t: gen_t(rng, c),
            nd: *rng.choose(&[1, 1, 1, 2, 2, 3, 4, 6, 8]).unwrap_or(&1),
            from_left: rng.chance(0.5),
            non_zero: rng.chance(0.7),
        },
        5 | 6 => OpIn::EvaluatePoint { t: gen_t(rng, c) },
        7 => OpIn::Intervals,
        8 => OpIn::Knots {
            ivl: if rng.chance(0.5) {
                let x = rng.uniform(a - 0.1, b);
                Some([x, rng.uniform(x, b + 0.1)])
            } else {
                None
            },
        },
        9 => match rng.below(3) {
            0 => OpIn::ControlPolygon,
            1 => OpIn::ControlPoints,
            _ => OpIn::Ends,
        },
        10 => OpIn::Counts,
        11 => OpIn::Greville {
            index: rng.below(c.cpts.len() as u64 + 3) as usize,
        },
        12 => {
            if rng.chance(0.5) {
                OpIn::Convert2d
            } else {
                OpIn::Convert3d
            }
        }
        13 => OpIn::EditParam {
            ivl: match rng.below(5) {
                0 => [a, b],
                1 => [a + 1e-12, b],
                2 => [rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)],
                _ => {
                    let x = rng.uniform(-10.0, 10.0);
                    [x, x + rng.uniform(0.01, 20.0)]
                }
            },
            notify: rng.chance(0.5),
        },
        14 => OpIn::EditEnd {
            p: gen_vec(rng, -10.0, 10.0),
            start: rng.chance(0.5),
            w: if rng.chance(if rational { 0.5 } else { 0.15 }) {
                Some(rng.uniform(0.2, 3.0))
            } else {
                None
            },
        },
        15 => OpIn::Fixup {
            tol: 10f64.powf(-rng.uniform(1.0, 12.0)),
        },
        16 => OpIn::RemoveExtra {
            tol: 10f64.powf(-rng.uniform(1.0, 10.0)),
        },
        17 => OpIn::SetFromGw {
            crv: if rng.chance(0.85) {
                Some(CrvIn::Inline(gen_crv(rng)))
            } else {
                None
            },
        },
        18 => OpIn::Transform {
            frame: gen_frame(rng),
            scale: match rng.below(4) {
                0 => None,
                1 => Some([1.0, 1.0, 1.0]),
                2 => {
                    let s = rng.uniform(0.1, 4.0);
                    Some([s, s, s])
                }
                _ => Some(gen_vec(rng, -3.0, 3.0)),
            },
        },
        19 => OpIn::Mirror {
            frame: gen_frame(rng),
        },
        20 => {
            let normal = if rng.chance(0.05) {
                [0.0, 0.0, 1e-12]
            } else {
                gen_vec(rng, -1.0, 1.0)
            };
            let kind = *rng.choose(&[0u8, 0, 1, 1, 2, 3]).unwrap_or(&0);
            let dir = match rng.below(6) {
                0 => {
                    // perpendicular to the normal (parallel) / on the plane (perspective)
                    let [x, y, z] = normal;
                    [y, -x, 0.0 * z]
                }
                _ => gen_vec(rng, -20.0, 20.0),
            };
            OpIn::Projection {
                kind,
                point: if rng.chance(0.2) { [0.0; 3] } else { gen_vec(rng, -5.0, 5.0) },
                normal,
                dir,
            }
        }
        21 => {
            let other = match rng.below(6) {
                0 => This {
                    ctor: CtorIn::Empty,
                    dim: this.dim,
                    oob: false,
                    crv: None,
                },
                1 => This {
                    dim: if this.dim == 3 { 2 } else { 3 },
                    ..this.clone()
                },
                2 => This {
                    crv: Some(CrvIn::Inline(perturb(rng, c, 1e-15))),
                    ..this.clone()
                },
                3 => This {
                    crv: Some(CrvIn::Inline(perturb(rng, c, 1e-3))),
                    ..this.clone()
                },
                4 => This {
                    ctor: gen_ctor(rng),
                    ..this.clone()
                },
                _ => This {
                    ctor: gen_ctor(rng),
                    crv: Some(CrvIn::Inline(gen_crv(rng))),
                    ..this.clone()
                },
            };
            OpIn::Eq { other }
        }
        22 => OpIn::IsKindOf {
            t: if rng.chance(0.85) {
                BSplineCurve::TYPE
            } else {
                *rng.choose(&[14000, 14001, 14034, 1]).unwrap_or(&1)
            },
        },
        23 => {
            if rng.chance(0.5) {
                OpIn::Type
            } else {
                OpIn::MakeNurb
            }
        }
        24 => OpIn::Continuities {
            tol: if rng.chance(0.5) { 1.0 } else { rng.uniform(0.01, 5.0) },
        },
        _ => {
            if rng.chance(0.06) {
                OpIn::Tessellate {
                    ivl: [a, b],
                    chord: rng.uniform(0.01, 0.2),
                    angle: rng.uniform(5.0, 30.0),
                    min_segments: rng.below(4) as usize,
                }
            } else {
                OpIn::Copy
            }
        }
    }
}

/// The curve op a NURB-less curve can run.
fn gen_empty_op(rng: &mut Rng) -> OpIn {
    match rng.below(10) {
        0 => OpIn::State,
        1 => OpIn::Intervals,
        2 => OpIn::EditParam {
            ivl: if rng.chance(0.5) { [0.0, 0.0] } else { [0.0, 1.0] },
            notify: true,
        },
        3 => OpIn::RemoveExtra { tol: 1e-6 },
        4 => OpIn::SetFromGw {
            crv: if rng.chance(0.8) {
                Some(CrvIn::Inline(gen_crv(rng)))
            } else {
                None
            },
        },
        5 => OpIn::Eq {
            other: This {
                ctor: CtorIn::Empty,
                dim: 1 + rng.below(3),
                oob: false,
                crv: None,
            },
        },
        6 => OpIn::IsKindOf { t: BSplineCurve::TYPE },
        7 => OpIn::Type,
        8 => OpIn::MakeNurb,
        _ => OpIn::Copy,
    }
}

fn gen_canonical(rng: &mut Rng) -> OpIn {
    let deg = 1 + rng.below(6) as usize;
    let n_inner = rng.below(6) as usize;
    let mut knots = vec![rng.uniform(-3.0, 3.0)];
    for _ in 0..=n_inner {
        let last = knots.last().copied().unwrap_or(0.0);
        knots.push(last + rng.uniform(0.1, 2.0));
    }
    let mut mults = vec![deg + 1];
    for _ in 0..n_inner {
        mults.push(1 + rng.below(deg as u64) as usize);
    }
    mults.push(deg + 1);
    let total: usize = mults.iter().sum();
    let ncp = total - deg - 1;
    let two_d = rng.chance(0.2);
    let cps: Vec<[f64; 3]> = (0..ncp)
        .map(|_| {
            let [x, y, z] = gen_vec(rng, -10.0, 10.0);
            [x, y, if two_d { 0.0 } else { z }]
        })
        .collect();
    let mut weights = match rng.below(4) {
        0 => Some(vec![1.0; ncp]),
        1 => Some((0..ncp).map(|_| rng.uniform(0.3, 3.0)).collect::<Vec<_>>()),
        _ => None,
    };
    let (lo, hi) = (knots.first().copied().unwrap_or(0.0), knots.last().copied().unwrap_or(1.0));
    let trim = match rng.below(6) {
        0 => Some([lo, hi]),
        1 => {
            let x = rng.uniform(lo, hi);
            Some([x, rng.uniform(x, hi)])
        }
        _ => None,
    };
    let mut dim = if two_d { 2 } else { 3 };
    let mut deg_out = deg;
    match rng.below(30) {
        0 => dim = *rng.choose(&[0u64, 4]).unwrap_or(&0),
        1 => deg_out = 33,
        2 => weights = Some(vec![1.0; ncp + 1]),
        3 => {
            if let Some(w) = weights.as_mut().and_then(|w| w.first_mut()) {
                *w = -1.0;
            }
        }
        4 => dim = 1,
        _ => {}
    }
    OpIn::Canonical {
        dim,
        deg: deg_out,
        cps,
        form: rng.below(7) as u8,
        knots,
        mults,
        kt: rng.below(5) as u8,
        weights,
        trim,
    }
}

fn gen_static_op(rng: &mut Rng) -> OpIn {
    match rng.below(12) {
        0..=2 => gen_canonical(rng),
        3 | 4 => OpIn::Circle {
            dim: if rng.chance(0.1) { *rng.choose(&[1u64, 4]).unwrap_or(&1) } else { 2 + rng.below(2) },
            frame: gen_frame(rng),
            r: if rng.chance(0.05) { 1e-9 } else { rng.uniform(0.1, 10.0) },
            a0: if rng.chance(0.05) { 400.0 } else { rng.uniform(-360.0, 360.0) },
            a1: match rng.below(8) {
                0 => 360.0,
                1 => -360.0,
                _ => rng.uniform(-360.0, 360.0),
            },
            quintic: rng.chance(0.3),
        },
        5 | 6 => {
            let center = gen_vec(rng, -5.0, 5.0);
            let r = rng.uniform(0.2, 8.0);
            let ang = match rng.below(10) {
                0 => 1e-9,
                1 => std::f64::consts::PI,
                2 => -rng.uniform(0.1, 3.0),
                _ => rng.uniform(0.05, 3.1),
            };
            let (c, s) = (ang.cos(), ang.sin());
            let p1 = [center[0] + r, center[1], center[2]];
            let r2 = if rng.chance(0.1) { r * 1.1 } else { r };
            let p2 = [center[0] + r2 * c, center[1] + r2 * s, center[2]];
            OpIn::Arc {
                center,
                p1,
                p2,
                quintic: rng.chance(0.3),
            }
        }
        7 => OpIn::Degenerate {
            dim: if rng.chance(0.15) { *rng.choose(&[1u64, 4]).unwrap_or(&1) } else { 2 + rng.below(2) },
            p: gen_vec(rng, -10.0, 10.0),
        },
        8 | 9 => {
            let p0 = gen_vec(rng, -10.0, 10.0);
            OpIn::Line {
                dim: if rng.chance(0.1) { *rng.choose(&[1u64, 4]).unwrap_or(&1) } else { 2 + rng.below(2) },
                p0,
                p1: if rng.chance(0.08) { p0 } else { gen_vec(rng, -10.0, 10.0) },
            }
        }
        _ => {
            let np = if rng.chance(0.05) { 3 } else { 2 };
            let nt = if rng.chance(0.05) { 1 } else { 2 };
            let pts = (0..np).map(|_| gen_vec(rng, -5.0, 5.0)).collect();
            let mut tans: Vec<[f64; 3]> = (0..nt).map(|_| gen_vec(rng, -3.0, 3.0)).collect();
            if rng.chance(0.1) {
                if let Some(&t0) = tans.first() {
                    if let Some(t1) = tans.get_mut(1) {
                        *t1 = [2.0 * t0[0], 2.0 * t0[1], 2.0 * t0[2]];
                    }
                }
            }
            let nh = *rng.choose(&[0usize, 2, 2, 4, 4, 3]).unwrap_or(&0);
            let hi = if rng.chance(0.3) {
                None
            } else {
                Some((0..nh).map(|_| gen_vec(rng, -3.0, 3.0)).collect())
            };
            OpIn::Fair {
                pts,
                tans,
                hi,
                prevent: rng.chance(0.6),
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.25) {
        return Inputs {
            this: None,
            op: gen_static_op(rng),
        };
    }
    if rng.chance(0.04) {
        return Inputs {
            this: Some(This {
                ctor: CtorIn::Empty,
                dim: 1 + rng.below(3),
                oob: rng.chance(0.3),
                crv: None,
            }),
            op: gen_empty_op(rng),
        };
    }
    let mut c = match rng.below(10) {
        // Bezier curves: no interior knots (continuities without SmCurve)
        0 | 1 => {
            let p = 1 + rng.below(5) as usize;
            gen_crv_deg(rng, p, 0)
        }
        _ => gen_crv(rng),
    };
    // zero first derivative at an end
    if rng.chance(0.08) && c.cpts.len() > 2 {
        if let Some(&p0) = c.cpts.first() {
            if let Some(p1) = c.cpts.get_mut(1) {
                *p1 = p0;
            }
        }
    }
    let this = this_of(rng, &c);
    let mut op = gen_curve_op(rng, &c, &this);
    if let OpIn::RemoveExtra { .. } = op {
        if rng.chance(0.7) {
            let (a, b) = ends(&c);
            c = insert_knot(&c, rng.uniform(a, b));
            op = OpIn::RemoveExtra {
                tol: 10f64.powf(-rng.uniform(4.0, 9.0)),
            };
        }
    }
    let this = This {
        crv: Some(CrvIn::Inline(c)),
        ..this
    };
    Inputs {
        this: Some(this),
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.b_spline_curve_p1"
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
