//! Runner for unit `topology.face_p1`. C++: `solidcore_ffi::units::topology_face_p1`.
//! Rust: `solidcore::topology::face::p1`.
//!
//! Every op runs on a B-rep of unit `topology.vertexuse` (upstream `.smb` files, faces of
//! B-spline surfaces or real `SmPlane`s, wire edges, the `my_test_topology` network): the
//! C++ side builds a real `SmBrep` and runs the `SmFace` member on the selected face of its
//! native geometry; the Rust side builds the same B-rep itself ([`own_brep`]: the Rust
//! kernel's builders, or the Rust `.smb` reader, renumbered into the shim's traversal
//! order), gives each edge curve its edge as owner (as upstream) and runs the port. No
//! C++ state reaches the Rust side: each side resolves the face and loop selectors
//! (modulo their counts) on its own B-rep. Both sides produce one flat record
//! (`out`, laid out as in the ffi module): statuses, domains, gaps, curve samples and
//! the orientation / UV trim curve state after the mutating ops.
//!
//! `MoveSeam` / `SplitAtSeam` take `HasSeamProblem`'s classifications and enums. Each
//! side computes its own: the C++ side calls `SmFace::HasSeamProblem` before the call,
//! the Rust side `topology::face::p4::has_seam_problem_in_brep_mut` (default arguments)
//! on its own B-rep, and both record what it found (`has crossed missing near_miss`,
//! then per direction the target-loop flag, interval count and middle point classes),
//! so the seam problem is compared as a value and no C++ result reaches the Rust call.
//! With `props` both sides also pass face properties
//! with the given seam verdicts and closed flags. The classification handles name the
//! B-rep's own entities, surface, curves and seam temporaries (as upstream's `SmObject`
//! pointers do), so the seam problem a `MoveSeam` with split recomputes after the move
//! (`HasSeamProblem` over the face's own objects) runs as upstream's does. The split
//! that follows defers: the caller's classification geometry here cannot borrow the
//! B-rep the call mutates, so its merges resolve no topology (`plan/findings.md`).
//!
//! Deferred: the cone `TrimWithDomain` callbacks of `ShrinkGeometry`
//! (`surface.surface_p2`), and the `MoveSeam` splits past the recomputed seam problem
//! (merges whose handles the geometry cannot resolve). Rejected: the seam calls the C++ crash trap (below) catches.
//!
//! **C++ crash trap.** Upstream `SplitAtSeam`'s U-then-V branch recurses on the wrong
//! face and crashes (`plan/upstream-issues.md` UB-137). The shim runs every seam call
//! that can reach it in a forked child: a `SplitAtSeam` merging in both directions on a
//! face periodic in either (by the face properties' closed flags or by the surface
//! itself), and every `MoveSeam` with split past its early return (it recomputes the seam
//! problem before splitting). A child that crashes answers `-4`; where the shim cannot
//! fork (Windows) it skips the call (`-3`) as before. Both are inputs undefined in C++
//! (AGENTS.md section 5.6), so the C++ side reports them as `rejected` with a reason
//! naming UB-137 and the trap: the harness then skips the Rust side and compares
//! nothing. Seam calls the trap runs to completion compare as usual.
//!
//! Both sides hold the B-rep's native geometry (analytic surfaces, conic edge curves),
//! so the conic and analytic branches (`SwapUV`'s conic mirrors) are compared.

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
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p1 as fp1;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::face::p4 as fp4;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, LoopId, VertexId};
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
/// A seam call the shim skipped: it could not run it in a child process (UB-137).
const NOT_RUN: f64 = -3.0;
/// A seam call whose child process crashed, hung or sent no whole record (UB-137).
const CRASHED: f64 = -4.0;
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
    UvDomainAfterTrimShift {
        limit: bool,
        want_max: bool,
        want_2nd: bool,
        domain: [f64; 4],
        shift: [f64; 2],
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
        OpIn::UvDomainAfterTrimShift {
            limit,
            want_max,
            want_2nd,
            domain,
            shift,
        } => {
            let mut args = vec![flag(*limit), flag(*want_max), flag(*want_2nd)];
            args.extend(domain);
            args.extend(shift);
            (Op::UvDomainAfterTrimShift, args)
        }
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
    Ok(resolve_with(i, c).map(|r| (r, ex)).map_err(|o| *o))
}

/// The inputs with the selectors resolved against entity counts `c`, or the output of
/// a case without a face (or loop).
fn resolve_with(i: &Inputs, c: [usize; 10]) -> Result<Inputs, Box<Output>> {
    let [_, _, _, n_f, _, n_l, ..] = c;
    if n_f == 0 {
        return Err(Box::new(Output::err("no_face")));
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
                return Err(Box::new(Output::err("no_loop")));
            }
            *loop_ = shared::sel((*loop_).max(0), n_l);
        }
        _ => {}
    }
    Ok(r)
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
    if let Some(reason) = trapped(&out) {
        return Ok(Output::rejected(reason));
    }
    Ok(Output::new().values("out", out))
}

/// The rejection of a seam call the shim's crash trap did not get a record from: the
/// input is undefined in C++ (`plan/upstream-issues.md` UB-137, AGENTS.md section 5.6).
/// The child crashed (`-4`, confirmed at runtime), or the shim could not fork and
/// skipped the call (`-3`). The harness then runs neither the Rust side nor a compare.
fn trapped(out: &[f64]) -> Option<&'static str> {
    match out {
        [b, x] if *b == 0.0 && *x == CRASHED => {
            Some("UB-137: SmFace::SplitAtSeam crashed in the shim's fork crash trap")
        }
        [b, x] if *b == 0.0 && *x == NOT_RUN => {
            Some("UB-137: SmFace::SplitAtSeam can crash; no fork for the crash trap, call skipped")
        }
        _ => None,
    }
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

/// A classification object. The classifications `HasSeamProblem` makes name the B-rep's
/// entities, its surface, edge and trim curves and the seam temporaries, as upstream's
/// `SmObject` pointers do (`topology::face::p4::BrepSeamObjects` requires all of them).
#[derive(Debug, Clone, Copy, PartialEq)]
enum H {
    Topo(EntityRef),
    Surface(FaceId),
    Curve(bool),
    EdgeCurve(EdgeId),
    Trim(EdgeuseId),
    /// A temporary curve (the seam curves of `SmFace::HasSeamProblem`).
    Temp(usize),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Topo(e) => e.class_kind(),
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Curve(_) | Self::EdgeCurve(_) | Self::Trim(_) | Self::Temp(_) => {
                ClassObjectKind::Curve
            }
        }
    }

    fn temporary(index: usize, kind: ClassObjectKind) -> Option<Self> {
        (kind == ClassObjectKind::Curve).then_some(Self::Temp(index))
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self::Topo(EntityRef::Loop(l))
    }

    fn as_loop(&self) -> Option<LoopId> {
        match self {
            Self::Topo(e) => e.as_loop(),
            _ => None,
        }
    }

    fn as_vertex(&self) -> Option<VertexId> {
        match self {
            Self::Topo(e) => e.as_vertex(),
            _ => None,
        }
    }

    fn from_face(f: FaceId) -> Option<Self> {
        Some(Self::Topo(EntityRef::Face(f)))
    }

    fn from_face_surface(f: FaceId) -> Option<Self> {
        Some(Self::Surface(f))
    }

    fn from_classify_curve(uv: bool) -> Option<Self> {
        Some(Self::Curve(uv))
    }

    fn from_topology(t: EntityRef) -> Option<Self> {
        Some(Self::Topo(t))
    }

    fn as_topology(&self) -> Option<EntityRef> {
        match self {
            Self::Topo(t) => Some(*t),
            _ => None,
        }
    }

    fn from_edge_curve(e: EdgeId) -> Option<Self> {
        Some(Self::EdgeCurve(e))
    }

    fn as_edge_curve(&self) -> Option<EdgeId> {
        match self {
            Self::EdgeCurve(e) => Some(*e),
            _ => None,
        }
    }

    fn from_trim_curve(eu: EdgeuseId) -> Option<Self> {
        Some(Self::Trim(eu))
    }

    fn as_trim_curve(&self) -> Option<EdgeuseId> {
        match self {
            Self::Trim(eu) => Some(*eu),
            _ => None,
        }
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

/// Puts one classification (`tgt_only n mid_class*n`, as the shim's `fp1_put_class`).
fn put_class(o: &mut Vec<f64>, c: &CurveClassification<H>) -> Result<(), Fail> {
    o.push(flag(c.tgt_loops_only()));
    o.push(c.size() as f64);
    for i in 0..c.size() {
        let pc = c.at(i).map_err(|e| fail(&e))?.mid.point_class();
        o.push(f64::from(pc as i32));
    }
    Ok(())
}

/// The seam ops' own `HasSeamProblem` (default arguments) on the face, recorded
/// (`has crossed missing near_miss class_u class_v`) as the shim records the C++ one;
/// returns its classifications and enums for the seam call.
fn seam_problem(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    o: &mut Vec<f64>,
) -> Result<
    (
        fp1::SeamProblem,
        CurveClassification<H>,
        CurveClassification<H>,
    ),
    Fail,
> {
    let check =
        fp4::has_seam_problem_in_brep_mut::<H>(k, b, f, fp4::HasSeamProblemOptions::default())
            .map_err(|e| fail(&e))?;
    o.push(flag(check.has_problem));
    o.extend([
        f64::from(check.problem.crossed as u8),
        f64::from(check.problem.missing as u8),
        f64::from(check.problem.near_miss as u8),
    ]);
    put_class(o, &check.class_u)?;
    put_class(o, &check.class_v)?;
    Ok((check.problem, check.class_u, check.class_v))
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
fn rust_record(i: &Inputs, mut b: Brep) -> Result<Vec<f64>, Fail> {
    let mut k = Kernel::default();
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
        }
        | OpIn::UvDomainAfterTrimShift {
            limit,
            want_max,
            want_2nd,
            ..
        } => {
            if let OpIn::UvDomainAfterTrimShift { domain, shift, .. } = &i.op {
                if fp2::surface(&b, f).map_err(e)?.is_none() {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
                solidcore::topology::face::p3::create_uv_trim_curves(&k, &mut b, f, false, None)
                    .map_err(e)?;
                b.face_mut(f).map_err(e)?.uv_domain =
                    Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]);
                let mut transform = solidcore::geom::axis2_placement::Axis2Placement::default();
                transform.translate(solidcore::math::Vec3::new(shift[0], shift[1], 0.0));
                for lu in
                    solidcore::topology::face::p4::upward_loopuses(&b, f, false, None).map_err(e)?
                {
                    for eu in eu1::loopuse_edgeuses(&b, lu).map_err(e)? {
                        let (_, owner) = eu1::uv_trim_curve(&b, eu).map_err(e)?;
                        if let Some(owner) = owner {
                            if let Some(curve) =
                                b.edgeuse_mut(owner).map_err(e)?.uv_trim_curve.as_mut()
                            {
                                curve.transform(&k, &transform, None).map_err(e)?;
                            }
                        }
                    }
                }
            }
            let mut d = fp1::UvDomainFromTrim {
                domain: Extent2d::new(),
                max_uv_loop_gap: 0.0,
                second_max_uv_loop_gap: 0.0,
            };
            let r = fp1::calculate_uv_domain_from_uv_trim_curves_into(
                &k, &mut b, f, *limit, *want_max, *want_2nd, &mut d,
            );
            o.push(status(&r)?);
            put_domain(&mut o, &d.domain);
            if r.is_ok() {
                o.extend([d.max_uv_loop_gap, d.second_max_uv_loop_gap]);
            } else {
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
            let r = fp1::set_surface(&k, &mut b, f, key_of(EntityRef::Face(f)), s, *delete);
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
                Some(s) => put_domain(
                    &mut o,
                    &s.natural_uv_domain_with_nurb(&k)
                        .map_err(|e| HarnessError::new(e.to_string()))?,
                ),
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
            let (problem, mut cu, mut cv) = seam_problem(&k, &mut b, f, &mut o)?;
            let fp = face_props(*props)?;
            let opts = fp1::MoveSeamOptions {
                ok_to_move_to_center: *center,
                ok_to_split: *split,
            };
            let r = fp1::move_seam(
                &mut k,
                &mut b,
                f,
                key_of(EntityRef::Face(f)),
                &mut cu,
                &mut cv,
                problem,
                opts,
                fp.as_ref(),
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
            let (problem, mut cu, mut cv) = seam_problem(&k, &mut b, f, &mut o)?;
            let fp = face_props(*props)?;
            let r = fp1::split_at_seam(
                &mut k,
                &mut b,
                f,
                &mut cu,
                &mut cv,
                problem,
                fp.as_ref(),
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

/// The Rust B-rep of a `topology.vertexuse` B-rep block, built by the Rust kernel as
/// that unit's shim (`sc_topology_fixture.h`) builds it: a shim-built source by
/// [`crate::own_builder`], an `.smb` file by the Rust reader, both renumbered into the
/// shim's traversal order under its conventions, or the build's error. No C++ state
/// reaches it.
///
/// # Errors
///
/// A harness error for a source the shim would not accept either.
pub(super) fn own_built(
    src: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
) -> Result<crate::own_builder::Built, HarnessError> {
    use crate::own_builder::{build_as, read_as, renumber_with, Built, Conventions};
    let mut k = Kernel::default();
    let conv = &Conventions::TOPOLOGY_FIXTURE;
    let BrepIn::File { rebuild, .. } = src else {
        return build_as(&mut k, src, validate_only, init_gaps, conv)?
            .ok_or_else(|| harness("own builder: no build for a shim-built source"));
    };
    let path = shared::path_of(src)?;
    if init_gaps {
        // the gap walk may leave size estimates a copy does not carry: read afresh
        return read_as(&mut k, &path, *rebuild, validate_only, init_gaps, conv);
    }
    let key = (path, *rebuild, validate_only);
    // a copy of the same file's earlier Rust read (the reader is deterministic): the
    // renumbering of a B-rep already in traversal order is a plain copy
    let hit = OWN_READS.with(|c| {
        c.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, b)| renumber_with(b, conv))
    });
    if let Some(b) = hit {
        return Ok(Built::Brep(Box::new(b?)));
    }
    let built = read_as(&mut k, &key.0, key.1, validate_only, false, conv)?;
    if let Built::Brep(b) = &built {
        let copy = renumber_with(b, conv)?;
        OWN_READS.with(|c| {
            let mut c = c.borrow_mut();
            if c.len() >= 4 {
                c.remove(0);
            }
            c.push((key, copy));
        });
    }
    Ok(built)
}

/// A file source's key (path, rebuild, `validate_only`) and its Rust read.
type OwnRead = ((String, bool, bool), Brep);

thread_local! {
    /// The last few Rust `.smb` reads, by key.
    static OWN_READS: RefCell<Vec<OwnRead>> = const { RefCell::new(Vec::new()) };
}

/// [`own_built`] with its entity counts; a failed build is the record the shim gives
/// one (`-1 status`), or the deferral of an unported callee.
///
/// # Errors
///
/// As [`own_built`].
pub(super) fn own_brep(
    src: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
) -> Result<crate::own_builder::RustBrep<(Brep, [usize; 10])>, HarnessError> {
    use crate::own_builder::{build_status, counts, Built};
    match own_built(src, validate_only, init_gaps)? {
        Built::Brep(b) => {
            let c = counts(&b);
            Ok(Ok((*b, c)))
        }
        Built::Failed(e) => Ok(Err(Output::unported(&e)
            .unwrap_or_else(|| Output::new().values("out", vec![-1.0, build_status(&e)])))),
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let (b, c) = match own_brep(&i.brep, false, false)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let r = match resolve_with(i, c) {
        Ok(r) => r,
        Err(o) => return Ok(*o),
    };
    match rust_record(&r, b) {
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
        // split): the crash trap must catch upstream's UB-137 there
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
