//! C++ parity for merge control, topology cleanup, face coincidence, vertex copies,
//! region attributes and file-local region/translation helpers. Topology fixtures
//! are constructed by each side's own kernel (the Rust side's by the Rust `.smb` reader
//! and constructors, renumbered into the fixture's traversal order; no C++ export
//! reaches it); the C++ side's post-op graph is imported to compare it. Deferred paths still run the native helper.
use super::topology_edgeuse_p1::{self as spec, BrepIn};
use super::topology_vertexuse::import_raw;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::boolean::merge::p1 as port;
use solidcore::boolean::merge::p4::{BooleanKernels, KernelManifoldIntersector, ManifoldIntersector};
use solidcore::boolean::topology_intersector::p1::Relations;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{ids, Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::context::{Context, NotifyCallback};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::kernel::Kernel;
use solidcore::math::{Vec3, EFF_ZERO_SQ, PI};
use solidcore::topology::shell::ShellMembers;
use solidcore::topology::topology::Topology;
use solidcore::topology::{
    ArenaId, Brep, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, LoopId, LoopuseId,
    LoopuseStart, RegionId, ShellId, VertexId, VertexuseId, VertexuseOwner,
};
use solidcore::SolidError;
use solidcore_ffi::units::boolean_merge_p1 as ffi;
use std::collections::BTreeMap;

/// The op of `find_brep_region_of_other_face` (`SmMerge::FindBrepRegionOfOtherFace`).
const OP_FIND_REGION: u8 = 5;
/// The op of `SmMerge::FixFoundFaceCoincidence`, which can edit both operands.
const OP_FACE_COINCIDENCE: u8 = 2;

/// Takes the other operand's graph off the end of op 2's C++ record. The record is:
/// changed flag; the count and faces of each side; the count and mates of the primary's
/// edges and of the other's ([`face_coincidence_record`]); then the length of the other
/// operand's raw export, and the export.
fn split_other_graph(record: &mut Vec<f64>) -> Result<Brep, HarnessError> {
    let short = || harness("short face coincidence record");
    let at = |record: &[f64], i: usize| -> Result<usize, HarnessError> {
        let v = *record.get(i).ok_or_else(short)?;
        if v.is_finite() && v >= 0.0 && v.fract() == 0.0 {
            Ok(v as usize)
        } else {
            Err(short())
        }
    };
    // the position after a count at `i` and the values it counts
    let skip = |record: &[f64], i: usize| -> Result<usize, HarnessError> {
        at(record, i)?
            .checked_add(i)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(short)
    };
    let second = skip(record, 1)?; // the other side's face count
    let primary_edges = skip(record, second)?;
    let other_edges = skip(record, primary_edges)?;
    let len_at = skip(record, other_edges)?;
    let len = at(record, len_at)?;
    let start = len_at.checked_add(1).ok_or_else(short)?;
    let end = start.checked_add(len).ok_or_else(short)?;
    if end != record.len() {
        return Err(harness("face coincidence record has trailing values"));
    }
    let (mut other, _) = import_raw(record.get(start..end).ok_or_else(short)?)?;
    // the face loop lists and loop edge lists the export does not carry, derived from
    // the exported uses as the Rust B-rep holds them (as `topology.brep_p5`)
    other.refresh_cached_topology();
    record.truncate(len_at);
    Ok(other)
}

/// The values of op 2's record that follow the changed flag: the faces of each side and
/// the edge mates (the primary edges' other mates, then the other edges' B-rep mates, as
/// numbers of `edges`' side's edges, -1 for none), all in the numbering of the B-reps
/// after the call, as the shim writes them.
fn face_coincidence_record(
    changed: bool,
    faces: &[FaceId],
    other_faces: &[FaceId],
    b: &Brep,
    other: &Brep,
    mates: &solidcore::boolean::topology_intersector::p1::MateMap<EdgeId>,
) -> Vec<f64> {
    let (cb, co) = (Canon::reachable(b), Canon::reachable(other));
    let mut out = vec![f64::from(u8::from(changed)), faces.len() as f64];
    out.extend(faces.iter().map(|&f| cb.faces.of(Some(f)) as f64));
    out.push(other_faces.len() as f64);
    out.extend(other_faces.iter().map(|&f| co.faces.of(Some(f)) as f64));
    out.push(cb.edges.items.len() as f64);
    out.extend(cb.edges.items.iter().map(|&e| {
        co.edges
            .of(mates.primary_to_other().get(&e).copied()) as f64
    }));
    out.push(co.edges.items.len() as f64);
    out.extend(co.edges.items.iter().map(|&e| {
        cb.edges
            .of(mates.other_to_primary().get(&e).copied()) as f64
    }));
    out
}

/// Adds the other operand's records and links (op 2 only), compared like the primary's.
fn with_other_graph(mut out: Output, other: Option<&Brep>) -> Output {
    if let Some(other) = other {
        let mut g = graph(other, Vec::new());
        out.values.extend(g.values.remove("geometry").map(|v| ("other_geometry".to_owned(), v)));
        out.ints.extend(g.ints.remove("graph").map(|v| ("other_graph".to_owned(), v)));
    }
    out
}

/// Differential runner for `boolean.merge_p1`.
pub struct Runner;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: u8,
    args: [u8; 3],
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    other: Option<BrepIn>,
    #[serde(default)]
    tolerances: Option<[f64; 2]>,
    #[serde(default = "half")]
    parameter: f64,
}
fn half() -> f64 {
    0.5
}
fn harness(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn options(mask: u8) -> port::MergeOptions {
    port::MergeOptions::new(
        mask & 1 != 0,
        mask & 2 != 0,
        mask & 4 != 0,
        mask & 8 != 0,
        mask & 16 != 0,
    )
}
fn flags(o: port::MergeOptions) -> Vec<f64> {
    [
        o.cookie_cutter,
        o.imprinting,
        o.boolean_post_process,
        o.imprint_and_classify_faces,
        o.keep_other_brep,
    ]
    .into_iter()
    .map(|b| f64::from(u8::from(b)))
    .collect()
}
fn control([a, b, c]: [u8; 3]) -> Vec<f64> {
    let mut state = port::MergeState::new(());
    let mut v = flags(state.options);
    v.push(state.tag_id as f64);
    let o = options(a);
    v.extend(flags(o));
    state.set_merge_options((b != 0).then_some(&o));
    v.extend(flags(state.options));
    v.push(state.tag_id as f64);
    state.options = options(c);
    v.extend(flags(state.options));
    v.push(state.tag_id as f64);
    v
}
/// An operand's Rust B-rep, built by the Rust kernel ([`crate::own_builder::build_any`]:
/// the Rust `.smb` reader or constructors, in the fixture's traversal order), with its
/// geometry owned under arena identities (the B-rep cache solves of op 5's point
/// classification, `BrepQuery::default`, find faces and edges by them, as upstream's by
/// pointer); the output of a failed build otherwise.
fn own_operand(k: &mut Kernel, src: &BrepIn) -> Result<Result<Brep, Output>, HarnessError> {
    use crate::own_builder::{build_any, failed_output, Built, Conventions};
    match build_any(k, src, false, false, &Conventions::TOPOLOGY_FIXTURE)? {
        Built::Brep(b) => {
            let mut b = *b;
            crate::util::own_arena_geometry(&mut b);
            // the new owners drop the face tolerances cached on the surfaces
            let faces: Vec<_> = b.faces().ids().collect();
            for f in faces {
                b.sync_face_surface_tolerance(f)
                    .map_err(|e| harness(e.to_string()))?;
            }
            Ok(Ok(b))
        }
        Built::Failed(e) => Ok(Err(failed_output(&e))),
    }
}
fn block(i: &Inputs) -> Result<(String, Vec<f64>), HarnessError> {
    if i.op == 0 {
        return Ok((String::new(), Vec::new()));
    }
    Ok((
        spec::path_of(i.brep.as_ref())?,
        spec::brep_block_of(i.brep.as_ref(), false, false)?,
    ))
}
fn other_block(i: &Inputs) -> Result<(String, Vec<f64>), HarnessError> {
    if let Some(other) = &i.other {
        Ok((
            spec::path_of(Some(other))?,
            spec::brep_block_of(Some(other), false, false)?,
        ))
    } else {
        block(i)
    }
}
fn obj(k: u64, t: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(k),
        type_code: t,
    }
}
fn id<I: ArenaId>(v: Option<I>) -> i64 {
    v.map_or(-1, |v| i64::from(v.index()))
}
/// One kind's records in first-seen order of [`Canon::of`] and each arena index's
/// position in it.
pub(crate) struct Order<I> {
    pub(crate) items: Vec<I>,
    index: BTreeMap<u32, i64>,
}
impl<I: ArenaId + Copy> Order<I> {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            index: BTreeMap::new(),
        }
    }
    fn add(&mut self, id: Option<I>) {
        if let Some(id) = id {
            if !self.index.contains_key(&id.index()) {
                self.index.insert(id.index(), self.items.len() as i64);
                self.items.push(id);
            }
        }
    }
    /// The canonical number of `id` (`-1` for none).
    pub(crate) fn of(&self, id: Option<I>) -> i64 {
        id.map_or(-1, |v| self.index.get(&v.index()).copied().unwrap_or(-3))
    }
    /// [`Order::of`] for a record slot holding an arena index (negative: none).
    pub(crate) fn of_slot(&self, slot: f64) -> f64 {
        if slot < 0.0 {
            return slot;
        }
        self.index.get(&(slot as u32)).copied().unwrap_or(-3) as f64
    }
}
/// Canonical record numbering: the shims export a B-rep by walking it from its regions
/// and edge list (`sc_edgeuse_graph.h` `walk`), so a C++ import numbers records in
/// first-seen order, while Rust arenas append new records. Both sides are renumbered
/// by the same walk (the shim's, from the B-rep's regions in arena order and the edge
/// list; on an imported C++ export it is the identity), so records created by the op
/// compare by position in the topology, not by allocation order. [`graph`] compares only
/// the records the walk reaches ([`Canon::reachable`]), as the C++ export walk emits
/// only those: a record upstream deletes stays in the Rust arena, unlinked (the arena
/// model's documented deviation, e.g. `MakeEdgeVertexTopology`'s vertex-shell vertex
/// use), and is not part of the B-rep. A record that is wrongly still linked is reached,
/// so it is still compared. The unreached count is a report diagnostic ([`graph`]).
/// [`Canon::of`] (numbering record slots) also numbers the unreached records, after the
/// reached ones.
pub(crate) struct Canon {
    pub(crate) regions: Order<RegionId>,
    pub(crate) shells: Order<ShellId>,
    pub(crate) faceuses: Order<FaceuseId>,
    pub(crate) faces: Order<FaceId>,
    pub(crate) loopuses: Order<LoopuseId>,
    pub(crate) loops: Order<LoopId>,
    pub(crate) edgeuses: Order<EdgeuseId>,
    pub(crate) edges: Order<EdgeId>,
    pub(crate) vertexuses: Order<VertexuseId>,
    pub(crate) vertices: Order<VertexId>,
}
impl Canon {
    /// Every record: the walk from the B-rep's regions and edge list, then the
    /// unreachable records in arena order (and the walk from them).
    pub(crate) fn of(b: &Brep) -> Self {
        Self::walk(b, true, false)
    }

    /// Only the records reachable from the B-rep's regions and edge list, in the order
    /// of [`Canon::of`] (an arena keeps the records an op deleted, reset).
    pub(crate) fn reachable(b: &Brep) -> Self {
        Self::walk(b, false, false)
    }

    /// Native list order, excluding detached arena records that C++ deleted.
    pub(crate) fn native_reachable(b: &Brep) -> Self {
        Self::walk(b, false, true)
    }

    #[allow(clippy::too_many_lines)]
    fn walk(b: &Brep, unreachable: bool, native: bool) -> Self {
        let mut g = Self {
            regions: Order::new(),
            shells: Order::new(),
            faceuses: Order::new(),
            faces: Order::new(),
            loopuses: Order::new(),
            loops: Order::new(),
            edgeuses: Order::new(),
            edges: Order::new(),
            vertexuses: Order::new(),
            vertices: Order::new(),
        };
        if native {
            for &e in b.edge_list() {
                g.edges.add(Some(e));
            }
            for &v in b.vertex_list() {
                g.vertices.add(Some(v));
            }
        }
        for (id, r) in b.regions().iter() {
            if r.in_brep {
                g.regions.add(Some(id));
            }
        }
        for &e in b.edge_list() {
            g.edges.add(Some(e));
        }
        let (mut r, mut s, mut fu, mut lu, mut eu, mut vu, mut l, mut e, mut v) =
            (0, 0, 0, 0, 0, 0, 0, 0, 0);
        let mut more = true;
        while more {
            more = false;
            while let Some(&id) = g.regions.items.get(r) {
                if let Ok(rec) = b.region(id) {
                    for &x in &rec.shells {
                        g.shells.add(Some(x));
                    }
                }
                r += 1;
                more = true;
            }
            while let Some(&id) = g.shells.items.get(s) {
                if let Ok(rec) = b.shell(id) {
                    match &rec.members {
                        ShellMembers::Faceuses(list) => {
                            for &x in list {
                                g.faceuses.add(Some(x));
                            }
                        }
                        ShellMembers::Edgeuse(x) => g.edgeuses.add(Some(*x)),
                        ShellMembers::Vertexuse(x) => g.vertexuses.add(Some(*x)),
                        ShellMembers::Empty => {}
                    }
                }
                s += 1;
                more = true;
            }
            while let Some(&id) = g.faceuses.items.get(fu) {
                if let Ok(rec) = b.faceuse(id) {
                    g.faces.add(rec.face);
                    g.faceuses.add(rec.mate);
                    for &x in &rec.loopuses {
                        g.loopuses.add(Some(x));
                    }
                }
                fu += 1;
                more = true;
            }
            while let Some(&id) = g.loopuses.items.get(lu) {
                if let Ok(rec) = b.loopuse(id) {
                    g.loops.add(rec.loop_);
                    g.loopuses.add(rec.mate);
                    match rec.start {
                        LoopuseStart::Edgeuse(start) => {
                            g.edgeuses.add(start);
                            // the CCW walk of an edge loop
                            let mut q = start;
                            let mut k = 0;
                            while let Some(x) = q {
                                if k > b.edgeuses().len() {
                                    break;
                                }
                                g.edgeuses.add(Some(x));
                                q = b.edgeuse(x).ok().and_then(|x| x.ccw);
                                if q == start {
                                    break;
                                }
                                k += 1;
                            }
                        }
                        LoopuseStart::Vertexuse(x) => g.vertexuses.add(x),
                        LoopuseStart::Unknown => {}
                    }
                }
                lu += 1;
                more = true;
            }
            while let Some(&id) = g.edgeuses.items.get(eu) {
                if let Ok(rec) = b.edgeuse(id) {
                    g.edges.add(rec.edge);
                    match rec.owner {
                        EdgeuseOwner::Loopuse(x) => g.loopuses.add(x),
                        EdgeuseOwner::Shell(x) => g.shells.add(x),
                        EdgeuseOwner::Unknown => {}
                    }
                    g.edgeuses.add(rec.cw);
                    g.edgeuses.add(rec.ccw);
                    g.vertexuses.add(rec.vertexuse);
                    // GetNext / GetLast: the neighbours in the edge's circular list
                    let list = rec
                        .edge
                        .and_then(|x| b.edge(x).ok())
                        .map_or(&[][..], |x| x.edgeuses.as_slice());
                    if let Some(at) = list.iter().position(|&x| x == id) {
                        let n = list.len();
                        g.edgeuses.add(list.get((at + 1) % n).copied());
                        g.edgeuses.add(list.get((at + n - 1) % n).copied());
                    }
                }
                eu += 1;
                more = true;
            }
            while let Some(&id) = g.edges.items.get(e) {
                if let Ok(rec) = b.edge(id) {
                    for &x in &rec.edgeuses {
                        g.edgeuses.add(Some(x));
                    }
                }
                e += 1;
                more = true;
            }
            while let Some(&id) = g.vertexuses.items.get(vu) {
                if let Ok(rec) = b.vertexuse(id) {
                    g.vertices.add(rec.vertex);
                    match rec.owner {
                        VertexuseOwner::Edgeuse(x) => g.edgeuses.add(x),
                        VertexuseOwner::Loopuse(x) => g.loopuses.add(x),
                        VertexuseOwner::Shell(x) => g.shells.add(x),
                        VertexuseOwner::Unknown => {}
                    }
                }
                vu += 1;
                more = true;
            }
            while let Some(&id) = g.vertices.items.get(v) {
                if let Ok(rec) = b.vertex(id) {
                    for &x in &rec.vertexuses {
                        g.vertexuses.add(Some(x));
                    }
                }
                v += 1;
                more = true;
            }
            while let Some(&id) = g.loops.items.get(l) {
                if let Ok(rec) = b.loop_(id) {
                    g.loopuses.add(rec.loopuse);
                }
                l += 1;
                more = true;
            }
            if !more && unreachable {
                // unreachable records, in arena order, then walk on from them
                let before = g.len();
                for (id, _) in b.regions().iter() {
                    g.regions.add(Some(id));
                }
                for (id, _) in b.shells().iter() {
                    g.shells.add(Some(id));
                }
                for (id, _) in b.faceuses().iter() {
                    g.faceuses.add(Some(id));
                }
                for (id, _) in b.faces().iter() {
                    g.faces.add(Some(id));
                }
                for (id, _) in b.loopuses().iter() {
                    g.loopuses.add(Some(id));
                }
                for (id, _) in b.loops().iter() {
                    g.loops.add(Some(id));
                }
                for (id, _) in b.edgeuses().iter() {
                    g.edgeuses.add(Some(id));
                }
                for (id, _) in b.edges().iter() {
                    g.edges.add(Some(id));
                }
                for (id, _) in b.vertexuses().iter() {
                    g.vertexuses.add(Some(id));
                }
                for (id, _) in b.vertices().iter() {
                    g.vertices.add(Some(id));
                }
                more = g.len() != before;
            }
        }
        g
    }
    /// The arena records of `b` the walk did not reach (deleted records the arenas
    /// keep, or a leak).
    pub(crate) fn unreached(&self, b: &Brep) -> usize {
        let all = b.regions().len()
            + b.shells().len()
            + b.faceuses().len()
            + b.faces().len()
            + b.loopuses().len()
            + b.loops().len()
            + b.edgeuses().len()
            + b.edges().len()
            + b.vertexuses().len()
            + b.vertices().len();
        all.saturating_sub(self.len())
    }
    fn len(&self) -> usize {
        self.regions.items.len()
            + self.shells.items.len()
            + self.faceuses.items.len()
            + self.faces.items.len()
            + self.loopuses.items.len()
            + self.loops.items.len()
            + self.edgeuses.items.len()
            + self.edges.items.len()
            + self.vertexuses.items.len()
            + self.vertices.items.len()
    }
}
/// The B-rep's records and links in [`Canon`] numbering, after `record` (which the
/// caller has already put in that numbering, see [`Canon::of`]).
pub(crate) fn graph(b: &Brep, record: Vec<f64>) -> Output {
    graph_counted(b, record, true)
}

/// [`graph`] (kept for its callers): the reachable topology ([`Canon::reachable`]) and
/// its record counts, as the C++ side's fresh export walk.
pub(crate) fn graph_reachable(b: &Brep, record: Vec<f64>) -> Output {
    graph_counted(b, record, true)
}

#[allow(clippy::too_many_lines)]
fn graph_counted(b: &Brep, record: Vec<f64>, reachable: bool) -> Output {
    let c = if reachable {
        Canon::reachable(b)
    } else {
        Canon::of(b)
    };
    let mut ints = if reachable {
        [
            c.regions.items.len(),
            c.shells.items.len(),
            c.faceuses.items.len(),
            c.faces.items.len(),
            c.loopuses.items.len(),
            c.loops.items.len(),
            c.edgeuses.items.len(),
            c.edges.items.len(),
            c.vertexuses.items.len(),
            c.vertices.items.len(),
        ]
        .map(|n| n as i64)
        .to_vec()
    } else {
        vec![
            b.regions().len() as i64,
            b.shells().len() as i64,
            b.faceuses().len() as i64,
            b.faces().len() as i64,
            b.loopuses().len() as i64,
            b.loops().len() as i64,
            b.edgeuses().len() as i64,
            b.edges().len() as i64,
            b.vertexuses().len() as i64,
            b.vertices().len() as i64,
        ]
    };
    let regions: Vec<_> = c
        .regions
        .items
        .iter()
        .filter_map(|&x| b.region(x).ok())
        .collect();
    let shells: Vec<_> = c
        .shells
        .items
        .iter()
        .filter_map(|&x| b.shell(x).ok())
        .collect();
    let faceuses: Vec<_> = c
        .faceuses
        .items
        .iter()
        .filter_map(|&x| b.faceuse(x).ok())
        .collect();
    let faces: Vec<_> = c
        .faces
        .items
        .iter()
        .filter_map(|&x| b.face(x).ok())
        .collect();
    let loopuses: Vec<_> = c
        .loopuses
        .items
        .iter()
        .filter_map(|&x| b.loopuse(x).ok())
        .collect();
    let loops: Vec<_> = c
        .loops
        .items
        .iter()
        .filter_map(|&x| b.loop_(x).ok())
        .collect();
    let edgeuses: Vec<_> = c
        .edgeuses
        .items
        .iter()
        .filter_map(|&x| b.edgeuse(x).ok())
        .collect();
    let edges: Vec<_> = c
        .edges
        .items
        .iter()
        .filter_map(|&x| b.edge(x).ok())
        .collect();
    let vertexuses: Vec<_> = c
        .vertexuses
        .items
        .iter()
        .filter_map(|&x| b.vertexuse(x).ok())
        .collect();
    let vertices: Vec<_> = c
        .vertices
        .items
        .iter()
        .filter_map(|&x| b.vertex(x).ok())
        .collect();
    for r in &regions {
        ints.push(i64::from(r.in_brep));
    }
    for f in &faces {
        ints.extend([
            c.shells.of(Some(f.shell)),
            c.faceuses.of(f.faceuse),
            i64::from(f.rectangular_trim),
            f.loops.len() as i64,
        ]);
        ints.extend(f.loops.iter().map(|&v| c.loops.of(Some(v))));
    }
    for l in &loops {
        ints.extend([
            c.faces.of(Some(l.face)),
            c.loopuses.of(l.loopuse),
            l.edges.len() as i64,
        ]);
        for e in &l.edges {
            ints.extend([c.edges.of(Some(e.edge)), i64::from(e.forward)]);
        }
    }
    for l in &loopuses {
        ints.extend(match l.start {
            LoopuseStart::Unknown => [0, -1],
            LoopuseStart::Edgeuse(e) => [1, c.edgeuses.of(e)],
            LoopuseStart::Vertexuse(v) => [2, c.vertexuses.of(v)],
        });
    }
    for e in &edgeuses {
        ints.extend(match e.owner {
            EdgeuseOwner::Unknown => [0, -1],
            EdgeuseOwner::Loopuse(l) => [1, c.loopuses.of(l)],
            EdgeuseOwner::Shell(sh) => [2, c.shells.of(sh)],
        });
    }
    for v in &vertexuses {
        ints.push(c.vertices.of(v.vertex));
        ints.extend(match v.owner {
            VertexuseOwner::Unknown => [0, -1],
            VertexuseOwner::Edgeuse(e) => [1, c.edgeuses.of(e)],
            VertexuseOwner::Loopuse(l) => [2, c.loopuses.of(l)],
            VertexuseOwner::Shell(sh) => [3, c.shells.of(sh)],
        });
    }
    for s in &shells {
        ints.push(c.regions.of(s.region));
        ints.push(s.faces.len() as i64);
        ints.extend(s.faces.iter().map(|&v| c.faces.of(Some(v))));
    }
    for f in &faceuses {
        ints.extend([
            c.shells.of(f.shell),
            c.faces.of(f.face),
            c.faceuses.of(f.mate),
            f.orientation as i64,
            f.loopuses.len() as i64,
        ]);
        ints.extend(f.loopuses.iter().map(|&v| c.loopuses.of(Some(v))));
    }
    for l in &loopuses {
        ints.extend([
            c.faceuses.of(l.faceuse),
            c.loops.of(l.loop_),
            c.loopuses.of(l.mate),
            l.orientation as i64,
        ]);
    }
    for e in &edges {
        ints.push(e.edgeuses.len() as i64);
        ints.extend(e.edgeuses.iter().map(|&v| c.edgeuses.of(Some(v))));
    }
    for e in &edgeuses {
        ints.extend([
            c.edges.of(e.edge),
            c.edgeuses.of(e.ccw),
            c.edgeuses.of(e.cw),
            c.vertexuses.of(e.vertexuse),
            e.orientation as i64,
        ]);
    }
    let mut values = Vec::new();
    for v in &vertexuses {
        values.extend([v.uv_point.x, v.uv_point.y]);
    }
    for v in &vertices {
        values.extend([v.point.x, v.point.y, v.point.z, v.tolerance]);
        ints.push(v.vertexuses.len() as i64);
        ints.extend(v.vertexuses.iter().map(|&x| c.vertexuses.of(Some(x))));
    }
    for f in &faces {
        let a = f.uv_domain.min();
        let z = f.uv_domain.max();
        values.extend([a.x, a.y, z.x, z.y, f.tolerance]);
    }
    for e in &edges {
        values.extend([e.interval.min(), e.interval.max(), e.tolerance]);
    }
    let out = Output::new()
        .values("record", record)
        .values("geometry", values)
        .ints("graph", ints);
    match c.unreached(b) {
        0 => out,
        n => out.diagnostic("unreached_records", n as i64),
    }
}
#[derive(Debug, Default)]
struct Recorder {
    events: Vec<[i64; 5]>,
}
impl NotifyCallback for Recorder {
    fn execute(
        &mut self,
        operation: NotifyOperation,
        caller: ObjectRef,
        a: Option<ObjectRef>,
        b: Option<ObjectRef>,
        c: Option<ObjectRef>,
    ) -> solidcore::Result<()> {
        if matches!(
            operation,
            NotifyOperation::Copy | NotifyOperation::RegPropagation
        ) {
            let key = |o: Option<ObjectRef>| o.map_or(-1, |o| o.key.0 as i64);
            self.events.push([
                i64::from(operation as i32),
                caller.key.0 as i64,
                key(a),
                key(b),
                key(c),
            ]);
            return Err(SolidError::sm_err("difftest::notify_callback"));
        }
        Ok(())
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
struct TwoRegions {
    primary: Brep,
    other: Brep,
}
impl solidcore::topology::shell::RegionLookup for TwoRegions {
    fn resolve_region(
        &self,
        object: ObjectKey,
    ) -> solidcore::Result<solidcore::topology::shell::RegionRef<'_>> {
        let (brep, index) = match object.0 {
            1 => (&self.primary, 0),
            2 => (&self.other, 1),
            _ => {
                return Err(SolidError::invalid_input(
                    "difftest::region_lookup",
                    "unknown region object",
                ))
            }
        };
        Ok(solidcore::topology::shell::RegionRef {
            brep,
            brep_id: solidcore::topology::BrepId::from_index(index),
            region_id: RegionId::from_index(0),
        })
    }
}
fn append_events(context: &Context, out: &mut Vec<f64>) -> solidcore::Result<()> {
    let recorder = context
        .user_notify_callback()
        .and_then(NotifyCallback::as_any)
        .and_then(|a| a.downcast_ref::<Recorder>())
        .ok_or_else(|| SolidError::null_pointer("difftest::recorder"))?;
    out.push(recorder.events.len() as f64);
    out.extend(recorder.events.iter().flatten().map(|&v| v as f64));
    Ok(())
}
fn run_rust(i: &Inputs, b: &mut Brep, other: &mut Brep) -> Result<Vec<f64>, SolidError> {
    let [a, c, d] = i.args;
    let mut k = Kernel::default();
    let mut out = Vec::new();
    let mut context = Context::default();
    context.set_user_notify_callback(Some(Box::new(Recorder::default())));
    match i.op {
        1 => {
            let mut r = port::CleanupRelations::default();
            if c != 0 {
                r.edges = b.edges().ids().collect();
                r.vertices = b.vertices().ids().collect();
            }
            if c == 1 {
                for (x, y) in b.edges().ids().zip(other.edges().ids()) {
                    r.mates.edges.record(x, y)?;
                }
                for (x, y) in b.vertices().ids().zip(other.vertices().ids()) {
                    r.mates.vertices.record(x, y)?;
                }
            }
            if c == 2 && a == 0 {
                r.edges.push(EdgeId::from_index(u32::MAX));
                r.vertices.push(VertexId::from_index(u32::MAX));
            }
            port::boolean_post_process(&mut k, b, &r, a != 0)?;
        }
        2 => {
            let maps: BTreeMap<_, _> = if c != 0 {
                b.edges()
                    .ids()
                    .zip(other.edges().ids())
                    .enumerate()
                    .filter(|(j, _)| c != 2 || *j != 0)
                    .map(|(_, v)| v)
                    .collect()
            } else {
                BTreeMap::new()
            };
            let f = FaceId::from_index(u32::from(a) % b.faces().len() as u32);
            let g = FaceId::from_index(u32::from(d) % other.faces().len() as u32);
            // `SmMerge(context)`'s intersector: `SM_EFF_ZERO_SQ` and 30 degrees
            let mut merge = KernelManifoldIntersector::new(0)?;
            merge.set_intersection_tolerances(EFF_ZERO_SQ, 30.0 * (PI / 180.0));
            for (&x, &y) in &maps {
                merge.merge.intersector.edges.record(x, y)?;
            }
            let (x, y, changed) = merge.fix_face_coincidence(
                &mut BooleanKernels {
                    primary: &mut k,
                    other: None,
                },
                b,
                other,
                &mut AttributeStore::new(),
                f,
                g,
            )?;
            out.extend(face_coincidence_record(
                changed,
                &x,
                &y,
                b,
                other,
                &merge.merge.intersector.edges,
            ));
        }
        3 => {
            let source = (a != 0)
                .then(|| VertexId::from_index(u32::from(d) % other.vertices().len() as u32));
            let dest = VertexId::from_index(u32::from(d) % b.vertices().len() as u32);
            let mut relations = Relations::default();
            if c != 0 {
                if let Some(s) = source {
                    relations.vertices.record(dest, s)?;
                }
            }
            if let Some([pt, ot]) = i.tolerances {
                solidcore::topology::vertex::set_tolerance(
                    b,
                    dest,
                    solidcore::geom::tol::zone_tol3d(&k, pt),
                    false,
                    true,
                )?;
                if let Some(src) = source {
                    solidcore::topology::vertex::set_tolerance(
                        other,
                        src,
                        solidcore::geom::tol::zone_tol3d(&k, ot),
                        false,
                        true,
                    )?;
                }
            }
            let mut attrs = AttributeStore::new();
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut src = Topology::new();
            src.set_all_flags(16);
            let mut dst = Topology::new();
            dst.set_all_flags(1);
            records.insert(ObjectKey(1), src);
            records.insert(ObjectKey(2), dst);
            // a copy's new vertex: its own (default) record, as an upstream object's
            let fresh = 300 + b.vertices().len() as u64;
            records.insert(ObjectKey(fresh), Topology::new());
            let vertex_key = |v: VertexId| {
                if v == dest {
                    2
                } else {
                    300 + u64::from(v.index())
                }
            };
            let h = attrs.insert(Attribute::long(555, 29, Behavior::Copy))?;
            attrs.add_attribute(ObjectKey(1), h)?;
            let result = port::merge_vertex(
                b,
                other,
                source,
                (a != 2).then_some(RegionId::from_index(0)),
                &mut relations,
                &mut port::VertexCopy {
                    records: &mut records,
                    attributes: &mut attrs,
                    source_key: &|_| obj(1, 16021),
                    destination_key: &|e| match e {
                        solidcore::EntityRef::Vertex(v) => obj(vertex_key(v), 16021),
                        solidcore::EntityRef::Vertexuse(v) => {
                            obj(100 + u64::from(v.index()), 16006)
                        }
                        _ => obj(200, 16001),
                    },
                },
                Some(&mut context),
            )?;
            // the shim's `fresh.find(9, result)` of a walk after the op
            out.push(result.map_or(-1.0, |v| Canon::of(b).vertices.of(Some(v)) as f64));
            if let Some(v) = result {
                let key = ObjectKey(vertex_key(v));
                out.push(
                    records
                        .get(&key)
                        .ok_or_else(|| SolidError::null_pointer("difftest::merge_copy"))?
                        .all_flags() as f64,
                );
                out.push(
                    attrs
                        .find_attribute(key, 555)
                        .and_then(|h| attrs.get(h))
                        .and_then(Attribute::long_value)
                        .unwrap_or(-1) as f64,
                );
            }
            // the shim's recorder knows the source, the destination and the two B-reps
            // only (`-99` for any other object)
            let start = out.len();
            append_events(&context, &mut out)?;
            if let Some(events) = out.get_mut(start + 1..) {
                for (j, x) in events.iter_mut().enumerate() {
                    if j % 5 != 0 && *x >= 100.0 {
                        *x = -99.0;
                    }
                }
            }
        }
        4 => {
            let mut attrs = AttributeStore::new();
            let r = obj(1, 16012);
            let h = attrs.insert(Attribute::long(555, 29, Behavior::Copy))?;
            attrs.add_attribute(r.key, h)?;
            if c != 0 {
                let h = attrs.insert(Attribute::color(
                    ids::COLOR,
                    Vec3::new(0.2, 0.4, 0.6),
                    Behavior::Copy,
                ))?;
                attrs.add_attribute(r.key, h)?;
            }
            if d != 0 {
                let h = attrs.insert(Attribute::merge_region(r.key, 0, ObjectKey(2), 1))?;
                attrs.add_attribute(r.key, h)?;
            }
            // Object 1 is the primary's first region (with the case's void flag), object 2
            // the other operand's first region, as the shim wires them.
            let mut primary = b.clone();
            primary
                .region_mut(RegionId::from_index(0))?
                .set_is_void(i32::from(a != 0));
            let lookup = TwoRegions {
                primary,
                other: other.clone(),
            };
            port::propagate_other_src_region_attribs(
                &[port::RegionPropagation {
                    region: r,
                    is_void: a != 0,
                    source_lists: [obj(5, 0), obj(6, 0)],
                }],
                &mut attrs,
                Some(&mut context),
                BooleanOperation::Union,
                Some(&lookup),
            )?;
            for id in [555, ids::COLOR, ids::OTHER_REGION_ID] {
                out.push(f64::from(u8::from(
                    attrs.find_attribute(r.key, id).is_some(),
                )));
            }
            append_events(&context, &mut out)?;
        }
        OP_FIND_REGION => {
            let f = FaceuseId::from_index(u32::from(a) % other.faceuses().len() as u32);
            out.push(
                port::find_brep_region_of_other_face(
                    &k,
                    b,
                    other,
                    f,
                    &if c != 0 {
                        b.edges()
                            .ids()
                            .zip(other.edges().ids())
                            .map(|(a, b)| (b, a))
                            .collect()
                    } else {
                        BTreeMap::new()
                    },
                )?
                .map_or(-1.0, |v| f64::from(v.index())),
            );
        }
        6 => {
            let e = EdgeId::from_index(u32::from(a) % b.edges().len() as u32);
            out.push(port::edge_borders_solid(b, e)?.map_or(-1.0, |v| f64::from(v.index())));
        }
        7 => {
            let mut other = other.clone();
            // The native probe constructs and immediately destroys its shift.
            let shift = port::temporary_brep_transform(&k, &k, b, &other)?;
            // SmBrep::Transform, both operands in the one kernel
            let mut transform =
                |brep: &mut Brep, _: bool, xfm: &_, scale: Option<_>, scale_tol: bool| {
                    solidcore::topology::brep::p5::transform(&mut k, brep, xfm, scale, scale_tol)
                };
            shift.apply(b, &mut other, &mut transform)?.restore(
                Some(b),
                Some(&mut other),
                &mut transform,
            )?;
        }
        8 | 9 => {
            let sec = EdgeuseId::from_index(u32::from(a) % b.edgeuses().len() as u32);
            let cls = EdgeuseId::from_index(u32::from(c) % other.edgeuses().len() as u32);
            let edge = b
                .edgeuse(sec)?
                .edge
                .ok_or_else(|| SolidError::null_pointer("difftest::radial"))?;
            if i.op == 8 {
                out.push(f64::from(
                    solidcore::query::edge::radial_sector_classify_between(
                        &k,
                        b,
                        edge,
                        i.parameter,
                        sec,
                        Some(&mut *other),
                        cls,
                    )?
                    .code(),
                ));
            } else {
                let r = solidcore::topology::edge::find_radial_sector_between(
                    &k,
                    b,
                    edge,
                    Some(&mut *other),
                    cls,
                    None,
                    solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same,
                    None,
                )?;
                out.extend([id(r.radial_edgeuse) as f64, id(r.coincident_faceuse) as f64]);
            }
        }
        _ => return Err(SolidError::invalid_input("difftest", "bad operation")),
    }
    Ok(out)
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.merge_p1"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (path, block) = block(&i)?;
        let (other_path, other_block) = other_block(&i)?;
        let v = ffi::eval(ffi::Request {
            op: i.op,
            args: i.args,
            primary: ffi::Operand {
                path: &path,
                block: &block,
            },
            other: ffi::Operand {
                path: &other_path,
                block: &other_block,
            },
            tolerances: i.tolerances,
            parameter: i.parameter,
        })
        .map_err(|e| harness(e.to_string()))?;
        if i.op == 0 {
            return Ok(Output::new().values("record", v));
        }
        let [status, count, ..] = v.as_slice() else {
            return Err(harness("short merge result"));
        };
        if *status != 0.0 {
            return Ok(Output::err(format!("status_{}", *status as i64)));
        }
        let end = (*count as usize)
            .checked_add(2)
            .ok_or_else(|| harness("record count overflow"))?;
        let raw = v.get(end..).ok_or_else(|| harness("short graph"))?;
        let (mut b, _) = import_raw(raw)?;
        // the face loop lists and loop edge lists the export does not carry, derived
        // from the exported uses as the Rust B-rep holds them (the Rust side builds its
        // input itself, so its faces and loops have them; as `topology.brep_p5`)
        b.refresh_cached_topology();
        let mut record = v
            .get(2..end)
            .ok_or_else(|| harness("short record"))?
            .to_vec();
        let other = if i.op == OP_FACE_COINCIDENCE {
            Some(split_other_graph(&mut record)?)
        } else {
            None
        };
        Ok(with_other_graph(graph(&b, record), other.as_ref()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        if i.op == 0 {
            return Ok(Output::new().values("record", control(i.args)));
        }
        // both B-reps built by the Rust kernel as the shim's fixture `build` makes them
        // (no C++ export reaches the Rust side)
        let mut k = Kernel::default();
        let primary = i.brep.as_ref().ok_or_else(|| harness("B-rep op without brep"))?;
        let mut b = match own_operand(&mut k, primary)? {
            Ok(b) => b,
            Err(o) => return Ok(o),
        };
        let other_src = i.other.as_ref().unwrap_or(primary);
        let mut other = match own_operand(&mut k, other_src)? {
            Ok(b) => b,
            Err(o) => return Ok(o),
        };
        // the FFI's selection guard: the selectors are taken modulo these counts
        let counts = crate::own_builder::counts;
        if !ffi::selection_valid(i.op, &counts(&b), &counts(&other)) {
            return Err(harness("fixture lacks selected entity"));
        }
        match run_rust(&i, &mut b, &mut other) {
            Ok(v) => Ok(with_other_graph(
                graph(&b, v),
                (i.op == OP_FACE_COINCIDENCE).then_some(&other),
            )),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| {
                Output::err(format!(
                    "status_{}",
                    match e {
                        SolidError::Upstream { code, .. } => code,
                        _ => -1,
                    }
                ))
            })),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.below(3) == 0 {
            return Some(json!({"op":0,"args":[rng.below(32),rng.below(2),rng.below(32)]}));
        }
        let wire = rng.below(2) == 0;
        let op = if wire {
            *[1, 3, 4, 6, 7].get(rng.below(5) as usize)?
        } else {
            *[1, 2, 4, 5, 6, 7].get(rng.below(6) as usize)?
        };
        // Include both sides of the default 3D zone tolerance as well as tiny/huge geometry.
        let scale = *[1e-6, 0.999999e-5, 1e-5, 1.000001e-5, 1.0, 1e6].get(rng.below(6) as usize)?;
        let p = rng.point3(-10.0 * scale, 10.0 * scale);
        let brep = if wire {
            json!({"kind":"wire","curve":{"dim":3,"cpts":[[p[0],p[1],p[2],1.0],[p[0]+scale,p[1]+scale,p[2],1.0]],"p":1,"u":[0,0,1,1]},"t0":0.125,"t1":0.875})
        } else {
            json!({"kind":"face","surface":{"plane":{"o":p,"n":[0,0,1]}},"dom":[-2,-1,3,4]})
        };
        let mut case = json!({"op":op,"args":[rng.below(2),rng.below(3),rng.below(2)],"brep":brep,
          "tolerances":[*[1e-5,1e-3,1e-1].get(rng.below(3) as usize)?,*[1e-5,1e-3,1e-1].get(rng.below(3) as usize)?]});
        if op == 2 && !wire && rng.below(3) != 0 {
            // The same plane with another rectangle (the face's is [-2, -1, 3, 4]): the edges
            // do not mate, so `MergeCoincidentSurfaces` runs on rectangles that overlap, are
            // contained, enclose it, touch it, share edges with it, or do not meet it (the
            // merge then stops at its disjoint-boxes return). The touching and disjoint
            // ones compare the B-reps; the others end in an error status that C++ and Rust
            // agree on (`both_errored`; see plan/findings.md, boolean.merge_p1).
            let doms: [[f64; 4]; 8] = [
                [0.0, 0.0, 5.0, 5.0],
                [0.0, 0.0, 1.0, 1.0],
                [-4.0, -3.0, 6.0, 7.0],
                [3.0, 4.0, 6.0, 8.0],
                [-2.0, -1.0, 3.0, 1.5],
                [10.0, 10.0, 12.0, 12.0],
                [-20.0, -20.0, -18.0, -18.0],
                [6.0, 0.0, 9.0, 3.0],
            ];
            let dom = *doms.get(rng.below(doms.len() as u64) as usize)?;
            case.as_object_mut()?.insert(
                "other".to_owned(),
                json!({"kind":"face","surface":{"plane":{"o":p,"n":[0,0,1]}},"dom":dom}),
            );
        }
        Some(case)
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            root: ctx.root.clone(),
            seeds: ctx.seeds.max(96),
        };
        crate::runner::default_cases(self, &ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::CaseSource;
    use solidcore::topology::{Vertexuse, VertexuseOwner};

    /// A B-rep with one shell vertex in its infinite region.
    fn shell_vertex() -> (Brep, VertexId) {
        let mut b = Brep::new_with_infinite_region().unwrap();
        let inf = b.infinite_region();
        let (_, v) =
            solidcore::topology::brep::p2::make_shell_vertex_topology(&mut b, inf).unwrap();
        (b, v)
    }

    /// `graph` compares what the C++ export walk emits: a record the walk does not reach
    /// (one upstream deleted, which the arena keeps unlinked) leaves the compared output
    /// unchanged and is only counted as a diagnostic; an extra vertex use that is still
    /// linked is reached, so it changes the output.
    #[test]
    fn graph_compares_only_reachable_records() {
        let (b, _) = shell_vertex();
        let base = graph(&b, vec![]);
        assert!(base.diagnostics.is_none());

        let (mut unlinked, _) = shell_vertex();
        unlinked.add_vertexuse_record(Vertexuse::default()).unwrap();
        let g = graph(&unlinked, vec![]);
        assert_eq!((&g.values, &g.ints), (&base.values, &base.ints));
        assert_eq!(g.diagnostic_value("unreached_records"), Some(1));

        let (mut linked, v) = shell_vertex();
        let extra = linked
            .add_vertexuse_record(Vertexuse {
                vertex: Some(v),
                owner: VertexuseOwner::Unknown,
                ..Vertexuse::default()
            })
            .unwrap();
        linked.vertex_mut(v).unwrap().vertexuses.push(extra);
        let g = graph(&linked, vec![]);
        assert_ne!(g.ints, base.ints);
        assert!(g.diagnostics.is_none());
    }

    /// Fuzz 30000 (fuzz03460): op 2 on a wire, which has no face to select. The FFI
    /// rejects the fixture; the Rust side must refuse it too instead of taking the
    /// selector modulo zero faces.
    #[test]
    fn op_without_selected_entity_is_refused_not_a_panic() {
        let case = Case {
            unit: "boolean.merge_p1".into(),
            name: "fuzz03460".into(),
            source: CaseSource::Inline,
            inputs: json!({"args":[2,1,0],"brep":{"curve":{"cpts":[[0,0,0,1],[4,0,0,1]],
                "dim":3,"p":1,"u":[0,0,1,1]},"kind":"wire","t0":0.125,"t1":0.875},
                "op":2,"tolerances":[0.001,1.0]}),
        };
        let e = Runner.run_rust(&case).unwrap_err();
        assert!(e.to_string().contains("lacks selected entity"), "{e}");
    }
}
