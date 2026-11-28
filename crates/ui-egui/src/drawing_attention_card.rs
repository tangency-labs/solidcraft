//! Read-only Drawing attention card. Kept separate from sheet rendering (#13211).
//! The host supplies a current DrawingAttention snapshot; no CAD command runs here.
use crate::drawing_dimension_status::{DimensionItem, DrawingAttention};
use crate::ui_kit;
use std::collections::BTreeMap;

fn tr(id: &str) -> String {
    solidcraft_ui_text::format(id, &[])
}
fn fmt(id: &str, values: &[(&str, &str)]) -> String {
    solidcraft_ui_text::format(id, values)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionRow {
    pub id: String,
    pub state: String,
    pub reason: String,
    pub location: String,
}

pub fn rows(snapshot: &DrawingAttention) -> Vec<AttentionRow> {
    rows_with_names(snapshot, &BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new())
}

pub fn rows_with_names(
    snapshot: &DrawingAttention,
    dimensions: &BTreeMap<String, String>,
    views: &BTreeMap<String, String>,
    sheets: &BTreeMap<String, String>,
) -> Vec<AttentionRow> {
    snapshot
        .dimensions
        .iter()
        .filter(|d| d.needs_attention())
        .map(|d| AttentionRow {
            id: dimensions.get(&d.id).cloned().unwrap_or_else(|| d.id.clone()),
            state: state(d),
            reason: reason(d),
            location: match (&d.sheet, &d.view) {
                (Some(sheet), Some(view)) => fmt(
                    "ui.issue15776_attention.sheet_view",
                    &[
                        ("sheet", sheets.get(sheet).map(String::as_str).unwrap_or(sheet)),
                        ("view", views.get(view).map(String::as_str).unwrap_or(view)),
                    ],
                ),
                (None, Some(view)) => fmt("ui.issue15776_attention.view_only", &[("view", views.get(view).map(String::as_str).unwrap_or(view))]),
                _ => tr("ui.issue15776_attention.unknown"),
            },
        })
        .collect()
}

fn state(d: &DimensionItem) -> String {
    match d.status.as_str() {
        "lost" => tr("ui.issue15776_attention.lost"),
        "ambiguous" => tr("ui.issue15776_attention.ambiguous"),
        "ok" | "overridden" if d.out_of_date => tr("ui.issue15776_attention.stale"),
        "ok" | "overridden" => tr("ui.issue15776_attention.inspect"),
        _ => tr("ui.issue15776_attention.other"),
    }
}

fn reason(d: &DimensionItem) -> String {
    if let Some(error) = &d.inspect_error {
        return fmt("ui.issue15776_attention.inspection_failed", &[("error", error)]);
    }
    if let Some(why) = &d.why
        && !why.trim().is_empty()
    {
        return why.clone();
    }
    if d.out_of_date {
        return tr("ui.issue15776_attention.changed");
    }
    tr("ui.issue15776_attention.reference")
}

/// Draw a status card only; callers decide its position and refresh cadence.
/// State is conveyed by both text and a shape, never by colour alone.
pub fn show(
    ui: &mut egui::Ui,
    snapshot: &DrawingAttention,
    dimensions: &BTreeMap<String, String>,
    views: &BTreeMap<String, String>,
    sheets: &BTreeMap<String, String>,
) {
    let items = rows_with_names(snapshot, dimensions, views, sheets);
    ui.group(|ui| {
        ui_kit::section_heading(ui, solidcraft_ui_text::tr("ui.issue15776_attention.title"));
        if items.is_empty() {
            ui.label(format!("✓ {}", solidcraft_ui_text::tr("ui.issue15776_attention.clear")));
        } else {
            ui.label(format!("◇ {}", fmt("ui.issue15776_attention.count", &[("count", &items.len().to_string())])));
            for item in items {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label("◇");
                    ui.strong(format!("{} {} · {}", solidcraft_ui_text::tr("ui.issue15776_attention.dimension"), item.id, item.state));
                });
                ui.label(&item.reason);
                ui.small(&item.location);
            }
        }
        if snapshot.held {
            ui.label(format!("◇ {}", solidcraft_ui_text::tr("ui.issue15776_attention.held")));
        }
        if snapshot.waiting_views > 0 {
            ui.label(format!("◇ {}", fmt("ui.issue15776_attention.waiting", &[("count", &snapshot.waiting_views.to_string())])));
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
        assert_eq!(list[0].state, tr("ui.issue15776_attention.lost"));
        assert_eq!(list[0].reason, "Edge deleted");
        assert_eq!(list[0].location, tr("ui.issue15776_attention.unknown"));
        assert_eq!(list[1].state, tr("ui.issue15776_attention.stale"));
    }

    #[test]
    fn inspection_error_is_visible_without_claiming_repair() {
        let mut snapshot = from_status(&json!({"dimensions":[{"dimension":"d","status":"ambiguous"}]}));
        snapshot.dimensions[0].inspect_error = Some("not available".to_owned());
        let list = rows(&snapshot);
        assert_eq!(list[0].reason, fmt("ui.issue15776_attention.inspection_failed", &[("error", "not available")]));
        assert_eq!(list[0].state, tr("ui.issue15776_attention.ambiguous"));
    }
}
