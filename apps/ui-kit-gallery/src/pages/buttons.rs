//! Buttons: text buttons and their kinds, icon buttons, menu buttons, dialog footers, keys.

use super::{Group, Page};
use crate::{disabled, state};
use egui::vec2;
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as kit;

pub const BUTTON: Page = Page {
    id: "button",
    title: "Button",
    group: Group::Buttons,
    items: &["button", "Button", "primary_button", "wide_primary_button", "button_at", "native_button_focus"],
    what: "A push button. `Button` says its kind: the dialog's main action (Primary), an ordinary one (Normal), a small one in a toolbar or row (Small), a destructive one (DangerOutlined) and a quiet one (Ghost).",
    usage: r#"
if ui_kit::button(ui, "Add", ui_kit::Button::Normal).clicked() { /* … */ }
ui_kit::button(ui, "Replace", ui_kit::Button::DangerOutlined);
ui_kit::button(ui, "Apply", ui_kit::Button::Enabled(changed)); // greyed while there is nothing to apply
ui_kit::primary_button(ui, "Export");
"#,
    dos: &[
        "Have one primary button per dialog, the action the dialog is for.",
        "Label buttons with a verb: Export, Replace, Add parameter.",
        "Keep a disabled button visible, and say why on hover where it is not obvious.",
    ],
    donts: &[
        "Don't use `egui::Button` or `ui.button`: they miss the kit's colours, focus ring and handles.",
        "Don't put two primary buttons side by side.",
    ],
    demo: |ui, d| {
        state(ui, "Kinds (hover and press them)", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, kind) in [
                    ("Primary", kit::Button::Primary),
                    ("Normal", kit::Button::Normal),
                    ("Small", kit::Button::Small),
                    ("Selected", kit::Button::SmallSelected),
                    ("Ghost", kit::Button::Ghost),
                    ("Delete", kit::Button::DangerOutlined),
                ] {
                    if kit::button(ui, label, kind).clicked() {
                        d.said(format!("{label} clicked"));
                    }
                }
            });
        });
        state(ui, "Wide primary", |ui| {
            kit::wide_primary_button(ui, "Export");
        });
        state(ui, "Enabled(false)", |ui| {
            kit::button(ui, "Apply", kit::Button::Enabled(false));
        });
        disabled(ui, |ui| {
            ui.horizontal(|ui| {
                kit::button(ui, "Primary", kit::Button::Primary);
                kit::button(ui, "Normal", kit::Button::Normal);
            });
        });
    },
};

pub const ICON_BUTTONS: Page = Page {
    id: "icon-buttons",
    title: "Icon buttons",
    group: Group::Buttons,
    items: &[
        "icon_label_button",
        "compact_icon_label_button",
        "icon_action",
        "compact_button",
        "icon_button_at",
        "quiet_icon_button_at",
        "mirrored_icon_button_at",
    ],
    what: "Buttons drawn as an icon, with or without a label: row actions, toolbars, the small up and down arrows of a list. The icon names come from the installed icon set.",
    usage: r#"
ui_kit::icon_label_button(ui, "Copy path", "copy", true, false);   // enabled, danger
ui_kit::icon_action(ui, "Delete", "delete", true, true);
ui_kit::compact_button(ui, "▲", "Move up", enabled, selected);
ui_kit::icon_button_at(ui, rect, "settings", "Settings");            // at a rect you laid out
"#,
    dos: &["Every icon-only button has a label: it is the hover text and the accessible name.", "Use the danger flag for actions that delete."],
    donts: &[
        "Don't add a tiny + where a row can be added: tables use `add_prompt`.",
        "Don't draw an icon with the painter and sense clicks on it yourself.",
    ],
    demo: |ui, d| {
        state(ui, "With labels", |ui| {
            ui.horizontal(|ui| {
                if kit::icon_label_button(ui, "Copy path", "copy", true, false).clicked() {
                    d.said("Copy path");
                }
                kit::compact_icon_label_button(ui, "Export", "export", true);
                kit::icon_action(ui, "Delete", "delete", true, true);
            });
        });
        state(ui, "Icon only (hover for the label)", |ui| {
            let t = Tokens::get();
            let size = vec2(t.control_height, t.control_height);
            ui.horizontal(|ui| {
                let rect = ui.allocate_exact_size(size, egui::Sense::hover()).0;
                if kit::icon_button_at(ui, rect, "settings", "Settings").clicked() {
                    d.said("Settings");
                }
                let rect = ui.allocate_exact_size(size, egui::Sense::hover()).0;
                kit::quiet_icon_button_at(ui, rect, "eye", "Hide");
                let rect = ui.allocate_exact_size(size, egui::Sense::hover()).0;
                kit::mirrored_icon_button_at(ui, rect, "undo", "Redo");
                kit::compact_button(ui, "▲", "Move up", true, false);
                kit::compact_button(ui, "▼", "Move down", true, true);
            });
        });
        disabled(ui, |ui| {
            ui.horizontal(|ui| {
                kit::icon_label_button(ui, "Copy path", "copy", false, false);
                kit::compact_button(ui, "▲", "Move up", false, false);
            });
        });
    },
};

pub const TEXT_BUTTONS: Page = Page {
    id: "text-buttons",
    title: "Text and menu buttons",
    group: Group::Buttons,
    items: &["text_button", "command_menu_button", "action_menu", "icon_label_action", "selection_actions"],
    what: "A link-like text button for a secondary action inside text, a menu row that runs a command with its icon and shortcut, and a button that opens a small menu of actions.",
    usage: r#"
ui_kit::text_button(ui, "Show all", true);
ui_kit::command_menu_button(ui, "save", "Save", "save", "Ctrl+S", true); // id, label, icon, shortcut, enabled
ui_kit::action_menu(ui, "Actions", |ui| { ui_kit::button(ui, "Duplicate", ui_kit::Button::Normal); });
"#,
    dos: &["Show the shortcut in command menus so people learn it."],
    donts: &["Don't use a text button for the dialog's main action."],
    demo: |ui, d| {
        state(ui, "Text button", |ui| {
            if kit::text_button(ui, "Show all", true).clicked() {
                d.said("Show all");
            }
        });
        state(ui, "Command menu button", |ui| {
            ui.set_max_width(260.0);
            kit::command_menu_button(ui, "save", "Save", "save", "Ctrl+S", true);
            kit::command_menu_button(ui, "export", "Export…", "export", "", true);
        });
        state(ui, "Action menu (open it)", |ui| {
            kit::action_menu(ui, "Actions", |ui| {
                kit::button(ui, "Duplicate", kit::Button::Normal);
                kit::button(ui, "Rename", kit::Button::Normal);
            });
        });
        disabled(ui, |ui| {
            kit::text_button(ui, "Show all", false);
            ui.set_max_width(260.0);
            kit::command_menu_button(ui, "save_off", "Save", "save", "Ctrl+S", false);
        });
    },
};

pub const FOOTER: Page = Page {
    id: "footer",
    title: "Dialog footer",
    group: Group::Buttons,
    items: &["footer", "Footer", "Action", "button_pair", "PairButton"],
    what: "The OK and Cancel row at the foot of a dialog, with Apply and a status when asked. It returns which one was chosen; Enter and Escape choose OK and Cancel.",
    usage: r#"
match ui_kit::footer(ui, "OK", "Cancel", valid, ui_kit::Footer::default()) {
    ui_kit::Action::Ok => { /* run the command */ }
    ui_kit::Action::Cancel => { /* close */ }
    _ => {}
}
ui_kit::footer(ui, "OK", "Cancel", true, ui_kit::Footer { apply: Some("Apply"), nothing_to_apply: !changed, ..Default::default() });
"#,
    dos: &[
        "Disable OK while the input is invalid, and say what is wrong above it.",
        "Let Apply be disabled until something changed (`nothing_to_apply`).",
    ],
    donts: &["Don't lay out OK and Cancel yourself: the order and keys are the kit's.", "Don't close a dialog on OK before the command succeeded."],
    demo: |ui, d| {
        state(ui, "OK and Cancel", |ui| {
            let action = kit::footer(ui, "OK", "Cancel", true, kit::Footer::default());
            if action != kit::Action::None {
                d.said(format!("{action:?}"));
            }
        });
        state(ui, "With Apply", |ui| {
            kit::footer(ui, "OK", "Cancel", true, kit::Footer { apply: Some("Apply"), ..Default::default() });
        });
        state(ui, "Computing", |ui| {
            kit::footer(ui, "OK", "Cancel", true, kit::Footer { status: Some("Computing…"), ..Default::default() });
        });
        state(ui, "Invalid input (OK disabled)", |ui| {
            kit::footer(ui, "OK", "Cancel", false, kit::Footer::default());
        });
        state(ui, "Button pair", |ui| {
            kit::button_pair(
                ui,
                "gallery_pair",
                [false, true],
                [
                    kit::PairButton { key: "look_at", label: "Look at", icon: Some("lookat") },
                    kit::PairButton { key: "slice", label: "Slice", icon: None },
                ],
            );
        });
    },
};

pub const KEYBOARD: Page = Page {
    id: "keyboard",
    title: "Keyboard",
    group: Group::Buttons,
    items: &["keyboard", "install_keyboard_focus", "select_all_text", "cell_key", "land_in_cell"],
    what: "The kit's keyboard rules: Escape cancels and Enter submits a valid dialog (`keyboard`); Tab into a text box selects its text so typing replaces it (`install_keyboard_focus`); Enter in a table cell moves down a row (`cell_key`).",
    usage: r#"
ui_kit::install_keyboard_focus(ctx);                 // once per context, cheap every frame
match ui_kit::keyboard(ui.ctx(), ok_enabled) {      // in a dialog without a `footer`
    ui_kit::Action::Ok => run(),
    ui_kit::Action::Cancel => close(),
    _ => {}
}
"#,
    dos: &["Let every dialog close on Escape.", "Install the focus plugin in every context that draws kit text boxes."],
    donts: &["Don't make Enter run a destructive action without a confirmation."],
    demo: |ui, d| {
        kit::install_keyboard_focus(ui.ctx());
        state(ui, "Press Tab to move between the boxes: the text is selected", |ui| {
            kit::text_row(ui, "First", &mut d.text, "");
            kit::text_row(ui, "Second", &mut d.rename, "");
        });
        state(ui, "Press Enter or Escape with nothing focused", |ui| {
            let action = kit::keyboard(ui.ctx(), true);
            if action != kit::Action::None {
                d.said(format!("{action:?}"));
            }
            kit::text(ui, "The result shows below.", kit::Text::Note("keys"));
        });
    },
};
