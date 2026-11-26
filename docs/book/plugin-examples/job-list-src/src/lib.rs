wit_bindgen::generate!({ path: "wit", world: "panel-plugin" });

use craft::plugin::types::{Datum, FieldFailure, PluginError, Value as Cell};
use craft::plugin::ui::{self, ButtonSpec, Column, Declarations, DockZone, Layout, Node, PanelSpec, SelectionMode, TableSpec, UiEvent, Widget};
use craft::plugin::{commands, forms};
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
                jobs.push(json!({"id": next_id(&jobs), "name": name, "tool": tool}));
                save(&jobs)?;
                ui::show_panel("jobs", true)?;
            }
            "clear" => {
                jobs.clear();
                save(&jobs)?;
            }
            "refresh" => {}
            // A press of Remove: `row` is the job selected in the table, or null.
            "remove" => {
                let row = selected_row(&params)?;
                jobs.retain(|job| job["id"] != row);
                save(&jobs)?;
            }
            // A double-click on a row: open `update`'s form filled with that job. The form opens
            // once this run has finished; nothing changes until the person applies it.
            "edit" => {
                let row = selected_row(&params)?;
                let job = jobs.iter().find(|job| job["id"] == row).ok_or_else(|| invalid("row", format!("no job `{row}`")))?;
                forms::open("update", &job.to_string())?;
            }
            // The form's Apply: `id` came from `edit` and is kept although the form has no field for it.
            "update" => {
                let id = params["id"].as_str().ok_or_else(|| invalid("id", "which job to change"))?;
                let job = jobs.iter_mut().find(|job| job["id"] == id).ok_or_else(|| invalid("id", format!("no job `{id}`")))?;
                job["name"] = params["name"].clone();
                job["tool"] = params["tool"].clone();
                save(&jobs)?;
            }
            _ => return Err(invalid("entry", format!("no entry `{entry}`"))),
        }
        show(&jobs)?;
        Ok(json!({"jobs": jobs.len()}).to_string())
    }
}

/// Write the jobs into the design: one undo step, saved with it.
fn save(jobs: &[Value]) -> Result<(), PluginError> {
    commands::run("storage.set", &json!({"document": {"jobs": jobs}}).to_string())?;
    Ok(())
}

/// The `row` of a panel event: the id of the table row it is about.
fn selected_row(params: &Value) -> Result<&str, PluginError> {
    params["row"].as_str().ok_or_else(|| invalid("row", "select a job in the table first"))
}

/// An id no job has: one past the highest number in use, so a removed job's id is not reused.
fn next_id(jobs: &[Value]) -> String {
    let highest = jobs.iter().filter_map(|job| job["id"].as_str()?.strip_prefix("job")?.parse::<u64>().ok()).max().unwrap_or(0);
    format!("job{}", highest + 1)
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
            on_edit: Some("edit".into()),
        };
        let nodes = vec![
            Node { widget: Widget::Group(vec![1, 2]) },
            Node { widget: Widget::Toolbar(vec![button("Refresh", "refresh"), button("Remove", "remove"), button("Clear", "clear")]) },
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
