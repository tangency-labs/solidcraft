//! Replace a boundary by a target surface, extending the source's neighbours before
//! trimming when necessary. Accept only a single replacement cap: every unselected
//! input face must survive and the old cap must disappear. A split of unrelated
//! geometry or an ambiguous/multiple intersection is an error, never a guessed cut.

use std::collections::BTreeSet;

use crate::history::{Change, FaceOrigin};
use crate::{Body, KernelError, Result, SplitTool};
use solidcraft_geom::Vec3;

fn fail(s: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("Replace Face: {}", s.into()))
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
    let growth = reach.max(body.size() * 0.1);
    let grown = match crate::offset_faces(body, at, growth) {
        Ok(grown) => grown,
        Err(e) => {
            return Err(match merge_failure {
                Some(m) => fail(format!("the faces around the selection cannot close onto the target ({m})")),
                None => fail(format!("the selected faces cannot be extended to the target ({e})")),
            });
        }
    };
    match attempt(&grown, true)? {
        Some(result) => Ok(result),
        None => Err(match merge_failure {
            Some(m) => fail(format!("the faces around the selection cannot close onto the target ({m})")),
            None => fail("the target cannot replace only the selected faces while retaining their neighbours"),
        }),
    }
}
