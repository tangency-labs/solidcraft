//! The 3D mouse calibration window, opened from Preferences › Navigation. It asks what kind of
//! device it is (Return to Center, or a trackball), then asks the user to leave it still and
//! to move it along each axis both ways, moving on by itself after each
//! (`solidcraft_space_mouse::Wizard`), and saves what it found for the device it started with.
//! The window closes with Preferences, and when another device (or none) is read instead.

use solidcraft_space_mouse::{Calibration, Kind, Problem, Stage, Wizard};

use crate::SolveApp;

/// What each of `solidcraft_space_mouse::STEPS` asks for, in order.
const STEP_PROMPTS: [&str; 12] = [
    "ui.3d_mouse_step_slide_right",
    "ui.3d_mouse_step_slide_left",
    "ui.3d_mouse_step_pull",
    "ui.3d_mouse_step_push",
    "ui.3d_mouse_step_press",
    "ui.3d_mouse_step_lift",
    "ui.3d_mouse_step_tilt_toward",
    "ui.3d_mouse_step_tilt_away",
    "ui.3d_mouse_step_tilt_left",
    "ui.3d_mouse_step_tilt_right",
    "ui.3d_mouse_step_twist_clockwise",
    "ui.3d_mouse_step_twist_counterclockwise",
];

/// The same for a trackball: it has nothing to tilt, so those steps aren't asked.
const TRACKBALL_PROMPTS: [&str; 12] = [
    "ui.3d_mouse_step_roll_right",
    "ui.3d_mouse_step_roll_left",
    "ui.3d_mouse_step_roll_toward",
    "ui.3d_mouse_step_roll_away",
    "ui.3d_mouse_step_ring_clockwise",
    "ui.3d_mouse_step_ring_counterclockwise",
    "",
    "",
    "",
    "",
    "ui.3d_mouse_step_spin_clockwise",
    "ui.3d_mouse_step_spin_counterclockwise",
];

/// Each kind of device, as the first step offers it.
const KINDS: [(Kind, &str); 2] = [(Kind::ReturnToCenter, "ui.3d_mouse_kind_cap"), (Kind::Trackball, "ui.3d_mouse_kind_trackball")];

/// The dead zone, as a share of full travel, shown as a percentage.
pub(crate) fn dead_zone_slider(ui: &mut egui::Ui, share: &mut f32) -> egui::Response {
    crate::ui_kit::slider(
        ui,
        share,
        0.0..=solidcraft_space_mouse::MAX_DEAD_ZONE_SHARE,
        crate::ui_kit::Slider { step: Some(0.01), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
    )
}

/// The tooltip of the circle for axes `(h, v)`: each axis's dead zone on its two sides and, where the
/// page says (not in the calibration window), the view axis it drives.
pub(crate) fn pad_tooltip(shares: &[[f32; 2]; 6], (h, v): (usize, usize), stories: &[String; 6]) -> String {
    const NAMES: [&str; 6] = ["X", "Y", "Z", "Rx", "Ry", "Rz"];
    let line = |axis: usize| {
        let percent = format!("{} −{:.0} % +{:.0} %", NAMES[axis], shares[axis][0] * 100.0, shares[axis][1] * 100.0);
        if stories[axis].is_empty() { percent } else { format!("{percent} → {}", stories[axis]) }
    };
    format!("{}\n{}", line(h), line(v))
}

/// The calibration's dead zones the way a game controller tester shows a stick: per pair of axes,
/// a circle for the full travel, the dead zone shaded inside it (each direction its own size), and
/// a dot where the cap is now, before the dead zone, so it's plain when the view starts to move.
pub(crate) fn dead_zone_pads(ui: &mut egui::Ui, calibration: &Calibration, raw: [i16; 6], centred: bool, stories: &[String; 6]) {
    const NAMES: [&str; 6] = ["X", "Y", "Z", "Rx", "Ry", "Rz"];
    // (horizontal axis, its positive way on screen, vertical axis, its positive way down), so the
    // dot moves as the cap does: slid right and pulled toward you, twisted and pressed, tilted.
    const PADS: [(usize, f32, usize, f32); 3] = [(0, 1.0, 1, 1.0), (5, 1.0, 2, 1.0), (4, -1.0, 3, -1.0)];
    const SIZE: f32 = 76.0;
    let (shares, travel, moving) = (calibration.dead_zone_shares(), calibration.travel(raw), calibration.apply(raw));
    const GAP: f32 = 12.0;
    let visuals = ui.visuals().clone();
    ui.horizontal(|ui| {
        // The three together with small gaps: centred in the calibration window, from the left edge
        // (under the device's picture) in Preferences.
        ui.spacing_mut().item_spacing.x = GAP;
        if centred {
            ui.add_space(((ui.available_width() - PADS.len() as f32 * SIZE - (PADS.len() - 1) as f32 * GAP) / 2.0).max(0.0));
        }
        for (h, h_way, v, v_way) in PADS {
            ui.vertical(|ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let (centre, radius) = (rect.center(), SIZE / 2.0 - 2.0);
                let faint = crate::theme::Tokens::get().text_dim;
                // Which side of an axis's dead zone lies along a way on screen.
                let side = |axis: usize, way: f32, screen: f32| shares[axis][usize::from(way * screen > 0.0)];
                let zone: Vec<egui::Pos2> = (0..64)
                    .map(|i| {
                        let (sin, cos) = (i as f32 / 64.0 * std::f32::consts::TAU).sin_cos();
                        let (x, y) = (side(h, h_way, cos) * cos, side(v, v_way, sin) * sin);
                        centre + egui::vec2(x, y) * radius
                    })
                    .collect();
                painter.add(egui::Shape::convex_polygon(zone, visuals.selection.bg_fill.gamma_multiply(0.35), egui::Stroke::NONE));
                painter.line_segment(
                    [centre - egui::vec2(radius, 0.0), centre + egui::vec2(radius, 0.0)],
                    egui::Stroke::new(1.0, faint.gamma_multiply(0.4)),
                );
                painter.line_segment(
                    [centre - egui::vec2(0.0, radius), centre + egui::vec2(0.0, radius)],
                    egui::Stroke::new(1.0, faint.gamma_multiply(0.4)),
                );
                painter.circle_stroke(centre, radius, egui::Stroke::new(1.0, faint));
                let dot = egui::vec2(h_way * travel[h], v_way * travel[v]);
                let dot = if dot.length() > 1.0 { dot.normalized() } else { dot };
                let live = moving[h] != 0 || moving[v] != 0;
                painter.circle_filled(centre + dot * radius, 3.5, if live { visuals.selection.stroke.color } else { faint });
                response.on_hover_text(pad_tooltip(&shares, (h, v), stories));
                // As wide as the circle, so the labels don't push the circles apart.
                ui.allocate_ui(egui::vec2(SIZE, 0.0), |ui| {
                    crate::ui_kit::text(ui, &format!("{} · {}", NAMES[h], NAMES[v]), crate::ui_kit::Text::Note("space_mouse_pad"));
                });
            });
        }
    });
}

/// The device's six raw axes as they come, so the jitter at rest is there to see: six equal
/// columns, each axis name centred on one line and its number centred under it, the group centred
/// like the dead-zone pads. Painted in one go so every name and number shares its column and baseline.
fn reading_grid(ui: &mut egui::Ui, raw: [i16; 6]) {
    const NAMES: [&str; 6] = ["X", "Y", "Z", "Rx", "Ry", "Rz"];
    const CELL: f32 = 48.0;
    let t = crate::theme::Tokens::get();
    let (name_font, number_font) = (egui::FontId::proportional(t.font_small), egui::FontId::monospace(t.font_small));
    let painter = ui.painter().clone();
    let line = |font: &egui::FontId| painter.layout_no_wrap("0".into(), font.clone(), t.text_dim).size().y;
    let (name_height, number_height) = (line(&name_font), line(&number_font));
    let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), name_height + number_height + t.space_4), egui::Sense::hover());
    crate::scenario::publish_rect("space_mouse:reading_block", area);
    let left = area.left() + ((area.width() - CELL * 6.0) / 2.0).max(0.0);
    for (i, (name, value)) in NAMES.into_iter().zip(raw).enumerate() {
        let centre = left + (i as f32 + 0.5) * CELL;
        let cell = |text: &str, font: &egui::FontId, colour: egui::Color32, top: f32, key: &str| {
            let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), colour);
            let rect = egui::Rect::from_center_size(egui::pos2(centre, top + galley.size().y / 2.0), galley.size());
            painter.galley(rect.min, galley, colour);
            crate::scenario::publish_rect(&format!("space_mouse:reading_{key}:{i}"), rect);
        };
        cell(name, &name_font, t.text_dim, area.top(), "name");
        cell(&value.to_string(), &number_font, t.form_ink, area.top() + name_height + t.space_4, "value");
    }
}

/// Starts calibrating the device being read, if there is one.
pub fn start(app: &mut SolveApp) {
    app.space_mouse.calibrating = app.space_mouse.device.clone().map(|device| (device, Wizard::choosing()));
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((_, wizard)) = &app.space_mouse.calibrating else { return };
    let (stage, kind, progress, result) = (wizard.stage(), wizard.kind(), wizard.progress(), wizard.result());
    let (can_skip, mismatch, (step_number, steps)) = (wizard.can_skip(), wizard.mismatch(), wizard.counter());
    let mut dead_zone = wizard.dead_zone_share();
    let raw: [i16; 6] = app.space_mouse.raw.into();
    let prompts = match kind {
        Kind::ReturnToCenter => &STEP_PROMPTS,
        Kind::Trackball => &TRACKBALL_PROMPTS,
    };
    let prompt = match (stage, result) {
        (Stage::Choose, _) => "ui.3d_mouse_kind_question",
        (Stage::Rest, _) if kind == Kind::Trackball => "ui.3d_mouse_step_rest_trackball",
        (Stage::Rest, _) => "ui.3d_mouse_step_rest",
        (Stage::Touch, _) => "ui.3d_mouse_step_touch",
        (Stage::Step(index), _) => prompts.get(index).copied().unwrap_or_default(),
        (Stage::Done, Some(Ok(_))) => "ui.3d_mouse_calibration_done",
        (Stage::Done, _) => "ui.3d_mouse_calibration_no_response",
    };
    // The calibrated motion that barely moved, as Preferences names it.
    let weak = match result {
        Some(Err(Problem::NoResponse(axis))) => crate::space_mouse::MOTIONS.get(axis).copied(),
        _ => None,
    };
    let counter = match stage {
        Stage::Choose | Stage::Rest | Stage::Touch => String::new(),
        Stage::Step(_) | Stage::Done => format!("{step_number} / {steps}"),
    };
    let (mut back, mut over, mut skip) = (false, false, false);
    let mut chosen = None;
    let done = matches!(result, Some(Ok(_)));
    let text = |ui: &mut egui::Ui, text: &str, key: &str| crate::ui_kit::text(ui, text, crate::ui_kit::Text::Report(key, false));
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_space_mouse_calibration"), "settings", app.tr("ui.3d_mouse_calibrate_title"))
        .actions(if done { app.tr("ui.save") } else { "" }, app.tr("ui.cancel"), None)
        .show_with_left(
            ctx,
            done,
            |ui| {
                text(ui, app.tr(prompt), "prompt");
                if let Some(axis) = weak {
                    crate::ui_kit::text(ui, app.tr(axis), crate::ui_kit::Text::Heading("weak_axis"));
                }
                // The device doesn't behave like the kind picked: say so, and offer the other.
                if let Some(other) = mismatch {
                    let (hint, switch) = match other {
                        Kind::Trackball => ("ui.3d_mouse_looks_like_trackball", "ui.3d_mouse_switch_trackball"),
                        Kind::ReturnToCenter => ("ui.3d_mouse_looks_like_cap", "ui.3d_mouse_switch_cap"),
                    };
                    crate::ui_kit::text(ui, app.tr(hint), crate::ui_kit::Text::Hint("space_mouse_kind_mismatch"));
                    if crate::ui_kit::button(ui, app.tr(switch), crate::ui_kit::Button::Normal).clicked() {
                        chosen = Some(other);
                    }
                }
                if stage == Stage::Choose {
                    // Side by side where they fit, one under the other where the window is narrow.
                    ui.horizontal_wrapped(|ui| {
                        for (kind, label) in KINDS {
                            ui.vertical(|ui| {
                                crate::space_mouse_model::rest(ui, kind);
                                if crate::ui_kit::button(ui, app.tr(label), crate::ui_kit::Button::Normal).clicked() {
                                    chosen = Some(kind);
                                }
                            });
                        }
                    });
                } else {
                    crate::ui_kit::progress(ui, "space_mouse_calibration", progress as f32);
                    crate::ui_kit::text(ui, &counter, crate::ui_kit::Text::Note("space_mouse_counter"));
                    // Each step shows the motion it asks for; once calibrated, the device's picture follows it.
                    let step = match stage {
                        Stage::Step(index) => solidcraft_space_mouse::STEPS.get(index).copied(),
                        _ => None,
                    };
                    match (step, result) {
                        (Some(step), _) => crate::space_mouse_model::demo(ui, step, kind),
                        (_, Some(Ok(calibration))) => {
                            let calibration = Calibration { dead_zone_share: dead_zone, ..calibration };
                            crate::space_mouse_model::live(ui, &calibration, raw);
                            dead_zone_pads(ui, &calibration, raw, true, &Default::default());
                        }
                        _ => crate::space_mouse_model::rest(ui, kind),
                    }
                }
                // The device's six raw axes as they come, so the jitter at rest is there to see.
                crate::ui_kit::text(ui, app.tr("ui.3d_mouse_reading"), crate::ui_kit::Text::Note("space_mouse_reading"));
                reading_grid(ui, raw);
                // A reassignment made in Preferences stays on top of whatever this records.
                if app.preferences.space_mouse_axis_source != crate::space_mouse::SAME_AXES {
                    crate::ui_kit::text(ui, app.tr("ui.3d_mouse_axes_reassigned"), crate::ui_kit::Text::Note("space_mouse_axes_reassigned"));
                }
                crate::ui_kit::row(ui, app.tr("ui.3d_mouse_dead_zone"), |ui| {
                    dead_zone_slider(ui, &mut dead_zone).on_hover_text(app.tr("ui.3d_mouse_dead_zone_tip"));
                });
            },
            |ui| {
                ui.horizontal(|ui| {
                    back = ui
                        .add_enabled_ui(stage != Stage::Choose, |ui| crate::ui_kit::button(ui, app.tr("ui.back"), crate::ui_kit::Button::Normal))
                        .inner
                        .clicked();
                    skip = can_skip
                        && crate::ui_kit::button(ui, app.tr("ui.3d_mouse_skip"), crate::ui_kit::Button::Normal)
                            .on_hover_text(app.tr("ui.3d_mouse_skip_tip"))
                            .clicked();
                    over =
                        matches!(result, Some(Err(_))) && crate::ui_kit::button(ui, app.tr("ui.start_over"), crate::ui_kit::Button::Normal).clicked();
                });
            },
        );
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    let save = action == crate::ui_kit::Action::Ok;
    if cancel {
        app.space_mouse.calibrating = None;
    } else if let Some((device, wizard)) = &mut app.space_mouse.calibrating {
        wizard.set_dead_zone_share(dead_zone);
        let result = wizard.result();
        if save && let Some(Ok(calibration)) = result {
            app.preferences.space_mouse_calibrations.insert(std::mem::take(device), calibration);
            app.space_mouse.calibrating = None;
        } else if let Some(kind) = chosen {
            wizard.choose(kind);
        } else if over {
            // A new recording of the same kind; the dead zone set so far stays.
            wizard.choose(wizard.kind());
        } else if skip {
            wizard.skip();
        } else if back {
            wizard.back();
        }
    }
}
