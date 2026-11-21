//! The Navigation page of Preferences: the mouse mapping, Orbit around, the orbit style, the wheel
//! and trackpad settings and the view settings. The page reads and writes the host's
//! [`NavigationSettings`] through [`PrefsHost`]; it publishes its controls to the widget registry
//! like every kit page.
use crate::prefs_navigation::*;
use crate::{BackingTooltipExt as _, NavigatorAxis, settings_search, view_compass};
use egui::{Rect, Ui};
use solidcraft_theme::Tokens;
use solidcraft_ui_registry as registry;

/// What the Navigation page needs from its host: the settings, read and written as a whole, and
/// the two parts of the page the host draws itself.
pub trait PrefsHost {
    /// The settings as they are now.
    fn navigation(&self) -> NavigationSettings;
    /// Replaces the settings with `settings` (the page calls this only when a control changed them).
    fn set_navigation(&mut self, settings: NavigationSettings);
    /// A tag drawn at the page heading, if the host has one.
    fn heading_tag(&self) -> Option<fn(&mut Ui)> {
        None
    }
    /// Rows the host draws in the Mouse section, below the mapping's bindings (SolidCraft's mouse
    /// gestures). They edit `settings`, the page's draft, so the host's edits and the kit
    /// controls' reach the host in the one write at the end of the frame.
    fn mouse_gesture_rows(&mut self, _settings: &mut NavigationSettings, _ui: &mut Ui) {}
}

/// Restore Defaults for the Navigation page.
pub fn navigation_defaults<H: PrefsHost>(host: &mut H) {
    host.set_navigation(NavigationSettings::default());
}

/// Draws the Navigation page. The settings are read once, edited on a copy, and written back only
/// when the copy differs, so the host sees one change per frame at most.
pub fn navigation_page<H: PrefsHost>(ui: &mut Ui, host: &mut H) {
    let before = host.navigation();
    let mut s = before.clone();
    let t = Tokens::get();
    crate::settings_page(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        crate::settings_heading_with_tag(ui, text("prefs.navigation"), text("prefs.structure.navigation_note"), host.heading_tag());
        mouse(&mut s, host, ui, &t);
        orbit(&mut s, ui, &t);
        wheel(&mut s, ui, &t);
        view(&mut s, ui, &t);
    });
    if s != before {
        host.set_navigation(s);
    }
}

/// The page's words: the catalogue's text for an id.
fn text(id: &str) -> &str {
    solidcraft_ui_text::tr(id)
}

/// The legend follows the mapping, ring ownership, wheel anchor and right-orbit switch.
pub fn mapping_rows(s: &NavigationSettings) -> Vec<(Vec<String>, String)> {
    let mut gestures: Vec<_> = nav_preset(&s.nav)
        .bindings
        .iter()
        .filter(|binding| !(s.gestures_active() && binding.button == NavButton::Right))
        .filter(|binding| s.right_drag_orbits || !(binding.button == NavButton::Right && binding.mode == NavMode::Orbit))
        .map(|binding| {
            let button = match binding.button {
                NavButton::Left => "prefs.button_left",
                NavButton::Middle => "prefs.button_middle",
                NavButton::Right => "prefs.button_right",
            };
            let mut keys = Vec::new();
            if binding.keys.shift {
                keys.push(text("prefs.shift"));
            }
            if binding.keys.ctrl {
                keys.push("Ctrl/Cmd");
            }
            if binding.keys.alt {
                keys.push("Alt/Option");
            }
            keys.push(text(button));
            let action = match binding.mode {
                NavMode::Orbit => "prefs.gesture_orbit",
                NavMode::Pan => "prefs.gesture_pan",
                NavMode::Zoom => "prefs.gesture_zoom",
            };
            (keys.into_iter().map(str::to_owned).collect::<Vec<_>>(), text(action).to_string())
        })
        .collect();
    gestures.extend([
        (vec![text("prefs.wheel").to_owned()], text(if s.zoom_to_cursor { "prefs.zoom_toward_cursor" } else { "prefs.gesture_zoom" }).to_owned()),
        (vec![text("prefs.left_click").to_owned()], text("prefs.select").to_owned()),
        (
            vec![text("prefs.right_click").to_owned()],
            text(if s.mouse_gestures.enabled && s.nav == MOUSE_MODE { "prefs.context_menu_ring" } else { "prefs.context_menu" }).to_owned(),
        ),
    ]);
    if s.nav == TOUCHPAD_NAV {
        gestures.extend([
            (vec![text("prefs.shift").to_owned()], text("prefs.gesture_pan").to_owned()),
            (vec!["Alt/Option".to_owned()], text("prefs.gesture_orbit").to_owned()),
        ]);
    }
    gestures
}

/// Whether this platform is a Mac, for the labels that name Cmd.
fn is_mac(ui: &Ui) -> bool {
    matches!(ui.ctx().os(), egui::os::OperatingSystem::Mac | egui::os::OperatingSystem::IOS)
}

/// A checkbox with the help the page gives it on hover (if any), published under `target` (if any).
fn check(ui: &mut Ui, label: &str, value: &mut bool, help: Option<&str>, target: Option<&str>) {
    let response = crate::checkbox(ui, label, value);
    let response = match help {
        Some(help) => response.backing_on_hover_text(help),
        None => response,
    };
    if let Some(target) = target {
        registry::publish_target(target, response.rect);
    }
}

fn mouse(s: &mut NavigationSettings, host: &mut impl PrefsHost, ui: &mut Ui, t: &Tokens) {
    crate::settings_section(ui, "mouse", text("prefs.mouse_heading"), |ui| {
        let help = if s.nav == TOUCHPAD_NAV {
            text("nav.touchpad.help").to_owned()
        } else if !s.free_orbit && s.nav != OPENSCAD_NAV {
            format!("{} {}", text("prefs.extra_navigation"), text("prefs.orbit_snap"))
        } else {
            text("prefs.extra_navigation").to_owned()
        };
        let gestures = mapping_rows(s);
        crate::settings_mouse_mapping(ui, &gestures, |ui| {
            let width = crate::ContentWidth::label(ui, text("prefs.mouse_mapping")) + t.space_16 + t.space_4;
            crate::settings_form(ui, width, |ui| {
                crate::settings_row_help(ui, text("prefs.mouse_mapping"), &help, |ui| {
                    crate::settings_control_width(ui, t.space_24 * 9.0 + t.space_4, |ui| {
                        let response = crate::dropdown_menu(ui, "pref_nav", nav_preset(&s.nav).translated_label(), |ui| {
                            for preset in NAV_PRESETS {
                                if crate::list_picker(ui, preset.id, preset.translated_label(), s.nav == preset.id)
                                    .backing_on_hover_text(preset.translated_help())
                                    .clicked()
                                {
                                    s.choose_preset(preset);
                                }
                            }
                        });
                        registry::publish_target("dialog:Pan, zoom, orbit", response.rect);
                        registry::publish_target("prefs:nav", response.rect);
                        response.backing_on_hover_text(nav_preset(&s.nav).translated_help());
                    });
                });
            });
        });
        let touch =
            crate::checkbox(ui, text("nav.touchscreen.label"), &mut s.touchscreen_navigation).backing_on_hover_text(text("nav.touchscreen.help"));
        registry::publish_target("prefs:touchscreen_navigation", touch.rect);
        if s.nav == "maya" {
            let axis = crate::checkbox(ui, text("nav.maya_horizontal_zoom.label"), &mut s.maya_horizontal_zoom)
                .backing_on_hover_text(text("nav.maya_horizontal_zoom.help"));
            registry::publish_target("prefs:maya_horizontal_zoom", axis.rect);
        }
        if nav_preset(&s.nav).bindings.iter().any(|b| b.button == NavButton::Right && b.mode == NavMode::Orbit) {
            check(
                ui,
                text("nav.right_drag_orbits.label"),
                &mut s.right_drag_orbits,
                Some(text("nav.right_drag_orbits.help")),
                Some("prefs:right_drag_orbits"),
            );
        }
        check(
            ui,
            text("nav.drag_keeps_mode.label"),
            &mut s.nav_drag_keeps_mode,
            Some(text("nav.drag_keeps_mode.help")),
            Some("prefs:nav_drag_keeps_mode"),
        );
        check(
            ui,
            text("nav.orbit_cursor_wrap.label"),
            &mut s.orbit_cursor_wrap,
            Some(text("nav.orbit_cursor_wrap.help")),
            Some("prefs:orbit_cursor_wrap"),
        );
        check(
            ui,
            text("nav.upright_orbit_unrolls.label"),
            &mut s.upright_orbit_unrolls,
            Some(text("nav.upright_orbit_unrolls.help")),
            Some("prefs:upright_orbit_unrolls"),
        );
        check(
            ui,
            text("nav.mark_orbit_point.label"),
            &mut s.mark_orbit_point,
            Some(text("nav.mark_orbit_point.help")),
            Some("prefs:mark_orbit_point"),
        );
        crate::checkbox(ui, text("Radial context menu"), &mut s.radial_context_menu).backing_on_hover_text(text("prefs.navigation.radial_help"));
        crate::checkbox(ui, text("prefs.quick_menu_after_selection"), &mut s.quick_menu_after_selection);
        let label = text("prefs.middle_double_click");
        let width = crate::ContentWidth::label(ui, label);
        crate::settings_form(ui, width, |ui| {
            crate::row(ui, label, |ui| {
                let current = MiddleDoubleClick::of_id(&s.middle_double_click);
                crate::settings_control_width(ui, t.space_24 * 9.0 + t.space_4, |ui| {
                    let response = crate::dropdown_menu(ui, "pref_middle_double_click", solidcraft_ui_text::tr_label(current.label()), |ui| {
                        for (mode, id, label) in MiddleDoubleClick::ALL {
                            if crate::list_picker(ui, id, solidcraft_ui_text::tr_label(label), current == mode).clicked() {
                                s.middle_double_click = id.into();
                            }
                        }
                    });
                    registry::publish_target("prefs:middle_double_click", response.rect);
                    response.backing_on_hover_text(solidcraft_ui_text::tr_label(
                        "Double-clicking the middle button in the view: Home (the home view, framing what is shown), Fit (frame what is shown, keeping the orientation) or nothing",
                    ));
                });
            });
        });
        host.mouse_gesture_rows(s, ui);
    });
}

fn orbit(s: &mut NavigationSettings, ui: &mut Ui, t: &Tokens) {
    crate::settings_section(ui, "orbit_pivot", text("ui.orbit_around"), |ui| {
        let options = [
            crate::PictureChoice { key: "Origin", label: text("ui.origin"), figure: crate::paint_orbit_origin },
            crate::PictureChoice { key: "View center", label: text("ui.view_centre"), figure: crate::paint_orbit_centre },
            crate::PictureChoice { key: "Under the cursor", label: text("prefs.under_cursor"), figure: crate::paint_orbit_cursor },
        ];
        let selected = match s.orbit_around {
            OrbitAround::Origin => 0,
            OrbitAround::ViewCentre => 1,
            OrbitAround::Cursor => 2,
        };
        if let Some(index) = crate::settings_picture_choices_sized(
            ui,
            "orbit_around",
            text("ui.orbit_around"),
            &options,
            selected,
            true,
            crate::PictureChoiceSize { width: t.space_24 * 8.0 + t.space_4, aspect: 120.0 / 64.0, caption: false },
        ) {
            s.orbit_around = match index {
                0 => OrbitAround::Origin,
                2 => OrbitAround::Cursor,
                _ => OrbitAround::ViewCentre,
            };
        }
        for option in &options {
            if let Some(rect) = registry::rect_of(&format!("prefs:orbit_around:{}", option.key)) {
                registry::publish_target(&format!("prefs:option:{}", option.label), rect);
            }
        }
    });
    crate::settings_section(ui, "orbit_style", text("prefs.navigation.orbit_style"), |ui| {
        let label = text("ui.orbit");
        let help = text("nav.free_orbit.snap_help");
        let width = crate::ContentWidth::label(ui, label) + t.space_16 + t.space_4;
        crate::settings_form(ui, width, |ui| {
            crate::settings_row_help(ui, label, help, |ui| {
                let options = [solidcraft_ui_text::tr("prefs.navigation.turntable"), solidcraft_ui_text::tr("prefs.navigation.free")];
                settings_search::note_choices(ui, &options);
                let mut selected = usize::from(s.free_orbit);
                let response = ui.scope(|ui| {
                    crate::segmented(
                        ui,
                        &mut selected,
                        &options,
                        crate::Segments { intrinsic: true, targets: &["prefs:turntable", "prefs:free_orbit"], ..Default::default() },
                    )
                });
                if response.inner {
                    s.free_orbit = selected == 1;
                }
                if !settings_search::texts_hit(ui, &[label]) {
                    for option in options {
                        if let Some(rect) = registry::rect_of(&format!("dialog:{label}:{option}")) {
                            settings_search::remember(ui, option, rect);
                            settings_search::follow(ui, option, rect);
                        }
                    }
                }
            });
        });
        ui.add_enabled_ui(s.free_orbit, |ui| {
            check(
                ui,
                text("nav.free_orbit_upright.label"),
                &mut s.free_orbit_upright,
                Some(text("nav.free_orbit_upright.help")),
                Some("prefs:free_orbit_upright"),
            );
        });
    });
}

fn wheel(s: &mut NavigationSettings, ui: &mut Ui, t: &Tokens) {
    crate::settings_section(ui, "wheel_trackpad", text("prefs.wheel_trackpad"), |ui| {
        let available = ui.available_width();
        let gap = t.space_24 + t.space_8;
        let natural = [
            ["ui.zoom_to_the_cursor", "ui.reverse_zoom_direction", "ui.reverse_horizontal_pan_direction", "ui.reverse_vertical_pan_direction"],
            ["ui.smooth_wheel_zoom", "ui.two_finger_scroll_pans", "ui.reverse_trackpad_orbit_direction", "prefs.zoom_stop_at_model"],
        ]
        .map(|labels| labels.into_iter().map(|id| crate::ContentWidth::label(ui, text(id)) + t.space_24).fold(0.0, f32::max));
        let beside = available >= natural[0] + natural[1] + gap;
        let left = if beside { (available - gap) * natural[0] / (natural[0] + natural[1]).max(1.0) } else { available };
        let right = if beside { available - gap - left } else { available };
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                check(ui, &tr_label("Zoom to the cursor"), &mut s.zoom_to_cursor, Some(&tr_label(ZOOM_TO_CURSOR_HELP)), None);
                check(ui, &tr_label("Reverse zoom direction"), &mut s.zoom_reverse, None, None);
                check(ui, &tr_label("ui.reverse_horizontal_pan_direction"), &mut s.pan_reverse_x, Some(&tr_label(PAN_X_HELP)), None);
                check(ui, &tr_label("ui.reverse_vertical_pan_direction"), &mut s.pan_reverse_y, Some(&tr_label(PAN_Y_HELP)), None);
            });
            ui.allocate_ui_with_layout(egui::vec2(right, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                check(ui, &tr_label("Smooth wheel zoom"), &mut s.smooth_wheel_zoom, Some(&tr_label(SMOOTH_HELP)), None);
                let touchpad = s.nav == TOUCHPAD_NAV;
                ui.add_enabled_ui(!touchpad, |ui| {
                    let response = crate::checkbox(ui, text("ui.two_finger_scroll_pans"), &mut s.trackpad_pan);
                    let response = if touchpad {
                        response.backing_on_disabled_hover_text(text("nav.touchpad.scroll"))
                    } else {
                        response.backing_on_hover_text(text("ui.trackpad_pan_tip"))
                    };
                    registry::publish_target("prefs:trackpad_pan", response.rect);
                });
                check(ui, &tr_label("ui.reverse_trackpad_orbit_direction"), &mut s.orbit_reverse, Some(&tr_label(ORBIT_HELP)), None);
                crate::checkbox(ui, text("prefs.trackpad_twist_turns"), &mut s.trackpad_twist_turns)
                    .backing_on_hover_text(text("prefs.trackpad_twist_turns_tip"));
                let response = crate::checkbox(ui, text("prefs.zoom_stop_at_model"), &mut s.zoom_stop_at_model)
                    .backing_on_hover_text(text("prefs.zoom_stop_at_model_tip"));
                registry::publish_target("prefs:zoom_stop_at_model", response.rect);
            });
        });
        let label_width = ["ui.wheel_zoom_speed", "prefs.trackpad_zoom_modifier"]
            .into_iter()
            .map(|id| crate::ContentWidth::label(ui, text(id)))
            .fold(0.0, f32::max);
        crate::settings_form(ui, label_width, |ui| {
            crate::row(ui, text("ui.wheel_zoom_speed"), |ui| {
                let response = crate::slider(
                    ui,
                    &mut s.wheel_zoom_speed,
                    *WHEEL_ZOOM_SPEED.start()..=*WHEEL_ZOOM_SPEED.end(),
                    crate::Slider { step: Some(0.05), format: |v| format!("{v:.2}×"), ..Default::default() },
                );
                registry::publish_rect("prefs:wheel_zoom_speed", response.rect);
                response
            });
            crate::row(ui, text("prefs.trackpad_zoom_modifier"), |ui| {
                let mac = is_mac(ui);
                let choice = &mut s.trackpad_zoom_modifier;
                crate::settings_control_width(ui, t.space_24 * 7.0 + t.space_12, |ui| {
                    let response = crate::dropdown_menu(ui, "pref_trackpad_zoom_modifier", text(choice.label_id(mac)), |ui| {
                        for (index, value) in TrackpadZoomModifier::ALL.into_iter().enumerate() {
                            if crate::list_picker(ui, &format!("trackpad_zoom_modifier_{index}"), text(value.label_id(mac)), *choice == value)
                                .clicked()
                            {
                                *choice = value;
                            }
                        }
                    })
                    .backing_on_hover_text(text("prefs.trackpad_zoom_modifier.help"));
                    registry::publish_target("prefs:trackpad_zoom_modifier", response.rect);
                });
            });
        });
    });
}

/// The help under the wheel and pan settings, as the page's catalogue and English words give them.
const ZOOM_TO_CURSOR_HELP: &str = "The wheel zooms toward the point under the cursor (off: toward the view's center)";
const SMOOTH_HELP: &str = "Turn off to apply each wheel event immediately, without a smoothing tail";
const PAN_X_HELP: &str = "Horizontal two-finger scrolling and the thumb wheel pan the other way";
const PAN_Y_HELP: &str = "Vertical two-finger scrolling pans the other way";
const ORBIT_HELP: &str = "Shift with two-finger scrolling orbits the other way";

/// The catalogue's text for a label written in English.
fn tr_label(label: &str) -> String {
    solidcraft_ui_text::tr_label(label).to_owned()
}

/// The default view's six signed world axes, as the navigator figure poses them.
pub fn default_navigator_axes() -> [NavigatorAxis; 6] {
    let (right, up, back) = solidcraft_viewport::Camera::default().basis();
    let axes = view_compass::ViewAxes { right: right.into(), up: up.into(), back: back.into() };
    view_compass::AXES.map(|(name, direction)| {
        let ([x, y], depth) = axes.pose(direction);
        NavigatorAxis { name, direction, screen: egui::vec2(x as f32, y as f32), depth }
    })
}

/// The Signed axes card: the navigator as the default view poses it.
fn paint_signed_axes(p: &egui::Painter, rect: Rect) {
    crate::paint_navigation_axes(p, rect, &default_navigator_axes());
}

fn view(s: &mut NavigationSettings, ui: &mut Ui, t: &Tokens) {
    crate::settings_section(ui, "navigation_view", text("ui.tab_view"), |ui| {
        let options = [
            crate::PictureChoice { key: "Signed axes", label: text("prefs.navigation.axes_signed"), figure: paint_signed_axes },
            crate::PictureChoice { key: "Custom", label: solidcraft_ui_text::tr_label("Custom"), figure: crate::paint_navigation_custom },
        ];
        if let Some(index) = crate::settings_picture_choices_sized(
            ui,
            "navigator",
            text("View navigator"),
            &options,
            NavigatorStyle::ALL.iter().position(|style| *style == s.navigator_style).unwrap_or_default(),
            true,
            crate::PictureChoiceSize { width: t.space_24 * 8.0 - t.space_2, aspect: 178.0 / 115.0, caption: true },
        ) {
            s.navigator_style = NavigatorStyle::ALL.get(index).copied().unwrap_or_default();
        }
        if let Some(all) = ["Signed axes", "Custom"]
            .into_iter()
            .map(|key| registry::rect_of(&format!("prefs:navigator:{key}")))
            .collect::<Option<Vec<_>>>()
            .and_then(|rects| rects.into_iter().reduce(Rect::union))
        {
            registry::publish_target("prefs:navigator", all);
        }
        crate::checkbox(ui, &tr_label("Animate view changes"), &mut s.look_at_anim).backing_on_hover_text(tr_label("ui.look_at_anim_desc"));
        let width = ["ui.animation_speed", "prefs.home_fit_size"].into_iter().map(|id| crate::ContentWidth::label(ui, text(id))).fold(0.0, f32::max);
        crate::settings_form(ui, width, |ui| {
            crate::row(ui, &tr_label("Animation speed"), |ui| {
                ui.add_enabled_ui(s.look_at_anim, |ui| {
                    // The slider clamps what it draws; a stored speed outside its range (the
                    // validator accepts more) stays as stored until the user moves the slider.
                    let mut shown = s.look_at_speed;
                    let response = crate::slider(
                        ui,
                        &mut shown,
                        0.5..=3.0,
                        crate::Slider { step: Some(0.25), format: |v| format!("{v:.2}×"), ..Default::default() },
                    );
                    if response.changed() {
                        s.look_at_speed = shown;
                    }
                    response
                })
                .inner
            });
            let (label, note) = (text("prefs.home_fit_size").to_owned(), text("prefs.home_fit_size_note").to_owned());
            crate::row(ui, &label, |ui| {
                let response = crate::slider(
                    ui,
                    &mut s.home_fit_size,
                    *HOME_FIT_SIZE.start()..=*HOME_FIT_SIZE.end(),
                    crate::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                );
                registry::publish_rect("prefs:home_fit_size", response.rect);
                response.backing_on_hover_text(note);
            });
        });
        let (label, note) = (text("prefs.navigator_fit").to_owned(), text("prefs.navigator_fit_note").to_owned());
        let response = crate::checkbox(ui, &label, &mut s.navigator_fit).backing_on_hover_text(note);
        registry::publish_target("prefs:navigator_fit", response.rect);
        let (label, note) = (text("prefs.keep_hover_while_navigating").to_owned(), text("prefs.keep_hover_while_navigating_note").to_owned());
        let response = crate::checkbox(ui, &label, &mut s.keep_hover_while_navigating).backing_on_hover_text(note);
        registry::publish_target("prefs:keep_hover_while_navigating", response.rect);
        let (label, note) = (text("prefs.invert_navigator_arrows").to_owned(), text("prefs.invert_navigator_arrows_note").to_owned());
        let response = crate::checkbox(ui, &label, &mut s.invert_navigator_arrows).backing_on_hover_text(note);
        registry::publish_target("prefs:invert_navigator_arrows", response.rect);
        let (label, note) = (text("prefs.invert_curved_roll_arrows").to_owned(), text("prefs.invert_curved_roll_arrows_note").to_owned());
        let response = crate::checkbox(ui, &label, &mut s.invert_curved_roll_arrows).backing_on_hover_text(note);
        registry::publish_target("prefs:invert_curved_roll_arrows", response.rect);
        crate::checkbox(ui, text("prefs.browser_arrow_keys"), &mut s.browser_arrow_keys);
    });
}
