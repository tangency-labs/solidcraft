//! Tests of the CAM commands: contours and pockets planned on designs built with commands,
//! written only inside an agent's folders and read back; posts listed and copied; G-code
//! checked; unknown keys and hostile parameters refused.

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
    // The default contour runs outside; inside runs a tool radius in.
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
