//! Pickers are host UI; the commands themselves never open a dialog. Desktop
//! parsing/writing runs off the event thread and imports retain stale-result guards.
use super::*;

enum Outcome {
    Imported(Result<Box<solidcraft_engine::ComputedCommands>, String>),
    #[cfg(not(target_arch = "wasm32"))]
    Exported(Result<serde_json::Value, String>),
}
pub(super) struct Pending {
    done: std::sync::mpsc::Receiver<Outcome>,
}
fn surface(app: &SolveApp) -> bool {
    app.forms_prototype
        .blend_surface
        .unwrap_or_else(|| app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some() || f.matching.is_some() || f.hierarchy.is_some()))
}

pub(crate) fn pick(app: &mut SolveApp, import: bool) {
    if app.forms_prototype.interchange.is_some() || app.forms_prototype.pending.is_some() {
        return;
    }
    if import {
        app.pick_open_for(&["blend"], |app, path| {
            if let Some(path) = path {
                start(app, true, path);
            }
        });
    } else {
        let evaluated = surface(app);
        app.pick_save(if evaluated { "Form-surface.blend" } else { "Form.blend" }, &["blend"], move |app, path| {
            if let Some(path) = path {
                start_export(app, path, evaluated);
            }
        });
    }
}
fn start(app: &mut SolveApp, import: bool, path: String) {
    start_command(app, import, json!({"path":path}));
}
fn start_export(app: &mut SolveApp, path: String, surface: bool) {
    let mut params = json!({"path":path,"surface":surface});
    if surface && app.session.forms_prototype.is_some() {
        match controls::commit_params(app) {
            Ok(finish) => {
                if let Some(fields) = finish.as_object() {
                    for (key, value) in fields {
                        params[key] = value.clone();
                    }
                }
            }
            Err(error) => {
                app.set_status(error, true);
                return;
            }
        }
    }
    start_command(app, false, params);
}
fn start_command(app: &mut SolveApp, import: bool, params: serde_json::Value) {
    let id = if import { "form.blend.import" } else { "form.blend.export" };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut copy = app.session.scratch();
        let given = params.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("forms-interchange".into()).spawn(move || {
            let result = if import {
                Outcome::Imported(copy.compute_commands(&[(id.into(), given)], None).map(Box::new).map_err(|e| e.to_string()))
            } else {
                Outcome::Exported(copy.execute(id, &given).map_err(|e| e.to_string()))
            };
            let _ = tx.send(result);
        });
        if spawned.is_ok() {
            app.forms_prototype.interchange = Some(Pending { done: rx });
            return;
        }
    }
    let result = app.session.execute(id, &params).map_err(|e| e.to_string());
    complete(app, id, result);
}
fn complete(app: &mut SolveApp, id: &str, result: Result<serde_json::Value, String>) {
    let warning = result
        .as_ref()
        .ok()
        .and_then(|v| v["rejected"].as_array())
        .map(|v| v.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("\n"))
        .filter(|s| !s.is_empty());
    let success = result.is_ok();
    let _ = app.command_completed(id, result);
    if success && let Some(warning) = warning {
        app.set_status(warning, true);
    }
}
pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = &app.forms_prototype.interchange else {
        return;
    };
    let outcome = match pending.done.try_recv() {
        Ok(outcome) => outcome,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
            return;
        }
        Err(_) => Outcome::Imported(Err(app.tr("forms.prototype.interchange_stopped").to_owned())),
    };
    app.forms_prototype.interchange = None;
    match outcome {
        Outcome::Imported(result) => match result.and_then(|prepared| app.session.accept_commands(*prepared).map_err(|e| e.to_string())) {
            Ok(replies) => {
                for (id, reply) in replies {
                    complete(app, &id, Ok(reply));
                }
            }
            Err(error) => app.set_status(error, true),
        },
        #[cfg(not(target_arch = "wasm32"))]
        Outcome::Exported(result) => complete(app, "form.blend.export", result),
    }
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut evaluated = surface(app);
    let r = ui.checkbox(&mut evaluated, app.tr("forms.prototype.blend_surface_mesh"));
    crate::scenario::publish_handle("forms_blend_surface_mesh", r.rect.center());
    if r.changed() {
        app.forms_prototype.blend_surface = Some(evaluated);
    }
    ui.add(egui::Label::new(app.tr(if evaluated { "forms.prototype.blend_surface_help" } else { "forms.prototype.blend_cage_help" })).wrap());
    ui.horizontal_wrapped(|ui| {
        for (import, key, handle) in
            [(true, "forms.prototype.blend_import", "forms_blend_import"), (false, "forms.prototype.blend_export", "forms_blend_export")]
        {
            let r = ui.button(app.tr(key));
            crate::scenario::publish_handle(handle, r.rect.center());
            if r.clicked() {
                pick(app, import);
            }
        }
    });
    if let Some(source) = app.session.forms_prototype.as_ref().and_then(|f| f.imported.as_ref()) {
        ui.add(egui::Label::new(&source.object).wrap());
        let file = source.file.rsplit(['\\', '/']).next().unwrap_or(&source.file);
        ui.add(egui::Label::new(egui::RichText::new(file).small()).wrap()).on_hover_text(&source.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blender_pickers_cancel_without_changing_the_editor_and_prepared_imports_keep_stale_edits() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1]})).unwrap();
        let original = app.session.forms_prototype.as_ref().unwrap().cage.clone();
        app.services.pick_open = Some(Box::new(|ext| {
            assert_eq!(ext, ["blend"]);
            Box::pin(std::future::ready(None))
        }));
        app.services.pick_save = Some(Box::new(|name, ext| {
            assert!(name.ends_with(".blend"));
            assert_eq!(ext, ["blend"]);
            Box::pin(std::future::ready(None))
        }));
        for id in ["form.blend.import", "form.blend.export"] {
            app.start(id);
            assert!(app.file_picker_busy());
            crate::file_picker::tick(&mut app, &egui::Context::default());
            assert!(!app.file_picker_busy());
            assert!(app.forms_prototype.interchange.is_none());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage, original);
        }
        let source = std::env::temp_dir().join(format!(
            "solidcraft-blender-ui-{}-{}.blend",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let cage = solidcraft_engine::geom::subdivision::primitives::Parameters { segments: [1; 3], dimensions: [6.; 3], ..Default::default() }
            .build()
            .unwrap();
        std::fs::write(
            &source,
            solidcraft_engine::io::blend::write(&[solidcraft_engine::io::blend::Object { name: "Picked cage".into(), cage, levels: 2 }]).unwrap(),
        )
        .unwrap();
        for (initial_editor, stale) in [(true, false), (true, true), (false, false)] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            if initial_editor {
                app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1]})).unwrap();
            }
            let prepared = app.session.scratch().compute_commands(&[("form.blend.import".into(), json!({"path":source}))], None).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(Outcome::Imported(Ok(Box::new(prepared)))).unwrap();
            app.forms_prototype.interchange = Some(Pending { done: rx });
            if stale {
                let p = app.session.forms_prototype.as_ref().unwrap().control_positions()[0];
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x-0.5,p.y,p.z]]})).unwrap();
            }
            app.logic(&egui::Context::default());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().imported.is_some(), !stale);
            assert!(app.session.doc.features.is_empty());
            if !stale {
                assert!(!app.ui.show_browser);
                assert!(app.cam.distance < 30.0, "a six-millimetre cage should fill the editor rather than use a ten-millimetre minimum radius");
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().imported.as_ref().unwrap().object, "Picked cage");
            }
        }
        std::fs::remove_file(source).unwrap();
    }
}
