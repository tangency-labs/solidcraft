//! Insert Pulley: a timing-belt pulley's profile, grooves, belt width, flanges and axle hole. The
//! command (`pulley.insert`) checks the sizes; the dialog only reads its fields.
use crate::{
    SolveApp,
    dialogs::{Kind, check},
    selection::SelInput,
    ui_kit::{Count, choice_row, count_row, expr_row_positive, row},
};
use serde_json::{Value, json};
use solidcraft_engine::doc::expr::Kind as ValueKind;

/// The profiles, in the order the dialog lists them: (command id, name, default belt width).
const PROFILES: [(&str, &str, &str); 3] = [("gt2", "GT2 (2 mm)", "6 mm"), ("htd3m", "HTD 3M", "9 mm"), ("htd5m", "HTD 5M", "15 mm")];

#[derive(Clone, Debug)]
pub struct Pulley {
    /// An index into `PROFILES`.
    pub profile: usize,
    pub teeth: String,
    pub belt_width: String,
    pub flanges: bool,
    /// An axle hole through it.
    pub bore: bool,
    pub bore_diameter: String,
    pub bore_offset: String,
}

impl Default for Pulley {
    fn default() -> Self {
        Self {
            profile: 0,
            teeth: "20".into(),
            belt_width: PROFILES[0].2.into(),
            flanges: true,
            bore: true,
            bore_diameter: "5 mm".into(),
            bore_offset: "0 mm".into(),
        }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "pulley.insert").then(|| (Kind::Pulley(Pulley::default()), Vec::new()))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, k: &mut Pulley) -> bool {
    let was = k.profile;
    let names: Vec<&str> = PROFILES.iter().map(|p| p.1).collect();
    choice_row(ui, app.tr("ui.pulley.profile"), "pulley_profile", &mut k.profile, &names);
    // A new profile brings its own belt width, unless one was typed.
    if k.profile != was
        && let (Some(old), Some(new)) = (PROFILES.get(was), PROFILES.get(k.profile))
        && k.belt_width.trim() == old.2
    {
        k.belt_width = new.2.into();
    }
    let doc = &app.session.doc;
    let eval = |text: &str| doc.eval(text.trim(), ValueKind::Unitless).ok();
    let mut enter = count_row(ui, app.tr("ui.pulley.teeth"), &mut k.teeth, &Count { min: 10, max: 200, eval: &eval });
    enter |= expr_row_positive(ui, app.tr("ui.pulley.belt_width"), &mut k.belt_width);
    row(ui, app.tr("ui.pulley.flanges"), |ui| check(ui, &mut k.flanges));
    row(ui, app.tr("ui.pulley.bore"), |ui| check(ui, &mut k.bore));
    if k.bore {
        enter |= expr_row_positive(ui, app.tr("ui.pulley.bore_diameter"), &mut k.bore_diameter);
        enter |= expr_row_positive(ui, app.tr("ui.pulley.bore_offset"), &mut k.bore_offset);
    }
    enter
}

/// A field's value in mm (or a plain number), as the document evaluates it.
fn value(app: &SolveApp, text: &str, kind: ValueKind, label: &str) -> Result<f64, String> {
    crate::length_input::eval(&app.session.doc, text, kind).map_err(|e| format!("{}: {e}", app.tr(label)))
}

pub fn commands(app: &SolveApp, k: &Pulley) -> Result<Vec<(String, Value)>, String> {
    let profile = PROFILES.get(k.profile).map_or("gt2", |p| p.0);
    let teeth = value(app, &k.teeth, ValueKind::Unitless, "ui.pulley.teeth")?.round();
    if !(10.0..=200.0).contains(&teeth) {
        return Err(format!("{}: {teeth}", app.tr("ui.pulley.teeth")));
    }
    let mut params = json!({
        "profile": profile,
        "teeth": teeth as u32,
        "belt_width": value(app, &k.belt_width, ValueKind::Length, "ui.pulley.belt_width")?,
        "flanges": k.flanges,
    });
    if k.bore
        && let Value::Object(p) = &mut params
    {
        p.insert("bore_diameter".into(), json!(value(app, &k.bore_diameter, ValueKind::Length, "ui.pulley.bore_diameter")?));
        p.insert("bore_offset".into(), json!(value(app, &k.bore_offset, ValueKind::Length, "ui.pulley.bore_offset")?));
    }
    Ok(vec![("pulley.insert".into(), params)])
}
