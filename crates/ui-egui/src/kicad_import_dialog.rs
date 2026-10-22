//! Import settings for a KiCad board.
//!
//! Opening a board asks what comes in with it: the referenced 3D models, the front and back
//! copper (and its pads) as an overlay or modelled, the silkscreen and its text, the substrate
//! colour and the thickness. The chosen values are the `doc.open` options, so the same board can
//! be opened again from a script or the control channel.

use serde_json::{Value, json};
use solidcraft_engine::io::KicadCopper;

use crate::SolveApp;

/// The open KiCad import window.
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub path: String,
    pub components: bool,
    pub copper: KicadCopper,
    pub silkscreen: bool,
    /// Use the board's own `general.thickness` (otherwise `thickness`).
    pub board_thickness: bool,
    pub thickness: f64,
    pub color: [u8; 3],
}

impl Dialog {
    pub fn new(path: &str) -> Dialog {
        Dialog {
            path: path.to_string(),
            components: true,
            copper: KicadCopper::None,
            silkscreen: true,
            board_thickness: true,
            thickness: 1.6,
            color: [13, 84, 46],
        }
    }

    /// The `doc.open` parameters, without the path.
    pub fn params(&self) -> Value {
        let mut params = json!({
            "components": self.components,
            "copper": self.copper.as_str(),
            "silkscreen": self.silkscreen,
            "color": format!("#{:02x}{:02x}{:02x}", self.color[0], self.color[1], self.color[2]),
        });
        if !self.board_thickness
            && let Some(map) = params.as_object_mut()
        {
            map.insert("thickness".into(), json!(self.thickness));
        }
        params
    }

    /// Why the options cannot be imported yet, if they cannot.
    fn problem(&self) -> Option<&'static str> {
        if !self.board_thickness && (!self.thickness.is_finite() || self.thickness <= 0.0 || self.thickness > 1000.0) {
            return Some("Board thickness must be greater than 0 and at most 1000 mm.");
        }
        None
    }
}

/// Open the window for a chosen board.
pub fn open(app: &mut SolveApp, path: &str) {
    app.kicad_import = Some(Dialog::new(path));
}

/// The window. Called once per frame while it is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.kicad_import.take() else { return };
    let mut action: Option<&str> = None;
    let mut result = None;

    let (closed, _) = crate::ui_kit::Window::new(egui::Id::new("sc_kicad_import"), "import", crate::i18n::tr_label("Import KiCad PCB"))
        .cards(2.0)
        .fields()
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, true, |ui| {
            ui.label(crate::i18n::tr_label("The board substrate always comes in, with its cutouts and drilled holes."));
            ui.label(crate::i18n::tr_label("Choose what to build with it:"));
            ui.add_space(6.0);
            egui::Grid::new("kicad_import_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                let r = ui.checkbox(&mut dialog.components, crate::i18n::tr_label("3D component models"));
                crate::scenario::publish_handle("kicad:components", r.rect.center());
                ui.label(crate::i18n::tr_label("STEP and VRML models referenced by footprints"));
                ui.end_row();

                ui.label(crate::i18n::tr_label("Copper (traces, vias, zones, pads)"));
                ui.vertical(|ui| {
                    for (mode, label) in [
                        (KicadCopper::None, "None"),
                        (KicadCopper::Overlay, "Overlay on the board"),
                        (KicadCopper::Mesh, "Modelled"),
                    ] {
                        let r = ui.radio(dialog.copper == mode, crate::i18n::tr_label(label));
                        crate::scenario::publish_handle(&format!("kicad:copper:{}", mode.as_str()), r.rect.center());
                        if r.clicked() {
                            dialog.copper = mode;
                        }
                    }
                });
                ui.end_row();

                let r = ui.checkbox(&mut dialog.silkscreen, crate::i18n::tr_label("Silkscreen"));
                crate::scenario::publish_handle("kicad:silkscreen", r.rect.center());
                ui.label(crate::i18n::tr_label("Lines and filled shapes on the front and back silk layers"));
                ui.end_row();

                let r = ui.color_edit_button_srgb(&mut dialog.color);
                crate::scenario::publish_handle("kicad:color", r.rect.center());
                ui.label(crate::i18n::tr_label("Board colour"));
                ui.end_row();

                let r = ui.checkbox(&mut dialog.board_thickness, crate::i18n::tr_label("Use the board's thickness"));
                crate::scenario::publish_handle("kicad:board_thickness", r.rect.center());
                let t = ui.add_enabled(
                    !dialog.board_thickness,
                    egui::DragValue::new(&mut dialog.thickness).speed(0.1).range(0.01..=1000.0).suffix(" mm").max_decimals(3),
                );
                crate::scenario::publish_handle("kicad:thickness", t.rect.center());
                ui.end_row();
            });
            if let Some(problem) = dialog.problem() {
                ui.add_space(4.0);
                ui.colored_label(crate::theme::Tokens::get().error, crate::i18n::tr_label(problem));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ok = ui.add_enabled(dialog.problem().is_none(), egui::Button::new(crate::i18n::tr_label("Import")));
                crate::scenario::publish_handle("kicad:import", ok.rect.center());
                if ok.clicked() {
                    action = Some("import");
                }
                let cancel = ui.button(crate::i18n::tr_label("Cancel"));
                crate::scenario::publish_handle("kicad:cancel", cancel.rect.center());
                if cancel.clicked() {
                    action = Some("cancel");
                }
            });
            if let Some(p) = dialog.problem() {
                result = Some(p);
            }
        });
    if closed {
        action = Some("cancel");
    }

    match action {
        Some("import") if dialog.problem().is_none() => {
            let params = dialog.params();
            app.open_path_async_with_options(&dialog.path, params);
            app.set_status(format!("{}: {}", crate::i18n::tr_label("Importing KiCad board"), dialog.path), false);
        }
        Some("cancel") => {}
        _ => {
            if let Some(problem) = result {
                app.set_status(crate::i18n::tr_label(problem), true);
            }
            app.kicad_import = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_name_only_the_chosen_options() {
        let mut d = Dialog::new("board.kicad_pcb");
        d.components = false;
        d.copper = KicadCopper::Overlay;
        d.silkscreen = false;
        d.color = [1, 2, 3];
        let p = d.params();
        assert_eq!(p["components"], json!(false));
        assert_eq!(p["copper"], json!("overlay"));
        assert!(p.get("pads").is_none(), "pads follow the copper option");
        assert_eq!(p["silkscreen"], json!(false));
        assert_eq!(p["color"], json!("#010203"));
        assert!(p.get("thickness").is_none());
        d.board_thickness = false;
        d.thickness = 2.4;
        assert_eq!(d.params()["thickness"], json!(2.4));
    }

    #[test]
    fn an_impossible_thickness_is_refused() {
        let mut d = Dialog::new("board.kicad_pcb");
        d.board_thickness = false;
        d.thickness = 0.0;
        assert!(d.problem().is_some());
        d.thickness = 2.0;
        assert!(d.problem().is_none());
    }

    #[test]
    fn the_open_picker_shows_the_settings_window_for_a_board() {
        let mut h = crate::scenario::Harness::new();
        h.app.open_chosen_file("board.kicad_pcb");
        h.frames(1);
        assert!(h.app.kicad_import.is_some(), "a board opens through its settings");
        assert!(!h.app.file_load.busy(), "nothing is read before the settings are chosen");
    }

    #[test]
    fn a_dropped_board_asks_for_its_settings() {
        let mut h = crate::scenario::Harness::new();
        h.app.drop_path("board.kicad_pcb");
        h.frames(1);
        assert!(h.app.kicad_import.is_some(), "a dropped board opens through its settings");
        assert!(!h.app.file_load.busy());
    }

    /// Every way of opening a file goes through the settings window: the app bar's File > Open
    /// and Ctrl+O too, not only Home, a drop and the command behind the Open button.
    #[test]
    fn the_file_menu_and_ctrl_o_open_a_board_through_its_settings() {
        for by_menu in [true, false] {
            let mut h = crate::scenario::Harness::new();
            h.frames(2);
            h.step(&json!({ "choose_file": "board.kicad_pcb" })).unwrap();
            if by_menu {
                h.step(&json!({ "click": { "handle": "appbar:file" } })).unwrap();
                h.step(&json!({ "widget": "Open…" })).unwrap();
            } else {
                h.step(&json!({ "key": "O", "cmd": true })).unwrap();
            }
            h.frames(2);
            let how = if by_menu { "the File menu" } else { "Ctrl+O" };
            assert!(h.app.kicad_import.is_some(), "{how} skipped the import settings");
            assert!(!h.app.file_load.busy(), "{how} read the board before its settings were chosen");
        }
    }

    #[test]
    fn the_window_follows_its_switches_and_imports() {
        let mut h = crate::scenario::Harness::new();
        open(&mut h.app, "board.kicad_pcb");
        h.frames(2);
        for label in [
            "3D component models",
            "Copper (traces, vias, zones, pads)",
            "None",
            "Overlay on the board",
            "Modelled",
            "Silkscreen",
        ] {
            assert!(h.widget(label).is_some(), "`{label}` is missing");
        }
        h.frames(4);
        h.step(&json!({"widget": "Modelled"})).unwrap();
        h.frames(1);
        h.step(&json!({"click": {"handle": "kicad:silkscreen"}})).unwrap();
        h.frames(1);
        let d = h.app.kicad_import.as_ref().unwrap();
        assert_eq!((d.copper, d.silkscreen), (KicadCopper::Mesh, false), "{d:?}");
        // Cancel leaves the design and the background queue alone.
        h.step(&json!({"click": {"handle": "kicad:cancel"}})).unwrap();
        h.frames(1);
        assert!(h.app.kicad_import.is_none() && !h.app.file_load.busy());
        assert!(h.app.session.doc.features.is_empty());

        // Import hands the switches to the command and the board comes in with copper overlay.
        let path = std::env::temp_dir().join(format!("kicad_dialog_{}.kicad_pcb", std::process::id()));
        std::fs::write(
            &path,
            r#"(kicad_pcb (version 20240108) (generator pcbnew)
  (general (thickness 1.6))
  (gr_rect (start -20 -20) (end 20 20) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts"))
  (segment (start -10 0) (end 10 0) (width 2) (layer "F.Cu") (net 0)))"#,
        )
        .unwrap();
        open(&mut h.app, &path.to_string_lossy());
        h.frames(2);
        h.step(&json!({"click": {"handle": "kicad:copper:overlay"}})).unwrap();
        h.frames(1);
        h.step(&json!({"click": {"handle": "kicad:import"}})).unwrap();
        h.settle();
        assert!(h.app.kicad_import.is_none() && !h.app.file_load.busy());
        let names: Vec<String> = h.app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        assert!(names.iter().any(|n| n == "Copper F.Cu"), "{names:?}");
        let _ = std::fs::remove_file(path);
    }
}
