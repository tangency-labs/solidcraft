//! Exact axis classes and bounded coordinates for placed planar bodies.

mod levels;

use self::levels::Levels;
use super::{cardinal, coords, expansion_span, trim, Arm, Corridor, PointBounds, Segment};
use crate::blend::fillet_section::interval::Ivl;
use crate::curve::line::Line;
use crate::error::Result;
use crate::geom::expansion;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
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
        trim::dot(point, coords(self.direction).map(Ivl::from)).div_ulp(self.length_squared)
    }

    fn contact_roundoff(self, error: PointBounds) -> Ivl {
        // A cardinal projection can use the actual Cartesian enclosures.
        // A placed axis needs the rounding from this endpoint in its own chart.
        if cardinal([0.0; 3], coords(self.direction)).is_some() {
            Ivl::from(0.0)
        } else {
            self.coordinate_bounds(error)
        }
    }
}

fn reaches_level(range: Ivl, roundoff: Ivl, level: f64) -> bool {
    let range = range.add_ulp(roundoff);
    range.is_finite() && range.lo <= level && level <= range.hi
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
    pub(super) fn plane_coordinate(&self, plane: &Plane) -> Option<(usize, Ivl)> {
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
        Some((i, basis.coordinate(frame.origin())?))
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
        raw: Segment,
    ) -> Result<Option<Segment>> {
        let [wall, floor] = arm.rail_faces;
        let [near_a, near_z] = arm.near_caps;
        let on_rail = arm.rail_faces.contains(&face);
        let (Some(&axial), Some(&near)) = (arm.spans.get(arm.along), arm.origin.get(arm.along))
        else {
            return Ok(None);
        };
        let far = if near == axial.lo { axial.hi } else { axial.lo };
        let boundaries = [
            (wall, arm.across, arm.origin.get(arm.across).copied()),
            (floor, arm.above, arm.origin.get(arm.above).copied()),
            (near_a, arm.along, on_rail.then_some(near)),
            (near_z, arm.along, on_rail.then_some(near)),
            (arm.far_cap, arm.along, on_rail.then_some(far)),
        ];
        let Some(plane) = read_plane(b, face)? else {
            return Ok(None);
        };
        let interval = edge::interval(b, e)?;
        let [Some(first), Some(last)] = [interval.min(), interval.max()]
            .map(|parameter| trim::line_end_with_error(line, parameter))
        else {
            return Ok(None);
        };
        let project = |point| self.frame.axes.map(|axis| axis.coordinate_bounds(point));
        let mut projected = raw.map(project);
        let mut regenerated = None;
        for ((bounds, (vertex, nominal)), (spatial, error)) in
            projected.iter_mut().zip(endpoints).zip([first, last])
        {
            let spatial = project(spatial);
            for (other, axis, level) in boundaries {
                let Some(level) = level else {
                    continue;
                };
                if other == face || coords(nominal).get(axis) != Some(&level) {
                    continue;
                }
                let (Some(value), Some(spatial), Some(basis)) = (
                    bounds.get_mut(axis),
                    spatial.get(axis),
                    self.frame.axes.get(axis),
                ) else {
                    return Ok(None);
                };
                // Both representations must reach the level at this endpoint.
                let rounding = basis.contact_roundoff(error);
                if !reaches_level(*value, rounding, level)
                    || !reaches_level(*spatial, rounding, level)
                {
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
        bounds: [super::Span; 3],
        opening_wall: bool,
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
        let stops_at_start = |axis: usize| {
            let start = *arm.origin.get(axis)?;
            let swept = arm.spans.get(axis)?;
            let shape = bounds.get(axis)?;
            Some(if start == swept.lo {
                shape.hi <= start
            } else {
                shape.lo >= start
            })
        };
        let side = *arm.origin.get(arm.across)?;
        let across = bounds.get(arm.across)?;
        (opening_wall
            && across.lo == side
            && across.hi == side
            && stops_at_start(arm.along)?
            && stops_at_start(arm.above)?)
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
        let offset = 2.0_f64.powi(-35);
        assert!(line.point_error(length).expect("finite end error") > offset);
        for t in [0.0, length] {
            let (spatial, error) =
                trim::line_end_with_error(&line, t).expect("finite line endpoint");
            let rounding = axis.contact_roundoff(error);
            assert!(reaches_level(
                axis.coordinate_bounds(spatial),
                rounding,
                1.0
            ));
            assert!(!reaches_level(Ivl::from(1.0 + offset), rounding, 1.0));
        }
    }
}
