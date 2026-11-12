//! Tests of the CAM commands: contours, pockets and 3D operations planned on designs built with
//! commands, written only inside an agent's folders and read back; posts listed and copied;
//! G-code checked; unknown keys and hostile parameters refused.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_cam::gcode5::{JointWords, Post5, parse5};

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A folder of its own under the temporary folder, removed afterwards.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let d = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-cam-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
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

fn flat(number: u32, diameter: f64) -> Value {
    json!({"number": number, "kind": "flat_endmill", "diameter": diameter, "flute_length": 15, "spindle_rpm": 12000, "feed": 800, "plunge_feed": 200})
}

/// A 40 × 30 block, its top at Z 0, with a 16 × 12 pocket 4 mm deep in it.
fn top_pocket() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [-20, -15, -10], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 16, "width": 12, "height": 4, "corner": [-8, -6, -4], "operation": "cut"}));
    s
}

fn pocket_job() -> Value {
    json!({
        "name": "pocket",
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [flat(1, 4.0)],
        "operations": [{
            "name": "top pocket", "type": "pocket", "tool": 1,
            "polygon": [[-8, -6], [8, -6], [8, 6], [-8, 6]],
            "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -4},
            "stepdown": 2, "stepover": 1.5
        }]
    })
}

fn assert_clean(r: &Value) {
    assert_eq!(r["passed"], true, "{r}");
    assert_eq!(r["readback"]["pass"], true, "{r}");
    assert_eq!(r["gouge"]["pass"], true, "{r}");
    assert_eq!(r["gouge"]["worst"].as_f64(), Some(0.0), "{r}");
    assert!(r["failures"].as_array().is_some_and(Vec::is_empty), "{r}");
}

/// The tip after each motion block of the program at `path`, and whether the block was a rapid.
fn tips(path: &str) -> Vec<(Vec3, bool)> {
    let text = std::fs::read_to_string(path).unwrap();
    parse5(&text, &[])
        .unwrap()
        .into_iter()
        .filter_map(|b| match b {
            JointWords::Rapid { linear, .. } => Some((linear, true)),
            JointWords::Feed { linear, .. } | JointWords::Arc { linear, .. } | JointWords::PlaneArc { linear, .. } => Some((linear, false)),
            JointWords::Dwell { .. } => None,
        })
        .collect()
}

#[test]
fn a_pocket_is_written_only_inside_the_agents_folders() {
    let mut s = top_pocket();
    let dir = Dir::new("pocket");
    let root = dir.0.join("allowed");
    std::fs::create_dir_all(&root).unwrap();
    let inside = root.join("pocket.ngc").to_string_lossy().into_owned();
    let page = root.join("pocket.html").to_string_lossy().into_owned();
    let outside = dir.file("pocket.ngc");
    let scope = Some(solidcraft_io::vfs::PathScope::new([root.clone()]));
    solidcraft_io::vfs::scoped(scope, || {
        let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "output": inside, "preview": page}));
        assert_clean(&r);
        assert_eq!(r["outputs"]["gcode"], inside.as_str(), "{r}");
        assert_eq!(r["outputs"]["preview"], page.as_str(), "{r}");
        assert_eq!((r["name"].as_str(), r["machine"].as_str(), r["post"].as_str()), (Some("pocket"), Some("3-axis"), Some("linuxcnc_4axis")), "{r}");
        let op = &r["operations"][0];
        assert_eq!(op["kind"], "pocket", "{r}");
        assert!(op["moves"].as_u64().is_some_and(|m| m > 10) && op["cut_length"].as_f64().is_some_and(|l| l > 50.0), "{r}");
        // The operations' minutes add up to the program's as the controller times it.
        assert!((op["cut_minutes"].as_f64().unwrap() - r["readback"]["cut_minutes"].as_f64().unwrap()).abs() < 1e-6, "{r}");
        // Outside the folders nothing is computed or written, the preview included.
        let e = s.execute("cam.job", &json!({"job": pocket_job(), "output": outside, "preview": dir.file("pocket.html")})).unwrap_err();
        assert!(e.to_string().contains("outside the folders this agent may use"), "{e}");
        let e = s.execute("cam.job", &json!({"job": pocket_job(), "output": dir.file("in-a-missing-folder/x.ngc")})).unwrap_err();
        assert!(!e.to_string().is_empty());
    });
    assert!(!PathBuf::from(&outside).exists() && !dir.0.join("pocket.html").exists());
    let text = std::fs::read_to_string(&inside).unwrap();
    assert!(text.starts_with("(pocket)"), "{text}");
    assert!(text.contains("G91.1") && text.contains("G54"), "the post's header: {text}");
    assert!(!text.contains(" A"), "a 3-axis program writes no A: {text}");
    // Every cut stays a tool radius inside the pocket's walls and goes no deeper than its floor.
    let pts = tips(&inside);
    assert!(pts.iter().filter(|(p, _)| p.z < 0.0).all(|(p, _)| p.x.abs() <= 6.0 + 1e-3 && p.y.abs() <= 4.0 + 1e-3), "{text}");
    let deepest = pts.iter().map(|(p, _)| p.z).fold(f64::INFINITY, f64::min);
    assert!((deepest + 4.0).abs() < 1e-6, "{deepest}");
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.contains("\"scenes\"") && !html.contains("/*DATA*/null") && html.contains("top pocket"), "the page holds the scenes");
    // The page draws the part's triangles and loads nothing from the network.
    assert!(!html.contains("\"tris\":[]"), "the part is drawn");
    assert!(!html.contains("http://") && !html.contains("https://") && !html.contains("<script src"), "self-contained");
    // What was written reads back as the reply says.
    let c = run(&mut s, "cam.gcode.check", json!({"path": inside, "rapid_rate": 3000}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert!(c["feed_length"].as_f64().is_some_and(|l| l > 50.0) && c["rapid_minutes"].as_f64().is_some(), "{c}");
    assert_eq!(c["g93_blocks"], 0, "{c}");
    assert!(c["rotary_ranges"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert_eq!(c["bounds"][0][2].as_f64(), Some(-4.0), "{c}");
    assert_eq!(c["bounds"][1][2].as_f64(), Some(20.0), "{c}");
}

/// A contour round a sketch's circle keeps the circle as arcs, a tool radius out from it, and the
/// program reads back through `cam.gcode.check` with those arcs measured round.
#[test]
fn a_contour_round_a_sketch_circle_is_written_with_arcs() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 5, "axis": "Z", "base": [0, 0, -5], "body_name": "Disc"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    run(&mut s, "sketch.finish", json!({}));
    let job = json!({
        "machine": "3axis",
        "post": "linuxcnc",
        "tools": [flat(3, 6.0)],
        "operations": [{
            "name": "outline", "type": "contour", "tool": 3, "sketch": "Outline",
            "heights": {"clearance": 15, "retract": 3, "top": 0, "bottom": -5}, "stepdown": 2.5
        }]
    });
    let dir = Dir::new("contour");
    let out = dir.file("disc.ngc");
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out}));
    assert_clean(&r);
    assert_eq!(r["operations"][0]["kind"], "contour", "{r}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.lines().any(|l| l.contains("G2") || l.contains("G3")), "the circle is written as arcs: {text}");
    // Every cut is on a circle of 13 mm round the axis: the profile's 10 and the tool's radius.
    let pts = tips(&out);
    assert!(pts.iter().filter(|(p, cut)| !cut && p.z < 0.0).all(|(p, _)| (p.x.hypot(p.y) - 13.0).abs() < 1e-3), "{text}");
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert!(c["arcs"].as_u64().is_some_and(|n| n >= 2), "{c}");
    // Two passes round, each 2π × 13 mm long at least.
    assert!(c["feed_length"].as_f64().is_some_and(|l| l > 2.0 * std::f64::consts::TAU * 13.0), "{c}");
    // The default contour runs outside; inside runs a tool radius in, round a hole of the same
    // circle through a plate.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "corner": [-20, -20, -5], "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 7, "axis": "Z", "base": [0, 0, -6], "operation": "cut"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    run(&mut s, "sketch.finish", json!({}));
    let mut inside = job.clone();
    inside["operations"][0]["side"] = json!("inside");
    let r = run(&mut s, "cam.job", json!({"job": inside, "output": out}));
    assert_clean(&r);
    let pts = tips(&out);
    assert!(pts.iter().filter(|(p, cut)| !cut && p.z < 0.0).all(|(p, _)| (p.x.hypot(p.y) - 7.0).abs() < 1e-3), "{pts:?}");
}

/// The program and the preview are each written whole to a temporary file beside the old one
/// and renamed over it: a second name of the old file (a hard link, as a controller's share may
/// hold) keeps the old program, and nothing is left beside them.
#[test]
fn a_job_replaces_its_files_through_a_temporary_file() {
    let mut s = top_pocket();
    let dir = Dir::new("atomic");
    let (program, page) = (dir.file("pocket.ngc"), dir.file("pocket.html"));
    for (f, alias) in [(&program, "ngc.old"), (&page, "html.old")] {
        std::fs::write(f, "old").unwrap();
        std::fs::hard_link(f, dir.0.join(alias)).unwrap();
    }
    let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "output": program, "preview": page}));
    assert_clean(&r);
    for (f, alias) in [(&program, "ngc.old"), (&page, "html.old")] {
        assert!(std::fs::read_to_string(f).unwrap().len() > 100, "{f} was written");
        assert_eq!(std::fs::read_to_string(dir.0.join(alias)).unwrap(), "old", "{f} was replaced, not written over in place");
    }
    let names: Vec<String> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

/// A dry run writes nothing and says so; a turned-off operation is not planned or written; an
/// output named with another extension than the post's is a note.
#[test]
fn a_dry_run_writes_nothing_and_a_turned_off_operation_is_left_out() {
    let mut s = top_pocket();
    let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["dry_run"], true, "{r}");
    assert!(r["outputs"]["gcode"].is_null() && r["outputs"]["preview"].is_null(), "{r}");
    assert!(r["program_lines"].as_u64().is_some_and(|n| n > 10), "{r}");
    let mut job = pocket_job();
    let mut off = job["operations"][0].clone();
    off["name"] = json!("left out");
    off["enabled"] = json!(false);
    job["operations"].as_array_mut().unwrap().push(off);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["operations"].as_array().map(Vec::len), Some(1), "{r}");
    assert!(!r["program_head"].to_string().contains("left out"), "{r}");
    job["operations"][0]["enabled"] = json!(false);
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("turned off"), "{e}");
    let dir = Dir::new("ext");
    let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "output": dir.file("pocket.tap")}));
    assert!(r["notes"].to_string().contains("writes .ngc files; this one is .tap"), "{r}");
}

/// On a machine with a 4th axis a contour or pocket runs with A held at zero.
#[test]
fn a_pocket_on_a_4th_axis_machine_holds_a_at_zero() {
    let mut s = top_pocket();
    let mut job = pocket_job();
    job["machine"] = json!("4axis_a");
    let dir = Dir::new("four");
    let out = dir.file("pocket.ngc");
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out}));
    assert_clean(&r);
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert_eq!(c["rotary_ranges"], json!([{"letter": "A", "min_deg": 0.0, "max_deg": 0.0}]), "{c}");
}

#[test]
fn a_job_file_reads_its_post_and_tools_beside_it() {
    let mut s = top_pocket();
    let dir = Dir::new("files");
    let post = run(&mut s, "cam.post.get", json!({"name": "LinuxCNC"}));
    assert_eq!(post["name"], "linuxcnc_4axis");
    let mut post = post["post"].clone();
    post["base"]["header"] = json!(["(my mill)", "G17 G21 G40 G49 G54 G80 G90 G91.1 G94"]);
    // The program-layout switches are known keys too, not refused as misspellings.
    for (k, v) in [
        ("upper_case_comments", json!(true)),
        ("comment_chars", json!(" .,=_-")),
        ("tool_list", json!(true)),
        ("operation_name_only", json!(true)),
        ("blank_lines", json!(true)),
        ("operation_start", json!(["G17 G90 G94", "{work_offset}"])),
        ("safe_retract", json!({"lines": ["G28 G91 Z0.", "G90"], "at_start": true, "at_tool_change": true, "at_end": true})),
        ("tool_length_on_first_z", json!(true)),
        ("modal_motion", json!(true)),
    ] {
        post["base"][k] = v;
    }
    std::fs::write(dir.0.join("my.post.json"), post.to_string()).unwrap();
    std::fs::write(dir.0.join("tools.json"), json!([flat(1, 4.0)]).to_string()).unwrap();
    let mut job = pocket_job();
    let o = job.as_object_mut().unwrap();
    o.remove("post");
    o.remove("tools");
    o.insert("post_path".into(), json!("my.post.json"));
    o.insert("tool_library_path".into(), json!("tools.json"));
    let job_path = dir.file("pocket.cam.json");
    std::fs::write(&job_path, job.to_string()).unwrap();
    let r = run(&mut s, "cam.job", json!({"job_path": job_path, "dry_run": true}));
    assert_clean(&r);
    assert!(r["program_head"].as_array().unwrap().iter().any(|l| l == "(my mill)"), "{r}");
    let list = run(&mut s, "cam.post.list", json!({}));
    let names: Vec<&str> = list["posts"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert_eq!(names, ["linuxcnc_4axis", "grblhal_4axis"]);
    // Every post listed is one `cam.post.get` returns, and the same post the crate builds.
    for name in names {
        let got = run(&mut s, "cam.post.get", json!({"name": name}));
        let built: Post5 = serde_json::from_value(got["post"].clone()).unwrap();
        assert_eq!(Some(built), Post5::builtin(&name.replace('_', "-")), "{name}");
    }
    // A misspelt key in the post file is refused, not ignored.
    post["base"]["decimls"] = json!(3);
    std::fs::write(dir.0.join("my.post.json"), post.to_string()).unwrap();
    let e = s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("decimls"), "{e}");
}

/// The grblHAL post writes no tool length offset, comments with semicolons and stops for the
/// operator to change the tool.
#[test]
fn a_grblhal_program_has_semicolon_comments_and_no_length_offset() {
    let mut s = top_pocket();
    let mut job = pocket_job();
    job["post"] = json!("grblhal");
    job["tools"].as_array_mut().unwrap().push(flat(2, 6.0));
    job["operations"].as_array_mut().unwrap().push(json!({
        "name": "outline", "type": "contour", "tool": 2, "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
        "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -2}
    }));
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    let head: Vec<&str> = r["program_head"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(head.first().is_some_and(|l| l.starts_with(';')), "{head:?}");
    assert!(!head.iter().any(|l| l.contains("G43")), "{head:?}");
    assert!(head.iter().any(|l| l.trim() == "M0"), "a manual tool change: {head:?}");
}

/// A program written with failed checks says so on its first line, and keeps every line where
/// it was.
#[test]
fn a_program_written_with_failed_checks_says_so_on_its_first_line() {
    let post = Post5::linuxcnc_4axis();
    let marked = super::mark_unsafe("(part)\nG21\nM30\n", &post);
    assert_eq!(marked, "(part - WRITTEN WITH FAILED CHECKS)\nG21\nM30\n");
    // A first line that is no comment is left as it is.
    assert_eq!(super::mark_unsafe("G21\nM30\n", &post), "G21\nM30\n");
    let semicolon = Post5::grblhal_4axis();
    assert_eq!(super::mark_unsafe("; part\nG21\n", &semicolon), "; part - WRITTEN WITH FAILED CHECKS\nG21\n");
}

#[test]
fn unknown_keys_are_refused_at_every_level() {
    let mut s = top_pocket();
    let with = |f: &dyn Fn(&mut Value)| {
        let mut job = pocket_job();
        f(&mut job);
        json!({"job": job, "dry_run": true})
    };
    let cases = [
        (json!({"job": pocket_job(), "dry_rn": true}), "dry_rn"),
        (with(&|j| j["tolerence"] = json!(0.01)), "tolerence"),
        (with(&|j| j["operations"][0]["stepovr"] = json!(1)), "stepovr"),
        (with(&|j| j["operations"][0]["heights"]["clear"] = json!(1)), "clear"),
        (with(&|j| j["operations"][0]["feeds"] = json!({"cut": 500, "plunge": 100, "ramp": 1})), "ramp"),
        (with(&|j| j["tools"][0]["stickot"] = json!(20)), "stickot"),
        (with(&|j| j["tools"][0]["holder"] = json!([{"diameter": 20, "lenght": 5}])), "lenght"),
        (with(&|j| j["operations"][0]["type"] = json!("pockt")), "pockt"),
        (with(&|j| j["machine"] = json!({"name": "m", "rotaries": [], "linear_limit": null})), "linear_limit"),
        (with(&|j| j["machine"] = json!("5axis")), "5axis"),
        (with(&|j| j["post"] = json!("nope")), "nope"),
    ];
    for (p, word) in cases {
        let e = s.execute("cam.job", &p).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(word), "{p}: {e}");
    }
    let e = s.execute("cam.post.list", &json!({"name": "linuxcnc"})).unwrap_err();
    assert!(e.to_string().contains("name"), "{e}");
    let e = s.execute("cam.post.get", &json!({"nme": "linuxcnc"})).unwrap_err();
    assert!(e.to_string().contains("nme"), "{e}");
    let e = s.execute("cam.gcode.check", &json!({"path": "x.nc", "rapid": 1})).unwrap_err();
    assert!(e.to_string().contains("rapid"), "{e}");
}

#[test]
fn hostile_jobs_are_refused_without_panics() {
    let mut s = top_pocket();
    let dir = Dir::new("hostile");
    let job = |f: &dyn Fn(&mut Value)| {
        let mut j = pocket_job();
        f(&mut j);
        json!({"job": j, "dry_run": true})
    };
    let pocket = pocket_job()["operations"][0].clone();
    let many: Vec<Value> = (0..201)
        .map(|i| {
            let mut o = pocket.clone();
            o["name"] = json!(format!("o{i}"));
            o
        })
        .collect();
    let heights = json!({"clearance": 20, "retract": 5, "top": 0, "bottom": -4});
    let cases = vec![
        json!({}),
        Value::Null,
        json!({"dry_run": true}),
        json!({"job": 5, "dry_run": true}),
        json!({"job": {}, "dry_run": true}),
        json!({"job": pocket_job()}),
        json!({"job": pocket_job(), "job_path": "x.json", "dry_run": true}),
        json!({"job_path": dir.file("missing.json"), "dry_run": true}),
        json!({"job": pocket_job(), "output": ""}),
        json!({"job": pocket_job(), "output": dir.file("a.ngc"), "preview": dir.file("a.ngc")}),
        job(&|j| j["operations"] = json!([])),
        job(&|j| j["operations"] = json!(many)),
        job(&|j| j["tolerance"] = json!(-1)),
        job(&|j| j["tolerance"] = json!(1e308)),
        job(&|j| j["origin"] = json!([1e308, 0, 0])),
        job(&|j| j["work_offset"] = json!(9)),
        job(&|j| j["work_offset"] = json!(-1)),
        job(&|j| j["name"] = json!("a\nb")),
        job(&|j| j["post"] = Value::Null),
        job(&|j| j["post_path"] = json!("x.json")),
        job(&|j| j["machine"] = Value::Null),
        job(&|j| j["tools"] = json!([])),
        job(&|j| j["tools"][0]["diameter"] = json!(-6)),
        job(&|j| j["tools"][0]["diameter"] = json!(1e308)),
        job(&|j| j["tools"] = json!([flat(1, 4.0), flat(1, 6.0)])),
        job(&|j| j["tools"][0]["kind"] = json!("drill")),
        job(&|j| j["tool_library_path"] = json!("x.json")),
        job(&|j| j["bodies"] = json!(["Nope"])),
        job(&|j| j["bodies"] = json!([])),
        job(&|j| j["a_limits_deg"] = json!([0, 90])),
        job(&|j| {
            j["machine"] = json!("4axis_a");
            j["a_limits_deg"] = json!([90, 0]);
        }),
        job(&|j| j["machine"] = json!({"name": "m", "rotaries": [{"letter": "A", "direction": [0, 0, 0], "center": [0, 0, 0], "carrier": "table"}]})),
        job(&|j| j["machine"] = json!(42)),
        job(&|j| j["operations"][0]["tool"] = json!(7)),
        job(&|j| j["operations"][0]["tool"] = json!(-1)),
        job(&|j| j["operations"][0]["name"] = json!("  ")),
        job(&|j| j["operations"][0]["stepover"] = json!(0)),
        job(&|j| j["operations"][0]["stepover"] = json!(1e308)),
        job(&|j| j["operations"][0]["stepdown"] = json!(-1)),
        job(&|j| j["operations"][0]["stock_to_leave"] = json!(1e308)),
        job(&|j| j["operations"][0]["ramp_angle_deg"] = json!(1e308)),
        job(&|j| j["operations"][0]["tolerance"] = json!(0)),
        job(&|j| j["operations"][0]["heights"] = json!({"clearance": -30, "retract": 5, "top": 0, "bottom": -4})),
        job(&|j| j["operations"][0]["heights"]["retract"] = json!(-10)),
        job(&|j| j["operations"][0]["heights"]["bottom"] = json!(1e308)),
        job(&|j| j["operations"][0]["feeds"] = json!({"cut": 0, "plunge": 100})),
        job(&|j| j["operations"][0]["spindle_rpm"] = json!(-5)),
        job(&|j| j["operations"][0]["profiles"] = json!([0])),
        job(&|j| j["operations"][0]["sketch"] = json!("Nope")),
        job(&|j| j["operations"][0]["polygon"] = json!([[0, 0], [1, 1]])),
        job(&|j| j["operations"][0]["polygon"] = json!([[0, 0], [1e308, 0], [0, 1]])),
        job(&|j| j["operations"][0]["polygon"] = json!([[0, 0], [1, 1], [2, 2]])),
        job(&|j| j["operations"][0]["polygon"] = json!([[0, 0], [2, 0], [2, 2], [0, 2]])),
        job(&|j| j["operations"][0] = json!({"name": "c", "type": "contour", "tool": 1, "heights": heights.clone()})),
        job(&|j| j["operations"][0] = json!({"name": "c", "type": "contour", "tool": 1, "polygon": [[0, 0], [5, 0], [0, 5]]})),
        json!({"job": pocket_job(), "dry_run": "yes"}),
        json!({"job": pocket_job(), "output": 5}),
    ];
    for p in &cases {
        match s.execute("cam.job", p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    // Junk G-code is reported, never a panic; a missing file is an error.
    let junk = dir.file("junk.nc");
    std::fs::write(&junk, "G1 X1 Y2 Z3 F100\nG2 X5 Y5\n\u{0}\u{1}garbage QQQ\nG91 G1 X1\n").unwrap();
    let c = run(&mut s, "cam.gcode.check", json!({"path": junk}));
    assert!(c["errors"].as_array().is_some_and(|e| e.len() == 1), "{c}");
    assert!(s.execute("cam.gcode.check", &json!({"path": dir.file("missing.nc")})).is_err());
    assert!(s.execute("cam.gcode.check", &json!({"path": junk, "rapid_rate": -1})).is_err());
    assert!(s.execute("cam.gcode.check", &json!({})).is_err());
    assert!(s.execute("cam.post.get", &json!({"name": "nope"})).is_err());
    assert!(s.execute("cam.post.get", &json!({})).is_err());
}

fn ball(number: u32, diameter: f64) -> Value {
    json!({"number": number, "kind": "ball_endmill", "diameter": diameter, "flute_length": 12, "spindle_rpm": 12000, "feed": 900, "plunge_feed": 250})
}

/// A 40 mm square block, its top at Z 0, with a dome of radius 12 rising 7 mm out of it.
fn dome() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "corner": [-20, -20, -20], "body_name": "Block"}));
    run(&mut s, "solid.sphere", json!({"radius": 12, "center": [0, 0, -5], "operation": "join"}));
    s
}

fn dome_job() -> Value {
    json!({
        "name": "dome",
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [ball(2, 6.0)],
        "operations": [{
            "name": "finish", "type": "parallel", "tool": 2,
            "heights": {"clearance": 30, "retract": 15, "top": 7, "bottom": -1},
            "stepover": 1.5, "boundary": {"rect": [[-14, -14], [14, 14]]}
        }]
    })
}

/// How far the tip `p` of a ball of radius `r` is into the dome of `dome` (a sphere of radius
/// 12 about (0, 0, -5) on a block whose top is Z 0): its centre's distance inside the sphere
/// grown by `r`, or below the top plane by `r`, whichever is less deep.
fn into_dome(p: Vec3, r: f64) -> f64 {
    let c = Vec3::new(p.x, p.y, p.z + r);
    let sphere = (12.0 + r) - c.x.hypot(c.y).hypot(c.z + 5.0);
    let plane = r - c.z;
    if p.x.abs() > 20.0 || p.y.abs() > 20.0 { 0.0 } else { sphere.max(plane) }
}

#[test]
fn a_parallel_finish_over_a_dome_is_written_and_stays_on_its_surface() {
    let mut s = dome();
    let dir = Dir::new("dome");
    let out = dir.file("dome.ngc");
    let page = dir.file("dome.html");
    let scope = Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()]));
    let r = solidcraft_io::vfs::scoped(scope, || run(&mut s, "cam.job", json!({"job": dome_job(), "output": out, "preview": page})));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "parallel", "{r}");
    assert!(op["moves"].as_u64().is_some_and(|m| m > 100) && op["cut_length"].as_f64().is_some_and(|l| l > 300.0), "{r}");
    assert!((op["cut_minutes"].as_f64().unwrap() - r["readback"]["cut_minutes"].as_f64().unwrap()).abs() < 1e-6, "{r}");
    assert!(r["notes"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.contains("tessellated"))), "{r}");
    let pts = tips(&out);
    // The ball's tip reaches the dome's top and never cuts into it (the job's tolerance less
    // what the tessellation may be off by).
    let worst = pts.iter().filter(|(_, rapid)| !rapid).map(|(p, _)| into_dome(*p, 3.0)).fold(0.0, f64::max);
    assert!(worst < 0.01, "{worst}");
    let top = pts.iter().filter(|(_, rapid)| !rapid).map(|(p, _)| p.z).fold(f64::NEG_INFINITY, f64::max);
    assert!((top - 7.0).abs() < 0.05, "{top}");
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.contains("\"scenes\"") && html.contains("finish"), "the page holds the scenes");
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert_eq!(c["g93_blocks"], 0, "{c}");
}

/// A parallel finish over a 20 mm square boss 10 mm high on a plate: the moves down the boss's
/// walls inside a pass run at `feeds.descent`, the cut feed by default; at the plunge feed when
/// that is given as the descent feed; and never slowed with `descent_limit_deg` 90.
#[test]
fn a_parallel_finish_runs_down_walls_at_the_descent_feed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, -10], "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [-10, -10, 0], "operation": "join"}));
    let job = |feeds: Value, limit: Option<f64>| {
        let mut j = dome_job();
        let op = &mut j["operations"][0];
        op["heights"] = json!({"clearance": 30, "retract": 15, "top": 10, "bottom": -1});
        op["boundary"] = json!({"rect": [[-16, -16], [16, 16]]});
        op["feeds"] = feeds;
        if let Some(l) = limit {
            op["descent_limit_deg"] = json!(l);
        }
        json!({"job": j, "dry_run": true})
    };
    let minutes = |r: &Value| r["operations"][0]["cut_minutes"].as_f64().unwrap();
    let fast = run(&mut s, "cam.job", job(json!({"cut": 900, "plunge": 250}), None));
    assert_clean(&fast);
    let slow = run(&mut s, "cam.job", job(json!({"cut": 900, "plunge": 250, "descent": 250}), None));
    assert_clean(&slow);
    let never = run(&mut s, "cam.job", job(json!({"cut": 900, "plunge": 250, "descent": 250}), Some(90.0)));
    assert_clean(&never);
    assert!(minutes(&slow) > minutes(&fast) * 1.2, "{} {}", minutes(&slow), minutes(&fast));
    assert!((minutes(&never) - minutes(&fast)).abs() < 1e-9, "{} {}", minutes(&never), minutes(&fast));
    // A descent feed between the plunge and the cut feed is kept as the program is written.
    let mid = run(&mut s, "cam.job", job(json!({"cut": 900, "plunge": 250, "descent": 575}), None));
    assert_clean(&mid);
    assert!(minutes(&mid) < minutes(&slow) && minutes(&mid) > minutes(&fast), "{} {} {}", minutes(&fast), minutes(&mid), minutes(&slow));
    let e = s.execute("cam.job", &job(json!({"cut": 900, "plunge": 250, "descent": 0}), None)).unwrap_err();
    assert!(e.to_string().contains("descent feed"), "{e}");
}

/// Roughing the dome in levels leaves its `stock_to_leave` on it; a waterline round the boss of a
/// plate runs a tool radius out from its walls at every level.
#[test]
fn roughing_leaves_its_stock_and_a_waterline_follows_the_walls() {
    let mut s = dome();
    let dir = Dir::new("rough");
    let scope = Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()]));
    let mut job = dome_job();
    job["tools"] = json!([flat(1, 6.0)]);
    job["operations"] = json!([{
        "name": "rough", "type": "roughing", "tool": 1, "heights": {"clearance": 30, "retract": 15, "top": 7, "bottom": 0},
        "stepdown": 2, "stock_to_leave": 0.5, "boundary": {"rect": [[-16, -16], [16, 16]]}
    }]);
    let out = dir.file("rough.ngc");
    let r = solidcraft_io::vfs::scoped(scope.clone(), || run(&mut s, "cam.job", json!({"job": job, "output": out})));
    assert_clean(&r);
    assert_eq!(r["operations"][0]["kind"], "roughing", "{r}");
    let pts = tips(&out);
    let cuts: Vec<Vec3> = pts.iter().filter(|(_, rapid)| !rapid).map(|(p, _)| *p).collect();
    assert!(cuts.len() > 20, "{}", cuts.len());
    // A flat end: its rim stays half a millimetre off the sphere and the levels stop at the top.
    for p in &cuts {
        let reach = p.x.hypot(p.y);
        let surface = if reach < 3.0 + 12.0 { (144.0_f64 - (reach - 3.0).max(0.0).powi(2)).max(0.0).sqrt() - 5.0 } else { 0.0 };
        assert!(p.z >= surface.max(0.0) + 0.5 - 0.02, "{p:?} under {surface}");
    }

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, -10], "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [-10, -10, 0], "operation": "join"}));
    let mut job = dome_job();
    job["tools"] = json!([flat(1, 6.0)]);
    job["operations"] = json!([{
        "name": "walls", "type": "waterline", "tool": 1, "heights": {"clearance": 30, "retract": 15, "top": 9, "bottom": 1},
        "stepdown": 2, "step": 0.5
    }]);
    let out = dir.file("walls.ngc");
    let r = solidcraft_io::vfs::scoped(scope, || run(&mut s, "cam.job", json!({"job": job, "output": out})));
    assert_clean(&r);
    let cuts: Vec<Vec3> = tips(&out).into_iter().filter(|(p, rapid)| !rapid && p.z < 9.5).map(|(p, _)| p).collect();
    assert!(cuts.len() > 20, "{}", cuts.len());
    let levels: std::collections::BTreeSet<i64> = cuts.iter().map(|p| (p.z * 1000.0).round() as i64).collect();
    assert!(levels.len() >= 4, "{levels:?}");
    // Every point on a level is a tool radius from the boss: its square grown by 3 mm.
    for p in &cuts {
        let (dx, dy) = ((p.x.abs() - 10.0).max(0.0), (p.y.abs() - 10.0).max(0.0));
        let out = if dx > 0.0 && dy > 0.0 { dx.hypot(dy) } else { dx.max(dy) };
        assert!((out - 3.0).abs() < 0.05, "{p:?}: {out}");
    }
}

/// A 40 × 30 floor 10 mm thick, its top at Z 0, with a block 8 mm tall standing on its +X half:
/// a sharp concave corner along Y at X 0.
fn stepped_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [-20, -15, -10], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 8, "corner": [0, -15, 0], "operation": "join"}));
    s
}

fn finish_job(tools: Value, op: Value) -> Value {
    json!({"name": "finish", "machine": "3axis", "post": "linuxcnc_4axis", "tools": tools, "operations": [op]})
}

#[test]
fn pencil_and_rest_finishing_cut_the_corner_of_a_stepped_block() {
    let mut s = stepped_block();
    let dir = Dir::new("corner");
    let h = json!({"clearance": 30, "retract": 15, "top": 8, "bottom": -1});
    let b = json!({"rect": [[-14, -10], [10, 10]]});
    let tools = json!([ball(1, 10.0), ball(2, 4.0)]);
    let scope = Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()]));
    // The pencil runs along the corner: the 4 mm ball's tip stops 2 mm short of the wall.
    let pencil = json!({"name": "pencil", "type": "pencil", "tool": 2, "heights": h, "boundary": b});
    let out = dir.file("pencil.ngc");
    let r = solidcraft_io::vfs::scoped(scope.clone(), || run(&mut s, "cam.job", json!({"job": finish_job(tools.clone(), pencil), "output": out})));
    assert_clean(&r);
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    let (lo, hi) = (&c["bounds"][0], &c["bounds"][1]);
    assert!((lo[0].as_f64().unwrap() + 2.0).abs() < 0.05 && (hi[0].as_f64().unwrap() + 2.0).abs() < 0.05, "{c}");
    assert!(lo[2].as_f64().unwrap().abs() < 0.01, "{c}");
    // Rest finishing after the 10 mm ball cuts only between where the two tips stop, and a
    // little round it.
    let rest = json!({"name": "rest", "type": "rest_finish", "tool": 2, "previous_tool": 1, "heights": h, "scallop_height": 0.01, "overlap": 0.5, "boundary": b});
    let out = dir.file("rest.ngc");
    let r = solidcraft_io::vfs::scoped(scope, || run(&mut s, "cam.job", json!({"job": finish_job(tools.clone(), rest.clone()), "output": out})));
    assert_clean(&r);
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    let (lo, hi) = (c["bounds"][0][0].as_f64().unwrap(), c["bounds"][1][0].as_f64().unwrap());
    assert!(lo > -5.0 - 0.5 - 0.3 && hi < -2.0 + 0.05, "{c}");
    assert!(c["feed_length"].as_f64().is_some_and(|l| l > 100.0), "{c}");
    // Both a scallop height and a stepover, an unknown previous tool, values out of range and
    // a misspelt key: refused.
    for (f, word) in [
        (json!({"stepover": 0.5}), "not both"),
        (json!({"previous_tool": 9}), "previous_tool"),
        (json!({"threshold": -1}), "threshold"),
        (json!({"overlap": 1e308}), "overlap"),
        (json!({"previous_tol": 1}), "previous_tol"),
    ] {
        let mut op = rest.clone();
        for (k, v) in f.as_object().unwrap() {
            op[k] = v.clone();
        }
        let e = s.execute("cam.job", &json!({"job": finish_job(tools.clone(), op), "dry_run": true})).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(word), "{f}: {e}");
    }
    for op in [
        json!({"name": "x", "type": "steep_shallow", "tool": 2, "heights": h, "threshold_deg": 95}),
        json!({"name": "x", "type": "steep_shallow", "tool": 2, "heights": h, "angle_deg": 10}),
        json!({"name": "x", "type": "pencil", "tool": 2, "heights": h, "passes": 100000}),
        json!({"name": "x", "type": "pencil", "tool": 2, "heights": h, "min_angle_deg": f64::MAX}),
        json!({"name": "x", "type": "scallop", "tool": 2, "heights": h, "scallop_height": 5}),
        json!({"name": "x", "type": "scallop", "tool": 2, "heights": h, "step": 1e-9}),
        json!({"name": "x", "type": "scallop", "tool": 2, "heights": h, "direction": "sideways"}),
        json!({"name": "x", "type": "parallel", "tool": 2, "heights": h, "boundary": {"rect": [[5, 5], [-5, -5]]}}),
        json!({"name": "x", "type": "parallel", "tool": 2, "heights": h, "boundary": {"holes": [[[0, 0], [1, 0], [0, 1]]]}}),
        json!({"name": "x", "type": "parallel", "tool": 2, "heights": h, "descent_limit_deg": 120}),
        json!({"name": "x", "type": "parallel", "tool": 2, "heights": h, "stepover": 1e308}),
        json!({"name": "x", "type": "roughing", "tool": 2, "heights": h, "stepdown": 0}),
        json!({"name": "x", "type": "waterline", "tool": 2, "heights": h, "step": -1}),
        json!({"name": "x", "type": "waterline", "tool": 2, "heights": h, "boundry": {}}),
    ] {
        match s.execute("cam.job", &json!({"job": finish_job(tools.clone(), op.clone()), "dry_run": true})) {
            Err(EngineError::Internal(_, msg)) => panic!("{op}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{op}: accepted: {v}"),
        }
    }
    // A 3D operation needs a part.
    let mut empty = Session::default();
    let e = empty
        .execute("cam.job", &json!({"job": finish_job(tools, json!({"name": "x", "type": "parallel", "tool": 2, "heights": h})), "dry_run": true}))
        .unwrap_err();
    assert!(e.to_string().contains("need a part"), "{e}");
}

#[test]
fn steep_shallow_and_outward_scallop_on_a_stepped_block_read_back() {
    let mut s = stepped_block();
    let dir = Dir::new("steep");
    let scope = Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()]));
    let h = json!({"clearance": 30, "retract": 15, "top": 8, "bottom": -1});
    let b = json!({"rect": [[-14, -10], [14, 10]]});
    let tools = json!([ball(1, 10.0), ball(2, 6.0)]);
    // The z-levels down the wall at X 0 and the passes on the floor and the top, written and
    // read back; climb and conventional, inward and outward.
    for (file, op) in [
        (
            "steep.ngc",
            json!({"name": "split", "type": "steep_shallow", "tool": 2, "heights": h, "stepover": 0.6, "threshold_deg": 45, "boundary": b}),
        ),
        (
            "raster.ngc",
            json!({"name": "split", "type": "steep_shallow", "tool": 2, "heights": h, "stepover": 0.6, "shallow": "parallel", "climb": false, "boundary": b}),
        ),
        ("out.ngc", json!({"name": "offsets", "type": "scallop", "tool": 2, "heights": h, "stepover": 0.6, "direction": "outward", "boundary": b})),
    ] {
        let out = dir.file(file);
        let r = solidcraft_io::vfs::scoped(scope.clone(), || {
            run(&mut s, "cam.job", json!({"job": finish_job(tools.clone(), op.clone()), "output": out}))
        });
        assert_clean(&r);
        assert!(r["operations"][0]["moves"].as_u64().is_some_and(|m| m > 100), "{r}");
        let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
        // The ball's tip never below the floor's top, nor outside the boundary.
        let (lo, hi) = (&c["bounds"][0], &c["bounds"][1]);
        assert!(lo[2].as_f64().is_some_and(|z| z > -1e-3), "{file}: {c}");
        assert!(lo[0].as_f64().is_some_and(|x| x > -14.0 - 1e-6) && hi[0].as_f64().is_some_and(|x| x < 14.0 + 1e-6), "{file}: {c}");
        // Every cut stays off the block: the ball's centre a radius from the floor and the step.
        for (p, rapid) in tips(&out) {
            if rapid {
                continue;
            }
            let c = Vec3::new(p.x, p.y, p.z + 3.0);
            let (dx, dz) = ((-c.x).max(0.0), (c.z - 8.0).max(0.0));
            let off_step = if c.x >= 0.0 {
                c.z - 8.0
            } else if c.z <= 8.0 {
                -c.x
            } else {
                dx.hypot(dz)
            };
            assert!(c.z - 0.0 >= 3.0 - 0.01 && off_step >= 3.0 - 0.01, "{file}: {p:?}");
        }
    }
    // Rest finishing where the previous tool left nothing (the flat top): a job with no cut,
    // not an error.
    let rest =
        json!({"name": "rest", "type": "rest_finish", "tool": 2, "previous_tool": 1, "heights": h, "boundary": {"rect": [[4, -10], [14, 10]]}});
    let r = run(&mut s, "cam.job", json!({"job": finish_job(tools, rest), "dry_run": true}));
    assert_eq!(r["passed"], true, "{r}");
}

/// A block 40 × 30 × 30 about the X axis with a 12 × 12 pocket 5 mm deep in its +Y face.
fn pocketed_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 30, "corner": [-20, -15, -15], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 12, "width": 10, "height": 12, "corner": [-6, 10, -6], "operation": "cut"}));
    s
}

fn side_pocket_job() -> Value {
    json!({
        "machine": "4axis_a",
        "post": "linuxcnc_4axis",
        "tools": [flat(1, 4.0)],
        "operations": [{
            "type": "indexed", "a_deg": 90,
            "operation": {
                "name": "side pocket", "type": "pocket", "tool": 1,
                "polygon": [[-6, -6], [6, -6], [6, 6], [-6, 6]],
                "heights": {"clearance": 40, "retract": 20, "top": 15, "bottom": 10},
                "stepdown": 2.5, "stepover": 1.5
            }
        }]
    })
}

/// A cylinder of radius 10 along X from -20 to 20.
fn bar() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40, "axis": "X", "base": [-20, 0, 0], "body_name": "Bar"}));
    s
}

fn barrel_job() -> Value {
    json!({
        "machine": "4axis_a",
        "post": "linuxcnc_4axis",
        "tools": [ball(3, 4.0)],
        "operations": [{
            "name": "barrel", "type": "rotary_finish", "tool": 3,
            "pattern": {"type": "lines", "angle_step_deg": 15},
            "clearance": 25, "retract": 15, "range": [-15, 15]
        }]
    })
}

/// The cuts of a written program as `(x, a in radians, z)`: on the 4th axis the tip is under
/// the tool at Y 0, Z its radius.
fn written_cuts(text: &str) -> Vec<(f64, f64, f64)> {
    use solidcraft_cam::kinematics::AxisLetter;
    parse5(text, &[AxisLetter::A])
        .unwrap()
        .into_iter()
        .filter_map(|b| match b {
            JointWords::Feed { linear, rotary, .. } => Some((linear.x, rotary[0], linear.z)),
            _ => None,
        })
        .collect()
}

/// The frame A = 90° brings under the tool has its Z along work +Y, as docs/cam-jobs.md says:
/// a pocket indexed there cuts the block's side.
#[test]
fn a_pocket_indexed_at_90_degrees_cuts_the_side_and_keeps_its_arcs() {
    let frame = solidcraft_cam::indexed::frame_for_index(&solidcraft_cam::kinematics::Machine::four_axis_a(), &[90f64.to_radians()]).unwrap();
    assert!((frame.z.y - 1.0).abs() < 1e-9, "{frame:?}");
    let mut s = pocketed_block();
    let dir = Dir::new("side");
    let out = dir.file("side.ngc");
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": side_pocket_job(), "output": out}))
    });
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["name"], "side pocket", "{r}");
    assert_eq!(op["kind"], "indexed pocket", "{r}");
    assert_eq!(op["a_deg"].as_f64(), Some(90.0), "{r}");
    assert!((op["max_tilt_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6, "the tool comes in along work +Y: {r}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("A90"), "{text}");
    assert!(text.contains("G2") || text.contains("G3"), "the pocket's arcs are kept: {text}");
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert_eq!(c["rotary_ranges"][0]["letter"], "A", "{c}");
    // In program coordinates the pocket's floor is 10 mm up the turned block, its walls a tool
    // radius in from the 12 mm square.
    for (x, _, z) in written_cuts(&text) {
        assert!(z >= 10.0 - 1e-6 && x.abs() <= 4.0 + 1e-3, "{x} {z}");
    }
    let preview = dir.file("side.html");
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": side_pocket_job(), "output": dir.file("again.ngc"), "preview": preview}))
    });
    assert!(std::fs::read_to_string(&preview).unwrap().contains("3+1 A90"), "the preview says the setup");
    // An indexed operation needs one rotary axis, and an angle.
    let mut job = side_pocket_job();
    job["machine"] = json!("3axis");
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("one rotary axis"), "{e}");
    let mut job = side_pocket_job();
    job["operations"][0]["a_deg"] = json!(1e308);
    assert!(s.execute("cam.job", &json!({"job": job, "dry_run": true})).is_err());
}

#[test]
fn a_rotary_finish_on_a_bar_reads_back_and_turns_a_full_turn() {
    let mut s = bar();
    let dir = Dir::new("bar");
    let out = dir.file("bar.ngc");
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": barrel_job(), "output": out}))
    });
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "rotary_finish", "{r}");
    assert!(op["max_tilt_deg"].as_f64().is_some_and(|t| t > 170.0), "the tool goes all round: {r}");
    assert!((op["cut_minutes"].as_f64().unwrap() - r["readback"]["cut_minutes"].as_f64().unwrap()).abs() < 1e-6, "{r}");
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert!(c["g93_blocks"].as_u64().is_some_and(|n| n > 0), "cuts that turn A are in inverse time: {c}");
    let a = &c["rotary_ranges"][0];
    assert_eq!(a["letter"], "A", "{c}");
    assert!(a["max_deg"].as_f64().unwrap() - a["min_deg"].as_f64().unwrap() >= 330.0, "{c}");
    // The tip stays on the bar: never inside radius 10 less the tolerance, in machine Z.
    let low = c["bounds"][0][2].as_f64().unwrap();
    assert!(low > 9.9, "{c}");
    // Rings step over exactly in the written program, a whole turn of A each.
    let mut job = barrel_job();
    job["operations"][0]["pattern"] = json!({"type": "rings", "stepover": 2.5});
    let out = dir.file("rings.ngc");
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": job, "output": out}))
    });
    assert_clean(&r);
    let cuts = written_cuts(&std::fs::read_to_string(&out).unwrap());
    let mut rings: Vec<(f64, f64, f64)> = Vec::new();
    for &(x, a, _) in &cuts {
        match rings.last_mut() {
            Some((rx, lo, hi)) if (x - *rx).abs() <= 1e-3 => {
                *lo = lo.min(a);
                *hi = hi.max(a);
            }
            _ => rings.push((x, a, a)),
        }
    }
    rings.retain(|(_, lo, hi)| hi - lo > 6.0);
    assert_eq!(rings.len(), 13, "{rings:?}");
    for (k, (x, lo, hi)) in rings.iter().enumerate() {
        assert!((x - (-15.0 + 2.5 * k as f64)).abs() <= 1e-3, "ring {k} at {x}");
        assert!(hi - lo >= std::f64::consts::TAU - 0.07, "ring {k} turns {}", hi - lo);
    }
}

/// Roughing round stock of radius 14 down to the bar leaves its `stock_to_leave`, and refuses to
/// start without the stock's radius.
#[test]
fn a_rotary_rough_takes_the_round_stock_down_to_the_bar() {
    let mut s = bar();
    let mut job = barrel_job();
    job["operations"][0] = json!({
        "name": "rough", "type": "rotary_roughing", "tool": 3, "pattern": {"type": "lines", "angle_step_deg": 10},
        "stock_radius": 14, "clearance": 20, "retract": 15, "range": [-15, 15], "stepdown": 1.5, "stock_to_leave": 0.5
    });
    let dir = Dir::new("rough");
    let out = dir.file("rough.ngc");
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": job, "output": out}))
    });
    assert_clean(&r);
    assert_eq!(r["operations"][0]["kind"], "rotary_roughing", "{r}");
    let cuts = written_cuts(&std::fs::read_to_string(&out).unwrap());
    let lowest = cuts.iter().map(|c| c.2).fold(f64::INFINITY, f64::min);
    assert!((10.5 - 0.02..12.0).contains(&lowest), "the last level keeps the stock to leave off the bar: {lowest}");
    job["operations"][0].as_object_mut().unwrap().remove("stock_radius");
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("stock_radius"), "{e}");
}

/// The bar with a slot wrapped round it: 6 mm wide along X, 2 mm deep, a quarter turn round
/// from A −45° to +45° (a rectangle on XZ above the axis revolved about X both ways, cut).
fn slotted_bar() -> Session {
    let mut s = bar();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Slot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-3, -8], "p1": [3, -12]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch": "Slot", "axis": "X", "angle": "90 deg", "direction": "symmetric", "operation": "cut"}));
    s
}

/// The slot of `slotted_bar` as a wrapped operation of `kind` with a 4 mm flat endmill.
fn slot_job(kind: &str) -> Value {
    let q = 10.0 * std::f64::consts::FRAC_PI_4;
    let mut op = json!({
        "name": "slot", "type": kind, "tool": 1, "radius": 10,
        "polygon": [[-3, -q], [3, -q], [3, q], [-3, q]],
        "bottom": -2, "stepdown": 1
    });
    if kind == "wrap_contour" {
        op["side"] = json!("inside");
    } else {
        op["stepover"] = json!(1.5);
    }
    json!({"machine": "4axis_a", "post": "linuxcnc_4axis", "tools": [flat(1, 4.0)], "operations": [op]})
}

#[test]
fn a_wrapped_slot_is_cut_to_its_width_at_the_floor_radius() {
    for kind in ["wrap_pocket", "wrap_contour"] {
        let mut s = slotted_bar();
        let dir = Dir::new("slot");
        let out = dir.file("slot.ngc");
        let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
            run(&mut s, "cam.job", json!({"job": slot_job(kind), "output": out}))
        });
        assert_clean(&r);
        assert_eq!(r["operations"][0]["kind"], kind, "{r}");
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("G93"), "cuts that turn A are in inverse time");
        let cuts = written_cuts(&text);
        // The floor is at radius 8, exactly, and nothing goes deeper.
        let lowest = cuts.iter().map(|c| c.2).fold(f64::INFINITY, f64::min);
        assert!((lowest - 8.0).abs() <= 1e-3, "{kind}: {lowest}");
        let floor: Vec<&(f64, f64, f64)> = cuts.iter().filter(|c| (c.2 - 8.0).abs() <= 1e-3).collect();
        let (xlo, xhi) = floor.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c.0), hi.max(c.0)));
        let (alo, ahi) = floor.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c.1), hi.max(c.1)));
        // Unwrapped at the floor: the slot's 6 mm along X and its quarter turn round, at
        // radius 8, are the centres' travel plus the tool's 4 mm.
        assert!((xhi - xlo + 4.0 - 6.0).abs() <= 2e-3, "{kind}: {xlo}..{xhi}");
        let arc = (ahi - alo) * 8.0 + 4.0;
        assert!((arc - 8.0 * std::f64::consts::FRAC_PI_2).abs() <= 0.02, "{kind}: {arc}");
        assert!(alo > -std::f64::consts::FRAC_PI_4 && ahi < std::f64::consts::FRAC_PI_4, "{kind}: A within the slot");
    }
    // A path wrapped round the bar: a groove going a third of the way round as it runs along X.
    let mut s = bar();
    let mut job = slot_job("wrap_pocket");
    job["operations"] = json!([{
        "name": "groove", "type": "wrap", "tool": 1, "radius": 10, "gouge_check": false,
        "points": [[-10, 0, -0.5], [0, 10, -0.5], [10, 20, -0.5]]
    }]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["operations"][0]["kind"], "wrap", "{r}");
    job["operations"][0]["points"] = json!([[0, 0, 5]]);
    assert!(s.execute("cam.job", &json!({"job": job, "dry_run": true})).is_err());
}

/// A bar 40 long with a 30 mm square section about X: a flat face 15 mm out at each quarter turn.
fn square_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 30, "corner": [-20, -15, -15], "body_name": "Block"}));
    s
}

fn face_op(name: &str, clearance: f64) -> Value {
    json!({
        "name": name, "type": "parallel", "tool": 1,
        "heights": {"clearance": clearance, "retract": 17, "top": 15, "bottom": 0},
        "stepover": 3, "boundary": {"rect": [[-10, -6], [10, 6]]}
    })
}

fn faces_job(operations: Value) -> Value {
    json!({"machine": "4axis_a", "post": "linuxcnc_4axis", "tools": [flat(1, 4.0)], "operations": operations})
}

/// A post file in `dir` that goes to the machine's top (`G53 G0 Z0`) before A turns, in place of
/// rising to the clearance height: such a post is never refused for a clearance the part reaches.
fn with_top_turning_post(mut job: Value, dir: &Dir) -> Value {
    let mut post = Post5::linuxcnc_4axis();
    post.safe_rotate = vec!["G53 G0 Z0".into()];
    post.clearance_retract = false;
    let path = dir.file("top-turning.post.json");
    std::fs::write(&path, serde_json::to_string(&post).unwrap()).unwrap();
    if let Some(o) = job.as_object_mut() {
        o.remove("post");
        o.insert("post_path".into(), json!(path));
    }
    job
}

#[test]
fn an_indexed_pattern_is_the_indexed_operations_it_stands_for() {
    let mut s = square_block();
    let dir = Dir::new("pattern");
    let write = |s: &mut Session, job: Value, file: &str| {
        let out = dir.file(file);
        let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
            run(s, "cam.job", json!({"job": job, "output": out}))
        });
        assert_clean(&r);
        (r, std::fs::read_to_string(&out).unwrap())
    };
    let pattern = faces_job(json!([{"type": "indexed_pattern", "a_deg": [0, 90, 180, 270], "operation": face_op("face", 40.0)}]));
    let (r, text) = write(&mut s, pattern, "pattern.ngc");
    let four: Vec<Value> =
        [0, 90, 180, 270].iter().map(|a| json!({"type": "indexed", "a_deg": a, "operation": face_op(&format!("face A{a}"), 40.0)})).collect();
    let (r4, text4) = write(&mut s, faces_job(json!(four)), "four.ngc");
    assert_eq!(text, text4, "the pattern writes what the four indexed operations write");
    assert_eq!(r["operations"], r4["operations"]);
    let ops = r["operations"].as_array().unwrap();
    let names: Vec<&str> = ops.iter().filter_map(|o| o["name"].as_str()).collect();
    assert_eq!(names, ["face A0", "face A90", "face A180", "face A270"]);
    let angles: Vec<f64> = ops.iter().filter_map(|o| o["a_deg"].as_f64()).collect();
    assert_eq!(angles, [0.0, 90.0, 180.0, 270.0]);
    assert!(ops.iter().all(|o| o["kind"] == "indexed parallel"), "{r}");
    for a in ["A90", "A180", "A270"] {
        assert!(text.split_whitespace().any(|w| w.trim_end_matches('.') == a), "{a}: {text}");
    }
    // A count spreads the angles evenly over a turn: the same four.
    let count = faces_job(json!([{"type": "indexed_pattern", "count": 4, "operation": face_op("face", 40.0)}]));
    let (_, text_count) = write(&mut s, count, "count.ngc");
    assert_eq!(text_count, text);
    // Both, neither, too many, and no name at all: refused.
    let mut unnamed = face_op("", 40.0);
    unnamed["name"] = json!("");
    for bad in [
        json!({"type": "indexed_pattern", "a_deg": [0], "count": 2, "operation": face_op("f", 40.0)}),
        json!({"type": "indexed_pattern", "operation": face_op("f", 40.0)}),
        json!({"type": "indexed_pattern", "count": 201, "operation": face_op("f", 40.0)}),
        json!({"type": "indexed_pattern", "a_deg": [], "operation": face_op("f", 40.0)}),
        json!({"type": "indexed_pattern", "a_deg": [0], "start_deg": 5, "operation": face_op("f", 40.0)}),
        json!({"type": "indexed_pattern", "count": 4, "operation": unnamed}),
    ] {
        match s.execute("cam.job", &json!({"job": faces_job(json!([bad.clone()])), "dry_run": true})) {
            Err(EngineError::BadParams { .. }) => {}
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn a_clearance_inside_the_part_is_refused() {
    // The square bar reaches 21.21 mm from the axis at its corners: A turning at a clearance
    // of 18 would swing them into the tool.
    let mut s = square_block();
    let job = faces_job(json!([{"type": "indexed_pattern", "count": 4, "operation": face_op("face", 18.0)}]));
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    let msg = e.to_string();
    assert!(matches!(e, EngineError::BadParams { .. }) && msg.contains("face A0") && msg.contains("21.21") && msg.contains("part"), "{msg}");
    // A post that turns A at the machine's top instead is not refused for it.
    let dir = Dir::new("top-turn");
    let job = with_top_turning_post(job, &dir);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["operations"].as_array().map(Vec::len), Some(4), "{r}");
    // A wrapped pocket whose rapids would go round just outside the cylinder it is drawn on, in
    // a part that reaches farther: refused before anything is written.
    let mut slot = slot_job("wrap_pocket");
    slot["operations"][0]["radius"] = json!(8);
    slot["operations"][0]["bottom"] = json!(0);
    slot["operations"][0]["clearance"] = json!(1);
    slot["operations"][0]["retract"] = json!(0.5);
    let e = slotted_bar().execute("cam.job", &json!({"job": slot, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("not outside the part"), "{e}");
    // Roughing from round stock: the clearance must clear the stock as well.
    let mut job = barrel_job();
    job["operations"][0] = json!({
        "name": "rough", "type": "rotary_roughing", "tool": 3, "pattern": {"type": "rings", "stepover": 3},
        "stock_radius": 14, "clearance": 14, "retract": 14, "range": [-15, 15]
    });
    let e = bar().execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("not outside the stock"), "{e}");
}

/// A stepped bar about X: a 30 mm square section from X −20 to 0 (its corners 21.21 mm from
/// the axis) and a 20 mm one from 0 to 20 (14.14 mm).
fn stepped_bar() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 30, "corner": [-20, -15, -15], "body_name": "Big"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [0, -10, -10], "body_name": "Small"}));
    s
}

#[test]
fn a_turn_between_operations_is_checked_where_the_tool_turns() {
    let mut s = stepped_bar();
    // A 3-axis finish of the big section's top (15 mm up), its rapids `clearance` mm up.
    let big = |clearance: f64| {
        json!({
            "name": "big", "type": "parallel", "tool": 1,
            "heights": {"clearance": clearance, "retract": 17, "top": 15, "bottom": 0},
            "stepover": 3, "boundary": {"rect": [[-16, -6], [-4, 6]]}
        })
    };
    // The small section's side at A 90°, its rapids 16 mm up: clear of the small section.
    let small = json!({"type": "indexed", "a_deg": 90, "operation": {
        "name": "small", "type": "parallel", "tool": 1,
        "heights": {"clearance": 16, "retract": 12, "top": 10, "bottom": 0},
        "stepover": 3, "boundary": {"rect": [[5, -5], [15, 5]]}
    }});
    // `big` leaves the tool 18 mm up over the big section, and A turns to 90° there: its corners,
    // 21.21 mm out, swing into the tool. Each operation is clear where it works.
    let e = s.execute("cam.job", &json!({"job": faces_job(json!([big(18.0), small.clone()])), "dry_run": true})).unwrap_err();
    let msg = e.to_string();
    assert!(matches!(e, EngineError::BadParams { .. }), "{msg}");
    assert!(msg.contains("`small`") && msg.contains("`big`") && msg.contains("21.21"), "{msg}");
    // From 25 mm up the turn is clear.
    let r = run(&mut s, "cam.job", json!({"job": faces_job(json!([big(25.0), small.clone()])), "dry_run": true}));
    assert_clean(&r);
    // The other way round A turns over the small section, at 18 mm (the post rises to `big`'s
    // clearance first): clear, and so not refused.
    let r = run(&mut s, "cam.job", json!({"job": faces_job(json!([small.clone(), big(18.0)])), "dry_run": true}));
    assert_clean(&r);
    // A post that goes to the machine's top before turning is never refused for it.
    let dir = Dir::new("top-turn");
    let job = with_top_turning_post(faces_job(json!([big(18.0), small])), &dir);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["operations"].as_array().map(Vec::len), Some(2), "{r}");
}

/// A machine file beside the job is read as the job's machine; `machine` and `machine_path`
/// together, a misspelt key in the file, and a file that is no object are refused.
#[test]
fn a_machine_file_beside_the_job_is_its_machine() {
    let mut s = bar();
    let dir = Dir::new("machine");
    let machine = serde_json::to_value(solidcraft_cam::kinematics::Machine::four_axis_a()).unwrap();
    std::fs::write(dir.0.join("mill.machine.json"), machine.to_string()).unwrap();
    let mut job = barrel_job();
    let o = job.as_object_mut().unwrap();
    o.remove("machine");
    o.insert("machine_path".into(), json!("mill.machine.json"));
    let job_path = dir.file("bar.cam.json");
    std::fs::write(&job_path, job.to_string()).unwrap();
    let r = run(&mut s, "cam.job", json!({"job_path": job_path, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["machine"], machine["name"], "{r}");
    job["machine"] = json!("4axis_a");
    std::fs::write(&job_path, job.to_string()).unwrap();
    let e = s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("not both"), "{e}");
    job.as_object_mut().unwrap().remove("machine");
    std::fs::write(&job_path, job.to_string()).unwrap();
    let mut misspelt = machine.clone();
    misspelt["rotarys"] = json!([]);
    std::fs::write(dir.0.join("mill.machine.json"), misspelt.to_string()).unwrap();
    let e = s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("rotarys"), "{e}");
    std::fs::write(dir.0.join("mill.machine.json"), "[1, 2]").unwrap();
    let e = s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("machine file"), "{e}");
}

#[test]
fn hostile_rotary_jobs_are_refused_without_panics() {
    let mut s = bar();
    let job = |op: Value| json!({"job": {"machine": "4axis_a", "post": "linuxcnc", "tools": [ball(3, 4.0)], "operations": [op]}, "dry_run": true});
    let lines = json!({"type": "lines", "angle_step_deg": 15});
    let cases = [
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": lines, "clearance": 15, "retract": 25}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": {"type": "lines", "angle_step_deg": 0}, "clearance": 25, "retract": 15}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": {"type": "helix", "pitch": -1}, "clearance": 25, "retract": 15}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": {"type": "rings", "stepover": 1e308}, "clearance": 25, "retract": 15}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": {"type": "spiral"}, "clearance": 25, "retract": 15}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": lines, "clearance": 1e308, "retract": 15}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": lines, "clearance": 25, "retract": 15, "range": [5, -5]}),
        json!({"name": "r", "type": "rotary_finish", "tool": 3, "pattern": lines, "clearance": 25, "retract": 15, "max_angle_step_deg": 0}),
        json!({"name": "r", "type": "rotary_roughing", "tool": 3, "pattern": lines, "stock_radius": -1, "clearance": 25, "retract": 15}),
        json!({"name": "r", "type": "rotary_roughing", "tool": 3, "pattern": lines, "stock_radius": 14, "clearance": 25, "retract": 15, "stepdown": 0}),
        json!({"name": "w", "type": "wrap", "tool": 3, "radius": 10}),
        json!({"name": "w", "type": "wrap", "tool": 3, "radius": -10, "points": [[0, 0, 0], [1, 1, 0]]}),
        json!({"name": "w", "type": "wrap", "tool": 3, "radius": 10, "points": [[0, 0, 0], [1, 1, 0]], "curve": "c"}),
        json!({"name": "w", "type": "wrap", "tool": 3, "radius": 10, "sketch": "Nope", "curve": "c"}),
        json!({"name": "p", "type": "wrap_pocket", "tool": 3, "radius": 10, "bottom": -2}),
        json!({"name": "p", "type": "wrap_pocket", "tool": 3, "radius": 10, "polygon": [[0, 0], [1, 0], [0, 1]], "bottom": 5}),
        json!({"name": "p", "type": "wrap_contour", "tool": 3, "radius": 0, "polygon": [[0, 0], [1, 0], [0, 1]], "bottom": -1}),
        json!({"type": "indexed", "a_deg": 90, "operation": {"type": "pocket", "tool": 3, "polygon": [[0, 0], [1, 0], [0, 1]], "heights": {"clearance": 40, "retract": 20, "top": 15, "bottom": 10}}}),
        json!({"name": "i", "type": "indexed", "a_deg": 90, "operation": {"type": "indexed", "a_deg": 0}}),
    ];
    for p in cases.iter().map(|c| job(c.clone())) {
        match s.execute("cam.job", &p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    // A rotary operation on a 3-axis machine.
    let mut j = job(cases[0].clone());
    j["job"]["machine"] = json!("3axis");
    j["job"]["operations"][0]["retract"] = json!(15);
    j["job"]["operations"][0]["clearance"] = json!(25);
    let e = s.execute("cam.job", &j).unwrap_err();
    assert!(e.to_string().contains("rotary axis"), "{e}");
}

/// A pocket on a face the model shows solid cuts into the part: refused and nothing written, a
/// failure in a dry run, and written with `allow_unsafe`, the reply saying where: the operation,
/// the move, the line of the program that ends it and the tool tip.
#[test]
fn a_pocket_into_the_solid_side_is_a_gouge_with_its_line() {
    let mut solid = Session::default();
    run(&mut solid, "solid.box", json!({"length": 40, "width": 30, "height": 30, "corner": [-20, -15, -15]}));
    let dir = Dir::new("gouge");
    let out = dir.file("side.ngc");
    let e = solid.execute("cam.job", &json!({"job": side_pocket_job(), "output": out})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains("gouge"), "{e}");
    assert!(!PathBuf::from(&out).exists(), "nothing was written");
    let r = run(&mut solid, "cam.job", json!({"job": side_pocket_job(), "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|w| w > 4.0), "the pocket is 5 mm into the face: {r}");
    assert_eq!(r["gouge"]["pass"], false, "{r}");
    let r = run(&mut solid, "cam.job", json!({"job": side_pocket_job(), "output": out, "allow_unsafe": true}));
    assert_eq!(r["passed"], false);
    assert!(PathBuf::from(&out).exists());
    let at = &r["gouge"]["at"];
    assert_eq!(at["operation"], "side pocket", "{r}");
    let moves = r["operations"][0]["moves"].as_u64().unwrap();
    assert!(at["move_index"].as_u64().is_some_and(|m| m < moves), "{r}");
    let line = at["line"].as_u64().unwrap_or_else(|| panic!("no line: {r}")) as usize;
    let text = std::fs::read_to_string(&out).unwrap();
    let written = text.lines().nth(line - 1).unwrap();
    assert!(written.starts_with('G') || written.starts_with('X') || written.starts_with('Y') || written.starts_with('Z'), "line {line}: {written}");
    // The tip is in the pocket, at its floor 5 mm into the +Y face.
    let xyz: Vec<f64> = at["xyz"].as_array().unwrap().iter().filter_map(Value::as_f64).collect();
    assert!(xyz.len() == 3 && xyz[0].abs() <= 6.0 + 1e-6 && (xyz[1] - 10.0).abs() <= 1e-6, "{xyz:?}");
    assert_eq!(r["operations"][0]["gouge_at"]["move_index"], at["move_index"], "{r}");
    let failure = r["failures"].as_array().unwrap().iter().filter_map(Value::as_str).find(|f| f.starts_with("gouge")).unwrap();
    assert!(failure.contains("side pocket") && failure.contains(&format!("G-code line {line}")) && failure.contains("work coordinates"), "{failure}");
    assert!(r["gouge"]["allowed"].as_f64().is_some_and(|a| (a - 0.0075).abs() < 1e-12), "{r}");
    // On the bar without the slot a wrapped slot cuts what the model does not show.
    let r = run(&mut bar(), "cam.job", json!({"job": slot_job("wrap_pocket"), "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|w| w > 1.9), "the slot is 2 mm deep: {r}");
}

/// `gouge_check: false` leaves an operation unchecked against the part (an engraving the model
/// does not show), never against a fixture; a fixture is never cut, and `fixtures` and `bodies`
/// may not share a body.
#[test]
fn a_fixture_is_never_cut_even_without_the_gouge_check() {
    let mut s = top_pocket();
    run(&mut s, "solid.box", json!({"length": 6, "width": 30, "height": 8, "corner": [23, -15, -10], "body_name": "Clamp"}));
    // An outline round the block: it runs through the clamp standing beside it.
    let mut job = pocket_job();
    job["fixtures"] = json!(["Clamp"]);
    job["operations"] = json!([{
        "name": "outline", "type": "contour", "tool": 1, "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
        "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -6}, "stepdown": 3
    }]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["failures"].as_array().unwrap().iter().any(|f| f.as_str().is_some_and(|f| f.contains("gouge"))), "{r}");
    // Without the check against the part, still against the fixture.
    job["operations"][0]["gouge_check"] = json!(false);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert_eq!(r["gouge"]["skipped"], json!(["outline"]), "{r}");
    assert!(r["failures"].as_array().unwrap().iter().any(|f| f.as_str().is_some_and(|f| f.contains("into a fixture"))), "{r}");
    assert!(r["operations"][0]["warnings"][0].as_str().is_some_and(|w| w.contains("gouge_check: false")), "{r}");
    // Above the clamp's top (Z -2), the outline passes.
    job["operations"][0]["gouge_check"] = json!(true);
    job["operations"][0]["heights"]["bottom"] = json!(-1);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    for (bodies, fixtures) in [(json!(["Block"]), json!(["Block"])), (json!(["Block"]), json!(["Nope"]))] {
        let mut j = job.clone();
        j["bodies"] = bodies;
        j["fixtures"] = fixtures;
        assert!(matches!(s.execute("cam.job", &json!({"job": j, "dry_run": true})), Err(EngineError::BadParams { .. })));
    }
}

/// A plate with a 6 mm hole through it.
fn plate_with_a_hole() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 15, "corner": [-15, -10, -15], "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 20, "axis": "Z", "base": [0, 0, -17], "operation": "cut"}));
    s
}

/// The points of a circle of radius `r` about the origin, `n` round.
fn circle(r: f64, n: usize) -> Value {
    json!(
        (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64;
                [r * a.cos(), r * a.sin()]
            })
            .collect::<Vec<_>>()
    )
}

/// A 4 mm end mill with a shank of its own diameter, cutting round the inside of a 6 mm hole:
/// its shank above the flutes runs along the wall the cutter has just cut. That is no holder
/// collision, and a shank wider than the cutter is one, counted in moves.
#[test]
fn a_shank_as_wide_as_the_cutter_passes_the_hole_it_cuts() {
    let mut s = plate_with_a_hole();
    let mut tool = flat(4, 4.0);
    tool["flute_length"] = json!(12);
    tool["shank_diameter"] = json!(4);
    tool["stickout"] = json!(20);
    tool["holder"] = json!([{"diameter": 34, "length": 4}, {"diameter": 34, "length": 18}]);
    let mut job = json!({
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [tool],
        "operations": [{
            "name": "hole", "type": "contour", "tool": 4, "polygon": circle(3.0, 256), "side": "inside",
            "heights": {"clearance": 10, "retract": 2, "top": 0, "bottom": -15.3}, "stepdown": 1, "ramp_angle_deg": 2
        }]
    });
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["holder_collisions"], 0, "{r}");
    // Wider than the cutter, the shank is into the wall.
    job["tools"][0]["shank_diameter"] = json!(5);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    let (hits, moves) = (r["operations"][0]["holder_collisions"].as_u64().unwrap(), r["operations"][0]["moves"].as_u64().unwrap());
    assert!(hits > 0 && hits <= moves, "{hits} collisions in {moves} moves");
    assert!(r["failures"].as_array().unwrap().iter().any(|f| f.as_str().is_some_and(|f| f.starts_with("holder"))), "{r}");
    // A tool without holder data is not checked for it: `null`, not 0.
    let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "dry_run": true}));
    assert!(r["holder_collisions"].is_null(), "{r}");
}

fn drill_tool(number: u32, diameter: f64) -> Value {
    json!({
        "number": number, "kind": "drill", "diameter": diameter, "flute_length": 30, "point_angle_deg": 118,
        "spindle_rpm": 6000, "feed": 300, "plunge_feed": 250
    })
}

/// The motion blocks of the program at `path`.
fn blocks(path: &str) -> Vec<JointWords> {
    parse5(&std::fs::read_to_string(path).unwrap(), &[]).unwrap()
}

/// The Z words of a program, in order.
fn z_words(text: &str) -> Vec<f64> {
    text.lines().flat_map(|l| l.split_whitespace().filter_map(|w| w.strip_prefix('Z')?.parse::<f64>().ok())).collect()
}

fn count_z(text: &str, z: f64) -> usize {
    z_words(text).into_iter().filter(|w| (w - z).abs() < 1e-6).count()
}

/// A 60 × 40 × 20 plate, its top at Z 0, with four 6 mm holes `depth` deep at (-15, 0),
/// (15, 0), (0, 10) and (0, -10), and a sketch `Holes` on its top: 6 mm circles at the first
/// three, a 30 mm circle round the middle (no hole), and a point at the fourth.
fn drilled_plate(depth: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 20, "corner": [-30, -20, -20], "body_name": "Plate"}));
    for [x, y] in [[-15, 0], [15, 0], [0, 10], [0, -10]] {
        run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": depth + 1.0, "base": [x, y, -depth], "operation": "cut"}));
    }
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Holes"}));
    for [x, y] in [[-15, 0], [15, 0], [0, 10]] {
        run(&mut s, "sketch.circle.center", json!({"center": [x, y], "diameter": 6}));
    }
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 30}));
    run(&mut s, "sketch.point", json!({"point": [0, -10]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn drill_job(op: Value) -> Value {
    json!({"machine": "3axis", "post": "linuxcnc_4axis", "tools": [drill_tool(3, 6.0)], "operations": [op]})
}

fn peck_op() -> Value {
    json!({
        "name": "holes", "type": "drill", "tool": 3, "sketch": "Holes", "diameter_range": [5, 7],
        "heights": {"clearance": 15, "retract": 2, "top": 0, "bottom": -10}, "cycle": "peck", "peck": 3
    })
}

#[test]
fn drilling_takes_the_holes_from_a_sketch_pecks_exactly_and_reads_back() {
    let mut s = drilled_plate(10.0);
    let dir = Dir::new("drill");
    let out = dir.file("holes.ngc");
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": drill_job(peck_op()), "output": out}))
    });
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "drill", "{r}");
    // Four holes (three circles in the range and the point; not the 30 mm circle), each:
    // over it, down to R, four pecks to -3, -6, -9 and -10 with a full retract and a return to
    // 0.5 mm above the depth after the first three, out to R and up to clearance.
    assert_eq!(op["moves"], 4 * 14, "{r}");
    assert!((op["cut_length"].as_f64().unwrap() - 4.0 * (12.0 + 1.5)).abs() < 1e-9, "{r}");
    let text = std::fs::read_to_string(&out).unwrap();
    for (z, n) in [(-3.0, 4), (-6.0, 4), (-9.0, 4), (-10.0, 4), (-2.5, 4), (-5.5, 4), (-8.5, 4)] {
        assert_eq!(count_z(&text, z), n, "Z{z}: {text}");
    }
    assert!(z_words(&text).iter().all(|z| *z >= -10.0 - 1e-9), "never below the bottom: {text}");
    for canned in ["G73", "G81", "G82", "G83"] {
        assert!(!text.contains(canned), "expanded moves, no {canned}: {text}");
    }
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert_eq!(c["bounds"], json!([[-15.0, -10.0, -10.0], [15.0, 10.0, 15.0]]), "{c}");
    // The drill feeds at its plunge feed.
    assert!(text.lines().any(|l| l.contains("F250")), "{text}");
}

#[test]
fn a_drills_point_is_gouge_checked_and_full_diameter_depth_goes_deeper_by_it() {
    let full = |bottom: f64| {
        let mut op = peck_op();
        op["to_full_diameter"] = json!(true);
        op["heights"]["bottom"] = json!(bottom);
        drill_job(op)
    };
    // The holes are modelled flat-bottomed 10 mm deep: to full diameter at -10 the point is
    // 1.8 mm into their floors.
    let r = run(&mut drilled_plate(10.0), "cam.job", json!({"job": full(-10.0), "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|w| w > 1.5 && w < 2.1), "{r}");
    // Modelled 12 deep, the point (to -11.8) clears the floor.
    let r = run(&mut drilled_plate(12.0), "cam.job", json!({"job": full(-10.0), "dry_run": true}));
    assert_clean(&r);
    // No holes in the model: the drill cuts where the part shows material.
    let mut solid = Session::default();
    run(&mut solid, "solid.box", json!({"length": 60, "width": 40, "height": 20, "corner": [-30, -20, -20]}));
    run(&mut solid, "sketch.create", json!({"plane": "XY", "name": "Holes"}));
    run(&mut solid, "sketch.circle.center", json!({"center": [0, 0], "diameter": 6}));
    run(&mut solid, "sketch.finish", json!({}));
    let r = run(&mut solid, "cam.job", json!({"job": drill_job(peck_op()), "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|w| w > 9.0), "{r}");
    // A flat end mill may plunge the holes; a ball end mill may not drill, and a drill not mill.
    let mut job = drill_job(peck_op());
    job["tools"] = json!([flat(3, 6.0)]);
    assert_clean(&run(&mut drilled_plate(10.0), "cam.job", json!({"job": job, "dry_run": true})));
    job["tools"] = json!([ball(3, 6.0)]);
    let e = drilled_plate(10.0).execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("a drill, a flat end mill to plunge with, or a chamfer mill"), "{e}");
    let job = drill_job(json!({
        "name": "c", "type": "contour", "tool": 3, "polygon": [[-10, -10], [10, -10], [10, 10], [-10, 10]],
        "heights": {"clearance": 15, "retract": 2, "top": 0, "bottom": -1}
    }));
    let e = drilled_plate(10.0).execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("tool 3 is a drill: a contour cuts with"), "{e}");
}

#[test]
fn a_drill_indexed_at_90_degrees_drills_the_blocks_side() {
    // A block 40 × 30 × 30 about the X axis with two 5 mm holes 8 deep into its +Y face, and a
    // sketch on that face (its y running down work Z) with circles at them.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 30, "corner": [-20, -15, -15], "body_name": "Block"}));
    for [x, z] in [[-8, 0], [8, 4]] {
        run(&mut s, "solid.cylinder", json!({"diameter": 5, "height": 9, "base": [x, 7, z], "axis": "Y", "operation": "cut"}));
    }
    let side = json!({"plane": {"origin": [0, 15, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}, "name": "Side holes"});
    run(&mut s, "sketch.create", side.clone());
    for [x, y] in [[-8, 0], [8, -4]] {
        run(&mut s, "sketch.circle.center", json!({"center": [x, y], "diameter": 5}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let job = |sketch: &str| {
        json!({
            "machine": "4axis_a", "post": "linuxcnc_4axis", "tools": [drill_tool(4, 5.0)],
            "operations": [{"type": "indexed", "a_deg": 90, "operation": {
                "name": "side holes", "type": "drill", "tool": 4, "sketch": sketch,
                "heights": {"clearance": 40, "retract": 18, "top": 15, "bottom": 7},
                "cycle": "chip_break", "peck": 2.5, "back_off": 0.3
            }}]
        })
    };
    let r = run(&mut s, "cam.job", json!({"job": job("Side holes"), "dry_run": true}));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "indexed drill", "{r}");
    assert_eq!(op["a_deg"].as_f64(), Some(90.0), "{r}");
    assert!((op["max_tilt_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6, "the drill comes in along work +Y: {r}");
    // Two holes, each: over it, to R, four pecks (to 12.5, 10, 7.5 and 7) with three
    // back-offs, out to R, up to clearance.
    assert_eq!(op["moves"], 2 * 11, "{r}");
    // The holes are where the sketch says only if the frame maps them right: drilled into the
    // solid side, the same job gouges.
    let mut solid = Session::default();
    run(&mut solid, "solid.box", json!({"length": 40, "width": 30, "height": 30, "corner": [-20, -15, -15]}));
    run(&mut solid, "sketch.create", side);
    run(&mut solid, "sketch.circle.center", json!({"center": [-8, 0], "diameter": 5}));
    run(&mut solid, "sketch.finish", json!({}));
    let r = run(&mut solid, "cam.job", json!({"job": job("Side holes"), "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    // A sketch on the top is not square to the indexed tool axis.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Top"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 5}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("cam.job", &json!({"job": job("Top"), "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("not on a plane square"), "{e}");
}

#[test]
fn drilling_refuses_an_r_height_below_the_part_round_a_hole() {
    // A plate, its top at Z 0, with a 20 × 20 boss 10 high round (15, 0), and 6 mm holes 12 deep
    // into the plate at (-15, 0) and through the boss into the plate at (15, 0).
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 20, "corner": [-30, -20, -20], "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [5, -10, 0], "operation": "join"}));
    for [x, y] in [[-15, 0], [15, 0]] {
        run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 25, "base": [x, y, -12], "operation": "cut"}));
    }
    let job = |points: Value, top: f64, retract: f64| {
        let op = json!({
            "name": "holes", "type": "drill", "tool": 3, "points": points,
            "heights": {"clearance": 25, "retract": retract, "top": top, "bottom": -10}
        });
        json!({"job": drill_job(op), "dry_run": true})
    };
    // Both holes with the plate's heights: the drill would rapid from clearance down to R 2
    // through 8 mm of the boss. The gouge check cannot see it (the finished part has the hole).
    let e = s.execute("cam.job", &job(json!([[-15, 0], [15, 0]]), 0.0, 2.0)).unwrap_err();
    assert!(e.to_string().contains("(15, 0)") && e.to_string().contains("retract"), "{e}");
    // The plate's hole alone, and the boss's from the boss's top, are clean.
    assert_clean(&run(&mut s, "cam.job", job(json!([[-15, 0]]), 0.0, 2.0)));
    assert_clean(&run(&mut s, "cam.job", job(json!([[15, 0]]), 10.0, 12.0)));
}

#[test]
fn hostile_drilling_jobs_are_refused_without_panics() {
    let mut s = drilled_plate(10.0);
    let with = |f: &dyn Fn(&mut Value)| {
        let mut op = peck_op();
        f(&mut op);
        json!({"job": drill_job(op), "dry_run": true})
    };
    let points_op = |extra: Value| {
        let mut op = json!({"name": "p", "type": "drill", "tool": 3, "heights": {"clearance": 15, "retract": 2, "top": 0, "bottom": -10}});
        if let (Some(o), Value::Object(e)) = (op.as_object_mut(), extra) {
            o.extend(e);
        }
        json!({"job": drill_job(op), "dry_run": true})
    };
    let cases = [
        (with(&|o| o["sketch"] = json!("Nope")), "Nope"),
        (with(&|o| o["points"] = json!([[0, 0]])), "not both"),
        (points_op(json!({})), "give a `sketch`"),
        (points_op(json!({"points": []})), "no holes"),
        (points_op(json!({"points": [[1e308, 0]]})), "finite"),
        (points_op(json!({"points": [[0, 0]], "circles": true})), "go with `sketch`"),
        (with(&|o| o["diameter_range"] = json!([7, 5])), "smallest first"),
        (with(&|o| o["diameter_range"] = json!([-1, 5])), "smallest first"),
        (with(&|o| o["diameter_range"] = json!([0, 1e308])), "finite"),
        (
            with(&|o| {
                o["diameter_range"] = json!([50, 60]);
                o["sketch_points"] = json!(false);
            }),
            "no circles in `diameter_range`",
        ),
        (
            with(&|o| {
                o["circles"] = json!(false);
                o["sketch_points"] = json!(false);
            }),
            "no holes",
        ),
        (with(&|o| o["cycle"] = json!("simple")), "`peck` goes with"),
        (with(&|o| o["dwell_seconds"] = json!(1)), "`dwell_seconds` goes with"),
        (with(&|o| o["back_off"] = json!(1)), "`back_off` goes with"),
        (with(&|o| o["peck"] = json!(1e-9)), "pecks"),
        (with(&|o| o["peck"] = json!(-1)), "peck"),
        (with(&|o| o["peck_clearance"] = json!(0)), "clearance"),
        (with(&|o| o["cycle"] = json!("pecking")), "pecking"),
        (with(&|o| o["pek"] = json!(1)), "pek"),
        (with(&|o| o["feature"] = json!("hole:0")), "feature"),
        (with(&|o| o["heights"]["bottom"] = json!(-40)), "flutes"),
        (with(&|o| o["heights"]["bottom"] = json!(5)), "ordered"),
        (with(&|o| o["heights"]["bottom"] = json!(0)), "no depth"),
        (with(&|o| o["feeds"] = json!({"cut": 100, "plunge": -1})), "feed"),
    ];
    for (p, word) in &cases {
        match s.execute("cam.job", p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(e) => assert!(e.to_string().contains(word), "{p}: {e}"),
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    let mut job = drill_job(peck_op());
    job["tools"][0]["point_angle_deg"] = json!(0);
    assert!(s.execute("cam.job", &json!({"job": job, "dry_run": true})).is_err());
}

/// A plate 40 × 30 × 10, its top at Z 0, with a ⌀10 hole 6 deep at (-10, 0) and a ⌀5 hole
/// through at (10, 0) (an M6 tap drill), and a sketch `Holes` of both circles on its top.
fn holed_plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [-20, -15, -10], "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 7, "base": [-10, 0, -6], "operation": "cut", "targets": ["Plate"]}));
    run(&mut s, "solid.cylinder", json!({"radius": 2.5, "height": 12, "base": [10, 0, -11], "operation": "cut", "targets": ["Plate"]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Holes"}));
    run(&mut s, "sketch.circle.center", json!({"center": [-10, 0], "radius": 5}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 0], "radius": 2.5}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn thread_mill_tool(number: u32, diameter: f64) -> Value {
    json!({"number": number, "kind": "thread_mill", "diameter": diameter, "flute_length": 12, "spindle_rpm": 12000, "feed": 300, "plunge_feed": 100})
}

fn bore_job(op: Value) -> Value {
    let mut op = op;
    for (k, v) in [("name", json!("bore")), ("type", json!("bore")), ("tool", json!(1))] {
        op[k] = v;
    }
    op["heights"] = json!({"clearance": 20, "retract": 5, "top": 0, "bottom": -6});
    json!({"machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)], "operations": [op]})
}

fn m6_job() -> Value {
    json!({
        "machine": "4axis_a",
        "post": "linuxcnc_4axis",
        "tools": [thread_mill_tool(2, 4.5)],
        "operations": [{
            "name": "M6 thread", "type": "thread_mill", "tool": 2, "centers": [[10, 0]],
            "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -8},
            "pitch": 1.0, "thread_diameter": 6.0
        }]
    })
}

/// The Z of every helical arc of `text` (a G2/G3 block with Z), with its motion code: the
/// motion is modal, so a block without G2/G3 continues the last one.
fn helix_z(text: &str) -> Vec<(f64, u8)> {
    let mut out = Vec::new();
    let mut motion = 0u8;
    for line in text.lines().filter(|l| !l.starts_with('(')) {
        let words: Vec<&str> = line.split_whitespace().collect();
        for w in &words {
            match *w {
                "G0" | "G00" => motion = 0,
                "G1" | "G01" => motion = 1,
                "G2" | "G02" => motion = 2,
                "G3" | "G03" => motion = 3,
                _ => {}
            }
        }
        let z = words.iter().find_map(|w| w.strip_prefix('Z')).and_then(|v| v.parse::<f64>().ok());
        if let (Some(z), true, true) = (z, words.iter().any(|w| w.starts_with('I')), motion >= 2) {
            out.push((z, motion));
        }
    }
    out
}

#[test]
fn a_bore_reads_back_with_its_helical_arcs_and_clears_the_hole_it_is_checked_against() {
    let mut s = holed_plate();
    let dir = Dir::new("bore");
    let out = dir.file("bore.ngc");
    let job = bore_job(json!({"holes": [{"center": [-10, 0], "diameter": 10}], "pitch": 0.5}));
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out}));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "bore", "{r}");
    assert!(op["warnings"].as_array().is_some_and(Vec::is_empty), "{r}");
    let text = std::fs::read_to_string(&out).unwrap();
    // Down a quarter of a millimetre each half turn, exactly, counter-clockwise (climb), to the floor.
    let z = helix_z(&text);
    assert_eq!(z.len(), 24, "{text}");
    assert!(z.iter().all(|(_, g)| *g == 3), "{z:?}");
    for (k, (zk, _)) in z.iter().enumerate() {
        assert!((zk - (-0.25 * (k + 1) as f64)).abs() < 1e-9, "{z:?}");
    }
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty) && c["arcs"].as_u64().is_some_and(|n| n >= 28), "{c}");
    assert!((c["bounds"][0][2].as_f64().unwrap() + 6.0).abs() < 1e-9, "{c}");
    // A bore wider than the hole in the model cuts into the part: the gouge check says so.
    let wide = bore_job(json!({"holes": [{"center": [-10, 0], "diameter": 12}], "pitch": 0.5}));
    let r = run(&mut s, "cam.job", json!({"job": wide, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|w| w > 0.5), "{r}");
    // The circles of a sketch in a diameter range: the ⌀5 one is smaller than the ⌀6 tool, refused
    // with the hole named; out of range, no circles.
    let job = bore_job(json!({"sketch": "Holes", "diameter_range": [8, 12], "ramp_angle_deg": 2, "finish_passes": 1}));
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert!(r["operations"][0]["moves"].as_u64().is_some_and(|m| m > 30), "{r}");
    let e = s.execute("cam.job", &json!({"job": bore_job(json!({"sketch": "Holes", "diameter_range": [20, 30]})), "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("no circles"), "{e}");
    let e = s.execute("cam.job", &json!({"job": bore_job(json!({"sketch": "Holes"})), "dry_run": true})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains("hole 2 at (10, 0)"), "{e}");
}

#[test]
fn an_m6x1_internal_thread_is_written_one_pitch_a_turn_and_passes_the_checks() {
    let mut s = holed_plate();
    let dir = Dir::new("thread");
    let out = dir.file("m6.ngc");
    let r = run(&mut s, "cam.job", json!({"job": m6_job(), "output": out}));
    assert_clean(&r);
    assert_eq!(r["operations"][0]["kind"], "thread_mill", "{r}");
    let text = std::fs::read_to_string(&out).unwrap();
    let z = helix_z(&text);
    // Up from -8 to 0, counter-clockwise: the lead-in and lead-out each rise a quarter pitch
    // (a quarter turn about the axis) and every half turn of the helix exactly half a pitch.
    assert_eq!(z.len(), 17, "{text}");
    assert!(z.iter().all(|(_, g)| *g == 3), "{z:?}");
    let mut last = -8.0;
    for (k, (zk, _)) in z.iter().enumerate() {
        let rise = if k == 0 || k == z.len() - 1 { 0.25 } else { 0.5 };
        assert!((zk - last - rise).abs() < 1e-9, "{z:?}");
        last = *zk;
    }
    assert!((last - 0.0).abs() < 1e-9, "{z:?}");
    // The program names the thread mill itself; only the checks take its core.
    assert!(text.contains("T2 D4.5 thread mill"), "{text}");
    // A thread mill too big for the minor diameter, or an end mill, is refused.
    let mut big = m6_job();
    big["tools"][0]["diameter"] = json!(5.2);
    let e = s.execute("cam.job", &json!({"job": big, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("minor diameter"), "{e}");
    let mut end_mill = m6_job();
    end_mill["tools"] = json!([flat(2, 4.0)]);
    let e = s.execute("cam.job", &json!({"job": end_mill, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("thread mill"), "{e}");
    // A thread mill in a pocket is refused: only its core would be checked.
    let mut pocket = m6_job();
    pocket["operations"][0] = json!({"name": "p", "type": "pocket", "tool": 2, "polygon": [[0, -5], [5, -5], [5, 5], [0, 5]], "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -1}});
    assert!(s.execute("cam.job", &json!({"job": pocket, "dry_run": true})).unwrap_err().to_string().contains("thread mill"));
}

#[test]
fn hostile_bores_and_threads_are_refused_without_panics() {
    let mut s = holed_plate();
    let bore = |f: &dyn Fn(&mut Value)| {
        let mut j = bore_job(json!({"holes": [{"center": [-10, 0], "diameter": 10}], "pitch": 0.5}));
        f(&mut j["operations"][0]);
        json!({"job": j, "dry_run": true})
    };
    let thread = |f: &dyn Fn(&mut Value)| {
        let mut j = m6_job();
        f(&mut j);
        json!({"job": j, "dry_run": true})
    };
    let unset = |o: &mut Value, k: &str| {
        if let Some(m) = o.as_object_mut() {
            m.remove(k);
        }
    };
    let cases = vec![
        bore(&|o| o["holes"] = json!([])),
        bore(&|o| o["holes"] = json!([{"center": [1e308, 0], "diameter": 10}])),
        bore(&|o| o["holes"] = json!([{"center": [0, 0], "diameter": -10}])),
        bore(&|o| o["holes"] = json!([{"center": [0, 0], "diameter": 1e308}])),
        bore(&|o| o["holes"] = json!([{"center": [0, 0]}])),
        bore(&|o| o["holes"] = json!([{"center": [0, 0], "diameter": 10, "depth": 3}])),
        bore(&|o| o["pich"] = json!(1)),
        bore(&|o| o["pitch"] = json!(-1)),
        bore(&|o| o["pitch"] = json!(0)),
        bore(&|o| o["pitch"] = json!(1e308)),
        bore(&|o| o["pitch"] = json!(1e-9)),
        bore(&|o| o["ramp_angle_deg"] = json!(2)),
        bore(&|o| o["stepdown"] = json!(1)),
        bore(&|o| o["sketch"] = json!("Holes")),
        bore(&|o| o["diameter_range"] = json!([1, 2])),
        bore(&|o| {
            unset(o, "pitch");
            o["ramp_angle_deg"] = json!(90);
        }),
        bore(&|o| {
            unset(o, "holes");
            o["sketch"] = json!("Holes");
            o["diameter_range"] = json!([12, 8]);
        }),
        bore(&|o| {
            unset(o, "holes");
            o["sketch"] = json!("Nope");
        }),
        bore(&|o| o["stepover"] = json!(0)),
        bore(&|o| o["stepover"] = json!(100)),
        bore(&|o| o["stock_to_leave"] = json!(-1)),
        bore(&|o| o["stock_to_leave"] = json!(4)),
        bore(&|o| o["finish_passes"] = json!(1000)),
        bore(&|o| o["finish_passes"] = json!(-1)),
        bore(&|o| o["heights"]["bottom"] = json!(-1e9)),
        bore(&|o| o["heights"]["bottom"] = json!(0)),
        bore(&|o| o["tool"] = json!(9)),
        bore(&|o| o["feature"] = json!("hole:0")),
        thread(&|j| j["operations"][0]["pitch"] = json!(0)),
        thread(&|j| j["operations"][0]["pitch"] = json!(1e308)),
        thread(&|j| j["operations"][0]["thread_diameter"] = json!(-6)),
        thread(&|j| j["operations"][0]["thread_diameter"] = json!(1e308)),
        thread(&|j| j["operations"][0]["thread_depth"] = json!(0)),
        thread(&|j| j["operations"][0]["thread_depth"] = json!(5)),
        thread(&|j| j["operations"][0]["radial_passes"] = json!(0)),
        thread(&|j| j["operations"][0]["radial_passes"] = json!(1000)),
        thread(&|j| j["operations"][0]["centers"] = json!([])),
        thread(&|j| j["operations"][0]["centers"] = json!([[1e308, 0]])),
        thread(&|j| j["operations"][0]["center"] = json!([0, 0])),
        thread(&|j| j["operations"][0]["internal"] = json!("yes")),
    ];
    for p in &cases {
        match s.execute("cam.job", p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
}

fn plate(chamfered: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 10, "corner": [-15, -10, -10], "body_name": "Plate"}));
    if chamfered {
        run(&mut s, "solid.chamfer", json!({"edges": [[0, -10, 0], [15, 0, 0], [0, 10, 0], [-15, 0, 0]], "distance": 1, "tangent_chain": false}));
    }
    s
}

fn chamfer_mill(number: u32) -> Value {
    json!({"number": number, "kind": "chamfer", "diameter": 6, "tip_diameter": 0, "angle_deg": 90, "flute_length": 10, "flutes": 3,
           "spindle_rpm": 20000, "feed": 1600, "plunge_feed": 100})
}

fn face_mill(number: u32) -> Value {
    json!({"number": number, "kind": "face_mill", "diameter": 10, "corner_radius": 0.2, "flute_length": 6, "flutes": 1,
           "spindle_rpm": 18000, "feed": 1600, "plunge_feed": 50})
}

/// Faces the plate's top with a face mill, then chamfers its top edges `width` wide.
fn chamfer_job(width: f64) -> Value {
    json!({
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [face_mill(1), chamfer_mill(7)],
        "operations": [
            {"name": "face", "type": "pocket", "tool": 1, "polygon": [[-14, -9], [14, -9], [14, 9], [-14, 9]],
             "heights": {"clearance": 10, "retract": 2, "top": 0.5, "bottom": 0}, "stepover": 7, "ramp_angle_deg": 2},
            {"name": "deburr", "type": "chamfer", "tool": 7, "polygon": [[-15, -10], [15, -10], [15, 10], [-15, 10]],
             "heights": {"clearance": 10, "retract": 2, "top": 0}, "width": width}
        ]
    })
}

#[test]
fn a_chamfer_mill_cuts_the_modelled_chamfer_as_wide_as_asked() {
    let mut s = plate(true);
    let dir = Dir::new("chamfer");
    let out = dir.file("plate.ngc");
    let r = run(&mut s, "cam.job", json!({"job": chamfer_job(1.0), "output": out}));
    assert_clean(&r);
    assert_eq!(r["operations"][1]["kind"], "chamfer", "{r}");
    // The width as the program cuts it: along the +X side, the 90° cone's radius at the top,
    // 1.2 above its tip, less the axis's distance from the wall.
    let cuts: Vec<Vec3> = blocks(&out)
        .iter()
        .filter_map(|b| match b {
            JointWords::Feed { linear, .. } => Some(*linear),
            _ => None,
        })
        .collect();
    let bottom = cuts.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    assert!((bottom + 1.2).abs() < 1e-6, "{bottom}");
    let side: Vec<f64> =
        cuts.iter().filter(|p| (p.z - bottom).abs() < 1e-6 && p.x > 7.5 && p.y.abs() <= 10.0 + 1e-6).map(|p| 1.2 - (p.x - 15.0)).collect();
    assert!(!side.is_empty(), "{cuts:?}");
    for w in side {
        assert!((w - 1.0).abs() <= 1e-3, "the chamfer is {w} mm wide, not 1");
    }
    // A chamfer wider than modelled is the operation's own band: allowed.
    assert_clean(&run(&mut s, "cam.job", json!({"job": chamfer_job(1.2), "dry_run": true})));
}

#[test]
fn a_chamfer_the_model_does_not_show_is_allowed_and_a_cut_past_it_is_not() {
    // A deburr on a sharp edge: the cone cuts the edge as deep as the chamfer band it is told
    // to cut, which the gouge check allows.
    let mut s = plate(false);
    let r = run(&mut s, "cam.job", json!({"job": chamfer_job(1.0), "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["operations"][1]["gouge"].as_f64(), Some(0.0), "{r}");
    assert_eq!(r["gouge"]["skipped"], json!([]), "the chamfer is checked, not skipped: {r}");
    // A profile 2 mm inside the edge cuts the top face beyond the band: a gouge.
    let mut job = chamfer_job(1.0);
    job["operations"][1]["polygon"] = json!([[-13, -8], [13, -8], [13, 8], [-13, 8]]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["operations"][1]["gouge"].as_f64().is_some_and(|g| g > 0.1), "{r}");
    // `gouge_check: false` still skips it.
    let mut job = chamfer_job(1.0);
    job["operations"][1]["gouge_check"] = json!(false);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["gouge"]["skipped"], json!(["deburr"]), "{r}");
}

#[test]
fn a_chamfer_mill_engraves_a_line_and_spots_holes() {
    // V-engraving: a contour `side: on` with a chamfer mill traces the line to its depth, the
    // groove allowed as its own; spot drilling with it cuts the cone a drill starts in.
    let mut s = plate(false);
    let mut job = chamfer_job(1.0);
    job["operations"] = json!([
        {"name": "engrave", "type": "contour", "tool": 7, "side": "on", "polygon": [[-10, -5], [10, -5], [10, 5], [-10, 5]],
         "heights": {"clearance": 10, "retract": 2, "top": 0, "bottom": -0.3}},
        {"name": "spot", "type": "drill", "tool": 7, "points": [[-8, 0], [8, 0]],
         "heights": {"clearance": 10, "retract": 2, "top": 0, "bottom": -1}}
    ]);
    let dir = Dir::new("engrave");
    let out = dir.file("e.ngc");
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out}));
    assert_clean(&r);
    let cuts: Vec<Vec3> = blocks(&out)
        .iter()
        .filter_map(|b| match b {
            JointWords::Feed { linear, .. } => Some(*linear),
            _ => None,
        })
        .collect();
    assert!(cuts.iter().any(|p| (p.z + 0.3).abs() < 1e-6 && (p.x - 10.0).abs() < 1e-6), "the trace runs on the line at its depth");
    assert!(cuts.iter().any(|p| (p.z + 1.0).abs() < 1e-6 && (p.x - 8.0).abs() < 1e-6), "the spot goes to its depth");
    // Deeper than the band it is told to cut, by a trace below the face it names: a gouge.
    let mut deep = job.clone();
    deep["operations"][0]["heights"] = json!({"clearance": 10, "retract": 2, "top": -1, "bottom": -1.3});
    let r = run(&mut s, "cam.job", json!({"job": deep, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    // Beside the line a chamfer mill is refused.
    let mut beside = job.clone();
    beside["operations"][0]["side"] = json!("outside");
    let e = s.execute("cam.job", &json!({"job": beside, "dry_run": true})).unwrap_err().to_string();
    assert!(e.contains("tapered"), "{e}");
}

#[test]
fn a_chamfer_band_is_the_parts_alone_never_a_fixtures() {
    // A vise jaw along the plate's +X side, its top 0.5 mm below the plate's: the deburr's point
    // runs 1.2 mm down just outside the edge, into the jaw. The band the chamfer mill is told to
    // cut is cut into the part, so the jaw is held against the whole tool.
    let jawed = |top: f64| {
        let mut s = plate(false);
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 30, "height": 20.0 + top, "corner": [15.1, -15, -20], "operation": "new", "body_name": "Jaw"}),
        );
        s
    };
    let mut s = jawed(-0.5);
    let mut job = chamfer_job(1.0);
    job["bodies"] = json!(["Plate"]);
    job["fixtures"] = json!(["Jaw"]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["operations"][1]["gouge"].as_f64().is_some_and(|g| g > 0.3), "{r}");
    // Clear of the jaw (its top 1.5 mm down), the deburr passes.
    let mut s = jawed(-1.5);
    assert_clean(&run(&mut s, "cam.job", json!({"job": job, "dry_run": true})));
}

#[test]
fn hostile_chamfers_and_chamfer_mills_are_refused() {
    let mut s = plate(true);
    let refused = |s: &mut Session, job: Value, needle: &str| {
        let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(needle), "{needle}: {e}");
    };
    // Wider than the cone reaches: it ends 3 out, the axis runs 0.2 out with the default tip offset.
    refused(&mut s, chamfer_job(2.9), "widest chamfer is 2.8");
    let mut job = chamfer_job(1.0);
    job["operations"][1]["side"] = json!("on");
    refused(&mut s, job, "not `on`");
    let mut job = chamfer_job(1.0);
    job["operations"][1]["tool"] = json!(1);
    refused(&mut s, job, "a chamfer cuts with a chamfer mill");
    let mut job = chamfer_job(1.0);
    job["operations"][1]["depth"] = json!(1);
    refused(&mut s, job, "depth");
    let mut job = chamfer_job(1.0);
    job["operations"][1]["heights"]["bottom"] = json!(-1);
    refused(&mut s, job, "bottom");
    let mut job = chamfer_job(1.0);
    job["operations"][0]["tool"] = json!(7);
    refused(&mut s, job, "a pocket cuts with");
    for (k, v) in [("angle_deg", json!(180)), ("angle_deg", json!(null)), ("tip_diameter", json!(6)), ("tip_diameter", json!(-1))] {
        let mut job = chamfer_job(1.0);
        job["tools"][1][k] = v;
        refused(&mut s, job, "chamfer mill");
    }
    for (w, t) in [(json!(-1), json!(0.2)), (json!(0), json!(0.2)), (json!(1e300), json!(0.2)), (json!(1), json!(-0.5)), (json!(1), json!(1e300))] {
        let mut job = chamfer_job(1.0);
        job["operations"][1]["width"] = w;
        job["operations"][1]["tip_offset"] = t;
        assert!(s.execute("cam.job", &json!({"job": job, "dry_run": true})).is_err());
    }
    // Indexed, the chamfer runs in the frame the A angle brings under the tool.
    let mut job = chamfer_job(1.0);
    job["machine"] = json!("4axis_a");
    let mut inner = job["operations"][1].take();
    // Clear of the plate's corners as A turns (14.1 mm from the axis): the 4th-axis setup check.
    inner["heights"]["clearance"] = json!(20);
    job["operations"] = json!([{"type": "indexed", "a_deg": 0, "operation": inner}]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["operations"][0]["kind"], "indexed chamfer", "{r}");
}

fn leads_plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 6, "corner": [-20, -15, -6], "body_name": "Plate"}));
    s
}

fn profile_job(extra: Value) -> Value {
    let mut op = json!({
        "name": "profile", "type": "contour", "tool": 1,
        "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
        "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -6}, "stepdown": 2
    });
    for (k, v) in extra.as_object().unwrap() {
        op[k] = v.clone();
    }
    json!({"machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)], "operations": [op]})
}

#[test]
fn a_contour_with_leads_tabs_and_finishing_passes_reads_back_and_misses_the_part() {
    let mut s = leads_plate();
    let dir = Dir::new("profile");
    let out = dir.file("profile.ngc");
    let job = profile_job(json!({
        "lead": {"radius": 2, "line": 1},
        "entry": {"type": "ramp", "angle_deg": 3},
        "tabs": {"count": 4, "width": 4, "height": 1.5},
        "finish_passes": 1, "finish_stepover": 0.25, "spring_pass": true,
        "linking": "stay_down"
    }));
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": job, "output": out}))
    });
    assert_clean(&r);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.lines().any(|l| l.contains("G2") || l.contains("G3")), "the leads are written as arcs");
    let pts = tips(&out);
    // The tabs: the tool climbs to 1.5 above the bottom four times on each pass at the bottom
    // (the last depth pass, the finishing pass and the spring pass).
    let mut climbs = 0;
    for w in pts.windows(2) {
        if !w[1].1 && w[0].0.z < -4.5 - 1e-3 && (w[1].0.z + 4.5).abs() < 1e-3 {
            climbs += 1;
        }
    }
    assert_eq!(climbs, 3 * 4, "{text}");
    // Never inside the tool's radius of the plate below its top; the passes between levels and
    // the finishing passes go across at the retract height, above the part.
    let outside = |p: Vec3| (p.x.clamp(-20.0, 20.0) - p.x).hypot(p.y.clamp(-15.0, 15.0) - p.y);
    assert!(pts.iter().filter(|(p, _)| p.z < 0.0).all(|(p, _)| outside(*p) >= 3.0 - 2e-3), "{text}");
    let hops = pts.windows(2).filter(|w| w[1].1 && (w[1].0.x - w[0].0.x).hypot(w[1].0.y - w[0].0.y) > 1e-6 && (w[1].0.z - 5.0).abs() < 1e-6).count();
    assert!(hops >= 3, "{text}");
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert!(c["errors"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert!(c["arcs"].as_u64().is_some_and(|n| n >= 8), "{c}");
}

#[test]
fn a_pocket_with_a_helix_entry_never_plunges_into_uncut_material() {
    let mut s = top_pocket();
    let dir = Dir::new("helix");
    let out = dir.file("pocket.ngc");
    let mut job = pocket_job();
    job["operations"][0]["entry"] = json!({"type": "helix"});
    let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
        run(&mut s, "cam.job", json!({"job": job, "output": out}))
    });
    assert_clean(&r);
    let pts = tips(&out);
    // Helical arcs: cuts that go down while going round.
    let mut last: Option<Vec3> = None;
    let mut helical = 0;
    for b in &blocks(&out) {
        if let (JointWords::Arc { linear, .. }, Some(p)) = (b, last)
            && linear.z < p.z - 1e-6
        {
            helical += 1;
        }
        last = match b {
            JointWords::Rapid { linear, .. }
            | JointWords::Feed { linear, .. }
            | JointWords::Arc { linear, .. }
            | JointWords::PlaneArc { linear, .. } => Some(*linear),
            JointWords::Dwell { .. } => last,
        };
    }
    assert!(helical >= 4, "{helical}");
    // Every straight-down cut below the top ends no lower than what was already cut there.
    let mut deepest: Vec<(f64, f64, f64)> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        let here = |x: f64, y: f64| deepest.iter().filter(|d| (d.0 - x).hypot(d.1 - y) < 1e-3).map(|d| d.2).fold(f64::INFINITY, f64::min);
        if !w[1].1 && (b.x - a.x).hypot(b.y - a.y) < 1e-9 && b.z < a.z && b.z < -1e-6 {
            assert!(b.z >= here(b.x, b.y) - 1e-3, "a plunge into uncut material at {b:?}");
        }
        if !w[1].1 {
            deepest.push((b.x, b.y, b.z));
        }
    }
}

/// A block with a U-shaped pocket 4 mm deep (two arms joined at the bottom) and, between the
/// arms, a pillar standing 10 mm above the top when `pillar`, half a millimetre off the pocket's wall.
fn u_pocket(pillar: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 32, "height": 10, "corner": [-20, -18, -10], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 24, "height": 4, "corner": [-15, -14, -4], "operation": "cut"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 24, "height": 4, "corner": [5, -14, -4], "operation": "cut"}));
    run(&mut s, "solid.box", json!({"length": 30, "width": 6, "height": 4, "corner": [-15, -14, -4], "operation": "cut"}));
    if pillar {
        run(&mut s, "solid.box", json!({"length": 6, "width": 15.5, "height": 10, "corner": [-3, -7.5, 0], "operation": "join"}));
    }
    s
}

#[test]
fn stay_down_links_go_over_the_part_at_the_retract_height_only_where_it_is_clear() {
    let job = json!({
        "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 4.0)],
        "operations": [{"name": "u", "type": "pocket", "tool": 1,
                        "polygon": [[-15, -14], [15, -14], [15, 10], [5, 10], [5, -8], [-5, -8], [-5, 10], [-15, 10]],
                        "heights": {"clearance": 20, "retract": 3, "top": 0, "bottom": -4}, "stepdown": 2, "stepover": 1.5,
                        "linking": "stay_down"}]
    });
    let dir = Dir::new("u");
    let sideways = |pts: &[(Vec3, bool)]| -> Vec<(Vec3, Vec3)> {
        pts.windows(2).filter(|w| w[1].1 && (w[1].0.x - w[0].0.x).hypot(w[1].0.y - w[0].0.y) > 1e-6).map(|w| (w[0].0, w[1].0)).collect()
    };
    for pillar in [false, true] {
        let mut s = u_pocket(pillar);
        let out = dir.file(if pillar { "pillar.ngc" } else { "open.ngc" });
        let r = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), || {
            run(&mut s, "cam.job", json!({"job": job, "output": out}))
        });
        assert_clean(&r);
        let moves = sideways(&tips(&out));
        // From one arm to the other at the retract height when nothing stands in between.
        let low = moves.iter().filter(|(_, b)| (b.z - 3.0).abs() < 1e-6).count();
        // Over the pillar (with the tool's radius round it) only above its top.
        let over = |(a, b): &(Vec3, Vec3)| {
            (0..=100).any(|i| {
                let t = i as f64 / 100.0;
                let (x, y) = (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                (x.clamp(-3.0, 3.0) - x).hypot(y.clamp(-7.5, 8.0) - y) < 2.0
            })
        };
        if pillar {
            assert!(moves.iter().filter(|m| over(m)).all(|(_, b)| b.z >= 10.0), "{moves:?}");
            assert!(moves.iter().any(over), "the arms are linked over the pillar: {moves:?}");
        } else {
            assert!(low >= 2, "{moves:?}");
        }
    }
}

#[test]
fn hostile_leads_tabs_and_entries_are_refused_without_panics() {
    let mut s = leads_plate();
    let pocket = |extra: Value| {
        let mut op = json!({"name": "p", "type": "pocket", "tool": 1, "polygon": [[-8, -6], [8, -6], [8, 6], [-8, 6]],
                            "heights": {"clearance": 20, "retract": 3, "top": 0, "bottom": -4}});
        for (k, v) in extra.as_object().unwrap() {
            op[k] = v.clone();
        }
        json!({"job": {"machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)], "operations": [op]}, "dry_run": true})
    };
    let contour = |extra: Value| json!({"job": profile_job(extra), "dry_run": true});
    let cases = [
        (contour(json!({"lead": {"radius": 2, "radus": 1}})), "radus"),
        (contour(json!({"entry": {"type": "spiral"}})), "spiral"),
        (contour(json!({"linking": "hover"})), "hover"),
        (contour(json!({"tabs": {"count": 4, "spacing": 10, "width": 4, "height": 1}})), "not both"),
        (contour(json!({"tabs": {"width": 4, "height": 1}})), "tabs.count"),
        (contour(json!({"tabs": {"count": 4, "width": 4, "height": 1}, "side": "inside"})), "outside"),
        (contour(json!({"tabs": {"count": 4, "width": 4, "height": 6}})), "tab height"),
        (contour(json!({"tabs": {"count": 100, "width": 4, "height": 1}})), "no room"),
        (contour(json!({"entry": {"type": "plunge"}, "ramp_angle_deg": 3})), "not both"),
        (contour(json!({"entry": {"type": "helix", "radius": 10}})), "core"),
        (contour(json!({"finish_passes": 2, "finish_stepover": 2})), "finishing stepover"),
        (contour(json!({"finish_passes": 1000})), "finishing passes"),
        (contour(json!({"lead": {"radius": 2, "sweep_deg": 0}})), "sweep"),
        (contour(json!({"lead": {"radius": 1e308}})), "lead radius"),
        // Inside a 40 × 30 opening a lead of radius 14 reaches the far wall.
        (contour(json!({"lead": {"radius": 14, "sweep_deg": 180}, "side": "inside"})), "lead would cut into the part"),
        (pocket(json!({"entry": {"type": "helix", "angle_deg": 90}})), "helix angle"),
        (pocket(json!({"entry": {"type": "ramp", "angle_deg": -1}})), "ramp angle"),
        (pocket(json!({"lead": {"radius": 2}})), "lead"),
        (pocket(json!({"tabs": {"count": 2, "width": 1, "height": 1}})), "tabs"),
    ];
    for (p, word) in cases {
        match s.execute("cam.job", &p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(e) => assert!(e.to_string().contains(word), "{p}: {e}"),
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    // Values at the limits plan or are refused, never panic.
    for v in [json!(0), json!(-1), json!(1e-300), json!(1e308), json!(-1e308), json!(4294967295u64)] {
        for extra in [
            json!({"lead": {"radius": v, "line": v}}),
            json!({"entry": {"type": "helix", "radius": v, "angle_deg": v}}),
            json!({"tabs": {"spacing": v, "width": v, "height": v, "ramp_angle_deg": v}}),
            json!({"tabs": {"count": v, "width": 4, "height": 1}}),
            json!({"finish_passes": v, "finish_stepover": v, "spring_pass": true}),
        ] {
            if let Err(EngineError::Internal(_, msg)) = s.execute("cam.job", &contour(extra.clone())) {
                panic!("{extra}: panicked: {msg}");
            }
        }
        if let Err(EngineError::Internal(_, msg)) = s.execute("cam.job", &pocket(json!({"entry": {"type": "helix", "radius": v}}))) {
            panic!("{v}: panicked: {msg}");
        }
    }
}

/// A 40 mm block, its top at Z 0, with a 24 mm square pocket 3 mm deep.
fn pocket_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, -10], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 24, "width": 24, "height": 3, "corner": [-12, -12, -3], "operation": "cut"}));
    s
}

fn adaptive_job(op: Value) -> Value {
    json!({"name": "adaptive", "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)], "operations": [op]})
}

fn adaptive_pocket_op() -> Value {
    json!({"name": "clear", "type": "adaptive", "tool": 1, "polygon": [[-12, -12], [12, -12], [12, 12], [-12, 12]],
           "heights": {"clearance": 20, "retract": 2, "top": 0, "bottom": -3}, "optimal_load": 0.6, "max_engagement_deg": 60})
}

/// The adaptive pocket is written, read back and gouge checked, never slots outside its helix,
/// and is reported beside a plain offset pocket that does.
#[test]
fn an_adaptive_pocket_reads_back_and_reports_its_engagement() {
    let mut s = pocket_block();
    let r = run(&mut s, "cam.job", json!({"job": adaptive_job(adaptive_pocket_op()), "dry_run": true}));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "adaptive", "{op}");
    let a = &op["adaptive"];
    assert!(a["max_engagement_deg"].as_f64().is_some_and(|m| m > 20.0 && m <= 63.0), "{a}");
    assert_eq!(a["full_slot_share"].as_f64(), Some(0.0), "{a}");
    assert_eq!(a["limit_deg"].as_f64().map(f64::round), Some(60.0), "{a}");
    assert!(a["helices"].as_u64().is_some_and(|h| h >= 1), "{a}");
    let p = &a["pocket"];
    assert!(p["max_engagement_deg"].as_f64().is_some_and(|m| m >= 170.0), "{a}");
    assert!(p["full_slot_share"].as_f64().is_some_and(|f| f > 0.0), "{a}");
    assert!(r["program_lines"].as_u64().is_some_and(|n| n > 100), "{r}");
}

/// Without engagement keys the limit follows the load (10% of the diameter here: 36.9° on a
/// straight edge, the limit half as much again); `compare: false` plans no pocket.
#[test]
fn an_adaptive_pocket_takes_its_limits_from_the_load() {
    let mut s = pocket_block();
    let mut op = adaptive_pocket_op();
    op.as_object_mut().unwrap().remove("max_engagement_deg");
    op["compare"] = json!(false);
    op["stepdown"] = json!(1.5);
    let r = run(&mut s, "cam.job", json!({"job": adaptive_job(op), "dry_run": true}));
    assert_clean(&r);
    let a = &r["operations"][0]["adaptive"];
    assert!(a["limit_deg"].as_f64().is_some_and(|l| (l - 55.3).abs() < 0.1), "{a}");
    assert!(a.get("pocket").is_none(), "{a}");
    assert_eq!(a["levels"].as_u64(), Some(2), "{a}");
    // A load as wide as the tool still keeps the least engagement below the most given.
    assert!(super::adaptive::limits(3.0, 6.0, None, Some(55.0)).0 < 55.0);
    let capped = json!({"name": "capped", "type": "adaptive", "tool": 1, "polygon": [[-7, -7], [7, -7], [7, 7], [-7, 7]],
                        "heights": {"clearance": 20, "retract": 2, "top": -2, "bottom": -3}, "optimal_load": 6,
                        "max_engagement_deg": 55, "compare": false});
    let r = run(&mut s, "cam.job", json!({"job": adaptive_job(capped), "dry_run": true}));
    assert_clean(&r);
    let a = &r["operations"][0]["adaptive"];
    assert_eq!(a["measured"], true, "{a}");
    assert!(a["max_engagement_deg"].as_f64().is_some_and(|m| m <= 58.0), "{a}");
}

/// 3D adaptive roughing of the dome over its block: entered from open air, read back and
/// gouge checked, within the engagement limit; without a boundary it takes the part's extent as
/// the stock and says so.
#[test]
fn adaptive_roughing_of_the_dome_reads_back_and_passes_the_gouge_check() {
    let mut s = dome();
    let op = json!({"name": "rough", "type": "adaptive_roughing", "tool": 1,
                    "heights": {"clearance": 30, "retract": 10, "top": 7, "bottom": 0.25}, "stepdown": 2.5,
                    "optimal_load": 0.6, "max_engagement_deg": 60, "boundary": {"rect": [[-20, -20], [20, 20]]}});
    let r = run(&mut s, "cam.job", json!({"job": adaptive_job(op), "dry_run": true}));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "adaptive_roughing", "{op}");
    let a = &op["adaptive"];
    assert!(a["max_engagement_deg"].as_f64().is_some_and(|m| m > 10.0 && m <= 63.0), "{a}");
    assert_eq!(a["full_slot_share"].as_f64(), Some(0.0), "{a}");
    assert!(a["levels"].as_u64().is_some_and(|l| l >= 3), "{a}");
    assert_eq!(a["helices"].as_u64(), Some(0), "{a}");
    let base = json!({"name": "rough", "type": "adaptive_roughing", "tool": 1,
                      "heights": {"clearance": 30, "retract": 10, "top": 7, "bottom": 3}, "stepdown": 4,
                      "optimal_load": 0.6, "max_engagement_deg": 60});
    let r = run(&mut s, "cam.job", json!({"job": adaptive_job(base.clone()), "dry_run": true}));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert!(op["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("the part's extent"))), "{op}");
    // A polygon larger than the rectangle: the stock is where both are, and so is the material
    // the engagement is measured in.
    let mut op = base;
    op["boundary"] = json!({"rect": [[-20, -20], [20, 20]], "polygon": [[-40, -40], [40, -40], [40, 40], [-40, 40]]});
    let r = run(&mut s, "cam.job", json!({"job": adaptive_job(op), "dry_run": true}));
    assert_clean(&r);
    let a = &r["operations"][0]["adaptive"];
    assert_eq!(a["measured"], true, "{a}");
    assert!(a["max_engagement_deg"].as_f64().is_some_and(|m| m <= 63.0), "{a}");
    assert_eq!(a["full_slot_share"].as_f64(), Some(0.0), "{a}");
}

/// Adaptive clearing of the side pocket at A = 90°: written with A held, read back and gouge
/// checked in the turned frame, within the engagement limit.
#[test]
fn an_adaptive_pocket_indexed_at_90_degrees_reads_back() {
    let mut s = pocketed_block();
    let job = json!({
        "machine": "4axis_a", "post": "linuxcnc_4axis", "tools": [flat(1, 4.0)],
        "operations": [{"type": "indexed", "a_deg": 90, "operation": {
            "name": "side adaptive", "type": "adaptive", "tool": 1, "polygon": [[-6, -6], [6, -6], [6, 6], [-6, 6]],
            "heights": {"clearance": 40, "retract": 20, "top": 15, "bottom": 10}, "stepdown": 2.5,
            "optimal_load": 0.4, "max_engagement_deg": 60, "compare": false}}]
    });
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["kind"], "indexed adaptive", "{r}");
    assert!((op["max_tilt_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6, "{r}");
    let a = &op["adaptive"];
    assert_eq!(a["measured"], true, "{a}");
    assert_eq!(a["levels"].as_u64(), Some(2), "{a}");
    assert!(a["max_engagement_deg"].as_f64().is_some_and(|m| m > 10.0 && m <= 63.0), "{a}");
    assert_eq!(a["full_slot_share"].as_f64(), Some(0.0), "{a}");
    let head: Vec<&str> = r["program_head"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(head.iter().any(|l| l.contains("A90")), "{head:?}");
}

/// A measured path above the limit, or in a full slot outside its entries, is warned about; an
/// unmeasured one says nothing it does not know.
#[test]
fn adaptive_engagement_above_the_limit_is_warned_about() {
    use crate::schema::cam_adaptive::AdaptiveReport;
    let ok = AdaptiveReport { measured: true, max_engagement_deg: 61.0, limit_deg: 60.0, ..AdaptiveReport::default() };
    assert!(super::adaptive::engagement_warnings(&ok).is_empty());
    let over = AdaptiveReport { max_engagement_deg: 75.0, ..ok.clone() };
    assert!(super::adaptive::engagement_warnings(&over).iter().any(|w| w.contains("above the limit")));
    let slot = AdaptiveReport { full_slot_share: 0.02, ..ok.clone() };
    assert!(super::adaptive::engagement_warnings(&slot).iter().any(|w| w.contains("full slot")));
    let unmeasured = AdaptiveReport { measured: false, ..over };
    assert!(super::adaptive::engagement_warnings(&unmeasured).is_empty());
}

/// Hostile adaptive operations are refused with an error, never a panic or a hang.
#[test]
fn hostile_adaptive_operations_are_refused() {
    let mut s = pocket_block();
    let with = |key: &str, v: Value| {
        let mut op = adaptive_pocket_op();
        op[key] = v;
        json!({"job": adaptive_job(op), "dry_run": true})
    };
    let rough = |extra: Value| {
        let mut op =
            json!({"name": "r", "type": "adaptive_roughing", "tool": 1, "heights": {"clearance": 30, "retract": 10, "top": 0, "bottom": -1}});
        if let (Some(o), Some(e)) = (op.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        json!({"job": adaptive_job(op), "dry_run": true})
    };
    let cases = [
        with("optimal_load", json!(-1)),
        with("optimal_load", json!(7)),
        with("max_engagement_deg", json!(1)),
        with("max_engagement_deg", json!(500)),
        with("min_engagement_deg", json!(90)),
        with("helix_diameter", json!(20)),
        with("ramp_angle_deg", json!(0)),
        with("stepdown", json!(0)),
        with("tolerance", json!(0)),
        with("optimal_lode", json!(1)),
        with("rest_from", json!("previous")),
        with("polygon", json!([[-1e5, -1e5], [1e5, -1e5], [1e5, 1e5], [-1e5, 1e5]])),
        with("polygon", json!([[0, 0], [1, 0]])),
        rough(json!({"heights": {"clearance": 30, "retract": 0.1, "top": 0, "bottom": -1}})),
        rough(json!({"boundary": {"rect": [[-1e5, -1e5], [1e5, 1e5]]}})),
        rough(json!({"flats": "yes"})),
    ];
    let start = std::time::Instant::now();
    for p in &cases {
        match s.execute("cam.job", p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
    // A helix with no room is refused, saying so.
    let e = s.execute("cam.job", &with("polygon", json!([[-12, -3.5], [12, -3.5], [12, 3.5], [-12, 3.5]]))).unwrap_err();
    assert!(e.to_string().contains("no room for the entry helix"), "{e}");
}

/// A 50 mm square block, its top at Z -1, under 1 mm of stock: faced with a 32 mm end mill down
/// to the block's top. Every descent is beside the stock with the whole cutter clear.
#[test]
fn facing_a_box_stock_enters_beside_it_and_reads_back() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-25, -25, -11], "body_name": "Block"}));
    let mill = json!({"number": 5, "kind": "flat_endmill", "diameter": 32, "flute_length": 6, "spindle_rpm": 9000, "feed": 1500, "plunge_feed": 400});
    let job = |stock: Value| {
        json!({
            "machine": "3axis", "post": "linuxcnc_4axis", "tools": [mill.clone()], "stock": stock,
            "operations": [{"name": "face", "type": "face", "tool": 5, "bottom": -1, "stepdown": 0.5}]
        })
    };
    let dir = Dir::new("face");
    let out = dir.file("face.ngc");
    let r = run(&mut s, "cam.job", json!({"job": job(json!({"box": {"min": [-25, -25, -11], "max": [25, 25, 0]}})), "output": out}));
    assert_clean(&r);
    assert_eq!(r["rapids"]["pass"], true, "{r}");
    assert_eq!(r["operations"][0]["kind"], "face", "{r}");
    assert_eq!(r["stock"]["max"], json!([25.0, 25.0, 0.0]), "{r}");
    let mut last: Option<Vec3> = None;
    let (mut descents, mut at_bottom) = (0, 0);
    for b in &blocks(&out) {
        let (p, cut) = match b {
            JointWords::Feed { linear, .. } => (*linear, true),
            JointWords::Rapid { linear, .. } => (*linear, false),
            _ => continue,
        };
        let gap = ((p.x.abs() - 25.0).max(0.0)).hypot((p.y.abs() - 25.0).max(0.0));
        if last.is_some_and(|q| p.z < q.z - 1e-9) || (!cut && p.z < 0.0) {
            descents += 1;
            assert!(gap >= 16.0, "a descent onto the stock at {p:?}");
        }
        if cut && (p.z + 1.0).abs() < 1e-9 {
            at_bottom += 1;
        }
        last = Some(p);
    }
    assert!(descents >= 2 && at_bottom > 4, "{descents} {at_bottom}");
    // The same stock as a margin round the part, read on its exact box.
    let r = run(&mut s, "cam.job", json!({"job": job(json!({"around_part": {"margin": [0, 0, 1]}})), "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["stock"]["min"], json!([-25.0, -25.0, -12.0]), "{r}");
    assert_eq!(r["stock"]["max"], json!([25.0, 25.0, 0.0]), "{r}");
    // A face below the block's top cuts into it.
    let mut deep = job(json!({"box": {"min": [-25, -25, -11], "max": [25, 25, 0]}}));
    deep["operations"][0]["bottom"] = json!(-2);
    let r = run(&mut s, "cam.job", json!({"job": deep, "dry_run": true}));
    assert_eq!(r["gouge"]["pass"], false, "{r}");
}

/// A `rect` inside the stock along the passes would take the face mill down into the stock, and
/// a bottom below the stock's would cut what holds it: both refused, naming the operation. A
/// strip reaching past the stock along the passes is faced.
#[test]
fn facing_down_into_the_stock_or_below_it_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-25, -25, -11], "body_name": "Block"}));
    let job = |op: Value| {
        let mut face = json!({"name": "face", "type": "face", "tool": 1, "bottom": -1, "stepdown": 0.5});
        if let (Some(o), Value::Object(e)) = (face.as_object_mut(), op) {
            o.extend(e);
        }
        json!({"job": {
            "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 10.0)],
            "stock": {"box": {"min": [-25, -25, -11], "max": [25, 25, 0]}}, "operations": [face]
        }, "dry_run": true})
    };
    let r = run(&mut s, "cam.job", job(json!({})));
    assert_clean(&r);
    for bad in
        [json!({"rect": [[-15, -25], [15, 25]]}), json!({"rect": [[-25, -5], [25, 5]], "angle_deg": 90}), json!({"bottom": -12, "stepdown": 3})]
    {
        let e = s.execute("cam.job", &job(bad.clone())).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains("face"), "{bad}: {e}");
        assert!(e.to_string().contains("into the stock") || e.to_string().contains("below the stock's bottom"), "{bad}: {e}");
    }
    let r = run(&mut s, "cam.job", job(json!({"rect": [[-25, -5], [25, 5]]})));
    assert_clean(&r);
}

/// Faced all over from Z 0 down to the block's top at -1, the stock is lower for the pocket
/// after it: the pocket's clearance and retract heights, -0.4 and -0.5, below the stock's first
/// top, are above what is left. Without the face its rapids across at the clearance height run
/// through the stock.
#[test]
fn rapids_after_a_face_are_held_against_the_faced_stock() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 10, "corner": [-25, -25, -11], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 1, "corner": [-5, -5, -2], "operation": "cut"}));
    let face = json!({"name": "face", "type": "face", "tool": 1, "bottom": -1, "stepdown": 0.5});
    let pocket = json!({"name": "pocket", "type": "pocket", "tool": 2, "polygon": [[-5, -5], [5, -5], [5, 5], [-5, 5]],
                        "heights": {"clearance": -0.4, "retract": -0.5, "top": -1, "bottom": -2}, "stepover": 2});
    let job = |ops: Value| {
        json!({"job": {
            "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 10.0), flat(2, 4.0)],
            "stock": {"box": {"min": [-25, -25, -11], "max": [25, 25, 0]}}, "operations": ops
        }, "dry_run": true})
    };
    let r = run(&mut s, "cam.job", job(json!([face, pocket])));
    assert_clean(&r);
    assert_eq!(r["rapids"]["pass"], true, "{r}");
    let r = run(&mut s, "cam.job", job(json!([pocket])));
    assert_eq!(r["rapids"]["pass"], false, "{r}");
    assert!(r["rapids"]["first"].as_array().unwrap().iter().all(|h| h["kind"] == "stock" && h["operation"] == "pocket"), "{r}");
}

#[test]
fn rapids_through_the_stock_and_beyond_the_travel_are_refused() {
    let mut s = dome();
    let mut job = dome_job();
    job["operations"][0]["zigzag"] = json!(false);
    job["stock"] = json!({"box": {"min": [-20, -20, -20], "max": [20, 20, 18]}});
    let e = s.execute("cam.job", &json!({"job": job, "output": Dir::new("stock").file("d.ngc")})).unwrap_err();
    assert!(e.to_string().contains("through the stock"), "{e}");
    job["stock"] = json!({"box": {"min": [-20, -20, -20], "max": [20, 20, 8]}});
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    // A planned move beyond the travel is refused as the program is written.
    let at = Vec3::new;
    let mut m = solidcraft_cam::kinematics::Machine::three_axis();
    m.linear_limits = Some((at(-200.0, -200.0, -100.0), at(200.0, 200.0, 20.0)));
    let mut job = dome_job();
    job["machine"] = serde_json::to_value(&m).unwrap();
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("beyond its travel"), "{e}");
    m.linear_limits = Some((at(-200.0, -200.0, -100.0), at(200.0, 200.0, 50.0)));
    job["machine"] = serde_json::to_value(&m).unwrap();
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["travel"]["pass"], true, "{r}");
    // Without `linear_limits` the travel is not checked.
    assert!(run(&mut s, "cam.job", json!({"job": dome_job(), "dry_run": true}))["travel"].is_null());
}

/// Round stock of radius 30 reaches past a clearance of 25 round a bar: the rapids are in it,
/// the turn too. Rotary roughing takes its radius from the job's round stock, and its
/// clearance must clear that stock.
#[test]
fn a_round_stock_holds_the_rapids_and_gives_the_roughing_its_radius() {
    let mut s = square_block();
    let mut job = faces_job(json!([{"type": "indexed_pattern", "count": 4, "operation": face_op("face", 25.0)}]));
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    job["stock"] = json!({"cylinder": {"radius": 30, "x_min": -25, "x_max": 25}});
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["stock"]["kind"], "cylinder", "{r}");
    assert_eq!(r["rapids"]["pass"], false, "{r}");
    assert!(r["failures"].as_array().unwrap().iter().any(|f| f.as_str().is_some_and(|f| f.contains("through the stock"))), "{r}");
    // Without `stock_radius` the job's round stock gives it: refused when the clearance is in it ...
    let mut job = barrel_job();
    job["stock"] = json!({"cylinder": {"radius": 14, "x_min": -20, "x_max": 20}});
    job["operations"][0] = json!({
        "name": "rough", "type": "rotary_roughing", "tool": 3, "pattern": {"type": "rings", "stepover": 3},
        "clearance": 14, "retract": 14, "range": [-15, 15]
    });
    let e = bar().execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("not outside the stock"), "{e}");
    // ... and planned from it when the clearance is outside it.
    job["operations"][0]["clearance"] = json!(17);
    job["operations"][0]["retract"] = json!(15);
    let r = run(&mut bar(), "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    // Neither the radius nor a stock: refused, saying what to give.
    job.as_object_mut().unwrap().remove("stock");
    let e = bar().execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("stock_radius"), "{e}");
}

/// A cut-out through a box stock goes below its bottom: refused with the `breakthrough` that
/// lets it, which then passes; a smaller one does not, and nonsense is refused.
#[test]
fn a_through_cut_may_go_as_far_below_the_stock_as_the_breakthrough_allows() {
    let mut s = dome();
    let mut job = dome_job();
    job["stock"] = json!({"box": {"min": [-27, -27, -20], "max": [27, 27, 8]}});
    job["tools"] = json!([flat(1, 6.0)]);
    job["operations"] = json!([{
        "name": "cut out", "type": "contour", "tool": 1, "side": "outside", "polygon": [[-20, -20], [20, -20], [20, 20], [-20, 20]],
        "heights": {"clearance": 30, "retract": 15, "top": 0, "bottom": -21}, "stepdown": 3
    }]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    let f = r["failures"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).find(|f| f.starts_with("below the stock")).unwrap();
    assert!(f.contains("`breakthrough: 1`"), "{f}");
    job["breakthrough"] = json!(1.5);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert!(r["notes"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.contains("breakthrough"))), "{r}");
    job["breakthrough"] = json!(0.5);
    assert_eq!(run(&mut s, "cam.job", json!({"job": job, "dry_run": true}))["passed"], false);
    for bad in [json!(-1), json!(26), json!("deep")] {
        job["breakthrough"] = bad;
        assert!(s.execute("cam.job", &json!({"job": job, "dry_run": true})).is_err());
    }
}

/// A 2D adaptive clearing takes the job's stock box: round a boss on a plate drawn to the
/// stock's edge it goes down in the open air beside the stock and cuts in from there, with no
/// helix; with `from_stock: false` the profiles are full of material and entered with a helix.
#[test]
fn a_2d_adaptive_clearing_takes_the_jobs_stock() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 50, "height": 5, "corner": [-30, -25, -10], "body_name": "Part"}));
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 5, "corner": [-15, -10, -5], "operation": "join"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Clear"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-30, -25], "p1": [30, 25]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-15, -10], "p1": [15, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    let job = |from: bool| {
        json!({"job": {"name": "ring", "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)],
                       "stock": {"box": {"min": [-30, -25, -10], "max": [30, 25, 0]}},
                       "operations": [{"name": "clear", "type": "adaptive", "tool": 1, "sketch": "Clear", "profiles": [0],
                                       "heights": {"clearance": 10, "retract": 2, "top": 0, "bottom": -5}, "stepdown": 5,
                                       "optimal_load": 0.6, "compare": false, "from_stock": from}]},
               "dry_run": true})
    };
    let r = run(&mut s, "cam.job", job(true));
    assert_clean(&r);
    let op = &r["operations"][0];
    assert_eq!(op["adaptive"]["helices"], 0, "{op}");
    assert_eq!(op["adaptive"]["full_slot_share"].as_f64(), Some(0.0), "{op}");
    let ratio = op["cut_length"].as_f64().unwrap() / (2400.0 / 0.6);
    assert!(ratio <= 1.5, "{ratio}: {op}");
    let r = run(&mut s, "cam.job", job(false));
    assert_clean(&r);
    assert!(r["operations"][0]["adaptive"]["helices"].as_u64().is_some_and(|h| h >= 1), "{r}");
}

/// A job's `orientation` turns the part for its setup: a pocket in the block's bottom is cut
/// from above once the block is flipped, however the flip is given, and gouges unturned.
#[test]
fn a_job_orientation_turns_the_part_for_its_setup() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [-20, -15, -10], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 3, "corner": [-5, -5, -10], "operation": "cut"}));
    let job = |orientation: Option<Value>| {
        let mut j = json!({"name": "op2", "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 4.0)], "origin": [0, 0, -10],
                           "stock": {"box": {"min": [-21, -16, -10], "max": [21, 16, 0]}},
                           "operations": [{"name": "back pocket", "type": "pocket", "tool": 1, "polygon": [[-5, -5], [5, -5], [5, 5], [-5, 5]],
                                           "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -3}}]});
        if let Some(o) = orientation {
            j["orientation"] = o;
        }
        json!({"job": j, "dry_run": true})
    };
    for o in [json!("flip_x"), json!("flip_y"), json!({"axis": [1, 0, 0], "angle_deg": 180})] {
        let r = run(&mut s, "cam.job", job(Some(o.clone())));
        assert_clean(&r);
        assert!(r["notes"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.contains("orientation"))), "{o}: {r}");
    }
    let r = run(&mut s, "cam.job", job(None));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|g| g > 1.0), "{r}");
    // A sketch on the block's bottom, its normal down: work Z along it is the turned block's up.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, -10], "x_dir": [1, 0, 0], "y_dir": [0, -1, 0]}, "name": "Bottom"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 2}));
    run(&mut s, "sketch.finish", json!({}));
    let r = run(&mut s, "cam.job", job(Some(json!({"sketch": "Bottom"}))));
    assert_clean(&r);
    for bad in
        [json!("flip_z"), json!({"axis": [0, 0, 0], "angle_deg": 90}), json!({"axis": [1, 0, 0], "angle_deg": 1e308}), json!({"sketch": "Nope"})]
    {
        assert!(s.execute("cam.job", &job(Some(bad.clone()))).is_err(), "{bad}");
    }
    // A 4th axis turns the part with its own angles.
    let mut j = job(Some(json!("flip_x")));
    j["job"]["machine"] = json!("4axis_a");
    let e = s.execute("cam.job", &j).unwrap_err();
    assert!(e.to_string().contains("3-axis machine"), "{e}");
}

#[test]
fn hostile_stock_and_face_jobs_are_refused_without_panics() {
    let mut s = dome();
    let job = |f: &dyn Fn(&mut Value)| {
        let mut j = dome_job();
        f(&mut j);
        json!({"job": j, "dry_run": true})
    };
    // A face with a flat tool (number 7) and `extra` keys.
    let face = |extra: Value| {
        let mut j = dome_job();
        j["tools"] = json!([ball(2, 6.0), flat(7, 10.0)]);
        let mut op = json!({"name": "f", "type": "face", "tool": 7, "top": 7, "bottom": 6});
        if let (Some(o), Value::Object(e)) = (op.as_object_mut(), extra) {
            o.extend(e);
        }
        j["operations"] = json!([op]);
        json!({"job": j, "dry_run": true})
    };
    let cases = vec![
        job(&|j| j["stock"] = json!({"box": {"min": [0, 0, 0], "max": [0, 1, 1]}})),
        job(&|j| j["stock"] = json!({"box": {"min": [0, 0, 0], "max": [1e308, 1, 1]}})),
        job(&|j| j["stock"] = json!({"box": {"min": [0, 0], "max": [1, 1, 1]}})),
        job(&|j| j["stock"] = json!({"box": {"min": [0, 0, 0], "max": [1, 1, 1], "maxx": 1}})),
        job(&|j| j["stock"] = json!({"cylinder": {"radius": -1, "x_min": 0, "x_max": 1}})),
        job(&|j| j["stock"] = json!({"cylinder": {"radius": 5, "x_min": 1, "x_max": 1}})),
        job(&|j| j["stock"] = json!({"around_part": {"margin": [-1, 0, 0]}})),
        job(&|j| j["stock"] = json!({"around_part": {"margin": [1e308, 0, 0]}})),
        job(&|j| j["stock"] = json!({"around_part": {"margin": [0, 0, 0], "top": -1}})),
        job(&|j| j["stock"] = json!({"sphere": {"radius": 5}})),
        job(&|j| j["stock"] = json!({"simulated": {"path": "x.json"}})),
        job(&|j| j["stock"] = json!({"box": {"min": [0, 0, 0], "max": [1, 1, 1]}, "cylinder": {"radius": 1, "x_min": 0, "x_max": 1}})),
        job(&|j| j["stock"] = json!("box")),
        // A face without a stock or a top, with a ball, with hostile numbers or keys.
        job(&|j| j["operations"] = json!([{"name": "f", "type": "face", "tool": 2, "bottom": 6}])),
        job(&|j| j["operations"] = json!([{"name": "f", "type": "face", "tool": 2, "top": 7, "bottom": 6}])),
        face(json!({"top": null})),
        face(json!({"stepover": 1e308})),
        face(json!({"stepover": 1e-12})),
        face(json!({"stepdown": 1e-300})),
        face(json!({"bottom": 1e308})),
        face(json!({"bottom": 8})),
        face(json!({"rect": [[5, 5], [-5, -5]]})),
        face(json!({"rect": [[-1e300, 5], [5, 5]]})),
        face(json!({"overhang": -1})),
        face(json!({"angle_deg": 1e308})),
        face(json!({"retract": 1})),
        face(json!({"clearance": 1e308})),
        face(json!({"stepdwn": 1})),
        job(&|j| {
            j["operations"] = json!([{"type": "indexed", "a_deg": 0, "operation": {"name": "f", "type": "face", "tool": 2, "top": 7, "bottom": 6}}])
        }),
    ];
    for p in &cases {
        match s.execute("cam.job", p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    // The face that the hostile ones start from is fine (it faces above the dome's top).
    let r = run(&mut s, "cam.job", face(json!({"top": 9, "bottom": 7.5})));
    assert_eq!(r["passed"], true, "{r}");
}

const VIEW_MODEL: &str = r#"{"format": "solidcraft.machine_model", "version": 1, "name": "test mill",
    "rates": {"rapid_xyz_mm_min": 6000, "accel_mm_s2": 1500, "tool_change_seconds": 20},
    "spindle": {"nose_gap": 5, "nose_max_above_table": 400},
    "table": {"size": [300, 200, 15], "cut_allowance": 0.1},
    "parts": [{"name": "the Z plate", "code": "z-plate", "mount": "spindle", "shape": {"box": {"min": [-40, 30, -1], "max": [40, 45, 200]}}}]}"#;

const VIEW_VISE: &str = r#"{"format": "solidcraft.workholding", "version": 1, "holding": "vise",
    "vise": {"model": {"length": 175, "width": 64, "height": 64, "floor": 32, "fixed_jaw": 35.6, "moving_jaw": 50, "max_opening": 85},
             "center": [150, 100]},
    "parallels": {"height": 10, "thickness": 3, "length": 150}}"#;

/// A folder with the test mill's machine model (its spindle nose reaching `nose_max` above the
/// table) and a vise on parallels: the folder, the model's path and the vise's.
fn vise_dir(tag: &str, nose_max: f64) -> (Dir, String, String) {
    let dir = Dir::new(tag);
    let (model, vise) = (dir.file("m.machine-model.json"), dir.file("v.workholding.json"));
    std::fs::write(&model, VIEW_MODEL.replace("\"nose_max_above_table\": 400", &format!("\"nose_max_above_table\": {nose_max}"))).unwrap();
    std::fs::write(&vise, VIEW_VISE).unwrap();
    (dir, model, vise)
}

/// A cut-out round the dome's block whose cutter runs 1 to 7 mm outside the block in Y, into
/// the vise's jaws below their tops (work Z 2).
fn cut_out_job(model: &str, vise: &str) -> Value {
    json!({
        "name": "cut out", "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)],
        "stock": {"box": {"min": [-20, -20, -20], "max": [20, 20, 8]}},
        "machine_model_path": model, "workholding_path": vise,
        "operations": [{
            "name": "outline", "type": "contour", "tool": 1, "side": "outside", "polygon": [[-21, -21], [21, -21], [21, 21], [-21, 21]],
            "heights": {"clearance": 30, "retract": 15, "top": 0, "bottom": -5}, "stepdown": 5
        }]
    })
}

/// A cut-out running into the vise's fixed jaw fails the job with the operation, the line, the
/// place and a fix; without the workholding the same job passes; `allow_unsafe` writes it and
/// its first line says so.
#[test]
fn a_cut_into_the_vise_fails_the_job() {
    let mut s = dome();
    let (dir, model, vise) = vise_dir("hold-vise", 400.0);
    let job = cut_out_job(&model, &vise);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    let w = &r["workholding"];
    assert_eq!((w["holding"].as_str(), w["pass"].as_bool(), w["machine_model"].as_str()), (Some("vise"), Some(false), Some("test mill")), "{w}");
    let hit =
        w["hits"].as_array().unwrap().iter().find(|h| h["code"].as_str().is_some_and(|c| c.ends_with("-hits-vise"))).unwrap_or_else(|| panic!("{w}"));
    assert_eq!((hit["operation"].as_str(), hit["error"].as_bool()), (Some("outline"), Some(true)), "{hit}");
    assert!(hit["line"].as_u64().unwrap() > 5 && hit["moves"].as_u64().unwrap() >= 1, "{hit}");
    let at = hit["at"].as_array().unwrap();
    assert!(at[1].as_f64().unwrap().abs() > 20.0 && at[2].as_f64().unwrap() < 2.0, "in the jaw: {hit}");
    assert!(hit["fix"].as_str().unwrap().contains("parallels"), "{hit}");
    let f = r["failures"].as_array().unwrap().iter().filter_map(Value::as_str).find(|f| f.starts_with("workholding: operation `outline`"));
    assert!(f.is_some_and(|f| f.contains(&format!("G-code line {}", hit["line"]))), "{r}");
    // Refused when it is to be written.
    let out = dir.file("cut.ngc");
    let e = s.execute("cam.job", &json!({"job": job, "output": out})).unwrap_err().to_string();
    assert!(e.contains("workholding") && !std::path::Path::new(&out).exists(), "{e}");
    // Without the workholding the same job passes.
    let mut free = job.clone();
    let o = free.as_object_mut().unwrap();
    o.remove("workholding_path");
    o.remove("machine_model_path");
    let r = run(&mut s, "cam.job", json!({"job": free, "dry_run": true}));
    assert_eq!(r["passed"], true, "{r}");
    assert!(r.get("workholding").is_none(), "{r}");
    // allow_unsafe writes it, and says so in its first line, every line where the failures say.
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out, "allow_unsafe": true}));
    assert_eq!(r["passed"], false);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.lines().next().unwrap().contains("WRITTEN WITH FAILED CHECKS"), "{}", &text[..200.min(text.len())]);
    assert_eq!(text.lines().count(), r["program_lines"].as_u64().unwrap() as usize);
    // The same workholding written into the job reports the same.
    let mut inline = job.clone();
    inline.as_object_mut().unwrap().remove("workholding_path");
    inline["workholding"] = serde_json::from_str(VIEW_VISE).unwrap();
    let r2 = run(&mut s, "cam.job", json!({"job": inline, "dry_run": true}));
    let r1 = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r2["workholding"], r1["workholding"]);
    inline["workholding_path"] = json!(vise);
    let e = s.execute("cam.job", &json!({"job": inline, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("not both"), "{e}");
}

/// The 34 mm holder over a ball finishing a bar runs into a rotary unit's chuck jaws: the job
/// fails, its fix saying to stick the tool out further; stuck out far enough it passes, and the
/// reply says how the bar sits in the chuck.
#[test]
fn a_holder_into_the_chuck_jaws_fails_the_job() {
    let mut s = bar();
    let dir = Dir::new("hold-rotary");
    let (model, rotary) = (dir.file("m.machine-model.json"), dir.file("r.workholding.json"));
    std::fs::write(&model, VIEW_MODEL).unwrap();
    std::fs::write(
        &rotary,
        r#"{"format": "solidcraft.workholding", "version": 1, "name": "test rotary", "holding": "rotary",
            "rotary": {"axis": [100, 60], "headstock": {"min": [222, 70, 0], "max": [290, 130, 120]},
                       "chuck": {"diameter": 80, "length": 45, "face_x": 177, "side": "plus_x", "jaws": {"count": 4, "radial": 20, "axial": 14}}}}"#,
    )
    .unwrap();
    let mut job = barrel_job();
    // The bar held 14 mm in the chuck: the jaws reach from work X 18 to 32, out to radius 31, clear
    // of the ball (to X 17) but not of the 34 mm holder over it.
    job["stock"] = json!({"cylinder": {"radius": 11, "x_min": -20, "x_max": 32}});
    job["tools"] = json!([{"number": 3, "kind": "ball_endmill", "diameter": 4, "flute_length": 12, "stickout": 15, "spindle_rpm": 12000,
                           "feed": 900, "plunge_feed": 250, "holder": [{"diameter": 34, "length": 30}]}]);
    job["machine_model_path"] = json!(model);
    job["workholding_path"] = json!(rotary);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    let w = &r["workholding"];
    assert_eq!(w["holding"], "rotary", "{w}");
    let hit = w["hits"].as_array().unwrap().iter().find(|h| h["code"] == "holder-hits-rotary").unwrap_or_else(|| panic!("{w}"));
    assert_eq!(hit["operation"], "barrel", "{hit}");
    assert!(hit["fix"].as_str().unwrap().contains("stick the tool out further"), "{hit}");
    // Stuck out far enough, the holder clears the jaws.
    job["tools"][0]["stickout"] = json!(30);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["workholding"]["pass"], true, "{r}");
    assert_eq!(r["passed"], true, "{r}");
    // How the bar goes in the chuck: its +X end at the face, 14 mm in the jaws (X 18 to 32), 38 mm
    // standing out of them to X -20.
    let g = &r["workholding"]["bar_grip"];
    for (k, want) in [("face_x", 32.0), ("held_end_x", 32.0), ("free_end_x", -20.0), ("in_jaws", 14.0), ("stands_out", 38.0), ("zero_out", 32.0)] {
        assert!((g[k].as_f64().unwrap_or(f64::NAN) - want).abs() < 1e-9, "{k}: {g}");
    }
    // A rotary unit needs a machine with a rotary axis.
    job["machine"] = json!("3axis");
    job["operations"] = dome_job()["operations"].clone();
    job["tools"] = json!([ball(2, 6.0)]);
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("rotary axis"), "{e}");
}

/// The travel is checked in machine coordinates from where the workholding puts the work on the
/// table: soft limits that leave the vise's place room pass, ones that do not fail with a fix.
#[test]
fn the_travel_is_checked_from_where_the_workholding_puts_the_work() {
    let mut s = dome();
    let (dir, _, vise) = vise_dir("hold-travel", 400.0);
    let model = dir.file("t.machine-model.json");
    let with =
        |x: f64| VIEW_MODEL.replace(r#""table": {"#, &format!(r#""soft_limits": {{"min": [0, 0, -1000], "max": [{x}, 300, 0]}}, "table": {{"#));
    let mut job = dome_job();
    job["stock"] = json!({"box": {"min": [-20, -20, -20], "max": [20, 20, 8]}});
    job["machine_model_path"] = json!(model);
    job["workholding_path"] = json!(vise);
    std::fs::write(&model, with(100.0)).unwrap();
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], true, "{r}");
    std::fs::write(&model, with(20.0)).unwrap();
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    let f = r["failures"].as_array().unwrap().iter().filter_map(|f| f.as_str()).find(|f| f.starts_with("travel:")).unwrap_or_else(|| panic!("{r}"));
    assert!(f.contains("X would be at") && f.contains("travel centred on the table") && f.contains("work_zero_machine"), "{f}");
}

/// The spindle head's body (the machine model's exact parts) is checked against the part as the
/// holder is: an arm hanging down beside the spindle into the block fails; a spindle that cannot
/// rise to a clearance fails its Z travel.
#[test]
fn the_spindle_head_and_its_travel_are_checked() {
    let mut s = dome();
    let (dir, _, vise) = vise_dir("hold-head", 400.0);
    let model = dir.file("arm.machine-model.json");
    let arm = r#"{"name": "the arm", "code": "arm", "mount": "spindle", "shape": {"box": {"min": [-100, -100, -50], "max": [100, 100, -30]}}}, "#;
    std::fs::write(&model, VIEW_MODEL.replace(r#""parts": ["#, &format!(r#""parts": [{arm}"#))).unwrap();
    let mut job = dome_job();
    job["stock"] = json!({"box": {"min": [-20, -20, -20], "max": [20, 20, 8]}});
    job["machine_model_path"] = json!(model);
    job["workholding_path"] = json!(vise);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    let f = r["failures"].as_array().unwrap().iter().filter_map(|f| f.as_str()).find(|f| f.starts_with("head:")).unwrap_or_else(|| panic!("{r}"));
    assert!(f.contains("the arm runs into the part") && f.contains("stick tool 2 out further"), "{f}");
    // A spindle whose nose rises only 60 mm over the table cannot reach the clearance of 30 over
    // the vise.
    let (_dir, low, vise) = vise_dir("hold-low", 60.0);
    job["machine_model_path"] = json!(low);
    job["workholding_path"] = json!(vise);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["workholding"]["hits"].as_array().unwrap().iter().any(|h| h["code"] == "z-travel"), "{r}");
    assert!(r["failures"].as_array().unwrap().iter().any(|f| f.as_str().is_some_and(|f| f.starts_with("z_travel:"))), "{r}");
}

/// A deep side cut over a vise gripping little of the stock is warned about, not failed.
#[test]
fn a_deep_side_cut_over_a_short_grip_warns() {
    let warned = |parallels: &str, name: &str| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [-20, -15, 0]}));
        let dir = Dir::new(name);
        let (vise, model) = (dir.file("vise.workholding.json"), dir.file("m.machine-model.json"));
        std::fs::write(&vise, VIEW_VISE.replace(r#""height": 10,"#, &format!(r#""height": {parallels},"#))).unwrap();
        std::fs::write(&model, VIEW_MODEL).unwrap();
        let job = json!({
            "machine": "3axis", "post": "linuxcnc_4axis", "tools": [flat(1, 6.0)], "origin": [0, 0, 20],
            "stock": {"box": {"min": [-22, -17, -24], "max": [22, 17, 1]}}, "workholding_path": vise, "machine_model_path": model,
            "operations": [{"name": "outline", "type": "contour", "tool": 1, "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
                            "heights": {"clearance": 30, "retract": 5, "top": 1, "bottom": -19}, "stepdown": 5}]
        });
        let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
        let w = r["workholding"]["setup_warnings"].as_array().cloned().unwrap_or_default();
        let in_notes = r["notes"].as_array().map_or(0, |n| n.iter().filter(|n| n.to_string().contains("grip")).count());
        assert_eq!(w.len(), in_notes, "{r}");
        w
    };
    let short = warned("29", "grip-short");
    assert_eq!(short.len(), 1, "{short:?}");
    assert!(short[0].as_str().is_some_and(|m| m.contains("grip")), "{short:?}");
    assert!(warned("15", "grip-deep").is_empty());
}

#[test]
fn hostile_workholdings_are_refused_without_panics() {
    let mut s = dome();
    let (dir, model, vise) = vise_dir("hold-hostile", 400.0);
    let bad = dir.file("bad.workholding.json");
    let mut job = dome_job();
    job["stock"] = json!({"box": {"min": [-20, -20, -20], "max": [20, 20, 8]}});
    let cases = [
        // A workholding needs a machine model.
        json!({"workholding_path": vise}),
        json!({"machine_model_path": dir.file("missing.json"), "workholding_path": vise}),
        json!({"machine_model_path": model, "workholding_path": dir.file("missing.json")}),
        json!({"machine_model_path": model, "workholding_path": bad}),
        json!({"machine_model_path": model, "workholding": {"holding": "vise"}}),
        json!({"machine_model_path": model, "workholding": {"format": "solidcraft.workholding", "version": 1, "holding": "glue"}}),
        json!({"machine_model_path": model, "workholding": 5}),
    ];
    std::fs::write(&bad, "{not json").unwrap();
    for extra in cases {
        let mut j = job.clone();
        for (k, v) in extra.as_object().unwrap() {
            j[k] = v.clone();
        }
        match s.execute("cam.job", &json!({"job": j, "dry_run": true})) {
            Err(EngineError::Internal(_, msg)) => panic!("{extra}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{extra}: accepted: {v}"),
        }
    }
}

// Machine rates, the setup sheet and one program per tool.

/// A small mill's top rates: 8000 mm/min across, 4000 in Z, A at 8000°/min, the spindle from
/// 10000 to 24000 rpm with a six-second start.
fn mill_rates() -> Value {
    json!({
        "rapid_xyz_mm_min": 8000, "rapid_z_mm_min": 4000, "rapid_a_deg_min": 8000,
        "max_feed_mm_min": 8000, "max_feed_z_mm_min": 4000, "max_a_deg_min": 8000,
        "spindle_rpm": [10000, 24000], "spindle_start_seconds": 6, "travel_mm": [300, 170, 130]
    })
}

#[test]
fn machine_rates_time_the_program_and_warn_about_slowed_blocks() {
    let mut s = top_pocket();
    // Without rates: the time at feed as written, no rapid time.
    let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "dry_run": true}));
    assert_clean(&r);
    assert!(r["machine_time"].is_null() && r["rapid_minutes"].is_null(), "{r}");
    assert_eq!(r["cut_minutes"], r["readback"]["cut_minutes"], "{r}");

    // With the mill's rates: rapids timed, one spindle start, nothing slowed.
    let mut job = pocket_job();
    job["machine_rates"] = mill_rates();
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    let t = &r["machine_time"];
    assert!(r["rapid_minutes"].as_f64().is_some_and(|m| m > 0.0), "{r}");
    assert_eq!(r["rapid_minutes"], t["rapid_minutes"], "{r}");
    assert_eq!(r["cut_minutes"], t["cut_minutes"], "{r}");
    assert!((t["spindle_minutes"].as_f64().unwrap() - 0.1).abs() < 1e-12, "{t}");
    assert_eq!(t["slowed_blocks"], 0, "{t}");
    let written = t["written_cut_minutes"].as_f64().unwrap();
    assert!((written - r["readback"]["cut_minutes"].as_f64().unwrap()).abs() < 1e-9, "{r}");
    let total = t["cut_minutes"].as_f64().unwrap() + t["rapid_minutes"].as_f64().unwrap() + t["spindle_minutes"].as_f64().unwrap();
    assert!((t["total_minutes"].as_f64().unwrap() - total).abs() < 1e-9, "{t}");

    // A top feed below the job's 800 mm/min: the controller slows the cuts, the reply says so,
    // and the program still passes its checks.
    job["machine_rates"]["max_feed_mm_min"] = json!(500);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    let t = &r["machine_time"];
    assert!(t["feed_over"].as_u64().is_some_and(|n| n > 0), "{t}");
    assert!(t["cut_minutes"].as_f64().unwrap() > written, "{t}");
    assert!(!r["notes"].as_array().unwrap().is_empty(), "the slowed blocks are in the notes: {r}");
    // The setup sheet states the machine's limits the job runs into first.
    assert!(!r["summary"]["warnings"].is_null(), "{r}");

    // A spindle speed below the machine's range is run at its lowest: the job passes and says so.
    job["machine_rates"]["max_feed_mm_min"] = json!(8000);
    job["machine_rates"]["spindle_rpm"] = json!([15000, 24000]);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], true, "{r}");
    let notes = r["notes"].to_string();
    assert!(notes.contains("outside the machine's 15000-24000 rpm (S12000 at line"), "{notes}");
    assert_eq!(r["machine_time"]["spindle_out_of_range"][0]["rpm"].as_f64(), Some(12000.0), "{r}");

    // The rates may come from a file beside the job.
    let dir = Dir::new("rates");
    std::fs::write(dir.0.join("rates.json"), mill_rates().to_string()).unwrap();
    let mut job = pocket_job();
    job["machine_rates_path"] = json!("rates.json");
    let job_path = dir.file("pocket.cam.json");
    std::fs::write(&job_path, job.to_string()).unwrap();
    let r = run(&mut s, "cam.job", json!({"job_path": job_path, "dry_run": true}));
    assert_clean(&r);
    assert_eq!(r["machine_time"]["slowed_blocks"], 0, "{r}");

    // Bad rates are refused before anything is planned, never a panic.
    let with = |f: &dyn Fn(&mut Value)| {
        let mut job = pocket_job();
        job["machine_rates"] = mill_rates();
        f(&mut job);
        json!({"job": job, "dry_run": true})
    };
    let cases = [
        (with(&|j| j["machine_rates"]["rapid_xyz"] = json!(1)), "rapid_xyz"),
        (with(&|j| j["machine_rates"]["rapid_xyz_mm_min"] = json!(0)), "machine rate"),
        (with(&|j| j["machine_rates"]["max_a_deg_min"] = json!(-5)), "machine rate"),
        (with(&|j| j["machine_rates"]["max_feed_mm_min"] = json!(1e300)), "machine rate"),
        (with(&|j| j["machine_rates"]["spindle_rpm"] = json!([24000, 10000])), "spindle"),
        (with(&|j| j["machine_rates"]["spindle_start_seconds"] = json!(1e9)), "spindle"),
        (with(&|j| j["machine_rates"]["travel_mm"] = json!([300, 0, 130])), "travel"),
        (with(&|j| j["machine_rates"] = json!({"max_a_deg_min": 100})), "rapid_xyz_mm_min"),
        (with(&|j| j["machine_rates"] = json!("8000")), "machine_rates"),
        (with(&|j| j["machine_rates_path"] = json!("rates.json")), "not both"),
    ];
    for (p, word) in cases {
        let e = s.execute("cam.job", &p).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(word), "{p}: {e}");
    }
    std::fs::write(dir.0.join("rates.json"), json!({"rapid_xyz_mm_min": 8000, "max_a": 1}).to_string()).unwrap();
    let e = s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("max_a"), "{e}");
    std::fs::write(dir.0.join("rates.json"), "{not json").unwrap();
    assert!(matches!(s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})), Err(EngineError::BadParams { .. })));
    std::fs::remove_file(dir.0.join("rates.json")).unwrap();
    assert!(matches!(s.execute("cam.job", &json!({"job_path": job_path, "dry_run": true})), Err(EngineError::BadParams { .. })));
}

/// A two-tool job on the pocketed block: the pocket with a 4 mm end mill, then the outline with
/// a 6 mm one that has a holder and a stickout.
fn two_tool_job() -> Value {
    let mut job = pocket_job();
    job["name"] = json!("block two tools");
    job["tools"] = json!([
        flat(1, 4.0),
        {"number": 2, "kind": "flat_endmill", "diameter": 6, "flute_length": 15, "description": "6 mm end mill, long reach",
         "stickout": 25, "holder": [{"diameter": 30, "length": 20}], "spindle_rpm": 10000, "feed": 900, "plunge_feed": 250}
    ]);
    job["operations"].as_array_mut().unwrap().push(json!({
        "name": "outline", "type": "contour", "tool": 2, "spindle_rpm": 11000, "feeds": {"cut": 700, "plunge": 150},
        "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
        "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -10}, "stepdown": 5
    }));
    job
}

fn scoped<T>(dir: &Dir, f: impl FnOnce() -> T) -> T {
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([dir.0.clone()])), f)
}

#[test]
fn a_setup_sheet_lists_the_tools_operations_and_checks_of_a_two_tool_job() {
    let mut s = top_pocket();
    let dir = Dir::new("sheet");
    let (out, page, md) = (dir.file("block.ngc"), dir.file("block-setup.html"), dir.file("block-setup.md"));
    let r = scoped(&dir, || run(&mut s, "cam.job", json!({"job": two_tool_job(), "output": out, "setup_sheet": page})));
    assert_clean(&r);
    assert_eq!(r["outputs"]["setup_sheet"], page.as_str(), "{r}");
    let sum = &r["summary"];
    // Only the tools the operations use, by number, each with its operations and how low it goes.
    let tools = sum["tools"].as_array().unwrap();
    assert_eq!(tools.iter().map(|t| t["number"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2], "{sum}");
    assert_eq!(tools[0]["operations"], json!(["top pocket"]), "{sum}");
    assert_eq!(tools[1]["operations"], json!(["outline"]), "{sum}");
    assert_eq!(tools[1]["holder"], "Ø30 x 20", "{sum}");
    assert_eq!(tools[1]["stickout"].as_f64(), Some(25.0), "{sum}");
    assert!(tools[0].get("holder").is_none(), "{sum}");
    // Z min is the lowest Z the program writes for each tool: what cam.gcode.check reads.
    let c = run(&mut s, "cam.gcode.check", json!({"path": out}));
    assert_eq!(c["bounds"][0][2].as_f64(), Some(-10.0), "{c}");
    let zmins: Vec<f64> = tools.iter().map(|t| t["z_min"].as_f64().unwrap()).collect();
    assert_eq!(zmins, [-4.0, -10.0], "{sum}");
    let ops = sum["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!((ops[1]["spindle_rpm"].as_f64(), ops[1]["feed"].as_f64(), ops[1]["plunge_feed"].as_f64()), (Some(11000.0), Some(700.0), Some(150.0)));
    assert_eq!((ops[0]["spindle_rpm"].as_f64(), ops[0]["feed"].as_f64()), (Some(12000.0), Some(800.0)), "the tool's data: {sum}");
    assert!(ops[0].get("rotary").is_none(), "a 3-axis machine has no rotary: {sum}");
    assert!(ops.iter().all(|o| o["cut_length"].as_f64().is_some_and(|l| l > 50.0) && o["cut_minutes"].as_f64().is_some_and(|m| m > 0.0)));
    assert_eq!(sum["totals"]["tool_changes"], 2, "{sum}");
    assert_eq!(sum["totals"]["operations"], 2, "{sum}");
    assert_eq!(sum["totals"]["tools"], 2, "{sum}");
    // The part's extent from work zero, from the block's exact bounds.
    assert_eq!(sum["part_extent"], json!([[-20.0, -15.0, -10.0], [20.0, 15.0, 0.0]]), "{sum}");
    let zero = sum["work_zero"].to_string();
    assert!(zero.contains("X0 is in the middle of the part in X") && zero.contains("Z0 is on the part's top"), "{zero}");
    assert!(sum["rotary"].as_array().is_some_and(Vec::is_empty), "{sum}");
    assert!(sum["checks"].to_string().contains("Read-back: passed"), "{sum}");
    // The page holds the same.
    let html = std::fs::read_to_string(&page).unwrap();
    for want in [
        "Setup sheet: block two tools",
        "top pocket",
        "outline",
        "T1",
        "T2",
        "Ø30 x 20",
        "6 mm end mill, long reach",
        "11000",
        "Read-back",
        "passed",
        "block.ngc",
        "@media print",
    ] {
        assert!(html.contains(want), "{want}: {html}");
    }
    assert!(!html.contains("<script") && !html.contains("http://") && !html.contains("https://") && !html.contains("<link"), "self-contained");
    // The Markdown sheet; the summary is the same whatever the sheet's format, and on a dry run.
    let r2 = scoped(&dir, || run(&mut s, "cam.job", json!({"job": two_tool_job(), "output": out, "setup_sheet": md})));
    assert_eq!(r2["summary"], r["summary"]);
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.starts_with("# Setup sheet: block two tools\n") && text.contains("| T2 | flat end mill | Ø6 |"), "{text}");
    let dry = run(&mut s, "cam.job", json!({"job": two_tool_job(), "dry_run": true}));
    assert_eq!(dry["summary"]["tools"], r["summary"]["tools"]);
    assert!(dry["outputs"].get("setup_sheet").is_none(), "{dry}");
}

#[test]
fn a_setup_sheet_escapes_hostile_names() {
    let mut s = top_pocket();
    let dir = Dir::new("sheet-hostile");
    let mut job = two_tool_job();
    job["name"] = json!("<script>alert(1)</script> & 'x'");
    job["operations"][0]["name"] = json!("<img src=x onerror=alert(2)> | *b* `c`");
    job["tools"][1]["description"] = json!("</td></tr></table><script>alert(3)</script>");
    let (page, md) = (dir.file("hostile.html"), dir.file("hostile.md"));
    run(&mut s, "cam.job", json!({"job": job, "output": dir.file("a.ngc"), "setup_sheet": page}));
    run(&mut s, "cam.job", json!({"job": job, "output": dir.file("b.ngc"), "setup_sheet": md}));
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(!html.contains("<script") && !html.contains("<img") && !html.contains("</td></tr></table><"), "{html}");
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &#39;x&#39;") && html.contains("&lt;img src=x onerror=alert(2)&gt;"),
        "{html}"
    );
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(!text.contains("<script") && !text.contains("<img"), "{text}");
    let row = text.lines().find(|l| l.starts_with("| 1 |")).unwrap();
    assert!(row.contains("\\| \\*b\\* \\`c\\`"), "{row}");
    assert_eq!(row.replace("\\|", "").matches('|').count(), 13, "the row keeps its cells: {row}");
}

#[test]
fn a_setup_sheet_is_written_only_inside_the_agents_folders() {
    let mut s = top_pocket();
    let dir = Dir::new("sheet-scope");
    let root = dir.0.join("allowed");
    std::fs::create_dir_all(&root).unwrap();
    let inside = root.join("block.ngc").to_string_lossy().into_owned();
    let outside = dir.file("block-setup.html");
    let scope = Some(solidcraft_io::vfs::PathScope::new([root.clone()]));
    solidcraft_io::vfs::scoped(scope, || {
        let e = s.execute("cam.job", &json!({"job": two_tool_job(), "output": inside, "setup_sheet": outside})).unwrap_err();
        assert!(e.to_string().contains("outside the folders this agent may use"), "{e}");
        let sheet_in = root.join("s.html").to_string_lossy().into_owned();
        let txt = root.join("s.txt").to_string_lossy().into_owned();
        for (p, word) in [
            (json!({"job": two_tool_job(), "output": inside, "setup_sheet": txt}), ".html or .md"),
            (json!({"job": two_tool_job(), "dry_run": true, "setup_sheet": "sheet.pdf"}), ".html or .md"),
            (json!({"job": two_tool_job(), "output": inside, "setup_sheet": ""}), ".html or .md"),
            (json!({"job": two_tool_job(), "output": sheet_in, "setup_sheet": sheet_in}), "file of its own"),
            (json!({"job": two_tool_job(), "output": inside, "preview": sheet_in, "setup_sheet": sheet_in}), "file of its own"),
            (json!({"job": two_tool_job(), "output": inside, "setup_sheet": 5}), "setup_sheet"),
        ] {
            let e = s.execute("cam.job", &p).unwrap_err();
            assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains(word), "{p}: {e}");
        }
    });
    assert!(!PathBuf::from(&outside).exists(), "nothing outside the folders");
    assert!(!PathBuf::from(&inside).exists(), "nothing at all: the paths are checked before anything is written");
}

#[test]
fn a_setup_sheet_is_the_same_bytes_every_time() {
    let dir = Dir::new("sheet-same");
    let (out, page, md) = (dir.file("p.ngc"), dir.file("p.html"), dir.file("p.md"));
    let mut runs: Vec<(Vec<u8>, Vec<u8>, Value)> = Vec::new();
    for _ in 0..2 {
        // A fresh session each time: nothing carried over.
        let mut s = top_pocket();
        let r = run(&mut s, "cam.job", json!({"job": two_tool_job(), "output": out, "setup_sheet": page}));
        run(&mut s, "cam.job", json!({"job": two_tool_job(), "output": out, "setup_sheet": md}));
        runs.push((std::fs::read(&page).unwrap(), std::fs::read(&md).unwrap(), r["summary"].clone()));
    }
    assert!(runs[0] == runs[1], "two runs give different sheets");
    let html = String::from_utf8(runs[0].0.clone()).unwrap();
    assert!(!html.contains("2026") && !html.contains("2025"), "no date on the sheet");
}

#[test]
fn a_setup_sheet_spelt_like_the_preview_is_still_the_preview() {
    let mut s = top_pocket();
    let dir = Dir::new("sheet-same-file");
    let out = dir.file("block.ngc");
    let page = dir.file("block.html");
    let dotted = dir.0.join(".").join("block.html").to_string_lossy().into_owned();
    let mut same = vec![dotted];
    if cfg!(windows) {
        same.push(dir.file("BLOCK.HTML"));
    }
    for sheet in same {
        let e =
            scoped(&dir, || s.execute("cam.job", &json!({"job": two_tool_job(), "output": out, "preview": page, "setup_sheet": sheet}))).unwrap_err();
        assert!(e.to_string().contains("file of its own"), "{sheet}: {e}");
    }
    assert!(!PathBuf::from(&page).exists() && !PathBuf::from(&out).exists(), "nothing written");
}

#[test]
fn a_4th_axis_setup_sheet_gives_the_a_convention_and_the_centreline() {
    let mut s = top_pocket();
    let mut job = pocket_job();
    job["machine"] = json!("4axis_a");
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    let sum = &r["summary"];
    assert_eq!(sum["operations"][0]["rotary"], "A0°", "A held at zero, as written: {sum}");
    let rotary = sum["rotary"].to_string();
    assert!(rotary.contains("A turns the table (and the part) about +X through work zero"), "{rotary}");
    assert!(rotary.contains("A+90° brings the part's +Y side up under the tool"), "{rotary}");
    assert!(sum["work_zero"].to_string().contains("Y0 Z0 is on the A axis centreline"), "{sum}");
    // Reversed: positive A turns the other way.
    job["a_reversed"] = json!(true);
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_clean(&r);
    assert!(r["summary"]["rotary"].to_string().contains("-Y side up"), "{r}");
}

/// A job of two tools for a machine that ignores tool changes: one program stops for the
/// operator at the change and is warned about; split, each tool's program is complete on its own.
#[test]
fn a_job_is_split_one_program_per_tool_for_a_machine_that_ignores_tool_changes() {
    let mut s = top_pocket();
    let dir = Dir::new("split");
    let mut job = two_tool_job();
    job["tool_change"] = json!("ignored");
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], true, "a warning, not a failure: {r}");
    let w = r["warnings"][0].as_str().unwrap_or_else(|| panic!("{r}"));
    assert!(w.contains("ignores tool changes") && w.contains("T1, T2") && w.contains("split_by_tool"), "{w}");
    let (out, sheet) = (dir.file("block.ngc"), dir.file("block.setup.md"));
    // Not split, the program stops at the change to T2 for the operator, and the sheet says so.
    run(&mut s, "cam.job", json!({"job": job, "output": out, "setup_sheet": sheet}));
    let md = std::fs::read_to_string(&sheet).unwrap();
    assert!(md.contains("## Tool changes") && md.contains("it stops (M0) at each change after"), "{md}");
    let tap = std::fs::read_to_string(&out).unwrap();
    assert_eq!(tap.lines().filter(|l| l.trim() == "M0").count(), 1, "{tap}");
    std::fs::remove_file(&out).unwrap();
    // Split: two programs, each with its own safe start, tool change, length offset and header.
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out, "setup_sheet": sheet, "split_by_tool": true}));
    assert!(r["warnings"].is_null(), "{r}");
    let files: Vec<String> = r["outputs"]["programs"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    assert_eq!(files, [dir.file("block-1-T1.ngc"), dir.file("block-2-T2.ngc")], "{r}");
    assert!(r["outputs"]["gcode"].is_null() && !std::path::Path::new(&out).exists());
    let parts = r["programs"].as_array().unwrap();
    assert_eq!((parts[1]["order"].as_u64(), parts[1]["tool"].as_u64(), parts[1]["stickout"].as_f64()), (Some(2), Some(2), Some(25.0)), "{r}");
    assert_eq!(parts[1]["operations"], json!(["outline"]));
    for (k, (f, t)) in files.iter().zip([1, 2]).enumerate() {
        let text = std::fs::read_to_string(f).unwrap();
        let head: Vec<String> = text.lines().take(5).map(str::to_lowercase).collect();
        assert!(head[1].contains(&format!("program {} of 2", k + 1)), "{head:?}");
        assert!(head[2].starts_with(&format!("(this program uses t{t} - ")) && head[2].contains("stickout"), "{head:?}");
        assert_eq!(text.lines().filter(|l| l.contains("M6")).count(), 1, "one tool change: {f}");
        assert!(text.contains(&format!("T{t} M6")) && text.contains(&format!("H{t}")), "{f}");
        assert!(text.trim_end().ends_with("M30"), "{f}");
        // Each reads on its own.
        let c = run(&mut s, "cam.gcode.check", json!({"path": f}));
        assert!(c["errors"].as_array().unwrap().is_empty(), "{c}");
    }
    let md = std::fs::read_to_string(&sheet).unwrap();
    assert!(
        md.contains("## Programs, in the order to run them") && md.contains("1. [ ] block-1-T1.ngc") && md.contains("2. [ ] block-2-T2.ngc"),
        "{md}"
    );
    assert!(md.contains("## Tool changes") && md.contains("Run the programs below in order"), "{md}");
    // The header names the programs written, not the one file of every tool (not written).
    assert!(md.contains("- **Programs:** block-1-T1.ngc, block-2-T2.ngc") && !md.contains("**Program:** block.ngc"), "{md}");
}

#[test]
fn a_split_job_beside_an_older_whole_program_warns_about_it() {
    let mut s = top_pocket();
    let dir = Dir::new("split-stale");
    let mut job = two_tool_job();
    job["tool_change"] = json!("ignored");
    let out = dir.file("block.ngc");
    run(&mut s, "cam.job", json!({"job": job, "output": out}));
    assert!(std::path::Path::new(&out).exists());
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out, "split_by_tool": true}));
    let w = r["warnings"].as_array().unwrap_or_else(|| panic!("{r}"));
    assert!(w.iter().filter_map(|w| w.as_str()).any(|w| w.contains("block.ngc") && w.contains("delete")), "{w:?}");
    assert!(!r["warnings"].to_string().contains("   "), "no run of spaces in the text: {w:?}");
}

// The stock-removal simulation.

/// `cam.simulate` and `cam.job`'s `simulate`: the written program swept through stock taken
/// from the part (its box) and held against the part.
#[test]
fn the_simulation_passes_a_good_program_and_catches_a_gouge_and_a_rapid() {
    let mut s = top_pocket();
    let dir = Dir::new("sim");
    let (out, sheet) = (dir.file("pocket.ngc"), dir.file("pocket.md"));
    let r = run(&mut s, "cam.job", json!({"job": pocket_job(), "output": out, "setup_sheet": sheet, "simulate": true}));
    assert_clean(&r);
    let sim = &r["simulation"];
    assert_eq!(sim["passed"], true, "{sim}");
    assert_eq!(sim["gouge"]["cells"], 0, "{sim}");
    assert!(sim["rapids_in_material"].as_array().is_some_and(Vec::is_empty), "{sim}");
    assert_eq!(sim["stock"]["kind"], "box", "{sim}");
    assert_eq!(sim["stock"]["max"], json!([20.0, 15.0, 0.0]), "the block's box: {sim}");
    // The pocket, 16 × 12 × 4, less the corners a 4 mm tool cannot reach.
    let removed = sim["removed_volume"].as_f64().unwrap();
    assert!((700.0..=768.0).contains(&removed), "{sim}");
    assert!(sim["minutes"].as_f64().is_some_and(|m| m > 0.0), "{sim}");
    // Those corners are left on the part: a finding, not a failed check, and the sheet says so.
    assert_eq!(sim["verdict"], "part not finished", "{sim}");
    assert!(sim["part_left"]["open_max"].as_f64().is_some_and(|t| t > 0.3), "{sim}");
    assert!(r["warnings"].to_string().contains("part not finished"), "{r}");
    assert!(r["summary"]["checks"].to_string().contains("Simulation: part not finished"), "{r}");
    assert!(std::fs::read_to_string(&sheet).unwrap().contains("Simulation"), "the sheet's checks");
    // The file on its own, with the stock left as an STL file.
    let stl = dir.file("stock.stl");
    let c = run(&mut s, "cam.simulate", json!({"path": out, "job": pocket_job(), "stock_stl": stl}));
    assert_eq!(c["passed"], true, "{c}");
    assert_eq!(c["path"], out.as_str());
    assert_eq!(c["removed_volume"], sim["removed_volume"]);
    assert_eq!(c["stock_stl"], stl.as_str());
    assert!(std::fs::metadata(&stl).is_ok_and(|m| m.len() > 84 + 50 * 1000));
    // 1 mm into the block's top beside the pocket, then a rapid on through the block.
    let bad = dir.file("bad.ngc");
    std::fs::write(&bad, "G21\nT1 M6\nG0 X-18. Y-13. Z10.\nG1 Z-1. F300\nG1 X-12.\nG0 Y-10.\nG0 Z30.\n").unwrap();
    let c = run(&mut s, "cam.simulate", json!({"path": bad, "job": pocket_job(), "resolution": 0.1}));
    assert_eq!(c["passed"], false, "{c}");
    assert_eq!(c["verdict"], "failed", "{c}");
    assert!((c["gouge"]["worst"].as_f64().unwrap() - 1.0).abs() < 0.01, "{c}");
    let lines: Vec<u64> = c["rapids_in_material"].as_array().unwrap().iter().filter_map(|m| m["line"].as_u64()).collect();
    assert_eq!(lines, vec![6], "{c}");
    assert_eq!(c["failures"].as_array().map(Vec::len), Some(2), "{c}");
    // A job whose simulation fails is refused, as for every other check.
    let mut deep = pocket_job();
    deep["operations"][0]["heights"]["bottom"] = json!(-5);
    let e = s.execute("cam.job", &json!({"job": deep, "output": out, "simulate": true})).unwrap_err();
    assert!(e.to_string().contains("simulation: the program cuts up to"), "{e}");
    let r = run(&mut s, "cam.job", json!({"job": deep, "dry_run": true, "simulate": true}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["summary"]["checks"].to_string().contains("Simulation: FAILED"), "{r}");
    // `simulate_resolution` goes with `simulate`.
    let e = s.execute("cam.job", &json!({"job": pocket_job(), "dry_run": true, "simulate_resolution": 0.1})).unwrap_err();
    assert!(e.to_string().contains("simulate_resolution"), "{e}");
}

/// A bar on a 4th axis: the stock is the cylinder through the part's farthest point, and a
/// program that never writes A has no move the simulation can follow, which is a failure, not a
/// pass with nothing checked.
#[test]
fn a_simulation_on_a_4th_axis_that_follows_no_move_does_not_pass() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 30, "base": [-15, 0, 0], "axis": "X", "body_name": "Bar"}));
    let job = json!({"machine": "4axis_a", "post": "linuxcnc_4axis", "tools": [flat(3, 4.0)], "operations": []});
    let dir = Dir::new("sim-no-a");
    let gcode = dir.file("no-a.ngc");
    std::fs::write(&gcode, "G21\nT3 M6\nG0 X0. Y0. Z30.\nG0 Z5.\nG1 X5. F100\nG0 Z30.\n").unwrap();
    let r = run(&mut s, "cam.simulate", json!({"path": gcode, "job": job, "resolution": 0.2}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["failures"].as_array().unwrap().iter().any(|f| f.as_str().is_some_and(|f| f.contains("no move"))), "{r}");
    assert_eq!(r["stock"]["kind"], "cylinder", "{r}");
    assert!((r["stock"]["radius"].as_f64().unwrap() - 10.0).abs() < 1e-6, "{r}");
    // With the A written, the plunge into the bar is caught.
    std::fs::write(&gcode, "G21\nT3 M6\nG0 X0. Y0. Z30. A0.\nG0 Z5.\nG1 X5. F100\nG0 Z30.\n").unwrap();
    let r = run(&mut s, "cam.simulate", json!({"path": gcode, "job": job, "resolution": 0.2}));
    assert_eq!(r["passed"], false, "{r}");
    assert!(r["rapids_in_material"].as_array().is_some_and(|m| !m.is_empty()), "{r}");
    assert!(r["gouge"]["worst"].as_f64().is_some_and(|w| w > 4.0), "{r}");
}

#[test]
fn hostile_simulations_are_refused_without_panics() {
    let mut s = top_pocket();
    let dir = Dir::new("sim-hostile");
    let gcode = dir.file("p.ngc");
    std::fs::write(&gcode, "G21\nT1 M6\nG0 X0. Y0. Z10.\nG1 Z8. F300\n").unwrap();
    let junk = dir.file("junk.ngc");
    std::fs::write(&junk, "G21\nT1 M6\nG0 X0 Y0 Z10\n\u{0}garbage QQQ\n").unwrap();
    let five = json!({"name": "5ax", "rotaries": [
        {"letter": "A", "direction": [1, 0, 0], "center": [0, 0, 0], "carrier": "table"},
        {"letter": "C", "direction": [0, 0, 1], "center": [0, 0, 0], "carrier": "table"}]});
    let mut five_job = pocket_job();
    five_job["machine"] = five;
    let mut no_body = pocket_job();
    no_body["bodies"] = json!(["Nothing"]);
    let cases = [
        json!({"path": gcode}),
        json!({"job": pocket_job()}),
        json!({"path": gcode, "job": pocket_job(), "job_path": dir.file("j.json")}),
        json!({"path": gcode, "job": pocket_job(), "resolution": 0}),
        json!({"path": gcode, "job": pocket_job(), "resolution": -1}),
        json!({"path": gcode, "job": pocket_job(), "resolution": 1e-5}),
        json!({"path": gcode, "job": pocket_job(), "resolution": 0.001}),
        json!({"path": gcode, "job": pocket_job(), "resolution": "fine"}),
        json!({"path": gcode, "job": pocket_job(), "stock_stl": ""}),
        json!({"path": gcode, "job": pocket_job(), "stock_stl": gcode}),
        json!({"path": gcode, "job": pocket_job(), "then": vec![gcode.clone(); 201]}),
        json!({"path": dir.file("missing.ngc"), "job": pocket_job()}),
        json!({"path": junk, "job": pocket_job()}),
        json!({"path": gcode, "job": five_job}),
        json!({"path": gcode, "job": no_body}),
        json!({"path": gcode, "job": pocket_job(), "resolutoin": 0.1}),
    ];
    for p in &cases {
        match s.execute("cam.simulate", p) {
            Err(EngineError::Internal(_, msg)) => panic!("{p}: panicked: {msg}"),
            Err(_) => {}
            Ok(v) => panic!("{p}: accepted: {v}"),
        }
    }
    // A tool the program loads that the job lacks.
    std::fs::write(&gcode, "G21\nT9 M6\nG0 X0. Y0. Z10.\n").unwrap();
    let e = s.execute("cam.simulate", &json!({"path": gcode, "job": pocket_job()})).unwrap_err();
    assert!(e.to_string().contains("tool 9"), "{e}");
}

/// The simulation warns about spindle speeds outside the machine's range and notes the program's
/// stops for tool changes.
#[test]
fn the_simulation_checks_spindle_speeds_and_lists_tool_change_stops() {
    let mut s = top_pocket();
    let dir = Dir::new("sim-spindle");
    let tap = dir.file("speeds.ngc");
    let text = "G21 G90\nT1 M6\nS30000 M3\nG0 X-30 Y-30 Z10\nT2 M6\nM0\nS1000 M3\nG0 X30 Y-30 Z10\nT1 M6\nS12000 M3\nG0 X30 Y30 Z10\nM30\n";
    std::fs::write(&tap, text).unwrap();
    let job = json!({
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [flat(1, 6.0), flat(2, 4.0)],
        "operations": [],
        "machine_rates": {"rapid_xyz_mm_min": 5000, "spindle_rpm": [3000, 24000]}
    });
    let c = run(&mut s, "cam.simulate", json!({"path": tap, "job": job, "resolution": 0.5}));
    let warnings = c["warnings"].to_string();
    assert!(warnings.contains("2 spindle speeds are outside the machine's 3000-24000 rpm (S30000 at line 3)"), "{c}");
    let notes = c["notes"].to_string();
    assert!(notes.contains("stops for 3 tool changes") && notes.contains("at lines 2, 5, 9"), "{notes}");
    // In range: no spindle warning.
    let mut quiet = job.clone();
    quiet["machine_rates"]["spindle_rpm"] = json!([500, 40000]);
    let c = run(&mut s, "cam.simulate", json!({"path": tap, "job": quiet, "resolution": 0.5}));
    assert!(!c["warnings"].to_string().contains("spindle speeds"), "{c}");
}

/// The programs of a job written one per tool, simulated in order on one stock, leave what the
/// whole program leaves; the last one alone leaves the first one's cuts out.
#[test]
fn programs_written_one_per_tool_simulate_in_order_as_the_whole_program() {
    let mut s = top_pocket();
    let dir = Dir::new("sim-split");
    let mut job = two_tool_job();
    job["tool_change"] = json!("ignored");
    let (whole, out) = (dir.file("whole.ngc"), dir.file("block.ngc"));
    run(&mut s, "cam.job", json!({"job": job, "output": whole}));
    let r = run(&mut s, "cam.job", json!({"job": job, "output": out, "split_by_tool": true}));
    let files: Vec<String> = r["outputs"]["programs"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    let a = run(&mut s, "cam.simulate", json!({"path": whole, "job": job}));
    let b = run(&mut s, "cam.simulate", json!({"path": files[0], "then": [files[1]], "job": job}));
    let last = run(&mut s, "cam.simulate", json!({"path": files[1], "job": job}));
    let removed = |r: &Value| r["removed_volume"].as_f64().unwrap_or_else(|| panic!("{r}"));
    assert!((removed(&a) - removed(&b)).abs() <= 1e-6 * removed(&a), "{} {}", a["removed_volume"], b["removed_volume"]);
    assert!(removed(&last) < removed(&b) - 1.0, "{} {}", last["removed_volume"], b["removed_volume"]);
    assert!(b["notes"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.contains("2 programs simulated in order"))), "{b}");
}
