//! Runner for unit `surface.volume_p2`: the second half of the `SmVolume` base class
//! (compound domain checks of lines and boxes, bounding boxes through the orient and
//! next maps, compound point evaluation, the inverse point solvers,
//! `HasDiscontinuities` and the polymorphic `ReadFromDB`).
//!
//! C++: `solidcore_ffi::units::surface_volume_p2` (upstream
//! `source/SMLib/src/SmVolume.cpp` on chains of plain `SmVolume`, `SmTransform`,
//! `SmTwistVolume`, `SmBSplineVolume`, default and component-constructed
//! `SmUnbendVolume` and `SmBendVolume` maps, each but the twist and the component
//! unbend and bend optionally oriented by an `SmTransform`). Rust:
//! `solidcore::surface::volume`.
//!
//! A case is a volume (`vol`: maps `{kind, ...}`, kind `0` plain, `1` transform `t =
//! [origin, X, Y, Z]`, `2` twist `tw`, `3` B-spline `bs`, `4` default unbend, `5`
//! component unbend `ub`, `6` default bend, `7` component bend `ub`; optional
//! `orient`) and an `op` with its parameters, or a
//! `read_db` of ASCII text. Both sides
//! produce the flat record documented in the ffi module; the runner reports its first
//! entry as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a
//! Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest
//! as `out`.
//!
//! Deferred cases: ops reaching unported callees of the unbend volume (its
//! `IsCurveInParamDomainSimple` needs `SmCurve::HasPlaneCrossings`) or of the curve
//! bounding boxes its `EvaluateBoundingBoxSimple` computes, and of the bend volume
//! (its `IsCurveInParamDomainSimple` needs `SmCurve::IsOnPositiveSideOfPlanes`).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject};
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::spiral::allocate_nurb_volume;
use solidcore::surface::b_spline_volume::BSplineVolume;
use solidcore::surface::bend_volume::BendVolume;
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::unbend_volume::UnbendVolume;
use solidcore::surface::volume::{Discontinuities3d, InvPoints, Volume, VolumeKind};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::surface_volume_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

/// Generated cases per run (at least).
pub const MIN_GENERATED: usize = 2000;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

type Tf = [[f64; 3]; 4];
type Bx = [[f64; 3]; 2];

/// A twist's component constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TwistIn {
    o: [f64; 3],
    axis: [f64; 3],
    mid: [f64; 3],
    period: f64,
    min: f64,
}

/// An unbend's or a bend's component constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UnbendIn {
    c: [f64; 3],
    axis: [f64; 3],
    mid: [f64; 3],
    d: f64,
    tol: f64,
}

/// A B-spline record.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BsIn {
    hi: [usize; 3],
    deg: [usize; 3],
    knots: [Vec<f64>; 3],
    cpts: Vec<[f64; 4]>,
}

/// One map of a volume chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MapIn {
    /// `0` plain, `1` transform, `2` twist, `3` B-spline, `4` default unbend, `5`
    /// component unbend, `6` default bend, `7` component bend.
    kind: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t: Option<Tf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tw: Option<TwistIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bs: Option<BsIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ub: Option<UnbendIn>,
    /// The orient transform `[origin, X, Y, Z]` (not for a twist or a component
    /// unbend or bend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orient: Option<Tf>,
}

type Vol = Vec<MapIn>;

/// A pseudo box: basis and `(a, b)` intervals (`SmExtent1d(a, b)`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PbIn {
    basis: [[f64; 3]; 3],
    ivl: [[f64; 2]; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    PtInParam {
        vol: Vol,
        p: [f64; 3],
        comp: bool,
    },
    LineInInSpace {
        vol: Vol,
        a: [f64; 3],
        b: [f64; 3],
        comp: bool,
    },
    BoxInParam {
        vol: Vol,
        bx: Bx,
        comp: bool,
    },
    PboxInInSpace {
        vol: Vol,
        pb: PbIn,
        comp: bool,
    },
    EvalBbox {
        vol: Vol,
        bx: Bx,
        #[serde(default)]
        ppb: Option<PbIn>,
        /// `[out_box, out_pseudo_box, in_box, in_pseudo_box]` requested.
        want: [bool; 4],
        comp: bool,
    },
    MapPbox {
        vol: Vol,
        pb: PbIn,
        want: [bool; 2],
        comp: bool,
        trim: bool,
    },
    EvalPoint {
        vol: Vol,
        p: [f64; 3],
        comp: bool,
    },
    Eval1st {
        vol: Vol,
        p: [f64; 3],
        left: [bool; 3],
        nz: bool,
        comp: bool,
    },
    InvEval {
        vol: Vol,
        p: [f64; 3],
        #[serde(default)]
        guess: Option<[f64; 3]>,
        #[serde(default)]
        dom: Option<Bx>,
        comp: bool,
    },
    InvMap {
        vol: Vol,
        p: [f64; 3],
        #[serde(default)]
        guess: Option<[f64; 3]>,
        #[serde(default)]
        dom: Option<PbIn>,
        comp: bool,
    },
    Global {
        vol: Vol,
        p: [f64; 3],
        #[serde(default)]
        dom: Option<Bx>,
        comp: bool,
    },
    HasDisc {
        vol: Vol,
        want: bool,
        geometric: bool,
    },
    BboxSimple {
        vol: Vol,
        bx: Bx,
        #[serde(default)]
        ppb: Option<PbIn>,
        want: [bool; 2],
    },
    PboxSimple {
        vol: Vol,
        pb: PbIn,
        want: [bool; 2],
    },
    InvEvalSimple {
        vol: Vol,
        p: [f64; 3],
        #[serde(default)]
        guess: Option<[f64; 3]>,
        #[serde(default)]
        dom: Option<Bx>,
    },
    GlobalSimple {
        vol: Vol,
        p: [f64; 3],
        #[serde(default)]
        dom: Option<Bx>,
    },
    LocalSimple {
        vol: Vol,
        p: [f64; 3],
        g: [f64; 3],
        #[serde(default)]
        dom: Option<Bx>,
    },
    ReadDb {
        #[serde(rename = "type")]
        ty: i64,
        data: String,
    },
}

impl Inputs {
    fn vol(&self) -> Option<&Vol> {
        match self {
            Self::PtInParam { vol, .. }
            | Self::LineInInSpace { vol, .. }
            | Self::BoxInParam { vol, .. }
            | Self::PboxInInSpace { vol, .. }
            | Self::EvalBbox { vol, .. }
            | Self::MapPbox { vol, .. }
            | Self::EvalPoint { vol, .. }
            | Self::Eval1st { vol, .. }
            | Self::InvEval { vol, .. }
            | Self::InvMap { vol, .. }
            | Self::Global { vol, .. }
            | Self::HasDisc { vol, .. }
            | Self::BboxSimple { vol, .. }
            | Self::PboxSimple { vol, .. }
            | Self::InvEvalSimple { vol, .. }
            | Self::GlobalSimple { vol, .. }
            | Self::LocalSimple { vol, .. } => Some(vol),
            Self::ReadDb { .. } => None,
        }
    }
}

// ---- C++ side -----------------------------------------------------------------------

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn push_tf(o: &mut Vec<f64>, t: &Tf) {
    for p in t {
        o.extend(p);
    }
}

fn push_vol(o: &mut Vec<f64>, v: &Vol) -> Result<(), HarnessError> {
    o.push(v.len() as f64);
    for m in v {
        o.push(f64::from(m.kind));
        match m.kind {
            1 => push_tf(o, &m.t.ok_or_else(|| harness("transform map without t"))?),
            2 => {
                let t = m.tw.as_ref().ok_or_else(|| harness("twist map without tw"))?;
                o.extend(t.o);
                o.extend(t.axis);
                o.extend(t.mid);
                o.extend([t.period, t.min]);
            }
            3 => {
                let r = m.bs.as_ref().ok_or_else(|| harness("B-spline map without bs"))?;
                o.extend(r.hi.map(|x| x as f64));
                o.extend(r.deg.map(|x| x as f64));
                for k in &r.knots {
                    o.extend(k);
                }
                for c in &r.cpts {
                    o.extend(c);
                }
            }
            5 | 7 => {
                let u = m.ub.as_ref().ok_or_else(|| harness("(un)bend map without ub"))?;
                o.extend(u.c);
                o.extend(u.axis);
                o.extend(u.mid);
                o.extend([u.d, u.tol]);
            }
            _ => {}
        }
        if m.kind != 2 && m.kind != 5 && m.kind != 7 {
            match &m.orient {
                Some(t) => {
                    o.push(1.0);
                    push_tf(o, t);
                }
                None => o.push(0.0),
            }
        }
    }
    Ok(())
}

fn push_box(o: &mut Vec<f64>, bx: &Bx) {
    o.extend(bx[0]);
    o.extend(bx[1]);
}

fn push_pb(o: &mut Vec<f64>, pb: &PbIn) {
    for v in pb.basis {
        o.extend(v);
    }
    for i in pb.ivl {
        o.extend(i);
    }
}

#[allow(clippy::too_many_lines)]
fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>, Vec<u8>), HarnessError> {
    let mut o = Vec::new();
    if let Some(v) = i.vol() {
        push_vol(&mut o, v)?;
    }
    let mut data = Vec::new();
    let op = match i {
        Inputs::PtInParam { p, comp, .. } => {
            o.extend(p);
            o.push(b(*comp));
            Op::PtInParam
        }
        Inputs::LineInInSpace { a, b: e, comp, .. } => {
            o.extend(a);
            o.extend(e);
            o.push(b(*comp));
            Op::LineInInSpace
        }
        Inputs::BoxInParam { bx, comp, .. } => {
            push_box(&mut o, bx);
            o.push(b(*comp));
            Op::BoxInParam
        }
        Inputs::PboxInInSpace { pb, comp, .. } => {
            push_pb(&mut o, pb);
            o.push(b(*comp));
            Op::PBoxInInSpace
        }
        Inputs::EvalBbox {
            bx,
            ppb,
            want,
            comp,
            ..
        } => {
            push_box(&mut o, bx);
            o.push(b(ppb.is_some()));
            if let Some(pb) = ppb {
                push_pb(&mut o, pb);
            }
            o.extend(want.map(b));
            o.push(b(*comp));
            Op::EvalBBox
        }
        Inputs::MapPbox {
            pb,
            want,
            comp,
            trim,
            ..
        } => {
            push_pb(&mut o, pb);
            o.extend(want.map(b));
            o.extend([b(*comp), b(*trim)]);
            Op::MapPBox
        }
        Inputs::EvalPoint { p, comp, .. } => {
            o.extend(p);
            o.push(b(*comp));
            Op::EvalPoint
        }
        Inputs::Eval1st {
            p, left, nz, comp, ..
        } => {
            o.extend(p);
            o.extend(left.map(b));
            o.extend([b(*nz), b(*comp)]);
            Op::Eval1st
        }
        Inputs::InvEval {
            p,
            guess,
            dom,
            comp,
            ..
        } => {
            o.extend(p);
            o.push(b(guess.is_some()));
            if let Some(g) = guess {
                o.extend(g);
            }
            o.push(b(dom.is_some()));
            if let Some(d) = dom {
                push_box(&mut o, d);
            }
            o.push(b(*comp));
            Op::InvEval
        }
        Inputs::InvMap {
            p,
            guess,
            dom,
            comp,
            ..
        } => {
            o.extend(p);
            o.push(b(guess.is_some()));
            if let Some(g) = guess {
                o.extend(g);
            }
            o.push(b(dom.is_some()));
            if let Some(d) = dom {
                push_pb(&mut o, d);
            }
            o.push(b(*comp));
            Op::InvMap
        }
        Inputs::Global { p, dom, comp, .. } => {
            o.extend(p);
            o.push(b(dom.is_some()));
            if let Some(d) = dom {
                push_box(&mut o, d);
            }
            o.push(b(*comp));
            Op::Global
        }
        Inputs::HasDisc {
            want, geometric, ..
        } => {
            o.extend([b(*want), b(*geometric)]);
            Op::HasDisc
        }
        Inputs::BboxSimple { bx, ppb, want, .. } => {
            push_box(&mut o, bx);
            o.push(b(ppb.is_some()));
            if let Some(pb) = ppb {
                push_pb(&mut o, pb);
            }
            o.extend(want.map(b));
            Op::BBoxSimple
        }
        Inputs::PboxSimple { pb, want, .. } => {
            push_pb(&mut o, pb);
            o.extend(want.map(b));
            Op::PBoxSimple
        }
        Inputs::InvEvalSimple { p, guess, dom, .. } => {
            o.extend(p);
            o.push(b(guess.is_some()));
            if let Some(g) = guess {
                o.extend(g);
            }
            o.push(b(dom.is_some()));
            if let Some(d) = dom {
                push_box(&mut o, d);
            }
            Op::InvEvalSimple
        }
        Inputs::GlobalSimple { p, dom, .. } => {
            o.extend(p);
            o.push(b(dom.is_some()));
            if let Some(d) = dom {
                push_box(&mut o, d);
            }
            Op::GlobalSimple
        }
        Inputs::LocalSimple { p, g, dom, .. } => {
            o.extend(p);
            o.extend(g);
            o.push(b(dom.is_some()));
            if let Some(d) = dom {
                push_box(&mut o, d);
            }
            Op::LocalSimple
        }
        Inputs::ReadDb { ty, data: text } => {
            o.push(*ty as f64);
            data = text.clone().into_bytes();
            Op::ReadDb
        }
    };
    Ok((op, o, data))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input, data) = cpp_input(i)?;
    match ffi::eval(op, &input, &data) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side ----------------------------------------------------------------------

/// The volume's solution handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Me;

impl SolutionObject for Me {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Volume
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn transform_of(t: &Tf) -> Transform {
    let [o, x, y, z] = *t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn ext(bx: &Bx) -> Extent3d {
    Extent3d::from_points(v3(bx[0]), v3(bx[1]))
}

fn pbox(pb: &PbIn) -> PseudoBox {
    let [i0, i1, i2] = pb.ivl.map(|[a, e]| Extent1d::from_min_max(a, e));
    PseudoBox::from_basis_intervals(pb.basis.map(v3), [i0, i1, i2])
}

fn bspline(r: &BsIn) -> Result<BSplineVolume, HarnessError> {
    let [l, m, n] = r.hi;
    let [p, q, s] = r.deg;
    let mut rec = allocate_nurb_volume(r.hi, r.deg, [l + p + 1, m + q + 1, n + s + 1])
        .map_err(|e| harness(e.to_string()))?;
    for (d, k) in r.knots.iter().enumerate() {
        let dst = rec.knots_mut(d);
        if dst.len() != k.len() {
            return Err(harness("knot count mismatch"));
        }
        dst.copy_from_slice(k);
    }
    if rec.cpts().len() != r.cpts.len() {
        return Err(harness("control point count mismatch"));
    }
    for (dst, c) in rec.cpts_mut().iter_mut().zip(&r.cpts) {
        let [x, y, z, w] = *c;
        *dst = CPoint::new(x, y, z, w);
    }
    Ok(BSplineVolume::new(rec))
}

fn build_map(m: &MapIn) -> Result<Volume, HarnessError> {
    let orient = m.orient.as_ref().map(transform_of);
    Ok(match m.kind {
        1 => Volume::oriented(
            VolumeKind::Transform(transform_of(
                m.t.as_ref().ok_or_else(|| harness("transform map without t"))?,
            )),
            orient,
        ),
        2 => {
            let t = m.tw.as_ref().ok_or_else(|| harness("twist map without tw"))?;
            TwistVolume::create(v3(t.o), v3(t.axis), v3(t.mid), t.period, t.min)
        }
        3 => Volume::oriented(
            VolumeKind::BSpline(bspline(
                m.bs.as_ref().ok_or_else(|| harness("B-spline map without bs"))?,
            )?),
            orient,
        ),
        4 => Volume::oriented(VolumeKind::Unbend(UnbendVolume::new()), orient),
        5 => {
            let u = m.ub.as_ref().ok_or_else(|| harness("unbend map without ub"))?;
            UnbendVolume::create(v3(u.c), v3(u.axis), v3(u.mid), u.d, u.tol)
        }
        6 => Volume::oriented(VolumeKind::Bend(BendVolume::new()), orient),
        7 => {
            let u = m.ub.as_ref().ok_or_else(|| harness("bend map without ub"))?;
            BendVolume::create(v3(u.c), v3(u.axis), v3(u.mid), u.d, u.tol)
        }
        _ => Volume::oriented(VolumeKind::Base, orient),
    })
}

fn build(v: &Vol) -> Result<Volume, HarnessError> {
    let mut maps = v.iter();
    let mut head = build_map(maps.next().ok_or_else(|| harness("empty volume"))?)?;
    for m in maps {
        head.add_next_map(build_map(m)?, 2)
            .map_err(|e| harness(e.to_string()))?;
    }
    Ok(head)
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_box(o: &mut Vec<f64>, bx: &Extent3d) {
    pv(o, bx.min());
    pv(o, bx.max());
}

fn put_pb(o: &mut Vec<f64>, pb: &PseudoBox) {
    for v in pb.basis() {
        pv(o, *v);
    }
    for i in 0..3 {
        let ivl = pb.interval(i).unwrap_or_default();
        o.extend([ivl.min(), ivl.max()]);
    }
}

fn put_mat(o: &mut Vec<f64>, t: &Transform) {
    for row in t.matrix() {
        o.extend(row);
    }
}

fn put_state(o: &mut Vec<f64>, v: &Volume) {
    let mut maps = vec![v];
    while let Some(next) = maps.last().and_then(|m| m.next_map()) {
        maps.push(next);
    }
    o.push(maps.len() as f64);
    for m in maps {
        o.push(m.type_code() as f64);
        match m.kind() {
            VolumeKind::Transform(t) => put_mat(o, t),
            VolumeKind::Twist(t) => o.extend([t.twist_period_dist(), t.period_min_dist()]),
            VolumeKind::Unbend(u) => o.extend([u.unbend_neutral_dist(), u.singularity_tol()]),
            VolumeKind::Bend(b) => o.extend([b.bend_neutral_dist(), b.singularity_tol()]),
            _ => {}
        }
        for t in [m.orient_map(), m.inv_orient_map()] {
            match t {
                Some(t) => {
                    o.push(1.0);
                    put_mat(o, t);
                }
                None => o.push(0.0),
            }
        }
    }
}

fn cont(c: ContinuityType) -> f64 {
    f64::from(c as i32)
}

fn put_disc(o: &mut Vec<f64>, d: &Discontinuities3d) {
    o.extend([b(d.has_discontinuities), cont(d.min_continuity)]);
    for dir in [&d.u, &d.v, &d.w] {
        o.extend([b(dir.unaligned), cont(dir.min_cont), dir.params.len() as f64]);
    }
}

fn put_solution(o: &mut Vec<f64>, s: &Solution<Me>) {
    o.extend([
        f64::from(s.solution_type.code()),
        s.num_variables as f64,
        s.start.solution_value,
        s.start.parameters[0],
        s.start.parameters[1],
        s.start.parameters[2],
        s.num_objects as f64,
    ]);
}

fn put_inv(o: &mut Vec<f64>, success: bool, points: &[Vec3], gaps: &[f64]) {
    o.extend([0.0, b(success), points.len() as f64]);
    for p in points {
        pv(o, *p);
    }
    o.push(gaps.len() as f64);
    o.extend(gaps);
}

fn put_solutions(o: &mut Vec<f64>, found: bool, sols: &SolutionArray<Me>) {
    o.extend([0.0, b(found), sols.len() as f64]);
    for s in sols.as_slice() {
        put_solution(o, s);
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// Evaluates `r`: `Ok(v)`, or returns the deferral / status record.
macro_rules! tri {
    ($o:ident, $r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                $o.push(status(&e));
                return Ok(to_output(&$o));
            }
        }
    };
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    if let Inputs::ReadDb { ty, data } = i {
        let mut db = DatabaseReader::new();
        db.open_for_read(data.clone().into_bytes(), FileType::Ascii);
        let v = match Volume::read_from_db(*ty, &mut db, 30) {
            Ok(v) => v,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                // no volume made
                return Ok(to_output(&[status(&e), 0.0]));
            }
        };
        o.extend([0.0, 1.0]);
        put_state(&mut o, &v);
        return Ok(to_output(&o));
    }
    let v = build(i.vol().ok_or_else(|| harness("no volume"))?)?;
    let k = Kernel::default();
    match i {
        Inputs::PtInParam { p, comp, .. } => {
            let r = tri!(o, v.is_point_in_param_domain(v3(*p), *comp));
            o.extend([0.0, b(r)]);
        }
        Inputs::LineInInSpace { a, b: e, comp, .. } => {
            let (r, pa, pb) = tri!(o, v.is_line_in_in_space_domain(&k, v3(*a), v3(*e), *comp));
            o.extend([0.0, b(r)]);
            pv(&mut o, pa);
            pv(&mut o, pb);
        }
        Inputs::BoxInParam { bx, comp, .. } => {
            let r = tri!(o, v.is_box_in_param_domain(&k, &ext(bx), *comp));
            o.extend([0.0, b(r)]);
        }
        Inputs::PboxInInSpace { pb, comp, .. } => {
            let r = tri!(o, v.is_pseudo_box_in_in_space_domain(&k, &pbox(pb), *comp));
            o.extend([0.0, b(r)]);
        }
        Inputs::EvalBbox {
            bx,
            ppb,
            want,
            comp,
            ..
        } => {
            let ppb = ppb.as_ref().map(pbox);
            let (mut ob, mut ib) = (Extent3d::new(), Extent3d::new());
            let (mut opb, mut ipb) = (PseudoBox::new(), PseudoBox::new());
            let [w0, w1, w2, w3] = *want;
            tri!(
                o,
                v.evaluate_bounding_box(
                    &k,
                    &ext(bx),
                    ppb.as_ref(),
                    w0.then_some(&mut ob),
                    w1.then_some(&mut opb),
                    w2.then_some(&mut ib),
                    w3.then_some(&mut ipb),
                    *comp,
                )
            );
            o.push(0.0);
            if w0 {
                put_box(&mut o, &ob);
            }
            if w1 {
                put_pb(&mut o, &opb);
            }
            if w2 {
                put_box(&mut o, &ib);
            }
            if w3 {
                put_pb(&mut o, &ipb);
            }
        }
        Inputs::MapPbox {
            pb,
            want,
            comp,
            trim,
            ..
        } => {
            let mut ob = Extent3d::new();
            let mut opb = PseudoBox::new();
            let [w0, w1] = *want;
            tri!(
                o,
                v.map_pseudo_box(
                    &k,
                    &pbox(pb),
                    w0.then_some(&mut ob),
                    w1.then_some(&mut opb),
                    *comp,
                    *trim,
                )
            );
            o.push(0.0);
            if w0 {
                put_box(&mut o, &ob);
            }
            if w1 {
                put_pb(&mut o, &opb);
            }
        }
        Inputs::EvalPoint { p, comp, .. } => {
            let q = tri!(o, v.evaluate_point(v3(*p), *comp));
            o.push(0.0);
            pv(&mut o, q);
        }
        Inputs::Eval1st {
            p, left, nz, comp, ..
        } => {
            let d = tri!(o, v.evaluate_1st_derivatives(v3(*p), *left, *nz, *comp));
            o.push(0.0);
            for x in d {
                pv(&mut o, x);
            }
        }
        Inputs::InvEval {
            p,
            guess,
            dom,
            comp,
            ..
        } => {
            let d = dom.as_ref().map(ext);
            let r: InvPoints = tri!(
                o,
                v.inv_evaluate_point(&k, v3(*p), guess.map(v3), d.as_ref(), *comp)
            );
            put_inv(&mut o, r.success, &r.points, &r.gaps);
        }
        Inputs::InvMap {
            p,
            guess,
            dom,
            comp,
            ..
        } => {
            let d = dom.as_ref().map(pbox);
            let r = tri!(o, v.inv_map_point(&k, v3(*p), guess.map(v3), d.as_ref(), *comp));
            put_inv(&mut o, r.success, &r.points, &r.gaps);
        }
        Inputs::Global { p, dom, comp, .. } => {
            let d = dom.as_ref().map(ext);
            let (found, sols) = tri!(o, v.global_point_solve(&k, v3(*p), d.as_ref(), *comp, Me));
            put_solutions(&mut o, found, &sols);
        }
        Inputs::HasDisc {
            want, geometric, ..
        } => {
            let mut d = Discontinuities3d::new();
            d.set_unaligned();
            let r = tri!(o, v.has_discontinuities(&k, want.then_some(&mut d), *geometric));
            o.extend([0.0, b(r)]);
            put_disc(&mut o, &d);
        }
        Inputs::BboxSimple { bx, ppb, want, .. } => {
            let ppb = ppb.as_ref().map(pbox);
            let mut ob = Extent3d::new();
            let mut opb = PseudoBox::new();
            let [w0, w1] = *want;
            tri!(
                o,
                v.evaluate_bounding_box_simple(&k, 
                    &ext(bx),
                    ppb.as_ref(),
                    w0.then_some(&mut ob),
                    w1.then_some(&mut opb),
                )
            );
            o.push(0.0);
            if w0 {
                put_box(&mut o, &ob);
            }
            if w1 {
                put_pb(&mut o, &opb);
            }
        }
        Inputs::PboxSimple { pb, want, .. } => {
            let mut ob = Extent3d::new();
            let mut opb = PseudoBox::new();
            let [w0, w1] = *want;
            tri!(
                o,
                v.evaluate_pseudo_box_simple(
                    &pbox(pb),
                    w0.then_some(&mut ob),
                    w1.then_some(&mut opb)
                )
            );
            o.push(0.0);
            if w0 {
                put_box(&mut o, &ob);
            }
            if w1 {
                put_pb(&mut o, &opb);
            }
        }
        Inputs::InvEvalSimple { p, guess, dom, .. } => {
            let d = dom.as_ref().map(ext);
            let (ok, pts, gaps) =
                tri!(o, v.inv_evaluate_point_simple(v3(*p), guess.map(v3), d.as_ref()));
            put_inv(&mut o, ok, &pts, &gaps);
        }
        Inputs::GlobalSimple { p, dom, .. } => {
            let d = dom.as_ref().map(ext);
            let mut sols = SolutionArray::new();
            let found = tri!(o, v.global_point_solve_simple(v3(*p), &mut sols, d.as_ref(), Me));
            put_solutions(&mut o, found, &sols);
        }
        Inputs::LocalSimple { p, g, dom, .. } => {
            let d = dom.as_ref().map(ext);
            let mut sol = Solution::new();
            let found = tri!(
                o,
                v.local_point_solve_simple(v3(*p), v3(*g), &mut sol, d.as_ref(), Me)
            );
            o.extend([0.0, b(found)]);
            put_solution(&mut o, &sol);
        }
        Inputs::ReadDb { .. } => {}
    }
    Ok(to_output(&o))
}

// ---- generation ---------------------------------------------------------------------

/// `SM_INFINITE_PARAMETER`, the unbounded interval marker.
const INF: f64 = 1_234_567.0;

fn coord(rng: &mut Rng, scale: f64) -> [f64; 3] {
    let p = rng.point3(-scale, scale);
    // sometimes integral coordinates (exact arithmetic paths)
    if rng.chance(0.15) {
        p.map(f64::round)
    } else {
        p
    }
}

/// A transform `[origin, X, Y, Z]`: identity, rotation, singular, uniform scale,
/// tiny or huge, general.
fn gen_transform(rng: &mut Rng) -> Tf {
    match rng.below(10) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        1 | 2 => {
            let a = rng.uniform(-3.2, 3.2);
            let (s, c) = a.sin_cos();
            [coord(rng, 5.0), [c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
        }
        3 => {
            let x = coord(rng, 2.0);
            [coord(rng, 5.0), x, x, coord(rng, 2.0)]
        }
        4 => {
            let s = rng.uniform(0.1, 4.0);
            [coord(rng, 5.0), [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
        }
        5 => {
            // tiny or huge scale
            let s = if rng.chance(0.5) { 1e-6 } else { 1e5 };
            [coord(rng, 5.0), [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
        }
        6 => {
            // axis permutation (aligned, negated)
            let [x, y, z] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            // (axes[(k + 1) % 3], axes[(k + 2) % 3], axes[k])
            let (u, v, w) = match rng.below(3) {
                0 => (y, z, x),
                1 => (z, x, y),
                _ => (x, y, z),
            };
            let sgn = if rng.chance(0.5) { -1.0 } else { 1.0 };
            [coord(rng, 5.0), u.map(|c| c * sgn), v, w]
        }
        _ => [coord(rng, 5.0), coord(rng, 2.0), coord(rng, 2.0), coord(rng, 2.0)],
    }
}

fn gen_knots(rng: &mut Rng, count: usize) -> Vec<f64> {
    let lo = if rng.chance(0.4) {
        0.0
    } else {
        rng.uniform(-3.0, 3.0)
    };
    let span = rng.uniform(0.5, 4.0);
    if rng.chance(0.5) {
        return (0..count)
            .map(|i| lo + span * i as f64 / (count.max(2) - 1) as f64)
            .collect();
    }
    let mut v: Vec<f64> = (0..count).map(|_| lo + span * rng.unit()).collect();
    v.sort_by(f64::total_cmp);
    v
}

fn gen_bspline(rng: &mut Rng) -> BsIn {
    let hi = [
        1 + rng.below(3) as usize,
        1 + rng.below(3) as usize,
        rng.below(3) as usize,
    ];
    let deg = hi.map(|h| (1 + rng.below(h as u64) as usize).min(h.max(1)).min(3));
    let deg = [deg[0], deg[1], deg[2].min(hi[2])];
    let knots = [
        gen_knots(rng, hi[0] + deg[0] + 2),
        gen_knots(rng, hi[1] + deg[1] + 2),
        gen_knots(rng, hi[2] + deg[2] + 2),
    ];
    let count = (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1);
    let rational = rng.chance(0.3);
    let cpts = (0..count)
        .map(|_| {
            let p = coord(rng, 8.0);
            let w = if rational { rng.uniform(0.3, 2.0) } else { 1.0 };
            [p[0], p[1], p[2], w]
        })
        .collect();
    BsIn {
        hi,
        deg,
        knots,
        cpts,
    }
}

fn gen_twist(rng: &mut Rng) -> TwistIn {
    let axis = match rng.below(6) {
        0 => [0.0, 0.0, 1.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0; 3],
        _ => coord(rng, 2.0),
    };
    let mid = match rng.below(6) {
        0 => [1.0, 0.0, 0.0],
        1 => axis.map(|c| c * 2.0),
        _ => coord(rng, 2.0),
    };
    let period = match rng.below(8) {
        0 => 0.0,
        1 => -rng.uniform(1.0, 20.0),
        2 => 1e-9,
        _ => rng.uniform(1.0, 40.0),
    };
    TwistIn {
        o: coord(rng, 5.0),
        axis,
        mid,
        period,
        min: if rng.chance(0.2) { 0.0 } else { 1e-3 },
    }
}

fn gen_unbend(rng: &mut Rng) -> UnbendIn {
    let axis = match rng.below(6) {
        0 => [0.0, 0.0, 1.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0; 3],
        _ => coord(rng, 2.0),
    };
    let mid = match rng.below(6) {
        0 => [1.0, 0.0, 0.0],
        1 => axis.map(|c| c * 2.0),
        2 => [0.0; 3],
        _ => coord(rng, 2.0),
    };
    let tol = match rng.below(6) {
        0 => 0.0,
        1 => 1e-3,
        2 => 0.5,
        _ => 0.01,
    };
    let d = match rng.below(8) {
        0 => 0.0,
        1 => -rng.uniform(0.1, 5.0),
        2 => tol * 0.5,
        3 => 1e-6,
        4 => 1e4,
        _ => rng.uniform(0.1, 10.0),
    };
    UnbendIn {
        c: coord(rng, 5.0),
        axis,
        mid,
        d,
        tol,
    }
}

/// Which bending maps a generated chain may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bending {
    None,
    Unbend,
    Bend,
    Both,
}

/// Kinds weighted towards evaluating maps; unbends and bends only when allowed.
fn gen_map(rng: &mut Rng, bending: Bending) -> MapIn {
    let roll = rng.below(20);
    // the default (`4`/`6`) or component (`5`/`7`) constructor of an allowed kind
    let bend_kind = |rng: &mut Rng, component: bool| -> u8 {
        let bend = match bending {
            Bending::Bend => true,
            Bending::Both => rng.chance(0.5),
            Bending::None | Bending::Unbend => false,
        };
        4 + u8::from(component) + if bend { 2 } else { 0 }
    };
    let kind = match roll {
        0..=9 => 1,
        10..=13 => 2,
        14..=16 => 3,
        17 if bending != Bending::None => bend_kind(rng, false),
        18 | 19 if bending != Bending::None => bend_kind(rng, true),
        _ => 0,
    };
    let framed = kind == 2 || kind == 5 || kind == 7;
    MapIn {
        kind,
        t: (kind == 1).then(|| gen_transform(rng)),
        tw: (kind == 2).then(|| gen_twist(rng)),
        bs: (kind == 3).then(|| gen_bspline(rng)),
        ub: (kind == 5 || kind == 7).then(|| gen_unbend(rng)),
        orient: (!framed && rng.chance(0.4)).then(|| gen_transform(rng)),
    }
}

fn gen_vol(rng: &mut Rng, bending: Bending) -> Vol {
    let n = 1 + rng.below(3) as usize;
    (0..n).map(|_| gen_map(rng, bending)).collect()
}

fn gen_box(rng: &mut Rng) -> Bx {
    match rng.below(10) {
        0 => [[-INF; 3], [INF; 3]],
        1 => {
            // partially unbounded
            let mut a = coord(rng, 3.0);
            let mut e = coord(rng, 3.0);
            let k = rng.below(3) as usize;
            if let (Some(ak), Some(ek)) = (a.get_mut(k), e.get_mut(k)) {
                *ak = -INF;
                *ek = INF;
            }
            [a, e]
        }
        2 => {
            // a point-sized box
            let p = coord(rng, 3.0);
            [p, p]
        }
        3 => [[0.0; 3], [1.0; 3]],
        4 => {
            // tiny box
            let p = coord(rng, 3.0);
            [p, p.map(|c| c + 1e-7)]
        }
        _ => [coord(rng, 3.0), coord(rng, 3.0)],
    }
}

fn gen_pb(rng: &mut Rng) -> PbIn {
    let basis = match rng.below(5) {
        0 | 1 => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        2 => {
            let a = rng.uniform(-3.2, 3.2);
            let (s, c) = a.sin_cos();
            [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
        }
        3 => [[0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]],
        _ => [coord(rng, 2.0), coord(rng, 2.0), coord(rng, 2.0)],
    };
    let bx = gen_box(rng);
    PbIn {
        basis,
        ivl: [
            [bx[0][0], bx[1][0]],
            [bx[0][1], bx[1][1]],
            [bx[0][2], bx[1][2]],
        ],
    }
}

fn opt<T>(rng: &mut Rng, p: f64, f: impl FnOnce(&mut Rng) -> T) -> Option<T> {
    if rng.chance(p) {
        Some(f(rng))
    } else {
        None
    }
}

/// The natural parameter domain `[knot[deg], knot[hi + 1]]` per direction of a
/// B-spline head map.
fn head_domain(vol: &Vol) -> Option<[[f64; 2]; 3]> {
    let r = vol.first()?.bs.as_ref()?;
    let mut d = [[0.0; 2]; 3];
    for (k, slot) in d.iter_mut().enumerate() {
        let knots = r.knots.get(k)?;
        *slot = [
            *knots.get(*r.deg.get(k)?)?,
            *knots.get(*r.hi.get(k)? + 1)?,
        ];
    }
    Some(d)
}

/// A parameter point: inside a B-spline head's domain more often than not.
fn gen_param(rng: &mut Rng, vol: &Vol, scale: f64) -> [f64; 3] {
    match head_domain(vol) {
        Some(d) if rng.chance(0.7) => d.map(|[a, e]| a + (e - a) * rng.unit()),
        _ => coord(rng, scale),
    }
}

/// A parameter box: inside a B-spline head's domain more often than not.
fn gen_param_box(rng: &mut Rng, vol: &Vol) -> Bx {
    match head_domain(vol) {
        Some(d) if rng.chance(0.7) => {
            let a = d.map(|[a, e]| a + (e - a) * rng.unit() * 0.5);
            let e = d.map(|[a, e]| e - (e - a) * rng.unit() * 0.5);
            [a, e]
        }
        _ => gen_box(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let op = rng.below(20);
    let bending = match rng.below(20) {
        0..=4 => Bending::Unbend,
        5..=9 => Bending::Bend,
        10 | 11 => Bending::Both,
        _ => Bending::None,
    };
    let vol = gen_vol(rng, bending);
    let p = gen_param(rng, &vol, 8.0);
    let comp = rng.chance(0.75);
    match op {
        0 => Inputs::PtInParam { vol, p, comp },
        1 => {
            let e = gen_param(rng, &vol, 8.0);
            Inputs::LineInInSpace {
                vol,
                a: p,
                b: e,
                comp,
            }
        }
        2 => {
            let bx = gen_param_box(rng, &vol);
            Inputs::BoxInParam { vol, bx, comp }
        }
        3 => Inputs::PboxInInSpace {
            vol,
            pb: gen_pb(rng),
            comp,
        },
        4 | 5 => Inputs::EvalBbox {
            bx: gen_param_box(rng, &vol),
            vol,
            ppb: opt(rng, 0.7, gen_pb),
            want: [
                rng.chance(0.6),
                rng.chance(0.6),
                rng.chance(0.4),
                rng.chance(0.4),
            ],
            comp,
        },
        6 => Inputs::EvalPoint { vol, p, comp },
        7 => Inputs::Eval1st {
            vol,
            p,
            left: [rng.chance(0.5), rng.chance(0.5), rng.chance(0.5)],
            nz: rng.chance(0.5),
            comp,
        },
        8 => Inputs::InvEval {
            p,
            guess: opt(rng, 0.5, |r| gen_param(r, &vol, 3.0)),
            dom: if rng.chance(0.4) {
                Some(gen_param_box(rng, &vol))
            } else {
                None
            },
            vol,
            comp,
        },
        9 => Inputs::InvMap {
            vol,
            p,
            guess: opt(rng, 0.5, |r| coord(r, 3.0)),
            dom: opt(rng, 0.4, gen_pb),
            comp,
        },
        10 => Inputs::Global {
            p,
            dom: if rng.chance(0.4) {
                Some(gen_param_box(rng, &vol))
            } else {
                None
            },
            vol,
            comp,
        },
        11 => Inputs::HasDisc {
            vol,
            want: rng.chance(0.7),
            geometric: rng.chance(0.5),
        },
        12 | 13 => Inputs::BboxSimple {
            bx: gen_param_box(rng, &vol),
            vol,
            ppb: opt(rng, 0.7, gen_pb),
            want: [rng.chance(0.6), rng.chance(0.6)],
        },
        14 => Inputs::PboxSimple {
            vol,
            pb: gen_pb(rng),
            want: [rng.chance(0.6), rng.chance(0.6)],
        },
        15 => Inputs::InvEvalSimple {
            p,
            guess: opt(rng, 0.5, |r| gen_param(r, &vol, 3.0)),
            dom: if rng.chance(0.4) {
                Some(gen_param_box(rng, &vol))
            } else {
                None
            },
            vol,
        },
        16 => Inputs::GlobalSimple {
            p,
            dom: if rng.chance(0.4) {
                Some(gen_param_box(rng, &vol))
            } else {
                None
            },
            vol,
        },
        17 => Inputs::LocalSimple {
            p,
            g: gen_param(rng, &vol, 3.0),
            dom: if rng.chance(0.4) {
                Some(gen_param_box(rng, &vol))
            } else {
                None
            },
            vol,
        },
        18 => Inputs::MapPbox {
            vol,
            pb: gen_pb(rng),
            want: [rng.chance(0.6), rng.chance(0.6)],
            comp,
            trim: rng.chance(0.5),
        },
        _ => gen_read(rng),
    }
}

/// The ASCII text of a transform volume (`matrix rows`, then base data).
fn text_transform(rng: &mut Rng, depth: usize, out: &mut String) {
    let t = gen_transform(rng);
    let [o, x, y, z] = t;
    for (row, w) in [(x, 0.0), (y, 0.0), (z, 0.0), (o, 1.0)] {
        let [a, b, c] = row;
        let _ = writeln!(out, "{a} {b} {c} {w}");
    }
    text_base(rng, depth, out);
}

fn text_base(rng: &mut Rng, depth: usize, out: &mut String) {
    let next = depth < 3 && rng.chance(0.4);
    if next {
        out.push_str("1\n");
        if rng.chance(0.2) {
            // a twist next map
            out.push_str("Object Type 22011\n");
            let _ = write!(out, "{}\n{}\n", rng.uniform(1.0, 20.0), 1e-3);
            text_base(rng, depth + 1, out);
        } else if rng.chance(0.15) {
            // an unbend next map
            out.push_str("Object Type 22010\n");
            let _ = write!(out, "{}\n{}\n", rng.uniform(0.0, 10.0), 1e-2);
            text_base(rng, depth + 1, out);
        } else if rng.chance(0.18) {
            // a bend next map
            out.push_str("Object Type 22006\n");
            let _ = write!(out, "{}\n{}\n", rng.uniform(0.0, 10.0), 1e-2);
            text_base(rng, depth + 1, out);
        } else if rng.chance(0.1) {
            // a B-spline next map: SM_ERR, nothing read
            out.push_str("Object Type 22004\n");
        } else {
            out.push_str("Object Type 22008\n");
            text_transform(rng, depth + 1, out);
        }
    } else {
        out.push_str("0\n");
    }
    if depth < 3 && rng.chance(0.4) {
        out.push_str("1\nObject Type 22008\n");
        text_transform(rng, depth + 1, out);
    } else {
        out.push_str("0\n");
    }
}

fn gen_read(rng: &mut Rng) -> Inputs {
    let (ty, mut data) = match rng.below(12) {
        0 => (ffi::TWIST_VOLUME_TYPE, {
            let mut s = format!("{}\n{}\n", rng.uniform(-10.0, 10.0), rng.uniform(0.0, 1.0));
            text_base(rng, 0, &mut s);
            s
        }),
        1 => (ffi::B_SPLINE_VOLUME_TYPE, String::new()),
        2 => (12345, String::from("0\n0\n")),
        3 => (ffi::UNBEND_VOLUME_TYPE, {
            let mut s = format!("{}\n{}\n", rng.uniform(0.0, 10.0), rng.uniform(0.0, 0.1));
            text_base(rng, 0, &mut s);
            s
        }),
        4 | 5 => (ffi::BEND_VOLUME_TYPE, {
            let mut s = format!("{}\n{}\n", rng.uniform(-1.0, 10.0), rng.uniform(0.0, 0.1));
            text_base(rng, 0, &mut s);
            s
        }),
        _ => (ffi::TRANSFORM_TYPE, {
            let mut s = String::new();
            text_transform(rng, 0, &mut s);
            s
        }),
    };
    if rng.chance(0.2) {
        // truncate at a line end
        let lines: Vec<&str> = data.split_inclusive('\n').collect();
        let keep = rng.below(lines.len() as u64 + 1) as usize;
        data = lines.iter().take(keep).copied().collect();
    }
    Inputs::ReadDb { ty, data }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.volume_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input, data)) = cpp_input(&i) else {
                continue;
            };
            if ffi::check(op, &input, &data).is_ok() {
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
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
