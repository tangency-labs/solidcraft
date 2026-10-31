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
                let halo_count = lines.iter().filter(|line| line_color(&scene, *line) == halo).count();
                crate::prefs::set(&mut app, &json!({"sketch_halo": false})).unwrap();
                assert_ne!(super::sketch_scene_key(&app, &projection), enabled_key, "changing the preference rebuilds the sketch scene");
                let disabled = super::build_sketch_scene(&app, &projection);
                let disabled_bytes = if finished { &disabled.lines } else { &disabled.overlay };
                let (without_halos, _) = disabled_bytes.as_chunks::<{ crate::gpu::LINE_SIZE }>();
                let cores_only: Vec<&[u8]> = lines.iter().filter(|line| line_color(&scene, *line) != halo).map(|line| &line[..]).collect();
                assert_eq!(cores_only.len(), without_halos.len(), "off draws only the original curves");
                for (a, b) in cores_only.iter().zip(without_halos) {
                    assert_eq!(a[..24], b[..24], "same segment");
                    assert_eq!(line_color(&scene, a), line_color(&disabled, b), "same colour");
                    assert_eq!(a[28..32], b[28..32], "same width");
                }
                if !finished {
                    assert_eq!(halo_count, 0, "a sketch being edited has no halo");
                    continue;
                }
                assert!(halo_count > 5, "rectangle, circle and line emit halos");
                let width = |line: &[u8]| f32::from_le_bytes(line[28..32].try_into().unwrap());
                let projected = super::c4(tokens.sketch_projected);
                let mut cores = 0;
                let mut references = 0;
                for (index, core) in lines.iter().enumerate().filter(|(_, line)| line_color(&scene, *line) != halo) {
                    let rim = lines[..index].iter().find(|line| line[..24] == core[..24] && line_color(&scene, *line) == halo);
                    if line_color(&scene, core) == projected {
                        assert!(rim.is_none(), "the face's projected outline has no halo");
                        references += 1;
                    } else {
                        assert_eq!(line_color(&scene, core), super::c4(tokens.sketch_fixed));
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
fn face_colours_share_gpu_geometry_and_keep_transparency_and_picking_faces() {
    let (mut h, _, first) = crate::test_support::component_box();
    h.app.run("occurrence.copy", json!({"occurrence":first,"translate":[30,0,0]})).unwrap();
    h.app.run("appearance.assign", json!({"body":"Body1","faces":[[5,5,10]],"color":"#ff0000","opacity":0.4})).unwrap();
    h.app.viewport.gpu =
        Some(super::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
    h.app.ui.ground_shadow = false;
    h.app.set_origin_shown(false);
    let scene = build_scene(&h.app);
    assert!(scene.instanced.len() > 1, "the painted face and remaining triangles have separate draws");
    let tris = &scene.instanced[0].tris;
    assert!(scene.instanced.iter().all(|draw| std::sync::Arc::ptr_eq(tris, &draw.tris)), "appearances should not copy GPU geometry");
    assert!(scene.instanced.iter().any(|draw| draw.look == crate::gpu::InstanceLook::Glass));
    let expanded = scene.merged();
    let transparent: Vec<_> = expanded.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>().0.iter().collect();
    assert!(!transparent.is_empty());
    assert!(transparent.iter().all(|vertex| vertex_color(&expanded, *vertex) == [255, 0, 0, 102]));
    assert!(!expanded.tris.is_empty());
    let world = h.app.session.world_state();
    let second = world.bodies.iter().find(|b| b.name != "Body1").unwrap();
    let picked = doc::appearance::face_index_at(second, Vec3::new(35., 5., 10.)).unwrap();
    assert!(h.app.session.doc.face_colors(second).iter().any(|(index, _)| *index == picked));
    h.app.viewport.gpu.as_mut().unwrap().tess = true;
    let scene = build_scene(&h.app);
    assert!(scene.planned.is_empty());
    assert!(scene.instanced.len() > 1);
    assert!(scene.instanced.iter().all(|draw| std::sync::Arc::ptr_eq(&scene.instanced[0].tris, &draw.tris)));
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
        .add_feature(
            doc::FeatureKind::Import { file: "glass.step".into(), step: step.into(), components: Vec::new(), skip: Default::default() },
            None,
        )
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
    assert!(vertices.iter().all(|v| vertex_color(&scene, v)[3] == 102));
    app.run("appearance.assign", json!({"bodies":[source],"color":"#336699","opacity":0.7})).unwrap();
    let scene = build_scene(&app).merged();
    let (vertices, remainder) = scene.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>();
    assert!(remainder.is_empty());
    assert!(vertices.iter().all(|v| vertex_color(&scene, v)[3] == 179));
    app.run("appearance.assign", json!({"bodies":[source]})).unwrap();
    let scene = build_scene(&app).merged();
    let (vertices, remainder) = scene.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>();
    assert!(remainder.is_empty());
    assert!(vertices.iter().all(|v| vertex_color(&scene, v)[3] == 102));
}

/// The colour of a built triangle vertex: its palette index resolves through the scene.
fn vertex_color(scene: &crate::gpu::GpuScene, v: &[u8]) -> [u8; 4] {
    let index = v.get(crate::gpu::TRI_SIZE - 4..crate::gpu::TRI_SIZE).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes);
    scene.palette.get(crate::gpu::PaletteIndex(index)).unwrap_or([0; 4])
}

/// The colour of a line record: its palette index resolves through the scene.
fn line_color(scene: &crate::gpu::GpuScene, line: &[u8]) -> [u8; 4] {
    let index = line.get(24..28).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes);
    scene.palette.get(crate::gpu::PaletteIndex(index)).unwrap_or([0; 4])
}

/// The first vertex's palette index and its colour.
fn first_vertex(scene: &crate::gpu::GpuScene, vertices: &[u8]) -> (u32, Option<[u8; 4]>) {
    let index = vertices.get(24..28).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes);
    (index, scene.palette.get(crate::gpu::PaletteIndex(index)))
}

#[test]
fn selection_does_not_rebuild_the_model_scene() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.ui.ground_shadow = false;
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    h.app.viewport.gpu =
        Some(super::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
    h.frame();
    let key = h.app.viewport.key;
    let highlight = h.app.viewport.hl_key;
    // Clear the scene the first frame uploaded; an untouched model must not upload another.
    let _ = h.app.viewport.slot.lock().unwrap().take();
    let name = h.app.session.world_state().bodies[0].name.clone();
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name }];
    h.frame();
    assert_eq!(h.app.viewport.key, key, "a selection does not change the model scene key");
    assert!(h.app.viewport.slot.lock().unwrap().is_none(), "and does not rebuild it");
    assert_ne!(h.app.viewport.hl_key, highlight, "the highlight scene does rebuild");
}

#[test]
fn an_appearance_edit_resolves_only_the_bodies_it_changes() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.run("solid.box", json!({"length": 20, "width": 15, "height": 10, "body_name": "A"})).unwrap();
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "B"})).unwrap();
    let _ = build_scene(&app);
    let before = app.viewport.appearance.lock().unwrap().resolved;
    app.run("appearance.assign", json!({"bodies": ["A"], "color": "#336699"})).unwrap();
    let _ = build_scene(&app);
    let after = app.viewport.appearance.lock().unwrap().resolved;
    assert_eq!(after.saturating_sub(before), 1, "only the body whose look changed is resolved again");
}

#[test]
fn a_colour_edit_reuses_every_block_vertex() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    let name = app.session.world_state().bodies[0].name.clone();
    let first = build_scene(&app);
    let block = first.blocks.iter().find(|b| !b.bytes.vertices.is_empty()).expect("the box has a block");
    let (key, vertices) = (block.key, block.bytes.vertices.clone());
    app.run("appearance.assign", json!({"bodies": [name], "color": "#336699"})).unwrap();
    let second = build_scene(&app);
    let block = second.blocks.iter().find(|b| b.key == key).expect("the block comes back");
    assert!(std::sync::Arc::ptr_eq(&vertices, &block.bytes.vertices), "a colour edit must not rebuild a vertex");
    assert_eq!(first_vertex(&second, &block.bytes.vertices).1, Some([51, 102, 153, 255]), "the palette carries the new colour");
}

#[test]
fn a_material_colour_edit_its_undo_and_redo_reach_the_viewport() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    let name = app.session.world_state().bodies[0].name.clone();
    app.run("material.create", json!({"name": "Wood dark", "density_g_cm3": 0.7, "color": "#c8a573"})).unwrap();
    app.run("material.assign", json!({"bodies": [name], "material": "Wood dark"})).unwrap();
    let colour = |app: &SolveApp| {
        let scene = build_scene(app);
        let block = scene.blocks.iter().find(|b| !b.bytes.vertices.is_empty()).expect("the box has a block");
        first_vertex(&scene, &block.bytes.vertices).1
    };
    assert_eq!(colour(&app), Some([0xc8, 0xa5, 0x73, 255]));
    app.run("material.edit", json!({"material": "Wood dark", "color": "#ccb799"})).unwrap();
    assert_eq!(colour(&app), Some([0xcc, 0xb7, 0x99, 255]), "the edited material colour is drawn");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(colour(&app), Some([0xc8, 0xa5, 0x73, 255]), "undo draws the old colour again");
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(colour(&app), Some([0xcc, 0xb7, 0x99, 255]), "redo draws the edited colour again");
}

#[test]
fn an_opacity_class_change_keeps_vertices_and_rewrites_only_the_runs() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    let name = app.session.world_state().bodies[0].name.clone();
    let first = build_scene(&app);
    let block = first.blocks.iter().find(|b| !b.bytes.vertices.is_empty()).expect("the box has a block");
    let (key, bytes) = (block.key, block.bytes.clone());
    app.run("appearance.assign", json!({"bodies": [name], "color": "#336699", "opacity": 0.5})).unwrap();
    let second = build_scene(&app);
    let block = second.blocks.iter().find(|b| b.key == key).expect("the block key is geometry's");
    assert!(std::sync::Arc::ptr_eq(&bytes.vertices, &block.bytes.vertices), "the class change keeps every vertex");
    assert!(!std::sync::Arc::ptr_eq(&bytes.opaque, &block.bytes.opaque), "the opaque runs change");
    assert!(block.bytes.opaque.is_empty() && !block.bytes.glass.is_empty(), "the triangles moved to the glass runs");
    let triangles: u32 = block.bytes.glass.iter().map(|r| r.len() as u32).sum::<u32>() / 3;
    assert_eq!(block.bytes.glass.len(), 1, "one body of one class is one run, drawn with no index buffer");
    assert_eq!(triangles, 12, "every triangle of the box");
    let vertex = block.bytes.glass.first().map_or(0, |r| r.start) as usize;
    let at = vertex * crate::gpu::TRI_SIZE;
    let colour = first_vertex(&second, block.bytes.vertices.get(at..).unwrap_or_default()).1;
    assert_eq!(colour.map(|c| c[3]), Some(128), "0.5 of 255");
}

/// On the GPU, a colour edit writes palette rows and no geometry, also while the body is
/// see-through (the sorted glass stream of the no-OIT path is kept).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn appearance_edits_upload_palette_rows_only() {
    const SIZE: u32 = 64;
    let Some(mut frames) = crate::gpu::OffscreenFrames::new(SIZE, false) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.viewport.gpu =
        Some(crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
    app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    app.fit_view();
    let name = app.session.world_state().bodies[0].name.clone();
    let highlight: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut key = 0;
    let mut draw = |app: &SolveApp| {
        key += 1;
        let slot: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(Some(build_scene(app))));
        frames.frame(crate::scale_tests::view_frame(app, SIZE, (key, &slot), (0, &highlight)));
        frames.uploads()
    };
    let cold = draw(&app);
    assert!(cold.geometry > 0 && cold.palette > 0, "the first scene uploads its geometry and palette: {cold:?}");
    app.run("appearance.assign", json!({"bodies": [&name], "color": "#336699"})).unwrap();
    let colour = draw(&app);
    assert_eq!(colour.geometry, 0, "a colour edit uploads no geometry: {colour:?}");
    assert!(colour.palette > 0, "it writes the palette: {colour:?}");
    app.run("appearance.assign", json!({"bodies": [&name], "color": "#336699", "opacity": 0.5})).unwrap();
    let class = draw(&app);
    assert!(class.geometry <= 12 * 3 * crate::gpu::TRI_SIZE as u64, "a class change uploads at most the sorted glass stream: {class:?}");
    app.run("appearance.assign", json!({"bodies": [&name], "color": "#993366", "opacity": 0.5})).unwrap();
    let glass = draw(&app);
    assert_eq!(glass.geometry, 0, "a see-through body's colour edit uploads no geometry: {glass:?}");
    assert!(glass.palette > 0, "it writes the palette: {glass:?}");
}

/// A body out of the view is left out of its block's draws: two boxes far apart, drawn with
/// both in view and then with the camera on one of them, draw half the triangles. Skipped
/// without a GPU adapter.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bodies_out_of_the_view_are_not_drawn() {
    const SIZE: u32 = 64;
    let Some(mut frames) = crate::gpu::OffscreenFrames::new(SIZE, false) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.viewport.gpu = frames.target();
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new"})).unwrap();
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [1000, 0, 0], "operation": "new"})).unwrap();
    app.fit_view();
    let scene = build_scene(&app);
    let spans: usize = scene.blocks.iter().map(|b| b.bytes.bodies.len()).sum();
    assert_eq!(spans, 2, "each body's box is kept with its block");
    let slot: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(Some(scene)));
    let none: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    frames.frame(crate::scale_tests::view_frame(&app, SIZE, (1, &slot), (0, &none)));
    let both = frames.stats().triangles;
    assert_eq!(both, 24, "both boxes in view");
    app.cam.target = solidcraft_engine::geom::Vec3::new(5.0, 5.0, 5.0);
    app.cam.distance = 40.0;
    frames.frame(crate::scale_tests::view_frame(&app, SIZE, (1, &slot), (0, &none)));
    assert_eq!(frames.stats().triangles, 12, "the far box is not drawn");
    app.fit_view();
    frames.frame(crate::scale_tests::view_frame(&app, SIZE, (1, &slot), (0, &none)));
    assert_eq!(frames.stats().triangles, both, "and drawn again once in view");
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
    h.app.viewport.gpu =
        Some(super::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
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
        assert!(vertices.iter().all(|v| vertex_color(&scene, v) == colour), "{theme}: stale body colour");
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
        assert!(vertices.iter().all(|v| vertex_color(&scene, v) == [51, 102, 153, 255]), "{theme}: explicit appearance preserved");
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

/// Every shape the last frame painted in `colour`: point squares, glyph outlines, dimension
/// lines and arrows.
fn painted_in(shapes: &[egui::epaint::ClippedShape], colour: egui::Color32) -> usize {
    fn walk(shape: &egui::Shape, colour: egui::Color32) -> usize {
        let solid = |c: &egui::epaint::ColorMode| matches!(c, egui::epaint::ColorMode::Solid(x) if *x == colour);
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().map(|s| walk(s, colour)).sum(),
            egui::Shape::Rect(r) => usize::from(r.fill == colour || r.stroke.color == colour),
            egui::Shape::Circle(c) => usize::from(c.fill == colour || c.stroke.color == colour),
            egui::Shape::LineSegment { stroke, .. } => usize::from(stroke.color == colour),
            egui::Shape::Path(p) => usize::from(p.fill == colour || solid(&p.stroke.color)),
            _ => 0,
        }
    }
    shapes.iter().map(|s| walk(&s.shape, colour)).sum()
}

#[test]
fn conflicting_curves_points_glyphs_and_dimensions_take_the_conflict_colour_until_the_conflict_is_gone() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    // A conflict colour nothing else in a sketch uses: every line and shape in it is a conflict's.
    let marker = egui::Color32::from_rgb(0x12, 0xab, 0x34);
    h.call(
        "ui.prefs",
        json!({"theme_import": {"format":"solidcraft-theme/1","name":"Conflict marker","base":"light","colors":{"sketch_conflict":"#12ab34"}}}),
    );
    h.frame();
    assert_eq!(crate::theme::Tokens::get().sketch_conflict, marker);
    // `w` places a guide line; a second sketch ends a 12 mm line on it. Moving the guide out of
    // reach makes the second sketch's constraints conflict (#7244).
    let app = &mut h.app;
    app.run("parameters.add", json!({"name": "w", "expression": "10 mm"})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "Guide"})).unwrap();
    app.run("sketch.line", json!({"points": [[10, -30], [10, 30]], "ids": ["guide"], "connect": false})).unwrap();
    app.run("sketch.constraint.horizontal_vertical", json!({"line": "guide", "mode": "vertical"})).unwrap();
    app.run("sketch.dimension", json!({"entities": ["origin", "guide.start"], "type": "horizontal", "value": "w"})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let reach = app.run("sketch.create", json!({"plane": "XY", "name": "Reach"})).unwrap()["sketch"].as_u64().unwrap();
    let projected = app.run("sketch.project", json!({"refs": [{"sketch": "Guide", "curve": "guide"}]})).unwrap()["links"][0]["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let line = app.run("sketch.line", json!({"points": [[0, 0], [10, -(44.0_f64).sqrt()]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
    app.run("sketch.dimension", json!({"entities": [line], "value": 12})).unwrap();
    app.run("sketch.constraint.coincident", json!({"a": format!("{line}.end"), "b": projected})).unwrap();
    // A second, unrelated line: never part of the conflict.
    let other = app.run("sketch.line", json!({"points": [[-20, 5], [-5, 5]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
    app.run("sketch.finish", json!({})).unwrap();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let conflict_lines = |app: &SolveApp| {
        let scene = super::build_sketch_scene(app, &super::projection(app, rect));
        let (lines, _) = scene.overlay.as_chunks::<{ crate::gpu::LINE_SIZE }>();
        // A line names its colour by palette index (bytes 24..28).
        lines
            .iter()
            .filter_map(|line| line.get(24..28)?.try_into().ok().map(u32::from_le_bytes))
            .filter(|&i| scene.palette.get(crate::gpu::PaletteIndex(i)) == Some(super::c4(marker)))
            .count()
    };
    for (w, conflicting, preference) in [("10 mm", false, true), ("20 mm", true, true), ("20 mm", true, false), ("10 mm", false, true)] {
        h.app.preferences.sketch_conflict_colour = preference;
        if h.app.session.active_sketch.is_some() {
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.app.run("parameters.change", json!({"name": "w", "expression": w})).unwrap();
        h.app.edit_sketch(reach);
        h.settle();
        assert_eq!(h.app.session.active_sketch, Some(reach));
        let st = h.app.session.world_state();
        let ss = st.sketch(reach).unwrap();
        assert_eq!(!ss.report.ok(), conflicting, "w = {w}: {:?}", ss.report);
        let conflict = super::Conflict::of(&h.app, &ss.sketch, &ss.report);
        let failing: Vec<_> = ss.sketch.constraints.iter().filter(|c| ss.report.failing.contains(&c.id)).collect();
        assert_eq!(failing.is_empty(), !conflicting, "w = {w}: {:?}", ss.report);
        // With the preference off, nothing is in the conflict colour: the failing glyphs and
        // dimensions are red (`error`), as before.
        let failing: Vec<_> = if preference { failing } else { Vec::new() };
        // Exactly the curves and points the failing constraints hold.
        for (i, curve) in ss.sketch.curves.iter().enumerate() {
            let held = failing.iter().any(|c| c.kind.indices().1.contains(&i));
            assert_eq!(conflict.curve(i), held, "w = {w}: curve {}", curve.id);
        }
        for i in 0..ss.sketch.points.len() {
            assert_eq!(conflict.point(i), failing.iter().any(|c| c.kind.indices().0.contains(&i)), "w = {w}: point {i}");
        }
        let other = ss.sketch.curve_index(&other).unwrap();
        assert!(!conflict.curve(other));
        // Drawn: the held curves' lines, and the held points, the failing glyphs and dimensions.
        let (lines, shapes) = (conflict_lines(&h.app), painted_in(&h.shapes, marker));
        if conflicting && preference {
            assert!(lines > 0, "the held curves are drawn in the conflict colour");
            assert!(shapes > 0, "points, glyphs or dimensions in the conflict colour");
        } else {
            assert_eq!((lines, shapes), (0, 0), "w = {w}, preference {preference}: nothing in the conflict colour");
        }
        if conflicting && !preference {
            let error = crate::theme::Tokens::get().error;
            assert!(painted_in(&h.shapes, error) > 0, "the failing glyphs and dimensions stay red");
        }
    }
    h.call("ui.prefs", json!({"theme": "dark"}));
    h.frame();
}

/// Vertex markers follow the edges (#7834): a box's eight corners are marked once each in the
/// edge colour wherever edges are drawn (shaded with edges, wireframe, shaded with hidden
/// edges), faintly behind the model where hidden edges are, and not at all in plain shaded.
#[test]
fn vertex_markers_follow_the_edges_display_mode() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.ui.ground_shadow = false;
    app.set_origin_shown(false);
    app.run("solid.box", json!({"length": 20, "width": 15, "height": 10})).unwrap();
    // Off by default: no marker in any style.
    assert!(!app.preferences.vertex_markers, "vertex markers are off by default");
    for style in 0..4 {
        app.ui.visual_style = style;
        let scene = build_scene(&app).merged();
        let dots = |bytes: &[u8]| bytes.as_chunks::<{ crate::gpu::LINE_SIZE }>().0.iter().filter(|r| r[..12] == r[12..24]).count();
        assert_eq!((dots(&scene.lines), dots(&scene.hidden)), (0, 0), "style {style}: no markers by default");
    }
    app.preferences.vertex_markers = true;
    // A marker is a lifted line from a point to itself.
    let markers = |scene: &crate::gpu::GpuScene, bytes: &[u8]| -> Vec<([u8; 12], [u8; 4])> {
        let (records, rest) = bytes.as_chunks::<{ crate::gpu::LINE_SIZE }>();
        assert!(rest.is_empty());
        records.iter().filter(|r| r[..12] == r[12..24] && r[31] & 0x80 != 0).map(|r| (r[..12].try_into().unwrap(), line_color(scene, r))).collect()
    };
    for (style, drawn, hidden) in [(0, true, false), (1, false, false), (2, true, false), (3, true, true)] {
        app.ui.visual_style = style;
        let scene = build_scene(&app).merged();
        let (front, back) = (markers(&scene, &scene.lines), markers(&scene, &scene.hidden));
        let edge = scene.lines.as_chunks::<{ crate::gpu::LINE_SIZE }>().0.iter().find(|r| r[..12] != r[12..24]).map(|r| line_color(&scene, r));
        assert_eq!(front.len(), if drawn { 8 } else { 0 }, "style {style}: one marker per corner");
        assert_eq!(back.len(), if hidden { 8 } else { 0 }, "style {style}: hidden markers only with hidden edges");
        let corners: std::collections::HashSet<_> = front.iter().map(|(p, _)| *p).collect();
        assert_eq!(corners.len(), front.len(), "style {style}: each corner once");
        assert!(front.iter().all(|(_, c)| Some(*c) == edge), "style {style}: markers in the edge colour");
    }
}
