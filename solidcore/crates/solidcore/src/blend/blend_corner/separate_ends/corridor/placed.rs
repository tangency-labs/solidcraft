//! Exact axis classes and bounded coordinates for placed planar bodies.

mod levels;

use self::levels::Levels;
use super::{cardinal, coords, expansion_span, trim, Arm, Corridor, PointBounds, Segment, Span};
use crate::blend::fillet_section::interval::Ivl;
use crate::curve::line::Line;
use crate::error::Result;
use crate::geom::certified::{next_down, next_up};
use crate::geom::expansion;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::gamma;
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p2 as face2;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

pub(super) struct Placed {
    frame: Frame,
    levels: Levels,
    zone: f64,
}

#[derive(Clone, Copy)]
struct Axis {
    direction: Vec3,
    length_squared: Ivl,
}

pub(super) struct Frame {
    axes: [Axis; 3],
}

fn exact_perpendicular(a: Vec3, b: Vec3) -> bool {
    expansion::dot(a, b).is_some_and(|terms| expansion::is_zero(&terms))
}

fn read_plane(b: &Brep, face: FaceId) -> Result<Option<&Plane>> {
    Ok(match face2::surface(b, face)?.map(Surface::kind) {
        Some(SurfaceKind::Plane(plane)) => Some(plane),
        _ => None,
    })
}

fn bounded_dot(a: Vec3, b: Vec3) -> Option<Ivl> {
    let terms = expansion::dot(a, b)?;
    let span = expansion_span(&terms)?;
    Some(Ivl::new(span.lo, span.hi))
}

impl Axis {
    fn new(direction: Vec3) -> Option<Self> {
        let length_squared = bounded_dot(direction, direction)?;
        (length_squared.lo > 0.0).then_some(Self {
            direction,
            length_squared,
        })
    }

    fn projection(self, point: Point3) -> Option<Ivl> {
        let direction = bounded_dot(point, self.direction)?.div_ulp(self.length_squared.sqrt());
        direction.is_finite().then_some(direction)
    }

    fn coordinate(self, point: Point3) -> Option<Ivl> {
        let coordinate = bounded_dot(point, self.direction)?.div_ulp(self.length_squared);
        coordinate.is_finite().then_some(coordinate)
    }
    fn coordinate_bounds(self, point: PointBounds) -> Ivl {
        let direction = coords(self.direction);
        if self.length_squared == Ivl::from(1.0) {
            if let Some((axis, positive)) = cardinal([0.0; 3], direction) {
                if let Some(&value) = point.get(axis) {
                    return if positive {
                        value
                    } else {
                        Ivl::new(-value.hi, -value.lo)
                    };
                }
            }
        }
        trim::dot(point, direction.map(Ivl::from)).div_ulp(self.length_squared)
    }

    /// The signed line projection relative to the level, in numerator units.
    /// Apply parameter uncertainty to the projected slope. An exactly zero
    /// slope removes all dependence on movement along the line.
    fn edge_residual(self, line: &Line, t: f64, anchor: Point3) -> Option<(Ivl, Ivl)> {
        let (base, origin_size) = trim::relative_origin(line.line_point(), anchor, self.direction)?;
        let slope = expansion::scale(
            &expansion::dot(self.direction, line.line_vector())?,
            line.line_scale(),
        )?;
        let (step, step_error) = if expansion::is_zero(&slope) {
            (0.0, 0.0)
        } else {
            line.exact_step_parameter_at(t)
                .map(|step| (step, 0.0))
                .or_else(|| line.step_param_enclosure(t))?
        };
        let residual = expansion_span(&expansion::add(&base, &expansion::scale(&slope, step)?))?;
        let slope = expansion_span(&slope)?;
        let speed = Ivl::new(slope.lo, slope.hi);
        let parameter_error = speed.mul_ulp(Ivl::new(-step_error, step_error));
        let bounds = Ivl::new(residual.lo, residual.hi).add_ulp(parameter_error);
        if cardinal([0.0; 3], coords(self.direction)).is_some() {
            let level = expansion::dot(self.direction, anchor)?;
            return Some((bounds, cardinal_point_roundoff(&level)?));
        }
        // The origin lives in the cap's frame, and travel contributes only
        // through the already projected slope.
        let travel =
            Ivl::from(speed.mag()).mul_ulp(Ivl::from(step.abs()).add_ulp(Ivl::from(step_error)));
        let roundoff = origin_size.add_ulp(travel).mul_ulp(Ivl::from(gamma(4)));
        roundoff
            .is_finite()
            .then_some((bounds, Ivl::new(-roundoff.hi, roundoff.hi)))
    }
}

/// A cardinal projection of the spatial line may round to the stored cap
/// coordinate. Its rounding cell excludes the next representable value on
/// either side. Trims use the exact cardinal projection independently.
fn cardinal_point_roundoff(level: &[f64]) -> Option<Ivl> {
    let at = expansion::to_f64(level);
    if !at.is_finite() || !expansion::is_zero(&expansion::sub(level, &[at])) {
        return None;
    }
    let half_gap = |gap: f64| {
        let half = gap * 0.5;
        if !half.is_finite() || half == 0.0 {
            0.0
        } else if at.to_bits() & 1 == 0 {
            half
        } else {
            next_down(half)
        }
    };
    Some(Ivl::new(
        -half_gap(next_up(at) - at),
        half_gap(at - next_down(at)),
    ))
}

fn reaches_level(range: Ivl, roundoff: Ivl, level: f64) -> bool {
    let range = range.add_ulp(roundoff);
    range.is_finite() && range.lo <= level && level <= range.hi
}

/// Check both representations against the same cap. A spatial edge may
/// also inherit the rounding of the face chart at their shared seam.
fn reaches_cap(spatial: (Ivl, Ivl), uv: (Ivl, Ivl)) -> bool {
    let (edge, edge_error) = spatial;
    let (trim, trim_error) = uv;
    reaches_level(trim, trim_error, 0.0) && reaches_level(edge, edge_error.hull(trim_error), 0.0)
}

impl Frame {
    pub(super) fn from_corridor(b: &Brep, c: &Corridor) -> Result<Option<Self>> {
        let [wall, other_wall, floor, ..] = c.faces;
        let (Some(wall), Some(other_wall), Some(floor)) = (
            read_plane(b, wall)?,
            read_plane(b, other_wall)?,
            read_plane(b, floor)?,
        ) else {
            return Ok(None);
        };
        let normals = [wall, other_wall, floor].map(|plane| plane.position().z_axis());
        let [a, b, c] = normals;
        let directions = if exact_perpendicular(a, b)
            && exact_perpendicular(b, c)
            && exact_perpendicular(a, c)
        {
            normals
        } else {
            let Some((pivot, (axis, positive))) =
                normals.into_iter().enumerate().find_map(|(i, normal)| {
                    cardinal([0.0; 3], coords(normal)).map(|cardinal| (i, cardinal))
                })
            else {
                return Ok(None);
            };
            let mut pole = [0.0; 3];
            let Some(dir) = pole.get_mut(axis) else {
                return Ok(None);
            };
            *dir = if positive { 1.0 } else { -1.0 };
            let [px, py, pz] = pole;
            let pole = Vec3::new(px, py, pz);
            let Some(other) = normals
                .into_iter()
                .enumerate()
                .find(|(i, _)| *i != pivot)
                .map(|(_, normal)| normal)
            else {
                return Ok(None);
            };
            let mut tangent = coords(other);
            let Some(along_pole) = tangent.get_mut(axis) else {
                return Ok(None);
            };
            *along_pole = 0.0;
            let [tx, ty, tz] = tangent;
            let tangent = Vec3::new(tx, ty, tz);
            [pole, tangent, pole.cross(tangent)]
        };
        let [Some(a), Some(b), Some(c)] = directions.map(Axis::new) else {
            return Ok(None);
        };
        let frame = Self { axes: [a, b, c] };

        Ok(Some(frame))
    }

    /// Identify the common axis of the plane. Edges and lifted UV trims must
    /// subsequently certify the entire face boundary against this level.
    pub(super) fn plane_coordinate(
        &self,
        plane: &Plane,
        domain: &Extent2d,
    ) -> Option<(usize, Ivl, Ivl)> {
        let frame = plane.position();
        let normal = frame.z_axis();
        let bounds = self.axes.map(|axis| axis.projection(normal));
        let [Some(x), Some(y), Some(z)] = bounds else {
            return None;
        };
        let bounds = [x, y, z];
        let (i, basis) = self.axes.iter().enumerate().find(|(i, _)| {
            let Some(projection) = bounds.get(*i).copied() else {
                return false;
            };
            let lower = if projection.lo > 0.0 {
                projection.lo
            } else {
                -projection.hi
            };
            lower > 0.0
                && bounds
                    .iter()
                    .enumerate()
                    .all(|(j, other)| *i == j || lower > other.mag())
        })?;
        let level = basis.coordinate(frame.origin())?;
        // Project the affine chart before enclosing the domain. Projecting
        // an XYZ box would lose the correlation along each tangent.
        let project = |direction, scale, domain: crate::geom::extent1d::Extent1d| {
            let slope = bounded_dot(direction, basis.direction)?
                .div_ulp(basis.length_squared)
                .mul_ulp(Ivl::from(scale));
            Some(slope.mul_ulp(Ivl::new(domain.min(), domain.max())))
        };
        let u = project(frame.x_axis(), plane.uv_scale().x, domain.u_interval())?;
        let v = project(frame.y_axis(), plane.uv_scale().y, domain.v_interval())?;
        let range = level.add_ulp(u).add_ulp(v);
        range.is_finite().then_some((i, level, range))
    }

    pub(super) fn world(&self, [x, y, z]: [f64; 3]) -> PointBounds {
        self.world_bounds([x, y, z].map(Ivl::from))
    }

    fn world_bounds(&self, local: PointBounds) -> PointBounds {
        local
            .into_iter()
            .zip(self.axes)
            .fold([Ivl::from(0.0); 3], |[u, v, w], (scale, axis)| {
                let dir = axis.direction;
                [
                    u.add_ulp(scale.mul_ulp(Ivl::from(dir.x))),
                    v.add_ulp(scale.mul_ulp(Ivl::from(dir.y))),
                    w.add_ulp(scale.mul_ulp(Ivl::from(dir.z))),
                ]
            })
    }

    /// Enclose the physical distance between two parallel levels of an axis.
    pub(super) fn plane_gap(&self, axis: usize, exact: Ivl, level: f64) -> Option<f64> {
        let length = self.axes.get(axis)?.length_squared.sqrt();
        let gap = exact.sub_ulp(Ivl::from(level)).mul_ulp(length);
        gap.is_finite().then_some(gap.mag())
    }

    /// A width enclosing the requested physical sweep on each of the 3 axes.
    pub(super) fn width(&self, physical: f64) -> Option<f64> {
        let mut width = 0.0_f64;
        for axis in self.axes {
            let bound = Ivl::from(physical).div_ulp(axis.length_squared.sqrt());
            if !bound.is_finite() {
                return None;
            }
            width = width.max(bound.hi);
        }
        Some(width)
    }
}

impl Placed {
    pub(super) fn read(k: &Kernel, b: &Brep, corridor: &Corridor) -> Result<Option<Self>> {
        let Some(frame) = Frame::from_corridor(b, corridor)? else {
            return Ok(None);
        };
        if frame
            .axes
            .iter()
            .all(|axis| cardinal([0.0; 3], coords(axis.direction)).is_some())
        {
            // The ordinary chart keeps stricter exact rail coordinates.
            return Ok(None);
        }
        let zone = b.tolerance();
        if !zone.is_finite() || zone <= 0.0 {
            return Ok(None);
        }
        let Some(levels) = Levels::build(k, b, &frame)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            frame,
            levels,
            zone,
        }))
    }

    pub(super) fn point(&self, b: &Brep, v: VertexId) -> Result<Option<Point3>> {
        let Some(local) = self.levels.vertex(v) else {
            return Ok(None);
        };
        let world = self.frame.world(local);
        let [x, y, z] = local;
        let bound = trim::stored_vertex_in_zone(b, v, world, self.zone)?;

        Ok(bound.then_some(Point3::new(x, y, z)))
    }

    pub(super) fn bound_trim(
        &self,
        b: &Brep,
        eu: EdgeuseId,
        e: EdgeId,
        plane: &Plane,
        line: &Line,
        [u, v]: [VertexId; 2],
        [a, z]: [Point3; 2],
    ) -> Result<Option<Segment>> {
        // Only locally resolved support levels constrain this trim.
        let (from, to) = (coords(a), coords(z));
        let Some((axis, _)) = cardinal(from, to) else {
            return Ok(None);
        };
        let (Some(&from), Some(&to), Some(margin)) =
            (from.get(axis), to.get(axis), self.foreign_margin())
        else {
            return Ok(None);
        };
        if Ivl::from(from.max(to)).sub_ulp(Ivl::from(from.min(to))).lo <= margin {
            return Ok(None);
        }
        trim::bound_affine_trim(
            b,
            eu,
            e,
            plane,
            line,
            [u, v],
            [a, z].map(|p| self.frame.world(coords(p))),
            self.zone,
        )
    }

    /// Keep the entire lifted segment except where a shared intrinsic UV
    /// boundary certifies contact with an open side of this rectangle.
    pub(super) fn support_trim(
        &self,
        k: &Kernel,
        b: &Brep,
        arm: Arm,
        (face, eu, e, line): (FaceId, EdgeuseId, EdgeId, &Line),
        endpoints: [(VertexId, Point3); 2],
        (raw, spatial): (Segment, Option<Segment>),
    ) -> Result<Option<Segment>> {
        let [wall, floor] = arm.rail_faces;
        let [near_a, near_z] = arm.near_caps;
        let opening = spatial.is_some();
        let checks_caps = arm.rail_faces.contains(&face) || opening;
        let (Some(&axial), Some(&near)) = (arm.spans.get(arm.along), arm.origin.get(arm.along))
        else {
            return Ok(None);
        };
        let far = if near == axial.lo { axial.hi } else { axial.lo };
        let boundaries = [
            (wall, arm.across, arm.origin.get(arm.across).copied()),
            (floor, arm.above, arm.origin.get(arm.above).copied()),
            (near_a, arm.along, checks_caps.then_some(near)),
            (near_z, arm.along, checks_caps.then_some(near)),
            (arm.far_cap, arm.along, checks_caps.then_some(far)),
        ];
        let Some(plane) = read_plane(b, face)? else {
            return Ok(None);
        };
        let interval = edge::interval(b, e)?;
        let project = |point| self.frame.axes.map(|axis| axis.coordinate_bounds(point));
        let mut projected = raw.map(project);
        // The opposite opening wall is foreign to this arm. Keep both
        // representations until a shared cap certifies their contact.
        if let Some(spatial) = spatial {
            for (lifted, spatial) in projected.iter_mut().zip(spatial.map(project)) {
                for (lifted, spatial) in lifted.iter_mut().zip(spatial) {
                    *lifted = lifted.hull(spatial);
                }
            }
        }
        let uv = trim::uv_terms(b, eu)?;
        let mut regenerated = None;
        for (end, ((bounds, (vertex, nominal)), parameter)) in projected
            .iter_mut()
            .zip(endpoints)
            .zip([interval.min(), interval.max()])
            .enumerate()
        {
            for (other, axis, level) in boundaries {
                let Some(level) = level else {
                    continue;
                };
                if other == face || coords(nominal).get(axis) != Some(&level) {
                    continue;
                }
                let Some(value) = bounds.get_mut(axis) else {
                    return Ok(None);
                };
                let Some(cap) = read_plane(b, other)? else {
                    continue;
                };
                let cap = cap.position();
                let Some(basis) = Axis::new(cap.z_axis()) else {
                    continue;
                };
                let Some((spatial, rounding)) = basis.edge_residual(line, parameter, cap.origin())
                else {
                    continue;
                };
                let Some(lifted) = uv.as_ref().and_then(|uv| {
                    trim::projected_uv_ends(plane, uv, basis.direction, cap.origin())
                }) else {
                    continue;
                };
                let Some(&(lifted, uv_rounding)) = lifted.get(end) else {
                    return Ok(None);
                };
                if !reaches_cap((spatial, rounding), (lifted, uv_rounding)) {
                    continue;
                }
                // The opposite opening wall is already identified by the
                // corner fan. An enclosed common axial or side endpoint
                // only establishes contact at that face of the arm.
                if opening && axis != arm.above {
                    *value = Ivl::from(level);
                    continue;
                }
                let same_support = |peer| self.levels.same_support(peer, other);
                let mut proof = trim::on_iso_seam(k, b, face, plane, eu, vertex, same_support)?;
                if !proof && trim::touches_support(b, eu, vertex, same_support)? {
                    if regenerated.is_none() {
                        regenerated = Some(trim::reprojects(k, b, eu, face, plane)?);
                    }
                    proof = regenerated == Some(true);
                }
                if proof {
                    *value = Ivl::from(level);
                }
            }
        }
        Ok(projected
            .iter()
            .flatten()
            .all(|bound| bound.is_finite())
            .then_some(projected))
    }

    pub(super) fn width(&self, physical: f64) -> Option<f64> {
        // An edge and its adjoining skin can each deviate from the shared levels
        // by one existing zone. Budget both deviations at the outer boundary.
        let physical = Ivl::from(physical).add_ulp(Ivl::from(self.zone).mul_ulp(Ivl::from(2.0)));
        self.frame.width(physical.hi)
    }

    /// Outward allowance for a foreign boundary on each placed axis.
    pub(super) fn foreign_margin(&self) -> Option<f64> {
        self.frame.width(2.0 * self.zone)
    }

    /// Enclose both representations on every side. Contacts of an unrelated
    /// face at a shared level remain ambiguous. The opposite opening wall is
    /// identified by the corner fan; it may share the start of the prism when
    /// its whole boundary follows the opposite side of both incident trims.
    pub(super) fn foreign_contact(
        &self,
        arm: Arm,
        bounds: [Span; 3],
        opening_trim: Option<[Span; 3]>,
    ) -> Option<bool> {
        if arm.contacts(bounds)? {
            return Some(true);
        }
        let margin = self.foreign_margin()?;
        let meets = arm.spans.into_iter().zip(bounds).all(|(span, boundary)| {
            let total = Ivl::new(boundary.lo, boundary.hi).add_ulp(Ivl::new(-margin, margin));
            total.lo <= span.hi && span.lo <= total.hi
        });
        if !meets {
            return Some(false);
        }
        // A nominal contact is not enough to clear a margin-expanded opening.
        let actual = opening_trim?;
        let stops_at_start = |axis: usize| {
            let start = *arm.origin.get(axis)?;
            let swept = arm.spans.get(axis)?;
            let shape = actual.get(axis)?;
            Some(if start == swept.lo {
                shape.hi <= start
            } else {
                shape.lo >= start
            })
        };
        let side = *arm.origin.get(arm.across)?;
        let across = bounds.get(arm.across)?;
        let actual_across = actual.get(arm.across)?;
        let on_side = actual_across.lo == side && actual_across.hi == side;
        (across.lo == side
            && across.hi == side
            && stops_at_start(arm.along)?
            && (on_side || stops_at_start(arm.above)?))
        .then_some(false)
    }

    pub(super) fn extent(&self, arm: Arm) -> Option<Extent3d> {
        let [x, y, z] = self
            .frame
            .world_bounds(arm.spans.map(|span| Ivl::new(span.lo, span.hi)));
        let pad = |value: Ivl| value.add_ulp(Ivl::new(-self.zone, self.zone));
        let [x, y, z] = [pad(x), pad(y), pad(z)];
        if ![x, y, z].into_iter().all(Ivl::is_finite) {
            return None;
        }
        Some(Extent3d::from_points(
            Point3::new(x.lo, y.lo, z.lo),
            Point3::new(x.hi, y.hi, z.hi),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::BoundedLineOptions;
    use crate::geom::extent1d::Extent1d;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec2;

    #[test]
    fn an_opening_wall_needs_its_actual_trim_to_stop_at_the_start() {
        let axes = [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| Axis::new(axis).unwrap());
        let frame = Frame { axes };
        let levels = Levels::build(&Kernel::default(), &Brep::default(), &frame)
            .unwrap()
            .unwrap();
        let zone = 1e-4;
        let placed = Placed {
            frame,
            levels,
            zone,
        };
        let arm = Arm::new(
            Point3::ZERO,
            Point3::X,
            Point3::Y,
            Point3::Z,
            1.0,
            [FaceId::from_index(0); 5],
        )
        .unwrap();
        let nominal = [
            Span::between(0.0, 0.0),
            Span::between(-2.0, 0.0),
            Span::between(-2.0, 0.0),
        ];
        assert_eq!(placed.foreign_contact(arm, nominal, None), None);
        assert_eq!(
            placed.foreign_contact(arm, nominal, Some(nominal)),
            Some(false)
        );
        let mut advanced = nominal;
        advanced[2].hi = zone / 2.0;
        assert_eq!(placed.foreign_contact(arm, nominal, Some(advanced)), None);
    }

    #[test]
    fn a_long_edge_does_not_hide_an_offset_across_a_cardinal_cap() {
        let length = 2.0_f64.powi(20);
        let line = Line::bounded(
            Point3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.6, 0.8),
            &Extent1d::from_min_max(0.0, length),
            &BoundedLineOptions::default(),
        );
        let axis = Axis::new(Vec3::new(1.0, 0.0, 0.0)).expect("finite cap normal");
        let anchor = Point3::new(1.0, 0.0, 0.0);
        let offset = 2.0_f64.powi(-35);
        assert!(line.point_error(length).expect("finite end error") > offset);
        for t in [0.0, length] {
            let (spatial, error) = axis
                .edge_residual(&line, t, anchor)
                .expect("finite line endpoint");
            assert!(reaches_level(spatial, error, 0.0));
            assert!(!reaches_level(Ivl::from(offset), error, 0.0));
        }
    }

    #[test]
    fn tangential_travel_and_translation_do_not_hide_a_shift_across_the_cap() {
        let length = 2.0_f64.powi(20);
        let offset = 2.0_f64.powi(-35);
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
        let normal = Vec3::new(0.6, 0.8, 0.0);
        for (origin, normal, tangent, parameters) in [
            (
                normal,
                normal,
                Vec3::new(-0.8, 0.6, 0.0),
                [length, length + 1.0],
            ),
            (
                Point3::new(length + 1.0, 1.0 - length, 0.0),
                Vec3::new(diagonal, diagonal, 0.0),
                Vec3::new(diagonal, -diagonal, 0.0),
                [0.0, 1.0],
            ),
        ] {
            let [start, end] = parameters;
            let line = Line::bounded(
                origin,
                tangent,
                &Extent1d::from_min_max(start, end),
                &BoundedLineOptions::default(),
            );
            let plane = test_chart(origin, normal, tangent, end);
            let axis = Axis::new(normal).expect("finite cap normal");
            for (shift, contact) in [(-offset, false), (0.0, true), (offset, false)] {
                let ends = parameters.map(|v| [vec![shift], vec![v]]);
                let lifted = trim::projected_uv_ends(&plane, &ends, normal, origin)
                    .expect("affine trim within the chart");
                for (t, lifted) in parameters.into_iter().zip(lifted) {
                    let spatial = axis
                        .edge_residual(&line, t, origin)
                        .expect("finite line endpoint");
                    assert_eq!(reaches_cap(spatial, lifted), contact, "shift={shift}");
                }
            }
        }
    }

    #[test]
    fn a_nearly_tangential_spatial_edge_cannot_overrun_an_unchanged_uv_cap() {
        let length = 2.0_f64.powi(20);
        let normal = Vec3::new(0.6, 0.8, 0.0);
        let tangent = Vec3::new(-0.8, 0.6, 0.0);
        let line = Line::bounded(
            normal,
            Vec3::new(tangent.x, next_up(tangent.y), 0.0),
            &Extent1d::from_min_max(length, length + 1.0),
            &BoundedLineOptions::default(),
        );
        let plane = test_chart(normal, normal, tangent, length + 1.0);
        let axis = Axis::new(normal).expect("finite cap normal");
        let parameters = [length, length + 1.0];
        let ends = parameters.map(|v| [vec![0.0], vec![v]]);
        let lifted = trim::projected_uv_ends(&plane, &ends, normal, normal)
            .expect("affine trim within the chart");
        for (t, lifted) in parameters.into_iter().zip(lifted) {
            let spatial = axis
                .edge_residual(&line, t, normal)
                .expect("finite line endpoint");
            assert!(!reaches_cap(spatial, lifted));
        }
    }

    fn test_chart(origin: Point3, normal: Vec3, tangent: Vec3, end: f64) -> Plane {
        Plane::new(
            origin,
            normal,
            tangent,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-1.0, 0.0, 1.0, end + 1.0),
        )
    }

    #[test]
    fn a_clamped_trim_must_not_hide_a_bend_between_its_ends() {
        let plane = Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        );
        let epsilon = 0.125;
        let normal = Vec3::new(1.0, 1.0, 0.0);
        let anchor = Point3::new(epsilon, 0.0, 0.0);
        let bent = [
            [vec![-epsilon], vec![epsilon]],
            [vec![epsilon], vec![-epsilon]],
        ];
        assert!(trim::projected_uv_ends(&plane, &bent, normal, anchor).is_none());

        let straight = [
            [vec![-epsilon], vec![epsilon]],
            [vec![epsilon], vec![epsilon]],
        ];
        let [(first, first_error), (last, last_error)] =
            trim::projected_uv_ends(&plane, &straight, normal, anchor)
                .expect("a fixed UV coordinate only shortens the straight segment");
        assert!(reaches_level(first, first_error, 0.0));
        assert!(!reaches_level(last, last_error, 0.0));
    }

    #[test]
    fn either_cardinal_axis_sign_preserves_the_next_value_across_a_cap() {
        let at = 1.0_f64;
        for direction in [1.0, -1.0] {
            let axis = Axis::new(Vec3::new(direction, 0.0, 0.0)).expect("finite cap normal");
            let level = direction * at;
            let no_rounding = Ivl::from(0.0);
            let stored = cardinal_point_roundoff(&[level]).expect("finite stored coordinate");
            for (x, contact) in [(next_down(at), false), (at, true), (next_up(at), false)] {
                let point = [Ivl::from(x), Ivl::from(2.0), Ivl::from(3.0)];
                assert_eq!(
                    reaches_level(axis.coordinate_bounds(point), no_rounding, level),
                    contact,
                    "direction={direction}, x={x:?}"
                );
                let residual = Ivl::from(direction * x - level);
                assert_eq!(reaches_level(residual, stored, 0.0), contact);
            }
            let inside_rounding_cell = direction * (next_up(at) - at) * 0.25;
            assert!(reaches_level(Ivl::from(inside_rounding_cell), stored, 0.0));
        }
    }
}
