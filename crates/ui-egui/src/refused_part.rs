//! A part the modelling kernel refuses: an operation on it fails, and the kernel (`Body::smlib_refusal`)
//! says which face. Said in words where the operation fails (the feature dialog) and where the
//! part is listed (the browser), with what to do: repair the part, or look at the face.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::kernel::SurfaceKind;
use solidcraft_engine::doc::{FeatureKind, ModelState, PlaneRef};

use crate::SolveApp;

/// The command that repairs a part (planned).
pub(crate) const REPAIR: &str = "body.repair";

/// A part an operation uses that the kernel refuses.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RefusedPart {
    pub body: String,
    /// The face the kernel refuses (in the body's face order), when it found one.
    pub face: Option<usize>,
    pub surface: Option<SurfaceKind>,
}

/// The first of `bodies` (names) the kernel refused when it was last sent there, in `st`.
pub(crate) fn refused_among<'a>(st: &ModelState, bodies: impl IntoIterator<Item = &'a str>) -> Option<RefusedPart> {
    bodies.into_iter().find_map(|name| {
        let r = st.body(name)?.body.smlib_refused()?;
        Some(RefusedPart { body: name.to_string(), face: r.face, surface: r.surface })
    })
}

/// The bodies a dialog's commands use: the ones their parameters name (targets, participants,
/// a body), the ones its picks are on, and the body its sketch lies on.
pub(crate) fn operands(app: &SolveApp, commands: &[(String, Value)], picks: &[Sel]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    for (_, p) in commands {
        for key in ["targets", "participants", "bodies"] {
            for name in p.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                add(name);
            }
        }
        for key in ["body", "target"] {
            if let Some(name) = p.get(key).and_then(Value::as_str) {
                add(name);
            }
        }
        let sketch = p.get("sketch").and_then(Value::as_u64).and_then(|id| app.session.doc.feature(id));
        if let Some(FeatureKind::Sketch { plane: PlaneRef::Face { anchor: Some(anchor), .. }, .. }) = sketch.map(|f| &f.kind) {
            add(&anchor.target.body);
        }
    }
    for pick in picks {
        match pick {
            Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => add(name),
            _ => {}
        }
    }
    out
}

/// What the dialog says: what could not be done to the part, then what is wrong with it.
/// `operation` is the command's (`join`, `cut`, `intersect`; anything else is a change).
pub(crate) fn message(part: &RefusedPart, operation: Option<&str>) -> String {
    let what = match operation {
        Some("join") => "refused.join",
        Some("cut") => "refused.cut",
        Some("intersect") => "refused.intersect",
        _ => "refused.change",
    };
    format!("{} {}", crate::i18n::format(what, &[("body", &part.body)]), crate::i18n::tr(why_key(part)))
}

/// What is wrong with the part, as a sentence id.
fn why_key(part: &RefusedPart) -> &'static str {
    match (part.face, part.surface) {
        (None, _) => "refused.faces_apart",
        (Some(_), Some(SurfaceKind::Plane)) => "refused.face_flat",
        (Some(_), Some(SurfaceKind::Cylinder | SurfaceKind::Cone)) => "refused.face_curved",
        (Some(_), Some(SurfaceKind::Sphere | SurfaceKind::Torus)) => "refused.face_rounded",
        (Some(_), Some(SurfaceKind::Freeform) | None) => "refused.face_freeform",
    }
}

/// Ring `body`'s refused `face` in the viewport while this is called each frame.
pub(crate) fn ring(app: &mut SolveApp, body: &str, face: usize) {
    if !app.viewport.refused_ring_next.iter().any(|(b, f)| b == body && *f == face) {
        app.viewport.refused_ring_next.push((body.to_string(), face));
    }
}

/// The faces asked for last frame are the ones ringed now; a change is drawn at once.
pub(crate) fn next_frame(app: &mut SolveApp, ctx: &egui::Context) {
    let next = std::mem::take(&mut app.viewport.refused_ring_next);
    if next != app.viewport.refused_ring {
        app.viewport.refused_ring = next;
        ctx.request_repaint();
    }
}

/// Repair the part (the command says when it cannot yet).
pub(crate) fn repair(app: &mut SolveApp, body: &str) {
    if let Err(e) = app.run(REPAIR, json!({ "body": body })) {
        app.set_status(e, true);
    }
}

/// Frame the face the kernel refuses (ringed while the message shows). The selection is left
/// as it is: in a dialog it would become the dialog's pick.
pub(crate) fn show_face(app: &mut SolveApp, part: &RefusedPart) {
    let Some(face) = part.face else { return };
    let world = crate::open_meshing::session_world(&app.session);
    let Some(b) = world.body(&part.body) else { return };
    let mesh = b.mesh();
    let corners = mesh.tri_face.iter().zip(&mesh.triangles).filter(|(f, _)| **f as usize == face).flat_map(|(_, t)| t.iter());
    let bounds: solidcraft_engine::geom::Aabb3 = corners.filter_map(|i| mesh.positions.get(*i as usize).copied()).collect();
    if !bounds.is_empty() {
        app.frame_bounds(bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog names the part and what is wrong in a CAD user's words: no kernel name, no
    /// status code.
    #[test]
    fn a_refused_part_is_named_with_what_is_wrong_with_it() {
        let _english = crate::i18n::scope("en");
        let rounded = RefusedPart { body: "VALVE_COVER_01".into(), face: Some(412), surface: Some(SurfaceKind::Torus) };
        assert_eq!(message(&rounded, Some("join")), "Couldn't join to VALVE_COVER_01. One of its rounded faces can't be used for modelling.");
        let apart = RefusedPart { body: "Bracket".into(), face: None, surface: None };
        assert_eq!(message(&apart, Some("cut")), "Couldn't cut Bracket. Its faces don't meet closely enough to be used for modelling.");
        let flat = RefusedPart { body: "Plate".into(), face: Some(0), surface: Some(SurfaceKind::Plane) };
        let text = message(&flat, None);
        assert_eq!(text, "Couldn't change Plate. One of its flat faces can't be used for modelling.");
        assert!(!text.contains("SMLib") && !text.contains("SM_ERR"), "{text}");
    }
}
