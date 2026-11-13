//! Explicit, user-confirmed repair of a lost or ambiguous drawing dimension.
//! No CAD mutation occurs until confirm() is called with a valid selection.
use serde_json::{json, Value};
use solidcraft_engine::Session;

#[derive(Clone, Debug, PartialEq)]
pub struct RepairSelection {
    pub dimension: String,
    pub slot: String,
    pub pick: Value,
}

fn valid_slot(slot: &str) -> bool {
    matches!(slot, "a" | "b" | "edge" | "face") || slot.parse::<usize>().is_ok_and(|n| n.to_string() == slot)
}

fn valid_pick(pick: &Value) -> bool {
    let Some(obj) = pick.as_object() else { return false };
    obj.len() == 1 && ["edge", "corner", "face"].iter().any(|k| obj.contains_key(*k))
        && !pick.to_string().contains("\"path\"")
}

/// Prepare a repair request, but never issue a mutating command.
pub fn prepare(dimension: &str, status: &str, slot: &str, pick: Value) -> Result<RepairSelection, String> {
    if !matches!(status, "lost" | "ambiguous") {
        return Err("Only lost or ambiguous references can be repaired".into());
    }
    if dimension.trim().is_empty() || !valid_slot(slot) || !valid_pick(&pick) {
        return Err("A dimension, valid reference slot, and matching geometry pick are required".into());
    }
    Ok(RepairSelection { dimension: dimension.to_owned(), slot: slot.to_owned(), pick })
}

/// Called only after an explicit user confirmation. The engine validates the
/// actual current status, pick type, occurrence and undo transaction.
pub fn confirm(session: &mut Session, selection: &RepairSelection, confirmed: bool) -> Result<Value, String> {
    if !confirmed {
        return Err("Repair was not confirmed".into());
    }
    session.execute("drawing.dimension.repair", &json!({
        "dimension": selection.dimension,
        "slot": selection.slot,
        "pick": selection.pick,
    })).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_lost_or_ambiguous_references_are_eligible() {
        for state in ["ok", "overridden", "suppressed", "failed", "unmeasurable", "out_of_date"] {
            assert!(prepare("d1", state, "edge", json!({"edge":{"body":"b","index":0}})).is_err(), "{state}");
        }
        for state in ["lost", "ambiguous"] {
            assert!(prepare("d1", state, "edge", json!({"edge":{"body":"b","index":0}})).is_ok());
        }
    }
    #[test]
    fn invalid_or_incomplete_user_selection_is_rejected() {
        assert!(prepare("", "lost", "edge", json!({"edge":{}})).is_err());
        assert!(prepare("d", "lost", "other", json!({"edge":{}})).is_err());
        assert!(prepare("d", "lost", "edge", json!({})).is_err());
        assert!(prepare("d", "lost", "edge", json!({"edge":{},"face":{}})).is_err());
        assert!(prepare("d", "lost", "edge", json!({"edge":{"path":[1]}})).is_err());
    }
    #[test]
    fn confirmation_is_required_before_any_engine_call() {
        let mut session = Session::default();
        let request = prepare("d", "lost", "edge", json!({"edge":{"body":"b","index":0}})).unwrap();
        assert_eq!(confirm(&mut session, &request, false).unwrap_err(), "Repair was not confirmed");
    }
}
