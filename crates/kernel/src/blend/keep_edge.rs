//! The "keep edge" round: a concave round taller than the wall it rises on, ending on the wall's
//! top edge instead of running on past it.
//!
//! A rolling ball of radius `R` between a floor and a wall too low to reach its contact line (for a
//! square wall, `h < R`) cannot touch the
//! wall (its tangent point on the wall's plane is above the wall), so the ordinary round carries
//! the wall's plane on and grows the top face to meet it (the carried-on round). The keep-edge
//! round keeps the top face and its edge as they are: the round's cylinder is tangent to the
//! floor only and passes through the wall's top edge. In the section square to a straight edge,
//! with the floor along x, the wall's top edge at `T = (tx, h)` and the wall rising from the
//! origin, the section is the arc of the circle of radius `R` centred at `(tx + c, R)`,
//! `c = sqrt(R^2 - (R - h)^2)`, from its lowest point `(tx + c, 0)` round to `T`: the round's
//! surface is that cylinder, trimmed by the top face's plane at the top edge (they meet there).
//! The material it adds is the prism of that section (floor, arc, wall) along the edge, joined to
//! the body. Exact geometry only: an extruded profile of lines and an arc, and booleans.
//!
//! Supported: a straight concave edge between two flat faces, the wall's outline a quadrilateral
//! with a straight top edge parallel to the base, and at each end of the edge either a flat face
//! square to it (the prism ends there) or an upright face (flat or a cylinder about the floor's
//! normal) that bounds the floor, such as a ring's own cylinders: the prism runs on past that end
//! and is kept within the floor's outline.

use solidcore_ops::keep_edge as geo;
use solidcore_record::brep::Surface;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use super::{Faces, closed, over_body_alone, v3};
use crate::body::Body;
use crate::{KernelError, Result};

/// Tolerance, relative to the body's size, on straightness, squareness and heights.
const REL: f64 = 1e-7;

fn refuse(why: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("keep-edge round: {why}"))
}

/// A point or direction as the kernel's measuring functions take it.
fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// How the prism ends at one end of the edge.
#[derive(Clone, Copy, PartialEq)]
enum End {
    /// A flat face square to the edge: the prism ends in its plane.
    Square,
    /// An upright face bounding the floor: the prism runs on and is kept within the floor's outline.
    Upright,
}

/// One edge's added material, with its record edge index.
struct Fill {
    solid: Body,
}

/// The constant-radius keep-edge round of the edges nearest `edges`.
pub fn fillet_keep_edge(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_keep_edge");
    body.require_brep("keep-edge round")?;
    if !(radius.is_finite() && radius > 1e-6 && radius < 1e6) {
        return Err(KernelError::Invalid("keep-edge round size must be positive and finite".into()));
    }
    if edges.is_empty() || edges.len() > 1000 || edges.iter().any(|p| ![p.x, p.y, p.z].into_iter().all(f64::is_finite)) {
        return Err(KernelError::Invalid("keep-edge round needs 1 to 1000 finite edge points".into()));
    }
    let (mut picked, _) = crate::blend_smlib::picked_edges(body, edges)?;
    picked.sort_unstable();
    picked.dedup();
    if picked.is_empty() {
        return Err(refuse("no edge picked"));
    }
    let rec = Faces::of(body)?;
    let mut fills = Vec::with_capacity(picked.len());
    // The picks that are rounded as usual (a convex edge, a wall as high as the radius).
    let mut usual = Vec::new();
    for &e in &picked {
        match fill_of(body, &rec, e, radius)? {
            Some(f) => fills.push(f),
            None => usual.push(
                // A point ON the edge's curve (its parameter midpoint), not the chord midpoint: the
                // chord of a curved or split-circle edge is off the edge and names another one.
                super::edge_middle(&rec, e).ok_or_else(|| refuse("a point on a picked edge"))?,
            ),
        }
    }
    let mut out = body.clone();
    for f in &fills {
        out = crate::boolean(&out, &f.solid, crate::BoolOp::Union)?.ok_or_else(|| refuse("joining the added material gave no body"))?;
    }
    if !fills.is_empty() {
        let out_closed = closed(body, out)?;
        let (before, after) = (crate::measure(body)?.volume, crate::measure(&out_closed)?.volume);
        if after <= before {
            return Err(refuse("it adds no material"));
        }
        out = out_closed;
    }
    if !usual.is_empty() {
        out = super::fillet(&out, &usual, radius)?.without_history();
        return Ok(out);
    }
    Ok(match picked.as_slice() {
        [e] => over_body_alone(out, body, *e),
        _ => out.without_history(),
    })
}

/// The material `radius` adds along record edge `e`; `None` where the edge is rounded as usual (not
/// concave between flat faces, or its wall not lower than the radius).
fn fill_of(body: &Body, rec: &Faces, e: usize, radius: f64) -> Result<Option<Fill>> {
    let tol = (body.size() * REL).max(1e-9);
    let Some((a, b)) = rec.ends(e) else { return Ok(None) };
    let faces: Vec<usize> = rec.faces_of(e).into_iter().map(|f| f as usize).collect();
    let [f0, f1] = faces.as_slice() else { return Ok(None) };
    if !rec.straight(e) {
        return Ok(None);
    }
    let Some(along) = geo::unit_between(arr(a), arr(b)).map(|u| Vec3::new(u[0], u[1], u[2])) else { return Ok(None) };
    let mut why = None;
    // Either face may be the wall: the one that is lower than the radius.
    for (wall, floor) in [(*f0, *f1), (*f1, *f0)] {
        match with_wall(body, rec, e, wall, floor, (a, b, along), radius, tol) {
            Ok(Some(fill)) => return Ok(Some(fill)),
            Ok(None) => {}
            Err(KernelError::Failed(w)) => why = Some(w),
            Err(other) => return Err(other),
        }
    }
    match why {
        Some(w) => Err(KernelError::Failed(w)),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
fn with_wall(body: &Body, rec: &Faces, e: usize, wall: usize, floor: usize, ends: (Vec3, Vec3, Vec3), radius: f64, tol: f64) -> Result<Option<Fill>> {
    let (a, b, along) = ends;
    let use_in = |f: usize| rec.uses(f).find(|u| u.edge as usize == e).ok_or_else(|| refuse("the edge is not in its face's loop"));
    let (Some(nf), Some(nw)) = (rec.plane_normal(floor), rec.plane_normal(wall)) else { return Ok(None) };
    // Square to the edge, into each face.
    let f_dir = rec.direction_into_face(floor, use_in(floor)?).ok_or_else(|| refuse("the floor's direction"))?;
    let w_dir = rec.direction_into_face(wall, use_in(wall)?).ok_or_else(|| refuse("the wall's direction"))?;
    // Concave: the wall rises from the floor on its outer side, and the floor runs off the wall's.
    let Some((wx, wy)) = geo::wall_frame(arr(f_dir), arr(w_dir), arr(nf), arr(nw)) else { return Ok(None) };
    // The ordinary round (a ball tangent to both faces) touches the wall at a distance
    // R / tan(a / 2) from the edge, a being the angle between the faces (cos a = wx), which is a
    // height R (1 + wx) up the wall (tan(a / 2) = wy / (1 + wx)). Where the wall is at least that
    // high the contact line is on it and the ordinary round fits; keep-edge changes nothing there.
    let reach = radius * (1.0 + wx);
    let mut tallest = 0.0_f64;
    for u in rec.uses(wall) {
        for v in rec.rec.edges.get(u.edge as usize).map(|x| x.vertices).unwrap_or_default() {
            if let Some(p) = rec.point(v) {
                tallest = tallest.max(geo::wall_height(arr(a), arr(p), arr(w_dir), wy));
            }
        }
    }
    if tallest >= reach * (1.0 - 1e-9) {
        return Ok(None);
    }
    // The wall's outline: a quadrilateral, the edge, two sides and a top edge parallel to it.
    let uses: Vec<_> = rec.uses(wall).collect();
    if uses.len() != 4 || uses.iter().any(|u| !rec.straight(u.edge as usize)) {
        return Err(refuse("the wall must be a quadrilateral with a straight top edge"));
    }
    let mut top = Vec::new();
    let mut sides = [None, None];
    let [va, vb] = rec.rec.edges.get(e).ok_or_else(|| refuse("the edge"))?.vertices;
    for u in &uses {
        let k = u.edge as usize;
        if k == e {
            continue;
        }
        let [p, q] = rec.rec.edges.get(k).ok_or_else(|| refuse("the wall's edge"))?.vertices;
        for (slot, v) in [(0usize, va), (1, vb)] {
            if (p == v || q == v)
                && let Some(s) = sides.get_mut(slot)
            {
                if s.is_some() {
                    return Err(refuse("the wall must be a quadrilateral with a straight top edge"));
                }
                *s = Some(k);
            }
        }
        if p != va && p != vb && q != va && q != vb {
            top.push(k);
        }
    }
    let ([Some(side_a), Some(side_b)], [t]) = (sides, top.as_slice()) else {
        return Err(refuse("the wall must be a quadrilateral with a straight top edge"));
    };
    let (t0, t1) = rec.ends(*t).ok_or_else(|| refuse("the top edge"))?;
    let top = geo::top_edge(arr(a), arr(t0), arr(t1), arr(w_dir), arr(along)).ok_or_else(|| refuse("the top edge"))?;
    let length = top.length;
    if (length - top.level).abs() > tol || length <= tol || !top.parallel {
        return Err(refuse("the wall's top edge must be straight and parallel to its base"));
    }
    let r = radius;
    if length * wy >= reach * (1.0 - 1e-9) {
        return Ok(None);
    }
    // The section: x along the floor away from the wall, y along its normal; the wall's top
    // edge at (tx, h); the circle tangent to the floor through it.
    let section = geo::round_section(length, wx, wy, r).ok_or_else(|| refuse("the wall is too high for the round"))?;
    let geo::Section { h, tx, cx, theta, .. } = section;
    if cx <= 1e-9 {
        return Err(refuse("the wall leans too far over the floor for the round to leave it"));
    }
    // The arc stays on the air side of the wall's plane.
    if !geo::arc_clears_wall(&section, wx, wy, r, tol) {
        return Err(refuse("the round would cross the wall"));
    }
    // The floor reaches under the whole round: the contact line at its middle and quarters.
    for t in [0.25, 0.5, 0.75] {
        let at = a.lerp(b, t) + f_dir * cx;
        if body.point_lies_on_face(floor, at, tol.max(1e-6))?.is_none() {
            return Err(refuse("the floor is too narrow for the round (its contact line would leave the floor)"));
        }
    }
    // The ends.
    let end_a = classify_end(rec, wall, floor, side_a, nf, along, tol)?;
    let end_b = classify_end(rec, wall, floor, side_b, nf, along, tol)?;
    let margin = |end: End| if end == End::Upright { 4.0 * r } else { 0.0 };
    // The prism, in the section plane at the edge's start, x along the floor, y up from it.
    let plane = Plane { origin: a, x: f_dir, y: nf };
    let (lo, hi) = geo::prism_span(arr(a), arr(b), arr(f_dir), arr(nf), arr(along), margin(end_a), margin(end_b));
    let p0 = Vec2::new(0.0, 0.0);
    let t_pt = Vec2::new(tx, h);
    let start = -std::f64::consts::FRAC_PI_2;
    let centre = Vec2::new(cx, r);
    let one = |segs: Vec<Seg2>| -> Result<Body> {
        let region = Region2 { outer: Loop2 { segs }, holes: vec![] };
        crate::build::extrude(&plane, &[region], lo, hi)?.into_iter().next().ok_or_else(|| refuse("building the round's prism gave no body"))
    };
    // Kept within the floor's outline, holes and openings included, whatever the ends: the
    // floor's own sheet, thickened both ways past the round's height.
    let sheet = crate::surfaces::taken(body, &rec.rec, &[(floor, false)])?;
    let room = 4.0 * r + body.size() * 1e-3;
    let column = crate::thicken(&sheet, 2.0 * room, true)?;
    let trim =
        |prism: &Body| crate::boolean(prism, &column, crate::BoolOp::Intersect)?.ok_or_else(|| refuse("the round lies outside the floor's outline"));
    if tx >= -tol {
        let prism = one(vec![
            Seg2::Line { a: p0, b: Vec2::new(cx, 0.0) },
            Seg2::Arc { center: centre, radius: r, start, sweep: -theta },
            Seg2::Line { a: t_pt, b: p0 },
        ])?;
        return Ok(Some(Fill { solid: trim(&prism)? }));
    }
    // A wall leaning back: part of the round stands over the wall's own foot (x < 0), where there
    // is no floor face, between the wall and the vertical plane through the edge. The floor's
    // outline bounds the part over the floor (x >= 0); the wall bounds the other.
    if end_a == End::Upright || end_b == End::Upright {
        return Err(refuse("a wall leaning back with an upright end is not supported"));
    }
    let cross = (cx / r).asin();
    let z0 = r - (r * r - cx * cx).sqrt();
    let over_floor = one(vec![
        Seg2::Line { a: p0, b: Vec2::new(cx, 0.0) },
        Seg2::Arc { center: centre, radius: r, start, sweep: -cross },
        Seg2::Line { a: Vec2::new(0.0, z0), b: p0 },
    ])?;
    let over_wall = one(vec![
        Seg2::Line { a: p0, b: Vec2::new(0.0, z0) },
        Seg2::Arc { center: centre, radius: r, start: start - cross, sweep: -(theta - cross) },
        Seg2::Line { a: t_pt, b: p0 },
    ])?;
    let solid =
        crate::boolean(&trim(&over_floor)?, &over_wall, crate::BoolOp::Union)?.ok_or_else(|| refuse("joining the round's parts gave no body"))?;
    Ok(Some(Fill { solid }))
}

/// How the prism ends where the wall's side edge `side` meets the face beyond it.
fn classify_end(rec: &Faces, wall: usize, floor: usize, side: usize, nf: Vec3, along: Vec3, tol: f64) -> Result<End> {
    let beyond: Vec<usize> = rec.faces_of(side).into_iter().map(|f| f as usize).filter(|f| *f != wall).collect();
    let [g] = beyond.as_slice() else {
        return Err(refuse("the wall's end must meet one face"));
    };
    let g = *g;
    if g == floor {
        return Err(refuse("the wall's end must meet a face beyond the floor"));
    }
    let face = rec.rec.faces.get(g).ok_or_else(|| refuse("the end face"))?;
    let upright = match &face.surface {
        Surface::Plane { .. } | Surface::PlaneThrough { .. } => {
            let n = rec.plane_normal(g).ok_or_else(|| refuse("the end face"))?;
            match geo::plane_end(arr(n), arr(nf), arr(along)) {
                geo::EndKind::Tilted => return Err(refuse("the wall's end must be square to the floor")),
                geo::EndKind::Square => return Ok(End::Square),
                geo::EndKind::Upright => true,
            }
        }
        Surface::Cylinder { frame, .. } => {
            if !geo::cylinder_is_upright(arr(v3(frame.x_axis)), arr(v3(frame.y_axis)), arr(nf)) {
                return Err(refuse("the wall's end must be square to the floor"));
            }
            true
        }
        _ => return Err(refuse("the wall's end must be a flat or cylindrical face")),
    };
    // An upright end that bounds the floor: it shares an edge with it.
    let shares = rec.uses(g).any(|u| rec.faces_of(u.edge as usize).contains(&(floor as u32)));
    let _ = tol;
    if upright && shares { Ok(End::Upright) } else { Err(refuse("the wall's end face must bound the floor")) }
}
