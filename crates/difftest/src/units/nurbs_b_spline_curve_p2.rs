//! Runner for unit `nurbs.b_spline_curve_p2`: `SmBSplineCurve` part 2 (bounding boxes,
//! joining, point and offset curves, canonical data, line / piecewise-C0 / degeneracy
//! queries, control point access and editing, weight matching, degree elevation,
//! parameter reversal, STEP interval adjustment, reading from a database stream).
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_curve_p2` (upstream
//! `source/SMLib/src/SmBSplineCurve.cpp`). Rust: `solidcore::nurbs::b_spline_curve::p2`.
//!
//! A case is `this` (the curve a member op runs on: `dim`, `oob` and an optional
//! inline, `.smc`, interpolated or circle curve; without one the curve has no NURB),
//! absent for the static ops, plus `op` and its parameters. Interpolated and circle
//! inputs are built by the Rust side's `InterpolatePoints` / `CreateCircleSegment`
//! (bit-identical to upstream, see their units) and handed to both sides as inline
//! curves. Both sides produce the same flat record, split into keys by `to_output`:
//! `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001` `SM_ERR`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce), the op's outputs, the state of a curve the op created (`new_*`), of the
//! segments of a join (`s<i>_*`) and of `this` after the op (`this_*`). A state is
//! `flags` (`dim oob borrowed knot_type form has_nurb`) and, with a NURB, `dims`
//! (`n p m`), `cpts` (x y z w as stored) and `u`.
//!
//! Cases whose Rust side reaches an unported callee are deferred: `IsLine` of rational
//! curves and of curves it cannot classify from the control points (`SmCurve::IsLine`,
//! unit `curve.curve_p5`), the bounding boxes of Bezier curves that reach it,
//! `CreateAnalyticCurve` of lines and rational quadratics (`SmLine`, `curve.line`;
//! `SmCurve::IsArc`), `IsDegenerate` and so `HasRepeatedEndControlPoints`
//! (`SmCurve::IsKindOf`,
//! `curve.curve_p2`), `GetMaxAnalyticDomain` (`SmCurve::IsClosed`, `curve.curve_p4`)
//! and `CreateSimpleOffset` (`SmCurve::IsArc`, `curve.curve_p5`; `EvaluateContinuity`,
//! `curve.curve_p2`). Every run has at least [`MIN_GENERATED`] generated cases;
//! generated inputs the ffi wrapper rejects are redrawn.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{
    BSplineCurve, BSplineCurveForm, BoundingBoxRequest, CanonicalData, ControlPointForm, KnotType,
    NurbCircleParam,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_curve_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nurbs.b_spline_curve_p2` runner.
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

/// `SetCanonical` data.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CanonIn {
    dim: u64,
    deg: usize,
    pts: Vec<[f64; 3]>,
    form: u8,
    knots: Vec<f64>,
    mults: Vec<usize>,
    kt: u8,
    #[serde(default)]
    weights: Option<Vec<f64>>,
    #[serde(default)]
    trim: Option<[f64; 2]>,
}

/// A database stream: an upstream text file, or a header and values the runner
/// writes as ASCII lines or native binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DbIn {
    /// An upstream text file (absolute path after fixture expansion), read as is.
    File { file: String },
    /// Header and values.
    Data(DbData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbData {
    n: i64,
    p: i16,
    rat: i16,
    dim: i16,
    #[serde(default = "one")]
    kt: u64,
    #[serde(default = "six")]
    form: u64,
    /// Control points (2 to 4 values each) then knots, in reading order.
    vals: Vec<f64>,
}

fn one() -> u64 {
    1
}

fn six() -> u64 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Intersector,
    BBox {
        a: f64,
        b: f64,
        normal: bool,
        pseudo: bool,
        polar: bool,
    },
    Analytic {
        a: f64,
        b: f64,
    },
    Join {
        segs: Vec<This>,
        #[serde(default)]
        senses: Option<Vec<bool>>,
    },
    Point {
        p: [f64; 3],
    },
    Offset {
        tol: f64,
        n: [f64; 3],
        d: f64,
        #[serde(default)]
        matched: bool,
    },
    Repeated {
        tol: f64,
    },
    Canonical,
    IsLine {
        ns: usize,
        tol: f64,
    },
    C0,
    Degenerate {
        tol: f64,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Expert {
        form: u8,
        start: usize,
        end: usize,
        stride: usize,
    },
    MaxDomain,
    Unique,
    Polygon {
        a: f64,
        b: f64,
    },
    Match {
        other: This,
    },
    Elevate {
        deg: usize,
    },
    Reverse {
        a: f64,
        b: f64,
    },
    SetCanonical {
        data: CanonIn,
    },
    SetCp {
        form: u8,
        index: usize,
        p: [f64; 3],
        w: f64,
    },
    Adjust {
        a: f64,
        b: f64,
    },
    ReadDb {
        dim: u64,
        binary: bool,
        version: u64,
        db: DbIn,
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
            OpIn::Intersector => Op::Intersector,
            OpIn::BBox { .. } => Op::BBox,
            OpIn::Analytic { .. } => Op::Analytic,
            OpIn::Join { .. } => Op::Join,
            OpIn::Point { .. } => Op::Point,
            OpIn::Offset { .. } => Op::Offset,
            OpIn::Repeated { .. } => Op::Repeated,
            OpIn::Canonical => Op::Canonical,
            OpIn::IsLine { .. } => Op::IsLine,
            OpIn::C0 => Op::C0,
            OpIn::Degenerate { .. } => Op::Degenerate,
            OpIn::Expert { .. } => Op::Expert,
            OpIn::MaxDomain => Op::MaxDomain,
            OpIn::Unique => Op::Unique,
            OpIn::Polygon { .. } => Op::Polygon,
            OpIn::Match { .. } => Op::Match,
            OpIn::Elevate { .. } => Op::Elevate,
            OpIn::Reverse { .. } => Op::Reverse,
            OpIn::SetCanonical { .. } => Op::SetCanonical,
            OpIn::SetCp { .. } => Op::SetCp,
            OpIn::Adjust { .. } => Op::Adjust,
            OpIn::ReadDb { .. } => Op::ReadDb,
        }
    }
}

fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let c = crate::fixtures::text_curve::read(path)?;
    Ok(InlineCrv {
        cpts: c.cpts,
        p: c.p,
        u: c.u,
    })
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

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
        CrvIn::Interp { interp } => {
            let pts: Vec<Vec3> = interp.pts.iter().map(|&p| v3(p)).collect();
            let c = BSplineCurve::interpolate_points(
                &pts,
                None,
                interp.deg,
                None,
                None,
                false,
                ParamMethod::ChordLength,
            )
            .map_err(|e| harness(format!("interp input: {e}")))?;
            let cur = c.gw_nurb().ok_or_else(|| harness("interp input: no NURB"))?;
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

fn encode_canon(c: &CanonIn, o: &mut Vec<f64>) {
    o.extend([c.dim as f64, c.deg as f64, c.pts.len() as f64]);
    for p in &c.pts {
        o.extend_from_slice(p);
    }
    o.extend([f64::from(c.form), c.knots.len() as f64]);
    o.extend_from_slice(&c.knots);
    o.extend(c.mults.iter().map(|&m| m as f64));
    o.push(f64::from(c.kt));
    o.push(flag(c.weights.is_some()));
    if let Some(w) = &c.weights {
        o.push(w.len() as f64);
        o.extend_from_slice(w);
    }
    o.push(flag(c.trim.is_some()));
    o.extend_from_slice(&c.trim.unwrap_or([0.0, 0.0]));
}

/// The database bytes of a `ReadDb` input.
fn db_bytes(binary: bool, version: u64, db: &DbIn) -> Result<Vec<u8>, HarnessError> {
    match db {
        DbIn::File { file } => std::fs::read(file).map_err(|e| harness(format!("{file}: {e}"))),
        DbIn::Data(d) => {
            let per = 2 + usize::from(d.dim != 2) + usize::from(d.rat != 0);
            let n_pts = usize::try_from(d.n).map_or(0, |n| n + 1);
            if binary {
                let mut b = Vec::new();
                b.extend_from_slice(&d.n.to_ne_bytes());
                b.extend_from_slice(&d.p.to_ne_bytes());
                b.extend_from_slice(&d.rat.to_ne_bytes());
                b.extend_from_slice(&d.dim.to_ne_bytes());
                if version > 32 {
                    b.extend_from_slice(&d.kt.to_ne_bytes());
                    b.extend_from_slice(&d.form.to_ne_bytes());
                }
                for x in &d.vals {
                    b.extend_from_slice(&x.to_ne_bytes());
                }
                Ok(b)
            } else {
                let mut t = format!("{}\n{}\n{}\n{}\n", d.n, d.p, d.rat, d.dim);
                if version > 32 {
                    let _ = write!(t, "{}\n{}\n", d.kt, d.form);
                }
                let (pts, knots) = d.vals.split_at((n_pts * per).min(d.vals.len()));
                for c in pts.chunks(per) {
                    let s: Vec<String> = c.iter().map(|x| format!("{x:.17e}")).collect();
                    t.push_str(&s.join(" "));
                    t.push('\n');
                }
                for x in knots {
                    let _ = writeln!(t, "{x:.17e}");
                }
                Ok(t.into_bytes())
            }
        }
    }
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    if let Some(t) = &i.this {
        encode_this(t, &mut o)?;
    }
    match &i.op {
        OpIn::Intersector | OpIn::Canonical | OpIn::C0 | OpIn::MaxDomain | OpIn::Unique => {}
        OpIn::BBox {
            a,
            b,
            normal,
            pseudo,
            polar,
        } => o.extend([*a, *b, flag(*normal), flag(*pseudo), flag(*polar)]),
        OpIn::Analytic { a, b }
        | OpIn::Polygon { a, b }
        | OpIn::Reverse { a, b }
        | OpIn::Adjust { a, b } => o.extend([*a, *b]),
        OpIn::Join { segs, senses } => {
            // upstream reads `(*cpOptSenses)[ii]` past a shorter array (UB)
            if senses.as_ref().is_some_and(|v| v.len() < segs.len()) {
                return Err(HarnessError::new("join: fewer senses than segments"));
            }
            o.push(segs.len() as f64);
            for s in segs {
                encode_this(s, &mut o)?;
            }
            o.push(flag(senses.is_some()));
            for k in 0..segs.len() {
                let s = senses.as_ref().and_then(|v| v.get(k).copied()).unwrap_or(true);
                o.push(flag(s));
            }
        }
        OpIn::Point { p } => o.extend_from_slice(p),
        OpIn::Offset { tol, n, d, matched } => {
            o.push(*tol);
            o.extend_from_slice(n);
            o.extend([*d, flag(*matched)]);
        }
        OpIn::Repeated { tol } => o.push(*tol),
        OpIn::IsLine { ns, tol } => o.extend([*ns as f64, *tol]),
        OpIn::Degenerate { tol, ivl } => {
            o.extend([*tol, flag(ivl.is_some())]);
            o.extend_from_slice(&ivl.unwrap_or([0.0, 1.0]));
        }
        OpIn::Expert {
            form,
            start,
            end,
            stride,
        } => o.extend([f64::from(*form), *start as f64, *end as f64, *stride as f64]),
        OpIn::Match { other } => encode_this(other, &mut o)?,
        OpIn::Elevate { deg } => o.push(*deg as f64),
        OpIn::SetCanonical { data } => encode_canon(data, &mut o),
        OpIn::SetCp { form, index, p, w } => {
            o.extend([f64::from(*form), *index as f64]);
            o.extend_from_slice(p);
            o.push(*w);
        }
        OpIn::ReadDb {
            dim,
            binary,
            version,
            db,
        } => {
            let bytes = db_bytes(*binary, *version, db)?;
            o.extend([*dim as f64, flag(*binary), *version as f64, bytes.len() as f64]);
            o.extend(bytes.iter().map(|&b| f64::from(b)));
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

    /// A flag, then the state of a new curve when it is set.
    fn new_curve(&mut self) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let created = has.first().copied() == Some(1.0);
        let out = std::mem::take(&mut self.out);
        self.out = out.values("new", has);
        if created {
            self.state("new_")?;
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
            OpIn::Intersector
            | OpIn::Repeated { .. }
            | OpIn::C0
            | OpIn::Degenerate { .. }
            | OpIn::Analytic { .. } => s.vals("flag", 1)?,
            OpIn::Unique => s.vals("unique", 1)?,
            OpIn::MaxDomain | OpIn::Reverse { .. } => s.vals("ivl", 2)?,
            OpIn::IsLine { .. } => {
                let f = s.take(1)?;
                let yes = f.first().copied() == Some(1.0);
                s.out = s.out.values("flag", f);
                if yes {
                    s.vals("line", 6)?;
                }
            }
            OpIn::BBox {
                normal,
                pseudo,
                polar,
                ..
            } => {
                if *normal {
                    s.vals("normal", 6)?;
                }
                if *pseudo {
                    s.vals("pseudo", 15)?;
                }
                if *polar {
                    s.vals("polar", 14)?;
                }
            }
            OpIn::Join { .. } | OpIn::Point { .. } | OpIn::ReadDb { .. } => s.new_curve()?,
            OpIn::Offset { .. } => {
                s.vals("gap", 1)?;
                s.new_curve()?;
            }
            OpIn::Canonical => {
                s.vals("canon_head", 4)?;
                let np = s.count("canon_np")?;
                s.vals("canon_pts", 3 * np)?;
                let nk = s.count("canon_nk")?;
                s.vals("canon_knots", nk)?;
                s.vals("canon_mults", nk)?;
                let nw = s.count("canon_nw")?;
                s.vals("canon_w", nw)?;
            }
            OpIn::Expert { .. } => {
                let k = s.count("expert_len")?;
                s.vals("expert", k)?;
            }
            OpIn::Polygon { .. } => {
                let k = s.count("polygon_count")?;
                s.vals("polygon", 3 * k)?;
            }
            OpIn::Match { .. }
            | OpIn::Elevate { .. }
            | OpIn::SetCanonical { .. }
            | OpIn::SetCp { .. }
            | OpIn::Adjust { .. } => {}
        }
    }
    if let OpIn::Join { segs, .. } = &i.op {
        for k in 0..segs.len() {
            s.state(&format!("s{k}_"))?;
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
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

fn cp_form_of(v: u8) -> Result<ControlPointForm, HarnessError> {
    ControlPointForm::from_code(i64::from(v)).ok_or_else(|| harness(format!("bad form {v}")))
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

fn push_new(o: &mut Vec<f64>, c: Option<&BSplineCurve>) {
    o.push(flag(c.is_some()));
    if let Some(c) = c {
        dump_state(o, c);
    }
}

fn push_flag(o: &mut Vec<f64>, b: bool) {
    o.push(flag(b));
}

/// Runs a static op other than `Join`; pushes its outputs (after the status).
fn rust_static(op: &OpIn, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::Point { p } => {
            let c = BSplineCurve::create_point_curve(v3(*p))?;
            push_new(o, Some(&c));
        }
        OpIn::ReadDb {
            dim,
            binary,
            version,
            db,
        } => {
            let bytes = db_bytes(*binary, *version, db)?;
            let mut r = DatabaseReader::new();
            r.open_for_read(bytes, if *binary { FileType::Binary } else { FileType::Ascii });
            let c = BSplineCurve::read_from_db(&mut r, *dim, *version)?;
            push_new(o, Some(&c));
        }
        _ => return Err(harness("not a static op").into()),
    }
    Ok(())
}

/// Runs a curve op on `c`; pushes its outputs (after the status) on success.
fn rust_curve(k: &Kernel, op: &OpIn, c: &mut BSplineCurve, o: &mut Vec<f64>) -> Result<(), Fail> {
    let ivl = |a: f64, b: f64| Extent1d::from_min_max(a, b);
    match op {
        OpIn::Intersector => push_flag(o, c.is_intersector_approx()?),
        OpIn::BBox {
            a,
            b,
            normal,
            pseudo,
            polar,
        } => {
            let req = BoundingBoxRequest {
                normal: *normal,
                pseudo: *pseudo,
                polar: *polar,
            };
            let bx = c.calculate_bounding_box(&ivl(*a, *b), req)?;
            if let Some(nb) = bx.normal {
                o.extend((0..3).map(|i| nb.min_at(i)));
                o.extend((0..3).map(|i| nb.max_at(i)));
            }
            if let Some(pb) = bx.pseudo {
                for v in pb.basis() {
                    push3(o, *v);
                }
                for iv in pb.intervals() {
                    o.extend([iv.min(), iv.max()]);
                }
            }
            if let Some(pl) = bx.polar {
                o.push(f64::from(pl.state().code()));
                for v in pl.basis() {
                    push3(o, *v);
                }
                let d = pl.domain();
                o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
            }
        }
        OpIn::Analytic { a, b } => push_flag(o, c.create_analytic_curve(&Kernel::default(), &ivl(*a, *b))?.is_some()),
        OpIn::Offset { tol, n, d, matched } => {
            let (r, gap) = c.create_simple_offset(&Kernel::default(), *tol, v3(*n), *d, *matched)?;
            o.push(gap);
            push_new(o, Some(&r));
        }
        OpIn::Repeated { tol } => push_flag(o, c.has_repeated_end_control_points(k, *tol)?),
        OpIn::Canonical => {
            let k = c.canonical()?;
            o.extend([
                k.dim as f64,
                k.degree as f64,
                f64::from(k.form as i32),
                f64::from(k.knot_type as i32),
            ]);
            o.push(k.control_points.len() as f64);
            for p in &k.control_points {
                push3(o, *p);
            }
            o.push(k.knots.len() as f64);
            o.extend_from_slice(&k.knots);
            o.extend(k.knot_multiplicities.iter().map(|&m| m as f64));
            o.push(k.weights.len() as f64);
            o.extend_from_slice(&k.weights);
        }
        OpIn::IsLine { ns, tol } => match c.is_line(*ns as u64, *tol)? {
            Some((p, v)) => {
                o.push(1.0);
                push3(o, p);
                push3(o, v);
            }
            None => o.push(0.0),
        },
        OpIn::C0 => push_flag(o, c.is_piecewise_c0()?),
        OpIn::Degenerate { tol, ivl: iv } => {
            let e = iv.map(|[a, b]| ivl(a, b));
            push_flag(o, c.is_degenerate(k, *tol, e.as_ref())?);
        }
        OpIn::Expert {
            form,
            start,
            end,
            stride,
        } => {
            let v = c.control_points_expert(cp_form_of(*form)?, *start, *end, *stride)?;
            o.push(v.len() as f64);
            o.extend_from_slice(&v);
        }
        OpIn::MaxDomain => {
            let d = c.max_analytic_domain(k)?;
            o.extend([d.min(), d.max()]);
        }
        OpIn::Unique => o.push(c.number_of_unique_knots()? as f64),
        OpIn::Polygon { a, b } => {
            let pts = c.polygon(&ivl(*a, *b))?;
            o.push(pts.len() as f64);
            for p in pts {
                push3(o, p);
            }
        }
        OpIn::Match { other } => {
            let other = build_this(other)?;
            c.match_weights(&other)?;
        }
        OpIn::Elevate { deg } => c.degree_elevate(*deg)?,
        OpIn::Reverse { a, b } => {
            let r = c.reverse_parameterization(&ivl(*a, *b))?;
            o.extend([r.min(), r.max()]);
        }
        OpIn::SetCanonical { data } => {
            let pts: Vec<Vec3> = data.pts.iter().map(|&p| v3(p)).collect();
            c.set_canonical(&CanonicalData {
                dim: data.dim,
                degree: data.deg,
                control_points: &pts,
                form: form_of(data.form)?,
                knot_multiplicities: &data.mults,
                knots: &data.knots,
                knot_type: knot_type_of(data.kt)?,
                weights: data.weights.as_deref(),
                trim: data.trim.map(|[a, b]| ivl(a, b)),
            })?;
        }
        OpIn::SetCp { form, index, p, w } => {
            c.set_control_point(cp_form_of(*form)?, *index, v3(*p), *w)?;
        }
        OpIn::Adjust { a, b } => {
            let k = Kernel::default();
            c.adjust_step_interval(&k, &ivl(*a, *b))?;
        }
        _ => return Err(harness("not a curve op").into()),
    }
    Ok(())
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    // joins: status, new curve, then every segment
    if let OpIn::Join { segs, senses } = &i.op {
        let mut set = segs.iter().map(build_this).collect::<Result<Vec<_>, _>>()?;
        let mut o = Vec::new();
        match BSplineCurve::create_by_joining(&mut set, senses.as_deref()) {
            Ok(c) => {
                o.push(0.0);
                push_new(&mut o, Some(&c));
            }
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                o.push(status(&e));
            }
        }
        for c in &set {
            dump_state(&mut o, c);
        }
        return to_output(i, &o);
    }

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
    let p = 1 + rng.below(6) as usize;
    let extra = rng.below(8) as usize;
    let rational = rng.chance(0.25);
    let three_d = rng.chance(0.75);
    gen_crv_full(rng, p, extra, p, rational, three_d)
}

/// A piecewise C0 cubic (every interior knot triple), as intersection curves are.
fn gen_pw_c0(rng: &mut Rng) -> InlineCrv {
    let segs = 1 + rng.below(4) as usize;
    let a = rng.uniform(-2.0, 2.0);
    let mut u = vec![a; 4];
    let mut t = a;
    for s in 0..segs {
        t += rng.uniform(0.2, 2.0);
        let mult = if s + 1 == segs { 4 } else { 3 };
        u.extend(std::iter::repeat_n(t, mult));
    }
    let n = u.len() - 5;
    let mut cpts = Vec::with_capacity(n + 1);
    let mut p = rng.point3(-5.0, 5.0);
    for _ in 0..=n {
        let d = rng.point3(-2.0, 2.0);
        p = [p[0] + d[0] + 1.0, p[1] + d[1], p[2] + d[2]];
        cpts.push([p[0], p[1], p[2], NOW]);
    }
    InlineCrv { cpts, p: 3, u }
}

/// A non-rational Bezier line with evenly spaced (or slightly perturbed) points.
fn gen_line_bezier(rng: &mut Rng) -> InlineCrv {
    let p = 1 + rng.below(4) as usize;
    let s = rng.point3(-5.0, 5.0);
    let e = rng.point3(-5.0, 5.0);
    let jitter = if rng.chance(0.3) { 1e-3 } else { 0.0 };
    let cpts = (0..=p)
        .map(|i| {
            let t = i as f64 / p as f64;
            let j = if i > 0 && i < p { jitter } else { 0.0 };
            [
                s[0] + t * (e[0] - s[0]) + j,
                s[1] + t * (e[1] - s[1]),
                s[2] + t * (e[2] - s[2]),
                NOW,
            ]
        })
        .collect();
    let (a, b) = (rng.uniform(-2.0, 2.0), rng.uniform(2.5, 6.0));
    let mut u = vec![a; p + 1];
    u.extend(std::iter::repeat_n(b, p + 1));
    InlineCrv { cpts, p, u }
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

/// Moves `c` so that its start (or end, when `at_end`) is `to`.
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

/// End-to-end connected segments (some reversed through their senses).
fn gen_join(rng: &mut Rng) -> OpIn {
    let k = 1 + rng.below(4) as usize;
    let three_d = rng.chance(0.7);
    let any_rational = rng.chance(0.3);
    let mut segs: Vec<InlineCrv> = Vec::with_capacity(k);
    let mut senses = Vec::with_capacity(k);
    let mut cur_end: Option<[f64; 3]> = None;
    for _ in 0..k {
        let p = 1 + rng.below(4) as usize;
        let extra = rng.below(3) as usize;
        let rational = any_rational && rng.chance(0.5);
        let mut c = gen_crv_full(rng, p, extra, 1, rational, three_d);
        let sense = rng.chance(0.7);
        if let Some(e) = cur_end {
            // the joined start of this segment is its start (sense) or its end
            let at = if sense { first_point(&c) } else { last_point(&c) };
            translate_to(&mut c, at, e);
        }
        cur_end = Some(if sense { last_point(&c) } else { first_point(&c) });
        senses.push(sense);
        segs.push(c);
    }
    let with_senses = rng.chance(0.6) || senses.iter().any(|s| !s);
    if three_d && rng.chance(0.2) {
        // mixed dimensions: some segments 2D (the 3D ones are made 2D by the join)
        for c in &mut segs {
            if rng.chance(0.5) {
                for q in &mut c.cpts {
                    q[2] = NOZ;
                }
            }
        }
    }
    let mut segs: Vec<This> = segs.into_iter().map(|c| this_of(rng, c)).collect();
    if rng.chance(0.1) {
        // a gap: the join fails
        if let Some(This {
            crv: Some(CrvIn::Inline(c)),
            ..
        }) = segs.last_mut()
        {
            if let Some(p) = c.cpts.first_mut() {
                p[0] += 1.0;
            }
        }
    }
    OpIn::Join {
        segs,
        senses: if with_senses { Some(senses) } else { None },
    }
}

fn gen_canon(rng: &mut Rng) -> CanonIn {
    let deg = 1 + rng.below(4) as usize;
    let extra = rng.below(4) as usize;
    let ncp = deg + 1 + extra;
    let pts = (0..ncp).map(|_| rng.point3(-5.0, 5.0)).collect();
    let mut knots = vec![0.0];
    let mut mults = vec![deg + 1];
    for i in 0..extra {
        knots.push((i + 1) as f64);
        mults.push(1);
    }
    knots.push((extra + 1) as f64);
    mults.push(deg + 1);
    CanonIn {
        dim: *rng.choose(&[3u64, 3, 2]).unwrap_or(&3),
        deg,
        pts,
        form: rng.below(7) as u8,
        knots,
        mults,
        kt: rng.below(5) as u8,
        weights: if rng.chance(0.3) {
            Some((0..ncp).map(|_| rng.uniform(0.5, 2.0)).collect())
        } else {
            None
        },
        trim: None,
    }
}

fn gen_curve_op(rng: &mut Rng) -> Inputs {
    let pick = rng.below(20);
    let c = match pick {
        0 if rng.chance(0.6) => gen_pw_c0(rng),
        1 | 7 if rng.chance(0.5) => gen_line_bezier(rng),
        _ => gen_crv(rng),
    };
    let (a, b) = ends(&c);
    let len = b - a;
    let n = c.cpts.len() - 1;
    let p = c.p;
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
    let op = match pick {
        0 => OpIn::Intersector,
        1 => {
            let (x, y) = sub(rng);
            OpIn::BBox {
                a: x,
                b: y,
                normal: rng.chance(0.8),
                pseudo: rng.chance(0.7),
                polar: rng.chance(0.6),
            }
        }
        2 => {
            let (x, y) = sub(rng);
            OpIn::Analytic { a: x, b: y }
        }
        3 => OpIn::Canonical,
        4 => OpIn::C0,
        5 => OpIn::Unique,
        6 => {
            let (x, y) = sub(rng);
            OpIn::Polygon {
                a: x,
                b: if rng.chance(0.1) { y + len } else { y },
            }
        }
        7 => OpIn::IsLine {
            ns: rng.below(12) as usize,
            tol: *rng.choose(&[1e-9, 1e-6, 1e-3, 0.1, 0.0]).unwrap_or(&1e-6),
        },
        8 => {
            let start = rng.below(n as u64 + 2) as usize;
            let end = start + rng.below(4) as usize;
            OpIn::Expert {
                form: rng.below(3) as u8,
                start,
                end,
                stride: 2 + rng.below(5) as usize,
            }
        }
        9 => {
            let other = if rng.chance(0.8) {
                // the same geometry, other weights
                let mut o = c.clone();
                let make_rational = rng.chance(0.6);
                for q in &mut o.cpts {
                    if q[3] == NOW && !make_rational {
                        continue;
                    }
                    let f = if rng.chance(0.3) { 1.0 } else { rng.uniform(0.4, 2.5) };
                    if q[3] == NOW {
                        q[3] = 1.0;
                    }
                    q[0] *= f;
                    q[1] *= f;
                    if q[2] != NOZ {
                        q[2] *= f;
                    }
                    q[3] *= f;
                }
                o
            } else {
                gen_crv(rng)
            };
            OpIn::Match {
                other: this_of(rng, other),
            }
        }
        10 => OpIn::Elevate {
            deg: if rng.chance(0.1) {
                p.saturating_sub(1)
            } else {
                p + rng.below(4) as usize
            },
        },
        11 | 12 => {
            let (x, y) = sub(rng);
            OpIn::Reverse { a: x, b: y }
        }
        13 => OpIn::SetCanonical {
            data: gen_canon(rng),
        },
        14 | 15 => OpIn::SetCp {
            form: rng.below(3) as u8,
            index: rng.below(n as u64 + 2) as usize,
            p: rng.point3(-5.0, 5.0),
            w: *rng.choose(&[1.0, 2.0, 0.5, 1.5]).unwrap_or(&1.0),
        },
        16 => {
            let (x, y) = match rng.below(4) {
                0 => (a - rng.uniform(0.05, 0.5) * len, b),
                1 => (a, b + rng.uniform(0.05, 0.5) * len),
                2 => (a - rng.uniform(0.05, 0.5) * len, b + rng.uniform(0.05, 0.5) * len),
                _ => sub(rng),
            };
            OpIn::Adjust { a: x, b: y }
        }
        17 => OpIn::Repeated {
            tol: *rng.choose(&[0.0, 1e-5, 0.1]).unwrap_or(&0.0),
        },
        18 => OpIn::Degenerate {
            tol: 1e-9,
            ivl: if rng.chance(0.5) { Some([a, b]) } else { None },
        },
        _ => OpIn::MaxDomain,
    };
    Inputs {
        this: Some(this_of(rng, c)),
        op,
    }
}

fn gen_static_op(rng: &mut Rng) -> Inputs {
    let op = match rng.below(6) {
        0..=2 => gen_join(rng),
        3 => OpIn::Point {
            p: rng.point3(-10.0, 10.0),
        },
        _ => {
            let c = gen_crv(rng);
            let rat = c.cpts.first().is_some_and(|p| p[3] != NOW);
            let three = c.cpts.first().is_some_and(|p| p[2] != NOZ);
            let mut vals = Vec::new();
            for q in &c.cpts {
                vals.extend([q[0], q[1]]);
                if three {
                    vals.push(q[2]);
                }
                if rat {
                    vals.push(q[3]);
                }
            }
            vals.extend_from_slice(&c.u);
            let version = *rng.choose(&[30u64, 33, 40]).unwrap_or(&30);
            OpIn::ReadDb {
                dim: *rng.choose(&[3u64, 2]).unwrap_or(&3),
                binary: rng.chance(0.5),
                version,
                db: DbIn::Data(DbData {
                    n: (c.cpts.len() - 1) as i64,
                    p: c.p as i16,
                    rat: i16::from(rat),
                    dim: if three { 3 } else { 2 },
                    kt: rng.below(5),
                    form: rng.below(7),
                    vals,
                }),
            }
        }
    };
    Inputs { this: None, op }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.25) {
        gen_static_op(rng)
    } else {
        gen_curve_op(rng)
    }
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
        "nurbs.b_spline_curve_p2"
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
        let k = &Kernel::default();
        let i: Inputs = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: the Rust side honours sizes
        // the FFI refuses (fuzz00841: a 2^31 stride, a 16 GiB output array).
        ffi::validate(i.op.ffi(), &cpp_input(&i)?).map_err(|e| harness(e.to_string()))?;
        rust(k, &i)
    }
}
