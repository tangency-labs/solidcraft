//! Replace a boundary by a target surface, extending the source's neighbours before
//! trimming when necessary. Accept only a single replacement cap: every unselected
//! input face must survive and the old cap must disappear. A split of unrelated
//! geometry or an ambiguous/multiple intersection is an error, never a guessed cut.

use std::collections::BTreeSet;

use truck_modeling::{self as mt, builder};

use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};
use crate::{Body, BoolOp, KernelError, Result, SplitTool};
use solidcraft_geom::{Plane, Vec3};

fn fail(s: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("Replace Face: {}", s.into()))
}

/// The plane a target names, when it is a plane or a flat face.
fn target_plane(tool: &SplitTool) -> Option<Plane> {
    match tool {
        SplitTool::Plane(p) => Some(*p),
        SplitTool::Face { body, face } => match crate::face_surface(body, *face) {
            Some(crate::FaceSurface::Plane { normal, point }) => Plane::from_normal(point, normal.vec()),
            _ => None,
        },
        SplitTool::Curves { .. } => None,
    }
}

/// Move one flat face to a parallel plane too far for the offset path (a face beside it would
/// turn inside out): sweep the face there and join or cut, so the boolean trims the neighbours
/// — a hole in a face reaching its edge opens as a notch, which the offset path refuses.
fn extend_to_plane(body: &Body, chosen: &BTreeSet<usize>, plane: &Plane) -> Result<Option<Body>> {
    let only: Vec<usize> = chosen.iter().copied().collect();
    let [index] = only.as_slice() else { return Ok(None) };
    let Some(crate::FaceSurface::Plane { normal, point }) = crate::face_surface(body, *index) else { return Ok(None) };
    let n = normal.vec();
    if plane.normal().dot(n).abs() < 1.0 - 1e-9 {
        return Ok(None);
    }
    let distance = (plane.origin - point).dot(n);
    if !distance.is_finite() || distance.abs() <= body.size() * 1e-9 {
        return Ok(None);
    }
    let face = body.solid.face_iter().nth(*index).cloned().ok_or_else(|| fail("the selected face is gone"))?;
    let tool = crate::guard("replace face sweep", || Body::new(builder::tsweep(&face, crate::body::v3(n * distance))))?;
    let op = if distance > 0.0 { BoolOp::Union } else { BoolOp::Cut };
    let Some(result) = crate::guard("replace face sweep boolean", || crate::boolean(body, &tool, op))? else {
        return Ok(None);
    };
    Ok(Some(keep_body_origins(result, body)))
}

/// `result`'s history over `[body, tool]` as one over `[body]`: the body's faces keep their
/// origins, faces the sweep and boolean made come from nothing (they are named geometrically).
fn keep_body_origins(result: Body, body: &Body) -> Body {
    let counts = crate::record::counts(&[body]);
    let Some(h) = result.history() else { return result.without_history() };
    let faces: Vec<FaceProv> = h
        .faces
        .iter()
        .map(|f| {
            let origins: Vec<FaceOrigin> = f
                .origins
                .iter()
                .copied()
                .filter(|o| {
                    matches!(o, FaceOrigin::Face { operand: 0, .. } | FaceOrigin::Edge { operand: 0, .. } | FaceOrigin::Offset { operand: 0, .. })
                })
                .collect();
            let change = match origins.len() {
                0 => Change::Generated,
                1 => Change::Modified,
                _ => Change::Merged,
            };
            FaceProv { origins, change }
        })
        .collect();
    let edges: Vec<EdgeProv> = h
        .edges
        .iter()
        .map(|e| EdgeProv {
            faces: e.faces.clone(),
            origins: e.origins.iter().cloned().filter(|o| matches!(o, EdgeOrigin::Edge { operand: 0, .. })).collect(),
        })
        .collect();
    result.with_checked_history(Some(History { operands: 1, faces, edges }), &counts)
}

/// When the target is a neighbour's plane, the replacement joins that face instead
/// of producing a separate split cap. Heal only connected selections and certify
/// that the target changed and every other unselected face survived.
fn merge_into_neighbour(body: &Body, chosen: &BTreeSet<usize>, tool: &SplitTool) -> Result<Option<Body>> {
    crate::guard("replace face adjacent plane", || {
        let (normal, point) = match tool {
            SplitTool::Plane(p) => (p.normal(), p.origin),
            SplitTool::Face { body, face } => match crate::face_surface(body, *face) {
                Some(crate::FaceSurface::Plane { normal, point }) => (normal.vec(), point),
                _ => return Ok(None),
            },
            SplitTool::Curves { .. } => return Ok(None),
        };
        let faces: Vec<_> = body.solid.face_iter().collect();
        let tol = body.size().max(1.0) * 1e-7;
        let targets: BTreeSet<_> = (0..faces.len())
            .filter(|i| !chosen.contains(i))
            .filter(|i| match crate::face_surface(body, *i) {
                Some(crate::FaceSurface::Plane { normal: n, point: p }) => {
                    normal.dot(n.vec()).abs() > 1.0 - 1e-9 && (p - point).dot(normal).abs() <= tol
                }
                _ => false,
            })
            .collect();
        if targets.is_empty() {
            return Ok(None);
        }
        // Every selected component must touch the target through shared edges.
        // A remote coplanar face must not turn Replace Face into arbitrary deletion.
        let mut connected = targets.clone();
        loop {
            let edges: std::collections::HashSet<_> =
                connected.iter().filter_map(|i| faces.get(*i)).flat_map(|f| f.edge_iter().map(|e| e.id())).collect();
            let next: Vec<_> = chosen
                .iter()
                .filter(|i| !connected.contains(i))
                .filter(|i| faces.get(**i).is_some_and(|f| f.edge_iter().any(|e| edges.contains(&e.id()))))
                .copied()
                .collect();
            if next.is_empty() {
                break;
            }
            connected.extend(next);
        }
        if !chosen.is_subset(&connected) {
            return Ok(None);
        }
        let result = crate::delete_faces(body, &chosen.iter().copied().collect::<Vec<_>>())?;
        let Some(history) = result.history() else { return Ok(None) };
        let mut kept = BTreeSet::new();
        let mut extended_target = false;
        for face in &history.faces {
            for origin in &face.origins {
                if let FaceOrigin::Face { operand: 0, face: index } = origin {
                    if chosen.contains(index) {
                        return Ok(None);
                    }
                    kept.insert(*index);
                    extended_target |= targets.contains(index) && face.change == Change::Modified;
                }
            }
        }
        if extended_target && kept.len() == faces.len() - chosen.len() { Ok(Some(result)) } else { Ok(None) }
    })
}

/// Replace the surface selected by `at` with `tool` (extended). History is over
/// `[body]`; the replacement surface is generated and neighbouring faces retain
/// their lineage. Targets requiring removal of another face are refused.
pub fn replace_faces(body: &Body, at: &[Vec3], tool: &SplitTool) -> Result<Body> {
    body.require_brep("Replace Face")?;
    if at.is_empty() || at.len() > 1000 || at.iter().any(|p| !p.is_finite()) {
        return Err(fail("pick 1…1000 finite face points"));
    }
    let chosen: BTreeSet<_> = crate::polyhedron::faces_at(body, at)?.into_iter().collect();
    let count = body.face_count();
    if chosen.is_empty() || chosen.len() >= count {
        return Err(fail("the selection cannot replace the whole body"));
    }
    // A target lying on a face the selection borders is met by extending that face over the
    // gap, which Delete Face's healing already does. Keep its failure: the offset fallback
    // cannot help once the neighbours themselves cannot follow.
    let mut merge_failure = None;
    match merge_into_neighbour(body, &chosen, tool) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) => {}
        Err(e) => merge_failure = Some(e.to_string()),
    }
    let counts = crate::record::counts(&[body]);
    let attempt = |grown: &Body, compose: bool| -> Result<Option<Body>> {
        let pieces = crate::split_body(grown, tool)?;
        let mut accepted = Vec::new();
        for piece in pieces {
            let piece = if compose { crate::record::after(piece, grown, &counts) } else { piece };
            let Some(history) = piece.history() else { continue };
            let mut kept = BTreeSet::new();
            let mut caps = 0;
            let mut old_cap = false;
            for face in &history.faces {
                if face.origins.is_empty() {
                    caps += 1;
                }
                for origin in &face.origins {
                    if let FaceOrigin::Face { operand: 0, face } | FaceOrigin::Offset { operand: 0, face } = origin {
                        if chosen.contains(face) {
                            old_cap = true;
                        } else {
                            kept.insert(*face);
                        }
                    }
                }
            }
            if !old_cap && caps == 1 && kept.len() == count - chosen.len() {
                accepted.push(piece);
            }
        }
        if accepted.len() > 1 {
            return Err(fail("the target makes more than one possible replacement"));
        }
        Ok(accepted.pop())
    };
    // Trimming needs no extension and also handles a curved source cap. If the
    // target passes exactly through an old cap edge the splitter can refuse that
    // coincident boundary; moving the cap beyond it gives a transverse split.
    if let Ok(Some(result)) = attempt(body, false) {
        return Ok(result);
    }
    let reach = match tool {
        SplitTool::Plane(p) => body.size() + at.iter().map(|x| x.dist(p.origin)).fold(0.0, f64::max),
        SplitTool::Face { body: target, .. } => {
            let a = body.outline_bounds();
            let b = target.outline_bounds();
            body.size() + target.size() + a.min.dist(b.min)
        }
        SplitTool::Curves { .. } => return Err(fail("the target must be a plane or face")),
    };
    if !reach.is_finite() || reach > 1e6 {
        return Err(fail("target is too far from the source"));
    }
    // A parallel target too far for the offset path: sweep the face there and let the boolean
    // trim its neighbours, which is what a large move asks for.
    let sweep = |tool: &SplitTool| -> Result<Option<Body>> {
        match target_plane(tool) {
            Some(plane) if chosen.len() == 1 => extend_to_plane(body, &chosen, &plane),
            _ => Ok(None),
        }
    };
    let growth = reach.max(body.size() * 0.1);
    let grown = match crate::offset_faces(body, at, growth) {
        Ok(grown) => grown,
        Err(e) => {
            if let Some(result) = sweep(tool)? {
                return Ok(result);
            }
            return Err(match merge_failure {
                Some(m) => fail(format!("the faces around the selection cannot close onto the target ({m})")),
                None => fail(format!("the selected faces cannot be extended to the target ({e})")),
            });
        }
    };
    match attempt(&grown, true)? {
        Some(result) => Ok(result),
        None => match sweep(tool)? {
            Some(result) => Ok(result),
            None => Err(match merge_failure {
                Some(m) => fail(format!("the faces around the selection cannot close onto the target ({m})")),
                None => fail("the target cannot replace only the selected faces while retaining their neighbours"),
            }),
        },
    }
}
