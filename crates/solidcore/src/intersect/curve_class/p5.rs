//! Curve classification, part 5: intersection insertion, geometry extraction and merging.
//!
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`insert_solutions`] | `sm_InsertSolutions` (static) |
//! | [`CurveClassification::extract_geometry`] | `SmCurveClassification::ExtractGeometry` |
//! | [`CurveClassification::fixup_classification`] | `SmCurveClassification::FixupClassification` |
//! | [`CurveClassification::insert_topology_intersections`] | `SmCurveClassification::InsertTopologyIntersections` |
//! | [`CurveClassification::intersect_vertices`] | `SmCurveClassification::IntersectVertices` |
//! | [`CurveClassification::merge_classifications`] | `SmCurveClassification::MergeClassifications` |
//! | [`CurveClassification::merge_intervals`] | `SmCurveClassification::MergeIntervals` |
//!
//! Deviations: output lists become owned results; the crate-private merge variant
//! retains completed intervals' partial output in its error for ignored-status
//! callers. Handles and geometry use the part-1
//! model. Optional [`ClassGeometry`] callbacks resolve vertex points, topology tolerances
//! and vertex/face gaps. Opaque handles still defer. The SSS refinement block
//! (upstream lines 14826–15095) needs mutable curve/topology ownership and defers
//! before editing geometry; this block and the edge branch of `insert_solutions`
//! remain unported. That block of `IntersectVertices` (SmCurveClass.cpp
//! 14826-15095) is this unit's own unported code (`UNPORTED(intersect.curve_class_p5)`);
//! the mate lookups it ends with are ported in `boolean.topology_intersector_p1`.
//! Ordinary vertex projections are
//! compared; large-gap projections remain uncertified because the oracle vertices
//! have no vertexuse and its Rust adapter supplies no vertex/face gap.
//! Part 2 remains an explicit dependency; parts 6/7 are wired from main. The fixup
//! restart loop is bounded by three times the initial interval count plus one: fixes remove an interval or
//! snap an edge classification to a vertex. Undefined
//! native null dereferences and unequal paired classification lengths return errors.
//! Debug drawing/assertions are omitted. Ignored ordinary Trim failures stay ignored;
//! failed Copy returns an error rather than dereferencing a null native result.
//! `insert_topology_intersections` rejects `object_index > 1`, which upstream accepts.
//! `p5_closed` errors on an unresolved curve, where upstream returns FALSE.
//! The differential runner's face zone tolerance of zero is not compared against C++.

use super::{
    make_consistent_classifications, src_zone_tol_3d_of, CurveClassification, CurveInterval,
};
use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::{ignore_status, ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use crate::geom::tol::{xsect_tol3d, xsect_tol_3d_from_zones, ANG_TOL_DEG, USE_DEFAULT};
use crate::intersect::point_class::{
    object_zone_tol_3d, ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
    PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{Point3, BIG_DOUBLE, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::surface::surface::{Surface, SurfaceKind};

#[derive(Clone, Copy, PartialEq)]
struct NoObject;
impl SolutionObject for NoObject {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

const OP: &str = "intersect::curve_class::p5";

/// Geometry of common classified intervals and isolated boundary points.
#[derive(Debug, Default, Clone)]
pub struct ExtractedGeometry {
    /// Copies of the classification curve trimmed to common intervals.
    pub curves_3d: Vec<Curve>,
    /// Trimmed copies of this classification's first UV curve, when requested.
    pub uv_curves1: Vec<Curve>,
    /// Trimmed copies of the other classification's first UV curve, when requested.
    pub uv_curves2: Vec<Curve>,
    /// Isolated classified points, when requested.
    pub points_3d: Vec<Point3>,
}

/// Requested optional outputs of `ExtractGeometry` (upstream defaults: all false).
#[derive(Debug, Default, Clone, Copy)]
pub struct ExtractOptions {
    /// Request the first UV curves.
    pub uv_curves1: bool,
    /// Request the second UV curves.
    pub uv_curves2: bool,
    /// Request isolated points.
    pub points_3d: bool,
}

/// Topology created by a merge. Collection order is native insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedTopology<O> {
    /// Newly created faces.
    pub faces: Vec<O>,
    /// Split edges (including reused edges).
    pub edges: Vec<O>,
    /// Newly created vertices.
    pub vertices: Vec<O>,
}

impl<O> Default for MergedTopology<O> {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        }
    }
}

/// A failed `MergeIntervals` pass, including lists from completed intervals.
#[derive(Debug)]
pub(crate) struct PartialMergeError<O> {
    /// The first failed interval's status.
    pub(crate) error: SolidError,
    /// Output appended before that failure (native `SER` skips the failing interval).
    pub(crate) merged: Box<MergedTopology<O>>,
}

fn contained_mid(c: PointClassType) -> bool {
    matches!(
        c,
        PointClassType::Edge | PointClassType::Face | PointClassType::Region
    )
}

fn contained_point(c: PointClassType) -> bool {
    contained_mid(c) || c == PointClassType::Vertex
}

fn copy_uv_bspline(curve: &Curve) -> Result<Curve> {
    // Native constructs SmBSplineCurve from the UV curve's B-spline base, slicing
    // analytic subclasses. A non-B-spline UV curve makes that native cast undefined.
    let base = match curve.kind() {
        CurveKind::BSpline(c) => c.clone(),
        CurveKind::Line(c) => c.base_with_nurb()?.into_owned(),
        CurveKind::Ellipse(c) => c.base().clone(),
        CurveKind::Hyperbola(c) => c.base().clone(),
        CurveKind::Parabola(c) => c.base().clone(),
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "UV curve has no B-spline base",
            ))
        }
    };
    Ok(Curve::from(base))
}

fn append_unique<O: PartialEq>(dst: &mut Vec<O>, src: impl IntoIterator<Item = O>) {
    for value in src {
        if !dst.contains(&value) {
            dst.push(value);
        }
    }
}

/// Native `sBigGapXSectTol3d`: a face gap overrides the default constructor only
/// when both large-gap processing and a face are present.
fn vertex_big_gap_tolerance<O: Copy>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    vertex: O,
    surface: &Surface,
    face: Option<O>,
    has_big_gaps: bool,
    tolerance: f64,
) -> Result<f64> {
    if !has_big_gaps {
        return Ok(tolerance);
    }
    let has_face = face.is_some()
        || surface
            .owner()
            .is_some_and(|owner| owner.type_code == crate::geom::assert_array::labels::FACE_TYPE);
    if !has_face {
        return Ok(xsect_tol3d(k, USE_DEFAULT));
    }
    let gap = geometry
        .vertex_face_gap(vertex, surface, face)
        .ok_or_else(|| {
            SolidError::invalid_input(OP, "unresolved classification vertex/face gap")
        })?;
    Ok(if gap > tolerance { gap } else { tolerance })
}

impl<O: ClassObject> CurveClassification<O> {
    fn p5_closed(&self, k: &Kernel, geometry: &impl ClassGeometry<O>) -> Result<bool> {
        let Some(handle) = self.curve() else {
            return Ok(false);
        };
        let curve = geometry
            .curve(handle)
            .ok_or(SolidError::invalid_input(OP, "unresolved curve"))?;
        let zone = src_zone_tol_3d_of(k, Some(self));
        curve.is_closed(
            &curve.natural_interval(),
            xsect_tol_3d_from_zones(k, zone, zone),
        )
    }

    // Remaining dependency boundaries belong to the independent p2/p7 ports.
    fn p5_point_class_by_index(
        classification: &Self,
        index: usize,
    ) -> Result<(&PointClassification<O>, f64)> {
        classification.point_class_by_index(index)
    }

    fn p5_classify_crv_loc_by_index(
        _classification: &Self,
        _geometry: &impl ClassGeometry<O>,
        _index: usize,
    ) -> Result<(Point3, f64)> {
        // UNPORTED(intersect.curve_class_p2): SmCurveClassification::GetClassifyCrvLoc
        Err(SolidError::unported(
            "SmCurveClassification::GetClassifyCrvLoc",
            "intersect.curve_class_p2",
        ))
    }

    fn p5_merge_interval(
        classification: &mut Self,
        index: usize,
        vertices_only: bool,
    ) -> Result<MergedTopology<O>> {
        let merged = classification.merge_interval(index, vertices_only)?;
        Ok(MergedTopology {
            faces: merged.new_faces,
            edges: merged.new_edges,
            vertices: merged.new_vertices,
        })
    }

    /// Extracts common geometry after homogenizing and cleaning both classifications.
    ///
    /// # Errors
    /// Unresolvable handles or unfinished topology merging defer; invalid geometry fails.
    pub fn extract_geometry(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &mut Self,
        options: ExtractOptions,
    ) -> Result<ExtractedGeometry> {
        let mut out = ExtractedGeometry::default();
        self.homogenize(k, geometry, other)?;
        self.cleanup_and_validate(k, geometry, other)?;
        if self.size() != other.size() {
            return Err(SolidError::sm_err(OP));
        }
        let mut found = Vec::new();
        for i in 0..self.size() {
            let this = self.at(i)?;
            let common = contained_mid(this.mid.point_class())
                && contained_mid(other.at(i)?.mid.point_class());
            if common {
                let mut curve = self.curve_geometry(geometry)?.copy(k)?;
                let mut interval = this.interval;
                ignore_status(curve.trim(k, &mut interval, TrimMode::default()))?;
                out.curves_3d.push(curve);
                for (requested, handle, curves) in [
                    (options.uv_curves1, self.uv_curve1(), &mut out.uv_curves1),
                    (options.uv_curves2, other.uv_curve1(), &mut out.uv_curves2),
                ] {
                    if requested {
                        if let Some(handle) = handle {
                            let mut uv =
                                copy_uv_bspline(geometry.curve(handle).ok_or(
                                    SolidError::invalid_input(OP, "unresolved UV curve"),
                                )?)?;
                            ignore_status(uv.trim(k, &mut interval, TrimMode::default()))?;
                            curves.push(uv);
                        }
                    }
                }
            }
            found.push(common);
        }
        let mut found_last = self.p5_closed(k, geometry)? && found.last().copied().unwrap_or(false);
        for j in 0..self.size() {
            let (this, _) = Self::p5_point_class_by_index(self, j)?;
            let (that, _) = Self::p5_point_class_by_index(other, j)?;
            let found_current = found.get(j).copied().ok_or(SolidError::sm_err(OP))?;
            if contained_point(this.point_class())
                && contained_point(that.point_class())
                && !found_current
                && !found_last
            {
                // Upstream computes a tolerance-weighted point, then overwrites it with
                // GetClassifyCrvLoc(j). Retain the fallible lookups before that overwrite.
                let p1 = Self::p5_classify_crv_loc_by_index(self, geometry, j)?.0;
                let p2 = Self::p5_classify_crv_loc_by_index(other, geometry, j)?.0;
                if (p2 - p1).length() > EFF_ZERO {
                    object_zone_tol_3d(k, geometry, this.object())?;
                    object_zone_tol_3d(k, geometry, that.object())?;
                }
                let point = Self::p5_classify_crv_loc_by_index(self, geometry, j)?.0;
                if options.points_3d {
                    out.points_3d.push(point);
                }
            }
            found_last = found_current;
        }
        Ok(out)
    }

    /// Repairs classified interval endpoints, restarting after each interval removal.
    ///
    /// # Errors
    /// Evaluation errors, unresolved topology, or a repair loop that makes no progress.
    pub fn fixup_classification(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
    ) -> Result<()> {
        let mut restarts = self.size().saturating_mul(3).saturating_add(1);
        'restart: loop {
            for i in 0..self.size() {
                if self.at(i)?.mid.point_class() == PointClassType::Unknown {
                    continue;
                }
                let curve = self.curve_geometry(geometry)?;
                let interval = self.at(i)?.interval;
                let start = curve.evaluate_point(interval.min())?;
                let end = curve.evaluate_point(interval.max())?;
                if self.fix_end_point_class(k, geometry, i, start)?
                    || self.fix_end_point_class(k, geometry, i + 1, end)?
                {
                    restarts = restarts.checked_sub(1).ok_or(SolidError::sm_err(OP))?;
                    continue 'restart;
                }
            }
            break;
        }
        make_consistent_classifications(self)
    }

    /// Inserts topology solutions in four passes: nonface ranges, vertices, other
    /// points, then face ranges longer than one twentieth of the classified interval.
    ///
    /// # Errors
    /// Invalid object index, null point-solution objects, insertion errors or deferrals.
    pub fn insert_topology_intersections(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        solutions: &[Solution<O>],
        object_index: usize,
    ) -> Result<()> {
        if object_index > 1 {
            return Err(SolidError::invalid_input(OP, "object index must be 0 or 1"));
        }
        let other_index = usize::from(object_index == 0);
        let zone = src_zone_tol_3d_of(k, Some(self));
        for pass in 0..4 {
            for sol in solutions {
                let object = sol.objects.get(other_index).copied().flatten();
                let face = object.is_some_and(|o| o.class_kind() == ClassObjectKind::Face);
                if (pass == 0 || pass == 3)
                    && sol.solution_type == SolutionType::RangeOfValues
                    && ((pass == 0 && !face)
                        || (pass == 3
                            && face
                            && (sol.end.param(object_index)? - sol.start.param(object_index)?)
                                .abs()
                                > self.interval().length() / 20.0))
                {
                    let object =
                        object.ok_or(SolidError::invalid_input(OP, "null solution object"))?;
                    let (start, end) =
                        if sol.start.param(object_index)? <= sol.end.param(object_index)? {
                            (&sol.start, &sol.end)
                        } else {
                            (&sol.end, &sol.start)
                        };
                    let mut ivl = CurveInterval::with_interval(
                        k,
                        &Extent1d::from_min_max(
                            start.param(object_index)?,
                            end.param(object_index)?,
                        ),
                        Some(self),
                    );
                    ivl.start.load_from_intersection(
                        zone,
                        object,
                        start,
                        other_index,
                        start.param(object_index)?,
                    )?;
                    ivl.mid
                        .load_from_intersection(zone, object, start, other_index, BIG_DOUBLE)?;
                    ivl.end.load_from_intersection(
                        zone,
                        object,
                        end,
                        other_index,
                        end.param(object_index)?,
                    )?;
                    self.insert_curve_interval(k, geometry, &ivl)?;
                } else if (pass == 1 || pass == 2) && sol.solution_type == SolutionType::SingleValue
                {
                    let object = object
                        .ok_or(SolidError::invalid_input(OP, "null point-solution object"))?;
                    let vertex = object.class_kind() == ClassObjectKind::Vertex;
                    if (pass == 1 && vertex) || (pass == 2 && !vertex) {
                        let mut pc = PointClassification::new(zone);
                        pc.load_from_intersection(
                            zone,
                            object,
                            &sol.start,
                            other_index,
                            sol.start.param(object_index)?,
                        )?;
                        self.insert_point_class(
                            k,
                            geometry,
                            &pc,
                            sol.start.param(object_index)?,
                            BIG_DOUBLE,
                            false,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Intersects the curve with vertices (defaults: no large gaps, UV space allowed).
    ///
    /// # Errors
    /// Missing vertex points or vertex/face gaps are invalid input. Generic 2D
    /// curve-on-surface dispatch and the unported SSS block defer.
    pub fn intersect_vertices(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        vertices: &[O],
        surface: &Surface,
        has_big_gaps: bool,
        uv_space_okay: bool,
    ) -> Result<()> {
        let original_curve = self.curve_geometry(geometry)?;
        let wrapped;
        let curve = if !uv_space_okay && original_curve.dim() == 2 {
            let SurfaceKind::BSpline(bs) = surface.kind() else {
                // UNPORTED(curve.crv_on_surf): SmCrvOnSurf over a generic Surface
                return Err(SolidError::unported(
                    "SmCrvOnSurf generic Surface dispatch",
                    "curve.crv_on_surf",
                ));
            };
            wrapped = Curve::from(CrvOnSurf::new(
                k,
                original_curve.clone(),
                bs.clone(),
                Some(surface.natural_uv_domain()),
                0,
            )?);
            &wrapped
        } else {
            original_curve
        };
        let source_zone = src_zone_tol_3d_of(k, Some(self));
        for &vertex in vertices {
            let vertex_zone = object_zone_tol_3d(k, geometry, Some(vertex))?;
            let tolerance = xsect_tol_3d_from_zones(k, source_zone, vertex_zone);
            let point = geometry
                .classification_vertex_point(vertex)
                .ok_or_else(|| {
                    SolidError::invalid_input(OP, "unresolved classification vertex point")
                })?;
            let big_tolerance = vertex_big_gap_tolerance(
                k,
                geometry,
                vertex,
                surface,
                self.face(),
                has_big_gaps,
                tolerance,
            )?;
            let solutions = curve.global_point_solve::<NoObject>(
                k,
                &self.interval(),
                SolverOperation::Minimize,
                point,
                tolerance,
                SolutionRequested::All,
                &GlobalPointSolveOptions {
                    target_distance: Some(0.0),
                    ..GlobalPointSolveOptions::default()
                },
            )?;
            if solutions.is_empty() {
                continue;
            }
            if self.surface1().is_some()
                && self.surface2().is_some()
                && (solutions.len() > 1
                    || (solutions.len() == 1
                        && solutions.as_slice().first().is_some_and(|s| {
                            s.start.solution_value > 1e-7 && s.start.solution_value < tolerance
                        })))
            {
                // UNPORTED(intersect.curve_class_p5): SmCurveClassification::IntersectVertices
                // This unit's own unported GWC:CHANGED_SSS block of IntersectVertices
                // (SmCurveClass.cpp:14826-15095): knot snapping (SnapToKnots), local and
                // global point solves, sm_SetXSectCurveThroughPoint, SmVertex::GetEdges /
                // SmEdge::GetEdgeuses walks, SetUVTrimCurve, then the mate lookups
                // (SmTopologyIntersector::GetOtherMate / GetBrepMate, ported in
                // boolean.topology_intersector_p1). Porting it needs mutable curve and B-rep
                // access here plus a mate-lookup closure. Deferred before any mutation. This
                // block does not call sm_RefineCCIToSurfaces.
                return Err(SolidError::unported(
                    "SmCurveClassification::IntersectVertices (SSS block, SmCurveClass.cpp:14826-15095)",
                    "intersect.curve_class_p5",
                ));
            }
            for solution in solutions.as_slice() {
                if solution.start.solution_value > tolerance {
                    if !has_big_gaps || solution.start.solution_value > big_tolerance {
                        continue;
                    }
                    let domain = surface.natural_uv_domain();
                    let vertex_solutions = surface.global_point_solve::<NoObject>(
                        k,
                        None,
                        &domain,
                        SolverOperation::Minimize,
                        point,
                        vertex_zone,
                        None,
                        SolutionRequested::Single,
                    )?;
                    let Some(projected) = vertex_solutions.as_slice().first() else {
                        continue;
                    };
                    let vertex_on_surface = surface.evaluate_point(crate::math::Vec2::new(
                        projected.start.param(0)?,
                        projected.start.param(1)?,
                    ))?;
                    let curve_point = curve.evaluate_point(solution.start.param(0)?)?;
                    let curve_solutions = surface.global_point_solve::<NoObject>(
                        k,
                        None,
                        &domain,
                        SolverOperation::Minimize,
                        curve_point,
                        source_zone,
                        None,
                        SolutionRequested::Single,
                    )?;
                    let Some(projected) = curve_solutions.as_slice().first() else {
                        continue;
                    };
                    let curve_on_surface = surface.evaluate_point(crate::math::Vec2::new(
                        projected.start.param(0)?,
                        projected.start.param(1)?,
                    ))?;
                    if curve_on_surface.distance(vertex_on_surface) > tolerance {
                        continue;
                    }
                }
                let mut pc = PointClassification::new(source_zone);
                pc.set_class_object(PointClassType::Vertex, Some(vertex));
                pc.set_gap_3d(solution.start.solution_value);
                pc.set_src_zone_tol_3d(source_zone);
                pc.set_pre_snap_param(solution.start.param(0)?);
                self.insert_point_class(
                    k,
                    geometry,
                    &pc,
                    solution.start.param(0)?,
                    BIG_DOUBLE,
                    false,
                )?;
            }
        }
        Ok(())
    }

    /// Merges common classified intervals, then optional singularities, into both B-reps.
    ///
    /// # Errors
    /// Unequal interval counts fail before mutation; missing merging operations defer.
    pub fn merge_classifications(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &mut Self,
        merge_singularities: bool,
    ) -> Result<(MergedTopology<O>, MergedTopology<O>)> {
        if self.size() != other.size() {
            return Err(SolidError::sm_err(OP));
        }
        let mut out = MergedTopology::default();
        let mut out_other = MergedTopology::default();
        let mut scratch = MergedTopology::default();
        let mut scratch_other = MergedTopology::default();
        let closed = self.p5_closed(k, geometry)?;
        for i in 0..self.size() {
            if !contained_mid(self.at(i)?.mid.point_class())
                || !contained_mid(other.at(i)?.mid.point_class())
            {
                continue;
            }
            scratch = Self::p5_merge_interval(self, i, false)?;
            append_unique(&mut out.faces, scratch.faces.iter().copied());
            append_unique(&mut out.edges, scratch.edges.iter().copied());
            append_unique(&mut out.vertices, scratch.vertices.iter().copied());
            scratch_other = Self::p5_merge_interval(other, i, false)?;
            append_unique(&mut out_other.faces, scratch_other.faces.iter().copied());
            append_unique(&mut out_other.edges, scratch_other.edges.iter().copied());
            append_unique(
                &mut out_other.vertices,
                scratch_other.vertices.iter().copied(),
            );
            if i == 0 && closed {
                let last = self.size() - 1;
                self.at_mut(last)?.end = self.at(0)?.start;
                other.at_mut(last)?.end = other.at(0)?.start;
            }
        }
        if merge_singularities && self.size() > 0 {
            for j in 0..=self.size() {
                if contained_point(Self::p5_point_class_by_index(self, j)?.0.point_class())
                    && contained_point(Self::p5_point_class_by_index(other, j)?.0.point_class())
                {
                    self.merge_point_class(
                        k,
                        geometry,
                        j,
                        Some(&mut scratch.edges),
                        Some(&mut scratch.vertices),
                    )?;
                    // MergePointClass appends to the reused scratch lists. Native
                    // Append copies the whole lists, retaining repeated prefixes.
                    out.edges.extend_from_slice(&scratch.edges);
                    out.vertices.extend_from_slice(&scratch.vertices);
                    other.merge_point_class(
                        k,
                        geometry,
                        j,
                        Some(&mut scratch_other.edges),
                        Some(&mut scratch_other.vertices),
                    )?;
                    out_other.edges.extend_from_slice(&scratch_other.edges);
                    out_other
                        .vertices
                        .extend_from_slice(&scratch_other.vertices);
                }
            }
        }
        Ok((out, out_other))
    }

    /// Imprints interval vertices first and edges second; `vertices_only` skips pass two.
    /// The upstream default is false. The debug-only original face argument is omitted.
    ///
    /// # Errors
    /// Unfinished topology merging defers when an interval requires graph mutation.
    pub fn merge_intervals(&mut self, vertices_only: bool) -> Result<MergedTopology<O>> {
        self.merge_intervals_with_partial(vertices_only)
            .map_err(|e| e.error)
    }

    /// The merge status and completed-interval output, including on failure.
    /// Callers that ignore the native status must still consume its partial lists.
    pub(crate) fn merge_intervals_with_partial(
        &mut self,
        vertices_only: bool,
    ) -> Result<MergedTopology<O>, PartialMergeError<O>> {
        self.merge_intervals_using(vertices_only, Self::p5_merge_interval)
    }

    fn merge_intervals_using(
        &mut self,
        vertices_only: bool,
        mut merge: impl FnMut(&mut Self, usize, bool) -> Result<MergedTopology<O>>,
    ) -> Result<MergedTopology<O>, PartialMergeError<O>> {
        let mut out = MergedTopology::default();
        let status = (|| {
            for pass in 0..if vertices_only { 1 } else { 2 } {
                for j in 0..self.size() {
                    let ivl = self.at(j)?;
                    if ivl.start.object().is_none() && ivl.end.object().is_none() {
                        continue;
                    }
                    let merged = merge(self, j, pass == 0)?;
                    out.faces.extend(merged.faces);
                    out.edges.extend(merged.edges);
                    out.vertices.extend(merged.vertices);
                }
            }
            Ok(())
        })();
        match status {
            Ok(()) => Ok(out),
            Err(error) => Err(PartialMergeError {
                error,
                merged: Box::new(out),
            }),
        }
    }
}

/// Converts sub-tolerance ranges to points and inserts curve/edge/plane intersections.
///
/// `xsect_tolerance` defaults to the sum of source and target zones. Solutions are
/// modified in place when a range collapses; their end records are preserved.
///
/// # Errors
/// Invalid target kinds fail. Opaque edge topology and unavailable insertion callees defer.
/// Native short-range tangency filtering without an edge dereferences a null pointer;
/// Rust returns an invalid-input error for that branch.
pub fn insert_solutions<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    classification: &mut CurveClassification<O>,
    solutions: &mut [Solution<O>],
    object: O,
    xsect_tolerance: Option<f64>,
) -> Result<()> {
    let source = classification.curve_geometry(geometry)?;
    let source_zone = src_zone_tol_3d_of(k, Some(classification));
    let target_zone = object_zone_tol_3d(k, geometry, Some(object))?;
    let tol = xsect_tolerance.map_or_else(
        || xsect_tol_3d_from_zones(k, source_zone, target_zone),
        |t| xsect_tol3d(k, t),
    );
    let target = geometry.curve(object);
    let plane = geometry
        .surface(object)
        .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
    if object.class_kind() == ClassObjectKind::Edge {
        // UNPORTED(intersect.curve_class_p2): sm_InsertSolutions edge branch (InsertAt)
        // The unported branch also needs p7 point-index queries, vertex snapping,
        // sm_RefineCCIToSurfaces and the 2D class object; resolving an edge curve
        // alone does not implement it.
        return Err(SolidError::unported(
            "sm_InsertSolutions edge branch (InsertAt / GetPointClassByIndex)",
            "intersect.curve_class_p2",
        ));
    }
    if target.is_none() && !plane {
        return Err(SolidError::sm_err(OP));
    }
    for sol in solutions {
        collapse_short_range(geometry, sol, tol)?;
        if sol.solution_type == SolutionType::SingleValue {
            let mut pc = PointClassification::new(source_zone);
            pc.set_class_object(
                if target.is_some() {
                    PointClassType::Curve
                } else {
                    PointClassType::Surface
                },
                Some(object),
            );
            pc.set_gap_3d(sol.start.solution_value);
            pc.set_src_zone_tol_3d(source_zone);
            pc.set_pre_snap_param(sol.start.param(0)?);
            pc.set_t_param(sol.start.param(1)?, classification.doing_cross_hatch());
            let mut do_insertion = true;
            if classification.ignore_tangency() {
                if let Some(target) = target {
                    let a = source.evaluate(sol.start.param(0)?, 1, true, true)?;
                    let b = target.evaluate(sol.start.param(1)?, 1, true, true)?;
                    let ta = a.get(1).copied().ok_or(SolidError::sm_err(OP))?;
                    let tb = b.get(1).copied().ok_or(SolidError::sm_err(OP))?;
                    let mut angle = tb.angle_between(ta).unwrap_or(1.0);
                    if angle > PI / 2.0 {
                        angle = PI - angle;
                    }
                    if angle < 5.0 * PI * ANG_TOL_DEG / 180.0 {
                        do_insertion = false;
                    }
                    if classification
                        .interval()
                        .is_value_on_boundary(sol.start.param(0)?, EFF_ZERO)
                        .0
                    {
                        do_insertion = true;
                    }
                }
            }
            if do_insertion {
                classification.insert_point_class(
                    k,
                    geometry,
                    &pc,
                    sol.start.param(0)?,
                    BIG_DOUBLE,
                    false,
                )?;
            }
        } else if sol.solution_type == SolutionType::RangeOfValues {
            if sol.start.param(0)? > sol.end.param(0)? {
                continue;
            }
            let mut ivl = CurveInterval::with_interval(
                k,
                &Extent1d::from_min_max(sol.start.param(0)?, sol.end.param(0)?),
                Some(classification),
            );
            for pc in [&mut ivl.start, &mut ivl.mid, &mut ivl.end] {
                pc.set_class_object(
                    if target.is_some() {
                        PointClassType::Curve
                    } else {
                        PointClassType::Surface
                    },
                    Some(object),
                );
            }
            for (pc, end) in [(&mut ivl.start, &sol.start), (&mut ivl.end, &sol.end)] {
                pc.set_gap_3d(end.solution_value);
                pc.set_pre_snap_param(end.param(0)?);
                pc.set_t_param(end.param(1)?, classification.doing_cross_hatch());
            }
            if classification.ignore_tangency()
                && ivl.interval.length() / classification.interval().length() < 0.05
            {
                return Err(SolidError::invalid_input(
                    OP,
                    "native short-range tangency filtering requires an edge",
                ));
            }
            classification.insert_curve_interval(k, geometry, &ivl)?;
        }
    }
    Ok(())
}

fn collapse_short_range<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    sol: &mut Solution<O>,
    tolerance: f64,
) -> Result<()> {
    if sol.solution_type != SolutionType::RangeOfValues {
        return Ok(());
    }
    // GetPoint(0) advances no parameters. A curve uses the first parameter and
    // reports ordinary evaluation failure through status; that leaves the range intact.
    let Some(handle) = sol.objects.first().copied().flatten() else {
        return Ok(());
    };
    let Some(curve) = geometry.curve(handle) else {
        return Ok(());
    };
    let (Some(start), Some(end)) = (
        ignored(curve.evaluate_point(sol.start.param(0)?))?,
        ignored(curve.evaluate_point(sol.end.param(0)?))?,
    ) else {
        return Ok(());
    };
    if !start.closer_than(tolerance, end) {
        return Ok(());
    }
    let mut interval = Extent1d::from_value(sol.start.param(0)?);
    interval.add_value(sol.end.param(0)?);
    if curve.approximate_length(&interval, 6, false)?.0 < tolerance {
        sol.solution_type = SolutionType::SingleValue;
        if sol.end.solution_value < sol.start.solution_value * 0.8 {
            sol.start.solution_value = sol.end.solution_value;
            for j in 0..2 {
                *sol.start.param_mut(j)? = sol.end.param(j)?;
            }
        } else if sol.end.solution_value < sol.start.solution_value * 1.2 {
            sol.start.solution_value = 0.5 * (sol.start.solution_value + sol.end.solution_value);
            for j in 0..2 {
                *sol.start.param_mut(j)? = 0.5 * (sol.start.param(j)? + sol.end.param(j)?);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::math::Vec3;
    use crate::surface::volume::Volume;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct H;
    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            ClassObjectKind::Curve
        }
    }
    struct Geometry(Curve);
    impl ClassGeometry<H> for Geometry {
        fn curve(&self, _: H) -> Option<&Curve> {
            Some(&self.0)
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
    fn classification(k: &Kernel, count: usize) -> CurveClassification<H> {
        let mut cc = CurveClassification::new(k);
        cc.set_curve(Some(H), false);
        cc.set_src_zone_tol_3d(1e-6);
        cc.set_size(k, count).unwrap();
        for j in 0..count {
            cc.at_mut(j).unwrap().set_interval(&Extent1d::from_min_max(
                j as f64 / count as f64,
                (j + 1) as f64 / count as f64,
            ));
        }
        cc
    }
    fn geometry() -> Geometry {
        Geometry(Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            &Extent1d::from_min_max(0., 1.),
            &BoundedLineOptions::default(),
        )))
    }

    #[derive(Clone, Copy, PartialEq)]
    enum VertexTestObject {
        Curve,
        Vertex,
        Face,
    }

    impl ClassObject for VertexTestObject {
        fn class_kind(&self) -> ClassObjectKind {
            match self {
                Self::Curve => ClassObjectKind::Curve,
                Self::Vertex => ClassObjectKind::Vertex,
                Self::Face => ClassObjectKind::Face,
            }
        }
    }

    struct VertexGeometry {
        curve: Curve,
        point: Option<Point3>,
        gap_calls: core::cell::Cell<usize>,
    }

    impl ClassGeometry<VertexTestObject> for VertexGeometry {
        fn curve(&self, object: VertexTestObject) -> Option<&Curve> {
            (object == VertexTestObject::Curve).then_some(&self.curve)
        }
        fn surface(&self, _: VertexTestObject) -> Option<&Surface> {
            None
        }
        fn volume(&self, _: VertexTestObject) -> Option<&Volume> {
            None
        }
        fn point(&self, _: VertexTestObject) -> Option<Point3> {
            None
        }
        fn classification_vertex_point(&self, _: VertexTestObject) -> Option<Point3> {
            self.point
        }
        fn topology_zone_tol_3d(&self, _: VertexTestObject) -> Option<f64> {
            Some(0.0)
        }
        fn vertex_face_gap(
            &self,
            _: VertexTestObject,
            _: &Surface,
            _: Option<VertexTestObject>,
        ) -> Option<f64> {
            self.gap_calls.set(self.gap_calls.get() + 1);
            None
        }
    }

    fn vertex_classification(k: &Kernel) -> CurveClassification<VertexTestObject> {
        let mut cc = CurveClassification::new(k);
        cc.set_curve(Some(VertexTestObject::Curve), false);
        cc.set_interval(&Extent1d::from_min_max(0., 1.)).unwrap();
        cc
    }

    #[test]
    fn unresolved_vertex_point_is_invalid_input() {
        let k = Kernel::default();
        let g = VertexGeometry {
            curve: geometry().0,
            point: None,
            gap_calls: core::cell::Cell::new(0),
        };
        let surface = Surface::from(crate::surface::plane::Plane::from_normal(
            Vec3::ZERO,
            Vec3::new(0., 0., 1.),
        ));
        let result = vertex_classification(&k).intersect_vertices(
            &k,
            &g,
            &[VertexTestObject::Vertex],
            &surface,
            false,
            true,
        );
        assert!(matches!(result, Err(SolidError::InvalidInput { .. })));
        assert_eq!(g.gap_calls.get(), 0);
    }

    #[test]
    fn large_gap_adapter_is_only_required_when_a_face_exists() {
        use crate::geom::assert_array::labels::FACE_TYPE;
        use crate::geom::attribute::ObjectKey;
        use crate::geom::object::{ObjectRef, OBJECT_TYPE};

        let k = Kernel::default();
        for face_mode in 0..4 {
            let g = VertexGeometry {
                curve: geometry().0,
                // The vertex is far from the curve, so the ordinary limited solve
                // produces no intersection after the adapter preconditions pass.
                point: Some(Vec3::new(0., 10., 0.)),
                gap_calls: core::cell::Cell::new(0),
            };
            let mut cc = vertex_classification(&k);
            let mut surface = Surface::from(crate::surface::plane::Plane::from_normal(
                Vec3::ZERO,
                Vec3::new(0., 0., 1.),
            ));
            if face_mode == 1 {
                cc.set_face(Some(VertexTestObject::Face));
            } else if face_mode >= 2 {
                surface.set_owner(Some(ObjectRef {
                    type_code: if face_mode == 2 {
                        FACE_TYPE
                    } else {
                        OBJECT_TYPE
                    },
                    key: ObjectKey(1),
                }));
            }
            let result =
                cc.intersect_vertices(&k, &g, &[VertexTestObject::Vertex], &surface, true, true);
            if face_mode == 1 || face_mode == 2 {
                assert!(matches!(result, Err(SolidError::InvalidInput { .. })));
                assert_eq!(g.gap_calls.get(), 1);
            } else {
                assert!(result.is_ok());
                assert_eq!(g.gap_calls.get(), 0);
            }
        }
    }

    #[test]
    fn no_face_large_gap_keeps_default_cutoff() {
        let mut k = Kernel::default();
        let g = VertexGeometry {
            curve: geometry().0,
            point: None,
            gap_calls: core::cell::Cell::new(0),
        };
        let surface = Surface::from(crate::surface::plane::Plane::from_normal(
            Vec3::ZERO,
            Vec3::new(0., 0., 1.),
        ));
        let ordinary = 1e-5;
        let cutoff = vertex_big_gap_tolerance(
            &k,
            &g,
            VertexTestObject::Vertex,
            &surface,
            None,
            true,
            ordinary,
        )
        .unwrap();
        assert_eq!(cutoff, 2e-5);
        assert!(1.5e-5 > ordinary && 1.5e-5 < cutoff);
        assert!(2.5e-5 > cutoff);
        assert_eq!(
            vertex_big_gap_tolerance(
                &k,
                &g,
                VertexTestObject::Vertex,
                &surface,
                None,
                false,
                ordinary,
            )
            .unwrap(),
            ordinary
        );
        k.tolerances.zone_tol_3d = 4e-5;
        assert_eq!(
            vertex_big_gap_tolerance(
                &k,
                &g,
                VertexTestObject::Vertex,
                &surface,
                None,
                true,
                ordinary,
            )
            .unwrap(),
            8e-5
        );
        assert_eq!(g.gap_calls.get(), 0);
    }

    #[test]
    fn paired_classification_lengths_fail_before_mutation() {
        let k = Kernel::default();
        let g = geometry();
        for other_count in [1, 3] {
            let mut cc = classification(&k, 2);
            let mut other = classification(&k, other_count);
            let before = cc.intervals().to_vec();
            let other_before = other.intervals().to_vec();
            assert!(matches!(
                cc.merge_classifications(&k, &g, &mut other, true),
                Err(SolidError::Upstream { code: 1001, .. })
            ));
            assert_eq!(cc.intervals(), before);
            assert_eq!(other.intervals(), other_before);
        }
        let mut cc = classification(&k, 2);
        let mut other = classification(&k, 2);
        let (a, b) = cc.merge_classifications(&k, &g, &mut other, false).unwrap();
        assert_eq!(a, MergedTopology::default());
        assert_eq!(b, MergedTopology::default());
    }

    #[test]
    fn collapsed_range_keeps_end_and_unrelated_parameters() {
        let g = geometry();
        for (ratio, expected) in [(0.79, 0.6e-6), (0.8, 0.55e-6), (1.2, 0.5e-6)] {
            let mut sol = Solution::new();
            sol.solution_type = SolutionType::RangeOfValues;
            sol.objects[0] = Some(H);
            sol.num_objects = 1;
            sol.num_variables = 2;
            *sol.start.param_mut(0).unwrap() = 0.5e-6;
            *sol.end.param_mut(0).unwrap() = 0.6e-6;
            *sol.start.param_mut(1).unwrap() = 0.5e-6;
            *sol.end.param_mut(1).unwrap() = 0.6e-6;
            *sol.start.param_mut(2).unwrap() = 42.;
            sol.start.solution_value = 1.;
            sol.end.solution_value = ratio;
            let end = sol.end;
            collapse_short_range(&g, &mut sol, 1e-6).unwrap();
            assert_eq!(sol.solution_type, SolutionType::SingleValue);
            assert_eq!(sol.start.param(0).unwrap(), expected);
            assert_eq!(sol.start.param(2).unwrap(), 42.);
            assert_eq!(sol.end, end);
        }
    }
    #[test]
    fn failed_merge_retains_completed_intervals_in_native_visit_order() {
        for vertices_only in [true, false] {
            let k = Kernel::default();
            let mut cc = classification(&k, 3);
            for j in 0..3 {
                cc.at_mut(j)
                    .unwrap()
                    .start
                    .set_class_object(PointClassType::Curve, Some(H));
            }
            let mut visited = Vec::new();
            let failure = cc
                .merge_intervals_using(vertices_only, |_, index, vertex_pass| {
                    visited.push((index, vertex_pass));
                    if index == 1 && (vertices_only || !vertex_pass) {
                        return Err(SolidError::sm_err(OP));
                    }
                    Ok(MergedTopology {
                        faces: vec![H],
                        edges: vec![H, H],
                        vertices: vec![H, H, H],
                    })
                })
                .unwrap_err();
            let completed = if vertices_only { 1 } else { 4 };
            assert!(matches!(
                failure.error,
                SolidError::Upstream { code: 1001, .. }
            ));
            assert_eq!(failure.merged.faces.len(), completed);
            assert_eq!(failure.merged.edges.len(), 2 * completed);
            assert_eq!(failure.merged.vertices.len(), 3 * completed);
            let mut expected = vec![(0, true), (1, true)];
            if !vertices_only {
                expected = vec![(0, true), (1, true), (2, true), (0, false), (1, false)];
            }
            assert_eq!(visited, expected);
        }
    }
}
