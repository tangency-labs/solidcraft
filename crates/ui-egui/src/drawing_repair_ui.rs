//! User-confirmed repair controls, separate from the read-only attention card.
//! Only candidates supplied by drawing.dimension.inspect are offered; no guessed geometry.
use crate::drawing_dimension_repair;
use crate::drawing_dimension_status::DrawingAttention;
use serde_json::Value;
use solidcraft_engine::{Sel, Session};
use serde_json::json;

/// Convert exactly one existing viewport selection to a repair pick. Never guess.
pub fn selected_geometry(selection: &[Sel]) -> Result<(String, Value), String> {
    match selection {
        [Sel::Edge { body, index, .. }] => Ok(("edge".into(), json!({"edge":{"body":body,"index":index}}))),
        [Sel::Face { body, index, .. }] => Ok(("face".into(), json!({"face":{"body":body,"index":index}}))),
        [] => Err("Select one replacement edge or face in the model viewport first".into()),
        _ => Err("Select exactly one edge or face; multiple or other selections cannot repair a dimension".into()),
    }
}


#[derive(Default)]
pub struct RepairUiState {
    dimension: Option<String>,
    slot: String,
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
pub fn show(ui: &mut egui::Ui, session: &mut Session, state: &mut RepairUiState, snapshot: &DrawingAttention) -> bool {
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
            state.slot = "edge".into();
            match session.execute("drawing.dimension.inspect", &serde_json::json!({"dimension": dim.id})) {
                Ok(inspection) => {
                    state.candidates = inspection.get("candidates").and_then(Value::as_array).cloned().unwrap_or_default()
                        .into_iter().filter(|candidate| candidate.is_object()).collect();
                    if state.candidates.is_empty() {
                        state.message = Some("No candidate geometry was supplied by the engine. Select a replacement in the drawing before repair can be implemented.".into());
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
            match selected_geometry(&session.selection) {
                Ok((slot, pick)) => {
                    state.slot = slot;
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
        ui.horizontal(|ui| {
            ui.label("Reference slot");
            if ui.text_edit_singleline(&mut state.slot).changed() {
                state.awaiting_confirmation = false;
            }
        });
        if !state.candidates.is_empty() {
            egui::ComboBox::from_id_salt("drawing_repair_candidate")
                .selected_text(format!("Candidate {}", state.candidate_index + 1))
                .show_ui(ui, |ui| {
                    for (index, candidate) in state.candidates.iter().enumerate() {
                        if ui.selectable_value(&mut state.candidate_index, index, candidate.to_string()).changed() {
                            state.awaiting_confirmation = false;
                        }
                    }
                });
            if !state.awaiting_confirmation {
                if ui.button("Review repair").clicked() {
                    state.awaiting_confirmation = true;
                }
            } else {
                ui.label("Confirm replacing this reference. This modifies the drawing and creates an undo step.");
                if ui.button("Confirm repair").clicked() {
                    let status = snapshot.dimensions.iter().find(|d| d.id == id).map(|d| d.status.as_str()).unwrap_or("");
                    let candidate = state.candidates[state.candidate_index].clone();
                    match drawing_dimension_repair::prepare(&id, status, &state.slot, candidate)
                        .and_then(|request| drawing_dimension_repair::confirm(session, &request, true).map(|_| ())) {
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
        let mut state = RepairUiState { dimension: Some("d".into()), slot: "edge".into(), candidates: vec![serde_json::json!({"edge":{"body":"b","index":1}})], candidate_index: 0, awaiting_confirmation: true, message: None };
        state.clear();
        assert!(state.dimension.is_none());
        assert!(!state.awaiting_confirmation);
        assert!(state.candidates.is_empty());
    }
}
