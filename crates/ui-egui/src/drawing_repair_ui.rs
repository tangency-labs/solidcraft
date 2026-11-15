//! User-confirmed repair controls, separate from the read-only attention card.
//! Only candidates supplied by drawing.dimension.inspect are offered; no guessed geometry.
use crate::drawing_dimension_repair;
use crate::drawing_dimension_status::DrawingAttention;
use serde_json::Value;
use serde_json::json;
use solidcraft_engine::Sel;

/// Convert exactly one existing viewport selection to a repair pick. Never guess.
pub fn selected_geometry(selection: &[Sel]) -> Result<(String, Value), String> {
    match selection {
        [Sel::Edge { body, index, .. }] => Ok(("edge".into(), json!({"edge":{"body":body,"index":index}}))),
        [Sel::Face { body, index, .. }] => Ok(("face".into(), json!({"face":{"body":body,"index":index}}))),
        [] => Err("Select one replacement edge or face in the model viewport first".into()),
        _ => Err("Select exactly one edge or face; multiple or other selections cannot repair a dimension".into()),
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
    let (name, fields) = kind.iter().next().expect("single checked entry");
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

/// Returns true only after a successful mutation. Caller must invalidate/refresh status.
pub fn show(
    ui: &mut egui::Ui,
    selection: &[Sel],
    state: &mut RepairUiState,
    snapshot: &DrawingAttention,
    mut run: impl FnMut(&str, Value) -> Result<Value, String>,
) -> bool {
    if let Some(selected) = &state.dimension {
        if !snapshot.dimensions.iter().any(|d| &d.id == selected && eligible(&d.status)) {
            state.clear();
        }
    }
    let mut changed = false;
    for dim in snapshot.dimensions.iter().filter(|d| eligible(&d.status)) {
        if ui.button(format!("Repair {}", dim.id)).clicked() {
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
                        state.message = Some("The dimension has no supported reference slots.".into());
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
                        state.message = Some(
                            "No candidate geometry was supplied by the engine. Select a replacement in the drawing before repair can be implemented."
                                .into(),
                        );
                    }
                }
                Err(error) => state.message = Some(format!("Inspection failed: {error}")),
            }
        }
    }
    if let Some(id) = state.dimension.clone() {
        ui.separator();
        ui.strong(format!("Repair dimension {id}"));
        ui.label("Select the affected reference slot and an engine-provided candidate.");
        ui.label("Alternatively, select one edge or face in the model viewport and capture it here.");
        if ui.button("Use selected geometry").clicked() {
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
            egui::ComboBox::from_id_salt("drawing_repair_slot")
                .selected_text(if state.slot.is_empty() { "Choose reference" } else { &state.slot })
                .show_ui(ui, |ui| {
                    for slot in &state.slots {
                        if ui.selectable_value(&mut state.slot, slot.clone(), slot).changed() {
                            state.awaiting_confirmation = false;
                        }
                    }
                });
        } else if !state.slot.is_empty() {
            ui.label(format!("Reference: {}", state.slot));
        }
        if !state.candidates.is_empty() && !state.slot.is_empty() {
            egui::ComboBox::from_id_salt("drawing_repair_candidate").selected_text(format!("Candidate {}", state.candidate_index + 1)).show_ui(
                ui,
                |ui| {
                    for (index, candidate) in state.candidates.iter().enumerate() {
                        if ui.selectable_value(&mut state.candidate_index, index, candidate.to_string()).changed() {
                            state.awaiting_confirmation = false;
                        }
                    }
                },
            );
            if !state.awaiting_confirmation {
                if ui.button("Review repair").clicked() {
                    state.awaiting_confirmation = true;
                }
            } else {
                ui.label("Confirm replacing this reference. This modifies the drawing and creates an undo step.");
                if ui.button("Confirm repair").clicked() {
                    let status = snapshot.dimensions.iter().find(|d| d.id == id).map(|d| d.status.as_str()).unwrap_or("");
                    let Some(candidate) = state.candidates.get(state.candidate_index).cloned() else {
                        state.message = Some("Selected repair candidate is no longer available.".to_owned());
                        state.awaiting_confirmation = false;
                        return false;
                    };
                    match drawing_dimension_repair::prepare(&id, status, &state.slot, candidate).and_then(|request| {
                        run("drawing.dimension.repair", serde_json::json!({"dimension":request.dimension,"slot":request.slot,"pick":request.pick}))
                            .map(|_| ())
                    }) {
                        Ok(()) => {
                            changed = true;
                            state.clear();
                        }
                        Err(error) => {
                            state.message = Some(format!("Repair failed: {error}"));
                            state.awaiting_confirmation = false;
                        }
                    }
                }
            }
        }
        if ui.button("Cancel repair").clicked() {
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
