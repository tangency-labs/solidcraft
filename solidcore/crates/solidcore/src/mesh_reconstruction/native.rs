//! Build and check the solids and planar intersections used to certify a reconstruction.
//!
//! Certified arcs construct cylinder surfaces directly; source facets share exact indexed topology.
//! Deviations: no upstream counterpart. No recursion.

use super::topology::{self, Face, Region, Side};
use crate::boolean::api_brep::{boolean, boolean_2d, Boolean2dOutcome};
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::mass::MassFailure;
use crate::error::topology::TopologyFailure;
use crate::eval::Frame;
use crate::geom::a_object::BooleanOperation;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::{brep_volume_default, no_unsure_region};
use crate::math::det;
use crate::math::{Vec2, Vec3};
use crate::primitive::api_trim_ops::create_planar_faces;
use crate::primitive::primitive_creation::Boolean2d;
use crate::prism::profile::{
    axes, check_regions, curve_on_frame, to_local, to_world, Region2, Seg2,
};
use crate::prism::sheet::sheet_regions;
use crate::surface::cone::Cone;
use crate::surface::plane::Plane;
use crate::surface::surface::Surface;
use crate::topology::brep::is_outward_solid;
use crate::{Brep, Result, SolidError};

const PROFILE_TOLERANCE: f64 = 1e-7;
const VOLUME_ACCURACY: f64 = 1e-8;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("reconstruct mesh", reason)
}

/// Extrude the already certified disjoint regions and their holes on their recovered frame.
pub(super) fn extrude(
    k: &mut Kernel,
    frame: &Frame,
    regions: &[Region2],
    height: f64,
) -> Result<Option<Brep>> {
    if check_regions(regions.iter(), PROFILE_TOLERANCE, k.tolerances.zone_tol_3d).is_err() {
        return Ok(None);
    }
    let mut regions = regions.iter();
    let Some(first) = regions.next() else {
        return Ok(None);
    };
    let mut body = extrude_one(k, frame, first, height)?;
    for region in regions {
        let part = extrude_one(k, frame, region, height)?;
        body = union(k, body, part)?;
    }
    Ok(Some(body))
}

/// A bounded plane covering the supplied boundary in the recovered orthonormal frame.
pub(super) fn plane_surface(frame: &Frame, points: &[Vec3]) -> Result<Surface> {
    let (origin, x, y, _) = axes(frame);
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &p in points {
        let p = to_local(frame, p);
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    if !(lo.x < hi.x && lo.y < hi.y) {
        return Err(invalid("a reconstructed plane has no area"));
    }
    Ok(Surface::from(Plane::new(
        origin,
        x,
        y,
        Vec2::new(1., 1.),
        &Extent2d::from_bounds(lo.x, lo.y, hi.x, hi.y),
    )))
}

fn line(a: Vec3, b: Vec3) -> Result<Side> {
    Ok(Side::new(Curve::from(Line::create_line_segment(
        3, a, b, None,
    )?)))
}

/// The caps and each known profile segment's wall are produced together. A cylinder uses
/// the certified arc's centre, radius and interval, without refitting or a sweep setting.
fn extrude_one(k: &mut Kernel, frame: &Frame, region: &Region2, height: f64) -> Result<Brep> {
    let mut faces = Vec::new();
    let (_, x, y, normal) = axes(frame);
    let mut extent = Vec::new();
    for seg in region.loops().flat_map(|lp| &lp.segs) {
        extent.extend([
            to_world(frame, seg.start(), 0.),
            to_world(frame, seg.end(), 0.),
        ]);
        if let Seg2::Arc { center, radius, .. } = *seg {
            extent.extend(
                [
                    Vec2::new(center.x - radius, center.y - radius),
                    Vec2::new(center.x + radius, center.y + radius),
                ]
                .map(|p| to_world(frame, p, 0.)),
            );
        }
    }
    for (level, forward) in [(0., false), (height, true)] {
        let mut loops = Vec::new();
        for lp in region.loops() {
            let mut wire = lp
                .segs
                .iter()
                .map(|seg| curve_on_frame(k, frame, level, seg).map(Side::new))
                .collect::<Result<Vec<_>>>()?;
            if !forward {
                wire.reverse();
                for side in &mut wire {
                    side.forward = false;
                }
            }
            loops.push(wire);
        }
        let cap_frame = Frame {
            origin: (Vec3::from_array(frame.origin) + normal * level).to_array(),
            ..frame.clone()
        };
        let points: Vec<_> = extent.iter().map(|p| *p + normal * level).collect();
        let surface = plane_surface(&cap_frame, &points)?;
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        faces.push(Face {
            loops,
            surface,
            domain,
            forward,
        });
    }
    for seg in region.loops().flat_map(|lp| &lp.segs) {
        let a = to_world(frame, seg.start(), 0.);
        let b = to_world(frame, seg.end(), 0.);
        let bottom = Side::new(curve_on_frame(k, frame, 0., seg)?);
        let top = Side::new(curve_on_frame(k, frame, height, seg)?).reversed(k)?;
        let loops = vec![vec![
            bottom,
            line(b, b + normal * height)?,
            top,
            line(a + normal * height, a)?,
        ]];
        let (surface, forward) = match *seg {
            Seg2::Line { .. } => {
                let tangent = (b - a).unitize()?;
                let surface = Surface::from(Plane::new(
                    a,
                    tangent,
                    normal,
                    Vec2::new(1., 1.),
                    &Extent2d::from_bounds(0., 0., a.distance(b), height),
                ));
                (surface, true)
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                // The analytic support and its form share the first increasing-angle meridian.
                let first = start + sweep.min(0.);
                let radial = x * det::cos(first) + y * det::sin(first);
                let mut placement = Axis2Placement::IDENTITY;
                placement.set_step_canonical(to_world(frame, center, 0.), normal, radial)?;
                let mut cylinder = Cone::new_cylinder_bounded(
                    placement.origin(),
                    placement.x_axis(),
                    placement.y_axis(),
                    radius,
                    0.,
                    sweep.abs().to_degrees(),
                    height,
                    false,
                    false,
                    false,
                );
                cylinder.make_nurb(k)?;
                (Surface::from(cylinder), sweep > 0.)
            }
            _ => {
                return Err(invalid(
                    "a reconstructed section is not made of lines and arcs",
                ));
            }
        };
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        faces.push(Face {
            loops,
            surface,
            domain,
            forward,
        });
    }
    let shells = vec![(0..faces.len()).collect()];
    topology::build(
        k,
        faces,
        &shells,
        &[
            Region {
                void: true,
                shells: vec![0],
            },
            Region {
                void: false,
                shells: vec![0],
            },
        ],
    )
}

/// The area the two profiles share in their own plane, using the exact planar Boolean.
pub(super) fn intersection_area(
    k: &mut Kernel,
    a: &Region2,
    b: &Region2,
    tolerance: f64,
) -> Result<Option<f64>> {
    if check_regions([a, b].into_iter(), tolerance, k.tolerances.zone_tol_3d).is_err() {
        return Ok(None);
    }
    let frame = Frame {
        origin: [0., 0., 0.],
        x_axis: [1., 0., 0.],
        y_axis: [0., 1., 0.],
    };
    let mut face = |region: &Region2| {
        let curves = region
            .loops()
            .flat_map(|lp| &lp.segs)
            .map(|seg| curve_on_frame(k, &frame, 0., seg))
            .collect::<Result<Vec<_>>>()?;
        match create_planar_faces(k, &curves) {
            Ok(face) => Ok(Some(face)),
            Err(error)
                if matches!(
                    error.topology_failure(),
                    Some(TopologyFailure::NoPlaneThroughCurves {
                        class: crate::error::ErrorKind::InvalidInput
                    })
                ) && error.cause().is_some_and(|cause| {
                    matches!(
                        cause.topology_failure(),
                        Some(TopologyFailure::LoopWithoutNormal)
                    ) && cause.cause().is_none()
                }) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    };
    let Some(sa) = face(a)? else {
        return Ok(None);
    };
    let Some(sb) = face(b)? else {
        return Ok(None);
    };
    match boolean_2d(k, sa, sb, Boolean2d::Intersection)? {
        Boolean2dOutcome::Unassigned { .. } => Ok(Some(0.)),
        Boolean2dOutcome::Result(overlap) if overlap.face_list().is_empty() => Ok(Some(0.)),
        Boolean2dOutcome::Result(overlap) => sheet_regions(k, &overlap, &frame, tolerance)?
            .regions
            .iter()
            .try_fold(0., |area, region| Ok(area + region.signed_area()?.value))
            .map(Some),
    }
}

/// Join two reconstructed solids through the kernel's B-rep Boolean.
pub(super) fn union(k: &mut Kernel, a: Brep, b: Brep) -> Result<Brep> {
    let body = boolean(k, a, b, BooleanOperation::Union)?.result;
    if body.face_list().is_empty() {
        return Err(invalid("empty reconstructed part union"));
    }
    Ok(body)
}

/// Expected inability to certify an analytic candidate is applicability, while missing
/// topology, cancellation, unported operations and unexpected kernel defects stay errors.
pub(super) fn certificate<T>(answer: Result<T>) -> Result<Option<T>> {
    match answer {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            // A wrapped kernel failure is not a direct answer of this certificate.
            if error.cause().is_some() {
                return Err(error);
            }
            let geometric = matches!(&error, SolidError::UnsureRegion { .. })
                || matches!(
                    error.mass_failure(),
                    Some(
                        MassFailure::StepControl
                            | MassFailure::Integrand
                            | MassFailure::SkippedSpan
                            | MassFailure::EdgeCurveSingular
                            | MassFailure::TrimNotFinite
                    )
                )
                || matches!(
                    &error,
                    SolidError::Unsupported {
                        operation: "mass::enclosure",
                        reason: "a parameter box that is not finite"
                            | "a surface whose weights are not proved positive at a point"
                            | "a surface whose weights are not proved positive along a trim"
                    }
                )
                || matches!(
                    &error,
                    SolidError::InvalidInput {
                        operation: "mass::enclosure",
                        reason: "an edge whose face sides do not pair in opposite senses"
                            | "a face whose sides are not one material region and one void"
                            | "an edge whose two pcurves share no parameters"
                            | "a loop's end that is not finite"
                    }
                );
            if geometric {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

pub(super) fn volume(k: &mut Kernel, body: &mut Brep) -> Result<Option<f64>> {
    k.check_cancelled("reconstruct mesh")?;
    if certificate(no_unsure_region(body))?.is_none() {
        return Ok(None);
    }
    Ok(certificate(brep_volume_default(k, body, VOLUME_ACCURACY))?
        .filter(|v| v.is_finite() && *v > 0.))
}

pub(super) fn is_valid(k: &mut Kernel, body: &mut Brep) -> Result<bool> {
    k.check_cancelled("reconstruct mesh")?;
    Ok(certificate(is_outward_solid(k, body))?.unwrap_or(false))
}
