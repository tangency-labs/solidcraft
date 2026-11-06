//! The vibration study's transient settings, background solve and linked viewport readout.

use std::{collections::BTreeMap, sync::Arc};

use egui::{Align2, Color32, Id, Rect, Sense, Stroke, pos2, vec2};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Document, Mat};
use solidcraft_engine::geom::{Aabb3, prelude::*};
use solidcraft_engine::vibration::{COMMAND, Shape, Support, SweepResult};
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_engine::vibration::{Progress, SweepParams, with_progress};

use crate::SolveApp;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::ui_kit as kit;
use crate::viewport::Proj;

#[derive(Clone)]
struct Anchor {
    support: Support,
    point: Vec3,
}

#[cfg(not(target_arch = "wasm32"))]
type JobResult = Result<SweepResult, String>;

#[cfg(not(target_arch = "wasm32"))]
struct Job {
    done: std::sync::mpsc::Receiver<JobResult>,
    progress: Arc<Progress>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Job {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

pub(crate) struct Study {
    document: Arc<Document>,
    pending_moves: BTreeMap<u64, Mat>,
    body: Option<String>,
    anchors: Vec<Anchor>,
    start: f64,
    end: f64,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<Job>,
    result: Option<SweepResult>,
    error: Option<String>,
    mode: bool,
    selected: usize,
    playing: bool,
    scale: f64,
    mode_scale: f64,
}

impl Study {
    fn current(&self, app: &SolveApp) -> bool {
        Arc::ptr_eq(&self.document, &app.session.doc) && self.pending_moves == app.session.pending_moves
    }

    fn shape(&self) -> Option<(&SweepResult, &Shape)> {
        let r = self.result.as_ref()?;
        let values = if self.mode { &r.modes } else { &r.peaks };
        Some((r, values.get(self.selected)?))
    }

    fn running(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn complete(&mut self, result: JobResult) {
        match result {
            Ok(result) => {
                self.selected = result.peaks.iter().enumerate().max_by(|(_, a), (_, b)| maximum(a).total_cmp(&maximum(b))).map_or(0, |(i, _)| i);
                self.result = Some(result);
            }
            Err(message) => self.error = Some(message),
        }
    }
}

pub(crate) fn open(app: &mut SolveApp) {
    if app.vibration.is_some() {
        return;
    }
    let selection = app.session.selection.clone();
    let mut study = Study {
        document: Arc::clone(&app.session.doc),
        pending_moves: app.session.pending_moves.clone(),
        body: None,
        anchors: Vec::new(),
        start: 20.0,
        end: 500.0,
        #[cfg(not(target_arch = "wasm32"))]
        pending: None,
        result: None,
        error: None,
        mode: false,
        selected: 0,
        playing: false,
        scale: 25.0,
        mode_scale: 3.0,
    };
    add_anchors(&mut study, &selection);
    app.vibration = Some(study);
}

fn add_anchors(study: &mut Study, selection: &[Sel]) {
    for item in selection {
        let Sel::Face { body, index, point } = item else { continue };
        let Ok(face) = u32::try_from(*index) else { continue };
        if study.body.as_ref().is_none_or(|b| b == body)
            && study.anchors.len() < 64
            && !study.anchors.iter().any(|a| a.support.body == *body && a.support.face == face)
        {
            study.anchors.push(Anchor { support: Support { body: body.clone(), face }, point: *point });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn begin(app: &SolveApp, study: &mut Study, ctx: &egui::Context) {
    study.error = None;
    study.playing = false;
    study.mode = false;
    study.result = None;
    let params = SweepParams {
        body: study.body.clone(),
        supports: study.anchors.iter().map(|a| a.support.clone()).collect(),
        start_hz: study.start,
        end_hz: study.end,
    };
    let mut session = app.session.scratch();
    let run = move || -> JobResult {
        let params = serde_json::to_value(params).map_err(|e| e.to_string())?;
        let value = session.execute(COMMAND, &params).map_err(|e| e.to_string())?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    };
    let (send, receive) = std::sync::mpsc::channel();
    let repaint = ctx.clone();
    let progress = Progress::new();
    let worker_progress = Arc::clone(&progress);
    match std::thread::Builder::new().name("vibration-sweep".into()).spawn(move || {
        let _ = send.send(with_progress(Some(worker_progress), run));
        repaint.request_repaint();
    }) {
        Ok(_) => study.pending = Some(Job { done: receive, progress }),
        Err(e) => study.error = Some(e.to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn begin(_: &SolveApp, study: &mut Study, _: &egui::Context) {
    study.error = Some(tr("vibration.desktop").into());
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut study) = app.vibration.take() else { return };
    if !study.current(app) {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(job) = &study.pending {
        use std::sync::mpsc::TryRecvError;
        let result = match job.done.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Disconnected) => Some(Err(tr("vibration.stopped").into())),
            Err(TryRecvError::Empty) => None,
        };
        if let Some(result) = result {
            study.pending = None;
            study.complete(result);
        }
    }
    let running = study.running();
    let mut solve = false;
    let close = crate::ui_kit::Window::new(Id::unique(COMMAND), "graph", tr("vibration.title"))
        .anchor(Align2::RIGHT_TOP, vec2(-22.0, 120.0))
        .cards(1.45)
        .show_body(ctx, |ui| {
            if study.result.is_some() {
                if kit::text_button(ui, tr("vibration.back"), true).clicked() {
                    study.result = None;
                    study.playing = false;
                }
                results(ui, &mut study);
            } else if running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("vibration.solving"));
                });
            } else {
                let world = app.session.world_state().ok();
                let names = world.as_ref().map(|w| w.bodies.as_slice()).unwrap_or_default();
                ui.label(tr("vibration.target"));
                let old = study.body.clone();
                let current = old.as_deref().unwrap_or_else(|| tr("vibration.all"));
                kit::dropdown_menu(ui, "vibration-target", current, |ui| {
                    if kit::list_picker(ui, "vibration-all-bodies", tr("vibration.all"), study.body.is_none()).clicked() {
                        study.body = None;
                    }
                    for body in names {
                        let selected = study.body.as_deref() == Some(body.name.as_str());
                        if kit::list_picker(ui, &body.name, &body.name, selected).clicked() {
                            study.body = Some(body.name.clone());
                        }
                    }
                });
                if study.body != old {
                    study.anchors.retain(|a| study.body.as_ref().is_none_or(|b| b == &a.support.body));
                    study.error = None;
                }
                ui.separator();
                ui.strong(tr("vibration.fixed"));
                ui.label(tr("vibration.pick"));
                ui.horizontal(|ui| {
                    if kit::text_button(ui, tr("vibration.add"), true).clicked() {
                        add_anchors(&mut study, &app.session.selection);
                    }
                    if kit::text_button(ui, tr("vibration.clear"), true).clicked() {
                        study.anchors.clear();
                    }
                });
                let mut remove = None;
                for (i, a) in study.anchors.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} · {} / {}", i + 1, a.support.body, a.support.face));
                        if kit::button(ui, "×", kit::Button::Small).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    study.anchors.remove(i);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tr("vibration.start"));
                    kit::number_field(ui, &mut study.start, 0.01..=1e6, 1.0, " Hz");
                    ui.label(tr("vibration.end"));
                    kit::number_field(ui, &mut study.end, 0.01..=1e6, 1.0, " Hz");
                });
                ui.small(tr("vibration.conditions"));
                if let Some(error) = &study.error {
                    ui.colored_label(Tokens::get().figure_conflict, error);
                }
                let ready = !study.anchors.is_empty() && study.start < study.end && !names.is_empty();
                let run = kit::text_button(ui, tr("vibration.run"), ready);
                crate::scenario::publish_target("vibration:run", run.rect);
                solve = run.clicked();
            }
        });
    if !close {
        if solve {
            begin(app, &mut study, ctx);
        }
        app.vibration = Some(study);
    }
}

fn maximum(shape: &Shape) -> f64 {
    shape.amplitude.get(shape.maximum).copied().unwrap_or_default()
}

fn results(ui: &mut egui::Ui, study: &mut Study) {
    let Some(r) = &study.result else { return };
    let mut mode = usize::from(study.mode);
    if kit::segmented(ui, &mut mode, &[tr("vibration.response"), tr("vibration.modal")], kit::Segments { intrinsic: true, ..Default::default() }) {
        study.mode = mode == 1;
        study.selected = 0;
    }
    let shapes = if study.mode { &r.modes } else { &r.peaks };
    plot(ui, r, study.mode, &mut study.selected);
    if !study.mode && !r.has_resonance {
        ui.small(tr("vibration.band_maximum"));
    }
    if shapes.is_empty() {
        ui.label(tr("vibration.empty"));
    } else {
        let label = |shape: &Shape| {
            if study.mode { format!("{:.2} Hz", shape.hz) } else { format!("{:.2} Hz · {:.4} mm", shape.hz, maximum(shape)) }
        };
        let labels: Vec<_> = shapes.iter().map(label).collect();
        let options: Vec<_> = labels.iter().map(String::as_str).collect();
        kit::dropdown(ui, "vibration-frequency", &mut study.selected, &options);
        if let Some(shape) = shapes.get(study.selected) {
            let unit = if study.mode { "100%".to_owned() } else { format!("{:.4} mm", maximum(shape)) };
            let body = r.vertex_body.get(shape.maximum).and_then(|&i| r.bodies.get(i)).map_or("", String::as_str);
            ui.strong(format!("{}  {unit} · {body}", tr("vibration.maximum")));
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
            for i in 0..48 {
                let a = i as f32 / 48.0;
                let b = (i + 1) as f32 / 48.0;
                let band = Rect::from_min_max(pos2(rect.left() + a * rect.width(), rect.top()), pos2(rect.left() + b * rect.width(), rect.bottom()));
                ui.painter().rect_filled(band, 0.0, heat(f64::from(a)));
            }
            ui.horizontal(|ui| {
                ui.small(if study.mode { "0%" } else { "0 mm" });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(unit);
                });
            });
        }
        ui.horizontal(|ui| {
            let play = if study.playing { "vibration.pause" } else { "vibration.play" };
            if kit::text_button(ui, tr(play), true).clicked() {
                study.playing = !study.playing;
            }
            ui.label(tr("vibration.scale"));
            if study.mode {
                kit::number_field(ui, &mut study.mode_scale, 0.0..=30.0, 1.0, "%");
            } else {
                kit::number_field(ui, &mut study.scale, 0.0..=100.0, 1.0, "×");
            }
        });
    }
    ui.small(tr(if study.mode { "vibration.normalized" } else { "vibration.conditions" }));
    ui.small(tr("vibration.estimate"));
    for warning in &r.warnings {
        ui.small(warning);
    }
}

fn plot(ui: &mut egui::Ui, r: &SweepResult, modal: bool, index: &mut usize) {
    let Some((start, end)) = r.curve.first().zip(r.curve.last()) else { return };
    let (rect, hit) = ui.allocate_exact_size(vec2(ui.available_width().max(150.0), 145.0), Sense::click());
    let chart = Rect::from_min_max(rect.min + vec2(46.0, 12.0), rect.max - vec2(12.0, 27.0));
    let tokens = Tokens::get();
    let painter = ui.painter_at(rect);
    let y_max = r.curve.iter().map(|p| p.max_mm).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
    let x = |hz: f64| chart.left() + chart.width() * ((hz - start.hz) / (end.hz - start.hz)) as f32;
    let y = |amp: f64| chart.bottom() - chart.height() * (amp / y_max) as f32;
    painter.rect_filled(chart, 0.0, tokens.figure_ground);
    let font = egui::FontId::proportional(tokens.font_caption);
    for i in 0..=4 {
        let part = f64::from(i) / 4.0;
        let hz = start.hz + (end.hz - start.hz) * part;
        let xx = x(hz);
        painter.vline(xx, chart.y_range(), Stroke::new(1.0, tokens.dialog_rule));
        painter.text(pos2(xx, chart.bottom() + 4.0), Align2::CENTER_TOP, format!("{hz:.1}"), font.clone(), tokens.dialog_label);
    }
    if !modal {
        for i in 0..=2 {
            let v = y_max * f64::from(i) / 2.0;
            let yy = y(v);
            painter.hline(chart.x_range(), yy, Stroke::new(1.0, tokens.dialog_rule));
            painter.text(pos2(chart.left() - 5.0, yy), Align2::RIGHT_CENTER, format!("{v:.3}"), font.clone(), tokens.dialog_label);
        }
        let line = r.curve.iter().map(|p| pos2(x(p.hz), y(p.max_mm))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(1.5, tokens.figure_accent)));
        painter.text(pos2(rect.left(), rect.top()), Align2::LEFT_TOP, "mm", font.clone(), tokens.dialog_label);
    }
    painter.text(rect.right_bottom(), Align2::RIGHT_BOTTOM, "Hz", font, tokens.dialog_label);
    let shapes = if modal { &r.modes } else { &r.peaks };
    if hit.clicked()
        && let Some(at) = hit.interact_pointer_pos()
        && let Some((i, _)) = shapes.iter().enumerate().min_by(|(_, a), (_, b)| (x(a.hz) - at.x).abs().total_cmp(&(x(b.hz) - at.x).abs()))
    {
        *index = i;
    }
    for (i, s) in shapes.iter().enumerate() {
        let color = if i == *index { tokens.figure_warn } else { tokens.figure_accent };
        let at = pos2(x(s.hz), if modal { chart.center().y } else { y(maximum(s)) });
        painter.line_segment([at, pos2(at.x, chart.bottom())], Stroke::new(1.0, color));
        painter.circle_filled(at, if i == *index { 4.5 } else { 3.0 }, color);
    }
}

fn heat(value: f64) -> Color32 {
    let t = Tokens::get();
    let (a, b, k) =
        if value < 0.5 { (t.figure_axis_z, t.figure_ok, (2.0 * value) as f32) } else { (t.figure_ok, t.figure_conflict, (2.0 * value - 1.0) as f32) };
    egui::lerp(egui::Rgba::from(a)..=egui::Rgba::from(b), k.clamp(0.0, 1.0)).into()
}

fn vector(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

fn undeformed_outline(r: &SweepResult, painter: &egui::Painter, proj: &Proj) {
    let mut edges = std::collections::BTreeMap::new();
    let eye = proj.cam.eye();
    for tri in &r.triangles {
        let [a, b, c] = tri.vertices;
        let Some((&x, (&y, &z))) = r.vertices_mm.get(a).zip(r.vertices_mm.get(b).zip(r.vertices_mm.get(c))) else { continue };
        let (x, y, z) = (vector(x), vector(y), vector(z));
        if (y - x).cross(z - x).dot(eye - x) > 0.0 {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                *edges.entry((u.min(v), u.max(v))).or_insert(0) += 1;
            }
        }
    }
    let stroke = Stroke::new(1.25, Tokens::get().figure_reference);
    for ((a, b), count) in edges {
        if count == 1 {
            let to_screen = |i| proj.to_screen(vector(*r.vertices_mm.get(i)?));
            if let Some((a, b)) = to_screen(a).zip(to_screen(b)) {
                painter.extend(crate::viewport::dashed_line(&[a, b], painter.clip_rect(), stroke, 4.0, 4.0));
            }
        }
    }
}

pub(crate) fn draw(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(study) = &app.vibration else { return };
    if !study.current(app) {
        return;
    }
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_body);
    if let Some((r, shape)) = study.shape() {
        let max = maximum(shape).max(f64::MIN_POSITIVE);
        let (real, imag) = shape
            .real
            .get(shape.maximum)
            .copied()
            .zip(shape.imaginary.get(shape.maximum).copied())
            .map(|(a, b)| (vector(a), vector(b)))
            .unwrap_or((Vec3::ZERO, Vec3::ZERO));
        let peak_phase = (2.0 * real.dot(imag)).atan2(real.dot(real) - imag.dot(imag)) * 0.5;
        let phase = if study.playing {
            painter.ctx().request_repaint_after(std::time::Duration::from_millis(30));
            painter.ctx().input(|i| i.time).rem_euclid(1.25) * std::f64::consts::TAU / 1.25
        } else {
            peak_phase
        };
        let (sin, cos) = phase.sin_cos();
        let scale = if study.mode {
            let size = r
                .vertices_mm
                .iter()
                .copied()
                .map(vector)
                .fold(Aabb3::EMPTY, |mut b, p| {
                    b.add(p);
                    b
                })
                .diagonal();
            size * study.mode_scale / 100.0
        } else {
            study.scale
        };
        let vertices: Vec<_> = r
            .vertices_mm
            .iter()
            .zip(&shape.real)
            .zip(&shape.imaginary)
            .zip(&shape.amplitude)
            .map(|(((&p, &a), &b), &m)| {
                let xyz = vector(p) + (vector(a) * cos + vector(b) * sin) * scale;
                (xyz, proj.to_screen(xyz), heat(m / max))
            })
            .collect();
        let eye = proj.cam.eye();
        let mut triangles = Vec::with_capacity(r.triangles.len());
        for tri in &r.triangles {
            let [a, b, c] = tri.vertices;
            let Some((&(p, Some(x), ca), (&(q, Some(y), cb), &(s, Some(z), cc)))) = vertices.get(a).zip(vertices.get(b).zip(vertices.get(c))) else {
                continue;
            };
            let centre = (p + q + s) * (1.0 / 3.0);
            if (q - p).cross(s - p).dot(eye - centre) > 0.0 {
                triangles.push(((centre - eye).len(), [(x, ca), (y, cb), (z, cc)]));
            }
        }
        triangles.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut mesh = egui::Mesh::default();
        for (_, vertices) in triangles {
            let Ok(i) = u32::try_from(mesh.vertices.len()) else { break };
            for (pos, color) in vertices {
                mesh.colored_vertex(pos, color);
            }
            mesh.add_triangle(i, i + 1, i + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
        if scale > 0.0 {
            undeformed_outline(r, painter, proj);
        }
        if let Some((_, Some(at), _)) = vertices.get(shape.maximum) {
            let label = if study.mode { "100%".to_owned() } else { format!("{:.4} mm", maximum(shape)) };
            painter.circle_filled(*at, 5.0, t.figure_warn);
            painter.text(*at + vec2(9.0, -8.0), Align2::LEFT_BOTTOM, label, font.clone(), t.figure_warn);
        }
    }
    for (i, a) in study.anchors.iter().enumerate() {
        if let Some(at) = proj.to_screen(a.point) {
            painter.circle_filled(at, 9.0, t.selected_bg);
            painter.circle_stroke(at, 9.0, Stroke::new(1.0, t.figure_accent));
            painter.text(at, Align2::CENTER_CENTER, (i + 1).to_string(), font.clone(), t.selected_ink);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vibration_window_runs_a_selected_support_and_draws_the_response() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"body_name":"Beam","length":180,"width":18,"height":12,"corner":[0,0,0]})).unwrap();
        h.app
            .run(
                "material.create",
                json!({"name":"Test steel","density_g_cm3":7.85,"physical_card":{
                    "schema_version":1,"designation":"Analytic specimen","family":"metal",
                    "sources":[{"id":"analytic","title":"Analytic specimen","kind":"illustrative"}],
                    "properties":[{"source_id":"analytic","data":{"kind":"elasticity","model":{
                        "symmetry":"isotropic","young_modulus_pa":200e9,"poisson_ratio":0.3}}}]}
                }),
            )
            .unwrap();
        h.app.run("material.assign", json!({"bodies":["Beam"],"material":"Test steel"})).unwrap();
        let world = h.app.session.world_state().unwrap();
        let mesh = world.body("Beam").unwrap().mesh();
        let index = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .find_map(|(tri, &face)| mesh.tri(tri).filter(|p| p.iter().all(|v| v.x.abs() < 1e-7)).map(|_| face as usize))
            .unwrap();
        h.app.session.selection = vec![Sel::Face { body: "Beam".into(), index, point: Vec3::new(0.0, 9.0, 6.0) }];
        h.app.start(COMMAND);
        h.frames(3);
        assert_eq!(h.app.vibration.as_ref().unwrap().anchors.len(), 1);
        h.step(&json!({"click":{"handle":"vibration:run"}})).unwrap();
        let study = h.app.vibration.as_mut().unwrap();
        if let Some(job) = study.pending.take() {
            let result = job.done.recv().unwrap();
            study.complete(result);
        }
        assert!(study.error.is_none(), "{:?}", study.error);
        h.frames(2);
        let study = h.app.vibration.as_ref().unwrap();
        let (result, peak) = study.shape().unwrap();
        assert!(result.has_resonance && maximum(peak) > 0.0);
        assert!(h.shapes.iter().any(|item| matches!(&item.shape, egui::Shape::Text(t) if t.galley.job.text == format!("{:.4} mm", maximum(peak)))));
    }
}
