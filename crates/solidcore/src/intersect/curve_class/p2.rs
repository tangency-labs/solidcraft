//! Curve classification, part 2: assignment and reset of a classification, interval
//! insertion, merging and editing, boundary queries (zone tolerance, location, dead
//! topology), the UV ends of an interval and the entry checks of the classification
//! curve / edge intersection refinement onto surf/surf/surf points.
//!
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust                                                        | Upstream                                              |
//! |-------------------------------------------------------------|-------------------------------------------------------|
//! | [`CurveClassification::assign_from`]                        | `SmCurveClassification::operator=`                    |
//! | [`CurveClassification::point_obj_zone_tol_3d`]              | `GetPointObjZoneTol3d`                                |
//! | [`CurveClassification::has_dead_topology_point_object`]     | `HasDeadTopologyPointObject`                          |
//! | [`CurveClassification::classify_crv_loc_by_index`]          | `GetClassifyCrvLoc(ULONG, double*)`                   |
//! | [`CurveClassification::insert_at`]                          | `InsertAt`                                            |
//! | [`CurveClassification::reset`], [`ResetOptions`]            | `ReSet`                                               |
//! | [`CurveClassification::connect_intervals`]                  | `ConnectIntervals`                                    |
//! | [`CurveClassification::update_point_class`]                 | `UpdatePointClass`                                    |
//! | [`CurveClassification::edit_interval_min`]                  | `EditIntervalMin`                                     |
//! | [`CurveClassification::edit_interval_max`]                  | `EditIntervalMax`                                     |
//! | [`CurveClassification::interval_min_uv`]                    | `SmCurveInterval::GetMinUV`                           |
//! | [`CurveClassification::interval_max_uv`]                    | `SmCurveInterval::GetMaxUV`                           |
//! | [`refine_cci_to_surfaces`], [`RefineCci`]                   | `sm_RefineCCIToSurfaces`                              |
//! | [`refine_cci_to_surfaces_in`]                               | `sm_RefineCCIToSurfaces`, through an adapter          |
//!
//! Deviations:
//! - **Handles.** As in part 1, every object is a caller handle `O` resolved through a
//!   [`ClassGeometry`]. `operator=` copies the curve handle also for an owned curve
//!   (upstream deep-copies it; the copy has the same geometry) when `SmCurve::Copy`
//!   succeeds ([`Curve::copy`](crate::curve::curve::p2::Curve::copy)); when it fails
//!   (status ignored: `SmHermiteCurve`, `SmConic` and `SmIsoCurve` have no `Copy`) the
//!   curve stays what it was, `NULL` if the old curve was owned (freed). `m_cpContext` is
//!   not stored. `SmCurveInterval::GetMinUV` / `GetMaxUV` reach the classification through
//!   the interval's back pointer, which is not ported: they are members of the
//!   classification taking the interval index.
//! - **Borrowed interval storage** (`ReSet`'s `nDataSize` / `pOptBorrowedData`) is a
//!   memory-management feature that is not modelled (see part 1): [`ResetOptions`] has no
//!   such field, and a reset with a new curve always owns a fresh array.
//! - **Out-params** become return values: `(point, parameter)` of
//!   [`CurveClassification::classify_crv_loc_by_index`], the UV point of
//!   [`CurveClassification::interval_min_uv`] (the caller's `uv` when upstream leaves it
//!   unwritten), [`RefineCci`].
//! - **Undefined behaviour upstream** is an error here (`InvalidInput`): boundary or
//!   interval indices outside the classification, a `NULL` curve where it is
//!   dereferenced, a reset with a new curve but no interval, and `InsertAt(0, ..)` into a
//!   non-empty classification (upstream's unsigned shift loop `ii >= 0` never ends and
//!   reads before the array; see `plan/upstream-issues.md`).
//! - **`EditIntervalMin` reading past the end.** When collapsing the neighbour removes
//!   the interval before the last one, upstream reads the target interval at its old
//!   index, which is now the stale slot past `m_lSize` (a copy of the last interval, as
//!   `RemoveAt` leaves it), and its `CollapseInterval` then fails (status ignored). This
//!   port reads the copy of the last interval the same way.
//! - **`sm_RefineCCIToSurfaces`** is ported through its no-work gate:
//!   [`refine_cci_to_surfaces_in`] resolves the edge, its faces, edge uses, UV trim
//!   curves and Boolean mate through the adapter (`sm_GetCCIGeometry`), measures the
//!   gaps (`sm_GetCCISurfaceGaps`) and returns unchanged when they are within the edge
//!   zone tolerance / 2000 or a point fails to drop, as upstream. The large-gap tail
//!   (the surf/surf/surf, surface/edge and edge/surface refinements, which rewrite the
//!   classification curve, the edge curve and its UV trim curves and then recurse)
//!   needs the mutable B-rep and stays `UNPORTED(intersect.curve_class_p2)`; it recurses
//!   through upstream's own counter (`lRecursionCount`, at most 5 levels), which
//!   [`RefineCci::recursion_count`] carries. [`refine_cci_to_surfaces`], which has no
//!   adapter, still defers at the edge.
//! - **Topology callees** return [`SolidError::Unported`]: `SmEdge::GetCurve`
//!   (`topology.edge`) and `SmFace::GetSurface` (`topology.face_p2`) are called on
//!   opaque edge / face handles that carry no B-rep the port can reach (as in part 3
//!   and `intersect::point_class`: the stubs wait for handle-to-B-rep resolution, not
//!   for those units); `SmBrep::IsLiveTopologyMember` (`topology.brep_p1`, not ported)
//!   and the topology zone tolerances of point classifications (through
//!   `intersect.point_class`).
//! - **Recursion.** None here (see `sm_RefineCCIToSurfaces` above).

use crate::curve::curve::p2::CurveKind;
use crate::error::{ignore_status, ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{Solution, SolutionType};
use crate::geom::tol::{are_same, xsect_tol_3d_from_zones, zone_tol3d, UNINIT_TOL, ZONE_TOL_3D};
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, PointClassType, PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::brep::p1::is_live_topology_member;
use crate::topology::face::p2 as face_p2;
use crate::topology::Brep;
use crate::EntityRef;

use super::p1::{
    xsect_tol_2d_or_3d_for_classify_pair, CurveClassification, CurveInterval, IntervalPosition,
};
use super::p3::curve_is_degenerate;
use super::p7::NoObject;

const OP_CC: &str = "intersect::curve_class::p2::CurveClassification";
const OP_INSERT: &str = "intersect::curve_class::p2::insert_at";
const OP_RESET: &str = "intersect::curve_class::p2::reset";
const OP_CONNECT: &str = "intersect::curve_class::p2::connect_intervals";
const OP_EDIT: &str = "intersect::curve_class::p2::edit_interval";
const OP_UV: &str = "intersect::curve_class::p2::interval_uv";

/// `lMaxRecursion` of `sm_RefineCCIToSurfaces`.
const MAX_REFINE_RECURSION: u64 = 5;

/// The arguments of [`CurveClassification::reset`] (`ReSet`), with upstream's defaults
/// from [`Default`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResetOptions<O> {
    /// `cpOptCurve` (default `NULL`): with `uv_curve`, `None` for both keeps the current
    /// curve and only resets the intervals.
    pub curve: Option<O>,
    /// `cpOptInterval` (default `NULL`): the interval of the new curve, required with a
    /// new curve.
    pub interval: Option<Extent1d>,
    /// `cpOptUVCurve` (default `NULL`).
    pub uv_curve: Option<O>,
    /// `sZoneTol3d`, an `SmZoneTol3d` (default `SM_ZONE_TOL_3D`): the zone tolerance of
    /// the curve's source, given to the first interval's point classifications.
    pub zone_tol_3d: f64,
    /// `bIsCurveOwned` (default `FALSE`).
    pub is_curve_owned: bool,
    /// `bIsCurveCurrent` (default `TRUE`).
    pub is_curve_current: bool,
    /// `cpSurface1` (default `NULL`).
    pub surface1: Option<O>,
    /// `cpSurface2` (default `NULL`).
    pub surface2: Option<O>,
    /// `cpUVCurve2` (default `NULL`).
    pub uv_curve2: Option<O>,
    /// `cpTI` (default `NULL`).
    pub ti: Option<O>,
}

impl<O> Default for ResetOptions<O> {
    fn default() -> Self {
        Self {
            curve: None,
            interval: None,
            uv_curve: None,
            zone_tol_3d: ZONE_TOL_3D,
            is_curve_owned: false,
            is_curve_current: true,
            surface1: None,
            surface2: None,
            uv_curve2: None,
            ti: None,
        }
    }
}

/// The result of [`refine_cci_to_surfaces`] (its out-params; the status is always
/// `SM_SUCCESS` when no error is returned).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefineCci {
    /// `bSSSPointFlag`: use the surf/surf/surf intersection point.
    pub sss_point_flag: bool,
    /// `vSSSPoint` out: the surf/surf/surf point (the caller's value unless set).
    pub sss_point: Point3,
    /// `dDeviation`: the largest curve / surf/surf/surf point distance.
    pub deviation: f64,
    /// `lRecursionCount` out: the recursion counter after the call.
    pub recursion_count: u64,
}

impl<O: ClassObject> CurveClassification<O> {
    /// Makes this classification a copy of `other` (`operator=`): every member, the
    /// target loops and the intervals (the interval array is sized with
    /// [`CurveClassification::set_size`], so an empty `other` frees it). An owned curve
    /// of `other` is deep-copied upstream: its handle is copied when the curve can be
    /// copied, else (`SmCurve::Copy` fails, status ignored) this classification keeps its
    /// curve, or none when it owned it (upstream freed it first).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when `other` owns a handle that is not a curve; those
    /// of [`CurveClassification::set_size`] (none for a valid `other`).
    pub fn assign_from(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &Self,
    ) -> Result<()> {
        // when needed - free old curve
        if self.is_curve_owned && self.curve.is_some() {
            self.curve = None;
        }

        // copy member values
        self.is_curve_owned = other.is_curve_owned;
        match other.curve {
            Some(c) if self.is_curve_owned => {
                // deep copy (the handle stands for the copy); on failure m_cpCurve is
                // left unchanged (status ignored)
                let curve = geometry.curve(c).ok_or(SolidError::invalid_input(
                    OP_CC,
                    "owned classification curve is not a curve",
                ))?;
                if ignored(curve.copy(k))?.is_some() {
                    self.curve = Some(c);
                }
            }
            _ => self.curve = other.curve,
        }

        self.uv_curve1 = other.uv_curve1;
        self.is_curve_current = other.is_curve_current;
        self.surface1 = other.surface1;
        self.surface2 = other.surface2;
        self.doing_cross_hatch = other.doing_cross_hatch;
        self.doing_fillets = other.doing_fillets;
        self.ok_to_set_tolerance = other.ok_to_set_tolerance;
        self.tgt_loops_only = other.tgt_loops_only;
        self.tgt_loops.clear();
        self.tgt_loops.extend_from_slice(&other.tgt_loops);
        self.other_classification = other.other_classification;
        self.face = other.face;
        self.uv_curve2 = other.uv_curve2;
        self.ti = other.ti;

        self.src_zone_tol_3d = other.src_zone_tol_3d;

        self.interval = other.interval;
        self.set_size(k, other.data.len())?;
        for (dst, src) in self.data.iter_mut().zip(&other.data) {
            *dst = *src;
        }
        self.ignore_tangency = other.ignore_tangency;

        // all done
        Ok(())
    }

    /// The zone tolerance of the object of the point classification at interval
    /// boundary `index` (`GetPointObjZoneTol3d`).
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::point_class_by_index`] and
    /// [`PointClassification::obj_zone_tol_3d`].
    pub fn point_obj_zone_tol_3d(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
    ) -> Result<f64> {
        let (point_class, _) = self.point_class_by_index(index)?;

        // all done: return the object's zone tolerance
        point_class.obj_zone_tol_3d(k, geometry)
    }

    /// Whether a vertex, edge or face of an interval boundary classification is no
    /// longer live topology of `brep` (`HasDeadTopologyPointObject`); false for no brep
    /// and for an empty classification. The objects resolve through
    /// [`ClassGeometry::topology_entity`]; one of another B-rep is not a member of `brep`.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for a topology object the adapter cannot resolve to a
    /// B-rep (`SmBrep::IsLiveTopologyMember`, unit `topology.brep_p1`).
    pub fn has_dead_topology_point_object(
        &self,
        geometry: &impl ClassGeometry<O>,
        brep: Option<&Brep>,
    ) -> Result<bool> {
        let Some(brep) = brep else {
            return Ok(false);
        };
        if self.size() == 0 {
            return Ok(false); // empty classification has no interval boundaries to inspect
        }

        // point classifications: 0..IvlCnt inclusive
        for i in 0..=self.size() {
            let (point_class, _) = self.point_class_by_index(i)?;
            let Some(obj) = point_class.object() else {
                continue;
            };
            let obj_class = point_class.point_class();
            if obj_class == PointClassType::Vertex
                || obj_class == PointClassType::Edge
                || obj_class == PointClassType::Face
            {
                let (b, e) = geometry.topology_entity(obj).ok_or(SolidError::unported(
                    "SmBrep::IsLiveTopologyMember",
                    "topology.brep_p1",
                ))?;
                if !(std::ptr::eq(b, brep) && is_live_topology_member(brep, Some(e))) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// The location of the point classification at interval boundary `index` and its
    /// curve parameter (`GetClassifyCrvLoc(ULONG, double*)`): its surf/surf/surf point
    /// when set, else the curve point.
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::point_class_by_index`] and
    /// [`CurveClassification::classify_crv_loc`].
    pub fn classify_crv_loc_by_index(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
    ) -> Result<(Point3, f64)> {
        let (point_class, param) = self.point_class_by_index(index)?;

        // all done: return SSS point if possible else m_cpCurve(dCurveParameter)
        Ok((
            self.classify_crv_loc(k, geometry, point_class, param)?,
            param,
        ))
    }

    /// Inserts `interval` at `index` (`InsertAt`): past the end the classification
    /// grows to `index + 1` intervals (the new ones in between as
    /// [`CurveClassification::set_size`] makes them), else the intervals from `index` on
    /// move up by one.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for `index == 0` in a non-empty classification
    /// (upstream's shift loop never ends) and for more than
    /// [`MAX_INTERVALS`](super::MAX_INTERVALS) intervals.
    pub fn insert_at(
        &mut self,
        k: &Kernel,
        index: usize,
        interval: CurveInterval<O>,
    ) -> Result<()> {
        let size = self.size();
        if index >= size {
            // adding after the end of the array: grow so nIndex is valid
            let new_size = index
                .checked_add(1)
                .ok_or(SolidError::invalid_input(OP_INSERT, "index too large"))?;
            self.set_size(k, new_size)?;
        } else {
            // inserting in the middle of the array
            if index == 0 {
                // for(ii = m_lSize-1-1; ii >= 0; ii--) on an unsigned index never ends
                return Err(SolidError::invalid_input(
                    OP_INSERT,
                    "insert_at(0) into a non-empty classification",
                ));
            }
            self.set_size(k, size + 1)?; // grow it to new size

            // shift old data up to fill gap
            let mut i1 = size;
            while i1 > index {
                let src = *self.at(i1 - 1)?;
                *self.at_mut(i1)? = src;
                i1 -= 1;
            }

            // re-init slots we copied from
            self.at_mut(index)?.reset(k);
        }

        // insert new value in the gap
        *self.at_mut(index)? = interval;
        Ok(())
    }

    /// Resets the classification (`ReSet`). With a new curve or UV curve every member is
    /// set as the classification constructor does from `options` and the intervals
    /// become one unclassified interval over `options.interval`; without, only the
    /// curve-current flag changes and an allocated interval array becomes one
    /// unclassified interval over the current interval. The point classifications of
    /// that interval take `options.zone_tol_3d`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a new curve without `options.interval` (nothing
    /// is changed).
    pub fn reset(&mut self, k: &Kernel, options: &ResetOptions<O>) -> Result<()> {
        let is_new_curve = options.curve.is_some() || options.uv_curve.is_some();
        // (checked before any change, so the error leaves the classification unchanged;
        // upstream dereferences the missing interval)
        if is_new_curve && options.interval.is_none() {
            return Err(SolidError::invalid_input(
                OP_RESET,
                "a new curve needs its interval",
            ));
        }

        // set the modified curve bit
        self.is_curve_current = options.is_curve_current;

        let zone_tol_3d = options.zone_tol_3d;

        // when new curve is given - reset to default constructor state
        if let Some(interval) = options.interval.filter(|_| is_new_curve) {
            // remove existing curve and data
            self.data.clear();
            self.allocated = false;

            // set new values
            self.is_curve_owned = options.is_curve_owned;
            self.curve = options.curve;
            self.src_zone_tol_3d = if zone_tol_3d > 0.0 {
                zone_tol_3d
            } else {
                zone_tol3d(k, UNINIT_TOL)
            };

            self.uv_curve1 = options.uv_curve;
            self.is_curve_current = options.is_curve_current;
            self.surface1 = options.surface1;
            self.surface2 = options.surface2;
            self.doing_cross_hatch = false;
            self.doing_fillets = false;
            // (m_bOKToSetTolerance propagates through resets)
            self.tgt_loops_only = false;
            self.tgt_loops.clear();
            self.other_classification = None;

            self.uv_curve2 = options.uv_curve2;
            self.ti = options.ti;

            self.interval = interval;
            // SetSize(SM_MIN_ARRAY_SIZE), then m_lSize = 1
            self.allocated = true;
            self.ignore_tangency = false;

            // init first curve interval
            let first = self.first_interval(k, &interval, zone_tol_3d);
            self.data.push(first);
        } else {
            // reset the classification to 1 unclassified interval when mem has already
            // been allocated
            if self.allocated {
                let interval = self.interval;
                let first = self.first_interval(k, &interval, zone_tol_3d);
                self.data.clear();
                self.data.push(first);
            }
        }
        Ok(())
    }

    /// `SmCurveInterval(interval, ctx, this)` with its point classifications' source zone
    /// tolerance set to `zone_tol_3d`.
    fn first_interval(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        zone_tol_3d: f64,
    ) -> CurveInterval<O> {
        let mut first = CurveInterval::with_interval(k, interval, Some(self));
        first.start.set_src_zone_tol_3d(zone_tol_3d);
        first.mid.set_src_zone_tol_3d(zone_tol_3d);
        first.end.set_src_zone_tol_3d(zone_tol_3d);
        first
    }

    /// Removes interval `index` by squeezing its `end_to_squeeze` end out
    /// (`ConnectIntervals`): an end interval squeezed at the curve end is deleted and the
    /// classified interval shortened (only with `allow_removal_of_ends`; otherwise the
    /// other end is squeezed), else the neighbour on the squeezed side takes over the
    /// interval's range and far end classification. Positions other than
    /// [`IntervalPosition::Start`] and [`IntervalPosition::End`] change nothing.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `index >= size()`.
    pub fn connect_intervals(
        &mut self,
        index: usize,
        end_to_squeeze: IntervalPosition,
        allow_removal_of_ends: bool,
    ) -> Result<()> {
        // check state - lIntervalIndex within bounds
        let size = self.size();
        if index >= size {
            return Err(SolidError::sm_err(OP_CONNECT));
        }
        let mut end_to_squeeze = end_to_squeeze;

        // If we don't allow removal of ends then swap the end to squeeze
        if !allow_removal_of_ends {
            if end_to_squeeze == IntervalPosition::Start && index == 0 {
                end_to_squeeze = IntervalPosition::End;
            }
            if end_to_squeeze == IntervalPosition::End && index == size - 1 {
                end_to_squeeze = IntervalPosition::Start;
            }
        }

        // First take care of easy situation where squeezePoint is at the CurveEnd
        if (end_to_squeeze == IntervalPosition::Start && index == 0)
            || (end_to_squeeze == IntervalPosition::End && index == size - 1)
        {
            // Remove the interval
            self.remove_at(index)?;

            // Now shorten m_vInterval when any other intervals are left
            if let (Some(first), Some(last)) = (self.data.first(), self.data.last()) {
                let (min, max) = (first.interval.min(), last.interval.max());
                ignore_status(self.interval.set_min_max(min, max))?;
            }
        }
        // merge with previous interval
        else if end_to_squeeze == IntervalPosition::Start {
            let ivl = *self.at(index)?;
            let prev = self.at_mut(index - 1)?;
            prev.interval.add_value(ivl.interval.max());
            prev.end = ivl.end;
            self.remove_at(index)?;
        }
        // merge with next interval
        else if end_to_squeeze == IntervalPosition::End {
            let ivl = *self.at(index)?;
            let next = self.at_mut(index + 1)?;
            next.interval.add_value(ivl.interval.min());
            next.start = ivl.start;
            self.remove_at(index)?;
        }

        // all done
        Ok(())
    }

    /// Sets the point classification at interval boundary `index` and its mate (see
    /// [`CurveClassification::point_class_mate_by_index`]) to `new_values`
    /// (`UpdatePointClass`).
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::point_class_by_index`] and
    /// [`CurveClassification::point_class_mate_by_index`]; the boundary classification
    /// is set by then.
    pub fn update_point_class(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        new_values: &PointClassification<O>,
    ) -> Result<()> {
        // copy rNewValues into PointClassification
        *self.point_class_by_index_mut(index)?.0 = *new_values;

        // copy rNewValues into corresponding PC in adjacent interval: the mate is the
        // end of an interval
        if let Some(i) = self.point_class_mate_interval(k, geometry, index)? {
            self.at_mut(i)?.end = *new_values;
        }

        // all done
        Ok(())
    }

    /// Moves the start of interval `index` to `new_param` and the end of its predecessor
    /// with it (`EditIntervalMin`; upstream default `collapse_degen_intervals = TRUE`).
    /// With `collapse_degen_intervals`, the predecessor and then the interval are
    /// collapsed ([`CurveClassification::collapse_interval`], status ignored) when
    /// degenerate within the pair tolerance of their ends.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `index >= size()` or `new_param` outside
    /// `[previous min (or the classified interval's min), max]`;
    /// [`SolidError::InvalidInput`] for no curve; those of the tolerances.
    pub fn edit_interval_min(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        new_param: f64,
        collapse_degen_intervals: bool,
    ) -> Result<()> {
        // check input
        if index >= self.size() {
            return Err(SolidError::sm_err(OP_EDIT));
        }

        // Get allowed displacement range for the new Param value
        let lo = if index > 0 {
            self.at(index - 1)?.interval.min()
        } else {
            self.interval.min()
        };
        let allowed = Extent1d::from_min_max(lo, self.at(index)?.interval.max());

        // check input - new MinParam Value is in allowed range
        if !allowed.contains_value(new_param, 0.0) {
            return Err(SolidError::sm_err(OP_EDIT));
        }

        // modify the target interval boundary position
        let moved = self.at_mut(index)?.set_interval_min(new_param).is_ok();

        // when there is a neighbor that needs to be updated
        if moved && index > 0 {
            // modifiy the matching neighbor interval boundary position (status ignored)
            ignore_status(self.at_mut(index - 1)?.set_interval_max(new_param))?;

            let tol = self.neighbour_tol(k, geometry, index - 1)?;

            // if the neighbor interval has shrunk to zero length
            if collapse_degen_intervals {
                let curve = self.curve_geometry(geometry)?;
                if curve_is_degenerate(k, Some(curve), &self.at(index - 1)?.interval, tol)? {
                    // remove the interval from the SmCurveClassification (status ignored)
                    ignore_status(self.collapse_interval(k, geometry, index - 1))?;
                }
            }
        }

        // when target interval has shrunk to zero length; past the end after a collapse,
        // upstream reads the stale slot, a copy of the last interval
        let ivl = match self.data.get(index).or_else(|| self.data.last()) {
            Some(ivl) => *ivl,
            None => return Err(SolidError::invalid_input(OP_EDIT, "no intervals left")),
        };
        self.collapse_if_degenerate(k, geometry, index, &ivl, collapse_degen_intervals)?;

        // all done
        Ok(())
    }

    /// Moves the end of interval `index` to `new_param` and the start of its successor
    /// with it (`EditIntervalMax`; upstream default `collapse_degen_intervals = TRUE`).
    /// With `collapse_degen_intervals`, the successor and then the interval are
    /// collapsed when degenerate within the pair tolerance of their ends.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `index >= size()` or `new_param` outside
    /// `[min, next max (or the classified interval's max)]`;
    /// [`SolidError::InvalidInput`] for no curve; those of the tolerances.
    pub fn edit_interval_max(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        new_param: f64,
        collapse_degen_intervals: bool,
    ) -> Result<()> {
        // check input
        let size = self.size();
        if index >= size {
            return Err(SolidError::sm_err(OP_EDIT));
        }

        // Get allowed displacement range for the new Param value
        let hi = if index < size - 1 {
            self.at(index + 1)?.interval.max()
        } else {
            self.interval.max()
        };
        let allowed = Extent1d::from_min_max(self.at(index)?.interval.min(), hi);

        // check input - new MaxParam Value is in allowed range
        if !allowed.contains_value(new_param, 0.0) {
            return Err(SolidError::sm_err(OP_EDIT));
        }

        // modify the target interval boundary position
        let moved = self.at_mut(index)?.set_interval_max(new_param).is_ok();

        // when there is a neighbor that needs to be updated
        if moved && index < size - 1 {
            // modifiy the matching neighbor interval boundary position (status ignored)
            ignore_status(self.at_mut(index + 1)?.set_interval_min(new_param))?;

            let tol = self.neighbour_tol(k, geometry, index + 1)?;

            // if the neighbor interval has shrunk to zero length
            if collapse_degen_intervals {
                let curve = self.curve_geometry(geometry)?;
                if curve_is_degenerate(k, Some(curve), &self.at(index + 1)?.interval, tol)? {
                    // remove the interval from the SmCurveClassification (status ignored)
                    ignore_status(self.collapse_interval(k, geometry, index + 1))?;
                }
            }
        }

        // when target interval has shrunk to zero length
        let ivl = *self.at(index)?;
        self.collapse_if_degenerate(k, geometry, index, &ivl, collapse_degen_intervals)?;

        // all done
        Ok(())
    }

    /// The collapse tolerance of neighbour interval `neighbour` in `EditIntervalMin` /
    /// `EditIntervalMax`: for a 3D curve the intersection tolerance of its end objects'
    /// zone tolerances, for a 2D curve the pair tolerance of the first interval's ends
    /// (as upstream).
    fn neighbour_tol(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        neighbour: usize,
    ) -> Result<f64> {
        let curve = self.curve_geometry(geometry)?;
        if curve.dim() == 3 {
            let ivl = self.at(neighbour)?;
            Ok(xsect_tol_3d_from_zones(
                k,
                ivl.start.obj_zone_tol_3d(k, geometry)?,
                ivl.end.obj_zone_tol_3d(k, geometry)?,
            ))
        } else {
            // the curve being classified is in 2d
            let first = self.at(0)?;
            xsect_tol_2d_or_3d_for_classify_pair(
                k,
                curve,
                self.surface1_geometry(geometry)?,
                first.interval.min(),
                first.start.obj_zone_tol_3d(k, geometry)?,
                first.interval.max(),
                first.end.obj_zone_tol_3d(k, geometry)?,
            )
        }
    }

    /// The tail of `EditIntervalMin` / `EditIntervalMax`: the pair tolerance of `ivl`'s
    /// ends and, with `collapse`, `CollapseInterval(index)` (status ignored) when `ivl`
    /// is degenerate within it.
    fn collapse_if_degenerate(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        ivl: &CurveInterval<O>,
        collapse: bool,
    ) -> Result<()> {
        let curve = self.curve_geometry(geometry)?;
        let tol = xsect_tol_2d_or_3d_for_classify_pair(
            k,
            curve,
            self.surface1_geometry(geometry)?,
            ivl.interval.min(),
            ivl.start.obj_zone_tol_3d(k, geometry)?,
            ivl.interval.max(),
            ivl.end.obj_zone_tol_3d(k, geometry)?,
        )?;
        if collapse && curve_is_degenerate(k, Some(curve), &ivl.interval, tol)? {
            // remove the interval from the SmCurveClassification (status ignored)
            ignore_status(self.collapse_interval(k, geometry, index))?;
        }
        Ok(())
    }

    /// The UV point of the start of interval `index` on the classified face's surface
    /// (`SmCurveInterval::GetMinUV`): the point of a 2D curve, the UV point of a curve on
    /// a surface (the caller's `uv` when that evaluation fails, its status being
    /// ignored), else the curve point dropped to the face's surface.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for `index >= size()` or no curve; `SM_ERR` for a 3D
    /// curve without a face or face surface; [`SolidError::Unported`] for a face the
    /// adapter cannot resolve to a B-rep ([`ClassGeometry::topology_entity`];
    /// `SmFace::GetSurface`, unit `topology.face_p2`); those of the surface drop.
    pub fn interval_min_uv(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        uv: Point2,
    ) -> Result<Point2> {
        let param = self.at(index)?.interval.min();
        self.interval_uv_at(k, geometry, param, uv)
    }

    /// The UV point of the end of interval `index` (`SmCurveInterval::GetMaxUV`; see
    /// [`CurveClassification::interval_min_uv`]).
    ///
    /// # Errors
    ///
    /// As [`CurveClassification::interval_min_uv`].
    pub fn interval_max_uv(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        uv: Point2,
    ) -> Result<Point2> {
        let param = self.at(index)?.interval.max();
        self.interval_uv_at(k, geometry, param, uv)
    }

    /// `GetMinUV` / `GetMaxUV` at curve parameter `param`.
    fn interval_uv_at(
        &self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        param: f64,
        uv: Point2,
    ) -> Result<Point2> {
        let curve = self.curve_geometry(geometry)?;

        // set output - UV point
        if curve.dim() == 2 {
            // (status ignored: an undefined point on failure)
            let p = ignored(curve.evaluate_point(k, param))?.unwrap_or(Vec3::UNDEF);
            return Ok(Point2::new(p.x, p.y));
        }
        if let CurveKind::CrvOnSurf(c) = curve.kind() {
            // (status ignored: rUV unchanged on failure)
            return Ok(ignored(c.evaluate_uv_point(k, param))?.unwrap_or(uv));
        }

        // Curve is 3d branch - Use drop point
        let Some(face) = self.face else {
            // check state - the surface must be given
            return Err(SolidError::sm_err(OP_UV));
        };
        let surface = match geometry.topology_entity(face) {
            Some((b, EntityRef::Face(f))) => face_p2::surface(b, f)?,
            Some(_) => {
                return Err(SolidError::invalid_input(
                    OP_UV,
                    "face handle is not a face",
                ))
            }
            None => {
                return Err(SolidError::unported(
                    "SmFace::GetSurface",
                    "topology.face_p2",
                ))
            }
        };
        let surface = surface.ok_or(SolidError::sm_err(OP_UV))?;

        // (EvaluatePoint's status is ignored upstream: an undefined point on failure)
        let point = ignored(curve.evaluate_point(k, param))?.unwrap_or(Vec3::UNDEF);
        let drop = surface.drop_point::<NoObject>(
            k,
            point,
            &surface.natural_uv_domain(),
            &DropPointOptions::default(),
        )?;
        Ok(drop.result_uv)
    }
}

/// Tightens a classification-curve / `edge` intersection `solution` onto its surfaces
/// (`sm_RefineCCIToSurfaces`). The recursion counter is bumped first and nothing is
/// done past `MAX_REFINE_RECURSION` (5) levels; nothing is done either for a solution
/// that is not a point (a range whose curve parameters differ at both ends) or for a
/// classification without both its surfaces. `sss_point` is the caller's value of the
/// output point.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for no edge where the refinement starts (upstream
/// dereferences it); [`SolidError::Unported`] from there on: the edge's curve
/// (`SmEdge::GetCurve`, unit `topology.edge`) comes first.
pub fn refine_cci_to_surfaces<O: ClassObject, S: Copy>(
    classification: &CurveClassification<O>,
    solution: &Solution<S>,
    edge: Option<O>,
    sss_point: Point3,
    recursion_count: u64,
) -> Result<RefineCci> {
    // init outputs
    let out = RefineCci {
        sss_point_flag: false,
        sss_point,
        deviation: 0.0,
        recursion_count: recursion_count.saturating_add(1),
    };

    if recursion_count > MAX_REFINE_RECURSION {
        return Ok(out); // just return with no changes.
    }

    // no work case - Not an intersection curve point solution
    let point_solution = solution.solution_type == SolutionType::SingleValue
        || are_same(solution.start.param(0)?, solution.end.param(0)?)
        || are_same(solution.start.param(1)?, solution.end.param(1)?);
    if !point_solution || classification.surface1().is_none() || classification.surface2().is_none()
    {
        return Ok(out);
    }

    // get ClassCurve and EdgeCurve xsect points (pEdge->GetCurve() comes first)
    if edge.is_none() {
        return Err(SolidError::invalid_input(OP_CC, "no edge"));
    }
    // this entry point has no adapter to resolve the edge ([`refine_cci_to_surfaces_in`]
    // has one)
    // UNPORTED(topology.edge): SmEdge::GetCurve
    Err(SolidError::unported("SmEdge::GetCurve", "topology.edge"))
}

/// [`refine_cci_to_surfaces`] with an adapter: the edge, its faces, edge uses, UV trim
/// curves and Boolean mate resolve through `geometry`
/// ([`get_cci_geometry_in`](super::p1::get_cci_geometry_in)). When the classification
/// curve, the edge curve and their surfaces already meet within the no-work tolerance
/// (edge zone tolerance / 2000), or a point does not drop to a connected surface,
/// nothing changes, as upstream.
///
/// # Errors
///
/// [`SolidError::Unported`] when the gaps are large: upstream then moves the point to a
/// surf/surf/surf intersection and rewrites the classification curve, the edge curve
/// and its UV trim curves, which needs the mutable B-rep
/// (`UNPORTED(intersect.curve_class_p2)`); those of
/// [`get_cci_geometry_in`](super::p1::get_cci_geometry_in) and
/// [`get_cci_surface_gaps`](super::p1::get_cci_surface_gaps);
/// [`SolidError::InvalidInput`] for a classification without a curve or an edge
/// without one (upstream dereferences them).
pub fn refine_cci_to_surfaces_in<O: ClassObject, S: Copy>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    classification: &CurveClassification<O>,
    solution: &Solution<S>,
    edge: O,
    sss_point: Point3,
    recursion_count: u64,
) -> Result<RefineCci> {
    // init outputs
    let out = RefineCci {
        sss_point_flag: false,
        sss_point,
        deviation: 0.0,
        recursion_count: recursion_count.saturating_add(1),
    };

    if recursion_count > MAX_REFINE_RECURSION {
        return Ok(out); // just return with no changes.
    }

    // no work case - Not an intersection curve point solution
    let point_solution = solution.solution_type == SolutionType::SingleValue
        || are_same(solution.start.param(0)?, solution.end.param(0)?)
        || are_same(solution.start.param(1)?, solution.end.param(1)?);
    if !point_solution || classification.surface1().is_none() || classification.surface2().is_none()
    {
        return Ok(out);
    }

    // get ClassCurve and EdgeCurve xsect points
    let src_curve = classification.curve_geometry(geometry)?;
    let edge_curve = super::p3::edge_curve(geometry, edge)?
        .ok_or(SolidError::invalid_input(OP_CC, "edge without a curve"))?;

    let edge_zone_tol_3d =
        crate::intersect::point_class::object_zone_tol_3d(k, geometry, Some(edge))?;
    let no_work_xsect_tol_3d = edge_zone_tol_3d / 2000.0; // usually about 1e-8 = ApproxTol/1000 (2 gets ApproxTol)

    // (the statuses of both evaluations are ignored upstream)
    let src_curve_pt =
        ignored(src_curve.evaluate_point(k, solution.start.param(0)?))?.unwrap_or(Vec3::UNDEF);
    let edge_curve_pt =
        ignored(edge_curve.evaluate_point(k, solution.start.param(1)?))?.unwrap_or(Vec3::UNDEF);

    // get cci connected geometry and topology (sm_GetCCIGeometry always succeeds)
    let cci = super::p1::get_cci_geometry_in(geometry, classification, edge)?;

    // get CurvePoint/EdgePoint, CurvePoint/Surface, and EdgePoint/Surface gaps
    let gaps = super::p1::get_cci_surface_gaps(
        k,
        src_curve_pt,
        edge_curve_pt,
        solution,
        &cci.surfaces,
        &cci.uv_curves,
        cci.edge_surface_count,
    )?;

    // no work case - gaps are fine or current solution failed to project to all surfaces
    if !gaps.success
        || (gaps.curve_edge_gap < no_work_xsect_tol_3d
            && gaps.curve_surface_gap < no_work_xsect_tol_3d
            && gaps.edge_surface_gap < no_work_xsect_tol_3d)
    {
        return Ok(out);
    }

    // Part 2 - Fix Large Gaps: moves the point to a surf/surf/surf or surface/edge
    // intersection and rewrites the classification curve, the edge curve and its UV trim
    // curves (sm_ApplyPointSolution, SmEdgeuse::SetUVTrimCurve), then recurses.
    // UNPORTED(intersect.curve_class_p2): sm_RefineCCIToSurfaces (large-gap refinement
    // of the mutable B-rep's curves)
    Err(SolidError::unported(
        "sm_RefineCCIToSurfaces (large-gap refinement)",
        "intersect.curve_class_p2",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::p2::Curve;
    use crate::curve::line::Line;
    use crate::intersect::point_class::ClassObjectKind;
    use crate::surface::surface::Surface;
    use crate::surface::volume::Volume;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(usize, ClassObjectKind);

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            self.1
        }
    }

    struct G(Vec<Curve>);

    impl ClassGeometry<H> for G {
        fn curve(&self, o: H) -> Option<&Curve> {
            self.0.get(o.0)
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

    fn line() -> G {
        let c = Line::from_points(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), 3);
        G(vec![Curve::from(c)])
    }

    fn cc(k: &Kernel, bounds: &[f64]) -> CurveClassification<H> {
        let mut c = CurveClassification::new(k);
        c.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        c.set_size(k, bounds.len() - 1).ok();
        for (i, w) in bounds.windows(2).enumerate() {
            c.at_mut(i)
                .unwrap()
                .set_interval(&Extent1d::from_min_max(w[0], w[1]));
        }
        c.interval = Extent1d::from_min_max(bounds[0], *bounds.last().unwrap());
        c
    }

    #[test]
    fn insert_reset_assign() {
        let k = Kernel::default();
        let mut c = cc(&k, &[0.0, 0.5, 1.0]);
        let ivl = CurveInterval::with_interval(&k, &Extent1d::from_min_max(0.2, 0.3), None);
        assert!(c.insert_at(&k, 0, ivl).is_err());
        c.insert_at(&k, 1, ivl).unwrap();
        assert_eq!(c.size(), 3);
        assert_eq!(c.at(1).unwrap().min(), 0.2);
        assert_eq!(c.at(2).unwrap().min(), 0.5);
        c.insert_at(&k, 5, ivl).unwrap();
        assert_eq!(c.size(), 6);

        let mut d = CurveClassification::new(&k);
        d.assign_from(&k, &line(), &c).unwrap();
        assert_eq!(d, c);

        c.reset(&k, &ResetOptions::default()).unwrap();
        assert_eq!(c.size(), 1);
        assert_eq!(c.at(0).unwrap().start.src_zone_tol_3d(), ZONE_TOL_3D);
        let opts = ResetOptions {
            uv_curve: Some(H(0, ClassObjectKind::Curve)),
            zone_tol_3d: 0.0,
            ..ResetOptions::default()
        };
        assert!(c.reset(&k, &opts).is_err());
        let opts = ResetOptions {
            interval: Some(Extent1d::from_min_max(0.0, 2.0)),
            ..opts
        };
        c.reset(&k, &opts).unwrap();
        assert_eq!(c.curve(), None);
        assert_eq!(c.src_zone_tol_3d(), UNINIT_TOL);
        assert_eq!(c.interval().max(), 2.0);
    }

    #[test]
    fn assign_owned_curve_without_copy() {
        use crate::curve::hermite_curve::HermiteCurve;
        let k = Kernel::default();
        let mut g = line();
        let h = HermiteCurve::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            3,
        );
        g.0.push(Curve::from(h));
        let line_h = H(0, ClassObjectKind::Curve);
        let herm_h = H(1, ClassObjectKind::Curve);
        let mut src = CurveClassification::new(&k);
        src.set_curve(Some(herm_h), true);
        // SmHermiteCurve::Copy fails: a non-owned curve stays, an owned one is freed
        let mut d = CurveClassification::new(&k);
        d.set_curve(Some(line_h), false);
        d.assign_from(&k, &g, &src).unwrap();
        assert_eq!(d.curve(), Some(line_h));
        assert!(d.is_curve_owned());
        let mut d = CurveClassification::new(&k);
        d.set_curve(Some(line_h), true);
        d.assign_from(&k, &g, &src).unwrap();
        assert_eq!(d.curve(), None);
        // a line copies
        src.set_curve(Some(line_h), true);
        d.assign_from(&k, &g, &src).unwrap();
        assert_eq!(d.curve(), Some(line_h));
    }

    #[test]
    fn connect_and_edit() {
        let k = Kernel::default();
        let g = line();
        let mut c = cc(&k, &[0.0, 0.25, 0.5, 1.0]);
        assert!(c
            .connect_intervals(3, IntervalPosition::Start, true)
            .is_err());
        c.connect_intervals(1, IntervalPosition::End, true).unwrap();
        assert_eq!(c.size(), 2);
        assert_eq!(c.at(1).unwrap().min(), 0.25);
        c.connect_intervals(0, IntervalPosition::Start, true)
            .unwrap();
        assert_eq!(c.interval().min(), 0.25);

        let mut c = cc(&k, &[0.0, 0.25, 0.5, 1.0]);
        assert!(c.edit_interval_min(&k, &g, 1, 0.6, true).is_err());
        c.edit_interval_min(&k, &g, 1, 0.1, true).unwrap();
        assert_eq!(c.at(0).unwrap().max(), 0.1);
        c.edit_interval_max(&k, &g, 1, 1.0, true).unwrap();
        assert_eq!(c.size(), 2);
        // the moved start of the first interval collapses it onto the second
        let mut c = cc(&k, &[0.0, 0.25, 0.5, 1.0]);
        c.edit_interval_min(&k, &g, 2, 0.25, true).unwrap();
        assert_eq!(c.size(), 2);
    }

    #[test]
    fn queries() {
        let k = Kernel::default();
        let g = line();
        let c = cc(&k, &[0.0, 0.5, 1.0]);
        let (p, t) = c.classify_crv_loc_by_index(&k, &g, 2).unwrap();
        assert_eq!((p.x, t), (1.0, 1.0));
        assert!(c.classify_crv_loc_by_index(&k, &g, 3).is_err());
        assert!(!c
            .has_dead_topology_point_object(&g, Some(&Brep::new()))
            .unwrap());
        assert!(c
            .interval_min_uv(&k, &g, 0, Point2::new(0.0, 0.0))
            .is_err_and(|e| !e.is_unported()));
        let s: Solution<H> = Solution::new();
        let r = refine_cci_to_surfaces(&c, &s, None, Point3::new(1.0, 2.0, 3.0), 6).unwrap();
        assert_eq!(r.recursion_count, 7);
        assert_eq!(r.sss_point.y, 2.0);
    }
}
