//! Copy Design with Links: the window that picks and renames what `file.copy_with_links` copies.
//!
//! Opened from the File menu, it lists the saved design, every file it links all the way down and
//! the designs beside them whose drawings draw one (`file.copy_with_links.list`), each ticked and
//! keeping its own file name. Untick a file to leave it where it is (a library part): the copies
//! keep linking it there. Rename a file in its row, or find and replace over every ticked name at
//! once; choose the folder, then Copy runs `file.copy_with_links` with what is ticked. A script, the
//! control channel or MCP calls the command with its own picks and never meets this window.

use std::path::Path;

use serde_json::{Value, json};

use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, ui_kit};

/// One file of the list, as its row edits it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: String,
    /// `design`, `drawing` or `file`.
    pub kind: String,
    /// Why it can't be copied; it is then left where it is, unticked.
    pub problem: Option<String>,
    pub copy: bool,
    /// The copy's file name.
    pub name: String,
}

/// The window's state while it is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CopyLinks {
    /// The design copied.
    pub design: String,
    pub rows: Vec<Row>,
    pub find: String,
    pub replace: String,
    /// The folder the copies go in.
    pub folder: String,
    pub error: Option<String>,
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn folder_of(path: &str) -> String {
    Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

impl CopyLinks {
    /// The window's rows from `file.copy_with_links.list`'s reply.
    pub fn from_list(listed: &Value) -> Option<Self> {
        let design = listed.get("design")?.as_str()?.to_string();
        let rows = listed
            .get("items")?
            .as_array()?
            .iter()
            .filter_map(|item| {
                let path = item.get("path")?.as_str()?.to_string();
                let problem = item.get("problem").and_then(Value::as_str).map(str::to_string);
                Some(Row {
                    name: file_name(&path),
                    kind: item.get("kind").and_then(Value::as_str).unwrap_or("design").to_string(),
                    copy: problem.is_none(),
                    problem,
                    path,
                })
            })
            .collect();
        Some(Self { design, rows, ..Self::default() })
    }

    /// Find and replace over the names of the ticked files; how many changed.
    pub fn replace_all(&mut self) -> usize {
        if self.find.is_empty() {
            return 0;
        }
        let mut changed = 0;
        for row in self.rows.iter_mut().filter(|r| r.copy) {
            let renamed = row.name.replace(&self.find, &self.replace);
            if renamed != row.name {
                row.name = renamed;
                changed += 1;
            }
        }
        changed
    }

    /// `file.copy_with_links`'s parameters for what is ticked.
    pub fn params(&self) -> Value {
        let copy: Vec<Value> = self.rows.iter().filter(|r| r.copy).map(|r| json!({"path": r.path, "name": r.name.trim()})).collect();
        json!({"path": self.design, "folder": self.folder, "copy": copy})
    }
}

/// Open the window for the open design, or say why it can't open.
pub fn open(app: &mut SolveApp) {
    match app.run("file.copy_with_links.list", json!({})) {
        Ok(listed) => match CopyLinks::from_list(&listed) {
            Some(window) => app.copy_links = Some(window),
            None => app.set_status(solidcraft_ui_text::tr("copy_links.unreadable").to_string(), true),
        },
        Err(e) => app.set_status(e, true),
    }
}

/// Ask for the folder with the host's dialog.
fn choose_folder(app: &mut SolveApp) {
    let current = app.copy_links.as_ref().map(|w| if w.folder.is_empty() { folder_of(&w.design) } else { w.folder.clone() }).unwrap_or_default();
    app.pick_folder(&current, |app, path| {
        if let (Some(p), Some(window)) = (path, app.copy_links.as_mut()) {
            window.folder = p;
        }
    });
}

/// The window, while it is open. Copy writes the copies; Cancel, Esc or the close button writes
/// nothing.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut window) = app.copy_links.take() else { return };
    let t = solidcraft_theme::Tokens::get();
    let title = solidcraft_ui_text::tr("cmd.file_copy_with_links");
    let ok = solidcraft_ui_text::tr("copy_links.copy");
    let cancel = solidcraft_ui_text::tr("ui.cancel");
    let width = (t.form_card_width * 3.0).min((ctx.content_rect().width() - t.space_24 * 2.0).max(t.form_card_width));
    let mut pick_folder = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_copy_links"), "copy", title)
        .actions(ok, cancel, None)
        .cards(width / t.form_card_width)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, true, |ui| {
            ui_kit::text(ui, solidcraft_ui_text::tr("copy_links.intro"), ui_kit::Text::Note("copy_links.intro"));
            ui.add_space(t.space_6);
            ui_kit::text_row(ui, solidcraft_ui_text::tr("copy_links.find"), &mut window.find, "");
            ui_kit::text_row(ui, solidcraft_ui_text::tr("copy_links.replace_with"), &mut window.replace, "");
            if ui_kit::button(ui, solidcraft_ui_text::tr("copy_links.replace_all"), ui_kit::Button::Normal).clicked() {
                window.replace_all();
            }
            ui.add_space(t.space_6);
            rows(ui, &mut window);
            ui.add_space(t.space_6);
            ui.horizontal(|ui| {
                ui_kit::text(ui, solidcraft_ui_text::tr("copy_links.folder"), ui_kit::Text::Inline);
                if !window.folder.is_empty() {
                    ui_kit::text(ui, &window.folder, ui_kit::Text::Report("folder", false));
                }
                if ui_kit::button(ui, solidcraft_ui_text::tr("copy_links.choose_folder"), ui_kit::Button::Normal).clicked() {
                    pick_folder = true;
                }
            });
            if let Some(error) = &window.error {
                ui_kit::text(ui, error, ui_kit::Text::Error("copy_links_error"));
            }
        });
    match action {
        ui_kit::Action::Ok => {
            if window.folder.trim().is_empty() {
                window.error = Some(solidcraft_ui_text::tr("copy_links.no_folder").to_string());
                app.copy_links = Some(window);
                return;
            }
            match app.run("file.copy_with_links", window.params()) {
                Ok(made) => {
                    let count = made.get("copied").and_then(Value::as_array).map_or(0, Vec::len).to_string();
                    let message = solidcraft_ui_text::format("copy_links.done", &[("count", &count), ("folder", &window.folder)]);
                    app.set_status(message, false);
                }
                Err(e) => {
                    window.error = Some(e);
                    app.copy_links = Some(window);
                }
            }
        }
        ui_kit::Action::None if !closed => {
            app.copy_links = Some(window);
            if pick_folder {
                choose_folder(app);
            }
        }
        // Esc closes this window only.
        _ => {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
    }
}

/// The list: a tick, the file, where it is, what it is, and the copy's name.
fn rows(ui: &mut egui::Ui, window: &mut CopyLinks) {
    let t = solidcraft_theme::Tokens::get();
    let columns = [
        ui_kit::Column::new("", 0.25).icon_only(),
        ui_kit::Column::new(solidcraft_ui_text::tr("copy_links.file"), 1.6).flex(),
        ui_kit::Column::new(solidcraft_ui_text::tr("copy_links.kind"), 0.7),
        ui_kit::Column::new(solidcraft_ui_text::tr("copy_links.new_name"), 1.8).flex(),
        ui_kit::Column::new(solidcraft_ui_text::tr("copy_links.location"), 1.0).flex(),
    ];
    let design = window.design.clone();
    egui::ScrollArea::vertical().max_height(t.form_card_width * 1.2).show(ui, |ui| {
        ui_kit::table_custom(ui, "copy_links_rows", &columns, "", |ui, table| {
            table.headers(ui, |_, _| false);
            for row in &mut window.rows {
                let top = row.path == design;
                table.row(ui, &row.path.clone(), false, |cells| {
                    cells.cell(|ui| {
                        // The design itself is always copied; a file that can't be read stays.
                        ui.add_enabled_ui(!top && row.problem.is_none(), |ui| ui_kit::checkbox(ui, "", &mut row.copy));
                    });
                    cells.cell(|ui| ui_kit::text(ui, &file_name(&row.path), ui_kit::Text::Inline));
                    cells.cell(|ui| {
                        let kind = match row.kind.as_str() {
                            "drawing" => "copy_links.kind_drawing",
                            "file" => "copy_links.kind_file",
                            _ => "copy_links.kind_design",
                        };
                        ui_kit::text(ui, solidcraft_ui_text::tr(kind), ui_kit::Text::Cell)
                    });
                    cells.cell(|ui| match &row.problem {
                        Some(why) => ui_kit::text(ui, why, ui_kit::Text::Error("copy_links_problem")),
                        None if row.copy => {
                            ui_kit::text_field(ui, &mut row.name, "");
                            ui.label("")
                        }
                        None => ui_kit::text(ui, solidcraft_ui_text::tr("copy_links.left_in_place"), ui_kit::Text::Cell),
                    });
                    cells.cell(|ui| {
                        // The folder's own name: a library part's stands out from the design's.
                        // The whole path shows on hover.
                        let folder = folder_of(&row.path);
                        let name = Path::new(&folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.clone());
                        ui_kit::text(ui, &name, ui_kit::Text::Cell).backing_on_hover_text(folder)
                    });
                });
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed() -> Value {
        json!({"design": "/w/skid.sc", "items": [
            {"path": "/w/skid.sc", "kind": "design", "used_by": []},
            {"path": "/w/plate.sc", "kind": "design", "used_by": ["/w/skid.sc"]},
            {"path": "/lib/bolt.sc", "kind": "design", "used_by": ["/w/skid.sc"]},
            {"path": "/w/plate drawing.sc", "kind": "drawing", "used_by": ["/w/plate.sc"]},
            {"path": "/w/gone.sc", "kind": "design", "used_by": ["/w/skid.sc"], "problem": "missing"},
        ]})
    }

    #[test]
    fn every_file_starts_ticked_under_its_own_name_but_one_that_cant_be_read() {
        let w = CopyLinks::from_list(&listed()).unwrap();
        assert_eq!(w.design, "/w/skid.sc");
        assert_eq!(
            w.rows.iter().map(|r| (r.name.as_str(), r.copy)).collect::<Vec<_>>(),
            [("skid.sc", true), ("plate.sc", true), ("bolt.sc", true), ("plate drawing.sc", true), ("gone.sc", false)]
        );
        assert_eq!(w.rows[4].problem.as_deref(), Some("missing"));
        assert!(CopyLinks::from_list(&json!({})).is_none());
    }

    #[test]
    fn find_and_replace_renames_only_the_ticked_files_and_the_call_carries_them() {
        let mut w = CopyLinks::from_list(&listed()).unwrap();
        w.rows[2].copy = false; // the library's bolt stays where it is
        w.find = "plate".into();
        w.replace = "base".into();
        assert_eq!(w.replace_all(), 2);
        w.find = "skid".into();
        w.replace = "skid-2".into();
        assert_eq!(w.replace_all(), 1);
        w.folder = "/w2".into();
        assert_eq!(
            w.params(),
            json!({"path": "/w/skid.sc", "folder": "/w2", "copy": [
                {"path": "/w/skid.sc", "name": "skid-2.sc"},
                {"path": "/w/plate.sc", "name": "base.sc"},
                {"path": "/w/plate drawing.sc", "name": "base drawing.sc"},
            ]})
        );
        w.find.clear();
        assert_eq!(w.replace_all(), 0, "nothing to find changes nothing");
    }

    /// A saved skid of two parts in one folder and a bolt in another.
    fn skid(h: &mut crate::scenario::Harness, root: &Path) -> std::path::PathBuf {
        let (work, lib) = (root.join("Skid-100"), root.join("Library"));
        for dir in [&work, &lib] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let (plate, bolt, skid) = (work.join("Skid-100 Plate.sc"), lib.join("Bolt.sc"), work.join("Skid-100.sc"));
        for (path, height) in [(&plate, 5), (&bolt, 40)] {
            h.call("ui.documents", json!({"action": "new"}));
            h.app.run("solid.box", json!({"length": 10, "width": 20, "height": height, "corner": [0, 0, 0]})).unwrap();
            h.app.run("file.save", json!({"path": path})).unwrap();
        }
        h.call("ui.documents", json!({"action": "new"}));
        h.app.run("file.save", json!({"path": skid})).unwrap();
        for part in [&plate, &bolt] {
            h.app.run("component.insert_design", json!({"path": part})).unwrap();
        }
        h.app.run("file.save", json!({"path": skid})).unwrap();
        skid
    }

    /// The whole flow through the window: open it, untick the library's bolt, find and replace
    /// over the names, and Copy writes the ticked files under their new names, the bolt left out.
    #[test]
    fn the_window_copies_the_ticked_files_under_the_names_found_and_replaced() {
        let root = std::env::temp_dir().join(format!("solidcraft-copy-links-window-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut h = crate::scenario::Harness::new();
        skid(&mut h, &root);
        let out = root.join("Skid-200");
        std::fs::create_dir_all(&out).unwrap();
        h.app.start("file.copy_with_links");
        h.frames(3);
        let w = h.app.copy_links.as_mut().expect("starting the command opens the window");
        assert_eq!(w.rows.len(), 3, "{:?}", w.rows);
        let bolt = w.rows.iter_mut().find(|r| r.name == "Bolt.sc").unwrap();
        bolt.copy = false;
        w.find = "Skid-100".into();
        w.replace = "Skid-200".into();
        w.folder = out.to_string_lossy().into_owned();
        h.step(&json!({"widget": "Replace All"})).unwrap();
        h.frames(2);
        let names: Vec<String> = h.app.copy_links.as_ref().unwrap().rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, ["Skid-200.sc", "Skid-200 Plate.sc", "Bolt.sc"]);
        h.step(&json!({"widget": "Copy"})).unwrap();
        h.frames(2);
        assert!(h.app.copy_links.is_none(), "Copy closes the window: {:?}", h.app.copy_links.as_ref().and_then(|w| w.error.clone()));
        let mut made: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        made.sort();
        assert_eq!(made, ["Skid-200 Plate.sc", "Skid-200.sc"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
