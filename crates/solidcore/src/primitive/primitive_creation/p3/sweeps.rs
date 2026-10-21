//! Linear and rotational primitive sweep control.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation::create_repeated_linear_sweep`] | seven-argument `CreateLinearSweep` |
//! | [`PrimitiveCreation::create_rotational_sweep`] | full `CreateRotationalSweep` |
//! | [`PrimitiveCreation::create_partial_rotational_sweep`] | partial `CreateRotationalSweep` |
//!
//! Deviations: typed services and owned curve slots replace pointers; null curve
//! dereferences become errors. The existing sweep engine bounds repetitions to
//! 4096. No additional recursion. Unavailable calls retain their full caller tails.

use super::{construct::target, geometry::bspline, PrimitiveAccess};
use crate::curve::curve::Curve;
use crate::curve::ellipse::Ellipse;
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::geom_utility::{
    p1::{line_closest_point, line_point_distance},
    p2::ContinuityType,
};
use crate::math::{max4, Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::primitive::primitive_creation::p1::{analytic_rotational_sweep, PrimitiveCreation};
use crate::surface::surf_of_revolution::p1::SurfOfRevolution;
use crate::surface::surface::Surface;
use crate::sweep::rotational_sweep_geometry::RotationalSweepGeometry;
use crate::sweep::topology_sweep::{
    engine::SweepTargets, geometry::SweepGeometry, SweepEntity, TopologySweep,
};
use crate::sweep::trans_sweep_geometry::TranslationalSweepGeometry;
use crate::topology::{BrepId, FaceId};
const OP: &str = "primitive::primitive_creation::p3::sweeps";

/// Flags shared by the repeated linear and partial rotational sweeps.
#[derive(Debug, Clone, Copy)]
pub struct RepeatedSweepOptions {
    /// Number of incremental sweeps, including zero as accepted upstream.
    pub repetitions: u64,
    /// Create end caps; repeated capped sweeps require exactly one repetition.
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
        let curve = slot.as_ref().ok_or_else(|| SolidError::null_pointer(OP))?;
        let curve = bspline(curve)
            .ok_or_else(|| SolidError::invalid_input(OP, "non B-spline continuity input"))?;
        let pieces =
            ignored(curve.subdivide_at_discontinuity(a.kernel(b)?, ContinuityType::C1, 0.5))?
                .unwrap_or_default();
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
        if options.cap_ends && options.repetitions != 1 {
            return Err(SolidError::sm_err(OP));
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
            if let Some(()) = ignored(a.planar_faces(region, curves, tolerance, &mut faces))? {
                for &face in &faces {
                    edges.extend(a.face_edges((brep, face))?);
                }
            } else {
                for slot in curves.iter_mut() {
                    if slot.is_none() {
                        return Err(SolidError::sm_err(OP));
                    }
                    let Some(edge) = ignored(a.wire(brep, slot))? else {
                        return Err(SolidError::sm_err(OP));
                    };
                    edges.push(edge);
                }
                faces.clear();
            }
            if distance == 0.0 {
                return Ok(());
            }
            let mut cap = options.cap_ends;
            if let Some(&base) = faces.first() {
                edges = a.face_edges((brep, base))?;
            } else {
                cap = false;
            }
            let mut geometry = TranslationalSweepGeometry::new(distance * direction);
            geometry.base.eps = tolerance;
            let geometry = SweepGeometry::Translation(geometry);
            let mut sweep = TopologySweep {
                do_tagging: self.tag_id() > 0,
                tag_id: self.tag_id(),
                repetitions: options.repetitions,
                ..Default::default()
            };
            let mut selected = targets(brep, &faces, &edges, cap);
            if cap {
                sweep.do_stitching = false;
                sweep.do_sweep(a, &geometry, Some(brep), Some(region), &mut selected)?;
                a.stitch_linear_caps(brep, &mut sweep, tolerance)?;
            } else {
                sweep.do_sweep(a, &geometry, Some(brep), Some(region), &mut selected)?;
                for face in faces {
                    ignore_status(a.delete_primitive_face((brep, face)))?;
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
        if let [Some(curve)] = curves {
            if ignored(a.with_primitive_brep(brep, |k, b| {
                analytic_rotational_sweep(k, b, curve, axis_base, axis)
            }))?
            .is_some()
            {
                if let Some(slot) = curves.first_mut() {
                    *slot = None;
                }
                return Ok(());
            }
        }
        for slot in curves.iter_mut() {
            let curve = slot.as_ref().ok_or_else(|| SolidError::null_pointer(OP))?;
            let length = ignored(curve.length(a.kernel(brep)?, &curve.natural_interval(), 0.001))?
                .unwrap_or(0.0);
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
                    ignore_status(frame.set_canonical(origin, tangent, axis.cross(tangent)))?;
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
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    let tolerance = a.brep(brep)?.tolerance();
                    ignore_status(a.planar_face((brep, infinite), circles, tolerance, &mut face))?;
                    *slot = None;
                }
            }
            if face.is_none() {
                if let Some(curve) = slot.as_ref() {
                    let rev = ignored(SurfOfRevolution::create_canonical(
                        a.kernel(brep)?,
                        curve.clone(),
                        axis_base,
                        axis,
                        false,
                    ))?;
                    let mut success = false;
                    if let Some(rev) = rev {
                        let surface = Surface::from(rev);
                        let domain = surface.natural_uv_domain();
                        success =
                            ignored(a.surface_face(brep, surface, domain, &mut face))?.is_some();
                    }
                    if face.is_none() || !success {
                        *slot = None;
                    }
                }
            }
        }
        ignore_status(a.stitch_and_orient(brep))?;
        a.set_editing(brep, false)?;
        Ok(())
    }

    /// Partial rotation and its full-revolution dispatch, axis-interior rejection,
    /// sweep, ordered face deletion and callback stitching.
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
        self.init_entity_maps();
        let region = target(self, brep)?;
        let previous = a.kernel(brep)?.context.doing_boolean();
        a.kernel_mut(brep)?.context.set_doing_boolean(true);
        let result = (|| {
            if options.test_continuity {
                split(a, brep, curves)?;
            }
            if angle_deg >= 359.99999 {
                return self.create_rotational_sweep(a, brep, curves, axis_base, axis);
            }
            if options.cap_ends && options.repetitions != 1 {
                return Err(SolidError::sm_err(OP));
            }
            let tolerance = a.brep(brep)?.tolerance();
            a.set_editing(brep, true)?;
            let mut faces = Vec::new();
            let mut edges = Vec::new();
            if ignored(a.planar_faces(region, curves, tolerance, &mut faces))?.is_none() {
                for slot in curves.iter_mut() {
                    let Some(edge) = ignored(a.wire(brep, slot))? else {
                        return Err(SolidError::sm_err(OP));
                    };
                    edges.push(edge);
                }
                faces.clear();
            }
            let base = faces.first().copied();
            for &face in &faces {
                edges.extend(a.face_edges((brep, face))?);
            }
            let mut cap = options.cap_ends;
            if let Some(face) = base {
                let bound = a.primitive_face_box((brep, face))?.sphere_bound();
                let direction = axis.unitize()?;
                let param = line_closest_point(axis_base, direction, bound.center);
                let point = axis_base + param * direction;
                let start = point - 2.0 * bound.radius * direction;
                let end = point + 2.0 * bound.radius * direction;
                let axis_curve = Curve::from(BSplineCurve::create_line_segment(3, start, end)?);
                if a.axis_inside_face((brep, face), &axis_curve)? {
                    a.delete_primitive_faces(brep, &[face])?;
                    return Err(SolidError::Upstream {
                        operation: OP,
                        code: 1025,
                        entity: None,
                    });
                }
            } else {
                cap = false;
            }
            let geometry = SweepGeometry::Rotation(RotationalSweepGeometry::new(
                axis_base, axis, angle_deg, tolerance,
            ));
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
                ignore_status(a.delete_primitive_faces(brep, &faces))?;
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
