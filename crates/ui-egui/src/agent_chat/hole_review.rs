//! The Assistant can hold a proposed Hole for a person to check on the model. The preview
//! runs the very same command the Apply button submits, on a scratch session.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{SolveApp, ui_kit};
use solidcraft_control::DesignGuard;
use solidcraft_ui_text::tr;

pub(super) const COMMAND: &str = "assistant.review_hole";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    target: String,
    position: [f64; 3],
    diameter: f64,
    depth: Option<f64>,
}

pub(super) struct Review {
    pub(super) target: String,
    position: [String; 3],
    diameter: String,
    depth: String,
    pub(super) document: DesignGuard,
}

impl Review {
    pub(super) fn new(app: &mut SolveApp, params: &Value) -> Result<Self, String> {
        if app.session.read_only {
            return Err("this design is read-only".into());
        }
        let proposal: Proposal = crate::control_params::parse(params)?;
        if proposal.target.is_empty()
            || proposal.target == "*"
            || proposal.target.len() > 512
            || proposal.target.chars().any(char::is_control)
            || app.session.model.state().body(&proposal.target).is_none()
        {
            return Err("target must name one existing body".into());
        }
        let review = Self {
            target: proposal.target,
            position: proposal.position.map(|v| v.to_string()),
            diameter: proposal.diameter.to_string(),
            depth: proposal.depth.map_or_else(String::new, |v| v.to_string()),
            document: DesignGuard {
                document_id: app.session.document_id.clone(),
                design_revision: app.session.design_revision(),
                session_revision: app.session.revision,
            },
        };
        if review.params().is_none() {
            return Err("position must contain three finite numbers and the diameter and optional depth must be positive".into());
        }
        Ok(review)
    }

    pub(super) fn params(&self) -> Option<Value> {
        let [x, y, z] = self.position.each_ref().map(|v| ui_kit::parse_number(v));
        let positive = |s: &str| ui_kit::parse_number(s).filter(|v| *v > 0.0);
        let mut p = json!({
            "position": [x?, y?, z?],
            "diameter": positive(&self.diameter)?,
            "targets": [self.target],
        });
        if !self.depth.trim().is_empty() {
            p["depth"] = json!(positive(&self.depth)?);
        }
        Some(p)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn fields(&mut self, ui: &mut egui::Ui) {
        ui.label(tr("ui.agent_hole_review_position"));
        let mut row = |label: &str, key: &str, value: &mut String, hint: &str| {
            ui.push_id(key, |ui| {
                ui_kit::row(ui, label, |ui| {
                    let (_, response) = ui_kit::text_field_reserving(ui, value, hint, 0.0);
                    solidcraft_ui_registry::publish_handle(&format!("agent_chat:hole_review:{key}"), response.rect.center());
                });
            });
            if value.len() > 64 {
                *value = value.chars().take(64).collect();
            }
        };
        for ((label, key), value) in [("X", "x"), ("Y", "y"), ("Z", "z")].into_iter().zip(&mut self.position) {
            row(tr(label), key, value, "");
        }
        row(tr("ui.diameter"), "diameter", &mut self.diameter, "");
        row(tr("ui.depth"), "depth", &mut self.depth, tr("ui.agent_hole_review_through"));
    }
}
