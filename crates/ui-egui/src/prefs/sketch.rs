//! Sketch and selection preferences with shared figures and the live modifier legend.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_page(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui_kit::settings_heading_with_tag(
            ui,
            app.tr("prefs.structure.sketch"),
            app.tr("prefs.sketch_selection_note"),
            Some(crate::settings_profiles::tag),
        );
        snapping(app, ui);
        drawing(app, ui);
        opening(app, ui);
        marks(app, ui);
        selecting(app, ui);
    });
}

fn snapping(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_section(ui, "sketch", app.tr("prefs.sketch.snapping_guides"), |ui| {
        let available = ui.available_width();
        let figure_width = t.space_24 * 5.0;
        let labels_width = ["prefs.sketch_tracking_guides", "prefs.sketch_tracking_points", "prefs.sketch_recent_snap", "prefs.sketch_align_points"]
            .into_iter()
            .map(|id| check_width(ui, app.tr(id), true))
            .fold(0.0, f32::max);
        let controls_width = if available >= figure_width + t.space_14 + labels_width { available - figure_width - t.space_14 } else { available };
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_14;
            if !crate::prefs_search::suppress_chrome(ui) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(figure_width.min(available), t.space_24 * 2.0 + t.space_16), egui::Sense::hover());
                ui_kit::paint_sketch_guides(&ui.painter_at(rect), rect);
            }
            ui.allocate_ui_with_layout(egui::vec2(controls_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                check(
                    ui,
                    "sketch_tracking_guides",
                    "prefs.sketch_tracking_guides",
                    Some("prefs.sketch_tracking_guides_note"),
                    &mut app.preferences.sketch_tracking_guides,
                );
                ui.add_enabled_ui(app.preferences.sketch_tracking_guides, |ui| {
                    check(
                        ui,
                        "sketch_tracking_points",
                        "prefs.sketch_tracking_points",
                        Some("prefs.sketch_tracking_points_note"),
                        &mut app.preferences.sketch_tracking_points,
                    );
                });
                check(
                    ui,
                    "sketch_recent_snap",
                    "prefs.sketch_recent_snap",
                    Some("prefs.sketch_recent_snap_note"),
                    &mut app.preferences.sketch_recent_snap,
                );
                check(
                    ui,
                    "sketch_align_points",
                    "prefs.sketch_align_points",
                    Some("prefs.sketch_align_points_note"),
                    &mut app.preferences.sketch_align_points,
                );
            });
        });
        let label_width = ui_kit::ContentWidth::label(ui, app.tr("prefs.hold_stop_snapping"));
        ui_kit::settings_form(ui, label_width, |ui| {
            ui_kit::row(ui, app.tr("prefs.hold_stop_snapping"), |ui| {
                ui_kit::settings_control_width(ui, t.space_24 * 8.0 + t.space_8, |ui| {
                    let combo = &mut app.preferences.suppress_sketch_snap;
                    let response = ui_kit::dropdown_menu(ui, "pref_suppress_sketch_snap", &combo.shown_label(), |ui| {
                        for choice in ModifierCombo::SNAP_SUPPRESS_CHOICES {
                            if ui_kit::list_picker(ui, &choice.label(), &choice.shown_label(), *combo == choice).clicked() {
                                *combo = choice;
                            }
                        }
                    })
                    .on_hover_text(crate::i18n::tr("prefs.snap_drawing_note"));
                    crate::scenario::publish_target("prefs:suppress_sketch_snap", response.rect);
                });
            });
        });
    });
}

fn drawing(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_section(ui, "drawing", app.tr("prefs.sketch.drawing"), |ui| {
        let left_labels = [
            "prefs.line_drag_draws_one",
            "prefs.line_hover_tangent_arc",
            "ui.center_arc_follows_cursor",
            "prefs.sketch_construction_lines",
            "Dimension new shapes",
            "Also place them from the origin (fully constrained)",
        ];
        let right_labels =
            ["ui.shortcut_cycles_modes", "prefs.ribbon_click_ends_tool", "ui.project_face_edges_sketch", "prefs.project_options_dialog"];
        let natural = [left_labels.as_slice(), right_labels.as_slice()]
            .map(|labels| labels.iter().map(|id| check_width(ui, app.tr(id), true)).fold(0.0, f32::max));
        let available = ui.available_width();
        let gap = t.space_24 + t.space_8;
        let two_columns = available >= t.space_24 * 20.0 + gap && !crate::prefs_search::suppress_chrome(ui);
        let left = if two_columns { (available - gap) * natural[0] / (natural[0] + natural[1]).max(1.0) } else { available };
        let right = if two_columns { available - gap - left } else { available };
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                check(ui, "line_drag_draws_one", "prefs.line_drag_draws_one", Some("prefs.line_drag_draws_one_note"), &mut app.preferences.line_drag_draws_one);
                check(ui, "line_hover_tangent_arc", "prefs.line_hover_tangent_arc", Some("prefs.line_hover_tangent_arc_note"), &mut app.preferences.line_hover_tangent_arc);
                check(ui, "center_arc_follows_cursor", "ui.center_arc_follows_cursor", Some("ui.center_arc_follows_cursor_tip"), &mut app.preferences.center_arc_follows_cursor);
                check(ui, "sketch_construction_lines", "prefs.sketch_construction_lines", None, &mut app.preferences.sketch_construction_lines);
                check(ui, "sketch_auto_dimensions", "Dimension new shapes", Some("Each shape you draw gets all its dimensions, at the sizes you drew (typed values win). A size the sketch already sets is left out."), &mut app.preferences.sketch_auto_dimensions);
                ui.add_enabled_ui(app.preferences.sketch_auto_dimensions, |ui| {
                    check(ui, "sketch_auto_locate", "Also place them from the origin (fully constrained)", Some("Each new shape is also dimensioned from the sketch origin until nothing of it is left free. A shape already attached to the origin or to placed geometry gets none."), &mut app.preferences.sketch_auto_locate);
                });
            });
            ui.allocate_ui_with_layout(egui::vec2(right, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                check(ui, "shortcut_cycles_modes", "ui.shortcut_cycles_modes", Some("ui.shortcut_cycles_modes_tip"), &mut app.preferences.shortcut_cycles_modes);
                check(ui, "ribbon_click_ends_tool", "prefs.ribbon_click_ends_tool", Some("prefs.ribbon_click_ends_tool_note"), &mut app.preferences.ribbon_click_ends_tool);
                let mut auto = app.session.auto_project;
                if check(ui, "auto_project", "ui.project_face_edges_sketch", None, &mut auto).changed() {
                    let _ = app.run("sketch.auto_project", json!({"value": auto}));
                }
                check(ui, "project_options_dialog", "prefs.project_options_dialog", Some("prefs.project_options_dialog_note"), &mut app.preferences.project_options_dialog);
            });
        });
    });
}

fn opening(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_section(ui, "opening_closing", app.tr("prefs.sketch.opening_closing"), |ui| {
        check(
            ui,
            "sketch_face_activates_part",
            "prefs.sketch_face_activates_part",
            Some("prefs.sketch_face_activates_part_note"),
            &mut app.preferences.sketch_face_activates_part,
        );
        check(
            ui,
            "sketch_open_look_at",
            "prefs.sketch_open_look_at",
            Some("prefs.sketch_open_look_at_note"),
            &mut app.preferences.sketch_open_look_at,
        );
        picture_group(ui, app.tr("prefs.sketch_exit_view"), |ui| {
            let label = app.tr("prefs.sketch_exit_view");
            let mut indexed = crate::prefs_search::texts_hit(ui, &[label]);
            if !crate::prefs_search::suppress_chrome(ui) {
                ui_kit::section_heading(ui, label);
            }
            let cards = ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                let width = (t.space_24 * 8.0 + t.space_4).min(ui.max_rect().width());
                for (choice, key, sub, figure) in [
                    (SketchExitView::Previous, "previous", "", ui_kit::paint_sketch_exit_previous as fn(&egui::Painter, egui::Rect)),
                    (SketchExitView::Keep, "keep", app.tr("prefs.default"), ui_kit::paint_sketch_exit_keep),
                    (
                        SketchExitView::UnlessRotated,
                        "unless_rotated",
                        app.tr("prefs.sketch.unless_turned_note"),
                        ui_kit::paint_sketch_exit_unless_turned,
                    ),
                ] {
                    let response = ui_kit::picture_card(
                        ui,
                        &ui_kit::Card {
                            label: app.tr(choice.label_id()),
                            sub: Some(sub),
                            selected: app.preferences.sketch_exit_view == choice,
                            enabled: true,
                            options: None,
                            check: false,
                            aspect: 120.0 / 64.0,
                            figure: &figure,
                        },
                        width,
                    )
                    .on_hover_text(app.tr("prefs.sketch_exit_view_note"));
                    crate::scenario::publish_target(&format!("prefs:sketch_exit_view:{key}"), response.rect);
                    crate::scenario::publish_target(&format!("dialog:list:{}", choice.label_id()), response.rect);
                    if app.preferences.sketch_exit_view == choice {
                        crate::scenario::publish_target("prefs:sketch_exit_view", response.rect);
                        crate::scenario::publish_target("dialog:When a sketch ends", response.rect);
                    }
                    let option = app.tr(choice.label_id());
                    if !indexed && crate::prefs_search::texts_hit(ui, &[option]) {
                        crate::prefs_search::remember(ui, option, response.rect);
                        indexed = true;
                    }
                    crate::prefs_search::follow(ui, option, response.rect);
                    if response.clicked() {
                        app.preferences.sketch_exit_view = choice;
                    }
                }
            });
            crate::scenario::publish_rect("prefs:sketch_exit_view:choices", cards.response.rect);
            crate::prefs_search::remember(ui, label, cards.response.rect);
            crate::prefs_search::follow(ui, label, cards.response.rect);
        });
    });
}

fn marks(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_section(ui, "marks", app.tr("prefs.sketch.marks"), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
            let available = ui.available_width();
            let gap = ui.spacing().item_spacing.x;
            let nominal = t.space_24 * 8.0 + t.space_16;
            let fitted = (available - gap * 2.0) / 3.0;
            let width = if fitted >= nominal { fitted } else { nominal.min(available) };
            // Round down to whole device pixels so three borders fit along with both gaps.
            let ppp = ui.ctx().pixels_per_point().max(0.5);
            let width = (width.max(0.0) * ppp).floor() / ppp;
            for (key, label, help, value, figure) in [
                (
                    "sketch_state_icons",
                    "prefs.sketch_state_icons",
                    "prefs.sketch_state_icons_note",
                    &mut app.preferences.sketch_state_icons,
                    ui_kit::paint_sketch_state_icons as fn(&egui::Painter, egui::Rect),
                ),
                (
                    "sketch_conflict_colour",
                    "prefs.sketch_conflict_colour",
                    "prefs.sketch_conflict_colour_note",
                    &mut app.preferences.sketch_conflict_colour,
                    ui_kit::paint_sketch_conflict,
                ),
                (
                    "hide_constraint_glyphs_zoomed_out",
                    "prefs.hide_constraint_glyphs_zoomed_out",
                    "prefs.hide_constraint_glyphs_zoomed_out_note",
                    &mut app.preferences.hide_constraint_glyphs_zoomed_out,
                    ui_kit::paint_sketch_hide_glyphs,
                ),
            ] {
                let response = ui_kit::toggle_tile(
                    ui,
                    &ui_kit::Card {
                        label: crate::i18n::tr(label),
                        sub: None,
                        selected: *value,
                        enabled: true,
                        options: None,
                        check: false,
                        aspect: 120.0 / 64.0,
                        figure: &figure,
                    },
                    width,
                )
                .on_hover_text(crate::i18n::tr(help));
                targets(key, &response);
                if response.clicked() {
                    *value = !*value;
                }
            }
        });
    });
}

fn selecting(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_section(ui, "selection", app.tr("prefs.selecting"), |ui| {
        picture_group(ui, app.tr("prefs.sketch.click_selects"), |ui| {
            let label = app.tr("prefs.sketch.click_selects");
            let mut indexed = crate::prefs_search::texts_hit(ui, &[label]);
            if !crate::prefs_search::suppress_chrome(ui) {
                ui_kit::section_heading(ui, label);
            }
            let cards = ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                let width = (t.space_24 * 6.0 + t.space_6).min(ui.max_rect().width());
                for (bodies, key, label, figure) in [
                    (false, "faces_edges", "prefs.sketch.faces_edges", ui_kit::paint_select_faces_edges as fn(&egui::Painter, egui::Rect)),
                    (true, "bodies", "prefs.sketch.whole_bodies", ui_kit::paint_select_bodies),
                ] {
                    let response = ui_kit::picture_card(
                        ui,
                        &ui_kit::Card {
                            label: app.tr(label),
                            sub: None,
                            selected: app.ui.pick_bodies == bodies,
                            enabled: true,
                            options: None,
                            check: false,
                            aspect: 120.0 / 64.0,
                            figure: &figure,
                        },
                        width,
                    );
                    crate::scenario::publish_target(&format!("prefs:pick_bodies:{key}"), response.rect);
                    if bodies {
                        crate::scenario::publish_target("prefs:pick_bodies", response.rect);
                        crate::scenario::publish_target("dialog:Click selects whole bodies", response.rect);
                    }
                    let option = app.tr(label);
                    if !indexed && crate::prefs_search::texts_hit(ui, &[option]) {
                        crate::prefs_search::remember(ui, option, response.rect);
                        indexed = true;
                    }
                    crate::prefs_search::follow(ui, option, response.rect);
                    if response.clicked() {
                        app.ui.pick_bodies = bodies;
                    }
                }
            });
            crate::scenario::publish_rect("prefs:pick_bodies:choices", cards.response.rect);
            crate::prefs_search::remember(ui, label, cards.response.rect);
            crate::prefs_search::follow(ui, label, cards.response.rect);
        });
        check(
            ui,
            "selection_shift_add_only",
            "prefs.shift_add_ctrl_toggle",
            Some("ui.selection_shift_add_only_tip"),
            &mut app.preferences.selection_shift_add_only,
        );
        // Draw after the checkbox so the Shift legend reflects this frame's change.
        let click = app.tr("prefs.click");
        let shift = app.tr("prefs.shift");
        let ctrl = crate::tool_modifiers::ctrl_name_for(crate::tool_modifiers::Platform::of(ui.ctx()));
        let toggle = app.tr("prefs.toggle_item");
        ui_kit::settings_bindings(
            ui,
            &[
                (&[click], app.tr("prefs.select_one_item")),
                (&[shift, click], if app.preferences.selection_shift_add_only { app.tr("prefs.add_item") } else { toggle }),
                (&[ctrl, click], toggle),
            ],
        );
        check(ui, "prefer_free_sketch_points", "prefs.prefer_free_sketch_points", None, &mut app.preferences.prefer_free_sketch_points);
        check(
            ui,
            "select_other_hidden",
            "prefs.select_other_hidden",
            Some("prefs.select_other_hidden_help"),
            &mut app.preferences.select_other_hidden,
        );
        check(
            ui,
            "manipulator_free_alt",
            "prefs.alt_frees_manipulators",
            Some("ui.manipulator_free_alt_tip"),
            &mut app.preferences.manipulator_free_alt,
        );
    });
}

/// Natural-height picture groups own their search entries without allocating a form label column.
fn picture_group(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let gate = crate::prefs_search::block_gate(ui, label, None);
    let before = crate::prefs_search::hits(ui);
    let show = gate.as_ref().is_none_or(|gate| gate.show);
    let content = |ui: &mut egui::Ui| {
        let group = ui.scope(|ui| ui_kit::named_control(ui, label, add));
        crate::scenario::publish_rect(&format!("dialog:row:{label}"), group.response.rect);
    };
    if show {
        content(ui);
    } else {
        crate::prefs_search::run_hidden(ui, label, content);
    }
    if let Some(gate) = &gate {
        crate::prefs_search::block_finish(ui, gate, show, before);
    }
}

fn check_width(ui: &egui::Ui, label: &str, help: bool) -> f32 {
    let t = Tokens::get();
    ui_kit::ContentWidth::label(ui, label) + t.space_14 + t.space_6 + if help { t.space_8 + t.space_12 + 1.0 } else { 0.0 }
}

/// Standard checkbox, its hover help, and its existing field target.
fn check(ui: &mut egui::Ui, key: &str, label: &str, help: Option<&str>, value: &mut bool) -> egui::Response {
    let help = help.map(crate::i18n::tr);
    ui.horizontal_top(|ui| {
        let t = Tokens::get();
        let show_help = help.is_some() && !crate::prefs_search::suppress_chrome(ui);
        let width = (ui.available_width() - if show_help { t.space_8 + t.space_12 + 1.0 } else { 0.0 }).max(0.0).min(check_width(
            ui,
            crate::i18n::tr(label),
            false,
        ));
        ui.spacing_mut().item_spacing.x = t.space_8;
        let response = ui
            .allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui_kit::checkbox(ui, crate::i18n::tr(label), value)
            })
            .inner;
        targets(key, &response);
        if let Some(help) = help {
            if show_help && response.rect.is_positive() {
                ui_kit::help_icon(ui, help);
            }
            response.on_hover_text(help)
        } else {
            response
        }
    })
    .inner
}

fn targets(key: &str, response: &egui::Response) {
    crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
    let legacy = match key {
        "sketch_tracking_guides" => "Center and midpoint alignment guides",
        "sketch_tracking_points" => "Alignment guides from end points and corners while drawing",
        "sketch_recent_snap" => "Auto snap to recent centers and midpoints",
        "sketch_align_points" => "Line up with other sketch points while drawing",
        "sketch_auto_dimensions" => "Dimension new shapes",
        "sketch_auto_locate" => "Also place them from the origin (fully constrained)",
        "center_arc_follows_cursor" => "Center point arc follows the cursor's direction",
        "shortcut_cycles_modes" => "Pressing a tool's shortcut again switches its mode",
        "project_options_dialog" => "Project options dialog",
        "sketch_state_icons" => "Sketch icons in the color of their state",
        "sketch_conflict_colour" => "Conflicting geometry in the conflict color",
        "hide_constraint_glyphs_zoomed_out" => "Hide constraint glyphs when zoomed out",
        "prefer_free_sketch_points" => "Prefer movable sketch points when points overlap",
        _ => return,
    };
    crate::scenario::publish_target(&format!("dialog:{legacy}"), response.rect);
}
