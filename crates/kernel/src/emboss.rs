//! Exact radial embossing of orthogonal profiles on complete cylindrical walls.
use crate::body::{Body, Solid, from_p3, p3};
use crate::history::Change;
use crate::numbered::NumberedRegion;
use crate::offset::{Surf, surf_of};
use crate::record::Operand;
use crate::{KernelError, Result, guard};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling::{self as mt, Transformed, builder};

fn fail(s: &str) -> KernelError {
    KernelError::Invalid(format!("cylinder emboss: {s}"))
}
fn angle(p: Vec3, o: Vec3, axis: Vec3, radial: Vec3, tangent: Vec3) -> f64 {
    let w = p - o;
    let w = w - axis * w.dot(axis);
    w.dot(tangent).atan2(w.dot(radial))
}
fn unwrap(a: f64, around: f64) -> f64 {
    around + (a - around + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

fn wall_angles(face: &mt::Face, o: Vec3, axis: Vec3, radial: Vec3, tangent: Vec3, tol: f64) -> Option<(f64, f64)> {
    if face.boundaries().len() != 1 || face.edge_iter().count() != 4 {
        return None;
    }
    for edge in face.edge_iter() {
        if let Some(Some(mid)) = crate::offset::arc_mid(&edge, tol * 10.) {
            let am = angle(mid, o, axis, radial, tangent);
            let aa = unwrap(angle(from_p3(edge.front().point()), o, axis, radial, tangent), am);
            let ab = unwrap(angle(from_p3(edge.back().point()), o, axis, radial, tangent), am);
            return Some((aa.min(ab), aa.max(ab)));
        }
    }
    None
}

/// Certify a tessellated pick and project it onto the cylinder used by emboss.
pub fn cylinder_emboss_anchor(body: &Body, face: usize, at: Vec3) -> Result<Vec3> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    guard("cylinder emboss anchor", || {
        let f = body.solid.face_iter().nth(face).ok_or_else(|| fail("selected face"))?;
        let tol = (body.size() * 1e-8).max(1e-8);
        let Some(Surf::Cylinder { o, a, r, .. }) = surf_of(f, tol) else {
            return Err(fail("selected face is not a cylinder"));
        };
        let w = at - o;
        let radial = w - a * w.dot(a);
        if (radial.len() - r).abs() > (body.size() * 1e-3).max(1e-3).min(r * 0.05) {
            return Err(fail("selected point is too far from cylinder"));
        }
        Ok(o + a * w.dot(a) + radial.normalized().ok_or_else(|| fail("radial point"))? * r)
    })
}

/// Certify a tessellated pick and project it onto the face used by emboss (supports cylinders, planes, spheres, etc.).
pub fn emboss_anchor(body: &Body, face: usize, at: Vec3) -> Result<Vec3> {
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }
    guard("emboss anchor", || {
        let f = body.solid.face_iter().nth(face).ok_or_else(|| fail("selected face"))?;
        let tol = (body.size() * 1e-8).max(1e-8);
        match surf_of(f, tol) {
            Some(Surf::Cylinder { o, a, r, .. }) => {
                let w = at - o;
                let radial = w - a * w.dot(a);
                if (radial.len() - r).abs() > (body.size() * 1e-3).max(1e-3).min(r * 0.05) {
                    return Err(fail("selected point is too far from cylinder"));
                }
                Ok(o + a * w.dot(a) + radial.normalized().ok_or_else(|| fail("radial point"))? * r)
            }
            Some(Surf::Plane { n, d }) => {
                let dist = at.dot(n) - d;
                if dist.abs() > (body.size() * 1e-3).max(1e-3) {
                    return Err(fail("selected point is too far from plane"));
                }
                Ok(at - n * dist)
            }
            Some(Surf::Sphere { c, r, .. }) => {
                let radial = at - c;
                if (radial.len() - r).abs() > (body.size() * 1e-3).max(1e-3).min(r * 0.05) {
                    return Err(fail("selected point is too far from sphere"));
                }
                Ok(c + radial.normalized().ok_or_else(|| fail("radial point"))? * r)
            }
            _ => Ok(at),
        }
    })
}

/// Project 2D sketch profile regions along the sketch plane normal onto a face of the body,
/// embossing (raised if `depth > 0`) or debossing (cut if `depth < 0`) by `depth.abs()`.
/// Supports all surface types: concave, convex, planar, spherical, etc.
pub fn emboss_project(body: &Body, at: Vec3, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    emboss_project_face(body, None, at, plane, regions, depth)
}

/// Project 2D sketch profile regions onto a face of the body with wrap mapping,
/// embossing (raised if `depth > 0`) or debossing (cut if `depth < 0`) by `depth.abs()`.
/// Supports all surface types: concave, convex, planar, spherical, etc.
pub fn emboss_project_mapped(body: &Body, at: Vec3, plane: &Plane, regions: &[Region2], depth: f64, _mapping: Option<&str>) -> Result<Body> {
    emboss_project_face(body, None, at, plane, regions, depth)
}

/// Project 2D sketch profile regions onto a face of the body with wrap mapping,
/// optionally targeting an explicit face index `target_face`,
/// embossing (raised if `depth > 0`) or debossing (cut if `depth < 0`) by `depth.abs()`.
/// Supports all surface types: concave, convex, planar, spherical, etc.
pub fn emboss_project_mapped_face(
    body: &Body,
    target_face: Option<usize>,
    at: Vec3,
    plane: &Plane,
    regions: &[Region2],
    depth: f64,
    _mapping: Option<&str>,
) -> Result<Body> {
    emboss_project_face(body, target_face, at, plane, regions, depth)
}

/// Project 2D sketch profile regions onto a face of the body with wrap mapping,
/// optionally targeting an explicit face index `target_face`,
/// embossing (raised if `depth > 0`) or debossing (cut if `depth < 0`) by `depth.abs()`.
/// Supports all surface types: concave, convex, planar, spherical, etc.
pub fn emboss_project_face(body: &Body, target_face: Option<usize>, at: Vec3, plane: &Plane, regions: &[Region2], depth: f64) -> Result<Body> {
    body.require_brep("emboss project")?;
    if !depth.is_finite() || depth.abs() < 1e-6 || depth.abs() > 1e6 || regions.is_empty() || regions.len() > 4096 {
        return Err(fail("needs finite nonzero depth and 1..4096 profiles"));
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
    if !at.is_finite() || at.len() > 1e9 {
        return Err(fail("invalid selected face point"));
    }

    guard("emboss project", || {
        let n = plane.normal();
        let tol = (body.size() * 1e-8).max(1e-8);
        let d_abs = depth.abs();

        let face_idx = match target_face {
            Some(fi) => {
                if fi >= body.solid.face_iter().count() {
                    return Err(fail("target face index out of bounds"));
                }
                fi
            }
            None => crate::surfaces::nearest_face(body, at)?,
        };
        let face_sheet = crate::surfaces::copy_face_indices(body, &[face_idx])?;
        let face_ref = body.solid.face_iter().nth(face_idx);
        let cyl_info = face_ref.and_then(|f| surf_of(f, tol)).and_then(|surf| match surf {
            Surf::Cylinder { o, a, r, .. } => Some((o, a, r)),
            _ => None,
        });

        // Build thickened concentric slab around the target face
        // Slab extends inward/outward so that the raised or cut feature is carved with the exact curved face
        let overlap = (d_abs * 0.2).clamp(0.1, 2.0);
        let slab = if depth > 0.0 {
            // Emboss: slab goes from slightly inside the face (-overlap) to +d_abs outside
            crate::surfaces::thicken_between(&face_sheet, -overlap, d_abs)?
        } else {
            // Deboss: slab goes from -d_abs inside the face to +overlap outside
            crate::surfaces::thicken_between(&face_sheet, -d_abs, overlap)?
        };

        if let Some((o, a, r)) = cyl_info {
            // Find face radial vector at pick point `at`
            let w = at - o;
            let w_radial = w - a * w.dot(a);
            let radial_at = w_radial.normalized().unwrap_or(w);

            // Determine if the cylinder axis aligns more with sketch Y or sketch X
            // Usually, sketch Y is along cylinder axis (text wraps around circumference)
            let ky = plane.y.dot(a);
            let kx = plane.x.dot(a);
            let text_along_axis = kx.abs() > ky.abs();

            let (v_perp, x_dir_sketch, sign_y, sign_x) = if !text_along_axis {
                let v = plane.x.cross(a);
                let v_norm = v.normalized().unwrap_or(radial_at);
                let sy = if ky < 0.0 { -1.0 } else { 1.0 };
                (v_norm, plane.x, sy, 1.0)
            } else {
                let v = plane.y.cross(a);
                let v_norm = v.normalized().unwrap_or(radial_at);
                let sx = if kx < 0.0 { -1.0 } else { 1.0 };
                (v_norm, plane.y, 1.0, sx)
            };

            // Choose the radial direction facing the selected face (matching radial_at)
            let radial_0 = if v_perp.dot(radial_at) >= 0.0 { v_perp } else { -v_perp };

            // Tangent along circumference at radial_0, pointing in the same direction as x_dir_sketch
            let t_cand = a.cross(radial_0);
            let tangent_0 = if t_cand.dot(x_dir_sketch) >= 0.0 {
                t_cand.normalized().unwrap_or(x_dir_sketch)
            } else {
                (-t_cand).normalized().unwrap_or(x_dir_sketch)
            };

            // Sketch origin offset relative to cylinder axis
            let u_orig = (plane.origin - o).dot(tangent_0);
            let z_orig = (plane.origin - o).dot(a);

            let mut profile_solids = Vec::new();
            for reg in regions {
                let c_2d = reg.centroid();

                // Compute centroid displacement relative to cylinder center
                let ds = u_orig + c_2d.x * plane.x.dot(tangent_0) + c_2d.y * plane.y.dot(tangent_0);
                let theta = ds / r;

                let cos_t = theta.cos();
                let sin_t = theta.sin();
                let radial_i = (radial_0 * cos_t + tangent_0 * sin_t).normalized().unwrap_or(radial_0);
                let tangent_i = (-radial_0 * sin_t + tangent_0 * cos_t).normalized().unwrap_or(tangent_0);

                // Axial position of the centroid
                let zh = z_orig + c_2d.x * plane.x.dot(a) + c_2d.y * plane.y.dot(a);
                let p_i = o + a * zh + radial_i * r;

                // Local frame on the tangent plane:
                // local_x points along the reading direction
                // local_y points along the upright direction
                let local_x = if !text_along_axis { tangent_i } else { a * sign_x };
                let local_y = if !text_along_axis { a * sign_y } else { tangent_i };

                let Some(local_plane) = Plane::new(p_i, local_x, local_y) else { continue };

                // Map region relative to centroid
                let mapped_reg = reg.map_points(&|p| p - c_2d);

                // Extrude symmetrically across tangent plane so it fully spans the slab
                let pad = (d_abs + overlap) * 2.5 + 2.0;
                let local_prisms = crate::extrude(&local_plane, &[mapped_reg], -pad, pad)?;
                for p in local_prisms {
                    if let Some(carved) = crate::boolean(&p, &slab, crate::BoolOp::Intersect)? {
                        profile_solids.push(carved);
                    }
                }
            }

            if profile_solids.is_empty() {
                return Err(KernelError::Invalid("profile does not intersect target cylinder face".into()));
            }
            apply_emboss_pieces(body, profile_solids, depth)
        } else {
            // General surface / planar fallback: extrude parallel to sketch normal and intersect with the slab
            let (lo, hi) = if let Ok(m) = slab.tessellate(slab.size() * 1e-2) {
                let mut min_h = f64::INFINITY;
                let mut max_h = f64::NEG_INFINITY;
                for v in &m.positions {
                    let h = (*v - plane.origin).dot(n);
                    if h < min_h {
                        min_h = h;
                    }
                    if h > max_h {
                        max_h = h;
                    }
                }
                let pad = (d_abs * 2.0).max(5.0);
                (min_h - pad, max_h + pad)
            } else {
                let h_at = (at - plane.origin).dot(n);
                let pad = (d_abs * 4.0).max(20.0);
                (h_at - pad, h_at + pad)
            };
            let prisms = crate::extrude(plane, regions, lo.min(hi), lo.max(hi))?;
            if prisms.is_empty() {
                return Err(KernelError::Invalid("profile extrusion produced no solids".into()));
            }

            let mut pieces = Vec::new();
            for prism in &prisms {
                if let Some(carved) = crate::boolean(prism, &slab, crate::BoolOp::Intersect)? {
                    pieces.push(carved);
                }
            }
            apply_emboss_pieces(body, pieces, depth)
        }
    })
}

/// Apply embossed or debossed profile solid pieces onto the body.
/// Tries a single multi-shell boolean first, falling back to sequential piece-by-piece booleans,
/// and returns clear human-readable error messages with developer diagnostics if unresolved.
fn apply_emboss_pieces(body: &Body, pieces: Vec<Body>, depth: f64) -> Result<Body> {
    if pieces.is_empty() {
        return Err(KernelError::Invalid("profile does not intersect target face".into()));
    }
    let is_emboss = depth > 0.0;
    let op = if is_emboss { crate::BoolOp::Union } else { crate::BoolOp::Cut };
    let piece_count = pieces.len();

    // 1. Fast path: try combining all pieces into one multi-shell tool.
    // When pieces are disjoint, a single boolean keeps the target faces clean
    // and avoids repeated intermediate face fragmentation.
    if let Ok(Some(combined)) = crate::ops::join_pieces(pieces.clone())
        && let Ok(Some(res)) = crate::boolean(body, &combined, op)
    {
        return Ok(res);
    }

    // 2. Sequential fallback: apply each piece individually
    let mut current = body.clone();
    let mut succeeded = 0;
    let mut first_err = None;

    for piece in pieces {
        match crate::boolean(&current, &piece, op) {
            Ok(Some(next)) => {
                current = next;
                succeeded += 1;
            }
            Ok(None) => {}
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }

    if succeeded > 0 {
        Ok(current)
    } else if let Some(err) = first_err {
        let op_name = if is_emboss { "emboss" } else { "deboss" };
        let raw = err.to_string();
        let technical_summary = if raw.contains("faces that touch, or cross at a shallow angle") {
            "faces touch or meet at a shallow angle on curved geometry"
        } else if raw.contains("outside the tool's region") || raw.contains("could not be computed reliably") {
            "coincident or flush faces caused boundary ambiguity"
        } else if raw.contains("no result") {
            "surface-surface intersection produced no valid boundary"
        } else {
            "boolean solver could not resolve boundary intersections"
        };
        Err(KernelError::Failed(format!(
            "Emboss failed: could not {op_name} profile onto target face ({technical_summary}). \
            Ensure profiles lie within the target face boundaries and do not graze cut edges. \
            [Details: {piece_count} profile pieces; {raw}]"
        )))
    } else {
        Err(KernelError::Failed("Emboss failed: no profile pieces could be applied to target face.".into()))
    }
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
        let tol = (body.size() * 1e-8).max(1e-8);
        let mut tangent = false;
        for (index, face) in body.solid.face_iter().enumerate() {
            if index >= 50_000 {
                return Err(fail("target search exceeds face work limit"));
            }
            if let Some(Surf::Cylinder { o, a, r, convex: true }) = surf_of(face, tol) {
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
                    if let Some((lo, hi)) = wall_angles(face, o, a, radial, a.cross(radial), tol) {
                        if lo >= -tol / r || hi <= tol / r {
                            continue;
                        }
                        if !profile_interval_overlaps(plane, regions, o, a.cross(radial), lo * r, hi * r)? {
                            continue;
                        }
                    }
                    let relevant = match face_axial_bounds(face, o, a)? {
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
        let tol = (body.size() * 1e-8).max(1e-8);
        for (index, face) in body.solid.face_iter().enumerate() {
            if index >= 50_000 {
                return Err(fail("target search exceeds face work limit"));
            }
            if matches!(surf_of(face, tol), Some(Surf::Cylinder { convex: true, .. })) {
                return Ok(true);
            }
        }
        Ok(false)
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
        if body.solid.boundaries().len() != 1 || body.solid.face_iter().take(17).count() > 16 || body.solid.edge_iter().take(257).count() > 256 {
            return Err(fail("requires a complete cylinder or coaxial sleeve"));
        }
        let faces: Vec<_> = body.solid.face_iter().cloned().collect();
        let tol = (body.size() * 1e-8).max(1e-8);
        let surfaces: Vec<_> =
            faces.iter().map(|f| surf_of(f, tol).ok_or_else(|| fail("requires only cylindrical walls and planar endcaps"))).collect::<Result<_>>()?;
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
            let f = faces.get(fi).ok_or_else(|| fail("face"))?;
            if f.boundaries().len() != 1 || f.edge_iter().count() != 4 {
                continue;
            }
            let Some((alo, ahi)) = wall_angles(f, o, a, radial, tangent, tol) else {
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
            let h = (at - o).dot(axis);
            let v = at - o - axis * h;
            let theta = angle(at, o, axis, radial, tangent);
            if (v.len() - r).abs() > tol * 10.0 || theta <= alo + tol / r || theta >= ahi - tol / r || h <= lo + tol || h >= hi - tol {
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
        let at = |p: Vec2, radius: f64| o + axis * p.y + (radial * (p.x / r).cos() + tangent * (p.x / r).sin()) * radius;
        let selected = faces.get(fi).ok_or_else(|| fail("face"))?;
        let mut base_patches = vec![selected.absolute_boundaries().clone()];
        let mut hole_patches = Vec::new();
        let mut footprints = Vec::new();
        let mut added = Vec::new();
        let k = rr / r;
        let coord = |v: Vec3| [v.x, v.y, v.z];
        let av = coord(axis);
        let ov = coord(o);
        let mut m = [[0.0; 4]; 4];
        for (j, col) in m.iter_mut().take(3).enumerate() {
            for (i, slot) in col.iter_mut().take(3).enumerate() {
                *slot = if i == j { k } else { 0.0 } + (1.0 - k) * av.get(i).copied().unwrap_or(0.0) * av.get(j).copied().unwrap_or(0.0);
            }
        }
        for (i, oi) in ov.iter().enumerate() {
            let translation =
                *oi - (0..3).map(|j| m.get(j).and_then(|row| row.get(i)).copied().unwrap_or(0.0) * ov.get(j).copied().unwrap_or(0.0)).sum::<f64>();
            if let Some(slot) = m.get_mut(3).and_then(|row| row.get_mut(i)) {
                *slot = translation;
            }
        }
        if let Some(slot) = m.get_mut(3).and_then(|row| row.get_mut(3)) {
            *slot = 1.0;
        }
        let [c0, c1, c2, c3] = m;
        let [a, b, c, d] = c0;
        let [e, f, g, h] = c1;
        let [i, j, l, n] = c2;
        let [p, q, s, t] = c3;
        let mat = mt::Matrix4::new(a, b, c, d, e, f, g, h, i, j, l, n, p, q, s, t);
        let raised = selected.oriented_surface().transformed(mat);
        // History of the faces added (walls from their profile segments, the raised or sunk
        // region the profile's end cap), in the order added.
        let mut added_history = Vec::new();
        for reg in &numbered {
            let mut cap = Vec::new();
            for (li, numbered_loop) in reg.loops().enumerate() {
                let lp = &numbered_loop.lp;
                let (base, top, walls) = wrapped_loop(lp, r, rr, &at)?;
                // One wall per segment, in order.
                added_history.extend((0..walls.len()).map(|j| crate::record::profile(numbered_loop.numbers(j))));
                if li == 0 {
                    footprints.push((lp, base.inverse()));
                } else {
                    let index = base_patches.len();
                    base_patches.push(vec![base.inverse()]);
                    hole_patches.push((lp, index));
                }
                cap.push(top);
                added.extend(walls);
            }
            added.push(mt::Face::try_new(cap, raised.clone()).map_err(|e| fail(&e.to_string()))?);
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
        let mut all: Vec<_> = faces.iter().enumerate().filter(|(i, _)| *i != fi).map(|(_, f)| f.clone()).collect();
        // History (one operand, the body; profile numbering over `regions`), face by face as
        // gathered: the other faces kept, the wall's pieces left around the footprints (split
        // when the profile's holes leave more than one), then the faces added.
        let pieces = if base_patches.len() > 1 { Change::Split } else { Change::Modified };
        let mut history: Vec<_> = crate::record::kept_except(faces.len(), fi).collect();
        history.extend(std::iter::repeat_n(crate::record::from_face(0, fi, pieces), base_patches.len()));
        history.extend(added_history);
        for patch in base_patches {
            all.push(mt::Face::try_new(patch, selected.oriented_surface()).map_err(|e| fail(&e.to_string()))?);
        }
        all.extend(added);
        let solid = Solid::try_new(vec![all.into()]).map_err(|e| fail(&e.to_string()))?;
        let result = Body::new(solid)?;
        if !result.validity().is_empty() {
            return Err(fail("rebuilt cylinder is not a closed manifold"));
        }
        Ok(EmbossOutcome::Body(crate::record::attach(result, &[Operand::new(body)], Some(history), Default::default())?))
    })
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
                Seg2::Bezier { n, p } if (1..=solidcraft_geom::MAX_BEZIER_DEGREE as u8).contains(&n) => {
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

fn face_axial_bounds(face: &mt::Face, o: Vec3, axis: Vec3) -> Result<Option<(f64, f64)>> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut count = 0usize;
    let mut add = |point: Vec3| -> Result<()> {
        count = count.saturating_add(1);
        if count > 100_000 || !point.is_finite() {
            return Err(fail("target boundary work limit or nonfinite geometry"));
        }
        let z = (point - o).dot(axis);
        lo = lo.min(z);
        hi = hi.max(z);
        Ok(())
    };
    for edge in face.edge_iter() {
        match edge.curve() {
            mt::Curve::Line(_) => {
                add(from_p3(edge.front().point()))?;
                add(from_p3(edge.back().point()))?;
            }
            mt::Curve::BSplineCurve(curve) => {
                for point in curve.control_points() {
                    add(from_p3(*point))?;
                }
            }
            mt::Curve::NurbsCurve(curve) => {
                for point in curve.control_points() {
                    if !point.w.is_finite() || point.w <= 0.0 {
                        return Ok(None);
                    }
                    add(Vec3::new(point.x / point.w, point.y / point.w, point.z / point.w))?;
                }
            }
            mt::Curve::IntersectionCurve(_) => return Ok(None),
        }
    }
    Ok((lo.is_finite() && hi.is_finite()).then_some((lo, hi)))
}

fn wrapped_loop(lp: &Loop2, r: f64, rr: f64, at: &impl Fn(Vec2, f64) -> Vec3) -> Result<(mt::Wire, mt::Wire, Vec<mt::Face>)> {
    let base: Vec<_> = lp.segs.iter().map(|s| builder::vertex(p3(at(s.start(), r)))).collect();
    let top: Vec<_> = lp.segs.iter().map(|s| builder::vertex(p3(at(s.start(), rr)))).collect();
    let radial: Vec<_> = base.iter().zip(&top).map(|(a, b)| builder::line(a, b)).collect();
    let mut be = Vec::new();
    let mut te = Vec::new();
    let mut walls = Vec::new();
    for (i, s) in lp.segs.iter().enumerate() {
        let next = (i + 1) % lp.segs.len();
        let (Some(a), Some(b), Some(c), Some(d), Some(ra), Some(rb)) =
            (base.get(i), base.get(next), top.get(i), top.get(next), radial.get(i), radial.get(next))
        else {
            return Err(fail("loop topology"));
        };
        let circular = (s.end().x - s.start().x).abs() > 1e-10;
        let eb = if circular { builder::circle_arc(a, b, p3(at(s.mid(), r))) } else { builder::line(a, b) };
        let et = if circular { builder::circle_arc(c, d, p3(at(s.mid(), rr))) } else { builder::line(c, d) };
        let w: mt::Wire = vec![eb.clone(), rb.clone(), et.inverse(), ra.inverse()].into();
        walls.push(builder::try_attach_plane(&[w]).map_err(|e| fail(&e.to_string()))?);
        be.push(eb);
        te.push(et);
    }
    Ok((be.into(), te.into(), walls))
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
        let before = near.tessellate_with(1e-4, true).unwrap().measure().volume;
        let result = emboss_cylinder_at(&near, other_anchor, &shifted, &[shifted_profile], 1.).unwrap();
        assert!((result.tessellate_with(1e-4, true).unwrap().measure().volume - before - 21.).abs() < 0.02);
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
        let before = body.tessellate_with(1e-4, true).unwrap().measure().volume;
        for depth in [1., -1.] {
            for regions in [vec![ring.clone(), island.clone()], vec![island.clone(), ring.clone()]] {
                let result = emboss_cylinder(&body, &plane(), &regions, depth).unwrap();
                assert_eq!(result.shell_count(), 1);
                assert!(result.validity().is_empty());
                let after = result.tessellate_with(1e-4, true).unwrap().measure().volume;
                let expected = 16. * (depth + depth * depth / 20.);
                assert!((after - before - expected).abs() < 0.02, "{} vs {expected}", after - before);
                let mesh = result.tessellate_with(0.001, true).unwrap();
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
            let before = body.tessellate_with(1e-4, true).unwrap().measure().volume;
            let after = result.tessellate_with(1e-4, true).unwrap().measure().volume;
            let expected = if depth > 0. { 21. } else { -19. };
            assert!((after - before - expected).abs() < 0.02, "{} vs {expected}", after - before);
            let mesh = result.tessellate_with(0.001, true).unwrap();
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
                let before = body.tessellate_with(1e-4, true).unwrap().measure().volume;
                let after = result.tessellate_with(1e-4, true).unwrap().measure().volume;
                let want = profile.area().abs() * (depth + depth * depth / 20.0);
                assert!((after - before - want).abs() < 0.02, "{} vs {want}", after - before);
                let point = |s: f64, z: f64, r: f64| origin + axis * z + (radial * (s / 10.0).cos() + tangent * (s / 10.0).sin()) * r;
                let mesh = result.tessellate_with(0.001, true).unwrap();
                for (s, z) in [(-1.5, 7.5), (1.5, 7.5), (0.5, 6.5)] {
                    let material = profile.contains(Vec2::new(s, z));
                    assert_eq!(mesh.contains(point(s, z, if depth > 0. { 10.5 } else { 9.5 })), if depth > 0. { material } else { !material });
                }
                let imported = crate::step_import(&crate::step_export(&[&result], "wrapped cylinder").unwrap()).unwrap();
                assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                assert_eq!(imported.bodies.len(), 1);
                let round = imported.bodies.first().unwrap();
                assert!(round.body.validity().is_empty());
                let roundmesh = round.body.tessellate_with(0.001, true).unwrap();
                assert_eq!(roundmesh.contains(point(-1.5, 7.5, if depth > 0. { 10.5 } else { 9.5 })), depth > 0.);
                let cut = crate::boolean(
                    &round.body,
                    &crate::box_solid(origin + Vec3::new(5., -20., -20.), origin + Vec3::new(10., 20., 20.)).unwrap(),
                    crate::BoolOp::Cut,
                )
                .unwrap()
                .unwrap();
                assert!(!cut.tessellate(0.01).unwrap().contains(point(-1.5, 7.5, 9.0)));
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
        assert!(emboss_cylinder_at(&sleeve, -Vec3::Y * 10. + Vec3::Z * 7.5, &plane(), &[profile()], 1.).is_err());
        assert_eq!(crate::step_export(&[&sleeve], "unchanged").unwrap(), before);
    }

    #[test]
    fn concave_cylinder_emboss_and_deboss_project() {
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(40., 40., 40.)).unwrap();
        let cut_cyl = crate::cylinder(Vec3::new(20., 0., 40.), Vec3::Y, 15., 40.).unwrap();
        let concave_part = crate::boolean(&block, &cut_cyl, crate::BoolOp::Cut).unwrap().unwrap();
        let at = Vec3::new(20., 20., 25.);

        let sketch_plane = Plane::new(Vec3::new(20., 20., 45.), Vec3::X, Vec3::Y).unwrap();
        let profile =
            Region2 { outer: Loop2::polygon(&[Vec2::new(-3., -3.), Vec2::new(3., -3.), Vec2::new(3., 3.), Vec2::new(-3., 3.)]), holes: vec![] };

        // Emboss (raised feature by 1 mm)
        let embossed = emboss_project(&concave_part, at, &sketch_plane, std::slice::from_ref(&profile), 1.0).unwrap();
        assert_eq!(embossed.shell_count(), 1);
        let vol_before = crate::measure(&concave_part).unwrap().volume;
        let vol_embossed = crate::measure(&embossed).unwrap().volume;
        assert!(vol_embossed > vol_before, "emboss should add volume: {vol_embossed} vs {vol_before}");

        // Deboss (cut feature by 1 mm)
        let debossed = emboss_project(&concave_part, at, &sketch_plane, std::slice::from_ref(&profile), -1.0).unwrap();
        assert_eq!(debossed.shell_count(), 1);
        let vol_debossed = crate::measure(&debossed).unwrap().volume;
        assert!(vol_debossed < vol_before, "deboss should remove volume: {vol_debossed} vs {vol_before}");
    }

    #[test]
    fn planar_and_convex_emboss_project() {
        // Planar face
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(40., 40., 20.)).unwrap();
        let at_planar = Vec3::new(20., 20., 20.);
        let sketch_plane = Plane::new(Vec3::new(20., 20., 30.), Vec3::X, Vec3::Y).unwrap();
        let profile =
            Region2 { outer: Loop2::polygon(&[Vec2::new(-5., -5.), Vec2::new(5., -5.), Vec2::new(5., 5.), Vec2::new(-5., 5.)]), holes: vec![] };
        let embossed_plane = emboss_project(&block, at_planar, &sketch_plane, std::slice::from_ref(&profile), 2.0).unwrap();
        let vol_block = crate::measure(&block).unwrap().volume;
        let vol_emb_plane = crate::measure(&embossed_plane).unwrap().volume;
        assert!((vol_emb_plane - (vol_block + 10.0 * 10.0 * 2.0)).abs() < 1e-4);

        let debossed_plane = emboss_project(&block, at_planar, &sketch_plane, std::slice::from_ref(&profile), -2.0).unwrap();
        let vol_deb_plane = crate::measure(&debossed_plane).unwrap().volume;
        assert!((vol_deb_plane - (vol_block - 10.0 * 10.0 * 2.0)).abs() < 1e-4);

        // Convex cylinder
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 20., 40.).unwrap();
        let at_cyl = Vec3::new(20., 0., 20.);
        let sketch_plane_cyl = Plane::new(Vec3::new(25., 0., 20.), -Vec3::Y, Vec3::Z).unwrap();
        let embossed_cyl = emboss_project(&cyl, at_cyl, &sketch_plane_cyl, std::slice::from_ref(&profile), 1.0).unwrap();
        let vol_cyl = crate::measure(&cyl).unwrap().volume;
        assert!(crate::measure(&embossed_cyl).unwrap().volume > vol_cyl);

        let debossed_cyl = emboss_project(&cyl, at_cyl, &sketch_plane_cyl, std::slice::from_ref(&profile), -1.0).unwrap();
        assert!(crate::measure(&debossed_cyl).unwrap().volume < vol_cyl);
    }
}
