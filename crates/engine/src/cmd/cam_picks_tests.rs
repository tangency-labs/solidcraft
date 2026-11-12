//! Tests of picks: an operation's geometry by persistent face and edge names, found again after
//! model edits that renumber the features, by its point when the name finds nothing, lost when
//! the face goes; hostile picks refused.

use serde_json::{Value, json};

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Err(e @ EngineError::BadParams { .. }) => e.to_string(),
        other => panic!("{id} {p}: expected a refusal, got {other:?}"),
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// A 40 × 30 × 20 block, its top at Z 20, with a 16 × 10 pocket 5 deep at X 0 and a 5 mm hole
/// through it at (14, 0).
fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [-20, -15, 0], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 16, "width": 10, "height": 5, "corner": [-8, -5, 15], "operation": "cut"}));
    run(&mut s, "solid.cylinder", json!({"radius": 2.5, "height": 20, "base": [14, 0, 0], "axis": "Z", "operation": "cut"}));
    s
}

/// The pick of face `index` of the block: its persistent name and a point on it.
fn face_pick(s: &mut Session, index: usize) -> Value {
    let faces = run(s, "model.faces", json!({"body": "Block"}));
    let face = faces["faces"].as_array().unwrap().iter().find(|x| x["index"] == index).unwrap_or_else(|| panic!("{faces}")).clone();
    json!({"body": "Block", "face": face["name"], "at": face["point"]})
}

/// The pick of the floor of the block's pocket whose floor is at work Z `floor`.
fn floor_pick(s: &mut Session, floor: f64) -> Value {
    let feats = run(s, "cam.features", json!({}));
    let p = feats["pockets"].as_array().unwrap().iter().find(|p| (f(&p["floor_z"]) - floor).abs() < 1e-6).cloned();
    let p = p.unwrap_or_else(|| panic!("{feats}"));
    let index = p["floor_face"].as_u64().unwrap() as usize;
    face_pick(s, index)
}

fn job(ops: Value) -> Value {
    let tool = json!({"number": 4, "kind": "flat_endmill", "diameter": 4, "flute_length": 12, "spindle_rpm": 12000, "feed": 800, "plunge_feed": 200});
    json!({"machine": "3axis", "post": "linuxcnc_4axis", "tools": [tool], "operations": ops})
}

fn dry(s: &mut Session, ops: Value) -> Value {
    run(s, "cam.job", json!({"job": job(ops), "dry_run": true}))
}

#[test]
fn a_pocket_picked_by_its_floor_plans_as_its_feature_and_follows_renumbering() {
    let mut s = block();
    let floor = floor_pick(&mut s, 15.0);
    assert!(floor["face"].as_str().is_some_and(|n| !n.is_empty()), "{floor}");
    let ops = json!([
        {"name": "by pick", "type": "pocket", "tool": 4, "picks": [floor]},
        {"name": "by id", "type": "pocket", "tool": 4, "feature": "pocket:0"}
    ]);
    let r = dry(&mut s, ops);
    let (a, b) = (&r["operations"][0], &r["operations"][1]);
    assert_eq!(a["moves"], b["moves"], "the same path: {a} {b}");
    assert!((f(&a["cut_length"]) - f(&b["cut_length"])).abs() < 1e-9);
    let resolved = &r["picks_resolved"][0];
    assert_eq!(resolved["by"], "name", "{r}");
    assert_eq!(resolved["feature"], "pocket:0");
    assert_eq!(resolved["op"], json!({"index": 0, "name": "by pick"}));
    assert!(resolved.get("note").is_none(), "a pick found by its exact name has nothing to check: {resolved}");

    // A second pocket further left: it is pocket:0 now, ours pocket:1. The pick still finds ours.
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 3, "corner": [-18, -2, 17], "operation": "cut"}));
    let ids = run(&mut s, "cam.features", json!({}));
    let ours = ids["pockets"].as_array().unwrap().iter().find(|p| (f(&p["bounds"][0][0]) + 8.0).abs() < 1e-6).unwrap()["id"].clone();
    assert_eq!(ours, "pocket:1", "renumbered: {ids}");
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-cam-picks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("pick.ngc").to_string_lossy().into_owned();
    let r = run(&mut s, "cam.job", json!({"job": job(json!([{"name": "by pick", "type": "pocket", "tool": 4, "picks": [floor]}])), "output": out}));
    assert_eq!(r["picks_resolved"][0]["feature"], "pocket:1", "{}", r["picks_resolved"]);
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    let b = &c["bounds"];
    assert!(f(&b[0][0]) > -8.0 && f(&b[1][0]) < 8.0, "still the 16 x 10 pocket: {c}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pick_whose_face_is_gone_is_lost_and_refuses_the_job() {
    let mut s = block();
    let floor = floor_pick(&mut s, 15.0);
    // The hole and the pocket undone: the floor is gone, and where it was is solid.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let e = refused(&mut s, "cam.job", json!({"job": job(json!([{"name": "p", "type": "pocket", "tool": 4, "picks": [floor]}])), "dry_run": true}));
    assert!(e.contains("operation 0 `p`") && e.contains("was not found") && e.contains("Nothing was written"), "{e}");
}

#[test]
fn a_pick_found_by_its_point_says_so() {
    let mut s = block();
    let mut floor = floor_pick(&mut s, 15.0);
    floor["face"] = json!("F99:nothing:here");
    let r = dry(&mut s, json!([{"name": "p", "type": "pocket", "tool": 4, "picks": [floor]}]));
    let resolved = &r["picks_resolved"][0];
    assert_eq!(resolved["by"], "point", "{r}");
    assert_eq!(resolved["feature"], "pocket:0", "{r}");
    assert!(resolved["note"].as_str().unwrap().contains("position"), "{resolved}");
}

/// The block made taller: its top is found by its name, but 2 mm from where it was picked, and
/// the reply says the face has moved.
#[test]
fn a_pick_whose_face_moved_says_so() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [-20, -15, 0], "body_name": "Block"}));
    let feats = run(&mut s, "cam.features", json!({}));
    let top = feats["faces"].as_array().unwrap().iter().find(|x| x["role"] == "top").unwrap()["face"].as_u64().unwrap() as usize;
    let pick = face_pick(&mut s, top);
    let ops = json!([{"name": "top", "type": "pocket", "tool": 4, "picks": [pick], "heights": {"top": 21}}]);
    let r = dry(&mut s, ops.clone());
    assert!(r["picks_resolved"][0].get("note").is_none(), "{r}");
    run(&mut s, "parameters.change", json!({"name": "d3", "expression": "22 mm"}));
    let r = dry(&mut s, json!([{"name": "top", "type": "pocket", "tool": 4, "picks": [pick], "heights": {"top": 23}}]));
    let note = r["picks_resolved"][0]["note"].as_str().unwrap_or_default().to_string();
    assert!(note.contains("moved") && note.contains("2 mm"), "{r}");
}

/// A wall of the pocket picked reads as the pocket for a pocket, and the block's side wall as its
/// outline for a contour, which then runs outside it.
#[test]
fn walls_picked_read_as_the_feature_the_operation_wants() {
    let mut s = block();
    let faces = run(&mut s, "model.faces", json!({"body": "Block"}));
    let at = |x: f64, y: f64, z: f64| {
        faces["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| {
                g["plane_normal"].is_array()
                    && (f(&g["centroid"][0]) - x).abs() < 1e-6
                    && (f(&g["centroid"][1]) - y).abs() < 1e-6
                    && (f(&g["centroid"][2]) - z).abs() < 1e-6
            })
            .map(|g| json!({"body": "Block", "face": g["name"], "at": [x, y, z]}))
            .unwrap_or_else(|| panic!("no face at ({x}, {y}, {z}): {faces}"))
    };
    // The pocket's +X wall (X 8, Z 15 to 20) and the block's -Y side (Y -15).
    let wall = at(8.0, 0.0, 17.5);
    let side = at(0.0, -15.0, 10.0);
    let r = dry(
        &mut s,
        json!([{"name": "pocket", "type": "pocket", "tool": 4, "picks": [wall]}, {"name": "outline", "type": "contour", "tool": 4, "picks": [side]}]),
    );
    assert_eq!(r["picks_resolved"][0]["feature"], "pocket:0", "{r}");
    assert_eq!(r["picks_resolved"][1]["feature"], "profile:0", "{r}");
    assert_eq!(r["passed"], true, "{r}");
}

#[test]
fn an_operation_turned_off_does_not_need_its_picks() {
    let mut s = block();
    let floor = floor_pick(&mut s, 15.0);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let off = json!({"name": "gone", "type": "pocket", "tool": 4, "picks": [floor], "enabled": false});
    // A cut down to the part's top only, so the job passes its gouge and rapid checks.
    let on = json!({"name": "top", "type": "face", "tool": 4, "top": 20.5, "bottom": 20});
    let r = dry(&mut s, json!([off, on]));
    assert_eq!(r["passed"], true, "{r}");
    assert!(r.get("picks_resolved").is_none(), "{r}");
}

#[test]
fn hostile_picks_are_refused() {
    let mut s = block();
    let pocket = |picks: Value| json!({"job": job(json!([{"name": "p", "type": "pocket", "tool": 4, "picks": picks}])), "dry_run": true});
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Block", "face": "x", "at": [f64::MAX, 0, 0]}])));
    assert!(e.contains("finite"), "{e}");
    let many: Vec<Value> = (0..1001).map(|i| json!({"body": "Block", "at": [i, 0, 0]})).collect();
    let e = refused(&mut s, "cam.job", pocket(json!(many)));
    assert!(e.contains("1000"), "{e}");
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Block", "face": "F1", "edge": "E1", "at": [0, 0, 20]}])));
    assert!(e.contains("not both"), "{e}");
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Block"}])));
    assert!(e.contains("needs a `face`"), "{e}");
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Block", "face": "a\nb", "at": [0, 0, 15]}])));
    assert!(e.contains("control characters"), "{e}");
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Nope", "face": "F1"}])));
    assert!(e.contains("no body `Nope`"), "{e}");
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Block", "at": [500, 500, 500]}])));
    assert!(e.contains("no face is at"), "{e}");
    let both = json!({"job": job(json!([{"name": "p", "type": "pocket", "tool": 4, "polygon": [[0, 0], [1, 0], [1, 1]], "picks": [{"body": "Block", "at": [0, 0, 15]}]}])), "dry_run": true});
    let e = refused(&mut s, "cam.job", both);
    assert!(e.contains("not both"), "{e}");
    let e = refused(&mut s, "cam.job", pocket(json!([{"body": "Block", "face": "F1", "at": [0, 0, 15], "sketch": "S"}])));
    assert!(e.contains("sketch"), "{e}");
    assert!(s.execute("cam.job", &pocket(json!("not a list"))).is_err());
    assert!(s.execute("cam.job", &pocket(json!([5]))).is_err());
}
