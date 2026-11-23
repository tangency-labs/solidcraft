//! `--json-out FILE` writes the JSON `run` and `eval` would print to a file the CLI makes
//! itself. A shell's `>` can re-encode stdout (Windows PowerShell writes UTF-16, or UTF-8 with a
//! byte-order mark), which strict JSON readers refuse; the file is plain UTF-8.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-json-out-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    /// A box, and a parameter whose comment has a character outside ASCII.
    fn script(&self) -> PathBuf {
        let path = self.0.join("box.json");
        let script = json!({"commands": [
            {"command": "parameters.add", "params": {"name": "bore", "expression": "5 mm", "comment": "\u{d8} of the bore"}},
            {"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4}},
        ]});
        std::fs::write(&path, script.to_string()).unwrap();
        path
    }

    /// A filleted box whose fillet a later parameter change makes impossible: every command
    /// succeeds, the timeline ends with a failing feature.
    fn broken_script(&self) -> PathBuf {
        let path = self.0.join("broken.json");
        let script = json!({"commands": [
            {"command": "parameters.add", "params": {"name": "r", "expression": "2 mm"}},
            {"command": "solid.box", "params": {"length": 20, "width": 20, "height": 10}},
            {"command": "solid.fillet", "params": {"edges": [[10, 0, 10]], "radius": "r"}},
            {"command": "parameters.change", "params": {"name": "r", "expression": "50 mm"}},
        ]});
        std::fs::write(&path, script.to_string()).unwrap();
        path
    }

    /// A box and its measure, then a cut from a sketch that does not exist, which fails, and a
    /// step that never runs (#19964).
    fn failing_step_script(&self) -> PathBuf {
        let path = self.0.join("failing-step.json");
        let script = json!([
            {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Box"}},
            {"command": "inspect.measure", "params": {"summary": true}},
            {"command": "solid.extrude", "params": {"sketch": "No such sketch", "distance": 5, "operation": "cut", "targets": ["Box"]}},
            {"command": "inspect.measure", "params": {"summary": true}},
        ]);
        std::fs::write(&path, script.to_string()).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
}

fn stdout_of(output: Output) -> Vec<u8> {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    output.stdout
}

/// Timings differ from one run to the next.
fn without_timings(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.remove("ms");
            map.values_mut().for_each(without_timings);
        }
        Value::Array(items) => items.iter_mut().for_each(without_timings),
        _ => {}
    }
}

/// The file holds what stdout carried in the same command without `--json-out`: the same JSON,
/// laid out the same way, in UTF-8 with no byte-order mark (UTF-8's or UTF-16's), so a strict
/// reader accepts it.
fn assert_plain_utf8_json(file: &Path, stdout: &[u8]) -> Value {
    let bytes = std::fs::read(file).unwrap();
    assert!(matches!(bytes.first(), Some(b'{' | b'[')), "the file starts with the JSON itself, not a mark");
    assert_eq!(bytes.last(), Some(&b'\n'), "like stdout, the file ends with a newline");
    let lines = |b: &[u8]| b.iter().filter(|c| **c == b'\n').count();
    assert_eq!(lines(&bytes), lines(stdout), "laid out like stdout");
    let mut value: Value = serde_json::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
    let mut printed: Value = serde_json::from_slice(stdout).unwrap();
    without_timings(&mut value);
    without_timings(&mut printed);
    assert_eq!(value, printed, "the file is what stdout would have carried");
    value
}

#[test]
fn run_results_json_out_holds_what_stdout_would() {
    let dir = Scratch::new();
    let script = dir.script();
    let file = dir.0.join("results.json");
    let on_stdout = stdout_of(cli().arg("run").arg(&script).arg("--results").output().unwrap());
    let output = cli().arg("run").arg(&script).arg("--results").arg("--json-out").arg(&file).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty(), "the JSON goes to the file, not to stdout");
    let results = assert_plain_utf8_json(&file, &on_stdout);
    assert_eq!(results.as_array().map(Vec::len), Some(2));
}

#[test]
fn run_json_out_without_results_holds_the_measurement_even_when_quiet() {
    let dir = Scratch::new();
    let script = dir.script();
    let on_stdout = stdout_of(cli().arg("run").arg(&script).output().unwrap());
    for (name, extra) in [("plain", &[][..]), ("quiet", &["--quiet"][..])] {
        let file = dir.0.join(format!("measure-{name}.json"));
        let output = cli().arg("run").arg(&script).args(extra).arg("--json-out").arg(&file).output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.is_empty(), "{name}");
        let measure = assert_plain_utf8_json(&file, &on_stdout);
        assert_eq!(measure["body_count"], 1, "{name}");
    }
}

#[test]
fn eval_json_out_holds_what_stdout_would() {
    let dir = Scratch::new();
    let script = dir.script();
    let on_stdout = stdout_of(cli().arg("eval").arg(&script).output().unwrap());
    // The option may come before the file to evaluate, and `inspect` is the same command.
    for name in ["eval", "inspect"] {
        let file = dir.0.join(format!("{name}.json"));
        let output = cli().arg(name).arg("--json-out").arg(&file).arg(&script).output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.is_empty(), "{name}");
        let measured = assert_plain_utf8_json(&file, &on_stdout);
        assert_eq!(measured["body_count"], 1, "{name}");
        // A character outside ASCII is written as it is, in UTF-8.
        assert!(std::fs::read_to_string(&file).unwrap().contains("\u{d8} of the bore"), "{name}");
    }
}

#[test]
fn json_out_needs_a_file_and_fails_when_it_cannot_write() {
    let dir = Scratch::new();
    let script = dir.script();
    for cmd in ["run", "eval", "inspect"] {
        for trailing in [&[][..], &["--in"][..]] {
            let output = cli().arg(cmd).arg(&script).arg("--json-out").args(trailing).output().unwrap();
            assert_eq!(output.status.code(), Some(1), "{cmd}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("--json-out needs a FILE"), "{cmd}");
            assert!(output.stdout.is_empty(), "{cmd}");
        }

        let missing = dir.0.join("missing").join("out.json");
        let output = cli().arg(cmd).arg(&script).arg("--json-out").arg(&missing).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{cmd}");
        assert!(!output.stderr.is_empty(), "{cmd}");
        assert!(output.stdout.is_empty(), "{cmd}");
        assert!(!missing.exists(), "{cmd}");
    }
}

#[test]
fn run_json_out_is_not_written_when_a_feature_fails() {
    let dir = Scratch::new();
    let script = dir.broken_script();
    for (name, extra) in [("plain", &[][..]), ("results", &["--results"][..])] {
        let file = dir.0.join(format!("failed-{name}.json"));
        let output = cli().arg("run").arg(&script).args(extra).arg("--json-out").arg(&file).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("nothing was written"), "{name}");
        assert!(!file.exists(), "{name}");
    }
}

/// #19964: a step that fails still hands over the results of the steps before it, with its
/// error, on stdout or in `--json-out`, with `--allow-errors` too; the run fails, and writes no
/// `--out`.
#[test]
fn run_results_keep_the_steps_before_a_failing_step_and_its_error() {
    let dir = Scratch::new();
    let script = dir.failing_step_script();
    let on_stdout = cli().arg("run").arg(&script).arg("--results").output().unwrap();
    assert_eq!(on_stdout.status.code(), Some(1));
    let mut printed: Value = serde_json::from_slice(&on_stdout.stdout).unwrap();
    without_timings(&mut printed);
    for (name, extra) in [("plain", &[][..]), ("allow-errors", &["--allow-errors"][..])] {
        let file = dir.0.join(format!("results-{name}.json"));
        let part = dir.0.join(format!("part-{name}.stl"));
        let output =
            cli().arg("run").arg(&script).arg("--results").args(extra).arg("--out").arg(&part).arg("--json-out").arg(&file).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{name}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("step 3 (solid.extrude)"), "{name}: {stderr}");
        assert!(output.stdout.is_empty(), "{name}: the JSON goes to the file");
        assert!(!part.exists(), "{name}: no part is written from a script that stopped");
        let mut written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        without_timings(&mut written);
        assert_eq!(written, printed, "{name}: the file is what stdout carried");
        let results = written["results"].as_array().unwrap();
        assert_eq!(results.len(), 2, "{name}: the two steps before the failure");
        assert_eq!(results[1]["body_count"], 1, "{name}: the measure before the failure");
        assert_eq!(written["error"]["step"], 3, "{name}");
        assert_eq!(written["error"]["command"], "solid.extrude", "{name}");
        assert!(written["error"]["error"].as_str().is_some_and(|e| !e.is_empty()), "{name}");
    }
}

/// With `--timings`, the failing step's time is kept beside the steps before it, and the error
/// is added beside the timings; a step that names no command has no `command`.
#[test]
fn run_timings_keep_the_failing_step_and_a_step_without_a_command() {
    let dir = Scratch::new();
    let script = dir.failing_step_script();
    let output = cli().arg("run").arg(&script).arg("--timings").arg("--results").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["timings"].as_array().map(Vec::len), Some(3), "the failing step is timed too");
    assert_eq!(printed["results"].as_array().map(Vec::len), Some(2));
    assert_eq!(printed["error"]["step"], 3);

    let nameless = dir.0.join("nameless.json");
    std::fs::write(&nameless, json!([{"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}, {"params": {}}]).to_string())
        .unwrap();
    let output = cli().arg("run").arg(&nameless).arg("--results").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("script step 2: missing `command`"));
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(printed["error"], json!({"step": 2, "error": "missing `command`"}));
}

/// A script whose steps all work prints its results as a list, as before.
#[test]
fn run_results_of_a_script_that_works_stay_a_list() {
    let dir = Scratch::new();
    let printed: Value = serde_json::from_slice(&stdout_of(cli().arg("run").arg(dir.script()).arg("--results").output().unwrap())).unwrap();
    assert!(printed.is_array());
}
