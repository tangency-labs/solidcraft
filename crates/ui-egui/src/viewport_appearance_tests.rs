use super::build_scene;
use crate::{Services, SolveApp};
use serde_json::json;
use solidcraft_engine::{Session, doc, geom::Vec3, kernel};

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
    app.ui.show_origin = false;
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
    h.app.ui.show_origin = false;
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
    let original_key = super::scene_key(&h.app);
    h.step(&json!({"click": {"handle": "panel:SOLID:INSPECT"}})).unwrap();
    h.frames(2);
    assert!(h.widget("Enable Component Colors").is_some());
    h.step(&json!({"click": {"handle": "dropdown:inspect.colored_components"}})).unwrap();
    assert!(h.app.session.colored_components.is_some());
    assert!(h.app.dialog.is_none());
    assert_ne!(super::scene_key(&h.app), original_key);
    let info = command.info(&h.app.session);
    assert_eq!((info.label, info.icon), ("Disable Component Colors", "colored_components_off"));
    h.step(&json!({"click": {"handle": "panel:SOLID:INSPECT"}})).unwrap();
    h.frames(2);
    assert!(h.widget("Disable Component Colors").is_some());
    h.step(&json!({"click": {"handle": "dropdown:inspect.colored_components"}})).unwrap();
    assert!(h.app.session.colored_components.is_none());
    let info = command.info(&h.app.session);
    assert_eq!((info.label, info.icon), ("Enable Component Colors", "colored_components"));
}
