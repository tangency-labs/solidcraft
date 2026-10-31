//! Asking which unit an STL or OBJ file is in. These formats carry no unit, so a file made in
//! inches would come in 25.4 times too small. When a person opens, inserts or drops one, a small
//! dialog asks for the unit first: OK imports with it (`units` of `file.insert_mesh` and
//! `doc.open`, which scale the coordinates to millimetres as the file is read), Cancel imports
//! nothing. The design keeps the millimetre mesh and not the unit, so a different unit is tried by
//! importing the file again.
//!
//! Only interactive entry points ask ([`file_load::begin`]): a script, the control channel or MCP
//! calls the commands with their own `units`, or none for millimetres, and never meets a dialog.

use serde_json::json;
use solidcraft_engine::doc::point_cloud::UNITS;

use crate::{SolveApp, file_load, ui_kit};

/// A file waiting for its unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    /// The file.
    pub path: String,
    /// Insert it into the design (else open it as a new design).
    pub insert: bool,
    /// The chosen unit: an index into the unit list, millimetres first.
    pub choice: usize,
    /// Insert it as a mesh whatever its extension says (a PLY chosen with Insert Mesh).
    pub mesh: bool,
}

/// Files that state no unit of their own and take one when imported.
pub fn needs_unit(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".stl") || lower.ends_with(".obj")
}

/// Ask for the unit of `path` before it is imported. Returns whether the file was held for the
/// answer (the caller then stops: the dialog starts the import itself). The web app asks too, for
/// the file a person uploaded, before its mesh is made.
pub fn ask(app: &mut SolveApp, path: &str, insert: bool) -> bool {
    if !needs_unit(path) {
        return false;
    }
    app.mesh_units = Some(Ask { path: path.into(), insert, choice: default_choice(), mesh: false });
    true
}

/// Insert Mesh was chosen and `path` is a PLY: ask its unit, then insert it as a mesh (its faces)
/// and not as the point cloud every other way of inserting a PLY makes. Returns whether it was
/// held for the answer; another file goes on as the extension says.
pub fn ask_ply_as_mesh(app: &mut SolveApp, path: &str) -> bool {
    if !solidcraft_engine::io::is_ply_path(path) {
        return false;
    }
    app.mesh_units = Some(Ask { path: path.into(), insert: true, choice: default_choice(), mesh: true });
    true
}

/// The unit chosen when the dialog opens: millimetres, the unit a file is read in without one.
fn default_choice() -> usize {
    UNITS.iter().position(|(name, _)| *name == "mm").unwrap_or(0)
}

/// The unit's id for `units` (`mm`, `in`, ...) at `choice`.
fn unit_at(choice: usize) -> &'static str {
    UNITS.get(choice).map_or("mm", |(name, _)| name)
}

/// The dialog, while a file waits for its unit.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut ask) = app.mesh_units.clone() else { return };
    let file = std::path::Path::new(&ask.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| ask.path.clone());
    // The symbols themselves (mm, cm, m, in, ft), as everywhere else units are chosen.
    let names: Vec<&str> = UNITS.iter().map(|(name, _)| *name).collect();
    let (closed, action) =
        ui_kit::Window::new(egui::Id::new("sc_mesh_units"), "import", crate::i18n::tr("ui.point_cloud_units")).show(ctx, true, |ui| {
            ui_kit::text(ui, &file, ui_kit::Text::Report("file", false));
            ui_kit::choice_row(ui, crate::i18n::tr("ui.units"), "mesh_units", &mut ask.choice, &names);
        });
    for (handle, key) in [("mesh_units:ok", "dialog:ok"), ("mesh_units:cancel", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    match action {
        ui_kit::Action::Ok => {
            app.mesh_units = None;
            let kind = if ask.insert { file_load::Kind::Insert } else { file_load::Kind::Open };
            let mut request = file_load::Request::new(&ask.path, kind);
            request.mesh = ask.mesh;
            request.options.insert("units".into(), json!(unit_at(ask.choice)));
            let _ = file_load::begin_request(app, request);
        }
        ui_kit::Action::Cancel => app.mesh_units = None,
        ui_kit::Action::None if closed => app.mesh_units = None,
        _ => app.mesh_units = Some(ask),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stl_and_obj_files_ask() {
        for path in ["part.stl", "PART.STL", "a/b/c.Obj", "x.obj"] {
            assert!(needs_unit(path), "{path}");
        }
        for path in ["part.3mf", "part.step", "part.glb", "part.ply", "stl", "part.stl.txt", ""] {
            assert!(!needs_unit(path), "{path}");
        }
    }

    #[test]
    fn a_ply_asks_only_when_it_is_inserted_as_a_mesh() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        // Open and Insert keep sending a PLY to the point cloud: no dialog.
        assert!(!ask(&mut app, "scan.ply", true) && !ask(&mut app, "scan.PLY", false));
        assert!(app.mesh_units.is_none());
        // Insert Mesh is explicit: the dialog, then the mesh command.
        assert!(!ask_ply_as_mesh(&mut app, "part.stl"), "only a PLY needs this route");
        assert!(ask_ply_as_mesh(&mut app, "scan.ply"));
        let held = app.mesh_units.clone().expect("held");
        assert_eq!((held.path.as_str(), held.insert, held.mesh, unit_at(held.choice)), ("scan.ply", true, true, "mm"));
    }

    #[test]
    fn millimetres_come_first_and_a_bad_choice_is_millimetres() {
        assert_eq!(unit_at(0), "mm");
        assert_eq!(unit_at(3), "in");
        assert_eq!(unit_at(99), "mm");
    }

    #[test]
    fn an_stl_waits_for_its_unit_and_the_control_channel_does_not() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        file_load::begin(&mut app, "part.stl", true);
        let ask = app.mesh_units.clone().expect("asked");
        assert_eq!((ask.path.as_str(), ask.insert, unit_at(ask.choice)), ("part.stl", true, "mm"));
        assert!(app.file_load.idle(), "nothing is imported until OK");
    }
}
