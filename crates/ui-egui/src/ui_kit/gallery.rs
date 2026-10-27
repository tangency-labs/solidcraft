//! Table-driven layout/paint references in both themes. No window or GPU required.
use super::*;
use crate::ui_kit::{Container, Surface};
use crate::{scenario, theme::Tokens};
use egui::{Ui, vec2};

struct Sample {
    value: String,
    second: String,
    selected: usize,
    checked: bool,
}

struct Element {
    name: &'static str,
    rect: &'static str,
    handle: Option<&'static str>,
    draw: fn(&mut Ui, &mut Sample),
}

macro_rules! element {
    ($name:literal, $rect:literal, $handle:expr, $draw:expr) => {
        Element { name: $name, rect: $rect, handle: $handle, draw: $draw }
    };
    ($name:literal, $draw:expr) => {
        element!($name, "gallery:allocation", None, $draw)
    };
}

// One entry per element/variant: names, rendering and geometry expectations stay together.
const ELEMENTS: &[Element] = &[
    element!("sphere_thumbnail", |ui, _| {
        ui.horizontal(|ui| {
            for kind in [FinishKind::Matte, FinishKind::Satin, FinishKind::Gloss, FinishKind::Metal, FinishKind::Glass] {
                sphere_thumbnail(
                    ui,
                    &format!("gallery:sphere:{kind:?}"),
                    Sphere { color: Tokens::get().render_accent, kind, opacity: None },
                    Tokens::get().space_24 * 2.0,
                );
            }
        });
    }),
    element!("look_chain", "gallery:chain", Some("gallery:chain:body"), |ui, _| {
        let t = Tokens::get();
        look_chain(
            ui,
            "gallery:chain",
            ["Face", "Body", "Component", "Material"],
            [
                None,
                Some(Sphere { color: t.render_accent, kind: FinishKind::Satin, opacity: None }),
                None,
                Some(Sphere { color: t.control_border, kind: FinishKind::Metal, opacity: None }),
            ],
            LookStep::Body,
        );
    }),
    element!("look_chain_empty", |ui, _| {
        look_chain(ui, "gallery:empty-chain", ["Face", "Body", "Component", "Material"], [None; 4], LookStep::Face);
    }),
    element!("settings_form", |ui, state| {
        settings_form(ui, Tokens::get().form_label_width, |ui| {
            row(ui, "Sample", |ui| {
                field(ui, &mut state.value);
            });
        });
    }),
    element!("settings_composition", |ui, _| {
        settings_heading(ui, "Interface", "Appearance and workspace");
        settings_section(ui, "gallery_settings", "Layout", |ui| {
            settings_pair(
                ui,
                |ui| {
                    text(ui, "Controls", Text::Inline);
                },
                |ui, ()| layout_preview(ui, LayoutPreview { compact: true, labels: false, history: 2, translucent: false }),
            )
        });
    }),
    element!("settings_permission_choices", |ui, state| {
        settings_choices(
            ui,
            &[
                SettingsChoice {
                    key: "gallery:ask",
                    title: "Ask first",
                    summary: "Confirm changes before they run.",
                    detail: "Full permission details",
                },
                SettingsChoice {
                    key: "gallery:edits",
                    title: "Allow edits",
                    summary: "Ask before running other tools.",
                    detail: "Full permission details",
                },
                SettingsChoice {
                    key: "gallery:all",
                    title: "Allow all",
                    summary: "Run supported tools without asking.",
                    detail: "Full permission details",
                },
            ],
            &mut state.selected,
        );
        settings_pointer_preview(ui, true, "Normal", true);
    }),
    element!("preference_theme_rail", |ui, _| {
        theme_rail(
            ui,
            |ui| {
                for label in ["Light", "Dark", "System"] {
                    option_card(
                        ui,
                        label,
                        Some("Built in"),
                        label == "Light",
                        true,
                        PreferenceFigure::Theme { palette: Tokens::light().into(), system: label == "System" },
                    );
                }
            },
            |ui| {
                theme_import_tile(ui, "Import…", "theme file");
            },
        );
    }),
    element!("preference_option_cards", |ui, _| {
        ui.horizontal_wrapped(|ui| {
            for figure in [
                PreferenceFigure::Theme { palette: Tokens::light().into(), system: true },
                PreferenceFigure::Pivot(2),
                PreferenceFigure::Quality(24),
                PreferenceFigure::Antialiasing(8),
                PreferenceFigure::ContactShading(true),
                PreferenceFigure::UpAxis(true),
            ] {
                option_card(ui, "Choice", Some("Example"), true, true, figure);
            }
        });
    }),
    element!("preference_specimens", |ui, _| {
        tab_accent_strip(ui);
        scale_specimen(ui, 1.1);
        status_palette_sample(ui, crate::theme::ColorVision::Deutan);
        colour_vision_sample(ui, &Tokens::get());
        settings_autosave_timeline(ui, 5.0);
        settings_axis_row(ui, "Push X", 0.6, [0.1, 0.2], |_| {}, |_| {}, |_| {});
        settings_mouse_mapping(ui, &[(vec!["Shift".into(), "Middle drag".into()], "Orbit".into())], |_| {});
    }),
    element!("assistant_entry", |ui, _| {
        let t = Tokens::get();
        egui::Frame::NONE.fill(t.app_bar).show(ui, |ui| {
            ui.horizontal(|ui| {
                for state in [
                    AssistantEntryState::Quiet,
                    AssistantEntryState::Ready,
                    AssistantEntryState::Busy,
                    AssistantEntryState::NeedsAuth,
                    AssistantEntryState::Stopped,
                    AssistantEntryState::Waiting(3),
                ] {
                    let (_, response) = ui.allocate_exact_size(vec2(t.space_24 + t.space_8, t.space_24 + t.space_8), egui::Sense::click());
                    assistant_entry(ui, &response, state, false, false);
                }
            });
        });
    }),
    element!("assistant_header", |ui, _| {
        assistant_header(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Assistant");
                assistant_badge(ui, "Ready", AssistantTone::Success);
            });
        });
    }),
    element!("assistant_controls", |ui, _| {
        assistant_controls(ui, |ui| {
            button(ui, "Connect", Button::Normal);
        });
    }),
    element!("assistant_badge", |ui, _| {
        ui.horizontal(|ui| {
            for (label, tone) in [
                ("Ready", AssistantTone::Success),
                ("Working", AssistantTone::Working),
                ("Waiting", AssistantTone::Warning),
                ("Error", AssistantTone::Error),
                ("Offline", AssistantTone::Neutral),
            ] {
                assistant_badge(ui, label, tone);
            }
        });
    }),
    element!("assistant_bubble", |ui, _| {
        assistant_bubble(ui, |ui| {
            ui.label("Set the plate thickness to 8 mm.");
        });
    }),
    element!("assistant_prose", |ui, _| {
        assistant_prose(ui, |ui| {
            ui.label("The thickness is now 8 mm. The design rebuilt successfully.");
        });
    }),
    element!("assistant_tool_status", |ui, _| {
        ui.horizontal(|ui| {
            for state in [AssistantToolState::Pending, AssistantToolState::Running, AssistantToolState::Done, AssistantToolState::Failed] {
                assistant_tool_status(ui, state);
            }
        });
    }),
    element!("assistant_tool", |ui, _| {
        assistant_tool_frame(ui, false, |ui| {
            ui.monospace("parameters.set");
            assistant_tool_detail(ui, |ui| {
                ui.monospace("thickness = 8 mm");
            });
        });
    }),
    element!("assistant_tool_failed", |ui, _| {
        assistant_tool_frame(ui, false, |ui| {
            assistant_tool_row(ui, true, |ui| {
                ui.monospace("solid.fillet");
            });
        });
    }),
    element!("assistant_approval", |ui, _| {
        assistant_approval(ui, "Allow the Assistant to run this?", |ui| {
            ui.label("Change the design parameter.");
            assistant_actions(ui, |ui| {
                primary_button(ui, "Allow once");
                button(ui, "Always allow", Button::Normal);
                button(ui, "Deny", Button::Normal);
            });
        });
    }),
    element!("assistant_composer", |ui, s| {
        assistant_composer_footer(ui, |ui| {
            assistant_composer(ui, false, |ui| {
                assistant_input(ui, ui.id().with("assistant_gallery"), &mut s.value, "Ask for a change…");
            });
        });
    }),
    element!("assistant_composer_focused", |ui, _| {
        assistant_composer(ui, true, |ui| {
            ui.label("Ask for a change…");
        });
    }),
    element!("assistant_send", |ui, _| {
        ui.horizontal(|ui| {
            assistant_send(ui, true, false, "Send");
            assistant_send(ui, true, true, "Stop");
            assistant_send(ui, false, false, "Send");
        });
    }),
    element!("assistant_empty", |ui, _| {
        assistant_empty(ui, "Set up an assistant", "Choose an installed agent in Preferences.", |ui| {
            button(ui, "Open Preferences", Button::Normal);
        });
    }),
    element!("assistant_banner", |ui, _| {
        assistant_banner(ui, AssistantTone::Warning, |ui| {
            ui.label("Paused after the configured step limit.");
        });
    }),
    element!("history_transport", |ui, _| {
        ui.horizontal(|ui| {
            let side = Tokens::get().space_24;
            for control in HistoryTransport::ALL {
                let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
                control.show(ui, rect);
            }
        });
    }),
    element!("history_marker", |ui, _| {
        let t = Tokens::get();
        let accent = crate::theme::TabAccent::for_tab(&t, "SURFACE");
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_24 * 7.0, t.space_12), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.panel);
            history_marker(ui.painter(), rect, true, accent);
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_12, t.control_height), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.timeline);
            history_marker(ui.painter(), rect, false, accent);
        });
    }),
    element!("history_seam_rest", |ui, _| history_seam_example(ui, HistoryHandleState::Rest)),
    element!("history_seam_hover", |ui, _| history_seam_example(ui, HistoryHandleState::Hover)),
    element!("history_seam_drag", |ui, _| history_seam_example(ui, HistoryHandleState::Drag)),
    element!("history_icons", |ui, _| history_example(ui, false)),
    element!("history_names", |ui, _| history_example(ui, true)),
    element!("file_navigation", |ui, _| {
        FileView::navigation(ui, |ui| {
            FileView::back_to(ui, "gallery:file:back", "Back to {name}", "Sample Plate", "Esc");
            FileView::nav_row(ui, "gallery:file:start", "Start", "", true, true);
            FileView::nav_row(ui, "gallery:file:open", "Open", "Ctrl+O", false, true);
            FileView::separator(ui);
            FileView::nav_row(ui, "gallery:file:save", "Save", "Ctrl+S", false, false);
        });
    }),
    element!("file_action", |ui, _| {
        FileView::action_tile(ui, "gallery:file:new", ui.available_width(), "new", "New design", "Millimetres · opens in Design", true);
    }),
    element!("file_recovery_banner", |ui, _| {
        FileView::recovery_banner(
            ui,
            "gallery:recovery",
            super::file_view::RecoveryMessage {
                title: "1 design was recovered",
                before_filename: " after SolidCraft closed unexpectedly: ",
                filename: "enclosure-lid.solidcraft",
                after_filename: ", 2 minutes ago.",
            },
            "Recover",
            Some("Discard"),
        );
    }),
    element!("file_recovery_review", |ui, _| {
        FileView::recovery_review(ui, "gallery:recovery-review", "Sample Plate", " · 14 features · 2 minutes ago", "Recover", "Discard");
    }),
    element!("file_recent_list", |ui, _| {
        FileView::recent_list(ui, |ui| {
            for id in ["first", "second"] {
                FileView::recent_row(
                    ui,
                    id,
                    RecentRow {
                        title: "Sample Plate",
                        path: "Designs/sample-plate.solidcraft",
                        when: "Open now",
                        workspace: "Design",
                        thumbnail: None,
                        thumbnail_failed: false,
                    },
                );
            }
        });
    }),
    element!("file_recent", |ui, _| {
        FileView::recent_row(
            ui,
            "gallery:file:recent",
            RecentRow {
                title: "Sample Plate",
                path: "Designs/sample-plate.solidcraft",
                when: "Open now",
                workspace: "Design",
                thumbnail: None,
                thumbnail_failed: false,
            },
        );
    }),
    element!("file_sample", |ui, _| {
        FileView::sample_grid(
            ui,
            "gallery:file:sample",
            &[SampleCard { title: "Sample Plate", description: "Parametric plate with holes and a bored boss", icon: "new", thumbnail: None }],
        );
    }),
    element!("file_recovery", |ui, _| {
        FileView::recovery_banner(
            ui,
            "gallery:recovery-summary",
            super::file_view::RecoveryMessage { title: "2 designs have unsaved changes", before_filename: "", filename: "", after_filename: "" },
            "Review",
            None,
        );
    }),
    element!("command_search", |ui, _| {
        let mut query = "fillet".to_owned();
        CommandSearch::field(
            ui,
            &mut query,
            false,
            true,
            true,
            SearchFieldGeometry {
                height: Tokens::get().control_height + Tokens::get().space_12,
                radius: CommandSearch::field_radius(1.0, true),
                progress: 1.0,
            },
            4096,
        );
        ui.add_space(Tokens::get().space_6);
        CommandSearch::sheet(ui, ui.available_width(), 1.0, false, |ui| {
            CommandSearch::heading(ui, "search.commands", 1);
            let c = solidcraft_engine::find_command("solid.fillet").unwrap();
            let mut row = MenuItem::new(c.id, c.label, c.icon);
            row.highlighted = true;
            row.shortcut = "F";
            row.search_row(ui, &query, "MODEL › MODIFY", None);
            CommandSearch::detail(ui, "radius  6 mm");
        });
    }),
    element!("menu", |ui, _| {
        Menu::show(ui, 268.0, |ui| {
            Menu::heading(ui, "Body");
            MenuItem::new("gallery:normal", "Properties", "measure").show(ui);
            let mut item = MenuItem::new("gallery:hover", "Hide", "eye");
            item.highlighted = true;
            item.shortcut = "H";
            item.show(ui);
            item = MenuItem::new("gallery:open", "Appearance", "appearance");
            item.submenu = true;
            item.highlighted = true;
            item.show(ui);
            item = MenuItem::new("gallery:disabled", "Paste", "paste");
            item.enabled = false;
            item.shortcut = "Ctrl+V";
            item.show(ui);
            item = MenuItem::new("gallery:checked", "Show Origin", "origin");
            item.checked = Some(true);
            item.show(ui);
            item.checked = Some(false);
            item.id = "gallery:unchecked";
            item.show(ui);
            item = MenuItem::new("gallery:swatch", "Aluminum - Satin", "appearance");
            item.swatch = Some(Tokens::get().control_border);
            item.show(ui);
            Menu::separator(ui);
            item = MenuItem::new("gallery:danger", "Delete", "delete");
            item.danger = true;
            item.shortcut = "Del";
            item.show(ui);
        });
    }),
    element!("menu_submenu", |ui, _| {
        ui.horizontal_top(|ui| {
            Menu::show(ui, 244.0, |ui| {
                let mut item = MenuItem::new("parent", "Appearance", "appearance");
                item.submenu = true;
                item.highlighted = true;
                item.show(ui);
            });
            Menu::show(ui, 236.0, |ui| {
                Menu::heading(ui, "Appearance");
                let mut item = MenuItem::new("look", "Aluminum - Satin", "appearance");
                item.swatch = Some(Tokens::get().control_border);
                item.show(ui);
            });
        });
    }),
    element!("rename_menu", |ui, s| {
        rename_menu(ui, &mut s.value, &mut false);
    }),
    element!("radial_menu", |ui, _| {
        let (r, _) = ui.allocate_exact_size(vec2(360.0, 360.0), egui::Sense::hover());
        let mut app = crate::test_support::sample_app();
        app.ui.tab = "SURFACE".into();
        crate::theme::TabAccent::publish(&app, ui.ctx());
        radial_menu(ui, r.center(), &crate::context_menu::radial_items(&app), Some(3));
    }),
    element!("workspace_menu", |ui, _| {
        let app = crate::test_support::sample_app();
        workspace_menu(ui, &crate::context_menu::items(&app, &crate::context_menu::Target::Workspace), None, 550.0);
    }),
    element!("window", "dialog:footer", Some("dialog:ok"), |ui, _| {
        Window::new(ui.id().with("window"), "settings", "Preferences").content(
            ui,
            true,
            |ui| {
                text(ui, "General", Text::Report("window", false));
            },
            |ui| {
                button(ui, "Restore defaults", Button::Normal);
            },
        );
    }),
    element!("window_pages", "dialog:pages", Some("gallery:page"), |ui, s| {
        panel::page_list(
            ui,
            &mut s.selected,
            &[WindowPage {
                group: Some("prefs.input"),
                icon: "settings",
                label: "ui.general",
                handle: "gallery:page",
                scroll: true,
                draw: |ui, _: &mut ()| {
                    text(ui, "General", Text::Inline);
                },
            }],
            &mut (),
        );
    }),
    element!("confirm_warning", "dialog:footer", Some("dialog:apply"), |ui, _| {
        Confirm::new(ui.id().with("confirm"), "status_warning", "Save changes?")
            .tone(Tone::Warning)
            .actions("Save", "Cancel", Some("Don't Save"))
            .content(ui, true, |ui| {
                text(ui, "Your changes will be lost.", Text::Report("message", false));
            });
    }),
    element!("confirm_danger", "dialog:footer", Some("dialog:ok"), |ui, _| {
        Confirm::new(ui.id().with("confirm"), "delete", "Delete")
            .tone(Tone::Danger)
            .detail("Fillet2 · Chamfer1")
            .actions("Delete", "Cancel", None)
            .content(ui, true, |ui| {
                text(ui, "These features will be deleted:", Text::Report("message", false));
            });
    }),
    element!("confirm_info", "dialog:footer", Some("dialog:ok"), |ui, _| {
        Confirm::new(ui.id().with("confirm"), "help", "Replace file?").actions("Replace", "Cancel", None).content(ui, true, |ui| {
            text(ui, "A file with this name already exists.", Text::Report("message", false));
        });
    }),
    element!("mono_note", |ui, _| {
        text(ui, "solid.fillet · replayable", Text::MonoNote("sample"));
    }),
    element!("color_thumbnail", |ui, _| {
        color_thumbnail(ui, ui.id().with("thumb"), Tokens::get().accent, "Sample", ());
    }),
    element!("list_frame", |ui, _| {
        list_frame(ui, "sample", |ui| {
            text(ui, "Sample", Text::Cell);
        });
    }),
    element!("segmented_primary", |ui, s| {
        segmented(ui, &mut s.selected, &["U8", "F"], Segments { primary: true, ..Default::default() });
    }),
    element!("color_popover", |ui, _| {
        color_popover(ui, &mut [204, 208, 214]);
    }),
    element!("report_list", |ui, _| {
        report_list(ui, "sample", 120.0, |ui| {
            report_header(ui, "Internal error", 3, "2026-10-18 21:04:12", "Report", |ui| {
                bin_button(ui, "Delete");
            });
            text(ui, "solid.fillet · replayable", Text::MonoNote("sample"));
        });
    }),
    element!("path_chip", |ui, _| {
        path_chip(ui, "/reports", |ui| {
            button(ui, "Copy path", Button::Normal);
        });
    }),
    element!("empty_box", |ui, _| {
        empty_box(ui, "empty", "No crash reports.");
    }),
    element!("report_header", |ui, _| {
        report_header(ui, "Internal error", 3, "2026-10-18 21:04:12", "Report", |ui| {
            bin_button(ui, "Delete");
        });
    }),
    element!("parameter_import", |ui, _| {
        icon_label_button(ui, "Import…", "import_tray", true, false);
    }),
    element!("parameter_export_csv", |ui, _| {
        icon_label_button(ui, "Export CSV…", "export_tray", true, false);
    }),
    element!("parameter_export_json", |ui, _| {
        icon_label_button(ui, "Export JSON…", "export_tray", true, false);
    }),
    element!("icon_label_button", |ui, _| {
        icon_label_button(ui, "Copy path", "copy", true, false);
    }),
    element!("section_count", |ui, _| {
        section_count(ui, "count", "Parameters", false, Some(5), |_| {});
    }),
    element!("bin_button", |ui, _| {
        bin_button(ui, "Delete");
    }),
    element!("active_pill", |ui, _| {
        active_pill(ui);
    }),
    element!("library_cell", |ui, s| {
        library_cell(ui, ui.id().with("library"), &mut s.value, "Default material", |ui| {
            button(ui, "Steel", Button::Normal);
        });
    }),
    element!("add_prompt", "dialog:table:add_prompt:add", Some("dialog:table:add_prompt:add"), |ui, _| {
        table(ui, "add_prompt", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.add_prompt(ui, "Add configuration", "Add configuration");
        });
    }),
    element!("removable_header", |ui, _| {
        ui.allocate_ui(vec2(140.0, row_height()), |ui| {
            removable_header(ui, "Width");
        });
    }),
    element!("plus_header", |ui, _| {
        ui.allocate_ui(vec2(140.0, row_height()), |ui| {
            ui.set_min_size(vec2(140.0, row_height()));
            plus_header(ui, |_| {});
        });
    }),
    element!("table_custom", |ui, _| {
        table_custom(ui, "custom", &[Column::new("Name", 1.0)], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            table.add_prompt(ui, "Add parameter", "Add parameter");
        });
    }),
    element!("table_row_height", |ui, _| {
        ui.allocate_space(vec2(table_row_height(), table_row_height()));
    }),
    element!("summary_row", |ui, s| {
        summary_row(ui, "Rule", "Drives width = 120 mm", &mut s.checked, ContentWidth::button(ui, "Edit"), |ui| {
            button(ui, "Edit", Button::Normal);
        });
    }),
    element!("compact_button", |ui, _| {
        compact_button(ui, "▲", "Move up", true, false);
    }),
    element!("error", |ui, _| {
        text(ui, "Invalid expression", Text::Error("gallery"));
    }),
    element!("table_flex", |ui, s| {
        table(
            ui,
            "flex",
            &[
                Column::new("Name", 1.0).flex().fit_content(ContentWidth::label(ui, "Width")).resizable(row_height()),
                Column::new("Expression", 1.5).flex().resizable(row_height()),
                Column::new("Actions", 0.0).icon_only(),
            ],
            "Empty",
            |ui, table| {
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| {
                        text(ui, "Width", Text::Cell);
                    });
                    row.cell(|ui| {
                        cell_editor(ui, ui.id().with("expression"), &mut s.value, "Expression", false);
                    });
                    row.cell(|ui| {
                        bin_button(ui, "Delete");
                    });
                });
                table.add_prompt(ui, "Add parameter", "Add parameter");
            },
        );
    }),
    element!("cell_name", |ui, _| {
        text(ui, "Width", Text::NameCell(false));
    }),
    element!("cell_name_strong", |ui, _| {
        text(ui, "Default", Text::NameCell(true));
    }),
    element!("text_heading", |ui, _| {
        text(ui, "Configuration rules", Text::Heading("gallery"));
    }),
    element!("cell_mono", |ui, _| {
        text(ui, "mm", Text::MonoCell);
    }),
    element!("cell_value", |ui, _| {
        text(ui, "80.0000 mm", Text::Value("gallery_value", false));
    }),
    element!("cell_value_error", |ui, _| {
        text(ui, "Unknown parameter", Text::Value("gallery_value_error", true));
    }),
    element!("scope", |ui, _| {
        scope(ui, |ui| {
            ui.allocate_space(vec2(row_height(), row_height()));
        });
    }),
    element!("row_height", |ui, _| {
        ui.allocate_space(vec2(row_height(), row_height()));
    }),
    element!("header_height", |ui, _| {
        ui.allocate_space(vec2(header_height(), header_height()));
    }),
    element!("slider_integer_without_editor", |ui, _| {
        slider(ui, &mut 12.0, 0.0..=100.0, Slider { show_value: false, integer: true, ..Default::default() });
    }),
    element!("canvas_value_field", |ui, s| {
        canvas_value_field(ui, ui.id().with("canvas_value"), &mut s.value, false);
    }),
    element!("placement_field", |ui, s| {
        placement_field(ui, ui.id().with("placement"), &mut s.value, "Distance");
    }),
    element!("bounded_cell_editor", |ui, s| {
        bounded_cell_editor(ui, ui.id().with("name"), &mut s.value, "Name", false, 128);
    }),
    element!("password_field", |ui, s| {
        password_field(ui, ui.id().with("secret"), &mut s.value, "API key");
    }),
    element!("number_field", |ui, _| {
        number_field(ui, &mut 3_u32, 1..=200, 1.0, "");
    }),
    element!("slider_percent", |ui, _| {
        slider(ui, &mut 1.0, 0.5..=2.5, Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() });
    }),
    element!("dropdown_menu", |ui, _| {
        dropdown_menu(ui, "gallery_menu", "Preset", |ui| {
            list_picker(ui, "preset", "Preset", true);
        });
    }),
    element!("dropdown_menu_with_close_behavior", |ui, s| {
        dropdown_menu_with_close_behavior(ui, "gallery_editable_menu", "Preset", egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            text_field(ui, &mut s.value, "Filter…");
            if list_picker(ui, "preset", "Preset", true).clicked() {
                ui.close();
            }
        });
    }),
    element!("message_field", |ui, s| {
        message_field(ui, ui.id().with("message"), &mut s.value, "Message");
    }),
    element!("text_lines", |ui, _| {
        text_lines(ui, "gallery", "Licence\nSecond line");
    }),
    element!("report_divider", "dialog:divider:gallery_report", None, |ui, _| {
        report_list(ui, "gallery_report", row_height() * 3.0, |ui| {
            text(ui, "Report message", Text::Report("gallery_report", false));
            report_divider(ui, "gallery_report");
        });
    }),
    element!("table", "dialog:table:gallery:row", None, |ui, s| {
        table(ui, "gallery", &[Column::new("Name", 1.0), Column::new("Expression", 1.5)], "No rows", |ui, table| {
            table.row(ui, "row", true, |row| {
                row.cell(|ui| {
                    text(ui, "Width", Text::Cell);
                });
                row.cell(|ui| {
                    cell_editor(ui, ui.id().with("value"), &mut s.value, "", false);
                });
            });
        });
    }),
    element!("list_picker", "dialog:list:gallery", Some("dialog:list:gallery"), |ui, _| {
        list_picker(ui, "gallery", "Preset", true);
    }),
    element!("inline_text", "dialog:inline:Active", Some("dialog:inline:Active"), |ui, _| {
        text(ui, "Active", Text::Inline);
    }),
    element!("cell_text", "dialog:cell:Width", Some("dialog:cell:Width"), |ui, _| {
        text(ui, "Width", Text::Cell);
    }),
    element!("label_actions", |ui, _| {
        label_actions(ui, "Configuration", |ui| {
            button(ui, "Apply", Button::Normal);
        });
    }),
    element!("text", "dialog:report:gallery", Some("dialog:report:gallery"), |ui, _| {
        text(ui, "Validation report with retained units: 10 mm", Text::Report("gallery", false));
    }),
    element!("search_field", |ui, s| {
        search_field(ui, ui.id().with("search"), &mut s.value, "Search");
    }),
    element!("cell_editor", |ui, s| {
        cell_editor(ui, ui.id().with("cell"), &mut s.value, "Expression", true);
    }),
    element!("color_field", |ui, _| {
        color_field(ui, &mut [120, 150, 180]);
    }),
    element!("slider", |ui, _| {
        slider(ui, &mut 0.5, 0.0..=1.0, Default::default());
    }),
    element!("slider_disabled", |ui, _| {
        ui.add_enabled_ui(false, |ui| slider(ui, &mut 0.5, 0.0..=1.0, Default::default()));
    }),
    element!("scalar_field", |ui, _| {
        scalar_field(ui, &mut 0.5, 0.0..=1.0);
    }),
    element!("length_field", |ui, _| {
        length_field(ui, &mut 12.7, "in", 2);
    }),
    element!("bounded_length_field", |ui, _| {
        bounded_length_field(ui, &mut 12.7, "in", 2, 0.0..=25.4, 0.254);
    }),
    element!("angle_field", |ui, _| {
        angle_field(ui, &mut 45.0);
    }),
    element!("text_area", |ui, s| {
        text_area(ui, "Gallery", ui.id().with("area"), &mut s.value, 5, 1024);
    }),
    element!("color_swatch", |ui, _| {
        color_swatch(ui, ui.id().with("swatch"), Tokens::get().accent, true, "Sample", 1);
    }),
    element!("swatch_grid", |ui, _| {
        swatch_grid(ui, &[1, 2, 3], |ui, n| {
            color_swatch(ui, ui.id().with(n), Tokens::get().accent, true, "Sample", *n);
        });
    }),
    element!("action_menu", |ui, _| {
        action_menu(ui, "Actions", |ui| {
            button(ui, "Add", Button::Normal);
        });
    }),
    element!("enabled_button", |ui, _| {
        button(ui, "Add", Button::Enabled(true));
    }),
    element!("header", "dialog:header", Some("dialog:close"), |ui, _| {
        header(ui, "fillet", "Fillet", || {});
    }),
    element!("disclosure", |ui, _| {
        let t = Tokens::get();
        for open in [true, false] {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
            disclosure(ui.painter(), rect.center(), open, t.form_dim);
        }
    }),
    element!("disclosure_button_at", |ui, _| {
        let t = Tokens::get();
        for open in [true, false] {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
            ui.push_id(open, |ui| disclosure_button_at(ui, rect, open, crate::i18n::tr_label(if open { "Collapse" } else { "Expand" })));
        }
    }),
    element!("panel_disclosure_button_at", |ui, _| {
        let t = Tokens::get();
        for open in [true, false] {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
            ui.push_id(open, |ui| panel_disclosure_button_at(ui, rect, open, crate::i18n::tr_label(if open { "Collapse" } else { "Expand" })));
        }
    }),
    element!("dropdown_indicator", |ui, _| {
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(t.form_section_icon_size, t.form_section_icon_size), egui::Sense::hover());
        dropdown_indicator(ui.painter(), rect, t.form_dim);
    }),
    element!("collapsible_header_closed", "dialog:header", Some("dialog:collapse"), |ui, _| {
        collapsible_header(ui, "fillet", "Fillet", &mut true, || {});
    }),
    element!("section_open", "dialog:section:Advanced", Some("dialog:section:Advanced"), |ui, _| {
        section(ui, "gallery_open", "Advanced", true, |_| {});
    }),
    element!("collapsible_header", "dialog:header", Some("dialog:collapse"), |ui, _| {
        collapsible_header(ui, "fillet", "Fillet", &mut false, || {});
    }),
    element!("row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            field(ui, &mut s.value);
        });
    }),
    element!("expr_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        expr_row(ui, "Gallery", &mut s.value);
    }),
    element!("count_row", "dialog:row:Gallery", Some("dialog:Gallery up"), |ui, _| {
        let mut count = "6".to_owned();
        count_row(ui, "Gallery", &mut count, &Count { min: 1, max: 10, eval: &|text: &str| text.trim().parse().ok() });
    }),
    element!("count_field", "dialog:row:Gallery", Some("dialog:Gallery down"), |ui, _| {
        row(ui, "Gallery", |ui| {
            let mut count = "1".to_owned();
            count_field(ui, &mut count, &Count { min: 1, max: 10, eval: &|text: &str| text.trim().parse().ok() });
        });
    }),
    element!("hint_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        hint_row(ui, "Gallery", &mut s.value, "Value");
    }),
    element!("text_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        text_row(ui, "Gallery", &mut s.value, "Name");
    }),
    element!("choice_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        choice_row(ui, "Gallery", "gallery", &mut s.selected, &["One", "Two"]);
    }),
    element!("multiline_text_field", "dialog:widget:Gallery", Some("dialog:Gallery"), |ui, s| {
        multiline_text_field(ui, "Gallery", ui.id().with("text"), &mut s.value);
    }),
    element!("searchable_dropdown", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            searchable_dropdown(ui, "gallery", "One", &mut String::new(), &[SearchOption { value: "one", label: "One", handle: None }], None, None);
        });
    }),
    element!("text_emphasis", "dialog:row:Gallery", Some("dialog:Bold"), |ui, s| {
        row(ui, "Gallery", |ui| {
            text_emphasis(ui, &mut s.checked, &mut false);
        });
    }),
    element!("value_pair_named", "dialog:row:Position", Some("dialog:Distance X"), |ui, s| {
        value_pair(ui, "Position", [&mut s.value, &mut s.second], ["X", "Y"], Some(["Distance X", "Distance Y"]));
    }),
    element!("inline_name", |ui, s| {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_height()), egui::Sense::hover());
        inline_name(ui, rect, ui.id().with("inline_name"), &mut s.value);
    }),
    element!("field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            field(ui, &mut s.value);
        });
    }),
    element!("focused_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            focused_field(ui, &mut s.value);
        });
    }),
    element!("hint_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            hint_field(ui, &mut s.value, "Length");
        });
    }),
    element!("text_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            text_field(ui, &mut s.value, "Name");
        });
    }),
    element!("focused_text_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            focused_text_field(ui, &mut s.value, "Name");
        });
    }),
    element!("fraction_field", "dialog:row:Gallery", Some("dialog:fraction:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            fraction_field(ui, &mut 0.5);
        });
    }),
    element!("dropdown", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            dropdown(ui, "gallery", &mut s.selected, &["One", "Two"]);
        });
    }),
    element!("selection_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            selection_field(ui, "Point 1", Selection { active: true, filled: true });
        });
    }),
    element!("selection_field_empty", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            selection_field(ui, "Select", Selection { active: false, filled: false });
        });
    }),
    element!("segmented", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented(ui, &mut s.selected, &["One", "Two"], Segments::default());
        });
    }),
    element!("segmented_compact", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented(ui, &mut s.selected, &["Left", "Center", "Right"], Segments { compact: true, ..Default::default() });
        });
    }),
    element!("segmented_states", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented(ui, &mut s.selected, &["One", "Two"], Segments { states: &[true, true], ..Default::default() });
        });
    }),
    element!("segmented_icons", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented_icons(ui, &mut s.selected, &[IconSegment { key: "normal", icon: "normal", tip: "Normal".into() }], &[]);
        });
    }),
    element!("checkbox", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            checkbox(ui, "Option", &mut s.checked);
        });
    }),
    element!("checkbox_off", |ui, _| {
        checkbox(ui, "Option", &mut false);
    }),
    element!("checkbox_disabled_on", |ui, _| {
        ui.add_enabled_ui(false, |ui| checkbox(ui, "Option", &mut true));
    }),
    element!("checkbox_disabled_off", |ui, _| {
        ui.add_enabled_ui(false, |ui| checkbox(ui, "Option", &mut false));
    }),
    element!("checkbox_indeterminate", |ui, _| {
        checkbox_indeterminate(ui, "Option", &mut false, true);
    }),
    element!("checkbox_disabled_mixed", |ui, _| {
        ui.add_enabled_ui(false, |ui| checkbox_indeterminate(ui, "Option", &mut false, true));
    }),
    element!("checkbox_focused", |ui, _| {
        egui::Frame::new().inner_margin(Tokens::get().space_4).show(ui, |ui| {
            checkbox(ui, "Option", &mut true).request_focus();
        });
    }),
    element!("checkbox_focused_off", |ui, _| {
        egui::Frame::new().inner_margin(Tokens::get().space_4).show(ui, |ui| {
            checkbox(ui, "Option", &mut false).request_focus();
        });
    }),
    element!("checkbox_focused_mixed", |ui, _| {
        egui::Frame::new().inner_margin(Tokens::get().space_4).show(ui, |ui| {
            checkbox_indeterminate(ui, "Option", &mut false, true).request_focus();
        });
    }),
    element!("checkbox_hovered", |ui, _| {
        checkbox(ui, "Option", &mut false);
    }),
    element!("checkbox_hovered_on", |ui, _| {
        checkbox(ui, "Option", &mut true);
    }),
    element!("checkbox_hovered_mixed", |ui, _| {
        checkbox_indeterminate(ui, "Option", &mut false, true);
    }),
    element!("command_menu_button", |ui, _| {
        command_menu_button(ui, "save", "Save", "save", "Ctrl+S", true);
    }),
    element!("primary_button", "dialog:row:Gallery", Some("dialog:button:Create"), |ui, _| {
        row(ui, "Gallery", |ui| {
            primary_button(ui, "Create");
        });
    }),
    element!("wide_primary_button", |ui, _| {
        wide_primary_button(ui, "Finish Sketch");
    }),
    element!("button", "dialog:row:Gallery", Some("dialog:button:Gallery:Add"), |ui, _| {
        row(ui, "Gallery", |ui| {
            button(ui, "Add", Button::Normal);
        });
    }),
    element!("button_at", |ui, _| {
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(t.space_24 * 5.0, t.file_action_height), egui::Sense::hover());
        button_at(ui, "gallery:button_at", rect, "Recover", Button::Primary);
    }),
    element!("small_button", "dialog:row:Gallery", Some("dialog:button:Gallery:×"), |ui, _| {
        row(ui, "Gallery", |ui| {
            button(ui, "×", Button::Small);
        });
    }),
    element!("checkbox_row", "sketch_palette:toggle:gallery", Some("sketch_palette:toggle:gallery"), |ui, _| {
        checkbox_row(ui, "gallery", "Option", true);
    }),
    element!("checkbox_grid", "dialog:checkbox:First", Some("dialog:First"), |ui, s| {
        checkbox_grid(ui, &mut [("First", &mut s.checked), ("Second", &mut false)], 2);
    }),
    element!("options", "dialog:checkbox:First", Some("dialog:First"), |ui, s| {
        options(ui, "gallery", &mut [("First", &mut s.checked), ("Second", &mut false)]);
    }),
    element!("footer", "dialog:footer", Some("dialog:ok"), |ui, _| {
        footer(ui, "OK", "Cancel", true, Footer::default());
    }),
    element!("footer_apply", "dialog:footer", Some("dialog:apply"), |ui, _| {
        footer(ui, "OK", "Cancel", true, Footer { apply: Some("Apply"), ..Default::default() });
    }),
    element!("footer_status", "dialog:footer", Some("dialog:ok"), |ui, _| {
        footer(ui, "OK", "Cancel", true, Footer { status: Some("Computing…"), ..Default::default() });
    }),
    element!("button_pair", "sketch_palette:look_at", Some("sketch_palette:look_at"), |ui, _| {
        button_pair(
            ui,
            "sketch_palette",
            [false, true],
            [PairButton { key: "look_at", label: "ui.look_at", icon: Some("lookat") }, PairButton { key: "slice", label: "Slice", icon: None }],
        );
    }),
    element!("icon_button_at", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(row_height(), row_height()), egui::Sense::hover());
        icon_button_at(ui, rect, "close", "Close");
    }),
    element!("quiet_icon_button_at", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(row_height(), row_height()), egui::Sense::hover());
        quiet_icon_button_at(ui, rect, "plus", "Add shortcut");
    }),
    element!("keyboard", |ui, _| {
        assert_eq!(keyboard(ui.ctx(), true), Action::None);
        ui.allocate_space(vec2(row_height(), row_height()));
    }),
    element!("list_row", "dialog:list:picked", Some("dialog:list:picked:remove"), |ui, _| {
        list_row(ui, "picked", "Point 1");
    }),
    element!("value_pair", "dialog:row:Position", Some("dialog:Y"), |ui, s| {
        value_pair(ui, "Position", [&mut s.value, &mut s.second], ["X", "Y"], None);
    }),
    element!("toggle_icon", "dialog:Alignment:Center", Some("dialog:Alignment:Center"), |ui, s| {
        toggle_icon(ui, "Alignment", "Center", &mut s.checked, "pattern_center");
    }),
    element!("tool_choices", "sketch_palette:feature:line", Some("sketch_palette:feature:line"), |ui, _| {
        tool_choices(ui, "Line", "line", &[ToolChoice { key: "line", icon: "normal", label: "Line", enabled: true }]);
    }),
    element!("status_header", "sketch_palette:meter", None, |ui, _| {
        ui.spacing_mut().item_spacing.y = 0.0;
        status_header(
            ui,
            "sketch_palette",
            "Sketch 1",
            StatusHeader { icon: "sketch", text: "2 degrees of freedom", complete: false, fraction: 0.5 },
        );
    }),
    element!("progress", "dialog:progress:gallery", None, |ui, _| {
        progress(ui, "gallery", 0.4);
    }),
    element!("hint", "dialog:hint:gallery", Some("dialog:hint:gallery"), |ui, _| {
        text(ui, "Pick a point", Text::Hint("gallery"));
    }),
    element!("note", "dialog:note:gallery", None, |ui, _| {
        text(ui, "Counts include the original. Center balances copies; Flip chooses the extra side for even totals.", Text::Note("gallery"));
    }),
    element!("caption", "dialog:caption:Gallery", None, |ui, _| {
        caption(ui, "Gallery", "Construction");
    }),
    element!("figure_rows", "dialog:figure:gallery", Some("dialog:Gallery"), |ui, s| {
        let Sample { value, second, .. } = s;
        figure_rows(ui, "gallery", &mut [("Gallery", value), ("Second", second)], |painter, rect, slots| {
            let t = Tokens::get();
            painter.rect_stroke(rect.shrink(t.space_4), t.control_radius, egui::Stroke::new(1.0, t.form_dim), egui::StrokeKind::Inside);
            for slot in slots {
                let ink = if slot.hot { t.form_primary } else { t.form_dim };
                let y = slot.rect.center().y;
                painter.line_segment([egui::pos2(rect.center().x, y), egui::pos2(slot.rect.left(), y)], egui::Stroke::new(1.0, ink));
            }
        });
    }),
    element!("section", "dialog:section:Advanced", Some("dialog:section:Advanced"), |ui, _| {
        section(ui, "gallery", "Advanced", false, |_| {});
    }),
    element!("divider", "dialog:divider:gallery", None, |ui, _| {
        divider(ui, "gallery");
    }),
    element!("popup_list", |ui, _| {
        let anchor = ui.allocate_space(vec2(100.0, row_height())).1;
        popup_list(
            ui.ctx(),
            ui.id().with("popup"),
            anchor,
            Some(0),
            &[PopupOption { kind: "parameter", text: "Width", detail: "10 mm", color: Tokens::get().text }],
            "Choose",
        );
        // The popup is on a separate layer; reserve its painted allocation for the same checks.
        ui.allocate_space(vec2(ui.available_width(), row_height() * 5.0));
    }),
];

fn example(ui: &mut Ui, element: &Element) -> (&'static str, Option<&'static str>) {
    let mut sample = Sample { value: "12 mm".into(), second: "25 mm".into(), selected: 0, checked: true };
    let rect = ui.scope(|ui| (element.draw)(ui, &mut sample)).response.rect;
    if matches!(element.name, "menu" | "menu_submenu" | "rename_menu" | "workspace_menu" | "command_search" | "history_icons" | "history_names") {
        // A popup's elevation is painted outside its layout rectangle. Reserve those
        // actual shadow bounds as part of the gallery sample, keeping the paint guard strict.
        ui.expand_to_include_rect(rect + Menu::frame().shadow.margin());
    }
    scenario::publish_rect("gallery:allocation", rect);
    (element.rect, element.handle)
}

#[test]
fn every_element_has_geometry_and_stays_in_its_allocation_in_both_themes() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for element in ELEMENTS {
            let name = element.name;
            let ctx = crate::fonts::test_context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            for pass in 0..3 {
                let events = if element.name.starts_with("checkbox_hovered") && pass > 0 {
                    vec![egui::Event::PointerMoved(scenario::rect_of("gallery:allocation").expect("hover allocation").center())]
                } else {
                    vec![]
                };
                scenario::clear_handles();
                let mut allocation = egui::Rect::NOTHING;
                let mut published = ("", None);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 600.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(320.0);
                        allocation = ui
                            .scope(|ui| {
                                scope(ui, |ui| {
                                    published = example(ui, element);
                                })
                            })
                            .response
                            .rect;
                    },
                );
                output.textures_delta.clear();
                let rect = scenario::rect_of(published.0).unwrap_or_else(|| panic!("{name}: missing {}", published.0));
                assert!(rect.is_finite() && rect.is_positive(), "{name}: {rect:?}");
                assert!(allocation.expand(1.0).contains_rect(rect), "{name}: {rect:?} outside {allocation:?}");
                if let Some(handle) = published.1 {
                    assert!(
                        rect.contains(scenario::handle_at(handle).unwrap_or_else(|| panic!("{name}: missing {handle}"))),
                        "{name}: handle outside {rect:?}"
                    );
                }
                if published.0.starts_with("dialog:row:") {
                    assert_eq!(rect.height(), Tokens::get().control_height, "{name}");
                }
                for shape in output.shapes {
                    // Native icon-only buttons keep an empty text shape. It has no vertices;
                    // its sentinel bounds do not describe any painted pixels.
                    if matches!(&shape.shape, egui::Shape::Text(text) if text.galley.num_vertices == 0) {
                        continue;
                    }
                    let painted = shape.shape.visual_bounding_rect().intersect(shape.clip_rect);
                    if painted.is_positive() {
                        assert!(allocation.expand(1.0).contains_rect(painted), "{name}: paint {painted:?} outside {allocation:?}: {:?}", shape.shape);
                    }
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn apply_does_not_submit_and_obeys_validation() {
    for enabled in [false, true] {
        let ctx = crate::fonts::test_context();
        let mut action = Action::None;
        let mut draw = |ui: &mut Ui| {
            action = footer(ui, "OK", "Cancel", enabled, Footer { apply: Some("Apply"), ..Default::default() });
        };
        super::test_support::frame(&ctx, vec![], &mut draw);
        let at = scenario::handle_at("dialog:apply").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        assert_eq!(action, if enabled { Action::Apply } else { Action::None });
    }
}

#[test]
fn picked_item_remove_and_paired_values_have_independent_targets() {
    let ctx = crate::fonts::test_context();
    let mut removed = false;
    let mut draw = |ui: &mut Ui| {
        removed = list_row(ui, "point-1", "Point 1").clicked();
    };
    super::test_support::frame(&ctx, vec![], &mut draw);
    let at = scenario::handle_at("dialog:list:point-1:remove").unwrap();
    super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
    super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
    assert!(removed);

    let mut x = String::new();
    let mut y = String::new();
    let mut draw = |ui: &mut Ui| {
        value_pair(ui, "Position", [&mut x, &mut y], ["X", "Y"], None);
    };
    super::test_support::frame(&ctx, vec![], &mut draw);
    let row = scenario::rect_of("dialog:row:Position").unwrap();
    let first = scenario::handle_at("dialog:X").unwrap();
    let second = scenario::handle_at("dialog:Y").unwrap();
    assert_eq!(row.height(), Tokens::get().control_height);
    assert!(row.contains(first) && row.contains(second) && first.x < second.x);
    super::test_support::frame(&ctx, super::test_support::pointer(second, true), &mut draw);
    super::test_support::frame(&ctx, super::test_support::pointer(second, false), &mut draw);
    super::test_support::frame(&ctx, vec![egui::Event::Text("25".into())], &mut draw);
    assert_eq!(x, "");
    assert_eq!(y, "25");
}

#[test]
fn gallery_rows_share_a_label_column_after_measurement_in_both_themes() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let mut value = "3".to_owned();
        for _ in 0..3 {
            super::test_support::frame(&ctx, vec![], |ui| {
                row(ui, "Edges", |ui| {
                    selection_field(ui, "Edge 1", Selection { active: true, filled: true });
                });
                row(ui, "Counterbore depth", |ui| {
                    field(ui, &mut value);
                });
            });
        }
        let first = scenario::rect_of("dialog:control:Edges").unwrap();
        let second = scenario::rect_of("dialog:control:Counterbore depth").unwrap();
        assert_eq!(first.left(), second.left());
        assert_eq!(first.height(), Tokens::get().control_height);
        assert_eq!(second.height(), Tokens::get().control_height);
        let width = scenario::rect_of("dialog:label:Edges").unwrap().width();
        assert!(width >= Tokens::get().form_label_width && width <= Tokens::get().form_label_width * 1.6);
    }
    crate::theme::set_dark(previous);
}

#[test]
fn compound_row_actions_preserve_the_original_field_handle() {
    let ctx = crate::fonts::test_context();
    let mut value = "3 mm".to_owned();
    let mut expected = None;
    super::test_support::frame(&ctx, vec![], |ui| {
        row(ui, "Mid Radius", |ui| {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(Tokens::get().space_24 * 3.0, row_height()),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        field(ui, &mut value);
                    },
                );
                expected = scenario::handle_at("dialog:Mid Radius");
                button(ui, "×", Button::Small);
            });
        });
    });
    assert_eq!(scenario::handle_at("dialog:Mid Radius"), expected);
    assert!(expected.is_some());
    assert_ne!(scenario::handle_at("dialog:button:Mid Radius:×"), expected);
}

#[test]
fn emphasis_and_compact_modes_keep_independent_actions_in_both_themes() {
    for dark in [false, true] {
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut bold = false;
        let mut italic = false;
        let mut selected = 0;
        let mut draw = |ui: &mut Ui| {
            row(ui, "Style", |ui| text_emphasis(ui, &mut bold, &mut italic));
            row(ui, "Alignment", |ui| {
                segmented(
                    ui,
                    &mut selected,
                    &["Long translated left", "Long translated centre", "Long translated right"],
                    Segments { compact: true, ..Default::default() },
                );
            });
        };
        super::test_support::frame(&ctx, vec![], &mut draw);
        for handle in ["dialog:Bold", "dialog:Italic", "dialog:Alignment:Long translated centre"] {
            let at = scenario::handle_at(handle).unwrap();
            super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
            super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        }
        assert!(bold && italic);
        assert_eq!(selected, 1);
    }
}

#[test]
fn table_cells_keep_alignment_fixed_rows_editing_and_action_targets() {
    for dark in [false, true] {
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut first = "10 mm".to_owned();
        let mut second = "20 mm".to_owned();
        let mut removed = false;
        let mut draw = |ui: &mut Ui| {
            scope(ui, |ui| {
                table(ui, "cells", &[Column::new("Name", 1.0), Column::new("Expression", 1.5), Column::new("Actions", 0.7)], "Empty", |ui, table| {
                    for (name, value) in [("Width", &mut first), ("Height", &mut second)] {
                        table.row(ui, name, name == "Height", |row| {
                            row.cell(|ui| {
                                text(ui, name, Text::Cell);
                            });
                            row.cell(|ui| {
                                let response = cell_editor(ui, ui.id().with("expression"), value, "Expression", false);
                                scenario::publish_handle(&format!("gallery:cell:{name}"), response.rect.center());
                                scenario::publish_rect(&format!("gallery:cell:{name}"), response.rect);
                            });
                            row.cell(|ui| {
                                if button(ui, "Delete", Button::Small).clicked() {
                                    removed = true;
                                }
                            });
                        });
                    }
                });
            });
        };
        for _ in 0..3 {
            super::test_support::frame(&ctx, vec![], &mut draw);
        }
        let first_rect = scenario::rect_of("gallery:cell:Width").unwrap();
        let second_rect = scenario::rect_of("gallery:cell:Height").unwrap();
        let header = scenario::rect_of("dialog:column:cells:Expression").unwrap();
        assert_eq!(first_rect.left(), header.left());
        assert_eq!(second_rect.left(), header.left());
        assert_eq!(first_rect.height(), Tokens::get().control_height);
        assert_eq!(second_rect.height(), Tokens::get().control_height);
        let at = scenario::handle_at("gallery:cell:Height").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        super::test_support::frame(&ctx, vec![egui::Event::Text("x".into())], &mut draw);
        let delete = scenario::handle_at("dialog:button:Actions:Delete").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(delete, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(delete, false), &mut draw);
        assert_eq!(first, "10 mm");
        assert!(second.contains('x'));
        assert!(removed);
    }
}

#[test]
fn popup_suggestions_and_selectable_lists_keep_click_selection() {
    for dark in [false, true] {
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut selected = None;
        let mut draw = |ui: &mut Ui| {
            selected = popup_list(
                ui.ctx(),
                egui::Id::new("suggestions"),
                egui::Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(220.0, 24.0)),
                Some(0),
                &[
                    PopupOption { kind: "parameter", text: "width", detail: "Parameter · 10 mm", color: Tokens::get().text },
                    PopupOption { kind: "parameter", text: "height", detail: "Parameter · 20 mm", color: Tokens::get().text },
                ],
                "Choose",
            );
        };
        for _ in 0..3 {
            super::test_support::frame(&ctx, vec![], &mut draw);
        }
        let at = scenario::handle_at("dialog:suggestion:1").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        assert_eq!(selected, Some(1));
        let mut picked = false;
        let mut draw = |ui: &mut Ui| {
            scope(ui, |ui| {
                if list_picker(ui, "preset", "Preset", picked).clicked() {
                    picked = !picked;
                }
            });
        };
        super::test_support::frame(&ctx, vec![], &mut draw);
        let at = scenario::handle_at("dialog:list:preset").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        assert!(picked);
    }
}

#[test]
fn many_draggable_samples_wrap_inside_the_available_width() {
    let ctx = crate::fonts::test_context();
    let mut rects = Vec::new();
    let mut bounds = egui::Rect::NOTHING;
    super::test_support::frame(&ctx, vec![], |ui| {
        ui.set_width(Tokens::get().form_label_width * 2.0);
        scope(ui, |ui| {
            bounds = ui.available_rect_before_wrap();
            swatch_grid(ui, &(0..20).collect::<Vec<_>>(), |ui, index| {
                let response = color_swatch(ui, ui.id().with(index), Tokens::get().accent, false, "Sample", *index);
                rects.push(response.rect);
            });
        });
    });
    assert_eq!(rects.len(), 20);
    assert!(rects.iter().all(|rect| rect.right() <= bounds.right() && rect.width() == Tokens::get().control_height + Tokens::get().space_6));
    assert!(rects.last().unwrap().top() > rects.first().unwrap().top(), "samples must wrap to additional rows");
}

#[test]
fn gallery_covers_every_inline_element() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui_kit");
    let names: std::collections::BTreeSet<_> = ELEMENTS.iter().map(|e| e.name).collect();
    assert_eq!(names.len(), ELEMENTS.len(), "duplicate gallery entries");
    for file in [
        "actions.rs",
        "checks.rs",
        "choice.rs",
        "color.rs",
        "disclosure.rs",
        "fields.rs",
        "layout.rs",
        "look_chain.rs",
        "menu.rs",
        "metrics.rs",
        "radial.rs",
        "reports.rs",
        "sphere.rs",
        "status.rs",
        "table.rs",
    ] {
        let source = std::fs::read_to_string(root.join(file)).unwrap();
        let ast = syn::parse_file(&source).unwrap();
        for item in ast.items {
            if let syn::Item::Fn(item) = item
                && matches!(&item.vis, syn::Visibility::Restricted(v) if v.path.is_ident("crate"))
            {
                let name = item.sig.ident.to_string();
                // The inherited row label is metadata, not a renderable element.
                if name != "label" {
                    assert!(names.contains(name.as_str()), "{file}: {name} needs a gallery entry");
                }
            }
        }
    }
}

#[cfg(test)]
fn history_seam_example(ui: &mut Ui, state: HistoryHandleState) {
    let t = Tokens::get();
    let allocation = ui.allocate_space(vec2(280.0, 250.0)).1;
    for (names, offset) in [(false, 0.0), (true, 125.0)] {
        let width = if state == HistoryHandleState::Drag { 112.0 } else { history_width(names) };
        // Separate samples vertically so the guide/chip fits within the gallery allocation.
        let card = egui::Rect::from_min_size(allocation.min + vec2(t.space_8, offset + t.space_8), vec2(width, 104.0));
        ui.painter().rect(card, t.radius_lg, t.panel, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        HistorySeam::new(card).paint(ui.painter(), card, names, state);
    }
}

#[cfg(test)]
fn history_example(ui: &mut Ui, names: bool) {
    let mut app = crate::test_support::sample_app();
    app.preferences.timeline_left = true;
    app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
    app.run("timeline.roll_to", serde_json::json!({"position":3})).unwrap();
    let feature = app.session.doc.features.get(2).unwrap().id;
    app.run("select.set", serde_json::json!({"items":[{"type":"feature","id":feature}]})).unwrap();
    let width = crate::timeline_left::strip_width(&app);
    let rect = ui.allocate_space(vec2(width, 650.0)).1;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(5.0)), |ui| {
        ui.painter().add(Container::new(ui.ctx(), Surface::Panel).frame(ui.ctx(), Tokens::get().panel, crate::frame::Side::Left).paint(rect));
        crate::timeline_left::left_timeline(&mut app, ui);
    });
}

/// CPU captures of the actual vertical strip and browser, in both themes and widths.
#[test]
fn history_board_in_both_themes_and_widths() {
    use super::checks::tests::{raster, textures_update};
    use std::collections::HashMap;
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        for names in [false, true] {
            for collapsed in [false, true] {
                for state in [HistoryHandleState::Rest, HistoryHandleState::Hover, HistoryHandleState::Drag] {
                    crate::theme::set_dark(dark);
                    let ctx = crate::fonts::test_context();
                    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                    ctx.global_style_mut(|style| style.animation_time = 0.0);
                    let mut app = crate::test_support::sample_app();
                    app.preferences.timeline_left = true;
                    app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
                    app.preferences.timeline_left_browser_collapsed = collapsed;
                    app.run("timeline.roll_to", serde_json::json!({"position":3})).unwrap();
                    let feature = app.session.doc.features.get(2).unwrap().id;
                    app.run("select.set", serde_json::json!({"items":[{"type":"feature","id":feature}]})).unwrap();
                    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(560.0, 680.0));
                    let mut textures = HashMap::new();
                    let mut image = None;
                    let mut drag_start = None;
                    for pass in 0..9 {
                        let mut events = vec![];
                        if pass >= 2 {
                            let card = app.shell.history.unwrap();
                            if state == HistoryHandleState::Hover {
                                events.push(egui::Event::PointerMoved(card.right_center()));
                            } else if state == HistoryHandleState::Drag {
                                let start = *drag_start.get_or_insert(egui::pos2(card.right() - 4.0, card.top() + 12.0));
                                if pass == 2 {
                                    events = crate::test_support::pointer(start, true);
                                } else if pass == 3 {
                                    events.push(egui::Event::PointerMoved(start + vec2(112.0 - history_width(names), 0.0)));
                                }
                            }
                        }
                        let mut output = ctx.run_ui(
                            egui::RawInput { screen_rect: Some(bounds), time: Some(pass as f64 * 0.2), focused: true, events, ..Default::default() },
                            |ui| {
                                ui.painter().rect_filled(bounds, 0.0, Tokens::get().viewport_top);
                                crate::timeline_left::left_panel(&mut app, ui);
                                crate::browser::browser(&mut app, ui);
                            },
                        );
                        textures_update(&mut textures, &output.textures_delta);
                        output.textures_delta.clear();
                        image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
                    }
                    if state == HistoryHandleState::Drag {
                        assert_eq!(app.shell.history_drag_width, Some(112.0));
                        assert_eq!(app.shell.history.unwrap().width(), 112.0);
                    }
                    assert!(app.shell.history.unwrap().right() < app.shell.browser.unwrap().left());
                    if let Ok(dir) = std::env::var("SOLIDCRAFT_HISTORY_EVIDENCE") {
                        std::fs::create_dir_all(&dir).unwrap();
                        image
                            .unwrap()
                            .save(std::path::Path::new(&dir).join(format!(
                                "history-{}-{}-{}-{:?}.png",
                                if names { "names" } else { "icons" },
                                if collapsed { "rail" } else { "browser" },
                                if dark { "dark" } else { "light" },
                                state
                            )))
                            .unwrap();
                    }
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

/// Render actual egui geometry without a desktop window. Opt-in PNGs are reviewer evidence.
#[test]
fn menu_board_in_both_themes() {
    use super::checks::tests::{raster, textures_update};
    use std::collections::HashMap;
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut textures = HashMap::new();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1100.0, 760.0));
        let mut app = crate::test_support::sample_app();
        app.last_command = Some(("solid.extrude".into(), "Extrude".into()));
        let items = crate::context_menu::radial_items(&app);
        let mut image = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                for (name, at) in
                    [("menu", egui::pos2(30.0, 30.0)), ("menu_submenu", egui::pos2(30.0, 430.0)), ("rename_menu", egui::pos2(630.0, 430.0))]
                {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(at, vec2(550.0, 350.0))), |ui| {
                        example(ui, ELEMENTS.iter().find(|e| e.name == name).unwrap());
                    });
                }
                ui.allocate_rect(egui::Rect::from_min_size(egui::pos2(630.0, 30.0), vec2(360.0, 360.0)), egui::Sense::hover());
                radial_menu(ui, egui::pos2(810.0, 210.0), &items, Some(3));
            });
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
        }
        let image = image.unwrap();
        assert!(image.pixels().any(|p| p.0[3] != 0));
        if let Ok(dir) = std::env::var("SOLIDCRAFT_MENU_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            image.save(std::path::Path::new(&dir).join(if dark { "menus-dark.png" } else { "menus-light.png" })).unwrap();
        }
    }
    crate::theme::set_dark(previous);
}

/// CPU raster of the actual popup in all five board states. No display or GPU required.
#[test]
fn command_search_board_in_both_themes() {
    use super::checks::tests::{raster, textures_update};
    use std::collections::HashMap;
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let t = Tokens::get();
        assert!(crate::theme::contrast_ratio(t.form_success, t.form_success_fill) >= 4.5);
        assert!(crate::theme::contrast_ratio(t.selected_ink, t.accent_soft) >= 4.5);
        for (state, query) in [
            ("empty", ""),
            ("matches", "fillet"),
            ("command", r#"solid.extrude {"distance":20}"#),
            ("invalid", r#"solid.extrude {"distance":}"#),
            ("no-match", "filletttt"),
        ] {
            let ctx = crate::fonts::test_context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            let mut app = crate::test_support::sample_app();
            app.run("parameters.add", serde_json::json!({"name":"fillet_radius","expression":"6 mm"})).unwrap();
            app.sbox.used("solid.extrude");
            app.sbox.used("solid.fillet");
            app.sbox.used("sketch.create");
            crate::shortcut_box::open(&mut app, Some(egui::pos2(20.0, 20.0)));
            app.sbox.text = query.to_owned();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(570.0, 650.0));
            let mut textures = HashMap::new();
            let mut image = None;
            for _ in 0..3 {
                scenario::clear_handles();
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), focused: true, ..Default::default() }, |ui| {
                    ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                    crate::shortcut_box::show(&mut app, ui.ctx());
                });
                textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
            }
            let popup = scenario::rect_of("ribbon:search_popup").unwrap();
            let field = scenario::rect_of("shortcut:search").unwrap();
            assert_eq!(field.height(), 40.0);
            assert_eq!(field.width(), 520.0);
            assert_eq!(popup.width(), 520.0);
            assert_eq!(popup.top(), field.bottom() + Tokens::get().space_6);
            assert_eq!(popup.x_range(), field.x_range());
            let footer = scenario::rect_of("ribbon:search_footer").unwrap();
            assert_eq!(footer, egui::Rect::from_min_max(egui::pos2(popup.left(), footer.top()), popup.right_bottom()));
            assert!(bounds.contains_rect(popup), "{state}: {popup:?}");
            match state {
                "matches" => {
                    assert!(scenario::handle_at("menu:solid.fillet").is_some());
                    assert_eq!(scenario::rect_of("menu:solid.fillet").unwrap().height(), 38.0);
                    assert!(scenario::handle_at("menu:search:design:0").is_some());
                }
                "command" => assert!(scenario::rect_of("dialog:note:command_parameters").is_some()),
                "invalid" => assert!(scenario::rect_of("dialog:report:command_parameters").is_some()),
                "no-match" => assert!(scenario::rect_of("dialog:report:search_no_match").is_some()),
                _ => {}
            }
            let image = image.unwrap();
            assert!(image.pixels().any(|p| p.0[3] != 0));
            if let Ok(dir) = std::env::var("SOLIDCRAFT_SEARCH_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                image.save(std::path::Path::new(&dir).join(format!("search-{state}-{}.png", if dark { "dark" } else { "light" }))).unwrap();
            }
        }
    }
    crate::theme::set_dark(previous);
}

/// Exercise the actual surface painters, including deliberately shared artwork whose
/// own accent differs from its tone, tab or feature kind.
#[test]
fn confirm_radial_and_history_rows_keep_multitone_artwork() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let mut cases = vec![
            ("warning", "extrude", "", "MODEL", Some("construction"), 20),
            ("danger", "extrude", "", "MODEL", Some("constraint"), 20),
            ("info", "extrude", "", "MODEL", None, 20),
            ("warning", "status_warning", "", "MODEL", Some("construction"), 20),
            ("danger", "delete", "", "MODEL", Some("constraint"), 20),
            ("danger_menu", "delete", "", "MODEL", Some("constraint"), 16),
            ("info", "surface_extrude", "", "MODEL", None, 20),
        ];
        for (tab, ramp) in [
            ("SKETCH", Some("sketch")),
            ("SURFACE", Some("surface")),
            ("MESH", Some("mesh")),
            ("FORM", Some("form")),
            ("MODEL", None),
            ("SHEET METAL", None),
            ("RENDER", None),
            ("unknown", None),
        ] {
            for surface in ["radial", "radial_hover"] {
                cases.push((surface, "extrude", "", tab, ramp, 24));
            }
        }
        for (kind, ramp) in [
            ("Sketch", Some("sketch")),
            ("SurfaceExtrudeFeature", Some("surface")),
            ("MeshFeature", Some("mesh")),
            ("FormFeature", Some("form")),
            ("ExtrudeFeature", None),
        ] {
            for (surface, side) in [("history_names", 15), ("history_icons", 19)] {
                cases.push((surface, "extrude", kind, "MODEL", ramp, side));
            }
        }
        // Surface feature names without the Surface prefix use the same alias rule
        // as the browser's timeline::icon_of assignments.
        cases.push(("history_names", "surface_extrude", "ExtrudeFeature", "MODEL", Some("surface"), 15));
        cases.push(("history_names", "continuity", "MatchFeature", "MODEL", Some("surface"), 15));
        let mut dominant_accents: std::collections::HashMap<&str, std::collections::HashSet<egui::Color32>> = std::collections::HashMap::new();
        for (surface, icon, kind, tab, ramp, side) in cases {
            let ctx = crate::fonts::test_context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            let mut app = crate::SolveApp::new(Default::default(), Default::default());
            app.ui.tab = tab.into();
            crate::theme::TabAccent::publish(&app, &ctx);
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 360.0));
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                match surface {
                    "radial" | "radial_hover" => {
                        radial_menu(
                            ui,
                            egui::pos2(180.0, 180.0),
                            &[crate::context_menu::Item::action("solid.extrude", "Extrude", icon)],
                            (surface == "radial_hover").then_some(0),
                        );
                    }
                    "danger_menu" => {
                        let mut item = MenuItem::new("gallery:danger", "Delete", icon);
                        item.danger = true;
                        item.show(ui);
                    }
                    "history_names" | "history_icons" => {
                        let style = if surface == "history_names" { HistoryStyle::Names } else { HistoryStyle::Icons };
                        HistoryChip {
                            icon,
                            name: "Extrude1",
                            kind,
                            selected: false,
                            hovered: false,
                            dim: false,
                            suppressed: false,
                            error: false,
                            warning: false,
                            reference: false,
                            accent: crate::theme::TabAccent::for_tab(&Tokens::get(), "MODEL"),
                        }
                        .paint(
                            ui.painter(),
                            egui::Rect::from_min_size(egui::pos2(20.0, 20.0), vec2(158.0, style.height())),
                            style,
                        );
                    }
                    _ => {
                        let tone = match surface {
                            "warning" => Tone::Warning,
                            "danger" => Tone::Danger,
                            _ => Tone::Info,
                        };
                        Confirm::new(ui.id().with("confirm"), icon, "Confirm").tone(tone).content(ui, true, |_| {});
                    }
                }
            });
            let image = output
                .textures_delta
                .set
                .values()
                .flatten()
                .find_map(|delta| match &delta.image {
                    egui::ImageData::Color(image) if image.size == [side, side] => Some(image),
                    _ => None,
                })
                .expect("the surface must upload its icon texture");
            let colours: std::collections::HashSet<_> = image.pixels.iter().filter(|pixel| pixel.a() == 255).copied().collect();
            assert!(colours.len() >= 2, "{surface}/{icon}/{kind}/{tab}, dark={dark}: opaque artwork is a silhouette");
            let resolved = crate::icon_svg::resolve(icon);
            assert!(!resolved.placeholder, "{icon} needs a master");
            let look = crate::icon_svg::Look::themed(dark, ramp.and_then(crate::icon_svg::accent_index), crate::theme::vision());
            let doc = crate::icon_svg::doc_for(resolved.master, side as u32).unwrap();
            let expected = if matches!(surface, "danger" | "danger_menu") {
                crate::icon_svg::render_tinted(doc, look, side as u32)
            } else {
                crate::icon_svg::render(doc, look, side as u32)
            }
            .unwrap();
            if icon == "delete" {
                assert!(colours.len() >= 3, "{surface}, dark={dark}: preserve the lid, faces and outline colours");
                let dominant = colours.iter().max_by_key(|colour| image.pixels.iter().filter(|pixel| *pixel == *colour).count()).unwrap();
                assert!(
                    dominant.r() > dominant.g() && dominant.r() > dominant.b(),
                    "{surface}, dark={dark}: dominant bin colour must be red, got {dominant:?}"
                );
            }
            assert_eq!(
                image.as_ref(),
                &egui::ColorImage::from_rgba_premultiplied([side, side], expected.data()),
                "{surface}/{kind}/{tab}: preserve roles with {ramp:?}"
            );
            if icon == "extrude" {
                let dominant = ["active-1", "active-2", "active-3"]
                    .map(|role| {
                        let colour = crate::icon_svg::role_colour(role, look);
                        (image.pixels.iter().filter(|pixel| **pixel == colour).count(), colour)
                    })
                    .into_iter()
                    .max_by_key(|(count, _)| *count)
                    .unwrap();
                assert!(dominant.0 > 0, "{surface}/{kind}/{tab}: missing accent pixels");
                let group = if matches!(surface, "warning" | "danger" | "info") { "confirm" } else { surface };
                dominant_accents.entry(group).or_default().insert(dominant.1);
            }
            if let Ok(dir) = std::env::var("SOLIDCRAFT_ICON_SURFACE_EVIDENCE") {
                let mut textures = std::collections::HashMap::new();
                super::checks::tests::textures_update(&mut textures, &output.textures_delta);
                let capture = super::checks::tests::raster(&ctx, output.shapes.clone(), output.pixels_per_point, bounds, &textures);
                std::fs::create_dir_all(&dir).unwrap();
                capture
                    .save(std::path::Path::new(&dir).join(format!("{surface}-{icon}-{kind}-{tab}-{}.png", if dark { "dark" } else { "light" })))
                    .unwrap();
            }
            output.textures_delta.clear();
        }
        for (group, count) in [("confirm", 3), ("radial", 5), ("radial_hover", 5), ("history_names", 5), ("history_icons", 5)] {
            assert_eq!(dominant_accents[group].len(), count, "{group}, dark={dark}: dominant accent pixels must differ across mappings");
        }
    }
    crate::theme::set_dark(previous);
}

/// Run the application UI, including the ribbon, and optionally save each transition.
#[test]
fn command_search_ribbon_transition_clips_content_in_both_themes() {
    use super::checks::tests::{raster, textures_update};
    use std::collections::HashMap;
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut app = crate::test_support::sample_app();
        app.home.open = false;
        app.ui.dark = dark;
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 1000.0));
        let mut textures = HashMap::new();
        let mut closed = egui::Rect::NOTHING;
        for frame in 0..26 {
            if frame == 3 {
                crate::shortcut_box::open(&mut app, None);
                app.sbox.text = "fillet".into();
            } else if frame == 14 {
                app.ui.palette_open = false;
                app.sbox.text.clear();
            }
            scenario::clear_handles();
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(bounds), time: Some(f64::from(frame) / 60.0), focused: true, ..Default::default() },
                |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
            );
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            let field = scenario::rect_of("ribbon:search").unwrap();
            if frame == 2 {
                closed = field;
            }
            if (3..14).contains(&frame) {
                let sheet = scenario::rect_of("ribbon:search_popup").unwrap();
                assert!(sheet.top() > field.bottom() && sheet.top() <= field.bottom() + Tokens::get().space_6);
                // Actual result text is painted with the unfolding sheet's clip,
                // rather than relying on a smaller background hiding overflow.
                let mut result_texts = 0;
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.job.text.contains("Fillet")
                        && text.pos.y >= sheet.top()
                        && text.pos.x >= sheet.left()
                        && text.pos.x < sheet.right()
                    {
                        result_texts += 1;
                        assert!(shape.clip_rect.bottom() <= sheet.bottom() + 0.5, "{frame}: {:?} vs {sheet:?}", shape.clip_rect);
                    }
                }
                if frame == 12 {
                    assert!(result_texts > 0);
                }
            }
            let state = match frame {
                2 => Some("closed"),
                4 => Some("opening"),
                12 => Some("open"),
                15 => Some("closing"),
                25 => Some("closed-again"),
                _ => None,
            };
            if frame == 12 {
                assert_eq!(field.size(), vec2(520.0, 40.0));
                assert_eq!(field.right(), closed.right());
            } else if frame == 25 {
                assert_eq!(field, closed);
            }
            if let (Some(state), Ok(dir)) = (state, std::env::var("SOLIDCRAFT_SEARCH_EVIDENCE")) {
                std::fs::create_dir_all(&dir).unwrap();
                let image = raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                image.save(std::path::Path::new(&dir).join(format!("ribbon-{state}-{}.png", if dark { "dark" } else { "light" }))).unwrap();
            }
        }
    }
    crate::theme::set_dark(previous);
}

/// Opt-in CPU capture of the actual File view, with public temporary fixtures only.
#[test]
#[ignore = "set SOLIDCRAFT_FILE_EVIDENCE to an output directory and run explicitly"]
fn file_view_application_evidence_in_both_themes_and_widths() {
    use super::checks::tests::{raster, textures_update};
    use std::collections::HashMap;
    let output_dir = std::path::PathBuf::from(std::env::var("SOLIDCRAFT_FILE_EVIDENCE").expect("set SOLIDCRAFT_FILE_EVIDENCE"));
    std::fs::create_dir_all(&output_dir).unwrap();
    let fixture_dir = std::env::temp_dir().join(format!("solidcraft-file-evidence-{}", std::process::id()));
    std::fs::create_dir_all(&fixture_dir).unwrap();
    let path = fixture_dir.join("sample-plate.solidcraft");
    let fixture = path.to_string_lossy().to_string();
    let mut sample = crate::test_support::sample_app();
    sample.session.execute("file.save_as", &serde_json::json!({"path":fixture})).unwrap();
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        for (name, size) in [("desktop", vec2(1440.0, 1000.0)), ("narrow", vec2(420.0, 1000.0))] {
            let ctx = crate::fonts::test_context();
            let mut app = crate::test_support::sample_app();
            app.home.show(0);
            app.home.add_recent(&fixture);
            app.session.path = Some(fixture.clone());
            let preview = image::load_from_memory(include_bytes!("../../../../assets/samples/exports/0.png")).unwrap().to_rgba8();
            let preview = egui::ColorImage::from_rgba_unmultiplied([preview.width() as usize, preview.height() as usize], preview.as_raw());
            crate::home::install(&mut app.home, &ctx, &fixture, Some(preview));
            app.preferences.theme = if dark { "dark" } else { "light" }.into();
            app.ui.dark = dark;
            app.recoverable.push(solidcraft_engine::recovery::Entry {
                id: "public-evidence".into(),
                path: Some("enclosure-lid.solidcraft".into()),
                name: "Enclosure lid".into(),
                saved_at: web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap().as_secs().saturating_sub(120),
                features: 14,
            });
            crate::theme::set_dark(dark);
            crate::theme::apply(&ctx);
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut textures = HashMap::new();
            for frame in 0..6 {
                let mut input =
                    egui::RawInput { screen_rect: Some(bounds), time: Some(f64::from(frame) / 60.0), focused: true, ..Default::default() };
                app.raw_input_hook(&mut input);
                let mut output = ctx.run_ui(input, |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                textures_update(&mut textures, &output.textures_delta);
                if frame == 5 {
                    assert!(app.home.open);
                    let page = scenario::rect_of("home:page").expect("actual File view allocation");
                    assert!(bounds.contains_rect(page));
                    raster(&ctx, output.shapes.clone(), output.pixels_per_point, bounds, &textures)
                        .save(output_dir.join(format!("file-{name}-{}.png", if dark { "dark" } else { "light" })))
                        .unwrap();
                }
                output.textures_delta.clear();
            }
        }
    }
    crate::theme::set_dark(previous);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(fixture_dir).unwrap();
}

/// CPU-rendered review board using the actual gallery widgets, without a window.
#[test]
fn material_widgets_board_in_both_themes() {
    use super::checks::tests::{raster, textures_update};
    use std::collections::HashMap;
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 240.0));
        let mut textures = HashMap::new();
        let mut image = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                ui.set_width(320.0);
                ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                scope(ui, |ui| {
                    for name in ["sphere_thumbnail", "look_chain", "look_chain_empty"] {
                        example(ui, ELEMENTS.iter().find(|e| e.name == name).unwrap());
                        ui.add_space(Tokens::get().space_12);
                    }
                });
            });
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
        }
        let image = image.unwrap();
        assert!(image.pixels().any(|p| p.0[3] != 0));
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_MATERIAL_WIDGET_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            image.save(std::path::Path::new(&dir).join(if dark { "material-widgets-dark.png" } else { "material-widgets-light.png" })).unwrap();
        }
    }
    crate::theme::set_dark(previous);
}
