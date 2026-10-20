use super::build_scene;
use crate::{Services, SolveApp};
use serde_json::json;
use solidcraft_engine::{Session, doc, geom::Vec3, kernel};

#[test]
fn a_finished_sketch_on_a_face_draws_a_halo_under_its_own_curves_only() {
    use crate::theme::{self, ColorVision, Tokens};

    let mut app = SolveApp::new(Session::default(), Services::default());
    app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    app.run("sketch.create", json!({"plane":{"face":[20,15,20]},"project_edges":true})).unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0":[-5,-5],"p1":[5,5]})).unwrap();
    app.run("sketch.circle.center", json!({"center":[0,0],"radius":3})).unwrap();
    app.run("sketch.line", json!({"points":[[-4,0],[4,0]]})).unwrap();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let original_dark = theme::is_dark();
    let original_vision = theme::vision();
    for finished in [false, true] {
        if finished {
            app.run("sketch.finish", json!({})).unwrap();
        }
        for dark in [false, true] {
            for vision in ColorVision::ALL {
                theme::set_dark(dark);
                theme::set_vision(vision);
                let tokens = Tokens::get();
                crate::prefs::set(&mut app, &json!({"sketch_halo": true})).unwrap();
                let projection = super::projection(&app, rect);
                let enabled_key = super::sketch_scene_key(&app, &projection);
                let scene = super::build_sketch_scene(&app, &projection);
                // The sketch being edited draws on top; a finished one is depth-tested.
                let bytes = if finished { &scene.lines } else { &scene.overlay };
                let (lines, remainder) = bytes.as_chunks::<{ crate::gpu::LINE_SIZE }>();
                assert!(remainder.is_empty());
                let halo = super::c4(tokens.sketch_halo);
                let halo_count = lines.iter().filter(|line| line[24..28] == halo).count();
                crate::prefs::set(&mut app, &json!({"sketch_halo": false})).unwrap();
                assert_ne!(super::sketch_scene_key(&app, &projection), enabled_key, "changing the preference rebuilds the sketch scene");
                let disabled = super::build_sketch_scene(&app, &projection);
                let without_halos: Vec<u8> = lines.iter().filter(|line| line[24..28] != halo).flat_map(|line| line.iter().copied()).collect();
                assert_eq!(if finished { &disabled.lines } else { &disabled.overlay }, &without_halos, "off draws only the original curves");
                if !finished {
                    assert_eq!(halo_count, 0, "a sketch being edited has no halo");
                    continue;
                }
                assert!(halo_count > 5, "rectangle, circle and line emit halos");
                let width = |line: &[u8]| f32::from_le_bytes(line[28..32].try_into().unwrap());
                let projected = super::c4(tokens.sketch_projected);
                let mut cores = 0;
                let mut references = 0;
                for (index, core) in lines.iter().enumerate().filter(|(_, line)| line[24..28] != halo) {
                    let rim = lines[..index].iter().find(|line| line[..24] == core[..24] && line[24..28] == halo);
                    if core[24..28] == projected {
                        assert!(rim.is_none(), "the face's projected outline has no halo");
                        references += 1;
                    } else {
                        assert_eq!(core[24..28], super::c4(tokens.sketch_fixed));
                        let rim = rim.unwrap();
                        assert_eq!(width(rim), width(core) + super::SKETCH_HALO_WIDTH, "halo precedes the narrower core");
                        cores += 1;
                    }
                }
                assert!(references > 0, "the projected face outline is drawn");
                assert_eq!(cores, halo_count, "one halo per curve segment");
            }
        }
    }
    theme::set_dark(original_dark);
    theme::set_vision(original_vision);
}

#[test]
fn placed_step_body_uses_imported_opacity_until_a_design_look_overrides_it() {
    let body = kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0))
        .unwrap()
        .with_color(Some([0.2, 0.6, 0.8]))
        .with_paint(Some(kernel::Paint { opacity: 0.4, faces: Vec::new() }));
    let step =
        kernel::step_export_bodies(&[kernel::ExportBody { name: "Imported glass".into(), body: &body, color: body.color() }], &Default::default())
            .unwrap();
    let mut document = doc::Document::default();
    let component = document.add_component(Some("Imported part"), 0).unwrap();
    let feature = document
        .add_feature(doc::FeatureKind::Import { file: "glass.step".into(), step: step.into(), components: Vec::new(), skip: Vec::new() }, None)
        .unwrap();
    document.features.iter_mut().find(|f| f.id == feature).unwrap().component = component;
    let mut session = Session::new(document);
    let source = session.model.state().bodies[0].name.clone();
    assert!((session.doc.model_body_opacity(&session.model.state().bodies[0]) - 0.4).abs() < 1e-6);
    session.execute("occurrence.copy", &json!({"component":component,"translate":[30,0,0]})).unwrap();
    let mut app = SolveApp::new(session, Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    let placed = app.session.world_state().bodies.iter().find(|b| b.name != source).unwrap().name.clone();
    app.session.selection = vec![solidcraft_engine::Sel::Body { name: placed }];
    app.start("appearance.assign");
    let crate::dialogs::Kind::Appearance(appearance) = &app.dialog.as_ref().unwrap().kind else { panic!("appearance dialog") };
    assert!((appearance.opacity - 0.4).abs() < 1e-6);
    assert_eq!(appearance.color, [51, 153, 204]);
    crate::dialogs::cancel(&mut app);
    let scene = build_scene(&app).merged();
    assert!(!scene.glass.is_empty());
    assert!(
        scene.tris.is_empty(),
        "opaque triangles {}, bodies {:?}",
        scene.tris.len(),
        app.session.world_state().bodies.iter().map(|b| (&b.name, b.body.paint(), &b.imported)).collect::<Vec<_>>()
    );
    let (vertices, remainder) = scene.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>();
    assert!(remainder.is_empty());
    assert!(vertices.iter().all(|v| v[27] == 102));
    app.run("appearance.assign", json!({"bodies":[source],"color":"#336699","opacity":0.7})).unwrap();
    let scene = build_scene(&app);
    let (vertices, remainder) = scene.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>();
    assert!(remainder.is_empty());
    assert!(vertices.iter().all(|v| v[27] == 179));
    app.run("appearance.assign", json!({"bodies":[source]})).unwrap();
    let scene = build_scene(&app);
    let (vertices, remainder) = scene.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>();
    assert!(remainder.is_empty());
    assert!(vertices.iter().all(|v| v[27] == 102));
}

/// Exercise the live GPU scene slots: a fresh headless render always rebuilds and would hide
/// stale cached geometry. Theme changes must update bytes while retaining the model's mesh.
#[test]
fn theme_switches_refresh_cached_viewport_colours_without_remeshing() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.ui.ground_shadow = false;
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    h.app.viewport.gpu = Some(super::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false });
    let body = h.app.session.world_state().bodies[0].name.clone();
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: body.clone() }];
    h.frame();
    let revision = h.app.session.revision;
    let mesh = h.app.session.world_state().bodies[0].cached_mesh().unwrap();
    let mut previous: Option<(u64, u64, u64)> = None;
    for theme in ["dark", "gruvbox-dark", "blueprint", "dark", "light", "solidcraft-95", "cupertino", "light"] {
        let result = h.call("ui.prefs", json!({"theme": theme}));
        assert!(result.get("error").is_none(), "{result}");
        h.frame();
        let keys = (h.app.viewport.key, h.app.viewport.sketch_key, h.app.viewport.hl_key);
        if let Some(old) = previous {
            assert_ne!(keys.0, old.0, "{theme}: model cache");
            assert_ne!(keys.1, old.1, "{theme}: sketch cache");
            assert_ne!(keys.2, old.2, "{theme}: highlight cache");
        }
        previous = Some(keys);
        let scene = h.app.viewport.slot.lock().unwrap().take().expect("new model colours uploaded").merged();
        let (vertices, remainder) = scene.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>();
        assert!(remainder.is_empty() && !vertices.is_empty());
        let colour = super::c4(crate::theme::Tokens::get().body);
        assert!(vertices.iter().all(|v| v[24..28] == colour), "{theme}: stale body colour");
        assert_eq!(h.app.session.revision, revision, "theme changes must not edit the design");
        assert!(std::sync::Arc::ptr_eq(&mesh, &h.app.session.world_state().bodies[0].cached_mesh().unwrap()));
        h.frame();
        assert_eq!((h.app.viewport.key, h.app.viewport.sketch_key, h.app.viewport.hl_key), keys);
        assert!(h.app.viewport.slot.lock().unwrap().is_none(), "an unchanged theme reuses its scene");
    }
    h.app.run("appearance.assign", json!({"bodies": [body], "color": "#336699"})).unwrap();
    for theme in ["gruvbox-dark", "blueprint", "solidcraft-95"] {
        h.call("ui.prefs", json!({"theme": theme}));
        h.frame();
        let scene = h.app.viewport.slot.lock().unwrap().take().unwrap().merged();
        let (vertices, _) = scene.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>();
        assert!(!vertices.is_empty());
        assert!(vertices.iter().all(|v| v[24..28] == [51, 102, 153, 255]), "{theme}: explicit appearance preserved");
    }
}

#[test]
fn cancelling_theme_preview_restores_viewport_palette_keys() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.call("ui.prefs", json!({"theme": "gruvbox-dark"}));
    h.frame();
    let keys = |app: &SolveApp| (super::scene_key(app), super::highlight_geometry_key(app));
    let before = keys(&h.app);
    h.call("ui.prefs", json!({"open": "Interface"}));
    h.call("ui.prefs", json!({"theme": "blueprint"}));
    h.frame();
    assert_ne!(keys(&h.app), before);
    h.call("ui.prefs", json!({"button": "cancel"}));
    h.frame();
    assert_eq!(keys(&h.app), before);
    // Replacing an imported palette keeps its id and base but still changes geometry colours.
    let custom = |color| json!({"format":"solidcraft-theme/1","name":"Viewport test","base":"dark","colors":{"body":color}});
    h.call("ui.prefs", json!({"theme_import": custom("#abcdef")}));
    h.call("ui.prefs", json!({"theme": "viewport-test"}));
    h.frame();
    let first = keys(&h.app);
    h.call("ui.prefs", json!({"theme_import": custom("#fedcba")}));
    h.frame();
    assert_ne!(keys(&h.app), first);
}

#[test]
fn inspect_menu_toggles_component_colors_without_a_dialog_and_refreshes_scene() {
    let mut h = crate::scenario::Harness::new();
    h.frames(3);
    let command = solidcraft_engine::find_command("inspect.colored_components").unwrap();
    let info = command.info(&h.app.session);
    assert_eq!((info.label, info.icon), ("Enable Component Colors", "colored_components"));
    h.step(&json!({"click": {"handle": "tab:INSPECT"}})).unwrap();
    h.frames(2);
    let original_key = super::scene_key(&h.app);
    h.step(&json!({"click": {"handle": "panel:INSPECT:CHECK"}})).unwrap();
    h.frames(2);
    assert!(h.widget("Enable Component Colors").is_some());
    h.step(&json!({"click": {"handle": "dropdown:inspect.colored_components"}})).unwrap();
    assert!(h.app.session.colored_components.is_some());
    assert!(h.app.dialog.is_none());
    assert_ne!(super::scene_key(&h.app), original_key);
    let info = command.info(&h.app.session);
    assert_eq!((info.label, info.icon), ("Disable Component Colors", "colored_components_off"));
    h.step(&json!({"click": {"handle": "panel:INSPECT:CHECK"}})).unwrap();
    h.frames(2);
    assert!(h.widget("Disable Component Colors").is_some());
    h.step(&json!({"click": {"handle": "dropdown:inspect.colored_components"}})).unwrap();
    assert!(h.app.session.colored_components.is_none());
    let info = command.info(&h.app.session);
    assert_eq!((info.label, info.icon), ("Enable Component Colors", "colored_components"));
}
