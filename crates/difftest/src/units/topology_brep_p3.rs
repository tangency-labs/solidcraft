//! Runner for unit `topology.brep_p3`. C++: `solidcore_ffi::units::topology_brep_p3`.
//! Rust: `solidcore::topology::brep::p3`.
//!
//! The C++ side builds a real `SmBrep` per call with the shared topology fixture (an
//! upstream `.smb` file, a wire edge, a one-face B-rep or the `my_test_topology` wire
//! network), as the `topology.brep_p2` shim builds it, and runs one member of the unit
//! (the constructors and `SmApiBrepCopy` too) on its real geometry, analytic surfaces
//! and curves included. The Rust side builds a wire edge, face or network source itself
//! ([`crate::own_builder`] under the `topology.brep_p2` shim's conventions) and reads an
//! `.smb` file with the Rust reader, renumbered the same way; the B-rep tolerance,
//! infinite region, region list head, editing flag, each vertex's vertex use order, the
//! geometry owners and each face's upward face use are the Rust build's own. No C++
//! export reaches the Rust side. It runs the port and writes the same output layout
//! (`out`).
//!
//! Entity selectors are taken modulo the matching entity count (`-1` selects null);
//! an op that needs an entity the B-rep lacks is skipped on both sides (`no target`).
//! `Merge` / `CopyVertices` target a second build of the same B-rep (a second build
//! on the Rust side), whose objects the Rust side notifies under identities of their
//! own ([`source_key_of`]). `DeleteFace` always passes a deleted list
//! (`pOptDeletedEdgesVertices`); the header default `NULL` only skips its `Add`s and
//! is not compared. `CopyVertices` also appends the source and existing destination
//! vertex coordinates before/after copying, then the returned vertices' destination
//! walk indices and coordinates in returned order.
//!
//! `Orient` outputs three values ([`split_orient`]): `out` (status, flag, face domains
//! and upward face uses, counts), `swapped` (each face's `SmFace::SwapUV` `POST_EDIT`
//! count, from that side's own tracker events) and `tracker` (the tracker's lists and
//! events), all compared. A swap's surface edit is in `tracker`: upstream the surface's
//! own `SwapUV` notifies `SM_NO_PRE_EDIT` / `SM_NO_POST_EDIT` on the surface
//! (`SmBSplineSurface::SwapUV` through `SetCanonical`, `SmBSplineSurface.cpp:4585`/`:4592`;
//! `SmOffsetSurface` through its base surface; `SmSrfInVolume` itself), which the
//! tracker records and turns into a `Chg` of the owning face. The Rust side issues each
//! pre-op face surface its identity on the tracked context with the face as its owner,
//! and the port's `SwapUV` (`face::p1::swap_uv_with_notifications`) sends the surface's
//! notifications under it; a surface is other geometry (`-30 n`) on both sides.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve as SmCurve, CurveKind};
use solidcore::error::SM_ERR;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::context::Context;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::surface::Surface as SmSurface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p3 as bp3;
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::shell;
use solidcore::topology::topology::TrackTopologyChanges;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    ArenaId, Brep, BrepId, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, LoopId, LoopuseId,
    LoopuseStart, RegionId, ShellId, VertexId, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_brep_p2 as ffi2;
use solidcore_ffi::units::topology_brep_p3::{self as ffi, Op};
use solidcore_ffi::units::topology_vertexuse::counts;

use super::topology_vertexuse::{
    gen_brep, gen_curve, path_of, push_curve, push_surface, BrepIn, CrvIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    /// `SmBrep(mse, lsr)` (no fixture B-rep is used).
    Construct {
        mse: f64,
        lsr: f64,
    },
    /// The copy constructor (`api`: `SmApiBrepCopy`). With `plain` (which implies
    /// `api`) the Rust copy is `brep_copy`, the copy without the caller's namings or
    /// notification environment (#2010).
    Copy {
        analytics: bool,
        api: bool,
        #[serde(default)]
        plain: bool,
    },
    Merge {
        analytics: bool,
    },
    CopyVertices {
        vertices: Vec<i64>,
    },
    DeleteFace {
        face: i64,
        delete_ev: bool,
        nesting: bool,
        keep: i64,
    },
    /// `which`: `0` `MakeWireEdge`, `1` `MakeWireEdgeVertex`; `orient` an
    /// `SmOrientType` code.
    Wire {
        which: u8,
        region: i64,
        v1: i64,
        v2: i64,
        curve: CrvIn,
        t0: f64,
        t1: f64,
        orient: u8,
        end: [f64; 3],
    },
    /// `faces`: `None` for no list.
    Orient {
        outward: bool,
        unify: bool,
        faces: Option<Vec<i64>>,
    },
    /// `which`: `0` the edge's `GetVertex`, `1` the other vertex, `2` vertex `v`.
    Squeeze {
        edge: i64,
        which: u8,
        v: i64,
    },
    TurnNurbs,
    /// `TurnNurbs` after wrapping every face-owned B-spline kind surface in an owning
    /// `SmOffsetSurface` of it by `offset`.
    TurnNurbsOffset {
        offset: f64,
    },
    ApiCopyNull,
    /// Every face and edge tagged with its own long attribute (behaviour `behavior`, an
    /// `SmAttributeBehaviorType` value), then `SmApiBrepCopy`; also compares the
    /// attributes of the copy's and the source's faces and edges (issue #1462). With
    /// `shared_naming` the Rust copy names its source by the copy's own naming
    /// ([`key_of`], not [`source_key_of`]), the B-rep's own naming a caller reaches for
    /// (the C++ side has no names). With `plain` the Rust copy is `brep_copy`
    /// (#2010), without the caller's namings or notification environment.
    AttributedCopy {
        #[serde(default)]
        behavior: u8,
        #[serde(default)]
        shared_naming: bool,
        #[serde(default)]
        plain: bool,
    },
    /// Every face and edge tagged as for `AttributedCopy`, then `MergeBrep` of the B-rep into
    /// a second build of it; compares the second build's and the source's attributes (does
    /// the merge carry the merged B-rep's attributes?).
    AttributedMerge {
        #[serde(default = "yes")]
        analytics: bool,
        #[serde(default)]
        behavior: u8,
    },
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default = "network")]
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    /// `-1` keep the built B-rep's `m_bEditingEnabled`, else set it.
    #[serde(default = "keep")]
    editing: i64,
    #[serde(flatten)]
    op: OpIn,
}

const fn network() -> BrepIn {
    BrepIn::Network
}

const fn keep() -> i64 {
    -1
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn head(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match &i.brep {
        BrepIn::File { rebuild, .. } => v.extend([0.0, flag(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Network => v.push(3.0),
    }
    v.push(flag(i.validate_only));
    v.push(flag(i.init_gaps));
    v.push(i.editing as f64);
    Ok(v)
}

/// Selector `s` modulo `n`; `None` for a negative selector or an empty kind.
fn pick(s: i64, n: usize) -> Option<usize> {
    if s < 0 || n == 0 {
        return None;
    }
    Some((s as u64 % n as u64) as usize)
}

fn idx(x: Option<usize>) -> f64 {
    x.map_or(-1.0, |i| i as f64)
}

/// The op's arguments after the head, with selectors resolved against the counts;
/// `None` when the op needs an entity the B-rep lacks.
fn op_args(op: &OpIn, c: &[usize; 10]) -> Result<Option<(Op, Vec<f64>)>, HarnessError> {
    let [n_r, _, _, n_f, _, _, _, n_e, _, n_v] = *c;
    Ok(Some(match op {
        OpIn::Construct { mse, lsr } => (Op::Construct, vec![*mse, *lsr]),
        OpIn::ApiCopyNull => (Op::ApiCopyNull, vec![0.0]),
        OpIn::TurnNurbs => (Op::TurnNurbs, vec![]),
        OpIn::TurnNurbsOffset { offset } => (Op::TurnNurbsOffset, vec![*offset]),
        OpIn::Copy {
            analytics,
            api,
            plain,
        } => (Op::Copy, vec![flag(*analytics), flag(*api || *plain)]),
        OpIn::Merge { analytics } => (Op::Merge, vec![flag(*analytics)]),
        OpIn::AttributedMerge {
            analytics,
            behavior,
        } => (
            Op::AttributedMerge,
            vec![flag(*analytics), f64::from(*behavior)],
        ),
        OpIn::AttributedCopy { behavior, .. } => {
            (Op::AttributedCopy, vec![f64::from(*behavior)])
        }
        OpIn::CopyVertices { vertices } => {
            let mut v = vec![vertices.len() as f64];
            for &s in vertices {
                let Some(x) = pick(s, n_v) else {
                    return Ok(None);
                };
                v.push(x as f64);
            }
            (Op::CopyVertices, v)
        }
        OpIn::DeleteFace {
            face,
            delete_ev,
            nesting,
            keep,
        } => {
            let Some(f) = pick(*face, n_f) else {
                return Ok(None);
            };
            (
                Op::DeleteFace,
                vec![
                    f as f64,
                    flag(*delete_ev),
                    flag(*nesting),
                    idx(pick(*keep, n_r)),
                ],
            )
        }
        OpIn::Wire {
            which,
            region,
            v1,
            v2,
            curve,
            t0,
            t1,
            orient,
            end,
        } => {
            let (Some(a), b) = (pick(*v1, n_v), pick(*v2, n_v)) else {
                return Ok(None);
            };
            if *which == 0 && b.is_none() {
                return Ok(None);
            }
            let mut v = vec![f64::from(*which), idx(pick(*region, n_r)), a as f64, idx(b)];
            push_curve(&mut v, curve)?;
            v.extend([*t0, *t1, f64::from(*orient)]);
            v.extend(end);
            (Op::Wire, v)
        }
        OpIn::Orient {
            outward,
            unify,
            faces,
        } => {
            let mut v = vec![flag(*outward), flag(*unify)];
            match faces {
                None => v.push(-1.0),
                Some(l) => {
                    v.push(l.len() as f64);
                    for &s in l {
                        let Some(x) = pick(s, n_f) else {
                            return Ok(None);
                        };
                        v.push(x as f64);
                    }
                }
            }
            (Op::Orient, v)
        }
        OpIn::Squeeze { edge, which, v } => {
            let Some(e) = pick(*edge, n_e) else {
                return Ok(None);
            };
            (
                Op::Squeeze,
                vec![e as f64, f64::from(*which), idx(pick(*v, n_v))],
            )
        }
    }))
}

fn ffi_err(e: &solidcore_ffi::FfiError) -> HarnessError {
    harness(e.to_string())
}

/// Whether the op runs without a fixture B-rep.
const fn standalone(op: &OpIn) -> bool {
    matches!(op, OpIn::Construct { .. } | OpIn::ApiCopyNull)
}

/// `(path, head, counts)` of a built B-rep, or the build failure's output.
type Prepared = Result<(String, Vec<f64>, [usize; 10]), Output>;

fn prepare(i: &Inputs) -> Result<Prepared, HarnessError> {
    let path = path_of(&i.brep)?;
    let h = head(i)?;
    let export = ffi2::eval(ffi2::Op::Export, &path, &h).map_err(|e| ffi_err(&e))?;
    match counts(&export) {
        Some(c) => Ok(Ok((path, h, c))),
        None => Ok(Err(Output::new().values("out", export))),
    }
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if standalone(&i.op) {
        let Some((op, args)) = op_args(&i.op, &[0; 10])? else {
            return Ok(Output::err("no target"));
        };
        let out = ffi::eval(op, "", &args).map_err(|e| ffi_err(&e))?;
        return Ok(Output::new().values("out", out));
    }
    let (path, mut input, c) = match prepare(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some((op, args)) = op_args(&i.op, &c)? else {
        return Ok(Output::err("no target"));
    };
    input.extend(args);
    let out = ffi::eval(op, &path, &input).map_err(|e| ffi_err(&e))?;
    if matches!(i.op, OpIn::Orient { .. }) {
        return Ok(split_orient(out));
    }
    Ok(Output::new().values("out", out))
}

// ---- Rust side ----------------------------------------------------------------------------

/// The identity each entity is notified and attributed under: its type code and
/// `(kind << 32) | (1 + index)`, so records of different kinds never share a key.
pub(super) fn key_of(e: EntityRef) -> ObjectRef {
    let (kind, type_code, i) = match e {
        EntityRef::Region(x) => (1u64, 16012, x.index()),
        EntityRef::Shell(x) => (2, 16013, x.index()),
        EntityRef::Faceuse(x) => (3, 16022, x.index()),
        EntityRef::Face(x) => (4, 16002, x.index()),
        EntityRef::Loopuse(x) => (5, 16004, x.index()),
        EntityRef::Loop(x) => (6, 16003, x.index()),
        EntityRef::Edgeuse(x) => (7, 16005, x.index()),
        EntityRef::Edge(x) => (8, 16016, x.index()),
        EntityRef::Vertexuse(x) => (9, 16006, x.index()),
        EntityRef::Vertex(x) => (10, 16021, x.index()),
        EntityRef::Brep(_) => (11, 16999, 0),
    };
    ObjectRef {
        type_code,
        key: ObjectKey((kind << 32) | (1 + u64::from(i))),
    }
}

/// The entity a [`key_of`] key names (`None` for another key).
fn entity_of_key(key: ObjectKey) -> Option<EntityRef> {
    let i = u32::try_from((key.0 & 0xffff_ffff).checked_sub(1)?).ok()?;
    Some(match key.0 >> 32 {
        1 => EntityRef::Region(RegionId::from_index(i)),
        2 => EntityRef::Shell(ShellId::from_index(i)),
        3 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        4 => EntityRef::Face(FaceId::from_index(i)),
        5 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        6 => EntityRef::Loop(LoopId::from_index(i)),
        7 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        8 => EntityRef::Edge(EdgeId::from_index(i)),
        9 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        10 => EntityRef::Vertex(VertexId::from_index(i)),
        _ => return None,
    })
}

/// The identity of the source B-rep's entities in a copy or merge into another B-rep
/// (its ids overlap the destination's, which are notified under [`key_of`]).
pub(super) fn source_key_of(e: EntityRef) -> ObjectRef {
    let r = key_of(e);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(r.key.0 | (1 << 40)),
    }
}

/// The shim's build conventions (`sc_topology_fixture.h` as the `topology.brep_p2`
/// shim uses it: the face, edge and vertex lists registered before the traversal,
/// `GetVertexEdgeGap` in the `init_gaps` walk), with the geometry owned under the arena
/// identities.
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    prefix_lists: true,
    vertex_edge_gaps: true,
    vertex_lists_last: false,
    ..crate::own_builder::Conventions::EDGEUSE_GRAPH
};

/// The Rust B-rep of the case and its entity counts, or the build failure's output:
/// shim-built sources are built by the Rust kernel ([`crate::own_builder`]) and `.smb`
/// files read by the Rust reader ([`crate::own_reader::file_in_order`]), both in the
/// shim's traversal order under [`CONVENTIONS`]; the shim's `editing` flag is applied
/// after. No C++ export reaches the Rust side.
fn own_brep(
    k: &mut Kernel,
    i: &Inputs,
) -> Result<Result<(Brep, [usize; 10]), Output>, HarnessError> {
    use crate::own_builder::{build_as, build_status, counts, Built};
    let built = if let Some(b) = build_as(k, &i.brep, i.validate_only, i.init_gaps, &CONVENTIONS)? {
        b
    } else {
        let BrepIn::File { rebuild, .. } = &i.brep else {
            return Err(harness("own builder: a non-file source was not built"));
        };
        let path = path_of(&i.brep)?;
        let (vo, gaps) = (i.validate_only, i.init_gaps);
        crate::own_reader::file_in_order(k, &path, *rebuild, vo, gaps, &CONVENTIONS)?
    };
    match built {
        Built::Brep(b) => {
            let mut b = *b;
            if i.editing >= 0 {
                b.set_editing_enabled(i.editing != 0);
            }
            let c = counts(&b);
            Ok(Ok((b, c)))
        }
        Built::Failed(e) => match Output::unported(&e) {
            Some(o) => Ok(Err(o)),
            None => Ok(Err(Output::new().values("out", vec![-1.0, build_status(&e)]))),
        },
    }
}

fn rust_curve(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let nl = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(BSplineCurve::from_nurb(nl, c.dim, false))
}

/// What a failed Rust op reports: the upstream status, or the deferral.
pub(crate) enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

/// The status slot of a Rust result (`0` success, else the upstream code).
fn status<T>(r: Result<T, SolidError>) -> Result<(f64, Option<T>), Fail> {
    match r {
        Ok(v) => Ok((0.0, Some(v))),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => match e {
                SolidError::Upstream { code, .. } => Ok((f64::from(code), None)),
                other => Err(Fail::Harness(harness(format!("rust: {other}")))),
            },
        },
    }
}

fn must<T>(r: Result<T, SolidError>) -> Result<T, Fail> {
    match status(r)? {
        (_, Some(v)) => Ok(v),
        (s, None) => Err(Fail::Harness(harness(format!("rust: status {s}")))),
    }
}

/// `faces edges vertices regions` of the B-rep's lists.
/// An arena id's index (`None` for null).
fn ix<I: ArenaId>(x: Option<I>) -> Option<u32> {
    x.map(ArenaId::index)
}

/// `SmUnknown_TYPE`, the type a use without an owner kind reports.
const UNKNOWN_TYPE: i64 = 16999;

/// The shim's `Graph` over the Rust arena: every object reached by [`walk`], per kind
/// (`0` regions, `1` shells, `2` face uses, `3` faces, `4` loop uses, `5` loops, `6` edge
/// uses, `7` edges, `8` vertex uses, `9` vertices), in first-seen order.
#[derive(Default)]
pub(crate) struct Walk {
    lists: [Vec<u32>; 10],
    index: [BTreeMap<u32, i64>; 10],
}

impl Walk {
    fn add(&mut self, k: usize, id: Option<u32>) {
        let (Some(id), Some(list), Some(index)) =
            (id, self.lists.get_mut(k), self.index.get_mut(k))
        else {
            return;
        };
        if let std::collections::btree_map::Entry::Vacant(slot) = index.entry(id) {
            slot.insert(list.len() as i64);
            list.push(id);
        }
    }

    /// The index of a link (`-1` null, `-3` an object the walk did not reach).
    pub(crate) fn find(&self, k: usize, id: Option<u32>) -> f64 {
        match id {
            None => -1.0,
            Some(id) => self
                .index
                .get(k)
                .and_then(|m| m.get(&id))
                .map_or(-3.0, |&i| i as f64),
        }
    }

    pub(crate) fn len(&self, k: usize) -> usize {
        self.lists.get(k).map_or(0, Vec::len)
    }

    pub(crate) fn at(&self, k: usize, i: usize) -> Result<u32, Fail> {
        self.lists
            .get(k)
            .and_then(|l| l.get(i))
            .copied()
            .ok_or_else(|| Fail::Harness(harness("walk index out of range")))
    }
}

/// `list_of(shell->GetList())` of a face-use shell: its ring from the head through the
/// face uses' `GetNext` links (bounded as the shim's).
fn faceuse_ring(b: &Brep, s: ShellId) -> Result<Vec<FaceuseId>, Fail> {
    let mut out = Vec::new();
    let shell::ShellMembers::Faceuses(members) = &must(b.shell(s))?.members else {
        return Ok(out);
    };
    let Some(&head) = members.first() else {
        return Ok(out);
    };
    let mut p = Some(head);
    while let Some(x) = p {
        if out.len() >= 100_000 {
            break;
        }
        out.push(x);
        p = must(shell::faceuse_sibling_links(b, x))?.0;
        if p == Some(head) {
            break;
        }
    }
    Ok(out)
}

/// An edge use's `GetNext` / `GetLast` in its edge's radial list.
fn edgeuse_ring_links(
    b: &Brep,
    eu: EdgeuseId,
) -> Result<(Option<EdgeuseId>, Option<EdgeuseId>), Fail> {
    let Some(e) = must(b.edgeuse(eu))?.edge else {
        return Ok((None, None));
    };
    let list = &must(b.edge(e))?.edgeuses;
    let Some(i) = list.iter().position(|&x| x == eu) else {
        return Ok((None, None));
    };
    let n = list.len();
    Ok((
        list.get((i + 1) % n).copied(),
        list.get((i + n - 1) % n).copied(),
    ))
}

const fn shell_head_kind(code: i64) -> Option<usize> {
    match code {
        16005 => Some(6),
        16006 => Some(8),
        _ => None,
    }
}

fn shell_head(sh: &solidcore::topology::Shell) -> (Option<usize>, Option<u32>) {
    match sh.members {
        shell::ShellMembers::Edgeuse(x) => (Some(6), Some(x.index())),
        shell::ShellMembers::Vertexuse(x) => (Some(8), Some(x.index())),
        shell::ShellMembers::Faceuses(ref l) => (Some(2), ix(l.first().copied())),
        shell::ShellMembers::Empty => (None, None),
    }
}

fn edgeuse_owner(o: EdgeuseOwner) -> (i64, usize, Option<u32>) {
    match o {
        EdgeuseOwner::Loopuse(x) => (16004, 4, ix(x)),
        EdgeuseOwner::Shell(x) => (16013, 1, ix(x)),
        EdgeuseOwner::Unknown => (UNKNOWN_TYPE, 0, None),
    }
}

fn vertexuse_owner(o: VertexuseOwner) -> (i64, usize, Option<u32>) {
    match o {
        VertexuseOwner::Edgeuse(x) => (16005, 6, ix(x)),
        VertexuseOwner::Loopuse(x) => (16004, 4, ix(x)),
        VertexuseOwner::Shell(x) => (16013, 1, ix(x)),
        VertexuseOwner::Unknown => (UNKNOWN_TYPE, 0, None),
    }
}

fn loopuse_start(s: LoopuseStart) -> (i64, usize, Option<u32>) {
    match s {
        LoopuseStart::Edgeuse(x) => (16005, 6, ix(x)),
        LoopuseStart::Vertexuse(x) => (16006, 8, ix(x)),
        LoopuseStart::Unknown => (UNKNOWN_TYPE, 0, None),
    }
}

const fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

/// The fixture's `walk` over the Rust arena, in the same first-seen order.
#[allow(clippy::too_many_lines)]
pub(crate) fn walk(b: &Brep) -> Result<Walk, Fail> {
    let mut g = Walk::default();
    for r in must(bp1::regions(b, None))? {
        g.add(0, Some(r.index()));
    }
    for e in must(bp1::edges(b, None))? {
        g.add(7, Some(e.index()));
    }
    let mut c = [0usize; 10];
    let mut more = true;
    while more {
        more = false;
        while c[0] < g.len(0) {
            let r = RegionId::from_index(g.at(0, c[0])?);
            for &s in &must(b.region(r))?.shells {
                g.add(1, Some(s.index()));
            }
            c[0] += 1;
            more = true;
        }
        while c[1] < g.len(1) {
            let s = ShellId::from_index(g.at(1, c[1])?);
            let sh = must(b.shell(s))?;
            let ty = sh.shell_type.code();
            if ty == 16022 {
                for fu in faceuse_ring(b, s)? {
                    g.add(2, Some(fu.index()));
                }
            } else if let Some(k) = shell_head_kind(ty) {
                let (kind, id) = shell_head(sh);
                if id.is_some() && kind != Some(k) {
                    return Err(Fail::Harness(harness("shell head of another kind")));
                }
                g.add(k, id);
            }
            c[1] += 1;
            more = true;
        }
        while c[2] < g.len(2) {
            let fu = must(b.faceuse(FaceuseId::from_index(g.at(2, c[2])?)))?;
            g.add(3, ix(fu.face));
            g.add(2, ix(fu.mate));
            for &lu in &fu.loopuses {
                g.add(4, Some(lu.index()));
            }
            c[2] += 1;
            more = true;
        }
        while c[4] < g.len(4) {
            let lu = must(b.loopuse(LoopuseId::from_index(g.at(4, c[4])?)))?;
            g.add(5, ix(lu.loop_));
            g.add(4, ix(lu.mate));
            let (_, k, start) = loopuse_start(lu.start);
            g.add(k, start);
            // the CCW walk of an edge loop
            if let LoopuseStart::Edgeuse(Some(head)) = lu.start {
                let mut q = Some(head);
                let mut n = 0;
                while let Some(x) = q {
                    if n >= 100_000 {
                        break;
                    }
                    g.add(6, Some(x.index()));
                    q = must(b.edgeuse(x))?.ccw;
                    n += 1;
                    if q == Some(head) {
                        break;
                    }
                }
            }
            c[4] += 1;
            more = true;
        }
        while c[6] < g.len(6) {
            let id = EdgeuseId::from_index(g.at(6, c[6])?);
            let eu = must(b.edgeuse(id))?;
            g.add(7, ix(eu.edge));
            let (_, k, owner) = edgeuse_owner(eu.owner);
            g.add(k, owner);
            g.add(6, ix(eu.cw));
            g.add(6, ix(eu.ccw));
            g.add(8, ix(eu.vertexuse));
            let (next, last) = edgeuse_ring_links(b, id)?;
            g.add(6, ix(next));
            g.add(6, ix(last));
            c[6] += 1;
            more = true;
        }
        while c[7] < g.len(7) {
            let e = EdgeId::from_index(g.at(7, c[7])?);
            for &x in &must(b.edge(e))?.edgeuses {
                g.add(6, Some(x.index()));
            }
            c[7] += 1;
            more = true;
        }
        while c[8] < g.len(8) {
            let vu = must(b.vertexuse(VertexuseId::from_index(g.at(8, c[8])?)))?;
            g.add(9, ix(vu.vertex));
            let (_, k, owner) = vertexuse_owner(vu.owner);
            g.add(k, owner);
            c[8] += 1;
            more = true;
        }
        while c[9] < g.len(9) {
            let v = VertexId::from_index(g.at(9, c[9])?);
            for &x in &must(b.vertex(v))?.vertexuses {
                g.add(8, Some(x.index()));
            }
            c[9] += 1;
            more = true;
        }
        while c[5] < g.len(5) {
            let l = must(b.loop_(LoopId::from_index(g.at(5, c[5])?)))?;
            g.add(4, ix(l.loopuse));
            c[5] += 1;
            more = true;
        }
    }
    Ok(g)
}

/// The shim's `put_graph`: the topology graph after the op, every owner / adjacency
/// link as an index in [`walk`]'s order (layout in the shim).
#[allow(clippy::too_many_lines)]
fn put_graph(o: &mut Vec<f64>, b: &Brep) -> Result<(), Fail> {
    let g = walk(b)?;
    for k in 0..10 {
        o.push(g.len(k) as f64);
    }
    for i in 0..g.len(0) {
        let r = RegionId::from_index(g.at(0, i)?);
        let region = must(b.region(r))?;
        o.push(f64::from(region.is_void()));
        o.push(flag(b.infinite_region() == Some(r)));
        o.push(region.shells.len() as f64);
        for &s in &region.shells {
            o.push(g.find(1, Some(s.index())));
        }
    }
    for i in 0..g.len(1) {
        let s = ShellId::from_index(g.at(1, i)?);
        let sh = must(b.shell(s))?;
        let ty = sh.shell_type.code();
        o.push(ty as f64);
        o.push(must(shell::shell_list_size(b, s))? as f64);
        o.push(g.find(0, ix(sh.region)));
        // T(ty, head): the head read as the type's kind
        let (kind, head) = shell_head(sh);
        let want = match ty {
            16022 => Some(2),
            t => shell_head_kind(t),
        };
        o.push(match (want, head) {
            (_, None) => -1.0,
            (Some(k), Some(_)) if kind == Some(k) => g.find(k, head),
            (Some(_), Some(_)) => return Err(Fail::Harness(harness("shell head of another kind"))),
            (None, Some(_)) => -4.0,
        });
    }
    for i in 0..g.len(2) {
        let id = FaceuseId::from_index(g.at(2, i)?);
        let fu = must(b.faceuse(id))?;
        let (next, last) = must(shell::faceuse_sibling_links(b, id))?;
        o.push(g.find(3, ix(fu.face)));
        o.push(g.find(2, ix(fu.mate)));
        o.push(g.find(1, ix(fu.shell)));
        o.push(g.find(2, ix(next)));
        o.push(g.find(2, ix(last)));
        o.push(orient_code(fu.orientation));
        o.push(fu.loopuses.len() as f64);
        for &lu in &fu.loopuses {
            o.push(g.find(4, Some(lu.index())));
        }
    }
    for i in 0..g.len(3) {
        let f = must(b.face(FaceId::from_index(g.at(3, i)?)))?;
        o.push(g.find(2, ix(f.faceuse)));
    }
    for i in 0..g.len(4) {
        let lu = must(b.loopuse(LoopuseId::from_index(g.at(4, i)?)))?;
        let (ty, k, start) = loopuse_start(lu.start);
        o.push(g.find(5, ix(lu.loop_)));
        o.push(g.find(4, ix(lu.mate)));
        o.push(g.find(2, ix(lu.faceuse)));
        o.push(ty as f64);
        o.push(if ty == UNKNOWN_TYPE && start.is_none() {
            -1.0
        } else {
            g.find(k, start)
        });
        o.push(orient_code(lu.orientation));
    }
    for i in 0..g.len(5) {
        let l = must(b.loop_(LoopId::from_index(g.at(5, i)?)))?;
        o.push(g.find(4, ix(l.loopuse)));
    }
    for i in 0..g.len(6) {
        let id = EdgeuseId::from_index(g.at(6, i)?);
        let eu = must(b.edgeuse(id))?;
        let (ty, k, owner) = edgeuse_owner(eu.owner);
        let (next, last) = edgeuse_ring_links(b, id)?;
        o.push(g.find(7, ix(eu.edge)));
        o.push(ty as f64);
        o.push(if ty == UNKNOWN_TYPE && owner.is_none() {
            -1.0
        } else {
            g.find(k, owner)
        });
        o.push(g.find(6, ix(eu.cw)));
        o.push(g.find(6, ix(eu.ccw)));
        o.push(g.find(8, ix(eu.vertexuse)));
        o.push(g.find(6, ix(next)));
        o.push(g.find(6, ix(last)));
        o.push(orient_code(eu.orientation));
    }
    for i in 0..g.len(7) {
        let e = must(b.edge(EdgeId::from_index(g.at(7, i)?)))?;
        o.push(e.edgeuses.len() as f64);
        for &x in &e.edgeuses {
            o.push(g.find(6, Some(x.index())));
        }
    }
    for i in 0..g.len(8) {
        let vu = must(b.vertexuse(VertexuseId::from_index(g.at(8, i)?)))?;
        let (ty, k, owner) = vertexuse_owner(vu.owner);
        o.push(g.find(9, ix(vu.vertex)));
        o.push(ty as f64);
        o.push(if ty == UNKNOWN_TYPE && owner.is_none() {
            -1.0
        } else {
            g.find(k, owner)
        });
    }
    for i in 0..g.len(9) {
        let v = must(b.vertex(VertexId::from_index(g.at(9, i)?)))?;
        o.push(v.vertexuses.len() as f64);
        for &x in &v.vertexuses {
            o.push(g.find(8, Some(x.index())));
        }
    }
    Ok(())
}

pub(crate) fn put_counts(o: &mut Vec<f64>, b: &Brep) -> Result<(), Fail> {
    o.push(must(bp1::faces(b, None))?.len() as f64);
    o.push(must(bp1::edges(b, None))?.len() as f64);
    o.push(must(bp1::vertices(b, None))?.len() as f64);
    let regions = must(bp1::regions(b, None))?;
    o.push(regions.len() as f64);
    // the shim's shell digest: per region `void infinite nshells`, per shell
    // `type size has_head`
    for r in regions {
        let region = must(b.region(r))?;
        let shells = must(region.get_shells(None, |_, _| Ok(true)))?;
        o.push(f64::from(region.is_void()));
        o.push(flag(b.infinite_region() == Some(r)));
        o.push(shells.len() as f64);
        for s in shells {
            let sh = must(b.shell(s))?;
            o.push(sh.shell_type.code() as f64);
            o.push(must(shell::shell_list_size(b, s))? as f64);
            o.push(flag(sh.members != shell::ShellMembers::Empty));
        }
    }
    put_graph(o, b)
}

/// The fixture's `put_curve` layout of an edge curve.
fn put_curve(o: &mut Vec<f64>, c: Option<&SmCurve>) -> Result<(), Fail> {
    let Some(c) = c else {
        o.push(-1.0);
        return Ok(());
    };
    let base = match c.kind() {
        CurveKind::BSpline(x) => x,
        CurveKind::Line(x) => x.base(),
        CurveKind::Ellipse(x) => x.base(),
        CurveKind::Parabola(x) => x.base(),
        CurveKind::Hyperbola(x) => x.base(),
        _ => {
            o.push(-2.0);
            return Ok(());
        }
    };
    o.push(c.type_code() as f64);
    o.push(base.dim() as f64);
    let nurb = base.gw_nurb();
    o.push(flag(nurb.is_some()));
    if let Some(n) = nurb {
        o.extend([n.n() as f64, n.degree() as f64, n.m() as f64]);
        for p in n.cpts().iter().take(n.n() + 1) {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.extend(n.knots().iter().take(n.m() + 1));
    }
    if let CurveKind::Line(_) = c.kind() {
        return Err(Fail::Harness(harness(
            "a line after TurnToNURBS (its extras are not exported)",
        )));
    }
    Ok(())
}

/// `SmRegion::m_bIsVoidFlag` of the new B-rep's infinite region (`SmRegion::IsVoid`,
/// unit `topology.shell`).
fn region_void_flag(b: &Brep, r: Option<RegionId>) -> Result<f64, SolidError> {
    let r =
        r.ok_or_else(|| SolidError::null_pointer("difftest::topology_brep_p3::region_void_flag"))?;
    Ok(f64::from(b.region(r)?.is_void()))
}

/// The shim's `TurnNurbsOffset` set-up: every face-owned B-spline kind surface is
/// detached from its face (so the constructor's periodicity and singularity tests use
/// the context tolerance, not `SmFace::GetTolerance`) and becomes an
/// `SmOffsetSurface(d, surface, TRUE)` owned by the face.
fn wrap_in_offsets(k: &Kernel, b: &mut Brep, faces: &[FaceId], d: f64) -> Result<(), Fail> {
    for &f in faces {
        let owner = Some(arena_identity(CacheObject::Face(f)));
        let face = must(b.face_mut(f))?;
        let wrap = match face.surface.as_ref() {
            Some(s) if s.owner() == owner => must(s.is_kind_of(BSplineSurface::TYPE))?,
            _ => false,
        };
        if !wrap {
            continue;
        }
        let Some(mut base) = face.surface.take() else {
            continue;
        };
        base.set_owner(None);
        let off = must(OffsetSurface::new(k, d, base, true, SS_UNKNOWN))?;
        let mut s = SmSurface::from(off);
        s.set_owner(owner);
        face.surface = Some(s);
    }
    Ok(())
}

/// The type codes of the identities whose destructions the recorders count:
/// `SmEdge_TYPE`, `SmEdgeuse_TYPE`, `SmFace_TYPE`, `SmShell_TYPE`.
const DESTROYED_KINDS: [i64; 4] = [16016, 16005, 16002, 16013];

/// Whether `key` is a geometry identity (the kernel context's range,
/// `Context::new_geometry_key`; every [`key_of`] key is below it).
const fn is_geometry_key(key: ObjectKey) -> bool {
    key.0 >= solidcore::geom::context::GEOMETRY_KEY_BASE
}

/// The shim's `GeomRecorder`: a user notify callback that keeps every geometry key a
/// notification names (the shim's tells geometry from topology while it is live), and
/// counts the `SM_NO_DESTRUCTION` notifications by caller kind: edge, edge use, face,
/// shell (`~SmEdge`, `~SmEdgeuse`, `~SmFace`, `~SmShell`).
#[derive(Debug, Default)]
struct GeomRecorder(
    std::sync::Arc<std::sync::Mutex<Vec<ObjectKey>>>,
    std::sync::Arc<std::sync::Mutex<[u64; 4]>>,
);

impl solidcore::geom::context::NotifyCallback for GeomRecorder {
    fn execute(
        &mut self,
        operation: solidcore::geom::object::NotifyOperation,
        caller: ObjectRef,
        data1: Option<ObjectRef>,
        data2: Option<ObjectRef>,
        data3: Option<ObjectRef>,
    ) -> Result<(), SolidError> {
        if operation == solidcore::geom::object::NotifyOperation::Destruction {
            if let Some(i) = DESTROYED_KINDS.iter().position(|&t| t == caller.type_code) {
                let mut destroyed = self
                    .1
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(n) = destroyed.get_mut(i) {
                    *n += 1;
                }
            }
        }
        let mut seen = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for x in [Some(caller), data1, data2, data3].into_iter().flatten() {
            if is_geometry_key(x.key) && !seen.contains(&x.key) {
                seen.push(x.key);
            }
        }
        Ok(())
    }
}

/// What the shim's `Tracked` holds: the arena lengths before the op, the walk after
/// it, the B-rep's key, each pre-op edge curve's key, the geometry the notifications
/// named and that geometry numbered by first appearance in the output.
struct Tracked<'a> {
    pre: [usize; 10],
    post: Walk,
    brep: ObjectKey,
    pre_curves: &'a BTreeMap<ObjectKey, usize>,
    post_curves: BTreeMap<ObjectKey, usize>,
    geometry: &'a [ObjectKey],
    numbered: Vec<ObjectKey>,
}

/// The shim's `put_tracked`, by identity only: `-1 -1` null, `-10 0` the B-rep; a
/// topology object that existed before the op or that the walk after it reaches: its
/// kind's type code, then for a face / edge / vertex its index before the op (or
/// 100000 + its index in the walk after it), for another kind `-5`; an edge's pre-op
/// curve `-20 edge`; else an edge's curve after the op `-21 edge` (its index in the
/// walk after the op); other geometry `-30 n` (by first appearance in the tracker's
/// lists and events); anything else `-2 -5`.
fn put_tracked(o: &mut Vec<f64>, key: Option<ObjectKey>, t: &mut Tracked<'_>) {
    const CODES: [f64; 10] = [
        16012.0, 16013.0, 16022.0, 16002.0, 16004.0, 16003.0, 16005.0, 16016.0, 16006.0, 16021.0,
    ];
    let Some(key) = key else {
        o.extend([-1.0, -1.0]);
        return;
    };
    if key == t.brep {
        o.extend([-10.0, 0.0]);
        return;
    }
    if let Some(&e) = t.pre_curves.get(&key) {
        o.extend([-20.0, e as f64]);
        return;
    }
    if let Some(&e) = t.post_curves.get(&key) {
        o.extend([-21.0, e as f64]);
        return;
    }
    if t.geometry.contains(&key) {
        let n = t
            .numbered
            .iter()
            .position(|&x| x == key)
            .unwrap_or_else(|| {
                t.numbered.push(key);
                t.numbered.len() - 1
            });
        o.extend([-30.0, n as f64]);
        return;
    }
    // key_of: (kind << 32) | (1 + index), kinds 1..=10 in the walk's order
    let kind = key.0 >> 32;
    let idx = (key.0 & 0xffff_ffff).wrapping_sub(1);
    let (Some(k), Ok(idx)) = (
        usize::try_from(kind)
            .ok()
            .and_then(|x| x.checked_sub(1))
            .filter(|&x| x < 10),
        u32::try_from(idx),
    ) else {
        o.extend([-2.0, -5.0]);
        return;
    };
    let existed = (idx as usize) < t.pre.get(k).copied().unwrap_or(0);
    let after = t.post.find(k, Some(idx));
    if !existed && after < 0.0 {
        o.extend([-2.0, -5.0]);
        return;
    }
    o.push(CODES.get(k).copied().unwrap_or(-2.0));
    o.push(if matches!(k, 3 | 7 | 9) {
        if existed {
            f64::from(idx)
        } else {
            100_000.0 + after
        }
    } else {
        -5.0
    });
}

/// The shim's `put_tracker`: the tracker's Rm / Add / Chg lists and its events.
fn put_tracker(
    o: &mut Vec<f64>,
    lists: &solidcore::topology::topology::TopologyChangeLists,
    t: &mut Tracked<'_>,
) {
    for l in [&lists.rm_list, &lists.add_list, &lists.chg_list] {
        o.push(l.len() as f64);
        for &x in l {
            put_tracked(o, Some(x), t);
        }
    }
    o.push(lists.notify_events.len() as f64);
    for e in &lists.notify_events {
        o.push(f64::from(e.operation.code()));
        for x in [e.caller, e.data1, e.data2, e.data3] {
            put_tracked(o, x.map(|x| x.key), t);
        }
    }
}

/// `run_rust_op_in`, the edits of `b` (as the shim's) under a topology change tracker
/// on a context of their own, whose lists and events follow the op's outputs.
fn run_rust_op(
    k: &mut Kernel,
    b: &mut Brep,
    second: Option<&mut Brep>,
    op: &OpIn,
    a: &[f64],
) -> Result<Vec<f64>, Fail> {
    let tracked = matches!(
        op,
        OpIn::DeleteFace { .. }
            | OpIn::Wire { .. }
            | OpIn::Orient { .. }
            | OpIn::Squeeze { .. }
            | OpIn::TurnNurbs
            | OpIn::TurnNurbsOffset { .. }
    );
    if !tracked {
        return run_rust_op_in(k, b, second, op, a, None);
    }
    // the op's notifications go through a context of their own, which issues the
    // curves' identities: each edge's curve before the op (the shim's pre_curves)
    let mut ctx = Context::default();
    let mut pre_curves = BTreeMap::new();
    let mut owners = Vec::new();
    let ids: Vec<EdgeId> = b.edges().ids().collect();
    for e in ids {
        if let Some(c) = must(bp3::edge_curve_identity(&mut ctx, b, e))? {
            pre_curves.insert(c.key, e.index() as usize);
            owners.push((c, key_of(EntityRef::Edge(e))));
        }
    }
    // and each face's surface (upstream SetCanonical notifies the surface itself, its
    // GetOwner() the face; the tracker lists name it as other geometry)
    let faces: Vec<FaceId> = b.faces().ids().collect();
    for f in faces {
        if let Some(s) = must(bp3::face_surface_identity(&mut ctx, b, f))? {
            owners.push((s, key_of(EntityRef::Face(f))));
        }
    }
    // (arena lengths in the walk's kind order: an imported B-rep's ids are its walk
    // indices, and an op only appends)
    let pre = [
        b.regions().len(),
        b.shells().len(),
        b.faceuses().len(),
        b.faces().len(),
        b.loopuses().len(),
        b.loops().len(),
        b.edgeuses().len(),
        b.edges().len(),
        b.vertexuses().len(),
        b.vertices().len(),
    ];
    let brep = solidcore::topology::brep::p2::brep_ref(b).key;
    let recorder = GeomRecorder::default();
    let seen = std::sync::Arc::clone(&recorder.0);
    let edges_destroyed = std::sync::Arc::clone(&recorder.1);
    ctx.set_user_notify_callback(Some(Box::new(recorder)));
    let mut tracker =
        TrackTopologyChanges::new(Some(BrepId::from_index(0)), 7, true, Some(&mut ctx));
    // Execute's owner walk: each pre-op curve's GetOwner() is its edge, each pre-op
    // surface's its face
    tracker.set_owner_metadata(owners.into_iter().map(|(c, e)| (c.key, Some(e))).collect());
    let r = run_rust_op_in(k, b, second, op, a, Some(&mut ctx));
    // the shim's StopTracking before put_tracker: 1.0.1 records the completed edit's
    // dependencies (RecordDependencies) on the tracker's lists
    let recorded =
        tracker.stop_tracking_with_brep(Some(&mut ctx), b, &entity_of_key, &|e| key_of(e).key);
    let lists = tracker.lists();
    tracker.finish(Some(&mut ctx));
    must(recorded)?;
    drop(must(ctx.take_notify_lease())?);
    let mut out = r?;
    let geometry_seen = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let post = walk(b)?;
    // the curve each edge has after the op, by the edge's index in that walk
    let mut post_curves = BTreeMap::new();
    for j in 0..post.len(7) {
        let e = EdgeId::from_index(post.at(7, j)?);
        if let Some(c) = must(b.edge(e))?
            .curve
            .as_ref()
            .and_then(SmCurve::notify_key)
        {
            post_curves.insert(c, j);
        }
    }
    let mut t = Tracked {
        pre,
        post,
        brep,
        pre_curves: &pre_curves,
        post_curves,
        geometry: &geometry_seen,
        numbered: Vec::new(),
    };
    let tracker_at = out.len();
    put_tracker(&mut out, &lists, &mut t);
    if matches!(op, OpIn::DeleteFace { .. }) {
        // the shim's trailer: the edges, edge uses, faces and shells destroyed (their
        // destructors' SM_NO_DESTRUCTION)
        let destroyed = *edges_destroyed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.extend(destroyed.iter().map(|&n| n as f64));
    }
    if matches!(op, OpIn::Orient { .. }) {
        // the shim's trailer: each pre-op face's SmFace::SwapUV POST_EDIT count, the
        // face count and where the tracker's values start (see split_orient)
        for i in 0..pre[3] {
            let face = key_of(EntityRef::Face(FaceId::from_index(i as u32))).key;
            let swaps = lists
                .notify_events
                .iter()
                .filter(|e| {
                    e.operation == solidcore::geom::object::NotifyOperation::PostEdit
                        && e.caller.is_some_and(|c| c.key == face)
                })
                .count();
            out.push(swaps as f64);
        }
        out.push(pre[3] as f64);
        out.push(tracker_at as f64);
    }
    Ok(out)
}

/// An `Orient` output (either side) from its trailer: `out` before the tracker,
/// `swapped` (each face's `SwapUV` count) and `tracker` (see the module docs). An
/// output without a trailer (a B-rep that did not build) stays whole in `out`.
fn split_orient(v: Vec<f64>) -> Output {
    let trailer = match v.as_slice() {
        [.., n, at] if *n >= 0.0 && *at >= 0.0 => Some((*n as usize, *at as usize)),
        _ => None,
    };
    let Some((n, at)) = trailer else {
        return Output::new().values("out", v);
    };
    let body = v.len() - 2;
    let Some(swaps_at) = body.checked_sub(n).filter(|&x| x >= at) else {
        return Output::new().values("out", v);
    };
    let mut v = v;
    v.truncate(body);
    let swapped = v.split_off(swaps_at);
    let tracker = v.split_off(at);
    Output::new()
        .values("out", v)
        .values("swapped", swapped)
        .values("tracker", tracker)
}

/// The shim's `tag_brep`: every face (`GetFaces` order) and edge (`GetEdges` order)
/// gets its own long attribute, id = value = `10007 + 500 kind + index`.
fn tag_brep(b: &mut Brep, behavior: Behavior) -> Result<(), Fail> {
    let faces = must(bp1::faces(b, None))?;
    let edges = must(bp1::edges(b, None))?;
    let tagged = faces
        .into_iter()
        .enumerate()
        .map(|(i, f)| (EntityRef::Face(f), 0u64, i))
        .chain(
            edges
                .into_iter()
                .enumerate()
                .map(|(i, e)| (EntityRef::Edge(e), 1, i)),
        );
    for (entity, kind, index) in tagged {
        let id = 10007 + 500 * kind + index as u64;
        let value = i64::try_from(id).map_err(|_| Fail::Harness(harness("attribute id")))?;
        must(b.add_attribute(entity, Attribute::long(id, value, behavior)))?;
    }
    Ok(())
}

/// The shim's `put_attribute_rows`: the sorted face rows then edge rows of `b`, each
/// `n` and per attribute (ascending) `id behavior value users`.
fn put_attribute_rows(o: &mut Vec<f64>, b: &Brep) -> Result<(), Fail> {
    let row = |e: EntityRef| {
        let mut entries: Vec<[f64; 4]> = b
            .entity_attributes(e)
            .into_iter()
            .map(|(_, a)| {
                [
                    a.attribute_id() as f64,
                    f64::from(a.behavior() as u8),
                    a.long_value().map_or(0.0, |v| v as f64),
                    a.users().len() as f64,
                ]
            })
            .collect();
        entries.sort_by(|x, y| cmp_f64s(x, y));
        let mut r = vec![entries.len() as f64];
        r.extend(entries.into_iter().flatten());
        r
    };
    let faces = must(bp1::faces(b, None))?;
    let edges = must(bp1::edges(b, None))?;
    for mut rows in [
        faces.into_iter().map(|f| row(EntityRef::Face(f))).collect::<Vec<_>>(),
        edges.into_iter().map(|e| row(EntityRef::Edge(e))).collect(),
    ] {
        rows.sort_by(|x, y| cmp_f64s(x, y));
        o.push(rows.len() as f64);
        o.extend(rows.into_iter().flatten());
    }
    Ok(())
}

/// `std::vector<double>`'s lexicographic `<` (finite values).
fn cmp_f64s(x: &[f64], y: &[f64]) -> core::cmp::Ordering {
    x.iter()
        .zip(y)
        .map(|(p, q)| p.total_cmp(q))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| x.len().cmp(&y.len()))
}

#[allow(clippy::too_many_lines)]
fn run_rust_op_in(
    k: &mut Kernel,
    b: &mut Brep,
    second: Option<&mut Brep>,
    op: &OpIn,
    a: &[f64],
    context: Option<&mut Context>,
) -> Result<Vec<f64>, Fail> {
    let mut out = vec![0.0];
    let arg = |i: usize| -> Result<f64, Fail> {
        a.get(i)
            .copied()
            .ok_or_else(|| Fail::Harness(harness("args too short")))
    };
    // (the edits seed every object's SmTopology record the caller keeps none for)
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attrs = AttributeStore::default();
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attrs,
        context,
        key_of: &key_of,
    };
    match op {
        OpIn::Construct { mse, lsr } => {
            let nb = must(bp3::new_brep(k, *mse, *lsr))?;
            out.push(0.0);
            out.push(nb.tolerance());
            out.push(nb.this_model_size_estimate(k, false));
            out.push(nb.this_large_small_size_ratio(k, false));
            out.push(flag(nb.editing_enabled()));
            out.push(must(region_void_flag(&nb, nb.infinite_region()))?);
            put_counts(&mut out, &nb)?;
        }
        OpIn::ApiCopyNull => {
            let (s, _) = status(bp3::api_brep_copy(k, &mut env, None, &source_key_of))?;
            out.push(s);
        }
        OpIn::Copy {
            analytics,
            api,
            plain,
        } => {
            let src: &Brep = b;
            let r = if *plain {
                bp3::brep_copy(k, src)
            } else if *api {
                bp3::api_brep_copy(k, &mut env, Some(src), &source_key_of)
            } else {
                bp3::copy_brep(
                    k,
                    &mut env,
                    src,
                    &source_key_of,
                    None,
                    None,
                    true,
                    *analytics,
                )
            };
            let c = must(r)?;
            out.push(0.0);
            out.push(c.tolerance());
            out.push(flag(c.editing_enabled()));
            put_counts(&mut out, &c)?;
        }
        OpIn::AttributedCopy {
            behavior,
            shared_naming,
            plain,
        } => {
            let behavior = Behavior::from_code(i32::from(*behavior))
                .map_err(|e| Fail::Harness(harness(format!("behavior: {e}"))))?;
            tag_brep(b, behavior)?;
            let src: &Brep = b;
            let source: &dyn Fn(EntityRef) -> ObjectRef = if *shared_naming {
                &key_of
            } else {
                &source_key_of
            };
            let c = if *plain {
                must(bp3::brep_copy(k, src))?
            } else {
                must(bp3::api_brep_copy(k, &mut env, Some(src), source))?
            };
            out.push(0.0);
            out.push(c.tolerance());
            out.push(flag(c.editing_enabled()));
            put_counts(&mut out, &c)?;
            put_attribute_rows(&mut out, &c)?;
            put_attribute_rows(&mut out, src)?;
        }
        OpIn::AttributedMerge {
            analytics,
            behavior,
        } => {
            let dst = second.ok_or_else(|| Fail::Harness(harness("no second B-rep")))?;
            let behavior = Behavior::from_code(i32::from(*behavior))
                .map_err(|e| Fail::Harness(harness(format!("behavior: {e}"))))?;
            tag_brep(b, behavior)?;
            let (s, _) = status(bp3::merge_brep(
                k,
                &mut env,
                dst,
                b,
                &source_key_of,
                None,
                None,
                *analytics,
            ))?;
            out.push(s);
            put_counts(&mut out, dst)?;
            put_attribute_rows(&mut out, dst)?;
            put_attribute_rows(&mut out, b)?;
        }
        OpIn::Merge { analytics } => {
            let dst = second.ok_or_else(|| Fail::Harness(harness("no second B-rep")))?;
            let (s, _) = status(bp3::merge_brep(
                k,
                &mut env,
                dst,
                b,
                &source_key_of,
                None,
                None,
                *analytics,
            ))?;
            out.push(s);
            put_counts(&mut out, dst)?;
        }
        OpIn::CopyVertices { .. } => {
            let dst = second.ok_or_else(|| Fail::Harness(harness("no second B-rep")))?;
            let source = must(bp1::vertices(b, None))?;
            let existing = must(bp1::vertices(dst, None))?;
            let mut geometry = Vec::new();
            put_vertex_points(&mut geometry, b, &source)?;
            put_vertex_points(&mut geometry, dst, &existing)?;
            let n = arg(0)? as usize;
            let vs: Vec<VertexId> = (0..n)
                .map(|j| arg(1 + j).map(|x| VertexId::from_index(x as u32)))
                .collect::<Result<_, _>>()?;
            let (s, r) = status(bp3::copy_vertices(k, b, &vs, dst, &mut env, &source_key_of))?;
            out.push(s);
            out.push(r.as_ref().map_or(0.0, |x| x.len() as f64));
            put_counts(&mut out, dst)?;
            put_vertex_points(&mut geometry, b, &source)?;
            put_vertex_points(&mut geometry, dst, &existing)?;
            let copied = r.unwrap_or_default();
            let post = walk(dst)?;
            geometry.push(copied.len() as f64);
            for v in copied {
                geometry.push(post.find(9, Some(v.index())));
                let p = must(solidcore::topology::vertex::point(dst, v))?;
                geometry.extend([p.x, p.y, p.z]);
            }
            out.extend(geometry);
        }
        OpIn::DeleteFace { .. } => {
            let f = FaceId::from_index(arg(0)? as u32);
            let keep = (arg(3)? >= 0.0).then(|| RegionId::from_index(arg(3).unwrap_or(0.0) as u32));
            let mut marks = Marks::default();
            let opts = bp3::DeleteFaceOptions {
                delete_edges_vertices: arg(1)? != 0.0,
                do_region_nesting: arg(2)? != 0.0,
                region_to_keep: keep,
            };
            let (s, r) = status(bp3::delete_face(k, b, &mut env, &mut marks, f, opts))?;
            out.push(s);
            out.push(r.map_or(0.0, |x| x.len() as f64));
            put_counts(&mut out, b)?;
        }
        OpIn::Wire { curve, end, .. } => {
            let which = arg(0)? as u8;
            let region =
                (arg(1)? >= 0.0).then(|| RegionId::from_index(arg(1).unwrap_or(0.0) as u32));
            let v1 = VertexId::from_index(arg(2)? as u32);
            let v2 = VertexId::from_index(arg(3)?.max(0.0) as u32);
            let crv = SmCurve::from(rust_curve(curve)?);
            let at = a.len() - 6;
            let ivl = Extent1d::from_raw(arg(at)?, arg(at + 1)?);
            let orient = match arg(at + 2)? {
                0.0 => OrientType::Unknown,
                1.0 => OrientType::Same,
                2.0 => OrientType::Opposite,
                _ => return Err(Fail::Harness(harness("bad orientation"))),
            };
            let s = if which == 0 {
                status(bp3::make_wire_edge(
                    k, b, &mut env, region, v1, v2, crv, &ivl, orient,
                ))?
                .0
            } else {
                let p = Vec3::new(end[0], end[1], end[2]);
                status(bp3::make_wire_edge_vertex(
                    k, b, &mut env, region, v1, crv, &ivl, orient, p,
                ))?
                .0
            };
            out.push(s);
            put_counts(&mut out, b)?;
        }
        OpIn::Orient {
            outward,
            unify,
            faces,
        } => {
            let list: Option<Vec<FaceId>> = match faces {
                None => None,
                Some(l) => Some(
                    (0..l.len())
                        .map(|j| arg(3 + j).map(|x| FaceId::from_index(x as u32)))
                        .collect::<Result<_, _>>()?,
                ),
            };
            let before = must(bp1::faces(b, None))?;
            let mut marks = Marks::default();
            let (s, r) = status(bp3::orient_trimmed_surfaces(
                k,
                b,
                &mut env,
                &mut marks,
                *outward,
                *unify,
                list.as_deref(),
            ))?;
            out.push(s);
            out.push(flag(r.is_some_and(|x| x.maybe_not_closed_solid)));
            for f in before {
                let face = must(b.face(f))?;
                let d = face.uv_domain;
                out.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
                out.push(face.faceuse.map_or(-1.0, |x| f64::from(x.index())));
            }
            put_counts(&mut out, b)?;
        }
        OpIn::Squeeze { .. } => {
            let e = EdgeId::from_index(arg(0)? as u32);
            let which = arg(1)? as u8;
            let any = (arg(2)? >= 0.0).then(|| VertexId::from_index(arg(2).unwrap_or(0.0) as u32));
            let v = match which {
                0 => must(solidcore::topology::edge::vertex(b, e))?,
                1 => {
                    let v0 = must(solidcore::topology::edge::vertex(b, e))?;
                    must(solidcore::topology::edge::other_vertex(b, e, v0))?
                }
                _ => any,
            };
            let s = match v {
                Some(v) => status(bp3::squeeze_edge(k, b, &mut env, e, v))?.0,
                // a null survivor is neither vertex of the edge (`SER_MSG(SM_ERR)`)
                None => f64::from(SM_ERR),
            };
            out.push(s);
            put_counts(&mut out, b)?;
        }
        OpIn::TurnNurbs | OpIn::TurnNurbsOffset { .. } => {
            let edges = must(bp1::edges(b, None))?;
            let faces = must(bp1::faces(b, None))?;
            let offset = match op {
                OpIn::TurnNurbsOffset { offset } => Some(*offset),
                _ => None,
            };
            if let Some(d) = offset {
                wrap_in_offsets(k, b, &faces, d)?;
            }
            let (s, _) = status(bp3::turn_to_nurbs(k, b, &mut env))?;
            out.push(s);
            for e in edges {
                put_curve(&mut out, must(b.edge(e))?.curve.as_ref())?;
            }
            for f in faces {
                let s = must(b.face(f))?.surface.as_ref();
                out.push(s.map_or(-1.0, |x| x.type_code() as f64));
                if let (Some(_), Some(s)) = (offset, s) {
                    let d = s
                        .natural_uv_domain_with_nurb(k)
                        .map_err(|e| HarnessError::new(e.to_string()))?;
                    out.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
                }
            }
            put_counts(&mut out, b)?;
        }
    }
    Ok(out)
}

fn put_vertex_points(o: &mut Vec<f64>, b: &Brep, vertices: &[VertexId]) -> Result<(), Fail> {
    o.push(vertices.len() as f64);
    for &v in vertices {
        let p = must(solidcore::topology::vertex::point(b, v))?;
        o.extend([p.x, p.y, p.z]);
    }
    Ok(())
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    if standalone(&i.op) {
        let Some((op, args)) = op_args(&i.op, &[0; 10])? else {
            return Ok(Output::err("no target"));
        };
        // an input the FFI refuses is a harness error on both sides
        ffi::validate(op, "", &args, &[0; 10], false).map_err(|e| ffi_err(&e))?;
        let mut b = Brep::new();
        return match run_rust_op(&mut k, &mut b, None, &i.op, &args) {
            Ok(out) => Ok(Output::new().values("out", out)),
            Err(Fail::Deferred(o)) => Ok(*o),
            Err(Fail::Harness(e)) => Err(e),
        };
    }
    let (mut b, c) = match own_brep(&mut k, i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some((op, args)) = op_args(&i.op, &c)? else {
        return Ok(Output::err("no target"));
    };
    // an input the FFI refuses is a harness error on both sides: the Rust port works
    // through the guarded hang regimes as upstream does (HANG-16: a `TurnNurbsOffset` of
    // 1e300 kept it busy for 25 s in the UV trim curve rebuild, nightly fuzz00282), so it
    // must not run them (fuzz mutations reach them with the C++ side refused). The
    // checks see the same input vector as the C++ side and this B-rep's own counts.
    let mut input = head(i)?;
    input.extend(&args);
    ffi::validate(
        op,
        &path_of(&i.brep)?,
        &input,
        &c,
        b.infinite_region().is_some(),
    )
    .map_err(|e| ffi_err(&e))?;
    let mut second = match i.op {
        OpIn::Merge { .. } | OpIn::AttributedMerge { .. } | OpIn::CopyVertices { .. } => {
            Some(own_brep(&mut k, i)?.map_err(
                |_| harness("the second build of the B-rep failed where the first did not"),
            )?.0)
        }
        _ => None,
    };
    match run_rust_op(&mut k, &mut b, second.as_mut(), &i.op, &args) {
        Ok(out) if matches!(i.op, OpIn::Orient { .. }) => Ok(split_orient(out)),
        Ok(out) => Ok(Output::new().values("out", out)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation ---------------------------------------------------------------------------

fn gen_op(rng: &mut Rng) -> (OpIn, i64) {
    let sel = |rng: &mut Rng| rng.below(1 << 20) as i64;
    let editing = if rng.chance(0.4) {
        -1
    } else {
        rng.below(2) as i64
    };
    match rng.below(12) {
        0 => (
            OpIn::Construct {
                mse: *rng
                    .choose(&[solidcore::geom::tol::USE_DEFAULT, 1.0, 100.0, 1e-3, 1e6])
                    .unwrap_or(&1.0),
                lsr: *rng
                    .choose(&[solidcore::geom::tol::USE_DEFAULT, 1e-3, 0.5])
                    .unwrap_or(&0.5),
            },
            editing,
        ),
        1 => (
            OpIn::Copy {
                analytics: rng.chance(0.5),
                api: rng.chance(0.5),
                plain: false,
            },
            editing,
        ),
        2 => (
            OpIn::Merge {
                analytics: rng.chance(0.5),
            },
            editing,
        ),
        3 => {
            let n = rng.below(4) as usize;
            (
                OpIn::CopyVertices {
                    vertices: (0..n).map(|_| sel(rng)).collect(),
                },
                editing,
            )
        }
        4 | 5 => (
            OpIn::DeleteFace {
                face: sel(rng),
                delete_ev: rng.chance(0.5),
                nesting: rng.chance(0.7),
                keep: if rng.chance(0.7) { -1 } else { sel(rng) },
            },
            editing,
        ),
        6 => {
            let curve = gen_curve(rng, 3, 1.0);
            let (a, b) = (
                curve.u.first().copied().unwrap_or(0.0),
                curve.u.last().copied().unwrap_or(1.0),
            );
            (
                OpIn::Wire {
                    which: rng.below(2) as u8,
                    region: if rng.chance(0.6) { -1 } else { sel(rng) },
                    v1: sel(rng),
                    v2: sel(rng),
                    curve,
                    t0: a,
                    t1: b,
                    orient: 1 + rng.below(2) as u8,
                    end: rng.point3(-2.0, 2.0),
                },
                editing,
            )
        }
        7 | 8 => (
            OpIn::Orient {
                outward: rng.chance(0.5),
                unify: rng.chance(0.6),
                faces: if rng.chance(0.6) {
                    None
                } else {
                    let n = rng.below(3) as usize;
                    Some((0..n).map(|_| sel(rng)).collect())
                },
            },
            editing,
        ),
        9 | 10 => (
            OpIn::Squeeze {
                edge: sel(rng),
                which: rng.below(3) as u8,
                v: sel(rng),
            },
            editing,
        ),
        _ => (OpIn::TurnNurbs, editing),
    }
}

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p3"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = gen_brep(rng);
        // (no face op on a B-rep without faces, the wire edge and the network: it would
        // be skipped on both sides and compare nothing)
        let has_faces = matches!(brep, BrepIn::File { .. } | BrepIn::Face { .. });
        let mut gen = gen_op(rng);
        for _ in 0..64 {
            let needs_face = match &gen.0 {
                OpIn::DeleteFace { .. } => true,
                OpIn::Orient { faces: Some(f), .. } => !f.is_empty(),
                _ => false,
            };
            if has_faces || !needs_face {
                break;
            }
            gen = gen_op(rng);
        }
        let (op, editing) = gen;
        serde_json::to_value(Inputs {
            brep,
            validate_only: rng.chance(0.2),
            init_gaps: rng.chance(0.2),
            editing,
            op,
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_vertices_observes_returned_geometry_and_order() {
        let i: Inputs = serde_json::from_value(serde_json::json!({
            "brep": { "kind": "wire", "curve": { "dim": 3, "p": 1,
                "cpts": [[2, -3, 5, 1], [7, 11, -13, 1]], "u": [0, 0, 1, 1] },
                "t0": 0, "t1": 1 },
            "editing": -1, "op": "copy_vertices", "vertices": [1, 0, 1]
        }))
        .unwrap();
        let cpp = run_cpp(&i).unwrap();
        let mut rust = run_rust(&i).unwrap();
        assert!(crate::compare::compare(&cpp, &rust, &[]).pass());
        let out = rust.values.get_mut("out").unwrap();
        let tail = &out[out.len() - 13..];
        assert_eq!(tail[0], 3.0);
        assert_eq!(&tail[2..5], &[7.0, 11.0, -13.0]);
        assert_eq!(&tail[6..9], &[2.0, -3.0, 5.0]);
        assert_eq!(&tail[10..13], &[7.0, 11.0, -13.0]);
        assert_ne!(
            tail[1], tail[9],
            "repeated input must yield distinct copies"
        );
        // A wrong copied coordinate must fail independently of counts and graph.
        *out.last_mut().unwrap() += 1.0;
        assert!(!crate::compare::compare(&cpp, &rust, &[]).pass());
    }

    /// Classifies the passing cases of the committed report by op and C++ status
    /// (`out[0]` the build status, `out[1]` the op's), the split the PR and the
    /// plan notes quote. Run from the repository with the C++ reference:
    /// `cargo test --profile difftest -p difftest --features cpp classify_passes --
    /// --ignored --nocapture`.
    /// The tracker serialisation tells geometry objects apart by an anchor of their
    /// own (an edge's curve before or after the op), not by the output: swapping the
    /// new and old curve of one `SM_NO_CHANGE_GEOMETRY` changes the output.
    #[test]
    fn swapped_geometry_changes_the_output() {
        use solidcore::geom::object::NotifyOperation;
        use solidcore::topology::topology::{NotifyEvent, TopologyChangeLists};
        let g = |n: u64| ObjectRef {
            type_code: BSplineCurve::TYPE,
            key: ObjectKey(solidcore::geom::context::GEOMETRY_KEY_BASE | n),
        };
        let edge = key_of(EntityRef::Edge(EdgeId::from_index(0)));
        let (a, b) = (g(1), g(2));
        let lists = |new: ObjectRef, old: ObjectRef| TopologyChangeLists {
            notify_events: vec![NotifyEvent {
                operation: NotifyOperation::ChangeGeometry,
                caller: Some(edge),
                data1: Some(new),
                data2: None,
                data3: Some(old),
            }],
            ..TopologyChangeLists::default()
        };
        let pre_curves = BTreeMap::new();
        let geometry = vec![a.key, b.key];
        let run = |l: &TopologyChangeLists, post_curves: BTreeMap<ObjectKey, usize>| {
            let mut t = Tracked {
                pre: [0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
                post: Walk::default(),
                brep: ObjectKey(u64::MAX),
                pre_curves: &pre_curves,
                post_curves,
                geometry: &geometry,
                numbered: Vec::new(),
            };
            let mut o = Vec::new();
            put_tracker(&mut o, l, &mut t);
            o
        };
        // anchored (each curve some edge's after the op): the swap shows
        let anchored: BTreeMap<_, _> = [(a.key, 0), (b.key, 1)].into_iter().collect();
        assert_ne!(
            run(&lists(a, b), anchored.clone()),
            run(&lists(b, a), anchored)
        );
        // geometry attached to no edge before or after the op is numbered by its first
        // appearance, so a swap of two such objects within one event does not show:
        // the documented boundary
        assert_eq!(
            run(&lists(a, b), BTreeMap::new()),
            run(&lists(b, a), BTreeMap::new())
        );
    }

    /// Prints both sides' outputs of one fixture case from its tail (`DUMP=<file>#<i>`,
    /// `DUMP_TAIL=<n>` values, default 120), for debugging a disagreement: `DUMP=...
    /// cargo test --profile difftest -p difftest --features cpp dump_case -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore = "needs the C++ reference and a DUMP=<fixture>#<index> case"]
    fn dump_case() {
        std::env::set_current_dir("../..").unwrap();
        let spec = std::env::var("DUMP").unwrap();
        let tail: usize = std::env::var("DUMP_TAIL")
            .ok()
            .and_then(|x| x.parse().ok())
            .unwrap_or(120);
        let (file, idx) = spec.split_once('#').unwrap();
        let j: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let i: Inputs =
            serde_json::from_value(j["cases"][idx.parse::<usize>().unwrap()]["inputs"].clone())
                .unwrap();
        for (side, o) in [
            ("cpp", run_cpp(&i).unwrap()),
            ("rust", run_rust(&i).unwrap()),
        ] {
            let out = o.values.get("out").cloned().unwrap_or_default();
            let from = out.len().saturating_sub(tail);
            println!(
                "DUMP {side} len={} err={:?} deferred={:?}",
                out.len(),
                o.error,
                o.deferred
            );
            println!("DUMP {side} [{from}..] {:?}", &out[from..]);
        }
    }

    #[test]
    #[ignore = "needs the C++ reference and reports/difftest/topology.brep_p3.json"]
    fn classify_passes() {
        std::env::set_current_dir("../..").unwrap();
        let r: Value = serde_json::from_str(
            &std::fs::read_to_string("reports/difftest/topology.brep_p3.json").unwrap(),
        )
        .unwrap();
        let mut counts = BTreeMap::<String, usize>::new();
        for c in r["cases"].as_array().unwrap() {
            if c["outcome"] != "pass" {
                continue;
            }
            let src = c["source"].as_str().unwrap();
            let v: Value = if let Some(seed) = src.strip_prefix("generated:seed=") {
                let mut rng = Rng::new(seed.parse().unwrap());
                Runner.generate(&mut rng).unwrap()
            } else {
                let f = src
                    .strip_prefix("fixture:")
                    .unwrap()
                    .split(' ')
                    .next()
                    .unwrap();
                let (file, idx) = f.split_once('#').unwrap();
                let j: Value =
                    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
                j["cases"][idx.parse::<usize>().unwrap()]["inputs"].clone()
            };
            let op = v["op"].as_str().unwrap().to_string();
            let i: Inputs = serde_json::from_value(v.clone()).unwrap();
            let o = run_cpp(&i).unwrap();
            let out = o.values.get("out").cloned().unwrap_or_default();
            let st = (out.first().copied(), out.get(1).copied());
            let extra = match (&i.op, st) {
                (OpIn::Orient { unify, faces, .. }, (Some(0.0), Some(0.0))) => format!(
                    " unify={unify} swaplist={}",
                    faces.as_ref().map_or(-1, |f| f.len() as i64)
                ),
                (OpIn::CopyVertices { vertices }, _) => format!(" n={}", vertices.len()),
                (OpIn::Squeeze { which, .. }, _) => format!(" which={which}"),
                (OpIn::DeleteFace { .. }, _) => format!(" editing={}", i.editing),
                _ => String::new(),
            };
            let key = format!("{op} {st:?} err={:?}{extra}", o.error);
            *counts.entry(key).or_default() += 1;
        }
        for (k, n) in counts {
            println!("CLASS {n} {k}");
        }
    }

    /// Geometry beyond 1e150 hangs the C++ in `SmCurve::GlobalCoincidenceChecker`
    /// (HANG-16; fuzz case 1124: one control point at z = 1e300, Orient by rays): the
    /// FFI must reject it before calling in, and still accept the non-rational weight.
    #[test]
    fn huge_geometry_is_rejected() {
        let case = |z: f64| -> Inputs {
            let w = -f64::MAX;
            let v = serde_json::json!({
                "brep": {"kind": "face", "dom": [0.0, 0.0, 1.0, 1.0], "surface": {"bspline": {
                    "net": [[[0, 0, 0, w], [0, 1, 0, w]], [[1, 0, 0, w], [1, 1, z, w]]],
                    "p": 1, "q": 1, "u": [0, 0, 1, 1], "v": [0, 0, 1, 1]}}},
                "op": "orient", "outward": false, "unify": false, "faces": null
            });
            serde_json::from_value(v).unwrap()
        };
        let err = run_cpp(&case(1e300)).unwrap_err().to_string();
        assert!(err.contains("HANG-16"), "{err}");
        // the Rust side applies the same guard (`ffi::validate`): the port works through
        // the regime for tens of seconds (nightly fuzz00282, a 1e300 TurnNurbsOffset)
        let err = run_rust(&case(1e300)).unwrap_err().to_string();
        assert!(err.contains("HANG-16"), "{err}");
        assert!(run_cpp(&case(0.5)).is_ok());
    }

    /// Nightly fuzz case `topology.brep_p3/fuzz00282` (`nurbs_off_box_with_large_fillet_-0.05`
    /// with `/offset = 1e+300`): the FFI refuses the C++ side (HANG-16) and the Rust side
    /// must be refused the same way instead of running the operation (it took 25 s).
    #[test]
    fn huge_offset_is_refused_on_both_sides() {
        let v = serde_json::json!({
            "brep": {"kind": "file", "file": "box_with_large_fillet.smb", "rebuild": false},
            "editing": -1, "op": "turn_nurbs_offset", "offset": 1e300
        });
        let i: Inputs = serde_json::from_value(v).unwrap();
        let cpp = run_cpp(&i).unwrap_err().to_string();
        let rust = run_rust(&i).unwrap_err().to_string();
        assert!(cpp.contains("HANG-16"), "{cpp}");
        assert_eq!(cpp, rust);
    }
}
