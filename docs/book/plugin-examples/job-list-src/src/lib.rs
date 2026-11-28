wit_bindgen::generate!({ path: "wit", world: "panel-plugin" });

use craft::plugin::commands;
use craft::plugin::types::{Datum, FieldFailure, PluginError, Value as Cell};
use craft::plugin::ui::{self, ButtonSpec, Column, Declarations, DockZone, Layout, Node, PanelSpec, SelectionMode, TableSpec, UiEvent, Widget};
use serde_json::{Value, json};

struct Plugin;

impl exports::craft::plugin::entry::Guest for Plugin {
    fn execute(entry: String, params: String) -> Result<String, PluginError> {
        let params: Value = serde_json::from_str(&params).map_err(|e| invalid("params", e))?;
        let mut jobs = read_jobs()?;
        match entry.as_str() {
            "add" => {
                let name = params["name"].as_str().unwrap_or("Pocket");
                let tool = params["tool"].as_f64().unwrap_or(6.0);
                jobs.push(json!({"id": format!("job{}", jobs.len() + 1), "name": name, "tool": tool}));
                // The jobs live in the design: one undo step, saved with it.
                commands::run("storage.set", &json!({"document": {"jobs": jobs}}).to_string())?;
                ui::show_panel("jobs", true)?;
            }
            "clear" => {
                jobs.clear();
                commands::run("storage.set", &json!({"document": {"jobs": jobs}}).to_string())?;
            }
            "refresh" => {}
            _ => return Err(invalid("entry", format!("no entry `{entry}`"))),
        }
        show(&jobs)?;
        Ok(json!({"jobs": jobs.len()}).to_string())
    }
}

/// The jobs kept in the open design, oldest first.
fn read_jobs() -> Result<Vec<Value>, PluginError> {
    let reply: Value = serde_json::from_str(&commands::run("storage.get", "{}")?).map_err(|e| invalid("storage", e))?;
    Ok(reply["document"]["jobs"].as_array().cloned().unwrap_or_default())
}

/// Replace the panel's rows: one cell per path, "/jobs/<row>/<column>".
fn show(jobs: &[Value]) -> Result<(), PluginError> {
    let mut cells = Vec::new();
    for (row, job) in jobs.iter().enumerate() {
        let cell = |key: &str, value: Cell| Datum { path: format!("/jobs/{row}/{key}"), value };
        cells.push(cell("id", Cell::Text(job["id"].as_str().unwrap_or_default().into())));
        cells.push(cell("name", Cell::Text(job["name"].as_str().unwrap_or_default().into())));
        cells.push(cell("tool", Cell::Number(job["tool"].as_f64().unwrap_or_default())));
    }
    ui::clear_data("jobs")?;
    ui::set_data("jobs", &cells)
}

impl exports::craft::plugin::ui_provider::Guest for Plugin {
    /// The same panel as `runtime.ui` in plugin.json.
    fn describe() -> Declarations {
        let button = |label: &str, entry: &str| ButtonSpec { label: label.into(), icon: None, entry: entry.into() };
        let column = |key: &str, label: &str, unit: Option<&str>| Column { key: key.into(), label: label.into(), unit: unit.map(Into::into) };
        let table = TableSpec {
            bind: "/jobs".into(),
            row_id: "id".into(),
            columns: vec![column("name", "Job", None), column("tool", "Tool", Some("length"))],
            select: SelectionMode::Single,
            on_select: None,
            on_edit: None,
        };
        let nodes = vec![
            Node { widget: Widget::Group(vec![1, 2]) },
            Node { widget: Widget::Toolbar(vec![button("Refresh", "refresh"), button("Clear", "clear")]) },
            Node { widget: Widget::Table(table) },
        ];
        let panel =
            PanelSpec { id: "jobs".into(), title: "Jobs".into(), zones: vec![DockZone::Right, DockZone::Bottom], layout: Layout { root: 0, nodes } };
        Declarations { ui_version: 1, panels: vec![panel], workspaces: vec![] }
    }

    fn on_event(_event: UiEvent) -> Result<(), PluginError> {
        Ok(())
    }

    fn on_enter(_workspace: String) -> Result<(), PluginError> {
        Ok(())
    }

    fn on_exit(_workspace: String) -> Result<(), PluginError> {
        Ok(())
    }
}

fn invalid(field: &str, reason: impl ToString) -> PluginError {
    PluginError::InvalidInput(FieldFailure { field: field.into(), reason: reason.to_string() })
}

export!(Plugin);
