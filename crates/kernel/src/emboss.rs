//! Exact radial embossing of orthogonal profiles on complete cylindrical walls, built on the
//! body's schema 4 record: the wall's face loses the profiles' footprints, and each profile
//! adds its walls (planes, along the axis or square to it) and its raised or sunk cap (the
//! wall's cylinder at the new radius).
use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::Change;
use crate::numbered::NumberedRegion;
use crate::offset::{Surf, face_edges, surf_of};
use crate::{KernelError, Result, guard};
use solidcore_record::brep::{Brep, Curve, Frame, Loop, Surface};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::brep::eval::CurveEval;
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
        let Some(Surf::Cylinder { o, a, r, convex: true }) = surf_of(&rec, face) else {
            return Err(fail("selected face is not an exterior cylinder"));
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
            for reg in regions {
                let c = reg.centroid();
                let theta = (u0 + c.x * plane.x.dot(tangent0) + c.y * plane.y.dot(tangent0)) / r;
                let height = z0 + c.x * kx + c.y * ky;
                // On a face of the wall: within its angles and heights, where its edges give them.
                let on_wall = angles.iter().zip(&heights).any(|(turn, along)| {
                    turn.is_none_or(|(alo, ahi)| [theta - tau, theta, theta + tau].iter().any(|t| (alo..=ahi).contains(t)))
                        && along.is_none_or(|(zl, zh)| (zl..=zh).contains(&height))
                });
                if !on_wall {
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
            slab
        };
        // Every region must reach the face: one that misses it would leave its letter out.
        let mut pieces = Vec::new();
        let mut missed = Vec::new();
        for (i, prism) in prisms.iter().enumerate() {
            match crate::boolean(prism, &slab, crate::BoolOp::Intersect)? {
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

/// Wrap sketch arc length around an exterior cylinder, retaining exact radial depth.
/// Profiles must contain orthogonal lines and lie inside one uninterrupted wall face.
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
            if let Some(Surf::Cylinder { o, a, r, convex: true }) = surf_of(&rec, face) {
                let w = plane.origin - o;
                let normal = plane.normal();
                let signed = w.dot(normal);
                if normal.dot(a).abs() < 1e-8 && (signed.abs() - r).abs() <= tol * 10.0 {
                    let radial = if signed < 0.0 { -normal } else { normal };
                    // Every supported uninterrupted wall uses one angle interval
                    // around this tangent normal, contained in (-pi,pi). A remote
                    // footprint entirely outside even that global interval cannot
                    // select this body, regardless of other unsupported faces.
                    if !profile_interval_overlaps(plane, regions, o, a.cross(radial), -std::f64::consts::PI * r, std::f64::consts::PI * r)? {
                        continue;
                    }
                    if let Some((lo, hi)) = wall_angles(&rec, face, o, a, radial, a.cross(radial)) {
                        if lo >= -tol / r || hi <= tol / r {
                            continue;
                        }
                        if !profile_interval_overlaps(plane, regions, o, a.cross(radial), lo * r, hi * r)? {
                            continue;
                        }
                    }
                    let relevant = match face_axial_bounds(&rec, face, o, a)? {
                        Some((lo, hi)) => profile_interval_overlaps(plane, regions, o, a, lo, hi)?,
                        None => true,
                    };
                    if relevant && signed < 0.0 {
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

/// Whether a body contains an analytic exterior cylindrical wall. This bounded
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
        Ok((0..rec.faces.len()).any(|face| matches!(surf_of(&rec, face), Some(Surf::Cylinder { convex: true, .. }))))
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
            let Surf::Cylinder { o, a, r, convex: true } = *s else { continue };
            let w = plane.origin - o;
            let radial = plane.normal();
            if radial.dot(a).abs() > 1e-8 || (w.dot(radial) - r).abs() > tol * 10.0 {
                continue;
            }
            let radial = radial.normalized().ok_or_else(|| fail("radial frame"))?;
            let tangent = a.cross(radial);
            let Some((alo, ahi)) = wall_angles(&rec, fi, o, a, radial, tangent) else {
                continue;
            };
            if alo >= -tol / r || ahi <= tol / r {
                continue;
            }
            if chosen.is_some() {
                return Err(fail("ambiguous cylindrical face"));
            }
            chosen = Some((fi, o, a, r, radial, tangent, alo, ahi));
        }
        let (fi, o, axis, r, radial, tangent, alo, ahi) =
            chosen.ok_or_else(|| fail("sketch must be tangent to one complete exterior cylindrical wall, clear of its seam"))?;
        if qualify && !profile_interval_overlaps(plane, regions, o, tangent, alo * r, ahi * r)? {
            return Ok(EmbossOutcome::NoMatch);
        }
        let mut caps = Vec::new();
        let mut inner: f64 = 0.0;
        for s in &surfaces {
            match *s {
                Surf::Plane { n, d } if n.dot(axis).abs() > 1.0 - 1e-8 => caps.push((d - n.dot(o)) / n.dot(axis)),
                Surf::Cylinder { o: co, a: ca, r: cr, convex }
                    if ca.dot(axis).abs() > 1.0 - 1e-8 && (co - o - axis * (co - o).dot(axis)).len() < tol * 10.0 =>
                {
                    if convex && (cr - r).abs() > tol * 10.0 {
                        return Err(fail("interrupted or stepped cylinder"));
                    }
                    if !convex {
                        inner = inner.max(cr);
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
        let rr = r + depth;
        if rr <= inner + tol * 10.0 {
            return Err(fail("depth reaches the axis or inner wall"));
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
        let map2 = |p: Vec2| Vec2::new(s0 + plane.dir_to_world(p).dot(tangent), z0 + plane.dir_to_world(p).dot(axis));
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
                if lp.segs.iter().any(|segment| !matches!(segment, Seg2::Line { .. })) {
                    return Err(fail("only orthogonal line profiles are supported on cylinders"));
                }
                if lp.segs.len() < 4 || !lp.is_closed(tol) {
                    return Err(fail("profile geometry budget or open loop"));
                }
                let mut segs = Vec::new();
                for s in &lp.segs {
                    let Seg2::Line { a, b } = *s else {
                        return Err(fail("only orthogonal line profiles are supported on cylinders"));
                    };
                    if !a.is_finite() || !b.is_finite() || a.len() > 1e7 || b.len() > 1e7 {
                        return Err(fail("nonfinite or excessive profile coordinates"));
                    }
                    let (a, b) = (map2(a), map2(b));
                    let d = b - a;
                    if d.len() < tol || (d.x.abs() > tol && d.y.abs() > tol) {
                        return Err(fail("profile edges must follow the cylinder axis or circumference"));
                    }
                    for p in [a, b] {
                        if p.x / r <= alo + tol / r || p.x / r >= ahi - tol / r || p.y <= lo + tol || p.y >= hi - tol {
                            return Err(fail("profile crosses a cylinder seam or endcap boundary"));
                        }
                    }
                    segs.push(Seg2::Line { a, b });
                }
                loops.push(Loop2 { segs });
            }
            let mut given = loops.into_iter();
            unoriented.push(Region2 { outer: given.next().ok_or_else(|| fail("profile"))?, holes: given.collect() });
        }
        let numbered: Vec<NumberedRegion> = crate::numbered::number_regions(&unoriented).iter().map(NumberedRegion::oriented).collect();
        let mapped: Vec<Region2> = numbered.iter().map(NumberedRegion::region).collect();
        validate_profiles(&mapped, tol)?;
        if qualify {
            let segment = mapped.first().and_then(|region| region.outer.segs.first()).ok_or_else(|| fail("empty profile"))?;
            let p = (segment.start() + segment.end()) * 0.5;
            let at = o + axis * p.y + (radial * (p.x / r).cos() + tangent * (p.x / r).sin()) * r;
            return Ok(EmbossOutcome::Anchor(fi, at));
        }
        let mut asm = Assembly::from_record(&rec)?;
        let selected = asm.faces.get_mut(fi).and_then(Option::take).ok_or_else(|| fail("face"))?;
        // The footprint's arcs lie on the wall face and are labelled on its frame, so their NURBS
        // forms run on the face's own quarter grid and their pcurves there are exact.
        let label = form::frame_of(&selected.surface);
        let wall = Wrap { o, axis, radial, tangent, r, label, tol };
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
            added.push(AFace { surface: wall.cylinder(rr), nurbs: None, natural_out: true, loops: cap, shell: selected.shell, domain: None });
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
    l.iter().rev().map(|u| AUse { edge: u.edge, forward: !u.forward, given: None }).collect()
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
    /// loop at `rr`, both running as the loop does, and one planar wall per segment between
    /// them (each loop seen from outside the body).
    fn wrapped_loop(&self, asm: &mut Assembly, lp: &Loop2, rr: f64, shell: usize) -> Result<(Vec<AUse>, Vec<AUse>, Vec<AFace>)> {
        let corners: Vec<Vec2> = lp.segs.iter().map(Seg2::start).collect();
        let base: Vec<u32> = corners.iter().map(|p| asm.vertex(self.at(*p, self.r))).collect::<Result<_>>()?;
        let top: Vec<u32> = corners.iter().map(|p| asm.vertex(self.at(*p, rr))).collect::<Result<_>>()?;
        let radial: Vec<u32> = base.iter().zip(&top).map(|(a, b)| asm.line(*a, *b)).collect::<Result<_>>()?;
        let n = corners.len();
        let (mut be, mut te, mut walls) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let j = (i + 1) % n;
            let (Some(&p), Some(&q), Some(&ba), Some(&bb), Some(&ta), Some(&tb), Some(&ra), Some(&rb)) =
                (corners.get(i), corners.get(j), base.get(i), base.get(j), top.get(i), top.get(j), radial.get(i), radial.get(j))
            else {
                return Err(fail("loop topology"));
            };
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
            walls.push(AFace { surface, nurbs: None, natural_out: true, loops: vec![lp], shell, domain: None });
            be.push(eb);
            te.push(et);
        }
        Ok((be, te, walls))
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
            let intersects = if cross.abs() > tol * tol {
                let u = (c - a).cross(cd) / cross;
                let v = (c - a).cross(ab) / cross;
                (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
            } else {
                false
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
        let circle = Region2 { outer: Loop2::circle(Vec2::new(0., 7.5), 1.), holes: vec![] };
        let mut crossing = profile();
        crossing.outer = Loop2::polygon(&[Vec2::new(-2., 5.), Vec2::new(2., 10.), Vec2::new(-2., 10.), Vec2::new(2., 5.)]);
        for (profiles, d) in [
            (vec![profile()], -2.),
            (vec![cap], 1.),
            (vec![circle], 1.),
            (vec![crossing], 1.),
            (vec![profile()], f64::NAN),
            (vec![profile(); 65], 1.),
        ] {
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
    fn volume(b: &Body) -> f64 {
        crate::measure(b).unwrap().volume
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
        // A depth the prisms cannot reach is refused, not cut short.
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 40.).unwrap();
        let side = face_where(&cyl, |s| matches!(s, Surf::Cylinder { convex: true, .. }));
        let e = emboss_project_face(&cyl, side, at, &sketch, &[square_at(5.)], 20.).unwrap_err();
        assert!(e.to_string().contains("radius"), "{e}");
    }
}
