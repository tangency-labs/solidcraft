//! Navigation preferences, composed from indexed shared controls and canonical figures.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_page(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui_kit::settings_heading_with_tag(
            ui,
            app.tr("prefs.navigation"),
            app.tr("prefs.structure.navigation_note"),
            Some(crate::settings_profiles::tag),
        );
        mouse(app, ui, t);
        orbit(app, ui, t);
        wheel(app, ui, t);
        view(app, ui, t);
    });
}

/// The legend follows the runtime preset, ring ownership, wheel anchor and right-orbit switch.
pub(super) fn mapping_rows(app: &SolveApp) -> Vec<(Vec<String>, String)> {
    let mut gestures: Vec<_> = nav_preset(&app.preferences.nav)
        .bindings
        .iter()
        .filter(|binding| !(crate::mouse_gesture::active(app) && binding.button == NavButton::Right))
        .filter(|binding| app.preferences.right_drag_orbits || !(binding.button == NavButton::Right && binding.mode == NavMode::Orbit))
        .map(|binding| {
            let button = match binding.button {
                NavButton::Left => "prefs.button_left",
                NavButton::Middle => "prefs.button_middle",
                NavButton::Right => "prefs.button_right",
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
    if app.preferences.nav == TOUCHPAD_NAV {
        gestures.extend([
            (vec![app.tr("prefs.shift").to_owned()], app.tr("prefs.gesture_pan").to_owned()),
            (vec!["Alt/Option".to_owned()], app.tr("prefs.gesture_orbit").to_owned()),
        ]);
    }
    gestures
}

fn mouse(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_section(ui, "mouse", app.tr("prefs.mouse_heading"), |ui| {
        let help = if app.preferences.nav == TOUCHPAD_NAV {
            app.tr("nav.touchpad.help").to_owned()
        } else if !app.preferences.free_orbit && app.preferences.nav != OPENSCAD_NAV {
            format!("{} {}", app.tr("prefs.extra_navigation"), app.tr("prefs.orbit_snap"))
        } else {
            app.tr("prefs.extra_navigation").to_owned()
        };
        let gestures = mapping_rows(app);
        ui_kit::settings_mouse_mapping(ui, &gestures, |ui| {
            let width = ui_kit::ContentWidth::label(ui, app.tr("prefs.mouse_mapping")) + t.space_16 + t.space_4;
            ui_kit::settings_form(ui, width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.mouse_mapping"), &help, |ui| {
                    ui_kit::settings_control_width(ui, t.space_24 * 9.0 + t.space_4, |ui| {
                        let response = ui_kit::dropdown_menu(ui, "pref_nav", nav_preset(&app.preferences.nav).translated_label(), |ui| {
                            for preset in NAV_PRESETS {
                                if ui_kit::list_picker(ui, preset.id, preset.translated_label(), app.preferences.nav == preset.id)
                                    .on_hover_text(preset.translated_help())
                                    .clicked()
                                {
                                    choose_nav_preset(&mut app.preferences, preset);
                                }
                            }
                        });
                        crate::scenario::publish_target("dialog:Pan, zoom, orbit", response.rect);
                        crate::scenario::publish_target("prefs:nav", response.rect);
                        response.on_hover_text(nav_preset(&app.preferences.nav).translated_help());
                    });
                });
            });
        });
        if nav_preset(&app.preferences.nav).bindings.iter().any(|b| b.button == NavButton::Right && b.mode == NavMode::Orbit) {
            let response = setting::right_drag_orbits(app, ui).on_hover_text(crate::i18n::tr("nav.right_drag_orbits.help"));
            crate::scenario::publish_target("prefs:right_drag_orbits", response.rect);
        }
        setting::radial_context_menu(app, ui).on_hover_text(crate::i18n::tr("prefs.navigation.radial_help"));
        let label = app.tr("Middle double-click");
        let width = ui_kit::ContentWidth::label(ui, label);
        ui_kit::settings_form(ui, width, |ui| {
            ui_kit::row(ui, label, |ui| {
                let current = MiddleDoubleClick::of(app);
                ui_kit::settings_control_width(ui, t.space_24 * 9.0 + t.space_4, |ui| {
                    let response = ui_kit::dropdown_menu(ui, "pref_middle_double_click", crate::i18n::tr_label(current.label()), |ui| {
                        for (mode, id, label) in MiddleDoubleClick::ALL {
                            if ui_kit::list_picker(ui, id, crate::i18n::tr_label(label), current == mode).clicked() {
                                app.preferences.middle_double_click = id.into();
                            }
                        }
                    });
                    crate::scenario::publish_target("prefs:middle_double_click", response.rect);
                    response.on_hover_text(crate::i18n::tr_label("Double-clicking the middle button in the view: Home (the home view, framing what is shown), Fit (frame what is shown, keeping the orientation) or nothing"));
                });
            });
        });
        crate::mouse_gesture::nav_rows(app, ui, t);
    });
}

fn orbit(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_section(ui, "orbit_pivot", app.tr("ui.orbit_around"), |ui| {
        let options = [
            ui_kit::PictureChoice { key: "Origin", label: app.tr("ui.origin"), figure: ui_kit::paint_orbit_origin },
            ui_kit::PictureChoice { key: "View center", label: app.tr("ui.view_centre"), figure: ui_kit::paint_orbit_centre },
            ui_kit::PictureChoice { key: "Under the cursor", label: app.tr("prefs.under_cursor"), figure: ui_kit::paint_orbit_cursor },
        ];
        let selected = match app.preferences.orbit_around {
            OrbitAround::Origin => 0,
            OrbitAround::ViewCentre => 1,
            OrbitAround::Cursor => 2,
        };
        if let Some(index) = ui_kit::settings_picture_choices_sized(
            ui,
            "orbit_around",
            app.tr("ui.orbit_around"),
            &options,
            selected,
            true,
            ui_kit::PictureChoiceSize { width: t.space_24 * 8.0 + t.space_4, aspect: 184.0 / 99.5, caption: false },
        ) {
            app.preferences.orbit_around = match index {
                0 => OrbitAround::Origin,
                2 => OrbitAround::Cursor,
                _ => OrbitAround::ViewCentre,
            };
        }
        for option in &options {
            if let Some(rect) = crate::scenario::rect_of(&format!("prefs:orbit_around:{}", option.key)) {
                crate::scenario::publish_target(&format!("prefs:option:{}", option.label), rect);
            }
        }
    });
    ui_kit::settings_section(ui, "orbit_style", app.tr("prefs.navigation.orbit_style"), |ui| {
        let label = app.tr("ui.orbit");
        let help = app.tr("prefs.navigation.free_help");
        let width = ui_kit::ContentWidth::label(ui, label) + t.space_16 + t.space_4;
        ui_kit::settings_form(ui, width, |ui| {
            ui_kit::settings_row_help(ui, label, help, |ui| {
                let options = [crate::i18n::tr("prefs.navigation.turntable"), crate::i18n::tr("prefs.navigation.free")];
                crate::prefs_search::note_current(ui, &options.join(" "));
                let mut selected = usize::from(app.preferences.free_orbit);
                let response = ui.scope(|ui| {
                    ui_kit::segmented(
                        ui,
                        &mut selected,
                        &options,
                        ui_kit::Segments { intrinsic: true, targets: &["prefs:turntable", "prefs:free_orbit"], ..Default::default() },
                    )
                });
                if response.inner {
                    app.preferences.free_orbit = selected == 1;
                }
                if !crate::prefs_search::texts_hit(ui, &[label]) {
                    for option in options {
                        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{label}:{option}")) {
                            crate::prefs_search::remember(ui, option, rect);
                            crate::prefs_search::follow(ui, option, rect);
                        }
                    }
                }
                response.response.on_hover_text(crate::i18n::tr("nav.free_orbit.snap_help"));
            });
        });
        ui.add_enabled_ui(app.preferences.free_orbit, |ui| {
            let response = setting::free_orbit_upright(app, ui).on_hover_text(crate::i18n::tr("nav.free_orbit_upright.help"));
            crate::scenario::publish_target("prefs:free_orbit_upright", response.rect);
        });
    });
}

fn wheel(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_section(ui, "wheel_trackpad", app.tr("prefs.wheel_trackpad"), |ui| {
        let available = ui.available_width();
        let gap = t.space_24 + t.space_8;
        let natural = [
            ["ui.zoom_to_the_cursor", "ui.reverse_zoom_direction", "ui.reverse_horizontal_pan_direction", "ui.reverse_vertical_pan_direction"],
            ["ui.smooth_wheel_zoom", "ui.two_finger_scroll_pans", "ui.reverse_trackpad_orbit_direction", "prefs.zoom_stop_at_model"],
        ]
        .map(|labels| labels.into_iter().map(|id| ui_kit::ContentWidth::label(ui, app.tr(id)) + t.space_24).fold(0.0, f32::max));
        let beside = available >= natural[0] + natural[1] + gap;
        let left = if beside { (available - gap) * natural[0] / (natural[0] + natural[1]).max(1.0) } else { available };
        let right = if beside { available - gap - left } else { available };
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                setting::zoom_to_cursor(app, ui);
                setting::zoom_reverse(app, ui);
                setting::pan_reverse_x(app, ui);
                setting::pan_reverse_y(app, ui);
            });
            ui.allocate_ui_with_layout(egui::vec2(right, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                setting::smooth_wheel_zoom(app, ui);
                let touchpad = app.preferences.nav == TOUCHPAD_NAV;
                ui.add_enabled_ui(!touchpad, |ui| {
                    let response = ui_kit::checkbox(ui, crate::i18n::tr("ui.two_finger_scroll_pans"), &mut app.preferences.trackpad_pan)
                        .on_hover_text(if touchpad { crate::i18n::tr("nav.touchpad.scroll") } else { crate::i18n::tr("ui.trackpad_pan_tip") });
                    crate::scenario::publish_target("prefs:trackpad_pan", response.rect);
                });
                setting::orbit_reverse(app, ui);
                let response = setting::zoom_stop_at_model(app, ui).on_hover_text(crate::i18n::tr("prefs.zoom_stop_at_model_tip"));
                crate::scenario::publish_target("prefs:zoom_stop_at_model", response.rect);
            });
        });
        let label_width = ["ui.wheel_zoom_speed", "prefs.trackpad_zoom_modifier"]
            .into_iter()
            .map(|id| ui_kit::ContentWidth::label(ui, app.tr(id)))
            .fold(0.0, f32::max);
        ui_kit::settings_form(ui, label_width, |ui| {
            setting::wheel_zoom_speed(app, ui);
            ui_kit::row(ui, app.tr("prefs.trackpad_zoom_modifier"), |ui| {
                let platform = crate::tool_modifiers::Platform::of(ui.ctx());
                let choice = &mut app.preferences.trackpad_zoom_modifier;
                ui_kit::settings_control_width(ui, t.space_24 * 7.0 + t.space_12, |ui| {
                    let response = ui_kit::dropdown_menu(ui, "pref_trackpad_zoom_modifier", crate::i18n::tr(choice.label_id(platform)), |ui| {
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
                    crate::scenario::publish_target("prefs:trackpad_zoom_modifier", response.rect);
                });
            });
        });
    });
}

fn view(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_section(ui, "navigation_view", app.tr("ui.tab_view"), |ui| {
        let options = [
            ui_kit::PictureChoice { key: "Signed axes", label: app.tr("prefs.navigation.axes_signed"), figure: ui_kit::paint_navigation_axes },
            ui_kit::PictureChoice { key: "Cube", label: app.tr("prefs.navigation.cube_compact"), figure: ui_kit::paint_navigation_cube },
        ];
        if let Some(index) = ui_kit::settings_picture_choices_sized(
            ui,
            "navigator",
            app.tr("View navigator"),
            &options,
            usize::from(app.preferences.navigator_style == NavigatorStyle::Cube),
            true,
            ui_kit::PictureChoiceSize { width: t.space_24 * 8.0 - t.space_2, aspect: 178.0 / 115.0, caption: true },
        ) {
            app.preferences.navigator_style = if index == 0 { NavigatorStyle::Axes } else { NavigatorStyle::Cube };
        }
        if let (Some(a), Some(b)) = (crate::scenario::rect_of("prefs:navigator:Signed axes"), crate::scenario::rect_of("prefs:navigator:Cube")) {
            crate::scenario::publish_target("prefs:navigator", a.union(b));
        }
        setting::look_at_anim(app, ui);
        let width =
            ["ui.animation_speed", "prefs.home_fit_size"].into_iter().map(|id| ui_kit::ContentWidth::label(ui, app.tr(id))).fold(0.0, f32::max);
        ui_kit::settings_form(ui, width, |ui| {
            setting::look_at_speed(app, ui);
            let (label, note) = (app.tr("prefs.home_fit_size").to_owned(), app.tr("prefs.home_fit_size_note").to_owned());
            ui_kit::row(ui, &label, |ui| {
                let response = ui_kit::slider(
                    ui,
                    &mut app.preferences.home_fit_size,
                    0.1..=1.0,
                    ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                );
                crate::scenario::publish_rect("prefs:home_fit_size", response.rect);
                response.on_hover_text(note);
            });
        });
        let (label, note) = (app.tr("prefs.navigator_fit").to_owned(), app.tr("prefs.navigator_fit_note").to_owned());
        let response = ui_kit::checkbox(ui, &label, &mut app.preferences.navigator_fit).on_hover_text(note);
        crate::scenario::publish_target("prefs:navigator_fit", response.rect);
        let (label, note) = (app.tr("prefs.invert_navigator_arrows").to_owned(), app.tr("prefs.invert_navigator_arrows_note").to_owned());
        let response = ui_kit::checkbox(ui, &label, &mut app.preferences.invert_navigator_arrows).on_hover_text(note);
        crate::scenario::publish_target("prefs:invert_navigator_arrows", response.rect);
        let (label, note) = (app.tr("prefs.invert_curved_roll_arrows").to_owned(), app.tr("prefs.invert_curved_roll_arrows_note").to_owned());
        let response = ui_kit::checkbox(ui, &label, &mut app.preferences.invert_curved_roll_arrows).on_hover_text(note);
        crate::scenario::publish_target("prefs:invert_curved_roll_arrows", response.rect);
    });
}
