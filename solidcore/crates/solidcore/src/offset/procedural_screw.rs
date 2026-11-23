//! Intersections of screw-invariant walls reduce to two-dimensional section roots.
//!
//! A root generates one helix. Its first pcurve is a constant-profile line; the
//! second is the exact fractional-linear change between rational arc parameters.
//! No spatial intersection trace or sampled root certificate is used.
//! Deviations: new construction. Root subdivision is iterative, at most 16,384
//! boxes and depth 48; windings and pcurve spans have fixed resource budgets.

mod certificate;
pub(crate) mod corner;
mod cylinder;
mod groove;
pub(super) mod seam;

pub(crate) use groove::{operation_reach, ThreadReach};

use std::f64::consts::TAU;

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::procedural_sweep::screw_offset::{
    IntersectionIsolation, ScrewOffsetSection, SectionIntersection,
};
use crate::surface::procedural_sweep::ProceduralSweep;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use crate::surface::surface::Surface;

const OP: &str = "offset::procedural_screw::intersect";
const MAX_BOXES: usize = 16_384;
const MAX_DEPTH: usize = 48;
const MAX_WINDINGS: i64 = 12_000;

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

struct Wall<'a> {
    surface: &'a Surface,
    definition: &'a ProceduralSweep,
    section: ScrewOffsetSection,
    source: [f64; 2],
    profile: Ivl,
    angle: Ivl,
    face_domain: Extent2d,
}

impl<'a> Wall<'a> {
    fn new(surface: &'a Surface, domain: &Extent2d) -> Result<Option<Self>> {
        let Some(sweep) = surface.procedural_sweep() else {
            return Ok(None);
        };
        let definition = sweep.definition();
        let Some((section, source)) = definition.screw_offset_section()? else {
            return Ok(None);
        };
        // Reject unsupported exact source controls before root isolation can
        // return emptiness; a partial runtime certificate must not authorize it.
        definition.runtime_evaluation_allowance()?;
        let (a, b, swapped) = definition.definition_map();
        let (u, v) = if swapped {
            (domain.v_interval(), domain.u_interval())
        } else {
            (domain.u_interval(), domain.v_interval())
        };
        let affine = |range: Ivl, a: f64, b: f64| {
            if a == 0.0 && b == 1.0 {
                range
            } else {
                Ivl::from(a) + Ivl::from(b) * range
            }
        };
        let w = affine(Ivl::new(u.min(), u.max()), a[0], b[0]);
        let t = affine(Ivl::new(v.min(), v.max()), a[1], b[1]);
        let support = definition.profile().natural_interval();
        if !support.min().is_finite()
            || !support.max().is_finite()
            || support.min() >= support.max()
        {
            return Err(bad("invalid screw profile support"));
        }
        let w = Ivl::new(
            crate::math::clamp_between(w.lo, support.min(), support.max()),
            crate::math::clamp_between(w.hi, support.min(), support.max()),
        );
        let [lo, hi] = source;
        let profile = (w - Ivl::from(lo)) / (Ivl::from(hi) - Ivl::from(lo));
        let angle = definition.mathematical_turn_range(t)?;
        if ![profile.lo, profile.hi, angle.lo, angle.hi]
            .iter()
            .all(|v| v.is_finite())
            || profile.lo >= profile.hi
            || angle.lo >= angle.hi
        {
            return Err(bad("invalid screw intersection domain"));
        }
        Ok(Some(Self {
            surface,
            definition,
            section,
            source,
            profile,
            angle,
            face_domain: *domain,
        }))
    }

    fn profile_parameter(&self, x: f64) -> f64 {
        let [lo, hi] = self.source;
        lo + (hi - lo) * x
    }
}

/// Whether mating this exact pair requires the complete section construction.
/// Classification is independent of the requested tolerance and trim domain;
/// failures after this boundary must not leave an original edge in the result.
pub(crate) fn requires_complete_mate(first: &Surface, second: &Surface) -> Result<bool> {
    use crate::surface::surface::SurfaceKind;
    if matches!(second.kind(), SurfaceKind::Cone(_)) {
        return cylinder::eligible(first, second);
    }
    if matches!(first.kind(), SurfaceKind::Cone(_)) {
        return cylinder::eligible(second, first);
    }
    let (Some(sa), Some(sb)) = (first.procedural_sweep(), second.procedural_sweep()) else {
        return Ok(false);
    };
    let (a, b) = (sa.definition(), sb.definition());
    if (!a.is_normal_offset() && !b.is_normal_offset())
        || !a.is_screw_invariant()
        || !b.is_screw_invariant()
    {
        return Ok(false);
    }
    let (Some(ma), Some(mb)) = (a.screw(), b.screw()) else {
        return Ok(false);
    };
    Ok(ma.origin() == mb.origin()
        && ma.axis() == mb.axis()
        && ma.pitch() == mb.pitch()
        && a.screw_offset_section()?.is_some()
        && b.screw_offset_section()?.is_some())
}

/// Returns `None` only for a different geometric class, before any construction.
/// Once classified, a failed certificate is an error, never a spatial-SSI retry.
pub(crate) fn intersect(
    k: &Kernel,
    first: &Surface,
    first_domain: &Extent2d,
    second: &Surface,
    second_domain: &Extent2d,
    tolerance: f64,
) -> Result<Option<SurfaceIntersection>> {
    if !requires_complete_mate(first, second)? {
        return Ok(None);
    }
    if first.procedural_sweep().is_some()
        && matches!(second.kind(), crate::surface::surface::SurfaceKind::Cone(_))
    {
        return cylinder::intersect(k, first, first_domain, second, second_domain, tolerance);
    }
    if second.procedural_sweep().is_some()
        && matches!(first.kind(), crate::surface::surface::SurfaceKind::Cone(_))
    {
        let mut result =
            cylinder::intersect(k, second, second_domain, first, first_domain, tolerance)?;
        if let Some(out) = &mut result {
            std::mem::swap(&mut out.surface1_uv_curves, &mut out.surface2_uv_curves);
        }
        return Ok(result);
    }
    let (Some(sa), Some(sb)) = (first.procedural_sweep(), second.procedural_sweep()) else {
        return Ok(None);
    };
    if !sa.definition().is_normal_offset() && !sb.definition().is_normal_offset() {
        return Ok(None);
    }
    if !sa.definition().is_screw_invariant() || !sb.definition().is_screw_invariant() {
        return Ok(None);
    }
    let (Some(ma), Some(mb)) = (sa.definition().screw(), sb.definition().screw()) else {
        return Ok(None);
    };
    if ma.origin() != mb.origin() || ma.axis() != mb.axis() || ma.pitch() != mb.pitch() {
        return Ok(None);
    }
    let (Some(a), Some(b)) = (
        Wall::new(first, first_domain)?,
        Wall::new(second, second_domain)?,
    ) else {
        return Ok(None);
    };
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(bad(
            "a screw intersection needs a positive finite tolerance",
        ));
    }
    let reference = a.section.evaluate(a.profile.mid())?.0 - ma.origin();
    let radial = reference - ma.axis() * reference.dot(ma.axis());
    if a.section == b.section {
        return coincident(k, &a, &b, radial, tolerance).map(Some);
    }
    let bounds_a = a
        .section
        .meridian_bounds(a.profile.lo, a.profile.hi, radial)?;
    let bounds_b = b
        .section
        .meridian_bounds(b.profile.lo, b.profile.hi, radial)?;
    let pitch = ma.pitch();
    let winding = if pitch == 0.0 {
        Ivl::from(0.0)
    } else {
        (bounds_a.height - bounds_b.height) / Ivl::from(pitch)
    };
    if !winding.lo.is_finite()
        || !winding.hi.is_finite()
        || winding.lo < -(MAX_WINDINGS as f64)
        || winding.hi > MAX_WINDINGS as f64
    {
        return Err(bad("screw intersection exceeds its winding budget"));
    }
    let mut out = SurfaceIntersection::default();
    let mut remaining = MAX_BOXES;
    for n in (winding.lo.ceil() as i64)..=(winding.hi.floor() as i64) {
        let shift = Ivl::from(pitch) * Ivl::from(n as f64);
        let roots = roots(
            [&a.section, &b.section],
            [a.profile, b.profile],
            radial,
            shift,
            tolerance / 64.0,
            &mut remaining,
        )?;
        for root in roots {
            let orbit = isolated_orbit(&a, &b, radial, n, root)?;
            append_helix(k, &a, &b, radial, orbit, tolerance, &mut out)?;
        }
    }
    Ok(Some(out))
}

#[derive(Clone, Copy)]
struct Orbit {
    parameters: [f64; 2],
    winding: i64,
    deviation: f64,
    kind: TsectCurveType,
}

fn isolated_orbit(
    a: &Wall<'_>,
    b: &Wall<'_>,
    radial: Vec3,
    winding: i64,
    root: SectionIntersection,
) -> Result<Orbit> {
    let box_a = a
        .section
        .meridian_bounds(root.first.lo, root.first.hi, radial)?;
    let box_b = b
        .section
        .meridian_bounds(root.second.lo, root.second.hi, radial)?;
    let dr = box_a.radius - box_b.radius;
    let dz = box_a.height
        - box_b.height
        - Ivl::from(winding as f64)
            * Ivl::from(
                a.definition
                    .screw()
                    .ok_or_else(|| bad("missing screw motion"))?
                    .pitch(),
            );
    let deviation = (Ivl::from(dr.mag()) * Ivl::from(dr.mag())
        + Ivl::from(dz.mag()) * Ivl::from(dz.mag()))
    .sqrt()
    .hi;
    Ok(Orbit {
        parameters: [root.first.mid(), root.second.mid()],
        winding,
        deviation,
        kind: TsectCurveType::Crossing,
    })
}

// Identical retained sections prove coincidence directly. Boundaries of the
// overlap rectangle are exact section curves and helices; no root is invented.
fn coincident(
    k: &Kernel,
    a: &Wall<'_>,
    b: &Wall<'_>,
    radial: Vec3,
    tolerance: f64,
) -> Result<SurfaceIntersection> {
    let lo = a.profile.lo.max(b.profile.lo);
    let hi = a.profile.hi.min(b.profile.hi);
    let first = a.angle.lo.max(b.angle.lo);
    let last = a.angle.hi.min(b.angle.hi);
    let mut out = SurfaceIntersection::default();
    if lo > hi || first > last {
        return Ok(out);
    }
    if first < last {
        for x in [lo, hi] {
            append_helix(
                k,
                a,
                b,
                radial,
                Orbit {
                    parameters: [x, x],
                    winding: 0,
                    deviation: 0.0,
                    kind: TsectCurveType::RegionBoundary,
                },
                tolerance,
                &mut out,
            )?;
            if lo == hi {
                break;
            }
        }
    }
    if lo < hi {
        for angle in [first, last] {
            let (first_uv, first_ideal) = section_pcurve(a, lo, hi, angle)?;
            let (second_uv, second_ideal) = section_pcurve(b, lo, hi, angle)?;
            let deviation = (Ivl::from(certificate::wall_error(a, &first_uv, &[first_ideal])?)
                + Ivl::from(certificate::wall_error(b, &second_uv, &[second_ideal])?))
            .hi;
            if !deviation.is_finite() || deviation > tolerance {
                return Err(bad(
                    "coincident section pcurves exceed the requested tolerance",
                ));
            }
            append_pair(
                k,
                a,
                first_uv,
                second_uv,
                TsectCurveType::RegionBoundary,
                deviation,
                &mut out,
            )?;
            if first == last {
                break;
            }
        }
    }
    Ok(out)
}

fn section_pcurve(
    wall: &Wall<'_>,
    from: f64,
    to: f64,
    angle: f64,
) -> Result<(Curve, certificate::Span)> {
    let t = wall.definition.parameter_at_angle(angle)?;
    let points = [from, to]
        .into_iter()
        .map(|x| {
            let p = wall
                .definition
                .face_parameters(Point2::new(wall.profile_parameter(x), t))?;
            Ok(Point3::new(p.x, p.y, 0.0))
        })
        .collect::<Result<Vec<_>>>()?;
    let exact_t = certificate::angle_parameter(wall, angle)?;
    let ideal = [from, to].map(|x| {
        let w = Ivl::from(wall.source[0])
            + (Ivl::from(wall.source[1]) - Ivl::from(wall.source[0])) * Ivl::from(x);
        certificate::point(w, exact_t, Ivl::from(1.0))
    });
    Ok((uv_curve(&points, None, &[0.0, 1.0], &[2, 2])?, ideal))
}

fn append_pair(
    k: &Kernel,
    a: &Wall<'_>,
    first_uv: Curve,
    second_uv: Curve,
    kind: TsectCurveType,
    deviation: f64,
    out: &mut SurfaceIntersection,
) -> Result<()> {
    let curve = Curve::from(CrvOnSurf::new(
        k,
        first_uv.copy(k)?,
        a.surface.copy(k)?,
        Some(a.face_domain),
        3,
    )?);
    out.curves_3d.push(curve);
    out.surface1_uv_curves.push(Some(first_uv));
    out.surface2_uv_curves.push(Some(second_uv));
    out.curve_types.push(kind);
    out.deviations.push(deviation);
    Ok(())
}

fn roots(
    sections: [&ScrewOffsetSection; 2],
    domains: [Ivl; 2],
    radial: Vec3,
    shift: Ivl,
    tolerance: f64,
    remaining: &mut usize,
) -> Result<Vec<SectionIntersection>> {
    let [a, b] = sections;
    let [first, second] = domains;
    let mut pending = vec![(first, second, 0_usize)];
    let mut result = Vec::new();
    while let Some((x, y, depth)) = pending.pop() {
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| bad("screw section root isolation exhausted its box budget"))?;
        match a.isolate_intersection_shifted(b, radial, x, y, shift, tolerance)? {
            IntersectionIsolation::NoRoot => {}
            IntersectionIsolation::Unique(root) => {
                let overlaps = |old: &&SectionIntersection| {
                    old.first.lo <= root.first.hi
                        && root.first.lo <= old.first.hi
                        && old.second.lo <= root.second.hi
                        && root.second.lo <= old.second.hi
                };
                if let Some(old) = result.iter().find(overlaps) {
                    let contains = |a: &SectionIntersection, b: &SectionIntersection| {
                        a.first.lo <= b.first.lo
                            && a.first.hi >= b.first.hi
                            && a.second.lo <= b.second.lo
                            && a.second.hi >= b.second.hi
                    };
                    // Existence in the contained box and uniqueness in its
                    // container prove identity. Mere overlap does not.
                    if !contains(old, &root) && !contains(&root, old) {
                        return Err(bad(
                            "overlapping section roots have no proved common identity",
                        ));
                    }
                } else {
                    result.push(root);
                }
            }
            IntersectionIsolation::Undecided { first, second } => {
                if depth >= MAX_DEPTH {
                    return Err(bad(
                        "a screw section is tangent, coincident or not isolated within the depth budget",
                    ));
                }
                if first.hi - first.lo >= second.hi - second.lo {
                    let m = first.mid();
                    if m <= first.lo || m >= first.hi {
                        return Err(bad("screw root interval cannot be subdivided"));
                    }
                    pending.push((Ivl::new(m, first.hi), second, depth + 1));
                    pending.push((Ivl::new(first.lo, m), second, depth + 1));
                } else {
                    let m = second.mid();
                    if m <= second.lo || m >= second.hi {
                        return Err(bad("screw root interval cannot be subdivided"));
                    }
                    pending.push((first, Ivl::new(m, second.hi), depth + 1));
                    pending.push((first, Ivl::new(second.lo, m), depth + 1));
                }
            }
        }
    }
    Ok(result)
}

fn uv_curve(
    points: &[Point3],
    weights: Option<&[f64]>,
    knots: &[f64],
    multiplicities: &[usize],
) -> Result<Curve> {
    // Adjacent rational pieces may have differently scaled homogeneous endpoints.
    // Match those scales before sharing the endpoint. Otherwise the canonical
    // constructor's removal of a full knot averages unequal weights and changes
    // the fractional-linear parameter map even though the Euclidean joins agree.
    let mut joined_points = Vec::new();
    let mut joined_weights = Vec::new();
    let mut joined_multiplicities = Vec::new();
    let join = knots.len() > 2 && multiplicities.iter().all(|&m| m == 2);
    if join {
        if points.len() != 2 * (knots.len() - 1) {
            return Err(bad("incompatible piecewise linear pcurve coefficients"));
        }
        for (i, pair) in points.chunks_exact(2).enumerate() {
            let [start, end] = *pair else { continue };
            let weight = |j: usize| -> Result<f64> {
                weights
                    .map_or(Some(1.0_f64), |w| w.get(j).copied())
                    .filter(|w| w.is_finite() && *w > 0.0)
                    .ok_or_else(|| bad("invalid paired pcurve weight"))
            };
            let wa = weight(2 * i)?;
            let wb = weight(2 * i + 1)?;
            let scale = joined_weights.last().copied().unwrap_or(wa) / wa;
            let end_weight = wb * scale;
            if !end_weight.is_finite() || end_weight <= 0.0 {
                return Err(bad("paired pcurve homogeneous scales overflowed"));
            }
            if i == 0 {
                joined_points.push(start);
                joined_weights.push(wa);
            }
            joined_points.push(end);
            joined_weights.push(end_weight);
        }
        joined_multiplicities = (0..knots.len())
            .map(|i| if i == 0 || i + 1 == knots.len() { 2 } else { 1 })
            .collect();
    }
    let (points, weights, multiplicities) = if join {
        (
            joined_points.as_slice(),
            Some(joined_weights.as_slice()),
            joined_multiplicities.as_slice(),
        )
    } else {
        (points, weights, multiplicities)
    };
    // The canonical import constructor coalesces knots within EFF_ZERO. Here
    // distinct geometric events must remain distinct, even beside a seam.
    // Construct the already clamped degree-one NURB directly; the certificate
    // below checks these actual coefficients, including homogeneous rounding.
    if knots.len() != multiplicities.len()
        || knots.len() < 2
        || knots.iter().any(|v| !v.is_finite())
        || knots.windows(2).any(|p| matches!(p, [a,b] if a >= b))
        || multiplicities.first() != Some(&2)
        || multiplicities.last() != Some(&2)
        || multiplicities.iter().any(|&m| m == 0 || m > 2)
        || weights.is_some_and(|w| w.len() != points.len())
    {
        return Err(bad("invalid exact pcurve knot data"));
    }
    let mut expanded = Vec::new();
    for (&knot, &count) in knots.iter().zip(multiplicities) {
        for _ in 0..count {
            expanded.push(knot);
        }
    }
    if points.len().checked_add(2) != Some(expanded.len()) {
        return Err(bad("exact pcurve controls do not match its knots"));
    }
    let mut homogeneous = Vec::new();
    for (i, p) in points.iter().enumerate() {
        let w = weights
            .map_or(Some(1.0), |w| w.get(i).copied())
            .filter(|w| w.is_finite() && *w > 0.0)
            .ok_or_else(|| bad("invalid exact pcurve weight"))?;
        let x = p.x * w;
        let y = p.y * w;
        if !x.is_finite() || !y.is_finite() {
            return Err(bad("exact pcurve homogeneous coefficient overflowed"));
        }
        homogeneous.push(crate::nurbs::math::CPoint::new(
            x,
            y,
            0.0,
            if weights.is_some() {
                w
            } else {
                crate::nurbs::math::NOW
            },
        ));
    }
    let raw = crate::nurbs::crv_basic::Curve::new(homogeneous, 1, expanded)?;
    let mut spline = BSplineCurve::new(2);
    spline.set_from_gw_nurb(Some(&raw))?;
    Ok(Curve::from(spline))
}

fn span(definition: &ProceduralSweep, angle: f64) -> Result<(f64, f64)> {
    let stations = definition.stations();
    let i = stations
        .partition_point(|&v| v <= angle)
        .saturating_sub(1)
        .min(stations.len().saturating_sub(2));
    match (stations.get(i), stations.get(i + 1)) {
        (Some(&a), Some(&b)) if a < b => Ok((a, b)),
        _ => Err(bad("missing screw arc span")),
    }
}

#[allow(clippy::too_many_arguments)]
fn append_helix(
    k: &Kernel,
    a: &Wall<'_>,
    b: &Wall<'_>,
    radial: Vec3,
    orbit: Orbit,
    tolerance: f64,
    out: &mut SurfaceIntersection,
) -> Result<()> {
    let [x, y] = orbit.parameters;
    let qa = a.section.meridian(x, radial)?;
    let qb = b.section.meridian(y, radial)?;
    let shift = qa.phase - qb.phase + orbit.winding as f64 * TAU;
    let lo = a.angle.lo.max(b.angle.lo - shift);
    let hi = a.angle.hi.min(b.angle.hi - shift);
    if lo >= hi {
        return Ok(());
    }
    let mut angles = vec![lo];
    angles.extend(
        a.definition
            .stations()
            .iter()
            .copied()
            .filter(|&v| v > lo && v < hi),
    );
    angles.extend(
        b.definition
            .stations()
            .iter()
            .map(|&v| v - shift)
            .filter(|&v| v > lo && v < hi),
    );
    angles.push(hi);
    angles.sort_by(f64::total_cmp);
    angles.dedup();
    if angles.len() > 24_002 {
        return Err(bad("screw helix exceeds its arc budget"));
    }
    let wa = a.profile_parameter(x);
    let wb = b.profile_parameter(y);
    let mut points = Vec::new();
    let mut weights = Vec::new();
    let mut knots = Vec::new();
    let mut ideal = Vec::new();
    let phase_a = a.section.meridian_bounds(x, x, radial)?.phase;
    let phase_b = b.section.meridian_bounds(y, y, radial)?.phase;
    let exact_shift = phase_a - phase_b + Ivl::from(orbit.winding as f64) * Ivl::from(TAU);
    let exact_wb =
        Ivl::from(b.source[0]) + (Ivl::from(b.source[1]) - Ivl::from(b.source[0])) * Ivl::from(y);
    for pair in angles.windows(2) {
        let [from, to] = *pair else { continue };
        let mid = from + (to - from) * 0.5;
        let (a0, a1) = span(a.definition, mid)?;
        let (b0, b1) = span(b.definition, mid + shift)?;
        let am = (a0 + a1) * 0.5;
        let bm = (b0 + b1) * 0.5;
        let at = ((a1 - a0) * 0.25).tan();
        let bt = ((b1 - b0) * 0.25).tan();
        let q = ((am + shift - bm) * 0.5).tan();
        let aw = Ivl::from(a1) - Ivl::from(a0);
        let bw = Ivl::from(b1) - Ivl::from(b0);
        let exact_am = (Ivl::from(a0) + Ivl::from(a1)) / Ivl::from(2.0);
        let exact_bm = (Ivl::from(b0) + Ivl::from(b1)) / Ivl::from(2.0);
        let exact_at = certificate::tan(aw / Ivl::from(4.0))?;
        let exact_bt = certificate::tan(bw / Ivl::from(4.0))?;
        let exact_q = certificate::tan((exact_am + exact_shift - exact_bm) / Ivl::from(2.0))?;
        let mut ideal_span = Vec::new();
        for angle in [from, to] {
            let t = a.definition.parameter_at_angle(angle)?;
            let z = at * (2.0 * (t - a0) / (a1 - a0) - 1.0);
            let denominator = 1.0 - q * z;
            let other_t = bm + (b1 - b0) / (2.0 * bt) * (z + q) / denominator;
            if !denominator.is_finite() || denominator <= 0.0 || !other_t.is_finite() {
                return Err(bad("singular screw arc parameter correspondence"));
            }
            let uv = b.definition.face_parameters(Point2::new(wb, other_t))?;
            points.push(Point3::new(uv.x, uv.y, 0.0));
            weights.push(denominator);
            let exact_z =
                exact_at * (Ivl::from(2.0) * (Ivl::from(t) - Ivl::from(a0)) / aw - Ivl::from(1.0));
            let exact_denominator = Ivl::from(1.0) - exact_q * exact_z;
            let exact_t = exact_bm
                + bw / (Ivl::from(2.0) * exact_bt) * (exact_z + exact_q) / exact_denominator;
            ideal_span.push(certificate::point(exact_wb, exact_t, exact_denominator));
            if angle == from {
                knots.push(t);
            }
        }
        let [start, end] = ideal_span.as_slice() else {
            return Err(bad("missing ideal helix span"));
        };
        ideal.push([*start, *end]);
    }
    let begin = a.definition.parameter_at_angle(lo)?;
    let end = a.definition.parameter_at_angle(hi)?;
    knots.push(end);
    let multiplicities = vec![2; knots.len()];
    let second_uv = uv_curve(&points, Some(&weights), &knots, &multiplicities)?;
    let first_points = [begin, end]
        .map(|t| a.definition.face_parameters(Point2::new(wa, t)))
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|p| Point3::new(p.x, p.y, 0.0))
        .collect::<Vec<_>>();
    let first_uv = uv_curve(&first_points, None, &[begin, end], &[2, 2])?;
    let exact_wa =
        Ivl::from(a.source[0]) + (Ivl::from(a.source[1]) - Ivl::from(a.source[0])) * Ivl::from(x);
    let first_ideal = [[
        certificate::point(exact_wa, Ivl::from(begin), Ivl::from(1.0)),
        certificate::point(exact_wa, Ivl::from(end), Ivl::from(1.0)),
    ]];
    let deviation = (Ivl::from(orbit.deviation)
        + Ivl::from(certificate::wall_error(a, &first_uv, &first_ideal)?)
        + Ivl::from(certificate::wall_error(b, &second_uv, &ideal)?))
    .hi;
    if !deviation.is_finite() || deviation > tolerance {
        return Err(bad(
            "the isolated screw helix does not meet the requested tolerance",
        ));
    }
    append_pair(k, a, first_uv, second_uv, orbit.kind, deviation, out)
}

/// Whether two screw motions are the same motion with no radial offset: equal origin, axis and
/// pitch, and each law absent or all zeros.
pub(in crate::offset) fn same_motion(
    a: &crate::surface::procedural_sweep::ScrewMotion,
    b: &crate::surface::procedural_sweep::ScrewMotion,
) -> bool {
    let zero = |m: &crate::surface::procedural_sweep::ScrewMotion| {
        m.law()
            .is_none_or(|law| !law.offsets().is_empty() && law.offsets().iter().all(|&x| x == 0.0))
    };
    a.origin() == b.origin() && a.axis() == b.axis() && a.pitch() == b.pitch() && zero(a) && zero(b)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn native_m20_candidate_body(segmented: bool) -> (Kernel, crate::topology::Brep) {
        use crate::boolean::api_brep::boolean_difference;
        use crate::geom::tol::{brep_approx_tol_3d, SmallTopoType};
        use crate::kernel::{KernelOptions, UpstreamFixes};
        use crate::primitive::{api_curves::create_line_segment, api_primitives};
        use crate::sweep::screw::screw_sweep;

        let mut k = Kernel::new(&KernelOptions::default());
        k.fixes = UpstreamFixes::all();
        let base = Point3::new(-1.0, -64.0, 0.0);
        let pitch = 2.5;
        let root = 10.0 - 5.0 * pitch * 3.0_f64.sqrt() / 16.0;
        let outer = 10.0 + 0.05 * pitch;
        let half_root = pitch / 8.0;
        let half_outer = half_root + (outer - root) / 3.0_f64.sqrt();
        let points = [
            Point3::new(base.x + root, base.y, -pitch - half_root),
            Point3::new(base.x + outer, base.y, -pitch - half_outer),
            Point3::new(base.x + outer, base.y, -pitch + half_outer),
            Point3::new(base.x + root, base.y, -pitch + half_root),
        ];
        let profile: Vec<_> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(&a, &b)| Curve::from(create_line_segment(a, b).unwrap()))
            .collect();
        let stock = api_primitives::create_cylinder(&mut k, base, 10.0, 20.0).unwrap();
        let tolerance = brep_approx_tol_3d(&k, &stock, SmallTopoType::NormalSize);
        let cutter = if segmented {
            let law: Vec<_> = (0..=5).map(|i| (2.0 * f64::from(i), 0.0)).collect();
            crate::sweep::screw::screw_sweep_radial(
                &mut k,
                &profile,
                base,
                Vec3::Z,
                pitch,
                10.0,
                &law,
                tolerance,
            )
            .unwrap()
        } else {
            screw_sweep(&mut k, &profile, base, Vec3::Z, pitch, 10.0, tolerance).unwrap()
        };
        let body = boolean_difference(&mut k, stock, cutter).unwrap();
        (k, body)
    }

    #[test]
    fn segmented_m20_family_reach_keeps_each_continuation_inside_its_budget() {
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        let (mut k, body) = native_m20_candidate_body(true);
        let ids: Vec<_> = body.faces().ids().collect();
        let tolerance = body.tolerance();
        let mut replacements = Vec::new();
        let mut flank_indices = Vec::new();
        for (index, &id) in ids.iter().enumerate() {
            let old = body.face(id).unwrap().surface.as_ref().unwrap();
            let new = if old.procedural_sweep().is_some() {
                flank_indices.push(index);
                let fu = crate::topology::face::p2::outer_faceuse(&body, id)
                    .unwrap()
                    .unwrap();
                let signed = if body.faceuse(fu).unwrap().orientation == OrientType::Opposite {
                    -0.1
                } else {
                    0.1
                };
                old.create_offset_surface(&k, signed, tolerance / 10.0)
                    .unwrap()
                    .unwrap()
            } else {
                old.copy_and_add_analytics(&k).unwrap()
            };
            replacements.push(new);
        }
        assert_eq!(flank_indices.len(), 10);
        // The groove certificate's budgeted searches stop when the operation is cancelled.
        let token = crate::kernel::CancelToken::default();
        k.set_cancel_token(token.clone());
        token.cancel();
        assert!(matches!(
            operation_reach(&k, &body, &ids, &replacements),
            Err(SolidError::Cancelled { .. })
        ));
        k.set_cancel_token(crate::kernel::CancelToken::default());
        let margin = operation_reach(&k, &body, &ids, &replacements)
            .unwrap()
            .expect("two complete compatible screw families");
        for &index in &flank_indices {
            let sweep = replacements[index].procedural_sweep().unwrap();
            let continued = sweep
                .definition()
                .extended_for_offset(sweep.net(), margin.reach, margin.turns, tolerance / 2.0)
                .unwrap();
            let continued = continued.procedural_sweep().unwrap();
            assert!(sweep
                .definition()
                .same_screw_parameterization(continued.definition()));
        }
        // Omitting an interior member leaves a genuine turn gap; it is not
        // filled by a hull of the remaining support ranges.
        let omitted = flank_indices[1];
        let incomplete_ids: Vec<_> = ids
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| (i != omitted).then_some(id))
            .collect();
        let incomplete: Vec<_> = replacements
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != omitted)
            .map(|(_, surface)| surface.clone())
            .collect();
        assert!(operation_reach(&k, &body, &incomplete_ids, &incomplete).is_err());
        let mut changed_chart = replacements.clone();
        changed_chart[flank_indices[0]]
            .reverse(&k, crate::nurbs::b_spline_surface::p1::SurfParamType::U)
            .unwrap();
        assert!(operation_reach(&k, &body, &ids, &changed_chart).is_err());
        let mut mixed_offsets = replacements.clone();
        let first = flank_indices[0];
        let old = body.face(ids[first]).unwrap().surface.as_ref().unwrap();
        let fu = crate::topology::face::p2::outer_faceuse(&body, ids[first])
            .unwrap()
            .unwrap();
        let signed = if body.faceuse(fu).unwrap().orientation == OrientType::Opposite {
            -0.2
        } else {
            0.2
        };
        mixed_offsets[first] = old
            .create_offset_surface(&k, signed, tolerance / 10.0)
            .unwrap()
            .unwrap();
        assert!(operation_reach(&k, &body, &ids, &mixed_offsets)
            .unwrap()
            .is_none());
    }

    #[test]
    fn native_m20_support_groups_are_disk_candidates_after_seam_cancellation() {
        use crate::surface::surface::SurfaceKind;
        use crate::topology::edgeuse::p1 as uses;
        use std::collections::BTreeMap;
        let (k, body) = native_m20_candidate_body(false);
        let mut edge_kinds = BTreeMap::new();
        for id in body.edges().ids() {
            let curve = body.edge(id).unwrap().curve.as_ref().unwrap();
            *edge_kinds.entry(curve.type_string()).or_insert(0_usize) += 1;
        }
        let mut walls = Vec::new();
        let mut cylinders: BTreeMap<u64, Vec<_>> = BTreeMap::new();
        let mut caps = Vec::new();
        for id in body.faces().ids() {
            let surface = body.face(id).unwrap().surface.as_ref().unwrap();
            if surface.procedural_sweep().is_some() {
                walls.push(id);
            } else {
                match surface.kind_with_nurb(&k).unwrap() {
                    SurfaceKind::Cone(c) => cylinders
                        .entry(c.canonical().radius.to_bits())
                        .or_default()
                        .push(id),
                    SurfaceKind::Plane(_) => caps.push(id),
                    _ => panic!("unexpected native M20 support"),
                }
            }
        }
        assert_eq!((walls.len(), cylinders.len(), caps.len()), (2, 2, 2));
        let cylinder_groups: Vec<_> = cylinders.values().collect();
        let mut labels = BTreeMap::new();
        for (label, group) in cylinder_groups.iter().enumerate() {
            for &face in *group {
                labels.insert(face, label);
            }
        }
        for (i, &face) in caps.iter().enumerate() {
            labels.insert(face, i + 2);
        }
        for &wall in &walls {
            let boundary = groove::cell::disk_boundary(&body, &[wall]).unwrap();
            let mut runs = Vec::new();
            for eu in boundary {
                let opposite = uses::radial(&body, eu).unwrap().unwrap();
                let fu = uses::faceuse(&body, opposite).unwrap().unwrap();
                let face = body.faceuse(fu).unwrap().face.unwrap();
                let label = labels[&face];
                if runs.last() != Some(&label) {
                    runs.push(label);
                }
            }
            if runs.first() == runs.last() {
                runs.pop();
            }
            assert_eq!(runs.len(), 4, "each flank has four support chains");
            assert_eq!(
                runs.iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                4
            );
            for pair in runs.iter().zip(runs.iter().cycle().skip(1)).take(4) {
                assert_ne!(
                    *pair.0 < 2,
                    *pair.1 < 2,
                    "cylinder and cap chains alternate"
                );
            }
        }
        for group in &cylinder_groups {
            let boundary = groove::cell::disk_boundary(&body, group).unwrap();
            assert!(!boundary.is_empty());
            let disconnected = vec![group[0], *group.last().unwrap()];
            assert!(groove::cell::disk_boundary(&body, &disconnected).is_err());
        }
        assert!(groove::cell::disk_boundary(&body, &walls).is_err());
        assert!(groove::cell::disk_boundary(&body, &[walls[0], walls[0]]).is_err());
    }

    #[test]
    fn a_tilted_cap_endpoint_can_move_farther_than_the_helix_height_shift() {
        // z=.03*x cuts the R=10, h=.4 helix transversely. Near phi=-pi/2,
        // changing G by .001 moves the cap endpoint by nearly .003 in z.
        // The old fixed-azimuth height-shift bound alone would miss this.
        let start = -std::f64::consts::FRAC_PI_2;
        let shift = 0.001;
        let mut phi = start;
        for _ in 0..12 {
            let residual = 0.4 * (phi - start) + shift - 0.3 * phi.cos();
            phi -= residual / (0.4 + 0.3 * phi.sin());
        }
        let axial = (0.3 * phi.cos()).abs();
        assert!(axial > 2.9 * shift);
        let bound = groove::cap_axial_motion(
            [Ivl::from(-0.03), Ivl::from(0.0), Ivl::from(1.0)],
            Vec3::Z,
            Ivl::from(0.4),
            Ivl::from(10.0),
            Ivl::from(shift),
            Ivl::from(0.0),
        )
        .unwrap();
        assert!(axial <= bound && bound < 3.01 * shift);
        assert!(
            groove::cap_axial_motion(
                [Ivl::from(-0.05), Ivl::from(0.0), Ivl::from(1.0)],
                Vec3::Z,
                Ivl::from(0.4),
                Ivl::from(10.0),
                Ivl::from(shift),
                Ivl::from(0.0),
            )
            .is_err(),
            "a plane that can be tangent to the orbit must be refused"
        );
    }

    /// One flank moves and the other stays (#11582): the kept flank's section starts on the
    /// root land's radius, so with only its own (zero) move to pad its search interval, that
    /// root sat on the interval's end and no endpoint sign proved it ("thread radial inverse
    /// chart is unresolved"). The operation's move pads every flank.
    #[test]
    fn a_kept_flank_beside_a_moved_one_proves_its_section_order() {
        use groove::{Affine, Band, Flank};
        let origin = Vec3::new(-1.0, -64.0, 0.0);
        let motion = ScrewMotion::new(origin, Vec3::Z, 2.5, None).unwrap();
        let lines = [
            [
                Vec3::new(7.646835306586814, -64.0, -2.8125),
                Vec3::new(9.125, -64.0, -3.6659187836487037),
            ],
            [
                Vec3::new(9.125, -64.0, -1.3340812163512963),
                Vec3::new(7.646835306586814, -64.0, -2.1875),
            ],
        ];
        for (moved, sign) in [(0, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)] {
            let mut index = 0;
            let flanks = lines.map(|[a, b]| {
                let rate = if index == moved { 0.1 * sign } else { 0.0 };
                index += 1;
                Flank {
                    section: ScrewOffsetSection::new(a, b, &motion, 0.0).unwrap(),
                    radius: Affine {
                        origin: Ivl::from(a.x) - Ivl::from(origin.x),
                        rate: Ivl::from(b.x) - Ivl::from(a.x),
                    },
                    distance: Affine {
                        origin: Ivl::from(0.0),
                        rate: Ivl::from(rate),
                    },
                    radial: Vec3::X,
                    cancel: None,
                }
            });
            let band = Band {
                flanks,
                radii: [8.646835306586814, 10.0].map(|r| Affine {
                    origin: Ivl::from(r),
                    rate: Ivl::from(0.0),
                }),
                pitch: 2.5,
                winding: 0,
                cancel: None,
            };
            let roots = band
                .certify()
                .unwrap_or_else(|e| panic!("flank {moved} moved {sign}: {e}"));
            assert!(roots
                .iter()
                .flatten()
                .all(|v| v.lo.is_finite() && v.hi.is_finite()));
        }
    }

    #[test]
    fn retained_m20_sections_prove_ordering_and_first_land_events() {
        use groove::{Affine, Band, Flank};
        let origin = Vec3::new(-1.0, -64.0, 0.0);
        let motion = ScrewMotion::new(origin, Vec3::Z, 2.5, None).unwrap();
        // Literal coefficients exported from the native M20 input, including
        // their unequal rounding. No mirror reconstruction enters the proof.
        let lines = [
            [
                Vec3::new(7.646835306586814, -64.0, -2.8125),
                Vec3::new(9.125, -64.0, -3.6659187836487037),
            ],
            [
                Vec3::new(9.125, -64.0, -1.3340812163512963),
                Vec3::new(7.646835306586814, -64.0, -2.1875),
            ],
        ];
        // Literal native cap placements, including the top cap's stored tilt.
        // Their parameter supports are [0,1]^2; no orthogonal reconstruction.
        let caps = [
            (
                Axis2Placement::from_stored(
                    Vec3::new(14.556349186104047, -79.55634918610404, 20.00000000000001),
                    Vec3::new(0.0, 1.0, -4.567541697989163e-16),
                    Vec3::new(-1.0, 0.0, -2.2837708489945816e-16),
                ),
                [31.112698372208094, 31.112698372208097],
            ),
            (
                Axis2Placement::from_stored(
                    Vec3::new(14.556349186104047, -79.55634918610404, 0.0),
                    -Vec3::X,
                    Vec3::Y,
                ),
                [31.112698372208097, 31.112698372208094],
            ),
        ];
        for (sign, cylinders_move, gap, end, reference) in [
            (1.0, false, 0, 4.0, 0.2704149008),
            (-1.0, false, 3, 2.0, 0.1352367446),
            (1.0, true, 0, 7.0, 0.5404828336),
            (-1.0, true, 3, 4.0, 0.2702986629),
        ] {
            let rate = 0.1 * sign;
            let flanks = lines.map(|[a, b]| Flank {
                section: ScrewOffsetSection::new(a, b, &motion, 0.0).unwrap(),
                radius: Affine {
                    origin: Ivl::from(a.x) - Ivl::from(origin.x),
                    rate: Ivl::from(b.x) - Ivl::from(a.x),
                },
                distance: Affine {
                    origin: Ivl::from(0.0),
                    rate: Ivl::from(rate),
                },
                radial: Vec3::X,
                cancel: None,
            });
            let mut band = Band {
                flanks,
                radii: [8.646835306586814, 10.0].map(|r| Affine {
                    origin: Ivl::from(r),
                    rate: Ivl::from(if cylinders_move { rate } else { 0.0 }),
                }),
                pitch: 2.5,
                winding: 0,
                cancel: None,
            };
            let roots = band
                .certify()
                .expect("the complete offset path to magnitude .1 stays ordered");
            assert!(roots
                .iter()
                .flatten()
                .all(|v| v.lo.is_finite() && v.hi.is_finite()));
            let bracket = band.event_bracket(gap, end).unwrap();
            let first = band.first_event(end).unwrap().expect("one boundary closes");
            assert!(first.lo <= bracket.hi && bracket.lo <= first.hi);
            assert!(band.first_event(1.0).unwrap().is_none());
            let distance = bracket * Ivl::from(0.1);
            assert!(distance.hi - distance.lo < 1e-8, "{distance:?}");
            assert!((distance.mid() - reference).abs() < 1e-8, "{distance:?}");
            // Extinction of one cylindrical land leaves a sharp helical
            // meeting of the two retained flanks inside the radial band.
            // It is not disappearance of the entire groove or ridge.
            let after_land = sign * if cylinders_move { 0.8 } else { 0.4 };
            let sections =
                lines.map(|[a, b]| ScrewOffsetSection::new(a, b, &motion, after_land).unwrap());
            let mut remaining = MAX_BOXES;
            let crossings = super::roots(
                [&sections[0], &sections[1]],
                [Ivl::new(0.0, 1.0); 2],
                Vec3::X,
                Ivl::from(if sign > 0.0 { 0.0 } else { -2.5 }),
                1e-8,
                &mut remaining,
            )
            .unwrap();
            assert_eq!(crossings.len(), 1, "post-land crossing: {crossings:?}");
            let radius = sections[0]
                .meridian_bounds(crossings[0].first.lo, crossings[0].first.hi, Vec3::X)
                .unwrap()
                .radius;
            let radial_shift = if cylinders_move { after_land } else { 0.0 };
            assert!(radius.lo > 8.646835306586814 + radial_shift);
            assert!(radius.hi < 10.0 + radial_shift);
            let final_gap = if sign > 0.0 { 2 } else { 1 };
            let final_end = if cylinders_move { 22.0 } else { 12.0 };
            let family = Ivl::new(0.0, final_end);
            assert!(band.radii[1].stays_above(band.radii[0], family));
            if cylinders_move {
                // Separate hulls overlap even though the literal affine
                // radius difference remains positive over this whole family.
                assert!(band.radii[0].at(family).hi >= band.radii[1].at(family).lo);
            }
            let radius_max = band.radii[1].at(family).hi;
            let rise = Ivl::from(2.5) / Ivl::checked(TAU, TAU);
            let slabs = caps.each_ref().map(|(placement, _)| {
                groove::cap_slab(placement, origin, Vec3::Z, rise, radius_max).unwrap()
            });
            assert!(slabs[1].hi < slabs[0].lo);
            for ((placement, scale), slab) in caps.iter().zip(slabs) {
                let uv = groove::cap_disk_parameters(
                    placement,
                    *scale,
                    origin,
                    Vec3::Z,
                    radius_max,
                    slab,
                )
                .unwrap();
                assert!(uv.iter().all(|v| v.lo > 0.0 && v.hi < 1.0));
            }
            let closure = band.event_bracket(final_gap, final_end).unwrap() * Ivl::from(0.1);
            let fit = band.fit_event(final_end).unwrap().unwrap() * Ivl::from(0.1);
            assert!(fit.lo <= closure.hi && closure.lo <= fit.hi);
            let surviving = band.surviving_section(after_land.abs() / 0.1).unwrap();
            let mut crossing_count = 0;
            for boundary in surviving {
                if let groove::SectionBoundary::Crossing {
                    winding,
                    radius: proved,
                    sources,
                } = boundary
                {
                    crossing_count += 1;
                    assert_eq!(winding, if sign > 0.0 { 0 } else { -1 });
                    assert!(proved.lo <= radius.hi && radius.lo <= proved.hi);
                    assert!(sources.iter().all(|s| s.lo > 0.0 && s.hi < 1.0));
                }
            }
            assert_eq!(crossing_count, 1);
            assert!(
                distance.hi < closure.lo,
                "land={distance:?}, final={closure:?}"
            );
            // The same physical event is found when the requested operation
            // itself passes the limit; it cannot be accepted as an ordered band.
            for flank in &mut band.flanks {
                flank.distance.rate = flank.distance.rate * Ivl::from(end);
            }
            for radius in &mut band.radii {
                radius.rate = radius.rate * Ivl::from(end);
            }
            let crossed = band.first_event(1.0).unwrap().unwrap() * Ivl::from(0.1) * Ivl::from(end);
            assert!(crossed.lo <= distance.hi && distance.lo <= crossed.hi);
            assert!(band.certify().is_err());
        }
    }
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::nurbs::b_spline_curve::NurbCircleParam;
    use crate::surface::procedural_sweep::{RadialLaw, ScrewMotion};
    use crate::sweep::screw::screw_net;

    fn line() -> BSplineCurve {
        BSplineCurve::create_line_segment(3, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 1.0))
            .unwrap()
    }

    fn wall(profile: &BSplineCurve, law: Option<RadialLaw>) -> Surface {
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, law).unwrap();
        let stations = [0.0, 0.1, 0.2];
        let net = screw_net(profile, &motion, &stations).unwrap();
        let definition = ProceduralSweep::create(profile, motion, &stations, 0.05).unwrap();
        let mut surface = Surface::from(net);
        surface.set_sweep_definition(definition);
        surface
    }

    fn affine_offset() -> Surface {
        let source = wall(&line(), None);
        let source = source.procedural_sweep().unwrap();
        source
            .definition()
            .normal_offset(source.net(), 0.1, 1e-4)
            .unwrap()
    }

    fn domain(surface: &Surface) -> Extent2d {
        surface
            .procedural_sweep()
            .unwrap()
            .net()
            .natural_uv_domain()
    }

    fn declines_both_orders(k: &Kernel, a: &Surface, da: &Extent2d, b: &Surface, db: &Extent2d) {
        for (first, fd, second, sd) in [(a, da, b, db), (b, db, a, da)] {
            match intersect(k, first, fd, second, sd, 1e-5) {
                Ok(None) => {}
                Ok(Some(_)) => panic!("a different geometric class entered screw specialization"),
                Err(error) => panic!(
                    "a different geometric class must decline before construction: {error:?}"
                ),
            }
        }
    }

    /// A seam edge between two half-turn pieces of one wall, as a body read back from its
    /// record holds it (#11582): a line whose stored support runs far past the edge, across the
    /// screw axis, since a record's line is unbounded. The edge's own segment lies in the
    /// authored half-plane and is certified; the whole support crosses the axis and is not.
    #[test]
    fn a_seam_line_is_certified_over_its_edge_not_its_whole_support() {
        use crate::geom::extent1d::Extent1d;
        let station = 16.0 * std::f64::consts::PI;
        let source = BSplineCurve::create_line_segment(
            3,
            Vec3::new(9.125, -64.0, -1.334_081_216_351_296_3),
            Vec3::new(7.646_835_306_586_814, -64.0, -2.1875),
        )
        .unwrap();
        let make = |stations: &[f64]| {
            let motion = ScrewMotion::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::Z, 2.5, None).unwrap();
            let net = screw_net(&source, &motion, stations).unwrap();
            let definition = ProceduralSweep::create(&source, motion, stations, 0.05).unwrap();
            let mut surface = Surface::from(net);
            surface.set_sweep_definition(definition);
            surface
        };
        let a = make(&[station - 0.1, station]);
        let b = make(&[station, station + 0.08]);
        let separator = seam::Separator::new(&a, &b).unwrap().unwrap();
        // the profile line carried eight pitches up, to the station
        let p0 = Vec3::new(9.125, -64.0, -1.334_081_216_351_296_3 + 20.0);
        let p1 = Vec3::new(7.646_835_306_586_814, -64.0, -2.1875 + 20.0);
        let reach = 1.0e6;
        let along = p1 - p0;
        let line = Curve::from(
            BSplineCurve::create_line_segment(3, p0 - along * reach, p1 + along * reach).unwrap(),
        );
        let span = 2.0 * reach + 1.0;
        let edge = Extent1d::from_min_max(reach / span, (reach + 1.0) / span);
        separator.certify_curve(&line, edge, 1e-5).unwrap();
        assert!(separator
            .certify_curve(&line, Extent1d::from_min_max(0.0, 1.0), 1e-5)
            .is_err());
    }

    #[test]
    fn segmented_meridian_seam_preserves_the_physical_separator_in_both_charts() {
        let k = Kernel::default();
        let station = 16.0 * std::f64::consts::PI;
        let corner = |radius| seam::CylinderCorner {
            radius: Ivl::from(radius),
            meridian_error: 0.0,
        };
        let radii = [corner(8.646_835_306_586_814), corner(10.0)];
        let source = BSplineCurve::create_line_segment(
            3,
            Vec3::new(9.125, -64.0, -1.334_081_216_351_296_3),
            Vec3::new(7.646_835_306_586_814, -64.0, -2.1875),
        )
        .unwrap();
        let make = |stations: &[f64], swapped| {
            let motion = ScrewMotion::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::Z, 2.5, None).unwrap();
            let net = screw_net(&source, &motion, stations).unwrap();
            let definition = ProceduralSweep::create(&source, motion, stations, 0.05).unwrap();
            let mut surface = Surface::from(net);
            surface.set_sweep_definition(definition);
            if swapped {
                surface.swap_uv(&k).unwrap();
            }
            surface
        };
        for swapped in [false, true] {
            let a = make(&[station - 0.1, station], swapped);
            let b = make(&[station, station + 0.08], swapped);
            let separator = seam::Separator::new(&a, &b).unwrap().unwrap();
            assert!(!separator.supports_targets(&a, &b).unwrap());
            let parent_uv = uv_curve(
                &[Point3::ZERO, Point3::new(0.0, 1.0, 0.0)],
                None,
                &[0.0, 1.0],
                &[2, 2],
            )
            .unwrap();
            // Literal crest-parent records differ by one ULP in the angular
            // coordinate. They are curved traces inside the same meridian tube,
            // not exactly constant-angle lines. Both native chart orders work.
            for parent_swapped in [false, true] {
                let frame = Axis2Placement::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::X, Vec3::Y);
                // Construct the literal chart directly: a legacy SwapUV edit can
                // rebuild analytic placement from its approximation instead.
                let cone = crate::surface::cone::Cone::create_canonical(
                    &k,
                    &frame,
                    10.0,
                    10.0,
                    1.0,
                    parent_swapped,
                    false,
                )
                .unwrap();
                assert!(cone
                    .coaxial_radius_enclosure(Vec3::new(-1.0, -64.0, 0.0), Vec3::Z)
                    .unwrap()
                    .is_some());
                let parent = Surface::from(cone);
                // A seam parent is a literal cylinder on the screw's axis; a tapered cone (a
                // chamfer meeting the shank) on the same axis is not one, and is skipped.
                assert!(separator.is_coaxial_cylinder(&parent).unwrap());
                let chamfer = crate::surface::cone::Cone::create_canonical_with_semi_angle(
                    &k,
                    &frame,
                    10.0,
                    -30.0,
                    Some(1.0),
                    parent_swapped,
                    false,
                )
                .unwrap();
                assert!(!separator
                    .is_coaxial_cylinder(&Surface::from(chamfer))
                    .unwrap());
                let crate::surface::surface::SurfaceKind::Cone(cone) = parent.kind() else {
                    panic!("lost cone");
                };
                let domain = cone.rev().base().natural_uv_domain();
                let angular = if parent_swapped {
                    domain.v_interval()
                } else {
                    domain.u_interval()
                };
                let height = if parent_swapped {
                    domain.u_interval()
                } else {
                    domain.v_interval()
                };
                let hi = angular.max();
                let lo = hi.next_down();
                assert!(lo < hi);
                let coordinate = |u, v| {
                    if parent_swapped {
                        Point3::new(v, u, 0.0)
                    } else {
                        Point3::new(u, v, 0.0)
                    }
                };
                let one_ulp = uv_curve(
                    &[coordinate(lo, height.min()), coordinate(hi, height.max())],
                    None,
                    &[0.0, 1.0],
                    &[2, 2],
                )
                .unwrap();
                let certified = separator
                    .certify_cylinder_uv(&k, &parent, &one_ulp, 2e-5)
                    .unwrap();
                assert!(certified.meridian_error < 2e-5);
                let ranges = [Ivl::new(lo, hi), Ivl::new(height.min(), height.max())];
                let [u, v] = ranges;
                let patch = if parent_swapped { [v, u] } else { ranges };
                let enclosure = cone.cylinder_patch_enclosure(&k, patch).unwrap().unwrap();
                let cap = cone.cylinder_runtime_allowance(&k).unwrap().unwrap();
                for u in [lo, hi] {
                    for v in [height.min(), height.max()] {
                        let uv = coordinate(u, v);
                        let point = cone.evaluate_point(&k, Point2::new(uv.x, uv.y)).unwrap();
                        for (range, value) in enclosure.into_iter().zip([point.x, point.y, point.z])
                        {
                            let padded = range + Ivl::new(-cap, cap);
                            assert!(padded.lo <= value && value <= padded.hi);
                        }
                    }
                }
                let off_plane = uv_curve(
                    &[
                        coordinate(hi - 0.01 * angular.length(), height.min()),
                        coordinate(hi, height.max()),
                    ],
                    None,
                    &[0.0, 1.0],
                    &[2, 2],
                )
                .unwrap();
                assert!(separator
                    .certify_cylinder_uv(&k, &parent, &off_plane, 2e-5)
                    .is_err());
            }
            for frame in [
                Axis2Placement::new(Vec3::new(-0.99, -64.0, 0.0), Vec3::X, Vec3::Y),
                Axis2Placement::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::X, Vec3::Z),
            ] {
                let cone = crate::surface::cone::Cone::create_canonical_cylinder(&k, &frame, 10.0)
                    .unwrap();
                let error = separator
                    .certify_cylinder_uv(&k, &Surface::from(cone), &parent_uv, 2e-5)
                    .unwrap_err();
                assert!(matches!(
                    error,
                    SolidError::InvalidInput {
                        reason: "seam parent is not a proved literal coaxial cylinder",
                        ..
                    }
                ));
            }
            let uv = |surface: &Surface| {
                let definition = surface.procedural_sweep().unwrap().definition();
                let points = [0.0, 1.0].map(|w| {
                    let p = definition.face_parameters(Point2::new(w, station)).unwrap();
                    Point3::new(p.x, p.y, 0.0)
                });
                uv_curve(&points, None, &[0.0, 1.0], &[2, 2]).unwrap()
            };
            separator.certify_source_uv(&a, &uv(&a), 2e-5).unwrap();
            separator.certify_source_uv(&b, &uv(&b), 2e-5).unwrap();
            for distance in [-0.1, 0.1] {
                let offset = |surface: &Surface| {
                    let retained = surface.procedural_sweep().unwrap();
                    retained
                        .definition()
                        .normal_offset(retained.net(), distance, 1e-6)
                        .unwrap()
                };
                let oa = offset(&a);
                let ob = offset(&b);
                // The correction goes beyond one original station boundary:
                // accepting these uncontinued charts would clamp the seam.
                assert!(separator
                    .construct(&k, (&oa, &domain(&oa)), (&ob, &domain(&ob)), radii, 2e-6)
                    .is_err());
                let extend = |surface: Surface| {
                    let retained = surface.procedural_sweep().unwrap();
                    retained
                        .definition()
                        .extended_for_offset(retained.net(), 0.22, 0.0, 5e-6)
                        .unwrap()
                };
                let ea = extend(oa);
                let eb = extend(ob);
                assert!(separator
                    .construct(
                        &k,
                        (&ea, &domain(&ea)),
                        (&eb, &domain(&eb)),
                        [corner(8.0), corner(20.0)],
                        2e-6
                    )
                    .is_err());
                assert!(separator
                    .construct(
                        &k,
                        (&ea, &domain(&ea)),
                        (&eb, &domain(&eb)),
                        [corner(10.0); 2],
                        2e-6
                    )
                    .is_err());
                let out = separator
                    .construct(&k, (&ea, &domain(&ea)), (&eb, &domain(&eb)), radii, 2e-6)
                    .unwrap();
                assert_eq!(out.curves_3d.len(), 1);
                assert!(out.deviations[0] <= 2e-6);
                let ua = out.surface1_uv_curves[0].as_ref().unwrap();
                let ub = out.surface2_uv_curves[0].as_ref().unwrap();
                let interval = out.curves_3d[0].natural_interval();
                for fraction in [0.0, 0.113, 0.379, 0.713, 0.937, 1.0] {
                    let x = interval.evaluate(fraction);
                    let pa = ua.evaluate_point(&k, x).unwrap();
                    let pb = ub.evaluate_point(&k, x).unwrap();
                    let ca = ea.evaluate_point(&k, Point2::new(pa.x, pa.y)).unwrap();
                    let cb = eb.evaluate_point(&k, Point2::new(pb.x, pb.y)).unwrap();
                    assert!(ca.distance(cb) <= out.deviations[0]);
                    assert!((ca.y + 64.0).abs() <= out.deviations[0] + 1e-12);
                }
            }
        }
    }

    #[test]
    fn a_non_affine_common_screw_wall_declines_specialization_in_either_order() {
        let k = Kernel::default();
        let frame = Axis2Placement::new(Vec3::new(1.5, 0.0, 0.5), Vec3::X, Vec3::Z);
        let circle = BSplineCurve::create_circle_segment(
            3,
            &frame,
            0.25,
            0.0,
            180.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let curved = wall(&circle, None);
        let affine = affine_offset();
        let a = affine.procedural_sweep().unwrap().definition();
        let b = curved.procedural_sweep().unwrap().definition();
        assert!(a.is_normal_offset());
        assert!(b.is_screw_invariant());
        assert_eq!(
            a.screw(),
            b.screw(),
            "same motion reaches the source-class guard"
        );
        assert!(b.screw_offset_section().unwrap().is_none());
        declines_both_orders(&k, &affine, &domain(&affine), &curved, &domain(&curved));
    }

    #[test]
    fn a_local_m20_projection_converges_on_the_definition() {
        use crate::geom::attribute::ObjectKey;
        use crate::geom::global_solver::SolverOperation;
        use crate::geom::object::ObjectRef;
        use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
        use crate::surface::surface::p7::good_drop_result;
        let k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(7.646835306586814, -64.0, -2.8125),
            Vec3::new(9.125, -64.0, -3.6659187836487037),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::Z, 2.5, None).unwrap();
        // Literal sweep construction: twenty half-turn pieces, 28 spans
        // each. This sequence is bit-equal to the exported input stations.
        let mut breaks = vec![0.0];
        let mut angle = TAU / 2.0;
        while angle < 10.0 * TAU - TAU / 16.0 {
            breaks.push(angle);
            angle += TAU / 2.0;
        }
        breaks.push(10.0 * TAU);
        let mut stations = vec![0.0];
        for pair in breaks.windows(2) {
            let [a, b] = *pair else { continue };
            for i in 1..=28 {
                stations.push(if i == 28 {
                    b
                } else {
                    a + (b - a) * f64::from(i) / 28.0
                });
            }
        }
        let net = screw_net(&profile, &motion, &stations).unwrap();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.06).unwrap();
        let mut surface = definition.normal_offset(&net, -0.1, 1e-4).unwrap();
        surface.swap_uv(&k).unwrap();
        surface.set_owner(Some(ObjectRef {
            type_code: crate::topology::face::p2::TYPE,
            key: ObjectKey(11),
        }));
        surface.set_owner_zone_tol_3d(Some(2e-5));
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let point = Vec3::new(-10.946468707948506, -62.97148313639685, 20.0);
        let guess = Point2::new(59.58760176333802, 0.9492411286543941);
        let r = surface
            .local_point_solve_1::<()>(&k, &domain, SolverOperation::Minimize, point, guess, None)
            .unwrap();
        let uv = Point2::new(
            r.solution.start.param(0).unwrap(),
            r.solution.start.param(1).unwrap(),
        );
        let d = surface
            .evaluate(&k, uv, 2, 2, EvaluateFlags::default())
            .unwrap();
        let gap = point - d[0][0];
        let residual = [
            gap.dot(d[1][0]) / d[1][0].length(),
            gap.dot(d[0][1]) / d[0][1].length(),
        ];
        let good =
            good_drop_result(&k, &surface, &domain, point, guess, &r.solution.start).unwrap();
        assert!(
            good,
            "a converged local foot must satisfy the existing S normal residual: {residual:?}"
        );
        assert!(r.found);
        // Fresh lazy evaluators honor both sides of the same dispatch boundary.
        // A seed may use B, but the subsequent exact local solve must use S.
        let mut exact =
            crate::surface::surface::p2::SurfaceEval::with_uv(&k, Some(&surface), uv.x, uv.y);
        exact.eval().unwrap();
        assert!((exact.values()[0][0] - d[0][0]).length() <= 1e-12);
        let approximate = crate::surface::procedural_blend::NetScope::with_net(&k, || {
            let mut seed =
                crate::surface::surface::p2::SurfaceEval::with_uv(&k, Some(&surface), uv.x, uv.y);
            seed.eval().unwrap();
            seed.values()[0][0]
        });
        let net = surface
            .procedural_sweep()
            .unwrap()
            .net()
            .evaluate_point(uv)
            .unwrap();
        assert!((approximate - net).length() <= 1e-12);
        assert!((approximate - d[0][0]).length() > 1e-7);
        // A lazy evaluator can outlive a temporary seed scope at the same UV.
        // Its cached position and normal must not turn B into an exact answer.
        let mut reused =
            crate::surface::surface::p2::SurfaceEval::with_uv(&k, Some(&surface), uv.x, uv.y);
        crate::surface::procedural_blend::NetScope::with_net(&k, || {
            reused.normal(&k).unwrap();
            assert!((reused.values()[0][0] - net).length() <= 1e-12);
        });
        let normal = exact.normal(&k).unwrap();
        assert!(
            (reused.normal(&k).unwrap() - normal).length() <= 1e-12,
            "a seed normal cached inside NetScope must not answer outside it"
        );
        reused.eval().unwrap();
        assert!((reused.values()[0][0] - d[0][0]).length() <= 1e-12);
        crate::surface::procedural_blend::NetScope::with_net(&k, || {
            reused.eval().unwrap();
            assert!((reused.values()[0][0] - net).length() <= 1e-12);
            {
                let _exact = crate::surface::procedural_blend::NetScope::exact(&k);
                reused.eval().unwrap();
                assert!((reused.values()[0][0] - d[0][0]).length() <= 1e-12);
            }
            reused.eval().unwrap();
            assert!((reused.values()[0][0] - net).length() <= 1e-12);
        });
    }

    #[test]
    fn a_supported_affine_offset_pair_reaches_the_specialized_construction() {
        let k = Kernel::default();
        let first = affine_offset();
        let second = first.copy(&k).unwrap();
        // Coincident affine supports are a supported class with four exact boundary curves.
        // This prevents an unconditional `None` from satisfying the declining-class tests.
        for (a, b) in [(&first, &second), (&second, &first)] {
            let result = intersect(&k, a, &domain(a), b, &domain(b), 1e-5)
                .unwrap()
                .expect("supported affine screw walls are specialized");
            assert_eq!(result.curves_3d.len(), 4);
            assert_eq!(result.surface1_uv_curves.len(), 4);
            assert_eq!(result.surface2_uv_curves.len(), 4);
        }
    }

    #[test]
    fn cylinder_specialization_declines_path_and_varying_law_walls_in_either_order() {
        let k = Kernel::default();
        let cylinder = Surface::from(
            crate::primitive::api_primitives::create_cylinder_surface(&k, Vec3::ZERO, 1.5, 2.0)
                .unwrap(),
        );
        let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        let path =
            BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0)).unwrap();
        let (definition, net) =
            ProceduralSweep::create_path_exact(&line(), &path, Vec3::X).unwrap();
        let mut path_wall = Surface::from(net);
        path_wall.set_sweep_definition(definition);
        assert!(path_wall
            .procedural_sweep()
            .unwrap()
            .definition()
            .path()
            .is_some());
        declines_both_orders(
            &k,
            &path_wall,
            &domain(&path_wall),
            &cylinder,
            &cylinder_domain,
        );

        let law = RadialLaw::new(Vec3::X, vec![0.0, 0.2], vec![0.0, 0.1]).unwrap();
        let varying = wall(&line(), Some(law));
        assert!(!varying
            .procedural_sweep()
            .unwrap()
            .definition()
            .is_screw_invariant());
        declines_both_orders(&k, &varying, &domain(&varying), &cylinder, &cylinder_domain);
        let offset = affine_offset();
        declines_both_orders(&k, &offset, &domain(&offset), &varying, &domain(&varying));
    }

    #[test]
    fn a_tapered_cone_declines_cylinder_specialization_in_either_order() {
        let k = Kernel::default();
        let cone = crate::surface::cone::Cone::create_canonical(
            &k,
            &Axis2Placement::IDENTITY,
            1.0,
            2.0,
            2.0,
            false,
            false,
        )
        .unwrap();
        assert!(!cone.is_cylinder_class());
        let cone = Surface::from(cone);
        let cone_domain = cone.natural_uv_domain_with_nurb(&k).unwrap();
        let offset = affine_offset();
        declines_both_orders(&k, &offset, &domain(&offset), &cone, &cone_domain);
    }
}
