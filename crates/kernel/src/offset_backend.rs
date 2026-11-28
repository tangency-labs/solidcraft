//! Shell, offset faces, draft and thicken through SMLib: the body goes to SMLib as its schema 4
//! record, tagged as operand 0, SMLib runs the operation, and the result comes back with SMLib's own
//! history, renumbered into the body's and the result's face and edge order (as
//! `crate::smlib_boolean` does for booleans). Where the evaluation's validation mode checks
//! each step, a result SMLib's own check refuses is an error, never a body
//! (`crate::validation`).
//!
//! SMLib's tags travel through the copies its offset makes, so a shell's inner wall and a
//! thickened sheet's moved copy carry the tag of the face they were made from, and SMLib reads
//! both that face and its copy as pieces of it (Split). Which is which is a matter of where they
//! lie: the copy is the operation's distance from the face, the face itself on it. So a result
//! face farther from its tagged face than half that distance is recorded as its offset
//! ([`FaceOrigin::Offset`]), and its edges as where its faces meet. A face SMLib makes new along
//! a sheet's open edge (a thickened sheet's wall) is recorded as made in place of that edge.

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec3;
use solidcraft_smlib::history::Order;
use solidcraft_smlib::{Brep, CheckLevel, Error as SmError};

use crate::body::Body;
use crate::history::{Change, EdgeOrigin, FaceOrigin, History};
use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
use crate::{KernelError, Result, record};

fn failed(op: &str, what: &str, e: impl crate::Cause) -> KernelError {
    KernelError::smlib(format!("SMLib {op}: {what}"), e)
}

/// The error for SMLib's refusal of `op`, in the words of the operation; `wall` is its wall's
/// thickness, where it has one.
fn refused(op: &str, wall: Option<f64>, e: SmError) -> KernelError {
    match e {
        SmError::NoRoom => KernelError::Invalid(format!("{op}: the wall is too thick: it leaves no room inside the body")),
        // The walls just meet: thinner they stay apart, thicker they cross and merge.
        SmError::WallsTouch => {
            let wall = wall.map(|t| format!(" {t} mm")).unwrap_or_default();
            KernelError::Invalid(format!(
                "{op}: the{wall} walls would meet along a line (between a hole and the side, for example), which leaves no solid there; \
                 use a thinner or thicker wall, or move the feature"
            ))
        }
        // A pad's or hole's corner on the edge of its face (#18568).
        SmError::OutlineTouches { at: [x, y, z], .. } => KernelError::Invalid(format!(
            "{op}: a corner of a pad or hole touches the edge of its face at ({x:.4}, {y:.4}, {z:.4}), so the walls offset from them would meet at a point, \
             which the {op} does not build; move the feature clear of the edge"
        )),
        // solidcore's one construction: a face curves tighter than the wall is thick.
        SmError::WallFolds { largest } => {
            let largest = largest.map(|w| format!("; the largest wall that works is {} mm", crate::blend::trimmed_mm(w))).unwrap_or_default();
            KernelError::Invalid(format!("{op}: the wall is thicker than a face curves, so the face's offset would fold{largest}"))
        }
        // A face the drafted face runs into without meeting it, a hole's wall near the side
        // (#19686): the draft trims only the faces around the drafted one.
        SmError::DraftCutsThrough { at: [x, y, z], depth } => KernelError::Invalid(format!(
            "{op}: the drafted face would cut through the face at ({}, {}, {}), which reaches at least {} mm past it \
             without meeting it (a hole's or pocket's wall near the side); a draft only extends or trims the faces around \
             the drafted face: use a smaller draft angle, or move that face farther from the drafted one",
            crate::blend::trimmed_mm(x),
            crate::blend::trimmed_mm(y),
            crate::blend::trimmed_mm(z),
            crate::blend::trimmed_mm(depth)
        )),
        SmError::NoMatch { point } => KernelError::Invalid(format!("{op}: no face at selection point {}", point + 1)),
        SmError::Ambiguous { point } => KernelError::Invalid(format!("{op}: selection point {} is as near to two faces", point + 1)),
        SmError::Unsupported => KernelError::Failed(format!("not supported yet: SMLib {op} for this geometry")),
        // A face the operation cannot handle, named for what it is (a draft's face that is neither
        // a plane nor a cone or cylinder, #18541).
        SmError::Refused(solidcore_ops::Error::Unsupported(why)) => KernelError::Failed(format!("not supported yet: {op}: {why}")),
        // A drafted cone or cylinder whose angle closes it within the face, or whose neutral plane
        // lies at or past its apex (#22807): a geometric refusal, not an unsupported feature.
        SmError::Refused(solidcore_ops::Error::OffsetRefused {
            why: why @ (solidcore_ops::OffsetRefusal::DraftConeCloses | solidcore_ops::OffsetRefusal::DraftNeutralPlanePastApex),
            ..
        }) => KernelError::Invalid(format!("{op}: {why}")),
        // A tangent blend that cannot follow the moved faces, in solidcore's words (#18575).
        SmError::BlendsNotFollowed { exact: false, why } => KernelError::Failed(format!("not supported yet: SMLib {op}: {why}")),
        SmError::BlendsNotFollowed { exact: true, why } => {
            KernelError::Failed(format!("SMLib {op}: SMLib cannot build this exactly in the input's geometry: {why}"))
        }
        SmError::NoEffect => KernelError::Failed(format!("SMLib made no change: the {op} reported success but moved no face onto its new surface")),
        e => failed(op, "operation", e),
    }
}

/// How far a selection point may be from the face it picks: the kernel's own picking reach.
fn pick_tolerance(b: &Body) -> f64 {
    b.size() * 1e-3 + 1e-6
}

/// The pick reach widened by the display chord deviation: a near-rim click picks the
/// nearest exact trimmed face, including on a curved wall.
pub(crate) fn exact_pick_reach(b: &Body) -> f64 {
    2.0 * pick_tolerance(b)
}

fn points(p: &[Vec3]) -> Vec<[f64; 3]> {
    p.iter().map(|v| [v.x, v.y, v.z]).collect()
}

/// Runs `op` on `b` in SMLib, tagged as operand 0, and brings the result back with SMLib's
/// history over `[b]` (none where an order cannot be matched). `wall`: the operation's wall
/// thickness, for its refusals. SMLib's check of the result runs here when the evaluation's
/// validation mode checks each step, and is left to the feature's end when it checks each
/// feature (`crate::validation`), as for booleans, blends and sweeps (#17601).
pub(crate) fn run(
    name: &str,
    b: &Body,
    wall: Option<f64>,
    op: impl FnOnce(&Brep) -> std::result::Result<Brep, SmError>,
) -> Result<(Body, Option<History>)> {
    run_ordered(name, b, wall, |tagged, _| op(tagged))
}

/// Like [`run`], also passing the operand's SMLib-to-record [`Order`]
/// so the operation can name edges directly by SMLib index.
pub(crate) fn run_ordered(
    name: &str,
    b: &Body,
    wall: Option<f64>,
    op: impl FnOnce(&Brep, Option<&Order>) -> std::result::Result<Brep, SmError>,
) -> Result<(Body, Option<History>)> {
    b.require_brep(name)?;
    let (sb, ob) = smlib_body_ordered(b)?;
    let tagged = sb.tagged(0).map_err(|e| failed(name, "tag", e))?;
    let r = op(&tagged, ob.as_ref()).map_err(|e| refused(name, wall, e))?;
    finish_run(name, b, &sb, ob, r)
}

/// Bring a tagged operation's result back, with its checked geometry and native origins.
fn finish_run(name: &str, b: &Body, sb: &Brep, ob: Option<Order>, r: Brep) -> Result<(Body, Option<History>)> {
    use crate::validation::Step;
    let step = crate::validation::step();
    let (body, or) = body_from_smlib_ordered(&r)?;
    if step == Step::Check
        && let crate::validation::DecodedCheck::Refused(detail) = crate::validation::check_decoded(&format!("SMLib {name}"), body.record())?
    {
        let input = sb.check(CheckLevel::All).is_ok_and(|c| !c.is_valid());
        let what = if input { "SMLib's check refuses the result (the body already fails it)" } else { "SMLib's check refuses the result" };
        return Err(failed(name, what, detail));
    }
    let history = match (ob, or) {
        (Some(ob), Some(or)) => r.history().map_err(|e| failed(name, "history", e))?.history(&[ob], &or).map(Some).unwrap_or_else(|e| {
            log::debug!(target: "smlib", "{name} history: {e}");
            None
        }),
        _ => None,
    };
    let body = if step == Step::Defer { body.left_unchecked(&format!("SMLib {name}"), &[b]) } else { body };
    Ok((body, history))
}

/// [`crate::shell`] through SMLib: walls of `thickness` inside, open at the faces under the
/// points `open` (the face of the nearest triangle) and the faces
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
    let seen = std::cell::RefCell::new(None);
    let attempt = |t: f64| {
        run("shell", b, Some(t), |s| {
            s.shell(&pts, tol, -t).inspect_err(|e| {
                *seen.borrow_mut() = Some(e.clone());
            })
        })
    };
    let (r, h) = match attempt(thickness) {
        Ok(done) => done,
        Err(first) => {
            let failure = seen.borrow().clone();
            return Err(match failure {
                Some(f) if shell_may_be_thinner(&f) => thinner_shell_hint(thickness, &f, first, |t| attempt(t).is_ok()),
                _ => first,
            });
        }
    };
    let h = h.and_then(|h| with_offsets(&r, b, h, thickness));
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// A refusal a thinner wall can cure: a fold, or a result that is not a solid.
fn shell_may_be_thinner(e: &SmError) -> bool {
    matches!(e, SmError::WallFolds { .. } | SmError::NotSolid | SmError::Refused(_) | SmError::RefusedWithDiagnostic { .. } | SmError::Status(_))
}

/// The walls tried under a refused one of `thickness`: from just under the wall the kernel
/// names as the largest (when it names one) down to half of it, else half and a quarter of the wall.
/// Three tries at most: each is a whole shell, and the dialog previews every edit.
pub(crate) fn thinner_walls(thickness: f64, largest: Option<f64>) -> Vec<f64> {
    let (base, steps): (f64, &[f64]) = match largest.filter(|l| l.is_finite() && *l > 0.0 && *l < thickness) {
        Some(l) => (l, &[0.95, 0.75, 0.5]),
        None => (thickness, &[0.5, 0.25]),
    };
    // Rounded to the micrometre first, so the wall tried is the wall the message names.
    steps.iter().map(|k| (base * k * 1e6).round() / 1e6).filter(|w| *w > 1e-6 && *w < thickness).collect()
}

/// The refusal of a shell of `thickness` with a hint that has been tried: the first of the
/// thinner walls that `builds` is the one named, so the wall the message offers does work. Where
/// none builds and the kernel refused the wall for folding, a face curving tighter than the wall
/// is thick, the message says the wall is not the obstacle. Any other cause is the kernel's own
/// refusal, `first`, kept as it is: a result that is not a solid at every wall is not a property
/// of the wall or of how the faces curve, and saying it is would blame valid input.
pub(crate) fn thinner_shell_hint(thickness: f64, failure: &SmError, first: KernelError, mut builds: impl FnMut(f64) -> bool) -> KernelError {
    let largest = match failure {
        SmError::WallFolds { largest } => *largest,
        _ => None,
    };
    let walls = thinner_walls(thickness, largest);
    let t = crate::blend::trimmed_mm(thickness);
    if let Some(w) = walls.into_iter().find(|w| builds(*w)) {
        let w = crate::blend::trimmed_mm(w);
        let why = if matches!(failure, SmError::WallFolds { .. }) {
            "a face curves tighter than the wall is thick, so its offset would fold"
        } else {
            "the walls offset from this body do not close into a solid"
        };
        return KernelError::Invalid(format!("shell: a wall of {t} mm does not fit: {why}; a wall of {w} mm builds"));
    }
    match first {
        KernelError::Invalid(_) | KernelError::Failed(_) if matches!(failure, SmError::WallFolds { .. }) => KernelError::Invalid(format!(
            "shell: a wall of {t} mm does not fit, and no thinner wall builds either, so the wall is not the obstacle: \
             a face of the body curves too tightly to offset inwards, or the body has faces too small or too many to offset into a solid \
             (a curve cut as many tiny flat pieces, for example); round the tight bend or rebuild the body with smoother faces"
        )),
        other => other,
    }
}

/// [`crate::offset_faces`] through SMLib: the faces `chosen` (indices in `b`'s face order)
/// moved `distance` along their outward normals. The moved faces are the faces themselves,
/// moved (Modified).
pub(crate) fn offset_faces(b: &Body, chosen: &[usize], distance: f64) -> Result<Body> {
    let at = face_points(b, chosen)?;
    let tol = pick_tolerance(b);
    let pts = points(&at);
    let (r, h) = run("offset faces", b, None, |s| s.offset_faces(&pts, tol, distance))?;
    Ok(moved_faces(r, h, b, chosen))
}

fn moved_faces(r: Body, history: Option<History>, input: &Body, chosen: &[usize]) -> Body {
    let h = history.map(|mut h| {
        for f in &mut h.faces {
            if f.change == Change::Kept && f.origins.iter().any(|o| matches!(o, FaceOrigin::Face { face, .. } if chosen.contains(face))) {
                f.change = Change::Modified;
            }
        }
        h
    });
    r.with_checked_history(h, &[(input.face_count(), input.edge_count())])
}

/// Retrim a single support. Resolve the pick on the original body, without healing away
/// split faces, and carry that exact face into the tagged copy in its native order.
pub(crate) fn transform_face(b: &Body, at: Vec3, translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Result<Body> {
    const NAME: &str = "transform face";
    let _span = solidcraft_profile::span("kernel", "transform_face");
    b.require_brep(NAME)?;
    let tolerance = exact_pick_reach(b);
    let chosen = b.face_within(at, tolerance)?.ok_or_else(|| KernelError::Invalid(format!("no face at {at}")))?;
    let (sb, ob) = smlib_body_ordered(b)?;
    let order = ob.ok_or_else(|| KernelError::Failed("transform face: cannot match the body's face order".into()))?;
    let index = if order.faces.is_empty() {
        chosen
    } else {
        order.faces.iter().position(|&face| face == chosen).ok_or_else(|| KernelError::Failed("transform face: missing selected face".into()))?
    };
    let pick = solidcraft_smlib::PickedFace { index, at: at.into(), tolerance };
    let tagged = sb.tagged(0).map_err(|e| failed(NAME, "tag", e))?;
    let (r, changed) = tagged.transform_face(pick, translate.into(), origin.into(), axis.into(), angle).map_err(|e| refused(NAME, None, e))?;
    if !changed {
        solidcraft_smlib::trace::exported(&r, b.record());
        return Ok(b.unchanged_step());
    }
    let (r, h) = finish_run(NAME, b, &sb, Some(order), r)?;
    Ok(moved_faces(r, h, b, &[chosen]).with_split_lines(b.splits.clone()).inherit_regularization_protection(b))
}

/// [`crate::thicken`] through SMLib: the sheet `b` grown by `thickness` along its faces'
/// normals, or half each way when `symmetric`.
pub(crate) fn thicken(b: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    if !(thickness.is_finite() && thickness.abs() > 1e-9) {
        return Err(KernelError::Invalid("thickness must be non-zero".into()));
    }
    let (r, h) = run("thicken", b, Some(thickness.abs()), |s| s.thicken(thickness, symmetric))?;
    let step = if symmetric { thickness.abs() / 2.0 } else { thickness.abs() };
    // The walls first, while the sheet's edges still carry their tags on both copies.
    let h = h.map(with_edge_walls).and_then(|h| with_offsets(&r, b, h, step));
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::offset_solid`] through SMLib: the closed solid `b` grown by `distance` along its
/// faces' outward normals (negative: shrunk), the convex edges extended or (`round`) rounded with
/// `distance` as the radius. Each moved face is the face itself, moved (Modified); a round SMLib
/// makes along an edge or at a corner is new.
pub(crate) fn offset_solid(b: &Body, distance: f64, round: bool) -> Result<Body> {
    if !(distance.is_finite() && distance.abs() > 1e-9 && distance.abs() <= 1e6) {
        return Err(KernelError::Invalid("the offset distance must be non-zero and at most 1e6 mm".into()));
    }
    let (r, h) = run("offset", b, None, |s| s.offset_solid(distance, round))?;
    let h = h.map(|mut h| {
        for f in &mut h.faces {
            if f.change == Change::Kept && f.origins.iter().any(|o| matches!(o, FaceOrigin::Face { operand: 0, .. })) {
                f.change = Change::Modified;
            }
        }
        h
    });
    Ok(r.with_checked_history(h, &record::counts(&[b])))
}

/// [`crate::draft`] through SMLib: the faces under the points `at` (flat, parallel to `pull`)
/// tilted by `angle` about their lines on `neutral`, leaning in going along `pull`; the faces
/// around them extended or trimmed to meet them. A tilted face is the face itself (Modified).
/// A face SMLib can't draft (neither flat nor a cone or cylinder about the pull, or a plane not
/// parallel to it) is "not supported yet", naming which.
pub(crate) fn draft(b: &Body, at: &[Vec3], neutral: &solidcore_record::Plane, pull: Vec3, angle: f64) -> Result<Body> {
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
    let (r, h) = run("draft", b, None, |s| s.draft(&pts, tol, [o.x, o.y, o.z], [normal.x, normal.y, normal.z], [pull.x, pull.y, pull.z], angle))?;
    Ok(moved_faces(r, h, b, &chosen))
}

/// The face of `b` under each point: the face of the nearest triangle, within the picking reach.
fn faces_under(b: &Body, at: &[Vec3]) -> Result<Vec<usize>> {
    let reach = pick_tolerance(b);
    let mesh = b.mesh(&crate::MeshTolerance::chord((b.size() * 1e-3).max(1e-3)))?;
    let mut out: Vec<usize> = Vec::new();
    for p in at {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (solidcraft_geom::compare::closest_on_triangle(*p, x, y, z).dist(*p), *f as usize)))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        match near {
            Some((d, f)) if d < reach => {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
            _ => return Err(KernelError::Invalid(format!("no face at {p}"))),
        }
    }
    Ok(out)
}

/// One point inside each face `faces` of `b`: the centroid of its largest triangle.
fn face_points(b: &Body, faces: &[usize]) -> Result<Vec<Vec3>> {
    let mesh = b.mesh(&crate::MeshTolerance::chord((b.size() * 1e-3).max(1e-3)))?;
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
    let source = input.mesh(&crate::MeshTolerance::chord(chord)).ok()?;
    let made = result.mesh(&crate::MeshTolerance::chord(chord)).ok()?;
    let at = |mesh: &solidcraft_geom::Mesh, face: usize| -> Vec<[Vec3; 3]> {
        mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, t)| **t as usize == face).filter_map(|(t, _)| mesh.tri(t)).collect()
    };
    let mut offset = vec![false; h.faces.len()];
    for (i, f) in h.faces.iter_mut().enumerate() {
        let [FaceOrigin::Face { operand: 0, face }] = f.origins[..] else { continue };
        let tris = at(&made, i);
        let p = tris.iter().max_by(|x, y| area(x).total_cmp(&area(y))).map(|[a, b, c]| (*a + *b + *c) * (1.0 / 3.0))?;
        let under = at(&source, face);
        let d = under.iter().map(|[a, b, c]| solidcraft_geom::compare::closest_on_triangle(p, *a, *b, *c).dist(p)).fold(f64::INFINITY, f64::min);
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
