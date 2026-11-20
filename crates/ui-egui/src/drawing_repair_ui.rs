//! User-confirmed repair controls, separate from the read-only attention card.
//! Only candidates supplied by drawing.dimension.inspect are offered; no guessed geometry.
use crate::drawing_dimension_repair;
use crate::drawing_dimension_status::DrawingAttention;
use crate::ui_kit;
use serde_json::Value;
use serde_json::json;
use solidcraft_engine::Sel;

/// Convert exactly one existing viewport selection to a repair pick. Never guess.
pub fn selected_geometry(selection: &[Sel]) -> Result<(String, Value), String> {
    match selection {
        [Sel::Edge { body, index, .. }] => Ok(("edge".into(), json!({"edge":{"body":body,"index":index}}))),
        [Sel::Face { body, index, .. }] => Ok(("face".into(), json!({"face":{"body":body,"index":index}}))),
        [] => Err(solidcraft_ui_text::tr("ui.issue15776_repair.no_selection").into()),
        _ => Err(solidcraft_ui_text::tr("ui.issue15776_repair.multiple_selection").into()),
    }
}

/// Allowed reference slots come from the saved dimension kind, never from a guess.
/// The engine validates the chosen slot and pick again on confirmation.
pub fn reference_slots(inspection: &Value) -> Vec<String> {
    let Some(kind) = inspection.get("record").and_then(|r| r.get("kind")).and_then(Value::as_object) else {
        return Vec::new();
    };
    if kind.len() != 1 {
        return Vec::new();
    }
    let Some((name, fields)) = kind.iter().next() else {
        return Vec::new();
    };
    match name.as_str() {
        "linear" | "angle" | "silhouettes" | "faces" => vec!["a".into(), "b".into()],
        "length" | "radial" => vec!["edge".into()],
        "cylinder" | "sphere" | "cone" => vec!["face".into()],
        "chain" => fields
            .get("points")
            .and_then(Value::as_array)
            .map(|points| (0..points.len()).map(|index| index.to_string()).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[derive(Default)]
pub struct RepairUiState {
    dimension: Option<String>,
    slot: String,
    slots: Vec<String>,
    candidates: Vec<Value>,
    candidate_index: usize,
    awaiting_confirmation: bool,
    message: Option<String>,
}

impl RepairUiState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

pub fn eligible(status: &str) -> bool {
    matches!(status, "lost" | "ambiguous")
}

// Only filter slots whose geometry type is unambiguous here. The engine still validates every pick.
pub(crate) fn candidate_matches_slot(slot: &str, candidate: &Value) -> bool {
    match slot {
        "edge" | "face" => candidate.as_object().is_some_and(|obj| obj.len() == 1 && obj.contains_key(slot)),
        _ => candidate.is_object(),
    }
}

/// Returns true only after a successful mutation. Caller must invalidate/refresh status.
pub fn show(
    ui: &mut egui::Ui,
    selection: &[Sel],
    state: &mut RepairUiState,
    snapshot: &DrawingAttention,
    mut run: impl FnMut(&str, Value) -> Result<Value, String>,
) -> bool {
    if let Some(selected) = &state.dimension
        && !snapshot.dimensions.iter().any(|d| &d.id == selected && eligible(&d.status))
    {
        state.clear();
    }
    let mut changed = false;
    for dim in snapshot.dimensions.iter().filter(|d| eligible(&d.status)) {
        if ui_kit::button(ui, solidcraft_ui_text::format("ui.issue15776_repair.button", &[("id", &dim.id)]), ui_kit::Button::Normal).clicked() {
            state.clear();
            state.dimension = Some(dim.id.clone());
            state.slot.clear();
            match run("drawing.dimension.inspect", serde_json::json!({"dimension": dim.id})) {
                Ok(inspection) => {
                    state.slots = reference_slots(&inspection);
                    if state.slots.len() == 1 {
                        state.slot = state.slots[0].clone();
                    }
                    if state.slots.is_empty() {
                        state.message = Some(solidcraft_ui_text::tr("ui.issue15776_repair.no_slots").into());
                    }
                    state.candidates = inspection
                        .get("candidates")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|candidate| candidate.is_object())
                        .collect();
                    if state.candidates.is_empty() {
                        state.message = Some(solidcraft_ui_text::tr("ui.issue15776_repair.no_candidates").into());
                    }
                }
                Err(error) => state.message = Some(solidcraft_ui_text::format("ui.issue15776_repair.inspect_failed", &[("error", &error)])),
            }
        }
    }
    if let Some(id) = state.dimension.clone() {
        ui.separator();
        ui.strong(solidcraft_ui_text::format("ui.issue15776_repair.title", &[("id", &id)]));
        ui.label(solidcraft_ui_text::tr("ui.issue15776_repair.instructions"));
        ui.label(solidcraft_ui_text::tr("ui.issue15776_repair.alternative"));
        if ui_kit::button(ui, solidcraft_ui_text::tr("ui.issue15776_repair.use_selected"), ui_kit::Button::Normal).clicked() {
            match selected_geometry(selection) {
                Ok((slot, pick)) => {
                    if state.slots.contains(&slot) {
                        state.slot = slot;
                    } else if state.slots.len() == 1 {
                        state.slot = state.slots[0].clone();
                    }
                    state.candidates = vec![pick];
                    state.candidate_index = 0;
                    state.awaiting_confirmation = false;
                    state.message = None;
                }
                Err(error) => {
                    state.awaiting_confirmation = false;
                    state.message = Some(error);
                }
            }
        }
        if state.slots.len() > 1 {
            let caption = if state.slot.is_empty() { solidcraft_ui_text::tr("ui.issue15776_repair.choose").to_owned() } else { state.slot.clone() };
            ui_kit::dropdown_menu(ui, "drawing_repair_slot", &caption, |ui| {
                for slot in &state.slots {
                    if ui_kit::list_picker(ui, slot, slot, state.slot == *slot).clicked() {
                        state.slot = slot.clone();
                        state.awaiting_confirmation = false;
                        ui.close();
                    }
                }
            });
        } else if !state.slot.is_empty() {
            ui.label(solidcraft_ui_text::format("ui.issue15776_repair.reference", &[("slot", &state.slot)]));
        }
        let compatible: Vec<_> = state
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate_matches_slot(&state.slot, candidate))
            .map(|(index, _)| index)
            .collect();
        if !compatible.contains(&state.candidate_index) {
            if let Some(&first) = compatible.first() {
                state.candidate_index = first;
            }
            state.awaiting_confirmation = false;
        }
        if !compatible.is_empty() && !state.slot.is_empty() {
            let caption = solidcraft_ui_text::format("ui.issue15776_repair.candidate", &[("number", &(state.candidate_index + 1).to_string())]);
            ui_kit::dropdown_menu(ui, "drawing_repair_candidate", &caption, |ui| {
                for &index in &compatible {
                    let label = state.candidates[index].to_string();
                    if ui_kit::list_picker(ui, &label, &label, state.candidate_index == index).clicked() {
                        state.candidate_index = index;
                        state.awaiting_confirmation = false;
                        ui.close();
                    }
                }
            });
            if !state.awaiting_confirmation {
                if ui_kit::button(ui, solidcraft_ui_text::tr("ui.issue15776_repair.review"), ui_kit::Button::Normal).clicked() {
                    state.awaiting_confirmation = true;
                }
            } else {
                ui.label(solidcraft_ui_text::tr("ui.issue15776_repair.confirm_note"));
                if ui_kit::button(ui, solidcraft_ui_text::tr("ui.issue15776_repair.confirm"), ui_kit::Button::Normal).clicked() {
                    let status = snapshot.dimensions.iter().find(|d| d.id == id).map(|d| d.status.as_str()).unwrap_or("");
                    let Some(candidate) = state.candidates.get(state.candidate_index).cloned() else {
                        state.message = Some(solidcraft_ui_text::tr("ui.issue15776_repair.unavailable").to_owned());
                        state.awaiting_confirmation = false;
                        return false;
                    };
                    match drawing_dimension_repair::prepare(&id, status, &state.slot, candidate).and_then(|request| {
                        run("drawing.dimension.repair", serde_json::json!({"dimension":request.dimension,"slot":drawing_dimension_repair::slot_payload(&request.slot),"pick":request.pick}))
                            .map(|_| ())
                    }) {
                        Ok(()) => {
                            changed = true;
                            state.clear();
                        }
                        Err(error) => {
                            state.message = Some(solidcraft_ui_text::format("ui.issue15776_repair.repair_failed", &[("error", &error)]));
                            state.awaiting_confirmation = false;
                        }
                    }
                }
            }
        }
        if ui_kit::button(ui, solidcraft_ui_text::tr("ui.issue15776_repair.cancel"), ui_kit::Button::Normal).clicked() {
            state.clear();
        }
        if let Some(message) = &state.message {
            ui.label(message);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_dimension_kind_without_guessing() {
        for (kind, expected) in [
            ("radial", vec!["edge"]),
            ("cylinder", vec!["face"]),
            ("linear", vec!["a", "b"]),
            ("angle", vec!["a", "b"]),
            ("faces", vec!["a", "b"]),
            ("silhouettes", vec!["a", "b"]),
            ("length", vec!["edge"]),
            ("sphere", vec!["face"]),
            ("cone", vec!["face"]),
        ] {
            let inspection = json!({"record":{"kind":{(kind): {}}}});
            assert_eq!(reference_slots(&inspection), expected);
        }
        assert_eq!(reference_slots(&json!({"record":{"kind":{"chain":{"points":[{},{},{}]}}}})), vec!["0", "1", "2"]);
        assert!(reference_slots(&json!({"record":{}})).is_empty());
        assert!(reference_slots(&json!({"record":{"kind":{"unknown":{}}}})).is_empty());
        assert!(reference_slots(&json!({"record":{"kind":{"linear":{},"radial":{}}}})).is_empty());
    }
    #[test]
    fn only_repairable_states_expose_repair_action() {
        assert!(eligible("lost"));
        assert!(eligible("ambiguous"));
        for state in ["ok", "overridden", "out_of_date", "suppressed", "failed", "unmeasurable"] {
            assert!(!eligible(state), "{state}");
        }
    }
    #[test]
    fn no_viewport_selection_is_not_a_repair_pick() {
        assert!(selected_geometry(&[]).is_err());
    }
    #[test]
    fn candidate_types_match_unambiguous_slots() {
        let edge = json!({"edge":{"body":"Plate","index":0}});
        let face = json!({"face":{"body":"Plate","index":4}});
        assert!(candidate_matches_slot("edge", &edge));
        assert!(!candidate_matches_slot("edge", &face));
        assert!(candidate_matches_slot("face", &face));
        assert!(!candidate_matches_slot("face", &edge));
        assert!(candidate_matches_slot("a", &edge));
    }
    #[test]
    fn cancel_clears_selection_and_confirmation() {
        let mut state = RepairUiState {
            dimension: Some("d".into()),
            slot: "edge".into(),
            slots: vec!["edge".into()],
            candidates: vec![serde_json::json!({"edge":{"body":"b","index":1}})],
            candidate_index: 0,
            awaiting_confirmation: true,
            message: None,
        };
        state.clear();
        assert!(state.dimension.is_none());
        assert!(!state.awaiting_confirmation);
        assert!(state.candidates.is_empty());
    }
}
