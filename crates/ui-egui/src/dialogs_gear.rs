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
    /// The values the gear had when the form opened on it: what Reset goes back to. A new gear has none.
    original: Option<Box<Gear>>,
    pub internal: bool,
    pub module: f64,
    pub teeth: f64,
    pub pressure_angle: f64,
    pub face_width: f64,
    pub profile_shift: f64,
    pub backlash: f64,
    pub backlash_thicker: bool,
    pub clearance: f64,
    pub root_fillet: f64,
    pub tip_round: f64,
    pub flank_points: f64,
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
            original: None,
            internal: false,
            module: 2.0,
            teeth: 20.0,
            pressure_angle: 20.0,
            face_width: 20.0,
            profile_shift: 0.0,
            backlash: 0.0,
            backlash_thicker: false,
            clearance: 0.25,
            root_fillet: 0.0,
            tip_round: 0.0,
            flank_points: 8.0,
            rim_thickness: 4.0,
            bore_diameter: 0.0,
            bore_offset: 0.0,
            helix_angle: 0.0,
            double_helical: false,
        }
    }

    fn saved(component: u64, g: &SpurGear) -> Self {
        let mut gear = Self::saved_values(component, g);
        gear.original = Some(Box::new(gear.clone()));
        gear
    }

    fn saved_values(component: u64, g: &SpurGear) -> Self {
        Self {
            component: Some(component),
            original: None,
            internal: g.internal,
            module: g.module,
            teeth: f64::from(g.teeth),
            pressure_angle: g.pressure_angle,
            face_width: g.face_width,
            profile_shift: g.profile_shift,
            backlash: g.backlash,
            backlash_thicker: g.backlash_thicker,
            clearance: g.clearance,
            root_fillet: g.root_fillet,
            tip_round: g.tip_round,
            flank_points: f64::from(g.flank_points),
            rim_thickness: if g.rim_thickness > 0.0 { g.rim_thickness } else { g.module * 2.0 },
            bore_diameter: g.bore_diameter,
            bore_offset: g.bore_offset,
            helix_angle: g.helix_angle,
            double_helical: g.double_helical,
        }
    }
}

/// The starting points the form offers: each sets what makes the kind of gear and leaves the size
/// (module, pressure angle, face width) as it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Preset {
    Spur,
    Helical,
    Herringbone,
    Ring,
}

impl Gear {
    /// Back to what the form opened with: the saved values of the gear being edited, or the defaults.
    pub fn reset(&mut self) {
        *self = match self.original.take() {
            // The copy stays, so a second Reset goes to the same place.
            Some(o) => Self { original: Some(o.clone()), ..*o },
            None => Self::new(),
        };
    }

    /// Make this a spur, helical, herringbone or ring gear; the axle hole, clearance and roundings stay.
    pub fn preset(&mut self, p: Preset) {
        (self.internal, self.helix_angle, self.double_helical) = (false, 0.0, false);
        match p {
            Preset::Spur => self.teeth = 20.0,
            Preset::Helical => (self.teeth, self.helix_angle) = (20.0, 20.0),
            Preset::Herringbone => (self.teeth, self.helix_angle, self.double_helical) = (20.0, 20.0, true),
            Preset::Ring => {
                self.internal = true;
                self.teeth = 40.0;
                self.rim_thickness = self.module * 2.0;
                (self.bore_diameter, self.bore_offset) = (0.0, 0.0);
            }
        }
    }
}

/// Insert Gear always opens a new form; Edit Gear needs one gear selected, and tells the user
/// when there is none.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    match id {
        "gear.insert" => Some((Kind::Gear(Gear::new()), vec![])),
        "gear.edit" => {
            let component = selected(app)?;
            Some((Kind::Gear(Gear::saved(component, app.session.doc.gears.get(&component)?)), vec![]))
        }
        _ => None,
    }
}

/// The component a selection stands for, if it is a gear.
pub fn selected(app: &SolveApp) -> Option<u64> {
    crate::dialogs_part_size::selected_component_where(app, |doc, c| doc.gears.contains_key(&c))
}

/// The component the picked bodies belong to, if it is a gear.
pub fn of_bodies(app: &SolveApp, bodies: &[String]) -> Option<u64> {
    crate::dialogs_part_size::component_for_bodies_where(app, bodies, |doc, c| doc.gears.contains_key(&c))
}

/// Edit Gear for a component picked in a menu: the form opens on exactly that gear.
pub fn open(app: &mut SolveApp, component: u64) {
    app.session.selection.clear();
    app.tree.picked_components = vec![component];
    app.start("gear.edit");
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
    {
        ui.label(solidcraft_ui_text::tr("gear.presets"));
        ui.horizontal_wrapped(|ui| {
            for (key, handle, p) in [
                ("gear.preset_spur", "dialog:gear:preset_spur", Preset::Spur),
                ("gear.preset_helical", "dialog:gear:preset_helical", Preset::Helical),
                ("gear.preset_herringbone", "dialog:gear:preset_herringbone", Preset::Herringbone),
                ("gear.preset_ring", "dialog:gear:preset_ring", Preset::Ring),
            ] {
                let r = crate::ui_kit::button(ui, solidcraft_ui_text::tr(key), crate::ui_kit::Button::Normal);
                crate::scenario::publish_handle(handle, r.rect.center());
                if r.clicked() {
                    g.preset(p);
                }
            }
            let r = crate::ui_kit::button(ui, solidcraft_ui_text::tr("gear.reset"), crate::ui_kit::Button::Normal);
            crate::scenario::publish_handle("dialog:gear:reset", r.rect.center());
            if r.clicked() {
                g.reset();
            }
        });
    }
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
    if g.backlash > 0.0 {
        form_row(ui, "", |ui| {
            let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("gear.backlash_thicker"), &mut g.backlash_thicker);
            crate::scenario::publish_handle("dialog:gear:backlash_thicker", r.rect.center());
        });
    }
    number(ui, "gear.clearance", "dialog:gear:clearance", &mut g.clearance, 0.0..=0.6, 0.01, "");
    number(ui, "gear.root_fillet", "dialog:gear:root_fillet", &mut g.root_fillet, 0.0..=1000.0, 0.05, " mm");
    number(ui, "gear.tip_round", "dialog:gear:tip_round", &mut g.tip_round, 0.0..=1000.0, 0.05, " mm");
    number(ui, "gear.flank_points", "dialog:gear:flank_points", &mut g.flank_points, 2.0..=64.0, 1.0, "");
    number(ui, "gear.helix_angle", "dialog:gear:helix_angle", &mut g.helix_angle, -45.0..=45.0, 0.5, "\u{b0}");
    if g.internal {
        number(ui, "gear.rim_thickness", "dialog:gear:rim_thickness", &mut g.rim_thickness, 0.001..=1e6, 0.5, " mm");
    } else {
        form_row(ui, "", |ui| {
            // Two mirrored helical halves need a helix angle to mirror.
            ui.add_enabled_ui(g.helix_angle != 0.0, |ui| {
                let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("gear.double_helical"), &mut g.double_helical);
                crate::scenario::publish_handle("dialog:gear:double_helical", r.rect.center());
            });
        });
        number(ui, "gear.bore_diameter", "dialog:gear:bore_diameter", &mut g.bore_diameter, 0.0..=1e6, 0.5, " mm");
        if g.bore_diameter > 0.0 {
            number(ui, "gear.bore_offset", "dialog:gear:bore_offset", &mut g.bore_offset, -1e6..=1e6, 0.5, " mm");
        }
    }
    ui.separator();
    diameters(ui, g);
}

/// The profile's diameters, from the values as they are now. They follow the form, so they show
/// before anything is inserted.
fn diameters(ui: &mut egui::Ui, g: &Gear) {
    let teeth = g.teeth.round().clamp(0.0, 1e6) as u32;
    let Some(d) = solidcraft_engine::cmd::gear_diameters(g.module, teeth, g.pressure_angle, g.profile_shift, g.clearance, g.internal) else {
        return;
    };
    for (key, handle, v) in [
        ("gear.pitch_diameter", "dialog:gear:pitch_diameter", d[0]),
        ("gear.base_diameter", "dialog:gear:base_diameter", d[1]),
        ("gear.tip_diameter", "dialog:gear:tip_diameter", d[2]),
        ("gear.root_diameter", "dialog:gear:root_diameter", d[3]),
    ] {
        form_row(ui, solidcraft_ui_text::tr(key), |ui| {
            let r = ui.label(format!("{v:.3} mm"));
            crate::scenario::publish_handle(handle, r.rect.center());
        });
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
        g.clearance,
        g.root_fillet,
        g.tip_round,
        g.flank_points,
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
        "backlash_direction": if g.backlash_thicker { "thicker" } else { "thinner" },
        "clearance": g.clearance,
        "root_fillet": g.root_fillet,
        "tip_round": g.tip_round,
        "flank_points": g.flank_points.round().clamp(0.0, 1e6) as u32,
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

    /// The form carries clearance, the two roundings, the backlash direction and the profile accuracy
    /// to the gear, and Edit Gear brings them back.
    #[test]
    fn the_form_sets_clearance_roundings_backlash_direction_and_accuracy() {
        let mut h = Harness::new();
        h.app.start("gear.insert");
        h.frames(3);
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (g.module, g.teeth, g.backlash, g.backlash_thicker) = (2.0, 24.0, 0.1, true);
            (g.clearance, g.root_fillet, g.tip_round, g.flank_points) = (0.3, 0.5, 0.2, 12.0);
        }
        h.frames(2);
        for k in ["clearance", "root_fillet", "tip_round", "flank_points", "backlash_thicker"] {
            assert!(handle_at(&format!("dialog:gear:{k}")).is_some(), "{k}");
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let (component, saved) = gear(&h).expect("a gear is saved");
        assert_eq!((saved.clearance, saved.root_fillet, saved.tip_round, saved.flank_points, saved.backlash_thicker), (0.3, 0.5, 0.2, 12, true));
        h.app.tree.picked_components = vec![component];
        h.app.start("gear.edit");
        h.frames(3);
        let Some(Kind::Gear(g)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no gear form") };
        assert_eq!((g.clearance, g.root_fillet, g.tip_round, g.flank_points, g.backlash_thicker), (0.3, 0.5, 0.2, 12.0, true));
    }

    /// Wait, within a bound, for the preview of the form's values to be computed.
    #[track_caller]
    fn wait_preview(h: &mut Harness) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        h.frames(2);
        while h.app.preview.busy || !(h.app.preview.active || h.app.preview.error.is_some()) {
            assert!(std::time::Instant::now() < deadline, "the gear preview did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.frame();
        }
        h.frames(2);
    }

    /// The form previews the gear before anything is inserted, follows the values, and leaves the
    /// design and its history alone when it is cancelled.
    #[test]
    fn the_form_previews_the_gear_and_cancel_changes_nothing() {
        let mut h = Harness::new();
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        h.app.start("gear.insert");
        wait_preview(&mut h);
        assert!(h.app.dialog.as_ref().is_some_and(|d| d.previews()), "the gear form previews");
        assert!(h.app.preview.active && h.app.preview.error.is_none(), "a preview is up: {:?}", h.app.preview.error);
        assert!(h.app.session.doc.gears.is_empty() && h.app.session.undo.len() == undo, "nothing is inserted");
        let first = h.app.preview.bounds;
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            g.teeth = 40.0;
        }
        wait_preview(&mut h);
        assert!(h.app.preview.active && h.app.preview.bounds != first, "the preview follows the values");
        // A value that makes no gear says so in the form, not in a preview.
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            g.root_fillet = 1.9;
        }
        wait_preview(&mut h);
        assert!(h.app.preview.error.is_some() || !h.app.preview.active, "a refused value shows no gear");
        crate::dialogs::cancel(&mut h.app);
        h.settle();
        assert!(!h.app.preview.active, "cancel takes the preview away");
        assert_eq!((h.app.session.doc.clone(), h.app.session.undo.len()), (doc, undo));
    }

    /// The presets set the kind of gear and keep the size; Reset goes back to what the form opened with.
    #[test]
    fn presets_set_the_kind_and_reset_goes_back_to_what_the_form_opened_with() {
        let mut g = Gear::new();
        g.module = 3.0;
        g.bore_diameter = 6.0;
        g.preset(Preset::Herringbone);
        assert_eq!((g.module, g.teeth, g.helix_angle, g.double_helical, g.internal, g.bore_diameter), (3.0, 20.0, 20.0, true, false, 6.0));
        g.preset(Preset::Ring);
        assert_eq!((g.internal, g.teeth, g.rim_thickness, g.helix_angle, g.double_helical, g.bore_diameter), (true, 40.0, 6.0, 0.0, false, 0.0));
        g.preset(Preset::Helical);
        assert_eq!((g.internal, g.helix_angle, g.double_helical), (false, 20.0, false));
        g.preset(Preset::Spur);
        assert_eq!((g.internal, g.helix_angle, g.teeth), (false, 0.0, 20.0));
        g.reset();
        assert_eq!(g, Gear::new(), "a new gear resets to the defaults");
        // An edit form resets to the gear as it was saved, still on that gear.
        let saved = SpurGear {
            module: 1.5,
            teeth: 30,
            pressure_angle: 25.0,
            face_width: 8.0,
            profile_shift: 0.0,
            backlash: 0.05,
            flank_points: 12,
            clearance: 0.3,
            root_fillet: 0.0,
            tip_round: 0.0,
            backlash_thicker: false,
            internal: false,
            rim_thickness: 0.0,
            bore_diameter: 5.0,
            bore_offset: 0.0,
            helix_angle: 15.0,
            double_helical: false,
        };
        let mut e = Gear::saved(7, &saved);
        let opened = e.clone();
        e.preset(Preset::Ring);
        e.module = 9.0;
        e.reset();
        assert_eq!(e, opened);
        assert_eq!((e.component, e.teeth, e.helix_angle, e.flank_points), (Some(7), 30.0, 15.0, 12.0));
    }

    /// The buttons are on the form, and a preset's gear builds.
    #[test]
    fn the_form_offers_the_presets_and_reset_and_a_preset_builds() {
        let mut h = Harness::new();
        h.app.start("gear.insert");
        h.frames(3);
        for k in ["preset_spur", "preset_helical", "preset_herringbone", "preset_ring", "reset"] {
            assert!(handle_at(&format!("dialog:gear:{k}")).is_some(), "{k}");
        }
        if let Some(Kind::Gear(g)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            g.preset(Preset::Herringbone);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let (_, saved) = gear(&h).expect("a gear is saved");
        assert_eq!((saved.helix_angle, saved.double_helical), (20.0, true));
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

    /// The form shows the profile's diameters before anything is inserted.
    #[test]
    fn the_form_shows_the_diameters_as_the_values_change() {
        let mut h = Harness::new();
        h.app.start("gear.insert");
        h.frames(3);
        for k in ["pitch", "base", "tip", "root"] {
            assert!(handle_at(&format!("dialog:gear:{k}_diameter")).is_some(), "{k}");
        }
        assert!(h.app.session.doc.gears.is_empty(), "nothing is inserted to show them");
    }

    /// A menu opens Edit Gear on exactly the gear it was opened for, and the ribbon offers both commands.
    #[test]
    fn a_menu_opens_the_form_on_the_gear_it_was_opened_for() {
        assert!(crate::workspace::SECTIONS.iter().any(|s| s.ids.contains(&"gear.insert") && s.ids.contains(&"gear.edit")));
        let mut h = Harness::new();
        h.app.run("gear.insert", json!({"module": 2, "teeth": 20, "name": "First"})).unwrap();
        h.app.run("gear.insert", json!({"module": 3, "teeth": 30, "name": "Second"})).unwrap();
        h.settle();
        let second = h.app.session.doc.gears.iter().find(|(_, g)| g.teeth == 30).map(|(c, _)| *c).expect("the second gear");
        open(&mut h.app, second);
        h.frames(3);
        let Some(Kind::Gear(g)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no gear form: {:?}", h.app.status) };
        assert_eq!((g.component, g.module, g.teeth), (Some(second), 3.0, 30.0));
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
