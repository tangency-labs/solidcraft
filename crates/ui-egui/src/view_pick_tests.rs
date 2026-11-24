//! `view.pick` / `ui.pick` (#6304): pixels and rays answer with the geometry under them,
//! read-only, from both frames, section-aware.

use serde_json::json;

use solidcore_record::Vec3;
use solidcraft_viewport::Camera;

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

/// The nearer triangle falls outside the section. The farther triangle still answers with its
/// depth along the world ray, including the distance to the near cutoff.
#[test]
fn clipped_mesh_pick_measures_from_the_original_ray_origin() {
    let mut h = Harness::new();
    let path = h.dir.join("clipped.obj");
    std::fs::write(&path, "v 0 0 2\nv 10 0 2\nv 0 10 2\nv 0 0 8\nv 10 0 8\nv 0 10 8\nf 1 2 3\nf 4 5 6\n").unwrap();
    let imported = h.app.run("file.insert_mesh", json!({"path":path})).unwrap();
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    let copied = h.app.run("occurrence.copy", json!({"component":imported["component"],"translate":[40,-7,4]})).unwrap();
    let occurrence = copied["occurrence"].as_u64().unwrap();
    let names = h.app.session.occurrence_bodies(&[occurrence], false).unwrap();
    assert_eq!(names.len(), 1);
    let state = crate::open_meshing::session_world(&h.app.session);
    let body = state.body(&names[0]).unwrap();
    assert!(body.body.is_mesh());
    let (mesh, place) = body.mesh_in_place();
    assert!(!solidcraft_engine::doc::is_identity(&place), "the ray must be transformed to the definition's mesh");
    assert!(h.app.viewport.pick_index.of(&mesh).is_some(), "the small mesh gets an index immediately");

    // The copy's triangles are at world z = 12 and z = 6. The ray crosses their interiors.
    let origin = Vec3::new(43.0, -3.0, 30.0);
    let dir = Vec3::new(0.0, 0.0, -1.0);
    let cases = [
        ((0.0, 30.0), Some(18.0)),
        // A hit exactly on the near cutoff remains visible even if the far triangle is clipped.
        ((18.0, 30.0), Some(18.0)),
        ((18.0, 23.0), Some(18.0)),
        ((21.0, 30.0), Some(24.0)),
        ((21.0, 23.0), None),
    ];
    for scan in [false, true] {
        let depths = crate::pick_index::SCAN.with(|setting| {
            let previous = setting.replace(scan);
            let depths = cases.map(|(clip, _)| super::mesh_hit_in_clip(&h.app, body, origin, dir, clip));
            setting.set(previous);
            depths
        });
        for ((clip, expected), depth) in cases.into_iter().zip(depths) {
            assert_eq!(depth, expected, "scan {scan}, clip {clip:?}");
        }
    }
}

/// A hidden body answers with nothing, as the viewport does not show it.
#[test]
fn a_hidden_body_is_not_picked() {
    let mut h = top_view();
    let name = crate::open_meshing::session_world(&h.app.session).bodies[0].name.clone();
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

/// A sheet has no inside, so one crossed in the removed half is not a cut: the pick looks
/// through the section to the sheet behind it instead of stopping on a cap (#17177).
#[test]
fn a_section_pick_reaches_a_sheet_behind_a_sheet_in_the_removed_half() {
    let mut h = Harness::new();
    for (name, y) in [("W1", 5), ("W2", 8)] {
        h.app.run("sketch.create", json!({"plane":"XY","name":name})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0, y], [10, y]],"ids":[format!("{name}c")]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.call(
            "engine.execute",
            json!({"command": "surface.extrude", "params": {"sketch": name, "curves": [format!("{name}c")], "distance": 10, "body_name": name}}),
        );
    }
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 0, 5], "normal": [0, 0, 1]}}}));
    // Through W1 at z = 7 (removed), the cut plane at y = 7, then W2 at z = 4 (kept).
    let r = pick(&mut h, json!({"rays": [{"origin": [5, 0, 12], "dir": [0, 1, -1]}]}));
    let hit = &r["hits"][0];
    assert!(hit.get("section_cap").is_none(), "{hit}");
    assert_eq!(hit["body"], "W2", "{hit}");
    assert!((hit["point"][2].as_f64().unwrap() - 4.0).abs() < 1e-6, "{hit}");
}

/// Each plane of a section hides its own side (x > 5 and z > 5 here): a pick down through
/// the kept quarter lands on the second plane's cap and names it, one in from the side lands on
/// the first's, and one down past the first plane finds nothing, the material beyond it hidden
/// though the second keeps it.
#[test]
fn a_pick_names_the_section_plane_of_the_cap() {
    let mut h = top_view();
    for plane in [json!({"origin": [5, 0, 0], "normal": [1, 0, 0]}), json!({"origin": [0, 0, 5], "normal": [0, 0, 1]})] {
        h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": plane, "add": true}}));
    }
    let down = |x: f64| json!({"origin": [x, 5, 100], "dir": [0, 0, -1]});
    let side = json!({"origin": [100, 5, 2.5], "dir": [-1, 0, 0]});
    let r = pick(&mut h, json!({"rays": [down(2.5), side, down(7.5)]}));
    let (top, from_side, beyond) = (&r["hits"][0], &r["hits"][1], &r["hits"][2]);
    assert_eq!((top["section_cap"].as_bool(), top["section_plane"].as_u64()), (Some(true), Some(2)), "{top}");
    assert!((top["point"][2].as_f64().unwrap() - 5.0).abs() < 1e-9, "{top}");
    assert_eq!((from_side["section_cap"].as_bool(), from_side["section_plane"].as_u64()), (Some(true), Some(1)), "{from_side}");
    assert!((from_side["point"][0].as_f64().unwrap() - 5.0).abs() < 1e-9, "{from_side}");
    assert!(beyond.is_null(), "material beyond the first plane is picked: {beyond}");
}

/// A frame member Ghost Frame Members draws see-through raises no cap where a section cuts it: a
/// pick goes through it, as it goes through its faces.
#[test]
fn a_section_through_a_ghosted_frame_member_raises_no_cap() {
    let mut h = Harness::new();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Rail"})),
        ("sketch.line", json!({"points": [[0, 0], [100, 0]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Rail", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 0, 5], "normal": [0, 0, 1]}}}));
    // Down through the tube's wall (y 17…20), cut at z = 5.
    let ray = json!({"rays": [{"origin": [50, 18.5, 50], "dir": [0, 0, -1]}]});
    assert_eq!(pick(&mut h, ray.clone())["hits"][0]["section_cap"], true);
    h.app.start("frame.member");
    if let Some(crate::dialogs::Kind::Frame(m)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        m.ghost = true;
    }
    let r = pick(&mut h, ray);
    assert!(r["hits"][0].get("section_cap").is_none(), "{r}");
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

/// A closed profile lying on the section's own plane (#20638) stays pickable for a feature, and is
/// drawn where it is: the section's cut face does not take the pick, and the profile's fill is
/// not lifted off the plane, where the section would clip it or the cut face hide it.
#[test]
fn a_profile_on_the_section_plane_stays_pickable() {
    use crate::viewport::{Hit, lies_on_section, pick, projection};
    let mut h = top_view();
    h.app.run("sketch.create", json!({"plane": "XY", "offset": 5, "name": "Middle"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [8, 8]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 0, 5], "normal": [0, 0, 1]}}}));
    h.app.start("solid.extrude");
    h.settle();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 600.0));
    let proj = projection(&h.app, rect);
    let inside = proj.to_screen(Vec3::new(5.0, 5.0, 5.0)).unwrap();
    assert!(matches!(pick(&h.app, &proj, inside).first(), Some(Hit::Profile { .. })), "{:?}", pick(&h.app, &proj, inside));
    assert!(lies_on_section(&h.app, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 1.0)));
    assert!(lies_on_section(&h.app, Vec3::new(3.0, 4.0, 5.0), Vec3::new(0.0, 0.0, -1.0)));
    assert!(!lies_on_section(&h.app, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 1.0)));
    assert!(!lies_on_section(&h.app, Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 0.0)));
}

/// With a Section Analysis on, `view.pick` must not answer with edges or vertices in the
/// half the section removes: the mouse picker has skipped them since #6722, and an agent
/// must not get an answer the user cannot see (#10704).
#[test]
fn a_section_pick_skips_edges_and_vertices_in_the_removed_half() {
    let mut h = Harness::new();
    h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}}));
    h.app.cam = Camera {
        target: Vec3::new(20.0, 15.0, 10.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        distance: 200.0,
        fov: 0.0,
        up: Default::default(),
    };
    // Section XZ at 15 mm: keeps y 0..15, removes y 15..30.
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 15, 0], "normal": [0, 1, 0]}}}));
    // The same projection the model-frame pick builds, so a world point's pixel can be picked.
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(960.0, 600.0));
    let cam = h.app.cam;
    let proj = crate::viewport::Proj { cam, vp: cam.view_proj(960.0 / 600.0, crate::viewport::scene_radius(&h.app)), rect, bounds: None };
    let px = |p: [f64; 3]| {
        let sp = proj.to_screen(Vec3::new(p[0], p[1], p[2])).unwrap();
        json!([sp.x as f64, sp.y as f64])
    };
    // Top-view pixels on the removed half's top edge, corner, and mid-edge: nothing is drawn
    // there, so the pick answers with nothing (it used to return the edge, with `body: null`).
    for w in [[40.0, 25.0, 20.0], [40.0, 30.0, 20.0], [20.0, 30.0, 20.0]] {
        let r = pick(&mut h, json!({"pixels": [px(w)]}));
        assert_eq!(r["hits"][0], serde_json::Value::Null, "the removed half must pick nothing at {w:?}: {}", r["hits"][0]);
    }
    // The same pick on the kept half's edge (y < 15) still answers with that edge.
    let r = pick(&mut h, json!({"pixels": [px([40.0, 5.0, 20.0])]}));
    assert!(r["hits"][0]["edge"].get("index").is_some(), "the kept half's edge must still pick: {}", r["hits"][0]);
}

/// `view.pick` (pixels and rays) and interactive viewport picking find visible point-cloud points,
/// return exact world positions, respect filters, sections and solid occlusion, and feed
/// construction-point and Measure tools (#8933).
#[test]
fn point_cloud_points_pick_in_view_pick_viewport_and_construct_tools() {
    use solidcraft_engine::doc::point_cloud::{ColorBy, Colors, Column, Complex, Filter, Floats, Keep, On, PointCloud, Points};
    use solidcraft_engine::geom::prelude::*;
    let mut h = Harness::new();
    // Three points on the z = 12 plane plus one point (index 0) on a back wall at z = 2 behind index 1.
    // Cloud origin is (10, 0, 0), so local (0, 0, 12) -> world (10, 0, 12).
    let cloud = PointCloud {
        id: 1,
        name: "Scan".into(),
        file: "scan.ply".into(),
        origin: Vec3::new(10.0, 0.0, 0.0),
        points: Points::new(vec![0.0, 0.0, 2.0, 0.0, 0.0, 12.0, 10.0, 0.0, 12.0, 0.0, 10.0, 12.0].into()),
        colors: Colors::default(),
        source_points: 4,
        units: "mm".into(),
        transform: solidcraft_engine::doc::IDENTITY,
        point_size: 4.0,
        color_by: ColorBy::File,
        color: None,
        height_range: None,
        visible: true,
        complex: Complex {
            columns: vec![Column { name: "score".into(), on: On::Point, integer: false, values: Floats(vec![1.0, 0.8, 0.9, 0.9].into()) }],
            ..Default::default()
        },
        color_property: None,
        filters: Vec::new(),
        source: String::new(),
        max_points: None,
    };
    h.app.session.doc_mut().point_clouds.push(cloud);
    h.app.cam = Camera {
        target: Vec3::new(15.0, 5.0, 6.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        distance: 100.0,
        fov: 0.0,
        up: Default::default(),
    };
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(960.0, 600.0));
    let cam = h.app.cam;
    let proj = crate::viewport::Proj { cam, vp: cam.view_proj(960.0 / 600.0, crate::viewport::scene_radius(&h.app)), rect, bounds: None };
    let px = |p: Vec3| {
        let sp = proj.to_screen(p).unwrap();
        (sp, json!([sp.x as f64, sp.y as f64]))
    };

    // 1. Pixel pick over (10, 0, 12) chooses front point (index 1) over back wall (index 0 at z = 2).
    let (sp1, px1) = px(Vec3::new(10.0, 0.0, 12.0));
    let r = pick(&mut h, json!({"pixels": [px1], "clouds": true}));
    let hit = &r["hits"][0];
    assert_eq!(hit["cloud"], 1, "{hit}");
    assert_eq!(hit["cloud_name"], "Scan", "{hit}");
    assert_eq!(hit["point_index"], 1, "{hit}");
    assert!(hit["body"].is_null() && hit["face"].is_null(), "{hit}");
    assert!((hit["point"][0].as_f64().unwrap() - 10.0).abs() < 1e-9, "{hit}");
    assert!((hit["point"][2].as_f64().unwrap() - 12.0).abs() < 1e-9, "{hit}");

    // 1b. A call that does not ask for clouds keeps its earlier answer (nothing under the pixel);
    // `point_px` asks for them too.
    let r = pick(&mut h, json!({"pixels": [px1]}));
    assert_eq!(r["hits"][0], serde_json::Value::Null, "{r}");
    let r = pick(&mut h, json!({"pixels": [px1], "point_px": 8}));
    assert_eq!(r["hits"][0]["point_index"], 1, "{r}");

    // 2. Viewport interactive pick finds the same cloud point and accepts it in construct.point.vertex.
    let hits = crate::viewport::pick(&h.app, &proj, sp1);
    assert!(
        hits.iter().any(
            |x| matches!(x, crate::viewport::Hit::PointCloudPoint { cloud: 1, index: 1, point, .. } if point.dist(Vec3::new(10.0, 0.0, 12.0)) < 1e-9)
        ),
        "{hits:?}"
    );
    h.app.start("construct.point.vertex");
    let picked = crate::viewport::candidate(&h.app, &hits).expect("construct.point.vertex accepts cloud points");
    assert!(matches!(picked.1, solidcraft_engine::Sel::PointCloudPoint { cloud: 1, index: 1, .. }), "{picked:?}");
    let cref = crate::dialogs::construct_ref(&h.app, &picked.1).expect("construct_ref produces {point}");
    assert_eq!(cref, json!({"point": [10.0, 0.0, 12.0]}));
    h.app.dialog = None;

    // 2b. End-to-end `construct.plane.three_points` from three picked cloud points on z = 12.
    let mut plane_refs = Vec::new();
    for (world, want_idx) in [(Vec3::new(10.0, 0.0, 12.0), 1usize), (Vec3::new(20.0, 0.0, 12.0), 2), (Vec3::new(10.0, 10.0, 12.0), 3)] {
        let (sp, _) = px(world);
        let phits = crate::viewport::pick(&h.app, &proj, sp);
        h.app.start("construct.plane.three_points");
        let cand = crate::viewport::candidate(&h.app, &phits).expect("construct.plane.three_points accepts cloud points");
        assert!(matches!(cand.1, solidcraft_engine::Sel::PointCloudPoint { cloud: 1, index, .. } if index == want_idx), "{cand:?}");
        plane_refs.push(crate::dialogs::construct_ref(&h.app, &cand.1).expect("construct_ref"));
        h.app.dialog = None;
    }
    let built = h.call(
        "engine.execute",
        json!({"command": "construct.plane.three_points", "params": {"a": plane_refs[0], "b": plane_refs[1], "c": plane_refs[2], "name": "ScanPlane"}}),
    );
    assert_eq!(built["ok"], true, "{built}");
    let planes = h.app.session.doc.features.iter().filter(|f| f.name == "ScanPlane").count();
    assert_eq!(planes, 1, "plane created from three picked cloud points");

    // 3. Filtering out score < 0.85 hides index 1 (0.8), so the same pixel now hits index 0 (1.0, z = 2).
    h.app.session.doc_mut().point_clouds[0].filters.push(Filter { property: "score".into(), keep: Keep::Range { range: [0.85, 1.0] } });
    let r = pick(&mut h, json!({"pixels": [px1], "clouds": true}));
    assert_eq!(r["hits"][0]["point_index"], 0, "{r}");
    assert!((r["hits"][0]["point"][2].as_f64().unwrap() - 2.0).abs() < 1e-9, "{r}");

    // 4. Section plane at z = 6 removes z > 6 (index 2 at (20, 0, 12) is clipped).
    h.app.session.doc_mut().point_clouds[0].filters.clear();
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"plane": {"origin": [0, 0, 6], "normal": [0, 0, 1]}}}));
    let (_, px2) = px(Vec3::new(20.0, 0.0, 12.0));
    let r = pick(&mut h, json!({"pixels": [px2], "clouds": true}));
    assert_eq!(r["hits"][0], serde_json::Value::Null, "{r}");
    h.call("engine.execute", json!({"command": "inspect.section", "params": {"clear": true}}));

    // 5. Solid body in front (z = 15..20) occludes index 1, while `all: true` lists body then cloud.
    h.call(
        "engine.execute",
        json!({"command": "solid.box", "params": {"length": 6, "width": 6, "height": 5, "corner": [7, -3, 15], "body_name": "Cover"}}),
    );
    let r = pick(&mut h, json!({"pixels": [px1], "clouds": true}));
    assert_eq!(r["hits"][0]["body"], "Cover", "{r}");
    let r = pick(&mut h, json!({"pixels": [px1], "all": true, "clouds": true}));
    let list = r["hits"][0].as_array().unwrap();
    assert_eq!(list.len(), 2, "{r}");
    assert_eq!(list[0]["body"], "Cover", "{r}");
    assert_eq!(list[1]["cloud"], 1, "{r}");
}
