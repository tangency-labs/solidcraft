//! Runner for unit `surface.volume_p2`: the second half of the `SmVolume` base class
//! (compound domain checks of lines and boxes, bounding boxes through the orient and
//! next maps, compound point evaluation, the inverse point solvers,
//! `HasDiscontinuities` and the polymorphic `ReadFromDB`).
//!
//! Ported from upstream `source/SMLib/src/SmVolume.cpp`; checked against its golden (on
//! chains of plain `SmVolume`, `SmTransform`, `SmTwistVolume`, `SmBSplineVolume`, default
//! and component-constructed `SmUnbendVolume` and `SmBendVolume` maps, each but the twist
//! and the component unbend and bend optionally oriented by an `SmTransform`). Rust:
//! `solidcore::surface::volume`.
//!
//! A case is a volume (`vol`: maps `{kind, ...}`, kind `0` plain, `1` transform `t =
//! [origin, X, Y, Z]`, `2` twist `tw`, `3` B-spline `bs`, `4` default unbend, `5`
//! component unbend `ub`, `6` default bend, `7` component bend `ub`; optional
//! `orient`) and an `op` with its parameters, or a
//! `read_db` of ASCII text. The Rust side
//! produces a flat record; the runner reports its first
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
use solidcore::surface::volume::p1::{
    BEND_VOLUME_TYPE, B_SPLINE_VOLUME_TYPE, TRANSFORM_TYPE, TWIST_VOLUME_TYPE, UNBEND_VOLUME_TYPE,
    VOLUME_TYPE,
};
use solidcore::surface::volume::{Discontinuities3d, InvPoints, Volume, VolumeKind};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;

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

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
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

// ---- input contract -----------------------------------------------------------------
//
// A port of the input contract of the C++ reference wrapper: finite values of at most
// `MAX_COORD`, B-spline records within `MAX_CP` / `MAX_DEGREE` with non-decreasing knots,
// no call path that hands a transform's `EvaluateBoundingBoxSimple` a null parameter pseudo
// box or a twist's an unbounded box without a box output, no call path that may hand an
// unbend's `EvaluateBoundingBoxSimple` a bounded box with a side shorter than `MIN_SIDE`,
// and `read_db` text in a strict token grammar without the reader's null or uninitialised
// cases. The generator redraws until it holds.

/// Most maps in a volume chain.
const MAX_MAPS: usize = 6;
/// Most control points per direction of a B-spline map.
const MAX_CP: usize = 6;
/// Highest degree per direction of a B-spline map.
const MAX_DEGREE: usize = 4;
/// Largest value magnitude.
const MAX_COORD: f64 = 1e8;
/// Most nested volume reads in a `read_db` text (the head included), as
/// `solidcore::topology::database_io::MAX_READ_NESTING`.
const MAX_READ_NESTING: usize = 32;
/// Longest database text.
const MAX_DATA: usize = 1 << 16;
/// Shortest side of a bounded box handed to an unbend's `EvaluateBoundingBoxSimple`
/// (`SmLine::CreateLineSegment` fails on a zero-length box edge and upstream then uses a
/// null or deleted line).
const MIN_SIDE: f64 = 1e-9;
/// `SM_INFINITE_PARAMETER` and its square root, the unbounded interval markers.
const INFINITE_MARKERS: [f64; 4] = [1_234_567.0, -1_234_567.0, 1111.111, -1111.111];

fn fin(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

fn fins<'a>(vs: impl IntoIterator<Item = &'a f64>) -> bool {
    vs.into_iter().all(|&v| fin(v))
}

fn tf_ok(t: &Tf) -> bool {
    fins(t.iter().flatten())
}

fn pb_ok(pb: &PbIn) -> bool {
    fins(pb.basis.iter().flatten()) && fins(pb.ivl.iter().flatten())
}

/// Whether a box is finite, and whether it is bounded with a side shorter than
/// [`MIN_SIDE`]; `None` when it is not finite. Also whether it has a bounded end
/// (`AnyBounds`).
fn box_info(bx: &Bx) -> Option<(bool, bool)> {
    let [lo, hi] = bx;
    if !fins(lo.iter().chain(hi)) {
        return None;
    }
    let inf = |x: &f64| INFINITE_MARKERS.contains(x);
    let any_bounds = lo.iter().zip(hi).any(|(a, b)| !(inf(a) && inf(b)));
    let bounded = !lo.iter().chain(hi).any(inf);
    let flat = lo.iter().zip(hi).any(|(a, b)| (b - a).abs() < MIN_SIDE);
    Some((any_bounds, bounded && flat))
}

/// The class of one map of a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Base,
    Transform,
    Twist,
    BSpline,
    Unbend,
    Bend,
}

/// One map of a chain: its class and whether it has an orient map.
#[derive(Debug, Clone, Copy)]
struct Map {
    kind: Kind,
    orient: bool,
}

/// Whether a B-spline record is within the size bounds with `hi + deg + 2`
/// non-decreasing knots per direction and a full net, all finite.
fn bspline_ok(r: &BsIn) -> bool {
    r.hi.iter().all(|&h| h < MAX_CP)
        && r.deg.iter().all(|&d| d <= MAX_DEGREE)
        && r.knots.iter().zip(r.hi.iter().zip(r.deg)).all(|(k, (h, d))| {
            k.len() == h + d + 2 && fins(k) && k.windows(2).all(|w| w.first() <= w.last())
        })
        && r.cpts.len() == r.hi.iter().map(|h| h + 1).product::<usize>()
        && fins(r.cpts.iter().flatten())
}

/// The chain of a volume that keeps the contract: 1 to [`MAX_MAPS`] maps of a known kind
/// with their data, finite orient maps.
fn chain_of(v: &Vol) -> Option<Vec<Map>> {
    if v.is_empty() || v.len() > MAX_MAPS {
        return None;
    }
    v.iter()
        .map(|m| {
            let kind = match m.kind {
                0 => Kind::Base,
                1 => m.t.as_ref().filter(|t| tf_ok(t)).map(|_| Kind::Transform)?,
                2 => {
                    let t = m.tw.as_ref()?;
                    let ok = fins(t.o.iter().chain(&t.axis).chain(&t.mid)) && fin(t.period) && fin(t.min);
                    ok.then_some(Kind::Twist)?
                }
                3 => m.bs.as_ref().filter(|r| bspline_ok(r)).map(|_| Kind::BSpline)?,
                4 | 6 => {
                    if m.kind == 4 {
                        Kind::Unbend
                    } else {
                        Kind::Bend
                    }
                }
                5 | 7 => {
                    let u = m.ub.as_ref()?;
                    let ok = fins(u.c.iter().chain(&u.axis).chain(&u.mid)) && fin(u.d) && fin(u.tol);
                    if !ok {
                        return None;
                    }
                    if m.kind == 5 {
                        Kind::Unbend
                    } else {
                        Kind::Bend
                    }
                }
                _ => return None,
            };
            // a twist's and a component unbend's or bend's frame is their orient map
            let orient = if matches!(m.kind, 2 | 5 | 7) {
                true
            } else {
                if !m.orient.as_ref().is_none_or(tf_ok) {
                    return None;
                }
                m.orient.is_some()
            };
            Some(Map { kind, orient })
        })
        .collect()
}

// undefined call paths

/// Whether the domain check of a line or box (`IsLineInParamDomain` with
/// compounding, as `IsBoxInParamDomain` and `IsPseudoBoxInInSpaceDomain` run it) on
/// map `i` passes: `Some(false)` when a later map is a plain `SmVolume` (its
/// `IsCurveInParamDomainSimple` is `FALSE`), `None` when a B-spline map decides it
/// from data.
fn domain_check(chain: &[Map], i: usize) -> Option<bool> {
    if chain.get(i).is_none_or(|m| m.kind == Kind::BSpline) {
        return None;
    }
    for m in chain.iter().skip(i + 1) {
        match m.kind {
            Kind::Base => return Some(false),
            Kind::BSpline => return None,
            _ => {}
        }
    }
    Some(true)
}

/// Which box outputs a call requests.
#[derive(Debug, Clone, Copy)]
struct Wants {
    /// The axis-aligned box.
    bx: bool,
    /// The pseudo box.
    pb: bool,
}

impl Wants {
    fn any(self) -> bool {
        self.bx || self.pb
    }
}

/// Whether `EvaluateBoundingBox` on map `i` with the out-space outputs `out` and
/// the in-space outputs `inside` may hand a transform's `EvaluateBoundingBoxSimple`
/// a null parameter pseudo box (data-dependent domain checks count as passing).
fn ebb_undefined(
    chain: &[Map],
    i: usize,
    has_ppb: bool,
    (out, inside): (Wants, Wants),
    comp: bool,
) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if (!out.any() && !inside.any()) || domain_check(chain, i) == Some(false) {
        return false;
    }
    if inside.any() && m.orient && !has_ppb {
        return true;
    }
    if out.any() {
        if m.kind == Kind::Transform && !has_ppb {
            return true;
        }
        if comp && i + 1 < chain.len() {
            let next_out = Wants {
                bx: out.bx,
                pb: true,
            };
            return mpb_undefined(chain, i + 1, next_out, true, false);
        }
    }
    false
}

/// Whether `MapPseudoBox` on map `i` with the outputs `outs` may reach a
/// transform's `EvaluateBoundingBoxSimple` with a null parameter pseudo box.
fn mpb_undefined(chain: &[Map], i: usize, outs: Wants, comp: bool, trim: bool) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if !trim && domain_check(chain, i) == Some(false) {
        return false;
    }
    let next = comp && i + 1 < chain.len();
    let has_this = outs.pb || next;
    let here = if m.orient {
        m.kind == Kind::Transform && !has_this
    } else {
        trim && m.kind == Kind::Transform && (outs.bx || has_this)
    };
    let next_outs = Wants {
        bx: outs.bx,
        pb: true,
    };
    here || (next && mpb_undefined(chain, i + 1, next_outs, comp, trim))
}

/// Whether `InvMapPoint` on map `i` may reach an undefined box mapping.
fn inv_map_undefined(chain: &[Map], i: usize, has_dom: bool, comp: bool) -> bool {
    if !comp || i + 1 >= chain.len() {
        return false;
    }
    (has_dom
        && mpb_undefined(
            chain,
            i,
            Wants {
                bx: false,
                pb: true,
            },
            false,
            true,
        ))
        || inv_map_undefined(chain, i + 1, has_dom, true)
}

/// Whether `InvEvaluatePoint` / `GlobalPointSolve` on map `0` may reach an undefined
/// box mapping.
fn inv_eval_undefined(chain: &[Map], has_dom: bool, comp: bool) -> bool {
    if !comp || chain.len() < 2 {
        return false;
    }
    let out = Wants { bx: true, pb: true };
    let inside = Wants {
        bx: false,
        pb: false,
    };
    (has_dom && ebb_undefined(chain, 0, false, (out, inside), false))
        || inv_map_undefined(chain, 1, has_dom, true)
}

/// Whether `EvaluateBoundingBox` on map `i` with the out-space outputs `out` and
/// the in-space outputs `inside` may reach an unbend's `EvaluateBoundingBoxSimple`
/// with a too-flat box: on map `i` itself with the caller's box (`flat`), on a later
/// map with a computed box (rejected whatever its size).
fn ebb_unbend(
    chain: &[Map],
    i: usize,
    (out, inside): (Wants, Wants),
    comp: bool,
    flat: bool,
) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if (!out.any() && !inside.any()) || domain_check(chain, i) == Some(false) {
        return false;
    }
    if out.any() {
        if m.kind.is_unbend() && flat {
            return true;
        }
        if comp && i + 1 < chain.len() {
            let next_out = Wants {
                bx: out.bx,
                pb: true,
            };
            return mpb_unbend(chain, i + 1, next_out, true, false);
        }
    }
    false
}

/// Whether `MapPseudoBox` on map `i` with the outputs `outs` may reach an unbend's
/// `EvaluateBoundingBoxSimple`. Its box there is computed from the in-space pseudo
/// box (sampled through the inverse orient map, or its axis-aligned extent when
/// trimming), so any such call is rejected.
fn mpb_unbend(chain: &[Map], i: usize, outs: Wants, comp: bool, trim: bool) -> bool {
    let Some(m) = chain.get(i) else {
        return false;
    };
    if !trim && domain_check(chain, i) == Some(false) {
        return false;
    }
    let next = comp && i + 1 < chain.len();
    let has_this = outs.pb || next;
    let here = m.kind.is_unbend() && (m.orient || (trim && (outs.bx || has_this)));
    let next_outs = Wants {
        bx: outs.bx,
        pb: true,
    };
    here || (next && mpb_unbend(chain, i + 1, next_outs, comp, trim))
}

/// Whether `InvMapPoint` on map `i` may reach an unbend's `EvaluateBoundingBoxSimple`.
fn inv_map_unbend(chain: &[Map], i: usize, has_dom: bool, comp: bool) -> bool {
    if !comp || i + 1 >= chain.len() {
        return false;
    }
    (has_dom
        && mpb_unbend(
            chain,
            i,
            Wants {
                bx: false,
                pb: true,
            },
            false,
            true,
        ))
        || inv_map_unbend(chain, i + 1, has_dom, true)
}

/// Whether `InvEvaluatePoint` / `GlobalPointSolve` on map `0` may reach an unbend's
/// `EvaluateBoundingBoxSimple` (`flat`: the caller's domain box is too flat).
fn inv_eval_unbend(chain: &[Map], has_dom: bool, comp: bool, flat: bool) -> bool {
    if !comp || chain.len() < 2 {
        return false;
    }
    let out = Wants { bx: true, pb: true };
    let inside = Wants {
        bx: false,
        pb: false,
    };
    (has_dom && ebb_unbend(chain, 0, (out, inside), false, flat))
        || inv_map_unbend(chain, 1, has_dom, true)
}

// database text

/// A strict tokenizer over the ASCII database text, modelling the `istream`
/// extractions and `GoToNextLine` of the readers on text it accepts.
struct Text<'a> {
    b: &'a [u8],
    at: usize,
    failed: bool,
}

/// What an extraction found.
enum Tok<'a> {
    /// The stream is at its end (or failed before): the value stays unchanged.
    End,
    /// A token.
    Word(&'a str),
}

impl<'a> Text<'a> {
    fn token(&mut self) -> Result<Tok<'a>, ()> {
        if self.failed {
            return Ok(Tok::End);
        }
        while self.b.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
        let start = self.at;
        while self
            .b
            .get(self.at)
            .is_some_and(|c| !c.is_ascii_whitespace())
        {
            self.at += 1;
        }
        if start == self.at {
            self.failed = true;
            return Ok(Tok::End);
        }
        let w = self
            .b
            .get(start..self.at)
            .and_then(|s| core::str::from_utf8(s).ok())
            .ok_or(())?;
        if w.len() > 200 {
            return Err(());
        }
        Ok(Tok::Word(w))
    }

    fn next_line(&mut self) {
        if self.failed {
            return;
        }
        while let Some(&c) = self.b.get(self.at) {
            self.at += 1;
            if c == b'\n' {
                break;
            }
        }
    }

    fn integer(&mut self) -> Result<Option<i64>, ()> {
        match self.token()? {
            Tok::End => Ok(None),
            Tok::Word(w) => {
                let digits = w.strip_prefix(['+', '-']).unwrap_or(w);
                if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(());
                }
                let v: i64 = w
                    .parse()
                    .map_err(|_| ())?;
                if v.unsigned_abs() > i32::MAX as u64 {
                    return Err(());
                }
                Ok(Some(v))
            }
        }
    }

    fn double(&mut self) -> Result<(), ()> {
        if let Tok::Word(w) = self.token()? {
            let body = w.strip_prefix(['+', '-']).unwrap_or(w);
            let (mantissa, exp) = match body.split_once(['e', 'E']) {
                Some((m, e)) => (m, Some(e)),
                None => (body, None),
            };
            let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
            let digits = |s: &str| s.bytes().all(|c| c.is_ascii_digit());
            let exp_ok = exp.is_none_or(|e| {
                let e = e.strip_prefix(['+', '-']).unwrap_or(e);
                !e.is_empty() && digits(e)
            });
            if (int.is_empty() && frac.is_empty()) || !digits(int) || !digits(frac) || !exp_ok {
                return Err(());
            }
            let v: f64 = w.parse().map_err(|_| ())?;
            if !v.is_finite() || v.abs() > 1e300 {
                return Err(());
            }
        }
        Ok(())
    }

    /// `>> word >> word >> long; GoToNextLine` after a "has map" flag.
    fn map_type(&mut self) -> Result<i64, ()> {
        self.token()?;
        self.token()?;
        let t = self
            .integer()?
            .ok_or(())?;
        self.next_line();
        Ok(t)
    }

    /// The derived data and base data of a volume of type `t`; `Ok(false)` when the
    /// read fails without reading anything.
    fn volume(&mut self, t: i64, depth: usize) -> Result<bool, ()> {
        // the Rust reader's documented bound (`MAX_READ_NESTING`, 32 nested reads):
        // a deeper stream is a deliberate deviation (Rust fails, C++ recurses on)
        if depth >= MAX_READ_NESTING {
            return Err(());
        }
        let (lines, per_line) = match t {
            TRANSFORM_TYPE => (4, 4),
            TWIST_VOLUME_TYPE | UNBEND_VOLUME_TYPE | BEND_VOLUME_TYPE => (2, 1),
            VOLUME_TYPE => return Err(()),
            _ => return Ok(false),
        };
        for _ in 0..lines {
            for _ in 0..per_line {
                self.double()?;
            }
            self.next_line();
        }
        // base data: next map
        if self.integer()?.is_some_and(|f| f != 0) {
            self.next_line();
            let nt = self.map_type()?;
            self.volume(nt, depth + 1)?;
        } else {
            self.next_line();
        }
        // orient map
        if self.integer()?.is_some_and(|f| f != 0) {
            self.next_line();
            let ot = self.map_type()?;
            if ot != TRANSFORM_TYPE {
                return Err(());
            }
            self.volume(ot, depth + 1)?;
        } else {
            self.next_line();
        }
        Ok(true)
    }
}


fn check_text(t: i64, data: &[u8]) -> Result<(), ()> {
    if data.len() > MAX_DATA {
        return Err(());
    }
    if data.split(|&c| c == b'\n').any(|l| l.len() > 200) {
        return Err(());
    }
    let mut text = Text {
        b: data,
        at: 0,
        failed: false,
    };
    text.volume(t, 0).map(|_| ())
}

impl Kind {
    fn is_unbend(self) -> bool {
        self == Kind::Unbend
    }
}

/// Whether a case keeps the input contract (see the section header).
#[allow(clippy::too_many_lines)]
fn keeps_contract(i: &Inputs) -> bool {
    let p3 = |p: &[f64; 3]| fins(p);
    let opt3 = |p: &Option<[f64; 3]>| p.as_ref().is_none_or(fins);
    if let Inputs::ReadDb { ty, data } = i {
        return fin(*ty as f64) && check_text(*ty, data.as_bytes()).is_ok();
    }
    let Some(chain) = i.vol().and_then(chain_of) else {
        return false;
    };
    let undefined = match i {
        Inputs::PtInParam { p, .. } | Inputs::EvalPoint { p, .. } | Inputs::Eval1st { p, .. } => {
            return p3(p);
        }
        Inputs::LineInInSpace { a, b, .. } => return p3(a) && p3(b),
        Inputs::BoxInParam { bx, .. } => return box_info(bx).is_some(),
        Inputs::PboxInInSpace { pb, .. } | Inputs::PboxSimple { pb, .. } => return pb_ok(pb),
        Inputs::HasDisc { .. } => return true,
        Inputs::EvalBbox {
            bx,
            ppb,
            want,
            comp,
            ..
        } => {
            let Some((_, flat)) = box_info(bx) else {
                return false;
            };
            if !ppb.as_ref().is_none_or(pb_ok) {
                return false;
            }
            let [ob, opb, ib, ipb] = *want;
            let out = Wants { bx: ob, pb: opb };
            let inside = Wants { bx: ib, pb: ipb };
            ebb_undefined(&chain, 0, ppb.is_some(), (out, inside), *comp)
                || ebb_unbend(&chain, 0, (out, inside), *comp, flat)
        }
        Inputs::MapPbox {
            pb,
            want,
            comp,
            trim,
            ..
        } => {
            if !pb_ok(pb) {
                return false;
            }
            let outs = Wants {
                bx: want[0],
                pb: want[1],
            };
            mpb_undefined(&chain, 0, outs, *comp, *trim)
                || mpb_unbend(&chain, 0, outs, *comp, *trim)
        }
        Inputs::InvEval {
            p,
            guess,
            dom,
            comp,
            ..
        } => {
            let flat = match dom {
                Some(d) => match box_info(d) {
                    Some((_, flat)) => flat,
                    None => return false,
                },
                None => false,
            };
            if !(p3(p) && opt3(guess)) {
                return false;
            }
            inv_eval_undefined(&chain, dom.is_some(), *comp)
                || inv_eval_unbend(&chain, dom.is_some(), *comp, flat)
        }
        Inputs::InvMap {
            p,
            guess,
            dom,
            comp,
            ..
        } => {
            if !(p3(p) && opt3(guess) && dom.as_ref().is_none_or(pb_ok)) {
                return false;
            }
            inv_map_undefined(&chain, 0, dom.is_some(), *comp)
                || inv_map_unbend(&chain, 0, dom.is_some(), *comp)
        }
        Inputs::Global { p, dom, comp, .. } => {
            let flat = match dom {
                Some(d) => match box_info(d) {
                    Some((_, flat)) => flat,
                    None => return false,
                },
                None => false,
            };
            if !p3(p) {
                return false;
            }
            inv_eval_undefined(&chain, dom.is_some(), *comp)
                || inv_eval_unbend(&chain, dom.is_some(), *comp, flat)
        }
        Inputs::BboxSimple { bx, ppb, want, .. } => {
            let Some((any_bounds, flat)) = box_info(bx) else {
                return false;
            };
            if !ppb.as_ref().is_none_or(pb_ok) {
                return false;
            }
            let [want_box, want_pb] = *want;
            let kind = chain.first().map(|m| m.kind);
            (want_box || want_pb)
                && ((kind == Some(Kind::Transform) && ppb.is_none())
                    || (kind == Some(Kind::Twist) && !want_box && !any_bounds)
                    || (kind == Some(Kind::Unbend) && flat))
        }
        Inputs::InvEvalSimple { p, guess, dom, .. } => {
            return p3(p) && opt3(guess) && dom.as_ref().is_none_or(|d| box_info(d).is_some());
        }
        Inputs::GlobalSimple { p, dom, .. } => {
            return p3(p) && dom.as_ref().is_none_or(|d| box_info(d).is_some());
        }
        Inputs::LocalSimple { p, g, dom, .. } => {
            return p3(p) && p3(g) && dom.as_ref().is_none_or(|d| box_info(d).is_some());
        }
        Inputs::ReadDb { .. } => return false,
    };
    !undefined
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
        0 => (TWIST_VOLUME_TYPE, {
            let mut s = format!("{}\n{}\n", rng.uniform(-10.0, 10.0), rng.uniform(0.0, 1.0));
            text_base(rng, 0, &mut s);
            s
        }),
        1 => (B_SPLINE_VOLUME_TYPE, String::new()),
        2 => (12345, String::from("0\n0\n")),
        3 => (UNBEND_VOLUME_TYPE, {
            let mut s = format!("{}\n{}\n", rng.uniform(0.0, 10.0), rng.uniform(0.0, 0.1));
            text_base(rng, 0, &mut s);
            s
        }),
        4 | 5 => (BEND_VOLUME_TYPE, {
            let mut s = format!("{}\n{}\n", rng.uniform(-1.0, 10.0), rng.uniform(0.0, 0.1));
            text_base(rng, 0, &mut s);
            s
        }),
        _ => (TRANSFORM_TYPE, {
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
            if keeps_contract(&i) {
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
