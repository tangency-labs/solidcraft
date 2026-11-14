//! `solidcraft-cli check`: Check Drawing (`drawing.check`) on a design's drawings from a shell, as
//! one JSON report, with exit code 1 when anything is found, so a script or a CI job can stop on it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use solidcraft_engine::Session;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-check-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    /// `s`'s design saved here as `name`.
    fn save(&self, s: &mut Session, name: &str) -> PathBuf {
        let path = self.0.join(name);
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// A plate 100 x 60 x 10 mm with a 2 mm chamfer on its top front edge.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[50, 0, 10]], "distance": 2, "body": "Plate"}));
    s
}

/// A drawing called `name` of `s`'s design with a front view at 1:1 and a length dimension of the
/// chamfer's lower edge, closed again; its id.
fn drawing(s: &mut Session, name: &str) -> String {
    let created = run(s, "drawing.create", json!({"name": name}));
    let sheet = created["sheets"][0]["id"].clone();
    let view = run(s, "drawing.view.base", json!({"sheet": sheet, "scale": "1:1"}))["view"].clone();
    run(s, "drawing.dimension.add", json!({"view": view, "kind": {"length": {"edge": [50, 0, 8]}}}));
    run(s, "drawing.close", json!({}));
    created["id"].as_str().unwrap().to_string()
}

/// The chamfer deleted: every drawing's dimension of it is lost.
fn without_chamfer(s: &mut Session) {
    run(s, "timeline.delete", json!({"feature": "Chamfer1"}));
}

fn check(design: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("check").arg(design).args(args).output().unwrap()
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
}

/// How many findings each checked drawing has, by name.
fn found(report: &Value) -> Vec<(String, usize)> {
    let drawings = report["drawings"].as_array().unwrap();
    drawings.iter().map(|d| (d["name"].as_str().unwrap().to_string(), d["check"]["findings"].as_array().unwrap().len())).collect()
}

/// The kinds of a drawing's findings.
fn kinds(drawing: &Value) -> Vec<&str> {
    drawing["check"]["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect()
}

/// The lost dimension is reported, as `drawing.check` gives it, with the total; the exit is 1
/// after the report, and stderr says how much was found.
#[test]
fn a_lost_dimension_fails_the_check_after_the_report() {
    let scratch = Scratch::new();
    let mut s = plate();
    let id = drawing(&mut s, "Plate drawing");
    without_chamfer(&mut s);
    let design = scratch.save(&mut s, "plate.solidcraft");
    let output = check(&design, &[]);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    let report = report(&output);
    let checked = &report["drawings"][0];
    assert_eq!((checked["id"].as_str(), checked["name"].as_str()), (Some(id.as_str()), Some("Plate drawing")), "{report}");
    assert!(kinds(checked).contains(&"lost"), "{report}");
    let counted: u64 = checked["check"]["counts"].as_array().unwrap().iter().map(|c| c["count"].as_u64().unwrap()).sum();
    let total = found(&report)[0].1;
    assert_eq!((report["findings"].as_u64(), counted), (Some(total as u64), total as u64), "{report}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("error: {total} finding(s) in 1 drawing(s)")), "{stderr}");
    // The same report in a file, and nothing on stdout.
    let file = scratch.0.join("report.json");
    let output = check(&design, &["--json-out", &file.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap(), report);
}

/// A drawing with nothing to find passes: its dimension stands, and its sheet has no title block
/// whose fields could have no value.
#[test]
fn a_drawing_with_nothing_to_find_passes() {
    let scratch = Scratch::new();
    let mut s = plate();
    let created = run(&mut s, "drawing.create", json!({"name": "Clean"}));
    let sheet = created["sheets"][0]["id"].clone();
    run(&mut s, "drawing.sheet.set", json!({"sheet": sheet, "title_block": null}));
    let view = run(&mut s, "drawing.view.base", json!({"sheet": sheet, "scale": "1:1"}))["view"].clone();
    run(&mut s, "drawing.dimension.add", json!({"view": view, "kind": {"length": {"edge": [50, 0, 8]}}}));
    run(&mut s, "drawing.close", json!({}));
    let design = scratch.save(&mut s, "clean.solidcraft");
    let output = check(&design, &[]);
    let report = report(&output);
    assert!(output.status.success(), "{report} {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!((found(&report), report["findings"].as_u64()), (vec![("Clean".to_string(), 0)], Some(0)), "{report}");
}

/// A design with no drawing has nothing to find: exit 0.
#[test]
fn a_design_without_drawings_passes() {
    let scratch = Scratch::new();
    let design = scratch.save(&mut plate(), "plate.solidcraft");
    let output = check(&design, &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(report(&output), json!({"drawings": [], "findings": 0}));
}

/// `--drawing` checks only the drawing it names, by name or by id; `--sheet` passes on to
/// `drawing.check`, and needs one drawing when there are several.
#[test]
fn drawing_and_sheet_choose_what_is_checked() {
    let scratch = Scratch::new();
    let mut s = plate();
    let first = drawing(&mut s, "First");
    let second = drawing(&mut s, "Second");
    without_chamfer(&mut s);
    let design = scratch.save(&mut s, "two.solidcraft");
    let all = report(&check(&design, &[]));
    assert_eq!(found(&all).iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["First", "Second"], "{all}");
    let by_name = report(&check(&design, &["--drawing", "Second"]));
    assert_eq!(by_name["drawings"].as_array().map(Vec::len), Some(1), "{by_name}");
    assert_eq!(by_name["drawings"][0]["id"], json!(second), "{by_name}");
    assert_eq!(report(&check(&design, &["--drawing", &first]))["drawings"][0]["name"], json!("First"));
    // A sheet of the drawing: drawing.check echoes it.
    let sheet = all["drawings"][0]["check"]["findings"][0]["sheet"].clone();
    let one_sheet = report(&check(&design, &["--drawing", "First", "--sheet", sheet.as_str().unwrap()]));
    assert_eq!(one_sheet["drawings"][0]["check"]["sheet"], sheet, "{one_sheet}");
    let output = check(&design, &["--sheet", sheet.as_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--sheet needs --drawing"), "{}", String::from_utf8_lossy(&output.stderr));
}

/// What can't be checked is an error with no report: an unknown drawing or sheet, a name two
/// drawings share, an option `check` doesn't take, a missing design.
#[test]
fn what_cannot_be_checked_is_an_error_without_a_report() {
    let scratch = Scratch::new();
    let mut s = plate();
    let ids = [drawing(&mut s, "Same"), drawing(&mut s, "Same")];
    let design = scratch.save(&mut s, "same.solidcraft");
    let missing = scratch.0.join("missing.solidcraft");
    let cases: [(&Path, &[&str], &str); 5] = [
        (&design, &["--drawing", "nowhere"], "no drawing `nowhere`"),
        (&design, &["--drawing", "Same"], &ids[1]),
        (&design, &["--drawing", &ids[0], "--sheet", "nowhere"], "nowhere"),
        (&design, &["--not-an-option"], "--not-an-option"),
        (&missing, &[], "missing.solidcraft"),
    ];
    for (design, args, said) in cases {
        let output = check(design, args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{args:?}: {}", String::from_utf8_lossy(&output.stdout));
        assert!(stderr.starts_with("error: ") && stderr.contains(said), "{args:?}: {stderr}");
    }
}
