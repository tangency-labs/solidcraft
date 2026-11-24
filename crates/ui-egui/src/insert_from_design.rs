//! Insert from Design (#6892, experimental): pick internal components out of another design. The
//! other design's parts and sub-assemblies are shown as a tree with a tick box each; what is
//! ticked is copied into the design being worked in as internal components of its own, each with
//! its timeline, placed where the other design had it.
//!
//! Ticking a sub-assembly ticks everything under it, and it comes whole. Unticking one of its
//! parts leaves the sub-assembly itself unticked, so the parts still ticked come as parts.
//!
//! The engine lists (`component.design_contents`) and inserts (`component.insert_from_design`);
//! this window is the app's layer.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;

/// The window's state: the design picked from and what it holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertFromDesign {
    /// The design picked from.
    pub path: String,
    /// The tree, parents before what they hold.
    pub rows: Vec<Row>,
    /// Keep what is picked linked to the design, instead of copying it in.
    pub link: bool,
}

/// One internal component of the design picked from.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// What `component.insert_from_design` takes to pick it.
    pub pick: String,
    /// The name of the component that uses it there.
    pub name: String,
    /// `part`, `assembly` or `design`.
    pub kind: String,
    /// How deep it is inside sub-assemblies: 0 for one the design itself holds.
    pub depth: usize,
    pub ticked: bool,
    /// It holds components of its own, shown under it while `open`.
    pub parent: bool,
    pub open: bool,
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// `inner` is somewhere under `outer` in the tree.
fn under(inner: &str, outer: &str) -> bool {
    inner.len() > outer.len() && inner.starts_with(outer) && inner.as_bytes().get(outer.len()) == Some(&b'/')
}

/// Ask for the design to pick from, then show what it holds.
pub(crate) fn pick(app: &mut SolveApp) {
    app.pick_open(|app, path| {
        if let Some(path) = path {
            open(app, &path);
        }
    });
}

fn rows_of(members: &Value, depth: usize, rows: &mut Vec<Row>) {
    for member in members.as_array().into_iter().flatten() {
        let text = |key: &str| member[key].as_str().unwrap_or_default().to_string();
        let parent = member["members"].as_array().is_some_and(|inside| !inside.is_empty());
        rows.push(Row { pick: text("pick"), name: text("name"), kind: text("kind"), depth, ticked: false, parent, open: true });
        rows_of(&member["members"], depth + 1, rows);
    }
}

/// Show what the design at `path` holds, to pick from. A design that holds nothing says so.
pub(crate) fn open(app: &mut SolveApp, path: &str) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    match app.session.execute("component.design_contents", &json!({"path": path})) {
        Ok(reply) => {
            let mut rows = Vec::new();
            rows_of(&reply["members"], 0, &mut rows);
            if rows.is_empty() {
                app.insert_from_design = None;
                app.set_status(solidcraft_ui_text::format("insert_from_design.none", &[("name", &file_name(path))]), false);
            } else {
                app.insert_from_design = Some(InsertFromDesign { path: path.to_string(), rows, link: false });
            }
        }
        Err(e) => app.set_status(e.to_string(), true),
    }
}

/// Tick or untick the row at `index`, as a click on its box does. Everything under it follows
/// it. A sub-assembly above it is ticked only while everything under that one is ticked, so
/// unticking one part leaves the others to come as parts.
pub(crate) fn tick(state: &mut InsertFromDesign, index: usize, on: bool) {
    let Some(pick) = state.rows.get(index).map(|row| row.pick.clone()) else { return };
    for row in &mut state.rows {
        if row.pick == pick || under(&row.pick, &pick) {
            row.ticked = on;
        }
    }
    // Parents come before what they hold, so going backwards settles the deepest first.
    for at in (0..state.rows.len()).rev() {
        let Some(above) = state.rows.get(at).filter(|row| row.parent && under(&pick, &row.pick)).map(|row| row.pick.clone()) else { continue };
        let all = state.rows.iter().filter(|row| under(&row.pick, &above)).all(|row| row.ticked);
        if let Some(row) = state.rows.get_mut(at) {
            row.ticked = all;
        }
    }
}

/// Some of what is under the row at `index` is ticked and it is not ticked itself.
pub(crate) fn mixed(state: &InsertFromDesign, index: usize) -> bool {
    state.rows.get(index).is_some_and(|row| row.parent && !row.ticked && state.rows.iter().any(|inner| inner.ticked && under(&inner.pick, &row.pick)))
}

/// The picks the ticks stand for: every ticked row that is not inside a ticked sub-assembly,
/// which brings what it holds with it.
pub(crate) fn picks(state: &InsertFromDesign) -> Vec<String> {
    let ticked: Vec<&str> = state.rows.iter().filter(|row| row.ticked).map(|row| row.pick.as_str()).collect();
    ticked.iter().filter(|pick| !ticked.iter().any(|outer| under(pick, outer))).map(|pick| pick.to_string()).collect()
}

/// Insert what is ticked, as one undo step, and close the window. With nothing ticked the
/// window stays.
pub(crate) fn insert(app: &mut SolveApp) {
    let Some(state) = app.insert_from_design.clone() else { return };
    let members = picks(&state);
    if members.is_empty() {
        return;
    }
    app.insert_from_design = None;
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    match app.run("component.insert_from_design", json!({"path": state.path, "members": members, "link": state.link})) {
        Ok(reply) => {
            let count = reply["inserted"].as_array().map_or(0, Vec::len).to_string();
            app.fit_view();
            app.set_status(solidcraft_ui_text::format("insert_from_design.done", &[("count", &count), ("name", &file_name(&state.path))]), false);
        }
        Err(e) => app.set_status(e, true),
    }
}

/// The window, while a design is being picked from.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut state) = app.insert_from_design.clone() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let t = solidcraft_theme::Tokens::get();
    let name = file_name(&state.path);
    let title = solidcraft_ui_text::format("insert_from_design.title", &[("name", &name)]);
    let detail = solidcraft_ui_text::format("insert_from_design.detail", &[("name", &name)]);
    let valid = state.rows.iter().any(|row| row.ticked);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_insert_from_design"), "component", &title)
        .actions(solidcraft_ui_text::tr("insert_from_design.insert"), solidcraft_ui_text::tr("ui.cancel"), None)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Report("insert_from_design", false));
            egui::ScrollArea::vertical().id_salt("insert_from_design_rows").max_height(t.control_height * 12.0).show(ui, |ui| {
                // A row is shown while every sub-assembly above it is open.
                let folded: Vec<String> = state.rows.iter().filter(|row| row.parent && !row.open).map(|row| row.pick.clone()).collect();
                let rows = state.rows.clone();
                for (index, row) in rows.iter().enumerate() {
                    if folded.iter().any(|above| under(&row.pick, above)) {
                        continue;
                    }
                    let part_mixed = mixed(&state, index);
                    ui.push_id(&row.pick, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(t.space_16 * row.depth.min(12) as f32);
                            let (fold, _) = ui.allocate_exact_size(egui::vec2(t.space_16, t.control_height), egui::Sense::hover());
                            if row.parent {
                                let button = ui_kit::disclosure_button_at(ui, fold, row.open, solidcraft_ui_text::tr("insert_from_design.fold"));
                                crate::scenario::publish_target(&format!("insert_from_design_fold:{}", row.pick), button.rect);
                                if button.clicked()
                                    && let Some(shown) = state.rows.get_mut(index)
                                {
                                    shown.open = !row.open;
                                }
                            }
                            let kind = solidcraft_ui_text::tr(match row.kind.as_str() {
                                "assembly" => "insert_from_design.assembly",
                                "part" => "insert_from_design.part",
                                _ => "insert_from_design.design",
                            });
                            let mut on = row.ticked;
                            let check = ui_kit::checkbox_indeterminate(ui, &format!("{} ({kind})", row.name), &mut on, part_mixed);
                            crate::scenario::publish_target(&format!("insert_from_design:{}", row.pick), check.rect);
                            if on != row.ticked {
                                tick(&mut state, index, on);
                            }
                        });
                    });
                }
            });
            // Copied in (the default) or kept linked to the design picked from.
            let keep = ui_kit::checkbox(ui, &solidcraft_ui_text::format("insert_from_design.link", &[("name", &name)]), &mut state.link);
            crate::scenario::publish_target("insert_from_design_link", keep.rect);
            if state.link {
                let linked = solidcraft_ui_text::format("insert_from_design.link_detail", &[("name", &name)]);
                ui_kit::text(ui, &linked, ui_kit::Text::Report("insert_from_design_link", false));
            }
        });
    app.insert_from_design = Some(state);
    match action {
        ui_kit::Action::Ok => insert(app),
        ui_kit::Action::Cancel => app.insert_from_design = None,
        _ if closed => app.insert_from_design = None,
        _ => {}
    }
    if app.insert_from_design.is_some() && ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        app.insert_from_design = None;
        app.esc_handled = true;
    }
}
