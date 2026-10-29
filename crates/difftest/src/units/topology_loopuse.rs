//! Runner for unit `topology.loopuse`. C++: `solidcore_ffi::units::topology_loopuse`.
//! Rust: `solidcore::topology::loopuse`.
//!
//! The B-reps are those of unit `topology.edgeuse_p1` (upstream `.smb` files, wire edges,
//! faces from B-spline surfaces and real `SmPlane`s, the wire network of `prog_test`
//! `my_test_topology`): the C++ side builds a real `SmBrep` per call. The Rust side
//! builds every non-file source itself ([`crate::own_builder`], renumbered into the
//! shim's traversal order); for an `.smb` file it imports the graph the shim exports
//! ([`super::topology_edgeuse_p1::import`], which carries each loop use's
//! `m_eOrientation`; each face's `m_pFU`, which the export lacks, comes from the shim's
//! `FaceFu` op). The ported functions run on both.
//! Loop, loop use, edge use and target arguments are selectors taken modulo the count
//! of their kind (`-1` selects null). Results are compared as traversal indices; the
//! mutating ops (`FlipLoopOrientation`, `CollectEdgeuses`, `SetEdgeuse`, the setters)
//! compare the loop, edge and vertex use links of the whole B-rep afterwards.
//!
//! `LuLists` attaches an attribute (id 777) to the selected objects on the C++ side; the
//! Rust filter is the predicate "is one of those objects". `Closed3d` with `edge_tol`
//! first sets every edge's tolerance to it (`fixtures/topology.loopuse/tolerance_limit.json`:
//! zero, tiny and negative tolerances), so that gaps exceed their `XSectTol3d` and the
//! over-tolerance branch (with its early stop when `check_all` is false) is compared.
//!
//! `IsConnectedTo` with a vertex or edge target runs `SmVertex::IsConnectedTo` /
//! `SmEdge::IsConnectedTo` (null dereference status when the loop has no loop use, or,
//! for a vertex, a loop use whose vertices cannot be walked).
//!
//! `GetUVCornerBoundaries` of an edge loop runs `GetOrCreateUVTrimCurve` on both sides
//! (`topology.edgeuse_p3`; creating a curve defers on its own callees).
//!
//! Deferred: `FlipLoopOrientation` of a loop on a face (the post-edit `SmFace::Notify`
//! reaches `SmFace::GetUpwardEdgeuses`, `topology.face_p4`), and `IsMissingSeamAtVertices` past
//! the closure test (the sector computation's own deferrals). `IsClosed3d` on edges
//! whose curve the export holds only as its B-spline part (ellipses and other
//! `SmBSplineCurve` subclasses of the `.smb` files) and `IsMissingSeamAtVertices` on such
//! surfaces are deferred as in unit `topology.edgeuse_p2`.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::loopuse as lu1;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, LoopId, LoopuseId, LoopuseStart,
    RegionId, ShellId, VertexId, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;
use solidcore_ffi::units::topology_loopuse::{self as ffi, Op};

use super::topology_edgeuse_p1::{
    brep_block_of, gen_brep, harness, import_with_types, path_of, sel, BrepIn, ImportTypes,
    FILES,
};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;
/// The shim did not run the op (deferred on the Rust side).
const NOT_RUN: f64 = -3.0;

/// The `IsKindOf` type codes the shim asks about (its `KIND_TYPES`).
const KIND_TYPES: [i64; 8] = [16004, 16003, 16001, 16005, 16010, 12060, 11001, 16999];

/// `SmTopoTypes.h` codes of target kinds, in the export's count order.
const KINDS: [i64; 10] = [
    16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021,
];

const SHELL_TYPE: f64 = 16013.0;
const LOOPUSE_TYPE: f64 = 16004.0;
const EDGEUSE_TYPE: f64 = 16005.0;
const VERTEXUSE_TYPE: f64 = 16006.0;
const UNKNOWN_TYPE: f64 = 16999.0;

/// In-code cases (systematic enumeration over the upstream files and constructed
/// B-reps) beyond the fixtures and the generated ones.
const EXTRA_GENERATED: usize = 600;

/// Times a generated case's B-rep is drawn again, at most, when it lacks the loopuse or
/// loop its op selects (a wire or an edge network has neither).
const BREP_DRAWS: usize = 8;

/// A target selector: an `SmTopoTypes.h` code and a selector of that kind.
type TargetIn = (i64, i64);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    LuQuery {
        lu: i64,
    },
    LuLists {
        lu: i64,
        use_attr: bool,
        #[serde(default)]
        attributed: Vec<TargetIn>,
        conn: Option<TargetIn>,
    },
    LuConnected {
        lu: i64,
        target: TargetIn,
    },
    LoopQuery {
        l: i64,
    },
    LoopConnected {
        l: i64,
        target: TargetIn,
    },
    /// `edge_tol`: every edge's tolerance is set to it first (tolerance-limit cases).
    Closed3d {
        of_loop: bool,
        index: i64,
        check_all: bool,
        want: [bool; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edge_tol: Option<f64>,
    },
    MissingSeam {
        lu: i64,
    },
    /// `dom` as fractions of the loop's face UV domain (`[0, 1]` without face).
    UvCorners {
        l: i64,
        dom: [f64; 4],
        tol: f64,
    },
    Flip {
        l: i64,
    },
    Collect {
        lu: i64,
        eu: i64,
    },
    SetEdgeuse {
        lu: i64,
        eu: i64,
    },
    SetOrient {
        lu: i64,
        orientation: u8,
    },
    SetLoop {
        lu: i64,
        l: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

/// An op with its selectors resolved to export indices (`-1` null) and its domain to
/// absolute UV values.
#[derive(Debug, Clone)]
enum Resolved {
    LuQuery(i64),
    LuLists(i64, bool, Vec<TargetIn>, TargetIn),
    LuConnected(i64, TargetIn),
    LoopQuery(i64),
    LoopConnected(i64, TargetIn),
    Closed3d(bool, i64, bool, [bool; 3], Option<f64>),
    MissingSeam(i64),
    UvCorners(i64, [f64; 4], f64),
    Flip(i64),
    Collect(i64, i64),
    SetEdgeuse(i64, i64),
    SetOrient(i64, u8),
    SetLoop(i64, i64),
}

struct Setup {
    path: String,
    block: Vec<f64>,
    brep: Brep,
    types: ImportTypes,
    op: Resolved,
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn resolve_target(t: TargetIn, c: [usize; 10]) -> TargetIn {
    match KINDS.iter().position(|&k| k == t.0) {
        Some(slot) => (t.0, sel(t.1, c.get(slot).copied().unwrap_or(0))),
        None => (t.0, -1),
    }
}

/// The face UV domain of a loop in the import (`[0, 1]^2` without one).
fn loop_domain(b: &Brep, l: i64) -> [f64; 4] {
    let face = u32::try_from(l)
        .ok()
        .and_then(|l| lu1::loop_face(b, LoopId::from_index(l)).ok().flatten());
    match face.and_then(|f| b.face(f).ok()) {
        Some(f) => {
            let d = f.uv_domain;
            [d.min().x, d.min().y, d.max().x, d.max().y]
        }
        None => [0.0, 0.0, 1.0, 1.0],
    }
}

/// Resolves `op`; `None` when a required entity kind is empty.
fn resolve(op: &OpIn, c: [usize; 10], b: &Brep) -> Option<Resolved> {
    let [_, _, _, _, n_lu, n_l, n_eu, _, _, _] = c;
    let req = |s: i64, n: usize| -> Option<i64> {
        let r = sel(s.max(0), n);
        (r >= 0).then_some(r)
    };
    Some(match op {
        OpIn::LuQuery { lu } => Resolved::LuQuery(req(*lu, n_lu)?),
        OpIn::LuLists {
            lu,
            use_attr,
            attributed,
            conn,
        } => Resolved::LuLists(
            req(*lu, n_lu)?,
            *use_attr,
            attributed.iter().map(|&t| resolve_target(t, c)).collect(),
            conn.map_or((-1, -1), |t| resolve_target(t, c)),
        ),
        OpIn::LuConnected { lu, target } => {
            Resolved::LuConnected(req(*lu, n_lu)?, resolve_target(*target, c))
        }
        OpIn::LoopQuery { l } => Resolved::LoopQuery(req(*l, n_l)?),
        OpIn::LoopConnected { l, target } => {
            Resolved::LoopConnected(req(*l, n_l)?, resolve_target(*target, c))
        }
        OpIn::Closed3d {
            of_loop,
            index,
            check_all,
            want,
            edge_tol,
        } => Resolved::Closed3d(
            *of_loop,
            req(*index, if *of_loop { n_l } else { n_lu })?,
            *check_all,
            *want,
            *edge_tol,
        ),
        OpIn::MissingSeam { lu } => Resolved::MissingSeam(req(*lu, n_lu)?),
        OpIn::UvCorners { l, dom, tol } => {
            let l = req(*l, n_l)?;
            let [u0, v0, u1, v1] = loop_domain(b, l);
            let (du, dv) = (u1 - u0, v1 - v0);
            Resolved::UvCorners(
                l,
                [u0 + dom[0] * du, v0 + dom[1] * dv, u0 + dom[2] * du, v0 + dom[3] * dv],
                *tol,
            )
        }
        OpIn::Flip { l } => Resolved::Flip(req(*l, n_l)?),
        OpIn::Collect { lu, eu } => Resolved::Collect(req(*lu, n_lu)?, sel(*eu, n_eu)),
        OpIn::SetEdgeuse { lu, eu } => Resolved::SetEdgeuse(req(*lu, n_lu)?, sel(*eu, n_eu)),
        OpIn::SetOrient { lu, orientation } => {
            Resolved::SetOrient(req(*lu, n_lu)?, orientation % 3)
        }
        OpIn::SetLoop { lu, l } => Resolved::SetLoop(req(*lu, n_lu)?, sel(*l, n_l)),
    })
}

/// The C++ export of a B-rep block and each face's `m_pFU` (`None` for a failed build).
type Graph = (Vec<f64>, Option<Vec<f64>>);

/// A cached graph with its key (the `.smb` path and the block's bits).
type CachedGraph = (String, Vec<u64>, Rc<Graph>);

thread_local! {
    /// The last few graphs (both sides of a case build the same B-rep).
    static GRAPHS: RefCell<Vec<CachedGraph>> = const { RefCell::new(Vec::new()) };
}

fn graph_of(path: &str, block: &[f64]) -> Result<Rc<Graph>, HarnessError> {
    let bits: Vec<u64> = block.iter().map(|x| x.to_bits()).collect();
    if let Some(g) = GRAPHS.with(|c| {
        c.borrow()
            .iter()
            .find(|(p, b, _)| p == path && *b == bits)
            .map(|(_, _, g)| Rc::clone(g))
    }) {
        return Ok(g);
    }
    let ex = ffi1::eval(ffi1::Op::Export, path, block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let fus = if ffi1::counts(&ex).is_some() {
        Some(
            ffi::eval(Op::FaceFu, path, block)
                .map_err(|e| harness(format!("c++ face uses: {e}")))?,
        )
    } else {
        None
    };
    let g = Rc::new((ex, fus));
    GRAPHS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 4 {
            c.remove(0);
        }
        c.push((path.to_string(), bits, Rc::clone(&g)));
    });
    Ok(g)
}

/// The case's B-rep and resolved op. `own` (the Rust side) builds a shim-built source
/// with the Rust kernel ([`crate::own_builder`]); the C++ side, and the Rust side for
/// an `.smb` file, import the shim's graph export.
fn setup(i: &Inputs, own: bool) -> Result<Result<Setup, Output>, HarnessError> {
    use crate::own_builder::{build, counts, failed_output, Built};
    let path = path_of(Some(&i.brep))?;
    let block = brep_block_of(Some(&i.brep), i.validate_only, i.init_gaps)?;
    if own {
        let mut k = Kernel::default();
        match build(&mut k, &i.brep, i.validate_only, i.init_gaps)? {
            Some(Built::Brep(b)) => {
                let brep = *b;
                let c = counts(&brep);
                let Some(op) = resolve(&i.op, c, &brep) else {
                    return Ok(Err(Output::err("no_entity_of_that_kind")));
                };
                return Ok(Ok(Setup {
                    path,
                    block,
                    brep,
                    types: ImportTypes::default(),
                    op,
                }));
            }
            Some(Built::Failed(e)) => return Ok(Err(failed_output(&e))),
            None => {}
        }
    }
    let g = graph_of(&path, &block)?;
    let (ex, fus) = (&g.0, &g.1);
    let (Some(c), Some(fus)) = (ffi1::counts(ex), fus) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let (mut brep, types) = import_with_types(ex)?;
    // each face's m_pFU, which the export lacks
    let n = fus.get(1).copied().unwrap_or(-1.0);
    if fus.first() != Some(&0.0) || n != brep.faces().len() as f64 {
        return Err(harness("c++ face uses: bad output"));
    }
    for (f, &fu) in fus.get(2..).unwrap_or(&[]).iter().enumerate() {
        brep.face_mut(FaceId::from_index(f as u32))
            .map_err(|e| harness(e.to_string()))?
            .faceuse = opt(fu as i64, FaceuseId::from_index);
    }
    let Some(op) = resolve(&i.op, c, &brep) else {
        return Ok(Err(Output::err("no_entity_of_that_kind")));
    };
    Ok(Ok(Setup {
        path,
        block,
        brep,
        types,
        op,
    }))
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let s = match setup(i, false)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = s.block.clone();
    let t = |input: &mut Vec<f64>, t: TargetIn| input.extend([t.0 as f64, t.1 as f64]);
    let op = match &s.op {
        Resolved::LuQuery(lu) => {
            input.push(*lu as f64);
            Op::LuQuery
        }
        Resolved::LuLists(lu, use_attr, attributed, conn) => {
            input.extend([*lu as f64, flag(*use_attr), attributed.len() as f64]);
            for a in attributed {
                t(&mut input, *a);
            }
            t(&mut input, *conn);
            Op::LuLists
        }
        Resolved::LuConnected(lu, target) => {
            input.push(*lu as f64);
            t(&mut input, *target);
            Op::LuConnected
        }
        Resolved::LoopQuery(l) => {
            input.push(*l as f64);
            Op::LoopQuery
        }
        Resolved::LoopConnected(l, target) => {
            input.push(*l as f64);
            t(&mut input, *target);
            Op::LoopConnected
        }
        Resolved::Closed3d(of_loop, index, all, want, edge_tol) => {
            input.extend([flag(*of_loop), *index as f64, flag(*all)]);
            input.extend(want.map(flag));
            input.extend([flag(edge_tol.is_some()), edge_tol.unwrap_or(0.0)]);
            Op::Closed3d
        }
        Resolved::MissingSeam(lu) => {
            input.push(*lu as f64);
            Op::MissingSeam
        }
        Resolved::UvCorners(l, dom, tol) => {
            input.push(*l as f64);
            input.extend(dom);
            input.push(*tol);
            Op::UvCorners
        }
        Resolved::Flip(l) => {
            input.push(*l as f64);
            Op::Flip
        }
        Resolved::Collect(lu, eu) => {
            input.extend([*lu as f64, *eu as f64]);
            Op::Collect
        }
        Resolved::SetEdgeuse(lu, eu) => {
            input.extend([*lu as f64, *eu as f64]);
            Op::SetEdgeuse
        }
        Resolved::SetOrient(lu, o) => {
            input.extend([*lu as f64, f64::from(*o)]);
            Op::SetOrient
        }
        Resolved::SetLoop(lu, l) => {
            input.extend([*lu as f64, *l as f64]);
            Op::SetLoop
        }
    };
    let out = ffi::eval(op, &s.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    if out.first() != Some(&0.0) {
        return Err(harness("c++: B-rep build failed after export"));
    }
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(output_of(&s.op, body))
}

/// The output of an op body, as both sides build it.
fn output_of(op: &Resolved, body: &[f64]) -> Output {
    if body.first() == Some(&NOT_RUN) && body.len() == 1 {
        // only on paths the Rust side defers; compared, it fails the case
        return Output::err("not_run_by_cpp");
    }
    let ints = |v: &[f64]| -> Vec<i64> { v.iter().map(|&x| x as i64).collect() };
    match op {
        Resolved::Closed3d(..) if body.len() == 4 => Output::new()
            .ints("q", ints(body.get(..2).unwrap_or(&[])))
            .values("gap_tol", body.get(2..).unwrap_or(&[]).to_vec()),
        Resolved::UvCorners(..) if body.len() > 1 => {
            // boundaries, edges list, sides list, then the params list
            let mut at = 1usize;
            for _ in 0..2 {
                let n = body.get(at).copied().unwrap_or(0.0) as usize;
                at += 1 + n;
            }
            let head = body.get(..at).unwrap_or(body);
            let params = body.get(at + 1..).unwrap_or(&[]);
            Output::new()
                .ints("q", ints(head))
                .int("n_params", body.get(at).copied().unwrap_or(-1.0) as i64)
                .values("params", params.to_vec())
        }
        _ => Output::new().ints("q", ints(body)),
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// What makes a Rust case deferred or a harness error; an operation failure is an
/// output slot.
enum Fail {
    Unported(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

/// A slot's value: the result, or [`NULL_DEREF`] for [`SolidError::InvalidInput`];
/// deferral and other errors end the case.
fn slot<T>(r: Result<T, SolidError>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(SolidError::InvalidInput { .. }) => Ok(None),
        Err(e) => Err(fail(&e)),
    }
}

fn fail(e: &SolidError) -> Fail {
    match Output::unported(e) {
        Some(o) => Fail::Unported(Box::new(o)),
        None => Fail::Harness(harness(format!("rust: {e}"))),
    }
}

fn idx<I>(o: Option<I>, f: impl Fn(I) -> u32) -> f64 {
    o.map_or(-1.0, |x| f64::from(f(x)))
}

fn put_list(o: &mut Vec<f64>, v: &[f64]) {
    o.push(v.len() as f64);
    o.extend(v);
}

fn put_str(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

fn orient_of(code: u8) -> OrientType {
    match code {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}

fn u(i: i64) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

fn opt<I>(i: i64, f: impl Fn(u32) -> I) -> Option<I> {
    (i >= 0).then(|| f(u(i)))
}

fn entity(t: TargetIn) -> Option<EntityRef> {
    let (ty, i) = t;
    if i < 0 {
        return None;
    }
    let i = u(i);
    Some(match ty {
        16012 => EntityRef::Region(RegionId::from_index(i)),
        16013 => EntityRef::Shell(ShellId::from_index(i)),
        16022 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        16002 => EntityRef::Face(FaceId::from_index(i)),
        16004 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        16003 => EntityRef::Loop(LoopId::from_index(i)),
        16005 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        16016 => EntityRef::Edge(EdgeId::from_index(i)),
        16006 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        16021 => EntityRef::Vertex(VertexId::from_index(i)),
        _ => return None,
    })
}

/// The four member lists of a loop use, as the shim's `put_lists`.
fn put_lists(
    o: &mut Vec<f64>,
    b: &Brep,
    lu: LoopuseId,
    attr: Option<lu1::AttributeFilter<'_>>,
    conn: Option<EntityRef>,
) -> Result<(), Fail> {
    match slot(lu1::edges(b, lu, attr))? {
        Some(v) => put_list(o, &v.iter().map(|&e| idx(e, EdgeId::index)).collect::<Vec<_>>()),
        None => o.push(NULL_DEREF),
    }
    match slot(lu1::edgeuses(b, lu, attr, conn))? {
        Some(v) => put_list(o, &v.iter().map(|e| f64::from(e.index())).collect::<Vec<_>>()),
        None => o.push(NULL_DEREF),
    }
    match slot(lu1::vertexuses(b, lu, attr))? {
        Some(v) => put_list(
            o,
            &v.iter().map(|&e| idx(e, VertexuseId::index)).collect::<Vec<_>>(),
        ),
        None => o.push(NULL_DEREF),
    }
    match slot(lu1::vertices(b, lu, attr))? {
        Some(v) => put_list(o, &v.iter().map(|&e| idx(e, VertexId::index)).collect::<Vec<_>>()),
        None => o.push(NULL_DEREF),
    }
    Ok(())
}

/// The use state, as the shim's `put_use_state`.
fn put_use_state(o: &mut Vec<f64>, b: &Brep) {
    for (_, r) in b.loopuses().iter() {
        let (ty, s) = match r.start {
            LoopuseStart::Unknown => (UNKNOWN_TYPE, -1.0),
            LoopuseStart::Edgeuse(eu) => (EDGEUSE_TYPE, idx(eu, EdgeuseId::index)),
            LoopuseStart::Vertexuse(vu) => (VERTEXUSE_TYPE, idx(vu, VertexuseId::index)),
        };
        o.extend([
            ty,
            s,
            orient_code(r.orientation),
            idx(r.loop_, LoopId::index),
            idx(r.faceuse, FaceuseId::index),
        ]);
    }
    for (_, r) in b.edgeuses().iter() {
        let (ty, s) = match r.owner {
            EdgeuseOwner::Unknown => (UNKNOWN_TYPE, -1.0),
            EdgeuseOwner::Loopuse(lu) => (LOOPUSE_TYPE, idx(lu, LoopuseId::index)),
            EdgeuseOwner::Shell(sh) => (SHELL_TYPE, idx(sh, ShellId::index)),
        };
        o.extend([ty, s]);
    }
    for (_, r) in b.vertexuses().iter() {
        let (ty, s) = match r.owner {
            VertexuseOwner::Unknown => (UNKNOWN_TYPE, -1.0),
            VertexuseOwner::Edgeuse(eu) => (EDGEUSE_TYPE, idx(eu, EdgeuseId::index)),
            VertexuseOwner::Loopuse(lu) => (LOOPUSE_TYPE, idx(lu, LoopuseId::index)),
            VertexuseOwner::Shell(sh) => (SHELL_TYPE, idx(sh, ShellId::index)),
        };
        o.extend([ty, s]);
    }
}

/// A status slot: `0`, the upstream code, or `None` for [`SolidError::InvalidInput`].
fn status(r: Result<(), SolidError>) -> Result<Option<f64>, Fail> {
    match r {
        Ok(()) => Ok(Some(0.0)),
        Err(SolidError::Upstream { code, .. }) => Ok(Some(f64::from(code))),
        Err(e) => slot(Err(e)).map(|_: Option<()>| None),
    }
}

fn bool_slot(o: &mut Vec<f64>, r: Result<bool, SolidError>) -> Result<(), Fail> {
    o.push(slot(r)?.map_or(NULL_DEREF, flag));
    Ok(())
}

fn run_rust_op(b: &mut Brep, op: &Resolved) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match op {
        Resolved::LuQuery(lu) => {
            let lu = LoopuseId::from_index(u(*lu));
            let e = |r| slot(r).map(|x: Option<f64>| x.unwrap_or(NULL_DEREF));
            o.push(e(lu1::vertexuse(b, lu).map(|x| idx(x, VertexuseId::index)))?);
            o.push(e(lu1::faceuse(b, lu).map(|x| idx(x, FaceuseId::index)))?);
            o.push(e(lu1::loop_(b, lu).map(|x| idx(x, LoopId::index)))?);
            o.push(e(lu1::orientation(b, lu).map(orient_code))?);
            o.push(e(lu1::other_loopuse(b, lu).map(|x| idx(x, LoopuseId::index)))?);
            let (st, si) = match slot(lu1::eu_or_vu(b, lu))?.flatten() {
                Some(EntityRef::Edgeuse(eu)) => (EDGEUSE_TYPE, f64::from(eu.index())),
                Some(EntityRef::Vertexuse(vu)) => (VERTEXUSE_TYPE, f64::from(vu.index())),
                Some(_) => (-4.0, -4.0),
                None => (-1.0, -1.0),
            };
            o.extend([st, si]);
            o.push(e(lu1::loopuse_type(b, lu).map(|t| t as f64))?);
            bool_slot(&mut o, lu1::is_vertex_loopuse(b, lu))?;
            bool_slot(&mut o, lu1::is_edge_loopuse(b, lu))?;
            o.push(e(lu1::shell(b, lu).map(|x| idx(x, ShellId::index)))?);
            bool_slot(&mut o, lu1::is_closed_ptrs(b, lu))?;
            o.push(lu1::TYPE as f64);
            o.extend(KIND_TYPES.map(|t| flag(lu1::is_kind_of(t))));
            put_str(&mut o, lu1::TYPE_STRING);
            put_str(&mut o, lu1::CLASS_STRING);
            put_lists(&mut o, b, lu, None, None)?;
        }
        Resolved::LuLists(lu, use_attr, attributed, conn) => {
            let lu = LoopuseId::from_index(u(*lu));
            let set: Vec<EntityRef> = attributed.iter().filter_map(|&t| entity(t)).collect();
            let filter = |e: EntityRef| set.contains(&e);
            let attr: Option<lu1::AttributeFilter<'_>> =
                if *use_attr { Some(&filter) } else { None };
            put_lists(&mut o, b, lu, attr, entity(*conn))?;
        }
        Resolved::LuConnected(lu, target) => {
            let lu = LoopuseId::from_index(u(*lu));
            bool_slot(&mut o, lu1::is_connected_to(b, lu, entity(*target)))?;
        }
        Resolved::LoopQuery(l) => {
            let l = LoopId::from_index(u(*l));
            o.push(idx(slot(lu1::loop_loopuse(b, l))?.flatten(), LoopuseId::index));
            o.push(match slot(lu1::loop_face(b, l))? {
                Some(f) => idx(f, FaceId::index),
                None => NULL_DEREF,
            });
            match slot(lu1::loop_loopuses(b, l))? {
                Some((a, c)) => o.extend([f64::from(a.index()), idx(c, LoopuseId::index)]),
                None => o.extend([NULL_DEREF, NULL_DEREF]),
            }
            bool_slot(&mut o, lu1::is_outer_loop(b, l))?;
            bool_slot(&mut o, lu1::loop_is_closed_ptrs(b, l))?;
            o.push(lu1::LOOP_TYPE as f64);
            o.push(lu1::loop_class_type() as f64);
            o.extend(KIND_TYPES.map(|t| flag(lu1::loop_is_kind_of(t))));
            put_str(&mut o, lu1::LOOP_TYPE_STRING);
            put_str(&mut o, lu1::LOOP_CLASS_STRING);
            match slot(lu1::loop_vertices(b, l))? {
                Some(v) => {
                    put_list(&mut o, &v.iter().map(|&e| idx(e, VertexId::index)).collect::<Vec<_>>());
                }
                None => o.push(NULL_DEREF),
            }
            match slot(lu1::loop_edges(b, l))? {
                Some(v) => put_list(&mut o, &v.iter().map(|&e| idx(e, EdgeId::index)).collect::<Vec<_>>()),
                None => o.push(NULL_DEREF),
            }
            match slot(lu1::loop_edgeuses(b, l))? {
                Some(v) => put_list(
                    &mut o,
                    &v.iter().map(|e| f64::from(e.index())).collect::<Vec<_>>(),
                ),
                None => o.push(NULL_DEREF),
            }
        }
        Resolved::LoopConnected(l, target) => {
            let l = LoopId::from_index(u(*l));
            bool_slot(&mut o, lu1::loop_is_connected_to(b, l, entity(*target)))?;
        }
        Resolved::Closed3d(of_loop, index, all, want, edge_tol) => {
            if let Some(t) = edge_tol {
                // `m_sZoneTol3d = t` (`SmZoneTol3d::operator=` stores `fabs`)
                for e in 0..b.edges().len() {
                    let e = EdgeId::from_index(u(e as i64));
                    b.edge_mut(e).map_err(|e| fail(&e))?.tolerance = t.abs();
                }
            }
            let r = if *of_loop {
                slot(lu1::loop_is_closed_3d(&k, b, LoopId::from_index(u(*index)), *all))?
            } else {
                slot(lu1::is_closed_3d(&k, b, LoopuseId::from_index(u(*index)), *all).map(Some))?
            };
            match r {
                None => o.push(NULL_DEREF),
                Some(None) => o.extend([1.0, -5.0, -7.0, 777.0]),
                Some(Some(c)) => {
                    o.push(flag(c.closed));
                    o.push(if want[0] { idx(c.edgeuse, EdgeuseId::index) } else { -5.0 });
                    o.push(if want[1] { c.max_gap } else { -7.0 });
                    o.push(if want[2] { c.xsect_tol_3d } else { 777.0 });
                }
            }
        }
        Resolved::MissingSeam(lu) => {
            let lu = LoopuseId::from_index(u(*lu));
            bool_slot(&mut o, lu1::is_missing_seam_at_vertices(&k, b, lu))?;
        }
        Resolved::UvCorners(l, dom, tol) => {
            let l = LoopId::from_index(u(*l));
            let d = solidcore::geom::extent2d::Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
            match slot(lu1::uv_corner_boundaries(&k, b, l, &d, *tol))? {
                None => o.push(NULL_DEREF),
                Some(r) => {
                    o.push(f64::from(r.boundaries));
                    put_list(
                        &mut o,
                        &r.on_edges.iter().map(|&e| idx(e, EdgeId::index)).collect::<Vec<_>>(),
                    );
                    put_list(
                        &mut o,
                        &r.on_boundaries.iter().map(|&x| f64::from(x)).collect::<Vec<_>>(),
                    );
                    put_list(&mut o, &r.on_params);
                }
            }
        }
        Resolved::Flip(l) => {
            let l = LoopId::from_index(u(*l));
            match status(lu1::flip_loop_orientation(b, l))? {
                None => o.push(NULL_DEREF),
                Some(s) => {
                    o.push(s);
                    put_use_state(&mut o, b);
                }
            }
        }
        Resolved::Collect(lu, eu) => {
            let lu = LoopuseId::from_index(u(*lu));
            let eu = opt(*eu, EdgeuseId::from_index);
            let s = status(lu1::collect_edgeuses(b, lu, eu))?.unwrap_or(NULL_DEREF);
            o.push(s);
            put_use_state(&mut o, b);
        }
        Resolved::SetEdgeuse(lu, eu) => {
            let lu = LoopuseId::from_index(u(*lu));
            let eu = opt(*eu, EdgeuseId::from_index);
            let s = status(lu1::set_edgeuse(b, lu, eu))?.unwrap_or(NULL_DEREF);
            o.push(s);
            put_use_state(&mut o, b);
        }
        Resolved::SetOrient(lu, or) => {
            let lu = LoopuseId::from_index(u(*lu));
            slot(lu1::set_orientation(b, lu, orient_of(*or)))?;
            o.push(slot(lu1::orientation(b, lu))?.map_or(NULL_DEREF, orient_code));
            put_use_state(&mut o, b);
        }
        Resolved::SetLoop(lu, l) => {
            let lu = LoopuseId::from_index(u(*lu));
            slot(lu1::set_loop(b, lu, opt(*l, LoopId::from_index)))?;
            o.push(idx(slot(lu1::loop_(b, lu))?.flatten(), LoopId::index));
            put_use_state(&mut o, b);
        }
    }
    Ok(o)
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`, `SmBSplineCurve_TYPE`, `SmLine_TYPE`.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// The deferral of a case whose edge curves (`IsClosed3d`) or face surface
/// (`IsMissingSeamAtVertices`) the import holds only as their B-spline part (as unit
/// `topology.edgeuse_p2`).
fn inexact_geometry(s: &Setup) -> Option<Output> {
    let b = &s.brep;
    let lu = match s.op {
        Resolved::Closed3d(true, l, ..) => lu1::loop_loopuse(b, LoopId::from_index(u(l))).ok()??,
        Resolved::Closed3d(false, lu, ..) | Resolved::MissingSeam(lu) => {
            LoopuseId::from_index(u(lu))
        }
        _ => return None,
    };
    if let Resolved::MissingSeam(_) = s.op {
        let f = lu1::faceuse(b, lu)
            .ok()
            .flatten()
            .and_then(|fu| b.faceuse(fu).ok().and_then(|r| r.face))?;
        let t = s.types.inexact_surface(f.index() as usize).unwrap_or(-1);
        if t >= 0 && !EXACT_SURFACES.contains(&t) {
            let (unit, symbol) = match t {
                // the DISPATCH GAP of `Surface::read_from_db` (issue #76) belongs to
                // `surface.surface_p2`, not to the analytic surface units (all green)
                15012 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone dispatch)"),
                15013 => ("surface.surface_p2", "SurfaceKind::Cone (SmCylinder dispatch)"),
                15014 => ("surface.surface_p2", "SurfaceKind::Sphere (SmSphere dispatch)"),
                15015 => ("surface.surface_p2", "SurfaceKind::Torus (SmTorus dispatch)"),
                15030 => (
                "harness:analytic-geometry-export",
                "SmSurfOfExtrusion face surface not exported exactly",
            ),
                _ => ("topology.brep_p10", "SmBrep::MakeTopologyFromData"),
            };
            return Some(Output::deferred(unit, symbol));
        }
        return None;
    }
    let eus = lu1::edgeuses(b, lu, None, None).ok()?;
    for eu in eus {
        let Some(e) = b.edgeuse(eu).ok().and_then(|r| r.edge) else {
            continue;
        };
        let t = s.types.inexact_curve(e.index() as usize).unwrap_or(-1);
        if t >= 0 && !EXACT_CURVES.contains(&t) {
            return Some(Output::deferred(
                "topology.brep_p10",
                "SmBrep::MakeTopologyFromData",
            ));
        }
    }
    None
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut s = match setup(i, true)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    if let Some(o) = inexact_geometry(&s) {
        return Ok(o);
    }
    match run_rust_op(&mut s.brep, &s.op) {
        Ok(body) => Ok(output_of(&s.op, &body)),
        Err(Fail::Unported(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -------------------------------------------------------------------------

fn gen_sel(rng: &mut Rng) -> i64 {
    if rng.chance(0.03) {
        -1
    } else {
        rng.below(1 << 20) as i64
    }
}

fn gen_target(rng: &mut Rng) -> TargetIn {
    let ty = if rng.chance(0.05) {
        *rng.choose(&[-1, 0, 16001, 16011]).unwrap_or(&-1)
    } else {
        *rng.choose(&KINDS).unwrap_or(&16004)
    };
    (ty, gen_sel(rng))
}

fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(26) {
        0..=2 => OpIn::LuQuery { lu: gen_sel(rng) },
        3..=5 => {
            let n = rng.below(12) as usize;
            OpIn::LuLists {
                lu: gen_sel(rng),
                use_attr: rng.chance(0.8),
                attributed: (0..n)
                    .map(|_| {
                        let ty = *rng.choose(&[16016, 16005, 16006, 16021, 16004]).unwrap_or(&16005);
                        (ty, gen_sel(rng))
                    })
                    .collect(),
                conn: rng.chance(0.4).then(|| gen_target(rng)),
            }
        }
        6..=8 => OpIn::LuConnected {
            lu: gen_sel(rng),
            target: gen_target(rng),
        },
        9..=10 => OpIn::LoopQuery { l: gen_sel(rng) },
        11..=13 => OpIn::LoopConnected {
            l: gen_sel(rng),
            target: gen_target(rng),
        },
        14..=16 => OpIn::Closed3d {
            of_loop: rng.chance(0.5),
            index: gen_sel(rng),
            check_all: rng.chance(0.5),
            want: [rng.chance(0.8), rng.chance(0.8), rng.chance(0.8)],
            edge_tol: None,
        },
        17 => OpIn::MissingSeam { lu: gen_sel(rng) },
        18 => {
            let a = rng.uniform(-0.2, 0.4);
            let c = rng.uniform(0.6, 1.2);
            OpIn::UvCorners {
                l: gen_sel(rng),
                dom: [a, rng.uniform(-0.2, 0.4), c, rng.uniform(0.6, 1.2)],
                tol: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3, 0.1, 1.0]).unwrap_or(&1e-6),
            }
        }
        19..=20 => OpIn::Flip { l: gen_sel(rng) },
        21..=22 => OpIn::Collect {
            lu: gen_sel(rng),
            eu: gen_sel(rng),
        },
        23 => OpIn::SetEdgeuse {
            lu: gen_sel(rng),
            eu: gen_sel(rng),
        },
        24 => OpIn::SetOrient {
            lu: gen_sel(rng),
            orientation: rng.below(3) as u8,
        },
        _ => OpIn::SetLoop {
            lu: gen_sel(rng),
            l: gen_sel(rng),
        },
    }
}

fn gen_body(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.5) {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
            rebuild: rng.chance(0.2),
        }
    } else {
        gen_brep(rng)
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let brep = gen_body(rng);
    let mut i = Inputs {
        brep,
        validate_only: rng.chance(0.1),
        init_gaps: rng.chance(0.2),
        op: gen_op(rng),
    };
    // A B-rep without the loopuse or loop the op selects compares only the same refusal
    // on both sides: draw another. One that doesn't build, or no draw with the kind, keeps
    // the case as it is, so its error stays visible.
    for _ in 0..BREP_DRAWS {
        if !lacks_kind(&i) {
            break;
        }
        i.brep = gen_body(rng);
    }
    i
}

/// Whether `op` selects a loop (else a loopuse), as [`resolve`] requires one.
fn selects_loop(op: &OpIn) -> bool {
    match op {
        OpIn::LoopQuery { .. } | OpIn::LoopConnected { .. } | OpIn::UvCorners { .. } | OpIn::Flip { .. } => true,
        OpIn::Closed3d { of_loop, .. } => *of_loop,
        _ => false,
    }
}

/// Whether `i`'s B-rep builds but has none of the loops or loopuses its op selects.
fn lacks_kind(i: &Inputs) -> bool {
    let counts = path_of(Some(&i.brep)).ok().zip(brep_block_of(Some(&i.brep), i.validate_only, i.init_gaps).ok());
    let Some(c) = counts.and_then(|(path, block)| graph_of(&path, &block).ok()).and_then(|g| ffi1::counts(&g.0)) else {
        return false;
    };
    let [_, _, _, _, n_lu, n_l, ..] = c;
    if selects_loop(&i.op) { n_l == 0 } else { n_lu == 0 }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.loopuse"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if super::topology_face_p4::level0::is_level0(case) {
            return super::topology_face_p4::level0::run_cpp(case);
        }
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if super::topology_face_p4::level0::is_level0(case) {
            return super::topology_face_p4::level0::run_rust(case);
        }
        run_rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = crate::runner::default_cases(self, ctx)?;
        let mut rng = Rng::new(0x100B_05E0);
        for i in 0..EXTRA_GENERATED {
            let inputs =
                serde_json::to_value(gen_inputs(&mut rng)).map_err(|e| harness(e.to_string()))?;
            v.push(Case {
                unit: self.id().to_string(),
                name: format!("extra{i:05}"),
                source: crate::CaseSource::Generated { seed: 0x100B_05E0 },
                inputs,
            });
        }
        {
            use super::topology_face_p4::level0::Level0Op;
            let ops: Vec<Level0Op> = vec![
            Level0Op::LoopuseSide { periodic_u: -1, periodic_v: -1 },
            Level0Op::LoopuseSide { periodic_u: 1, periodic_v: 0 },
            Level0Op::LoopuseSide { periodic_u: 0, periodic_v: 1 },
        ];
            v.extend(super::topology_face_p4::level0::cases(self.id(), &ops, 0x1419_0102, 36)?);
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `selects_loop` names the kind `resolve` refuses an op without, for every op drawn.
    #[test]
    fn selects_loop_matches_resolve() {
        let mut rng = Rng::new(7);
        let b = Brep::new();
        // Counts in the export's order; the loopuses (slot 4) or the loops (slot 5) empty.
        let loopuses_only = [1, 1, 2, 2, 3, 0, 6, 3, 6, 3];
        let loops_only = [1, 1, 2, 2, 0, 3, 6, 3, 6, 3];
        for _ in 0..2000 {
            let op = gen_op(&mut rng);
            assert_eq!(resolve(&op, loopuses_only, &b).is_none(), selects_loop(&op), "{op:?}");
            assert_eq!(resolve(&op, loops_only, &b).is_none(), !selects_loop(&op), "{op:?}");
        }
    }

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let i = gen_inputs(&mut rng);
            let v = serde_json::to_value(&i).unwrap();
            let back: Inputs = serde_json::from_value(v).unwrap();
            assert_eq!(
                serde_json::to_value(&back).unwrap(),
                serde_json::to_value(&i).unwrap()
            );
        }
    }
}
