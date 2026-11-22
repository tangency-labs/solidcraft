//! Radial embossing of profiles wrapped round complete cylindrical walls, outside or inside (a
//! bore, where raised is towards the axis), built on the body's schema 4 record: the wall's face
//! loses the profiles' footprints, and each profile adds its walls and its raised or sunk cap
//! (the wall's cylinder at the new radius). A segment along the axis or round the wall keeps an
//! exact line or arc and a planar wall; any other (a curve, a diagonal) is fitted: cubic
//! B-spline edges and a ruled spline wall within a quarter of the body's tolerance of the true
//! wrap, with fitted pcurves on the wall's and the cap's forms (the record's pcurve routine
//! makes only exact ones).
use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::Change;
use crate::numbered::{NumberedLoop, NumberedRegion};
use crate::offset::{Surf, face_edges, surf_of};
use crate::{KernelError, Result, guard};
use solidcore_record::brep::{Brep, Curve, Curve2, Frame, Loop, Surface};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::form;
use solidcraft_geom::prelude::*;

fn fail(s: &str) -> KernelError {
    KernelError::Invalid(format!("cylinder emboss: {s}"))
}
/// Why an emboss onto a picked face is refused when a profile lies off it, and what to do.
const NOT_FITTING: &str = "the text is not fitting the selected target face. Either scale the face up or the text down";
/// A refusal of an emboss onto a picked face.
fn refuse(s: &str) -> KernelError {
    KernelError::Invalid(format!("emboss: {s}"))
}
fn angle(p: Vec3, o: Vec3, axis: Vec3, radial: Vec3, tangent: Vec3) -> f64 {
    let w = p - o;
    let w = w - axis * w.dot(axis);
    w.dot(tangent).atan2(w.dot(radial))
}
fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// For a plane tangent to a cylinder about the axis `a` of radius `r` through `o`: the direction
/// from the axis to the tangent line, and the direction the wrapped profile's x runs in (found by
/// solidcore). An exterior wall needs the plane's normal pointing away from the axis.
fn wall_frame(normal: Vec3, w: Vec3, a: Vec3, r: f64, convex: bool, tol: f64) -> Option<(Vec3, Vec3)> {
    let f = solidcore_ops::cylinder_wall::wall_frame(arr(normal), arr(w), arr(a), r, convex, tol)?;
    Some((Vec3::from(f.radial), Vec3::from(f.tangent)))
}

/// The angles a wall face spans about the axis, seen from `radial`: from one of its arcs about
/// the axis (a face of one loop of four edge uses: two arcs and two lines, or a whole turn's two
/// circles and its seam used twice).
fn wall_angles(b: &Brep, face: usize, o: Vec3, axis: Vec3, radial: Vec3, tangent: Vec3) -> Option<(f64, f64)> {
    let f = b.faces.get(face)?;
    let [Loop::Edges { edges }] = f.loops.as_slice() else { return None };
    if edges.len() != 4 {
        return None;
    }
    for u in edges {
        let e = b.edges.get(u.edge as usize)?;
        let Curve::Circle { frame, .. } = &e.curve else { continue };
        let z = Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis));
        if z.dot(axis).abs() < 1.0 - 1e-9 {
            continue;
        }
        // The wall spans the angles of the arc's ends about the angle of its middle (a whole
        // turn, from the seam, for a wall in one piece). The ends are measured: `interval` is on
        // the circle's NURBS form where it has one, whose parameter is not the angle.
        let c = CurveEval::new(&e.curve).ok()?;
        let [t0, t1] = solidcore_ops::label::edge_curve_interval(e)?;
        let (p0, p1) = (c.point(t0)?, c.point(t1)?);
        let am = angle(c.point(0.5 * (t0 + t1))?, o, axis, radial, tangent);
        if p0.dist(p1) <= 1e-9 * (1.0 + p0.len()) {
            return Some((am - std::f64::consts::PI, am + std::f64::consts::PI));
        }
        let from_middle = |p: Vec3| {
            let d = (angle(p, o, axis, radial, tangent) - am).rem_euclid(std::f64::consts::TAU);
            if d > std::f64::consts::PI { d - std::f64::consts::TAU } else { d }
        };
        let (d0, d1) = (from_middle(p0), from_middle(p1));
        return Some((am + d0.min(d1), am + d0.max(d1)));
    }
    None
}

/// The body's record, for the queries below (a mesh body has none).
fn record(body: &Body) -> Result<Brep> {
    crate::native::brep_record(body)
}

/// Certify a tessellated pick and project it onto the cylinder used by emboss.
pub fn cylinder_emboss_anchor(body: &Body, face: usize, at: Vec3) -> Result<Vec3> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    guard("cylinder emboss anchor", || {
        let rec = record(body)?;
        if face >= rec.faces.len() {
            return Err(fail("selected face"));
        }
        let Some(Surf::Cylinder { o, a, r, .. }) = surf_of(&rec, face) else {
            return Err(fail("selected face is not a cylindrical wall"));
        };
        let w = at - o;
        let radial = w - a * w.dot(a);
        if (radial.len() - r).abs() > (body.size() * 1e-3).max(1e-3).min(r * 0.05) {
            return Err(fail("selected point is too far from cylinder"));
        }
        Ok(o + a * w.dot(a) + radial.normalized().ok_or_else(|| fail("radial point"))? * r)
    })
}

/// Certify a pick on face `face` and put it on the face's surface: a plane, a cylinder (either
/// side) or a sphere. A pick on another surface is kept as given.
pub fn emboss_anchor(body: &Body, face: usize, at: Vec3) -> Result<Vec3> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(refuse("invalid selected face point"));
    }
    guard("emboss anchor", || {
        let rec = record(body)?;
        if face >= rec.faces.len() {
            return Err(refuse("selected face"));
        }
        let near = (body.size() * 1e-3).max(1e-3);
        let onto = |c: Vec3, r: f64, w: Vec3| -> Result<Vec3> {
            if (w.len() - r).abs() > near.min(r * 0.05) {
                return Err(refuse("selected point is too far from the face"));
            }
            Ok(c + w.normalized().ok_or_else(|| refuse("radial point"))? * r)
        };
        match surf_of(&rec, face) {
            Some(Surf::Plane { n, d }) => {
                let off = at.dot(n) - d;
                if off.abs() > near {
                    return Err(refuse("selected point is too far from the face"));
                }
                Ok(at - n * off)
            }
            Some(Surf::Cylinder { o, a, r, .. }) => {
                let h = a * (at - o).dot(a);
                onto(o + h, r, at - o - h)
            }
            Some(Surf::Sphere { c, r, .. }) => onto(c, r, at - c),
            _ => Ok(at),
        }
    })
}

/// Emboss (`depth > 0`) or deboss (`depth < 0`) `regions` onto face `face` of `body`, `at` a
/// point on it. Each region is cut out of a slab between offsets of the face, so the raised or
/// sunk cap follows the face at `depth`, then joined to or cut from the body. On a cylinder
/// (either side) each region is laid on the wall at its centroid's arc length from the sketch;
/// on any other face it is projected along the sketch normal.
pub fn emboss_project_face(body: &Body, face: usize, at: Vec3, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    body.require_brep("emboss")?;
    if !depth.is_finite() || depth.abs() < 1e-6 || depth.abs() > 1e6 || regions.is_empty() || regions.len() > 4096 {
        return Err(refuse("needs finite nonzero depth and 1..4096 profiles"));
    }
    if !at.is_finite() || at.len() > 1e9 || Plane::new(plane.origin, plane.x, plane.y).is_none() || !plane.origin.is_finite() {
        return Err(refuse("invalid sketch frame or selected point"));
    }
    guard("emboss", || {
        let rec = record(body)?;
        if face >= rec.faces.len() {
            return Err(refuse("selected face"));
        }
        let d = depth.abs();
        // The slab reaches a little past the face on the side away from the depth, so the pieces
        // overlap the body rather than touch it.
        let overlap = (d * 0.2).clamp(0.1, 2.0);
        let (lo, hi) = if depth > 0.0 { (-overlap, d) } else { (-d, overlap) };
        let mut prisms = Vec::new();
        let mut through = false;
        let slab = if let Some(Surf::Cylinder { o, a, r, convex }) = surf_of(&rec, face) {
            // The prisms below reach at most the radius either side of the wall (further, they
            // would meet the ring's far side), so a deeper slab would be cut short.
            if d + overlap >= r {
                return Err(refuse("the depth must stay below the cylinder's radius"));
            }
            // SMLib does not thicken a whole wall, so the slab is the exact ring between the
            // offsets' radii, as long as the selected face along the axis (the body where the
            // face's edges do not bound it).
            let (r_in, r_out) = if convex { (r + lo, r + hi) } else { (r - hi, r - lo) };
            // The wall is every face of the body on this cylinder: a wall is stored as several
            // faces between its seams.
            let tol = (body.size() * 1e-7).max(1e-7);
            let on_cylinder = |f: usize| {
                matches!(surf_of(&rec, f), Some(Surf::Cylinder { o: fo, a: fa, r: fr, convex: fc })
                    if fc == convex && (fr - r).abs() <= tol && fa.dot(a).abs() > 1.0 - 1e-8 && (fo - o - a * (fo - o).dot(a)).len() <= tol)
            };
            let wall: Vec<usize> = (0..rec.faces.len()).filter(|f| *f == face || on_cylinder(*f)).collect();
            let heights = wall.iter().map(|f| face_axial_bounds(&rec, *f, o, a)).collect::<Result<Vec<_>>>()?;
            let (z_lo, z_hi) = match heights.iter().copied().collect::<Option<Vec<_>>>() {
                Some(spans) => spans.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), (a, b)| (l.min(*a), h.max(*b))),
                None => span(body.exact_bounds()?, o, a),
            };
            let base = o + a * z_lo;
            let length = z_hi - z_lo;
            let outer = crate::cylinder(base, a, r_out, length)?;
            let slab = if r_in > 1e-6 {
                crate::boolean(&outer, &crate::cylinder(base, a, r_in, length)?, crate::BoolOp::Cut)?.ok_or_else(|| refuse("slab"))?
            } else {
                outer
            };
            let w = at - o;
            let radial_at = (w - a * w.dot(a)).normalized().ok_or_else(|| refuse("selected point is on the axis"))?;
            // Text reads round the wall when the sketch's x runs round it, along the axis otherwise.
            let (kx, ky) = (plane.x.dot(a), plane.y.dot(a));
            let along_axis = kx.abs() > ky.abs();
            let reading = if along_axis { plane.y } else { plane.x };
            let v = reading.cross(a).normalized().unwrap_or(radial_at);
            let radial0 = if v.dot(radial_at) >= 0.0 { v } else { -v };
            let t = a.cross(radial0);
            let tangent0 = if t.dot(reading) >= 0.0 { t } else { -t };
            let (u0, z0) = ((plane.origin - o).dot(tangent0), (plane.origin - o).dot(a));
            // Through the ring near the region, short of its far side.
            let pad = ((d + overlap) * 2.5 + 2.0).min(r);
            let angles: Vec<_> = wall.iter().map(|f| wall_angles(&rec, *f, o, a, radial0, tangent0)).collect();
            let tau = std::f64::consts::TAU;
            // A sketch point's angle about the axis and height along it, as the wall lays it out.
            let (tx, ty) = (plane.x.dot(tangent0), plane.y.dot(tangent0));
            let laid = |p: Vec2| ((u0 + p.x * tx + p.y * ty) / r, z0 + p.x * kx + p.y * ky);
            let eps = 1e-9 * (1.0 + body.size());
            // On a face of the wall: within its angles and heights, where its edges give them.
            let on_wall = |(theta, height): (f64, f64)| {
                angles.iter().zip(&heights).any(|(turn, along)| {
                    turn.is_none_or(|(alo, ahi)| [theta - tau, theta, theta + tau].iter().any(|t| (alo - eps..=ahi + eps).contains(t)))
                        && along.is_none_or(|(zl, zh)| (zl - eps..=zh + eps).contains(&height))
                })
            };
            for reg in regions {
                let c = reg.centroid();
                let (theta, height) = laid(c);
                // The whole region must lie on the wall, not just its centre: the corners of its
                // box (the map is linear, so they bound it). shortcut: the outline's polyline
                // within 1e-4, so a curve may bulge past the box by that much.
                let (lo2, hi2) = reg
                    .outer
                    .polyline(1e-4)
                    .iter()
                    .fold((c, c), |(l, h), p| (Vec2::new(l.x.min(p.x), l.y.min(p.y)), Vec2::new(h.x.max(p.x), h.y.max(p.y))));
                let corners = [lo2, Vec2::new(hi2.x, lo2.y), hi2, Vec2::new(lo2.x, hi2.y)];
                if !on_wall((theta, height)) || !corners.into_iter().all(|p| on_wall(laid(p))) {
                    return Err(refuse(NOT_FITTING));
                }
                let radial = radial0 * theta.cos() + tangent0 * theta.sin();
                let tangent = tangent0 * theta.cos() - radial0 * theta.sin();
                let at_c = o + a * height + radial * r;
                let (x, y) = if along_axis { (a * kx.signum(), tangent) } else { (tangent, a * ky.signum()) };
                // The region keeps its sketch coordinates: the frame's origin sits so its centroid lands on the wall.
                let Some(local) = Plane::new(at_c - x * c.x - y * c.y, x, y) else { continue };
                prisms.extend(crate::extrude(&local, std::slice::from_ref(reg), -pad, pad)?);
            }
            slab
        } else {
            let sheet = crate::surfaces::offset_faces(body, &[face], lo, (body.size() * 1e-7).max(1e-7))?;
            let slab = crate::thicken(&sheet, hi - lo, false)?;
            let (h0, h1) = span(slab.exact_bounds()?, plane.origin, plane.normal());
            prisms = crate::extrude(plane, regions, h0 - d - 1.0, h1 + d + 1.0)?;
            through = true;
            slab
        };
        // Every region must reach the face: one that misses it would leave its letter out.
        let mut pieces = Vec::new();
        let mut missed = Vec::new();
        for (i, prism) in prisms.iter().enumerate() {
            match crate::boolean(prism, &slab, crate::BoolOp::Intersect)? {
                // A prism runs right through the slab: what is left of it is one part above and
                // one below, unless the slab cuts it short (over the face's edge or a hole),
                // where the two join round the side.
                Some(_)
                    if through && crate::boolean(prism, &slab, crate::BoolOp::Cut)?.map_or(0, |rest| rest.lumps().map_or(0, |l| l.len())) != 2 =>
                {
                    missed.push((i + 1).to_string())
                }
                Some(piece) => pieces.push(piece),
                None => missed.push((i + 1).to_string()),
            }
        }
        if !missed.is_empty() {
            let which = match missed.split_last() {
                Some((last, [])) => format!("profile {last} of {} lies", prisms.len()),
                Some((last, rest)) => format!("profiles {} and {last} of {} lie", rest.join(", "), prisms.len()),
                None => String::new(),
            };
            return Err(refuse(&format!("{NOT_FITTING} ({which} off it)")));
        }
        apply_pieces(body, pieces, depth)
    })
}

/// The heights of a box's corners above `o` along `dir`: lowest and highest.
fn span(b: solidcraft_geom::Aabb3, o: Vec3, dir: Vec3) -> (f64, f64) {
    let corners =
        [b.min.x, b.max.x].into_iter().flat_map(|x| [b.min.y, b.max.y].into_iter().flat_map(move |y| [b.min.z, b.max.z].map(|z| Vec3::new(x, y, z))));
    corners.map(|p| (p - o).dot(dir)).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(x), h.max(x)))
}

/// Join (`depth > 0`) or cut the emboss pieces: all at once where they make one tool, else one by
/// one. Every piece must apply: one that cannot refuses the whole emboss, so no letter goes missing.
fn apply_pieces(body: &Body, pieces: Vec<Body>, depth: f64) -> Result<Body> {
    if pieces.is_empty() {
        return Err(refuse("the profile does not meet the selected face"));
    }
    let op = if depth > 0.0 { crate::BoolOp::Union } else { crate::BoolOp::Cut };
    if let Ok(Some(tool)) = crate::ops::join_pieces(pieces.clone())
        && let Ok(Some(result)) = crate::boolean(body, &tool, op)
    {
        return Ok(result);
    }
    let mut current = body.clone();
    for piece in &pieces {
        current = crate::boolean(&current, piece, op)
            .map_err(|e| KernelError::Failed(format!("emboss: a profile could not be applied to the face: {e}")))?
            .ok_or_else(|| refuse("a profile would leave nothing of the body"))?;
    }
    Ok(current)
}

/// Wrap sketch arc length around a cylindrical wall (outside, or a bore), retaining exact radial depth.
/// Profiles of any segments must lie inside one uninterrupted wall face.
pub fn emboss_cylinder(body: &Body, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    emboss_impl(body, None, plane, regions, depth, false).and_then(EmbossOutcome::body)
}

/// As [`emboss_cylinder`], requiring the saved selected point to identify that wall.
pub fn emboss_cylinder_at(body: &Body, at: Vec3, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    emboss_impl(body, Some(at), plane, regions, depth, false).and_then(EmbossOutcome::body)
}

enum EmbossOutcome {
    Body(Body),
    Anchor(usize, Vec3),
    NoMatch,
}
impl EmbossOutcome {
    fn body(self) -> Result<Body> {
        match self {
            Self::Body(body) => Ok(body),
            Self::Anchor(..) | Self::NoMatch => Err(fail("unexpected qualification result")),
        }
    }
}

/// Qualify a tangent cylindrical target without changing it. A tangent exterior
/// cylinder with an unsupported profile or depth returns an explicit error;
/// a body without such a tangent wall returns `None` for planar compatibility.
/// The anchor lies on a validated profile boundary, clear of seams and endcaps.
pub fn infer_cylinder_emboss(body: &Body, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Option<(usize, Vec3)>> {
    if body.is_mesh() {
        return Ok(None);
    }
    guard("cylinder emboss inference", || {
        let rec = record(body)?;
        let tol = (body.size() * 1e-8).max(1e-8);
        let mut tangent = false;
        for face in 0..rec.faces.len() {
            if face >= 50_000 {
                return Err(fail("target search exceeds face work limit"));
            }
            if let Some(Surf::Cylinder { o, a, r, convex }) = surf_of(&rec, face) {
                // The sketch plane touches this wall from either side: take the wall's frame
                // as seen from the plane's side (the bore's form), then turn the tangent for
                // an exterior wall so the wrap runs the same way round as the face.
                if let Some((radial, bore_x)) = wall_frame(plane.normal(), plane.origin - o, a, r, false, tol) {
                    let wrap_x = if convex { -bore_x } else { bore_x };
                    // Every supported uninterrupted wall uses one angle interval
                    // around this tangent normal, contained in (-pi,pi). A remote
                    // footprint entirely outside even that global interval cannot
                    // select this body, regardless of other unsupported faces.
                    if !profile_interval_overlaps(plane, regions, o, wrap_x, -std::f64::consts::PI * r, std::f64::consts::PI * r)? {
                        continue;
                    }
                    if let Some((lo, hi)) = wall_angles(&rec, face, o, a, radial, wrap_x) {
                        if lo >= -tol / r || hi <= tol / r {
                            continue;
                        }
                        if !profile_interval_overlaps(plane, regions, o, wrap_x, lo * r, hi * r)? {
                            continue;
                        }
                    }
                    let relevant = match face_axial_bounds(&rec, face, o, a)? {
                        Some((lo, hi)) => profile_interval_overlaps(plane, regions, o, a, lo, hi)?,
                        None => true,
                    };
                    if relevant && convex && radial.dot(plane.normal()) < 0.0 {
                        return Err(fail("sketch normal must point outwards from the exterior wall"));
                    }
                    tangent |= relevant;
                }
            }
        }
        if !tangent {
            return Ok(None);
        }
        match emboss_impl(body, None, plane, regions, depth, true)? {
            EmbossOutcome::Anchor(index, at) => Ok(Some((index, at))),
            EmbossOutcome::NoMatch => Ok(None),
            EmbossOutcome::Body(_) => Err(fail("unexpected construction result")),
        }
    })
}

/// Whether a body contains an analytic cylindrical wall, outside or a bore. This bounded
/// preflight avoids resolving cylinder placement context for ordinary planar targets.
pub fn has_cylinder_emboss_wall(body: &Body) -> Result<bool> {
    if body.is_mesh() {
        return Ok(false);
    }
    guard("cylinder emboss target", || {
        let rec = record(body)?;
        if rec.faces.len() > 50_000 {
            return Err(fail("target search exceeds face work limit"));
        }
        Ok((0..rec.faces.len()).any(|face| matches!(surf_of(&rec, face), Some(Surf::Cylinder { .. }))))
    })
}

fn emboss_impl(body: &Body, anchor: Option<Vec3>, plane: &Plane, regions: &[Region2], depth: f64, qualify: bool) -> Result<EmbossOutcome> {
    body.require_brep("cylinder emboss")?;
    if !depth.is_finite() || depth.abs() < 1e-6 || depth.abs() > 1e6 || regions.is_empty() || regions.len() > 64 {
        return Err(fail("needs finite nonzero depth and 1..64 profiles"));
    }
    if !plane.origin.is_finite()
        || !plane.x.is_finite()
        || !plane.y.is_finite()
        || (plane.x.len() - 1.0).abs() > 1e-8
        || (plane.y.len() - 1.0).abs() > 1e-8
        || plane.x.dot(plane.y).abs() > 1e-8
    {
        return Err(fail("invalid sketch frame"));
    }
    guard("cylinder emboss", || {
        let rec = record(body)?;
        if rec.shells.len() != 1 || rec.faces.len() > 16 || rec.edges.len() > 256 {
            return Err(fail("requires a complete cylinder or coaxial sleeve"));
        }
        let tol = (body.size() * 1e-8).max(1e-8);
        let surfaces: Vec<Surf> = (0..rec.faces.len())
            .map(|f| surf_of(&rec, f).ok_or_else(|| fail("requires only cylindrical walls and planar endcaps")))
            .collect::<Result<_>>()?;
        let mut chosen = None;
        for (fi, s) in surfaces.iter().enumerate() {
            let Surf::Cylinder { o, a, r, convex } = *s else { continue };
            let Some((radial, tangent)) = wall_frame(plane.normal(), plane.origin - o, a, r, convex, tol) else {
                continue;
            };
            let Some((alo, ahi)) = wall_angles(&rec, fi, o, a, radial, tangent) else {
                continue;
            };
            if alo >= -tol / r || ahi <= tol / r {
                continue;
            }
            if chosen.is_some() {
                return Err(fail("ambiguous cylindrical face"));
            }
            chosen = Some((fi, o, a, r, convex, radial, tangent, alo, ahi));
        }
        let (fi, o, axis, r, outside, radial, tangent, alo, ahi) =
            chosen.ok_or_else(|| fail("sketch must be tangent to one complete cylindrical wall, outside or inside, clear of its seam"))?;
        if qualify && !profile_interval_overlaps(plane, regions, o, tangent, alo * r, ahi * r)? {
            return Ok(EmbossOutcome::NoMatch);
        }
        let mut caps = Vec::new();
        // Across the material: the largest bore radius for an exterior wall, the smallest outside
        // radius for a bore.
        let mut inner: f64 = 0.0;
        let mut outer = f64::INFINITY;
        for s in &surfaces {
            match *s {
                Surf::Plane { n, d } if n.dot(axis).abs() > 1.0 - 1e-8 => caps.push((d - n.dot(o)) / n.dot(axis)),
                Surf::Cylinder { o: co, a: ca, r: cr, convex }
                    if ca.dot(axis).abs() > 1.0 - 1e-8 && (co - o - axis * (co - o).dot(axis)).len() < tol * 10.0 =>
                {
                    if convex == outside {
                        if (cr - r).abs() > tol * 10.0 {
                            return Err(fail("interrupted or stepped cylinder"));
                        }
                    } else if outside {
                        inner = inner.max(cr);
                    } else {
                        outer = outer.min(cr);
                    }
                }
                _ => return Err(fail("requires a complete cylinder or coaxial sleeve")),
            }
        }
        if caps.len() != 2 {
            return Err(fail("requires two planar endcaps"));
        }
        let lo = caps.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = caps.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // An infinite cylinder can be tangent while its finite wall is far from
        // the selected footprint. Conservative analytic/control-polygon bounds
        // exclude that body before unsupported-profile or depth diagnostics.
        if qualify && !profile_interval_overlaps(plane, regions, o, axis, lo, hi)? {
            return Ok(EmbossOutcome::NoMatch);
        }
        // Positive depth is out of the material: away from the axis on the outside, towards it in
        // a bore.
        let rr = if outside { r + depth } else { r - depth };
        if outside && rr <= inner + tol * 10.0 {
            return Err(fail("depth reaches the axis or inner wall"));
        }
        if !outside {
            if rr <= tol * 10.0 {
                return Err(fail("depth reaches the axis"));
            }
            if !outer.is_finite() || rr >= outer - tol * 10.0 {
                return Err(fail("depth breaks through the outer wall"));
            }
        }
        if let Some(at) = anchor {
            // The saved pick identifies the wall, not the face the sketch is tangent to: every
            // exterior cylindrical face of the body is that wall (a stepped or interrupted
            // cylinder was refused above), stored as several faces between its seams. So the
            // pick lies on it wherever it is on the cylinder between the endcaps (#3782).
            let h = (at - o).dot(axis);
            let v = at - o - axis * h;
            if (v.len() - r).abs() > tol * 10.0 || h <= lo + tol || h >= hi - tol {
                return Err(fail("saved selection no longer identifies the sketch's cylindrical face"));
            }
        }
        let z0 = (plane.origin - o).dot(axis);
        let s0 = (plane.origin - o).dot(tangent);
        let mut count = 0usize;
        // The regions mapped onto the wall as given (their loops' turning may differ from the
        // sketch's): profile numbering follows them.
        let mut unoriented = Vec::new();
        for reg in regions {
            let mut loops = Vec::new();
            for lp in std::iter::once(&reg.outer).chain(&reg.holes) {
                count = count.saturating_add(lp.segs.len());
                if count > 4096 {
                    return Err(fail("profile geometry budget"));
                }
                if lp.segs.is_empty() || !lp.is_closed(tol) {
                    return Err(fail("profile geometry budget or open loop"));
                }
                let mut segs = Vec::new();
                for s in &lp.segs {
                    s.check(1e7).map_err(|e| fail(&format!("profile geometry: {e}")))?;
                    let s = on_wall(s, plane, s0, z0, tangent, axis).ok_or_else(|| fail("sketch must lie flat on the cylindrical wall"))?;
                    if s.start().dist(s.end()) < tol && s.length() < tol {
                        return Err(fail("a profile edge of no length"));
                    }
                    // The whole curve, not only its ends, stays clear of the seam and endcaps.
                    let (xlo, xhi) = (alo * r + tol, ahi * r - tol);
                    if !clear_within(s, Vec2::new(xlo, lo + tol), Vec2::new(xhi, hi - tol), tol, &mut 256) {
                        return Err(fail("profile crosses a cylinder seam or endcap boundary"));
                    }
                    segs.push(s);
                }
                loops.push(Loop2 { segs });
            }
            let mut given = loops.into_iter();
            unoriented.push(Region2 { outer: given.next().ok_or_else(|| fail("profile"))?, holes: given.collect() });
        }
        // A loop of one closed curve (a circle) is split in two, so each wall has two corners.
        let numbered: Vec<NumberedRegion> =
            crate::numbered::number_regions(&unoriented).iter().map(|r| r.oriented().map_loops(split_single)).collect();
        let mapped: Vec<Region2> = numbered.iter().map(NumberedRegion::region).collect();
        validate_profiles(&mapped, tol)?;
        if qualify {
            let segment = mapped.first().and_then(|region| region.outer.segs.first()).ok_or_else(|| fail("empty profile"))?;
            let p = segment.point_at(0.5);
            let at = o + axis * p.y + (radial * (p.x / r).cos() + tangent * (p.x / r).sin()) * r;
            return Ok(EmbossOutcome::Anchor(fi, at));
        }
        let mut asm = Assembly::from_record(&rec)?;
        let selected = asm.faces.get_mut(fi).and_then(Option::take).ok_or_else(|| fail("face"))?;
        // The footprint's arcs lie on the wall face and are labelled on its frame, so their NURBS
        // forms run on the face's own quarter grid and their pcurves there are exact.
        let label = form::frame_of(&selected.surface);
        // Curved and slanted edges, and their pcurves, are fitted within a quarter of the body's
        // tolerance each.
        let wall_form = label.as_ref().and_then(|f| OnForm::of(f, o, axis, radial, tangent));
        let wall_form = wall_form.zip(selected.nurbs.as_ref()).zip(label.as_ref()).and_then(|((on, n), f)| on.on(n, f));
        let cap_form = OnForm { delta: 0.0, sigma: 1.0, u0: 0.0, k: 1.0, v0: 0.0, vk: axis.dot(radial.cross(tangent)) };
        let wall = Wrap {
            o,
            axis,
            radial,
            tangent,
            r,
            label,
            tol,
            fit: asm.tolerance * 0.25,
            wall_form,
            cap_form,
            spans: std::cell::Cell::new(MAX_TOTAL_SPANS),
        };
        if mapped.iter().flat_map(|m| m.loops()).flat_map(|l| &l.segs).any(|seg| !wall.exact(seg)) {
            wall.check_wall_form(&selected, mapped.iter().flat_map(|m| m.loops()).flat_map(|l| &l.segs).find(|seg| !wall.exact(seg)))?;
        }
        // The cap's normal, out of the material, is away from the axis outside and towards it in a
        // bore, raised or sunk.
        let cap_out = outside;
        // The wall's pieces left around the footprints: the face itself, then one per profile
        // hole (the original radius inside it).
        let mut base_patches: Vec<Vec<Vec<AUse>>> = vec![selected.loops.clone()];
        let mut hole_patches = Vec::new();
        let mut footprints = Vec::new();
        let mut added: Vec<AFace> = Vec::new();
        // History of the faces added (walls from their profile segments, the raised or sunk
        // region the profile's end cap), in the order added.
        let mut added_history = Vec::new();
        for reg in &numbered {
            let mut cap = Vec::new();
            for (li, numbered_loop) in reg.loops().enumerate() {
                let lp = &numbered_loop.lp;
                let (base, top, walls) = wall.wrapped_loop(&mut asm, lp, rr, selected.shell)?;
                // One wall per segment, in order.
                added_history.extend((0..walls.len()).map(|j| crate::record::profile(numbered_loop.numbers(j))));
                let hole = reversed(&base);
                if li == 0 {
                    footprints.push((lp, hole));
                } else {
                    let index = base_patches.len();
                    base_patches.push(vec![hole]);
                    hole_patches.push((lp, index));
                }
                cap.push(top);
                added.extend(walls);
            }
            // A cap bounded by fitted edges carries their fitted pcurves, so it is put on its form
            // here (the cap's frame's quarter grid, as the pcurves are) over the region's box.
            let (nurbs, domain) = if reg.loops().flat_map(|l| &l.lp.segs).any(|seg| !wall.exact(seg)) {
                let (n, d) = wall.cap_form(&reg.region(), rr)?;
                (Some(n), Some(d))
            } else {
                (None, None)
            };
            added.push(AFace { surface: wall.cylinder(rr), nurbs, natural_out: cap_out, loops: cap, shell: selected.shell, domain });
            added_history.push(crate::record::cap(true));
        }
        // An island inside a profile hole cuts that hole's original-radius patch,
        // rather than adding another hole to the already-cut exterior patch.
        for (lp, wire) in footprints {
            let probe = lp.segs.first().ok_or_else(|| fail("empty profile"))?.start();
            let owner = hole_patches
                .iter()
                .filter(|(hole, _)| hole.contains(probe))
                .min_by(|(a, _), (b, _)| a.signed_area().abs().total_cmp(&b.signed_area().abs()))
                .map(|(_, index)| *index)
                .unwrap_or(0);
            base_patches.get_mut(owner).ok_or_else(|| fail("profile ownership"))?.push(wire);
        }
        // History (one operand, the body; profile numbering over `regions`), face by face as
        // gathered: the other faces kept, the wall's pieces left around the footprints (split
        // when the profile's holes leave more than one), then the faces added.
        let pieces = if base_patches.len() > 1 { Change::Split } else { Change::Modified };
        let mut history: Vec<_> = crate::record::kept_except(rec.faces.len(), fi).collect();
        history.extend(std::iter::repeat_n(crate::record::from_face(0, fi, pieces), base_patches.len()));
        history.extend(added_history);
        for (k, loops) in base_patches.into_iter().enumerate() {
            // The face's own parameter box only where it keeps its outer loop.
            let domain = if k == 0 { selected.domain } else { None };
            asm.face(AFace {
                surface: selected.surface.clone(),
                nurbs: selected.nurbs.clone(),
                natural_out: selected.natural_out,
                loops,
                shell: selected.shell,
                domain,
            });
        }
        for f in added {
            asm.face(f);
        }
        let finished = asm.finish()?;
        let result = crate::assemble::body_with_history(finished, &asm, body, history)?;
        if !result.validity().is_empty() {
            return Err(fail("rebuilt cylinder is not a closed manifold"));
        }
        Ok(EmbossOutcome::Body(result))
    })
}

/// A loop's uses the other way round.
fn reversed(l: &[AUse]) -> Vec<AUse> {
    l.iter().rev().map(|u| AUse { edge: u.edge, forward: !u.forward, given: u.given.clone() }).collect()
}

/// The wall a profile wraps onto: its axis, the sketch's radial and tangent directions there,
/// and its radius.
struct Wrap {
    o: Vec3,
    axis: Vec3,
    radial: Vec3,
    tangent: Vec3,
    r: f64,
    /// The wall face's own frame, which the arcs at its radius are labelled on.
    label: Option<Frame>,
    tol: f64,
    /// How far a fitted edge or side wall may stray from the true wrap.
    fit: f64,
    /// Where the wall face's form puts a wrapped point, when its label has a frame.
    wall_form: Option<OnForm>,
    /// Where the cap's form (on its own frame, [`Wrap::cylinder`]) puts a wrapped point.
    cap_form: OnForm,
    /// The spans fitted edges and pcurves may still take ([`MAX_TOTAL_SPANS`]).
    spans: std::cell::Cell<usize>,
}

/// How a cylinder's form parameterises the wall: wrapped coordinates (arc length `s` at the
/// wall's radius `r`, height `z`) lie at the angle `delta + sigma s / r` from where the form's
/// quarter arcs start, and at the height `v0 + vk z` along its frame's z axis. The form's u is
/// `u0 + k` times that angle's parameter on the quarter grid ([`form::param`]): its quarters are
/// `k` long (1 on [`form::surface`]'s forms, `r π/2` on a primitive's); its v is the height.
#[derive(Clone, Copy, Debug)]
struct OnForm {
    delta: f64,
    sigma: f64,
    u0: f64,
    k: f64,
    v0: f64,
    vk: f64,
}

impl OnForm {
    /// The form of a cylinder labelled on `frame`, about the wall's axis.
    fn of(frame: &Frame, o: Vec3, axis: Vec3, radial: Vec3, tangent: Vec3) -> Option<OnForm> {
        let (fo, fx, fy) = (Vec3::from(frame.origin), Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let fz = fx.cross(fy).normalized()?;
        if fz.dot(axis).abs() < 1.0 - 1e-9 {
            return None;
        }
        Some(OnForm {
            delta: radial.dot(fy).atan2(radial.dot(fx)),
            sigma: fz.dot(radial.cross(tangent)).signum(),
            u0: 0.0,
            k: 1.0,
            v0: (o - fo).dot(fz),
            vk: axis.dot(fz),
        })
    }

    /// The same, on the face form `nurbs` labelled on `frame`: its quarters as long as its first
    /// knot interval, starting where its first knot lies on the frame. [`Wrap::check_wall_form`]
    /// confirms it.
    fn on(self, nurbs: &Surface, frame: &Frame) -> Option<OnForm> {
        let Surface::RationalSpline { u_knots, v_knots, .. } = nurbs else { return None };
        let (&u0, &v_lo, &v_hi) = (u_knots.first()?, v_knots.first()?, v_knots.last()?);
        let next = *u_knots.iter().find(|x| **x > u0)?;
        let p = SurfaceEval::new(nurbs).ok()?.point([u0, 0.5 * (v_lo + v_hi)])?;
        let (fo, fx, fy) = (Vec3::from(frame.origin), Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let start = (p - fo).dot(fy).atan2((p - fo).dot(fx));
        Some(OnForm { delta: self.delta - start, u0, k: next - u0, ..self })
    }
}

/// tan(π/8): the quarter arcs' parameter is `tan((ψ − π/4)/2) = tan(π/8)(2u − 1)`.
const TAN_PI_8: f64 = 0.414_213_562_373_095_03;

/// The form parameter's rate of change with the angle at `theta` ([`form::param`]): on its
/// quarter, `sec²(ψ/2 − π/8) / (4 tan(π/8))`, the same at both ends of a quarter (so the
/// parameter is C1 across quarter lines; its second derivative is not).
fn param_rate(theta: f64) -> f64 {
    let quarter = std::f64::consts::FRAC_PI_2;
    let q = (theta / quarter + 1e-12).floor();
    let psi = (theta - q * quarter).clamp(0.0, quarter);
    let c = (0.5 * psi - std::f64::consts::FRAC_PI_8).cos();
    1.0 / (4.0 * TAN_PI_8 * c * c)
}

/// Bounds on the form parameter's derivatives 1 to 4 with the angle, within a quarter: those of
/// `tan(x)/(2 tan(π/8)) / 2^k` at `|x| = π/8`, where each is largest.
const PARAM_BOUNDS: [f64; 4] = [std::f64::consts::FRAC_1_SQRT_2, 0.292_894, 0.535_535, 0.736_549];

/// The most the angle changes per unit of the form parameter: `4 tan(π/8)`, at a quarter's middle.
const ANGLE_RATE: f64 = 4.0 * TAN_PI_8;

/// The parameters in (0, 1) where segment `seg`'s first coordinate is `x`: exactly for a line
/// and an arc, by halving the Bernstein form (polynomial, or a conic's numerator less `x` times
/// its denominator) to 1e-13 for the rest.
fn crossings(seg: &Seg2, x: f64) -> Vec<f64> {
    let mut out = Vec::new();
    match *seg {
        Seg2::Line { a, b } => {
            if (b.x - a.x).abs() > 0.0 {
                out.push((x - a.x) / (b.x - a.x));
            }
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let k = (x - center.x) / radius;
            if k.abs() <= 1.0 && sweep != 0.0 {
                let phi = k.acos();
                let tau = std::f64::consts::TAU;
                let (lo, hi) = if sweep > 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                for base in [phi, -phi] {
                    let mut m = ((lo - base) / tau).floor();
                    while base + m * tau <= hi && out.len() < 8 {
                        out.push((base + m * tau - start) / sweep);
                        m += 1.0;
                    }
                }
            }
        }
        Seg2::Cubic { p0, p1, p2, p3 } => bernstein_roots(&[p0.x - x, p1.x - x, p2.x - x, p3.x - x], 0.0, 1.0, &mut 4096, &mut out),
        Seg2::Bezier { n, ref p } => {
            let c: Vec<f64> = p.iter().take(usize::from(n) + 1).map(|q| q.x - x).collect();
            bernstein_roots(&c, 0.0, 1.0, &mut 4096, &mut out)
        }
        Seg2::Conic { a, apex, b, w } => bernstein_roots(&[a.x - x, w * (apex.x - x), b.x - x], 0.0, 1.0, &mut 4096, &mut out),
    }
    out.retain(|t| t.is_finite() && *t > 1e-12 && *t < 1.0 - 1e-12);
    out
}

/// The roots in (`lo`, `hi`) of the polynomial with Bernstein coefficients `c` there, by halving
/// wherever the coefficients change sign, to 1e-13 (or out of `budget` halvings).
fn bernstein_roots(c: &[f64], lo: f64, hi: f64, budget: &mut usize, out: &mut Vec<f64>) {
    if c.iter().all(|x| *x > 0.0) || c.iter().all(|x| *x < 0.0) || c.is_empty() {
        return;
    }
    if hi - lo < 1e-13 || *budget == 0 {
        let t = 0.5 * (lo + hi);
        if out.last().is_none_or(|l| t - l > 1e-10) {
            out.push(t);
        }
        return;
    }
    *budget -= 1;
    // de Casteljau at the middle: the left half's coefficients are the first of each row, the
    // right half's the last, read back.
    let mut row = c.to_vec();
    let (mut left, mut right) = (vec![0.0; c.len()], vec![0.0; c.len()]);
    for k in 0..c.len() {
        if let (Some(l), Some(f)) = (left.get_mut(k), row.first()) {
            *l = *f;
        }
        if let (Some(r), Some(b)) = (right.get_mut(c.len() - 1 - k), row.last()) {
            *r = *b;
        }
        row = row.windows(2).map(|w| 0.5 * (w[0] + w[1])).collect();
    }
    let mid = 0.5 * (lo + hi);
    bernstein_roots(&left, lo, mid, budget, out);
    bernstein_roots(&right, mid, hi, budget, out);
}

impl Wrap {
    /// The point at wrapped profile coordinates `p` (arc length at the wall, height) at `radius`.
    fn at(&self, p: Vec2, radius: f64) -> Vec3 {
        let t = p.x / self.r;
        self.o + self.axis * p.y + (self.radial * t.cos() + self.tangent * t.sin()) * radius
    }

    /// The wall's cylinder at `radius` (its angle measured from the sketch's radial).
    fn cylinder(&self, radius: f64) -> Surface {
        Surface::Cylinder { frame: Frame { origin: arr(self.o), x_axis: arr(self.radial), y_axis: arr(self.tangent) }, radius }
    }

    /// An edge from vertex `a` at `p` to vertex `b` at `q` (wrapped coordinates) at `radius`:
    /// an arc about the axis where the segment runs round the wall, else a line along it. The
    /// use that runs from `a` to `b`.
    fn edge(&self, asm: &mut Assembly, a: u32, b: u32, p: Vec2, q: Vec2, radius: f64) -> Result<AUse> {
        if (q.x - p.x).abs() <= 1e-10 {
            return Ok(AUse { edge: asm.line(a, b)?, forward: true, given: None });
        }
        let frame = Frame { origin: arr(self.o + self.axis * p.y), x_axis: arr(self.radial), y_axis: arr(self.tangent) };
        let (ta, tb) = (p.x / self.r, q.x / self.r);
        let forward = ta < tb;
        let (interval, ends) = if forward { ([ta, tb], (a, b)) } else { ([tb, ta], (b, a)) };
        let circle = Curve::Circle { frame, radius };
        let (circle, interval) = match &self.label {
            Some(label) if (radius - self.r).abs() <= self.tol => form::on_frame(&circle, interval, label, self.tol).unwrap_or((circle, interval)),
            _ => (circle, interval),
        };
        Ok(AUse { edge: asm.edge(circle, interval, ends.0, ends.1)?, forward, given: None })
    }

    /// A profile loop wrapped onto the wall: its footprint at the wall's radius and its cap's
    /// loop at `rr`, both running as the loop does, and one side wall per segment between them
    /// (each loop seen from outside the body). A segment along the axis or round the wall keeps
    /// its exact line or arc and a planar wall; any other is fitted ([`Wrap::fitted`]).
    fn wrapped_loop(&self, asm: &mut Assembly, lp: &Loop2, rr: f64, shell: usize) -> Result<(Vec<AUse>, Vec<AUse>, Vec<AFace>)> {
        let corners: Vec<Vec2> = lp.segs.iter().map(Seg2::start).collect();
        let base: Vec<u32> = corners.iter().map(|p| asm.vertex(self.at(*p, self.r))).collect::<Result<_>>()?;
        let top: Vec<u32> = corners.iter().map(|p| asm.vertex(self.at(*p, rr))).collect::<Result<_>>()?;
        let radial: Vec<u32> = base.iter().zip(&top).map(|(a, b)| asm.line(*a, *b)).collect::<Result<_>>()?;
        let n = corners.len();
        let (mut be, mut te, mut walls) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for (i, seg) in lp.segs.iter().enumerate() {
            let j = (i + 1) % n;
            let (Some(&p), Some(&q), Some(&ba), Some(&bb), Some(&ta), Some(&tb), Some(&ra), Some(&rb)) =
                (corners.get(i), corners.get(j), base.get(i), base.get(j), top.get(i), top.get(j), radial.get(i), radial.get(j))
            else {
                return Err(fail("loop topology"));
            };
            let (eb, et, wall) = if self.exact(seg) {
                let eb = self.edge(asm, ba, bb, p, q, self.r)?;
                let et = self.edge(asm, ta, tb, p, q, rr)?;
                // The wall's plane, its natural normal along the loop's turning: out of the
                // material whether the cap is raised or sunk.
                let mid = (p + q) * 0.5;
                let ring = [self.at(p, self.r), self.at(mid, self.r), self.at(q, self.r), self.at(q, rr), self.at(mid, rr), self.at(p, rr)];
                let normal = newell(&ring).ok_or_else(|| fail("a wall of no area"))?;
                let u = (ring[2] - ring[0]).normalized().ok_or_else(|| fail("a wall of no width"))?;
                let u = (u - normal * u.dot(normal)).normalized().ok_or_else(|| fail("a wall of no width"))?;
                let surface = Surface::Plane { origin: arr(ring[0]), u_axis: arr(u), v_axis: arr(normal.cross(u)) };
                let lp = vec![
                    eb.clone(),
                    AUse { edge: rb, forward: true, given: None },
                    AUse { edge: et.edge, forward: !et.forward, given: None },
                    AUse { edge: ra, forward: false, given: None },
                ];
                (eb, et, AFace { surface, nurbs: None, natural_out: true, loops: vec![lp], shell, domain: None })
            } else {
                self.fitted(asm, seg, q, rr, [ba, bb, ta, tb, ra, rb], shell)?
            };
            walls.push(wall);
            be.push(eb);
            te.push(et);
        }
        Ok((be, te, walls))
    }

    /// Take `n` spans from the emboss's budget, or refuse it as too detailed.
    fn take_spans(&self, n: usize) -> Result<()> {
        let left = self.spans.get().checked_sub(n).ok_or_else(|| fail("the profiles are too detailed to wrap"))?;
        self.spans.set(left);
        Ok(())
    }

    /// Whether segment `seg` (wall coordinates) wraps exactly: a line along the axis or round
    /// the wall. Any other is fitted.
    fn exact(&self, seg: &Seg2) -> bool {
        let d = seg.end() - seg.start();
        matches!(seg, Seg2::Line { .. }) && (d.x.abs() <= self.tol || d.y.abs() <= self.tol)
    }

    /// That the wall face's form is the one [`OnForm`] describes (its frame's quarter grid), at
    /// the start of the fitted segment `seg`: its pcurves are fitted on that form.
    fn check_wall_form(&self, selected: &AFace, seg: Option<&Seg2>) -> Result<()> {
        let refuse = || fail("the wall's surface form is not on its frame's quarter grid");
        let (Some(on), Some(nurbs), Some(seg)) = (self.wall_form, selected.nurbs.as_ref(), seg) else { return Err(refuse()) };
        let eval = SurfaceEval::new(nurbs).map_err(|_| refuse())?;
        let c = seg.start();
        let (mut u, v) = (on.u0 + on.k * form::param(on.delta + on.sigma * c.x / self.r), on.v0 + on.vk * c.y);
        // Onto the face's own turn of the form (four quarters to a turn).
        if let Some([[u0, _], _]) = selected.domain {
            u -= ((u - u0) / (4.0 * on.k)).floor() * 4.0 * on.k;
        }
        let hit = eval.point([u, v]).ok_or_else(refuse)?;
        if hit.dist(self.at(c, self.r)) > self.fit {
            return Err(refuse());
        }
        Ok(())
    }

    /// The cap's form over region `region`'s box (wall coordinates) at `rr`, and its domain.
    fn cap_form(&self, region: &Region2, rr: f64) -> Result<(Surface, [[f64; 2]; 2])> {
        let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for seg in region.loops().flat_map(|l| &l.segs) {
            let (l, h) = seg_box(seg);
            lo = Vec2::new(lo.x.min(l.x), lo.y.min(l.y));
            hi = Vec2::new(hi.x.max(h.x), hi.y.max(h.y));
        }
        let pad = self.tol * 100.0;
        let on = self.cap_form;
        let (v0, v1) = (on.v0 + on.vk * lo.y, on.v0 + on.vk * hi.y);
        // The angle padding keeps the domain short of a whole turn: a padded domain of a turn
        // would take the form of a whole turn from the quarter its padded start lies in, which
        // can end before the boundary's own angles (a footprint near a whole turn).
        let (a0, a1) = (lo.x / self.r, hi.x / self.r);
        let pad_u = (pad / self.r).min(0.5 * (std::f64::consts::TAU * (1.0 - 1e-8) - (a1 - a0)));
        if pad_u.is_nan() || pad_u <= 0.0 {
            return Err(fail("profile wraps a whole turn"));
        }
        let domain = [[a0 - pad_u, a1 + pad_u], [v0.min(v1) - pad, v0.max(v1) + pad]];
        form::surface(&self.cylinder(rr), domain).ok_or_else(|| fail("cap form"))
    }

    /// The pcurve of the wrap of segment `seg` (ending at the next corner `q`) on the form `on`
    /// of a cylinder of `radius`, in the edge's parameter t in [0, 1] (the segment's own): the
    /// cubic Hermite spline through the form parameters and their rates at breaks that include
    /// every quarter line it crosses (where the parameter is only C1), with enough breaks
    /// between them that it lifts within `fit` of the wrap. Lifted, an error `du` in u moves at
    /// most `radius · 4tan(π/8) · du` and one in v as much; the parameter's fourth derivative
    /// in t is bounded from [`PARAM_BOUNDS`] and the segment's [`derivative_bounds`] by Faà di
    /// Bruno's formula, as in [`fit_spans`].
    fn pcurve(&self, seg: &Seg2, q: Vec2, on: OnForm, radius: f64) -> Result<Curve2> {
        let b = derivative_bounds(seg).ok_or_else(|| fail("a profile curve whose shape cannot be bounded"))?;
        let across = across_bounds(seg).ok_or_else(|| fail("a profile curve whose shape cannot be bounded"))?;
        let th: Vec<f64> = across.iter().map(|x| x / self.r).collect();
        let (Some(&t1), Some(&t2), Some(&t3), Some(&t4)) = (th.first(), th.get(1), th.get(2), th.get(3)) else { return Err(fail("bounds")) };
        let [g1, g2, g3, g4] = PARAM_BOUNDS;
        let u4 = g1 * t4 + g2 * (4.0 * t1 * t3 + 3.0 * t2 * t2) + 6.0 * g3 * t1 * t1 * t2 + g4 * t1.powi(4);
        let m = radius * ANGLE_RATE * u4 + b[3];
        let room = self.fit - seg.end().dist(q) * (radius / self.r).max(1.0);
        if room.is_nan() || room <= 0.0 {
            return Err(fail("a profile curve is too detailed to wrap"));
        }
        // The quarter lines the segment crosses, as values of its first coordinate.
        let (lo, hi) = seg_box(seg);
        let quarter = std::f64::consts::FRAC_PI_2;
        let angle = |x: f64| on.delta + on.sigma * x / self.r;
        let (a0, a1) = (angle(lo.x).min(angle(hi.x)), angle(lo.x).max(angle(hi.x)));
        let mut cuts = vec![0.0, 1.0];
        let mut k = (a0 / quarter).ceil();
        while k * quarter <= a1 && cuts.len() < 64 {
            cuts.extend(crossings(seg, (k * quarter - on.delta) * self.r / on.sigma));
            k += 1.0;
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
        let mut breaks = vec![0.0];
        for w in cuts.windows(2) {
            let (a, z) = (w[0], w[1]);
            let n = ((z - a) * (3f64.sqrt() * m / (384.0 * room)).powf(0.25)).ceil().max(1.0);
            if !n.is_finite() || n > MAX_SPANS as f64 {
                return Err(fail("a profile curve is too detailed to wrap"));
            }
            let n = n as usize;
            breaks.extend((1..=n).map(|i| if i == n { z } else { a + (z - a) * i as f64 / n as f64 }));
        }
        self.take_spans(breaks.len())?;
        let mut knots = vec![0.0; 4];
        let mut points: Vec<[f64; 2]> = Vec::with_capacity(2 * breaks.len());
        let last = breaks.len() - 1;
        for (i, &t) in breaks.iter().enumerate() {
            let c = if i == last { q } else { seg.point_at(t) };
            let d = seg.derivative(t);
            let theta = angle(c.x);
            let (u, du) = (on.u0 + on.k * form::param(theta), on.k * param_rate(theta) * on.sigma * d.x / self.r);
            let (v, dv) = (on.v0 + on.vk * c.y, on.vk * d.y);
            if ![u, du, v, dv].iter().all(|x| x.is_finite()) {
                return Err(fail("a profile curve that does not evaluate"));
            }
            let before = i.checked_sub(1).and_then(|k| breaks.get(k)).map_or(0.0, |b| (t - b) / 3.0);
            let after = breaks.get(i + 1).map_or(0.0, |b| (b - t) / 3.0);
            if i > 0 {
                points.push([u - du * before, v - dv * before]);
            }
            if i == 0 || i == last {
                points.push([u, v]);
            }
            if i < last {
                points.push([u + du * after, v + dv * after]);
                if i > 0 {
                    knots.extend([t, t]);
                }
            }
        }
        knots.extend([1.0; 4]);
        Ok(Curve2::Spline { knots, points })
    }

    /// The wrap of a curved or slanted segment `seg` (wall coordinates; it ends at the next
    /// corner `q`): its base edge on the wall, a cubic B-spline through the wrap's points and
    /// tangents at even steps (Hermite, so C1 with double knots), with enough steps that it
    /// strays at most `fit` from the wrap ([`fit_spans`]); its top edge, the base's control points
    /// moved out radially to `rr`, which is the wrap at `rr` within `fit` too (the move is affine
    /// and scales errors by at most `max(1, rr / r)`, which the fit allows for); and the side wall
    /// between them, the ruled spline surface through both, whose ends are the radial edges.
    /// `v` holds the base corners, the top corners and the radial edges at the start and end.
    /// Every use on the side wall carries its exact pcurve (u along the segment, v from base to
    /// top).
    fn fitted(&self, asm: &mut Assembly, seg: &Seg2, q: Vec2, rr: f64, v: [u32; 6], shell: usize) -> Result<(AUse, AUse, AFace)> {
        let [ba, bb, ta, tb, ra, rb] = v;
        let scale = (rr / self.r).max(1.0);
        // Where the next corner is not exactly where this segment ends, the spline goes to the
        // corner: the gap counts against the fit.
        let room = self.fit / scale - seg.end().dist(q);
        let bounds = derivative_bounds(seg).ok_or_else(|| fail("a profile curve whose shape cannot be bounded"))?;
        let across = across_bounds(seg).ok_or_else(|| fail("a profile curve whose shape cannot be bounded"))?;
        let n = fit_spans(bounds, across, self.r, room).ok_or_else(|| fail("a profile curve is too detailed to wrap"))?;
        self.take_spans(n)?;
        let h = 1.0 / n as f64;
        // The B-spline's control points: each span's Bézier points, the point shared by two
        // spans left out (the double knot puts it halfway between its neighbours, where it is).
        let mut knots = vec![0.0; 4];
        let mut lower: Vec<Vec3> = Vec::with_capacity(2 * n + 2);
        for i in 0..=n {
            let t = i as f64 * h;
            let c = if i == n { q } else { seg.point_at(t) };
            let d = seg.derivative(t);
            if !c.is_finite() || !d.is_finite() {
                return Err(fail("a profile curve that does not evaluate"));
            }
            let w = self.at(c, self.r);
            let theta = c.x / self.r;
            let dw = (self.axis * d.y + (self.tangent * theta.cos() - self.radial * theta.sin()) * d.x) * (h / 3.0);
            if i == 0 {
                lower.extend([w, w + dw]);
            } else if i == n {
                lower.extend([w - dw, w]);
            } else {
                lower.extend([w - dw, w + dw]);
                knots.extend([t, t]);
            }
        }
        knots.extend([1.0; 4]);
        let (Some(pa), Some(pb), Some(qa), Some(qb)) = (asm.point(ba), asm.point(bb), asm.point(ta), asm.point(tb)) else {
            return Err(fail("vertex reference"));
        };
        let out = |p: Vec3| {
            let along = self.axis * (p - self.o).dot(self.axis);
            self.o + along + (p - self.o - along) * (rr / self.r)
        };
        let mut upper: Vec<Vec3> = lower.iter().map(|p| out(*p)).collect();
        // The ends are the corners' vertices exactly.
        for (row, a, b) in [(&mut lower, pa, pb), (&mut upper, qa, qb)] {
            if let Some(first) = row.first_mut() {
                *first = a;
            }
            if let Some(last) = row.last_mut() {
                *last = b;
            }
        }
        let spline = |row: &[Vec3]| Curve::Spline { knots: knots.clone(), points: row.iter().map(|p| arr(*p)).collect() };
        let eb = asm.edge(spline(&lower), [0.0, 1.0], ba, bb)?;
        let et = asm.edge(spline(&upper), [0.0, 1.0], ta, tb)?;
        let surface = Surface::Spline {
            u_knots: knots.clone(),
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: lower.iter().zip(&upper).map(|(a, b)| vec![arr(*a), arr(*b)]).collect(),
        };
        let line = |a: [f64; 2], b: [f64; 2]| Some(Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![a, b] });
        let lp = vec![
            AUse { edge: eb, forward: true, given: line([0.0, 0.0], [1.0, 0.0]) },
            AUse { edge: rb, forward: true, given: line([1.0, 0.0], [1.0, 1.0]) },
            AUse { edge: et, forward: false, given: line([0.0, 1.0], [1.0, 1.0]) },
            AUse { edge: ra, forward: false, given: line([0.0, 0.0], [0.0, 1.0]) },
        ];
        let face = AFace { surface, nurbs: None, natural_out: true, loops: vec![lp], shell, domain: None };
        let on_wall = self.wall_form.ok_or_else(|| fail("the wall's surface form is not on its frame's quarter grid"))?;
        let below = self.pcurve(seg, q, on_wall, self.r)?;
        let above = self.pcurve(seg, q, self.cap_form, rr)?;
        Ok((AUse { edge: eb, forward: true, given: Some(below) }, AUse { edge: et, forward: true, given: Some(above) }, face))
    }
}

/// The most cubic spans one fitted edge or pcurve takes, and all of one emboss's together: past
/// them a profile is refused as too detailed rather than built slowly.
const MAX_SPANS: usize = 512;
const MAX_TOTAL_SPANS: usize = 50_000;

/// Bounds on the size of a segment's derivatives 1 to 4 in its own parameter over [0, 1]: a
/// line's and an arc's exactly, a polynomial Bézier's from its derivatives' control points (a
/// Bézier lies in their hull), a conic's from its rational form (`C d = N` differentiated by
/// Leibniz's rule, `d` at least its least value on [0, 1]). `None` for a shape that does not
/// bound (a conic weight that is not positive).
fn derivative_bounds(seg: &Seg2) -> Option<[f64; 4]> {
    let b = match *seg {
        Seg2::Line { a, b } => [b.dist(a), 0.0, 0.0, 0.0],
        Seg2::Arc { radius, sweep, .. } => {
            let s = sweep.abs();
            [radius * s, radius * s * s, radius * s.powi(3), radius * s.powi(4)]
        }
        Seg2::Cubic { p0, p1, p2, p3 } => polynomial_bounds(&[p0, p1, p2, p3]),
        Seg2::Bezier { n, ref p } => polynomial_bounds(p.get(..=usize::from(n))?),
        Seg2::Conic { a, apex, b, w } => {
            if !w.is_finite() || w <= 0.0 {
                return None;
            }
            // About `a`: N = 2w(apex - a)t(1 - t) + (b - a)t², d = 1 + 2(w - 1)t(1 - t), whose
            // Bernstein coefficients are (0, w(apex - a), b - a) and (1, w, 1).
            let (q1, q2) = ((apex - a) * w, b - a);
            let b0 = (apex - a).len().max(q2.len());
            let n1 = (q1 * 2.0).len().max(((q2 - q1) * 2.0).len());
            let n2 = ((q2 - q1 * 2.0) * 2.0).len();
            let (d1, d2) = (2.0 * (w - 1.0).abs(), 4.0 * (w - 1.0).abs());
            let dmin = (0.5 * (1.0 + w)).min(1.0);
            let b1 = (n1 + d1 * b0) / dmin;
            let b2 = (n2 + 2.0 * d1 * b1 + d2 * b0) / dmin;
            let b3 = (3.0 * d1 * b2 + 3.0 * d2 * b1) / dmin;
            let b4 = (4.0 * d1 * b3 + 6.0 * d2 * b2) / dmin;
            [b1, b2, b3, b4]
        }
    };
    b.iter().all(|x| x.is_finite()).then_some(b)
}

/// [`derivative_bounds`] of a segment's first coordinate alone (in wall coordinates, the way
/// round the wall; the height runs straight and does not turn the wrap): those of its shadow on
/// that axis, its control points' first coordinates (a Bézier or conic's shadow is the Bézier
/// or conic of its controls' shadows); an arc's are its own.
fn across_bounds(seg: &Seg2) -> Option<[f64; 4]> {
    match seg {
        Seg2::Arc { .. } => derivative_bounds(seg),
        _ => derivative_bounds(&seg.map_controls(|p| Vec2::new(p.x, 0.0))),
    }
}

/// [`derivative_bounds`] of the polynomial Bézier with control points `p`.
fn polynomial_bounds(p: &[Vec2]) -> [f64; 4] {
    let mut out = [0.0; 4];
    let mut diffs: Vec<Vec2> = p.to_vec();
    let mut factor = 1.0;
    for slot in &mut out {
        let degree = diffs.len().saturating_sub(1);
        if degree == 0 {
            break;
        }
        factor *= degree as f64;
        diffs = diffs.windows(2).map(|w| w[1] - w[0]).collect();
        *slot = factor * diffs.iter().map(|d| d.len()).fold(0.0, f64::max);
    }
    out
}

/// How many even spans a cubic Hermite fit of the wrap of a segment needs to stay within `room`
/// of it, from the segment's [`derivative_bounds`] `b`, those of its first coordinate alone
/// `across` ([`across_bounds`]: only the way round the wall bends the wrap) and the wall radius `r`. The wrap is
/// `axis z + r (radial cos(s/r) + tangent sin(s/r))`; Faà di Bruno's formula bounds its fourth
/// derivative by `M = |z''''| + |s''''| + (4|s'||s'''| + 3|s''|²)/r + 6|s'|²|s''|/r² + |s'|⁴/r³`,
/// and a Hermite cubic on a span of length h misses a function by at most `h⁴/384` of its
/// fourth derivative, in each of the three coordinates. `None` past [`MAX_SPANS`] or without room.
fn fit_spans(b: [f64; 4], across: [f64; 4], r: f64, room: f64) -> Option<usize> {
    let [s1, s2, s3, _] = across;
    if room.is_nan() || room <= 0.0 || r.is_nan() || r <= 0.0 {
        return None;
    }
    let m = 2.0 * b[3] + (4.0 * s1 * s3 + 3.0 * s2 * s2) / r + 6.0 * s1 * s1 * s2 / (r * r) + s1.powi(4) / r.powi(3);
    let n = (3f64.sqrt() * m / (384.0 * room)).powf(0.25).ceil().max(1.0);
    (n.is_finite() && n <= MAX_SPANS as f64).then_some(n as usize)
}

/// `seg` (sketch coordinates) in the wall's coordinates (arc length round the wall from the
/// sketch's radial, height along the axis): the sketch plane is tangent to the wall, so the map
/// is a rigid motion of the plane, which keeps every kind of segment (an arc turns, or runs the
/// other way round where the map mirrors). `None` for an arc where the frame is not tangent.
fn on_wall(seg: &Seg2, plane: &Plane, s0: f64, z0: f64, tangent: Vec3, axis: Vec3) -> Option<Seg2> {
    let m = |p: Vec2| Vec2::new(plane.dir_to_world(p).dot(tangent), plane.dir_to_world(p).dot(axis));
    let map = |p: Vec2| m(p) + Vec2::new(s0, z0);
    Some(match *seg {
        Seg2::Arc { center, radius, start, sweep } => {
            let (cx, cy) = (m(Vec2::new(1.0, 0.0)), m(Vec2::new(0.0, 1.0)));
            if (cx.len() - 1.0).abs() > 1e-9 || (cy.len() - 1.0).abs() > 1e-9 || cx.dot(cy).abs() > 1e-9 {
                return None;
            }
            let det = cx.x * cy.y - cx.y * cy.x;
            let dir = m(Vec2::new(start.cos(), start.sin()));
            Seg2::Arc { center: map(center), radius, start: dir.y.atan2(dir.x), sweep: sweep * det.signum() }
        }
        _ => seg.map_controls(map),
    })
}

/// A box round a segment: a line's ends, a polynomial or conic Bézier's control points (it lies
/// in their hull; a conic's weight is positive), an arc's ends and the extremes it sweeps over.
fn seg_box(seg: &Seg2) -> (Vec2, Vec2) {
    let points: Vec<Vec2> = match *seg {
        Seg2::Line { a, b } => vec![a, b],
        Seg2::Cubic { p0, p1, p2, p3 } => vec![p0, p1, p2, p3],
        Seg2::Conic { a, apex, b, .. } => vec![a, apex, b],
        Seg2::Bezier { n, ref p } => p.iter().take(usize::from(n) + 1).copied().collect(),
        Seg2::Arc { center, radius, start, sweep } => {
            let mut v = vec![seg.start(), seg.end()];
            let (lo, hi) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
            let quarter = std::f64::consts::FRAC_PI_2;
            let mut k = (lo / quarter).ceil();
            while k * quarter <= hi && v.len() < 8 {
                v.push(center + Vec2::from_angle(k * quarter) * radius);
                k += 1.0;
            }
            v
        }
    };
    let first = points.first().copied().unwrap_or_default();
    points.iter().fold((first, first), |(l, h), p| (Vec2::new(l.x.min(p.x), l.y.min(p.y)), Vec2::new(h.x.max(p.x), h.y.max(p.y))))
}

/// Whether segments `s` and `t` cross or come within `tol`: their boxes ([`seg_box`]) are
/// halved (exactly, [`Seg2Geom::split_half`]) until they part, or both are under `tol` across
/// where they meet. Out of `budget` halvings it answers that they meet, which refuses the
/// profile rather than build a body that crosses itself.
fn segs_meet(s: Seg2, t: Seg2, tol: f64, budget: &mut usize) -> bool {
    let ((sl, sh), (tl, th)) = (seg_box(&s), seg_box(&t));
    if sl.x > th.x + tol || tl.x > sh.x + tol || sl.y > th.y + tol || tl.y > sh.y + tol {
        return false;
    }
    let (ss, ts) = ((sh - sl).len(), (th - tl).len());
    if ss <= tol && ts <= tol {
        return true;
    }
    if *budget == 0 {
        return true;
    }
    *budget -= 1;
    if ss >= ts {
        let (a, b) = s.split_half();
        segs_meet(a, t, tol, budget) || segs_meet(b, t, tol, budget)
    } else {
        let (a, b) = t.split_half();
        segs_meet(s, a, tol, budget) || segs_meet(s, b, tol, budget)
    }
}

/// Whether segment `s` lies strictly inside the box from `lo` to `hi`: its [`seg_box`] does, or
/// each half does (halved exactly, as in [`segs_meet`]), so control points past a limit that the
/// curve itself stays short of do not refuse it. A piece under `tol` across that still reaches a
/// limit, or running out of `budget` halvings, answers no, which refuses the profile.
fn clear_within(s: Seg2, lo: Vec2, hi: Vec2, tol: f64, budget: &mut usize) -> bool {
    let (sl, sh) = seg_box(&s);
    if sl.x > lo.x && sh.x < hi.x && sl.y > lo.y && sh.y < hi.y {
        return true;
    }
    if sh.x <= lo.x || sl.x >= hi.x || sh.y <= lo.y || sl.y >= hi.y || (sh - sl).len() <= tol || *budget == 0 {
        return false;
    }
    *budget -= 1;
    let (a, b) = s.split_half();
    clear_within(a, lo, hi, tol, budget) && clear_within(b, lo, hi, tol, budget)
}

/// A loop of one closed segment split into its two halves (both over the segment); any other
/// loop as it is.
fn split_single(l: &NumberedLoop) -> NumberedLoop {
    match (l.lp.segs.as_slice(), l.from.as_slice()) {
        ([seg], [from]) => {
            let (a, b) = seg.split_half();
            NumberedLoop { lp: Loop2 { segs: vec![a, b] }, from: vec![from.clone(), from.clone()] }
        }
        _ => l.clone(),
    }
}

/// The unit normal of a planar ring of points, along its turning (right-hand rule).
fn newell(ring: &[Vec3]) -> Option<Vec3> {
    let n = ring.len();
    let mut s = Vec3::ZERO;
    for i in 0..n {
        let (a, b) = (*ring.get(i)?, *ring.get((i + 1) % n)?);
        s += a.cross(b);
    }
    s.normalized()
}

/// The span along `axis` from `o` of face `face`'s edges, from their exact bounds (a line's
/// ends, a circle's extreme heights, a spline's control points); `None` for any other curve.
fn face_axial_bounds(b: &Brep, face: usize, o: Vec3, axis: Vec3) -> Result<Option<(f64, f64)>> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut count = 0usize;
    let mut add = |z0: f64, z1: f64| -> Result<()> {
        count = count.saturating_add(1);
        if count > 100_000 || !z0.is_finite() || !z1.is_finite() {
            return Err(fail("target boundary work limit or nonfinite geometry"));
        }
        lo = lo.min(z0);
        hi = hi.max(z1);
        Ok(())
    };
    let height = |p: Vec3| (p - o).dot(axis);
    for e in face_edges(b, face) {
        let edge = b.edges.get(e as usize).ok_or_else(|| fail("edge"))?;
        match &edge.curve {
            Curve::Line { .. } => {
                let c = CurveEval::new(edge.parameters()).map_err(|e| fail(&e.to_string()))?;
                for t in edge.interval {
                    let z = height(c.point(t).ok_or_else(|| fail("edge point"))?);
                    add(z, z)?;
                }
            }
            Curve::Circle { frame, radius } => {
                let (x, y) = (Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
                let z = height(Vec3::from(frame.origin));
                let reach = radius * x.dot(axis).hypot(y.dot(axis));
                add(z - reach, z + reach)?;
            }
            Curve::Spline { points, .. } => {
                for p in points {
                    let z = height(Vec3::from(*p));
                    add(z, z)?;
                }
            }
            Curve::RationalSpline { homogeneous, .. } => {
                for h in homogeneous {
                    if !h[3].is_finite() || h[3] <= 0.0 {
                        return Ok(None);
                    }
                    let z = height(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
                    add(z, z)?;
                }
            }
            Curve::Ellipse { .. } | Curve::Polyline { .. } | Curve::SurfaceIntersection { .. } => return Ok(None),
        }
    }
    Ok((lo.is_finite() && hi.is_finite()).then_some((lo, hi)))
}

fn profile_interval_overlaps(plane: &Plane, regions: &[Region2], o: Vec3, axis: Vec3, lo: f64, hi: f64) -> Result<bool> {
    let z0 = (plane.origin - o).dot(axis);
    let direction = Vec2::new(plane.x.dot(axis), plane.y.dot(axis));
    let mut count = 0usize;
    let mut overlaps = false;
    for region in regions {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for segment in &region.outer.segs {
            count = count.saturating_add(1);
            if count > 4096 {
                return Err(fail("profile geometry budget"));
            }
            let mut add = |p: Vec2| -> Result<()> {
                if !p.is_finite() || p.len() > 1e7 {
                    return Err(fail("nonfinite or excessive profile coordinates"));
                }
                let z = z0 + p.dot(direction);
                min = min.min(z);
                max = max.max(z);
                Ok(())
            };
            match *segment {
                Seg2::Line { a, b } => {
                    add(a)?;
                    add(b)?;
                }
                Seg2::Arc { center, radius, .. } => {
                    add(center)?;
                    if !radius.is_finite() || radius <= 0.0 || radius > 1e7 {
                        return Err(fail("invalid profile radius"));
                    }
                    let z = z0 + center.dot(direction);
                    min = min.min(z - radius * direction.len());
                    max = max.max(z + radius * direction.len());
                }
                Seg2::Cubic { p0, p1, p2, p3 } => {
                    for p in [p0, p1, p2, p3] {
                        add(p)?;
                    }
                }
                Seg2::Conic { a, apex, b, w } if w.is_finite() && w > 0.0 => {
                    for p in [a, apex, b] {
                        add(p)?;
                    }
                }
                Seg2::Bezier { n, p } if (1..=solidcore_record::MAX_BEZIER_DEGREE as u8).contains(&n) => {
                    for p in p.iter().take(usize::from(n) + 1) {
                        add(*p)?;
                    }
                }
                _ => return Err(fail("unsupported profile geometry")),
            }
        }
        overlaps |= min < hi && max > lo;
    }
    Ok(overlaps)
}

fn validate_profiles(regions: &[Region2], tol: f64) -> Result<()> {
    let loops: Vec<_> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).collect();
    let edges: Vec<_> = loops.iter().enumerate().flat_map(|(l, lp)| lp.segs.iter().enumerate().map(move |(i, s)| (l, i, *s))).collect();
    for (k, (l, i, s)) in edges.iter().enumerate() {
        for (ll, j, t) in edges.iter().skip(k + 1) {
            let len = loops.get(*l).map(|lp| lp.segs.len()).unwrap_or(0);
            if l == ll && ((*i + 1) % len == *j || (*j + 1) % len == *i) {
                continue;
            }
            let (a, b, c, d) = (s.start(), s.end(), t.start(), t.end());
            let ab = b - a;
            let cd = d - c;
            let cross = ab.cross(cd);
            let intersects = match (s, t) {
                (Seg2::Line { .. }, Seg2::Line { .. }) if cross.abs() > tol * tol => {
                    let u = (c - a).cross(cd) / cross;
                    let v = (c - a).cross(ab) / cross;
                    (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
                }
                (Seg2::Line { .. }, Seg2::Line { .. }) => false,
                // A curve's chord is not the curve: halve both until their boxes part or meet.
                _ => {
                    let mut budget = 20_000usize;
                    segs_meet(*s, *t, tol, &mut budget)
                }
            };
            if intersects
                || [a, b].iter().any(|p| t.point_at(t.closest_param(*p)).dist(*p) < tol)
                || [c, d].iter().any(|p| s.point_at(s.closest_param(*p)).dist(*p) < tol)
            {
                return Err(fail("profile boundaries intersect or touch"));
            }
        }
    }
    for (i, r) in regions.iter().enumerate() {
        if r.area() <= tol * tol {
            return Err(fail("profile has no area"));
        }
        for (h, lp) in r.holes.iter().enumerate() {
            if !r.outer.contains(lp.segs.first().ok_or_else(|| fail("empty hole"))?.start()) {
                return Err(fail("hole outside profile"));
            }
            for other in r.holes.iter().skip(h + 1) {
                if lp.contains(other.segs.first().ok_or_else(|| fail("empty hole"))?.start())
                    || other.contains(lp.segs.first().ok_or_else(|| fail("empty hole"))?.start())
                {
                    return Err(fail("nested profile holes"));
                }
            }
        }
        for other in regions.iter().skip(i + 1) {
            if r.contains(other.outer.segs.first().ok_or_else(|| fail("empty profile"))?.start())
                || other.contains(r.outer.segs.first().ok_or_else(|| fail("empty profile"))?.start())
            {
                return Err(fail("overlapping profiles"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Region2 {
        Region2 { outer: Loop2::polygon(&[Vec2::new(-2., 5.), Vec2::new(2., 5.), Vec2::new(2., 10.), Vec2::new(-2., 10.)]), holes: vec![] }
    }
    fn plane() -> Plane {
        Plane::new(Vec3::Y * 10., -Vec3::X, Vec3::Z).unwrap()
    }
    fn volume(b: &Body) -> f64 {
        crate::measure(b).unwrap().volume
    }
    #[test]
    fn inference_uses_finite_wall_footprint_and_an_interior_profile_anchor() {
        let near = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let far = crate::cylinder(Vec3::Z * 100., Vec3::Z, 10., 20.).unwrap();
        let other = crate::cylinder(Vec3::Z * 120., Vec3::Z, 8., 10.).unwrap();
        let unsupported = crate::ops::join_pieces(vec![far.clone(), other]).unwrap().unwrap();
        let regions = [profile()];
        for body in [&far, &unsupported] {
            assert!(infer_cylinder_emboss(body, &plane(), &regions, 1.).unwrap().is_none());
        }
        for x in [20., 80.] {
            let base = crate::cylinder(Vec3::X * x, Vec3::Z, 10., 20.).unwrap();
            let step = crate::cylinder(Vec3::X * x + Vec3::Z * 20., Vec3::Z, 8., 10.).unwrap();
            let remote = crate::ops::join_pieces(vec![base, step]).unwrap().unwrap();
            let inferred = infer_cylinder_emboss(&remote, &plane(), &regions, 1.);
            if x == 20. {
                // The offset maps this footprint across the remote wall's seam;
                // partial relevance must retain conservative unsupported refusal.
                assert!(inferred.is_err());
            } else {
                assert!(inferred.unwrap().is_none());
            }
        }
        let (index, anchor) = infer_cylinder_emboss(&near, &plane(), &regions, 1.).unwrap().unwrap();
        assert!(anchor.z > 0. && anchor.z < 20.);
        assert!((anchor.x.hypot(anchor.y) - 10.).abs() < 1e-9);
        assert!(cylinder_emboss_anchor(&near, index, anchor).unwrap().dist(anchor) < 1e-10);
        let shifted = Plane::new(Vec3::new(-3., 10., 0.), -Vec3::X, Vec3::Z).unwrap();
        let shifted_profile =
            Region2 { outer: Loop2::polygon(&[Vec2::new(-5., 5.), Vec2::new(-1., 5.), Vec2::new(-1., 10.), Vec2::new(-5., 10.)]), holes: vec![] };
        let (_, other_anchor) = infer_cylinder_emboss(&near, &shifted, std::slice::from_ref(&shifted_profile), 1.).unwrap().unwrap();
        assert!(other_anchor.dist(anchor) < 1e-10);
        let before = near.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
        let result = emboss_cylinder_at(&near, other_anchor, &shifted, &[shifted_profile], 1.).unwrap();
        assert!((result.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume - before - 21.).abs() < 0.02);
        assert!(infer_cylinder_emboss(&near, &plane(), &regions, -10.).is_err());
        let inward = Plane::new(Vec3::Y * 10., Vec3::X, Vec3::Z).unwrap();
        assert!(infer_cylinder_emboss(&near, &inward, &regions, 1.).unwrap_err().to_string().contains("outwards"));
    }
    #[test]
    fn wrapped_nested_island_uses_its_enclosing_hole_patch() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let mut ring = profile();
        ring.holes.push(Loop2::polygon(&[Vec2::new(-1., 6.), Vec2::new(1., 6.), Vec2::new(1., 9.), Vec2::new(-1., 9.)]));
        let island =
            Region2 { outer: Loop2::polygon(&[Vec2::new(-0.5, 6.5), Vec2::new(0.5, 6.5), Vec2::new(0.5, 8.5), Vec2::new(-0.5, 8.5)]), holes: vec![] };
        let before = body.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
        for depth in [1., -1.] {
            for regions in [vec![ring.clone(), island.clone()], vec![island.clone(), ring.clone()]] {
                let result = emboss_cylinder(&body, &plane(), &regions, depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                assert!(result.validity().is_empty());
                let after = result.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
                let expected = 16. * (depth + depth * depth / 20.);
                assert!((after - before - expected).abs() < 0.02, "{} vs {expected}", after - before);
                let mesh = result.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
                for (s, raised) in [(0., true), (0.75, false), (1.5, true)] {
                    let radial = -Vec3::X * (s / 10.0_f64).sin() + Vec3::Y * (s / 10.0_f64).cos();
                    let radius = if depth > 0. { 10.5 } else { 9.5 };
                    assert_eq!(mesh.contains(radial * radius + Vec3::Z * 7.5), if depth > 0. { raised } else { !raised });
                }
            }
        }
    }

    #[test]
    fn exact_wrapped_rectangle_is_connected_and_has_radial_depth() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        for depth in [1., -1.] {
            let result = emboss_cylinder(&body, &plane(), &[profile()], depth).unwrap();
            assert_eq!(result.shell_count(), 1);
            assert!(result.validity().is_empty());
            let before = body.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
            let after = result.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
            let expected = if depth > 0. { 21. } else { -19. };
            assert!((after - before - expected).abs() < 0.02, "{} vs {expected}", after - before);
            let mesh = result.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
            for s in [-1.5, 0., 1.5] {
                let radial = -Vec3::X * (s / 10.0_f64).sin() + Vec3::Y * (s / 10.0_f64).cos();
                assert_eq!(mesh.contains(radial * (if depth > 0. { 10.5 } else { 9.5 }) + Vec3::Z * 7.5), depth > 0.);
            }
        }
    }

    #[test]
    fn wrapped_holes_and_concave_profiles_preserve_material_on_rotated_axes() {
        let origin = Vec3::new(123.0, -456.0, 789.0);
        let axis = Vec3::X;
        let radial = Vec3::Y;
        let tangent = Vec3::Z;
        let body = crate::cylinder(origin, axis, 10.0, 20.0).unwrap();
        let plane = Plane::new(origin + radial * 10.0, tangent, axis).unwrap();
        let mut holed = profile();
        holed.holes.push(Loop2::polygon(&[Vec2::new(-1., 6.), Vec2::new(1., 6.), Vec2::new(1., 9.), Vec2::new(-1., 9.)]));
        let concave = Region2 {
            outer: Loop2::polygon(&[
                Vec2::new(-2., 5.),
                Vec2::new(2., 5.),
                Vec2::new(2., 7.),
                Vec2::new(0., 7.),
                Vec2::new(0., 10.),
                Vec2::new(-2., 10.),
            ]),
            holes: vec![],
        };
        for profile in [holed, concave] {
            for depth in [1., -1.] {
                let result = emboss_cylinder_at(&body, origin + radial * 10.0 + axis * 7.5, &plane, std::slice::from_ref(&profile), depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                let before = body.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
                let after = result.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
                let want = profile.area().abs() * (depth + depth * depth / 20.0);
                assert!((after - before - want).abs() < 0.02, "{} vs {want}", after - before);
                let point = |s: f64, z: f64, r: f64| origin + axis * z + (radial * (s / 10.0).cos() + tangent * (s / 10.0).sin()) * r;
                let mesh = result.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
                for (s, z) in [(-1.5, 7.5), (1.5, 7.5), (0.5, 6.5)] {
                    let material = profile.contains(Vec2::new(s, z));
                    assert_eq!(mesh.contains(point(s, z, if depth > 0. { 10.5 } else { 9.5 })), if depth > 0. { material } else { !material });
                }
                let imported = crate::step_import(&crate::step_export(&[&result], "wrapped cylinder").unwrap()).unwrap();
                assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                assert_eq!(imported.bodies.len(), 1);
                let round = imported.bodies.first().unwrap();
                assert!(round.body.validity().is_empty());
                let roundmesh = round.body.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
                assert_eq!(roundmesh.contains(point(-1.5, 7.5, if depth > 0. { 10.5 } else { 9.5 })), depth > 0.);
                let cut = crate::boolean(
                    &round.body,
                    &crate::box_solid(origin + Vec3::new(5., -20., -20.), origin + Vec3::new(10., 20., 20.)).unwrap(),
                    crate::BoolOp::Cut,
                )
                .unwrap()
                .unwrap();
                assert!(!cut.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().contains(point(-1.5, 7.5, 9.0)));
            }
        }
    }

    #[test]
    fn sleeve_depth_and_hostile_profiles_refuse_changes_atomically() {
        let outer = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let inner = crate::cylinder(-Vec3::Z, Vec3::Z, 8., 22.).unwrap();
        let sleeve = crate::boolean(&outer, &inner, crate::BoolOp::Cut).unwrap().unwrap();
        let good = emboss_cylinder(&sleeve, &plane(), &[profile()], -1.).unwrap();
        assert_eq!(good.shell_count(), 1);
        let before = crate::step_export(&[&sleeve], "unchanged").unwrap();
        let mut cap = profile();
        cap.outer = Loop2::polygon(&[Vec2::new(-2., 19.), Vec2::new(2., 19.), Vec2::new(2., 21.), Vec2::new(-2., 21.)]);
        // A circle wraps (#5358); a profile crossing itself does not.
        let circle = Region2 { outer: Loop2::circle(Vec2::new(0., 7.5), 1.), holes: vec![] };
        assert_eq!(emboss_cylinder(&sleeve, &plane(), &[circle], 1.).unwrap().shell_count(), 1);
        let mut crossing = profile();
        crossing.outer = Loop2::polygon(&[Vec2::new(-2., 5.), Vec2::new(2., 10.), Vec2::new(-2., 10.), Vec2::new(2., 5.)]);
        for (profiles, d) in [(vec![profile()], -2.), (vec![cap], 1.), (vec![crossing], 1.), (vec![profile()], f64::NAN), (vec![profile(); 65], 1.)] {
            assert!(emboss_cylinder(&sleeve, &plane(), &profiles, d).is_err());
        }
        // A pick anywhere on the exterior wall identifies it (#3782); one on the bore does not.
        assert!(emboss_cylinder_at(&sleeve, -Vec3::Y * 10. + Vec3::Z * 7.5, &plane(), &[profile()], 1.).is_ok());
        assert!(emboss_cylinder_at(&sleeve, -Vec3::Y * 8. + Vec3::Z * 7.5, &plane(), &[profile()], 1.).is_err());
        assert!(emboss_cylinder_at(&sleeve, -Vec3::Y * 10. + Vec3::Z * 20., &plane(), &[profile()], 1.).is_err());
        assert_eq!(crate::step_export(&[&sleeve], "unchanged").unwrap(), before);
    }

    /// The face of `body` on the surface `which` picks.
    fn face_where(body: &Body, which: impl Fn(Surf) -> bool) -> usize {
        let rec = record(body).unwrap();
        (0..rec.faces.len()).find(|f| surf_of(&rec, *f).is_some_and(&which)).unwrap()
    }
    fn square(h: f64) -> Region2 {
        Region2 { outer: Loop2::polygon(&[Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)]), holes: vec![] }
    }

    #[test]
    fn projected_emboss_on_a_plane_adds_and_removes_the_prism() {
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(40., 40., 20.)).unwrap();
        let top = face_where(&block, |s| matches!(s, Surf::Plane { n, .. } if n.z > 0.5));
        let at = emboss_anchor(&block, top, Vec3::new(20., 20., 20.0005)).unwrap();
        assert!((at.z - 20.).abs() < 1e-12);
        // The sketch floats above the face: the profile is projected down onto it.
        let sketch = Plane::new(Vec3::new(20., 20., 30.), Vec3::X, Vec3::Y).unwrap();
        let v = volume(&block);
        for depth in [2., -2.] {
            let result = emboss_project_face(&block, top, at, &sketch, &[square(5.)], depth).unwrap();
            assert_eq!(result.shell_count(), 1);
            assert!((volume(&result) - v - 200. * depth.signum()).abs() < 1e-4, "{depth}: {}", volume(&result) - v);
        }
        // A profile hanging over the face's edge (sketch x 15..25 is x 35..45 on a 40 wide
        // face) is refused, not cut short at the edge.
        let over =
            Region2 { outer: Loop2::polygon(&[Vec2::new(15., -5.), Vec2::new(25., -5.), Vec2::new(25., 5.), Vec2::new(15., 5.)]), holes: vec![] };
        for depth in [2., -2.] {
            let e = emboss_project_face(&block, top, at, &sketch, &[square(5.), over.clone()], depth).unwrap_err();
            assert!(e.to_string().contains("profile 2 of 2 lies off it"), "{e}");
        }
    }

    #[test]
    fn projected_emboss_on_concave_and_convex_cylinders() {
        // A trough: a box with a cylinder cut along y from its top.
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(40., 40., 40.)).unwrap();
        let tool = crate::cylinder(Vec3::new(20., 0., 40.), Vec3::Y, 15., 40.).unwrap();
        let trough = crate::boolean(&block, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let inner = face_where(&trough, |s| matches!(s, Surf::Cylinder { convex: false, .. }));
        let at = emboss_anchor(&trough, inner, Vec3::new(20., 20., 25.)).unwrap();
        let sketch = Plane::new(Vec3::new(20., 20., 45.), Vec3::X, Vec3::Y).unwrap();
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 20., 40.).unwrap();
        let wall = face_where(&cyl, |s| matches!(s, Surf::Cylinder { convex: true, .. }));
        let side = Plane::new(Vec3::new(25., 0., 20.), -Vec3::Y, Vec3::Z).unwrap();
        for (body, face, at, sketch) in [(&trough, inner, at, &sketch), (&cyl, wall, Vec3::new(20., 0., 20.), &side)] {
            let v = volume(body);
            // A 6 x 6 patch 1 deep on a wall of radius 15 or 20: about 36 either way.
            for depth in [1., -1.] {
                let result = emboss_project_face(body, face, at, sketch, &[square(3.)], depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                let change = (volume(&result) - v) * depth.signum();
                assert!((30.0..42.0).contains(&change), "{depth}: {change}");
            }
        }
        assert!(emboss_project_face(&cyl, 99, Vec3::new(20., 0., 20.), &side, &[square(3.)], 1.).is_err());
        // The trough's wall is half a turn, 23.6 either side of its bottom: a square laid from
        // 20 to 26 round it hangs off its edge.
        let past =
            Region2 { outer: Loop2::polygon(&[Vec2::new(20., -3.), Vec2::new(26., -3.), Vec2::new(26., 3.), Vec2::new(20., 3.)]), holes: vec![] };
        let e = emboss_project_face(&trough, inner, at, &sketch, &[past], -1.).unwrap_err();
        assert!(e.to_string().contains("not fitting the selected target face"), "{e}");
    }

    #[test]
    fn projected_cylinder_emboss_stays_on_the_selected_wall_and_within_reach() {
        // A stepped shaft: radius 10 up to z 10, radius 15 above it.
        let lower = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 10.).unwrap();
        let upper = crate::cylinder(Vec3::new(0., 0., 10.), Vec3::Z, 15., 10.).unwrap();
        let shaft = crate::boolean(&lower, &upper, crate::BoolOp::Union).unwrap().unwrap();
        let wall = face_where(&shaft, |s| matches!(s, Surf::Cylinder { r, convex: true, .. } if (r - 10.).abs() < 1e-9));
        let sketch = Plane::new(Vec3::new(10., 0., 0.), Vec3::Y, Vec3::Z).unwrap();
        let at = Vec3::new(10., 0., 5.);
        let square_at = |z: f64| Region2 {
            outer: Loop2::polygon(&[Vec2::new(-2., z - 2.), Vec2::new(2., z - 2.), Vec2::new(2., z + 2.), Vec2::new(-2., z + 2.)]),
            holes: vec![],
        };
        let v = volume(&shaft);
        let result = emboss_project_face(&shaft, wall, at, &sketch, &[square_at(5.)], -1.).unwrap();
        let removed = v - volume(&result);
        assert!((14.0..18.0).contains(&removed), "{removed}");
        // A profile above the step lies off the wall: it is refused rather than cut into the
        // wider section as a closed cavity.
        let e = emboss_project_face(&shaft, wall, at, &sketch, &[square_at(5.), square_at(15.)], -1.).unwrap_err();
        assert!(e.to_string().contains("not fitting the selected target face"), "{e}");
        // So is one whose centre is on the wall (z 9) but whose top (z 11) is past the step.
        let e = emboss_project_face(&shaft, wall, at, &sketch, &[square_at(9.)], -1.).unwrap_err();
        assert!(e.to_string().contains("not fitting the selected target face"), "{e}");
        // Flush with the step (z 6..10) still fits.
        assert!(emboss_project_face(&shaft, wall, at, &sketch, &[square_at(8.)], -1.).is_ok());
        // A depth the prisms cannot reach is refused, not cut short.
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 40.).unwrap();
        let side = face_where(&cyl, |s| matches!(s, Surf::Cylinder { convex: true, .. }));
        let e = emboss_project_face(&cyl, side, at, &sketch, &[square_at(5.)], 20.).unwrap_err();
        assert!(e.to_string().contains("radius"), "{e}");
    }

    /// A bore is wrapped like an outside wall, raised towards the axis and sunk away from it
    /// (#16683): exact counts, the annular sector's volume, and the depths that refuse.
    #[test]
    fn a_bore_is_embossed_and_debossed_to_exact_geometry() {
        let outer = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let hole = crate::cylinder(-Vec3::Z, Vec3::Z, 8., 22.).unwrap();
        let tube = crate::boolean(&outer, &hole, crate::BoolOp::Cut).unwrap().unwrap();
        // Tangent to the bore at +Y, with x running towards -X: an asymmetric rectangle.
        let bore = Plane::new(Vec3::Y * 8., -Vec3::X, Vec3::Z).unwrap();
        let rect =
            Region2 { outer: Loop2::polygon(&[Vec2::new(-1., 5.), Vec2::new(3., 5.), Vec2::new(3., 10.), Vec2::new(-1., 10.)]), holes: vec![] };
        let volume = |b: &Body| b.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
        let before = volume(&tube);
        let (faces, edges, vertices) = (tube.face_count(), tube.edge_count(), tube.vertex_count());
        // Wrapped at the bore's radius 8: the width 4 is the angle 0.5, the height 5.
        let sector = |rr: f64| 5. * 0.5 * (rr * rr - 64.).abs() / 2.;
        let (inferred, at) = infer_cylinder_emboss(&tube, &bore, std::slice::from_ref(&rect), -1.).unwrap().unwrap();
        assert!(cylinder_emboss_anchor(&tube, inferred, at).unwrap().dist(at) < 1e-10);
        for (depth, rr) in [(-1., 9.), (1., 7.)] {
            let result = emboss_cylinder_at(&tube, at, &bore, std::slice::from_ref(&rect), depth).unwrap();
            assert_eq!(result.shell_count(), 1);
            assert!(result.validity().is_empty());
            // Four walls and the floor or top; their eight corners, twelve edges.
            assert_eq!((result.face_count(), result.edge_count(), result.vertex_count()), (faces + 5, edges + 12, vertices + 8));
            let change = volume(&result) - before;
            let want = if depth < 0. { -sector(rr) } else { sector(rr) };
            assert!((change - want).abs() < 0.02, "{change} vs {want}");
            let mesh = result.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
            let point = |x: f64, z: f64, r: f64| -Vec3::X * r * (x / 8.).sin() + Vec3::Y * r * (x / 8.).cos() + Vec3::Z * z;
            for (x, inside) in [(2.5, true), (-0.5, true), (-2., false), (4., false)] {
                // Half way up the slot or boss, between the bore and its new surface.
                let probe = point(x, 7.5, 0.5 * (8. + rr));
                assert_eq!(mesh.contains(probe), if depth < 0. { !inside } else { inside });
            }
        }
        // Through the outer wall, or to the axis.
        for depth in [-2., -3., 8., 9.] {
            assert!(emboss_cylinder_at(&tube, at, &bore, std::slice::from_ref(&rect), depth).is_err(), "{depth}");
        }
        assert!(emboss_cylinder_at(&tube, at, &bore, std::slice::from_ref(&rect), -1.9).is_ok());
        // The sketch's side of the tangent plane does not matter in a bore.
        let flipped = Plane::new(Vec3::Y * 8., Vec3::X, Vec3::Z).unwrap();
        assert!(infer_cylinder_emboss(&tube, &flipped, &[rect], 1.).unwrap().is_some());
    }

    /// Curved and slanted profiles wrapped on a full outside wall and in a bore: the body stays
    /// one valid shell and changes by the wrapped region's exact volume, the profile's area times
    /// `|rr² - r²| / 2r`; every fitted edge lies on its cylinder within the fit.
    /// Profiles a looser bound refused wrap (#19229 review): a long slant that barely turns round
    /// a thin wall needs few spans (only the way round bends the wrap), and a curve whose control
    /// points pass the top endcap while the curve stays short of it is clear of the endcap.
    #[test]
    fn long_slants_and_curves_near_an_endcap_wrap() {
        let thin = crate::cylinder(Vec3::ZERO, Vec3::Z, 1., 200.).unwrap();
        let on_thin = Plane::new(Vec3::Y, -Vec3::X, Vec3::Z).unwrap();
        let slant = Loop2::polygon(&[Vec2::new(-0.25, 1.), Vec2::new(0.25, 1.), Vec2::new(0.45, 199.), Vec2::new(-0.05, 199.)]);
        // Endpoints at height 18, controls at 20: the curve peaks at 19.5 on a wall 20 high.
        let dome = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(2., 18.), b: Vec2::new(-2., 18.) },
                Seg2::Cubic { p0: Vec2::new(-2., 18.), p1: Vec2::new(-1., 20.), p2: Vec2::new(1., 20.), p3: Vec2::new(2., 18.) },
            ],
        };
        let outside = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        for (body, sketch, outer, r) in [(&thin, on_thin, slant, 1.), (&outside, plane(), dome, 10.)] {
            let region = Region2 { outer, holes: vec![] };
            let before = volume(body);
            for depth in [0.2, -0.2] {
                let rr: f64 = r + depth;
                let result = emboss_cylinder(body, &sketch, std::slice::from_ref(&region), depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                assert!(result.validity().is_empty());
                let want = depth.signum() * region.area() * (rr * rr - r * r).abs() / (2. * r);
                let change = volume(&result) - before;
                assert!((change - want).abs() < 1e-6 * region.area(), "r {r} depth {depth}: {change} vs {want}");
            }
        }
        // A curve that does cross the endcap is still refused.
        let past = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(2., 18.), b: Vec2::new(-2., 18.) },
                Seg2::Cubic { p0: Vec2::new(-2., 18.), p1: Vec2::new(-1., 21.), p2: Vec2::new(1., 21.), p3: Vec2::new(2., 18.) },
            ],
        };
        let e = emboss_cylinder(&outside, &plane(), &[Region2 { outer: past, holes: vec![] }], 0.2).unwrap_err();
        assert!(e.to_string().contains("seam or endcap"), "{e}");
    }

    #[test]
    fn curved_and_slanted_profiles_wrap_to_the_analytic_volume() {
        let circle = |c: Vec2, r: f64| Loop2::circle(c, r);
        let quarter = std::f64::consts::FRAC_1_SQRT_2;
        // A circle of four conic quarters, a lens of two parabolic conics, a blob of cubics.
        let conics = |c: Vec2, r: f64| Loop2 {
            segs: [(1., 0., 1., 1., 0., 1.), (0., 1., -1., 1., -1., 0.), (-1., 0., -1., -1., 0., -1.), (0., -1., 1., -1., 1., 0.)]
                .iter()
                .map(|&(ax, ay, px, py, bx, by)| Seg2::Conic {
                    a: c + Vec2::new(ax, ay) * r,
                    apex: c + Vec2::new(px, py) * r,
                    b: c + Vec2::new(bx, by) * r,
                    w: quarter,
                })
                .collect(),
        };
        let lens = Loop2 {
            segs: vec![
                Seg2::Conic { a: Vec2::new(-2., 7.5), apex: Vec2::new(0., 5.), b: Vec2::new(2., 7.5), w: 1. },
                Seg2::Conic { a: Vec2::new(2., 7.5), apex: Vec2::new(0., 10.), b: Vec2::new(-2., 7.5), w: 1. },
            ],
        };
        let k = 0.552_284_75 * 2.;
        let blob = Loop2 {
            segs: vec![
                Seg2::Cubic { p0: Vec2::new(2., 7.5), p1: Vec2::new(2., 7.5 + k), p2: Vec2::new(k, 9.5), p3: Vec2::new(0., 9.5) },
                Seg2::Cubic { p0: Vec2::new(0., 9.5), p1: Vec2::new(-k, 9.5), p2: Vec2::new(-2.5, 7.5 + k), p3: Vec2::new(-2.5, 7.5) },
                Seg2::Cubic { p0: Vec2::new(-2.5, 7.5), p1: Vec2::new(-2.5, 7.5 - k), p2: Vec2::new(-k, 5.5), p3: Vec2::new(0., 5.5) },
                Seg2::Cubic { p0: Vec2::new(0., 5.5), p1: Vec2::new(k, 5.5), p2: Vec2::new(2., 7.5 - k), p3: Vec2::new(2., 7.5) },
            ],
        };
        let slanted = Loop2::polygon(&[Vec2::new(-2., 5.), Vec2::new(1., 5.), Vec2::new(2., 10.), Vec2::new(-1., 10.)]);
        let profiles = [
            Region2 { outer: circle(Vec2::new(0., 7.5), 2.), holes: vec![] },
            Region2 { outer: slanted, holes: vec![] },
            Region2 { outer: circle(Vec2::new(0., 10.), 3.), holes: vec![circle(Vec2::new(0., 10.), 1.)] },
            Region2 { outer: conics(Vec2::new(0., 10.), 2.5), holes: vec![] },
            Region2 { outer: lens, holes: vec![] },
            Region2 { outer: blob, holes: vec![] },
        ];
        let outside = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let hole = crate::cylinder(-Vec3::Z, Vec3::Z, 8., 22.).unwrap();
        let tube = crate::boolean(&crate::cylinder(Vec3::ZERO, Vec3::Z, 12., 20.).unwrap(), &hole, crate::BoolOp::Cut).unwrap().unwrap();
        let bore = Plane::new(Vec3::Y * 8., -Vec3::X, Vec3::Z).unwrap();
        for (body, sketch, r, inward) in [(&outside, plane(), 10., false), (&tube, bore, 8., true)] {
            let before = volume(body);
            for region in &profiles {
                let area = region.area();
                for depth in [1., -1.] {
                    let rr: f64 = if inward { r - depth } else { r + depth };
                    let result = emboss_cylinder(body, &sketch, std::slice::from_ref(region), depth).unwrap();
                    assert_eq!(result.shell_count(), 1);
                    assert!(result.validity().is_empty());
                    let want = depth.signum() * area * (rr * rr - r * r).abs() / (2. * r);
                    let change = volume(&result) - before;
                    assert!((change - want).abs() < 1e-6 * area, "{area} at {depth}: {change} vs {want}");
                    let rec = record(&result).unwrap();
                    let fit = rec.tolerance * 0.25;
                    for e in rec.edges.iter().filter(|e| matches!(e.curve, Curve::Spline { .. })) {
                        let c = CurveEval::new(&e.curve).unwrap();
                        let off = (0..=200)
                            .map(|i| c.point(i as f64 / 200.).unwrap())
                            .map(|p| ((p.x.hypot(p.y) - r).abs()).min((p.x.hypot(p.y) - rr).abs()))
                            .fold(0.0, f64::max);
                        assert!(off <= fit, "an edge {off} off its cylinder");
                    }
                }
            }
        }
        // Inference finds the wall for a circle, and anchors on the profile.
        let (_, at) = infer_cylinder_emboss(&outside, &plane(), &profiles[..1], 1.).unwrap().unwrap();
        assert!((at.x.hypot(at.y) - 10.).abs() < 1e-9);
        // A curve still may not cross the endcap, even where its ends do not.
        let over = Region2 { outer: circle(Vec2::new(0., 18.5), 2.), holes: vec![] };
        let e = emboss_cylinder(&outside, &plane(), &[over], 1.).unwrap_err();
        assert!(e.to_string().contains("seam or endcap"), "{e}");
    }

    /// Two curved profiles whose chords cross without the curves meeting are wrapped; two
    /// circles that overlap are refused.
    #[test]
    fn wrapped_curves_are_checked_as_curves_not_chords() {
        let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
        let apart = [
            Region2 { outer: Loop2::circle(Vec2::new(-2., 8.), 1.5), holes: vec![] },
            Region2 { outer: Loop2::circle(Vec2::new(2., 8.), 1.5), holes: vec![] },
        ];
        assert!(emboss_cylinder(&body, &plane(), &apart, 1.).is_ok());
        let overlapping = [
            Region2 { outer: Loop2::circle(Vec2::new(-1., 8.), 1.5), holes: vec![] },
            Region2 { outer: Loop2::circle(Vec2::new(1., 8.), 1.5), holes: vec![] },
        ];
        assert!(emboss_cylinder(&body, &plane(), &overlapping, 1.).is_err());
    }

    /// A fitted wrap survives the boolean after it, on small and large walls, shallow and deep: a
    /// box cut through half of a wrapped circle leaves half the body and half the wrap, and a pin
    /// joined across the circle leaves one valid shell.
    #[test]
    fn a_fitted_wrap_takes_a_boolean_after_it() {
        let circle = Region2 { outer: Loop2::circle(Vec2::new(0., 10.), 2.5), holes: vec![] };
        for r in [4., 10., 100.] {
            let body = crate::cylinder(Vec3::ZERO, Vec3::Z, r, 20.).unwrap();
            let sketch = Plane::new(Vec3::Y * r, -Vec3::X, Vec3::Z).unwrap();
            for depth in [0.2, 2., -0.2, -2.] {
                let wrapped = emboss_cylinder(&body, &sketch, std::slice::from_ref(&circle), depth).unwrap();
                let v = volume(&wrapped);
                let top = crate::box_solid(Vec3::new(-2. * r, -2. * r, 10.), Vec3::new(2. * r, 2. * r, 30.)).unwrap();
                let cut = crate::boolean(&wrapped, &top, crate::BoolOp::Cut).unwrap().unwrap();
                assert_eq!(cut.shell_count(), 1);
                assert!(cut.validity().is_empty());
                let want = 0.5 * v;
                assert!((volume(&cut) - want).abs() < 1e-6 * v, "{r} {depth}: {} vs {want}", volume(&cut));
                let pin = crate::cylinder(Vec3::new(0., 0., 10.), Vec3::Y, 1., r + 4.).unwrap();
                let joined = crate::boolean(&wrapped, &pin, crate::BoolOp::Union).unwrap().unwrap();
                assert_eq!(joined.shell_count(), 1, "{r} {depth}");
                assert!(joined.validity().is_empty(), "{r} {depth}");
            }
        }
    }
}
