//! Curve classification, part 3: locating a parameter among the intervals
//! (`FindInterval`, `FindPointClass`, `IsCoincident`), inserting, merging and removing
//! interval-boundary point classifications (`InsertPointClass`, `RemovePointClass`,
//! `MergePointClass`), and the topology-snapping cleanups (`CleanupAndValidate`,
//! `sm_ChangeEdgePointClassToNearVertex`, `sm_FixProblemsBySqueezing`).
//!
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust                                                    | Upstream                                         |
//! |---------------------------------------------------------|--------------------------------------------------|
//! | [`CurveClassification::find_interval`], [`FoundInterval`] | `SmCurveClassification::FindInterval`          |
//! | [`CurveClassification::find_point_class`]               | `SmCurveClassification::FindPointClass`          |
//! | [`CurveClassification::is_coincident`]                  | `SmCurveClassification::IsCoincident`            |
//! | [`CurveClassification::insert_point_class`], [`CurveClassification::insert_point_class_mut`] | `SmCurveClassification::InsertPointClass` |
//! | [`CurveClassification::remove_point_class`]             | `SmCurveClassification::RemovePointClass`        |
//! | [`CurveClassification::merge_point_class`]              | `SmCurveClassification::MergePointClass`         |
//! | [`CurveClassification::cleanup_and_validate`]           | `SmCurveClassification::CleanupAndValidate`      |
//! | [`change_edge_point_class_to_near_vertex`]              | `sm_ChangeEdgePointClassToNearVertex` (static)   |
//! | [`fix_problems_by_squeezing`]                           | `sm_FixProblemsBySqueezing` (static)             |
//! | [`CurveClassification::classify_edge_interval`]         | fork `SmCurveClassification::ClassifyEdgeInterval` (WR-305; both straight or curved pieces certified, not a midpoint sample) |
//! | [`CurveClassification::set_edge_interval`], [`EdgeToleranceRaise`] | not upstream (#18065): an interval on its edge once the edge's tolerance is raised to its certified distance |
//! | `circle_arcs_within`, `radial_deviation`, `coordinate_resolution` (private) | not upstream (#18065): two coplanar circles' NURB arcs within the band in closed form, allowing the coordinates' rounding only |
//!
//! Deviations:
//! - **Object handles.** As in part 1, every `SmObject` pointer is a caller-chosen handle
//!   `O` ([`ClassObject`]) and the geometry behind it comes from a [`ClassGeometry`]; the
//!   kernel is passed explicitly. The topology members the code calls (`SmEdge::GetVertex`,
//!   `GetInterval`, `GetCurve`, `SmVertex::GetPoint`, the zone tolerances of both) go
//!   through [`ClassGeometry::topology_entity`] (and [`ClassGeometry::topology_handle`]
//!   for the vertices of an edge); an adapter without those hooks keeps the
//!   [`SolidError::Unported`] deferral, as in `intersect::point_class`.
//! - **Vertex moves.** The `Combine` of a boundary merge moves the vertex of a vertex
//!   classification to a propagated surf/surf/surf point (`SmVertex::SetPoint`):
//!   [`CurveClassification::insert_point_class_mut`] makes it through a
//!   [`MergeIntervalBrep`]; [`CurveClassification::insert_point_class`] defers there.
//! - **Interval reference.** `InsertPointClass` writes the input classification through
//!   the reference `rPtClass_Interval` to slot `lIndex` it took before an
//!   `EditIntervalMin` / `EditIntervalMax` that may collapse an interval: the port writes
//!   to whatever interval now sits in that slot, and drops the write when the collapse
//!   left the slot past the end (upstream writes a spare slot of the array that `SetSize`
//!   resets before reuse), instead of failing on the index. This copies an upstream defect
//!   on purpose (UB-256,
//!   `plan/upstream-issues/UB-256--insert-point-class-stale-interval-reference.md`).
//! - **3D-distinct snaps (solidcore-native).** `FindInterval` snaps a parameter within
//!   `SM_EFF_ZERO_PARAM` of an interval end to that end only when it is not strictly
//!   inside the interval at a 3D distance of at least the pair intersection tolerance
//!   (and more than the curve's scaled zero) from the end's point; such a parameter goes
//!   on to the 3D tests (an interior point). Without a curve, or when that distance,
//!   tolerance or scaled zero cannot be computed, upstream's snap stands (no new error;
//!   unported, cancelled and uncertified still propagate).
//!   Upstream snaps on the parameter alone, which on a fast-parametrised curve (a loop
//!   bisector over an infinite cylinder's ±1234567 axial range) merged two edge
//!   crossings 3.5e-3 apart and lost a loop crossing (#18342).
//! - **Coincident vertices keep the first (solidcore-native).** When `InsertPointClass`
//!   merges a vertex into a boundary held by another vertex, both gaps are zero to the
//!   scaled zero of the boundary's vertex and their pre-snap parameters agree to the
//!   parameter's scaled zero, the boundary keeps the vertex it has. Both `Combine` calls
//!   (the boundary and its neighbour) still run; only their gap comparison is changed:
//!   the input is given the gap of the boundary it merges into, so the two zero gaps
//!   compare as equal and `Combine` keeps the boundary (`This`), copying the input's
//!   surf/surf/surf point and moving the vertex to it as upstream does on a tie. The
//!   neighbour is tied only when it holds the same vertex; any other neighbour (one still
//!   unknown, say) is merged as upstream. Upstream compares the two gaps as they are,
//!   rounding noise: which loop owned a seam crossing then changed with the model's
//!   scale (#18342, `query.face_p4` gen0381).
//! - **Out-params.** `FindInterval` returns a [`FoundInterval`]; `InsertPointClass` returns
//!   `rbInsertionMade`; `sm_ChangeEdgePointClassToNearVertex` returns
//!   `rbChangedToVertexFlag`; `MergePointClass`'s optional output arrays are
//!   `Option<&mut Vec<O>>` that are added to, as upstream. `IsCoincident` ignores the
//!   status of its two `FindInterval` calls and uses the outputs they leave, as upstream.
//! - **Circle arcs in closed form (#18065).** A run along an edge
//!   (`curve_runs_along`) also takes a pair of arcs of two circles in one plane (decided
//!   exactly) whose stored NURBs are proved within the band plus the coordinates' resolution
//!   `√3 ulp(m)` (`m` the larger coordinate magnitude): a rounding allowance, not a wider
//!   tolerance. Each NURB's radial deviation from its circle is bounded exactly from its
//!   control points (`radial_deviation`) and counted. A boss overhanging a closed circular
//!   edge by exactly the band is `tol + 2.9e-16` from it (its stored radius's rounding and
//!   its NURB's), within the allowance (`7.7e-16`) but beyond the chord enclosures' own
//!   slack.
//! - **Recursion.** `sm_FixProblemsBySqueezing` ends with a tail call that restarts the
//!   scan after it removed an interval; it is a `loop` here that restarts at the same
//!   point, so the order is unchanged and the depth no longer depends on the input. The
//!   restarts are bounded by the interval count plus one (each squeeze removes an
//!   interval); past it `SM_ERR` is returned where upstream would recurse without end.
//! - **Pointers into the interval array.** `GetPointClassByIndex` /
//!   `GetPointClassMateByIndex` return pointers; the helpers below return a
//!   `PointClassLoc` (interval, end) that `CurveClassification::pc_at` /
//!   `CurveClassification::pc_at_mut` resolve.
//! - **Undefined behaviour upstream** is an error here: a `FindInterval` / `IsCoincident` /
//!   `InsertPointClass` over a classification without intervals (the code reads
//!   `m_pData[0]`), an `IsCoincident` interval whose end classifications walk off the
//!   intervals, intervals of the other classification that do not exist
//!   (`CleanupAndValidate`, `sm_FixProblemsBySqueezing`), and a missing classification
//!   curve.
//! - **Not ported (reporting only).** `SM_ASSERT*`, `SE_MSG` / `SM_DBG_WARN` messages (the
//!   release build ignores `SE`), the `SM_DEBUG_CODE` drawing blocks, the commented-out
//!   closed-curve trial code and `sm_ChangeEdgePointClassToNearVertex`'s unused locals.
//! - **Part-2 callees** (`InsertAt`, `EditIntervalMin`, `EditIntervalMax`,
//!   `ConnectIntervals`, `GetClassifyCrvLoc(ULONG)`) are thin wrappers over the
//!   `intersect::curve_class::p2` members (wired with that unit). The two
//!   uses of `GetClassifyCrvLoc(ULONG)` in `FindInterval` (the first and the last
//!   boundary) are expanded inline through [`CurveClassification::classify_crv_loc`].
//!   `SmTol::IsDegenerate(const SmCurve*, SmExtent1d, SmXSectTol3d*)` (unit `geom.tol`,
//!   whose stub still names `curve.curve_p2`) is [`curve_is_degenerate`] here for the one
//!   case this file needs (an explicit tolerance).

use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::{ignore_status, EntityRef, Result, SolidError};
use crate::geom::certified::{next_down, next_up};
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p1::line_closest_point;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::{
    are_parallel, are_same, are_same_to_tol, eff_zero_param, eff_zero_sq, is_zero_to_tol,
    scaled_zero_of_curve, scaled_zero_sqrt_of, xsect_tol_3d_from_zones, ScaledZero,
};
use crate::intersect::curve_class::p1::{
    object_dimension, src_zone_tol_3d_of, xsect_tol_2d_or_3d_for_classify_pair,
    CurveClassification, CurveInterval, IntervalPosition,
};
use crate::intersect::curve_class::p6::{Imprint, MergeIntervalBrep, Mutable, ReadOnly};
use crate::intersect::curve_class::p7::{edge_of, vertex_of, CoincidentMaybe};
use crate::intersect::point_class::{
    object_zone_tol_3d, ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
    PointClassification, Precedence,
};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::math::gamma;
use crate::surface::surface::Surface;
use crate::topology::{edge as topo_edge, vertex as topo_vertex, VertexId};

const OP_FIND: &str = "intersect::curve_class::p3::CurveClassification::find_interval";
const OP_FIND_PC: &str = "intersect::curve_class::p3::CurveClassification::find_point_class";
const OP_COINCIDENT: &str = "intersect::curve_class::p3::CurveClassification::is_coincident";
const OP_INSERT: &str = "intersect::curve_class::p3::CurveClassification::insert_point_class";
const OP_REMOVE: &str = "intersect::curve_class::p3::CurveClassification::remove_point_class";
const OP_MERGE: &str = "intersect::curve_class::p3::CurveClassification::merge_point_class";
const OP_CLEANUP: &str = "intersect::curve_class::p3::CurveClassification::cleanup_and_validate";
const OP_NEAR_VERTEX: &str = "intersect::curve_class::p3::change_edge_point_class_to_near_vertex";
const OP_SQUEEZE: &str = "intersect::curve_class::p3::fix_problems_by_squeezing";
const OP_EDGE_INTERVAL: &str =
    "intersect::curve_class::p3::CurveClassification::classify_edge_interval";

use super::p1::{missing_defect, EDGE_NO_VERTEX, PC_NO_OBJECT};
use crate::error::intersect::IntersectFailure;

/// The index `FindInterval` leaves in `rlIndex` until it finds an interval.
pub const NO_INTERVAL: usize = 999_999;

/// The outputs of [`CurveClassification::find_interval`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoundInterval {
    /// `rlIndex`: the containing interval ([`NO_INTERVAL`] when none was found).
    pub index: usize,
    /// `rdEndDist3d`: for start and end positions, the distance along the curve to the
    /// interval end point (0 when the parameter is within tolerance of the boundary
    /// parameter).
    pub end_dist_3d: f64,
    /// `rePosition`: start, inside, end, or outside (the error value).
    pub position: IntervalPosition,
}

/// Which point classification of which interval a pointer from `GetPointClassByIndex`
/// would address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PointClassLoc {
    /// The interval.
    pub(super) interval: usize,
    /// [`IntervalPosition::Start`], `Inside` (the interior) or `End`.
    pub(super) end: IntervalPosition,
}

/// `SM_FALSE` / `SM_TRUE` / `UNSURE`, the tri-state `AreCoincidentMaybe` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Coincident {
    /// `FALSE`.
    No,
    /// `TRUE`.
    Yes,
    /// `UNSURE`.
    Unsure,
}

// ---- topology members, through the adapter's B-rep ------------------------------------

/// `SmEdge::GetVertex` and `SmEdge::GetOtherVertex(GetVertex())`: the edge's two vertices
/// (a `NULL` vertex is dereferenced by the caller).
pub(super) fn edge_vertices<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    edge: O,
) -> Result<(O, O)> {
    let (b, e) = edge_of(geometry, edge, "SmEdge::GetVertex")?;
    let v1 = topo_edge::vertex(b, e)?;
    let v2 = topo_edge::other_vertex(b, e, v1)?;
    let handle = |v: Option<VertexId>| {
        let v = v.ok_or_else(|| missing_defect(OP_NEAR_VERTEX, EDGE_NO_VERTEX))?;
        geometry
            .topology_handle_in(b, EntityRef::Vertex(v))
            .ok_or_else(|| SolidError::invalid_input(OP_NEAR_VERTEX, "vertex without handle"))
    };
    Ok((handle(v1)?, handle(v2)?))
}

/// `SmEdge::GetInterval`.
pub(super) fn edge_interval<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    edge: O,
) -> Result<Extent1d> {
    let (b, e) = edge_of(geometry, edge, "SmEdge::GetInterval")?;
    topo_edge::interval(b, e)
}

/// `SmEdge::GetCurve`.
pub(super) fn edge_curve<O: ClassObject, G: ClassGeometry<O>>(
    geometry: &G,
    edge: O,
) -> Result<Option<&Curve>> {
    let (b, e) = edge_of(geometry, edge, "SmEdge::GetCurve")?;
    topo_edge::curve(b, e)
}

/// `SmVertex::GetPoint`.
pub(super) fn vertex_point<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    vertex: O,
) -> Result<Point3> {
    let (b, v) = vertex_of(geometry, vertex, "SmVertex::GetPoint")?;
    topo_vertex::point(b, v)
}

/// `SmCurveClassification::GetPointClassByIndex(index)`: the location of boundary
/// `index` (`0..=size`).
pub(super) fn point_class_loc_by_index<O: ClassObject>(
    cc: &CurveClassification<O>,
    index: usize,
) -> Result<PointClassLoc> {
    let (interval, is_end) = cc.point_class_by_index_interval(index)?;
    Ok(PointClassLoc {
        interval,
        end: if is_end {
            IntervalPosition::End
        } else {
            IntervalPosition::Start
        },
    })
}

/// `SmCurveClassification::GetPointClassMateByIndex(index)`: the location of the
/// neighbouring interval's classification of the same boundary, if any.
fn point_class_mate_loc_by_index<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    index: usize,
) -> Result<Option<PointClassLoc>> {
    Ok(cc
        .point_class_mate_interval(k, geometry, index)?
        .map(|interval| PointClassLoc {
            interval,
            end: IntervalPosition::End,
        }))
}

/// `SmCurveClassification::GetClassifyCrvLoc(ULONG)`: the location of boundary `index`.
pub(super) fn classify_crv_loc_by_index<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    index: usize,
) -> Result<Point3> {
    Ok(cc.classify_crv_loc_by_index(k, geometry, index)?.0)
}

/// `SmCurveClassification::AreCoincidentMaybe(other, index, endOfCoincidentMaybe)`
/// (`bReportProb` at its default `TRUE`).
pub(super) fn are_coincident_maybe<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    other: &CurveClassification<O>,
    index: usize,
) -> Result<(Coincident, IntervalPosition)> {
    let (coin, position) = cc.are_coincident_maybe(k, geometry, other, index, true)?;
    let coin = match coin {
        CoincidentMaybe::No => Coincident::No,
        CoincidentMaybe::Fixable => Coincident::Yes,
        CoincidentMaybe::Unsure => Coincident::Unsure,
    };
    Ok((coin, position))
}

/// (not upstream) `input`, with the gap of the boundary `kept` when `tie` is set. Two
/// vertices whose gaps are both zero to the scaled zero then compare as equal in
/// `Combine`: the boundary keeps its vertex (`This`), and `Combine` still copies the
/// input's surf/surf/surf point (and moves the vertex there) as upstream does on a tie.
fn tied_to<O: ClassObject>(
    input: &PointClassification<O>,
    kept: &PointClassification<O>,
    tie: bool,
) -> PointClassification<O> {
    let mut tied = *input;
    if tie {
        tied.set_gap_3d(kept.deviation());
    }
    tied
}

/// Whether the boundary `pc` is classified to the vertex `object`.
fn holds_vertex<O: ClassObject>(pc: &PointClassification<O>, object: Option<O>) -> bool {
    pc.point_class() == PointClassType::Vertex && object.is_some() && pc.object() == object
}

/// `SmCurveClassification::InsertAt(index, interval)`.
fn insert_at<O: ClassObject>(
    k: &Kernel,
    cc: &mut CurveClassification<O>,
    index: usize,
    interval: &CurveInterval<O>,
) -> Result<()> {
    cc.insert_at(k, index, *interval)
}

/// Slot `index` of the interval array, as `InsertPointClass` reaches it through the
/// reference `rPtClass_Interval = (*this)[lIndex]` it took before an `EditIntervalMin` /
/// `EditIntervalMax` that may collapse an interval: after a collapse the slot holds the
/// interval that moved down into it, or lies past the end (`None`: the write lands in a
/// spare slot of the array that `SetSize` resets before it is used again). This copies an
/// upstream defect on purpose, for parity (UB-256,
/// `plan/upstream-issues/UB-256--insert-point-class-stale-interval-reference.md`): the
/// write can land on the interval that moved into the slot.
fn slot_mut<O: ClassObject>(
    cc: &mut CurveClassification<O>,
    index: usize,
) -> Option<&mut CurveInterval<O>> {
    if index < cc.size() {
        cc.at_mut(index).ok()
    } else {
        None
    }
}

/// `SmCurveClassification::EditIntervalMin(index, param, collapseDegenerateIntervals)`.
fn edit_interval_min<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &mut CurveClassification<O>,
    index: usize,
    param: f64,
    collapse_degenerate_intervals: bool,
) -> Result<()> {
    cc.edit_interval_min(k, geometry, index, param, collapse_degenerate_intervals)
}

/// `SmCurveClassification::EditIntervalMax(index, param, collapseDegenerateIntervals)`.
fn edit_interval_max<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &mut CurveClassification<O>,
    index: usize,
    param: f64,
    collapse_degenerate_intervals: bool,
) -> Result<()> {
    cc.edit_interval_max(k, geometry, index, param, collapse_degenerate_intervals)
}

/// `SmCurveClassification::ConnectIntervals(index, end, allowRemovalOfEnds)`.
pub(super) fn connect_intervals<O: ClassObject>(
    cc: &mut CurveClassification<O>,
    index: usize,
    end: IntervalPosition,
    allow_removal_of_ends: bool,
) -> Result<()> {
    cc.connect_intervals(index, end, allow_removal_of_ends)
}

/// `SmTol::IsDegenerate(const SmCurve*, SmExtent1d, SmXSectTol3d*)` with a given
/// tolerance: true for no curve, else `SmCurve::IsDegenerate(tol, &interval)`.
///
/// # Errors
///
/// Those of [`Curve::is_degenerate`].
pub fn curve_is_degenerate(
    k: &Kernel,
    curve: Option<&Curve>,
    interval: &Extent1d,
    xsect_tol_3d: f64,
) -> Result<bool> {
    match curve {
        None => Ok(true),
        Some(c) => c.is_degenerate(k, xsect_tol_3d, Some(interval)),
    }
}

// ---- helpers ------------------------------------------------------------------------------

fn unresolved(op: &'static str) -> SolidError {
    SolidError::invalid_input(op, "classification object handle does not resolve")
}

/// The classification curve (`m_cpCurve`), an error for none (upstream dereferences it).
fn cc_curve<'g, O: ClassObject, G: ClassGeometry<O>>(
    cc: &CurveClassification<O>,
    geometry: &'g G,
    op: &'static str,
) -> Result<&'g Curve> {
    let c = cc
        .curve()
        .ok_or_else(|| SolidError::invalid_input(op, "no classification curve"))?;
    geometry.curve(c).ok_or_else(|| unresolved(op))
}

/// `m_cpSurface1` (`None` for none).
fn cc_surface1<'g, O: ClassObject, G: ClassGeometry<O>>(
    cc: &CurveClassification<O>,
    geometry: &'g G,
    op: &'static str,
) -> Result<Option<&'g Surface>> {
    match cc.surface1() {
        None => Ok(None),
        Some(s) => geometry.surface(s).map(Some).ok_or_else(|| unresolved(op)),
    }
}

/// `sm_GetXSectTol2dOr3d_ForClassifyPair(m_cpCurve, m_cpSurface1, ..)` of a classification.
fn pair_tol<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    op: &'static str,
    obj1_param: f64,
    obj1_zone_tol_3d: f64,
    obj2_param: f64,
    obj2_zone_tol_3d: f64,
) -> Result<f64> {
    let curve = cc_curve(cc, geometry, op)?;
    let surface1 = cc_surface1(cc, geometry, op)?;
    xsect_tol_2d_or_3d_for_classify_pair(
        k,
        curve,
        surface1,
        obj1_param,
        obj1_zone_tol_3d,
        obj2_param,
        obj2_zone_tol_3d,
    )
}

/// `SmCurveClassification::Evaluate`-style: the position and unit tangent of `curve` at
/// `param` (`Evaluate(param, 1, from_left, pts)` then `Unitize` on the tangent).
fn point_and_unit_tangent(
    k: &Kernel,
    curve: &Curve,
    param: f64,
    from_left: bool,
    op: &'static str,
) -> Result<(Point3, Vec3)> {
    let d = curve.evaluate(k, param, 1, from_left, true)?;
    let p = *d
        .first()
        .ok_or_else(|| SolidError::invalid_input(op, "no position"))?;
    let t = *d
        .get(1)
        .ok_or_else(|| SolidError::invalid_input(op, "no tangent"))?;
    // the status of the in-place Unitize is ignored (a zero tangent becomes (0, 0, 1))
    Ok((p, t.unitize_or_z()))
}

fn interval_end_position(ivl: &CurveInterval<impl ClassObject>, end: IntervalPosition) -> f64 {
    if end == IntervalPosition::Start {
        ivl.interval.min()
    } else {
        ivl.interval.max()
    }
}

impl<O: ClassObject> CurveClassification<O> {
    /// The classification an upstream pointer from `GetPointClassByIndex` addresses.
    pub(super) fn pc_at(&self, loc: PointClassLoc) -> Result<&PointClassification<O>> {
        let ivl = self.at(loc.interval)?;
        Ok(match loc.end {
            IntervalPosition::Start => &ivl.start,
            IntervalPosition::Inside => &ivl.mid,
            _ => &ivl.end,
        })
    }

    /// Mutable [`CurveClassification::pc_at`].
    fn pc_at_mut(&mut self, loc: PointClassLoc) -> Result<&mut PointClassification<O>> {
        let ivl = self.at_mut(loc.interval)?;
        Ok(match loc.end {
            IntervalPosition::Start => &mut ivl.start,
            IntervalPosition::Inside => &mut ivl.mid,
            _ => &mut ivl.end,
        })
    }

    /// Finds the interval that contains the curve parameter `param`
    /// (`SmCurveClassification::FindInterval`): a parameter within the intersection
    /// tolerance of an interval boundary classifies as that interval's start or end (with
    /// the distance to the boundary), one in between as inside, and one beyond the whole
    /// curve that does not snap to its end is an error. `last_param` (`SM_BIG_DOUBLE` to
    /// ignore) keeps two parameters from both classifying to one boundary;
    /// `param_obj_zone_tol_3d` is the zone tolerance of the object `param` belongs to.
    ///
    /// # Errors
    ///
    /// [`IntersectFailure::ParameterOffClassifiedCurve`] (status `SM_ERR_OUTSIDE_OF_DOMAIN`)
    /// for a parameter off either end that does not snap;
    /// a defect (`SM_ERR`) when no interval is found; the errors of the curve evaluations;
    /// [`SolidError::InvalidInput`] without intervals or curve (upstream reads past them);
    /// [`SolidError::Unported`] from the zone tolerances of topology objects and the
    /// 2D tolerance mapping.
    pub fn find_interval(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        param: f64,
        last_param: f64,
        param_obj_zone_tol_3d: f64,
    ) -> Result<FoundInterval> {
        let mut out = FoundInterval {
            index: NO_INTERVAL,
            end_dist_3d: 0.0,
            position: IntervalPosition::Outside,
        };
        self.find_interval_inner(
            k,
            geometry,
            param,
            last_param,
            param_obj_zone_tol_3d,
            &mut out,
        )
        .map(|()| out)
    }

    /// `FindInterval` with its out-params left in `out` also on failure (`IsCoincident`
    /// ignores the status and reads them).
    pub(super) fn find_interval_inner(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        param: f64,
        last_param: f64,
        param_obj_zone_tol_3d: f64,
        out: &mut FoundInterval,
    ) -> Result<()> {
        // initialize output values (done by the caller)
        let eff_zero_param = eff_zero_param();
        let size = self.size();

        // low work - parameter below min bound
        {
            let first = self.at(0)?;
            if param < first.interval.min() - eff_zero_param {
                // tolerance - 2d or 3d as needed
                let tol_2d_or_3d = pair_tol(
                    k,
                    geometry,
                    self,
                    OP_FIND,
                    param,
                    param_obj_zone_tol_3d,
                    first.interval.min(),
                    first.start.obj_zone_tol_3d(k, geometry)?,
                )?;

                // Check 3d distance against sXSectTol3d. Also, check only off the end,
                // not interior: use the curve tangent.
                // GetClassifyCrvLoc(0): SSSPointFlag ? SSSPoint : Curve(Param)
                let start_pt =
                    self.classify_crv_loc(k, geometry, &first.start, first.interval.min())?;

                // cp_Curve position, UnintTan for dParameter (SER)
                let curve = cc_curve(self, geometry, OP_FIND)?;
                let (pt, tan) = point_and_unit_tangent(k, curve, param, true, OP_FIND)?;

                // Leave rdEndDist3d as actual 3d distance.
                out.end_dist_3d = start_pt.distance(pt);

                // note: EndPt has an SSSPoint, sStartPT may not lie on m_cpCurve; snap when
                // the distance along the curve is within the snapping distance
                let line_dist_3d = line_closest_point(pt, tan, start_pt);

                // But check vs. where it is on the curve. If off the curve at the
                // beginning, dLineDist3d will be negative.
                if line_dist_3d < -tol_2d_or_3d {
                    return Err(SolidError::intersect(
                        OP_FIND,
                        IntersectFailure::ParameterOffClassifiedCurve {
                            past_end: false,
                            distance: -line_dist_3d,
                            tolerance: tol_2d_or_3d,
                        },
                    ));
                }
                // snap to the beginning of the 1st interval
                out.position = IntervalPosition::Start;
                out.index = 0;
                return Ok(());
            }
        }

        // low work - parameter above max bound
        if size > 0 {
            let last_index = size - 1;
            let last = self.at(last_index)?;
            if param > last.interval.max() + eff_zero_param {
                // tolerance - 2d or 3d as needed
                let tol_2d_or_3d = pair_tol(
                    k,
                    geometry,
                    self,
                    OP_FIND,
                    param,
                    param_obj_zone_tol_3d,
                    last.interval.max(),
                    last.end.obj_zone_tol_3d(k, geometry)?,
                )?;

                // GetClassifyCrvLoc(GetSize()): the last boundary
                let end_pt = self.classify_crv_loc(k, geometry, &last.end, last.interval.max())?;

                // cp_Curve position, UnintTan for dParameter (SER)
                let curve = cc_curve(self, geometry, OP_FIND)?;
                let (pt, tan) = point_and_unit_tangent(k, curve, param, false, OP_FIND)?;

                out.end_dist_3d = end_pt.distance(pt);

                let line_dist_3d = line_closest_point(pt, tan, end_pt);

                // But check vs. where it is on the curve.
                if line_dist_3d > tol_2d_or_3d {
                    return Err(SolidError::intersect(
                        OP_FIND,
                        IntersectFailure::ParameterOffClassifiedCurve {
                            past_end: true,
                            distance: line_dist_3d,
                            tolerance: tol_2d_or_3d,
                        },
                    ));
                }
                // snap to the end of the last interval
                out.position = IntervalPosition::End;
                out.index = last_index;
                return Ok(());
            }
        }

        // for every interval
        for i in 0..size {
            let test = self.at(i)?;
            let ivl = test.interval;
            let is_last = if i == size - 1 { 1.0 } else { 0.0 };

            // continue - when parameter is above interval max - move onto next interval
            if param > ivl.max() + eff_zero_param * is_last {
                continue;
            }

            // done - when parameter is effectively equal to interval max - SM_IP_END
            // (unless it is strictly inside and a distinct point in 3D: see below)
            if param > ivl.max() - eff_zero_param
                && !self.distinct_from_end(
                    k,
                    geometry,
                    test,
                    param,
                    param_obj_zone_tol_3d,
                    IntervalPosition::End,
                )?
            {
                out.index = i;
                out.position = IntervalPosition::End;
                return Ok(());
            }

            // done - when parameter is effectively equal to interval min - SM_IP_START
            if param >= ivl.min() - eff_zero_param
                && param < ivl.min() + eff_zero_param
                && !self.distinct_from_end(
                    k,
                    geometry,
                    test,
                    param,
                    param_obj_zone_tol_3d,
                    IntervalPosition::Start,
                )?
            {
                out.index = i;
                out.position = IntervalPosition::Start;
                return Ok(());
            }

            // get here when parameter is within current interval; check the 3d distances
            // between points to classify this as an inside or boundary point
            out.index = i;

            // Test 3D space tolerances to see if point lies near existing end-points.
            let curve = cc_curve(self, geometry, OP_FIND)?;
            let start_pt = curve.evaluate_point(k, ivl.min())?;
            let end_pt = curve.evaluate_point(k, ivl.max())?;
            let classify_pt = curve.evaluate_point(k, param)?;

            // Start/End tolerances - 2d or 3d as needed
            let start_tol = pair_tol(
                k,
                geometry,
                self,
                OP_FIND,
                param,
                param_obj_zone_tol_3d,
                ivl.min(),
                test.start.obj_zone_tol_3d(k, geometry)?,
            )?;
            let end_tol = pair_tol(
                k,
                geometry,
                self,
                OP_FIND,
                param,
                param_obj_zone_tol_3d,
                ivl.max(),
                test.end.obj_zone_tol_3d(k, geometry)?,
            )?;

            // quick check - StartPoint/TestPoint and EndPoint/TestPoint gap sizes
            let mut start_3d_dist = classify_pt.distance(start_pt);
            let mut end_3d_dist = classify_pt.distance(end_pt);

            // ClassifyPt not within ParamSpace tol of EndPts, when within Space3D Tol3d,
            // watch for misClassify due to Closed curve - check arc length not Dist3D
            if start_3d_dist < start_tol || end_3d_dist < end_tol {
                let seg = Extent1d::from_min_max(ivl.min(), param);
                start_3d_dist = curve.approximate_length(k, &seg, 5, false)?.0;

                let seg = Extent1d::from_min_max(param, ivl.max());
                end_3d_dist = curve.approximate_length(k, &seg, 5, false)?.0;
            }

            // Use ParamSpace Dist:[Param,EndPt] to help classify
            let start_u_dist = (param - ivl.min()).abs();
            let end_u_dist = (param - ivl.max()).abs();
            let last_start_u_dist = (last_param - ivl.min()).abs();

            // when both points are within tolerance (this catches both closed intervals
            // and short intervals)
            if start_3d_dist < start_tol && end_3d_dist < end_tol {
                // Use parameter distances to pick the closer end.
                if start_u_dist < end_u_dist {
                    out.position = IntervalPosition::Start;
                    out.end_dist_3d = start_3d_dist;

                    // exception when lastPoint is closer to start than this point count
                    // this point as inside
                    if last_start_u_dist < start_u_dist {
                        out.position = IntervalPosition::Inside;
                    }
                } else {
                    out.position = IntervalPosition::End;
                    out.end_dist_3d = end_3d_dist;
                }
            }
            // ParameterPoint is close to IntervalStartPoint
            else if start_3d_dist < start_tol {
                out.position = IntervalPosition::Start;
                out.end_dist_3d = start_3d_dist;

                // exception when lastPoint is closer to start than this point count this
                // point as inside
                if last_start_u_dist < start_u_dist {
                    out.position = IntervalPosition::Inside;
                }
            }
            // ParameterPoint is close to IntervalEndPoint
            else if end_3d_dist < end_tol {
                out.position = IntervalPosition::End;
                out.end_dist_3d = end_3d_dist;
            } else {
                out.position = IntervalPosition::Inside;
                out.end_dist_3d = start_3d_dist.min(end_3d_dist);
            }

            // all done
            return Ok(());
        }

        // If make it here interval not found - have an error of some sort (not reached: a
        // parameter past the last interval returned above, and the last interval takes
        // every other one)
        Err(crate::error::Failure::new(
            OP_FIND,
            IntersectFailure::Defect {
                why: "no interval took the parameter, though a parameter past the last \
                      interval's end returns before the search and the last interval takes \
                      every other one",
            },
        )
        .with_status(crate::error::SM_ERR)
        .into_error())
    }

    /// Whether `param`, which `FindInterval` would snap to the `end` (start or end) of
    /// interval `test` because it is within `SM_EFF_ZERO_PARAM` of it in parameter, is
    /// strictly inside the interval and yet a distinct point in 3D: the curve point at
    /// `param` is at least the pair intersection tolerance (`XSectTol3d` of the
    /// parameter's object and the end's object, mapped to working space) away from the
    /// curve point at the interval end, and more than the curve's scaled zero
    /// (`SmTol::GetScaledZero` of the curve: `SM_EFF_ZERO` times its size), below which
    /// its evaluation cannot tell points apart. Such a point is classified by the 3D
    /// tests that follow instead of being snapped.
    ///
    /// Without a classification curve, or when a tolerance, point or scaled zero cannot
    /// be computed, it answers `false` and upstream's parameter snap stands, as upstream
    /// needed none of them to snap: a failure of this extra measurement is never a new
    /// error of `FindInterval`. The exceptions propagate, as elsewhere in this file: an
    /// unported callee, a cancellation or a certification refusal
    /// (`SolidError::is_unported_or_cancelled`) is never answered for.
    ///
    /// A parameter tolerance alone cannot tell points apart on a fast-parametrised
    /// curve: a loop bisector across an infinite cylinder spans the ±1234567 sentinel
    /// axial range over [0, 1], so 1.2e-8 in parameter is 0.03 in 3D. There two
    /// circle edges 3.5e-3 apart (a 3.5e-3 tall face at scale 1e-3) were snapped to
    /// one boundary and merged, dropping a loop crossing.
    fn distinct_from_end(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        test: &CurveInterval<O>,
        param: f64,
        param_obj_zone_tol_3d: f64,
        end: IntervalPosition,
    ) -> Result<bool> {
        let ivl = test.interval;
        // only a parameter strictly inside the interval can become an interior point
        if !(param > ivl.min() && param < ivl.max()) {
            return Ok(false);
        }
        // a classification without a resolvable curve has no 3D separation to measure:
        // keep upstream's parameter snap (which needs no curve)
        let Some(curve) = self.curve().and_then(|c| geometry.curve(c)) else {
            return Ok(false);
        };
        let (end_param, end_pc) = if end == IntervalPosition::Start {
            (ivl.min(), &test.start)
        } else {
            (ivl.max(), &test.end)
        };
        // any failure to measure keeps upstream's snap, which needed none of these
        let measured = || -> Result<bool> {
            let tol = pair_tol(
                k,
                geometry,
                self,
                OP_FIND,
                param,
                param_obj_zone_tol_3d,
                end_param,
                end_pc.obj_zone_tol_3d(k, geometry)?,
            )?;
            let end_pt = curve.evaluate_point(k, end_param)?;
            let classify_pt = curve.evaluate_point(k, param)?;
            let dist = classify_pt.distance(end_pt);
            if dist < tol {
                return Ok(false);
            }
            // and beyond the curve's scaled zero (its size times SM_EFF_ZERO): closer
            // points are not told apart by its evaluation
            Ok(dist > scaled_zero_of_curve(k, curve)?)
        };
        match measured() {
            Ok(distinct) => Ok(distinct),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Ok(false),
        }
    }

    /// The point classification at curve parameter `param`
    /// (`SmCurveClassification::FindPointClass`): that of the interval start, interior or
    /// end [`CurveClassification::find_interval`] picks.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::find_interval`].
    pub fn find_point_class(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        param: f64,
        param_obj_zone_tol_3d: f64,
    ) -> Result<PointClassification<O>> {
        // get interval containing given parameter
        let found = self.find_interval(k, geometry, param, BIG_DOUBLE, param_obj_zone_tol_3d)?;
        let ivl = self
            .at(found.index)
            .map_err(|_| SolidError::invalid_input(OP_FIND_PC, "no interval found"))?;

        // Fetch PointClassification identified by parameter
        Ok(match found.position {
            IntervalPosition::Inside => ivl.mid,
            IntervalPosition::Start => ivl.start,
            _ => ivl.end,
        })
    }

    /// Whether every point of `interval` classifies to some edge
    /// (`SmCurveClassification::IsCoincident`): the interiors of all the intervals it
    /// overlaps (not just touches at an end) are classified to an edge. An inexpensive
    /// test of the stored classifications, not of the geometry. The statuses of the
    /// two `FindInterval` calls are ignored.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the walk leaves the intervals (upstream reads
    /// past them); [`SolidError::Unported`] from `FindInterval`.
    pub fn is_coincident(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        interval: &Extent1d,
        ivl_obj_zone_tol_3d: f64,
    ) -> Result<bool> {
        // find ClassificationInterval for start point
        let mut found1 = FoundInterval {
            index: NO_INTERVAL,
            end_dist_3d: 0.0,
            position: IntervalPosition::Outside,
        };
        ignore_status(self.find_interval_inner(
            k,
            geometry,
            interval.min(),
            BIG_DOUBLE,
            ivl_obj_zone_tol_3d,
            &mut found1,
        ))?;
        let mut index1 = found1.index;

        // skip subintervals which only overlap by the endPoint
        if found1.position == IntervalPosition::End {
            index1 += 1;
        }

        // find interval for end point
        let mut found2 = FoundInterval {
            index: NO_INTERVAL,
            end_dist_3d: 0.0,
            position: IntervalPosition::Outside,
        };
        ignore_status(self.find_interval_inner(
            k,
            geometry,
            interval.max(),
            BIG_DOUBLE,
            ivl_obj_zone_tol_3d,
            &mut found2,
        ))?;
        // skip subintervals which only overlap by the startPoint (an unsigned decrement:
        // from 0 it wraps and the walk below never ends before it leaves the intervals)
        let index2 = if found2.position == IntervalPosition::Start {
            found2.index.checked_sub(1)
        } else {
            Some(found2.index)
        };

        // Does every interval from start to end classify to an edge
        let mut ii = index1;
        while index2.is_none_or(|last| ii <= last) {
            let ivl = self.at(ii).map_err(|_| {
                SolidError::invalid_input(OP_COINCIDENT, "interval walk leaves the intervals")
            })?;

            // When interval midPoint not classified to an EDGE
            if ivl.mid.point_class() != PointClassType::Edge {
                // the curve classification is not completely coincident
                return Ok(false);
            }
            ii += 1;
        }

        // all done
        Ok(true)
    }

    /// Inserts a point classification at curve parameter `curve_param`, or merges it with
    /// the classification of the interval boundary it snaps to
    /// (`SmCurveClassification::InsertPointClass`). A parameter inside an interval
    /// splits it; one on a boundary combines with the boundary's classification unless
    /// the two conflict (different edges, different vertices, a vertex and an edge not
    /// connected, an unclassified end that is off by more than the intersection
    /// tolerance), when it is inserted, or `force_insertion`. Returns `rbInsertionMade`:
    /// true when inserted or merged, false when an interior classification conflicts
    /// (the new one is ignored) or when the existing values were kept.
    ///
    /// `last_curve_param` (`SM_BIG_DOUBLE` to ignore) keeps two parameters of a sequence
    /// from both classifying to one boundary.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::find_interval`] and
    /// [`CurveClassification::insert_at`]; [`SolidError::Unported`] for topology
    /// objects, and for a surf/surf/surf point a merge propagates to a vertex
    /// classification (`SmVertex::SetPoint`; [`Self::insert_point_class_mut`] makes
    /// the move).
    pub fn insert_point_class(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        point_class: &PointClassification<O>,
        curve_param: f64,
        last_curve_param: f64,
        force_insertion: bool,
    ) -> Result<bool> {
        self.insert_point_class_in(
            &mut ReadOnly { k, g: geometry },
            point_class,
            curve_param,
            last_curve_param,
            force_insertion,
        )
    }

    /// [`CurveClassification::insert_point_class`] moving the vertex of a boundary
    /// classification a merged surf/surf/surf point propagates to
    /// (`SmVertex::SetPoint` of `Combine`) in the B-rep a [`MergeIntervalBrep`] hands
    /// out.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::insert_point_class`], without the deferral of
    /// the move; [`SolidError::InvalidInput`] for a vertex the adapter cannot resolve
    /// mutably.
    pub fn insert_point_class_mut(
        &mut self,
        k: &mut Kernel,
        geometry: &mut impl MergeIntervalBrep<O>,
        point_class: &PointClassification<O>,
        curve_param: f64,
        last_curve_param: f64,
        force_insertion: bool,
    ) -> Result<bool> {
        self.insert_point_class_in(
            &mut Mutable { k, g: geometry },
            point_class,
            curve_param,
            last_curve_param,
            force_insertion,
        )
    }

    /// `InsertPointClass` over either kind of adapter.
    pub(super) fn insert_point_class_in(
        &mut self,
        a: &mut impl Imprint<O>,
        point_class: &PointClassification<O>,
        curve_param: f64,
        last_curve_param: f64,
        force_insertion: bool,
    ) -> Result<bool> {
        let (k, geometry) = (a.kernel(), a.geometry());
        // init output
        let mut insertion_made = false;
        let mut curve_param = curve_param;

        // locals
        let mut do_insertion = force_insertion;
        let pt_class_type = point_class.point_class();
        let pt_class_deviation = point_class.deviation();
        let pt_class_zone_tol_3d = point_class.obj_zone_tol_3d(k, geometry)?;
        let pt_class_pre_snap_param = if point_class.pre_snap_param() == BIG_DOUBLE {
            curve_param
        } else {
            point_class.pre_snap_param()
        };

        // classify parameter against current CurveClassification intervals
        let found = self.find_interval(
            k,
            geometry,
            curve_param,
            last_curve_param,
            pt_class_zone_tol_3d,
        )?;
        let index = found.index;
        let mut position = found.position;
        let end_dist_3d = found.end_dist_3d;

        // for this CurveInterval
        let ivl = *self.at(index)?;

        // undo all but the tightest jumps
        {
            // get IvlEnd_Obj locals
            let (ivl_end_param, ivl_end_zone_tol_3d) = if position == IntervalPosition::Start {
                (ivl.interval.min(), ivl.start.obj_zone_tol_3d(k, geometry)?)
            } else {
                (ivl.interval.max(), ivl.end.obj_zone_tol_3d(k, geometry)?)
            };

            // XSectTol3d(IvlEnd_Obj, ThisPointClass_Obj) mapped to working space
            let xsect_tol_2d_or_3d = pair_tol(
                k,
                geometry,
                self,
                OP_INSERT,
                ivl_end_param,
                ivl_end_zone_tol_3d,
                curve_param,
                pt_class_zone_tol_3d,
            )?;

            let scaled_zero_sqrt = scaled_zero_sqrt_of(&curve_param);
            if ((position == IntervalPosition::Start
                && (ivl.interval.min() - curve_param).abs() > scaled_zero_sqrt)
                || (position == IntervalPosition::End
                    && (ivl.interval.max() - curve_param).abs() > scaled_zero_sqrt))
                && end_dist_3d > eff_zero_sq()
                && end_dist_3d > xsect_tol_2d_or_3d
                && ivl.interval.contains_value(curve_param, 0.0)
            {
                // undo all but the tightest jumps
                position = IntervalPosition::Inside;
            }
        }

        // Test whether we have to insert the point classification into an interval, or
        // merge it with a point classification that's already in. Normally
        // PointClassifications on existing boundaries are merged not inserted.
        // This big if-statement will check those cases on existing boundaries.

        // (not upstream) whether the merge below compares the input as tied with the
        // boundary: set for two vertices that are indistinguishable
        let mut keep_found = false;

        // when inserting point param is on a boundary and Insertion is to be computed
        if !do_insertion
            && (position == IntervalPosition::Start || position == IntervalPosition::End)
        {
            // boundary locals
            let boundary = if position == IntervalPosition::Start {
                &ivl.start
            } else {
                &ivl.end
            };
            let ivl_end_pt_param = interval_end_position(&ivl, position);
            let ivl_end_pt_pre_snap_param = if boundary.pre_snap_param() == BIG_DOUBLE {
                ivl_end_pt_param
            } else {
                boundary.pre_snap_param()
            };
            let ivl_end_pt_deviation = boundary.deviation();
            let ivl_end_pt_object = boundary.object();
            let ivl_end_pt_type = boundary.point_class();
            let ivl_end_pt_zone_tol_3d = boundary.obj_zone_tol_3d(k, geometry)?;

            // next a sequence of incompatible topology checks

            // when existing boundary and new point are both EDGE classified but to
            // different objects
            if ivl_end_pt_type == PointClassType::Edge
                && pt_class_type == PointClassType::Edge
                && ivl_end_pt_object != point_class.object()
            {
                // and parameters are not exactly equal
                if !are_same_to_tol(ivl_end_pt_param, curve_param, eff_zero_param()) {
                    // schedule insertion (makes a short interval)
                    do_insertion = true;
                }
            }

            // when existing boundary and new point are both VERTEX classified but to
            // different objects
            if ivl_end_pt_type == PointClassType::Vertex
                && pt_class_type == PointClassType::Vertex
                && ivl_end_pt_object != point_class.object()
            {
                // Gather CurvePtClass and IvlEndPt_PtClass local information
                let ivl_end_pt_point = vertex_point(
                    geometry,
                    ivl_end_pt_object.ok_or_else(|| missing_defect(OP_INSERT, PC_NO_OBJECT))?,
                )?;
                // (the second vertex's point is only read by the SE_MSG test below)
                let _pt_class_point = vertex_point(
                    geometry,
                    point_class
                        .object()
                        .ok_or_else(|| missing_defect(OP_INSERT, PC_NO_OBJECT))?,
                )?;

                let scaled_zero = ivl_end_pt_point.scaled_zero();
                let scaled_zero_param = ivl_end_pt_param.scaled_zero();

                let pt_class_zero_deviation = is_zero_to_tol(pt_class_deviation, scaled_zero);
                let ivl_end_pt_zero_deviation = is_zero_to_tol(ivl_end_pt_deviation, scaled_zero);

                let pt_class_param_count =
                    if are_same_to_tol(curve_param, pt_class_pre_snap_param, scaled_zero_param) {
                        1
                    } else {
                        2
                    };
                let ivl_end_pt_param_count = if are_same_to_tol(
                    ivl_end_pt_param,
                    ivl_end_pt_pre_snap_param,
                    scaled_zero_param,
                ) {
                    1
                } else {
                    2
                };
                let pre_snap_param_count = if are_same_to_tol(
                    pt_class_pre_snap_param,
                    ivl_end_pt_pre_snap_param,
                    scaled_zero_param,
                ) {
                    1
                } else {
                    2
                };

                // when the vertices are 3 space coincident or when deviations are both
                // zero or deviations are only less than tol and the vertices can't be
                // distinguished in the Curve's param space, upstream signals
                // SM_ERR_BAD_COINCIDENT_VERTICES (SE_MSG, no effect on the flow)
                let _ = (pt_class_param_count, ivl_end_pt_param_count);

                // When both PtClass Deviations are effectively zero
                if pt_class_zero_deviation && ivl_end_pt_zero_deviation {
                    // when the preSnap parameters are distinct - make a short interval
                    if pre_snap_param_count == 2 {
                        // make sure current interval ends on foundPreSnapParam. Since we
                        // are making a short (sub tolerance) interval, send FALSE so we
                        // don't call CollapseInterval
                        if position == IntervalPosition::Start {
                            ignore_status(edit_interval_min(
                                k,
                                geometry,
                                self,
                                index,
                                ivl_end_pt_pre_snap_param,
                                false,
                            ))?;
                        } else {
                            ignore_status(edit_interval_max(
                                k,
                                geometry,
                                self,
                                index,
                                ivl_end_pt_pre_snap_param,
                                false,
                            ))?;
                        }

                        // schedule insertion of new point at InputPreSnapParam
                        curve_param = pt_class_pre_snap_param;
                        do_insertion = true;
                    } else {
                        // the preSnap parameters are the same (upstream: SE_MSG only).
                        // Not upstream: keep the found vertex. Upstream merges, and
                        // `Combine` then keeps the vertex with the smaller gap; both gaps
                        // are zero to the scaled zero tested above, so that comparison
                        // orders rounding noise, and the vertex (hence the loop) that owns
                        // this crossing changed with the model's scale (query.face_p4
                        // gen0381: at scale 1e-3 the gaps 2.2e-19 and 0 kept the second
                        // vertex, at scale 1 the gaps 2.2e-16 and 8.9e-16 the first). The
                        // first vertex inserted is kept: the insertion order follows the
                        // topology, which is the same at every scale. The merge below still
                        // runs `Combine`, with the input tied to the boundary's gap.
                        keep_found = true;
                    }
                }
                // When the input PtClass Deviation is effectively zero and the found
                // PtClass Deviation is large - replace the found PtClass with the input
                else if pt_class_zero_deviation && !ivl_end_pt_zero_deviation {
                    // make sure current interval ends on input point preSnapParameter and
                    // overwrite IvlEndPt_PtClass with InputPtClass
                    if position == IntervalPosition::Start {
                        ignore_status(edit_interval_min(
                            k,
                            geometry,
                            self,
                            index,
                            pt_class_pre_snap_param,
                            true,
                        ))?;
                        // (upstream's stale reference to slot `index`, UB-256 parity: `slot_mut`)
                        if let Some(slot) = slot_mut(self, index) {
                            slot.start = *point_class;
                        }
                    } else {
                        ignore_status(edit_interval_max(
                            k,
                            geometry,
                            self,
                            index,
                            pt_class_pre_snap_param,
                            true,
                        ))?;
                        // (upstream's stale reference to slot `index`, UB-256 parity: `slot_mut`)
                        if let Some(slot) = slot_mut(self, index) {
                            slot.end = *point_class;
                        }
                    }

                    // no need to insert point
                    return Ok(false);
                }
                // When the found PtClass Deviation is effectively zero and the input
                // PtClass Deviation is large - use the Found PtClass
                else if !pt_class_zero_deviation && ivl_end_pt_zero_deviation {
                    // make sure current interval ends on found point preSnapParameter
                    if position == IntervalPosition::Start {
                        ignore_status(edit_interval_min(
                            k,
                            geometry,
                            self,
                            index,
                            ivl_end_pt_pre_snap_param,
                            true,
                        ))?;
                    } else {
                        ignore_status(edit_interval_max(
                            k,
                            geometry,
                            self,
                            index,
                            ivl_end_pt_pre_snap_param,
                            true,
                        ))?;
                    }

                    // no need to insert point
                    return Ok(false);
                }
                // When both PtClass Deviations are larger than zero but less than
                // tolerance
                else if !pt_class_zero_deviation && !ivl_end_pt_zero_deviation {
                    // treat this just like intersecting two vertices tightly: if they have
                    // distinct preSnapParameters - insert a short segment, else SE_MSG only
                    if pre_snap_param_count == 2 {
                        if position == IntervalPosition::Start {
                            ignore_status(edit_interval_min(
                                k,
                                geometry,
                                self,
                                index,
                                ivl_end_pt_pre_snap_param,
                                false,
                            ))?;
                        } else {
                            ignore_status(edit_interval_max(
                                k,
                                geometry,
                                self,
                                index,
                                ivl_end_pt_pre_snap_param,
                                false,
                            ))?;
                        }

                        // schedule insertion of new point at InputPreSnapParam
                        curve_param = pt_class_pre_snap_param;
                        do_insertion = true;
                    }
                }
            } // end conflicting VERTEX/VERTEX case

            // for conflicting VERTEX/EDGE and EDGE/VERTEX cases
            let adjacency: Option<(Option<O>, Option<O>)> = if ivl_end_pt_type
                == PointClassType::Vertex
                && pt_class_type == PointClassType::Edge
            {
                Some((ivl_end_pt_object, point_class.object()))
            } else if ivl_end_pt_type == PointClassType::Edge
                && pt_class_type == PointClassType::Vertex
            {
                Some((point_class.object(), ivl_end_pt_object))
            } else {
                None
            };

            // when an EDGE/VERTEX conflict exists
            if let Some((vertex, edge)) = adjacency {
                let edge = edge.ok_or_else(|| missing_defect(OP_INSERT, PC_NO_OBJECT))?;
                let (test_v, other_test_v) = edge_vertices(geometry, edge)?;

                // when the edge is not connected to the vertex and parameters are not
                // exactly equal
                if vertex != Some(test_v)
                    && vertex != Some(other_test_v)
                    && !are_same(ivl_end_pt_param, curve_param)
                {
                    // schedule insertion (makes a short interval)
                    do_insertion = true;
                }
            }

            // XSectTol3d(IvlEnd_Obj, ThisPointClass_Obj) mapped to working space
            let xsect_tol_2d_or_3d = pair_tol(
                k,
                geometry,
                self,
                OP_INSERT,
                ivl_end_pt_param,
                ivl_end_pt_zone_tol_3d,
                curve_param,
                pt_class_zone_tol_3d,
            )?;

            // when points are close but not exactly equal in 3D XYZ space to interval
            // Begin/End boundaries
            if (index == 0 || index + 1 == self.size())
                && ivl_end_pt_type == PointClassType::Unknown
                && end_dist_3d > xsect_tol_2d_or_3d
            {
                // schedule insertion (makes a short interval)
                do_insertion = true;
            }

            // when points are close but not exactly equal in 1D param space (bRunParamTest
            // is the constant TRUE)
            let scaled_zero_param = eff_zero_param();
            if ivl_end_pt_type == PointClassType::Unknown
                && ((position == IntervalPosition::Start
                    && curve_param > ivl_end_pt_param + scaled_zero_param)
                    || (position == IntervalPosition::End
                        && curve_param < ivl_end_pt_param - scaled_zero_param))
            {
                do_insertion = true;
            }
        } // end when inserting point param is on a boundary and Insertion is to be computed

        // State: bDoInsertion is set. Now either insert, or merge with start or end.
        let ivl = *self.at(index)?;

        // when inserting a PointClassification
        if position == IntervalPosition::Inside || do_insertion {
            // error case - conflicting classification objects
            if ivl.mid.point_class() != PointClassType::Unknown
                && ivl.mid.object() != point_class.object()
            {
                // Sorry we can't insert anything here because it already has a
                // classification (upstream only warns, SM_DBG_WARN)
                return Ok(false);
            }

            // execute the insertion
            insertion_made = true;

            // First duplicate this interval just past itself.
            insert_at(k, self, index + 1, &ivl)?;

            // Set the PointClass between the two intervals to be the given PointClass.
            {
                let ivl1 = self.at_mut(index)?;
                ivl1.end = *point_class;
                ivl1.interval = Extent1d::from_min_max(ivl1.interval.min(), curve_param);
                ivl1.mid.unset_parameters();
            }
            {
                let ivl2 = self.at_mut(index + 1)?;
                ivl2.start = *point_class;
                ivl2.interval = Extent1d::from_min_max(curve_param, ivl2.interval.max());
                ivl2.mid.unset_parameters();
            }
        }
        // when merging a start boundary point classification
        else if position == IntervalPosition::Start {
            let found_ivl = self.at_mut(index)?;
            let found_object = found_ivl.start.object();
            let input = tied_to(point_class, &found_ivl.start, keep_found);
            if a.combine(&mut found_ivl.start, &input)? == Precedence::Other {
                found_ivl.start = *point_class;
            }
            insertion_made = true;

            // remember to merge the neighbor interval's boundary point
            if index > 0 {
                let found2 = self.at_mut(index - 1)?;
                let tie = keep_found && holds_vertex(&found2.end, found_object);
                let input = tied_to(point_class, &found2.end, tie);
                if a.combine(&mut found2.end, &input)? == Precedence::Other {
                    found2.end = *point_class;
                }
            }
        }
        // when merging an end boundary point classification
        else if position == IntervalPosition::End {
            let found_ivl = self.at_mut(index)?;
            let found_object = found_ivl.end.object();
            let input = tied_to(point_class, &found_ivl.end, keep_found);
            if a.combine(&mut found_ivl.end, &input)? == Precedence::Other {
                found_ivl.end = *point_class;
            }
            insertion_made = true;

            // remember to merge the neighbor interval's boundary point
            if index + 1 < self.size() {
                let found2 = self.at_mut(index + 1)?;
                let tie = keep_found && holds_vertex(&found2.start, found_object);
                let input = tied_to(point_class, &found2.start, tie);
                if a.combine(&mut found2.start, &input)? == Precedence::Other {
                    found2.start = *point_class;
                }
            }
        }

        Ok(insertion_made)
    }

    /// Removes the point classification at the start or end of interval `index`
    /// (`SmCurveClassification::RemovePointClass`): an end of the whole curve goes back to
    /// unknown; a boundary between two intervals connects them
    /// (`ConnectIntervals(index, which, FALSE)`, whose status is ignored).
    ///
    /// # Errors
    ///
    /// [`IntersectFailure::PositionNotAnEnd`] for a position other than start or end and
    /// [`IntersectFailure::IntervalIndexPastEnd`] for an interval index past the end (both
    /// `SM_ERR`); those of [`CurveClassification::connect_intervals`].
    pub fn remove_point_class(&mut self, index: usize, position: IntervalPosition) -> Result<()> {
        // check input - eIPos an allowed value
        if position != IntervalPosition::Start && position != IntervalPosition::End {
            return Err(SolidError::intersect(
                OP_REMOVE,
                IntersectFailure::PositionNotAnEnd,
            ));
        }

        // check input - valid lIndex
        if index >= self.size() {
            return Err(SolidError::intersect(
                OP_REMOVE,
                IntersectFailure::IntervalIndexPastEnd {
                    index,
                    size: self.size(),
                },
            ));
        }

        // locals
        let has_neighbor = if position == IntervalPosition::Start {
            index > 0
        } else {
            index + 1 < self.size()
        };

        // Determine what action to take
        //  case 1: end Boundary Point with no neighbor - reset PointClassification to
        //          SM_PC_UNKNOWN
        //  case 2: End Boundary Point with a neighbor - connect the intervals
        if has_neighbor {
            // Point being removed divides two intervals (the status is ignored)
            ignore_status(connect_intervals(self, index, position, false))?;
        } else {
            // reset the PtClassification back to its SM_PC_UNKNOWN state
            let default_point = PointClassification::new(self.src_zone_tol_3d());
            let ivl = self.at_mut(index)?;
            if position == IntervalPosition::Start {
                ivl.start = default_point;
            } else {
                ivl.end = default_point;
            }
        }

        Ok(())
    }

    /// Merges the classification of boundary `index` (`0..=size`) into its object's
    /// topology (`SmCurveClassification::MergePointClass`): the vertex is created, split
    /// into an edge, or updated, the mate classification and every interval that
    /// referenced a split edge are repointed at the new edges.
    ///
    /// `opt_new_edges` (the unique new edges, some of which reuse the parent edge) and
    /// `opt_new_vertices` are added to, not reset.
    ///
    /// # Errors
    ///
    /// The errors of [`PointClassification::merge_into_object`]; `SM_ERR` when an old
    /// edge's classification parameter lies in neither new edge; the curve evaluation and
    /// drop-point errors; those of `GetPointClassByIndex` / `GetPointClassMateByIndex`
    /// ([`CurveClassification::point_class_by_index`],
    /// [`CurveClassification::point_class_mate_by_index`]); [`SolidError::Unported`] for
    /// the topology members.
    pub fn merge_point_class(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        opt_new_edges: Option<&mut Vec<O>>,
        opt_new_vertices: Option<&mut Vec<O>>,
    ) -> Result<()> {
        self.merge_point_class_in(
            &mut ReadOnly { k, g: geometry },
            index,
            opt_new_edges,
            opt_new_vertices,
        )
    }

    /// [`CurveClassification::merge_point_class`] editing the B-rep behind the
    /// classification object through a [`MergeIntervalBrep`]
    /// ([`PointClassification::merge_into_brep`]): the region, face, edge or vertex
    /// the point classifies to gets (or is) the vertex.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::merge_point_class`], without the deferral of the
    /// edit; [`SolidError::InvalidInput`] for an object the adapter cannot resolve
    /// mutably.
    pub fn merge_point_class_mut(
        &mut self,
        k: &mut Kernel,
        geometry: &mut impl MergeIntervalBrep<O>,
        index: usize,
        opt_new_edges: Option<&mut Vec<O>>,
        opt_new_vertices: Option<&mut Vec<O>>,
    ) -> Result<()> {
        self.merge_point_class_in(
            &mut Mutable { k, g: geometry },
            index,
            opt_new_edges,
            opt_new_vertices,
        )
    }

    /// `MergePointClass` over either kind of adapter.
    pub(super) fn merge_point_class_in(
        &mut self,
        a: &mut impl Imprint<O>,
        index: usize,
        opt_new_edges: Option<&mut Vec<O>>,
        opt_new_vertices: Option<&mut Vec<O>>,
    ) -> Result<()> {
        // get the min point's classification and param value for this interval
        let loc = point_class_loc_by_index(self, index)?;

        // Get the PointClass from the adjacent interval that corresponds to the same
        // location as this one (the first and last have a mate only for closed curves).
        let adjacent = point_class_mate_loc_by_index(a.kernel(), a.geometry(), self, index)?;

        // evaluate the interval's min-point's position in 3 space using the curve
        // definition (gets SSSPoint when available else the curve point)
        let vert_pnt = classify_crv_loc_by_index(a.kernel(), a.geometry(), self, index)?;

        // when the point classifies to an edge - remember that edge
        let (point_class_type, point_class_object) = {
            let pc = self.pc_at(loc)?;
            (pc.point_class(), pc.object())
        };
        let old_edge: Option<O> = if point_class_type == PointClassType::Edge {
            point_class_object
        } else {
            None
        };

        // merge the point classification into its m_pObject's topology structure
        let src_zone_tol_3d = self.src_zone_tol_3d();
        let mut pc = *self.pc_at(loc)?;
        let merged = a.merge_into_object(&mut pc, vert_pnt);
        *self.pc_at_mut(loc)? = pc;
        let merged = merged?;
        let new_edges = merged.new_edges;
        if let Some(vertices) = opt_new_vertices {
            vertices.extend(merged.new_vertices);
        }

        // remember unique new edges
        if let Some(edges) = opt_new_edges {
            for e in &new_edges {
                if !edges.contains(e) {
                    edges.push(*e);
                }
            }
        }

        // propagate classification to adjacent PointClassification
        if let Some(adj) = adjacent {
            let pc = *self.pc_at(loc)?;
            *self.pc_at_mut(adj)? = pc;
        }

        // when a vertex splits an existing edge - set interval m_pObject = One of NewEdges
        if let [new_e1, new_e2] = new_edges[..] {
            let (k, geometry) = (a.kernel(), a.geometry());
            // update old edge pointers

            // for every classification interval
            for i in 0..self.size() {
                // update CurveInterval.m_vStart PtClassification EDGE object ptr
                if self.at(i)?.start.object() == old_edge {
                    // use CurveInterval.m_vStart param to figure out to which new edge it
                    // classifies
                    let t = self.at(i)?.start.t_param();
                    if edge_interval(geometry, new_e1)?.contains_value(t, 0.0) {
                        self.at_mut(i)?
                            .start
                            .set_class_object(PointClassType::Edge, Some(new_e1));
                    } else if edge_interval(geometry, new_e2)?.contains_value(t, 0.0) {
                        self.at_mut(i)?
                            .start
                            .set_class_object(PointClassType::Edge, Some(new_e2));
                    } else {
                        return Err(SolidError::intersect(
                            OP_MERGE,
                            IntersectFailure::NotOnSplitEdges { parameter: t },
                        ));
                    }
                }

                // update CurveInterval.m_vEnd PtClassification EDGE ptr
                if self.at(i)?.end.object() == old_edge {
                    let t = self.at(i)?.end.t_param();
                    if edge_interval(geometry, new_e1)?.contains_value(t, 0.0) {
                        self.at_mut(i)?
                            .end
                            .set_class_object(PointClassType::Edge, Some(new_e1));
                    } else if edge_interval(geometry, new_e2)?.contains_value(t, 0.0) {
                        self.at_mut(i)?
                            .end
                            .set_class_object(PointClassType::Edge, Some(new_e2));
                    } else {
                        return Err(SolidError::intersect(
                            OP_MERGE,
                            IntersectFailure::NotOnSplitEdges { parameter: t },
                        ));
                    }
                }

                // update CurveInterval.m_vMid PtClassification EDGE ptr
                if self.at(i)?.mid.object() == old_edge {
                    // evaluate interval midPoint position
                    let curve = cc_curve(self, geometry, OP_MERGE)?;
                    let mid_pnt = curve.evaluate_point(k, self.at(i)?.interval.evaluate(0.5))?;

                    // XSectTol3d(SrcCurve, pNewEdge)
                    let mut this_xsect_tol_3d = xsect_tol_3d_from_zones(
                        k,
                        src_zone_tol_3d,
                        object_zone_tol_3d(k, geometry, Some(new_e1))?,
                    );

                    // drop interval midPoint to newE1 and newE2 with progressively larger
                    // tolerances until it can be classified to one of the new edges
                    let mut scale_tol = 1.0;
                    while scale_tol < 100.0 {
                        // when Ivl MidPt drops to NewE1 to tight tol - using normalize to
                        // stop short Ivls classifying to neighbor Ivl EndPts
                        let drop1 = edge_curve(geometry, new_e1)?
                            .ok_or_else(|| {
                                SolidError::intersect(OP_MERGE, IntersectFailure::EdgeWithoutCurve)
                            })?
                            .drop_point(
                                k,
                                &edge_interval(geometry, new_e1)?,
                                mid_pnt,
                                None,
                                this_xsect_tol_3d,
                                None,
                                SolverOperation::Normalize,
                            )?;

                        if drop1.success && drop1.distance < this_xsect_tol_3d {
                            // update midInterval Classification - and quit
                            self.at_mut(i)?
                                .mid
                                .set_class_object(PointClassType::Edge, Some(new_e1));
                            break;
                        }

                        // drop interval midPoint to NewE2
                        let drop2 = edge_curve(geometry, new_e2)?
                            .ok_or_else(|| {
                                SolidError::intersect(OP_MERGE, IntersectFailure::EdgeWithoutCurve)
                            })?
                            .drop_point(
                                k,
                                &edge_interval(geometry, new_e2)?,
                                mid_pnt,
                                None,
                                this_xsect_tol_3d,
                                None,
                                SolverOperation::Normalize,
                            )?;
                        if drop2.success && drop2.distance < this_xsect_tol_3d {
                            // update midInterval Classification to NewE2
                            self.at_mut(i)?
                                .mid
                                .set_class_object(PointClassType::Edge, Some(new_e2));
                            break;
                        }
                        // both drops worked but were out of XSectTol3d - use the closest
                        // one
                        else if drop1.success && drop2.success {
                            let closest = if drop1.distance < drop2.distance {
                                new_e1
                            } else {
                                new_e2
                            };
                            self.at_mut(i)?
                                .mid
                                .set_class_object(PointClassType::Edge, Some(closest));
                            break;
                        }

                        // arrive here when both Drop attempts failed - increase dropTol and
                        // try again
                        scale_tol *= 2.0;
                        this_xsect_tol_3d *= 2.0;
                    } // end while dropping to NewE1 and NewE2
                } // end need to update midInterval edge classification check
            } // end iter each classification interval
        } // end split edge with new vertex check

        Ok(())
    }

    /// Cleans up this classification and its homogenized partner `other`
    /// (`SmCurveClassification::CleanupAndValidate`): until no interval passes
    /// `AreCoincidentMaybe`, or is degenerate (then both its ends, and `other`'s, are set
    /// to the lower-dimensional object), removes that interval from both.
    ///
    /// # Errors
    ///
    /// [`IntersectFailure::NoClassObject`] (`SM_ERR`) when `AreCoincidentMaybe` is unsure; [`SolidError::InvalidInput`] when
    /// `other` has fewer intervals; those of
    /// [`CurveClassification::are_coincident_maybe`] and
    /// [`CurveClassification::connect_intervals`].
    pub fn cleanup_and_validate(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &mut Self,
    ) -> Result<()> {
        // Next cleanup process is to look for coincident-maybe cases and remove them.
        let src_zone_tol_3d = self.src_zone_tol_3d();
        let xsect_tol_3d = xsect_tol_3d_from_zones(k, src_zone_tol_3d, src_zone_tol_3d);

        // iterate until all intervals fail the AreCoincidentMaybe() test
        let mut done = false;
        while !done {
            done = true;
            // for every interval
            for ii in 0..self.size() {
                // if interval passes AreCoincidentMaybe() test
                let (mut coin, mut end_of_cm) = are_coincident_maybe(k, geometry, self, other, ii)?;
                if coin == Coincident::Unsure {
                    // This signals an unrecoverable error: unsure only for an end classified
                    // to an edge whose object is not an edge, the classification's object
                    return Err(crate::error::Failure::new(
                        OP_CLEANUP,
                        IntersectFailure::NoClassObject,
                    )
                    .with_status(crate::error::SM_ERR)
                    .into_error());
                }

                // Another check: degenerate intervals.
                if coin == Coincident::No && self.size() > 1 {
                    let curve = cc_curve(self, geometry, OP_CLEANUP)?;
                    let ivl = self.at(ii)?.interval;
                    if curve_is_degenerate(k, Some(curve), &ivl, xsect_tol_3d)? {
                        coin = Coincident::Yes;
                        // Have to set eEndOfCM: doesn't matter except at ends of CC.
                        end_of_cm = if ii == 0 {
                            IntervalPosition::End
                        } else {
                            IntervalPosition::Start
                        };

                        // If Start and End are on different objects, set both to be the
                        // lower-dimensionality object.
                        {
                            let ci = self.at_mut(ii)?;
                            let dim1 = object_dimension(ci.start.object());
                            let dim2 = object_dimension(ci.end.object());
                            if dim1 < dim2 {
                                ci.end = ci.start;
                            } else if dim2 < dim1 {
                                ci.start = ci.end;
                            }
                        }
                        {
                            let ci = other.at_mut(ii)?;
                            let dim1 = object_dimension(ci.start.object());
                            let dim2 = object_dimension(ci.end.object());
                            if dim1 < dim2 {
                                ci.end = ci.start;
                            } else if dim2 < dim1 {
                                ci.start = ci.end;
                            }
                        }
                    }
                } // end check for degenerate intervals

                if coin == Coincident::Yes {
                    // remove the interval (allow for deletion of end-intervals); the
                    // statuses are ignored
                    ignore_status(connect_intervals(self, ii, end_of_cm, true))?;
                    ignore_status(connect_intervals(other, ii, end_of_cm, true))?;

                    // start the search all over again
                    done = false;
                    break;
                } // end AreCoincidentMaybe() check
            } // end iter every interval
        } // end iter until all intervals fail the AreCoincidentMaybe() test

        Ok(())
    }
}

/// Moves the classification of boundary `index` from the edge it classifies to onto the
/// edge's nearer vertex when that vertex is within the intersection tolerance of
/// `classify_point` (`sm_ChangeEdgePointClassToNearVertex`), propagating to the mate
/// classification and removing a short interval the move leaves with both ends on the
/// same vertex. Returns `rbChangedToVertexFlag`.
///
/// # Errors
///
/// The errors of `RemovePointClass`, `GetPointClassByIndex` and
/// `GetPointClassMateByIndex`; [`SolidError::Unported`] for the topology members.
pub fn change_edge_point_class_to_near_vertex<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    index: usize,
    cc: &mut CurveClassification<O>,
    classify_point: Point3,
) -> Result<bool> {
    // init output
    let mut changed_to_vertex = false;

    // locals
    let loc = point_class_loc_by_index(cc, index)?;

    // no work - pPC does not classify to an edge
    if cc.pc_at(loc)?.point_class() != PointClassType::Edge {
        return Ok(false);
    }

    // more locals
    let pc = *cc.pc_at(loc)?;
    let edge = pc
        .object()
        .ok_or_else(|| missing_defect(OP_NEAR_VERTEX, PC_NO_OBJECT))?;
    let (edge_vertex1, edge_vertex2) = edge_vertices(geometry, edge)?;
    let dist1 = vertex_point(geometry, edge_vertex1)?.distance(classify_point);
    let dist2 = vertex_point(geometry, edge_vertex2)?.distance(classify_point);

    // We should not snap to vertices that are not within tolerances.
    let edge_zone_tol_3d = pc.obj_zone_tol_3d(k, geometry)?;
    let src_zone_tol_3d = pc.src_zone_tol_3d();

    let xsect_tol_3d1 = xsect_tol_3d_from_zones(
        k,
        src_zone_tol_3d,
        object_zone_tol_3d(k, geometry, Some(edge_vertex1))?,
    );
    let xsect_tol_3d2 = xsect_tol_3d_from_zones(
        k,
        src_zone_tol_3d,
        object_zone_tol_3d(k, geometry, Some(edge_vertex2))?,
    );

    let accepts_native_vertex = |vertex| -> Result<bool> {
        if !cc.native_material_contacts() {
            return Ok(true);
        }
        let Some(marker) =
            crate::intersect::point_class::periodic_trim_witness(k, geometry, vertex)?
        else {
            return Ok(true);
        };
        let Some(curve) = cc.curve().and_then(|curve| geometry.curve(curve)) else {
            return Ok(true);
        };
        let Some(support) = crate::curve::contact::NativeSupport::curve(curve, &cc.interval())
        else {
            return Ok(true);
        };
        let zone = object_zone_tol_3d(k, geometry, Some(vertex))?.max(edge_zone_tol_3d);
        Ok(!marker.excluded_by(
            k,
            &support,
            src_zone_tol_3d,
            zone,
            cc.native_foreign_support(),
        ))
    };
    // when target_point is closer to pEdgeVertex1 and within tolerance
    if dist1 < dist2 && dist1 < xsect_tol_3d1 && accepts_native_vertex(edge_vertex1)? {
        // inform the public that the classification was modified
        changed_to_vertex = true;

        // set pPC and its mate's PointClassification to classify to the pEdgeVertex1 vertex
        let pc_mut = cc.pc_at_mut(loc)?;
        pc_mut.set_class_object(PointClassType::Vertex, Some(edge_vertex1));
        pc_mut.set_gap_3d(dist1);
        let moved = *pc_mut;

        if let Some(mate) = point_class_mate_loc_by_index(k, geometry, cc, index)? {
            *cc.pc_at_mut(mate)? = moved;
        }
    }
    // when target_point is closer to pEdgeVertex2 and within tolerance
    else if dist2 <= dist1 && dist2 < xsect_tol_3d2 && accepts_native_vertex(edge_vertex2)? {
        // inform the public that the classification was modified
        changed_to_vertex = true;

        // set pPC and its mate's PointClassification to classify to the pEdgeVertex2 vertex
        let pc_mut = cc.pc_at_mut(loc)?;
        pc_mut.set_class_object(PointClassType::Vertex, Some(edge_vertex2));
        pc_mut.set_gap_3d(dist2);
        let moved = *pc_mut;

        if let Some(mate) = point_class_mate_loc_by_index(k, geometry, cc, index)? {
            *cc.pc_at_mut(mate)? = moved;
        }
    }

    // Check to see if change created a short interval with both ends classified to the
    // same vertex; remember to check intervals on both sides of this point classification
    if changed_to_vertex {
        // locals - look for Vertex objects on the other ends of adjacent intervals
        let pc_last_loc = if index > 0 {
            Some(point_class_loc_by_index(cc, index - 1)?)
        } else {
            None
        };
        let pc_next_loc = if index < cc.size() {
            Some(point_class_loc_by_index(cc, index + 1)?)
        } else {
            None
        };
        let vertex = cc.pc_at(loc)?.object();
        let vertex_last = match pc_last_loc {
            Some(l) if cc.pc_at(l)?.point_class() == PointClassType::Vertex => {
                cc.pc_at(l)?.object()
            }
            _ => None,
        };
        let vertex_next = match pc_next_loc {
            Some(l) if cc.pc_at(l)?.point_class() == PointClassType::Vertex => {
                cc.pc_at(l)?.object()
            }
            _ => None,
        };

        // when next interval has both ends classifying to the same vertex and is not meant
        // to be a closed interval. note: review next interval first in case both intervals
        // have to be removed
        let edge_xsect_tol_3d = xsect_tol_3d_from_zones(k, edge_zone_tol_3d, edge_zone_tol_3d);
        if vertex == vertex_next
            && curve_is_degenerate(
                k,
                edge_curve(geometry, edge)?,
                &cc.at(index)?.interval,
                edge_xsect_tol_3d,
            )?
        {
            // init then remove the interval (end intervals can end up being set to
            // SM_IP_UNKNOWN)
            let ivl = cc.at_mut(index)?;
            ivl.mid.set_class_object(PointClassType::Unknown, None);

            // Don't remove a PointClass at the end, remove an interior one if possible.
            let bigger_gap_start = ivl.start.deviation() > ivl.end.deviation();

            let which = if index == 0 {
                IntervalPosition::End
            } else if index == cc.size() || bigger_gap_start {
                IntervalPosition::Start
            } else {
                IntervalPosition::End
            };

            // (RemovePointClass is called without SER: its status is ignored)
            ignore_status(cc.remove_point_class(index, which))?;
        } // end when next interval has both ends classifying to the same vertex

        // when previous interval has both ends classifying to the same vertex
        if let Some(prev) = index.checked_sub(1) {
            if vertex == vertex_last
                && curve_is_degenerate(
                    k,
                    edge_curve(geometry, edge)?,
                    &cc.at(prev)?.interval,
                    edge_xsect_tol_3d,
                )?
            {
                let ivl = cc.at_mut(prev)?;
                ivl.mid.set_class_object(PointClassType::Unknown, None);

                let bigger_gap_start = ivl.start.deviation() > ivl.end.deviation();

                let which = if index <= 1 {
                    IntervalPosition::End
                } else if index > cc.size() || bigger_gap_start {
                    IntervalPosition::Start
                } else {
                    IntervalPosition::End
                };

                ignore_status(cc.remove_point_class(prev, which))?;
            } // end when previous interval has both ends classifying to the same vertex
        }
    } // end need to review intervals after pointClassification change check

    // no changes made - all done
    Ok(changed_to_vertex)
}

/// Aligns two classifications of one common curve by squeezing out an interval end that
/// maps to different intervals of the other classification, or an interval that maps to a
/// single boundary of the other (`sm_FixProblemsBySqueezing`), restarting the scan after
/// each squeeze of `cc`.
///
/// # Errors
///
/// `ConnectIntervals`' errors; [`SolidError::InvalidInput`] for intervals of `other`
/// that do not exist; [`SolidError::Unported`] for the zone tolerances of topology
/// objects.
pub fn fix_problems_by_squeezing<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &mut CurveClassification<O>,
    other: &mut CurveClassification<O>,
) -> Result<()> {
    // upstream recurses after a squeeze of cc; restarting the scan here visits the same
    // intervals in the same order
    // Bound (see the Deviations line): every squeeze of `cc` removes an interval, so more
    // restarts than it had intervals mean upstream would recurse without end.
    let allowed = cc.size().saturating_add(1);
    let mut restarts_left = allowed;
    'restart: loop {
        // for every CurveClass Interval
        for ii in 0..cc.size() {
            let civl = *cc.at(ii)?;
            let start_obj_zone_tol_3d = civl.start.obj_zone_tol_3d(k, geometry)?;
            let end_obj_zone_tol_3d = civl.end.obj_zone_tol_3d(k, geometry)?;

            // classify ThisCurve Interval BeginPoint against OtherCurve
            let found1 = match other.find_interval(
                k,
                geometry,
                civl.interval.min(),
                BIG_DOUBLE,
                start_obj_zone_tol_3d,
            ) {
                Ok(f) => f,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => continue,
            };

            // classify ThisCurve Interval EndPoint against OtherCurve
            let found2 = match other.find_interval(
                k,
                geometry,
                civl.interval.max(),
                BIG_DOUBLE,
                end_obj_zone_tol_3d,
            ) {
                Ok(f) => f,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => continue,
            };

            let (mut index1, mut position1) = (found1.index, found1.position);
            let (mut index2, mut position2) = (found2.index, found2.position);

            // When classifying a param that lies on an Interval boundary (expected case),
            // its possible that the param can be classified to the lastInterval's endPoint
            // or the nextIntervals StartPoint. Make sure that beginPoints map to
            // BeginPoints and EndPoints map to EndPoints.
            if position2 == IntervalPosition::Start && index2 > index1 {
                position2 = IntervalPosition::End;
                index2 -= 1;
            }
            if position1 == IntervalPosition::End && index1 < index2 {
                position1 = IntervalPosition::Start;
                index1 += 1;
            }

            // Now both classifications should be mapped to same interval. If Not: try
            // heuristic to fix things: remove OtherCurve boundaryPoint to merge two
            // OtherCurve intervals.

            // when ThisCurve interval EndPoints map to different OtherCurve Intervals
            if index1 != index2 {
                // get OtherCurve intervals containing thisCurve Interval endPoints
                let len1 = other.at(index1)?.interval.length();
                let len2 = other.at(index2)?.interval.length();

                // For the shortest OtherCurve interval
                let shortest_index = if len1 < len2 { index1 } else { index2 };

                // select the otherCurve interval end to squeeze: when ends are connected to
                // different kinds of topology save the end connected to most specific
                // topology, else save the end with the best tolerance.
                let squeeze_pos = other.find_best_squeeze_end(shortest_index)?;

                // remove eSqueezePos end of lShortestIndex to merge two intervals into one
                // (SER)
                connect_intervals(other, shortest_index, squeeze_pos, false)?;
            }
            // else when: ThisCurve Interval endPoints map to the same OtherCurve boundary
            // Point - remove ThisCurve Interval treating it like a zero length object
            else if position1 == position2
                && (position1 == IntervalPosition::Start || position2 == IntervalPosition::End)
            {
                // Select ThisCurve IntervalEndPoint with worst tolerance
                let squeeze_pos = cc.find_best_squeeze_end(ii)?;

                // squeeze Selected ThisCurve IntervalEndPoint to its other endPoint
                // location (SER)
                connect_intervals(cc, ii, squeeze_pos, true)?;

                // after modifying ThisCurve - start over
                restarts_left = restarts_left.checked_sub(1).ok_or_else(|| {
                    SolidError::intersect(
                        OP_SQUEEZE,
                        IntersectFailure::RepairDidNotSettle { restarts: allowed },
                    )
                })?;
                continue 'restart;
            } // end thisCurveInterval is equivalent to zero length
        } // end iter every CurveClass interval

        // all done
        return Ok(());
    }
}

/// Not upstream (#18065): interval `index` runs along edge `edge` once the edge's tolerance is
/// raised to `tolerance` ([`CurveClassification::classify_edge_interval`]): a certified upper
/// bound of the distance between the interval's curve and the edge, past the intersection
/// tolerance and within the tolerance of a vertex of the edge whose ball holds an end of the
/// interval ([`tail_gap_within_vertex_ball`]). The caller raises the edge
/// ([`crate::topology::edge::set_tolerance`], only if larger, cascading to its vertices: the
/// edge's other vertex can rise to `tolerance` too) and then classifies the interval
/// ([`CurveClassification::set_edge_interval`]), or leaves it unclassified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeToleranceRaise<O> {
    /// The interval's index in the classification.
    pub index: usize,
    /// The edge handle the interval runs along.
    pub edge: O,
    /// The tolerance the edge needs.
    pub tolerance: f64,
    /// For the interval's start and end, the parameter on the edge of one that was
    /// unclassified and lies on it.
    pub unclassified: [Option<f64>; 2],
}

impl<O: ClassObject> CurveClassification<O> {
    /// Classify interval `index`, whose two ends lie on one face boundary edge, as on that
    /// edge (the fork's `SmCurveClassification::ClassifyEdgeInterval`, WR-305): when the
    /// interval and the edge are both straight, when both ends lie within the intersection
    /// tolerance of the edge's line (the distance to a line is convex along the interval, so
    /// its ends bound it); otherwise when the interval's ends are farther than that tolerance
    /// apart and the whole curve over the interval and the edge piece between its ends are
    /// proved within the intersection tolerance of each other on enclosures
    /// ([`curve_piece_runs_along_edge`]; the fork reads the midpoint only, a sample that also
    /// takes a curve bowing far off the edge between its ends, or a piece no longer than the
    /// tolerance, a point at it, crossing the edge). Both ends
    /// are on edge `E` when each classifies to `E` or to one of its two vertices and at least
    /// one to `E` itself. An interval whose midpoint is already classified, and a 2D
    /// classification curve's, is left as it is.
    ///
    /// Not upstream: an end the edge
    /// intersections left unclassified counts as on `E` when its point is within the intersection tolerance of `E`, farther than that from both
    /// vertices of `E`, the curve is tangent to `E` at both ends of the interval
    /// ([`tangent_to_edge_at_ends`]), and the whole curve over the interval is proved within that
    /// tolerance of `E` on enclosures ([`piece_within_band`]); it is then classified to
    /// `E` with its parameter on `E`. The tangency keeps out a curve that crosses `E` at the
    /// interval's classified end and stops a hair past it: a piece that short lies within the
    /// band whatever its angle to `E`. A curve that runs along an edge and ends on it without
    /// crossing it (an arc 1.4e-11 from another arc, ending 2.8e-5 short of that arc's
    /// corner) has no intersection there. Such an interval can have one end on a vertex and
    /// the other unclassified: `E` is then each edge of the classification's face at that
    /// vertex in turn.
    ///
    /// Not upstream (#18065): a curved interval not proved within the band, with an end in the
    /// ball of a vertex of `E` wider than the band, is returned as an [`EdgeToleranceRaise`]
    /// when its certified distance from `E` is within that vertex's tolerance
    /// ([`tail_gap_within_vertex_ball`]; typically a round's arc leaving a straight edge
    /// tangentially at its end); it is left unclassified for the caller to raise `E` and
    /// classify it. A certified distance below the band classifies it here. Every candidate
    /// edge is tried before a raise is returned: an edge the interval runs along as it is wins
    /// over one that needs a raise, and of those the first in candidate order is returned.
    /// A classification that may not set tolerances ([`Self::ok_to_set_tolerance`] false, as
    /// the seam checks) never gets a raise: such an interval is left unclassified, for the
    /// face classification, and no B-rep tolerance changes.
    ///
    /// # Errors
    ///
    /// An unresolved handle; those of the evaluations and the drops to the edge.
    #[must_use = "a returned raise leaves the interval unclassified until it is applied"]
    pub fn classify_edge_interval(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
    ) -> Result<Option<EdgeToleranceRaise<O>>> {
        let ivl = *self.at(index)?;

        // the edge one of the interval's ends classifies to
        let edge = if ivl.start.point_class() == PointClassType::Edge {
            ivl.start.object()
        } else if ivl.end.point_class() == PointClassType::Edge {
            ivl.end.object()
        } else {
            None
        };

        // no work - Mid already classified, or a 2D classification curve
        if ivl.mid.point_class() != PointClassType::Unknown
            || cc_curve(self, geometry, OP_EDGE_INTERVAL)?.dim() != 3
        {
            return Ok(None);
        }

        // the edge one of the interval's ends classifies to; else (not upstream) the face's
        // edges at a vertex one end classifies to, when the other end is unclassified
        let mut candidates: Vec<O> = edge.into_iter().collect();
        let any_unclassified = [&ivl.start, &ivl.end]
            .iter()
            .any(|end| end.point_class() == PointClassType::Unknown);
        if candidates.is_empty() && any_unclassified {
            if let Some((fb, EntityRef::Face(f))) =
                self.face().and_then(|face| geometry.topology_entity(face))
            {
                for end in [&ivl.start, &ivl.end] {
                    let (PointClassType::Vertex, Some(vertex)) = (end.point_class(), end.object())
                    else {
                        continue;
                    };
                    let (vb, v) = vertex_of(geometry, vertex, "SmVertex::GetEdges")?;
                    if !core::ptr::eq(vb, fb) {
                        continue;
                    }
                    for e in topo_vertex::edges(vb, v, Some(f))? {
                        if let Some(h) = geometry.topology_handle_in(vb, EntityRef::Edge(e)) {
                            candidates.push(h);
                        }
                    }
                }
            }
        }

        let mut first_raise = None;
        for edge in candidates {
            // an edge with no 3D curve proves nothing: no interval runs along it (the curve
            // found here is the one every test below uses)
            let Some(target) = edge_curve(geometry, edge)? else {
                continue;
            };
            let Some(unclassified) = self.ends_on_edge(k, geometry, (edge, target), &ivl)? else {
                continue;
            };
            let xsect_tol_3d = xsect_tol_3d_from_zones(
                k,
                src_zone_tol_3d_of(k, Some(&*self)),
                object_zone_tol_3d(k, geometry, Some(edge))?,
            );

            // Not upstream (#15718): both curves straight. The interval runs along the edge
            // when both its ends lie within the intersection tolerance of the edge's line
            // ([`straight_run_on_line`], a certificate, not a sample).
            let curve = cc_curve(self, geometry, OP_EDGE_INTERVAL)?;
            let straight = matches!(curve.kind(), CurveKind::Line(_))
                && matches!(target.kind(), CurveKind::Line(_));
            let on_edge = if straight {
                let edge_ivl = edge_interval(geometry, edge)?;
                straight_run_on_line(k, curve, &ivl.interval, target, &edge_ivl, xsect_tol_3d)?
            } else {
                curve_piece_runs_along_edge(
                    k,
                    geometry,
                    (edge, target),
                    curve,
                    &ivl.interval,
                    xsect_tol_3d,
                )?
            };

            if on_edge {
                self.mark_edge_interval(index, edge, unclassified)?;
                return Ok(None);
            }
            // Not upstream (#18065): a curved piece leaving the edge tangentially into the ball
            // of one of its vertices, past the band but within that vertex's tolerance
            if !straight {
                if let Some(gap) = tail_gap_within_vertex_ball(
                    k,
                    geometry,
                    (edge, target),
                    curve,
                    &ivl.interval,
                    xsect_tol_3d,
                )? {
                    if gap < xsect_tol_3d {
                        // proved within the band after all (a finer cut than `curve_runs_along`'s)
                        self.mark_edge_interval(index, edge, unclassified)?;
                        k.work_counters.add_curved_edge_run();
                        return Ok(None);
                    }
                    // a classification that may not set tolerances (the seam checks) gets
                    // no raise: the interval is left for the face classification
                    if first_raise.is_none() && self.ok_to_set_tolerance() {
                        first_raise = Some(EdgeToleranceRaise {
                            index,
                            edge,
                            tolerance: gap,
                            unclassified,
                        });
                    }
                }
            }
        }
        Ok(first_raise)
    }

    /// Classify interval `raise.index` as on edge `raise.edge`, with each end
    /// [`Self::classify_edge_interval`] found unclassified on it at its parameter there. The
    /// caller of a raise ([`EdgeToleranceRaise::tolerance`]) calls it once the edge's tolerance
    /// is raised, and only then: the interval is on the edge at the raised tolerance only.
    ///
    /// # Errors
    ///
    /// An interval index out of range.
    pub fn set_edge_interval(&mut self, raise: &EdgeToleranceRaise<O>) -> Result<()> {
        self.mark_edge_interval(raise.index, raise.edge, raise.unclassified)
    }

    /// Classify interval `index` as on `edge`, each end unclassified before and on the edge at
    /// its parameter `unclassified` there.
    fn mark_edge_interval(
        &mut self,
        index: usize,
        edge: O,
        unclassified: [Option<f64>; 2],
    ) -> Result<()> {
        let at = self.at_mut(index)?;
        at.mid.set_class_object(PointClassType::Edge, Some(edge));
        for (pc, param) in [
            (&mut at.start, unclassified[0]),
            (&mut at.end, unclassified[1]),
        ] {
            if let Some(param) = param {
                pc.set_class_object(PointClassType::Edge, Some(edge));
                pc.set_t_param(param, false);
            }
        }
        Ok(())
    }

    /// Whether both ends of interval `ivl` lie on `edge` (see
    /// [`Self::classify_edge_interval`]), and for each of them, start and end, that was
    /// unclassified and lies on it its parameter on `edge`. `None` when an end is elsewhere,
    /// when neither end is classified, when an unclassified end is within the intersection
    /// tolerance of a vertex of `edge` (it would be on that vertex, not inside the edge), or
    /// when with an unclassified end the curve is not tangent to `edge` at both ends of the
    /// interval ([`tangent_to_edge_at_ends`]) or not proved within that tolerance of `edge` as a
    /// whole ([`piece_within_band`]). `edge_crv` is the curve of `edge`.
    fn ends_on_edge(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        (edge, edge_crv): (O, &Curve),
        ivl: &CurveInterval<O>,
    ) -> Result<Option<[Option<f64>; 2]>> {
        let (eb, e) = edge_of(geometry, edge, "SmEdge::GetStartVertex")?;
        let vertex_handle = |v: Option<VertexId>| {
            v.and_then(|v| geometry.topology_handle_in(eb, EntityRef::Vertex(v)))
        };
        let (start_v, end_v) = (
            topo_edge::start_vertex(eb, e)?,
            topo_edge::end_vertex(eb, e)?,
        );
        let (start_vertex, end_vertex) = (vertex_handle(start_v), vertex_handle(end_v));
        let xsect_tol_3d = xsect_tol_3d_from_zones(
            k,
            src_zone_tol_3d_of(k, Some(self)),
            object_zone_tol_3d(k, geometry, Some(edge))?,
        );
        let curve = cc_curve(self, geometry, OP_EDGE_INTERVAL)?;
        let mut unclassified = [None; 2];
        let ends = [
            (&ivl.start, ivl.interval.min()),
            (&ivl.end, ivl.interval.max()),
        ];
        for ((end, t), flag) in ends.into_iter().zip(unclassified.iter_mut()) {
            let on_edge = (end.point_class() == PointClassType::Edge && end.object() == Some(edge))
                || (end.point_class() == PointClassType::Vertex
                    && end.object().is_some()
                    && (end.object() == start_vertex || end.object() == end_vertex));
            if on_edge {
                continue;
            }
            // (not upstream) an unclassified end on the edge, clear of its vertices
            if end.point_class() != PointClassType::Unknown {
                return Ok(None);
            }
            let p = curve.evaluate_point(k, t)?;
            let Some((d, param)) = drop_onto_edge(k, geometry, (edge, edge_crv), p, xsect_tol_3d)?
            else {
                return Ok(None);
            };
            if d.is_nan() || d >= xsect_tol_3d {
                return Ok(None);
            }
            for v in [start_v, end_v].into_iter().flatten() {
                let gap = topo_vertex::point(eb, v)?.distance(p);
                if gap.is_nan() || gap <= xsect_tol_3d {
                    return Ok(None);
                }
            }
            *flag = Some(param);
        }
        let (t0, t1) = (ivl.interval.min(), ivl.interval.max());
        let params = match unclassified {
            [None, None] => return Ok(Some(unclassified)),
            [Some(_), Some(_)] => return Ok(None),
            // the classified end's parameter on the edge, for the proof over the interval
            [Some(s0), None] => {
                let p = curve.evaluate_point(k, t1)?;
                drop_onto_edge(k, geometry, (edge, edge_crv), p, xsect_tol_3d)?
                    .map(|(_, s1)| [s0, s1])
            }
            [None, Some(s1)] => {
                let p = curve.evaluate_point(k, t0)?;
                drop_onto_edge(k, geometry, (edge, edge_crv), p, xsect_tol_3d)?
                    .map(|(_, s0)| [s0, s1])
            }
        };
        let Some(params) = params else {
            return Ok(None);
        };
        // A curve that crosses the edge at one end of a short interval keeps the whole piece
        // within the band too, so the piece runs along the edge only when it leaves neither
        // end across it: tangent to the edge at both
        if !tangent_to_edge_at_ends(k, (curve, [t0, t1]), (edge_crv, params))? {
            return Ok(None);
        }
        let proved = piece_within_band(k, curve, &ivl.interval, edge_crv, params, xsect_tol_3d)?;
        Ok(proved.then_some(unclassified))
    }
}

/// Not upstream: whether `curve` at both ends `[t0, t1]` (`t0 < t1`) of a piece runs
/// parallel to `edge_curve` at the parameters `[s0, s1]` its ends drop onto, either way,
/// within the angle tolerance ([`are_parallel`]); never for a zero or non-finite tangent.
///
/// Each tangent is read from inside the piece, which differs from the other side only at
/// a knot: on `curve` the span starting at `t0` and the span ending at `t1`, on
/// `edge_curve` the span from each drop towards the other. `from_left` picks the span
/// `[U[i], U[i+1])` holding the parameter ([`Side::Left`](crate::nurbs::knots::Side::Left)),
/// so at a knot `true` reads the span starting there and `false` the span ending there.
///
/// # Errors
///
/// Those of the derivative evaluations.
fn tangent_to_edge_at_ends(
    k: &Kernel,
    (curve, [t0, t1]): (&Curve, [f64; 2]),
    (edge_curve, [s0, s1]): (&Curve, [f64; 2]),
) -> Result<bool> {
    let tangent = |c: &Curve, u: f64, from_left: bool| -> Result<Option<Vec3>> {
        let d = c.evaluate(k, u, 1, from_left, true)?;
        Ok(d.get(1)
            .copied()
            .filter(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()))
    };
    for (t, t_left, s, s_left) in [(t0, true, s0, s0 < s1), (t1, false, s1, s1 < s0)] {
        let parallel = match (tangent(curve, t, t_left)?, tangent(edge_curve, s, s_left)?) {
            (Some(a), Some(b)) => are_parallel(a, b, None),
            _ => false,
        };
        if !parallel {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Not upstream: whether `curve` over `interval` lies within `band` of `edge_curve` as a
/// whole, where the two ends of `interval` drop onto `edge_curve` at `edge_params`; decided
/// on enclosures, never on samples ([`pieces_apart_bound`]).
///
/// # Errors
///
/// Those of the end evaluations and of a curve on a surface's enclosure ([`piece_tube`]).
fn piece_within_band(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    edge_curve: &Curve,
    edge_params: [f64; 2],
    band: f64,
) -> Result<bool> {
    // written so that a NaN bound refuses
    Ok(
        pieces_apart_bound(k, curve, interval, edge_curve, edge_params, band)?
            .is_some_and(|d| d < band),
    )
}

/// A piece of a curve enclosed about its chord `[a, b]`: every point of the piece within
/// `rho` of that segment and every point of the segment within `rho` of the piece, `a` and
/// `b` within `ea` and `eb` of the piece's exact ends ([`piece_tube`]).
#[derive(Clone, Copy)]
struct Tube {
    a: Point3,
    ea: f64,
    b: Point3,
    eb: f64,
    rho: f64,
}

impl Tube {
    /// The same tube with its ends swapped.
    const fn reversed(self) -> Self {
        Self {
            a: self.b,
            ea: self.eb,
            b: self.a,
            eb: self.ea,
            rho: self.rho,
        }
    }
}

/// Pieces of a curve on a surface's chord gap ([`crv_on_surf_tube`]) bisects before it
/// refuses.
const CRV_ON_SURF_TUBE_PIECES: usize = 256;

/// Not upstream: the [`Tube`] of `curve` over `interval`, `None` when the piece is not
/// enclosed. `tol` is the distance the caller compares the tube with.
///
/// A curve with certified control points ([`Curve::piece_hull_points`]) lies in their convex
/// hull, whose first and last points are its ends. Distance from a segment is convex, so every
/// point of the piece lies within `rho` of the chord between them, `rho` the largest distance
/// of a control point from it ([`segment_distance_bound`]), each point within twice its
/// coordinate error of the exact one. The piece runs from one end of the chord to the other, so
/// its projection onto the chord's line covers the chord: every point of the chord is within
/// `rho` of the piece too.
///
/// A curve on a surface `S(W(t))` (its UV curve `W` and its surface `S`) has no control points
/// of its own. Its chord is taken as the line `L` of degree 1 over `interval` through its two
/// end points as evaluated, and `rho` is the proved bound on the distance between `L` and the
/// image `S(W)` over `interval` both ways ([`trim_image_gap_bound_within`]: the surface's exact
/// net and the UV curve's NURBS data in rounded interval arithmetic). The chord is `L` itself,
/// so its ends are exact (`ea = eb = 0`).
///
/// A UV line's evaluation `P + s scale V` rounds, so its points are not those of its NURB, and
/// the enclosure takes only an exactly evaluated trim. A UV line along a parameter direction
/// (one coordinate of `V` nonzero) is taken all the same: its other coordinate is `P`'s
/// exactly at every parameter (`P_c + x 0 = P_c` for a finite `x`), and its coordinate along
/// `V` is monotone in the parameter (each rounded operation is), so its evaluated points run
/// along the segment between its two evaluated ends and never leave it. Only how fast they
/// run along it differs from the segment's own parameter, which the enclosure does not depend
/// on (it bounds the distance of the images as sets, both ways). So the piece's UV curve is
/// that segment, the line of degree 1 between the evaluated ends over `interval`: it is the
/// trim enclosed.
///
/// A surface without an exact net, a UV curve of another kind than a B-spline, an ellipse or
/// such a line, or a gap that the bisection does not enclose within its budget gives `None`;
/// a gap above `tol` gives an infinite `rho`.
///
/// [`trim_image_gap_bound_within`]: crate::query::curve_surface_gap::trim_image_gap_bound_within
///
/// # Errors
///
/// Those of the end evaluations, of the surface's copy and of the enclosure.
fn piece_tube(k: &Kernel, curve: &Curve, interval: &Extent1d, tol: f64) -> Result<Option<Tube>> {
    if let Some(points) = curve.piece_hull_points(interval) {
        let (Some(&(a, ea)), Some(&(b, eb))) = (points.first(), points.last()) else {
            return Ok(None);
        };
        let e_ends = ea.max(eb);
        // NaN propagates (a NaN distance proves nothing)
        let rho = points.iter().fold(0.0_f64, |m, &(q, eq)| {
            let d = segment_distance_bound(q, a, b) + 2.0 * (eq + e_ends);
            if d.is_nan() || d > m {
                d
            } else {
                m
            }
        });
        return Ok(Some(Tube { a, ea, b, eb, rho }));
    }
    let CurveKind::CrvOnSurf(cos) = curve.kind() else {
        return Ok(None);
    };
    let (t0, t1) = (interval.min(), interval.max());
    if !(t0 < t1 && t0.is_finite() && t1.is_finite()) {
        return Ok(None);
    }
    let (a, b) = (curve.evaluate_point(k, t0)?, curve.evaluate_point(k, t1)?);
    let Some(chord) = chord_line(a, b, t0, t1, 3) else {
        return Ok(None);
    };
    // a UV line along a parameter direction: its points as evaluated lie on the segment
    // between its two evaluated ends (see the docs above); any other UV line is refused below
    let iso;
    let uv = match cos.uv_curve().kind() {
        CurveKind::Line(l) => {
            let v = l.line_vector();
            let along_u = v.x != 0.0 && v.y == 0.0 && v.z == 0.0;
            let along_v = v.x == 0.0 && v.y != 0.0 && v.z == 0.0;
            if !(along_u || along_v) {
                return Ok(None);
            }
            let (w0, w1) = (
                cos.uv_curve().evaluate_point(k, t0)?,
                cos.uv_curve().evaluate_point(k, t1)?,
            );
            let Some(line) = chord_line(w0, w1, t0, t1, 2) else {
                return Ok(None);
            };
            iso = line;
            &iso
        }
        _ => cos.uv_curve(),
    };
    // (a copy without owner, as the drops use: its evaluations do not trim to a face)
    let mut surface = cos.surface().clone_for_evaluation()?;
    let _ = surface.set_owner(None);
    // (below a thousandth of the tolerance a tighter bound changes nothing)
    let floor = 1.0e-3 * tol;
    let rho = crate::query::curve_surface_gap::trim_image_gap_bound_within(
        k,
        &surface,
        &chord,
        uv,
        [t0, t1],
        floor,
        [floor, tol],
        Some(CRV_ON_SURF_TUBE_PIECES),
    )?;
    Ok(rho.map(|rho| Tube {
        a,
        ea: 0.0,
        b,
        eb: 0.0,
        rho,
    }))
}

/// The line of degree 1 from `a` at `t0` to `b` at `t1` (`t0 < t1`), as a B-spline of
/// dimension `dim` whose control points are `a` and `b` exactly; `None` when it cannot be
/// built.
fn chord_line(a: Point3, b: Point3, t0: f64, t1: f64, dim: u64) -> Option<Curve> {
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};
    let pw = vec![
        CPoint::new(a.x, a.y, a.z, NOW),
        CPoint::new(b.x, b.y, b.z, NOW),
    ];
    let nurb = NurbCurve::new(pw, 1, vec![t0, t0, t1, t1]).ok()?;
    Some(Curve::from(BSplineCurve::from_nurb(nurb, dim, false)))
}

/// Not upstream: a bound `d` such that every point of `curve` over `interval` is within `d`
/// of the piece of `other` between `other_params` and every point of that piece within `d`
/// of the curve over `interval`, the first of `other_params` paired with `interval`'s start.
///
/// Each piece lies within its `rho` of its chord and its chord within `rho` of it
/// ([`piece_tube`]). The point of one chord at fraction `s` lies within `max(|a - a'|,
/// |b - b'|)` of the point at `s` of the other (the distance is convex in `s`), each end gap
/// widened by twice the ends' errors and for its rounding ([`segment_distance_bound`]). So
/// every point of either piece is within `rho_c + max gap + rho_e` of the other, widened by
/// `gamma(3)` for the sum. `None` when either piece has no [`Tube`]; NaN propagates. `tol` is
/// the distance the caller compares the bound with ([`piece_tube`]).
///
/// # Errors
///
/// Those of [`piece_tube`].
fn pieces_apart_bound(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    other: &Curve,
    other_params: [f64; 2],
    tol: f64,
) -> Result<Option<f64>> {
    let [s0, s1] = other_params;
    let Some(c) = piece_tube(k, curve, interval, tol)? else {
        return Ok(None);
    };
    let Some(e) = piece_tube(
        k,
        other,
        &Extent1d::from_min_max(s0.min(s1), s0.max(s1)),
        tol,
    )?
    else {
        return Ok(None);
    };
    // the other piece's end at the curve's start first
    let e = if s0 <= s1 { e } else { e.reversed() };
    let gap_a = segment_distance_bound(c.a, e.a, e.a) + 2.0 * (c.ea + e.ea);
    let gap_b = segment_distance_bound(c.b, e.b, e.b) + 2.0 * (c.eb + e.eb);
    let ends = if gap_a.is_nan() || gap_a > gap_b {
        gap_a
    } else {
        gap_b
    };
    Ok(Some((c.rho + ends + e.rho) * (1.0 + gamma(3))))
}

/// Pieces [`curve_runs_along`] bounds before it refuses.
const RUN_ALONG_PIECES: usize = 1024;

/// Not upstream (WR-580): whether `curve` over `interval` and `other` over `other_interval`
/// lie within `tol` of each other as wholes, each point of either within `tol` of a point of
/// the other (their Hausdorff distance); decided on enclosures, never on samples.
///
/// The two are cut into pieces paired end to end, the curve's start with the end of `other`
/// whose point is nearer to it; a pair is proved by [`pieces_apart_bound`] (each piece within
/// its enclosure's distance from its chord, the chords within their end gaps). A pair
/// whose bound is not below `tol` is halved: the curve at its middle parameter, `other` at the
/// drop of the curve's middle point onto its piece. The drop only chooses where `other` is
/// cut; any cut gives pieces that cover both curves, so the bound holds whatever it returns.
/// Every pair proved, the curves are within `tol`. A pair that cannot be cut any further, a
/// failed drop, a curve of a kind with no enclosure ([`piece_tube`]), or more than [`RUN_ALONG_PIECES`]
/// pairs bounded proves nothing: `false`.
///
/// A pair the enclosures do not prove is also accepted when both are circle arcs in one plane
/// whose NURBs [`circle_arcs_within`] proves within `tol` plus the coordinates' resolution
/// [`coordinate_resolution`] (`√3 ulp` of their magnitude, `7.7e-16` near `2.4`): so `true`
/// means each point of either within `tol` of the other, or, for such circle pairs only,
/// within `tol + √3 ulp`, an acceptance allowance of about one ulp for the coordinates'
/// rounding (#18065).
///
/// # Errors
///
/// Those of the evaluations and of the drops.
pub(crate) fn curve_runs_along(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    other: &Curve,
    other_interval: &Extent1d,
    tol: f64,
) -> Result<bool> {
    let (t0, t1) = (interval.min(), interval.max());
    let (u0, u1) = (other_interval.min(), other_interval.max());
    let start = curve.evaluate_point(k, t0)?;
    let reversed =
        start.distance(other.evaluate_point(k, u1)?) < start.distance(other.evaluate_point(k, u0)?);
    let (s0, s1) = if reversed { (u1, u0) } else { (u0, u1) };
    let mut pending = vec![(t0, t1, s0, s1)];
    let mut bounded = 0_usize;
    while let Some((t0, t1, s0, s1)) = pending.pop() {
        let piece = Extent1d::from_min_max(t0, t1);
        // written so that a NaN bound refuses; two coplanar circles' arcs are proved in closed
        // form when their chords' enclosures are not tight enough
        if pieces_apart_bound(k, curve, &piece, other, [s0, s1], tol)?.is_some_and(|d| d < tol)
            || circle_arcs_within(curve, &piece, other, [s0, s1], tol)
        {
            continue;
        }
        bounded += 1;
        let tm = 0.5 * (t0 + t1);
        if bounded > RUN_ALONG_PIECES || !(t0 < tm && tm < t1) {
            return Ok(false);
        }
        let mid = curve.evaluate_point(k, tm)?;
        let (lo, hi) = (s0.min(s1), s0.max(s1));
        let dropped = other.drop_point(
            k,
            &Extent1d::from_min_max(lo, hi),
            mid,
            None,
            tol,
            None,
            SolverOperation::Minimize,
        )?;
        let sm = dropped.parameter;
        if !(dropped.success && lo < sm && sm < hi) {
            return Ok(false);
        }
        pending.push((tm, t1, sm, s1));
        pending.push((t0, tm, s0, sm));
    }
    Ok(true)
}

/// Not upstream (#18065): the distance to which two points whose coordinates are at most `m` in
/// magnitude are fixed by their doubles, `√3 ulp(m)`.
///
/// **Derivation.** A double stands for any real within half a unit in its last place, and the
/// unit of a smaller magnitude is not larger, so each coordinate of either point is fixed only
/// to `ulp(m) / 2`: each point to a cube of half side `ulp(m) / 2`, at most `√3 ulp(m) / 2` from
/// its centre, and the distance between the two points to `√3 ulp(m)`. So a modelled distance
/// that is above the band by less than this is above it only by what the coordinates' own
/// rounding can produce (the #18065 boss: a radius stored `4.2e-16` long). It is an acceptance
/// allowance of about one ulp, fixed by the magnitude alone and the same for every input of
/// it, not a tolerance a caller widens; raising the edge's tolerance instead
/// ([`EdgeToleranceRaise`]) would change the model for a gap no measurement can resolve. Here
/// `ulp(m)` is `next_up(m) - m` (exact: two doubles within a factor of two of each other), a
/// power of two; the double nearest `√3` is below `√3` and its product with a normal power of
/// two exact, so the value never exceeds `√3 ulp(m)`. The allowance covers the representation
/// of the stored coordinates only and is spent once, in the gap's final decision; every other
/// error (the NURBs' deviation from their circles, [`radial_deviation`], and each rounding of
/// the measurement) is counted on its own. `None` for a magnitude that is not finite and
/// positive or whose unit is not normal.
fn coordinate_resolution(m: f64) -> Option<f64> {
    if !(m.is_finite() && m > 0.0) {
        return None;
    }
    let ulp = next_up(m) - m;
    (ulp.is_normal() && (ulp * 2.0).is_finite()).then_some(ulp * SQRT_3)
}

/// A circle the curve is: an ellipse whose two radii are one finite, positive double, with its
/// centre and the two axis vectors of its plane, all finite.
fn circle_of(curve: &Curve) -> Option<(Point3, f64, Vec3, Vec3)> {
    let CurveKind::Ellipse(e) = curve.kind() else {
        return None;
    };
    let r = e.x_radius();
    let p = e.position();
    let (c, x, y) = (p.origin(), p.x_axis(), p.y_axis());
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    (r == e.y_radius() && r.is_finite() && r > 0.0 && finite(c) && finite(x) && finite(y))
        .then_some((c, r, x, y))
}

/// The double nearest `√3` (below it); `f64::consts::SQRT_3` is not stable at the MSRV.
const SQRT_3: f64 = 1.732_050_807_568_877_2;

/// The unit roundoff `2^-53`.
const ROUNDOFF: f64 = f64::EPSILON / 2.0;

/// An angle and radius of `p - c` measured in a circle plane's frame ([`PlaneFrame::polar`]),
/// with a bound of the angle's error.
#[derive(Debug, Clone, Copy)]
struct Polar {
    angle: f64,
    radius: f64,
    angle_err: f64,
}

/// The two axis vectors `x`, `y` of a circle's plane, as the frame angles in the plane are
/// measured in: the angle of `v` is `atan2(v · y, v · x)`.
#[derive(Debug, Clone, Copy)]
struct PlaneFrame {
    x: Vec3,
    y: Vec3,
    /// The larger 1-norm of `x` and `y`.
    l1: f64,
    /// A bound of how far `x`, `y` are from orthonormal: `|x·y| + |x·x - 1| + |y·y - 1|`
    /// with its rounding.
    skew: f64,
}

impl PlaneFrame {
    /// The frame of `x`, `y`; `None` when they are farther than `2^-30` from orthonormal.
    fn new(x: Vec3, y: Vec3) -> Option<Self> {
        let n1 = |v: Vec3| v.x.abs() + v.y.abs() + v.z.abs();
        let l1 = n1(x).max(n1(y));
        let skew = x.dot(y).abs()
            + (x.dot(x) - 1.0).abs()
            + (y.dot(y) - 1.0).abs()
            + 32.0 * ROUNDOFF * (l1 * l1 + 1.0);
        (skew <= 1.0 / 1_073_741_824.0).then_some(Self { x, y, l1, skew })
    }

    /// The angle and radius of the projection of `p - c` onto the plane, in a frame of
    /// orthonormal axes in the plane whose first is along `x` (the projection is exact: a
    /// component normal to the plane is dropped by both products).
    ///
    /// Errors: `p - c` is rounded by at most `u |v|∞` per coordinate (`u` the unit roundoff)
    /// and each product with an axis by `γ3 |v|∞ ‖axis‖1`, so the two coordinates move by at
    /// most `ε = 8 u |v|∞ l1` together. At a measured radius `r > 4 ε` the true radius is above
    /// `r / 2`, and the angle moves by at most `asin(2 ε / r) ≤ 3 ε / r`, plus
    /// [`ATAN2_ERR`] for the angle's own evaluation ([`atan2_enclosed`], basic operations
    /// only: no claim on the platform's libm, whose `atan2` accuracy Rust does not specify). The frame's skew
    /// `g` maps a vector of the orthonormal frame to the measured coordinates within `3 g` of
    /// its length, moving its angle by at most `4 g`. `None` when the radius is not above
    /// `4 ε` or a value is not finite.
    fn polar(&self, p: Point3, c: Point3) -> Option<Polar> {
        let v = p - c;
        let (x, y) = (v.dot(self.x), v.dot(self.y));
        let size = v.x.abs().max(v.y.abs()).max(v.z.abs());
        let eps = 8.0 * ROUNDOFF * size * self.l1;
        let radius = (x * x + y * y).sqrt();
        let angle = atan2_enclosed(y, x)?;
        if !(radius > 4.0 * eps && radius.is_finite() && angle.is_finite()) {
            return None;
        }
        Some(Polar {
            angle,
            radius,
            angle_err: 3.0 * eps / radius + ATAN2_ERR + 4.0 * self.skew,
        })
    }
}

/// The bound [`atan2_enclosed`] is within: `64 u`.
const ATAN2_ERR: f64 = 64.0 * ROUNDOFF;

/// `tan(π/8)` (rounded): [`atan2_enclosed`] reduces a ratio above it about `π/4`.
const TAN_PI_8: f64 = 0.414_213_562_373_095_03;

/// Not upstream (#18065): `atan2(y, x)` within [`ATAN2_ERR`] (`64 u`) of the true angle, in
/// `[-π, π]` with `atan2`'s signs, computed from basic operations only (each correctly
/// rounded, IEEE 754), so the bound does not rest on the platform's libm, whose accuracy
/// Rust does not specify. `None` when either is not finite or both are zero.
///
/// **Method.** With `z = min / max` of `|x|`, `|y|` in `[0, 1]`, `atan z` is
/// `π/4 + atan w`, `w = (z - 1) / (z + 1)`, when `z > tan(π/8)`, else `atan w`, `w = z`; so
/// `|w| ≤ tan(π/8) < 0.4143`. `atan w` is its Taylor series `Σ (-1)^k w^(2k+1) / (2k+1)`
/// to `k = 21` (Horner in `w²`); then `π/2 -` for `|y| > |x|`, `π -` for `x < 0`, and `y`'s
/// sign.
///
/// **Bound** (`u` the unit roundoff; atan is 1-Lipschitz): the ratio `z` rounded, `u`
/// (plus `2^-1074` should it be subnormal); `w` in three roundings, `γ3 |w| ≤ 1.25 u`; the
/// series' remainder, alternating and decreasing, below its first omitted term
/// `0.4143^45 / 45 < 0.01 u`; Horner over 22 coefficients (each rounded) at `w² ≤ 0.1717`,
/// `γ_46 Σ|c_k| w^2k ≤ 46.1 u · 1.21` plus `0.25 u` for `w²`'s own rounding, times
/// `|w|` and that product's rounding, `≤ 24 u`; `π/4`, `π/2`, `π` as doubles, `0.5 u`,
/// `u`, `2 u`; the three additions, each below `u` times its result (at most `3π/8`,
/// `π/2`, `π`), `≤ 5.9 u`. In all below `37 u`, counted as `64 u`.
fn atan2_enclosed(y: f64, x: f64) -> Option<f64> {
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    if !(x.is_finite() && y.is_finite()) || (x == 0.0 && y == 0.0) {
        return None;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let swap = ay > ax;
    let z = if swap { ax / ay } else { ay / ax };
    let (base, w) = if z > TAN_PI_8 {
        (FRAC_PI_4, (z - 1.0) / (z + 1.0))
    } else {
        (0.0, z)
    };
    // the bound's premise, checked (it holds for every `z` in `[0, 1]`; a NaN refuses)
    if w.abs().partial_cmp(&0.4143) != Some(core::cmp::Ordering::Less) {
        return None;
    }
    let w2 = w * w;
    let mut p = 0.0_f64;
    for k in (0..22_u32).rev() {
        let c = 1.0 / f64::from(2 * k + 1);
        p = p * w2 + if k % 2 == 0 { c } else { -c };
    }
    let a = base + w * p;
    let a = if swap { FRAC_PI_2 - a } else { a };
    let a = if x < 0.0 { PI - a } else { a };
    Some(if y.is_sign_negative() { -a } else { a })
}

/// Not upstream (#18065): a lower bound of `cos x` for `0 ≤ x < π/2`, from basic operations
/// only (not the platform's libm). `None` outside that range or for a NaN.
///
/// **Bound.** `T(x) = Σ_(k=0..11) (-1)^k x^(2k) / (2k)!` ends on a negative term, so
/// `cos x - T(x) = cos ξ x^24 / 24!` (Lagrange, `ξ` in `[0, x]`) is not negative: `T ≤ cos`.
/// The factorials are exact (each `n!`, `n ≤ 22`, is a double: its odd part is below
/// `2^53`), their reciprocals rounded once. `T = 1 - S`, `S = x² (c_1 - x² (c_2 - …))` by
/// Horner: `S`'s error is below `γ_22 + 11 u + u` (Horner, `x²`'s rounding through the
/// degree, the last product) times `Σ_(k≥1) x^(2k)/(2k)! = cosh x - 1 ≤ x² cosh(π/2) / 2
/// ≤ 1.26 x²`, so below `43 u x²`; `1 - S` and the final subtraction round once each, below
/// `u` (results at most 1). The pad `u (3 + 48 x²)` covers all of it (its own two roundings
/// within its slack).
fn cos_lower(x: f64) -> Option<f64> {
    use core::f64::consts::FRAC_PI_2;
    if !(0.0..FRAC_PI_2).contains(&x) {
        return None;
    }
    let x2 = x * x;
    let mut p = 0.0_f64;
    let mut factorial = [1.0_f64; 12];
    let mut f = 1.0_f64;
    for (k, slot) in factorial.iter_mut().enumerate().skip(1) {
        let k = k as f64;
        f = f * (2.0 * k - 1.0) * (2.0 * k);
        *slot = f;
    }
    for (k, &f) in factorial.iter().enumerate().skip(1).rev() {
        let c = 1.0 / f;
        p = if k % 2 == 1 { p * x2 + c } else { p * x2 - c };
    }
    let one_minus_s = 1.0 - x2 * p;
    Some((one_minus_s - ROUNDOFF * (3.0 + 48.0 * x2)).max(0.0))
}

/// An angle difference in `[-2π, 2π]` wrapped to `(-π, π]`.
fn wrap_angle(d: f64) -> f64 {
    use core::f64::consts::{PI, TAU};
    if d > PI {
        d - TAU
    } else if d <= -PI {
        d + TAU
    } else {
        d
    }
}

/// `e f` as an expansion.
fn expansion_mul(e: &[f64], f: &[f64]) -> Option<Vec<f64>> {
    f.iter().try_fold(vec![0.0], |acc, &c| {
        Some(crate::geom::expansion::add(
            &acc,
            &crate::geom::expansion::scale(e, c)?,
        ))
    })
}

/// An expansion's value bounded below and above by doubles: its rounded sum widened by the
/// sum's error bound, `2 n u Σ|e|` for `n` components (above `γ_(n-1) Σ|e|`,
/// [`crate::geom::expansion::to_f64`]). `None` when not finite.
fn expansion_bounds(e: &[f64]) -> Option<(f64, f64)> {
    let v = crate::geom::expansion::to_f64(e);
    let err = 2.0 * e.len() as f64 * ROUNDOFF * e.iter().map(|c| c.abs()).sum::<f64>();
    let (lo, hi) = (next_down(v - err), next_up(v + err));
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// The range of the Bernstein basis function `B_k^n` (`n` 2 or 4) over `[a, b] ⊆ [0, 1]`: it
/// is unimodal with its maximum at `k / n`, so its extremes are at the ends and, inside,
/// there; each value, a product of at most five rounded factors and one rounded difference,
/// is within `16 u` of itself as computed.
fn bernstein_range(n: usize, k: usize, [a, b]: [f64; 2]) -> (f64, f64) {
    let binom: &[f64] = if n == 2 {
        &[1.0, 2.0, 1.0]
    } else {
        &[1.0, 4.0, 6.0, 4.0, 1.0]
    };
    let c = binom.get(k).copied().unwrap_or(0.0);
    let f = n.saturating_sub(k);
    // plain products (`powi`'s accuracy is not specified)
    let at = |t: f64| {
        let s = 1.0 - t;
        let v = (0..k).fold(c, |v, _| v * t);
        (0..f).fold(v, |v, _| v * s)
    };
    let peak = k as f64 / n as f64;
    let (lo, mut hi) = (at(a).min(at(b)), at(a).max(at(b)));
    if a < peak && peak < b {
        hi = hi.max(at(peak));
    }
    (
        (lo * (1.0 - 16.0 * ROUNDOFF)).max(0.0),
        hi * (1.0 + 16.0 * ROUNDOFF),
    )
}

/// Not upstream (#18065): bounds `(inward, outward)`, both at least `0`, of how far the curve's
/// NURB over `range` lies inside and outside the circle of centre `c` and radius `r`: every
/// point `C` of it has `r - inward ≤ |C - c| ≤ r + outward`. Decided on the NURB's stored
/// doubles, not on evaluations: the NURB must be rational quadratic with positive weights,
/// clamped, each interior knot doubled (Bézier segments, as a circle's NURB is), and its
/// control points exactly in the plane through `c` spanned by `px`, `py` (so is the NURB,
/// their positive combination), else `None`.
///
/// On a segment with homogeneous control points `H_i` and weights `w_i`,
/// `|C - c|² - r² = N(τ) / W(τ)²` with `W = Σ w_i B_i^2 ≥ w_min` and the quartic
/// `N = Σ_(i,j) B_i^2 B_j^2 (Q_i·Q_j - r² w_i w_j)`, `Q_i = H_i - c w_i`; `B_i^2 B_j^2 =
/// C(2,i) C(2,j) / C(4,i+j) B_(i+j)^4`, so `6 N`'s Bernstein coefficients are integer sums
/// of those terms, computed exactly on expansions. Over the segment's part of `range` (its
/// local parameter widened by its rounding) `N` lies between the sums of each coefficient
/// times its basis function's range ([`bernstein_range`]), `W` likewise (its weights positive),
/// and `|C - c|² - r²` is that over `W²` (by `W`'s lower or upper bound as the sign needs).
/// Outward, `|C - c| - r` is that over `|C - c| + r ≥ 2 r`; inward, `r - |C - c| = d` has
/// `r² - |C - c|² = d (2 r - d) ≥ d r` (as `d ≤ r`), so `d` is at most that over `r`.
fn radial_deviation(
    curve: &Curve,
    (c, r): (Point3, f64),
    (px, py): (Vec3, Vec3),
    [lo, hi]: [f64; 2],
) -> Option<(f64, f64)> {
    use crate::geom::expansion::{add, det3, is_zero, product, scale, sub};
    use crate::nurbs::math::{NOW, NOZ};
    let nurb = match curve.kind() {
        CurveKind::Ellipse(e) => e.base().nurb_ref().ok()?,
        _ => return None,
    };
    let (knots, cpts) = (nurb.knots(), nurb.cpts());
    let n = cpts.len();
    let at = |i: usize| knots.get(i).copied();
    let clamped = n >= 3
        && n % 2 == 1
        && knots.len() == n + 3
        && at(0) == at(2)
        && at(1) == at(2)
        && at(n) == at(n + 2)
        && at(n + 1) == at(n + 2);
    if nurb.degree() != 2
        || !clamped
        || lo
            .partial_cmp(&hi)
            .is_none_or(|o| o == core::cmp::Ordering::Greater)
    {
        return None;
    }
    // homogeneous points and weights; a non-rational point has weight 1, a 2D one `z = 0`;
    // each point in the plane through `c` of axes `px`, `py`, exactly: `n·H_i = w_i n·c`
    let plane_c = det3(px, py, c)?;
    let mut hw = Vec::new();
    for p in cpts {
        let w = if p.w == NOW { 1.0 } else { p.w };
        let z = if p.z == NOZ { 0.0 } else { p.z };
        if !(w.is_finite() && w > 0.0 && p.x.is_finite() && p.y.is_finite() && z.is_finite()) {
            return None;
        }
        if !is_zero(&sub(
            &det3(px, py, Vec3::new(p.x, p.y, z))?,
            &scale(&plane_c, w)?,
        )) {
            return None;
        }
        hw.push(([p.x, p.y, z], w));
    }
    let r2 = product(r, r)?;
    let (mut inward, mut outward) = (0.0_f64, 0.0_f64);
    for j in 0..(n - 1) / 2 {
        let (Some(ka), Some(kb)) = (at(2 * j + 2), at(2 * j + 3)) else {
            return None;
        };
        if j + 1 < (n - 1) / 2 && at(2 * j + 3) != at(2 * j + 4) {
            return None;
        }
        if ka.partial_cmp(&kb) != Some(core::cmp::Ordering::Less) {
            return None;
        }
        if hi < ka || kb < lo {
            continue;
        }
        let seg = hw.get(2 * j..2 * j + 3)?;
        // `Q_i = H_i - c w_i`, exactly (a non-rational point's `H_i` is its point, `w_i = 1`)
        let mut q = Vec::new();
        for &(h, w) in seg {
            let mut qi = Vec::new();
            for (hc, cc) in h.into_iter().zip([c.x, c.y, c.z]) {
                qi.push(sub(&[hc], &product(cc, w)?));
            }
            q.push((qi, w));
        }
        let term = |i: usize, k: usize| -> Option<Vec<f64>> {
            let ((qi, wi), (qk, wk)) = (q.get(i)?, q.get(k)?);
            let mut d = vec![0.0];
            for (a, b) in qi.iter().zip(qk) {
                d = add(&d, &expansion_mul(a, b)?);
            }
            Some(sub(&d, &scale(&scale(&r2, *wi)?, *wk)?))
        };
        // `6 N`'s Bernstein coefficients
        let coeff = [
            scale(&term(0, 0)?, 6.0)?,
            scale(&add(&term(0, 1)?, &term(1, 0)?), 3.0)?,
            add(&add(&term(0, 2)?, &term(2, 0)?), &scale(&term(1, 1)?, 4.0)?),
            scale(&add(&term(1, 2)?, &term(2, 1)?), 3.0)?,
            scale(&term(2, 2)?, 6.0)?,
        ];
        // the segment's local parameters of the range, widened by their rounding
        let len = kb - ka;
        let local = |t: f64| (t.clamp(ka, kb) - ka) / len;
        let slack = 8.0 * ROUNDOFF * (ka.abs().max(kb.abs()) / len + 1.0);
        let span = [(local(lo) - slack).max(0.0), (local(hi) + slack).min(1.0)];
        let (mut n_lo, mut n_hi, mut mag) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (k, e) in coeff.iter().enumerate() {
            let (c_lo, c_hi) = expansion_bounds(e)?;
            let (b_lo, b_hi) = bernstein_range(4, k, span);
            n_hi += if c_hi >= 0.0 {
                c_hi * b_hi
            } else {
                c_hi * b_lo
            };
            n_lo += if c_lo >= 0.0 {
                c_lo * b_lo
            } else {
                c_lo * b_hi
            };
            mag += c_lo.abs().max(c_hi.abs()) * b_hi;
        }
        // `/ 6` and the sums' rounding: each of the five products rounded once and summed in
        // four roundings, so each sum is within `8 u` of the sum of the magnitudes `mag`
        // (itself within `8 u` of its value), padded by `16 u mag`
        let pad = 16.0 * ROUNDOFF * mag;
        let (n_lo, n_hi) = ((n_lo - pad) / 6.0, (n_hi + pad) / 6.0);
        // `W` over the span, from below and above (positive terms, each within `4 u`)
        let (mut wmin, mut wmax) = (0.0_f64, 0.0_f64);
        for (i, &(_, w)) in seg.iter().enumerate() {
            let (b_lo, b_hi) = bernstein_range(2, i, span);
            wmin += w * b_lo;
            wmax += w * b_hi;
        }
        let (wmin, wmax) = (wmin * (1.0 - 8.0 * ROUNDOFF), wmax * (1.0 + 8.0 * ROUNDOFF));
        if wmin.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
            return None;
        }
        // `W²`'s bounds, each within `u` of itself only when normal: an overflow (weights
        // near `1e155`) would make a quotient `0`, an underflow lose its relative bound. (A
        // quotient that itself underflows is off by at most `2^-1075`, far inside the `8 u`
        // pads on the radii in [`circle_arcs_within`].)
        let (wmin2, wmax2) = (wmin * wmin, wmax * wmax);
        if !(wmin2.is_normal() && wmax2.is_normal()) {
            return None;
        }
        let up = if n_hi >= 0.0 {
            n_hi / wmin2
        } else {
            n_hi / wmax2
        };
        let down = if n_lo >= 0.0 {
            n_lo / wmax2
        } else {
            n_lo / wmin2
        };
        if !(up.is_finite() && down.is_finite()) {
            return None;
        }
        outward = outward.max(up / (2.0 * r) * (1.0 + 8.0 * ROUNDOFF));
        inward = inward.max(-down / r * (1.0 + 8.0 * ROUNDOFF));
    }
    (inward.is_finite() && outward.is_finite()).then_some((inward, outward))
}

/// The angles about `centre`, measured in `frame`, of points each within its slack of a
/// vertex of a convex hull, lifted against `reference`; with their largest error.
fn hull_angles(
    frame: &PlaneFrame,
    points: &[(Point3, f64)],
    centre: Point3,
    reference: f64,
) -> Option<(Vec<f64>, f64)> {
    let mut angles = Vec::new();
    let mut err = 0.0_f64;
    for &(p, slack) in points {
        let polar = frame.polar(p, centre)?;
        // a point within `slack` of `p` is at an angle within `2 slack / r` of it
        if !(polar.radius > 4.0 * slack && slack.is_finite()) {
            return None;
        }
        err = err.max(polar.angle_err + 2.0 * slack / polar.radius);
        angles.push(wrap_angle(polar.angle - reference));
    }
    Some((angles, err))
}

/// Not upstream (#18065): whether the piece of the circle `curve` over `piece` and the piece
/// of the circle `other` between `other_params` lie within the band `tol` of each other as
/// wholes (their Hausdorff distance), the gap decided in closed form against `tol` plus the
/// coordinates' resolution ([`coordinate_resolution`]). `false` (nothing proved) unless both
/// curves are circles ([`circle_of`]) in exactly one plane (decided on exact expansions)
/// whose NURBs [`radial_deviation`] bounds.
///
/// **What is proved is the NURBs.** The pieces are the curves' NURBs as evaluated, not the
/// circles: each point `C` of the first lies in the annulus `ra - in_A ≤ |C - a0| ≤ ra + out_A`
/// about circle `A` (centre `a0`, radius `ra`), in the circles' plane (its control points
/// exactly in it, [`radial_deviation`]), and in the convex hull of its piece's control
/// points (positive weights; [`Curve::piece_hull_points`], each within its slack), and each
/// point of the second likewise about `E` (centre `e0`, radius `re`). Polar coordinates
/// `(ρ, φ)` are taken about `e0`.
///
/// **Angles.** Every hull vertex of both pieces is measured about `e0` and lifted against the
/// first one's angle; when they all lie within less than `π`, both hulls lie in one cone of
/// apex `e0` and the lifts are consistent. Each piece's angles then lie within its hull's
/// angle range and, a continuous path between its two ends, cover the range between its
/// ends' angles. So every point of either piece has a point of the other at an angle within
/// `Δ`, the larger excess of either hull's range over the other's ends' range.
///
/// **Radii.** With `δ = a0 - e0`, the first piece lies in the disk of radius `ra + out_A`
/// about `a0`: `ρ ≤ |δ| + ra + out_A` (the apex, exact). From below, when every hull vertex
/// lies within less than `π/2` of `δ`'s direction about `a0`, so does the hull (a convex set
/// in an open half-plane), and a point at distance `s ≥ ra - in_A` from `a0` in such a
/// direction `w` has `ρ² = |δ|² + s² + 2 s δ·w ≥ |δ|² + (ra - in_A)² + 2 (ra - in_A) |δ|
/// cos γ`, `γ` the widest vertex angle from `δ`; with `δ = 0`, `ρ ≥ ra - in_A`. The second
/// piece has `re - in_E ≤ ρ ≤ re + out_E`.
///
/// **The gap.** A point of one piece and the point of the other at an angle within `Δ` are,
/// by the cosine law and `1 - cos x ≤ x²/2`, within `sqrt(R² + ρ_hi (re + out_E) Δ²)`, `R`
/// the larger of `ρ_hi - (re - in_E)` and `(re + out_E) - ρ_lo` (and `0`).
///
/// **The decision.** `T = tol + A`, `A` the resolution of the larger coordinate magnitude of
/// the two circles (`‖c‖∞ + r`). With `x ≥ ρ_hi (re + out_E) Δ²` and `y ≥ x / tol ≥ x / T`,
/// `y < tol`, `R ≤ T - y` gives `R² ≤ (T - x/T)² ≤ T² - x` (as `x ≤ T²`), so `H ≤ T`. The
/// apex term is decided exactly, `|δ|² ≤ (re - in_E - ra - out_A + T - y)²` on expansions,
/// the lower one on a rigorous bound of `ρ_lo`. Every rounding of the measured angles and
/// radii is counted ([`PlaneFrame::polar`]); a value that cannot be measured, a NaN or a
/// non-finite value refuses. Bounded work: two hulls of the pieces and fixed-size expansions
/// per NURB segment.
///
/// An evaluation that cannot be bounded proves nothing (`false`).
fn circle_arcs_within(
    curve: &Curve,
    piece: &Extent1d,
    other: &Curve,
    [s0, s1]: [f64; 2],
    tol: f64,
) -> bool {
    use crate::geom::expansion::{add, cross, det3, is_zero, sign, sub};
    use core::f64::consts::{FRAC_PI_2, PI};
    let (Some((a0, ra, xa, ya)), Some((e0, re, xe, ye))) = (circle_of(curve), circle_of(other))
    else {
        return false;
    };
    if !(tol > 0.0 && tol.is_finite()) {
        return false;
    }
    // one plane, exactly: `E`'s axes span it (their cross product is not zero), `A`'s axes and
    // the offset of `A`'s centre from `E`'s lie in it
    let in_plane = |v: Vec3| det3(xe, ye, v).is_some_and(|d| is_zero(&d));
    let spans = cross(xe, ye).is_some_and(|c| c.iter().any(|e| !is_zero(e)));
    let offset_in_plane = match (det3(xe, ye, a0), det3(xe, ye, e0)) {
        (Some(p), Some(q)) => is_zero(&sub(&p, &q)),
        _ => false,
    };
    let Some(frame) = PlaneFrame::new(xe, ye) else {
        return false;
    };
    if !(spans && in_plane(xa) && in_plane(ya) && offset_in_plane) {
        return false;
    }
    let (t0, t1) = (piece.min(), piece.max());
    let (u0, u1) = (s0.min(s1), s0.max(s1));
    if !(t0 < t1 && u0 < u1) {
        return false;
    }
    // the NURBs' annuli about their circles
    let (Some((in_a, out_a)), Some((in_e, out_e))) = (
        radial_deviation(curve, (a0, ra), (xe, ye), [t0, t1]),
        radial_deviation(other, (e0, re), (xe, ye), [u0, u1]),
    ) else {
        return false;
    };
    // the pieces' hulls; their first and last points are the pieces' ends
    let (Some(hp), Some(hq)) = (
        curve.piece_hull_points(piece),
        other.piece_hull_points(&Extent1d::from_min_max(u0, u1)),
    ) else {
        return false;
    };
    let (Some(&(p_first, _)), Some(_), Some(_)) = (hp.first(), hp.last(), hq.last()) else {
        return false;
    };
    let Some(reference) = frame.polar(p_first, e0).map(|p| p.angle) else {
        return false;
    };
    let (Some((ap, err_p)), Some((aq, err_q))) = (
        hull_angles(&frame, &hp, e0, reference),
        hull_angles(&frame, &hq, e0, reference),
    ) else {
        return false;
    };
    let err = err_p.max(err_q);
    if err.partial_cmp(&(1.0 / 1_048_576.0)) != Some(core::cmp::Ordering::Less) {
        return false;
    }
    let range = |a: &[f64]| {
        a.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &x| {
                (l.min(x), h.max(x))
            })
    };
    let ends = |a: &[f64]| match (a.first(), a.last()) {
        (Some(&f), Some(&l)) => (f.min(l), f.max(l)),
        _ => (f64::NAN, f64::NAN),
    };
    let ((pl, ph), (ql, qh)) = (range(&ap), range(&aq));
    let ((pel, peh), (qel, qeh)) = (ends(&ap), ends(&aq));
    // one cone narrower than `π`: the lifts are consistent
    if !(pl.min(ql) > -PI && ph.max(qh) - pl.min(ql) < PI - 8.0 * err) {
        return false;
    }
    let excess = |(l, h): (f64, f64), (el, eh): (f64, f64)| (el - l).max(h - eh).max(0.0);
    let gap = excess((pl, ph), (qel, qeh)).max(excess((ql, qh), (pel, peh)));
    // each of the four angles in an excess within `err`, the differences rounded
    let delta = (gap + 2.0 * err + 64.0 * ROUNDOFF) * (1.0 + 4.0 * ROUNDOFF);
    if delta.partial_cmp(&FRAC_PI_2) != Some(core::cmp::Ordering::Less) {
        return false;
    }
    let size = |v: Vec3| v.x.abs().max(v.y.abs()).max(v.z.abs());
    let Some(allowance) = coordinate_resolution(next_up(size(a0) + ra).max(next_up(size(e0) + re)))
    else {
        return false;
    };
    let concentric = a0 == e0;
    let offset = a0 - e0;
    let offset_len = offset.length();
    // `ρ` over the first piece from below
    let s_lo = (ra - in_a) * (1.0 - 2.0 * ROUNDOFF);
    if s_lo.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return false;
    }
    let rho_lo = if concentric {
        s_lo
    } else {
        let Some(pd) = frame.polar(a0, e0) else {
            return false;
        };
        let mut widest = 0.0_f64;
        for &(p, slack) in &hp {
            let Some(polar) = frame.polar(p, a0) else {
                return false;
            };
            if polar.radius.partial_cmp(&(4.0 * slack)) != Some(core::cmp::Ordering::Greater) {
                return false;
            }
            let e = polar.angle_err + 2.0 * slack / polar.radius + pd.angle_err;
            // the difference (below `2π`) rounded, `6.3 u`; its wrap, `2π` as a double, `4 u`
            // (the subtraction is exact, Sterbenz); the two additions below `π/2`, `3.1 u`
            let gamma = wrap_angle(polar.angle - pd.angle).abs() + e + 16.0 * ROUNDOFF;
            if gamma.partial_cmp(&FRAC_PI_2) != Some(core::cmp::Ordering::Less) {
                return false;
            }
            widest = widest.max(gamma);
        }
        // `cos` bounded from below ([`cos_lower`], no libm) and decreasing on `[0, π/2]`;
        // `|δ|` within `4 u`
        let Some(cos_lo) = cos_lower(widest) else {
            return false;
        };
        let d_lo = offset_len * (1.0 - 4.0 * ROUNDOFF);
        let sq = (d_lo * d_lo + s_lo * s_lo + 2.0 * s_lo * d_lo * cos_lo) * (1.0 - 8.0 * ROUNDOFF);
        sq.sqrt() * (1.0 - 4.0 * ROUNDOFF)
    };
    let rho_hi = (offset_len + ra + out_a) * (1.0 + 8.0 * ROUNDOFF);
    let re_hi = (re + out_e) * (1.0 + 2.0 * ROUNDOFF);
    let x = rho_hi * re_hi * delta * delta * (1.0 + 8.0 * ROUNDOFF);
    let y = x / tol * (1.0 + 4.0 * ROUNDOFF);
    if !(y < tol && y.is_finite() && rho_lo.is_finite()) {
        return false;
    }
    // `T - y` exactly
    let band = add(&add(&[tol], &[allowance]), &[-y]);
    // apex: `|δ| ≤ re - in_E - ra - out_A + T - y`, both sides squared (the right one not
    // negative)
    let w = add(&sub(&sub(&sub(&[re], &[in_e]), &[ra]), &[out_a]), &band);
    let mut offset2 = vec![0.0];
    for (p, q) in [(a0.x, e0.x), (a0.y, e0.y), (a0.z, e0.z)] {
        let d = sub(&[p], &[q]);
        let Some(s) = expansion_mul(&d, &d) else {
            return false;
        };
        offset2 = add(&offset2, &s);
    }
    let Some(w2) = expansion_mul(&w, &w) else {
        return false;
    };
    let apex_within = sign(&w) >= 0 && sign(&sub(&w2, &offset2)) >= 0;
    // the other side: `re + out_E - ρ_lo ≤ T - y`
    let ends_within = sign(&sub(&sub(&add(&[re], &[out_e]), &[rho_lo]), &band)) <= 0;
    apex_within && ends_within
}

/// Not upstream (#18065): a certified upper bound of the distance between `curve` over
/// `interval` and `other` over `other_interval` as wholes (their Hausdorff distance), paired
/// and halved as in [`curve_runs_along`], the pair with the widest bound halved first, until
/// every pair is below `tol`, a pair cannot be cut, or [`RUN_ALONG_PIECES`] pairs are halved.
/// Every pair's bound holds whatever the cuts ([`pieces_apart_bound`]), so the widest of them
/// bounds the whole. `None` for a curve of a kind with no enclosure or a NaN bound.
///
/// # Errors
///
/// Those of the evaluations and of the drops.
fn run_along_gap_bound(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    other: &Curve,
    other_interval: &Extent1d,
    tol: f64,
) -> Result<Option<f64>> {
    let (t0, t1) = (interval.min(), interval.max());
    let (u0, u1) = (other_interval.min(), other_interval.max());
    let start = curve.evaluate_point(k, t0)?;
    let reversed =
        start.distance(other.evaluate_point(k, u1)?) < start.distance(other.evaluate_point(k, u0)?);
    let (s0, s1) = if reversed { (u1, u0) } else { (u0, u1) };
    let bound = |t0: f64, t1: f64, s0: f64, s1: f64| -> Result<Option<f64>> {
        let b = pieces_apart_bound(
            k,
            curve,
            &Extent1d::from_min_max(t0, t1),
            other,
            [s0, s1],
            tol,
        )?;
        Ok(b.filter(|b| !b.is_nan()))
    };
    let Some(first) = bound(t0, t1, s0, s1)? else {
        return Ok(None);
    };
    let mut pairs = vec![(first, t0, t1, s0, s1)];
    for _ in 0..RUN_ALONG_PIECES {
        let Some((i, &(widest, t0, t1, s0, s1))) = pairs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0))
        else {
            break;
        };
        let tm = 0.5 * (t0 + t1);
        if widest < tol || !(t0 < tm && tm < t1) {
            break;
        }
        let mid = curve.evaluate_point(k, tm)?;
        let (lo, hi) = (s0.min(s1), s0.max(s1));
        let dropped = other.drop_point(
            k,
            &Extent1d::from_min_max(lo, hi),
            mid,
            None,
            tol,
            None,
            SolverOperation::Minimize,
        )?;
        let sm = dropped.parameter;
        if !(dropped.success && lo < sm && sm < hi) {
            break;
        }
        let (Some(a), Some(b)) = (bound(t0, tm, s0, sm)?, bound(tm, t1, sm, s1)?) else {
            return Ok(None);
        };
        pairs.swap_remove(i);
        pairs.push((a, t0, tm, s0, sm));
        pairs.push((b, tm, t1, sm, s1));
    }
    Ok(pairs.iter().map(|p| p.0).max_by(f64::total_cmp))
}

/// An upper bound of the distance from `q` to the segment `[a, b]`: the distance to its
/// point at the clamped projection fraction `s` (any `s` in `[0, 1]` bounds it), widened by
/// the rounding of that point and of the difference and norm (`γ_16` of the largest
/// coordinate, `γ_4` relative). NaN for a NaN coordinate.
fn segment_distance_bound(q: Point3, a: Point3, b: Point3) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    let s = if len2 > 0.0 {
        let s = (q - a).dot(ab) / len2;
        if s.is_finite() {
            s.clamp(0.0, 1.0)
        } else {
            0.0
        }
    } else {
        0.0
    };
    let size = |v: Point3| v.x.abs().max(v.y.abs()).max(v.z.abs());
    let scale = size(q).max(size(a)).max(size(b));
    ((q - (a + s * ab)).length() + gamma(16) * scale) * (1.0 + gamma(4))
}

/// The distance from `p` to `edge_crv`, the curve of edge handle `edge`, over the edge's
/// interval and the edge parameter of the closest point; `None` when the drop fails.
fn drop_onto_edge<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    (edge, edge_crv): (O, &Curve),
    p: Point3,
    tol: f64,
) -> Result<Option<(f64, f64)>> {
    let dropped = edge_crv.drop_point(
        k,
        &edge_interval(geometry, edge)?,
        p,
        None,
        tol,
        None,
        SolverOperation::Minimize,
    )?;
    Ok(dropped
        .success
        .then_some((dropped.distance, dropped.parameter)))
}

/// Not upstream (WR-305): whether `curve` over `interval` runs along edge handle `edge`, whose
/// curve is `edge_crv`, within `tol`: the piece's two ends are farther than `tol` apart (or the
/// edge is closed and the piece goes all the way round it), and the piece of the edge's curve
/// between the drops of its two ends and the curve over
/// `interval` are proved within `tol` of each other as wholes ([`curve_runs_along`]), never
/// on samples. The drops only choose that edge piece: its ends' gaps to the interval's ends
/// are part of the bound, so a poor drop can only refuse.
///
/// **Ends apart.** Two points within the tolerance of each other are one point. A piece whose
/// two ends are that close starts and ends at one point as far as the tolerance tells: it runs
/// along no stretch of an edge between two drops (a piece about as short as the band lies
/// within it whatever its angle to the edge, as a sphere's arc clipping a box corner by about
/// the tolerance, which cuts across the corner to the edge's vertex). It runs along the edge
/// only all the way round a closed edge (its two ends one vertex): proved against the edge's
/// whole curve. Otherwise it is left as the edge intersections left it, at its ends.
///
/// **A closed edge.** On an edge whose two ends are one vertex, the drops of a piece that
/// runs along the edge through that vertex fall on either side of the edge's parameter seam,
/// and the edge piece between them runs the other way round. Where the piece between the
/// drops is not proved, that other way is tried ([`runs_through_seam`]): a drop at one end of
/// the edge's interval is the seam's point, so it is taken at the interval's other end; else
/// the curve is cut at the drop of the seam's point onto it, and each part is proved against
/// the edge piece from its end's drop to the seam's end of the edge's interval. The drops only
/// choose the edge pieces and where the curve is cut; each pair is proved as a whole, so they
/// cannot make the proof wrong.
///
/// A failed drop, two drops at one parameter, or a bound not proved: `false`.
///
/// # Errors
///
/// An unresolved handle; those of the evaluations and of the drops.
fn curve_piece_runs_along_edge<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    (edge, edge_crv): (O, &Curve),
    curve: &Curve,
    interval: &Extent1d,
    tol: f64,
) -> Result<bool> {
    let ends = [
        curve.evaluate_point(k, interval.min())?,
        curve.evaluate_point(k, interval.max())?,
    ];
    let [a, b] = ends;
    let apart = a.distance(b);
    if apart.is_nan() {
        return Ok(false);
    }
    let closed = closed_edge(geometry, edge)?;
    if apart <= tol {
        // one point: the piece runs along the edge only all the way round a closed one
        let runs = closed
            && curve_runs_along(
                k,
                curve,
                interval,
                edge_crv,
                &edge_interval(geometry, edge)?,
                tol,
            )?;
        if runs {
            k.work_counters.add_curved_edge_run();
        }
        return Ok(runs);
    }
    let mut params = [0.0; 2];
    for (p, s) in ends.into_iter().zip(params.iter_mut()) {
        let Some((_, param)) = drop_onto_edge(k, geometry, (edge, edge_crv), p, tol)? else {
            return Ok(false);
        };
        *s = param;
    }
    let [s0, s1] = params;
    // written so that a NaN parameter refuses
    if !(s0 < s1 || s1 < s0) {
        return Ok(false);
    }
    let mut runs = curve_runs_along(
        k,
        curve,
        interval,
        edge_crv,
        &Extent1d::from_min_max(s0.min(s1), s0.max(s1)),
        tol,
    )?;
    if !runs && closed {
        runs = runs_through_seam(k, geometry, (edge, edge_crv), curve, interval, params, tol)?;
    }
    if runs {
        k.work_counters.add_curved_edge_run();
    }
    Ok(runs)
}

/// Whether edge handle `edge` is closed: its two ends are one vertex.
///
/// # Errors
///
/// An unresolved handle.
fn closed_edge<O: ClassObject>(geometry: &impl ClassGeometry<O>, edge: O) -> Result<bool> {
    let (eb, e) = edge_of(geometry, edge, "SmEdge::GetStartVertex")?;
    let (start_v, end_v) = (
        topo_edge::start_vertex(eb, e)?,
        topo_edge::end_vertex(eb, e)?,
    );
    Ok(start_v.is_some() && start_v == end_v)
}

/// Not upstream (#18065): a certified upper bound of the distance between `curve` over
/// `interval` and the piece of edge handle `edge` (curve `edge_crv`) between the drops of its
/// ends ([`run_along_gap_bound`]), when it is within the tolerance of a vertex of the edge whose
/// ball holds an end of the piece and that tolerance is wider than `tol`; else `None`.
///
/// A round's arc that leaves a straight edge tangentially, at the edge's end, departs from it
/// quadratically: it stays within the band over most of its length and leaves it near the end,
/// inside the ball the vertex there already has. Its distance from the edge, bounded over the
/// whole piece on enclosures, is the tolerance the edge needs for the piece to run along it:
/// any piece within that bound of the edge lies within the raised edge's tolerance, wherever
/// along the piece the bound is reached, so no tangency or position of the gap is required.
/// Taken only within that vertex's tolerance, the edge is not raised past a tolerance its end
/// already has (its other vertex can rise to the bound with it). The edge is open (a closed edge's pieces run through its seam,
/// [`runs_through_seam`]), the piece's ends are farther than `tol` apart and drop onto the edge
/// at two parameters.
///
/// # Errors
///
/// An unresolved handle; those of the evaluations and of the drops.
fn tail_gap_within_vertex_ball<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    (edge, edge_crv): (O, &Curve),
    curve: &Curve,
    interval: &Extent1d,
    tol: f64,
) -> Result<Option<f64>> {
    if closed_edge(geometry, edge)? {
        return Ok(None);
    }
    let ends = [
        curve.evaluate_point(k, interval.min())?,
        curve.evaluate_point(k, interval.max())?,
    ];
    // written so that a NaN distance refuses
    if ends[0].distance(ends[1]).partial_cmp(&tol) != Some(core::cmp::Ordering::Greater) {
        return Ok(None);
    }
    let mut params = [0.0; 2];
    for (p, s) in ends.into_iter().zip(params.iter_mut()) {
        let Some((_, param)) = drop_onto_edge(k, geometry, (edge, edge_crv), p, tol)? else {
            return Ok(None);
        };
        *s = param;
    }
    let [s0, s1] = params;
    if !(s0 < s1 || s1 < s0) {
        return Ok(None);
    }
    // the widest tolerance of the edge's vertices whose ball holds an end of the piece
    let (eb, e) = edge_of(geometry, edge, "SmEdge::GetStartVertex")?;
    let mut ball = 0.0_f64;
    for v in [
        topo_edge::start_vertex(eb, e)?,
        topo_edge::end_vertex(eb, e)?,
    ]
    .into_iter()
    .flatten()
    {
        let (vt, vp) = (topo_vertex::tolerance(eb, v)?, topo_vertex::point(eb, v)?);
        if ends.iter().any(|p| p.distance(vp) <= vt) && vt > ball {
            ball = vt;
        }
    }
    if ball.partial_cmp(&tol) != Some(core::cmp::Ordering::Greater) {
        return Ok(None);
    }
    let gap = run_along_gap_bound(
        k,
        curve,
        interval,
        edge_crv,
        &Extent1d::from_min_max(s0.min(s1), s0.max(s1)),
        tol,
    )?;
    Ok(gap.filter(|&g| g <= ball))
}

/// Not upstream (WR-305): whether `curve` over `interval` runs along the closed edge handle
/// `edge` (its two ends one vertex, [`closed_edge`]), whose curve is `edge_crv`, through the
/// edge's parameter seam, its ends dropping onto the edge at `[s0, s1]` (see
/// [`curve_piece_runs_along_edge`], "A closed edge"): with a drop at one end of the edge's
/// interval taken at its other end, or cut at the seam. `false` for a failed drop of the
/// seam's point, a cut at either end of `interval`, or a part not proved.
///
/// # Errors
///
/// An unresolved handle; those of the evaluations and of the drops.
fn runs_through_seam<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    (edge, edge_crv): (O, &Curve),
    curve: &Curve,
    interval: &Extent1d,
    [s0, s1]: [f64; 2],
    tol: f64,
) -> Result<bool> {
    let edge_ivl = edge_interval(geometry, edge)?;
    let (e0, e1) = (edge_ivl.min(), edge_ivl.max());
    let (t0, t1) = (interval.min(), interval.max());
    // an end that drops onto the seam is at either end of the edge's interval
    let other_end = |s: f64| {
        if s == e0 {
            Some(e1)
        } else if s == e1 {
            Some(e0)
        } else {
            None
        }
    };
    for flipped in [
        other_end(s0).map(|f| [f, s1]),
        other_end(s1).map(|f| [s0, f]),
    ]
    .into_iter()
    .flatten()
    {
        let [u0, u1] = flipped;
        if (u0 < u1 || u1 < u0)
            && curve_runs_along(
                k,
                curve,
                interval,
                edge_crv,
                &Extent1d::from_min_max(u0.min(u1), u0.max(u1)),
                tol,
            )?
        {
            return Ok(true);
        }
    }
    let seam = edge_crv.evaluate_point(k, e0)?;
    let dropped = curve.drop_point(
        k,
        interval,
        seam,
        None,
        tol,
        None,
        SolverOperation::Minimize,
    )?;
    let tm = dropped.parameter;
    if !(dropped.success && t0 < tm && tm < t1) {
        return Ok(false);
    }
    // the curve's start runs to the seam's end of the edge's interval past its own drop, the
    // end from the other end of the interval
    let (first, second) = if s1 < s0 {
        ((s0, e1), (e0, s1))
    } else {
        ((e0, s0), (s1, e1))
    };
    for ((c0, c1), (u0, u1)) in [((t0, tm), first), ((tm, t1), second)] {
        if u0.partial_cmp(&u1) != Some(core::cmp::Ordering::Less)
            || !curve_runs_along(
                k,
                curve,
                &Extent1d::from_min_max(c0, c1),
                edge_crv,
                &Extent1d::from_min_max(u0, u1),
                tol,
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Not upstream (#15718): whether `curve` over `interval` runs along the line of
/// `edge_curve` over `edge_interval` within `tol`, both curves lines. The distance from a
/// point to the edge's line is the norm of an affine function of the point, hence convex
/// along the interval, so its two ends bound it, exactly up to the rounding counted here: a certificate, not a sample. `false` for a curve that is not
/// a line and for an edge of zero length.
///
/// # Errors
///
/// Those of the end evaluations.
pub(crate) fn straight_run_on_line(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    edge_curve: &Curve,
    edge_interval: &Extent1d,
    tol: f64,
) -> Result<bool> {
    let line = |c: &Curve| matches!(c.kind(), CurveKind::Line(_));
    if !line(curve) || !line(edge_curve) {
        return Ok(false);
    }
    let (e0, e1) = (
        edge_curve.evaluate_point(k, edge_interval.min())?,
        edge_curve.evaluate_point(k, edge_interval.max())?,
    );
    let along = e1 - e0;
    let length = along.length();
    let mut within = length > 0.0;
    for t in [interval.min(), interval.max()] {
        let p = curve.evaluate_point(k, t)?;
        // |(p - e0) x along| / |along|: the differences carry an absolute error of a few
        // units in the last place of the coordinates, which the direction `along / |along|`
        // amplifies by |p - e0| / |along|; the cross product, its length and the division
        // add a few relative roundings
        let w = p - e0;
        let rounding =
            gamma(16) * (p.length() + e0.length() + e1.length()) * (1.0 + w.length() / length);
        within &= w.cross(along).length() / length + rounding < tol;
    }
    Ok(within)
}

/// Whether `object` is an edge or vertex handle (used by the harness to keep stand-ins of
/// other kinds away from the topology paths).
#[must_use]
pub fn is_topology_handle<O: ClassObject>(object: O) -> bool {
    matches!(
        object.class_kind(),
        ClassObjectKind::Edge | ClassObjectKind::Vertex
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(usize, ClassObjectKind);

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            self.1
        }
    }

    struct Geo(Vec<Curve>);

    impl ClassGeometry<H> for Geo {
        fn curve(&self, o: H) -> Option<&Curve> {
            self.0.get(o.0)
        }
        fn surface(&self, _: H) -> Option<&Surface> {
            None
        }
        fn volume(&self, _: H) -> Option<&crate::surface::volume::Volume> {
            None
        }
        fn point(&self, _: H) -> Option<Point3> {
            None
        }
    }

    fn setup(k: &Kernel, bounds: &[f64]) -> (Geo, CurveClassification<H>) {
        let line = Line::from_points(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), 3);
        let geo = Geo(vec![Curve::from(line)]);
        let mut cc = CurveClassification::<H>::new(k);
        cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        let n = bounds.len() - 1;
        cc.set_size(k, n).expect("size");
        for (i, w) in bounds.windows(2).enumerate() {
            let ivl = cc.at_mut(i).expect("interval");
            ivl.interval = Extent1d::from_min_max(w[0], w[1]);
        }
        (geo, cc)
    }

    #[test]
    fn find_interval_inside_and_boundary() {
        let k = Kernel::default();
        let (geo, cc) = setup(&k, &[0.0, 1.0, 2.0]);
        let tol = 1e-6;
        let r = cc
            .find_interval(&k, &geo, 0.5, BIG_DOUBLE, tol)
            .expect("inside");
        assert_eq!((r.index, r.position), (0, IntervalPosition::Inside));
        let r = cc
            .find_interval(&k, &geo, 1.0, BIG_DOUBLE, tol)
            .expect("boundary");
        assert_eq!((r.index, r.position), (0, IntervalPosition::End));
        let r = cc
            .find_interval(&k, &geo, 1.5, BIG_DOUBLE, tol)
            .expect("inside");
        assert_eq!((r.index, r.position), (1, IntervalPosition::Inside));
    }

    /// #18342: on a line 2 469 134 long over [0, 1] (a loop bisector across an
    /// infinite cylinder's ±1234567 axial sentinel range), a parameter 1.43e-9 inside
    /// an interval end is within `SM_EFF_ZERO_PARAM` of it yet 3.5e-3 away in 3D: it is
    /// an interior point, not that end. Within the 3D tolerance it still snaps.
    #[test]
    fn find_interval_does_not_snap_a_3d_distinct_point_on_a_fast_curve() {
        let k = Kernel::default();
        let line = Line::from_points(
            Point3::new(0.0, 0.0, 1_234_567.0),
            Point3::new(0.0, 0.0, -1_234_567.0),
            3,
        );
        let geo = Geo(vec![Curve::from(line)]);
        let mut cc = CurveClassification::<H>::new(&k);
        cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        cc.set_size(&k, 2).expect("size");
        for (i, w) in [0.0, 0.5, 1.0].windows(2).enumerate() {
            cc.at_mut(i).expect("interval").interval = Extent1d::from_min_max(w[0], w[1]);
        }
        let tol = 1e-7;
        // 3.538e-3 above the end at 0.5 (z = 0): a distinct point inside interval 0
        let far = 0.5 - 3.538e-3 / 2_469_134.0;
        assert!(0.5 - far < crate::math::EFF_ZERO_PARAM);
        let r = cc
            .find_interval(&k, &geo, far, BIG_DOUBLE, tol)
            .expect("far");
        assert_eq!((r.index, r.position), (0, IntervalPosition::Inside));
        // the same distance past the end, at the start of interval 1
        let far = 0.5 + 3.538e-3 / 2_469_134.0;
        let r = cc
            .find_interval(&k, &geo, far, BIG_DOUBLE, tol)
            .expect("far");
        assert_eq!((r.index, r.position), (1, IntervalPosition::Inside));
        // 1e-9 in 3D: the same point, snapped as before
        let near = 0.5 - 1e-9 / 2_469_134.0;
        let r = cc
            .find_interval(&k, &geo, near, BIG_DOUBLE, tol)
            .expect("near");
        assert_eq!((r.index, r.position), (0, IntervalPosition::End));
    }

    #[test]
    fn find_interval_snaps_on_the_parameter_without_a_curve() {
        // no classification curve: no 3D separation to measure, so upstream's
        // parameter snap stands instead of failing on the missing curve
        let k = Kernel::default();
        let geo = Geo(Vec::new());
        let mut cc = CurveClassification::<H>::new(&k);
        cc.set_size(&k, 2).expect("size");
        for (i, w) in [0.0, 0.5, 1.0].windows(2).enumerate() {
            cc.at_mut(i).expect("interval").interval = Extent1d::from_min_max(w[0], w[1]);
        }
        let r = cc
            .find_interval(&k, &geo, 0.5 - 1e-9, BIG_DOUBLE, 1e-6)
            .expect("snaps to the end");
        assert_eq!((r.index, r.position), (0, IntervalPosition::End));
        let r = cc
            .find_interval(&k, &geo, 0.5 + 1e-9, BIG_DOUBLE, 1e-6)
            .expect("snaps to the start");
        assert_eq!((r.index, r.position), (1, IntervalPosition::Start));
    }

    #[test]
    fn find_interval_beyond_the_ends_snaps_on_a_straight_curve() {
        // upstream tests the signed distance along the tangent against the end point, which
        // is on the wrong side for a parameter beyond a monotone curve: it always snaps
        let k = Kernel::default();
        let (geo, cc) = setup(&k, &[0.0, 1.0]);
        let r = cc
            .find_interval(&k, &geo, 5.0, BIG_DOUBLE, 1e-6)
            .expect("snaps to the end");
        assert_eq!((r.index, r.position), (0, IntervalPosition::End));
        assert_eq!(r.end_dist_3d, 4.0);
        let r = cc
            .find_interval(&k, &geo, -5.0, BIG_DOUBLE, 1e-6)
            .expect("snaps to the start");
        assert_eq!((r.index, r.position), (0, IntervalPosition::Start));
        assert_eq!(r.end_dist_3d, 5.0);
    }

    #[test]
    fn insert_splits_the_interval() {
        let k = Kernel::default();
        let (geo, mut cc) = setup(&k, &[0.0, 1.0]);
        let pc = PointClassification::<H>::new(1e-6);
        let r = cc.insert_point_class(&k, &geo, &pc, 0.5, BIG_DOUBLE, false);
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn remove_end_resets_classification() {
        let k = Kernel::default();
        let (_, mut cc) = setup(&k, &[0.0, 1.0]);
        let ivl = cc.at_mut(0).expect("ivl");
        ivl.start
            .set_class_object(PointClassType::Curve, Some(H(0, ClassObjectKind::Curve)));
        cc.remove_point_class(0, IntervalPosition::Start)
            .expect("remove");
        assert_eq!(
            cc.at(0).expect("ivl").start.point_class(),
            PointClassType::Unknown
        );
        let e = cc
            .remove_point_class(0, IntervalPosition::Inside)
            .unwrap_err();
        assert_eq!(
            e.intersect_failure(),
            Some(&IntersectFailure::PositionNotAnEnd)
        );
        assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
        let e = cc
            .remove_point_class(3, IntervalPosition::Start)
            .unwrap_err();
        assert_eq!(
            e.intersect_failure(),
            Some(&IntersectFailure::IntervalIndexPastEnd { index: 3, size: 1 })
        );
    }

    /// A parameter off an end of the classified curve, whose end point (a surf/surf/surf
    /// point) lies a unit beyond it, is refused with the distance and the tolerance it would
    /// snap within.
    #[test]
    fn a_parameter_off_the_curve_names_the_end_and_its_distance() {
        let k = Kernel::default();
        let (geo, mut cc) = setup(&k, &[0.0, 1.0]);
        let ivl = cc.at_mut(0).expect("ivl");
        ivl.start.set_sss_point(Point3::new(-3.0, 0.0, 0.0));
        ivl.end.set_sss_point(Point3::new(3.0, 0.0, 0.0));
        for (param, past_end) in [(-2.0, false), (2.0, true)] {
            let e = cc
                .find_interval(&k, &geo, param, BIG_DOUBLE, 1e-6)
                .unwrap_err();
            let Some(&IntersectFailure::ParameterOffClassifiedCurve {
                past_end: end,
                distance,
                tolerance,
            }) = e.intersect_failure()
            else {
                panic!("{e}");
            };
            assert_eq!(end, past_end);
            assert!((distance - 1.0).abs() < 1e-9, "{distance}");
            assert!(tolerance > 0.0 && tolerance < distance, "{tolerance}");
            assert_eq!(e.sm_status(), Some(crate::error::SM_ERR_OUTSIDE_OF_DOMAIN));
        }
    }

    #[test]
    fn find_interval_without_intervals_is_an_error() {
        let k = Kernel::default();
        let (geo, _) = setup(&k, &[0.0, 1.0]);
        let mut cc = CurveClassification::<H>::new(&k);
        cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        // upstream reads m_pData[0]: undefined
        assert!(cc.find_interval(&k, &geo, 0.5, BIG_DOUBLE, 1e-6).is_err());
        assert!(cc.find_point_class(&k, &geo, 0.5, 1e-6).is_err());
    }

    #[test]
    fn find_point_class_returns_the_boundary_or_interior() {
        let k = Kernel::default();
        let (geo, mut cc) = setup(&k, &[0.0, 1.0, 2.0]);
        let edge = H(1, ClassObjectKind::Edge);
        cc.at_mut(0)
            .expect("ivl")
            .mid
            .set_class_object(PointClassType::Edge, Some(edge));
        let pc = cc.find_point_class(&k, &geo, 0.5, 1e-6).expect("inside");
        assert_eq!(pc.point_class(), PointClassType::Edge);
        let pc = cc.find_point_class(&k, &geo, 1.5, 1e-6).expect("inside");
        assert_eq!(pc.point_class(), PointClassType::Unknown);
    }

    #[test]
    fn is_coincident_follows_the_interval_interiors() {
        let k = Kernel::default();
        let (geo, mut cc) = setup(&k, &[0.0, 1.0, 2.0]);
        let edge = H(1, ClassObjectKind::Edge);
        for j in 0..2 {
            cc.at_mut(j)
                .expect("ivl")
                .mid
                .set_class_object(PointClassType::Edge, Some(edge));
        }
        let tol = 1e-6;
        let all = Extent1d::from_min_max(0.2, 1.8);
        assert!(cc.is_coincident(&k, &geo, &all, tol).expect("walk"));
        // an interval that only touches the second interval at its start skips it
        let first = Extent1d::from_min_max(0.2, 1.0);
        assert!(cc.is_coincident(&k, &geo, &first, tol).expect("walk"));
        cc.at_mut(1)
            .expect("ivl")
            .mid
            .set_class_object(PointClassType::Unknown, None);
        assert!(!cc.is_coincident(&k, &geo, &all, tol).expect("walk"));
        assert!(cc.is_coincident(&k, &geo, &first, tol).expect("walk"));
    }

    #[test]
    fn squeeze_without_overlaps_changes_nothing() {
        let k = Kernel::default();
        let (geo, mut cc) = setup(&k, &[0.0, 1.0, 2.0]);
        let (_, mut other) = setup(&k, &[0.0, 1.0, 2.0]);
        let before = cc.clone();
        fix_problems_by_squeezing(&k, &geo, &mut cc, &mut other).expect("squeeze");
        assert_eq!(cc.size(), before.size());
        // a zero-length interval maps to one boundary of `other`: it is squeezed out
        // (ConnectIntervals)
        let (_, mut squeezed) = setup(&k, &[0.0, 1.0, 1.0, 2.0]);
        fix_problems_by_squeezing(&k, &geo, &mut squeezed, &mut other).expect("squeeze");
        assert_eq!(squeezed.size(), 2);
    }

    /// A unit box with handles for its entities, `H(index into entities, kind)`, and the
    /// classification curves after them.
    struct BoxGeo {
        brep: crate::topology::Brep,
        entities: Vec<EntityRef>,
        curves: Vec<Curve>,
    }

    impl BoxGeo {
        fn handle(&self, e: EntityRef) -> H {
            let kind = match e {
                EntityRef::Vertex(_) => ClassObjectKind::Vertex,
                EntityRef::Edge(_) => ClassObjectKind::Edge,
                _ => ClassObjectKind::Face,
            };
            let i = self.entities.iter().position(|&x| x == e).expect("listed");
            H(i, kind)
        }
        fn vertex_at(&self, p: Point3) -> VertexId {
            crate::topology::brep::p1::vertices(&self.brep, None)
                .expect("vertices")
                .into_iter()
                .find(|&v| {
                    topo_vertex::point(&self.brep, v)
                        .expect("point")
                        .distance(p)
                        < 1e-12
                })
                .expect("a vertex there")
        }
    }

    impl ClassGeometry<H> for BoxGeo {
        fn curve(&self, o: H) -> Option<&Curve> {
            self.curves.get(o.0.checked_sub(self.entities.len())?)
        }
        fn surface(&self, _: H) -> Option<&Surface> {
            None
        }
        fn volume(&self, _: H) -> Option<&crate::surface::volume::Volume> {
            None
        }
        fn point(&self, _: H) -> Option<Point3> {
            None
        }
        fn topology_entity(&self, o: H) -> Option<(&crate::topology::Brep, EntityRef)> {
            self.entities.get(o.0).map(|&e| (&self.brep, e))
        }
        fn topology_handle(&self, e: EntityRef) -> Option<H> {
            self.entities.contains(&e).then(|| self.handle(e))
        }
    }

    /// The top face of the unit box, its edge from (0, 0, 1) to (1, 0, 1) and the vertex
    /// (1, 0, 1); a classification of the line from `from` to (1, 0, 1) on that face, one
    /// interval, its start unclassified and its end on the vertex.
    fn along_top_edge(from: Point3) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        along_top_edge_by(Curve::from(Line::from_points(
            from,
            Point3::new(1.0, 0.0, 1.0),
            3,
        )))
    }

    /// [`along_top_edge`] for `curve`, which ends at (1, 0, 1) at parameter 1 and is
    /// classified over `[0, 1]`.
    fn along_top_edge_by(curve: Curve) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        let mut k = Kernel::default();
        let brep = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )
        .expect("box");
        let mut entities = Vec::new();
        for v in crate::topology::brep::p1::vertices(&brep, None).expect("vertices") {
            entities.push(EntityRef::Vertex(v));
        }
        for e in crate::topology::brep::p1::edges(&brep, None).expect("edges") {
            entities.push(EntityRef::Edge(e));
        }
        for f in crate::topology::brep::p1::faces(&brep, None).expect("faces") {
            entities.push(EntityRef::Face(f));
        }
        let end = Point3::new(1.0, 0.0, 1.0);
        let geo = BoxGeo {
            brep,
            entities,
            curves: vec![curve],
        };
        let (corner, other, far) = (
            geo.vertex_at(end),
            geo.vertex_at(Point3::new(0.0, 0.0, 1.0)),
            geo.vertex_at(Point3::new(0.0, 1.0, 1.0)),
        );
        let faces_of = |v| topo_vertex::faces(&geo.brep, v).expect("faces");
        let top = faces_of(corner)
            .into_iter()
            .find(|f| faces_of(other).contains(f) && faces_of(far).contains(f))
            .expect("top face");
        let edge = topo_vertex::edges(&geo.brep, corner, Some(top))
            .expect("edges")
            .into_iter()
            .find(|&e| {
                [
                    topo_edge::start_vertex(&geo.brep, e),
                    topo_edge::end_vertex(&geo.brep, e),
                ]
                .into_iter()
                .any(|v| v.expect("vertex") == Some(other))
            })
            .expect("the edge along x");
        let mut cc = CurveClassification::<H>::new(&k);
        cc.set_curve(Some(H(geo.entities.len(), ClassObjectKind::Curve)), false);
        cc.set_face(Some(geo.handle(EntityRef::Face(top))));
        cc.set_size(&k, 1).expect("size");
        let ivl = cc.at_mut(0).expect("interval");
        ivl.interval = Extent1d::from_min_max(0.0, 1.0);
        ivl.end.set_class_object(
            PointClassType::Vertex,
            Some(geo.handle(EntityRef::Vertex(corner))),
        );
        let edge = geo.handle(EntityRef::Edge(edge));
        (k, geo, cc, edge)
    }

    #[test]
    fn an_unclassified_end_along_an_edge_is_on_that_edge() {
        // 1e-11 off the edge, halfway along it: the interval runs along the edge
        let (k, geo, mut cc, edge) = along_top_edge(Point3::new(0.5, 1e-11, 1.0));
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Edge);
        assert_eq!(ivl.mid.object(), Some(edge));
        assert_eq!(ivl.start.point_class(), PointClassType::Edge);
        assert_eq!(ivl.start.object(), Some(edge));
        assert_eq!(ivl.end.point_class(), PointClassType::Vertex);
        // the start keeps its parameter on the edge: the edge's point there is the start's
        // projection onto it
        assert!(ivl.start.are_parameters_set());
        let on_edge = edge_curve(&geo, edge)
            .expect("edge")
            .expect("curve")
            .evaluate_point(&k, ivl.start.t_param())
            .expect("point");
        let off = on_edge.distance(Point3::new(0.5, 0.0, 1.0));
        assert!(off < 1e-12, "{on_edge:?}");
    }

    #[test]
    fn an_unclassified_end_of_a_curve_that_bows_out_of_the_band_is_left_alone() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        // y(t) = 1e-6 (1 - t) + 1.25e-3 t (1 - t) (1 - 2 t) from x = 0.5 to the vertex at
        // x = 1: the start is 1e-6 off the edge and the middle 5e-7, both inside the band
        // (1e-5), but at t = 0.25 the curve is 1.179375e-4 off it
        let (k3, y3) = (1.25e-3 / 3.0, 1e-6 / 3.0);
        let pw = vec![
            CPoint::new(0.5, 3.0 * y3, 1.0, NOW),
            CPoint::new(2.0 / 3.0, 2.0 * y3 + k3, 1.0, NOW),
            CPoint::new(5.0 / 6.0, y3 - k3, 1.0, NOW),
            CPoint::new(1.0, 0.0, 1.0, NOW),
        ];
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let bezier =
            BSplineCurve::from_nurb(NurbCurve::new(pw, 3, knots).expect("curve"), 3, false);
        let (k, geo, mut cc, edge) = along_top_edge_by(Curve::from(bezier));
        let curve = geo
            .curve(H(geo.entities.len(), ClassObjectKind::Curve))
            .expect("curve");
        let off_edge = |t: f64| curve.evaluate_point(&k, t).expect("point").y.abs();
        assert!((off_edge(0.5) - 5e-7).abs() < 1e-12);
        assert!((off_edge(0.25) - 1.179375e-4).abs() < 1e-12);
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Unknown);
        assert_eq!(ivl.start.point_class(), PointClassType::Unknown);
        assert_ne!(ivl.mid.object(), Some(edge));
    }

    #[test]
    fn a_short_piece_that_crosses_the_edge_into_its_end_is_left_alone() {
        // the line arrives at the corner at 2.9 degrees to the edge (the angle tolerance is
        // 0.5) and starts 5e-6 off it, 1e-4 from the corner: inside the band (1e-5) all
        // along and clear of the vertex, but it crosses the edge, it does not run along it
        // (a cut passing a hair off a sheet metal flange's corner, where classifying such an
        // end onto the edge left an edge whose curve ends 1e-5 from its vertex)
        let (k, geo, mut cc, _) = along_top_edge(Point3::new(1.0 - 1e-4, 5e-6, 1.0));
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Unknown);
        assert_eq!(ivl.start.point_class(), PointClassType::Unknown);
    }

    #[test]
    fn an_edge_without_a_curve_is_skipped_not_an_error() {
        // the start 1e-11 off the edge runs along it (as in
        // `an_unclassified_end_along_an_edge_is_on_that_edge`), but the caller's edge has no
        // curve: it proves nothing, the other edge at the corner does not take the interval,
        // and nothing fails for want of the curve
        let (k, mut geo, mut cc, edge) = along_top_edge(Point3::new(0.5, 1e-11, 1.0));
        let Some(&EntityRef::Edge(e)) = geo.entities.get(edge.0) else {
            panic!("not an edge handle");
        };
        geo.brep.edge_mut(e).expect("edge").curve = None;
        let raise = cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        assert!(raise.is_none());
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Unknown);
        assert_eq!(ivl.start.point_class(), PointClassType::Unknown);
        assert_eq!(ivl.end.point_class(), PointClassType::Vertex);
    }

    #[test]
    fn an_unclassified_end_off_the_edge_or_at_its_vertex_is_left_alone() {
        // 1e-4 off the edge (the band is 1e-5), and 1e-5 from the edge's far vertex
        for from in [Point3::new(0.5, 1e-4, 1.0), Point3::new(1e-5, 0.0, 1.0)] {
            let (k, geo, mut cc, _) = along_top_edge(from);
            cc.classify_edge_interval(&k, &geo, 0).expect("classify");
            let ivl = cc.at(0).expect("interval");
            assert_eq!(ivl.mid.point_class(), PointClassType::Unknown, "{from:?}");
            assert_eq!(ivl.start.point_class(), PointClassType::Unknown, "{from:?}");
        }
    }

    /// The cubic Bézier from (0.5, y0, 1) to the corner (1, 0, 1) whose control points lie at
    /// x = 0.5, 2/3, 5/6, 1 with heights `ys`, on [`along_top_edge_by`]'s face, its start
    /// classified onto the edge along x.
    fn bezier_with_start_on_the_edge(ys: [f64; 4]) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        bezier_from_the_edge([0.5, 2.0 / 3.0, 5.0 / 6.0, 1.0], ys)
    }

    /// [`bezier_with_start_on_the_edge`] with control points at `xs` (the last at 1).
    fn bezier_from_the_edge(
        xs: [f64; 4],
        ys: [f64; 4],
    ) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let pw = xs
            .into_iter()
            .zip(ys)
            .map(|(x, y)| CPoint::new(x, y, 1.0, NOW))
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let bezier =
            BSplineCurve::from_nurb(NurbCurve::new(pw, 3, knots).expect("curve"), 3, false);
        let (k, geo, mut cc, edge) = along_top_edge_by(Curve::from(bezier));
        cc.at_mut(0)
            .expect("interval")
            .start
            .set_class_object(PointClassType::Edge, Some(edge));
        (k, geo, cc, edge)
    }

    #[test]
    fn a_curved_piece_along_the_edge_with_both_ends_on_it_is_on_that_edge() {
        // within 1e-11 of the edge all along (its control points are): the whole piece lies
        // in the band (1e-5), so the interval runs along the edge
        let (k, geo, mut cc, edge) = bezier_with_start_on_the_edge([0.0, 1e-11, 1e-11, 0.0]);
        let curve = geo
            .curve(H(geo.entities.len(), ClassObjectKind::Curve))
            .expect("curve");
        assert!(
            !matches!(curve.kind(), CurveKind::Line(_)),
            "a curved piece"
        );
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Edge);
        assert_eq!(ivl.mid.object(), Some(edge));
        assert_eq!(ivl.start.point_class(), PointClassType::Edge);
        assert_eq!(ivl.start.object(), Some(edge));
        assert_eq!(ivl.end.point_class(), PointClassType::Vertex);
        // by the curved piece's certificate
        assert_eq!(k.work_counters.curved_edge_runs(), 1);
    }

    #[test]
    fn a_curved_piece_with_both_ends_on_the_edge_that_bows_out_of_the_band_is_left_alone() {
        // the curve of `an_unclassified_end_of_a_curve_that_bows_out_of_the_band_is_left_alone`
        // with its start classified onto the edge: both ends and the middle (5e-7 off) lie in
        // the band (1e-5), but at t = 0.25 the curve is 1.179375e-4 off the edge, so it does
        // not run along it. The fork's midpoint test took it as on the edge.
        let (k3, y3) = (1.25e-3 / 3.0, 1e-6 / 3.0);
        let (k, geo, mut cc, edge) =
            bezier_with_start_on_the_edge([3.0 * y3, 2.0 * y3 + k3, y3 - k3, 0.0]);
        let curve = geo
            .curve(H(geo.entities.len(), ClassObjectKind::Curve))
            .expect("curve");
        let off_edge = |t: f64| curve.evaluate_point(&k, t).expect("point").y.abs();
        assert!((off_edge(0.5) - 5e-7).abs() < 1e-12);
        assert!((off_edge(0.25) - 1.179375e-4).abs() < 1e-12);
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Unknown);
        assert_ne!(ivl.mid.object(), Some(edge));
        assert_eq!(k.work_counters.curved_edge_runs(), 0);
    }

    /// A polyline (degree 1) along the top edge ending at `x_end`, by a knot at 0.25 up 4e-6
    /// at 45 degrees, down again and along the edge, over [0.25, 1] with its start on the
    /// edge: the whole piece lies in the band (1e-5) and meets its end along the edge, but
    /// at its start the piece's own direction (the span after the knot) crosses the edge,
    /// and only the span before it, outside the piece, runs along it.
    fn piece_leaving_a_knot_across_the_edge(
        x_end: f64,
    ) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let pw = [
            (x_end - 2e-5, 0.0),
            (x_end - 1.5e-5, 0.0),
            (x_end - 1.1e-5, 4e-6),
            (x_end - 7e-6, 0.0),
            (x_end, 0.0),
        ]
        .into_iter()
        .map(|(x, y)| CPoint::new(x, y, 1.0, NOW))
        .collect();
        let knots = vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0];
        let polyline =
            BSplineCurve::from_nurb(NurbCurve::new(pw, 1, knots).expect("curve"), 3, false);
        let (k, geo, mut cc, edge) = along_top_edge_by(Curve::from(polyline));
        let ivl = cc.at_mut(0).expect("interval");
        ivl.interval = Extent1d::from_min_max(0.25, 1.0);
        ivl.start.set_class_object(PointClassType::Edge, Some(edge));
        let curve = geo
            .curve(H(geo.entities.len(), ClassObjectKind::Curve))
            .expect("curve");
        // `from_left` picks the knot span `[U[i], U[i+1])` holding `t` (`Side::Left`), so
        // at a knot `true` reads the span starting there and `false` the span ending there
        let dir = |t: f64, from_left: bool| {
            let d = curve
                .evaluate(&k, t, 1, from_left, true)
                .expect("derivative")[1];
            d.y / d.x
        };
        assert!(
            (dir(0.25, true) - 1.0).abs() < 1e-6,
            "leaves the knot at 45 degrees"
        );
        assert!(
            dir(0.25, false).abs() < 1e-12,
            "the span before runs along the edge"
        );
        assert!(
            dir(1.0, false).abs() < 1e-12,
            "meets its end along the edge"
        );
        (k, geo, cc, edge)
    }

    #[test]
    fn a_piece_leaving_a_knot_across_the_edge_onto_its_vertex_is_on_the_edge() {
        // ending on the edge's corner vertex: both ends are classified, 1.5e-5 apart (more
        // than the band, 1e-5), and the whole piece is proved within 4e-6 of the edge, so the
        // curved piece's certificate accepts it, whatever its direction at the knot
        let (k, geo, mut cc, edge) = piece_leaving_a_knot_across_the_edge(1.0);
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Edge);
        assert_eq!(ivl.mid.object(), Some(edge));
        assert_eq!(k.work_counters.curved_edge_runs(), 1);
    }

    #[test]
    fn an_unclassified_end_of_a_piece_leaving_a_knot_across_the_edge_is_left_alone() {
        // ending unclassified halfway along the edge: the unclassified end's check refuses
        let (k, geo, mut cc, edge) = piece_leaving_a_knot_across_the_edge(0.5);
        cc.at_mut(0)
            .expect("interval")
            .end
            .set_class_object(PointClassType::Unknown, None);
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Unknown);
        assert_eq!(ivl.end.point_class(), PointClassType::Unknown);
        assert_ne!(ivl.mid.object(), Some(edge));
        assert_eq!(k.work_counters.curved_edge_runs(), 0);
    }

    /// The cubic Bézier from the edge to its corner over `x_len`, leaving and meeting the edge
    /// at 45 degrees and at most `x_len / 4` off it (its control points at heights 0,
    /// `x_len / 3`, `x_len / 3`, 0).
    fn lens_to_the_corner(x_len: f64) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        let (s1, s2) = (x_len / 3.0, 2.0 * x_len / 3.0);
        let (k, geo, cc, edge) =
            bezier_from_the_edge([1.0 - x_len, 1.0 - s2, 1.0 - s1, 1.0], [0.0, s1, s1, 0.0]);
        let curve = geo
            .curve(H(geo.entities.len(), ClassObjectKind::Curve))
            .expect("curve");
        let off_edge = |t: f64| curve.evaluate_point(&k, t).expect("point").y.abs();
        assert!((off_edge(0.5) - 0.25 * x_len).abs() < 1e-15);
        let slope = |t: f64| {
            let d = curve.evaluate(&k, t, 1, false, true).expect("derivative")[1];
            (d.y / d.x).abs()
        };
        assert!((slope(0.0) - 1.0).abs() < 1e-9 && (slope(1.0) - 1.0).abs() < 1e-9);
        (k, geo, cc, edge)
    }

    #[test]
    fn a_curved_piece_whose_ends_are_within_the_band_of_each_other_is_left_alone() {
        // 8e-6 long, from the edge to its corner, at most 2e-6 off it: its ends lie within
        // the band (1e-5) of each other, so they are one point at the tolerance and the piece
        // has no length along the edge to run (a sphere clipping a box corner by about the
        // tolerance, whose arc on a side face cuts across the corner: goldens `tess0033`)
        let (k, geo, mut cc, edge) = lens_to_the_corner(8e-6);
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Unknown);
        assert_ne!(ivl.mid.object(), Some(edge));
        assert_eq!(k.work_counters.curved_edge_runs(), 0);
    }

    #[test]
    fn a_curved_piece_longer_than_the_band_and_within_it_is_on_the_edge() {
        // 1.5e-5 long, at most 3.75e-6 off the edge: its ends are farther apart than the
        // band (1e-5) and the whole piece is proved within it, so it runs along the edge
        // however steeply it leaves it (tangency is not asked of a classified end)
        let (k, geo, mut cc, edge) = lens_to_the_corner(1.5e-5);
        cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        let ivl = cc.at(0).expect("interval");
        assert_eq!(ivl.mid.point_class(), PointClassType::Edge);
        assert_eq!(ivl.mid.object(), Some(edge));
        assert_eq!(k.work_counters.curved_edge_runs(), 1);
    }

    /// The cubic `y = h t^3` over the last 1e-2 of the edge, leaving it tangentially at its
    /// start and ending `h` off the edge's corner vertex, with that vertex's tolerance
    /// `corner_tol` (a round's arc running off a straight edge into its end vertex's ball).
    fn tangent_tail_to_the_corner(
        h: f64,
        corner_tol: f64,
    ) -> (Kernel, BoxGeo, CurveClassification<H>, H) {
        let len = 1e-2;
        let (k, mut geo, cc, edge) = bezier_from_the_edge(
            [1.0 - len, 1.0 - 2.0 * len / 3.0, 1.0 - len / 3.0, 1.0],
            [0.0, 0.0, 0.0, h],
        );
        let corner = geo.vertex_at(Point3::new(1.0, 0.0, 1.0));
        topo_vertex::set_tolerance(&mut geo.brep, corner, corner_tol, true, true)
            .expect("tolerance");
        (k, geo, cc, edge)
    }

    #[test]
    fn a_curved_piece_leaving_the_edge_tangentially_into_its_vertex_ball_raises_the_edge() {
        // 1.5e-5 off the edge at its end, past the band (1e-5) over the last 1.3e-3 of the
        // piece, inside the corner's ball (2e-5): the edge needs the piece's certified
        // distance, past the band and within that ball, and the interval waits for the raise
        let (k, geo, mut cc, edge) = tangent_tail_to_the_corner(1.5e-5, 2e-5);
        let raise = cc
            .classify_edge_interval(&k, &geo, 0)
            .expect("classify")
            .expect("a raise");
        let gap = raise.tolerance;
        assert_eq!((raise.index, raise.edge), (0, edge));
        assert!((1.5e-5..=2e-5).contains(&gap), "{gap}");
        assert_eq!(
            cc.at(0).expect("interval").mid.point_class(),
            PointClassType::Unknown
        );
        cc.set_edge_interval(&raise).expect("set");
        assert_eq!(cc.at(0).expect("interval").mid.object(), Some(edge));
    }

    #[test]
    fn a_curved_piece_leaving_the_edge_past_its_vertex_ball_is_left_alone() {
        // the same piece with the corner's ball no wider than the band: nothing at the edge's
        // end tolerates the 1.5e-5, so the edge is not raised and the interval is left alone
        let (k, geo, mut cc, _) = tangent_tail_to_the_corner(1.5e-5, 1e-5);
        let raise = cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        assert_eq!(raise, None);
        assert_eq!(
            cc.at(0).expect("interval").mid.point_class(),
            PointClassType::Unknown
        );
    }

    #[test]
    fn a_curved_piece_bowing_off_the_edge_farther_than_its_vertex_ball_is_left_alone() {
        // ends 1.5e-5 off the corner, inside its ball (2e-5), but the piece bows about 4e-5
        // off the edge before it (y = 3c t^2 (1-t) + h t^3 at t = 3/4): its certified distance
        // is past that ball, so the edge is not raised and the interval is left alone
        let len = 1e-2;
        let (k, mut geo, mut cc, _) = bezier_from_the_edge(
            [1.0 - len, 1.0 - 2.0 * len / 3.0, 1.0 - len / 3.0, 1.0],
            [0.0, 0.0, 8e-5, 1.5e-5],
        );
        let corner = geo.vertex_at(Point3::new(1.0, 0.0, 1.0));
        topo_vertex::set_tolerance(&mut geo.brep, corner, 2e-5, true, true).expect("tolerance");
        let raise = cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        assert_eq!(raise, None);
        assert_eq!(
            cc.at(0).expect("interval").mid.point_class(),
            PointClassType::Unknown
        );
    }

    /// The circles of #18065's half-torus seam test: a boss's top circle (its radius as the
    /// design's record round trip left it) overhanging the face's closed circular edge by the
    /// band `tol`, the edge's seam turned away from the overhang.
    fn boss_and_edge(k: &Kernel, cx: f64) -> (Curve, Curve) {
        use crate::curve::ellipse::{Ellipse, EllipseOptions};
        let circle = |c: Point3, x: Vec3, y: Vec3, r: f64| {
            let e = Ellipse::new_circle(
                k,
                c,
                x,
                y,
                Extent1d::from_raw(0.0, 360.0),
                r,
                &EllipseOptions::default(),
            )
            .expect("circle");
            Curve::from(e)
        };
        let boss = circle(
            Point3::new(cx, 0.0, 0.0),
            Vec3::new(0.8178681931579774, 0.5754056122601738, 0.0),
            Vec3::new(-0.5754056122601738, 0.8178681931579774, 0.0),
            0.07000000000000013,
        );
        let edge = circle(
            Point3::ZERO,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            2.4,
        );
        (boss, edge)
    }

    /// The boss's arc about its apex (its point farthest from the edge's centre) and the edge's
    /// parameters at the drops of the arc's ends.
    fn apex_piece(k: &Kernel, boss: &Curve, edge: &Curve, tol: f64) -> (Extent1d, Extent1d) {
        let drop = |c: &Curve, p: Point3| {
            let d = c
                .drop_point(
                    k,
                    &c.natural_interval(),
                    p,
                    None,
                    tol,
                    None,
                    SolverOperation::Minimize,
                )
                .expect("drop");
            assert!(d.success);
            d.parameter
        };
        let apex = drop(boss, Point3::new(3.0, 0.0, 0.0));
        let piece = Extent1d::from_min_max(apex - 0.0016, apex + 0.0016);
        let (s0, s1) = (
            drop(edge, boss.evaluate_point(k, piece.min()).expect("start")),
            drop(edge, boss.evaluate_point(k, piece.max()).expect("end")),
        );
        (piece, Extent1d::from_min_max(s0.min(s1), s0.max(s1)))
    }

    /// #18065: the closed-form gap of the overhanging arc is `cx + ra - re`, above the band by
    /// the stored radius's rounding (`4.2e-16`) and within the coordinates' resolution
    /// `√3 ulp(2.4) = 7.7e-16`; the chord enclosures' certified bound is no lower than it (it is
    /// a bound) and above the band, so only the closed form proves the run.
    #[test]
    fn an_arc_overhanging_a_circular_edge_by_the_band_runs_along_it_in_closed_form() {
        use crate::geom::expansion::{add, sign, sub, to_f64};
        let k = Kernel::default();
        let tol = 2.0e-5;
        let cx = 2.33002;
        let (boss, edge) = boss_and_edge(&k, cx);
        let excess = sub(&add(&add(&[cx], &[0.07000000000000013]), &[-2.4]), &[tol]);
        let resolution = coordinate_resolution(next_up(2.4)).expect("resolution");
        assert_eq!(sign(&excess), 1, "the closed-form gap is above the band");
        assert!(
            to_f64(&excess) < 0.6 * resolution,
            "{:e} within {resolution:e}",
            to_f64(&excess)
        );
        assert!((resolution - 7.69e-16).abs() < 1e-18, "{resolution:e}");
        let (piece, edge_piece) = apex_piece(&k, &boss, &edge, tol);
        let certified = run_along_gap_bound(&k, &boss, &piece, &edge, &edge_piece, tol)
            .expect("bound")
            .expect("certified");
        assert!(
            certified >= tol + to_f64(&excess),
            "the certificate {certified:e} bounds the closed-form gap"
        );
        assert!(certified > tol, "the chords alone refuse: {certified:e}");
        assert!(curve_runs_along(&k, &boss, &piece, &edge, &edge_piece, tol).expect("run"));
    }

    /// #18065: the closed form refuses a real defect near the accepted case: the boss's centre
    /// a few units further out puts its gap past the band by more than the resolution.
    #[test]
    fn an_arc_overhanging_a_circular_edge_past_the_resolution_does_not_run_along_it() {
        let k = Kernel::default();
        let tol = 2.0e-5;
        // `+ 1e-15` puts the gap about `1.4e-15` past the band, just past the resolution
        for cx in [
            2.33002 + 1.0e-15,
            2.33002 + 2.0e-15,
            2.33002 + 1.0e-12,
            2.33002 + 1.0e-7,
        ] {
            let (boss, edge) = boss_and_edge(&k, cx);
            let (piece, edge_piece) = apex_piece(&k, &boss, &edge, tol);
            assert!(
                !curve_runs_along(&k, &boss, &piece, &edge, &edge_piece, tol).expect("run"),
                "centre {cx}"
            );
        }
    }

    /// #18065: what is proved is the stored NURB, not the analytic circle: a boss whose circle
    /// lies well within the band but whose NURB is a larger circle's, bulging `1e-9` past the
    /// band, does not run along the edge.
    #[test]
    fn an_arc_whose_nurb_bulges_past_the_band_does_not_run_along_it() {
        use crate::curve::ellipse::{Ellipse, EllipseOptions};
        let k = Kernel::default();
        let tol = 2.0e-5;
        let (cx, ra) = (2.33002 - 1.0e-9, 0.07000000000000013);
        let (x, y) = (
            Vec3::new(0.8178681931579774, 0.5754056122601738, 0.0),
            Vec3::new(-0.5754056122601738, 0.8178681931579774, 0.0),
        );
        let circle = |r: f64, nurb: Option<&crate::nurbs::b_spline_curve::BSplineCurve>| {
            Curve::from(
                Ellipse::new_circle(
                    &k,
                    Point3::new(cx, 0.0, 0.0),
                    x,
                    y,
                    Extent1d::from_raw(0.0, 360.0),
                    r,
                    &EllipseOptions {
                        nurb,
                        ..EllipseOptions::default()
                    },
                )
                .expect("circle"),
            )
        };
        let larger = circle(ra + 2.0e-9, None);
        let boss = circle(ra, larger.b_spline_base());
        let (_, edge) = boss_and_edge(&k, cx);
        let (piece, edge_piece) = apex_piece(&k, &boss, &edge, tol);
        let apex = boss
            .evaluate_point(&k, 0.5 * (piece.min() + piece.max()))
            .expect("apex");
        assert!(
            apex.length() > 2.4 + tol + 5.0e-10,
            "the NURB bulges: {apex:?}"
        );
        let (_, outward) = radial_deviation(
            &boss,
            (Point3::new(cx, 0.0, 0.0), ra),
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            [piece.min(), piece.max()],
        )
        .expect("a circle's NURB is bounded");
        assert!(outward >= 1.9e-9, "the bulge is counted: {outward:e}");
        assert!(!circle_arcs_within(
            &boss,
            &piece,
            &edge,
            [edge_piece.min(), edge_piece.max()],
            tol
        ));
        assert!(!curve_runs_along(&k, &boss, &piece, &edge, &edge_piece, tol).expect("run"));
    }

    /// #18065: a NURB lifted out of its circle's plane is refused: a circle of radius
    /// `sqrt(r² - h²)` at height `h` is at distance `r` from the centre everywhere, so only its
    /// plane tells it apart from the edge it stands `h` away from.
    #[test]
    fn an_arc_whose_nurb_leaves_its_circles_plane_does_not_run_along_it() {
        use crate::curve::ellipse::{Ellipse, EllipseOptions};
        let k = Kernel::default();
        let tol = 2.0e-5;
        let (r, h) = (2.4_f64, 0.01_f64);
        let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let circle =
            |c: Point3, r: f64, nurb: Option<&crate::nurbs::b_spline_curve::BSplineCurve>| {
                Curve::from(
                    Ellipse::new_circle(
                        &k,
                        c,
                        x,
                        y,
                        Extent1d::from_raw(0.0, 360.0),
                        r,
                        &EllipseOptions {
                            nurb,
                            ..EllipseOptions::default()
                        },
                    )
                    .expect("circle"),
                )
            };
        let lifted = circle(Point3::new(0.0, 0.0, h), (r * r - h * h).sqrt(), None);
        let boss = circle(Point3::ZERO, r, lifted.b_spline_base());
        let edge = circle(Point3::ZERO, r, None);
        let piece = Extent1d::from_min_max(0.001, 0.003);
        let mid = boss.evaluate_point(&k, 0.002).expect("mid");
        assert!((mid.z - h).abs() < 1e-12, "the NURB is lifted: {mid:?}");
        assert!(radial_deviation(&boss, (Point3::ZERO, r), (x, y), [0.001, 0.003]).is_none());
        assert!(!circle_arcs_within(
            &boss,
            &piece,
            &edge,
            [0.001, 0.003],
            tol
        ));
        assert!(!curve_runs_along(&k, &boss, &piece, &edge, &piece, tol).expect("run"));
    }

    /// #20543: a NURB's equal weights scaled by `2^513` (the same curve) square past the
    /// doubles' range; `W²` overflowing made the radial bound `0` for a NURB bulging `1e-6`
    /// off its circle. It is refused instead.
    #[test]
    fn a_nurb_whose_weights_square_past_the_doubles_is_not_bounded() {
        use crate::curve::ellipse::{Ellipse, EllipseOptions};
        let k = Kernel::default();
        let r = 0.05;
        let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let circle = |r: f64, nurb: Option<&crate::nurbs::b_spline_curve::BSplineCurve>| {
            Curve::from(
                Ellipse::new_circle(
                    &k,
                    Point3::ZERO,
                    x,
                    y,
                    Extent1d::from_raw(0.0, 360.0),
                    r,
                    &EllipseOptions {
                        nurb,
                        ..EllipseOptions::default()
                    },
                )
                .expect("circle"),
            )
        };
        let larger = circle(r + 1.0e-6, None);
        let mut scaled = larger.b_spline_base().expect("a NURB").clone_exact();
        let scale = 2.0_f64.powi(513);
        for p in scaled.get_or_create_gw_nurb().expect("nurb").cpts_mut() {
            assert!(
                p.w != crate::nurbs::math::NOW,
                "a circle's NURB is rational"
            );
            p.x *= scale;
            p.y *= scale;
            if p.z != crate::nurbs::math::NOZ {
                p.z *= scale;
            }
            p.w *= scale;
        }
        let range = [0.001, 0.003];
        let unscaled = circle(r, larger.b_spline_base());
        let (_, outward) = radial_deviation(&unscaled, (Point3::ZERO, r), (x, y), range)
            .expect("the unscaled NURB is bounded");
        assert!(outward >= 0.9e-6, "the bulge is counted: {outward:e}");
        let boss = circle(r, Some(&scaled));
        let mid = boss.evaluate_point(&k, 0.002).expect("mid");
        assert!(mid.length() > r + 0.9e-6, "the same curve: {mid:?}");
        match radial_deviation(&boss, (Point3::ZERO, r), (x, y), range) {
            None => {}
            Some((_, outward)) => assert!(outward >= 0.9e-6, "the bulge is lost: {outward:e}"),
        }
    }

    /// #20543: a classification that may not set tolerances (the seam checks) gets no raise
    /// for a piece that would raise its edge: the interval is left for the face classification.
    #[test]
    fn a_classification_that_may_not_set_tolerances_gets_no_raise() {
        let (k, geo, mut cc, _) = tangent_tail_to_the_corner(1.5e-5, 2e-5);
        cc.set_ok_to_set_tolerance(false);
        let raise = cc.classify_edge_interval(&k, &geo, 0).expect("classify");
        assert_eq!(raise, None);
        assert_eq!(
            cc.at(0).expect("interval").mid.point_class(),
            PointClassType::Unknown
        );
    }

    /// #20543: the angle and cosine the circle certificate uses come from basic operations,
    /// within their stated bounds: exact angles, and the platform's values as a cross-check
    /// (within the two bounds together, as the platform's are within a few ulps).
    #[test]
    fn the_enclosed_angle_and_cosine_bound_their_values() {
        use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        for (y, x, exact) in [
            (1.0, 1.0, FRAC_PI_4),
            (1.0, 0.0, FRAC_PI_2),
            (0.0, -1.0, PI),
            (-0.0, -1.0, -PI),
            (-3.0, 0.0, -FRAC_PI_2),
            (0.0, 2.0, 0.0),
        ] {
            let a = atan2_enclosed(y, x).expect("an angle");
            assert!((a - exact).abs() <= ATAN2_ERR, "atan2({y}, {x}) = {a}");
        }
        assert_eq!(atan2_enclosed(0.0, 0.0), None);
        assert_eq!(atan2_enclosed(f64::NAN, 1.0), None);
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..10_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let t = (state >> 11) as f64 / (1_u64 << 53) as f64;
            let (y, x) = ((t * 7.0).sin() * (1.0 + t), (t * 7.0).cos() * (2.0 - t));
            let a = atan2_enclosed(y, x).expect("an angle");
            assert!(
                (a - y.atan2(x)).abs() <= ATAN2_ERR + 8.0 * ROUNDOFF,
                "{y} {x}"
            );
            let g = t * FRAC_PI_2;
            let c = cos_lower(g).expect("a cosine");
            assert!(c <= g.cos() + 4.0 * ROUNDOFF, "{g}: {c} above {}", g.cos());
            let pad = ROUNDOFF * (8.0 + 48.0 * g * g);
            assert!(c >= g.cos() - pad, "{g}: {c} far below {}", g.cos());
        }
        assert_eq!(cos_lower(0.0), Some(1.0 - 3.0 * ROUNDOFF));
        assert_eq!(cos_lower(FRAC_PI_2), None);
        assert_eq!(cos_lower(-1.0), None);
    }
}
