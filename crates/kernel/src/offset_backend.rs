//! Shell, offset faces and thicken through SMLib, when [`crate::backend`] is SMLib (the one
//! kernel setting, `SOLIDCRAFT_KERNEL`): the body goes to SMLib as its schema 4 record,
//! tagged as operand 0, SMLib runs the operation, and the result comes back with SMLib's own
//! history, renumbered into the body's and the result's face and edge order (as
//! `crate::smlib_boolean` does for booleans). A result SMLib's own check refuses is an
//! error, never a body.
//!
//! SMLib's tags travel through the copies its offset makes, so a shell's inner wall and a
//! thickened sheet's moved copy carry the tag of the face they were made from, and SMLib reads
//! both that face and its copy as pieces of it (Split). Which is which is a matter of where they
//! lie: the copy is the operation's distance from the face, the face itself on it. So a result
//! face farther from its tagged face than half that distance is recorded as its offset
//! ([`FaceOrigin::Offset`]), and its edges as where its faces meet. A face SMLib makes new along
//! a sheet's open edge (a thickened sheet's wall) is recorded as made in place of that edge, as
//! truck's thicken records it.

#![cfg(feature = "smlib")]

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_geom::Vec3;
use solidcraft_smlib::{Brep, CheckLevel, Error as SmError};

use crate::body::Body;
use crate::history::{Change, EdgeOrigin, FaceOrigin, History};
use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
use crate::{KernelError, Result, record};

fn failed(op: &str, what: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib {op}: {what}: {e}"))
}

/// The error for SMLib's refusal of `op`, in the words of the operation.
fn refused(op: &str, e: SmError) -> KernelError {
    match e {
        SmError::NoRoom => KernelError::Invalid(format!("{op}: the wall is too thick: it leaves no room inside the body")),
        SmError::NoMatch { point } => KernelError::Invalid(format!("{op}: no face at selection point {}", point + 1)),
        SmError::Ambiguous { point } => KernelError::Invalid(format!("{op}: selection point {} is as near to two faces", point + 1)),
        SmError::Unsupported => KernelError::Failed(format!("not supported yet: SMLib {op} for this geometry")),
        e => failed(op, "operation", e),
    }
}

/// How far a selection point may be from the face it picks: the kernel's own picking reach.
fn pick_tolerance(b: &Body) -> f64 {
    b.size() * 1e-3 + 1e-6
}

fn points(p: &[Vec3]) -> Vec<[f64; 3]> {
    p.iter().map(|v| [v.x, v.y, v.z]).collect()
}

/// Runs `op` on `b` in SMLib, tagged as operand 0, and brings the result back with SMLib's
/// history over `[b]` (none where an order cannot be matched).
fn run(name: &str, b: &Body, op: impl FnOnce(&Brep) -> std::result::Result<Brep, SmError>) -> Result<(Body, Option<History>)> {
    b.require_brep(name)?;
    let (sb, ob) = smlib_body_ordered(b)?;
    let tagged = sb.tagged(0).map_err(|e| failed(name, "tag", e))?;
    let r = op(&tagged).map_err(|e| refused(name, e))?;
    let check = r.check(CheckLevel::All).map_err(|e| failed(name, "check", e))?;
    if !check.is_valid() {
        let input = sb.check(CheckLevel::All).is_ok_and(|c| !c.is_valid());
        let what = if input { "SMLib's check refuses the result (the body already fails it)" } else { "SMLib's check refuses the result" };
        return Err(failed(name, what, check));
    }
    let (body, or) = body_from_smlib_ordered(&r)?;
    let history = match (ob, or) {
        (Some(ob), Some(or)) => r.history().map_err(|e| failed(name, "history", e))?.history(&[ob], &or).map(Some).unwrap_or_else(|e| {
            log::debug!(target: "smlib", "{name} history: {e}");
            None
        }),
        _ => None,
    };
    Ok((body, history))
}

/// [`crate::shell`] through SMLib: walls of `thickness` inside, open at the faces under the
/// points `open` (the face of the nearest triangle, as truck's shell picks them) and the faces
/// on the same surface joined to them: a round wall stored in pieces (a hole's two halves) opens
/// whole. SMLib is given one point inside each, never the pick itself, which may lie on the
/// line between two pieces.
pub(crate) fn shell(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    if !(thickness.is_finite() && thickness > 1e-6) {
        return Err(KernelError::Invalid("shell thickness must be positive".into()));
    }
    let picked = faces_under(b, open)?;
    let opened = crate::offset::with_same_surface(b, &picked);
    let tol = pick_tolerance(b);
    let pts = points(&face_points(b, &opened)?);
    let (r, h) = run("shell", b, |s| s.shell(&pts, tol, -thickness))?;
    let h = h.and_then(|h| with_offsets(&r, b, h, thickness));
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::offset_faces`] through SMLib: the faces `chosen` (indices in `b`'s face order)
/// moved `distance` along their outward normals. The moved faces are the faces themselves,
/// moved (Modified).
pub(crate) fn offset_faces(b: &Body, chosen: &[usize], distance: f64) -> Result<Body> {
    let at = face_points(b, chosen)?;
    let tol = pick_tolerance(b);
    let pts = points(&at);
    let (r, h) = run("offset faces", b, |s| s.offset_faces(&pts, tol, distance))?;
    let h = h.map(|mut h| {
        for f in &mut h.faces {
            if f.change == Change::Kept && f.origins.iter().any(|o| matches!(o, FaceOrigin::Face { face, .. } if chosen.contains(face))) {
                f.change = Change::Modified;
            }
        }
        h
    });
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::thicken`] through SMLib: the sheet `b` grown by `thickness` along its faces'
/// normals, or half each way when `symmetric`.
pub(crate) fn thicken(b: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    if !(thickness.is_finite() && thickness.abs() > 1e-9) {
        return Err(KernelError::Invalid("thickness must be non-zero".into()));
    }
    let (r, h) = run("thicken", b, |s| s.thicken(thickness, symmetric))?;
    let step = if symmetric { thickness.abs() / 2.0 } else { thickness.abs() };
    // The walls first, while the sheet's edges still carry their tags on both copies.
    let h = h.map(with_edge_walls).and_then(|h| with_offsets(&r, b, h, step));
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::draft`] through SMLib: the faces under the points `at` (flat, parallel to `pull`)
/// tilted by `angle` about their lines on `neutral`, leaning in going along `pull`; the faces
/// around them extended or trimmed to meet them. A tilted face is the face itself (Modified).
/// A face SMLib can't draft (not flat, or not parallel to the pull) is "not supported yet".
pub(crate) fn draft(b: &Body, at: &[Vec3], neutral: &solidcraft_geom::Plane, pull: Vec3, angle: f64) -> Result<Body> {
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("draft angle must be between −90° and 90°".into()));
    }
    let pull = pull.normalized().ok_or_else(|| KernelError::Invalid("pull direction".into()))?;
    let normal = neutral.normal().normalized().ok_or_else(|| KernelError::Invalid("neutral plane".into()))?;
    if normal.dot(pull).abs() < 1e-9 {
        return Err(KernelError::Failed("the neutral plane is parallel to the pull direction".into()));
    }
    let chosen = faces_under(b, at)?;
    let tol = pick_tolerance(b);
    let pts = points(&face_points(b, &chosen)?);
    let o = neutral.origin;
    let (r, h) = run("draft", b, |s| s.draft(&pts, tol, [o.x, o.y, o.z], [normal.x, normal.y, normal.z], [pull.x, pull.y, pull.z], angle))?;
    let h = h.map(|mut h| {
        for f in &mut h.faces {
            if f.change == Change::Kept && f.origins.iter().any(|o| matches!(o, FaceOrigin::Face { face, .. } if chosen.contains(face))) {
                f.change = Change::Modified;
            }
        }
        h
    });
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// The face of `b` under each point: the face of the nearest triangle, within the picking reach.
fn faces_under(b: &Body, at: &[Vec3]) -> Result<Vec<usize>> {
    let reach = pick_tolerance(b);
    let mesh = b.tessellate((b.size() * 1e-3).max(1e-3))?;
    let mut out: Vec<usize> = Vec::new();
    for p in at {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (crate::boolean_guard::point_triangle_dist(*p, x, y, z), *f as usize)))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        match near {
            Some((d, f)) if d < reach => {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
            _ => return Err(KernelError::Invalid(format!("no face at {:?}", [p.x, p.y, p.z]))),
        }
    }
    Ok(out)
}

/// One point inside each face `faces` of `b`: the centroid of its largest triangle.
fn face_points(b: &Body, faces: &[usize]) -> Result<Vec<Vec3>> {
    let mesh = b.tessellate((b.size() * 1e-3).max(1e-3))?;
    faces
        .iter()
        .map(|f| {
            mesh.triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter(|(_, t)| **t as usize == *f)
                .filter_map(|(t, _)| mesh.tri(t))
                .max_by(|x, y| area(x).total_cmp(&area(y)))
                .map(|[p, q, s]| (p + q + s) * (1.0 / 3.0))
                .ok_or_else(|| KernelError::Failed(format!("offset faces: face {f} has no triangles")))
        })
        .collect()
}

fn area([a, b, c]: &[Vec3; 3]) -> f64 {
    (*b - *a).cross(*c - *a).len()
}

/// `h` over `[input]` with each result face that lies `distance` (more than half of it) from
/// the input face it is tagged with recorded as that face's offset, the faces left on their
/// input face Split only where several still share it, and every edge of an offset face as
/// where its faces meet. `None` where a face cannot be measured.
fn with_offsets(result: &Body, input: &Body, mut h: History, distance: f64) -> Option<History> {
    let chord = (input.size().max(result.size()) * 1e-3).min(distance / 8.0).max(1e-6);
    let source = input.tessellate(chord).ok()?;
    let made = result.tessellate(chord).ok()?;
    let at = |mesh: &solidcraft_geom::Mesh, face: usize| -> Vec<[Vec3; 3]> {
        mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, t)| **t as usize == face).filter_map(|(t, _)| mesh.tri(t)).collect()
    };
    let mut offset = vec![false; h.faces.len()];
    for (i, f) in h.faces.iter_mut().enumerate() {
        let [FaceOrigin::Face { operand: 0, face }] = f.origins[..] else { continue };
        let tris = at(&made, i);
        let p = tris.iter().max_by(|x, y| area(x).total_cmp(&area(y))).map(|[a, b, c]| (*a + *b + *c) * (1.0 / 3.0))?;
        let under = at(&source, face);
        let d = under.iter().map(|[a, b, c]| crate::boolean_guard::point_triangle_dist(p, *a, *b, *c)).fold(f64::INFINITY, f64::min);
        if !d.is_finite() {
            return None;
        }
        if d > distance / 2.0 {
            *f = record::generated(vec![FaceOrigin::Offset { operand: 0, face }]);
            offset[i] = true;
        }
    }
    // Faces still on an input face: Split only where several share it.
    let mut uses: BTreeMap<FaceOrigin, usize> = BTreeMap::new();
    for f in &h.faces {
        if let [o @ FaceOrigin::Face { .. }] = f.origins[..] {
            *uses.entry(o).or_default() += 1;
        }
    }
    for f in &mut h.faces {
        if f.change == Change::Split
            && let [o @ FaceOrigin::Face { .. }] = f.origins[..]
            && uses.get(&o) == Some(&1)
        {
            f.change = Change::Modified;
        }
    }
    let faces = h.faces.clone();
    for e in &mut h.edges {
        if e.faces.iter().any(|f| offset.get(*f).copied().unwrap_or(false)) {
            e.origins = record::meeting(&e.faces, &faces);
        }
    }
    Some(h)
}

/// `h` with each face new to the operation that stands on exactly one input edge (its edges
/// tagged with that edge, a thickened sheet's wall) recorded as made in place of that edge.
fn with_edge_walls(mut h: History) -> History {
    let mut along: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); h.faces.len()];
    for e in &h.edges {
        for o in &e.origins {
            if let EdgeOrigin::Edge { operand: 0, edge } = o {
                for f in &e.faces {
                    if let Some(s) = along.get_mut(*f) {
                        s.insert(*edge);
                    }
                }
            }
        }
    }
    for (f, edges) in h.faces.iter_mut().zip(&along) {
        if f.origins.is_empty()
            && let [edge] = edges.iter().copied().collect::<Vec<_>>()[..]
        {
            *f = record::generated(vec![FaceOrigin::Edge { operand: 0, edge }]);
        }
    }
    h
}
