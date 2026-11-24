//! Proto topology, part 1: the containers of the Boolean's intersect-insert-relate
//! phase (vertex and edge definitions, proto vertices, edges and faces, and their
//! manager), their accessors and list management, evaluation, and the static helpers
//! of `SmProtoTopology.cpp`.
//!
//! Deviation: deleting a proto edge removes every incident entry at its endpoints
//! (#374), including the two entries of a closed edge. Upstream removes only one.

use crate::curve::curve::p2::Curve;
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::geom::tol::{within_coincidence_tol, xsect_tol3d, zone_tol3d};
use crate::intersect::curve_class::{src_zone_tol_3d_of, CurveClassification, CurveInterval};
use crate::intersect::point_class::{
    object_zone_tol_3d, ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
    PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{max2, min2, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::math::gamma;
use crate::query::face::p2 as face_p2;
use crate::surface::surface::Surface;
use crate::topology::brep::p2 as brep_p2;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::proto_topology::p2::EXACT_TOL_FACTOR;
use crate::topology::topology as topo;
use crate::topology::{edge, vertex, vertexuse, Brep, EdgeId, VertexId};

use crate::curve::curve::p1::GlobalPointSolveOptions;

const OP: &str = "topology::proto_topology::p1";
const OP_REMOVE_COINCIDENT: &str =
    "topology::proto_topology::p1::remove_coincident_no_edge_proto_vertex";

/// The factor of `CanCombineEdgeDefinitions`: the distance allowed between the curves of
/// two combinable edge definitions, in units of their summed tolerances (upstream's).
pub const ED_TOL_FACTOR: f64 = 5.0;
const OP_POSITION: &str = "topology::proto_topology::p1::find_position_from_pcs";
const OP_RESOLVE: &str =
    "topology::proto_topology::p1::ProtoTopologyManager::resolve_vertex_definition";
const OP_SET_POSITION: &str = "topology::proto_topology::p1::VertexDefinition::set_position";
const OP_EVALUATE: &str = "topology::proto_topology::p1::ProtoTopologyManager::proto_edge_evaluate";
const OP_SRC_TOPO: &str = "topology::proto_topology::p1::VertexDefinition::set_src_topo";
const OP_DEGEN: &str = "topology::proto_topology::p1::find_ed_on_degen_face";
const OP_END: &str = "topology::proto_topology::p1::ProtoTopologyManager::end_position";

use crate::error::topology::TopologyFailure;

/// `SmProtoTopologyManager_TYPE` (`TOPO_BASE_TYPE + 680`).
pub const MANAGER_TYPE: i64 = 16680;

/// `SmProtoTopologyManager::GetTypeString`.
pub const MANAGER_TYPE_STRING: &str = "SmProtoTopologyManager_TYPE";

/// `SmProtoTopologyManager::GetClassString`.
pub const MANAGER_CLASS_STRING: &str = "SmProtoTopologyManager";

/// `SmProtoTopologyManager::IsKindOf`: [`MANAGER_TYPE`], then the `SmTopology` chain.
#[must_use]
pub const fn manager_is_kind_of(t: i64) -> bool {
    if t == MANAGER_TYPE {
        true
    } else {
        topo::is_kind_of(t)
    }
}

macro_rules! proto_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            /// The creation index of the object in its manager.
            #[must_use]
            pub const fn index(self) -> u32 {
                self.0
            }

            /// The id with creation index `index`.
            #[must_use]
            pub const fn from_index(index: u32) -> Self {
                Self(index)
            }
        }
    };
}

proto_id!(
    /// Identifies a [`VertexDefinition`] of a [`ProtoTopologyManager`].
    VertexDefId
);
proto_id!(
    /// Identifies an [`EdgeDefinition`] of a [`ProtoTopologyManager`].
    EdgeDefId
);
proto_id!(
    /// Identifies a [`ProtoVertex`] of a [`ProtoTopologyManager`].
    ProtoVertexId
);
proto_id!(
    /// Identifies a [`ProtoEdge`] of a [`ProtoTopologyManager`].
    ProtoEdgeId
);
proto_id!(
    /// Identifies a [`ProtoFace`] of a [`ProtoTopologyManager`].
    ProtoFaceId
);

/// Mutable access to the curves behind handles, for the edits of an
/// [`EdgeDefinition`]'s curves (reversal, reparameterisation).
pub trait ProtoGeometry<O>: ClassGeometry<O> {
    /// The curve of a [`ClassObjectKind::Curve`] handle.
    fn curve_mut(&mut self, o: O) -> Option<&mut Curve>;
}

/// The B-rep topology walks the proto topology makes over the handles' vertices,
/// edges and faces (`sm_AreConnected`, `SmProtoVertex::GetFinalVertex`,
/// `sm_FindEDOnDegenFace`). [`Brep`] answers them for [`EntityRef`] handles of its
/// own entities.
pub trait ProtoTopology<O> {
    /// `SmVertex::GetVertexuses`, each use's face (`SmVertexuse::GetFaceuse`, then
    /// `SmFaceuse::GetFace`): `None` for a use without face use or face.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn vertex_use_faces(&self, vertex: O) -> Result<Vec<Option<O>>>;
    /// `SmEdge::GetEdgeuses`, each use's face (`SmEdgeuse::GetFace`).
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn edge_use_faces(&self, edge: O) -> Result<Vec<Option<O>>>;
    /// `SmEdge::GetStartVertex`.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk (an edge without primary edge use
    /// is a `NULL` dereference upstream).
    fn edge_start_vertex(&self, edge: O) -> Result<Option<O>>;
    /// `SmEdge::GetOtherVertex`: `None` when `vertex` is not an end of the edge.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn edge_other_vertex(&self, edge: O, vertex: Option<O>) -> Result<Option<O>>;
    /// `SmVertex::GetPoint`.
    ///
    /// # Errors
    ///
    /// Those of the implementation's lookup.
    fn vertex_point(&self, vertex: O) -> Result<Point3>;
    /// `SmEdge::GetBrep`: the identity of the edge's B-rep, `None` without one.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn edge_brep(&self, edge: O) -> Result<Option<ObjectKey>>;
    /// `SmEdge::GetFaces`.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn edge_faces(&self, edge: O) -> Result<Vec<O>>;
}

/// An [`EntityRef`] of `kind`, else an invalid-input error (a failed `dynamic_cast`
/// that upstream never reaches).
fn vertex_id(o: EntityRef) -> Result<VertexId> {
    match o {
        EntityRef::Vertex(v) => Ok(v),
        _ => Err(dangling("handle is not a vertex")),
    }
}

fn edge_id(o: EntityRef) -> Result<EdgeId> {
    match o {
        EntityRef::Edge(e) => Ok(e),
        _ => Err(dangling("handle is not an edge")),
    }
}

/// The walks over one B-rep's own entities, through units `topology.vertex`,
/// `topology.vertexuse`, `topology.edge` and `topology.edgeuse_p1`.
impl ProtoTopology<EntityRef> for Brep {
    fn vertex_use_faces(&self, vertex: EntityRef) -> Result<Vec<Option<EntityRef>>> {
        let mut out = Vec::new();
        for vu in vertex::vertexuses(self, vertex_id(vertex)?)? {
            let face = match vertexuse::faceuse(self, vu)? {
                Some(fu) => eu1::faceuse_face(self, fu)?,
                None => None,
            };
            out.push(face.map(EntityRef::Face));
        }
        Ok(out)
    }

    fn edge_use_faces(&self, edge: EntityRef) -> Result<Vec<Option<EntityRef>>> {
        let mut out = Vec::new();
        for eu in edge::edgeuses(self, edge_id(edge)?, None, None)? {
            out.push(eu1::face(self, eu)?.map(EntityRef::Face));
        }
        Ok(out)
    }

    fn edge_start_vertex(&self, edge: EntityRef) -> Result<Option<EntityRef>> {
        Ok(edge::start_vertex(self, edge_id(edge)?)?.map(EntityRef::Vertex))
    }

    fn edge_other_vertex(
        &self,
        edge: EntityRef,
        vertex: Option<EntityRef>,
    ) -> Result<Option<EntityRef>> {
        let v = match vertex {
            Some(v) => Some(vertex_id(v)?),
            None => None,
        };
        Ok(edge::other_vertex(self, edge_id(edge)?, v)?.map(EntityRef::Vertex))
    }

    fn vertex_point(&self, vertex: EntityRef) -> Result<Point3> {
        vertex::point(self, vertex_id(vertex)?)
    }

    fn edge_brep(&self, edge: EntityRef) -> Result<Option<ObjectKey>> {
        Ok(eu1::edge_brep(self, edge_id(edge)?)?.map(Brep::key))
    }

    fn edge_faces(&self, edge: EntityRef) -> Result<Vec<EntityRef>> {
        Ok(edge::faces(self, edge_id(edge)?, None)?
            .into_iter()
            .map(EntityRef::Face)
            .collect())
    }
}

/// The handle the curve point solves name (only the solution count is used).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveHandle(Extent1d);

impl SolutionObject for CurveHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.0)
    }
}

/// A dereference of a null pointer or a deleted object upstream (undefined
/// behaviour).
const fn dangling(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// `SM_CAST_PTR(SmTopology, obj)`: the object when it is a topology object.
fn as_topology<O: ClassObject>(o: Option<O>) -> Option<O> {
    o.filter(|o| {
        matches!(
            o.class_kind(),
            ClassObjectKind::Vertex
                | ClassObjectKind::Edge
                | ClassObjectKind::Face
                | ClassObjectKind::Loop
                | ClassObjectKind::Region
                | ClassObjectKind::Vertexuse
                | ClassObjectKind::Edgeuse
                | ClassObjectKind::Faceuse
                | ClassObjectKind::Loopuse
        )
    })
}

/// The curve behind `o` (a `NULL` dereference upstream for none).
fn curve_of<O: Copy>(geometry: &impl ClassGeometry<O>, o: Option<O>) -> Result<&Curve> {
    o.and_then(|o| geometry.curve(o))
        .ok_or(dangling("curve handle is null or unknown"))
}

/// `FindObjPoint3d` with its status ignored: the object point, `SetUninitialized`
/// on failure.
fn obj_point<O: ClassObject>(
    k: &Kernel,
    pc: &PointClassification<O>,
    geometry: &impl ClassGeometry<O>,
) -> Result<Point3> {
    Ok(ignored(pc.find_obj_point_3d(k, geometry))?.unwrap_or(Vec3::UNDEF))
}

/// `FindObjPoint3d` of a vertex definition's classification `pc`, the definition being
/// at `position`. A classification without parameters (an interval's midpoint after a
/// split, a region) has no object point, and upstream's `FindObjPoint3d` fails on it,
/// its status ignored and the unset point read. Not upstream (#18927): such a side gives
/// no evidence against the definition, whose position `sm_FindPositionFromPCs` took from
/// the other side, so its object point is `position`; the definition is then moved only
/// by a gap between two real object points. Every other failure is passed on, as the
/// fork does (bcc6f01).
fn definition_object_point<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    pc: &PointClassification<O>,
    position: Point3,
) -> Result<Point3> {
    if !pc.are_parameters_set() || pc.point_class() == PointClassType::Region {
        return Ok(position);
    }
    pc.find_obj_point_3d(k, geometry)
}

/// `EvaluatePoint` as the fork reads it (bcc6f01): `None` for a point that cannot be
/// evaluated, where the answer is then "no" ("no value, no evidence"; upstream reads
/// the unset point).
fn eval_point_or_no(k: &Kernel, curve: &Curve, t: f64) -> Result<Option<Point3>> {
    ignored(curve.evaluate_point(k, t))
}

// ---- static helpers ------------------------------------------------------------------

/// Whether a classification of type `t` can be imprinted (`sm_CanImprint`): regions,
/// faces, edges and vertices.
#[must_use]
pub fn can_imprint(t: PointClassType) -> bool {
    t == PointClassType::Region
        || t == PointClassType::Face
        || t == PointClassType::Edge
        || t == PointClassType::Vertex
}

/// The dimension of a topology object (`sm_TopoDimension`): 0 for a vertex, 1 for an
/// edge, 2 for a face, -1 otherwise.
#[must_use]
pub fn topo_dimension<O: ClassObject>(topo: O) -> i32 {
    match topo.class_kind() {
        ClassObjectKind::Vertex => 0,
        ClassObjectKind::Edge => 1,
        ClassObjectKind::Face => 2,
        _ => -1,
    }
}

/// The result of [`find_position_from_pcs`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionFromPcs {
    /// `rPosition`: the position the two classifications agree on.
    pub position: Point3,
    /// `rdDeviation`: the distance between the two object points (0 when only one
    /// was known at the start).
    pub deviation: f64,
}

/// Not upstream: the curve of the seam `pc` classifies onto: the seam edge its vertex ends
/// ([`vertex::seam_edge`]), or the seam edge itself when `pc` lies on one
/// ([`vertex::is_seam_edge`]), where the definition's vertex will split it and end both
/// pieces. `None` when `pc` is on neither, or the adapter cannot resolve its handle.
///
/// # Errors
///
/// Those of [`vertex::seam_edge`], [`vertex::is_seam_edge`] and [`edge::curve`].
fn seam_curve<'g, O: ClassObject>(
    geometry: &'g impl ClassGeometry<O>,
    pc: &PointClassification<O>,
) -> Result<Option<&'g Curve>> {
    let Some((b, entity)) = pc.object().and_then(|o| geometry.topology_entity(o)) else {
        return Ok(None);
    };
    match (pc.point_class(), entity) {
        (PointClassType::Vertex, EntityRef::Vertex(v)) => match vertex::seam_edge(b, v)? {
            Some(e) => edge::curve(b, e),
            None => Ok(None),
        },
        (PointClassType::Edge, EntityRef::Edge(e)) if vertex::is_seam_edge(b, e)? => {
            edge::curve(b, e)
        }
        _ => Ok(None),
    }
}

/// Not upstream: the point of the seam `curve` that `p` lies over: `p`'s foot on the curve
/// over its natural interval, moved along the curve's tangent there by `p`'s offset along
/// that tangent. With the foot inside the interval the offset is zero and the point is the
/// seam's nearest to `p`; where the curve stops short of `p` (a seam line ending at the
/// vertex, `p` past that end) it carries on along the seam's direction, as the seam of the
/// closed surface does.
///
/// # Errors
///
/// Those of [`Curve::drop_point`] and [`Curve::evaluate`]; a topology failure when `p`
/// does not drop onto the curve.
fn onto_seam(k: &Kernel, curve: &Curve, p: Point3) -> Result<Point3> {
    let natural = curve.natural_interval();
    let foot = curve.drop_point(k, &natural, p, None, 0.0, None, SolverOperation::Minimize)?;
    if !foot.success {
        return Err(SolidError::topology(
            OP_RESOLVE,
            TopologyFailure::NoFootOnSeam,
        ));
    }
    let eval = curve.evaluate(k, foot.parameter, 1, false, false)?;
    let (Some(&at), Some(&tangent)) = (eval.first(), eval.get(1)) else {
        return Err(SolidError::topology(
            OP_RESOLVE,
            TopologyFailure::Defect {
                why: "the seam's evaluation with one derivative at the definition's foot gave \
                      no point or no tangent, though it returns both at every parameter it \
                      evaluates",
            },
        ));
    };
    let len2 = tangent.dot(tangent);
    if len2 <= 0.0 {
        return Ok(at);
    }
    Ok(at + ((p - at).dot(tangent) / len2) * tangent)
}

/// The position two point classifications define (`sm_FindPositionFromPCs`). A
/// classification without parameters gets them from the other's object point when
/// possible. With both points known, the lower-dimensional object wins; at equal
/// dimension the points are blended towards the one with the smaller gap.
///
/// # Errors
///
/// `SM_ERR` when neither classification has parameters; those of
/// [`PointClassification::find_obj_point_3d`] and of the objects' zone tolerances
/// ([`SolidError::Unported`] for topology objects).
pub fn find_position_from_pcs<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    pc1: &mut PointClassification<O>,
    pc2: &mut PointClassification<O>,
) -> Result<PositionFromPcs> {
    let mut pt1 = Vec3::UNDEF;
    let mut pt2 = Vec3::UNDEF;

    // These should have been set in the SmVertexDefinition constructor.
    let mut found1 = pc1.are_parameters_set();
    let mut found2 = pc2.are_parameters_set();

    // error - both Parameters are not Set
    if !found1 && !found2 {
        return Err(SolidError::topology(
            OP_POSITION,
            TopologyFailure::ClassificationParametersUnset,
        ));
    }

    if found1 {
        pt1 = pc1.find_obj_point_3d(k, geometry)?;
    }
    if found2 {
        pt2 = pc2.find_obj_point_3d(k, geometry)?;
    }

    // (SmTol::GetXSectTol3d of the two objects only feeds a diagnostic message when
    // the deviation exceeds it; it is not computed here.)

    // deviation = dist between PtClassifyObjects when both point classifies have
    // ParametersSet, else 0.0
    let deviation = if found1 && found2 {
        pt1.distance(pt2)
    } else {
        0.0
    };

    let mut position = Vec3::UNDEF;
    if !found1 {
        // See if we can find a position for rPC1 from sPt2.
        let ok = ignored(pc1.compute_point_parameters(k, geometry, pt2))?.is_some();
        if ok && pc1.are_parameters_set() {
            pt1 = pc1.find_obj_point_3d(k, geometry)?;
            found1 = true;
        } else {
            position = pt2;
        }
    } else if !found2 {
        // See if we can find a position for rPC2 from sPt1.
        let ok = ignored(pc2.compute_point_parameters(k, geometry, pt1))?.is_some();
        if ok && pc2.are_parameters_set() {
            pt2 = pc2.find_obj_point_3d(k, geometry)?;
            found2 = true;
        } else {
            position = pt1;
        }
    }

    // when both PCs have Parameters
    if found1 && found2 {
        // high dimension means low weight
        let dim1 = as_topology(pc1.object()).map_or(3, topo_dimension);
        let dim2 = as_topology(pc2.object()).map_or(3, topo_dimension);

        // Give more weight to the one with lower dimension.
        position = match dim1.cmp(&dim2) {
            core::cmp::Ordering::Less => pt1,
            core::cmp::Ordering::Greater => pt2,
            core::cmp::Ordering::Equal => {
                // Same dimension. Give more weight to the one with the tighter gap.
                let gap1 = pc1.gap_3d(k, geometry, None)?;
                let gap2 = pc2.gap_3d(k, geometry, None)?;
                let sum = gap1 + gap2;
                let frac = if sum > EFF_ZERO { gap1 / sum } else { 0.5 };
                (1.0 - frac) * pt1 + frac * pt2
            }
        };
    }

    Ok(PositionFromPcs {
        position,
        deviation,
    })
}

/// Whether two intersection curves are meant to be the same (`sm_AreSameCurve`): the
/// same curve over the same interval, or the midpoint of the first drops onto the
/// second (an intersecting drop, guessed at the second interval's middle) closer than
/// `tol`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a missing curve that is dereferenced (undefined
/// upstream); [`SolidError::Unported`] from the drop.
pub fn are_same_curve<O: Copy + PartialEq>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    crv1: Option<O>,
    ivl1: &Extent1d,
    crv2: Option<O>,
    ivl2: &Extent1d,
    tol: f64,
) -> Result<bool> {
    // low work - same curve.
    if crv1 == crv2 {
        // return TRUE when intervals are the same
        return Ok(ivl1.are_same(ivl2));
    }

    // locals
    let param1 = ivl1.mid();
    let guess = ivl2.mid();

    // Crv1 mid point
    let Some(mid_pt1) = eval_point_or_no(k, curve_of(geometry, crv1)?, param1)? else {
        return Ok(false);
    };

    // Drop Crv1 mid point to Crv2 - get drop distance
    let dropped = ignored(curve_of(geometry, crv2)?.drop_point(
        k,
        ivl2,
        mid_pt1,
        None,
        tol,
        Some(guess),
        SolverOperation::Intersect,
    ))?;

    // return TRUE when DropPoint() succeeded and DropDist < dTol
    Ok(dropped.is_some_and(|d| d.success && d.distance < tol))
}

/// Not upstream (#15718): whether two edge definitions on lines are one piece within
/// `tol`, decided over both whole pieces: each end of the first definition's interval lies
/// within `tol` of the second's segment (the piece itself, not its infinite line), and
/// each end of the second's within `tol` of the first's ([`end_within_segment`], rounding
/// counted). The distance from a point to a segment is convex in the point (a segment is
/// convex), so along a piece its maximum is reached at one of the piece's ends: the four
/// end checks bound every point of each piece within `tol` of the other piece, in both
/// directions (their Hausdorff distance). Two pieces of one line shifted along it by more
/// than `tol` are therefore two pieces, as two crossing pieces are; upstream's
/// [`are_same_curve`] drops one midpoint onto the other's curve instead. `false` when either
/// curve is not a line or either piece has zero length (nothing combined: the definitions
/// are kept).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a missing curve; those of the end evaluations.
fn exact_lines_coincide<O: Copy>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    d1: &EdgeDefinition<O>,
    d2: &EdgeDefinition<O>,
    tol: f64,
) -> Result<bool> {
    use crate::curve::curve::CurveKind;
    let c1 = curve_of(geometry, d1.xsect_curve_3d)?;
    let c2 = curve_of(geometry, d2.xsect_curve_3d)?;
    let line = |c: &Curve| matches!(c.kind(), CurveKind::Line(_));
    if !line(c1) || !line(c2) {
        return Ok(false);
    }
    let ends = |c: &Curve, ivl: &Extent1d| -> Result<[Point3; 2]> {
        Ok([
            c.evaluate_point(k, ivl.min())?,
            c.evaluate_point(k, ivl.max())?,
        ])
    };
    let p1 = ends(c1, &d1.xsect_interval)?;
    let p2 = ends(c2, &d2.xsect_interval)?;
    Ok(p1.iter().all(|&p| end_within_segment(p, p2, tol))
        && p2.iter().all(|&p| end_within_segment(p, p1, tol)))
}

/// Not upstream (#15718): whether `p` lies within `tol` of the segment `seg`, with an upper
/// bound of the distance: the distance to the nearer end when the computed foot falls
/// outside the segment (an end is a point of the segment, so that bounds the distance
/// whatever the foot's rounding), and otherwise the distance to the segment's line plus the
/// rounding of the foot and the line distance (the differences carry a few units in the
/// last place of the coordinates, amplified by `|p - a| / |b - a|`). `false` for a segment
/// of zero length and for any NaN.
fn end_within_segment(p: Point3, seg: [Point3; 2], tol: f64) -> bool {
    use crate::nurbs::math::gamma;
    let [a, b] = seg;
    let along = b - a;
    let length = along.length();
    let usable = length > 0.0;
    if !usable {
        return false;
    }
    let w = p - a;
    let s = w.dot(along) / (length * length);
    let scale = p.length() + a.length() + b.length();
    let bound = if s <= 0.0 {
        p.distance(a) + gamma(8) * (p.length() + a.length())
    } else if s >= 1.0 {
        p.distance(b) + gamma(8) * (p.length() + b.length())
    } else {
        w.cross(along).length() / length + gamma(16) * scale * (1.0 + w.length() / length)
    };
    bound < tol
}

/// `sm_AreConnected(const SmVertex*, const SmFace*)`.
fn are_connected_vertex_face<O: Copy + PartialEq>(
    topology: &impl ProtoTopology<O>,
    vtx: O,
    face: O,
) -> Result<bool> {
    for f in topology.vertex_use_faces(vtx)? {
        if f == Some(face) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// `sm_AreConnected(const SmEdge*, const SmFace*)`.
fn are_connected_edge_face<O: Copy + PartialEq>(
    topology: &impl ProtoTopology<O>,
    edge: O,
    face: O,
) -> Result<bool> {
    for f in topology.edge_use_faces(edge)? {
        if f == Some(face) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// `sm_AreConnected(const SmVertex*, const SmEdge*)`.
fn are_connected_vertex_edge<O: Copy + PartialEq>(
    topology: &impl ProtoTopology<O>,
    vertex: O,
    edge: O,
) -> Result<bool> {
    let vtx = topology.edge_start_vertex(edge)?;
    if vtx == Some(vertex) {
        return Ok(true);
    }

    let vtx = topology.edge_other_vertex(edge, vtx)?;
    if vtx == Some(vertex) {
        return Ok(true);
    }

    Ok(false)
}

/// Whether two topology objects are neighbours in their B-rep
/// (`sm_AreConnected(const SmTopology*, const SmTopology*)`): the same object, or a
/// face and a boundary edge or vertex, or an edge and a boundary vertex; any other
/// pair is not connected.
///
/// # Errors
///
/// Those of the `topology` walks.
pub fn are_connected<O: ClassObject>(
    topology: &impl ProtoTopology<O>,
    topo1: Option<O>,
    topo2: Option<O>,
) -> Result<bool> {
    use ClassObjectKind as K;
    if topo1 == topo2 {
        return Ok(true);
    }
    let of_kind = |o: Option<O>, k: K| o.filter(|o| o.class_kind() == k);

    if let Some(face1) = of_kind(topo1, K::Face) {
        if let Some(edge2) = of_kind(topo2, K::Edge) {
            return are_connected_edge_face(topology, edge2, face1);
        }
        if let Some(vertex2) = of_kind(topo2, K::Vertex) {
            return are_connected_vertex_face(topology, vertex2, face1);
        }
    }
    if let Some(edge1) = of_kind(topo1, K::Edge) {
        if let Some(face2) = of_kind(topo2, K::Face) {
            return are_connected_edge_face(topology, edge1, face2);
        }
        if let Some(vertex2) = of_kind(topo2, K::Vertex) {
            return are_connected_vertex_edge(topology, vertex2, edge1);
        }
    }
    if let Some(vertex1) = of_kind(topo1, K::Vertex) {
        if let Some(face2) = of_kind(topo2, K::Face) {
            return are_connected_vertex_face(topology, vertex1, face2);
        }
        if let Some(edge2) = of_kind(topo2, K::Edge) {
            return are_connected_vertex_edge(topology, vertex1, edge2);
        }
    }
    Ok(false)
}

/// Whether `param` is strictly inside `domain` of `curve`, also in 3d
/// (`sm_IsInterior`): inside the domain, and its point farther than `zone_tol_3d`
/// from both end points.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a missing curve; [`SolidError::Unported`] from
/// the evaluations.
pub fn is_interior<O: Copy>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    curve: Option<O>,
    domain: &Extent1d,
    param: f64,
    zone_tol_3d: f64,
) -> Result<bool> {
    // low work - When Param is outside of Curve domain - it's not interior
    if !domain.contains_value(param, 0.0) {
        return Ok(false);
    }

    // next: Check 3d Pts. if this param's Pt3d is within ZoneTol3d dist of a crv
    // endpoint it on the boundary not the interior
    let c = curve_of(geometry, curve)?;
    let (Some(crv_pt), Some(start_pt), Some(end_pt)) = (
        eval_point_or_no(k, c, param)?,
        eval_point_or_no(k, c, domain.min())?,
        eval_point_or_no(k, c, domain.max())?,
    ) else {
        return Ok(false);
    };

    if crv_pt.closer_than(zone_tol_3d, start_pt) {
        return Ok(false);
    }
    if crv_pt.closer_than(zone_tol_3d, end_pt) {
        return Ok(false);
    }
    Ok(true)
}

/// Drops `point` onto `curve` over `domain` (intersecting within `tol`, guessed at
/// `guess`) and tests the drop parameter with [`is_interior`].
fn drop_is_interior<O: Copy>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    curve: Option<O>,
    domain: &Extent1d,
    point: Point3,
    tol: f64,
    guess: f64,
) -> Result<bool> {
    let c = curve_of(geometry, curve)?;
    let dropped = ignored(c.drop_point(
        k,
        domain,
        point,
        None,
        tol,
        Some(guess),
        SolverOperation::Intersect,
    ))?;
    match dropped {
        Some(d) if d.success => is_interior(k, geometry, curve, domain, d.parameter, tol),
        _ => Ok(false),
    }
}

/// Whether a curve interval and an edge definition overlap over a segment
/// (`sm_AreOverlapping`): an end of either curve drops (within the larger of the
/// interval's source zone tolerance and the definition's tolerance) to the interior
/// of the other. `classification` is the classification `interval` belongs to (its
/// curve and tolerance; upstream reaches it through the interval's back pointer).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a missing curve; [`SolidError::Unported`] from
/// the drops.
pub fn are_overlapping<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    classification: &CurveClassification<O>,
    interval: &CurveInterval<O>,
    ed: &EdgeDefinition<O>,
) -> Result<bool> {
    // locals
    let ci_curve = classification.curve();
    let ci_domain = *interval.interval();
    let ed_curve = ed.xsect_curve_3d();
    let ed_domain = ed.xsect_interval();

    // tolerance
    let zone_tol_3d = zone_tol3d(
        k,
        max2(src_zone_tol_3d_of(k, Some(classification)), ed.tolerance(k)),
    );

    // Drop ED->Curve Start and End Pts to CI->CurveClassification->Curve
    {
        let Some(start_pt) = eval_point_or_no(k, curve_of(geometry, ed_curve)?, ed_domain.min())?
        else {
            return Ok(false);
        };
        if drop_is_interior(
            k,
            geometry,
            ci_curve,
            &ci_domain,
            start_pt,
            zone_tol_3d,
            ci_domain.evaluate(0.1),
        )? {
            return Ok(true);
        }

        let Some(end_pt) = eval_point_or_no(k, curve_of(geometry, ed_curve)?, ed_domain.max())?
        else {
            return Ok(false);
        };
        if drop_is_interior(
            k,
            geometry,
            ci_curve,
            &ci_domain,
            end_pt,
            zone_tol_3d,
            ci_domain.evaluate(0.9),
        )? {
            return Ok(true);
        }
    }

    // Drop CI->Curve->EndPts to ED->Curve
    {
        let Some(start_pt) = eval_point_or_no(k, curve_of(geometry, ci_curve)?, ci_domain.min())?
        else {
            return Ok(false);
        };
        if drop_is_interior(
            k,
            geometry,
            ed_curve,
            &ed_domain,
            start_pt,
            zone_tol_3d,
            ci_domain.evaluate(0.1),
        )? {
            return Ok(true);
        }

        let Some(end_pt) = eval_point_or_no(k, curve_of(geometry, ci_curve)?, ci_domain.max())?
        else {
            return Ok(false);
        };
        if drop_is_interior(
            k,
            geometry,
            ed_curve,
            &ed_domain,
            end_pt,
            zone_tol_3d,
            ed_domain.evaluate(0.9),
        )? {
            return Ok(true);
        }
    }

    // none of the CI or CD curve endpoints drop to the interior of the other curve
    Ok(false)
}

/// Which of two coincident edges of one B-rep lies on a degenerate face and can be
/// deleted (`sm_FindEDOnDegenFace`; 0 neither, 1 the first, 2 the second): 0 when the
/// edges share no face.
///
/// # Errors
///
/// `SM_ERR_ASSERT_FAILURE` for edges of different B-reps; those of the `topology`
/// walks; [`SolidError::Unported`] for a shared face (`SmFace::GetEdges`, unit
/// `topology.face_p4`).
pub fn find_ed_on_degen_face<O: Copy + PartialEq>(
    topology: &impl ProtoTopology<O>,
    edge1: O,
    edge2: O,
) -> Result<u64> {
    // Check inputs
    let brep1 = topology.edge_brep(edge1)?;
    let brep2 = topology.edge_brep(edge2)?;
    if brep1 != brep2 {
        return Err(SolidError::topology(
            OP_DEGEN,
            TopologyFailure::EdgesOnDifferentBreps,
        ));
    }

    // Find shared faces (FindCommonElements: the second list's faces found in the first)
    let ed1_fs = topology.edge_faces(edge1)?;
    let ed2_fs = topology.edge_faces(edge2)?;
    let common: Vec<O> = ed2_fs.into_iter().filter(|f| ed1_fs.contains(f)).collect();

    // Degenerate face has 2 edges within tol of edge on other brep
    if let Some(face) = common.first() {
        let _ = face;
        // UNPORTED(topology.face_p4): SmFace::GetEdges, then SmBrep::DeleteFace(pFace,
        // FALSE, TRUE, &sTopos)
        // Proof (unreachable): the only caller, `CanCombineEdgeDefinitions`, gets here
        // with two different edges of one B-rep, which `sm_AreConnected` (no edge-edge
        // case, upstream and `are_connected`) reports unconnected, so it has already
        // returned 0. Upstream's `DeleteFace` without `bDeleteEdgesVertices` also leaves
        // `sTopos` empty, so even a reached call would answer 0 (its only effect being
        // the face deletion).
        return Err(SolidError::unported("SmFace::GetEdges", "topology.face_p4"));
    }

    // If no common faces, or deleted common face w/out deleting edge, then we have
    // coincident unstitched edges
    Ok(0)
}

// ---- SmVertexDefinition -----------------------------------------------------------------

/// One point intersection between a topology object of each B-rep
/// (`SmVertexDefinition`).
#[derive(Debug, Clone, PartialEq)]
pub struct VertexDefinition<O> {
    /// `m_sPC1`: the B-rep 1 classification of the point.
    pc1: PointClassification<O>,
    /// `m_sPC2`: the B-rep 2 classification of the point.
    pc2: PointClassification<O>,
    /// `m_sPosition`.
    position: Point3,
    /// `m_dTol`.
    tol: f64,
    /// `m_dGap`.
    gap: f64,
    /// `m_pMyProtoVertex`.
    proto_vertex: Option<ProtoVertexId>,
    /// `m_pFinalVtx1`.
    final_vtx1: Option<O>,
    /// `m_pFinalVtx2`.
    final_vtx2: Option<O>,
    /// Not upstream (WR-410, tangency-labs/solidcore#1059; the SolidCraft fork's
    /// `m_dCrossingSine`): the sine of the angle at which the intersection curve that
    /// placed this definition crosses its source edge; `-1` when no such crossing placed
    /// it ([`VertexDefinition::place_by_crossing`]).
    crossing_sine: f64,
    /// Not upstream (WR-410): the classifications this definition had when the first
    /// crossing reached it, from which every later placement's stretch is certified
    /// ([`VertexDefinition::place_by_crossing`]); `None` before that.
    first_placement: Option<[PointClassification<O>; 2]>,
    /// Not upstream (WR-410): the intersection curve whose crossing placed this definition
    /// first and its parameter at that end; `None` before the first crossing or when the
    /// crossing named no curve.
    first_curve: Option<(O, f64)>,
    /// Not upstream (WR-306): the certain hull points of the curves this definition
    /// absorbed (`resolve_proto_edge`), each with the error bound of its coordinates,
    /// which [`VertexDefinition::set_position`] keeps within the tolerance.
    absorbed_hull: Vec<(Point3, f64)>,
}

/// Not upstream (WR-410; the SolidCraft fork's `sm_CrossingError`): how far along its
/// edge a crossing at angle `asin(crossing_sine)`, whose points lie `gap` apart, may
/// misplace a vertex definition: the gap over the sine, or `BIG_DOUBLE` when the curve
/// runs along the edge (within the angular tolerance) or the angle is unknown.
fn crossing_error(gap: f64, crossing_sine: f64) -> f64 {
    // (upstream's `!(sine >= min)`: a NaN sine is unknown too)
    if crossing_sine.partial_cmp(&crate::geom::tol::ang_tol_rad().sin())
        == Some(core::cmp::Ordering::Less)
        || crossing_sine.is_nan()
    {
        return BIG_DOUBLE;
    }
    max2(gap, 0.0) / crossing_sine
}

/// Not upstream (WR-410): whether a vertex definition placed by `from` (its classifications
/// at the first crossing, made by the intersection curve `first_curve` at its parameter
/// there) and placed again by `to` stays one point. For each side classified on an edge
/// whose parameter moved, two certificates over the stretch of the edge between its two
/// parameters ([`curve_piece_gap_bound`], a bound over the whole piece with rounding
/// counted), both at most `tol`:
///
/// - the edge stays within `tol` of the other side's object all along it: its vertex's
///   point, its face's surface (matched by the straight map between the face's two
///   parameters) or its edge's curve (likewise), so every point of the stretch is a point of
///   both source objects;
/// - the edge stays within `tol` of the first crossing's curve all along it, matched to the
///   curve's piece from its parameter at the first crossing to the one nearest the new
///   point (a local drop, a witness only: the bound holds for any parameter; the straight
///   map is onto that piece, so each of the two lies within the bound of the other), so the
///   first crossing does not fix where along the stretch the curve meets the edge, and the
///   curve reaches the new point. A new point past the curve's end drops onto the end, and
///   the whole stretch must then lie within `tol` of that one point.
///
/// A steep first crossing leaves the edge at once, so it certifies only a stretch within
/// the tolerance; a grazing one runs along the edge as far as it stays within it, and no
/// farther (no reach argued from the angle). A side on a vertex has no stretch of its own;
/// its other side's edge bounds it. A side on a face whose uv moved is certified only as the
/// target of the other side's moved edge stretch (its surface piece between the two uv's);
/// a uv move without one is `false`. `false` also when a
/// classification's kind or object changed since `from`, the first crossing named no
/// curve, a drop or a bound is not made (another curve kind, a surface without a net) or a
/// bound is past `tol` (NaN included): the definition then keeps its placement.
///
/// # Errors
///
/// Those of resolving the classifications' objects and the kernel's unported and cancelled
/// statuses.
fn crossing_stretch_certified<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    from: &[PointClassification<O>; 2],
    first_curve: Option<(O, f64)>,
    to: [&PointClassification<O>; 2],
    tol: f64,
) -> Result<bool> {
    use crate::geom::global_solver::SolverOperation;
    use crate::query::curve_surface_gap::{curve_piece_gap_bound, PieceTarget};
    // (`bound <= tol`; NaN or no bound refuses)
    let within = |bound: Option<f64>| {
        matches!(
            bound.and_then(|g| g.partial_cmp(&tol)),
            Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
        )
    };
    for (a, b) in from.iter().zip(to) {
        if a.point_class() != b.point_class() || as_topology(a.object()) != as_topology(b.object())
        {
            return Ok(false);
        }
    }
    for (i, j) in [(0_usize, 1_usize), (1, 0)] {
        let (Some(a), Some(b), Some(oa), Some(ob)) =
            (from.get(i), to.get(i), from.get(j), to.get(j))
        else {
            return Ok(false);
        };
        // (a face's uv that moved is certified only as the target of the other side's
        // moved edge stretch, below; with no such stretch the move is not certified)
        if b.point_class() == PointClassType::Face
            && a.uv_param() != b.uv_param()
            && (ob.point_class() != PointClassType::Edge || oa.t_param() == ob.t_param())
        {
            return Ok(false);
        }
        // (only an edge's parameter moves along a stretch; equal ones have none)
        if b.point_class() != PointClassType::Edge || a.t_param() == b.t_param() {
            continue;
        }
        let Some(edge) = b.curve_object(geometry)? else {
            return Ok(false);
        };
        let stretch = [a.t_param(), b.t_param()];
        let target = match ob.point_class() {
            PointClassType::Vertex => PieceTarget::Point(ob.find_obj_point_3d(k, geometry)?),
            PointClassType::Face => {
                let Some(s) = ob.surface_object(geometry)? else {
                    return Ok(false);
                };
                PieceTarget::Surface(s, [oa.uv_param(), ob.uv_param()])
            }
            PointClassType::Edge => {
                let Some(c) = ob.curve_object(geometry)? else {
                    return Ok(false);
                };
                PieceTarget::Curve(c, [oa.t_param(), ob.t_param()])
            }
            _ => return Ok(false),
        };
        let on_other = curve_piece_gap_bound(k, edge, stretch, target, 0.01 * tol, tol)?;
        // the first crossing's curve, from its crossing to its point nearest the new one
        let Some((curve, s0)) = first_curve.and_then(|(o, s)| Some((geometry.curve(o)?, s))) else {
            return Ok(false);
        };
        let to_point = b.find_obj_point_3d(k, geometry)?;
        let dropped = match curve.drop_point(
            k,
            &curve.natural_interval(),
            to_point,
            None,
            tol,
            Some(s0),
            SolverOperation::Minimize,
        ) {
            Ok(d) => d,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        };
        if !dropped.success {
            return Ok(false);
        }
        // (the edge against the curve, matched end to end: the straight map is onto the
        // curve's piece, so every point of either lies within the bound of the other; a
        // drop onto the crossing itself makes the piece that point)
        let along = curve_piece_gap_bound(
            k,
            edge,
            stretch,
            PieceTarget::Curve(curve, [s0, dropped.parameter]),
            0.01 * tol,
            tol,
        )?;
        if !(within(on_other) && within(along)) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The most Newton steps [`common_point`] takes: a bound on the loop; convergence is
/// checked at each step, never assumed.
const COMMON_POINT_STEPS: usize = 32;

/// Not upstream (#3062): the value at `x` of the implicit equation of `a` and its gradient
/// (a plane, a cylinder or cone, a sphere; `None` for a torus, whose equation is quartic).
fn implicit_value(a: &crate::surface::implicit::Implicit, x: Point3) -> Option<(f64, Vec3)> {
    use crate::surface::implicit::Implicit;
    Some(match *a {
        Implicit::Plane { origin, normal } => ((x - origin).dot(normal), normal),
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => {
            let u = x - origin;
            let h = u.dot(axis);
            let radial = u - axis * h;
            let r = radius + tan_angle * h;
            let f = radial.dot(radial) - r * r;
            let g = (radial - axis * (r * tan_angle)) * 2.0;
            (f, g)
        }
        Implicit::Sphere { centre, radius } => {
            let u = x - centre;
            (u.dot(u) - radius * radius, u * 2.0)
        }
        Implicit::Torus { .. } => return None,
    })
}

/// Not upstream (#3062): the common point of three analytic surfaces nearest `start`, by
/// Newton's method on their implicit equations: `None` where one is not a plane, cylinder,
/// cone or sphere, the normals do not span space at a step, or it does not converge in
/// [`COMMON_POINT_STEPS`] steps to within rounding of `start`'s size.
fn common_point(surfaces: [&Surface; 3], start: Point3) -> Option<Point3> {
    use crate::surface::implicit::AnalyticSupport;
    let of = |s: &Surface| AnalyticSupport::of(s).map(|a| a.implicit());
    let [s0, s1, s2] = surfaces;
    let eq = [of(s0)?, of(s1)?, of(s2)?];
    let size = 1.0 + start.max_dimension();
    let mut x = start;
    for _ in 0..COMMON_POINT_STEPS {
        let mut rows = [(0.0, Vec3::ZERO); 3];
        for (row, a) in rows.iter_mut().zip(&eq) {
            *row = implicit_value(a, x)?;
        }
        let [(f0, g0), (f1, g1), (f2, g2)] = rows;
        let det = g0.dot(g1.cross(g2));
        let scale = g0.length() * g1.length() * g2.length();
        if det.is_nan() || det.abs() <= EFF_ZERO * scale {
            return None;
        }
        // Cramer's rule for g_i . dx = -f_i
        let dx = (g1.cross(g2) * -f0 + g2.cross(g0) * -f1 + g0.cross(g1) * -f2) / det;
        x += dx;
        if dx.length() <= 4.0 * f64::EPSILON * size {
            return Some(x);
        }
    }
    None
}

impl<O: ClassObject> VertexDefinition<O> {
    /// Not upstream (#3062): where this definition is a crossing of an edge of one B-rep
    /// (between two faces) and a face of the other, all three on planes, cylinders, cones or
    /// spheres, it is placed at their common point, solved on the surfaces from its
    /// position: a curve crossing the edge at a grazing angle fixes the crossing along the
    /// edge only to the tolerance over the angle's sine, and the curves meeting there each
    /// come within tolerance of the third face at a different point. Moved only within the
    /// definition's own tolerance. Returns whether it moved.
    ///
    /// # Errors
    ///
    /// Those of the topology walks.
    pub fn place_on_three_surfaces(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
    ) -> Result<bool> {
        let (edge_pc, face_pc) = match (self.pc1.point_class(), self.pc2.point_class()) {
            (PointClassType::Edge, PointClassType::Face) => (self.pc1, self.pc2),
            (PointClassType::Face, PointClassType::Edge) => (self.pc2, self.pc1),
            _ => return Ok(false),
        };
        let (Some(eo), Some(fo)) = (edge_pc.object(), face_pc.object()) else {
            return Ok(false);
        };
        let (Some((eb, EntityRef::Edge(e))), Some((fb, EntityRef::Face(f)))) =
            (geometry.topology_entity(eo), geometry.topology_entity(fo))
        else {
            return Ok(false);
        };
        let [a, b] = edge::faces(eb, e, None)?[..] else {
            return Ok(false);
        };
        let (Some(sa), Some(sb), Some(sf)) = (
            crate::topology::face::p2::surface(eb, a)?,
            crate::topology::face::p2::surface(eb, b)?,
            crate::topology::face::p2::surface(fb, f)?,
        ) else {
            return Ok(false);
        };
        let Some(x) = common_point([sa, sb, sf], self.position) else {
            return Ok(false);
        };
        let moved = x.distance(self.position);
        if moved.is_nan() || moved > self.tolerance(k) {
            return Ok(false);
        }
        // Already at the common point to rounding: left as it is, bit for bit, so a vertex
        // the crossing placed exactly is not nudged by the last digits of the solve.
        if moved <= 8.0 * f64::EPSILON * (1.0 + self.position.max_dimension()) {
            return Ok(false);
        }
        self.position = x;
        Ok(true)
    }

    /// `SmVertexDefinition(pCtx, cpPC1, cpPC2, rPosition, dTol, dGap, pOwner)`: copies
    /// the classifications and computes their parameters at `position` when unset
    /// (statuses ignored); a non-positive `gap` becomes the distance between the two
    /// object points, at least either classification's gap.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the classifications (topology objects, surface
    /// point solves).
    pub fn new(
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
        position: Point3,
        tol: f64,
        gap: f64,
        owner: Option<ProtoVertexId>,
    ) -> Result<Self> {
        let mut vd = Self {
            pc1: *pc1,
            pc2: *pc2,
            position,
            tol,
            gap,
            proto_vertex: owner,
            final_vtx1: None,
            final_vtx2: None,
            crossing_sine: -1.0,
            first_placement: None,
            first_curve: None,
            absorbed_hull: Vec::new(),
        };

        if !vd.pc1.are_parameters_set() {
            ignore_status(vd.pc1.compute_point_parameters(k, geometry, position))?;
        }
        if !vd.pc2.are_parameters_set() {
            ignore_status(vd.pc2.compute_point_parameters(k, geometry, position))?;
        }

        if gap <= 0.0 {
            let pt1 = obj_point(k, &vd.pc1, geometry)?;
            let pt2 = obj_point(k, &vd.pc2, geometry)?;
            vd.gap = pt1.distance(pt2);
            vd.gap = max2(vd.gap, pc1.gap_3d(k, geometry, None)?);
            vd.gap = max2(vd.gap, pc2.gap_3d(k, geometry, None)?);
        }
        Ok(vd)
    }

    /// The position (`GetPosition`).
    #[must_use]
    pub const fn position(&self) -> Point3 {
        self.position
    }

    /// Not upstream (WR-410, tangency-labs/solidcore#1059; the SolidCraft fork's
    /// `SmVertexDefinition::PlaceByCrossing`): places this definition by another
    /// intersection curve's crossing of its source edge (`pc1`, `pc2`, the crossing's
    /// classifications against this definition's objects, at angle
    /// `asin(crossing_sine)`) when that crossing fixes it more closely. The first
    /// crossing placed it. A later one replaces the point parameters on the source
    /// objects (an edge's t, a face's uv), the gap, the position and the crossing angle
    /// only when its points lie within both objects' tolerance of each other (a farther
    /// one is another point the proto vertex took in), its misplacement bound
    /// (`crossing_error`) is smaller, and the stretch between the first placement and the
    /// new one is certified one point ([`crossing_stretch_certified`]): every point of the
    /// source edge between its two parameters lies within this definition's tolerance of
    /// the other object and of the first crossing's curve (`curve`, the intersection curve
    /// and its parameter at this end, kept from the first crossing), by bounds over the
    /// whole piece with rounding counted. Without
    /// that certificate the definition keeps its placement (the first crossing's, as
    /// upstream). Each later crossing is certified from the first placement, so which one
    /// places the definition last does not depend on the order they arrive in (the least
    /// misplacement bound among the certified ones; of equal ones, the first). Returns
    /// whether it was placed again.
    ///
    /// # Errors
    ///
    /// Those of `FindObjPoint3d`, the objects' zone tolerances and
    /// [`find_position_from_pcs`].
    pub fn place_by_crossing(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
        crossing_sine: f64,
        curve: Option<(O, f64)>,
    ) -> Result<bool> {
        if self.crossing_sine < 0.0 {
            // The first crossing to reach this VertexDefinition placed it.
            self.crossing_sine = crossing_sine;
            self.first_placement = Some([self.pc1, self.pc2]);
            self.first_curve = curve;
            return Ok(false);
        }

        // This VertexDefinition with the crossing's point parameters.
        let mut pcs = [self.pc1, self.pc2];
        for (pc, new) in pcs.iter_mut().zip([pc1, pc2]) {
            if !new.are_parameters_set() {
                return Ok(false);
            }
            match pc.point_class() {
                PointClassType::Edge => pc.set_t_param(new.t_param(), false),
                PointClassType::Face => pc.set_uv_param(new.uv_param()),
                _ => continue,
            }
            pc.set_gap_3d(new.gap_3d(k, geometry, None)?);
        }
        let [mut pc1, mut pc2] = pcs;
        let pt1 = pc1.find_obj_point_3d(k, geometry)?;
        let pt2 = pc2.find_obj_point_3d(k, geometry)?;
        let gap = max2(
            max2(pt1.distance(pt2), pc1.gap_3d(k, geometry, None)?),
            pc2.gap_3d(k, geometry, None)?,
        );

        // Points farther apart than both source objects' tolerances are no point on
        // both: the crossing is another point that the ProtoVertex's containment radius
        // took in (the far end of a short curve it absorbs), not this one placed again.
        let obj_tol = max2(
            pc1.obj_zone_tol_3d(k, geometry)?,
            pc2.obj_zone_tol_3d(k, geometry)?,
        );
        // (upstream's `!(gap <= tol)` and `!(new < old)`: NaN keeps the placement)
        if !matches!(
            gap.partial_cmp(&obj_tol),
            Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
        ) {
            return Ok(false);
        }
        let (new_error, old_error) = (
            crossing_error(gap, crossing_sine),
            crossing_error(self.gap, self.crossing_sine),
        );
        if new_error.partial_cmp(&old_error) != Some(core::cmp::Ordering::Less) {
            return Ok(false);
        }

        let position = find_position_from_pcs(k, geometry, &mut pc1, &mut pc2)?;
        // Both points lie on both objects, but they are one point only where the objects
        // stay within the same tolerance of each other all the way between them; else the
        // crossing is another point the proto vertex took in, and moving there would carry
        // the proto vertex away from what placed it.
        let Some(first) = self.first_placement else {
            return Ok(false);
        };
        if !crossing_stretch_certified(
            k,
            geometry,
            &first,
            self.first_curve,
            [&pc1, &pc2],
            self.tolerance(k),
        )? {
            return Ok(false);
        }
        self.pc1 = pc1;
        self.pc2 = pc2;
        self.position = position.position;
        self.tol = max2(self.tol, position.deviation);
        self.gap = gap;
        self.crossing_sine = crossing_sine;
        Ok(true)
    }

    /// The tolerance (`GetTolerance`, as an `SmZoneTol3d`).
    #[must_use]
    pub fn tolerance(&self, k: &Kernel) -> f64 {
        zone_tol3d(k, self.tol)
    }

    /// The gap between the two source objects (`GetGap`).
    #[must_use]
    pub const fn gap(&self) -> f64 {
        self.gap
    }

    /// The vertex made in B-rep `which` (1, else 2) by the imprint
    /// (`GetFinalVertex`).
    #[must_use]
    pub const fn final_vertex(&self, which: i32) -> Option<O> {
        if which == 1 {
            self.final_vtx1
        } else {
            self.final_vtx2
        }
    }

    /// The owning proto vertex (`GetProtoVertex`).
    #[must_use]
    pub const fn proto_vertex(&self) -> Option<ProtoVertexId> {
        self.proto_vertex
    }

    /// The B-rep 1 topology object (`GetBrep1Topo`: the classification object when
    /// it is topology).
    #[must_use]
    pub fn brep1_topo(&self) -> Option<O> {
        as_topology(self.pc1.object())
    }

    /// The B-rep 2 topology object (`GetBrep2Topo`).
    #[must_use]
    pub fn brep2_topo(&self) -> Option<O> {
        as_topology(self.pc2.object())
    }

    /// The B-rep 1 classification type (`GetBrep1TopoType`).
    #[must_use]
    pub const fn brep1_topo_type(&self) -> PointClassType {
        self.pc1.point_class()
    }

    /// The B-rep 2 classification type (`GetBrep2TopoType`).
    #[must_use]
    pub const fn brep2_topo_type(&self) -> PointClassType {
        self.pc2.point_class()
    }

    /// The topology object in B-rep `which` (1, else 2) (`GetSrcTopo`).
    #[must_use]
    pub fn src_topo(&self, which: i32) -> Option<O> {
        if which == 1 {
            self.brep1_topo()
        } else {
            self.brep2_topo()
        }
    }

    /// The classification type in B-rep `which` (`GetSrcTopo_TYPE`).
    #[must_use]
    pub const fn src_topo_type(&self, which: i32) -> PointClassType {
        if which == 1 {
            self.brep1_topo_type()
        } else {
            self.brep2_topo_type()
        }
    }

    /// The edge parameter in B-rep `which` (`GetBrepTParam`).
    #[must_use]
    pub const fn brep_t_param(&self, which: i32) -> f64 {
        if which == 1 {
            self.pc1.t_param()
        } else {
            self.pc2.t_param()
        }
    }

    /// The face parameters in B-rep `which` (`GetBrepUVParam`).
    #[must_use]
    pub const fn brep_uv_param(&self, which: i32) -> Point2 {
        if which == 1 {
            self.pc1.uv_param()
        } else {
            self.pc2.uv_param()
        }
    }

    /// The classification in B-rep `which` (`GetBrepPointClassif`).
    #[must_use]
    pub const fn brep_point_classif(&self, which: i32) -> &PointClassification<O> {
        if which == 1 {
            &self.pc1
        } else {
            &self.pc2
        }
    }

    /// Mutable access to the classification in B-rep `which` (`GetBrepPointClassif`).
    pub fn brep_point_classif_mut(&mut self, which: i32) -> &mut PointClassification<O> {
        if which == 1 {
            &mut self.pc1
        } else {
            &mut self.pc2
        }
    }

    /// Not upstream (WR-306): the tolerance this definition needs at `position` to keep
    /// every absorbed hull point ([`VertexDefinition::absorb_hull`]) covered, by the same
    /// rounded distance `resolve_proto_edge` took it in by,
    /// `|p - position| * (1 + gamma(8)) + 2 * err`, and never below its tolerance now;
    /// `None` when a distance is not finite.
    #[must_use]
    pub fn covering_tolerance(&self, k: &Kernel, position: Point3) -> Option<f64> {
        let mut needed = self.tolerance(k);
        for &(p, err) in &self.absorbed_hull {
            let d = p.distance(position) * (1.0 + gamma(8)) + 2.0 * err;
            if !d.is_finite() {
                return None;
            }
            needed = max2(needed, d);
        }
        Some(needed)
    }

    /// Sets the position (`SetPosition`). Not upstream (WR-306): the tolerance then grows,
    /// never shrinks, to [`VertexDefinition::covering_tolerance`] at the new position, so
    /// each curve the definition absorbed stays within its tolerance.
    ///
    /// # Errors
    ///
    /// [`SolidError::Failed`] (a topology failure) when a hull point's distance from
    /// `position` is not finite; the definition is left unchanged then.
    pub fn set_position(&mut self, k: &Kernel, position: Point3) -> Result<()> {
        let needed = self.covering_tolerance(k, position).ok_or_else(|| {
            SolidError::topology(
                OP_SET_POSITION,
                TopologyFailure::AbsorbedCurveAtNoFiniteDistance,
            )
        })?;
        self.position = position;
        if needed > self.tolerance(k) {
            self.tol = needed;
        }
        Ok(())
    }

    /// Not upstream (WR-306): records the certain hull points (each with the error bound
    /// of its coordinates) of a curve this definition absorbed, for
    /// [`VertexDefinition::set_position`] to keep covered.
    pub fn absorb_hull(&mut self, points: impl IntoIterator<Item = (Point3, f64)>) {
        self.absorbed_hull.extend(points);
    }

    /// Not upstream (WR-306): the hull points recorded by
    /// [`VertexDefinition::absorb_hull`].
    #[must_use]
    pub fn absorbed_hull(&self) -> &[(Point3, f64)] {
        &self.absorbed_hull
    }

    /// Not upstream (WR-306): takes over the hull points `points` of a definition combined
    /// into this one, and grows the tolerance, never shrinks it, to
    /// [`VertexDefinition::covering_tolerance`] where the definition is, so the curves that
    /// definition absorbed stay within this one's tolerance.
    ///
    /// # Errors
    ///
    /// [`SolidError::Failed`] (a topology failure) when a point's distance from the
    /// position is not finite; the definition is left unchanged then.
    pub fn take_over_hull(&mut self, k: &Kernel, points: &[(Point3, f64)]) -> Result<()> {
        let kept = self.absorbed_hull.len();
        self.absorbed_hull.extend_from_slice(points);
        let position = self.position;
        if let Err(e) = self.set_position(k, position) {
            self.absorbed_hull.truncate(kept);
            return Err(e);
        }
        Ok(())
    }

    /// Sets the owning proto vertex (`SetProtoVertex`).
    pub fn set_proto_vertex(&mut self, pv: Option<ProtoVertexId>) {
        self.proto_vertex = pv;
    }

    /// Sets the vertex of B-rep `which` (1, else 2) (`SetFinalVertex`).
    pub fn set_final_vertex(&mut self, which: i32, vertex: Option<O>) {
        if which == 1 {
            self.final_vtx1 = vertex;
        } else {
            self.final_vtx2 = vertex;
        }
    }

    /// Sets the classification object of B-rep `which` (`SetSrcTopo`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `which` other than 1 and 2.
    pub fn set_src_topo(&mut self, which: i32, t: PointClassType, topo: Option<O>) -> Result<()> {
        if which == 1 {
            self.pc1.set_class_object(t, topo);
        } else if which == 2 {
            self.pc2.set_class_object(t, topo);
        } else {
            return Err(SolidError::topology(
                OP_SRC_TOPO,
                TopologyFailure::BrepSideNotOneOrTwo { which },
            ));
        }
        Ok(())
    }

    /// Recomputes both classifications' parameters at the position
    /// (`SetPointParameters`; statuses ignored upstream, passed on as the fork does,
    /// bcc6f01).
    ///
    /// # Errors
    ///
    /// Those of the classifications.
    pub fn set_point_parameters(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
    ) -> Result<()> {
        self.pc1
            .compute_point_parameters(k, geometry, self.position)?;
        self.pc2
            .compute_point_parameters(k, geometry, self.position)?;
        Ok(())
    }
}

// ---- SmEdgeDefinition -------------------------------------------------------------------

/// The `SM_TYPE` of a source topology object, as an [`EdgeDefinition`] caches it
/// (`m_pSrcTopo1_TYPE`).
fn src_type<O: ClassObject>(topo: O) -> ClassObjectKind {
    topo.class_kind()
}

/// One curve intersection between a topology object of each B-rep
/// (`SmEdgeDefinition`). Its curves are handles to the caller's geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeDefinition<O> {
    /// `m_pXSectCurve3d`.
    xsect_curve_3d: Option<O>,
    /// `m_pXSectUVCurve1`.
    xsect_uv_curve1: Option<O>,
    /// `m_pXSectUVCurve2`.
    xsect_uv_curve2: Option<O>,
    /// `m_pXSectInterval`.
    xsect_interval: Extent1d,
    /// `m_pSrcTopo1` (never null: set by the constructor and by `set_src_topo`).
    src_topo1: O,
    /// `m_pSrcTopo2` (never null, as `src_topo1`).
    src_topo2: O,
    /// `m_pSrcTopo1_TYPE`.
    src_topo1_type: ClassObjectKind,
    /// `m_pSrcTopo2_TYPE`.
    src_topo2_type: ClassObjectKind,
    /// `m_pProtoEdge`.
    proto_edge: Option<ProtoEdgeId>,
    /// `m_dTol`.
    tol: f64,
    /// `m_pFinalEdge1`.
    final_edge1: Option<O>,
    /// `m_pFinalEdge2`.
    final_edge2: Option<O>,
}

impl<O: ClassObject> EdgeDefinition<O> {
    /// `SmEdgeDefinition(pXSectCurve3d, pXSectInterval, pSrcTopo1, pSrcTopo2, dTol,
    /// pXSectUVCurve1, pXSectUVCurve2, pMyProtoEdge)`; the source objects' types are
    /// cached.
    #[must_use]
    pub fn new(
        xsect_curve_3d: Option<O>,
        xsect_interval: Extent1d,
        src_topo1: O,
        src_topo2: O,
        tol: f64,
        xsect_uv_curve1: Option<O>,
        xsect_uv_curve2: Option<O>,
        proto_edge: Option<ProtoEdgeId>,
    ) -> Self {
        Self {
            xsect_curve_3d,
            xsect_uv_curve1,
            xsect_uv_curve2,
            xsect_interval,
            src_topo1,
            src_topo2,
            src_topo1_type: src_type(src_topo1),
            src_topo2_type: src_type(src_topo2),
            proto_edge,
            tol,
            final_edge1: None,
            final_edge2: None,
        }
    }

    /// The copy constructor `SmEdgeDefinition(const SmEdgeDefinition&)`: everything
    /// but the final edges (left null).
    #[must_use]
    pub fn copy_definition(&self) -> Self {
        Self {
            final_edge1: None,
            final_edge2: None,
            ..self.clone()
        }
    }

    /// The destructor `~SmEdgeDefinition`: the curves it deletes, those not consumed
    /// by an imprint (the 3d and first UV curve when B-rep 1's source is an edge, the
    /// second UV curve when B-rep 2's is).
    #[must_use]
    pub fn destroy(mut self) -> Vec<O> {
        let mut deleted = Vec::new();
        if self.src_topo_type(1) == PointClassType::Edge {
            deleted.extend(self.xsect_curve_3d.take());
            deleted.extend(self.xsect_uv_curve1.take());
        }
        if self.src_topo_type(2) == PointClassType::Edge {
            deleted.extend(self.xsect_uv_curve2.take());
        }
        deleted
    }

    /// Deletes all three curves (`Destruct`): the handles deleted.
    pub fn destruct(&mut self) -> Vec<O> {
        let mut deleted = Vec::new();
        deleted.extend(self.xsect_curve_3d.take());
        deleted.extend(self.xsect_uv_curve1.take());
        deleted.extend(self.xsect_uv_curve2.take());
        deleted
    }

    /// The 3d intersection curve (`GetXSectCurve3d`).
    #[must_use]
    pub const fn xsect_curve_3d(&self) -> Option<O> {
        self.xsect_curve_3d
    }

    /// The curve interval (`GetXSectInterval`).
    #[must_use]
    pub const fn xsect_interval(&self) -> Extent1d {
        self.xsect_interval
    }

    /// The tolerance (`GetTolerance`, as an `SmZoneTol3d`).
    #[must_use]
    pub fn tolerance(&self, k: &Kernel) -> f64 {
        zone_tol3d(k, self.tol)
    }

    /// The raw tolerance `m_dTol` (read directly by `Imprint`).
    #[must_use]
    pub const fn raw_tolerance(&self) -> f64 {
        self.tol
    }

    /// The UV curve on B-rep `which`'s face (1, else 2) (`GetXSectUVCurve`).
    #[must_use]
    pub const fn xsect_uv_curve(&self, which: i32) -> Option<O> {
        if which == 1 {
            self.xsect_uv_curve1
        } else {
            self.xsect_uv_curve2
        }
    }

    /// The source object in B-rep `which` (`GetSrcTopo`).
    #[must_use]
    pub const fn src_topo(&self, which: i32) -> Option<O> {
        if which == 1 {
            Some(self.src_topo1)
        } else {
            Some(self.src_topo2)
        }
    }

    /// The classification type of the cached source type in B-rep `which`
    /// (`GetSrcTopo_TYPE`): vertex, edge, face, region, else unknown.
    #[must_use]
    pub fn src_topo_type(&self, which: i32) -> PointClassType {
        let t = if which == 1 {
            self.src_topo1_type
        } else {
            self.src_topo2_type
        };
        match t {
            ClassObjectKind::Vertex => PointClassType::Vertex,
            ClassObjectKind::Edge => PointClassType::Edge,
            ClassObjectKind::Face => PointClassType::Face,
            ClassObjectKind::Region => PointClassType::Region,
            _ => PointClassType::Unknown,
        }
    }

    /// The owning proto edge (`GetProtoEdge`).
    #[must_use]
    pub const fn proto_edge(&self) -> Option<ProtoEdgeId> {
        self.proto_edge
    }

    /// The edge made in B-rep `which` (`GetFinalEdge`).
    #[must_use]
    pub const fn final_edge(&self, which: i32) -> Option<O> {
        if which == 1 {
            self.final_edge1
        } else {
            self.final_edge2
        }
    }

    /// Sets the edge made in B-rep `which` (1, else 2) (the imprint's write of
    /// `m_pFinalEdge`).
    pub fn set_final_edge(&mut self, which: i32, edge: Option<O>) {
        if which == 1 {
            self.final_edge1 = edge;
        } else {
            self.final_edge2 = edge;
        }
    }

    /// Sets the 3d curve (`SetXSectCurve3d`).
    pub fn set_xsect_curve_3d(&mut self, curve: Option<O>) {
        self.xsect_curve_3d = curve;
    }

    /// Sets the UV curve of B-rep `which` (1, else 2) (`SetXSectCurveUV`).
    pub fn set_xsect_curve_uv(&mut self, which: i32, curve: Option<O>) {
        if which == 1 {
            self.xsect_uv_curve1 = curve;
        } else {
            self.xsect_uv_curve2 = curve;
        }
    }

    /// Sets the owning proto edge (`SetProtoEdge`).
    pub fn set_proto_edge(&mut self, pe: Option<ProtoEdgeId>) {
        self.proto_edge = pe;
    }

    /// Sets the curve interval (`SetXSectInterval(const SmExtent1d&)`).
    pub fn set_xsect_interval(&mut self, interval: Extent1d) {
        self.xsect_interval = interval;
    }

    /// Sets the source object of B-rep `which` (1, else 2) and caches its type
    /// (`SetSrcTopo`; the classification type is unused).
    pub fn set_src_topo(&mut self, which: i32, _t: PointClassType, topo: O) {
        if which == 1 {
            self.src_topo1 = topo;
            self.src_topo1_type = src_type(topo);
        } else {
            self.src_topo2 = topo;
            self.src_topo2_type = src_type(topo);
        }
    }

    /// The 3d curve's point at the start (or end) of the interval
    /// (`GetEndPosition(SmBoolean, SmPoint3d&)`; the evaluation status is ignored
    /// upstream, passed on as the fork does, bcc6f01).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a curve; [`SolidError::Unported`] from
    /// the evaluation.
    pub fn end_position(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        at_start: bool,
    ) -> Result<Point3> {
        // pick end param
        let param = if at_start {
            self.xsect_interval.min()
        } else {
            self.xsect_interval.max()
        };

        // evaluate Pos3d (passed on, fork bcc6f01)
        curve_of(geometry, self.xsect_curve_3d)?.evaluate_point(k, param)
    }

    /// The point and/or tangent of the 3d curve at `param` (`Evaluate`): `None` for
    /// what was not asked.
    ///
    /// # Errors
    ///
    /// Those of the curve's evaluation; [`SolidError::InvalidInput`] without a curve.
    pub fn evaluate(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        param: f64,
        want_pos: bool,
        want_tan: bool,
    ) -> Result<(Option<Point3>, Option<Vec3>)> {
        if !want_pos && !want_tan {
            return Ok((None, None));
        }

        let curve = curve_of(geometry, self.xsect_curve_3d)?;
        if !want_tan {
            return Ok((Some(curve.evaluate_point(k, param)?), None));
        }

        // Caller wants the deriv vector.
        let pt_vec = curve.evaluate(k, param, 1, true, true)?;
        let pos = pt_vec
            .first()
            .copied()
            .ok_or(dangling("evaluation result"))?;
        let tan = pt_vec
            .get(1)
            .copied()
            .ok_or(dangling("evaluation result"))?;
        Ok((want_pos.then_some(pos), Some(tan)))
    }

    /// [`EdgeDefinition::evaluate`] at the middle of the interval (`EvaluateMid`).
    ///
    /// # Errors
    ///
    /// Those of [`EdgeDefinition::evaluate`].
    pub fn evaluate_mid(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        want_pos: bool,
        want_tan: bool,
    ) -> Result<(Option<Point3>, Option<Vec3>)> {
        let param = self.xsect_interval.mid();
        self.evaluate(k, geometry, param, want_pos, want_tan)
    }

    /// Reverses the parameterisation of the 3d and UV curves, each updating the
    /// interval in turn (`ReverseOrientation`).
    ///
    /// # Errors
    ///
    /// Those of [`Curve::reverse_parameterization`] (the first failure stops);
    /// [`SolidError::InvalidInput`] for an unknown curve handle.
    pub fn reverse_orientation(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoGeometry<O>,
    ) -> Result<()> {
        for c in [
            self.xsect_curve_3d,
            self.xsect_uv_curve1,
            self.xsect_uv_curve2,
        ]
        .into_iter()
        .flatten()
        {
            let curve = geometry
                .curve_mut(c)
                .ok_or(dangling("curve handle is unknown"))?;
            self.xsect_interval = curve.reverse_parameterization(k, &self.xsect_interval)?;
        }
        Ok(())
    }

    /// Sets the interval from two values, reversing the curves first when
    /// `min > max` (`SetXSectInterval(double, double)`; the reversal's status is
    /// ignored).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the reversal.
    pub fn set_xsect_interval_min_max(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoGeometry<O>,
        min: f64,
        max: f64,
    ) -> Result<()> {
        // fork bcc6f01: returns the reversal's and the interval's statuses
        if min <= max {
            self.xsect_interval.set_min_max(min, max)?;
        } else {
            self.reverse_orientation(k, geometry)?;
            self.xsect_interval.set_min_max(max, min)?;
        }
        Ok(())
    }

    /// Reparameterises the curves to `new_domain`, reversing them first when asked
    /// (`Reparameterize`; the reversal's status is ignored). Nothing is edited when
    /// the domain is already the interval (to the scaled zero).
    ///
    /// # Errors
    ///
    /// Those of [`Curve::edit_parameterization`]; [`SolidError::InvalidInput`]
    /// without a 3d curve.
    pub fn reparameterize(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoGeometry<O>,
        new_domain: &Extent1d,
        reverse: bool,
    ) -> Result<()> {
        if reverse {
            self.reverse_orientation(k, geometry)?;
        }

        if new_domain.are_same(&self.xsect_interval) {
            return Ok(());
        }

        self.xsect_interval = *new_domain;

        let c3 = self
            .xsect_curve_3d
            .and_then(|c| geometry.curve_mut(c))
            .ok_or(dangling("curve handle is null or unknown"))?;
        c3.edit_parameterization(k, new_domain)?;

        for c in [self.xsect_uv_curve1, self.xsect_uv_curve2]
            .into_iter()
            .flatten()
        {
            geometry
                .curve_mut(c)
                .ok_or(dangling("curve handle is unknown"))?
                .edit_parameterization(k, new_domain)?;
        }
        Ok(())
    }

    /// Whether `position` is an interior point of the definition near its start (or
    /// end) (`IsInteriorPoint`): farther than `tol` from that end, dropping onto the
    /// 3d curve (minimising, guessed at that end) within `tol` of it, in that half of
    /// the interval, with the drop point farther than `tol` from the end. The parameter is the drop
    /// parameter when the result is `true` (or the end parameter when the position
    /// is at the end).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a curve; [`SolidError::Unported`] from
    /// the drop.
    pub fn is_interior_point(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        position: Point3,
        tol: f64,
        at_start: bool,
    ) -> Result<(bool, f64)> {
        let guess = if at_start {
            self.xsect_interval.min()
        } else {
            self.xsect_interval.max()
        };
        let mut param = guess;

        // fork bcc6f01: no end position, no evidence
        let Some(end_pos) = eval_point_or_no(k, curve_of(geometry, self.xsect_curve_3d)?, guess)?
        else {
            return Ok((false, guess));
        };
        let dist = position.distance(end_pos);
        if dist < tol {
            return Ok((false, param));
        }

        // Drop the point to our 3d curve.
        let dropped = ignored(curve_of(geometry, self.xsect_curve_3d)?.drop_point(
            k,
            &self.xsect_interval,
            position,
            None,
            tol,
            Some(guess),
            SolverOperation::Minimize,
        ))?;
        let Some(d) = dropped else {
            return Ok((false, param));
        };
        param = d.parameter;
        if !d.success {
            return Ok((false, param));
        }

        // SM_SO_MINIMIZE succeeds for any converged closest point, however far away.
        // A true result lets UpdateEdges() trim this EdgeDef to the parameter, so the
        // position must actually lie on the curve. (`!(dDropDist <= sTol3d)`: a NaN
        // distance is rejected.)
        if d.distance.is_nan() || d.distance > tol {
            return Ok((false, param));
        }

        // Sometimes for very short Edges, the point will be at or near the far end.
        if at_start && param > self.xsect_interval.mid() {
            return Ok((false, param));
        }
        if !at_start && param < self.xsect_interval.mid() {
            return Ok((false, param));
        }

        // See whether the split point is interior to this EdgeDef (in 3d).
        let edge_pt = match self.evaluate(k, geometry, param, true, false) {
            Ok((Some(p), _)) => p,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            // fork bcc6f01: no value, no evidence
            Err(_) => return Ok((false, param)),
            Ok((None, _)) => Vec3::UNDEF,
        };
        let dist = edge_pt.distance(end_pos);
        Ok((dist > tol, param))
    }
}

// ---- SmProtoVertex ----------------------------------------------------------------------

/// A set of vertex definitions at about the same position (`SmProtoVertex`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoVertex {
    /// `m_sPosition`.
    position: Point3,
    /// `m_dTol`.
    tol: f64,
    /// `m_sVertexDefinitions`.
    vertex_defs: Vec<VertexDefId>,
    /// `m_sProtoEdges` (a closed proto edge is listed twice).
    proto_edges: Vec<ProtoEdgeId>,
    /// The SMLib fork's `m_bAbsorbedProtoEdge`: a degenerate proto edge of ours was dropped,
    /// so our vertices stand for its curve (WR-306, opt-in).
    absorbed_proto_edge: bool,
    /// Not upstream (#16637): the computed points (positions and object points) of the
    /// vertex definitions combined away at this proto vertex, the attached curve ends
    /// evaluated when it combined them, and the points of the definitions that remained
    /// when it moved, which every write of its position covers
    /// ([`ProtoVertex::set_position`]).
    absorbed_points: Vec<Point3>,
    /// Not upstream (#16637): a point to absorb here (of a definition combined away or an
    /// attached curve end) could not be computed, so `absorbed_points` is incomplete and
    /// `resolve_proto_vertex` leaves the position where upstream puts it.
    absorbed_unbounded: bool,
}

impl ProtoVertex {
    /// Whether a degenerate proto edge of this proto vertex was dropped (the fork's
    /// `AbsorbedProtoEdge`, WR-306).
    #[must_use]
    pub const fn absorbed_proto_edge(&self) -> bool {
        self.absorbed_proto_edge
    }

    /// Not upstream (#16637): the computed points (positions and object points) of the
    /// vertex definitions combined away at this proto vertex, the attached curve ends
    /// evaluated when it combined them, and, when it moved, the points of the definitions
    /// that remained and the attached curve ends that trace to them.
    #[must_use]
    pub fn absorbed_points(&self) -> &[Point3] {
        &self.absorbed_points
    }

    /// Not upstream (#16637): whether a point to absorb here could not be computed (so
    /// [`ProtoVertex::absorbed_points`] is incomplete).
    #[must_use]
    pub const fn absorbed_unbounded(&self) -> bool {
        self.absorbed_unbounded
    }

    /// Not upstream (#16637): records the computed points of vertex definitions combined
    /// away at this proto vertex, and whether every point of them was computed. It does not
    /// change the tolerance: the caller places the vertex ([`ProtoVertex::set_position`])
    /// or covers the points where it is ([`ProtoVertex::cover_absorbed`]).
    pub fn absorb(&mut self, points: &[Point3], bounded: bool) {
        self.absorbed_points.extend_from_slice(points);
        self.absorbed_unbounded = self.absorbed_unbounded || !bounded;
    }

    /// Not upstream (#16637): widens the tolerance to reach every absorbed point from the
    /// position: the largest computed distance, rounded up by `γ_4` for the distance's own
    /// rounding. Distances between computed points only, no stored gap and no estimate.
    /// A non-finite distance (a non-finite position, or a distance that overflows) never
    /// widens, so the tolerance stays finite; that point is then not covered, so it marks
    /// the absorbed points unbounded ([`ProtoVertex::absorbed_unbounded`]: no later move).
    pub fn cover_absorbed(&mut self) {
        use crate::nurbs::math::gamma;
        let mut reach = 0.0_f64;
        for &at in &self.absorbed_points {
            let r = self.position.distance(at) * (1.0 + gamma(4));
            if !r.is_finite() {
                self.absorbed_unbounded = true;
            } else if r > reach {
                reach = r;
            }
        }
        if reach > self.tol {
            self.tol = reach;
        }
    }

    /// Marks a degenerate proto edge of this proto vertex as dropped (the fork's
    /// `SetAbsorbedProtoEdge`, WR-306).
    pub fn set_absorbed_proto_edge(&mut self) {
        self.absorbed_proto_edge = true;
    }

    /// The position (`GetPosition`).
    #[must_use]
    pub const fn position(&self) -> Point3 {
        self.position
    }

    /// Sets the position (through `GetPosition`'s reference). Not upstream (#16637): the
    /// tolerance then grows to cover every absorbed point from there
    /// ([`ProtoVertex::cover_absorbed`]); with none absorbed it is upstream's write. A
    /// vertex that absorbed points is not moved to a non-finite position, from which no
    /// distance to them could be measured: it stays where it is, still covering them.
    pub fn set_position(&mut self, position: Point3) {
        let finite = position.x.is_finite() && position.y.is_finite() && position.z.is_finite();
        if finite || self.absorbed_points.is_empty() {
            self.position = position;
        }
        self.cover_absorbed();
    }

    /// The tolerance (`GetTolerance`, as an `SmZoneTol3d`).
    #[must_use]
    pub fn tolerance(&self, k: &Kernel) -> f64 {
        zone_tol3d(k, self.tol)
    }

    /// The raw tolerance `m_dTol`.
    #[must_use]
    pub const fn raw_tolerance(&self) -> f64 {
        self.tol
    }

    /// Sets the raw tolerance `m_dTol` (the writes of `AddVertexDefinition`).
    pub fn set_raw_tolerance(&mut self, tol: f64) {
        self.tol = tol;
    }

    /// `GetNumVertexDefinitions`.
    #[must_use]
    pub fn num_vertex_definitions(&self) -> usize {
        self.vertex_defs.len()
    }

    /// `GetNumProtoEdges`.
    #[must_use]
    pub fn num_proto_edges(&self) -> usize {
        self.proto_edges.len()
    }

    /// The vertex definitions (`GetVertexDefinitions`).
    #[must_use]
    pub fn vertex_definitions(&self) -> &[VertexDefId] {
        &self.vertex_defs
    }

    /// Mutable access to the vertex definitions (`GetVertexDefinitions`).
    pub fn vertex_definitions_mut(&mut self) -> &mut Vec<VertexDefId> {
        &mut self.vertex_defs
    }

    /// The connected proto edges (`GetProtoEdges`).
    #[must_use]
    pub fn proto_edges(&self) -> &[ProtoEdgeId] {
        &self.proto_edges
    }

    /// Mutable access to the connected proto edges (`GetProtoEdges`).
    pub fn proto_edges_mut(&mut self) -> &mut Vec<ProtoEdgeId> {
        &mut self.proto_edges
    }

    /// Forgets the vertex definitions (`ClearVertexDefinitions`).
    pub fn clear_vertex_definitions(&mut self) {
        self.vertex_defs.clear();
    }

    /// Lists `pe`, even when already listed (`AddProtoEdge`, design decision DD1).
    pub fn add_proto_edge(&mut self, pe: ProtoEdgeId) {
        self.proto_edges.push(pe);
    }

    /// Unlists the first entry `vd` (`RemoveVertexDefinition`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `vd` is not listed.
    pub fn remove_vertex_definition(&mut self, vd: VertexDefId) -> Result<()> {
        match self.vertex_defs.iter().position(|&x| x == vd) {
            Some(i) => {
                self.vertex_defs.remove(i);
                Ok(())
            }
            None => Err(SolidError::topology(OP, TopologyFailure::NotOnOwnerList)),
        }
    }

    /// Unlists the first entry `pe` (`RemoveProtoEdge`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `pe` is not listed.
    pub fn remove_proto_edge(&mut self, pe: ProtoEdgeId) -> Result<()> {
        match self.proto_edges.iter().position(|&x| x == pe) {
            Some(i) => {
                self.proto_edges.remove(i);
                Ok(())
            }
            None => Err(SolidError::topology(OP, TopologyFailure::NotOnOwnerList)),
        }
    }
}

// ---- SmProtoEdge ------------------------------------------------------------------------

/// A set of edge definitions along about the same curve between two proto vertices
/// (`SmProtoEdge`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoEdge {
    /// `m_pXSectInterval`.
    xsect_interval: Extent1d,
    /// `m_pStartPV`.
    start_pv: Option<ProtoVertexId>,
    /// `m_pEndPV`.
    end_pv: Option<ProtoVertexId>,
    /// `m_sEdgeDefs`.
    edge_defs: Vec<EdgeDefId>,
    /// `m_dTol`.
    tol: f64,
    /// `m_sOverlappingPEs`.
    overlapping_pes: Vec<ProtoEdgeId>,
    /// `m_sCoincPFs`.
    coinc_pfs: Vec<ProtoFaceId>,
}

impl ProtoEdge {
    /// The interval (`GetXSectInterval`).
    #[must_use]
    pub const fn xsect_interval(&self) -> Extent1d {
        self.xsect_interval
    }

    /// The tolerance (`GetTolerance`, as an `SmZoneTol3d`).
    #[must_use]
    pub fn tolerance(&self, k: &Kernel) -> f64 {
        zone_tol3d(k, self.tol)
    }

    /// The raw tolerance `m_dTol`.
    #[must_use]
    pub const fn raw_tolerance(&self) -> f64 {
        self.tol
    }

    /// Sets the raw tolerance `m_dTol` (the write of `CombineProtoEdge`).
    pub fn set_raw_tolerance(&mut self, tol: f64) {
        self.tol = tol;
    }

    /// The start proto vertex (`GetStartProtoVertex`).
    #[must_use]
    pub const fn start_proto_vertex(&self) -> Option<ProtoVertexId> {
        self.start_pv
    }

    /// The end proto vertex (`GetEndProtoVertex`).
    #[must_use]
    pub const fn end_proto_vertex(&self) -> Option<ProtoVertexId> {
        self.end_pv
    }

    /// The overlapping proto edges (`GetOverlappingPEs`).
    #[must_use]
    pub fn overlapping_pes(&self) -> &[ProtoEdgeId] {
        &self.overlapping_pes
    }

    /// Empties the overlapping proto edges (`GetOverlappingPEs().ReSet()`).
    pub fn clear_overlapping_pes(&mut self) {
        self.overlapping_pes.clear();
    }

    /// `GetNumEdgeDefinitions`.
    #[must_use]
    pub fn num_edge_definitions(&self) -> usize {
        self.edge_defs.len()
    }

    /// The edge definitions (`GetEdgeDefinitions`).
    #[must_use]
    pub fn edge_definitions(&self) -> &[EdgeDefId] {
        &self.edge_defs
    }

    /// Mutable access to the edge definitions (`GetEdgeDefinitions`).
    pub fn edge_definitions_mut(&mut self) -> &mut Vec<EdgeDefId> {
        &mut self.edge_defs
    }

    /// The coincident proto faces this edge bounds (`GetCoincProtoFaces`).
    #[must_use]
    pub fn coinc_proto_faces(&self) -> &[ProtoFaceId] {
        &self.coinc_pfs
    }

    /// The coincident proto faces, for the edits upstream makes to `m_sCoincPFs`.
    pub fn coinc_proto_faces_mut(&mut self) -> &mut Vec<ProtoFaceId> {
        &mut self.coinc_pfs
    }

    /// Sets the interval (`SetXSectInterval`).
    pub fn set_xsect_interval(&mut self, interval: Extent1d) {
        self.xsect_interval = interval;
    }

    /// Sets the start proto vertex (`SetStartProtoVertex`).
    pub fn set_start_proto_vertex(&mut self, pv: Option<ProtoVertexId>) {
        self.start_pv = pv;
    }

    /// Sets the end proto vertex (`SetEndProtoVertex`).
    pub fn set_end_proto_vertex(&mut self, pv: Option<ProtoVertexId>) {
        self.end_pv = pv;
    }

    /// Lists an overlapping proto edge (`AddOverlappingPE`).
    pub fn add_overlapping_pe(&mut self, pe: ProtoEdgeId) {
        self.overlapping_pes.push(pe);
    }

    /// Lists overlapping proto edges (`AppendOverlappingPEs`).
    pub fn append_overlapping_pes(&mut self, pes: &[ProtoEdgeId]) {
        self.overlapping_pes.extend_from_slice(pes);
    }

    /// Forgets the edge definitions (`ClearEdgeDefinitions`).
    pub fn clear_edge_definitions(&mut self) {
        self.edge_defs.clear();
    }

    /// Replaces `old_pv` by `new_pv` at either end (`ReplaceProtoVertex`).
    pub fn replace_proto_vertex(
        &mut self,
        old_pv: Option<ProtoVertexId>,
        new_pv: Option<ProtoVertexId>,
    ) {
        if self.start_pv == old_pv {
            self.start_pv = new_pv;
        }
        if self.end_pv == old_pv {
            self.end_pv = new_pv;
        }
    }

    /// Unlists the first entry `ed` (`RemoveEdgeDefinition`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `ed` is not listed.
    pub fn remove_edge_definition(&mut self, ed: EdgeDefId) -> Result<()> {
        match self.edge_defs.iter().position(|&x| x == ed) {
            Some(i) => {
                self.edge_defs.remove(i);
                Ok(())
            }
            None => Err(SolidError::topology(OP, TopologyFailure::NotOnOwnerList)),
        }
    }

    /// Clears the ends equal to `pv` (`RemoveProtoVertex`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when neither end is `pv`.
    pub fn remove_proto_vertex(&mut self, pv: Option<ProtoVertexId>) -> Result<()> {
        let mut deleted = false;
        if pv == self.start_pv {
            self.start_pv = None;
            deleted = true;
        }
        if pv == self.end_pv {
            self.end_pv = None;
            deleted = true;
        }
        if deleted {
            Ok(())
        } else {
            Err(SolidError::topology(
                OP,
                TopologyFailure::ProtoVertexNotAnEnd,
            ))
        }
    }
}

// ---- SmProtoFace ------------------------------------------------------------------------

/// A coincident face pair of the two B-reps and the proto edges bounding it
/// (`SmProtoFace`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoFace<O> {
    /// `m_pBrep1Face`.
    brep1_face: Option<O>,
    /// `m_pBrep2Face`.
    brep2_face: Option<O>,
    /// `m_sCoincPEs`.
    coinc_pes: Vec<ProtoEdgeId>,
}

impl<O: ClassObject> ProtoFace<O> {
    /// The face of B-rep `which` (1, else 2) (`GetSrcTopo`).
    #[must_use]
    pub const fn src_topo(&self, which: i32) -> Option<O> {
        if which == 1 {
            self.brep1_face
        } else {
            self.brep2_face
        }
    }

    /// Always [`PointClassType::Face`] (`GetSrcTopo_TYPE`).
    #[must_use]
    pub const fn src_topo_type(&self, _which: i32) -> PointClassType {
        PointClassType::Face
    }

    /// The bounding proto edges (`GetCoincProtoEdges`).
    #[must_use]
    pub fn coinc_proto_edges(&self) -> &[ProtoEdgeId] {
        &self.coinc_pes
    }

    /// The bounding proto edges, for the edits of the list upstream makes through the
    /// pointer `GetCoincProtoEdges` returns.
    pub fn coinc_proto_edges_mut(&mut self) -> &mut Vec<ProtoEdgeId> {
        &mut self.coinc_pes
    }

    /// Sets the face of B-rep `which` (1, else 2): the object when it is a face
    /// (`SetSrcTopo`, `SM_CAST_PTR(SmFace, pTopo)`).
    pub fn set_src_topo(&mut self, which: i32, _t: PointClassType, topo: Option<O>) {
        let face = topo.filter(|t| t.class_kind() == ClassObjectKind::Face);
        if which == 1 {
            self.brep1_face = face;
        } else {
            self.brep2_face = face;
        }
    }
}

// ---- SmProtoTopologyManager -------------------------------------------------------------

/// The proto topology of one Boolean (`SmProtoTopologyManager`): the lists of proto
/// vertices, edges and faces, and the store every proto object and definition of
/// the operation lives in (upstream allocates them with `new (pOwner)`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoTopologyManager<O> {
    /// `m_pBrep1`.
    brep1: O,
    /// `m_pBrep2`.
    brep2: O,
    /// `m_pTI`.
    ti: O,
    /// `m_dTol`.
    tol: f64,
    /// `m_dCaptureDistance`.
    capture_distance: f64,
    /// `m_sPVs`.
    pvs: Vec<ProtoVertexId>,
    /// `m_sPEs`.
    pes: Vec<ProtoEdgeId>,
    /// `m_sPFs`.
    pfs: Vec<ProtoFaceId>,
    /// Every vertex definition made (`None` once deleted).
    vertex_def_store: Vec<Option<VertexDefinition<O>>>,
    /// Every edge definition made.
    edge_def_store: Vec<Option<EdgeDefinition<O>>>,
    /// Every proto vertex made.
    pv_store: Vec<Option<ProtoVertex>>,
    /// Every proto edge made.
    pe_store: Vec<Option<ProtoEdge>>,
    /// Every proto face made.
    pf_store: Vec<Option<ProtoFace<O>>>,
    /// Not upstream (WR-619): final vertices `(vertex, max_gap, speck_start, speck_end)`
    /// whose collapsed speck definition `SmEdgeDefinition::Imprint` skipped, so
    /// `ImprintIntersections` grows each vertex's tolerance to cover the speck's span.
    skipped_speck_vertices: Vec<(O, f64, Point3, Point3)>,
    /// Not upstream: the contact points a rule has proved one point
    /// ([`ProtoTopologyManager::set_declared_contacts`]).
    declared_contacts: Vec<DeclaredContact>,
    /// Not upstream: the uncertainty regions of the points each listed proto vertex was
    /// made of or took in ([`ProtoTopologyManager::find_proto_vertex_within`]).
    regions: Vec<(ProtoVertexId, DeclaredContact)>,
    /// Not upstream (#15718): pairs (B-rep 1's edge, B-rep 2's edge) of line edges whose
    /// two ends lie within their intersection tolerance of each other, end for end: one
    /// coincident interval, so a point where the two cross is no event and gets no vertex
    /// definition ([`ProtoTopologyManager::set_coincident_edges`]).
    coincident_edges: Vec<(O, O)>,
    /// Not upstream (WR-214): the first refusal of a proto edge from a proto vertex back
    /// to itself whose curve leaves its ends' reach of a definition on a vertex; the
    /// Boolean returns it once every proto edge is resolved
    /// ([`ProtoTopologyManager::take_self_loop_refusal`]).
    self_loop_refusal: Option<SolidError>,
}

/// Not upstream: a contact point that a rule of the intersector proves one point although
/// it spans more than the combined tolerance of the points in it (docs/booleans.md, "One
/// point or two"): every point within `radius` of the segment from `a` to `b`. Two points
/// both in it may share a proto vertex however far apart they are
/// ([`ProtoTopologyManager::find_proto_vertex_within`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeclaredContact {
    /// One end of the segment.
    pub a: Point3,
    /// The other end.
    pub b: Point3,
    /// How far from the segment its points reach.
    pub radius: f64,
}

impl DeclaredContact {
    /// The uncertainty region of a point `p` known to `tolerance` that a curve fixes on an
    /// edge of unit direction `along` there (docs/booleans.md, "One point or two"): across
    /// the edge the point is known to `tolerance`, along it only to the enclosed reach
    /// (the tolerance over the sine of the crossing angle:
    /// [`crate::curve::curve::Curve::crossing_reach`]; `limit`, the capture distance, how
    /// far proto vertices are looked for at all, where none is proved or it is longer).
    /// `reach` is that reach with how far the edge strays from the line through `p` along
    /// `along` within it ([`crate::curve::curve::Curve::stray_from_line`]), so the region is
    /// the segment that far either way along the edge, widened by the tolerance and the
    /// stray; a stray enclosed over a longer reach bounds it over `limit` too. With no
    /// enclosure of the edge (`None`) the region is the ball of `limit` and the tolerance:
    /// every point the lookup considers.
    #[must_use]
    pub fn of_crossing(
        p: Point3,
        tolerance: f64,
        along: Vec3,
        reach: Option<(f64, f64)>,
        limit: f64,
    ) -> Self {
        match reach {
            Some((reach, stray)) => {
                let reach = reach.min(limit);
                Self {
                    a: p - along * reach,
                    b: p + along * reach,
                    radius: (tolerance + stray) * (1.0 + gamma(1)),
                }
            }
            None => Self {
                a: p,
                b: p,
                radius: (tolerance + limit) * (1.0 + gamma(1)),
            },
        }
    }

    /// The region of a point with no crossing angle: the ball of its tolerance.
    #[must_use]
    pub const fn of_point(p: Point3, tolerance: f64) -> Self {
        Self {
            a: p,
            b: p,
            radius: tolerance,
        }
    }

    /// Whether this region and `other` are not proved apart: a lower bound of the distance
    /// between their segments is within the coincidence rule of their radii
    /// ([`within_coincidence_tol`]: the larger radius, at least the contact tolerance, and
    /// the rounding of the copies past it; docs/booleans.md, "One point or two"), where
    /// upstream's intersection tolerance would take their sum. The bound is the gap
    /// between the two segments' projections onto one direction, which no pair of their
    /// points is closer than whatever the direction, with the projections' rounding
    /// (`γ_4` of each point's distance from `other.a` times the direction's length)
    /// taken off. The direction is that of the closest pair found by clamping each
    /// segment's foot on the other in turn: only where to look, so a pair an ill-conditioned
    /// solve gets wrong (near-parallel segments) gives a smaller bound, never a larger one.
    /// The regions are taken for overlapping in doubt. A NaN is apart.
    #[must_use]
    pub fn overlaps(&self, k: &Kernel, other: &Self) -> bool {
        let (d1, d2, r) = (self.b - self.a, other.b - other.a, self.a - other.a);
        let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
        let unit = |x: f64| x.clamp(0.0, 1.0);
        let (s, t) = if a <= 0.0 && e <= 0.0 {
            (0.0, 0.0)
        } else if a <= 0.0 {
            (0.0, unit(f / e))
        } else {
            let c = d1.dot(r);
            if e <= 0.0 {
                (unit(-c / a), 0.0)
            } else {
                let b = d1.dot(d2);
                let denom = a * e - b * b;
                let mut s = if denom > 0.0 {
                    unit((b * f - c * e) / denom)
                } else {
                    0.0
                };
                let mut t = (b * s + f) / e;
                if t < 0.0 {
                    t = 0.0;
                    s = unit(-c / a);
                } else if t > 1.0 {
                    t = 1.0;
                    s = unit((b - c) / a);
                }
                (s, t)
            }
        };
        let w = self.a + d1 * s - (other.a + d2 * t);
        let width = w.length();
        // the points relative to `other.a`, each difference rounded once per coordinate
        let rb = self.b - other.a;
        let least = if width > 0.0 {
            let mine = (r.dot(w)).min(rb.dot(w));
            let theirs = d2.dot(w).max(0.0);
            let rounding = gamma(4) * width * (r.length() + rb.length() + d2.length());
            let gap = (mine - theirs) * (1.0 - gamma(1)) - rounding;
            if gap > 0.0 || gap.is_nan() {
                gap / width * (1.0 - gamma(4))
            } else {
                0.0
            }
        } else {
            width
        };
        let size = [self.a, self.b, other.a, other.b]
            .iter()
            .fold(0.0_f64, |m, p| m.max(largest_coordinate(*p)));
        within_coincidence_tol(k, least, self.radius, other.radius, size)
    }

    /// Whether `p` is within the radius of the segment, by an upper bound of its distance:
    /// the parameter of the nearest point is clamped to the segment, so any rounding of it
    /// only moves that point along the segment (the distance is at least the true one, as
    /// the true nearest point is no farther); the difference and the norm round by `γ_4`
    /// of `|p - a| + |b - a|`. A NaN distance is out.
    #[must_use]
    pub fn holds(&self, p: Point3) -> bool {
        let along = self.b - self.a;
        let w = p - self.a;
        let len2 = along.dot(along);
        let t = if len2 > 0.0 {
            (w.dot(along) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let distance = ((w - along * t).length() + gamma(4) * (w.length() + along.length()))
            * (1.0 + gamma(2));
        !(distance.is_nan() || distance > self.radius)
    }
}

/// The largest magnitude of `p`'s coordinates: the size whose rounding the coincidence rule
/// allows past its tolerance ([`within_coincidence_tol`]). A NaN coordinate is passed over
/// (`f64::max`); a NaN distance is apart whatever the size.
fn largest_coordinate(p: Point3) -> f64 {
    p.x.abs().max(p.y.abs()).max(p.z.abs())
}

fn push_store<T>(store: &mut Vec<Option<T>>, item: T) -> Result<u32> {
    let index = u32::try_from(store.len()).map_err(|_| dangling("store is full"))?;
    store.push(Some(item));
    Ok(index)
}

impl<O: ClassObject> ProtoTopologyManager<O> {
    /// `SmProtoTopologyManager(pBrep1, pBrep2, pTI, dCaptureDistance,
    /// dOverrideTol)`. Each B-rep is passed as its handle and its [`Brep`]. The
    /// tolerance is the topology intersector's approximation tolerance
    /// (`pTI->GetThisApproxTol3d()`, passed as `ti_approx_tol_3d`; the override is
    /// overwritten by it, as upstream); a tolerance below `SM_EFF_ZERO` becomes
    /// `min(1.25 * (tol1 + tol2), 0.5 * min(edge1, edge2))` from the B-reps'
    /// tolerances and smallest edge boxes ([`brep_p2::min_edge_bbox_size`]).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the edge box computation's unported callees;
    /// [`SolidError::NotFound`] for a dangling edge list.
    pub fn new(
        k: &Kernel,
        brep1: (O, &Brep),
        brep2: (O, &Brep),
        ti: O,
        ti_approx_tol_3d: f64,
        capture_distance: f64,
        override_tol: f64,
    ) -> Result<Self> {
        let _ = override_tol;
        let (brep1, b1) = brep1;
        let (brep2, b2) = brep2;
        // Get tol from pTI, which is from the SmMerge constructor.
        let mut tol = ti_approx_tol_3d;
        if tol < EFF_ZERO {
            let brep_tol = b1.tolerance() + b2.tolerance();
            // the edges found are unused, as upstream
            let (edge1_size, _) = brep_p2::min_edge_bbox_size(k, b1)?;
            let (edge2_size, _) = brep_p2::min_edge_bbox_size(k, b2)?;
            let edge_size = min2(edge1_size, edge2_size);
            tol = min2(brep_tol * 1.25, edge_size * 0.5);
        }
        Ok(Self {
            brep1,
            brep2,
            ti,
            tol,
            capture_distance,
            pvs: Vec::new(),
            pes: Vec::new(),
            pfs: Vec::new(),
            vertex_def_store: Vec::new(),
            edge_def_store: Vec::new(),
            pv_store: Vec::new(),
            pe_store: Vec::new(),
            pf_store: Vec::new(),
            skipped_speck_vertices: Vec::new(),
            declared_contacts: Vec::new(),
            regions: Vec::new(),
            coincident_edges: Vec::new(),
            self_loop_refusal: None,
        })
    }

    /// Sets the coincident line edge pairs (B-rep 1's edge, B-rep 2's edge): two straight
    /// edges whose ends lie within their intersection tolerance of each other, end for end,
    /// are within it of each other everywhere (the vector between points at the same
    /// fraction along both is affine in it, so its length is convex and its ends bound it),
    /// one coincident interval.
    /// Where they cross is where two coincident pieces cross within their tolerance, not a
    /// feature: [`ProtoTopologyManager::find_or_create_proto_vertex_pcs`] makes no vertex
    /// definition for a point classified on one edge of a pair in each B-rep. The
    /// Boolean's topology intersector sets them before intersecting. Not upstream
    /// (#15718).
    pub fn set_coincident_edges(&mut self, pairs: Vec<(O, O)>) {
        self.coincident_edges = pairs;
    }

    /// Whether `(edge1, edge2)` (of B-reps 1 and 2) is a coincident line edge pair
    /// ([`ProtoTopologyManager::set_coincident_edges`]). Not upstream.
    #[must_use]
    pub fn are_coincident_edges(&self, edge1: O, edge2: O) -> bool {
        self.coincident_edges.contains(&(edge1, edge2))
    }

    /// Final vertices `(vertex, max_gap, speck_start, speck_end)` whose collapsed speck
    /// definition `SmEdgeDefinition::Imprint` skipped (WR-619). Not upstream.
    #[must_use]
    pub fn skipped_speck_vertices(&self) -> &[(O, f64, Point3, Point3)] {
        &self.skipped_speck_vertices
    }

    /// Takes the refusal a proto edge from a proto vertex back to itself left
    /// (WR-214): its curve leaves its ends' reach of a definition on a vertex, so it can be
    /// neither split nor dropped. Not upstream. It also holds the refusal of a resolution
    /// that would grow a tolerance into another vertex definition (#18312,
    /// [`ProtoTopologyManager::refuse_growth_overlap`]), and the refusal of an overlapping
    /// proto edge group whose replacement failed part way (#18927,
    /// [`ProtoTopologyManager::resolve_overlapping_proto_edges`]).
    pub fn take_self_loop_refusal(&mut self) -> Option<SolidError> {
        self.self_loop_refusal.take()
    }

    /// Records the first such refusal (WR-214).
    pub(crate) fn note_self_loop_refusal(&mut self, refusal: SolidError) {
        if self.self_loop_refusal.is_none() {
            self.self_loop_refusal = Some(refusal);
        }
    }

    /// Records a final vertex whose collapsed speck definition was skipped at imprint.
    pub(crate) fn note_skipped_speck_vertex(
        &mut self,
        vertex: O,
        max_gap: f64,
        speck_start: Point3,
        speck_end: Point3,
    ) {
        self.skipped_speck_vertices
            .push((vertex, max_gap, speck_start, speck_end));
    }

    /// The destructor `~SmProtoTopologyManager`: first cuts the links between the
    /// listed proto edges and proto vertices (each listed edge's ends are cleared and
    /// each listed vertex's edge list emptied, so no destructor below unlists across
    /// them), then deletes the listed proto faces, then the listed proto edges (with their edge definitions, which delete the
    /// curves they own), then the listed proto vertices (with their vertex
    /// definitions). Returns the curve handles deleted. References to objects
    /// already deleted (a use after free upstream) are skipped. Objects that are not
    /// listed stay in the store (upstream allocates them from the context, not from
    /// the manager); the manager must not be used for anything else afterwards.
    #[must_use]
    pub fn destroy(&mut self) -> Vec<O> {
        // Cut the ProtoVertex <-> ProtoEdge links first: a ProtoVertex can still list a
        // ProtoEdge that has moved to another ProtoVertex, and its destructor would then
        // write into a deleted ProtoEdge.
        for &pe in &self.pes {
            if let Some(Some(e)) = self.pe_store.get_mut(pe.0 as usize) {
                e.set_start_proto_vertex(None);
                e.set_end_proto_vertex(None);
            }
        }
        for &pv in &self.pvs {
            if let Some(Some(v)) = self.pv_store.get_mut(pv.0 as usize) {
                v.proto_edges_mut().clear();
            }
        }

        let mut deleted = Vec::new();
        for pf in std::mem::take(&mut self.pfs) {
            if let Some(slot) = self.pf_store.get_mut(pf.0 as usize) {
                *slot = None;
            }
        }
        for pe in std::mem::take(&mut self.pes) {
            deleted.extend(self.delete_proto_edge(pe));
        }
        for pv in std::mem::take(&mut self.pvs) {
            self.delete_proto_vertex(pv);
        }
        self.skipped_speck_vertices.clear();
        deleted
    }

    /// `delete pPE` (`~SmProtoEdge`): deletes its edge definitions, unlists it from
    /// its end proto vertices (all occurrences, including closed edges); the curves
    /// deleted.
    fn delete_proto_edge(&mut self, pe: ProtoEdgeId) -> Vec<O> {
        let mut deleted = Vec::new();
        let Some(edge) = self.pe_store.get_mut(pe.0 as usize).and_then(Option::take) else {
            return deleted;
        };
        for ed in &edge.edge_defs {
            if let Some(d) = self
                .edge_def_store
                .get_mut(ed.0 as usize)
                .and_then(Option::take)
            {
                deleted.extend(d.destroy());
            }
        }
        for endpoint in [edge.start_pv, edge.end_pv].into_iter().flatten() {
            if let Some(Some(vertex)) = self.pv_store.get_mut(endpoint.0 as usize) {
                vertex.proto_edges.retain(|&incident| incident != pe);
            }
        }
        deleted
    }

    /// `delete pPV` (`~SmProtoVertex`): deletes its vertex definitions and clears it
    /// from the ends of the proto edges it lists.
    fn delete_proto_vertex(&mut self, pv: ProtoVertexId) {
        let Some(vertex) = self.pv_store.get_mut(pv.0 as usize).and_then(Option::take) else {
            return;
        };
        for vd in &vertex.vertex_defs {
            if let Some(slot) = self.vertex_def_store.get_mut(vd.0 as usize) {
                *slot = None;
            }
        }
        for pe in &vertex.proto_edges {
            if let Some(Some(e)) = self.pe_store.get_mut(pe.0 as usize) {
                let _ = e.remove_proto_vertex(Some(pv));
            }
        }
    }

    /// Empties the three lists without deleting anything (`Reset`).
    pub fn reset(&mut self) {
        self.declared_contacts.clear();
        self.regions.clear();
        self.pvs.clear();
        self.pes.clear();
        self.pfs.clear();
        self.skipped_speck_vertices.clear();
    }

    /// Not upstream: notes `region`, the uncertainty region of a point `pv` was made of or
    /// took in, so that later points are compared with it.
    pub fn note_region(&mut self, pv: ProtoVertexId, region: DeclaredContact) {
        self.regions.push((pv, region));
    }

    /// Not upstream: the contact points proved one point
    /// ([`DeclaredContact`]), set before the face pairs' proto vertices are made.
    pub fn set_declared_contacts(&mut self, contacts: Vec<DeclaredContact>) {
        self.declared_contacts = contacts;
    }

    // ---- simple access ---------------------------------------------------------------

    /// B-rep `which` (1, else 2) (`GetWhichBrep`).
    #[must_use]
    pub const fn which_brep(&self, which: i32) -> O {
        if which == 1 {
            self.brep1
        } else {
            self.brep2
        }
    }

    /// The topology intersector (`m_pTI`).
    #[must_use]
    pub const fn topology_intersector(&self) -> O {
        self.ti
    }

    /// The tolerance (`GetTolerance`, as an `SmZoneTol3d`).
    #[must_use]
    pub fn tolerance(&self, k: &Kernel) -> f64 {
        zone_tol3d(k, self.tol)
    }

    /// The raw tolerance `m_dTol` (read directly by the manager's own members).
    #[must_use]
    pub const fn raw_tolerance(&self) -> f64 {
        self.tol
    }

    /// Sets the tolerance (`SetTolerance`; the flags are unused).
    pub fn set_tolerance(&mut self, zone_tol_3d: f64) {
        self.tol = zone_tol_3d;
    }

    /// The capture distance (`GetCaptureDistance`).
    #[must_use]
    pub const fn capture_distance(&self) -> f64 {
        self.capture_distance
    }

    /// Sets the capture distance (`SetCaptureDistance`).
    pub fn set_capture_distance(&mut self, capture_distance: f64) {
        self.capture_distance = capture_distance;
    }

    /// The listed proto vertices (`GetProtoVertices`).
    #[must_use]
    pub fn proto_vertices(&self) -> &[ProtoVertexId] {
        &self.pvs
    }

    /// The listed proto edges (`GetProtoEdges`).
    #[must_use]
    pub fn proto_edges(&self) -> &[ProtoEdgeId] {
        &self.pes
    }

    /// The listed proto faces (`GetProtoFaces`).
    #[must_use]
    pub fn proto_faces(&self) -> &[ProtoFaceId] {
        &self.pfs
    }

    /// Lists `pv` (`AddProtoVertex`).
    pub fn add_proto_vertex(&mut self, pv: ProtoVertexId) {
        self.pvs.push(pv);
    }

    /// Lists `pe` (`AddProtoEdge`).
    pub fn add_proto_edge(&mut self, pe: ProtoEdgeId) {
        self.pes.push(pe);
    }

    /// Lists `pf` (`AddProtoFace`).
    pub fn add_proto_face(&mut self, pf: ProtoFaceId) {
        self.pfs.push(pf);
    }

    // ---- the store -------------------------------------------------------------------

    /// The vertex definition `vd`, if it exists.
    #[must_use]
    pub fn vertex_def(&self, vd: VertexDefId) -> Option<&VertexDefinition<O>> {
        self.vertex_def_store
            .get(vd.0 as usize)
            .and_then(Option::as_ref)
    }

    /// Mutable access to the vertex definition `vd`.
    pub fn vertex_def_mut(&mut self, vd: VertexDefId) -> Option<&mut VertexDefinition<O>> {
        self.vertex_def_store
            .get_mut(vd.0 as usize)
            .and_then(Option::as_mut)
    }

    /// The edge definition `ed`, if it exists.
    #[must_use]
    pub fn edge_def(&self, ed: EdgeDefId) -> Option<&EdgeDefinition<O>> {
        self.edge_def_store
            .get(ed.0 as usize)
            .and_then(Option::as_ref)
    }

    /// Mutable access to the edge definition `ed`.
    pub fn edge_def_mut(&mut self, ed: EdgeDefId) -> Option<&mut EdgeDefinition<O>> {
        self.edge_def_store
            .get_mut(ed.0 as usize)
            .and_then(Option::as_mut)
    }

    /// The proto vertex `pv`, if it exists.
    #[must_use]
    pub fn proto_vertex(&self, pv: ProtoVertexId) -> Option<&ProtoVertex> {
        self.pv_store.get(pv.0 as usize).and_then(Option::as_ref)
    }

    /// Mutable access to the proto vertex `pv`.
    pub fn proto_vertex_mut(&mut self, pv: ProtoVertexId) -> Option<&mut ProtoVertex> {
        self.pv_store
            .get_mut(pv.0 as usize)
            .and_then(Option::as_mut)
    }

    /// The proto edge `pe`, if it exists.
    #[must_use]
    pub fn proto_edge(&self, pe: ProtoEdgeId) -> Option<&ProtoEdge> {
        self.pe_store.get(pe.0 as usize).and_then(Option::as_ref)
    }

    /// Mutable access to the proto edge `pe`.
    pub fn proto_edge_mut(&mut self, pe: ProtoEdgeId) -> Option<&mut ProtoEdge> {
        self.pe_store
            .get_mut(pe.0 as usize)
            .and_then(Option::as_mut)
    }

    /// The proto face `pf`, if it exists.
    #[must_use]
    pub fn proto_face(&self, pf: ProtoFaceId) -> Option<&ProtoFace<O>> {
        self.pf_store.get(pf.0 as usize).and_then(Option::as_ref)
    }

    /// Mutable access to the proto face `pf`.
    pub fn proto_face_mut(&mut self, pf: ProtoFaceId) -> Option<&mut ProtoFace<O>> {
        self.pf_store
            .get_mut(pf.0 as usize)
            .and_then(Option::as_mut)
    }

    fn vd(&self, vd: VertexDefId) -> Result<&VertexDefinition<O>> {
        self.vertex_def(vd)
            .ok_or(dangling("vertex definition is deleted"))
    }

    fn ed(&self, ed: EdgeDefId) -> Result<&EdgeDefinition<O>> {
        self.edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))
    }

    fn pv(&self, pv: Option<ProtoVertexId>) -> Result<&ProtoVertex> {
        pv.and_then(|pv| self.proto_vertex(pv))
            .ok_or(dangling("proto vertex is null or deleted"))
    }

    fn pv_mut(&mut self, pv: ProtoVertexId) -> Result<&mut ProtoVertex> {
        self.proto_vertex_mut(pv)
            .ok_or(dangling("proto vertex is deleted"))
    }

    fn pe(&self, pe: Option<ProtoEdgeId>) -> Result<&ProtoEdge> {
        pe.and_then(|pe| self.proto_edge(pe))
            .ok_or(dangling("proto edge is null or deleted"))
    }

    fn pe_mut(&mut self, pe: ProtoEdgeId) -> Result<&mut ProtoEdge> {
        self.proto_edge_mut(pe)
            .ok_or(dangling("proto edge is deleted"))
    }

    /// Stores a vertex definition (`new (pOwner) SmVertexDefinition(..)`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the store is full.
    pub fn insert_vertex_definition(&mut self, vd: VertexDefinition<O>) -> Result<VertexDefId> {
        push_store(&mut self.vertex_def_store, vd).map(VertexDefId)
    }

    /// Stores an edge definition (`new (pOwner) SmEdgeDefinition(..)`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the store is full.
    pub fn insert_edge_definition(&mut self, ed: EdgeDefinition<O>) -> Result<EdgeDefId> {
        push_store(&mut self.edge_def_store, ed).map(EdgeDefId)
    }

    /// Stores a copy of the edge definition `ed` (the copy constructor
    /// `SmEdgeDefinition(const SmEdgeDefinition&)`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted definition or a full store.
    pub fn copy_edge_definition(&mut self, ed: EdgeDefId) -> Result<EdgeDefId> {
        let copy = self.ed(ed)?.copy_definition();
        self.insert_edge_definition(copy)
    }

    /// Deletes the vertex definition `vd` (`delete pVD`; `~SmVertexDefinition` does
    /// nothing else).
    pub fn delete_vertex_definition(&mut self, vd: VertexDefId) {
        if let Some(slot) = self.vertex_def_store.get_mut(vd.0 as usize) {
            *slot = None;
        }
    }

    /// Deletes the edge definition `ed` (`delete pED`): the curves deleted.
    pub fn delete_edge_definition(&mut self, ed: EdgeDefId) -> Vec<O> {
        self.edge_def_store
            .get_mut(ed.0 as usize)
            .and_then(Option::take)
            .map(EdgeDefinition::destroy)
            .unwrap_or_default()
    }

    /// Deletes the proto edge `pe` (`delete pPE`, `~SmProtoEdge`): the curves deleted.
    pub fn delete_proto_edge_object(&mut self, pe: ProtoEdgeId) -> Vec<O> {
        self.delete_proto_edge(pe)
    }

    /// Drops every reference the listed proto faces (coincident proto edges) and
    /// proto edges (overlapping proto edges) hold to `pe`, before it is deleted.
    ///
    /// SolidCraft fix tangency-labs/smlib 93efd6d (UB-239), which does this in
    /// `RemoveProtoEdge` because every library caller deletes the edge next. Upstream
    /// keeps the references, and later passes over those lists
    /// (`GlueCoincidentBrepVertices`, `Resolve`) read the edge after the free. The port
    /// calls it at those library deletes, so `remove_proto_edge` and
    /// `delete_proto_edge_object` stay upstream's on their own.
    pub fn forget_proto_edge(&mut self, pe: ProtoEdgeId) {
        for pf in self.pfs.clone() {
            if let Some(face) = self.proto_face_mut(pf) {
                face.coinc_proto_edges_mut().retain(|&x| x != pe);
            }
        }
        for other in self.pes.clone() {
            if let Some(edge) = self.proto_edge_mut(other) {
                edge.overlapping_pes.retain(|&x| x != pe);
            }
        }
    }

    /// Deletes the proto vertex `pv` (`delete pPV`, `~SmProtoVertex`).
    pub fn delete_proto_vertex_object(&mut self, pv: ProtoVertexId) {
        self.delete_proto_vertex(pv);
    }

    // ---- constructors of the proto objects -------------------------------------------

    /// A proto vertex without vertex definitions (`SmProtoVertex(crPos, dTol,
    /// pOwner)`), not listed yet.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the store is full.
    pub fn new_proto_vertex(&mut self, position: Point3, tol: f64) -> Result<ProtoVertexId> {
        push_store(
            &mut self.pv_store,
            ProtoVertex {
                position,
                tol,
                vertex_defs: Vec::new(),
                proto_edges: Vec::new(),
                absorbed_proto_edge: false,
                absorbed_points: Vec::new(),
                absorbed_unbounded: false,
            },
        )
        .map(ProtoVertexId)
    }

    /// A proto vertex with one new vertex definition of `pc1`, `pc2` at `position`
    /// (gap computed) (`SmProtoVertex(crPC1, crPC2, rPosition, dTol, pOwner)`), not
    /// listed yet.
    ///
    /// # Errors
    ///
    /// Those of [`VertexDefinition::new`].
    pub fn new_proto_vertex_with_pcs(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
        position: Point3,
        tol: f64,
    ) -> Result<ProtoVertexId> {
        let id = ProtoVertexId(
            u32::try_from(self.pv_store.len()).map_err(|_| dangling("store is full"))?,
        );
        let vd = VertexDefinition::new(k, geometry, pc1, pc2, position, tol, -1.0, Some(id))?;
        let vd_id = self.insert_vertex_definition(vd)?;
        push_store(
            &mut self.pv_store,
            ProtoVertex {
                position,
                tol,
                vertex_defs: vec![vd_id],
                proto_edges: Vec::new(),
                absorbed_proto_edge: false,
                absorbed_points: Vec::new(),
                absorbed_unbounded: false,
            },
        )
        .map(ProtoVertexId)
    }

    /// A proto edge from `start` to `end` over `interval` with the manager's
    /// tolerance (`SmProtoEdge(pStartPV, pEndPV, crIvl, pOwner)`), not listed yet.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the store is full.
    pub fn new_proto_edge(
        &mut self,
        k: &Kernel,
        start: Option<ProtoVertexId>,
        end: Option<ProtoVertexId>,
        interval: Extent1d,
    ) -> Result<ProtoEdgeId> {
        let tol = self.tolerance(k);
        push_store(
            &mut self.pe_store,
            ProtoEdge {
                xsect_interval: interval,
                start_pv: start,
                end_pv: end,
                edge_defs: Vec::new(),
                tol,
                overlapping_pes: Vec::new(),
                coinc_pfs: Vec::new(),
            },
        )
        .map(ProtoEdgeId)
    }

    /// A copy of the proto edge `pe` without definitions, overlaps or proto faces
    /// (the copy constructor `SmProtoEdge(const SmProtoEdge&)`), not listed.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted proto edge or a full store.
    pub fn copy_proto_edge(&mut self, pe: ProtoEdgeId) -> Result<ProtoEdgeId> {
        let src = self.pe(Some(pe))?;
        let copy = ProtoEdge {
            xsect_interval: src.xsect_interval,
            start_pv: src.start_pv,
            end_pv: src.end_pv,
            edge_defs: Vec::new(),
            tol: src.tol,
            overlapping_pes: Vec::new(),
            coinc_pfs: Vec::new(),
        };
        push_store(&mut self.pe_store, copy).map(ProtoEdgeId)
    }

    /// A proto face of the coincident faces `face1`, `face2` bounded by `coinc_pes`
    /// (`SmProtoFace(pBrep1Face, pBrep2Face, crCoincPEs, pOwner)`), not listed.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the store is full.
    pub fn new_proto_face(
        &mut self,
        face1: Option<O>,
        face2: Option<O>,
        coinc_pes: &[ProtoEdgeId],
    ) -> Result<ProtoFaceId> {
        push_store(
            &mut self.pf_store,
            ProtoFace {
                brep1_face: face1,
                brep2_face: face2,
                coinc_pes: coinc_pes.to_vec(),
            },
        )
        .map(ProtoFaceId)
    }

    // ---- SmVertexDefinition members that reach the manager --------------------------

    /// The number of proto edges at the vertex definition's proto vertex
    /// (`GetNumProtoEdges`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a proto vertex.
    pub fn vertex_def_num_proto_edges(&self, vd: VertexDefId) -> Result<usize> {
        Ok(self.pv(self.vd(vd)?.proto_vertex)?.num_proto_edges())
    }

    /// Resolves the vertex definition `vd` (`Resolve`): when its two object points
    /// are farther apart than the manager's tolerance and either classification has
    /// a gap, the position moves between them, proportionally closer to the object
    /// with the smaller gap but within both objects' zone tolerances. The status
    /// output is always 0. Not upstream (WR-306): the move goes through
    /// [`VertexDefinition::set_position`], so the tolerance grows to keep every absorbed
    /// hull point covered.
    ///
    /// Not upstream (#18274): a definition on a vertex that ends
    /// a seam edge ([`vertex::seam_edge`]), or on a seam edge itself, is put onto that seam,
    /// at the seam's point under the position worked out as upstream ([`onto_seam`]). The
    /// seam edge's curve is the seam, an iso line of its face's surface, and its vertex ends
    /// that line; a definition on the seam edge makes a vertex that splits it and ends both
    /// pieces. A vertex off the seam leaves the seam edge's trims off the seam, which the
    /// face's seam test refuses (test 36, a suspicious edge near the seam) when another edge
    /// runs beside the seam there: two edges crossing within tolerance, one of them a seam,
    /// are blended halfway (`find_position_from_pcs`), which puts the crossing off the seam
    /// by half the other edge's offset across it (#18689). Along the seam the definition
    /// still moves as upstream's, sharing the gap with the other object. With both sides on
    /// seams the first's seam holds it.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a proto vertex; [`SolidError::Unported`]
    /// from the classifications (topology objects); [`SolidError::Failed`] (from
    /// `set_position`, the definition unchanged) when an absorbed hull point lies at no
    /// finite distance from the new position, and the refusal
    /// ([`ProtoTopologyManager::refuse_growth_overlap`], the definition unchanged) when the
    /// growth reaches another vertex definition
    /// ([`ProtoTopologyManager::growth_reaches_another`]).
    pub fn resolve_vertex_definition(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        vd: VertexDefId,
    ) -> Result<i32> {
        let v = self.vd(vd)?;
        let dist1 = v.pc1.gap_3d(k, geometry, None)?;
        let dist2 = v.pc2.gap_3d(k, geometry, None)?;

        // statuses ignored upstream; passed on as the fork does (bcc6f01), except for a
        // classification with no object point (#18927; see `definition_object_point`)
        let pt1 = definition_object_point(k, geometry, &v.pc1, v.position)?;
        let pt2 = definition_object_point(k, geometry, &v.pc2, v.position)?;

        let dist = pt1.distance(pt2);

        // the manager's tolerance (through the proto vertex back pointer)
        self.pv(v.proto_vertex)?;
        let tol = self.tolerance(k);

        // (#18274) the seam a definition on a seam's vertex or edge keeps to
        let seam = match seam_curve(geometry, &v.pc1)? {
            Some(c) => Some(c),
            None => seam_curve(geometry, &v.pc2)?,
        };

        if dist > tol && dist1 + dist2 > EFF_ZERO {
            // Put it proportionally closer to the topo object with the smaller gap,
            // but not out of Tolerance
            let vec = pt2 - pt1;
            let vec_length = vec.length();
            let tol1 = object_zone_tol_3d(k, geometry, v.pc1.object())?;
            let tol2 = object_zone_tol_3d(k, geometry, v.pc2.object())?;
            let frac_min = 1. - tol2 / vec_length;
            let frac_max = tol1 / vec_length;
            let mut frac_ivl = Extent1d::from_min_max(0.0, 1.0);
            let disjoint = frac_max < frac_min;
            if !disjoint {
                frac_ivl.set_min_max(frac_min, frac_max)?;
            }

            let frac = dist1 / (dist1 + dist2);
            let frac = frac_ivl.clamp_value(frac);
            let position = pt1 + frac * vec;
            // Not upstream (WR-306): through `set_position`, so a definition that absorbed
            // a self-loop grows to keep the curve's hull covered from the new position (a
            // definition with no hull is moved as upstream moves it). Not upstream
            // (#18312): a growth into another vertex definition its tolerance kept clear of
            // is refused, with the definition unchanged. The move is upstream's, so "clear
            // before" is judged at the new position, as the self-loop split judges its own.
            if let Some(v) = self.vertex_def(vd) {
                if let Some(cover) = v.covering_tolerance(k, position) {
                    if self.growth_reaches_another(
                        k,
                        position,
                        v.tolerance(k),
                        position,
                        cover,
                        v.proto_vertex,
                        &[vd],
                    )? {
                        return Err(self.refuse_growth_overlap(cover));
                    }
                }
            }
            if let Some(v) = self.vertex_def_mut(vd) {
                v.set_position(k, position)?;
            }
        }
        if let Some(curve) = seam {
            let position = onto_seam(k, curve, self.vd(vd)?.position)?;
            if let Some(v) = self.vertex_def_mut(vd) {
                v.position = position;
            }
        }
        Ok(0)
    }

    // ---- SmEdgeDefinition members that reach the manager ----------------------------

    /// The start proto vertex of the definition's proto edge
    /// (`SmEdgeDefinition::GetStartProtoVertex`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a proto edge.
    pub fn edge_def_start_proto_vertex(&self, ed: EdgeDefId) -> Result<Option<ProtoVertexId>> {
        Ok(self.pe(self.ed(ed)?.proto_edge)?.start_pv)
    }

    /// The end proto vertex of the definition's proto edge
    /// (`SmEdgeDefinition::GetEndProtoVertex`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a proto edge.
    pub fn edge_def_end_proto_vertex(&self, ed: EdgeDefId) -> Result<Option<ProtoVertexId>> {
        Ok(self.pe(self.ed(ed)?.proto_edge)?.end_pv)
    }

    /// The 3d curve's point at the end of the definition bounded by `which_pv`
    /// (`SmEdgeDefinition::GetEndPosition(SmProtoVertex*, SmPoint3d&)`; the
    /// evaluation status is ignored upstream, passed on as the fork does, bcc6f01).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `which_pv` bounds neither end of the proto edge;
    /// [`SolidError::InvalidInput`] without a proto edge or curve.
    pub fn edge_def_end_position_at(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        ed: EdgeDefId,
        which_pv: Option<ProtoVertexId>,
    ) -> Result<Point3> {
        let d = self.ed(ed)?;
        let pe = self.pe(d.proto_edge)?;
        let t = if which_pv == pe.start_pv {
            d.xsect_interval.min()
        } else if which_pv == pe.end_pv {
            d.xsect_interval.max()
        } else {
            return Err(SolidError::topology(
                OP_END,
                TopologyFailure::ProtoVertexNotAnEnd,
            ));
        };
        curve_of(geometry, d.xsect_curve_3d)?.evaluate_point(k, t)
    }

    /// Whether the intersection interval `tgt_curve` over `tgt_domain` between
    /// `tgt_start`, `tgt_end` has the shape of the definition `ed`
    /// (`IsSameEdgeDefinition`), and whether it also comes from the same source
    /// objects (the midpoint classifications' objects).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without a proto edge or curve;
    /// [`SolidError::Unported`] from the curve comparison.
    pub fn is_same_edge_definition(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        ed: EdgeDefId,
        tgt_start: Option<ProtoVertexId>,
        tgt_end: Option<ProtoVertexId>,
        mid_pc1: &PointClassification<O>,
        mid_pc2: &PointClassification<O>,
        tgt_curve: Option<O>,
        tgt_domain: &Extent1d,
    ) -> Result<(bool, bool)> {
        let d = self.ed(ed)?;
        let pe = self.pe(d.proto_edge)?;
        let (start, end) = (pe.start_pv, pe.end_pv);

        // First, it would have to have the same two ProtoVertices.
        let forward =
            (start == tgt_start && end == tgt_end) || (end == tgt_start && start == tgt_end);
        if !forward {
            return Ok((false, false));
        }

        // The 3d curves have to be roughly the same (use the stored larger
        // CaptureDistance).
        let tol = self.capture_distance;
        if !are_same_curve(
            k,
            geometry,
            d.xsect_curve_3d,
            &d.xsect_interval,
            tgt_curve,
            tgt_domain,
            tol,
        )? {
            return Ok((false, false));
        }

        // when ED->SrcTopos are the same as the TgtInterval->MidPt->Objects
        let identical =
            mid_pc1.object() == Some(d.src_topo1) && mid_pc2.object() == Some(d.src_topo2);
        Ok((true, identical))
    }

    // ---- SmProtoVertex members that reach the manager -------------------------------

    /// Lists the vertex definition `vd` at `pv` and makes `pv` its proto vertex
    /// (`AddVertexDefinition(SmVertexDefinition*)`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects.
    pub fn add_vertex_definition(&mut self, pv: ProtoVertexId, vd: VertexDefId) -> Result<()> {
        self.pv_mut(pv)?.vertex_defs.push(vd);
        self.vertex_def_mut(vd)
            .ok_or(dangling("vertex definition is deleted"))?
            .proto_vertex = Some(pv);
        Ok(())
    }

    /// The first vertex definition at `pv` with source objects `topo1`, `topo2`
    /// (`FindMatchingVertexDefinition`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects.
    pub fn find_matching_vertex_definition(
        &self,
        pv: ProtoVertexId,
        topo1: Option<O>,
        topo2: Option<O>,
    ) -> Result<Option<VertexDefId>> {
        for &vd in &self.pv(Some(pv))?.vertex_defs {
            let v = self.vd(vd)?;
            if v.src_topo(1) == topo1 && v.src_topo(2) == topo2 {
                return Ok(Some(vd));
            }
        }
        Ok(None)
    }

    /// Not upstream (WR-410, tangency-labs/solidcore#1059; the SolidCraft fork's
    /// `SmProtoVertex::AddCrossing`): an intersection curve crosses an edge at `pv`
    /// (`pc1`, `pc2`, the crossing's classifications, with their parameters set, at angle
    /// `asin(crossing_sine)`; `curve`, the intersection curve and its parameter at the
    /// crossing). The vertex definition of the crossing's source objects takes its
    /// position from the certified crossing that fixes it best
    /// ([`VertexDefinition::place_by_crossing`]); placed again, it moves `pv` (the
    /// average of its definitions) with it and widens `pv`'s tolerance to its own.
    /// Nothing for an unknown angle or no matching definition.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects; those of
    /// [`VertexDefinition::place_by_crossing`].
    pub fn add_crossing(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pv: ProtoVertexId,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
        crossing_sine: f64,
        curve: Option<(O, f64)>,
    ) -> Result<()> {
        // (upstream's `!(sine >= 0)`: a NaN sine is unknown)
        if crossing_sine.is_nan() || crossing_sine < 0.0 {
            return Ok(());
        }
        let topo1 = as_topology(pc1.object());
        let topo2 = as_topology(pc2.object());
        let Some(vd) = self.find_matching_vertex_definition(pv, topo1, topo2)? else {
            return Ok(());
        };

        // Placed again by a crossing that fixes it more closely, it moves this
        // ProtoVertex, the average of its VertexDefinitions, with it.
        let old = self.vd(vd)?.position;
        let def = self
            .vertex_def_mut(vd)
            .ok_or(dangling("vertex definition is deleted"))?;
        let crossed = def.place_by_crossing(k, geometry, pc1, pc2, crossing_sine, curve)?;
        // (#3062) and then on its three faces' common point, where it has one
        let moved = def.place_on_three_surfaces(k, geometry)? || crossed;
        if moved {
            let (new, vd_tol) = {
                let v = self.vd(vd)?;
                (v.position, v.tolerance(k))
            };
            let count = self.pv(Some(pv))?.vertex_defs.len() as f64;
            let p = self.pv_mut(pv)?;
            // (#16637: through `set_position`, so a vertex that absorbed definitions keeps
            // covering them)
            p.set_position(p.position + (new - old) / count);
            p.tol = max2(p.tol, vd_tol);
        }
        Ok(())
    }

    /// The vertex definition at `pv` closest to `position`
    /// (`GetClosestVertexDefinition`; the first of equally close ones).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects.
    pub fn closest_vertex_definition(
        &self,
        pv: ProtoVertexId,
        position: Point3,
    ) -> Result<Option<VertexDefId>> {
        let mut ret = None;
        let mut min_dist = BIG_DOUBLE;
        for &vd in &self.pv(Some(pv))?.vertex_defs {
            let dist = position.distance_squared(self.vd(vd)?.position);
            if dist < min_dist {
                ret = Some(vd);
                min_dist = dist;
            }
        }
        Ok(ret)
    }

    /// The B-rep `which` vertex at `pv` connected to the source of the edge
    /// definition `ed` and closest to its start (or end) (`GetFinalVertex`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects or a missing curve; those of
    /// the `geometry` topology walks.
    pub fn proto_vertex_final_vertex(
        &self,
        k: &Kernel,
        geometry: &(impl ClassGeometry<O> + ProtoTopology<O>),
        pv: ProtoVertexId,
        which: i32,
        ed: EdgeDefId,
        at_start: bool,
    ) -> Result<Option<O>> {
        let vds = &self.pv(Some(pv))?.vertex_defs;
        if vds.is_empty() {
            return Ok(None);
        }

        let mut ret_val = None;
        let mut max_dist = BIG_DOUBLE;

        let d = self.ed(ed)?;
        let brep_topo_ed = d.src_topo(which);
        let ed_pos = d.end_position(k, geometry, at_start)?;

        for &vd in vds {
            let Some(vtx) = self.vd(vd)?.final_vertex(which) else {
                continue;
            };
            if are_connected(geometry, brep_topo_ed, Some(vtx))? {
                let dist = ed_pos.distance(geometry.vertex_point(vtx)?);
                if dist < max_dist {
                    max_dist = dist;
                    ret_val = Some(vtx);
                }
            }
        }
        Ok(ret_val)
    }

    /// Merges `other` into `pv` (`CombineProtoVertex`): the larger tolerance, all of
    /// `other`'s vertex definitions (re-owned, still listed at `other`), the position
    /// averaged over the definitions (`other` contributing its own position once per
    /// definition), and `other`'s proto edges re-pointed and listed at `pv`. Not upstream:
    /// the uncertainty regions of `other`'s points, and (#16637) `other`'s absorbed points,
    /// which are listed at `pv` too, the averaged position covering all of them
    /// ([`ProtoVertex::set_position`]).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects.
    pub fn combine_proto_vertex(&mut self, pv: ProtoVertexId, other: ProtoVertexId) -> Result<()> {
        // If they're the same, do nothing.
        if other == pv {
            return Ok(());
        }
        let o = self.pv(Some(other))?.clone();
        let this = self.pv_mut(pv)?;
        this.tol = max2(this.tol, o.tol);
        this.absorbed_proto_edge = this.absorbed_proto_edge || o.absorbed_proto_edge;
        // Not upstream (#16637): the definitions `other` combined away are this one's
        // (covered where the position is written below).
        this.absorb(&o.absorbed_points, !o.absorbed_unbounded);

        // the average of all VDs, starting with what is here already
        let mut ave_pos = (this.vertex_defs.len() as f64) * this.position;

        for &vd in &o.vertex_defs {
            self.add_vertex_definition(pv, vd)?;
            ave_pos += o.position;
        }
        let this = self.pv_mut(pv)?;
        let num_vds = this.vertex_defs.len();
        this.set_position(ave_pos / num_vds as f64);

        // Replace pOtherPV with 'this' in all of pOtherPV's ProtoEdges.
        for &pe in &o.proto_edges {
            self.pe_mut(pe)?.replace_proto_vertex(Some(other), Some(pv));
        }

        self.pv_mut(pv)?
            .proto_edges
            .extend_from_slice(&o.proto_edges);

        // Not upstream: `pv` now holds `other`'s points, so it keeps their uncertainty
        // regions ([`ProtoTopologyManager::find_proto_vertex_within`]).
        for (of, _) in &mut self.regions {
            if *of == other {
                *of = pv;
            }
        }
        Ok(())
    }

    // ---- SmProtoEdge members that reach the manager ---------------------------------

    /// The unique group of proto edges overlapping `pe`, directly or through other
    /// members (`GetOverlappingPEGroup`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects.
    pub fn overlapping_pe_group(&self, pe: ProtoEdgeId) -> Result<Vec<ProtoEdgeId>> {
        // start with this PE's overlapping PEs
        let mut group = self.pe(Some(pe))?.overlapping_pes.clone();
        let mut this_iter_cnt = 0usize;

        // chase Overlapping PE associations until the whole group is found
        while this_iter_cnt < group.len() {
            let last_iter_cnt = this_iter_cnt;
            this_iter_cnt = group.len();

            for ii in last_iter_cnt..this_iter_cnt {
                let member = *group.get(ii).ok_or(dangling("group index"))?;
                for &o in &self.pe(Some(member))?.overlapping_pes {
                    if !group.contains(&o) {
                        group.push(o);
                    }
                }
            }
        }
        Ok(group)
    }

    /// The position of the end of `pe` bounded by `which_pv`
    /// (`SmProtoEdge::GetEndPosition(SmProtoVertex*, SmPoint3d&)`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `which_pv` bounds neither end; those of
    /// [`ProtoTopologyManager::proto_edge_end_position`].
    pub fn proto_edge_end_position_at(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pe: ProtoEdgeId,
        which_pv: Option<ProtoVertexId>,
    ) -> Result<Point3> {
        let e = self.pe(Some(pe))?;
        if which_pv == e.start_pv {
            self.proto_edge_end_position(k, geometry, pe, true)
        } else if which_pv == e.end_pv {
            self.proto_edge_end_position(k, geometry, pe, false)
        } else {
            Err(SolidError::topology(
                OP_END,
                TopologyFailure::ProtoVertexNotAnEnd,
            ))
        }
    }

    /// The start (or end) position of `pe` (`SmProtoEdge::GetEndPosition(SmBoolean,
    /// SmPoint3d&)`): the average of its definitions' end points, or its proto
    /// vertex's position without definitions.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a missing proto vertex, definition or curve.
    pub fn proto_edge_end_position(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pe: ProtoEdgeId,
        at_start: bool,
    ) -> Result<Point3> {
        let e = self.pe(Some(pe))?;
        if e.edge_defs.is_empty() {
            let pv = if at_start { e.start_pv } else { e.end_pv };
            return Ok(self.pv(pv)?.position);
        }

        // average position of all EdgeDefs
        let mut ave_pt = Vec3::new(0.0, 0.0, 0.0);
        for &ed in &e.edge_defs {
            ave_pt += self.ed(ed)?.end_position(k, geometry, at_start)?;
        }
        Ok(ave_pt / e.edge_defs.len() as f64)
    }

    /// The average point and/or tangent of the definitions of `pe` at `param`
    /// (`SmProtoEdge::Evaluate`).
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` without definitions; those of
    /// [`EdgeDefinition::evaluate`].
    pub fn proto_edge_evaluate(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pe: ProtoEdgeId,
        param: f64,
        want_pos: bool,
        want_tan: bool,
    ) -> Result<(Option<Point3>, Option<Vec3>)> {
        let e = self.pe(Some(pe))?;
        let num_eds = e.edge_defs.len();
        if num_eds == 0 {
            return Err(SolidError::topology(
                OP_EVALUATE,
                TopologyFailure::ProtoEdgeWithoutDefinitions,
            ));
        }

        let mut sum_pts = Vec3::new(0.0, 0.0, 0.0);
        let mut sum_tans = Vec3::new(0.0, 0.0, 0.0);
        for &ed in &e.edge_defs {
            let (p, t) = self
                .ed(ed)?
                .evaluate(k, geometry, param, want_pos, want_tan)?;
            if let Some(p) = p {
                sum_pts += p;
            }
            if let Some(t) = t {
                sum_tans += t;
            }
        }

        Ok((
            want_pos.then(|| sum_pts / num_eds as f64),
            want_tan.then(|| sum_tans / num_eds as f64),
        ))
    }

    /// [`ProtoTopologyManager::proto_edge_evaluate`] at the middle of the interval
    /// (`SmProtoEdge::EvaluateMid`).
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::proto_edge_evaluate`].
    pub fn proto_edge_evaluate_mid(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pe: ProtoEdgeId,
        want_pos: bool,
        want_tan: bool,
    ) -> Result<(Option<Point3>, Option<Vec3>)> {
        let param = self.pe(Some(pe))?.xsect_interval.mid();
        self.proto_edge_evaluate(k, geometry, pe, param, want_pos, want_tan)
    }

    /// Whether the intersection interval has the shape of a definition of `pe`
    /// (`IsSameProtoEdge`); the second value (always `false` when the first is not
    /// `true` with an identical definition) tells whether that definition also comes
    /// from the same source objects.
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::is_same_edge_definition`].
    pub fn is_same_proto_edge(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pe: ProtoEdgeId,
        tgt_start: Option<ProtoVertexId>,
        tgt_end: Option<ProtoVertexId>,
        mid_pc1: &PointClassification<O>,
        mid_pc2: &PointClassification<O>,
        tgt_curve: Option<O>,
        tgt_domain: &Extent1d,
    ) -> Result<(bool, bool)> {
        let e = self.pe(Some(pe))?;
        let mut found_same_shape_only = false;

        // does this PE run between the Tgt PVs
        let forward = (tgt_start == e.start_pv && tgt_end == e.end_pv)
            || (tgt_start == e.end_pv && tgt_end == e.start_pv);
        if !forward {
            return Ok((false, false));
        }

        for &ed in &e.edge_defs {
            let (same, identical) = self.is_same_edge_definition(
                k, geometry, ed, tgt_start, tgt_end, mid_pc1, mid_pc2, tgt_curve, tgt_domain,
            )?;
            if same {
                if identical {
                    return Ok((true, true));
                }
                found_same_shape_only = true;
            }
        }
        Ok((found_same_shape_only, false))
    }

    /// Relates `pe` and the coincident proto face `pf` both ways, each without
    /// duplicates (`SmProtoEdge::RelateCoincidentPF`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects.
    pub fn relate_coincident_pf(&mut self, pe: ProtoEdgeId, pf: ProtoFaceId) -> Result<()> {
        let e = self.pe_mut(pe)?;
        if !e.coinc_pfs.contains(&pf) {
            e.coinc_pfs.push(pf);
        }
        let f = self
            .proto_face_mut(pf)
            .ok_or(dangling("proto face is deleted"))?;
        if !f.coinc_pes.contains(&pe) {
            f.coinc_pes.push(pe);
        }
        Ok(())
    }

    // ---- SmProtoTopologyManager -----------------------------------------------------

    /// The vertex definitions of all listed proto vertices, in list order
    /// (`GetAllVertexDefinitions`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted listed proto vertex.
    pub fn all_vertex_definitions(&self) -> Result<Vec<VertexDefId>> {
        let mut out = Vec::new();
        for &pv in &self.pvs {
            out.extend_from_slice(&self.pv(Some(pv))?.vertex_defs);
        }
        Ok(out)
    }

    /// The edge definitions of all listed proto edges, in list order
    /// (`GetAllEdgeDefinitions`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted listed proto edge.
    pub fn all_edge_definitions(&self) -> Result<Vec<EdgeDefId>> {
        let mut out = Vec::new();
        for &pe in &self.pes {
            out.extend_from_slice(&self.pe(Some(pe))?.edge_defs);
        }
        Ok(out)
    }

    /// The closest listed proto vertex containing `position`, and its distance
    /// ([`BIG_DOUBLE`] for none) (`FindProtoVertex`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted listed proto vertex.
    pub fn find_proto_vertex(&self, position: Point3) -> Result<(Option<ProtoVertexId>, f64)> {
        // Init outputs.
        let mut ret_val = None;
        let mut min_dist = BIG_DOUBLE;

        // for every ProtoVertex - find the closest one to rPosition that's within
        // max(m_dTol, GetCaptureDistance())
        for &id in &self.pvs {
            let pv = self.pv(Some(id))?;
            if face_p2::proto_vertex_contains_point(self, pv, position) {
                let this_dist = face_p2::proto_vertex_distance_to(pv, position);
                if this_dist < min_dist {
                    ret_val = Some(id);
                    min_dist = this_dist;
                }
            }
        }
        Ok((ret_val, min_dist))
    }

    /// Not upstream: the closest listed proto vertex containing `position` (as
    /// [`ProtoTopologyManager::find_proto_vertex`]) that is not provably farther from it than
    /// their combined tolerance: the proto vertex's tolerance plus `tolerance`, the point's
    /// (zone tolerance), unless a [`DeclaredContact`] holds both. The capture distance only
    /// says where to look; two points farther apart than their tolerances reach are two
    /// points (docs/booleans.md, "One point or two"), and a rule that proves them one
    /// contact point declares it ([`ProtoTopologyManager::set_declared_contacts`]). The
    /// distance is taken as at least its rounded value shrunk by `γ_4` (three differences,
    /// three squares, a sum and a root), so only pairs proved apart are refused; a NaN
    /// distance is apart.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted listed proto vertex.
    pub fn find_proto_vertex_within(
        &self,
        k: &Kernel,
        position: Point3,
        tolerance: f64,
        region: Option<&DeclaredContact>,
    ) -> Result<Option<ProtoVertexId>> {
        self.find_proto_vertex_within_if(k, position, tolerance, region, |_| Ok(true))
    }

    /// The closest reachable proto vertex satisfying an additional incidence predicate.
    pub(crate) fn find_proto_vertex_within_if(
        &self,
        k: &Kernel,
        position: Point3,
        tolerance: f64,
        region: Option<&DeclaredContact>,
        compatible: impl Fn(ProtoVertexId) -> Result<bool>,
    ) -> Result<Option<ProtoVertexId>> {
        let point = DeclaredContact::of_point(position, tolerance);
        let region = region.unwrap_or(&point);
        let mut ret_val = None;
        let mut min_dist = BIG_DOUBLE;
        for &id in &self.pvs {
            let pv = self.pv(Some(id))?;
            if !face_p2::proto_vertex_contains_point(self, pv, position) {
                continue;
            }
            let this_dist = face_p2::proto_vertex_distance_to(pv, position);
            let size = largest_coordinate(pv.position).max(largest_coordinate(position));
            let apart = !within_coincidence_tol(
                k,
                this_dist * (1.0 - gamma(4)),
                pv.tolerance(k),
                tolerance,
                size,
            );
            // (the proto vertex's own ball, and the regions of the points it holds)
            let own = DeclaredContact::of_point(pv.position, pv.tolerance(k));
            let one_region = || {
                region.overlaps(k, &own)
                    || self
                        .regions
                        .iter()
                        .any(|(of, r)| *of == id && region.overlaps(k, r))
            };
            if apart
                && !one_region()
                && !self
                    .declared_contacts
                    .iter()
                    .any(|c| c.holds(position) && c.holds(pv.position))
            {
                continue;
            }
            if this_dist < min_dist && compatible(id)? {
                ret_val = Some(id);
                min_dist = this_dist;
            }
        }
        Ok(ret_val)
    }

    /// Not upstream: combines every two listed proto vertices that one point does not
    /// tell apart (docs/booleans.md, "One point or two"). A point is taken in by the
    /// closest proto vertex its region is not proved apart from
    /// ([`ProtoTopologyManager::find_proto_vertex_within`]); a proto vertex made after it,
    /// or a second one its region also reaches, stays another proto vertex although the
    /// point may lie at either, so the two are not proved two places. After the face pairs'
    /// proto vertices are made, each noted region is compared with every other listed proto
    /// vertex within the capture distance of the region's point, as that lookup does (the
    /// proto vertex's own ball and the regions of the points it holds), and the two are
    /// combined (as `SplitIntersectingProtoEdges` combines and deletes), until no region
    /// reaches a second proto vertex. A proto edge between the two becomes a self-loop,
    /// which [`ProtoTopologyManager::resolve_proto_edge`] drops within its ends' reach or
    /// refuses.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted listed proto vertex; those of
    /// [`ProtoTopologyManager::combine_proto_vertex`] and the removal (passed on, as the
    /// combine of `SplitIntersectingProtoEdges`).
    pub fn combine_proto_vertices_not_proved_apart(&mut self, k: &Kernel) -> Result<()> {
        self.combine_proto_vertices_if(k, |_, _, _| Ok(true))
    }

    /// Coalesces reachable events whose native vertex/face incidences are compatible.
    pub(crate) fn combine_proto_vertices_with_incidence(
        &mut self,
        k: &Kernel,
        geometry: &impl super::super::p2::ProtoBrep<O>,
    ) -> Result<()> {
        self.combine_proto_vertices_if(k, |this, of, id| {
            for &def in this.pv(Some(of))?.vertex_definitions() {
                let def = this.vd(def)?;
                if this.proto_vertex_incidence_conflicts(
                    k,
                    geometry,
                    id,
                    [def.brep_point_classif(1), def.brep_point_classif(2)],
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }

    fn combine_proto_vertices_if(
        &mut self,
        k: &Kernel,
        compatible: impl Fn(&Self, ProtoVertexId, ProtoVertexId) -> Result<bool>,
    ) -> Result<()> {
        // sweeps over the regions until one combines nothing (each combine removes a
        // listed proto vertex, so there are at most as many as proto vertices)
        let mut combined = true;
        while combined {
            combined = false;
            let mut i = 0;
            while let Some(&(of, region)) = self.regions.get(i) {
                let other = if self.pvs.contains(&of) {
                    self.proto_vertex_not_apart_from(k, of, &region, |id| compatible(self, of, id))?
                } else {
                    None
                };
                let Some(gone) = other else {
                    i += 1;
                    continue;
                };
                // (the same region is compared again: its proto vertex now holds more)
                self.combine_proto_vertex(of, gone)?;
                self.pv_mut(gone)?.clear_vertex_definitions();
                self.remove_proto_vertex(gone)?;
                self.delete_proto_vertex_object(gone);
                // (a region whose proto vertex is gone without a combine reaches nothing)
                self.regions.retain(|(at, _)| *at != gone);
                combined = true;
            }
        }
        Ok(())
    }

    /// Not upstream: a listed proto vertex other than `of` that `region` is not proved
    /// apart from, looked for as [`ProtoTopologyManager::find_proto_vertex_within`] does
    /// (within the capture distance of the region's point; its own ball, or a region of a
    /// point it holds).
    fn proto_vertex_not_apart_from(
        &self,
        k: &Kernel,
        of: ProtoVertexId,
        region: &DeclaredContact,
        compatible: impl Fn(ProtoVertexId) -> Result<bool>,
    ) -> Result<Option<ProtoVertexId>> {
        let centre = region.a + (region.b - region.a) * 0.5;
        for &id in &self.pvs {
            if id == of {
                continue;
            }
            let pv = self.pv(Some(id))?;
            if !face_p2::proto_vertex_contains_point(self, pv, centre) {
                continue;
            }
            let own = DeclaredContact::of_point(pv.position, pv.tolerance(k));
            if region.overlaps(k, &own)
                || self
                    .regions
                    .iter()
                    .any(|(at, r)| *at == id && region.overlaps(k, r))
            {
                if !compatible(id)? {
                    continue;
                }
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// The first edge definition (of the listed proto edges) whose B-rep `which`
    /// source or final edge is `target_edge` (`FindEdgeDefinitionFromEdge`).
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::all_edge_definitions`].
    pub fn find_edge_definition_from_edge(
        &self,
        target_edge: Option<O>,
        which: i32,
    ) -> Result<Option<EdgeDefId>> {
        for ed in self.all_edge_definitions()? {
            let d = self.ed(ed)?;
            if d.src_topo(which) == target_edge {
                return Ok(Some(ed));
            }
            if d.final_edge(which) == target_edge {
                return Ok(Some(ed));
            }
        }
        Ok(None)
    }

    /// Unlists the vertex definition `vd` from its proto vertex (not deleted), and
    /// with `remove_owner_if_empty` unlists and deletes that proto vertex when it has
    /// no definitions left (`RemoveVertexDefinition`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects or no proto vertex; the
    /// removals' `SM_ERR` (passed on as the fork does, bcc6f01).
    pub fn remove_vertex_definition(
        &mut self,
        vd: VertexDefId,
        remove_owner_if_empty: bool,
    ) -> Result<()> {
        let owner = self
            .vd(vd)?
            .proto_vertex
            .ok_or(dangling("vertex definition has no proto vertex"))?;

        // status ignored upstream; passed on as the fork does (bcc6f01)
        self.pv_mut(owner)?.remove_vertex_definition(vd)?;

        if remove_owner_if_empty && self.pv(Some(owner))?.vertex_defs.is_empty() {
            self.remove_proto_vertex(owner)?;
            self.delete_proto_vertex(owner);
        }
        Ok(())
    }

    /// Unlists the first entry `pv` (`RemoveProtoVertex`; nothing is deleted).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `pv` is not listed.
    pub fn remove_proto_vertex(&mut self, pv: ProtoVertexId) -> Result<()> {
        match self.pvs.iter().position(|&x| x == pv) {
            Some(i) => {
                self.pvs.remove(i);
                Ok(())
            }
            None => Err(SolidError::topology(OP, TopologyFailure::NotOnOwnerList)),
        }
    }

    /// Unlists the first entry `pe` and unlists it at its end proto vertices
    /// (`RemoveProtoEdge`; nothing is deleted).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `pe` is not listed; [`SolidError::InvalidInput`] for deleted
    /// objects; the end proto vertices' `SM_ERR` when `pe` is not listed there (passed
    /// on as the fork does, bcc6f01).
    pub fn remove_proto_edge(&mut self, pe: ProtoEdgeId) -> Result<()> {
        match self.pes.iter().position(|&x| x == pe) {
            Some(i) => {
                self.pes.remove(i);
            }
            None => return Err(SolidError::topology(OP, TopologyFailure::NotOnOwnerList)),
        }

        // remove End PV->PE back pointers (statuses ignored upstream; passed on as the
        // fork does, bcc6f01)
        let e = self.pe(Some(pe))?;
        let (start, end) = (e.start_pv, e.end_pv);
        if let Some(s) = start {
            self.pv_mut(s)?.remove_proto_edge(pe)?;
        }
        if let Some(e) = end {
            self.pv_mut(e)?.remove_proto_edge(pe)?;
        }
        Ok(())
    }

    /// Adds `survivor` to the coincident proto edges of every listed proto face that
    /// has `removed` there and not `survivor` yet: the faces bounded by a proto edge
    /// combined away are bounded by the one it was combined into (SolidCraft change,
    /// tangency-labs/smlib 93efd6d, in `SplitIntersectingProtoEdges`).
    pub fn hand_over_coincident_proto_edge(&mut self, survivor: ProtoEdgeId, removed: ProtoEdgeId) {
        for pf in self.pfs.clone() {
            if let Some(face) = self.proto_face_mut(pf) {
                let list = face.coinc_proto_edges_mut();
                if list.contains(&removed) && !list.contains(&survivor) {
                    list.push(survivor);
                }
            }
        }
    }

    /// The proto edges with a definition from the same source objects as the
    /// interval pair's midpoints that overlaps `interval1` over a segment
    /// (`CheckOverlappingProtoEdges`; upstream appends them to `rOverlappingPEs`).
    /// Only intervals on a B-rep edge are checked. `classification1` is the
    /// classification `interval1` belongs to.
    ///
    /// # Errors
    ///
    /// Those of [`are_overlapping`]; [`SolidError::InvalidInput`] for deleted
    /// objects.
    pub fn check_overlapping_proto_edges(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        classification1: &CurveClassification<O>,
        interval1: &CurveInterval<O>,
        interval2: &CurveInterval<O>,
    ) -> Result<Vec<Option<ProtoEdgeId>>> {
        let mut out = Vec::new();
        let mid1 = &interval1.mid;
        let mid2 = &interval2.mid;
        if mid1.edge_object().is_none() && mid2.edge_object().is_none() {
            return Ok(out);
        }

        let obj1 = as_topology(mid1.object());
        let obj2 = as_topology(mid2.object());
        for ed in self.all_edge_definitions()? {
            let d = self.ed(ed)?;
            if d.src_topo(1) != obj1 || d.src_topo(2) != obj2 {
                continue;
            }
            if are_overlapping(k, geometry, classification1, interval1, d)? {
                out.push(d.proto_edge);
            }
        }
        Ok(out)
    }

    /// Makes and lists a proto face (`CreateProtoFace`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the store is full.
    pub fn create_proto_face(
        &mut self,
        face1: Option<O>,
        face2: Option<O>,
        coinc_pes: &[ProtoEdgeId],
    ) -> Result<ProtoFaceId> {
        let pf = self.new_proto_face(face1, face2, coinc_pes)?;
        self.add_proto_face(pf);
        Ok(pf)
    }

    /// Whether two definitions of one proto edge can be combined, and which to
    /// delete (`CanCombineEdgeDefinitions`; 0 neither, 1 the first, 2 the second):
    /// the second when both come from the same source objects; otherwise, for
    /// connected source pairs with the same curve, the one on higher-dimensional
    /// topology.
    ///
    /// # Errors
    ///
    /// Those of the `geometry` topology walks and of [`find_ed_on_degen_face`] but its statuses
    /// (those too, passed on as the fork does, bcc6f01).
    pub fn can_combine_edge_definitions(
        &self,
        k: &Kernel,
        geometry: &(impl ClassGeometry<O> + ProtoTopology<O>),
        ed1: EdgeDefId,
        ed2: EdgeDefId,
    ) -> Result<u64> {
        self.can_combine_edge_definitions_within(k, geometry, ed1, ed2, ED_TOL_FACTOR)
    }

    /// [`ProtoTopologyManager::can_combine_edge_definitions`] with the distance allowed
    /// between the two curves `factor` times the definitions' summed tolerances
    /// (upstream's [`ED_TOL_FACTOR`], or 1 for exactly placed definitions; see
    /// `proto_topology::p2`).
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::can_combine_edge_definitions`].
    pub fn can_combine_edge_definitions_within(
        &self,
        k: &Kernel,
        geometry: &(impl ClassGeometry<O> + ProtoTopology<O>),
        ed1: EdgeDefId,
        ed2: EdgeDefId,
        factor: f64,
    ) -> Result<u64> {
        let d1 = self.ed(ed1)?;
        let d2 = self.ed(ed2)?;
        let (t11, t12, t21, t22) = (d1.src_topo1, d1.src_topo2, d2.src_topo1, d2.src_topo2);

        // If they're sitting on the same topology in both Breps, combine.
        if t11 == t21 && t12 == t22 {
            return Ok(2);
        }

        // If either pair is not connected in their Brep, can't combine.
        if !are_connected(geometry, Some(t11), Some(t21))?
            || !are_connected(geometry, Some(t12), Some(t22))?
        {
            return Ok(0);
        }

        // Check distance: if they're too far apart, we can't combine them. (Upstream
        // compares the second curve over the first definition's interval.)
        let domain1 = d1.xsect_interval;
        let tol = (d1.tolerance(k) + d2.tolerance(k)) * factor;
        let same = if factor == EXACT_TOL_FACTOR {
            // Not upstream (#15718): at the exact radius both definitions are lines
            // (`edge_combine_factor`), and the decision is a certificate over both whole
            // pieces, not upstream's one midpoint.
            exact_lines_coincide(k, geometry, d1, d2, tol)?
        } else {
            are_same_curve(
                k,
                geometry,
                d1.xsect_curve_3d,
                &domain1,
                d2.xsect_curve_3d,
                &domain1,
                tol,
            )?
        };
        if !same {
            return Ok(0);
        }

        // Check dimensionality - [vertex:0, edge:1, face:2]
        let dim11 = topo_dimension(t11);
        let dim12 = topo_dimension(t12);
        let dim21 = topo_dimension(t21);
        let dim22 = topo_dimension(t22);

        let mut can_delete = 0;
        if dim11 < dim21 && dim12 <= dim22 {
            can_delete = 2;
        }
        if dim21 < dim11 && dim22 <= dim12 {
            can_delete = 1;
        }
        if dim12 < dim22 && dim11 <= dim21 {
            can_delete = 2;
        }
        if dim22 < dim12 && dim21 <= dim11 {
            can_delete = 1;
        }

        // check for coincident edges (e.g. unstitched edges or degenerate faces)
        let is_edge = |t: O| t.class_kind() == ClassObjectKind::Edge;
        if is_edge(t11) && is_edge(t12) && is_edge(t21) && is_edge(t22) {
            if t11 != t21 && t12 == t22 {
                let n = find_ed_on_degen_face(geometry, t11, t21)?;
                if n != 0 {
                    return Ok(n);
                }
            } else if t11 == t21 && t12 != t22 {
                let n = find_ed_on_degen_face(geometry, t12, t22)?;
                if n != 0 {
                    return Ok(n);
                }
            }
        }
        Ok(can_delete)
    }

    /// Removes the proto vertex `pv` when it has no proto edges and lies on the curve
    /// of `pe` (the first definition's curve over the proto edge's interval, within
    /// twice the proto edge's tolerance) (`RemoveCoincidentNoEdgeProtoVertex`; the
    /// proto vertex is unlisted, not deleted). When the point solve fails or finds
    /// nothing the answer is `false`, nothing removed (WR-73: upstream returns
    /// `SM_SUCCESS` from its `SmBoolean` function there, which reads as `true`). A proto
    /// vertex that has proto edges is `false` without its point solve (not upstream: no
    /// outcome of the solve gives another answer).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects or a missing curve;
    /// [`SolidError::Unported`] and [`SolidError::Cancelled`] from the point solve, where it
    /// runs (it runs for every proto vertex under an already-fired cancel token).
    pub fn remove_coincident_no_edge_proto_vertex(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pe: ProtoEdgeId,
        pv: ProtoVertexId,
    ) -> Result<bool> {
        let e = self.pe(Some(pe))?;

        // no work - ProtoEdge is already terminated by ProtoVertex.
        if e.start_pv == Some(pv) || e.end_pv == Some(pv) {
            return Ok(false);
        }

        // no work - ProtoEdge does not have any EdgeDefinitions
        let Some(&first_ed) = e.edge_defs.first() else {
            return Ok(false);
        };

        let p = self.pv(Some(pv))?;
        let vertex_pos = p.position;
        let edge_zone_tol = e.tolerance(k);
        let xsect_tol = xsect_tol3d(k, edge_zone_tol + edge_zone_tol);

        let crv_handle = self.ed(first_ed)?.xsect_curve_3d;
        let crv = curve_of(geometry, crv_handle)?;
        let crv_ivl = e.xsect_interval;

        // (not upstream) a proto vertex with proto edges is never removed, and a miss is
        // FALSE as well (WR-73): the answer is FALSE whatever the point solve finds, so it
        // is not run. A boolean of N tools in one call ran 4 N^2 - 2 N of these solves for
        // nothing.
        //
        // Under an already-fired cancel token the solve runs as it did (it is where this
        // test notices the cancel, when it builds a curve's cache; a line's or a circle's
        // solve does not), so from the same caches the call returns what it returned. A
        // skipped solve leaves its curve's cache cold, which a later cancelled call can
        // show as another label or outcome; the boolean asks the token again after this
        // step.
        if !p.proto_edges.is_empty() && k.check_cancelled(OP_REMOVE_COINCIDENT).is_ok() {
            return Ok(false);
        }

        // XSect ProtoEdgeCurve/ProtoVertexPosition
        let opts = GlobalPointSolveOptions {
            handle: Some(CurveHandle(crv.natural_interval())),
            ..GlobalPointSolveOptions::default()
        };
        k.work_counters.add_proto_vertex_point_solve();
        let sols = ignored(crv.global_point_solve(
            k,
            &crv_ivl,
            SolverOperation::Intersect,
            vertex_pos,
            xsect_tol,
            SolutionRequested::All,
            &opts,
        ))?;
        // upstream returns SM_SUCCESS here, which reads as TRUE (the proto vertex is
        // not removed): plan/upstream-issues WR-73; the port answers FALSE
        // (tangency-labs/solidcore#848)
        let Some(sols) = sols else {
            return Ok(false);
        };
        if sols.is_empty() {
            return Ok(false);
        }

        // When the ProtoVertex is Edgeless and XSects a ProtoEdge - remove it
        if p.proto_edges.is_empty() {
            let removed = self.remove_proto_vertex(pv);
            // fork bcc6f01: a vertex it could not remove was not removed
            if removed.is_err() {
                return Ok(false);
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// The proto vertex to combine at a proto edge split (`sm_WhichPVToCombine`):
    /// `split_pv` when the edge was split, else the end proto vertex containing the
    /// edge's point at `split_param`, if any.
    ///
    /// # Errors
    ///
    /// Without `split_pv`: [`SolidError::InvalidInput`] for a deleted proto edge or a
    /// proto edge without start or end proto vertex (upstream dereferences them), and
    /// those of [`ProtoTopologyManager::proto_edge_evaluate`] upstream does not ignore
    /// ([`SolidError::Unported`], [`SolidError::InvalidInput`]).
    pub fn which_pv_to_combine(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        orig_pe: ProtoEdgeId,
        split_param: f64,
        split_pv: Option<ProtoVertexId>,
    ) -> Result<Option<ProtoVertexId>> {
        if split_pv.is_some() {
            return Ok(split_pv);
        }

        // locals
        let e = self.pe(Some(orig_pe))?;
        let start_pv = e.start_pv;
        let end_pv = e.end_pv;

        // Eval PE at SplitParam (status ignored: the point stays `SmPoint3d()`)
        let split_pt =
            match self.proto_edge_evaluate(k, geometry, orig_pe, split_param, true, false) {
                Ok((Some(p), _)) => p,
                Err(
                    e @ (SolidError::Unported { .. }
                    | SolidError::Cancelled { .. }
                    | SolidError::InvalidInput { .. }
                    | SolidError::PortInvariant { .. }),
                ) => return Err(e),
                // fork bcc6f01: no value, no evidence
                Err(_) => return Ok(None),
                Ok((None, _)) => Vec3::UNDEF,
            };

        // return matching End PV or NULL for none
        if face_p2::proto_vertex_contains_point(self, self.pv(start_pv)?, split_pt) {
            Ok(start_pv)
        } else if face_p2::proto_vertex_contains_point(self, self.pv(end_pv)?, split_pt) {
            Ok(end_pv)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(usize, ClassObjectKind);

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            self.1
        }
    }

    fn manager(k: &Kernel) -> ProtoTopologyManager<H> {
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        ProtoTopologyManager::new(k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap()
    }

    /// Unlisting what is not listed is an `SM_ERR` that says so.
    #[test]
    fn unlisting_a_missing_entry_is_a_typed_failure() {
        let k = Kernel::default();
        let mut m = manager(&k);
        let pv = m.new_proto_vertex(Point3::ZERO, 1e-5).unwrap();
        let e = m
            .proto_vertex_mut(pv)
            .unwrap()
            .remove_vertex_definition(VertexDefId::from_index(7))
            .unwrap_err();
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(e.topology_failure(), Some(&TopologyFailure::NotOnOwnerList));
        let e = m
            .remove_proto_vertex(ProtoVertexId::from_index(99))
            .unwrap_err();
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(e.topology_failure(), Some(&TopologyFailure::NotOnOwnerList));
    }

    /// An entry the caller names that is not on the list, or a proto vertex that is
    /// neither end of the proto edge, is the caller's argument: class `invalid_input`,
    /// with the status upstream reports (`SM_ERR`).
    #[test]
    fn a_list_entry_or_end_the_caller_names_wrongly_is_invalid_input() {
        use crate::error::{ErrorKind, SM_ERR};
        let k = Kernel::default();
        let mut m = manager(&k);
        let pv = m.new_proto_vertex(Point3::ZERO, 1e-5).unwrap();
        let other = m.new_proto_vertex(Point3::X, 1e-5).unwrap();
        let pe = m
            .new_proto_edge(&k, Some(pv), None, Extent1d::from_raw(0.0, 1.0))
            .unwrap();
        let unlisted = TopologyFailure::NotOnOwnerList;
        let errors = [
            (
                m.proto_vertex_mut(pv)
                    .unwrap()
                    .remove_vertex_definition(VertexDefId::from_index(7))
                    .unwrap_err(),
                unlisted,
            ),
            (
                m.proto_vertex_mut(pv)
                    .unwrap()
                    .remove_proto_edge(ProtoEdgeId::from_index(7))
                    .unwrap_err(),
                unlisted,
            ),
            (
                m.proto_edge_mut(pe)
                    .unwrap()
                    .remove_edge_definition(EdgeDefId::from_index(7))
                    .unwrap_err(),
                unlisted,
            ),
            (
                m.proto_edge_mut(pe)
                    .unwrap()
                    .remove_proto_vertex(Some(other))
                    .unwrap_err(),
                TopologyFailure::ProtoVertexNotAnEnd,
            ),
            (
                m.remove_proto_vertex(ProtoVertexId::from_index(99))
                    .unwrap_err(),
                unlisted,
            ),
            (
                m.remove_proto_edge(ProtoEdgeId::from_index(99))
                    .unwrap_err(),
                unlisted,
            ),
        ];
        for (e, cause) in errors {
            assert_eq!(e.topology_failure(), Some(&cause), "{e}");
            assert_eq!(e.kind(), ErrorKind::InvalidInput, "{e}");
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{e}");
            assert!(!e.is_defect(), "{e}");
        }
    }

    #[test]
    fn deleting_a_closed_proto_edge_unlinks_both_incident_entries() {
        let k = Kernel::default();
        let mut m = manager(&k);
        let vertex = m.new_proto_vertex(Point3::ZERO, 1e-5).unwrap();
        let interval = Extent1d::from_raw(0.0, 1.0);
        let gone = m
            .new_proto_edge(&k, Some(vertex), Some(vertex), interval)
            .unwrap();
        let kept = m.new_proto_edge(&k, Some(vertex), None, interval).unwrap();
        let incident = m.proto_vertex_mut(vertex).unwrap();
        incident.add_proto_edge(gone);
        incident.add_proto_edge(kept);
        incident.add_proto_edge(gone);

        assert!(m.delete_proto_edge_object(gone).is_empty());
        assert!(m.proto_edge(gone).is_none());
        assert_eq!(m.proto_vertex(vertex).unwrap().proto_edges(), &[kept]);
        assert!(m.proto_edge(kept).is_some());
        // Repeated cleanup cannot disturb the remaining live edge.
        assert!(m.delete_proto_edge_object(gone).is_empty());
        assert_eq!(m.proto_vertex(vertex).unwrap().proto_edges(), &[kept]);
        m.delete_proto_vertex_object(vertex);
        assert_eq!(m.proto_edge(kept).unwrap().start_proto_vertex(), None);
    }

    #[test]
    fn deleting_a_proto_edge_after_vertex_combination_unlinks_every_entry() {
        let k = Kernel::default();
        let mut m = manager(&k);
        let start = m.new_proto_vertex(Point3::ZERO, 1e-5).unwrap();
        let end = m.new_proto_vertex(Point3::X, 1e-5).unwrap();
        let gone = m
            .new_proto_edge(&k, Some(start), Some(end), Extent1d::from_raw(0.0, 1.0))
            .unwrap();
        m.proto_vertex_mut(start).unwrap().add_proto_edge(gone);
        m.proto_vertex_mut(end).unwrap().add_proto_edge(gone);
        m.combine_proto_vertex(start, end).unwrap();
        assert_eq!(m.proto_vertex(start).unwrap().proto_edges(), &[gone, gone]);

        m.delete_proto_edge_object(gone);
        assert!(m.proto_vertex(start).unwrap().proto_edges().is_empty());
    }

    /// Two segments crossing at an angle of 1e-9 (their determinant rounds to zero, and the
    /// closest pair the clamped solve finds is 1e-9 apart) are not proved apart, however
    /// small the radii. Regions are one place by the coincidence rule
    /// (`within_coincidence_tol`): within the larger radius, never under the contact
    /// tolerance (the system zone tolerance, 1e-5 here), and the rounding of the copies
    /// past it (`γ_32` of the coordinates' size, 1.5: 5.3e-15). Parallel segments 1.5e-5
    /// apart with radii of 1e-5 are proved apart (upstream's intersection tolerance, the
    /// radii's sum of 2e-5, took them for one); 0.9e-5 apart they are one. A radius of
    /// 3e-5 takes in a segment 2.5e-5 off whatever the other's; radii of 1e-7 still reach
    /// the contact tolerance (0.9e-5 apart: one; 1.1e-5: two). A NaN is apart.
    #[test]
    fn near_parallel_crossing_regions_are_not_proved_apart() {
        let k = Kernel::default();
        let seg = |a: Point3, b: Point3, radius: f64| DeclaredContact { a, b, radius };
        let one = seg(
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            1e-12,
        );
        let other = seg(
            Point3::new(-0.5, -1e-9, 0.0),
            Point3::new(1.5, 1e-9, 0.0),
            1e-12,
        );
        assert!(one.overlaps(&k, &other));
        assert!(other.overlaps(&k, &one));
        let at = |off: f64, radius: f64| {
            seg(
                Point3::new(-0.5, off, 0.0),
                Point3::new(1.5, off, 0.0),
                radius,
            )
        };
        let one = at(0.0, 1e-5);
        for (off, radius, overlaps) in [
            (1.5e-5, 1e-5, false),
            (0.9e-5, 1e-5, true),
            (2.5e-5, 3e-5, true),
            (3.1e-5, 3e-5, false),
        ] {
            let parallel = at(off, radius);
            assert_eq!(one.overlaps(&k, &parallel), overlaps, "{off:e} {radius:e}");
            assert_eq!(parallel.overlaps(&k, &one), overlaps, "{off:e} {radius:e}");
        }
        let small = at(0.0, 1e-7);
        assert!(small.overlaps(&k, &at(0.9e-5, 1e-7)));
        assert!(!small.overlaps(&k, &at(1.1e-5, 1e-7)));
        let ball = DeclaredContact::of_point(Point3::new(0.3, 1.5e-5, 0.0), 1e-5);
        assert!(!one.overlaps(&k, &ball));
        assert!(DeclaredContact::of_point(Point3::new(0.3, 0.9e-5, 0.0), 1e-5).overlaps(&k, &one));
        let nan = DeclaredContact::of_point(Point3::new(f64::NAN, 0.0, 0.0), 1.0);
        assert!(!one.overlaps(&k, &nan));
    }

    /// Two points are one proto vertex within the larger of their tolerances (the coincidence
    /// rule), not their sum: with tolerances of 1e-5 (the contact tolerance too), a point
    /// 1.5e-5 from a proto vertex is past both balls, so it is two points (the sum, 2e-5,
    /// made them one); 0.9e-5 away it is one. A proto vertex with a tolerance of 2e-5 takes a
    /// point of tolerance 1e-5 1.5e-5 away (within its own ball); 2.1e-5 away it does not.
    #[test]
    fn two_points_are_one_proto_vertex_within_the_larger_tolerance() {
        let k = Kernel::default();
        let mut m = manager(&k);
        let tol = 1e-5;
        let pv = m.new_proto_vertex(Point3::new(1.0, 2.0, 3.0), tol).unwrap();
        m.add_proto_vertex(pv);
        let at = |x: f64| Point3::new(1.0 + x, 2.0, 3.0);
        assert_eq!(
            m.find_proto_vertex_within(&k, at(1.5e-5), tol, None)
                .unwrap(),
            None
        );
        assert_eq!(
            m.find_proto_vertex_within(&k, at(0.9e-5), tol, None)
                .unwrap(),
            Some(pv)
        );
        m.proto_vertex_mut(pv).unwrap().set_raw_tolerance(2e-5);
        assert_eq!(
            m.find_proto_vertex_within(&k, at(1.5e-5), tol, None)
                .unwrap(),
            Some(pv)
        );
        assert_eq!(
            m.find_proto_vertex_within(&k, at(2.1e-5), tol, None)
                .unwrap(),
            None
        );
    }

    /// A proto vertex combined into another hands over the uncertainty regions of its
    /// points: a point that only the combined-away proto vertex's region reached is found
    /// at the survivor after the combine, as before it.
    #[test]
    fn a_combined_proto_vertex_keeps_the_regions_of_its_points() {
        let k = Kernel::default();
        let mut m = manager(&k);
        let tol = 1e-6;
        let kept = m.new_proto_vertex(Point3::ZERO, tol).unwrap();
        let gone = m
            .new_proto_vertex(Point3::new(1e-4, 0.0, 0.0), tol)
            .unwrap();
        m.add_proto_vertex(kept);
        m.add_proto_vertex(gone);
        // a crossing at `gone` known only to 4e-4 along x
        m.note_region(
            gone,
            DeclaredContact::of_crossing(
                Point3::new(1e-4, 0.0, 0.0),
                tol,
                Vec3::X,
                Some((4e-4, 0.0)),
                1e-3,
            ),
        );
        let query = Point3::new(4.5e-4, 0.0, 0.0);
        assert_eq!(
            m.find_proto_vertex_within(&k, query, tol, None).unwrap(),
            Some(gone)
        );
        m.combine_proto_vertex(kept, gone).unwrap();
        m.remove_proto_vertex(gone).unwrap();
        m.delete_proto_vertex_object(gone);
        // (no vertex definitions to average: the survivor keeps its own position)
        m.proto_vertex_mut(kept).unwrap().set_position(Point3::ZERO);
        assert_eq!(
            m.find_proto_vertex_within(&k, query, tol, None).unwrap(),
            Some(kept)
        );
        // a point the region does not reach stays apart
        let off = Point3::new(4.5e-4, 1e-4, 0.0);
        assert_eq!(
            m.find_proto_vertex_within(&k, off, tol, None).unwrap(),
            None
        );
    }

    #[test]
    fn a_forgotten_proto_edge_leaves_every_face_and_overlap_list() {
        // 93efd6d (#369): SplitIntersectingProtoEdges deleted a combined-away proto
        // edge that proto faces and other proto edges still listed
        let k = Kernel::default();
        let mut m = manager(&k);
        let ivl = Extent1d::from_raw(0.0, 1.0);
        let [kept, gone, other] = [(); 3].map(|()| m.new_proto_edge(&k, None, None, ivl).unwrap());
        for pe in [kept, gone, other] {
            m.add_proto_edge(pe);
        }
        let f1 = m.new_proto_face(None, None, &[gone, other, gone]).unwrap();
        let f2 = m.new_proto_face(None, None, &[kept, gone]).unwrap();
        m.add_proto_face(f1);
        m.add_proto_face(f2);
        m.proto_edge_mut(other).unwrap().add_overlapping_pe(gone);
        m.proto_edge_mut(kept).unwrap().add_overlapping_pe(gone);
        m.proto_edge_mut(kept).unwrap().add_overlapping_pe(other);
        let coinc = |m: &ProtoTopologyManager<H>, pf| {
            m.proto_face(pf).unwrap().coinc_proto_edges().to_vec()
        };

        // the faces gone bounded are bounded by kept, once
        m.hand_over_coincident_proto_edge(kept, gone);
        assert_eq!(coinc(&m, f1), vec![gone, other, gone, kept]);
        assert_eq!(coinc(&m, f2), vec![kept, gone]);
        // unlisting alone keeps upstream's lists
        m.remove_proto_edge(gone).unwrap();
        assert_eq!(coinc(&m, f2), vec![kept, gone]);
        assert_eq!(m.proto_edges(), &[kept, other]);
        // the library's delete drops every reference first
        m.forget_proto_edge(gone);
        let _ = m.delete_proto_edge_object(gone);
        assert_eq!(coinc(&m, f1), vec![other, kept]);
        assert_eq!(coinc(&m, f2), vec![kept]);
        assert!(m.proto_edge(other).unwrap().overlapping_pes().is_empty());
        assert_eq!(m.proto_edge(kept).unwrap().overlapping_pes(), &[other]);
    }
}
