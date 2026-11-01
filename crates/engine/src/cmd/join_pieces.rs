//! What a feature's material does when it is joined: the test the feature windows use to choose
//! Join, New Body or Cut by themselves (#6901). It runs the Join itself, so it agrees with the
//! result OK makes, and with the warning a Join apart gets (#4434).

use serde_json::Value;

use crate::Session;

/// The most commands a feature window previews at once.
const MAX_COMMANDS: usize = 64;

/// What a feature's material makes, joined to the bodies it may join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addition {
    /// It would be a separate piece (or there is no body to join): a New Body.
    Apart,
    /// It makes one body with what it touches: a Join.
    Joins,
    /// It adds no material: it lies inside a body (a Cut, where the window offers one).
    Inside,
}

/// What `cmds` (a feature window's commands, with operation New Body or Join) make when run with
/// Join (see [`crate::Ahead::addition`]).
pub fn addition(s: &Session, cmds: &[(String, Value)], inside: bool) -> Option<Addition> {
    s.ahead().addition(cmds, inside).0
}

impl crate::Ahead {
    /// What `cmds` (a feature window's commands, with operation New Body or Join) make when run
    /// with Join, and, when they join, the Join evaluated (OK takes it rather than computing it
    /// again, [`crate::Session::prime`]). `inside`: tell material lying inside a body
    /// ([`Addition::Inside`]) from material that joins. None when they have no such operation
    /// (Combine, a Cut chosen by hand, a feature without one), the model has no body, or the Join
    /// fails.
    ///
    /// Material that touches no body can't be fused to one: that is found from the material alone
    /// and a contact test, without the Join (a boolean that takes seconds on a large body).
    pub fn addition(self, cmds: &[(String, Value)], inside: bool) -> (Option<Addition>, Option<crate::Evaluated>) {
        let s = self.0;
        let before = s.model.state();
        if cmds.is_empty() || cmds.len() > MAX_COMMANDS || before.bodies.is_empty() {
            return (None, None);
        }
        let (Some(alone), Some(joined)) = (with_operation(cmds, "new"), with_operation(cmds, "join")) else { return (None, None) };
        let Ok(tools) = s.preview(&alone) else { return (None, None) };
        let tools: Vec<&solidcraft_doc::ModelBody> = tools.after.bodies.iter().filter(|b| tools.before.body(&b.name).is_none()).collect();
        // Every body the Join could take part in (its scope is narrower): none touched, none joined.
        let named: Vec<String> = joined
            .iter()
            .flat_map(|(_, p)| {
                let p = p.get("params").filter(|_| p.get("command").is_some()).unwrap_or(p);
                crate::params::string_list_or_default(p, "targets").into_iter().chain(crate::params::string_list_or_default(p, "participants"))
            })
            .collect();
        let candidates: Vec<&solidcraft_doc::ModelBody> = before
            .bodies
            .iter()
            .filter(|b| named.is_empty() || named.iter().any(|n| n == "*" || *n == b.name))
            .filter(|b| !b.body.is_mesh())
            .collect();
        if !tools.is_empty() && !tools.iter().any(|t| candidates.iter().any(|b| t.body.meets(&b.body).unwrap_or(true))) {
            return (Some(Addition::Apart), None);
        }
        let Ok((after, _)) = s.preview_in_place(&joined, None) else { return (None, None) };
        let state = after.model.state();
        // A Join with nothing to join makes a new body.
        if state.bodies.iter().any(|b| before.body(&b.name).is_none()) || !solidcraft_doc::join_pieces_added(&before, &state).is_empty() {
            return (Some(Addition::Apart), None);
        }
        if inside && adds_nothing(&before, &state, &tools) {
            return (Some(Addition::Inside), None);
        }
        after.world_state().warm_display();
        (Some(Addition::Joins), Some(crate::Evaluated(after.model)))
    }
}

/// Whether the Join from `before` to `after` added (next to) no volume: its material lies inside
/// the bodies. Display meshes are close enough for that, against the material's own volume;
/// measuring a large body exactly takes seconds.
fn adds_nothing(before: &solidcraft_doc::ModelState, after: &solidcraft_doc::ModelState, tools: &[&solidcraft_doc::ModelBody]) -> bool {
    let (mut had, mut have) = (0.0, 0.0);
    for was in &before.bodies {
        match after.body(&was.name) {
            Some(now) if now.body.same_shape(&was.body) => {}
            Some(now) => {
                had += mesh_volume(&was.mesh());
                have += mesh_volume(&now.mesh());
            }
            None => had += mesh_volume(&was.mesh()),
        }
    }
    let material: f64 = tools.iter().map(|t| mesh_volume(&t.mesh())).sum();
    material > 0.0 && (have - had) < material * 0.01
}

/// The volume a closed mesh encloses.
fn mesh_volume(m: &solidcraft_geom::Mesh) -> f64 {
    m.triangles.iter().filter_map(|t| m.tri(t)).map(|[a, b, c]| a.dot(b.cross(c)) / 6.0).sum::<f64>().abs()
}

/// `cmds` with their New Body or Join operation set to `op`; None when none has one.
fn with_operation(cmds: &[(String, Value)], op: &str) -> Option<Vec<(String, Value)>> {
    let mut out = cmds.to_vec();
    let mut any = false;
    for (command, p) in &mut out {
        let inner = p.get("command").and_then(Value::as_str).unwrap_or(command.as_str());
        if inner == "solid.combine" {
            return None;
        }
        let fields = if p.get("command").is_some() { p.get_mut("params")? } else { p };
        if let Some(o) = fields.get_mut("operation")
            && o.as_str().is_some_and(|o| o == "new" || o == "join")
        {
            *o = Value::from(op);
            any = true;
        }
    }
    any.then_some(out)
}

#[cfg(test)]
#[path = "join_pieces_tests.rs"]
mod tests;
