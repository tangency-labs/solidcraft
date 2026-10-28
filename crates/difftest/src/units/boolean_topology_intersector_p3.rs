//! Runner for unit `boolean.topology_intersector_p3`. C++:
//! `solidcore_ffi::units::boolean_topology_intersector_p3` (shim
//! `harness/cpp/shim/boolean_topology_intersector_p3.cpp`). Rust:
//! `solidcore::boolean::topology_intersector::p3`.
//!
//! **Two real B-reps.** Each case builds two `SmBrep`s on the C++ side (an upstream
//! `.smb` file, often the same file twice so that every edge has a coincident partner,
//! or the wire network of `prog_test` `my_test_topology`), makes an
//! `SmTopologyIntersector` over them, relates some vertex or edge pairs first, and runs
//! one of the unit's three merges. The Rust side builds the same B-reps with the Rust
//! kernel (the p1 runner's `own_breps`; no C++ export reaches it) into the
//! `boolean.topology_intersector_p1` runner's two-B-rep adapter (`Two`, extended here
//! with [`IntersectorBrepP3`]) and runs the same merge on a [`TopologyIntersector`].
//! Both sides produce one flat record: the pre-relation statuses, the merge's status
//! and outputs (only on success: the port returns none with an error), then the state
//! (counts, every vertex and edge tolerance, and the vertex, edge and face mates sorted
//! by handle). Handles are the p1 runner's `brep kind index` triples, numbered after the
//! merge by both sides' re-walk of the B-reps (the shim's `walk_all`, `Walk` here), so
//! records the merge deleted (kept unlinked in the Rust arena) are unknown on both.
//!
//! **Selectors.** Generated handles are selectors taken modulo the entity count of
//! their kind; classification parameters are fractions of the classified edge's
//! interval. Both sides resolve them the same way before the run.
//!
//! **Guards.** The shim refuses (`-2`) what upstream would dereference null:
//! `Relate` of a null object past its conflict checks, `MergeCurveOnSurfaces` without a
//! curve or with an edge without curve or vertex or a missing face on a side that is
//! used, `MergeCurveClasses` with a classification without curve or with a class object
//! of the wrong kind, `MergeCoincidentSurfaces` with a surface without face. The runner
//! mirrors those guards before calling the port.
//!
//! **Deferred.** `MergeCoincidentSurfaces` reaches `SmFace::CalculateBoundingBox`
//! (`query.face_p1`) first; the merges reach `SmFace::Point3DClassify` (`query.face_p1`)
//! for a degenerate curve, `SmFace::GetEdges` (`topology.face_p4`) through the face
//! classification, `SmBrep::DeleteEdge` (`topology.brep_p4`) when repairing an edge
//! relation, `CombineCoincidentVertices` (`topology.brep_p5`) when squeezing, the
//! global point solve of `DropPoint` (`curve.curve_p1`) when relating new edges. The
//! topology edits of `MergeClassifications` (`MergeInterval`, `MergePointClass`) and of
//! the degenerate curve's `MergeIntoObject` run on the adapter's B-reps
//! (`MergeIntervalBrep`, implemented by the p1 runner's `Two`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::boolean::topology_intersector::p1::TopologyIntersector;
use solidcore::boolean::topology_intersector::p3::{CurveOnSurfaces, IntersectorBrepP3};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::curve_class::p4::ClassificationInit;
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p1 as qf1;
use solidcore::topology::edge as ed;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p4 as fp4;
use solidcore::topology::loopuse as lu;
use solidcore::topology::proto_topology::ProtoBrep;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopuseId, VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::boolean_topology_intersector_p1 as ffi1;
use solidcore_ffi::units::boolean_topology_intersector_p3::{self as ffi, Op};

use super::boolean_topology_intersector_p1::{
    brep_block, brep_h, build_failure, own_breps, BrepPair, eid, fid, flag, harness, hd, path_of, put_h, BrepIn, Hin,
    Two, H, K_BREP, K_EDGE, K_EDGE_CURVE, K_FACE, K_FACE_SURFACE, K_NULL, K_TRIM_CURVE, K_VERTEX,
    K_VERTEXUSE,
    NULL_DEREF, TI_HANDLE,
};
use super::boolean_merge_p1::Canon;
use super::topology_vertexuse::import;
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

/// Edge-use, face-use and loop-use handles (Rust side only, read by the coincident
/// surfaces' face search over a face's upward loop uses).
const K_EDGEUSE: u8 = 10;
const K_FACEUSE: u8 = 15;
const K_LOOPUSE: u8 = 16;

// ---- inputs --------------------------------------------------------------------------------

/// A point classification `class h`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PcIn {
    cls: i64,
    h: Hin,
}

/// A classification over `[f0, f1]` (fractions of the curve's edge interval) of an edge
/// curve, with its single interval's start, mid and end classifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClassIn {
    curve: Hin,
    f0: f64,
    f1: f64,
    tol: f64,
    start: PcIn,
    mid: PcIn,
    end: PcIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "merge", rename_all = "snake_case")]
enum MergeIn {
    CurveOnSurfaces {
        s1: Hin,
        s2: Hin,
        tol: f64,
        curve: Hin,
        edge1: Hin,
        edge2: Hin,
        /// `pUVCurve1` / `pUVCurve2`: an edge use's UV trim curve (`K_TRIM_CURVE`), null
        /// by default.
        #[serde(default = "null_h")]
        uv1: Hin,
        #[serde(default = "null_h")]
        uv2: Hin,
    },
    CurveClasses {
        merge_singularities: bool,
        c1: ClassIn,
        c2: ClassIn,
    },
    CoincidentSurfaces {
        s1: Hin,
        s2: Hin,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep_a: BrepIn,
    #[serde(default)]
    brep_b: Option<BrepIn>,
    approx: f64,
    #[serde(default)]
    ang: f64,
    #[serde(default)]
    doing_fillets: bool,
    #[serde(default)]
    ignore_tangency: bool,
    #[serde(default)]
    relates: Vec<[Hin; 2]>,
    merge: MergeIn,
}

// ---- resolution ------------------------------------------------------------------------------

/// The entity counts of the two B-reps: `[vertices, edges, faces, edge uses]`.
struct Counts([Option<[usize; 4]>; 2]);

impl Counts {
    /// Resolves a selector handle: the index modulo the count of its kind (`None` for a
    /// null handle, a kind with no entities or an absent B-rep).
    fn resolve(&self, h: Hin) -> Option<H> {
        let [b, kind, sel] = h;
        let kind = u8::try_from(kind).ok()?;
        if i64::from(kind) == K_NULL {
            return None;
        }
        if kind == K_BREP {
            let b = i8::try_from(b).ok()?;
            self.0.get(usize::from(b.unsigned_abs()))?.as_ref()?;
            return Some(brep_h(b));
        }
        let c = self.0.get(usize::try_from(b).ok()?)?.as_ref()?;
        let n = match kind {
            K_VERTEX => c[0],
            K_EDGE | K_EDGE_CURVE => c[1],
            K_FACE | K_FACE_SURFACE => c[2],
            K_TRIM_CURVE => c[3],
            _ => return None,
        };
        if n == 0 {
            return None;
        }
        let i = u32::try_from(u64::try_from(sel).ok()? % n as u64).ok()?;
        Some(hd(i8::try_from(b).ok()?, kind, i))
    }
}

/// A resolved classification.
struct ClassR {
    curve: Option<H>,
    t0: f64,
    t1: f64,
    tol: f64,
    pcs: [(i64, Option<H>); 3],
}

enum MergeR {
    CurveOnSurfaces {
        s1: Option<H>,
        s2: Option<H>,
        tol: f64,
        curve: Option<H>,
        edge1: Option<H>,
        edge2: Option<H>,
        uv1: Option<H>,
        uv2: Option<H>,
    },
    CurveClasses {
        merge_singularities: bool,
        c1: ClassR,
        c2: ClassR,
    },
    CoincidentSurfaces {
        s1: Option<H>,
        s2: Option<H>,
    },
}

struct Prepared {
    two: Two,
    relates: Vec<(Option<H>, Option<H>)>,
    merge: MergeR,
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

fn paths(i: &Inputs) -> Result<[String; 2], HarnessError> {
    Ok([path_of(Some(&i.brep_a))?, path_of(i.brep_b.as_ref())?])
}

/// The edge interval of an edge-curve handle's edge.
fn edge_interval_of(two: &Two, curve: Option<H>) -> Option<Extent1d> {
    let c = curve?;
    ProtoBrep::edge_interval(two, hd(c.b, K_EDGE, c.i))
}

fn resolve_class(c: &ClassIn, counts: &Counts, two: &Two) -> ClassR {
    let curve = counts.resolve(c.curve);
    let (t0, t1) = edge_interval_of(two, curve)
        .map_or((0.0, 1.0), |ivl| (ivl.evaluate(c.f0), ivl.evaluate(c.f1)));
    ClassR {
        curve,
        t0,
        t1,
        tol: c.tol,
        pcs: [
            (c.start.cls, counts.resolve(c.start.h)),
            (c.mid.cls, counts.resolve(c.mid.h)),
            (c.end.cls, counts.resolve(c.end.h)),
        ],
    }
}

/// Puts each imported vertex's uses in upstream's list order. The shared import lists
/// them in arena order (its export does not carry the order), which `GetEdges` /
/// `GetVertexuses` and so the "first edge" choices of `sm_MakeConsistentClassifications`
/// read.
fn apply_vertex_use_order(
    out: &[f64],
    brep_a: &mut Brep,
    brep_b: Option<&mut Brep>,
) -> Result<(), HarnessError> {
    let bad = || harness("vertex use order: malformed output");
    if out.first() != Some(&0.0) {
        return Err(bad());
    }
    let mut at = 1usize;
    let next = |at: &mut usize| -> Result<f64, HarnessError> {
        let v = *out.get(*at).ok_or_else(bad)?;
        *at += 1;
        Ok(v)
    };
    for b in [Some(brep_a), brep_b] {
        let n = next(&mut at)?;
        let Some(b) = b else {
            if n != -1.0 {
                return Err(bad());
            }
            continue;
        };
        if n < 0.0 || n as usize != b.vertices().len() {
            return Err(bad());
        }
        for v in 0..b.vertices().len() {
            let k = next(&mut at)? as usize;
            let mut order = Vec::new();
            for _ in 0..k {
                let u = next(&mut at)?;
                // (a use the export's walk never registered is not in the import either:
                // the rest keep their relative order)
                if u >= 0.0 {
                    order.push(VertexuseId::from_index(u as u32));
                }
            }
            let id = VertexId::from_index(u32::try_from(v).map_err(|_| bad())?);
            let rec = b.vertex_mut(id).map_err(|e| harness(e.to_string()))?;
            let mut have = rec.vertexuses.clone();
            let mut want = order.clone();
            have.sort();
            want.sort();
            if have != want {
                return Err(harness(
                    "vertex use order: not a permutation of the imported uses",
                ));
            }
            rec.vertexuses = order;
        }
    }
    Ok(())
}

/// The B-reps a side resolves the merge on: the Rust build of the p1 runner's
/// [`own_breps`] (the Rust `.smb` reader and constructors, renumbered into the
/// fixture's traversal order, each vertex's uses in their native list order; no C++
/// export reaches the Rust side). The C++ side resolves on the same Rust build, so both
/// get the same numbers, and only when the Rust build fails on the import of its own
/// exports with upstream's vertex use order.
fn side_breps(
    rust: bool,
    i: &Inputs,
) -> Result<BrepPair, Box<Result<Output, HarnessError>>> {
    match own_breps(&i.brep_a, i.brep_b.as_ref()) {
        Ok(Ok(x)) => return Ok(x),
        Ok(Err(o)) if rust => return Err(Box::new(Ok(o))),
        Err(e) if rust => return Err(Box::new(Err(e))),
        _ => {}
    }
    let [pa, pb] = paths(i).map_err(|e| Box::new(Err(e)))?;
    let bl = blocks(i).map_err(|e| Box::new(Err(e)))?;
    let ex = ffi::eval(Op::Export, [&pa, &pb], &bl)
        .map_err(|e| Box::new(Err(harness(format!("c++ export: {e}")))))?;
    let Some((a, b)) = ffi1::split_exports(&ex) else {
        return Err(Box::new(Ok(build_failure(&ex))));
    };
    let (mut brep_a, _) = import(&a).map_err(|e| Box::new(Err(e)))?;
    let mut brep_b = match &b {
        Some(b) => Some(import(b).map_err(|e| Box::new(Err(e)))?.0),
        None => None,
    };
    let uses = ffi::eval(Op::VertexUses, [&pa, &pb], &bl)
        .map_err(|e| Box::new(Err(harness(format!("c++ vertex uses: {e}")))))?;
    apply_vertex_use_order(&uses, &mut brep_a, brep_b.as_mut()).map_err(|e| Box::new(Err(e)))?;
    Ok((brep_a, brep_b))
}

fn prepare(i: &Inputs, rust: bool) -> Result<Prepared, Box<Result<Output, HarnessError>>> {
    let (brep_a, brep_b) = side_breps(rust, i)?;
    let count4 = |b: &Brep| {
        [
            b.vertices().len(),
            b.edges().len(),
            b.faces().len(),
            b.edgeuses().len(),
        ]
    };
    let counts = Counts([Some(count4(&brep_a)), brep_b.as_ref().map(count4)]);
    let two = Two::new(brep_a, brep_b);
    let relates = i
        .relates
        .iter()
        .map(|[a, b]| (counts.resolve(*a), counts.resolve(*b)))
        .collect();
    let merge = match &i.merge {
        MergeIn::CurveOnSurfaces {
            s1,
            s2,
            tol,
            curve,
            edge1,
            edge2,
            uv1,
            uv2,
        } => MergeR::CurveOnSurfaces {
            s1: counts.resolve(*s1),
            s2: counts.resolve(*s2),
            tol: *tol,
            curve: counts.resolve(*curve),
            edge1: counts.resolve(*edge1),
            edge2: counts.resolve(*edge2),
            uv1: counts.resolve(*uv1),
            uv2: counts.resolve(*uv2),
        },
        MergeIn::CurveClasses {
            merge_singularities,
            c1,
            c2,
        } => MergeR::CurveClasses {
            merge_singularities: *merge_singularities,
            c1: resolve_class(c1, &counts, &two),
            c2: resolve_class(c2, &counts, &two),
        },
        MergeIn::CoincidentSurfaces { s1, s2 } => MergeR::CoincidentSurfaces {
            s1: counts.resolve(*s1),
            s2: counts.resolve(*s2),
        },
    };
    Ok(Prepared {
        two,
        relates,
        merge,
    })
}

// ---- the C++ side ----------------------------------------------------------------------------

fn put_class(v: &mut Vec<f64>, c: &ClassR) {
    put_h(v, c.curve);
    v.extend([c.t0, c.t1, c.tol]);
    for (cls, h) in c.pcs {
        v.push(cls as f64);
        put_h(v, h);
    }
}

fn encode(i: &Inputs, p: &Prepared) -> Result<Vec<f64>, HarnessError> {
    let mut v = blocks(i)?;
    v.extend([
        i.approx,
        i.ang,
        flag(i.doing_fillets),
        flag(i.ignore_tangency),
    ]);
    v.push(p.relates.len() as f64);
    for &(a, b) in &p.relates {
        put_h(&mut v, a);
        put_h(&mut v, b);
    }
    match &p.merge {
        MergeR::CurveOnSurfaces {
            s1,
            s2,
            tol,
            curve,
            edge1,
            edge2,
            uv1,
            uv2,
        } => {
            v.push(1.0);
            put_h(&mut v, *s1);
            put_h(&mut v, *s2);
            v.push(*tol);
            put_h(&mut v, *curve);
            put_h(&mut v, *edge1);
            put_h(&mut v, *edge2);
            put_h(&mut v, *uv1);
            put_h(&mut v, *uv2);
        }
        MergeR::CurveClasses {
            merge_singularities,
            c1,
            c2,
        } => {
            v.push(2.0);
            v.push(flag(*merge_singularities));
            put_class(&mut v, c1);
            put_class(&mut v, c2);
        }
        MergeR::CoincidentSurfaces { s1, s2 } => {
            v.push(3.0);
            put_h(&mut v, *s1);
            put_h(&mut v, *s2);
        }
    }
    Ok(v)
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let p = match prepare(i, false) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let input = encode(i, &p)?;
    let [pa, pb] = paths(i)?;
    let out = ffi::eval(Op::Run, [&pa, &pb], &input).map_err(|e| harness(format!("c++: {e}")))?;
    if out.first() != Some(&0.0) {
        return Ok(build_failure(&out));
    }
    Ok(Output::new().values("state", out))
}

// ---- the Rust side ----------------------------------------------------------------------------

/// The status code of a result (`0` success, the upstream code of an upstream error,
/// [`NULL_DEREF`] for undefined behaviour upstream); `Err` for anything else.
fn status<T>(r: Result<T, SolidError>, o: &mut Vec<f64>) -> Result<Option<T>, SolidError> {
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
        Err(e) => Err(e),
    }
}

fn put_list(o: &mut Vec<f64>, walk: &Walk, hs: &[H]) {
    o.push(hs.len() as f64);
    for &h in hs {
        walk.put(o, Some(h));
    }
}

/// The shim's `walk_all` after the merge: both B-reps re-walked
/// ([`Canon::reachable`], the shim's `walk`), so a vertex, edge or face handle (and an
/// edge curve's or face surface's) is its position in that walk, and a record the merge
/// deleted (unlinked, but kept in the Rust arena) is unknown, `-1 -3 -1`, as a deleted
/// object upstream. Without deletions the walk is the import order (the identity).
struct Walk([Option<Canon>; 2]);

impl Walk {
    fn of(two: &Two) -> Self {
        Self([0i8, 1].map(|b| two.brep(brep_h(b)).map(Canon::reachable)))
    }

    /// The post-merge triple of `h` (see [`Walk`]).
    fn triple(&self, h: Option<H>) -> [f64; 3] {
        let mut o = Vec::with_capacity(3);
        self.put(&mut o, h);
        [
            o.first().copied().unwrap_or(-1.0),
            o.get(1).copied().unwrap_or(-3.0),
            o.get(2).copied().unwrap_or(-1.0),
        ]
    }

    fn put(&self, o: &mut Vec<f64>, h: Option<H>) {
        let Some(h) = h else {
            put_h(o, None);
            return;
        };
        let canon = usize::try_from(h.b)
            .ok()
            .and_then(|b| self.0.get(b))
            .and_then(Option::as_ref);
        let at = match (canon, h.kind) {
            (Some(c), K_VERTEX) => c.vertices.of(Some(VertexId::from_index(h.i))),
            (Some(c), K_EDGE | K_EDGE_CURVE) => c.edges.of(Some(EdgeId::from_index(h.i))),
            (Some(c), K_FACE | K_FACE_SURFACE) => c.faces.of(Some(FaceId::from_index(h.i))),
            _ => {
                put_h(o, Some(h));
                return;
            }
        };
        match u32::try_from(at) {
            Ok(i) => put_h(o, Some(hd(h.b, h.kind, i))),
            Err(_) => o.extend([-1.0, -3.0, -1.0]),
        }
    }
}

/// `SmEdge` with a curve and a vertex (`sm_SetEdgeClass` dereferences both).
fn edge_ok(two: &Two, e: Option<H>) -> bool {
    let Some(e) = e else {
        return false;
    };
    let Some(b) = two.brep(e) else {
        return false;
    };
    b.edge(eid(e)).is_ok_and(|edge| edge.curve.is_some())
        && ed::vertex(b, eid(e)).ok().flatten().is_some()
}

/// The face of a face-surface handle, as `SmSurface::GetFace` (`None` when the face has
/// no such surface).
fn face_of_surface(two: &Two, s: H) -> Option<H> {
    if s.kind != K_FACE_SURFACE {
        return None;
    }
    let f = hd(s.b, K_FACE, s.i);
    two.face_surface_h(f)?.map(|_| f)
}

/// The shim's class-object check: a vertex for `SM_PC_VERTEX`, a usable edge for
/// `SM_PC_EDGE`, a face for `SM_PC_FACE`.
fn pc_ok(two: &Two, cls: i64, h: Option<H>) -> bool {
    let kind = h.map(|h| h.kind);
    match PointClassType::from_code(cls as i32) {
        Some(PointClassType::Vertex) => kind == Some(K_VERTEX),
        Some(PointClassType::Edge) => kind == Some(K_EDGE) && edge_ok(two, h),
        Some(PointClassType::Face) => kind == Some(K_FACE),
        _ => true,
    }
}

fn class_ok(two: &Two, c: &ClassR) -> bool {
    c.curve.is_some() && c.pcs.iter().all(|&(cls, h)| pc_ok(two, cls, h)) && !unknown_end_on_face(c)
}

/// The shim's guard: an unknown start or end with a face or region mid. `MergeInterval`
/// gives the end the mid's face (it stays unknown, without object, when the point does
/// not project onto that face) or region (this runner names no region: a null object),
/// and the vertex merge then dereferences the null end (`SmCurveClass.cpp:17659`,
/// `:17672`).
fn unknown_end_on_face(c: &ClassR) -> bool {
    let code = |t: PointClassType| i64::from(t.code());
    let [(s, _), (m, _), (e, _)] = c.pcs;
    (m == code(PointClassType::Face) || m == code(PointClassType::Region))
        && (s == code(PointClassType::Unknown) || e == code(PointClassType::Unknown))
}

/// The full classification constructor with its single interval's classes set.
fn new_class(k: &Kernel, c: &ClassR) -> Result<CurveClassification<H>, SolidError> {
    let mut cc = CurveClassification::with_curve(
        k,
        &ClassificationInit {
            curve: c.curve,
            interval: Extent1d::from_min_max(c.t0, c.t1),
            uv_curve: None,
            src_zone_tol_3d: c.tol,
            opt_data_size: 0,
            is_curve_owned: false,
            is_curve_current: true,
            surface1: None,
            surface2: None,
            uv_curve2: None,
            ti: None,
        },
    )?;
    let ivl = cc.at_mut(0)?;
    let [(s, sh), (m, mh), (e, eh)] = c.pcs;
    let cls = |code: i64| PointClassType::from_code(code as i32).unwrap_or(PointClassType::Unknown);
    ivl.start.set_class_object(cls(s), sh);
    ivl.mid.set_class_object(cls(m), mh);
    ivl.end.set_class_object(cls(e), eh);
    Ok(cc)
}

fn put_class_out(
    o: &mut Vec<f64>,
    walk: &Walk,
    cc: &CurveClassification<H>,
) -> Result<(), SolidError> {
    o.push(cc.size() as f64);
    for i in 0..cc.size() {
        let ivl = cc.at(i)?;
        o.push(ivl.interval.min());
        o.push(ivl.interval.max());
        for pc in [&ivl.start, &ivl.mid, &ivl.end] {
            o.push(f64::from(pc.point_class().code()));
            walk.put(o, pc.object());
        }
    }
    Ok(())
}

/// The state after the merge (see the module docs), in the post-merge walk ([`Walk`]):
/// the records it reaches, in its order; mates sorted by their post-merge handles.
fn put_state(
    o: &mut Vec<f64>,
    two: &Two,
    walk: &Walk,
    ti: &TopologyIntersector<H>,
) -> Result<(), SolidError> {
    for b in 0..2i8 {
        let canon = usize::try_from(b).ok().and_then(|i| walk.0.get(i));
        let (Some(br), Some(Some(c))) = (two.brep(brep_h(b)), canon) else {
            o.extend([0.0, 0.0, 0.0]);
            continue;
        };
        o.push(c.vertices.items.len() as f64);
        o.push(c.edges.items.len() as f64);
        o.push(c.faces.items.len() as f64);
        for &v in &c.vertices.items {
            o.push(br.vertex(v)?.tolerance);
        }
        for &e in &c.edges.items {
            o.push(br.edge(e)?.tolerance);
        }
    }
    for kind in [
        ClassObjectKind::Vertex,
        ClassObjectKind::Edge,
        ClassObjectKind::Face,
    ] {
        let mut objs: Vec<_> = ti
            .mapped_brep_objects(kind)?
            .into_iter()
            .map(|h| (walk.triple(Some(h)), walk.triple(ti.other_mate(Some(h)))))
            .collect();
        objs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        o.push(objs.len() as f64);
        for (h, mate) in objs {
            o.extend(h);
            o.extend(mate);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_rust_merge(i: &Inputs, p: Prepared) -> Result<Vec<f64>, SolidError> {
    let Prepared {
        mut two,
        relates,
        merge,
    } = p;
    let mut k = Kernel::default();
    let mut ti = TopologyIntersector::new(
        Some(brep_h(0)),
        two.brep(brep_h(1)).map(|_| brep_h(1)),
        i.approx,
        i.ang,
    );
    ti.doing_fillets = i.doing_fillets;
    ti.ignore_tangency = i.ignore_tangency;
    let mut o = vec![0.0];

    // pre-existing relationships (the shim's guard: a null object past the conflict
    // checks is undefined upstream; the port reports InvalidInput there)
    for (a, b) in relates {
        status(ti.relate(&k, &mut two, a, b), &mut o)?;
    }

    match merge {
        MergeR::CurveOnSurfaces {
            s1,
            s2,
            tol,
            curve,
            edge1,
            edge2,
            uv1,
            uv2,
        } => {
            o.push(1.0);
            // the shim's guard, in upstream's order
            let no_face = s1.is_some_and(|s| face_of_surface(&two, s).is_none())
                || s2.is_some_and(|s| face_of_surface(&two, s).is_none());
            if !no_face {
                let refuse = match curve {
                    None => true,
                    Some(c) => {
                        let degenerate = ClassGeometry::curve(&two, c)
                            .map(|cv| cv.is_degenerate(&k, tol, None))
                            .transpose()?
                            .unwrap_or(false);
                        !degenerate
                            && ((if edge1.is_some() {
                                !edge_ok(&two, edge1)
                            } else {
                                s1.is_none()
                            }) || (if edge2.is_some() {
                                !edge_ok(&two, edge2)
                            } else {
                                s2.is_none()
                            }))
                    }
                };
                if refuse {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
            }
            let input = CurveOnSurfaces {
                surface: s1,
                other_surface: s2,
                curve_tolerance: tol,
                curve: curve.unwrap_or(hd(-1, 8, 0)),
                uv_curve1: uv1,
                uv_curve2: uv2,
                brep_edge: edge1,
                other_edge: edge2,
                ti: Some(TI_HANDLE),
                classifications: None,
            };
            let r = ti.merge_curve_on_surfaces(&mut k, &mut two, &input);
            if let Some(m) = status(r, &mut o)? {
                let walk = Walk::of(&two);
                o.push(flag(m.deleted_topology));
                put_list(&mut o, &walk, &m.deleted_edges.unwrap_or_default());
                put_list(&mut o, &walk, &m.deleted_vertices.unwrap_or_default());
            }
        }
        MergeR::CurveClasses {
            merge_singularities,
            c1,
            c2,
        } => {
            o.push(2.0);
            if !class_ok(&two, &c1) || !class_ok(&two, &c2) {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let mut cc1 = new_class(&k, &c1)?;
            let mut cc2 = new_class(&k, &c2)?;
            let r =
                ti.merge_curve_classes(&mut k, &mut two, &mut cc1, &mut cc2, merge_singularities);
            if let Some(m) = status(r, &mut o)? {
                let walk = Walk::of(&two);
                o.push(flag(m.topology_was_deleted));
                put_list(&mut o, &walk, &m.deleted_edges.unwrap_or_default());
                put_list(&mut o, &walk, &m.deleted_vertices.unwrap_or_default());
                put_class_out(&mut o, &walk, &cc1)?;
                put_class_out(&mut o, &walk, &cc2)?;
            }
        }
        MergeR::CoincidentSurfaces { s1, s2 } => {
            o.push(3.0);
            let (Some(s1), Some(s2)) = (s1, s2) else {
                o.push(NULL_DEREF);
                return Ok(o);
            };
            if face_of_surface(&two, s1).is_none() || face_of_surface(&two, s2).is_none() {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = ti.merge_coincident_surfaces(&mut k, &mut two, s1, s2, Some(TI_HANDLE));
            if let Some(m) = status(r, &mut o)? {
                let walk = Walk::of(&two);
                put_list(&mut o, &walk, &m.faces);
                put_list(&mut o, &walk, &m.other_faces);
            }
        }
    }
    put_state(&mut o, &two, &Walk::of(&two), &ti)?;
    Ok(o)
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let p = match prepare(i, true) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match run_rust_merge(i, p) {
        Ok(o) => Ok(Output::new().values("state", o)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => Err(harness(format!("rust: {e}"))),
        },
    }
}

// ---- the part-3 walks of the adapter ---------------------------------------------------------

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "difftest::boolean_topology_intersector_p3",
        reason,
    }
}

impl IntersectorBrepP3<H> for Two {
    fn surface_face(&self, surface: H) -> solidcore::Result<Option<H>> {
        Ok(face_of_surface(self, surface))
    }

    fn face_surface_trace_cache(
        &mut self,
        k: &Kernel,
        surface: H,
    ) -> solidcore::Result<Option<solidcore::surface::surface_cache::SurfaceCache<H>>> {
        if surface.kind != K_FACE_SURFACE {
            return Ok(None);
        }
        let side = surface.b;
        let mut lease = solidcore::query::face::p2::face_surface_cache(
            k,
            self.brep_m(surface)?,
            FaceId::from_index(surface.i),
        )?;
        if let Ok(Some(owner)) = lease.cache.base().surface().face() {
            let tol = self
                .topology_tolerance(hd(side, K_FACE, surface.i))
                .unwrap_or(0.0);
            lease
                .cache
                .bind_face(owner, EntityRef::Face(FaceId::from_index(surface.i)), tol)?;
        }
        // the cache's objects in this adapter's handles (the B-rep's topology)
        let mapped = lease.cache.base().map_handles(|e| match e {
            EntityRef::Vertexuse(vu) => Ok(hd(side, K_VERTEXUSE, vu.index())),
            e => Two::entity_h(side, e).ok_or_else(|| {
                SolidError::invalid_input(
                    "boolean.topology_intersector_p3 runner",
                    "cache object without a handle",
                )
            }),
        });
        lease.release();
        mapped.map(Some)
    }

    fn edge_is_manifold(&self, edge: H) -> solidcore::Result<bool> {
        ed::is_manifold(self.brep_r(edge)?, eid(edge))
    }

    fn loopuse_is_vertex_loopuse(&self, loopuse: H) -> solidcore::Result<bool> {
        lu::is_vertex_loopuse(self.brep_r(loopuse)?, LoopuseId::from_index(loopuse.i))
    }

    fn loopuse_edgeuses(&self, loopuse: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(loopuse)?;
        Ok(
            lu::edgeuses(b, LoopuseId::from_index(loopuse.i), None, None)?
                .into_iter()
                .map(|eu| hd(loopuse.b, K_EDGEUSE, eu.index()))
                .collect(),
        )
    }

    fn edgeuse_edge(&self, edgeuse: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(edgeuse)?;
        Ok(b.edgeuse(EdgeuseId::from_index(edgeuse.i))?
            .edge
            .map(|e| hd(edgeuse.b, K_EDGE, e.index())))
    }

    fn edgeuse_face(&self, edgeuse: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(edgeuse)?;
        Ok(eu1::face(b, EdgeuseId::from_index(edgeuse.i))?
            .map(|f| hd(edgeuse.b, K_FACE, f.index())))
    }

    fn faceuse_face(&self, faceuse: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(faceuse)?;
        Ok(b.faceuse(FaceuseId::from_index(faceuse.i))?
            .face
            .map(|f| hd(faceuse.b, K_FACE, f.index())))
    }

    fn edge_find_radial_sector(
        &mut self,
        k: &Kernel,
        edge: H,
        classify_edgeuse: H,
        required_orientation: OrientType,
    ) -> solidcore::Result<(Option<H>, Option<H>)> {
        if classify_edgeuse.kind != K_EDGEUSE {
            return Err(invalid("not an edge use"));
        }
        let eu = EdgeuseId::from_index(classify_edgeuse.i);
        let s = if edge.b == classify_edgeuse.b {
            let b = self.brep_m(edge)?;
            ed::find_radial_sector_between(
                k,
                b,
                eid(edge),
                None,
                eu,
                None,
                required_orientation,
                None,
            )?
        } else {
            // the edge and the classified edge use in the two different B-reps
            let [a, o] = &mut self.breps;
            let (b, other) = if edge.b == 0 { (a, o) } else { (o, a) };
            let b = b
                .as_mut()
                .ok_or_else(|| invalid("handle names an absent B-rep"))?;
            let other = other
                .as_mut()
                .ok_or_else(|| invalid("handle names an absent B-rep"))?;
            ed::find_radial_sector_between(
                k,
                b,
                eid(edge),
                Some(other),
                eu,
                None,
                required_orientation,
                None,
            )?
        };
        Ok((
            s.radial_edgeuse.map(|eu| hd(edge.b, K_EDGEUSE, eu.index())),
            s.coincident_faceuse
                .map(|fu| hd(edge.b, K_FACEUSE, fu.index())),
        ))
    }

    fn face_upward_loopuses(&self, face: H) -> solidcore::Result<Vec<H>> {
        if face.kind != K_FACE {
            return Err(invalid("not a face"));
        }
        Ok(
            fp4::upward_loopuses(self.brep_r(face)?, fid(face), false, None)?
                .into_iter()
                .map(|lu| hd(face.b, K_LOOPUSE, lu.index()))
                .collect(),
        )
    }

    fn face_bounding_box(&self, k: &Kernel, face: H) -> solidcore::Result<Extent3d> {
        if face.kind != K_FACE {
            return Err(invalid("not a face"));
        }
        qf1::calculate_bounding_box(k, self.brep_r(face)?, fid(face), false)
    }
}

// ---- generation -----------------------------------------------------------------------------

const FILES: [&str; 8] = [
    "plane_square.smb",
    "plane_circ.smb",
    "disk.smb",
    "cone.smb",
    "box_with_large_fillet.smb",
    "surf_extrude.smb",
    "sphere.smb",
    "torus.smb",
];

fn file(name: &str) -> BrepIn {
    BrepIn::File {
        file: name.to_owned(),
        rebuild: false,
    }
}

fn sel(rng: &mut Rng) -> i64 {
    i64::from(rng.below(64) as u32)
}

fn h(b: i64, kind: u8, s: i64) -> Hin {
    [b, i64::from(kind), s]
}

const NULL_H: Hin = [-1, K_NULL, -1];

/// [`NULL_H`] (the serde default of the optional UV curves).
const fn null_h() -> Hin {
    NULL_H
}

fn tol(rng: &mut Rng) -> f64 {
    [1e-7, 1e-6, 1e-5, 1e-4, 1e-3]
        .get(rng.below(5) as usize)
        .copied()
        .unwrap_or(1e-6)
}

/// A classification of edge `e`'s curve over its whole interval (or a sub-range), its
/// ends at vertex selectors and its middle on the edge (or other classes).
fn gen_class(rng: &mut Rng, b: i64, e: i64) -> ClassIn {
    let (f0, f1) = if rng.chance(0.8) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(0.0, 0.5);
        (a, rng.uniform(a + 0.1, 1.0))
    };
    let vertex_or = |rng: &mut Rng| {
        if rng.chance(0.85) {
            PcIn {
                cls: i64::from(PointClassType::Vertex.code()),
                h: h(b, K_VERTEX, sel(rng)),
            }
        } else {
            PcIn {
                cls: i64::from(PointClassType::Edge.code()),
                h: h(b, K_EDGE, e),
            }
        }
    };
    let start = vertex_or(rng);
    let mid = PcIn {
        cls: i64::from(PointClassType::Edge.code()),
        h: h(b, K_EDGE, e),
    };
    let end = vertex_or(rng);
    ClassIn {
        curve: h(b, K_EDGE_CURVE, e),
        f0,
        f1,
        tol: tol(rng),
        start,
        mid,
        end,
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let a_name = FILES
        .get(rng.below(FILES.len() as u64) as usize)
        .copied()
        .unwrap_or("disk.smb");
    let brep_a = file(a_name);
    // mostly the same file twice: every edge has a coincident partner
    let brep_b = if rng.chance(0.8) {
        file(a_name)
    } else {
        file(
            FILES
                .get(rng.below(FILES.len() as u64) as usize)
                .copied()
                .unwrap_or("disk.smb"),
        )
    };
    let e = sel(rng);
    let e2 = if rng.chance(0.75) { e } else { sel(rng) };
    let mut relates = Vec::new();
    for _ in 0..rng.below(3) {
        let pair = if rng.chance(0.5) {
            let v = sel(rng);
            [
                h(0, K_VERTEX, v),
                h(1, K_VERTEX, if rng.chance(0.7) { v } else { sel(rng) }),
            ]
        } else {
            let x = sel(rng);
            [
                h(0, K_EDGE, x),
                h(1, K_EDGE, if rng.chance(0.7) { x } else { sel(rng) }),
            ]
        };
        relates.push(pair);
    }
    let approx = tol(rng);
    let merge = match rng.below(10) {
        0..=5 => {
            let with_surfaces = rng.chance(0.2);
            let f = sel(rng);
            MergeIn::CurveOnSurfaces {
                s1: if with_surfaces {
                    h(0, K_FACE_SURFACE, f)
                } else {
                    NULL_H
                },
                s2: if with_surfaces {
                    h(1, K_FACE_SURFACE, f)
                } else {
                    NULL_H
                },
                tol: tol(rng),
                curve: h(0, K_EDGE_CURVE, e),
                edge1: h(0, K_EDGE, e),
                edge2: h(1, K_EDGE, e2),
                uv1: NULL_H,
                uv2: NULL_H,
            }
        }
        6..=8 => MergeIn::CurveClasses {
            merge_singularities: rng.chance(0.3),
            c1: gen_class(rng, 0, e),
            c2: gen_class(rng, 1, e2),
        },
        _ => {
            let f = sel(rng);
            MergeIn::CoincidentSurfaces {
                s1: h(0, K_FACE_SURFACE, f),
                s2: h(
                    1,
                    K_FACE_SURFACE,
                    if rng.chance(0.7) { f } else { sel(rng) },
                ),
            }
        }
    };
    Inputs {
        brep_a,
        brep_b: Some(brep_b),
        approx,
        ang: 0.01,
        doing_fillets: false,
        ignore_tangency: rng.chance(0.2),
        relates,
        merge,
    }
}

/// Generated cases at least.
const MIN_GENERATED: usize = 256;

/// Cases built in code: coincident edges of one file twice (with and without a
/// conflicting earlier relation), the wire network, a null curve (refused), a
/// classification pair at tiny tolerance and two coincident planar faces.
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut add = |name: &str, i: Inputs| {
        if let Ok(v) = serde_json::to_value(i) {
            out.push((name.to_owned(), v));
        }
    };
    let same = |f: &str| (file(f), Some(file(f)));
    let on_edges = |e1: i64, e2: i64, tol: f64| MergeIn::CurveOnSurfaces {
        s1: NULL_H,
        s2: NULL_H,
        tol,
        curve: h(0, K_EDGE_CURVE, e1),
        edge1: h(0, K_EDGE, e1),
        edge2: h(1, K_EDGE, e2),
        uv1: NULL_H,
        uv2: NULL_H,
    };
    let base = |(brep_a, brep_b): (BrepIn, Option<BrepIn>), merge: MergeIn| Inputs {
        brep_a,
        brep_b,
        approx: 1e-6,
        ang: 0.01,
        doing_fillets: false,
        ignore_tangency: false,
        relates: Vec::new(),
        merge,
    };
    for e in 0..4 {
        add(
            &format!("square_coincident_edge_{e}"),
            base(same("plane_square.smb"), on_edges(e, e, 1e-6)),
        );
    }
    add(
        "square_mismatched_edges",
        base(same("plane_square.smb"), on_edges(0, 2, 1e-6)),
    );
    let mut conflict = base(same("plane_square.smb"), on_edges(1, 1, 1e-6));
    conflict.relates = vec![[h(0, K_VERTEX, 1), h(1, K_VERTEX, 2)]];
    add("square_conflicting_vertex_relation", conflict);
    let mut edge_conflict = base(same("plane_square.smb"), on_edges(1, 1, 1e-6));
    edge_conflict.relates = vec![[h(0, K_EDGE, 1), h(1, K_EDGE, 3)]];
    add("square_conflicting_edge_relation", edge_conflict);
    add(
        "network_coincident_edge",
        base(
            (BrepIn::Network, Some(BrepIn::Network)),
            on_edges(2, 2, 1e-6),
        ),
    );
    add(
        "disk_null_curve_refused",
        base(
            same("disk.smb"),
            MergeIn::CurveOnSurfaces {
                s1: NULL_H,
                s2: NULL_H,
                tol: 1e-6,
                curve: NULL_H,
                edge1: h(0, K_EDGE, 0),
                edge2: h(1, K_EDGE, 0),
                uv1: NULL_H,
                uv2: NULL_H,
            },
        ),
    );
    // with the UV trim curves of the curve's own edge use on each face (pUVCurve1 /
    // pUVCurve2), both surfaces given and no edges: the CurveOnClassify path
    let with_uv = |e: i64, (f1, eu1): (i64, i64), (f2, eu2): (i64, i64)| MergeIn::CurveOnSurfaces {
        s1: h(0, K_FACE_SURFACE, f1),
        s2: h(1, K_FACE_SURFACE, f2),
        tol: 1e-6,
        curve: h(0, K_EDGE_CURVE, e),
        edge1: NULL_H,
        edge2: NULL_H,
        uv1: h(0, K_TRIM_CURVE, eu1),
        uv2: h(1, K_TRIM_CURVE, eu2),
    };
    add(
        "square_uv_curves",
        base(same("plane_square.smb"), with_uv(0, (0, 0), (0, 0))),
    );
    add(
        "disk_uv_curves",
        base(same("disk.smb"), with_uv(0, (0, 0), (0, 0))),
    );
    // edge 14 bounds face 0 (edge use 0) and face 5 (edge use 22)
    add(
        "fillet_box_uv_curves_two_faces",
        base(
            same("box_with_large_fillet.smb"),
            with_uv(14, (0, 0), (5, 22)),
        ),
    );
    let pc = |cls: PointClassType, b: i64, kind: u8, s: i64| PcIn {
        cls: i64::from(cls.code()),
        h: h(b, kind, s),
    };
    let class = |b: i64, e: i64, tol: f64| ClassIn {
        curve: h(b, K_EDGE_CURVE, e),
        f0: 0.0,
        f1: 1.0,
        tol,
        start: pc(PointClassType::Vertex, b, K_VERTEX, e),
        mid: pc(PointClassType::Edge, b, K_EDGE, e),
        end: pc(PointClassType::Vertex, b, K_VERTEX, e + 1),
    };
    add(
        "square_classes_tiny_tolerance",
        base(
            same("plane_square.smb"),
            MergeIn::CurveClasses {
                merge_singularities: true,
                c1: class(0, 1, 1e-12),
                c2: class(1, 1, 1e-12),
            },
        ),
    );
    // the review's crash: unknown ends of a face-mid interval that does not project onto
    // the face (refused on both sides)
    let face_mid = |b: i64| ClassIn {
        curve: h(b, K_EDGE_CURVE, 0),
        f0: 0.25,
        f1: 0.75,
        tol: 1e-6,
        start: PcIn {
            cls: i64::from(PointClassType::Unknown.code()),
            h: NULL_H,
        },
        mid: pc(PointClassType::Face, b, K_FACE, 0),
        end: PcIn {
            cls: i64::from(PointClassType::Unknown.code()),
            h: NULL_H,
        },
    };
    add(
        "fillet_box_unknown_ends_face_mid_refused",
        base(
            same("box_with_large_fillet.smb"),
            MergeIn::CurveClasses {
                merge_singularities: false,
                c1: face_mid(0),
                c2: face_mid(1),
            },
        ),
    );
    add(
        "square_coincident_surfaces",
        base(
            same("plane_square.smb"),
            MergeIn::CoincidentSurfaces {
                s1: h(0, K_FACE_SURFACE, 0),
                s2: h(1, K_FACE_SURFACE, 0),
            },
        ),
    );
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.topology_intersector_p3"
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
        let i: Inputs = case.parse()?;
        run_cpp(&i)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        run_rust(&i)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }
}

/// `SmFace::Point3DClassify` on the runner's B-reps
/// ([`solidcore::boolean::topology_intersector::p1::IntersectorBrep::face_point_3d_classify`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn face_point_3d_classify(
        this: &mut Two,
        k: &Kernel,
        face: H,
        point: Point3,
        src_zone_tol_3d: f64,
        do_boundary_intersections: bool,
        pc: &mut PointClassification<H>,
        opt_guess_uv: Option<Point2>,
    ) -> solidcore::Result<()> {
        if face.kind != K_FACE {
            return Err(invalid("not a face"));
        }
        // the caller's classification in the face's B-rep handles (its other members are
        // kept unless written, as upstream's out-param)
        let mut pe = pc.map_objects(&mut |h: H| {
            if h.b != face.b {
                return Err(invalid("classification object of the other B-rep"));
            }
            this.topology_entity(h)
                .map(|(_, e)| e)
                .ok_or_else(|| invalid("classification object without a B-rep entity"))
        })?;
        let b = this.brep_m(face)?;
        let r = qf1::point_3d_classify(
            k,
            b,
            fid(face),
            point,
            src_zone_tol_3d,
            do_boundary_intersections,
            &mut pe,
            qf1::Point3dClassifyOptions {
                guess_uv: opt_guess_uv,
                ..Default::default()
            },
        );
        // copied back whatever the status
        *pc = pe.map_objects(&mut |e: EntityRef| {
            Two::entity_h(face.b, e).ok_or_else(|| invalid("entity without a handle"))
        })?;
        r
    }
