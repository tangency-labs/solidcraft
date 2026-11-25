//! Review markups (`markup.*`): marking faces, following them through rebuilds, the history
//! marker and configurations, threads and closing, pictures, writers that meet, and hostile input.

use serde_json::{Value, json};

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

fn refusal(s: &mut Session, id: &str, params: Value) -> String {
    match s.execute(id, &params) {
        Ok(v) => panic!("{id} {params} should be refused: {v}"),
        Err(e) => e.to_string(),
    }
}

/// A fresh folder for one test, and a session whose design is saved there as `part.sc` (the file
/// itself need not exist: the set goes beside it).
fn session(tag: &str) -> (Session, std::path::PathBuf) {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-markup-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let s = Session { path: Some(dir.join("part.sc").to_string_lossy().to_string()), ..Default::default() };
    (s, dir)
}

fn listed(s: &mut Session) -> Vec<Value> {
    run(s, "markup.list", json!({}))["markups"].as_array().cloned().unwrap_or_default()
}

fn close(a: &Value, b: [f64; 3]) -> bool {
    a.as_array().is_some_and(|a| a.len() == 3 && a.iter().zip(b).all(|(x, y)| x.as_f64().is_some_and(|x| (x - y).abs() < 1e-6)))
}

#[test]
fn a_markup_is_added_replied_to_closed_reopened_and_deleted_without_touching_the_design() {
    let (mut s, dir) = session("round-trip");
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box", "name": "Base"}));
    let doc = s.doc.clone();
    let undo = s.undo.len();
    let added = run(&mut s, "markup.add", json!({"body": "Box", "at": [5.0, 10.0, 30.0], "text": "Round this top.", "by": "Reviewer"}));
    let id = added["markup"].as_u64().unwrap();
    assert_eq!(added["anchor"]["kind"], "face", "{added}");
    assert_eq!(added["anchor"]["body"], "Box");
    assert_eq!(added["found"]["state"], "anchored", "{added}");
    assert!(dir.join("part.sc.markups.json").is_file());

    run(&mut s, "markup.reply", json!({"markup": id, "text": "Which radius?"}));
    run(&mut s, "markup.reply", json!({"markup": id, "text": "Two millimetres.", "by": "Reviewer"}));
    let closed = run(&mut s, "markup.resolve", json!({"markup": id, "reason": "Top edges rounded 2 mm."}));
    assert_eq!(closed, json!({"markup": id, "status": "resolved"}));
    // Closed once: closing again is refused until it is reopened.
    assert!(refusal(&mut s, "markup.reject", json!({"markup": id, "reason": "no"})).contains("reopen it first"));

    let rows = listed(&mut s);
    assert_eq!(rows.len(), 1);
    let m = &rows[0];
    assert_eq!(m["status"], "resolved");
    assert_eq!(m["note"], "Round this top.");
    let thread: Vec<(&str, bool)> =
        m["thread"].as_array().unwrap().iter().map(|t| (t["text"].as_str().unwrap(), t["reviewer"].as_bool().unwrap())).collect();
    assert_eq!(thread, [("Round this top.", true), ("Which radius?", false), ("Two millimetres.", true), ("Top edges rounded 2 mm.", false)]);
    assert_eq!(m["outcome"]["by"], "agent");
    assert_eq!(m["outcome"]["reason"], "Top edges rounded 2 mm.");

    // By its guid too.
    let guid = m["guid"].as_str().unwrap().to_string();
    assert_eq!(run(&mut s, "markup.reopen", json!({"markup": guid, "reason": "Still sharp at one corner."}))["status"], "open");
    assert_eq!(run(&mut s, "markup.list", json!({"status": "open"}))["markups"].as_array().unwrap().len(), 1);
    assert_eq!(run(&mut s, "markup.list", json!({"status": "resolved"}))["counts"], json!({"open": 1, "resolved": 0, "rejected": 0}));
    run(&mut s, "markup.reject", json!({"markup": id, "reason": "The corner is a sharp by design."}));
    assert_eq!(listed(&mut s)[0]["status"], "rejected");

    assert_eq!(run(&mut s, "markup.delete", json!({"markup": id})), json!({"deleted": id}));
    assert!(listed(&mut s).is_empty());
    // Numbers are never used again.
    let next = run(&mut s, "markup.add", json!({"at": [100.0, 100.0, 100.0], "text": "Out here."}))["markup"].as_u64().unwrap();
    assert!(next > id);

    // Not one write changed the design or made an undo step.
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.undo.len(), undo);
}

#[test]
fn what_a_markup_is_on_comes_from_the_call_the_selection_or_the_view() {
    let (mut s, _dir) = session("anchors");
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box", "name": "Base"}));
    // A point in space, away from every body.
    let a = run(&mut s, "markup.add", json!({"at": [50.0, 50.0, 50.0], "text": "Room for a bracket here."}));
    assert_eq!(a["anchor"]["kind"], "point");
    // A point on a face, with no body named: the face it lies on, found exactly.
    let a = run(&mut s, "markup.add", json!({"at": [10.0, 5.0, 15.0], "text": "This side."}));
    assert_eq!(a["anchor"]["kind"], "face", "{a}");
    let side = a["anchor"]["face"].as_str().unwrap().to_string();
    // The same face by name, with no point: a point on it is chosen.
    let a = run(&mut s, "markup.add", json!({"face": side, "text": "By name."}));
    assert_eq!(a["anchor"]["face"], side);
    assert!(a["anchor"]["at"][0].as_f64().is_some_and(|x| (x - 10.0).abs() < 1e-6), "{a}");
    // A point that is not on the named face is refused.
    assert!(refusal(&mut s, "markup.add", json!({"face": side, "at": [0.0, 5.0, 15.0], "text": "x"})).contains("does not lie on"));
    // A body alone, a feature by name.
    assert_eq!(run(&mut s, "markup.add", json!({"body": "Box", "text": "Lighter."}))["anchor"]["kind"], "body");
    let f = run(&mut s, "markup.add", json!({"feature": "Base", "text": "Taller."}));
    assert_eq!(f["anchor"]["kind"], "feature");
    assert_eq!(f["anchor"]["name"], "Base");
    // The selection, when the call names nothing.
    run(&mut s, "select.set", json!({"items": [{"type": "face", "body": "Box", "point": [5.0, 10.0, 30.0]}]}));
    let a = run(&mut s, "markup.add", json!({"text": "The selected face."}));
    assert_eq!(a["anchor"]["kind"], "face", "{a}");
    assert!(close(&a["anchor"]["at"], [5.0, 10.0, 30.0]), "{a}");
    // The view alone, even with something selected: the view is what the call names.
    let view = json!({"target": [0.0, 0.0, 0.0], "yaw": 30.0, "pitch": 20.0, "distance": 200.0, "fov": 45.0});
    let a = run(&mut s, "markup.add", json!({"view": view, "text": "The view, not the selected face."}));
    assert_eq!(a["anchor"]["kind"], "view", "{a}");
    run(&mut s, "select.clear", json!({}));
    let a = run(&mut s, "markup.add", json!({"view": view, "text": "This whole view."}));
    assert_eq!(a["anchor"]["kind"], "view");
    assert_eq!(listed(&mut s).last().unwrap()["view"], view);
    // Nothing at all.
    assert!(refusal(&mut s, "markup.add", json!({"text": "On what?"})).contains("say what the markup is on"));
}

#[test]
fn an_edge_named_without_a_point_is_marked_at_a_point_on_it() {
    let (mut s, _dir) = session("edge-point");
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box", "name": "Base"}));
    let edges = run(&mut s, "model.edges", json!({"body": "Box"}))["edges"].as_array().cloned().unwrap();
    let named = |e: &Value| e["name"].as_str().map(str::to_string);
    let one = edges.iter().find(|e| named(e).is_some_and(|n| edges.iter().filter(|o| named(o).as_deref() == Some(n.as_str())).count() == 1)).unwrap();
    let a = run(&mut s, "markup.add", json!({"edge": named(one), "body": "Box", "text": "Break this edge."}));
    assert_eq!(a["anchor"]["kind"], "edge", "{a}");
    // The point kept lies on that straight edge, so the markup is found where it was made.
    let at: Vec<f64> = a["anchor"]["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let (p, q): (Vec<f64>, Vec<f64>) = (
        one["start"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect(),
        one["end"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect(),
    );
    let d: Vec<f64> = (0..3).map(|i| q[i] - p[i]).collect();
    let len2: f64 = d.iter().map(|x| x * x).sum();
    let k = ((0..3).map(|i| (at[i] - p[i]) * d[i]).sum::<f64>() / len2).clamp(0.0, 1.0);
    let off: f64 = (0..3).map(|i| (at[i] - (p[i] + k * d[i])).powi(2)).sum::<f64>().sqrt();
    assert!(off < 1e-6, "{at:?} is {off} mm off the edge {p:?}..{q:?}");
    assert_eq!(listed(&mut s).last().unwrap()["found"]["state"], "anchored");
}

#[test]
fn a_rebuild_keeps_the_face_anchor_and_says_where_it_moved() {
    let (mut s, _dir) = session("rebuild");
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box", "name": "Base"}));
    let top = run(&mut s, "markup.add", json!({"body": "Box", "at": [5.0, 10.0, 30.0], "text": "Top."}));
    let side = run(&mut s, "markup.add", json!({"body": "Box", "at": [5.0, 0.0, 10.0], "text": "Front, low down."}));
    let top_face = top["anchor"]["face"].as_str().unwrap().to_string();
    let side_face = side["anchor"]["face"].as_str().unwrap().to_string();
    assert_ne!(top_face, side_face);

    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"height": "40"}}));
    let rows = listed(&mut s);
    // The top went up 10 mm: the markup is still on it, moved.
    assert_eq!(rows[0]["anchor"]["face"], top_face, "the set keeps the face's lasting name");
    assert_eq!(rows[0]["found"]["state"], "moved", "{}", rows[0]);
    assert!(close(&rows[0]["found"]["at"], [5.0, 10.0, 40.0]), "{}", rows[0]);
    assert!(close(&rows[0]["found"]["was"], [5.0, 10.0, 30.0]));
    assert!((rows[0]["found"]["moved_by"].as_f64().unwrap() - 10.0).abs() < 1e-6);
    // The marked point still lies on the taller front face.
    assert_eq!(rows[1]["found"]["state"], "anchored", "{}", rows[1]);

    // The body renamed: found again by the feature that made it.
    run(&mut s, "body.rename", json!({"body": "Box", "name": "Block"}));
    assert_eq!(listed(&mut s)[1]["found"]["state"], "anchored");
}

#[test]
fn a_face_left_out_by_the_marker_or_a_configuration_is_hidden_and_a_deleted_one_gone() {
    let (mut s, _dir) = session("hidden");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base", "name": "Base"}));
    run(&mut s, "solid.box", json!({"corner": [20, 0, 0], "length": 5, "width": 5, "height": 5, "body_name": "Peg", "name": "Peg"}));
    let peg = run(&mut s, "markup.add", json!({"body": "Peg", "at": [22.0, 2.0, 5.0], "text": "Chamfer the peg."}))["markup"].as_u64().unwrap();
    let state = |s: &mut Session| {
        let rows = listed(s);
        let row = rows.iter().find(|m| m["id"] == json!(peg)).cloned().unwrap();
        (row["found"]["state"].as_str().unwrap().to_string(), row["found"]["why"].as_str().unwrap_or("").to_string())
    };
    assert_eq!(state(&mut s).0, "anchored");

    // Rolled back before the peg: hidden, not gone.
    run(&mut s, "timeline.rollback", json!({"position": 1}));
    let (st, why) = state(&mut s);
    assert_eq!(st, "hidden", "{why}");
    assert!(why.contains("history marker") && why.contains("Peg"), "{why}");
    run(&mut s, "timeline.rollback", json!({"position": "end"}));
    assert_eq!(state(&mut s).0, "anchored");

    // Suppressed: hidden.
    run(&mut s, "timeline.suppress", json!({"features": ["Peg"], "suppressed": true}));
    let (st, why) = state(&mut s);
    assert_eq!(st, "hidden", "{why}");
    assert!(why.contains("suppressed"), "{why}");
    run(&mut s, "timeline.suppress", json!({"features": ["Peg"], "suppressed": false}));
    assert_eq!(state(&mut s).0, "anchored");

    // Left out by another configuration: hidden.
    run(&mut s, "config.configure", json!({"features": ["Peg"]}));
    run(&mut s, "config.row", json!({"name": "No peg", "values": {"suppress:Peg": true}}));
    run(&mut s, "config.activate", json!({"row": "No peg"}));
    let (st, why) = state(&mut s);
    assert_eq!(st, "hidden", "{why}");
    let first = s.doc.configs.rows.first().map(|r| r.name.clone()).unwrap();
    run(&mut s, "config.activate", json!({"row": first}));
    assert_eq!(state(&mut s).0, "anchored");

    // Deleted: gone, in words that name no id.
    run(&mut s, "timeline.delete", json!({"features": ["Peg"]}));
    let (st, why) = state(&mut s);
    assert_eq!(st, "gone", "{why}");
    assert!(why.contains("deleted or replaced") && !why.chars().any(|c| c.is_ascii_digit()), "{why}");
}

#[test]
fn an_agent_says_it_in_280_characters_and_only_the_maker_changes_the_note() {
    let (mut s, _dir) = session("limits");
    let id = run(&mut s, "markup.add", json!({"at": [0.0, 0.0, 0.0], "text": "Here.", "by": "Reviewer"}))["markup"].as_u64().unwrap();
    let e = refusal(&mut s, "markup.reply", json!({"markup": id, "text": "x".repeat(658)}));
    assert!(e.contains("280") && e.contains("658") && e.contains("378 over"), "{e}");
    // The maker may write a long reply.
    run(&mut s, "markup.reply", json!({"markup": id, "text": "y".repeat(1000), "by": "Reviewer"}));
    // A closing reason is under the same rule.
    assert!(refusal(&mut s, "markup.resolve", json!({"markup": id, "reason": "z".repeat(300)})).contains("280"));
    assert!(refusal(&mut s, "markup.edit", json!({"markup": id, "text": "Changed."})).contains("only the markup's maker"));
    run(&mut s, "markup.edit", json!({"markup": id, "text": "Changed.", "by": "Reviewer"}));
    assert_eq!(listed(&mut s)[0]["note"], "Changed.");
    // An agent may pin it again.
    run(&mut s, "markup.edit", json!({"markup": id, "at": [1.0, 2.0, 3.0]}));
    assert!(close(&listed(&mut s)[0]["anchor"]["at"], [1.0, 2.0, 3.0]));
}

#[test]
fn a_picture_is_attached_listed_and_taken_off() {
    let (mut s, dir) = session("pictures");
    let id = run(&mut s, "markup.add", json!({"at": [0.0, 0.0, 0.0], "text": "See the picture."}))["markup"].as_u64().unwrap();
    let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    let shot = dir.join("view.png");
    std::fs::write(&shot, png).unwrap();
    let r = run(&mut s, "markup.image", json!({"markup": id, "file": shot.to_string_lossy(), "name": "screenshot"}));
    let file = r["image"].as_str().unwrap().to_string();
    // The bytes are the design's own entry, never a file beside it, never text.
    assert_eq!(s.doc.markup_images[&file].as_bytes(), png);
    assert!(!dir.join("part.sc.markups").join(&file).exists());
    let jpg = dir.join("photo.jpg");
    std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xE0, 1, 2]).unwrap();
    run(&mut s, "markup.image", json!({"markup": id, "file": jpg.to_string_lossy()}));
    let images = listed(&mut s)[0]["images"].as_array().cloned().unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0]["name"], "screenshot");
    assert_eq!(images[1]["name"], "photo.jpg");
    assert_eq!(images[0]["in_design"], true);
    // A container saves them as parts, and they open again.
    let saved = solidcraft_io::write_design_container(&s.doc).unwrap();
    assert_eq!(solidcraft_io::read_design(&saved).unwrap().markup_images, s.doc.markup_images);
    // Not a picture, Base64 text, two sources, or nothing.
    let gif = dir.join("x.gif");
    std::fs::write(&gif, b"GIF89a").unwrap();
    assert!(refusal(&mut s, "markup.image", json!({"markup": id, "file": gif.to_string_lossy()})).contains("PNG or a JPEG"));
    // Base64 text is no way in: `data` is not a parameter.
    refusal(&mut s, "markup.image", json!({"markup": id, "data": solidcraft_doc::canvas::base64_encode(&png)}));
    assert!(refusal(&mut s, "markup.image", json!({"markup": id, "file": shot.to_string_lossy(), "remove": file})).contains("one of"));
    assert!(refusal(&mut s, "markup.image", json!({"markup": id})).contains("one of"));
    run(&mut s, "markup.image", json!({"markup": id, "remove": file}));
    assert!(!s.doc.markup_images.contains_key(&file));
    assert_eq!(listed(&mut s)[0]["images"].as_array().unwrap().len(), 1);
    // Deleting the markup takes its pictures out of the design.
    let left = listed(&mut s)[0]["images"][0]["file"].as_str().unwrap().to_string();
    assert!(s.doc.markup_images.contains_key(&left));
    run(&mut s, "markup.delete", json!({"markup": id}));
    assert!(s.doc.markup_images.is_empty());
}

/// Attaching a picture is one undo step on the design, as inserting a canvas is: undo takes its
/// bytes out again, and the set's record then says the design does not hold it.
#[test]
fn attaching_a_picture_is_an_undo_step() {
    let (mut s, dir) = session("picture-undo");
    let id = run(&mut s, "markup.add", json!({"at": [0.0, 0.0, 0.0], "text": "Undo me."}))["markup"].as_u64().unwrap();
    let shot = dir.join("view.png");
    std::fs::write(&shot, [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1]).unwrap();
    run(&mut s, "markup.image", json!({"markup": id, "file": shot.to_string_lossy()}));
    assert_eq!(s.doc.markup_images.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.markup_images.is_empty());
    assert_eq!(listed(&mut s)[0]["images"][0]["in_design"], false);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(listed(&mut s)[0]["images"][0]["in_design"], true);
}

/// The window and an agent, each with its own session on the design, add at once.
#[test]
fn two_writers_at_once_lose_nothing() {
    let (s, _dir) = session("writers");
    let path = s.path.clone().unwrap();
    const EACH: usize = 20;
    let writer = |who: &'static str| {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut s = Session { path: Some(path), ..Default::default() };
            for i in 0..EACH {
                run(&mut s, "markup.add", json!({"at": [i as f64, 0.0, 0.0], "text": format!("{who} {i}"), "by": who}));
            }
        })
    };
    let (a, b) = (writer("window"), writer("agent"));
    a.join().unwrap();
    b.join().unwrap();
    let mut s = s;
    let rows = listed(&mut s);
    assert_eq!(rows.len(), 2 * EACH);
    let mut ids: Vec<u64> = rows.iter().map(|m| m["id"].as_u64().unwrap()).collect();
    ids.dedup();
    assert_eq!(ids.len(), 2 * EACH);
    for who in ["window", "agent"] {
        assert_eq!(rows.iter().filter(|m| m["by"] == who).count(), EACH);
    }
}

#[test]
fn a_set_from_a_newer_solidcraft_is_refused_and_kept() {
    let (mut s, dir) = session("newer");
    let set = dir.join("part.sc.markups.json");
    let newer = r#"{"format":2,"next_id":1,"markups":[],"areas":[]}"#;
    std::fs::write(&set, newer).unwrap();
    assert!(refusal(&mut s, "markup.list", json!({})).contains("newer SolidCraft"));
    assert!(refusal(&mut s, "markup.add", json!({"at": [0.0, 0.0, 0.0], "text": "x"})).contains("newer SolidCraft"));
    assert_eq!(std::fs::read_to_string(&set).unwrap(), newer);
}

#[test]
fn a_design_with_no_file_needs_a_set_and_a_set_is_a_markups_file() {
    let mut s = Session::default();
    assert!(refusal(&mut s, "markup.list", json!({})).contains("save it first"));
    assert!(refusal(&mut s, "markup.list", json!({"set": "/tmp/notes.json"})).contains(".markups.json"));
    let (_, dir) = session("explicit");
    let set = dir.join("review.markups.json").to_string_lossy().to_string();
    run(&mut s, "markup.add", json!({"set": set, "at": [0.0, 0.0, 0.0], "text": "In a named set."}));
    assert_eq!(run(&mut s, "markup.list", json!({"set": set}))["markups"].as_array().unwrap().len(), 1);
}

/// Every markup command, with each hostile value in each key it reads and as the whole call, on a
/// design with a set: refused or done, never a crash.
#[test]
fn hostile_markup_params_never_panic() {
    let (mut start, _dir) = session("hostile");
    run(&mut start, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
    let id = run(&mut start, "markup.add", json!({"body": "Box", "at": [5.0, 5.0, 10.0], "text": "Seed."}))["markup"].as_u64().unwrap();
    // Each call starts from the seed set, so the calls that write leave the next one a set its size.
    let set_file = format!("{}.markups.json", start.path.clone().unwrap());
    let seed = std::fs::read(&set_file).unwrap();
    let fresh = |start: &Session| {
        std::fs::write(&set_file, &seed).unwrap();
        let mut s = start.scratch();
        s.path = start.path.clone();
        s
    };
    let keys =
        ["set", "markup", "text", "by", "reason", "status", "face", "edge", "body", "at", "feature", "occurrence", "view", "file", "name", "remove"];
    let commands = [
        "markup.list",
        "markup.add",
        "markup.edit",
        "markup.reply",
        "markup.image",
        "markup.resolve",
        "markup.reject",
        "markup.reopen",
        "markup.delete",
    ];
    let mut values = crate::tests::hostile_values();
    values.extend([
        json!("../../../etc/passwd"),
        json!("x.markups.json"),
        json!("/"),
        json!("\u{0}"),
        json!([0, 0]),
        json!([1, 2, 3, 4]),
        json!(u64::MAX),
        json!("ffffffffffffffffffffffffffffffff"),
        json!({"target": [0, 0, 0], "yaw": 1e308, "pitch": 0, "distance": -1}),
        json!("x".repeat(10_000)),
    ]);
    let mut internal = Vec::new();
    for cmd in commands {
        for k in keys {
            for v in &values {
                // Each value alone, and beside a call that is otherwise good.
                for p in [json!({ k: v.clone() }), {
                    let mut p = json!({"markup": id, "text": "ok", "reason": "ok", "at": [5.0, 5.0, 10.0]});
                    p[k] = v.clone();
                    p
                }] {
                    // Never write outside the test's folder.
                    if k == "set" && v.as_str().is_some_and(|t| t.ends_with(".markups.json")) {
                        continue;
                    }
                    let mut s = fresh(&start);
                    if let Err(EngineError::Internal(id, msg)) = s.execute(cmd, &p) {
                        internal.push(format!("{id} {p}: {msg}"));
                    }
                }
            }
        }
        for v in &values {
            let mut s = fresh(&start);
            if let Err(EngineError::Internal(id, msg)) = s.execute(cmd, v) {
                internal.push(format!("{id} {v}: {msg}"));
            }
        }
    }
    assert!(internal.is_empty(), "{internal:#?}");
}
