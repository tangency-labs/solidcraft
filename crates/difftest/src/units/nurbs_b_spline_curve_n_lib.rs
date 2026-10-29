//! Runner for unit `nurbs.b_spline_curve_n_lib`: the `SmBSplineCurve` methods built on
//! NLib (arc, rational-curve and helix approximation, extension, interpolation, knot
//! insertion and spacing, degree reduction, evenly spaced points, compatible knot
//! vectors, end point deformation, arc-length reparametrisation, scaling, the
//! intersection-curve through-point edit and trimming).
//!
//! C++: `solidcore_ffi::units::nurbs_b_spline_curve_n_lib` (upstream
//! `source/SMLib/src/SmBSplineCurveNLib.cpp`). Rust:
//! `solidcore::nurbs::b_spline_curve_n_lib`.
//!
//! A case is `this` (the curve a member op runs on: `dim`, `oob` and an inline, `.smc`,
//! interpolated or circle curve, built with the fast constructor owning its NURB),
//! absent for the static ops, plus `op` and its parameters. Interpolated (`interp`) and
//! circle (`circle`) inputs are built by the Rust side's `InterpolatePoints` /
//! `CreateCircleSegment` (both bit-identical to upstream, see their units) and handed to
//! both sides as inline curves. Both sides produce the same flat record, split into
//! keys by `to_output`: `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001`
//! `SM_ERR`, `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream
//! cannot produce), the op's outputs, the state of a curve the op created (`new_*`), of
//! the curves of a curve set (`c<i>_*`) and of `this` after the op (`this_*`). A state
//! is `flags` (`dim oob borrowed knot_type form has_nurb`) and, with a NURB, `dims`
//! (`n p m`), `cpts` (x y z w as stored) and `u`.
//!
//! Cases whose Rust side reaches an unported callee are deferred: the
//! `SetIntersectionCurveThroughPoint` calls on intersector-form curves that reach
//! `SmCurve::ApproximateLength` (unit `curve.curve_p2`). Every run has at least
//! [`MIN_GENERATED`] generated cases; generated inputs the ffi wrapper rejects are
//! redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::b_spline_curve_n_lib::{HelixSpec, InValueType, TrimMode};
use solidcore::nurbs::crv_adv::p1::CurveEnd;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::geom::vector2d::Vec2;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::nurbs_b_spline_curve_n_lib::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nurbs.b_spline_curve_n_lib` runner.
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

/// A full quadratic circle (`CreateCircleSegment`, 0 to 360 degrees) about +z.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    center: [f64; 3],
    radius: f64,
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Built by interpolation.
    Interp { interp: InterpIn },
    /// A circle.
    Circle { circle: CircleIn },
    /// Inline data.
    Inline(InlineCrv),
}

/// A curve block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct This {
    dim: u64,
    #[serde(default)]
    oob: bool,
    crv: CrvIn,
}

/// An inline B-spline surface: control net as stored (`[x, y, z, w]`, rows in u),
/// degrees, knots (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A UV trim curve of the through-point edit: the 2D curve (sharing the knots of
/// `this`), its surface and the optional UV point of the through point on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UvIn {
    crv: InlineCrv,
    srf: InlineSrf,
    #[serde(default)]
    uvp: Option<[f64; 2]>,
}

/// A frame `[origin, x axis, y axis]` (`SetCanonical`).
type Frame = [[f64; 3]; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Arc {
        dim: u64,
        #[serde(default)]
        nc: Option<u64>,
        frame: Frame,
        r: f64,
        a0: f64,
        a1: f64,
        tol: f64,
    },
    Rational {
        tol: f64,
    },
    Helix {
        frame: Frame,
        h: f64,
        r0: f64,
        r1: f64,
        turns: f64,
        right: bool,
        tol: f64,
    },
    ExtParam {
        t: f64,
        cont: u8,
        precise: bool,
    },
    ExtDist {
        d: f64,
        end: u8,
        cont: u8,
    },
    InterpCrv {
        par: u8,
        dim: u64,
        deg: usize,
        pts: Vec<[f64; 3]>,
        #[serde(default)]
        vecs: Vec<[f64; 3]>,
        #[serde(default)]
        more: Option<Vec<[f64; 3]>>,
        ignore: bool,
    },
    Dampen {
        factor: f64,
        mult: usize,
    },
    DegRed {
        tol: f64,
        max: bool,
    },
    InsOne {
        knot: f64,
        times: usize,
    },
    Through {
        knot: f64,
        p: [f64; 3],
        #[serde(default)]
        tan: Option<[f64; 3]>,
        pct: f64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        uv: Vec<UvIn>,
    },
    Even {
        t0: f64,
        t1: f64,
        n: usize,
        tol: f64,
    },
    InsKnots {
        knots: Vec<f64>,
    },
    InterpPts {
        pts: Vec<[f64; 3]>,
        #[serde(default)]
        params: Option<Vec<f64>>,
        deg: usize,
        #[serde(default)]
        ts: Option<[f64; 3]>,
        #[serde(default)]
        te: Option<[f64; 3]>,
        closed: bool,
        par: u8,
    },
    Compat {
        curves: Vec<This>,
        tol: f64,
    },
    Deform {
        #[serde(default)]
        s: Option<[f64; 3]>,
        #[serde(default)]
        e: Option<[f64; 3]>,
        #[serde(default)]
        st: Option<[f64; 3]>,
        st_type: u8,
        #[serde(default)]
        et: Option<[f64; 3]>,
        et_type: u8,
    },
    Reparam,
    Scale {
        f: f64,
    },
    ScaleKnots {
        a: f64,
        b: f64,
    },
    Sync {
        curves: Vec<This>,
        tol: f64,
    },
    Trim {
        a: f64,
        b: f64,
        mode: u8,
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
            OpIn::Arc { .. } => Op::Arc,
            OpIn::Rational { .. } => Op::Rational,
            OpIn::Helix { .. } => Op::Helix,
            OpIn::ExtParam { .. } => Op::ExtParam,
            OpIn::ExtDist { .. } => Op::ExtDist,
            OpIn::InterpCrv { .. } => Op::InterpCrv,
            OpIn::Dampen { .. } => Op::Dampen,
            OpIn::DegRed { .. } => Op::DegRed,
            OpIn::InsOne { .. } => Op::InsOne,
            OpIn::Through { .. } => Op::Through,
            OpIn::Even { .. } => Op::Even,
            OpIn::InsKnots { .. } => Op::InsKnots,
            OpIn::InterpPts { .. } => Op::InterpPts,
            OpIn::Compat { .. } => Op::Compat,
            OpIn::Deform { .. } => Op::Deform,
            OpIn::Reparam => Op::Reparam,
            OpIn::Scale { .. } => Op::Scale,
            OpIn::ScaleKnots { .. } => Op::ScaleKnots,
            OpIn::Sync { .. } => Op::Sync,
            OpIn::Trim { .. } => Op::Trim,
        }
    }

    /// Whether a successful op is followed by a flag and the state of a new curve.
    fn creates(&self) -> bool {
        matches!(
            self,
            OpIn::Arc { .. }
                | OpIn::Rational { .. }
                | OpIn::Helix { .. }
                | OpIn::ExtParam { .. }
                | OpIn::ExtDist { .. }
                | OpIn::InterpCrv { .. }
                | OpIn::InterpPts { .. }
        )
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
                0.0,
                360.0,
                NurbCircleParam::Quadratic,
            )
            .map_err(|e| harness(format!("circle input: {e}")))?;
            let cur = c.gw_nurb().ok_or_else(|| harness("circle input: no NURB"))?;
            Ok(of_curve(cur))
        }
    }
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

fn encode_srf(s: &InlineSrf, o: &mut Vec<f64>) {
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    o.extend([
        n as f64,
        m as f64,
        s.p as f64,
        s.q as f64,
        s.u.len().saturating_sub(1) as f64,
        s.v.len().saturating_sub(1) as f64,
    ]);
    for row in &s.net {
        for c in row {
            o.extend_from_slice(c);
        }
    }
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
}

fn encode_this(t: &This, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    o.extend([t.dim as f64, flag(t.oob)]);
    encode_crv(&resolve(&t.crv)?, o);
    Ok(())
}

fn encode_frame(f: &Frame, o: &mut Vec<f64>) {
    for v in f {
        o.extend_from_slice(v);
    }
}

fn encode_pts(p: &[[f64; 3]], o: &mut Vec<f64>) {
    o.push(p.len() as f64);
    for v in p {
        o.extend_from_slice(v);
    }
}

fn encode_opt3(v: Option<[f64; 3]>, o: &mut Vec<f64>) {
    o.push(flag(v.is_some()));
    o.extend_from_slice(&v.unwrap_or([0.0; 3]));
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    if let Some(t) = &i.this {
        encode_this(t, &mut o)?;
    }
    match &i.op {
        OpIn::Arc {
            dim,
            nc,
            frame,
            r,
            a0,
            a1,
            tol,
        } => {
            o.extend([*dim as f64, flag(nc.is_some()), nc.unwrap_or(0) as f64]);
            encode_frame(frame, &mut o);
            o.extend([*r, *a0, *a1, *tol]);
        }
        OpIn::Rational { tol } => o.push(*tol),
        OpIn::Helix {
            frame,
            h,
            r0,
            r1,
            turns,
            right,
            tol,
        } => {
            encode_frame(frame, &mut o);
            o.extend([*h, *r0, *r1, *turns, flag(*right), *tol]);
        }
        OpIn::ExtParam { t, cont, precise } => o.extend([*t, f64::from(*cont), flag(*precise)]),
        OpIn::ExtDist { d, end, cont } => o.extend([*d, f64::from(*end), f64::from(*cont)]),
        OpIn::InterpCrv {
            par,
            dim,
            deg,
            pts,
            vecs,
            more,
            ignore,
        } => {
            o.extend([f64::from(*par), *dim as f64, *deg as f64]);
            encode_pts(pts, &mut o);
            encode_pts(vecs, &mut o);
            o.push(flag(more.is_some()));
            encode_pts(more.as_deref().unwrap_or(&[]), &mut o);
            o.push(flag(*ignore));
        }
        OpIn::Dampen { factor, mult } => o.extend([*factor, *mult as f64]),
        OpIn::DegRed { tol, max } => o.extend([*tol, flag(*max)]),
        OpIn::InsOne { knot, times } => o.extend([*knot, *times as f64]),
        OpIn::Through {
            knot,
            p,
            tan,
            pct,
            uv,
        } => {
            o.push(*knot);
            o.extend_from_slice(p);
            encode_opt3(*tan, &mut o);
            o.push(*pct);
            o.push(uv.len() as f64);
            for e in uv {
                o.extend([2.0, 0.0]);
                encode_crv(&e.crv, &mut o);
                encode_srf(&e.srf, &mut o);
                o.push(flag(e.uvp.is_some()));
                o.extend_from_slice(&e.uvp.unwrap_or([0.0; 2]));
            }
        }
        OpIn::Even { t0, t1, n, tol } => o.extend([*t0, *t1, *n as f64, *tol]),
        OpIn::InsKnots { knots } => {
            o.push(knots.len() as f64);
            o.extend_from_slice(knots);
        }
        OpIn::InterpPts {
            pts,
            params,
            deg,
            ts,
            te,
            closed,
            par,
        } => {
            encode_pts(pts, &mut o);
            let prm = params.as_deref().unwrap_or(&[]);
            o.extend([flag(params.is_some()), prm.len() as f64]);
            o.extend_from_slice(prm);
            o.push(*deg as f64);
            encode_opt3(*ts, &mut o);
            encode_opt3(*te, &mut o);
            o.extend([flag(*closed), f64::from(*par)]);
        }
        OpIn::Compat { curves, tol } | OpIn::Sync { curves, tol } => {
            o.push(curves.len() as f64);
            for c in curves {
                encode_this(c, &mut o)?;
            }
            o.push(*tol);
        }
        OpIn::Deform {
            s,
            e,
            st,
            st_type,
            et,
            et_type,
        } => {
            encode_opt3(*s, &mut o);
            encode_opt3(*e, &mut o);
            encode_opt3(*st, &mut o);
            o.push(f64::from(*st_type));
            encode_opt3(*et, &mut o);
            o.push(f64::from(*et_type));
        }
        OpIn::Reparam => {}
        OpIn::Scale { f } => o.push(*f),
        OpIn::ScaleKnots { a, b } => o.extend([*a, *b]),
        OpIn::Trim { a, b, mode } => o.extend([*a, *b, f64::from(*mode)]),
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
            OpIn::Through { .. } => s.vals("through", 2)?,
            OpIn::Even { .. } => {
                let k = s.count()?;
                let out = std::mem::take(&mut s.out);
                s.out = out.int("even_count", k as i64);
                s.vals("even_pts", 3 * k)?;
                s.vals("even_prm", k)?;
            }
            _ => {}
        }
        if i.op.creates() {
            let has = s.take(1)?;
            let created = has.first().copied() == Some(1.0);
            s.out = s.out.values("new", has);
            if created {
                s.state("new_")?;
            }
        }
    }
    match &i.op {
        OpIn::Trim { .. } => s.vals("ivl", 2)?,
        OpIn::Compat { curves, .. } | OpIn::Sync { curves, .. } => {
            for k in 0..curves.len() {
                s.state(&format!("c{k}_"))?;
            }
        }
        OpIn::Through { uv, .. } => {
            for k in 0..uv.len() {
                s.state(&format!("uv{k}_"))?;
            }
        }
        _ => {}
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

fn to_surface(s: &InlineSrf) -> Result<BSplineSurface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
        .collect();
    let sur = Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface: {e}")))?;
    Ok(BSplineSurface::from_nurb(sur, false))
}

fn build_this(t: &This) -> Result<BSplineCurve, HarnessError> {
    let mut c = BSplineCurve::from_nurb(to_curve(&resolve(&t.crv)?)?, t.dim, false);
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

fn frame_of(f: &Frame) -> Result<Axis2Placement, SolidError> {
    let mut a = Axis2Placement::default();
    a.set_canonical(v3(f[0]), v3(f[1]), v3(f[2]))?;
    Ok(a)
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

/// `SM_CP_*` / `SM_IT_*` (same order).
fn param_of(v: u8, interpolation: bool) -> ParamMethod {
    match v {
        0 => ParamMethod::Uniform,
        1 => ParamMethod::ChordLength,
        2 => ParamMethod::Centripetal,
        _ if interpolation => ParamMethod::ChordLength,
        _ => ParamMethod::Uniform,
    }
}

fn in_value_of(v: u8) -> Result<InValueType, HarnessError> {
    Ok(match v {
        0 => InValueType::Specified,
        1 => InValueType::Same,
        2 => InValueType::Unconstrained,
        _ => return Err(harness(format!("bad in-value type {v}"))),
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

fn push_new(o: &mut Vec<f64>, c: Option<&BSplineCurve>) {
    o.push(flag(c.is_some()));
    if let Some(c) = c {
        dump_state(o, c);
    }
}

fn pts_of(p: &[[f64; 3]]) -> Vec<Vec3> {
    p.iter().map(|&x| v3(x)).collect()
}

/// Runs a static op; pushes its outputs (after the status) on success.
fn rust_static(op: &OpIn, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::Arc {
            dim,
            nc,
            frame,
            r,
            a0,
            a1,
            tol,
        } => {
            let f = frame_of(frame)?;
            let c = BSplineCurve::approximate_arc(*dim, *nc, &f, *r, *a0, *a1, *tol)?;
            push_new(o, Some(&c));
        }
        OpIn::Helix {
            frame,
            h,
            r0,
            r1,
            turns,
            right,
            tol,
        } => {
            let f = frame_of(frame)?;
            let c = BSplineCurve::create_helix_segment(&HelixSpec {
                frame: f,
                height: *h,
                radius_start: *r0,
                radius_end: *r1,
                num_turns: *turns,
                right_handed: *right,
                approx_tol: *tol,
            })?;
            push_new(o, Some(&c));
        }
        OpIn::InterpCrv {
            par,
            dim,
            deg,
            pts,
            vecs,
            more,
            ignore,
        } => {
            let more = more.as_ref().map(|m| pts_of(m));
            let c = BSplineCurve::create_interpolating_curve(
                param_of(*par, false),
                *dim,
                *deg,
                &pts_of(pts),
                &pts_of(vecs),
                more.as_deref(),
                *ignore,
            )?;
            push_new(o, Some(&c));
        }
        OpIn::InterpPts {
            pts,
            params,
            deg,
            ts,
            te,
            closed,
            par,
        } => {
            let c = BSplineCurve::interpolate_points(
                &pts_of(pts),
                params.as_deref(),
                *deg,
                ts.map(v3),
                te.map(v3),
                *closed,
                param_of(*par, true),
            )?;
            push_new(o, Some(&c));
        }
        _ => return Err(harness("not a static op").into()),
    }
    Ok(())
}

/// Runs a curve-set op; returns the status and the curves after the call.
fn rust_set(op: &OpIn) -> Result<(f64, Vec<BSplineCurve>), Fail> {
    let (curves, tol, compat) = match op {
        OpIn::Compat { curves, tol } => (curves, *tol, true),
        OpIn::Sync { curves, tol } => (curves, *tol, false),
        _ => return Err(harness("not a curve-set op").into()),
    };
    let mut set = curves.iter().map(build_this).collect::<Result<Vec<_>, _>>()?;
    let res = if compat {
        BSplineCurve::make_curves_compatible(&mut set, tol)
    } else {
        BSplineCurve::synchronize_knots_of_curves(&mut set, tol)
    };
    match res {
        Ok(()) => Ok((0.0, set)),
        Err(e @ SolidError::Unported { .. }) => Err(Fail::Solid(e)),
        Err(e) => Ok((status(&e), set)),
    }
}

/// Runs a curve op on `c`; pushes its outputs (after the status) on success.
fn rust_curve(op: &OpIn, c: &mut BSplineCurve, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::Rational { tol } => {
            let r = c.approximate_rational_curve(*tol)?;
            push_new(o, Some(&r));
        }
        OpIn::ExtParam { t, cont, precise } => {
            let r = c.create_extended_curve(*t, continuity_of(*cont)?, *precise)?;
            push_new(o, r.as_ref());
        }
        OpIn::ExtDist { d, end, cont } => {
            let end = match end {
                1 => CurveEnd::Start,
                2 => CurveEnd::End,
                _ => return Err(harness("bad end flag").into()),
            };
            let r = c.create_extended_curve_by_distance(*d, end, continuity_of(*cont)?)?;
            push_new(o, r.as_ref());
        }
        OpIn::Dampen { factor, mult } => c.dampen_knot_spacing(*factor, *mult)?,
        OpIn::DegRed { tol, max } => c.degree_reduction(*tol, *max)?,
        OpIn::InsOne { knot, times } => c.insert_one_knot(*knot, *times)?,
        OpIn::Through { .. } => return Err(harness("through: handled by rust_through").into()),
        OpIn::Even { t0, t1, n, tol } => {
            let (pts, prm) = c.equally_spaced_points(*t0, *t1, *n, *tol)?;
            o.push(pts.len() as f64);
            for p in pts {
                push3(o, p);
            }
            o.extend_from_slice(&prm);
        }
        OpIn::InsKnots { knots } => c.insert_knots(knots)?,
        OpIn::Deform {
            s,
            e,
            st,
            st_type,
            et,
            et_type,
        } => c.deform_to_end_point_targets(
            s.map(v3),
            e.map(v3),
            st.map(v3),
            in_value_of(*st_type)?,
            et.map(v3),
            in_value_of(*et_type)?,
        )?,
        OpIn::Reparam => c.reparametrize_with_arc_length()?,
        OpIn::Scale { f } => c.scale(*f)?,
        OpIn::ScaleKnots { a, b } => c.scale_knot_vector(*a, *b)?,
        _ => return Err(harness("not a curve op").into()),
    }
    Ok(())
}

fn trim_mode(v: u8) -> Result<TrimMode, HarnessError> {
    Ok(match v {
        0 => TrimMode::Notify,
        1 => TrimMode::Silent,
        2 => TrimMode::IntervalOnly,
        _ => return Err(harness(format!("bad trim mode {v}"))),
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    // curve sets
    if matches!(i.op, OpIn::Compat { .. } | OpIn::Sync { .. }) {
        return match rust_set(&i.op) {
            Ok((st, set)) => {
                let mut o = vec![st];
                for c in &set {
                    dump_state(&mut o, c);
                }
                to_output(i, &o)
            }
            Err(Fail::Harness(e)) => Err(e),
            Err(Fail::Solid(e)) => Output::unported(&e).ok_or_else(|| harness(e.to_string())),
        };
    }

    let mut this = i.this.as_ref().map(build_this).transpose()?;

    // trim: the interval is output whatever the status
    if let (OpIn::Trim { a, b, mode }, Some(c)) = (&i.op, this.as_mut()) {
        let k = Kernel::default();
        let mut ivl = Extent1d::from_min_max(*a, *b);
        let st = match c.trim(&k, &mut ivl, trim_mode(*mode)?) {
            Ok(()) => 0.0,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                status(&e)
            }
        };
        let mut o = vec![st, ivl.min(), ivl.max()];
        dump_state(&mut o, c);
        return to_output(i, &o);
    }

    // through point: the UV curves are output whatever the status
    if let (
        OpIn::Through {
            knot,
            p,
            tan,
            pct,
            uv,
        },
        Some(c),
    ) = (&i.op, this.as_mut())
    {
        let mut uv_curves = uv
            .iter()
            .map(|e| Ok(BSplineCurve::from_nurb(to_curve(&e.crv)?, 2, false)))
            .collect::<Result<Vec<_>, HarnessError>>()?;
        let surfaces = uv
            .iter()
            .map(|e| to_surface(&e.srf))
            .collect::<Result<Vec<_>, _>>()?;
        let uv_points: Vec<Option<Vec2>> = uv
            .iter()
            .map(|e| e.uvp.map(|[a, b]| Vec2::new(a, b)))
            .collect();
        let mut k = *knot;
        let mut o = match c.set_intersection_curve_through_point(
            &Kernel::default(),
            &mut k,
            v3(*p),
            tan.map(v3),
            &mut uv_curves,
            &surfaces,
            &uv_points,
            *pct,
        ) {
            Ok(modified) => vec![0.0, flag(modified), k],
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                vec![status(&e)]
            }
        };
        for u in &uv_curves {
            dump_state(&mut o, u);
        }
        dump_state(&mut o, c);
        return to_output(i, &o);
    }

    let mut o = vec![0.0];
    let res = match this.as_mut() {
        Some(c) => rust_curve(&i.op, c, &mut o),
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
fn gen_crv_full(rng: &mut Rng, p: usize, extra: usize, max_mult: usize, rational: bool) -> InlineCrv {
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
    let three_d = rng.chance(0.75);
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
    gen_crv_full(rng, p, extra, p, rational)
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

fn ends(c: &InlineCrv) -> (f64, f64) {
    (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    )
}

fn gen_vec(rng: &mut Rng, lo: f64, hi: f64) -> [f64; 3] {
    rng.point3(lo, hi)
}

fn gen_frame(rng: &mut Rng) -> Frame {
    match rng.below(8) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        1 => [gen_vec(rng, -5.0, 5.0), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        2 if rng.chance(0.2) => [gen_vec(rng, -5.0, 5.0), [1.0, 1.0, 0.0], [2.0, 2.0, 0.0]],
        _ => [
            gen_vec(rng, -5.0, 5.0),
            gen_vec(rng, -1.0, 1.0),
            gen_vec(rng, -1.0, 1.0),
        ],
    }
}

/// Distinct data points along a wandering path.
fn gen_points(rng: &mut Rng, k: usize) -> Vec<[f64; 3]> {
    let mut p = gen_vec(rng, -5.0, 5.0);
    let planar = rng.chance(0.3);
    (0..k)
        .map(|_| {
            let d = gen_vec(rng, -2.0, 2.0);
            p = [p[0] + d[0] + 0.5, p[1] + d[1], if planar { 0.0 } else { p[2] + d[2] }];
            p
        })
        .collect()
}

fn this_of(rng: &mut Rng, c: InlineCrv) -> This {
    let three_d = c.cpts.first().is_some_and(|p| p[2] != NOZ);
    This {
        dim: if three_d { 3 } else { 2 },
        oob: rng.chance(0.2),
        crv: CrvIn::Inline(c),
    }
}

fn gen_cont(rng: &mut Rng) -> u8 {
    *rng.choose(&[3u8, 3, 5, 5, 13, 13, 13, 7, 2]).unwrap_or(&13)
}

fn gen_curve_op(rng: &mut Rng) -> Inputs {
    let pick = rng.below(17);
    let c = match pick {
        // rational approximation: rational curves of moderate degree
        0 => {
            let p = 1 + rng.below(5) as usize;
            let extra = rng.below(5) as usize;
            let rational = rng.chance(0.85);
            gen_crv_full(rng, p, extra, 1, rational)
        }
        // through point: mostly piecewise C0 cubics
        9 if rng.chance(0.7) => gen_pw_c0(rng),
        // deformation: mostly odd degree
        13 => {
            let p = *rng.choose(&[3usize, 3, 5, 1, 2, 4]).unwrap_or(&3);
            let extra = rng.below(7) as usize;
            let rational = rng.chance(0.2);
            gen_crv_full(rng, p, extra, 1, rational)
        }
        _ => gen_crv(rng),
    };
    let (a, b) = ends(&c);
    let len = b - a;
    let p = c.p;
    let op = match pick {
        0 => OpIn::Rational {
            tol: *rng.choose(&[1e-3, 1e-2, 1e-4, 0.1, 0.0, 0.5]).unwrap_or(&1e-3),
        },
        1 => OpIn::ExtParam {
            t: match rng.below(5) {
                0 => rng.uniform(a, b),
                1 => a - rng.uniform(0.01, 2.0) * len,
                _ => b + rng.uniform(0.01, 2.0) * len,
            },
            cont: gen_cont(rng),
            precise: rng.chance(0.5),
        },
        2 => OpIn::ExtDist {
            d: rng.uniform(0.01, 5.0),
            end: 1 + rng.below(2) as u8,
            cont: gen_cont(rng),
        },
        3 => OpIn::Dampen {
            factor: *rng.choose(&[2.0, 3.0, 1.5, 4.0, 10.0, 1.01]).unwrap_or(&2.0),
            mult: rng.below(p as u64 + 1) as usize,
        },
        4 => OpIn::DegRed {
            tol: *rng.choose(&[1e-6, 1e-3, 0.1, 1.0, 100.0]).unwrap_or(&1e-3),
            max: rng.chance(0.5),
        },
        5 => OpIn::InsOne {
            knot: if rng.chance(0.85) {
                rng.uniform(a, b)
            } else {
                *rng.choose(&c.u).unwrap_or(&a)
            },
            times: rng.below(p as u64 + 2) as usize,
        },
        6 => {
            let x = rng.uniform(a, b);
            let y = rng.uniform(a, b);
            OpIn::Even {
                t0: if rng.chance(0.3) { a } else { x.min(y) },
                t1: if rng.chance(0.3) { b } else { x.max(y) },
                n: 2 + rng.below(20) as usize,
                tol: *rng.choose(&[1e-3, 1e-2, 0.1, 1e-6]).unwrap_or(&1e-3),
            }
        }
        7 => {
            let k = 1 + rng.below(6) as usize;
            let mut knots: Vec<f64> = (0..k).map(|_| rng.uniform(a + 1e-3 * len, b - 1e-3 * len)).collect();
            knots.sort_by(f64::total_cmp);
            OpIn::InsKnots { knots }
        }
        8 => OpIn::Reparam,
        9 => {
            let knot = match rng.below(6) {
                0 => a - rng.uniform(0.0, 0.5) * len,
                1 => b + rng.uniform(0.0, 0.5) * len,
                2 => *rng.choose(&c.u).unwrap_or(&a),
                _ => rng.uniform(a, b),
            };
            OpIn::Through {
                knot,
                p: gen_vec(rng, -8.0, 8.0),
                tan: if rng.chance(0.5) {
                    Some(gen_vec(rng, -1.0, 1.0))
                } else {
                    None
                },
                pct: *rng.choose(&[0.001, 0.1, 0.3]).unwrap_or(&0.001),
                uv: Vec::new(),
            }
        }
        10 => OpIn::Scale {
            f: rng.uniform(-3.0, 3.0),
        },
        11 => {
            let x = rng.uniform(-5.0, 5.0);
            OpIn::ScaleKnots {
                a: x,
                b: if rng.chance(0.1) { x - 1.0 } else { x + rng.uniform(0.1, 10.0) },
            }
        }
        12 | 14 => {
            let x = rng.uniform(a, b);
            let y = rng.uniform(a, b);
            let (lo, hi) = match rng.below(6) {
                0 => (a, x),
                1 => (x, b),
                2 => (a - 1e-12, x),
                3 => (a - 0.5 * len, x),
                _ => (x.min(y), x.max(y)),
            };
            let (lo, hi) = if rng.chance(0.2) {
                let k = *rng.choose(&c.u).unwrap_or(&a);
                (lo, k + 1e-11)
            } else {
                (lo, hi)
            };
            OpIn::Trim {
                a: lo,
                b: hi,
                mode: *rng.choose(&[0u8, 0, 1, 2]).unwrap_or(&0),
            }
        }
        13 => {
            let tan_type = |rng: &mut Rng| *rng.choose(&[0u8, 1, 1, 2]).unwrap_or(&1);
            let st_type = tan_type(rng);
            let et_type = tan_type(rng);
            OpIn::Deform {
                s: if rng.chance(0.7) { Some(gen_vec(rng, -10.0, 10.0)) } else { None },
                e: if rng.chance(0.5) { Some(gen_vec(rng, -10.0, 10.0)) } else { None },
                st: if st_type == 0 || rng.chance(0.2) {
                    Some(gen_vec(rng, -1.0, 1.0))
                } else {
                    None
                },
                st_type,
                et: if et_type == 0 || rng.chance(0.2) {
                    Some(gen_vec(rng, -1.0, 1.0))
                } else {
                    None
                },
                et_type,
            }
        }
        _ => OpIn::Scale {
            f: *rng.choose(&[1.0, 0.0, -1.0, 2.5]).unwrap_or(&1.0),
        },
    };
    Inputs {
        this: Some(this_of(rng, c)),
        op,
    }
}

fn gen_static_op(rng: &mut Rng) -> Inputs {
    let op = match rng.below(10) {
        0 | 1 => OpIn::Arc {
            dim: *rng.choose(&[3u64, 3, 2, 1]).unwrap_or(&3),
            nc: if rng.chance(0.3) { Some(rng.below(12)) } else { None },
            frame: gen_frame(rng),
            r: if rng.chance(0.05) { 0.0 } else { rng.uniform(0.1, 20.0) },
            a0: rng.uniform(-360.0, 360.0),
            a1: match rng.below(6) {
                0 => 90.0,
                1 => 360.0,
                _ => rng.uniform(-360.0, 360.0),
            },
            tol: *rng.choose(&[0.01, 0.1, 1.0, 1e-4, 1e-6]).unwrap_or(&0.01),
        },
        2 => OpIn::Helix {
            frame: gen_frame(rng),
            h: if rng.chance(0.05) { -1.0 } else { rng.uniform(0.5, 30.0) },
            r0: rng.uniform(0.0, 5.0),
            r1: rng.uniform(0.1, 5.0),
            turns: rng.uniform(0.2, 6.0),
            right: rng.chance(0.5),
            tol: *rng.choose(&[1e-3, 1e-4, 1e-2]).unwrap_or(&1e-3),
        },
        3..=5 => {
            let k = 2 + rng.below(9) as usize;
            let pts = gen_points(rng, k);
            let deg = 1 + rng.below(5) as usize;
            let nv = match rng.below(5) {
                0 => k,
                1 | 2 => 0,
                3 => 2,
                _ => 1,
            };
            let vecs = (0..nv).map(|_| gen_vec(rng, -5.0, 5.0)).collect();
            let more = if rng.chance(0.4) {
                let nm = *rng.choose(&[1usize, 2, 4]).unwrap_or(&2);
                Some((0..nm).map(|_| gen_vec(rng, -5.0, 5.0)).collect())
            } else {
                None
            };
            OpIn::InterpCrv {
                par: rng.below(3) as u8,
                dim: *rng.choose(&[3u64, 3, 2, 1]).unwrap_or(&3),
                deg,
                pts,
                vecs,
                more,
                ignore: rng.chance(0.5),
            }
        }
        6..=8 => {
            let k = 2 + rng.below(9) as usize;
            let mut pts = gen_points(rng, k);
            let closed = rng.chance(0.3);
            if closed {
                if let Some(&first) = pts.first() {
                    pts.push(first);
                }
            }
            let params = if rng.chance(0.3) {
                let mut t = rng.uniform(-1.0, 1.0);
                Some(
                    (0..pts.len())
                        .map(|_| {
                            t += rng.uniform(0.1, 2.0);
                            t
                        })
                        .collect(),
                )
            } else {
                None
            };
            OpIn::InterpPts {
                pts,
                params,
                deg: 1 + rng.below(4) as usize,
                ts: if rng.chance(0.4) { Some(gen_vec(rng, -3.0, 3.0)) } else { None },
                te: if rng.chance(0.4) { Some(gen_vec(rng, -3.0, 3.0)) } else { None },
                closed,
                par: rng.below(3) as u8,
            }
        }
        _ => {
            let k = 1 + rng.below(3) as usize;
            let curves = (0..k)
                .map(|_| {
                    let c = gen_crv(rng);
                    this_of(rng, c)
                })
                .collect();
            if rng.chance(0.5) {
                OpIn::Compat {
                    curves,
                    tol: *rng.choose(&[1e-8, 1e-6, 1e-3, 0.0]).unwrap_or(&1e-8),
                }
            } else {
                OpIn::Sync {
                    curves,
                    tol: *rng.choose(&[1e-8, 1e-6, 1e-3]).unwrap_or(&1e-6),
                }
            }
        }
    };
    Inputs { this: None, op }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.3) {
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
        "nurbs.b_spline_curve_n_lib"
    }

    fn skipped_platform(&self, case: &Case) -> Option<String> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64"))
            && matches!(
                case.name.as_str(),
                "gen0695" | "gen0720" | "gen0880" | "gen0932" | "gen1279" | "gen1374" | "gen1482"
            )
            && case.inputs.get("op").and_then(Value::as_str) == Some("deform")
        {
            Some(
                "upstream use-after-free in SmBSplineCurveNLib.cpp:3127-3149 (issue #449); Rust matches the Linux reference"
                    .into(),
            )
        } else {
            None
        }
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
        let i: Inputs = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: the Rust side honours sizes
        // the FFI refuses (fuzz00880: a 2^31 count, 16 GiB knot arrays).
        ffi::validate(i.op.ffi(), &cpp_input(&i)?).map_err(|e| harness(e.to_string()))?;
        rust(&i)
    }
}
