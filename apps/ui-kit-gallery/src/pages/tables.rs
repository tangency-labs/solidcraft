//! Tables and lists: the editable table, the name list, tree rows, list pickers and report lists.

use super::{Group, Page};
use crate::{disabled, state};
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as ui_kit;

pub const TABLE: Page = Page {
    id: "table",
    title: "Table",
    group: Group::Tables,
    items: &[
        "table_custom",
        "table_fitted",
        "settings_mouse_mapping",
        "Column",
        "Table",
        "TableRow",
        "CellSelection",
        "SelectionMode",
        "cell_editor",
        "sort_header",
        "Sort",
        "bin_button",
        "removable_header",
        "add_column_button",
        "label_actions",
    ],
    what: "A dense editable table, such as Parameters: columns sized from the form's label column, cells that hold editors, a bin per row and a dashed row at the end that adds one.",
    usage: r#"
let columns = [ui_kit::Column::new("Name", 1.0), ui_kit::Column::new("Value", 1.5).flex(), ui_kit::Column::new("", 0.3).icon_only()];
ui_kit::table_custom(ui, "params", &columns, "No parameters yet", |ui, table| {
    table.headers(ui, |ui, column| column == 0 && { ui_kit::sort_header(ui, "Name", &mut sort); true });
    for (name, value) in &mut rows {
        table.row(ui, name, false, |row| {
            row.cell(|ui| ui_kit::text(ui, name, ui_kit::Text::Cell));
            row.cell(|ui| ui_kit::cell_editor(ui, ui.scope_id().with("value"), value, "", false));
            row.cell(|ui| ui_kit::bin_button(ui, "Delete"));
        });
    }
    if table.add_prompt(ui, "Add parameter", "Add a parameter").clicked() { /* insert a draft row */ }
});
"#,
    dos: &[
        "Add rows with `add_prompt`, never a tiny +.",
        "Mark a cell that does not evaluate with `cell_editor`'s invalid flag.",
        "Let a column the user can widen use `headers_resizable`.",
    ],
    donts: &[
        "Don't build tables from `egui::Grid`: rows, widths and keyboard moves would not match.",
        "Don't delete a row without a bin in the row itself.",
    ],
    demo: |ui, d| {
        state(ui, solidcraft_ui_text::tr("prefs.navigation"), |ui| {
            // These three bindings are shared by the app's default mouse presets.
            let bindings =
                [("prefs.wheel", "prefs.zoom_toward_cursor"), ("prefs.left_click", "prefs.select"), ("prefs.right_click", "prefs.context_menu")]
                    .map(|(input, action)| (vec![solidcraft_ui_text::tr(input).to_owned()], solidcraft_ui_text::tr(action).to_owned()));
            ui_kit::settings_mouse_mapping(ui, &bindings, |_| {});
        });
        state(ui, "Editable, sortable (click Name)", |ui| {
            let columns =
                [ui_kit::Column::new("Name", 1.0), ui_kit::Column::new("Expression", 1.5).flex(), ui_kit::Column::new("Delete", 0.3).icon_only()];
            let mut sort = d.sort;
            let mut remove = None;
            let mut add = false;
            d.sort.apply(&mut d.rows, |r| r.0.as_str());
            ui_kit::table_custom(ui, "gallery_params", &columns, "No parameters yet", |ui, table| {
                table.headers(ui, |ui, column| {
                    if column == 0 {
                        ui_kit::sort_header(ui, "Name", &mut sort);
                        true
                    } else {
                        false
                    }
                });
                for (i, (name, value)) in d.rows.iter_mut().enumerate() {
                    let invalid = value.trim().is_empty();
                    table.row(ui, &format!("row{i}"), i == 0, |row| {
                        row.cell(|ui| ui_kit::text(ui, name, ui_kit::Text::Cell));
                        row.cell(|ui| ui_kit::cell_editor(ui, ui.scope_id().with(("value", i)), value, "Expression", invalid));
                        row.cell(|ui| {
                            if ui_kit::bin_button(ui, "Delete").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                }
                add = table.add_prompt(ui, "Add parameter", "Add a parameter").clicked();
            });
            d.sort = sort;
            if let Some(i) = remove {
                let (name, _) = d.rows.remove(i);
                d.said(format!("deleted {name}"));
            }
            if add {
                let name = format!("Parameter{}", d.rows.len() + 1);
                d.said(format!("added {name}"));
                d.rows.push((name, String::new()));
            }
        });
        for (id, label, fallback, mode) in [
            ("gallery_cell_selection", "ui.table.gallery.cell_selection", "Cell selection", ui_kit::SelectionMode::Cell),
            ("gallery_range_selection", "ui.table.gallery.range_selection", "Range selection", ui_kit::SelectionMode::Range),
        ] {
            // The gallery build includes pending messages. Keep the fallback for hosts that
            // draw this page with only the folded English catalogue installed.
            let translated = solidcraft_ui_text::tr(label);
            state(ui, if translated == label { fallback } else { translated }, |ui| {
                let state_id = ui.make_persistent_id((id, "selection"));
                let mut selection = ui.data_mut(|data| data.get_temp::<ui_kit::CellSelection>(state_id)).unwrap_or_else(|| ui_kit::CellSelection {
                    anchor: Some((1, 1)),
                    focus: Some(if mode == ui_kit::SelectionMode::Cell { (1, 1) } else { (3, 3) }),
                });
                let columns = ["Name", "Expression", "Value", "Status", "Unit"].map(|label| ui_kit::Column::new(label, 1.0).flex());
                ui_kit::table_custom(ui, id, &columns, "Empty", |ui, table| {
                    table.cell_selection(&mut selection, mode);
                    table.headers(ui, |_, _| false);
                    for row in 0..6 {
                        table.row(ui, &row.to_string(), false, |cells| {
                            cells.cell(|ui| ui_kit::text(ui, "Width", ui_kit::Text::Cell));
                            cells.cell(|ui| ui_kit::text(ui, "20 mm", ui_kit::Text::Cell));
                            cells.cell(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui_kit::text(ui, "20.00", ui_kit::Text::MonoCell)
                                });
                            });
                            cells.cell(ui_kit::active_pill);
                            cells.cell(|ui| ui_kit::text(ui, "mm", ui_kit::Text::Cell));
                        });
                    }
                });
                ui.data_mut(|data| data.insert_temp(state_id, selection));
            });
        }
        state(ui, "Empty", |ui| {
            ui_kit::table_custom(ui, "gallery_empty", &[ui_kit::Column::new("Name", 1.0).flex()], "No configurations yet", |ui, table| {
                table.headers(ui, |_, _| false);
                table.add_prompt(ui, "Add configuration", "Add a configuration");
            });
        });
        disabled(ui, |ui| {
            ui_kit::table_custom(
                ui,
                "gallery_off",
                &[ui_kit::Column::new("Name", 1.0), ui_kit::Column::new("Value", 1.0).flex()],
                "None",
                |ui, table| {
                    table.headers(ui, |_, _| false);
                    table.row(ui, "off", false, |row| {
                        row.cell(|ui| ui_kit::text(ui, "Width", ui_kit::Text::Cell));
                        row.cell(|ui| ui_kit::text(ui, "40 mm", ui_kit::Text::Value("off", false)));
                    });
                },
            );
        });
    },
};

pub const GRID: Page = Page {
    id: "grid",
    title: "Spreadsheet grid (coming)",
    group: Group::Tables,
    items: &[],
    what: "A spreadsheet-like grid (`ui_kit::Grid`) for large tables of values, with cell selection and keyboard moves, is being built in its own pull request. It gets a page here once it is in the kit.",
    usage: r#"
// Not in the kit yet. Until it lands, use `table_custom` for editable rows.
"#,
    dos: &["Use `table_custom` until the grid is in the kit."],
    donts: &["Don't build a one-off grid for a single dialog."],
    demo: |ui, _| {
        ui_kit::empty_box(ui, "grid_coming", "The spreadsheet grid is coming in its own pull request.");
    },
};

pub const NAME_LIST: Page = Page {
    id: "name-list",
    title: "Name list",
    group: Group::Tables,
    items: &["name_list", "NameList", "NameRow"],
    what: "A read-only list of named things with a few columns and row actions, such as the files of a folder or the plugins installed. Long names cut in the middle and show in full on hover.",
    usage: r#"
ui_kit::name_list(ui, "files", &["Name", "Size", ""], "No files", |list| {
    list.row("Bracket.solidcraft", |row| {
        row.name("Bracket.solidcraft");
        row.cell(|ui| ui_kit::text(ui, "12.3 KB", ui_kit::Text::Inline));
        row.cell(|ui| ui_kit::button(ui, "Open", ui_kit::Button::Small));
    });
});
"#,
    dos: &["Give it an empty message that says what would be listed."],
    donts: &["Don't use it for values the user edits: that is a `table_custom`."],
    demo: |ui, d| {
        state(ui, "Default", |ui| {
            ui_kit::name_list(ui, "gallery_names", &["Name", "Size", "Actions"], "No files", |list| {
                for (name, size) in [("Bracket.solidcraft", "12.3 KB"), ("Gearbox housing with a very long name.solidcraft", "1.2 MB")] {
                    list.row(name, |row| {
                        row.name(name);
                        row.cell(|ui| ui_kit::text(ui, size, ui_kit::Text::Inline));
                        row.cell(|ui| {
                            if ui_kit::button(ui, "Open", ui_kit::Button::Small).clicked() {
                                d.said(format!("open {name}"));
                            }
                        });
                    });
                }
            });
        });
        state(ui, "Empty", |ui| {
            ui_kit::name_list(ui, "gallery_names_empty", &["Name", "Size"], "No files in this folder", |_| {});
        });
    },
};

pub const TREE: Page = Page {
    id: "tree",
    title: "Tree rows",
    group: Group::Tables,
    items: &["tree_row", "Row", "TreeAction", "RowResp", "tree_header", "install_row_names"],
    what: "The rows of a hierarchy such as the Browser or a render scene: fold arrow, icon, name, subtitle, an eye and hover actions. The host draws each row from its data and runs what the row returns.",
    usage: r#"
let actions = [ui_kit::TreeAction { icon: "plus", hover_only: false, label: "Add", reason: None, target: "scene:add" }];
let row = ui_kit::tree_row(ui, ui.scope_id().with(id), &ui_kit::Row {
    label: "Objects", icon: "body", depth: 0, fold: Some(open), eye: Some(true), actions: &actions, ..Default::default()
});
if row.clicked { /* select */ }
if row.fold { /* fold or unfold */ }
"#,
    dos: &["Give a disabled action its `reason`: it shows on hover.", "Use `hover_only` for actions that would clutter every row."],
    donts: &["Don't change your data inside the row's drawing: act on what it returns."],
    demo: |ui, d| {
        state(ui, "Header with lenses", |ui| {
            if ui_kit::tree_header(ui, "Scene", &mut d.lens, &["By object", "By look"], "Search objects") {
                d.said(format!("lens {}", d.lens));
            }
        });
        state(ui, "Rows (hover them)", |ui| {
            let t = Tokens::get();
            let add = [ui_kit::TreeAction { icon: "plus", hover_only: false, label: "Add", reason: None, target: "gallery:tree:add" }];
            let row = ui_kit::tree_row(
                ui,
                ui.scope_id().with("objects"),
                &ui_kit::Row { label: "Objects", icon: "folder", fold: Some(d.checked), actions: &add, ..Default::default() },
            );
            if row.clicked || row.fold {
                d.checked = !d.checked;
                d.said(if d.checked { "unfolded Objects" } else { "folded Objects" });
            }
            if d.checked {
                ui_kit::tree_row(
                    ui,
                    ui.scope_id().with("boss"),
                    &ui_kit::Row {
                        depth: 1,
                        label: "Boss",
                        icon: "body",
                        swatch: Some(t.accent),
                        eye: Some(true),
                        secondary: "Anodized blue",
                        selected: true,
                        ..Default::default()
                    },
                );
                ui_kit::tree_row(
                    ui,
                    ui.scope_id().with("rib"),
                    &ui_kit::Row { depth: 1, label: "Rib", icon: "body", eye: Some(false), ..Default::default() },
                );
            }
            ui_kit::tree_row(
                ui,
                ui.scope_id().with("output"),
                &ui_kit::Row { label: "Output", icon: "render_image", subtitle: "1920 × 1080 · PNG", ..Default::default() },
            );
        });
        state(ui, "Disabled action (hover the +)", |ui| {
            let add = [ui_kit::TreeAction {
                icon: "plus",
                hover_only: false,
                label: "Add",
                reason: Some("Open a design first"),
                target: "gallery:tree:add_off",
            }];
            ui_kit::tree_row(
                ui,
                ui.scope_id().with("lights"),
                &ui_kit::Row { label: "Lights", icon: "render_light_point", actions: &add, ..Default::default() },
            );
        });
        disabled(ui, |ui| {
            ui_kit::tree_row(ui, ui.scope_id().with("off"), &ui_kit::Row { label: "Objects", icon: "folder", ..Default::default() });
        });
    },
};

pub const LIST_PICKER: Page = Page {
    id: "list-picker",
    title: "List picker and list rows",
    group: Group::Tables,
    items: &["list_picker", "list_row", "selection_field", "Selection"],
    what: "A selectable line in a list (this gallery's sidebar is one), a removable row in a short list, and a field that shows what is selected in the model.",
    usage: r#"
if ui_kit::list_picker(ui, "preset1", "Preset 1", selected).clicked() { /* select */ }
let remove = ui_kit::list_row(ui, "face3", "Face 3").clicked();
ui_kit::selection_field(ui, "2 faces", ui_kit::Selection { active: true, filled: true });
"#,
    dos: &["Use `selection_field` for a dialog's model input, so the user sees it is waiting for a pick."],
    donts: &["Don't draw selection with a custom fill colour: the kit's picker uses the theme's selection tokens."],
    demo: |ui, d| {
        state(ui, "List pickers", |ui| {
            ui.set_max_width(260.0);
            for (i, name) in ["Studio", "Outdoor", "Night"].iter().enumerate() {
                if ui_kit::list_picker(ui, &format!("preset{i}"), name, d.selected == i).clicked() {
                    d.selected = i;
                    d.said(*name);
                }
            }
        });
        state(ui, "Removable rows (hover them)", |ui| {
            ui.set_max_width(260.0);
            ui_kit::list_row(ui, "face3", "Face 3");
            ui_kit::list_row(ui, "face7", "Face 7");
        });
        state(ui, "Selection field: waiting, filled", |ui| {
            ui.set_max_width(260.0);
            ui_kit::selection_field(ui, "Select faces", ui_kit::Selection { active: true, filled: false });
            ui_kit::selection_field(ui, "2 faces", ui_kit::Selection { active: false, filled: true });
        });
        disabled(ui, |ui| {
            ui.set_max_width(260.0);
            ui_kit::list_picker(ui, "preset_off", "Studio", true);
        });
    },
};

pub const REPORTS: Page = Page {
    id: "reports",
    title: "Report list",
    group: Group::Tables,
    items: &["report_list", "report_header", "report_card", "list_frame", "empty_box", "path_chip", "report_divider"],
    what: "Shared tables for comparable results, and individual diagnostic cards with quiet actions for crash reports.",
    usage: r#"
let columns = [ui_kit::Column::new("Report", 1.0).flex(), ui_kit::Column::new("Actions", 0.0).icon_only()];
ui_kit::report_list(ui, "reports", 160.0, &columns, "No reports.", |ui, table| {
    ui_kit::report_header(ui, table, "r1", "2026-10-18 21:04 · Internal error ×3", |ui| { ui_kit::bin_button(ui, "Delete"); });
    ui_kit::text(ui, "solid.fillet · replayable", ui_kit::Text::MonoNote("r1"));
});
let summary = ui_kit::ReportCardSummary { title: "Fillet stopped unexpectedly", subtitle: "24 Nov, 09:30 · replayable", icon: "warning", ink: Tokens::get().form_error };
let fields = [("version", "Version", "0.49.0 · abc1234"), ("command", "Command", "solid.fillet"), ("system", "System", "Windows · x86_64")];
let details = ui_kit::ReportCardDetails { fields: &fields, code: "index out of bounds\nblends.rs:418" };
ui_kit::report_card(ui, "crash", summary, details, &mut expanded, "Copy path", |action, response| { /* handle response.clicked() */ });
ui_kit::empty_box(ui, "none", "No crash reports.");
"#,
    dos: &["Say plainly in the empty box that there is nothing, and why that is good."],
    donts: &["Don't let a long list grow the dialog: give `report_list` a height."],
    demo: |ui, _| {
        state(ui, "Crash report cards", |ui| {
            ui_kit::path_chip(ui, "~/.local/share/solidcraft/reports", |ui| {
                ui_kit::button(ui, "Copy path", ui_kit::Button::Small);
            });
            ui.spacing_mut().item_spacing.y = Tokens::get().space_8;
            let t = Tokens::get();
            for (index, (key, title, subtitle, icon, ink, command, code)) in [
                (
                    "crash1",
                    "Fillet stopped unexpectedly",
                    "24 Nov, 09:30 · happened 3 times · replayable",
                    "warning",
                    t.form_error,
                    "solid.fillet",
                    "index out of bounds: the len is 4 but the index is 4\nblends.rs:418",
                ),
                (
                    "crash2",
                    "SolidCraft closed unexpectedly",
                    "23 Nov, 18:14 · while running Shell",
                    "stop",
                    t.warning,
                    "solid.shell",
                    "The previous SolidCraft session did not close cleanly",
                ),
                (
                    "crash3",
                    "Sketch offset failed, your design was restored",
                    "23 Nov, 16:02 · nothing was lost",
                    "shield",
                    t.form_success,
                    "sketch.offset",
                    "The kernel could not rebuild the offset",
                ),
                (
                    "crash4",
                    "Extrude stopped unexpectedly",
                    "22 Nov, 11:32 · replayable",
                    "warning",
                    t.form_error,
                    "solid.extrude",
                    "An internal error occurred while rebuilding Extrude2\nextrude.rs:205",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let id = ui.make_persistent_id(("crash_card", key));
                let mut expanded = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(index == 0);
                let fields = [("version", "Version", "0.49.0 · abc1234"), ("command", "Command", command), ("system", "System", "Windows · x86_64")];
                ui_kit::report_card(
                    ui,
                    key,
                    ui_kit::ReportCardSummary { title, subtitle, icon, ink },
                    ui_kit::ReportCardDetails { fields: &fields, code },
                    &mut expanded,
                    solidcraft_ui_text::tr("crash.copy_path"),
                    |_, _| {},
                );
                ui.data_mut(|data| data.insert_temp(id, expanded));
            }
        });
        state(ui, "Result table", |ui| {
            let columns = [ui_kit::Column::new("Report", 1.0).flex(), ui_kit::Column::new("Actions", 0.0).icon_only()];
            ui_kit::report_list(ui, "gallery_results", 140.0, &columns, "No reports.", |ui, table| {
                for (key, title) in [("check1", "No interference"), ("check2", "Everything is fully constrained")] {
                    ui_kit::report_header(ui, table, key, title, |ui| {
                        ui_kit::StageBar::icon_action(ui, "copy", "Copy");
                    });
                }
            });
        });
        state(ui, "Empty", |ui| {
            ui_kit::empty_box(ui, "gallery_none", "No crash reports.");
        });
    },
};
