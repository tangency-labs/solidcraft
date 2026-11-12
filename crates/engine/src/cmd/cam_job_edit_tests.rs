//! Tests of the open job's commands: new, open, edit, save, reload, undo and redo through
//! `cam.job.*`, edits made outside SolidCraft, `cam.job` planning the open job; hostile
//! parameters refused.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A folder of its own under the temporary folder, removed afterwards.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let d =
            solidcraft_io::vfs::temp_dir().join(format!("solidcraft-camjob-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn file(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 40 × 30 × 20 block, its top at Z 20, with a 16 × 10 pocket 5 deep in it.
fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [-20, -15, 0], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 16, "width": 10, "height": 5, "corner": [-8, -5, 15], "operation": "cut"}));
    s
}

const POCKET: [[f64; 2]; 4] = [[-8.0, -5.0], [8.0, -5.0], [8.0, 5.0], [-8.0, 5.0]];

fn job() -> Value {
    json!({
        "name": "block",
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [{"number": 4, "kind": "flat_endmill", "diameter": 4, "flute_length": 12, "spindle_rpm": 12000, "feed": 800, "plunge_feed": 200}],
        "operations": [
            {"name": "pocket", "type": "pocket", "tool": 4, "polygon": POCKET, "stepover": 1.5,
             "heights": {"clearance": 30, "retract": 22, "top": 20, "bottom": 15}},
            {"name": "outline", "type": "contour", "tool": 4, "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
             "heights": {"clearance": 30, "retract": 22, "top": 20, "bottom": 10}}
        ]
    })
}

/// The job written by a script: one space a level, keys as the script put them.
fn write_script_job(path: &str, job: &Value) {
    let text = serde_json::to_string_pretty(job).unwrap().replace("\n  ", "\n ");
    std::fs::write(path, text).unwrap();
}

fn ops(s: &Session) -> Vec<Value> {
    s.cam_job.as_ref().unwrap().value["operations"].as_array().unwrap().clone()
}

#[test]
fn a_job_is_opened_edited_saved_and_read_back_the_same() {
    let dir = Dir::new("edit");
    let path = dir.file("block.cam.json");
    write_script_job(&path, &job());
    let mut s = block();
    let sum = run(&mut s, "cam.job.open", json!({"path": path}));
    assert_eq!((sum["name"].as_str(), sum["dirty"].as_bool(), sum["machine"].as_str()), (Some("block"), Some(false), Some("3axis")), "{sum}");
    assert_eq!(sum["operations"][1]["type"], "contour");
    assert_eq!((sum["operations"][0]["tool"].as_u64(), sum["operations"][0]["enabled"].as_bool()), (Some(4), Some(true)), "{sum}");
    let sum = run(&mut s, "cam.job.edit", json!({"set": {"tolerance": 0.02, "origin": [0, 0, 0], "tools.0.feed": 700}}));
    assert_eq!(sum["dirty"], true, "{sum}");
    let saved = run(&mut s, "cam.job.save", json!({}));
    assert_eq!(saved["written"], true, "{saved}");
    let text = std::fs::read_to_string(&path).unwrap();
    // Kept the file's one-space indent and the job's key order.
    assert!(text.starts_with("{\n \"name\": \"block\",\n \"machine\""), "{text}");
    assert!(
        text.find("\"origin\"").unwrap() < text.find("\"tolerance\"").unwrap()
            && text.find("\"tolerance\"").unwrap() < text.find("\"tools\"").unwrap()
    );
    let read: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(read, s.cam_job.as_ref().unwrap().value);
    assert_eq!(read["tools"][0]["feed"], 700, "only the key set changed");
    // Saved and unchanged: nothing more to write; opened again it is byte for byte the same file.
    assert_eq!(run(&mut s, "cam.job.save", json!({}))["written"], false);
    run(&mut s, "cam.job.open", json!({"path": path}));
    run(&mut s, "cam.job.save", json!({"overwrite": true}));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    // Save as: the job is that file from then on.
    let other = dir.file("copy.cam.json");
    run(&mut s, "cam.job.save", json!({"path": other}));
    assert_eq!(s.cam_job.as_ref().unwrap().path.as_deref(), Some(other.as_str()));
    assert!(refused(&mut s, "cam.job.save", json!({"path": path})).contains("overwrite"));
}

#[test]
fn every_edit_undoes_and_redoes() {
    let dir = Dir::new("history");
    let mut s = block();
    let sum = run(&mut s, "cam.job.new", json!({"name": "block", "path": dir.file("new.cam.json")}));
    assert_eq!((sum["dirty"].as_bool(), sum["operations"].as_array().map(Vec::len)), (Some(true), Some(0)), "{sum}");
    assert!(!std::path::Path::new(&dir.file("new.cam.json")).exists(), "nothing is written until it is saved");
    let mut setup = job();
    setup.as_object_mut().unwrap().remove("operations");
    run(&mut s, "cam.job.edit", json!({"set": setup}));
    run(&mut s, "cam.job.edit", json!({"set": {"tolerance": 0.05}}));
    run(&mut s, "cam.job.edit", json!({"unset": ["tolerance"]}));
    let get = run(&mut s, "cam.job.get", json!({"job": true}));
    assert_eq!(get["undo"], json!(["Set tolerance", "Set tolerance", "Edit the job"]), "{get}");
    assert!(get["job"].get("tolerance").is_none(), "{get}");
    assert_eq!(run(&mut s, "cam.job.undo", json!({}))["step"], "Set tolerance");
    assert_eq!(s.cam_job.as_ref().unwrap().value["tolerance"], 0.05);
    assert_eq!(run(&mut s, "cam.job.undo", json!({}))["step"], "Set tolerance");
    assert!(s.cam_job.as_ref().unwrap().value.get("tolerance").is_none());
    assert_eq!(run(&mut s, "cam.job.redo", json!({}))["step"], "Set tolerance");
    assert_eq!(s.cam_job.as_ref().unwrap().value["tolerance"], 0.05);
    // An edit after an undo drops what redo would have taken.
    run(&mut s, "cam.job.edit", json!({"set": {"name": "renamed"}}));
    assert!(run(&mut s, "cam.job.redo", json!({})).get("step").is_none());
    // A new job over unsaved edits, or closing them, is refused without `discard`.
    assert!(refused(&mut s, "cam.job.new", json!({})).contains("discard"));
    assert!(refused(&mut s, "cam.job.close", json!({})).contains("discard"));
    run(&mut s, "cam.job.save", json!({}));
    assert_eq!(run(&mut s, "cam.job.close", json!({}))["closed"], true);
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["open"], false);
    assert_eq!(run(&mut s, "cam.job.close", json!({}))["closed"], false);
}

/// A new job of a saved design is named after it, beside it, and takes the next setup's name
/// when that file is taken.
#[test]
fn a_new_job_is_named_after_the_saved_design() {
    let dir = Dir::new("named");
    let mut s = block();
    s.path = Some(dir.file("bracket.sc"));
    let sum = run(&mut s, "cam.job.new", json!({}));
    assert_eq!(sum["path"].as_str(), Some(dir.file("bracket.cam.json").as_str()), "{sum}");
    std::fs::write(dir.file("bracket.cam.json"), "{\"operations\": []}").unwrap();
    let sum = run(&mut s, "cam.job.new", json!({"discard": true}));
    assert_eq!(sum["path"].as_str(), Some(dir.file("bracket.op2.cam.json").as_str()), "{sum}");
}

#[test]
fn edits_made_outside_are_seen_reloaded_and_undone() {
    let dir = Dir::new("outside");
    let path = dir.file("block.cam.json");
    write_script_job(&path, &job());
    let mut s = block();
    run(&mut s, "cam.job.open", json!({"path": path}));
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["outside"], "none");
    // An agent changes the file: a clean job sees it and reloads with what changed, one step.
    let mut theirs = job();
    theirs["operations"][0]["stepover"] = json!(2);
    let mut spot = theirs["operations"][0].clone();
    spot["name"] = json!("spot");
    theirs["operations"].as_array_mut().unwrap().push(spot);
    write_script_job(&path, &theirs);
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["outside"], "changed");
    let r = run(&mut s, "cam.job.reload", json!({}));
    let changes: Vec<&str> = r["changes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(changes.contains(&"operation 1 'pocket': stepover 1.5 -> 2") && changes.contains(&"added operation 3 'spot'"), "{changes:?}");
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["outside"], "none");
    assert_eq!(run(&mut s, "cam.job.undo", json!({}))["step"], "Reloaded: changed outside SolidCraft");
    assert_eq!(ops(&s).len(), 2);
    // With unsaved edits here: save and reload refuse until told which to keep.
    write_script_job(&path, &job());
    run(&mut s, "cam.job.edit", json!({"set": {"tolerance": 0.02}}));
    let mut again = job();
    again["name"] = json!("theirs");
    write_script_job(&path, &again);
    assert!(refused(&mut s, "cam.job.save", json!({})).contains("changed outside"));
    assert!(refused(&mut s, "cam.job.reload", json!({})).contains("discard"));
    run(&mut s, "cam.job.save", json!({"overwrite": true}));
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!((kept["name"].as_str(), kept["tolerance"].as_f64()), (Some("block"), Some(0.02)));
    // Without file times (the web) the bytes are compared.
    s.cam_job.as_mut().unwrap().disk.as_mut().unwrap().stamp = None;
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["outside"], "none", "same bytes");
    s.cam_job.as_mut().unwrap().disk.as_mut().unwrap().stamp = None;
    write_script_job(&path, &again);
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["outside"], "changed");
    // Gone.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(run(&mut s, "cam.job.get", json!({}))["outside"], "gone");
}

#[test]
fn a_file_that_is_not_a_job_is_refused_with_its_key_and_line() {
    let dir = Dir::new("bad");
    let path = dir.file("bad.cam.json");
    std::fs::write(&path, "{\n \"operations\": [\n  {\"name\": \"p\", \"type\": \"pocket\", \"tool\": 4, \"stepovr\": 2}\n ]\n}\n").unwrap();
    let mut s = block();
    let e = refused(&mut s, "cam.job.open", json!({"path": path}));
    assert!(e.contains("`operations[0].stepovr`: unknown field") && e.contains("line 4"), "{e}");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(refused(&mut s, "cam.job.open", json!({"path": path})).contains("not valid JSON"));
    std::fs::write(&path, "[1, 2]").unwrap();
    assert!(refused(&mut s, "cam.job.open", json!({"path": path})).contains("JSON object"));
    assert!(s.cam_job.is_none());
    assert!(refused(&mut s, "cam.job.open", json!({"path": dir.file("job.txt")})).contains(".json"));
    // A new job is not written over a file already there.
    std::fs::write(dir.file("taken.cam.json"), "{}").unwrap();
    assert!(refused(&mut s, "cam.job.new", json!({"path": dir.file("taken.cam.json")})).contains("cam.job.open"));
}

#[test]
fn the_open_job_is_what_cam_job_plans() {
    let dir = Dir::new("default");
    let path = dir.file("block.cam.json");
    write_script_job(&path, &job());
    let mut s = block();
    assert!(refused(&mut s, "cam.job", json!({"dry_run": true})).contains("cam.job.open"));
    run(&mut s, "cam.job.open", json!({"path": path}));
    let r = run(&mut s, "cam.job", json!({"dry_run": true}));
    assert_eq!(r["operations"].as_array().map(Vec::len), Some(2), "{r}");
    assert_eq!(r["passed"], true, "{r}");
    // Unsaved edits are what is planned: half the feed, twice the minutes.
    let before = r["cut_minutes"].as_f64().unwrap();
    run(&mut s, "cam.job.edit", json!({"set": {"tools.0.feed": 400, "tools.0.plunge_feed": 100}}));
    let r = run(&mut s, "cam.job", json!({"dry_run": true}));
    assert!(r["cut_minutes"].as_f64().unwrap() > before * 1.5, "{r}");
    // A job given inline still takes the place of the open one.
    let mut one = job();
    one["operations"].as_array_mut().unwrap().truncate(1);
    let r = run(&mut s, "cam.job", json!({"job": one, "dry_run": true}));
    assert_eq!(r["operations"].as_array().map(Vec::len), Some(1), "{r}");
}

#[test]
fn hostile_job_edits_are_refused_and_change_nothing() {
    let mut s = block();
    for id in ["cam.job.save", "cam.job.reload", "cam.job.edit", "cam.job.undo", "cam.job.redo"] {
        assert!(refused(&mut s, id, json!({})).contains("cam.job.open"), "{id}");
    }
    run(&mut s, "cam.job.new", json!({}));
    assert!(refused(&mut s, "cam.job.save", json!({})).contains("give `path`"), "an unsaved design's new job has no file yet");
    let before = s.cam_job.as_ref().unwrap().value.clone();
    // Not a number where one goes, the operations, an unknown key, paths that are no paths.
    for set in [
        json!({"origin": [0, "x", 0]}),
        json!({"tolerance": "fine"}),
        json!({"operations": []}),
        json!({"tools.x": 5}),
        json!({"no_such_key": 1}),
        json!({"": 1}),
        json!({"a..b": 1}),
        json!({"name.inner": 1}),
    ] {
        assert!(s.execute("cam.job.edit", &json!({"set": set})).is_err(), "{set}");
    }
    assert!(s.execute("cam.job.edit", &json!({"unset": ["operations"]})).is_err());
    let mut deep = json!(1);
    for _ in 0..40 {
        deep = json!({"x": deep});
    }
    assert!(refused(&mut s, "cam.job.edit", json!({"set": {"machine": deep}})).contains("nested"));
    let many: serde_json::Map<String, Value> = (0..201).map(|i| (format!("k{i}"), json!(1))).collect();
    assert!(refused(&mut s, "cam.job.edit", json!({"set": many})).contains("at most"));
    assert_eq!(s.cam_job.as_ref().unwrap().value, before);
    assert!(s.cam_job.as_ref().unwrap().undo.is_empty(), "a refused edit is no step");
    for path in ["", "x\ny.json", "job.txt"] {
        assert!(s.execute("cam.job.open", &json!({"path": path})).is_err(), "{path:?}");
        assert!(s.execute("cam.job.save", &json!({"path": path})).is_err(), "{path:?}");
    }
    let scope = solidcraft_io::vfs::PathScope::new([Dir::new("scope").0.clone()]);
    let outside = Dir::new("elsewhere").file("x.cam.json");
    solidcraft_io::vfs::scoped(Some(scope), || {
        assert!(s.execute("cam.job.save", &json!({"path": outside})).is_err());
        assert!(s.execute("cam.job.open", &json!({"path": outside})).is_err());
    });
    assert!(!std::path::Path::new(&outside).exists());
    assert!(s.execute("cam.job.new", &json!({"name": "a\nb", "discard": true})).is_err());
    assert!(s.execute("cam.job.get", &json!({"jobs": true})).is_err());
}
