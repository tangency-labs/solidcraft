//! Interval insertion, homogenization and endpoint repair for curve classifications.
//!
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`CurveClassification::homogenize`] | `SmCurveClassification::Homogenize` |
//! | [`CurveClassification::insert_curve_interval`] | `SmCurveClassification::InsertCurveInterval` |
//! | [`CurveClassification::fix_end_point_class`] | `SmCurveClassification::FixEndPointClass` |
//! | [`CurveClassification::merge_interval`] | `SmCurveClassification::MergeInterval` |
//!
//! Deviations: handles and caller-provided geometry replace pointers, as in p1/p3.
//! Status/out-parameters become Results. The optional classify-curve parameter passed
//! to `ComputePointParameters` is explicitly unused upstream and is omitted here.
//! Undefined indices return an error. Debug-only
//! diagnostics are omitted. Insertion restarts after a squeeze in a loop, in upstream
//! order, bounded by the initial interval count plus one (every restart removes one).
//! Homogenization's final squeezes must reduce the count. Nonempty boundary lookups
//! await p2, as does interval splicing/squeezing.
//! `MergeInterval` ports its early exits and the boundary zone tolerances
//! (`GetPointObjZoneTol3d`, p2); the topology mutation after them is not implemented
//! or certified here and returns `Unported` naming this unit.

use super::p3::{
    are_coincident_maybe, classify_crv_loc_by_index, connect_intervals, point_class_loc_by_index,
    Coincident,
};
use super::{
    curve_is_degenerate, fix_problems_by_squeezing, squeeze_out_smallest_interval,
    CurveClassification, CurveInterval, IntervalPosition,
};
use crate::error::{Result, SolidError};
use crate::geom::tol::ScaledZero;
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, PointClassType as Pc, PointClassification, Precedence,
};
use crate::kernel::Kernel;
use crate::math::{Point3, BIG_DOUBLE};

/// Topology created by [`CurveClassification::merge_interval`].
/// These replace the three optional output arrays, which upstream clears on entry.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeIntervalOutput<O> {
    /// Faces created by splitting an existing face.
    pub new_faces: Vec<O>,
    /// Edges created or reused by splitting or merging.
    pub new_edges: Vec<O>,
    /// Vertices created by merging the interval's boundaries.
    pub new_vertices: Vec<O>,
}

impl<O> Default for MergeIntervalOutput<O> {
    fn default() -> Self {
        Self {
            new_faces: Vec::new(),
            new_edges: Vec::new(),
            new_vertices: Vec::new(),
        }
    }
}

const OP: &str = "intersect::curve_class::p6";

fn combine<O: ClassObject>(
    target: &mut PointClassification<O>,
    other: &PointClassification<O>,
) -> Result<()> {
    if target.combine(other)? == Precedence::Other {
        *target = *other;
    }
    Ok(())
}

impl<O: ClassObject> CurveClassification<O> {
    /// Inserts each distinct boundary into the paired classification (`Homogenize`).
    /// Near boundaries retain upstream's sliding/squeezing heuristics.
    ///
    /// # Errors
    /// Propagates classification/geometry failures and unported p2/p4/p7 calls.
    pub fn homogenize(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        other: &mut Self,
    ) -> Result<()> {
        homogenize_boundaries(k, geometry, self, other)?;
        homogenize_boundaries(k, geometry, other, self)?;
        fix_problems_by_squeezing(k, geometry, self, other)?;
        fix_problems_by_squeezing(k, geometry, other, self)?;
        while self.size() > other.size() {
            let count = self.size();
            crate::error::ignore_status(squeeze_out_smallest_interval(self))?;
            if self.size() >= count {
                return Err(SolidError::sm_err(OP));
            }
        }
        while other.size() > self.size() {
            let count = other.size();
            crate::error::ignore_status(squeeze_out_smallest_interval(other))?;
            if other.size() >= count {
                return Err(SolidError::sm_err(OP));
            }
        }
        Ok(())
    }

    /// Integrates a homogeneous interval, combining classifications in upstream order.
    ///
    /// # Errors
    /// Propagates tolerance, geometry and unported p2/p7 failures. Invalid indices and
    /// a restart without progress return an error instead of recursing indefinitely.
    pub fn insert_curve_interval(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        input: &CurveInterval<O>,
    ) -> Result<()> {
        let mut restarts = self.size().saturating_add(1);
        loop {
            if !self.insert_curve_interval_step(k, geometry, input)? {
                return Ok(());
            }
            restarts = restarts
                .checked_sub(1)
                .ok_or_else(|| SolidError::sm_err(OP))?;
        }
    }

    // true means upstream's tail-recursive restart after removal of an interval.
    fn insert_curve_interval_step(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        input: &CurveInterval<O>,
    ) -> Result<bool> {
        let tol = input.mid.obj_zone_tol_3d(k, geometry)?;
        let a = self.find_interval(k, geometry, input.min(), BIG_DOUBLE, tol)?;
        let b = self.find_interval(k, geometry, input.max(), BIG_DOUBLE, tol)?;
        let (mut i, mut j, mut pa, mut pb) = (a.index, b.index, a.position, b.position);
        if pb == IntervalPosition::Start && j > i {
            pb = IntervalPosition::End;
            j -= 1;
        }
        if pa == IntervalPosition::End && i < j {
            pa = IntervalPosition::Start;
            i += 1;
        }
        if i != j {
            for index in [i, j] {
                let (coin, end) = are_coincident_maybe(k, geometry, self, self, index)?;
                match coin {
                    Coincident::Unsure => return Err(SolidError::sm_err(OP)),
                    Coincident::Yes => {
                        connect_intervals(self, index, end, false)?;
                        return Ok(true);
                    }
                    Coincident::No => {}
                }
            }
            if (input.start.point_class() == Pc::Face
                && input.mid.point_class() == Pc::Face
                && input.end.point_class() == Pc::Face)
                || (input.start.point_class() == Pc::Edge
                    && input.mid.point_class() == Pc::Edge
                    && input.end.point_class() == Pc::Edge)
            {
                for index in i..=j {
                    let ivl = self.at_mut(index)?;
                    combine(&mut ivl.start, &input.start)?;
                    combine(&mut ivl.mid, &input.mid)?;
                    combine(&mut ivl.end, &input.end)?;
                }
                return Ok(false);
            }
            while i < j {
                let first = *self.at(i)?;
                let last = *self.at(j)?;
                if interval_degenerate(k, geometry, self, &first)? {
                    i += 1;
                } else if interval_degenerate(k, geometry, self, &last)? {
                    j -= 1;
                } else {
                    break;
                }
            }
            if j == i + 1 {
                let first = *self.at(i)?;
                let last = *self.at(j)?;
                if first.start.point_class() != Pc::Vertex {
                    if first.interval.length() < last.interval.length() * 10.0 {
                        connect_intervals(self, i, IntervalPosition::End, false)?;
                        return Ok(true);
                    }
                    if last.interval.length() < first.interval.length() * 10.0 {
                        connect_intervals(self, j, IntervalPosition::Start, false)?;
                        return Ok(true);
                    }
                }
                for (index, ivl) in [(i, first), (j, last)] {
                    if ivl.start.point_class() == Pc::Vertex
                        && ivl.end.point_class() == Pc::Vertex
                        && ivl.interval.length() < self.interval().length() / 1000.0
                    {
                        if ivl.start.deviation() < ivl.end.deviation() / 1000.0 {
                            // RemovePointClass's status is ignored upstream.
                            crate::error::ignore_status(
                                self.remove_point_class(index, IntervalPosition::End),
                            )?;
                            return Ok(true);
                        } else if ivl.end.deviation() < ivl.start.deviation() / 1000.0 {
                            crate::error::ignore_status(
                                self.remove_point_class(index, IntervalPosition::Start),
                            )?;
                            return Ok(true);
                        }
                    }
                }
            }
            if i != j && input.mid.point_class() == Pc::Edge {
                return Ok(false);
            }
        }
        if pa == pb && matches!(pa, IntervalPosition::Start | IntervalPosition::End) {
            let ivl = self.at_mut(i)?;
            let end = if pa == IntervalPosition::Start {
                &mut ivl.start
            } else {
                &mut ivl.end
            };
            combine(end, &input.start)?;
            combine(end, &input.end)?;
            return Ok(false);
        }
        if pa == IntervalPosition::Start {
            combine(&mut self.at_mut(i)?.start, &input.start)?;
        } else if pa == IntervalPosition::Inside {
            if !self.insert_point_class(
                k,
                geometry,
                &input.start,
                input.min(),
                BIG_DOUBLE,
                false,
            )? {
                return Ok(false);
            }
            i += 1;
        } else {
            return Err(SolidError::sm_err(OP));
        }
        if pb == IntervalPosition::End {
            combine(&mut self.at_mut(i)?.end, &input.end)?;
        } else if pb == IntervalPosition::Inside {
            let found = self.find_interval(k, geometry, input.max(), BIG_DOUBLE, tol)?;
            if found.position != IntervalPosition::Inside {
                return Ok(false);
            }
            self.insert_point_class(k, geometry, &input.end, input.max(), BIG_DOUBLE, false)?;
        } else {
            return Err(SolidError::sm_err(OP));
        }
        combine(&mut self.at_mut(i)?.mid, &input.mid)?;
        Ok(false)
    }

    /// Repairs an endpoint classified to an edge (`FixEndPointClass`).
    /// Returns whether the classification changed.
    ///
    /// # Errors
    /// Propagates p7 boundary lookup, p2 splicing and geometry failures.
    pub fn fix_end_point_class(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        point: Point3,
    ) -> Result<bool> {
        if self.size() == 0 || index > self.size() {
            return Err(SolidError::invalid_input(OP, "invalid boundary index"));
        }
        let loc = point_class_loc_by_index(self, index)?;
        let pc = *self.pc_at(loc)?;
        if pc.point_class() != Pc::Edge {
            return Ok(false);
        }
        let mut open = true;
        if index > 0 && self.at(index - 1)?.mid.point_class() == Pc::Edge {
            open = false;
        }
        if index < self.size() && self.at(index)?.mid.point_class() == Pc::Edge {
            open = false;
        }
        if open && super::change_edge_point_class_to_near_vertex(k, geometry, index, self, point)? {
            return Ok(true);
        }
        if index > 0 && index < self.size() {
            let prev = &self.at(index - 1)?.mid;
            let next = &self.at(index)?.mid;
            if prev.point_class() == Pc::Edge
                && next.point_class() == Pc::Edge
                && prev.object() == next.object()
                && prev.object() != pc.object()
            {
                connect_intervals(self, index, IntervalPosition::Start, true)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Merges an interval's classifications into topology (`MergeInterval`).
    /// The unknown-interior and vertex/edge/vertex early exits and the boundary zone
    /// tolerances (`GetPointObjZoneTol3d`) are implemented; the vertex and edge
    /// imprinting after them is not ported yet.
    ///
    /// # Errors
    /// Invalid interval indices, or [`SolidError::Unported`] for topology tolerances of
    /// opaque handles, p7 fillet handling or the imprinting.
    pub fn merge_interval(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        vertices_only: bool,
    ) -> Result<MergeIntervalOutput<O>> {
        self.merge_interval_with(k, geometry, index, vertices_only, None)
    }

    /// [`Self::merge_interval`] with the classification that
    /// [`Self::other_classification`] names (`GetOtherClassification()`), which the
    /// fillet geometry step (`sm_SetDoFilletingGeometry`) reads. Without it, a
    /// classification doing fillets with an other classification defers that step.
    ///
    /// # Errors
    /// Those of [`Self::merge_interval`]; [`SolidError::Unported`] from the fillet
    /// geometry step (whose other statuses upstream ignores).
    pub fn merge_interval_with(
        &mut self,
        k: &Kernel,
        geometry: &impl ClassGeometry<O>,
        index: usize,
        vertices_only: bool,
        other: Option<&CurveClassification<O>>,
    ) -> Result<MergeIntervalOutput<O>> {
        let ivl = self.at(index)?;
        if ivl.mid.point_class() == Pc::Unknown && !vertices_only {
            return Ok(MergeIntervalOutput::default());
        }
        if ivl.start.point_class() == Pc::Vertex
            && ivl.mid.point_class() == Pc::Edge
            && ivl.end.point_class() == Pc::Vertex
        {
            // When DoingFillets with FilletData
            if self.doing_fillets() && self.other_classification().is_some() {
                let Some(other) = other else {
                    // the caller did not supply the classification the handle names
                    return Err(SolidError::unported(
                        "SmCurveClassification::GetOtherClassification",
                        "intersect.curve_class_p7",
                    ));
                };
                // Make sure to use the FilletGeometry and not the OriginalBrep Geometry
                // (1 | 2 | 4: start vertex, end vertex and mid edge; status ignored)
                crate::error::ignored(super::p7::set_do_filleting_geometry(
                    k,
                    geometry,
                    index,
                    self,
                    Some(other),
                    7,
                ))?;
            }
            return Ok(MergeIntervalOutput::default());
        }
        // Get the interval start and stop tols.
        let start_obj_zone_tol_3d = self.point_obj_zone_tol_3d(k, geometry, index)?;
        let end_obj_zone_tol_3d = self.point_obj_zone_tol_3d(k, geometry, index + 1)?;
        let _ = (start_obj_zone_tol_3d, end_obj_zone_tol_3d);
        // The rest of MergeInterval (end points, closed / degenerate test, vertex and
        // edge imprinting through MergePointClass and the B-rep) is not ported in this
        // part yet; see plan/findings.md (intersect.curve_class_p2).
        Err(SolidError::unported(
            "SmCurveClassification::MergeInterval (imprinting)",
            "intersect.curve_class_p6",
        ))
    }
}

fn interval_degenerate<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    ivl: &CurveInterval<O>,
) -> Result<bool> {
    let curve = cc
        .curve()
        .and_then(|o| geometry.curve(o))
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let surface = cc
        .surface1()
        .map(|o| {
            geometry
                .surface(o)
                .ok_or_else(|| SolidError::invalid_input(OP, "surface handle does not resolve"))
        })
        .transpose()?;
    let tol = super::xsect_tol_2d_or_3d_for_classify_pair(
        k,
        curve,
        surface,
        ivl.min(),
        ivl.start.obj_zone_tol_3d(k, geometry)?,
        ivl.max(),
        ivl.end.obj_zone_tol_3d(k, geometry)?,
    )?;
    curve_is_degenerate(k, Some(curve), &ivl.interval, tol)
}

fn homogenize_boundaries<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    src: &CurveClassification<O>,
    dst: &mut CurveClassification<O>,
) -> Result<()> {
    let count = src.size();
    let mut last = BIG_DOUBLE;
    for index in 0..=count {
        if count == 0 {
            break;
        }
        let loc = point_class_loc_by_index(src, index)?;
        let pc = src.pc_at(loc)?;
        let ivl = src.at(loc.interval)?;
        let param = if loc.end == IntervalPosition::Start {
            ivl.min()
        } else {
            ivl.max()
        };
        let point = classify_crv_loc_by_index(k, geometry, src, index)?;
        let tol = pc.obj_zone_tol_3d(k, geometry)?;
        let found = dst.find_interval(k, geometry, param, last, tol)?;
        let mut pos = found.position;
        if pos == IntervalPosition::Outside {
            return Err(SolidError::sm_err(OP));
        }
        let mut force = false;
        if pos != IntervalPosition::Inside && found.end_dist_3d.abs() > point.scaled_zero() {
            let ivl = dst.at(found.index)?;
            let end = if pos == IntervalPosition::Start {
                &ivl.start
            } else {
                &ivl.end
            };
            let lower = if index > 0 {
                src.at(index - 1)?.mid.point_class()
            } else {
                Pc::Unknown
            };
            let upper = if index < count {
                src.at(index)?.mid.point_class()
            } else {
                Pc::Unknown
            };
            let up = (pos == IntervalPosition::Start && param < ivl.min())
                || (pos == IntervalPosition::End && param < ivl.max());
            if ivl.mid.point_class() == end.point_class()
                && ((up && upper == Pc::Unknown) || (!up && lower == Pc::Unknown))
            {
                pos = IntervalPosition::Inside;
                force = true;
            }
        }
        if pos == IntervalPosition::Inside {
            let mut new_pc = dst.at(found.index)?.mid;
            new_pc.set_pre_snap_param(param);
            new_pc.copy_sss_point(pc);
            if matches!(new_pc.point_class(), Pc::Face | Pc::Edge) {
                let point = classify_crv_loc_by_index(k, geometry, src, index)?;
                // Ordinary projection failures turn the classification into UNKNOWN.
                match new_pc.compute_point_parameters(k, geometry, point) {
                    Ok(()) => {}
                    Err(e) if e.is_unported() => return Err(e),
                    Err(_) => new_pc.set_class_object(Pc::Unknown, None),
                }
            }
            if !dst.insert_point_class(k, geometry, &new_pc, param, last, force)? {
                return Err(SolidError::sm_err(OP));
            }
        }
        last = param;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::p2::Curve;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::geom::extent1d::Extent1d;
    use crate::intersect::point_class::ClassObjectKind;
    use crate::math::Vec3;
    use crate::surface::surface::Surface;
    use crate::surface::volume::Volume;

    #[derive(Clone, Copy, PartialEq)]
    struct H;
    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            ClassObjectKind::Curve
        }
    }
    struct G(Curve);
    impl ClassGeometry<H> for G {
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
    fn setup() -> (Kernel, G, CurveClassification<H>) {
        let k = Kernel::default();
        let range = Extent1d::from_min_max(0.0, 1.0);
        let g = G(Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            &range,
            &BoundedLineOptions::default(),
        )));
        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(H), false);
        cc.set_size(&k, 1).unwrap();
        cc.at_mut(0).unwrap().set_interval(&range);
        (k, g, cc)
    }
    #[test]
    fn full_interval_combines_all_three_classifications() {
        let (k, g, mut cc) = setup();
        let mut input = CurveInterval::with_interval(&k, &Extent1d::from_min_max(0.0, 1.0), None);
        input.start.set_class_object(Pc::Curve, Some(H));
        input.mid = input.start;
        input.end = input.start;
        input.start.set_t_param(0.125, false);
        input.mid.set_t_param(0.5, false);
        input.end.set_t_param(0.875, false);
        cc.insert_curve_interval(&k, &g, &input).unwrap();
        let got = cc.at(0).unwrap();
        assert_eq!(got.start.t_param(), 0.125);
        assert_eq!(got.mid.t_param(), 0.5);
        assert_eq!(got.end.t_param(), 0.875);
        assert_eq!(cc.size(), 1);
    }
    #[test]
    fn collapsed_interval_combines_start_then_end_and_preserves_sss() {
        let (k, g, mut cc) = setup();
        let mut input = CurveInterval::with_interval(&k, &Extent1d::from_min_max(0.0, 0.0), None);
        input.start.set_class_object(Pc::Curve, Some(H));
        input.start.set_t_param(0.25, false);
        input.end = input.start;
        input.end.set_t_param(0.75, false);
        input.end.set_sss_point(Vec3::new(2.0, 3.0, 4.0));
        cc.insert_curve_interval(&k, &g, &input).unwrap();
        let got = cc.at(0).unwrap();
        assert_eq!(got.start.t_param(), 0.25);
        assert_eq!(got.start.sss_point(), Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(got.mid.point_class(), Pc::Unknown);
    }
    #[test]
    fn missing_curve_and_extreme_indices_return_errors() {
        let (k, g, mut cc) = setup();
        cc.set_curve(None, false);
        let input = CurveInterval::with_interval(&k, &Extent1d::from_min_max(0.25, 0.75), None);
        assert!(cc.insert_curve_interval(&k, &g, &input).is_err());
        assert!(cc.merge_interval(&k, &g, usize::MAX, false).is_err());
        assert!(matches!(
            cc.fix_end_point_class(&k, &g, usize::MAX, Vec3::ZERO),
            Err(SolidError::InvalidInput { .. })
        ));
    }
    #[test]
    fn unclassified_merge_and_empty_homogenization_use_real_api() {
        let (k, g, mut cc) = setup();
        assert!(cc.merge_interval(&k, &g, 0, false).is_ok());
        let mut a = CurveClassification::<H>::new(&k);
        let mut b = CurveClassification::<H>::new(&k);
        assert!(a.homogenize(&k, &g, &mut b).is_ok());
    }
}
