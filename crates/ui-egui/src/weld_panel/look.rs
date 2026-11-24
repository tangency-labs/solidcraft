//! Process textures and the six spot appearance specimens.
use super::*;

const PROCESSES: &[(&str, Option<&str>)] = &[
    ("ui.weld.process.mig", Some("135")),
    ("ui.weld.process.tig", Some("141")),
    ("ui.weld.process.stick", Some("111")),
    ("ui.weld.process.saw", Some("121")),
    ("ui.weld.process.laser", Some("52")),
    ("ui.weld.process.fsw", Some("43")),
    ("ui.weld.process.unset", None),
];

pub(in crate::weld_panel) fn process_number(process: usize) -> Option<&'static str> {
    PROCESSES.get(process).and_then(|(_, number)| *number)
}

fn texture(ui: &mut Ui, state: &WeldPanelState, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(t.form_label_width + t.space_24 * 2.0 + t.space_2, t.control_height - t.space_2), Sense::hover());
    let bead = rect.shrink2(vec2(t.space_2, t.space_3));
    if state.weld_type.info().look_tbd || state.process == 6 {
        // The board deliberately reserves an unfilled slot when its look is undecided.
        let stroke = t.control_border_stroke(t.accent);
        ui.painter().extend(kit::dashed_line(&[bead.left_top(), bead.right_top()], ui.clip_rect(), stroke, t.space_4, t.space_3));
        ui.painter().extend(kit::dashed_line(&[bead.left_bottom(), bead.right_bottom()], ui.clip_rect(), stroke, t.space_4, t.space_3));
        ui.painter().line_segment([bead.left_top(), bead.left_bottom()], stroke);
        ui.painter().line_segment([bead.right_top(), bead.right_bottom()], stroke);
        return;
    }
    ui.painter().rect_filled(bead, bead.height() / 2.0, t.accent);
    ui.painter().rect_stroke(bead, bead.height() / 2.0, t.control_border_stroke(t.icon), egui::StrokeKind::Inside);
    let step = match state.texture {
        1 => t.space_8 + t.space_2,
        2 => t.space_14,
        3 => t.space_6,
        4 => t.space_6,
        _ => t.space_6 + t.space_2 / 2.0,
    };
    let mut x = bead.left() + t.space_6;
    while x < bead.right() - t.space_6 {
        let points = (0..=8)
            .map(|i| {
                let phase = i as f32 / 8.0;
                pos2(
                    x + if state.texture == 3 { 0.0 } else { (phase * std::f32::consts::PI).sin() * t.space_3 },
                    bead.top() + t.space_2 + phase * (bead.height() - t.space_4),
                )
            })
            .collect();
        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.0, t.on_accent.gamma_multiply(0.6))));
        x += step;
    }
    if state.texture == 5 {
        ui.painter().circle(
            Rect::from_min_size(bead.max - vec2(t.space_16, bead.height()), vec2(t.space_16, bead.height())).center(),
            t.space_4,
            t.dialog_body,
            t.control_border_stroke(t.icon),
        );
    }
}

pub(super) fn show(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements) {
    section(ui, t, m, "look", |ui, m| {
        if state.weld_type == WeldType::Spot {
            let looks = [
                "ui.weld.spot.marks",
                "ui.weld.spot.ball",
                "ui.weld.spot.marks_ball",
                "ui.weld.spot.marks_pin",
                "ui.weld.spot.pin",
                "ui.weld.spot.dent",
            ];
            for (row, items) in looks.chunks(3).enumerate() {
                let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height * 3.0 + t.space_10), Sense::hover());
                let width = (band.width() - 2.0 * t.space_6) / 3.0;
                for (column, text) in items.iter().enumerate() {
                    let index = row * 3 + column;
                    let rect = Rect::from_min_size(band.min + vec2(column as f32 * (width + t.space_6), 0.0), vec2(width, band.height()));
                    let response = at_button(ui, m, rect, text, "", if state.spot_look == index { Button::SmallSelected } else { Button::Normal });
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), state.spot_look == index, tr(text)));
                    let thumb = Rect::from_center_size(
                        pos2(rect.center().x, rect.top() + t.space_4 + (t.control_height * 2.0 - t.space_2) / 2.0),
                        vec2(t.control_height * 3.0, t.control_height * 2.0 - t.space_2),
                    );
                    spot_thumbnail(ui, thumb, index, t);
                    let galley = ui.painter().layout(tr(text).into(), FontId::proportional(t.font_caption), t.form_ink, rect.width() - t.space_8);
                    ui.painter().galley(
                        pos2(rect.center().x - galley.size().x / 2.0, rect.bottom() - t.space_6 - galley.size().y),
                        galley,
                        t.form_ink,
                    );
                    if response.clicked() {
                        state.spot_look = index;
                    }
                }
            }
            label(ui, "ui.weld.spot.two_sheets", t, true);
        } else {
            let options: Vec<_> = PROCESSES.iter().map(|(id, _)| tr(id)).collect();
            let before = state.process;
            kit::dropdown(ui, "process", &mut state.process, &options);
            if state.process != before {
                state.texture = state.process;
            }
            ui.horizontal(|ui| {
                ui.add_sized(
                    vec2(t.form_label_width + t.space_6, t.control_height),
                    egui::Label::new(RichText::new(tr("ui.weld.texture")).size(t.font_body).color(t.form_dim)),
                );
                texture(ui, state, t);
                label(ui, "ui.weld.from_process", t, true);
            });
            kit::checkbox(ui, tr("ui.weld.passes"), &mut state.passes_shown);
            kit::checkbox(ui, tr("ui.weld.penetration"), &mut state.penetration_shown);
        }
    });
}

/// These are the board's six section illustrations, not new icon assets.
fn spot_thumbnail(ui: &Ui, rect: Rect, index: usize, t: &Tokens) {
    let scale = rect.width() / 84.0;
    let at = |x: f32, y: f32| rect.min + vec2(x, y) * scale;
    let outline = egui::Stroke::new(1.2 * scale, t.icon);
    ui.painter().rect(Rect::from_min_max(at(6.0, 18.0), at(78.0, 36.0)), t.space_2, t.figure_face_left, outline, egui::StrokeKind::Inside);
    let cap = |left: f32, right: f32, y: f32, control_y: f32, fill| {
        let mut points: Vec<_> = (0..=16)
            .map(|i| {
                let q = i as f32 / 16.0;
                at(left + (right - left) * q, (1.0 - q).powi(2) * y + 2.0 * (1.0 - q) * q * control_y + q.powi(2) * y)
            })
            .collect();
        points.push(at(left, y));
        ui.painter().add(egui::Shape::convex_polygon(points, fill, outline));
    };
    if matches!(index, 0 | 2 | 3) {
        cap(31.0, 53.0, 18.0, 13.0, t.accent);
        cap(31.0, 53.0, 36.0, 41.0, t.accent);
    }
    match index {
        1 | 2 => {
            ui.painter().circle(at(42.0, 27.0), if index == 1 { 12.0 } else { 6.0 } * scale, t.accent, outline);
        }
        3 => {
            ui.painter().rect(Rect::from_min_max(at(39.5, 18.0), at(44.5, 36.0)), 0, t.accent, outline, egui::StrokeKind::Inside);
        }
        4 => {
            // Concave-ended pin: each strip is convex, so the tessellator never fills a concavity.
            let mut top = Vec::new();
            let mut bottom = Vec::new();
            for i in 0..=16 {
                let q = i as f32 / 16.0;
                top.push(at(37.0 + q * 10.0, 18.0 + 10.0 * q * (1.0 - q)));
                bottom.push(at(37.0 + q * 10.0, 36.0 - 10.0 * q * (1.0 - q)));
            }
            for (a, b) in top.windows(2).zip(bottom.windows(2)) {
                ui.painter().add(egui::Shape::convex_polygon(vec![a[0], a[1], b[1], b[0]], t.accent, egui::Stroke::NONE));
            }
            ui.painter().add(egui::Shape::line(top, outline));
            ui.painter().add(egui::Shape::line(bottom, outline));
            ui.painter().line_segment([at(37.0, 18.0), at(37.0, 36.0)], outline);
            ui.painter().line_segment([at(47.0, 18.0), at(47.0, 36.0)], outline);
        }
        5 => cap(30.0, 54.0, 18.0, 28.0, t.figure_face_top),
        _ => {}
    }
}
