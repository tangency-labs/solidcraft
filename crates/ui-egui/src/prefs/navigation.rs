//! Navigation preferences, composed from the shared settings controls and figures.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_heading(ui, app.tr("prefs.navigation"), app.tr("prefs.navigation_note"));
    ui_kit::settings_section(ui, "mouse", app.tr("prefs.mouse_heading"), |ui| {
        let mut gestures: Vec<_> = nav_preset(&app.preferences.nav)
            .bindings
            .iter()
            .filter(|binding| !(crate::mouse_gesture::active(app) && binding.button == NavButton::Right))
            .map(|binding| {
                let button = match binding.button {
                    NavButton::Left => "prefs.button_left",
                    NavButton::Middle => "prefs.button_middle",
                    NavButton::Right => "prefs.button_right",
                };
                let mut keys = Vec::new();
                if binding.keys.shift {
                    keys.push("Shift");
                }
                if binding.keys.ctrl {
                    keys.push("Ctrl/Cmd");
                }
                if binding.keys.alt {
                    keys.push("Alt/Option");
                }
                keys.push(crate::i18n::tr(button));
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
                        for preset in &NAV_PRESETS {
                            if ui_kit::list_picker(ui, preset.id, preset.translated_label(), app.preferences.nav == preset.id).clicked() {
                                app.preferences.nav = preset.id.into();
                                app.preferences.zoom_reverse = preset.reverse_zoom;
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
            ui_kit::text(ui, crate::i18n::tr("nav.free_orbit.help"), ui_kit::Text::Hint("free_orbit"));
        }
        let help = if app.preferences.nav == TOUCHPAD_NAV {
            app.tr("nav.touchpad.help").to_owned()
        } else if !app.preferences.free_orbit && app.preferences.nav != OPENSCAD_NAV {
            format!("{} {}", app.tr("prefs.extra_navigation"), app.tr("prefs.orbit_snap"))
        } else {
            app.tr("prefs.extra_navigation").to_owned()
        };
        ui_kit::text(ui, &help, ui_kit::Text::Hint("navigation"));
    });
    ui_kit::settings_section(ui, "orbit_pivot", app.tr_label("Orbit around"), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            for (index, (mode, label)) in
                [(OrbitAround::Origin, "Origin"), (OrbitAround::ViewCentre, "View centre"), (OrbitAround::Cursor, "prefs.under_cursor")]
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
            });
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                setting::smooth_wheel_zoom(app, ui);
                let touchpad = app.preferences.nav == TOUCHPAD_NAV;
                ui.add_enabled_ui(!touchpad, |ui| {
                    ui_kit::checkbox(ui, crate::i18n::tr_label("Two-finger scroll pans"), &mut app.preferences.trackpad_pan).on_hover_text(if touchpad {
                        crate::i18n::tr("nav.touchpad.scroll")
                    } else {
                        crate::i18n::tr_label("Trackpad gestures pan (off: they zoom like the wheel); mouse wheel zooms, thumb wheel pans sideways, pinch always zooms")
                    });
                });
                setting::radial_context_menu(app, ui);
            });
        });
        ui_kit::divider(ui, "navigation_view_controls");
        ui_kit::settings_form(ui, 150.0, |ui| {
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
    ui_kit::settings_section(ui, "devices", app.tr("prefs.devices"), |ui| {
        ui.horizontal_wrapped(|ui| {
            setting::space_mouse_enabled(app, ui);
            let device = app.space_mouse.device.as_ref();
            let saved = device.is_some_and(|device| app.preferences.space_mouse_calibrations.contains_key(device));
            let status = match device {
                None => app.tr("ui.space_mouse_none").to_owned(),
                Some(device) => format!("{} · {device}", app.tr(if saved { "ui.space_mouse_calibrated" } else { "ui.space_mouse_not_calibrated" })),
            };
            ui_kit::settings_badge(ui, &status, !saved);
        });
        let mapping = app.space_mouse.mapping;
        let rows: Vec<(String, bool)> = (0..6)
            .map(|row| {
                let label = crate::space_mouse::row_label(mapping, row);
                let text = crate::i18n::Locale::for_language(&app.preferences.language).format(label, &[("n", &(row + 1).to_string())]);
                (text, label == "ui.space_mouse_view_off")
            })
            .collect();
        let driven_by = app.tr("ui.space_mouse_driven_by").to_owned();
        let off_tip = app.tr("ui.space_mouse_view_off_tip").to_owned();
        let motions: Vec<String> = crate::space_mouse::MOTIONS.iter().map(|motion| app.tr(motion).to_owned()).collect();
        let calibration = app.space_mouse.device.as_ref().and_then(|device| app.preferences.space_mouse_calibrations.get(device));
        let raw: [i16; 6] = app.space_mouse.raw.into();
        let travel = calibration.map(|calibration| calibration.travel(raw));
        let zones = calibration.map(|calibration| calibration.dead_zone_shares());
        ui_kit::settings_axis_header(ui);
        for (axis, (name, off)) in rows.iter().enumerate() {
            ui.push_id(("space_mouse_axis", axis), |ui| {
                let from = app.preferences.space_mouse_axis_source.get(axis).copied().unwrap_or(axis);
                let value = travel
                    .and_then(|travel| travel.get(from).copied())
                    .unwrap_or_else(|| f32::from(raw.get(from).copied().unwrap_or(0)) / f32::from(crate::space_mouse::FULL_SCALE));
                let zone = zones.and_then(|zones| zones.get(from).copied()).unwrap_or([20.0 / f32::from(crate::space_mouse::FULL_SCALE); 2]);
                let enabled = app.preferences.space_mouse_enabled;
                let preferences = &mut app.preferences;
                let source = &mut preferences.space_mouse_axis_source;
                let reverse = preferences.space_mouse_reverse.get_mut(axis);
                let speed = preferences.space_mouse_axis_speed.get_mut(axis);
                ui_kit::settings_axis_row(
                    ui,
                    name,
                    value,
                    zone,
                    |ui| {
                        ui.add_enabled_ui(enabled && !off, |ui| {
                            if let Some(reverse) = reverse {
                                ui_kit::checkbox(ui, crate::i18n::tr_label("Invert"), reverse);
                            }
                        });
                    },
                    |ui| {
                        ui.add_enabled_ui(enabled && !off, |ui| {
                            if let Some(speed) = speed {
                                ui_kit::settings_axis_gain(ui, speed);
                            }
                        });
                    },
                    |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            let current = source.get(axis).copied().unwrap_or(axis);
                            let mut chosen = current;
                            ui_kit::dropdown_menu(ui, ("sc_space_mouse_source", axis), motions.get(current).map_or("", String::as_str), |ui| {
                                for (from, name) in motions.iter().enumerate() {
                                    if ui_kit::list_picker(ui, name, name, chosen == from).clicked() {
                                        chosen = from;
                                    }
                                }
                            })
                            .on_hover_text(if *off { &off_tip } else { &driven_by });
                            if chosen != current {
                                crate::space_mouse::reassign(source, axis, chosen);
                            }
                        });
                    },
                );
            });
        }
        ui_kit::settings_form(ui, 100.0, |ui| {
            ui_kit::row(ui, app.tr("prefs.overall_speed"), |ui| {
                ui.add_enabled_ui(app.preferences.space_mouse_enabled, |ui| {
                    ui_kit::slider(
                        ui,
                        &mut app.preferences.space_mouse_speed,
                        0.25..=3.0,
                        ui_kit::Slider { step: Some(0.05), format: |value| format!("{value:.2}×"), ..Default::default() },
                    );
                });
            });
        });
        ui_kit::text(ui, app.tr("prefs.dead_zone_note"), ui_kit::Text::Hint("space_mouse_zone"));
        // Only where the reader has seen the choice: Linux, with spacenavd and the device both there.
        if let Some(present) = app.space_mouse.drivers.filter(|present| present.spacenavd || present.direct) {
            let [label, auto, direct, only_spacenavd, only_direct] =
                ["driver", "driver_auto", "driver_direct", "driver_only_spacenavd", "driver_only_direct"]
                    .map(|key| app.tr(&format!("ui.space_mouse_{key}")).to_owned());
            let driver = &mut app.preferences.space_mouse_driver;
            ui_kit::row(ui, &label, |ui| match (present.spacenavd, present.direct) {
                (true, true) => {
                    let name = |driver| match driver {
                        SpaceMouseDriver::Automatic => auto.as_str(),
                        SpaceMouseDriver::Spacenavd => "spacenavd",
                        SpaceMouseDriver::Direct => direct.as_str(),
                    };
                    ui_kit::dropdown_menu(ui, "sc_space_mouse_driver", name(*driver), |ui| {
                        for choice in SpaceMouseDriver::ALL {
                            if ui_kit::list_picker(ui, name(choice), name(choice), *driver == choice).clicked() {
                                *driver = choice;
                            }
                        }
                    });
                }
                (true, false) => {
                    ui_kit::text(ui, &only_spacenavd, ui_kit::Text::Hint("space_mouse_driver"));
                }
                (false, _) => {
                    ui_kit::text(ui, &only_direct, ui_kit::Text::Hint("space_mouse_driver"));
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let device = app.space_mouse.device.clone();
            let saved = device.as_ref().is_some_and(|device| app.preferences.space_mouse_calibrations.contains_key(device));
            ui.add_enabled_ui(app.preferences.space_mouse_enabled && device.is_some(), |ui| {
                if ui_kit::button(ui, app.tr("ui.space_mouse_calibrate"), ui_kit::Button::Normal)
                    .on_hover_text(app.tr("ui.space_mouse_calibrate_tip"))
                    .clicked()
                {
                    crate::space_mouse_calibration::start(app);
                }
            });
            ui.add_enabled_ui(app.preferences.space_mouse_enabled && saved, |ui| {
                if ui_kit::button(ui, app.tr("ui.space_mouse_forget"), ui_kit::Button::Ghost).clicked()
                    && let Some(device) = &device
                {
                    app.preferences.space_mouse_calibrations.remove(device);
                }
            });
            let reassigned = app.preferences.space_mouse_axis_source != crate::space_mouse::SAME_AXES;
            ui.add_enabled_ui(app.preferences.space_mouse_enabled && reassigned, |ui| {
                if ui_kit::button(ui, app.tr("ui.space_mouse_reset_axes"), ui_kit::Button::Ghost).clicked() {
                    app.preferences.space_mouse_axis_source = crate::space_mouse::SAME_AXES;
                }
            });
        });
        let enabled = app.preferences.space_mouse_enabled;
        let (label, tip) = (app.tr("ui.space_mouse_dead_zone").to_owned(), app.tr("ui.space_mouse_dead_zone_tip").to_owned());
        let (device, raw) = (app.space_mouse.device.clone(), app.space_mouse.raw.into());
        if let Some(calibration) = device.and_then(|device| app.preferences.space_mouse_calibrations.get_mut(&device)) {
            ui_kit::row(ui, &label, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui_kit::slider(
                        ui,
                        &mut calibration.dead_zone_share,
                        0.0..=solidcraft_space_mouse::MAX_DEAD_ZONE_SHARE,
                        ui_kit::Slider { step: Some(0.01), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                    )
                    .on_hover_text(tip);
                });
            });
            crate::space_mouse_calibration::dead_zone_pads(ui, calibration, raw);
        }
        ui_kit::text(
            ui,
            crate::i18n::tr_label("Move and rotate the view together; sketches use pan and zoom only."),
            ui_kit::Text::Hint("space_mouse"),
        );
    });
}
