//! Curve classification against 3D edges and trimmed face boundaries.
//!
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`CurveClassification::intersect_edges`] | `SmCurveClassification::IntersectEdges` |
//! | [`CurveClassification::intersect_ts_bound`] | `SmCurveClassification::IntersectTSBound` |
//!
//! Deviations: topology is supplied as borrowed views resolved from arena ids, and
//! the caller supplies the face's trim cache. Cached marks are local ordered lists
//! of visited handles. No input-driven recursion is added. Temporary surface-owner
//! clearing is represented by a private copy for a lifted 2D source. Missing generic
//! curve-on-surface dispatch and mutable edge insertion retain their dependency
//! deferrals; no synthetic classification is inserted. Ignored native evaluation
//! and drop statuses preserve initialized outputs; unported errors propagate. The
//! native checked-drop guess aliases its output and is reset to zero before solving;
//! the Rust call explicitly uses that zero guess. A 3D trim-boundary source returns
//! `InvalidInput` instead of upstream's debug-only assertion.

use super::p1::{cull_or_refine_cci_in_3d, src_zone_tol_3d_of};
use super::p5::insert_solutions;
use super::CurveClassification;
use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p5::make_curve_cache;
use crate::curve::curve::p6::DroppedPoint;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::Curve;
use crate::error::{ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use crate::geom::tol::{xsect_tol3d, xsect_tol_3d_from_zones};
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, PointClassType, PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{max2, min2, Point2, Point3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::surface::surface::p1::{map_to_2d, map_to_2d_on_curve};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::surface_cache::p1::FaceContainment;
use crate::surface::trim_srf_cache::TrimSrfCache;

const OP: &str = "intersect::curve_class::p9";

/// Read-only `SmEdge` data, resolved through its owning B-rep.
#[derive(Debug, Clone, Copy)]
pub struct BoundaryEdge<O> {
    /// Edge classification handle.
    pub edge: O,
    /// Handle of the edge's actual curve (preserves pointer identity).
    pub curve: O,
    /// Edge parameter interval.
    pub interval: Extent1d,
    /// Stored edge zone tolerance (`SmEdge::GetTolerance`).
    pub zone_tol_3d: f64,
    /// Maximum gap for this face's edgeuse; `None` means no such edgeuse.
    pub face_gap: Option<f64>,
}

/// A vertexuse and its vertex, with its computed UV point.
#[derive(Debug, Clone, Copy)]
pub struct BoundaryVertex<O> {
    /// Vertexuse identity, used to deduplicate cached node lists.
    pub vertexuse: O,
    /// Vertex classification handle.
    pub vertex: O,
    /// `ComputeUVPoint` (fallback) or the cached UV point.
    pub uv: Point2,
    /// Vertex position (`SmVertex::GetPoint`).
    pub point: Point3,
    /// Stored vertex zone tolerance (`SmVertex::GetTolerance`).
    pub zone_tol_3d: f64,
}

/// Read-only edgeuse data, with the real UV trim curve.
#[derive(Debug, Clone, Copy)]
pub struct BoundaryEdgeuse<O> {
    /// Edgeuse identity, used to deduplicate cached node lists.
    pub edgeuse: O,
    /// Edge and its 3D curve.
    pub edge: BoundaryEdge<O>,
    /// UV trim curve handle. A missing lazy trim defers to its owning unit.
    pub trim_curve: Option<O>,
}

/// One upward loopuse's children in native traversal order.
#[derive(Debug, Clone)]
pub struct BoundaryLoop<O> {
    /// `GetVertexuses`, in native order (may repeat across loops).
    pub vertices: Vec<BoundaryVertex<O>>,
    /// `GetEdgeuses`, in native order.
    pub edges: Vec<BoundaryEdgeuse<O>>,
}

/// Resolved upward faceuse traversal and the face's actual surface and trim cache.
/// These views borrow geometry; they do not replace the B-rep arenas.
pub struct BoundaryFace<'a, O> {
    /// Face surface.
    pub surface: &'a Surface,
    /// Upward loopuses, in native order.
    pub loops: &'a [BoundaryLoop<O>],
    /// Caller-owned trimmed surface cache (`GetSurfaceCache`).
    pub cache: &'a TrimSrfCache<O>,
}

/// `IntersectEdges` flags. `has_big_gaps` is required upstream; this Rust options
/// struct defaults it to false. `uv_space_okay` has the native default true.
#[derive(Debug, Clone, Copy)]
pub struct IntersectEdgesOptions {
    /// Increase intersection tolerance for known edge/face gaps.
    pub has_big_gaps: bool,
    /// Permit UV-space intersection; false lifts a 2D source onto the surface.
    pub uv_space_okay: bool,
}
impl Default for IntersectEdgesOptions {
    fn default() -> Self {
        Self {
            has_big_gaps: false,
            uv_space_okay: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Source,
    Target,
}
impl SolutionObject for Side {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

fn curve<O: ClassObject>(g: &impl ClassGeometry<O>, handle: O) -> Result<&Curve> {
    g.curve(handle).ok_or(SolidError::null_pointer(OP))
}
fn lifted(k: &Kernel, uv: &Curve, surface: &Surface) -> Result<Curve> {
    let SurfaceKind::BSpline(bs) = surface.kind() else {
        // DISPATCH GAP(curve.crv_on_surf): owned generic Surface in SmCrvOnSurf.
        return Err(SolidError::unported(
            "SmCrvOnSurf over a generic Surface",
            "curve.crv_on_surf",
        ));
    };
    Ok(Curve::from(CrvOnSurf::new(
        k,
        uv.clone(),
        bs.clone(),
        Some(surface.natural_uv_domain()),
        0,
    )?))
}
fn checked_vertex_drop(
    k: &Kernel,
    lift: &Curve,
    interval: &Extent1d,
    point: Point3,
    tolerance: f64,
) -> Result<DroppedPoint> {
    // IntersectTSBound passes &dParam as the guess and dParam as the output.
    // SmCrvOnSurf::DropPoint initializes its output to 0 before reading that
    // aliased guess. Preserve the observed zero seed, not the earlier UV result.
    Ok(ignored(lift.drop_point(
        k,
        interval,
        point,
        None,
        tolerance,
        Some(0.0),
        SolverOperation::Intersect,
    ))?
    .unwrap_or_default())
}
fn named<O: Copy>(s: &Solution<Side>, source: O, target: O) -> Solution<O> {
    let mut out = Solution::new();
    out.solution_type = s.solution_type;
    out.num_variables = s.num_variables;
    out.num_objects = s.num_objects;
    out.start = s.start;
    out.end = s.end;
    out.nodes = s.nodes;
    for (to, from) in out.objects.iter_mut().zip(s.objects) {
        *to = from.map(|x| match x {
            Side::Source => source,
            Side::Target => target,
        });
    }
    out
}

impl<O: ClassObject> CurveClassification<O> {
    /// Intersects the source curve with each edge, appending classifications in order.
    /// A shared curve uses the exact interval-overlap shortcut before insertion.
    ///
    /// # Errors
    /// Missing geometry fails; unavailable generic surface lifting or mutable edge
    /// insertion defers to its dependency. Earlier successful insertions are retained.
    pub fn intersect_edges(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        edges: &[BoundaryEdge<O>],
        surface: &Surface,
        options: IntersectEdgesOptions,
    ) -> Result<()> {
        let source_handle = self.curve().ok_or(SolidError::null_pointer(OP))?;
        let source = curve(geometry, source_handle)?;
        let lift;
        let working = if !options.uv_space_okay && source.dim() == 2 {
            let mut unowned = surface.clone();
            *unowned.owner_mut() = None;
            lift = lifted(k, source, &unowned)?;
            &lift
        } else {
            source
        };
        let source_zone = src_zone_tol_3d_of(k, Some(self));
        for edge in edges {
            let target = curve(geometry, edge.curve)?;
            let edge_zone = edge.zone_tol_3d;
            let mut tolerance = xsect_tol_3d_from_zones(k, edge_zone, source_zone);
            if options.has_big_gaps {
                if let Some(gap) = edge.face_gap {
                    if gap > tolerance {
                        tolerance = gap * 1.1;
                    }
                }
            }
            let mut solutions = Vec::new();
            if core::ptr::eq(target, working) {
                let lo = max2(edge.interval.min(), self.interval().min());
                let hi = min2(edge.interval.max(), self.interval().max());
                if lo < hi {
                    let mut sol = Solution::new();
                    sol.solution_type = SolutionType::RangeOfValues;
                    sol.num_variables = 2;
                    sol.num_objects = 2;
                    for slot in sol.objects.iter_mut().take(2) {
                        *slot = Some(edge.curve);
                    }
                    for i in 0..2 {
                        *sol.start.param_mut(i)? = lo;
                        *sol.end.param_mut(i)? = hi;
                    }
                    solutions.push(sol);
                }
            } else {
                let found = working.global_curve_intersect(
                    k,
                    &self.interval(),
                    target,
                    &edge.interval,
                    tolerance,
                    &CurveIntersectOptions {
                        handles: [Some(Side::Source), Some(Side::Target)],
                        ..CurveIntersectOptions::default()
                    },
                )?;
                solutions.extend(
                    found
                        .as_slice()
                        .iter()
                        .map(|s| named(s, source_handle, edge.curve)),
                );
            }
            if !solutions.is_empty() {
                insert_solutions(
                    k,
                    geometry,
                    self,
                    &mut solutions,
                    edge.edge,
                    Some(tolerance),
                )?;
            }
        }
        Ok(())
    }

    /// Intersects a 2D source with a face's trim vertices and curves. A valid trim
    /// cache restricts work to intersecting leaf nodes; otherwise all loopuses run.
    /// `do_check_3d` also verifies vertex hits in 3D. Trim-curve hits always refine in 3D.
    ///
    /// # Errors
    /// Missing geometry/cache records fail; generic surface lifting and mutable
    /// edge insertion retain their existing dependency deferrals.
    pub fn intersect_ts_bound(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        face: &BoundaryFace<'_, O>,
        do_check_3d: bool,
    ) -> Result<()> {
        let source_handle = self.curve().ok_or(SolidError::null_pointer(OP))?;
        let source = curve(geometry, source_handle)?;
        if source.dim() != 2 {
            return Err(SolidError::invalid_input(
                OP,
                "trim boundary source must be 2D",
            ));
        }
        let base = face.cache.base();
        if base.face_was_modified
            || !base.have_t_surface_cache
            || base.face_containment_done == FaceContainment::NotDone
        {
            for lp in face.loops {
                for vertex in &lp.vertices {
                    self.p9_vertex(k, geometry, source, face.surface, *vertex, do_check_3d)?;
                }
                for edgeuse in &lp.edges {
                    self.p9_edgeuse(k, geometry, source_handle, face.surface, *edgeuse, false)?;
                }
            }
            return Ok(());
        }
        let cache = make_curve_cache::<Side>(k, source, Some(Side::Source))?;
        let tree = cache.tree().ok_or(SolidError::null_pointer(OP))?;
        let top = tree.top_node().ok_or(SolidError::null_pointer(OP))?;
        let mut domain = Extent2d::from_extent3d(&tree.node(top)?.bbox);
        domain.expand_absolute(EFF_ZERO_SQRT);
        let nodes = base.find_uv_nodes_built(k, &domain, false)?;
        let tree = base.tree().ok_or(SolidError::null_pointer(OP))?;
        let mut visited = Vec::new();
        for &id in &nodes {
            if tree.node(id)?.child1.is_some() {
                continue;
            }
            for item in &base.aux(id)?.vertex_list {
                let vu = item.vertexuse.ok_or(SolidError::null_pointer(OP))?;
                if visited.contains(&vu) {
                    continue;
                }
                visited.push(vu);
                let mut vertex = face
                    .loops
                    .iter()
                    .flat_map(|lp| &lp.vertices)
                    .find(|v| v.vertexuse == vu)
                    .copied()
                    .ok_or(SolidError::null_pointer(OP))?;
                vertex.uv = item.uv_point;
                self.p9_vertex(k, geometry, source, face.surface, vertex, do_check_3d)?;
            }
        }
        for &id in &nodes {
            if tree.node(id)?.child1.is_some() {
                continue;
            }
            for &eu in &base.aux(id)?.edgeuse_list {
                if visited.contains(&eu) {
                    continue;
                }
                visited.push(eu);
                let edgeuse = face
                    .loops
                    .iter()
                    .flat_map(|lp| &lp.edges)
                    .find(|e| e.edgeuse == eu)
                    .copied()
                    .ok_or(SolidError::null_pointer(OP))?;
                self.p9_edgeuse(k, geometry, source_handle, face.surface, edgeuse, true)?;
            }
        }
        Ok(())
    }

    fn p9_vertex(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        source: &Curve,
        surface: &Surface,
        vertex: BoundaryVertex<O>,
        do_check_3d: bool,
    ) -> Result<()> {
        let source_zone = src_zone_tol_3d_of(k, Some(self));
        let zone = vertex.zone_tol_3d;
        let tolerance = xsect_tol_3d_from_zones(k, source_zone, zone);
        let singular = surface
            .is_singularity(k, vertex.uv, EFF_ZERO, false)?
            .singular;
        let lift;
        let (drop_curve, point, distance_tol) = if singular {
            lift = lifted(k, source, surface)?;
            let point = ignored(surface.evaluate_point(vertex.uv))?.unwrap_or(Point3::UNDEF);
            (&lift, point, tolerance)
        } else {
            (
                source,
                Point3::from(vertex.uv),
                xsect_tol3d(k, map_to_2d(tolerance, vertex.uv, surface)?),
            )
        };
        let mut found = drop_curve.drop_point(
            k,
            &self.interval(),
            point,
            None,
            distance_tol,
            None,
            SolverOperation::Intersect,
        )?;
        if !found.success {
            return Ok(());
        }
        let mut distance_3d = 0.0;
        if do_check_3d {
            let lift = lifted(k, source, surface)?;
            let point = vertex.point;
            // Native ignores the status, but DropPoint initializes success to false.
            // A failed checked solve must not retain the earlier UV success.
            found = checked_vertex_drop(k, &lift, &self.interval(), point, tolerance)?;
            let on_surface =
                ignored(lift.evaluate_point(found.parameter))?.unwrap_or(Point3::UNDEF);
            distance_3d = point.distance(on_surface);
        }
        if found.success && (!do_check_3d || distance_3d < tolerance) {
            let mut pc = PointClassification::new(source_zone);
            pc.set_class_object(PointClassType::Vertex, Some(vertex.vertex));
            pc.set_gap_3d(found.distance);
            pc.set_src_zone_tol_3d(source_zone);
            pc.set_pre_snap_param(found.parameter);
            self.insert_point_class(k, geometry, &pc, found.parameter, BIG_DOUBLE, false)?;
        }
        Ok(())
    }

    fn p9_edgeuse(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        source_handle: O,
        surface: &Surface,
        eu: BoundaryEdgeuse<O>,
        cached: bool,
    ) -> Result<()> {
        let trim_handle = eu.trim_curve.ok_or_else(|| {
            if !cached {
                return SolidError::null_pointer(OP);
            }
            // UNPORTED(topology.edgeuse_p3): SmEdgeuse::GetOrCreateUVTrimCurve
            SolidError::unported("SmEdgeuse::GetOrCreateUVTrimCurve", "topology.edgeuse_p3")
        })?;
        let trim = curve(geometry, trim_handle)?;
        let source = curve(geometry, source_handle)?;
        let source_zone = src_zone_tol_3d_of(k, Some(self));
        let edge_zone = eu.edge.zone_tol_3d;
        let (tol3d, tol2d) = if !cached && self.doing_cross_hatch() {
            let tol = xsect_tol_3d_from_zones(k, source_zone, source_zone);
            (tol, tol)
        } else {
            let tol = xsect_tol_3d_from_zones(k, source_zone, edge_zone);
            (
                tol,
                xsect_tol3d(
                    k,
                    map_to_2d_on_curve(
                        k,
                        tol,
                        trim.natural_interval().evaluate(0.5),
                        trim,
                        surface,
                    )?,
                ),
            )
        };
        let mut solutions = source.global_curve_intersect(
            k,
            &self.interval(),
            trim,
            &trim.natural_interval(),
            tol2d,
            &CurveIntersectOptions {
                handles: [Some(Side::Source), Some(Side::Target)],
                ..CurveIntersectOptions::default()
            },
        )?;
        if !solutions.is_empty() {
            cull_or_refine_cci_in_3d(
                k,
                surface,
                source,
                &self.interval(),
                curve(geometry, eu.edge.curve)?,
                &eu.edge.interval,
                tol3d,
                &mut solutions,
            )?;
            let mut named_solutions: Vec<_> = solutions
                .as_slice()
                .iter()
                .map(|s| named(s, source_handle, trim_handle))
                .collect();
            insert_solutions(
                k,
                geometry,
                self,
                &mut named_solutions,
                eu.edge.edge,
                Some(tol2d),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::intersect::point_class::ClassObjectKind;
    use crate::math::Vec3;
    use crate::surface::plane::Plane;
    use crate::surface::surface_cache::p1::SurfaceCacheOptions;
    use crate::surface::volume::Volume;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum H {
        Source,
        Target,
        Edge,
    }
    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            if *self == Self::Edge {
                ClassObjectKind::Edge
            } else {
                ClassObjectKind::Curve
            }
        }
    }
    struct Geometry {
        source: Curve,
        target: Curve,
    }
    impl ClassGeometry<H> for Geometry {
        fn topology_zone_tol_3d(&self, h: H) -> Option<f64> {
            (h == H::Edge).then_some(1e-6)
        }
        fn curve(&self, h: H) -> Option<&Curve> {
            match h {
                H::Source => Some(&self.source),
                H::Target => Some(&self.target),
                H::Edge => None,
            }
        }
        fn surface(&self, _: H) -> Option<&Surface> {
            None
        }
        fn volume(&self, _: H) -> Option<&Volume> {
            None
        }
        fn point(&self, _: H) -> Option<Point3> {
            None
        }
    }
    fn geometry() -> Geometry {
        let line = |a: Vec3, dir: Vec3| {
            Curve::from(Line::bounded(
                a,
                dir,
                &Extent1d::from_min_max(0., 1.),
                &BoundedLineOptions {
                    scale: 1.,
                    dim: 3,
                    ..Default::default()
                },
            ))
        };
        Geometry {
            source: line(Vec3::ZERO, Vec3::new(1., 0., 0.)),
            target: line(Vec3::new(0.5, -0.5, 2.1e-6), Vec3::new(0., 1., 0.)),
        }
    }
    fn classification(k: &Kernel) -> CurveClassification<H> {
        CurveClassification::with_curve_interval(k, H::Source, Extent1d::from_min_max(0., 1.), 1e-6)
    }
    #[test]
    fn face_gap_threshold_changes_the_intersection_tolerance() {
        let k = Kernel::default();
        let geometry = geometry();
        let surface = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::new(0., 0., 1.)));
        let mut edge = BoundaryEdge {
            edge: H::Edge,
            curve: H::Target,
            interval: Extent1d::from_min_max(0., 1.),
            face_gap: Some(2e-6),
            zone_tol_3d: 1e-6,
        };
        let options = IntersectEdgesOptions {
            has_big_gaps: true,
            ..Default::default()
        };
        let mut cc = classification(&k);
        let original = cc.intervals().to_vec();
        cc.intersect_edges(&k, &geometry, &[edge], &surface, options)
            .unwrap();
        assert_eq!(cc.intervals(), original);
        edge.face_gap = Some(2.01e-6);
        let error = cc
            .intersect_edges(&k, &geometry, &[edge], &surface, options)
            .unwrap_err();
        assert!(matches!(
            error,
            SolidError::Unported {
                unit: "intersect.curve_class_p2",
                ..
            }
        ));
        assert_eq!(cc.intervals(), original);
    }
    #[test]
    fn missing_trim_does_not_mean_lazy_creation_in_the_fallback() {
        let k = Kernel::default();
        let mut geometry = geometry();
        geometry.source = Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            &Extent1d::from_min_max(0., 1.),
            &BoundedLineOptions {
                dim: 2,
                ..Default::default()
            },
        ));
        let surface = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::new(0., 0., 1.)));
        let cache = TrimSrfCache::new(surface.clone(), SurfaceCacheOptions::default());
        let loops = [BoundaryLoop {
            vertices: Vec::new(),
            edges: vec![BoundaryEdgeuse {
                edgeuse: H::Edge,
                edge: BoundaryEdge {
                    edge: H::Edge,
                    curve: H::Target,
                    interval: Extent1d::from_min_max(0., 1.),
                    face_gap: None,
                    zone_tol_3d: 1e-6,
                },
                trim_curve: None,
            }],
        }];
        let mut cc = classification(&k);
        assert!(matches!(
            cc.intersect_ts_bound(
                &k,
                &geometry,
                &BoundaryFace {
                    surface: &surface,
                    loops: &loops,
                    cache: &cache
                },
                false
            ),
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
    }
    #[test]
    fn checked_drop_preserves_the_native_aliased_zero_guess() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as NlSurface;
        let k = Kernel::default();
        let net = (0..2)
            .map(|u| {
                (0..2)
                    .map(|v| CPoint::new(1e6 * f64::from(u), 1e6 * f64::from(v), 0., NOW))
                    .collect()
            })
            .collect();
        let knots = vec![0., 0., 1., 1.];
        let surface = Surface::from(BSplineSurface::from_nurb(
            NlSurface::new(net, 1, 1, knots.clone(), knots).unwrap(),
            false,
        ));
        let source = Curve::from(Line::from_points(
            Vec3::new(0.2, 0.5, 0.),
            Vec3::new(0.8, 0.5, 0.),
            2,
        ));
        let lift = lifted(&k, &source, &surface).unwrap();
        // Native aliased-call probes return these exact parameter/gap pairs.
        // Keeping the UV guess instead returns gap 0 for both points.
        for (u, parameter) in [
            (0.6315271935871615, 0.7192119893119358),
            (0.7485310025864422, 0.9142183376440705),
        ] {
            let found = checked_vertex_drop(
                &k,
                &lift,
                &Extent1d::from_min_max(0., 1.),
                Vec3::new(1e6 * u, 5e5, 0.),
                0.19999999999999998,
            )
            .unwrap();
            assert!(found.success);
            assert_eq!(found.parameter, parameter);
            assert_eq!(found.distance, 1.1641532182693481e-10);
        }
    }
}
