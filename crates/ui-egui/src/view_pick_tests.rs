//! `view.pick` / `ui.pick` (#6304): pixels and rays answer with the geometry under them,
//! read-only, from both frames, section-aware.

use serde_json::json;

use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::scenario::Harness;

/// A 10×10×10 box with an orthographic camera straight over its top face: the centre pixel
/// of a 960×600 model render looks down the +Z face at (5, 5, 10), 95 mm from the eye.
fn top_view() -> Harness {
    let mut h = Harness::new();
    h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}));
    h.app.cam = Camera {
        target: Vec3::new(5.0, 5.0, 5.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        distance: 100.0,
        fov: 0.0,
        up: Default::default(),
    };
    h
}

fn pick(h: &mut Harness, mut params: serde_json::Value) -> serde_json::Value {
    params.as_object_mut().unwrap().entry("frame").or_insert(json!("model"));
    let v = h.call("ui.pick", params);
    assert_eq!(v["ok"], true, "{v}");
    v["result"].clone()
}

/// The centre pixel of the top face answers with the exact face, point, normal and depth;
/// a pixel off the box answers with nothing.
#[test]
fn the_centre_pixel_of_a_box_hits_its_facing_face() {
    let mut h = top_view();
    let r = pick(&mut h, json!({"pixels": [[480, 300], [40, 40]]}));
    assert_eq!(r["frame"], "model");
    assert_eq!(r["camera"]["projection"], "orthographic");
    let hit = &r["hits"][0];
    assert!(hit["body"].is_string(), "{hit}");
    assert!(hit["face"].as_u64().is_some_and(|f| f < 6), "{hit}");
    let [x, y, z] = hit["point"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>().try_into().unwrap();
    assert!((x - 5.0).abs() < 1e-9 && (y - 5.0).abs() < 1e-9 && (z - 10.0).abs() < 1e-9, "{hit}");
    let n = hit["normal"].as_array().unwrap();
    assert!((n[2].as_f64().unwrap() - 1.0).abs() < 1e-9, "{hit}");
    assert!((hit["depth"].as_f64().unwrap() - 395.0).abs() < 1e-9, "orthographic depth runs from the ray origin, 4 distances ahead: {hit}");
    assert_eq!(r["hits"][1], serde_json::Value::Null);
}

/// Perspective and orthographic agree on the centre pixel's face and point.
#[test]
fn perspective_and_orthographic_agree_on_the_centre_pixel() {
    let mut h = top_view();
    let flat = pick(&mut h, json!({"pixels": [[480, 300]]}));
    h.app.cam.fov = 0.6;
    let round = pick(&mut h, json!({"pixels": [[480, 300]]}));
    assert_eq!(round["camera"]["projection"], "perspective");
    assert_eq!(round["hits"][0]["face"], flat["hits"][0]["face"]);
    let (a, b) = (round["hits"][0]["point"].as_array().unwrap(), flat["hits"][0]["point"].as_array().unwrap());
    for i in 0..3 {
        assert!((a[i].as_f64().unwrap() - b[i].as_f64().unwrap()).abs() < 1e-6, "{round} vs {flat}");
    }
}

/// A world ray answers without any pixel, and a zero direction is refused.
#[test]
fn world_rays_pick_without_a_pixel() {
    let mut h = top_view();
    let r = pick(&mut h, json!({"rays": [{"origin": [5.0, 5.0, 50.0], "dir": [0.0, 0.0, -1.0]}]}));
    assert!((r["hits"][0]["depth"].as_f64().unwrap() - 40.0).abs() < 1e-9, "{}", r["hits"][0]);
    assert!((r["hits"][0]["point"][2].as_f64().unwrap() - 10.0).abs() < 1e-9);
    let v = h.call("ui.pick", json!({"rays": [{"origin": [0.0, 0.0, 0.0], "dir": [0.0, 0.0, 0.0]}]}));
    assert_eq!(v["ok"], false);
}

/// A hidden body answers with nothing, as the viewport does not show it.
#[test]
fn a_hidden_body_is_not_picked() {
    let mut h = top_view();
    let name = h.app.session.world_state().bodies[0].name.clone();
    h.app.session.visibility.hidden_bodies.push(name);
    let r = pick(&mut h, json!({"pixels": [[480, 300]]}));
    assert_eq!(r["hits"][0], serde_json::Value::Null);
}

/// A section hides the material its normal points at: over the removed half the pick
/// lands on the cut cap (or the far side), never on the removed face.
#[test]
fn a_section_pick_lands_on_the_cap_not_the_removed_face() {
    let mut h = top_view();
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 0, 5], "normal": [0, 0, 1]}}}));
    let r = pick(&mut h, json!({"pixels": [[480, 300]]}));
    let hit = &r["hits"][0];
    assert_eq!(hit["section_cap"], true, "{hit}");
    assert!(hit["face"].is_null(), "{hit}");
    assert!((hit["point"][2].as_f64().unwrap() - 5.0).abs() < 1e-9, "{hit}");
    assert!((hit["normal"][2].as_f64().unwrap() - 1.0).abs() < 1e-9, "{hit}");
    // Off the sectioned column (the plane cuts the whole box here, so a side pixel that
    // missed the box entirely still answers with nothing).
    assert_eq!(r["hits"].as_array().map(Vec::len), Some(1));
}

/// A section of two planes hides only what both hide: a pick over the cut-away quarter lands
/// on the cap and names the plane it is the cut face of; beside it, the kept top face answers.
#[test]
fn a_pick_names_the_section_plane_of_the_cap() {
    let mut h = top_view();
    for plane in [json!({"origin": [5, 0, 0], "normal": [1, 0, 0]}), json!({"origin": [0, 0, 5], "normal": [0, 0, 1]})] {
        h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": plane, "add": true}}));
    }
    let ray = |x: f64| json!({"origin": [x, 5, 100], "dir": [0, 0, -1]});
    let r = pick(&mut h, json!({"rays": [ray(7.5), ray(2.5)]}));
    let (cut, kept) = (&r["hits"][0], &r["hits"][1]);
    assert_eq!((cut["section_cap"].as_bool(), cut["section_plane"].as_u64()), (Some(true), Some(2)), "{cut}");
    assert!((cut["point"][2].as_f64().unwrap() - 5.0).abs() < 1e-9, "{cut}");
    assert!(kept["face"].is_u64() && (kept["point"][2].as_f64().unwrap() - 10.0).abs() < 1e-9, "{kept}");
}

/// A pixel near a silhouette edge reports that edge with its distance; a pixel in the
/// middle of the face reports none.
#[test]
fn edge_proximity_follows_the_pixel() {
    let mut h = top_view();
    // The front edge of the top face lies at y = 0: a pixel two points off it (0.2 mm at
    // this zoom is well inside 6 px) reports the edge.
    let near = pick(&mut h, json!({"pixels": [[480, 335.5]]}));
    assert!(near["hits"][0]["edge"].get("index").is_some(), "{}", near["hits"][0]);
    let middle = pick(&mut h, json!({"pixels": [[480, 300]]}));
    assert!(middle["hits"][0].get("edge").is_none(), "{}", middle["hits"][0]);
}

/// `ui.at` (world → screen) and `ui.pick` (screen → world) round-trip through the window.
#[test]
fn window_picks_round_trip_through_ui_at() {
    let mut h = Harness::new();
    h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}));
    h.frames(60); // the first-body fit animates; the camera must hold still between the two calls
    let at = h.call("ui.at", json!({"world": [5.0, 5.0, 10.0]}));
    assert_eq!(at["ok"], true, "{at}");
    let xy = at["result"].as_array().unwrap();
    let r = pick(&mut h, json!({"frame": "window", "pixels": [[xy[0], xy[1]]]}));
    assert_eq!(r["frame"], "window");
    let hit = &r["hits"][0];
    let n = hit["normal"].as_array().unwrap();
    assert!((n[2].as_f64().unwrap() - 1.0).abs() < 1e-6, "the pick must land on the top face: {hit}");
    let p = hit["point"].as_array().unwrap();
    for (got, want) in p.iter().zip([5.0, 5.0, 10.0]) {
        assert!((got.as_f64().unwrap() - want).abs() < 1e-6, "{hit}");
    }
}

/// The same round-trip at a 2× HiDPI scale: window pixels are physical, so they double.
/// Driven through `control::handle` directly: a harness frame would reset the scale factor.
#[test]
fn window_picks_divide_physical_pixels_at_hidpi() {
    let mut h = Harness::new();
    h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}));
    h.app.viewport.set_pixels_per_point(2.0);
    let at = raw_call(&mut h, "ui.at", json!({"world": [5.0, 5.0, 10.0]}));
    let xy = at["result"].as_array().unwrap();
    let doubled: Vec<f64> = xy.iter().map(|v| v.as_f64().unwrap() * 2.0).collect();
    let v = raw_call(&mut h, "ui.pick", json!({"frame": "window", "pixels": [doubled]}));
    assert_eq!(v["ok"], true, "{v}");
    let hit = &v["result"]["hits"][0];
    let n = hit["normal"].as_array().unwrap();
    assert!((n[2].as_f64().unwrap() - 1.0).abs() < 1e-6, "the HiDPI pick must land on the same face: {hit}");
}

/// One control request without the harness settling frames (which would reset the scale).
fn raw_call(h: &mut Harness, method: &str, params: serde_json::Value) -> serde_json::Value {
    let (req, _rx) = crate::control::ControlRequest::new(method, params);
    match crate::control::handle(&mut h.app, &h.ctx, &req) {
        crate::control::Outcome::Done(v) => v,
        crate::control::Outcome::Screenshot { .. } => json!({"ok": true}),
    }
}

/// The command is read-only: selection, camera, revision and undo stand exactly where they were.
#[test]
fn picking_changes_nothing() {
    let mut h = top_view();
    let (cam, undo, revision) = (h.app.cam, h.app.session.undo.len(), h.app.session.revision);
    let r = pick(&mut h, json!({"pixels": [[480, 300], [40, 40]], "rays": [{"origin": [5.0, 5.0, 50.0], "dir": [0.0, 0.0, -1.0]}]}));
    assert_eq!(r["hits"].as_array().map(Vec::len), Some(3));
    assert_eq!((h.app.cam.distance, h.app.cam.target.z), (cam.distance, cam.target.z));
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.revision, revision);
}

/// MCP reaches the command through `engine.execute` and `engine.batch` like any command.
#[test]
fn the_command_runs_through_execute_and_batch() {
    let mut h = top_view();
    let params = json!({"frame": "model", "pixels": [[480, 300]]});
    let v = h.call("engine.execute", json!({"command": "view.pick", "params": params}));
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["result"]["hits"][0]["face"].is_number(), "{v}");
    let v = h.call("engine.batch", json!({"commands": [{"command": "view.pick", "params": params}]}));
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["result"]["results"][0]["hits"][0]["face"].is_number(), "{v}");
}

/// `all` lists every body the ray meets, nearest first; the input cap is enforced.
#[test]
fn all_lists_the_faces_along_the_ray_and_the_cap_is_enforced() {
    let mut h = top_view();
    let r = pick(&mut h, json!({"pixels": [[480, 300]], "all": true}));
    let row = r["hits"][0].as_array().unwrap();
    assert_eq!(row.len(), 1, "one body under the ray: {r}");
    assert!(row[0]["face"].is_number());
    let pixels: Vec<Vec<f64>> = (0..1025).map(|i| vec![i as f64 % 960.0, 300.0]).collect();
    let v = h.call("ui.pick", json!({"pixels": pixels}));
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().is_some_and(|e| e.contains("1024")), "{v}");
}

/// The pointer over a section's cap picks nothing behind it (#6328): hovering the cut used to
/// pick the removed top face, whose highlighted outline then showed dimmed through the cap
/// wherever it ran behind it, so the cut looked see-through. Through the pocket the cut opens,
/// the pointer still finds the pocket's floor.
#[test]
fn the_mouse_picker_stops_at_a_section_cap() {
    use crate::viewport::{Hit, pick, pick_all, projection};
    let mut h = top_view();
    h.call(
        "engine.execute",
        json!({"command": "solid.box", "params": {"length": 4, "width": 4, "height": 4, "corner": [3, 3, 6], "operation": "cut"}}),
    );
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 0, 8], "normal": [0, 0, 1]}}}));
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 600.0));
    let proj = projection(&h.app, rect);
    let faces = |hits: &[Hit]| hits.iter().filter(|x| matches!(x, Hit::Face { .. })).cloned().collect::<Vec<_>>();
    // Over the cut material, between the pocket and the box's side: no face, and no edge or
    // corner behind the cap.
    let solid = proj.to_screen(Vec3::new(1.5, 5.0, 8.0)).unwrap();
    let hits = pick(&h.app, &proj, solid);
    assert!(hits.iter().all(|x| !matches!(x, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })), "{hits:?}");
    assert!(faces(&pick_all(&h.app, &proj, solid)).is_empty());
    // Over the pocket: its floor at z = 6, below the cut.
    let pocket = proj.to_screen(Vec3::new(5.0, 5.0, 8.0)).unwrap();
    match faces(&pick(&h.app, &proj, pocket)).first() {
        Some(Hit::Face { point, .. }) => assert!((point.z - 6.0).abs() < 1e-6, "{point:?}"),
        other => panic!("the pocket's floor: {other:?}"),
    }
    // Without the section, the box's top face answers there.
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"clear": true}}));
    match faces(&pick(&h.app, &proj, solid)).first() {
        Some(Hit::Face { point, .. }) => assert!((point.z - 10.0).abs() < 1e-6, "{point:?}"),
        other => panic!("the top face: {other:?}"),
    }
}
