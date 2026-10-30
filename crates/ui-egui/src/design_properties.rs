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

use egui::{Align2, FontId, Stroke};
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::metadata::{DESCRIPTION, Owner, keys};
use solidcraft_engine::geom::Vec3;

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
}

impl DesignProperties {
    pub fn new(app: &SolveApp) -> Self {
        let m = &app.session.doc.metadata;
        let opened: Vec<String> = keys(Owner::Design).map(|k| m.text(k.name).unwrap_or_default().to_string()).collect();
        DesignProperties { fields: opened.clone(), opened, error: None }
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
            d.error = Some(e);
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
    let metadata = app.session.doc.metadata.clone();
    let title = crate::i18n::tr_label("Design Properties");
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_design_properties"), "settings", title).fields().show(ctx, true, |ui| {
        for (i, k) in keys(Owner::Design).enumerate() {
            let (key, label) = (k.name, crate::i18n::tr(k.label));
            let Some(field) = d.fields.get_mut(i) else { continue };
            if key == DESCRIPTION {
                ui_kit::multiline_text_field(ui, label, egui::Id::new(("sc_design_properties", key)), field);
            } else {
                ui_kit::row(ui, label, |ui| ui_kit::text_field(ui, field, ""));
            }
            // Saved by another tool as something other than text: kept unless typed over.
            if metadata.get(key).is_some() && metadata.text(key).is_none() && d.opened.get(i).is_some_and(String::is_empty) {
                ui_kit::caption(ui, label, crate::i18n::tr("ui.design_properties.not_text"));
            }
        }
        ui_kit::text(ui, crate::i18n::tr("ui.design_properties.exports"), ui_kit::Text::Note("exports"));
        let others: Vec<(&String, &Value)> = metadata.others().collect();
        if !others.is_empty() {
            let heading = format!("{} ({})", crate::i18n::tr("ui.design_properties.others"), others.len());
            ui_kit::section(ui, "sc_design_properties_others", &heading, false, |ui| {
                ui_kit::text(ui, crate::i18n::tr("ui.design_properties.others_note"), ui_kit::Text::Note("others"));
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
    if let Some(wait) = app.apply_off_thread
        && let Some(rx) = measure_beside(app.session.scratch(), command, params)
    {
        match rx.recv_timeout(wait) {
            Ok(reply) => opened(app, reply, fallback),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => app.menu.props_measuring = Some(Measuring { fallback, rx }),
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
/// has no name, and the reply to come.
pub struct Measuring {
    fallback: String,
    rx: std::sync::mpsc::Receiver<Result<Value, String>>,
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

/// Run `command` on `copy` on a worker; the reply comes on the returned channel.
#[cfg(not(target_arch = "wasm32"))]
fn measure_beside(mut copy: solidcraft_engine::Session, command: &str, params: &Value) -> Option<std::sync::mpsc::Receiver<Result<Value, String>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (command, params) = (command.to_string(), params.clone());
    std::thread::Builder::new()
        .name("properties".into())
        .spawn(move || {
            let _ = tx.send(copy.execute(&command, &params).map_err(|e| e.to_string()));
        })
        .ok()?;
    Some(rx)
}

/// No threads on the web: measured here.
#[cfg(target_arch = "wasm32")]
fn measure_beside(_copy: solidcraft_engine::Session, _command: &str, _params: &Value) -> Option<std::sync::mpsc::Receiver<Result<Value, String>>> {
    None
}

/// While a measurement runs beside the window: a card saying so (Close drops it), and the
/// Properties card in its place once the reply is in.
fn show_measuring(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(m) = app.menu.props_measuring.take() else { return };
    match m.rx.try_recv() {
        Ok(reply) => return opened(app, reply, m.fallback),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => return app.set_status(app.tr("ui.properties.measuring_failed").to_string(), true),
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    let (heading, text) = (app.tr("ui.properties").to_string(), app.tr("ui.properties.measuring").to_string());
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_props"), "settings", &heading).actions("", "Close", None).show(ctx, true, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui_kit::text(ui, &text, ui_kit::Text::Note("measuring"));
        });
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
        let label = crate::i18n::format("ui.properties.principal_axis", &[("number", &(n + 1).to_string())]);
        rows.push((label, [e.x, e.y, e.z].map(|x| format!("{:.4}", if x.abs() < 5e-5 { 0.0 } else { x })).to_vec()));
    }
    rows
}

/// The window's title: just Properties for the whole design (its name would only be the
/// file's), else with the name of the component, placement or bodies it measured.
fn window_title(title: &str, v: &Value) -> String {
    if v["component"].as_u64() == Some(0) {
        crate::i18n::tr("ui.properties").to_string()
    } else {
        crate::i18n::format("properties.title", &[("name", title)])
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
    let tk = crate::theme::Tokens::view();
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
        rows.push(("", crate::i18n::tr_label("Open surface — no volume").to_string()));
    } else {
        rows.push(("Mass", format!("{:.3} g", number(total, "mass_g"))));
        if let Some(mass) = v["physical_override"]["mass"].as_f64() {
            rows.push(("Mass override", format!("{mass:.3} kg")));
        }
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
    let note = crate::i18n::format("ui.properties.inertia_note", &[("unit", unit)]);
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
    let heading = window_title(&title, &v);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_props"), "settings", &heading).actions("", "Close", None).show(ctx, true, |ui| {
        for (label, value) in &rows {
            ui_kit::row(ui, crate::i18n::tr_label(label), |ui| ui_kit::text(ui, value, ui_kit::Text::Cell));
        }
        if let Some(first) = &first {
            ui_kit::row(ui, &first_label, |ui| ui_kit::text(ui, first, ui_kit::Text::Cell));
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
                            for (column, cell) in columns.iter_mut().zip(cells) {
                                ui_kit::text(column, cell, ui_kit::Text::MonoCell);
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
    if closed || action == ui_kit::Action::Cancel {
        close_object(app);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

    #[test]
    fn properties_note_a_mass_override_next_to_the_computed_mass() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Motor"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.run("component.physical_override", json!({"component": "Motor", "mass": 2.5})).unwrap();
        let measured = h.app.run("component.properties", json!({"component": "Motor"})).unwrap();
        h.app.menu.props = Some(("Motor".into(), measured));
        h.frames(2);
        assert!(h.widget("Mass").is_some());
        assert!(h.widget("Mass override").is_some());
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
    }

    #[test]
    fn each_field_is_labelled_from_the_table_of_keys() {
        use solidcraft_engine::doc::metadata::{KEYS, Owner, keys};
        let english = crate::i18n::Locale::for_language("en");
        for k in KEYS {
            assert_ne!(english.text(k.label), k.label, "`{}`'s label is in the English catalog", k.name);
        }
        let mut h = Harness::new();
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
        use solidcraft_engine::geom::Vec3;
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
        super::open_object(&mut h.app, "component.properties", &json!({}), String::new());
        assert!(h.app.menu.props.is_none(), "nothing is waited for on the window thread");
        let m = h.app.menu.props_measuring.take().expect("measuring beside the window");
        assert_eq!(m.rx.recv().unwrap().unwrap(), here, "the copy measures what the design here does");

        let (tx, rx) = std::sync::mpsc::channel();
        h.app.menu.props_measuring = Some(super::Measuring { fallback: String::new(), rx });
        h.frames(2);
        assert!(h.widget("Measuring…").is_some() && h.app.menu.props.is_none());
        tx.send(Ok(here.clone())).unwrap();
        h.frames(2);
        assert_eq!(h.app.menu.props.as_ref().map(|(_, v)| v), Some(&here));
        assert!(h.widget("Moment of inertia").is_some());

        let (_tx, rx) = std::sync::mpsc::channel::<Result<serde_json::Value, String>>();
        super::close_object(&mut h.app);
        h.app.menu.props_measuring = Some(super::Measuring { fallback: String::new(), rx });
        h.frames(2);
        h.step(&json!({"widget": "Close"})).unwrap();
        assert!(h.app.menu.props_measuring.is_none() && h.app.menu.props.is_none(), "Close drops the measurement");
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
        let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Component { id: 0 });
        let item = items.iter().find(|i| i.id == "ui.designProperties").expect("root component menu item");
        assert_eq!(item.label, "Design Properties");
        crate::context_menu::run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(h.app.design_properties.is_some());
        h.app.design_properties = None;
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_file_menu"), true));
        h.frames(2);
        h.step(&json!({"widget": "Design Properties…"})).unwrap();
        assert!(h.app.design_properties.is_some());
        h.app.design_properties = None;
        // Browser → Document Settings → Design Properties.
        h.step(&json!({"click": {"chevron": "Document Settings"}})).unwrap();
        h.step(&json!({"click": {"browser": "Design Properties"}})).unwrap();
        assert!(h.app.design_properties.is_some());
    }
}
