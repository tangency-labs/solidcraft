//! Reconstruction of an offset boundary when a periodic chart seam disappears.
//!
//! Upstream: none. A seam is a cut of the parameter chart, not a material support.
//! The endpoint events are roots on the genuine support beyond that cut. Where
//! the seam joins a closed rim, its two incidences and the part of the rim across
//! the chart cut are replaced by the rim between those roots. All remaining
//! oriented boundary curves retain their source identities.
//!
//! Deviations: this first construction covers a seam adjoining one closed rim.
//! Other seam incidence graphs are refused; they are never joined by projection.
//! No recursion; walks are bounded by the source and result topology sizes.

use std::collections::{BTreeMap, BTreeSet};

use super::p1::OffsetExecutive;
use super::p2::OffsetNotifications;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::NotifyOperation;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::exact_pieces::{certified_lift, common_trim_interval, trim_unsnapped};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::brep::p3::{delete_face_with_body_identity, DeleteFaceOptions};
use crate::topology::brep::p7::{make_face_with_curve_owners, FaceCurvesInput};
use crate::topology::edge::{self, NotificationStores, NotifyState};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::p2 as face;
use crate::topology::topology;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP: &str = "offset::offset_executive::periodic_boundary";

/// One branch ending on the genuine boundary beyond an obsolete chart seam.
#[derive(Debug, Clone, Copy)]
pub(super) struct BoundaryEnd {
    pub(super) edgeuse: EdgeuseId,
    pub(super) vertex: VertexId,
    pub(super) seam: EdgeuseId,
    pub(super) across: EdgeuseId,
    pub(super) point: Point3,
}

/// Record one observation in a group already keyed by source face and seam
/// edge. The two chart-side uses may observe the same event, but a different
/// support or root is an additional constraint the planner must see.
pub(super) fn record_boundary_end(events: &mut Vec<BoundaryEnd>, event: BoundaryEnd) {
    if !events.iter().any(|e| {
        e.edgeuse == event.edgeuse
            && e.vertex == event.vertex
            && e.across == event.across
            && e.point == event.point
    }) {
        events.push(event);
    }
}

// Keep the missing relation in the diagnostic without allocating or discarding
// the typed Result. These are construction invariants, not projection candidates.
macro_rules! required {
    ($value:expr) => {
        $value.ok_or_else(|| {
            SolidError::invalid_input(
                OP,
                concat!("missing periodic boundary topology: ", stringify!($value)),
            )
        })
    };
}

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

fn edge_of(b: &Brep, u: EdgeuseId) -> Result<EdgeId> {
    required!(eu::edge(b, u)?)
}

/// The builder discards a coincident input curve while retaining its shared
/// edge. Copies of one source edge trimmed to the same interval are the same
/// 3D curve by construction. Its unique completed owner therefore identifies
/// that second incidence without a geometric search.
fn incidence_owners(
    b: &Brep,
    sources: &[EdgeuseId],
    intervals: &[Extent1d],
    owners: &[Option<EdgeId>],
) -> Result<Vec<EdgeId>> {
    if sources.len() != intervals.len() || sources.len() != owners.len() {
        return Err(unsupported(
            "boundary source, interval and owner counts disagree",
        ));
    }
    let mut completed: BTreeMap<EdgeId, Vec<(Extent1d, EdgeId)>> = BTreeMap::new();
    for ((&source, &interval), &owner) in sources.iter().zip(intervals).zip(owners) {
        if let Some(owner) = owner {
            completed
                .entry(edge_of(b, source)?)
                .or_default()
                .push((interval, owner));
        }
    }
    let mut result = Vec::new();
    for ((&source, &interval), &owner) in sources.iter().zip(intervals).zip(owners) {
        let owner = if let Some(owner) = owner {
            owner
        } else {
            let mut shared = BTreeSet::new();
            for &(other_interval, edge) in required!(completed.get(&edge_of(b, source)?))? {
                if interval == other_interval {
                    shared.insert(edge);
                }
            }
            if shared.len() != 1 {
                return Err(unsupported(
                    "coincident boundary incidence has no unique identical owner",
                ));
            }
            required!(shared.into_iter().next())?
        };
        result.push(owner);
    }
    Ok(result)
}

/// Invert a root already proved on both supports on their rim. This is a
/// parameter solve, with the body's inherited error bound, not a nearest choice
/// of material side.
fn rim_parameter(k: &Kernel, b: &Brep, edge: EdgeId, point: Point3) -> Result<f64> {
    let record = b.edge(edge)?;
    let curve = required!(record.curve.as_ref())?;
    let solutions = curve.global_point_solve(
        k,
        &record.interval,
        SolverOperation::Minimize,
        point,
        record.tolerance,
        SolutionRequested::Single,
        &GlobalPointSolveOptions::<NoObject>::default(),
    )?;
    let t = required!(solutions.get(0))?.start.param(0)?;
    let gap = curve.evaluate_point(k, t)?.distance(point);
    if !t.is_finite() || !gap.is_finite() || gap > record.tolerance {
        return Err(SolidError::invalid_input(
            OP,
            "boundary root is not on its rim",
        ));
    }
    Ok(t)
}

struct Plan {
    face: FaceId,
    input: FaceCurvesInput,
    sources: Vec<EdgeuseId>,
    old_uses: Vec<EdgeuseId>,
    surface: Surface,
    domain: crate::geom::extent2d::Extent2d,
}

/// Each new arc has its prescribed orientation and meets its two prescribed
/// neighbours. In particular the closed-rim cut must not choose its other arc.
fn validate_plan(k: &Kernel, b: &Brep, plan: &Plan) -> Result<()> {
    let curves = required!(plan.input.curves_3d.as_ref())?;
    let uvs = required!(plan.input.uv_curves.as_ref())?;
    let mut offset = 0usize;
    for &count in &plan.input.loops {
        let mut ends = Vec::new();
        let stop = offset
            .checked_add(count)
            .ok_or_else(|| unsupported("boundary loop count overflow"))?;
        for index in offset..stop {
            let curve = required!(required!(curves.get(index))?.as_ref())?;
            let interval = curve.natural_interval();
            let a = curve.evaluate_point(k, interval.min())?;
            let z = curve.evaluate_point(k, interval.max())?;
            let uv = required!(uvs.get(index))?;
            if uv.natural_interval() != interval {
                return Err(unsupported("boundary 3D and UV intervals disagree"));
            }
            let ua = uv.evaluate_point(k, interval.min())?;
            let uz = uv.evaluate_point(k, interval.max())?;
            let (a, z, ua, uz) = match required!(plan.input.orientations.get(index))? {
                OrientType::Same => (a, z, ua, uz),
                OrientType::Opposite => (z, a, uz, ua),
                OrientType::Unknown => {
                    return Err(unsupported("boundary incidence has unknown orientation"))
                }
            };
            let source = *required!(plan.sources.get(index))?;
            ends.push((a, z, ua, uz, b.edge(edge_of(b, source)?)?.tolerance));
        }
        let first = required!(ends.first())?;
        for ((_, end, _, uv_end, tol_a), (start, _, uv_start, _, tol_z)) in ends
            .iter()
            .zip(ends.iter().skip(1).chain(std::iter::once(first)))
        {
            let tolerance = tol_a.max(*tol_z).max(b.tolerance());
            let gap = end.distance(*start);
            let uv_gap = uv_end.distance(*uv_start);
            let uv_tolerance = crate::surface::surface::p1::map_to_2d(
                k,
                tolerance,
                Point2::new(uv_end.x, uv_end.y),
                &plan.surface,
            )?;
            if !tolerance.is_finite()
                || !uv_tolerance.is_finite()
                || uv_tolerance < 0.0
                || !gap.is_finite()
                || gap > tolerance
                || !uv_gap.is_finite()
                || uv_gap > uv_tolerance
            {
                return Err(SolidError::invalid_input(
                    OP,
                    "periodic boundary arcs do not close in their prescribed order",
                ));
            }
        }
        offset = stop;
    }
    if offset != curves.len() || offset != uvs.len() || offset != plan.sources.len() {
        return Err(unsupported("boundary loop and curve counts disagree"));
    }
    Ok(())
}

impl OffsetExecutive {
    /// Reconstruct the periodic quotient boundary before the gap closer observes
    /// the old seam vertex. Every plan is validated before any face is replaced.
    pub(super) fn reconstruct_periodic_boundaries(
        &mut self,
        k: &mut Kernel,
        original: &mut Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        if self.periodic_boundary_ends.is_empty() {
            return Ok(());
        }
        let mut b = required!(self.take_offset_brep())?;
        let result = (|| {
            let mut groups: BTreeMap<(FaceId, EdgeId), Vec<BoundaryEnd>> = BTreeMap::new();
            for event in &self.periodic_boundary_ends {
                let f = required!(eu::face(original, event.seam)?)?;
                let seam = edge_of(original, event.seam)?;
                let events = groups.entry((f, seam)).or_default();
                record_boundary_end(events, *event);
            }
            let mut plans = Vec::new();
            let mut faces = BTreeSet::new();
            for ((source_face, seam), events) in groups {
                let plan = self.periodic_boundary_plan(
                    k,
                    original,
                    &b,
                    source_face,
                    seam,
                    &events,
                    notifications,
                )?;
                validate_plan(k, &b, &plan)?;
                if !faces.insert(plan.face) {
                    return Err(unsupported(
                        "multiple periodic boundary events affect one face",
                    ));
                }
                plans.push(plan);
            }
            let mut supports = BTreeSet::new();
            for event in &self.periodic_boundary_ends {
                let radial = required!(eu::radial(original, event.across)?)?;
                supports.insert(required!(eu::face(original, radial)?)?);
            }
            let mut separated = BTreeSet::new();
            for source_face in supports {
                if let Some(plan) =
                    self.separated_rim_plan(k, original, &b, source_face, notifications)?
                {
                    validate_plan(k, &b, &plan)?;
                    if !faces.insert(plan.face) {
                        return Err(unsupported(
                            "periodic boundary and separated rim plans overlap",
                        ));
                    }
                    separated.insert(plan.face);
                    plans.push(plan);
                }
            }
            for plan in plans {
                let resolve = separated.contains(&plan.face);
                let new_face = self.apply_periodic_boundary_plan(k, &mut b, plan, notifications)?;
                if resolve {
                    self.resolve_crossing_loops_of_face(
                        k,
                        original,
                        &mut b,
                        new_face,
                        notifications,
                    )?;
                }
            }
            self.periodic_boundary_ends.clear();
            Ok(())
        })();
        self.set_offset_brep(b);
        result
    }

    /// Shell stores the face image on the source incidence selected by region
    /// and inset. Either faceuse can carry that relation; their union must name
    /// exactly one image for this local reconstruction.
    fn periodic_face_image(&self, original: &Brep, source: FaceId) -> Result<FaceId> {
        let fu = required!(face::upward_faceuse(original, source)?)?;
        let mate = required!(eu::faceuse_mate(original, fu)?)?;
        let mut images = BTreeSet::new();
        for incidence in [fu, mate] {
            images.extend(self.faceuse_to_faces.seconds(incidence));
        }
        if images.len() != 1 {
            return Err(unsupported("source face has no unique offset face image"));
        }
        required!(images.into_iter().next())
    }

    fn source_edge_of_use(&self, original: &Brep, b: &Brep, u: EdgeuseId) -> Result<EdgeId> {
        let mut sources = self.edgeuse_to_edgeuses.firsts(u);
        sources.extend(self.edgeuse_to_edgeuses.firsts(required!(eu::mate(b, u)?)?));
        let mut edges = BTreeSet::new();
        for source in sources {
            edges.insert(edge_of(original, source)?);
        }
        if edges.len() != 1 {
            return Err(unsupported("boundary incidence has no unique source edge"));
        }
        required!(edges.into_iter().next())
    }

    /// A seam's missing pcurve is the retained pcurve on its other chart side,
    /// translated by the surface's exact parameter period. No drop against the
    /// not-yet-reconstructed face, or tolerance-changing drop retry, is needed.
    fn boundary_uv(&self, k: &Kernel, original: &Brep, b: &Brep, u: EdgeuseId) -> Result<Curve> {
        if let Some(uv) = eu::uv_trim_curve(b, u)?.0 {
            return uv.copy(k);
        }
        let originals = self.edgeuse_to_edgeuses.firsts(u);
        let (source, from_mate) = match originals.as_slice() {
            [source] => (*source, false),
            [] => {
                let uses = self.edgeuse_to_edgeuses.firsts(required!(eu::mate(b, u)?)?);
                let [source] = uses.as_slice() else {
                    return Err(unsupported("boundary mate has no unique source incidence"));
                };
                (required!(eu::mate(original, *source)?)?, true)
            }
            _ => {
                return Err(unsupported(
                    "boundary incidence has multiple source incidences",
                ))
            }
        };
        let other = required!(eu::radial(original, source)?)?;
        let source_face = required!(eu::face(original, source)?)?;
        if eu::face(original, other)? != Some(source_face) {
            return Err(unsupported(
                "missing pcurve does not belong to a same-face seam",
            ));
        }
        let source_surface = required!(original.face(source_face)?.surface.as_ref())?;
        let domain = source_surface.natural_uv_domain_with_nurb(k)?;
        let here = required!(eu::uv_trim_curve(original, source)?.0)?;
        let there = required!(eu::uv_trim_curve(original, other)?.0)?;
        let delta = here.evaluate_point(k, here.natural_interval().evaluate(0.5))?
            - there.evaluate_point(k, there.natural_interval().evaluate(0.5))?;
        let (axis, period, shift) = if delta.x.abs() > delta.y.abs() {
            let period = domain.u_interval().length();
            (
                SurfParamType::U,
                period,
                Vec3::new(if delta.x > 0.0 { period } else { -period }, 0.0, 0.0),
            )
        } else if delta.y != 0.0 {
            let period = domain.v_interval().length();
            (
                SurfParamType::V,
                period,
                Vec3::new(0.0, if delta.y > 0.0 { period } else { -period }, 0.0),
            )
        } else {
            return Err(unsupported("seam pcurves do not identify a periodic axis"));
        };
        if !source_surface.is_periodic(k, &domain, axis)? {
            return Err(unsupported(
                "source seam surface is not periodic on its chart",
            ));
        }
        let target_face = required!(eu::face(b, u)?)?;
        let target_surface = required!(b.face(target_face)?.surface.as_ref())?;
        let target_domain = target_surface.natural_uv_domain_with_nurb(k)?;
        let target_period = match axis {
            SurfParamType::U => target_domain.u_interval().length(),
            SurfParamType::V => target_domain.v_interval().length(),
            _ => return Err(unsupported("unsupported target seam parameter axis")),
        };
        if period != target_period || !target_surface.is_periodic(k, &target_domain, axis)? {
            return Err(unsupported("source and target seam periods disagree"));
        }
        let other_offset = if from_mate {
            let source_mate = required!(eu::mate(original, other)?)?;
            let image = required!(self.offset_edgeuse(source_mate))?;
            required!(eu::mate(b, image)?)?
        } else {
            required!(self.offset_edgeuse(other))?
        };
        let uv = required!(eu::uv_trim_curve(b, other_offset)?.0)?;
        let mut translated = required!(uv.b_spline_base())?.copy();
        for point in translated.get_or_create_gw_nurb()?.cpts_mut() {
            *point = point.translate(shift);
        }
        let translated = Curve::from(translated);
        let record = b.edge(edge_of(b, u)?)?;
        certified_lift(
            k,
            target_surface,
            &translated,
            required!(record.curve.as_ref())?,
            record.interval,
            record.tolerance,
        )?;
        Ok(translated)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one boundary event plan
    fn periodic_boundary_plan(
        &mut self,
        k: &Kernel,
        original: &Brep,
        b: &Brep,
        source_face: FaceId,
        seam: EdgeId,
        events: &[BoundaryEnd],
        notifications: &OffsetNotifications<'_>,
    ) -> Result<Plan> {
        let [a, z] = events else {
            return Err(unsupported("periodic rim needs exactly two boundary ends"));
        };
        let rim = edge_of(original, a.across)?;
        let near = a.vertex;
        let far = original
            .edge(seam)?
            .vertices
            .iter()
            .copied()
            .find(|&v| v != near);
        let far = required!(far)?;
        if z.vertex != near
            || !original.edge(rim)?.vertices.iter().all(|&v| v == far)
            || edge_of(original, z.across)? != rim
            || original.edge(rim)?.vertices.first() != original.edge(rim)?.vertices.last()
            || edge_of(original, a.edgeuse)? == edge_of(original, z.edgeuse)?
        {
            return Err(unsupported(
                "periodic boundary event is not a seam adjoining one closed rim",
            ));
        }
        let f = self.periodic_face_image(original, source_face)?;
        let mut fu = required!(face::upward_faceuse(b, f)?)?;
        if eu::faceuse_orientation(b, fu)? != OrientType::Same {
            fu = required!(eu::faceuse_mate(b, fu)?)?;
        }
        let mut input = FaceCurvesInput::default();
        let mut sources = Vec::new();
        let mut old_uses = Vec::new();
        let mut curves = Vec::new();
        let mut uvs = Vec::new();
        let mut changed = false;
        for lu in eu::faceuse_loopuses(b, fu, false, None)? {
            let uses = eu::loopuse_edgeuses(b, lu)?;
            let source_edges: Vec<EdgeId> = uses
                .iter()
                .map(|&u| self.source_edge_of_use(original, b, u))
                .collect::<Result<_>>()?;
            let rim_index = source_edges.iter().position(|&e| e == rim);
            let has_seam = source_edges.contains(&seam);
            let mut cut = None;
            if has_seam {
                let rr = required!(rim_index)?;
                let prev = rr
                    .checked_sub(1)
                    .or_else(|| uses.len().checked_sub(1))
                    .ok_or_else(|| unsupported("boundary loop count overflow"))?;
                let next = if rr + 1 == uses.len() { 0 } else { rr + 1 };
                if source_edges.get(prev) != Some(&seam)
                    || source_edges.get(next) != Some(&seam)
                    || source_edges.iter().filter(|&&e| e == seam).count() != 2
                {
                    return Err(unsupported(
                        "obsolete seam incidences do not adjoin their closed rim",
                    ));
                }
                let rim_use = *required!(uses.get(rr))?;
                let rim_edge = edge_of(b, rim_use)?;
                let ta = rim_parameter(k, b, rim_edge, a.point)?;
                let tz = rim_parameter(k, b, rim_edge, z.point)?;
                let rim_interval = b.edge(rim_edge)?.interval;
                if ta == tz
                    || ![ta, tz]
                        .iter()
                        .all(|&t| rim_interval.min() < t && t < rim_interval.max())
                {
                    return Err(unsupported(
                        "boundary roots do not cut the interior of their closed rim",
                    ));
                }
                // The old seam's far vertex is the closed edge's sole vertex,
                // hence its interval cut. Removing that incidence removes the
                // arc through the cut, leaving the interval between the roots.
                // validate_plan verifies its direction against both SSI ends.
                cut = Some((rim_use, Extent1d::from_min_max(ta, tz)));
                changed = true;
            }
            let mut count = 0;
            for (&u, &source_edge) in uses.iter().zip(&source_edges) {
                old_uses.push(u);
                if source_edge == seam {
                    continue;
                }
                let edge = b.edge(edge_of(b, u)?)?;
                let mut curve = required!(edge.curve.as_ref())?.copy(k)?;
                let mut uv = self.boundary_uv(k, original, b, u)?;
                let interval = match cut {
                    Some((rim_use, interval)) if rim_use == u => {
                        common_trim_interval(k, &[&curve, &uv], interval, OP)?
                    }
                    _ => edge.interval,
                };
                trim_unsnapped(k, &mut curve, interval, OP)?;
                trim_unsnapped(k, &mut uv, interval, OP)?;
                curves.push(Some(curve));
                uvs.push(uv);
                sources.push(u);
                input.orientations.push(eu::orientation(b, u)?);
                count += 1;
            }
            input.loops.push(count);
        }
        if !changed {
            return Err(unsupported("periodic face has no obsolete seam incidence"));
        }
        input.curves_3d = Some(curves);
        input.uv_curves = Some(uvs);
        let surface = required!(b.face(f)?.surface.as_ref())?;
        let margin = self.offset_extension_margin(f);
        let extension = self
            .get_create_extended_surface(
                k,
                (notifications.result_key_of)(EntityRef::Face(f)).key.0,
                surface,
                margin,
            )?
            .copy(k)?;
        let domain = extension.natural_uv_domain_with_nurb(k)?;
        Ok(Plan {
            face: f,
            input,
            sources,
            old_uses,
            surface: extension,
            domain,
        })
    }

    /// A source vertex can identify the seams of two otherwise closed rims.
    /// After displacement those rims are independent closed curves. Separate
    /// their cycles before arranging their actual crossings; the old vertex is
    /// not a geometric intersection of the displaced supports.
    fn separated_rim_plan(
        &mut self,
        k: &Kernel,
        original: &Brep,
        b: &Brep,
        source_face: FaceId,
        notifications: &OffsetNotifications<'_>,
    ) -> Result<Option<Plan>> {
        let f = self.periodic_face_image(original, source_face)?;
        let mut fu = required!(face::upward_faceuse(b, f)?)?;
        if eu::faceuse_orientation(b, fu)? != OrientType::Same {
            fu = required!(eu::faceuse_mate(b, fu)?)?;
        }
        let mut loops = Vec::new();
        let mut changed = false;
        for lu in eu::faceuse_loopuses(b, fu, false, None)? {
            let uses = eu::loopuse_edgeuses(b, lu)?;
            let all_closed = uses.iter().try_fold(true, |closed, &u| -> Result<bool> {
                let record = b.edge(edge_of(b, u)?)?;
                Ok(closed && record.vertices.first() == record.vertices.last())
            })?;
            if uses.len() > 1 && all_closed {
                changed = true;
                for u in uses {
                    loops.push(vec![u]);
                }
            } else {
                loops.push(uses);
            }
        }
        if !changed {
            return Ok(None);
        }
        let mut classified = Vec::new();
        for uses in loops {
            let mut uv = Vec::new();
            let mut senses = Vec::new();
            for &u in &uses {
                let mut curve = self.boundary_uv(k, original, b, u)?;
                trim_unsnapped(k, &mut curve, b.edge(edge_of(b, u)?)?.interval, OP)?;
                uv.push(curve);
                senses.push(eu::orientation(b, u)? == OrientType::Same);
            }
            let orientation =
                crate::topology::face::p3::uv_loop_orientation(k, &uv, &senses, None)?;
            if orientation == OrientType::Unknown {
                return Err(unsupported(
                    "separated rim cycle has unknown UV orientation",
                ));
            }
            classified.push((orientation == OrientType::Same, uses, uv));
        }
        if classified.iter().filter(|(outer, _, _)| *outer).count() != 1 {
            return Err(unsupported(
                "separated rim cycles have no unique outer cycle",
            ));
        }
        classified.sort_by_key(|(outer, _, _)| !*outer);
        let mut input = FaceCurvesInput::default();
        let mut sources = Vec::new();
        let mut curves = Vec::new();
        let mut uvs = Vec::new();
        for (_, uses, uv) in classified {
            input.loops.push(uses.len());
            for u in uses {
                let record = b.edge(edge_of(b, u)?)?;
                let mut curve = required!(record.curve.as_ref())?.copy(k)?;
                trim_unsnapped(k, &mut curve, record.interval, OP)?;
                curves.push(Some(curve));
                sources.push(u);
                input.orientations.push(eu::orientation(b, u)?);
            }
            uvs.extend(uv);
        }
        input.curves_3d = Some(curves);
        input.uv_curves = Some(uvs);
        let surface = required!(b.face(f)?.surface.as_ref())?;
        let margin = self.offset_extension_margin(f);
        let extension = self
            .get_create_extended_surface(
                k,
                (notifications.result_key_of)(EntityRef::Face(f)).key.0,
                surface,
                margin,
            )?
            .copy(k)?;
        let domain = extension.natural_uv_domain_with_nurb(k)?;
        Ok(Some(Plan {
            face: f,
            input,
            old_uses: sources.clone(),
            sources,
            surface: extension,
            domain,
        }))
    }

    #[allow(clippy::too_many_lines)] // one notified face replacement
    fn apply_periodic_boundary_plan(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        plan: Plan,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<FaceId> {
        let key = notifications.result_key_of;
        let mut owners = Vec::new();
        let intervals = required!(plan.input.curves_3d.as_ref())?
            .iter()
            .map(|curve| Ok(required!(curve.as_ref())?.natural_interval()))
            .collect::<Result<Vec<_>>>()?;
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: key,
        };
        let region = b.infinite_region();
        let made = NotifyState::from_kernel_stores(&mut stores).with_kernel_env(k, |k, env| {
            make_face_with_curve_owners(
                k,
                b,
                region,
                plan.input,
                plan.surface,
                &plan.domain,
                OrientType::Same,
                Some(true),
                &|f| key(EntityRef::Face(f)),
                &mut owners,
                env,
            )
        })?;
        let new_face = required!(made.face)?;
        let owners = incidence_owners(b, &plan.sources, &intervals, &owners)?;
        let mut pairs = vec![(EntityRef::Face(plan.face), EntityRef::Face(new_face))];
        for (&source_use, owner) in plan.sources.iter().zip(owners) {
            let new_edge = owner;
            let source_edge = edge_of(b, source_use)?;
            pairs.push((EntityRef::Edge(source_edge), EntityRef::Edge(new_edge)));
            let tolerance = b.edge(source_edge)?.tolerance;
            edge::set_tolerance(b, new_edge, tolerance, true, false)?;
            let source_record = b.edge(source_edge)?;
            let new_record = b.edge(new_edge)?;
            let old_parameters = [source_record.interval.min(), source_record.interval.max()];
            let new_parameters = [new_record.interval.min(), new_record.interval.max()];
            let mut vertex_copies = Vec::new();
            for ((&old_v, &new_v), (&old_t, &new_t)) in source_record
                .vertices
                .iter()
                .zip(&new_record.vertices)
                .zip(old_parameters.iter().zip(&new_parameters))
            {
                if old_t == new_t
                    && b.vertex(old_v)?.point.distance(b.vertex(new_v)?.point) <= tolerance
                {
                    pairs.push((EntityRef::Vertex(old_v), EntityRef::Vertex(new_v)));
                    vertex_copies.push((new_v, b.vertex(old_v)?.tolerance));
                }
            }
            for (v, tolerance) in vertex_copies {
                crate::topology::vertex::set_tolerance(b, v, tolerance, true, false)?;
            }
            for old in [source_use, required!(eu::mate(b, source_use)?)?] {
                let old_fu = required!(eu::faceuse(b, old)?)?;
                let old_orientation = eu::faceuse_orientation(b, old_fu)?;
                let old_sense = eu::orientation(b, old)?;
                let mut matching_uses = Vec::new();
                for new_use in edge::edgeuses(b, new_edge, None, None)? {
                    let new_fu = required!(eu::faceuse(b, new_use)?)?;
                    if eu::faceuse_orientation(b, new_fu)? == old_orientation
                        && eu::orientation(b, new_use)? == old_sense
                    {
                        matching_uses.push(new_use);
                    }
                }
                let [new_use] = matching_uses.as_slice() else {
                    return Err(unsupported(
                        "rebuilt edge incidence has no unique matching faceuse and sense",
                    ));
                };
                for original in self.edgeuse_to_edgeuses.firsts(old) {
                    self.edgeuse_to_edgeuses.relate_pair(original, *new_use)?;
                }
            }
        }
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: key,
        };
        NotifyState::from_kernel_stores(&mut stores).with_kernel_env(k, |_, env| {
            let mut copied = BTreeSet::new();
            for (old, new) in pairs {
                if !copied.insert((old, new)) {
                    continue;
                }
                topology::notify(
                    env.records,
                    env.attributes,
                    env.context.as_deref_mut(),
                    NotifyOperation::Copy,
                    key(old),
                    Some(key(new)),
                    None,
                    None,
                )?;
            }
            Ok(())
        })?;
        for source in self.faceuse_to_faces.firsts(plan.face) {
            self.faceuse_to_faces.remove_pair(source, plan.face)?;
            self.faceuse_to_faces.relate_pair(source, new_face)?;
        }
        for source in self.edge_to_faces.firsts(plan.face) {
            self.edge_to_faces.remove_pair(source, plan.face)?;
            self.edge_to_faces.relate_pair(source, new_face)?;
        }
        for source in self.vertex_to_faces.firsts(plan.face) {
            self.vertex_to_faces.remove_pair(source, plan.face)?;
            self.vertex_to_faces.relate_pair(source, new_face)?;
        }
        for blended in self.blend_to_faces.seconds(plan.face) {
            self.blend_to_faces.remove_pair(plan.face, blended)?;
            self.blend_to_faces.relate_pair(new_face, blended)?;
        }
        for blend in self.blend_to_faces.firsts(plan.face) {
            self.blend_to_faces.remove_pair(blend, plan.face)?;
            self.blend_to_faces.relate_pair(blend, new_face)?;
        }
        for source_use in plan.old_uses {
            for old in [source_use, required!(eu::mate(b, source_use)?)?] {
                for source in self.edgeuse_to_edgeuses.firsts(old) {
                    self.edgeuse_to_edgeuses.remove_pair(source, old)?;
                }
            }
        }
        if self.inverted_faces.remove(&plan.face) {
            self.inverted_faces.insert(new_face);
        }
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: key,
        };
        NotifyState::from_kernel_stores(&mut stores).with_kernel_env(k, |k, env| {
            delete_face_with_body_identity(
                k,
                b,
                env,
                plan.face,
                DeleteFaceOptions {
                    delete_edges_vertices: true,
                    do_region_nesting: true,
                    region_to_keep: None,
                },
                notifications.result_brep,
            )
        })?;
        Ok(new_face)
    }
}
