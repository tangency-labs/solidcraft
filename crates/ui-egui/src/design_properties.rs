//! The Design Properties window (#1015): the design's project, part number, revision, author,
//! organization, description and licence, and a read-only list of the data other tools keep in
//! the design. OK applies the edited fields through one `document.metadata.set` (one undo step);
//! Cancel and the close button change nothing. File → Design Properties…, the Browser's
//! Document Settings, the root component's menu and `ui.start` of `document.metadata` or
//! `document.metadata.set` open it. [`show_object`] draws the Properties card (mass, volume, area,
//! centre of mass, bounding box, counts, and for a component its first moments and moment of
//! inertia: #8095) of the body or component a menu or Inspect → Measure → Properties asked
//! about; both are `ui_kit::Window` cards. While the card is open, [`overlay`] marks the centre
//! of mass in the view with the principal axes standing out of it.

use solidcraft_engine::geom::prelude::*;
use std::sync::Arc;

use egui::{Align2, FontId, Stroke};
use serde_json::{Map, Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::doc::metadata::{DESCRIPTION, Owner, keys};
use solidcraft_engine::doc::progress::Progress;

use crate::SolveApp;
use crate::ui_kit;
use crate::viewport::Proj;

/// Longest preview of another tool's value in the list.
const PREVIEW_CHARS: usize = 80;
/// Rows of other tools' data shown before the list scrolls.
const OTHERS_ROWS: f32 = 6.0;

/// What is being typed; the properties themselves live in the document.
pub struct DesignProperties {
    /// Each of SolidCraft's design properties' text when the window opened, in
    /// `keys(Owner::Design)` order.
    pub opened: Vec<String>,
    /// The fields as edited.
    pub fields: Vec<String>,
    /// The last refusal from `document.metadata.set`.
    pub error: Option<String>,
    /// What the fields were when OK was refused: `error` stays only while they still are.
    refused: Option<Vec<String>>,
}

impl DesignProperties {
    pub fn new(app: &SolveApp) -> Self {
        let m = &app.session.doc.metadata;
        let opened: Vec<String> = keys(Owner::Design).map(|k| m.text(k.name).unwrap_or_default().to_string()).collect();
        DesignProperties { fields: opened.clone(), opened, error: None, refused: None }
    }

    /// Show why OK was refused, for as long as the fields stay as they are.
    fn refuse(&mut self, error: String) {
        self.refused = Some(self.fields.clone());
        self.error = Some(error);
    }

    /// The fields that differ from what the window opened with, as `document.metadata.set`
    /// takes them (empty text removes the key).
    pub fn changes(&self) -> Map<String, Value> {
        keys(Owner::Design)
            .zip(self.opened.iter().zip(&self.fields))
            .filter(|(_, (was, now))| was != now)
            .map(|(k, (_, now))| (k.name.to_string(), Value::String(now.clone())))
            .collect()
    }
}

/// Open the window (a fresh copy of the design's properties).
pub fn open(app: &mut SolveApp) {
    app.design_properties = Some(DesignProperties::new(app));
}

/// Apply the edits; the window stays open with the reason when they are refused.
fn apply(app: &mut SolveApp, d: &mut DesignProperties) -> bool {
    let values = d.changes();
    if values.is_empty() {
        return true;
    }
    match app.run("document.metadata.set", serde_json::json!({ "values": values })) {
        Ok(_) => true,
        Err(e) => {
            d.refuse(e);
            false
        }
    }
}

/// A short one-line view of a value another tool keeps.
fn preview(v: &Value) -> String {
    let text = serde_json::to_string(v).unwrap_or_default();
    if text.chars().count() <= PREVIEW_CHARS { text } else { format!("{}…", text.chars().take(PREVIEW_CHARS).collect::<String>()) }
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.design_properties.take() else { return };
    // A refused OK's message is about the fields it ran with (#641): once they change, it goes.
    if d.refused.as_ref().is_some_and(|was| *was != d.fields) {
        d.error = None;
        d.refused = None;
    }
    let metadata = app.session.doc.metadata.clone();
    let title = solidcraft_ui_text::tr_label("Design Properties");
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_design_properties"), "settings", title).fields().show(ctx, true, |ui| {
        for (i, k) in keys(Owner::Design).enumerate() {
            let (key, label) = (k.name, solidcraft_ui_text::tr(k.label));
            let Some(field) = d.fields.get_mut(i) else { continue };
            if key == DESCRIPTION {
                ui_kit::multiline_text_field(ui, label, egui::Id::unique(("sc_design_properties", key)), field);
            } else {
                ui_kit::row(ui, label, |ui| ui_kit::text_field(ui, field, ""));
            }
            // Saved by another tool as something other than text: kept unless typed over.
            if metadata.get(key).is_some() && metadata.text(key).is_none() && d.opened.get(i).is_some_and(String::is_empty) {
                ui_kit::caption(ui, label, solidcraft_ui_text::tr("ui.design_properties.not_text"));
            }
        }
        ui_kit::text(ui, solidcraft_ui_text::tr("ui.design_properties.exports"), ui_kit::Text::Note("exports"));
        let others: Vec<(&String, &Value)> = metadata.others().collect();
        if !others.is_empty() {
            let heading = format!("{} ({})", solidcraft_ui_text::tr("ui.design_properties.others"), others.len());
            ui_kit::section(ui, "sc_design_properties_others", &heading, false, |ui| {
                ui_kit::text(ui, solidcraft_ui_text::tr("ui.design_properties.others_note"), ui_kit::Text::Note("others"));
                egui::ScrollArea::vertical().max_height(ui_kit::row_height() * OTHERS_ROWS).show(ui, |ui| {
                    for (key, value) in others {
                        ui_kit::row(ui, key, |ui| ui_kit::text(ui, &preview(value), ui_kit::Text::Cell));
                    }
                });
            });
        }
        if let Some(error) = &d.error {
            ui_kit::text(ui, error, ui_kit::Text::Report("error", true));
        }
    });
    if closed || action == ui_kit::Action::Cancel || (action == ui_kit::Action::Ok && apply(app, &mut d)) {
        return;
    }
    app.design_properties = Some(d);
}

/// Open the Properties window on `command`'s reply (`component.properties`,
/// `occurrence.properties` or `inspect.measure`), titled with the reply's name, else `fallback`.
/// A refusal goes to the status line.
pub fn open_object(app: &mut SolveApp, command: &str, params: &Value, fallback: String) {
    close_object(app);
    // A large assembly takes seconds to measure, which froze the window (#5203): measured on a
    // copy beside it, the card shows when done. Headless and in tests (no wait set), here.
    // The worker reports how many bodies it has measured, and stops when the card closes (#8706).
    let progress = Arc::new(Progress::default());
    // Tests hold the reply until let go (`apply_gate`), so a window thread held up under load
    // cannot find the measurement already in where the card should still be measuring.
    #[cfg(test)]
    let gate = app.apply_gate.clone();
    let hold = move || {
        #[cfg(test)]
        if let Some(gate) = gate
            && let Ok(gate) = gate.lock()
        {
            let _ = gate.recv();
        }
    };
    if let Some(wait) = app.apply_off_thread
        && let Some(rx) = measure_beside(app.session.scratch(), command, params, progress.clone(), hold)
    {
        match rx.recv_timeout(wait) {
            Ok(reply) => opened(app, reply, fallback),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => app.menu.props_measuring = Some(Measuring { fallback, rx, progress }),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => app.set_status(app.tr("ui.properties.measuring_failed").to_string(), true),
        }
        return;
    }
    let reply = app.session.execute(command, params).map_err(|e| e.to_string());
    opened(app, reply, fallback);
}

/// Close the Properties card, and drop a measurement still running for it.
pub fn close_object(app: &mut SolveApp) {
    app.menu.props = None;
    app.menu.props_measuring = None;
}

/// A Properties measurement running beside the window (#5203): the card's title when the reply
/// has no name, the reply to come, and how far the worker is (#8706).
pub struct Measuring {
    fallback: String,
    rx: std::sync::mpsc::Receiver<Result<Value, String>>,
    /// The worker's progress; cancelling it stops the measuring at its next body.
    progress: Arc<Progress>,
}

impl Measuring {
    /// Bodies measured so far and how many there are (none before the worker counts them).
    pub fn bodies(&self) -> Option<(usize, usize)> {
        self.progress.work().filter(|(stage, _, _)| *stage == "measuring").map(|(_, done, total)| (done, total))
    }
}

impl Drop for Measuring {
    /// A card closed (or a design switched away from) stops its measuring, which on a large
    /// assembly would otherwise run on for minutes (#8706).
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// Show `reply` in the card, titled with its name (else `fallback`); a refusal goes to the
/// status line.
fn opened(app: &mut SolveApp, reply: Result<Value, String>, fallback: String) {
    match reply {
        Ok(v) => {
            let title = v["name"].as_str().map(str::to_string).unwrap_or(fallback);
            app.menu.props = Some((title, v));
        }
        Err(e) => app.set_status(e, true),
    }
}

/// Run `command` on `copy` on a worker, watched by `progress`; the reply comes on the returned
/// channel once `hold` returns (which only tests make wait).
#[cfg(not(target_arch = "wasm32"))]
fn measure_beside(
    mut copy: solidcraft_engine::Session,
    command: &str,
    params: &Value,
    progress: Arc<Progress>,
    hold: impl FnOnce() + Send + 'static,
) -> Option<std::sync::mpsc::Receiver<Result<Value, String>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (command, params) = (command.to_string(), params.clone());
    std::thread::Builder::new()
        .name("properties".into())
        .spawn(move || {
            let reply = solidcraft_engine::doc::progress::watch(progress, || copy.execute(&command, &params));
            hold();
            let _ = tx.send(reply.map_err(|e| e.to_string()));
        })
        .ok()?;
    Some(rx)
}

/// No threads on the web: measured here.
#[cfg(target_arch = "wasm32")]
fn measure_beside(
    _copy: solidcraft_engine::Session,
    _command: &str,
    _params: &Value,
    _progress: Arc<Progress>,
    _hold: impl FnOnce() + Send + 'static,
) -> Option<std::sync::mpsc::Receiver<Result<Value, String>>> {
    None
}

/// While a measurement runs beside the window: a card saying so (Close drops it), and the
/// Properties card in its place once the reply is in.
fn show_measuring(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.menu.props_measuring.take() else { return };
    match m.rx.try_recv() {
        Ok(reply) => return opened(app, reply, std::mem::take(&mut m.fallback)),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => return app.set_status(app.tr("ui.properties.measuring_failed").to_string(), true),
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    // How far it is, once the worker has counted the bodies (#8706).
    let bodies = m.bodies();
    let heading = app.tr("ui.properties").to_string();
    let text = match bodies {
        Some((done, total)) => {
            solidcraft_ui_text::format("ui.properties.measuring_bodies", &[("done", &done.to_string()), ("total", &total.to_string())])
        }
        None => app.tr("ui.properties.measuring").to_string(),
    };
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_props"), "settings", &heading).actions("", "Close", None).show(ctx, true, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui_kit::text(ui, &text, ui_kit::Text::Note("measuring"));
        });
        if let Some((done, total)) = bodies.filter(|(_, total)| *total > 0) {
            ui_kit::progress(ui, "properties_measuring", done as f32 / total as f32);
        }
    });
    if !(closed || action == ui_kit::Action::Cancel) {
        app.menu.props_measuring = Some(m);
    }
}

/// Inspect → Measure → Properties (#8095): the window of the one selected placement, else of the
/// active component (at the root, the whole design).
pub fn open_selected(app: &mut SolveApp) {
    let (command, params) = match app.session.selection.as_slice() {
        [solidcraft_engine::Sel::Occurrence { path }] if !path.is_empty() => {
            ("occurrence.properties", json!({"occurrence": path.last(), "path": path}))
        }
        _ => ("component.properties", json!({})),
    };
    open_object(app, command, &params, String::new());
}

/// A reply's 3 × 3 tensor, row by row.
fn tensor(v: &Value) -> Option<[[f64; 3]; 3]> {
    let row = |r: &Value| -> Option<[f64; 3]> { Some([r.get(0)?.as_f64()?, r.get(1)?.as_f64()?, r.get(2)?.as_f64()?]) };
    Some([row(v.get(0)?)?, row(v.get(1)?)?, row(v.get(2)?)?])
}

fn point(v: &Value) -> Option<Vec3> {
    Some(Vec3::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?))
}

/// `x` with the length decimals, or with more where those would leave fewer than three
/// significant digits; never a signed zero.
fn number_text(app: &SolveApp, x: f64) -> String {
    let decimals = i32::from(app.preferences.length_decimals.min(8));
    let lead = if x == 0.0 || !x.is_finite() { 0 } else { x.abs().log10().floor() as i32 };
    let digits = decimals.max(2 - lead).clamp(0, 12);
    let x = if x.abs() < 0.5 * 10f64.powi(-digits) { 0.0 } else { x };
    format!("{x:.*}", digits as usize)
}

/// The tensor the window shows (#8095): in g × the design's length unit squared, with the
/// products of inertia that are round-off against the largest entry as zero.
fn shown_tensor(app: &SolveApp, total: &Value) -> Option<[[f64; 3]; 3]> {
    let k = crate::prefs::unit_scale(&app.session.doc.units).0;
    let i = tensor(&total["inertia_g_mm2"])?.map(|r| r.map(|x| x * k * k));
    let largest = i.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    Some(i.map(|r| r.map(|x| if x.abs() <= 1e-9 * largest { 0.0 } else { x })))
}

/// What Copy matrix puts on the clipboard: the shown tensor's rows at full precision, the values
/// tab-separated, so it pastes as a 3 × 3 grid into a spreadsheet and splits easily in code.
fn tensor_text(t: &[[f64; 3]; 3]) -> String {
    t.iter().map(|r| r.map(|x| x.to_string()).join("\t")).collect::<Vec<_>>().join("\n")
}

/// What an inertia cell reads as: its entry's name from the row's label ("Ixx, Ixy, Ixz") and its
/// value, or the row's label and the cell's place when the label does not name each cell (the
/// principal moments).
fn cell_name(label: &str, cells: usize, i: usize, value: &str) -> String {
    let names: Vec<&str> = label.split(',').map(str::trim).collect();
    match names.get(i) {
        Some(name) if names.len() == cells => format!("{name}: {value}"),
        _ => format!("{label} {}: {value}", i + 1),
    }
}

/// The rows under Moment of inertia (#8095): label and cells, the tensor's three first.
fn inertia_rows(app: &SolveApp, total: &Value) -> Vec<(String, Vec<String>)> {
    let k = crate::prefs::unit_scale(&app.session.doc.units).0;
    let Some(i) = shown_tensor(app, total) else {
        return Vec::new();
    };
    let mut rows: Vec<(String, Vec<String>)> = ["x", "y", "z"]
        .iter()
        .zip(i)
        .map(|(axis, r)| (app.tr(&format!("ui.properties.inertia_{axis}")).to_string(), r.map(|x| number_text(app, x)).to_vec()))
        .collect();
    if let Some(moments) = total["principal_moments_g_mm2"].as_array() {
        let moments = moments.iter().map(|m| number_text(app, m.as_f64().unwrap_or(f64::NAN) * k * k)).collect();
        rows.push((app.tr("ui.properties.principal_moments").to_string(), moments));
    }
    for (n, axis) in total["principal_axes"].as_array().into_iter().flatten().enumerate() {
        let Some(e) = point(axis) else { continue };
        let label = solidcraft_ui_text::format("ui.properties.principal_axis", &[("number", &(n + 1).to_string())]);
        rows.push((label, [e.x, e.y, e.z].map(|x| format!("{:.4}", if x.abs() < 5e-5 { 0.0 } else { x })).to_vec()));
    }
    rows
}

/// The window's title: just Properties for the whole design (its name would only be the
/// file's), else with the name of the component, placement or bodies it measured.
fn window_title(title: &str, v: &Value) -> String {
    if v["component"].as_u64() == Some(0) {
        solidcraft_ui_text::tr("ui.properties").to_string()
    } else {
        solidcraft_ui_text::format("properties.title", &[("name", title)])
    }
}

/// The centre of mass the window shows, its principal axes when it has them, and how long to
/// draw them: a third of the bounding box's diagonal.
fn centre_and_axes(v: &Value) -> Option<(Vec3, Vec<Vec3>, f64)> {
    let total = &v["total"];
    let bodies = v["bodies"].as_array();
    let geometry = if total["center_of_mass"].is_array() { total } else { bodies.filter(|b| b.len() == 1)?.first()? };
    let centre = point(&geometry["center_of_mass"])?;
    let axes = total["principal_axes"].as_array().into_iter().flatten().filter_map(point).collect();
    let reach = point(&geometry["bbox"]["max"]).zip(point(&geometry["bbox"]["min"])).map(|(a, b)| (a - b).len() / 3.0);
    Some((centre, axes, reach.filter(|r| r.is_finite() && *r > 0.0).unwrap_or(10.0)))
}

/// While the Properties window is open: its centre of mass, with each principal axis standing
/// out of it in the X, Y and Z colours in turn and numbered as the window lists them (#8095).
pub fn overlay(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some((centre, axes, reach)) = app.menu.props.as_ref().and_then(|(_, v)| centre_and_axes(v)) else { return };
    let Some(at) = proj.to_screen(centre) else { return };
    let tk = solidcraft_theme::Tokens::view();
    for (n, (axis, colour)) in axes.iter().zip(tk.axes()).enumerate() {
        let Some(tip) = proj.to_screen(centre + *axis * reach) else { continue };
        painter.line_segment([at, tip], Stroke::new(2.0, colour));
        painter.text(tip, Align2::LEFT_BOTTOM, (n + 1).to_string(), FontId::proportional(tk.font_small), colour);
    }
    painter.circle(at, 6.0, tk.panel.gamma_multiply(0.8), Stroke::new(1.6, tk.accent));
    painter.line_segment([at - egui::vec2(10.0, 0.0), at + egui::vec2(10.0, 0.0)], Stroke::new(1.2, tk.accent));
    painter.line_segment([at - egui::vec2(0.0, 10.0), at + egui::vec2(0.0, 10.0)], Stroke::new(1.2, tk.accent));
}

/// Draw the Properties window of the body or component the menu asked about (mass, volume,
/// area, centre of mass, bounding box and counts; a component's first moments and moment of
/// inertia), when it is open. Close dismisses it.
pub fn show_object(app: &mut SolveApp, ctx: &egui::Context) {
    show_measuring(app, ctx);
    let Some((title, v)) = app.menu.props.clone() else { return };
    let number = |x: &Value, k: &str| x.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    let mut rows: Vec<(&str, String)> = Vec::new();
    let total = &v["total"];
    let bodies = v["bodies"].as_array().cloned().unwrap_or_default();
    let skipped = total["open_bodies_skipped"].as_u64().unwrap_or(0);
    if let [b] = bodies.as_slice()
        && b["closed"] == true
    {
        rows.push(("Material", b["material"].as_str().unwrap_or_default().to_string()));
        rows.push(("Density", format!("{:.3} g/cm³", number(b, "density_g_cm3"))));
    }
    if !bodies.is_empty() && skipped == bodies.len() as u64 {
        rows.push(("", solidcraft_ui_text::tr_label("Open surface — no volume").to_string()));
    } else {
        rows.push(("Mass", format!("{:.3} g", number(total, "mass_g"))));
        rows.push(("Volume", crate::prefs::show_mm(app, number(total, "volume_mm3"), 3)));
    }
    if skipped > 0 && skipped < bodies.len() as u64 {
        rows.push(("Open bodies (no volume)", skipped.to_string()));
    }
    rows.push(("Area", crate::prefs::show_mm(app, number(total, "area_mm2"), 2)));
    let geometry = if total.get("center_of_mass").is_some() { Some(total) } else { bodies.first().filter(|_| bodies.len() == 1) };
    if let Some(b) = geometry {
        let unit = crate::prefs::unit_scale(&app.session.doc.units).1;
        if let Some(c) = b["center_of_mass"].as_array() {
            let c: Vec<String> = c.iter().map(|x| crate::prefs::show_mm_bare(app, x.as_f64().unwrap_or(f64::NAN))).collect();
            rows.push(("Center of mass", format!("({}) {unit}", c.join(", "))));
        }
        let (mn, mx) = (&b["bbox"]["min"], &b["bbox"]["max"]);
        if let (Some(a), Some(z)) = (mn.as_array(), mx.as_array()) {
            let d: Vec<String> =
                a.iter().zip(z).map(|(a, z)| crate::prefs::show_mm_bare(app, z.as_f64().unwrap_or(0.0) - a.as_f64().unwrap_or(0.0))).collect();
            rows.push(("Bounding box", format!("{} {unit}", d.join(" × "))));
        }
    }
    rows.push(("Faces / edges / vertices", format!("{} / {} / {}", total["faces"], total["edges"], total["vertices"])));
    let (k, unit) = crate::prefs::unit_scale(&app.session.doc.units);
    let first = point(&total["first_moment_g_mm"]).filter(|_| number(total, "mass_g") > 0.0).map(|m| {
        let m: Vec<String> = [m.x, m.y, m.z].iter().map(|x| number_text(app, x * k)).collect();
        format!("({}) g·{unit}", m.join(", "))
    });
    let inertia = inertia_rows(app, total);
    let note = solidcraft_ui_text::format("ui.properties.inertia_note", &[("unit", unit)]);
    // A component's closed bodies have mass, but a mesh that would not close gave no tensor.
    let unavailable =
        inertia.is_empty() && total["inertia_g_mm2"].is_null() && number(total, "mass_g") > 0.0 && total.get("first_moment_g_mm").is_some();
    let (first_label, section, unavailable_text) = (
        app.tr("ui.properties.first_moments").to_string(),
        app.tr("ui.properties.inertia").to_string(),
        app.tr("ui.properties.inertia_unavailable").to_string(),
    );
    let copy = shown_tensor(app, total).map(|t| tensor_text(&t));
    let copy_label = app.tr("ui.properties.copy_matrix").to_string();
    let copy_hint = app.tr("ui.properties.copy_value").to_string();
    let mut copied = false;
    let heading = window_title(&title, &v);
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_props"), "settings", &heading).actions("", "Close", None).show(ctx, true, |ui| {
        // Each value copies itself as shown, unit and all, when clicked (#12218).
        for (label, value) in &rows {
            let label = solidcraft_ui_text::tr_label(label);
            let name = format!("{label}: {value}");
            ui_kit::row(ui, label, |ui| copied |= ui_kit::copy_cell(ui, value, &name, false, &copy_hint));
        }
        if let Some(first) = &first {
            let name = format!("{first_label}: {first}");
            ui_kit::row(ui, &first_label, |ui| copied |= ui_kit::copy_cell(ui, first, &name, false, &copy_hint));
        }
        if !inertia.is_empty() || unavailable {
            ui_kit::section(ui, "sc_props_inertia", &section, true, |ui| {
                if unavailable {
                    ui_kit::text(ui, &unavailable_text, ui_kit::Text::Note("inertia_unavailable"));
                    return;
                }
                ui_kit::text(ui, &note, ui_kit::Text::Note("inertia_note"));
                for (n, (label, cells)) in inertia.iter().enumerate() {
                    ui_kit::row(ui, label, |ui| {
                        ui.columns(cells.len(), |columns| {
                            for (i, (column, cell)) in columns.iter_mut().zip(cells).enumerate() {
                                let name = cell_name(label, cells.len(), i, cell);
                                copied |= ui_kit::copy_cell(column, cell, &name, true, &copy_hint);
                            }
                        })
                    });
                    // Under the tensor's last row: copy it whole.
                    if n == 2
                        && let Some(text) = &copy
                    {
                        ui_kit::row(ui, "", |ui| {
                            let button = ui_kit::button(ui, &copy_label, ui_kit::Button::Small);
                            crate::scenario::publish_handle("properties:copy_inertia", button.rect.center());
                            if button.clicked() {
                                ui.ctx().copy_text(text.clone());
                            }
                        });
                    }
                }
            });
        }
    });
    if copied {
        app.set_status(solidcraft_ui_text::tr("ui.properties.value_copied"), false);
    }
    if closed || action == ui_kit::Action::Cancel {
        close_object(app);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::geom::prelude::*;

    use crate::scenario::Harness;

    /// Original analytic fixture: a box in a movable component, with an exact top-face pick.
    fn review_fixture() -> Harness {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name":"ReviewPart", "grounded":false})).unwrap();
        h.app.run("solid.box", json!({"name":"ReviewBox", "body_name":"ReviewBlock", "length":20,"width":10,"height":5})).unwrap();
        h.app.run("component.activate", json!({"component":"root"})).unwrap();
        h.app.session.selection = vec![crate::dialogs::face_sel(&h.app.session, solidcore_record::Vec3::new(6.0, 4.0, 5.0)).unwrap()];
        h
    }

    fn review_with_note(h: &mut Harness) -> crate::review_notes::Note {
        let mut draft = crate::review_notes::new_draft(&h.app).unwrap();
        draft.note.text = "Increase clearance here.".into();
        crate::review_notes::save(&mut h.app, &draft, false).unwrap();
        crate::review_notes::Notebook::read(&h.app.session).unwrap().notes.remove(0)
    }

    #[test]
    fn review_anchor_follows_only_its_exact_occurrence_through_rigid_moves() {
        use crate::review_notes::{capture, resolve};
        use solidcore_record::Vec3;
        let mut h = review_fixture();
        let anchor = capture(&h.app.session, &h.app.session.selection[0]).unwrap();
        assert_eq!(anchor.local_point, Vec3::new(6.0, 4.0, 5.0));
        assert_eq!(anchor.original_point, anchor.local_point);
        assert_eq!(anchor.occurrence_path.len(), 1);
        assert!(matches!(&anchor.locator, crate::review_notes::Locator::Face { name, .. } if !name.is_empty()));
        assert_eq!(resolve(&h.app.session, &anchor), Some(anchor.original_point));
        h.app.run("occurrence.copy", json!({"component":"ReviewPart", "translate":[0,40,0]})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence":"ReviewPart:1", "translate":[30,0,0], "axis":[0,0,1], "angle":"90 deg"})).unwrap();
        let moved = resolve(&h.app.session, &anchor).unwrap();
        assert!(moved.dist(Vec3::new(26.0, 6.0, 5.0)) < 1e-8, "{moved:?}");
        assert_eq!(anchor.original_point, Vec3::new(6.0, 4.0, 5.0), "original evidence stays unchanged");
        // A saved external revision that removes the original placement still has a copy.
        let mut missing = (*h.app.session.doc).clone();
        missing.occurrences.retain(|o| !anchor.occurrence_path.contains(&o.id));
        let missing = solidcraft_engine::Session::new(missing);
        assert!(resolve(&missing, &anchor).is_none(), "must not jump to the other occurrence");
        let mut wrong_face = anchor.clone();
        if let crate::review_notes::Locator::Face { name, .. } = &mut wrong_face.locator {
            name.push_str("-different");
        }
        assert!(resolve(&h.app.session, &wrong_face).is_none());
        let mut wrong_feature = anchor.clone();
        wrong_feature.feature += 1;
        assert!(resolve(&h.app.session, &wrong_feature).is_none());
        let mut wrong_offset = anchor.clone();
        wrong_offset.definition_offset[3][0] += 5.0;
        assert!(resolve(&h.app.session, &wrong_offset).is_none(), "an internal body transfer is not a rigid occurrence move");
    }

    #[test]
    fn review_changed_geometry_is_stale_until_undone() {
        let mut h = review_fixture();
        let note = review_with_note(&mut h);
        h.app.run("timeline.edit", json!({"feature":"ReviewBox","set":{"height":"8"}})).unwrap();
        assert!(crate::review_notes::resolve(&h.app.session, &note.anchors[0]).is_none());
        assert_eq!(crate::review_notes::Notebook::read(&h.app.session).unwrap().notes[0], note);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(crate::review_notes::resolve(&h.app.session, &note.anchors[0]), Some(note.anchors[0].original_point));
    }

    #[test]
    fn review_edits_status_delete_undo_and_native_save_roundtrip() {
        use crate::review_notes::{Draft, KEY, Notebook, Status, save};
        let mut h = review_fixture();
        h.app.run("document.metadata.set", json!({"key":"org.example.other", "value":{"keep":true}})).unwrap();
        let evaluated_model = h.app.session.model.state();
        let before = h.app.session.undo.len();
        let note = review_with_note(&mut h);
        assert_eq!(h.app.session.undo.len(), before + 1);
        let mut draft = Draft { note: note.clone(), base: Some(note.clone()) };
        draft.note.text = "Clearance checked.".into();
        draft.note.status = Status::ReadyForReview;
        save(&mut h.app, &draft, false).unwrap();
        let ready = Notebook::read(&h.app.session).unwrap().notes[0].clone();
        assert_eq!(ready.status, Status::ReadyForReview);
        // An old editor cannot overwrite the last change.
        assert!(save(&mut h.app, &draft, false).is_err());
        draft = Draft { note: ready.clone(), base: Some(ready) };
        draft.note.status = Status::Resolved;
        save(&mut h.app, &draft, false).unwrap();
        let resolved = Notebook::read(&h.app.session).unwrap().notes[0].clone();
        draft = Draft { note: resolved.clone(), base: Some(resolved.clone()) };
        save(&mut h.app, &draft, true).unwrap();
        assert!(Notebook::read(&h.app.session).unwrap().notes.is_empty());
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(Notebook::read(&h.app.session).unwrap().notes, vec![resolved]);
        assert!(
            std::sync::Arc::ptr_eq(&evaluated_model, &h.app.session.model.state()),
            "review changes and undo retain the same evaluated definition model"
        );
        assert_eq!(h.app.session.doc.metadata.get("org.example.other"), Some(&json!({"keep":true})));
        let expected = h.app.session.doc.metadata.get(KEY).unwrap().clone();
        let path = std::env::temp_dir().join(format!("solidcraft-review-roundtrip-{}.sc", std::process::id()));
        h.app.run("file.save_as", json!({"path":path,"format":"container"})).unwrap();
        let mut reopened = solidcraft_engine::Session::default();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(reopened.doc.metadata.get(KEY), Some(&expected));
        let reopened_note = Notebook::read(&reopened).unwrap().notes.remove(0);
        assert_eq!(crate::review_notes::resolve(&reopened, &reopened_note.anchors[0]), Some(note.anchors[0].original_point));
        assert_eq!(reopened.execute("document.review", &json!({})).unwrap()["value"], expected);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn review_unknown_fields_survive_and_hostile_metadata_is_never_overwritten() {
        use crate::review_notes::{Draft, KEY, Notebook, save};
        let mut h = review_fixture();
        review_with_note(&mut h);
        let mut good = h.app.session.doc.metadata.get(KEY).unwrap().clone();
        good["future"] = json!({"preserve":[1,true,"yes"]});
        good["notes"][0]["author"] = json!("Reviewer");
        good["notes"][0]["anchors"][0]["future_anchor"] = json!({"extra":5});
        good["notes"][0]["anchors"][0]["locator"]["future_locator"] = json!({"extra":7});
        good["notes"][0]["camera"]["future_camera"] = json!(7);
        h.app.run("document.metadata.set", json!({"key":KEY,"value":good})).unwrap();
        let n = Notebook::read(&h.app.session).unwrap().notes.remove(0);
        let mut draft = Draft { note: n.clone(), base: Some(n) };
        draft.note.text = "Updated text".into();
        save(&mut h.app, &draft, false).unwrap();
        let preserved = h.app.session.doc.metadata.get(KEY).unwrap().clone();
        assert_eq!(preserved["future"], good["future"]);
        for key in ["author", "anchors", "camera"] {
            assert_eq!(preserved["notes"][0][key], good["notes"][0][key]);
        }
        let mut cases = vec![json!("broken"), json!({"version":2,"notes":[]}), json!({"version":1,"notes":[]})];
        for (field, value) in
            [("status", json!("unknown")), ("text", json!("é".repeat(2049))), ("id", json!(0)), ("camera", json!({})), ("anchors", json!({}))]
        {
            let mut bad = preserved.clone();
            bad["notes"][0][field] = value;
            cases.push(bad);
        }
        let mut duplicate = preserved.clone();
        duplicate["notes"] = json!([duplicate["notes"][0], duplicate["notes"][0]]);
        cases.push(duplicate);
        for anchors in [json!([]), json!(vec![preserved["notes"][0]["anchors"][0].clone(); 33])] {
            let mut bad = preserved.clone();
            bad["notes"][0]["anchors"] = anchors;
            cases.push(bad);
        }
        let mut unknown_kind = preserved.clone();
        unknown_kind["notes"][0]["anchors"][0]["locator"]["kind"] = json!("future_geometry");
        cases.push(unknown_kind);
        let mut future_version = preserved.clone();
        future_version["version"] = json!(3);
        cases.push(future_version);
        let mut large = preserved.clone();
        large["unknown"] = json!("x".repeat(65536));
        cases.push(large);
        for value in cases {
            // Simulates malformed data loaded from a file/another tool, which Metadata preserves.
            h.app.session.doc_mut().metadata.0.insert(KEY.into(), value.clone());
            let undo = h.app.session.undo.len();
            assert!(Notebook::read(&h.app.session).is_err(), "{value}");
            assert!(save(&mut h.app, &draft, false).is_err());
            assert!(save(&mut h.app, &draft, true).is_err());
            assert_eq!(h.app.session.doc.metadata.get(KEY), Some(&value));
            assert_eq!(h.app.session.undo.len(), undo);
        }
        let mut bounded = Notebook::default();
        let mut n = draft.note;
        n.id = 1;
        n.text = "é".repeat(2048);
        bounded.next_id = 2;
        bounded.notes.push(n);
        assert!(bounded.value().is_ok(), "exactly 4096 UTF-8 bytes accepted");
        bounded.notes[0].text.push('a');
        assert!(bounded.value().is_err());
        bounded.notes[0].text = "short".into();
        let n = bounded.notes[0].clone();
        for id in 2..=129 {
            let mut next = n.clone();
            next.id = id;
            bounded.notes.push(next);
        }
        bounded.next_id = 130;
        assert!(bounded.value().is_err(), "count bounded before encoding");
    }

    #[test]
    fn review_right_click_opens_focused_editor_at_exact_face_point() {
        use crate::context_menu::{Target, items};
        use solidcore_record::Vec3;
        for selected in [false, true] {
            let mut h = review_fixture();
            h.app.home.open = false;
            if !selected {
                h.app.session.selection.clear();
            }
            let selection = h.app.session.selection.clone();
            h.call("ui.view", json!({"view":"top"}));
            h.app.cam.target = Vec3::new(10.0, 5.0, 5.0);
            h.app.cam.distance = 45.0;
            h.settle();
            let world = h.app.session.world_state().unwrap();
            let model = h.app.session.model.state();
            let undo = h.app.session.undo.len();
            h.step(&json!({"click":{"world":[14,6,5]},"button":"right"})).unwrap();
            let menu = items(&h.app, &Target::Viewport);
            assert!(menu.iter().any(|item| item.id == "ui.addReviewNote" && item.enabled));
            assert!(menu.iter().any(|item| item.id == "solid.extrude"), "normal face commands remain available");
            if selected {
                assert_eq!(h.app.session.selection, selection, "the normal face menu retains its existing selection");
            }
            h.step(&json!({"click":{"handle":"menu:ui.addReviewNote"}})).unwrap();
            assert!(h.app.viewport.context_menu.is_none());
            let draft = h.app.review_notes.as_ref().unwrap().draft.as_ref().expect("menu opens the editor directly");
            assert!(draft.note.anchors[0].original_point.dist(Vec3::new(14.0, 6.0, 5.0)) < 1e-5, "{:?}", draft.note.anchors[0]);
            assert_eq!(draft.note.anchors[0].occurrence_path.len(), 1);
            assert!(draft.base.is_none());
            h.step(&json!({"text":"Directly typed review note"})).unwrap();
            assert_eq!(h.app.review_notes.as_ref().unwrap().draft.as_ref().unwrap().note.text, "Directly typed review note");
            assert!(std::sync::Arc::ptr_eq(&world, &h.app.session.world_state().unwrap()), "opening a note does not move or rebuild geometry");
            assert!(std::sync::Arc::ptr_eq(&model, &h.app.session.model.state()));
            assert_eq!(h.app.session.undo.len(), undo, "a draft makes no document edit");
        }
    }

    #[test]
    fn review_right_click_preserves_occurrence_selection_and_anchors_the_clicked_copy() {
        use crate::context_menu::{Target, close, items};
        use solidcore_record::Vec3;
        let mut h = review_fixture();
        h.app.home.open = false;
        h.app.run("occurrence.copy", json!({"component":"ReviewPart", "translate":[0,40,0]})).unwrap();
        h.app.ui.pick_components = true;
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.target = Vec3::new(10.0, 45.0, 5.0);
        h.app.cam.distance = 45.0;
        h.settle();
        h.step(&json!({"click":{"world":[6,44,5]}})).unwrap();
        let selection = h.app.session.selection.clone();
        let [solidcraft_engine::Sel::Occurrence { path }] = selection.as_slice() else { panic!("{selection:?}") };
        let path = path.clone();
        let doc = h.app.session.doc.clone();
        h.step(&json!({"click":{"world":[14,46,5]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, selection, "right-click retains the selected occurrence");
        let item = items(&h.app, &Target::Viewport).into_iter().find(|item| item.id == "ui.addReviewNote").unwrap();
        h.step(&json!({"click":{"handle":"menu:ui.addReviewNote"}})).unwrap();
        let draft = h.app.review_notes.as_ref().unwrap().draft.as_ref().unwrap();
        assert_eq!(draft.note.anchors[0].occurrence_path, path);
        assert!(draft.note.anchors[0].original_point.dist(Vec3::new(14.0, 46.0, 5.0)) < 1e-5);
        assert!(draft.note.anchors[0].local_point.dist(Vec3::new(14.0, 6.0, 5.0)) < 1e-5);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        h.step(&json!({"text":"Keep this draft"})).unwrap();
        // Replaying a menu action must not replace an unsaved draft.
        crate::context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
        assert_eq!(h.app.review_notes.as_ref().unwrap().draft.as_ref().unwrap().note.text, "Keep this draft");
        h.step(&json!({"click":{"handle":"review:cancel"}})).unwrap();
        close(&mut h.app);
        h.app.viewport.hover = None;
        crate::context_menu::open(&mut h.app, egui::Pos2::ZERO, Default::default());
        assert!(items(&h.app, &Target::Viewport).iter().all(|item| item.id != "ui.addReviewNote"), "empty space cannot reuse a previous face hit");
    }

    #[test]
    fn review_right_click_edge_and_vertex_keep_exact_hit_in_component_mode() {
        use crate::review_notes::{Locator, Notebook};
        use solidcore_record::Vec3;
        use solidcraft_engine::doc;
        for (kind, local_point, rotated) in [
            ("edge", Vec3::new(6.0, 0.0, 5.0), false),
            ("vertex", Vec3::new(20.0, 10.0, 5.0), false),
            ("edge", Vec3::new(6.0, 0.0, 5.0), true),
            ("vertex", Vec3::new(20.0, 10.0, 5.0), true),
        ] {
            let mut h = review_fixture();
            h.app.home.open = false;
            h.app.ui.pick_components = true;
            let transform = if rotated {
                h.app.run("occurrence.copy", json!({"component":"ReviewPart", "translate":[0,0,0]})).unwrap();
                h.app.run("occurrence.move", json!({"occurrence":"ReviewPart:2", "translate":[50,40,0],"axis":[0,0,1],"angle":"90 deg"})).unwrap();
                doc::rigid(Vec3::new(50.0, 40.0, 0.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2)
            } else {
                doc::IDENTITY
            };
            let point = doc::apply_point(&transform, local_point);
            let face_point = doc::apply_point(&transform, Vec3::new(6.0, 4.0, 5.0));
            // An oblique view separates the rotated box's top/bottom outline
            // edges, which occupy the same pixel in an exact top view.
            h.call("ui.view", json!({"view":if rotated { "iso" } else { "top" }}));
            h.app.cam.target = doc::apply_point(&transform, Vec3::new(10.0, 5.0, 5.0));
            h.app.cam.distance = 45.0;
            h.settle();
            h.step(&json!({"click":{"world":[face_point.x,face_point.y,face_point.z]}})).unwrap();
            let selection = h.app.session.selection.clone();
            assert!(matches!(selection.as_slice(), [solidcraft_engine::Sel::Occurrence { .. }]));
            let model = h.app.session.model.state();
            h.step(&json!({"click":{"world":[point.x,point.y,point.z]},"button":"right"})).unwrap();
            assert_eq!(h.app.session.selection, selection);
            h.step(&json!({"click":{"handle":"menu:ui.addReviewNote"}})).unwrap();
            let draft = h.app.review_notes.as_ref().unwrap().draft.as_ref().unwrap();
            let anchor = &draft.note.anchors[0];
            assert!(anchor.original_point.dist(point) < 1e-5, "{kind}: {:?}", anchor.original_point);
            assert!(anchor.local_point.dist(local_point) < 1e-5);
            let [solidcraft_engine::Sel::Occurrence { path }] = selection.as_slice() else { panic!("occurrence selection") };
            assert_eq!(&anchor.occurrence_path, path);
            assert!(matches!((&anchor.locator, kind), (Locator::Edge { .. }, "edge") | (Locator::Vertex { .. }, "vertex")));
            if kind == "edge" {
                assert!(anchor.local_point.dist(Vec3::new(10.0, 0.0, 5.0)) > 3.9, "edge notes must not jump to the midpoint");
            }
            h.step(&json!({"text":"Exact geometry target"})).unwrap();
            h.step(&json!({"click":{"handle":"review:save"}})).unwrap();
            let note = Notebook::read(&h.app.session).unwrap().notes.remove(0);
            assert_eq!(note.text, "Exact geometry target");
            assert!(std::sync::Arc::ptr_eq(&model, &h.app.session.model.state()));
            h.app.run("edit.undo", json!({})).unwrap();
            assert!(Notebook::read(&h.app.session).unwrap().notes.is_empty());
        }
    }

    #[test]
    fn review_mixed_multi_selection_roundtrips_with_exact_primary_and_partial_stale_targets() {
        use crate::review_notes::{Locator, Notebook, new_draft, resolve};
        use solidcore_record::Vec3;
        use solidcraft_engine::Sel;
        let mut h = review_fixture();
        h.app.home.open = false;
        let source = h.app.session.model.state();
        let mesh = source.bodies[0].mesh();
        let edge_index = mesh
            .edges
            .iter()
            .position(|edge| {
                edge.first().zip(edge.last()).is_some_and(|(a, b)| {
                    (a.dist(Vec3::new(0.0, 0.0, 5.0)) < 1e-9 && b.dist(Vec3::new(20.0, 0.0, 5.0)) < 1e-9)
                        || (b.dist(Vec3::new(0.0, 0.0, 5.0)) < 1e-9 && a.dist(Vec3::new(20.0, 0.0, 5.0)) < 1e-9)
                })
            })
            .unwrap();
        h.app.run("occurrence.copy", json!({"component":"ReviewPart", "translate":[0,0,0]})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence":"ReviewPart:2", "translate":[50,40,0],"axis":[0,0,1],"angle":"90 deg"})).unwrap();
        let face = crate::dialogs::face_sel(&h.app.session, Vec3::new(6.0, 4.0, 5.0)).unwrap();
        let Sel::Face { body, .. } = &face else { panic!("face fixture") };
        let edge = Sel::Edge { body: body.clone(), index: edge_index, point: Vec3::new(6.0, 0.0, 5.0) };
        let copy_body = h.app.session.world_state().unwrap().bodies.iter().find(|b| b.name != *body).unwrap().name.clone();
        let vertex = Sel::Vertex { body: copy_body, point: Vec3::new(40.0, 60.0, 5.0) };
        let selection = vec![face, edge, vertex];
        h.app.session.selection = selection.clone();
        assert_eq!(new_draft(&h.app).unwrap().note.anchors.len(), 3, "the primary Add button uses the whole geometry selection");
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.target = Vec3::new(25.0, 30.0, 5.0);
        h.app.cam.distance = 100.0;
        h.settle();
        h.step(&json!({"click":{"world":[14,6,5]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, selection);
        h.step(&json!({"click":{"handle":"menu:ui.addReviewNote"}})).unwrap();
        let draft = h.app.review_notes.as_ref().unwrap().draft.as_ref().unwrap();
        assert_eq!(draft.note.anchors.len(), 3);
        assert!(matches!(&draft.note.anchors[0].locator, Locator::Face { .. }));
        assert!(draft.note.anchors[0].original_point.dist(Vec3::new(14.0, 6.0, 5.0)) < 1e-5);
        assert!(matches!(&draft.note.anchors[1].locator, Locator::Edge { .. }));
        assert_eq!(draft.note.anchors[1].original_point, Vec3::new(6.0, 0.0, 5.0));
        assert!(matches!(&draft.note.anchors[2].locator, Locator::Vertex { source_point, .. } if source_point.dist(Vec3::new(20.0,10.0,5.0)) < 1e-9));
        assert_ne!(draft.note.anchors[0].occurrence_path, draft.note.anchors[2].occurrence_path);
        h.step(&json!({"text":"One note about all three targets"})).unwrap();
        h.step(&json!({"click":{"handle":"review:save"}})).unwrap();
        let note = Notebook::read(&h.app.session).unwrap().notes.remove(0);
        for index in 0..3 {
            assert!(crate::scenario::rect_of(&format!("review:pin:1:{index}")).is_some(), "every target has the same numbered note marker");
        }
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(Notebook::read(&h.app.session).unwrap().notes.is_empty());
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(Notebook::read(&h.app.session).unwrap().notes, vec![note.clone()]);
        let path = std::env::temp_dir().join(format!("solidcraft-review-multi-{}.sc", std::process::id()));
        h.app.run("file.save_as", json!({"path":path,"format":"container"})).unwrap();
        let mut reopened = solidcraft_engine::Session::default();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(Notebook::read(&reopened).unwrap().notes, vec![note.clone()]);
        for anchor in &note.anchors {
            assert!(resolve(&reopened, anchor).unwrap().dist(anchor.original_point) < 1e-8);
        }
        let _ = std::fs::remove_file(path);
        h.app.run("occurrence.move", json!({"occurrence":"ReviewPart:2", "translate":[20,0,0]})).unwrap();
        assert!(resolve(&h.app.session, &note.anchors[2]).unwrap().dist(Vec3::new(60.0, 60.0, 5.0)) < 1e-8);
        assert!(resolve(&h.app.session, &note.anchors[0]).unwrap().dist(note.anchors[0].original_point) < 1e-8);
        h.app.run("timeline.edit", json!({"feature":"ReviewBox","set":{"height":"8"}})).unwrap();
        assert!(note.anchors.iter().all(|a| resolve(&h.app.session, a).is_none()));
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(note.anchors.iter().all(|a| resolve(&h.app.session, a).is_some()));
        let mut missing = (*h.app.session.doc).clone();
        missing.occurrences.retain(|o| !note.anchors[2].occurrence_path.contains(&o.id));
        h.app = crate::SolveApp::new(solidcraft_engine::Session::new(missing), Default::default());
        h.app.home.open = false;
        h.step(&json!({"start":"document.review"})).unwrap();
        assert!(resolve(&h.app.session, &note.anchors[0]).is_some());
        assert!(resolve(&h.app.session, &note.anchors[1]).is_some());
        assert!(resolve(&h.app.session, &note.anchors[2]).is_none());
        assert_eq!(Notebook::read(&h.app.session).unwrap().notes, vec![note]);
        assert!(h.widget("Stale targets: 1 of 3. Their pins are hidden; remaining targets stay attached. The complete note is retained.").is_some());
    }

    #[test]
    fn review_vertex_locator_accounts_for_definition_offset_and_rejects_unrelated_points() {
        use crate::review_notes::{Locator, capture, resolve};
        use solidcore_record::Vec3;
        use solidcraft_engine::{Sel, doc};
        let mut h = review_fixture();
        let offset = doc::rigid(Vec3::new(3.0, 7.0, 2.0), Vec3::ZERO, Vec3::Z, 0.0);
        h.app.session.doc_mut().body_offsets.insert("ReviewBlock".into(), offset);
        let body = h.app.session.world_state().unwrap().bodies[0].name.clone();
        let point = Vec3::new(23.0, 17.0, 7.0);
        let anchor = capture(&h.app.session, &Sel::Vertex { body: body.clone(), point }).unwrap();
        assert!(matches!(&anchor.locator, Locator::Vertex { source_point, .. } if *source_point == Vec3::new(20.0,10.0,5.0)));
        assert_eq!(anchor.local_point, point);
        assert_eq!(resolve(&h.app.session, &anchor), Some(point));
        assert!(capture(&h.app.session, &Sel::Vertex { body, point: point + Vec3::new(0.01, 0.0, 0.0) }).is_err(), "no nearest-vertex reattachment");
        let mut wrong = anchor.clone();
        if let Locator::Vertex { source_point, .. } = &mut wrong.locator {
            source_point.x += 1.0;
        }
        assert!(resolve(&h.app.session, &wrong).is_none());
        let mut wrong = anchor;
        if let Locator::Vertex { topology, .. } = &mut wrong.locator {
            *topology = "0".repeat(64);
        }
        assert!(resolve(&h.app.session, &wrong).is_none());
    }

    #[test]
    fn review_five_legacy_faces_upgrade_only_on_save_and_keep_unknown_fields_and_undo() {
        use crate::review_notes::{Draft, KEY, Locator, Notebook, resolve, save};
        let mut h = review_fixture();
        let seed = review_with_note(&mut h);
        let mut notes = Vec::new();
        for id in 1..=5 {
            let mut note = serde_json::to_value(&seed).unwrap();
            note["id"] = json!(id);
            note["text"] = json!(format!("Legacy review note {id}"));
            note["author"] = json!({"keep":id});
            let object = note.as_object_mut().unwrap();
            let mut anchor = object.remove("anchors").unwrap().as_array().unwrap()[0].clone();
            let fields = anchor.as_object_mut().unwrap();
            let locator = fields.remove("locator").unwrap();
            fields.insert("face_index".into(), locator["index"].clone());
            fields.insert("face_name".into(), locator["name"].clone());
            fields.insert("future_anchor".into(), json!({"preserve":true}));
            object.insert("anchor".into(), anchor);
            notes.push(note);
        }
        let legacy = json!({"version":1,"next_id":6,"notes":notes,"future_book":{"preserve":true}});
        h.app.run("document.metadata.set", json!({"key":KEY,"value":legacy})).unwrap();
        let migrated = Notebook::read(&h.app.session).unwrap();
        assert_eq!(h.app.session.doc.metadata.get(KEY), Some(&legacy), "reading never rewrites the saved metadata");
        assert_eq!((migrated.version, migrated.notes.len()), (2, 5));
        for (index, note) in migrated.notes.iter().enumerate() {
            assert_eq!(note.text, format!("Legacy review note {}", index + 1));
            assert_eq!(note.anchors.len(), 1);
            assert!(matches!(note.anchors[0].locator, Locator::Face { .. }));
            assert_eq!(resolve(&h.app.session, &note.anchors[0]), Some(seed.anchors[0].original_point));
            assert_eq!(note.extra["author"], json!({"keep":index + 1}));
            assert_eq!(note.anchors[0].extra["future_anchor"], json!({"preserve":true}));
        }
        let base = migrated.notes[0].clone();
        let draft = Draft { note: base.clone(), base: Some(base) };
        save(&mut h.app, &draft, false).unwrap();
        assert_eq!(h.app.session.doc.metadata.get(KEY).unwrap()["version"], 2, "the older prototype refuses v2 instead of treating edges as faces");
        assert_eq!(Notebook::read(&h.app.session).unwrap(), migrated);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.metadata.get(KEY), Some(&legacy));
        h.app.run("edit.redo", json!({})).unwrap();
        let path = std::env::temp_dir().join(format!("solidcraft-review-legacy-five-{}.sc", std::process::id()));
        h.app.run("file.save_as", json!({"path":path,"format":"container"})).unwrap();
        let mut reopened = solidcraft_engine::Session::default();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(Notebook::read(&reopened).unwrap(), migrated);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn review_window_picks_faces_and_saves_edits_without_a_browser() {
        use crate::review_notes::{Notebook, Status};
        let mut h = review_fixture();
        // Even component-selection mode yields a precise face while reviewing.
        h.app.ui.pick_components = true;
        h.step(&json!({"start":"document.review"})).unwrap();
        let sel = h.app.session.selection[0].clone();
        let solidcraft_engine::Sel::Face { body, index, point } = sel else { panic!("face fixture") };
        let hit = crate::viewport::Hit::Face { body, index, point };
        assert!(matches!(crate::viewport::candidate(&h.app, &[hit]).unwrap().1, solidcraft_engine::Sel::Face { .. }));
        h.step(&json!({"click":{"handle":"review:add"}})).unwrap();
        let d = h.app.review_notes.as_mut().unwrap().draft.as_mut().unwrap();
        d.note.text = "Check this exact face.".into();
        let saved_view = d.note.camera.clone();
        h.frames(2);
        h.step(&json!({"click":{"handle":"review:save"}})).unwrap();
        assert_eq!(Notebook::read(&h.app.session).unwrap().notes.len(), 1);
        h.step(&json!({"click":{"handle":"review:pin:1"}})).unwrap();
        assert!(h.app.review_notes.as_ref().unwrap().draft.is_some(), "the numbered 3D pin opens its note");
        h.step(&json!({"click":{"handle":"review:cancel"}})).unwrap();
        h.app.preferences.look_at_anim = false;
        h.app.cam.yaw += 0.5;
        h.step(&json!({"click":{"handle":"review:note:1"}})).unwrap();
        assert_eq!(serde_json::to_value(h.app.cam).unwrap(), saved_view);
        let d = h.app.review_notes.as_mut().unwrap().draft.as_mut().unwrap();
        d.note.text = "Resolved in review.".into();
        d.note.status = Status::Resolved;
        h.frames(2);
        h.step(&json!({"click":{"handle":"review:save"}})).unwrap();
        assert_eq!(Notebook::read(&h.app.session).unwrap().notes[0].status, Status::Resolved);
        h.step(&json!({"click":{"handle":"review:note:1"}})).unwrap();
        h.step(&json!({"click":{"handle":"review:delete"}})).unwrap();
        assert!(Notebook::read(&h.app.session).unwrap().notes.is_empty());
        h.step(&json!({"click":{"handle":"review:undo"}})).unwrap();
        assert_eq!(Notebook::read(&h.app.session).unwrap().notes.len(), 1);
    }

    #[test]
    fn open_surface_properties_show_area_without_volume_rows() {
        let mut h = Harness::new();
        for (command, params) in [
            ("sketch.create", json!({"plane": "XY", "name": "Profile"})),
            ("sketch.line", json!({"points": [[30, 0], [30, 40]], "ids": ["side"]})),
            ("sketch.finish", json!({})),
            ("surface.revolve", json!({"sketch": "Profile", "curves": ["side"], "axis": "y", "angle": "270 deg", "body_name": "OpenTube"})),
        ] {
            h.app.run(command, params).unwrap();
        }
        let measured = h.app.run("inspect.measure", json!({"bodies": ["OpenTube"]})).unwrap();
        h.app.menu.props = Some(("OpenTube".into(), measured));
        h.frames(2);
        assert!(h.widget("Open surface — no volume").is_some());
        assert!(h.widget("Area").is_some());
        for label in ["Volume", "Density", "Mass", "Center of mass", "First moments", "Moment of inertia"] {
            assert!(h.widget(label).is_none(), "{label} must not be shown for an open surface");
        }
        assert!(h.widget("Open bodies (no volume)").is_none());
        h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box"})).unwrap();
        let both = h.app.run("inspect.measure", json!({})).unwrap();
        h.app.menu.props = Some(("2 bodies".into(), both));
        h.frames(2);
        for label in ["Volume", "Mass", "Open bodies (no volume)"] {
            assert!(h.widget(label).is_some(), "{label} is shown for a solid with an open surface");
        }
    }

    #[test]
    fn the_window_edits_properties_in_one_undo_step_and_keeps_other_tools_data() {
        let mut h = Harness::new();
        h.app.run("document.metadata.set", json!({"values": {"author": "Jane", "com.acme.tool": {"id": 7}}})).unwrap();
        // A well-known key another tool saved as a number.
        h.app.session.doc_mut().metadata.0.insert("revision".into(), json!(3));
        let undo = h.app.session.undo.len();
        h.step(&json!({"start": "document.metadata.set"})).unwrap();
        assert!(h.app.design_properties.is_some(), "ui.start opens the window, it doesn't run the command");
        assert!(h.widget("Jane").is_some(), "the author's field");
        assert!(h.widget("Other tools' data (1)").is_some());
        let d = h.app.design_properties.as_mut().unwrap();
        assert_eq!(d.opened[2], "", "a number is not a revision");
        d.fields[0] = "ACME enclosures".into();
        d.fields[3] = String::new();
        h.frames(2);
        h.step(&json!({"widget": "OK"})).unwrap();
        assert!(h.app.design_properties.is_none());
        let m = &h.app.session.doc.metadata;
        assert_eq!(m.project(), Some("ACME enclosures"));
        assert_eq!(m.author(), None, "an emptied field removes the key");
        assert_eq!(m.get("revision"), Some(&json!(3)), "an untouched field keeps what another tool saved");
        assert_eq!(m.get("com.acme.tool"), Some(&json!({"id": 7})));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");

        // Cancel changes nothing; a refused value keeps the window open with the reason.
        h.step(&json!({"start": "document.metadata"})).unwrap();
        h.app.design_properties.as_mut().unwrap().fields[0] = "Other".into();
        h.frames(2);
        h.step(&json!({"widget": "Cancel"})).unwrap();
        assert!(h.app.design_properties.is_none());
        assert_eq!(h.app.session.doc.metadata.project(), Some("ACME enclosures"));
        h.step(&json!({"start": "document.metadata.set"})).unwrap();
        h.app.design_properties.as_mut().unwrap().fields[1] = "bad\u{7}".into();
        h.frames(2);
        h.step(&json!({"widget": "OK"})).unwrap();
        let d = h.app.design_properties.as_ref().expect("still open");
        assert!(d.error.as_deref().is_some_and(|e| e.contains("control character")), "{:?}", d.error);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        // Correcting the field takes the refusal away before OK is clicked again.
        h.app.design_properties.as_mut().unwrap().fields[1] = "PN-1".into();
        h.frames(2);
        let d = h.app.design_properties.as_ref().expect("still open");
        assert_eq!(d.error, None, "the message was about the control character, which is gone");
        // The message is about the fields OK ran with, as a dialog's is about its inputs (#641):
        // a change to another field takes it away too, and the next OK refuses again.
        h.app.design_properties.as_mut().unwrap().fields[1] = "bad\u{7}".into();
        h.frames(2);
        h.step(&json!({"widget": "OK"})).unwrap();
        let refused = h.app.design_properties.as_ref().expect("still open").error.clone();
        assert!(refused.as_deref().is_some_and(|e| e.contains("control character")), "{refused:?}");
        h.app.design_properties.as_mut().unwrap().fields[0] = "ACME".into();
        h.frames(2);
        let d = h.app.design_properties.as_ref().expect("still open");
        assert_eq!(d.error, None, "the fields differ from the refused ones");
        h.step(&json!({"widget": "OK"})).unwrap();
        let d = h.app.design_properties.as_ref().expect("still open: the part number is still refused");
        assert_eq!(d.error, refused, "the same reason again");
        assert_eq!(h.app.session.undo.len(), undo + 1, "nothing applied");
    }

    #[test]
    fn each_field_is_labelled_from_the_table_of_keys() {
        crate::i18n::install();
        use solidcraft_engine::doc::metadata::{KEYS, Owner, keys};
        let english = solidcraft_ui_text::Locale::for_language("en");
        for k in KEYS {
            assert_ne!(english.text(k.label), k.label, "`{}`'s label is in the English catalog", k.name);
        }
        let mut h = Harness::new();
        h.app.preferences.language = "en".into();
        // A key a newer SolidCraft might read: kept as another tool's data, never labelled as one of ours.
        h.app.session.doc_mut().metadata.0.insert("future_key".into(), json!("kept"));
        h.step(&json!({"start": "document.metadata"})).unwrap();
        h.frames(2);
        let labels: Vec<&str> = keys(Owner::Design).map(|k| english.text(k.label)).collect();
        assert_eq!(labels, ["Project", "Part number", "Revision", "Author", "Organization", "Description", "License"]);
        for label in &labels {
            assert!(h.widget(label).is_some(), "{label}");
        }
        let d = h.app.design_properties.as_ref().unwrap();
        assert_eq!((d.opened.len(), d.fields.len()), (labels.len(), labels.len()), "a field for each key, none for others");
        assert!(h.widget("Other tools' data (1)").is_some(), "the unknown key is listed with other tools' data");
    }

    /// #8095: Inspect → Measure → Properties opens the window (it used to run the command and
    /// show nothing), with a component's first moments and moment of inertia; the view marks
    /// its centre of mass with three principal axes a third of the box's diagonal long.
    #[test]
    fn the_measure_panels_properties_button_opens_the_window_with_the_moment_of_inertia() {
        use solidcore_record::Vec3;
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"})).unwrap();
        h.step(&json!({"start": "component.properties"})).unwrap();
        h.frames(2);
        let (_, v) = h.app.menu.props.clone().expect("the button opens the window");
        assert!(v["total"]["inertia_g_mm2"].is_array(), "{v}");
        for label in ["First moments", "Moment of inertia", "Ixx, Ixy, Ixz", "Izx, Izy, Izz", "Principal moments", "Principal axis 3"] {
            assert!(h.widget(label).is_some(), "{label}");
        }
        let (centre, axes, reach) = super::centre_and_axes(&v).unwrap();
        assert!((centre - Vec3::new(5.0, 10.0, 15.0)).len() < 1e-9, "{centre:?}");
        assert_eq!(axes, [Vec3::Z, Vec3::Y, Vec3::X * -1.0]);
        assert!((reach - 1400f64.sqrt() / 3.0).abs() < 1e-9, "{reach}");
        assert_eq!(super::window_title("Untitled", &v), "Properties", "the whole design's card has no file name in its title");

        // Copy matrix: the shown tensor, three tab-separated rows at full precision.
        // The card settles its size over a few frames: click where the button ends up.
        h.frames(3);
        let [x, y] = h.widget("Copy matrix").expect("the Copy matrix button");
        let (request, _reply) = crate::control::ControlRequest::new("ui.click", json!({"x": x, "y": y}));
        crate::control::handle(&mut h.app, &h.ctx, &request);
        let mut copied = Vec::new();
        for _ in 0..4 {
            h.frame();
            copied.extend(h.output.commands.iter().filter_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            }));
        }
        let text = copied.last().expect("Copy matrix puts the tensor on the clipboard");
        let rows: Vec<Vec<f64>> = text.lines().map(|l| l.split('\t').map(|x| x.parse().unwrap()).collect()).collect();
        let want = super::shown_tensor(&h.app, &v["total"]).unwrap();
        assert_eq!(rows, want.map(|r| r.to_vec()).to_vec(), "{text}");
        let m = v["total"]["mass_g"].as_f64().unwrap();
        assert!((rows[0][0] - m * (400.0 + 900.0) / 12.0).abs() < 1e-6 * rows[0][0] && rows[0][1] == 0.0, "{text}");

        // #12218: a click on a value copies it as shown, and the status bar says so.
        let mass = format!("{m:.3} g");
        // It reads as what it is and its value.
        let [x, y] = h.widget(&format!("Mass: {mass}")).expect("the mass cell");
        let (request, _reply) = crate::control::ControlRequest::new("ui.click", json!({"x": x, "y": y}));
        crate::control::handle(&mut h.app, &h.ctx, &request);
        let mut copied = Vec::new();
        for _ in 0..4 {
            h.frame();
            copied.extend(h.output.commands.iter().filter_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            }));
        }
        assert_eq!(copied.last(), Some(&mass), "the mass cell copies its text");
        assert_eq!(h.app.status.as_ref().map(|(text, ..)| text.as_str()), Some("Value copied"));
        h.step(&json!({"widget": "Close"})).unwrap();
        assert!(h.app.menu.props.is_none());

        // One selected placement: its own properties, by its path.
        let c = h.app.run("component.create", json!({"name": "Pin", "grounded": false})).unwrap();
        h.app.run("solid.cylinder", json!({"radius": 4, "height": 10, "base": [50, 0, 0]})).unwrap();
        let occurrence = c["occurrence"].as_u64().unwrap();
        h.app.session.selection = vec![solidcraft_engine::Sel::Occurrence { path: vec![occurrence] }];
        h.step(&json!({"start": "component.properties"})).unwrap();
        let (title, v) = h.app.menu.props.clone().expect("the window opens on the placement");
        assert_eq!((title.as_str(), v["occurrence"].as_u64()), ("Pin:1", Some(occurrence)), "{v}");
        assert_eq!(super::window_title(&title, &v), "Properties: Pin:1", "a placement's card keeps its name");
        let (_, axes, _) = super::centre_and_axes(&v).unwrap();
        for (axis, want) in axes.iter().zip([Vec3::Z, Vec3::X, Vec3::Y]) {
            assert!((*axis - want).len() < 1e-9, "a round pin's axes are the world axes: {axes:?}");
        }
    }

    /// #5203: Properties on a large assembly took seconds on the window thread, which froze it.
    /// Measured on a copy beside the window (here never waited for), the card says it is
    /// measuring and shows the reply once it is in; Close drops a measurement still running.
    #[test]
    fn properties_measure_beside_the_window_and_show_when_done() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"})).unwrap();
        let here = h.app.session.execute("component.properties", &json!({})).unwrap();
        h.app.apply_off_thread = Some(std::time::Duration::ZERO);
        // The worker holds its reply until let go, so a window thread held up under load cannot
        // find it already in.
        let (finish, gate) = std::sync::mpsc::channel();
        h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
        super::open_object(&mut h.app, "component.properties", &json!({}), String::new());
        assert!(h.app.menu.props.is_none(), "nothing is waited for on the window thread");
        let m = h.app.menu.props_measuring.take().expect("measuring beside the window");
        finish.send(()).unwrap();
        assert_eq!(m.rx.recv().unwrap().unwrap(), here, "the copy measures what the design here does");

        let (tx, rx) = std::sync::mpsc::channel();
        h.app.menu.props_measuring = Some(super::Measuring { fallback: String::new(), rx, progress: Default::default() });
        h.frames(2);
        assert!(h.widget("Measuring…").is_some() && h.app.menu.props.is_none());
        tx.send(Ok(here.clone())).unwrap();
        h.frames(2);
        assert_eq!(h.app.menu.props.as_ref().map(|(_, v)| v), Some(&here));
        assert!(h.widget("Moment of inertia").is_some());

        // #8706: while it measures, the card says how many bodies are done, and Close stops the
        // worker rather than leaving it to measure on for minutes.
        let (_tx, rx) = std::sync::mpsc::channel::<Result<serde_json::Value, String>>();
        super::close_object(&mut h.app);
        let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        h.app.menu.props_measuring = Some(super::Measuring { fallback: String::new(), rx, progress: progress.clone() });
        solidcraft_engine::doc::progress::watch(progress.clone(), || {
            if let Some(w) = solidcraft_engine::kernel::watch::current() {
                w.progress("measuring", 3, 10);
            }
        });
        h.frames(2);
        assert!(h.widget("Measuring… 3 of 10 bodies").is_some());
        h.step(&json!({"widget": "Close"})).unwrap();
        assert!(h.app.menu.props_measuring.is_none() && h.app.menu.props.is_none(), "Close drops the measurement");
        assert!(progress.is_cancelled(), "Close stops the worker");
    }

    /// #8706: Properties of an assembly from the Browser's menu returns at once with the card
    /// measuring on a copy, and the result (the same a script gets) fills it in.
    #[test]
    fn assembly_properties_from_the_menu_measure_off_the_window_thread() {
        let mut h = Harness::new();
        let block = h.app.run("component.create", json!({"name": "Block"})).unwrap()["component"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        for i in 1..12 {
            h.app.run("occurrence.copy", json!({"component": block, "translate": [20 * i, 0, 0]})).unwrap();
        }
        h.app.apply_off_thread = Some(std::time::Duration::ZERO);
        // Held until checked: under load the worker could otherwise finish before the menu returns.
        let (finish, gate) = std::sync::mpsc::channel();
        h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
        let items = crate::context_menu::entries(&h.app, &crate::context_menu::Target::Component { id: 0 });
        let item = items.iter().find(|i| i.label == "Properties").expect("Properties item");
        crate::context_menu::run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(h.app.menu.props_measuring.is_some(), "the menu returns at once, the card measuring");
        assert!(h.app.menu.props.is_none());
        finish.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        while h.app.menu.props.is_none() {
            assert!(std::time::Instant::now() < deadline, "the measurements never came in");
            assert!(h.app.menu.props_measuring.is_some(), "still measuring until the result is in");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.app.menu.props_measuring.is_none());
        let (title, v) = h.app.menu.props.clone().unwrap();
        assert_eq!(title, h.app.session.doc.name);
        assert_eq!(v["body_count"], 12);
        assert!((v["total"]["volume_mm3"].as_f64().unwrap() - 12_000.0).abs() < 1e-6, "{v}");
        let script = h.app.session.execute("component.properties", &json!({"component": 0})).unwrap();
        assert_eq!(v, script);
    }

    #[test]
    fn moments_keep_three_significant_digits_and_no_signed_zero() {
        let h = Harness::new();
        let decimals = usize::from(h.app.preferences.length_decimals);
        assert_eq!(super::number_text(&h.app, 1234.5678), format!("{:.decimals$}", 1234.5678));
        assert_eq!(super::number_text(&h.app, 0.000_012_34), "0.0000123");
        assert!(!super::number_text(&h.app, -1e-30).starts_with('-'));
        assert_eq!(super::number_text(&h.app, 0.0), format!("{:.decimals$}", 0.0));
    }

    #[test]
    fn the_file_menu_browser_and_root_component_open_the_window() {
        let mut h = Harness::new();
        let items = crate::context_menu::entries(&h.app, &crate::context_menu::Target::Component { id: 0 });
        let item = items.iter().find(|i| i.id == "ui.designProperties").expect("root component menu item");
        assert_eq!(item.label, "Design Properties");
        crate::context_menu::run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(h.app.design_properties.is_some());
        h.app.design_properties = None;
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), true));
        h.frames(2);
        h.step(&json!({"widget": "Design Properties…"})).unwrap();
        assert!(h.app.design_properties.is_some());
        h.app.design_properties = None;
        // Browser → Document Settings → Design Properties.
        h.step(&json!({"click": {"chevron": "Document Settings"}})).unwrap();
        h.step(&json!({"click": {"browser": "Design Properties"}})).unwrap();
        assert!(h.app.design_properties.is_some());
    }

    /// #12218: an inertia cell reads as its entry and value; a row whose label does not name each
    /// cell, as the label and the cell's place.
    #[test]
    fn inertia_cells_read_as_their_entry() {
        assert_eq!(super::cell_name("Ixx, Ixy, Ixz", 3, 1, "4.5"), "Ixy: 4.5");
        assert_eq!(super::cell_name("Principal moments", 3, 2, "7"), "Principal moments 3: 7");
    }
}
