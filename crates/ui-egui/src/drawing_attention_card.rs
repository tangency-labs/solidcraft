//! Read-only Drawing attention card. Kept separate from sheet rendering (#13211).
//! The host supplies a current DrawingAttention snapshot; no CAD command runs here.
use crate::drawing_dimension_status::{DimensionItem, DrawingAttention};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionRow {
    pub id: String,
    pub state: &'static str,
    pub reason: String,
    pub location: String,
}

pub fn rows(snapshot: &DrawingAttention) -> Vec<AttentionRow> {
    snapshot.dimensions.iter().filter(|d| d.needs_attention()).map(|d| AttentionRow {
        id: d.id.clone(),
        state: state(d),
        reason: reason(d),
        location: match (&d.sheet, &d.view) {
            (Some(sheet), Some(view)) => format!("Sheet {sheet} · View {view}"),
            (None, Some(view)) => format!("View {view} · sheet unknown"),
            _ => "Location unknown".to_owned(),
        },
    }).collect()
}

fn state(d: &DimensionItem) -> &'static str {
    match d.status.as_str() {
        "lost" => "Lost",
        "ambiguous" => "Ambiguous",
        "ok" | "overridden" if d.out_of_date => "Out of date",
        "ok" | "overridden" => "Inspection needed",
        _ => "Needs attention",
    }
}

fn reason(d: &DimensionItem) -> String {
    if let Some(error) = &d.inspect_error {
        return format!("Inspection failed: {error}");
    }
    if let Some(why) = &d.why {
        if !why.trim().is_empty() {
            return why.clone();
        }
    }
    if d.out_of_date {
        return "The drawing changed since this dimension was updated.".to_owned();
    }
    "The dimension reference needs inspection.".to_owned()
}

/// Draw a status card only; callers decide its position and refresh cadence.
/// State is conveyed by both text and a shape, never by colour alone.
pub fn show(ui: &mut egui::Ui, snapshot: &DrawingAttention) {
    let items = rows(snapshot);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.heading("Drawing status");
        if items.is_empty() {
            ui.label("✓ No dimensions need attention");
        } else {
            ui.label(format!("◇ {} dimensions need attention", items.len()));
            for item in items {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label("◇");
                    ui.strong(format!("{} · {}", item.id, item.state));
                });
                ui.label(&item.reason);
                ui.small(&item.location);
            }
        }
        if snapshot.held {
            ui.label("◇ Drawing updates are held");
        }
        if snapshot.waiting_views > 0 {
            ui.label(format!("◇ {} views waiting for update", snapshot.waiting_views));
        }
        for warning in &snapshot.warnings {
            ui.label(format!("◇ {warning}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing_dimension_status::from_status;
    use serde_json::json;

    #[test]
    fn shows_only_affected_dimensions_and_never_guesses_location() {
        let snapshot = from_status(&json!({"dimensions":[
            {"dimension":"d1","status":"lost","why":"Edge deleted"},
            {"dimension":"d2","status":"ok","out_of_date":true},
            {"dimension":"d3","status":"ok"}
        ]}));
        let list = rows(&snapshot);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].state, "Lost");
        assert_eq!(list[0].reason, "Edge deleted");
        assert_eq!(list[0].location, "Location unknown");
        assert_eq!(list[1].state, "Out of date");
    }

    #[test]
    fn inspection_error_is_visible_without_claiming_repair() {
        let mut snapshot = from_status(&json!({"dimensions":[{"dimension":"d","status":"ambiguous"}]}));
        snapshot.dimensions[0].inspect_error = Some("not available".to_owned());
        let list = rows(&snapshot);
        assert_eq!(list[0].reason, "Inspection failed: not available");
        assert_eq!(list[0].state, "Ambiguous");
    }
}
