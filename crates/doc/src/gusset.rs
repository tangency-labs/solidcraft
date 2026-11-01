//! Attached fabrication gussets, composed from existing planar extrusion operations.
use std::collections::BTreeMap;

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Expr, Feature, FeatureKind, ModelState, PlaneRef, Result};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub feature: crate::FeatureId,
    pub line: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Support {
    pub plane: PlaneRef,
    pub member: Option<Member>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Center,
    Positive,
    Negative,
}

fn invalid(s: impl Into<String>) -> DocError {
    DocError::Invalid(s.into())
}

pub fn member(doc: &Document, st: &ModelState, plane: &PlaneRef) -> Result<Option<Member>> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = plane else { return Err(invalid("gusset supports require direct persistent planar faces")) };
    let source = doc.feature(anchor.target.origin_feature).ok_or_else(|| invalid("gusset support producer no longer exists"))?;
    let FeatureKind::FrameMember { names, .. } = &source.kind else { return Ok(None) };
    let ids = crate::frame::member_line_ids(st, &source.kind)?;
    let line = if source.body_names.is_empty() {
        ids.iter().find(|line| names.get(*line).is_some_and(|name| *name == anchor.target.body))
    } else {
        source.body_names.iter().position(|name| *name == anchor.target.body).and_then(|i| ids.get(i))
    }
    .ok_or_else(|| invalid("gusset support is no longer the selected member"))?;
    Ok(Some(Member { feature: source.id.into(), line: line.clone() }))
}

struct Face {
    plane: Plane,
    triangles: Vec<[Vec3; 3]>,
}
fn face(doc: &Document, st: &ModelState, f: &Feature, support: &Support) -> Result<Face> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = &support.plane else { return Err(invalid("gusset support must be an attached face")) };
    if member(doc, st, &support.plane)? != support.member {
        return Err(invalid("gusset support's member identity changed"));
    }
    let plane = doc.resolve_face_anchor(anchor, f.component, st)?;
    let body = st.body(&anchor.target.body).ok_or_else(|| invalid("gusset support body disappeared"))?;
    if body.body.face_count() > 4096 {
        return Err(invalid("gusset support exceeds the 4096-face limit"));
    }
    let (indices, _) = doc.anchored_face_indices(st, body, &anchor.target)?;
    let transform = anchor.source_to_author(doc, f.component, body)?;
    let mesh = body.body.tessellate((body.body.size() * 1e-5).clamp(1e-5, 0.05))?;
    if mesh.triangles.len() > 131072 {
        return Err(invalid("gusset support exceeds the 131072-triangle limit"));
    }
    let mut triangles = Vec::new();
    for (tri, index) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if indices.contains(&(*index as usize))
            && let Some(points) = mesh.tri(tri)
        {
            triangles.push(points.map(|p| crate::apply_point(&transform, p)));
        }
    }
    if triangles.is_empty() {
        return Err(invalid("gusset support has no material"));
    }
    Ok(Face { plane, triangles })
}

fn length(v: f64, positive: bool) -> Result<f64> {
    if !v.is_finite() || v.abs() > 1e6 || (positive && v < 0.01) {
        return Err(invalid("gusset sizes must be 0.01…1e6 mm; offsets must be finite and within 1e6 mm"));
    }
    Ok(v)
}

/// Clip a planar face triangle to a contact rectangle. Summing clipped areas verifies the
/// entire contact strip, including holes and concave boundaries; endpoint picks alone cannot.
pub(crate) fn clipped_area(triangle: [Vec2; 3], x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    let mut polygon = triangle.to_vec();
    for (axis, bound, sign) in [(0, x0, 1.), (0, x1, -1.), (1, y0, 1.), (1, y1, -1.)] {
        let coordinate = |p: Vec2| if axis == 0 { p.x } else { p.y };
        let mut result = Vec::new();
        if let Some(mut previous) = polygon.last().copied() {
            for current in &polygon {
                let (a, b) = ((coordinate(previous) - bound) * sign, (coordinate(*current) - bound) * sign);
                if (a >= 0.) != (b >= 0.) {
                    result.push(previous + (*current - previous) * (a / (a - b)));
                }
                if b >= 0. {
                    result.push(*current);
                }
                previous = *current;
            }
        }
        polygon = result;
    }
    let mut twice = 0.;
    if let Some(mut previous) = polygon.last().copied() {
        for p in polygon {
            twice += previous.x * p.y - previous.y * p.x;
            previous = p;
        }
    }
    twice.abs() * 0.5
}

fn contact(face: &Face, origin: Vec3, leg: Vec3, direction: Vec3, from: f64, to: f64, start: f64, end: f64) -> Result<()> {
    let area: f64 = face
        .triangles
        .iter()
        .map(|triangle| {
            clipped_area(
                triangle.map(|p| {
                    let r = p - origin;
                    Vec2::new(r.dot(leg), r.dot(direction))
                }),
                from,
                to,
                start,
                end,
            )
        })
        .sum();
    let expected = (to - from) * (end - start);
    if !area.is_finite() || (area - expected).abs() > expected * 1e-6 + 1e-8 {
        return Err(invalid("gusset contact must fit entirely on both selected faces; reduce the legs/thickness or change the joint offset"));
    }
    Ok(())
}

pub(crate) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameGusset { supports, leg1, leg2, thickness, relief, clip, offset, placement } = &f.kind else {
        return Err(invalid("not a gusset"));
    };
    let [a, b] = supports.as_slice() else { return Err(invalid("a gusset requires exactly two supports")) };
    if a.plane == b.plane {
        return Err(invalid("select two different support faces"));
    }
    let (a, b) = (face(doc, st, f, a)?, face(doc, st, f, b)?);
    let value = |e: &Expr, positive| length(crate::eval::val(vals, e, Kind::Length)?, positive);
    let (l1, l2, t) = (value(leg1, true)?, value(leg2, true)?, value(thickness, true)?);
    let (r, c, offset) = (value(relief, false)?, value(clip, false)?, value(offset, false)?);
    if r < 0. || c < 0. || r + c >= l1 || r + c >= l2 {
        return Err(invalid("relief and clipping must be nonnegative and together shorter than each leg"));
    }
    let (n1, n2) = (a.plane.normal(), b.plane.normal());
    let dot = n1.dot(n2);
    let denom = 1. - dot * dot;
    if !denom.is_finite() || denom < 1e-8 {
        return Err(invalid("gusset supports must not be parallel or nearly parallel"));
    }
    let direction = n1.cross(n2).normalized().ok_or_else(|| invalid("gusset joint has no direction"))?;
    let u = (n2 - n1 * dot).normalized().ok_or_else(|| invalid("gusset first leg is degenerate"))?;
    let v = (n1 - n2 * dot).normalized().ok_or_else(|| invalid("gusset second leg is degenerate"))?;
    let (h1, h2) = (n1.dot(a.plane.origin), n2.dot(b.plane.origin));
    let mut origin = n1 * ((h1 - dot * h2) / denom) + n2 * ((h2 - dot * h1) / denom);
    let interval = |face: &Face| {
        face.triangles.iter().flatten().map(|p| p.dot(direction)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let (ia, ib) = (interval(&a), interval(&b));
    let (lo, hi) = (ia.0.max(ib.0), ia.1.min(ib.1));
    if hi - lo < 0.01 {
        return Err(invalid("support faces have no common joint span"));
    }
    origin += direction * ((lo + hi) * 0.5 - origin.dot(direction) + offset);
    if !origin.is_finite() || origin.len() > 1e9 {
        return Err(invalid("gusset joint exceeds the coordinate limit"));
    }
    let (start, end) = match placement {
        Placement::Center => (-t / 2., t / 2.),
        Placement::Positive => (0., t),
        Placement::Negative => (-t, 0.),
    };
    contact(&a, origin, u, direction, r, l1 - c, start, end)?;
    contact(&b, origin, v, direction, r, l2 - c, start, end)?;
    let plane = Plane::new(origin, u, direction.cross(u)).ok_or_else(|| invalid("gusset plane is degenerate"))?;
    let (p1, p2) = (u * l1, v * l2);
    let diagonal = p2 - p1;
    let distance = diagonal.len();
    if distance <= 2. * c {
        return Err(invalid("outer clipping consumes the gusset diagonal"));
    }
    let mut points = vec![u * r];
    if c > 0. {
        points.extend([p1 - u * c, p1 + diagonal * (c / distance), p2 - diagonal * (c / distance), p2 - v * c]);
    } else {
        points.extend([p1, p2]);
    }
    if r > 0. {
        points.push(v * r);
    }
    let mut points: Vec<Vec2> = points.into_iter().map(|p| Vec2::new(p.dot(plane.x), p.dot(plane.y))).collect();
    // The ordered support normals define thickness direction, independently of outline winding.
    points.reverse();
    let mut outer = Loop2::polygon(&points);
    if outer.signed_area() < 0. {
        outer = outer.reversed();
    }
    if outer.signed_area() < 1e-6 {
        return Err(invalid("gusset profile has no usable area"));
    }
    let mut bodies = solidcraft_kernel::extrude(&plane, &[Region2 { outer, holes: Vec::new() }], start, end)?;
    let body = bodies.pop().filter(|_| bodies.is_empty()).ok_or_else(|| invalid("gusset must be one solid"))?;
    let name = crate::eval::new_name(st, f, 0);
    st.bodies.push(crate::ModelBody::traced_new(name, crate::lineage::numbered_tool(body), f.id));
    Ok(())
}
