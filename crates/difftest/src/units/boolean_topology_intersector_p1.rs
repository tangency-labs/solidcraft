//! Runner for unit `boolean.topology_intersector_p1`. C++:
//! `solidcore_ffi::units::boolean_topology_intersector_p1` (shim
//! `harness/cpp/shim/boolean_topology_intersector_p1.cpp`). Rust:
//! `solidcore::boolean::topology_intersector::p1`.
//!
//! **Two real B-reps.** Each case builds two `SmBrep`s on the C++ side (an upstream
//! `.smb` file, a wire edge, a face from a surface, or the wire network of
//! `prog_test` `my_test_topology`; the second may be absent), makes an
//! `SmTopologyIntersector` over them and runs a script of the unit's functions. The
//! Rust side imports the shim's exports of both B-reps (as the `topology.vertexuse`
//! runner does) into two [`Brep`]s behind one adapter ([`Two`], which implements the
//! port's [`IntersectorBrep`] and the proto topology's `ProtoBrep` with the ported
//! topology functions) and runs the same script on a [`TopologyIntersector`]. Both
//! sides produce the same flat record: per op its code and outputs; object handles
//! are `brep kind index` triples in the exports' traversal order. The record is
//! compared as values.
//!
//! **Selectors.** Generated handles are selectors taken modulo the entity count of
//! their kind (both sides resolve them the same way from the exports). A script runs
//! up to its first unrunnable op (a handle naming nothing, a handle-taking op after a
//! mutating one, a manager op without a manager, an op that dereferences the
//! intersector's B-reps while one is null): both sides run that prefix and end the
//! record with `-4 index` naming the op left out; a script cut at its first op is an
//! error on both sides (counted as both-errored, since it compares nothing). Parameters of generated
//! classification intervals and points are relative to the edge's interval or
//! evaluated on the face's surface at resolution. The generator only draws handle
//! kinds that exist (no faces on wire or network B-reps, no registered curve before a
//! `register_ssi`).
//!
//! **Guards.** The shim refuses (`-2`) what upstream dereferences without checking:
//! edges without a curve or vertices and faces without a surface in the geometry ops;
//! the runner mirrors those guards explicitly, as the proto-topology runners do. The
//! ops that edit topology (`sm_SetEdgeClass`, the splits, imprints and intersections)
//! emit their outputs only on a success status on both sides: the port returns no
//! partly filled outputs with an error.
//!
//! **Ordering.** `GetMappedBrepObjects` and `GetSubset` list objects in pointer-hash
//! order upstream; both sides sort those lists by handle (plan hazard `ptr-hash`, see
//! `plan/findings.md`).
//!
//! **Deferred.** Everything that reaches `SmBrep::GetEdges` / `GetVertices`
//! (`topology.brep_p1`: the common edge/vertex lists, the shell-vertex and wire/lamina
//! intersections), the B-rep edits (`topology.brep_p3/p5/p7/p9`), `SmFace::GetEdges` /
//! `GetVertices` / `GetBrep` (`topology.face_p4`), `SmFace::CalculateBoundingBox` and
//! `Point3DClassify` (`query.face_p1`), `SmFace::FaceIntersect` (`topology.face_p3`),
//! `SmSurface::GlobalCurveIntersect` / `CoincidenceCheck` (`surface.surface_p8`).
//! The manager's construction with a zero approximation tolerance runs upstream's
//! B-rep fallback (`SmBrep::GetTolerance`, `SmBrep::GetMinEdgeBBoxSize`) on both B-reps.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::boolean::topology_intersector::p1 as ti;
use solidcore::boolean::topology_intersector::p1::{IntersectorBrep, Reason, TopologyIntersector};
use solidcore::boolean::topology_intersector::p2::IntersectorBrepP2;
use solidcore::curve::curve::p2::{CurveKind, BSPLINE_CURVE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::a_object::BREP_TYPE;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::object::{ObjectRef, ShapeMember};
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject};
use solidcore::geom::tol::{update_object_tolerance, zone_tol3d, TopologyToUpdate, UNINIT_TOL};
use solidcore::intersect::curve_class::p1::{CurveClassification, CurveInterval};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::intersect::topology_solver::p1::TopologyObjects;
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::query::face::p2::{
    curve_on_classify, point_classify, CurveOnClassifyObjects, PointClassifyOptions,
};
use solidcore::surface::surface::p2::TsectCurveType;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p5 as bp5;
use solidcore::topology::brep::p7;
use solidcore::topology::brep_cache::{arena_identity, BrepCache, CacheObject, CacheSource};
use solidcore::topology::edge as ed;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3 as eu3;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::face::p3 as fp3;
use solidcore::topology::face::p3::{FaceIntersectOptions, IntersectObject};
use solidcore::topology::proto_topology::p1::ProtoTopology;
use solidcore::topology::proto_topology::{ProtoBrep, ProtoGeometry, ProtoTopologyManager};
use solidcore::topology::topology::Topology;
use solidcore::topology::vertex as vx;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, LoopId, VertexId, VertexuseId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::boolean_topology_intersector_p1::{self as ffi, Op};
use solidcore_ffi::units::boolean_topology_intersector_p2 as ffi2;

use super::topology_vertexuse::import;
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

/// Generated cases at least.
pub(super) const MIN_GENERATED: usize = 800;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// `SmTopoTypes.h` codes of `GetMappedBrepObjects`' type argument.
const VERTEX_TYPE: i64 = 16021;
const EDGE_TYPE: i64 = 16016;
const FACE_TYPE: i64 = 16002;

// ---- inputs -----------------------------------------------------------------------------

/// A B-spline curve: `dim`, control points `[x y z w]` (`w = NOW` non-rational, `z =
/// NOZ` 2D), degree, knots (the `topology.vertexuse` layout).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CrvIn {
    dim: u64,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default)]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SrfIn {
    Bspline(BssIn),
    Plane { o: [f64; 3], n: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum BrepIn {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File {
        file: String,
        #[serde(default)]
        rebuild: bool,
    },
    Wire {
        curve: CrvIn,
        t0: f64,
        t1: f64,
    },
    Face {
        surface: SrfIn,
        dom: [f64; 4],
    },
    Network,
}

/// A handle `[brep, kind, index]`; the index of a generated handle is a selector.
pub(super) type Hin = [i64; 3];

/// A point classification input: the class code and the object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PcIn {
    pub(super) cls: i64,
    pub(super) h: Hin,
}

/// A point classification of a classification input (part 2): the class code, the
/// object, the parameter (`t`, a fraction of the classification's interval when it is
/// relative; none: not set) and the deviation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PciIn {
    pub(super) cls: i64,
    pub(super) h: Hin,
    #[serde(default)]
    pub(super) t: Option<f64>,
    #[serde(default)]
    pub(super) dev: f64,
}

/// One interval of a classification input: its ends and start, mid and end
/// classifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct IvlIn {
    pub(super) a: f64,
    pub(super) b: f64,
    pub(super) pcs: [PciIn; 3],
}

/// A curve classification input (part 2's fix-ups): the curve (an edge's curve or a
/// registered one), its interval, the source zone tolerance and the intervals. With
/// `relative` every parameter is a fraction of the edge's interval (edge curves only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CcIn {
    pub(super) c: Hin,
    pub(super) t0: f64,
    pub(super) t1: f64,
    pub(super) tol: f64,
    pub(super) ivls: Vec<IvlIn>,
    #[serde(default)]
    pub(super) relative: bool,
}

/// One registered intersection curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SsiCurve {
    c3: CrvIn,
    #[serde(default)]
    uv1: Option<CrvIn>,
    #[serde(default)]
    uv2: Option<CrvIn>,
    #[serde(default)]
    ty: i64,
    #[serde(default)]
    dev: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(super) enum OpIn {
    Relate {
        a: Hin,
        b: Hin,
    },
    OtherMate {
        h: Hin,
    },
    ThisMate {
        h: Hin,
    },
    BrepMate {
        h: Hin,
    },
    IsMated {
        a: Hin,
        b: Hin,
    },
    MappedBrep {
        ty: i64,
    },
    CommonEdges,
    CommonVertices,
    SetSubset {
        list: Vec<Hin>,
    },
    FixupEnd {
        dim: u8,
        tol: f64,
        pc_b: PcIn,
        pc_o: PcIn,
    },
    FixupMid {
        dim: u8,
        tol: f64,
        pc_b: PcIn,
        pc_o: PcIn,
    },
    Getters,
    SetTols {
        approx: f64,
        ang: f64,
    },
    SetBreps {
        a: Hin,
        b: Hin,
    },
    RegisterSsi {
        s1: Hin,
        s2: Hin,
        curves: Vec<SsiCurve>,
    },
    CheckSsi {
        s1: Hin,
        s2: Hin,
    },
    ClearSsi,
    CanImprint {
        ty: i64,
    },
    Lowest {
        list: Vec<Hin>,
    },
    /// `t0`/`t1`: absolute, or fractions of the edge's interval when `relative`.
    SetEdgeClass {
        e: Hin,
        t0: f64,
        t1: f64,
        tol: f64,
        #[serde(default)]
        relative: bool,
    },
    CheckInterior {
        e: Hin,
        t: f64,
        tol: f64,
        #[serde(default)]
        relative: bool,
    },
    SplitTwo {
        e1: Hin,
        e2: Hin,
        tol: f64,
    },
    /// `p`: the point, or with `on_surface` the `[u, v, offset-along-normal]` to
    /// evaluate on the face's surface (`uv` as fractions of its domain).
    ImprintFace {
        f: Hin,
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        on_surface: bool,
    },
    VertexFace {
        verts: Vec<Hin>,
        faces: Vec<Hin>,
        manifold: bool,
        ignore: bool,
    },
    EdgeFace {
        edges: Vec<Hin>,
        faces: Vec<Hin>,
        manifold: bool,
        ignore: bool,
    },
    FaceFace {
        verts: Vec<Hin>,
        faces1: Vec<Hin>,
        faces2: Vec<Hin>,
        ignore: bool,
        nesting: bool,
    },
    FaceFaceCoin {
        edges: Vec<Hin>,
        faces: Vec<Hin>,
        ignore: bool,
        nesting: bool,
    },
    Resolve,
    Imprint,
    ShellVerts {
        first: u8,
        swap: bool,
    },
    WireLamina {
        first: u8,
        lamina: bool,
        swap: bool,
    },
    MgrNew {
        capture: f64,
        override_tol: f64,
    },
    // ---- part 2 (unit boolean.topology_intersector_p2; shim ops 41 to 52) ----
    RemoveRel {
        a: Hin,
        b: Hin,
    },
    Squeeze {
        bv: Hin,
        ov: Hin,
    },
    DeleteFaces {
        faces: Vec<Hin>,
        #[serde(default)]
        keep: Option<Vec<Hin>>,
    },
    FixGaps {
        verts: Vec<Hin>,
    },
    IntersectTopo,
    SetRels,
    Iir,
    FixupClass {
        b: CcIn,
        o: CcIn,
    },
    FixupSplit {
        index: usize,
        b: CcIn,
        o: CcIn,
    },
    EdgeEdge {
        edges1: Vec<Hin>,
        edges2: Vec<Hin>,
        manifold: bool,
        ignore: bool,
    },
    EdgeFaceCoin {
        verts: Vec<Hin>,
        excluded: Vec<Hin>,
        ignore: bool,
        nesting: bool,
    },
    CloseLoop {
        v: Hin,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Inputs {
    pub(super) brep_a: BrepIn,
    #[serde(default)]
    pub(super) brep_b: Option<BrepIn>,
    /// 0: the constructor with B-reps and tolerances; 1: the context-only one plus
    /// the setters.
    #[serde(default)]
    pub(super) ctor: u8,
    pub(super) approx: f64,
    pub(super) ang: f64,
    pub(super) ops: Vec<OpIn>,
}

// ---- encoding ---------------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn push_curve(v: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("curve without points"))?;
    let m =
        c.u.len()
            .checked_sub(1)
            .ok_or_else(|| harness("curve without knots"))?;
    v.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        v.extend(p);
    }
    v.extend(&c.u);
    Ok(())
}

fn push_surface(v: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend(o);
            v.extend(n);
        }
        SrfIn::Bspline(b) => {
            let n = b
                .net
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("empty net"))?;
            let m = b
                .net
                .first()
                .and_then(|r| r.len().checked_sub(1))
                .ok_or_else(|| harness("empty net row"))?;
            if b.net.iter().any(|r| r.len() != m + 1) {
                return Err(harness("ragged net"));
            }
            let ku =
                b.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("surface without u knots"))?;
            let kv =
                b.v.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("surface without v knots"))?;
            v.extend([
                0.0,
                flag(b.oob),
                f64::from(b.form),
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                ku as f64,
                kv as f64,
            ]);
            for row in &b.net {
                for p in row {
                    v.extend(p);
                }
            }
            v.extend(&b.u);
            v.extend(&b.v);
        }
    }
    Ok(())
}

/// The `.smb` path of a B-rep input ("" when it is not a file).
fn path_of(b: Option<&BrepIn>) -> Result<String, HarnessError> {
    match b {
        Some(BrepIn::File { file, .. }) => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

fn brep_block(v: &mut Vec<f64>, b: &BrepIn) -> Result<(), HarnessError> {
    match b {
        BrepIn::File { rebuild, .. } => {
            v.push(0.0);
            v.push(flag(*rebuild));
        }
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(v, surface)?;
            v.extend(dom);
        }
        BrepIn::Network => v.push(3.0),
    }
    v.push(0.0); // validate_only
    v.push(0.0); // init_gaps
    Ok(())
}

/// The two B-rep blocks of a case.
fn blocks(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    brep_block(&mut v, &i.brep_a)?;
    match &i.brep_b {
        Some(b) => {
            v.push(1.0);
            brep_block(&mut v, b)?;
        }
        None => v.push(0.0),
    }
    Ok(v)
}

/// The C++ exports of the case's B-reps.
fn export(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let pa = path_of(Some(&i.brep_a))?;
    let pb = path_of(i.brep_b.as_ref())?;
    let block = blocks(i)?;
    crate::export_memo::export(
        "boolean_topology_intersector_p1",
        &[&pa, &pb],
        &block,
        || ffi::eval(Op::Export, [&pa, &pb], &block),
    )
    .map_err(|e| harness(format!("c++ export: {e}")))
}

/// A build failure as an operation error.
fn build_failure(export: &[f64]) -> Output {
    Output::err(format!(
        "brep_build_{}_status_{}",
        if export.first() == Some(&-3.0) {
            "b"
        } else {
            "a"
        },
        export.get(1).copied().unwrap_or(0.0)
    ))
}

// ---- handles ------------------------------------------------------------------------------

/// Handle kinds (the shim's `HKind`, plus the Rust-only kinds).
pub(super) const K_VERTEX: u8 = 0;
pub(super) const K_EDGE: u8 = 1;
pub(super) const K_FACE: u8 = 2;
pub(super) const K_EDGE_CURVE: u8 = 3;
pub(super) const K_FACE_SURFACE: u8 = 4;
pub(super) const K_CURVE: u8 = 5;
pub(super) const K_BREP: u8 = 6;
/// A curve copy made by the port (never named by an input; `-1 -3 -1` in outputs, as
/// the shim's table copies).
const K_EXTRA: u8 = 7;
pub(super) const K_NULL: i64 = 8;
/// The intersector itself (the manager's `pTI`).
const K_TI: u8 = 9;
const K_EDGEUSE: u8 = 10;
const K_LOOP: u8 = 11;
/// A vertex use, by the index of its id (Rust side only: the classification's own
/// identities, never exported by the shim).
const K_VERTEXUSE: u8 = 12;
/// The UV trim curve stored on an edge use (or its mate), by the edge use's index
/// (Rust side only, resolved by `ClassGeometry::curve`).
const K_TRIM_CURVE: u8 = 13;

/// An object handle: the B-rep (`2` for the curve stores, `-1` for the intersector),
/// the kind and the index. Ordered as the shim sorts (`brep`, `kind`, `index`). A
/// curve handle also carries its curve's natural interval (what `SmSolutionArray`
/// reads through `SmCurve::GetNaturalInterval`); it takes no part in identity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct H {
    pub(crate) b: i8,
    pub(crate) kind: u8,
    pub(crate) i: u32,
    ivl: [f64; 2],
}

impl PartialEq for H {
    fn eq(&self, o: &Self) -> bool {
        (self.b, self.kind, self.i) == (o.b, o.kind, o.i)
    }
}

impl Eq for H {}

impl PartialOrd for H {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for H {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.b, self.kind, self.i).cmp(&(o.b, o.kind, o.i))
    }
}

pub(crate) const fn hd(b: i8, kind: u8, i: u32) -> H {
    H {
        b,
        kind,
        i,
        ivl: [f64::NAN, f64::NAN],
    }
}

/// A curve handle carrying the curve's natural interval.
fn curve_h(b: i8, kind: u8, i: u32, c: &SmCurve) -> H {
    let ivl = c.natural_interval();
    H {
        b,
        kind,
        i,
        ivl: [ivl.min(), ivl.max()],
    }
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self.kind {
            K_VERTEX => ClassObjectKind::Vertex,
            K_EDGE => ClassObjectKind::Edge,
            K_FACE => ClassObjectKind::Face,
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_TRIM_CURVE => ClassObjectKind::Curve,
            K_FACE_SURFACE => ClassObjectKind::Surface,
            K_EDGEUSE => ClassObjectKind::Edgeuse,
            K_VERTEXUSE => ClassObjectKind::Vertexuse,
            K_LOOP => ClassObjectKind::Loop,
            _ => ClassObjectKind::Other,
        }
    }
}

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        match self.kind {
            K_VERTEX => ObjectKind::Vertex,
            K_EDGE => ObjectKind::Edge,
            K_FACE => ObjectKind::Face,
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_TRIM_CURVE => ObjectKind::Curve,
            K_FACE_SURFACE => ObjectKind::Surface,
            _ => ObjectKind::Other,
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        match self.kind {
            K_EDGE_CURVE | K_CURVE | K_EXTRA if self.ivl[0].is_finite() => {
                Some(Extent1d::from_min_max(self.ivl[0], self.ivl[1]))
            }
            _ => None,
        }
    }
}

const TI_HANDLE: H = hd(-1, K_TI, 0);

pub(crate) const fn brep_h(b: i8) -> H {
    hd(b, K_BREP, b as u32)
}

fn put_h(o: &mut Vec<f64>, h: Option<H>) {
    match h {
        None => o.extend([-1.0, f64::from(K_NULL as u8), -1.0]),
        Some(h)
            if h.kind == K_EXTRA || h.kind == K_TI || h.kind == K_EDGEUSE || h.kind == K_LOOP =>
        {
            o.extend([-1.0, -3.0, -1.0]);
        }
        Some(h) => o.extend([f64::from(h.b), f64::from(h.kind), f64::from(h.i)]),
    }
}

fn h_to_triple(h: Option<H>) -> Hin {
    match h {
        None => [-1, K_NULL, -1],
        Some(h) => [i64::from(h.b), i64::from(h.kind), i64::from(h.i)],
    }
}

fn triple_to_h(t: Hin) -> Option<H> {
    if t[1] == K_NULL {
        return None;
    }
    Some(hd(t[0] as i8, t[1] as u8, t[2] as u32))
}

// ---- the two B-reps behind the handles --------------------------------------------------

/// The topology records, identities and attribute store a vertex notification runs
/// with (the `topology.vertex` runner's environment).
struct NotifyEnv {
    records: BTreeMap<ObjectKey, Topology>,
    attributes: AttributeStore,
    this: ObjectRef,
}

impl NotifyEnv {
    fn new(b: Option<&Brep>) -> Self {
        let mut records = BTreeMap::new();
        records.insert(ObjectKey(1), Topology::new());
        if let Some(b) = b {
            for id in b.vertexuses().ids() {
                records.insert(ObjectKey(100 + u64::from(id.index())), Topology::new());
            }
        }
        Self {
            records,
            attributes: AttributeStore::default(),
            this: ObjectRef {
                type_code: vx::TYPE,
                key: ObjectKey(1),
            },
        }
    }
}

fn vertexuse_ref(vu: VertexuseId) -> ObjectRef {
    ObjectRef {
        type_code: solidcore::topology::vertexuse::TYPE,
        key: ObjectKey(100 + u64::from(vu.index())),
    }
}

/// The two B-reps, the registered curves (kind 5) and the port's curve copies (kind
/// 7).
pub(crate) struct Two {
    breps: [Option<Brep>; 2],
    curves: Vec<SmCurve>,
    extra: Vec<SmCurve>,
    env: [NotifyEnv; 2],
    /// Each B-rep's cache (`SmBrep::GetBrepCache`), built on first use as the C++
    /// topology solver builds it on demand.
    caches: [std::cell::OnceCell<BrepCache<H>>; 2],
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("difftest::boolean_topology_intersector_p1::Two", reason)
}

const fn vid(h: H) -> VertexId {
    VertexId::from_index(h.i)
}
const fn eid(h: H) -> EdgeId {
    EdgeId::from_index(h.i)
}
const fn fid(h: H) -> FaceId {
    FaceId::from_index(h.i)
}

impl Two {
    pub(crate) fn new(a: Brep, b: Option<Brep>) -> Self {
        let env = [NotifyEnv::new(Some(&a)), NotifyEnv::new(b.as_ref())];
        Self {
            breps: [Some(a), b],
            curves: Vec::new(),
            extra: Vec::new(),
            env,
            caches: [std::cell::OnceCell::new(), std::cell::OnceCell::new()],
        }
    }

    /// Deletes the cache of the B-rep behind `brep` (`SmBrep::Notify`'s `PRE_EDIT` /
    /// `DESTRUCTION`); the next use rebuilds it.
    pub(crate) fn delete_cache(&mut self, brep: H) -> solidcore::Result<()> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        self.caches
            .get_mut(slot)
            .ok_or_else(|| invalid("bad B-rep"))?
            .take();
        Ok(())
    }

    fn brep(&self, h: H) -> Option<&Brep> {
        self.breps.get(usize::try_from(h.b).ok()?)?.as_ref()
    }

    fn brep_mut(&mut self, h: H) -> Option<&mut Brep> {
        self.breps.get_mut(usize::try_from(h.b).ok()?)?.as_mut()
    }

    pub(crate) fn brep_r(&self, h: H) -> solidcore::Result<&Brep> {
        self.brep(h)
            .ok_or_else(|| invalid("handle names an absent B-rep"))
    }

    pub(crate) fn brep_m(&mut self, h: H) -> solidcore::Result<&mut Brep> {
        self.brep_mut(h)
            .ok_or_else(|| invalid("handle names an absent B-rep"))
    }

    /// The edge's curve handle (`ProtoBrep::edge_curve`'s shape).
    #[allow(clippy::option_option)]
    fn edge_curve_h(&self, e: H) -> Option<Option<H>> {
        let b = self.brep(e)?;
        let edge = b.edge(eid(e)).ok()?;
        Some(
            edge.curve
                .as_ref()
                .map(|c| curve_h(e.b, K_EDGE_CURVE, e.i, c)),
        )
    }

    /// The face's surface handle (`ProtoBrep::face_surface`'s shape).
    #[allow(clippy::option_option)]
    fn face_surface_h(&self, f: H) -> Option<Option<H>> {
        let b = self.brep(f)?;
        let face = b.face(fid(f)).ok()?;
        Some(face.surface.as_ref().map(|_| hd(f.b, K_FACE_SURFACE, f.i)))
    }

    /// Gives a curve made by the port a handle in the curve store.
    fn register_curve(&mut self, curve: SmCurve) -> solidcore::Result<H> {
        self.insert_curve(curve)
            .ok_or_else(|| invalid("curve store is full"))
    }

    /// [`Two::register_curve`] for each curve present.
    fn register_curves(
        &mut self,
        curves: Vec<Option<SmCurve>>,
    ) -> solidcore::Result<Vec<Option<H>>> {
        curves
            .into_iter()
            .map(|c| c.map(|c| self.register_curve(c)).transpose())
            .collect()
    }

    fn entity_h(b: i8, e: EntityRef) -> Option<H> {
        match e {
            EntityRef::Vertex(v) => Some(hd(b, K_VERTEX, v.index())),
            EntityRef::Edge(x) => Some(hd(b, K_EDGE, x.index())),
            EntityRef::Face(f) => Some(hd(b, K_FACE, f.index())),
            EntityRef::Loop(l) => Some(hd(b, K_LOOP, l.index())),
            EntityRef::Edgeuse(eu) => Some(hd(b, K_EDGEUSE, eu.index())),
            _ => None,
        }
    }

    /// Whether the shim would run a geometry op on this edge (curve and a vertex).
    fn edge_ok(&self, e: Option<H>) -> bool {
        let Some(e) = e else {
            return false;
        };
        let Some(b) = self.brep(e) else {
            return false;
        };
        b.edge(eid(e)).is_ok_and(|edge| edge.curve.is_some())
            && ed::vertex(b, eid(e)).ok().flatten().is_some()
    }

    fn face_ok(&self, f: Option<H>) -> bool {
        let Some(f) = f else {
            return false;
        };
        self.brep(f)
            .and_then(|b| b.face(fid(f)).ok())
            .is_some_and(|face| face.surface.is_some())
    }

    fn has_primary_edgeuse(&self, e: H) -> bool {
        self.brep(e)
            .and_then(|b| eu1::primary_edgeuse(b, eid(e)).ok().flatten())
            .is_some()
    }
}

impl ClassGeometry<H> for Two {
    fn curve(&self, o: H) -> Option<&SmCurve> {
        match o.kind {
            K_EDGE_CURVE => self.brep(o)?.edge(eid(o)).ok()?.curve.as_ref(),
            K_CURVE => self.curves.get(o.i as usize),
            K_TRIM_CURVE => {
                eu1::uv_trim_curve(self.brep(o)?, EdgeuseId::from_index(o.i))
                    .ok()?
                    .0
            }
            K_EXTRA => self.extra.get(o.i as usize),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match o.kind {
            K_FACE_SURFACE => self.brep(o)?.face(fid(o)).ok()?.surface.as_ref(),
            _ => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: H) -> Option<Point3> {
        None
    }
    // What `intersect.curve_class_p5` asks of the operands' topology (as the
    // `query.face_p2` runner answers it): a vertex's point and the stored zone
    // tolerance of a vertex, edge or face. The vertex/face gap stays at the trait's
    // default (`None`): part 5's runner adapter supplies none either, and its
    // large-gap projections are uncertified on main for that reason.
    fn classification_vertex_point(&self, o: H) -> Option<Point3> {
        if o.kind != K_VERTEX {
            return None;
        }
        vx::point(self.brep(o)?, vid(o)).ok()
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        match o.kind {
            K_VERTEX | K_EDGE | K_FACE => self.topology_tolerance(o),
            _ => None,
        }
    }
}

impl ProtoGeometry<H> for Two {
    fn curve_mut(&mut self, o: H) -> Option<&mut SmCurve> {
        match o.kind {
            K_EDGE_CURVE => self.brep_mut(o)?.edge_mut(eid(o)).ok()?.curve.as_mut(),
            K_CURVE => self.curves.get_mut(o.i as usize),
            K_EXTRA => self.extra.get_mut(o.i as usize),
            _ => None,
        }
    }
}

/// The face-side objects of `curve_on_classify` for one B-rep.
struct Objs<'a> {
    two: &'a Two,
    b: i8,
}

impl ClassGeometry<H> for Objs<'_> {
    fn curve(&self, o: H) -> Option<&SmCurve> {
        ClassGeometry::curve(self.two, o)
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        ClassGeometry::surface(self.two, o)
    }
    fn volume(&self, o: H) -> Option<&Volume> {
        ClassGeometry::volume(self.two, o)
    }
    fn point(&self, o: H) -> Option<Point3> {
        ClassGeometry::point(self.two, o)
    }
}

impl CurveOnClassifyObjects<H> for Objs<'_> {
    fn face(&self, f: FaceId) -> H {
        hd(self.b, K_FACE, f.index())
    }
    fn vertex(&self, v: VertexId) -> H {
        hd(self.b, K_VERTEX, v.index())
    }
    fn edge(&self, e: EdgeId) -> H {
        hd(self.b, K_EDGE, e.index())
    }
    fn vertexuse(&self, vu: VertexuseId) -> H {
        hd(self.b, K_VERTEXUSE, vu.index())
    }
    fn edgeuse(&self, eu: EdgeuseId) -> H {
        hd(self.b, K_EDGEUSE, eu.index())
    }
    fn edge_curve(&self, e: EdgeId) -> H {
        hd(self.b, K_EDGE_CURVE, e.index())
    }
    fn trim_curve(&self, eu: EdgeuseId) -> H {
        hd(self.b, K_TRIM_CURVE, eu.index())
    }
    fn face_surface(&self, f: FaceId) -> Option<H> {
        self.two
            .face_surface_h(hd(self.b, K_FACE, f.index()))
            .flatten()
    }
    fn loop_id(&self, tgt_loop: H) -> Option<LoopId> {
        (tgt_loop.kind == K_LOOP && tgt_loop.b == self.b).then(|| LoopId::from_index(tgt_loop.i))
    }
}

impl ProtoBrep<H> for Two {
    fn edge_interval(&self, edge: H) -> Option<Extent1d> {
        ed::interval(self.brep(edge)?, eid(edge)).ok()
    }
    fn edge_curve(&self, edge: H) -> Option<Option<H>> {
        self.edge_curve_h(edge)
    }
    fn topology_tolerance(&self, topo: H) -> Option<f64> {
        let b = self.brep(topo)?;
        match topo.kind {
            K_VERTEX => vx::tolerance(b, vid(topo)).ok(),
            K_EDGE => ed::tolerance(b, eid(topo)).ok(),
            K_FACE => fp2::tolerance(b, fid(topo)).ok(),
            _ => None,
        }
    }
    fn face_uv_domain(&self, face: H) -> Option<Extent2d> {
        fp2::uv_domain(self.brep(face)?, fid(face)).ok()
    }
    fn set_face_uv_domain(&mut self, face: H, domain: Extent2d) -> Option<()> {
        fp2::set_uv_domain(self.brep_mut(face)?, fid(face), &domain).ok()
    }
    fn face_surface(&self, face: H) -> Option<Option<H>> {
        self.face_surface_h(face)
    }
    fn edge_edgeuse_of_face(&self, edge: H, face: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        let face = (face.b == edge.b && face.kind == K_FACE).then(|| fid(face));
        Ok(ed::edgeuse_of_face(b, eid(edge), face)?.map(|eu| hd(edge.b, K_EDGEUSE, eu.index())))
    }
    fn edge_is_connected_to_face(&self, edge: H, face: H) -> solidcore::Result<bool> {
        let b = self.brep_r(edge)?;
        let face = (face.b == edge.b && face.kind == K_FACE).then(|| fid(face));
        ed::is_connected_to_face(b, eid(edge), face)
    }
    fn insert_curve(&mut self, curve: SmCurve) -> Option<H> {
        let i = u32::try_from(self.extra.len()).ok()?;
        let hh = curve_h(2, K_EXTRA, i, &curve);
        self.extra.push(curve);
        Some(hh)
    }
    fn vertex_common_edges(&self, v1: H, v2: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(v1)?;
        // (a vertex of the other B-rep, or no vertex, ends no edge of this one: upstream
        // finds no common edge)
        if v2.b != v1.b || v2.kind != K_VERTEX {
            return Ok(Vec::new());
        }
        Ok(vx::common_edges(b, vid(v1), Some(vid(v2)), None)?
            .into_iter()
            .map(|e| hd(v1.b, K_EDGE, e.index()))
            .collect())
    }
    fn face_curve_on_classify(
        &self,
        k: &Kernel,
        face: H,
        do_point_classify: bool,
        cc: &mut CurveClassification<H>,
    ) -> solidcore::Result<bool> {
        let b = self.brep_r(face)?;
        let objs = Objs {
            two: self,
            b: face.b,
        };
        curve_on_classify(
            k,
            b,
            fid(face),
            &objs,
            do_point_classify,
            cc,
            false,
            false,
            true,
        )
    }
}

/// The B-rep walks over one operand's own entities (as `Brep`'s own impl, through the
/// handles); an unknown handle is an invalid-input error, as a null dereference upstream.
impl ProtoTopology<H> for Two {
    fn vertex_use_faces(&self, vertex: H) -> solidcore::Result<Vec<Option<H>>> {
        let b = self.brep_r(vertex)?;
        let mut out = Vec::new();
        for vu in vx::vertexuses(b, vid(vertex))? {
            let face = match solidcore::topology::vertexuse::faceuse(b, vu)? {
                Some(fu) => eu1::faceuse_face(b, fu)?,
                None => None,
            };
            out.push(face.map(|f| hd(vertex.b, K_FACE, f.index())));
        }
        Ok(out)
    }
    fn edge_use_faces(&self, edge: H) -> solidcore::Result<Vec<Option<H>>> {
        let b = self.brep_r(edge)?;
        let mut out = Vec::new();
        for eu in ed::edgeuses(b, eid(edge), None, None)? {
            out.push(eu1::face(b, eu)?.map(|f| hd(edge.b, K_FACE, f.index())));
        }
        Ok(out)
    }
    fn edge_start_vertex(&self, edge: H) -> solidcore::Result<Option<H>> {
        Ok(ed::start_vertex(self.brep_r(edge)?, eid(edge))?
            .map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn edge_other_vertex(&self, edge: H, vertex: Option<H>) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        let v = vertex
            .filter(|v| v.b == edge.b && v.kind == K_VERTEX)
            .map(vid);
        Ok(ed::other_vertex(b, eid(edge), v)?.map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn vertex_point(&self, vertex: H) -> solidcore::Result<Point3> {
        vx::point(self.brep_r(vertex)?, vid(vertex))
    }
    fn edge_brep(&self, edge: H) -> solidcore::Result<Option<ObjectKey>> {
        Ok(eu1::edge_brep(self.brep_r(edge)?, eid(edge))?.map(Brep::key))
    }
    fn edge_faces(&self, edge: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(edge)?;
        Ok(ed::faces(b, eid(edge), None)?
            .into_iter()
            .map(|f| hd(edge.b, K_FACE, f.index()))
            .collect())
    }
}

pub(crate) fn p9_entity_ref(entity: EntityRef) -> ObjectRef {
    let (kind, index) = match entity {
        EntityRef::Brep(v) => (16011, v.index()),
        EntityRef::Shell(v) => (16013, v.index()),
        EntityRef::Face(v) => (16002, v.index()),
        EntityRef::Loop(v) => (16003, v.index()),
        EntityRef::Edge(v) => (16016, v.index()),
        EntityRef::Vertex(v) => (16021, v.index()),
        EntityRef::Region(v) => (16012, v.index()),
        EntityRef::Faceuse(v) => (16022, v.index()),
        EntityRef::Loopuse(v) => (16004, v.index()),
        EntityRef::Edgeuse(v) => (16005, v.index()),
        EntityRef::Vertexuse(v) => (16006, v.index()),
    };
    ObjectRef {
        key: ObjectKey((kind as u64) << 32 | u64::from(index)),
        type_code: kind,
    }
}

impl solidcore::topology::proto_topology::p3::BrepGlue<H> for Two {
    fn glue_vertices(
        &mut self,
        k: &mut Kernel,
        brep: H,
        v1: H,
        v2: H,
        opt_wire_edges_in_brep: Option<&[H]>,
    ) -> solidcore::Result<()> {
        if brep.kind != K_BREP
            || v1.kind != K_VERTEX
            || v2.kind != K_VERTEX
            || v1.b != brep.b
            || v2.b != brep.b
        {
            return Err(invalid("glue vertex owner mismatch"));
        }
        let wires = opt_wire_edges_in_brep
            .map(|edges| {
                edges
                    .iter()
                    .map(|&e| {
                        if e.kind == K_EDGE && e.b == brep.b {
                            Ok(eid(e))
                        } else {
                            Err(invalid("glue wire edge owner mismatch"))
                        }
                    })
                    .collect::<solidcore::Result<Vec<_>>>()
            })
            .transpose()?;
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("absent B-rep"))?;
        // The shim's B-reps carry no attributes, so the glue notifies through part 5's
        // arena-only state (every object's default record), as a plain caller does.
        solidcore::topology::brep::p5::glue_vertices(
            k,
            b,
            Some(vid(v1)),
            Some(vid(v2)),
            wires.as_deref(),
        )
    }
}

impl IntersectorBrep<H> for Two {
    fn make_vertex_loop(&mut self, brep: H, face: H, point: Point3) -> solidcore::Result<(H, H)> {
        if brep.kind != K_BREP || face.kind != K_FACE || brep.b != face.b {
            return Err(invalid("vertex loop owner mismatch"));
        }
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("absent B-rep"))?;
        let store = self
            .env
            .get_mut(slot)
            .ok_or_else(|| invalid("bad notification owner"))?;
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut store.records,
            attributes: &mut store.attributes,
            context: None,
            key_of: &p9_entity_ref,
        };
        let (loop_, vertex) = solidcore::topology::brep::p9::make_vertex_loop(
            b,
            fid(face),
            point,
            OrientType::Opposite,
            &mut env,
        )?;
        Ok((
            hd(brep.b, K_LOOP, loop_.index()),
            hd(brep.b, K_VERTEX, vertex.index()),
        ))
    }
    fn make_vertex_split_edge(
        &mut self,
        k: &mut Kernel,
        brep: H,
        edge: H,
        parameter: f64,
    ) -> solidcore::Result<(H, H, H)> {
        if brep.kind != K_BREP || edge.kind != K_EDGE || brep.b != edge.b {
            return Err(invalid("split edge owner mismatch"));
        }
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("absent B-rep"))?;
        let store = self
            .env
            .get_mut(slot)
            .ok_or_else(|| invalid("bad notification owner"))?;
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut store.records,
            attributes: &mut store.attributes,
            context: None,
            key_of: &p9_entity_ref,
        };
        let out = solidcore::topology::brep::p9::make_vertex_split_edge(
            k,
            b,
            eid(edge),
            parameter,
            &mut env,
            &|e, kind| ObjectRef {
                key: ObjectKey(((kind as u64) << 32) | u64::from(e.index())),
                type_code: kind,
            },
        )?;
        if !out.split {
            return Err(SolidError::sm_err("difftest::topology_intersector::split"));
        }
        let [a, z] = out
            .edges
            .ok_or_else(|| invalid("successful split without edges"))?;
        let v = out
            .vertex
            .ok_or_else(|| invalid("successful split without vertex"))?;
        Ok((
            hd(brep.b, K_EDGE, a.index()),
            hd(brep.b, K_EDGE, z.index()),
            hd(brep.b, K_VERTEX, v.index()),
        ))
    }

    fn brep_edges(&self, brep: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(brep)?;
        Ok(bp1::edges(b, None)?
            .into_iter()
            .map(|e| hd(brep.b, K_EDGE, e.index()))
            .collect())
    }
    fn brep_vertices(&self, brep: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(brep)?;
        Ok(bp1::vertices(b, None)?
            .into_iter()
            .map(|v| hd(brep.b, K_VERTEX, v.index()))
            .collect())
    }
    fn brep_arena(&self, brep: H) -> solidcore::Result<&Brep> {
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        self.brep_r(brep)
    }
    fn face_calculate_bounding_box(
        &self,
        k: &Kernel,
        face: H,
        tight: bool,
    ) -> solidcore::Result<Extent3d> {
        let b = self.brep_r(face)?;
        solidcore::query::face::p1::calculate_bounding_box(k, b, fid(face), tight)
    }
    fn vertex_brep(&self, vertex: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(vertex)?;
        Ok(vx::brep(b, vid(vertex))?.map(|_| brep_h(vertex.b)))
    }
    fn edge_brep_handle(&self, edge: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        Ok(ed::brep(b, eid(edge))?.map(|_| brep_h(edge.b)))
    }
    fn set_vertex_point(&mut self, _k: &Kernel, vertex: H, point: Point3) -> solidcore::Result<()> {
        let slot = usize::try_from(vertex.b).map_err(|_| invalid("bad B-rep"))?;
        let (breps, env) = (&mut self.breps, &mut self.env);
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let env = env.get_mut(slot).ok_or_else(|| invalid("bad B-rep"))?;
        vx::set_point(
            b,
            vid(vertex),
            point,
            &mut env.records,
            &mut env.attributes,
            None,
            env.this,
            &vertexuse_ref,
        )
    }
    fn set_topology_tolerance(
        &mut self,
        _k: &Kernel,
        topo: H,
        tolerance: f64,
        update_only_if_larger: bool,
        cascade_to_boundaries: bool,
    ) -> solidcore::Result<()> {
        let b = self.brep_m(topo)?;
        match topo.kind {
            K_VERTEX => vx::set_tolerance(
                b,
                vid(topo),
                tolerance,
                update_only_if_larger,
                cascade_to_boundaries,
            ),
            K_EDGE => ed::set_tolerance(
                b,
                eid(topo),
                tolerance,
                update_only_if_larger,
                cascade_to_boundaries,
            ),
            _ => Err(invalid("SetTolerance of a non-vertex, non-edge")),
        }
    }
    fn update_object_tolerance(
        &mut self,
        k: &Kernel,
        topo: H,
        this_gap: f64,
        this_tol: f64,
        check_gaps: bool,
    ) -> solidcore::Result<()> {
        let target = match topo.kind {
            K_VERTEX => TopologyToUpdate::Vertex(vid(topo)),
            K_EDGE => TopologyToUpdate::Edge(eid(topo)),
            K_FACE => TopologyToUpdate::Face(fid(topo)),
            _ => return Err(invalid("UpdateObjectTolerance of a non-topology object")),
        };
        let b = self.brep_m(topo)?;
        update_object_tolerance(k, b, Some(target), this_gap, this_tol, check_gaps)
    }
    fn vertex_is_shell_vertex(&self, vertex: H) -> solidcore::Result<bool> {
        vx::is_shell_vertex(self.brep_r(vertex)?, vid(vertex))
    }
    fn edge_is_wire(&self, edge: H) -> solidcore::Result<bool> {
        ed::is_wire(self.brep_r(edge)?, eid(edge))
    }
    fn edge_is_lamina(&self, edge: H) -> solidcore::Result<bool> {
        ed::is_lamina(self.brep_r(edge)?, eid(edge))
    }
    fn edge_vertex(&self, edge: H) -> solidcore::Result<Option<H>> {
        Ok(ed::vertex(self.brep_r(edge)?, eid(edge))?.map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn edge_primary_edgeuse_orientation(&self, edge: H) -> solidcore::Result<Option<OrientType>> {
        let b = self.brep_r(edge)?;
        match eu1::primary_edgeuse(b, eid(edge))? {
            Some(eu) => Ok(Some(eu1::orientation(b, eu)?)),
            None => Ok(None),
        }
    }
    fn edge_bounding_box(&self, k: &Kernel, edge: H) -> solidcore::Result<Extent3d> {
        let b = self.brep_r(edge)?;
        let boxes = solidcore::query::edge::calculate_bounding_box(
            k,
            b,
            eid(edge),
            BoundingBoxRequest {
                normal: true,
                ..BoundingBoxRequest::default()
            },
            false,
        )?;
        boxes.normal.ok_or_else(|| invalid("no normal box"))
    }
    fn face_uv_domain_from_uv_trim_curves(
        &mut self,
        k: &Kernel,
        face: H,
    ) -> solidcore::Result<Extent2d> {
        if face.kind != K_FACE {
            return Err(invalid("not a face handle"));
        }
        let b = self.brep_m(face)?;
        Ok(
            solidcore::topology::face::p1::calculate_uv_domain_from_uv_trim_curves(
                k,
                b,
                fid(face),
                true,
                false,
                false,
            )?
            .domain,
        )
    }
    fn face_point_classify(
        &mut self,
        k: &Kernel,
        face: H,
        uv: Point2,
        pc: &mut PointClassification<H>,
    ) -> solidcore::Result<()> {
        // (mutable: the classification may create UV trim curves, as upstream)
        let b = self.brep_m(face)?;
        let mut pe: PointClassification<EntityRef> = PointClassification::new(pc.tolerance());
        let r = point_classify(
            k,
            b,
            fid(face),
            uv,
            &mut pe,
            PointClassifyOptions::default(),
        );
        // copy the classification back whatever the status (upstream's out-param)
        pc.set_class_object(
            pe.point_class(),
            pe.object().and_then(|e| Self::entity_h(face.b, e)),
        );
        pc.set_face(pe.face().and_then(|e| Self::entity_h(face.b, e)));
        pc.set_gap_3d(pe.deviation());
        pc.set_pre_snap_param(pe.pre_snap_param());
        if pe.are_parameters_set() {
            pc.set_t_param(pe.t_param(), false);
            pc.set_uv_param(pe.uv_param());
        } else {
            pc.unset_parameters();
        }
        r
    }
    fn face_intersect(
        &mut self,
        k: &Kernel,
        face1: H,
        face2: H,
        approx_tol_3d: f64,
        ang_tol_deg: f64,
        opt_coincidence_flag: Option<bool>,
    ) -> solidcore::Result<ti::FaceIntersection<H>> {
        if face1.kind != K_FACE || face2.kind != K_FACE || face1.b != face2.b {
            return Err(invalid("FaceIntersect needs two faces of one B-rep"));
        }
        let options = FaceIntersectOptions {
            want_this_classes: true,
            want_other_classes: true,
            want_cc_curves: true,
            coincidence: opt_coincidence_flag,
            ..FaceIntersectOptions::default()
        };
        let out = fp3::face_intersect(
            k,
            self.brep_r(face1)?,
            fid(face1),
            fid(face2),
            approx_tol_3d,
            ang_tol_deg,
            &options,
        )?;
        let bi = face1.b;

        // every curve the call made gets a handle in the curve store
        let mut curves_3d = Vec::new();
        for c in out.curves_3d {
            curves_3d.push(self.register_curve(c)?);
        }
        let cc3d: Vec<Option<H>> = self.register_curves(
            out.cc_curves_3d
                .map(|v| v.into_iter().map(Some).collect())
                .unwrap_or_default(),
        )?;
        let uv1 = self.register_curves(out.cc_this_uv_curves.unwrap_or_default())?;
        let uv2 = self.register_curves(out.cc_other_uv_curves.unwrap_or_default())?;

        // the classifications name their objects by handle
        let mut map = |o: IntersectObject| -> solidcore::Result<H> {
            let bad = || invalid("classification object without a handle");
            match o {
                IntersectObject::Curve(i) => cc3d.get(i).copied().flatten().ok_or_else(bad),
                IntersectObject::UvCurve(i, false) => uv1.get(i).copied().flatten().ok_or_else(bad),
                IntersectObject::UvCurve(i, true) => uv2.get(i).copied().flatten().ok_or_else(bad),
                IntersectObject::Surface(f) => Ok(hd(bi, K_FACE_SURFACE, f.index())),
                IntersectObject::EdgeCurve(e) => self
                    .edge_curve_h(hd(bi, K_EDGE, e.index()))
                    .flatten()
                    .ok_or_else(bad),
                IntersectObject::Trim(eu) => Ok(hd(bi, K_TRIM_CURVE, eu.index())),
                IntersectObject::Topology(e) => match e {
                    EntityRef::Vertexuse(vu) => Ok(hd(bi, K_VERTEXUSE, vu.index())),
                    other => Self::entity_h(bi, other).ok_or_else(bad),
                },
            }
        };
        let mut cc1 = Vec::new();
        for c in out.this_classes.unwrap_or_default() {
            cc1.push(c.map_objects(&mut map)?);
        }
        let mut cc2 = Vec::new();
        for c in out.other_classes.unwrap_or_default() {
            cc2.push(c.map_objects(&mut map)?);
        }
        Ok(ti::FaceIntersection {
            curves_3d,
            cc1,
            cc2,
            cc_curves: [
                cc3d.into_iter().flatten().collect(),
                uv1.into_iter().flatten().collect(),
                uv2.into_iter().flatten().collect(),
            ],
        })
    }
    fn editing_enabled(&self, brep: H) -> solidcore::Result<bool> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        self.breps
            .get(slot)
            .and_then(Option::as_ref)
            .map(Brep::editing_enabled)
            .ok_or_else(|| invalid("not a B-rep handle"))
    }
    fn brep_tolerance(&self, brep: H) -> solidcore::Result<f64> {
        Ok(self.brep_r(brep)?.tolerance())
    }
    fn set_editing_enabled(&mut self, brep: H, enabled: bool) -> solidcore::Result<bool> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("not a B-rep handle"))?;
        let old = b.editing_enabled();
        b.set_editing_enabled(enabled);
        Ok(old)
    }
    fn topology_objects(&self) -> &dyn TopologyObjects<H> {
        self
    }
    fn brep_glue_edges_general(
        &mut self,
        k: &mut Kernel,
        brep: H,
        surviving: Option<H>,
        relative_orientation: OrientType,
        edge_gap: f64,
        do_region_nesting: bool,
        to_delete: Option<H>,
        inside_faceuses: Option<&[H]>,
    ) -> solidcore::Result<()> {
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let edge = |h: Option<H>| -> solidcore::Result<Option<EdgeId>> {
            match h {
                None => Ok(None),
                Some(h) if h.kind == K_EDGE && h.b == brep.b => Ok(Some(eid(h))),
                Some(_) => Err(invalid("not an edge of this B-rep")),
            }
        };
        let (surviving, to_delete) = (edge(surviving)?, edge(to_delete)?);
        // (no face-use handles exist here: the only caller passes NULL)
        let inside: Option<&[solidcore::topology::FaceuseId]> = match inside_faceuses {
            None => None,
            Some([]) => Some(&[]),
            Some(_) => return Err(invalid("face-use handles are not modelled")),
        };
        let (breps, envs) = (&mut self.breps, &mut self.env);
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let env = envs.get_mut(slot).ok_or_else(|| invalid("bad B-rep"))?;
        let mut notify = ed::NotifyEnv {
            records: &mut env.records,
            attributes: &mut env.attributes,
            context: None,
            key_of: &glue_key_of,
        };
        p7::glue_edges_general(
            k,
            b,
            surviving,
            relative_orientation,
            edge_gap,
            do_region_nesting,
            to_delete,
            inside,
            &mut notify,
        )
    }
}

/// The identity a glue notification runs under: vertex uses as the vertex
/// environment's, edges, faces and edge uses by kind and index.
fn glue_key_of(e: EntityRef) -> ObjectRef {
    let (type_code, key) = match e {
        EntityRef::Vertexuse(vu) => return vertexuse_ref(vu),
        EntityRef::Face(f) => (fp2::TYPE, 1 + u64::from(f.index())),
        EntityRef::Edge(x) => (ed::TYPE, 1 << 40 | u64::from(x.index())),
        EntityRef::Edgeuse(x) => (16005, 2 << 40 | u64::from(x.index())),
        _ => (0, 3 << 40),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(key),
    }
}

impl TopologyObjects<H> for Two {
    /// The B-rep's cache (`SmBrep::GetBrepCache`), built from the B-rep on first use (the
    /// C++ topology solver builds it on demand) with this adapter's handles. The build
    /// starts with `SmBrep::CalculateBoundingBox` (unit `query.brep`), so until that
    /// lands the solves needing a cache defer there.
    fn brep_cache(&self, brep: H) -> solidcore::Result<&BrepCache<H>> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let cell = self.caches.get(slot).ok_or_else(|| invalid("bad B-rep"))?;
        if let Some(cache) = cell.get() {
            return Ok(cache);
        }
        let b = self
            .breps
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let bi = brep.b;
        let mut cache = BrepCache::new(BREP_TYPE)?;
        cache.build_trees(
            &Kernel::default(),
            CacheSource::Brep(b),
            |o| match o {
                CacheObject::Vertex(v) => hd(bi, K_VERTEX, v.index()),
                CacheObject::Edge(e) => hd(bi, K_EDGE, e.index()),
                CacheObject::Face(f) => hd(bi, K_FACE, f.index()),
                CacheObject::Curve(e) => b
                    .edge(e)
                    .ok()
                    .and_then(|x| x.curve.as_ref())
                    .map_or(hd(bi, K_EDGE_CURVE, e.index()), |c| {
                        curve_h(bi, K_EDGE_CURVE, e.index(), c)
                    }),
                CacheObject::Surface(f) => hd(bi, K_FACE_SURFACE, f.index()),
                CacheObject::Brep => brep_h(bi),
                _ => hd(bi, K_NULL as u8, 0),
            },
            &arena_identity,
        )?;
        let _ = cell.set(cache);
        cell.get().ok_or_else(|| invalid("B-rep cache not stored"))
    }
    fn curve(&self, object: H) -> solidcore::Result<&SmCurve> {
        ClassGeometry::curve(self, object).ok_or_else(|| invalid("not a curve handle"))
    }
    fn analytic_curve_handle(&self, _curve: H) -> Option<H> {
        None
    }
    fn vertex_point(&self, vertex: H) -> solidcore::Result<Point3> {
        vx::point(self.brep_r(vertex)?, vid(vertex))
    }
    fn vertex_tolerance(&self, vertex: H) -> solidcore::Result<f64> {
        vx::tolerance(self.brep_r(vertex)?, vid(vertex))
    }
    fn curve_edge(&self, curve: H) -> solidcore::Result<Option<H>> {
        Ok((curve.kind == K_EDGE_CURVE).then_some(hd(curve.b, K_EDGE, curve.i)))
    }
    fn edge_curve(&self, edge: H) -> solidcore::Result<Option<H>> {
        self.edge_curve_h(edge)
            .ok_or_else(|| invalid("not an edge handle"))
    }
    fn edge_interval(&self, edge: H) -> solidcore::Result<Extent1d> {
        ed::interval(self.brep_r(edge)?, eid(edge))
    }
    fn surface_face(&self, surface: H) -> solidcore::Result<Option<H>> {
        Ok((surface.kind == K_FACE_SURFACE).then_some(hd(surface.b, K_FACE, surface.i)))
    }
    fn face_surface(&self, face: H) -> solidcore::Result<H> {
        self.face_surface_h(face).flatten().ok_or_else(|| {
            SolidError::null_pointer("difftest::boolean_topology_intersector_p1::face_surface")
        })
    }
    fn surface(&self, surface: H) -> solidcore::Result<&Surface> {
        ClassGeometry::surface(self, surface).ok_or_else(|| invalid("not a surface handle"))
    }
    fn face_uv_domain(&self, face: H) -> solidcore::Result<Extent2d> {
        fp2::uv_domain(self.brep_r(face)?, fid(face))
    }
    fn face_tolerance(&self, face: H) -> solidcore::Result<f64> {
        fp2::tolerance(self.brep_r(face)?, fid(face))
    }
    fn surface_owner(&self, surface: H) -> solidcore::Result<Option<H>> {
        self.surface_face(surface)
    }
    fn shape_member(&self, _object: H) -> ShapeMember {
        ShapeMember::Other
    }
}

// ---- curves -------------------------------------------------------------------------------

fn rust_curve(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let cur = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

/// The shim's `put_curve` block of a B-spline curve.
fn put_curve(o: &mut Vec<f64>, c: Option<&SmCurve>) {
    let Some(c) = c else {
        o.push(-1.0);
        return;
    };
    if !c.is_kind_of(BSPLINE_CURVE_TYPE) {
        o.push(-2.0);
        return;
    }
    let CurveKind::BSpline(b) = c.kind() else {
        // other B-spline kinds never reach this runner (only plain registered curves)
        o.push(-2.0);
        return;
    };
    o.push(c.type_code() as f64);
    o.push(b.dim() as f64);
    match b.gw_nurb() {
        Some(nurb) => {
            o.push(1.0);
            o.push(nurb.n() as f64);
            o.push(nurb.degree() as f64);
            o.push(nurb.m() as f64);
            for p in nurb.cpts().iter().take(nurb.n() + 1) {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend(nurb.knots().iter().take(nurb.m() + 1));
        }
        None => o.push(0.0),
    }
}

/// `SmCurveClassification(pCurve, interval, NULL, tol)` built from the port's setters
/// (the constructor is unit `intersect.curve_class_p4`): the curve, the interval, the
/// source zone tolerance (`SM_UNINIT_TOL` when not positive) and one interval over the
/// whole range whose classifications carry the tolerance.
fn new_classification(
    k: &Kernel,
    curve: H,
    ivl: Extent1d,
    tol: f64,
) -> Result<CurveClassification<H>, SolidError> {
    let mut cc: CurveClassification<H> = CurveClassification::new(k);
    cc.set_curve(Some(curve), false);
    // the constructor's `SmZoneTol3d sSrcZoneTol3d` argument converts the double first
    // (`SM_USE_DEFAULT` -> the system zone tolerance, then `fabs`); `ReSet` then keeps it
    // only when positive and gives the first interval's points the converted value
    let tol = zone_tol3d(k, tol);
    cc.set_src_zone_tol_3d(if tol > 0.0 { tol } else { UNINIT_TOL });
    let _ = cc.set_interval(&ivl); // SetMinMax's status is ignored (inline SetInterval is void)
    cc.set_size(k, 1)?;
    let mut ci = CurveInterval::with_interval(k, &ivl, Some(&cc));
    ci.start.set_src_zone_tol_3d(tol);
    ci.mid.set_src_zone_tol_3d(tol);
    ci.end.set_src_zone_tol_3d(tol);
    *cc.at_mut(0)? = ci;
    Ok(cc)
}

/// `SmTsectCurveType` of its code (`resolve` refuses anything but 0..=6, as the FFI does).
fn tsect_type(code: i64) -> Result<TsectCurveType, HarnessError> {
    Ok(match code {
        0 => TsectCurveType::Unknown,
        1 => TsectCurveType::Touching,
        2 => TsectCurveType::Crossing,
        3 => TsectCurveType::Tangent,
        4 => TsectCurveType::Coincident,
        5 => TsectCurveType::NearTangent,
        6 => TsectCurveType::RegionBoundary,
        _ => return Err(harness("bad curve type")),
    })
}

/// An `SmPointClassificationType` code (100..=110, as the FFI accepts), else the
/// resolver's refusal.
fn cls_code(code: i64) -> Result<i64, String> {
    if i32::try_from(code)
        .ok()
        .and_then(PointClassType::from_code)
        .is_some()
    {
        Ok(code)
    } else {
        Err("bad_point_class".into())
    }
}

// ---- resolution -------------------------------------------------------------------------

/// Entity counts per B-rep: `[vertices, edges, faces]`.
#[derive(Debug, Clone, Copy)]
struct Counts {
    breps: [Option<[usize; 3]>; 2],
}

impl Counts {
    fn of(c: Option<[usize; 3]>, kind: u8) -> usize {
        match (c, kind) {
            (Some(c), K_VERTEX) => c[0],
            (Some(c), K_EDGE | K_EDGE_CURVE) => c[1],
            (Some(c), K_FACE | K_FACE_SURFACE) => c[2],
            _ => 0,
        }
    }

    /// Resolves a selector handle: `None` for null, `Some(Err)` when it names nothing.
    fn resolve(&self, h: Hin, curves: usize) -> Result<Option<H>, ()> {
        let [b, kind, sel] = h;
        if kind == K_NULL || sel < 0 {
            return Ok(None);
        }
        let kind = u8::try_from(kind).map_err(|_| ())?;
        let sel = u64::try_from(sel).map_err(|_| ())?;
        match kind {
            K_BREP => {
                let slot = usize::try_from(b).map_err(|_| ())?;
                if self.breps.get(slot).copied().flatten().is_none() {
                    return Err(());
                }
                Ok(Some(brep_h(b as i8)))
            }
            K_CURVE => {
                if curves == 0 {
                    return Err(());
                }
                Ok(Some(hd(2, kind, (sel % curves as u64) as u32)))
            }
            K_VERTEX | K_EDGE | K_FACE | K_EDGE_CURVE | K_FACE_SURFACE => {
                let slot = usize::try_from(b).map_err(|_| ())?;
                let n = Self::of(*self.breps.get(slot).ok_or(())?, kind);
                if n == 0 {
                    return Err(());
                }
                Ok(Some(hd(b as i8, kind, (sel % n as u64) as u32)))
            }
            _ => Err(()),
        }
    }
}

/// A resolved op: concrete handles (triples, `[-1, 8, -1]` null) and absolute
/// parameters; the layout the shim reads.
#[derive(Debug, Clone)]
struct ROp {
    op: OpIn,
}

/// Resolves the longest runnable prefix of a case's ops against the imported B-reps:
/// the resolved ops and, when an op was left out, the index and name of the first
/// unrunnable op.
/// What the resolution of a script reads besides its ops: the constructor, the initial
/// approximation tolerance and the largest magnitude of the B-rep blocks (the FFI's
/// `ScriptState`).
#[derive(Debug, Clone, Copy)]
struct Start {
    ctor: u8,
    approx: f64,
    block_max_abs: f64,
}

fn resolve_prefix(
    k: &Kernel,
    ops: &[OpIn],
    c: &Counts,
    two: &Two,
    start: Start,
) -> (Vec<ROp>, Option<(usize, String)>) {
    let mut n = ops.len();
    let mut cut: Option<(usize, String)> = None;
    loop {
        match resolve(k, ops.get(..n).unwrap_or(&[]), c, two, start) {
            Ok(r) => return (r, cut),
            Err(name) => {
                if n == 0 {
                    return (Vec::new(), Some((0, name)));
                }
                n -= 1;
                cut = Some((n, name));
            }
        }
    }
}

/// Resolves every op of `ops` against the imported B-reps; `Err(name)` names the
/// first op that cannot be run.
#[allow(clippy::too_many_lines)]
fn resolve(k: &Kernel, ops: &[OpIn], c: &Counts, two: &Two, start: Start) -> Result<Vec<ROp>, String> {
    let i_ctor = start.ctor;
    let mut approx = start.approx;
    let mut out = Vec::new();
    let mut curves = 0usize;
    let mut has_mgr = false;
    let mut mutated = false;
    // the intersector's B-reps (the FFI's `have_breps`)
    let mut have_breps = [true, c.breps[1].is_some()];
    let mut closed = false;
    for (idx, op) in ops.iter().enumerate() {
        if closed {
            return Err(format!("unresolvable_op{idx}_after_last"));
        }
        let fail = |what: &str| format!("unresolvable_op{idx}_{what}");
        let h = |x: Hin| -> Result<Hin, String> {
            c.resolve(x, curves)
                .map(h_to_triple)
                .map_err(|()| fail("handle"))
        };
        let hk = |x: Hin, kinds: &[u8], allow_null: bool| -> Result<Hin, String> {
            let r = c.resolve(x, curves).map_err(|()| fail("handle"))?;
            match r {
                None if allow_null => Ok(h_to_triple(None)),
                None => Err(fail("null")),
                Some(r) if kinds.contains(&r.kind) => Ok(h_to_triple(Some(r))),
                Some(_) => Err(fail("kind")),
            }
        };
        let list = |xs: &[Hin], kinds: &[u8], allow_null: bool| -> Result<Vec<Hin>, String> {
            xs.iter().map(|&x| hk(x, kinds, allow_null)).collect()
        };
        let takes_handles = !matches!(
            op,
            OpIn::Getters
                | OpIn::SetTols { .. }
                | OpIn::ClearSsi
                | OpIn::CanImprint { .. }
                | OpIn::Resolve
                | OpIn::Imprint
                | OpIn::ShellVerts { .. }
                | OpIn::WireLamina { .. }
                | OpIn::MgrNew { .. }
                | OpIn::CommonEdges
                | OpIn::CommonVertices
                | OpIn::IntersectTopo
                | OpIn::SetRels
                | OpIn::Iir
        );
        if mutated && takes_handles {
            return Err(fail("after_mutation"));
        }
        // part 2: what the FFI refuses
        if matches!(op, OpIn::IntersectTopo) && (!has_mgr || !have_breps[0] || !have_breps[1]) {
            return Err(fail("intersect_topology"));
        }
        if matches!(op, OpIn::SetRels) && !has_mgr {
            return Err(fail("set_relationships"));
        }
        if matches!(op, OpIn::CloseLoop { .. }) && (!have_breps[0] || !have_breps[1]) {
            return Err(fail("close_intersection_loop"));
        }
        if matches!(op, OpIn::Iir) && (i_ctor != 0 || !have_breps[0] || !have_breps[1]) {
            return Err(fail("intersect_insert_relate"));
        }
        // the FFI's hang guard of IntersectInsertRelate (HANG-25, HANG-16's class)
        if matches!(op, OpIn::Iir) && (approx == 0.0 || start.block_max_abs >= ffi2::HANG_COORD) {
            return Err(fail("intersect_insert_relate_hang"));
        }
        // and its time-budget guard (a large tolerance finishes, but slowly)
        if matches!(op, OpIn::Iir) && (approx.is_nan() || approx.abs() > ffi2::IIR_MAX_APPROX) {
            return Err(fail("intersect_insert_relate_time_budget"));
        }
        if let OpIn::SetTols { approx: a, .. } = op {
            approx = *a;
        }
        // GetCommonEdges / GetCommonVertices read only m_pBrep
        if matches!(op, OpIn::CommonEdges | OpIn::CommonVertices) && !have_breps[0] {
            return Err(fail("null_brep"));
        }
        if matches!(
            op,
            OpIn::FaceFace { .. } | OpIn::FaceFaceCoin { .. } | OpIn::Resolve | OpIn::Imprint
        ) && (!have_breps[0] || !have_breps[1])
        {
            return Err(fail("null_brep"));
        }
        let any = [
            K_VERTEX,
            K_EDGE,
            K_FACE,
            K_EDGE_CURVE,
            K_FACE_SURFACE,
            K_CURVE,
            K_BREP,
        ];
        let r = match op {
            OpIn::Relate { a, b } => OpIn::Relate {
                a: h(*a)?,
                b: h(*b)?,
            },
            OpIn::OtherMate { h: x } => OpIn::OtherMate { h: h(*x)? },
            OpIn::ThisMate { h: x } => OpIn::ThisMate { h: h(*x)? },
            OpIn::BrepMate { h: x } => OpIn::BrepMate { h: h(*x)? },
            OpIn::IsMated { a, b } => OpIn::IsMated {
                a: h(*a)?,
                b: h(*b)?,
            },
            OpIn::MappedBrep { ty } => OpIn::MappedBrep { ty: *ty },
            OpIn::CommonEdges => OpIn::CommonEdges,
            OpIn::CommonVertices => OpIn::CommonVertices,
            OpIn::SetSubset { list: l } => OpIn::SetSubset {
                list: list(l, &any, true)?,
            },
            OpIn::FixupEnd {
                dim,
                tol,
                pc_b,
                pc_o,
            } => OpIn::FixupEnd {
                dim: *dim,
                tol: *tol,
                pc_b: PcIn {
                    cls: cls_code(pc_b.cls)?,
                    h: h(pc_b.h)?,
                },
                pc_o: PcIn {
                    cls: cls_code(pc_o.cls)?,
                    h: h(pc_o.h)?,
                },
            },
            OpIn::FixupMid {
                dim,
                tol,
                pc_b,
                pc_o,
            } => OpIn::FixupMid {
                dim: *dim,
                tol: *tol,
                pc_b: PcIn {
                    cls: cls_code(pc_b.cls)?,
                    h: h(pc_b.h)?,
                },
                pc_o: PcIn {
                    cls: cls_code(pc_o.cls)?,
                    h: h(pc_o.h)?,
                },
            },
            OpIn::Getters => OpIn::Getters,
            OpIn::SetTols { approx, ang } => OpIn::SetTols {
                approx: *approx,
                ang: *ang,
            },
            OpIn::SetBreps { a, b } => {
                let ra = hk(*a, &[K_BREP], true)?;
                let rb = hk(*b, &[K_BREP], true)?;
                have_breps = [ra[1] != K_NULL, rb[1] != K_NULL];
                OpIn::SetBreps { a: ra, b: rb }
            }
            OpIn::RegisterSsi { s1, s2, curves: cs } => {
                let r = OpIn::RegisterSsi {
                    s1: hk(*s1, &[K_FACE_SURFACE], true)?,
                    s2: hk(*s2, &[K_FACE_SURFACE], true)?,
                    curves: cs.clone(),
                };
                for sc in cs {
                    if !(0..=6).contains(&sc.ty) {
                        return Err("bad_curve_type".into()); // SmTsectCurveType is 0..=6
                    }
                    curves += 1 + usize::from(sc.uv1.is_some()) + usize::from(sc.uv2.is_some());
                }
                r
            }
            OpIn::CheckSsi { s1, s2 } => OpIn::CheckSsi {
                s1: hk(*s1, &[K_FACE_SURFACE], true)?,
                s2: hk(*s2, &[K_FACE_SURFACE], true)?,
            },
            OpIn::ClearSsi => OpIn::ClearSsi,
            OpIn::CanImprint { ty } => OpIn::CanImprint { ty: cls_code(*ty)? },
            OpIn::Lowest { list: l } => OpIn::Lowest {
                list: list(l, &any, true)?,
            },
            OpIn::SetEdgeClass {
                e,
                t0,
                t1,
                tol,
                relative,
            } => {
                let e = hk(*e, &[K_EDGE], false)?;
                let (t0, t1) = if *relative {
                    let ivl = triple_to_h(e)
                        .and_then(|e| ProtoBrep::edge_interval(two, e))
                        .ok_or_else(|| fail("edge_interval"))?;
                    let len = ivl.max() - ivl.min();
                    (ivl.min() + t0 * len, ivl.min() + t1 * len)
                } else {
                    (*t0, *t1)
                };
                // (an inverted or empty interval is accepted: SmExtent1d(t0, t1) swaps)
                OpIn::SetEdgeClass {
                    e,
                    t0,
                    t1,
                    tol: *tol,
                    relative: false,
                }
            }
            OpIn::CheckInterior {
                e,
                t,
                tol,
                relative,
            } => {
                let e = hk(*e, &[K_EDGE], false)?;
                let t = if *relative {
                    let ivl = triple_to_h(e)
                        .and_then(|e| ProtoBrep::edge_interval(two, e))
                        .ok_or_else(|| fail("edge_interval"))?;
                    ivl.min() + t * (ivl.max() - ivl.min())
                } else {
                    *t
                };
                OpIn::CheckInterior {
                    e,
                    t,
                    tol: *tol,
                    relative: false,
                }
            }
            OpIn::SplitTwo { e1, e2, tol } => OpIn::SplitTwo {
                e1: hk(*e1, &[K_EDGE], false)?,
                e2: hk(*e2, &[K_EDGE], false)?,
                tol: *tol,
            },
            OpIn::ImprintFace {
                f,
                p,
                tol,
                on_surface,
            } => {
                let f = hk(*f, &[K_FACE], false)?;
                let p = if *on_surface {
                    let fh = triple_to_h(f).ok_or_else(|| fail("face"))?;
                    let dom =
                        ProtoBrep::face_uv_domain(two, fh).ok_or_else(|| fail("face_domain"))?;
                    let sh = two
                        .face_surface_h(fh)
                        .flatten()
                        .ok_or_else(|| fail("surface"))?;
                    let s = ClassGeometry::surface(two, sh).ok_or_else(|| fail("surface"))?;
                    let uv = dom.evaluate(p[0], p[1]);
                    let pt = s.evaluate_point(k, uv).map_err(|_| fail("evaluate"))?;
                    let n = s.evaluate_normal_or_zero(uv);
                    let q = pt + n * p[2];
                    [q.x, q.y, q.z]
                } else {
                    *p
                };
                OpIn::ImprintFace {
                    f,
                    p,
                    tol: *tol,
                    on_surface: false,
                }
            }
            OpIn::VertexFace {
                verts,
                faces,
                manifold,
                ignore,
            } => OpIn::VertexFace {
                verts: list(verts, &[K_VERTEX], true)?,
                faces: list(faces, &[K_FACE], true)?,
                manifold: *manifold,
                ignore: *ignore,
            },
            OpIn::EdgeFace {
                edges,
                faces,
                manifold,
                ignore,
            } => OpIn::EdgeFace {
                edges: list(edges, &[K_EDGE], true)?,
                faces: list(faces, &[K_FACE], true)?,
                manifold: *manifold,
                ignore: *ignore,
            },
            OpIn::FaceFace {
                verts,
                faces1,
                faces2,
                ignore,
                nesting,
            } => OpIn::FaceFace {
                verts: list(verts, &[K_VERTEX], false)?,
                faces1: list(faces1, &[K_FACE], false)?,
                faces2: list(faces2, &[K_FACE], false)?,
                ignore: *ignore,
                nesting: *nesting,
            },
            OpIn::FaceFaceCoin {
                edges,
                faces,
                ignore,
                nesting,
            } => OpIn::FaceFaceCoin {
                edges: list(edges, &[K_EDGE], true)?,
                faces: list(faces, &[K_FACE], false)?,
                ignore: *ignore,
                nesting: *nesting,
            },
            OpIn::Resolve | OpIn::Imprint | OpIn::ShellVerts { .. } | OpIn::WireLamina { .. } => {
                if !has_mgr {
                    return Err(fail("no_manager"));
                }
                op.clone()
            }
            OpIn::MgrNew { .. } => {
                if has_mgr || c.breps[1].is_none() {
                    return Err(fail("manager"));
                }
                has_mgr = true;
                op.clone()
            }
            OpIn::RemoveRel { a, b } => OpIn::RemoveRel {
                a: h(*a)?,
                b: h(*b)?,
            },
            OpIn::Squeeze { bv, ov } => OpIn::Squeeze {
                bv: hk(*bv, &[K_VERTEX], false)?,
                ov: hk(*ov, &[K_VERTEX], false)?,
            },
            OpIn::DeleteFaces { faces, keep } => OpIn::DeleteFaces {
                faces: list(faces, &[K_FACE], false)?,
                keep: match keep {
                    Some(k) => Some(list(k, &[K_EDGE], true)?),
                    None => None,
                },
            },
            OpIn::FixGaps { verts } => OpIn::FixGaps {
                verts: list(verts, &[K_VERTEX], false)?,
            },
            OpIn::IntersectTopo | OpIn::SetRels => op.clone(),
            OpIn::Iir => {
                closed = true;
                op.clone()
            }
            OpIn::FixupClass { b, o } => OpIn::FixupClass {
                b: resolve_cc(b, &hk, two, &fail)?,
                o: resolve_cc(o, &hk, two, &fail)?,
            },
            OpIn::FixupSplit { index, b, o } => {
                let b = resolve_cc(b, &hk, two, &fail)?;
                let o = resolve_cc(o, &hk, two, &fail)?;
                if *index >= b.ivls.len() || *index >= o.ivls.len() {
                    return Err(fail("interval_index"));
                }
                OpIn::FixupSplit {
                    index: *index,
                    b,
                    o,
                }
            }
            OpIn::EdgeEdge {
                edges1,
                edges2,
                manifold,
                ignore,
            } => OpIn::EdgeEdge {
                edges1: list(edges1, &[K_EDGE], true)?,
                edges2: list(edges2, &[K_EDGE], true)?,
                manifold: *manifold,
                ignore: *ignore,
            },
            OpIn::EdgeFaceCoin {
                verts,
                excluded,
                ignore,
                nesting,
            } => OpIn::EdgeFaceCoin {
                verts: list(verts, &[K_VERTEX], true)?,
                excluded: list(excluded, &[K_EDGE], true)?,
                ignore: *ignore,
                nesting: *nesting,
            },
            OpIn::CloseLoop { v } => OpIn::CloseLoop {
                v: hk(*v, &[K_VERTEX], true)?,
            },
        };
        if matches!(
            r,
            OpIn::SplitTwo { .. }
                | OpIn::ImprintFace { .. }
                | OpIn::VertexFace { .. }
                | OpIn::EdgeFace { .. }
                | OpIn::FaceFace { .. }
                | OpIn::FaceFaceCoin { .. }
                | OpIn::Imprint
                | OpIn::Squeeze { .. }
                | OpIn::DeleteFaces { .. }
                | OpIn::FixGaps { .. }
                | OpIn::Iir
                | OpIn::EdgeEdge { .. }
                | OpIn::EdgeFaceCoin { .. }
                | OpIn::CloseLoop { .. }
        ) {
            mutated = true;
        }
        out.push(ROp { op: r });
    }
    Ok(out)
}

/// A handle resolver: the handle, the kinds allowed and whether null is allowed.
type HandleOfKind<'a> = dyn Fn(Hin, &[u8], bool) -> Result<Hin, String> + 'a;

/// Resolves a classification input (part 2): its handles (a vertex classification
/// must name a vertex, the curve must be an edge's or a registered one) and, when
/// relative, its parameters against the edge's interval.
fn resolve_cc(
    cc: &CcIn,
    hk: &HandleOfKind<'_>,
    two: &Two,
    fail: &dyn Fn(&str) -> String,
) -> Result<CcIn, String> {
    let c = hk(cc.c, &[K_EDGE_CURVE, K_CURVE], false)?;
    let map: Box<dyn Fn(f64) -> f64> = if cc.relative {
        let ch = triple_to_h(c).ok_or_else(|| fail("curve"))?;
        if ch.kind != K_EDGE_CURVE {
            return Err(fail("relative_curve"));
        }
        let ivl = ProtoBrep::edge_interval(two, hd(ch.b, K_EDGE, ch.i))
            .ok_or_else(|| fail("edge_interval"))?;
        let (a, len) = (ivl.min(), ivl.max() - ivl.min());
        Box::new(move |t| a + t * len)
    } else {
        Box::new(|t| t)
    };
    let mut ivls = Vec::new();
    for iv in &cc.ivls {
        let mut pcs = iv.pcs.clone();
        for pc in &mut pcs {
            let any = [
                K_VERTEX,
                K_EDGE,
                K_FACE,
                K_EDGE_CURVE,
                K_FACE_SURFACE,
                K_CURVE,
                K_BREP,
            ];
            let kinds: &[u8] = if pc.cls == 104 { &[K_VERTEX] } else { &any };
            pc.h = hk(pc.h, kinds, pc.cls != 104)?;
            pc.t = pc.t.map(&map);
        }
        ivls.push(IvlIn {
            a: map(iv.a),
            b: map(iv.b),
            pcs,
        });
    }
    Ok(CcIn {
        c,
        t0: map(cc.t0),
        t1: map(cc.t1),
        tol: cc.tol,
        ivls,
        relative: false,
    })
}

/// The surface normal at `uv`, or zero when it cannot be evaluated.
trait NormalOrZero {
    fn evaluate_normal_or_zero(&self, uv: Point2) -> Vec3;
}

impl NormalOrZero for Surface {
    fn evaluate_normal_or_zero(&self, uv: Point2) -> Vec3 {
        let k = &Kernel::default();
        // a small finite-difference normal: enough to offset a test point off the surface
        let h = 1e-3;
        let (Ok(p), Ok(pu), Ok(pv)) = (
            self.evaluate_point(k, uv),
            self.evaluate_point(k, Point2::new(uv.x + h, uv.y)),
            self.evaluate_point(k, Point2::new(uv.x, uv.y + h)),
        ) else {
            return Vec3::new(0.0, 0.0, 0.0);
        };
        let n = (pu - p).cross(pv - p);
        let len = n.length();
        if len > 0.0 {
            n * (1.0 / len)
        } else {
            Vec3::new(0.0, 0.0, 0.0)
        }
    }
}

// ---- the script, C++ side ---------------------------------------------------------------

fn push_h(v: &mut Vec<f64>, h: Hin) {
    v.extend([h[0] as f64, h[1] as f64, h[2] as f64]);
}

fn push_list(v: &mut Vec<f64>, l: &[Hin]) {
    v.push(l.len() as f64);
    for h in l {
        push_h(v, *h);
    }
}

fn push_pc(v: &mut Vec<f64>, pc: &PcIn) {
    v.push(pc.cls as f64);
    push_h(v, pc.h);
}

fn encode(i: &Inputs, ops: &[ROp]) -> Result<Vec<f64>, HarnessError> {
    let mut v = blocks(i)?;
    v.push(f64::from(i.ctor));
    v.push(i.approx);
    v.push(i.ang);
    v.push(ops.len() as f64);
    for r in ops {
        match &r.op {
            OpIn::Relate { a, b } => {
                v.push(1.0);
                push_h(&mut v, *a);
                push_h(&mut v, *b);
            }
            OpIn::OtherMate { h } => {
                v.push(2.0);
                push_h(&mut v, *h);
            }
            OpIn::ThisMate { h } => {
                v.push(3.0);
                push_h(&mut v, *h);
            }
            OpIn::BrepMate { h } => {
                v.push(4.0);
                push_h(&mut v, *h);
            }
            OpIn::IsMated { a, b } => {
                v.push(5.0);
                push_h(&mut v, *a);
                push_h(&mut v, *b);
            }
            OpIn::MappedBrep { ty } => v.extend([6.0, *ty as f64]),
            OpIn::CommonEdges => v.push(7.0),
            OpIn::CommonVertices => v.push(8.0),
            OpIn::SetSubset { list } => {
                v.push(9.0);
                push_list(&mut v, list);
            }
            OpIn::FixupEnd {
                dim,
                tol,
                pc_b,
                pc_o,
            }
            | OpIn::FixupMid {
                dim,
                tol,
                pc_b,
                pc_o,
            } => {
                v.push(if matches!(r.op, OpIn::FixupEnd { .. }) {
                    11.0
                } else {
                    12.0
                });
                v.push(f64::from(*dim));
                v.push(*tol);
                push_pc(&mut v, pc_b);
                push_pc(&mut v, pc_o);
            }
            OpIn::Getters => v.push(13.0),
            OpIn::SetTols { approx, ang } => v.extend([14.0, *approx, *ang]),
            OpIn::SetBreps { a, b } => {
                v.push(15.0);
                push_h(&mut v, *a);
                push_h(&mut v, *b);
            }
            OpIn::RegisterSsi { s1, s2, curves } => {
                v.push(16.0);
                push_h(&mut v, *s1);
                push_h(&mut v, *s2);
                v.push(curves.len() as f64);
                for c in curves {
                    push_curve(&mut v, &c.c3)?;
                    for uv in [&c.uv1, &c.uv2] {
                        match uv {
                            Some(c) => {
                                v.push(1.0);
                                push_curve(&mut v, c)?;
                            }
                            None => v.push(0.0),
                        }
                    }
                    v.push(c.ty as f64);
                    v.push(c.dev);
                }
            }
            OpIn::CheckSsi { s1, s2 } => {
                v.push(17.0);
                push_h(&mut v, *s1);
                push_h(&mut v, *s2);
            }
            OpIn::ClearSsi => v.push(18.0),
            OpIn::CanImprint { ty } => v.extend([19.0, *ty as f64]),
            OpIn::Lowest { list } => {
                v.push(20.0);
                push_list(&mut v, list);
            }
            OpIn::SetEdgeClass { e, t0, t1, tol, .. } => {
                v.push(21.0);
                push_h(&mut v, *e);
                v.extend([*t0, *t1, *tol]);
            }
            OpIn::CheckInterior { e, t, tol, .. } => {
                v.push(22.0);
                push_h(&mut v, *e);
                v.extend([*t, *tol]);
            }
            OpIn::SplitTwo { e1, e2, tol } => {
                v.push(23.0);
                push_h(&mut v, *e1);
                push_h(&mut v, *e2);
                v.push(*tol);
            }
            OpIn::ImprintFace { f, p, tol, .. } => {
                v.push(24.0);
                push_h(&mut v, *f);
                v.extend(p);
                v.push(*tol);
            }
            OpIn::VertexFace {
                verts,
                faces,
                manifold,
                ignore,
            } => {
                v.push(25.0);
                push_list(&mut v, verts);
                push_list(&mut v, faces);
                v.extend([flag(*manifold), flag(*ignore)]);
            }
            OpIn::EdgeFace {
                edges,
                faces,
                manifold,
                ignore,
            } => {
                v.push(26.0);
                push_list(&mut v, edges);
                push_list(&mut v, faces);
                v.extend([flag(*manifold), flag(*ignore)]);
            }
            OpIn::FaceFace {
                verts,
                faces1,
                faces2,
                ignore,
                nesting,
            } => {
                v.push(27.0);
                push_list(&mut v, verts);
                push_list(&mut v, faces1);
                push_list(&mut v, faces2);
                v.extend([flag(*ignore), flag(*nesting)]);
            }
            OpIn::FaceFaceCoin {
                edges,
                faces,
                ignore,
                nesting,
            } => {
                v.push(28.0);
                push_list(&mut v, edges);
                push_list(&mut v, faces);
                v.extend([flag(*ignore), flag(*nesting)]);
            }
            OpIn::Resolve => v.push(29.0),
            OpIn::Imprint => v.push(30.0),
            OpIn::ShellVerts { first, swap } => v.extend([31.0, f64::from(*first), flag(*swap)]),
            OpIn::WireLamina {
                first,
                lamina,
                swap,
            } => {
                v.extend([32.0, f64::from(*first), flag(*lamina), flag(*swap)]);
            }
            OpIn::MgrNew {
                capture,
                override_tol,
            } => v.extend([33.0, *capture, *override_tol]),
            OpIn::RemoveRel { a, b } => {
                v.push(41.0);
                push_h(&mut v, *a);
                push_h(&mut v, *b);
            }
            OpIn::Squeeze { bv, ov } => {
                v.push(42.0);
                push_h(&mut v, *bv);
                push_h(&mut v, *ov);
            }
            OpIn::DeleteFaces { faces, keep } => {
                v.push(43.0);
                push_list(&mut v, faces);
                v.push(flag(keep.is_some()));
                push_list(&mut v, keep.as_deref().unwrap_or(&[]));
            }
            OpIn::FixGaps { verts } => {
                v.push(44.0);
                push_list(&mut v, verts);
            }
            OpIn::IntersectTopo => v.push(45.0),
            OpIn::SetRels => v.push(46.0),
            OpIn::Iir => v.push(47.0),
            OpIn::FixupClass { b, o } => {
                v.push(48.0);
                push_cc(&mut v, b);
                push_cc(&mut v, o);
            }
            OpIn::FixupSplit { index, b, o } => {
                v.push(49.0);
                v.push(*index as f64);
                push_cc(&mut v, b);
                push_cc(&mut v, o);
            }
            OpIn::EdgeEdge {
                edges1,
                edges2,
                manifold,
                ignore,
            } => {
                v.push(50.0);
                push_list(&mut v, edges1);
                push_list(&mut v, edges2);
                v.extend([flag(*manifold), flag(*ignore)]);
            }
            OpIn::EdgeFaceCoin {
                verts,
                excluded,
                ignore,
                nesting,
            } => {
                v.push(51.0);
                push_list(&mut v, verts);
                push_list(&mut v, excluded);
                v.extend([flag(*ignore), flag(*nesting)]);
            }
            OpIn::CloseLoop { v: bv } => {
                v.push(52.0);
                push_h(&mut v, *bv);
            }
        }
    }
    Ok(v)
}

/// A resolved classification input (`curve t0 t1 tol n (a b pci pci pci)*`).
fn push_cc(v: &mut Vec<f64>, cc: &CcIn) {
    push_h(v, cc.c);
    v.extend([cc.t0, cc.t1, cc.tol, cc.ivls.len() as f64]);
    for iv in &cc.ivls {
        v.extend([iv.a, iv.b]);
        for pc in &iv.pcs {
            v.push(pc.cls as f64);
            push_h(v, pc.h);
            v.extend([pc.t.unwrap_or(0.0), flag(pc.t.is_some()), pc.dev]);
        }
    }
}

/// The imported B-reps, their counts and the resolved script of a case.
struct Prepared {
    two: Two,
    ops: Vec<ROp>,
    /// The first unrunnable op (index, name), when the script was cut there.
    cut: Option<(usize, String)>,
}

/// The record's trailing marker of a cut script.
fn put_cut(o: &mut Vec<f64>, cut: Option<&(usize, String)>) {
    if let Some((idx, _)) = cut {
        o.push(-4.0);
        o.push(*idx as f64);
    }
}

/// Exports and imports the B-reps and resolves the script; `Err(Ok(output))` is the
/// output both sides report when the B-reps cannot be built or an op cannot be
/// resolved.
fn prepare(k: &Kernel, i: &Inputs) -> Result<Prepared, Box<Result<Output, HarnessError>>> {
    let ex = export(i).map_err(|e| Box::new(Err(e)))?;
    let Some((a, b)) = ffi::split_exports(&ex) else {
        return Err(Box::new(Ok(build_failure(&ex))));
    };
    let (brep_a, _) = import(&a).map_err(|e| Box::new(Err(e)))?;
    let brep_b = match &b {
        Some(b) => Some(import(b).map_err(|e| Box::new(Err(e)))?.0),
        None => None,
    };
    let count3 = |b: &Brep| [b.vertices().len(), b.edges().len(), b.faces().len()];
    let counts = Counts {
        breps: [Some(count3(&brep_a)), brep_b.as_ref().map(count3)],
    };
    let two = Two::new(brep_a, brep_b);
    // (NOW / NOZ markers are +-f64::MAX)
    let block_max_abs = blocks(i)
        .map_err(|e| Box::new(Err(e)))?
        .iter()
        .map(|v| v.abs())
        .filter(|v| *v < f64::MAX)
        .fold(0.0, f64::max);
    let start = Start {
        ctor: i.ctor,
        approx: i.approx,
        block_max_abs,
    };
    let (ops, cut) = resolve_prefix(k, &i.ops, &counts, &two, start);
    // a script cut at its first op compares nothing: both sides report the error (it is
    // counted as both-errored, not as a pass)
    if ops.is_empty() {
        if let Some((_, name)) = cut {
            return Err(Box::new(Ok(Output::err(name))));
        }
    }
    Ok(Prepared { two, ops, cut })
}

fn run_cpp(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    run_cpp_with(k, i, ffi::eval)
}

/// The shim entry a case runs through (part 1's, or part 2's with its ops).
pub(super) type Eval = fn(Op, [&str; 2], &[f64]) -> Result<Vec<f64>, solidcore_ffi::FfiError>;

/// [`run_cpp`] through `eval`.
pub(super) fn run_cpp_with(k: &Kernel, i: &Inputs, eval: Eval) -> Result<Output, HarnessError> {
    let p = match prepare(k, i) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let input = encode(i, &p.ops)?;
    let pa = path_of(Some(&i.brep_a))?;
    let pb = path_of(i.brep_b.as_ref())?;
    let mut out = eval(Op::Run, [&pa, &pb], &input).map_err(|e| harness(format!("c++: {e}")))?;
    if out.first() != Some(&0.0) {
        return Ok(build_failure(&out));
    }
    put_cut(&mut out, p.cut.as_ref());
    Ok(Output::new().values("state", out))
}

// ---- the script, Rust side --------------------------------------------------------------

/// A failure of the Rust side: an error to report or an unported callee.
enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Self::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

/// The status of a result (`0` success, the upstream code of an upstream error);
/// `None` with [`NULL_DEREF`] pushed for undefined behaviour upstream; `Err` for
/// everything else (unported callees are reported by the caller).
fn status<T>(r: Result<T, SolidError>, o: &mut Vec<f64>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(SolidError::Upstream { code, .. }) => {
            o.push(f64::from(code));
            Ok(None)
        }
        Err(SolidError::InvalidInput { .. }) => {
            o.push(NULL_DEREF);
            Ok(None)
        }
        Err(e) => Err(Fail::Solid(e)),
    }
}

fn pc_from(k: &PcIn, tol: f64) -> Result<PointClassification<H>, HarnessError> {
    let mut pc = PointClassification::new(tol);
    let cls = i32::try_from(k.cls)
        .ok()
        .and_then(PointClassType::from_code)
        .ok_or_else(|| harness("bad point class"))?;
    pc.set_class_object(cls, triple_to_h(k.h));
    Ok(pc)
}

fn put_pc(o: &mut Vec<f64>, pc: &PointClassification<H>) {
    o.push(f64::from(pc.point_class().code()));
    put_h(o, pc.object());
}

fn put_pc_full(o: &mut Vec<f64>, pc: &PointClassification<H>) {
    put_pc(o, pc);
    o.push(pc.pre_snap_param());
    o.push(pc.deviation());
    o.push(pc.t_param());
    o.push(flag(pc.are_parameters_set()));
}

/// The tolerance of a vertex, edge or face handle (`-1` for anything else).
fn tol_of(two: &Two, h: Option<H>) -> f64 {
    h.and_then(|h| ProtoBrep::topology_tolerance(two, h))
        .unwrap_or(-1.0)
}

fn put_hs(o: &mut Vec<f64>, hs: &[H]) {
    o.push(hs.len() as f64);
    for &h in hs {
        put_h(o, Some(h));
    }
}

fn reason_code(r: Reason) -> f64 {
    match r {
        Reason::NoReason => 0.0,
        Reason::FarFromGeom => 1.0,
        Reason::CloseToBoundary => 2.0,
    }
}

/// The Rust side's record and, per op run, where its output starts in the record (its
/// code; for the ops that report one, the status follows).
#[allow(clippy::too_many_lines)]
fn run_rust_script(i: &Inputs, p: Prepared) -> Result<(Vec<f64>, Vec<usize>), Fail> {
    let Prepared { mut two, ops, cut } = p;
    let mut k = Kernel::default();
    let hb = two.breps[1].as_ref().map(|_| brep_h(1));
    let mut ti: TopologyIntersector<H> = if i.ctor == 0 {
        TopologyIntersector::new(Some(brep_h(0)), hb, i.approx, i.ang)
    } else {
        let mut t = TopologyIntersector::with_context();
        t.set_primary_brep(Some(brep_h(0)));
        t.set_other_brep(hb);
        t.set_this_approx_tol_3d(i.approx);
        t.set_this_ang_tol_rad(i.ang);
        t
    };
    let mut o = vec![0.0]; // the shim's "both B-reps built"
    let mut registered = 0usize;
    let mut marks = Vec::new();
    for r in &ops {
        marks.push(o.len());
        match &r.op {
            OpIn::Relate { a, b } => {
                o.push(1.0);
                let (a, b) = (triple_to_h(*a), triple_to_h(*b));
                if a.is_none() || i.ctor != 0 {
                    o.push(NULL_DEREF);
                    continue;
                }
                status(ti.relate(&k, &mut two, a, b), &mut o)?;
                o.push(tol_of(&two, a));
                o.push(tol_of(&two, b));
            }
            OpIn::OtherMate { h } => {
                o.push(2.0);
                put_h(&mut o, ti.other_mate(triple_to_h(*h)));
            }
            OpIn::ThisMate { h } => {
                o.push(3.0);
                put_h(&mut o, ti.this_mate(triple_to_h(*h)));
            }
            OpIn::BrepMate { h } => {
                o.push(4.0);
                put_h(&mut o, ti.brep_mate(triple_to_h(*h)));
            }
            OpIn::IsMated { a, b } => {
                o.push(5.0);
                o.push(flag(ti.is_mated_pair(triple_to_h(*a), triple_to_h(*b))));
            }
            OpIn::MappedBrep { ty } => {
                o.push(6.0);
                if i.ctor != 0 {
                    o.push(NULL_DEREF);
                    continue;
                }
                let kind = match *ty {
                    VERTEX_TYPE => ClassObjectKind::Vertex,
                    EDGE_TYPE => ClassObjectKind::Edge,
                    FACE_TYPE => ClassObjectKind::Face,
                    _ => ClassObjectKind::Other,
                };
                let mut list = ti.mapped_brep_objects(kind)?;
                list.sort();
                put_hs(&mut o, &list);
            }
            OpIn::CommonEdges => {
                o.push(7.0);
                let (a, b) = ti.common_edges(&two)?;
                put_hs(&mut o, &a);
                put_hs(&mut o, &b);
            }
            OpIn::CommonVertices => {
                o.push(8.0);
                let (a, b) = ti.common_vertices(&two)?;
                put_hs(&mut o, &a);
                put_hs(&mut o, &b);
            }
            OpIn::SetSubset { list } => {
                o.push(9.0);
                let subset: Vec<H> = list.iter().filter_map(|&h| triple_to_h(h)).collect();
                ti.set_subset(&subset);
                o.push(0.0);
                match ti.subset() {
                    None => o.push(-1.0),
                    Some(s) => {
                        let v: Vec<H> = s.iter().copied().collect();
                        put_hs(&mut o, &v);
                    }
                }
            }
            OpIn::FixupEnd {
                dim,
                tol,
                pc_b,
                pc_o,
            }
            | OpIn::FixupMid {
                dim,
                tol,
                pc_b,
                pc_o,
            } => {
                let end = matches!(r.op, OpIn::FixupEnd { .. });
                o.push(if end { 11.0 } else { 12.0 });
                let mut b = pc_from(pc_b, *tol)?;
                let mut c = pc_from(pc_o, *tol)?;
                if end {
                    ti.fixup_end_point_class_mates(u64::from(*dim), &mut b, &mut c, None);
                } else {
                    ti.fixup_mid_point_class_mates(u64::from(*dim), &mut b, &mut c, None);
                }
                o.push(0.0);
                put_pc(&mut o, &b);
                put_pc(&mut o, &c);
            }
            OpIn::Getters => {
                o.push(13.0);
                put_h(&mut o, ti.primary_brep());
                put_h(&mut o, ti.other_brep());
                o.push(ti.this_approx_tol_3d());
                o.push(ti.this_ang_tol_rad());
            }
            OpIn::SetTols { approx, ang } => {
                o.push(14.0);
                ti.set_this_approx_tol_3d(*approx);
                ti.set_this_ang_tol_rad(*ang);
                o.push(ti.this_approx_tol_3d());
                o.push(ti.this_ang_tol_rad());
            }
            OpIn::SetBreps { a, b } => {
                o.push(15.0);
                ti.set_primary_brep(triple_to_h(*a));
                ti.set_other_brep(triple_to_h(*b));
                put_h(&mut o, ti.primary_brep());
                put_h(&mut o, ti.other_brep());
            }
            OpIn::RegisterSsi { s1, s2, curves } => {
                o.push(16.0);
                // RegisterExistingSSI (unit boolean.topology_intersector_p2) of the
                // script's curves, stored as the shim stores them (3D, then each UV
                // curve given), with null UV entries where none is given
                let mut c3s: Vec<Option<H>> = Vec::new();
                let mut u1s: Vec<Option<H>> = Vec::new();
                let mut u2s: Vec<Option<H>> = Vec::new();
                let mut tys: Vec<TsectCurveType> = Vec::new();
                let mut devs: Vec<f64> = Vec::new();
                let store = |two: &mut Two, c: &CrvIn| -> Result<H, Fail> {
                    let curve = rust_curve(c)?;
                    let idx = u32::try_from(two.curves.len()).map_err(|_| harness("store"))?;
                    let h = curve_h(2, K_CURVE, idx, &curve);
                    two.curves.push(curve);
                    Ok(h)
                };
                for sc in curves {
                    c3s.push(Some(store(&mut two, &sc.c3)?));
                    registered += 1;
                    for (uv, list) in [(&sc.uv1, &mut u1s), (&sc.uv2, &mut u2s)] {
                        match uv {
                            Some(uv) => {
                                list.push(Some(store(&mut two, uv)?));
                                registered += 1;
                            }
                            None => list.push(None),
                        }
                    }
                    tys.push(tsect_type(sc.ty)?);
                    devs.push(sc.dev);
                }
                let r = ti.register_existing_ssi(
                    &mut two,
                    triple_to_h(*s1),
                    triple_to_h(*s2),
                    &c3s,
                    &u1s,
                    &u2s,
                    &tys,
                    &devs,
                );
                status(r, &mut o)?;
            }
            OpIn::CheckSsi { s1, s2 } => {
                o.push(17.0);
                let r = ti.check_if_have_existing_ssi(&mut two, triple_to_h(*s1), triple_to_h(*s2));
                let Some(r) = status(r, &mut o)? else {
                    continue;
                };
                // (SM_ERR after a non-B-spline 3D curve comes with the outputs filled so far)
                if let Some(e) = &r.status {
                    o.pop();
                    o.push(match e {
                        SolidError::Upstream { code, .. } => f64::from(*code),
                        _ => NULL_DEREF,
                    });
                }
                o.push(flag(r.have_existing_ssi));
                o.push(r.curves_3d.len() as f64);
                for j in 0..r.curves_3d.len() {
                    let c3 = r.curves_3d.get(j).copied().flatten();
                    let u1 = r.uv_curves1.get(j).copied().flatten();
                    let u2 = r.uv_curves2.get(j).copied().flatten();
                    put_curve(&mut o, c3.and_then(|h| ClassGeometry::curve(&two, h)));
                    put_curve(&mut o, u1.and_then(|h| ClassGeometry::curve(&two, h)));
                    put_curve(&mut o, u2.and_then(|h| ClassGeometry::curve(&two, h)));
                    o.push(r.curve_types.get(j).map_or(-1.0, |t| *t as i64 as f64));
                    o.push(r.deviations.get(j).copied().unwrap_or(-1.0));
                }
            }
            OpIn::ClearSsi => {
                o.push(18.0);
                let _ = ti.clear_existing_ssi();
                o.push(0.0);
            }
            OpIn::CanImprint { ty } => {
                o.push(19.0);
                let cls = i32::try_from(*ty)
                    .ok()
                    .and_then(PointClassType::from_code)
                    .ok_or_else(|| harness("bad point class"))?;
                o.push(flag(ti::can_imprint(cls)));
            }
            OpIn::Lowest { list } => {
                o.push(20.0);
                let hs: Vec<Option<H>> = list.iter().map(|&h| triple_to_h(h)).collect();
                if hs.len() > 1 && hs.iter().any(Option::is_none) {
                    o.push(NULL_DEREF);
                    continue;
                }
                let mut sols: SolutionArray<H> = SolutionArray::with_capacity(hs.len());
                for h in hs {
                    let mut s = Solution::default();
                    s.objects[0] = h;
                    s.num_objects = 1;
                    sols.add(&s);
                }
                match ti::find_first_lowest_dimension_sol(&sols) {
                    Ok(idx) => o.push(idx as f64),
                    Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
                    Err(e) => return Err(e.into()),
                }
            }
            OpIn::SetEdgeClass { e, t0, t1, tol, .. } => {
                o.push(21.0);
                let eh = triple_to_h(*e);
                if !two.edge_ok(eh) || !eh.is_some_and(|e| two.has_primary_edgeuse(e)) {
                    o.push(NULL_DEREF);
                    continue;
                }
                let eh = eh.ok_or_else(|| harness("edge"))?;
                let curve = two
                    .edge_curve_h(eh)
                    .flatten()
                    .ok_or_else(|| harness("curve"))?;
                let ivl = Extent1d::from_min_max(*t0, *t1);
                let mut cc = new_classification(&k, curve, ivl, *tol)?;
                if status(ti::set_edge_class(&k, &two, &mut cc, eh), &mut o)?.is_none() {
                    continue;
                }
                o.push(cc.size() as f64);
                let ci = cc.at(0)?;
                put_pc_full(&mut o, &ci.start);
                put_pc_full(&mut o, &ci.mid);
                put_pc_full(&mut o, &ci.end);
            }
            OpIn::CheckInterior { e, t, tol, .. } => {
                o.push(22.0);
                let eh = triple_to_h(*e);
                if !two.edge_ok(eh) {
                    o.push(NULL_DEREF);
                    continue;
                }
                let eh = eh.ok_or_else(|| harness("edge"))?;
                match status(ti::check_interior_edge(&k, &two, eh, *t, *tol), &mut o)? {
                    Some(interior) => o.push(flag(interior)),
                    None => o.push(0.0), // bInterior == FALSE on trouble
                }
            }
            OpIn::SplitTwo { e1, e2, tol } => {
                o.push(23.0);
                let (a, b) = (triple_to_h(*e1), triple_to_h(*e2));
                if !two.edge_ok(a) || !two.edge_ok(b) {
                    o.push(NULL_DEREF);
                    continue;
                }
                let (a, b) = (
                    a.ok_or_else(|| harness("edge"))?,
                    b.ok_or_else(|| harness("edge"))?,
                );
                if let Some(r) = status(
                    ti::test_and_split_two_edges(&mut k, &mut two, a, b, *tol),
                    &mut o,
                )? {
                    o.extend([
                        r.dist_to_edge,
                        r.min_unstitched,
                        r.new_verts.len() as f64,
                        r.new_edges1.len() as f64,
                        r.new_edges2.len() as f64,
                    ]);
                }
            }
            OpIn::ImprintFace { f, p, tol, .. } => {
                o.push(24.0);
                let fh = triple_to_h(*f);
                if !two.face_ok(fh) {
                    o.push(NULL_DEREF);
                    continue;
                }
                let fh = fh.ok_or_else(|| harness("face"))?;
                let pt = Point3::new(p[0], p[1], p[2]);
                if let Some(r) = status(
                    ti::test_and_imprint_face(&k, &mut two, fh, pt, *tol),
                    &mut o,
                )? {
                    o.extend([
                        reason_code(r.reason),
                        r.dist_to_face,
                        flag(r.new_vertex.is_some()),
                    ]);
                }
            }
            OpIn::VertexFace {
                verts,
                faces,
                manifold,
                ignore,
            } => {
                o.push(25.0);
                let faces: Vec<Option<H>> = faces.iter().map(|&h| triple_to_h(h)).collect();
                if faces.iter().any(|f| f.is_some() && !two.face_ok(*f)) {
                    o.push(NULL_DEREF);
                    continue;
                }
                let mut vs: Vec<Option<H>> = verts.iter().map(|&h| triple_to_h(h)).collect();
                let r =
                    ti.vertex_face_intersect(&mut k, &mut two, &mut vs, &faces, *manifold, *ignore);
                let Some(r) = status(r, &mut o)? else {
                    continue;
                };
                put_hs(&mut o, &r.processed_verts);
                put_hs(&mut o, &r.processed_faces);
                o.push(r.max_vertex_gap);
                o.push(r.min_unstitched_vert_gap);
                o.push(vs.len() as f64);
                for v in &vs {
                    o.push(flag(v.is_some()));
                }
            }
            OpIn::EdgeFace {
                edges,
                faces,
                manifold,
                ignore,
            } => {
                o.push(26.0);
                let es: Vec<Option<H>> = edges.iter().map(|&h| triple_to_h(h)).collect();
                let fs: Vec<Option<H>> = faces.iter().map(|&h| triple_to_h(h)).collect();
                if es.iter().any(|e| e.is_some() && !two.edge_ok(*e))
                    || fs.iter().any(|f| f.is_some() && !two.face_ok(*f))
                {
                    o.push(NULL_DEREF);
                    continue;
                }
                let r = ti.edge_face_intersect(&mut k, &mut two, &es, &fs, *manifold, *ignore);
                let Some(r) = status(r, &mut o)? else {
                    continue;
                };
                o.push(r.new_verts.len() as f64);
                put_hs(&mut o, &r.split_edges);
                o.push(r.new_edges.len() as f64);
                put_hs(&mut o, &r.processed_faces);
                o.push(r.max_vertex_gap);
                o.push(r.min_unstitched_vert_gap);
            }
            OpIn::FaceFace {
                verts,
                faces1,
                faces2,
                ignore,
                nesting,
            } => {
                o.push(27.0);
                let vs: Vec<Option<H>> = verts.iter().map(|&h| triple_to_h(h)).collect();
                let f1: Vec<H> = faces1.iter().filter_map(|&h| triple_to_h(h)).collect();
                let f2: Vec<H> = faces2.iter().filter_map(|&h| triple_to_h(h)).collect();
                if vs.iter().any(Option::is_none)
                    || f1.len() != faces1.len()
                    || f2.len() != faces2.len()
                    || f1.iter().chain(f2.iter()).any(|f| !two.face_ok(Some(*f)))
                {
                    o.push(NULL_DEREF);
                    continue;
                }
                let vs: Vec<H> = vs.into_iter().flatten().collect();
                let r = ti.face_face_intersect(&mut k, &mut two, &vs, &f1, &f2, *ignore, *nesting);
                let Some(r) = status(r, &mut o)? else {
                    continue;
                };
                put_hs(&mut o, &r.processed_faces);
                o.push(r.new_faces.len() as f64);
                o.push(r.new_edges.len() as f64);
                o.push(r.max_gap);
                o.push(r.min_unstitched);
            }
            OpIn::FaceFaceCoin {
                edges,
                faces,
                ignore,
                nesting,
            } => {
                o.push(28.0);
                let es: Vec<Option<H>> = edges.iter().map(|&h| triple_to_h(h)).collect();
                let fs: Vec<H> = faces.iter().filter_map(|&h| triple_to_h(h)).collect();
                let mut ok = fs.len() == faces.len();
                for e in es.iter().flatten() {
                    for f in two.edge_faces(*e)? {
                        if !two.face_ok(Some(f)) {
                            ok = false;
                        }
                    }
                }
                if !ok {
                    o.push(NULL_DEREF);
                    continue;
                }
                let r = ti.face_face_coincidence(&k, &mut two, &es, &fs, *ignore, *nesting);
                let Some(r) = status(r, &mut o)? else {
                    continue;
                };
                put_hs(&mut o, &r.processed_faces);
                put_hs(&mut o, &r.deleted_faces);
                o.push(r.max_gap);
                o.push(r.min_unstitched);
            }
            OpIn::MgrNew {
                capture,
                override_tol,
            } => {
                o.push(33.0);
                let mgr = ProtoTopologyManager::new(
                    &k,
                    (brep_h(0), two.brep_r(brep_h(0))?),
                    (brep_h(1), two.brep_r(brep_h(1))?),
                    TI_HANDLE,
                    ti.this_approx_tol_3d(),
                    *capture,
                    *override_tol,
                )?;
                let tol = mgr.tolerance(&k);
                ti.set_proto_topology_manager(Some(mgr));
                o.push(0.0);
                o.push(tol);
            }
            OpIn::Resolve => {
                o.push(29.0);
                status(ti.resolve_intersections(&k, &mut two), &mut o)?;
                let m = ti
                    .proto_topology_manager()
                    .ok_or_else(|| harness("no manager"))?;
                o.push(m.proto_vertices().len() as f64);
                o.push(m.proto_edges().len() as f64);
            }
            OpIn::Imprint => {
                o.push(30.0);
                status(ti.imprint_intersections(&mut k, &mut two), &mut o)?;
                let m = ti
                    .proto_topology_manager()
                    .ok_or_else(|| harness("no manager"))?;
                o.push(m.proto_vertices().len() as f64);
                o.push(m.proto_edges().len() as f64);
            }
            OpIn::ShellVerts { first, swap } | OpIn::WireLamina { first, swap, .. } => {
                let (b1, b2) = if *first == 0 {
                    (brep_h(0), brep_h(1))
                } else {
                    (brep_h(1), brep_h(0))
                };
                let ti_tol = ti.this_approx_tol_3d();
                let m = ti
                    .proto_topology_manager_mut()
                    .ok_or_else(|| harness("no manager"))?;
                let r = match &r.op {
                    OpIn::ShellVerts { .. } => {
                        o.push(31.0);
                        ti::intersect_shell_vertices(&k, &mut two, b1, b2, *swap, m)
                    }
                    OpIn::WireLamina { lamina, .. } => {
                        o.push(32.0);
                        ti::intersect_wire_lamina_edges(
                            &k, &mut two, b1, b2, *lamina, *swap, ti_tol, m,
                        )
                    }
                    _ => unreachable!(),
                };
                status(r, &mut o)?;
                o.push(m.proto_vertices().len() as f64);
                o.push(m.proto_edges().len() as f64);
            }
            _ => run_part2_op(i, &r.op, &mut k, &mut two, &mut ti, &mut o)?,
        }
    }
    let _ = registered;
    put_cut(&mut o, cut.as_ref());
    Ok((o, marks))
}

// ---- part 2's ops (unit boolean.topology_intersector_p2), Rust side -----------------------

/// The curve handle of a classification input (an edge's curve carries its natural
/// interval, as [`ProtoBrep::edge_curve`] gives it).
fn cc_curve_h(two: &Two, c: Hin) -> Result<H, Fail> {
    let h = triple_to_h(c).ok_or_else(|| harness("classification curve"))?;
    if h.kind == K_EDGE_CURVE {
        return two
            .edge_curve_h(hd(h.b, K_EDGE, h.i))
            .flatten()
            .ok_or_else(|| Fail::Harness(harness("edge curve")));
    }
    Ok(h)
}

/// `SmCurveClassification(curve, SmExtent1d(t0, t1), NULL, tol)`, `SetSize(n)` and each
/// interval `SmCurveInterval(SmExtent1d(a, b), ctx, &cc)` with its classifications set
/// (`SetClassObject`, `SetTParam` when given, `SetDeviation`), as the shim builds it.
fn build_cc(k: &Kernel, two: &Two, cc: &CcIn) -> Result<CurveClassification<H>, Fail> {
    let curve = cc_curve_h(two, cc.c)?;
    let mut out = new_classification(k, curve, Extent1d::from_min_max(cc.t0, cc.t1), cc.tol)?;
    out.set_size(k, cc.ivls.len())?;
    for (n, iv) in cc.ivls.iter().enumerate() {
        let mut ci =
            CurveInterval::with_interval(k, &Extent1d::from_min_max(iv.a, iv.b), Some(&out));
        for (pc, inp) in [&mut ci.start, &mut ci.mid, &mut ci.end]
            .into_iter()
            .zip(&iv.pcs)
        {
            let cls = PointClassType::from_code(inp.cls as i32)
                .ok_or_else(|| harness(format!("unknown point class {}", inp.cls)))?;
            pc.set_class_object(cls, triple_to_h(inp.h));
            if let Some(t) = inp.t {
                pc.set_t_param(t, false);
            }
            pc.set_deviation(inp.dev);
        }
        *out.at_mut(n)? = ci;
    }
    Ok(out)
}

fn put_cc(o: &mut Vec<f64>, cc: &CurveClassification<H>) {
    o.push(cc.size() as f64);
    for ci in cc.intervals() {
        o.push(ci.interval.min());
        o.push(ci.interval.max());
        put_pc_full(o, &ci.start);
        put_pc_full(o, &ci.mid);
        put_pc_full(o, &ci.end);
    }
}

/// The shim's `vertex_edges_ok`: every edge of the vertex has a curve and a vertex.
fn vertex_edges_ok(two: &Two, v: H) -> solidcore::Result<bool> {
    Ok(two
        .vertex_edges(v)?
        .into_iter()
        .all(|e| two.edge_ok(Some(e))))
}

/// The shim's `vertex_faces_ok`: every face of the vertex has a surface.
fn vertex_faces_ok(two: &Two, v: H) -> solidcore::Result<bool> {
    Ok(two
        .vertex_faces(v)?
        .into_iter()
        .all(|f| two.face_ok(Some(f))))
}

/// Runs one of part 2's ops (shim ops 41 to 51), with the shim's guards.
#[allow(clippy::too_many_lines)]
fn run_part2_op(
    i: &Inputs,
    op: &OpIn,
    k: &mut Kernel,
    two: &mut Two,
    ti: &mut TopologyIntersector<H>,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    let have_breps = ti.primary_brep().is_some() && ti.other_brep().is_some();
    match op {
        OpIn::RemoveRel { a, b } => {
            o.push(41.0);
            status(ti.remove_relationship(triple_to_h(*a), triple_to_h(*b)), o)?;
        }
        OpIn::Squeeze { bv, ov } => {
            o.push(42.0);
            let (Some(bv), Some(ov)) = (triple_to_h(*bv), triple_to_h(*ov)) else {
                o.push(NULL_DEREF);
                return Ok(());
            };
            // (a null B-rep where a combination would run, or a mate that is not a vertex
            // where upstream dereferences it, is the port's InvalidInput, reported as -2
            // like the shim's guard)
            let Some(r) = status(ti.check_for_squeeze_vertices(k, two, bv, ov), o)? else {
                return Ok(());
            };
            o.push(r.deleted_vertices.len() as f64);
            o.push(r.surviving_vertices.len() as f64);
            o.push(r.deleted_edges.len() as f64);
        }
        OpIn::DeleteFaces { faces, keep } => {
            o.push(43.0);
            let fs: Vec<H> = faces.iter().filter_map(|&h| triple_to_h(h)).collect();
            if fs.len() != faces.len() {
                o.push(NULL_DEREF);
                return Ok(());
            }
            let keep: Option<Vec<H>> = keep
                .as_ref()
                .map(|l| l.iter().filter_map(|&h| triple_to_h(h)).collect());
            status(ti.delete_faces(two, &fs, keep.as_deref()), o)?;
        }
        OpIn::FixGaps { verts } => {
            o.push(44.0);
            let vs: Vec<H> = verts.iter().filter_map(|&h| triple_to_h(h)).collect();
            // (a null B-rep or a non-vertex mate of the first squeeze candidate is the
            // port's InvalidInput, reported as -2 like the shim's guard)
            let mut ok = vs.len() == verts.len();
            for &v in &vs {
                if !vertex_edges_ok(two, v)? {
                    ok = false;
                }
            }
            if !ok {
                o.push(NULL_DEREF);
                return Ok(());
            }
            let Some(r) = status(ti.fix_gaps(k, two, &vs), o)? else {
                return Ok(());
            };
            o.push(flag(r.modified_topology));
            o.push(r.deleted_vertices.len() as f64);
            o.push(r.deleted_edges.len() as f64);
        }
        OpIn::IntersectTopo | OpIn::SetRels => {
            let topo = matches!(op, OpIn::IntersectTopo);
            o.push(if topo { 45.0 } else { 46.0 });
            if ti.proto_topology_manager().is_none() || (topo && !have_breps) {
                o.push(NULL_DEREF);
                return Ok(());
            }
            if topo {
                status(ti.intersect_topology(k, two), o)?;
            } else {
                // (without the maps, a definition with both finals is the port's
                // InvalidInput, which the shim refuses with -2 alone)
                let r = ti.set_relationships(k, two);
                if matches!(r, Err(SolidError::InvalidInput { .. })) {
                    o.push(NULL_DEREF);
                    return Ok(());
                }
                status(r, o)?;
            }
            let m = ti
                .proto_topology_manager()
                .ok_or_else(|| harness("no manager"))?;
            o.push(m.proto_vertices().len() as f64);
            o.push(m.proto_edges().len() as f64);
            o.push(m.proto_faces().len() as f64);
        }
        OpIn::Iir => {
            o.push(47.0);
            if i.ctor != 0 || !have_breps {
                o.push(NULL_DEREF);
                return Ok(());
            }
            status(ti.intersect_insert_relate(k, two, TI_HANDLE), o)?;
        }
        OpIn::FixupClass { b, o: oc } | OpIn::FixupSplit { b, o: oc, .. } => {
            let split = matches!(op, OpIn::FixupSplit { .. });
            o.push(if split { 49.0 } else { 48.0 });
            let mut cb = build_cc(k, two, b)?;
            let mut co = build_cc(k, two, oc)?;
            let r = match op {
                OpIn::FixupSplit { index, .. } => {
                    ti.fixup_split_vertex_pair(k, two, *index, &mut cb, &mut co)
                }
                _ => ti
                    .fixup_class_mates(k, two, &mut cb, &mut co)
                    .map(|()| false),
            };
            let Some(changed) = status(r, o)? else {
                return Ok(());
            };
            if split {
                o.push(flag(changed));
            }
            put_cc(o, &cb);
            put_cc(o, &co);
        }
        OpIn::EdgeEdge {
            edges1,
            edges2,
            manifold,
            ignore,
        } => {
            o.push(50.0);
            let e1: Vec<Option<H>> = edges1.iter().map(|&h| triple_to_h(h)).collect();
            let e2: Vec<Option<H>> = edges2.iter().map(|&h| triple_to_h(h)).collect();
            if e1
                .iter()
                .chain(&e2)
                .any(|e| e.is_some() && !two.edge_ok(*e))
            {
                o.push(NULL_DEREF);
                return Ok(());
            }
            let r = ti.edge_edge_intersect(k, two, &e1, &e2, *manifold, *ignore);
            let Some(r) = status(r, o)? else {
                return Ok(());
            };
            put_hs(o, &r.split_edges1);
            o.push(r.new_edges1.len() as f64);
            put_hs(o, &r.split_edges2);
            o.push(r.new_edges2.len() as f64);
            o.push(r.new_verts.len() as f64);
            o.push(r.max_edge_gap);
            o.push(r.min_unstitched_edge_gap);
        }
        OpIn::EdgeFaceCoin {
            verts,
            excluded,
            ignore,
            nesting,
        } => {
            o.push(51.0);
            let vs: Vec<Option<H>> = verts.iter().map(|&h| triple_to_h(h)).collect();
            let ex: Vec<Option<H>> = excluded.iter().map(|&h| triple_to_h(h)).collect();
            let has_null = vs.iter().any(Option::is_none);
            let mut ok = true;
            for &v in vs.iter().flatten() {
                if !vertex_edges_ok(two, v)? || !vertex_faces_ok(two, v)? {
                    ok = false;
                    continue;
                }
                for e in two.vertex_edges(v)? {
                    if has_null && two.edge_other_vertex(e, Some(v))?.is_none() {
                        ok = false;
                    }
                }
            }
            if !ok {
                o.push(NULL_DEREF);
                return Ok(());
            }
            let r = ti.edge_face_coincidence(k, two, &vs, &ex, *ignore, *nesting);
            let Some(r) = status(r, o)? else {
                return Ok(());
            };
            put_hs(o, &r.imprinted_edges);
            put_hs(o, &r.processed_faces);
            o.push(r.new_faces.len() as f64);
            put_hs(o, &r.split_faces);
            o.push(r.max_edge_gap);
            o.push(r.this_min_unstitched);
        }
        OpIn::CloseLoop { v } => {
            o.push(52.0);
            let bv = triple_to_h(*v);
            // (the shim's close_loop_edges_ok: the edges of both vertices have a curve and
            // vertices, the B-rep vertex's a primary edge use)
            if let Some(bv) = bv {
                let mut ok = vertex_edges_ok(two, bv)?;
                for e in two.vertex_edges(bv)? {
                    if two.edge_primary_edgeuse_orientation(e)?.is_none() {
                        ok = false;
                    }
                }
                if let Some(ov) = ti
                    .other_mate(Some(bv))
                    .filter(|m| m.class_kind() == ClassObjectKind::Vertex)
                {
                    if !vertex_edges_ok(two, ov)? {
                        ok = false;
                    }
                }
                if !ok {
                    o.push(NULL_DEREF);
                    return Ok(());
                }
            }
            let Some(made) = status(ti.close_intersection_loop(k, two, bv), o)? else {
                return Ok(());
            };
            o.push(flag(made));
            // (the shim's read-back: the other mate of each edge of the vertex)
            let bv = bv.ok_or_else(|| Fail::Harness(harness("success without a vertex")))?;
            let edges = two.vertex_edges(bv)?;
            o.push(edges.len() as f64);
            for e in edges {
                put_h(o, ti.other_mate(Some(e)));
            }
        }
        _ => return Err(Fail::Harness(harness("part 1 op in part 2's dispatch"))),
    }
    Ok(())
}

impl IntersectorBrepP2<H> for Two {
    fn brep_faces(&self, brep: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(brep)?;
        Ok(bp1::faces(b, None)?
            .into_iter()
            .map(|f| hd(brep.b, K_FACE, f.index()))
            .collect())
    }
    fn brep_combine_coincident_vertices(
        &mut self,
        k: &mut Kernel,
        brep: H,
        surviving: H,
        to_delete: H,
    ) -> solidcore::Result<Option<H>> {
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        let vertex = |h: H| -> solidcore::Result<VertexId> {
            if h.kind == K_VERTEX && h.b == brep.b {
                Ok(vid(h))
            } else {
                Err(invalid("not a vertex of this B-rep"))
            }
        };
        let (surviving, to_delete) = (vertex(surviving)?, vertex(to_delete)?);
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let (breps, envs) = (&mut self.breps, &mut self.env);
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let env = envs.get_mut(slot).ok_or_else(|| invalid("bad B-rep"))?;
        // (SmVertex::SetPoint notifies under the vertex environment's identities, as
        // set_vertex_point)
        let this = env.this;
        let key_of = move |e: EntityRef| match e {
            EntityRef::Vertex(_) => this,
            e => glue_key_of(e),
        };
        let mut notify = ed::NotifyEnv {
            records: &mut env.records,
            attributes: &mut env.attributes,
            context: None,
            key_of: &key_of,
        };
        let mut deleted_edge = None;
        bp5::combine_with_notifications(
            k,
            b,
            surviving,
            to_delete,
            &mut deleted_edge,
            Some(&mut notify),
        )?;
        Ok(deleted_edge.map(|e| hd(brep.b, K_EDGE, e.index())))
    }
    fn edge_normalized_evaluate(
        &mut self,
        k: &Kernel,
        edge: H,
        t: f64,
    ) -> solidcore::Result<Point3> {
        let b = self.brep_m(edge)?;
        let eu = eu1::primary_edgeuse(b, eid(edge))?
            .ok_or_else(|| invalid("edge without primary edge use"))?;
        // (pOptFirstDeriv NULL, bOKToMakeUVTrimCurve TRUE by default)
        Ok(eu3::normalized_evaluate(k, b, eu, t, false, false, true)?.point)
    }
    fn vertex_edges(&self, vertex: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(vertex)?;
        Ok(vx::edges(b, vid(vertex), None)?
            .into_iter()
            .map(|e| hd(vertex.b, K_EDGE, e.index()))
            .collect())
    }
    fn vertex_faces(&self, vertex: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(vertex)?;
        Ok(vx::faces(b, vid(vertex))?
            .into_iter()
            .map(|f| hd(vertex.b, K_FACE, f.index()))
            .collect())
    }
}

pub(super) fn run_rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let p = match prepare(k, i) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match run_rust_script(i, p) {
        Ok((o, _)) => Ok(Output::new().values("state", o)),
        Err(Fail::Solid(e)) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => Err(harness(format!("rust: {e}"))),
        },
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// Per op run on the Rust side, its code and the two values after it (the status of the
/// ops that report one, and their first output); `None` for a deferred or unrunnable
/// case. For coverage censuses.
#[cfg(test)]
pub(super) fn rust_op_statuses(i: &Inputs) -> Option<Vec<(f64, f64, f64)>> {
    let k = &Kernel::default();
    let p = prepare(k, i).ok()?;
    let (o, marks) = run_rust_script(i, p).ok()?;
    Some(
        marks
            .iter()
            .map(|&m| {
                (
                    o.get(m).copied().unwrap_or(f64::NAN),
                    o.get(m + 1).copied().unwrap_or(f64::NAN),
                    o.get(m + 2).copied().unwrap_or(f64::NAN),
                )
            })
            .collect(),
    )
}

// ---- generation -----------------------------------------------------------------------------

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

fn clamped_knots(rng: &mut Rng, p: usize, n: usize, a: f64, b: f64) -> Vec<f64> {
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + (b - a) * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(ks);
    u.extend(vec![b; p + 1]);
    u
}

pub(super) fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|i| {
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            let k = if rational { w } else { 1.0 };
            let x = (i as f64 + rng.uniform(-0.3, 0.3)) * scale;
            let y = rng.uniform(-1.0, 1.0) * scale;
            let z = if dim == 2 {
                NOZ
            } else {
                rng.uniform(-1.0, 1.0) * scale * k
            };
            [x * k, y * k, z, w]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-5.0, 0.0), rng.uniform(1.0, 6.0))
    };
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

fn gen_surface(rng: &mut Rng) -> SrfIn {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    if rng.chance(0.25) {
        let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        return SrfIn::Plane { o, n };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 * scale,
                        j as f64 * scale,
                        rng.uniform(-0.5, 0.5) * scale,
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: rng.chance(0.2),
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

fn domain(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        SrfIn::Bspline(b) => [
            b.u.first().copied().unwrap_or(0.0),
            b.v.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
            b.v.last().copied().unwrap_or(1.0),
        ],
    }
}

pub(super) fn gen_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(10) {
        0..=5 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smb")).to_string(),
            rebuild: rng.chance(0.2),
        },
        6 => {
            let c = gen_curve(rng, 3, 1.0);
            let (a, b) = (
                c.u.first().copied().unwrap_or(0.0),
                c.u.last().copied().unwrap_or(1.0),
            );
            let (t0, t1) = if rng.chance(0.5) {
                (a, b)
            } else {
                (
                    a + (b - a) * rng.uniform(0.0, 0.3),
                    b - (b - a) * rng.uniform(0.0, 0.3),
                )
            };
            BrepIn::Wire { curve: c, t0, t1 }
        }
        7 | 8 => {
            let s = gen_surface(rng);
            let [u0, v0, u1, v1] = domain(&s);
            let dom = if rng.chance(0.5) {
                [u0, v0, u1, v1]
            } else {
                let (du, dv) = (u1 - u0, v1 - v0);
                [
                    u0 + du * rng.uniform(0.0, 0.3),
                    v0 + dv * rng.uniform(0.0, 0.3),
                    u1 - du * rng.uniform(0.0, 0.3),
                    v1 - dv * rng.uniform(0.0, 0.3),
                ]
            };
            BrepIn::Face { surface: s, dom }
        }
        _ => BrepIn::Network,
    }
}

pub(super) fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 16) as i64
}

/// What the generator may draw: the handle kinds each B-rep has (a wire or network
/// B-rep has no faces) and whether registered curves exist yet.
pub(super) struct Gen {
    pub(super) kinds: [&'static [u8]; 2],
    pub(super) has_b: bool,
    pub(super) has_curves: bool,
}

const FACE_KINDS: &[u8] = &[K_VERTEX, K_EDGE, K_FACE, K_EDGE_CURVE, K_FACE_SURFACE];
const WIRE_KINDS: &[u8] = &[K_VERTEX, K_EDGE, K_EDGE_CURVE];

impl Gen {
    pub(super) fn new(a: &BrepIn, b: Option<&BrepIn>) -> Self {
        let kinds_of = |x: &BrepIn| match x {
            BrepIn::File { .. } | BrepIn::Face { .. } => FACE_KINDS,
            BrepIn::Wire { .. } | BrepIn::Network => WIRE_KINDS,
        };
        Self {
            kinds: [kinds_of(a), b.map_or(&[], kinds_of)],
            has_b: b.is_some(),
            has_curves: false,
        }
    }
}

/// A random handle selector of one of `kinds` that exists (null with probability
/// `null`, or when no requested kind exists on the B-rep drawn).
pub(super) fn gen_h(rng: &mut Rng, g: &Gen, kinds: &[u8], null: f64) -> Hin {
    if rng.chance(null) {
        return [-1, K_NULL, -1];
    }
    let b = i64::from(g.has_b && rng.chance(0.5));
    let ok: Vec<u8> = kinds
        .iter()
        .copied()
        .filter(|&k| match k {
            K_BREP => true,
            K_CURVE => g.has_curves,
            _ => g.kinds.get(b as usize).is_some_and(|ks| ks.contains(&k)),
        })
        .collect();
    let Some(&kind) = rng.choose(&ok) else {
        return [-1, K_NULL, -1];
    };
    if kind == K_BREP {
        return [b, i64::from(K_BREP), b];
    }
    if kind == K_CURVE {
        return [2, i64::from(K_CURVE), gen_sel(rng)];
    }
    [b, i64::from(kind), gen_sel(rng)]
}

pub(super) fn gen_list(rng: &mut Rng, g: &Gen, kinds: &[u8], null: f64, max: u64) -> Vec<Hin> {
    (0..rng.below(max + 1))
        .map(|_| gen_h(rng, g, kinds, null))
        .collect()
}

pub(super) fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-5, 1e-5, 1e-3, 1e-6, 1e-2, 0.1, 1.0])
        .unwrap_or(&1e-5)
}

pub(super) fn gen_pc(rng: &mut Rng, g: &Gen) -> PcIn {
    let cls = *rng
        .choose(&[104, 104, 103, 103, 102, 100, 101])
        .unwrap_or(&104);
    let kinds: &[u8] = match cls {
        104 => &[K_VERTEX, K_VERTEX, K_EDGE],
        103 => &[K_EDGE, K_EDGE, K_VERTEX],
        102 => &[K_FACE],
        _ => &[K_VERTEX, K_EDGE, K_FACE],
    };
    PcIn {
        cls,
        h: gen_h(rng, g, kinds, 0.1),
    }
}

#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Rng, g: &mut Gen, has_mgr: &mut bool, last: bool) -> OpIn {
    let has_b = g.has_b;
    let topo: [u8; 3] = [K_VERTEX, K_EDGE, K_FACE];
    let any: [u8; 7] = [
        K_VERTEX,
        K_EDGE,
        K_FACE,
        K_EDGE_CURVE,
        K_FACE_SURFACE,
        K_CURVE,
        K_BREP,
    ];
    // SetBreps anywhere (it is not a mutating op): exercises the null-B-rep guard
    if rng.chance(0.04) {
        return OpIn::SetBreps {
            a: gen_h(rng, g, &[K_BREP], 0.25),
            b: gen_h(rng, g, &[K_BREP], 0.25),
        };
    }
    let pick = rng.below(if last { 30 } else { 21 });
    match pick {
        0..=2 => OpIn::Relate {
            a: gen_h(rng, g, &topo, 0.03),
            b: gen_h(rng, g, &topo, 0.05),
        },
        3 => OpIn::OtherMate {
            h: gen_h(rng, g, &any, 0.1),
        },
        4 => OpIn::ThisMate {
            h: gen_h(rng, g, &any, 0.1),
        },
        5 => OpIn::BrepMate {
            h: gen_h(rng, g, &any, 0.1),
        },
        6 => OpIn::IsMated {
            a: gen_h(rng, g, &topo, 0.1),
            b: gen_h(rng, g, &topo, 0.1),
        },
        7 => OpIn::MappedBrep {
            ty: *rng
                .choose(&[VERTEX_TYPE, EDGE_TYPE, FACE_TYPE, 16013])
                .unwrap_or(&EDGE_TYPE),
        },
        8 => OpIn::SetSubset {
            list: gen_list(rng, g, &any, 0.1, 5),
        },
        9 => OpIn::FixupEnd {
            dim: if rng.chance(0.5) { 3 } else { 2 },
            tol: gen_tol(rng),
            pc_b: gen_pc(rng, g),
            pc_o: gen_pc(rng, g),
        },
        10 => OpIn::FixupMid {
            dim: if rng.chance(0.5) { 3 } else { 2 },
            tol: gen_tol(rng),
            pc_b: gen_pc(rng, g),
            pc_o: gen_pc(rng, g),
        },
        11 => OpIn::Getters,
        12 => OpIn::SetTols {
            approx: gen_tol(rng),
            ang: rng.uniform(0.0, 0.5),
        },
        13 => {
            g.has_curves = true;
            OpIn::RegisterSsi {
                s1: gen_h(rng, g, &[K_FACE_SURFACE], 0.15),
                s2: gen_h(rng, g, &[K_FACE_SURFACE], 0.3),
                curves: (0..rng.below(3))
                    .map(|_| SsiCurve {
                        c3: gen_curve(rng, 3, 1.0),
                        uv1: rng.chance(0.6).then(|| gen_curve(rng, 2, 1.0)),
                        uv2: rng.chance(0.6).then(|| gen_curve(rng, 2, 1.0)),
                        ty: rng.below(7) as i64,
                        dev: rng.uniform(0.0, 1e-3),
                    })
                    .collect(),
            }
        }
        14 | 15 => OpIn::CheckSsi {
            s1: gen_h(rng, g, &[K_FACE_SURFACE], 0.15),
            s2: gen_h(rng, g, &[K_FACE_SURFACE], 0.3),
        },
        16 => OpIn::ClearSsi,
        17 => OpIn::CanImprint {
            ty: 100 + rng.below(11) as i64,
        },
        18 => OpIn::Lowest {
            list: gen_list(rng, g, &[K_VERTEX, K_EDGE, K_FACE, K_EDGE_CURVE], 0.05, 5),
        },
        19 => OpIn::SetEdgeClass {
            e: gen_h(rng, g, &[K_EDGE], 0.0),
            t0: rng.uniform(-0.2, 0.6),
            t1: rng.uniform(0.6, 1.2),
            tol: gen_tol(rng),
            relative: true,
        },
        20 => OpIn::CheckInterior {
            e: gen_h(rng, g, &[K_EDGE], 0.0),
            t: rng.uniform(-0.2, 1.2),
            tol: gen_tol(rng),
            relative: true,
        },
        21 | 22 => OpIn::ImprintFace {
            f: gen_h(rng, g, &[K_FACE], 0.0),
            p: [
                rng.uniform(-0.2, 1.2),
                rng.uniform(-0.2, 1.2),
                rng.uniform(-1.0, 1.0) * rng.uniform(0.0, 1.0),
            ],
            tol: gen_tol(rng),
            on_surface: true,
        },
        23 => OpIn::SplitTwo {
            e1: gen_h(rng, g, &[K_EDGE], 0.0),
            e2: gen_h(rng, g, &[K_EDGE], 0.0),
            tol: gen_tol(rng),
        },
        24 | 25 => OpIn::VertexFace {
            verts: gen_list(rng, g, &[K_VERTEX], 0.1, 4),
            faces: gen_list(rng, g, &[K_FACE], 0.1, 4),
            manifold: rng.chance(0.5),
            ignore: rng.chance(0.5),
        },
        26 => OpIn::EdgeFace {
            edges: gen_list(rng, g, &[K_EDGE], 0.1, 3),
            faces: gen_list(rng, g, &[K_FACE], 0.1, 3),
            manifold: rng.chance(0.5),
            ignore: rng.chance(0.5),
        },
        27 => OpIn::FaceFace {
            verts: gen_list(rng, g, &[K_VERTEX], 0.0, 4),
            faces1: gen_list(rng, g, &[K_FACE], 0.0, 3),
            faces2: gen_list(rng, g, &[K_FACE], 0.0, 3),
            ignore: rng.chance(0.5),
            nesting: rng.chance(0.5),
        },
        28 => OpIn::FaceFaceCoin {
            edges: gen_list(rng, g, &[K_EDGE], 0.1, 4),
            faces: gen_list(rng, g, &[K_FACE], 0.0, 2),
            ignore: rng.chance(0.5),
            nesting: rng.chance(0.5),
        },
        _ => {
            if has_b && !*has_mgr {
                *has_mgr = true;
                OpIn::MgrNew {
                    capture: gen_tol(rng),
                    override_tol: if rng.chance(0.5) { 0.0 } else { gen_tol(rng) },
                }
            } else {
                OpIn::Getters
            }
        }
    }
}

fn generate(rng: &mut Rng) -> Option<Value> {
    let brep_a = gen_brep(rng);
    let has_b = rng.chance(0.7);
    let brep_b = has_b.then(|| gen_brep(rng));
    let mut g = Gen::new(&brep_a, brep_b.as_ref());
    let ctor = u8::from(rng.chance(0.08));
    let approx = *rng
        .choose(&[1e-5, 1e-5, 1e-4, 1e-3, 1e-6, 0.0])
        .unwrap_or(&1e-5);
    let ang = *rng
        .choose(&[20.0_f64.to_radians(), 0.1, 0.0, 1e-3])
        .unwrap_or(&0.1);
    let n = 1 + rng.below(7) as usize;
    let mut has_mgr = false;
    let mut ops = Vec::new();
    for j in 0..n {
        ops.push(gen_op(rng, &mut g, &mut has_mgr, j + 1 == n));
    }
    // a manager sequence after the handle ops, sometimes
    if has_b && rng.chance(0.25) {
        if !has_mgr {
            ops.push(OpIn::MgrNew {
                capture: gen_tol(rng),
                override_tol: 0.0,
            });
        }
        match rng.below(4) {
            0 => ops.push(OpIn::ShellVerts {
                first: rng.below(2) as u8,
                swap: rng.chance(0.5),
            }),
            1 => ops.push(OpIn::WireLamina {
                first: rng.below(2) as u8,
                lamina: rng.chance(0.5),
                swap: rng.chance(0.5),
            }),
            2 => ops.push(OpIn::Resolve),
            _ => {
                ops.push(OpIn::Resolve);
                ops.push(OpIn::Imprint);
            }
        }
    }
    serde_json::to_value(Inputs {
        brep_a,
        brep_b,
        ctor,
        approx,
        ang,
        ops,
    })
    .ok()
}

// ---- the runner ---------------------------------------------------------------------------

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.topology_intersector_p1"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        let i: Inputs = case.parse()?;
        run_cpp(k, &i)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        let i: Inputs = case.parse()?;
        run_rust(k, &i)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        generate(rng)
    }
}

/// Cases built in code: every op on the wire network and a file B-rep pair at unit,
/// tiny and huge scale.
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut add = |name: &str, i: Inputs| {
        if let Ok(v) = serde_json::to_value(i) {
            out.push((name.to_string(), v));
        }
    };
    let file = |f: &str| BrepIn::File {
        file: f.to_string(),
        rebuild: false,
    };
    let v = |b: i64, i: i64| [b, i64::from(K_VERTEX), i];
    let e = |b: i64, i: i64| [b, i64::from(K_EDGE), i];
    let f = |b: i64, i: i64| [b, i64::from(K_FACE), i];
    let null: Hin = [-1, K_NULL, -1];
    let base = |ops: Vec<OpIn>| Inputs {
        brep_a: BrepIn::Network,
        brep_b: Some(BrepIn::Network),
        ctor: 0,
        approx: 1e-5,
        ang: 0.1,
        ops,
    };

    add(
        "network_mates",
        base(vec![
            OpIn::Relate {
                a: v(0, 0),
                b: v(1, 1),
            },
            OpIn::Relate {
                a: e(0, 0),
                b: e(1, 2),
            },
            OpIn::Relate {
                a: v(0, 0),
                b: v(1, 2),
            }, // conflict: SM_ERR
            OpIn::Relate {
                a: v(0, 1),
                b: e(1, 0),
            }, // kinds differ: SM_ERR_NULL_POINTER
            OpIn::Relate {
                a: e(0, 1),
                b: null,
            }, // other null
            OpIn::OtherMate { h: v(0, 0) },
            OpIn::ThisMate { h: v(1, 1) },
            OpIn::BrepMate { h: e(1, 2) },
            OpIn::BrepMate { h: e(1, 0) },
            OpIn::IsMated {
                a: v(0, 0),
                b: v(1, 1),
            },
            OpIn::IsMated {
                a: v(0, 0),
                b: v(1, 0),
            },
            OpIn::IsMated {
                a: null,
                b: v(1, 1),
            },
            OpIn::MappedBrep { ty: VERTEX_TYPE },
            OpIn::MappedBrep { ty: EDGE_TYPE },
            OpIn::MappedBrep { ty: FACE_TYPE },
            OpIn::Getters,
        ]),
    );
    add(
        "network_context_only",
        Inputs {
            ctor: 1,
            ..base(vec![
                OpIn::OtherMate { h: v(0, 0) },
                OpIn::BrepMate { h: v(1, 0) },
                OpIn::IsMated {
                    a: v(0, 0),
                    b: v(1, 0),
                },
                OpIn::Relate {
                    a: v(0, 0),
                    b: v(1, 1),
                },
                OpIn::MappedBrep { ty: EDGE_TYPE },
                OpIn::Getters,
                OpIn::SetTols {
                    approx: 1e-3,
                    ang: 0.2,
                },
                OpIn::SetBreps {
                    a: [1, i64::from(K_BREP), 1],
                    b: null,
                },
                OpIn::Getters,
            ])
        },
    );
    add(
        "network_subset_fixups",
        base(vec![
            OpIn::SetSubset {
                list: vec![
                    e(1, 2),
                    v(1, 0),
                    e(1, 2),
                    null,
                    [0, i64::from(K_EDGE_CURVE), 1],
                ],
            },
            OpIn::SetSubset { list: vec![] },
            OpIn::Relate {
                a: v(0, 1),
                b: v(1, 3),
            },
            OpIn::Relate {
                a: e(0, 2),
                b: e(1, 1),
            },
            OpIn::FixupEnd {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 104,
                    h: v(0, 1),
                },
                pc_o: PcIn {
                    cls: 104,
                    h: v(1, 3),
                },
            },
            OpIn::FixupEnd {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 104,
                    h: v(0, 1),
                },
                pc_o: PcIn {
                    cls: 104,
                    h: v(1, 0),
                },
            },
            OpIn::FixupEnd {
                dim: 2,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 103,
                    h: e(0, 2),
                },
                pc_o: PcIn {
                    cls: 104,
                    h: v(1, 0),
                },
            },
            OpIn::FixupMid {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 103,
                    h: e(0, 2),
                },
                pc_o: PcIn {
                    cls: 103,
                    h: e(1, 1),
                },
            },
            OpIn::FixupMid {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 103,
                    h: e(0, 2),
                },
                pc_o: PcIn {
                    cls: 103,
                    h: e(1, 0),
                },
            },
            OpIn::FixupMid {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 104,
                    h: v(0, 1),
                },
                pc_o: PcIn {
                    cls: 104,
                    h: v(1, 0),
                },
            },
            OpIn::FixupMid {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn {
                    cls: 103,
                    h: e(0, 2),
                },
                pc_o: PcIn {
                    cls: 104,
                    h: v(1, 0),
                },
            },
            OpIn::FixupMid {
                dim: 3,
                tol: 1e-5,
                pc_b: PcIn { cls: 102, h: null },
                pc_o: PcIn { cls: 100, h: null },
            },
            OpIn::CanImprint { ty: 101 },
            OpIn::CanImprint { ty: 105 },
            OpIn::Lowest {
                list: vec![f(0, 0), e(0, 1), v(0, 2), e(0, 0)],
            },
            OpIn::Lowest {
                list: vec![e(0, 1), e(0, 0)],
            },
            OpIn::Lowest { list: vec![null] },
            OpIn::Lowest { list: vec![] },
        ]),
    );
    add(
        "network_edge_class",
        base(vec![
            OpIn::SetEdgeClass {
                e: e(0, 0),
                t0: 0.0,
                t1: 1.0,
                tol: 1e-5,
                relative: true,
            },
            OpIn::SetEdgeClass {
                e: e(0, 1),
                t0: 0.2,
                t1: 0.8,
                tol: 1e-5,
                relative: true,
            },
            OpIn::SetEdgeClass {
                e: e(1, 2),
                t0: -0.1,
                t1: 1.1,
                tol: 0.5,
                relative: true,
            },
            OpIn::CheckInterior {
                e: e(0, 0),
                t: 0.5,
                tol: 1e-5,
                relative: true,
            },
            OpIn::CheckInterior {
                e: e(0, 0),
                t: 0.0,
                tol: 1e-5,
                relative: true,
            },
            OpIn::CheckInterior {
                e: e(0, 0),
                t: 1.0,
                tol: 1e-5,
                relative: true,
            },
            OpIn::CheckInterior {
                e: e(0, 0),
                t: 1e-7,
                tol: 1e-5,
                relative: true,
            },
            OpIn::CheckInterior {
                e: e(0, 0),
                t: 0.5,
                tol: 10.0,
                relative: true,
            },
            OpIn::CheckInterior {
                e: e(0, 0),
                t: 1.5,
                tol: 1e-5,
                relative: true,
            },
            OpIn::SplitTwo {
                e1: e(0, 0),
                e2: e(0, 1),
                tol: 1e-5,
            },
        ]),
    );
    add(
        "network_split_other_brep",
        base(vec![
            OpIn::CheckInterior {
                e: e(1, 3),
                t: 0.5,
                tol: 1e-5,
                relative: true,
            },
            OpIn::SplitTwo {
                e1: e(0, 0),
                e2: e(1, 3),
                tol: 1e-5,
            },
        ]),
    );
    for (name, fa, fb) in [
        ("files_plane_sphere", "plane_square.smb", "sphere.smb"),
        ("files_cone_torus", "cone.smb", "torus.smb"),
        ("files_box_disk", "box_with_large_fillet.smb", "disk.smb"),
    ] {
        add(
            name,
            Inputs {
                brep_a: file(fa),
                brep_b: Some(file(fb)),
                ctor: 0,
                approx: 1e-5,
                ang: 0.1,
                ops: vec![
                    OpIn::Relate {
                        a: f(0, 0),
                        b: f(1, 0),
                    },
                    OpIn::Relate {
                        a: e(0, 0),
                        b: e(1, 0),
                    },
                    OpIn::Relate {
                        a: v(0, 0),
                        b: v(1, 0),
                    },
                    OpIn::MappedBrep { ty: FACE_TYPE },
                    OpIn::MappedBrep { ty: EDGE_TYPE },
                    OpIn::MappedBrep { ty: VERTEX_TYPE },
                    OpIn::RegisterSsi {
                        s1: [0, i64::from(K_FACE_SURFACE), 0],
                        s2: [1, i64::from(K_FACE_SURFACE), 0],
                        curves: vec![SsiCurve {
                            c3: CrvIn {
                                dim: 3,
                                cpts: vec![
                                    [0.0, 0.0, 0.0, NOW],
                                    [1.0, 2.0, 0.5, NOW],
                                    [2.0, 0.0, 1.0, NOW],
                                ],
                                p: 2,
                                u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                            },
                            uv1: Some(CrvIn {
                                dim: 2,
                                cpts: vec![[0.0, 0.0, NOZ, NOW], [1.0, 1.0, NOZ, NOW]],
                                p: 1,
                                u: vec![0.0, 0.0, 1.0, 1.0],
                            }),
                            uv2: None,
                            ty: 2,
                            dev: 1e-6,
                        }],
                    },
                    OpIn::RegisterSsi {
                        s1: [0, i64::from(K_FACE_SURFACE), 0],
                        s2: null,
                        curves: vec![],
                    },
                    OpIn::CheckSsi {
                        s1: [0, i64::from(K_FACE_SURFACE), 0],
                        s2: [1, i64::from(K_FACE_SURFACE), 0],
                    },
                    OpIn::CheckSsi {
                        s1: [1, i64::from(K_FACE_SURFACE), 0],
                        s2: [0, i64::from(K_FACE_SURFACE), 0],
                    },
                    OpIn::CheckSsi {
                        s1: [0, i64::from(K_FACE_SURFACE), 0],
                        s2: null,
                    },
                    OpIn::ClearSsi,
                    OpIn::CheckSsi {
                        s1: [0, i64::from(K_FACE_SURFACE), 0],
                        s2: [1, i64::from(K_FACE_SURFACE), 0],
                    },
                    OpIn::SetEdgeClass {
                        e: e(0, 0),
                        t0: 0.1,
                        t1: 0.9,
                        tol: 1e-5,
                        relative: true,
                    },
                    OpIn::CheckInterior {
                        e: e(1, 0),
                        t: 0.5,
                        tol: 1e-5,
                        relative: true,
                    },
                    OpIn::ImprintFace {
                        f: f(0, 0),
                        p: [0.5, 0.5, 1.0],
                        tol: 1e-5,
                        on_surface: true,
                    },
                ],
            },
        );
    }
    add(
        "files_vertex_face_far",
        Inputs {
            brep_a: file("plane_square.smb"),
            brep_b: Some(file("sphere.smb")),
            ctor: 0,
            approx: 1e-5,
            ang: 0.1,
            ops: vec![OpIn::VertexFace {
                verts: vec![v(1, 0), null, v(1, 1)],
                faces: vec![f(0, 0), null],
                manifold: false,
                ignore: true,
            }],
        },
    );
    add(
        "files_edge_face_connected",
        Inputs {
            brep_a: file("plane_square.smb"),
            brep_b: None,
            ctor: 0,
            approx: 1e-5,
            ang: 0.1,
            ops: vec![OpIn::EdgeFace {
                edges: vec![e(0, 0), e(0, 1)],
                faces: vec![f(0, 0)],
                manifold: false,
                ignore: false,
            }],
        },
    );
    add(
        "files_face_face_empty",
        Inputs {
            brep_a: file("cone.smb"),
            brep_b: Some(file("torus.smb")),
            ctor: 0,
            approx: 1e-5,
            ang: 0.1,
            ops: vec![OpIn::FaceFace {
                verts: vec![],
                faces1: vec![],
                faces2: vec![f(1, 0)],
                ignore: false,
                nesting: false,
            }],
        },
    );
    add(
        "files_cone_torus_coincidence",
        Inputs {
            brep_a: file("cone.smb"),
            brep_b: Some(file("torus.smb")),
            ctor: 0,
            approx: 1e-5,
            ang: 0.1,
            ops: vec![OpIn::FaceFaceCoin {
                edges: vec![null],
                faces: vec![],
                ignore: true,
                nesting: false,
            }],
        },
    );
    add(
        "files_manager_empty",
        Inputs {
            brep_a: file("plane_square.smb"),
            brep_b: Some(file("disk.smb")),
            ctor: 0,
            approx: 1e-5,
            ang: 0.1,
            ops: vec![
                OpIn::MgrNew {
                    capture: 1e-5,
                    override_tol: 0.0,
                },
                OpIn::Resolve,
                OpIn::Imprint,
            ],
        },
    );
    add(
        "files_manager_shell_vertices",
        Inputs {
            brep_a: BrepIn::Network,
            brep_b: Some(file("plane_square.smb")),
            ctor: 0,
            approx: 1e-5,
            ang: 0.1,
            ops: vec![
                OpIn::MgrNew {
                    capture: 1e-5,
                    override_tol: 0.0,
                },
                OpIn::ShellVerts {
                    first: 0,
                    swap: false,
                },
                OpIn::WireLamina {
                    first: 0,
                    lamina: true,
                    swap: true,
                },
                OpIn::Resolve,
            ],
        },
    );
    add(
        "files_manager_zero_tol",
        Inputs {
            brep_a: file("plane_square.smb"),
            brep_b: Some(file("disk.smb")),
            ctor: 0,
            approx: 0.0,
            ang: 0.1,
            ops: vec![
                OpIn::MgrNew {
                    capture: 1e-5,
                    override_tol: 0.0,
                },
                OpIn::Resolve,
            ],
        },
    );
    for (name, scale) in [("scale_tiny", 1e-6), ("scale_huge", 1e6)] {
        let s = scale;
        add(
            name,
            Inputs {
                brep_a: BrepIn::Wire {
                    curve: CrvIn {
                        dim: 3,
                        cpts: vec![
                            [0.0, 0.0, 0.0, NOW],
                            [s, s * 0.5, 0.0, NOW],
                            [2.0 * s, 0.0, s, NOW],
                        ],
                        p: 2,
                        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    },
                    t0: 0.0,
                    t1: 1.0,
                },
                brep_b: Some(BrepIn::Face {
                    surface: SrfIn::Plane {
                        o: [0.0, 0.0, 0.0],
                        n: [0.0, 0.0, 1.0],
                    },
                    dom: [-s, -s, s, s],
                }),
                ctor: 0,
                approx: 1e-5 * scale,
                ang: 0.1,
                ops: vec![
                    OpIn::SetEdgeClass {
                        e: e(0, 0),
                        t0: 0.3,
                        t1: 0.7,
                        tol: 1e-5 * scale,
                        relative: true,
                    },
                    OpIn::CheckInterior {
                        e: e(0, 0),
                        t: 0.5,
                        tol: 1e-5 * scale,
                        relative: true,
                    },
                    OpIn::CheckInterior {
                        e: e(0, 0),
                        t: 0.001,
                        tol: 1e-5 * scale,
                        relative: true,
                    },
                    OpIn::ImprintFace {
                        f: f(1, 0),
                        p: [0.5, 0.5, 0.5],
                        tol: 1e-5 * scale,
                        on_surface: true,
                    },
                ],
            },
        );
        add(
            &format!("{name}_on_face"),
            Inputs {
                brep_a: BrepIn::Face {
                    surface: SrfIn::Plane {
                        o: [0.0, 0.0, 0.0],
                        n: [0.0, 0.0, 1.0],
                    },
                    dom: [-s, -s, s, s],
                },
                brep_b: None,
                ctor: 0,
                approx: 1e-5 * scale,
                ang: 0.1,
                ops: vec![OpIn::ImprintFace {
                    f: f(0, 0),
                    p: [0.5, 0.5, 0.0],
                    tol: 1e-5 * scale,
                    on_surface: true,
                }],
            },
        );
    }
    out
}
