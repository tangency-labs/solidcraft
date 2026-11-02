//! Insert and edit spur gears through the same command flow as other standard parts.

use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::doc::Document;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::dialogs_part_size;
use crate::selection::SelInput;
use crate::ui_kit::row;

#[derive(Clone, Debug)]
pub struct Form {
    pub component: Option<u64>,
    pub module: String,
    pub teeth: String,
    pub face_width: String,
    pub bore: String,
    pub pressure_angle: String,
    pub backlash: String,
    pub root_fillet_radius: String,
    pub head_fillet_radius: String,
    pub helix_angle: String,
    pub num_points: String,
    pub simple: bool,
    pub double_helix: bool,
    pub axle_hole: bool,
    pub internal_gear: bool,
    pub rim_thickness: String,
    pub offset_hole: bool,
    pub offset_hole_offset: String,
    pub offset_hole_size: String,
    pub profile_shift: String,
    pub clearance: String,
    pub head_tolerance: String,
    pub reversed_backlash: bool,
    source: Option<Arc<Document>>,
    original: Option<Vec<String>>,
}

impl Form {
    fn defaults() -> Self {
        Self {
            component: None,
            module: "2".into(),
            teeth: "24".into(),
            face_width: "10".into(),
            bore: "8".into(),
            pressure_angle: "20".into(),
            backlash: "0".into(),
            root_fillet_radius: "0".into(),
            head_fillet_radius: "0".into(),
            helix_angle: "90".into(),
            num_points: "20".into(),
            simple: false,
            double_helix: false,
            axle_hole: true,
            internal_gear: false,
            rim_thickness: "10".into(),
            offset_hole: false,
            offset_hole_offset: "10".into(),
            offset_hole_size: "10".into(),
            profile_shift: "0".into(),
            clearance: "0.25".into(),
            head_tolerance: "0".into(),
            reversed_backlash: false,
            source: None,
            original: None,
        }
    }

    fn existing(app: &SolveApp, component: u64) -> Option<Self> {
        let part = app.session.doc.parts.get(&component)?;
        if part.family != "spur_gear" {
            return None;
        }

        let mut values = std::collections::BTreeMap::new();
        for field in part.size.split(';') {
            let (key, value) = field.split_once('=')?;
            if values.insert(key, value).is_some() {
                return None;
            }
        }
        let get = |key: &str, default: &str| values.get(key).copied().unwrap_or(default).to_owned();
        let face_width = part.length?.to_string();
        let mut form = Self {
            component: Some(component),
            module: values.get("m")?.to_string(),
            teeth: values.get("z")?.to_string(),
            face_width,
            bore: values.get("b")?.to_string(),
            pressure_angle: values.get("pa")?.to_string(),
            backlash: get("bl", "0"),
            root_fillet_radius: get("fr", "0"),
            head_fillet_radius: get("hf", "0"),
            helix_angle: get("ha", "90"),
            num_points: get("np", "20"),
            simple: get("sm", "false") == "true",
            double_helix: get("dh", "false") == "true",
            axle_hole: get("ah", "true") == "true",

            internal_gear: get("ig", "false") == "true",
            rim_thickness: get("rh", "10"),

            offset_hole: get("oh", "false") == "true",
            offset_hole_offset: get("oho", "10"),
            offset_hole_size: get("ohs", "10"),
            profile_shift: get("ps", "0"),
            clearance: get("cl", "0.25"),
            head_tolerance: get("ht", "0"),
            reversed_backlash: get("rb", "false") == "true",
            source: Some(app.session.doc.clone()),
            original: None,
        };
        form.original = Some(form.snapshot());
        Some(form)
    }

    fn snapshot(&self) -> Vec<String> {
        vec![
            self.module.clone(),
            self.teeth.clone(),
            self.face_width.clone(),
            self.bore.clone(),
            self.pressure_angle.clone(),
            self.backlash.clone(),
            self.root_fillet_radius.clone(),
            self.head_fillet_radius.clone(),
            self.helix_angle.clone(),
            self.num_points.clone(),
            self.simple.to_string(),
            self.double_helix.to_string(),
            self.axle_hole.to_string(),
            self.internal_gear.to_string(),
            self.rim_thickness.clone(),
            self.offset_hole.to_string(),
            self.offset_hole_offset.clone(),
            self.offset_hole_size.clone(),
            self.profile_shift.clone(),
            self.clearance.clone(),
            self.head_tolerance.clone(),
            self.reversed_backlash.to_string(),
        ]
    }

    fn current(&self, app: &SolveApp) -> Result<(), String> {
        let Some(component) = self.component else {
            return Ok(());
        };
        if !self.source.as_ref().is_some_and(|source| Arc::ptr_eq(source, &app.session.doc))
            || app.session.doc.parts.get(&component).is_none_or(|part| part.family != "spur_gear")
        {
            return Err("The design changed. Reopen Edit Gear Parameters for the component.".into());
        }
        Ok(())
    }

    fn params(&self) -> Result<Value, String> {
        let parse = |label: &str, value: &str| {
            let parsed = value.trim().parse::<f64>().map_err(|_| format!("{label} must be a number"))?;
            if !parsed.is_finite() {
                return Err(format!("{label} must be finite"));
            }
            Ok(parsed)
        };
        let teeth = self.teeth.trim().parse::<u32>().map_err(|_| "Tooth count must be a positive integer")?;

        let internal_gear = self.internal_gear;

        Ok(json!({
            "module": parse("Module", &self.module)?,
            "teeth": teeth,
            "height": parse("Gear height", &self.face_width)?,
                    // Internal gears do not use axle or offset holes.
            "bore": if internal_gear {
                0.0
            } else {
                parse("Bore", &self.bore)?
            },
            "pressure_angle": parse("Pressure angle", &self.pressure_angle)?,
            "backlash": parse("Backlash", &self.backlash)?,
            "root_fillet_radius": parse("Root fillet radius", &self.root_fillet_radius)?,
            "head_fillet_radius": parse("Head fillet radius", &self.head_fillet_radius)?,
            "helix_angle": parse("Helix angle", &self.helix_angle)?,
            "num_points": parse("Profile accuracy", &self.num_points)?,
            "simple": self.simple,
            "double_helix": self.double_helix,

            "axle_hole": !internal_gear && self.axle_hole,
            "internal_gear": internal_gear,
            "rim_thickness": parse("Rim thickness", &self.rim_thickness)?,


            "offset_hole": !internal_gear && self.offset_hole,
            "offset_hole_offset": parse("Offset hole distance", &self.offset_hole_offset)?,
            "offset_hole_size": parse("Offset hole diameter", &self.offset_hole_size)?,

            "profile_shift": parse("Profile shift", &self.profile_shift)?,
            "clearance": parse("Clearance", &self.clearance)?,
            "head_tolerance": parse("Head tolerance", &self.head_tolerance)?,
            "reversed_backlash": self.reversed_backlash
        }))
    }
}

pub fn selected_component(app: &SolveApp) -> Option<u64> {
    let component = dialogs_part_size::selected_component(app)?;
    app.session.doc.parts.get(&component).is_some_and(|part| part.family == "spur_gear").then_some(component)
}

pub fn edit_command(app: &SolveApp, component: u64) -> Option<&'static str> {
    let part = app.session.doc.parts.get(&component)?;
    Some(if part.family == "spur_gear" { "gears.resize" } else { "parts.resize" })
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let form = match id {
        "gears.insert" => Form::defaults(),
        "gears.resize" => Form::existing(app, selected_component(app)?)?,
        _ => return None,
    };
    Some((Kind::Gear(form), vec![]))
}

pub fn open(app: &mut SolveApp, component: u64) {
    if Form::existing(app, component).is_none() {
        app.set_status("Select an inserted spur gear to edit its parameters", true);
        return;
    }
    app.session.selection.clear();
    app.tree.picked_components = vec![component];
    app.start("gears.resize");
}

pub fn rows(_app: &mut SolveApp, ui: &mut egui::Ui, form: &mut Form) -> bool {
    row(ui, "Module (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.module, "e.g. 2").0);
    row(ui, "Tooth count", |ui| crate::ui_kit::text_field(ui, &mut form.teeth, "e.g. 24").0);
    row(ui, "Gear height (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.face_width, "e.g. 10").0);

    row(ui, "Internal Gear", |ui| ui.checkbox(&mut form.internal_gear, "").changed());

    if form.internal_gear {
        row(ui, "Rim thickness (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.rim_thickness, "e.g. 5").0);
    } else {
        row(ui, "Axle hole", |ui| ui.checkbox(&mut form.axle_hole, "").changed());

        if form.axle_hole {
            row(ui, "Axle hole diameter (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.bore, "0 for none").0);
        }

        row(ui, "Offset hole", |ui| ui.checkbox(&mut form.offset_hole, "").changed());

        if form.offset_hole {
            row(ui, "Offset hole distance (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.offset_hole_offset, "from gear center").0);

            row(ui, "Offset hole diameter (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.offset_hole_size, "hole diameter").0);
        }
    }

    row(ui, "Pressure angle (deg)", |ui| crate::ui_kit::text_field(ui, &mut form.pressure_angle, "e.g. 20").0);
    row(ui, "Backlash (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.backlash, "positive or negative").0);
    row(ui, "Reverse backlash", |ui| ui.checkbox(&mut form.reversed_backlash, "").changed());
    row(ui, "Profile shift (modules)", |ui| crate::ui_kit::text_field(ui, &mut form.profile_shift, "e.g. 0").0);
    row(ui, "Clearance (modules)", |ui| crate::ui_kit::text_field(ui, &mut form.clearance, "e.g. 0.25").0);
    row(ui, "Head tolerance (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.head_tolerance, "e.g. 0").0);
    row(ui, "Root fillet radius (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.root_fillet_radius, "0 for none").0);
    row(ui, "Head fillet radius (mm)", |ui| crate::ui_kit::text_field(ui, &mut form.head_fillet_radius, "0 for none").0);
    row(ui, "Profile accuracy points", |ui| crate::ui_kit::text_field(ui, &mut form.num_points, "4 to 24").0);
    row(ui, "Simple profile", |ui| ui.checkbox(&mut form.simple, "").changed());
    row(ui, "Helix angle (deg)", |ui| crate::ui_kit::text_field(ui, &mut form.helix_angle, "90 straight, <90 helical; max 24 teeth").0);
    row(ui, "Double helix", |ui| ui.checkbox(&mut form.double_helix, "").changed());

    let numeric = |value: &str| value.trim().parse::<f64>().ok().filter(|value| value.is_finite());
    if let (Some(module), Some(teeth), Some(pressure_angle), Some(helix_angle), Some(shift), Some(clearance), Some(head), Some(backlash)) = (
        numeric(&form.module),
        numeric(&form.teeth),
        numeric(&form.pressure_angle),
        numeric(&form.helix_angle),
        numeric(&form.profile_shift),
        numeric(&form.clearance),
        numeric(&form.head_tolerance),
        numeric(&form.backlash),
    ) && module > 0.0
        && teeth > 0.0
        && helix_angle > 0.0
        && helix_angle <= 90.0
    {
        let pitch_radius = module * teeth / 2.0;
        let pitch_diameter = pitch_radius * 2.0;
        let outside_diameter = 2.0 * (pitch_radius + module * (1.0 + shift) - head);
        let root_diameter = 2.0 * (pitch_radius - module * (1.0 + clearance - shift));
        let base_diameter = pitch_diameter * pressure_angle.to_radians().cos();
        let transverse_module = module / helix_angle.to_radians().sin();
        let effective_backlash = if form.reversed_backlash { -backlash } else { backlash };
        row(ui, "Pitch diameter (mm)", |ui| {
            ui.label(format!("{pitch_diameter:.3}"));
            false
        });
        row(ui, "Addendum diameter (mm)", |ui| {
            ui.label(format!("{outside_diameter:.3}"));
            false
        });
        row(ui, "Root diameter (mm)", |ui| {
            ui.label(format!("{root_diameter:.3}"));
            false
        });
        row(ui, "Base diameter (mm)", |ui| {
            ui.label(format!("{base_diameter:.3}"));
            false
        });
        row(ui, "Transverse module (mm)", |ui| {
            ui.label(format!("{transverse_module:.3}"));
            false
        });
        row(ui, "Transverse pitch (mm)", |ui| {
            ui.label(format!("{:.3}", std::f64::consts::PI * transverse_module));
            false
        });
        row(ui, "Angular backlash (deg)", |ui| {
            ui.label(format!("{:.3}", (effective_backlash / pitch_radius).to_degrees()));
            false
        });
    }
    false
}

pub fn commands(app: &SolveApp, form: &Form) -> Result<Vec<(String, Value)>, String> {
    form.current(app)?;
    let mut params = form.params()?;
    let command = if let Some(component) = form.component {
        params["component"] = json!(component);
        "gears.resize"
    } else {
        "gears.insert"
    };
    if let Some(original) = &form.original {
        if form.snapshot() == *original {
            return Ok(Vec::new());
        }
    }
    Ok(vec![(command.into(), params)])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::Services;
    use crate::dialogs::apply_commands;
    use solidcraft_engine::Session;

    #[test]
    fn gear_insert_and_edit_dialog_keep_parameters_and_rebuild_the_component() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        app.start("gears.insert");
        let Some(crate::dialogs::Kind::Gear(form)) = app.dialog.as_mut().map(|dialog| &mut dialog.kind) else {
            panic!("gear insert should open its parameter dialog");
        };
        form.module = "2".into();
        form.teeth = "12".into();
        form.face_width = "8".into();
        form.bore = "4".into();
        form.pressure_angle = "25".into();
        form.backlash = "-0.1".into();
        form.root_fillet_radius = "0.4".into();
        form.double_helix = true;
        form.axle_hole = false;
        form.offset_hole = true;
        form.offset_hole_offset = "5".into();
        form.offset_hole_size = "2".into();
        form.profile_shift = "0.1".into();
        form.clearance = "0.2".into();
        form.head_tolerance = "0.05".into();
        form.num_points = "12".into();
        form.simple = true;
        form.reversed_backlash = true;
        form.helix_angle = "45".into();
        let commands = apply_commands(&app, app.dialog.as_ref().expect("gear dialog")).expect("valid gear form");
        for (id, params) in commands {
            app.run(&id, params).expect("insert gear");
        }
        crate::dialogs::cancel(&mut app);
        let component = app.session.doc.parts.iter().find(|(_, part)| part.family == "spur_gear").map(|(id, _)| *id).expect("inserted gear");

        let items = crate::context_menu::items(&app, &crate::context_menu::Target::Component { id: component });
        let edit = items.iter().find(|item| item.id == "gears.resize").expect("gear edit menu item");
        assert_eq!(edit.label, "Edit Gear Parameters");
        crate::context_menu::run_item(&mut app, edit, egui::Pos2::ZERO);
        assert!(apply_commands(&app, app.dialog.as_ref().expect("gear edit dialog")).expect("unchanged gear form").is_empty());
        let Some(crate::dialogs::Kind::Gear(form)) = app.dialog.as_mut().map(|dialog| &mut dialog.kind) else {
            panic!("gear resize should open its parameter dialog");
        };
        assert_eq!(form.teeth, "12");
        assert_eq!(form.pressure_angle, "25");
        assert_eq!(form.backlash, "-0.1");
        assert_eq!(form.root_fillet_radius, "0.4");
        assert!(form.double_helix && form.simple);
        assert!(!form.axle_hole && form.offset_hole && form.reversed_backlash);
        assert_eq!(form.offset_hole_offset, "5");
        assert_eq!(form.offset_hole_size, "2");
        assert_eq!(form.profile_shift, "0.1");
        assert_eq!(form.clearance, "0.2");
        assert_eq!(form.head_tolerance, "0.05");
        assert_eq!(form.num_points, "12");
        assert_eq!(form.helix_angle, "45");
        form.teeth = "24".into();
        form.pressure_angle = "20".into();
        form.backlash = "-0.2".into();
        form.root_fillet_radius = "0.1".into();
        form.helix_angle = "60".into();
        let commands = apply_commands(&app, app.dialog.as_ref().expect("gear resize dialog")).expect("valid changed gear form");
        for (id, params) in commands {
            app.run(&id, params).expect("resize gear");
        }
        assert!(app.session.doc.parts[&component].size.contains("z=24"));
        assert!(app.session.doc.parts[&component].size.contains("pa=20"));
        assert!(app.session.doc.parts[&component].size.contains("bl=-0.2"));
        assert!(app.session.doc.parts[&component].size.contains("fr=0.1"));
        assert!(app.session.doc.parts[&component].size.contains("ha=60"));

        let path = std::env::temp_dir().join(format!("solidcraft-gear-parameters-{}.solidcraft", std::process::id()));
        app.run("file.save", serde_json::json!({"path":path})).expect("save edited gear");
        let mut reopened = Session::default();
        reopened.execute("doc.open", &serde_json::json!({"path":path})).expect("reopen edited gear");
        let reopened = SolveApp::new(reopened, Services::default());
        let form = Form::existing(&reopened, component).expect("gear parameters persisted");
        assert_eq!(form.teeth, "24");
        assert_eq!(form.pressure_angle, "20");
        assert_eq!(form.backlash, "-0.2");
        assert_eq!(form.root_fillet_radius, "0.1");
        assert_eq!(form.helix_angle, "60");
        assert!(form.double_helix && form.simple);
        assert!(!form.axle_hole && form.offset_hole && form.reversed_backlash);
        assert_eq!(form.offset_hole_offset, "5");
        assert_eq!(form.offset_hole_size, "2");
        assert_eq!(form.profile_shift, "0.1");
        assert_eq!(form.clearance, "0.2");
        assert_eq!(form.head_tolerance, "0.05");
        assert_eq!(form.num_points, "12");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("solidcraft.bak"));
    }
}
