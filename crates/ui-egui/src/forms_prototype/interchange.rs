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
    let r = crate::ui_kit::checkbox(ui, app.tr("forms.prototype.blend_surface_mesh"), &mut evaluated);
    crate::scenario::publish_handle("forms_blend_surface_mesh", r.rect.center());
    if r.changed() {
        app.forms_prototype.blend_surface = Some(evaluated);
    }
    ui.add(egui::Label::new(app.tr(if evaluated { "forms.prototype.blend_surface_help" } else { "forms.prototype.blend_cage_help" })).wrap());
    ui.horizontal_wrapped(|ui| {
        for (import, key, handle) in
            [(true, "forms.prototype.blend_import", "forms_blend_import"), (false, "forms.prototype.blend_export", "forms_blend_export")]
        {
            let r = crate::ui_kit::button(ui, app.tr(key), crate::ui_kit::Button::Normal);
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
            app.ui.show_browser = initial_editor;
            app.ui.show_timeline = !initial_editor;
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
            assert_eq!(app.ui.show_browser, initial_editor);
            assert_eq!(app.ui.show_timeline, !initial_editor);
            if !stale {
                assert!(app.cam.distance < 30.0, "a six-millimetre cage should fill the editor rather than use a ten-millimetre minimum radius");
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().imported.as_ref().unwrap().object, "Picked cage");
            }
        }
        std::fs::remove_file(source).unwrap();
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blender_buttons_route_picked_paths_through_real_workers_without_publishing_or_changing_exported_source() {
        use std::{cell::Cell, rc::Rc, task::Poll};

        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"size":10,"segments":[1,1,1]})).unwrap();
        h.frames(3);
        let original = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        let document = h.app.session.execute("document.revision", &json!({})).unwrap();
        let input = h.dir.join("picked-input.blend");
        let output = h.dir.join("picked-output.blend");
        let cage = solidcraft_engine::geom::subdivision::primitives::Parameters { segments: [1; 3], dimensions: [6.; 3], ..Default::default() }
            .build()
            .unwrap();
        std::fs::write(
            &input,
            solidcraft_engine::io::blend::write(&[solidcraft_engine::io::blend::Object {
                name: "Picked cage".into(),
                cage: cage.clone(),
                levels: 2,
            }])
            .unwrap(),
        )
        .unwrap();
        let assert_picked_cage = |actual: &solidcraft_engine::geom::subdivision::Cage| {
            assert_eq!(actual.faces, cage.faces);
            assert_eq!(actual.edge_creases, cage.edge_creases);
            assert_eq!(actual.vertex_creases, cage.vertex_creases);
            assert_eq!(actual.points.len(), cage.points.len());
            for (actual, expected) in actual.points.iter().zip(&cage.points) {
                // Blender stores single-precision coordinates and its metre scale.
                // Four relative f32 roundoffs cover both writes and reads of this fixture.
                let bound = 4.0 * f64::from(f32::EPSILON) * expected.len();
                assert!((*actual - *expected).len() <= bound, "{actual:?} != {expected:?} within {bound} mm");
            }
        };

        let answer_open = Rc::new(Cell::new(false));
        let open_calls = Rc::new(Cell::new(0));
        let open_path = input.to_string_lossy().into_owned();
        h.app.services.pick_open = Some(Box::new({
            let answer = answer_open.clone();
            let calls = open_calls.clone();
            move |extensions| {
                assert_eq!(extensions, ["blend"]);
                calls.set(calls.get() + 1);
                let answer = answer.clone();
                let path = open_path.clone();
                Box::pin(std::future::poll_fn(move |_| if answer.get() { Poll::Ready(Some(path.clone())) } else { Poll::Pending }))
            }
        }));
        let section = crate::scenario::handle_at("forms.prototype.interchange").unwrap();
        h.call("ui.scroll", json!({"x":section.x,"y":500,"delta":-1200,"unit":"point"}));
        crate::test_support::play(&mut h);
        crate::test_support::click(&mut h, "forms.prototype.interchange");
        // Expanding the last section adds content below the current scroll limit.
        h.call("ui.scroll", json!({"x":section.x,"y":500,"delta":-1200,"unit":"point"}));
        crate::test_support::play(&mut h);
        crate::test_support::click(&mut h, "forms_blend_import");
        assert_eq!(open_calls.get(), 1);
        assert!(h.app.file_picker_busy());
        assert!(h.app.forms_prototype.interchange.is_none());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), original);

        answer_open.set(true);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert!(!h.app.file_picker_busy());
        let pending = h.app.forms_prototype.interchange.take().expect("the selected .blend path must launch the real native worker");
        let result = pending.done.recv().unwrap();
        assert!(matches!(&result, Outcome::Imported(Ok(_))));
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        h.app.forms_prototype.interchange = Some(Pending { done: rx });
        tick(&mut h.app, &h.ctx);
        let imported = h.app.session.forms_prototype.as_ref().unwrap();
        assert_picked_cage(&imported.cage);
        assert_eq!(imported.levels, 2);
        assert_eq!(imported.imported.as_ref().unwrap().object, "Picked cage");
        assert!(h.app.session.doc.features.is_empty());
        assert_eq!(h.app.session.execute("document.revision", &json!({})).unwrap(), document);

        let before = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        h.app.forms_prototype.blend_surface = Some(false);
        let answer_save = Rc::new(Cell::new(false));
        let save_calls = Rc::new(Cell::new(0));
        let save_path = output.to_string_lossy().into_owned();
        h.app.services.pick_save = Some(Box::new({
            let answer = answer_save.clone();
            let calls = save_calls.clone();
            move |name, extensions| {
                assert_eq!(name, "Form.blend");
                assert_eq!(extensions, ["blend"]);
                calls.set(calls.get() + 1);
                let answer = answer.clone();
                let path = save_path.clone();
                Box::pin(std::future::poll_fn(move |_| if answer.get() { Poll::Ready(Some(path.clone())) } else { Poll::Pending }))
            }
        }));
        h.frames(3);
        h.call("ui.scroll", json!({"x":section.x,"y":500,"delta":-1200,"unit":"point"}));
        crate::test_support::play(&mut h);
        crate::test_support::click(&mut h, "forms_blend_export");
        assert_eq!(save_calls.get(), 1);
        assert!(h.app.file_picker_busy());
        assert!(!output.exists());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        answer_save.set(true);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert!(!h.app.file_picker_busy());
        let pending = h.app.forms_prototype.interchange.take().expect("the selected save path must launch the real native export worker");
        let result = pending.done.recv().unwrap();
        match &result {
            Outcome::Exported(Ok(reply)) => {
                assert_eq!(reply["surface"], false);
                assert_eq!(reply["objects"].as_array().unwrap().len(), 1);
                assert_eq!(reply["rejected"], json!([]));
            }
            _ => panic!("the rendered Export button did not run a successful native cage export"),
        }
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        h.app.forms_prototype.interchange = Some(Pending { done: rx });
        tick(&mut h.app, &h.ctx);
        let exported = solidcraft_engine::io::blend::read(&std::fs::read(&output).unwrap()).unwrap();
        assert!(exported.rejected.is_empty());
        assert_eq!(exported.objects.len(), 1);
        assert_picked_cage(&exported.objects[0].cage);
        assert_eq!(exported.objects[0].levels, 2);
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(h.app.session.execute("document.revision", &json!({})).unwrap(), document);
        assert!(h.app.session.doc.features.is_empty());
    }
}
