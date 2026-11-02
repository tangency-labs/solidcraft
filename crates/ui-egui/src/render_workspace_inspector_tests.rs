use super::*;
use crate::{scenario::Harness, theme::Tokens};
fn fixture() -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    h.app.run("solid.box", json!({"length":20,"width":15,"height":10,"body_name":"Subject"})).unwrap();
    super::super::enter(&mut h.app, true).unwrap();
    h.app.run("view.render_stage", json!({"stage":"output"})).unwrap();
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    h.frames(3);
    h
}

fn reveal_inspector_target(h: &mut Harness, target: &str) -> egui::Rect {
    for _ in 0..8 {
        let rect = crate::scenario::rect_of(target).unwrap();
        let inspector = crate::scenario::rect_of("render:inspector").unwrap();
        let top = crate::scenario::rect_of("render:inspector:header").unwrap().bottom() + Tokens::get().space_8;
        let bottom = inspector.bottom() - Tokens::get().space_8;
        if rect.top() >= top && rect.bottom() <= bottom {
            return rect;
        }
        let at = rect.center();
        let delta = at.y.clamp(top + rect.height() / 2.0, bottom - rect.height() / 2.0) - at.y;
        h.call("ui.scroll", json!({"x":inspector.center().x,"y":(top+bottom)/2.0,"delta":delta}));
        h.frames(20);
    }
    panic!("inspector target {target} is still clipped");
}

fn ordered_sections(keys: &[&str], app: &SolveApp) {
    let mut bottom = f32::NEG_INFINITY;
    for key in keys {
        let rect = crate::scenario::rect_of(&format!("dialog:section:{}", app.tr(key))).unwrap();
        assert!(rect.top() >= bottom, "{key} must follow the previous section");
        bottom = rect.bottom();
    }
}

#[test]
fn render_workspace_shot_and_scene_widgets_fit_inspector_content_width() {
    let mut h = fixture();
    setups::save(&mut h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    super::super::shot::choose_aspect(&mut h.app, &name, 0).unwrap();
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(-18.0, 125.5, -18.0);
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    for width in [350.0, 320.0] {
        for editor in ["scene", "raster", "cpu_raytrace", "gpu_path", "orthographic"] {
            if editor != "scene" {
                h.app.cam.fov = if editor == "orthographic" { 0.0 } else { 0.6 };
                let job = if editor == "orthographic" { "raster" } else { editor };
                let mut job = json!({"method":job});
                if editor == "gpu_path" {
                    job["options"] = json!({"lens":h.app.render_workspace.view.studio.lens});
                }
                let patch = json!({"job":job,"camera":h.app.cam});
                super::super::shot::edit_settings(&mut h.app, &name, patch).unwrap();
                h.app.cam.yaw += 0.1; // Include the changed-camera banner and enabled Update/Revert.
            }
            let before = h.app.session.doc.to_json();
            let content = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(width, 1600.0));
            let mut widgets = Vec::new();
            for frame in 0..3 {
                crate::scenario::clear_handles();
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(content.expand(8.0)), ..Default::default() }, |ui| {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                        ui.painter().rect_filled(content, 0.0, crate::theme::Tokens::get().dialog_body);
                        ui_kit::scope(ui, |ui| {
                            let preceding: std::collections::HashSet<_> = ctx.viewport(|vp| {
                                vp.this_pass.widgets.layers().flat_map(|(_, widgets)| widgets.iter().map(|widget| widget.id)).collect()
                            });
                            if editor == "scene" {
                                scene_controls(&mut h.app, ui);
                            } else {
                                super::super::shot::editor(&mut h.app, ui, &name);
                            }
                            widgets = ctx.viewport(|vp| {
                                vp.this_pass
                                    .widgets
                                    .layers()
                                    .flat_map(|(_, widgets)| widgets.iter().filter(|widget| !preceding.contains(&widget.id)).copied())
                                    .collect()
                            });
                        });
                    });
                });
                ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && [h.app.tr("render.perspective"), h.app.tr("render.stage_orthographic"), h.app.tr("render.shot_distance")]
                            .contains(&text.galley.text())
                    {
                        assert!(!text.galley.elided, "{editor} at {width} pt: {} is truncated", text.galley.text());
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(shape.clip_rect.expand(1.0).contains_rect(rect), "{} is clipped: {rect:?}", text.galley.text());
                    }
                }
                if frame == 2
                    && width == 350.0
                    && let Some(dir) = std::env::var_os("SOLIDCRAFT_SHOT_LAYOUT_EVIDENCE")
                {
                    std::fs::create_dir_all(&dir).unwrap();
                    ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, content, &textures)
                        .save(std::path::Path::new(&dir).join(format!("{editor}.png")))
                        .unwrap();
                }
            }
            assert!(widgets.len() > 10, "must inspect actual editor widgets");
            for widget in widgets {
                assert!(
                    widget.rect.left() >= content.left() - 0.5 && widget.rect.right() <= content.right() + 0.5,
                    "{editor} at {width} pt: widget {:?} extends outside {content:?}: {:?}",
                    widget.id,
                    widget.rect
                );
            }
            if editor != "scene" {
                let cells = crate::scenario::rects(&format!("dialog:widget:{}", h.app.tr("render.shot_target")));
                assert_eq!(cells.len(), 3);
                assert!(cells.windows(2).all(|pair| (pair[0].1.width() - pair[1].1.width()).abs() < 0.5));
            }
            assert_eq!(h.app.session.doc.to_json(), before, "layout must not edit saved intent");
        }
    }
}

#[test]
fn render_workspace_scene_overview_selection_order_links_and_readiness_are_read_only() {
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":1600}));
    setups::save(&mut h.app).unwrap();
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.frames(3);
    assert_eq!(selected_editor(&h.app), Editor::Scene);
    ordered_sections(&["render.scene_stages", "render.scene_ready", "render.scene_reminders"], &h.app);
    let mut bottom = f32::NEG_INFINITY;
    for id in ["look", "light", "frame", "output"] {
        let rect = crate::scenario::rect_of(&format!("render:scene:{id}")).unwrap();
        assert!(rect.top() >= bottom);
        bottom = rect.bottom();
    }
    for id in ["open-check", "check", "render", "all-shots"] {
        assert!(crate::scenario::rect_of(&format!("render:scene:{id}")).is_some());
    }
    assert!(crate::scenario::rect_of("dialog:note:render.scene_batch_reason").is_some());
    h.step(&json!({"click":{"handle":"render:scene:frame"}})).unwrap();
    assert!(matches!(h.app.render_workspace.view.selection, super::super::RenderSelection::Shot(_)));
    assert_eq!(selected_editor(&h.app), Editor::Camera);
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:scene:output"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Output);
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:scene:open-check"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Check);
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Check);
    assert!(crate::scenario::rect_of("render:check").is_some());
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.render_workspace.view.job.is_none());
}

#[test]
fn render_workspace_shot_e7_order_disabled_reasons_output_link_and_confirm_remove() {
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    setups::save(&mut h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    h.app.run("view.render_select", json!({"kind":"shot","id":name})).unwrap();
    let before = h.app.session.doc.to_json();
    h.frames(3);
    assert_eq!(selected_editor(&h.app), Editor::Camera);
    assert!(crate::scenario::rect_of("render:job-footer").is_none(), "Shot owns its actions");
    assert!(crate::scenario::rect_of("dialog:editor_footer").is_none(), "Appearance footer stays in Appearance");
    let inspector = crate::scenario::rect_of("render:inspector").unwrap();
    let header = crate::scenario::rect_of("render:inspector:header").unwrap();
    assert!(inspector.contains_rect(header));
    assert!(h.widget(&name).is_some(), "selected Shot names the inspector");
    ordered_sections(&["render.page_camera", "render.shot_lens", "render.shot_exposure_section", "render.stage_frame"], &h.app);
    for key in ["render.shot_lens_gpu_reason", "render.shot_exposure_reason", "render.shot_selection_reason"] {
        assert!(crate::scenario::rect_of(&format!("dialog:note:{key}")).is_some(), "{key}");
    }
    for id in ["name", "update", "revert", "duplicate", "remove", "output", "distance", "look-selection"] {
        assert!(crate::scenario::rect_of(&format!("render:shot:{id}")).is_some(), "{id}");
    }
    assert!(crate::scenario::rect_of("render:shot:width").is_none(), "size belongs in Output");
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":"render:shot:remove"}})).unwrap();
    assert!(confirmation_open(&h.app));
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(!confirmation_open(&h.app));
    assert_eq!(h.app.session.doc.to_json(), before, "cancelling removal keeps the Shot");
    h.step(&json!({"click":{"handle":"render:shot:output"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Output);
    assert_eq!(selected_editor(&h.app), Editor::Output);
}

#[test]
fn render_workspace_shot_camera_controls_save_undo_and_refuse_hostile_values() {
    let mut h = fixture();
    setups::save(&mut h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    let before = h.app.session.doc.to_json();
    let camera = solidcraft_engine::render::Camera { yaw: 0.7, roll: 0.2, distance: 123.0, ..h.app.cam };
    super::super::shot::set_camera(&mut h.app, &name, camera).unwrap();
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.doc.render.setups[&name].camera, camera);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, f64::MAX] {
        let camera = solidcraft_engine::render::Camera { distance: value, ..camera };
        assert!(super::super::shot::set_camera(&mut h.app, &name, camera).is_err());
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

#[test]
fn render_workspace_shot_lens_saves_only_lens_and_orthographic_keeps_faded_controls() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    let name = "Lens shot";
    let camera = solidcraft_engine::render::Camera { fov: 0.6, ..h.app.cam };
    h.app
        .run("render.settings.set", json!({"name":name,"set":{"camera":camera,"job":{"method":"gpu_path","options":{"samples":128,"seed":42}}}}))
        .unwrap();
    h.app.run("render.camera.set", json!({"camera":camera})).unwrap();
    h.app.follow_view_request(false);
    h.app.run("view.render_select", json!({"kind":"shot","id":name})).unwrap();
    let before = h.app.session.doc.render.setups[name].clone();
    let undo = h.app.session.undo.len();
    h.frames(3);
    h.step(&json!({"widget":h.app.tr("render.shot_dof")})).unwrap();
    let mut expected = before.clone();
    if let Some(RenderJob::GpuPath { options, .. }) = &mut expected.job {
        options.lens = Some(h.app.render_workspace.view.studio.lens);
    }
    assert_eq!(h.app.session.doc.render.setups[name], expected);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.render.setups[name], before);
    h.app.cam.fov = 0.0;
    h.frames(3);
    for key in ["render.shot_fov_reason", "render.shot_lens_perspective_reason"] {
        assert!(crate::scenario::rect_of(&format!("dialog:note:{key}")).is_some(), "{key}");
    }
    assert_eq!(h.app.session.doc.render.setups[name], before, "opening must not change saved intent or method");
}
#[test]
fn inspector_dispatch_has_one_editor_per_selection_independent_of_every_stage() {
    use super::super::{RenderSelection, Stage};
    for dark in [false, true] {
        for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
            let mut h = fixture();
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.app.ui.layout = layout;
            h.call("ui.resize", json!({"width":1440,"height":1000}));
            super::super::select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
            super::super::light::edit(&mut h.app, "render.lights.add", None, Some(json!({"kind":"point"}))).unwrap();
            let light = super::super::light::rows(&h.app).unwrap().last().unwrap().id.clone();
            let shot = h.app.render_workspace.view.name.clone();
            let before = h.app.session.doc.to_json();
            let undo = (h.app.session.undo.len(), h.app.session.redo.len());
            for (selection, kind) in [
                (RenderSelection::Nothing, "scene"),
                (RenderSelection::Object(vec![solidcraft_engine::Sel::Body { name: "Subject".into() }]), "appearance"),
                (RenderSelection::Environment, "environment"),
                (RenderSelection::Lighting, "lighting"),
                (RenderSelection::Light(light.clone()), "light"),
                (RenderSelection::Shot(shot.clone()), "shot"),
                (RenderSelection::Output, "output"),
                (RenderSelection::Sampling, "sampling"),
                (RenderSelection::Effects, "effects"),
                (RenderSelection::Check, "check"),
                (RenderSelection::Wear, "wear"),
            ] {
                if let RenderSelection::Object(_) = &selection {
                    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: "Subject".into() }]})).unwrap();
                } else {
                    let mut params = json!({"kind":if kind == "scene" { "nothing" } else { kind }});
                    if let RenderSelection::Light(id) | RenderSelection::Shot(id) = &selection {
                        params["id"] = json!(id);
                    }
                    h.app.run("view.render_select", params).unwrap();
                }
                assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), undo);
                let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
                for stage in Stage::ALL {
                    h.app.run("view.render_stage", json!({"stage":stage.key()})).unwrap();
                    h.frames(3);
                    assert_eq!(h.app.render_workspace.view.selection, selection);
                    let editors = crate::scenario::rects("render:editor:");
                    assert_eq!(editors.len(), 1, "{kind}/{stage:?}: {editors:?}");
                    assert!(crate::scenario::rect_of(&format!("render:editor:{kind}")).is_some());
                    assert!(crate::scenario::rect_of("render:job-footer").is_none());
                    assert!(crate::scenario::rect_of("render:setups").is_none());
                    assert!(crate::scenario::rect_of("render:setup-name").is_none());
                    assert_eq!(crate::scenario::rect_of("render:output-footer").is_some(), kind == "output");
                    assert_eq!(crate::scenario::rect_of("dialog:editor_footer").is_some(), kind == "appearance");
                    assert!(h.widget(h.app.tr("render.studio_title")).is_none());
                    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
                    assert_eq!(h.app.session.doc.to_json(), before);
                    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
                    assert!(h.app.render_workspace.view.job.is_none());
                    assert!(h.app.render_workspace.view.completed.is_none());
                }
                if !dark && layout == crate::shell::LayoutMode::Floating {
                    appearance_inspector_capture(&mut h, &format!("dispatch-{kind}"));
                }
            }
        }
    }
}

#[test]
fn stage_navigation_is_local_and_never_switches_executor_or_overwrites_drafts() {
    let mut h = fixture();
    let before = h.app.session.doc.to_json();
    h.app.render_workspace.view.samples = 37;
    h.app.render_workspace.view.draft.width = 777;
    for stage in super::super::Stage::ALL {
        h.step(&json!({"click":{"handle":format!("render:stage:{}",stage.key())}})).unwrap();
        assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Output);
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
        assert_eq!(h.app.render_workspace.view.samples, 37);
        assert_eq!(h.app.render_workspace.view.draft.width, 777);
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

#[test]
fn numerical_editor_saves_before_switch_and_keeps_navigation_camera_explicit() {
    let mut h = fixture();
    h.app.render_workspace.view.name = "A".into();
    setups::save(&mut h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"B","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
    let before = setups::draft(&h.app).unwrap();
    let camera = before.camera;
    h.app.render_workspace.view.draft.width = 123;
    h.app.cam.yaw += 0.25;
    setups::persist_editor(&mut h.app, &before).unwrap();
    assert_eq!(h.app.session.doc.render.setups["A"].width, 123);
    assert_eq!(h.app.session.doc.render.setups["A"].camera, camera);
    request_action(&mut h.app, PendingAction::Switch("B".into()));
    assert!(confirmation_open(&h.app), "persisting width must not silently discard the camera draft");
    assert_eq!(h.app.render_workspace.view.name, "A");
    h.frames(3);
    h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "B");
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Shot("B".into()));
    assert_eq!(h.app.render_workspace.view.draft.width, 321);
    assert_eq!(h.app.session.doc.render.setups["A"].width, 123);
}

#[test]
fn shot_switch_confirms_transient_inputs_and_current_shot_focus_preserves_them() {
    use solidcraft_engine::render::job::ReflectionMode;
    for kind in ["glass", "maps", "cpu", "camera"] {
        let mut h = fixture();
        h.app.render_workspace.view.name = "A".into();
        super::super::select_image_method(&mut h.app, if kind == "cpu" { ImageMethod::CpuRaytrace } else { ImageMethod::GpuRaytrace });
        setups::save(&mut h.app).unwrap();
        h.app.run("render.settings.set", json!({"name":"B","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
        setups::load(&mut h.app);
        if kind == "cpu" {
            super::super::cpu::set_reflection(&mut h.app, "Subject", Some(ReflectionMode::Mirror)).unwrap();
        } else if kind == "camera" {
            h.app.cam.yaw += 0.25;
        } else {
            let target = materials::read_targets(&mut h.app).unwrap().remove(0);
            let mut binding = materials::Binding::new(target, &h.app);
            binding.glass_enabled = kind == "glass";
            binding.maps_enabled = kind == "maps";
            h.app.render_workspace.view.materials.enabled = true;
            h.app.render_workspace.view.materials.bindings.push(binding);
        }
        let camera = h.app.cam;
        let runtime = format!("{:?}", h.app.render_workspace.view.materials);
        let reflections = h.app.render_workspace.view.raytrace.reflections.clone();
        let doc = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        switch_shot(&mut h.app, "A".into());
        assert!(!confirmation_open(&h.app), "focusing the current shot must not reload {kind}");
        assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Shot("A".into()));
        assert_eq!(format!("{:?}", h.app.render_workspace.view.materials), runtime);
        assert_eq!(h.app.render_workspace.view.raytrace.reflections, reflections);
        assert_eq!(h.app.cam, camera);
        switch_shot(&mut h.app, "B".into());
        h.frames(3);
        assert!(confirmation_open(&h.app), "unsaved {kind} must be reviewed");
        assert_eq!(h.app.render_workspace.view.name, "A");
        h.step(&json!({"widget":h.app.tr("render.keep_editing")})).unwrap();
        assert!(!confirmation_open(&h.app));
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert_eq!(format!("{:?}", h.app.render_workspace.view.materials), runtime);
        assert_eq!(h.app.render_workspace.view.raytrace.reflections, reflections);
        assert_eq!(h.app.cam, camera);
        switch_shot(&mut h.app, "B".into());
        h.frames(3);
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
        assert_eq!(h.app.render_workspace.view.name, "B");
        assert_eq!(h.app.render_workspace.view.draft.width, 321);
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    }
}

#[test]
fn expired_confirmation_cannot_remove_a_changed_shot() {
    let mut h = fixture();
    setups::save(&mut h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    request_action(&mut h.app, PendingAction::Remove(name.clone()));
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"body_name":"Later"})).unwrap();
    h.frames(3);
    assert!(h.app.render_workspace.view.inspector.pending.is_none());
    assert!(h.app.session.doc.render.setups.contains_key(&name));
}
#[test]
fn projected_quality_changes_only_requested_budget_fields_and_has_no_runtime_claim() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.cam.fov = 0.6;
    h.app.render_workspace.view.studio.intensity = 0.42;
    h.app.render_workspace.view.draft.exposure_ev = 1.23;
    h.app.render_workspace.view.studio.ambient = [0.11, 0.22, 0.33];
    let before = h.app.session.doc.to_json();
    let camera = h.app.cam;
    let plan = h.app.run("render.quality.prepare", json!({"id":"Q-002","variant":"fixed_samples_png","acknowledge_omissions":true})).unwrap();
    apply_quality(&mut h.app, &plan).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1600, 900));
    assert_eq!(h.app.render_workspace.view.samples, 512);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.studio.intensity, 0.42);
    assert_eq!(h.app.render_workspace.view.studio.ambient, [0.11, 0.22, 0.33]);
    assert_eq!(h.app.render_workspace.view.draft.exposure_ev, 1.23);
    assert_eq!(h.app.session.doc.to_json(), before);
    super::super::select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert!(apply_quality(&mut h.app, &plan).is_err());
}
#[test]
fn complete_native_studio_can_be_reviewed_and_applied_from_real_ui_controls() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    setups::save(&mut h.app).unwrap();
    library::open_category(&mut h.app, "native_scenes");
    h.frames(4);
    h.step(&json!({"click":{"handle":"render:light:NATIVE-STUDIO-002"}})).unwrap();
    let before = h.app.session.doc.to_json();
    h.step(&json!({"click":{"handle":"render:light:review"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.render_workspace.view.library.review.is_some());
    h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(!h.app.render_workspace.view.draft.stage.is_empty());
    assert!(h.app.render_workspace.view.library.review.is_none());
    assert!(!h.app.render_workspace.view.dirty);
}

#[test]
fn surface_stage_contains_preview_target_plane_repeat_and_method_reason_in_order() {
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name:"Subject".into() }]})).unwrap();
    library::open_category(&mut h.app, "surfaces");
    h.frames(3);
    let before = h.app.session.doc.to_json();
    h.step(&json!({"click":{"handle":"render:library:SURF-001"}})).unwrap();
    h.frames(3);
    let note = crate::scenario::rect_of("dialog:note:surface:note").unwrap();
    let target = crate::scenario::rect_of("render:surface:target").unwrap();
    let plane = crate::scenario::rect_of("render:surface:xy").unwrap();
    let repeat = crate::scenario::rect_of("render:surface:repeat").unwrap();
    let reason = crate::scenario::rect_of("dialog:note:surface:gpu:reason").unwrap();
    assert!(note.bottom() < target.top());
    assert!(target.bottom() <= plane.top());
    assert!(plane.bottom() <= repeat.top());
    assert!(repeat.bottom() < reason.top());
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn stage_catalogs_fill_a_290_by_530_panel_and_keep_two_line_labels_inside_tiles() {
    use crate::{scenario, ui_kit};
    use egui::{Pos2, RawInput, Rect, vec2};
    let previous_theme = crate::theme::is_dark();
    for dark in [false, true] {
        for (kind, selected, method, key) in [
            ("appearances", None, ImageMethod::Raster, "render:library:catalog"),
            ("surfaces", None, ImageMethod::Raster, "render:surface:catalog"),
            ("surfaces", Some("SURF-001"), ImageMethod::Raster, "render:surface:catalog"),
            ("physical_profiles", Some("steel-304-plate"), ImageMethod::Raster, "render:surface:catalog"),
            ("native_scenes", None, ImageMethod::Raster, "render:light:catalog"),
            ("native_scenes", Some("NATIVE-STUDIO-002"), ImageMethod::GpuRaytrace, "render:light:catalog"),
        ] {
            let mut h = fixture();
            // Capture the font atlas from its first upload along with panel textures.
            h.ctx = crate::fonts::test_context();
            crate::theme::set_dark(dark);
            h.ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            // A resizable 290 pt card needs the floating, wide-screen policy.
            h.app.ui.layout = crate::shell::LayoutMode::Floating;
            h.app.preferences.render_browser_width = 290.0;
            h.app.preferences.render_browser_side = "left".into();
            super::super::select_image_method(&mut h.app, method);
            library::open_category(&mut h.app, kind);
            crate::theme::TabAccent::publish(&h.app, &h.ctx);
            let document = h.app.session.doc.to_json();
            let history = (h.app.session.undo.len(), h.app.session.redo.len());
            // Panel::workspace reserves an 8 pt bottom gap in these bounds.
            let bounds = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 538.0));
            let mut textures = std::collections::HashMap::new();
            let mut output = None;
            let mut unselected_height = 0.0;
            for pass in 0..8 {
                if pass == 4 {
                    unselected_height = scenario::rect_of(&format!("{key}:body")).unwrap().height();
                    if let Some(id) = selected {
                        let mut view = std::mem::take(&mut h.app.render_workspace.view.library);
                        library::choose(&mut h.app, &mut view, kind, id).unwrap();
                        h.app.render_workspace.view.library = view;
                    }
                }
                let mut frame = h.ctx.run_ui(RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                    scenario::clear_handles();
                    library::card(&mut h.app, ui, bounds);
                });
                ui_kit::test_textures_update(&mut textures, &frame.textures_delta);
                frame.textures_delta.clear();
                output = Some(frame);
            }
            let panel = h.app.shell.workspace_panels.get("render:appearances").unwrap().0;
            assert!((panel.width() - 290.0).abs() < 2.0 && (panel.height() - 530.0).abs() < 2.0, "{panel:?}");
            let content = scenario::rect_of(&format!("{key}:content")).unwrap();
            let body = scenario::rect_of(&format!("{key}:body")).unwrap();
            let footer = scenario::rect_of(&format!("{key}:footer")).unwrap();
            assert!(body.height() >= 0.7 * (content.height() - footer.height()), "{kind} {selected:?}: {content:?} {body:?} {footer:?}");
            assert!(panel.contains_rect(body) && panel.contains_rect(footer), "{kind}: {panel:?} {body:?} {footer:?}");
            assert!(body.bottom() <= footer.top() + 0.1, "{kind}: overlapping body and footer");
            assert!((content.bottom() - footer.bottom()).abs() <= 1.0, "{kind}: footer is not at the bottom");
            if key == "render:surface:catalog" {
                let row = scenario::rect_of("render:library:SURF-001").or_else(|| scenario::rect_of("render:library:steel-304-plate")).unwrap();
                assert!(body.height() >= row.height() * 3.0, "three complete list rows must remain visible");
                if selected.is_some() {
                    assert!(body.height() < unselected_height, "selecting a resource must make room for its detail card");
                }
            } else {
                let labels: Vec<_> = scenario::rects("render:").into_iter().filter(|(name, _)| name.ends_with(":galley")).collect();
                assert!(labels.len() >= 3, "{kind}: missing tile captions");
                for (name, galley) in labels {
                    let id = name.strip_suffix(":galley").unwrap();
                    let tile = scenario::rect_of(id).unwrap();
                    let label = scenario::rect_of(&format!("{id}:label")).unwrap();
                    assert!(tile.contains_rect(label) && label.expand(0.1).contains_rect(galley), "{name}: {tile:?} {label:?} {galley:?}");
                }
                let frame = output.as_ref().unwrap();
                let captions: Vec<_> = crate::test_support::text_shapes(frame).filter(|text| text.galley.job.wrap.max_rows == 2).collect();
                assert!(captions.iter().any(|text| text.galley.rows.len() == 2), "{kind}: exercise a two-line caption");
                assert!(captions.iter().all(|text| text.galley.rows.len() <= 2 && !text.galley.job.wrap.break_anywhere));
                let note = h.app.tr(if kind == "appearances" { "render.stage_layout.appearance_note" } else { "render.stage_layout.studio_note" });
                let note = crate::test_support::text_shapes(frame).find(|text| text.galley.job.text == note).unwrap();
                assert_eq!(note.galley.rows.len(), 1, "catalog notes must stay on one line");
            }
            assert_eq!(h.app.session.doc.to_json(), document);
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
            if let Some(dir) = std::env::var_os("SOLIDCRAFT_STAGE_LAYOUT_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                let frame = output.unwrap();
                let image = ui_kit::test_raster(&h.ctx, frame.shapes, frame.pixels_per_point, panel, &textures);
                image
                    .save(std::path::Path::new(&dir).join(format!(
                        "{kind}-{}-{}-{}.png",
                        selected.unwrap_or("none"),
                        method.label(),
                        if dark { "dark" } else { "light" }
                    )))
                    .unwrap();
            }
        }
    }
    crate::theme::set_dark(previous_theme);
}

#[test]
fn review_expires_when_the_reviewed_settings_or_document_change() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    library::review::quality(&mut h.app, "Q-002").unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
    h.app.render_workspace.view.draft.width = 333;
    h.frames(3);
    assert!(crate::scenario::rect_of("dialog:report:review:stale").is_some());
    let before = h.app.session.doc.to_json();
    let ok = crate::scenario::rect_of("dialog:ok").unwrap().center();
    h.step(&json!({"click":[ok.x,ok.y]})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.render_workspace.view.draft.width, 333);
    assert!(h.app.render_workspace.view.library.review.is_some());
}

#[test]
fn quality_review_is_callable_by_command_and_saves_only_after_acknowledgement() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let mut view = library::View::default();
    library::choose(&mut h.app, &mut view, "quality", "Q-002").unwrap();
    h.app.render_workspace.view.library = view;
    let before = h.app.session.doc.to_json();
    h.app.start("render.quality.prepare");
    h.frames(3);
    assert!(h.app.render_workspace.view.library.review.is_some());
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    let saved = &h.app.session.doc.render.setups[&h.app.render_workspace.view.name];
    assert_eq!((saved.width, saved.height), (1600, 900));
    assert_eq!(h.app.render_workspace.view.samples, 512);
    assert!(h.app.render_workspace.view.library.review.is_none());
}

#[test]
fn physical_review_is_read_only_and_can_be_closed_after_acknowledgement() {
    let mut h = fixture();
    let before = h.app.session.doc.to_json();
    let plan = h.app.run("render.library.physical.prepare", json!({"id":"steel-304-plate","name":"Reference","acknowledge_reference":true})).unwrap();
    library::review::open(&mut h.app, "physical", "304 steel".into(), plan.clone(), library::review::Apply::Physical(plan)).unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of("dialog:note:review:physical").is_some());
    assert!(crate::scenario::rect_of("dialog:apply").is_some());
    h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.session.doc.user_materials.is_empty());
}

#[test]
fn quality_review_rejects_hostile_ids_and_preserves_the_design() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let before = h.app.session.doc.to_json();
    for id in ["", "../Q-002", "\0", "Q-999999999999999999999999999"] {
        assert!(library::review::quality(&mut h.app, id).is_err());
    }
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.render_workspace.view.library.review.is_none());
}

#[test]
fn saved_optics_removal_requires_confirmation_and_keeps_geometry() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    setups::save(&mut h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    h.app
        .run(
            "render.optics.bind",
            json!({"name":name,"body_index":0,"body_name":"Subject", "expected_revision":h.app.session.revision,"optics":{"ior":1.4}}),
        )
        .unwrap();
    setups::load(&mut h.app);
    let target = h.app.render_workspace.view.draft.optics.iter().next().unwrap().target.clone();
    let before = h.app.session.doc.to_json();
    let geometry = h.app.session.doc.features.clone();
    let revision = h.app.session.revision;
    library::review::remove(&mut h.app, "render.optics.unbind", json!({"name":name,"target":target,"expected_revision":revision}));
    h.frames(3);
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    library::review::remove(&mut h.app, "render.optics.unbind", json!({"name":name,"target":target,"expected_revision":revision}));
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.session.doc.render.setups[&name].optics.is_empty());
    assert_eq!(h.app.session.doc.features, geometry);
}

#[test]
fn temporary_maps_review_keeps_plane_scale_and_requires_acknowledgement() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let plan = h
        .app
        .run("render.library.surface.prepare", json!({"id":"SURF-001","body_index":target.ordinal,"body_name":target.name,"plane":"yz","scale":2.5}))
        .unwrap();
    let before = h.app.session.doc.to_json();
    library::review::open(
        &mut h.app,
        "maps",
        "Brushed stainless".into(),
        plan,
        library::review::Apply::Maps { id: "SURF-001".into(), target, plane: "yz".into(), scale: 2.5 },
    )
    .unwrap();
    h.frames(3);
    assert!(h.app.render_workspace.view.materials.bindings.is_empty());
    h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    let binding = &h.app.render_workspace.view.materials.bindings[0];
    assert_eq!(binding.maps.frame.u, [0.0, 250.0, 0.0]);
    assert_eq!(binding.maps.frame.v, [0.0, 0.0, 250.0]);
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn rig_review_contains_fit_and_consent_controls_in_design_order_without_applying() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let mut view = library::View::default();
    library::choose(&mut h.app, &mut view, "rigs", "RIG-002").unwrap();
    let before = h.app.session.doc.to_json();
    library::review::light(&mut h.app, &view, "RIG-002").unwrap();
    h.frames(3);
    let mut previous = crate::scenario::rect_of("review:sphere").unwrap().bottom();
    for key in ["render:review:center:0", "render:review:extent", "render:review:bounds", "render:review:emitters-only", "render:review:acknowledge"]
    {
        let rect = crate::scenario::rect_of(key).unwrap();
        assert!(rect.top() >= previous, "{key} is out of design order");
        previous = rect.bottom();
    }
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(!h.app.render_workspace.view.library.review.as_ref().unwrap().acknowledged);
}

#[test]
fn returning_between_look_and_light_restores_catalogs_for_the_selected_scope() {
    let mut h = fixture();
    let catalog = h.app.run("appearance.library", json!({})).unwrap();
    let id = catalog["appearances"].as_array().unwrap().iter().find_map(|entry| entry["id"].as_str()).unwrap().to_string();
    let before = h.app.session.doc.to_json();
    library::open_category(&mut h.app, "native_scenes");
    h.frames(3);
    assert!(crate::scenario::rect_of("render:light:NATIVE-STUDIO-001").is_some());
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of(&format!("render:library:{id}")).is_some());
    library::open_category(&mut h.app, "scenes");
    h.frames(3);
    assert!(crate::scenario::rect_of("render:light:SCENE-001").is_some());
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    h.app.run("view.render_stage", json!({"stage":"light"})).unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of("render:light:SCENE-001").is_some());
    assert!(crate::scenario::rect_of("dialog:note:render:light:reason").is_some());
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn surface_target_dropdown_keeps_the_chosen_body_through_review() {
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Other"})).unwrap();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name:"Subject".into() }]})).unwrap();
    library::open_category(&mut h.app, "surfaces");
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:library:SURF-001"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:surface:target"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:list:Other"}})).unwrap();
    h.frames(3);
    h.step(&json!({"widget":h.app.tr("render.stage_panels.use_uniform")})).unwrap();
    h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.session.doc.appearances.bodies.contains_key("Other"));
    assert!(!h.app.session.doc.appearances.bodies.contains_key("Subject"));
}
#[test]
fn unsupported_output_is_visible_and_never_fakes_export_or_clamps_intent() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    select_editor(&mut h.app, Editor::Output);
    h.app.render_workspace.view.draft.width = 7000;
    h.app.render_workspace.view.draft.height = 6000;
    h.app.render_workspace.view.samples = 37;
    h.frames(4);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (7000, 6000));
    assert_eq!(h.app.render_workspace.view.samples, 37);
    assert!(super::super::image_request(&h.app).is_err());
    assert!(h.app.render_workspace.view.completed.is_none());
    assert!(h.app.tr("render.output_format_status").contains("EXR"));
}

#[test]
fn rational_resolution_lock_preserves_anchor_rounding_and_rejects_without_partial_changes() {
    let mut edit = Resolution { locked: true, ..Default::default() };
    let mut size = [3840, 2160];
    size = edit.edit(size, 0, 1920, 8192).unwrap();
    assert_eq!(size, [1920, 1080]);
    size = edit.edit(size, 1, 720, 8192).unwrap();
    assert_eq!(size, [1280, 720]);
    let anchor = edit.anchor;
    for _ in 0..100 {
        size = edit.edit(size, 0, 1001, 8192).unwrap();
        assert_eq!(size, [1001, 563]);
        size = edit.edit(size, 0, 1920, 8192).unwrap();
        assert_eq!(size, [1920, 1080]);
    }
    assert_eq!(edit.anchor, anchor, "rounded edits must not drift the anchor");
    assert!(edit.edit(size, 1, 8192, 8192).is_none());
    assert_eq!(edit.observed, Some(size), "a refused linked dimension must not partly change size");
    assert!(edit.rejected);
    assert!(edit.edit(size, 0, 0, 8192).is_none());
    assert!(edit.edit(size, 2, 100, 8192).is_none());
    let mut extreme = Resolution { locked: true, ..Default::default() };
    assert!(extreme.edit([1, u32::MAX], 0, u32::MAX, u32::MAX).is_none());
    assert!(Resolution { locked: true, ..Default::default() }.edit([0, 1], 0, 1, 8192).is_none());
}

#[test]
fn resolution_lock_reanchors_for_explicit_presets_and_does_not_affect_other_intent() {
    let mut edit = Resolution { locked: true, ..Default::default() };
    assert_eq!(edit.edit([3840, 2160], 0, 1920, 8192), Some([1920, 1080]));
    edit.observe([1080, 1920]);
    assert_eq!(edit.edit([1080, 1920], 1, 960, 8192), Some([540, 960]));
    edit.observe([1000, 1000]);
    assert_eq!(edit.edit([1000, 1000], 0, 777, 8192), Some([777, 777]));
    edit.locked = false;
    assert_eq!(edit.edit([777, 777], 0, 555, 8192), Some([555, 777]));
    edit.locked = true;
    assert_eq!(edit.edit([555, 777], 0, 1110, 8192), Some([1110, 1554]));
}

#[test]
fn actual_output_ratio_is_local_and_orientation_saves_once_preserving_other_capture_fields() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.draft.width = 1920;
    h.app.render_workspace.view.draft.height = 1080;
    h.app.render_workspace.view.samples = 37;
    select_editor(&mut h.app, Editor::Output);
    setups::save(&mut h.app).unwrap();
    let saved = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    reveal(&mut h.app, "render_output_advanced");
    h.frames(24);
    h.step(&json!({"click":{"handle":"render:aspect-lock"}})).unwrap();
    assert!(h.app.render_workspace.view.inspector.resolution.locked);
    assert!(!h.app.render_workspace.view.dirty, "coupling editor coordinates is not a saved numerical edit");
    assert!(Arc::ptr_eq(&saved, &h.app.session.doc));
    assert_eq!(undo, h.app.session.undo.len());
    h.step(&json!({"click":{"handle":"render:swap-size"}})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1080, 1920));
    assert_eq!(h.app.render_workspace.view.samples, 37);
    assert_eq!(h.app.cam, camera);
    let mut expected = saved.render.setups[&h.app.render_workspace.view.name].clone();
    expected.width = 1080;
    expected.height = 1920;
    assert_eq!(h.app.session.doc.render.setups[&h.app.render_workspace.view.name], expected);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!h.app.render_workspace.view.dirty, "the numeric edit is already saved");
    setups::load(&mut h.app);
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.inspector.resolution.anchor, Some([1080, 1920]));
}

#[test]
fn contextual_scope_notes_wrap_with_native_tokens_instead_of_clipping() {
    let mut h = fixture();
    select_editor(&mut h.app, Editor::Scene);
    h.frames(3);
    let rect = crate::scenario::rect_of("dialog:note:render.scene_preserves_design").unwrap();
    let inspector = crate::scenario::rect_of("render:inspector").unwrap();
    assert!(rect.height() > Tokens::get().font_small + Tokens::get().space_8, "long scope notice must have more than one line");
    assert!(inspector.contains_rect(rect));
    assert!(rect.bottom() < inspector.bottom());
    assert!(crate::scenario::rect_of("render:job-footer").is_none());
}

#[test]
fn temporary_library_maps_preserve_the_reviewed_plane_scale_and_optical_exclusivity() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let targets = h.app.run("render.scene", json!({})).unwrap();
    let target: materials::Target = serde_json::from_value(targets["bodies"][0].clone()).unwrap();
    let before = h.app.session.doc.to_json();
    materials::install_library_surface(&mut h.app, "SURF-001", target.clone(), "yz", 2.5).unwrap();
    let binding = &h.app.render_workspace.view.materials.bindings[0];
    assert_eq!(binding.maps.frame.u, [0.0, 250.0, 0.0]);
    assert_eq!(binding.maps.frame.v, [0.0, 0.0, 250.0]);
    assert_eq!(h.app.session.doc.to_json(), before, "temporary maps must not change the native model");
    let original = binding.maps.frame;
    assert!(materials::install_library_surface(&mut h.app, "SURF-001", target.clone(), "unknown", 1.0).is_err());
    assert!(materials::install_library_surface(&mut h.app, "SURF-001", target.clone(), "xy", 0.0).is_err());
    for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, f64::MAX] {
        assert!(materials::install_library_surface(&mut h.app, "SURF-001", target.clone(), "xy", scale).is_err());
        assert_eq!(h.app.render_workspace.view.materials.bindings[0].maps.frame, original);
    }
    assert_eq!(h.app.render_workspace.view.materials.bindings[0].maps.frame, original);
    h.app.render_workspace.view.materials.bindings[0].glass_enabled = true;
    assert!(materials::install_library_surface(&mut h.app, "SURF-001", target, "xy", 1.0).is_err());
    assert!(h.app.render_workspace.view.materials.bindings[0].glass_enabled);
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn native_composition_review_actions_are_visible_in_shared_dialog() {
    for dark in [false, true] {
        let mut h = fixture();
        h.call("ui.set", json!({"dark":dark}));
        super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
        setups::save(&mut h.app).unwrap();
        library::open_category(&mut h.app, "native_scenes");
        h.frames(3);
        h.step(&json!({"click":{"handle":"render:light:NATIVE-STUDIO-002"}})).unwrap();
        h.step(&json!({"click":{"handle":"render:light:review"}})).unwrap();
        h.frames(3);
        let card = crate::scenario::rect_of("review:sphere").unwrap();
        let replaces = crate::scenario::rect_of("dialog:note:review:replaces").unwrap();
        let kept = crate::scenario::rect_of("dialog:note:review:kept").unwrap();
        let ack = crate::scenario::rect_of("render:review:acknowledge").unwrap();
        assert!(card.bottom() <= replaces.top());
        assert!(replaces.bottom() <= kept.top());
        assert!(kept.bottom() <= ack.top());
        assert!(crate::scenario::rect_of("dialog:ok").is_some());
        assert!(!h.app.render_workspace.view.library.review.as_ref().unwrap().acknowledged);
    }
}

#[test]
fn native_composition_review_actions_are_visible_before_auxiliary_resource_details() {
    for dark in [false, true] {
        let mut h = fixture();
        h.call("ui.set", json!({"dark":dark}));
        super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
        setups::save(&mut h.app).unwrap();
        library::open_category(&mut h.app, "native_scenes");
        h.frames(3);
        let before = h.app.session.doc.to_json();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        h.step(&json!({"click":{"handle":"render:light:NATIVE-STUDIO-002"}})).unwrap();
        let library = h.app.shell.workspace_panels.get("render:appearances").unwrap().0;
        let review = crate::scenario::rect_of("render:light:review").unwrap();
        let note = crate::scenario::rect_of("catalog:footer").unwrap();
        assert!(library.contains_rect(review), "the review action must remain inside the library card: {library:?} {review:?}");
        assert!(review.bottom() <= note.top(), "the review action must precede supplementary prose");
        h.step(&json!({"click":{"handle":"render:light:review"}})).unwrap();
        h.frames(3);
        let card = crate::scenario::rect_of("review:sphere").unwrap();
        let replaces = crate::scenario::rect_of("dialog:note:review:replaces").unwrap();
        let kept = crate::scenario::rect_of("dialog:note:review:kept").unwrap();
        let ack = crate::scenario::rect_of("render:review:acknowledge").unwrap();
        assert!(card.bottom() <= replaces.top());
        assert!(replaces.bottom() <= kept.top());
        assert!(kept.bottom() <= ack.top());
        let apply = crate::scenario::rect_of("dialog:ok").unwrap();
        assert!(ack.bottom() <= apply.top());
        let dialog = crate::scenario::rect_of("dialog:window").unwrap();
        for (name, rect) in [("acknowledge", ack), ("apply", apply)] {
            assert!(dialog.contains_rect(rect), "{name} must remain inside the review dialog: {dialog:?} {rect:?}");
            assert!(h.ctx.content_rect().contains_rect(rect), "{name} must remain on screen: rect={rect:?}, dark={dark}");
        }
        assert!(!h.app.render_workspace.view.library.review.as_ref().unwrap().acknowledged);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
        assert!(h.app.render_workspace.view.completed.is_none());
    }
}

#[test]
fn ribbon_navigation_reuses_inspector_and_preserves_exact_unsaved_intent() {
    let mut h = fixture();
    h.app.render_workspace.view.samples = 37;
    h.app.render_workspace.view.draft.width = 777;
    let doc = h.app.session.doc.to_json();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for (id, page) in [
        ("render.preset.resolve", Editor::Scene),
        ("render.settings.get", Editor::Camera),
        ("render.library.appearance.apply", Editor::Appearance),
        ("render.library.surface.bind", Editor::Appearance),
        ("render.library.studio.prepare", Editor::Lighting),
        ("render.library.studio.apply", Editor::Lighting),
        ("render.library.environment.apply", Editor::Environment),
        ("render.stage.backdrop", Editor::Environment),
        ("render.quality.prepare", Editor::Sampling),
        ("render.status", Editor::Check),
        ("render.camera.get", Editor::Camera),
        ("render.camera.set", Editor::Camera),
        ("render.preset.apply", Editor::Scene),
        ("render.shot.duplicate", Editor::Camera),
        ("render.shot.rename", Editor::Camera),
        ("render.shot.remove", Editor::Camera),
        ("render.optics.bind", Editor::Appearance),
        ("render.optics.unbind", Editor::Appearance),
        ("render.library.surface.prepare", Editor::Appearance),
        ("render.library.surface.unbind", Editor::Appearance),
        ("render.library.physical.prepare", Editor::Appearance),
        ("render.stage.ground", Editor::Environment),
        ("render.stage.clear", Editor::Environment),
        ("render.hdr.inspect", Editor::Environment),
        ("render.environment.set", Editor::Environment),
        ("render.settings.capture_hdr", Editor::Environment),
        ("render.aging.describe", Editor::Wear),
        ("render.effects.describe", Editor::Effects),
        ("render.backends", Editor::Check),
    ] {
        h.app.start(id);
        h.frames(3);
        let expected = if matches!(page, Editor::Appearance | Editor::Camera) { Editor::Scene } else { page };
        assert_eq!(selected_editor(&h.app), expected, "{id}");
        assert_eq!(h.app.render_workspace.view.samples, 37);
        assert_eq!(h.app.render_workspace.view.draft.width, 777);
        assert_eq!(h.app.session.doc.to_json(), doc, "navigation may not execute a parameterless apply");
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
        assert!(h.app.render_workspace.view.completed.is_none());
        assert!(!crate::icon_svg::resolve(solidcraft_engine::find_command(id).unwrap().icon).placeholder);
    }
}

#[test]
fn render_stage_bar_keeps_actions_and_personal_preferences_in_both_layouts() {
    assert!(crate::workspace::panels("RENDER").is_empty());
    for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
        let mut h = fixture();
        h.app.ui.layout = layout;
        h.app.toolbar_custom.panels.insert("DESIGN/RENDER/RESULT".into(), vec!["render.export".into()]);
        let preferences = h.app.toolbar_custom.prefs();
        for width in [800, 1024, 1440, 2000] {
            h.call("ui.resize", json!({"width":width,"height":900}));
            h.frames(3);
            let bar = crate::scenario::rect_of("render:stage_bar").unwrap();
            let workspace = crate::scenario::rect_of("ribbon:workspace").unwrap();
            assert_eq!(bar.height(), workspace.height());
            assert_eq!(bar.top(), workspace.top());
            for id in ["render:stage:render.start", "render:stage:render.cancel"] {
                if let Some(action) = crate::scenario::rect_of(id) {
                    assert!(bar.contains_rect(action));
                }
            }
            assert!(crate::scenario::rect_of("ribbon:card").is_none());
            assert!(super::super::job_ready(&h.app, "render.cancel").is_err());
            assert!(h.app.render_workspace.view.job.is_none());
            assert_eq!(h.app.toolbar_custom.prefs(), preferences);
        }
    }
}

#[test]
fn ribbon_and_sticky_footer_submit_the_current_draft_not_defaults_or_saved_shot() {
    let mut h = fixture();
    h.app.render_workspace.view.draft.width = 32;
    h.app.render_workspace.view.draft.height = 24;
    h.app.render_workspace.view.draft.background = [24, 63, 107];
    let before = h.app.session.doc.to_json();
    let export = solidcraft_engine::find_command("render.export").unwrap();
    assert!(!super::super::interactive_command_info(&h.app, export).enabled);
    assert!(export.info(&h.app.session).enabled, "UI state must not alter direct engine API semantics");
    h.app.start("render.preflight");
    let request = super::super::image_request(&h.app).unwrap();
    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let job = h.app.render_workspace.view.job.expect("ribbon submitted an actual job");
    let handle = h.app.session.render_job_handle(job).unwrap();
    assert_eq!((handle.status().unwrap().request.width, handle.status().unwrap().request.height), (32, 24));
    assert_eq!(handle.status().unwrap().request.background, [24, 63, 107]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.app.render_workspace.view.completed != Some(job) {
        assert!(std::time::Instant::now() < deadline, "CPU fixture did not complete");
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(super::super::image_request(&h.app).unwrap(), request);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(super::super::interactive_command_info(&h.app, export).enabled);
    h.app.render_workspace.view.studio.export_exr = true;
    assert!(!super::super::interactive_command_info(&h.app, export).enabled, "a PNG-only result never pretends to have EXR");
}

#[test]
fn saved_bloom_intent_roundtrips_and_rejects_unsupported_transparent_output() {
    use solidcraft_engine::render::job::bloom::Bloom;
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.studio.finishing.bloom = Some(Bloom { strength: 0.25, threshold: 2.5, radius: 12 });
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio.finishing.bloom = None;
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups[&h.app.render_workspace.view.name], expected);
    h.app.render_workspace.view.transparent = true;
    assert!(setups::snapshot(&h.app).is_err());
    h.app.render_workspace.view.transparent = false;
    super::super::select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert!(super::super::image_request(&h.app).is_ok(), "inactive GPU settings remain portable but are not sent to CPU");
}

#[test]
fn ribbon_frame_preserves_explicit_roll_world_up_and_saved_view_authority() {
    let mut h = fixture();
    h.app.cam.roll = 0.37;
    h.app.cam.yaw = 0.21;
    h.app.cam.pitch = 0.43;
    h.app.cam.up = solidcraft_engine::render::UpAxis::Y;
    let orientation = (h.app.cam.roll, h.app.cam.yaw, h.app.cam.pitch, h.app.cam.up);
    let document = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.app.start("render.camera.fit");
    assert_eq!((h.app.cam.roll, h.app.cam.yaw, h.app.cam.pitch, h.app.cam.up), orientation);
    assert_eq!(h.app.session.doc.to_json(), document);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.session.camera.is_none(), "the explicit view request was consumed by the viewport");
    assert_eq!(h.app.ui.up_axis, h.app.cam.up);
}

#[test]
fn independent_sampling_and_volume_intent_roundtrip_offline_without_silent_conflict_repairs() {
    use solidcraft_engine::render::job::{adaptive::AdaptiveSampling, volume::HomogeneousVolume};
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.samples = 64;
    h.app.render_workspace.view.studio.adaptive =
        Some(AdaptiveSampling { pilot_samples: 16, min_samples: 32, relative_error: 0.03, absolute_error: 0.004, coverage_error: 0.02 });
    h.app.render_workspace.view.studio.volume = Some(HomogeneousVolume {
        min_mm: [-30.0, -20.0, -1.0],
        max_mm: [30.0, 20.0, 40.0],
        extinction_per_mm: 0.002,
        scattering_albedo: [0.7, 0.8, 0.9],
        anisotropy: 0.35,
    });
    h.app.render_workspace.view.studio.linear_exr = true;
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio.adaptive = None;
    h.app.render_workspace.view.studio.volume = None;
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups[&h.app.render_workspace.view.name], expected);
    let original = h.app.session.doc.to_json();
    h.app.render_workspace.view.studio.denoise = true;
    assert!(setups::snapshot(&h.app).is_err());
    assert_eq!(h.app.session.doc.to_json(), original);
    h.app.render_workspace.view.studio.denoise = false;
    h.app.render_workspace.view.studio.transmission_samples = 4;
    assert!(setups::snapshot(&h.app).is_err(), "active volume does not silently replace transmission strata");
    h.app.render_workspace.view.studio.volume = None;
    assert!(setups::snapshot(&h.app).is_ok());
    h.app.render_workspace.view.samples = 16;
    assert!(setups::snapshot(&h.app).is_err(), "minimum production count may not be silently clamped to the ceiling");
    h.app.render_workspace.view.samples = 64;
    setups::save(&mut h.app).unwrap();
    let stored = setups::snapshot(&h.app).unwrap();
    h.app.render_workspace.view.studio.transmission_samples = 1;
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), stored);
}

#[test]
fn master_regrade_requires_a_retained_master_instead_of_reconstructing_a_png() {
    let mut h = fixture();
    let spec = solidcraft_engine::find_command("render.result.regrade").unwrap();
    assert!(spec.info(&h.app.session).enabled, "direct API is validated by exact explicit source identity");
    assert!(!super::super::interactive_command_info(&h.app, spec).enabled);
    let document = h.app.session.doc.to_json();
    h.app.start("render.result.regrade");
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(h.app.session.doc.to_json(), document);
    assert!(!crate::icon_svg::resolve(spec.icon).placeholder);
    h.app.render_workspace.view.draft.width = 8;
    h.app.render_workspace.view.draft.height = 8;
    h.app.start("render.start");
    let job = h.app.render_workspace.view.job.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.app.render_workspace.view.completed != Some(job) {
        assert!(std::time::Instant::now() < deadline);
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!super::super::interactive_command_info(&h.app, spec).enabled);
    assert_eq!(h.app.session.doc.to_json(), document);
}

#[test]
fn public_render_inspection_exposes_exact_identities_without_execution_or_document_mutation() {
    let mut h = fixture();
    let doc = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    let inspected = h.call("ui.inspect", json!({}));
    assert_eq!(inspected["result"]["render"]["active"], true);
    assert_eq!(inspected["result"]["render"]["method"], "raster");
    assert!(inspected["result"]["render"]["job"].is_null());
    h.app.render_workspace.view.draft.width = 8;
    h.app.render_workspace.view.draft.height = 8;
    h.app.start("render.start");
    let identity = h.app.render_workspace.view.job.unwrap();
    let inspected = h.call("ui.inspect", json!({}));
    assert_eq!(inspected["result"]["render"]["job"], json!(identity));
    assert_eq!(h.app.session.doc.to_json(), doc);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.cam, camera);
    h.app.session.execute("render.cancel", &json!({"job":identity})).unwrap();
}

#[test]
fn aging_tab_adds_reorders_disables_and_persists_layers_without_engineering_edits() {
    use solidcraft_engine::render::job::aging::AgingKind;
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let before = h.app.session.doc.to_json();
    h.app.start("render.aging.describe");
    h.frames(3);
    assert_eq!(selected_editor(&h.app), Editor::Wear);
    assert!(!crate::icon_svg::resolve(solidcraft_engine::find_command("render.aging.describe").unwrap().icon).placeholder);
    for code in [1, 3] {
        h.step(&json!({"click":{"handle":"render:aging-add"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":format!("dialog:list:aging-add-{code}")}})).unwrap();
        h.frames(2);
    }
    assert_eq!(h.app.render_workspace.view.studio.aging.len(), 2);
    assert_eq!(h.app.render_workspace.view.studio.aging.get(1).unwrap().kind, AgingKind::Dust);
    h.step(&json!({"click":{"handle":"render:aging-up"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.studio.aging.get(0).unwrap().kind, AgingKind::Dust);
    h.step(&json!({"click":{"handle":"render:aging-enabled"}})).unwrap();
    h.frames(2);
    assert!(!h.app.render_workspace.view.studio.aging.get(0).unwrap().enabled);
    assert_ne!(h.app.session.doc.to_json(), before, "Wear saves its stack through render.settings.set");
    assert_eq!(setups::snapshot(&h.app).unwrap().job, h.app.session.doc.render.setups[&h.app.render_workspace.view.name].job);
    let targets = super::super::materials::read_targets(&mut h.app).unwrap();
    let identity = targets.iter().find(|target| target.name == "Subject").unwrap().stable_target.clone().unwrap();
    h.app.render_workspace.view.studio.aging.get_mut(0).unwrap().target = Some(identity.clone());
    let expected = setups::snapshot(&h.app).unwrap();
    let wire = serde_json::to_value(&expected).unwrap();
    assert_eq!(wire["job"]["options"]["aging"][0]["target"]["definition"], "Subject");
    assert!(wire["job"]["options"]["aging"][0]["target"].get("body_index").is_none());
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio.aging = Default::default();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups[&h.app.render_workspace.view.name], expected);
    assert_eq!(h.app.render_workspace.view.studio.aging.get(0).unwrap().target.as_ref(), Some(&identity));
    h.step(&json!({"click":{"handle":"render:aging-remove"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.studio.aging.len(), 1);
}

#[test]
fn wear_selection_precedes_output_page_and_appearance_selection_chrome() {
    fn assert_header(h: &Harness, title: &str) {
        let inspector = crate::scenario::rect_of("render:inspector").unwrap();
        let header = crate::scenario::rect_of("render:inspector:header").unwrap();
        assert!(inspector.contains_rect(header));
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let context = format!(
            "layout={:?}, page={:?}, title={title:?}, header={header:?}, pixels_per_point={}",
            h.app.ui.layout,
            selected_editor(&h.app),
            h.ctx.pixels_per_point(),
        );
        // egui stores Role::Label text in value(), not label(). Each pass supplies the
        // title node; its bounds are UI points (the root carries the pixel-scale transform).
        let titles: Vec<_> =
            tree.nodes.iter().filter(|(_, node)| node.role() == egui::accesskit::Role::Label && node.value() == Some(title)).collect();
        assert!(!titles.is_empty(), "missing inspector title: {context}");
        let bounds: Vec<_> = titles.iter().map(|(id, node)| (id, node.bounds())).collect();
        eprintln!(
            "header diagnostic: {context}, bounds={bounds:?}, label_matches={}",
            tree.nodes.iter().filter(|(_, node)| node.label() == Some(title)).count()
        );
        assert!(
            titles.iter().any(|(_, node)| {
                node.bounds().is_some_and(|bounds| {
                    bounds.x1 > bounds.x0
                        && bounds.y1 > bounds.y0
                        && header.contains_rect(egui::Rect::from_min_max(
                            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                        ))
                })
            }),
            "inspector title has missing, empty or out-of-header bounds: {context}, bounds={bounds:?}"
        );
    }
    for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
        let mut h = fixture();
        h.app.ui.layout = layout;
        h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Subject".into() }];
        {
            let selection = super::super::object_selection(&h.app);
            super::super::selection_editor(&mut h.app, &selection);
        }
        let before = h.app.session.doc.to_json();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        h.app.run("view.render_select", json!({"kind":"wear"})).unwrap();
        assert!(h.app.session.selection.is_empty(), "Render focus clears object picks");
        let wear_history = (history.0 + 1, history.1, history.2);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), wear_history);
        for stage in super::super::Stage::ALL {
            h.app.run("view.render_stage", json!({"stage":stage.key()})).unwrap();
            h.frames(3);
            assert_header(&h, h.app.tr("render.wear_title"));
            assert!(crate::scenario::rect_of("render:wear:controls").is_some());
            assert!(crate::scenario::rect_of("render:appearance:close").is_none());
            assert!(crate::scenario::rect_of("dialog:editor_footer").is_none());
            assert!(crate::scenario::rect_of("render:job-footer").is_none());
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), wear_history);
        }
        h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
        assert!(h.app.session.selection.is_empty());
        let output_history = wear_history;
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), output_history);
        h.frames(3);
        assert_header(&h, h.app.tr("render.page_output"));
        assert!(crate::scenario::rect_of("render:wear:controls").is_none());
        assert!(crate::scenario::rect_of("render:job-footer").is_none());
        assert!(crate::scenario::rect_of("render:appearance:close").is_none());
        h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
        // Clearing semantic focus opens Scene; choose Appearance to test its header recovery.
        assert_eq!(selected_editor(&h.app), Editor::Scene);
        h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Subject".into() }];
        {
            let selection = super::super::object_selection(&h.app);
            super::super::selection_editor(&mut h.app, &selection);
        }
        h.frames(3);
        assert_header(&h, "Subject");
        let inspector = crate::scenario::rect_of("render:inspector").unwrap();
        let header = crate::scenario::rect_of("render:inspector:header").unwrap();
        assert!(header.contains_rect(crate::scenario::rect_of("render:appearance:close").unwrap()));
        assert!(inspector.contains_rect(crate::scenario::rect_of("dialog:editor_footer").unwrap()));
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), output_history);
    }
}

#[test]
fn wear_selection_shows_disabled_controls_with_reason_and_never_changes_method() {
    use solidcraft_engine::render::job::aging::{AgingKind, AgingLayer};
    fn text_color(shape: &egui::Shape, label: &str) -> Option<egui::Color32> {
        match shape {
            egui::Shape::Text(text) if text.galley.text().to_lowercase().contains(&label.to_lowercase()) => Some(text.fallback_color),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_color(shape, label)),
            _ => None,
        }
    }
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":1800}));
    h.app.render_workspace.view.studio.aging.push(AgingLayer::preset(AgingKind::Rust)).unwrap();
    h.app.render_workspace.view.dirty = true;
    let document = h.app.session.doc.to_json();
    let stack = h.app.render_workspace.view.studio.aging.clone();
    h.app.run("view.render_select", json!({"kind":"wear"})).unwrap();
    let mut colours = Vec::new();
    for method in [ImageMethod::Raster, ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        h.app.render_workspace.view.method = method;
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.method, method);
        assert!(crate::scenario::rect_of("render:wear:controls").is_some());
        assert!(crate::scenario::rect_of("render:aging-enabled").is_some());
        assert!(crate::scenario::rect_of("render:aging-target").is_some());
        assert!(crate::scenario::rect_of("render:page:aging").is_none(), "Wear has no master page switcher");
        assert!(h.shapes.iter().any(|s| text_color(&s.shape, h.app.tr("render.effects_gpu_only")).is_some()));
        colours.push(h.shapes.iter().find_map(|s| text_color(&s.shape, h.app.tr("render.wear_layers"))).unwrap());
        if method != ImageMethod::GpuRaytrace {
            for handle in ["render:aging-add", "render:aging-enabled", "render:aging-layer:0:enabled", "render:aging-remove"] {
                h.step(&json!({"click":{"handle":handle}})).unwrap();
                assert_eq!(h.app.render_workspace.view.studio.aging, stack);
            }
        }
        assert_eq!(h.app.session.doc.to_json(), document);
    }
    for colour in &colours[..2] {
        assert!(colour.a() < colours[2].a(), "unsupported methods fade the editor");
    }
    h.step(&json!({"click":{"handle":"render:aging-layer:0:enabled"}})).unwrap();
    assert!(!h.app.render_workspace.view.studio.aging.get(0).unwrap().enabled);
    h.app.start("render.aging.describe");
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Wear);
    assert!(crate::scenario::rect_of("render:wear:controls").is_some());
    assert!(h.shapes.iter().all(|s| text_color(&s.shape, h.app.tr("render.aging_airborne")).is_none()));
}

#[test]
fn wear_editor_bounds_stale_selection_and_hostile_layer_values_never_panic() {
    use solidcraft_engine::render::job::aging::{AgingKind, AgingLayer};
    let mut h = fixture();
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.run("view.render_select", json!({"kind":"wear"})).unwrap();
    h.app.render_workspace.view.inspector.effects.selected_aging = usize::MAX;
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.inspector.effects.selected_aging, 0);
    h.app.render_workspace.view.studio.aging.push(AgingLayer::preset(AgingKind::Dust)).unwrap();
    h.app.render_workspace.view.dirty = true;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e300, 1e300] {
        let layer = h.app.render_workspace.view.studio.aging.get_mut(0).unwrap();
        layer.strength = value;
        layer.coverage = value;
        layer.scale_mm = value;
        layer.roughness = value;
        layer.metallic = value;
        layer.upward_bias = value;
        layer.origin_mm = [value; 3];
        layer.seed = u32::MAX;
        h.app.render_workspace.view.inspector.effects.selected_aging = usize::MAX;
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.inspector.effects.selected_aging, 0);
    }
    let model = h.app.session.doc.features.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:aging-remove"}})).unwrap();
    assert!(h.app.render_workspace.view.studio.aging.is_empty());
    let saved = &h.app.session.doc.render.setups[&h.app.render_workspace.view.name];
    assert!(matches!(&saved.job, Some(solidcraft_engine::doc::render_settings::RenderJob::GpuPath { options, .. }) if options.aging.is_empty()));
    assert_eq!(h.app.session.doc.features, model);
    assert_eq!(h.app.session.undo.len(), undo + 1, "removing an invalid layer saves one corrective command");
    h.frames(3);
    assert!(crate::scenario::rect_of("render:aging-enabled").is_none());
}

#[test]
fn output_effects_retains_ordered_looks_and_photographic_intent_across_save_and_reload() {
    use solidcraft_engine::render::job::looks::LookKind;
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let before = h.app.session.doc.to_json();
    h.app.start("render.effects.describe");
    h.frames(24);
    assert_eq!(selected_editor(&h.app), Editor::Effects);
    assert_eq!(h.app.session.doc.to_json(), before, "opening Effects is read-only");
    let undo = h.app.session.undo.len();

    for code in [3, 6] {
        h.step(&json!({"click":{"handle":"render:look-add"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":format!("dialog:list:look-add-{code}")}})).unwrap();
        h.frames(2);
    }
    assert_eq!(h.app.render_workspace.view.studio.finishing.looks.len(), 2);
    assert_eq!(h.app.render_workspace.view.studio.finishing.looks.get(0).unwrap().kind, LookKind::Warm);
    h.step(&json!({"click":{"handle":"render:look-up"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.studio.finishing.looks.get(0).unwrap().kind, LookKind::Faded);
    assert_eq!(h.app.session.undo.len(), undo + 3);
    assert_eq!(setups::snapshot(&h.app).unwrap().job, h.app.session.doc.render.setups[&h.app.render_workspace.view.name].job);
    h.app.render_workspace.view.studio.finishing.photographic.chromatic_pixels = 1.5;
    h.app.render_workspace.view.studio.finishing.photographic.flare.strength = 0.4;
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio.finishing = Default::default();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let doc = h.app.session.doc.to_json();
    h.app.render_workspace.view.transparent = true;
    assert!(super::super::ribbon::presentation(&h.app).validate().is_err(), "spatial lens effects cannot silently flatten alpha");
    assert_eq!(h.app.session.doc.to_json(), doc);
}

#[test]
fn backgrounds_ribbon_exposes_camera_and_physical_options_without_mutating_draft_or_switching_renderer() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.studio.backdrop_enabled = true;
    h.app.render_workspace.view.studio.backdrop.first_srgb = [30, 60, 90];
    h.app.render_workspace.view.dirty = true;
    let draft = setups::snapshot(&h.app).unwrap();
    let doc = h.app.session.doc.to_json();
    let revision = h.app.session.revision;
    h.app.start("render.stage.backdrop");
    h.frames(3);
    assert_eq!(selected_editor(&h.app), Editor::Environment);
    assert_eq!(setups::snapshot(&h.app).unwrap(), draft);
    assert_eq!(h.app.session.doc.to_json(), doc);
    assert_eq!(h.app.session.revision, revision);
    assert!(crate::scenario::rect_of("dialog:section:Backdrop").is_some());
    assert!(crate::scenario::rect_of("dialog:section:Ground and stage").is_some());
    assert!(crate::scenario::rect_of("dialog:section:Atmosphere").is_some());
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(
        super::super::ribbon::action_ready(&h.app, "render.result.regrade").is_err(),
        "no raw master cannot be silently reconstructed from the viewport"
    );
}

#[test]
fn clearing_physical_stage_preserves_unsaved_camera_background_and_output_intent() {
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    setups::save(&mut h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    h.app.run("render.stage.ground", json!({"name":name})).unwrap();
    setups::load(&mut h.app);
    assert_eq!(h.app.render_workspace.view.draft.stage.len(), 1);
    h.app.render_workspace.view.draft.background = [17, 53, 89];
    h.app.render_workspace.view.draft.width = 421;
    h.app.render_workspace.view.draft.height = 239;
    h.app.render_workspace.view.draft.exposure_ev = 1.25;
    h.app.render_workspace.view.studio.backdrop_enabled = true;
    h.app.render_workspace.view.studio.backdrop.mode = solidcraft_engine::render::job::finishing::BackdropMode::Radial;
    h.app.render_workspace.view.studio.backdrop.center = [0.3, 0.6];
    h.app.render_workspace.view.dirty = true;
    let mut expected = setups::snapshot(&h.app).unwrap();
    expected.stage = Default::default();
    let geometry = h.app.session.doc.features.clone();
    let materials = h.app.session.doc.materials.clone();
    h.app.start("render.stage.clear");
    h.frames(4);
    assert_eq!(selected_editor(&h.app), Editor::Environment);
    h.step(&json!({"click":{"handle":"render:stage-clear"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.draft.stage.len(), 1, "removal needs consent");
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected, "clearing stage must keep the unsaved numerical draft, not reload an older one");
    assert_eq!(h.app.session.doc.render.setups[&name], expected);
    assert_eq!(h.app.session.doc.features, geometry);
    assert_eq!(h.app.session.doc.materials, materials);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups[&name], expected);
}

#[test]
fn render_camera_labels_do_not_shadow_mechanical_pitch_and_axes_remain_explicit() {
    let en = crate::i18n::Locale::for_language("en");
    let de = crate::i18n::Locale::for_language("de");
    assert_eq!(de.label("Pitch"), "Steigung");
    assert_eq!(de.text("coupling.pitch"), "Steigung");
    assert_eq!(en.text("render.camera_pitch"), "Camera pitch");
    assert_ne!(en.text("render.camera_pitch"), en.text("coupling.pitch"));
    assert_eq!(de.text("render.camera_pitch"), "Kameraneigung");
    for locale in ["en", "de", "es", "ja", "zh"] {
        let locale = crate::i18n::Locale::for_language(locale);
        assert_eq!(locale.text("material.editor.axes_numeric"), "1 / 2 / 3");
        assert_eq!(locale.text("material.editor.axes_wood"), "L / R / T");
    }
}

#[test]
fn camera_draft_uses_render_command_and_keeps_saved_design_unchanged() {
    let mut h = fixture();
    let document = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for up in [solidcraft_engine::render::UpAxis::Z, solidcraft_engine::render::UpAxis::Y] {
        let mut camera = h.app.cam;
        camera.up = up;
        camera.roll = 0.41;
        camera.pitch = 0.27;
        apply_camera_draft(&mut h.app, camera).unwrap();
        assert_eq!(h.app.cam, camera);
        assert!(h.app.render_workspace.view.dirty);
        let mut invalid = camera;
        invalid.distance = 0.0;
        assert!(apply_camera_draft(&mut h.app, invalid).is_err());
        assert_eq!(h.app.cam, camera);
        fit_subject(&mut h.app).unwrap();
        assert_eq!((h.app.cam.up, h.app.cam.roll, h.app.cam.pitch), (up, camera.roll, camera.pitch));
        assert_eq!(h.app.session.doc.to_json(), document);
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

#[test]
fn shots_strip_switch_keeps_numeric_edits_saved_and_opens_the_selected_shot() {
    let mut h = fixture();
    h.app.render_workspace.view.name = "A".into();
    setups::save(&mut h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"B","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
    let before = setups::draft(&h.app).unwrap();
    h.app.render_workspace.view.draft.width = 123;
    setups::persist_editor(&mut h.app, &before).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shots:setup:B"}})).unwrap();
    assert!(!confirmation_open(&h.app));
    assert_eq!(h.app.render_workspace.view.name, "B");
    assert_eq!(h.app.render_workspace.view.draft.width, 321);
    assert_eq!(h.app.session.doc.render.setups["A"].width, 123);
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Shot("B".into()));
}

#[test]
fn output_sections_bloom_tone_and_grading_save_one_command_per_edit_without_starting() {
    use solidcraft_engine::render::job::ToneMap;
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":3000}));
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.start("render.effects.describe");
    h.frames(24);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(undo, h.app.session.undo.len());
    let tone = crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr("render.children.tone"))).unwrap();
    let grading = crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr("render.children.finishing"))).unwrap();
    let bloom = crate::scenario::rect_of("render:bloom-enable").unwrap();
    let reset = crate::scenario::rect_of("render:post-reset").unwrap();
    assert!(tone.bottom() <= grading.top());
    assert!(grading.bottom() <= bloom.top() && bloom.bottom() <= reset.top(), "Bloom belongs to the Finishing section");
    reveal_inspector_target(&mut h, "render:bloom-enable");
    h.step(&json!({"click":{"handle":"render:bloom-enable"}})).unwrap();
    assert_eq!(undo + 1, h.app.session.undo.len());
    assert!(h.app.render_workspace.view.studio.finishing.bloom.is_some());
    let (next, expected) = if h.app.render_workspace.view.studio.tone_map == ToneMap::Reinhard {
        ("render:tone:aces", ToneMap::AcesFitted)
    } else {
        ("render:tone:reinhard", ToneMap::Reinhard)
    };
    reveal_inspector_target(&mut h, next);
    h.step(&json!({"click":{"handle":next}})).unwrap();
    assert_eq!(h.app.render_workspace.view.studio.tone_map, expected);
    assert_eq!(undo + 2, h.app.session.undo.len());
    let contrast = reveal_inspector_target(&mut h, "render.post_contrast");
    h.step(&json!({"click":[contrast.right()-10.0,contrast.center().y],"double":true})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"1.4"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.render_workspace.view.studio.finishing.contrast, 1.4);
    assert_eq!(undo + 3, h.app.session.undo.len());
    assert_eq!(setups::snapshot(&h.app).unwrap().job, h.app.session.doc.render.setups[&h.app.render_workspace.view.name].job);
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(h.app.session.doc.features, document.features);
}

#[test]
fn output_sections_check_report_copies_success_staleness_and_errors_through_platform_queue() {
    let mut h = fixture();
    h.app.session.set_render_job_host(Some(solidcraft_engine::render::job::CpuHost::cooperative())).unwrap();
    let doc = h.app.session.doc.clone();
    let state = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty());
    super::super::ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
    let report = check_report(&h.app).unwrap();
    assert!(report.contains(h.app.tr("render.output_ready")));
    assert!(report.contains("cpu-raster"));
    h.app.run("render.preflight.copy", json!({"text":report})).unwrap();
    assert_eq!(h.app.render_workspace.clipboard_text.as_deref(), Some(report.as_str()));
    h.frames(1);
    assert!(h.output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == &report)));
    assert!(h.app.render_workspace.clipboard_text.is_none());
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(h.app.session.render_job_host().unwrap().admitted_bytes(), 0);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(state, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty()));
    h.app.render_workspace.view.draft.width = 32;
    let lines = check_lines(&h.app);
    assert_eq!(lines[0].key, "stale");
    assert_eq!(lines[0].status, ui_kit::CheckStatus::Warning);
    assert!(check_report(&h.app).unwrap().contains(h.app.tr("render.review_stale")));
    let request = super::super::image_params(&h.app).unwrap();
    record_check(&mut h.app, &request, &json!({"error":"host refused the request"}));
    let lines = check_lines(&h.app);
    assert_eq!(lines[0].status, ui_kit::CheckStatus::Error);
    assert_eq!(lines[0].label, "host refused the request");
    assert!(check_report(&h.app).unwrap().contains("host refused the request"));

    let quality = json!({"name":"Quality report regression", "gpu_options":{"samples":128}});
    library::review::open(&mut h.app, "quality", "Quality".into(), quality.clone(), library::review::Apply::Quality(quality.clone())).unwrap();
    let report = check_report(&h.app).unwrap();
    assert!(report.contains(h.app.tr("render.quality_review")));
    assert!(report.contains(&quality.to_string()));
    h.app.run("render.preflight.copy", json!({"text":report})).unwrap();
    assert_eq!(h.app.render_workspace.clipboard_text.as_deref(), Some(report.as_str()));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(state, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty()));
}

#[test]
fn output_sections_sizes_refuse_hostile_input_and_4k_follows_the_selected_budget() {
    let mut h = fixture();
    let before = h.app.render_workspace.view.draft.clone();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    for (width, height) in [(0, 1), (1, 0), (u32::MAX, 1), (4096, 4096), (3840, 2160)] {
        assert!(apply_size(&mut h.app, width, height, false).is_err());
        assert_eq!(h.app.render_workspace.view.draft, before);
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
    }
    let reason = size_limit_reason(&h.app, 3840, 2160);
    assert!(reason.contains("4194304"));
    assert!(reason.contains("2730 × 1536"));
    super::super::select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.raytrace.budget = solidcraft_engine::render::job::CpuBudget::Studio;
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:preset:4k"}})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (3840, 2160));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.render_workspace.view.job.is_none());
}

/// #8941: the Shots strip offers 4K UHD whatever the method. Under the default CPU raster, which
/// has no profiles, the list and the refusal name CPU ray tracing with Studio; neither is selected.
#[test]
fn cpu_raster_refusing_a_size_names_the_method_and_profile_that_render_it() {
    use solidcraft_engine::render::job::CpuBudget;
    let mut h = fixture();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster, "the default method is unchanged");
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:shots:size"}})).unwrap();
    h.step(&json!({"widget":"Full HD · 1920 × 1080"})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1920, 1080));
    assert!(super::super::image_request(&h.app).is_ok());
    h.step(&json!({"click":{"handle":"render:shots:size"}})).unwrap();
    h.step(&json!({"widget":"4K UHD · 3840 × 2160 · needs Studio"})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (3840, 2160));
    let refusal = super::super::image_request(&h.app).unwrap_err();
    assert!(refusal.contains("Largest fitting size: 2730 × 1536."), "{refusal}");
    assert!(refusal.contains("To render 3840 × 2160, choose CPU ray tracing with the Studio CPU resource profile, or a smaller size."), "{refusal}");
    assert_eq!(refusal, size_limit_reason(&h.app, 3840, 2160));
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
    assert_eq!(h.app.render_workspace.view.raytrace.budget, CpuBudget::Compact);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!size_limit_reason(&h.app, 16384, 16384).contains("CPU resource profile"), "past Workstation no profile is named");
    assert_eq!(size_mark(&h.app, "fits", Some((2048, 2048))), None);
    h.app.preferences.language = "de".into();
    let _language = crate::i18n::scope("de");
    assert!(super::super::image_request(&h.app).unwrap_err().contains("CPU-Ressourcenprofil Studio"));
}

/// #8941: under the default Compact CPU profile 4K UHD is refused with a translated reason that
/// names Studio, and the size lists mark only the presets the selected profile can't render.
/// Nothing switches the profile or the size; GPU path tracing shows no CPU profile.
#[test]
fn a_size_the_cpu_profile_refuses_names_the_profile_that_renders_it() {
    use solidcraft_engine::render::job::CpuBudget;
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shots:size"}})).unwrap();
    h.step(&json!({"widget":"4K UHD · 3840 × 2160 · needs Studio"})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.budget, CpuBudget::Compact, "the default profile is unchanged");
    let reason = size_limit_reason(&h.app, 3840, 2160);
    assert!(reason.contains("Largest fitting size: 2730 × 1536."), "{reason}");
    assert!(reason.contains("To render 3840 × 2160, choose the Studio CPU resource profile, or a smaller size."), "{reason}");
    assert_eq!(size_mark(&h.app, "4K UHD", Some((3840, 2160))).as_deref(), Some("4K UHD · needs Studio"));
    for fits in [(1280, 720), (1920, 1080), (2048, 2048)] {
        assert_eq!(size_mark(&h.app, "fits", Some(fits)), None, "{fits:?}");
    }
    assert_eq!(size_mark(&h.app, "custom", None), None);
    assert_eq!(size_mark(&h.app, "past every profile", Some((16385, 16385))), None);
    let reason = size_limit_reason(&h.app, 16384, 16384);
    assert!(!reason.contains("CPU resource profile"), "past Workstation no profile is named: {reason}");

    // Render and Check are refused with the same translated reason, and nothing is changed.
    h.app.render_workspace.view.draft.width = 3840;
    h.app.render_workspace.view.draft.height = 2160;
    let doc = h.app.session.doc.clone();
    h.frames(3);
    assert_eq!(cpu::validate_progressive_image(&h.app).unwrap_err(), size_limit_reason(&h.app, 3840, 2160));
    assert!(super::super::image_request(&h.app).unwrap_err().contains("choose the Studio CPU resource profile"));
    assert_eq!(h.app.render_workspace.view.raytrace.budget, CpuBudget::Compact);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (3840, 2160));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));

    // Studio renders 4K and marks nothing up to it; a 16K-wide image names Workstation.
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Studio;
    cpu::validate_progressive_image(&h.app).unwrap();
    assert_eq!(size_mark(&h.app, "4K UHD", Some((3840, 2160))), None);
    assert_eq!(size_mark(&h.app, "16K", Some((15360, 4320))).as_deref(), Some("16K · needs Workstation"));
    assert!(size_limit_reason(&h.app, 15360, 4320).contains("choose the Workstation CPU resource profile"));
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Workstation;
    assert_eq!(size_mark(&h.app, "16K", Some((15360, 4320))), None);

    // Progressive sampling's camera-sample budget counts: 4K at 64 spp needs Studio too.
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Compact;
    h.app.render_workspace.view.raytrace.progressive = Some(solidcraft_engine::render::job::CpuProgressiveOptions { samples_per_pixel: 64, seed: 0 });
    assert_eq!(cpu::profile_for_size(&h.app, 3840, 2160), Some(CpuBudget::Studio));
    h.app.render_workspace.view.raytrace.progressive = None;

    // Translated: the profile is named as the profile selector names it.
    h.app.preferences.language = "ja".into();
    {
        let _language = crate::i18n::scope("ja");
        let reason = size_limit_reason(&h.app, 3840, 2160);
        assert!(reason.contains("CPUリソースプロファイル「スタジオ」"), "{reason}");
        assert_eq!(size_mark(&h.app, "4K UHD", Some((3840, 2160))).as_deref(), Some("4K UHD・「スタジオ」が必要"));
    }
    h.app.preferences.language = "en".into();
    h.frames(3);

    // GPU path tracing renders 4K and never names a CPU profile.
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shots:size"}})).unwrap();
    h.step(&json!({"widget":"4K UHD · 3840 × 2160"})).unwrap();
    assert_eq!(size_mark(&h.app, "4K UHD", Some((3840, 2160))), None);
    // 15360 × 4320 is past the GPU limits, yet Workstation would admit it: still no CPU profile.
    assert_eq!(size_mark(&h.app, "16K", Some((15360, 4320))), None);
    assert!(!size_limit_reason(&h.app, 15360, 4320).contains("CPU resource profile"));
    cpu::validate_progressive_image(&h.app).unwrap();
}

#[test]
fn shots_strip_render_submits_the_current_job_request() {
    let mut h = fixture();
    h.app.render_workspace.view.draft.width = 24;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.draft.background = [31, 57, 83];
    let document = h.app.session.doc.clone();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shots:render"}})).unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let status = h.app.session.render_job_handle(job).unwrap().status().unwrap();
    assert_eq!((status.request.width, status.request.height), (24, 16));
    assert_eq!(status.request.background, [31, 57, 83]);
    assert_eq!(h.app.session.doc, document);
}

#[test]
fn browser_shot_remove_keeps_confirmation_and_rejects_stale_actions() {
    let mut h = fixture();
    h.app.render_workspace.view.name = "A".into();
    setups::save(&mut h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"B","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
    let before = h.app.session.doc.to_json();
    shot_row_action(&mut h.app, "B".into(), "render.shot.remove");
    h.frames(3);
    assert!(confirmation_open(&h.app));
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    shot_row_action(&mut h.app, "B".into(), "render.shot.remove");
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(!h.app.session.doc.render.setups.contains_key("B"));
    assert_eq!(h.app.render_workspace.view.name, "A");
}

#[test]
fn appearance_inspector_targets_and_chain_follow_body_face_and_component() {
    use super::selection_appearance::{chains, inspected_look, targets};
    use solidcraft_engine::{Sel, geom::Vec3};
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Part"})).unwrap();
    h.app.run("solid.box", json!({"body_name":"Subject","length":20,"width":15,"height":10})).unwrap();
    h.app.run("material.assign", json!({"bodies":["Subject"],"material":"Steel"})).unwrap();
    h.app.run("appearance.assign", json!({"components":["Part"],"appearance":"ABS - Blue"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    h.app.run("appearance.assign", json!({"body":"Subject","faces":[[10,7,10]],"appearance":"Copper"})).unwrap();
    let face = h.app.run("model.faces", json!({"body":"Subject"})).unwrap()["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["point"][2] == 10.0)
        .unwrap()["index"]
        .as_u64()
        .unwrap() as usize;
    h.app.session.selection = vec![Sel::Face { body: "Subject".into(), index: face, point: Vec3::new(10.0, 7.0, 10.0) }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    let before = h.app.session.doc.to_json();
    for (step, present) in [
        (ui_kit::LookStep::Face, [true, true, true, true]),
        (ui_kit::LookStep::Body, [false, true, true, true]),
        (ui_kit::LookStep::Component, [false, false, true, true]),
    ] {
        let target = targets(&h.app, step);
        let chain = chains(&mut h.app, &target).unwrap();
        assert_eq!(chain[0].each_ref().map(Option::is_some), present);
        assert_eq!(inspected_look(&chain[0], Some(ui_kit::LookStep::Component)).unwrap().name, "ABS - Blue");
        assert_eq!(inspected_look(&chain[0], Some(ui_kit::LookStep::Material)).unwrap().name, "Steel");
        let mut inherited = chain[0].clone();
        inherited[1] = None;
        inherited[2] = None;
        assert_eq!(inspected_look(&inherited, Some(ui_kit::LookStep::Body)).unwrap().name, "Steel");
    }
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn appearance_inspector_all_users_replaces_exact_look_once_without_changing_mass() {
    use super::selection_appearance::{assign, chains, targets, users};
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("solid.box", json!({"body_name":"Other","length":5,"width":5,"height":5,"corner":[30,0,0]})).unwrap();
    h.app.run("material.assign", json!({"bodies":["Subject","Other"],"material":"Steel"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Subject","Other"],"appearance":"ABS - Red"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    let target = targets(&h.app, ui_kit::LookStep::Body);
    let from = chains(&mut h.app, &target).unwrap()[0][1].clone().unwrap();
    assert_eq!(users(&h.app, &from), 2);
    let mut to = from.clone();
    to.color = [12, 34, 56];
    let mass = h.app.run("component.properties", json!({})).unwrap();
    let undo = h.app.session.undo.len();
    assign(&mut h.app, &target, &from, &to, true).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    for name in ["Subject", "Other"] {
        assert_eq!(h.app.session.doc.appearances.bodies.get(name), Some(&to));
    }
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(users(&h.app, &from), 2);
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:stage:look"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Appearance);
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":format!("dialog:{}:{}", h.app.tr("render.inspector_applies_to"), crate::i18n::format("render.inspector_all_users", &[("count", "2")]))}}))
        .unwrap();
    h.step(&json!({"click":{"handle":"render:appearance:preview:replace"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog: search"}})).unwrap();
    h.step(&json!({"text":"ABS - Blue"})).unwrap();
    let blue = crate::dialogs_appearance::replacement_library(&mut h.app).iter().position(|look| look.name == "ABS - Blue").unwrap();
    h.step(&json!({"click":{"handle":format!("render:appearance:replace:{blue}")}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    for name in ["Subject", "Other"] {
        assert_eq!(h.app.session.doc.appearances.bodies[name].name, "ABS - Blue");
    }
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":format!("dialog:{}:{}",h.app.tr("render.inspector_finish"),h.app.tr("render.inspector_matte"))}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1, "an all-users field edit is one replacement command");
    for name in ["Subject", "Other"] {
        assert_eq!(h.app.session.doc.appearances.bodies[name].pbr.unwrap().roughness(), 0.85);
    }
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
}

#[test]
fn appearance_inspector_finish_presets_mixed_set_all_and_clear_use_existing_assignments() {
    use super::selection_appearance::{assign, chains, clear, finish, mixed, targets};
    use solidcraft_engine::{Sel, doc::appearance::Look};
    let mut h = fixture();
    h.app.run("solid.box", json!({"body_name":"Other","length":5,"width":5,"height":5,"corner":[30,0,0]})).unwrap();
    h.app.run("material.assign", json!({"bodies":["Subject","Other"],"material":"Steel"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Other"],"appearance":"ABS - Blue"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }, Sel::Body { name: "Other".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    let target = targets(&h.app, ui_kit::LookStep::Body);
    let looks: Vec<_> = chains(&mut h.app, &target).unwrap().into_iter().map(|c| c[1].clone().unwrap()).collect();
    assert!(mixed(&looks, |l| l.color));
    assert!(!mixed(&looks, |l| l.opacity));
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:stage:look"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Appearance);
    assert!(h.widget(h.app.tr("render.tree_mixed")).is_some());
    appearance_inspector_capture(&mut h, "mixed");
    let mass = h.app.run("component.properties", json!({})).unwrap();
    let mut to = Look::custom([40, 50, 60], 0.5);
    for (index, metallic, roughness) in [(0, 0.0, 0.85), (1, 0.0, 0.5), (2, 0.0, 0.12), (3, 1.0, 0.32)] {
        finish(&mut to, index).unwrap();
        let pbr = to.pbr.unwrap();
        assert_eq!((pbr.metallic(), pbr.roughness(), to.opacity), (metallic, roughness, 1.0));
    }
    let undo = h.app.session.undo.len();
    assign(&mut h.app, &target, &looks[0], &to, false).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.appearances.bodies.get("Subject"), h.app.session.doc.appearances.bodies.get("Other"));
    clear(&mut h.app, &target, &to).unwrap();
    assert!(h.app.session.doc.appearances.bodies.is_empty());
    assert!(chains(&mut h.app, &target).unwrap().iter().all(|c| c[3].as_ref().is_some_and(|l| l.name == "Steel")));
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
}

#[test]
fn appearance_inspector_real_controls_switch_targets_inspect_chain_and_edit_finish() {
    use solidcraft_engine::{Sel, geom::Vec3};
    let mut h = fixture();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    let face = h.app.run("model.faces", json!({"body":"Subject"})).unwrap()["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["point"][2] == 10.0)
        .unwrap()["index"]
        .as_u64()
        .unwrap() as usize;
    h.app.session.selection = vec![Sel::Face { body: "Subject".into(), index: face, point: Vec3::new(10.0, 7.0, 10.0) }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:stage:look"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Appearance);
    let before = h.app.session.doc.to_json();
    h.step(&json!({"click":{"handle":"render:appearance:chain-menu"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:appearance:chain:body"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    appearance_inspector_capture(&mut h, "face");
    h.frames(3);
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":format!("dialog:{}:{}",h.app.tr("render.inspector_finish"),h.app.tr("render.inspector_metal"))}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.appearances.bodies["Subject"].pbr.unwrap().metallic(), 1.0);
    let roughness = crate::scenario::rect_of(&format!("dialog:scalar:{}", h.app.tr("appearance.roughness"))).unwrap().center();
    let drag_undo = h.app.session.undo.len();
    h.step(&json!({"drag":[[roughness.x,roughness.y],[roughness.x+20.0,roughness.y]],"steps":12})).unwrap();
    assert_eq!(h.app.session.undo.len(), drag_undo + 1, "a multi-frame field edit is one undo step");
    assert_ne!(h.app.session.doc.appearances.bodies["Subject"].pbr.unwrap().roughness(), 0.32);
    let edited_undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":format!("dialog::{}",h.app.tr("render.pick_body"))}})).unwrap();
    assert!(h.app.ui.pick_bodies);
    assert_eq!(h.app.session.undo.len(), edited_undo);
    h.step(&json!({"click":{"handle":format!("dialog::{}",h.app.tr("render.pick_component"))}})).unwrap();
    assert!(h.app.ui.pick_components);
    assert!(h.widget(h.app.tr("render.inspector_pick_target")).is_some(), "root bodies have no component override target");
    h.step(&json!({"click":{"handle":format!("dialog::{}",h.app.tr("render.pick_face"))}})).unwrap();
    assert!(!h.app.ui.pick_components && !h.app.ui.pick_bodies);
    h.step(&json!({"click":{"handle":format!("dialog::{}",h.app.tr("render.pick_body"))}})).unwrap();
    assert_eq!(h.app.session.undo.len(), edited_undo);
    appearance_inspector_capture(&mut h, "body");
    h.app.run("select.clear", json!({})).unwrap();
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Nothing);
    appearance_inspector_capture(&mut h, "explicit-target");
}

#[test]
fn appearance_inspector_mixed_fields_are_inert_until_edited_and_accept_the_first_value() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("solid.box", json!({"body_name":"Other","length":5,"width":5,"height":5,"corner":[30,0,0]})).unwrap();
    for (name, opacity) in [("Subject", 0.25), ("Other", 0.75)] {
        h.app.run("appearance.assign", json!({"bodies":[name],"color":"#123456","opacity":opacity})).unwrap();
    }
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }, Sel::Body { name: "Other".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:stage:look"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Appearance);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let field = format!("dialog:scalar:{}", h.app.tr("render.inspector_opacity"));
    h.step(&json!({"click":{"handle":field}})).unwrap();
    h.step(&json!({"key":"Tab"})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before, "leaving Mixed without editing must be inert");
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"click":{"handle":field}})).unwrap();
    h.step(&json!({"text":"NaN"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before, "invalid input must not set mixed targets");
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"click":{"handle":field}})).unwrap();
    h.step(&json!({"text":"0.25"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo, "typing keeps a local draft until commit");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    for name in ["Subject", "Other"] {
        assert_eq!(h.app.session.doc.appearances.bodies[name].opacity, 0.25);
    }
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn appearance_inspector_optical_colour_and_opacity_edits_commit_once_with_shared_bounds() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app
        .run(
            "appearance.assign",
            json!({"bodies":["Subject"],"color":"#123456",
        "pbr":{"metallic":0.2,"roughness":0.4,"dielectric_ior":1.5}}),
        )
        .unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:stage:look"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Appearance);
    let mass = h.app.run("component.properties", json!({})).unwrap();
    for (key, text) in [("appearance.metallic", "2"), ("appearance.roughness", "0"), ("render.inspector_opacity", "0.25")] {
        let undo = h.app.session.undo.len();
        h.step(&json!({"click":{"handle":format!("dialog:scalar:{}",h.app.tr(key))}})).unwrap();
        h.step(&json!({"text":text})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1, "{key}");
        let look = &h.app.session.doc.appearances.bodies["Subject"];
        match key {
            "appearance.metallic" => assert_eq!(look.pbr.unwrap().metallic(), 1.0),
            "appearance.roughness" => assert_eq!(look.pbr.unwrap().roughness(), 0.045),
            _ => assert_eq!((look.opacity, look.pbr), (0.25, None)),
        }
    }
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":format!("dialog:scalar:{}",h.app.tr("render.appearance_editor.ior"))}})).unwrap();
    h.step(&json!({"text":"3.5"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let look = &h.app.session.doc.appearances.bodies["Subject"];
    assert_eq!((look.pbr.unwrap().dielectric_ior(), look.opacity), (3.0, 1.0));
    h.step(&json!({"click":{"handle":format!("dialog:color:{}",h.app.tr("render.inspector_colour"))}})).unwrap();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"color:channel:R"}})).unwrap();
    h.step(&json!({"text":"200"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.appearances.bodies["Subject"].color[0], 200);
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
}

#[test]
fn appearance_inspector_hex_edits_validate_and_commit_once() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    appearance_picker_open(&mut h);
    let hex = "color:hex";
    h.step(&json!({"click":{"handle":hex}})).unwrap();
    h.step(&json!({"key":"A","cmd":true,"ctrl":true})).unwrap();
    h.step(&json!({"text":"#GG0000"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":hex}})).unwrap();
    h.step(&json!({"key":"A","cmd":true,"ctrl":true})).unwrap();
    h.step(&json!({"text":"#123456"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.appearances.bodies["Subject"].color, [0x12, 0x34, 0x56]);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("solid.box", json!({"body_name":"Other","length":5,"width":5,"height":5,"corner":[30,0,0]})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Other"],"color":"#ABCDEF"})).unwrap();
    h.app.session.selection.push(Sel::Body { name: "Other".into() });
    h.frames(3);
    let mixed_before = h.app.session.doc.to_json();
    if crate::scenario::handle_at(hex).is_none() {
        appearance_picker_open(&mut h);
    }
    h.step(&json!({"click":{"handle":hex}})).unwrap();
    h.step(&json!({"key":"Tab"})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), mixed_before);
    h.step(&json!({"click":{"handle":hex}})).unwrap();
    h.step(&json!({"key":"A","cmd":true,"ctrl":true})).unwrap();
    h.step(&json!({"text":"#123456"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.appearances.bodies["Other"].color, [0x12, 0x34, 0x56]);
}

fn appearance_picker_open(h: &mut Harness) {
    h.step(&json!({"click":{"handle":format!("dialog:color:{}", h.app.tr("render.inspector_colour"))}})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":"color:format:Hex"}})).unwrap();
}

fn appearance_picker_paste(h: &mut Harness, text: &str) {
    h.step(&json!({"click":{"handle":"color:hex"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true,"ctrl":true})).unwrap();
    h.frame_with(vec![egui::Event::Paste(text.into())]);
}

#[test]
fn appearance_inspector_shared_picker_preserves_exact_opacity_and_commits_alpha() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#abcdef","opacity":0.123456,"appearance":"Tint"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    let mass = h.app.run("component.properties", json!({})).unwrap();
    appearance_picker_open(&mut h);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for mode in ["F", "U8", "Hex"] {
        h.step(&json!({"click":{"handle":format!("color:format:{mode}")}})).unwrap();
        let draft = h.app.render_workspace.view.inspector.selection_appearance.draft().unwrap();
        assert_eq!((draft.color, draft.opacity, draft.pbr), ([171, 205, 239], 0.123456, None));
    }
    for text in ["#abc", "#abcdefz0"] {
        appearance_picker_paste(&mut h, text);
        h.frames(2);
        let draft = h.app.render_workspace.view.inspector.selection_appearance.draft().unwrap();
        assert_eq!((draft.color, draft.opacity), ([171, 205, 239], 0.123456));
        assert!(h.widget(text).is_some(), "invalid text remains editable inside the picker");
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
    }
    for (text, color, opacity) in [
        ("#0102fe", [1, 2, 254], 0.123456),
        ("#0102fe80", [1, 2, 254], 128.0 / 255.0),
        ("#fE8001", [254, 128, 1], 128.0 / 255.0),
        ("#22334400", [34, 51, 68], 0.0),
    ] {
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        appearance_picker_paste(&mut h, text);
        let draft = h.app.render_workspace.view.inspector.selection_appearance.draft().unwrap();
        assert_eq!((draft.color, draft.opacity, draft.pbr), (color, opacity, None), "{text} updates the exact draft immediately");
        assert_eq!(h.app.session.doc.to_json(), before, "typing does not commit the model");
        assert_eq!(h.app.session.undo.len(), undo);
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        let look = &h.app.session.doc.appearances.bodies["Subject"];
        assert_eq!((look.color, look.opacity, look.pbr), (color, opacity, None));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one committed edit for {text}");
    }
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
}

#[test]
fn appearance_inspector_shared_picker_pbr_rejects_nonopaque_alpha() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app
        .run("appearance.assign", json!({"bodies":["Subject"],"color":"#123456", "pbr":{"metallic":0.2,"roughness":0.4,"dielectric_ior":1.6}}))
        .unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let pbr = h.app.session.doc.appearances.bodies["Subject"].pbr;
    appearance_picker_open(&mut h);
    for text in ["#abcdef80", "#abcdef00"] {
        appearance_picker_paste(&mut h, text);
        let draft = h.app.render_workspace.view.inspector.selection_appearance.draft().unwrap();
        assert_eq!((draft.color, draft.opacity, draft.pbr), ([18, 52, 86], 1.0, pbr));
        assert!(h.widget(text).is_some());
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
    }
    appearance_picker_paste(&mut h, "#abcdefFF");
    h.step(&json!({"key":"Enter"})).unwrap();
    let look = &h.app.session.doc.appearances.bodies["Subject"];
    assert_eq!((look.color, look.opacity, look.pbr), ([171, 205, 239], 1.0, pbr));
    assert_eq!(h.app.session.undo.len(), undo + 1);
}

#[test]
fn appearance_inspector_shared_picker_has_no_outer_hex_editor_and_keeps_mixed_values_visible() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("solid.box", json!({"body_name":"Other","length":5,"width":5,"height":5,"corner":[30,0,0]})).unwrap();
    for (body, color) in [("Subject", "#123456"), ("Other", "#abcdef")] {
        h.app.run("appearance.assign", json!({"bodies":[body],"color":color,"opacity":0.123456})).unwrap();
    }
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }, Sel::Body { name: "Other".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    let outer = format!("dialog:color_hex:{}", h.app.tr("render.inspector_colour"));
    assert!(crate::scenario::handle_at(&outer).is_none(), "Hex editing belongs only inside the shared picker");
    assert!(crate::scenario::rect_of("dialog:note:color:mixed").is_some(), "mixed colour remains visibly labelled beside the swatch");
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    appearance_picker_open(&mut h);
    assert!(crate::scenario::handle_at(&outer).is_none());
    assert!(crate::scenario::handle_at("color:hex").is_some());
    h.step(&json!({"click":{"handle":"color:hex"}})).unwrap();
    h.step(&json!({"key":"Tab"})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before, "opening and leaving mixed input is inert");
    assert_eq!(h.app.session.undo.len(), undo);
    appearance_picker_paste(&mut h, "#123456");
    h.step(&json!({"key":"Enter"})).unwrap();
    for body in ["Subject", "Other"] {
        let look = &h.app.session.doc.appearances.bodies[body];
        assert_eq!((look.color, look.opacity, look.pbr), ([18, 52, 86], 0.123456, None));
    }
    assert_eq!(h.app.session.undo.len(), undo + 1, "accepting the first colour still sets mixed targets once");
}

#[test]
fn appearance_inspector_component_and_face_overrides_remove_to_material_from_real_controls() {
    use solidcraft_engine::{Sel, geom::Vec3};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    h.app.run("component.create", json!({"name":"Part"})).unwrap();
    h.app.run("solid.box", json!({"body_name":"Subject","length":20,"width":15,"height":10})).unwrap();
    h.app.run("material.assign", json!({"bodies":["Subject"],"material":"Steel"})).unwrap();
    h.app.run("appearance.assign", json!({"components":["Part"],"appearance":"ABS - Blue"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    h.app.run("appearance.assign", json!({"body":"Subject","faces":[[10,7,10]],"appearance":"Copper"})).unwrap();
    let face = h.app.run("model.faces", json!({"body":"Subject"})).unwrap()["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["point"][2] == 10.0)
        .unwrap()["index"]
        .as_u64()
        .unwrap() as usize;
    h.app.session.selection = vec![Sel::Face { body: "Subject".into(), index: face, point: Vec3::new(10.0, 7.0, 10.0) }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    super::super::enter(&mut h.app, true).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:stage:look"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Appearance);
    let mass = h.app.run("component.properties", json!({})).unwrap();
    for (step, name) in [("face", "Copper"), ("body", "ABS - Red"), ("component", "ABS - Blue"), ("material", "Steel")] {
        let before = h.app.session.doc.to_json();
        h.step(&json!({"click":{"handle":"render:appearance:chain-menu"}})).unwrap();
        h.step(&json!({"click":{"handle":format!("render:appearance:chain:{step}")}})).unwrap();
        assert!(h.widget(name).is_some(), "inspecting {step} must show {name}");
        assert_eq!(h.app.session.doc.to_json(), before);
    }
    h.step(&json!({"click":{"handle":format!("dialog::{}",h.app.tr("render.pick_component"))}})).unwrap();
    appearance_inspector_capture(&mut h, "component");
    h.frames(3);
    for step in ["component", "body", "face"] {
        h.step(&json!({"click":{"handle":format!("dialog::{}",h.app.tr(match step {
            "component" => "render.pick_component", "body" => "render.pick_body", _ => "render.pick_face",
        }))}}))
        .unwrap();
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget":"…"})).unwrap();
        h.step(&json!({"widget":h.app.tr("render.inspector_remove")})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1, "clearing {step}: {:?}", h.app.render_workspace.view.inspector.selection_appearance);
    }
    assert!(h.app.session.doc.appearances.is_empty());
    assert!(h.widget("Steel").is_some());
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
}

// Opt-in evidence from the actual application UI, without requiring a window server.
fn appearance_inspector_capture(h: &mut Harness, name: &str) {
    let Some(dir) = std::env::var_os("SOLIDCRAFT_INSPECTOR_EVIDENCE") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for dark in [false, true] {
        h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
        h.app.ui.dark = dark;
        h.app.preferences.render_inspector_width = 305.0;
        // Texture handles belong to the Harness context. Keep that context and
        // reupload cached artwork so this CPU renderer has the complete images,
        // including textures uploaded before evidence recording began.
        let ctx = h.ctx.clone();
        crate::icons::clear_texture_cache(&ctx);
        super::super::library::clear_preview_texture(&mut h.app);
        let thumbnails = std::mem::take(&mut h.app.render_workspace.thumbnails);
        let mut textures = std::collections::HashMap::new();
        textures.insert(egui::TextureId::default(), ctx.fonts(|fonts| fonts.image()));
        let start = ctx.input(|input| input.time);
        let mut last = None;
        for frame in 0..10 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                    time: Some(start + (frame + 1) as f64 / 10.0),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            ui_kit::update_test_textures(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            last = Some(output);
        }
        let output = last.unwrap();
        let card = crate::scenario::rect_of("render:inspector").unwrap();
        let crop = ui_kit::raster_ui(&ctx, output.shapes.clone(), output.pixels_per_point, card, &textures);
        crop.save(std::path::Path::new(&dir).join(format!("{name}-{}-305.png", if dark { "dark" } else { "light" }))).unwrap();
        let image = ui_kit::raster_ui(&ctx, output.shapes, output.pixels_per_point, ctx.content_rect(), &textures);
        image.save(std::path::Path::new(&dir).join(format!("{name}-{}.png", if dark { "dark" } else { "light" }))).unwrap();
        h.app.render_workspace.thumbnails = thumbnails;
    }
}

#[test]
fn appearance_inspector_approved_layout_is_305_wide_and_preserves_stage_bounds() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("solid.box", json!({"body_name":"Boss","length":10,"width":10,"height":10,"corner":[30,0,0]})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Boss","Subject"],"appearance":"Aluminum - Anodized Blue"})).unwrap();
    h.app.run("material.assign", json!({"bodies":["Boss","Subject"],"material":"Steel"})).unwrap();
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Boss".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.preferences.render_inspector_width = 305.0;
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    appearance_inspector_capture(&mut h, "approved");
    h.frames(3);
    let inspector = crate::scenario::rect_of("render:inspector").unwrap();
    let browser = crate::scenario::rect_of("browser:card").unwrap();
    let shots = crate::scenario::rect_of("render:shots_strip").unwrap();
    assert!((inspector.width() - 305.0).abs() <= 1.0, "{inspector:?}");
    assert!((inspector.top() - browser.top()).abs() <= 1.0);
    assert!(inspector.bottom() < shots.top());
    assert!(h.widget(&crate::i18n::format("render.inspector_all_users", &[("count", "2")])).is_some());
    assert!(h.widget(h.app.tr("render.inspector_save_library")).is_some());
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"click":{"handle":"render:appearance:close"}})).unwrap();
    assert!(h.app.render_workspace.view.inspector.hidden);
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    assert!(!h.app.render_workspace.view.inspector.hidden);
    h.step(&json!({"click":{"handle":format!("dialog:{}:{}",h.app.tr("render.inspector_finish"),h.app.tr("render.inspector_matte"))}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:action:{}", h.app.tr("render.inspector_reset"))}})).unwrap();
    assert_eq!(h.app.session.doc.appearances.bodies.get("Boss"), h.app.session.doc.appearances.bodies.get("Subject"));
    assert!(h.app.session.doc.appearances.bodies.contains_key("Subject"));
    assert_eq!(h.app.session.undo.len(), undo + 2);
}

#[test]
fn appearance_editor_e2_sections_are_ordered_collapsed_and_expand_for_each_method() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":4000}));
    h.app.preferences.render_inspector_width = 308.0;
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    let titles = [
        "render.inspector_edit",
        "render.appearance_editor.glass",
        "render.appearance_editor.coat",
        "render.appearance_editor.maps",
        "render.appearance_editor.saved",
        "render.appearance_editor.reflections",
    ];
    h.frames(3);
    let before = h.app.session.doc.to_json();
    let mut last = 0.0;
    for title in titles {
        let rect = crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(title))).unwrap();
        assert!(rect.top() > last, "{title} follows the preceding design section");
        last = rect.bottom();
    }
    assert!(h.widget(h.app.tr("render.appearance_editor.glass_enable")).is_none(), "optics default to collapsed");
    assert!(h.widget(h.app.tr("render.appearance_editor.gpu_reason")).is_some());
    for title in titles.into_iter().skip(1) {
        h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr(title))}})).unwrap();
    }
    for method in [ImageMethod::Raster, ImageMethod::GpuRaytrace, ImageMethod::CpuRaytrace] {
        h.app.render_workspace.view.method = method;
        h.frames(3);
        for key in [
            "render.appearance_editor.glass_enable",
            "render.appearance_editor.recipe",
            "render.appearance_editor.boundary",
            "render.appearance_editor.thickness",
            "render.appearance_editor.absorption",
            "render.material_shadow",
            "render.subsurface_enable",
            "render.material_coat_enable",
            "render.appearance_editor.weight",
            "render.material_maps_enable",
            "render.appearance_editor.base_colour",
            "render.appearance_editor.normal",
            "appearance.metallic",
            "appearance.roughness",
            "render.appearance_editor.origin",
            "render.appearance_editor.u_repeat",
            "render.appearance_editor.v_repeat",
            "render.appearance_editor.normal_strength",
            "render.appearance_editor.v_up",
            "render.material_color_replace",
            "render.appearance_editor.save",
            "render.appearance_editor.remove",
            "render.appearance_editor.replace",
            "render.appearance_editor.revalidate",
            "render.cpu_reflection_off",
            "render.cpu_reflection_mirror",
            "render.cpu_reflection_rough",
        ] {
            assert!(h.widget(h.app.tr(key)).is_some(), "expanded E2 is missing {key} for {method:?}");
        }
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let glass = tree.nodes.iter().find(|(_, n)| n.label() == Some(h.app.tr("render.appearance_editor.glass_enable"))).unwrap();
        assert_eq!(glass.1.is_disabled(), method != ImageMethod::GpuRaytrace);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.render_workspace.view.materials.bindings.is_empty(), "opening never installs temporary bindings");
    }
    for selection in
        [Sel::Face { body: "Subject".into(), index: 0, point: solidcraft_engine::geom::Vec3::ZERO }, Sel::Body { name: "Subject".into() }]
    {
        h.app.session.selection = vec![selection];
        {
            let selection = super::super::object_selection(&h.app);
            super::super::selection_editor(&mut h.app, &selection);
        }
        if matches!(h.app.session.selection.first(), Some(Sel::Body { .. })) {
            h.app.ui.pick_components = true;
        }
        h.frames(3);
        assert!(h.widget(h.app.tr("render.appearance_editor.body_reason")).is_some());
        for title in titles.into_iter().skip(1) {
            assert!(crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(title))).is_some());
        }
    }
}

#[test]
fn output_children_select_their_editor_with_all_sections_in_design_order() {
    use super::super::RenderSelection;
    for method in [ImageMethod::Raster, ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1440,"height":3000}));
        super::super::select_image_method(&mut h.app, method);
        let doc = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        for (kind, selection, page, sections) in [
            (
                "sampling",
                RenderSelection::Sampling,
                Editor::Sampling,
                vec!["render.output_quality", "render.children.path", "render.children.adaptive", "render.children.glass", "render.children.cpu"],
            ),
            (
                "effects",
                RenderSelection::Effects,
                Editor::Effects,
                vec![
                    "render.children.tone",
                    "render.children.looks",
                    "render.children.lens_effects",
                    "render.children.finishing",
                    "render.children.regrade",
                ],
            ),
            (
                "check",
                RenderSelection::Check,
                Editor::Check,
                vec!["render.children.check", "render.children.request", "render.children.renderers", "render.children.under_hood"],
            ),
            (
                "output",
                RenderSelection::Output,
                Editor::Output,
                vec!["render.children.renderer", "render.output_image", "render.output_destination", "render.children.render"],
            ),
        ] {
            let handle = if kind == "output" { "render:tree:output".to_string() } else { format!("render:tree:output:{kind}") };
            // The capped Browser can publish rows below its clip after Lights adds more entries.
            // Reveal the Output group before exercising each real child-row click.
            h.frames(3);
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            let header = crate::scenario::rect_of("render:browser:header").unwrap();
            h.call("ui.scroll", json!({"x":browser.center().x,"y":(header.bottom()+browser.bottom())/2.0,"delta":-2000}));
            h.frames(20);
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            let header = crate::scenario::rect_of("render:browser:header").unwrap();
            let target = crate::scenario::handle_at(&handle).unwrap();
            assert!(browser.shrink(Tokens::get().space_8).contains(target) && target.y > header.bottom(), "{handle} is outside the visible Browser");
            h.step(&json!({"click":{"handle":handle}})).unwrap();
            h.frames(3);
            assert_eq!(h.app.render_workspace.view.selection, selection);
            assert_eq!(selected_editor(&h.app), page);
            let mut bottom = 0.0;
            for key in sections {
                let rect = crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(key)))
                    .unwrap_or_else(|| panic!("missing {key} for {method:?}/{kind}"));
                assert!(rect.top() >= bottom, "section order for {method:?}/{kind}/{key}");
                bottom = rect.bottom();
            }
            assert!(crate::scenario::rect_of("render:job-footer").is_none());
            assert!(crate::scenario::rect_of("render:page:output").is_none());
            assert_eq!(h.app.render_workspace.view.method, method);
            assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
            assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
        }
        h.step(&json!({"click":{"handle":"render:output:check"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Check);
        h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
        h.frames(3);
        h.step(&json!({"click":{"handle":"render:stage:check"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Check);
        assert!(h.app.render_workspace.view.inspector.checked.is_none(), "opening Check never runs preflight");
        h.frames(3);
        h.step(&json!({"click":{"handle":"render:quality:custom"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Sampling);
    }
}

#[test]
fn appearance_editor_reset_restores_each_picked_saved_look() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    let saved = h.app.session.doc.appearances.bodies["Subject"].clone();
    let target = selection_appearance::targets(&h.app, ui_kit::LookStep::Body);
    let changed = solidcraft_engine::doc::appearance::Look::custom([10, 20, 30], 0.3);
    selection_appearance::assign(&mut h.app, &target, &saved, &changed, false).unwrap();
    h.frames(3);
    selection_appearance::reset(&mut h.app);
    assert_eq!(h.app.session.doc.appearances.bodies["Subject"], saved);
}

#[test]
fn appearance_editor_saved_optics_removal_confirms_and_preserves_other_drafts() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":2200}));
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    let set = setups::snapshot(&h.app).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    h.app
        .run(
            "render.optics.bind",
            json!({"name":name,"set":set,"body_index":0,"body_name":"Subject",
        "optics":{"ior":1.5,"absorption_per_mm":[0.0,0.0,0.0],"boundary":"closed_mesh","thickness_mm":0.0,"shadow_transmission":false},
        "expected_revision":h.app.session.revision}),
        )
        .unwrap();
    setups::load(&mut h.app);
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.appearance_editor.saved"))}})).unwrap();
    let before = h.app.session.doc.to_json();
    h.step(&json!({"click":{"handle":"render:optics-remove"}})).unwrap();
    assert!(materials::confirmation_open(&h.app));
    assert!(h.widget(h.app.tr("render.appearance_editor.remove_title")).is_some());
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(!materials::confirmation_open(&h.app));
    assert_eq!(h.app.session.doc.to_json(), before);
    h.step(&json!({"click":{"handle":"render:optics-remove"}})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"appearance":"ABS - Red"})).unwrap();
    h.frames(3);
    assert!(!materials::confirmation_open(&h.app), "a changed document expires the pending confirmation");
    assert_eq!(h.app.session.doc.render.setups[&name].optics.len(), 1);
    h.app.render_workspace.view.draft.width = 777;
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:optics-remove"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.session.doc.render.setups[&name].optics.is_empty());
    assert!(h.app.render_workspace.view.draft.optics.is_empty());
    assert_eq!(h.app.render_workspace.view.draft.width, 777);
    assert_eq!(h.app.session.doc.appearances.bodies["Subject"].name, "ABS - Red");
}

#[test]
fn appearance_editor_save_optics_keeps_other_temporary_bindings() {
    use solidcraft_engine::Sel;
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":2600}));
    h.app.run("solid.box", json!({"body_name":"Other","length":5,"width":5,"height":5,"corner":[30,0,0]})).unwrap();
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    let other = materials::read_targets(&mut h.app).unwrap().into_iter().find(|t| t.name == "Other").unwrap();
    let mut binding = materials::Binding::new(other, &h.app);
    binding.coat_enabled = true;
    h.app.render_workspace.view.materials.bindings.push(binding);
    h.app.render_workspace.view.materials.enabled = true;
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    {
        let selection = super::super::object_selection(&h.app);
        super::super::selection_editor(&mut h.app, &selection);
    }
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.appearance_editor.glass"))}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.material_custom")})).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:starter.glass.amber-tint"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.appearance_editor.saved"))}})).unwrap();
    let geometry = h.app.run("component.properties", json!({})).unwrap();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:optics-save"}})).unwrap();
    let view = &h.app.render_workspace.view;
    assert_eq!(h.app.session.doc.render.setups[&view.name].optics.len(), 1);
    assert_eq!(view.materials.bindings.len(), 1);
    assert_eq!(view.materials.bindings[0].target.name, "Other");
    assert!(view.materials.bindings[0].coat_enabled);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.run("component.properties", json!({})).unwrap(), geometry);
}

#[test]
fn output_children_unsupported_controls_stay_visible_with_reason_and_cannot_edit() {
    fn has_text(shape: &egui::Shape, text: &str) -> bool {
        match shape {
            egui::Shape::Text(s) => s.galley.text().contains(text),
            egui::Shape::Vec(s) => s.iter().any(|s| has_text(s, text)),
            _ => false,
        }
    }
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1440,"height":3000}));
    let doc = h.app.session.doc.clone();
    h.app.run("view.render_select", json!({"kind":"sampling"})).unwrap();
    h.frames(3);
    assert!(h.shapes.iter().any(|s| has_text(&s.shape, h.app.tr("render.effects_gpu_only"))));
    assert!(h.shapes.iter().any(|s| has_text(&s.shape, h.app.tr("render.children.needs_cpu"))));
    h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.children.adaptive"))}})).unwrap();
    h.frames(3);
    let before = h.app.render_workspace.view.studio.clone();
    let point = reveal_inspector_target(&mut h, "render:adaptive-enable").center();
    h.step(&json!({"click":[point.x,point.y]})).unwrap();
    assert_eq!(h.app.render_workspace.view.studio.adaptive, before.adaptive);
    h.app.run("view.render_select", json!({"kind":"effects"})).unwrap();
    h.frames(3);
    assert!(h.shapes.iter().any(|s| has_text(&s.shape, h.app.tr("render.children.effects_banner"))));
    let point = reveal_inspector_target(&mut h, "render:bloom-enable").center();
    h.step(&json!({"click":[point.x,point.y]})).unwrap();
    assert_eq!(h.app.render_workspace.view.studio.finishing, before.finishing);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
}

#[test]
fn output_children_hostile_drafts_are_readable_without_normalizing_or_saving() {
    let mut h = fixture();
    super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.samples = u32::MAX;
    h.app.render_workspace.view.max_depth = u32::MAX;
    h.app.render_workspace.view.studio.finishing.contrast = f64::NAN;
    h.app.render_workspace.view.dirty = true;
    let doc = h.app.session.doc.clone();
    for kind in ["sampling", "effects", "check", "output"] {
        h.app.run("view.render_select", json!({"kind":kind})).unwrap();
        h.frames(3);
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.render_workspace.view.samples, u32::MAX);
        assert_eq!(h.app.render_workspace.view.max_depth, u32::MAX);
        assert!(h.app.render_workspace.view.studio.finishing.contrast.is_nan());
        assert!(h.app.render_workspace.view.job.is_none());
    }
}

#[test]
fn output_children_check_diagnostics_are_collapsed_and_copy_the_current_request() {
    let mut h = fixture();
    setups::save(&mut h.app).unwrap();
    h.call("ui.resize", json!({"width":1440,"height":3000}));
    h.app.session.set_render_job_host(Some(solidcraft_engine::render::job::CpuHost::cooperative())).unwrap();
    h.app.pending_fit = None;
    h.app.cancel_view_animation();
    h.app.run("view.render_select", json!({"kind":"check"})).unwrap();
    h.frames(3);
    assert!(h.widget(h.app.tr("render.children.copy_request")).is_none());
    h.step(&json!({"click":{"handle":"render:check"}})).unwrap();
    assert!(h.app.render_workspace.view.inspector.checked.is_some());
    assert!(h.app.render_workspace.view.job.is_none());
    let request = super::super::image_request(&h.app).unwrap();
    let expected = serde_json::to_string_pretty(&request).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.under_hood"))}})).unwrap();
    h.frames(3);
    let button = crate::scenario::rect_of("render:check:copy-request").unwrap();
    assert!(crate::scenario::rect_of("render:inspector").unwrap().contains_rect(button));
    // The control-channel click helper settles four extra frames, which discards
    // one-frame platform events. Drive the native button and observe every frame.
    let at = button.center();
    h.frame_with(vec![egui::Event::PointerMoved(at)]);
    for pressed in [true, false] {
        h.frame_with(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
    }
    let copied = |h: &Harness| {
        h.output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
    };
    let mut text = copied(&h);
    if text.is_none() {
        assert_eq!(h.app.render_workspace.clipboard_text.as_deref(), Some(expected.as_str()));
        h.frame();
        text = copied(&h);
    }
    assert_eq!(text.as_deref(), Some(expected.as_str()));
    let request = super::super::image_params(&h.app).unwrap();
    record_check(&mut h.app, &request, &json!({"warnings":["Sampling budget needs review"]}));
    h.frames(3);
    assert!(crate::scenario::rect_of("render:check:go-to:1").is_some());
    h.step(&json!({"click":{"handle":"render:check:go-to:1"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, super::super::RenderSelection::Sampling);
    h.app.run("view.render_select", json!({"kind":"check"})).unwrap();
    record_check(&mut h.app, &request, &json!({"error":"Exposure exceeds the CPU limit"}));
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:check:go-to:0"}})).unwrap();
    assert_eq!(selected_editor(&h.app), Editor::Camera);
}

#[test]
fn output_children_focus_survives_clearing_an_object_pick() {
    let mut h = fixture();
    let document = h.app.session.doc.clone();
    for (kind, selection) in [
        ("sampling", super::super::RenderSelection::Sampling),
        ("effects", super::super::RenderSelection::Effects),
        ("check", super::super::RenderSelection::Check),
    ] {
        h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: "Subject".into() }]})).unwrap();
        h.app.run("view.render_select", json!({"kind":kind})).unwrap();
        assert_eq!(h.app.render_workspace.view.selection, selection);
        assert!(h.app.session.selection.is_empty());
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    }
}
