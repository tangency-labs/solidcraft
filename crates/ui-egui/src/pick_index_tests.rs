use egui::{Rect, pos2, vec2};
use serde_json::json;

use crate::SolveApp;
use crate::viewport::{Hit, pick, pick_all, projection};

/// Overlapping bodies with flat and curved faces, filleted edges and seams.
fn assembly() -> SolveApp {
    let mut s = solidcraft_engine::Session::default();
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
    s.execute("solid.fillet", &json!({"edges": [[40, 15, 20]], "radius": 4})).unwrap();
    s.execute("solid.cylinder", &json!({"radius": 8, "height": 35, "base": [10, 10, 0], "operation": "new_body"})).unwrap();
    s.execute("solid.sphere", &json!({"radius": 9, "center": [30, 25, 22]})).unwrap();
    let mut app = SolveApp::new(s, crate::Services::default());
    app.set_origin_shown(false);
    app
}

/// Every cursor position on a grid over the view, for each camera: the indexed pick answers what
/// scanning every triangle and edge answers (the nearest of each kind, and Select Other's list).
#[test]
fn indexed_picks_match_the_full_scan() {
    let mut app = assembly();
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0));
    let mut compared = (0, 0, 0);
    for (perspective, yaw, pitch) in [(false, 0.6, 0.5), (true, 0.6, 0.5), (true, 2.4, -0.3), (false, 0.0, 1.5)] {
        app.ui.perspective = perspective;
        app.cam.yaw = yaw;
        app.cam.pitch = pitch;
        app.cam.target = solidcraft_engine::geom::Vec3::new(20.0, 15.0, 12.0);
        app.cam.distance = 110.0;
        let proj = projection(&app, rect);
        for j in 0..48 {
            for i in 0..64 {
                let p = pos2(5.0 + i as f32 * 10.0 + (j % 3) as f32, 5.0 + j as f32 * 10.0);
                let fast = (pick(&app, &proj, p), pick_all(&app, &proj, p));
                super::SCAN.with(|s| s.set(true));
                let scan = (pick(&app, &proj, p), pick_all(&app, &proj, p));
                super::SCAN.with(|s| s.set(false));
                assert_eq!(fast, scan, "at {p:?} (perspective {perspective}, yaw {yaw}, pitch {pitch})");
                compared.0 += usize::from(fast.0.iter().any(|h| matches!(h, Hit::Face { .. })));
                compared.1 += usize::from(fast.0.iter().any(|h| matches!(h, Hit::Edge { .. })));
                compared.2 += usize::from(fast.0.iter().any(|h| matches!(h, Hit::Vertex { .. })));
            }
        }
    }
    // The grid reaches every kind many times over.
    assert!(compared.0 > 2000 && compared.1 > 100 && compared.2 > 10, "{compared:?}");
    assert_eq!(app.viewport.pick_index.len(), 3, "one index per body mesh");
}

/// A new model state lets go of the indexes of meshes it no longer has.
#[test]
fn indexes_follow_the_model_state() {
    let mut app = assembly();
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0));
    app.cam.target = solidcraft_engine::geom::Vec3::new(20.0, 15.0, 12.0);
    let proj = projection(&app, rect);
    let centre = proj.to_screen(solidcraft_engine::geom::Vec3::new(20.0, 15.0, 10.0)).unwrap();
    pick(&app, &proj, centre);
    assert!(app.viewport.pick_index.len() >= 1);
    app.session.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [-20, -20, 0], "operation": "new_body"})).unwrap();
    pick(&app, &proj, centre);
    assert!(app.viewport.pick_index.len() <= 4, "indexes of meshes gone with the old state are dropped");
    app.session.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [-40, -20, 0], "operation": "new_body"})).unwrap();
    pick(&app, &proj, centre);
    assert!(app.viewport.pick_index.len() <= 5);
}

/// Timing, not a check (`cargo test --release -p solidcraft-ui-egui --lib pick_timing -- --ignored
/// --nocapture`; `SOLIDCRAFT_BENCH_MESH=part.stl` adds 25 placements of that mesh): picks over a
/// grid of a 1400 × 900 view, scanning every triangle and edge versus through the indexes, on many
/// curved bodies, on one body with many curved faces, and on eight layers of bodies seen from
/// above (rays passing near many hidden edges). Release, Apple M2 Max: tens of microseconds a
/// pick through the indexes on each, 25 placements of a 2 M-triangle scan included.
#[test]
#[ignore]
fn pick_timing_on_curved_models() {
    let mut many = solidcraft_engine::Session::default();
    for i in 0..12 {
        for j in 0..12 {
            let (x, y) = (f64::from(i) * 30.0, f64::from(j) * 30.0);
            many.execute("solid.cylinder", &json!({"radius": 9, "height": 30, "base": [x, y, 0], "operation": "new_body"})).unwrap();
            many.execute("solid.sphere", &json!({"radius": 8, "center": [x, y, 40], "operation": "new_body"})).unwrap();
        }
    }
    let mut one = solidcraft_engine::Session::default();
    one.execute("solid.box", &json!({"length": 340, "width": 340, "height": 10, "corner": [-15, -15, -5]})).unwrap();
    for i in 0..8 {
        for j in 0..8 {
            let (x, y) = (f64::from(i) * 42.0, f64::from(j) * 42.0);
            one.execute("solid.cylinder", &json!({"radius": 14, "height": 30, "base": [x, y, 0], "operation": "join"})).unwrap();
        }
    }
    let mut deep = solidcraft_engine::Session::default();
    for k in 0..8 {
        for i in 0..8 {
            for j in 0..8 {
                let (x, y, z) = (f64::from(i) * 40.0 + f64::from(k) * 5.0, f64::from(j) * 40.0 + f64::from(k) * 3.0, f64::from(k) * 25.0);
                deep.execute("solid.cylinder", &json!({"radius": 15, "height": 20, "base": [x, y, z], "operation": "new_body"})).unwrap();
            }
        }
    }
    let mut scenes = vec![
        ("many bodies", SolveApp::new(many, crate::Services::default()), 0.6),
        ("one body", SolveApp::new(one, crate::Services::default()), 0.6),
    ];
    scenes.push(("eight layers", SolveApp::new(deep, crate::Services::default()), 1.2));
    if let Some(path) = std::env::var_os("SOLIDCRAFT_BENCH_MESH") {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let component = app.run("file.insert_mesh", json!({"path": path.to_string_lossy()})).unwrap()["component"].clone();
        app.run("component.activate", json!({"component":"root"})).unwrap();
        let size = app.session.world_state().bodies[0].mesh_bounds().size();
        let step = size.x.max(size.y) * 1.3;
        for k in 1..25 {
            app.run("occurrence.copy", json!({"component":component,"translate":[f64::from(k % 5) * step, f64::from(k / 5) * step, 0.0]})).unwrap();
        }
        scenes.push(("25 placed meshes", app, 0.6));
    }
    for (name, mut app, pitch) in scenes {
        app.set_origin_shown(false);
        (app.cam.yaw, app.cam.pitch) = (0.6, pitch);
        let st = app.session.world_state();
        app.cam.fit(&st.bodies.iter().fold(solidcraft_engine::geom::Aabb3::EMPTY, |a, b| a.union(&b.mesh_bounds())));
        let (tris, segs) = st
            .bodies
            .iter()
            .map(|b| b.mesh_in_place().0)
            .fold((0, 0), |(t, s), m| (t + m.triangles.len(), s + m.edges.iter().map(|e| e.len().saturating_sub(1)).sum::<usize>()));
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
        let proj = projection(&app, rect);
        let points: Vec<_> = (0..20).flat_map(|j| (0..30).map(move |i| pos2(30.0 + i as f32 * 45.0, 30.0 + j as f32 * 42.0))).collect();
        // Indexes made once, before timing (large ones on workers).
        for b in &st.bodies {
            let m = b.mesh_in_place().0;
            while app.viewport.pick_index.of(&m).is_none() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        for p in &points {
            pick(&app, &proj, *p);
        }
        let time = |scan: bool| {
            super::SCAN.with(|s| s.set(scan));
            let t = std::time::Instant::now();
            for p in &points {
                std::hint::black_box(pick(&app, &proj, *p));
            }
            super::SCAN.with(|s| s.set(false));
            t.elapsed().as_secs_f64() * 1000.0 / points.len() as f64
        };
        let (scan, indexed) = (if tris < 10_000_000 { time(true) } else { f64::NAN }, time(false));
        eprintln!("{name}: {} bodies, {tris} triangles, {segs} edge segments: scan {scan:.3} ms per pick, indexed {indexed:.3} ms", st.bodies.len());
    }
}

/// Timing, not a check: mouse moves over a large sketch being edited, with no tool, with Trim and
/// with Extend (`SOLIDCRAFT_BENCH_SKETCH=out/testdaten/hover-circles.json cargo test --release -p
/// solidcraft-ui-egui --lib sketch_hover_timing -- --ignored --nocapture`; the scripts and how
/// they are made are in `out/testdaten/README.md`). Each move is one whole UI frame, as the window
/// runs it; the profile lookup under the cursor and the Trim piece are also timed on their own.
#[test]
#[ignore]
fn sketch_hover_timing() {
    use std::time::Instant;
    let Some(path) = std::env::var_os("SOLIDCRAFT_BENCH_SKETCH") else { return };
    let script: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut h = crate::scenario::Harness::new();
    let began = Instant::now();
    for c in script["commands"].as_array().unwrap() {
        h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
    }
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let ss = h.app.session.world_state().sketch(sid).unwrap().clone();
    eprintln!(
        "{}: {} curves, {} profiles, built in {:.1} s",
        path.to_string_lossy(),
        ss.sketch.curves.len(),
        ss.profiles.len(),
        began.elapsed().as_secs_f64()
    );
    // Points on the lines (where Trim and Extend show something), then a grid over the sketch.
    let mut at: Vec<(String, solidcraft_engine::geom::Vec2)> = Vec::new();
    for c in &ss.sketch.curves {
        if let solidcraft_engine::sketch::CurveKind::Line { a, b } = c.kind
            && at.len() < 60
        {
            let (p, q) = (ss.sketch.point(a).unwrap(), ss.sketch.point(b).unwrap());
            for t in [0.31, 0.62, 0.93] {
                at.push((c.id.clone(), p.lerp(q, t)));
            }
        }
    }
    let bounds: solidcraft_engine::geom::Aabb2 = ss.sketch.points.iter().map(|p| p.pos).collect();
    for j in 0..6 {
        for i in 0..10 {
            let size = bounds.max - bounds.min;
            let r = solidcraft_engine::geom::Vec2::new(size.x * (f64::from(i) + 0.5) / 10.0, size.y * (f64::from(j) + 0.5) / 6.0);
            at.push((String::new(), bounds.min + r));
        }
    }
    let screen: Vec<egui::Pos2> = at
        .iter()
        .map(|(_, q)| {
            let [x, y] = h.at(&json!({"sketch": [q.x, q.y]})).unwrap();
            egui::pos2(x as f32, y as f32)
        })
        .collect();
    let stats = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        (v[v.len() / 2], v[v.len() * 9 / 10])
    };
    for tool in [None, Some("sketch.trim"), Some("sketch.extend")] {
        if let Some(t) = tool {
            h.step(&json!({"start": t})).unwrap();
            h.frames(2);
        }
        let mut ms = Vec::new();
        for round in 0..3 {
            for p in &screen {
                let t = Instant::now();
                h.frame_with(vec![egui::Event::PointerMoved(*p)]);
                if round > 0 {
                    ms.push(t.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        let (median, p90) = stats(ms);
        eprintln!("frame per mouse move, {}: median {median:.3} ms, p90 {p90:.3} ms", tool.unwrap_or("no tool"));
        if tool.is_some() {
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
        }
    }
    let t = Instant::now();
    let mut found = 0;
    for _ in 0..10 {
        for (_, q) in &at {
            found += usize::from(std::hint::black_box(solidcraft_engine::sketch::profile_at_point(&ss.profiles, *q)).is_some());
        }
    }
    eprintln!("profile under the cursor: {:.4} ms a point ({found} found)", t.elapsed().as_secs_f64() * 100.0 / at.len() as f64);
    h.step(&json!({"start": "sketch.trim"})).unwrap();
    let mut ms = Vec::new();
    for (id, q) in at.iter().filter(|(id, _)| !id.is_empty()) {
        if let Some(tool) = h.app.tool.as_mut() {
            tool.hover = Some((*q, None));
        }
        let t = Instant::now();
        std::hint::black_box(crate::sketch_tools::trim_hover_piece(&h.app, sid, id));
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let (median, p90) = stats(ms);
    eprintln!("Trim piece per cursor move: median {median:.4} ms, p90 {p90:.4} ms");
}
