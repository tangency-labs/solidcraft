//! Linear and rotational primitive sweep control.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation::create_repeated_linear_sweep`] | seven-argument `CreateLinearSweep` |
//! | [`PrimitiveCreation::create_rotational_sweep`] | full `CreateRotationalSweep` |
//! | [`PrimitiveCreation::create_partial_rotational_sweep`] | partial `CreateRotationalSweep` |
//! | [`split_curves_on_axis`] | fork-only `sm_SplitCurvesOnAxis` (WR-590) |
//!
//! Deviations: typed services and owned curve slots replace pointers; null curve
//! dereferences become errors. The existing sweep engine bounds repetitions to
//! 4096; the repeated linear sweep checks that bound before its end cap, which moves
//! the profile once per repetition. No additional recursion. Unavailable calls retain
//! their full caller tails.
//! Not upstream (#22184): the profile's split at its C1 discontinuities also cuts a
//! B-spline piece exactly where it turns too sharply at an end for the result's check
//! (`BSplineCurve::split_sharp_ends`), so each edge passes it with the same geometry.
//! Not upstream (WR-409, tangency-labs/solidcore#1113): a profile that bounds no planar
//! face becomes wires joined where its curves meet (`join_profile_wire_ends`).
//! Not upstream (WR-442, tangency-labs/solidcore#478): after a full turn's
//! stitch, a closed shell inside another becomes that solid's cavity.
//! Not upstream (WR-448, tangency-labs/solidcore#545; SolidCraft change,
//! tangency-labs/smlib 0b143f7): upstream makes analytic faces only for a one-curve
//! profile. Here a whole turn gives each curve its cylinder, cone, sphere or torus
//! ([`analytic_revolution`]) before falling back to the `SmSurfOfRevolution`, and a
//! partial turn's [`RotationalSweepGeometry`] does the same for each edge it sweeps.
//! There is no one-curve special case (`sm_CreateAnalyticRotationalSweep`), as fork
//! 9853e54 deletes it: a one-curve profile goes through the same per-curve rule, so a
//! whole circle's torus has its seam at the profile (#1491).
//! Not upstream (WR-590; SolidCraft change, tangency-labs/smlib e4f1377): a partial
//! turn first splits each
//! curve where it meets the axis inside itself ([`split_curves_on_axis`], the fork's
//! `sm_SplitCurvesOnAxis`), which `CheckCurveSweep` otherwise refuses
//! (`SM_SC_ROT_ON_CURVE`); a closed curve's last piece is joined back onto its first.
//! **Statuses pass on** (WR-662; SolidCraft change, tangency-labs/smlib c8f6a31): the
//! statuses upstream drops here are returned (`SER`).

use super::{construct::target, geometry::bspline_with_nurb, PrimitiveAccess};
use crate::curve::curve::Curve;
use crate::curve::curve::CurveKind;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::primitive::PrimitiveFailure;
use crate::error::{ignore_status, ignored, EntityRef, Failure, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::geom_utility::{
    p1::{line_closest_point, line_point_distance},
    p2::ContinuityType,
};
use crate::kernel::Kernel;
use crate::math::{max4, Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::crv_basic::CONIC_ANGLE_TOL_DEG;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::surface::surf_of_revolution::p1::SurfOfRevolution;
use crate::surface::surface::Surface;
use crate::sweep::analytic_revolution::analytic_revolution;
use crate::sweep::rotational_sweep_geometry::RotationalSweepGeometry;
use crate::sweep::topology_sweep::{
    engine::{check_sweep_repetitions, SweepTargets},
    geometry::SweepGeometry,
    SweepEntity, TopologySweep,
};
use crate::sweep::trans_sweep_geometry::TranslationalSweepGeometry;
use crate::topology::{BrepId, FaceId};
const OP: &str = "primitive::primitive_creation::p3::sweeps";
const OP_SPLIT_ON_AXIS: &str = "primitive::primitive_creation::p3::sweeps::split_curves_on_axis";

/// A profile curve that makes no wire edge ([`PrimitiveFailure::ProfileMakesNoWire`]),
/// keeping the wire's error as its cause; what `ignored` passes on (unported, cancelled,
/// a preparation the caller must do) passes on unchanged.
#[track_caller]
fn no_wire(e: SolidError) -> SolidError {
    if e.is_unported_or_cancelled() || matches!(e, SolidError::PreparationRequired { .. }) {
        e
    } else {
        Failure::new(OP, PrimitiveFailure::ProfileMakesNoWire)
            .caused_by(e)
            .into_error()
    }
}

/// Not upstream (#21672): a partial turn of `angle_deg` degrees whose size is below the
/// circle builder's [`CONIC_ANGLE_TOL_DEG`] is refused with
/// [`PrimitiveFailure::SweepAngleTooSmall`]. The builder makes such an arc a full turn,
/// so the sweep's rails and side faces would close up while its caps stay together. A
/// NaN angle passes on, to be refused where it is used.
fn refuse_vanishing_turn(angle_deg: f64) -> Result<()> {
    if angle_deg.abs() < CONIC_ANGLE_TOL_DEG {
        return Err(SolidError::primitive(
            OP,
            PrimitiveFailure::SweepAngleTooSmall {
                angle: angle_deg,
                minimum: CONIC_ANGLE_TOL_DEG,
            },
        ));
    }
    Ok(())
}

/// Flags shared by the repeated linear and partial rotational sweeps.
#[derive(Debug, Clone, Copy)]
pub struct RepeatedSweepOptions {
    /// Number of incremental sweeps, including zero as accepted upstream.
    pub repetitions: u64,
    /// Create end caps on the first and last profiles; capped sweeps require at least
    /// one repetition.
    pub cap_ends: bool,
    /// Split inputs at C1 discontinuities (default true).
    pub test_continuity: bool,
}
impl Default for RepeatedSweepOptions {
    fn default() -> Self {
        Self {
            repetitions: 1,
            cap_ends: false,
            test_continuity: true,
        }
    }
}

fn split(a: &impl PrimitiveAccess, b: BrepId, curves: &mut Vec<Option<Curve>>) -> Result<()> {
    let mut index = 0;
    while let Some(slot) = curves.get(index) {
        let curve = slot
            .as_ref()
            .ok_or_else(|| SolidError::primitive(OP, PrimitiveFailure::NoCurveGiven))?;
        let k = a.kernel(b)?;
        // `((SmBSplineCurve *)cr3DCurves[ii])->SubdivideAtDiscontinuity(..)`: the curve with
        // the NURB its own `GetOrCreateGwNurbPointer` makes (a line's lazy NURB), not a copy
        // of its bare base, which has none and refuses (`make_nurb`, 1001).
        let spline = matches!(curve.kind(), CurveKind::BSpline(_));
        let curve = bspline_with_nurb(k, curve)?
            .ok_or_else(|| SolidError::invalid_input(OP, "non B-spline continuity input"))?;
        // fork c8f6a31 (WR-662): SER(SubdivideAtDiscontinuity(..)).
        let mut pieces = curve.subdivide_at_discontinuity(k, ContinuityType::C1, 0.5)?;
        // Not upstream (#22184): a B-spline piece that turns too sharply at an end for the
        // result's check (test 14) is cut there exactly, so each edge passes it with the
        // profile's own geometry.
        if spline {
            let mut cut = Vec::new();
            for piece in pieces {
                match piece.split_sharp_ends(k)? {
                    Some(more) => cut.extend(more),
                    None => cut.push(piece),
                }
            }
            pieces = cut;
        }
        if pieces.len() > 1 {
            let count = pieces.len();
            curves.splice(
                index..=index,
                pieces.iter().cloned().map(|c| Some(Curve::from(c))),
            );
            index += count;
        } else {
            index += 1;
        }
    }
    Ok(())
}
/// `(SmBSplineCurve*)` of a split piece: the B-spline every curve kind the split
/// makes is built on.
fn piece_as_bspline(curve: Curve) -> Result<BSplineCurve> {
    match curve.into_kind() {
        CurveKind::BSpline(b) => Ok(b),
        CurveKind::Line(l) => Ok(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Ok(e.base().clone()),
        CurveKind::Hyperbola(h) => Ok(h.base().clone()),
        CurveKind::Parabola(p) => Ok(p.base().clone()),
        _ => Err(SolidError::invalid_input(
            OP_SPLIT_ON_AXIS,
            "split piece is not a B-spline curve",
        )),
    }
}

/// Not upstream (WR-590, the fork's `sm_SplitCurvesOnAxis`, tangency-labs/smlib
/// e4f1377): the curves of a profile to turn about the axis through `axis_base` along
/// `axis`, each split where it meets the axis inside itself (a circle tangent to the
/// axis: a horn torus). `RotationalSweepGeometry::check_curve_sweep` refuses such a
/// curve (`SM_SC_ROT_ON_CURVE`); this finds the points as that check does, with the
/// same tolerance, and splits there. A curve closed on itself, cut at its points, has
/// its last piece joined back onto its first, so it stays one curve that starts at
/// the first of them. Empty slots are dropped, as the fork's loop skips them.
///
/// # Errors
///
/// Those of [`Curve::intersect_with_line`], [`Curve::create_by_split_curve`] and
/// [`BSplineCurve::create_by_joining`].
pub fn split_curves_on_axis(
    k: &Kernel,
    curves: &mut Vec<Option<Curve>>,
    axis_base: Point3,
    axis: Vec3,
    tol: f64,
) -> Result<()> {
    let mut out = Vec::new();
    let axis_line = Line::infinite(axis_base, axis, 3, false);
    let axis_ivl = axis_line.natural_interval();
    for slot in curves.iter_mut() {
        let Some(mut curve) = slot.take() else {
            continue;
        };
        let ivl = curve.natural_interval();
        let hits = curve.intersect_with_line(k, &ivl, &axis_line, &axis_ivl, tol)?;
        let mut at = Vec::new();
        for sol in &hits.solutions {
            let param = sol.start.param(0)?;
            if !ivl.is_value_on_boundary(param, tol).0 {
                at.push(param);
            }
        }
        at.sort_by(f64::total_cmp);
        let Some((&first, rest)) = at.split_first() else {
            out.push(Some(curve));
            continue;
        };
        let closed = curve.is_closed(k, &ivl, tol)?;
        // the split leaves the first piece in `curve`; `last` runs on from each split
        let mut last = curve.create_by_split_curve(k, first)?;
        let head = curve;
        let mut middle = Vec::new();
        for &param in rest {
            let next = last.create_by_split_curve(k, param)?;
            middle.push(last);
            last = next;
        }
        if closed {
            // the last piece runs on to the curve's start: it and the first are one piece
            let mut join = [piece_as_bspline(last)?, piece_as_bspline(head)?];
            out.push(Some(Curve::from(BSplineCurve::create_by_joining(
                &mut join, None,
            )?)));
            out.extend(middle.into_iter().map(Some));
        } else {
            out.push(Some(head));
            out.extend(middle.into_iter().map(Some));
            out.push(Some(last));
        }
    }
    *curves = out;
    Ok(())
}

fn entity(b: BrepId, item: EntityRef) -> Result<SweepEntity> {
    match item {
        EntityRef::Region(i) => Ok(SweepEntity::Region(b, i)),
        EntityRef::Face(i) => Ok(SweepEntity::Face(b, i)),
        EntityRef::Edge(i) => Ok(SweepEntity::Edge(b, i)),
        EntityRef::Vertex(i) => Ok(SweepEntity::Vertex(b, i)),
        _ => Err(SolidError::invalid_input(
            OP,
            "non-sweep entity in primitive map",
        )),
    }
}
fn copy_maps(p: &PrimitiveCreation, b: BrepId, sweep: &mut TopologySweep) -> Result<()> {
    for (input, output) in [
        (&p.maps.to_same, &mut sweep.to_same),
        (&p.maps.to_higher, &mut sweep.to_higher),
        (&p.maps.from_same, &mut sweep.from_same),
        (&p.maps.from_lower, &mut sweep.from_lower),
    ] {
        output.clear();
        for (&from, &to) in input {
            output.insert(Some(entity(b, from)?), Some(entity(b, to)?));
        }
    }
    Ok(())
}
fn targets(
    b: BrepId,
    faces: &[FaceId],
    edges: &[crate::topology::EdgeId],
    cap: bool,
) -> SweepTargets {
    if cap {
        SweepTargets {
            faces_higher: Some(faces.iter().map(|&f| Some((b, f))).collect()),
            ..Default::default()
        }
    } else {
        SweepTargets {
            edges_higher: Some(edges.iter().map(|&e| Some((b, e))).collect()),
            ..Default::default()
        }
    }
}
/// Not upstream (WR-409, tangency-labs/solidcore#1113; the SolidCraft fork's
/// `sm_CreateJoinedWireEdges`): the profile's wire edges `edges`, in the curves' order,
/// joined where they meet. An end within the B-rep's tolerance of an earlier wire's end
/// is glued to that earlier vertex (`GlueVertices`), so two curves meeting at a point
/// share one vertex there. Without it the topology sweep's copies of two coincident
/// vertices came out coincident, each with a preimage, and its stitch refused them: a
/// partial revolve or repeated linear sweep of an open profile of more than one curve
/// failed.
///
/// # Errors
///
/// Those of the vertex queries and of [`crate::topology::brep::p5::glue_vertices`].
fn join_profile_wire_ends(
    a: &mut impl PrimitiveAccess,
    brep: BrepId,
    edges: &[crate::topology::EdgeId],
) -> Result<()> {
    use crate::topology::{edge, vertex, VertexId};
    a.with_primitive_brep(brep, |k, b| {
        let tol = b.tolerance();
        // the distinct vertices of the wires so far
        let mut vertices: Vec<VertexId> = Vec::new();
        for &e in edges {
            let start = edge::start_vertex(b, e)?;
            let end = edge::other_vertex(b, e, start)?;
            for (kk, v) in [start, end].into_iter().enumerate() {
                let Some(v) = v else {
                    continue;
                };
                if kk == 1 && Some(v) == start {
                    continue;
                }
                let point = vertex::point(b, v)?;
                let mut found = None;
                for &w in &vertices {
                    if vertex::point(b, w)?.distance(point) <= tol {
                        found = Some(w);
                        break;
                    }
                }
                match found {
                    Some(w) => {
                        crate::topology::brep::p5::glue_vertices(k, b, Some(w), Some(v), None)?;
                    }
                    None => vertices.push(v),
                }
            }
        }
        Ok(())
    })
}

impl PrimitiveCreation {
    /// Repeated linear sweep, with the active upstream analytic fast path.
    pub fn create_repeated_linear_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        curves: &mut Vec<Option<Curve>>,
        direction: Vec3,
        distance: f64,
        options: RepeatedSweepOptions,
    ) -> Result<()> {
        self.init_entity_maps();
        if options.cap_ends && options.repetitions == 0 {
            return Err(SolidError::primitive(
                OP,
                PrimitiveFailure::CappedSweepNoRepetition,
            ));
        }
        let region = target(self, brep)?;
        let tolerance = a.brep(brep)?.tolerance();
        a.set_editing(brep, true)?;
        let previous = a.kernel(brep)?.context.doing_boolean();
        a.kernel_mut(brep)?.context.set_doing_boolean(true);
        let result = (|| {
            if options.test_continuity {
                split(a, brep, curves)?;
            }
            if options.repetitions == 1 {
                // Active USE_NEW_ANALYTICS branch, including distance==0.
                let owned: Vec<_> = curves.iter_mut().filter_map(Option::take).collect();
                return a.with_primitive_brep(brep, |k, b| {
                    self.create_linear_sweep(k, b, owned, direction, distance, options.cap_ends)
                });
            }
            let mut faces = Vec::new();
            let mut edges = Vec::new();
            // the profile the end cap is copied from (the planar faces consume the curves)
            let profile: Vec<Curve> = if options.cap_ends {
                curves.iter().flatten().cloned().collect()
            } else {
                Vec::new()
            };
            if let Some(()) = ignored(a.planar_faces(region, curves, tolerance, &mut faces))? {
                for &face in &faces {
                    edges.extend(a.face_edges((brep, face))?);
                }
            } else {
                for slot in curves.iter_mut() {
                    if slot.is_none() {
                        return Err(SolidError::primitive(
                            OP,
                            PrimitiveFailure::ProfileSlotEmpty,
                        ));
                    }
                    let edge = match a.wire(brep, slot) {
                        Ok(edge) => edge,
                        Err(e) => return Err(no_wire(e)),
                    };
                    edges.push(edge);
                }
                join_profile_wire_ends(a, brep, &edges)?;
                faces.clear();
            }
            if distance == 0.0 {
                return Ok(());
            }
            // without planar faces there is nothing to cap
            let cap = options.cap_ends && !faces.is_empty();

            // Each step is distance along the unit direction; a zero vector is rejected.
            let sweep_dir = direction.unitize()?;

            let mut translation = TranslationalSweepGeometry::new(distance * sweep_dir);
            translation.base.eps = tolerance;
            let step = translation.sweep_transform();
            let geometry = SweepGeometry::Translation(translation);
            let mut sweep = TopologySweep {
                do_tagging: self.tag_id() > 0,
                tag_id: self.tag_id(),
                repetitions: options.repetitions,
                ..Default::default()
            };

            // Not upstream: the end cap below moves the profile once per repetition, so
            // the sweep's repetition cap is checked before it, not only in do_sweep
            // (u64::MAX repetitions ran without end, #2490 fuzz)
            check_sweep_repetitions(options.repetitions)?;

            // when asked and it's possible - build the end cap: the profile moved by
            // every step
            if cap {
                let mut end_curves = Vec::new();
                for curve in &profile {
                    let mut end_curve = curve.copy(a.kernel(brep)?)?;
                    for _ in 0..options.repetitions {
                        // fork c8f6a31 (WR-662): SER_DELETE(pEndCurve->Transform(sStep)).
                        let k = a.kernel(brep)?;
                        end_curve.transform(k, &step, None)?;
                    }
                    end_curves.push(Some(end_curve));
                }
                let mut end_faces = Vec::new();
                a.planar_faces(region, &mut end_curves, tolerance, &mut end_faces)?;
                sweep.do_stitching = true; // stitch the sides to the caps
            }

            let mut selected = targets(brep, &faces, &edges, false);
            sweep.do_sweep(a, &geometry, Some(brep), Some(region), &mut selected)?;

            if cap {
                // both caps have the profile's orientation
                a.orient(brep, true, false)?;
            } else {
                // unrelated clean up - we didn't use the tmp faces (if any - delete them)
                // fork c8f6a31 (WR-662): SER(pBrep->DeleteFace(..)).
                for face in faces {
                    a.delete_primitive_face((brep, face))?;
                }
            }
            copy_maps(self, brep, &mut sweep)?;
            a.set_editing(brep, false)?;
            Ok(())
        })();
        a.kernel_mut(brep)?.context.set_doing_boolean(previous);
        result
    }

    /// Full rotation: prefer analytic surfaces, then circular planes or revolutions.
    /// Failed planar face construction still consumes that source curve upstream.
    pub fn create_rotational_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        curves: &mut [Option<Curve>],
        axis_base: Point3,
        axis: Vec3,
    ) -> Result<()> {
        self.init_entity_maps();
        let _ = target(self, brep)?;
        a.set_editing(brep, true)?;
        // Not upstream (#1491): fork 9853e54 deletes the one-curve special case
        // (sm_CreateAnalyticRotationalSweep), whose whole-circle torus puts its seam a
        // quarter turn from the profile; the per-curve rule below covers a one-curve
        // profile too
        for slot in curves.iter_mut() {
            let curve = slot
                .as_ref()
                .ok_or_else(|| SolidError::primitive(OP, PrimitiveFailure::NoCurveGiven))?;
            // fork c8f6a31 (WR-662): SER(Length(..)), SER(SetCanonical(..)).
            let k = a.kernel(brep)?;
            let length = curve.length(k, &curve.natural_interval(), 0.001)?;
            if length < EFF_ZERO {
                return Err(SolidError::invalid_input(OP, "degenerate revolved curve"));
            }
            let ends = curve.ends(a.kernel(brep)?)?;
            let (start_dist, _) = line_point_distance(axis_base, axis, ends.start);
            let (end_dist, _) = line_point_distance(axis_base, axis, ends.end);
            let length_scale = max4(start_dist, end_dist, length, EFF_ZERO_SQRT);
            let angle_tol = a.brep(brep)?.tolerance() / length_scale * (180.0 / PI);
            let mut face = None;
            if curve.is_linear(a.kernel(brep)?, EFF_ZERO, false)?.linear {
                let mut tangent = ends.end - ends.start;
                tangent /= tangent.length();
                if tangent.is_perpendicular_to(axis, angle_tol) {
                    let zero = crate::geom::tol::scaled_zero2(&ends.start, &ends.end);
                    let param = line_closest_point(axis_base, axis, ends.start);
                    let origin = axis_base + param * axis;
                    if start_dist > end_dist {
                        tangent *= -1.0;
                    }
                    let mut frame = Axis2Placement::IDENTITY;
                    frame.set_canonical(origin, tangent, axis.cross(tangent))?;
                    let mut circles = Vec::new();
                    if start_dist > zero {
                        circles.push(Curve::from(Ellipse::create_canonical_circle(
                            a.kernel(brep)?,
                            &frame,
                            start_dist,
                            None,
                            None,
                        )?));
                    }
                    if end_dist > zero {
                        circles.push(Curve::from(Ellipse::create_canonical_circle(
                            a.kernel(brep)?,
                            &frame,
                            end_dist,
                            None,
                            None,
                        )?));
                    }
                    let infinite = a
                        .brep(brep)?
                        .infinite_region()
                        .ok_or_else(|| SolidError::primitive(OP, PrimitiveFailure::NoRegion))?;
                    let tolerance = a.brep(brep)?.tolerance();
                    ignore_status(a.planar_face((brep, infinite), circles, tolerance, &mut face))?;
                    *slot = None;
                }
            }
            if face.is_none() {
                if let Some(curve) = slot.as_ref() {
                    // Not upstream (WR-448, #545): the curve's cylinder, cone,
                    // sphere or torus, else the surface of revolution
                    let tolerance = a.brep(brep)?.tolerance();
                    let analytic = analytic_revolution(
                        a.kernel(brep)?,
                        curve,
                        axis_base,
                        axis,
                        360.0,
                        tolerance,
                        None,
                    )?;
                    // Not upstream (#17204): the frame comes from a point of the curve
                    // off the axis by more than the B-rep's tolerance (`tolerance`),
                    // within which a profile vertex is on the axis
                    let rev = if let Some(surface) = analytic {
                        Some(surface)
                    } else {
                        // Not upstream (#17204): a B-spline end within the B-rep's
                        // tolerance of the axis is on it for the body, so the surface
                        // of revolution turns the curve with that end's control point
                        // on the axis. A curve the analytic rule above recognised (a
                        // circular arc's sphere) keeps its own ends: its surface puts
                        // its poles on the axis itself
                        let profile = spline_ends_on_axis(
                            a.kernel(brep)?,
                            curve,
                            axis_base,
                            axis,
                            tolerance,
                        )?
                        .unwrap_or_else(|| curve.clone());
                        ignored(SurfOfRevolution::create_canonical_off_axis(
                            a.kernel(brep)?,
                            profile,
                            axis_base,
                            axis,
                            false,
                            tolerance,
                        ))?
                        .map(Surface::from)
                    };
                    let mut success = false;
                    if let Some(surface) = rev {
                        let domain = surface.natural_uv_domain_with_nurb(a.kernel(brep)?)?;
                        success =
                            ignored(a.surface_face(brep, surface, domain, &mut face))?.is_some();
                    }
                    if face.is_none() || !success {
                        *slot = None;
                    }
                }
            }
        }
        // fork c8f6a31 (WR-662): SER(pBrep->StitchAndOrient()).
        a.stitch_and_orient(brep)?;
        // not upstream (WR-442, #478): a closed shell inside another is its cavity
        a.with_brep_mut(brep, |k, b, _, env| {
            crate::topology::brep::p6::nest_closed_shells_as_cavities(k, b, env)
        })?;
        a.set_editing(brep, false)?;
        Ok(())
    }

    /// Partial rotation and its full-revolution dispatch, axis-interior rejection,
    /// sweep, ordered face deletion and callback stitching.
    ///
    /// # Errors
    ///
    /// [`PrimitiveFailure::SweepAngleTooSmall`] for a turn below `CONIC_ANGLE_TOL_DEG`
    /// in size (not upstream, #21672); those of the profile, the sweep and the stitch.
    pub fn create_partial_rotational_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        curves: &mut Vec<Option<Curve>>,
        axis_base: Point3,
        axis: Vec3,
        angle_deg: f64,
        options: RepeatedSweepOptions,
    ) -> Result<()> {
        self.partial_rotational_sweep(a, brep, curves, axis_base, axis, angle_deg, options, None)
    }

    /// [`Self::create_partial_rotational_sweep`] with the path arc's radius as the
    /// omitted-corner scale. Only a closed path of lines and arcs calls this.
    pub(crate) fn create_partial_rotational_sweep_at_scale(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        curves: &mut Vec<Option<Curve>>,
        axis_base: Point3,
        axis: Vec3,
        angle_deg: f64,
        options: RepeatedSweepOptions,
        feature_scale: f64,
    ) -> Result<()> {
        self.partial_rotational_sweep(
            a,
            brep,
            curves,
            axis_base,
            axis,
            angle_deg,
            options,
            Some(feature_scale),
        )
    }

    fn partial_rotational_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        curves: &mut Vec<Option<Curve>>,
        axis_base: Point3,
        axis: Vec3,
        angle_deg: f64,
        options: RepeatedSweepOptions,
        corner_scale: Option<f64>,
    ) -> Result<()> {
        self.init_entity_maps();
        let region = target(self, brep)?;
        let previous = a.kernel(brep)?.context.doing_boolean();
        a.kernel_mut(brep)?.context.set_doing_boolean(true);
        let result = (|| {
            // Not upstream (#21672): a turn the circle builder cannot draw is refused. Below
            // `CONIC_ANGLE_TOL_DEG` every rail and side surface comes out a full turn
            // (`conic_data`) while the end cap is the start cap moved by the exact, near-zero
            // rotation: a full-turn ring with both caps on its seam, for a request of no volume
            refuse_vanishing_turn(angle_deg)?;
            if options.test_continuity {
                split(a, brep, curves)?;
            }
            if angle_deg >= 359.99999 {
                return self.create_rotational_sweep(a, brep, curves, axis_base, axis);
            }
            if options.cap_ends && options.repetitions != 1 {
                return Err(SolidError::primitive(
                    OP,
                    PrimitiveFailure::CappedRevolutionRepeats {
                        repetitions: options.repetitions,
                    },
                ));
            }
            let tolerance = a.brep(brep)?.tolerance();
            // Not upstream (WR-590): no curve meets the axis inside itself
            // (CheckCurveSweep's SM_SC_ROT_ON_CURVE)
            split_curves_on_axis(a.kernel(brep)?, curves, axis_base, axis, tolerance)?;
            a.set_editing(brep, true)?;
            let mut faces = Vec::new();
            let mut edges = Vec::new();
            if ignored(a.planar_faces(region, curves, tolerance, &mut faces))?.is_none() {
                for slot in curves.iter_mut() {
                    let edge = match a.wire(brep, slot) {
                        Ok(edge) => edge,
                        Err(e) => return Err(no_wire(e)),
                    };
                    edges.push(edge);
                }
                join_profile_wire_ends(a, brep, &edges)?;
                faces.clear();
            }
            for &face in &faces {
                edges.extend(a.face_edges((brep, face))?);
            }
            let mut cap = options.cap_ends;
            if faces.is_empty() {
                cap = false;
            }
            // Not upstream (#22035 review): every cap face is checked, not only the first;
            // a profile of several loops in one plane gives several faces, each capped
            for &face in &faces {
                let bound = a.primitive_face_box((brep, face))?.sphere_bound();
                let direction = axis.unitize()?;
                let param = line_closest_point(axis_base, direction, bound.center);
                let point = axis_base + param * direction;
                let start = point - 2.0 * bound.radius * direction;
                let end = point + 2.0 * bound.radius * direction;
                let axis_curve = Curve::from(BSplineCurve::create_line_segment(3, start, end)?);
                if a.axis_inside_face((brep, face), &axis_curve)? {
                    a.delete_primitive_faces(brep, &faces)?;
                    return Err(SolidError::primitive(OP, PrimitiveFailure::AxisInsideFace));
                }
                // Not upstream (#21671): the check above sees only an axis lying in the
                // face. An axis crossing the face's plane at a point inside the face leaves
                // that point fixed, so both caps contain it, and two caps through a common
                // inside point cross (or overlap, when the axis is the plane's normal)
                if cap && axis_crosses_face_inside(a, (brep, face), axis_base, axis)? {
                    a.delete_primitive_faces(brep, &faces)?;
                    return Err(SolidError::primitive(OP, PrimitiveFailure::AxisInsideFace));
                }
                // Not upstream (#22036): an axis square to the face's plane that meets the
                // plane outside the face passes both checks above, but the turn keeps the
                // face in its own plane: both caps lie in that plane and the body built
                // between them encloses no volume
                if cap && turn_keeps_face_in_plane(a, (brep, face), axis_base, axis)? {
                    a.delete_primitive_faces(brep, &faces)?;
                    return Err(SolidError::primitive(
                        OP,
                        PrimitiveFailure::AxisNormalToProfile,
                    ));
                }
            }
            // Not upstream (WR-448, #545): analytic side faces (with `make_analytics`, the default)
            let mut rotation = RotationalSweepGeometry::new(axis_base, axis, angle_deg, tolerance);
            // A closed line-and-arc path: the band is the arc's own zone. Unset, on-axis
            // tests stay on `tolerance`.
            if let Some(scale) = corner_scale {
                rotation.set_omitted_corner_scale(scale);
            }
            let geometry = SweepGeometry::Rotation(rotation);
            let mut sweep = TopologySweep {
                repetitions: options.repetitions,
                ..Default::default()
            };
            if self.tag_id() > 0 {
                sweep.do_tagging = true;
                sweep.tag_id = self.tag_id();
            }
            let mut selected = targets(brep, &faces, &edges, cap);
            if cap {
                sweep.do_sweep(a, &geometry, Some(brep), Some(region), &mut selected)?;
            } else {
                sweep.do_stitching = false;
                sweep.do_sweep(a, &geometry, Some(brep), Some(region), &mut selected)?;
                // fork c8f6a31 (WR-662): SER(sTI.DeleteFaces(aFaces)).
                a.delete_primitive_faces(brep, &faces)?;
                a.stitch(brep, &mut sweep, tolerance, false)?;
            }
            copy_maps(self, brep, &mut sweep)?;
            a.set_editing(brep, false)?;
            Ok(())
        })();
        a.kernel_mut(brep)?.context.set_doing_boolean(previous);
        result
    }
}

/// Not upstream (#21671): whether the axis through `axis_base` along `axis` crosses the
/// plane of the planar profile `face` at a point strictly inside the face, off its
/// boundary by more than the face's tolerance.
///
/// The rotation fixes every point of its axis, so that point lies on the start cap and on
/// the end cap of a capped partial revolution. The two caps' planes both pass through it:
/// they cross along a line through it (or coincide, when the axis is the plane's normal),
/// and since the point is inside both caps they cross there along a segment that is no
/// edge of the body, or overlap. The point is where the axis line meets the face's plane,
/// classified against the face's own loops (`point_3d_classify_at`); an axis parallel to
/// the plane meets it nowhere, or lies in it, which `axis_inside_face` decides. A face
/// that is not planar within its tolerance is not decided here (`false`).
///
/// # Errors
///
/// Those of the face's surface and of the point classification.
fn axis_crosses_face_inside(
    a: &mut impl PrimitiveAccess,
    face: crate::sweep::topology_sweep::access::FaceRef,
    axis_base: Point3,
    axis: Vec3,
) -> Result<bool> {
    use crate::intersect::point_class::{PointClassType, PointClassification};
    use crate::query::face::p1::{point_3d_classify_at, Point3dClassifyOptions};

    a.with_brep_mut(face.0, |k, b, _, _| {
        let tolerance = b.face(face.1)?.tolerance;
        let Some(surface) = crate::topology::face::p2::surface(b, face.1)? else {
            return Ok(false);
        };
        let plane = surface.is_planar(k, tolerance)?;
        if !plane.is_planar {
            return Ok(false);
        }
        let along = plane.normal.dot(axis);
        if along == 0.0 || !along.is_finite() {
            return Ok(false);
        }
        let t = plane.normal.dot(plane.point - axis_base) / along;
        let point = axis_base + t * axis;
        if ![point.x, point.y, point.z].iter().all(|c| c.is_finite()) {
            return Ok(false);
        }
        let mut pc = PointClassification::new(crate::geom::tol::zone_tol3d(k, tolerance));
        point_3d_classify_at(
            k,
            b,
            face.1,
            point,
            tolerance,
            true,
            &mut pc,
            Point3dClassifyOptions::default(),
        )?;
        Ok(pc.point_class() == PointClassType::Face)
    })
}

/// Not upstream (#22036): whether every turn about the axis through `axis_base` along
/// `axis` keeps every point of the planar profile `face` within the face's tolerance of
/// the face's plane, so that a capped partial revolution of it is flat: both caps lie in
/// that plane and the body between them encloses no volume.
///
/// Take `n` the plane's unit normal, `a` the unit axis and `P` the point where the axis
/// meets the plane. By Rodrigues' formula a turn by `phi` about the axis moves `P + v`,
/// for `v` in the plane, by `(cos phi - 1) v + sin phi (a x v) + (1 - cos phi) a (a . v)`,
/// whose component along `n` is `sin phi v . (n x a) + (1 - cos phi)(a . n)(a . v)`. As
/// `v` is square to `n`, `a . v = a_t . v` with `a_t = a - (a . n) n`; `n x a` and `a_t`
/// both lie in the plane, are square to each other and have length `s = |n x a|`, so
/// `(v . (n x a))^2 + (a_t . v)^2 = s^2 |v|^2`. With `|a . n| <= 1` the lift is at most
/// `sqrt(sin^2 phi + (1 - cos phi)^2) s |v| = 2 |sin(phi / 2)| s |v| <= 2 s |v|`. The
/// check uses `(1 + sqrt 2) s R`, with `R` bounding `|v|` over the face (the distance
/// from `P` to the face's bounding sphere, plus its radius): the bound of the two terms
/// taken one at a time. It exceeds the exact `2 s R` by the factor `(1 + sqrt 2) / 2`,
/// about 1.21, and that 21 % margin holds the floating-point rounding of `P`, `R` and the
/// product, a few units in the last place of numbers no larger than the coordinates.
/// A point of the face off the plane by up to the planarity deviation `d` moves rigidly
/// with its foot in the plane, so it stays within `d` of where the foot goes. The face is
/// flat for the whole turn when `d + (1 + sqrt 2) s R` is within the tolerance. The bound
/// is an upper one: an axis tilted from the normal far enough to sweep a solid thicker
/// than the tolerance, however thin, is never decided flat. An axis close
/// to parallel to the plane meets it far away or nowhere, and is not flat (`false`); a face
/// that is not planar within its tolerance is not decided here (`false`).
///
/// # Errors
///
/// Those of the face's box and surface.
fn turn_keeps_face_in_plane(
    a: &mut impl PrimitiveAccess,
    face: crate::sweep::topology_sweep::access::FaceRef,
    axis_base: Point3,
    axis: Vec3,
) -> Result<bool> {
    let bound = a.primitive_face_box(face)?.sphere_bound();
    let Ok(direction) = axis.unitize() else {
        return Ok(false);
    };
    a.with_brep_mut(face.0, |k, b, _, _| {
        let tolerance = b.face(face.1)?.tolerance;
        let Some(surface) = crate::topology::face::p2::surface(b, face.1)? else {
            return Ok(false);
        };
        let plane = surface.is_planar(k, tolerance)?;
        if !plane.is_planar {
            return Ok(false);
        }
        let Ok(normal) = plane.normal.unitize() else {
            return Ok(false);
        };
        let along = normal.dot(direction);
        if along == 0.0 || !along.is_finite() {
            return Ok(false);
        }
        let t = normal.dot(plane.point - axis_base) / along;
        let pierce = axis_base + t * direction;
        let reach = pierce.distance(bound.center) + bound.radius;
        let lift = plane.deviation.abs()
            + (1.0 + std::f64::consts::SQRT_2) * normal.cross(direction).length() * reach;
        Ok(lift.is_finite() && lift <= tolerance)
    })
}

/// Not upstream (#17204): `curve`, a B-spline, with each end that lies within `tolerance`
/// of the axis through `axis_base` along `axis` but off it put exactly on it (the end's
/// control point, which is the end of a curve clamped there, moved to its foot on the axis,
/// its weight kept). Within the B-rep's tolerance the end is on the axis for the body; left
/// a hair off it (a sketch solver's 3e-11 mm), its surface of revolution's last row of
/// control points is a circle of that radius, no pole, and the face's degenerate edge there
/// is a gap in its loop. `None` for another curve, or when no end moves.
///
/// # Errors
///
/// Those of the curve's ends and control points.
fn spline_ends_on_axis(
    k: &Kernel,
    curve: &Curve,
    axis_base: Point3,
    axis: Vec3,
    tolerance: f64,
) -> Result<Option<Curve>> {
    use crate::nurbs::b_spline_curve::p2::ControlPointForm;
    let CurveKind::BSpline(spline) = curve.kind() else {
        return Ok(None);
    };
    let ends = curve.ends(k)?;
    let count = spline.nurb_ref()?.cpts().len();
    let rational = spline
        .nurb_ref()?
        .cpts()
        .first()
        .is_some_and(|c| c.w != crate::nurbs::math::NOW);
    let form = if rational {
        ControlPointForm::EuclideanRational
    } else {
        ControlPointForm::NonRational
    };
    let mut moved: Option<BSplineCurve> = None;
    for (end, index) in [(ends.start, Some(0)), (ends.end, count.checked_sub(1))] {
        let Some(index) = index else {
            continue;
        };
        let (dist, _) = line_point_distance(axis_base, axis, end);
        if !(dist > 0.0 && dist <= tolerance) {
            continue;
        }
        let (point, weight) = spline.control_point(form, index)?;
        // the end is that control point only where the curve is clamped
        if point.distance(end) > EFF_ZERO * (1.0 + end.max_dimension()) {
            continue;
        }
        let foot = axis_base + line_closest_point(axis_base, axis, end) * axis;
        let target = moved.get_or_insert_with(|| spline.clone());
        target.set_control_point(form, index, foot, weight)?;
    }
    Ok(moved.map(Curve::from))
}

#[cfg(test)]
mod tests {
    use crate::curve::curve::Curve;
    use crate::kernel::Kernel;
    use crate::math::det;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_curves;
    use crate::sweep::api_brep::create_rotational_sweep;

    fn segment(a: [f64; 3], b: [f64; 3]) -> Curve {
        let [ax, ay, az] = a;
        let [bx, by, bz] = b;
        Curve::from(
            api_curves::create_line_segment(Point3::new(ax, ay, az), Point3::new(bx, by, bz))
                .expect("segment"),
        )
    }

    /// #21671 (goldens `primitive.primitive_creation_p3`
    /// `capped_rot_square_axis_tilted_through_face_{90,270}`): a capped partial revolve
    /// whose axis crosses the profile's plane inside the profile is refused with
    /// `AxisInsideFace`. The square [2,4] x [0,2] at z 0 about the axis (1, 0, 1)
    /// through its centre (3, 1, 0): the rotation fixes the centre, so both caps contain
    /// it; after 90 degrees the end cap's normal is (1/2, -1/sqrt 2, 1/2), at 60 degrees
    /// to the start cap's, and the caps cross along a segment of length sqrt 6 through
    /// the centre that is no edge. Before, the 90 degree turn returned that body. The
    /// axis (0, 0, 1) through the centre turns the square in its own plane: the caps
    /// overlap, refused the same way.
    /// Controls that still build, by Pappus (an axis in the profile's plane, a quarter
    /// turn: volume = area 4 * centroid distance * pi / 2): the y axis outside the square
    /// (centroid 3 from it, 6 pi), and the y axis along the edge x 0 of [0,2]^2
    /// (centroid 1 from it, 2 pi).
    #[test]
    fn issue_21671_an_axis_through_the_capped_profile_is_refused() {
        use crate::error::primitive::PrimitiveFailure;
        use crate::mass::integrator::properties::brep_volume_default;

        let square = |x0: f64| {
            [
                segment([x0, 0.0, 0.0], [x0 + 2.0, 0.0, 0.0]),
                segment([x0 + 2.0, 0.0, 0.0], [x0 + 2.0, 2.0, 0.0]),
                segment([x0 + 2.0, 2.0, 0.0], [x0, 2.0, 0.0]),
                segment([x0, 2.0, 0.0], [x0, 0.0, 0.0]),
            ]
        };
        let off = square(2.0);
        let curves: Vec<Option<&Curve>> = off.iter().map(Some).collect();
        let centre = Point3::new(3.0, 1.0, 0.0);
        let mut k = Kernel::default();
        for (axis, angle) in [
            (Vec3::new(1.0, 0.0, 1.0), 90.0),
            (Vec3::new(1.0, 0.0, 1.0), 270.0),
            (Vec3::new(-1.0, 0.0, 1.0), 45.0),
            (Vec3::new(0.0, 0.0, 1.0), 90.0),
        ] {
            let got = create_rotational_sweep(&mut k, &curves, centre, axis, angle, true);
            assert_eq!(
                got.as_ref()
                    .err()
                    .and_then(|e| e.primitive_failure())
                    .copied(),
                Some(PrimitiveFailure::AxisInsideFace),
                "axis {axis:?} through the centre, {angle} degrees: {:?}",
                got.map(|_| ())
            );
        }

        // #22035 review: a profile of two loops in one plane gives two cap faces; the axis
        // through the second square's centre (7, 1, 0) is refused as well
        let two: Vec<Curve> = square(2.0).into_iter().chain(square(6.0)).collect();
        let two: Vec<Option<&Curve>> = two.iter().map(Some).collect();
        let got = create_rotational_sweep(
            &mut k,
            &two,
            Point3::new(7.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 1.0),
            90.0,
            true,
        );
        assert_eq!(
            got.as_ref()
                .err()
                .and_then(|e| e.primitive_failure())
                .copied(),
            Some(PrimitiveFailure::AxisInsideFace),
            "the axis through the second square: {:?}",
            got.map(|_| ())
        );

        let pi = std::f64::consts::PI;
        let on_edge = square(0.0);
        let on_edge: Vec<Option<&Curve>> = on_edge.iter().map(Some).collect();
        let (o, y) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        for (curves, want, what) in [
            (&curves, 6.0 * pi, "outside"),
            (&on_edge, 2.0 * pi, "on edge"),
        ] {
            let mut body = create_rotational_sweep(&mut k, curves, o, y, 90.0, true)
                .unwrap_or_else(|e| panic!("y axis {what}: {e}"));
            assert!(
                crate::topology::brep::p2::is_manifold_solid(&body).unwrap(),
                "{what}"
            );
            // signed: an inward-facing solid fails too
            let volume = brep_volume_default(&k, &mut body, 1e-8).unwrap();
            assert!(
                (volume - want).abs() < 1e-6 * want,
                "y axis {what}: volume {volume}, want {want}"
            );
        }
    }

    /// #22036 (goldens `primitive.primitive_creation_p3`
    /// `capped_rot_square_axis_z_outside_{90,270}`): a capped partial revolve whose axis is
    /// square to the profile's plane and meets it outside the profile turns the profile in
    /// its own plane, and is refused with `AxisNormalToProfile`. The square [2,4] x [0,2]
    /// at z 0 about the z axis: every point keeps z 0, so both caps lie in z 0 and the body
    /// between them has no volume. Before, the 90 degree turn returned that body. The same
    /// is refused about the reversed axis through a base off the plane, (0, 0, 5), and about
    /// an axis tilted 1e-12 rad from the normal, which lifts no point by more than
    /// (1 + sqrt 2) * 1e-12 * (sqrt 10 + sqrt 2), about 1.1e-11, far within the tolerance.
    /// Control that still builds: the axis (sin e, 0, cos e) through the origin, tilted
    /// e = 0.4 degrees (inside the kernel's 0.5 degree parallel tolerance, so the check is
    /// not an angle test). A rigid turn by theta about the unit axis a through P sweeps
    /// the planar region F of unit normal n through the volume theta * area(F) *
    /// (c - P) . (n x a), c its centroid (the flux of the velocity a x (p - P) through F,
    /// the same at every instant as the turn moves F and n together; Pappus' theorem when a
    /// lies in the plane). Here n x a = (0, sin e, 0) and c - P = (3, 1, 0), and the flux
    /// y sin e is nowhere negative on F, so the swept body is the turned region once over:
    /// a quarter turn gives pi / 2 * 4 * sin e = 2 pi sin e, about 0.0439.
    #[test]
    fn issue_22036_an_axis_square_to_the_capped_profile_outside_it_is_refused() {
        use crate::error::primitive::PrimitiveFailure;
        use crate::mass::integrator::properties::brep_volume_default;

        let square = [
            segment([2.0, 0.0, 0.0], [4.0, 0.0, 0.0]),
            segment([4.0, 0.0, 0.0], [4.0, 2.0, 0.0]),
            segment([4.0, 2.0, 0.0], [2.0, 2.0, 0.0]),
            segment([2.0, 2.0, 0.0], [2.0, 0.0, 0.0]),
        ];
        let curves: Vec<Option<&Curve>> = square.iter().map(Some).collect();
        let origin = Point3::new(0.0, 0.0, 0.0);
        let mut k = Kernel::default();
        for (base, axis, angle) in [
            (origin, Vec3::new(0.0, 0.0, 1.0), 90.0),
            (origin, Vec3::new(0.0, 0.0, 1.0), 270.0),
            (Point3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0), 90.0),
            (origin, Vec3::new(1e-12, 0.0, 1.0), 90.0),
        ] {
            let got = create_rotational_sweep(&mut k, &curves, base, axis, angle, true);
            assert_eq!(
                got.as_ref()
                    .err()
                    .and_then(|e| e.primitive_failure())
                    .copied(),
                Some(PrimitiveFailure::AxisNormalToProfile),
                "axis {axis:?} through {base:?}, {angle} degrees: {:?}",
                got.map(|_| ())
            );
        }

        let e = 0.4_f64.to_radians();
        let tilted = Vec3::new(det::sin(e), 0.0, det::cos(e));
        let mut body = create_rotational_sweep(&mut k, &curves, origin, tilted, 90.0, true)
            .unwrap_or_else(|e| panic!("axis tilted 0.4 degrees: {e}"));
        assert!(crate::topology::brep::p2::is_manifold_solid(&body).unwrap());
        // signed: an inward-facing solid fails too
        let want = 2.0 * std::f64::consts::PI * det::sin(e);
        let volume = brep_volume_default(&k, &mut body, 1e-8).unwrap();
        assert!(
            (volume - want).abs() < 1e-6 * want,
            "axis tilted 0.4 degrees: volume {volume}, want {want}"
        );
    }

    /// WR-776: a line skew to the axis sweeps a hyperboloid of one sheet. Every point of
    /// the result is as far from the axis as the profile's point at the same axial
    /// coordinate, both measured here from the profile's and the axis's own numbers.
    /// Upstream's one-curve special case made the cone through the end circles, 3.85 mm
    /// off its hyperboloid half way along this line; the analytic rule leaves the line
    /// its surface of revolution.
    #[test]
    fn a_line_skew_to_the_axis_revolves_onto_its_hyperboloid() {
        let (p0, p1) = ([600.0, 0.0, 0.0], [700.0, 0.0, 200.0]);
        let o = [500.0, -50.0, 0.0];
        let n = (40.0_f64 * 40.0 + 80.0 * 80.0 + 300.0 * 300.0).sqrt();
        let a = [40.0 / n, 80.0 / n, 300.0 / n];
        let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        // the axial coordinate of p and its distance from the axis
        let polar = |p: [f64; 3]| {
            let q = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            let h = dot(q, a);
            (h, (dot(q, q) - h * h).max(0.0).sqrt())
        };
        let ((h0, _), (h1, _)) = (polar(p0), polar(p1));
        let profile = segment(p0, p1);
        let mut k = Kernel::default();
        let sheet = create_rotational_sweep(
            &mut k,
            &[Some(&profile)],
            Point3::new(o[0], o[1], o[2]),
            Vec3::new(a[0], a[1], a[2]),
            360.0,
            false,
        )
        .unwrap();
        let faces = crate::topology::brep::p1::faces(&sheet, None).unwrap();
        assert!(!faces.is_empty());
        let mut worst = 0.0_f64;
        for f in faces {
            let surface = crate::topology::face::p2::surface(&sheet, f)
                .unwrap()
                .expect("a surface");
            let d = surface.natural_uv_domain_with_nurb(&k).unwrap();
            for i in 0..=24 {
                for j in 0..=24 {
                    let uv = crate::geom::vector2d::Point2::new(
                        d.u_min() + (d.u_max() - d.u_min()) * f64::from(i) / 24.0,
                        d.v_min() + (d.v_max() - d.v_min()) * f64::from(j) / 24.0,
                    );
                    let x = surface.evaluate_point(&k, uv).unwrap();
                    let (h, r) = polar([x.x, x.y, x.z]);
                    let t = (h - h0) / (h1 - h0);
                    assert!((-1e-9..=1.0 + 1e-9).contains(&t), "past the profile: {x:?}");
                    let on = [0, 1, 2].map(|c| p0[c] + t * (p1[c] - p0[c]));
                    worst = worst.max((r - polar(on).1).abs());
                }
            }
        }
        assert!(worst <= 1e-6, "{worst} mm off the hyperboloid");
    }

    /// A full turn of a half annulus about its diameter is a hollow ball; upstream
    /// makes the inner ball a second solid.
    #[test]
    fn issue_478_full_turn_nested_shell_is_a_cavity() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};

        let (o, x) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let arc = |r: f64| Curve::from(api_curves::create_arc(o, r, 0.0, 180.0).unwrap());
        let profile = [
            segment([18.5, 0.0, 0.0], [40.0, 0.0, 0.0]),
            arc(40.0),
            segment([-40.0, 0.0, 0.0], [-18.5, 0.0, 0.0]),
            arc(18.5),
        ];
        let curves: Vec<Option<&Curve>> = profile.iter().map(Some).collect();

        let mut k = Kernel::default();
        let mut ball = create_rotational_sweep(&mut k, &curves, o, x, 360.0, true).unwrap();
        let store = AttributeStore::default();
        let key_of = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let volume = brep_volume(&k, &mut ball, 1e-6, &attributes).unwrap();
        // 4/3 pi (40^3 - 18.5^3)
        let want =
            4.0 / 3.0 * std::f64::consts::PI * (det::powi(40.0_f64, 3) - det::powi(18.5_f64, 3));
        assert!(
            (volume - want).abs() < 1e-3 * want,
            "volume {volume}, want {want}"
        );
        assert!(crate::topology::brep::p2::is_manifold_solid(&ball).unwrap());

        // WR-443: a difference with the hollow ball as the tool keeps what lies in the
        // hollow. A block wholly inside it keeps its 512; one crossing the inner sphere
        // loses only its part outside r 18.5 (the reference's sphere-difference control).
        for (corner, size, want) in [
            ([-4.0, -4.0, -4.0], 8.0, 512.0),
            ([8.0, -4.0, -4.0], 12.0, 653.342_908_027_093_8),
        ] {
            let [x0, y0, z0] = corner;
            let block = crate::primitive::api_primitives::create_box(
                &mut k,
                Point3::new(x0, y0, z0),
                size,
                8.0,
                8.0,
            )
            .unwrap();
            let mut cut = crate::boolean::api_brep::boolean_difference(
                &mut k,
                block,
                ball.clone_standing_for_self().unwrap(),
            )
            .unwrap();
            let got = brep_volume(&k, &mut cut, 1e-8, &attributes).unwrap();
            assert!(
                (got - want).abs() < 1e-6 * want,
                "block at {corner:?} minus the hollow ball: volume {got}, want {want}"
            );
        }
    }

    /// A dome turned a whole turn about z: a cubic Bezier from the pole on the axis down
    /// to the rim at r 10, a line across the base and one up the axis. With the spline's
    /// pole a hair past the axis (3.4e-11 mm on the far side, as a sketch solver leaves
    /// it; SolidCraft puts the lines' ends on the axis, not a spline's, #17204), well
    /// within the B-rep's tolerance, the body is the one built with the pole on the axis:
    /// the surface of revolution's frame faces the profile.
    /// Before, the frame faced the pole's side, half a turn from the profile, and the
    /// check found the face's edges 10.7 mm off its surface.
    #[test]
    fn issue_17204_dome_pole_a_hair_past_the_axis() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};

        let dome = |x: f64| {
            let pts = [
                [x, 0.0, 5.0],
                [5.0, 0.0, 5.0],
                [10.0, 0.0, 3.0],
                [10.0, 0.0, 0.0],
            ];
            let pw = pts
                .iter()
                .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                .collect();
            let nurb = NurbCurve::new(pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
            let profile = [
                Curve::from(BSplineCurve::from_nurb(nurb, 3, false)),
                segment([10.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                segment([0.0, 0.0, 0.0], [0.0, 0.0, 5.0]),
            ];
            let curves: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
            let (o, z) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
            // with the fixes SolidCraft runs
            let mut k = Kernel {
                fixes: crate::kernel::UpstreamFixes::all(),
                ..Kernel::default()
            };
            let mut b = create_rotational_sweep(&mut k, &curves, o, z, 360.0, true).unwrap();
            let check = crate::topology::brep::validation::check_brep(
                &mut k,
                &mut b,
                crate::topology::brep::validation::CheckLevel::Level2,
            )
            .unwrap();
            assert_eq!(
                check.failure_count(),
                0,
                "pole at x {x}: {:?}",
                check.lines()
            );
            let store = AttributeStore::default();
            let key_of = |_| ObjectKey(0);
            let attributes = PropertyAttributes {
                store: &store,
                key_of: &key_of,
            };
            brep_volume(&k, &mut b, 1e-8, &attributes).unwrap()
        };
        let on = dome(0.0);
        let past = dome(-3.4e-11);
        assert!(on > 0.0, "volume {on}");
        assert!(
            (past - on).abs() <= 1e-6 * on,
            "volume {past}, on the axis {on}"
        );
    }

    /// The on-axis tolerance of a surface of revolution is a finite, non-negative
    /// distance: NaN, infinity and a negative one are refused, not read as the default
    /// or as putting every point on the axis (#17204).
    #[test]
    fn issue_17204_on_axis_tolerance_must_be_finite() {
        use crate::surface::surf_of_revolution::p1::SurfOfRevolution;

        for tol in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            let k = Kernel::default();
            let line = segment([5.0, 0.0, 0.0], [5.0, 0.0, 5.0]);
            let r = SurfOfRevolution::create_canonical_off_axis(
                &k,
                line,
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                false,
                tol,
            );
            assert!(r.is_err(), "tolerance {tol} accepted");
        }
    }

    /// A square with a square hole (20 x 20 at r 10 to 30, the hole 8 x 8 centred in it)
    /// turned a whole turn about z is a ring with a ring-shaped void: with the opt-ins
    /// (WR-529, the fork's tangency-labs/smlib@38b4212) the void is not counted as
    /// material, and the volume is Pappus's, area 336 about the centroid at r 20.
    #[test]
    fn holed_profile_full_turn_leaves_the_hole_a_void() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};

        let square = |x0: f64, z0: f64, side: f64| {
            let (x1, z1) = (x0 + side, z0 + side);
            [
                segment([x0, 0.0, z0], [x1, 0.0, z0]),
                segment([x1, 0.0, z0], [x1, 0.0, z1]),
                segment([x1, 0.0, z1], [x0, 0.0, z1]),
                segment([x0, 0.0, z1], [x0, 0.0, z0]),
            ]
        };
        let mut profile = square(10.0, 0.0, 20.0).to_vec();
        profile.extend(square(16.0, 6.0, 8.0));
        let curves: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
        let (o, z) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));

        let mut k = Kernel::default();
        let mut ring = create_rotational_sweep(&mut k, &curves, o, z, 360.0, true).unwrap();
        let store = AttributeStore::default();
        let key_of = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let volume = brep_volume(&k, &mut ring, 1e-6, &attributes).unwrap();
        let want = (400.0 - 64.0) * 2.0 * std::f64::consts::PI * 20.0;
        assert!(
            (volume - want).abs() < 1e-3 * want,
            "volume {volume}, want {want}"
        );
        let check = crate::topology::brep::validation::check_brep(
            &mut k,
            &mut ring,
            crate::topology::brep::validation::CheckLevel::Level2,
        )
        .unwrap();
        assert_eq!(check.failure_count(), 0, "{:?}", check.lines());
    }

    /// An open chain of two lines (out from r 6 to 8, then up 2) revolved half a turn into
    /// a sheet joins at its meeting vertex (WR-409): two faces, the flat ring sector and
    /// the cylinder, of area (8^2 - 6^2) pi / 2 + 8 pi 2. Upstream left the two wires
    /// apart and the sweep failed.
    #[test]
    fn issue_1113_open_chain_revolves_into_a_joined_sheet() {
        use crate::mass::integrator::properties::{face_area, face_properties};
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::topology::face::p2::PPF_AREA;

        let chain = [
            segment([6.0, 0.0, 0.0], [8.0, 0.0, 0.0]),
            segment([8.0, 0.0, 0.0], [8.0, 0.0, 2.0]),
        ];
        let curves: Vec<Option<&Curve>> = chain.iter().map(Some).collect();
        let (o, z) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));

        let mut k = Kernel::default();
        let mut sheet = create_rotational_sweep(&mut k, &curves, o, z, 180.0, false).unwrap();
        let faces = crate::topology::brep::p1::faces(&sheet, None)
            .unwrap()
            .len();
        assert_eq!(faces, 2);
        // Joined at (8, 0, 0): the profile's three vertices and their copies at 180
        // degrees (unjoined, the coincident ends are two vertices and the sweep fails).
        let vertices = crate::topology::brep::p1::vertices(&sheet, None)
            .unwrap()
            .len();
        assert_eq!(vertices, 6);
        let edges = crate::topology::brep::p1::edges(&sheet, None)
            .unwrap()
            .len();
        assert_eq!(edges, 7);
        // The area is 14 pi (the half annulus) plus 16 pi (the half cylinder), per face
        // through `face_properties` at accuracy 1e-9: `brep_area` / `face_area` clamp the
        // accuracy to [1e-4, 1e-1] (`SmApiFaceComputeArea`), which allows 7.7e-5 here.
        // The bound 1e-8 relative is ten times the integration accuracy.
        let pi = std::f64::consts::PI;
        let mut areas = Vec::new();
        for f in crate::topology::brep::p1::faces(&sheet, None).unwrap() {
            let est = face_area(&k, &mut sheet, f, 1e-4).unwrap();
            let a = face_properties(
                &k,
                &mut sheet,
                f,
                OrientType::Same,
                1e-9,
                Point3::new(0.0, 0.0, 0.0),
                est,
                0.0,
                PPF_AREA,
            )
            .unwrap()
            .area
            .abs();
            areas.push(a);
        }
        areas.sort_by(f64::total_cmp);
        for (got, want) in areas.iter().zip([14.0 * pi, 16.0 * pi]) {
            assert!(
                (got - want).abs() <= 1e-8 * want,
                "face area {got}, want {want}"
            );
        }
    }

    /// Goldens `sweep.api_brep` `issue1463_drilled_hole_{pz,mz}_120` (WR-409): a drilled
    /// hole's open profile, the wall r 2 from z 5 down to 1 and the cone from there to the
    /// axis at the origin, turned 120 degrees about +z and about -z, is a sheet of two
    /// cone faces (the wall's of half-angle 0) meeting at the profile's middle vertex:
    /// 5 vertices (the apex is one), 6 edges, valid at Level 2.
    /// By Pappus the wall is 2 pi / 3 * 2 * 4 = 16 pi / 3 and the cone, slant sqrt 5
    /// about radius 1, is 2 pi sqrt 5 / 3. Upstream left the two wires apart and both
    /// sweeps failed (1001).
    #[test]
    fn issue_1463_drilled_hole_profile_revolves_part_way_into_a_joined_sheet() {
        use crate::mass::integrator::properties::{face_area, face_properties};
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::surface::surface::SurfaceKind;
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        use crate::topology::face::p2::PPF_AREA;

        let chain = [
            segment([2.0, 0.0, 5.0], [2.0, 0.0, 1.0]),
            segment([2.0, 0.0, 1.0], [0.0, 0.0, 0.0]),
        ];
        let curves: Vec<Option<&Curve>> = chain.iter().map(Some).collect();
        let pi = std::f64::consts::PI;
        let want = [2.0 * pi * 5.0_f64.sqrt() / 3.0, 16.0 * pi / 3.0];
        for z in [1.0, -1.0] {
            let mut k = Kernel::default();
            let (o, axis) = (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, z));
            let mut sheet = create_rotational_sweep(&mut k, &curves, o, axis, 120.0, false)
                .unwrap_or_else(|e| panic!("axis z {z}: {e}"));
            let faces = crate::topology::brep::p1::faces(&sheet, None).unwrap();
            assert_eq!(faces.len(), 2, "axis z {z}");
            let edges = crate::topology::brep::p1::edges(&sheet, None).unwrap();
            assert_eq!(edges.len(), 6, "axis z {z}");
            let vertices = crate::topology::brep::p1::vertices(&sheet, None).unwrap();
            assert_eq!(vertices.len(), 5, "axis z {z}");
            // The profile's top end (2, 0, 5) turned 120 degrees the axis's way round.
            let top = Point3::new(-1.0, z * 3.0_f64.sqrt(), 5.0);
            let turned = vertices.iter().any(|&v| {
                crate::topology::vertex::point(&sheet, v).is_ok_and(|p| p.distance(top) < 1e-9)
            });
            assert!(turned, "axis z {z}: no vertex at {top:?}");
            let check = check_brep(&mut k, &mut sheet, CheckLevel::Level2).unwrap();
            assert!(check.is_valid(), "axis z {z}: {:?}", check.lines());
            let mut areas = Vec::new();
            for &f in &faces {
                let surface = crate::topology::face::p2::surface(&sheet, f)
                    .unwrap()
                    .expect("face surface");
                assert!(
                    matches!(surface.kind(), SurfaceKind::Cone(_)),
                    "axis z {z}: face {f:?} is not a cone"
                );
                // `face_properties` at accuracy 1e-9, as in the test above.
                let est = face_area(&k, &mut sheet, f, 1e-4).unwrap();
                let a = face_properties(
                    &k,
                    &mut sheet,
                    f,
                    OrientType::Same,
                    1e-9,
                    Point3::new(0.0, 0.0, 0.0),
                    est,
                    0.0,
                    PPF_AREA,
                )
                .unwrap()
                .area
                .abs();
                areas.push(a);
            }
            areas.sort_by(f64::total_cmp);
            for (got, want) in areas.iter().zip(want) {
                assert!(
                    (got - want).abs() <= 1e-8 * want,
                    "axis z {z}: face area {got}, want {want}"
                );
            }
        }
    }

    /// #21672: a capped revolve by 0 degrees, or by less than the circle builder's
    /// `CONIC_ANGLE_TOL_DEG`, is refused by name. Before, every rail and side face came
    /// out a full turn (an arc below 1e-6 degrees is a whole circle) while the end cap was
    /// the start cap in place: for the unit square [0,1]^2 at z 0 about the x axis through
    /// (0, -5, 0), a ring of volume 11 pi with both caps on its seam (6 faces, 8 edges,
    /// 4 vertices), for a request of no volume. A partial sphere of radius 1e6 turned
    /// 2e-12 degrees was a whole sphere with two half discs through its axis. Turns of
    /// 90 and 360 degrees still build, with their closed-form volumes: the square is 5 to
    /// 6 from the axis, so a turn of t radians sweeps t / 2 (6^2 - 5^2) = 5.5 t.
    #[test]
    fn issue_21672_a_revolve_by_almost_nothing_is_refused() {
        use crate::error::primitive::PrimitiveFailure;
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};
        use crate::nurbs::crv_basic::CONIC_ANGLE_TOL_DEG;
        use crate::primitive::api_primitives::create_partial_sphere;

        let square = [
            segment([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            segment([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            segment([1.0, 1.0, 0.0], [0.0, 1.0, 0.0]),
            segment([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let curves: Vec<Option<&Curve>> = square.iter().map(Some).collect();
        let (base, axis) = (Point3::new(0.0, -5.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let too_small = |angle: f64| PrimitiveFailure::SweepAngleTooSmall {
            angle,
            minimum: CONIC_ANGLE_TOL_DEG,
        };

        let mut k = Kernel::default();
        for angle in [0.0, 1e-7, -1e-7, 2e-12, 0.99e-6] {
            for cap in [true, false] {
                let got = create_rotational_sweep(&mut k, &curves, base, axis, angle, cap);
                let failure = got
                    .as_ref()
                    .err()
                    .and_then(|e| e.primitive_failure())
                    .copied();
                assert_eq!(
                    failure,
                    Some(too_small(angle)),
                    "a turn of {angle} degrees (caps {cap}): {:?}",
                    got.map(|_| ())
                );
            }
        }
        let got = create_partial_sphere(
            &mut k,
            Point3::new(0.0, 0.0, 0.0),
            1e6,
            30.0,
            30.000000000002,
        );
        assert!(
            matches!(
                got.as_ref().err().and_then(|e| e.primitive_failure()),
                Some(PrimitiveFailure::SweepAngleTooSmall { angle, minimum })
                    if *angle < 1e-11 && *minimum == CONIC_ANGLE_TOL_DEG
            ),
            "a partial sphere turned 2e-12 degrees: {:?}",
            got.map(|_| ())
        );

        let store = AttributeStore::default();
        let key_of = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        for (angle, cap) in [(90.0, true), (360.0, true)] {
            let mut ring = create_rotational_sweep(&mut k, &curves, base, axis, angle, cap)
                .unwrap_or_else(|e| panic!("a turn of {angle} degrees: {e}"));
            assert!(crate::topology::brep::p2::is_manifold_solid(&ring).unwrap());
            let volume = brep_volume(&k, &mut ring, 1e-8, &attributes).unwrap();
            let want = 5.5 * f64::to_radians(angle);
            assert!(
                (volume - want).abs() < 1e-6 * want,
                "a turn of {angle} degrees: volume {volume}, want {want}"
            );
        }
        // a quarter of the unit ball, pi / 3
        let mut wedge =
            create_partial_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 0.0, 90.0).unwrap();
        let volume = brep_volume(&k, &mut wedge, 1e-8, &attributes).unwrap();
        let want = std::f64::consts::PI / 3.0;
        assert!(
            (volume - want).abs() < 1e-6 * want,
            "volume {volume}, want {want}"
        );
    }
}
