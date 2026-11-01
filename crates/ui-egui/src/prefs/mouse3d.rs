//! 3D mouse preferences. Device state and all actions come from the live reader and existing jobs.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_page(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui_kit::settings_heading(ui, app.tr("prefs.structure.mouse3d"), app.tr("prefs.structure.mouse3d_note"));
        device(app, ui);
        orbit(app, ui);
        axes(app, ui);
        ui_kit::settings_section(ui, "space_mouse_buttons", app.tr("ui.3d_mouse_buttons"), |ui| space_mouse_buttons(app, ui));
        advanced(app, ui);
    });
}

fn device(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_section(ui, "space_mouse_device", app.tr("prefs.navigation.device"), |ui| {
        // A confirmed input record identifies the device; an open reader alone does not.
        let card =
            ui_kit::settings_card(ui, |ui| {
                let device = app.space_mouse.device.clone();
                let saved = device.as_ref().is_some_and(|device| app.preferences.space_mouse_calibrations.contains_key(device));
                let denied = found_but_not_allowed(app.space_mouse.drivers);
                // Native wrapping labels use the initial row height as their first-line minimum.
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), t.control_height),
                    egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                    |ui| {
                        if let Some(device) = &device {
                            ui_kit::text(ui, device, ui_kit::Text::Inline);
                            ui_kit::settings_state_badge(ui, app.tr("ui.nav_bridge_connected"), ui_kit::SettingsBadgeTone::Success);
                            ui_kit::settings_state_badge(
                                ui,
                                app.tr(if saved { "ui.3d_mouse_calibrated" } else { "ui.3d_mouse_not_calibrated" }),
                                if saved { ui_kit::SettingsBadgeTone::Success } else { ui_kit::SettingsBadgeTone::Warning },
                            );
                        } else {
                            let status = app.tr(if denied {
                                "ui.3d_mouse_denied_short"
                            } else if waiting_for_a_record(app.space_mouse.drivers) {
                                "ui.3d_mouse_calibrate_wait"
                            } else {
                                "ui.3d_mouse_none"
                            });
                            let response = ui_kit::text(ui, status, ui_kit::Text::Report("calibration_status", false))
                                .on_hover_text(app.tr(if denied { "ui.3d_mouse_denied" } else { "ui.3d_mouse_wiggle" }));
                            crate::scenario::publish_target("prefs:space_mouse_status", response.rect);
                        }
                    },
                );
                let calibrate_width = ui_kit::ContentWidth::button(ui, app.tr("ui.3d_mouse_calibrate"));
                let forget_width = ui_kit::ContentWidth::button(ui, app.tr("ui.3d_mouse_forget"));
                let available = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(available, t.control_height),
                    egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = t.space_8;
                        ui.allocate_ui_with_layout(
                            egui::vec2(calibrate_width.min(available), t.control_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui_kit::settings_form(ui, 0.0, |ui| {
                                    ui_kit::row(ui, app.tr("ui.3d_mouse_calibrate"), |ui| {
                                        ui.add_enabled_ui(app.preferences.space_mouse_enabled && device.is_some(), |ui| {
                                            let response = ui_kit::button(ui, app.tr("ui.3d_mouse_calibrate"), ui_kit::Button::Normal)
                                                .on_hover_text(app.tr("ui.3d_mouse_calibrate_tip"))
                                                .on_disabled_hover_text(app.tr(if device.is_none() {
                                                    "ui.3d_mouse_calibrate_wait"
                                                } else {
                                                    "ui.3d_mouse_calibrate_tip"
                                                }));
                                            crate::scenario::publish_target("prefs:space_mouse_calibrate", response.rect);
                                            crate::scenario::publish_target(
                                                &format!("dialog:button:Calibration:{}", app.tr("ui.3d_mouse_calibrate")),
                                                response.rect,
                                            );
                                            if response.clicked() {
                                                crate::space_mouse_calibration::start(app);
                                            }
                                        });
                                    });
                                });
                            },
                        );
                        if saved {
                            ui.allocate_ui_with_layout(
                                egui::vec2(forget_width.min(available), t.control_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui_kit::settings_form(ui, 0.0, |ui| {
                                        ui_kit::row(ui, app.tr("ui.3d_mouse_forget"), |ui| {
                                            ui.add_enabled_ui(app.preferences.space_mouse_enabled, |ui| {
                                                let response = ui_kit::button(ui, app.tr("ui.3d_mouse_forget"), ui_kit::Button::Normal);
                                                crate::scenario::publish_target("prefs:space_mouse_forget", response.rect);
                                                crate::scenario::publish_target(
                                                    &format!("dialog:button:Calibration:{}", app.tr("ui.3d_mouse_forget")),
                                                    response.rect,
                                                );
                                                if response.clicked()
                                                    && let Some(device) = &device
                                                {
                                                    app.preferences.space_mouse_calibrations.remove(device);
                                                }
                                            });
                                        });
                                    });
                                },
                            );
                        }
                    },
                );
            });
        crate::scenario::publish_rect("prefs:space_mouse_device_card", card.response.rect);
        setting::space_mouse_enabled(app, ui).on_hover_text(crate::i18n::tr("ui.3d_mouse_hint"));
        setting::space_mouse_in_view(app, ui).on_hover_text(crate::i18n::tr("ui.3d_mouse_show_in_view_tip"));
        let names = [
            (SpaceMouseGhost::LeftViewport, app.tr("ui.3d_mouse_ghost_left_viewport").to_owned()),
            (SpaceMouseGhost::RightViewport, app.tr("ui.3d_mouse_ghost_right_viewport").to_owned()),
            (SpaceMouseGhost::LeftWindow, app.tr("ui.3d_mouse_ghost_left_window").to_owned()),
        ];
        let label_width = ["prefs.overall_speed", "ui.3d_mouse_ghost_position"]
            .into_iter()
            .map(|id| ui_kit::ContentWidth::label(ui, app.tr(id)))
            .fold(0.0, f32::max);
        ui_kit::settings_form(ui, label_width, |ui| {
            ui_kit::row(ui, app.tr("prefs.overall_speed"), |ui| {
                ui.add_enabled_ui(app.preferences.space_mouse_enabled, |ui| {
                    let response = ui_kit::slider(
                        ui,
                        &mut app.preferences.space_mouse_speed,
                        0.25..=5.0,
                        ui_kit::Slider { step: Some(0.05), format: |value| format!("{value:.2}×"), ..Default::default() },
                    );
                    crate::scenario::publish_target("prefs:space_mouse_speed", response.rect);
                });
            });
            ui_kit::row(ui, app.tr("ui.3d_mouse_ghost_position"), |ui| {
                let (in_view, position) = (app.preferences.space_mouse_in_view, &mut app.preferences.space_mouse_ghost_position);
                ui.add_enabled_ui(in_view, |ui| {
                    ui_kit::settings_control_width(ui, t.space_24 * 10.0, |ui| {
                        let current = names.iter().find(|(choice, _)| choice == position).map_or("", |(_, name)| name.as_str());
                        let response = ui_kit::dropdown_menu(ui, "sc_space_mouse_ghost", current, |ui| {
                            for (choice, name) in &names {
                                if ui_kit::list_picker(ui, name, name, choice == position).clicked() {
                                    *position = *choice;
                                }
                            }
                        });
                        crate::scenario::publish_target("prefs:space_mouse_ghost_position", response.rect);
                    });
                });
            });
        });
    });
}

fn orbit(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "space_mouse_orbit", app.tr("ui.orbit"), |ui| {
        let orbits = [app.tr("ui.3d_mouse_orbit_traced").to_owned(), app.tr("ui.3d_mouse_orbit_classic").to_owned()];
        let label = app.tr("ui.3d_mouse_orbit");
        let help = app.tr("ui.3d_mouse_orbit_tip");
        let width = ui_kit::ContentWidth::label(ui, label) + Tokens::get().space_16 + Tokens::get().space_4;
        ui_kit::settings_form(ui, width, |ui| {
            ui_kit::settings_row_help(ui, label, help, |ui| {
                crate::prefs_search::note_current(ui, &orbits.join(" "));
                let mut selected = usize::from(!app.preferences.space_mouse_model_pivot);
                let response = ui.scope(|ui| {
                    ui_kit::segmented(
                        ui,
                        &mut selected,
                        &orbits.each_ref().map(String::as_str),
                        ui_kit::Segments {
                            intrinsic: true,
                            targets: &["prefs:3d_mouse_orbit_traced", "prefs:3d_mouse_orbit_classic"],
                            ..Default::default()
                        },
                    )
                });
                if response.inner {
                    app.preferences.space_mouse_model_pivot = selected == 0;
                }
                if !crate::prefs_search::texts_hit(ui, &[label]) {
                    for option in &orbits {
                        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{label}:{option}")) {
                            crate::prefs_search::remember(ui, option, rect);
                            crate::prefs_search::follow(ui, option, rect);
                        }
                    }
                }
            });
        });
        ui.add_enabled_ui(app.preferences.space_mouse_enabled, |ui| {
            setting::space_mouse_sketch_orbit(app, ui).on_hover_text(crate::i18n::tr("ui.3d_mouse_sketch_orbit_tip"));
        });
    });
}

fn axes(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "space_mouse_axes", app.tr("prefs.axes"), |ui| {
        // Rows say what they do to the view, through the mapping navigation uses; the choices
        // are the device's calibrated motions. Neither names a kind of device.
        let mapping = app.space_mouse.mapping;
        let rows: Vec<(String, bool)> = (0..6)
            .map(|row| {
                let label = crate::space_mouse::row_label(mapping, row);
                let text = crate::i18n::Locale::for_language(&app.preferences.language).format(label, &[("n", &(row + 1).to_string())]);
                (text, label == "ui.3d_mouse_view_off")
            })
            .collect();
        let driven_by = app.tr("ui.3d_mouse_driven_by").to_owned();
        let off_tip = app.tr("ui.3d_mouse_view_off_tip").to_owned();
        let motions: Vec<String> = crate::space_mouse::MOTIONS.iter().map(|motion| app.tr(motion).to_owned()).collect();
        // The axes are a table of their own: each row is a live meter of the motion that drives that
        // view axis, then its Invert, Speed and the motion choice.
        let enabled = app.preferences.space_mouse_enabled;
        let raw: [i16; 6] = app.space_mouse.raw.into();
        // Frames come on input, so a reading that moved since the last one gets a frame to show it.
        let seen = ui.id().with("space_mouse_meter_raw");
        let before = ui.data_mut(|data| data.get_temp::<[i16; 6]>(seen));
        ui.data_mut(|data| data.insert_temp(seen, raw));
        if before.is_some_and(|before| before != raw) {
            ui.ctx().request_repaint();
        }
        let calibration = app.space_mouse.device.as_ref().and_then(|device| app.preferences.space_mouse_calibrations.get(device));
        let meters: [_; 6] = std::array::from_fn(|axis| axis_meter(calibration, raw, app.preferences.space_mouse_axis_source, axis));
        ui_kit::settings_axis_header(ui);
        for (axis, (name, off)) in rows.iter().enumerate() {
            ui.push_id(("space_mouse_axis", axis), |ui| {
                let (value, zone) = meters[axis];
                let prefs = &mut app.preferences;
                let (reverse, speed, source) =
                    (prefs.space_mouse_reverse.get_mut(axis), prefs.space_mouse_axis_speed.get_mut(axis), &mut prefs.space_mouse_axis_source);
                ui_kit::settings_axis_row(
                    ui,
                    name,
                    value,
                    zone,
                    // Invert and speed do nothing on a row the driver turns off. The motion choice
                    // stays live, so a motion can still be swapped out of it.
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
        ui_kit::row(ui, "", |ui| {
            let reassigned = app.preferences.space_mouse_axis_source != crate::space_mouse::SAME_AXES;
            ui.add_enabled_ui(app.preferences.space_mouse_enabled && reassigned, |ui| {
                let response = ui_kit::button(ui, app.tr("ui.3d_mouse_reset_axes"), ui_kit::Button::Normal);
                crate::scenario::publish_target("prefs:space_mouse_reset_axes", response.rect);
                if response.clicked() {
                    app.preferences.space_mouse_axis_source = crate::space_mouse::SAME_AXES;
                }
            });
        });
    });
}

fn advanced(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "space_mouse_advanced", app.tr("Advanced"), |ui| {
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            // Only where the reader has seen the choice: Linux, with spacenavd and the device both there.
            if let Some(present) = app.space_mouse.drivers.filter(|present| present.spacenavd || (present.direct && !present.denied)) {
                let [label, auto, direct, only_spacenavd, only_direct] =
                    ["driver", "driver_auto", "driver_direct", "driver_only_spacenavd", "driver_only_direct"]
                        .map(|key| app.tr(&format!("ui.3d_mouse_{key}")).to_owned());
                let driver = &mut app.preferences.space_mouse_driver;
                ui_kit::row(ui, &label, |ui| match (present.spacenavd, present.direct) {
                    (true, true) => {
                        let name = |driver| match driver {
                            SpaceMouseDriver::Automatic => auto.as_str(),
                            SpaceMouseDriver::Spacenavd => "spacenavd",
                            SpaceMouseDriver::Direct => direct.as_str(),
                        };
                        ui_kit::settings_control_width(ui, Tokens::get().space_24 * 8.0 + Tokens::get().space_8, |ui| {
                            ui_kit::dropdown_menu(ui, "sc_space_mouse_driver", name(*driver), |ui| {
                                for choice in SpaceMouseDriver::ALL {
                                    if ui_kit::list_picker(ui, name(choice), name(choice), *driver == choice).clicked() {
                                        *driver = choice;
                                    }
                                }
                            });
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
            // The setting says what was asked for; this says what is read when that is not it: Direct or
            // Automatic with the daemon read because the device itself could not be opened.
            if let Some(present) = app.space_mouse.drivers
                && present.spacenavd
                && present.direct
                && app.preferences.space_mouse_driver != SpaceMouseDriver::Spacenavd
                && app.space_mouse.device.as_deref() == Some("spacenavd")
            {
                hint_under_controls(ui, app.tr("ui.3d_mouse_driver_fallback"), "space_mouse_driver_fallback");
            }
            let enabled = app.preferences.space_mouse_enabled;
            let (label, tip) = (app.tr("ui.3d_mouse_dead_zone").to_owned(), app.tr("ui.3d_mouse_dead_zone_tip").to_owned());
            let device = app.space_mouse.device.clone();
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
            }
        });

        nav_bridge(app, ui);
        ui_kit::settings_form(ui, FORM_LABEL_WIDTH, |ui| {
            // Restore this section's settings (after asking); the calibration, the button mapping and the
            // test model are not touched.
            if app.space_mouse.restore_pending {
                // A short question on its own, not in a row (a row has one line's height and cut it off), so
                // that it wraps to its whole height at any width; the buttons are under it.
                hint_under_controls(ui, app.tr("ui.3d_mouse_restore_ask"), "space_mouse_restore_ask");
                ui_kit::row(ui, "", |ui| {
                    if ui_kit::button(ui, app.tr("ui.3d_mouse_restore"), ui_kit::Button::Normal).clicked() {
                        restore_space_mouse_defaults(app);
                        app.space_mouse.restore_pending = false;
                    }
                    if ui_kit::button(ui, app.tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                        app.space_mouse.restore_pending = false;
                    }
                });
            } else {
                ui_kit::row(ui, "", |ui| {
                    if ui_kit::button(ui, app.tr("ui.3d_mouse_restore"), ui_kit::Button::Normal).clicked() {
                        app.space_mouse.restore_pending = true;
                    }
                });
            }
        });

        // The calibrated device moving as it does, with the test model beside it, then its dead zones.
        let (device, raw) = (app.space_mouse.device.clone(), app.space_mouse.raw.into());
        let (boat, motion, free_orbit) = (&mut app.space_mouse.boat, app.space_mouse.motion, app.preferences.free_orbit);
        if let Some(calibration) = device.as_ref().and_then(|device| app.preferences.space_mouse_calibrations.get_mut(device)) {
            ui_kit::divider(ui, "space_mouse_picture");
            // Under each circle's axes, what the view does with them: the rows' own labels.
            let stories = crate::space_mouse::stories(&app.preferences.language, app.space_mouse.mapping, app.preferences.space_mouse_axis_source);
            let help = crate::i18n::tr("ui.3d_mouse_circles_help").to_string();
            let pads = |ui: &mut egui::Ui| {
                crate::space_mouse_calibration::dead_zone_pads(ui, calibration, raw, false, &stories);
                ui_kit::text(ui, &help, ui_kit::Text::Hint("space_mouse_circles_help"));
            };
            picture_and_boat(ui, boat, motion, free_orbit, true, |ui| crate::space_mouse_model::live(ui, calibration, raw), pads);
        } else if device.is_some() {
            // Not calibrated: the test model alone.
            ui_kit::divider(ui, "space_mouse_picture");
            picture_and_boat(ui, boat, motion, free_orbit, false, |_| {}, |_| {});
        }
    });
}
