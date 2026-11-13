//! Curve-sweep, swung, skinned and blended primitive orchestration.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`,
//! `source/SMLib/src/SmBSplineSurfaceNLib.cpp` (`CreateSwungSurface`).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation::create_curve_sweep`] | `CreateCurveSweep` |
//! | [`PrimitiveCreation::create_curve_sweep_with_placements`], `profile_motions` (private) | `CreateCurveSweep` with the SolidCraft fork's `pOptPlacements` |
//! | [`PrimitiveCreation::create_swung_primitive`] | `CreateSwungPrimitive` |
//! | [`PrimitiveCreation::create_skin_from_faces`] | `CreateSkinFromFaces` |
//! | [`PrimitiveCreation::create_blend_primitive`] | `CreateBlendPrimitive` |
//! | `swung_curve_scale` (private) | `CreateSwungPrimitive` scale initialization |
//! | `swung_surface` (private) | `CreateSwungSurface` virtual NURB acquisition |
//!
//! Deviations: typed service resolution replaces pointers. Output face lists
//! preserve partial results; owned geometry handles cleanup. Missing analytic
//! NURBs are generated on owned copies with caller context/owner. No recursion.
//! Not upstream (WR-404, tangency-labs/solidcore#411): the curve sweep's caps
//! come from the side faces at the sweep's ends (`sweep_end_curves`), not the first
//! and last `count` side faces.
//! Not upstream (tangency-labs/solidcore#2192, opt-in
//! `Kernel::fixes.curve_sweep_closed_path_joins_ends`): a sweep along a closed path
//! whose end sections lie on its start sections (`sweep_ends_coincide`) is not capped,
//! so the stitch joins its side faces' end edges to their start edges.
//! Not upstream (the SolidCraft fork's tangency-labs/smlib 5d3939e, WR-523, #1450):
//! [`PrimitiveCreation::create_curve_sweep_with_placements`] reports the rigid motions
//! carrying the profile onto the first, middle and last sections.
//! Not upstream: [`PrimitiveCreation::create_skin_from_faces`] skins sections on
//! parallel planes at their planes' distances (`SectionPlanes`) where the chord-length
//! skin's control points leave the slab between the first and last planes, so the skin
//! stays between them; every other skin keeps the chord-length parameters.
//! Not upstream (#15263): turning planar sections carry their winding along the
//! centroid travel and align their cyclic seams by exact curve evaluations before
//! joining. Parallel sections with coherent winding retain their correspondence.
//! **Statuses pass on** (opt-in `Kernel::fixes.sweep_statuses_pass_on`, WR-662;
//! SolidCraft change, tangency-labs/smlib c8f6a31): the statuses upstream drops here
//! are returned instead (`SER`).

use super::{
    geometry::{prepare_curve_sweep, GeometryHandle, PreparedCurveSweep},
    PrimitiveAccess,
};
use crate::curve::composite_curve::CompositeCurve;
use crate::curve::curve::p2::CurvePropertyType;
use crate::curve::curve::p7::PropertyAnalysisOptions;
use crate::curve::curve::Curve;
use crate::error::{ignore_status, passed_on, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::tol::ZONE_TOL_3D;
use crate::geom::vector2d::Point2;
use crate::math::{Point3, Vec3, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::{BSplineSurface, SurfParamType};
use crate::nurbs::b_spline_surface_n_lib::p2::SkinnedSurfaceOptions;
use crate::nurbs::knots::Side;
use crate::primitive::primitive_creation::{p1::PrimitiveCreation, SweepOptions};
use crate::surface::procedural_sweep::ProceduralSweep;
use crate::surface::surface::{
    p2::{BlendEndType, CurvatureType, DerivSurfDefinition},
    Surface,
};
use crate::sweep::topology_sweep::access::{FaceRef, RegionRef};
use crate::topology::{BrepId, EdgeuseId, FaceId};

use super::skin_sources::{layer_sources, LoopSpans, NotTraced, Segment, SkinSources, SkinTrace};

const OP: &str = "primitive::primitive_creation::p3";

/// Exact polygon area moments for degree-one profile curves. A whole polygon may
/// be one B-spline edge: its control points are exact corners, not display vertices.
fn linear_section_centroid(
    loops: &[(CompositeCurve, Vec<crate::topology::EdgeId>)],
    normal: Vec3,
) -> Result<Option<Point3>> {
    let mut origin = None;
    let mut twice_area = 0.0;
    let mut moment = Vec3::ZERO;
    for (composite, _) in loops {
        let curve = composite.clone().make_composite_nurb()?;
        if curve.degree()? != 1 {
            return Ok(None);
        }
        let points = curve.control_points()?;
        let Some((first, last)) = points.first().zip(points.last()) else {
            return Ok(None);
        };
        let first = first.to_euclid();
        if first != last.to_euclid() {
            return Ok(None);
        }
        let reference = *origin.get_or_insert(first);
        for pair in points.windows(2) {
            let Some((p, q)) = pair.first().zip(pair.get(1)) else {
                continue;
            };
            let p = p.to_euclid() - reference;
            let q = q.to_euclid() - reference;
            let area = p.cross(q).dot(normal);
            twice_area += area;
            moment += (p + q) * area;
        }
    }
    let Some(origin) = origin else {
        return Ok(None);
    };
    if !twice_area.is_finite() || twice_area == 0.0 {
        return Ok(None);
    }
    let centroid = origin + moment / (3.0 * twice_area);
    if ![centroid.x, centroid.y, centroid.z]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(SolidError::primitive_failed(
            OP,
            "a section centroid is non-finite",
        ));
    }
    Ok(Some(centroid))
}

/// Carry the winding through a turn before joining the section's constituent curves.
/// Reorder the edge provenance with the curves, so skin history follows the new seam.
fn align_section_loop(
    k: &crate::kernel::Kernel,
    composite: &mut CompositeCurve,
    edges: &mut Vec<crate::topology::EdgeId>,
    previous: &BSplineCurve,
    reverse: bool,
) -> Result<()> {
    // A closed B-spline can hold an entire loop in one edge. Its knot parameters
    // are seam candidates too; every piece retains that edge's source.
    if composite.num_segments() == 1 {
        let joined = composite.make_composite_nurb()?;
        let domain = joined.natural_interval();
        let (knots, _) = joined.knots(None)?;
        let mut remainder = Curve::from(joined);
        let mut pieces = Vec::new();
        for t in knots {
            if t > domain.min() && t < domain.max() {
                let interval = remainder.natural_interval();
                let mut head = remainder.copy(k)?;
                // These are existing exact knots. Trim directly rather than using
                // SplitCurve's proximity rejection for arbitrary split parameters.
                head.trim(
                    k,
                    &mut crate::geom::extent1d::Extent1d::from_min_max(interval.min(), t),
                    crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
                )?;
                remainder.trim(
                    k,
                    &mut crate::geom::extent1d::Extent1d::from_min_max(t, interval.max()),
                    crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
                )?;
                pieces.push(head);
            }
        }
        if !pieces.is_empty() {
            pieces.push(remainder);
            for piece in &mut pieces {
                let interval = piece.natural_interval();
                piece.edit_parameterization(
                    k,
                    &crate::geom::extent1d::Extent1d::from_min_max(
                        domain.min(),
                        domain.min() + interval.max() - interval.min(),
                    ),
                )?;
            }
            if let Some(&edge) = edges.first() {
                edges.resize(pieces.len(), edge);
            }
            *composite = CompositeCurve::with_curves(3, pieces, true, None, None)?;
        }
    }
    let mut segments = composite.segments().to_vec();
    if reverse {
        segments.reverse();
        edges.reverse();
        for segment in &mut segments {
            segment.same_sense = !segment.same_sense;
            std::mem::swap(&mut segment.gap_start, &mut segment.gap_end);
        }
    }
    let make = |segments: &[crate::curve::composite_curve::CompositeCurveSegment]| {
        let mut candidate = composite.clone();
        for (i, segment) in segments.iter().enumerate() {
            candidate.set_segment(i, segment.clone())?;
        }
        Ok::<_, SolidError>(candidate)
    };
    let mut best = make(&segments)?;
    let mut best_score = f64::INFINITY;
    let mut best_shift = 0;
    let previous_domain = previous.natural_interval();
    // Compare exact curve evaluations at the joined curves' knot-span ends and
    // midpoints. This uses neither a tessellation nor a nearest display vertex.
    for shift in 0..segments.len() {
        let mut candidate = make(&segments)?;
        let joined = candidate.make_composite_nurb()?;
        let domain = joined.natural_interval();
        let range = domain.max() - domain.min();
        if !range.is_finite() || range <= 0.0 {
            return Err(SolidError::primitive_failed(
                OP,
                "a section loop has no finite parameter interval",
            ));
        }
        let (knots, _) = joined.knots(None)?;
        let mut score = 0.0;
        for span in knots.windows(2) {
            let Some((&start, &end)) = span.first().zip(span.get(1)) else {
                continue;
            };
            for t in [start, (start + end) * 0.5] {
                let fraction = (t - domain.min()) / range;
                let p = previous.evaluate_point(previous_domain.evaluate(fraction))?;
                score += (joined.evaluate_point(t)? - p).length();
            }
        }
        if !score.is_finite() {
            return Err(SolidError::primitive_failed(
                OP,
                "section correspondence has a non-finite distance",
            ));
        }
        if score < best_score {
            best_score = score;
            best_shift = shift;
            best = candidate;
        }
        // Move the first segment to the end, without a call that can panic.
        let mut rest = std::mem::take(&mut segments).into_iter();
        let first = rest.next();
        segments = rest.chain(first).collect();
    }
    if edges.len() == segments.len() {
        let mut rest = std::mem::take(edges).into_iter();
        let head: Vec<_> = rest.by_ref().take(best_shift).collect();
        *edges = rest.chain(head).collect();
    } else {
        // Missing provenance must only disable the trace, never the geometry.
        edges.clear();
    }
    *composite = best;
    Ok(())
}

pub(super) fn target(primitive: &PrimitiveCreation, brep: BrepId) -> Result<RegionRef> {
    Ok((
        brep,
        primitive
            .region()
            .ok_or_else(|| SolidError::primitive_missing(OP, "the target region"))?,
    ))
}
fn face_surface(a: &impl PrimitiveAccess, face: FaceRef) -> Result<&Surface> {
    crate::topology::face::p2::surface(a.brep(face.0)?, face.1)?
        .ok_or_else(|| SolidError::primitive_missing(OP, "the face's surface"))
}

/// Not upstream: section faces that are planes, three or more, all parallel to the first
/// (within `EFF_ZERO_DEG`) and strictly in order along its normal.
///
/// A skin through them is meant to stay in the slab between the first and last planes.
/// Upstream's chord-length parameters come from the control points' whole motion: where
/// sections move far sideways for a small step along the normal (a carved top's first
/// steps), the height's interpolant overshoots, the skin dips behind its first section's
/// plane and crosses its own cap face, and a union with a body whose face is coplanar
/// with that cap fails. [`SectionPlanes::leaves`] finds that on the chord-length skin's
/// control points; only then is the skin built again at [`SectionPlanes::params`].
struct SectionPlanes {
    origin: Point3,
    unit: Vec3,
    /// The last plane's height above the first along `unit` (non-zero).
    last: f64,
    /// Each plane's height over `last`: 0 at the first section, 1 at the last.
    params: Vec<f64>,
}

impl SectionPlanes {
    /// `None` for sections that are not all on parallel planes in order.
    fn of(a: &impl PrimitiveAccess, faces: &[FaceRef]) -> Result<Option<Self>> {
        use crate::math::EFF_ZERO_DEG;
        use crate::surface::surface::SurfaceKind;
        if faces.len() < 3 {
            return Ok(None);
        }
        let mut planes = Vec::new();
        for &face in faces {
            let k = a.kernel(face.0)?;
            match face_surface(a, face)?.kind_with_nurb(k)? {
                SurfaceKind::Plane(p) => {
                    let (point, normal, _) = p.is_planar();
                    planes.push((point, normal));
                }
                _ => return Ok(None),
            }
        }
        let Some(&(origin, normal)) = planes.first() else {
            return Ok(None);
        };
        let Ok(unit) = normal.unitize() else {
            return Ok(None);
        };
        let mut heights = Vec::new();
        for &(point, n) in &planes {
            if !unit.is_parallel_to(n, EFF_ZERO_DEG) {
                return Ok(None);
            }
            heights.push(unit.dot(point - origin));
        }
        let Some(&last) = heights.last() else {
            return Ok(None);
        };
        // Strictly in order along the normal, either way (NaN fails every comparison).
        let increasing = heights
            .iter()
            .zip(heights.iter().skip(1))
            .all(|(p, q)| q > p);
        let decreasing = heights
            .iter()
            .zip(heights.iter().skip(1))
            .all(|(p, q)| q < p);
        if !(increasing || decreasing) || !last.is_finite() || last == 0.0 {
            return Ok(None);
        }
        let mut params: Vec<f64> = heights.iter().map(|h| h / last).collect();
        // The ends exactly 0 and 1, as the interpolation's knot vector needs.
        if let Some(f) = params.first_mut() {
            *f = 0.0;
        }
        if let Some(l) = params.last_mut() {
            *l = 1.0;
        }
        if !params.iter().all(|t| t.is_finite()) {
            return Ok(None);
        }
        Ok(Some(Self {
            origin,
            unit,
            last,
            params,
        }))
    }

    /// Whether a control point of `surface` lies farther than `tol` outside the slab
    /// between the first and last planes. A non-rational skin lies in its control points'
    /// convex hull, so with none outside it stays in the slab (within `tol`); a rational
    /// one (positive weights) likewise, read on its Euclidean points.
    fn leaves(&self, surface: &BSplineSurface, tol: f64) -> Result<bool> {
        let (low, high) = if self.last > 0.0 {
            (-tol, self.last + tol)
        } else {
            (self.last - tol, tol)
        };
        let (_, _, points) = surface.control_points()?;
        Ok(points.into_iter().any(|p| {
            let h = self.unit.dot(p.to_euclid() - self.origin);
            // NaN counts as leaving.
            !(h >= low && h <= high)
        }))
    }

    /// Plane-distance parameters (each section's height is then one linear function of
    /// its parameter, which the interpolation reproduces exactly: the control rows lie
    /// on the planes at their Greville abscissae's heights and the skin in the slab).
    fn params(&self) -> &[f64] {
        &self.params
    }
}

/// Parameters for `CreateBlendPrimitive`.
#[derive(Debug, Clone, Copy)]
pub struct BlendPrimitiveOptions {
    /// Requested blend continuity.
    pub curvature: CurvatureType,
    /// Smooth or cusped ends.
    pub ends: BlendEndType,
    /// 0 guesses directions; 1 matches starts; 2 matches start to end.
    pub curves_direction: u64,
    /// Starting takeoff length.
    pub start_tangent_length: f64,
    /// Ending takeoff length (divided by the start, including zero, upstream).
    pub end_tangent_length: f64,
}

// Keep the derived curve dispatch here: the upstream SM_CAST_PTR checks the
// inheritance chain without slicing SmLine/conics to their cached NURB base.
fn swung_curve_scale(
    kernel: &crate::kernel::Kernel,
    path: &Curve,
    profile: &Curve,
    approx_tol: f64,
) -> Result<f64> {
    let mut point = profile.evaluate_point(kernel, profile.natural_interval().min())?;
    let normal = Vec3::Y;
    let d = -point.dot(normal);
    let property = path.global_property_analysis(
        kernel,
        &profile.natural_interval(),
        CurvePropertyType::PlaneIntersection,
        approx_tol,
        &PropertyAnalysisOptions {
            property_value: Some(d),
            opt_vectors: &[normal],
            handle: Some(GeometryHandle(path.natural_interval())),
            ..Default::default()
        },
    )?;
    if let Some(solution) = property.solutions.get(0) {
        point = path.evaluate_point(kernel, solution.start.param(0)?)?;
    }
    Ok((1.0 / point.x).abs())
}

// CreateSwungSurface queries GetOrCreateGwNurbPointer on each original
// derived curve. Copying a lazy SmLine's empty base would lose its MakeNurb.
fn swung_nurb(kernel: &crate::kernel::Kernel, curve: &Curve) -> Result<BSplineCurve> {
    super::geometry::bspline_with_nurb(kernel, curve)?
        .ok_or_else(|| SolidError::invalid_input(OP, "non B-spline swung input"))
}

fn swung_surface(
    kernel: &crate::kernel::Kernel,
    path: &Curve,
    profile: &Curve,
    scale: f64,
) -> Result<BSplineSurface> {
    // The native builder obtains section data before trajectory data.
    let profile = swung_nurb(kernel, profile)?;
    let path = swung_nurb(kernel, path)?;
    BSplineSurface::create_swung_surface(&profile, &path, scale)
}

impl PrimitiveCreation {
    /// Build swung surfaces for every B-spline-compatible path/profile pair.
    /// Only pair (0,0) initializes the scale, even if earlier slots were skipped.
    pub fn create_swung_primitive(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        paths: &[Option<Curve>],
        profiles: &[Option<Curve>],
        scale: f64,
        approx_tol: f64,
    ) -> Result<()> {
        self.init_entity_maps();
        let _ = target(self, brep)?;
        let mut curve_scale = 0.0;
        let mut faces = Vec::new();
        for (i, path) in paths.iter().enumerate() {
            let Some(path) = path.as_ref() else {
                continue;
            };
            if !path.is_kind_of(BSplineCurve::TYPE) {
                continue;
            }
            for (j, profile) in profiles.iter().enumerate() {
                let Some(profile) = profile.as_ref() else {
                    continue;
                };
                if !profile.is_kind_of(BSplineCurve::TYPE) {
                    continue;
                }
                if i == 0 && j == 0 {
                    curve_scale = swung_curve_scale(a.kernel(brep)?, path, profile, approx_tol)?;
                }
                let surface = swung_surface(a.kernel(brep)?, path, profile, scale * curve_scale)?;
                let surface = Surface::from(surface);
                // (a B-spline surface just made: never lazy)
                let domain = surface.natural_uv_domain_if_built();
                a.surface_faces(brep, surface, domain, &mut faces)?;
            }
        }
        Ok(())
    }

    /// Loft matching loops of faces, or the supplied sequence of profile curves.
    /// `new_faces` is reset before any failing call, as upstream.
    pub fn create_skin_from_faces(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        faces: &[FaceRef],
        profiles: &[BSplineCurve],
        approx_tol: f64,
        degree: usize,
        new_faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        self.create_skin_from_faces_traced(
            a, brep, faces, profiles, approx_tol, degree, new_faces, None,
        )
    }

    /// [`PrimitiveCreation::create_skin_from_faces`], also tracing which profile curves
    /// each new face was swept from into `trace` when given (not upstream:
    /// [`super::skin_sources`], tangency-labs/solidcore#3139). The faces and surfaces are
    /// those `create_skin_from_faces` makes; the trace adds no failure.
    ///
    /// # Errors
    ///
    /// Those of [`PrimitiveCreation::create_skin_from_faces`].
    #[allow(clippy::too_many_arguments)] // upstream's arguments and the trace
    pub fn create_skin_from_faces_traced(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        faces: &[FaceRef],
        profiles: &[BSplineCurve],
        approx_tol: f64,
        degree: usize,
        new_faces: &mut Vec<FaceId>,
        mut trace: Option<&mut SkinTrace>,
    ) -> Result<()> {
        self.init_entity_maps();
        new_faces.clear();
        let _ = target(self, brep)?;
        let mut skin_curves = Vec::new();
        let mut layers = Vec::new();
        let mut layer_count = 0;
        // (the trace: each section loop's spans, by layer, and the first reason it fails)
        let mut loop_spans: Vec<(usize, LoopSpans)> = Vec::new();
        let mut untraced: Option<NotTraced> = None;
        let mut previous_section: Option<(Point3, Vec3)> = None;
        let mut previous_loops: Vec<BSplineCurve> = Vec::new();
        for (i, &face) in faces.iter().enumerate() {
            let domain = crate::topology::face::p2::uv_domain(a.brep(face.0)?, face.1)?;
            let normal = face_surface(a, face)?.evaluate_normal(
                a.kernel(brep)?,
                domain.evaluate(0.5, 0.5),
                Side::Left,
                Side::Left,
            )?;
            let planar = matches!(
                face_surface(a, face)?.kind(),
                crate::surface::surface::SurfaceKind::Plane(_)
            );
            let composites = a.face_composites_with_edges(face)?;
            let mut section = if planar {
                let centroid = match linear_section_centroid(&composites, normal)? {
                    Some(centroid) => centroid,
                    None => {
                        a.with_primitive_brep(face.0, |k, b| {
                            // Integration accuracy is independent of the skin's zero
                            // approximation tolerance for exact section interpolation.
                            crate::mass::integrator::properties::face_centroid(k, b, face.1, 1e-9)
                        })?
                        .1
                    }
                };
                Some((centroid, normal))
            } else {
                None
            };
            let mut reverse = false;
            let mut align = false;
            if let (Some((point, previous_normal)), Some((centroid, normal))) =
                (previous_section, section)
            {
                let direction = centroid - point;
                let before = previous_normal.dot(direction);
                let after = normal.dot(direction);
                reverse = (before < 0.0 && after > 0.0) || (before > 0.0 && after < 0.0);
                // Preserve the caller's correspondence byte for byte on ordinary
                // parallel sections. Turns need a new seam as well as winding.
                align =
                    reverse || !previous_normal.is_parallel_to(normal, crate::math::EFF_ZERO_DEG);
                if reverse {
                    section = Some((centroid, -normal));
                }
            }
            if i == 0 {
                layer_count = composites.len();
            } else if layer_count != composites.len() {
                return Err(SolidError::primitive_failed(
                    OP,
                    "the faces have different numbers of layers",
                ));
            }
            let mut current_loops = Vec::new();
            for (j, (mut composite, mut edges)) in composites.into_iter().enumerate() {
                if align {
                    if let Some(previous) = previous_loops.get(j) {
                        align_section_loop(
                            a.kernel(brep)?,
                            &mut composite,
                            &mut edges,
                            previous,
                            reverse,
                        )?;
                    }
                }
                let (joined, breaks) = composite.make_composite_nurb_with_breaks()?;
                if trace.is_some() && untraced.is_none() {
                    let segments = edges.into_iter().map(Segment::Edge).collect();
                    loop_spans.push((j, LoopSpans { breaks, segments }));
                }
                current_loops.push(joined.clone());
                skin_curves.push(joined);
                layers.push(j);
            }
            previous_section = section;
            previous_loops = current_loops;
        }
        if faces.is_empty() {
            skin_curves.extend_from_slice(profiles);
            layer_count = 1;
            layers.resize(skin_curves.len(), 0);
            // (the trace: the open loops the skin recorded, one per profile, all one layer)
            if let Some(t) = trace.as_deref_mut() {
                let open = std::mem::take(&mut t.open_loops);
                if open.len() == profiles.len() {
                    loop_spans.extend(open.into_iter().map(|l| (0, l)));
                } else {
                    untraced = Some(NotTraced::OpenProfiles);
                }
            }
        }
        // (the trace: where each section's knots went, when asked; the surface is the same
        // either way)
        let want_maps = trace.is_some() && untraced.is_none();
        // Each layer's skin at chord-length parameters, as upstream.
        let skin = |k: &crate::kernel::Kernel, curves: &[BSplineCurve], params: Option<&[f64]>| {
            BSplineSurface::create_skinned_surface_mapped(
                k,
                curves,
                SkinnedSurfaceOptions {
                    approx_tol,
                    degree,
                    deriv_surfs: Some([None, None]),
                    section_params: params,
                    ..Default::default()
                },
                want_maps,
            )
        };
        let mut layer_curves = Vec::new();
        let mut surfaces = Vec::new();
        for layer in 0..layer_count {
            let curves: Vec<BSplineCurve> = skin_curves
                .iter()
                .zip(&layers)
                .filter(|(_, l)| **l == layer)
                .map(|(c, _)| c.clone())
                .collect();
            surfaces.push(skin(a.kernel(brep)?, &curves, None)?);
            layer_curves.push(curves);
        }
        // Not upstream: sections on parallel planes whose chord-length skin leaves the slab
        // between the first and last planes are skinned at their planes' distances, every
        // layer alike (`SectionPlanes`).
        if let Some(planes) = SectionPlanes::of(&*a, faces)? {
            let tol = a.brep(brep)?.tolerance();
            let mut leaves = false;
            for (s, _) in &surfaces {
                leaves = leaves || planes.leaves(s, tol)?;
            }
            if leaves {
                surfaces.clear();
                for curves in &layer_curves {
                    surfaces.push(skin(a.kernel(brep)?, curves, Some(planes.params()))?);
                }
            }
        }
        let mut traced: Vec<(FaceId, Vec<usize>)> = Vec::new();
        let mut made = Vec::new();
        for (layer, (surface, maps)) in surfaces.into_iter().enumerate() {
            // (the trace: the surface's knots across the sections, V)
            let surface_knots = trace
                .is_some()
                .then(|| surface.knots(SurfParamType::V, None).map(|(k, _)| k));
            let surface = Surface::from(surface);
            // (a B-spline surface just made: never lazy)
            let domain = surface.natural_uv_domain_if_built();
            // fork c8f6a31 (WR-662): SER(CreateFacesFromSurface(..)).
            let p = a.kernel(brep)?.fixes.sweep_statuses_pass_on;
            ignore_status(passed_on(
                a.surface_faces(brep, surface, domain, &mut made),
                p,
            )?)?;
            new_faces.extend_from_slice(&made);
            if trace.is_some() && untraced.is_none() {
                let sections: Vec<&LoopSpans> = loop_spans
                    .iter()
                    .filter(|(l, _)| *l == layer)
                    .map(|(_, s)| s)
                    .collect();
                let found = match (surface_knots, face_v_intervals(&*a, brep, &made)) {
                    (Some(Ok(knots)), Some(intervals)) => {
                        let edge_curves = trace
                            .as_deref()
                            .map_or(&[][..], |t| t.edge_curves.as_slice());
                        let maps = maps.as_deref().unwrap_or(&[]);
                        layer_sources(edge_curves, &sections, maps, &knots, &intervals)
                    }
                    (Some(Err(_)) | None, _) => Err(NotTraced::SurfaceKnotsUnreadable),
                    (_, None) => Err(NotTraced::FaceDomainUnreadable),
                };
                match found {
                    Ok(mut layer_faces) => traced.append(&mut layer_faces),
                    Err(why) => untraced = Some(why),
                }
            }
        }
        if let Some(t) = trace {
            t.sources = Some(match untraced {
                Some(why) => SkinSources::NotTraced { why },
                None => SkinSources::Traced(traced),
            });
        }
        Ok(())
    }

    /// Construct a blend and add its face, preserving the ignored face status.
    pub fn create_blend_primitive(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        first: Option<EdgeuseId>,
        second: Option<EdgeuseId>,
        output: &mut Option<FaceId>,
        options: BlendPrimitiveOptions,
    ) -> Result<()> {
        self.init_entity_maps();
        let _ = target(self, brep)?;
        let flag = match options.curvature {
            CurvatureType::NoCurvature => 1,
            CurvatureType::G3FromSurface => 3,
            _ => 2,
        };
        let mut derivatives = DerivSurfDefinition {
            curvature_type: options.curvature,
            blend_end_type: options.ends,
            start_tan_length: options.start_tangent_length,
            end_tan_scale: options.end_tangent_length / options.start_tangent_length,
            ..Default::default()
        };
        let surface = a.with_primitive_brep(brep, |kernel, model| {
            crate::topology::brep::p1::create_edge_edge_blend(
                kernel,
                flag,
                model,
                first,
                None, // Both edge uses belong to this primitive's B-rep.
                second,
                options.curves_direction,
                Some(&mut derivatives),
            )
        })?;
        a.set_editing(brep, true)?;
        // (a B-spline surface just made: never lazy)
        let domain = surface.natural_uv_domain_if_built();
        // fork c8f6a31 (WR-662): SER(pBrep->CreateFaceFromSurface(..)).
        let p = a.kernel(brep)?.fixes.sweep_statuses_pass_on;
        ignore_status(passed_on(a.surface_face(brep, surface, domain, output), p)?)?;
        Ok(())
    }

    /// Sweep copies of the profiles along the path, optionally scaling and capping.
    /// Only `side_faces` is reset; cap output slots retain their upstream lifetime.
    /// The scale reference retains its derived curve dispatch; path and scale use
    /// B-spline copies, as in the upstream constructor.
    pub fn create_curve_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        profiles: &[Option<Curve>],
        path: &BSplineCurve,
        scale_reference: Option<&Curve>,
        scale_curve: Option<&BSplineCurve>,
        approx_tol: f64,
        options: SweepOptions,
        test_continuity: bool,
        start_faces: &mut Vec<FaceId>,
        side_faces: &mut Vec<FaceId>,
        end_faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        self.curve_sweep(
            a,
            brep,
            profiles,
            path,
            scale_reference,
            scale_curve,
            approx_tol,
            options,
            test_continuity,
            start_faces,
            side_faces,
            end_faces,
            None,
        )
    }

    /// [`create_curve_sweep`](Self::create_curve_sweep) that also reports where it
    /// placed the profile (the SolidCraft fork's `pOptPlacements`, tangency-labs/smlib
    /// 5d3939e, WR-523, #1450): the rigid motions carrying the profile curves as given
    /// onto the sweep's first, middle and last sections (of the first composite; all
    /// share the path and its start frame). The B-rep and faces are the same as
    /// [`create_curve_sweep`](Self::create_curve_sweep)'s.
    ///
    /// # Errors
    ///
    /// `InvalidInput` with a scale curve (its sections are no rigid images of the
    /// profile); otherwise those of [`create_curve_sweep`](Self::create_curve_sweep).
    pub fn create_curve_sweep_with_placements(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        profiles: &[Option<Curve>],
        path: &BSplineCurve,
        scale_reference: Option<&Curve>,
        scale_curve: Option<&BSplineCurve>,
        approx_tol: f64,
        options: SweepOptions,
        test_continuity: bool,
        start_faces: &mut Vec<FaceId>,
        side_faces: &mut Vec<FaceId>,
        end_faces: &mut Vec<FaceId>,
    ) -> Result<[Axis2Placement; 3]> {
        let mut placements = [Axis2Placement::IDENTITY; 3];
        self.curve_sweep(
            a,
            brep,
            profiles,
            path,
            scale_reference,
            scale_curve,
            approx_tol,
            options,
            test_continuity,
            start_faces,
            side_faces,
            end_faces,
            Some(&mut placements),
        )?;
        Ok(placements)
    }

    /// `CreateCurveSweep` with the fork's optional `pOptPlacements` output.
    fn curve_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        profiles: &[Option<Curve>],
        path: &BSplineCurve,
        scale_reference: Option<&Curve>,
        scale_curve: Option<&BSplineCurve>,
        approx_tol: f64,
        options: SweepOptions,
        test_continuity: bool,
        start_faces: &mut Vec<FaceId>,
        side_faces: &mut Vec<FaceId>,
        end_faces: &mut Vec<FaceId>,
        mut opt_placements: Option<&mut [Axis2Placement; 3]>,
    ) -> Result<()> {
        self.init_entity_maps();
        side_faces.clear();
        if profiles.is_empty() {
            return Err(SolidError::primitive_failed(OP, "no profiles to sweep"));
        }
        // SolidCraft fork (5d3939e): a scaled sweep's sections are no rigid images of the
        // profile.
        if opt_placements.is_some() && scale_curve.is_some() {
            return Err(SolidError::invalid_input(
                OP,
                "no placements of a scaled sweep",
            ));
        }
        let region = target(self, brep)?;
        let previous = a.kernel(brep)?.context.doing_boolean();
        a.kernel_mut(brep)?.context.set_doing_boolean(true);
        let result = (|| {
            let mut prepared = prepare_curve_sweep(
                a.kernel(brep)?,
                profiles,
                path,
                scale_reference,
                scale_curve,
                options,
                test_continuity,
            )?;
            let curves = core::mem::take(&mut prepared.profiles)
                .into_iter()
                .map(Curve::from)
                .collect();
            let composites = CompositeCurve::build_composites_from_curves(
                a.kernel(brep)?,
                curves,
                a.brep(brep)?.tolerance(),
                0.0,
            )?;
            // SolidCraft fork (5d3939e): where the sweep places the profile, in the space
            // it sweeps in (the same for every composite: they share the path and its
            // start frame).
            let mut placed = [Axis2Placement::IDENTITY; 3];
            // each composite's side faces (WR-404's caps)
            let mut groups = Vec::new();
            for (index, mut composite) in composites.composites.into_iter().enumerate() {
                let profile = composite.make_composite_nurb()?;
                let want_placements = index == 0 && opt_placements.is_some();
                let (net, trace) = BSplineSurface::create_sweep_surface_traced(
                    &profile,
                    &prepared.path,
                    prepared.scale.as_ref(),
                    options.translational_sweep,
                    approx_tol,
                    want_placements.then_some(&mut placed),
                )?;
                // not upstream (#2572): a path sweep without a scale keeps its definition. A path of
                // lines and circular arcs has its net built from the definition itself (exact to
                // rounding, its bound within the kernel's tolerance); any other keeps the fit and,
                // where the definition certifies it, the net's certified distance from it (the
                // error is the reason it has none, and the ledger keeps the kinds of wall that do not)
                let exact = match (&trace, prepared.scale.is_none()) {
                    (Some(trace), true) => ProceduralSweep::create_path_exact(
                        &profile,
                        &prepared.path,
                        trace.start_normal,
                    )
                    .ok()
                    .filter(|(exact, _)| exact.bound() <= ZONE_TOL_3D),
                    _ => None,
                };
                let (net, definition) = if let Some((exact, built)) = exact {
                    (built, Some(exact))
                } else {
                    let fitted = match (&trace, prepared.scale.is_none()) {
                        (Some(trace), true) => {
                            ProceduralSweep::create_path(&profile, &prepared.path, trace, &net).ok()
                        }
                        _ => None,
                    };
                    (net, fitted)
                };
                let mut surface = Surface::from(net);
                if let Some(definition) = definition {
                    surface.set_sweep_definition(definition);
                }
                // (a B-spline surface just made: never lazy)
                let domain = surface.natural_uv_domain_if_built();
                let mut made = Vec::new();
                ignore_status(a.surface_faces(brep, surface, domain, &mut made))?;
                let first = side_faces.len();
                side_faces.extend(made);
                groups.push(first..side_faces.len());
            }
            // fork c8f6a31 (WR-662): SER(pBrep->Transform(..)) for both moves back.
            let p = a.kernel(brep)?.fixes.sweep_statuses_pass_on;
            ignore_status(passed_on(
                a.transform_primitive(brep, &prepared.rotation_back),
                p,
            )?)?;
            let placement = if options.move_to_profile {
                &prepared.profile_back
            } else {
                &prepared.path_back
            };
            ignore_status(passed_on(a.transform_primitive(brep, placement), p)?)?;
            // SolidCraft fork (5d3939e): the motions carrying the profile as given onto
            // its first, middle and last sections. A profile point went forward into the
            // sweep's space (to the origin, the path turned onto x, oriented with the path
            // when asked), the sweep placed it, and the result came back: each motion is
            // that chain, applied to the global system.
            if let Some(out) = opt_placements.as_deref_mut() {
                *out = profile_motions(&prepared, placement, &placed);
            }
            // not upstream (#2192, opt-in): a closed path whose end sections lie on its
            // start sections gets no caps; the stitch joins the ends instead
            let cap_ends = options.cap_ends
                && !(a.kernel(brep)?.fixes.curve_sweep_closed_path_joins_ends
                    && sweep_ends_coincide(&*a, brep, side_faces, options.translational_sweep)?);
            if cap_ends {
                // not upstream (WR-404, #411)
                let (mut starts, mut ends) = sweep_end_curves(
                    &*a,
                    brep,
                    side_faces,
                    &groups,
                    options.translational_sweep,
                    approx_tol,
                )?;
                a.planar_faces(region, &mut starts, approx_tol, start_faces)?;
                a.planar_faces(region, &mut ends, approx_tol, end_faces)?;
            }
            // fork c8f6a31 (WR-662): SER(pBrep->StitchAndOrient()).
            let p = a.kernel(brep)?.fixes.sweep_statuses_pass_on;
            ignore_status(passed_on(a.stitch_and_orient(brep), p)?)?;
            Ok(())
        })();
        a.kernel_mut(brep)?.context.set_doing_boolean(previous);
        result
    }
}

/// The rigid motions carrying the profile as given onto the sweep's sections placed
/// on `placed` (in the sweep's space), with `back_move` the move back from the path
/// (the SolidCraft fork's `pOptPlacements` of `CreateCurveSweep`, tangency-labs/smlib
/// 5d3939e): the global system carried forward (profile to the origin, path turned onto
/// x, oriented when asked), placed, and carried back.
fn profile_motions(
    prepared: &PreparedCurveSweep,
    back_move: &Axis2Placement,
    placed: &[Axis2Placement; 3],
) -> [Axis2Placement; 3] {
    let forward = |q: Vec3| {
        let a = prepared.profile_to_origin.transform_point(q);
        let b = prepared.rotation.transform_point(a);
        match &prepared.orientation {
            Some(orientation) => orientation.transform_point(b),
            None => b,
        }
    };
    let back = |q: Vec3| {
        let a = prepared.rotation_back.transform_point(q);
        back_move.transform_point(a)
    };
    let turned = |at: &Axis2Placement, v: Vec3| {
        let a = prepared.rotation.transform_vector(v);
        let b = match &prepared.orientation {
            Some(orientation) => orientation.transform_vector(a),
            None => a,
        };
        let c = at.transform_vector(b);
        let d = prepared.rotation_back.transform_vector(c);
        back_move.transform_vector(d)
    };
    placed.map(|at| {
        let origin = at.transform_point(forward(Vec3::ZERO));
        Axis2Placement::new(back(origin), turned(&at, Vec3::X), turned(&at, Vec3::Y))
    })
}

/// The 3D curves of one cap's loops, as `planar_faces` takes them.
type CapCurves = Vec<Option<Curve>>;

/// The cap curves of a curve sweep (not upstream, WR-404, #411): the iso curves at the
/// sweep's start and end of each composite's side faces that reach them. A path that
/// is not G1 splits every swept surface into one face per piece, so upstream's first
/// and last `count` side faces are not the faces at the ends.
fn sweep_end_curves(
    a: &impl PrimitiveAccess,
    brep: BrepId,
    side_faces: &[FaceId],
    groups: &[core::ops::Range<usize>],
    translational: bool,
    approx_tol: f64,
) -> Result<(CapCurves, CapCurves)> {
    // the sweep runs along v for a translational sweep, else along u
    let direction = if translational {
        SurfParamType::V
    } else {
        SurfParamType::U
    };
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for group in groups {
        let mut spans = Vec::new();
        for &face in side_faces.get(group.clone()).unwrap_or_default() {
            let domain =
                face_surface(a, (brep, face))?.natural_uv_domain_with_nurb(a.kernel(brep)?)?;
            let span = if translational {
                (domain.min().y, domain.max().y)
            } else {
                (domain.min().x, domain.max().x)
            };
            spans.push((face, span));
        }
        let first = spans.iter().map(|(_, s)| s.0).fold(f64::INFINITY, f64::min);
        let last = spans
            .iter()
            .map(|(_, s)| s.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let tol = EFF_ZERO_SQRT * (1.0 + first.abs().max(last.abs()));
        for &(face, (low, high)) in &spans {
            let surface = face_surface(a, (brep, face))?;
            for (at, keep, out) in [
                (low, low - first <= tol, &mut starts),
                (high, last - high <= tol, &mut ends),
            ] {
                if keep {
                    let curve = surface
                        .create_iso_parametric_curve(
                            a.kernel(brep)?,
                            direction,
                            at,
                            approx_tol / 10.0,
                            None,
                            false,
                        )?
                        .curve;
                    out.push(Some(curve));
                }
            }
        }
    }
    Ok((starts, ends))
}

/// Whether every side face of a curve sweep spans the whole sweep and its section at the
/// sweep's end lies on its section at the start, within the B-rep's tolerance: the path
/// closes on itself and the profile comes back where it started (tangency-labs/solidcore#2192).
/// The sweep runs along v for a translational sweep, else along u.
fn sweep_ends_coincide(
    a: &impl PrimitiveAccess,
    brep: BrepId,
    side_faces: &[FaceId],
    translational: bool,
) -> Result<bool> {
    // sections compared at this many intervals across each face
    const SAMPLES: u32 = 16;
    let k = a.kernel(brep)?;
    let tol = a.brep(brep)?.tolerance();
    let mut faces = Vec::new();
    for &face in side_faces {
        let surface = face_surface(a, (brep, face))?;
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let (along, across) = if translational {
            (
                (domain.min().y, domain.max().y),
                (domain.min().x, domain.max().x),
            )
        } else {
            (
                (domain.min().x, domain.max().x),
                (domain.min().y, domain.max().y),
            )
        };
        faces.push((surface, along, across));
    }
    if faces.is_empty() {
        return Ok(false);
    }
    let first = faces.iter().map(|f| f.1 .0).fold(f64::INFINITY, f64::min);
    let last = faces
        .iter()
        .map(|f| f.1 .1)
        .fold(f64::NEG_INFINITY, f64::max);
    let span_tol = EFF_ZERO_SQRT * (1.0 + first.abs().max(last.abs()));
    for (surface, (low, high), (from, to)) in faces {
        if low - first > span_tol || last - high > span_tol {
            return Ok(false);
        }
        for i in 0..=SAMPLES {
            let s = from + (to - from) * (f64::from(i) / f64::from(SAMPLES));
            let (start, end) = if translational {
                (Point2::new(s, low), Point2::new(s, high))
            } else {
                (Point2::new(low, s), Point2::new(high, s))
            };
            let gap = surface
                .evaluate_point(k, start)?
                .distance(surface.evaluate_point(k, end)?);
            if gap.is_nan() || gap > tol {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Each of `faces`' V interval: its surface's natural domain across the sections
/// (`None` when one cannot be read; the trace then reports it, the skin is unchanged).
fn face_v_intervals(
    a: &impl PrimitiveAccess,
    brep: BrepId,
    faces: &[FaceId],
) -> Option<Vec<(FaceId, f64, f64)>> {
    let k = a.kernel(brep).ok()?;
    let b = a.brep(brep).ok()?;
    let mut out = Vec::new();
    for &f in faces {
        let surface = crate::topology::face::p2::surface(b, f).ok()??;
        let domain = surface.natural_uv_domain_with_nurb(k).ok()?;
        out.push((f, domain.min().y, domain.max().y));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::extent1d::Extent1d;
    use crate::kernel::Kernel;

    // An imported line may have last-bit differences between its analytic data
    // and cached NURB. Virtual evaluations must continue to use the analytic data.
    fn imported_line(point: Vec3, vector: Vec3, scale: f64) -> Curve {
        let cached_point = Vec3::new(f64::from_bits(point.x.to_bits() + 2), point.y, point.z);
        let base =
            BSplineCurve::create_line_segment(3, cached_point, cached_point + scale * vector)
                .unwrap();
        Curve::from(Line::from_parts(
            base,
            point,
            vector,
            Extent1d::from_min_max(0.0, 1.0),
            scale,
            false,
        ))
    }

    #[test]
    fn swung_builder_materializes_lazy_line_nurbs() -> Result<()> {
        let kernel = Kernel::default();
        let p0 = Vec3::new(1.0, 0.0, 0.0);
        let p1 = Vec3::new(1.0, 0.0, 1.0);
        let t0 = Vec3::new(2.0, -1.0, 0.0);
        let t1 = Vec3::new(2.0, 1.0, 0.0);
        let profile = Line::create_line_segment(3, p0, p1, None)?;
        let path = Line::create_line_segment(3, t0, t1, None)?;
        let expected = Surface::from(BSplineSurface::create_swung_surface(
            profile.base(),
            path.base(),
            0.5,
        )?);
        let lazy_profile = Line::from_points(p0, p1, 3);
        let lazy_path = Line::from_points(t0, t1, 3);
        assert!(lazy_profile.base().gw_nurb().is_none());
        assert!(lazy_path.base().gw_nurb().is_none());
        // Each input must reach its own virtual MakeNurb, including when both
        // caches are empty. The old base copies fail before building a surface.
        for (profile, path) in [
            (lazy_profile.clone(), path.clone()),
            (profile, lazy_path.clone()),
            (lazy_profile, lazy_path),
        ] {
            let surface = Surface::from(swung_surface(
                &kernel,
                &Curve::from(path),
                &Curve::from(profile),
                0.5,
            )?);
            let domain = surface.natural_uv_domain_if_built();
            assert_eq!(domain, expected.natural_uv_domain_if_built());
            for i in 0..=4 {
                for j in 0..=4 {
                    let uv = domain.evaluate(f64::from(i) / 4.0, f64::from(j) / 4.0);
                    assert_eq!(
                        surface.evaluate_point(&Kernel::default(), uv)?,
                        expected.evaluate_point(&Kernel::default(), uv)?,
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn swung_builder_keeps_an_existing_cached_nurb() -> Result<()> {
        let kernel = Kernel::default();
        let profile = imported_line(Vec3::new(2.0, 0.0, 0.0), Vec3::Z, 1.0);
        let path = Curve::from(Line::from_points(Vec3::X, Vec3::X + Vec3::Y, 3));
        let surface = Surface::from(swung_surface(&kernel, &path, &profile, 1.0)?);
        let uv = surface.natural_uv_domain_if_built().min();
        let point = surface.evaluate_point(&kernel, uv)?;
        // Surface construction uses the existing NURB, even though scale
        // initialization above must use the original analytic evaluations.
        assert_eq!(point.x, f64::from_bits(2.0_f64.to_bits() + 2));
        assert_ne!(point.x, 2.0);
        Ok(())
    }

    #[test]
    fn swung_scale_keeps_analytic_profile_when_no_plane_intersection_exists() {
        let kernel = Kernel::default();
        let path = Curve::from(BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::X).unwrap());
        let profile = imported_line(Vec3::new(2.0, 2.0, 0.0), Vec3::Z, 1.0);
        assert_eq!(
            swung_curve_scale(&kernel, &path, &profile, 1e-5).unwrap(),
            0.5
        );
        let sliced = Curve::from(super::super::geometry::bspline(&profile).unwrap());
        assert_ne!(
            swung_curve_scale(&kernel, &path, &sliced, 1e-5).unwrap(),
            0.5
        );
    }

    #[test]
    fn swung_scale_keeps_analytic_path_after_plane_intersection() {
        let kernel = Kernel::default();
        let path = imported_line(Vec3::new(2.0, -1.0, 0.0), Vec3::Y, 2.0);
        let profile =
            Curve::from(BSplineCurve::create_line_segment(3, Vec3::X, Vec3::X + Vec3::Z).unwrap());
        assert_eq!(
            swung_curve_scale(&kernel, &path, &profile, 1e-5).unwrap(),
            0.5
        );
        let sliced = Curve::from(super::super::geometry::bspline(&path).unwrap());
        assert_ne!(
            swung_curve_scale(&kernel, &sliced, &profile, 1e-5).unwrap(),
            0.5
        );
    }

    /// An annulus swept along a line and a tangent quarter arc gets its two caps
    /// with the opt-in: each wall is two faces, one per path piece (WR-404).
    #[test]
    fn issue_411_holed_profile_sweeps_along_a_curved_path() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};
        use crate::nurbs::b_spline_curve::p1::NurbCircleParam;
        use crate::primitive::api_curves::create_circle;
        use crate::sweep::api_brep::create_curve_sweep;

        let o = Vec3::new(0.0, 0.0, 0.0);
        let line = BSplineCurve::create_line_segment(3, o, Vec3::new(0.0, 0.0, 10.0)).unwrap();
        let arc = BSplineCurve::create_arc_from_points(
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(10.0, 0.0, 20.0),
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let k0 = Kernel::default();
        let mut built = CompositeCurve::build_composites_from_curves(
            &k0,
            vec![Curve::from(line), Curve::from(arc)],
            1e-6,
            0.0,
        )
        .unwrap();
        let path = Curve::from(built.composites[0].make_composite_nurb().unwrap());
        let outer = Curve::from(create_circle(o, 2.0).unwrap());
        let inner = Curve::from(create_circle(o, 1.0).unwrap());
        let options = SweepOptions {
            cap_ends: true,
            ..SweepOptions::default()
        };

        let store = AttributeStore::default();
        let key_of = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let mut k = Kernel::default();
        let sweep = |k: &mut Kernel, profile: &[Option<&Curve>]| {
            let swept =
                create_curve_sweep(k, profile, Some(&path), None, None, Some(&options)).unwrap();
            assert_eq!(swept.start_faces.len(), 1);
            assert_eq!(swept.end_faces.len(), 1);
            assert!(crate::topology::brep::p2::is_manifold_solid(&swept.brep).unwrap());
            swept
        };
        let mut outer_solid = sweep(&mut k, &[Some(&outer)]).brep;
        let solid_outer = brep_volume(&k, &mut outer_solid, 1e-6, &attributes).unwrap();

        let mut pipe = sweep(&mut k, &[Some(&outer), Some(&inner)]);
        // each cap is the annulus: the outer loop and the hole
        for cap in [pipe.start_faces[0], pipe.end_faces[0]] {
            let loops = crate::topology::face::p2::loops(&pipe.brep, cap, false, None).unwrap();
            assert_eq!(loops.len(), 2);
        }
        // (the precise integration is not trusted to 1e-3 across this path's C1 joint,
        // tangency-labs/solidcore#384, so only the hole's removal is checked)
        let volume = brep_volume(&k, &mut pipe.brep, 1e-6, &attributes).unwrap();
        assert!(
            volume > 0.0 && volume < 0.9 * solid_outer,
            "volume {volume}, outer {solid_outer}"
        );
    }
}
