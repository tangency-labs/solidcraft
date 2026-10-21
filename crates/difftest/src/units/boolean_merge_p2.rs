//! Runner for unit `boolean.merge_p2`: the `SmMerge` B-rep constructor, `MergeEdge`,
//! `MakeFaceuse`, `MergeAddedFace` and `MergeAddedTopology` on identical raw input
//! topology. C++: `solidcore_ffi::units::boolean_merge_p2`. Rust:
//! `solidcore::boolean::merge::p2`.
//!
//! Operands are built by the shared real-B-rep fixture builder of `boolean.merge_p5`
//! and imported into Rust as raw arena records before either side runs; the fixture's
//! mates are established on both sides with `Relate` (edges, vertices, faces). Outputs
//! are the request's results followed by snapshots of the operands, as `merge_p5`'s.
//! `MergeAddedTopology` runs on the Rust side through the intersector runner's
//! two-B-rep adapter (`Two`), which implements [`MergeBrep`] here.
//!
//! Tolerances may be the strings `"NaN"`, `"inf"`, `"-inf"` (JSON has no non-finite
//! numbers). Failures are `status_<SmStatus>`-style integers in the first output slot
//! on both sides.

use super::boolean_merge_p5::{self as p5, Spec};
use super::boolean_topology_intersector_p1::{brep_h, hd, Two, H, K_EDGE, K_FACE, K_VERTEX};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::boolean::merge::p1::{MergeOptions, MergeState, VertexCopy};
use solidcore::boolean::merge::p2::{
    make_faceuse, merge_edge, MergeAddedOptions, MergeBrep, SplitEdges, StitchSettings,
    StitchedEdges, StitchedVertices, TopologyCopy,
};
use solidcore::boolean::topology_intersector::p1::{Relations, TopologyIntersector};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::MarkLock;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::heal::stitch::p1::{DefaultStitchCallback, Stitch};
use solidcore::kernel::Kernel;
use solidcore::topology;
use solidcore::topology::topology::Topology;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    edge, edgeuse, face, vertex, ArenaId, Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, RegionId,
    VertexId,
};
use solidcore::SolidError;
use solidcore_ffi::units::boolean_merge_p2::{self as ffi, Mates, Request};
use std::collections::BTreeMap;

/// Differential runner for `boolean.merge_p2`.
pub struct Runner;

/// Handle kinds of the Rust-only face use, shell and region handles of [`MergeBrep`].
const K_FACEUSE: u8 = 14;
const K_SHELL: u8 = 15;
const K_REGION: u8 = 16;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MatesIn {
    /// Other edge -> primary edge (`-1`: none); default: by index.
    #[serde(default)]
    edges: Option<Vec<i64>>,
    /// Other vertex -> primary vertex (`-1`: none); default: by index.
    #[serde(default)]
    vertices: Option<Vec<i64>>,
    /// Other face -> primary face (`-1`: none); default: none.
    #[serde(default)]
    faces: Option<Vec<i64>>,
}

fn none() -> i64 {
    -1
}

fn face_flags() -> [bool; 5] {
    [false, true, true, true, true]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    Construct,
    MergeEdge {
        edgeuse: i64,
        #[serde(default = "none")]
        region: i64,
    },
    MakeFaceuse {
        faceuse: i64,
        #[serde(default = "none")]
        region: i64,
        #[serde(default)]
        new_faces: bool,
    },
    MergeAddedTopology {
        #[serde(default)]
        faces: Option<Vec<i64>>,
        #[serde(default)]
        edges: Option<Vec<i64>>,
        #[serde(default)]
        vertices: Option<Vec<i64>>,
        /// The definition's order: manifold solid, fast edge compare, region
        /// nesting, ignore problems, `bDoGluing`.
        #[serde(default = "face_flags")]
        flags: [bool; 5],
    },
    MergeAddedFace {
        face: i64,
    },
}

fn default_tol() -> Value {
    json!(1.0e-3)
}

fn zero() -> Value {
    json!(0.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    primary: Spec,
    #[serde(default)]
    other: Option<Spec>,
    #[serde(default = "default_tol")]
    tol: Value,
    #[serde(default = "zero")]
    ang: Value,
    /// `SmMergeOptions` bits 1, 2, 4, 8, 16 in argument order; absent: null.
    #[serde(default)]
    options: Option<u8>,
    #[serde(default)]
    mates: MatesIn,
    #[serde(flatten)]
    op: Op,
}

fn err(e: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(e.to_string())
}

/// A JSON number or `"NaN"` / `"inf"` / `"-inf"`.
fn float(v: &Value) -> Result<f64, HarnessError> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| err(&format!("bad number {n}"))),
        Value::String(s) if s == "NaN" => Ok(f64::NAN),
        Value::String(s) if s == "inf" => Ok(f64::INFINITY),
        Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
        other => Err(err(&format!("bad float {other}"))),
    }
}

fn options_of(mask: u8) -> MergeOptions {
    MergeOptions::new(
        mask & 1 != 0,
        mask & 2 != 0,
        mask & 4 != 0,
        mask & 8 != 0,
        mask & 16 != 0,
    )
}

/// The mates of a request: the given lists, else by index (edges, vertices) and none
/// (faces).
fn mates_of(m: &MatesIn, a: [usize; 10], b: [usize; 10]) -> [Vec<i64>; 3] {
    let by_index = |pa: usize, pb: usize| -> Vec<i64> {
        (0..pb)
            .map(|j| if j < pa { j as i64 } else { -1 })
            .collect()
    };
    [
        m.edges.clone().unwrap_or_else(|| by_index(a[7], b[7])),
        m.vertices.clone().unwrap_or_else(|| by_index(a[9], b[9])),
        m.faces.clone().unwrap_or_else(|| vec![-1; b[3]]),
    ]
}

/// Everything both engines need: construction programs, counts and the mates.
struct Prepared {
    a: (String, Vec<f64>),
    b: Option<(String, Vec<f64>)>,
    mates: [Vec<i64>; 3],
    tol: f64,
    ang: f64,
}

fn prepare(i: &Inputs) -> Result<Prepared, HarnessError> {
    let a = p5::program(&i.primary)?;
    let b = i.other.as_ref().map(p5::program).transpose()?;
    let ac = ffi::counts(p5::operand(&a)).map_err(|e| err(&e))?;
    let bc = match &b {
        Some(b) => ffi::counts(p5::operand(b)).map_err(|e| err(&e))?,
        None => [0; 10],
    };
    let mates = mates_of(&i.mates, ac, bc);
    Ok(Prepared {
        a,
        b,
        mates,
        tol: float(&i.tol)?,
        ang: float(&i.ang)?,
    })
}

fn request<'a>(op: &'a Op, mates: &'a [Vec<i64>; 3]) -> Request<'a> {
    let m = Mates {
        edges: &mates[0],
        vertices: &mates[1],
        faces: &mates[2],
    };
    match op {
        Op::Construct => Request::Construct,
        Op::MergeEdge { edgeuse, region } => Request::MergeEdge {
            mates: m,
            edgeuse: *edgeuse,
            region: *region,
        },
        Op::MakeFaceuse {
            faceuse,
            region,
            new_faces,
        } => Request::MakeFaceuse {
            mates: m,
            faceuse: *faceuse,
            region: *region,
            want_new_faces: *new_faces,
        },
        Op::MergeAddedTopology {
            faces,
            edges,
            vertices,
            flags,
        } => Request::MergeAddedTopology {
            faces: faces.as_deref(),
            edges: edges.as_deref(),
            vertices: vertices.as_deref(),
            flags: *flags,
        },
        Op::MergeAddedFace { face } => Request::MergeAddedFace { face: *face },
    }
}

fn output(raw: &[f64]) -> Result<Output, HarnessError> {
    let n = *raw.first().ok_or_else(|| err("empty result"))? as usize;
    let end = n.checked_add(1).ok_or_else(|| err("output overflow"))?;
    let ints: Vec<i64> = raw
        .get(1..end)
        .ok_or_else(|| err("truncated integers"))?
        .iter()
        .map(|&x| x as i64)
        .collect();
    let values = raw
        .get(end..)
        .ok_or_else(|| err("truncated values"))?
        .to_vec();
    Ok(Output::new()
        .ints("topology", ints)
        .values("geometry", values))
}

/// The upstream status of a Rust error (`SM_ERR_INVALID_INPUT` for
/// [`SolidError::InvalidInput`]); `-1` for kinds without one.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::Upstream { code, .. } => i64::from(*code),
        SolidError::InvalidInput { .. } => 1007,
        _ => -1,
    }
}

fn id<I: ArenaId>(value: Option<I>) -> i64 {
    value.map_or(-1, |i| i64::from(i.index()))
}

fn opt_id<I: ArenaId>(i: i64) -> Option<I> {
    u32::try_from(i).ok().map(I::from_index)
}

/// The snapshots of the operands with a fresh, unused mark (as the shim's lock).
fn snapshots(
    breps: &[&Brep],
    ids: &mut Vec<i64>,
    values: &mut Vec<f64>,
) -> Result<(), HarnessError> {
    let mut k = Kernel::default();
    let lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, 1).map_err(|e| err(&e))?;
    let mt = lock.mark_type();
    let marks = Marks::default();
    for b in breps {
        p5::snapshot(&k, b, &marks, mt, ids, values)?;
    }
    Ok(())
}

/// The fixture's `Relate` calls, in the shim's order (edges, vertices, faces).
fn relate(a: &mut Brep, b: &mut Brep, mates: &[Vec<i64>; 3]) -> Result<Relations, HarnessError> {
    let mut r = Relations::default();
    for (j, &m) in mates[0].iter().enumerate() {
        if let Some(e) = opt_id::<EdgeId>(m) {
            r.relate_edges(a, e, b, EdgeId::from_index(j as u32))
                .map_err(|e| err(&e))?;
        }
    }
    for (j, &m) in mates[1].iter().enumerate() {
        if let Some(v) = opt_id::<VertexId>(m) {
            r.relate_vertices(a, v, b, VertexId::from_index(j as u32))
                .map_err(|e| err(&e))?;
        }
    }
    for (j, &m) in mates[2].iter().enumerate() {
        if let Some(f) = opt_id::<FaceId>(m) {
            r.relate_faces(a, f, b, FaceId::from_index(j as u32))
                .map_err(|e| err(&e))?;
        }
    }
    Ok(r)
}

fn key(base: u64, type_code: i64, i: u32) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(base + u64::from(i)),
        type_code,
    }
}

impl MergeBrep<H> for Two {
    fn edge_vertices(&self, e: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(e)?;
        Ok(edge::vertices(b, EdgeId::from_index(e.i), None)?
            .into_iter()
            .flatten()
            .map(|v| hd(e.b, K_VERTEX, v.index()))
            .collect())
    }
    fn face_faceuses(&self, f: H) -> solidcore::Result<(Option<H>, Option<H>)> {
        let b = self.brep_r(f)?;
        let (a, c) = face::p2::faceuses(b, FaceId::from_index(f.i))?;
        let h = |fu: Option<FaceuseId>| fu.map(|fu| hd(f.b, K_FACEUSE, fu.index()));
        Ok((h(a), h(c)))
    }
    fn test_closure_split_region(
        &mut self,
        k: &mut Kernel,
        brep: H,
        test_closure: bool,
        faceuse: H,
    ) -> solidcore::Result<(Option<H>, Option<H>)> {
        // The SmTopology marks live in a record per call (upstream's object marks);
        // the region split takes and releases its own mark locks. The fixtures carry no
        // attribute records: its notifications run on an empty store over the regions,
        // as topology.brep_p10's runner.
        let b = self.brep_m(brep)?;
        let mut state = topology::brep::p2::RegionNotifyState::new(b);
        let (shell, region) = topology::brep::p6::test_closure_split_region(
            k,
            b,
            &mut Marks::default(),
            &mut state.env(),
            test_closure,
            FaceuseId::from_index(faceuse.i),
        )?;
        Ok((
            shell.map(|s| hd(brep.b, K_SHELL, s.index())),
            region.map(|r| hd(brep.b, K_REGION, r.index())),
        ))
    }
    fn notify_brep(
        &mut self,
        k: &mut Kernel,
        brep: H,
        operation: NotifyOperation,
    ) -> solidcore::Result<()> {
        let _ = k;
        // PRE_EDIT and DESTRUCTION delete the B-rep's cache; it is rebuilt on demand.
        if matches!(
            operation,
            NotifyOperation::PreEdit | NotifyOperation::Destruction
        ) {
            self.delete_cache(brep)?;
        }
        // The fixtures carry no attribute records: an empty store over the regions, as
        // heal.stitch_p1's PRE_EDIT.
        let b = self.brep_r(brep)?;
        let mut state = topology::brep::p2::RegionNotifyState::new(b);
        let this = topology::brep::p2::NotifyArg::Object(topology::brep::p2::brep_ref(b));
        topology::brep::p2::notify(
            b,
            &mut state.env(),
            None,
            operation,
            [Some(this), None, None],
        )
    }
    fn is_manifold_solid(&self, brep: H) -> solidcore::Result<bool> {
        topology::brep::p2::is_manifold_solid(self.brep_r(brep)?)
    }
    fn faceuse_shell(&self, fu: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(fu)?;
        Ok(edgeuse::p1::faceuse_shell(b, FaceuseId::from_index(fu.i))?
            .map(|s| hd(fu.b, K_SHELL, s.index())))
    }
    fn face_bounding_box(&self, k: &Kernel, face: H) -> solidcore::Result<Extent3d> {
        // bTight = FALSE, the header default
        solidcore::query::face::p1::calculate_bounding_box(
            k,
            self.brep_r(face)?,
            FaceId::from_index(face.i),
            false,
        )
    }
    fn stitch_vertex_pairs(
        &mut self,
        k: &mut Kernel,
        brep: H,
        settings: &StitchSettings,
        list1: &mut [Option<H>],
        list2: &mut [Option<H>],
    ) -> solidcore::Result<StitchedVertices<H>> {
        let mut l1 = ids_of::<VertexId>(list1);
        let mut l2 = ids_of::<VertexId>(list2);
        let mut callback = DefaultStitchCallback::new();
        let mut stitch = stitch_of(&mut callback, settings);
        let r = stitch.stitch_vertex_pairs(k, self.brep_m(brep)?, &mut l1, &mut l2);
        // Glued entries become null on both sides, also when the stitch fails.
        null_where(list1, &l1);
        null_where(list2, &l2);
        let r = r?;
        Ok(StitchedVertices {
            processed: handles(brep, K_VERTEX, &r.processed),
            deleted: handles(brep, K_VERTEX, &r.deleted),
            max_vertex_gap: r.max_vertex_gap,
            min_unstitched_vert_gap: r.min_unstitched_vert_gap,
        })
    }
    fn split_edges_with_vertices(
        &mut self,
        k: &mut Kernel,
        brep: H,
        settings: &StitchSettings,
        verts: &mut [Option<H>],
        edges: &mut Vec<Option<H>>,
    ) -> solidcore::Result<SplitEdges<H>> {
        let mut v = ids_of::<VertexId>(verts);
        let mut e = ids_of::<EdgeId>(edges);
        let mut callback = DefaultStitchCallback::new();
        let mut stitch = stitch_of(&mut callback, settings);
        let r = stitch.split_edges_with_vertices(k, self.brep_m(brep)?, &mut v, &mut e);
        null_where(verts, &v);
        // The routine nulls processed edges and appends the new ones.
        *edges = e
            .iter()
            .map(|e| e.map(|e| hd(brep.b, K_EDGE, e.index())))
            .collect();
        let r = r?;
        Ok(SplitEdges {
            processed_verts: handles(brep, K_VERTEX, &r.processed),
            split_edges: handles(brep, K_EDGE, &r.split),
            new_edges: handles(brep, K_EDGE, &r.new_edges),
            max_vertex_gap: r.max_vertex_gap,
            min_unstitched_vert_gap: r.min_unstitched_vert_gap,
        })
    }
    fn stitch_edges_of_vertices(
        &mut self,
        k: &mut Kernel,
        brep: H,
        settings: &StitchSettings,
        verts: &[Option<H>],
    ) -> solidcore::Result<StitchedEdges<H>> {
        let v = ids_of::<VertexId>(verts);
        let mut callback = DefaultStitchCallback::new();
        let mut stitch = stitch_of(&mut callback, settings);
        let r = stitch.stitch_edges_of_vertices(k, self.brep_m(brep)?, &v)?;
        Ok(StitchedEdges {
            glued: handles(brep, K_EDGE, &r.glued),
            deleted: handles(brep, K_EDGE, &r.deleted),
            max_edge_gap: r.max_edge_gap,
            min_unstitched_edge_gap: r.min_unstitched_edge_gap,
        })
    }
    fn find_and_set_infinite_region(&mut self, _k: &mut Kernel, brep: H) -> solidcore::Result<()> {
        // pOptShellToTest = NULL, SM_VS_INFINITEREGION_SWAP (2), no bounding box
        topology::brep::p5::find_and_set_infinite_region(self.brep_m(brep)?, None, 2, None)
    }
}

/// The `SmStitch` of `MergeAddedTopology`: `SmStitch(sCB, tol)` with the members it sets.
fn stitch_of<'c>(callback: &'c mut DefaultStitchCallback, s: &StitchSettings) -> Stitch<'c> {
    let mut stitch = Stitch::new(
        callback,
        s.stitch_tol_3d,
        s.do_region_nesting,
        s.validate_result,
    );
    stitch.making_manifold_solid = s.making_manifold_solid;
    stitch.ignore_problems = s.ignore_problems;
    stitch.fast_edge_compare = s.fast_edge_compare;
    stitch.squeeze_small_edges = s.squeeze_small_edges;
    stitch.split_edges_with_vertices = s.split_edges_with_vertices;
    stitch.remove_laminar_slivers = s.remove_laminar_slivers;
    stitch
}

/// Arena ids of a handle list (null entries stay null).
fn ids_of<I: ArenaId>(list: &[Option<H>]) -> Vec<Option<I>> {
    list.iter().map(|h| h.map(|h| I::from_index(h.i))).collect()
}

/// Null the handles whose ids the routine nulled.
fn null_where<I>(list: &mut [Option<H>], ids: &[Option<I>]) {
    for (h, id) in list.iter_mut().zip(ids) {
        if id.is_none() {
            *h = None;
        }
    }
}

fn handles<I: ArenaId>(brep: H, kind: u8, ids: &[I]) -> Vec<H> {
    ids.iter().map(|i| hd(brep.b, kind, i.index())).collect()
}

fn import_operand(p: &(String, Vec<f64>)) -> Result<Brep, HarnessError> {
    p5::import(
        &solidcore_ffi::units::boolean_merge_p5::export(p5::operand(p)).map_err(|e| err(&e))?,
    )
}

fn run_rust(i: &Inputs, p: &Prepared) -> Result<Output, HarnessError> {
    let mut a = import_operand(&p.a)?;
    let mut b = p.b.as_ref().map(import_operand).transpose()?;
    let options = i.options.map(options_of);
    let k = Kernel::default();
    let mut ids = Vec::new();
    let mut values = Vec::new();
    match &i.op {
        Op::Construct => {
            let m = match MergeState::<TopologyIntersector<H>>::with_breps(
                &k,
                (&a, brep_h(0)),
                b.as_ref().map(|b| (b, brep_h(1))),
                p.tol,
                p.ang,
                options.as_ref(),
            ) {
                Ok(m) => m,
                Err(e) => return Output::unported(&e).ok_or_else(|| err(&e)),
            };
            ids.push(0);
            values.extend([
                m.intersector.this_approx_tol_3d(),
                m.intersector.this_ang_tol_rad(),
            ]);
            let o = m.options;
            values.extend(
                [
                    o.cookie_cutter,
                    o.imprinting,
                    o.boolean_post_process,
                    o.imprint_and_classify_faces,
                    o.keep_other_brep,
                ]
                .map(|f| f64::from(u8::from(f))),
            );
            // m_bManifoldBoolean, m_bIntersectionLoopClosed (the B-rep constructor sets
            // both; an indeterminate value would be a harness error)
            for flag in [m.manifold_boolean, m.intersection_loop_closed] {
                values.push(f64::from(u8::from(
                    flag.ok_or_else(|| err("indeterminate merge flag"))?,
                )));
            }
            values.push(m.tag_id as f64);
        }
        Op::MergeEdge { .. } | Op::MakeFaceuse { .. } => {
            let b = b.as_mut().ok_or_else(|| err("no other B-rep"))?;
            let mut relations = relate(&mut a, b, &p.mates)?;
            // A fresh SmTopology record per vertex, edge and face of both operands,
            // under the identities notified below (copies propagate their flags).
            let mut records = BTreeMap::new();
            for (base, brep) in [(0, &a), (1_000_000, &*b)] {
                for v in brep.vertices().ids() {
                    records.insert(ObjectKey(base + u64::from(v.index())), Topology::new());
                }
                for e in brep.edges().ids() {
                    records.insert(
                        ObjectKey(2_000_000 + base + u64::from(e.index())),
                        Topology::new(),
                    );
                }
                for f in brep.faces().ids() {
                    records.insert(
                        ObjectKey(4_000_000 + base + u64::from(f.index())),
                        Topology::new(),
                    );
                }
            }
            for vu in a.vertexuses().ids() {
                records.insert(
                    ObjectKey(6_000_000 + u64::from(vu.index())),
                    Topology::new(),
                );
            }
            let mut attributes = AttributeStore::new();
            let vertex_source = |v: VertexId| key(1_000_000, vertex::TYPE, v.index());
            // The primary's vertices and vertex uses (the ones a new shell vertex makes).
            let vertex_dest = |e: solidcore::EntityRef| match e {
                solidcore::EntityRef::Vertex(v) => key(0, vertex::TYPE, v.index()),
                solidcore::EntityRef::Vertexuse(vu) => {
                    key(6_000_000, topology::vertexuse::TYPE, vu.index())
                }
                _ => key(9_000_000, topology::topology::TYPE, 0),
            };
            let edge_source = |e: EdgeId| key(3_000_000, edge::TYPE, e.index());
            let edge_dest = |e: EdgeId| key(2_000_000, edge::TYPE, e.index());
            let face_source = |f: FaceId| key(5_000_000, face::p2::TYPE, f.index());
            let face_dest = |f: FaceId| key(4_000_000, face::p2::TYPE, f.index());
            let mut copy = TopologyCopy {
                vertices: VertexCopy {
                    records: &mut records,
                    attributes: &mut attributes,
                    source_key: &vertex_source,
                    destination_key: &vertex_dest,
                },
                edge_source_key: &edge_source,
                edge_destination_key: &edge_dest,
                face_source_key: &face_source,
                face_destination_key: &face_dest,
            };
            match &i.op {
                Op::MergeEdge { edgeuse, region } => {
                    match merge_edge(
                        &k,
                        &mut a,
                        b,
                        opt_id::<EdgeuseId>(*edgeuse),
                        opt_id::<RegionId>(*region),
                        &mut relations,
                        &mut copy,
                        None,
                    ) {
                        Ok(e) => ids.extend([0, id(e)]),
                        Err(e) if e.is_unported() => {
                            return Output::unported(&e).ok_or_else(|| err(&e))
                        }
                        Err(e) => ids.extend([status(&e), -1]),
                    }
                }
                Op::MakeFaceuse {
                    faceuse,
                    region,
                    new_faces,
                } => {
                    let mut made = Vec::new();
                    match make_faceuse(
                        &k,
                        &mut a,
                        b,
                        FaceuseId::from_index(u32::try_from(*faceuse).map_err(|e| err(&e))?),
                        opt_id::<RegionId>(*region),
                        &mut relations,
                        &mut copy,
                        None,
                        new_faces.then_some(&mut made),
                    ) {
                        Ok(r) => {
                            ids.extend([0, id(r.old_region), id(r.new_region), made.len() as i64]);
                            ids.extend(made.iter().map(|f| i64::from(f.index())));
                        }
                        Err(e) if e.is_unported() => {
                            return Output::unported(&e).ok_or_else(|| err(&e))
                        }
                        Err(e) => ids.extend([status(&e), -1, -1, 0]),
                    }
                }
                _ => return Err(err("unreachable op")),
            }
            snapshots(&[&a, b], &mut ids, &mut values)?;
        }
        Op::MergeAddedTopology { .. } | Op::MergeAddedFace { .. } => {
            let mut state = match MergeState::<TopologyIntersector<H>>::with_breps(
                &k,
                (&a, brep_h(0)),
                None,
                p.tol,
                p.ang,
                options.as_ref(),
            ) {
                Ok(m) => m,
                Err(e) => return Output::unported(&e).ok_or_else(|| err(&e)),
            };
            let mut two = Two::new(a, None);
            let mut k = Kernel::default();
            let handles = |list: &Option<Vec<i64>>, kind: u8| -> Option<Vec<H>> {
                list.as_ref()
                    .map(|l| l.iter().map(|&x| hd(0, kind, x as u32)).collect())
            };
            let result = match &i.op {
                Op::MergeAddedTopology {
                    faces,
                    edges,
                    vertices,
                    flags,
                } => {
                    let (f, e, v) = (
                        handles(faces, K_FACE),
                        handles(edges, K_EDGE),
                        handles(vertices, K_VERTEX),
                    );
                    state.merge_added_topology(
                        &mut k,
                        &mut two,
                        f.as_deref(),
                        e.as_deref(),
                        v.as_deref(),
                        MergeAddedOptions {
                            making_manifold_solid: flags[0],
                            fast_edge_compare: flags[1],
                            do_region_nesting: flags[2],
                            ignore_problems: flags[3],
                        },
                    )
                }
                Op::MergeAddedFace { face } => {
                    state.merge_added_face(&mut k, &mut two, hd(0, K_FACE, *face as u32))
                }
                _ => return Err(err("unreachable op")),
            };
            match result {
                Ok(m) => {
                    ids.extend([0, m.num_edges_stitched as i64]);
                    values.extend([
                        m.max_vert_gap,
                        m.max_edge_gap,
                        m.min_unstitched_vert_gap,
                        m.min_unstitched_edge_gap,
                    ]);
                }
                Err(e) if e.is_unported() => return Output::unported(&e).ok_or_else(|| err(&e)),
                Err(e) => return Err(err(&format!("Rust failed without outputs: {e}"))),
            }
            let brep = two.brep_r(brep_h(0)).map_err(|e| err(&e))?;
            snapshots(&[brep], &mut ids, &mut values)?;
        }
    }
    Ok(Output::new()
        .ints("topology", ids)
        .values("geometry", values))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.merge_p2"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let p = prepare(&i)?;
        let raw = ffi::eval(
            p5::operand(&p.a),
            p.b.as_ref().map(p5::operand),
            p.tol,
            p.ang,
            i.options,
            request(&i.op, &p.mates),
        )
        .map_err(|e| err(&e))?;
        output(&raw)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let p = prepare(&i)?;
        run_rust(&i, &p)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        generate(rng)
    }
}

fn counts_of(spec: &Value) -> Option<[usize; 10]> {
    let spec: Spec = serde_json::from_value(spec.clone()).ok()?;
    let p = p5::program(&spec).ok()?;
    ffi::counts(p5::operand(&p)).ok()
}

/// Operands with faces: planar faces at several scales and B-rep files.
fn face_operand(rng: &mut Rng) -> Option<Value> {
    Some(match rng.below(4) {
        0 | 1 => {
            let scale = *rng.choose(&[1.0e-6, 1.0, 1.0e6])?;
            let o = rng.point3(-scale, scale);
            json!({"kind":"graph","brep":{"kind":"face","surface":{"plane":{"o":o,"n":[0.0,0.0,1.0]}},
                "dom":[-scale,-scale,scale,scale]}})
        }
        _ => {
            let file = *rng.choose(&[
                "plane_square.smb",
                "plane_circ.smb",
                "sphere.smb",
                "surf_extrude.smb",
            ])?;
            json!({"kind":"graph","brep":{"kind":"file","file":file}})
        }
    })
}

/// A permutation of `0..n` or by-index mates, with some entries dropped.
fn mates(rng: &mut Rng, primaries: usize, others: usize, drops: &[u64], shuffle: bool) -> Vec<i64> {
    let drop_per_8 = rng.choose(drops).copied().unwrap_or(0);
    let mut targets: Vec<i64> = (0..primaries as i64).collect();
    if shuffle {
        for j in (1..targets.len()).rev() {
            let at = rng.below((j + 1) as u64) as usize;
            targets.swap(j, at);
        }
    }
    (0..others)
        .map(|j| {
            if rng.below(8) < drop_per_8 {
                -1
            } else {
                targets.get(j).copied().unwrap_or(-1)
            }
        })
        .collect()
}

fn pick(rng: &mut Rng, n: usize, null_per_8: u64) -> i64 {
    if n == 0 || rng.below(8) < null_per_8 {
        -1
    } else {
        rng.below(n as u64) as i64
    }
}

fn subset(rng: &mut Rng, n: usize) -> Option<Vec<i64>> {
    if rng.below(3) == 0 {
        return None;
    }
    Some((0..n as i64).filter(|_| rng.below(2) == 0).collect())
}

/// A generated case; never `None` (that would end the unit's generated cases): a
/// draw whose operands collapse during construction is retried, then replaced by a
/// constructor case.
fn generate(rng: &mut Rng) -> Option<Value> {
    for _ in 0..8 {
        let which = rng.below(10);
        if let Some(v) = generate_one(rng, which) {
            return Some(v);
        }
    }
    generate_one(rng, 0).or_else(|| Some(json!({"op":"construct","primary":{"kind":"empty"}})))
}

fn generate_one(rng: &mut Rng, which: u64) -> Option<Value> {
    match which {
        0 | 1 => {
            let (primary, _) = p5::generated_operand(rng, 1.0);
            let random = rng.uniform(1e-9, 10.0);
            let tol = *rng.choose(&[
                0.0, -0.0, -1.0, 0.5e-12, 1.0e-12, 1.0e-3, 1.0, 1.0e6, random,
            ])?;
            let other = if tol < 1.0e-12 || rng.below(2) == 0 {
                Some(p5::generated_operand(rng, 1.0).0)
            } else {
                None
            };
            let ang = *rng.choose(&[0.0, -0.0, 0.1, -1.0, std::f64::consts::PI])?;
            let options = (rng.below(3) != 0).then(|| rng.below(32));
            Some(
                json!({"op":"construct","primary":primary,"other":other,"tol":tol,"ang":ang,"options":options}),
            )
        }
        2..=4 => {
            // The same construction for both operands, so mates match geometry.
            let primary = if rng.below(2) == 0 {
                face_operand(rng)?
            } else {
                {
                    let scale = *rng.choose(&[1.0e-6, 1.0, 1.0e6])?;
                    p5::generated_operand(rng, scale).0
                }
            };
            let other = if rng.below(4) == 0 {
                p5::generated_operand(rng, 1.0).0
            } else {
                primary.clone()
            };
            let (a, b) = (counts_of(&primary)?, counts_of(&other)?);
            let shuffle = rng.below(4) == 0;
            let edges = mates(rng, a[7], b[7], &[0, 2, 8], shuffle);
            let vertices = mates(rng, a[9], b[9], &[0, 1, 8], shuffle);
            Some(json!({"op":"merge_edge","primary":primary,"other":other,
                "tol":rng.uniform(1e-6, 1e-2),
                "mates":{"edges":edges,"vertices":vertices},
                "edgeuse":pick(rng, b[6], 1),"region":pick(rng, a[0], 4)}))
        }
        5..=7 => {
            let primary = face_operand(rng)?;
            let other = primary.clone();
            let (a, b) = (counts_of(&primary)?, counts_of(&other)?);
            if b[2] == 0 {
                return None;
            }
            let shuffle = rng.below(4) == 0;
            let edges = mates(rng, a[7], b[7], &[0, 0, 1], shuffle);
            let scramble = rng.below(5) == 0;
            let vertices = mates(rng, a[9], b[9], &[0, 0, 1], scramble);
            let faces = mates(rng, a[3], b[3], &[8, 8, 4], false);
            Some(json!({"op":"make_faceuse","primary":primary,"other":other,
                "tol":rng.uniform(1e-6, 1e-2),
                "mates":{"edges":edges,"vertices":vertices,"faces":faces},
                "faceuse":pick(rng, b[2], 0),"region":pick(rng, a[0], 5),"new_faces":rng.below(2)==0}))
        }
        _ => {
            let primary = if rng.below(2) == 0 {
                face_operand(rng)?
            } else {
                p5::generated_operand(rng, 1.0).0
            };
            let a = counts_of(&primary)?;
            let tol = *rng.choose(&[1.0e-12, 1.0e-6, 1.0e-3, 0.1])?;
            if a[3] > 0 && rng.below(3) == 0 {
                return Some(json!({"op":"merge_added_face","primary":primary,"tol":tol,
                    "face":rng.below(a[3] as u64)}));
            }
            let flags: Vec<bool> = (0..5).map(|_| rng.below(2) == 0).collect();
            Some(
                json!({"op":"merge_added_topology","primary":primary,"tol":tol,
                "faces":subset(rng, a[3]),"edges":subset(rng, a[7]),"vertices":subset(rng, a[9]),
                "flags":flags,"options":(rng.below(2)==0).then(|| rng.below(32))}),
            )
        }
    }
}
