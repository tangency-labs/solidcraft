//! The Align dialog's command: what each From and To pick stands for, and the bodies it moves.
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, construct_ref, occurrence_path_of_body, occurrence_paths, plane_value, sels};
use crate::selection::{SelectionExt as _, point_value as pt};

/// The Align dialog's command and its parameters: `solid.align` for bodies, `occurrence.align`
/// for components.
pub(crate) fn command(app: &SolveApp, d: &Dialog) -> Result<(&'static str, Value), String> {
    let Kind::Align { flip, components, axes, angle } = &d.kind else { return Err("this is not the Align dialog".into()) };
    let s = &app.session;
    let from = sels(d, 1).first().ok_or("select what to move from first")?;
    sels(d, 2).first().ok_or("select where to move it to first")?;
    // Bodies or Components left empty: what the From pick is on moves.
    let owner = match from {
        Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } if !body.is_empty() => Some(body.as_str()),
        _ => None,
    };
    let picked = sels(d, 0);
    let mut moved = Vec::new();
    let mut p = if *components {
        let paths =
            if picked.is_empty() { owner.and_then(|b| occurrence_path_of_body(s, b)).into_iter().collect() } else { occurrence_paths(s, picked) };
        if paths.is_empty() {
            let why = if picked.is_empty() { "the From pick is on no component: pick the components to move" } else { "select components first" };
            return Err(why.into());
        }
        json!({"paths": paths, "flip": flip})
    } else {
        moved = if picked.is_empty() {
            vec![owner.ok_or("the From pick is on no body: pick the bodies to move")?.to_string()]
        } else {
            picked.body_names()
        };
        let mut p = json!({"bodies": moved, "flip": flip});
        if *axes != [true; 3] {
            p["axes"] = json!(axes);
        }
        p
    };
    for (i, key) in [(1, "from"), (2, "to")] {
        // Bodies take what is under the cursor (Auto); components, which take no frames, as picked.
        let snap = d.extra.get(&format!("{key}_snap")).and_then(Value::as_str).unwrap_or(if *components { "picked" } else { "auto" });
        // A round edge is its circle, not its centre point: a point has no direction for the
        // bodies to turn by, nor for Flip to turn them over.
        // The command takes no frame on what moves as a target: a To pick there stays as picked.
        let frame = |sel: &Sel| match snap {
            _ if key == "to" && matches!(sel, Sel::Face { body, .. } | Sel::Edge { body, .. } if moved.contains(body)) => None,
            "auto" => circle_frame(s, sel, &moved).or_else(|| face_frame(s, sel, &moved)),
            "picked" | "circle" => circle_frame(s, sel, &moved),
            _ => None,
        };
        if !*components && let Some(frame) = sels(d, i).first().and_then(frame) {
            p[format!("{key}_frame")] = frame;
            continue;
        }
        match (sels(d, i).first(), snap) {
            // Onto Plane sets the bodies on a plane: a planar face or a plane, nothing else.
            (Some(sel), "plane") if key == "to" && !*components && !matches!(sel, Sel::Face { .. } | Sel::Plane { .. }) => {
                return Err("Onto Plane takes a planar face or a plane".into());
            }
            // A sketch point or circle is a frame whatever the snap: the bodies follow it
            // when it moves (#6257).
            (Some(sel @ (Sel::SketchPoint { .. } | Sel::SketchCurve { .. })), _) if !*components => {
                p[format!("{key}_frame")] = construct_ref(app, sel)?
            }
            // The joint origin the Joint dialog makes of the pick, found again by name.
            (Some(sel @ (Sel::Face { .. } | Sel::Edge { .. } | Sel::Vertex { .. })), "origin")
                if !*components && !matches!(sel, Sel::Vertex { body, .. } if body.is_empty()) =>
            {
                p[format!("{key}_frame")] = crate::dialogs_assembly::snap_of(s, sel).ok_or("this pick has no joint origin")?.param;
            }
            (Some(Sel::Face { point, .. }), "center") => p[key] = json!({"snap": "face_center", "at": pt(*point)}),
            (Some(Sel::Face { point, .. } | Sel::Edge { point, .. }), "circle") => p[key] = json!({"snap": "circle_center", "at": pt(*point)}),
            // Onto Plane: the To face as the plane it lies in, so the bodies come onto it
            // along its normal only.
            (Some(sel @ Sel::Face { .. }), "plane") if key == "to" && !*components => {
                p["to_plane"] = plane_value(s, Some(sel)).ok_or("Onto Plane takes a planar face or a plane")?
            }
            (Some(Sel::Face { point, .. }), _) => p[format!("{key}_face")] = pt(*point),
            (Some(Sel::Edge { point, .. }), _) => p[key] = json!({"snap": "edge_mid", "at": pt(*point)}),
            // The origin point (a vertex of no body at the origin) stays the origin; a snap
            // point of no body elsewhere (a sketch line's middle) is where it is.
            (Some(Sel::Vertex { body, point }), _) if body.is_empty() && [point.x, point.y, point.z].iter().all(|v| v.abs() < 1e-9) => {
                p[key] = json!("origin")
            }
            (Some(Sel::Vertex { point, .. }), _) => p[key] = pt(*point),
            // A plane stays a plane, so the bodies follow it when it moves.
            (Some(Sel::Plane { name }), _) => p[format!("{key}_plane")] = json!(name),
            _ => return Err("pick faces, edges, vertices, planes or sketch points".into()),
        }
    }
    // A target on what moves would move away with it: say so in the dialog's words.
    if p.get("to_frame").is_some()
        && let Some(Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. }) = sels(d, 2).first()
        && moved.contains(body)
    {
        return Err("the To pick is on a body that moves: pick the target on another body".into());
    }
    // Axes limit a point-to-point move; a face, plane or frame pick has no point to limit, and the
    // command would refuse with its parameter names (#13502). Say it in the dialog's words.
    if p.get("axes").is_some() && ["from_face", "to_face", "from_plane", "to_plane", "from_frame", "to_frame"].iter().any(|k| p.get(*k).is_some()) {
        return Err(
            "X, Y and Z limit a move between points: with a face, plane or frame pick, tick all three, or pick a vertex, an edge or a Center snap"
                .into(),
        );
    }
    // Frames and a turn are the bodies' (#6257); components align without them.
    if !*components && !angle.trim().is_empty() {
        p["angle"] = json!(angle.trim());
    }
    Ok((if *components { "occurrence.align" } else { "solid.align" }, p))
}

/// A round edge or cylindrical face picked for a side of bodies `moved`, as its circle (found
/// from its exact geometry): a joint origin at its centre whose z is its axis, which the bodies
/// turn to meet and Flip turns over. `None` for any other pick, or where the command takes no
/// frame: on another component than the bodies, or on one placed more than once.
pub(crate) fn circle_frame(s: &Session, sel: &Sel, moved: &[String]) -> Option<Value> {
    let (body, point) = match sel {
        Sel::Edge { body, index, point } if crate::dialogs::edge_circle(s, body, *index).is_some() => (body, point),
        Sel::Face { body, index, point } if is_cylinder(s, body, *index) => (body, point),
        _ => return None,
    };
    frame_on(s, body, moved, json!({"circle": pt(*point)}))
}

/// A flat face picked for a side, as a frame at its centre whose z is its normal: the faces meet
/// centre on centre. `None` as for [`circle_frame`].
fn face_frame(s: &Session, sel: &Sel, moved: &[String]) -> Option<Value> {
    let Sel::Face { body, index, point } = sel else { return None };
    crate::dialogs::planar_face(s, body, *index)?;
    frame_on(s, body, moved, json!({"face": pt(*point)}))
}

fn is_cylinder(s: &Session, body: &str, index: usize) -> bool {
    use solidcraft_engine::kernel::{self, FaceSurface};
    let st = crate::open_meshing::session_world(s);
    st.body(body).is_some_and(|b| {
        matches!(kernel::guard("align face surface", || Ok(kernel::face_surface(&b.body, index))), Ok(Some(FaceSurface::Cylinder { .. })))
    })
}

/// `param` as a frame on `body`, with the placement it was picked on, when the command takes a
/// frame there for bodies `moved`.
fn frame_on(s: &Session, body: &str, moved: &[String], mut param: Value) -> Option<Value> {
    let st = crate::open_meshing::session_world(s);
    let component = |name: &str| st.body(name).map(|b| s.doc.model_body_component(b));
    let on = component(body)?;
    if moved.iter().any(|m| component(m) != Some(on)) || (on != 0 && s.doc.occurrences.iter().filter(|o| o.component == on).count() != 1) {
        return None;
    }
    if let (Some(path), Value::Object(m)) = (crate::dialogs_assembly::placement_of_body(s, body), &mut param)
        && let Some(last) = path.last()
    {
        m.insert("occurrence".into(), json!(last));
        m.insert("path".into(), json!(path));
    }
    Some(param)
}

#[cfg(test)]
#[path = "dialogs_align_tests.rs"]
mod tests;
