//! Point sets and closed point sequences with lazily cached properties: centroid,
//! best-fit axes, axis-aligned and pseudo bounding boxes, enclosed area and
//! winding-number point containment.
//!
//! Upstream: `source/SMLib/src/SmPointSet.cpp`, `source/SMLib/inc/SmPointSet.h`.
//!
//! | Rust                                   | Upstream                                         |
//! |----------------------------------------|--------------------------------------------------|
//! | [`PointSet3d`]                         | `SmPointSet3d`                                   |
//! | [`PointSet3d::new`]                    | `SmPointSet3d(SmTArray<SmPoint3d>&, dTol3d, bPlanar, pOptNormal, bCopyData)` |
//! | [`PointSet3d::clear_cache`]            | `SmPointSet3d::ClearCache`                       |
//! | [`PointSet3d::points`]                 | `SmPointSet3d::GetPoints`                        |
//! | [`PointSet3d::centroid`]               | `SmPointSet3d::GetCentroid`                      |
//! | [`PointSet3d::normal`]                 | `SmPointSet3d::GetNormal()`                      |
//! | [`PointSet3d::bbox`]                   | `SmPointSet3d::GetBBox`                          |
//! | [`PointSet3d::tight_pbox`]             | `SmPointSet3d::GetTightPBox`                     |
//! | `set_up_axes` (private)                | `SmPointSet3d::SetUpAxes`                        |
//! | `set_up_centroid` (private)            | `SmPointSet3d::SetUpCentroid`                    |
//! | `set_up_bboxes` (private)              | `SmPointSet3d::SetUpBBoxes`                      |
//! | `set_up_tight_pseudo_box` (private)    | `SmPointSet3d::SetUpTightPseudoBox`              |
//! | [`DataRequest`]                        | `SmPointSetDataType` (the two box requests)      |
//! | [`PointSequence`]                      | `SmPointSequence`                                |
//! | [`PointSequence::new`]                 | `SmPointSequence(SmTArray<SmPoint3d>&, dTol3d, bClosed, bPlanar, pOptNormal, bCopyData)` |
//! | [`PointSequence::area`]                | `SmPointSequence::GetArea`                       |
//! | `set_up_area` (private)                | `SmPointSequence::SetUpArea`                     |
//! | `calc_quad_num` (private)              | `SmPointSequence::CalcQuadNum`                   |
//! | [`PointSequence::point_containment`]   | `SmPointSequence::PointContainment`              |
//! | [`PointContainment`]                   | `SmPointObjectContainmentType` (`SmTypes.h`)     |
//!
//! The other members of `SmPointSet.cpp` (`SetUpData`, `SetUpPointSetType`,
//! `SetUpDeviation`, coincidence/monotonicity, `SmPointGrid`, copy/assignment/equality,
//! the empty constructors, `Draw`, `Dump`) are not reached by in-scope code
//! (`harness/callgraph`) and are not ported.
//!
//! Deviations:
//! - `SmPointSequence` derives from `SmPointSet3d`; here [`PointSequence`] holds a
//!   [`PointSet3d`] and dereferences to it.
//! - The point array is owned (`bCopyData` and the borrowed-memory flags disappear);
//!   the unused `bPlanar` argument is dropped.
//! - Upstream unitizes the caller's `pOptNormal` / `pOptInputPBoxZ` in place; those
//!   arguments are `Option<&mut Vec3>` and are updated the same way.
//! - `SetUpAxes` returns the point-set type as its status and every caller ignores it;
//!   the Rust functions return `Result` only to propagate errors of
//!   `smgu_BestFitPlaneToPoints`, which cannot happen for finite input.
//! - `CCWAngleBetween` failures (unreachable after its own parallel-vector checks)
//!   keep the previous angle, as upstream; the first "previous" angle is `0.0`
//!   (uninitialised upstream).
//! - The point-set type of a set built without a normal is
//!   [`PointSetType::Unknown`] until classified (uninitialised upstream; it is never
//!   read before then).
//! - `SetUpCentroid` sums all blocks and caches the result (WR-15), unlike
//!   upstream which drops the last block at positive multiples of 50.
//! - `SetUpAxes` calls `smgu_BestFitPlaneToPoints`, whose best-fit line is the
//!   least-squares line (`WR-10`), so collinear sets are [`PointSetType::Linear`].
//!   `SetUpBBoxes` runs `SetUpTightPseudoBox`'s hull-edge alignment for Linear sets as
//!   well as Planar ones: pinned v1.0.1 ran it only for Planar sets, and reached
//!   collinear sets only because its skewed line misclassified them as Planar. The
//!   alignment keeps a rotated box only when its base area is smaller, so it never
//!   loosens the box.
//! - `SetUpTightPseudoBox` calls the `const` `ProjectPointToPlane` on its boundary and
//!   candidate points and discards the results (`WR-14`). The port uses the projections,
//!   so the hull chords lie in the box plane. `CCWAngleBetween` projects as well, but
//!   for points far off the plane the unprojected chords' roundoff can decide an exact
//!   tie between two rotations the wrong way.
//! - The tolerance member `m_dDistTol3d` is an `SmTol3d`: the constructors store
//!   `|tol3d|` ([`crate::geom::tol::tol3d`]) as upstream's conversion does.
//! - `SM_DEBUG_CODE` blocks and `SM_DBG_WARN` messages are omitted.

use core::ops::{Deref, DerefMut};

use crate::error::Result;
use crate::geom::extent1d::ExtentPointType;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p1::{polygon_area, PointSetType};
use crate::geom::geom_utility::p2::{best_fit_plane_to_points, is_point_on_segment};
use crate::geom::pseudo_box::PseudoBox;
use crate::geom::tol::{is_zero, tol3d as to_tol3d};
use crate::geom::vector3d::{Point3, Vec3};
use crate::math::{max2, min2, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, PI};

/// Which bounding-box data a box query needs (`SmPointSetDataType`, the
/// `SM_PSD_BBOXES` and `SM_PSD_REFINE_PBOX` requests of `SetUpBBoxes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRequest {
    /// `SM_PSD_BBOXES`: the axis-aligned and pseudo boxes.
    BBoxes,
    /// `SM_PSD_REFINE_PBOX`: the pseudo box rotated about its normal to shrink it.
    RefinePBox,
}

/// Classification of a point against a closed point sequence
/// (`SmPointObjectContainmentType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointContainment {
    /// `SM_POC_UNKNOWN`: the winding count was inconsistent.
    Unknown = 0,
    /// `SM_POC_INSIDE`.
    Inside = 1,
    /// `SM_POC_OUTSIDE`.
    Outside = 2,
    /// `SM_POC_ON_BOUNDARY`: within tolerance of a point or segment.
    OnBoundary = 3,
}

impl PointContainment {
    /// Upstream enum value (`SM_POC_UNKNOWN` = 0 .. `SM_POC_ON_BOUNDARY` = 3).
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// An unordered set of 3d points with lazily computed, cached properties
/// (`SmPointSet3d`).
///
/// The coordinate system `[centroid, x_axis, y_axis, normal]` comes from the best
/// fit plane (or line) through the points, or from a normal given at construction.
#[derive(Debug, Clone, PartialEq)]
pub struct PointSet3d {
    points: Vec<Point3>,
    dist_tol3d: f64,
    /// Which cached values are valid (`m_b*Set` flags, bits [`Cache`]).
    cache: Cache,
    point_set_type: PointSetType,
    centroid: Point3,
    x_axis: Vec3,
    y_axis: Vec3,
    normal: Vec3,
    max_centroid_dev: f64,
    max_dev: f64,
    bbox: Extent3d,
    pbox: PseudoBox,
}

/// The `m_b*Set` cache-state flags of `SmPointSet3d`, as a bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cache(u8);

impl Cache {
    /// `m_bAxesSet`.
    const AXES: u8 = 1;
    /// `m_bNormalSet`.
    const NORMAL: u8 = 2;
    /// `m_bCentroidSet`.
    const CENTROID: u8 = 4;
    /// `m_bPointSetTypeSet`.
    const POINT_SET_TYPE: u8 = 8;
    /// `m_bMaxDevSet`.
    const MAX_DEV: u8 = 16;
    /// `m_bMaxCentroidDevSet`.
    const MAX_CENTROID_DEV: u8 = 32;
    /// `m_bBBoxSet`.
    const BBOX: u8 = 64;
    /// `m_bPBoxRefined`.
    const PBOX_REFINED: u8 = 128;

    const fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    fn set(&mut self, bit: u8) {
        self.0 |= bit;
    }
}

/// `v.MakeUnitOrthoVectors(pYRef, rX, rY, rZ)` with the status ignored, as the new
/// values of `[rX, rY, rZ]`. On failure the out-params upstream wrote before
/// returning ([`Vec3::unit_ortho_vectors_on_failure`]) replace those of `current`:
/// a zero-length `v` only sets `rX` to `(0, 0, 1)`; a `v` whose squared length
/// overflows sets `rX` and `rZ` and keeps `rY`.
fn unit_ortho(v: Vec3, y_ref: Option<Vec3>, current: [Vec3; 3]) -> [Vec3; 3] {
    if let Ok(frame) = v.make_unit_ortho_vectors(y_ref) {
        return frame;
    }
    let mut out = current;
    for (slot, w) in out.iter_mut().zip(v.unit_ortho_vectors_on_failure()) {
        if let Some(w) = w {
            *slot = w;
        }
    }
    out
}

impl PointSet3d {
    /// A point set on `points` with distinct-point tolerance `tol3d` (upstream
    /// default `SM_EFF_ZERO`) (`SmPointSet3d(rTArray, dTol3d, bPlanar, pOptNormal)`).
    ///
    /// A given `normal` is unitized in place; when it is not zero-length it becomes
    /// the cached normal and the set is classified planar (never checked against the
    /// points).
    #[must_use]
    pub fn new(points: Vec<Point3>, tol3d: f64, normal: Option<&mut Vec3>) -> Self {
        let mut s = Self {
            points,
            // the member is an `SmTol3d`, whose constructor takes `fabs`
            dist_tol3d: to_tol3d(tol3d),
            cache: Cache::default(),
            point_set_type: PointSetType::Unknown,
            // `SmVector3d` members default to SM_UNDEF_DOUBLE components
            centroid: Vec3::UNDEF,
            x_axis: Vec3::UNDEF,
            y_axis: Vec3::UNDEF,
            normal: Vec3::UNDEF,
            max_centroid_dev: 0.0,
            max_dev: 0.0,
            bbox: Extent3d::default(),
            pbox: PseudoBox::new(),
        };
        s.clear_cache();

        // Save user given normal - it never gets checked against the point set
        if let Some(n) = normal {
            let ok = n.length_squared() >= EFF_ZERO_SQ;
            *n = n.unitize_or_z();
            if ok {
                s.normal = *n;
                s.cache.set(Cache::NORMAL);
                s.point_set_type = PointSetType::Planar;
                s.cache.set(Cache::POINT_SET_TYPE);
            }
        }
        s
    }

    /// Forgets every cached property (`ClearCache`), e.g. after the points changed.
    pub fn clear_cache(&mut self) {
        self.cache = Cache::default();
    }

    /// The points (`GetPoints`).
    #[must_use]
    pub fn points(&self) -> &[Point3] {
        &self.points
    }

    /// The geometric centre: the cached best-fit centroid if axes were computed,
    /// otherwise the block-wise average, with every point included (WR-15).
    pub fn centroid(&mut self) -> Point3 {
        self.set_up_centroid();
        self.centroid
    }

    /// The unit normal of the set's coordinate system (`GetNormal()`): the given
    /// normal, the best-fit plane normal, or a normal to the best-fit line.
    ///
    /// # Errors
    ///
    /// Only from the best-fit plane (not for finite points).
    pub fn normal(&mut self) -> Result<Vec3> {
        self.set_up_axes()?;
        Ok(self.normal)
    }

    /// The axis-aligned bounding box (`GetBBox`). `pbox_z`, when given, is unitized in
    /// place and fixes the third basis vector of the cached pseudo box.
    ///
    /// # Errors
    ///
    /// Only from the best-fit plane (not for finite points).
    pub fn bbox(&mut self, pbox_z: Option<&mut Vec3>) -> Result<Extent3d> {
        self.set_up_bboxes(DataRequest::BBoxes, pbox_z)?;
        Ok(self.bbox)
    }

    /// The pseudo box rotated about its third basis vector to (roughly) minimise its
    /// base area (`GetTightPBox`); `pbox_z` as for [`PointSet3d::bbox`].
    ///
    /// # Errors
    ///
    /// Only from the best-fit plane (not for finite points).
    pub fn tight_pbox(&mut self, pbox_z: Option<&mut Vec3>) -> Result<PseudoBox> {
        self.set_up_bboxes(DataRequest::RefinePBox, pbox_z)?;
        Ok(self.pbox)
    }

    /// `SetUpAxes`: caches normal, x and y axes (and, from the best-fit plane, the
    /// type, centroid and deviations).
    fn set_up_axes(&mut self) -> Result<()> {
        // no work - requested data already computed
        if self.cache.has(Cache::AXES) {
            return Ok(());
        }

        // branch on user given Normals
        if self.cache.has(Cache::NORMAL) && is_zero(1.0 - self.normal.length_squared()) {
            // mark state
            self.cache.set(Cache::AXES);

            let n = self.normal;
            // when the points have been classified linear - set XAxis = tangent
            // projected to Normal plane
            if self.cache.has(Cache::POINT_SET_TYPE) && self.point_set_type == PointSetType::Linear
            {
                // find the tangent line direction
                let mut tangent = Vec3::ZERO;
                let dist_tol2 = self.dist_tol3d * self.dist_tol3d;
                let mut iter = self.points.iter();
                if let Some(&o) = iter.next() {
                    for &p in iter {
                        // seek first point to specify a line
                        if o.distance_squared(p) > dist_tol2 {
                            tangent = p - o;
                            break;
                        }
                    }
                }
                // Z = normal, X = tangent projected to the normal plane, Y = Z x X
                [self.normal, self.x_axis, self.y_axis] =
                    unit_ortho(n, Some(tangent), [self.normal, self.x_axis, self.y_axis]);
            } else {
                // use given normal as Z and compute any X and Y axes.
                [self.normal, self.x_axis, self.y_axis] =
                    unit_ortho(n, None, [self.normal, self.x_axis, self.y_axis]);
            }
            return Ok(());
        }

        // compute Point Normal from best fit plane to points
        let fit = best_fit_plane_to_points(&self.points, self.dist_tol3d)?;
        self.point_set_type = fit.set_type;
        self.centroid = fit.point;
        self.normal = fit.normal;
        self.max_dev = fit.max_plane_gap;
        self.max_centroid_dev = fit.max_centroid_gap;

        // set axes
        let n = self.normal;
        match self.point_set_type {
            PointSetType::Void | PointSetType::PointSized => {
                self.x_axis = Vec3::X;
                self.y_axis = Vec3::Y;
                self.normal = Vec3::Z;
            }
            // currently normal = BestLine Tangent
            PointSetType::Linear => {
                [self.x_axis, self.y_axis, self.normal] =
                    unit_ortho(n, None, [self.x_axis, self.y_axis, self.normal]);
            }
            // currently normal = BestPlane Normal
            PointSetType::Planar | PointSetType::Scattered => {
                [self.normal, self.x_axis, self.y_axis] =
                    unit_ortho(n, None, [self.normal, self.x_axis, self.y_axis]);
            }
            PointSetType::Unknown => {}
        }

        // mark all saved values
        for bit in [
            Cache::AXES,
            Cache::NORMAL,
            Cache::POINT_SET_TYPE,
            Cache::CENTROID,
            Cache::MAX_DEV,
            Cache::MAX_CENTROID_DEV,
        ] {
            self.cache.set(bit);
        }
        Ok(())
    }

    /// `SetUpCentroid`: the average of the points, summed in blocks of 50.
    fn set_up_centroid(&mut self) {
        const BLOCK_SIZE: usize = 50;

        // no work - requested data already computed
        if self.cache.has(Cache::CENTROID) {
            return;
        }

        let size = self.points.len();
        let extras = size % BLOCK_SIZE;
        let iters = size / BLOCK_SIZE + usize::from(extras != 0);
        let size_f = size as f64;

        // init centroid
        self.centroid = Vec3::ZERO;

        // First the partial block, if any, then full blocks of 50.
        let mut idx = 0;
        let mut count = if extras == 0 { BLOCK_SIZE } else { extras };
        for _ in 0..iters {
            let mut block_sum = Vec3::ZERO;
            for p in self.points.iter().skip(idx).take(count) {
                block_sum += *p;
            }
            idx += count;
            // accumulate weighted block sums [B671]
            self.centroid += block_sum / size_f;
            count = BLOCK_SIZE;
        }

        self.cache.set(Cache::CENTROID);
    }

    /// `SetUpBBoxes`: builds the boxes when needed and, for
    /// [`DataRequest::RefinePBox`], tightens the pseudo box.
    fn set_up_bboxes(&mut self, reason: DataRequest, mut pbox_z: Option<&mut Vec3>) -> Result<()> {
        // prep
        if let Some(z) = pbox_z.as_deref_mut() {
            *z = z.unitize_or_z();
        }
        let want_bboxes = reason == DataRequest::BBoxes;
        let want_refine = reason == DataRequest::RefinePBox;
        let [_, _, basis2] = *self.pbox.basis();
        let z_matches = pbox_z
            .as_deref()
            .is_none_or(|z| z.closer_than(EFF_ZERO, basis2));

        // no work - requested data already computed
        if (!want_bboxes || self.cache.has(Cache::BBOX))
            && (!want_refine || self.cache.has(Cache::PBOX_REFINED))
            && z_matches
        {
            return Ok(());
        }

        // when BBoxes need to be built (either request asks for them)
        if !self.cache.has(Cache::BBOX) || !z_matches {
            // mark state
            self.cache.set(Cache::BBOX);

            // Pseudo box uses the stored axes
            self.set_up_axes()?;
            match pbox_z.as_deref_mut() {
                None => {
                    // upstream ignores the status
                    let _ = self.pbox.set_basis(self.x_axis, self.y_axis, self.normal);
                }
                Some(z) => {
                    let [sz, sx, sy] = unit_ortho(*z, None, [*z, Vec3::UNDEF, Vec3::UNDEF]);
                    *z = sz;
                    let _ = self.pbox.set_basis(sx, sy, sz);
                }
            }

            // init box extents
            self.bbox.init();
            self.pbox.init();

            // for every point
            for &p in &self.points {
                self.bbox.add_point(p);
                self.pbox.add_point(p);
            }
        }

        // when asked and if needed - refine PBox
        if !self.cache.has(Cache::PBOX_REFINED) && want_refine {
            // Upstream aligns the box with a hull edge only for Planar sets; a Linear
            // set's hull is its segment, which the alignment finds as well (WR-10).
            let align = self.cache.has(Cache::POINT_SET_TYPE)
                && matches!(
                    self.point_set_type,
                    PointSetType::Planar | PointSetType::Linear
                );
            self.set_up_tight_pseudo_box(align, pbox_z)?;
        }
        Ok(())
    }

    /// `SetUpTightPseudoBox`: tries five rotations of the pseudo box about its normal
    /// and keeps the smallest base area; with `extra_planar_work`, also tries the
    /// rotation that puts a hull edge on the box boundary.
    fn set_up_tight_pseudo_box(
        &mut self,
        extra_planar_work: bool,
        pbox_z: Option<&mut Vec3>,
    ) -> Result<()> {
        const ROT_CNT: usize = 5;

        // no work - already computed
        if self.cache.has(Cache::PBOX_REFINED) {
            return Ok(());
        }

        // set state
        self.cache.set(Cache::PBOX_REFINED);

        // ensure the pseudo box is built
        self.set_up_bboxes(DataRequest::BBoxes, pbox_z)?;

        let plane_point = self.pbox.evaluate(0.5, 0.5, 0.5);
        let [mut plane_x, mut plane_y, mut plane_z] = *self.pbox.basis();
        let plane_normal = plane_x.cross(plane_y).unitize_or_z();
        let mut area = self.pbox.xy_area();
        let mut this_box = PseudoBox::new();

        // for a few rotations
        for ii in 0..ROT_CNT {
            let ang_rad = (ii + 1) as f64 * (PI / 2.0 / (ROT_CNT + 1) as f64);

            // build pseudo box with rotated basis0 and basis1 vectors
            let rot_x = plane_x.rotate_vec_about_axis(plane_normal, ang_rad);
            let rot_y = plane_y.rotate_vec_about_axis(plane_normal, ang_rad);
            let _ = this_box.set_basis(rot_x, rot_y, plane_z);
            this_box.init();
            for &p in &self.points {
                this_box.add_point(p);
            }

            // save the smallest area box
            let this_area = this_box.xy_area();
            if this_area < area {
                area = this_area;
                self.pbox = this_box;
            }
        }

        if !extra_planar_work {
            return Ok(());
        }

        // extra work: try to align the box with a convex polygon line segment
        let (mut found_beg0, mut found_end0, mut found_beg1, mut found_end1) = (0, 0, 0, 0);
        let (mut beg_index0, mut end_index0, mut beg_index1, mut end_index1) = (0, 0, 0, 0);

        // find points on the min and max Basis[0] / Basis[1] boundaries
        for (ii, &p) in self.points.iter().enumerate() {
            let [bnd0, bnd1, _] = self.pbox.classify_point(p, EFF_ZERO);
            if bnd0 == ExtentPointType::Start {
                beg_index0 = ii;
                found_beg0 += 1;
            }
            if bnd0 == ExtentPointType::End {
                end_index0 = ii;
                found_end0 += 1;
            }
            if bnd1 == ExtentPointType::Start {
                beg_index1 = ii;
                found_beg1 += 1;
            }
            if bnd1 == ExtentPointType::End {
                end_index1 = ii;
                found_end1 += 1;
            }
        }

        // no more work - some convex polygon line is already on a boundary
        if found_beg0 > 1 || found_end0 > 1 || found_beg1 > 1 || found_end1 > 1 {
            return Ok(());
        }

        // update the pseudobox coordinate vectors
        [plane_x, plane_y, plane_z] = *self.pbox.basis();

        // Upstream (`SmPointSet.cpp:777-780, 790`) calls the `const`
        // `ProjectPointToPlane(sPlanePoint, sPlaneZ)` on the four boundary points and
        // each candidate point and discards its return value (`WR-14`); the port uses
        // the projections, so the chords lie in the box plane.
        let project = |p: Point3| p.project_point_to_plane(plane_point, plane_z);
        let point_at = |i: usize| project(self.points.get(i).copied().unwrap_or(Vec3::ZERO));
        let beg_pt0 = point_at(beg_index0);
        let end_pt0 = point_at(end_index0);
        let beg_pt1 = point_at(beg_index1);
        let end_pt1 = point_at(end_index1);
        let (mut beg_ang0, mut beg_min0, mut beg_max0) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);
        let (mut end_ang0, mut end_min0, mut end_max0) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);
        let (mut beg_ang1, mut beg_min1, mut beg_max1) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);
        let (mut end_ang1, mut end_min1, mut end_max1) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);

        // find the neighbour points with the smallest angles from the boundary planes
        for (ii, &raw_pt) in self.points.iter().enumerate() {
            // skip the bound points
            if beg_index0 == ii || end_index0 == ii || beg_index1 == ii || end_index1 == ii {
                continue;
            }

            let this_pt = project(raw_pt);
            let beg_vec0 = this_pt - beg_pt0;
            let end_vec0 = this_pt - end_pt0;
            let beg_vec1 = this_pt - beg_pt1;
            let end_vec1 = this_pt - end_pt1;

            // get the angles (a failure keeps the previous value, as upstream)
            beg_ang0 = plane_normal
                .ccw_angle_between(plane_y, beg_vec0)
                .unwrap_or(beg_ang0);
            end_ang0 = plane_normal
                .ccw_angle_between(plane_y, end_vec0)
                .unwrap_or(end_ang0);
            beg_ang1 = plane_normal
                .ccw_angle_between(plane_x, beg_vec1)
                .unwrap_or(beg_ang1);
            end_ang1 = plane_normal
                .ccw_angle_between(plane_x, end_vec1)
                .unwrap_or(end_ang1);

            // save the min and max angles
            if beg_ang0 < beg_min0 {
                beg_min0 = beg_ang0;
            }
            if beg_ang0 > beg_max0 {
                beg_max0 = beg_ang0;
            }
            if end_ang0 < end_min0 {
                end_min0 = end_ang0;
            }
            if end_ang0 > end_max0 {
                end_max0 = end_ang0;
            }

            if beg_ang1 < beg_min1 {
                beg_min1 = beg_ang1;
            }
            if beg_ang1 > beg_max1 {
                beg_max1 = beg_ang1;
            }
            if end_ang1 < end_min1 {
                end_min1 = end_ang1;
            }
            if end_ang1 > end_max1 {
                end_max1 = end_ang1;
            }
        }

        // rotate the basis the max amount that keeps every point inside
        let pos_ang0 = min2(end_min0, beg_min0 + PI);
        let neg_ang0 = max2(end_max0 - PI, beg_max0);
        let pos_ang1 = min2(end_min1 + PI, beg_min1);
        let neg_ang1 = max2(end_max1, beg_max1 - PI);

        let ang_rad = if min2(pos_ang0, pos_ang1) > -max2(neg_ang0, neg_ang1) {
            min2(pos_ang0, pos_ang1)
        } else {
            max2(neg_ang0, neg_ang1)
        };

        // build pseudo box with rotated basis0 and basis1 vectors
        let rot_x = plane_x.rotate_vec_about_axis(plane_normal, ang_rad);
        let rot_y = plane_y.rotate_vec_about_axis(plane_normal, ang_rad);
        let _ = this_box.set_basis(rot_x, rot_y, plane_z);
        this_box.init();
        for &p in &self.points {
            this_box.add_point(p);
        }

        // save the smallest area box
        let this_area = this_box.xy_area();
        if this_area < area {
            self.pbox = this_box;
        }
        Ok(())
    }
}

/// An ordered point set with implied segments between consecutive points, closed or
/// not (`SmPointSequence`). Dereferences to its [`PointSet3d`].
#[derive(Debug, Clone, PartialEq)]
pub struct PointSequence {
    set: PointSet3d,
    closed: bool,
    area_set: bool,
    area: f64,
}

impl Deref for PointSequence {
    type Target = PointSet3d;
    fn deref(&self) -> &PointSet3d {
        &self.set
    }
}

impl DerefMut for PointSequence {
    fn deref_mut(&mut self) -> &mut PointSet3d {
        &mut self.set
    }
}

impl PointSequence {
    /// A point sequence (`SmPointSequence(sPoints, dTol3d, bClosed, bPlanar,
    /// pOptNormal)`); `normal` as for [`PointSet3d::new`].
    #[must_use]
    pub fn new(points: Vec<Point3>, tol3d: f64, closed: bool, normal: Option<&mut Vec3>) -> Self {
        Self {
            set: PointSet3d::new(points, tol3d, normal),
            closed,
            area_set: false,
            area: 0.0,
        }
    }

    /// True when the last point connects back to the first (`IsClosed`).
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Signed area enclosed by the sequence (treated as closed) projected along the
    /// set's normal, positive for counter-clockwise (`GetArea`); `-1` for an empty
    /// sequence.
    ///
    /// # Errors
    ///
    /// Only from the best-fit plane (not for finite points).
    pub fn area(&mut self) -> Result<f64> {
        self.set_up_area()?;
        Ok(self.area)
    }

    /// `SetUpArea`.
    fn set_up_area(&mut self) -> Result<()> {
        // no work - already computed
        if self.area_set {
            return Ok(());
        }
        self.area_set = true;

        // compute area on plane defined by PointSet Normal
        let normal = self.set.normal()?;
        self.area = polygon_area(&self.set.points, normal).unwrap_or(-1.0);
        Ok(())
    }

    /// Quadrant of `test - origin` in the set's x/y axes (`CalcQuadNum`):
    /// 0 = +x+y, 1 = -x+y, 2 = -x-y, 3 = +x-y.
    fn calc_quad_num(&self, origin: Point3, test: Point3) -> i32 {
        let v = test - origin;
        if v.dot(self.set.x_axis) > 0.0 {
            if v.dot(self.set.y_axis) <= 0.0 {
                3
            } else {
                0
            }
        } else if v.dot(self.set.y_axis) > 0.0 {
            1
        } else {
            2
        }
    }

    /// Classifies `point` against the closed sequence by its winding number
    /// (`PointContainment`); an open sequence is always outside.
    ///
    /// # Errors
    ///
    /// Only from the best-fit plane (not for finite points).
    pub fn point_containment(&mut self, point: Point3) -> Result<PointContainment> {
        // First, if not closed, always outside.
        if !self.closed {
            return Ok(PointContainment::Outside);
        }

        // side effect - SetUpAxes() and set the bounding box
        self.set.bbox(None)?;

        // low work - not in BBox
        let tol = self.set.dist_tol3d;
        if !self.set.bbox.contains_point(point, tol) {
            return Ok(PointContainment::Outside);
        }

        // Winding number of the point set points around the given point.
        let Some(&last) = self.set.points.last() else {
            return Ok(PointContainment::Outside);
        };
        let mut this_pt = last;
        let mut prev_quad = self.calc_quad_num(point, this_pt);
        let mut accum = 0;

        for &p in &self.set.points {
            let prev_pt = this_pt;
            this_pt = p;

            // First check 'on': coincident point.
            if this_pt.closer_than(tol, point) {
                return Ok(PointContainment::OnBoundary);
            }

            let this_quad = self.calc_quad_num(point, this_pt);
            let delta = this_quad - prev_quad;

            if delta != 0 {
                // First check 'on': on line segment.
                if is_point_on_segment(prev_pt, this_pt, point, tol)?.on {
                    return Ok(PointContainment::OnBoundary);
                }

                if delta == 1 || delta == -3 {
                    accum += 1;
                } else if delta == -1 || delta == 3 {
                    accum -= 1;
                } else {
                    // +- 2: diagonal. CW or CCW around the test point?
                    let cross = (prev_pt - point).cross(this_pt - point);
                    if cross.dot(self.set.normal) > 0.0 {
                        accum += 2;
                    } else {
                        accum -= 2;
                    }
                }
            }
            prev_quad = this_quad;
        }

        // The result depends on accum.
        Ok(match accum {
            -1..=1 => PointContainment::Outside,
            -5..=-3 | 3..=5 => PointContainment::Inside,
            _ => PointContainment::Unknown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Point3> {
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ]
    }

    #[test]
    fn square_area_and_containment() {
        let mut n = Vec3::new(0.0, 0.0, 3.0);
        let mut seq = PointSequence::new(square(), 1e-9, true, Some(&mut n));
        assert_eq!(n, Vec3::Z);
        assert!((seq.area().unwrap() - 4.0).abs() < 1e-12);
        let c =
            |seq: &mut PointSequence, x, y| seq.point_containment(Vec3::new(x, y, 0.0)).unwrap();
        assert_eq!(c(&mut seq, 1.0, 1.0), PointContainment::Inside);
        assert_eq!(c(&mut seq, 3.0, 1.0), PointContainment::Outside);
        assert_eq!(c(&mut seq, 1.0, 0.0), PointContainment::OnBoundary);
        assert_eq!(c(&mut seq, 2.0, 2.0), PointContainment::OnBoundary);
    }

    /// Fuzz regression: the tolerance is an `SmTol3d` upstream, so a negative one
    /// behaves as its magnitude.
    #[test]
    fn negative_tolerance_is_its_magnitude() {
        let p = Vec3::new(1.0, 0.05, 0.0);
        let mut neg = PointSequence::new(square(), -0.1, true, None);
        let mut pos = PointSequence::new(square(), 0.1, true, None);
        assert_eq!(
            neg.point_containment(p).unwrap(),
            PointContainment::OnBoundary
        );
        assert_eq!(
            neg.point_containment(p).unwrap(),
            pos.point_containment(p).unwrap()
        );
    }

    #[test]
    fn open_sequence_is_outside() {
        let mut seq = PointSequence::new(square(), 1e-9, false, None);
        assert_eq!(
            seq.point_containment(Vec3::new(1.0, 1.0, 0.0)).unwrap(),
            PointContainment::Outside
        );
    }

    #[test]
    fn best_fit_normal_and_boxes() {
        let mut set = PointSet3d::new(square(), 1e-9, None);
        let n = set.normal().unwrap();
        assert!((n.z.abs() - 1.0).abs() < 1e-12);
        let bb = set.bbox(None).unwrap();
        assert_eq!(bb.max_at(0), 2.0);
        let pb = set.tight_pbox(None).unwrap();
        assert!(pb.xy_area() <= 4.0 + 1e-9);
    }

    /// A constant 100-point set has that same point as its mean (WR-15).
    #[test]
    fn wr15_default_centroid_includes_every_full_block() {
        let point = Vec3::new(2.0, 4.0, 6.0);
        let mut set = PointSet3d::new(vec![point; 100], 1e-9, None);
        assert_eq!(set.centroid(), point);
    }

    #[test]
    fn empty_set() {
        let mut seq = PointSequence::new(Vec::new(), 1e-9, true, None);
        assert_eq!(seq.centroid(), Vec3::ZERO);
        assert_eq!(seq.normal().unwrap(), Vec3::Z);
        assert_eq!(seq.area().unwrap(), -1.0);
        assert_eq!(
            seq.point_containment(Vec3::ZERO).unwrap(),
            PointContainment::Outside
        );
    }

    #[test]
    fn given_pbox_z_is_unitized_in_place() {
        let mut set = PointSet3d::new(square(), 1e-9, None);
        let mut z = Vec3::new(0.0, 0.0, 5.0);
        let _ = set.tight_pbox(Some(&mut z)).unwrap();
        assert_eq!(z, Vec3::Z);
    }

    /// Constructs symmetric planar pairs about a known centre.
    fn planar_paired_points(half_count: u32, center: Point3) -> Vec<Point3> {
        let mut pts = Vec::with_capacity((half_count * 2) as usize);
        for i in 0..half_count {
            let t = f64::from(i) + 1.0;
            let delta = if i % 2 == 0 {
                Vec3::new(t, 0.0, 0.0)
            } else {
                Vec3::new(0.0, t, 0.0)
            };
            pts.push(center + delta);
        }
        for i in 0..half_count {
            let t = f64::from(i) + 1.0;
            let delta = if i % 2 == 0 {
                Vec3::new(t, 0.0, 0.0)
            } else {
                Vec3::new(0.0, t, 0.0)
            };
            pts.push(center - delta);
        }
        pts
    }

    /// Empty and partial blocks preserve their closed-form means and cache state.
    #[test]
    fn wr15_empty_and_non_multiple_block_sizes() {
        let mut empty = PointSet3d::new(Vec::new(), 1e-9, None);
        assert_eq!(empty.centroid(), Vec3::ZERO);
        assert!(empty.cache.has(Cache::CENTROID));
        for (count, expected) in [
            (49_u32, Vec3::new(24.0, 43.0, 3.5)),
            (51, Vec3::new(25.0, 45.0, 3.5)),
        ] {
            let pts = (0..count)
                .map(|i| Vec3::new(f64::from(i), 2.0 * f64::from(i) - 5.0, 3.5))
                .collect();
            let mut set = PointSet3d::new(pts, 1e-9, None);
            let first = set.centroid();
            assert!((first - expected).length() < 1e-12);
            assert!(set.cache.has(Cache::CENTROID));
            assert_eq!(set.centroid(), first);
            set.clear_cache();
            assert!(!set.cache.has(Cache::CENTROID));
            assert_eq!(set.centroid(), first);
        }
    }

    /// Full blocks and inherited sequence queries use analytic arithmetic means.
    #[test]
    fn wr15_multiples_of_50_points_50_100_150() {
        for count in [50_u32, 100, 150] {
            let pts: Vec<_> = (1..=count)
                .map(|i| Vec3::new(f64::from(i), -2.0 * f64::from(i), 4.0))
                .collect();
            let mean = (f64::from(count) + 1.0) / 2.0;
            let expected = Vec3::new(mean, -2.0 * mean, 4.0);
            let mut set = PointSet3d::new(pts.clone(), 1e-9, None);
            assert!((set.centroid() - expected).length() < 1e-12);
            assert!(set.cache.has(Cache::CENTROID));
            let mut seq = PointSequence::new(pts, 1e-9, true, None);
            assert!((seq.centroid() - expected).length() < 1e-12);
            assert!(seq.cache.has(Cache::CENTROID));
        }
    }

    /// Normal and box queries preserve the known centre on symmetric planar inputs.
    #[test]
    fn wr15_call_order_before_and_after_normal_and_bbox() {
        let center = Vec3::new(7.0, -2.0, 1.5);
        for half_count in [25, 50] {
            let pts = planar_paired_points(half_count, center);
            for given in [false, true] {
                for bbox_first in [false, true] {
                    let mut normal = Vec3::Z;
                    let mut set = PointSet3d::new(pts.clone(), 1e-9, given.then_some(&mut normal));
                    if !bbox_first {
                        assert!((set.centroid() - center).length() < 1e-12);
                    }
                    let n = set.normal().unwrap();
                    assert!((n.z.abs() - 1.0).abs() < 1e-12);
                    set.bbox(None).unwrap();
                    assert!((set.centroid() - center).length() < 1e-12);
                    set.clear_cache();
                    assert!((set.centroid() - center).length() < 1e-12);
                }
                let mut normal = Vec3::Z;
                let mut set = PointSet3d::new(pts.clone(), 1e-9, given.then_some(&mut normal));
                set.bbox(None).unwrap();
                assert!((set.centroid() - center).length() < 1e-12);
            }
        }
    }

    /// Non-finite coordinates continue through the existing IEEE arithmetic path.
    #[test]
    fn wr15_nonfinite_coordinates_follow_existing_arithmetic() {
        let mut set = PointSet3d::new(vec![Vec3::new(f64::INFINITY, 2.0, 4.0)], 1e-9, None);
        let c = set.centroid();
        assert_eq!(c.x, f64::INFINITY);
        assert_eq!(c.y, 2.0);
        assert_eq!(c.z, 4.0);
    }

    /// WR-10: axis-aligned collinear points are Linear (pinned v1.0.1: Planar), with
    /// `x_axis` along the line and a degenerate tight box.
    #[test]
    fn wr10_point_set_collinear_axes() {
        let pts_x = vec![
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
        ];

        let mut set = PointSet3d::new(pts_x.clone(), 1e-6, None);
        let n = set.normal().unwrap();
        assert_eq!(set.point_set_type, PointSetType::Linear);
        assert!(set.x_axis.dot(Vec3::X).abs() > 1.0 - 1e-12);
        assert!(set.y_axis.dot(Vec3::X).abs() < 1e-12);
        assert!(n.dot(Vec3::X).abs() < 1e-12);
        assert!((set.x_axis.cross(set.y_axis) - n).length() < 1e-12);
        assert!(set.max_dev <= 1e-15);

        let bb = set.bbox(None).unwrap();
        assert_eq!(bb.min_at(0), -3.0);
        assert_eq!(bb.max_at(0), 3.0);
        assert_eq!(bb.min_at(1), 0.0);
        assert_eq!(bb.max_at(1), 0.0);
        assert_eq!(bb.min_at(2), 0.0);
        assert_eq!(bb.max_at(2), 0.0);

        let pb = set.tight_pbox(None).unwrap();
        assert!(pb.xy_area() <= 1e-24);

        // A user-given normal is kept and the set stays Planar.
        let mut user_n = Vec3::new(0.0, 0.0, 4.0);
        let mut set_given = PointSet3d::new(pts_x.clone(), 1e-6, Some(&mut user_n));
        assert_eq!(set_given.normal().unwrap(), Vec3::Z);
        assert_eq!(set_given.point_set_type, PointSetType::Planar);

        let mut seq = PointSequence::new(pts_x, 1e-6, true, None);
        assert!(seq.area().unwrap().abs() <= 1e-15);
        assert_eq!(seq.point_set_type, PointSetType::Linear);
        assert_eq!(
            seq.point_containment(Vec3::new(1.0, 0.0, 0.0)).unwrap(),
            PointContainment::OnBoundary
        );
        assert_eq!(
            seq.point_containment(Vec3::new(1.0, 1.0, 0.0)).unwrap(),
            PointContainment::Outside
        );
    }

    /// WR-10 tight boxes (golden `geom.point_set` `gen0034`): six collinear points
    /// along an oblique line, classified Linear, boxed about an explicit `+z`. Only
    /// the 15-degree rotation search ran for Linear sets, leaving a 12.7 area box
    /// (pinned v1.0.1 misclassified the set as Planar and its hull-edge alignment
    /// found the line, area ~1e-13). The alignment now runs for Linear sets too: the
    /// box lies along the line's projection and still holds every point.
    #[test]
    fn wr10_linear_set_tight_box_aligns_with_the_line() {
        let pts = vec![
            Vec3::new(21.80485446950501, 2.127459047116308, -16.137096725968778),
            Vec3::new(7.345372386578687, -0.9303172661055201, -20.022576112649997),
            Vec3::new(12.803349297106683, 0.22389229820735135, -18.5559359759414),
            Vec3::new(6.7065268606288875, -1.065415238899695, -20.19424347307935),
            Vec3::new(7.529993876510043, -0.8912749812168781, -19.972965553340014),
            Vec3::new(14.860701819016317, 0.6589648529063028, -18.003094541568345),
        ];
        let mut seq = PointSequence::new(pts.clone(), 1e-6, true, None);
        seq.normal().unwrap();
        assert_eq!(seq.point_set_type, PointSetType::Linear);
        let mut z = Vec3::Z;
        let pb = seq.tight_pbox(Some(&mut z)).unwrap();
        // The line's projection onto z = 0 is 15.4 long; its box has no width.
        let along = Vec3::new(pts[0].x - pts[3].x, pts[0].y - pts[3].y, 0.0);
        assert!(pb.basis()[0].cross(along).length() < 1e-9 * along.length());
        assert!(pb.xy_area() < 1e-9, "area {}", pb.xy_area());
        for &p in &pts {
            assert!(pb.contains_point(p, 1e-9));
        }
    }

    /// Builds a 6-point convex footprint in the `(nx, ny)` frame rotated by `theta = 0.08`
    /// rad with a `chamfer_ang` upper-right chamfer, lifted along `nz` by `ws`.
    fn wr14_tilted_hull_points(
        nx: Vec3,
        ny: Vec3,
        nz: Vec3,
        chamfer_ang: f64,
        ws: [f64; 6],
    ) -> Vec<Point3> {
        let theta = 0.08_f64;
        let (c, s) = (theta.cos(), theta.sin());
        let rot2 = |u: f64, v: f64| (u * c - v * s, u * s + v * c);
        let d = rot2(-10.0, 1.0); // beg0 (min x)
        let b = rot2(10.0, -1.0); // end0 (max x)
        let a = rot2(-10.0, -1.0); // beg1 (min y)
        let top = (5.0, 2.0); // end1 (max y)
        let p4 = rot2(0.0, -1.0); // on long bottom edge A -> B (+0.08 rad from beg1)
        let p5 = (5.0 + chamfer_ang.cos(), 2.0 - chamfer_ang.sin()); // chamfer from end1
        let coords_2d = [d, b, a, top, p4, p5];
        coords_2d
            .into_iter()
            .zip(ws)
            .map(|((u, v), w)| nx * u + ny * v + nz * w)
            .collect()
    }

    /// WR-14: the tight box measures hull-chord angles between points projected onto
    /// the box plane (upstream discards the projections). Planar sets at any elevation
    /// select the `+0.08` rad hull edge. For points up to 4e5 off the plane, the
    /// unprojected chords tie the `+0.08` rad edge with the `-0.08` rad chamfer and
    /// roundoff picks the chamfer, whose rotated box is rejected (area > 75); the
    /// projected chords select the long edge, a box about 30% smaller.
    #[test]
    fn wr14_tight_box_on_projected_points() {
        // Planar set in `z = 0` with `normal = None` (best-fit plane).
        let pts_xy0 = wr14_tilted_hull_points(Vec3::X, Vec3::Y, Vec3::Z, 0.06, [0.0; 6]);
        let mut set_xy0 = PointSet3d::new(pts_xy0.clone(), 1e-9, None);
        let pb_xy0 = set_xy0.tight_pbox(None).unwrap();
        for &p in &pts_xy0 {
            assert!(pb_xy0.contains_point(p, 1e-12));
        }

        // The same footprint at `z = 3.5` with an explicit `+Z` normal gives the same
        // rotation.
        let pts_elev = wr14_tilted_hull_points(Vec3::X, Vec3::Y, Vec3::Z, 0.06, [3.5; 6]);
        let mut z = Vec3::new(0.0, 0.0, 2.0);
        let mut set_elev = PointSet3d::new(pts_elev.clone(), 1e-9, Some(&mut z));
        let pb_elev = set_elev.tight_pbox(None).unwrap();
        assert!((pb_elev.xy_area() - pb_xy0.xy_area()).abs() < 1e-11);
        for &p in &pts_elev {
            assert!(pb_elev.contains_point(p, 1e-12));
        }

        // Oblique planar set (`w = 7.5` along `nz` for `n0 = (1, 2, 3)`): the box turns
        // onto the `+0.08` rad hull edge.
        let n0 = Vec3::new(1.0, 2.0, 3.0);
        let [nz, nx, ny] = n0.make_unit_ortho_vectors(None).unwrap();
        let pts_obl = wr14_tilted_hull_points(nx, ny, nz, 0.06, [7.5; 6]);
        let mut n_obl = n0;
        let mut set_obl = PointSet3d::new(pts_obl.clone(), 1e-9, Some(&mut n_obl));
        let pb_obl = set_obl.tight_pbox(None).unwrap();
        let rot = nx.angle_between(pb_obl.basis()[0]).unwrap();
        assert!((rot - 0.08).abs() < 1e-10);
        assert!((pb_obl.xy_area() - pb_xy0.xy_area()).abs() < 1e-11);
        for &p in &pts_obl {
            assert!(pb_obl.contains_point(p, 1e-9));
        }

        // Points far off the box plane: the long hull edge is selected.
        let ws = [2.0e5, -2.0e5, -3.0e5, 4.0e5, 3.0e5, -4.0e5];
        let pts_far = wr14_tilted_hull_points(nx, ny, nz, 0.08, ws);
        let mut n_far = n0;
        let mut set_far = PointSet3d::new(pts_far.clone(), 1e-9, Some(&mut n_far));
        let pb_far = set_far.tight_pbox(None).unwrap();
        let rot_far = nx.angle_between(pb_far.basis()[0]).unwrap();
        assert!((rot_far - 0.08).abs() < 1e-10, "rotation {rot_far}");
        assert!(pb_far.xy_area() < 53.0, "area {}", pb_far.xy_area());
        assert!(pb_far.volume() < 4.2e7, "volume {}", pb_far.volume());
        let mut z_far = n0;
        let pb_far_z = set_far.tight_pbox(Some(&mut z_far)).unwrap();
        assert!(pb_far_z.xy_area() < 53.0, "area {}", pb_far_z.xy_area());
        for &p in &pts_far {
            assert!(pb_far.contains_point(p, 1e-7));
            assert!(pb_far_z.contains_point(p, 1e-7));
        }
    }
}
