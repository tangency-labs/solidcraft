//! The Point Cloud Deviation card (#14356): measures how far each point of a cloud lies from a
//! body with `pointcloud.deviation`, then colours the cloud by that distance and lets the pointer
//! probe a point. The colours and the numbers belong to the card, never to the design: nothing is
//! written to the cloud, nothing goes on the undo stack, and closing the card puts the cloud's own
//! colours back.
//!
//! The measurement runs beside the window (in the browser build, where there is one thread, it
//! runs at once) and Cancel stops it: a cancelled run shows nothing. A result remembers what it was
//! measured from (the cloud's points and placement, the body as evaluated now, the tolerance); once
//! any of them changes, its colours are taken off and its numbers are marked out of date.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use egui::{Painter, Stroke, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_engine::doc::ModelState;
use solidcraft_engine::doc::point_cloud::{ColorBy, Colors, PLAIN, PointCloud, byte, height_ramp};
use solidcraft_engine::doc::progress::Progress;
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::ui_kit;
use crate::viewport::Proj;

/// The tolerance the card starts with (mm), as the command's default.
const TOLERANCE: f64 = 0.1;
/// How near the pointer must be to a point to probe it (points on screen).
const PROBE_RADIUS: f32 = 8.0;
/// Steps the legend's colour bar is drawn in.
const LEGEND_STEPS: usize = 48;

/// What the card reads of the command's reply.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Report {
    pub signed: bool,
    pub tolerance_mm: f64,
    pub cloud_points: usize,
    pub sample_step: usize,
    pub sampled: usize,
    pub measured: usize,
    pub failed: usize,
    pub mean_abs_mm: Option<f64>,
    pub mean_signed_mm: Option<f64>,
    pub rms_mm: Option<f64>,
    pub max_abs_mm: Option<f64>,
    pub within_tolerance: usize,
    pub within_tolerance_fraction: Option<f64>,
    #[serde(default)]
    pub distances: Vec<Option<f64>>,
    #[serde(default)]
    pub closest: Vec<Option<[f64; 3]>>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// What a result was measured from. It is current while each part is the same: the cloud's point
/// data (by identity, so a reload under the same id is a change), its placement and units, the
/// body as the design evaluates it now (by identity: the state is kept, so its address cannot be
/// reused), and the analysis settings.
#[derive(Clone)]
struct Stamp {
    cloud: u64,
    points: Arc<[f32]>,
    len: usize,
    origin: [u64; 3],
    transform: [u64; 16],
    units: String,
    body: String,
    state: Arc<ModelState>,
    tolerance: u64,
}

impl Stamp {
    fn of(app: &SolveApp, cloud: u64, body: &str, tolerance: f64) -> Option<Stamp> {
        let c = app.session.doc.point_clouds.iter().find(|c| c.id == cloud)?;
        let state = app.session.world_state().ok()?;
        state.body(body)?;
        Some(Stamp {
            cloud,
            points: c.points.values().clone(),
            len: c.len(),
            origin: [c.origin.x, c.origin.y, c.origin.z].map(f64::to_bits),
            transform: std::array::from_fn(|i| c.transform[i / 4][i % 4].to_bits()),
            units: c.units.clone(),
            body: body.to_string(),
            state,
            tolerance: tolerance.to_bits(),
        })
    }

    /// Whether `now` measures the same thing.
    fn same(&self, now: &Stamp) -> bool {
        let body = |s: &Stamp| s.state.body(&s.body).map(|b| std::ptr::from_ref(b.body.record()));
        self.cloud == now.cloud
            && Arc::ptr_eq(&self.points, &now.points)
            && self.len == now.len
            && self.origin == now.origin
            && self.transform == now.transform
            && self.units == now.units
            && self.body == now.body
            && self.tolerance == now.tolerance
            && (Arc::ptr_eq(&self.state, &now.state) || body(self).is_some_and(|b| body(now) == Some(b)))
    }
}

/// A measurement running beside the window. Dropped unfinished (the card closed, a new
/// measurement started), it is cancelled.
struct Run {
    done: Receiver<Result<Value, String>>,
    progress: Arc<Progress>,
    stamp: Stamp,
}

impl Drop for Run {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// A finished measurement: its report, what it was measured from, and the cloud's colours by it.
pub(crate) struct Shown {
    pub report: Report,
    stamp: Stamp,
    colors: Colors,
    /// The distance at the ends of the colour scale (mm): ±`scale` signed, 0 to `scale` unsigned.
    pub scale: f64,
}

/// The Point Cloud Deviation card.
pub struct DeviationCard {
    pub cloud: u64,
    body: usize,
    tolerance: f64,
    run: Option<Run>,
    pub(crate) shown: Option<Shown>,
    error: Option<String>,
}

impl DeviationCard {
    pub fn new(cloud: u64) -> Self {
        DeviationCard { cloud, body: 0, tolerance: TOLERANCE, run: None, shown: None, error: None }
    }

    /// Whether a measurement is running.
    pub fn running(&self) -> bool {
        self.run.is_some()
    }
}

/// The bodies a cloud can be measured against: those with exact faces, by name.
fn bodies(app: &SolveApp) -> Vec<String> {
    app.session.world_state().map(|st| st.bodies.iter().filter(|b| !b.body.is_mesh()).map(|b| b.name.clone()).collect()).unwrap_or_default()
}

/// The colour of a distance on the scale.
fn ramp(signed: bool, scale: f64, d: f64) -> [u8; 3] {
    let scale = scale.max(f64::MIN_POSITIVE);
    let t = if signed { (d + scale) / (2.0 * scale) } else { d.abs() / scale };
    height_ramp(t.clamp(0.0, 1.0))
}

/// The cloud's colours by `report`: a sampled and measured point by its distance, every other
/// point grey.
fn colors(len: usize, report: &Report, scale: f64) -> Colors {
    let plain = PLAIN.map(byte);
    let step = report.sample_step.max(1);
    let mut out = Vec::with_capacity(len * 3);
    for i in 0..len {
        let d = if i % step == 0 { report.distances.get(i / step).copied().flatten() } else { None };
        out.extend(d.map_or(plain, |d| ramp(report.signed, scale, d)));
    }
    Colors(Arc::from(out))
}

/// Start measuring what the card names, replacing (and so cancelling) a measurement running.
fn start(app: &mut SolveApp, card: &mut DeviationCard) {
    card.run = None;
    card.error = None;
    let names = bodies(app);
    let Some(body) = names.get(card.body).cloned() else {
        card.error = Some(solidcraft_ui_text::tr("ui.deviation_no_body").to_string());
        return;
    };
    let Some(stamp) = Stamp::of(app, card.cloud, &body, card.tolerance) else {
        card.error = Some(solidcraft_ui_text::tr("ui.deviation_no_body").to_string());
        return;
    };
    let command = vec![(
        "pointcloud.deviation".to_string(),
        json!({ "cloud": card.cloud, "body": body, "tolerance": card.tolerance, "per_point": true, "worst": 0 }),
    )];
    let progress = Arc::new(Progress::default());
    let (tx, done) = std::sync::mpsc::channel();
    #[cfg(not(target_arch = "wasm32"))]
    if app.apply_off_thread.is_some() {
        let (ahead, watched, sent, list) = (app.session.ahead(), progress.clone(), tx.clone(), command.clone());
        let spawned = std::thread::Builder::new().name("point cloud deviation".into()).spawn(move || {
            let _ = sent.send(measure(ahead, watched, &list));
        });
        if spawned.is_ok() {
            card.run = Some(Run { done, progress, stamp });
            return;
        }
    }
    let _ = tx.send(measure(app.session.ahead(), progress.clone(), &command));
    card.run = Some(Run { done, progress, stamp });
}

/// Run the measurement on a copy of the session, with `progress` watching it so that Cancel
/// reaches the command.
fn measure(ahead: solidcraft_engine::Ahead, progress: Arc<Progress>, command: &[(String, Value)]) -> Result<Value, String> {
    let replies = solidcraft_engine::doc::progress::watch(progress, || ahead.replies(command));
    match replies.into_iter().next() {
        Some(reply) => reply.map_err(|e| e.to_string()),
        None => Err(String::new()),
    }
}

/// Take a finished measurement in: shown when it was not cancelled, its error otherwise.
fn collect(card: &mut DeviationCard) {
    let Some(run) = card.run.as_ref() else { return };
    let reply = match run.done.try_recv() {
        Ok(reply) => reply,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(String::new()),
    };
    let Some(run) = card.run.take() else { return };
    if run.progress.is_cancelled() {
        return;
    }
    match reply.and_then(|v| serde_json::from_value::<Report>(v).map_err(|e| e.to_string())) {
        Ok(report) => {
            let scale = report.max_abs_mm.unwrap_or(0.0).max(report.tolerance_mm);
            let colors = colors(run.stamp.len, &report, scale);
            card.shown = Some(Shown { report, stamp: run.stamp.clone(), colors, scale });
        }
        Err(e) => card.error = Some(e),
    }
}

/// The card's result, when it is current: measured from the cloud, the body and the settings as
/// they are now.
fn current<'a>(app: &SolveApp, card: &'a DeviationCard) -> Option<&'a Shown> {
    let shown = card.shown.as_ref()?;
    let names = bodies(app);
    let body = names.get(card.body)?;
    let now = Stamp::of(app, card.cloud, body, card.tolerance)?;
    shown.stamp.same(&now).then_some(shown)
}

/// The cloud as the view draws it: coloured by its distances while the card shows a current
/// result for it, otherwise as it is.
pub(crate) fn drawn<'a>(app: &SolveApp, c: &'a PointCloud) -> std::borrow::Cow<'a, PointCloud> {
    let Some(card) = app.tree.deviation_card.as_ref().filter(|card| card.cloud == c.id) else {
        return std::borrow::Cow::Borrowed(c);
    };
    let Some(shown) = current(app, card) else {
        return std::borrow::Cow::Borrowed(c);
    };
    let mut copy = c.clone();
    copy.colors = shown.colors.clone();
    copy.color_by = ColorBy::File;
    std::borrow::Cow::Owned(copy)
}

/// A distance as the card writes it (mm).
fn mm(v: f64) -> String {
    format!("{v:.4} mm")
}

/// The colour bar: the scale's ends, zero for a signed result, and the tolerance band marked.
fn legend(ui: &mut egui::Ui, shown: &Shown) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 12.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let signed = shown.report.signed;
    let lo = if signed { -shown.scale } else { 0.0 };
    let width = rect.width() / LEGEND_STEPS as f32;
    for k in 0..LEGEND_STEPS {
        let d = lo + (shown.scale - lo) * (k as f64 + 0.5) / LEGEND_STEPS as f64;
        let x = rect.left() + width * k as f32;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(width + 0.5, rect.height())),
            0.0,
            crate::theme::model_color(ramp(signed, shown.scale, d)),
        );
    }
    let at = |d: f64| rect.left() + rect.width() * ((d - lo) / (shown.scale - lo).max(f64::MIN_POSITIVE)) as f32;
    let tol = shown.report.tolerance_mm;
    for d in if signed { vec![-tol, tol] } else { vec![tol] } {
        let x = at(d);
        painter.line_segment([egui::pos2(x, rect.top() - 2.0), egui::pos2(x, rect.bottom() + 2.0)], Stroke::new(1.5, t.text));
    }
    ui.horizontal(|ui| {
        ui.label(mm(lo));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(mm(shown.scale)));
    });
    note(ui, &solidcraft_ui_text::format("ui.deviation_band", &[("tolerance", &mm(tol))]));
    note(ui, solidcraft_ui_text::tr(if signed { "ui.deviation_signed" } else { "ui.deviation_unsigned" }));
    note(ui, solidcraft_ui_text::tr("ui.deviation_grey"));
}

/// A line of the card that wraps rather than being cut short.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).wrap());
}

/// A wrapping line in the warning colour.
fn warn(ui: &mut egui::Ui, color: egui::Color32, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
}

/// The numbers of a result.
fn numbers(ui: &mut egui::Ui, r: &Report) {
    let none = || "-".to_string();
    ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_measured"), |ui| {
        ui.label(solidcraft_ui_text::format("ui.deviation_counts", &[("measured", &r.measured.to_string()), ("sampled", &r.sampled.to_string())]))
    });
    ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_sampled"), |ui| {
        ui.label(solidcraft_ui_text::format(
            "ui.deviation_sampling",
            &[("step", &r.sample_step.to_string()), ("points", &r.cloud_points.to_string())],
        ))
    });
    if r.failed > 0 {
        ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_failed"), |ui| ui.label(r.failed.to_string()));
    }
    ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_mean"), |ui| ui.label(r.mean_abs_mm.map_or_else(none, mm)));
    if r.signed {
        ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_mean_signed"), |ui| ui.label(r.mean_signed_mm.map_or_else(none, mm)));
    }
    ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_rms"), |ui| ui.label(r.rms_mm.map_or_else(none, mm)));
    ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_max"), |ui| ui.label(r.max_abs_mm.map_or_else(none, mm)));
    ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_within"), |ui| {
        ui.label(r.within_tolerance_fraction.map_or_else(none, |f| {
            solidcraft_ui_text::format(
                "ui.deviation_within_value",
                &[("within", &r.within_tolerance.to_string()), ("measured", &r.measured.to_string()), ("percent", &format!("{:.1}", f * 100.0))],
            )
        }))
    });
    for w in &r.warnings {
        note(ui, w);
    }
}

/// The card, while it is open.
pub(crate) fn card(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut card) = app.tree.deviation_card.take() else { return };
    let Some(cloud) = app.session.doc.point_clouds.iter().find(|c| c.id == card.cloud) else {
        return;
    };
    let name = cloud.name.clone();
    let points = cloud.len();
    collect(&mut card);
    if card.running() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let names = bodies(app);
    let fresh = current(app, &card).is_some();
    let t = Tokens::get();
    let title = solidcraft_ui_text::format("ui.deviation_title", &[("name", &name)]);
    let (pivot, at) = crate::shell::floating_editor_anchor(app, ctx);
    let mut measure = false;
    let mut cancel = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_point_cloud_deviation"), "compare", &title)
        .placed(pivot, at)
        .fields()
        .actions("", solidcraft_ui_text::tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui_kit::row(ui, solidcraft_ui_text::tr("ui.points"), |ui| ui.label(points.to_string()));
            if names.is_empty() {
                ui.label(solidcraft_ui_text::tr("ui.deviation_no_body"));
            } else {
                let options: Vec<&str> = names.iter().map(String::as_str).collect();
                card.body = card.body.min(names.len() - 1);
                ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_body"), |ui| {
                    ui_kit::dropdown(ui, "point_cloud_deviation_body", &mut card.body, &options)
                });
            }
            ui_kit::row(ui, solidcraft_ui_text::tr("ui.deviation_tolerance"), |ui| {
                ui_kit::number_field(ui, &mut card.tolerance, 0.0001..=1000.0, 0.01, " mm")
            });
            if card.running() {
                ui.horizontal(|ui| {
                    ui.label(solidcraft_ui_text::tr("ui.deviation_measuring"));
                    cancel = ui_kit::button(ui, solidcraft_ui_text::tr("ui.cancel"), ui_kit::Button::Normal).clicked();
                });
            } else if !names.is_empty() {
                measure = ui_kit::primary_button(ui, solidcraft_ui_text::tr("ui.deviation_measure")).clicked();
            }
            if let Some(e) = &card.error {
                warn(ui, t.form_error, e);
            }
            if let Some(shown) = card.shown.as_ref() {
                if fresh {
                    legend(ui, shown);
                    numbers(ui, &shown.report);
                } else {
                    warn(ui, t.form_error, solidcraft_ui_text::tr("ui.deviation_stale"));
                }
            }
        });
    if cancel {
        card.run = None;
    }
    if measure {
        start(app, &mut card);
        collect(&mut card);
    }
    if !(closed || action == ui_kit::Action::Cancel) {
        app.tree.deviation_card = Some(card);
    }
}

/// The point under the pointer, while the card shows a current result for a shown cloud: a ring
/// on it, a dot on its closest point on the body and a line between, and its distance.
pub(crate) fn probe(app: &SolveApp, painter: &Painter, proj: &Proj) {
    let Some(card) = app.tree.deviation_card.as_ref() else { return };
    let Some(shown) = current(app, card) else { return };
    let Some(c) = solidcraft_engine::view::shown_point_clouds(&app.session).find(|c| c.id == card.cloud) else {
        return;
    };
    let Some(pointer) = painter.ctx().pointer_hover_pos().filter(|p| proj.rect.contains(*p)) else {
        return;
    };
    // Only the points the view shows: the cloud's filters, one playing at its range now.
    let drawn = crate::point_clouds::as_drawn(c, |k| app.tree.cloud_panel.as_ref().and_then(|p| p.playing(c.id, k)));
    let kept = if drawn.filters.is_empty() { Vec::new() } else { solidcraft_engine::doc::point_cloud_distribution::kept_points(&drawn, &drawn.filters) };
    let Some((index, k, at)) = nearest(c, &shown.report, &kept, proj, pointer) else { return };
    let (Some(d), Some(closest)) = (shown.report.distances.get(k).copied().flatten(), shown.report.closest.get(k).copied().flatten()) else {
        return;
    };
    let t = Tokens::get();
    let foot = proj.to_screen(closest.into());
    if let Some(foot) = foot {
        painter.line_segment([at, foot], Stroke::new(1.5, t.accent));
        painter.circle_filled(foot, 3.0, t.accent);
    }
    painter.circle_stroke(at, 6.0, Stroke::new(2.0, t.accent));
    let text = solidcraft_ui_text::format(
        "ui.deviation_probe",
        &[
            ("index", &index.to_string()),
            ("distance", &mm(d)),
            ("x", &format!("{:.4}", closest[0])),
            ("y", &format!("{:.4}", closest[1])),
            ("z", &format!("{:.4}", closest[2])),
        ],
    );
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(t.font_body), t.text);
    // Beside the point, on the side of the view with more room.
    let size = galley.size();
    let left = if at.x > proj.rect.center().x { at.x - 12.0 - size.x } else { at.x + 12.0 };
    let rect = egui::Rect::from_min_size(egui::pos2(left, at.y - 12.0 - size.y), size).expand(4.0);
    painter.rect_filled(rect, t.radius_sm, t.panel);
    painter.galley(rect.min + vec2(4.0, 4.0), galley, t.text);
}

/// The measured point nearest `pointer` on screen, within [`PROBE_RADIUS`], among those `kept`
/// (shown): its index in the cloud, its place in the report, and where it is on screen.
fn nearest(c: &PointCloud, report: &Report, kept: &[bool], proj: &Proj, pointer: egui::Pos2) -> Option<(usize, usize, egui::Pos2)> {
    let step = report.sample_step.max(1);
    let mut best: Option<(f32, usize, usize, egui::Pos2)> = None;
    for (k, d) in report.distances.iter().enumerate() {
        if d.is_none() {
            continue;
        }
        let index = k * step;
        if !kept.get(index).copied().unwrap_or(true) {
            continue;
        }
        let Some(at) = c.world_point(index).and_then(|p| proj.to_screen(p)) else { continue };
        let gap = at.distance(pointer);
        if gap <= PROBE_RADIUS && best.is_none_or(|b| gap < b.0) {
            best = Some((gap, index, k, at));
        }
    }
    best.map(|(_, index, k, at)| (index, k, at))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::DeviationCard;
    use solidcraft_engine::doc::point_cloud::ColorBy;

    /// A cloud of `points` from an XYZ file, by id.
    fn cloud(h: &mut crate::scenario::Harness, test: &str, points: &[[f64; 3]]) -> u64 {
        let dir = std::env::temp_dir().join(format!("solidcraft-deviation-card-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scan.xyz");
        let text: String = points.iter().map(|p| format!("{} {} {}\n", p[0], p[1], p[2])).collect();
        std::fs::write(&path, text).unwrap();
        h.app.run("pointcloud.insert", json!({"path": path.to_string_lossy()})).unwrap()["cloud"].as_u64().unwrap()
    }

    fn harness_with_box_and_cloud(test: &str) -> (crate::scenario::Harness, u64) {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Box"})).unwrap();
        let id = cloud(&mut h, test, &[[5.0, 5.0, 10.05], [10.0, 10.0, 10.3], [15.0, 5.0, 9.8]]);
        (h, id)
    }

    #[test]
    fn measuring_colours_the_cloud_without_changing_the_design_and_closing_puts_it_back() {
        let (mut h, id) = harness_with_box_and_cloud("measure");
        let undo = h.app.session.undo.len();
        let revision = h.app.session.revision;
        h.app.tree.deviation_card = Some(DeviationCard::new(id));
        h.frames(2);
        h.step(&json!({"widget": solidcraft_ui_text::tr("ui.deviation_measure")})).unwrap();
        h.frames(2);
        let card = h.app.tree.deviation_card.as_ref().unwrap();
        let shown = card.shown.as_ref().expect("a result");
        assert_eq!((shown.report.measured, shown.report.sampled), (3, 3));
        let d: Vec<f64> = shown.report.distances.iter().map(|d| d.unwrap()).collect();
        for (got, want) in d.iter().zip([0.05, 0.3, -0.2]) {
            assert!((got - want).abs() <= 1e-5, "{got} vs {want}");
        }
        // The view draws the cloud by its distances; the design's cloud is as it was.
        let c = h.app.session.doc.point_clouds.iter().find(|c| c.id == id).unwrap().clone();
        let drawn = super::drawn(&h.app, &c);
        assert!(matches!(drawn, std::borrow::Cow::Owned(_)));
        assert_eq!(drawn.color_by, ColorBy::File);
        assert_eq!(drawn.colors.len(), 3);
        assert!(!std::sync::Arc::ptr_eq(&drawn.colors.0, &c.colors.0));
        assert_eq!((h.app.session.undo.len(), h.app.session.revision), (undo, revision));
        // The probe agrees with the report: the closest point is straight below the first point.
        let closest = shown.report.closest[0].unwrap();
        assert!((closest[0] - 5.0).abs() < 1e-5 && (closest[1] - 5.0).abs() < 1e-5 && (closest[2] - 10.0).abs() < 1e-5);
        h.step(&json!({"widget": "Close"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.deviation_card.is_none());
        assert!(matches!(super::drawn(&h.app, &c), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn a_changed_cloud_body_or_tolerance_takes_the_colours_off() {
        let (mut h, id) = harness_with_box_and_cloud("stale");
        let coloured = |h: &crate::scenario::Harness| {
            let c = h.app.session.doc.point_clouds.iter().find(|c| c.id == id).unwrap();
            matches!(super::drawn(&h.app, c), std::borrow::Cow::Owned(_))
        };
        let measure = |h: &mut crate::scenario::Harness| {
            h.step(&json!({"widget": solidcraft_ui_text::tr("ui.deviation_measure")})).unwrap();
            h.frames(2);
        };
        h.app.tree.deviation_card = Some(DeviationCard::new(id));
        h.frames(2);
        measure(&mut h);
        assert!(coloured(&h));
        // The tolerance changed in the card: the result no longer answers it.
        h.app.tree.deviation_card.as_mut().unwrap().tolerance = 0.2;
        assert!(!coloured(&h));
        h.app.tree.deviation_card.as_mut().unwrap().tolerance = super::TOLERANCE;
        assert!(coloured(&h));
        // The cloud's units changed: its points moved.
        h.app.run("pointcloud.edit", json!({"cloud": id, "units": "cm"})).unwrap();
        assert!(!coloured(&h));
        h.app.run("pointcloud.edit", json!({"cloud": id, "units": "mm"})).unwrap();
        measure(&mut h);
        assert!(coloured(&h));
        // The body moved.
        h.app.run("solid.move", json!({"bodies": ["Box"], "translate": [0, 0, 1]})).unwrap();
        assert!(!coloured(&h));
        h.frames(2);
        assert!(h.widget(solidcraft_ui_text::tr("ui.deviation_stale")).is_some());
        // The cloud read again under the same id.
        measure(&mut h);
        assert!(coloured(&h));
        h.app.run("pointcloud.reload", json!({"cloud": id})).unwrap();
        assert!(!coloured(&h));
    }

    #[test]
    fn a_cancelled_measurement_shows_nothing() {
        let (mut h, id) = harness_with_box_and_cloud("cancel");
        let mut card = DeviationCard::new(id);
        let stamp = super::Stamp::of(&h.app, id, "Box", card.tolerance).unwrap();
        let (tx, done) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        card.run = Some(super::Run { done, progress: progress.clone(), stamp });
        // Cancelled, then its reply comes in anyway: it is dropped.
        progress.cancel();
        tx.send(Ok(json!({}))).unwrap();
        super::collect(&mut card);
        assert!(card.run.is_none() && card.shown.is_none() && card.error.is_none());
        // Dropping a running measurement cancels it.
        let stamp = super::Stamp::of(&h.app, id, "Box", card.tolerance).unwrap();
        let (_tx, done) = std::sync::mpsc::channel::<Result<serde_json::Value, String>>();
        let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        card.run = Some(super::Run { done, progress: progress.clone(), stamp });
        h.app.tree.deviation_card = Some(card);
        h.app.tree.deviation_card = None;
        assert!(progress.is_cancelled());
    }
}
