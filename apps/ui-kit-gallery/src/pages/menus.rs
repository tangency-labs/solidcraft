//! Menus: the context menu and its rows, the workspace menu, rename, the marking menu, command
//! search and the popup list.

use super::{Group, Page};
use crate::state;
use egui::vec2;
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as kit;

pub const MENU: Page = Page {
    id: "menu",
    title: "Menu",
    group: Group::Menus,
    items: &["Menu", "MenuItem", "Item", "ItemIcon", "LIST_W", "menu_frame"],
    what: "The kit's menu card and its rows: icon, label, shortcut, a check, a colour swatch, a submenu arrow, a danger row. Context menus, the File menu and every flyout are built from it.",
    usage: r#"
ui_kit::Menu::show(ui, 268.0, |ui| {
    ui_kit::Menu::heading(ui, "Body");
    if ui_kit::MenuItem::new("body:props", "Properties", "measure").show(ui).clicked() { /* … */ }
    let mut hide = ui_kit::MenuItem::new("body:hide", "Hide", "eye");
    hide.shortcut = "H";
    hide.show(ui);
    ui_kit::Menu::separator(ui);
    let mut delete = ui_kit::MenuItem::new("body:delete", "Delete", "delete");
    delete.danger = true;
    delete.show(ui);
});
"#,
    dos: &[
        "Show the shortcut beside the command.",
        "Put destructive rows last, after a separator, with `danger`.",
        "Keep a row that does not apply visible and disabled.",
    ],
    donts: &[
        "Don't use `ui.menu_button` or `response.context_menu`: open the kit's `Menu` in a `Container` popup.",
        "Don't nest submenus more than one level.",
    ],
    demo: |ui, d| {
        state(ui, "Rows in every state (hover them)", |ui| {
            kit::Menu::show(ui, 268.0, |ui| {
                kit::Menu::heading(ui, "Body");
                if kit::MenuItem::new("gallery:props", "Properties", "measure").show(ui).clicked() {
                    d.said("Properties");
                }
                let mut item = kit::MenuItem::new("gallery:hide", "Hide", "eye");
                item.shortcut = "H";
                if item.show(ui).clicked() {
                    d.said("Hide");
                }
                item = kit::MenuItem::new("gallery:appearance", "Appearance", "appearance");
                item.submenu = true;
                item.show(ui);
                item = kit::MenuItem::new("gallery:paste", "Paste", "paste");
                item.enabled = false;
                item.shortcut = "Ctrl+V";
                item.show(ui);
                item = kit::MenuItem::new("gallery:origin", "Show origin", "origin");
                item.checked = Some(d.checked);
                if item.show(ui).clicked() {
                    d.checked = !d.checked;
                }
                item = kit::MenuItem::new("gallery:swatch", "Aluminium, satin", "appearance");
                item.swatch = Some(Tokens::get().control_border);
                item.show(ui);
                kit::Menu::separator(ui);
                item = kit::MenuItem::new("gallery:delete", "Delete", "delete");
                item.danger = true;
                item.shortcut = "Del";
                if item.show(ui).clicked() {
                    d.said("Delete");
                }
            });
        });
    },
};

pub const WORKSPACE_MENU: Page = Page {
    id: "workspace-menu",
    title: "Workspace menu",
    group: Group::Menus,
    items: &["workspace_menu", "WorkspaceMenu", "WorkspaceStyle", "Item"],
    what: "The workspace switcher's menu: each workspace with its accent, description and tabs, the current one marked and planned ones shown but unavailable.",
    usage: r#"
let items = [ui_kit::Item::action("ws.design", "Design", "ws_design").key("current"), /* … */];
let style = |item: &ui_kit::Item| Some(ui_kit::WorkspaceStyle { slug: "design", description: "…", tabs: Some("Solid · Surface"), ramp: None });
let chosen = ui_kit::workspace_menu(ui, &ui_kit::WorkspaceMenu { items: &items, style: &style, footer: "…" }, None, 480.0);
"#,
    dos: &["Mark the current workspace with `.key(\"current\")`."],
    donts: &["Don't list a planned workspace as if it worked: `.on(false)`."],
    demo: |ui, d| {
        let items = [
            kit::Item::action("sample.design", "Design", "ws_design").key("current"),
            kit::Item::action("sample.render", "Render", "ws_render"),
            kit::Item::action("sample.drawing", "Drawing", "ws_drawing").on(false),
        ];
        let style = |item: &kit::Item| {
            let (slug, description, tabs, ramp) = match item.label.as_str() {
                "Design" => ("design", "Sketch, model and assemble", Some("Solid · Surface · Sheet metal"), None),
                "Render" => ("render", "Lights, looks and pictures", Some("Scene · Shots"), Some(solidcraft_theme::AccentRamp::Render)),
                _ => ("drawing", "Coming later", None, None),
            };
            Some(kit::WorkspaceStyle { slug, description, tabs, ramp })
        };
        if let Some(item) = kit::workspace_menu(
            ui,
            &kit::WorkspaceMenu { items: &items, style: &style, footer: "Every workspace opens the same design." },
            None,
            460.0,
        ) {
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
        if let Some(keep) = kit::rename_menu(ui, &mut d.rename, &mut focused) {
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
            kit::Item::action("sample.repeat", "Repeat", "redo").key("R"),
            kit::Item::action("sample.delete", "Delete", "delete").key("Del"),
            kit::Item::action("sample.sketch", "Sketch", "sketch"),
            kit::Item::action("sample.extrude", "Extrude", "extrude"),
            kit::Item::action("sample.fillet", "Fillet", "fillet"),
            kit::Item::action("sample.measure", "Measure", "measure"),
            kit::Item::action("sample.undo", "Undo", "undo"),
            kit::Item::action("sample.move", "Move/Copy", "move"),
        ];
        if let Some(item) = kit::radial_menu(ui, rect.center(), &items, None) {
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
        kit::CommandSearch::field(
            ui,
            &mut d.search,
            false,
            true,
            true,
            kit::SearchFieldGeometry { height: t.control_height + t.space_12, radius: kit::CommandSearch::field_radius(1.0, true), progress: 1.0 },
            Some("S"),
            4096,
        );
        ui.add_space(t.space_6);
        let query = if d.search.is_empty() { "fillet".to_owned() } else { d.search.clone() };
        kit::CommandSearch::sheet(ui, ui.available_width(), 1.0, false, |ui| {
            kit::CommandSearch::heading(ui, "search.commands", 2);
            let mut row = kit::MenuItem::new("sample.fillet", "Fillet", "fillet");
            row.highlighted = true;
            row.shortcut = "F";
            row.search_row(ui, &query, "Solid › Modify", None);
            let row = kit::MenuItem::new("sample.sketch_fillet", "Sketch fillet", "sketch_fillet");
            row.search_row(ui, &query, "Sketch › Modify", None);
            kit::CommandSearch::detail(ui, "radius  6 mm");
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
        let anchor = ui.allocate_space(vec2(240.0, kit::row_height())).1;
        ui.painter().rect_stroke(anchor, t.control_radius, egui::Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
        kit::popup_list(
            ui.ctx(),
            ui.scope_id().with("popup"),
            anchor,
            Some(0),
            &[
                kit::PopupOption { kind: "parameter", text: "Width", detail: "40 mm", color: t.text },
                kit::PopupOption { kind: "parameter", text: "Height", detail: "20 mm", color: t.text },
                kit::PopupOption { kind: "function", text: "sqrt", detail: "square root", color: t.text_dim },
            ],
            "Choose",
        );
        ui.allocate_space(vec2(ui.available_width(), kit::row_height() * 5.0));
    },
};
