//! Navigation preferences, composed from the shared settings controls and figures.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_heading(ui, app.tr("prefs.navigation"), app.tr("prefs.navigation_note"));
    ui_kit::settings_section(ui, "mouse", app.tr("prefs.mouse_heading"), |ui| {
        let mut gestures: Vec<_> = nav_preset(&app.preferences.nav)
            .bindings
            .iter()
            .filter(|binding| binding.button != NavButton::Trackpad && !(crate::mouse_gesture::active(app) && binding.button == NavButton::Right))
            .filter(|binding| app.preferences.right_drag_orbits || !(binding.button == NavButton::Right && binding.mode == NavMode::Orbit))
            .map(|binding| {
                let buttons: &[&str] = match binding.button {
                    NavButton::Left => &["prefs.button_left"],
                    NavButton::Middle => &["prefs.button_middle"],
                    NavButton::Right => &["prefs.button_right"],
                    NavButton::MiddleLeft => &["prefs.button_middle", "prefs.button_left"],
                    NavButton::MiddleRight => &["prefs.button_middle", "prefs.button_right"],
                    NavButton::Trackpad => &[],
                };
                let mut keys = Vec::new();
                if binding.keys.shift {
                    keys.push(crate::i18n::tr("prefs.shift"));
                }
                if binding.keys.ctrl {
                    keys.push("Ctrl/Cmd");
                }
                if binding.keys.alt {
                    keys.push("Alt/Option");
                }
                keys.extend(buttons.iter().map(|button| crate::i18n::tr(button)));
                let action = match binding.mode {
                    NavMode::Orbit => "prefs.gesture_orbit",
                    NavMode::Pan => "prefs.gesture_pan",
                    NavMode::Zoom => "prefs.gesture_zoom",
                };
                (keys.into_iter().map(str::to_owned).collect::<Vec<_>>(), crate::i18n::tr(action).to_string())
            })
            .collect();
        gestures.extend([
            (
                vec![app.tr("prefs.wheel").to_owned()],
                app.tr(if app.preferences.zoom_to_cursor { "prefs.zoom_toward_cursor" } else { "prefs.gesture_zoom" }).to_owned(),
            ),
            (vec![app.tr("prefs.left_click").to_owned()], app.tr("prefs.select").to_owned()),
            (
                vec![app.tr("prefs.right_click").to_owned()],
                app.tr(if app.preferences.mouse_gestures.enabled && app.preferences.nav == crate::mouse_gesture::MOUSE_MODE {
                    "prefs.context_menu_ring"
                } else {
                    "prefs.context_menu"
                })
                .to_owned(),
            ),
        ]);
        ui_kit::settings_mouse_mapping(ui, &gestures, |ui| {
            ui_kit::settings_form(ui, 110.0, |ui| {
                ui_kit::row(ui, app.tr("prefs.mouse_mapping"), |ui| {
                    let picker = ui_kit::dropdown_menu(ui, "pref_nav", nav_preset(&app.preferences.nav).translated_label(), |ui| {
                        for preset in NAV_PRESETS {
                            if ui_kit::list_picker(ui, preset.id, preset.translated_label(), app.preferences.nav == preset.id).clicked() {
                                choose_nav_preset(&mut app.preferences, preset);
                            }
                        }
                    });
                    crate::scenario::publish_target("dialog:Pan, zoom, orbit", picker.rect);
                    picker.on_hover_text(crate::i18n::tr_label(nav_preset(&app.preferences.nav).help));
                });
            });
        });
        // These are real navigation options absent from the static board; retain them below
        // the illustrated mapping rather than enlarging the diagram or repeating its legend.
        let free_orbit = setting::free_orbit(app, ui);
        crate::scenario::publish_target("prefs:free_orbit", free_orbit.rect);
        if app.preferences.free_orbit {
            ui_kit::text(ui, crate::i18n::tr("nav.free_orbit.snap_help"), ui_kit::Text::Hint("free_orbit"));
            let upright = setting::free_orbit_upright(app, ui);
            crate::scenario::publish_target("prefs:free_orbit_upright", upright.rect);
            if app.preferences.free_orbit_upright {
                ui_kit::text(ui, crate::i18n::tr("nav.free_orbit_upright.help"), ui_kit::Text::Hint("free_orbit_upright"));
            }
        }
        // Only shown where the mapping orbits on a right drag (#6492).
        if nav_preset(&app.preferences.nav).bindings.iter().any(|b| b.button == NavButton::Right && b.mode == NavMode::Orbit) {
            let right_drag = setting::right_drag_orbits(app, ui).on_hover_text(crate::i18n::tr("nav.right_drag_orbits.help"));
            crate::scenario::publish_target("prefs:right_drag_orbits", right_drag.rect);
        }
        let help = if app.preferences.nav == TOUCHPAD_NAV {
            app.tr("nav.touchpad.help").to_owned()
        } else if !app.preferences.free_orbit && app.preferences.nav != OPENSCAD_NAV {
            format!("{} {}", app.tr("prefs.extra_navigation"), app.tr("prefs.orbit_snap"))
        } else {
            app.tr("prefs.extra_navigation").to_owned()
        };
        ui_kit::text(ui, &help, ui_kit::Text::Hint("navigation"));
        ui_kit::row(ui, crate::i18n::tr_label("Trackpad"), |ui| {
            let help = match app.preferences.trackpad_mode(Modifiers::NONE) {
                Pan => "Two-finger movement pans; Shift+two-finger movement orbits. Pinch or Ctrl/Cmd+vertical movement zooms. No click is needed.",
                Orbit => "Two-finger movement orbits; Shift+two-finger movement pans. Pinch or Ctrl/Cmd+vertical movement zooms. No click is needed.",
                Zoom => "Vertical two-finger movement and pinch zoom.",
            };
            ui_kit::text(ui, crate::i18n::tr_label(help), ui_kit::Text::Hint("trackpad"));
        });
        let stop = setting::zoom_stop_at_model(app, ui).on_hover_text(crate::i18n::tr("prefs.zoom_stop_at_model_tip"));
        crate::scenario::publish_target("prefs:zoom_stop_at_model", stop.rect);
    });
    ui_kit::settings_section(ui, "orbit_pivot", app.tr_label("Orbit around"), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            for (index, (mode, label)) in
                [(OrbitAround::Origin, "Origin"), (OrbitAround::ViewCentre, "View center"), (OrbitAround::Cursor, "prefs.under_cursor")]
                    .into_iter()
                    .enumerate()
            {
                if ui_kit::option_card(
                    ui,
                    app.tr_label(label),
                    None,
                    app.preferences.orbit_around == mode,
                    true,
                    ui_kit::PreferenceFigure::Pivot(index),
                )
                .clicked()
                {
                    app.preferences.orbit_around = mode;
                }
            }
        });
    });
    ui_kit::settings_section(ui, "wheel_trackpad", app.tr("prefs.wheel_trackpad"), |ui| {
        // Board order is column-major in meaning: zoom behavior at left, gesture behavior at
        // right. Each checkbox keeps its original shared-control label and scenario target.
        let width = (ui.available_width() - (t.space_16 + t.space_4)) * 0.5;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_16 + t.space_4;
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                setting::zoom_to_cursor(app, ui);
                setting::zoom_reverse(app, ui);
                setting::pan_reverse_x(app, ui);
                setting::pan_reverse_y(app, ui);
                setting::orbit_reverse(app, ui);
            });
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                setting::smooth_wheel_zoom(app, ui);
                let touchpad = app.preferences.nav == TOUCHPAD_NAV;
                ui.add_enabled_ui(!touchpad, |ui| {
                    ui_kit::checkbox(ui, crate::i18n::tr_label("Use preset trackpad gestures"), &mut app.preferences.trackpad_pan).on_hover_text(
                        if touchpad {
                            crate::i18n::tr("nav.touchpad.scroll")
                        } else {
                            crate::i18n::tr_label("Two-finger movement uses the selected preset (off: vertical movement zooms); pinch always zooms")
                        },
                    );
                });
                setting::radial_context_menu(app, ui);
                setting::direct_interaction(app, ui);
            });
        });
        ui_kit::row(ui, app.tr("prefs.trackpad_zoom_modifier"), |ui| {
            let platform = crate::tool_modifiers::Platform::of(ui.ctx());
            let choice = &mut app.preferences.trackpad_zoom_modifier;
            ui_kit::dropdown_menu(ui, "pref_trackpad_zoom_modifier", crate::i18n::tr(choice.label_id(platform)), |ui| {
                for (index, value) in TrackpadZoomModifier::ALL.into_iter().enumerate() {
                    if ui_kit::list_picker(
                        ui,
                        &format!("trackpad_zoom_modifier_{index}"),
                        crate::i18n::tr(value.label_id(platform)),
                        *choice == value,
                    )
                    .clicked()
                    {
                        *choice = value;
                    }
                }
            })
            .on_hover_text(crate::i18n::tr("prefs.trackpad_zoom_modifier.help"));
        });
        let cursor = ui_kit::checkbox(ui, crate::i18n::tr_label("Show tool at cursor"), &mut app.ui.tool_cursor);
        crate::scenario::publish_target("view:tool_cursor", cursor.rect);
        ui_kit::divider(ui, "navigation_view_controls");
        ui_kit::settings_form(ui, 150.0, |ui| {
            setting::wheel_zoom_speed(app, ui);
            let (label, note) = (app.tr("prefs.home_fit_size").to_owned(), app.tr("prefs.home_fit_size_note").to_owned());
            ui_kit::row(ui, &label, |ui| {
                let response = ui_kit::slider(
                    ui,
                    &mut app.preferences.home_fit_size,
                    0.1..=1.0,
                    ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                );
                crate::scenario::publish_rect("prefs:home_fit_size", response.rect);
                response.on_hover_text(&note)
            });
            ui_kit::row(ui, app.tr_label("View navigator"), |ui| {
                let mut selected = usize::from(app.preferences.navigator_style == NavigatorStyle::Cube);
                if ui_kit::segmented(
                    ui,
                    &mut selected,
                    &[app.tr("prefs.axes"), crate::i18n::tr_label("Cube")],
                    ui_kit::Segments { intrinsic: true, ..Default::default() },
                ) {
                    app.preferences.navigator_style = if selected == 0 { NavigatorStyle::Axes } else { NavigatorStyle::Cube };
                }
                if let Some(rect) = crate::scenario::rect_of(&format!("dialog:segments:{}", crate::i18n::tr_label("View navigator"))) {
                    crate::scenario::publish_target("prefs:navigator", rect);
                    for (label, key) in [(app.tr("prefs.axes"), "Signed axes"), (crate::i18n::tr_label("Cube"), "Cube")] {
                        if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{label}", crate::i18n::tr_label("View navigator"))) {
                            crate::scenario::publish_target(
                                &format!("prefs:navigator:{key}"),
                                egui::Rect::from_center_size(at, egui::vec2(t.space_24, t.control_height)),
                            );
                        }
                    }
                }
            });
            let (label, note) = (app.tr("prefs.navigator_fit").to_owned(), app.tr("prefs.navigator_fit_note").to_owned());
            let fit = ui_kit::checkbox(ui, &label, &mut app.preferences.navigator_fit).on_hover_text(note);
            crate::scenario::publish_target("prefs:navigator_fit", fit.rect);
            ui_kit::row(ui, app.tr_label("Middle double-click"), |ui| {
                let current = MiddleDoubleClick::of(app);
                ui_kit::settings_control_width(ui, t.form_label_width * 2.0 + t.space_24 * 2.0 + t.space_12, |ui| {
                    ui_kit::dropdown_menu(ui, "pref_middle_double_click", crate::i18n::tr_label(current.label()), |ui| {
                    for (mode, id, label) in MiddleDoubleClick::ALL {
                        if ui_kit::list_picker(ui, id, crate::i18n::tr_label(label), current == mode).clicked() {
                            app.preferences.middle_double_click = id.into();
                        }
                    }
                    }).on_hover_text(crate::i18n::tr_label("Double-clicking the middle button in the view: Home (the home view, framing what is shown), Fit (frame what is shown, keeping the orientation) or nothing"));
                });
            });
            setting::look_at_anim(app, ui);
            setting::look_at_speed(app, ui);
        });
        // The existing ring editor is part of the real product and remains available.
        crate::mouse_gesture::nav_rows(app, ui, t);
    });
    // The 3D mouse is a section like the ones above: the driver, calibration, axes, picture and buttons
    // (`prefs::space_mouse_section`).
    space_mouse_section(app, ui);
}

/// A device's own app on this computer driving the view (the navigation bridge): on or off, its
/// port, and whether an app is connected. Part of the 3D mouse section (`space_mouse_rows`).
pub(super) fn nav_bridge(app: &mut SolveApp, ui: &mut egui::Ui) {
    let (label, tip, port) = (app.tr("ui.nav_bridge").to_owned(), app.tr("ui.nav_bridge_tip").to_owned(), app.tr("ui.nav_bridge_port").to_owned());
    let status = match (&app.space_mouse.bridge, app.preferences.nav_bridge_enabled) {
        (_, false) => None,
        (Some(name), true) => Some((format!("{} · {name}", app.tr("ui.nav_bridge_connected")), false)),
        (None, true) => Some((app.tr("ui.nav_bridge_waiting").to_owned(), true)),
    };
    ui.horizontal_wrapped(|ui| {
        ui_kit::checkbox(ui, &label, &mut app.preferences.nav_bridge_enabled).on_hover_text(&tip);
        if let Some((status, waiting)) = &status {
            ui_kit::settings_badge(ui, status, *waiting);
        }
    });
    ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
        ui_kit::row(ui, &port, |ui| {
            ui.add_enabled_ui(app.preferences.nav_bridge_enabled, |ui| {
                ui_kit::number_field(ui, &mut app.preferences.nav_bridge_port, 1..=u16::MAX, 1.0, "");
            });
        });
    });
}
