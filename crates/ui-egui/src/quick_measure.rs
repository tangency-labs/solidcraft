//! Quick measurement (#7000): what the selection measures, in the view's bottom right corner above
//! History, so a simple size needs no Measure tool. One item gives the Measure tool's rows (an
//! edge's length, Ø and R for a round one, a face's area, a point's position); two give their
//! distance and angle; several edges or several faces their count and total length or area.
//! Computed off the window thread from a copy of the session ([`Session::measure_items`]) whenever
//! the selection or the design changes; nothing shows while it computes, or for a selection with
//! nothing to measure. A click copies the readout.

use crate::ui_kit::BackingTooltipExt as _;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;

/// Items summed into a total, at most.
const MAX_SUMMED: usize = 200;

/// What a selection and design were measured as, and the measurement once it is in.
#[derive(Default)]
pub struct QuickMeasure {
    /// The selection and design the measurement is for.
    key: Option<Key>,
    job: Option<std::sync::mpsc::Receiver<Option<Value>>>,
    result: Option<Value>,
}

#[derive(Clone, PartialEq)]
struct Key {
    selection: Vec<Sel>,
    doc: usize,
    revision: u64,
}

impl QuickMeasure {
    /// The readout as shown: each line a label and its value.
    pub(crate) fn lines(&self, app: &SolveApp) -> Vec<(String, String)> {
        self.result.as_ref().map(|r| lines(app, r)).unwrap_or_default()
    }
}

/// Is the readout wanted at all: the preference, a design shown, no Measure tool saying the same.
fn wanted(app: &SolveApp) -> bool {
    app.preferences.quick_measure
        && !app.home.open
        && !crate::render_workspace::active(app)
        && !app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Measure { .. }))
}

/// Can `sel` be measured here? A sketch's curves, points and regions are, open or only shown.
fn measurable(sel: &Sel) -> bool {
    matches!(
        sel,
        Sel::Edge { .. }
            | Sel::Face { .. }
            | Sel::Vertex { .. }
            | Sel::SketchCurve { .. }
            | Sel::SketchPoint { .. }
            | Sel::ExternalSketchCurve { .. }
            | Sel::ExternalSketchPoint { .. }
            | Sel::Profile { .. }
    )
}

/// Each frame: start measuring a selection that changed, take a measurement that is in.
pub(crate) fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    let selection: Vec<Sel> = app.session.selection.iter().filter(|s| measurable(s)).cloned().collect();
    if !wanted(app) || selection.is_empty() || selection.len() != app.session.selection.len() {
        app.quick_measure = QuickMeasure::default();
        return;
    }
    let key = Key { selection, doc: std::sync::Arc::as_ptr(&app.session.doc) as usize, revision: app.session.revision };
    if app.quick_measure.key.as_ref() != Some(&key) {
        app.quick_measure = QuickMeasure { job: start(app, &key.selection), key: Some(key), result: None };
    }
    if let Some(job) = &app.quick_measure.job {
        match job.try_recv() {
            Ok(result) => {
                app.quick_measure.result = result;
                app.quick_measure.job = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(30)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => app.quick_measure.job = None,
        }
    }
}

/// Measure `selection` on a copy of the session, off the window thread.
fn start(app: &SolveApp, selection: &[Sel]) -> Option<std::sync::mpsc::Receiver<Option<Value>>> {
    let session = app.session.scratch();
    let selection = selection.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    let measure = move || {
        let _ = tx.send(measure(&session, &selection));
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::Builder::new().name("quick measure".into()).spawn(measure).ok()?;
    }
    #[cfg(target_arch = "wasm32")]
    measure();
    Some(rx)
}

/// One or two items as the Measure tool measures them; more, their count and total length (edges
/// and sketch curves) or area (faces and sketch regions). `None`: nothing to show.
fn measure(session: &solidcraft_engine::Session, selection: &[Sel]) -> Option<Value> {
    if selection.len() <= 2 {
        let r = session.measure_items(selection);
        return r.get("error").is_none().then_some(r);
    }
    let lengths = selection.iter().all(|s| matches!(s, Sel::Edge { .. } | Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. }));
    let areas = selection.iter().all(|s| matches!(s, Sel::Face { .. } | Sel::Profile { .. }));
    if !(lengths || areas) || selection.len() > MAX_SUMMED {
        return None;
    }
    let mut total = 0.0;
    for sel in selection {
        let r = session.measure_items(std::slice::from_ref(sel));
        let it = r.get("items").and_then(|items| items.get(0))?;
        total += if areas {
            it["area_mm2"].as_f64()?
        } else {
            it["length_mm"].as_f64().or_else(|| it["circumference"].as_f64()).or_else(|| it["arc_length"].as_f64())?
        };
    }
    Some(json!({"count": selection.len(), if areas { "total_area_mm2" } else { "total_length_mm" }: total}))
}

/// One measured item's rows, each "label  value": Ø, R and the length of a round one (its
/// circumference, or an arc's length and angle), a slot's sizes, a straight edge's length, a face's
/// area, a point's position. The Measure tool lists the same.
pub(crate) fn item_lines(app: &SolveApp, it: &Value) -> Vec<String> {
    let tr = crate::i18n::tr;
    let mm = |v: &Value| v.as_f64().map(|x| crate::prefs::show_mm(app, x, 1)).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    if it.get("diameter").is_some() {
        lines.push(format!("Ø  {}", mm(&it["diameter"])));
        lines.push(format!("R  {}", mm(&it["radius"])));
        if it.get("circumference").is_some() {
            lines.push(format!("{}  {}", tr("Circumference"), mm(&it["circumference"])));
        } else if let Some(a) = it["angle_deg"].as_f64() {
            lines.push(format!("{}  {}", tr("Arc length"), mm(&it["arc_length"])));
            lines.push(format!("{}  {a:.2}°", crate::i18n::tr_label("Angle")));
        }
    }
    if let Some(sl) = it.get("slot") {
        lines.push(format!("{}  {}", tr("Slot width"), mm(&sl["width"])));
        lines.push(format!("{}  {}", tr("Centre to centre"), mm(&sl["center_distance"])));
        lines.push(format!("{}  {}", tr("Overall length"), mm(&sl["overall_length"])));
    }
    let round = it.get("diameter").is_some();
    match it["type"].as_str() {
        Some("edge" | "sketch_curve") if !round => lines.push(format!("{}  {}", tr("Length"), mm(&it["length_mm"]))),
        Some("face" | "profile") => lines.push(format!("{}  {}", tr("Area"), crate::prefs::show_mm(app, it["area_mm2"].as_f64().unwrap_or(0.0), 2))),
        Some("vertex" | "sketch_point" | "point") => {
            let p = &it["position"];
            lines.push(format!("{}  {}, {}, {}", tr("measure.at"), mm(&p[0]), mm(&p[1]), mm(&p[2])));
        }
        Some("plane") => {
            let p = &it["point"];
            lines.push(format!("{}  {}, {}, {}", tr("measure.at"), mm(&p[0]), mm(&p[1]), mm(&p[2])));
            let n = &it["normal"];
            lines.push(format!("Normal  {}, {}, {}", mm(&n[0]), mm(&n[1]), mm(&n[2])));
        }
        Some("axis") => {
            let p = &it["origin"];
            lines.push(format!("{}  {}, {}, {}", tr("measure.at"), mm(&p[0]), mm(&p[1]), mm(&p[2])));
            let d = &it["direction"];
            lines.push(format!("Direction  {}, {}, {}", mm(&d[0]), mm(&d[1]), mm(&d[2])));
        }
        _ => {}
    }
    lines
}

/// The readout's rows for a measurement.
fn lines(app: &SolveApp, r: &Value) -> Vec<(String, String)> {
    let split = |line: String| match line.split_once("  ") {
        Some((label, value)) => (label.to_string(), value.to_string()),
        None => (String::new(), line),
    };
    if let Some(n) = r["count"].as_u64() {
        let n = n.to_string();
        return match (r["total_length_mm"].as_f64(), r["total_area_mm2"].as_f64()) {
            (Some(l), _) => vec![(crate::i18n::format("measure.quick_edges", &[("count", &n)]), crate::prefs::show_mm(app, l, 1))],
            (_, Some(a)) => vec![(crate::i18n::format("measure.quick_faces", &[("count", &n)]), crate::prefs::show_mm(app, a, 2))],
            _ => Vec::new(),
        };
    }
    let items = r["items"].as_array().map(Vec::as_slice).unwrap_or_default();
    if let [one] = items {
        return item_lines(app, one).into_iter().map(split).collect();
    }
    let mut rows = Vec::new();
    if let Some(d) = r["center_distance"].as_f64() {
        rows.push((crate::i18n::tr("Centre to centre").to_string(), crate::prefs::show_mm(app, d, 1)));
    }
    if let Some(d) = r["distance_mm"].as_f64() {
        rows.push((crate::i18n::tr_label("Distance").to_string(), crate::prefs::show_mm(app, d, 1)));
    }
    if let Some(a) = r["angle_deg"].as_f64() {
        rows.push((crate::i18n::tr_label("Angle").to_string(), format!("{a:.2}°")));
    }
    rows
}

/// The readout in the bottom right corner of the view, above History, kept clear of `taken` (the
/// panels and cards over the view, the navigation bar): left of one standing along the right edge,
/// above any other. Returns where it is.
pub(crate) fn show(app: &mut SolveApp, ui: &egui::Ui, view: egui::Rect, taken: &[egui::Rect]) -> Option<egui::Rect> {
    let rows = app.quick_measure.lines(app);
    if rows.is_empty() || !view.is_finite() || !view.is_positive() {
        return None;
    }
    let t = crate::theme::Tokens::get();
    let painter = ui.painter().clone();
    let font = egui::FontId::proportional(t.font_label);
    let pad = egui::vec2(t.space_8, t.space_4);
    let gap = t.space_8;
    let galleys: Vec<_> = rows
        .iter()
        .map(|(label, value)| {
            (painter.layout_no_wrap(label.clone(), font.clone(), t.text_dim), painter.layout_no_wrap(value.clone(), font.clone(), t.text))
        })
        .collect();
    let label_w = galleys.iter().map(|(l, _)| l.size().x).fold(0.0, f32::max);
    let value_w = galleys.iter().map(|(_, v)| v.size().x).fold(0.0, f32::max);
    let row_h = galleys.iter().map(|(l, v)| l.size().y.max(v.size().y)).fold(0.0, f32::max);
    let size = egui::vec2(pad.x * 2.0 + label_w + gap + value_w, pad.y * 2.0 + row_h * galleys.len() as f32);
    let mut r = egui::Rect::from_min_size(view.right_bottom() - size - egui::vec2(t.space_8, t.space_8), size);
    // Each step clears one rectangle, so this ends after at most two moves per one.
    for _ in 0..=2 * taken.len() {
        let Some(o) = taken.iter().find(|o| o.expand(t.space_4).intersects(r)) else { break };
        r = if o.height() > view.height() * 0.5 {
            r.translate(egui::vec2(o.left() - t.space_8 - r.right(), 0.0))
        } else {
            r.translate(egui::vec2(0.0, o.top() - t.space_8 - r.bottom()))
        };
    }
    if !view.contains_rect(r) {
        return None;
    }
    let response =
        ui.interact(r, egui::Id::new("sc_quick_measure"), egui::Sense::click()).backing_on_hover_text(crate::i18n::tr("measure.quick_copy"));
    crate::scenario::publish_rect("measure:quick", r);
    painter.rect_filled(r, t.radius_sm, if response.hovered() { t.hover } else { t.overlay });
    painter.rect_stroke(r, t.radius_sm, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, (label, value)) in galleys.into_iter().enumerate() {
        let y = r.top() + pad.y + row_h * i as f32;
        painter.galley(egui::pos2(r.left() + pad.x, y), label, t.text_dim);
        painter.galley(egui::pos2(r.right() - pad.x - value.size().x, y), value, t.text);
    }
    if response.clicked() {
        let text = rows
            .iter()
            .map(|(label, value)| if label.is_empty() { value.clone() } else { format!("{label} {value}") })
            .collect::<Vec<_>>()
            .join("\n");
        ui.ctx().copy_text(text);
        app.set_status(crate::i18n::tr("measure.quick_copied"), false);
    }
    Some(r)
}
