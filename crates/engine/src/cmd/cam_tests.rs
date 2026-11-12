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
