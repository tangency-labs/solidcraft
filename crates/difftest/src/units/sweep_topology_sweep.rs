//! Stateful Rust/C++ parity for topology sweep maps, the stitch callback, base
//! geometry dispatch and vertex helpers. Every script compares all four maps,
//! including null entries, reverse-map leftovers and both B-rep namespaces.
//! Vertex construction is deferred at the exact missing B-rep constructor;
//! whole-B-rep geometry construction and the tails behind those calls are not
//! claimed as compared coverage by the map scripts. `whole` cases instead run the
//! whole-B-rep `DoSweep` branch (all six target arrays null) on a real box in the
//! production `SweepStore` and compare the swept B-rep (see [`whole_output`]).
#![allow(clippy::indexing_slicing)]
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::geom::{
    attribute::{ids, Attribute, AttributeStore, AttributeValue, Behavior, ObjectKey, TagData},
    context::MarkType,
};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::sweep::topology_sweep::access::{SweepMergeId,EdgeRef, FaceRef, SweepAccess};
use solidcore::sweep::topology_sweep::geometry::SweepGeometry;
use solidcore::sweep::topology_sweep::{SweepEntity as E, SweepGeometryCreation, TopologySweep};
use solidcore::sweep::{
    rotational_sweep_geometry::RotationalSweepGeometry,
    trans_sweep_geometry::TranslationalSweepGeometry,
};
use solidcore::topology::{Brep, BrepId, EdgeId, FaceId, Region, RegionId, Vertex, VertexId};
use solidcore::{Result, SolidError};
use std::collections::BTreeMap;

/// `sweep.topology_sweep` runner.
pub struct Runner;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Input {
    steps: Vec<[i64; 4]>,
    #[serde(default)]
    kind: u8,
    #[serde(default = "vector")]
    vector: [f64; 3],
    /// Whole-B-rep `DoSweep` of a box instead of a map script
    /// (`solidcore_ffi::units::sweep_topology_sweep::whole` input layout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    whole: Option<[f64; 11]>,
    /// Whole-B-rep `DoSweep` of a non-manifold body instead of a map script
    /// (`solidcore_ffi::units::sweep_topology_sweep::body` input layout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<[f64; 16]>,
    /// Explicit `DoSweep` target lists for `body` (its `targets` is 2):
    /// `solidcore_ffi::units::sweep_topology_sweep::body_lists` layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lists: Option<[f64; 7]>,
}
fn vector() -> [f64; 3] {
    [1., 0., 0.]
}
fn entity(id: i64) -> Option<E> {
    if id == 64 {
        return None;
    }
    let b = BrepId::from_index(((id % 16) / 8) as u32);
    let i = (id % 8) as u32;
    Some(match id / 16 {
        0 => E::Vertex(b, VertexId::from_index(i)),
        1 => E::Edge(b, EdgeId::from_index(i)),
        2 => E::Face(b, FaceId::from_index(i)),
        _ => E::Region(b, RegionId::from_index(i)),
    })
}
fn entity_id(e: Option<E>) -> i64 {
    match e {
        None => 64,
        Some(E::Vertex(b, i)) if b.index() < 2 && i.index() < 8 => {
            i64::from(b.index() * 8 + i.index())
        }
        Some(E::Edge(b, i)) if b.index() < 2 && i.index() < 8 => {
            16 + i64::from(b.index() * 8 + i.index())
        }
        Some(E::Face(b, i)) if b.index() < 2 && i.index() < 8 => {
            32 + i64::from(b.index() * 8 + i.index())
        }
        Some(E::Region(b, i)) if b.index() < 2 && i.index() < 8 => {
            48 + i64::from(b.index() * 8 + i.index())
        }
        Some(_) => -2,
    }
}
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::Upstream { code, .. } => i64::from(*code),
        SolidError::InvalidInput { .. } => 1007,
        SolidError::Nlib { .. } => 1001,
        _ => -1,
    }
}
fn unsupported<T>() -> Result<T> {
    Err(SolidError::invalid_input(
        "runner",
        "fixture has no service for this path",
    ))
}
struct Model {
    k: Kernel,
    b: Vec<Brep>,
    records: BTreeMap<ObjectKey, solidcore::topology::topology::Topology>,
    attributes: AttributeStore,
    keys: BTreeMap<E, ObjectKey>,
    curve_keys: BTreeMap<EdgeRef, ObjectKey>,
}
impl Model {
    fn new(vector: [f64; 3]) -> Result<Self> {
        use solidcore::error::EntityRef as R;
        let mut b = vec![Brep::new(), Brep::new()];
        for brep in &mut b {
            let region = brep.add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })?;
            brep.set_infinite_region(Some(region));
            for i in 0..8 {
                brep.add_vertex_record(Vertex {
                    point: Point3::new(
                        vector[0] * f64::from(i),
                        vector[1] * f64::from(i),
                        vector[2] * f64::from(i),
                    ),
                    ..Vertex::default()
                })?;
            }
        }
        let mut attributes = AttributeStore::new();
        let mut keys = BTreeMap::new();
        let mut curve_keys = BTreeMap::new();
        for id in 0..64 {
            let entity = entity(id).ok_or_else(|| SolidError::null_pointer("runner"))?;
            let (brep, local) = match entity {
                E::Vertex(b, v) => (b, R::Vertex(v)),
                E::Edge(b, e) => (b, R::Edge(e)),
                E::Face(b, f) => (b, R::Face(f)),
                E::Region(b, r) => (b, R::Region(r)),
            };
            keys.insert(entity, service_object(brep, local).key);
        }
        for (bid, brep) in b.iter_mut().enumerate() {
            for index in 0..8 {
                let bid = BrepId::from_index(bid as u32);
                let id = 16 + i64::from(bid.index()) * 8 + index;
                let edge = brep.add_edge_record(solidcore::topology::Edge {
                    vertices: [VertexId::from_index(0); 2],
                    edgeuses: Vec::new(),
                    interval: solidcore::geom::extent1d::Extent1d::from_min_max(0., 1.),
                    tolerance: 0.,
                    curve: Some(solidcore::curve::curve::Curve::from(
                        solidcore::curve::line::Line::create_line_segment(
                            3,
                            Point3::ZERO,
                            Point3::X,
                            None,
                        )?,
                    )),
                })?;
                let key = ObjectKey(40_000 + id as u64 * 59);
                curve_keys.insert((bid, edge), key);
                if id % 2 != 0 {
                    let h = attributes.insert(Attribute::tag(
                        ids::TAG,
                        TagData {
                            primary_id: 700 + id,
                            primary_ordinal: 12 + id,
                            ..TagData::default()
                        },
                        Behavior::Copy,
                    ))?;
                    attributes.add_attribute(key, h)?;
                }
            }
        }
        Ok(Self {
            k: Kernel::default(),
            b,
            records: BTreeMap::new(),
            attributes,
            keys,
            curve_keys,
        })
    }
}
fn service_object(
    brep: BrepId,
    entity: solidcore::error::EntityRef,
) -> solidcore::geom::object::ObjectRef {
    use solidcore::error::EntityRef as R;
    use solidcore::geom::assert_array::labels;
    let (type_code, index) = match entity {
        R::Brep(id) => (labels::BREP_TYPE, id.index()),
        R::Region(id) => (labels::REGION_TYPE, id.index()),
        R::Shell(id) => (labels::SHELL_TYPE, id.index()),
        R::Face(id) => (labels::FACE_TYPE, id.index()),
        R::Loop(id) => (labels::LOOP_TYPE, id.index()),
        R::Edge(id) => (labels::EDGE_TYPE, id.index()),
        R::Vertex(id) => (labels::VERTEX_TYPE, id.index()),
        R::Faceuse(id) => (labels::FACEUSE_TYPE, id.index()),
        R::Loopuse(id) => (labels::LOOPUSE_TYPE, id.index()),
        R::Edgeuse(id) => (labels::EDGEUSE_TYPE, id.index()),
        R::Vertexuse(id) => (labels::VERTEXUSE_TYPE, id.index()),
    };
    solidcore::geom::object::ObjectRef {
        type_code,
        key: ObjectKey(
            1_000_000
                + u64::from(brep.index()) * 100_000
                + (type_code as u64) * 1000
                + u64::from(index),
        ),
    }
}
impl SweepAccess for Model {
    fn brep(&self, id: BrepId) -> Result<&Brep> {
        self.b
            .get(id.index() as usize)
            .ok_or_else(|| SolidError::null_pointer("runner"))
    }
    fn brep_mut(&mut self, id: BrepId) -> Result<&mut Brep> {
        self.b
            .get_mut(id.index() as usize)
            .ok_or_else(|| SolidError::null_pointer("runner"))
    }
    fn kernel(&self, _id: BrepId) -> Result<&Kernel> {
        Ok(&self.k)
    }
    fn kernel_mut(&mut self, _id: BrepId) -> Result<&mut Kernel> {
        Ok(&mut self.k)
    }
    fn with_brep_mut<T>(
        &mut self,
        id: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &dyn Fn(FaceId) -> solidcore::geom::object::ObjectRef,
            &mut solidcore::topology::edge::NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let brep = self
            .b
            .get_mut(id.index() as usize)
            .ok_or_else(|| SolidError::null_pointer("sweep test model"))?;
        let face_objects =
            |face: FaceId| service_object(id, solidcore::error::EntityRef::Face(face));
        let key_of = |entity| service_object(id, entity);
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut self.records,
            attributes: &mut self.attributes,
            context: None,
            key_of: &key_of,
        };
        body(&mut self.k, brep, &face_objects, &mut env)
    }
    fn with_breps_mut<T>(
        &mut self,
        source: BrepId,
        destination: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(solidcore::EntityRef) -> solidcore::geom::object::ObjectRef,
            &dyn Fn(FaceId) -> solidcore::geom::object::ObjectRef,
            Option<&mut solidcore::geom::context::Context>,
            &mut solidcore::topology::edge::NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let si = source.index() as usize;
        let di = destination.index() as usize;
        if si == di {
            return Err(SolidError::invalid_input(
                "sweep test model",
                "aliased B-reps",
            ));
        }
        let (left, right) = self
            .b
            .split_at_mut_checked(si.max(di))
            .ok_or_else(|| SolidError::null_pointer("sweep test model"))?;
        let (src, dst) = if si < di {
            (left.get_mut(si), right.first_mut())
        } else {
            (right.first_mut(), left.get_mut(di))
        };
        let src = src.ok_or_else(|| SolidError::null_pointer("sweep test model"))?;
        let dst = dst.ok_or_else(|| SolidError::null_pointer("sweep test model"))?;
        let face_objects =
            |face: FaceId| service_object(destination, solidcore::error::EntityRef::Face(face));
        let key_of = |entity| service_object(destination, entity);
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut self.records,
            attributes: &mut self.attributes,
            context: None,
            key_of: &key_of,
        };
        body(&mut self.k, src, dst, &|e| service_object(source, e), &face_objects, None, &mut env)
    }
    fn with_brep_notifications<T>(
        &mut self,
        id: BrepId,
        body: impl FnOnce(&mut Brep, &mut solidcore::topology::edge::NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        let brep = self
            .b
            .get_mut(id.index() as usize)
            .ok_or_else(|| SolidError::null_pointer("sweep test model"))?;
        let key_of = |entity| service_object(id, entity);
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut self.records,
            attributes: &mut self.attributes,
            context: Some(&mut self.k.context),
            key_of: &key_of,
        };
        body(brep, &mut env)
    }
    fn retain_hcr(&mut self, _: BrepId, _: solidcore::blend::hcr::Hcr) -> Result<BrepId> {
        Err(SolidError::invalid_input(
            "sweep test model",
            "fixture has no retained HCR service",
        ))
    }
    fn hcr_session(&self, _: BrepId) -> Result<&solidcore::blend::hcr::Hcr> {
        Err(SolidError::invalid_input(
            "sweep test model",
            "fixture has no retained HCR service",
        ))
    }
    fn new_brep(&mut self, _id: BrepId) -> Result<BrepId> {
        unsupported()
    }
    fn new_merge(&mut self, _: BrepId, _: BrepId, _: BrepId, _: f64, _: f64) -> Result<SweepMergeId> { Err(SolidError::invalid_input("sweep fixture", "no merge engine")) }
    fn merge_operands(&self, _: SweepMergeId) -> Result<(BrepId, BrepId)> { Err(SolidError::invalid_input("sweep fixture", "no merge engine")) }
    fn merge_relations(&mut self, _: SweepMergeId) -> Result<&mut solidcore::boolean::topology_intersector::p1::Relations> { Err(SolidError::invalid_input("sweep fixture", "no merge engine")) }
    fn piecewise_merge(&mut self, _: SweepMergeId, _: bool, _: bool) -> Result<BrepId> { Err(SolidError::invalid_input("sweep fixture", "no merge engine")) }
    fn union(&mut self, _: SweepMergeId) -> Result<BrepId> { Err(SolidError::invalid_input("sweep fixture", "no merge engine")) }
    fn copy_faces(&mut self, source: BrepId, _: &[FaceId], destination: BrepId, _: SweepMergeId, _: &mut solidcore::sweep::topology_sweep::access::BTreeEdgeMap) -> Result<()> {
        self.brep(source)?; self.brep(destination)?;
        Err(SolidError::invalid_input("fixture", "no merge engine"))
    }
    fn delete_merge(&mut self, _id: SweepMergeId) {}
    fn delete_brep(&mut self, _id: BrepId) {}
    fn tag(&self, e: E) -> Result<Option<TagData>> {
        let key = self
            .keys
            .get(&e)
            .ok_or_else(|| SolidError::null_pointer("runner"))?;
        Ok(self
            .attributes
            .find_attribute(*key, ids::TAG)
            .and_then(|h| self.attributes.get(h))
            .and_then(|a| {
                if let AttributeValue::Tag(t) = a.value() {
                    Some(*t)
                } else {
                    None
                }
            }))
    }
    fn add_tag(&mut self, e: E, tag: TagData) -> Result<()> {
        let key = *self
            .keys
            .get(&e)
            .ok_or_else(|| SolidError::null_pointer("runner"))?;
        let h = self
            .attributes
            .insert(Attribute::tag(ids::TAG, tag, Behavior::Copy))?;
        self.attributes.add_attribute(key, h)
    }
    fn curve_tag(&self, edge: EdgeRef) -> Result<Option<TagData>> {
        let key = self
            .curve_keys
            .get(&edge)
            .ok_or_else(|| SolidError::null_pointer("runner"))?;
        Ok(self
            .attributes
            .find_attribute(*key, ids::TAG)
            .and_then(|h| self.attributes.get(h))
            .and_then(|a| {
                if let AttributeValue::Tag(t) = a.value() {
                    Some(*t)
                } else {
                    None
                }
            }))
    }
    fn set_editing(&mut self, _id: BrepId, _editing: bool) -> Result<bool> {
        unsupported()
    }
    fn lock_mark(&mut self, _id: BrepId, _mask: i32) -> Result<MarkType> {
        unsupported()
    }
    fn unlock_mark(&mut self, _id: BrepId, _mark: MarkType) -> Result<()> {
        unsupported()
    }
    fn mark(&mut self, _e: E, _mark: MarkType) -> Result<()> {
        unsupported()
    }
    fn is_marked(&self, _e: E, _mark: MarkType) -> Result<bool> {
        unsupported()
    }
    fn collect_faces(
        &mut self,
        _f: FaceRef,
        _mark: MarkType,
        _out: &mut Vec<FaceId>,
    ) -> Result<()> {
        unsupported()
    }
}
/// Distinct identities for the whole-B-rep store: kind and index under the B-rep
/// handle, as a native object has one address per topology object.
struct WholeIdentities;
impl solidcore::sweep::topology_sweep::store::SweepIdentities for WholeIdentities {
    fn topology(
        &self,
        b: BrepId,
        e: solidcore::error::EntityRef,
    ) -> solidcore::geom::object::ObjectRef {
        use solidcore::error::EntityRef as R;
        // (kind, index, native type code)
        let (kind, index, type_code) = match e {
            R::Brep(i) => (0_u64, i.index(), 16011),
            R::Shell(i) => (1, i.index(), 16013),
            R::Face(i) => (2, i.index(), 16002),
            R::Loop(i) => (3, i.index(), 16003),
            R::Edge(i) => (4, i.index(), 16016),
            R::Vertex(i) => (5, i.index(), 16021),
            R::Region(i) => (6, i.index(), 16012),
            R::Faceuse(i) => (7, i.index(), 16022),
            R::Loopuse(i) => (8, i.index(), 16004),
            R::Edgeuse(i) => (9, i.index(), 16005),
            R::Vertexuse(i) => (10, i.index(), 16006),
        };
        solidcore::geom::object::ObjectRef {
            type_code,
            key: ObjectKey((u64::from(b.index()) << 40) | (kind << 32) | u64::from(index)),
        }
    }
    fn curve(
        &self,
        b: BrepId,
        e: EdgeId,
        _: &solidcore::curve::curve::Curve,
    ) -> solidcore::geom::object::ObjectRef {
        solidcore::geom::object::ObjectRef {
            type_code: 11001,
            key: ObjectKey((u64::from(b.index()) << 40) | (20 << 32) | u64::from(e.index())),
        }
    }
    /// A curve the stitch allocates when it splits an edge (`MakeVertexSplitEdge`'s
    /// curve copy) is named, as every edge curve here, by its edge: the native copy
    /// is a new object of its own, distinct from every other curve.
    fn new_curve(
        &self,
        b: BrepId,
        e: EdgeId,
        type_code: i64,
    ) -> Result<solidcore::geom::object::ObjectRef> {
        Ok(solidcore::geom::object::ObjectRef {
            type_code,
            key: ObjectKey((u64::from(b.index()) << 40) | (20 << 32) | u64::from(e.index())),
        })
    }
    fn owned_auxiliary_objects(&self, _: BrepId, _: &Brep) -> Vec<ObjectKey> {
        Vec::new()
    }
    /// The merge's two source lists of a primary region: handles outside every
    /// topology and curve kind above.
    fn source_lists(
        &self,
        b: BrepId,
        r: RegionId,
    ) -> Result<[solidcore::geom::object::ObjectRef; 2]> {
        Ok([21_u64, 22].map(|kind| solidcore::geom::object::ObjectRef {
            type_code: 0,
            key: ObjectKey((u64::from(b.index()) << 40) | (kind << 32) | u64::from(r.index())),
        }))
    }
}

/// The whole-B-rep record from the native layout, in a canonical order. Upstream
/// takes the silhouette edges from a pointer hash (`SmHCR::GetSilhouetteEdges`,
/// `plan/upstream-issues` ND-2), so the creation order of the swept topology is not
/// reproducible; the record therefore compares, in `whole`, the status, the six
/// counts and the number of edges without two vertices, and in `points` the vertex
/// points sorted, then each edge's two end points (lesser first) sorted.
fn whole_output(record: &[f64]) -> Output {
    let Some((&status, rest)) = record.split_first() else {
        return Output::err("empty whole-B-rep record".to_string());
    };
    let mut ints = vec![status as i64];
    let mut values = Vec::new();
    if let Some(counts) = rest.get(..6) {
        ints.extend(counts.iter().map(|&c| c as i64));
        let n = counts[5] as usize;
        let tail = &rest[6..];
        let vertices: Vec<[f64; 3]> = tail
            .get(..3 * n)
            .unwrap_or_default()
            .as_chunks::<3>()
            .0
            .to_vec();
        // The canonical order sorts on coordinates quantized to 1e-7 of the largest
        // coordinate, then exactly: rounding noise (a coordinate of 10 computed as
        // 10 + 2e-15 on one side) must not reorder two records whose values agree.
        // Only the order uses the quantization; the values are compared unchanged.
        let scale = vertices
            .iter()
            .flatten()
            .fold(0.0_f64, |m, x| m.max(x.abs()))
            .max(f64::MIN_POSITIVE)
            * 1e-7;
        let cmp = |a: &[f64; 3], b: &[f64; 3]| {
            let quantized = a
                .iter()
                .zip(b)
                .map(|(x, y)| (x / scale).round().total_cmp(&(y / scale).round()))
                .find(|o| o.is_ne());
            quantized.unwrap_or_else(|| {
                a.iter()
                    .zip(b)
                    .map(|(x, y)| x.total_cmp(y))
                    .find(|o| o.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        };
        let mut sorted = vertices.clone();
        sorted.sort_by(cmp);
        let mut open = 0;
        let mut edges = Vec::new();
        for pair in tail.get(3 * n..).unwrap_or_default().as_chunks::<2>().0 {
            let end = |i: f64| (i >= 0.).then(|| vertices.get(i as usize).copied()).flatten();
            match (end(pair[0]), end(pair[1])) {
                (Some(p), Some(q)) => edges.push(if cmp(&p, &q).is_le() { [p, q] } else { [q, p] }),
                _ => open += 1,
            }
        }
        edges.sort_by(|a, b| cmp(&a[0], &b[0]).then_with(|| cmp(&a[1], &b[1])));
        ints.push(open);
        values.extend(sorted.into_iter().flatten());
        values.extend(edges.into_iter().flatten().flatten());
    }
    Output::new().ints("whole", ints).values("points", values)
}

/// Rust side of `sc_sweep_topology_sweep_whole`: the same box (`create_box`) in a
/// [`SweepStore`] with distinct caller identities, swept with all targets null.
fn whole_rust(input: [f64; 11]) -> Result<Output> {
    use solidcore::sweep::topology_sweep::store::{SweepIdentities, SweepStore};
    use solidcore::topology::brep::p1;
    let [length, width, height, kind, vx, vy, vz, angle, stitch, merge, reps] = input;
    let mut k = Kernel::default();
    let mut record = Vec::new();
    let mut b = match solidcore::primitive::api_primitives::create_box(
        &mut k,
        Point3::ZERO,
        length,
        width,
        height,
    ) {
        Ok(b) => b,
        Err(e @ SolidError::Unported { .. }) => return Err(e),
        Err(e) => return Ok(whole_output(&[status(&e) as f64])),
    };
    let v = Point3::new(vx, vy, vz);
    let g = if kind == 1. {
        SweepGeometry::Translation(TranslationalSweepGeometry::new(v))
    } else {
        SweepGeometry::Rotation(RotationalSweepGeometry::new(
            Point3::ZERO,
            v,
            angle,
            solidcore::geom::tol::ZONE_TOL_3D,
        ))
    };
    let mut records = BTreeMap::new();
    let mut attributes = AttributeStore::new();
    let mut next = 0;
    let mut store = SweepStore::new(&WholeIdentities, &mut records, &mut attributes, &mut next);
    let context = store.register_kernel(&mut k)?;
    let handle = BrepId::from_index(0);
    b.set_key(WholeIdentities.topology(handle, solidcore::error::EntityRef::Brep(handle)).key);
    let id = store.register_brep(context, &mut b)?;
    let region = store.infinite_region(id)?;
    let mut sweep = TopologySweep {
        do_stitching: stitch != 0.,
        do_merge: merge != 0.,
        repetitions: reps as u64,
        ..TopologySweep::default()
    };
    let result = sweep.do_sweep(
        &mut store,
        &g,
        Some(id),
        Some((id, region)),
        &mut solidcore::sweep::topology_sweep::engine::SweepTargets::default(),
    );
    drop(store);
    match result {
        Err(e @ SolidError::Unported { .. }) => return Err(e),
        Err(e) => record.push(status(&e) as f64),
        Ok(()) => {
            record.push(0.);
            let vertices = p1::vertices(&b, None)?;
            let edges = p1::edges(&b, None)?;
            let faces = p1::faces(&b, None)?;
            // SmBrep::GetLoops: the loops of every face, in face order.
            let mut loops = 0;
            for &f in &faces {
                loops += solidcore::topology::face::p2::loops(&b, f, false, None)?.len();
            }
            for n in [
                p1::regions(&b, None)?.len(),
                p1::shells(&mut k, &b, None)?.len(),
                faces.len(),
                loops,
                edges.len(),
                vertices.len(),
            ] {
                record.push(n as f64);
            }
            for &v in &vertices {
                let p = solidcore::topology::vertex::point(&b, v)?;
                record.extend([p.x, p.y, p.z]);
            }
            let index = |v: Option<VertexId>| {
                v.and_then(|v| vertices.iter().position(|&w| w == v))
                    .map_or(-1., |i| i as f64)
            };
            for e in edges {
                record.push(index(solidcore::topology::edge::start_vertex(&b, e)?));
                record.push(index(solidcore::topology::edge::end_vertex(&b, e)?));
            }
        }
    }
    Ok(whole_output(&record))
}

/// The non-manifold body record: [`whole_output`] of its prefix, then the wire-edge
/// and shell-vertex counts in `body` and the face and edge tags, each as a sorted
/// multiset (creation order is not reproducible, see [`whole_output`]), in `tags`.
fn body_output(record: &[f64]) -> Output {
    let counts = record.get(1..7);
    let Some(counts) = counts else {
        return whole_output(record);
    };
    let (faces, edges, vertices) = (counts[2] as usize, counts[4] as usize, counts[5] as usize);
    let prefix = 7 + 3 * vertices + 2 * edges;
    let Some(head) = record.get(..prefix) else {
        return Output::err("short non-manifold body record".to_string());
    };
    let rest = &record[prefix..];
    let mut output = whole_output(head);
    let extra: Vec<i64> = rest.iter().take(2).map(|&c| c as i64).collect();
    let tags = rest.get(2..).unwrap_or_default();
    let mut tag_ints = Vec::new();
    for (n, from) in [(faces, 0), (edges, 9 * faces)] {
        let mut list: Vec<Vec<i64>> = tags
            .get(from..from + 9 * n)
            .unwrap_or_default()
            .chunks(9)
            .map(|c| c.iter().map(|&x| x as i64).collect())
            .collect();
        list.sort();
        tag_ints.push(n as i64);
        tag_ints.extend(list.into_iter().flatten());
    }
    output = output.ints("body", extra).ints("tags", tag_ints);
    output
}

/// A B-spline line segment as an edge curve (`SmBSplineCurve::CreateLineSegment`, 3D).
fn bspline_line(a: [f64; 3], b: [f64; 3]) -> Result<solidcore::curve::curve::Curve> {
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    Ok(BSplineCurve::create_line_segment(3, Point3::new(a[0], a[1], a[2]), Point3::new(b[0], b[1], b[2]))?.into())
}

/// The shim's `sc_test_tsurf_creation` (`prog_test` `my_test_tsurf_creation`): a 10x10
/// B-spline plane face with a notched outer loop and two inner loops.
fn test_tsurf_creation(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut solidcore::topology::edge::NotifyEnv<'_>,
) -> Result<()> {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::nurbs::b_spline_curve::{p1::NurbCircleParam, BSplineCurve};
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::brep::p7::{self as brep_p7, FaceCurvesInput};
    solidcore::topology::brep::p2::set_tolerance(k, b, 0.0001, true, true)?;
    let plane = BSplineSurface::create_bilinear_surface(
        Point3::new(0., 0., 0.),
        Point3::new(10., 0., 0.),
        Point3::new(0., 10., 0.),
        Point3::new(10., 10., 0.),
    )?;
    let surface = plane.copy_and_add_analytics(k)?;
    let (same, opposite) = (OrientType::Same, OrientType::Opposite);
    let segments = [
        ([0., 0., 0.], [10., 0., 0.], same),
        ([10., 5., 0.], [10., 0., 0.], opposite),
        ([10., 5., 0.], [5., 5., 0.], same),
        ([5., 5., 0.], [5., 10., 0.], same),
        ([5., 10., 0.], [0., 10., 0.], same),
        ([0., 10., 0.], [0., 0., 0.], same),
        ([1., 5., 0.], [1., 8., 0.], same),
        ([1., 8., 0.], [4., 5., 0.], same),
        ([4., 5., 0.], [1., 5., 0.], same),
    ];
    let mut curves = Vec::new();
    let mut orientations = Vec::new();
    for (a, z, o) in segments {
        curves.push(Some(bspline_line(a, z)?));
        orientations.push(o);
    }
    let mut frame = Axis2Placement::IDENTITY;
    frame.set_canonical(
        Point3::new(6., 2.5, 0.),
        Point3::new(-1., 0., 0.),
        Point3::new(0., -1., 0.),
    )?;
    curves.push(Some(
        BSplineCurve::create_circle_segment(3, &frame, 1.5, 0.0, 360.0, NurbCircleParam::Quadratic)?
            .into(),
    ));
    orientations.push(opposite);
    let input = FaceCurvesInput {
        loops: vec![6, 3, 1],
        curves_3d: Some(curves),
        uv_curves: None,
        orientations,
        loop_points: Vec::new(),
    };
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let region = b.infinite_region();
    let key_of = env.key_of;
    let this_face = |face| key_of(solidcore::error::EntityRef::Face(face));
    brep_p7::make_face_with_curves(k, b, region, input, surface, &domain, same, None, &this_face, env)?;
    Ok(())
}

/// The shim's `sc_create_test_brep` (`prog_test` `my_create_test_brep`), with the
/// context's doing-Boolean flag set while it adds the vertex loop, the edge loop,
/// the wire edge and the shell vertex.
fn create_test_brep(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut solidcore::topology::edge::NotifyEnv<'_>,
) -> Result<()> {
    test_tsurf_creation(k, b, env)?;
    let old = k.context.doing_boolean();
    k.context.set_doing_boolean(true);
    let result = (|| {
        use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use solidcore::topology::brep::{p1, p3, p5, p9};
        let vertices = p1::vertices(b, None)?;
        let faces = p1::faces(b, None)?;
        let (Some(&face), Some(&v0), Some(&v1), Some(&v3)) =
            (faces.first(), vertices.first(), vertices.get(1), vertices.get(3))
        else {
            return Err(SolidError::invalid_input("sweep fixture", "test face"));
        };
        p9::make_vertex_loop(b, face, Point3::new(3.5, 8.5, 0.), OrientType::Opposite, env)?;
        let line = bspline_line([0., 0., 0.], [5., 5., 0.])?;
        let interval = line.natural_interval();
        b.set_editing_enabled(true);
        let mut made = p5::EdgeInFace::default();
        p5::make_edge_in_face(
            k,
            b,
            true,
            Some(face),
            Some(v0),
            Some(v3),
            line,
            None,
            interval,
            OrientType::Same,
            solidcore::geom::tol::ZONE_TOL_3D,
            &mut made,
            Some(&mut *env),
        )?;
        let wire = bspline_line([10., 0., 0.], [15., 2., 0.])?;
        let interval = wire.natural_interval();
        let region = b.infinite_region();
        p3::make_wire_edge_vertex(k, b, env, region, v1, wire, &interval, OrientType::Same, Point3::new(15., 2., 0.))?;
        let region = b.infinite_region();
        p9::make_shell_vertex(b, region, Point3::new(-5., 0., 0.), env)?;
        Ok(())
    })();
    k.context.set_doing_boolean(old);
    result
}

/// The shim's `sc_add_wire`: a shell vertex at (10,0,0) extended by a wire edge to
/// (15,2,0).
fn add_wire(k: &mut Kernel, b: &mut Brep, env: &mut solidcore::topology::edge::NotifyEnv<'_>) -> Result<()> {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::brep::{p3, p9};
    let region = b.infinite_region();
    let (_, start) = p9::make_shell_vertex(b, region, Point3::new(10., 0., 0.), env)?;
    let wire = bspline_line([10., 0., 0.], [15., 2., 0.])?;
    let interval = wire.natural_interval();
    let region = b.infinite_region();
    p3::make_wire_edge_vertex(k, b, env, region, start, wire, &interval, OrientType::Same, Point3::new(15., 2., 0.))?;
    Ok(())
}

/// The shim's `sc_build_body`.
fn build_body(
    k: &mut Kernel,
    body: f64,
    key: ObjectKey,
    env: &mut solidcore::topology::edge::NotifyEnv<'_>,
) -> Result<Brep> {
    use solidcore::topology::brep::p9;
    let mut b = if body == 0. || body == 5. {
        solidcore::primitive::api_primitives::create_box(k, Point3::ZERO, 4., 3., 2.)?
    } else {
        Brep::new_with_infinite_region()?
    };
    b.set_key(key);
    if body == 0. {
        add_wire(k, &mut b, env)?;
        let region = b.infinite_region();
        p9::make_shell_vertex(&mut b, region, Point3::new(-5., 0., 0.), env)?;
    } else if body == 1. {
        create_test_brep(k, &mut b, env)?;
    } else if body == 6. {
        test_tsurf_creation(k, &mut b, env)?;
    } else if body == 3. {
        let region = b.infinite_region();
        p9::make_shell_vertex(&mut b, region, Point3::new(-5., 0., 0.), env)?;
    } else if body == 4. {
        add_wire(k, &mut b, env)?;
    }
    Ok(b)
}

/// One explicit target list from its selector (the shim's `pick`): `None` for -1
/// (NULL), else every entity of `all` (0), `wire` (1), none (2), every entity but the
/// last (3) or the single entity `all[sel - 10]`.
fn pick_list<T: Copy>(all: &[T], wire: &[T], sel: f64) -> Result<Option<Vec<T>>> {
    Ok(Some(match sel {
        s if s < 0. => return Ok(None),
        0. => all.to_vec(),
        1. => wire.to_vec(),
        2. => Vec::new(),
        3. => all.get(..all.len().saturating_sub(1)).unwrap_or_default().to_vec(),
        s => vec![*all
            .get((s - 10.) as usize)
            .ok_or_else(|| SolidError::invalid_input("runner", "target index past the list"))?],
    }))
}

/// Rust side of `sc_sweep_topology_sweep_body`: the same body built by the same
/// kernel calls in a [`SweepStore`], swept whole (all targets null), with every face
/// as faces-higher, or with the explicit `lists`.
fn body_rust(input: [f64; 16], lists: Option<[f64; 7]>) -> Result<Output> {
    use solidcore::sweep::topology_sweep::store::{SweepIdentities, SweepStore};
    use solidcore::topology::brep::p1;
    let [body, kind, vx, vy, vz, angle, cx, cy, cz, eps, stitch, merge, reps, tagging, tag_id, targets] =
        input;
    let mut k = Kernel::default();
    let mut records = BTreeMap::new();
    let mut attributes = AttributeStore::new();
    let handle = BrepId::from_index(0);
    let key_of = |e: solidcore::error::EntityRef| WholeIdentities.topology(handle, e);
    let built = {
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key_of,
        };
        build_body(&mut k, body, key_of(solidcore::error::EntityRef::Brep(handle)).key, &mut env)
    };
    let mut b = match built {
        Ok(b) => b,
        Err(e @ SolidError::Unported { .. }) => return Err(e),
        Err(e) => return Ok(body_output(&[status(&e) as f64])),
    };
    let v = Point3::new(vx, vy, vz);
    let g = if kind == 0. {
        SweepGeometry::Base(SweepGeometryCreation { eps, ..SweepGeometryCreation::default() })
    } else if kind == 1. {
        SweepGeometry::Translation(TranslationalSweepGeometry::new(v))
    } else {
        SweepGeometry::Rotation(RotationalSweepGeometry::new(Point3::new(cx, cy, cz), v, angle, eps))
    };
    let mut next = 0;
    let mut store = SweepStore::new(&WholeIdentities, &mut records, &mut attributes, &mut next);
    let context = store.register_kernel(&mut k)?;
    let id = store.register_brep(context, &mut b)?;
    let region = store.infinite_region(id)?;
    let mut sweep = TopologySweep {
        do_stitching: stitch != 0.,
        do_merge: merge != 0.,
        do_tagging: tagging != 0.,
        tag_id: tag_id as i64,
        repetitions: reps as u64,
        ..TopologySweep::default()
    };
    let mut sweep_targets = solidcore::sweep::topology_sweep::engine::SweepTargets::default();
    if targets == 1. {
        let faces = p1::faces(store.brep(id)?, None)?;
        sweep_targets.faces_higher = Some(faces.into_iter().map(|f| Some((id, f))).collect());
    }
    let mut destination = Some((id, region));
    if let Some(lists) = lists {
        let brep = store.brep(id)?;
        let faces = p1::faces(brep, None)?;
        let edges = p1::edges(brep, None)?;
        let wires = p1::wire_edges(brep, None)?;
        let vertices = p1::vertices(brep, None)?;
        let shell_vertices = solidcore::topology::brep::p2::shell_vertices(brep, None)?;
        sweep_targets.faces_higher = pick_list(&faces, &faces, lists[0])?.map(|l| l.into_iter().map(|x| Some((id, x))).collect());
        sweep_targets.edges_higher = pick_list(&edges, &wires, lists[1])?.map(|l| l.into_iter().map(|x| Some((id, x))).collect());
        sweep_targets.vertices_higher =
            pick_list(&vertices, &shell_vertices, lists[2])?.map(|l| l.into_iter().map(|x| Some((id, x))).collect());
        sweep_targets.faces_same = pick_list(&faces, &faces, lists[3])?.map(|l| l.into_iter().map(|x| Some((id, x))).collect());
        sweep_targets.edges_same = pick_list(&edges, &wires, lists[4])?.map(|l| l.into_iter().map(|x| Some((id, x))).collect());
        sweep_targets.vertices_same =
            pick_list(&vertices, &shell_vertices, lists[5])?.map(|l| l.into_iter().map(|x| Some((id, x))).collect());
        if lists[6] == 1. {
            destination = None;
        }
    }
    let result = sweep.do_sweep(&mut store, &g, Some(id), destination, &mut sweep_targets);
    let mut tags = Vec::new();
    if result.is_ok() {
        let tag = |t: Option<TagData>| {
            let mut block = vec![f64::from(u8::from(t.is_some()))];
            block.extend(t.unwrap_or_default().to_array().map(|x| x as f64));
            block
        };
        let faces = p1::faces(store.brep(id)?, None)?;
        let edges = p1::edges(store.brep(id)?, None)?;
        for face in faces {
            tags.extend(tag(store.tag(E::Face(id, face))?));
        }
        for edge in edges {
            tags.extend(tag(store.tag(E::Edge(id, edge))?));
        }
    }
    drop(store);
    let mut record = Vec::new();
    match result {
        Err(e @ SolidError::Unported { .. }) => return Err(e),
        Err(e) => record.push(status(&e) as f64),
        Ok(()) => {
            record.push(0.);
            let vertices = p1::vertices(&b, None)?;
            let edges = p1::edges(&b, None)?;
            let faces = p1::faces(&b, None)?;
            let mut loops = 0;
            for &f in &faces {
                loops += solidcore::topology::face::p2::loops(&b, f, false, None)?.len();
            }
            for n in [
                p1::regions(&b, None)?.len(),
                p1::shells(&mut k, &b, None)?.len(),
                faces.len(),
                loops,
                edges.len(),
                vertices.len(),
            ] {
                record.push(n as f64);
            }
            for &v in &vertices {
                let p = solidcore::topology::vertex::point(&b, v)?;
                record.extend([p.x, p.y, p.z]);
            }
            let index = |v: Option<VertexId>| {
                v.and_then(|v| vertices.iter().position(|&w| w == v))
                    .map_or(-1., |i| i as f64)
            };
            for e in edges {
                record.push(index(solidcore::topology::edge::start_vertex(&b, e)?));
                record.push(index(solidcore::topology::edge::end_vertex(&b, e)?));
            }
            record.push(p1::wire_edges(&b, None)?.len() as f64);
            record.push(solidcore::topology::brep::p2::shell_vertices(&b, None)?.len() as f64);
            record.extend(tags);
        }
    }
    Ok(body_output(&record))
}

fn rust(i: &Input) -> Result<Output> {
    if let Some(whole) = i.whole {
        return whole_rust(whole);
    }
    if let Some(body) = i.body {
        return body_rust(body, i.lists);
    }
    let mut s = TopologySweep::default();
    let mut out = Vec::new();
    let mut values = Vec::new();
    let mut model = Model::new(i.vector)?;
    let g = match i.kind {
        0 => SweepGeometry::Base(SweepGeometryCreation::default()),
        1 => SweepGeometry::Translation(TranslationalSweepGeometry::new(Point3::new(
            i.vector[0],
            i.vector[1],
            i.vector[2],
        ))),
        _ => SweepGeometry::Rotation(RotationalSweepGeometry::new(
            Point3::ZERO,
            Point3::Z,
            90.,
            1e-5,
        )),
    };
    for &[op, ai, bi, arg] in &i.steps {
        let a = entity(ai);
        let b = entity(bi);
        let mut keep = true;
        let mut distance = -123.;
        let mut frame = [0.0; 12];
        let result = match op {
            0 => {
                s.set_same(a, b);
                Ok(())
            }
            1 => {
                s.set_higher(a, b);
                Ok(())
            }
            2 => s.select_topology_to_keep(a, b, i.vector[0], &mut distance, &mut keep),
            3 => {
                s.remove_from_to_same(a);
                Ok(())
            }
            4 => {
                s.remove_from_from_same(a);
                Ok(())
            }
            5 => {
                let map = match arg {
                    0 => &mut s.to_same,
                    1 => &mut s.to_higher,
                    2 => &mut s.from_same,
                    _ => &mut s.from_lower,
                };
                map.insert(a, b);
                Ok(())
            }
            6 => {
                s.repetitions = arg as u64;
                Ok(())
            }
            7 => {
                // Both native defaults and the rotational override clear this
                // output before returning an error for too many turns.
                keep = false;
                g.is_sweep_closed(arg as u64).map(|v| keep = v)
            }
            8 | 9 => {
                let result = if op == 8 {
                    g.start_coord_system(Point3::ZERO)
                } else {
                    g.sweep_transform()
                };
                result.map(|placement| {
                    let v = [
                        placement.origin(),
                        placement.x_axis(),
                        placement.y_axis(),
                        placement.z_axis(),
                    ];
                    for (out, p) in frame.as_chunks_mut::<3>().0.iter_mut().zip(v) {
                        out.copy_from_slice(&[p.x, p.y, p.z]);
                    }
                })
            }
            10 | 11 => {
                let Some(E::Vertex(b, v)) = a else {
                    return unsupported();
                };
                let region = (BrepId::from_index(0), RegionId::from_index(0));
                if op == 10 {
                    s.sweep_vertices_same(&mut model, &g, region, &[Some((b, v))])
                } else {
                    s.sweep_vertices_higher(&mut model, &g, region, &[Some((b, v))])
                }
            }
            15 => {
                s.do_stitching = false;
                s.do_merge = false;
                s.repetitions = 1;
                // Match the native fixture's distinct original shell vertex;
                // it is not one of the 64 map-script pool objects.
                let tolerance = model.b[0].tolerance();
                let source = model.b[0].add_vertex_record(Vertex {
                    point: Point3::ZERO,
                    tolerance,
                    ..Vertex::default()
                })?;
                s.do_sweep(
                    &mut model,
                    &g,
                    Some(BrepId::from_index(0)),
                    Some((BrepId::from_index(0), RegionId::from_index(0))),
                    &mut solidcore::sweep::topology_sweep::engine::SweepTargets {
                        vertices_higher: Some(vec![Some((BrepId::from_index(0), source))]),
                        ..Default::default()
                    },
                )
            }
            14 => {
                let Some(E::Edge(ba, ia)) = a else {
                    return unsupported();
                };
                let Some(E::Edge(bb, ib)) = b else {
                    return unsupported();
                };
                s.tag_profile(
                    &mut model,
                    &solidcore::sweep::topology_sweep::engine::SweepTargets {
                        edges_higher: Some(vec![Some((ba, ia)), Some((bb, ib))]),
                        ..Default::default()
                    },
                )
            }
            13 => s.do_sweep(
                &mut model,
                &g,
                None,
                None,
                &mut solidcore::sweep::topology_sweep::engine::SweepTargets::default(),
            ),
            _ => {
                s.do_tagging = arg % 2 != 0;
                s.tag_id = arg;
                s.do_merge = arg % 2 != 0;
                s.do_stitching = (arg / 2) % 2 != 0;
                Ok(())
            }
        };
        let status = match result {
            Ok(()) => 0,
            Err(err @ SolidError::Unported { .. }) => return Err(err),
            Err(err) => status(&err),
        };
        out.extend([status, i64::from(keep), distance as i64]);
        values.push(distance);
        values.extend(frame);
    }
    out.extend([
        i64::from(s.do_tagging),
        s.tag_id,
        s.repetitions as i64,
        i64::from(s.do_merge),
        i64::from(s.do_stitching),
    ]);
    for map in [&s.to_same, &s.to_higher, &s.from_same, &s.from_lower] {
        out.push(map.len() as i64);
        for id in 0..=64 {
            let e = entity(id);
            out.extend([
                i64::from(map.contains_key(&e)),
                entity_id(map.get(&e).copied().flatten()),
            ]);
        }
    }
    for id in 16..32 {
        let tag = model.tag(entity(id).ok_or_else(|| SolidError::null_pointer("runner"))?)?;
        out.push(i64::from(tag.is_some()));
        out.extend(tag.unwrap_or_default().to_array());
    }
    Ok(Output::new().ints("state", out).values("geometry", values))
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "sweep.topology_sweep"
    }
    fn run_cpp(&self, c: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Input = c.parse()?;
        if let Some(body) = i.body {
            // An input whose C++ path is undefined (UB-269, UB-270) is refused by the
            // FFI guard before the call; Rust is not run on it.
            if let Some(reason) = solidcore_ffi::units::sweep_topology_sweep::body_undefined(&body) {
                return Ok(Output::rejected(reason));
            }
            // Fresh process per case, as for `whole` below.
            return solidcore_ffi::isolate::fresh_process(|| match i.lists {
                Some(lists) => solidcore_ffi::units::sweep_topology_sweep::body_lists(body, lists),
                None => solidcore_ffi::units::sweep_topology_sweep::body(body),
            })
            .map(|record| body_output(&record))
            .map_err(|e| HarnessError::new(e.to_string()));
        }
        if let Some(whole) = i.whole {
            // Each whole-B-rep case runs in a fresh process: native stitching follows
            // heap layout (the pointer-hash silhouette order), so a case's C++ result
            // must not depend on the cases run before it in the same process.
            return solidcore_ffi::isolate::fresh_process(|| {
                solidcore_ffi::units::sweep_topology_sweep::whole(whole)
            })
            .map(|record| whole_output(&record))
                .map_err(|e| HarnessError::new(e.to_string()));
        }
        solidcore_ffi::units::sweep_topology_sweep::eval_with_values(&i.steps, i.vector, i.kind)
            .map(|(state, values)| {
                Output::new()
                    .ints("state", state)
                    .values("geometry", values)
            })
            .map_err(|e| HarnessError::new(e.to_string()))
    }
    fn run_rust(&self, c: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Input = c.parse()?;
        Ok(match rust(&i) {
            Ok(output) => output,
            Err(e) => Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())),
        })
    }
    fn cases(&self, ctx: &CaseContext) -> std::result::Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(512);
        crate::runner::default_cases(self, &ctx)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let mut steps = Vec::new();
        for _ in 0..(4 + rng.below(24)) {
            let op = rng.below(10);
            let a = rng.below(65);
            let b = rng.below(65);
            let arg = if op == 5 {
                rng.below(4)
            } else {
                rng.below(1000)
            };
            steps.push([op as i64, a as i64, b as i64, arg as i64]);
        }
        if rng.below(3) == 0 {
            steps.push([14, 16 + rng.below(16) as i64, 16 + rng.below(16) as i64, 0]);
        }
        if rng.below(4) == 0 {
            steps.push([12, 0, 0, rng.below(1000) as i64]);
        }
        Some(
            json!({"steps":steps,"kind":rng.below(3),"vector":[rng.uniform(-10.,10.),rng.uniform(-10.,10.),rng.uniform(-10.,10.)]}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::surface::{plane::Plane, surface::Surface};
    use solidcore::sweep::topology_sweep::access::{FaceInput};
    use solidcore::topology::shell::{ShellMembers, ShellType};
    use solidcore::topology::{Edgeuse, Face, Faceuse, Shell, ShellId};

    #[test]
    fn default_queries_reach_the_live_lists_and_missing_infinite_region_is_null() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let b = BrepId::from_index(0);
        let inf = model.infinite_region(b)?;
        let f1 = model.b[0].add_face_record(Face::new(ShellId::from_index(0)))?;
        let f2 = model.b[0].add_face_record(Face::new(ShellId::from_index(0)))?;
        model.b[0].set_face_list(vec![f2, f1])?;
        assert_eq!(model.faces(b)?, vec![f2, f1]);
        let fu1 = model.b[0].add_faceuse_record(Faceuse {
            face: Some(f1),
            ..Faceuse::default()
        })?;
        let fu2 = model.b[0].add_faceuse_record(Faceuse {
            face: Some(f2),
            ..Faceuse::default()
        })?;
        let shell = model.b[0].add_shell_record(Shell {
            shell_type: ShellType::Faceuse,
            members: ShellMembers::Faceuses(vec![fu2, fu1, fu2]),
            ..Shell::default()
        })?;
        model.b[0].region_mut(inf)?.shells = vec![shell];
        assert_eq!(model.region_shells((b, inf))?, vec![shell]);
        assert_eq!(model.shell_faceuse_faces((b, shell))?, vec![f2, f1, f2]);
        model.b.push(Brep::new());
        assert!(model.wire_edges(BrepId::from_index(2))?.is_empty());
        model.b[0].set_infinite_region(None);
        assert!(matches!(
            model.infinite_region(b),
            Err(SolidError::Upstream { code: 1006, .. })
        ));
        Ok(())
    }
    #[test]
    fn remove_all_topology_calls_the_live_port_and_preserves_editing_errors() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let brep = BrepId::from_index(0);
        assert!(matches!(
            model.remove_all_topology(brep),
            Err(SolidError::Upstream {
                code: solidcore::error::SM_ERR,
                ..
            })
        ));
        model.b[0].set_editing_enabled(true);
        // No traversable shell topology: the actual empty cleanup now completes.
        model.remove_all_topology(brep)?;
        assert!(model.b[0].editing_enabled());
        assert!(matches!(
            model.remove_all_topology(BrepId::from_index(99)),
            Err(SolidError::Upstream { code: 1006, .. })
        ));
        Ok(())
    }
    #[test]
    fn copying_defaults_reject_incomplete_topology_and_keep_the_merge_boundary() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let source = BrepId::from_index(0);
        let destination = BrepId::from_index(1);
        assert!(model.copy_edges(source, &[], destination)?.is_empty());
        // This fixture edge deliberately has no primary edgeuse. The now-ported
        // constructor reaches its native topology validation rather than deferring.
        let copied = model.copy_edges(source, &[EdgeId::from_index(0)], destination);
        assert!(
            matches!(
                copied,
                Err(SolidError::InvalidInput {
                    operation: "topology::edge",
                    reason: "edge without primary edge use",
                })
            ),
            "{copied:?}"
        );
        let mut mates = BTreeMap::new();
        assert!(matches!(model.copy_faces(source, &[], destination, SweepMergeId(0), &mut mates), Err(SolidError::InvalidInput { .. })));
        assert!(mates.is_empty());
        assert!(matches!(
            model.copy_faces(source, &[], source, SweepMergeId(0), &mut mates),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            model.copy_faces(
                source,
                &[],
                BrepId::from_index(99),
                SweepMergeId(0),
                &mut mates
            ),
            Err(SolidError::Upstream { code: 1006, .. })
        ));
        Ok(())
    }
    #[test]
    fn make_face_default_calls_the_port_and_rejects_an_open_loop() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let surface = Surface::from(Plane::from_normal(Point3::ZERO, Point3::Z));
        let curve = model.b[0]
            .edge(EdgeId::from_index(0))?
            .curve
            .clone()
            .ok_or_else(|| SolidError::null_pointer("test"))?;
        let input = FaceInput {
            region: (BrepId::from_index(0), RegionId::from_index(0)),
            loop_counts: vec![1],
            curves: vec![curve],
            uv_curves: None,
            orientations: vec![OrientType::Same],
            loop_points: Vec::new(),
            domain: Extent2d::new(),
            surface,
            orientation: OrientType::Same,
        };
        assert!(matches!(
            model.make_face(input),
            Err(SolidError::InvalidInput {
                reason: "loop does not close",
                ..
            })
        ));
        Ok(())
    }
    #[test]
    fn new_defaults_reach_real_ports_and_restore_context_on_stitch_error() -> Result<()> {
        use solidcore::geom::axis2_placement::Axis2Placement;
        let mut model = Model::new([1., 0., 0.])?;
        let b = BrepId::from_index(0);
        let region = model.infinite_region(b)?;
        let (shell, vertex) = model.make_shell_vertex((b, region), Point3::ZERO)?;
        assert_eq!(shell.0, b);
        assert_eq!(vertex.0, b);
        assert_eq!(model.b[0].shell(shell.1)?.region, Some(region));
        assert_eq!(model.b[0].vertex(vertex.1)?.point, Point3::ZERO);
        assert_eq!(model.shell_vertices(b)?, vec![vertex.1]);
        assert!(matches!(
            model.shell_vertices(BrepId::from_index(99)),
            Err(SolidError::Upstream { code: 1006, .. })
        ));
        let face = model.b[0].add_face_record(Face::new(ShellId::from_index(0)))?;
        // This bare face has no upward faceuse. The real PRE_EDIT notification
        // runs before MakeVertexLoop reports that missing topology (native NER).
        model.b[0].face_mut(face)?.rectangular_trim = 1;
        let vertex_loop = model.make_vertex_loop((b, face), Point3::ZERO);
        assert!(
            matches!(
                vertex_loop,
                Err(SolidError::Upstream {
                    operation: "topology::brep::p9",
                    code: 1006,
                    ..
                })
            ),
            "{vertex_loop:?}"
        );
        assert_eq!(
            model.b[0].face(face)?.rectangular_trim,
            solidcore::topology::face::p2::UNSURE
        );
        let edge = model.b[0].edge(EdgeId::from_index(0))?;
        let curve = edge
            .curve
            .clone()
            .ok_or_else(|| SolidError::null_pointer("test"))?;
        let interval = edge.interval;
        assert!(matches!(
            model.make_edge_in_face(
                (b, face),
                (BrepId::from_index(1), VertexId::from_index(0)),
                (BrepId::from_index(1), VertexId::from_index(1)),
                curve.clone(),
                None,
                interval,
                OrientType::Same,
                0.375
            ),
            Err(SolidError::Upstream {
                code: solidcore::error::SM_ERR,
                ..
            })
        ));
        model.b[0].set_editing_enabled(true);
        let uv_line = solidcore::curve::curve::Curve::from(
            solidcore::curve::line::Line::create_line_segment(2, Point3::ZERO, Point3::X, None)?,
        );
        assert!(matches!(
            model.make_edge_in_face(
                (b, face),
                (b, VertexId::from_index(0)),
                (b, VertexId::from_index(1)),
                curve,
                Some(uv_line),
                interval,
                OrientType::Same,
                0.375
            ),
            // FindVertexSector (topology.face_p4) finds no sector of a face without
            // face uses, so MakeEdgeInFace's NER(pStartVU) reports a null pointer
            Err(SolidError::Upstream {
                operation: "topology::brep::p5",
                code: 1006,
                ..
            })
        ));
        model.b[0].set_editing_enabled(false);
        assert!(!model.has_lamina_edge((b, face))?);
        assert!(model.remove_faces(b, &[]).is_err());
        model.b[0].set_editing_enabled(true);
        let old_mark = model.k.context.current_mark(MarkType::Mark);
        model.remove_faces(b, &[])?;
        assert_ne!(model.k.context.current_mark(MarkType::Mark), old_mark);
        assert!(!model.k.context.is_mark_locked(MarkType::Mark));
        model.transform(b, Axis2Placement::default())?;
        // HCR's scene transform is wired (topology::brep::p5::transform_with_notifications):
        // the scene is built, then this fixture has no service to retain it in.
        let hcr = model.hcr(b, Axis2Placement::default(), 0.001, 0.1, false, false, true);
        assert!(
            matches!(
                hcr,
                Err(SolidError::InvalidInput {
                    reason: "fixture has no retained HCR service",
                    ..
                })
            ),
            "{hcr:?}"
        );
        model.b[0].set_editing_enabled(false);
        model.k.context.set_doing_boolean(false);
        // query.brep (CalculateBoundingBox) is ported: the stitch now fails later,
        // in the B-rep merge, and still restores the caller's context.
        let stitched = model.stitch(b, &mut TopologySweep::default(), 0.125, false);
        assert!(
            matches!(stitched, Err(SolidError::Upstream { code: 1006, .. })),
            "{stitched:?}"
        );
        assert!(!model.b[0].editing_enabled());
        assert!(!model.k.context.doing_boolean());
        Ok(())
    }

    #[test]
    fn shell_vertex_query_uses_nonzero_owner_and_vertex_list_order() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let first_owner = BrepId::from_index(0);
        let other_owner = BrepId::from_index(1);
        let first_region = model.infinite_region(first_owner)?;
        let other_region = model.infinite_region(other_owner)?;
        let (_, only) = model.make_shell_vertex((first_owner, first_region), Point3::ZERO)?;
        let (_, first) = model.make_shell_vertex((other_owner, other_region), Point3::X)?;
        let (_, second) = model.make_shell_vertex((other_owner, other_region), Point3::Y)?;
        assert_eq!(model.shell_vertices(other_owner)?, vec![first.1, second.1]);
        assert_eq!(model.shell_vertices(first_owner)?, vec![only.1]);
        Ok(())
    }

    #[test]
    fn face_tolerance_uses_owning_kernel_conversion_and_grows_boundaries() -> Result<()> {
        use solidcore::topology::{Loopuse, LoopuseStart, Vertexuse};
        for (input, expected) in [
            (-0.25, 0.25),
            (solidcore::geom::tol::USE_DEFAULT, 0.125),
            (0.03125, 0.03125),
        ] {
            let mut model = Model::new([1., 0., 0.])?;
            model.k.tolerances.zone_tol_3d = 0.125;
            let b = BrepId::from_index(0);
            let brep = &mut model.b[0];
            let edge = EdgeId::from_index(0);
            let v1 = VertexId::from_index(0);
            let v2 = VertexId::from_index(1);
            let vu1 = brep.add_vertexuse_record(Vertexuse {
                vertex: Some(v1),
                ..Vertexuse::default()
            })?;
            let vu2 = brep.add_vertexuse_record(Vertexuse {
                vertex: Some(v2),
                ..Vertexuse::default()
            })?;
            let eu1 = brep.add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                vertexuse: Some(vu1),
                ..Edgeuse::default()
            })?;
            let eu2 = brep.add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                vertexuse: Some(vu2),
                ..Edgeuse::default()
            })?;
            brep.edgeuse_mut(eu1)?.orientation = OrientType::Same;
            brep.edgeuse_mut(eu2)?.orientation = OrientType::Opposite;
            brep.edgeuse_mut(eu1)?.ccw = Some(eu1);
            brep.edge_mut(edge)?.edgeuses = vec![eu1, eu2];
            let lu = brep.add_loopuse_record(Loopuse {
                start: LoopuseStart::Edgeuse(Some(eu1)),
                ..Loopuse::default()
            })?;
            let fu = brep.add_faceuse_record(Faceuse {
                loopuses: vec![lu],
                ..Faceuse::default()
            })?;
            let face = brep.add_face_record(Face {
                faceuse: Some(fu),
                ..Face::new(ShellId::from_index(0))
            })?;
            model.set_face_tolerance((b, face), input)?;
            assert_eq!(model.b[0].face(face)?.tolerance, expected);
            assert_eq!(model.b[0].edge(edge)?.tolerance, expected);
            assert_eq!(model.b[0].vertex(v1)?.tolerance, expected);
            assert_eq!(model.b[0].vertex(v2)?.tolerance, expected);
        }
        Ok(())
    }
    #[test]
    fn binormal_error_keeps_both_caller_outputs() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let eu = model.b[0].add_edgeuse_record(Edgeuse::default())?;
        let mut point = Point3::new(7., 8., 9.);
        let mut normal = Point3::new(3., 2., 1.);
        assert!(matches!(
            model.binormal(
                (BrepId::from_index(0), eu),
                0.5,
                false,
                &mut point,
                &mut normal
            ),
            Err(SolidError::Upstream { code: 1007, .. })
        ));
        assert_eq!(point, Point3::new(7., 8., 9.));
        assert_eq!(normal, Point3::new(3., 2., 1.));
        Ok(())
    }
}
