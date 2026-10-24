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
    h.app.viewport.gpu = Some(crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: true });
    let planned = ms(|| drop(crate::viewport::build_highlight(&h.app)));
    println!("HIGHLIGHT bodies={bodies} triangles={triangles} cpu_copy_ms={copied:.2} model_triangles_ms={planned:.2}");
}
