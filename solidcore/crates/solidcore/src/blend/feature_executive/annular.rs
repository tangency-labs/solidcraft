//! Joint extension across the two circumferential boundaries of a turned feature.
//! Each circle bounds an entire period of the support. Trimming to either one alone
//! discards the very band between them.

use super::{
    add_patch, bp1, copy_faces_carrying, defect, edge_vertices, stitch_and_orient, Feature,
    FeatureLoop, RebuildBrepType,
};
use crate::blend::fillet_section::{interval::Ivl, station::as_b_spline};
use crate::curve::curve::CurveKind;
use crate::error::blend::BlendFailure;
use crate::error::defeature::DefeatureFailure;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::tol::{self, SmallTopoType};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::query::api_queries::brep_is_manifold_solid;
use crate::surface::cone::Cone;
use crate::surface::implicit::{conic_turn, curve_lies_on, lies_on, AnalyticSupport, Implicit};
use crate::surface::surface::Surface;
use crate::topology::brep::{p3, p5, sheet};
use crate::topology::face::p3::{face_intersect_breps, FaceBreps, FaceIntersectOptions};
use crate::topology::{edge, vertex, Brep, FaceId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct Ring {
    height: f64,
    radius: f64,
}

/// One of the neighbouring meridians, expressed in the common axis frame.
#[derive(Clone, Copy)]
struct Support {
    face: FaceId,
    rim: Ring,
    slope: f64,
    max_extension: f64,
}

pub(super) struct Closure {
    origin: Vec3,
    axis: Vec3,
    x: Vec3,
    sides: [Support; 2],
    /// Two different meridians meet on this circle; identical ones need no join.
    intersection: Option<Ring>,
    shortfall: Option<DefeatureFailure>,
}

fn neighbour(b: &Brep, f: &Feature, l: &FeatureLoop) -> Result<Option<FaceId>> {
    let mut support = None;
    for &edge in &l.base_edges {
        let faces = edge::faces(b, edge, None)?;
        let mut kept = faces.into_iter().filter(|face| !f.faces.contains(face));
        let Some(face) = kept.next() else {
            return Ok(None);
        };
        if kept.next().is_some() || support.is_some_and(|other| other != face) {
            return Ok(None);
        }
        support = Some(face);
    }
    Ok(support)
}

fn cone_on(b: &Brep, face: FaceId) -> Result<Option<Implicit>> {
    Ok(b.face(face)?
        .surface
        .as_ref()
        .and_then(AnalyticSupport::of)
        .map(|a| a.implicit())
        .filter(|a| matches!(a, Implicit::Cone { .. })))
}

fn surface_lies_on(k: &Kernel, b: &Brep, face: FaceId, locus: &Implicit) -> Result<bool> {
    let Some(s) = &b.face(face)?.surface else {
        return Ok(false);
    };
    match as_b_spline(k, s)? {
        Some(net) => lies_on(&net, locus),
        None => Ok(false),
    }
}

/// Prove each trimmed edge lies on the cross-section and travels monotonically
/// around it. The closed edge chain must cover one turn, also for a split circle.
fn full_circle(
    k: &Kernel,
    b: &Brep,
    l: &FeatureLoop,
    locus: &Implicit,
    centre: Vec3,
    axis: Vec3,
    x: Vec3,
    radius: f64,
) -> Result<bool> {
    let plane = Implicit::Plane {
        origin: centre,
        normal: axis,
    };
    let y = axis.cross(x);
    let mut total = 0.0;
    let mut next = BTreeMap::new();
    for &edge in &l.base_edges {
        let Some(curve) = edge::create_trimmed_nurbs_curve(k, b, edge)? else {
            return Ok(false);
        };
        let CurveKind::BSpline(net) = curve.kind() else {
            return Ok(false);
        };
        if !curve_lies_on(net, &plane)? || !curve_lies_on(net, locus)? {
            return Ok(false);
        }
        let (a, z) = edge_vertices(b, edge)?;
        let (from, to, turn) = if let Some(turn) = conic_turn(net, centre, x, y, radius, radius)? {
            (a, z, turn)
        } else if let Some(turn) = conic_turn(net, centre, x, -y, radius, radius)? {
            (z, a, turn)
        } else {
            return Ok(false);
        };
        if next.insert(from, to).is_some() {
            return Ok(false);
        }
        total += turn;
    }
    let Some(&start) = next.keys().next() else {
        return Ok(false);
    };
    let mut at = start;
    while let Some(to) = next.remove(&at) {
        at = to;
    }
    // A consistently oriented, closed walk on the circle winds an integer number of
    // times. One turn is the only such angle between half a turn and one and a half.
    Ok(at == start && next.is_empty() && total > 180.0 && total < 540.0)
}

impl Closure {
    pub(super) fn supports(&self) -> [FaceId; 2] {
        self.sides.map(|s| s.face)
    }

    pub(super) fn of(k: &Kernel, b: &Brep, f: &Feature) -> Result<Option<Self>> {
        let [first, second] = f.loops.as_slice() else {
            return Ok(None);
        };
        if f.is_partitioned
            || !f.defeature_ready
            || f.loops.iter().any(|l| {
                l.base_rb_type != RebuildBrepType::ExtendSurface
                    || l.feat_rb_type != RebuildBrepType::FromOtherBrep
                    || l.base_patch.is_some()
                    || l.feat_patch.is_some()
                    || !l.base_ext_dist.is_finite()
                    || l.base_edges.is_empty()
            })
        {
            return Ok(None);
        }
        let (Some(a), Some(z)) = (neighbour(b, f, first)?, neighbour(b, f, second)?) else {
            return Ok(None);
        };
        if a == z {
            return Ok(None);
        }
        let Some(locus @ Implicit::Cone { origin, axis, .. }) = cone_on(b, a)? else {
            return Ok(None);
        };
        let Some(first_edge) = first.base_edges.first() else {
            return Ok(None);
        };
        let p = vertex::point(b, edge_vertices(b, *first_edge)?.0)? - origin;
        let Ok(x) = (p - axis * p.dot(axis)).unitize() else {
            return Ok(None);
        };
        let side = |face, l: &FeatureLoop| -> Result<Option<Support>> {
            let Some(Implicit::Cone {
                origin: o,
                axis: d,
                radius,
                tan_angle,
            }) = cone_on(b, face)?
            else {
                return Ok(None);
            };
            let Some(&edge) = l.base_edges.first() else {
                return Ok(None);
            };
            let point = vertex::point(b, edge_vertices(b, edge)?.0)?;
            let height = (point - origin).dot(axis);
            let centre = origin + axis * height;
            let radius = radius + (point - o).dot(d) * tan_angle;
            let slope = tan_angle * d.dot(axis).signum();
            let common_axis_cone = Implicit::Cone {
                origin: centre,
                axis,
                radius,
                tan_angle: slope,
            };
            if !radius.is_finite()
                || radius <= 0.0
                || !surface_lies_on(k, b, face, &common_axis_cone)?
                || !full_circle(k, b, l, &common_axis_cone, centre, axis, x, radius)?
            {
                return Ok(None);
            }
            Ok(Some(Support {
                face,
                rim: Ring { height, radius },
                slope,
                max_extension: super::extension_distance(k, b, &l.base_edges, l.base_ext_dist),
            }))
        };
        let (Some(mut lo), Some(mut hi)) = (side(a, first)?, side(z, second)?) else {
            return Ok(None);
        };
        if hi.rim.height < lo.rim.height {
            std::mem::swap(&mut lo, &mut hi);
        }
        let gap = hi.rim.height - lo.rim.height;
        if !gap.is_finite() || gap <= b.tolerance() || !brep_is_manifold_solid(b)? {
            return Ok(None);
        }
        // A retained neighbour has to run away from the interval being filled.
        // Its complete rim separates the two sides of the analytic support.
        for (s, sign) in [(lo, -1.0), (hi, 1.0)] {
            let mut outside = false;
            for v in crate::topology::face::p4::vertices(b, s.face, None, false, None)? {
                let distance = ((vertex::point(b, v)? - origin).dot(axis) - s.rim.height) * sign;
                if distance < -b.tolerance() {
                    return Ok(None);
                }
                outside |= distance > b.tolerance();
            }
            if !outside {
                return Ok(None);
            }
        }
        let available = [lo.max_extension, hi.max_extension];
        let [lo_scale, hi_scale] = [lo, hi].map(|s| s.slope.hypot(1.0));
        let (intersection, shortfall) = if surface_lies_on(k, b, z, &locus)? {
            let missing = gap - (lo.max_extension / lo_scale + hi.max_extension / hi_scale);
            // Any division closes a shared generator. One sufficient pair leaves
            // both current reaches in place and shares the remaining gap equally.
            let shortfall = if missing > 0.0 {
                let a = lo.max_extension + (0.5 * missing) * lo_scale;
                let mut b = hi.max_extension + (0.5 * missing) * hi_scale;
                let axial = a / lo_scale;
                if axial + b / hi_scale < gap {
                    // Replay uses the same rounded axial reach. The upper bound on
                    // the remainder is representable, so b / hi_scale rounds to at
                    // least that bound. Adding axial then rounds to at least gap.
                    let remaining_axial = Ivl::from(gap).sub_ulp(Ivl::from(axial)).hi;
                    b = Ivl::from(remaining_axial).mul_ulp(Ivl::from(hi_scale)).hi;
                }
                if !a.is_finite() || !b.is_finite() {
                    return Ok(None);
                }
                Some(DefeatureFailure::InsufficientAnnularReach {
                    available,
                    sufficient: [a, b],
                })
            } else {
                None
            };
            (None, shortfall)
        } else {
            // r_lo + m_lo*t = r_hi + m_hi*(t-gap). Require the intersection
            // of the two generators strictly inside the missing interval.
            let d = Ivl::from(gap);
            let t = (Ivl::from(hi.rim.radius) - Ivl::from(lo.rim.radius) - Ivl::from(hi.slope) * d)
                / (Ivl::from(lo.slope) - Ivl::from(hi.slope));
            let radius = Ivl::from(lo.rim.radius) + Ivl::from(lo.slope) * t;
            if !(t.lo > 0.0 && t.hi < gap && radius.lo > 0.0) {
                return Ok(None);
            }
            let sufficient = [t.hi * lo_scale, (gap - t.lo) * hi_scale];
            let shortfall = sufficient
                .iter()
                .zip(available)
                .any(|(&need, reach)| need > reach)
                .then_some(DefeatureFailure::InsufficientAnnularReach {
                    available,
                    sufficient,
                });
            let join = Ring {
                height: lo.rim.height + t.mid(),
                radius: radius.mid(),
            };
            (Some(join), shortfall)
        };
        Ok(Some(Self {
            origin,
            axis,
            x,
            sides: [lo, hi],
            intersection,
            shortfall,
        }))
    }

    fn surface(&self, k: &Kernel, lo: Ring, hi: Ring) -> Result<Surface> {
        let origin = self.origin + self.axis * lo.height;
        let height = hi.height - lo.height;
        let y = self.axis.cross(self.x);
        let mut cone = if lo.radius == hi.radius {
            Cone::new_cylinder_bounded(
                origin, self.x, y, lo.radius, 0.0, 360.0, height, false, false, false,
            )
        } else {
            Cone::new_bounded(
                origin, self.x, y, lo.radius, hi.radius, 0.0, 360.0, height, false, false, false,
            )
        };
        cone.make_nurb(k)?;
        Ok(Surface::from(cone))
    }

    fn patch(&self, k: &mut Kernel, tolerance: f64) -> Result<Brep> {
        let [lo, hi] = self.sides.map(|s| s.rim);
        let surface = self.surface(k, lo, self.intersection.unwrap_or(hi))?;
        let (mut patch, _) = sheet::sheet_on(k, surface, tolerance)?;
        if let Some(join) = self.intersection {
            let surface = self.surface(k, join, hi)?;
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            sheet::face_on(k, &mut patch, surface, &domain)?;
            patch.refresh_cached_topology();
        }
        stitch_and_orient(k, &mut patch)?;
        Ok(patch)
    }

    /// There is no junction between the interior of the new band and any other
    /// retained face. A rib or another shell crossing the gap would need a different
    /// trim and cannot be filled by this annulus.
    fn obstruction(
        &self,
        k: &Kernel,
        base: &Brep,
        selected: &[FaceId],
        patch: &Brep,
    ) -> Result<Option<FaceId>> {
        let tolerance = tol::brep_zone_tol_3d(k, base, SmallTopoType::default());
        let approx = tol::approx_tol3d(k, tolerance);
        let sides = self.supports();
        let options = FaceIntersectOptions {
            want_points: true,
            ..Default::default()
        };
        let mut probes = None;
        for wall in bp1::faces(patch, None)? {
            let wall_box = crate::query::face::p1::calculate_bounding_box(k, patch, wall, false)?;
            for other in bp1::faces(base, None)? {
                if sides.contains(&other) || selected.contains(&other) {
                    continue;
                }
                let face_box =
                    crate::query::face::p1::calculate_bounding_box(k, base, other, false)?;
                if wall_box.are_disjoint(&face_box, tolerance) {
                    continue;
                }
                // The intersection may keep UV trims and grow tolerances on both
                // bodies. Only its decision and the original face id leave the probe.
                let (patch_probe, base_probe) = match &mut probes {
                    Some(pair) => pair,
                    empty @ None => {
                        empty.insert((patch.clone_for_trial()?, base.clone_for_trial()?))
                    }
                };
                let intersection = face_intersect_breps(
                    k,
                    FaceBreps::Two(patch_probe, base_probe),
                    wall,
                    other,
                    approx,
                    20.0,
                    &options,
                )?;
                if !intersection.curves_3d.is_empty()
                    || intersection.points_3d.is_some_and(|p| !p.is_empty())
                {
                    return Ok(Some(other));
                }
            }
        }
        Ok(None)
    }

    /// A closed shell lying wholly in the gap never intersects the band. With other
    /// material or void regions present, try the patch on a body with the same arena
    /// ids, and keep it only if the untouched faces still separate material and void.
    pub(super) fn remove(
        &self,
        k: &mut Kernel,
        base: &mut Brep,
        feature: &mut Feature,
        patches: &mut Vec<Option<Brep>>,
    ) -> Result<Option<DefeatureFailure>> {
        if self.shortfall.is_some() {
            return Ok(self.shortfall);
        }
        let patch = self.patch(k, base.tolerance())?;
        if let Some(face) = self.obstruction(k, base, &feature.faces, &patch)? {
            return Ok(Some(DefeatureFailure::PatchIntersectsFace { face }));
        }
        if bp1::regions(base, None)?.len() <= 2 {
            super::partition_feature(k, base, feature)?;
            self.apply(k, base, feature, patch, patches)?;
            return Ok(None);
        }
        let mut trial = base.clone_for_trial()?;
        let mut cut = Feature {
            loops: feature.loops.clone(),
            faces: feature.faces.clone(),
            defeature_ready: feature.defeature_ready,
            ..Feature::default()
        };
        let previous = patches.len();
        super::partition_feature(k, &mut trial, &mut cut)?;
        self.apply(k, &mut trial, &mut cut, patch, patches)?;
        let supports = self.supports();
        for face in bp1::faces(base, None)? {
            if supports.contains(&face) || feature.faces.contains(&face) {
                continue;
            }
            if bounds_material(base, face)? && !bounds_material(&trial, face)? {
                patches.truncate(previous);
                return Ok(Some(DefeatureFailure::PatchEnclosesFace { face }));
            }
        }
        *base = trial;
        *feature = cut;
        Ok(None)
    }

    fn apply(
        &self,
        k: &mut Kernel,
        base: &mut Brep,
        f: &mut Feature,
        mut patch: Brep,
        patches: &mut Vec<Option<Brep>>,
    ) -> Result<()> {
        let faces = bp1::faces(&patch, None)?;
        let (added, copied) = copy_faces_carrying(k, &mut patch, &faces, base, true)?;
        if !copied {
            return Err(defect("copying the annular patch to the base left out a patch face"));
        }
        stitch_and_orient(k, base)?;
        let supports = self.supports();
        let mut rims = Vec::new();
        let mut rim_vertices = Vec::new();
        let mut certified = BTreeSet::new();
        for e in bp1::edges(base, None)? {
            let faces = edge::faces(base, e, None)?;
            if let (Some(&wall), Some(&neighbour)) = (
                faces.iter().find(|f| added.contains(f)),
                faces.iter().find(|f| supports.contains(f)),
            ) {
                if certified.insert((wall, neighbour)) {
                    let reason = BlendFailure::Defect {
                        why: "the constructed annular patch does not lie on its retained support",
                    };
                    let defect = || SolidError::blend_at(super::OP, reason, EntityRef::Edge(e));
                    let locus = cone_on(base, neighbour)?.ok_or_else(defect)?;
                    if !surface_lies_on(k, base, wall, &locus)? {
                        return Err(defect());
                    }
                }
                let (a, z) = edge_vertices(base, e)?;
                rim_vertices.extend([a, z]);
                rims.push(e);
            }
        }
        rim_vertices.sort_unstable();
        rim_vertices.dedup();
        p5::remove_topological_joins(k, base, &rims, &rim_vertices)?;
        let remaining = supports
            .iter()
            .chain(&added)
            .filter(|&&id| base.face(id).is_ok())
            .count();
        if remaining != faces.len() {
            return Err(SolidError::blend(
                super::OP,
                BlendFailure::Defect {
                    why: "coalescing the certified circular joins left the wrong number of support faces",
                },
            ));
        }
        // CopyFaces binds under its notification names. The surviving patch surface
        // and, at a two-support join, its circle belong to the receiving body's
        // entities under that body's naming scheme (as in local_operation::move_faces).
        let naming = base.naming();
        p3::rebind_geometry_owners(base, crate::topology::loopuse::notify_ref, |e| {
            crate::topology::scheme_owner_identity(naming, e)
        })?;
        let feature = f.fbrep.as_mut().ok_or_else(|| defect(super::FEATURE_BREP_GONE))?;
        let (_, copied) = copy_faces_carrying(k, &mut patch, &faces, feature, true)?;
        if !copied {
            return Err(defect("copying the annular patch to the partitioned feature left out a patch face"));
        }
        stitch_and_orient(k, feature)?;
        if !brep_is_manifold_solid(base)? || !brep_is_manifold_solid(feature)? {
            return Err(SolidError::blend(
                super::OP,
                BlendFailure::Defect {
                    why: "stitching the patch between the two certified rims left a non-manifold solid",
                },
            ));
        }
        let id = add_patch(patches, patch);
        for (i, l) in f.loops.iter_mut().enumerate() {
            l.base_patch = Some(id);
            l.feat_patch = Some(id);
            l.use_b_patch = i == 0;
            l.use_f_patch = i == 0;
        }
        f.joined_patches = true;
        f.use_b_patch_for_f = true;
        f.base_rebuilt = true;
        f.feat_rebuilt = true;
        f.base_was_applied = true;
        f.feat_was_applied = true;
        f.can_restore = false;
        Ok(())
    }
}

fn bounds_material(b: &Brep, f: FaceId) -> Result<bool> {
    use crate::mass::integrator::properties::{partition_face, sheet_face};
    Ok(!partition_face(b, f)? && !sheet_face(b, f)?)
}
