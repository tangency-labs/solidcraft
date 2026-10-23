//! The 3D mouse calibration window, opened from Preferences › Navigation. It asks the user to
//! leave the cap still, then to move it along each axis both ways, moving on by itself after each
//! (`solidcraft_space_mouse::Wizard`), and saves what it found for the device it started with.
//! The window closes with Preferences, and when another device (or none) is read instead.

use solidcraft_space_mouse::{Calibration, Problem, Stage, Wizard};

use crate::SolveApp;

/// What each of `solidcraft_space_mouse::STEPS` asks for, in order.
const STEP_PROMPTS: [&str; 12] = [
    "ui.space_mouse_step_slide_right",
    "ui.space_mouse_step_slide_left",
    "ui.space_mouse_step_pull",
    "ui.space_mouse_step_push",
    "ui.space_mouse_step_press",
    "ui.space_mouse_step_lift",
    "ui.space_mouse_step_tilt_back",
    "ui.space_mouse_step_tilt_forward",
    "ui.space_mouse_step_tilt_left",
    "ui.space_mouse_step_tilt_right",
    "ui.space_mouse_step_twist_clockwise",
    "ui.space_mouse_step_twist_counterclockwise",
];

/// The dead zone, as a share of full travel, shown as a percentage.
pub(crate) fn dead_zone_slider(ui: &mut egui::Ui, share: &mut f32) -> egui::Response {
    crate::ui_kit::slider(
        ui,
        share,
        0.0..=solidcraft_space_mouse::MAX_DEAD_ZONE_SHARE,
        crate::ui_kit::Slider { step: Some(0.01), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
    )
}

/// The calibration's dead zones the way a game controller tester shows a stick: per pair of axes,
/// a circle for the full travel, the dead zone shaded inside it (each direction its own size), and
/// a dot where the cap is now, before the dead zone, so it's plain when the view starts to move.
pub(crate) fn dead_zone_pads(ui: &mut egui::Ui, calibration: &Calibration, raw: [i16; 6]) {
    const NAMES: [&str; 6] = ["X", "Y", "Z", "Rx", "Ry", "Rz"];
    // (horizontal axis, its positive way on screen, vertical axis, its positive way down), so the
    // dot moves as the cap does: slid right and pulled toward you, twisted and pressed, tilted.
    const PADS: [(usize, f32, usize, f32); 3] = [(0, 1.0, 1, 1.0), (5, 1.0, 2, 1.0), (4, -1.0, 3, -1.0)];
    const SIZE: f32 = 76.0;
    let (shares, travel, moving) = (calibration.dead_zone_shares(), calibration.travel(raw), calibration.apply(raw));
    let visuals = ui.visuals().clone();
    ui.horizontal(|ui| {
        for (h, h_way, v, v_way) in PADS {
            ui.vertical(|ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let (centre, radius) = (rect.center(), SIZE / 2.0 - 2.0);
                let faint = visuals.weak_text_color();
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
                let percent = |axis: usize| format!("{} −{:.0} % +{:.0} %", NAMES[axis], shares[axis][0] * 100.0, shares[axis][1] * 100.0);
                response.on_hover_text(format!("{}\n{}", percent(h), percent(v)));
                ui.label(egui::RichText::new(format!("{} · {}", NAMES[h], NAMES[v])).small().color(faint));
            });
        }
    });
}

/// Starts calibrating the device being read, if there is one.
pub fn start(app: &mut SolveApp) {
    app.space_mouse.calibrating = app.space_mouse.device.clone().map(|device| (device, Wizard::default()));
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((_, wizard)) = &app.space_mouse.calibrating else { return };
    let (stage, progress, result) = (wizard.stage(), wizard.progress(), wizard.result());
    let mut dead_zone = wizard.dead_zone_share();
    let raw: [i16; 6] = app.space_mouse.raw.into();
    let prompt = match (stage, result) {
        (Stage::Rest, _) => "ui.space_mouse_step_rest",
        (Stage::Step(index), _) => STEP_PROMPTS.get(index).copied().unwrap_or_default(),
        (Stage::Done, Some(Ok(_))) => "ui.space_mouse_calibration_done",
        (Stage::Done, _) => "ui.space_mouse_calibration_no_response",
    };
    // The calibrated motion that barely moved, as Preferences names it.
    let weak = match result {
        Some(Err(Problem::NoResponse(axis))) => crate::space_mouse::MOTIONS.get(axis).copied(),
        _ => None,
    };
    let counter = match stage {
        Stage::Rest => String::new(),
        Stage::Step(index) => format!("{} / {}", index + 1, STEP_PROMPTS.len()),
        Stage::Done => format!("{0} / {0}", STEP_PROMPTS.len()),
    };
    let (mut back, mut over, mut cancel, mut save) = (false, false, false, false);
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_space_mouse_calibration"), "settings", app.tr("ui.space_mouse_calibrate_title"))
        .show_body(ctx, |ui| {
            ui.label(app.tr(prompt));
            if let Some(axis) = weak {
                ui.label(egui::RichText::new(app.tr(axis)).strong());
            }
            ui.add_space(6.0);
            crate::ui_kit::progress(ui, "space_mouse_calibration", progress as f32);
            ui.label(counter);
            if let Some(Ok(calibration)) = result {
                ui.add_space(6.0);
                dead_zone_pads(ui, &Calibration { dead_zone_share: dead_zone, ..calibration }, raw);
            }
            // The device's six raw axes as they come, so the jitter at rest is there to see.
            let reading = raw.map(|value| format!("{value:>6}")).join(" ");
            ui.label(egui::RichText::new(format!("{}: {reading}", app.tr("ui.space_mouse_reading"))).monospace());
            // A reassignment made in Preferences stays on top of whatever this records.
            if app.preferences.space_mouse_axis_source != crate::space_mouse::SAME_AXES {
                ui.label(egui::RichText::new(app.tr("ui.space_mouse_axes_reassigned")).small());
            }
            ui.horizontal(|ui| {
                ui.label(app.tr("ui.space_mouse_dead_zone"));
                dead_zone_slider(ui, &mut dead_zone).on_hover_text(app.tr("ui.space_mouse_dead_zone_tip"));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                back = ui
                    .add_enabled_ui(stage != Stage::Rest, |ui| crate::ui_kit::button(ui, app.tr("ui.back"), crate::ui_kit::Button::Normal))
                    .inner
                    .clicked();
                over = matches!(result, Some(Err(_))) && crate::ui_kit::button(ui, app.tr("ui.start_over"), crate::ui_kit::Button::Normal).clicked();
                cancel = crate::ui_kit::button(ui, app.tr("ui.cancel"), crate::ui_kit::Button::Normal).clicked();
                save = matches!(result, Some(Ok(_))) && crate::ui_kit::button(ui, app.tr("ui.save"), crate::ui_kit::Button::Normal).clicked();
            });
        });
    if cancel || closed {
        app.space_mouse.calibrating = None;
    } else if let Some((device, wizard)) = &mut app.space_mouse.calibrating {
        wizard.set_dead_zone_share(dead_zone);
        let result = wizard.result();
        if save && let Some(Ok(calibration)) = result {
            app.preferences.space_mouse_calibrations.insert(std::mem::take(device), calibration);
            app.space_mouse.calibrating = None;
        } else if over {
            // A new recording; the dead zone set so far stays.
            *wizard = Wizard::default();
            wizard.set_dead_zone_share(dead_zone);
        } else if back {
            wizard.back();
        }
    }
}
