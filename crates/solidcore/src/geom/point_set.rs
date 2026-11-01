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
//! | [`PointSet3d::centroid_opt_in`]        | `SmPointSet3d::GetCentroid` (WR-15 opt-in)       |
//! | [`PointSet3d::centroid_with_kernel`]   | `SmPointSet3d::GetCentroid` (WR-15 kernel flag)  |
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
//! - `SetUpCentroid` never marks the centroid cached and, when the point count is a
//!   positive multiple of 50, sums one empty block and drops the last 50 points
//!   (WR-15, `plan/findings/`): [`PointSet3d::centroid`] keeps both by default;
//!   [`PointSet3d::centroid_opt_in`] / [`PointSet3d::centroid_with_kernel`]
//!   ([`Kernel::fixes.point_set_centroid_all_blocks`](crate::kernel::UpstreamFixes::point_set_centroid_all_blocks))
//!   sums every full 50-point block and marks `Cache::CENTROID`.
//! - `SetUpTightPseudoBox` calls the const `ProjectPointToPlane` on its boundary and
//!   neighbour points and discards the results, so the angles are measured between
//!   the unprojected points; kept (see `plan/findings.md`).
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
use crate::kernel::Kernel;
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

    /// The geometric centre of the points (`GetCentroid`): the best-fit centroid when
    /// the axes were computed from the points, else the block-wise average (preserving
    /// upstream's WR-15 behaviour by default).
    pub fn centroid(&mut self) -> Point3 {
        self.centroid_opt_in(false)
    }

    /// The geometric centre of the points (`GetCentroid`), optionally enabling the
    /// WR-15 fix (`all_blocks = true`): when the point count is a positive multiple of
    /// 50, sums every full 50-point block instead of starting with a 0-point block and
    /// dropping the last 50 points, and marks `Cache::CENTROID` (`m_bCentroidSet = TRUE`)
    /// after computing it.
    pub fn centroid_opt_in(&mut self, all_blocks: bool) -> Point3 {
        self.set_up_centroid(all_blocks);
        self.centroid
    }

    /// The geometric centre of the points (`GetCentroid`), honouring
    /// [`Kernel::fixes.point_set_centroid_all_blocks`](crate::kernel::UpstreamFixes::point_set_centroid_all_blocks)
    /// (WR-15).
    pub fn centroid_with_kernel(&mut self, k: &Kernel) -> Point3 {
        self.centroid_opt_in(k.fixes.point_set_centroid_all_blocks)
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
    fn set_up_centroid(&mut self, all_blocks: bool) {
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

        // for every block - first the extras, then blocks of BLOCK_SIZE.
        // Upstream starts `jCnt` at `lExtras` unconditionally, so when `extras == 0`
        // and `size > 0` iteration 0 sums 0 points and the last 50 points are dropped
        // (WR-15); with `all_blocks`, every iteration sums a full block of 50.
        let mut idx = 0;
        let mut count = if all_blocks && extras == 0 {
            BLOCK_SIZE
        } else {
            extras
        };
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

        if all_blocks {
            self.cache.set(Cache::CENTROID);
        }
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
        let basis2 = self.pbox.basis()[2];
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
            let planar = self.cache.has(Cache::POINT_SET_TYPE)
                && self.point_set_type == PointSetType::Planar;
            self.set_up_tight_pseudo_box(planar, pbox_z)?;
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

        // Upstream means to project the points along basis[2] onto the plane through the
        // box centre before testing angles, but discards the results of the const
        // `ProjectPointToPlane` calls: the raw points are used (see plan/findings.md),
        // and the box centre (`sPlanePoint`) is not needed.
        let point_at = |i: usize| self.points.get(i).copied().unwrap_or(Vec3::ZERO);
        let beg_pt0 = point_at(beg_index0);
        let end_pt0 = point_at(end_index0);
        let beg_pt1 = point_at(beg_index1);
        let end_pt1 = point_at(end_index1);
        let (mut beg_ang0, mut beg_min0, mut beg_max0) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);
        let (mut end_ang0, mut end_min0, mut end_max0) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);
        let (mut beg_ang1, mut beg_min1, mut beg_max1) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);
        let (mut end_ang1, mut end_min1, mut end_max1) = (0.0, BIG_DOUBLE, -BIG_DOUBLE);

        // find the neighbour points with the smallest angles from the boundary planes
        for (ii, &this_pt) in self.points.iter().enumerate() {
            // skip the bound points
            if beg_index0 == ii || end_index0 == ii || beg_index1 == ii || end_index1 == ii {
                continue;
            }

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

    #[test]
    fn centroid_drops_last_block_at_multiples_of_50() {
        let pts: Vec<Point3> = (0..50).map(|i| Vec3::new(f64::from(i), 1.0, 0.0)).collect();
        let mut set = PointSet3d::new(pts, 1e-9, None);
        // upstream quirk: one empty block, the 50 points are never summed
        assert_eq!(set.centroid(), Vec3::ZERO);
        let pts: Vec<Point3> = (0..51).map(|_| Vec3::new(1.0, 2.0, 3.0)).collect();
        let mut set = PointSet3d::new(pts, 1e-9, None);
        let c = set.centroid();
        assert!((c.x - 1.0).abs() < 1e-12 && (c.z - 3.0).abs() < 1e-12);
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

    #[test]
    fn wr15_empty_and_non_multiple_block_sizes() {
        let k_default = Kernel::default();
        let mut k_opt = Kernel::default();
        k_opt.fixes.point_set_centroid_all_blocks = true;

        // 0 points: both modes return (0, 0, 0); only opt-in sets Cache::CENTROID.
        let mut empty_def = PointSet3d::new(Vec::new(), 1e-9, None);
        assert_eq!(empty_def.centroid_with_kernel(&k_default), Vec3::ZERO);
        assert!(!empty_def.cache.has(Cache::CENTROID));

        let mut empty_opt = PointSet3d::new(Vec::new(), 1e-9, None);
        assert_eq!(empty_opt.centroid_with_kernel(&k_opt), Vec3::ZERO);
        assert!(empty_opt.cache.has(Cache::CENTROID));

        // 49 points: extras = 49, single block; arithmetic matches, opt-in caches.
        let pts49: Vec<Point3> = (0..49_u32)
            .map(|i| Vec3::new(f64::from(i), 2.0 * f64::from(i) - 5.0, 3.5))
            .collect();
        let mut set49_def = PointSet3d::new(pts49.clone(), 1e-9, None);
        let c49_def = set49_def.centroid_opt_in(false);
        assert!(!set49_def.cache.has(Cache::CENTROID));
        assert!((c49_def - Vec3::new(24.0, 43.0, 3.5)).length() < 1e-12);

        let mut set49_opt = PointSet3d::new(pts49, 1e-9, None);
        let c49_opt = set49_opt.centroid_opt_in(true);
        assert!(set49_opt.cache.has(Cache::CENTROID));
        assert_eq!(c49_opt, c49_def);
        set49_opt.clear_cache();
        assert!(!set49_opt.cache.has(Cache::CENTROID));

        // 51 points: extras = 1 + one block of 50; arithmetic matches, opt-in caches.
        let pts51: Vec<Point3> = (0..51_u32)
            .map(|i| Vec3::new(f64::from(i), 2.0 * f64::from(i) - 5.0, 3.5))
            .collect();
        let mut set51_def = PointSet3d::new(pts51.clone(), 1e-9, None);
        let c51_def = set51_def.centroid_opt_in(false);
        assert!(!set51_def.cache.has(Cache::CENTROID));
        assert!((c51_def - Vec3::new(25.0, 45.0, 3.5)).length() < 1e-12);

        let mut set51_opt = PointSet3d::new(pts51, 1e-9, None);
        let c51_opt = set51_opt.centroid_opt_in(true);
        assert!(set51_opt.cache.has(Cache::CENTROID));
        assert_eq!(c51_opt, c51_def);
    }

    #[test]
    fn wr15_multiples_of_50_points_50_and_100() {
        let k_default = Kernel::default();
        let mut k_opt = Kernel::default();
        k_opt.fixes.point_set_centroid_all_blocks = true;

        // 50 points: x in 1..=50 (mean 25.5), y in -2..=-100 (mean -51.0), z = 4.0.
        let pts50: Vec<Point3> = (1..=50_u32)
            .map(|i| Vec3::new(f64::from(i), -2.0 * f64::from(i), 4.0))
            .collect();
        let expected50 = Vec3::new(25.5, -51.0, 4.0);

        let mut set50 = PointSet3d::new(pts50.clone(), 1e-9, None);
        assert_eq!(set50.centroid(), Vec3::ZERO);
        assert_eq!(set50.centroid_opt_in(false), Vec3::ZERO);
        assert_eq!(set50.centroid_with_kernel(&k_default), Vec3::ZERO);
        assert!(!set50.cache.has(Cache::CENTROID));

        // Because default left Cache::CENTROID unset, calling opt-in next computes and caches it.
        let c50_opt = set50.centroid_with_kernel(&k_opt);
        assert!(set50.cache.has(Cache::CENTROID));
        assert!((c50_opt - expected50).length() < 1e-12);

        let mut seq50 = PointSequence::new(pts50, 1e-9, true, None);
        assert!((seq50.centroid_opt_in(true) - expected50).length() < 1e-12);
        assert!(seq50.cache.has(Cache::CENTROID));

        // 100 points: x in 1..=100 (mean 50.5, first-50 sum / 100 = 12.75), y = 10, z = -3.
        let pts100: Vec<Point3> = (1..=100_u32)
            .map(|i| Vec3::new(f64::from(i), 10.0, -3.0))
            .collect();
        let mut set100_def = PointSet3d::new(pts100.clone(), 1e-9, None);
        let c100_def = set100_def.centroid_with_kernel(&k_default);
        assert!(!set100_def.cache.has(Cache::CENTROID));
        assert!((c100_def - Vec3::new(12.75, 5.0, -1.5)).length() < 1e-12);

        let mut set100_opt = PointSet3d::new(pts100, 1e-9, None);
        let c100_opt = set100_opt.centroid_with_kernel(&k_opt);
        assert!(set100_opt.cache.has(Cache::CENTROID));
        assert!((c100_opt - Vec3::new(50.5, 10.0, -3.0)).length() < 1e-12);
    }

    #[test]
    fn wr15_call_order_independence_before_and_after_normal() {
        let mut k_opt = Kernel::default();
        k_opt.fixes.point_set_centroid_all_blocks = true;
        let center = Vec3::new(7.0, -2.0, 1.5);

        // 50 planar points around (7, -2, 1.5), without a user-given normal.
        let pts50 = planar_paired_points(25, center);

        // Default mode depends on call order: (0, 0, 0) before normal(), center after normal().
        let mut def_before = PointSet3d::new(pts50.clone(), 1e-9, None);
        assert_eq!(def_before.centroid(), Vec3::ZERO);
        let n = def_before.normal().unwrap();
        assert!((n.z.abs() - 1.0).abs() < 1e-12);
        assert!((def_before.centroid() - center).length() < 1e-12);

        // Opt-in mode is call-order independent: centroid -> normal == normal -> centroid.
        let mut opt_c_first = PointSet3d::new(pts50.clone(), 1e-9, None);
        let c50_before = opt_c_first.centroid_with_kernel(&k_opt);
        assert!(opt_c_first.cache.has(Cache::CENTROID));
        let _ = opt_c_first.normal().unwrap();
        let c50_after = opt_c_first.centroid_with_kernel(&k_opt);

        let mut opt_n_first = PointSet3d::new(pts50.clone(), 1e-9, None);
        let _ = opt_n_first.normal().unwrap();
        let c50_n_first = opt_n_first.centroid_with_kernel(&k_opt);

        assert!((c50_before - center).length() < 1e-12);
        assert!((c50_after - center).length() < 1e-12);
        assert!((c50_n_first - center).length() < 1e-12);
        assert!((c50_before - c50_n_first).length() < 1e-12);

        // With a user-given normal, SetUpAxes does not run BestFitPlaneToPoints: default
        // stays (0, 0, 0) even after normal(), while opt-in gives center in both orders.
        let mut z1 = Vec3::Z;
        let mut def_given = PointSet3d::new(pts50.clone(), 1e-9, Some(&mut z1));
        let _ = def_given.normal().unwrap();
        assert_eq!(def_given.centroid(), Vec3::ZERO);

        let mut z2 = Vec3::Z;
        let mut opt_given_c_first = PointSet3d::new(pts50.clone(), 1e-9, Some(&mut z2));
        let cg_before = opt_given_c_first.centroid_with_kernel(&k_opt);
        let _ = opt_given_c_first.normal().unwrap();
        let cg_after = opt_given_c_first.centroid_with_kernel(&k_opt);

        let mut z3 = Vec3::Z;
        let mut opt_given_n_first = PointSet3d::new(pts50, 1e-9, Some(&mut z3));
        let _ = opt_given_n_first.normal().unwrap();
        let cg_n_first = opt_given_n_first.centroid_with_kernel(&k_opt);

        assert_eq!(cg_before, center);
        assert_eq!(cg_after, center);
        assert_eq!(cg_n_first, center);

        // 100 planar points: first 50 offset positively, second 50 negatively around center.
        let pts100 = planar_paired_points(50, center);

        let mut def100 = PointSet3d::new(pts100.clone(), 1e-9, None);
        assert_eq!(def100.centroid(), Vec3::new(9.75, 5.5, 0.75));
        let _ = def100.normal().unwrap();
        assert_eq!(def100.centroid(), center);

        let mut opt100_c_first = PointSet3d::new(pts100.clone(), 1e-9, None);
        let c100_before = opt100_c_first.centroid_with_kernel(&k_opt);
        assert!(opt100_c_first.cache.has(Cache::CENTROID));
        let _ = opt100_c_first.normal().unwrap();
        let c100_after = opt100_c_first.centroid_with_kernel(&k_opt);

        let mut opt100_n_first = PointSet3d::new(pts100, 1e-9, None);
        let _ = opt100_n_first.normal().unwrap();
        let c100_n_first = opt100_n_first.centroid_with_kernel(&k_opt);

        assert_eq!(c100_before, center);
        assert_eq!(c100_after, center);
        assert_eq!(c100_n_first, center);
    }
}
