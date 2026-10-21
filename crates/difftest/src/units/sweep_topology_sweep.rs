//! Stateful Rust/C++ parity for topology sweep maps, the stitch callback, base
//! geometry dispatch and vertex helpers. Every script compares all four maps,
//! including null entries, reverse-map leftovers and both B-rep namespaces.
//! Vertex construction is deferred at the exact missing B-rep constructor;
//! whole-B-rep geometry construction and the tails behind those calls are not
//! claimed as compared coverage.
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
use solidcore::sweep::topology_sweep::access::{EdgeRef, FaceRef, SweepAccess};
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
            &dyn Fn(FaceId) -> solidcore::geom::object::ObjectRef,
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
        body(&mut self.k, src, dst, &face_objects, &mut env)
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
    fn delete_merge(&mut self, _id: solidcore::sweep::topology_sweep::access::SweepMergeId) {}
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
fn rust(i: &Input) -> Result<Output> {
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
    use solidcore::sweep::topology_sweep::access::{FaceInput, SweepMergeId};
    use solidcore::topology::shell::{ShellMembers, ShellType};
    use solidcore::topology::{Edgeuse, Face, Faceuse, Shell, ShellId};

    fn assert_unported<T>(result: Result<T>, expected_unit: &str) {
        let error = result.err().expect("expected an unavailable dependency");
        let actual_unit = if let SolidError::Unported { unit, .. } = &error {
            Some(*unit)
        } else {
            None
        };
        assert_eq!(actual_unit, Some(expected_unit), "{error:?}");
    }

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
    fn copying_defaults_reach_the_next_unavailable_callees() -> Result<()> {
        let mut model = Model::new([1., 0., 0.])?;
        let source = BrepId::from_index(0);
        let destination = BrepId::from_index(1);
        assert!(model.copy_edges(source, &[], destination)?.is_empty());
        // The live shell-vertex constructor now reaches wire-edge topology creation.
        let copied = model.copy_edges(source, &[EdgeId::from_index(0)], destination);
        assert!(
            matches!(
                copied,
                Err(SolidError::Unported {
                    upstream: "SmBrep::MakeWireEdgeVertex",
                    unit: "topology.brep_p3",
                })
            ),
            "{copied:?}"
        );
        let mut mates = BTreeMap::new();
        assert_unported(
            model.copy_faces(source, &[], destination, true, SweepMergeId(0), &mut mates),
            "boolean.merge_p2",
        );
        assert!(mates.is_empty());
        assert!(matches!(
            model.copy_faces(source, &[], source, true, SweepMergeId(0), &mut mates),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            model.copy_faces(
                source,
                &[],
                BrepId::from_index(99),
                true,
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
            Err(SolidError::Unported {
                upstream: "SmFace::FindVertexSector",
                unit: "topology.face_p4"
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
        assert!(matches!(
            model.hcr(b, Axis2Placement::default(), 0.001, 0.1, false, false, true),
            Err(SolidError::Unported {
                upstream: "SmBrep::SmBrep(const SmBrep&)",
                unit: "topology.brep_p3"
            })
        ));
        model.b[0].set_editing_enabled(false);
        model.k.context.set_doing_boolean(false);
        assert_unported(
            model.stitch(b, &mut TopologySweep::default(), 0.125, false),
            "query.brep",
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
