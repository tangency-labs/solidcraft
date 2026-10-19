//! Wire edges and isolated shell vertices in a boolean merge.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/inc/SmMerge.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`process_wires_and_shell_vertices`] | `SmMerge::ProcessWiresAndShellVertices` |
//! | [`process_wires_and_shell_vertices_with_kernels`] | the same stage with each operand context |
//! | [`Relations`] | shared bidirectional mate lookups from `m_vTI` |
//!
//! Deviations: operands, marks and mate maps are borrowed explicitly instead of
//! reading `SmMerge::m_vTI`; ids replace pointers. Output stays mutable because
//! upstream preserves its partial contents on failure. Classification (including
//! face/edge-to-region resolution) waits for `query.brep`; graph deletion waits
//! for `topology.brep_p4`; edge copying waits for `boolean.merge_p2`. Vertex
//! copying delegates to [`p1::merge_vertex`]. B-rep
//! caches are not attached to arena B-reps yet, so the optional cache resizing is
//! absent. The deletion retry loop is iterative; wire collection uses the bounded
//! explicit stack of `topology_traverser`. No input-dependent recursion is added.
//! Relations are shared mutably with `merge_p1`; notifications use its caller-owned
//! records and attribute store. Newly created vertex relationships go through
//! `Relate` ([`Relations::relate_vertices`]), which may raise the other operand's
//! vertex tolerance, so `other` is mutable.
//! Missing/dangling mates are rejected only when added to the keep list:
//! typed ids cannot represent the null pointer upstream can append there.

use crate::boolean::merge::p1::{self, VertexCopy};
use crate::boolean::topology_intersector::p1::Relations;
use crate::error::{Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::geom::context::{Context, MarkType};
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::topology::topology_traverser::{collect_wire_edges, Entity, Marks};
use crate::topology::{edge, vertex, vertexuse, Brep, EdgeId, RegionId, VertexId};

const OP: &str = "boolean::merge::p5::process_wires_and_shell_vertices";

// Result contract of Point3DClassify followed by the upstream region lookup.
// This is not a new topology record: region data remains owned by the B-rep.
struct LocatedRegion {
    id: RegionId,
    is_void: bool,
}

fn classify_region(
    _k: &Kernel,
    _b: &Brep,
    _point: Point3,
    _zone_tolerance: f64,
    _boundary_intersections: bool,
    _accept_face_or_edge: bool,
) -> Result<Option<LocatedRegion>> {
    // Point3DClassify uses SM_EFF_ZERO, boundary_intersections, and an SmPointClassification
    // initialised with the source object's zone tolerance. Editing is temporarily
    // disabled. Only the primary-wire pass accepts a face or edge and resolves its
    // region through upward faceuse / primary edgeuse -> shell -> region.
    // None means another classification kind: wire passes signal SE(SM_ERR)
    // and continue; shell-vertex passes return SM_ERR via SER(SM_ERR).
    // UNPORTED(query.brep): SmBrep::Point3DClassify
    Err(SolidError::unported(
        "SmBrep::Point3DClassify",
        "query.brep",
    ))
}

fn delete_edge(_brep: &mut Brep, _edge: EdgeId) -> Result<()> {
    // UNPORTED(topology.brep_p4): SmBrep::DeleteEdge
    Err(SolidError::unported(
        "SmBrep::DeleteEdge",
        "topology.brep_p4",
    ))
}

fn delete_vertex(_brep: &mut Brep, _vertex: VertexId) -> Result<()> {
    // UNPORTED(topology.brep_p4): SmBrep::DeleteVertex
    Err(SolidError::unported(
        "SmBrep::DeleteVertex",
        "topology.brep_p4",
    ))
}

fn merge_edge(_brep: &mut Brep, _other: &Brep, _edge: EdgeId, _region: RegionId) -> Result<()> {
    // UNPORTED(boolean.merge_p2): SmMerge::MergeEdge
    Err(SolidError::unported(
        "SmMerge::MergeEdge",
        "boolean.merge_p2",
    ))
}

fn midpoint(b: &Brep, id: EdgeId) -> Result<Point3> {
    let edge = b.edge(id)?;
    let curve = edge
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    curve.evaluate_point(edge.interval.evaluate(0.5))
}

fn removes_primary(operation: BooleanOperation) -> bool {
    matches!(
        operation,
        BooleanOperation::Union
            | BooleanOperation::Intersection
            | BooleanOperation::Difference
            | BooleanOperation::Slice
    )
}

fn remove_in_region(operation: BooleanOperation, is_void: bool) -> bool {
    (matches!(
        operation,
        BooleanOperation::Union | BooleanOperation::Difference | BooleanOperation::Slice
    ) && !is_void)
        || (operation == BooleanOperation::Intersection && is_void)
}

fn keep_other(operation: BooleanOperation, is_void: bool) -> bool {
    match operation {
        BooleanOperation::Union => is_void,
        BooleanOperation::Difference | BooleanOperation::Slice | BooleanOperation::Intersection => {
            !is_void
        }
        BooleanOperation::ExclusiveOr
        | BooleanOperation::Merge
        | BooleanOperation::PartialMerge => true,
        _ => false,
    }
}

/// Remove unwanted primary wires and shell vertices, collect mapped primary
/// edges that must survive face deletion, then copy needed other geometry.
/// The caller supplies the already-established intersection marks (not advanced).
/// `keep_as_wires` is cleared even when the first subsequent operation fails.
/// Missing callees return [`SolidError::Unported`] at the first call reached;
/// malformed topology or missing mates return an error instead of dereferencing
/// a null pointer. The mate maps must refer to the supplied operands. `other` is
/// mutable because relating a kept vertex (`SmTopologyIntersector::Relate`) raises the
/// smaller of the two vertex tolerances, in whichever operand holds it.
pub fn process_wires_and_shell_vertices(
    k: &Kernel,
    brep: &mut Brep,
    other: &mut Brep,
    brep_marks: &mut Marks,
    other_marks: &mut Marks,
    relations: &mut Relations,
    copy: &mut VertexCopy<'_>,
    mut context: Option<&mut Context>,
    operation: BooleanOperation,
    brep_mark: MarkType,
    other_mark: MarkType,
    keep_as_wires: &mut Vec<EdgeId>,
) -> Result<()> {
    let keep_vertices = collect_wires_and_shell_vertices(
        k,
        k,
        brep,
        other,
        brep_marks,
        other_marks,
        relations,
        operation,
        brep_mark,
        other_mark,
        keep_as_wires,
    )?;
    for (vertex, region) in keep_vertices {
        p1::merge_vertex(
            brep,
            other,
            Some(vertex),
            Some(region),
            relations,
            copy,
            context.as_deref_mut(),
        )?;
    }
    Ok(())
}

/// The shared wire stage with live, possibly distinct operand kernels.
/// Marks are read from their owning contexts; copied other vertices notify through
/// the other context (or the shared primary context). Caller records and attributes
/// remain in `copy`; no replacement context or attribute store is constructed.
pub fn process_wires_and_shell_vertices_with_kernels(
    primary_kernel: &mut Kernel,
    other_kernel: Option<&mut Kernel>,
    brep: &mut Brep,
    other: &mut Brep,
    brep_marks: &mut Marks,
    other_marks: &mut Marks,
    relations: &mut Relations,
    copy: &mut VertexCopy<'_>,
    operation: BooleanOperation,
    brep_mark: MarkType,
    other_mark: MarkType,
    keep_as_wires: &mut Vec<EdgeId>,
) -> Result<()> {
    let keep_vertices = collect_wires_and_shell_vertices(
        primary_kernel,
        other_kernel.as_deref().unwrap_or(primary_kernel),
        brep,
        other,
        brep_marks,
        other_marks,
        relations,
        operation,
        brep_mark,
        other_mark,
        keep_as_wires,
    )?;
    let source_context = &mut other_kernel.unwrap_or(primary_kernel).context;
    for (vertex, region) in keep_vertices {
        p1::merge_vertex(
            brep,
            other,
            Some(vertex),
            Some(region),
            relations,
            copy,
            Some(&mut *source_context),
        )?;
    }
    Ok(())
}

fn collect_wires_and_shell_vertices(
    k: &Kernel,
    other_k: &Kernel,
    brep: &mut Brep,
    other: &mut Brep,
    brep_marks: &mut Marks,
    other_marks: &mut Marks,
    relations: &Relations,
    operation: BooleanOperation,
    brep_mark: MarkType,
    other_mark: MarkType,
    keep_as_wires: &mut Vec<EdgeId>,
) -> Result<Vec<(VertexId, RegionId)>> {
    keep_as_wires.clear();
    let mut collected = Vec::new();
    let mut done = !removes_primary(operation);
    while !done {
        done = true;
        let mut edges: Vec<_> = brep.edges().ids().map(Some).collect();
        for i in 0..edges.len() {
            let Some(id) = edges.get_mut(i).and_then(Option::take) else {
                continue;
            };
            if !vertexuse::edge_is_wire(brep, id)?
                || brep_marks.is_marked(k, Entity::Edge(id), brep_mark)
            {
                continue;
            }
            let region = classify_region(
                k,
                other,
                midpoint(brep, id)?,
                brep.edge(id)?.tolerance,
                true,
                true,
            )?;
            let Some(region) = region else { continue };
            collect_wire_edges(
                k,
                brep,
                brep_marks,
                Some(id),
                brep_mark,
                &mut collected,
                None,
            )?;
            if remove_in_region(operation, region.is_void) {
                for &edge_id in &collected {
                    done = false;
                    let v1 =
                        edge::vertex(brep, edge_id)?.ok_or_else(|| SolidError::null_pointer(OP))?;
                    let v2 = edge::other_vertex(brep, edge_id, Some(v1))?
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    if let Some(slot) = edges.iter_mut().find(|slot| **slot == Some(edge_id)) {
                        *slot = None;
                    }
                    delete_edge(brep, edge_id)?;
                    for v in [Some(v1), (v1 != v2).then_some(v2)].into_iter().flatten() {
                        if vertex::is_shell_vertex(brep, v)?
                            && relations.vertices.other(v).is_none()
                        {
                            delete_vertex(brep, v)?;
                        }
                    }
                }
            }
        }
    }

    for id in other.edges().ids() {
        let is_wire = vertexuse::edge_is_wire(other, id)?;
        let marked = other_marks.is_marked(other_k, Entity::Edge(id), other_mark);
        if is_wire && marked {
            match operation {
                BooleanOperation::Union
                | BooleanOperation::Intersection
                | BooleanOperation::Merge
                | BooleanOperation::PartialMerge => {
                    let mate = relations
                        .edges
                        .primary(id)
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    brep.edge(mate)?;
                    keep_as_wires.push(mate);
                }
                // Upstream SE(SM_ERR) records the error but does not return.
                _ => {}
            }
            continue;
        } else if marked || !is_wire {
            continue;
        }
        let region = classify_region(
            k,
            brep,
            midpoint(other, id)?,
            other.edge(id)?.tolerance,
            true,
            false,
        )?;
        let Some(region) = region else { continue };
        collect_wire_edges(
            other_k,
            other,
            other_marks,
            Some(id),
            other_mark,
            &mut collected,
            None,
        )?;
        if keep_other(operation, region.is_void) {
            for &edge in &collected {
                merge_edge(brep, other, edge, region.id)?;
            }
        }
    }

    if removes_primary(operation) {
        let vertices: Vec<_> = brep.vertices().ids().collect();
        for v in vertices {
            if brep_marks.is_marked(k, Entity::Vertex(v), brep_mark)
                || !vertex::is_shell_vertex(brep, v)?
            {
                continue;
            }
            let vertex = brep.vertex(v)?;
            let region = classify_region(k, other, vertex.point, vertex.tolerance, false, false)?
                .ok_or_else(|| SolidError::sm_err(OP))?;
            if remove_in_region(operation, region.is_void) {
                delete_vertex(brep, v)?;
            }
        }
    }

    let mut keep_vertices = Vec::new();
    for v in other.vertices().ids() {
        if other_marks.is_marked(other_k, Entity::Vertex(v), other_mark)
            || !vertex::is_shell_vertex(other, v)?
        {
            continue;
        }
        let vertex = other.vertex(v)?;
        let region = classify_region(k, brep, vertex.point, vertex.tolerance, false, false)?
            .ok_or_else(|| SolidError::sm_err(OP))?;
        if keep_other(operation, region.is_void) {
            keep_vertices.push((v, region.id));
        }
    }
    Ok(keep_vertices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::attribute::{AttributeStore, ObjectKey};
    use crate::geom::object::ObjectRef;
    use crate::topology::topology::Topology;
    use std::collections::BTreeMap;

    use crate::curve::line::Line;
    use crate::geom::context::MarkLock;
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::topology::{Edge, Edgeuse, EdgeuseOwner, Vertexuse, VertexuseOwner};

    fn copy<'a>(
        records: &'a mut BTreeMap<ObjectKey, Topology>,
        attributes: &'a mut AttributeStore,
    ) -> VertexCopy<'a> {
        VertexCopy {
            records,
            attributes,
            source_key: &|v| ObjectRef {
                key: ObjectKey(1_000_000 + u64::from(v.index())),
                type_code: 16021,
            },
            destination_key: &|e| match e {
                crate::EntityRef::Vertex(v) => ObjectRef {
                    key: ObjectKey(u64::from(v.index())),
                    type_code: 16021,
                },
                crate::EntityRef::Vertexuse(v) => ObjectRef {
                    key: ObjectKey(2_000_000 + u64::from(v.index())),
                    type_code: 16006,
                },
                _ => ObjectRef {
                    key: ObjectKey(3_000_000),
                    type_code: 16001,
                },
            },
        }
    }

    fn wire(b: &mut Brep) -> EdgeId {
        let shell = b.add_shell().unwrap();
        let p = Point3::new(0.0, 0.0, 0.0);
        let q = Point3::new(1.0, 0.0, 0.0);
        let v = b.add_vertex(p).unwrap();
        let w = b.add_vertex(q).unwrap();
        let e = b.add_edge_record(Edge::new([v, w])).unwrap();
        for (vertex, orientation) in [(v, OrientType::Same), (w, OrientType::Opposite)] {
            let eu = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(e),
                    owner: EdgeuseOwner::Shell(Some(shell)),
                    orientation,
                    ..Edgeuse::default()
                })
                .unwrap();
            let vu = b
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(vertex),
                    owner: VertexuseOwner::Edgeuse(Some(eu)),
                    ..Vertexuse::default()
                })
                .unwrap();
            b.edgeuse_mut(eu).unwrap().vertexuse = Some(vu);
            b.edge_mut(e).unwrap().edgeuses.push(eu);
            b.vertex_mut(vertex).unwrap().vertexuses.push(vu);
        }
        let edge = b.edge_mut(e).unwrap();
        edge.curve = Some(Line::from_points(p, q, 3).into());
        edge.interval = crate::geom::extent1d::Extent1d::from_raw(0.0, 1.0);
        e
    }

    #[test]
    fn output_resets_and_preserves_mates_before_a_deferred_call() {
        let mut k = Kernel::default();
        let lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, 1).unwrap();
        let mt = lock.mark_type();
        let mut a = Brep::new();
        let ae = wire(&mut a);
        let mut b = Brep::new();
        let be = wire(&mut b);
        wire(&mut b);
        let mut am = Marks::default();
        let mut bm = Marks::default();
        am.mark(&k, Entity::Edge(ae), mt);
        bm.mark(&k, Entity::Edge(be), mt);
        let mut relations = Relations::default();
        relations.edges.record(ae, be).unwrap();
        let mut records = BTreeMap::new();
        let mut attrs = AttributeStore::new();
        let mut keep = vec![EdgeId::from_index(u32::MAX)];
        let result = process_wires_and_shell_vertices(
            &k,
            &mut a,
            &mut b,
            &mut am,
            &mut bm,
            &mut relations,
            &mut copy(&mut records, &mut attrs),
            None,
            BooleanOperation::Union,
            mt,
            mt,
            &mut keep,
        );
        assert!(matches!(
            result,
            Err(SolidError::Unported {
                unit: "query.brep",
                ..
            })
        ));
        assert_eq!(keep, [ae]);
    }

    #[test]
    fn missing_and_extreme_mates_return_errors_without_panicking() {
        let mut k = Kernel::default();
        let lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, 1).unwrap();
        let mt = lock.mark_type();
        let mut a = Brep::new();
        let ae = wire(&mut a);
        let mut b = Brep::new();
        let be = wire(&mut b);
        let mut am = Marks::default();
        let mut bm = Marks::default();
        am.mark(&k, Entity::Edge(ae), mt);
        bm.mark(&k, Entity::Edge(be), mt);
        for edges in [
            BTreeMap::new(),
            BTreeMap::from([(be, EdgeId::from_index(u32::MAX))]),
        ] {
            let mut relations = Relations::default();
            for (&other, &primary) in &edges {
                relations.edges.record(primary, other).unwrap();
            }
            let mut records = BTreeMap::new();
            let mut attrs = AttributeStore::new();
            for operation in [
                BooleanOperation::Union,
                BooleanOperation::Intersection,
                BooleanOperation::Merge,
                BooleanOperation::PartialMerge,
            ] {
                let mut keep = vec![ae];
                assert!(process_wires_and_shell_vertices(
                    &k,
                    &mut a,
                    &mut b,
                    &mut am,
                    &mut bm,
                    &mut relations,
                    &mut copy(&mut records, &mut attrs),
                    None,
                    operation,
                    mt,
                    mt,
                    &mut keep,
                )
                .is_err());
                assert!(keep.is_empty());
            }
            for operation in [
                BooleanOperation::Unknown,
                BooleanOperation::Difference,
                BooleanOperation::ExclusiveOr,
                BooleanOperation::Imprint,
                BooleanOperation::ImprintClassify,
                BooleanOperation::ExtractSeparate,
                BooleanOperation::Slice,
            ] {
                let mut keep = vec![ae];
                process_wires_and_shell_vertices(
                    &k,
                    &mut a,
                    &mut b,
                    &mut am,
                    &mut bm,
                    &mut relations,
                    &mut copy(&mut records, &mut attrs),
                    None,
                    operation,
                    mt,
                    mt,
                    &mut keep,
                )
                .unwrap();
                assert!(keep.is_empty(), "{operation:?}");
            }
        }
    }
}
