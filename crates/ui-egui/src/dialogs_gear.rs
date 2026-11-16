//! Insert Gear and Edit Gear: the gear's size, teeth and shape in one form, and OK runs
//! `gear.insert` or `gear.edit` (#6796). Edit Gear starts from the selected gear's saved values.

use serde_json::{Value, json};
use solidcraft_engine::doc::SpurGear;

use crate::SolveApp;
use crate::dialogs::{Kind, form_row};
use crate::selection::SelInput;

/// The values the form edits. `component` is the gear being edited, or `None` for a new one.
/// `teeth` is a number field's value: it is rounded when OK runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Gear {
    pub component: Option<u64>,
    pub internal: bool,
    pub module: f64,
    pub teeth: f64,
    pub pressure_angle: f64,
    pub face_width: f64,
    pub profile_shift: f64,
    pub backlash: f64,
    pub rim_thickness: f64,
    pub bore_diameter: f64,
    pub bore_offset: f64,
    pub helix_angle: f64,
    pub double_helical: bool,
}

impl Gear {
    /// A new gear: module 2, 20 teeth, the engine's own defaults for the rest.
    fn new() -> Self {
        Self {
            component: None,
            internal: false,
            module: 2.0,
            teeth: 20.0,
            pressure_angle: 20.0,
            face_width: 20.0,
            profile_shift: 0.0,
            backlash: 0.0,
            rim_thickness: 4.0,
            bore_diameter: 0.0,
            bore_offset: 0.0,
            helix_angle: 0.0,
            double_helical: false,
        }
    }

    fn saved(component: u64, g: &SpurGear) -> Self {
        Self {
            component: Some(component),
            internal: g.internal,
            module: g.module,
            teeth: f64::from(g.teeth),
            pressure_angle: g.pressure_angle,
            face_width: g.face_width,
            profile_shift: g.profile_shift,
            backlash: g.backlash,
            rim_thickness: if g.rim_thickness > 0.0 { g.rim_thickness } else { g.module * 2.0 },
            bore_diameter: g.bore_diameter,
            bore_offset: g.bore_offset,
            helix_angle: g.helix_angle,
            double_helical: g.double_helical,
        }
    }
}

/// Insert Gear always opens a new form; Edit Gear needs one gear selected, and tells the user
/// when there is none.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    match id {
        "gear.insert" => Some((Kind::Gear(Gear::new()), vec![])),
        "gear.edit" => {
            let component = crate::dialogs_part_size::selected_component_where(app, |doc, c| doc.gears.contains_key(&c))?;
            Some((Kind::Gear(Gear::saved(component, app.session.doc.gears.get(&component)?)), vec![]))
        }
        _ => None,
    }
}

pub fn command_id(g: &Gear) -> &'static str {
    if g.component.is_some() { "gear.edit" } else { "gear.insert" }
}

pub fn title(g: &Gear) -> &'static str {
    if g.component.is_some() { "EDIT GEAR" } else { "INSERT GEAR" }
}

fn number(ui: &mut egui::Ui, label_key: &str, handle: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64, suffix: &str) {
    form_row(ui, solidcraft_ui_text::tr(label_key), |ui| {
        let r = crate::ui_kit::number_field(ui, v, range, speed, suffix);
        crate::scenario::publish_handle(handle, r.rect.center());
    });
}

pub fn rows(ui: &mut egui::Ui, g: &mut Gear) {
    form_row(ui, "", |ui| {
        let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("gear.internal"), &mut g.internal);
        crate::scenario::publish_handle("dialog:gear:internal", r.rect.center());
    });
    number(ui, "gear.module", "dialog:gear:module", &mut g.module, 0.001..=1000.0, 0.1, " mm");
    number(ui, "gear.teeth", "dialog:gear:teeth", &mut g.teeth, 6.0..=1000.0, 1.0, "");
    number(ui, "gear.pressure_angle", "dialog:gear:pressure_angle", &mut g.pressure_angle, 0.1..=35.0, 0.5, "\u{b0}");
    number(ui, "gear.face_width", "dialog:gear:face_width", &mut g.face_width, 0.001..=1e6, 0.5, " mm");
    if !g.internal {
        number(ui, "gear.profile_shift", "dialog:gear:profile_shift", &mut g.profile_shift, -2.0..=2.0, 0.05, "");
    }
    number(ui, "gear.backlash", "dialog:gear:backlash", &mut g.backlash, 0.0..=100.0, 0.01, " mm");
    number(ui, "gear.helix_angle", "dialog:gear:helix_angle", &mut g.helix_angle, -45.0..=45.0, 0.5, "\u{b0}");
    if g.internal {
        number(ui, "gear.rim_thickness", "dialog:gear:rim_thickness", &mut g.rim_thickness, 0.001..=1e6, 0.5, " mm");
    } else {
        form_row(ui, "", |ui| {
            let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("gear.double_helical"), &mut g.double_helical);
            crate::scenario::publish_handle("dialog:gear:double_helical", r.rect.center());
        });
        number(ui, "gear.bore_diameter", "dialog:gear:bore_diameter", &mut g.bore_diameter, 0.0..=1e6, 0.5, " mm");
        if g.bore_diameter > 0.0 {
            number(ui, "gear.bore_offset", "dialog:gear:bore_offset", &mut g.bore_offset, -1e6..=1e6, 0.5, " mm");
        }
    }
}

/// The command OK runs. Every value is sent, so an edit replaces what was saved. A value the
/// form cannot hold is refused here; the engine checks the rest and says what is wrong.
pub fn commands(g: &Gear) -> Result<Vec<(String, Value)>, String> {
    let finite = [
        g.module,
        g.teeth,
        g.pressure_angle,
        g.face_width,
        g.profile_shift,
        g.backlash,
        g.rim_thickness,
        g.bore_diameter,
        g.bore_offset,
        g.helix_angle,
    ]
    .iter()
    .all(|v| v.is_finite());
    if !finite {
        return Err("every gear value must be a number".into());
    }
    let mut p = json!({
        "module": g.module,
        "teeth": g.teeth.round().clamp(0.0, 1e6) as u32,
        "pressure_angle": g.pressure_angle,
        "face_width": g.face_width,
        "backlash": g.backlash,
        "helix_angle": g.helix_angle,
        "internal": g.internal,
    });
    if g.internal {
        p["rim_thickness"] = json!(g.rim_thickness);
        p["bore_diameter"] = json!(0);
    } else {
        p["profile_shift"] = json!(g.profile_shift);
        p["double_helical"] = json!(g.double_helical && g.helix_angle != 0.0);
        p["bore_diameter"] = json!(g.bore_diameter);
        p["bore_offset"] = json!(if g.bore_diameter > 0.0 { g.bore_offset } else { 0.0 });
        p["rim_thickness"] = json!(0);
    }
    if g.internal {
        p["double_helical"] = json!(false);
        p["bore_offset"] = json!(0);
    }
    if let Some(c) = g.component {
        p["component"] = json!(c);
    }
    Ok(vec![(command_id(g).to_string(), p)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    fn gear(h: &Harness) -> Option<(u64, SpurGear)> {
        h.app.session.doc.gears.iter().next().map(|(c, g)| (*c, g.clone()))
    }

    /// The Insert Gear form builds a gear from the values set in it, in one undo step.
    #[test]
    fn the_insert_form_builds_a_helical_gear_with_an_axle_hole() {
        let mut h = Harness::new();
        h.app.start("gear.insert");
        h.frames(3);
        let Some(Kind::Gear(g)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no gear form: {:?}", h.app.status) };
        assert_eq!((g.module, g.teeth, g.component), (2.0, 20.0, None));
        assert!(handle_at("dialog:gear:module").is_some() && handle_at("dialog:gear:helix_angle").is_some(), "the fields are drawn");
        assert!(handle_at("dialog:gear:rim_thickness").is_none(), "an external gear has no ring");
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (g.module, g.teeth, g.face_width, g.helix_angle, g.bore_diameter) = (1.5, 24.0, 8.0, 15.0, 5.0);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let (_, saved) = gear(&h).expect("a gear is saved");
        assert_eq!((saved.module, saved.teeth, saved.face_width, saved.helix_angle, saved.bore_diameter), (1.5, 24, 8.0, 15.0, 5.0));
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert!(h.app.session.doc.gears.is_empty(), "one undo removes the gear");
    }

    /// Edit Gear starts from the selected gear's saved values and rebuilds it in place.
    #[test]
    fn the_edit_form_starts_from_the_saved_values_and_rebuilds_in_place() {
        let mut h = Harness::new();
        h.app.run("gear.insert", json!({"module": 2, "teeth": 40, "internal": true, "face_width": 6})).unwrap();
        h.settle();
        let (component, _) = gear(&h).expect("a gear");
        h.app.tree.picked_components = vec![component];
        h.app.start("gear.edit");
        h.frames(3);
        let Some(Kind::Gear(g)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no gear form: {:?}", h.app.status) };
        assert_eq!((g.component, g.internal, g.teeth, g.face_width), (Some(component), true, 40.0, 6.0));
        assert!(handle_at("dialog:gear:rim_thickness").is_some(), "an internal gear shows its ring");
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (g.teeth, g.rim_thickness) = (36.0, 5.0);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let (c, saved) = gear(&h).expect("still a gear");
        assert_eq!((c, saved.teeth, saved.rim_thickness, saved.internal), (component, 36, 5.0, true));
    }

    /// A value the form cannot hold is refused, and Cancel adds nothing.
    #[test]
    fn a_bad_value_is_refused_and_cancel_adds_nothing() {
        let good = Gear::new();
        assert!(commands(&good).is_ok());
        for bad in [Gear { module: f64::NAN, ..Gear::new() }, Gear { helix_angle: f64::INFINITY, ..Gear::new() }] {
            assert!(commands(&bad).is_err());
        }
        let mut h = Harness::new();
        h.app.start("gear.insert");
        h.frames(2);
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none() && h.app.session.doc.gears.is_empty());
    }

    /// The engine's refusal shows in the form, and nothing is added.
    #[test]
    fn the_engines_refusal_stays_in_the_form() {
        let mut h = Harness::new();
        h.app.start("gear.insert");
        h.frames(2);
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (g.helix_angle, g.double_helical, g.face_width) = (0.0, true, 5.0);
            g.bore_diameter = 500.0;
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.session.doc.gears.is_empty());
        assert!(h.app.dialog.as_ref().is_some_and(|d| d.error.is_some()), "the form shows why");
    }
}
