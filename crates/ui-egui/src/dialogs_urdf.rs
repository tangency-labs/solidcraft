//! Export URDF: the options of `file.export` for a robot description (docs/urdf.md), with the
//! motion-link choice remembered.
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UrdfExport {
    pub mesh_prefix: String,
    /// Empty: the export's default, with its warning.
    pub effort: String,
    pub velocity: String,
    pub mimic: bool,
    /// A body with no collision shape of its own becomes a convex hull, not the full mesh.
    pub hulls: bool,
}

pub fn open(app: &mut SolveApp) {
    let form = UrdfExport { mimic: app.preferences.urdf_mimic, ..Default::default() };
    app.dialog = Some(Dialog::new(Kind::UrdfExport(form), Vec::new()));
}

pub fn rows(ui: &mut egui::Ui, f: &mut UrdfExport) -> bool {
    let mut enter = crate::ui_kit::text_row(ui, "Mesh file prefix", &mut f.mesh_prefix, "package://robot/meshes/").0;
    enter |= crate::ui_kit::text_row(ui, "Joint effort", &mut f.effort, "1000").0;
    enter |= crate::ui_kit::text_row(ui, "Joint velocity", &mut f.velocity, "10").0;
    crate::ui_kit::text(ui, crate::i18n::tr_label("urdf.fallback_help"), crate::ui_kit::Text::Note("urdf_fallback"));
    let r = crate::ui_kit::checkbox(ui, crate::i18n::tr_label("Couple motion links"), &mut f.mimic);
    crate::scenario::publish_handle("dialog:urdf:mimic", r.rect.center());
    crate::ui_kit::checkbox(ui, crate::i18n::tr_label("Convex hull collision"), &mut f.hulls);
    enter
}

/// The parameters of `file.export`, or what is wrong with the form.
pub fn params(f: &UrdfExport, path: &str) -> Result<Value, String> {
    let mut p = json!({"path": path, "format": "urdf", "mimic": f.mimic, "collision": if f.hulls { "convex_hull" } else { "mesh" }});
    if !f.mesh_prefix.trim().is_empty() {
        p["mesh_prefix"] = json!(f.mesh_prefix.trim());
    }
    for (key, label, text) in [("effort", "Joint effort", &f.effort), ("velocity", "Joint velocity", &f.velocity)] {
        let text = text.trim();
        if !text.is_empty() {
            let x = text.parse::<f64>().ok().filter(|x| x.is_finite() && *x > 0.0);
            let x = x.ok_or_else(|| format!("{}: {}", crate::i18n::tr_label(label), crate::i18n::tr_label("urdf.positive_number")))?;
            p[key] = json!(x);
        }
    }
    Ok(p)
}

pub fn run(app: &mut SolveApp, f: &UrdfExport) -> Result<(), String> {
    params(f, "")?;
    app.preferences.urdf_mimic = f.mimic;
    let form = f.clone();
    let name = format!("{}.urdf", app.session.doc.name);
    app.pick_save(&name, &["urdf"], move |app, path| {
        let Some(path) = path else { return };
        let Ok(p) = params(&form, &path) else { return };
        if app.run("file.export", p).is_ok() {
            app.set_status(format!("Exported {path}"), false);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> UrdfExport {
        UrdfExport { mimic: true, ..Default::default() }
    }

    #[test]
    fn empty_numbers_are_left_to_the_exports_fallback() {
        let p = params(&form(), "r.urdf").unwrap();
        assert_eq!(p, json!({"path": "r.urdf", "format": "urdf", "mimic": true, "collision": "mesh"}));
    }

    #[test]
    fn the_dialog_passes_every_option_to_file_export() {
        let f = UrdfExport { mesh_prefix: "package://r/meshes/".into(), effort: "5".into(), velocity: "2.5".into(), mimic: false, hulls: true };
        let p = params(&f, "r.urdf").unwrap();
        assert_eq!(
            p,
            json!({"path": "r.urdf", "format": "urdf", "mimic": false, "collision": "convex_hull", "mesh_prefix": "package://r/meshes/", "effort": 5.0, "velocity": 2.5})
        );
    }

    #[test]
    fn a_number_the_export_would_refuse_stops_here() {
        for bad in ["lots", "0", "-1", "inf"] {
            assert!(params(&UrdfExport { effort: bad.into(), ..form() }, "r.urdf").is_err(), "{bad}");
        }
    }

    #[test]
    fn export_urdf_opens_the_dialog_with_the_preference() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.urdf_mimic = false;
        open(&mut h.app);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::UrdfExport(UrdfExport { mimic: false, .. }))));
    }

    /// Two boxes on revolute joints to a grounded one, the second driven by the first.
    fn coupled(h: &mut crate::scenario::Harness) {
        for (name, x) in [("H", 0), ("A", 20), ("B", 40)] {
            h.app.run("component.activate", json!({"component": "root"})).unwrap();
            h.app.run("component.create", json!({"name": name})).unwrap();
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        for (name, x) in [("A", 20), ("B", 40)] {
            let b = json!({"occurrence": format!("{name}:1"), "face": [x + 5, 5, 0]});
            h.app.run("joint.create", json!({"type": "revolute", "a": {"occurrence": "H:1", "face": [5, 5, 10]}, "b": b, "name": name})).unwrap();
        }
        h.app.run("joint.gear", json!({"a": "A", "b": "B", "ra": 10, "rb": 20})).unwrap();
    }

    fn export(h: &mut crate::scenario::Harness, mimic: bool) -> String {
        let dir = h.dir.clone();
        h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(dir.join("r.urdf").to_string_lossy().into_owned())))));
        run(&mut h.app, &UrdfExport { mimic, ..Default::default() }).unwrap();
        h.settle();
        std::fs::read_to_string(h.dir.join("r.urdf")).unwrap()
    }

    #[test]
    fn the_preference_off_exports_no_mimic_and_is_remembered() {
        let mut h = crate::scenario::Harness::new();
        coupled(&mut h);
        assert!(export(&mut h, true).contains("<mimic"));
        assert!(h.app.preferences.urdf_mimic);
        assert!(!export(&mut h, false).contains("<mimic"));
        let saved = h.app.prefs();
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&saved);
        assert!(!restored.preferences.urdf_mimic);
    }
}
