//! The Align dialog's command: what each From and To pick stands for, and the bodies it moves.
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

use crate::selection::point_value as pt;

/// A round edge picked for a side of bodies `moved`, as the circle it lies on (found from its
/// exact curve): a joint origin at its centre whose z is its axis, which the bodies turn to meet
/// and Flip turns over. `None` for any other pick, or where the command takes no frame: on another
/// component than the bodies, or on one placed more than once.
pub(crate) fn circle_frame(s: &Session, sel: &Sel, moved: &[String]) -> Option<Value> {
    let Sel::Edge { body, index, point } = sel else { return None };
    crate::dialogs::edge_circle(s, body, *index)?;
    frame_on(s, body, moved, json!({"circle": pt(*point)}))
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
