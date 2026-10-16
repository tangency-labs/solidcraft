//! Runner for unit `nurbs.b_spline_curve_p3`: `SmBSplineCurve` part 3 (scale-translate-
//! rotate copies, internal poles, segment lines, single control points, plane
//! containment, joining, subdivision at discontinuities, gap trimming, `AssertValid`).
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_curve_p3` (upstream
//! `source/SMLib/src/SmBSplineCurve.cpp`). Rust: `solidcore::nurbs::b_spline_curve::p3`.
//!
//! A case is `this` (the curve the op runs on: `dim`, `oob` and an optional inline,
//! `.smc`, interpolated, joined-interpolated or circle curve; without one the curve has
//! no NURB) plus `op` and its parameters. Interpolated, joined and circle inputs are
//! built by the Rust side's `InterpolatePoints` / `CreateByJoining` /
//! `CreateCircleSegment` (bit-identical to upstream, see their units) and handed to both
//! sides as inline curves. Both sides produce the same flat record, split into keys by
//! `to_output`: `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001` `SM_ERR`,
//! `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce), the op's outputs, the state of curves the op created (`new_*`, `p<i>_*`),
//! of the other curve of a join or gap (`other_*`) and of `this` after the op
//! (`this_*`). A state is `flags` (`dim oob borrowed knot_type form has_nurb`) and,
//! with a NURB, `dims` (`n p m`), `cpts` (x y z w as stored) and `u`.
//!
//! Cases whose Rust side reaches an unported callee are deferred: `HasInternalPole` of
//! curves with a NURB (`IsDegenerate` needs `SmCurve::IsKindOf`, unit
//! `curve.curve_p2`), `IsSegmentLine` (its `SmLine_TYPE` test), `IsOnPlane` of curves
//! on the plane, joins without a gap tolerance (`SmCurve::ApproximateLength`) or of
//! continuing lines (the `SmLine_TYPE` test) or of curves `IsLine` cannot classify
//! (`curve.curve_p5`), subdivision of curves with interior knots
//! (`SmCurve::EvaluateContinuity`), gap trimming whose intersection of the extended
//! curves reaches a Newton intersection (`SmCurve::GlobalCurveIntersect` through
//! `SmCCIntersectENFO::Evaluate`, `curve.curve_p5`) and `AssertValid` past its
//! knot checks (`SmCurve::AssertValid`, `curve.curve_p4`; `SmTol::IsDegenerate`).
//! Every run has at least [`MIN_GENERATED`] generated cases; generated inputs the ffi
//! wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{
    BSplineCurve, ControlPointForm, GapFix, NurbCircleParam,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_curve_p3::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nurbs.b_spline_curve_p3` runner.
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

/// Points interpolated by `InterpolatePoints` (open, chord length).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InterpIn {
    pts: Vec<[f64; 3]>,
    deg: usize,
}

/// A quadratic circular arc (`CreateCircleSegment`) about +z.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    center: [f64; 3],
    radius: f64,
    #[serde(default)]
    a0: f64,
    #[serde(default = "full_turn")]
    a1: f64,
}

fn full_turn() -> f64 {
    360.0
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Built by interpolation.
    Interp { interp: InterpIn },
    /// Interpolated pieces joined by `CreateByJoining`.
    Joined { joined: Vec<InterpIn> },
    /// A circular arc.
    Circle { circle: CircleIn },
    /// Inline data.
    Inline(InlineCrv),
}

/// A curve block; without `crv` the curve has no NURB.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct This {
    dim: u64,
    #[serde(default)]
    oob: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    crv: Option<CrvIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Scale {
        sv: [f64; 3],
        ev: [f64; 3],
        deg: f64,
    },
    Pole {
        tol: f64,
    },
    Segment {
        a: f64,
        b: f64,
        ns: usize,
        tol: f64,
    },
    Cp {
        form: u8,
        index: usize,
    },
    OnPlane {
        pp: [f64; 3],
        pn: [f64; 3],
        tol: f64,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Join {
        this_end: bool,
        other: This,
        other_end: bool,
        #[serde(default)]
        tol: Option<f64>,
    },
    Subdivide {
        cont: u8,
        ang: f64,
    },
    Gap {
        other: This,
        fix: u8,
        dmin: f64,
    },
    Assert {
        given: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    this: This,
    #[serde(flatten)]
    op: OpIn,
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Scale { .. } => Op::Scale,
            OpIn::Pole { .. } => Op::Pole,
            OpIn::Segment { .. } => Op::Segment,
            OpIn::Cp { .. } => Op::Cp,
            OpIn::OnPlane { .. } => Op::OnPlane,
            OpIn::Join { .. } => Op::Join,
            OpIn::Subdivide { .. } => Op::Subdivide,
            OpIn::Gap { .. } => Op::Gap,
            OpIn::Assert { .. } => Op::Assert,
        }
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

/// Reads an upstream `.smc` text file: `n`, `p`, rational flag, dimension, `n+1`
/// points (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
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

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn of_curve(c: &Curve) -> InlineCrv {
    InlineCrv {
        cpts: c.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: c.degree(),
        u: c.knots().to_vec(),
    }
}

fn interp(i: &InterpIn) -> Result<BSplineCurve, HarnessError> {
    let pts: Vec<Vec3> = i.pts.iter().map(|&p| v3(p)).collect();
    BSplineCurve::interpolate_points(&pts, None, i.deg, None, None, false, ParamMethod::ChordLength)
        .map_err(|e| harness(format!("interp input: {e}")))
}

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
        CrvIn::Interp { interp: i } => {
            let c = interp(i)?;
            let cur = c.gw_nurb().ok_or_else(|| harness("interp input: no NURB"))?;
            Ok(of_curve(cur))
        }
        CrvIn::Joined { joined } => {
            let mut segs = joined.iter().map(interp).collect::<Result<Vec<_>, _>>()?;
            let c = BSplineCurve::create_by_joining(&mut segs, None)
                .map_err(|e| harness(format!("joined input: {e}")))?;
            let cur = c.gw_nurb().ok_or_else(|| harness("joined input: no NURB"))?;
            Ok(of_curve(cur))
        }
        CrvIn::Circle { circle } => {
            let f = Axis2Placement::new(
                v3(circle.center),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            );
            let c = BSplineCurve::create_circle_segment(
                3,
                &f,
                circle.radius,
                circle.a0,
                circle.a1,
                NurbCircleParam::Quadratic,
            )
            .map_err(|e| harness(format!("circle input: {e}")))?;
            let cur = c.gw_nurb().ok_or_else(|| harness("circle input: no NURB"))?;
            Ok(of_curve(cur))
        }
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([n as f64, c.p as f64, c.u.len().saturating_sub(1) as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn encode_this(t: &This, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    o.extend([flag(t.crv.is_some()), t.dim as f64, flag(t.oob)]);
    if let Some(c) = &t.crv {
        encode_crv(&resolve(c)?, o);
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    encode_this(&i.this, &mut o)?;
    match &i.op {
        OpIn::Scale { sv, ev, deg } => {
            o.extend_from_slice(sv);
            o.extend_from_slice(ev);
            o.push(*deg);
        }
        OpIn::Pole { tol } => o.push(*tol),
        OpIn::Segment { a, b, ns, tol } => o.extend([*a, *b, *ns as f64, *tol]),
        OpIn::Cp { form, index } => o.extend([f64::from(*form), *index as f64]),
        OpIn::OnPlane { pp, pn, tol, ivl } => {
            o.extend_from_slice(pp);
            o.extend_from_slice(pn);
            o.extend([*tol, flag(ivl.is_some())]);
            o.extend_from_slice(&ivl.unwrap_or([0.0, 1.0]));
        }
        OpIn::Join {
            this_end,
            other,
            other_end,
            tol,
        } => {
            o.push(flag(*this_end));
            encode_this(other, &mut o)?;
            o.extend([flag(*other_end), flag(tol.is_some()), tol.unwrap_or(0.0)]);
        }
        OpIn::Subdivide { cont, ang } => o.extend([f64::from(*cont), *ang]),
        OpIn::Gap { other, fix, dmin } => {
            encode_this(other, &mut o)?;
            o.extend([f64::from(*fix), *dmin]);
        }
        OpIn::Assert { given } => o.push(flag(*given)),
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

    fn count(&mut self, key: &str) -> Result<usize, HarnessError> {
        let x = self.take(1)?.first().copied().unwrap_or(f64::NAN);
        if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
            return Err(harness(format!("bad count {x} in record")));
        }
        let out = std::mem::take(&mut self.out);
        self.out = out.int(key, x as i64);
        Ok(x as usize)
    }

    fn vals(&mut self, key: &str, k: usize) -> Result<(), HarnessError> {
        let v = self.take(k)?;
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
        Ok(())
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
            if n.fract() != 0.0
                || m.fract() != 0.0
                || !(0.0..=1e6).contains(&n)
                || !(0.0..=1e6).contains(&m)
            {
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
            OpIn::Scale { .. } => {
                let has = s.take(1)?;
                let created = has.first().copied() == Some(1.0);
                s.out = s.out.values("new", has);
                if created {
                    s.state("new_")?;
                }
            }
            OpIn::Pole { .. } | OpIn::Gap { .. } => s.vals("flag", 1)?,
            OpIn::Segment { .. } => {
                s.vals("flag", 1)?;
                s.vals("line", 6)?;
            }
            OpIn::Cp { .. } => s.vals("cp", 4)?,
            OpIn::OnPlane { .. } => s.vals("on_plane", 2)?,
            OpIn::Join { .. } => {}
            OpIn::Subdivide { .. } => {
                let k = s.count("pieces")?;
                for p in 0..k {
                    s.state(&format!("p{p}_"))?;
                }
            }
            OpIn::Assert { .. } => {
                s.vals("flag", 1)?;
                let k = s.count("reports")?;
                s.vals("report", 5 * k)?;
            }
        }
    }
    if matches!(&i.op, OpIn::Join { .. } | OpIn::Gap { .. }) {
        s.state("other_")?;
    }
    s.state("this_")?;
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
    let mut c = match &t.crv {
        Some(crv) => BSplineCurve::from_nurb(to_curve(&resolve(crv)?)?, t.dim, false),
        None => BSplineCurve::new(t.dim),
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

fn cp_form_of(v: u8) -> Result<ControlPointForm, HarnessError> {
    ControlPointForm::from_code(i64::from(v)).ok_or_else(|| harness(format!("bad form {v}")))
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

fn fix_of(v: u8) -> Result<GapFix, HarnessError> {
    Ok(match v {
        0 => GapFix::Both,
        1 => GapFix::ToThis,
        2 => GapFix::ToOther,
        _ => return Err(harness(format!("bad gap fix {v}"))),
    })
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

/// Runs an op on `c` (and `other` for joins and gaps); pushes its outputs (after the
/// status) on success.
fn rust_op(
    op: &OpIn,
    c: &mut BSplineCurve,
    other: Option<&mut BSplineCurve>,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    let ivl = |a: f64, b: f64| Extent1d::from_min_max(a, b);
    let k = Kernel::default();
    match op {
        OpIn::Scale { sv, ev, deg } => {
            let r = c.create_by_scale_trans_rot(v3(*sv), v3(*ev), *deg)?;
            o.push(1.0);
            dump_state(o, &r);
        }
        OpIn::Pole { tol } => o.push(flag(c.has_internal_pole(&k, *tol)?)),
        OpIn::Segment { a, b, ns, tol } => {
            let r = c.is_segment_line(&ivl(*a, *b), *ns as u64, *tol)?;
            o.push(flag(r.is_line));
            match r.line {
                Some((p, v)) => {
                    push3(o, p);
                    push3(o, v);
                }
                None => o.extend([f64::NAN; 6]),
            }
        }
        OpIn::Cp { form, index } => {
            let (p, w) = c.control_point(cp_form_of(*form)?, *index)?;
            push3(o, p);
            o.push(w);
        }
        OpIn::OnPlane { pp, pn, tol, ivl: iv } => {
            let e = iv.map(|[a, b]| ivl(a, b));
            let r = c.is_on_plane(v3(*pp), v3(*pn), *tol, e.as_ref())?;
            o.extend([flag(r.on_plane), r.max_dist.unwrap_or(f64::NAN)]);
        }
        OpIn::Join {
            this_end,
            other_end,
            tol,
            ..
        } => {
            let other = other.ok_or_else(|| harness("join without other curve"))?;
            c.join_with(&k, *this_end, other, *other_end, *tol)?;
        }
        OpIn::Subdivide { cont, ang } => {
            let parts = c.subdivide_at_discontinuity(&k, continuity_of(*cont)?, *ang)?;
            o.push(parts.len() as f64);
            for p in &parts {
                dump_state(o, p);
            }
        }
        OpIn::Gap { fix, dmin, .. } => {
            let other = other.ok_or_else(|| harness("gap without other curve"))?;
            o.push(flag(c.trim_gap(&k, other, fix_of(*fix)?, *dmin)?));
        }
        OpIn::Assert { given } => {
            let mut list = AssertArray::new();
            let ok = c.assert_valid(&k, Some(&mut list), *given, ObjectKey(1))?;
            o.push(flag(ok));
            o.push(list.len() as f64);
            for r in list.reports() {
                o.extend([
                    r.test_index as f64,
                    r.tol3d,
                    r.value,
                    r.owner_param.x,
                    r.owner_param.y,
                ]);
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut this = build_this(&i.this)?;
    let mut other = match &i.op {
        OpIn::Join { other, .. } | OpIn::Gap { other, .. } => Some(build_this(other)?),
        _ => None,
    };
    let mut o = vec![0.0];
    match rust_op(&i.op, &mut this, other.as_mut(), &mut o) {
        Ok(()) => {}
        Err(Fail::Harness(e)) => return Err(e),
        Err(Fail::Solid(e)) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            o = vec![status(&e)];
        }
    }
    if let Some(c) = &other {
        dump_state(&mut o, c);
    }
    dump_state(&mut o, &this);
    to_output(i, &o)
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped curve: degree `p`, `extra` interior knots (fewer when they
/// collide) with multiplicities up to `max_mult`, 2D or 3D, rational or not.
fn gen_crv_full(
    rng: &mut Rng,
    p: usize,
    extra: usize,
    max_mult: usize,
    rational: bool,
    three_d: bool,
) -> InlineCrv {
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let mut inner: Vec<f64> = Vec::new();
    let mut used: Vec<f64> = Vec::new();
    let mut remaining = extra;
    let mut attempts = 0;
    while remaining > 0 && attempts < 64 {
        attempts += 1;
        let v = if rng.chance(0.3) {
            a + len * (1.0 + rng.below(15) as f64) / 16.0
        } else {
            rng.uniform(a, b)
        };
        if v <= a || v >= b || used.iter().any(|&u| (u - v).abs() < 1e-6 * len) {
            continue;
        }
        used.push(v);
        let mult = if max_mult > 1 && rng.chance(0.25) {
            1 + rng.below(max_mult.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining = remaining.saturating_sub(mult);
    }
    inner.sort_by(f64::total_cmp);
    let n = p + inner.len();
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(5) as usize;
    let extra = if rng.chance(0.35) { 0 } else { rng.below(6) as usize };
    let rational = rng.chance(0.25);
    let three_d = rng.chance(0.75);
    gen_crv_full(rng, p, extra, p, rational, three_d)
}

/// A non-rational line (evenly spaced points), degree 1 to 3, no interior knots.
fn gen_line(rng: &mut Rng, s: [f64; 3], e: [f64; 3], three_d: bool) -> InlineCrv {
    let p = 1 + rng.below(3) as usize;
    let cpts = (0..=p)
        .map(|i| {
            let t = i as f64 / p as f64;
            [
                s[0] + t * (e[0] - s[0]),
                s[1] + t * (e[1] - s[1]),
                if three_d { s[2] + t * (e[2] - s[2]) } else { NOZ },
                NOW,
            ]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-2.0, 2.0), rng.uniform(2.5, 6.0))
    };
    let mut u = vec![a; p + 1];
    u.extend(std::iter::repeat_n(b, p + 1));
    InlineCrv { cpts, p, u }
}

/// A curve with coincident control points (an internal pole) or a degenerate curve.
fn gen_pole(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(3) as usize;
    let extra = 2 + rng.below(3) as usize;
    let mut c = gen_crv_full(rng, p, extra, 1, false, true);
    let n = c.cpts.len();
    if rng.chance(0.2) {
        let q = c.cpts.first().copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
        c.cpts.fill(q);
    } else {
        let at = rng.below(n as u64) as usize;
        let run = c.p + 1;
        let q = c.cpts.get(at).copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
        for x in c.cpts.iter_mut().skip(at).take(run) {
            let j = if rng.chance(0.3) { 1e-7 } else { 0.0 };
            *x = [q[0] + j, q[1], q[2], q[3]];
        }
    }
    c
}

/// A curve for `AssertValid` (knot multiplicities may be too high).
fn gen_assert_crv(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    match rng.below(4) {
        0 => {
            // an interior knot of multiplicity p + 1
            let (a, b) = ends(&c);
            let v = a + (b - a) * (1.0 + rng.below(7) as f64) / 8.0;
            let p = c.p;
            let mut u = vec![a; p + 1];
            u.extend(std::iter::repeat_n(v, p + 1));
            u.extend(std::iter::repeat_n(b, p + 1));
            let n = u.len() - p - 2;
            c.u = u;
            c.cpts.resize(n + 1, c.cpts.last().copied().unwrap_or([0.0, 0.0, 0.0, NOW]));
            c
        }
        1 => {
            // an end knot of multiplicity p + 2
            let (a, b) = ends(&c);
            let p = c.p;
            let at_start = rng.chance(0.5);
            let mut u = vec![a; p + 1 + usize::from(at_start)];
            u.extend(std::iter::repeat_n(b, p + 1 + usize::from(!at_start)));
            let n = u.len() - p - 2;
            if n < p {
                return c;
            }
            c.u = u;
            c.cpts.truncate(n + 1);
            while c.cpts.len() < n + 1 {
                c.cpts.push(c.cpts.last().copied().unwrap_or([0.0, 0.0, 0.0, NOW]));
            }
            c
        }
        _ => c,
    }
}

fn ends(c: &InlineCrv) -> (f64, f64) {
    (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    )
}

fn this_of(rng: &mut Rng, c: InlineCrv) -> This {
    let three_d = c.cpts.first().is_some_and(|p| p[2] != NOZ);
    This {
        dim: if three_d { 3 } else { 2 },
        oob: rng.chance(0.2),
        crv: Some(CrvIn::Inline(c)),
    }
}

/// The Euclidean end point of an inline curve.
fn last_point(c: &InlineCrv) -> [f64; 3] {
    let [x, y, z, w] = c.cpts.last().copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

fn first_point(c: &InlineCrv) -> [f64; 3] {
    let [x, y, z, w] = c.cpts.first().copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// Moves `c` by `to - at`.
fn translate_to(c: &mut InlineCrv, at: [f64; 3], to: [f64; 3]) {
    let d = [to[0] - at[0], to[1] - at[1], to[2] - at[2]];
    for p in &mut c.cpts {
        let w = if p[3] == NOW { 1.0 } else { p[3] };
        p[0] += d[0] * w;
        p[1] += d[1] * w;
        if p[2] != NOZ {
            p[2] += d[2] * w;
        }
    }
}

/// Two curves with (nearly) touching chosen ends.
fn gen_pair(rng: &mut Rng, lines: bool, gap: f64) -> (InlineCrv, bool, InlineCrv, bool) {
    let three_d = rng.chance(0.7);
    let (mut a, mut b) = if lines {
        let s = rng.point3(-5.0, 5.0);
        let m = rng.point3(-5.0, 5.0);
        let e = if rng.chance(0.4) {
            // continuing line
            let f = rng.uniform(0.3, 2.0);
            [m[0] + f * (m[0] - s[0]), m[1] + f * (m[1] - s[1]), m[2] + f * (m[2] - s[2])]
        } else {
            rng.point3(-5.0, 5.0)
        };
        (gen_line(rng, s, m, three_d), gen_line(rng, m, e, three_d))
    } else {
        let pa = 1 + rng.below(4) as usize;
        let pb = 1 + rng.below(4) as usize;
        let ea = rng.below(3) as usize;
        let eb = rng.below(3) as usize;
        let ra = rng.chance(0.2);
        let rb = rng.chance(0.2);
        (
            gen_crv_full(rng, pa, ea, 1, ra, three_d),
            gen_crv_full(rng, pb, eb, 1, rb, three_d),
        )
    };
    let a_end = rng.chance(0.6);
    let b_end = rng.chance(0.4);
    if lines {
        // the lines meet at a's end and b's start; flip the ends asked for
        if !a_end {
            a.cpts.reverse();
        }
        if b_end {
            b.cpts.reverse();
        }
    } else {
        let at = if b_end { last_point(&b) } else { first_point(&b) };
        let to = if a_end { last_point(&a) } else { first_point(&a) };
        let to = [to[0] + gap, to[1], to[2]];
        translate_to(&mut b, at, to);
    }
    if lines && gap != 0.0 {
        let to = if b_end { last_point(&b) } else { first_point(&b) };
        translate_to(&mut b, to, [to[0] + gap, to[1], to[2]]);
    }
    (a, a_end, b, b_end)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let pick = rng.below(18);
    let c = match pick {
        0 | 1 => gen_pole(rng),
        14 | 15 => gen_assert_crv(rng),
        _ => gen_crv(rng),
    };
    let (a, b) = ends(&c);
    let n = c.cpts.len() - 1;
    let sub = |rng: &mut Rng| {
        let x = rng.uniform(a, b);
        let y = rng.uniform(a, b);
        match rng.below(5) {
            0 => (a, b),
            1 => (a, x.max(y)),
            2 => (x.min(y), b),
            _ => (x.min(y), x.max(y)),
        }
    };
    let (this, op) = match pick {
        0 | 1 => (
            this_of(rng, c),
            OpIn::Pole {
                tol: *rng.choose(&[0.0, 1e-5, 1e-6, 1e-3, -1.0]).unwrap_or(&0.0),
            },
        ),
        2 | 3 => (
            this_of(rng, c),
            OpIn::Scale {
                sv: rng.point3(-2.0, 2.0),
                ev: if rng.chance(0.2) { [0.0; 3] } else { rng.point3(-2.0, 2.0) },
                deg: if rng.chance(0.3) { 0.0 } else { rng.uniform(-180.0, 180.0) },
            },
        ),
        4 => {
            let (x, y) = sub(rng);
            (
                this_of(rng, c),
                OpIn::Segment {
                    a: x,
                    b: y,
                    ns: rng.below(12) as usize,
                    tol: *rng.choose(&[1e-6, 1e-3, 0.1, 0.0]).unwrap_or(&1e-6),
                },
            )
        }
        5 | 6 => (
            this_of(rng, c),
            OpIn::Cp {
                form: rng.below(3) as u8,
                index: rng.below(n as u64 + 2) as usize,
            },
        ),
        7 | 8 => {
            // a plane through the start point, often containing a planar curve
            let three_d = c.cpts.first().is_some_and(|p| p[2] != NOZ);
            let mut cc = c;
            let planar = !three_d || rng.chance(0.5);
            if three_d && planar {
                for q in &mut cc.cpts {
                    q[2] = 0.0;
                }
            }
            let pn = if planar && rng.chance(0.8) {
                [0.0, 0.0, 1.0]
            } else {
                let [x, y, z] = rng.point3(-1.0, 1.0);
                let l = (x * x + y * y + z * z).sqrt().max(1e-3);
                [x / l, y / l, z / l]
            };
            let pp = if rng.chance(0.2) {
                [0.0, 0.0, rng.uniform(-1.0, 1.0)]
            } else {
                [rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0), 0.0]
            };
            let ivl = if rng.chance(0.4) {
                let (x, y) = sub(rng);
                Some([x, y])
            } else {
                None
            };
            (
                this_of(rng, cc),
                OpIn::OnPlane {
                    pp,
                    pn,
                    tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
                    ivl,
                },
            )
        }
        9..=11 => {
            let lines = rng.chance(0.5);
            let gap = if rng.chance(0.15) { rng.uniform(0.02, 1.0) } else { 0.0 };
            let (ca, ea, cb, eb) = gen_pair(rng, lines, gap);
            let tol = if rng.chance(0.85) {
                Some(*rng.choose(&[0.01, 1e-7, 1e-3]).unwrap_or(&0.01))
            } else {
                None
            };
            let other = this_of(rng, cb);
            (
                this_of(rng, ca),
                OpIn::Join {
                    this_end: ea,
                    other,
                    other_end: eb,
                    tol,
                },
            )
        }
        12 | 13 => {
            let cc = if rng.chance(0.6) {
                let p = 1 + rng.below(4) as usize;
                let rational = rng.chance(0.2);
                let three_d = rng.chance(0.7);
                gen_crv_full(rng, p, 0, 1, rational, three_d)
            } else {
                c
            };
            (
                this_of(rng, cc),
                OpIn::Subdivide {
                    cont: rng.below(14) as u8,
                    ang: *rng.choose(&[1.0, 0.5, 5.0]).unwrap_or(&1.0),
                },
            )
        }
        14 | 15 => (
            this_of(rng, c),
            OpIn::Assert {
                given: rng.chance(0.8),
            },
        ),
        _ => {
            let r = rng.below(3);
            let gap = match r {
                0 => 0.0,
                1 => rng.uniform(0.5, 3.0),
                _ => rng.uniform(1e-3, 0.05),
            };
            let lines = rng.chance(0.3);
            let (ca, _, cb, _) = gen_pair(rng, lines, gap);
            let other = this_of(rng, cb);
            (
                this_of(rng, ca),
                OpIn::Gap {
                    other,
                    fix: rng.below(3) as u8,
                    dmin: *rng.choose(&[0.1, 0.01, 1.0]).unwrap_or(&0.1),
                },
            )
        }
    };
    Inputs { this, op }
}

/// A generated case the ffi wrapper accepts (redrawn up to 64 times).
fn gen_valid(rng: &mut Rng) -> Option<Inputs> {
    for _ in 0..64 {
        let i = gen_case(rng);
        let ok = cpp_input(&i).is_ok_and(|input| ffi::validate(i.op.ffi(), &input).is_ok());
        if ok {
            return Some(i);
        }
    }
    None
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.b_spline_curve_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_valid(rng)?).ok()
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
