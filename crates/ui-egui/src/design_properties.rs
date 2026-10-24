//! The Design Properties window (#1015): the design's project, part number, revision, author,
//! organization, description and licence, and a read-only list of the data other tools keep in
//! the design. OK applies the edited fields through one `document.metadata.set` (one undo step);
//! Cancel and the close button change nothing. File → Design Properties…, the Browser's
//! Document Settings, the root component's menu and `ui.start` of `document.metadata` or
//! `document.metadata.set` open it. [`show_object`] draws the Properties card (mass, volume, area,
//! centre of mass, bounding box, counts) of the body or component a menu asked about; both are
//! `ui_kit::Window` cards.

use serde_json::{Map, Value};
use solidcraft_engine::doc::metadata::{DESCRIPTION, WELL_KNOWN};

use crate::SolveApp;
use crate::ui_kit;

/// Longest preview of another tool's value in the list.
const PREVIEW_CHARS: usize = 80;
/// Rows of other tools' data shown before the list scrolls.
const OTHERS_ROWS: f32 = 6.0;

/// What is being typed; the properties themselves live in the document.
pub struct DesignProperties {
    /// Each well-known key's text when the window opened, in [`WELL_KNOWN`] order.
    pub opened: [String; WELL_KNOWN.len()],
    /// The fields as edited.
    pub fields: [String; WELL_KNOWN.len()],
    /// The last refusal from `document.metadata.set`.
    pub error: Option<String>,
}

impl DesignProperties {
    pub fn new(app: &SolveApp) -> Self {
        let m = &app.session.doc.metadata;
        let opened = WELL_KNOWN.map(|k| m.text(k).unwrap_or_default().to_string());
        DesignProperties { fields: opened.clone(), opened, error: None }
    }

    /// The fields that differ from what the window opened with, as `document.metadata.set`
    /// takes them (empty text removes the key).
    pub fn changes(&self) -> Map<String, Value> {
        WELL_KNOWN
            .iter()
            .zip(self.opened.iter().zip(&self.fields))
            .filter(|(_, (was, now))| was != now)
            .map(|(k, (_, now))| (k.to_string(), Value::String(now.clone())))
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

fn label(key: &str) -> &'static str {
    match key {
        "project" => crate::i18n::tr("ui.design_properties.project"),
        "part_number" => crate::i18n::tr("ui.design_properties.part_number"),
        "revision" => crate::i18n::tr("ui.design_properties.revision"),
        "author" => crate::i18n::tr("ui.design_properties.author"),
        "organization" => crate::i18n::tr("ui.design_properties.organization"),
        "description" => crate::i18n::tr("ui.design_properties.description"),
        _ => crate::i18n::tr("ui.design_properties.license"),
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
        for (i, key) in WELL_KNOWN.iter().enumerate() {
            let label = label(key);
            let Some(field) = d.fields.get_mut(i) else { continue };
            if *key == DESCRIPTION {
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

/// Draw the Properties window of the body or component the menu asked about (mass, volume,
/// area, centre of mass, bounding box and counts), when it is open. Close dismisses it.
pub fn show_object(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((title, v)) = app.menu.props.clone() else { return };
    let number = |x: &Value, k: &str| x.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    let mut rows: Vec<(&str, String)> = Vec::new();
    let total = &v["total"];
    let bodies = v["bodies"].as_array().cloned().unwrap_or_default();
    if let [b] = bodies.as_slice() {
        // The engine names every body's material, measured or not: nothing to guess here.
        rows.push(("Material", b["material"].as_str().unwrap_or_default().to_string()));
        rows.push(("Density", format!("{:.3} g/cm³", number(b, "density_g_cm3"))));
    }
    rows.push(("Mass", format!("{:.3} g", number(total, "mass_g"))));
    rows.push(("Volume", crate::prefs::show_mm(app, number(total, "volume_mm3"), 3)));
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
    let heading = crate::i18n::format("properties.title", &[("name", &title)]);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_props"), "settings", &heading).actions("", "Close", None).show(ctx, true, |ui| {
        for (label, value) in &rows {
            ui_kit::row(ui, crate::i18n::tr_label(label), |ui| ui_kit::text(ui, value, ui_kit::Text::Cell));
        }
    });
    if closed || action == ui_kit::Action::Cancel {
        app.menu.props = None;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

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
