//! Remove protrusions whose opposing planar walls consume their entire cavity.
//!
//! Only the source of the inner offset is simplified. The shell's outer operand is
//! retained. A positive-weight control hull proves each removed face lies between
//! the opposing planes; a concave root distinguishes a rib from its persistent wall.
//! An exact shoulder cut reconstructs the surviving planar neighbors. The
//! persistent wall's new boundary must remain on its original trimmed face.
//!
//! Upstream: none (regularized inner offsets).
//! Deviations: finite topology walks and at most the input face count of
//! reconstruction passes, each strictly decreasing the face count. No recursion.

use std::collections::BTreeSet;

use crate::curve::curve::CurveKind;
use crate::error::{Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p3::with_nurb;
use crate::topology::edge;
use crate::topology::edgeuse::{p1 as eu, p2 as eu2};
use crate::topology::face::{p2 as face, p4 as face4};
use crate::topology::{Brep, FaceId};

const OP: &str = "offset::offset_executive::consumed";

#[derive(Clone, Copy)]
pub(super) struct Plane {
    pub(super) origin: Point3,
    pub(super) normal: Vec3,
}

fn plane(b: &Brep, id: FaceId) -> Result<Option<Plane>> {
    let Some(surface) = b.face(id)?.surface.as_ref() else {
        return Ok(None);
    };
    let SurfaceKind::Plane(p) = surface.kind() else {
        return Ok(None);
    };
    let Some(fu) = face::outer_faceuse(b, id)? else {
        return Ok(None);
    };
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
        1.0
    } else {
        -1.0
    };
    Ok(Some(Plane {
        origin: p.position().origin(),
        normal: p.evaluate_normal().unitize()? * sign,
    }))
}

fn same_plane(a: Plane, b: Plane) -> bool {
    let magnitude = a.normal.x.abs() * (a.origin.x.abs() + b.origin.x.abs())
        + a.normal.y.abs() * (a.origin.y.abs() + b.origin.y.abs())
        + a.normal.z.abs() * (a.origin.z.abs() + b.origin.z.abs());
    magnitude.is_finite()
        && a.normal == b.normal
        && a.normal.dot(b.origin - a.origin).abs() <= gamma(32) * magnitude
}

fn match_plane(b: &Brep, p: Plane) -> Result<Option<FaceId>> {
    let mut found = None;
    for f in bp1::faces(b, None)? {
        if plane(b, f)?.is_some_and(|q| same_plane(p, q)) {
            if found.is_some() {
                return Ok(None);
            }
            found = Some(f);
        }
    }
    Ok(found)
}

/// Positive rational hulls enclose the trimmed boundary of a plane, or the entire
/// parameter rectangle of another analytic face. A blend's fitted net would not
/// prove containment of its definition and is therefore ineligible.
pub(super) fn in_slab(k: &Kernel, b: &Brep, f: FaceId, p: Plane, gap: f64) -> Result<bool> {
    let Some(surface) = b.face(f)?.surface.as_ref() else {
        return Ok(false);
    };
    let tolerance = b.tolerance();
    if matches!(surface.kind(), SurfaceKind::Plane(_)) {
        // A linear functional on a bounded planar region attains its extrema on
        // the boundary. Enclose the actual trimmed curves: a rotated UV rectangle
        // can extend beyond the face and cannot decide this case tightly.
        let edges = face4::edges(b, f, None, false, None)?;
        if edges.is_empty() {
            return Ok(false);
        }
        for e in edges {
            let Some(curve) = edge::create_trimmed_nurbs_curve(k, b, e)? else {
                return Ok(false);
            };
            let CurveKind::BSpline(curve) = curve.kind() else {
                return Ok(false);
            };
            let Some(net) = curve.gw_nurb() else {
                return Ok(false);
            };
            if net.cpts().is_empty()
                || !net
                    .cpts()
                    .iter()
                    .all(|&point| point_in_slab(point, p, gap, tolerance))
            {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if !surface.is_analytic()? {
        return Ok(false);
    }
    let Some(nurb) = with_nurb(k, surface)? else {
        return Ok(false);
    };
    let domain = face::uv_domain(b, f)?;
    let natural = nurb.natural_uv_domain();
    if !(domain.min().x >= natural.min().x
        && domain.max().x <= natural.max().x
        && domain.min().y >= natural.min().y
        && domain.max().y <= natural.max().y)
    {
        if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
            eprintln!(
                "consumed trace: face={f:?} UV domain={domain:?} outside natural={natural:?}"
            );
        }
        return Ok(false);
    }
    let patch = nurb.copy_sub_patch(&domain)?;
    let Some(net) = patch.gw_nurb() else {
        return Ok(false);
    };
    for point in net.rows().flatten() {
        if !point_in_slab(*point, p, gap, tolerance) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn point_in_slab(point: CPoint, p: Plane, gap: f64, tolerance: f64) -> bool {
    if point.w != NOW && !(point.w > 0.0 && point.w.is_finite()) {
        return false;
    }
    let q = point.to_euclid();
    let d = -p.normal.dot(q - p.origin);
    // Reciprocal and homogeneous division, subtraction, dot product and the
    // magnitude sum use fewer than 32 rounded operations. Include cancellation
    // against the plane's origin instead of bounding only the final distance.
    let magnitude = p.normal.x.abs() * (q.x.abs() + p.origin.x.abs())
        + p.normal.y.abs() * (q.y.abs() + p.origin.y.abs())
        + p.normal.z.abs() * (q.z.abs() + p.origin.z.abs());
    let rounding = gamma(32) * magnitude;
    d.is_finite()
        && rounding.is_finite()
        && d - rounding >= -tolerance
        && d + rounding <= gap + tolerance
}

/// The protrusion is wholly beyond its shoulder. This distinguishes its short
/// wall from a persistent wall that also bounds a different concave corner.
fn beyond_shoulder(k: &Kernel, b: &Brep, group: &[FaceId], shoulder: Plane) -> Result<bool> {
    let halfspace = Plane {
        origin: shoulder.origin,
        normal: -shoulder.normal,
    };
    for &f in group {
        // The upper bound is absent: the same control-hull proof bounds only the
        // signed distance towards the shoulder's outward halfspace.
        if !in_slab(k, b, f, halfspace, f64::INFINITY)? {
            return Ok(false);
        }
    }
    Ok(true)
}

struct Candidate {
    faces: Vec<FaceId>,
    persistent: FaceId,
    shoulder: Plane,
}

fn candidates(k: &Kernel, b: &mut Brep, open: &[FaceId], t: f64) -> Result<Vec<Candidate>> {
    let faces = bp1::faces(b, None)?;
    let mut found = Vec::new();
    for &seed in &faces {
        k.check_cancelled(OP)?;
        if open.contains(&seed) {
            continue;
        }
        let Some(a) = plane(b, seed)? else { continue };
        for &persistent in &faces {
            if seed == persistent || open.contains(&persistent) {
                continue;
            }
            let Some(p) = plane(b, persistent)? else {
                continue;
            };
            // Sketch intersections can leave an axis direction a few ulps from
            // its opposing direction. The complete seed hull below must also fit
            // the slab, so an angular error cannot hide a varying wall width.
            if (a.normal + p.normal).length() > gamma(64) {
                continue;
            }
            let gap = -p.normal.dot(a.origin - p.origin);
            let magnitude = p.normal.x.abs() * (a.origin.x.abs() + p.origin.x.abs())
                + p.normal.y.abs() * (a.origin.y.abs() + p.origin.y.abs())
                + p.normal.z.abs() * (a.origin.z.abs() + p.origin.z.abs());
            // Independently intersected planes can differ by rounding at the
            // collapse event. Resolve their separation at the B-rep's tolerance.
            let rounding = gamma(32) * (magnitude + 2.0 * t);
            if !(gap > b.tolerance() && gap <= 2.0 * t + b.tolerance() + rounding) {
                continue;
            }
            if !in_slab(k, b, seed, p, gap)? {
                continue;
            }
            let mut group = vec![seed];
            let mut at = 0;
            while let Some(&f) = group.get(at) {
                k.check_cancelled(OP)?;
                for e in face4::edges(b, f, None, false, None)? {
                    for next in edge::faces(b, e, None)? {
                        if next != persistent
                            && !open.contains(&next)
                            && !group.contains(&next)
                            && in_slab(k, b, next, p, gap)?
                        {
                            group.push(next);
                        }
                    }
                }
                at += 1;
            }
            let mut concave_root = None;
            let mut analytic_boundary = true;
            let mut touches_persistent = false;
            for &f in &group {
                for e in face4::edges(b, f, None, false, None)? {
                    let neighbors = edge::faces(b, e, None)?;
                    for next in neighbors.into_iter().filter(|next| !group.contains(next)) {
                        touches_persistent |= next == persistent;
                        if !b.face(next)?.surface.as_ref().is_some_and(|s| {
                            matches!(
                                s.kind(),
                                SurfaceKind::Plane(_)
                                    | SurfaceKind::Cone(_)
                                    | SurfaceKind::Sphere(_)
                                    | SurfaceKind::Torus(_)
                            )
                        }) {
                            analytic_boundary = false;
                        }
                        // A line between planes has constant binormals: one evaluation
                        // determines its sector over the whole edge, without sampling.
                        if f == seed && next != persistent && plane(b, next)?.is_some() {
                            let Some(shoulder) = plane(b, next)? else {
                                continue;
                            };
                            let outer = face::outer_faceuse(b, f)?;
                            for edgeuse in edge::edgeuses(b, e, None, None)? {
                                if eu::face(b, edgeuse)? == Some(f)
                                    && eu::faceuse(b, edgeuse)? == outer
                                    && !eu2::is_tangent_sector(k, b, edgeuse, 0.0, false, true)?
                                    && !eu2::is_convex_radial_sector(k, b, edgeuse, 1, false, 0.0)?
                                    && beyond_shoulder(k, b, &group, shoulder)?
                                {
                                    concave_root = Some(shoulder);
                                }
                            }
                        }
                    }
                }
            }
            if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
                eprintln!("consumed trace: seed={seed:?} persistent={persistent:?} gap={gap} group={group:?} root={} analytic={analytic_boundary} touches={touches_persistent}", concave_root.is_some());
            }
            if let Some(shoulder) = concave_root {
                if analytic_boundary
                    && touches_persistent
                    && face::loops(b, persistent, false, None)?.len() == 1
                {
                    found.push(Candidate {
                        faces: group,
                        persistent,
                        shoulder,
                    });
                }
            }
        }
    }
    Ok(found)
}

fn boundary_stays_on(
    k: &Kernel,
    original: &mut Brep,
    old: FaceId,
    made: &Brep,
    new: FaceId,
) -> Result<bool> {
    let inside = |class| {
        matches!(
            class,
            PointClassType::Face | PointClassType::Edge | PointClassType::Vertex
        )
    };
    for e in face4::edges(made, new, None, false, None)? {
        let edge = made.edge(e)?;
        let Some(curve) = edge.curve.as_ref() else {
            return Ok(false);
        };
        let tolerance = original.tolerance();
        let runs = face::classify_free_curve_on_face(
            k,
            original,
            old,
            curve,
            edge.interval,
            tolerance,
            false,
        )?;
        let mut covered = edge.interval.min();
        for run in runs {
            if run.interval.min() > covered
                || !inside(run.start.0)
                || !inside(run.mid)
                || !inside(run.end.0)
            {
                return Ok(false);
            }
            covered = covered.max(run.interval.max());
        }
        if covered < edge.interval.max() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Simplify the inner source, retaining the original shell operand and remapping
/// selected planar openings by their unique supporting plane.
pub(super) fn regularize(
    k: &mut Kernel,
    original: &Brep,
    open: &[FaceId],
    t: f64,
) -> Result<Option<(Brep, Vec<FaceId>)>> {
    let mut opening_planes = Vec::new();
    for &f in open {
        let Some(p) = plane(original, f)? else {
            return Ok(None);
        };
        if match_plane(original, p)? != Some(f) {
            return Ok(None);
        }
        opening_planes.push(p);
    }
    let mut source = original.clone_standing_for_self()?;
    let mut openings = open.to_vec();
    let mut changed = false;
    let bound = source.face_list().len();
    if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
        for f in bp1::faces(&source, None)? {
            if let Some(p) = plane(&source, f)? {
                eprintln!(
                    "consumed trace: plane={f:?} origin={:?} normal={:?}",
                    p.origin, p.normal
                );
            }
        }
    }
    for _ in 0..bound {
        let found = candidates(k, &mut source, &openings, t)?;
        if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
            eprintln!("consumed trace: t={t} candidate_count={}", found.len());
        }
        if found.is_empty() {
            break;
        }
        let mut old = source.clone_standing_for_self()?;
        let shoulder = found.first().map(|candidate| candidate.shoulder);
        let mut consumed = BTreeSet::new();
        let mut persistent = BTreeSet::new();
        for candidate in found {
            consumed.extend(candidate.faces);
            persistent.insert(candidate.persistent);
        }
        // A curved surviving rib can share only part of the short wall. Whole-face
        // removal would consume that surviving portion too. Leave such a group to
        // offset-space regularization of its coincident planar patches.
        let mut planar_boundary = true;
        for &f in &consumed {
            for e in face4::edges(&source, f, None, false, None)? {
                for next in edge::faces(&source, e, None)? {
                    if !consumed.contains(&next)
                        && (openings.contains(&next) || plane(&source, next)?.is_none())
                    {
                        planar_boundary = false;
                    }
                }
            }
        }
        if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
            eprintln!("consumed trace: union={consumed:?} persistent={persistent:?} planar_boundary={planar_boundary}");
        }
        if !planar_boundary {
            break;
        }
        let before = source.face_list().len();
        let clipped = match shoulder {
            Some(shoulder) => super::clip::clip(k, &source, &consumed, shoulder)?,
            None => None,
        };
        if std::env::var_os("SOLIDCORE_OFFSET_TRACE").is_some() {
            eprintln!("consumed trace: clipped={}", clipped.is_some());
        }
        let Some(clipped) = clipped else {
            // This certificate does not cover every possible topology change.
            // Leave the source untouched and retain offset-space regularization.
            break;
        };
        source = clipped;
        source.refresh_cached_topology();
        if source.face_list().len() >= before {
            return Err(SolidError::sm_err(OP));
        }
        for f in persistent {
            // A support wall can itself belong to another proven consumed rib.
            // Only walls retained by the union need a containment certificate.
            if consumed.contains(&f) {
                continue;
            }
            let Some(p) = plane(&old, f)? else {
                return Err(SolidError::sm_err(OP));
            };
            let Some(new) = match_plane(&source, p)? else {
                return Err(SolidError::sm_err(OP));
            };
            if !boundary_stays_on(k, &mut old, f, &source, new)? {
                return Err(SolidError::sm_err(OP));
            }
        }
        openings.clear();
        for &p in &opening_planes {
            let Some(f) = match_plane(&source, p)? else {
                return Err(SolidError::sm_err(OP));
            };
            openings.push(f);
        }
        changed = true;
    }
    Ok(changed.then_some((source, openings)))
}
