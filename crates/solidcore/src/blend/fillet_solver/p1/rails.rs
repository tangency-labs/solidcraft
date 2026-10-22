//! Typed pseudo-B-rep rail split and support-curve operations.
//!
//! Upstream: `source/SMLib/src/SmFilletSolver.cpp` (5999–6064, 6342–6504).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`split_rail`] | `sm_SplitRail` |
//! | [`compute_support_edge`] | `sm_ComputeSupportEdge` |
//!
//! Deviations: typed arena ids and caller-owned notification routing. Edge
//! construction delegates to the ported fillet geometry builder; surface
//! intersection calls the ported `blend::fillet_geom::p2::srf_srf_intersection`.
//! No recursion.
use crate::blend::fillet_geom::p1::{
    CliffHandle, FilStatus, FilletBrep, FilletEdgeType, FilletGeomId, FilletVertex,
};
use crate::curve::curve::{p2::OwnerChange, Curve};
use crate::error::{ignored, Result, SolidError};
use crate::geom::{global_solver::SolverOperation, object::ObjectRef};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::surface::surface::Surface;
use crate::topology::{edge, edgeuse::p1 as eu, EdgeId, Vertex, VertexId};
const OP: &str = "blend::fillet_solver::p1::rails";

/// Caller-owned object identities and notification routing for new rail curves.
pub trait RailOwnership {
    /// Resolve the actual owner identity and its optional B-rep identity.
    fn edge_owner(&self, edge: EdgeId) -> Result<(ObjectRef, Option<ObjectRef>)>;
    /// Deliver the notification produced by `Curve::set_owner`.
    fn owner_changed(&mut self, change: OwnerChange) -> Result<()>;
}
pub(crate) struct SupportIntersection<'a> {
    pub start: VertexId,
    pub end: Option<VertexId>,
    pub start_direction: Option<Vec3>,
    pub end_direction: Option<Vec3>,
    pub surfaces: [&'a Surface; 2],
    pub approx_tol: f64,
    pub angle_tol: f64,
    pub skip_two_points_intersection: bool,
    pub reference_point: Option<Point3>,
}

pub(crate) trait RailOps {
    fn make_edge(
        &mut self,
        _k: &Kernel,
        fb: &mut FilletBrep,
        start: VertexId,
        end: VertexId,
    ) -> Result<EdgeId> {
        fb.make_fillet_edge(Some(start), Some(end), None)
    }
    fn surface_intersection(
        &mut self,
        k: &Kernel,
        fb: &mut FilletBrep,
        request: SupportIntersection<'_>,
    ) -> Result<Option<Curve>> {
        let SupportIntersection {
            start,
            end,
            mut start_direction,
            end_direction,
            surfaces,
            approx_tol,
            angle_tol,
            skip_two_points_intersection,
            reference_point,
        } = request;
        let [first, second] = surfaces;
        crate::blend::fillet_geom::p2::srf_srf_intersection(
            k,
            Some(fb.vertex_point(start)?),
            end.map(|v| fb.vertex_point(v)).transpose()?,
            start_direction.as_mut(),
            end_direction,
            first,
            second,
            approx_tol,
            angle_tol,
            crate::blend::fillet_geom::p2::SrfSrfOptions {
                skip_two_pnts_intersection: skip_two_points_intersection,
                ref_point: reference_point,
            },
        )
        .map(|curves| curves.curve_3d)
    }
}
pub(crate) struct Missing;
impl RailOps for Missing {}

/// Split a rail in the pseudo-B-rep, preserving ownership and partial mutations.
///
/// # Errors
/// Missing topology or geometry, curve-copy failures, or unavailable edge construction.
pub fn split_rail(
    k: &Kernel,
    fillets: &mut FilletBrep,
    ownership: &mut dyn RailOwnership,
    geom: FilletGeomId,
    rail: EdgeId,
) -> Result<(VertexId, EdgeId)> {
    split_rail_with_ops(k, fillets, ownership, geom, rail, &mut Missing)
}
pub(crate) fn split_rail_with_ops(
    k: &Kernel,
    fillets: &mut FilletBrep,
    ownership: &mut dyn RailOwnership,
    geom: FilletGeomId,
    rail: EdgeId,
    ops: &mut impl RailOps,
) -> Result<(VertexId, EdgeId)> {
    Rails {
        k,
        fillets,
        ownership,
    }
    .split_rail(geom, rail, ops)
}
/// Compute, trim and install a support curve between its adjacent fillet surfaces.
///
/// # Errors
/// Missing topology/surfaces, failed intersection/drop/trim, or unavailable surface intersection.
pub fn compute_support_edge(
    k: &Kernel,
    fillets: &mut FilletBrep,
    ownership: &mut dyn RailOwnership,
    support: EdgeId,
    geom: FilletGeomId,
    approx: f64,
    angle: f64,
) -> Result<()> {
    compute_support_edge_with_ops(
        k,
        fillets,
        ownership,
        support,
        geom,
        approx,
        angle,
        &mut Missing,
    )
}
pub(crate) fn compute_support_edge_with_ops(
    k: &Kernel,
    fillets: &mut FilletBrep,
    ownership: &mut dyn RailOwnership,
    support: EdgeId,
    geom: FilletGeomId,
    approx: f64,
    angle: f64,
    ops: &mut impl RailOps,
) -> Result<()> {
    Rails {
        k,
        fillets,
        ownership,
    }
    .support_edge(support, geom, approx, angle, ops)
}
fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| SolidError::null_pointer(OP))
}
struct Rails<'a> {
    k: &'a Kernel,
    fillets: &'a mut FilletBrep,
    ownership: &'a mut dyn RailOwnership,
}
impl Rails<'_> {
    fn start(&self, rail: EdgeId) -> Result<VertexId> {
        required(edge::vertex(self.fillets.brep(), rail)?)
    }
    fn end(&self, rail: EdgeId) -> Result<VertexId> {
        let start = self.start(rail)?;
        required(edge::other_vertex(self.fillets.brep(), rail, Some(start))?)
    }
    fn install_curve(&mut self, edge: EdgeId, curve: Curve) -> Result<()> {
        crate::blend::fillet_geom::p1::set_rail_curve(self.fillets.brep_mut(), edge, curve)?;
        let (owner, brep) = self.ownership.edge_owner(edge)?;
        let curve = required(self.fillets.brep_mut().edge_mut(edge)?.curve.as_mut())?;
        let (_, notification) = curve.set_owner(Some(owner), brep);
        if let Some(notification) = notification {
            self.ownership.owner_changed(notification)?;
        }
        Ok(())
    }

    fn split_rail(
        &mut self,
        geom: FilletGeomId,
        rail: EdgeId,
        ops: &mut impl RailOps,
    ) -> Result<(VertexId, EdgeId)> {
        let start = self.start(rail)?;
        let end = self.end(rail)?;
        let mut edgeuse = required(eu::primary_edgeuse(self.fillets.brep(), rail)?)?;
        let mut vu = required(self.fillets.brep().edgeuse(edgeuse)?.vertexuse)?;
        if self.fillets.brep().vertexuse(vu)?.vertex != Some(end) {
            edgeuse = required(eu::mate(self.fillets.brep(), edgeuse)?)?;
            vu = required(self.fillets.brep().edgeuse(edgeuse)?.vertexuse)?;
        }
        if start == end
            && self.fillets.brep().edgeuse(edgeuse)?.orientation
                != crate::nurbs::b_spline_surface_n_lib::p2::OrientType::Opposite
        {
            edgeuse = required(eu::mate(self.fillets.brep(), edgeuse)?)?;
            vu = required(self.fillets.brep().edgeuse(edgeuse)?.vertexuse)?;
        }
        self.fillets
            .set_edgeuse_uv_curve(edgeuse, None, true, true)?;
        self.fillets
            .brep_mut()
            .vertex_mut(end)?
            .vertexuses
            .retain(|v| *v != vu);
        // SmVertex() starts at the origin with tolerance 0, independent of
        // the system zone tolerance used by Brep::add_vertex's convenience API.
        let vertex = self
            .fillets
            .brep_mut()
            .add_vertex_record(Vertex::default())?;
        self.fillets
            .insert_fillet_vertex(vertex, FilletVertex::new(self.k, None));
        self.fillets
            .brep_mut()
            .vertex_mut(vertex)?
            .vertexuses
            .push(vu);
        self.fillets.brep_mut().vertexuse_mut(vu)?.vertex = Some(vertex);
        let (new_start, new_end) = edge::start_end_vertices(self.fillets.brep(), rail)?;
        self.fillets.brep_mut().edge_mut(rail)?.vertices =
            [required(new_start)?, required(new_end)?];
        let new_rail = ops.make_edge(self.k, self.fillets, vertex, end)?;
        let original_face = self.fillets.fillet_edge(rail)?.original_face();
        let record = self.fillets.fillet_edge_mut(new_rail)?;
        record.set_fillet_edge_type(FilletEdgeType::Rail);
        record.set_original_face(original_face);
        let mut new_eu = required(eu::primary_edgeuse(self.fillets.brep(), new_rail)?)?;
        let new_vu = required(self.fillets.brep().edgeuse(new_eu)?.vertexuse)?;
        if self.fillets.brep().vertexuse(new_vu)?.vertex != Some(end) {
            new_eu = required(eu::mate(self.fillets.brep(), new_eu)?)?;
        }
        self.fillets.set_edgeuse_fillet_geom(new_eu, Some(geom))?;
        let curve = required(edge::curve(self.fillets.brep(), rail)?)?.copy(self.k)?;
        self.install_curve(new_rail, curve)?;
        let tolerance = edge::tolerance(self.fillets.brep(), rail)?;
        edge::set_tolerance(self.fillets.brep_mut(), new_rail, tolerance, true, true)?;
        Ok((vertex, new_rail))
    }

    fn support_edge(
        &mut self,
        support: EdgeId,
        geom: FilletGeomId,
        approx: f64,
        angle: f64,
        ops: &mut impl RailOps,
    ) -> Result<()> {
        let start = self.start(support)?;
        if !self.fillets.fillet_vertex(start)?.is_processed() {
            return Err(SolidError::sm_err(OP));
        }
        let start_point = self.fillets.vertex_point(start)?;
        let primary = required(eu::primary_edgeuse(self.fillets.brep(), support)?)?;
        let mate = required(eu::mate(self.fillets.brep(), primary)?)?;
        let first = required(self.fillets.fillet_edgeuse(primary)?.fillet_geom())?;
        let second = required(self.fillets.fillet_edgeuse(mate)?.fillet_geom())?;
        let surf1 = required(self.fillets.geom(first)?.fillet_surface())?.clone();
        let surf2 = required(self.fillets.geom(second)?.fillet_surface())?.clone();
        let outcome = ops.surface_intersection(
            self.k,
            self.fillets,
            SupportIntersection {
                start,
                end: None,
                start_direction: None,
                end_direction: None,
                surfaces: [&surf1, &surf2],
                approx_tol: approx,
                angle_tol: angle,
                skip_two_points_intersection: false,
                reference_point: None,
            },
        );
        let mut curve = match outcome {
            Err(e) if e.is_unported() => return Err(e),
            Ok(Some(curve)) => curve,
            _ => {
                self.fillets
                    .fillet_edge_mut(support)?
                    .set_status(FilStatus::SurfIntFailure);
                return Err(SolidError::sm_err(OP));
            }
        };
        let mut interval = curve.natural_interval();
        let surface = required(self.fillets.geom(geom)?.fillet_surface())?;
        let solutions = surface.global_curve_intersect(
            self.k,
            [CliffHandle::Curve(&curve), CliffHandle::Surface(surface)],
            &surface.natural_uv_domain(),
            &curve,
            &interval,
            approx,
        )?;
        let sol = solutions.get(0).ok_or_else(|| SolidError::sm_err(OP))?;
        let t = sol
            .start
            .parameters
            .first()
            .copied()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let end_point = curve.evaluate_point(self.k, t)?;
        let end = self.end(support)?;
        self.fillets.brep_mut().vertex_mut(end)?.point = end_point;
        self.fillets
            .fillet_vertex_mut(end)?
            .set_status(FilStatus::Processed);
        let drop1 = curve.drop_point(
            self.k,
            &interval,
            start_point,
            None,
            approx,
            None,
            SolverOperation::Minimize,
        )?;
        if !drop1.success {
            return Err(SolidError::sm_err(OP));
        }
        let drop2 = curve.drop_point(
            self.k,
            &interval,
            end_point,
            None,
            approx,
            None,
            SolverOperation::Minimize,
        )?;
        if !drop2.success {
            return Err(SolidError::sm_err(OP));
        }
        let reverse = drop1.parameter > drop2.parameter;
        if reverse {
            ignored(interval.set_min_max(drop2.parameter, drop1.parameter))?;
        } else {
            ignored(interval.set_min_max(drop1.parameter, drop2.parameter))?;
        }
        curve.trim(
            self.k,
            &mut interval,
            crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
        )?;
        if reverse {
            curve.reverse_parameterization(self.k, &interval)?;
        }
        self.install_curve(support, curve)
    }
}
