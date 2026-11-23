//! Collapse the transverse edges of a strip of fillets and sew each pair of rails
//! together on the sharp curve. The adjacent faces keep their surfaces and identities.

use super::ring::{extend_line_seam, RingFilletRemoval};
use super::{
    bspline_part_with_nurb, glue_edges, ner, own_edge_curve, refused, set_vertex_point,
    stage_failed,
};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::blend::{BlendFailure, RemovalRefusal as R};
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::implicit::AnalyticSupport;
use crate::surface::surface::exact_pieces::certified_lift;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::p3::{delete_face, with_kernel_callbacks, DeleteFaceOptions};
use crate::topology::brep::p4::{delete_edge, DeleteEnv};
use crate::topology::brep::p5::glue_vertices;
use crate::topology::edge::{self, SetCurveOptions};
use crate::topology::edgeuse::p1::{self as eu, UvTrimCurveArg};
use crate::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use crate::topology::face::p2 as face;
use crate::topology::topology_traverser::Marks;
use crate::topology::{vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};
use std::collections::{BTreeMap, BTreeSet};

/// The sharp curve starts at the same end of the strip section as the first rail.
pub(super) struct Section {
    pub(super) face: FaceId,
    pub(super) rails: [EdgeId; 2],
    /// Both supporting trims must fit the tighter of the original rails' budgets.
    pub(super) rail_tolerance: f64,
    pub(super) sharp: Curve,
    pub(super) rail_orientation: OrientType,
}

/// One transverse edge, shared by two sections or meeting the end cap of an open strip.
pub(super) struct Junction {
    pub(super) cross_edge: EdgeId,
    pub(super) vertices: [VertexId; 2],
    pub(super) point: Point3,
}

/// The surviving boundary between two support faces (or a seam of one of them).
/// Each UV is kept on its original use, including both sides of a periodic seam.
struct Side {
    edge: EdgeId,
    span: Extent1d,
    curve: Option<Curve>,
    trims: Vec<(EdgeuseId, Curve)>,
}

struct Join {
    cap_edge: Option<EdgeId>,
    keep: VertexId,
    remove: VertexId,
    point: Point3,
}

type SideTargets = BTreeMap<EdgeId, [Option<Point3>; 2]>;

/// Notify the geometry change (invalidating the old trims), then bind the curve to
/// the edge under the same identity the B-rep records for its geometry owner.
fn install_curve(k: &mut Kernel, b: &mut Brep, e: EdgeId, curve: Option<Curve>) -> Result<()> {
    with_kernel_callbacks(k, b, |_, b, env| {
        let options = SetCurveOptions {
            delete_old_curve: true,
            ..SetCurveOptions::default()
        };
        edge::set_curve(b, e, curve, options, env, [None, None])?;
        own_edge_curve(b, e)
    })
}

/// Identify the edges which run on to a junction on the *retained* faces. The rails
/// and cross edges of neighbouring sections are all incident on these same vertices.
fn retained_sides(
    b: &Brep,
    sections: &[Section],
    junctions: &[Junction],
) -> Result<(Vec<Join>, SideTargets)> {
    let faces: BTreeSet<_> = sections.iter().map(|s| s.face).collect();
    let strip: BTreeSet<_> = sections
        .iter()
        .flat_map(|s| s.rails)
        .chain(junctions.iter().map(|j| j.cross_edge))
        .collect();
    let mut joins = Vec::new();
    let mut sides = SideTargets::new();
    for j in junctions {
        let incident = edge::faces(b, j.cross_edge, None)?;
        let cap = incident.iter().any(|f| !faces.contains(f));
        let mut keeps_side = [false; 2];
        for (&v, keeps) in j.vertices.iter().zip(&mut keeps_side) {
            for e in vertex::edges(b, v, None)? {
                if strip.contains(&e) {
                    continue;
                }
                if *keeps && cap {
                    return Err(refused(R::CornerNeedsFittedIntersection));
                }
                let (start, end) = edge::start_end_vertices(b, e)?;
                if start == end {
                    return Err(refused(R::RailVertexHasOtherEdges));
                }
                let [low, high] = sides.entry(e).or_default();
                let target = if start == Some(v) {
                    low
                } else if end == Some(v) {
                    high
                } else {
                    return Err(stage_failed());
                };
                *target = Some(j.point);
                *keeps = true;
            }
        }
        if cap && keeps_side != [true, true] {
            return Err(refused(R::CornerNeedsFittedIntersection));
        }
        let [first, second] = j.vertices;
        let (keep, remove) = if keeps_side == [false, true] {
            (second, first)
        } else {
            (first, second)
        };
        joins.push(Join {
            cap_edge: cap.then_some(j.cross_edge),
            keep,
            remove,
            point: j.point,
        });
    }
    Ok((joins, sides))
}

/// The point lies on the current curve or on its analytic continuation at the
/// specified end. For a circular seam the continuation keeps the original parameter
/// by lifting its meridian on the support. A free-standing circle does not fix that
/// parameterisation.
fn side_parameter(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    curve: &Curve,
    point: Point3,
    high: bool,
    tol: f64,
) -> Result<(f64, Option<Curve>)> {
    let project = |c: &Curve, span: Extent1d| {
        c.drop_point(k, &span, point, None, tol, None, SolverOperation::Minimize)
    };
    let on_curve = project(curve, curve.natural_interval())?;
    if !on_curve.success {
        return Err(stage_failed());
    }
    if on_curve.distance <= tol {
        return Ok((on_curve.parameter, None));
    }
    let (continued, domain) = if curve.is_linear(k, tol, false)?.linear {
        let v = ner(if high {
            edge::end_vertex(b, e)?
        } else {
            edge::start_vertex(b, e)?
        })?;
        let reach = vertex::point(b, v)?.distance(point);
        let line = extend_line_seam(k, curve, high, reach, tol)?;
        let span = line.natural_interval();
        (line, span)
    } else if matches!(curve.kind(), CurveKind::Ellipse(c) if c.is_circle()) {
        let mut support = None;
        for f in edge::faces(b, e, None)? {
            if matches!(
                ner(face::surface(b, f)?)?.kind(),
                SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)
            ) {
                support = Some(f);
                break;
            }
        }
        RingFilletRemoval::extend_circular_seam(
            k,
            b,
            support.ok_or_else(|| refused(R::SideEdgeNotStraight))?,
            e,
            high,
            tol,
        )?
    } else {
        return Err(refused(R::SideEdgeNotStraight));
    };
    let at = project(&continued, domain)?;
    if !at.success {
        return Err(stage_failed());
    }
    if at.distance > tol {
        return Err(refused(R::SeamTrimMissesNewVertex));
    }
    Ok((at.parameter, Some(continued)))
}

fn side_edit(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    ends: [Option<Point3>; 2],
    tol: f64,
) -> Result<Side> {
    let tol = tol.min(edge::tolerance(b, e)?);
    let uses = b.edge(e)?.edgeuses.clone();
    for &use_id in &uses {
        get_or_create_uv_trim_curve(k, b, use_id, false, false, true)?.status?;
    }
    let old_span = edge::interval(b, e)?;
    let mut limits = [old_span.min(), old_span.max()];
    let old_curve = ner(edge::curve(b, e)?)?;
    let mut replacement = None;
    for ((high, point), limit) in [false, true].into_iter().zip(ends).zip(&mut limits) {
        if let Some(p) = point {
            let current = replacement.as_ref().unwrap_or(old_curve);
            let (at, longer) = side_parameter(k, b, e, current, p, high, tol)?;
            *limit = at;
            if longer.is_some() {
                replacement = longer;
            }
        }
    }
    let [low, high] = limits;
    if !(low.is_finite() && high.is_finite() && low < high) {
        return Err(refused(R::SeamTrimMissesNewVertex));
    }
    let span = Extent1d::from_raw(low, high);
    if let Some(curve) = replacement.as_mut() {
        let mut actual = span;
        curve.trim(k, &mut actual, TrimMode::Notify)?;
        if actual != span {
            curve.edit_parameterization_exactly(k, &span)?;
        }
    }
    let mut trims = Vec::new();
    for use_id in uses {
        let Some(old) = eu::uv_trim_curve_pointer(b, use_id)? else {
            continue;
        };
        let mut uv = old.clone_for_evaluation()?;
        for t in limits {
            if !uv.natural_interval().contains_value(t, 0.0) {
                let spline = ner(bspline_part_with_nurb(&uv)?)?;
                uv = Curve::from(ner(spline.create_continued_curve(t)?)?);
            }
        }
        trims.push((use_id, uv));
    }
    if trims.is_empty() {
        return Err(refused(R::SeamHasNoOwnTrim));
    }
    Ok(Side {
        edge: e,
        span,
        curve: replacement,
        trims,
    })
}

/// Grow the face window and its straight surface spans over a retained trim. A
/// plane can continue either way and a cone or cylinder along its generator.
fn include_trim(k: &Kernel, b: &mut Brep, f: FaceId, domain: Extent2d) -> Result<()> {
    let domain = face::uv_domain(b, f)?.union(&domain);
    face::set_uv_domain(b, f, &domain)?;
    let surface = ner(b.face_mut(f)?.surface.as_mut())?;
    match surface.kind_mut() {
        SurfaceKind::Plane(plane) => {
            let basis = plane.natural_uv_domain();
            if !domain.is_contained_by(&basis, 0.0) {
                plane.update_analytical_domain(&domain.union(&basis));
            }
        }
        SurfaceKind::Cone(cone) => {
            // A cone's generator is straight. Continue that boundary span in its
            // existing parameters, keeping the circle and its periodic seam in place.
            let swap = cone.rev().swap_uv();
            let basis = surface.natural_uv_domain_with_nurb(k)?;
            let (dir, old, target) = if swap {
                (SurfParamType::U, basis.u_interval(), domain.u_interval())
            } else {
                (SurfParamType::V, basis.v_interval(), domain.v_interval())
            };
            if !target.is_contained_by(&old, 0.0) {
                let support = AnalyticSupport::check(k, surface)?;
                super::expand_boundary_span(k, surface, target.min(), dir)?;
                super::expand_boundary_span(k, surface, target.max(), dir)?;
                let grown = surface.natural_uv_domain_with_nurb(k)?;
                super::update_analytical_domain(k, surface, &grown)?;
                if AnalyticSupport::check(k, surface)? != support {
                    return Err(refused(R::TangentSurfacesDoNotMeet));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// A STEP trim fitted to a cone's periodic seam can drift past the parameter
/// window when continued. The seam is the boundary ruling on that side of the
/// face. Put its UV on that ruling, then certify it over the whole edge.
fn cone_seam_trim(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    f: FaceId,
    uv: &Curve,
    span: Extent1d,
) -> Result<Option<Curve>> {
    let surface = ner(face::surface(b, f)?)?;
    let SurfaceKind::Cone(cone) = surface.kind() else {
        return Ok(None);
    };
    let swap = cone.rev().swap_uv();
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let angle = if swap {
        domain.v_interval()
    } else {
        domain.u_interval()
    };
    let angular = |p: Point3| if swap { p.y } else { p.x };
    let mut start = uv.evaluate_point(k, span.min())?;
    let mut end = uv.evaluate_point(k, span.max())?;
    if angle.contains_value(angular(start), 0.0) && angle.contains_value(angular(end), 0.0) {
        return Ok(None);
    }
    if !edge::is_seam_of_surface(k, b, e, f)? {
        return Ok(None);
    }
    let mid = angular(uv.evaluate_point(k, span.mid())?);
    let at = if mid < angle.mid() {
        angle.min()
    } else {
        angle.max()
    };
    if swap {
        start.y = at;
        end.y = at;
    } else {
        start.x = at;
        end.x = at;
    }
    Ok(Some(Curve::from(Line::create_line_segment(
        2,
        start,
        end,
        Some(&span),
    )?)))
}

impl Side {
    /// An imported plane can end at the old rail. Continue its analytic domain
    /// before checking the lift over the new part of each trim.
    fn prepare(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        domains: &mut BTreeMap<FaceId, Extent2d>,
        tol: f64,
    ) -> Result<()> {
        let tol = tol.min(edge::tolerance(b, self.edge)?);
        let old = edge::interval(b, self.edge)?;
        for (use_id, uv) in &mut self.trims {
            let f = ner(eu::face(b, *use_id)?)?;
            let ruling = cone_seam_trim(k, b, self.edge, f, uv, self.span)?;
            let replaced = ruling.is_some();
            if let Some(ruling) = ruling {
                *uv = ruling;
            }
            let bounds = ner(uv.certified_bounding_box(k, &self.span)?)?;
            let domain = domains.entry(f).or_insert(face::uv_domain(b, f)?);
            *domain = domain.union(&Extent2d::from_extent3d(&bounds));
            include_trim(k, b, f, *domain)?;
            let surface = ner(face::surface(b, f)?)?;
            let curve = ner(self.curve.as_ref().or(edge::curve(b, self.edge)?))?;
            if replaced {
                certified_lift(k, surface, uv, curve, self.span, tol)?;
                continue;
            }
            for (a, z) in [(self.span.min(), old.min()), (old.max(), self.span.max())] {
                if a < z {
                    certified_lift(k, surface, uv, curve, Extent1d::from_raw(a, z), tol)?;
                }
            }
        }
        Ok(())
    }
}

/// Make enough of each support available to project the sharp curve before deleting
/// either rail. A plane's projection is affine, so the projected eight box corners
/// enclose the entire curve, also for a tilted plane and a circular edge.
fn open_supports(
    k: &Kernel,
    b: &mut Brep,
    s: &Section,
    domains: &mut BTreeMap<FaceId, Extent2d>,
) -> Result<()> {
    for rail in s.rails {
        for f in edge::faces(b, rail, None)? {
            if f == s.face {
                continue;
            }
            domains.entry(f).or_insert(face::uv_domain(b, f)?);
            let surface = ner(face::surface(b, f)?)?;
            let domain = match surface.kind() {
                SurfaceKind::Plane(p) => {
                    let bounds = ner(s
                        .sharp
                        .certified_bounding_box(k, &s.sharp.natural_interval())?)?;
                    let mut domain = Extent2d::new();
                    for corner in bounds.corners() {
                        domain.add_point(p.project_point_to_uv_domain(corner));
                    }
                    domain
                }
                _ => surface.natural_uv_domain_with_nurb(k)?,
            };
            include_trim(k, b, f, domain)?;
        }
    }
    Ok(())
}

/// The caller owns the rollback covering recognition, intersection and this edit.
pub(super) fn remove_chain(
    k: &mut Kernel,
    b: &mut Brep,
    sections: &[Section],
    junctions: &[Junction],
    tol: f64,
) -> Result<()> {
    let (joins, targets) = retained_sides(b, sections, junctions)?;
    let mut sides = Vec::new();
    for (e, ends) in targets {
        RingFilletRemoval::extend_circular_support(k, b, e, ends, tol)?;
        sides.push(side_edit(k, b, e, ends, tol)?);
    }
    let mut domains = BTreeMap::new();
    for side in &mut sides {
        side.prepare(k, b, &mut domains, tol)?;
    }
    for section in sections {
        open_supports(k, b, section, &mut domains)?;
    }
    for section in sections {
        with_kernel_callbacks(k, b, |k, b, env| {
            delete_face(
                k,
                b,
                env,
                &mut Marks::default(),
                section.face,
                DeleteFaceOptions {
                    // Once the last incident face has gone, an internal cross edge is a wire.
                    delete_edges_vertices: true,
                    do_region_nesting: false,
                    region_to_keep: None,
                },
            )
        })?;
    }
    for side in sides {
        if let Some(curve) = side.curve {
            install_curve(k, b, side.edge, Some(curve))?;
        }
        edge::set_interval(b, side.edge, &side.span)?;
        for (use_id, uv) in side.trims {
            eu::set_uv_trim_curve(
                b,
                use_id,
                UvTrimCurveArg::New(Box::new(uv)),
                0.0,
                true,
                None,
            )?;
        }
    }
    for join in joins {
        if let Some(e) = join.cap_edge {
            with_kernel_callbacks(k, b, |k, b, env| {
                delete_edge(k, b, &mut DeleteEnv::from_notify(env), e, None)
            })?;
        }
        // GlueVertices encloses the distance between its inputs in the surviving
        // tolerance. Both old vertices now represent the same computed sharp corner.
        set_vertex_point(k, b, join.keep, join.point)?;
        set_vertex_point(k, b, join.remove, join.point)?;
        glue_vertices(k, b, Some(join.keep), Some(join.remove), None)?;
    }
    for section in sections {
        let [keep, remove] = section.rails;
        let span = section.sharp.natural_interval();
        install_curve(k, b, keep, Some(section.sharp.clone_for_evaluation()?))?;
        install_curve(k, b, remove, None)?;
        edge::set_interval(b, keep, &span)?;
        edge::set_interval(b, remove, &span)?;
        // GlueEdges normally encloses the *larger* of the two tolerances. The old
        // rails are gone here: the new curve and both trims share the tighter budget.
        for rail in section.rails {
            edge::set_tolerance(b, rail, section.rail_tolerance, false, false)?;
        }
        with_kernel_callbacks(k, b, |k, b, env| {
            glue_edges(k, b, env, keep, section.rail_orientation, 0.0, remove)
        })?;
    }
    b.refresh_cached_topology();
    for section in sections {
        let [keep, _] = section.rails;
        let span = section.sharp.natural_interval();
        let budget = section.rail_tolerance;
        for use_id in b.edge(keep)?.edgeuses.clone() {
            // The two members of a mated pair share one trim on the same support.
            if eu::orientation(b, use_id)? != OrientType::Same {
                continue;
            }
            get_or_create_uv_trim_curve(k, b, use_id, false, false, true)?.status?;
            let f = ner(eu::face(b, use_id)?)?;
            let uv = ner(eu::uv_trim_curve(b, use_id)?.0)?;
            let refusal = || {
                Failure::new(
                    super::OP,
                    BlendFailure::RemovalRefused(R::RailTrimOutsideTolerance),
                )
                .at(EntityRef::Edge(keep))
            };
            if edge::tolerance(b, keep)? > budget || edge::interval(b, keep)? != span {
                return Err(refusal().into_error());
            }
            certified_lift(
                k,
                ner(face::surface(b, f)?)?,
                uv,
                &section.sharp,
                span,
                budget,
            )
            .map_err(|e| match e {
                e @ SolidError::Unsupported { .. } => refusal().caused_by(e).into_error(),
                e => e,
            })?;
            let bounds = ner(uv.certified_bounding_box(k, &span)?)?;
            let domain = domains.get_mut(&f).ok_or_else(stage_failed)?;
            *domain = domain.union(&Extent2d::from_extent3d(&bounds));
        }
    }
    // The full supporting domains were only search windows for projecting the sharp
    // edges. Keep each original face domain and the actual new trims it must contain.
    for (f, domain) in domains {
        face::set_uv_domain(b, f, &domain)?;
    }
    Ok(())
}
