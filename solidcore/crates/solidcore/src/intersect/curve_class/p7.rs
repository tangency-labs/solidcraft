//! Curve classification, part 7: queries over the interval end classifications (point
//! classifications by boundary index and their mates, touches, vertices and edges met,
//! edge crossings, face crossings, near misses), interval collapse, the
//! coincident-maybe and partial-coincidence heuristics, face classification of an
//! interval and the fillet geometry hand-over.
//!
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust                                                        | Upstream                                              |
//! |-------------------------------------------------------------|-------------------------------------------------------|
//! | [`CurveClassification::point_class_by_index`], [`CurveClassification::point_class_by_index_mut`] | `GetPointClassByIndex` |
//! | [`CurveClassification::point_class_mate_by_index`]          | `GetPointClassMateByIndex`                            |
//! | [`CurveClassification::collapse_interval`]                  | `CollapseInterval`                                    |
//! | [`CurveClassification::point_classifies_connected_to`]      | `GetPointClassifiesConnectedTo`                       |
//! | [`CurveClassification::find_vertex_objects`], [`VertexObjects`] | `FindVertexObjects`                               |
//! | [`CurveClassification::find_edge_objects`]                  | `FindEdgeObjects`                                     |
//! | [`CurveClassification::is_crossed_by_edge`]                 | `IsCrossedByEdge`                                     |
//! | [`CurveClassification::is_crossing_face`]                   | `IsCrossingFace`                                      |
//! | [`CurveClassification::has_near_misses`]                    | `HasNearMisses`                                       |
//! | [`CurveClassification::are_coincident_maybe`], [`CoincidentMaybe`] | `AreCoincidentMaybe`                          |
//! | [`CurveClassification::classify_face_interval`], [`CurveClassification::classify_face_interval_mut`] | `ClassifyFaceInterval` |
//! | [`CurveClassification::test_for_partial_coincident_edges`], [`PartialCoincidence`] | `TestForPartialCoincidentEdges` |
//! | [`set_do_filleting_geometry`], [`set_do_filleting_geometry_mut`] | `sm_SetDoFilletingGeometry`                     |
//! | [`CurveClassification::curve`] of the owner                 | `SmCurveInterval::GetCurve`                           |
//! | [`curve_is_degenerate`] (`intersect.curve_class_p3`)        | `SmTol::IsDegenerate(const SmCurve*, SmExtent1d, SmXSectTol3d*)` (CO-PORTED from `geom.tol`, with the tolerance given) |
//! | `tol_is_closed_curve` (private)                             | `SmTol::IsClosed(const SmObject*, SmXSectTol3d*)` for a curve (CO-PORTED from `geom.tol`, with the tolerance given) |
//!
//! Deviations:
//! - **Handles.** As in part 1, every object is a caller handle `O` resolved through a
//!   [`ClassGeometry`]. `SmCurveInterval::GetCurve` reads the owner's curve through its
//!   back pointer, which is not ported: use [`CurveClassification::curve`] of the
//!   classification holding the interval.
//! - **Out-params** become return values: [`VertexObjects`], [`PartialCoincidence`],
//!   `(CoincidentMaybe, IntervalPosition)`, `(point classification, parameter)`. The
//!   touches of [`CurveClassification::point_classifies_connected_to`] carry
//!   `pt_classify = None` (a point classification inside a classification has no
//!   handle): `pt_classify_index` 0 is the first interval's start, `i > 0` the end of
//!   interval `i - 1`.
//! - **The other classification** of `sm_SetDoFilletingGeometry` is reached through
//!   `GetOtherClassification` upstream; here the caller passes the classification the
//!   handle names. `sm_SetDoFilletingGeometry` reads the target classification only
//!   (upstream edits the topology it classifies to).
//! - **`SmCrvOnSurf` in `AreCoincidentMaybe`** is built only where it is evaluated (the
//!   edge-coincidence branch), not at the top of each pass; it has no other effect.
//! - **`ClassifyFaceInterval`**: the `SmClassifyProblem` causes are not returned by the
//!   face queries (only the commented-out step 3 read them); `lFaceIndx` is passed on
//!   as upstream (debugging only).
//! - **`TestForPartialCoincidentEdges`** re-runs the coincidence check into the array
//!   it is iterating, and keeps reading its solution reference `rSol`: here `rSol` is
//!   the re-run's solution at the same index, or the old one when the re-run found
//!   fewer (upstream's fixed 16-entry buffer keeps the old entry).
//! - **Undefined behaviour upstream** is an error here (`InvalidInput`): interval or
//!   boundary indices past the intervals, the first interval of an empty
//!   classification, a missing curve where it is dereferenced, and a coincident edge
//!   whose curve is not of a kind derived from `SmBSplineCurve` (`SM_CAST_PTR` gives
//!   `NULL`; the copy slices a line or conic to its B-spline part).
//! - **Debug-only work** (`SM_DEBUG_CODE`, `SM_DBG_WARN`, `bReportProb` messages, the
//!   unused `AreCoincidentMaybe` angle check) is dropped; the angle is still computed.
//! - **Topology members.** `SmEdge::GetCurve` / `GetInterval` / `IsConnectedTo`,
//!   `SmVertex::GetPoint` / `IsConnectedTo` and the edge tolerances of
//!   `SmTol::GetXSectTol3d(obj, obj)` are the ported ones (units `topology.edge`,
//!   `topology.vertex`), reached through [`ClassGeometry::topology_entity`]; a handle whose
//!   adapter lacks that hook keeps the earlier [`SolidError::Unported`] deferral, and a
//!   connection target in another B-rep is connected to nothing. `IsConnectedTo(NULL)` is
//!   `TRUE` before any member is read, as upstream. The edits of `sm_SetDoFilletingGeometry`
//!   (`SmVertex::SetPoint`, `SmEdge::SetCurve`) on the mutable path ([`set_do_filleting_geometry_mut`])
//!   update the B-rep entities directly; the read-only path ([`set_do_filleting_geometry`]) returns
//!   [`SolidError::Unported`].
//! - **Face classifiers.** `SmFace::EdgeCurveOnClassify` / `Point3DClassify`
//!   ([`crate::query::face::p1`]) run on the B-rep and face that the adapter resolves
//!   the face handle to. Upstream's members are `const` but may add UV trim curves and
//!   trimmed surface caches to the B-rep.
//!   [`CurveClassification::classify_face_interval_mut`] (a [`ClassGeometryMut`]) runs
//!   them on that B-rep itself, so those additions stay, in upstream's order.
//!   **Deviation (#614, kept):** the read-only
//!   [`CurveClassification::classify_face_interval`] runs them on a copy of the B-rep,
//!   so their additions are discarded. A handle the adapter cannot resolve keeps the
//!   [`SolidError::Unported`] dispatch-gap deferral. An edge of another B-rep has no
//!   use on the face: not inside, as upstream's use walk finds. **Cache history:** a
//!   face query on the mutable path may build the face surface's trimmed surface cache
//!   with the face (`smsurf_GetSurfaceCache`), whose UV trim curves raise vertex and
//!   edge tolerances, or reuse the one the surface already holds (made earlier, with or
//!   without the face). The surface's cache slot keeps that history
//!   ([`crate::query::face::p2::face_surface_cache`]), so the query does what upstream
//!   does and the raised tolerances stay on the B-rep (`plan/findings.md`, "trimmed
//!   surface cache history: tolerance-growth deferral removed").
//! - **`SmFace::FindVertexSector`** runs on the mutable path only; the read-only
//!   path returns [`SolidError::Unported`] (`topology.face_p4`).
//! - **`SmFace::PointClassify`** ([`crate::query::face::p2::point_classify`]) runs on
//!   the resolved B-rep on the mutable path, so the UV trim curves its loop walk makes
//!   stay there. The read-only path runs the same classification over the unchanged
//!   topology ([`crate::query::face::p2::point_classify_prepared`], no copy): a loop
//!   walk that needs a UV trim curve made returns [`SolidError::Unported`].
//! - **Callees in units not ported yet** return [`SolidError::Unported`]: the topology
//!   zone tolerances of point classifications (through `intersect.point_class`).
//! - **`HasNearMisses`** measures a vertex end's gap across the curve within surface 1
//!   when the classification has a surface 1 and a UV curve 1: upstream counts the
//!   whole gap (or, with `bInSurfaceGaps`, its part in the tangent plane), so two seams
//!   ending 1e-8 apart along the seam read as a near miss. A coincident edge's gap is
//!   counted as upstream. A vertex that ends a coincident edge lying on the curve (the
//!   seam's own edge) is that edge's end, so its gap across the curve is bounded by the
//!   vertex's tolerance, not by the fixed `1000 * SM_EFF_ZERO`: a near miss only past the
//!   tolerance. This applies at a shared interval boundary from either side of
//!   its coincident edge. Whole-definition control-polygon strip bounds or native
//!   derivative enclosures establish actual 3D across-seam coincidence. UV identity
//!   only locates the comparison; samples cannot grant relief.
//!   A cut wall whose seam edge ends on a body edge 1e-7 to 1e-5 across the
//!   seam (the cut's sketch circle that far off the edge) read as a near miss; its loop
//!   closes at the vertex within tolerance, and no sliver lies beside the seam.
//! - **Recursion.** None.

use std::borrow::Cow;

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, nurbs_partials_over_box, NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{span, Nurbs};
use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::curve::native_affine::{exact_four_products_zero, of_line};
use crate::error::{ignored, EntityRef, Result, SolidError};
use crate::geom::expansion;
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use crate::geom::tol::{eff_zero_ang_rad, scaled_zero, xsect_tol_3d_from_zones, ScaledZero};
use crate::intersect::curve_class::p3::curve_is_degenerate;
use crate::intersect::curve_class::p5::ClassGeometryMut;
use crate::intersect::point_class::{
    object_zone_tol_3d, ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
    PointClassification, Precedence,
};
use crate::kernel::Kernel;
use crate::math::{max2, min2, Point2, Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::query::face::p1 as qf1;
use crate::query::face::p2 as qf2;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, VertexId};

use super::p1::{
    get_curve_curve_angle, src_zone_tol_3d_of, xsect_tol_2d_or_3d_for_classify_pair,
    CurveClassification, CurveInterval, IntervalPosition, TouchData,
};

const OP_INDEX: &str = "intersect::curve_class::p7::point_class_by_index";
const OP_COLLAPSE: &str = "intersect::curve_class::p7::collapse_interval";
const OP_COIN: &str = "intersect::curve_class::p7::are_coincident_maybe";
const OP_FACE: &str = "intersect::curve_class::p7::classify_face_interval";
const OP_PARTIAL: &str = "intersect::curve_class::p7::test_for_partial_coincident_edges";
const OP_FILLET: &str = "intersect::curve_class::p7::set_do_filleting_geometry";
const OP_GEOM: &str = "intersect::curve_class::p7";

/// `SM_UNDEF_ULONG`: the loop index of a touch found without a target list.
pub const UNDEF_LOOP_INDEX: u64 = 123_456_789;

/// The verdict of [`CurveClassification::are_coincident_maybe`] (upstream `SmBoolean`
/// `FALSE`, `TRUE`, `UNSURE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoincidentMaybe {
    /// `FALSE`: not a coincident-maybe case, or not a fixable one.
    No,
    /// `TRUE`: a fixable coincident-maybe case (remove the reported interval end).
    Fixable,
    /// `UNSURE`: an edge classification whose object is not an edge.
    Unsure,
}

impl CoincidentMaybe {
    /// The upstream value (`FALSE` 0, `TRUE` 1, `UNSURE` 2).
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::No => 0,
            Self::Fixable => 1,
            Self::Unsure => 2,
        }
    }
}

/// The vertices met by the classification curve and their curve parameters
/// (`FindVertexObjects`' `rVertices`, `pOptClassifyParams`; the count is
/// `vertices.len()`).
#[derive(Debug, Clone, PartialEq)]
pub struct VertexObjects<O> {
    /// The vertices, without repeats.
    pub vertices: Vec<O>,
    /// The curve parameter of every vertex end met (one per end, so repeats of a vertex
    /// add parameters).
    pub params: Vec<f64>,
}

/// The result of [`CurveClassification::test_for_partial_coincident_edges`]
/// (`rdMaxPartialCoinGap`, `rpNewCurveToTest`).
#[derive(Debug, Clone, Default)]
pub struct PartialCoincidence {
    /// The largest gap at the ends of the coincident range; `0.0` for none.
    pub max_partial_coin_gap: f64,
    /// A copy of the edge curve trimmed to the coincident range, when the curve is
    /// partially coincident to an edge.
    pub new_curve_to_test: Option<BSplineCurve>,
}

/// A solution object naming nothing (the coincidence checks here pass no objects).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct NoObject;

impl SolutionObject for NoObject {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

impl<O: ClassObject> CurveClassification<O> {
    /// The point classification at interval boundary `index` (`0 ..= size()`) and its
    /// curve parameter (`GetPointClassByIndex`): the start of interval `index`, or the
    /// end of the last interval for `index == size()`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for `index > size()` or an empty classification
    /// (upstream reads outside the intervals).
    pub fn point_class_by_index(&self, index: usize) -> Result<(&PointClassification<O>, f64)> {
        let size = self.size();
        if size == 0 || index > size {
            return Err(bad_boundary());
        }
        if index == size {
            let ivl = self.at(index - 1)?;
            return Ok((&ivl.end, ivl.interval.max()));
        }
        let ivl = self.at(index)?;
        Ok((&ivl.start, ivl.interval.min()))
    }

    /// Where [`CurveClassification::point_class_by_index`]'s pointer points: the
    /// interval, and whether it is that interval's end (else its start).
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::point_class_by_index`].
    pub(crate) fn point_class_by_index_interval(&self, index: usize) -> Result<(usize, bool)> {
        let size = self.size();
        if size == 0 || index > size {
            return Err(bad_boundary());
        }
        if index == size {
            return Ok((index - 1, true));
        }
        Ok((index, false))
    }

    /// [`CurveClassification::point_class_by_index`], mutable.
    ///
    /// # Errors
    ///
    /// As [`CurveClassification::point_class_by_index`].
    pub fn point_class_by_index_mut(
        &mut self,
        index: usize,
    ) -> Result<(&mut PointClassification<O>, f64)> {
        let size = self.size();
        if size == 0 || index > size {
            return Err(bad_boundary());
        }
        if index == size {
            let ivl = self.at_mut(index - 1)?;
            let param = ivl.interval.max();
            return Ok((&mut ivl.end, param));
        }
        let ivl = self.at_mut(index)?;
        let param = ivl.interval.min();
        Ok((&mut ivl.start, param))
    }

    /// The point classification sharing boundary `index` with the one
    /// [`CurveClassification::point_class_by_index`] returns
    /// (`GetPointClassMateByIndex`): the end of interval `index - 1`. The first and
    /// last boundaries have no mate, except that boundary 0 of a curve closed over the
    /// classified interval mates the end of the last interval.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for `index > size()`, a missing curve at the first
    /// or last boundary (upstream dereferences it), and an empty classification whose
    /// curve is closed; those of [`Curve::is_closed`].
    pub fn point_class_mate_by_index(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
    ) -> Result<Option<&PointClassification<O>>> {
        match self.point_class_mate_interval(k, geometry, index)? {
            Some(i) => Ok(Some(&self.at(i)?.end)),
            None => Ok(None),
        }
    }

    /// The interval whose end [`CurveClassification::point_class_mate_by_index`]
    /// returns (where its pointer points), `None` for no mate.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::point_class_mate_by_index`].
    pub(crate) fn point_class_mate_interval(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
    ) -> Result<Option<usize>> {
        let size = self.size();
        if index > size {
            return Err(bad_boundary());
        }

        // No mate for first and last one unless interval is closed
        if index == size || index == 0 {
            let zone_tol_3d = src_zone_tol_3d_of(k, Some(self));
            let xsect_tol_3d = xsect_tol_3d_from_zones(k, zone_tol_3d, zone_tol_3d);

            let curve = self.curve_geometry(geometry)?;
            if curve.is_closed(k, &self.interval(), xsect_tol_3d)? && index == 0 {
                if size == 0 {
                    return Err(bad_boundary());
                }
                return Ok(Some(size - 1));
            }
            return Ok(None);
        }

        Ok(Some(index - 1))
    }

    /// Removes interval `index` whose length is degenerate (`CollapseInterval`): the
    /// combined end classification (see [`PointClassification::combine`]) becomes both
    /// ends and the neighbours' shared ends; interval parameters are not changed.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `index >= size()`, fewer than two intervals, or an interval that is
    /// not degenerate within the pair tolerance of its ends
    /// ([`xsect_tol_2d_or_3d_for_classify_pair`]); [`SolidError::InvalidInput`] for a
    /// missing curve; those of the tolerance and degeneracy computations and of
    /// [`PointClassification::combine`].
    pub fn collapse_interval(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
    ) -> Result<()> {
        // check input - lInterval index is valid
        if index >= self.size() {
            return Err(SolidError::intersect_failed(
                OP_COLLAPSE,
                "the interval index is past the end of the classification",
            ));
        }

        // Do not squeeze out the only interval.  [B232]
        if self.size() < 2 {
            return Err(SolidError::intersect_failed(
                OP_COLLAPSE,
                "the only interval of the classification is not squeezed out",
            ));
        }

        // locals
        let ivl = *self.at(index)?;

        let start_tol = ivl.start.obj_zone_tol_3d(k, geometry)?;
        let end_tol = ivl.end.obj_zone_tol_3d(k, geometry)?;
        let curve = self.curve_geometry(geometry)?;
        let surface1 = self.surface1_geometry(geometry)?;
        let xsect_tol_2d_or_3d = xsect_tol_2d_or_3d_for_classify_pair(
            k,
            curve,
            surface1,
            ivl.min(),
            start_tol,
            ivl.max(),
            end_tol,
        )?;

        // no work and an error - CurveClassification interval is not degenerate
        if !curve_is_degenerate(k, Some(curve), &ivl.interval, xsect_tol_2d_or_3d)? {
            return Err(SolidError::intersect_failed(
                OP_COLLAPSE,
                "the interval to collapse is not degenerate",
            ));
        }

        // pick the PtClassification data to persist
        let mut start = ivl.start;
        let kept = match start.combine(&ivl.end)? {
            Precedence::This => start,
            Precedence::Other => ivl.end,
        };
        {
            let target = self.at_mut(index)?;
            target.start = kept;
            target.end = kept;
        }

        // propagate the persistent PtClassification data to the neighbour boundaries
        if index >= 1 {
            self.at_mut(index - 1)?.end = kept;
        }
        if index + 2 <= self.size() {
            self.at_mut(index + 1)?.start = kept;
        }

        // compress the m_vData array
        self.remove_at(index)
    }

    /// The interval end classifications to vertices and edges, each boundary once, in
    /// curve order (`GetPointClassifiesConnectedTo`): all of them for an empty
    /// `targets`, else those whose object is connected to one of `targets` (the touch
    /// then records which). Touches carry `loop_dir` [`CrvDirOnFaceType::Uninit`]
    /// and, without targets, `loop_index` [`UNDEF_LOOP_INDEX`].
    ///
    /// [`CrvDirOnFaceType::Uninit`]: super::p1::CrvDirOnFaceType::Uninit
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an empty classification (upstream reads its
    /// missing first interval); those of [`PointClassification::is_connected_to_any`].
    pub fn point_classifies_connected_to(
        &self,
        geometry: &impl ClassGeometry<O>,
        targets: &[O],
    ) -> Result<Vec<TouchData<O>>> {
        // locals
        let mut touches = Vec::new();
        let mut found_index = UNDEF_LOOP_INDEX;
        let first = self.at(0).map_err(|_| empty_classification(OP_GEOM))?;

        let push = |touches: &mut Vec<TouchData<O>>, param: f64, index: usize, found_index: u64| {
            touches.push(TouchData {
                connected_to: if targets.is_empty() {
                    None
                } else {
                    usize::try_from(found_index)
                        .ok()
                        .and_then(|i| targets.get(i).copied())
                },
                loop_dir: super::p1::CrvDirOnFaceType::Uninit,
                loop_index: found_index,
                param,
                pt_classify_index: index as u64,
                pt_classify: None,
            });
        };

        // When First Interval Start Edge/Vertex Object is connected to a target
        if is_vertex_or_edge(&first.start)
            && (targets.is_empty()
                || connected_any(geometry, &first.start, targets, &mut found_index)?)
        {
            push(&mut touches, first.interval.min(), 0, found_index);
        }

        // for every Interval End
        for (ii, ivl) in self.intervals().iter().enumerate() {
            if is_vertex_or_edge(&ivl.end)
                && (targets.is_empty()
                    || connected_any(geometry, &ivl.end, targets, &mut found_index)?)
            {
                push(&mut touches, ivl.interval.max(), ii + 1, found_index);
            }
        }

        Ok(touches)
    }

    /// The vertices the classification curve meets (`FindVertexObjects`; upstream
    /// default `pOptConnectedTarget = NULL`): the first start and every interval end
    /// classified to a vertex connected to `connected_target` (any, for `None`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an empty classification;
    /// [`SolidError::Unported`] for a vertex (`SmVertex::IsConnectedTo`, unit
    /// `topology.vertex`).
    pub fn find_vertex_objects(
        &self,
        geometry: &impl ClassGeometry<O>,
        connected_target: Option<O>,
    ) -> Result<VertexObjects<O>> {
        // init output
        let mut out = VertexObjects {
            vertices: Vec::new(),
            params: Vec::new(),
        };

        // locals
        let first = self.at(0).map_err(|_| empty_classification(OP_GEOM))?;

        // for the first CurveInterval Start - add any VertexObject
        if let Some(v) = first.start.vertex_object() {
            if vertex_is_connected_to(geometry, v, connected_target)? {
                out.vertices.push(v);
                out.params.push(first.interval.min());
            }
        }

        // for every interval
        for ivl in self.intervals() {
            if let Some(v) = ivl.end.vertex_object() {
                if vertex_is_connected_to(geometry, v, connected_target)? {
                    if !out.vertices.contains(&v) {
                        out.vertices.push(v);
                    }
                    out.params.push(ivl.interval.max());
                }
            }
        }

        Ok(out)
    }

    /// The edges the classification curve meets (`FindEdgeObjects`; upstream defaults
    /// `pOptConnectedTarget = NULL`, `lWorkFlag = 3`): with bit 1 of `work_flag` the
    /// edges an interval is coincident with, with bit 2 the edges crossed at an
    /// interval end not next to a coincident interval; only edges connected to
    /// `connected_target` (any, for `None`), each once.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an empty classification;
    /// [`SolidError::Unported`] for an edge tested for connection
    /// (`SmEdge::IsConnectedTo`, unit `topology.edge`).
    pub fn find_edge_objects(
        &self,
        geometry: &impl ClassGeometry<O>,
        connected_target: Option<O>,
        work_flag: u64,
    ) -> Result<Vec<O>> {
        // init output
        let mut edges: Vec<O> = Vec::new();

        // locals
        let get_coins = work_flag & 1 != 0;
        let get_xsects = work_flag & 2 != 0;
        let first = self.at(0).map_err(|_| empty_classification(OP_GEOM))?;
        let start_edge = first.start.edge_object();
        let mut mid_edge = first.mid.edge_object();
        let mut mid_edge_next = mid_edge;

        // for the first CurveInterval Start - add any XSecting EdgeObject
        if get_xsects {
            if let Some(e) = start_edge {
                if edge_is_connected_to(geometry, e, connected_target)? && mid_edge.is_none() {
                    edges.push(e);
                }
            }
        }

        // for every interval - add Mid and End Edges as appropriate
        let size = self.size();
        for ii in 0..size {
            mid_edge = mid_edge_next;
            let end_edge = self.at(ii)?.end.edge_object();
            mid_edge_next = if ii < size - 1 {
                self.at(ii + 1)?.mid.edge_object()
            } else {
                None
            };

            // when asked and appropriate - add Coincident mid edges
            if get_coins {
                if let Some(e) = mid_edge {
                    if edge_is_connected_to(geometry, e, connected_target)? && !edges.contains(&e) {
                        edges.push(e);
                    }
                }
            }

            // when asked and appropriate - add XSecting end edges
            if get_xsects {
                if let Some(e) = end_edge {
                    if edge_is_connected_to(geometry, e, connected_target)?
                        && mid_edge.is_none()
                        && mid_edge_next.is_none()
                        && !edges.contains(&e)
                    {
                        edges.push(e);
                    }
                }
            }
        }

        Ok(edges)
    }

    /// Whether an edge crosses the classification curve (`IsCrossedByEdge`): an
    /// interval end on an edge, away from the edge's ends, where neither the interval
    /// nor its neighbour on that side is coincident with the edge.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for an interval end on an edge (`SmEdge::GetInterval`,
    /// unit `topology.edge`).
    pub fn is_crossed_by_edge(&self, geometry: &impl ClassGeometry<O>) -> Result<bool> {
        let size = self.size();

        // for every classification interval
        for ii in 0..size {
            let ivl = self.at(ii)?;

            // When interval starts on an EDGE
            if ivl.start.point_class() == PointClassType::Edge {
                let t = ivl.start.t_param();
                let edge = ivl.start.edge_object();
                let edge_ivl = edge_interval(geometry, edge)?;
                let sz = t.scaled_zero();

                // not crossing when XSection happens at bndry of pEdge Interval
                if edge_ivl.is_value_on_boundary(t, sz).0 {
                    continue;
                }

                // not crossing if pEdge is coincident with Classification Curve
                if ivl.mid.point_class() == PointClassType::Edge && edge == ivl.mid.edge_object() {
                    continue;
                }

                // not crossing if previous CrvIvl is coincident with this Edge
                if ii > 0 {
                    let prev = self.at(ii - 1)?;
                    if prev.end.point_class() == PointClassType::Edge
                        && prev.end.edge_object() == edge
                        && prev.mid.point_class() == PointClassType::Edge
                        && prev.mid.edge_object() == edge
                    {
                        continue;
                    }
                }

                // an Edge was found crossing the Classification Curve
                return Ok(true);
            }

            // When Interval ends on an Edge
            if ivl.end.point_class() == PointClassType::Edge {
                let t = ivl.end.t_param();
                let edge = ivl.end.edge_object();
                let edge_ivl = edge_interval(geometry, edge)?;
                let sz = t.scaled_zero();

                // not crossing if the intersection happens at boundary of the pEdge Interval
                if edge_ivl.is_value_on_boundary(t, sz).0 {
                    continue;
                }

                // not crossing if pEdge is coincident with Classification Curve
                if ivl.mid.point_class() == PointClassType::Edge && edge == ivl.mid.edge_object() {
                    continue;
                }

                // not crossing if next CrvIvl is coincident with this Edge
                if ii < size - 1 {
                    let next = self.at(ii + 1)?;
                    if next.start.point_class() == PointClassType::Edge
                        && next.start.edge_object() == edge
                        && next.mid.point_class() == PointClassType::Edge
                        && next.mid.edge_object() == edge
                    {
                        continue;
                    }
                }

                // an Edge was found crossing the Classification Curve
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Whether an interval interior is classified to a face (`IsCrossingFace`; upstream
    /// default `pOptTgtFace = NULL`): to `target_face`, or to any face for `None`.
    #[must_use]
    pub fn is_crossing_face(&self, target_face: Option<O>) -> bool {
        self.intervals().iter().any(|ivl| {
            ivl.mid.point_class() == PointClassType::Face
                && (target_face.is_none() || ivl.mid.object() == target_face)
        })
    }

    /// Whether a vertex end or a coincident edge is only a near miss of the curve
    /// (`HasNearMisses`; upstream default `bInSurfaceGaps = FALSE`): its gap is at
    /// least `1000 * SM_EFF_ZERO`, measured within the surface-1 tangent plane with
    /// `in_surface_gaps` and a surface 1 (see [`PointClassification::gap_3d`]). A vertex
    /// end that far off is measured again across the curve within surface 1, when the
    /// classification has a surface 1 and a UV curve 1 on it ([`Self::gap_across_curve`]),
    /// and is a near miss only when that gap reaches the threshold too: an offset along
    /// the curve or off the surface leaves no sliver beside the curve (deviation, see the
    /// file header). A coincident edge's gap is counted as upstream.
    ///
    /// # Errors
    ///
    /// Those of [`PointClassification::gap_3d`] with a surface;
    /// [`SolidError::InvalidInput`] for an unresolved surface handle with
    /// `in_surface_gaps`; [`SolidError::Unported`] and cancellation from
    /// [`Self::gap_across_curve`].
    pub fn has_near_misses(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        in_surface_gaps: bool,
    ) -> Result<bool> {
        // locals
        let surface = if in_surface_gaps {
            self.surface1_geometry(geometry)?
        } else {
            None
        };
        let near_miss_dist_3d = EFF_ZERO * 1000.0;
        // Only a potentially missed vertex needs the whole-span evidence for
        // its own-edge tolerance; ordinary endpoints require no extra drops.
        let vertex_misses = |pc: &PointClassification<O>, t: f64, end: bool| -> Result<bool> {
            if pc.point_class() != PointClassType::Vertex
                || pc.gap_3d(k, geometry, surface)? < near_miss_dist_3d
            {
                return Ok(false);
            }
            let across = match self.surface1().and_then(|s| geometry.surface(s)) {
                Some(s) => self.gap_across_curve(k, geometry, pc, t, end, s)?,
                None => None,
            };
            if across.is_some_and(|gap| gap < near_miss_dist_3d) {
                return Ok(false);
            }
            // A shared classification boundary is the same vertex from
            // either neighboring interval. Its coincident edge may be on
            // the other side of the interval being visited.
            let mut on_curve = false;
            if pc.object().is_some() {
                for adjacent in self.intervals() {
                    let shared_start =
                        adjacent.interval.min() == t && adjacent.start.object() == pc.object();
                    let shared_end =
                        adjacent.interval.max() == t && adjacent.end.object() == pc.object();
                    if (shared_start || shared_end)
                        && adjacent.mid.point_class() == PointClassType::Edge
                        && adjacent.mid.gap_3d(k, geometry, surface)? < near_miss_dist_3d
                        && self.coincident_span_lies_on_curve(
                            k,
                            geometry,
                            adjacent,
                            near_miss_dist_3d,
                        )?
                    {
                        on_curve = true;
                        break;
                    }
                }
            }
            let threshold = if on_curve && !self.edge_runs_beside(k, geometry, pc)? {
                max2(
                    near_miss_dist_3d,
                    object_zone_tol_3d(k, geometry, pc.object())?,
                )
            } else {
                near_miss_dist_3d
            };
            Ok(across.is_none_or(|gap| gap >= threshold))
        };

        for ivl in self.intervals() {
            let mid = &ivl.mid;
            let coincident = mid.point_class() == PointClassType::Edge;
            let beside = coincident && mid.gap_3d(k, geometry, surface)? >= near_miss_dist_3d;
            if vertex_misses(&ivl.start, ivl.interval.min(), false)?
                || beside
                || vertex_misses(&ivl.end, ivl.interval.max(), true)?
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Proves coincidence over the entire native edge definition, not at stations.
    /// A positive-weight rational B-spline is a convex combination of its Euclidean
    /// control points. Thus a strip containing every control point contains the
    /// whole curve, regardless of degree, knot spacing or a bow between samples.
    /// On a plane with an affine source the across-seam direction is constant;
    /// `native_curve_in_strip` encloses its projection with outward arithmetic.
    /// On a curved surface, an exact constant UV coordinate of every trim control
    /// proves that the represented edge lies on the same isoparametric support as
    /// the seam. A derivative remainder must additionally bound the actual stored
    /// 3D edge's across-seam separation over every native knot span. This uses the
    /// same native surface definition, never a cached approximation. Unproved
    /// definitions receive no endpoint tolerance relief.
    fn coincident_span_lies_on_curve(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        interval: &CurveInterval<O>,
        threshold: f64,
    ) -> Result<bool> {
        let (Some(curve), Some(surface), Some(object)) = (
            self.curve().and_then(|c| geometry.curve(c)),
            self.surface1().and_then(|s| geometry.surface(s)),
            interval.mid.object(),
        ) else {
            return Ok(false);
        };
        let other = if let Some((b, EntityRef::Edge(e))) = geometry.topology_entity(object) {
            let edge = b.edge(e)?;
            let Some(other) = edge.curve.as_ref() else {
                return Ok(false);
            };
            // Identity is a proof even for definitions with no polynomial form.
            if std::ptr::eq(curve, other) {
                return Ok(true);
            }
            match curve.kind() {
                CurveKind::CrvOnSurf(source) if same_native_surface(source.surface(), surface) => {
                    for eu in &edge.edgeuses {
                        let on_surface = crate::topology::edgeuse::p1::face(b, *eu)?
                            .and_then(|f| b.face(f).ok())
                            .and_then(|f| f.surface.as_ref())
                            .is_some_and(|s| std::ptr::eq(s, surface));
                        if !on_surface {
                            continue;
                        }
                        let Some(trim) = b.edgeuse(*eu)?.uv_trim_curve.as_ref() else {
                            continue;
                        };
                        if native_edge_on_iso(
                            k,
                            surface,
                            source.uv_curve(),
                            other,
                            trim,
                            edge.interval,
                            threshold,
                        )? {
                            return Ok(true);
                        }
                    }
                }
                _ => {}
            }
            other
        } else {
            let Some(other) = geometry.curve(object) else {
                return Ok(false);
            };
            other
        };
        if surface.procedural_definition().is_some() {
            return Ok(false);
        }
        k.check_cancelled(OP_GEOM)?;
        let (origin, across) = match surface.kind() {
            SurfaceKind::Plane(plane) => {
                let Some(line) = of_line(curve, interval.interval) else {
                    return Ok(false);
                };
                let (origin, tangent) = line.jet_over(Ivl::from(interval.interval.min()));
                let axes = plane.position();
                let normal =
                    interval_cross(interval_point(axes.x_axis()), interval_point(axes.y_axis()));
                (origin, interval_cross(normal, tangent))
            }
            SurfaceKind::Cone(_) => {
                let Some(uv_curve) = self.uv_curve1().and_then(|c| geometry.curve(c)) else {
                    return Ok(false);
                };
                let Some(frame) = ruling_frame(k, curve, uv_curve, surface, interval.interval)?
                else {
                    return Ok(false);
                };
                frame
            }
            _ => return Ok(false),
        };
        native_curve_in_strip(other, origin, across, threshold)
    }

    /// Whether an edge at the vertex end `pc` other than the curve's own edges runs beside
    /// the curve: its middle within the vertex's tolerance of the curve. Such an edge and
    /// the curve bound a sliver (a face between them a tolerance wide), where an edge that
    /// leaves the vertex across the curve makes a corner. `false` where the vertex, its
    /// edges or the curve cannot be resolved, or a drop fails.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and cancellation from the evaluations and the drop; any
    /// other failure counts as no edge beside.
    fn edge_runs_beside(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pc: &PointClassification<O>,
    ) -> Result<bool> {
        let (Some((b, EntityRef::Vertex(v))), Some(curve)) = (
            pc.object().and_then(|o| geometry.topology_entity(o)),
            self.curve().and_then(|c| geometry.curve(c)),
        ) else {
            return Ok(false);
        };
        let (Some(first), Some(last)) = (self.intervals().first(), self.intervals().last()) else {
            return Ok(false);
        };
        let span = Extent1d::from_min_max(first.interval.min(), last.interval.max());
        let tolerance = object_zone_tol_3d(k, geometry, pc.object())?;
        // the curve's own edges: those coincident with it
        let own: Vec<_> = self
            .intervals()
            .iter()
            .filter(|i| i.mid.point_class() == PointClassType::Edge)
            .filter_map(|i| i.mid.object().and_then(|o| geometry.topology_entity(o)))
            .filter_map(|(_, e)| match e {
                EntityRef::Edge(e) => Some(e),
                _ => None,
            })
            .collect();
        let Some(edges) = ignored(vertex::edges(b, v, None))? else {
            return Ok(false);
        };
        for e in edges.into_iter().filter(|e| !own.contains(e)) {
            let Ok(edge) = b.edge(e) else { continue };
            let Some(other) = edge.curve.as_ref() else {
                continue;
            };
            let Some(middle) = ignored(other.evaluate_point(k, edge.interval.mid()))? else {
                continue;
            };
            let Some(dropped) = ignored(curve.drop_point(
                k,
                &span,
                middle,
                None,
                tolerance,
                None,
                SolverOperation::Minimize,
            ))?
            else {
                continue;
            };
            if dropped.success && dropped.distance <= tolerance {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The gap of the vertex end `pc` at curve parameter `t` (the interval's end with
    /// `end`, its start without) across the curve within `surface`: the component of the
    /// vertex's offset from `C(t)` along `n × C'(t)`, `n` the surface normal at the UV
    /// curve 1's point, which shares the curve's parameter (the seam's `CrvOnSurf` and
    /// its UV curve). The tangent is the interval's own, from its side of `t`. `None`
    /// where that direction is not available: no curve or UV curve 1, a vanishing
    /// tangent or normal, or an evaluation that fails.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and cancellation from the evaluations; any other failure
    /// is `None`.
    #[allow(clippy::too_many_arguments)]
    fn gap_across_curve(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        pc: &PointClassification<O>,
        t: f64,
        end: bool,
        surface: &Surface,
    ) -> Result<Option<f64>> {
        let (Some(curve), Some(uv_curve)) = (
            self.curve().and_then(|c| geometry.curve(c)),
            self.uv_curve1().and_then(|c| geometry.curve(c)),
        ) else {
            return Ok(None);
        };
        let Some(derivs) = ignored(curve.evaluate(k, t, 1, end, false))? else {
            return Ok(None);
        };
        let (Some(&point), Some(&tangent)) = (derivs.first(), derivs.get(1)) else {
            return Ok(None);
        };
        let Some(uv) = ignored(uv_curve.evaluate_point(k, t))? else {
            return Ok(None);
        };
        let uv = Point2::new(uv.x, uv.y);
        let Some(normal) = ignored(surface.evaluate_normal(k, uv, Side::Left, Side::Left))? else {
            return Ok(None);
        };
        let Ok(across) = normal.cross(tangent).unitize() else {
            return Ok(None);
        };
        let Some(vertex) = ignored(pc.find_obj_point_3d(k, geometry))? else {
            return Ok(None);
        };
        Ok(Some((vertex - point).dot(across).abs()))
    }

    /// Whether interval `index` of `self` or of its partner `other` (`self` again to
    /// check one classification) only looks coincident with an edge and can be fixed by
    /// removing the interval (`AreCoincidentMaybe`; upstream default
    /// `bReportProb = FALSE`, whose messages are not reproduced). Returns the verdict
    /// and the interval end to remove ([`IntervalPosition::Inside`] when none was
    /// picked; a picked end stays reported when the case turns out not fixable, as
    /// upstream).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for `index` past either classification or a missing
    /// curve; [`SolidError::Unported`] in the edge-coincidence branch (edge tolerances
    /// and curves, the topology units).
    pub fn are_coincident_maybe(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &Self,
        index: usize,
        report_prob: bool,
    ) -> Result<(CoincidentMaybe, IntervalPosition)> {
        let _ = report_prob;

        // init output
        let mut position = IntervalPosition::Inside;
        let mut rtn = false;

        let this_ivl = self.at(index)?;
        let other_ivl = other.at(index)?;

        // no work - neither interval coincident (maybe) with an edge
        let edge = PointClassType::Edge;
        if (this_ivl.mid.point_class() != edge
            || (this_ivl.end.point_class() != edge && this_ivl.start.point_class() != edge))
            && (other_ivl.mid.point_class() != edge
                || (other_ivl.end.point_class() != edge && other_ivl.start.point_class() != edge))
        {
            return Ok((CoincidentMaybe::No, position));
        }

        // check four times: this(Start, Mid), other(Start, Mid), this(End, Mid),
        // other(End, Mid)
        for ii in 0..4 {
            // select tgt classification and interval
            let (tgt_class, tgt_ivl, mate_ivl, mate_owner) = if ii == 0 || ii == 2 {
                (self, this_ivl, other_ivl, other)
            } else {
                (other, other_ivl, this_ivl, self)
            };
            let tgt_curve_3d = tgt_class.curve_geometry(geometry)?;
            let on_surface = tgt_curve_3d.dim() == 2 && tgt_class.surface1().is_some();

            // select target end of the tgt interval
            let (tgt_beg, tgt_end, mate_beg, end_beg, end_end, tgt_param) = if ii < 2 {
                (
                    &tgt_ivl.start,
                    &tgt_ivl.end,
                    &mate_ivl.start,
                    IntervalPosition::Start,
                    IntervalPosition::End,
                    tgt_ivl.interval.min(),
                )
            } else {
                (
                    &tgt_ivl.end,
                    &tgt_ivl.start,
                    &mate_ivl.end,
                    IntervalPosition::End,
                    IntervalPosition::Start,
                    tgt_ivl.interval.max(),
                )
            };
            let tgt_mid = &tgt_ivl.mid;
            let mate_mid = &mate_ivl.mid;

            // when this interval mid and beg point look coincident with an edge
            if tgt_beg.point_class() == edge
                && tgt_mid.point_class() == edge
                && tgt_mid.object() == tgt_beg.object()
            {
                let xsect_tol_3d = xsect_tol_3d_from_zones(
                    k,
                    src_zone_tol_3d_of(k, Some(mate_owner)),
                    tgt_mid.obj_zone_tol_3d(k, geometry)?,
                );

                // get Edge, Interval, and Curve lengths
                let Some(edge_obj) = tgt_beg
                    .object()
                    .filter(|o| o.class_kind() == ClassObjectKind::Edge)
                else {
                    // probable bug - PointClassify classifies to SmEdge but its object
                    // is not an SmEdge
                    return Ok((CoincidentMaybe::Unsure, position));
                };
                let edge_param = tgt_beg.t_param();

                let edge_curve = edge_curve(geometry, edge_obj)?;
                let edge_ivl = edge_interval(geometry, Some(edge_obj))?;
                let tgt_curve: Cow<'_, Curve> = if on_surface {
                    Cow::Owned(crv_on_surf(
                        k,
                        tgt_curve_3d,
                        tgt_class.surface1_geometry(geometry)?,
                    )?)
                } else {
                    Cow::Borrowed(tgt_curve_3d)
                };
                let edge_len = edge_curve.approximate_length(k, &edge_ivl, 4, false)?.0;
                let cm_len = tgt_curve
                    .approximate_length(k, &tgt_ivl.interval, 4, false)?
                    .0;
                let total_cc_len = tgt_curve
                    .approximate_length(k, &self.interval(), 4, false)?
                    .0;

                // curve/edge angle at curve/edge intersection
                let angle_rad = get_curve_curve_angle(
                    k,
                    Some(edge_curve),
                    edge_param,
                    Some(&tgt_curve),
                    tgt_param,
                    xsect_tol_3d,
                )?;

                // case 1: original heuristic - left in to preserve current behavior
                let this_zone_tol_3d = src_zone_tol_3d_of(k, Some(self)) / 1000.0;
                let tgt_beg_gap_3d = tgt_beg.gap_3d(k, geometry, None)?;

                if (tgt_class.ignore_tangency() || tgt_beg_gap_3d > this_zone_tol_3d)
                    && (cm_len < edge_len / 10.0
                        || (self.ignore_tangency() && cm_len < total_cc_len / 2.0))
                {
                    // conclude this target interval is mislabeled as coincident
                    position = end_beg;
                    rtn = true;
                }

                // case 2: Curve runs close to vertex and intersects an existing edge
                // attached to the vertex
                let near_vertex = tgt_end.gap_3d(k, geometry, None)? > scaled_zero() * 100.0;
                let tight_crv_xsect = tgt_beg.gap_3d(k, geometry, None)? < scaled_zero() * 100.0;

                if !rtn
                    && tgt_end.point_class() == PointClassType::Vertex
                    && near_vertex
                    && tight_crv_xsect
                    && (cm_len < edge_len / 10.0
                        || cm_len < total_cc_len / 10.0
                        || cm_len < EFF_ZERO_SQRT)
                {
                    // remove the non-vertex end of the interval
                    position = end_end;
                    rtn = true;
                }

                // case 3: not yet an action (a place for a break upstream)
                let _ = !rtn && angle_rad > eff_zero_ang_rad() * 10.0;

                // when a coincident segment was identified
                if rtn {
                    let mb = mate_beg.point_class();
                    let mm = mate_mid.point_class();
                    let mate_mergeable = (mb == mm
                        && (mb == PointClassType::Edge
                            || mb == PointClassType::Face
                            || mb == PointClassType::Region))
                        || (mm == PointClassType::Face && mb == PointClassType::Unknown);

                    if mate_mergeable {
                        return Ok((CoincidentMaybe::Fixable, position));
                    }
                    // look for other fixable cases of coincident-maybe
                    rtn = false;
                }
            }
        }

        // arrive here when no removable coincident-maybe segments were found
        Ok((CoincidentMaybe::No, position))
    }

    /// Classifies interval `index`, known to lie on `face`, against it
    /// (`ClassifyFaceInterval`; upstream defaults `bUVSpaceOkay = TRUE`,
    /// `lFaceIndx = SM_UNDEF_ULONG`): through the vertex sector or edge side at its
    /// classified ends, else (with `do_point_classify`) by classifying a point near its
    /// middle (in UV space through the surface-1 UV curve when there is one and
    /// `uv_space_okay`). An interval found inside the face gets its interior, and any
    /// unknown or face end, classified to it. Only intervals whose interior is unknown,
    /// or classified to `old_face` (when given), are considered.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for `index >= size()` or a missing curve where it is
    /// used; `SM_ERR_NULL_POINTER` for a vertex or edge end without its object; those
    /// of the curve evaluations and face queries; [`SolidError::Unported`] for
    /// `FindVertexSector` (unit `topology.face_p4`), for a `PointClassify` that needs
    /// a UV trim curve made, and for face or edge handles the adapter cannot resolve.
    pub fn classify_face_interval(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        do_point_classify: bool,
        face: O,
        old_face: Option<O>,
        uv_space_okay: bool,
        face_index: Option<u64>,
    ) -> Result<()> {
        self.classify_face_interval_with(
            k,
            &mut ReadOnly(geometry),
            index,
            do_point_classify,
            face,
            old_face,
            uv_space_okay,
            face_index,
        )
    }

    /// [`CurveClassification::classify_face_interval`] with mutable access to the
    /// B-rep behind the face handle: the face queries (`SmFace::EdgeCurveOnClassify`,
    /// `Point3DClassify`, `FindVertexSector`, `PointClassify`) run on the B-rep that
    /// [`ClassGeometryMut::topology_entity_mut`] resolves the face handle to, so the
    /// UV trim curves and caches they make stay there, as upstream's `const` members
    /// leave them on the face's B-rep.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::classify_face_interval`]; a face handle that
    /// resolves read-only but not mutably keeps the read-only path's behaviour.
    #[allow(clippy::too_many_arguments)] // upstream's arguments
    pub fn classify_face_interval_mut(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ClassGeometryMut<O>,
        index: usize,
        do_point_classify: bool,
        face: O,
        old_face: Option<O>,
        uv_space_okay: bool,
        face_index: Option<u64>,
    ) -> Result<()> {
        self.classify_face_interval_with(
            k,
            &mut Mutable(geometry),
            index,
            do_point_classify,
            face,
            old_face,
            uv_space_okay,
            face_index,
        )
    }

    #[allow(clippy::too_many_arguments)] // upstream's arguments
    fn classify_face_interval_with(
        &mut self,
        k: &Kernel,
        access: &mut impl FaceQueries<O>,
        index: usize,
        do_point_classify: bool,
        face: O,
        old_face: Option<O>,
        uv_space_okay: bool,
        face_index: Option<u64>,
    ) -> Result<()> {
        // locals
        let mut ivl = *self.at(index)?;
        let src_zone_tol_3d = src_zone_tol_3d_of(k, Some(&*self));
        let ctx = FaceIntervalContext {
            curve: self.curve(),
            uv_curve1: self.uv_curve1(),
            src_zone_tol_3d,
            face,
            old_face,
            do_point_classify,
            uv_space_okay,
            face_index,
        };
        let r = classify_face_interval_on(k, access, &ctx, &mut ivl);
        *self.at_mut(index)? = ivl;
        r
    }

    /// Whether the classification and its partner `other` (`self` again to check one)
    /// are only partially coincident with an edge (`TestForPartialCoincidentEdges`):
    /// their first (or last) intervals lie on edges without starting (ending) on a
    /// vertex, or the single interval runs between vertices at one end pair only, and
    /// the two edges have a coincident range. The result then holds the largest gap at
    /// the range ends and a copy of the edge curve trimmed to the range; otherwise
    /// (empty classifications, a closed curve, complete coincidence) gap `0` and no
    /// curve.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an unresolved curve handle or a coincident
    /// edge curve that is not a B-spline; those of [`Curve::is_closed`] and the
    /// coincidence checks; [`SolidError::Unported`] where edges are examined (unit
    /// `topology.edge`).
    pub fn test_for_partial_coincident_edges(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &Self,
    ) -> Result<PartialCoincidence> {
        // init output
        let none = PartialCoincidence::default();
        let src_zone_tol_3d = src_zone_tol_3d_of(k, Some(self));
        let xsect_tol_3d = xsect_tol_3d_from_zones(k, src_zone_tol_3d, src_zone_tol_3d);

        // no work - one of the classifications has no segments
        if self.size() == 0 || other.size() == 0 {
            return Ok(none);
        }

        // no work - closed curve
        let curve = match self.curve() {
            Some(c) => Some(geometry.curve(c).ok_or(SolidError::invalid_input(
                OP_PARTIAL,
                "unresolved curve handle",
            ))?),
            None => None,
        };
        if tol_is_closed_curve(k, curve, xsect_tol_3d)? {
            return Ok(none);
        }

        // First check initial interval for edge coincidence that does not start on a
        // vertex.
        {
            let ivl = self.at(0)?;
            let o_ivl = other.at(0)?;
            if ivl.mid.point_class() == PointClassType::Edge
                && o_ivl.mid.point_class() == PointClassType::Edge
            {
                let vert_on_start = ivl.start.point_class() == PointClassType::Vertex
                    || o_ivl.start.point_class() == PointClassType::Vertex;
                if !vert_on_start {
                    if let Some(found) =
                        partial_coincidence(k, geometry, &ivl.mid, &o_ivl.mid, CoinCheck::Range)?
                    {
                        return Ok(found);
                    }
                }
            }
        }

        // Second, check last interval for edge coincidence that does not end on a
        // vertex.
        {
            let ivl = self.at(self.size() - 1)?;
            let o_ivl = other.at(other.size() - 1)?;
            if ivl.mid.point_class() == PointClassType::Edge
                && o_ivl.mid.point_class() == PointClassType::Edge
            {
                let vert_on_end = ivl.end.point_class() == PointClassType::Vertex
                    || o_ivl.end.point_class() == PointClassType::Vertex;
                if !vert_on_end {
                    if let Some(found) =
                        partial_coincidence(k, geometry, &ivl.mid, &o_ivl.mid, CoinCheck::Range)?
                    {
                        return Ok(found);
                    }
                }
            }
        }

        // when classification has just 1 segment
        if self.size() == 1 {
            let ivl = self.at(0)?;
            let o_ivl = other.at(0)?;

            if ivl.mid.point_class() == PointClassType::Edge
                && o_ivl.mid.point_class() == PointClassType::Edge
            {
                // remember if both segments start on a vertex (upstream tests this
                // interval's ends, then the other's)
                let coin1 = ivl.end.point_class() == PointClassType::Vertex
                    && ivl.start.point_class() == PointClassType::Vertex;
                let coin2 = o_ivl.start.point_class() == PointClassType::Vertex
                    && o_ivl.end.point_class() == PointClassType::Vertex;

                // both segments align to vertices - return no edge to check
                if coin1 && coin2 {
                    return Ok(none);
                }

                if coin1 {
                    if let Some(found) =
                        partial_coincidence(k, geometry, &ivl.mid, &o_ivl.mid, CoinCheck::Vertices)?
                    {
                        return Ok(found);
                    }
                }

                if coin2 {
                    if let Some(found) =
                        partial_coincidence(k, geometry, &ivl.mid, &o_ivl.mid, CoinCheck::Vertices)?
                    {
                        return Ok(found);
                    }
                }
            }
        }

        // all done
        Ok(none)
    }

    /// The resolved surface 1, if any.
    pub(super) fn surface1_geometry<'g>(
        &self,
        geometry: &'g impl ClassGeometry<O>,
    ) -> Result<Option<&'g Surface>> {
        match self.surface1() {
            Some(s) => geometry
                .surface(s)
                .map(Some)
                .ok_or(SolidError::invalid_input(
                    OP_GEOM,
                    "unresolved surface handle",
                )),
            None => Ok(None),
        }
    }
}

/// Copies fillet geometry from the other classification's topology onto the topology
/// interval `index` of `this` was merged into (`sm_SetDoFilletingGeometry`): with bit
/// 1 of `todo` the start vertex position, bit 2 the end vertex position, bit 4 the
/// interior edge curve (the other edge's curve trimmed to the matching range,
/// reparametrised to this edge's interval and reversed when it runs the other way).
/// Nothing is done unless `this` is doing fillets with an other classification;
/// `other` is the classification that handle names. Processing stops at the first
/// requested item whose ends are not both classified to vertices (edges).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `index` past either classification or a missing
/// `other` when `this` names one; `SM_ERR_NULL_POINTER` for a missing vertex or edge
/// object; [`SolidError::Unported`] for the topology edits (units `topology.vertex`,
/// `topology.edge`).
pub fn set_do_filleting_geometry<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    index: usize,
    this: &CurveClassification<O>,
    other: Option<&CurveClassification<O>>,
    todo: u64,
) -> Result<()> {
    set_do_filleting_geometry_with(k, &mut ReadOnly(geometry), index, this, other, todo, false)
}

/// [`set_do_filleting_geometry`] with mutable access to the B-rep behind the topology
/// handles: the vertex and edge modifications are applied directly to the entities.
///
/// # Errors
///
/// Those of [`set_do_filleting_geometry`].
pub fn set_do_filleting_geometry_mut<O: ClassObject>(
    k: &Kernel,
    geometry: &mut impl ClassGeometryMut<O>,
    index: usize,
    this: &CurveClassification<O>,
    other: Option<&CurveClassification<O>>,
    todo: u64,
) -> Result<()> {
    set_do_filleting_geometry_with(k, &mut Mutable(geometry), index, this, other, todo, false)
}

/// The same operation with an explicitly linked, actual local peer.
/// # Errors
/// The shared fillet geometry operation fails or lacks editing context.
pub(super) fn set_do_filleting_geometry_linked<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    index: usize,
    this: &CurveClassification<O>,
    other: &CurveClassification<O>,
    todo: u64,
) -> Result<()> {
    set_do_filleting_geometry_with(
        k,
        &mut ReadOnly(geometry),
        index,
        this,
        Some(other),
        todo,
        true,
    )
}

/// The linked-peer operation over actual mutable topology.
/// # Errors
/// The shared fillet geometry operation fails or lacks editing context.
pub(super) fn set_do_filleting_geometry_mut_linked<O: ClassObject>(
    k: &Kernel,
    geometry: &mut impl ClassGeometryMut<O>,
    index: usize,
    this: &CurveClassification<O>,
    other: &CurveClassification<O>,
    todo: u64,
) -> Result<()> {
    set_do_filleting_geometry_with(
        k,
        &mut Mutable(geometry),
        index,
        this,
        Some(other),
        todo,
        true,
    )
}

fn set_do_filleting_geometry_with<O: ClassObject>(
    k: &Kernel,
    access: &mut impl FilletAccess<O>,
    index: usize,
    this: &CurveClassification<O>,
    other: Option<&CurveClassification<O>>,
    todo: u64,
    linked: bool,
) -> Result<()> {
    // no work - don't have DoingFillet Data
    if !this.doing_fillets() || (!linked && this.other_classification().is_none()) {
        return Ok(());
    }

    // locals
    let other = other.ok_or(SolidError::invalid_input(
        OP_FILLET,
        "no other classification",
    ))?;
    let this_ivl = this.at(index)?;
    let other_ivl = other.at(index)?;

    // when asked - set StartVertex Geometry
    if todo & 1 != 0 {
        if this_ivl.start.point_class() != PointClassType::Vertex
            || other_ivl.start.point_class() != PointClassType::Vertex
        {
            return Ok(());
        }
        let this_v = this_ivl.start.vertex_object().ok_or_else(|| {
            SolidError::intersect_missing(OP_FILLET, "the vertex at this interval's start")
        })?;
        let other_v = other_ivl.start.vertex_object().ok_or_else(|| {
            SolidError::intersect_missing(OP_FILLET, "the vertex at the other interval's start")
        })?;
        let point = vertex_point(access.geometry(), other_v)?;
        access.set_vertex_point(this_v, point)?;
    }

    // when asked - set EndVertex Geometry
    if todo & 2 != 0 {
        if this_ivl.end.point_class() != PointClassType::Vertex
            || other_ivl.end.point_class() != PointClassType::Vertex
        {
            return Ok(());
        }
        let this_v = this_ivl.end.vertex_object().ok_or_else(|| {
            SolidError::intersect_missing(OP_FILLET, "the vertex at this interval's end")
        })?;
        let other_v = other_ivl.end.vertex_object().ok_or_else(|| {
            SolidError::intersect_missing(OP_FILLET, "the vertex at the other interval's end")
        })?;
        let point = vertex_point(access.geometry(), other_v)?;
        access.set_vertex_point(this_v, point)?;
    }

    // when asked - set MidEdge Geometry
    if todo & 4 != 0 {
        if this_ivl.mid.point_class() != PointClassType::Edge
            || other_ivl.mid.point_class() != PointClassType::Edge
        {
            return Ok(());
        }

        // this mid edge
        let this_edge = this_ivl.mid.edge_object().ok_or_else(|| {
            SolidError::intersect_missing(OP_FILLET, "the edge of this interval's middle")
        })?;
        let this_edge_curve = edge_curve(access.geometry(), this_edge)?;
        let this_edge_ivl = edge_interval(access.geometry(), Some(this_edge))?;

        // other mid edge
        let other_edge = other_ivl.mid.edge_object().ok_or_else(|| {
            SolidError::intersect_missing(OP_FILLET, "the edge of the other interval's middle")
        })?;
        let other_edge_curve = edge_curve(access.geometry(), other_edge)?;
        let other_edge_ivl = edge_interval(access.geometry(), Some(other_edge))?;

        // DropPoint locals
        let other_start_guess = other_edge_ivl.min();
        let other_end_guess = other_edge_ivl.max();
        let xsect_tol_3d = edge_pair_xsect_tol_3d(k, access.geometry(), this_edge, other_edge)?;

        // this and other edge end point samples (statuses ignored)
        let sample = |c: &Curve, t: f64| -> Result<[Vec3; 2]> {
            Ok(match ignored(c.evaluate(k, t, 1, true, true))? {
                Some(v) => [
                    v.first().copied().unwrap_or(Vec3::UNDEF),
                    v.get(1).copied().unwrap_or(Vec3::UNDEF),
                ],
                None => [Vec3::UNDEF; 2],
            })
        };
        let this_start = sample(this_edge_curve, this_edge_ivl.min())?;
        let this_end = sample(this_edge_curve, this_edge_ivl.max())?;
        let other_start = sample(other_edge_curve, other_edge_ivl.min())?;
        let other_end = sample(other_edge_curve, other_edge_ivl.max())?;

        let start_start_close = this_start[0].closer_than(xsect_tol_3d, other_start[0]);
        let start_end_close = this_start[0].closer_than(xsect_tol_3d, other_end[0]);
        let end_start_close = this_end[0].closer_than(xsect_tol_3d, other_start[0]);
        let end_end_close = this_end[0].closer_than(xsect_tol_3d, other_end[0]);

        // ThisStartPoint drop to OtherEdge->Curve
        let start_guess = if start_start_close {
            Some(other_start_guess)
        } else if start_end_close {
            Some(other_end_guess)
        } else {
            None
        };
        let start_drop = other_edge_curve.drop_point(
            k,
            &other_edge_ivl,
            this_start[0],
            None,
            xsect_tol_3d,
            start_guess,
            SolverOperation::Minimize,
        );

        // ThisEndPoint drop to OtherEdge->Curve
        let end_guess = if end_end_close {
            Some(other_end_guess)
        } else if end_start_close {
            Some(other_start_guess)
        } else {
            None
        };
        let end_drop = other_edge_curve.drop_point(
            k,
            &other_edge_ivl,
            this_end[0],
            None,
            xsect_tol_3d,
            end_guess,
            SolverOperation::Minimize,
        );

        // check both drops worked - do no harm
        let (Some(start_drop), Some(end_drop)) = (ignored(start_drop)?, ignored(end_drop)?) else {
            return Ok(());
        };
        if !start_drop.success || !end_drop.success {
            return Ok(());
        }
        let other_start_param = start_drop.parameter;
        let other_end_param = end_drop.parameter;

        // copy pOtherEdgeCurve
        let mut copy = other_edge_curve.copy(k)?;

        // Trim interval bounded by DropPoint Params (may snap to existing knots)
        let mut final_ivl = Extent1d::new();
        ignored(final_ivl.set_min_max(
            min2(other_start_param, other_end_param),
            max2(other_start_param, other_end_param),
        ))?;
        ignored(copy.trim(k, &mut final_ivl, TrimMode::Notify))?;

        // reparameterize the OtherCurveCopy to the ThisEdge Interval (not upstream, #15505:
        // exactly, as ThisEdge keeps its interval; ends the same within the scaled zero
        // were left outside the copy's natural interval)
        ignored(copy.edit_parameterization_exactly(k, &this_edge_ivl))?;

        // when needed reverse parameterization
        if (((start_start_close && start_end_close) || (end_start_close && end_end_close))
            && this_start[1].dot(other_start[1]) <= 0.0)
            || (((!start_start_close || !start_end_close) && (!end_start_close || !end_end_close))
                && other_start_param > other_end_param)
        {
            ignored(copy.reverse_parameterization(k, &this_edge_ivl))?;
        }

        // Set ThisEdge->Curve from OtherEdge->Curve Copy
        access.set_edge_curve(this_edge, copy)?;
    }

    Ok(())
}

// ---- classify face interval ------------------------------------------------------------

/// The classification data `ClassifyFaceInterval` reads besides the interval.
struct FaceIntervalContext<O> {
    curve: Option<O>,
    uv_curve1: Option<O>,
    src_zone_tol_3d: f64,
    face: O,
    old_face: Option<O>,
    do_point_classify: bool,
    uv_space_okay: bool,
    face_index: Option<u64>,
}

/// The answer of a face query on an interval end: inside the face, and whether the
/// answer is to be doubted (`bPossibleProblem`).
#[derive(Debug, Clone, Copy, Default)]
struct EndInside {
    inside: bool,
    possible_problem: bool,
}

/// The classification curve `m_cpCurve` of a [`FaceIntervalContext`].
fn context_curve<'g, O: ClassObject>(
    geometry: &'g impl ClassGeometry<O>,
    ctx: &FaceIntervalContext<O>,
) -> Result<&'g Curve> {
    let c = ctx.curve.ok_or(SolidError::invalid_input(
        OP_FACE,
        "no classification curve",
    ))?;
    geometry.curve(c).ok_or(SolidError::invalid_input(
        OP_FACE,
        "unresolved curve handle",
    ))
}

/// The classification's surface-1 UV curve (`GetUVCurve1()`) of a
/// [`FaceIntervalContext`].
fn context_uv_curve1<'g, O: ClassObject>(
    geometry: &'g impl ClassGeometry<O>,
    ctx: &FaceIntervalContext<O>,
) -> Result<Option<&'g Curve>> {
    match ctx.uv_curve1 {
        Some(c) => geometry.curve(c).map(Some).ok_or(SolidError::invalid_input(
            OP_FACE,
            "unresolved UV curve handle",
        )),
        None => Ok(None),
    }
}

/// `*m_cpCurve` and `GetUVCurve1()` of a [`FaceIntervalContext`], copied out of the
/// adapter (in that order: the first unresolved one is the error).
fn context_curves<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    ctx: &FaceIntervalContext<O>,
) -> Result<(Curve, Option<Curve>)> {
    let curve = context_curve(geometry, ctx)?.clone();
    let uv_curve = context_uv_curve1(geometry, ctx)?.cloned();
    Ok((curve, uv_curve))
}

/// The body of [`CurveClassification::classify_face_interval`] on a copy of the
/// interval. The curves are copied for the face queries, which may need the adapter
/// mutably.
fn classify_face_interval_on<O: ClassObject>(
    k: &Kernel,
    access: &mut impl FaceQueries<O>,
    ctx: &FaceIntervalContext<O>,
    ivl: &mut CurveInterval<O>,
) -> Result<()> {
    // no work - ClassifMid not classified to UNKNOWN or FACE when given pOldFace
    let mid_class = ivl.mid.point_class();
    if !(mid_class == PointClassType::Unknown
        || (ctx.old_face.is_some() && mid_class == PointClassType::Face))
    {
        return Ok(());
    }

    // no work - OldFace not ivl's classifObj
    if ctx.old_face.is_some()
        && ivl.mid.face_object().is_some()
        && ctx.old_face != ivl.mid.face_object()
    {
        return Ok(());
    }

    let face = ctx.face;
    let mut did_min = false;
    let mut did_mid = false;
    let mut did_max = false;
    let mut min = EndInside::default();
    let mut max = EndInside::default();

    // 1. When StartPoint is classified to a vertex or edge (looking forwards)
    match ivl.start.point_class() {
        PointClassType::Vertex => {
            did_min = true;
            let v = ivl.start.object().ok_or_else(|| {
                SolidError::intersect_missing(OP_FACE, "the vertex at the interval's start")
            })?;
            let (curve, uv_curve) = context_curves(access.geometry(), ctx)?;
            min = access.find_vertex_sector(
                k,
                face,
                v,
                &curve,
                uv_curve.as_ref(),
                ivl.interval.min(),
                ivl.interval.max(),
                OrientType::Same,
                ctx.src_zone_tol_3d,
                ctx.uv_space_okay,
            )?;
        }
        PointClassType::Edge => {
            did_min = true;
            let e = ivl.start.object().ok_or_else(|| {
                SolidError::intersect_missing(OP_FACE, "the edge at the interval's start")
            })?;
            let curve = context_curve(access.geometry(), ctx)?.clone();
            min = access.edge_curve_on_classify(
                k,
                face,
                e,
                ivl.start.t_param(),
                &curve,
                &ivl.interval,
                OrientType::Same,
                ctx.src_zone_tol_3d,
                ctx.uv_space_okay,
                None,
            )?;
        }
        _ => {}
    }

    // 2. when EndPoint is classified to a vertex or edge (looking backwards)
    match ivl.end.point_class() {
        PointClassType::Vertex => {
            did_max = true;
            let v = ivl.end.object().ok_or_else(|| {
                SolidError::intersect_missing(OP_FACE, "the vertex at the interval's end")
            })?;
            let (curve, uv_curve) = context_curves(access.geometry(), ctx)?;
            max = access.find_vertex_sector(
                k,
                face,
                v,
                &curve,
                uv_curve.as_ref(),
                ivl.interval.max(),
                ivl.interval.min(),
                OrientType::Opposite,
                ctx.src_zone_tol_3d,
                ctx.uv_space_okay,
            )?;
        }
        PointClassType::Edge => {
            did_max = true;
            let e = ivl.end.object().ok_or_else(|| {
                SolidError::intersect_missing(OP_FACE, "the edge at the interval's end")
            })?;
            let curve = context_curve(access.geometry(), ctx)?.clone();
            max = access.edge_curve_on_classify(
                k,
                face,
                e,
                ivl.end.t_param(),
                &curve,
                &ivl.interval,
                OrientType::Opposite,
                ctx.src_zone_tol_3d,
                ctx.uv_space_okay,
                ctx.face_index,
            )?;
        }
        _ => {}
    }

    // 4. Do a PointClassification when interval couldn't be classified through
    // endPoint classifications
    if ctx.do_point_classify && !did_min && !did_max {
        did_mid = true;

        // pick a near interval mid-point curve parameter
        let param = ivl.interval.evaluate(0.4567);

        let uv = match context_uv_curve1(access.geometry(), ctx)? {
            Some(uv_curve) if ctx.uv_space_okay => {
                let pnt = uv_curve.evaluate_point(k, param)?;
                Some(Point2::new(pnt.x, pnt.y))
            }
            _ => None,
        };
        if let Some(uv) = uv {
            // Don't put this face's classification directly into m_vMid [B97]
            let mut temp = PointClassification::new(ctx.src_zone_tol_3d);
            access.point_classify(k, face, uv, ctx.src_zone_tol_3d, false, true, &mut temp)?;
            if temp.point_class() != PointClassType::Unknown {
                ivl.mid = temp;
            }
        } else {
            let pnt = context_curve(access.geometry(), ctx)?.evaluate_point(k, param)?;

            // Don't put this face's classification directly into m_vMid [B97]
            let mut temp = PointClassification::new(ctx.src_zone_tol_3d);
            // status ignored (assume the point was not on the surface)
            ignored(access.point_3d_classify(k, face, pnt, ctx.src_zone_tol_3d, false, &mut temp))?;
            if temp.point_class() != PointClassType::Unknown {
                ivl.mid = temp;
            }
        }
    }

    // 5. check for endPoint-based classification conflicts
    if did_min && did_max && min.inside != max.inside {
        // Fix 1: use lone classification not marked as problematic
        if min.possible_problem && !max.possible_problem {
            min.inside = max.inside;
        } else if !min.possible_problem && max.possible_problem {
            max.inside = min.inside;
        } else {
            // Fix 2: use classification from endPoint with smallest deviation
            let start_dev = ivl.start.deviation();
            let end_dev = ivl.end.deviation();
            if start_dev > end_dev + 1.0E-10 {
                min.inside = max.inside;
            } else if start_dev < end_dev - 1.0E-10 {
                max.inside = min.inside;
            } else {
                return Ok(());
            }
        }
    }

    // 6. check for successful inside classification of interval to the face
    if (did_min && min.inside)
        || (did_max && max.inside)
        || (did_mid
            && ivl.mid.point_class() == PointClassType::Face
            && ivl.mid.object() == Some(face))
    {
        // when the classification was done through the endPoints
        if !did_mid {
            ivl.mid.set_class_object(PointClassType::Face, Some(face));
        }

        // set any StartPoint or EndPoint with face (or no) classifications to the
        // MidPoint's face
        let mid_obj = ivl.mid.object();
        if matches!(
            ivl.start.point_class(),
            PointClassType::Face | PointClassType::Unknown
        ) {
            ivl.start.set_class_object(PointClassType::Face, mid_obj);
        }
        if matches!(
            ivl.end.point_class(),
            PointClassType::Face | PointClassType::Unknown
        ) {
            ivl.end.set_class_object(PointClassType::Face, mid_obj);
        }
    }

    Ok(())
}

// ---- partial coincidence ---------------------------------------------------------------

/// Which return rule a coincidence block of `TestForPartialCoincidentEdges` follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoinCheck {
    /// First / last interval: return the first coincident range.
    Range,
    /// Single interval between vertices at one end pair: stop at the first coincident
    /// range, reporting it only when its gap exceeds a third of the tolerance.
    Vertices,
}

/// One coincidence block of `TestForPartialCoincidentEdges` over the edges of two
/// interval interiors: `Some` to return (with or without a curve), `None` to go on.
fn partial_coincidence<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    mid: &PointClassification<O>,
    o_mid: &PointClassification<O>,
    check: CoinCheck,
) -> Result<Option<PartialCoincidence>> {
    let edge = mid.object().ok_or_else(|| {
        SolidError::intersect_missing(OP_PARTIAL, "the edge of the interval's middle")
    })?;
    let o_edge = o_mid.object().ok_or_else(|| {
        SolidError::intersect_missing(OP_PARTIAL, "the edge of the other interval's middle")
    })?;
    let curve = edge_curve(geometry, edge)?;
    let o_curve = edge_curve(geometry, o_edge)?;
    let edge_ivl = edge_interval(geometry, Some(edge))?;
    let o_edge_ivl = edge_interval(geometry, Some(o_edge))?;
    let xsect_tol_3d = edge_pair_xsect_tol_3d(k, geometry, edge, o_edge)?;

    let coincide = |tol: f64| -> Result<Vec<Solution<NoObject>>> {
        Ok(curve
            .global_coincidence_checker(k, &edge_ivl, o_curve, &o_edge_ivl, tol, true, [None; 2])?
            .solutions)
    };

    // find coincident points and spans between the two segments with larger tol
    let mut solutions = coincide(3.0 * xsect_tol_3d)?;

    let mut i = 0;
    while i < solutions.len() {
        let Some(mut sol) = solutions.get(i).copied() else {
            break;
        };

        // skip points of coincidence
        if sol.solution_type != SolutionType::RangeOfValues {
            i += 1;
            continue;
        }

        // set maxTol to max deviation at coincident range end points
        let mut max_gap = max2(sol.start.solution_value, sol.end.solution_value);

        // when MaxGap is larger tol
        if max_gap > xsect_tol_3d {
            // check for coincidence with normal tolerance (into the same array:
            // rSol now reads the re-run's entry i, or keeps the old one)
            solutions = coincide(xsect_tol_3d)?;
            if let Some(s) = solutions.get(i) {
                sol = *s;
            }

            // when segments still have coincident regions
            if let Some(sol2) = solutions.first() {
                if sol2.solution_type == SolutionType::RangeOfValues {
                    max_gap = max2(sol.start.solution_value, sol.end.solution_value);
                }
            }
        }

        if check == CoinCheck::Vertices && max_gap <= xsect_tol_3d / 3.0 {
            return Ok(Some(PartialCoincidence::default()));
        }

        // return a copy of the edge curve trimmed to the coincident range
        // `new SmBSplineCurve(*pBSC)`: a line or conic is sliced to its B-spline part
        let bsc = as_b_spline(curve).ok_or(SolidError::invalid_input(
            OP_PARTIAL,
            "coincident edge curve is not a B-spline",
        ))?;
        let mut copy = bsc.clone();
        let mut coin_domain = Extent1d::from_min_max(
            sol.start.parameters.first().copied().unwrap_or(0.0),
            sol.end.parameters.first().copied().unwrap_or(0.0),
        );
        ignored(copy.trim(k, &mut coin_domain, TrimMode::Notify))?;

        return Ok(Some(PartialCoincidence {
            max_partial_coin_gap: max_gap,
            new_curve_to_test: Some(copy),
        }));
    }

    Ok(None)
}

// ---- helpers -----------------------------------------------------------------------------

/// `SM_CAST_PTR(SmBSplineCurve, ..)`: the B-spline (base) of the kinds derived from
/// `SmBSplineCurve` (`SmLine`, `SmEllipse`/`SmCircle`, `SmHyperbola`, `SmParabola`), `None`
/// for the others.
const fn as_b_spline(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        CurveKind::Hermite(_)
        | CurveKind::Conic(_)
        | CurveKind::Projected(_)
        | CurveKind::Iso(_)
        | CurveKind::Offset(_)
        | CurveKind::CrvInVolume(_)
        | CurveKind::CrvOnSurf(_)
        | CurveKind::TangentField(_)
        | CurveKind::Composite(_) => None,
    }
}

fn bad_boundary() -> SolidError {
    SolidError::invalid_input(OP_INDEX, "boundary index out of range")
}

fn empty_classification(op: &'static str) -> SolidError {
    SolidError::invalid_input(op, "empty classification")
}

fn is_vertex_or_edge<O: ClassObject>(pc: &PointClassification<O>) -> bool {
    matches!(
        pc.point_class(),
        PointClassType::Vertex | PointClassType::Edge
    )
}

/// `IsConnectedTo(crTgtOwners, lFoundIndx)`: on a match the found index is updated.
fn connected_any<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    pc: &PointClassification<O>,
    targets: &[O],
    found_index: &mut u64,
) -> Result<bool> {
    match pc.is_connected_to_any(geometry, targets)? {
        Some(i) => {
            *found_index = i as u64;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// `SmTol::IsClosed(const SmObject*, SmXSectTol3d*)` for a curve with the tolerance
/// given: closed over its natural interval; false for no curve.
fn tol_is_closed_curve(k: &Kernel, curve: Option<&Curve>, xsect_tol_3d: f64) -> Result<bool> {
    match curve {
        None => Ok(false),
        Some(c) => c.is_closed(k, &c.natural_interval(), xsect_tol_3d),
    }
}

/// `SmCrvOnSurf(uv_curve, surface)` as a curve.
fn crv_on_surf(k: &Kernel, uv_curve: &Curve, surface: Option<&Surface>) -> Result<Curve> {
    let Some(surface) = surface else {
        return Err(SolidError::invalid_input(OP_COIN, "no surface 1"));
    };
    Ok(Curve::from(CrvOnSurf::new(
        k,
        uv_curve.clone_for_evaluation()?,
        surface.clone_for_evaluation()?,
        None,
        0,
    )?))
}

// ---- callees in units not ported yet ---------------------------------------------------

/// The B-rep edge behind edge handle `edge` ([`ClassGeometry::topology_entity`]); an
/// adapter without that hook keeps the deferral on `upstream`.
pub(super) fn edge_of<'g, O: ClassObject>(
    geometry: &'g impl ClassGeometry<O>,
    edge: O,
    upstream: &'static str,
) -> Result<(&'g Brep, EdgeId)> {
    match geometry.topology_entity(edge) {
        Some((b, EntityRef::Edge(e))) => Ok((b, e)),
        Some(_) => Err(SolidError::invalid_input(OP_GEOM, "handle is not an edge")),
        None => Err(SolidError::unported(upstream, "topology.edge")),
    }
}

/// The B-rep vertex behind vertex handle `vertex`; an adapter without the
/// [`ClassGeometry::topology_entity`] hook keeps the deferral on `upstream`.
pub(super) fn vertex_of<'g, O: ClassObject>(
    geometry: &'g impl ClassGeometry<O>,
    vertex: O,
    upstream: &'static str,
) -> Result<(&'g Brep, VertexId)> {
    match geometry.topology_entity(vertex) {
        Some((b, EntityRef::Vertex(v))) => Ok((b, v)),
        Some(_) => Err(SolidError::invalid_input(OP_GEOM, "handle is not a vertex")),
        None => Err(SolidError::unported(upstream, "topology.vertex")),
    }
}

/// The entity of connection target `target` in B-rep `b`; `None` for a target in
/// another B-rep, connected to nothing here.
fn connect_target<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    b: &Brep,
    target: O,
    upstream: &'static str,
    unit: &'static str,
) -> Result<Option<EntityRef>> {
    let (tb, e) = geometry
        .topology_entity(target)
        .ok_or_else(|| SolidError::unported(upstream, unit))?;
    Ok(std::ptr::eq(b, tb).then_some(e))
}

/// `SmEdge::GetCurve`; a `NULL` curve is dereferenced by every caller.
fn edge_curve<O: ClassObject>(geometry: &impl ClassGeometry<O>, edge: O) -> Result<&Curve> {
    let (b, e) = edge_of(geometry, edge, "SmEdge::GetCurve")?;
    edge::curve(b, e)?.ok_or_else(|| SolidError::intersect_missing(OP_GEOM, "the edge's curve"))
}

/// `SmEdge::GetInterval` (of the edge object of a classification; a `NULL` edge is
/// dereferenced upstream).
fn edge_interval<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    edge: Option<O>,
) -> Result<Extent1d> {
    let edge =
        edge.ok_or_else(|| SolidError::intersect_missing(OP_GEOM, "the classification's edge"))?;
    let (b, e) = edge_of(geometry, edge, "SmEdge::GetInterval")?;
    edge::interval(b, e)
}

/// `SmEdge::IsConnectedTo`.
fn edge_is_connected_to<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    edge: O,
    target: Option<O>,
) -> Result<bool> {
    const UPSTREAM: &str = "SmEdge::IsConnectedTo";
    // no work - no Topo (IsConnectedTo(NULL) is TRUE before any member is read)
    let Some(target) = target else {
        return Ok(true);
    };
    let (b, e) = edge_of(geometry, edge, UPSTREAM)?;
    match connect_target(geometry, b, target, UPSTREAM, "topology.edge")? {
        Some(t) => edge::is_connected_to(b, e, Some(t)),
        None => Ok(false),
    }
}

/// `SmVertex::IsConnectedTo`.
fn vertex_is_connected_to<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    vertex: O,
    target: Option<O>,
) -> Result<bool> {
    const UPSTREAM: &str = "SmVertex::IsConnectedTo";
    // no work - no Topo (IsConnectedTo(NULL) is TRUE before any member is read)
    let Some(target) = target else {
        return Ok(true);
    };
    let (b, v) = vertex_of(geometry, vertex, UPSTREAM)?;
    match connect_target(geometry, b, target, UPSTREAM, "topology.vertex")? {
        Some(t) => vertex::is_connected_to(b, v, Some(t)),
        None => Ok(false),
    }
}

/// `SmVertex::GetPoint`.
fn vertex_point<O: ClassObject>(geometry: &impl ClassGeometry<O>, vertex: O) -> Result<Point3> {
    let (b, v) = vertex_of(geometry, vertex, "SmVertex::GetPoint")?;
    vertex::point(b, v)
}

/// `SmTol::GetXSectTol3d(const SmObject*, const SmObject*)` of two edges: the sum of
/// their zone tolerances (`GetTolerance`).
fn edge_pair_xsect_tol_3d<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    edge: O,
    other: O,
) -> Result<f64> {
    Ok(xsect_tol_3d_from_zones(
        k,
        object_zone_tol_3d(k, geometry, Some(edge))?,
        object_zone_tol_3d(k, geometry, Some(other))?,
    ))
}

// ---- face queries ------------------------------------------------------------------------

/// The face queries of `ClassifyFaceInterval` (upstream's `const` `SmFace` members,
/// which may add UV trim curves and caches to the face's B-rep) over either kind of
/// adapter.
trait FaceQueries<O: ClassObject> {
    type Geometry: ClassGeometry<O>;
    fn geometry(&self) -> &Self::Geometry;
    /// `SmFace::FindVertexSector(pV, *m_cpCurve, GetUVCurve1(), from, to, direction,
    /// sSrcZoneTol3d, TRUE, pVUFound, bPossibleProblem, eProblem, bUVSpaceOkay)`:
    /// inside when a vertex use sector of `face` at `vertex` contains the curve leaving
    /// `from` towards `to`.
    #[allow(clippy::too_many_arguments)] // upstream's arguments
    fn find_vertex_sector(
        &mut self,
        k: &Kernel,
        face: O,
        vertex: O,
        curve: &Curve,
        uv_curve: Option<&Curve>,
        from: f64,
        to: f64,
        direction: OrientType,
        src_zone_tol_3d: f64,
        uv_space_okay: bool,
    ) -> Result<EndInside>;
    /// `SmFace::EdgeCurveOnClassify(pE, dEdgeParam, *m_cpCurve, ivl, eCurveDirection,
    /// sSrcZoneTol3d, rbInside, rbPossibleProblem, reProblem, bUVSpaceOkay[,
    /// lFaceIndx])`.
    #[allow(clippy::too_many_arguments)] // upstream's arguments
    fn edge_curve_on_classify(
        &mut self,
        k: &Kernel,
        face: O,
        edge: O,
        edge_param: f64,
        curve: &Curve,
        interval: &Extent1d,
        direction: OrientType,
        src_zone_tol_3d: f64,
        uv_space_okay: bool,
        face_index: Option<u64>,
    ) -> Result<EndInside>;
    /// `SmFace::PointClassify(uv, tol, bDoXSects, bUseUV, pc)`.
    #[allow(clippy::too_many_arguments)] // upstream's arguments
    fn point_classify(
        &mut self,
        k: &Kernel,
        face: O,
        uv: Point2,
        src_zone_tol_3d: f64,
        do_xsects: bool,
        use_uv: bool,
        point_class: &mut PointClassification<O>,
    ) -> Result<()>;
    /// `SmFace::Point3DClassify(point, tol, bDoXSects, pc)`.
    fn point_3d_classify(
        &mut self,
        k: &Kernel,
        face: O,
        point: Point3,
        src_zone_tol_3d: f64,
        do_xsects: bool,
        point_class: &mut PointClassification<O>,
    ) -> Result<()>;
}

/// The mutable and read-only topology operations of [`set_do_filleting_geometry_with`].
trait FilletAccess<O: ClassObject> {
    type Geometry: ClassGeometry<O>;
    fn geometry(&self) -> &Self::Geometry;
    fn set_vertex_point(&mut self, vertex: O, point: Point3) -> Result<()>;
    fn set_edge_curve(&mut self, edge: O, curve: Curve) -> Result<()>;
}

/// A read-only adapter: `EdgeCurveOnClassify` and `Point3DClassify` run on a copy of
/// the face's B-rep (#614); `PointClassify` runs over the unchanged topology (a UV trim
/// it would make defers); `FindVertexSector` defers until the caller gives mutable
/// access ([`CurveClassification::classify_face_interval_mut`]).
struct ReadOnly<'g, G>(&'g G);

impl<O: ClassObject, G: ClassGeometry<O>> FilletAccess<O> for ReadOnly<'_, G> {
    type Geometry = G;
    fn geometry(&self) -> &G {
        self.0
    }
    fn set_vertex_point(&mut self, _vertex: O, _point: Point3) -> Result<()> {
        // DISPATCH GAP: SmVertex::SetPoint (topology.vertex, green)
        // Proof (unreachable): the ReadOnly view serves queries that only make caches, and
        // 0 hits in the green difftest sweep show none met this; Mutable and the B-rep
        // impls resolve it.
        Err(SolidError::unported(
            "SmVertex::SetPoint",
            "topology.vertex",
        ))
    }
    fn set_edge_curve(&mut self, _edge: O, _curve: Curve) -> Result<()> {
        // DISPATCH GAP: SmEdge::SetCurve (topology.edge, green)
        // Proof (unreachable): the ReadOnly view serves queries that only make caches, and
        // 0 hits in the green difftest sweep show none met this; Mutable and the B-rep
        // impls resolve it.
        Err(SolidError::unported("SmEdge::SetCurve", "topology.edge"))
    }
}

impl<O: ClassObject, G: ClassGeometry<O>> FaceQueries<O> for ReadOnly<'_, G> {
    type Geometry = G;
    fn geometry(&self) -> &G {
        self.0
    }
    fn find_vertex_sector(
        &mut self,
        _k: &Kernel,
        _face: O,
        _vertex: O,
        _curve: &Curve,
        _uv_curve: Option<&Curve>,
        _from: f64,
        _to: f64,
        _direction: OrientType,
        _src_zone_tol_3d: f64,
        _uv_space_okay: bool,
    ) -> Result<EndInside> {
        // UNPORTED(topology.face_p4): SmFace::FindVertexSector
        // Proof (unreachable): the ReadOnly view serves queries that only make caches, and
        // 0 hits in the green difftest sweep show none met this; Mutable and the B-rep
        // impls resolve it.
        Err(SolidError::unported(
            "SmFace::FindVertexSector",
            "topology.face_p4",
        ))
    }
    fn edge_curve_on_classify(
        &mut self,
        k: &Kernel,
        face: O,
        edge: O,
        edge_param: f64,
        curve: &Curve,
        interval: &Extent1d,
        direction: OrientType,
        src_zone_tol_3d: f64,
        uv_space_okay: bool,
        face_index: Option<u64>,
    ) -> Result<EndInside> {
        let unresolved = || {
            // DISPATCH GAP: the adapter cannot resolve the face or edge handle to its
            // `Brep` entity (`ClassGeometry::topology_entity`).
            SolidError::unported("SmFace::EdgeCurveOnClassify", "query.face_p1")
        };
        let (b, f) = resolve_face(self.0, face).ok_or_else(unresolved)?;
        let Some(e) = same_brep_edge(self.0, b, edge).ok_or_else(unresolved)?? else {
            return Ok(EndInside::default());
        };
        let mut copy = b.clone_standing_for_self()?;
        edge_curve_on_classify_in(
            k,
            &mut copy,
            f,
            e,
            edge_param,
            curve,
            interval,
            direction,
            src_zone_tol_3d,
            uv_space_okay,
            face_index,
        )
    }
    fn point_classify(
        &mut self,
        k: &Kernel,
        face: O,
        uv: Point2,
        src_zone_tol_3d: f64,
        do_xsects: bool,
        use_uv: bool,
        point_class: &mut PointClassification<O>,
    ) -> Result<()> {
        let geometry = self.0;
        let (b, f) = resolve_face(geometry, face).ok_or_else(unresolved_uv)?;
        let mut pc = point_class.map_objects(&mut |o| {
            geometry
                .topology_entity(o)
                .map(|(_, e)| e)
                .ok_or_else(unresolved_uv)
        })?;
        // read-only topology: a UV trim the walk would create is `Unported`
        let r = qf2::point_classify_prepared(
            k,
            b,
            f,
            uv,
            &mut pc,
            point_classify_options(src_zone_tol_3d, do_xsects, use_uv),
        );
        *point_class =
            pc.map_objects(&mut |e| geometry.topology_handle_in(b, e).ok_or_else(unresolved_uv))?;
        r
    }
    fn point_3d_classify(
        &mut self,
        k: &Kernel,
        face: O,
        point: Point3,
        src_zone_tol_3d: f64,
        do_xsects: bool,
        point_class: &mut PointClassification<O>,
    ) -> Result<()> {
        let geometry = self.0;
        let (b, f) = resolve_face(geometry, face).ok_or_else(unresolved_3d)?;
        let mut copy = b.clone_standing_for_self()?;
        point_3d_classify_in(
            k,
            geometry,
            &mut copy,
            b,
            f,
            point,
            src_zone_tol_3d,
            do_xsects,
            point_class,
        )
    }
}

/// A mutable adapter: the face queries run on the B-rep that
/// [`ClassGeometryMut::topology_entity_mut`] resolves the face handle to. A face handle
/// it cannot resolve falls back to [`ReadOnly`].
struct Mutable<'g, G>(&'g mut G);

impl<O: ClassObject, G: ClassGeometryMut<O>> FaceQueries<O> for Mutable<'_, G> {
    type Geometry = G;
    fn geometry(&self) -> &G {
        self.0
    }
    fn find_vertex_sector(
        &mut self,
        k: &Kernel,
        face: O,
        vertex: O,
        curve: &Curve,
        uv_curve: Option<&Curve>,
        from: f64,
        to: f64,
        direction: OrientType,
        src_zone_tol_3d: f64,
        uv_space_okay: bool,
    ) -> Result<EndInside> {
        let unresolved = || {
            // DISPATCH GAP: the adapter cannot resolve the face or vertex handle to its
            // `Brep` entity (`ClassGeometry::topology_entity`).
            SolidError::unported("SmFace::FindVertexSector", "topology.face_p4")
        };
        let v = {
            let geometry = &*self.0;
            let (b, _) = resolve_face(geometry, face).ok_or_else(unresolved)?;
            same_brep_vertex(geometry, b, vertex).ok_or_else(unresolved)??
        };
        let Some(v) = v else {
            return Ok(EndInside::default());
        };
        let Some((b, f)) = resolve_face_mut(self.0, face) else {
            return ReadOnly(&*self.0).find_vertex_sector(
                k,
                face,
                vertex,
                curve,
                uv_curve,
                from,
                to,
                direction,
                src_zone_tol_3d,
                uv_space_okay,
            );
        };
        let r = find_vertex_sector_in(
            k,
            b,
            f,
            v,
            curve,
            uv_curve,
            from,
            to,
            direction,
            src_zone_tol_3d,
            uv_space_okay,
        )?;
        Ok(r)
    }
    fn edge_curve_on_classify(
        &mut self,
        k: &Kernel,
        face: O,
        edge: O,
        edge_param: f64,
        curve: &Curve,
        interval: &Extent1d,
        direction: OrientType,
        src_zone_tol_3d: f64,
        uv_space_okay: bool,
        face_index: Option<u64>,
    ) -> Result<EndInside> {
        let unresolved = || {
            // DISPATCH GAP: the adapter cannot resolve the face or edge handle to its
            // `Brep` entity (`ClassGeometry::topology_entity`).
            SolidError::unported("SmFace::EdgeCurveOnClassify", "query.face_p1")
        };
        let e = {
            let geometry = &*self.0;
            let (b, _) = resolve_face(geometry, face).ok_or_else(unresolved)?;
            same_brep_edge(geometry, b, edge).ok_or_else(unresolved)??
        };
        let Some(e) = e else {
            return Ok(EndInside::default());
        };
        let Some((b, f)) = resolve_face_mut(self.0, face) else {
            return ReadOnly(&*self.0).edge_curve_on_classify(
                k,
                face,
                edge,
                edge_param,
                curve,
                interval,
                direction,
                src_zone_tol_3d,
                uv_space_okay,
                face_index,
            );
        };
        let r = edge_curve_on_classify_in(
            k,
            b,
            f,
            e,
            edge_param,
            curve,
            interval,
            direction,
            src_zone_tol_3d,
            uv_space_okay,
            face_index,
        )?;
        Ok(r)
    }
    fn point_classify(
        &mut self,
        k: &Kernel,
        face: O,
        uv: Point2,
        src_zone_tol_3d: f64,
        do_xsects: bool,
        use_uv: bool,
        point_class: &mut PointClassification<O>,
    ) -> Result<()> {
        resolve_face(&*self.0, face).ok_or_else(unresolved_uv)?;
        let mut pc = point_class.map_objects(&mut |o| {
            self.0
                .topology_entity(o)
                .map(|(_, e)| e)
                .ok_or_else(unresolved_uv)
        })?;
        let Some((b, f)) = resolve_face_mut(self.0, face) else {
            return ReadOnly(&*self.0).point_classify(
                k,
                face,
                uv,
                src_zone_tol_3d,
                do_xsects,
                use_uv,
                point_class,
            );
        };
        let r = qf2::point_classify(
            k,
            b,
            f,
            uv,
            &mut pc,
            point_classify_options(src_zone_tol_3d, do_xsects, use_uv),
        );
        let geometry = &*self.0;
        let (b, _) = resolve_face(geometry, face).ok_or_else(unresolved_uv)?;
        *point_class =
            pc.map_objects(&mut |e| geometry.topology_handle_in(b, e).ok_or_else(unresolved_uv))?;
        r
    }
    fn point_3d_classify(
        &mut self,
        k: &Kernel,
        face: O,
        point: Point3,
        src_zone_tol_3d: f64,
        do_xsects: bool,
        point_class: &mut PointClassification<O>,
    ) -> Result<()> {
        resolve_face(&*self.0, face).ok_or_else(unresolved_3d)?;
        let mut pc = point_class.map_objects(&mut |o| {
            self.0
                .topology_entity(o)
                .map(|(_, e)| e)
                .ok_or_else(unresolved_3d)
        })?;
        let Some((b, f)) = resolve_face_mut(self.0, face) else {
            return ReadOnly(&*self.0).point_3d_classify(
                k,
                face,
                point,
                src_zone_tol_3d,
                do_xsects,
                point_class,
            );
        };
        let r = qf1::point_3d_classify(
            k,
            b,
            f,
            point,
            src_zone_tol_3d,
            do_xsects,
            &mut pc,
            qf1::Point3dClassifyOptions::default(),
        );
        let geometry = &*self.0;
        let (b, _) = resolve_face(geometry, face).ok_or_else(unresolved_3d)?;
        *point_class =
            pc.map_objects(&mut |e| geometry.topology_handle_in(b, e).ok_or_else(unresolved_3d))?;
        r
    }
}

impl<O: ClassObject, G: ClassGeometryMut<O>> FilletAccess<O> for Mutable<'_, G> {
    type Geometry = G;
    fn geometry(&self) -> &G {
        self.0
    }
    fn set_vertex_point(&mut self, vertex: O, point: Point3) -> Result<()> {
        match self.0.topology_entity_mut(vertex) {
            Some((b, EntityRef::Vertex(v))) => {
                b.vertex_mut(v)?.point = point;
                Ok(())
            }
            _ => ReadOnly(&*self.0).set_vertex_point(vertex, point),
        }
    }
    fn set_edge_curve(&mut self, edge: O, curve: Curve) -> Result<()> {
        self.0.set_filleting_edge_curve(edge, curve)
    }
}

/// The B-rep and face behind face handle `face` ([`ClassGeometry::topology_entity`]);
/// `None` for an adapter that cannot resolve it.
fn resolve_face<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    face: O,
) -> Option<(&Brep, FaceId)> {
    match geometry.topology_entity(face)? {
        (b, EntityRef::Face(f)) => Some((b, f)),
        _ => None,
    }
}

/// [`resolve_face`], mutably for the classification queries
/// ([`ClassGeometryMut::topology_entity_for_query`]).
fn resolve_face_mut<O: ClassObject>(
    geometry: &mut impl ClassGeometryMut<O>,
    face: O,
) -> Option<(&mut Brep, FaceId)> {
    match geometry.topology_entity_for_query(face)? {
        (b, EntityRef::Face(f)) => Some((b, f)),
        _ => None,
    }
}

/// The edge behind edge handle `edge` when it is one of B-rep `b`'s: `Some(None)` for
/// an edge of another B-rep (no edge use on a face of `b`), `None` for a handle the
/// adapter cannot resolve.
fn same_brep_edge<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    b: &Brep,
    edge: O,
) -> Option<Result<Option<EdgeId>>> {
    let (edge_brep, entity) = geometry.topology_entity(edge)?;
    let EntityRef::Edge(e) = entity else {
        return Some(Err(SolidError::invalid_input(
            OP_FACE,
            "edge class object not an edge",
        )));
    };
    Some(Ok(core::ptr::eq(b, edge_brep).then_some(e)))
}

/// The vertex behind vertex handle `vertex` when it is one of B-rep `b`'s: `Some(None)`
/// for a vertex of another B-rep (no vertex use on a face of `b`), `None` for a handle the
/// adapter cannot resolve.
fn same_brep_vertex<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    b: &Brep,
    vertex: O,
) -> Option<Result<Option<VertexId>>> {
    let (vertex_brep, entity) = geometry.topology_entity(vertex)?;
    let EntityRef::Vertex(v) = entity else {
        return Some(Err(SolidError::invalid_input(
            OP_FACE,
            "vertex class object not a vertex",
        )));
    };
    Some(Ok(core::ptr::eq(b, vertex_brep).then_some(v)))
}

/// [`crate::topology::face::p4::find_vertex_sector`] of face `f` of `b`.
#[allow(clippy::too_many_arguments)]
fn find_vertex_sector_in(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    v: VertexId,
    curve: &Curve,
    uv_curve: Option<&Curve>,
    from: f64,
    to: f64,
    direction: OrientType,
    src_zone_tol_3d: f64,
    uv_space_okay: bool,
) -> Result<EndInside> {
    let sector = crate::topology::face::p4::find_vertex_sector(
        k,
        b,
        f,
        crate::topology::face::p4::FindVertexSectorArgs {
            vertex: v,
            curve,
            uv_curve,
            param_at_vertex: from,
            param_ivl_end: to,
            orientation: direction,
            zone_tol_3d: src_zone_tol_3d,
            validate_sector: true,
            uv_space_okay,
        },
    )?;
    Ok(EndInside {
        inside: sector.vertexuse.is_some(),
        possible_problem: sector.possible_problem,
    })
}

/// [`qf1::edge_curve_on_classify`] of face `f` of `b` (an edge of another B-rep has no
/// edge use on this face: not inside, no problem, as upstream's edge use walk finds).
#[allow(clippy::too_many_arguments)]
fn edge_curve_on_classify_in(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    e: EdgeId,
    edge_param: f64,
    curve: &Curve,
    interval: &Extent1d,
    direction: OrientType,
    src_zone_tol_3d: f64,
    uv_space_okay: bool,
    face_index: Option<u64>,
) -> Result<EndInside> {
    let class = qf1::edge_curve_on_classify(
        k,
        b,
        f,
        Some(e),
        curve,
        &qf1::EdgeCurveOnOptions {
            edge_param,
            curve_interval: *interval,
            curve_orientation: direction,
            curve_tolerance: src_zone_tol_3d,
            uv_space_okay,
            face_index,
        },
    )?;
    Ok(EndInside {
        inside: class.inside,
        possible_problem: class.possible_problem,
    })
}

/// The error of a `Point3DClassify` whose face or classified entity the adapter
/// cannot resolve.
fn unresolved_3d() -> SolidError {
    // DISPATCH GAP: the adapter cannot resolve the face handle to its `Brep` face
    // (`ClassGeometry::topology_entity`) or a classified entity back to a handle.
    SolidError::unported("SmFace::Point3DClassify", "query.face_p1")
}

/// The error of a `PointClassify` whose face or classified entity the adapter cannot
/// resolve.
fn unresolved_uv() -> SolidError {
    // DISPATCH GAP: the adapter cannot resolve the face handle to its `Brep` face
    // (`ClassGeometry::topology_entity`) or a classified entity back to a handle.
    SolidError::unported("SmFace::PointClassify", "query.face_p2")
}

/// `PointClassify(uv, sSrcZoneTol3d, bDoXSects, bUseUV, pc)`'s arguments (the others
/// at their defaults).
fn point_classify_options(
    src_zone_tol_3d: f64,
    do_xsects: bool,
    use_uv: bool,
) -> qf2::PointClassifyOptions<'static> {
    qf2::PointClassifyOptions {
        src_zone_tol_3d,
        do_boundary_intersections: do_xsects,
        use_uv_classification: use_uv,
        ..qf2::PointClassifyOptions::default()
    }
}

/// [`qf1::point_3d_classify`] of face `f` on `work` (the face's B-rep `b`, or a copy
/// of it), its classification handed back in the caller's handles
/// ([`ClassGeometry::topology_handle_in`] of `b`). `point_class` holds what the
/// classification wrote also when it fails, as upstream's.
#[allow(clippy::too_many_arguments)]
fn point_3d_classify_in<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    work: &mut Brep,
    b: &Brep,
    f: FaceId,
    point: Point3,
    src_zone_tol_3d: f64,
    do_xsects: bool,
    point_class: &mut PointClassification<O>,
) -> Result<()> {
    let mut pc = point_class.map_objects(&mut |o| {
        geometry
            .topology_entity(o)
            .map(|(_, e)| e)
            .ok_or_else(unresolved_3d)
    })?;
    let r = qf1::point_3d_classify(
        k,
        work,
        f,
        point,
        src_zone_tol_3d,
        do_xsects,
        &mut pc,
        qf1::Point3dClassifyOptions::default(),
    );
    *point_class =
        pc.map_objects(&mut |e| geometry.topology_handle_in(b, e).ok_or_else(unresolved_3d))?;
    r
}

/// Native surface identity is stricter than coincidence. A kept procedural
/// definition cannot be certified from its net, even when those nets are equal.
fn same_native_surface(a: &Surface, b: &Surface) -> bool {
    if a.procedural_definition().is_some() || b.procedural_definition().is_some() {
        return false;
    }
    match (a.kind(), b.kind()) {
        (SurfaceKind::BSpline(a), SurfaceKind::BSpline(b)) => {
            a.gw_nurb().is_some() && a.gw_nurb() == b.gw_nurb()
        }
        (SurfaceKind::Plane(a), SurfaceKind::Plane(b)) => {
            a.position().canonical() == b.position().canonical() && a.uv_scale() == b.uv_scale()
        }
        _ => false,
    }
}

/// Exact algebraic identity of the constant coordinate, including rational
/// controls: P_i.axis = fixed * w_i. Positive basis weights then give that same
/// coordinate for every parameter. No midpoint or fitted curve enters the proof.
fn same_iso_support(source: &Curve, trim: &Curve) -> bool {
    let CurveKind::Line(line) = source.kind() else {
        return false;
    };
    let (axis, fixed) = if line.line_vector().x == 0.0 && line.line_vector().y != 0.0 {
        (0, line.line_point().x)
    } else if line.line_vector().y == 0.0 && line.line_vector().x != 0.0 {
        (1, line.line_point().y)
    } else {
        return false;
    };
    let coordinate = |p: Vec3| if axis == 0 { p.x } else { p.y };
    let varying = |p: Vec3| if axis == 0 { p.y } else { p.x };
    let Some((lo, hi)) = line_coordinate_ends(line, varying) else {
        return false;
    };
    // Also prove the entire trim lies within the source UV line's finite image.
    // Multiplying by each positive weight avoids a rounded rational division.
    let covered = |value: f64, weight: f64| {
        let Some(low) = expansion::scale(&lo, weight) else {
            return false;
        };
        let Some(high) = expansion::scale(&hi, weight) else {
            return false;
        };
        expansion::sign(&expansion::sub(&[value], &low)) >= 0
            && expansion::sign(&expansion::sub(&high, &[value])) >= 0
    };
    match trim.kind() {
        CurveKind::Line(other) => {
            let Some((a, b)) = line_coordinate_ends(other, varying) else {
                return false;
            };
            coordinate(other.line_vector()) == 0.0
                && coordinate(other.line_point()) == fixed
                && expansion::sign(&expansion::sub(&a, &lo)) >= 0
                && expansion::sign(&expansion::sub(&hi, &b)) >= 0
        }
        CurveKind::BSpline(other) => {
            let Some(net) = convex_native_net(other) else {
                return false;
            };
            !net.cpts().is_empty()
                && net.cpts().iter().all(|p| {
                    let w = if p.w == crate::nurbs::math::NOW {
                        1.0
                    } else {
                        p.w
                    };
                    let value = if axis == 0 { p.x } else { p.y };
                    w > 0.0
                        && w.is_finite()
                        && covered(if axis == 0 { p.y } else { p.x }, w)
                        && exact_four_products_zero([
                            (value, 1.0, false),
                            (fixed, w, true),
                            (0.0, 0.0, false),
                            (0.0, 0.0, false),
                        ])
                })
        }
        _ => false,
    }
}

/// Nonnegative partition-of-unity basis over the whole public native domain:
/// finite ordered knots, clamped ends and no polynomial extrapolation. Positive
/// rational weights are checked by the caller before using the convex hull.
fn convex_native_net(spline: &BSplineCurve) -> Option<&crate::nurbs::crv_basic::Curve> {
    let net = spline.gw_nurb()?;
    let p = net.degree();
    let n = net.cpts().len();
    if spline.is_out_of_bounds_enabled()
        || net.counts_are_valid().is_err()
        || n <= p
        || n.checked_add(p).and_then(|v| v.checked_add(1)) != Some(net.knots().len())
        || net.knots().iter().any(|v| !v.is_finite())
        || net.knots().windows(2).any(|v| v.first() > v.last())
        || net.knots().first() != net.knots().get(p)
        || net.knots().last() != net.knots().get(n)
        || net.knots().first() >= net.knots().last()
    {
        return None;
    }
    Some(net)
}

/// Exact endpoints of one coordinate of a bounded native line, ordered by
/// value. Its native parameter map traverses its complete stored STEP interval;
/// reversing the map does not change this image. Expansions retain all product
/// and sum rounding, including the three factors in the line displacement.
fn line_coordinate_ends(
    line: &crate::curve::line::Line,
    coordinate: impl Fn(Vec3) -> f64,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if !line.is_bounded()
        || line.natural_interval().length() <= 0.0
        || crate::geom::tol::is_zero(line.natural_interval().length())
    {
        return None;
    }
    let vector = expansion::product(coordinate(line.line_vector()), line.line_scale())?;
    let endpoint = |step| {
        let offset = expansion::scale(&vector, step)?;
        Some(expansion::add(&[coordinate(line.line_point())], &offset))
    };
    let a = endpoint(line.step_interval().min())?;
    let b = endpoint(line.step_interval().max())?;
    if expansion::sign(&expansion::sub(&b, &a)) < 0 {
        Some((b, a))
    } else {
        Some((a, b))
    }
}

/// Exact native curve data, retaining homogeneous controls without the rounded
/// Euclidean divide/remultiply performed by a convenience control-polygon API.
enum SeamCurveDefinition {
    Line(crate::curve::native_affine::LineDefinition),
    Nurbs(NurbsCurve),
}

impl SeamCurveDefinition {
    fn of(curve: &Curve, domain: Extent1d) -> Option<Self> {
        let natural = curve.natural_interval();
        if !(domain.min().is_finite()
            && domain.max().is_finite()
            && domain.min() < domain.max()
            && domain.min() >= natural.min()
            && domain.max() <= natural.max())
        {
            return None;
        }
        match curve.kind() {
            CurveKind::Line(_) => of_line(curve, domain).map(Self::Line),
            CurveKind::BSpline(bs) => {
                let net = convex_native_net(bs)?;
                if net.degree() == 0 || net.degree() > 18 {
                    return None;
                }
                let mut pw = Vec::new();
                for cp in net.cpts() {
                    let w = if cp.w == crate::nurbs::math::NOW {
                        1.0
                    } else {
                        cp.w
                    };
                    let z = if cp.z == crate::nurbs::math::NOZ {
                        0.0
                    } else {
                        cp.z
                    };
                    if !(w > 0.0
                        && w.is_finite()
                        && cp.x.is_finite()
                        && cp.y.is_finite()
                        && z.is_finite())
                    {
                        return None;
                    }
                    pw.extend([cp.x, cp.y, z, w]);
                }
                Some(Self::Nurbs(NurbsCurve {
                    p: net.degree(),
                    n: net.cpts().len(),
                    u: net.knots().to_vec(),
                    pw,
                }))
            }
            _ => None,
        }
    }

    fn cuts(&self) -> &[f64] {
        match self {
            Self::Line(_) => &[],
            Self::Nurbs(n) => &n.u,
        }
    }

    fn jets(&self, t: Ivl, station: f64) -> Result<[[Ivl; 3]; 4]> {
        let zero = [Ivl::from(0.0); 3];
        match self {
            Self::Line(line) => {
                let (p, d) = line.jet_over(t);
                Ok([p, d, zero, zero])
            }
            Self::Nurbs(n) => {
                // The caller splits at every curve knot. Use this cell's one
                // polynomial piece at both endpoints, including its left limit.
                let piece = span(&n.u, n.p, n.n, station);
                let d = curve_derivs_over(n, t, 3, Some(piece))?;
                let mut out = [zero; 4];
                for (to, from) in out.iter_mut().zip(d) {
                    *to = [from.x, from.y, from.z];
                }
                Ok(out)
            }
        }
    }
}

fn iv_add(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + bx, ay + by, az + bz]
}

fn iv_sub(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax - bx, ay - by, az - bz]
}

fn iv_scale(a: [Ivl; 3], b: Ivl) -> [Ivl; 3] {
    a.map(|v| v * b)
}

/// A whole-cell enclosure of g, g'' and the across-seam direction's length,
/// where D = Q(t) - S(uv(t)), T = dS(uv(t))/dt, A = (Su x Sv) x T,
/// and g = D.A. Surface partials through order three and trim derivatives
/// through order three enclose every term of g'' by the product/chain rules.
struct SeamGapCertificate<'a> {
    edge: &'a SeamCurveDefinition,
    trim: &'a SeamCurveDefinition,
    surface: &'a Nurbs,
    fixed: (usize, f64),
}

impl SeamGapCertificate<'_> {
    fn over(&self, t: Ivl, station: f64) -> Result<(Ivl, Ivl, Ivl)> {
        let q = self.edge.jets(t, station)?;
        let mut uv = self.trim.jets(t, station)?;
        // Exact UV identity has proved these coordinates, so retaining a
        // dependency-widened interval here would lose the boundary identity.
        for (order, value) in uv.iter_mut().enumerate() {
            if let Some(v) = value.get_mut(self.fixed.0) {
                *v = Ivl::from(if order == 0 { self.fixed.1 } else { 0.0 });
            }
        }
        let [[u, v, _], [du, dv, _], [ddu, ddv, _], [dddu, dddv, _]] = uv;
        let n = self.surface;
        let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
            (n.u.get(n.p), n.u.get(n.nu), n.v.get(n.q), n.v.get(n.nv))
        else {
            return Err(SolidError::invalid_input(
                OP_GEOM,
                "missing native surface domain",
            ));
        };
        // Before this call, the complete native trim was proved inside this
        // domain by exact homogeneous comparisons. Clamping only removes
        // interval arithmetic's outward overshoot, never actual trim points.
        let s = nurbs_partials_over_box(
            n,
            u.lo.max(u0),
            u.hi.min(u1),
            v.lo.max(v0),
            v.hi.min(v1),
            3,
            2,
        )?;
        let partial = |i, j| {
            let p = s.at(i, j);
            [p.x, p.y, p.z]
        };
        let su = partial(1, 0);
        let sv = partial(0, 1);
        let along = |a, b| iv_add(iv_scale(a, du), iv_scale(b, dv));
        let su1 = along(partial(2, 0), partial(1, 1));
        let sv1 = along(partial(1, 1), partial(0, 2));
        let second = |a, b, c, d, e| {
            iv_add(
                iv_add(iv_scale(a, du * du), iv_scale(b, Ivl::from(2.0) * du * dv)),
                iv_add(
                    iv_scale(c, dv * dv),
                    iv_add(iv_scale(d, ddu), iv_scale(e, ddv)),
                ),
            )
        };
        let su2 = second(
            partial(3, 0),
            partial(2, 1),
            partial(1, 2),
            partial(2, 0),
            partial(1, 1),
        );
        let sv2 = second(
            partial(2, 1),
            partial(1, 2),
            partial(0, 3),
            partial(1, 1),
            partial(0, 2),
        );
        let p1 = along(su, sv);
        let p2 = iv_add(
            along(su1, sv1),
            iv_add(iv_scale(su, ddu), iv_scale(sv, ddv)),
        );
        let p3 = iv_add(
            along(su2, sv2),
            iv_add(
                iv_scale(
                    iv_add(iv_scale(su1, ddu), iv_scale(sv1, ddv)),
                    Ivl::from(2.0),
                ),
                iv_add(iv_scale(su, dddu), iv_scale(sv, dddv)),
            ),
        );
        let normal = interval_cross(su, sv);
        let normal1 = iv_add(interval_cross(su1, sv), interval_cross(su, sv1));
        let normal2 = iv_add(
            iv_add(interval_cross(su2, sv), interval_cross(su, sv2)),
            iv_scale(interval_cross(su1, sv1), Ivl::from(2.0)),
        );
        let a = interval_cross(normal, p1);
        let a1 = iv_add(interval_cross(normal1, p1), interval_cross(normal, p2));
        let a2 = iv_add(
            iv_add(interval_cross(normal2, p1), interval_cross(normal, p3)),
            iv_scale(interval_cross(normal1, p2), Ivl::from(2.0)),
        );
        let [q0, q1, q2, _] = q;
        let d = iv_sub(q0, partial(0, 0));
        let d1 = iv_sub(q1, p1);
        let d2 = iv_sub(q2, p2);
        let g2 = interval_dot(d2, a) + Ivl::from(2.0) * interval_dot(d1, a1) + interval_dot(d, a2);
        Ok((interval_dot(d, a), g2, interval_dot(a, a).sqrt()))
    }
}

/// Prove the native trim's entire image stays in the surface domain. Convexity
/// reduces this to homogeneous control comparisons, evaluated without rounding
/// the rational division. Lines use their exact finite-image endpoints.
fn trim_inside_surface(trim: &Curve, n: &Nurbs) -> bool {
    let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
        (n.u.get(n.p), n.u.get(n.nu), n.v.get(n.q), n.v.get(n.nv))
    else {
        return false;
    };
    let covered = |value: f64, weight: f64, lo: f64, hi: f64| {
        let (Some(low), Some(high)) = (
            expansion::product(lo, weight),
            expansion::product(hi, weight),
        ) else {
            return false;
        };
        expansion::sign(&expansion::sub(&[value], &low)) >= 0
            && expansion::sign(&expansion::sub(&high, &[value])) >= 0
    };
    match trim.kind() {
        CurveKind::Line(line) => {
            for (coordinate, lo, hi) in [(0, u0, u1), (1, v0, v1)] {
                let Some((a, b)) =
                    line_coordinate_ends(line, |p| if coordinate == 0 { p.x } else { p.y })
                else {
                    return false;
                };
                if expansion::sign(&expansion::sub(&a, &[lo])) < 0
                    || expansion::sign(&expansion::sub(&[hi], &b)) < 0
                {
                    return false;
                }
            }
            true
        }
        CurveKind::BSpline(bs) => convex_native_net(bs).is_some_and(|net| {
            net.cpts().iter().all(|p| {
                let w = if p.w == crate::nurbs::math::NOW {
                    1.0
                } else {
                    p.w
                };
                w > 0.0 && w.is_finite() && covered(p.x, w, u0, u1) && covered(p.y, w, v0, v1)
            })
        }),
        _ => false,
    }
}

/// A certificate for the ACTUAL stored 3D edge, not merely its isoparametric
/// pcurve. On each smooth native knot cell the scalar interpolation remainder
/// gives |g(t)| <= max(|g(lo)|,|g(hi)|) + (hi-lo)^2/8 sup|g''|. Dividing this
/// outward bound by inf|A| bounds across-seam displacement throughout that cell.
/// UV identity is only a prerequisite locating the comparison on this seam.
/// Unknown definitions, crossed surface knots below C2, poles and exhausted
/// subdivision budgets refuse relief; samples alone never grant it.
fn native_edge_on_iso(
    k: &Kernel,
    surface: &Surface,
    source_uv: &Curve,
    edge: &Curve,
    trim: &Curve,
    domain: Extent1d,
    threshold: f64,
) -> Result<bool> {
    if !same_iso_support(source_uv, trim) || surface.procedural_definition().is_some() {
        return Ok(false);
    }
    let (SurfaceKind::BSpline(surface), CurveKind::Line(source)) =
        (surface.kind(), source_uv.kind())
    else {
        return Ok(false);
    };
    if surface.is_out_of_bounds_enabled() {
        return Ok(false);
    }
    let Some(n) = ignored(Nurbs::of(surface))? else {
        return Ok(false);
    };
    if n.p > 18
        || n.q > 18
        || n.u.iter().chain(&n.v).chain(&n.pw).any(|x| !x.is_finite())
        || n.u.windows(2).any(|w| w.first() > w.last())
        || n.v.windows(2).any(|w| w.first() > w.last())
        || !trim_inside_surface(trim, &n)
    {
        return Ok(false);
    }
    let (Some(edge_def), Some(trim_def)) = (
        SeamCurveDefinition::of(edge, domain),
        SeamCurveDefinition::of(trim, domain),
    ) else {
        return Ok(false);
    };
    let fixed = if source.line_vector().x == 0.0 {
        (0, source.line_point().x)
    } else {
        (1, source.line_point().y)
    };
    let certificate = SeamGapCertificate {
        edge: &edge_def,
        trim: &trim_def,
        surface: &n,
        fixed,
    };
    let mut cuts = vec![domain.min(), domain.max()];
    cuts.extend(
        edge_def
            .cuts()
            .iter()
            .chain(trim_def.cuts())
            .copied()
            .filter(|t| *t > domain.min() && *t < domain.max()),
    );
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    if cuts.len() > 4096 {
        return Ok(false);
    }
    let mut work = Vec::new();
    for ends in cuts.windows(2) {
        if let [lo, hi] = ends {
            work.push((*lo, *hi, 0_u32));
        }
    }
    let mut visits = 0;
    while let Some((lo, hi, depth)) = work.pop() {
        k.check_cancelled(OP_GEOM)?;
        visits += 1;
        if visits > 4096 {
            return Ok(false);
        }
        let mid = lo + (hi - lo) * 0.5;
        if !(mid > lo && mid < hi) {
            return Ok(false);
        }
        let Some((_, g2, length)) = ignored(certificate.over(Ivl::new(lo, hi), mid))? else {
            return Ok(false);
        };
        let Some((glo, _, alo)) = ignored(certificate.over(Ivl::from(lo), mid))? else {
            return Ok(false);
        };
        let Some((ghi, _, ahi)) = ignored(certificate.over(Ivl::from(hi), mid))? else {
            return Ok(false);
        };
        // A certified bad endpoint already disproves whole-span coincidence.
        // Endpoint observations can refuse relief, but never grant it.
        if glo.mig() >= (Ivl::from(threshold) * alo).hi
            || ghi.mig() >= (Ivl::from(threshold) * ahi).hi
        {
            return Ok(false);
        }
        let h = Ivl::from(hi) - Ivl::from(lo);
        let bound = (Ivl::from(glo.mag().max(ghi.mag()))
            + h * h * Ivl::from(g2.mag()) / Ivl::from(8.0))
            / length;
        if length.lo > 0.0 && bound.hi < threshold {
            continue;
        }
        if depth >= 20 {
            return Ok(false);
        }
        work.push((mid, hi, depth + 1));
        work.push((lo, mid, depth + 1));
    }
    Ok(true)
}

fn interval_point(p: Vec3) -> [Ivl; 3] {
    [p.x, p.y, p.z].map(Ivl::from)
}

fn interval_cross([ax, ay, az]: [Ivl; 3], [bx, by, bz]: [Ivl; 3]) -> [Ivl; 3] {
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn interval_dot(a: [Ivl; 3], b: [Ivl; 3]) -> Ivl {
    a.into_iter()
        .zip(b)
        .fold(Ivl::from(0.0), |sum, (a, b)| sum + a * b)
}

/// The origin and across-seam direction of a cone's or cylinder's seam over `domain`,
/// as enclosures for [`native_curve_in_strip`]. The seam of such a surface is a
/// ruling, a straight line along which the surface normal is constant, so the
/// across-seam direction `n x t` is one vector for the whole seam. That is checked,
/// not assumed: three stations must be collinear and share one normal that is
/// perpendicular to the line, to far below any tolerance, or there is no frame and
/// so no relief. The values are floating point, so each is widened by a relative
/// margin of `1e-12`, orders above the rounding of these few operations.
/// `None` for a seam that is no ruling or whose normal cannot be evaluated.
fn ruling_frame(
    k: &Kernel,
    seam: &Curve,
    uv_seam: &Curve,
    surface: &Surface,
    domain: Extent1d,
) -> Result<Option<([Ivl; 3], [Ivl; 3])>> {
    const MARGIN: f64 = 1e-12;
    let stations = [domain.min(), domain.mid(), domain.max()];
    let mut points = Vec::with_capacity(3);
    let mut normals = Vec::with_capacity(3);
    for t in stations {
        let Some(point) = ignored(seam.evaluate_point(k, t))? else {
            return Ok(None);
        };
        let Some(uv) = ignored(uv_seam.evaluate_point(k, t))? else {
            return Ok(None);
        };
        let Some(normal) =
            ignored(surface.evaluate_normal(k, Point2::new(uv.x, uv.y), Side::Left, Side::Left))?
        else {
            return Ok(None);
        };
        points.push(point);
        normals.push(normal);
    }
    let (Some(&p0), Some(&p1), Some(&p2), Some(&n0), Some(&n1), Some(&n2)) = (
        points.first(),
        points.get(1),
        points.get(2),
        normals.first(),
        normals.get(1),
        normals.get(2),
    ) else {
        return Ok(None);
    };
    let line = p2 - p0;
    let length = line.length();
    if !(length > 0.0 && length.is_finite()) {
        return Ok(None);
    }
    let direction = line / length;
    let scale = 1.0 + length;
    let straight = (p1 - p0).cross(direction).length() <= MARGIN * scale;
    let same_normal = (n1 - n0).length() <= MARGIN && (n2 - n0).length() <= MARGIN;
    let normal_length = n0.length();
    if !(straight
        && same_normal
        && normal_length > 0.0
        && (n0 / normal_length).dot(direction).abs() <= MARGIN)
    {
        return Ok(None);
    }
    let across = (n0 / normal_length).cross(direction);
    let widen = |v: f64| Ivl::new(v - MARGIN * (1.0 + v.abs()), v + MARGIN * (1.0 + v.abs()));
    Ok(Some((
        [p0.x, p0.y, p0.z].map(widen),
        [across.x, across.y, across.z].map(widen),
    )))
}

/// Convex-hull strip bound. For Q(t) = sum alpha_i(t) Q_i, alpha_i >= 0,
/// sum alpha_i = 1, |(Q(t)-O).A|/|A| <= max_i |(Q_i-O).A|/|A|.
/// Every arithmetic operation is rounded outward, including rational division
/// and the direction's length. Only native polynomial/rational definitions qualify.
fn native_curve_in_strip(
    curve: &Curve,
    origin: [Ivl; 3],
    across: [Ivl; 3],
    threshold: f64,
) -> Result<bool> {
    let length = interval_dot(across, across).sqrt();
    if !(length.lo > 0.0 && length.hi.is_finite()) {
        return Ok(false);
    }
    let in_strip = |point: [Ivl; 3]| {
        let [px, py, pz] = point;
        let [ox, oy, oz] = origin;
        let offset = [px - ox, py - oy, pz - oz];
        (interval_dot(offset, across) / length).mag() < threshold
    };
    match curve.kind() {
        CurveKind::Line(_) => {
            let Some(line) = of_line(curve, curve.natural_interval()) else {
                return Ok(false);
            };
            Ok(
                in_strip(line.jet_over(Ivl::from(curve.natural_interval().min())).0)
                    && in_strip(line.jet_over(Ivl::from(curve.natural_interval().max())).0),
            )
        }
        CurveKind::BSpline(spline) => {
            let Some(net) = convex_native_net(spline) else {
                return Ok(false);
            };
            for cp in net.cpts() {
                let w = if cp.w == crate::nurbs::math::NOW {
                    1.0
                } else {
                    cp.w
                };
                if !(w > 0.0 && w.is_finite()) {
                    return Ok(false);
                }
                let z = if cp.z == crate::nurbs::math::NOZ {
                    0.0
                } else {
                    cp.z
                };
                if !in_strip([cp.x, cp.y, z].map(|v| Ivl::from(v) / Ivl::from(w))) {
                    return Ok(false);
                }
            }
            Ok(!net.cpts().is_empty())
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::intersect::point_class::ClassObjectKind as K;
    use crate::surface::volume::Volume;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(usize, K);

    impl ClassObject for H {
        fn class_kind(&self) -> K {
            self.1
        }
    }

    struct Geo {
        curves: Vec<Curve>,
    }

    impl ClassGeometry<H> for Geo {
        fn curve(&self, o: H) -> Option<&Curve> {
            self.curves.get(o.0)
        }
        fn surface(&self, _o: H) -> Option<&Surface> {
            None
        }
        fn volume(&self, _o: H) -> Option<&Volume> {
            None
        }
        fn point(&self, _o: H) -> Option<Point3> {
            None
        }
    }

    fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
        Curve::from(Line::bounded(
            Vec3::from_array(a),
            Vec3::from_array([b[0] - a[0], b[1] - a[1], b[2] - a[2]]),
            &Extent1d::from_min_max(0.0, 1.0),
            &BoundedLineOptions::default(),
        ))
    }

    fn classification(k: &Kernel, params: &[f64]) -> CurveClassification<H> {
        let mut cc = CurveClassification::new(k);
        cc.set_size(k, params.len() - 1).unwrap();
        for (i, w) in params.windows(2).enumerate() {
            cc.at_mut(i)
                .unwrap()
                .set_interval(&Extent1d::from_min_max(w[0], w[1]));
        }
        cc.set_curve(Some(H(0, K::Curve)), false);
        cc
    }

    #[test]
    fn an_isoparametric_trim_does_not_certify_its_stored_edges_hidden_bow() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as NurbSurface;

        let k = Kernel::default();
        let plane = Surface::from(BSplineSurface::from_nurb(
            NurbSurface::new(
                vec![
                    vec![
                        CPoint::new(0.0, 0.0, 0.0, NOW),
                        CPoint::new(0.0, 1.0, 0.0, NOW),
                    ],
                    vec![
                        CPoint::new(1.0, 0.0, 0.0, NOW),
                        CPoint::new(1.0, 1.0, 0.0, NOW),
                    ],
                ],
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            false,
        ));
        let trim = line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let domain = Extent1d::from_min_max(0.0, 1.0);
        let bowed = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(
                [
                    (0.0, 0.0),
                    (0.125, 2e-6),
                    (0.25, 0.0),
                    (0.5, 0.0),
                    (0.75, 0.0),
                    (1.0, 0.0),
                ]
                .into_iter()
                .map(|(t, x)| CPoint::new(x, t, 0.0, NOW))
                .collect(),
                1,
                vec![0.0, 0.0, 0.125, 0.25, 0.5, 0.75, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        ));
        assert!(same_iso_support(&trim, &trim));
        for t in [0.25, 0.5, 0.75] {
            assert_eq!(bowed.evaluate_point(&k, t).unwrap().x, 0.0);
        }
        // The pcurve is the seam itself, but the stored 3D curve departs across
        // the surface by twice the vertex zone between the old sample stations.
        assert!(!native_edge_on_iso(&k, &plane, &trim, &bowed, &trim, domain, 1e-9).unwrap());
        let normal_offset = line([0.0, 0.0, 1e-5], [0.0, 1.0, 1e-5]);
        assert!(
            native_edge_on_iso(&k, &plane, &trim, &normal_offset, &trim, domain, 1e-9).unwrap()
        );

        // A genuinely curved native isoparametric seam, S(0,v) = (0,v,v^2),
        // exercises the surface chain derivatives as well as the planar case.
        let curved = Surface::from(BSplineSurface::from_nurb(
            NurbSurface::new(
                [0.0, 1.0]
                    .into_iter()
                    .map(|u| {
                        vec![
                            CPoint::new(u, 0.0, 0.0, NOW),
                            CPoint::new(u, 0.5, 0.0, NOW),
                            CPoint::new(u, 1.0, 1.0, NOW),
                        ]
                    })
                    .collect(),
                1,
                2,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            false,
        ));
        let parabola = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(0.0, 0.5, 0.0, NOW),
                    CPoint::new(0.0, 1.0, 1.0, NOW),
                ],
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        ));
        assert!(native_edge_on_iso(&k, &curved, &trim, &parabola, &trim, domain, 1e-9).unwrap());
    }

    #[test]
    fn rational_isoparametric_identity_requires_the_whole_finite_image() {
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOZ};

        let source = line([2.0, 0.0, 0.0], [2.0, 1.0, 0.0]);
        let rational = |last_y: f64| {
            Curve::from(BSplineCurve::from_nurb(
                NurbCurve::new(
                    [(0.0, 0.5), (0.25, 2.0), (0.75, 1.0), (last_y, 4.0)]
                        .into_iter()
                        .map(|(y, w)| CPoint::new(2.0 * w, y * w, NOZ, w))
                        .collect(),
                    3,
                    vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                )
                .unwrap(),
                2,
                false,
            ))
        };
        // Unequal positive weights preserve x = 2 exactly in homogeneous form.
        assert!(same_iso_support(&source, &rational(1.0)));
        // The same infinite isoparametric support does not prove coincidence
        // with this finite source: the last endpoint extends beyond its image.
        assert!(!same_iso_support(&source, &rational(1.25)));
        assert!(!same_iso_support(
            &source,
            &line([2.0, 0.0, 0.0], [2.0, 1.25, 0.0]),
        ));
    }

    #[test]
    fn isoparametric_identity_refuses_a_trim_bowing_between_three_stations() {
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW, NOZ};

        let k = Kernel::default();
        let source = line([2.0, 0.0, 0.0], [2.0, 1.0, 0.0]);
        let trim = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(
                [
                    (0.0, 2.0),
                    (0.125, 2.000002),
                    (0.25, 2.0),
                    (0.5, 2.0),
                    (0.75, 2.0),
                    (1.0, 2.0),
                ]
                .into_iter()
                .map(|(y, x)| CPoint::new(x, y, NOZ, NOW))
                .collect(),
                1,
                vec![0.0, 0.0, 0.125, 0.25, 0.5, 0.75, 1.0, 1.0],
            )
            .unwrap(),
            2,
            false,
        ));
        for t in [0.25, 0.5, 0.75] {
            assert_eq!(trim.evaluate_point(&k, t).unwrap().x, 2.0);
        }
        assert!(trim.evaluate_point(&k, 0.125).unwrap().x - 2.0 > 1e-6);
        assert!(!same_iso_support(&source, &trim));
    }

    #[test]
    fn boundary_indices_and_mates() {
        let k = Kernel::default();
        let geo = Geo {
            curves: vec![line([0.0; 3], [1.0, 0.0, 0.0])],
        };
        let cc = classification(&k, &[0.0, 0.25, 1.0]);
        assert_eq!(cc.point_class_by_index(0).unwrap().1, 0.0);
        assert_eq!(cc.point_class_by_index(1).unwrap().1, 0.25);
        assert_eq!(cc.point_class_by_index(2).unwrap().1, 1.0);
        assert!(cc.point_class_by_index(3).is_err());
        assert!(cc.point_class_mate_by_index(&k, &geo, 0).unwrap().is_none());
        assert!(cc.point_class_mate_by_index(&k, &geo, 2).unwrap().is_none());
        assert!(cc.point_class_mate_by_index(&k, &geo, 1).unwrap().is_some());
        assert!(CurveClassification::<H>::new(&k)
            .point_class_by_index(0)
            .is_err());
    }

    #[test]
    fn crossing_face_and_touches() {
        let k = Kernel::default();
        let g = Geo { curves: Vec::new() };
        let mut cc = classification(&k, &[0.0, 0.5, 1.0]);
        let face = H(7, K::Face);
        cc.at_mut(1)
            .unwrap()
            .mid
            .set_class_object(PointClassType::Face, Some(face));
        assert!(cc.is_crossing_face(None));
        assert!(cc.is_crossing_face(Some(face)));
        assert!(!cc.is_crossing_face(Some(H(8, K::Face))));
        assert!(cc
            .point_classifies_connected_to(&g, &[])
            .unwrap()
            .is_empty());
        assert!(!cc.is_crossed_by_edge(&g).unwrap());
        assert!(cc
            .find_vertex_objects(&g, None)
            .unwrap()
            .vertices
            .is_empty());
        assert!(cc.find_edge_objects(&g, None, 3).unwrap().is_empty());
    }

    #[test]
    fn collapse_degenerate_interval() {
        let k = Kernel::default();
        let geo = Geo {
            curves: vec![line([0.0; 3], [1.0, 0.0, 0.0])],
        };
        let mut cc = classification(&k, &[0.0, 0.5, 0.5, 1.0]);
        assert!(cc.collapse_interval(&k, &geo, 0).is_err());
        cc.collapse_interval(&k, &geo, 1).unwrap();
        assert_eq!(cc.size(), 2);
        assert!(cc.collapse_interval(&k, &geo, 5).is_err());
    }

    #[test]
    fn partial_coincidence_needs_intervals() {
        let k = Kernel::default();
        let geo = Geo {
            curves: vec![line([0.0; 3], [1.0, 0.0, 0.0])],
        };
        let cc = classification(&k, &[0.0, 1.0]);
        let r = cc.test_for_partial_coincident_edges(&k, &geo, &cc).unwrap();
        assert_eq!(r.max_partial_coin_gap, 0.0);
        assert!(r.new_curve_to_test.is_none());
    }

    /// A seam-like classification: the circle of radius 10 about the origin in the plane
    /// z = 0, whose UV curve is the same circle (the plane's UV is its x and y), and the
    /// vertex at its end.
    struct SeamGeo {
        curves: Vec<Curve>,
        plane: Surface,
        vertex: Point3,
    }

    impl ClassGeometry<H> for SeamGeo {
        fn curve(&self, o: H) -> Option<&Curve> {
            self.curves.get(o.0)
        }
        fn surface(&self, o: H) -> Option<&Surface> {
            (o.0 == 0).then_some(&self.plane)
        }
        fn volume(&self, _o: H) -> Option<&Volume> {
            None
        }
        fn point(&self, _o: H) -> Option<Point3> {
            None
        }
        fn classification_vertex_point(&self, _vertex: H) -> Option<Point3> {
            Some(self.vertex)
        }
        fn topology_zone_tol_3d(&self, _object: H) -> Option<f64> {
            Some(1e-6)
        }
    }

    /// Whether the circle's first quarter, ending at a vertex 1e-8 from its end point
    /// along `offset` (in the end's frame: along the circle, along the plane's normal,
    /// across the circle in the plane), is a near miss; `uv1` and `s1` name the UV curve
    /// and the surface (index 1 and 0 resolve, others don't).
    fn quarter_ending_off(offset: [f64; 3], uv1: usize, s1: usize) -> bool {
        use crate::curve::ellipse::Ellipse;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::geom::extent2d::Extent2d;
        use crate::math::Vec2;

        let k = Kernel::default();
        let circle = Curve::from(
            Ellipse::create_canonical_circle(&k, &Axis2Placement::IDENTITY, 10.0, None, None)
                .unwrap(),
        );
        let ivl = circle.natural_interval();
        let t = ivl.min() + 0.25 * (ivl.max() - ivl.min());
        let d = circle.evaluate(&k, t, 1, true, false).unwrap();
        let along = d[1].unitize().unwrap();
        let across = Vec3::Z.cross(along);
        let gap = 1e-8;
        let vertex = d[0] + (along * offset[0] + Vec3::Z * offset[1] + across * offset[2]) * gap;
        let geo = SeamGeo {
            curves: vec![circle.clone(), circle],
            plane: Surface::from(crate::surface::plane::Plane::new(
                Vec3::ZERO,
                Vec3::X,
                Vec3::Y,
                Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(-20.0, -20.0, 20.0, 20.0),
            )),
            vertex,
        };
        let mut cc = classification(&k, &[ivl.min(), t]);
        cc.set_uv_curve1(Some(H(uv1, K::Curve)));
        cc.set_surface1(Some(H(s1, K::Surface)));
        let end = &mut cc.at_mut(0).unwrap().end;
        end.set_class_object(PointClassType::Vertex, Some(H(0, K::Vertex)));
        end.set_t_param(t, false);
        end.set_gap_3d(gap);
        cc.has_near_misses(&k, &geo, false).unwrap()
    }

    #[test]
    fn a_vertex_is_a_near_miss_of_a_curve_only_across_it_in_the_surface() {
        // across the circle within the plane: a sliver beside it
        assert!(quarter_ending_off([0.0, 0.0, 1.0], 1, 0));
        assert!(quarter_ending_off([1.0, 1.0, 1.0], 1, 0));
        // along the circle, off the plane, or both: no sliver
        assert!(!quarter_ending_off([1.0, 0.0, 0.0], 1, 0));
        assert!(!quarter_ending_off([0.0, 1.0, 0.0], 1, 0));
        assert!(!quarter_ending_off([1.0, 1.0, 0.0], 1, 0));
        // no UV curve or no surface to measure across in: the whole gap counts
        assert!(quarter_ending_off([1.0, 0.0, 0.0], 5, 0));
        assert!(quarter_ending_off([1.0, 0.0, 0.0], 1, 5));
    }

    #[test]
    fn agreement_at_three_stations_does_not_hide_a_spike_between_them() {
        use crate::geom::extent2d::Extent2d;
        use crate::math::Vec2;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};

        let k = Kernel::default();
        // A native piecewise-linear edge: every former station agrees exactly,
        // but at t = 1/8 its gap is twice the 1e-6 vertex zone.
        let edge = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(0.125, 2e-6, 0.0, NOW),
                    CPoint::new(0.25, 0.0, 0.0, NOW),
                    CPoint::new(0.5, 0.0, 0.0, NOW),
                    CPoint::new(0.75, 0.0, 0.0, NOW),
                    CPoint::new(1.0, 0.0, 0.0, NOW),
                ],
                1,
                vec![0.0, 0.0, 0.125, 0.25, 0.5, 0.75, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        ));
        for t in [0.25, 0.5, 0.75] {
            assert_eq!(edge.evaluate_point(&k, t).unwrap().y, 0.0);
        }
        assert!(edge.evaluate_point(&k, 0.125).unwrap().y > 1e-6);
        let geo = SeamGeo {
            curves: vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), edge],
            plane: Surface::from(crate::surface::plane::Plane::new(
                Vec3::ZERO,
                Vec3::X,
                Vec3::Y,
                Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(-1.0, -1.0, 2.0, 1.0),
            )),
            vertex: Point3::new(0.0, 1e-8, 0.0),
        };
        let mut cc = classification(&k, &[0.0, 1.0]);
        cc.set_surface1(Some(H(0, K::Surface)));
        cc.set_uv_curve1(Some(H(0, K::Curve)));
        let ivl = cc.at_mut(0).unwrap();
        ivl.mid
            .set_class_object(PointClassType::Edge, Some(H(1, K::Edge)));
        ivl.mid.set_gap_3d(0.0);
        ivl.start
            .set_class_object(PointClassType::Vertex, Some(H(0, K::Vertex)));
        ivl.start.set_gap_3d(1e-8);
        assert!(cc.has_near_misses(&k, &geo, false).unwrap());
    }

    #[test]
    fn a_midpoint_coincidence_does_not_hide_a_bow_beside_the_seam() {
        use crate::geom::extent2d::Extent2d;
        use crate::math::Vec2;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};

        let k = Kernel::default();
        for normal_offset in [false, true] {
            let control = |x, offset| {
                if normal_offset {
                    CPoint::new(x, 0.0, offset, NOW)
                } else {
                    CPoint::new(x, offset, 0.0, NOW)
                }
            };
            // The edge agrees at both ends and the midpoint, but has opposite
            // bows at the quarter stations, each inside the vertex's zone.
            let bowed = Curve::from(BSplineCurve::from_nurb(
                NurbCurve::new(
                    vec![
                        control(0.0, 0.0),
                        control(1.0 / 3.0, -1e-5 / 6.0),
                        control(2.0 / 3.0, 1e-5 / 6.0),
                        control(1.0, 0.0),
                    ],
                    3,
                    vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                )
                .unwrap(),
                3,
                false,
            ));
            let geo = SeamGeo {
                curves: vec![
                    line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                    bowed,
                    line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ],
                plane: Surface::from(crate::surface::plane::Plane::new(
                    Vec3::ZERO,
                    Vec3::X,
                    Vec3::Y,
                    Vec2::new(1.0, 1.0),
                    &Extent2d::from_bounds(-1.0, -1.0, 2.0, 1.0),
                )),
                vertex: Point3::new(0.0, 1e-8, 0.0),
            };
            for shared in [false, true] {
                let params: &[f64] = if shared {
                    &[-1.0, 0.0, 1.0]
                } else {
                    &[0.0, 1.0]
                };
                let mut cc = classification(&k, params);
                cc.set_uv_curve1(Some(H(2, K::Curve)));
                cc.set_surface1(Some(H(0, K::Surface)));
                let own = usize::from(shared);
                let ivl = cc.at_mut(own).unwrap();
                ivl.mid
                    .set_class_object(PointClassType::Edge, Some(H(1, K::Edge)));
                ivl.mid.set_gap_3d(0.0);
                ivl.start
                    .set_class_object(PointClassType::Vertex, Some(H(0, K::Vertex)));
                ivl.start.set_t_param(0.0, false);
                ivl.start.set_gap_3d(1e-8);
                if shared {
                    let end = &mut cc.at_mut(0).unwrap().end;
                    end.set_class_object(PointClassType::Vertex, Some(H(0, K::Vertex)));
                    end.set_t_param(0.0, false);
                    end.set_gap_3d(1e-8);
                }
                assert_eq!(
                    cc.has_near_misses(&k, &geo, false).unwrap(),
                    !normal_offset,
                    "shared={shared}, normal_offset={normal_offset}"
                );
            }
        }
    }

    #[test]
    fn a_seam_vertex_uses_its_coincident_interval_from_either_side() {
        use crate::geom::extent2d::Extent2d;
        use crate::math::Vec2;

        let k = Kernel::default();
        let mut geo = SeamGeo {
            curves: vec![
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
            plane: Surface::from(crate::surface::plane::Plane::new(
                Vec3::ZERO,
                Vec3::X,
                Vec3::Y,
                Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(-1.0, -1.0, 2.0, 1.0),
            )),
            vertex: Point3::new(0.5, 1e-8, 0.0),
        };
        for coincident in [0, 1] {
            let mut cc = classification(&k, &[0.0, 0.5, 1.0]);
            cc.set_uv_curve1(Some(H(1, K::Curve)));
            cc.set_surface1(Some(H(0, K::Surface)));
            for (i, end) in [(0, true), (1, false)] {
                let ivl = cc.at_mut(i).unwrap();
                let pc = if end { &mut ivl.end } else { &mut ivl.start };
                pc.set_class_object(PointClassType::Vertex, Some(H(0, K::Vertex)));
                pc.set_t_param(0.5, false);
                pc.set_gap_3d(1e-8);
            }
            cc.at_mut(coincident)
                .unwrap()
                .mid
                .set_class_object(PointClassType::Edge, Some(H(0, K::Edge)));
            // The unknown interval ends at the coincident interval's own vertex,
            // within its stored zone. Visiting that interval first must not
            // turn the same shared boundary into a near miss.
            assert!(!cc.has_near_misses(&k, &geo, false).unwrap());
            geo.vertex.y = 2e-6;
            cc.at_mut(0).unwrap().end.set_gap_3d(2e-6);
            cc.at_mut(1).unwrap().start.set_gap_3d(2e-6);
            assert!(cc.has_near_misses(&k, &geo, false).unwrap());
            geo.vertex.y = 1e-8;
            cc.at_mut(0).unwrap().end.set_gap_3d(1e-8);
            cc.at_mut(1).unwrap().start.set_gap_3d(1e-8);
            // A different vertex at the same parameter does not establish
            // that this boundary ends the coincident interval.
            let ivl = cc.at_mut(coincident).unwrap();
            let boundary = if coincident == 0 {
                &mut ivl.end
            } else {
                &mut ivl.start
            };
            boundary.set_class_object(PointClassType::Vertex, Some(H(9, K::Vertex)));
            assert!(cc.has_near_misses(&k, &geo, false).unwrap());
            // Without a coincident interval the same offset is a near miss.
            cc.at_mut(coincident)
                .unwrap()
                .mid
                .set_class_object(PointClassType::Unknown, None);
            assert!(cc.has_near_misses(&k, &geo, false).unwrap());
        }
    }
}
