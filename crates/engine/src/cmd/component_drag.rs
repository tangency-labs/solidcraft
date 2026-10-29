//! Dragging a part to put it somewhere (#2804): what the viewport's left-button drag on a free
//! part runs each frame, and what scripts and agents use to move a placement by a grabbed point.
//! A grounded part stays where it is, and so does one that joints hold to something fixed
//! (`joint.reach` moves those as far as their joints allow). A chain of joints joined to nothing
//! fixed moves as one, so its joints stay as they are.

use serde_json::{Value, json};
use solidcraft_doc::{mat_inverse, mat_mul};

use super::{move_vector, occurrence, occurrence_path, translation, valid_position};
use crate::params::bad;
use crate::{EngineError, Result, Session};

#[cfg(test)]
#[path = "component_drag_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

/// Most placements one drag moves (the dragged one and the chain of joints it is in).
const MAX_MOVED: usize = 1000;

fn refused(message: String) -> EngineError {
    EngineError::Other(message)
}

impl Session {
    /// The placements a drag on the placement `path` moves, each by its exact path: that
    /// placement first, then those a chain of joints joined to nothing fixed carries along with
    /// it. An error says why it can't be dragged (grounded, held by joints to something fixed,
    /// or held in a rigid group). Shared definitions move in all of their placements.
    pub fn drag_group(&self, path: &[u64]) -> Result<Vec<Vec<u64>>> {
        let placement = self.occurrence_placement(path)?;
        let doc = &self.doc;
        let name = |id: u64| doc.occurrences.iter().find(|o| o.id == id).map_or_else(|| id.to_string(), |o| o.name.clone());
        let dragged = placement.occurrence;
        if placement.grounded {
            return Err(refused(format!("{} is grounded", placement.name)));
        }
        if let Some(group) = self.held_by_rigid_group(dragged) {
            let group = doc.components.iter().find(|c| c.id == group).map(|c| c.name.clone()).unwrap_or_default();
            return Err(refused(format!("{} is held in {group} by a rigid group: drag {group}, or Unlock Contents first", placement.name)));
        }
        // Is `o` the placement `top` or inside its component (in every placement of it)?
        let within = |o: u64, top: u64| -> bool {
            if o == top {
                return true;
            }
            let (Some(o), Some(top)) = (doc.occurrences.iter().find(|x| x.id == o), doc.occurrences.iter().find(|x| x.id == top)) else {
                return false;
            };
            o.parent != 0 && doc.component_within(o.parent, top.component)
        };
        let joints: Vec<_> = doc.assembly.joints.iter().filter(|j| !j.suppressed).collect();
        let mut members = vec![dragged];
        // Grow the chain one joint at a time; each pass adds a member or stops.
        for _ in 0..MAX_MOVED {
            let inside = |o: u64| members.iter().any(|m| within(o, *m));
            let next = joints.iter().find_map(|j| match (inside(j.a.occurrence), inside(j.b.occurrence)) {
                (true, false) => Some((j, j.b.occurrence)),
                (false, true) => Some((j, j.a.occurrence)),
                _ => None,
            });
            let Some((joint, other)) = next else {
                break;
            };
            if other == 0 {
                return Err(refused(format!("{} holds {} to the design's origin", joint.name, placement.name)));
            }
            // Only this member's placement is edited. A grounded ancestor fixes its own
            // transform, while its free contents may still move inside that frame.
            if doc.occurrences.iter().any(|o| o.id == other && o.grounded) {
                return Err(refused(format!(
                    "{} holds {} to grounded {}: it moves only as far as its joints allow (joint.reach)",
                    joint.name,
                    placement.name,
                    name(other)
                )));
            }
            if path.contains(&other) || within(dragged, other) {
                return Err(refused(format!("{} holds {} to the assembly it is in: drag {}", joint.name, placement.name, name(other))));
            }
            members.push(other);
            if members.len() > MAX_MOVED {
                return Err(refused(format!("the chain of joints {} is in moves more than {MAX_MOVED} parts", placement.name)));
            }
        }
        // A member inside another one moves with it (moving both would move it twice).
        let tops: Vec<u64> = members.iter().copied().filter(|m| !members.iter().any(|o| o != m && within(*m, *o))).collect();
        let mut out = vec![path.to_vec()];
        for m in tops.into_iter().filter(|m| *m != dragged) {
            let parent = doc.occurrences.iter().find(|o| o.id == m).map_or(0, |o| o.parent);
            let mut chain = doc.first_path(parent);
            chain.push(m);
            out.push(chain);
        }
        Ok(out)
    }
}

/// `occurrence.drag`: move a placement by `point - at` in world coordinates (and the free chain
/// of joints it is in), capturing its position.
pub(super) fn drag(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.drag";
    let path = if p.get("occurrence").is_some() {
        let id = occurrence(s, p, cmd)?;
        occurrence_path(s, p, id, cmd)?
    } else {
        let path = p
            .get("path")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty() && a.len() <= 64)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .ok_or_else(|| bad(cmd, "give `occurrence` or `path` (1..64 occurrence ids)"))?;
        s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
        path
    };
    if p.get("at").is_none() || p.get("point").is_none() {
        return Err(bad(cmd, "`at` and `point` must both be given"));
    }
    let at = move_vector(p, "at", Default::default(), true, cmd)?;
    let point = move_vector(p, "point", Default::default(), true, cmd)?;
    let t = point - at;
    let world_move = translation(t);
    if let Some(id) = path.last() {
        super::mirror::placed_by_mirror(s, *id, cmd)?;
    }
    let group = s.drag_group(&path).map_err(|e| bad(cmd, e.to_string()))?;
    let dragged = s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
    let inverse = mat_inverse(&dragged.parent_world).ok_or_else(|| bad(cmd, "parent frame is singular"))?;
    let local_delta = mat_mul(&mat_mul(&inverse, &world_move), &dragged.parent_world);
    // Joints refer to shared occurrence definitions, and the solver places them through their
    // first path. Carry the selected placement's local change into that same frame before
    // moving its peers. Using the pointer's world delta directly would break the joint when
    // the selected instance has a different rotation from the first one.
    let parent = s.doc.occurrences.iter().find(|o| o.id == dragged.occurrence).map_or(0, |o| o.parent);
    let mut first_path = s.doc.first_path(parent);
    first_path.push(dragged.occurrence);
    let first = s.occurrence_placement(&first_path).map_err(|e| bad(cmd, e.to_string()))?;
    let first_inverse = mat_inverse(&first.parent_world).ok_or_else(|| bad(cmd, "parent frame is singular"))?;
    let world_delta = mat_mul(&mat_mul(&first.parent_world, &local_delta), &first_inverse);
    // Every new position first, from the placements as they are, then all of them at once.
    let mut moves = Vec::with_capacity(group.len());
    for placement_path in &group {
        let placement = s.occurrence_placement(placement_path).map_err(|e| bad(cmd, e.to_string()))?;
        let inverse = mat_inverse(&placement.parent_world).ok_or_else(|| bad(cmd, "parent frame is singular"))?;
        let delta =
            if placement.occurrence == dragged.occurrence { local_delta } else { mat_mul(&mat_mul(&inverse, &world_delta), &placement.parent_world) };
        let next = mat_mul(&delta, &placement.local);
        if !valid_position(&next) {
            return Err(bad(cmd, "drag exceeds supported numeric range"));
        }
        moves.push((placement.occurrence, next));
    }
    let moved: Vec<u64> = moves.iter().map(|(id, _)| *id).collect();
    if world_move != solidcraft_doc::IDENTITY {
        for (id, next) in moves {
            s.pending_moves.remove(&id);
            if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
                o.transform = next;
            }
        }
    }
    let continuous = p.get("continuous").and_then(Value::as_bool).unwrap_or(false);
    Ok(json!({"occurrence": path.last(), "path": path, "moved": moved, "translate": [t.x, t.y, t.z], "continuous": continuous}))
}
