//! Runner for unit `topology.face_p1`. C++: `solidcore_ffi::units::topology_face_p1`.
//! Rust: `solidcore::topology::face::p1`.
//!
//! Every op runs on a B-rep of unit `topology.vertexuse` (upstream `.smb` files, faces of
//! B-spline surfaces or real `SmPlane`s, wire edges, the `my_test_topology` network): the
//! C++ side builds a real `SmBrep`, preserves exactly exported analytic surfaces and
//! gives other surfaces and edge curves the plain B-spline copies the Rust import
//! rebuilds (keeping the owners), then runs the
//! `SmFace` member on the selected face; the Rust side imports the graph that unit's shim
//! exports (with each edge curve owned by its edge, as upstream) and runs the port. Face
//! and loop selectors are taken modulo their counts. Both sides produce one flat record
//! (`out`, laid out as in the ffi module): statuses, domains, gaps, curve samples and
//! the orientation / UV trim curve state after the mutating ops.
//!
//! `MoveSeam` / `SplitAtSeam` take `HasSeamProblem`'s classifications and enums
//! (`topology.face_p4`, unported): the C++ side computes them before the call, the Rust
//! side asks the shim for them (`Probe`) and rebuilds classifications of the same shape
//! (target-loop flag, interval count, middle point classes), which is all the port reads
//! before its first unported callee. With `props` both sides also pass face properties
//! with the given seam verdicts and closed flags.
//!
//! Deferred: the seam calls behind the C++ crash guard (below) and the cone
//! `TrimWithDomain` callbacks of `ShrinkGeometry` (`surface.surface_p2`).
//!
//! **C++ crash guard.** Upstream `SplitAtSeam`'s U-then-V branch recurses on the wrong
//! face and crashes (`plan/upstream-issues.md` UB-137). The shim skips (`-3`) every seam
//! call that can reach it: a `SplitAtSeam` merging in both directions on a face periodic
//! in either (by the face properties' closed flags or by the surface itself), and every
//! `MoveSeam` with split past its early return (it recomputes the seam problem before
//! splitting). The Rust side defers all of these before anything is compared. The Rust side
//! reports those cases as deferred to `cpp-crash-guard:UB-137`; they never become
//! comparable.
//!
//! **Not compared.** Edge curves and UV trim curves are still imported as plain
//! B-splines (except lines), so conic curve paths (`SwapUV`'s conic mirrors) are not
//! compared with the C++. Exactly exported analytic surface branches are retained.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::heal::face_props::{FaceProps, LoopObject, Tri};
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p1 as fp1;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{Brep, EdgeId, FaceId, LoopId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_face_p1::{self as ffi, Op};
use solidcore_ffi::units::topology_vertexuse as fixture;

use super::topology_vertexuse::{self as shared, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;
/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// The `.smb` files the systematic cases run on (those of unit `topology.vertexuse`).
const FILES: [&str; 15] = [
    "Mouse.smb",
    "Endcap.smb",
    "box_with_large_fillet.smb",
    "cone.smb",
    "disk.smb",
    "ellips_ts2.smb",
    "plane_circ.smb",
    "plane_square.smb",
    "sphere.smb",
    "surf_extrude.smb",
    "surf_rev.smb",
    "surf_sphere150.smb",
    "torus.smb",
    "toruswvy.smb",
    "vase_rev.smb",
];

// ---- inputs -----------------------------------------------------------------------------

/// Face properties for the seam ops (`SmSurfParamType` codes and `SmBoolean`s).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PropsIn {
    crossed: u8,
    missing: u8,
    near_miss: u8,
    closed_u: u8,
    closed_v: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    UvDomain {
        limit: bool,
        want_max: bool,
        want_2nd: bool,
    },
    Oriented {
        param: bool,
        fill: bool,
        /// `-1`: the whole face.
        #[serde(rename = "loop")]
        loop_: i64,
    },
    Rect,
    Flip,
    SetSurface {
        delete: bool,
    },
    LoopRoundTrip {
        #[serde(rename = "loop")]
        loop_: i64,
    },
    RemoveUv,
    SwapUv,
    Shrink,
    MoveSeam {
        #[serde(default)]
        props: Option<PropsIn>,
        center: bool,
        split: bool,
    },
    SplitAtSeam {
        #[serde(default)]
        props: Option<PropsIn>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    face: i64,
    #[serde(flatten)]
    op: OpIn,
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

/// The B-rep block (`validate_only` and `init_gaps` off).
fn brep_block(b: &BrepIn) -> Result<Vec<f64>, HarnessError> {
    let mut out = Vec::new();
    match b {
        BrepIn::File { rebuild, .. } => out.extend([0.0, flag(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            out.push(1.0);
            shared::push_curve(&mut out, curve)?;
            out.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            out.push(2.0);
            shared::push_surface(&mut out, surface)?;
            out.extend(dom);
        }
        BrepIn::Network => out.push(3.0),
    }
    out.extend([0.0, 0.0]);
    Ok(out)
}

fn props_args(p: Option<PropsIn>) -> [f64; 6] {
    match p {
        None => [0.0, 4.0, 4.0, 4.0, 0.0, 0.0],
        Some(p) => [
            1.0,
            f64::from(p.crossed),
            f64::from(p.missing),
            f64::from(p.near_miss),
            f64::from(p.closed_u),
            f64::from(p.closed_v),
        ],
    }
}

/// The op code and its arguments after the face index.
fn op_args(op: &OpIn) -> (Op, Vec<f64>) {
    match op {
        OpIn::UvDomain {
            limit,
            want_max,
            want_2nd,
        } => (
            Op::UvDomain,
            vec![flag(*limit), flag(*want_max), flag(*want_2nd)],
        ),
        OpIn::Oriented { param, fill, loop_ } => {
            (Op::Oriented, vec![flag(*param), flag(*fill), *loop_ as f64])
        }
        OpIn::Rect => (Op::Rect, vec![]),
        OpIn::Flip => (Op::Flip, vec![]),
        OpIn::SetSurface { delete } => (Op::SetSurface, vec![flag(*delete)]),
        OpIn::LoopRoundTrip { loop_ } => (Op::LoopRoundTrip, vec![*loop_ as f64]),
        OpIn::RemoveUv => (Op::RemoveUv, vec![]),
        OpIn::SwapUv => (Op::SwapUv, vec![]),
        OpIn::Shrink => (Op::Shrink, vec![]),
        OpIn::MoveSeam {
            props,
            center,
            split,
        } => {
            let mut v = props_args(*props).to_vec();
            v.extend([flag(*center), flag(*split)]);
            (Op::MoveSeam, v)
        }
        OpIn::SplitAtSeam { props } => {
            let mut v = props_args(*props).to_vec();
            v.extend([0.0, 0.0]);
            (Op::SplitAtSeam, v)
        }
    }
}

/// A cached export with its key (the `.smb` path and the block's bits).
type CachedExport = (String, Vec<u64>, Rc<Vec<f64>>);

thread_local! {
    /// The last few exports (both sides of a case export the same B-rep).
    static EXPORTS: RefCell<Vec<CachedExport>> = const { RefCell::new(Vec::new()) };
}

/// The export of a B-rep.
fn export_of(brep: &BrepIn) -> Result<Rc<Vec<f64>>, HarnessError> {
    let path = shared::path_of(brep)?;
    let block = brep_block(brep)?;
    let bits: Vec<u64> = block.iter().map(|x| x.to_bits()).collect();
    if let Some(e) = EXPORTS.with(|c| {
        c.borrow()
            .iter()
            .find(|(p, b, _)| *p == path && *b == bits)
            .map(|(_, _, e)| Rc::clone(e))
    }) {
        return Ok(e);
    }
    let e = Rc::new(
        fixture::eval(fixture::Op::Export, &path, &block)
            .map_err(|e| harness(format!("c++ export: {e}")))?,
    );
    EXPORTS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 4 {
            c.remove(0);
        }
        c.push((path, bits, Rc::clone(&e)));
    });
    Ok(e)
}

/// The inputs with the face and loop selectors resolved; `Err(output)` when the case
/// ends before the op (the B-rep did not build, or it has no face).
/// A resolved case (inputs and the B-rep export), or the output of a case that ends
/// before the op.
type Resolved = Result<(Inputs, Rc<Vec<f64>>), Output>;

fn resolve(i: &Inputs) -> Result<Resolved, HarnessError> {
    let ex = export_of(&i.brep)?;
    let Some(c) = fixture::counts(&ex) else {
        return Ok(Err(Output::new().values("out", ex.to_vec())));
    };
    let [_, _, _, n_f, _, n_l, ..] = c;
    if n_f == 0 {
        return Ok(Err(Output::err("no_face")));
    }
    let mut r = i.clone();
    r.face = shared::sel(i.face.max(0), n_f);
    match &mut r.op {
        OpIn::Oriented { loop_, .. } => {
            *loop_ = if *loop_ < 0 {
                -1
            } else {
                shared::sel(*loop_, n_l)
            };
        }
        OpIn::LoopRoundTrip { loop_ } => {
            if n_l == 0 {
                return Ok(Err(Output::err("no_loop")));
            }
            *loop_ = shared::sel((*loop_).max(0), n_l);
        }
        _ => {}
    }
    Ok(Ok((r, ex)))
}

fn input_of(i: &Inputs) -> Result<(Op, String, Vec<f64>), HarnessError> {
    let (op, args) = op_args(&i.op);
    let mut v = vec![i.face as f64];
    v.extend(args);
    v.extend(brep_block(&i.brep)?);
    Ok((op, shared::path_of(&i.brep)?, v))
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (r, _) = match resolve(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let (op, path, input) = input_of(&r)?;
    let out = ffi::eval(op, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side --------------------------------------------------------------------------

/// What ends a Rust case early: a deferral or a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn fail(e: &SolidError) -> Fail {
    match Output::unported(e) {
        Some(o) => Fail::Deferred(Box::new(o)),
        None => Fail::Harness(harness(format!("rust: {e}"))),
    }
}

/// The `SmStatus` of an error (`0` success; `SolidError::InvalidInput` is
/// `SM_ERR_INVALID_INPUT`); a deferral or another error ends the case.
fn status<T>(r: &Result<T, SolidError>) -> Result<f64, Fail> {
    match r {
        Ok(_) => Ok(0.0),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(*code)),
        Err(SolidError::InvalidInput { .. }) => Ok(SM_ERR_INVALID_INPUT),
        Err(e) => Err(fail(e)),
    }
}

/// The identity of a topology entity (faces as the import keys their surfaces' owners).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, key) = match e {
        EntityRef::Face(f) => (fp2::TYPE, 1 + u64::from(f.index())),
        EntityRef::Edge(x) => (
            solidcore::topology::edge::TYPE,
            1 << 40 | u64::from(x.index()),
        ),
        EntityRef::Edgeuse(x) => (16005, 2 << 40 | u64::from(x.index())),
        EntityRef::Loop(x) => (16003, 3 << 40 | u64::from(x.index())),
        _ => (0, 4 << 40),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(key),
    }
}

/// A classification object (always null here).
#[derive(Debug, Clone, Copy, PartialEq)]
struct H(LoopId);

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Loop
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self(l)
    }
}

/// Classification geometry for the null handles here: resolves nothing.
struct NoGeometry;

impl ClassGeometry<H> for NoGeometry {
    fn curve(&self, _: H) -> Option<&Curve> {
        None
    }
    fn surface(&self, _: H) -> Option<&Surface> {
        None
    }
    fn volume(&self, _: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _: H) -> Option<Point3> {
        None
    }
}

fn put_domain(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

/// A curve as samples (the shim's `fp1_put_curve`).
fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: Option<&Curve>) {
    let Some(c) = c else {
        o.push(-1.0);
        return;
    };
    o.push(c.dim() as f64);
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=4 {
        let t = if i == 4 {
            ivl.max()
        } else {
            ivl.min() + f64::from(i) / 4.0 * (ivl.max() - ivl.min())
        };
        let p = c.evaluate_point(k, t).unwrap_or_default();
        o.extend([p.x, p.y, p.z]);
    }
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

fn idx<I: solidcore::topology::ArenaId>(i: Option<I>) -> f64 {
    i.map_or(-1.0, |i| f64::from(i.index()))
}

/// The shim's `fp1_put_orientation`.
fn put_orientation(o: &mut Vec<f64>, b: &Brep) {
    for (_, f) in b.faces().iter() {
        o.push(idx(f.faceuse));
    }
    for (_, fu) in b.faceuses().iter() {
        o.push(orient_code(fu.orientation));
        o.push(fu.loopuses.len() as f64);
        o.extend(fu.loopuses.iter().map(|lu| f64::from(lu.index())));
    }
    for (_, l) in b.loops().iter() {
        o.push(idx(l.loopuse));
    }
}

fn put_uv_curves(k: &Kernel, o: &mut Vec<f64>, b: &Brep) {
    for (_, eu) in b.edgeuses().iter() {
        put_curve(k, o, eu.uv_trim_curve.as_ref());
    }
}

fn surf_param(code: u8) -> Result<SurfParamType, Fail> {
    SurfParamType::from_code(u64::from(code))
        .ok_or_else(|| Fail::Harness(harness("SmSurfParamType")))
}

fn tri(code: u8) -> Result<Tri, Fail> {
    match code {
        0 => Ok(Tri::False),
        1 => Ok(Tri::True),
        2 => Ok(Tri::Unsure),
        _ => Err(Fail::Harness(harness("SmBoolean"))),
    }
}

/// The face properties of a seam op.
fn face_props(p: Option<PropsIn>) -> Result<Option<FaceProps<H>>, Fail> {
    let Some(p) = p else {
        return Ok(None);
    };
    let mut fp = FaceProps::<H>::new();
    fp.bad_crossed_seam_dir = surf_param(p.crossed)?;
    fp.bad_missing_seam_dir = surf_param(p.missing)?;
    fp.bad_near_miss_seam_dir = surf_param(p.near_miss)?;
    fp.closed_u = tri(p.closed_u)?;
    fp.closed_v = tri(p.closed_v)?;
    Ok(Some(fp))
}

/// Reads one classification of the probe (`tgt_only n mid_class*n`).
fn read_class(k: &Kernel, v: &[f64], at: &mut usize) -> Result<CurveClassification<H>, Fail> {
    let mut next = || -> Result<f64, Fail> {
        let x = v
            .get(*at)
            .copied()
            .ok_or_else(|| Fail::Harness(harness("probe too short")))?;
        *at += 1;
        Ok(x)
    };
    let mut c = CurveClassification::<H>::new(k);
    c.set_tgt_loops_only(next()? != 0.0);
    let n = next()? as usize;
    c.set_size(k, n).map_err(|e| fail(&e))?;
    for i in 0..n {
        let pc = PointClassType::from_code(next()? as i32)
            .ok_or_else(|| Fail::Harness(harness("probe point class")))?;
        c.at_mut(i)
            .map_err(|e| fail(&e))?
            .mid
            .set_class_object(pc, None);
    }
    Ok(c)
}

/// The seam inputs `HasSeamProblem` gives the C++ side: `None` when the shim refuses the
/// face (`-2`).
#[allow(clippy::type_complexity)]
fn probe(
    k: &Kernel,
    i: &Inputs,
) -> Result<
    Option<(
        fp1::SeamProblem,
        CurveClassification<H>,
        CurveClassification<H>,
    )>,
    Fail,
> {
    let mut v = vec![i.face as f64];
    let path = shared::path_of(&i.brep)?;
    v.extend(brep_block(&i.brep)?);
    let out = ffi::eval(Op::Probe, &path, &v).map_err(|e| harness(format!("c++ probe: {e}")))?;
    if out.get(1) == Some(&NULL_DEREF) {
        return Ok(None);
    }
    let code = |j: usize| -> Result<u8, Fail> {
        out.get(j)
            .map(|x| *x as u8)
            .ok_or_else(|| Fail::Harness(harness("probe too short")))
    };
    let problem = fp1::SeamProblem {
        crossed: surf_param(code(2)?)?,
        missing: surf_param(code(3)?)?,
        near_miss: surf_param(code(4)?)?,
    };
    let mut at = 5;
    let cu = read_class(k, &out, &mut at)?;
    let cv = read_class(k, &out, &mut at)?;
    Ok(Some((problem, cu, cv)))
}

/// The shim's guard of the seam ops (upstream's `SplitAtSeam` recursion crashes in
/// `MergeInterval`, see `plan/upstream-issues.md`): a seam problem in both directions on
/// a face periodic in either direction, and `MoveSeam` with split past its early return.
/// Such cases are deferred to `cpp-crash-guard:UB-137`.
#[allow(clippy::too_many_arguments)]
fn seam_guard(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    props: Option<PropsIn>,
    problem: fp1::SeamProblem,
    cu: &CurveClassification<H>,
    cv: &CurveClassification<H>,
    split_op: bool,
    split: bool,
) -> Result<bool, Fail> {
    use SurfParamType as Sp;
    let s = b
        .face(f)
        .map_err(|e| fail(&e))?
        .surface
        .as_ref()
        .ok_or_else(|| Fail::Harness(harness("face without surface")))?;
    let nat = s.natural_uv_domain();
    // (as the shim: the face properties' closed flag or the real surface)
    let real_u = s.is_periodic(k, &nat, Sp::U).map_err(|e| fail(&e))?;
    let real_v = s.is_periodic(k, &nat, Sp::V).map_err(|e| fail(&e))?;
    let (per_u, per_v) = match props {
        Some(p) => (p.closed_u != 0 || real_u, p.closed_v != 0 || real_v),
        None => (real_u, real_v),
    };
    let quiet_dir = |c: u8| c == Sp::Neither as u8 || c == Sp::Unknown as u8;
    if split_op {
        let has = |p: Sp, d: Sp| p == d || p == Sp::Both;
        let merge_u = has(problem.crossed, Sp::U) || has(problem.missing, Sp::U);
        let merge_v = has(problem.crossed, Sp::V) || has(problem.missing, Sp::V);
        let no_missing = props.is_some_and(|p| quiet_dir(p.missing));
        return Ok(!no_missing && (per_u || per_v) && merge_u && merge_v);
    }
    if !split {
        return Ok(false);
    }
    let quiet = if let Some(p) = props {
        quiet_dir(p.crossed) && quiet_dir(p.missing)
    } else {
        let q = |c: &CurveClassification<H>| match c.intervals() {
            [] => true,
            [one] => one.mid.point_class() != PointClassType::Face,
            _ => false,
        };
        q(cu) && q(cv)
    };
    let _ = (per_u, per_v);
    Ok(!quiet)
}

/// The cases the shim keeps away from upstream's `SplitAtSeam` crash (UB-137): not a
/// deferral to a unit, they never become comparable.
fn merge_deferred() -> Fail {
    Fail::Deferred(Box::new(Output::deferred(
        "cpp-crash-guard:UB-137",
        "SmFace::SplitAtSeam",
    )))
}

/// The C++ shim's null guards for loop `l` of face `f` (`LoopRoundTrip`).
fn ok_round_trip(b: &Brep, f: FaceId, l: LoopId) -> Result<bool, SolidError> {
    let Some(a) = b.loop_(l)?.loopuse else {
        return Ok(false);
    };
    let Some(m) = b.loopuse(a)?.mate else {
        return Ok(false);
    };
    let (Some(_), Some(_)) = (b.loopuse(a)?.faceuse, b.loopuse(m)?.faceuse) else {
        return Ok(false);
    };
    // (a loop of another face is valid input)
    let _ = f;
    Ok(true)
}

/// `GetBrep() != NULL` as the shim checks it: the upward face use's shell reaches a B-rep.
fn reaches_brep(b: &Brep, f: FaceId) -> Result<bool, SolidError> {
    let Some(fu) = b.face(f)?.faceuse else {
        return Ok(false);
    };
    let Some(s) = eu1::faceuse_shell(b, fu)? else {
        return Ok(false);
    };
    Ok(eu1::shell_brep(b, s)?.is_some())
}

#[allow(clippy::too_many_lines)] // one record per op
fn rust_record(i: &Inputs, ex: &[f64]) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let (mut b, _) = shared::import(ex)?;
    // upstream edge curves are owned by their edges
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    for e in edges {
        if let Some(c) = b.edge_mut(e).map_err(|e| fail(&e))?.curve.as_mut() {
            let _ = c.set_owner(Some(key_of(EntityRef::Edge(e))), None);
        }
    }
    let f = FaceId::from_index(i.face as u32);
    let ko = &key_of;
    let mut o = vec![0.0];
    let e = |e: SolidError| fail(&e);
    match &i.op {
        OpIn::UvDomain {
            limit,
            want_max,
            want_2nd,
        } => {
            let r = fp1::calculate_uv_domain_from_uv_trim_curves(
                &k, &mut b, f, *limit, *want_max, *want_2nd,
            );
            o.push(status(&r)?);
            if let Ok(d) = r {
                put_domain(&mut o, &d.domain);
                o.extend([d.max_uv_loop_gap, d.second_max_uv_loop_gap]);
            } else {
                // upstream leaves the face domain in the output (the gaps are not
                // reported for a failed call)
                put_domain(&mut o, &b.face(f).map_err(e)?.uv_domain);
                o.extend([0.0, 0.0]);
            }
        }
        OpIn::Oriented { param, fill, loop_ } => {
            let l = (*loop_ >= 0).then(|| LoopId::from_index(*loop_ as u32));
            let lu_ok = match l {
                Some(l) => b.loop_(l).map_err(e)?.loopuse.is_some(),
                None => true,
            };
            // (upstream dereferences the upward face use only without a loop to use)
            if (l.is_none() && b.face(f).map_err(e)?.faceuse.is_none()) || !lu_ok {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = fp1::create_oriented_trim_boundary(&k, &mut b, f, *param, *fill, l, ko);
            o.push(status(&r)?);
            match r {
                Ok(t) => {
                    o.push(t.loops.len() as f64);
                    o.extend(t.loops.iter().map(|&n| n as f64));
                    o.push(t.curves.len() as f64);
                    for c in &t.curves {
                        put_curve(&k, &mut o, Some(c));
                    }
                }
                Err(_) => o.extend([0.0, 0.0]),
            }
        }
        OpIn::Rect => {
            let r = fp1::find_if_rectangular_trim(&k, &mut b, f, ko);
            o.push(status(&r)?);
            let t = r.unwrap_or(fp1::RectangularTrim {
                uv_domain: Extent2d::new(),
                rectangular: false,
                natural: false,
            });
            put_domain(&mut o, &t.uv_domain);
            o.extend([flag(t.rectangular), flag(t.natural)]);
        }
        OpIn::Flip => {
            let r = fp1::flip_face_orientation(&mut b, f);
            o.push(status(&r)?);
            put_orientation(&mut o, &b);
        }
        OpIn::SetSurface { delete } => {
            let Some(s) = b.face(f).map_err(e)?.surface.clone() else {
                o.push(NULL_DEREF);
                return Ok(o);
            };
            let r = fp1::set_surface(&mut b, f, key_of(EntityRef::Face(f)), s, *delete);
            status(&r)?;
            o.push(0.0);
            let face = b.face(f).map_err(e)?;
            put_domain(&mut o, &face.uv_domain);
            o.push(flag(face.surface.as_ref().is_some_and(|s| {
                s.owner() == Some(key_of(EntityRef::Face(f)))
            })));
        }
        OpIn::LoopRoundTrip { loop_ } => {
            let l = LoopId::from_index(*loop_ as u32);
            if !ok_round_trip(&b, f, l).map_err(e)? {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = fp1::remove_loop(&mut b, f, l);
            o.push(status(&r)?);
            let r = fp1::insert_loop(&mut b, f, l);
            o.push(status(&r)?);
            put_orientation(&mut o, &b);
        }
        OpIn::RemoveUv => {
            if b.face(f).map_err(e)?.faceuse.is_none() {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = fp1::remove_uv_trim_curves(&mut b, f);
            o.push(status(&r)?);
            put_uv_curves(&k, &mut o, &b);
        }
        OpIn::SwapUv => {
            let face = b.face(f).map_err(e)?;
            if face.surface.is_none()
                || face.faceuse.is_none()
                || !reaches_brep(&b, f).map_err(e)?
            {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = fp1::swap_uv(&k, &mut b, f);
            o.push(status(&r)?);
            put_domain(&mut o, &b.face(f).map_err(e)?.uv_domain);
            put_orientation(&mut o, &b);
            put_uv_curves(&k, &mut o, &b);
        }
        OpIn::Shrink => {
            let r = fp1::shrink_geometry(&k, &mut b, f, ko);
            o.push(status(&r)?);
            let face = b.face(f).map_err(e)?;
            put_domain(&mut o, &face.uv_domain);
            match face.surface.as_ref() {
                Some(s) => put_domain(&mut o, &s.natural_uv_domain()),
                None => o.push(-1.0),
            }
        }
        OpIn::MoveSeam {
            props,
            center,
            split,
        } => {
            let face = b.face(f).map_err(e)?;
            if face.surface.is_none() || face.faceuse.is_none() {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let Some((problem, mut cu, mut cv)) = probe(&k, i)? else {
                return Err(Fail::Harness(harness("probe refused a face with surface")));
            };
            if seam_guard(&k, &b, f, *props, problem, &cu, &cv, false, *split)? {
                return Err(merge_deferred());
            }
            let fp = face_props(*props)?;
            let opts = fp1::MoveSeamOptions {
                ok_to_move_to_center: *center,
                ok_to_split: *split,
            };
            let r = fp1::move_seam(
                &k,
                &mut b,
                f,
                key_of(EntityRef::Face(f)),
                &mut cu,
                &mut cv,
                problem,
                opts,
                fp.as_ref(),
                &NoGeometry,
                &|_| None,
            );
            o.push(status(&r)?);
            let r = r.unwrap_or_default();
            o.extend([
                f64::from(r.u as u8),
                f64::from(r.v as u8),
                r.split.faces.len() as f64,
                r.split.edges.len() as f64,
                r.split.vertices.len() as f64,
            ]);
        }
        OpIn::SplitAtSeam { props } => {
            let face = b.face(f).map_err(e)?;
            if face.surface.is_none() || face.faceuse.is_none() {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let Some((problem, mut cu, mut cv)) = probe(&k, i)? else {
                return Err(Fail::Harness(harness("probe refused a face with surface")));
            };
            if seam_guard(&k, &b, f, *props, problem, &cu, &cv, true, false)? {
                return Err(merge_deferred());
            }
            let fp = face_props(*props)?;
            let r = fp1::split_at_seam(
                &k,
                &mut b,
                f,
                &mut cu,
                &mut cv,
                problem,
                fp.as_ref(),
                &NoGeometry,
                &|_| None,
            );
            o.push(status(&r)?);
            let r = r.unwrap_or_default();
            o.extend([
                r.faces.len() as f64,
                r.edges.len() as f64,
                r.vertices.len() as f64,
            ]);
        }
    }
    Ok(o)
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let (r, ex) = match resolve(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    match rust_record(&r, &ex) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- cases ------------------------------------------------------------------------------

/// The seam-op face properties tried: none, then crossed/missing verdicts and closed flags.
/// `(crossed, missing, near_miss, closed_u, closed_v)`.
type PropsTuple = (u8, u8, u8, u8, u8);

const PROPS: [Option<PropsTuple>; 7] = [
    None,
    Some((4, 4, 4, 1, 1)),
    Some((2, 2, 2, 2, 2)),
    Some((0, 4, 4, 1, 0)),
    Some((4, 1, 4, 0, 0)),
    Some((3, 3, 0, 1, 1)),
    Some((4, 0, 4, 0, 2)),
];

fn props_of(p: Option<PropsTuple>) -> Option<PropsIn> {
    p.map(
        |(crossed, missing, near_miss, closed_u, closed_v)| PropsIn {
            crossed,
            missing,
            near_miss,
            closed_u,
            closed_v,
        },
    )
}

/// Every op variant for one face selector `s` (`v` varies the flags).
fn ops_for(s: i64, v: usize) -> Vec<OpIn> {
    let bit = |k: usize| (v >> k) & 1 == 1;
    vec![
        OpIn::UvDomain {
            limit: bit(0),
            want_max: bit(1),
            want_2nd: bit(2),
        },
        OpIn::Oriented {
            param: bit(0),
            fill: bit(1),
            loop_: if bit(2) { s * 3 + 1 } else { -1 },
        },
        OpIn::Rect,
        OpIn::Flip,
        OpIn::SetSurface { delete: bit(1) },
        OpIn::LoopRoundTrip { loop_: s },
        OpIn::RemoveUv,
        OpIn::SwapUv,
        OpIn::Shrink,
        OpIn::MoveSeam {
            props: props_of(PROPS.get(v % PROPS.len()).copied().flatten()),
            center: bit(0),
            split: bit(1),
        },
        OpIn::SplitAtSeam {
            props: props_of(PROPS.get((v + 3) % PROPS.len()).copied().flatten()),
        },
    ]
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    // only B-reps with faces: a wire or the wire network would make a case both sides
    // reject without comparing anything
    let brep = loop {
        let b = shared::gen_brep(rng);
        if matches!(b, BrepIn::File { .. } | BrepIn::Face { .. }) {
            break b;
        }
    };
    let s = rng.below(64) as i64;
    let ops = ops_for(s, rng.below(1 << 10) as usize);
    let op = rng.choose(&ops).cloned().unwrap_or(OpIn::Rect);
    Inputs { brep, face: s, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.face_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        // every op on the first faces of every upstream file, with and without rebuilt
        // UV trim curves
        for file in FILES {
            for rebuild in [false, true] {
                for s in 0..6i64 {
                    for (j, op) in ops_for(s, s as usize * 5 + usize::from(rebuild))
                        .into_iter()
                        .enumerate()
                    {
                        let inputs = Inputs {
                            brep: BrepIn::File {
                                file: file.to_string(),
                                rebuild,
                            },
                            face: s,
                            op,
                        };
                        v.push(Case {
                            unit: self.id().to_string(),
                            name: format!("{file}_{}_f{s}_op{j}", u8::from(rebuild)),
                            source: crate::CaseSource::Fixture {
                                path: "built-in: systematic cases".to_string(),
                                index: v.len(),
                                upstream_ref: Some(
                                    "tests/prog_test (the pt_TestFiles B-reps)".to_string(),
                                ),
                            },
                            inputs: serde_json::to_value(inputs)
                                .map_err(|e| harness(e.to_string()))?,
                        });
                    }
                }
            }
        }
        // seam ops with every face-property variant on the periodic files
        for file in [
            "sphere.smb",
            "torus.smb",
            "cone.smb",
            "surf_rev.smb",
            "vase_rev.smb",
        ] {
            for (pi, p) in PROPS.iter().enumerate() {
                for s in 0..4i64 {
                    for (j, op) in [
                        OpIn::MoveSeam {
                            props: props_of(*p),
                            center: true,
                            split: false,
                        },
                        OpIn::SplitAtSeam {
                            props: props_of(*p),
                        },
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let inputs = Inputs {
                            brep: BrepIn::File {
                                file: file.to_string(),
                                rebuild: false,
                            },
                            face: s,
                            op,
                        };
                        v.push(Case {
                            unit: self.id().to_string(),
                            name: format!("seam_{file}_p{pi}_f{s}_{j}"),
                            source: crate::CaseSource::Fixture {
                                path: "built-in: seam cases".to_string(),
                                index: v.len(),
                                upstream_ref: Some(
                                    "tests/prog_test (the pt_TestFiles B-reps)".to_string(),
                                ),
                            },
                            inputs: serde_json::to_value(inputs)
                                .map_err(|e| harness(e.to_string()))?,
                        });
                    }
                }
            }
        }
        // the reviews' crash reproducers (vase_rev.smb face 0, face properties claiming a
        // crossed seam in both directions and a missing seam, closed in U, V or both, with
        // split): the guard must keep them away from upstream's UB-137
        for missing in [0u8, 1, 3] {
            for (cu, cv) in [(1u8, 0u8), (0, 1), (1, 1)] {
                for center in [false, true] {
                    for rebuild in [false, true] {
                        let inputs = Inputs {
                            brep: BrepIn::File {
                                file: "vase_rev.smb".to_string(),
                                rebuild,
                            },
                            face: 0,
                            op: OpIn::MoveSeam {
                                props: Some(PropsIn {
                                    crossed: 3,
                                    missing,
                                    near_miss: 4,
                                    closed_u: cu,
                                    closed_v: cv,
                                }),
                                center,
                                split: true,
                            },
                        };
                        v.push(Case {
                            unit: self.id().to_string(),
                            name: format!(
                                "guard_vase_rev_m{missing}_c{cu}{cv}_{}{}",
                                u8::from(center),
                                u8::from(rebuild)
                            ),
                            source: crate::CaseSource::Fixture {
                                path: "built-in: review crash reproducers".to_string(),
                                index: v.len(),
                                upstream_ref: Some(
                                    "tests/prog_test (the pt_TestFiles B-reps)".to_string(),
                                ),
                            },
                            inputs: serde_json::to_value(inputs)
                                .map_err(|e| harness(e.to_string()))?,
                        });
                    }
                }
            }
        }
        Ok(v)
    }
}
