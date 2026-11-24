//! Sweeps of a planar profile along a planar path of several curves.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation::create_sweep_along_planar_path`] | eight-argument static `CreateSweepAlongPlanarPath` |
//! | `sweep_profiles_along_path` (private) | `SweepProfilesAlongPath` |
//!
//! Deviations: the static function takes the access and the B-rep whose context
//! upstream's `crContext` is; it returns the new B-rep. A path curve that is not a
//! B-spline (an invalid C cast upstream, UB-113) is an error. A cutting curve that
//! was never built (a null dereference upstream) is an error.
//! `CreateExtendedCurve`'s end flag 1 is the start and 0 or 2 the end, as in NLib.
//! Not upstream (#483): the statuses upstream ignores here propagate, and every owned
//! temporary B-rep is released on failure. Successful prefix edits are kept; the
//! caller's context body and a transferred result are never released. The path
//! plane normal is computed only at a mitre (a straight path needs none). A failed
//! mitre intersection search (`BrepIntersect`, `GlobalCurveSolve`) is returned too,
//! as the SolidCraft fork does (tangency-labs/smlib c8f6a31, WR-662).
//! Not upstream
//! (WR-400, tangency-labs/solidcore#837): `SweepProfilesAlongPath` asks for caps only for a profile that closes.
//! Not upstream (WR-780, #13687): the reference points of a mitre trim stay within the
//! reach of the cutting line. No recursion.
//! Not upstream (WR-793, #17314): a corner where the path turns back within 2 degrees is refused,
//! where upstream takes it for a smooth joint and never mitres it, and so is a sweep of a
//! closed profile meant as a solid that does not close (upstream returns it).

use super::{as_bspline, PrimitiveCreationAccess};
use crate::curve::curve::p9::CurveSolveOptions;
use crate::curve::curve::Curve;
use crate::error::primitive::PrimitiveFailure;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Vec3, BIG_DOUBLE};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::crv_adv::p1::CurveEnd;
use crate::primitive::primitive_creation::p1::{
    extend_path_curves, position_profile_curves_impl, PrimitiveCreation,
};
use crate::primitive::primitive_creation::SweepOptions;
use crate::topology::brep::p10::TrimType;
use crate::topology::BrepId;

const OP: &str = "primitive::primitive_creation::p2::planar_path";

/// The solver handles of the cutting curve (0) and an intersection curve (1).
#[derive(Debug, Clone, Copy, PartialEq)]
struct PathCurve(u8, Extent1d);
impl SolutionObject for PathCurve {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.1)
    }
}

/// `Evaluate(t, 1, from_left, out)` (non-zero tangents); its status is returned (not
/// upstream, #483).
fn evaluate_into(
    k: &Kernel,
    curve: &Curve,
    t: f64,
    from_left: bool,
    out: &mut [Vec3; 2],
) -> Result<()> {
    let values = curve.evaluate(k, t, 1, from_left, true)?;
    for (target, value) in out.iter_mut().zip(values) {
        *target = value;
    }
    Ok(())
}

/// `CreateExtendedCurve(dist, end, SM_CT_G1, out)`; its status is returned (not
/// upstream, #483), and no extension leaves `out`.
fn extend_into(
    curve: &BSplineCurve,
    distance: f64,
    end: CurveEnd,
    out: &mut Option<BSplineCurve>,
) -> Result<()> {
    if let Some(next) =
        curve.create_extended_curve_by_distance(distance, end, ContinuityType::G1)?
    {
        *out = Some(next);
    }
    Ok(())
}

/// `CreateLineSegment(3, start, end, out)`; its status is returned (not upstream, #483),
/// and the line is returned (it is always made when the status is good).
fn line(start: Vec3, end: Vec3) -> Result<BSplineCurve> {
    BSplineCurve::create_line_segment(3, start, end)
}

// Why each step that fails here is the kernel's fault (`PrimitiveFailure::Defect`).

/// Why the temporary cutting curve is not there.
const NO_TEMP_CUTTING: &str = "the temporary cutting curve is not there, though the distance \
     extension of the corner's cutting line just made it, and that extension returns a curve \
     whenever it succeeds";
/// Why the extended cutting curve of a closed path's last corner is not there.
const NO_EXTENDED_CUTTING: &str = "the cutting curve of the closed path's last corner is not \
     there, though the distance extension of its cutting line just made it, and that \
     extension returns a curve whenever it succeeds";
/// Why the union of the pieces does not leave one B-rep.
const NOT_ONE_RESULT: &str = "the union of the pieces does not leave exactly one B-rep, though \
     its tree names each piece once with one union per piece after the first, and the Boolean \
     tree leaves one result per tree and no unused B-rep";

/// Not upstream (WR-400, tangency-labs/solidcore#837): whether the profile curves
/// close, every curve end meeting another curve end (a closed curve's own other end)
/// within `tol`.
fn profile_closes(k: &Kernel, profiles: &[Curve], tol: f64) -> Result<bool> {
    let mut ends = Vec::new();
    for curve in profiles {
        let domain = curve.natural_interval();
        ends.push(curve.evaluate_point(k, domain.min())?);
        ends.push(curve.evaluate_point(k, domain.max())?);
    }
    Ok(ends.iter().enumerate().all(|(i, end)| {
        ends.iter()
            .enumerate()
            .any(|(j, other)| j != i && end.distance(*other) < tol)
    }))
}

/// `SweepProfilesAlongPath`: sweep the positioned profiles along one path segment,
/// capped only when the profile closes (see [`profile_closes`]), then remove the unwanted caps. The
/// sweep's and removals' statuses propagate (not upstream, #483, which ignores them).
fn sweep_profiles_along_path(
    a: &mut impl PrimitiveCreationAccess,
    profiles: &[Curve],
    path: &BSplineCurve,
    tol: f64,
    cap_start: bool,
    cap_end: bool,
    move_profile_center_to_path: bool,
    brep: BrepId,
) -> Result<()> {
    // Not upstream (WR-400, #837): upstream always asks for caps; an open profile bounds
    // no planar region, its cap fails and `CreateCurveSweep` returns before stitching
    // its walls (a failure upstream ignores).
    let cap_ends = profile_closes(a.kernel(brep)?, profiles, tol)?;

    // CreateCurveSweep somehow mangles the path curve
    let path = path.copy();

    let mut start_faces = Vec::new();
    let mut side_faces = Vec::new();
    let mut end_faces = Vec::new();
    let options = SweepOptions {
        cap_ends,
        translational_sweep: false,
        move_profile_center_to_path,
        orient_profile_perpendicular_to_path: false,
        move_to_profile: false,
        profile_at_path_start: false,
        up: None,
    };
    let region = a.infinite_region(brep)?;
    let mut primitive = PrimitiveCreation::new(Some(region), 0, false);
    let profiles: Vec<Option<Curve>> = profiles.iter().map(|c| Some(c.clone())).collect();
    primitive.create_curve_sweep(
        a,
        brep,
        &profiles,
        &path,
        None,
        None,
        tol,
        options,
        false,
        &mut start_faces,
        &mut side_faces,
        &mut end_faces,
    )?;

    if !cap_start {
        a.set_editing(brep, true)?;
        a.remove_faces(brep, &start_faces)?;
    }
    if !cap_end {
        a.set_editing(brep, true)?;
        a.remove_faces(brep, &end_faces)?;
    }
    Ok(())
}

/// Reference points on either side of a cutting line, then trim the B-reps on both
/// sides (`ProjectAndTrim(..., SM_TT_DELETE_POINT, &dTol, NULL, NULL)`, whose
/// statuses upstream ignores and these return, not upstream, #483).
fn trim_both(
    a: &mut impl PrimitiveCreationAccess,
    breps: [BrepId; 2],
    cutting: &BSplineCurve,
    normal: Vec3,
    references: [Vec3; 2],
    tol: f64,
) -> Result<()> {
    let [before, after] = breps;
    let [reference1, reference2] = references;
    a.project_and_trim(
        before,
        cutting,
        normal,
        reference2,
        TrimType::DeletePoint,
        tol,
    )?;
    a.project_and_trim(
        after,
        cutting,
        normal,
        reference1,
        TrimType::DeletePoint,
        tol,
    )
}

/// Compute the normal only before the first operation that consumes it.
fn path_plane_normal(k: &Kernel, paths: &[Curve], cached: &mut Option<Vec3>) -> Result<Vec3> {
    if let Some(normal) = *cached {
        return Ok(normal);
    }
    let normal = crate::curve::curve::p3::compute_crvs_normal(k, paths)?;
    *cached = Some(normal);
    Ok(normal)
}

impl PrimitiveCreation {
    /// Sweep the planar `profiles` along the ordered, connected planar `paths`
    /// (eight-argument static `CreateSweepAlongPlanarPath`): one capped sweep per
    /// path curve, mitred at tangent discontinuities, the pieces united, stitched
    /// and classified. `cap_start` / `cap_end` cap the ends of an open path; a closed
    /// path has no end caps. Returns the new B-rep, in `context_of`'s context.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for no path curves or a path curve that is not a B-spline;
    /// `SM_ERR` when the pieces do not unite into one B-rep, or for a manifold
    /// result without material; `SM_ERR` for a corner where the path turns back within
    /// 2 degrees, and for a closed profile meant as a solid (`cap_start` and `cap_end`, or
    /// a closed path) whose result does not close (not upstream, WR-793); those of the
    /// union, the final stitch and the region classification. Mandatory evaluations, geometry construction, cap
    /// removals, trims and cleanup also propagate (not upstream, #483); every owned
    /// temporary is deleted on any error. No transaction is imposed on the caller's
    /// B-rep.
    pub fn create_sweep_along_planar_path(
        a: &mut impl PrimitiveCreationAccess,
        context_of: BrepId,
        profiles: &[Curve],
        paths: &[Curve],
        tol: f64,
        cap_start: bool,
        cap_end: bool,
        move_profile_center_to_path: bool,
    ) -> Result<BrepId> {
        let mut owned = Vec::new();
        let result = Self::create_sweep_along_planar_path_owned(
            a,
            context_of,
            profiles,
            paths,
            tol,
            cap_start,
            cap_end,
            move_profile_center_to_path,
            &mut owned,
        );
        if result.is_err() {
            for id in owned {
                a.delete_brep(id);
            }
        }
        result
    }

    fn create_sweep_along_planar_path_owned(
        a: &mut impl PrimitiveCreationAccess,
        context_of: BrepId,
        profiles: &[Curve],
        paths: &[Curve],
        tol: f64,
        cap_start: bool,
        cap_end: bool,
        move_profile_center_to_path: bool,
        owned: &mut Vec<BrepId>,
    ) -> Result<BrepId> {
        let first_path = paths
            .first()
            .ok_or_else(|| SolidError::invalid_input(OP, "no path curves"))?;
        let last_path = paths
            .last()
            .ok_or_else(|| SolidError::invalid_input(OP, "no path curves"))?;
        let mut bspline_paths = Vec::with_capacity(paths.len());
        for path in paths {
            bspline_paths
                .push(as_bspline(a.kernel(context_of)?, path)?.ok_or_else(|| {
                    SolidError::invalid_input(OP, "path curve is not a B-spline")
                })?);
        }

        // Determine whether path curves form a closed and periodic loop.
        let mut start_pv = [Vec3::UNDEF; 2];
        let mut end_pv = [Vec3::UNDEF; 2];
        evaluate_into(
            a.kernel(context_of)?,
            first_path,
            first_path.natural_interval().min(),
            true,
            &mut start_pv,
        )?;
        evaluate_into(
            a.kernel(context_of)?,
            last_path,
            last_path.natural_interval().max(),
            true,
            &mut end_pv,
        )?;
        let [start_point, start_tangent] = start_pv;
        let [end_point, end_tangent] = end_pv;
        let closed = start_point.distance(end_point) < tol;
        let periodic = closed && start_tangent.is_parallel_to(end_tangent, 2.0);

        // Determine max size of profile
        let k = a.kernel(context_of)?;
        let mut total = Extent3d::new();
        let mut bounds = Extent3d::new();
        for profile in profiles {
            let boxes = profile.calculate_bounding_box(
                k,
                &profile.natural_interval(),
                BoundingBoxRequest {
                    normal: true,
                    ..BoundingBoxRequest::default()
                },
                true,
            )?;
            if let Some(normal) = boxes.normal {
                bounds = normal;
            }
            total.add_point(bounds.min());
            total.add_point(bounds.max());
        }
        let max_dimension = total.max_dimension();
        let mut extend = max_dimension * 2.0_f64.sqrt();
        extend *= 2.0;

        // Extend all the path curves at start and end to make sure sweeps go past corner
        let extended = extend_path_curves(&bspline_paths, closed, periodic, extend)?;

        // A plane normal is only needed at a mitre. A single straight path has
        // no unique normal (`ComputeCrvsNormal` returns SM_ERR) and needs none.
        // Not upstream (#483): upstream computes it here and ignores its status.
        let mut path_normal = None;

        // Prepare for loop where we position profile curves, sweep, and trim
        let mut new_breps: Vec<BrepId> = Vec::new();
        let mut before_brep: Option<BrepId> = None;
        let mut first_brep: Option<BrepId> = None;
        let mut cutting: Option<BSplineCurve> = None;
        let mut temp_cutting: Option<BSplineCurve> = None;
        let mut before = [Vec3::UNDEF; 2];
        let mut after = [Vec3::UNDEF; 2];
        let mut first_pv = [Vec3::UNDEF; 2];

        for (ii, (path, extended_path)) in paths.iter().zip(&extended).enumerate() {
            // find start point and derivative of new path segment
            let range = path.natural_interval();
            evaluate_into(a.kernel(context_of)?, path, range.min(), false, &mut after)?;
            after[1] = after[1].unitize_or_z();

            // position profile curves at the start of the path segment
            let mut new_profiles = Vec::new();
            position_profile_curves_impl(
                a.kernel(context_of)?,
                profiles,
                tol,
                &Curve::from(extended_path.clone()),
                &mut new_profiles,
                paths,
            )?;

            // sweep the profiles along this path segment creating new brep
            let after_brep = a.new_brep(context_of)?;
            owned.push(after_brep);
            let cap_this_start = ii == 0 && !closed && cap_start;
            let cap_this_end = ii == paths.len() - 1 && !closed && cap_end;
            sweep_profiles_along_path(
                a,
                &new_profiles,
                extended_path,
                tol,
                cap_this_start,
                cap_this_end,
                move_profile_center_to_path,
                after_brep,
            )?;
            new_breps.push(after_brep);

            // The first segment only records its derivatives; later ones cut both
            // breps at the mitre. Past the first, the path before and its piece are there
            // (the piece is recorded at the end of each pass).
            let previous_piece = ii
                .checked_sub(1)
                .and_then(|i| paths.get(i))
                .zip(before_brep);
            if ii == 0 {
                first_brep = Some(after_brep);
                first_pv = after;
            } else if before[1].parallel_sense(after[1], 2.0) == (true, Some(true)) {
                return Err(SolidError::primitive(OP, PrimitiveFailure::PathTurnsBack));
            } else if let Some((previous, before_id)) =
                previous_piece.filter(|_| !before[1].is_parallel_to(after[1], 2.0))
            {
                let path_normal =
                    path_plane_normal(a.kernel(context_of)?, paths, &mut path_normal)?;
                // Average the derivatives to determine cutting plane normal.
                let mut average = (-before[1]) + after[1];
                average = average.unitize_or_z();

                // An inner corner flips the average direction to extend correctly.
                let angle_between =
                    rad_to_deg(path_normal.ccw_angle_between(-before[1], after[1])?);
                if angle_between > 0.0 {
                    average = -average;
                }

                // Create temp cutting curve: the line from the corner along the
                // average derivative, then extended backwards.
                let cutting_point = after[0] + (average * extend);
                let cut_line = line(after[0], cutting_point)?;
                extend_into(&cut_line, extend, CurveEnd::Start, &mut temp_cutting)?;

                cutting = None;
                if path.is_linear(a.kernel(context_of)?, tol, false)?.linear
                    && previous
                        .is_linear(a.kernel(context_of)?, tol, false)?
                        .linear
                {
                    cutting.clone_from(&temp_cutting);
                } else {
                    // Intersect the temp cutting curve with every intersection curve
                    // and build the real cutting curve through the two best points.
                    // `pBrepBefore->BrepIntersect(crContext, pBrepAfter, ..)`: the faces'
                    // caches, UV trim curves and classifications are made on their own
                    // B-reps (`after_brep` is of `context_of`'s context)
                    let intersection_search =
                        a.with_breps_mut(before_id, after_brep, |k, before, after, _, _, _, _| {
                            crate::topology::brep::p1::brep_intersect_mut(
                                k, before, after, tol, 2.0, false,
                            )
                        });
                    // fork c8f6a31: SER(pBrepBefore->BrepIntersect(..)).
                    let (intersections, _) = intersection_search?;
                    if intersections.len() > 1 {
                        let temp = temp_cutting.as_ref().ok_or_else(|| {
                            SolidError::primitive_broken_link(OP, NO_TEMP_CUTTING)
                        })?;
                        let temp = Curve::from(temp.clone());
                        let mut best = Vec3::UNDEF;
                        let mut best2 = Vec3::UNDEF;
                        let mut best_dist = BIG_DOUBLE;
                        let mut best_dist2 = BIG_DOUBLE;
                        for intersection in &intersections {
                            let options = CurveSolveOptions {
                                target_distance: None,
                                opt_vectors: None,
                                handles: [
                                    Some(PathCurve(0, temp.natural_interval())),
                                    Some(PathCurve(1, intersection.natural_interval())),
                                ],
                            };
                            // fork c8f6a31: SER(pTempCuttingCrv->GlobalCurveSolve(..)).
                            let solutions = temp.global_curve_solve(
                                a.kernel(context_of)?,
                                &temp.natural_interval(),
                                intersection,
                                &intersection.natural_interval(),
                                SolverOperation::Minimize,
                                tol,
                                &options,
                                SolutionRequested::Single,
                            )?;
                            for index in 0..solutions.len() {
                                let solution = solutions.get_at(index)?;
                                let on_line = temp.evaluate_point(
                                    a.kernel(context_of)?,
                                    solution.start.param(0)?,
                                )?;
                                let on_intersection = intersection.evaluate_point(
                                    a.kernel(context_of)?,
                                    solution.start.param(1)?,
                                )?;
                                let dist = on_intersection.distance(on_line);
                                // keep track of the best two solutions
                                if dist < best_dist {
                                    if best.is_initialized() {
                                        best2 = best;
                                    }
                                    best_dist2 = best_dist;
                                    best = on_intersection;
                                    best_dist = dist;
                                } else if dist < best_dist2 {
                                    best2 = on_intersection;
                                    best_dist2 = dist;
                                }
                            }
                        }
                        if best.is_initialized()
                            && best2.is_initialized()
                            && best.distance(best2) > tol * 10.0
                        {
                            let mut line = line(best, best2)?;
                            let mut extended_line = None;
                            extend_into(&line, extend, CurveEnd::End, &mut extended_line)?;
                            if let Some(next) = extended_line {
                                line = next;
                            }
                            extend_into(&line, extend, CurveEnd::Start, &mut cutting)?;
                        }
                    }
                }

                // Just as a backup: this should still work.
                if cutting.is_none() {
                    cutting.clone_from(&temp_cutting);
                }
                if let Some(curve) = cutting.as_ref() {
                    // determine reference points on either side of the cutting line.
                    // Upstream steps 0.1 along each leg. The cutting line reaches only
                    // `extend` from the corner, so for a small profile the foot of that
                    // point lies past the end of the line, where the trim finds it on
                    // neither side. The step stays within the reach of the line (not
                    // upstream, #13687, WR-780).
                    //
                    // The step also grows with the tolerance (not upstream, WR-780). The
                    // side test calls a point within `tol` of its foot on the cutting
                    // line undecided, and the trim fails. The cutting line runs through
                    // the corner along the bisector of the legs, so a point `step` along
                    // a leg lies `step * sin(beta / 2)` from it, where beta is the angle
                    // between the legs. A fixed 0.1 is inside the tolerance of a large
                    // part (tolerance 1e-5 of a part 1e4 across is 0.1).
                    //
                    // So `step = min(max(0.1, 100 tol), extend / 2)`, where
                    // `extend / 2 = sqrt(2) * max_dimension`, and the side test passes
                    // when `step * sin(beta / 2) > tol` (the larger of `tol` and the
                    // face's tolerance, as `project_and_trim` takes it). A corner reaches this
                    // point only when its legs are more than 2 degrees from parallel
                    // (the `is_parallel_to(.., 2.0)` test above; a hairpin within 2
                    // degrees is refused before it), so `sin(beta / 2) > sin(1 deg) = 0.0175`.
                    // Where `100 tol` sets the step, the point therefore lies at least
                    // `1.75 tol` from the line and the trim never fails. Where the reach
                    // cap wins (a profile with `max_dimension < 100 tol / sqrt(2)`, about
                    // `70.7 tol`), the condition is `sqrt(2) * max_dimension *
                    // sin(beta / 2) > tol`; when it fails, the side test refuses the
                    // sweep with "the point is on the curve" (a typed refusal, not a
                    // body). The floor 0.1 keeps upstream's step wherever `100 tol` is
                    // smaller.
                    let step = 0.1_f64.max(100.0 * tol).min(extend * 0.5);
                    let reference1 = before[0] + (-before[1] * step);
                    let reference2 = after[0] + (after[1] * step);
                    trim_both(
                        a,
                        [before_id, after_brep],
                        curve,
                        path_normal,
                        [reference1, reference2],
                        tol,
                    )?;
                }
            }

            // Get ready for next path segment
            before_brep = Some(after_brep);

            // find end derivative of path before
            evaluate_into(a.kernel(context_of)?, path, range.max(), true, &mut before)?;
            before[1] = before[1].unitize_or_z();

            // a closed path trims the end of the last and the beginning of the first
            // (the first pass recorded the first piece)
            if let Some(after_id) = first_brep.filter(|_| closed && ii == paths.len() - 1) {
                after = first_pv;

                if before[1].parallel_sense(after[1], 2.0) == (true, Some(true)) {
                    return Err(SolidError::primitive(OP, PrimitiveFailure::PathTurnsBack));
                }
                // tangent continuous paths need no trim
                if !before[1].is_parallel_to(after[1], 2.0) {
                    let path_normal =
                        path_plane_normal(a.kernel(context_of)?, paths, &mut path_normal)?;
                    before[1] = before[1].unitize_or_z();
                    after[1] = after[1].unitize_or_z();
                    let mut average = (-before[1]) + after[1];
                    let angle_between =
                        rad_to_deg(path_normal.ccw_angle_between(-before[1], after[1])?);
                    if angle_between > 0.0 {
                        average = -average;
                    }

                    // the cutting line runs 50 along the average and back
                    let cutting_point = after[0] + (average * 50.0);
                    let cut_line = line(after[0], cutting_point)?;
                    extend_into(&cut_line, 50.0, CurveEnd::Start, &mut cutting)?;
                    let curve = cutting.as_ref().ok_or_else(|| {
                        SolidError::primitive_broken_link(OP, NO_EXTENDED_CUTTING)
                    })?;

                    // the piece before is this pass's own
                    let before_id = after_brep;
                    let (reference1, reference2) = if before_id == after_id {
                        (
                            before[0] + (before[1] * 10.0),
                            after[0] + (-after[1] * 10.0),
                        )
                    } else {
                        (
                            before[0] + (-before[1] * 30.0),
                            after[0] + (after[1] * 30.0),
                        )
                    };
                    if before_id == after_id {
                        a.stitch_faces(before_id, 0.01)?;
                        a.project_and_trim(
                            before_id,
                            curve,
                            path_normal,
                            reference2,
                            TrimType::Split,
                            tol,
                        )?;
                        let mut remove = Vec::new();
                        let brep = a.brep(before_id)?;
                        for edge in crate::topology::brep::p1::edges(brep, None)? {
                            if crate::topology::edge::is_lamina(brep, edge)? {
                                remove.extend(crate::topology::edge::faces(brep, edge, None)?);
                            }
                        }
                        a.remove_faces(before_id, &remove)?;
                        a.remove_topological_edges_and_vertices(before_id)?;
                    } else {
                        trim_both(
                            a,
                            [before_id, after_id],
                            curve,
                            path_normal,
                            [reference1, reference2],
                            tol,
                        )?;
                    }
                }
            }
        }

        // merge all results into one brep: the last first, then a union per piece
        let count = new_breps.len();
        let mut ordered = Vec::with_capacity(count);
        let mut tree: Vec<i64> = vec![0];
        ordered.extend(new_breps.pop());
        for index in (1_i64..).take(count.saturating_sub(1)) {
            ordered.extend(new_breps.pop());
            tree.push(index);
            // Convert to different form of Boolean numbering (0 - 1)
            tree.push(-1);
        }
        a.boolean_trees(&mut ordered, &tree)?;
        let [sweep] = *ordered.as_slice() else {
            // Something went wrong
            return Err(SolidError::primitive(
                OP,
                PrimitiveFailure::Defect {
                    why: NOT_ONE_RESULT,
                },
            ));
        };

        a.stitch_faces(sweep, tol)?;
        a.remove_topological_edges_and_vertices(sweep)?;
        // A second attempt can remove additional edges
        a.remove_topological_edges_and_vertices(sweep)?;

        // Solid intent with a manifold boundary gets the nested-solid parity
        // classification.
        let solid_intent = closed || (cap_start && cap_end);
        // Not upstream (WR-793): a closed profile meant as a solid must close, whatever
        // left it open. One cause is a corner too sharp for its mitre: at an inner corner
        // the pieces run `extend` past the corner and the cutting line reaches as far,
        // while the mitre of a profile `w` wide on either side of the path reaches
        // `w / sin(beta / 2)`, beta the angle between the legs.
        if solid_intent
            && profile_closes(a.kernel(sweep)?, profiles, tol)?
            && !crate::topology::brep::p2::is_closed_manifold(a.brep(sweep)?)?
        {
            return Err(SolidError::primitive(
                OP,
                PrimitiveFailure::SweepDoesNotClose,
            ));
        }
        let manifold = a.is_manifold_solid(sweep)?;
        if solid_intent && manifold {
            a.with_primitive_brep(sweep, |k, b| {
                crate::topology::brep::p1::set_region_is_void_flags_for_nested_solids(k, b)
            })?;
        }

        // Reject a manifold boundary whose regions are all void.
        let brep = a.brep(sweep)?;
        let mut has_material = false;
        for region in crate::topology::brep::p1::regions(brep, None)? {
            if brep.region(region)?.is_void() == 0 {
                has_material = true;
                break;
            }
        }
        if manifold && !has_material {
            return Err(SolidError::primitive(
                OP,
                PrimitiveFailure::SweepEnclosesNoMaterial,
            ));
        }
        Ok(sweep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::primitive::api_curves;

    fn segment(a: [f64; 3], b: [f64; 3]) -> Curve {
        let [ax, ay, az] = a;
        let [bx, by, bz] = b;
        Curve::from(
            api_curves::create_line_segment(Point3::new(ax, ay, az), Point3::new(bx, by, bz))
                .expect("segment"),
        )
    }

    /// An open profile wire asks for no caps (upstream always asks, and
    /// its cap fails); a closed one still does, and the open profile sweeps to a sheet.
    #[test]
    fn issue_837_open_profile_sweeps_without_caps() {
        let open = [
            segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            segment([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
        ];
        let closed = [
            segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            segment([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            segment([1.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let mut k = Kernel::default();
        assert!(!profile_closes(&k, &open, 1e-6).unwrap());
        assert!(profile_closes(&k, &closed, 1e-6).unwrap());

        let path = segment([0.0, 0.0, 0.0], [0.0, 0.0, 5.0]);
        let sheet = crate::sweep::api_brep::create_sweep_along_planar_path(
            &mut k,
            &[&open[0], &open[1]],
            &[&path],
            false,
            3,
        )
        .unwrap();
        assert_eq!(sheet.face_list().len(), 2);
        assert!(!crate::topology::brep::p2::is_manifold_solid(&sheet).unwrap());
    }

    /// Goldens `primitive.primitive_creation_p2` `gen0003`-style elbows (WR-780): a
    /// circle of radius `0.1 s` swept along two perpendicular legs of length `s`, capped,
    /// with tolerance `1e-5 s`. The mitre's reference points must lie farther than the
    /// tolerance from the cutting line. At a fixed 0.1 they did not (0.1 is the tolerance
    /// at `s = 1e4`), so the trims failed and the sweep was refused (main, ignoring the
    /// trims, returned an invalid body of zero volume). Each leg's tube is cut by the
    /// bisector plane through the corner, so the elbow's volume is the tube's along the
    /// centre line: `pi r^2 * 2 s`. Four faces: the two walls and the two caps, one
    /// manifold solid. Level 2 is not asserted: the sweep's tolerance `1e-5 s` is far above
    /// the zone tolerance the new pieces get (`PrimitiveCreationAccess::new_brep` makes
    /// them at the default), and checks #59 and #82 report edge and vertex offsets of
    /// the sweep's tolerance (0.1 at s = 1e4) as too wide for that B-rep tolerance: a
    /// mismatch of the inputs, not of the elbow's geometry.
    #[test]
    fn a_mitred_elbow_builds_at_any_size() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};
        use crate::primitive::primitive_creation::model::PrimitiveModel;

        for s in [1e4, 1e5, 1e6] {
            let r = 0.1 * s;
            let mut k = Kernel::default();
            let profile = Curve::from(
                api_curves::create_arc(Point3::new(0.0, 0.0, 0.0), r, 0.0, 360.0).unwrap(),
            );
            let paths = [
                segment([0.0, 0.0, 0.0], [s, 0.0, 0.0]),
                segment([s, 0.0, 0.0], [s, s, 0.0]),
            ];
            let (_, mut elbow) = PrimitiveModel::build_new(&mut k, |_, model| {
                let id = PrimitiveCreation::create_sweep_along_planar_path(
                    model,
                    PrimitiveModel::MAIN,
                    std::slice::from_ref(&profile),
                    &paths,
                    1e-5 * s,
                    true,
                    true,
                    true,
                )?;
                model.take_brep(id)
            })
            .unwrap_or_else(|e| panic!("s {s}: {e}"));
            elbow.refresh_cached_topology();
            assert_eq!(elbow.face_list().len(), 4, "s {s}");
            assert!(
                crate::topology::brep::p2::is_manifold_solid(&elbow).unwrap(),
                "s {s}"
            );
            let store = AttributeStore::default();
            let key_of = |_| ObjectKey(0);
            let attributes = PropertyAttributes {
                store: &store,
                key_of: &key_of,
            };
            let volume = brep_volume(&k, &mut elbow, 1e-8, &attributes).unwrap();
            let want = std::f64::consts::PI * r * r * 2.0 * s;
            // 1e-8 relative: the integrator's requested accuracy (a step-control
            // target, not a guaranteed bound; the bisector trims are built within the
            // sweep tolerance). MEASURED: 4.5e-11 at all three sizes.
            assert!(
                (volume - want).abs() <= 1e-8 * want,
                "s {s}: volume {volume}, want {want}"
            );
        }
    }

    /// Where the reach cap sets the mitre step (WR-780), a profile too small for the
    /// tolerance is refused, not built. A circle of radius `r` along two perpendicular
    /// legs with `tol = 1e-3`: the profile's `max_dimension` is `r`, so the step is
    /// `sqrt(2) r` and each reference point lies `sqrt(2) r * sin(45 deg) = r` from the
    /// cutting line. At `r = 0.6 tol` that is inside the tolerance and the side test
    /// refuses the sweep; at `r = 5 tol` (still capped, `sqrt(2) r` is below `100 tol`)
    /// the elbow builds: two walls and two caps.
    #[test]
    fn a_profile_below_the_tolerance_is_refused_at_a_mitre() {
        use crate::primitive::primitive_creation::model::PrimitiveModel;

        let tol = 1e-3;
        let sweep = |r: f64| {
            let mut k = Kernel::default();
            let profile = Curve::from(
                api_curves::create_arc(Point3::new(0.0, 0.0, 0.0), r, 0.0, 360.0).unwrap(),
            );
            let paths = [
                segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                segment([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            ];
            PrimitiveModel::build_new(&mut k, |_, model| {
                let id = PrimitiveCreation::create_sweep_along_planar_path(
                    model,
                    PrimitiveModel::MAIN,
                    std::slice::from_ref(&profile),
                    &paths,
                    tol,
                    true,
                    true,
                    true,
                )?;
                model.take_brep(id)
            })
            .map(|(_, b)| b)
        };

        let e = sweep(0.6 * tol).unwrap_err();
        assert_eq!(
            e.topology_failure(),
            Some(&crate::error::topology::TopologyFailure::SideUndecided {
                why: crate::error::topology::SideOpen::OnCurve
            }),
            "{e}"
        );

        let mut elbow = sweep(5.0 * tol).unwrap();
        elbow.refresh_cached_topology();
        assert_eq!(elbow.face_list().len(), 4);
        assert!(crate::topology::brep::p2::is_manifold_solid(&elbow).unwrap());
    }

    /// A path that turns straight back at its corner is refused by name (WR-793), as a
    /// configuration the mitre does not take, keeping `SM_ERR`.
    #[test]
    fn a_path_that_turns_back_is_refused_by_name() {
        use crate::primitive::primitive_creation::model::PrimitiveModel;

        let mut k = Kernel::default();
        let profile = Curve::from(
            api_curves::create_arc(Point3::new(0.0, 0.0, 0.0), 0.1, 0.0, 360.0).unwrap(),
        );
        let paths = [
            segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            segment([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let e = PrimitiveModel::build_new(&mut k, |_, model| {
            let id = PrimitiveCreation::create_sweep_along_planar_path(
                model,
                PrimitiveModel::MAIN,
                std::slice::from_ref(&profile),
                &paths,
                1e-5,
                true,
                true,
                true,
            )?;
            model.take_brep(id)
        })
        .map(|(_, b)| b)
        .unwrap_err();
        assert_eq!(
            e.primitive_failure(),
            Some(&PrimitiveFailure::PathTurnsBack),
            "{e}"
        );
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(e.kind(), crate::error::ErrorKind::Unsupported);
    }
}

#[cfg(test)]
mod audit_tests;
