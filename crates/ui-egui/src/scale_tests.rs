//! How the cost of a frame and of cheap commands grows with the number of bodies (#3044): a
//! design with many bodies must not make every frame or every click slower per body squared.

use serde_json::json;

use crate::scenario::Harness;

/// A design of `n` boxes (ten patterns of a box each), in a headless app, settled.
fn grid(n: usize) -> Harness {
    let mut h = Harness::new();
    let per = n.div_ceil(10).max(1);
    for k in 0..n.div_ceil(per) {
        let count = per.min(n - k * per);
        h.app
            .run("solid.box", json!({"length": 4, "width": 4, "height": 4, "center": [0, 6 * k, 2], "operation": "new", "name": format!("Seed{k}")}))
            .unwrap();
        if count > 1 {
            h.app
                .run(
                    "solid.pattern.rectangular",
                    json!({"features": [format!("Seed{k}")], "dir1": [1, 0, 0], "count1": count, "spacing1": 6, "name": format!("Row{k}")}),
                )
                .unwrap();
        }
    }
    assert_eq!(h.app.session.model.state().bodies.len(), n);
    h.frames(3);
    h
}

fn ms(f: impl FnOnce()) -> f64 {
    let t = std::time::Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

/// Times per body count; run with `--ignored --nocapture` (not a pass/fail check).
#[test]
#[ignore = "manual scaling measurement"]
fn frame_and_cheap_command_cost_by_body_count() {
    for n in [50, 500, 5000] {
        let mut h = grid(n);
        h.app.session.world_state().warm_meshes(|| true);
        let scene_cold = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let scene_again = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let idle = ms(|| h.frames(10)) / 10.0;
        h.app.tree.expanded.insert("c0/bodies".into());
        h.frames(2);
        let expanded = ms(|| h.frames(10)) / 10.0;
        let rename = ms(|| {
            h.app.run("body.rename", json!({"body": "Body7", "name": "Renamed"})).unwrap();
            h.frame();
        });
        let scene_renamed = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let select = ms(|| {
            h.app.run("select.set", json!({"items": [{"type": "body", "name": "Renamed"}]})).unwrap();
            h.frame();
        });
        let highlight = ms(|| drop(crate::viewport::build_highlight(&h.app)));
        let hide = ms(|| {
            h.app.run("browser.visibility", json!({"visible": false, "items": ["Renamed"]})).unwrap();
            h.frame();
        });
        let undo = ms(|| {
            h.app.run("edit.undo", json!({})).unwrap();
            h.frame();
        });
        println!(
            "SCALE n={n} scene_cold_ms={scene_cold:.2} scene_again_ms={scene_again:.2} scene_renamed_ms={scene_renamed:.2} highlight_ms={highlight:.2} idle_frame_ms={idle:.2} expanded_frame_ms={expanded:.2} rename_ms={rename:.2} select_ms={select:.2} hide_ms={hide:.2} undo_ms={undo:.2}"
        );
    }
}

/// Times the model scene build after the edits that used to rebuild and upload everything:
/// selection (a revision bump with no model change), an appearance colour and an opacity class
/// change, a theme switch, and one body's geometry edit. Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual palette measurement"]
fn appearance_and_selection_rebuild_cost() {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    for n in [200usize, 1000, 5000] {
        let mut h = grid(n);
        h.app.session.world_state().warm_meshes(|| true);
        h.frames(2);
        let body = h.app.session.world_state().bodies[0].name.clone();
        let cold = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let again = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let proj = crate::viewport::projection(&h.app, rect);
        let key = crate::viewport::display_scene_keys(&h.app, &proj).0;
        h.app.run("select.set", json!({"items":[{"type":"body","name":body}]})).unwrap();
        let selection_key_changed = crate::viewport::display_scene_keys(&h.app, &proj).0 != key;
        let selection = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("select.clear", json!({})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":[body.clone()],"color":"#336699"})).unwrap();
        let colour = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("appearance.assign", json!({"bodies":[body.clone()],"color":"#336699","opacity":0.5})).unwrap();
        let opacity_class = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let theme_name = if crate::theme::is_dark() { "light" } else { "dark" };
        let _ = h.call("ui.prefs", json!({"theme": theme_name}));
        let theme = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let geometry = match h.app.run("solid.move", json!({"bodies":[body.clone()], "translate":[1.0, 0.0, 0.0]})) {
            Ok(_) => ms(|| drop(crate::viewport::build_scene(&h.app))),
            Err(e) => {
                println!("MEASURE n={n} geometry edit refused: {e}");
                f64::NAN
            }
        };
        println!(
            "MEASURE n={n} cold_ms={cold:.1} unchanged_ms={again:.1} selection_ms={selection:.1} selection_rebuilds={selection_key_changed} colour_ms={colour:.1} opacity_class_ms={opacity_class:.1} theme_ms={theme:.1} geometry_ms={geometry:.1}"
        );
    }
}

/// The same edits on a design of few, large meshes: the case where rebuilding a block rebuilt and
/// re-uploaded a large part of the model. Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual palette measurement"]
fn large_body_scene_rebuild_cost() {
    for n in [20usize, 100] {
        let mut h = Harness::new();
        for k in 0..n {
            h.app
                .run("solid.torus", json!({"major": 100, "minor": 10, "center": [k as f64 * 220.0, 0.0, 0.0], "body_name": format!("T{k}")}))
                .unwrap();
        }
        h.app.session.world_state().warm_meshes(|| true);
        h.frames(2);
        let bodies = h.app.session.world_state().bodies.len();
        let triangles: usize = h.app.session.world_state().bodies.iter().map(|b| b.mesh().triangles.len()).sum();
        let body = h.app.session.world_state().bodies[0].name.clone();
        let cold = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("appearance.assign", json!({"bodies":[body.clone()],"color":"#336699"})).unwrap();
        let colour = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("appearance.assign", json!({"bodies":[body.clone()],"color":"#336699","opacity":0.5})).unwrap();
        let opacity_class = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let theme_name = if crate::theme::is_dark() { "light" } else { "dark" };
        let _ = h.call("ui.prefs", json!({"theme": theme_name}));
        let theme = ms(|| drop(crate::viewport::build_scene(&h.app)));
        println!(
            "LARGE n={n} bodies={bodies} triangles={triangles} cold_ms={cold:.1} colour_ms={colour:.1} opacity_class_ms={opacity_class:.1} theme_ms={theme:.1}"
        );
    }
}

/// An assembly: `unique` detailed parts (a patterned torus, ~8,400 triangles each) and one
/// detailed component placed `placed` times (drawn as instances). Settled, with warm meshes.
fn assembly(unique: usize, placed: usize) -> Harness {
    let mut h = Harness::new();
    let per = |n: usize| (n as f64).sqrt().ceil().max(1.0) as usize;
    h.app.run("solid.torus", json!({"major": 40, "minor": 6, "center": [0, 0, 0], "body_name": "Part"})).unwrap();
    let (c1, c2) = (per(unique), unique.div_ceil(per(unique)));
    h.app
        .run(
            "solid.pattern.rectangular",
            json!({"bodies": ["Part"], "dir1": [1, 0, 0], "count1": c1, "spacing1": 100, "dir2": [0, 1, 0], "count2": c2, "spacing2": 100}),
        )
        .unwrap();
    h.app.run("solid.torus", json!({"major": 40, "minor": 6, "center": [0, 0, -200], "body_name": "Wheel"})).unwrap();
    h.app.run("component.from_bodies", json!({"bodies": ["Wheel"]})).unwrap();
    let (p1, p2) = (per(placed), placed.div_ceil(per(placed)));
    h.app
        .run(
            "solid.pattern.rectangular",
            json!({"components": ["Wheel"], "dir1": [1, 0, 0], "count1": p1, "spacing1": 100, "dir2": [0, 1, 0], "count2": p2, "spacing2": 100}),
        )
        .unwrap();
    h.app.session.world_state().warm_meshes(|| true);
    h.frames(3);
    h
}

/// The edits made on an assembly, timed on the model scene build: selection, a part's colour and
/// opacity, a component's colour (every placement), a theme switch, hiding a part, and moving one
/// part. Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual palette measurement"]
fn assembly_scene_rebuild_cost() {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    for (unique, placed) in [(100, 100), (300, 500)] {
        let mut h = assembly(unique, placed);
        let st = h.app.session.world_state();
        let bodies = st.bodies.len();
        let triangles: usize = st.bodies.iter().map(|b| b.mesh().triangles.len()).sum();
        let part = st.bodies.iter().find(|b| b.component == 0).map(|b| b.name.clone()).unwrap();
        drop(st);
        let cold = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let unchanged = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let proj = crate::viewport::projection(&h.app, rect);
        let key = crate::viewport::display_scene_keys(&h.app, &proj).0;
        h.app.run("select.set", json!({"items": [{"type": "body", "name": part}]})).unwrap();
        let selection_rebuilds = crate::viewport::display_scene_keys(&h.app, &proj).0 != key;
        let selection = if selection_rebuilds { ms(|| drop(crate::viewport::build_scene(&h.app))) } else { 0.0 };
        h.app.run("select.clear", json!({})).unwrap();
        // The command itself (the engine's refresh) and then the scene build.
        let colour_command = ms(|| drop(h.app.run("appearance.assign", json!({"bodies": [part.clone()], "color": "#336699"})).unwrap()));
        let colour = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("appearance.assign", json!({"bodies": [part.clone()], "color": "#336699", "opacity": 0.5})).unwrap();
        let opacity_class = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("appearance.assign", json!({"components": ["Wheel"], "color": "#cc6633"})).unwrap();
        let component_colour = ms(|| drop(crate::viewport::build_scene(&h.app)));
        let theme_name = if crate::theme::is_dark() { "light" } else { "dark" };
        let _ = h.call("ui.prefs", json!({"theme": theme_name}));
        let theme = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("browser.visibility", json!({"visible": false, "items": [part.clone()]})).unwrap();
        let hide = ms(|| drop(crate::viewport::build_scene(&h.app)));
        h.app.run("browser.visibility", json!({"visible": true, "items": [part.clone()]})).unwrap();
        let _ = crate::viewport::build_scene(&h.app);
        let geometry = match h.app.run("solid.move", json!({"bodies": [part.clone()], "translate": [1.0, 0.0, 0.0]})) {
            Ok(_) => ms(|| drop(crate::viewport::build_scene(&h.app))),
            Err(_) => f64::NAN,
        };
        println!(
            "ASSEMBLY unique={unique} placed={placed} bodies={bodies} triangles={triangles} cold_ms={cold:.1} unchanged_ms={unchanged:.1} selection_ms={selection:.1} colour_command_ms={colour_command:.1} colour_ms={colour:.1} opacity_class_ms={opacity_class:.1} component_colour_ms={component_colour:.1} theme_ms={theme:.1} hide_ms={hide:.1} geometry_ms={geometry:.1}"
        );
    }
}

/// Hovering the document tree on an assembly: the highlight scene built as the pointer moves
/// from row to row (one part, one placement of the component, the component's every placement,
/// the whole design), then back over rows it has lit before. Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual highlight measurement"]
fn assembly_tree_hover_highlight_cost() {
    for (unique, placed) in [(100, 100), (300, 500)] {
        let mut h = assembly(unique, placed);
        let _ = crate::viewport::build_scene(&h.app);
        let st = h.app.session.world_state();
        let part = st.bodies.iter().find(|b| b.component == 0).map(|b| b.name.clone()).unwrap();
        let wheels: Vec<String> = st.bodies.iter().filter(|b| b.component != 0).map(|b| b.name.clone()).collect();
        let everything: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
        drop(st);
        let rows = [("part", vec![part.clone()]), ("placement", wheels[..1].to_vec()), ("component", wheels.clone()), ("design", everything.clone())];
        let mut line = format!("HOVER unique={unique} placed={placed}");
        for (name, bodies) in rows.iter() {
            h.app.viewport.hover_bodies = bodies.clone();
            let first = ms(|| drop(crate::viewport::build_highlight(&h.app)));
            line.push_str(&format!(" {name}_ms={first:.2}"));
        }
        // Moving back and forth between two rows, as the pointer runs down the tree.
        let mut again = Vec::new();
        for i in 0..20 {
            h.app.viewport.hover_bodies = if i % 2 == 0 { vec![part.clone()] } else { wheels.clone() };
            again.push(ms(|| drop(crate::viewport::build_highlight(&h.app))));
        }
        again.sort_by(f64::total_cmp);
        line.push_str(&format!(" back_and_forth_median_ms={:.2}", again[again.len() / 2]));
        h.app.viewport.hover_bodies.clear();
        let none = ms(|| drop(crate::viewport::build_highlight(&h.app)));
        line.push_str(&format!(" none_ms={none:.2}"));
        println!("{line}");
    }
}

/// A frame of the viewport for `app`'s camera, drawing the given model and highlight scenes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn view_frame(
    app: &crate::SolveApp,
    size: u32,
    model: (u64, &crate::gpu::SceneSlot),
    highlight: (u64, &crate::gpu::SceneSlot),
) -> crate::gpu::Frame {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size as f32, size as f32));
    let proj = crate::viewport::projection(app, rect);
    let e = proj.cam.eye().to_f32();
    let t = crate::theme::Tokens::get();
    let none = || std::sync::Arc::new(std::sync::Mutex::new(None));
    crate::gpu::Frame {
        product: false,
        product_lights: None,
        upload_error: None,
        key: model.0,
        slot: model.1.clone(),
        sketch_key: 0,
        sketch_slot: none(),
        hl_key: highlight.0,
        hl_slot: highlight.1.clone(),
        pv_key: 0,
        pv_slot: none(),
        pv_hide: false,
        view_proj: proj.vp.to_f32(),
        back: proj.cam.back().to_f32(),
        size_px: [size as f32; 2],
        clip: [0.0; 4],
        cap: [0.0; 4],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis: [0.0; 8],
        analysis_colors: crate::gpu::analysis_colors(&t.analysis),
        access: None,
        outline: false,
        eye: [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }],
        ao_radius: 0.0,
        ao_samples: solidcraft_gpu::view::AO_SAMPLES,
        sky: [0.0; 4],
        ssaa: 1.0,
        hover_tint: [f32::from(t.hover_face_lift) / 255.0; 3],
        selection_tint: [0.4, 0.6, 0.9],
    }
}

/// Time hovering four rows of `h`'s tree (one part, one placement, every placement of the
/// component, the whole design), each hover change building, uploading and drawing the highlight.
#[cfg(not(target_arch = "wasm32"))]
fn hover_rows(h: &mut Harness, frames: &mut crate::gpu::OffscreenFrames, size: u32, model_key: u64, label: &str) -> String {
    let st = h.app.session.world_state();
    let part = st.bodies.iter().find(|b| b.component == 0).map(|b| b.name.clone()).unwrap();
    let wheels: Vec<String> = st.bodies.iter().filter(|b| b.component != 0).map(|b| b.name.clone()).collect();
    let everything: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
    drop(st);
    let model: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(Some(crate::viewport::build_scene(&h.app))));
    let highlight: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let cold = frames.frame(view_frame(&h.app, size, (model_key, &model), (model_key, &highlight)));
    let mut still = Vec::new();
    for _ in 0..5 {
        still.push(frames.frame(view_frame(&h.app, size, (model_key, &model), (model_key, &highlight))));
    }
    still.sort_by(f64::total_cmp);
    let mut line = format!("{label} cold_ms={cold:.1} still_ms={:.2} triangles={}", still[2], frames.stats().triangles);
    let rows = [("part", vec![part]), ("placement", wheels[..1].to_vec()), ("component", wheels.clone()), ("design", everything)];
    let mut key = model_key;
    for (name, bodies) in rows.iter() {
        let mut times = Vec::new();
        for _ in 0..7 {
            // Off the row and back on: each time a new highlight scene, as a sweep does.
            for hovered in [Vec::new(), bodies.clone()] {
                h.app.viewport.hover_bodies = hovered;
                key += 1;
                let built = std::time::Instant::now();
                *highlight.lock().unwrap() = Some(crate::viewport::build_highlight(&h.app));
                let build_ms = built.elapsed().as_secs_f64() * 1000.0;
                let ms = frames.frame(view_frame(&h.app, size, (model_key, &model), (key, &highlight)));
                if !h.app.viewport.hover_bodies.is_empty() {
                    times.push((build_ms + ms, build_ms));
                }
            }
        }
        times.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (total, build) = times[times.len() / 2];
        line.push_str(&format!(" {name}_ms={total:.2} {name}_build_ms={build:.2}"));
    }
    h.app.viewport.hover_bodies.clear();
    line
}

/// Hovering the document tree on an assembly, as the window draws it: each hover change builds
/// the highlight scene, uploads it and draws the frame (GPU work included), on an offscreen
/// renderer with GPU tessellation where the device has it. Then the same with the component
/// active (every other part faded, as when working in context) and with see-through parts.
/// Skipped without a GPU adapter. Run with `--ignored --nocapture`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "manual highlight measurement"]
fn assembly_tree_hover_frame_cost() {
    const SIZE: u32 = 1024;
    let Some(mut frames) = crate::gpu::OffscreenFrames::new(SIZE, true) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    for (design, (unique, placed)) in [(100, 100), (300, 500)].into_iter().enumerate() {
        // Keys of their own for each scene: the renderer keeps a scene while its key comes back.
        let base = 1_000_000 * (3 * design as u64 + 1);
        let mut h = assembly(unique, placed);
        h.app.viewport.gpu = frames.target();
        h.app.fit_view();
        let label = format!("HOVERFRAME unique={unique} placed={placed} tess={}", frames.tessellates());
        println!("{}", hover_rows(&mut h, &mut frames, SIZE, base, &format!("{label} mode=plain")));
        h.app.run("component.activate", json!({"component": "Wheel"})).unwrap();
        println!("{}", hover_rows(&mut h, &mut frames, SIZE, base + 1_000_000, &format!("{label} mode=component_active")));
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        let parts: Vec<String> = h.app.session.world_state().bodies.iter().filter(|b| b.component == 0).map(|b| b.name.clone()).collect();
        h.app.run("appearance.assign", json!({"bodies": parts, "color": "#88aacc", "opacity": 0.5})).unwrap();
        println!("{}", hover_rows(&mut h, &mut frames, SIZE, base + 2_000_000, &format!("{label} mode=glass_parts")));
    }
}

/// A design from a file (`SOLIDCRAFT_SCALE_DESIGN`), every browser folder open: the time of an
/// idle frame and of the cheap commands, as [`frame_and_cheap_command_cost_by_body_count`].
#[test]
#[ignore = "manual measurement on a design file"]
fn frame_cost_of_a_design_file() {
    let Some(path) = std::env::var_os("SOLIDCRAFT_SCALE_DESIGN") else { return };
    let bytes = std::fs::read(&path).unwrap();
    let doc = solidcraft_engine::io::read_design(&bytes).unwrap();
    let mut h = Harness::new();
    h.app.session = solidcraft_engine::Session::new(doc);
    h.app.session.world_state().warm_meshes(|| true);
    h.frames(3);
    let idle = ms(|| h.frames(5)) / 5.0;
    let keys: Vec<String> =
        h.app.session.doc.components.iter().flat_map(|c| ["bodies", "components", "occurrences"].map(|f| format!("c{}/{f}", c.id))).collect();
    h.app.tree.expanded.extend(keys);
    h.frames(2);
    let expanded = ms(|| h.frames(5)) / 5.0;
    let scene = ms(|| drop(crate::viewport::build_scene(&h.app)));
    let scene_again = ms(|| drop(crate::viewport::build_scene(&h.app)));
    let bodies = h.app.session.world_state().bodies.len();
    let rename = ms(|| {
        h.app.run("body.rename", json!({"body": "Body1", "name": "Renamed"})).unwrap();
        h.frame();
    });
    let scene_renamed = ms(|| drop(crate::viewport::build_scene(&h.app)));
    println!(
        "DESIGN bodies={bodies} idle_frame_ms={idle:.2} expanded_frame_ms={expanded:.2} scene_ms={scene:.2} scene_again_ms={scene_again:.2} rename_ms={rename:.2} scene_renamed_ms={scene_renamed:.2}"
    );
    let t = std::time::Instant::now();
    while t.elapsed().as_secs() < std::env::var("SOLIDCRAFT_SCALE_LOOP").ok().and_then(|v| v.parse().ok()).unwrap_or(0) {
        h.frame();
    }
}

/// A design from a file (`SOLIDCRAFT_SCALE_DESIGN`) open in two tabs, both with an unsaved change
/// the comparison with the saved design finds only at its end (a favourite, as a changed
/// appearance is): the time of an idle frame, and of one unsaved-changes check
/// (`Session::is_dirty`, which the tab strip and the autosave ask every frame).
#[test]
#[ignore = "manual measurement on a design file"]
fn idle_frame_cost_of_two_unsaved_tabs() {
    let Some(path) = std::env::var_os("SOLIDCRAFT_SCALE_DESIGN") else { return };
    let path = path.to_string_lossy().to_string();
    let open = || {
        let (doc, saved) = solidcraft_engine::io::read_design_file(&path, None::<fn(solidcraft_engine::io::DesignPreview)>).unwrap();
        let mut s = solidcraft_engine::Session::opened(doc, saved.as_ref());
        s.doc_mut().favorites.insert("idle-measurement".into());
        s.revision += 1;
        s
    };
    let mut h = Harness::new();
    h.app.session = open();
    crate::documents::open_tab(&mut h.app, open()).unwrap();
    assert!(h.app.session.is_dirty());
    h.frames(5);
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        (v[v.len() / 2], v[0], v[v.len() - 1])
    };
    let (frame, frame_min, frame_max) = median((0..30).map(|_| ms(|| h.frame())).collect());
    let (dirty, dirty_min, dirty_max) = median((0..30).map(|_| ms(|| assert!(std::hint::black_box(h.app.session.is_dirty())))).collect());
    let bodies = h.app.session.model.state().bodies.len();
    println!(
        "IDLE2TABS bodies={bodies} idle_frame_ms={frame:.3} ({frame_min:.3}-{frame_max:.3}) is_dirty_ms={dirty:.4} ({dirty_min:.4}-{dirty_max:.4})"
    );
}

/// A whole assembly's hover highlight: the model's own triangles (its display plans on the GPU)
/// against the copy of every triangle it used to build here. Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual scaling measurement"]
fn whole_body_highlight_cost_from_the_models_own_triangles() {
    let mut h = Harness::new();
    let mut names = Vec::new();
    // Each torus holds some 8k triangles at Normal quality: twenty make a large assembly.
    for k in 0..20 {
        let name = format!("Torus{k}");
        h.app.run("solid.torus", json!({"major": 100, "minor": 10, "center": [k as f64 * 220.0, 0.0, 0.0], "body_name": name.clone()})).unwrap();
        names.push(name);
    }
    let st = h.app.session.world_state();
    let triangles: usize = st.bodies.iter().map(|b| b.mesh().triangles.len()).sum();
    st.warm_meshes(|| true);
    for b in &st.bodies {
        let _ = b.display_plan();
    }
    let bodies = names.len();
    h.app.viewport.hover_bodies = names;
    let copied = ms(|| drop(crate::viewport::build_highlight(&h.app)));
    h.app.viewport.gpu =
        Some(crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: true, oit: false });
    let planned = ms(|| drop(crate::viewport::build_highlight(&h.app)));
    println!("HIGHLIGHT bodies={bodies} triangles={triangles} cpu_copy_ms={copied:.2} model_triangles_ms={planned:.2}");
}

/// A STEP file (`SOLIDCRAFT_SCALE_STEP`), opened as the app opens one (a new design and
/// `file.insert_step`): the time of each phase of opening it, of an idle frame with its tree, of
/// the model scene and of appearance edits on it, and of hovering its bodies on an offscreen GPU
/// (skipped without an adapter). Run with `--ignored --nocapture`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "manual measurement on a STEP file"]
fn step_file_cost() {
    let Some(path) = std::env::var_os("SOLIDCRAFT_SCALE_STEP") else { return };
    let mut h = Harness::new();
    let mut line = String::from("STEP");
    let phase = |line: &mut String, name: &str, ms: f64| {
        line.push_str(&format!(" {name}_ms={ms:.1}"));
        eprintln!("{name}: {ms:.1} ms");
    };
    let import = ms(|| drop(h.app.run("file.insert_step", json!({"path": path.to_string_lossy()})).unwrap()));
    phase(&mut line, "import", import);
    let world = ms(|| drop(h.app.session.world_state()));
    phase(&mut line, "world", world);
    let mesh = ms(|| h.app.session.world_state().warm_meshes(|| true));
    phase(&mut line, "mesh", mesh);
    let st = h.app.session.world_state();
    let bodies: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
    let triangles: usize = st.bodies.iter().map(|b| b.mesh().triangles.len()).sum();
    let components = h.app.session.doc.components.len();
    let occurrences = h.app.session.doc.occurrences.len();
    drop(st);
    line.push_str(&format!(" bodies={} triangles={triangles} components={components} occurrences={occurrences}", bodies.len()));
    let first_frame = ms(|| h.frame());
    phase(&mut line, "first_frame", first_frame);
    let mut idle: Vec<f64> = (0..5).map(|_| ms(|| h.frame())).collect();
    idle.sort_by(f64::total_cmp);
    phase(&mut line, "idle_frame", idle[2]);
    let cold = ms(|| drop(crate::viewport::build_scene(&h.app)));
    phase(&mut line, "scene_cold", cold);
    let unchanged = ms(|| drop(crate::viewport::build_scene(&h.app)));
    phase(&mut line, "scene_unchanged", unchanged);
    let Some(part) = bodies.first().cloned() else {
        println!("{line}");
        return;
    };
    let colour_command = ms(|| drop(h.app.run("appearance.assign", json!({"bodies": [&part], "color": "#336699"})).unwrap()));
    phase(&mut line, "colour_command", colour_command);
    let colour = ms(|| drop(crate::viewport::build_scene(&h.app)));
    phase(&mut line, "colour_scene", colour);
    let opacity_command = ms(|| drop(h.app.run("appearance.assign", json!({"bodies": [&part], "color": "#336699", "opacity": 0.5})).unwrap()));
    phase(&mut line, "opacity_command", opacity_command);
    let opacity = ms(|| drop(crate::viewport::build_scene(&h.app)));
    phase(&mut line, "opacity_scene", opacity);
    let theme_name = if crate::theme::is_dark() { "light" } else { "dark" };
    let _ = h.call("ui.prefs", json!({"theme": theme_name}));
    let theme = ms(|| drop(crate::viewport::build_scene(&h.app)));
    phase(&mut line, "theme_scene", theme);
    let select = ms(|| drop(h.app.run("select.set", json!({"items": [{"type": "body", "name": &part}]})).unwrap()));
    phase(&mut line, "select_command", select);
    let selected_frame = ms(|| h.frame());
    phase(&mut line, "selected_frame", selected_frame);
    let _ = h.app.run("select.clear", json!({}));
    const SIZE: u32 = 1024;
    if let Some(mut frames) = crate::gpu::OffscreenFrames::new(SIZE, true) {
        h.app.viewport.gpu = frames.target();
        h.app.fit_view();
        let model: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(Some(crate::viewport::build_scene(&h.app))));
        let highlight: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let gpu_cold = frames.frame(view_frame(&h.app, SIZE, (1, &model), (1, &highlight)));
        phase(&mut line, "gpu_cold_frame", gpu_cold);
        let mut still: Vec<f64> = (0..5).map(|_| frames.frame(view_frame(&h.app, SIZE, (1, &model), (1, &highlight)))).collect();
        still.sort_by(f64::total_cmp);
        phase(&mut line, "gpu_still_frame", still[2]);
        line.push_str(&format!(" gpu_triangles={}", frames.stats().triangles));
        let tenth = bodies.len().div_ceil(10);
        let mut key = 1;
        for (name, hovered) in [("hover_one", vec![part.clone()]), ("hover_tenth", bodies[..tenth].to_vec()), ("hover_all", bodies.clone())] {
            let mut times = Vec::new();
            for _ in 0..5 {
                for set in [Vec::new(), hovered.clone()] {
                    h.app.viewport.hover_bodies = set;
                    key += 1;
                    let built = std::time::Instant::now();
                    *highlight.lock().unwrap() = Some(crate::viewport::build_highlight(&h.app));
                    let build = built.elapsed().as_secs_f64() * 1000.0;
                    let frame = frames.frame(view_frame(&h.app, SIZE, (1, &model), (key, &highlight)));
                    if !h.app.viewport.hover_bodies.is_empty() {
                        times.push((build + frame, build));
                    }
                }
            }
            times.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (total, build) = times[times.len() / 2];
            phase(&mut line, name, total);
            phase(&mut line, &format!("{name}_build"), build);
        }
        h.app.viewport.hover_bodies.clear();
    }
    println!("{line}");
}

/// Zoomed onto one part of an assembly, as when editing it: the GPU frame and the triangles
/// drawn, against the whole assembly in view. Bodies out of the view are left out of their
/// blocks' draws. Skipped without a GPU adapter. Run with `--ignored --nocapture`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "manual culling measurement"]
fn zoomed_view_frame_cost() {
    const SIZE: u32 = 1024;
    let Some(mut frames) = crate::gpu::OffscreenFrames::new(SIZE, false) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut h = assembly(300, 500);
    h.app.viewport.gpu = frames.target();
    h.app.fit_view();
    let model: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(Some(crate::viewport::build_scene(&h.app))));
    let none: crate::gpu::SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let still = |h: &Harness, frames: &mut crate::gpu::OffscreenFrames| {
        frames.frame(view_frame(&h.app, SIZE, (7, &model), (0, &none)));
        let mut t: Vec<f64> = (0..7).map(|_| frames.frame(view_frame(&h.app, SIZE, (7, &model), (0, &none)))).collect();
        t.sort_by(f64::total_cmp);
        (t[3], frames.stats().triangles)
    };
    let (whole, whole_tris) = still(&h, &mut frames);
    let st = h.app.session.world_state();
    let b = st.bodies.iter().find(|b| b.component == 0).unwrap().mesh().bounds();
    drop(st);
    h.app.cam.target = b.center();
    h.app.cam.distance = b.diagonal() * 1.5;
    let (zoomed, zoomed_tris) = still(&h, &mut frames);
    // Orbiting: a new view every frame, so the bodies are culled again each time.
    let mut t: Vec<f64> = (0..21)
        .map(|i| {
            h.app.cam.yaw += 0.01 * f64::from(i);
            frames.frame(view_frame(&h.app, SIZE, (7, &model), (0, &none)))
        })
        .collect();
    t.sort_by(f64::total_cmp);
    let orbit = t[10];
    println!(
        "ZOOMED whole_ms={whole:.2} whole_triangles={whole_tris} zoomed_ms={zoomed:.2} zoomed_triangles={zoomed_tris} zoomed_orbit_ms={orbit:.2}"
    );
}

/// Every body of a design selected (`SOLIDCRAFT_SCALE_DESIGN`, or a generated grid of 1,440
/// boxes), the pointer moving from face to face: the time of each hover
/// change's highlight build, headless and as the app builds it on a GPU (bodies drawn from their
/// plans, and from the model's own blocks). Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual highlight measurement"]
fn many_selected_bodies_hover_cost() {
    use solidcraft_engine::geom::Vec3;
    let mut h = match std::env::var_os("SOLIDCRAFT_SCALE_DESIGN") {
        Some(path) => {
            let mut h = Harness::new();
            h.app.run("doc.open", json!({"path": path.to_string_lossy()})).unwrap();
            h.frames(3);
            h
        }
        None => grid(1440),
    };
    let st = h.app.session.world_state();
    st.warm_meshes(|| true);
    for b in &st.bodies {
        let _ = b.display_plan();
    }
    let bodies: Vec<(String, usize)> = st.bodies.iter().map(|b| (b.name.clone(), b.body.face_count())).collect();
    drop(st);
    let big = (0..bodies.len()).max_by_key(|&i| bodies[i].1).unwrap_or(0);
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let modes: [(&str, Option<bool>); 3] = [("headless", None), ("gpu_plans", Some(true)), ("gpu_blocks", Some(false))];
    for (mode, tess) in modes {
        h.app.viewport.gpu =
            tess.map(|tess| crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
        drop(crate::viewport::build_scene(&h.app));
        for (selected, biggest) in [(false, false), (true, false), (false, true), (true, true)] {
            h.app.session.selection =
                if selected { bodies.iter().map(|(name, _)| solidcraft_engine::Sel::Body { name: name.clone() }).collect() } else { Vec::new() };
            let hover = |h: &mut Harness, i: usize| {
                let (name, faces) = if biggest { &bodies[big] } else { &bodies[(i * 37) % bodies.len()] };
                h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: name.clone(), index: (i * 7) % (*faces).max(1), point: Vec3::ZERO });
            };
            hover(&mut h, 0);
            drop(crate::viewport::build_highlight(&h.app));
            let loops = std::env::var("SOLIDCRAFT_SCALE_LOOP").ok().and_then(|v| v.parse().ok()).unwrap_or(31);
            let changes: Vec<f64> = (1..=loops)
                .map(|i| {
                    hover(&mut h, i);
                    ms(|| drop(crate::viewport::build_highlight(&h.app)))
                })
                .collect();
            println!(
                "MANYSEL bodies={} mode={mode} all_selected={selected} hovering_biggest_body={biggest} hover_change_median_ms={:.3} hover_change_max_ms={:.3}",
                bodies.len(),
                median(changes.clone()),
                changes.iter().copied().fold(0.0, f64::max)
            );
        }
    }
}

/// An imported drawing of `k` × `k` closed blobs, four cubic Béziers each, as SVG text.
fn blob_logo(k: usize) -> String {
    let mut d = String::new();
    for i in 0..k {
        for j in 0..k {
            let (x, y) = (i as f64 * 3.0, j as f64 * 3.0);
            d.push_str(&format!(
                "<path d=\"M{x0},{y} C{x1},{y1} {x2},{y1} {x3},{y} C{x4},{y2} {x4},{y3} {x3},{y4} C{x2},{y5} {x1},{y5} {x0},{y4} C{x5},{y3} {x5},{y2} {x0},{y} Z\"/>",
                x0 = x,
                x1 = x + 0.6,
                x2 = x + 1.4,
                x3 = x + 2.0,
                x4 = x + 2.6,
                x5 = x - 0.6,
                y1 = y - 0.5,
                y2 = y + 0.6,
                y3 = y + 1.4,
                y4 = y + 2.0,
                y5 = y + 2.5,
            ));
        }
    }
    format!("<svg width=\"{w}mm\" height=\"{w}mm\" viewBox=\"-1 -1 {w} {w}\">{d}</svg>", w = k * 3 + 2)
}

/// Times the sketch scene while the camera orbits and zooms over a large open sketch (an
/// imported drawing): the scene alone on each new view, again on the same view (its sampled
/// curves cached), and a whole headless frame after it. Run with `--ignored --nocapture`.
#[test]
#[ignore = "manual sketch scene measurement"]
fn sketch_scene_cost_while_the_camera_moves() {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    for k in [10, 20, 32] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.insert_svg", json!({"text": blob_logo(k)})).unwrap();
        h.frames(3);
        let st = h.app.session.world_state();
        let sid = h.app.session.active_sketch.expect("the sketch is open");
        let (curves, profiles) = st.sketch(sid).map_or((0, 0), |ss| (ss.sketch.curves.len(), ss.profiles.len()));
        drop(st);
        h.app.fit_view();
        h.frames(3);
        let median = |mut t: Vec<f64>| {
            t.sort_by(f64::total_cmp);
            (t[t.len() / 2], t[0], t[t.len() - 1])
        };
        for (label, step) in [("orbit", 0usize), ("zoom", 1), ("orbit-no-fill", 0)] {
            if label == "orbit-no-fill" {
                h.app.session.visibility.hidden_profiles.push(sid);
            }
            let (mut fresh, mut again, mut frame, mut lines) = (Vec::new(), Vec::new(), Vec::new(), 0);
            for i in 0..30 {
                if step == 0 {
                    h.app.cam.yaw += 0.02;
                    h.app.cam.pitch = 0.9 + 0.01 * f64::from(i % 5);
                } else {
                    h.app.cam.distance *= if i < 15 { 0.97 } else { 1.0 / 0.97 };
                }
                let proj = crate::viewport::projection(&h.app, rect);
                let mut scene = None;
                fresh.push(ms(|| scene = Some(crate::viewport::build_sketch_scene(&h.app, &proj))));
                lines = scene.map_or(0, |s| s.merged().lines.len());
                again.push(ms(|| drop(crate::viewport::build_sketch_scene(&h.app, &proj))));
                frame.push(ms(|| h.frame()));
            }
            let (f, fmin, fmax) = median(fresh);
            let (a, amin, amax) = median(again);
            let (fr, frmin, frmax) = median(frame);
            println!(
                "SKETCHSCENE k={k} curves={curves} profiles={profiles} move={label} scene_new_view_ms={f:.2} ({fmin:.2}-{fmax:.2}) scene_same_view_ms={a:.2} ({amin:.2}-{amax:.2}) frame_ms={fr:.2} ({frmin:.2}-{frmax:.2}) line_bytes={lines}"
            );
        }
    }
}

/// Orbiting a large design with the right button, as a person turns the view: per frame, the
/// window thread's frame (the model scene built, as the app builds it once a renderer is there),
/// its named steps, and the renderer's prepare, draw recording and GPU work on an offscreen device
/// (skipped without an adapter, as with `WGPU_BACKEND=metal` on Linux, to time the window thread
/// alone). The design is `SOLIDCRAFT_SCALE_STEP` (inserted as the app opens a STEP file),
/// `SOLIDCRAFT_SCALE_DESIGN` (opened), or the generated assembly of 300 parts and 500 placements.
/// `SOLIDCRAFT_SCALE_LOOP` frames (default 60). `painted` fingerprints the window's picture once
/// the view rests, to compare two builds. Run with `--ignored --nocapture`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "manual measurement on a design file"]
fn orbit_frame_cost() {
    const SIZE: u32 = 1024;
    let mut h = if let Some(path) = std::env::var_os("SOLIDCRAFT_SCALE_STEP") {
        let mut h = Harness::new();
        h.app.run("file.insert_step", json!({"path": path.to_string_lossy()})).unwrap();
        h
    } else if let Some(path) = std::env::var_os("SOLIDCRAFT_SCALE_DESIGN") {
        let mut h = Harness::new();
        h.app.run("doc.open", json!({"path": path.to_string_lossy()})).unwrap();
        h
    } else {
        assembly(300, 500)
    };
    let st = h.app.session.world_state();
    st.warm_meshes(|| true);
    for b in &st.bodies {
        let _ = b.display_plan();
    }
    let bodies = st.bodies.len();
    let triangles: usize = st.bodies.iter().map(|b| b.mesh().triangles.len()).sum();
    drop(st);
    let mut frames = crate::gpu::OffscreenFrames::new(SIZE, true);
    h.app.viewport.gpu = match &frames {
        Some(f) => f.target(),
        None => Some(crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: true, oit: false }),
    };
    h.app.fit_view();
    h.frames(5);
    let rect = h.app.viewport.rect.unwrap();
    let at = rect.center();
    let draw = |h: &Harness, frames: &mut Option<crate::gpu::OffscreenFrames>| -> [f64; 3] {
        let Some(f) = frames.as_mut() else { return [0.0; 3] };
        let v = &h.app.viewport;
        f.frame_parts(view_frame(&h.app, SIZE, (v.key, &v.slot), (v.hl_key, &v.hl_slot)))
    };
    let cold = draw(&h, &mut frames);
    let none = egui::Modifiers::NONE;
    h.frame_with(vec![egui::Event::PointerMoved(at)]);
    h.frame_with(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: true, modifiers: none }]);
    let yaw = h.app.cam.yaw;
    let n = std::env::var("SOLIDCRAFT_SCALE_LOOP").ok().and_then(|v| v.parse().ok()).unwrap_or(60usize);
    let mut ui = Vec::new();
    let mut parts = Vec::new();
    let mut tess = Vec::new();
    let mut steps: std::collections::BTreeMap<&'static str, Vec<f64>> = Default::default();
    let mut scene_builds = 0;
    for i in 0..n {
        let key = h.app.viewport.key;
        let p = at + egui::vec2(4.0 * ((i % 80) as f32 - 40.0).abs(), 0.0) - egui::vec2(160.0, 0.0);
        ui.push(ms(|| h.frame_with(vec![egui::Event::PointerMoved(p)])));
        let shapes = std::mem::take(&mut h.shapes);
        tess.push(ms(|| drop(h.ctx.tessellate(shapes, 1.0))));
        scene_builds += usize::from(h.app.viewport.key != key);
        for (name, t) in h.app.frame_watch.steps() {
            steps.entry(name).or_default().push(*t);
        }
        parts.push(draw(&h, &mut frames));
    }
    h.frame_with(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: false, modifiers: none }]);
    assert_ne!(h.app.cam.yaw, yaw, "the right drag turned the view");
    // What the window painted once the view rests, tessellated: positions, colours and
    // triangles, the same between builds that only save work. Not the glyphs' places in the
    // font atlas, which follow the order the text was first drawn in.
    h.frames(3);
    let painted = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for clipped in h.ctx.tessellate(std::mem::take(&mut h.shapes), 1.0) {
            [clipped.clip_rect.min.x, clipped.clip_rect.min.y, clipped.clip_rect.max.x, clipped.clip_rect.max.y].map(f32::to_bits).hash(&mut hasher);
            if let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive {
                mesh.indices.hash(&mut hasher);
                for v in &mesh.vertices {
                    [v.pos.x.to_bits(), v.pos.y.to_bits()].hash(&mut hasher);
                    v.color.to_array().hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    };
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    };
    let mut line = format!(
        "ORBIT bodies={bodies} triangles={triangles} frames={n} scene_builds={scene_builds} ui_ms={:.2} egui_tessellate_ms={:.2} prepare_ms={:.2} paint_ms={:.2} gpu_ms={:.2} cold_prepare_ms={:.1} drawn_triangles={} painted={painted:016x}",
        median(ui),
        median(tess),
        median(parts.iter().map(|p| p[0]).collect()),
        median(parts.iter().map(|p| p[1]).collect()),
        median(parts.iter().map(|p| p[2]).collect()),
        cold[0],
        frames.as_ref().map_or(0, |f| f.stats().triangles),
    );
    for (name, t) in steps {
        line.push_str(&format!(" [{name}]={:.2}", median(t)));
    }
    println!("{line}");
}

/// A cylinder whose round wall is `pieces` faces of one surface (its circle drawn as `pieces`
/// arcs about one centre), as an imported surface split into many faces: hovering one face
/// lights the whole wall. Or such a design saved (`out/testdaten/split_cylinder.py`): the sketch
/// of many arcs takes long to build again. The wall's faces, ascending.
fn split_cylinder(pieces: usize, saved: Option<&str>) -> (Harness, Vec<usize>) {
    let mut h = Harness::new();
    if let Some(path) = saved {
        h.app.run("doc.open", json!({"path": path})).unwrap();
    } else {
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        let at = |i: usize| {
            let a = std::f64::consts::TAU * (i % pieces) as f64 / pieces as f64;
            [20.0 * a.cos(), 20.0 * a.sin()]
        };
        for i in 0..pieces {
            h.app.run("sketch.arc.center_point", json!({"center": [0, 0], "start": at(i), "end": at(i + 1)})).unwrap();
        }
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch": 1, "distance": 40})).unwrap();
    }
    h.app.set_origin_shown(false);
    h.frames(2);
    let st = h.app.session.world_state();
    let m = st.bodies[0].mesh();
    let wall = m.edge_faces.iter().zip(&m.seams).find(|(_, s)| **s).map(|(fs, _)| fs[0] as usize).unwrap();
    let patch = m.surface_patch(wall);
    drop(st);
    (h, patch)
}

fn median_ms(mut t: Vec<f64>) -> f64 {
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

/// Hovering faces of one smooth surface of many pieces (`split_cylinder`; 32 pieces built here, the
/// saved designs `SOLIDCRAFT_SPLIT_CYLINDERS=<a.solidcraft>:<b.solidcraft>`): the highlight built at
/// each hover change, from one piece of the wall to another and to a cap, at display qualities
/// Normal and Fine, with nothing and with the whole wall selected; and the per-frame highlight
/// key. With `SOLIDCRAFT_ROVER=<design>`, also the rover's per-frame key and a face hover there.
/// Run with `--ignored --nocapture` (not a pass/fail check).
#[test]
#[ignore = "manual hover measurement"]
fn smooth_surface_face_hover_cost() {
    use solidcraft_engine::doc::display_quality::{DisplayQuality, set_current};
    use solidcraft_engine::geom::Vec3;
    let saved = solidcraft_engine::geom::env::var("SPLIT_CYLINDERS").unwrap_or_default();
    let designs: Vec<Option<&str>> = if saved.is_empty() { vec![None] } else { saved.split(':').map(Some).collect() };
    for design in designs {
        let (mut h, wall) = split_cylinder(32, design);
        let pieces = wall.len();
        let body = h.app.session.world_state().bodies[0].clone();
        let cap = (0..body.body.face_count()).find(|f| !wall.contains(f)).unwrap();
        for quality in [DisplayQuality::Normal, DisplayQuality::Fine] {
            set_current(quality);
            let tris = h.app.session.world_state().bodies[0].plan_mesh().map_or(0, |m| m.triangles.len());
            for selected in [false, true] {
                h.app.session.selection = if selected {
                    wall.iter().map(|&index| solidcraft_engine::Sel::Face { body: body.name.clone(), index, point: Vec3::ZERO }).collect()
                } else {
                    Vec::new()
                };
                let targets = [wall[0], wall[wall.len() / 2], cap];
                let hover = |h: &mut Harness, i: usize| {
                    h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: body.name.clone(), index: targets[i % 3], point: Vec3::ZERO })
                };
                hover(&mut h, 0);
                let _ = crate::viewport::build_highlight(&h.app);
                let changes: Vec<f64> = (1..31)
                    .map(|i| {
                        hover(&mut h, i);
                        ms(|| drop(crate::viewport::build_highlight(&h.app)))
                    })
                    .collect();
                let key: Vec<f64> = (0..31)
                    .map(|_| {
                        ms(|| {
                            std::hint::black_box(crate::viewport::highlight_key(&h.app));
                        })
                    })
                    .collect();
                println!(
                    "HOVER pieces={pieces} quality={quality:?} plan_triangles={tris} wall_selected={selected} hover_change_median_ms={:.3} hover_change_max_ms={:.3} highlight_key_median_ms={:.4}",
                    median_ms(changes.clone()),
                    changes.iter().copied().fold(0.0, f64::max),
                    median_ms(key)
                );
            }
        }
        set_current(DisplayQuality::Normal);
    }
    let Some(path) = solidcraft_engine::geom::env::var("ROVER") else { return };
    let mut h = Harness::new();
    let t = std::time::Instant::now();
    h.app.run("doc.open", json!({"path": path})).unwrap();
    h.frames(3);
    let st = h.app.session.world_state();
    println!("ROVER bodies={} open_s={:.1}", st.bodies.len(), t.elapsed().as_secs_f64());
    let names: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
    // The body with the most faces, hovered face by face.
    let big = st.bodies.iter().max_by_key(|b| b.body.face_count()).unwrap().clone();
    drop(st);
    for selected in [false, true] {
        h.app.session.selection =
            if selected { names.iter().map(|name| solidcraft_engine::Sel::Body { name: name.clone() }).collect() } else { Vec::new() };
        let key: Vec<f64> = (0..31)
            .map(|_| {
                ms(|| {
                    std::hint::black_box(crate::viewport::highlight_key(&h.app));
                })
            })
            .collect();
        let faces = big.body.face_count();
        let changes: Vec<f64> = (0..31)
            .map(|i| {
                h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: big.name.clone(), index: (i * 7) % faces, point: Vec3::ZERO });
                ms(|| drop(crate::viewport::build_highlight(&h.app)))
            })
            .collect();
        println!(
            "ROVER all_selected={selected} biggest_body_faces={faces} highlight_key_median_ms={:.4} hover_change_median_ms={:.3}",
            median_ms(key),
            median_ms(changes)
        );
    }
}
