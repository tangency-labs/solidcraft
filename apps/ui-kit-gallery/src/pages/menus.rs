//! Menus: the context menu and its rows, the workspace menu, rename, the marking menu, command
//! search and the popup list.

use super::{Group, Page};
use crate::state;
use egui::vec2;
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as ui_kit;

pub const MENU: Page = Page {
    id: "menu",
    title: "Menu",
    group: Group::Menus,
    items: &["Menu", "Item", "ItemIcon", "MenuAnchor", "MenuOutput"],
    what: "One recursive definition and renderer for context menus, button menus, File and every flyout. The shared component owns density, placement, keyboard navigation, scrolling and dismissal.",
    usage: r#"
let items = [
    ui_kit::Item::action("body:props", "Properties", "measure"),
    ui_kit::Item::action("body:hide", "Hide", "eye").key("H"),
    ui_kit::Item::submenu("export", "Export", "export", vec![
        ui_kit::Item::action("export:step", "STEP...", "export"),
        ui_kit::Item::action("export:stl", "STL...", "mesh_export"),
    ]),
    ui_kit::Item::sep(),
    ui_kit::Item::action("body:delete", "Delete", "delete"),
];
if let Some(picked) = ui_kit::Menu::context_items(&response, &items) {
    dispatch(&picked.id, picked.params);
}
// A button opens the exact same element:
let picked = ui_kit::Menu::button_items(&button_response, &items);
"#,
    dos: &[
        "Define rows with Item and dispatch the returned action outside the component.",
        "Use stable action IDs; labels can be translated independently.",
        "Put destructive rows last, after a separator, and keep unavailable actions visible but disabled.",
    ],
    donts: &[
        "Don't build menu rows in a UI closure or own menu sizing, focus, hover or dismissal at the call site.",
        "Don't create a second menu renderer for navigation, File, overflow or a particular workspace.",
    ],
    demo: |ui, d| {
        state(ui, "One definition, every menu state", |ui| {
            let [r, g, b, _] = Tokens::get().control_border.to_array();
            let mut swatch = ui_kit::Item::action("gallery:swatch", "Aluminium, satin", "appearance");
            swatch.swatch = Some([r, g, b]);
            let items = [
                ui_kit::Item::heading("Body"),
                ui_kit::Item::action("gallery:props", "Properties", "measure"),
                ui_kit::Item::action("gallery:hide", "Hide", "eye").key("H"),
                ui_kit::Item::submenu("gallery:appearance", "Appearance", "appearance", vec![swatch]),
                ui_kit::Item::action("gallery:paste", "Paste", "paste").key("Ctrl+V").on_if(false),
                ui_kit::Item::action("gallery:origin", "Show origin", "origin").checked(d.checked),
                ui_kit::Item::sep(),
                ui_kit::Item::action("gallery:delete", "Delete", "delete").key("Del"),
            ];
            if let Some(item) = ui_kit::Menu::items(ui, ui.scope_id().with("gallery_menu"), &items, false).picked {
                if item.id == "gallery:origin" {
                    d.checked = !d.checked;
                }
                d.said(item.label);
            }
        });
    },
};

pub const WORKSPACE_MENU: Page = Page {
    id: "workspace-menu",
    title: "Workspace menu",
    group: Group::Menus,
    items: &["Menu", "Item"],
    what: "The workspace switcher supplies the same Item definition as any other menu. Current and unavailable workspaces use the shared checked and disabled states.",
    usage: r#"
let items = [
    ui_kit::Item::action("ws.design", "Design", "ws_design").checked(true),
    ui_kit::Item::action("ws.render", "Render", "ws_render"),
    ui_kit::Item::action("ws.drawing", "Drawing", "ws_drawing").on_if(false),
];
let chosen = ui_kit::Menu::button_items(&workspace_button, &items);
"#,
    dos: &["Mark the current workspace with checked state and keep its action ID stable."],
    donts: &["Don't give workspace menus a separate card or grid renderer."],
    demo: |ui, d| {
        let items = [
            ui_kit::Item::action("sample.design", "Design", "ws_design").checked(true),
            ui_kit::Item::action("sample.render", "Render", "ws_render"),
            ui_kit::Item::action("sample.drawing", "Drawing", "ws_drawing").on_if(false),
        ];
        if let Some(item) = ui_kit::Menu::items(ui, ui.scope_id().with("workspace_menu"), &items, false).picked {
            d.said(item.label);
        }
    },
};

pub const RENAME_MENU: Page = Page {
    id: "rename-menu",
    title: "Rename",
    group: Group::Menus,
    items: &["rename_menu"],
    what: "The small inline editor that renames a thing in a menu or list. Enter keeps the name, Escape cancels.",
    usage: r#"
match ui_kit::rename_menu(ui, &mut name, &mut focused) {
    Some(true) => commit(&name),   // Enter
    Some(false) => cancel(),       // Escape
    None => {}
}
"#,
    dos: &["Select the old name when it opens so typing replaces it."],
    donts: &["Don't rename on every keystroke: commit on Enter."],
    demo: |ui, d| {
        let mut focused = false;
        ui.set_max_width(260.0);
        if let Some(keep) = ui_kit::rename_menu(ui, &mut d.rename, &mut focused) {
            d.said(if keep { format!("renamed to {}", d.rename) } else { "cancelled".into() });
        }
    },
};

pub const RADIAL_MENU: Page = Page {
    id: "radial-menu",
    title: "Marking menu",
    group: Group::Menus,
    items: &["radial_menu", "RADIAL_RADIUS"],
    what: "The eight-slot marking menu that opens around the pointer for the commands used most. It returns the slot chosen.",
    usage: r#"
let chosen: Option<ui_kit::Item> = ui_kit::radial_menu(ui, centre, &items, hot_slot);
"#,
    dos: &["Keep each slot's command in the same place, so the gesture becomes habit."],
    donts: &["Don't put more than eight items in it."],
    demo: |ui, d| {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(380.0), 360.0), egui::Sense::hover());
        let items = [
            ui_kit::Item::action("sample.repeat", "Repeat", "redo").key("R"),
            ui_kit::Item::action("sample.delete", "Delete", "delete").key("Del"),
            ui_kit::Item::action("sample.sketch", "Sketch", "sketch"),
            ui_kit::Item::action("sample.extrude", "Extrude", "extrude"),
            ui_kit::Item::action("sample.fillet", "Fillet", "fillet"),
            ui_kit::Item::action("sample.measure", "Measure", "measure"),
            ui_kit::Item::action("sample.undo", "Undo", "undo"),
            ui_kit::Item::action("sample.move", "Move/Copy", "move"),
        ];
        if let Some(item) = ui_kit::radial_menu(ui, rect.center(), &items, None) {
            d.said(item.label);
        }
    },
};

pub const COMMAND_SEARCH: Page = Page {
    id: "command-search",
    title: "Command search",
    group: Group::Menus,
    items: &["CommandSearch", "SearchFieldGeometry", "SearchBadge"],
    what: "The app bar's command search: its field and the sheet of results, each with its place in the ribbon and its shortcut.",
    usage: r#"
ui_kit::CommandSearch::field(ui, &mut query, focused, open, enabled, geometry, Some("S"), 4096);
ui_kit::CommandSearch::sheet(ui, width, 1.0, false, |ui| {
    ui_kit::CommandSearch::heading(ui, "search.commands", 1);
    row.search_row(ui, &query, "Solid › Modify", None);
});
"#,
    dos: &["Show where the command lives, so the search teaches the ribbon."],
    donts: &["Don't open a second search for the same commands."],
    demo: |ui, d| {
        let t = Tokens::get();
        ui.set_max_width(420.0);
        ui_kit::CommandSearch::field(
            ui,
            &mut d.search,
            false,
            true,
            true,
            ui_kit::SearchFieldGeometry {
                height: t.control_height + t.space_12,
                radius: ui_kit::CommandSearch::field_radius(1.0, true),
                progress: 1.0,
            },
            Some("S"),
            4096,
        );
        ui.add_space(t.space_6);
        let query = if d.search.is_empty() { "fillet".to_owned() } else { d.search.clone() };
        ui_kit::CommandSearch::sheet(ui, ui.available_width(), 1.0, false, |ui| {
            ui_kit::CommandSearch::heading(ui, "search.commands", 2);
            let mut row = ui_kit::MenuItem::new("sample.fillet", "Fillet", "fillet");
            row.highlighted = true;
            row.shortcut = "F";
            row.search_row(ui, &query, "Solid › Modify", None);
            let row = ui_kit::MenuItem::new("sample.sketch_fillet", "Sketch fillet", "sketch_fillet");
            row.search_row(ui, &query, "Sketch › Modify", None);
            ui_kit::CommandSearch::detail(ui, "radius  6 mm");
        });
    },
};

pub const POPUP_LIST: Page = Page {
    id: "popup-list",
    title: "Popup list",
    group: Group::Menus,
    items: &["popup_list", "PopupOption"],
    what: "The completion list under an expression field: parameters, functions and units, each with its kind and value.",
    usage: r#"
let picked = ui_kit::popup_list(ui.ctx(), id, field_rect, Some(0), &[
    ui_kit::PopupOption { kind: "parameter", text: "Width", detail: "40 mm", color: t.text },
], "Choose");
"#,
    dos: &["Show the value beside each parameter."],
    donts: &["Don't take the keyboard from the field: the list follows the arrow keys the field passes it."],
    demo: |ui, _| {
        let t = Tokens::get();
        ui.set_max_width(320.0);
        let anchor = ui.allocate_space(vec2(240.0, ui_kit::row_height())).1;
        ui.painter().rect_stroke(anchor, t.control_radius, egui::Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
        ui_kit::popup_list(
            ui.ctx(),
            ui.scope_id().with("popup"),
            anchor,
            Some(0),
            &[
                ui_kit::PopupOption { kind: "parameter", text: "Width", detail: "40 mm", color: t.text },
                ui_kit::PopupOption { kind: "parameter", text: "Height", detail: "20 mm", color: t.text },
                ui_kit::PopupOption { kind: "function", text: "sqrt", detail: "square root", color: t.text_dim },
            ],
            "Choose",
        );
        ui.allocate_space(vec2(ui.available_width(), ui_kit::row_height() * 5.0));
    },
};
