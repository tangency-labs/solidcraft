//! W36's runnable interaction harness, backed by the real read-only engine command.
//! Temporary sample geometry only: this is not a Drawing workspace or a file format.
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_engine::Session;
use solidcraft_engine::geom::Vec2;
use solidcraft_engine::geom::drawing_break::{Band, BreakMap};
use solidcraft_engine::geom::drawing_view::Scale;
use solidcraft_engine::schema::drawing_break::{BreakBand, PreviewParams, PreviewResult};

use crate::i18n::tr_label as tr;
use crate::theme::Tokens;
use crate::ui_kit::{self, Button};

/// Developer preview state. Completed edits are undoable within this temporary session.
pub struct BreakPreview {
    session: Session,
    bands: Vec<BreakBand>,
    undo: Vec<Vec<BreakBand>>,
    redo: Vec<Vec<BreakBand>>,
    output: Option<PreviewResult>,
    angle: f64,
    gap: f64,
    scale: usize,
    appearance: usize,
    active: bool,
    pending: Option<f64>,
    error: Option<String>,
    /// Last canvas rectangle, also used by the interaction tests.
    canvas: Rect,
}

impl Default for BreakPreview {
    fn default() -> Self {
        let mut preview = Self {
            session: Session::new(Default::default()),
            bands: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            output: None,
            angle: -90.0,
            gap: 8.0,
            scale: 0,
            appearance: 0,
            active: false,
            pending: None,
            error: None,
            canvas: Rect::NOTHING,
        };
        preview.refresh();
        preview
    }
}

/// An original 240 x 70 mm test plate. Round holes are explicitly a sampled fixture,
/// not a claim that W36's straight-line clipping handles analytic curves yet.
fn fixture() -> Vec<[[f64; 2]; 2]> {
    let mut lines = vec![[[0.0, 0.0], [240.0, 0.0]], [[240.0, 0.0], [240.0, 70.0]], [[240.0, 70.0], [0.0, 70.0]], [[0.0, 70.0], [0.0, 0.0]]];
    for x in [25.0, 215.0] {
        for i in 0..64 {
            let a = std::f64::consts::TAU * f64::from(i) / 64.0;
            let b = std::f64::consts::TAU * f64::from(i + 1) / 64.0;
            lines.push([[x + 10.0 * a.cos(), 35.0 + 10.0 * a.sin()], [x + 10.0 * b.cos(), 35.0 + 10.0 * b.sin()]]);
        }
    }
    lines
}

impl BreakPreview {
    fn scale_text(&self) -> &'static str {
        if self.scale == 0 { "1:1" } else { "1:2" }
    }

    fn map(&self) -> Result<BreakMap, String> {
        let scale: Scale = self.scale_text().parse().map_err(|e: solidcraft_engine::geom::drawing_view::Invalid| e.to_string())?;
        let bands: Vec<_> = self.bands.iter().map(|b| Band { start: b.start, end: b.end, gap_mm: b.gap_mm }).collect();
        BreakMap::new(self.angle.to_radians(), scale, &bands).map_err(|e| e.to_string())
    }

    fn refresh(&mut self) -> bool {
        let params = PreviewParams {
            angle_rad: self.angle.to_radians(),
            scale: self.scale_text().into(),
            breaks: self.bands.clone(),
            lines: fixture(),
            paper_points: Vec::new(),
        };
        let result = serde_json::to_value(params)
            .map_err(|e| e.to_string())
            .and_then(|p| self.session.execute("drawing.break.preview", &p).map_err(|e| e.to_string()))
            .and_then(|r| serde_json::from_value(r).map_err(|e| e.to_string()))
            .and_then(|output: PreviewResult| {
                // The example needs retained edges to define finite canvas bounds.
                if output.lines.is_empty() { Err("Breaks would remove all sample geometry".into()) } else { Ok(output) }
            });
        match result {
            Ok(output) => {
                self.output = Some(output);
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn edit(&mut self, next: Vec<BreakBand>) -> bool {
        let old = std::mem::replace(&mut self.bands, next);
        if self.refresh() {
            self.undo.push(old);
            self.redo.clear();
            true
        } else {
            self.bands = old;
            false
        }
    }

    fn cancel(&mut self) {
        if self.pending.take().is_none() {
            self.active = false;
        }
        self.error = None;
    }

    fn undo(&mut self) {
        self.pending = None;
        if let Some(old) = self.undo.last().cloned() {
            let current = std::mem::replace(&mut self.bands, old);
            if self.refresh() {
                let _ = self.undo.pop();
                self.redo.push(current);
            } else {
                self.bands = current;
            }
        }
    }

    fn redo(&mut self) {
        self.pending = None;
        if let Some(next) = self.redo.last().cloned() {
            let current = std::mem::replace(&mut self.bands, next);
            if self.refresh() {
                let _ = self.redo.pop();
                self.undo.push(current);
            } else {
                self.bands = current;
            }
        }
    }

    fn place(&mut self, model: Vec2) {
        if !self.active {
            return;
        }
        let angle = self.angle.to_radians();
        let coordinate = model.dot(Vec2::new(-angle.sin(), angle.cos()));
        if let Some(first) = self.pending {
            let mut next = self.bands.clone();
            next.push(BreakBand { start: first, end: coordinate, gap_mm: self.gap });
            if self.edit(next) {
                self.pending = None;
            }
        } else {
            self.pending = Some(coordinate);
            self.error = None;
        }
    }

    /// Render inside the host's shared outer container. The fixed sample page has no camera.
    pub fn show(&mut self, ui: &mut Ui) {
        let t = Tokens::get();
        ui.heading(tr("Broken View interaction preview"));
        ui.label(tr("Temporary sample view. Drawing-file integration is not available yet."));
        ui.add_space(t.space_8);
        ui.horizontal(|ui| {
            if ui_kit::button(ui, tr(if self.active { "Finish" } else { "Start Broken View" }), Button::Normal).clicked() {
                self.active = !self.active;
                self.pending = None;
            }
            ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                if ui_kit::button(ui, tr("Undo"), Button::Normal).clicked() {
                    self.undo();
                }
            });
            ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
                if ui_kit::button(ui, tr("Redo"), Button::Normal).clicked() {
                    self.redo();
                }
            });
            ui.add_enabled_ui(!self.bands.is_empty(), |ui| {
                if ui_kit::button(ui, tr("Clear breaks"), Button::Normal).clicked() {
                    self.edit(Vec::new());
                    self.pending = None;
                }
            });
            ui.label(format!("{}: {}", tr("Completed breaks"), self.bands.len()));
        });
        ui.horizontal(|ui| {
            ui.set_width(390.0);
            ui.add_enabled_ui(self.bands.is_empty() && self.pending.is_none(), |ui| {
                let previous = self.angle;
                ui_kit::row(ui, tr("Line angle"), |ui| {
                    ui_kit::slider(ui, &mut self.angle, -180.0..=180.0, ui_kit::Slider { step: Some(1.0), ..Default::default() });
                });
                if previous != self.angle {
                    self.refresh();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.set_width(390.0);
            ui.add_enabled_ui(self.pending.is_none(), |ui| {
                ui_kit::row(ui, tr("Paper gap (mm)"), |ui| {
                    ui_kit::slider(ui, &mut self.gap, 1.0..=20.0, ui_kit::Slider { step: Some(0.5), ..Default::default() });
                });
            });
        });
        let previous = self.scale;
        ui_kit::choice_row(ui, tr("Scale"), "break-preview-scale", &mut self.scale, &["1:1", "1:2"]);
        if previous != self.scale {
            self.pending = None;
            if !self.refresh() {
                self.scale = previous;
            }
        }
        ui_kit::choice_row(ui, tr("Break line appearance"), "break-preview-style", &mut self.appearance, &["Zigzag", "Straight"]);
        let hint = if !self.active {
            "Start the tool, then click two break lines on the sample view."
        } else if self.pending.is_some() {
            "Place the second line. Right-click or Escape cancels the pending pair."
        } else {
            "Place the first line. Right-click or Escape finishes the tool."
        };
        ui.colored_label(t.accent, tr(hint));
        if let Some(error) = &self.error {
            ui.colored_label(t.error, error);
        }
        ui.label(tr("Parallel breaks only. Line angle is locked while breaks exist. Holes in this fixture are sampled polylines."));
        let (response, painter) = ui.allocate_painter(ui.available_size().max(vec2(100.0, 180.0)), Sense::click());
        self.canvas = response.rect;
        painter.rect_filled(response.rect, t.radius_md, t.file_surface);
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel();
        }
        if response.secondary_clicked() {
            self.cancel();
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y)) {
            self.redo();
        }

        let Some(output) = &self.output else {
            return;
        };
        let mut min = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut max = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for point in output.lines.iter().flatten() {
            min.x = min.x.min(point[0]);
            min.y = min.y.min(point[1]);
            max.x = max.x.max(point[0]);
            max.y = max.y.max(point[1]);
        }
        let center = (min + max) * 0.5;
        let pixels = ((f64::from(response.rect.width()) - 90.0) / 260.0).min((f64::from(response.rect.height()) - 110.0) / 100.0).max(0.01);
        let screen = |p: Vec2| -> Pos2 {
            pos2(response.rect.center().x + ((p.x - center.x) * pixels) as f32, response.rect.center().y - ((p.y - center.y) * pixels) as f32)
        };
        for [a, b] in &output.lines {
            painter.line_segment([screen(Vec2::new(a[0], a[1])), screen(Vec2::new(b[0], b[1]))], Stroke::new(1.8, t.text));
        }
        let n = Vec2::new(output.normal[0], output.normal[1]);
        let tangent = Vec2::new(n.y, -n.x);
        for gap in &output.gaps {
            for q in gap {
                let hits: Vec<_> = output.lines.iter().flatten().map(|p| Vec2::new(p[0], p[1])).filter(|p| (p.dot(n) - q).abs() < 1e-6).collect();
                let lo = hits.iter().map(|p| p.dot(tangent)).fold(f64::INFINITY, f64::min);
                let hi = hits.iter().map(|p| p.dot(tangent)).fold(f64::NEG_INFINITY, f64::max);
                if lo < hi {
                    draw_marker(&painter, screen(n * *q + tangent * lo), screen(n * *q + tangent * hi), self.appearance, t.text);
                }
            }
        }
        let dim_y = screen(max).y - 25.0;
        let left = pos2(screen(min).x, dim_y);
        let right = pos2(screen(max).x, dim_y);
        painter.line_segment([left, right], Stroke::new(1.0, t.accent));
        painter.text(
            pos2((left.x + right.x) * 0.5, dim_y - 9.0),
            egui::Align2::CENTER_BOTTOM,
            tr("True model length: 240 mm"),
            FontId::proportional(t.font_body),
            t.accent,
        );
        painter.text(
            response.rect.left_bottom() + vec2(14.0, -14.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{}: {:.2} mm  |  {} {}", tr("Paper shortening"), output.shortening_mm, tr("Scale"), self.scale_text()),
            FontId::proportional(t.font_small),
            t.text_dim,
        );

        let hover_model = response
            .hover_pos()
            .and_then(|pointer| {
                let paper = center
                    + Vec2::new(f64::from(pointer.x - response.rect.center().x) / pixels, -f64::from(pointer.y - response.rect.center().y) / pixels);
                self.map().ok()?.to_model(paper).ok()?
            })
            .filter(|p| (0.0..=240.0).contains(&p.x) && (0.0..=70.0).contains(&p.y));
        if self.active {
            if let Ok(map) = self.map() {
                let factor = if self.scale == 0 { 1.0 } else { 0.5 };
                for coordinate in self.pending.into_iter().chain(hover_model.map(|p| p.dot(n))) {
                    // Intersect the preview line with the original fixture's four edges.
                    let mut hits = Vec::new();
                    for [a, b] in fixture().into_iter().take(4) {
                        let a = Vec2::new(a[0], a[1]);
                        let b = Vec2::new(b[0], b[1]);
                        let d = (b - a).dot(n);
                        if d.abs() > 1e-12 {
                            let parameter = (coordinate - a.dot(n)) / d;
                            if (0.0..=1.0).contains(&parameter) {
                                hits.push(a + (b - a) * parameter);
                            }
                        }
                    }
                    if let Some(p) = hits.first() {
                        let lo = hits.iter().map(|p| p.dot(tangent)).fold(f64::INFINITY, f64::min);
                        let hi = hits.iter().map(|p| p.dot(tangent)).fold(f64::NEG_INFINITY, f64::max);
                        if let Ok(Some(mapped)) = map.to_paper(*p) {
                            let q = mapped.dot(n);
                            painter.line_segment(
                                [screen(n * q + tangent * lo * factor), screen(n * q + tangent * hi * factor)],
                                Stroke::new(2.0, t.accent),
                            );
                        }
                    }
                }
            }
            if response.clicked()
                && let Some(model) = hover_model
            {
                self.place(model);
            }
        }
    }
}

fn draw_marker(painter: &egui::Painter, a: Pos2, b: Pos2, style: usize, color: Color32) {
    if style == 1 {
        painter.line_segment([a, b], Stroke::new(1.5, color));
        return;
    }
    let d = b - a;
    let length = d.length();
    if length <= 1.0 {
        return;
    }
    let normal = vec2(-d.y, d.x) / length;
    let points: Vec<_> = (0..=12)
        .map(|i| {
            let offset = if i == 0 || i == 12 {
                0.0
            } else if i % 2 == 0 {
                -3.0
            } else {
                3.0
            };
            a + d * (i as f32 / 12.0) + normal * offset
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
}

#[cfg(test)]
#[path = "broken_view_preview_tests.rs"]
mod tests;
