//! Mounting plates: a perforated planar profile extruded as a separate fabrication body.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Loop2, Plane, Region2, Vec2};

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Expr, Feature, FeatureKind, ModelState, Result};

/// The particular sketch line that produced a supporting frame member. One Frame Member
/// feature can produce many bodies with the same cap roles; a reused name is not the same line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct PlateMember {
    pub feature: crate::FeatureId,
    pub line: String,
}

pub fn plate_member(doc: &Document, state: &ModelState, plane: &crate::PlaneRef) -> Result<Option<PlateMember>> {
    let anchor = crate::face_anchor::plane_face_anchor(plane).ok_or_else(|| invalid("plate support needs a persistent face"))?;
    let source = doc.feature(anchor.target.origin_feature).ok_or_else(|| invalid("plate support producer no longer exists"))?;
    let FeatureKind::FrameMember { names, .. } = &source.kind else { return Ok(None) };
    let ids = crate::frame::member_line_ids(state, &source.kind)?;
    let line = if source.body_names.is_empty() {
        ids.iter().find(|line| names.get(*line).is_some_and(|name| *name == anchor.target.body))
    } else {
        source.body_names.iter().position(|name| *name == anchor.target.body).and_then(|index| ids.get(index))
    }
    .ok_or_else(|| invalid("plate support is no longer the selected frame member"))?;
    Ok(Some(PlateMember { feature: source.id.into(), line: line.clone() }))
}

/// The outline of a mounting plate, centred on its support face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlateShape {
    Rectangle {
        #[walk(length = "Width")]
        width: Expr,
        #[walk(length = "Height")]
        height: Expr,
    },
    Round {
        #[walk(length = "Diameter")]
        diameter: Expr,
    },
}

/// Through holes in the plate only. The rectangular pattern always has four holes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(tag = "pattern", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlateHoles {
    #[default]
    None,
    Rectangle {
        #[walk(length = "Hole Diameter")]
        diameter: Expr,
        #[walk(length = "Horizontal Pitch")]
        pitch_x: Expr,
        #[walk(length = "Vertical Pitch")]
        pitch_y: Expr,
    },
    Circle {
        #[walk(length = "Hole Diameter")]
        diameter: Expr,
        #[walk(length = "Bolt Circle Diameter")]
        pitch_diameter: Expr,
        count: u32,
        #[walk(angle = "Bolt Pattern Angle")]
        angle: Expr,
    },
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

/// Values are bounded here too: saved designs and later parameter edits bypass command checks.
pub fn plate_length(value: f64, positive: bool) -> Result<f64> {
    if !value.is_finite() || value.abs() > 1e6 || (positive && value <= 1e-5) {
        return Err(invalid("plate lengths must be finite, at most 1e6 mm, and sizes greater than 0.00001 mm"));
    }
    Ok(value)
}

/// An exact line/arc region, after validating every hole's clearance from the outline and its
/// neighbours. No Boolean cut is needed, and none of the supporting member is drilled.
pub fn plate_region(shape: &PlateShape, holes: &PlateHoles, mut length: impl FnMut(&Expr) -> Result<f64>, angle: f64) -> Result<Region2> {
    let mut size = |e: &Expr| plate_length(length(e)?, true);
    let (outer, bounds) = match shape {
        PlateShape::Rectangle { width, height } => {
            let (w, h) = (size(width)?, size(height)?);
            (
                Loop2::polygon(&[Vec2::new(-w / 2., -h / 2.), Vec2::new(w / 2., -h / 2.), Vec2::new(w / 2., h / 2.), Vec2::new(-w / 2., h / 2.)]),
                (w, h),
            )
        }
        PlateShape::Round { diameter } => {
            let d = size(diameter)?;
            (Loop2::circle(Vec2::ZERO, d / 2.), (d, d))
        }
    };
    let (diameter, centres) = match holes {
        PlateHoles::None => return Ok(Region2 { outer, holes: Vec::new() }),
        PlateHoles::Rectangle { diameter, pitch_x, pitch_y } => {
            let (d, x, y) = (size(diameter)?, size(pitch_x)? / 2., size(pitch_y)? / 2.);
            (d, vec![Vec2::new(-x, -y), Vec2::new(x, -y), Vec2::new(x, y), Vec2::new(-x, y)])
        }
        PlateHoles::Circle { diameter, pitch_diameter, count, .. } => {
            if !(2..=128).contains(count) {
                return Err(invalid("a bolt circle must have 2…128 holes"));
            }
            if !angle.is_finite() || angle.abs() > 1e6 {
                return Err(invalid("bolt pattern angle must be finite and at most 1e6 radians"));
            }
            let (d, r) = (size(diameter)?, size(pitch_diameter)? / 2.);
            let centres = (0..*count)
                .map(|i| {
                    let a = angle + std::f64::consts::TAU * f64::from(i) / f64::from(*count);
                    Vec2::new(a.cos() * r, a.sin() * r)
                })
                .collect();
            (d, centres)
        }
    };
    let r = diameter / 2.;
    for (i, p) in centres.iter().enumerate() {
        let clearance = match shape {
            PlateShape::Rectangle { .. } => (bounds.0 / 2. - p.x.abs()).min(bounds.1 / 2. - p.y.abs()),
            PlateShape::Round { .. } => bounds.0 / 2. - p.len(),
        };
        if clearance <= r + 1e-5 {
            return Err(invalid("bolt holes must stay strictly inside the plate's perimeter"));
        }
        if centres.iter().take(i).any(|q| p.dist(*q) <= diameter + 1e-5) {
            return Err(invalid("bolt holes must not overlap or touch"));
        }
    }
    Ok(Region2 { outer, holes: centres.into_iter().map(|p| Loop2::circle(p, r).reversed()).collect() })
}

/// Resolve a supported, exact face identity first, then centre on its current area centroid.
/// This also centres over the opening of a hollow member instead of at the clicked wall point.
fn support_plane(doc: &Document, f: &Feature, st: &ModelState, plane: &crate::PlaneRef, member: &Option<PlateMember>) -> Result<Plane> {
    let crate::PlaneRef::Face { anchor: Some(anchor), .. } = plane else {
        return Err(invalid("a mounting plate requires a direct persistent planar face"));
    };
    if &plate_member(doc, st, plane)? != member {
        return Err(invalid("plate support's member identity changed; select its face again"));
    }
    let resolved = doc.resolve_face_anchor(anchor, f.component, st)?;
    let body = st.body(&anchor.target.body).ok_or_else(|| invalid("plate support body no longer exists"))?;
    if body.body.face_count() > 4096 {
        return Err(invalid("plate support exceeds the 4096-face work limit"));
    }
    let (indices, _) = doc.anchored_face_indices(st, body, &anchor.target)?;
    let faces = body.body.faces((body.body.size() * 1e-6).clamp(1e-6, 0.01))?;
    let (mut area, mut centre) = (0., solidcraft_geom::Vec3::ZERO);
    for index in indices {
        let face = faces.get(index).ok_or_else(|| invalid("plate support face no longer exists"))?;
        area += face.area;
        centre += face.centroid * face.area;
    }
    if !area.is_finite() || area <= 1e-10 {
        return Err(invalid("plate support face has no area"));
    }
    let centre = crate::apply_point(&anchor.source_to_author(doc, f.component, body)?, centre / area);
    let origin = centre - resolved.normal() * (centre - resolved.origin).dot(resolved.normal());
    Plane::new(origin, resolved.x, resolved.y).ok_or_else(|| invalid("plate support plane is degenerate"))
}

pub(crate) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FramePlate { plane, member, shape, holes, thickness, offset, x_offset, y_offset, angle } = &f.kind else {
        return Err(invalid("not a mounting plate feature"));
    };
    let length = |e: &Expr| super::eval::val(vals, e, Kind::Length);
    let turn = super::eval::val(vals, angle, Kind::Angle)?;
    if !turn.is_finite() || turn.abs() > 1e6 {
        return Err(invalid("plate angle must be finite and at most 1e6 radians"));
    }
    let pattern_angle = match holes {
        PlateHoles::Circle { angle, .. } => super::eval::val(vals, angle, Kind::Angle)?,
        _ => 0.,
    };
    let region = plate_region(shape, holes, length, pattern_angle)?;
    let t = plate_length(length(thickness)?, true)?;
    let z = plate_length(length(offset)?, false)?;
    let (dx, dy) = (plate_length(length(x_offset)?, false)?, plate_length(length(y_offset)?, false)?);
    let support = support_plane(doc, f, st, plane, member)?;
    let (s, c) = turn.sin_cos();
    let plane = Plane::new(support.origin + support.x * dx + support.y * dy, support.x * c + support.y * s, support.y * c - support.x * s)
        .ok_or_else(|| invalid("plate plane is degenerate"))?;
    let mut bodies = solidcraft_kernel::extrude(&plane, &[region], z, z + t)?;
    let body = bodies.pop().filter(|_| bodies.is_empty()).ok_or_else(|| invalid("plate must be one solid"))?;
    let name = super::eval::new_name(st, f, 0);
    st.bodies.push(crate::ModelBody::traced_new(name, crate::lineage::numbered_tool(body), f.id));
    Ok(())
}
