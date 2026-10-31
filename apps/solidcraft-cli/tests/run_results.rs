//! Result reporting must not skip requested files or execute a script twice.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-run-results-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn script(&self) -> PathBuf {
        let path = self.0.join("box.json");
        std::fs::write(&path, json!({"commands": [{"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4}}]}).to_string()).unwrap();
        path
    }

    fn saving_script(&self) -> (PathBuf, PathBuf) {
        let path = self.0.join("saving.json");
        let saved = self.0.join("script-ran.solidcraft");
        std::fs::write(
            &path,
            json!({"commands": [{"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4}},
                {"command": "file.save_as", "params": {"path": saved}}]})
            .to_string(),
        )
        .unwrap();
        (path, saved)
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

fn json_output(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn body_count(path: &Path) -> Value {
    json_output(cli().arg("eval").arg(path).output().unwrap())["body_count"].clone()
}

#[test]
fn run_rejects_invalid_options_before_script_execution_or_output_writes() {
    let dir = Scratch::new();
    let (script, script_saved) = dir.saving_script();
    let cases = [
        (&["--out"][..], "--out needs a value"),
        (&["--save"][..], "--save needs a value"),
        (&["--in"][..], "--in needs a value"),
        (&["--out", "--quiet"][..], "--out needs a value"),
        (&["--ouut", "part.step"][..], "doesn't take --ouut"),
        (&["--unknown"][..], "doesn't take --unknown"),
    ];
    for existing in [false, true] {
        let saved = dir.0.join(format!("{existing}.solidcraft"));
        let exported = dir.0.join(format!("{existing}.step"));
        let report = dir.0.join(format!("{existing}.json"));
        let outputs = [&saved, &exported, &report];
        if existing {
            for path in outputs {
                std::fs::write(path, b"existing output").unwrap();
            }
        }
        for (extra, message) in cases {
            let output = cli()
                .arg("run")
                .arg(&script)
                .arg("--save")
                .arg(&saved)
                .arg("--out")
                .arg(&exported)
                .arg("--json-out")
                .arg(&report)
                .args(extra)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{extra:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains(message), "{extra:?}: {}", String::from_utf8_lossy(&output.stderr));
            assert!(output.stdout.is_empty());
            assert!(!script_saved.exists(), "invalid arguments must be rejected before the script runs");
            for path in outputs {
                if existing {
                    assert_eq!(std::fs::read(path).unwrap(), b"existing output");
                } else {
                    assert!(!path.exists());
                }
            }
        }
    }
}

#[test]
fn eval_and_inspect_reject_invalid_options_before_script_execution_or_reporting() {
    let dir = Scratch::new();
    let (script, script_saved) = dir.saving_script();
    let cases = [
        (&["--in"][..], "--in needs a value"),
        (&["--in", "--quiet"][..], "--in needs a value"),
        (&["--out", "part.step"][..], "doesn't take --out"),
        (&["--unknown"][..], "doesn't take --unknown"),
    ];
    for command in ["eval", "inspect"] {
        for existing in [false, true] {
            let report = dir.0.join(format!("{command}-{existing}.json"));
            if existing {
                std::fs::write(&report, b"existing report").unwrap();
            }
            for (extra, message) in cases {
                let output = cli().arg(command).arg(&script).arg("--json-out").arg(&report).args(extra).output().unwrap();
                assert_eq!(output.status.code(), Some(1), "{command} {extra:?}");
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains(message),
                    "{command} {extra:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(output.stdout.is_empty());
                assert!(!script_saved.exists(), "invalid arguments must be rejected before the script runs");
                if existing {
                    assert_eq!(std::fs::read(&report).unwrap(), b"existing report");
                } else {
                    assert!(!report.exists());
                }
            }
        }
    }
}

#[test]
fn cli_loads_both_design_encodings_by_contents() {
    let dir = Scratch::new();
    for extension in ["sc", "solidcraft", "solvecraft", "json"] {
        let saved = dir.0.join(format!("box.{extension}"));
        json_output(cli().arg("run").arg(dir.script()).arg("--save").arg(&saved).output().unwrap());
        let bytes = std::fs::read(&saved).unwrap();
        assert_eq!(solidcraft_engine::io::is_design_container(&bytes), extension != "json");
        let renamed = dir.0.join(format!("{extension}.unusual"));
        std::fs::write(&renamed, bytes).unwrap();
        let measured = json_output(cli().arg("eval").arg(&renamed).output().unwrap());
        assert_eq!(measured["body_count"], 1);
        assert!((measured["bodies"][0]["volume_mm3"].as_f64().unwrap() - 24.0).abs() < 1e-6);
    }
}

/// #1369: a pending concept that did not land in the numbered release must not be silently
/// dropped on open/resave, in either encoding or before replacing an existing destination.
#[test]
fn unknown_pending_concepts_preserve_the_open_design_and_output_files() {
    use solidcraft_engine::{Session, doc, io};

    let dir = Scratch::new();
    for version in [33, doc::format::FORMAT] {
        let mut experimental = doc::Document::new("Experimental");
        experimental.format = doc::format::format_string(version);
        experimental.format_pending.push("experimental-card".into());
        for (extension, bytes) in [("json", io::write_design(&experimental)), ("solidcraft", io::write_design_container(&experimental).unwrap())] {
            let source = dir.0.join(format!("experimental-{version}.{extension}"));
            std::fs::write(&source, &bytes).unwrap();
            let mut session = Session::default();
            session.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4})).unwrap();
            let original = session.doc.clone();
            let original_path = session.path.clone();
            let undo = session.undo.len();
            let dirty = session.is_dirty();
            let err = session.execute("doc.open", &json!({"path": source})).unwrap_err().to_string();
            assert!(err.contains("experimental-card") && err.contains("update SolidCraft"), "{err}");
            assert_eq!(session.doc, original);
            assert_eq!(session.path, original_path);
            assert_eq!(session.undo.len(), undo);
            assert_eq!(session.is_dirty(), dirty);

            for existing in [false, true] {
                let destination = dir.0.join(format!("output-{version}-{extension}-{existing}.{extension}"));
                let backup = io::backup_path(&destination);
                if existing {
                    std::fs::write(&destination, b"existing design").unwrap();
                    std::fs::write(&backup, b"existing backup").unwrap();
                }
                let output = cli().args(["run", "--in"]).arg(&source).arg("--save").arg(&destination).output().unwrap();
                assert_eq!(output.status.code(), Some(1));
                let error = String::from_utf8_lossy(&output.stderr);
                assert!(error.contains("experimental-card") && error.contains("update SolidCraft"), "{error}");
                if existing {
                    assert_eq!(std::fs::read(&destination).unwrap(), b"existing design");
                    assert_eq!(std::fs::read(&backup).unwrap(), b"existing backup");
                } else {
                    assert!(!destination.exists() && !backup.exists());
                }
                assert_eq!(std::fs::read(&source).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn known_pending_concepts_and_released_files_round_trip_with_inert_metadata() {
    use solidcraft_engine::{doc, io};

    let dir = Scratch::new();
    let current = doc::Document::new("Supported");
    let mut released = current.clone();
    released.format = doc::format::format_string(33);
    released.format_pending.clear();
    let mut experimental = released.clone();
    experimental.format_pending.push("reserved-body-names".into());
    for (index, mut document) in [current, released, experimental].into_iter().enumerate() {
        document.metadata.0.insert("com.example.tool".into(), json!({"format_pending": ["experimental-card"], "physical_card": {"density": 700}}));
        for (extension, bytes) in [("json", io::write_design(&document)), ("solidcraft", io::write_design_container(&document).unwrap())] {
            let source = dir.0.join(format!("supported-{index}.{extension}"));
            let destination = dir.0.join(format!("resaved-{index}.{extension}"));
            std::fs::write(&source, &bytes).unwrap();
            let output = cli().args(["run", "--in"]).arg(&source).arg("--save").arg(&destination).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let reopened = io::read_design(&std::fs::read(&destination).unwrap()).unwrap();
            assert_eq!(reopened.metadata, document.metadata);
            assert_eq!(reopened.name, document.name);
            assert_eq!(reopened.features, document.features);
            assert_eq!(std::fs::read(&source).unwrap(), bytes);
        }
    }
}

#[test]
fn results_still_save_and_export_the_script_once() {
    let dir = Scratch::new();
    // JSON is explicit here because the fixture edits its saved format tag.
    let saved = dir.0.join("box.json");
    let stl = dir.0.join("box.stl");
    let obj = dir.0.join("box.obj");
    let result = json_output(
        cli().arg("run").arg(dir.script()).arg("--results").arg("--save").arg(&saved).arg("--out").arg(&stl).arg("--out").arg(&obj).output().unwrap(),
    );
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(body_count(&saved), 1);
    for exported in [&stl, &obj] {
        assert!(std::fs::metadata(exported).unwrap().len() > 0);
        assert_eq!(body_count(exported), 1);
    }
}

#[test]
fn results_with_base_design_and_quiet_still_report_and_save() {
    let dir = Scratch::new();
    let script = dir.script();
    let base = dir.0.join("base.solidcraft");
    let saved = dir.0.join("combined.solidcraft");
    json_output(cli().arg("run").arg(&script).arg("--save").arg(&base).output().unwrap());
    let result = json_output(cli().args(["run", "--results", "--quiet", "--in"]).arg(&base).arg(&script).arg("--save").arg(&saved).output().unwrap());
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(body_count(&saved), 2);
    assert_eq!(body_count(&base), 1);
}

/// `eval` and its alias must measure the script applied to the input design, rather than
/// silently reporting the input alone (#6861).
#[test]
fn eval_runs_its_script_on_the_in_design() {
    let dir = Scratch::new();
    let base = dir.0.join("base.solidcraft");
    let script = dir.0.join("add.json");
    json_output(cli().arg("run").arg(dir.script()).arg("--save").arg(&base).output().unwrap());
    let original = std::fs::read(&base).unwrap();
    std::fs::write(
        &script,
        json!({"commands": [{"command": "solid.box", "params": {"length": 5, "width": 3, "height": 4, "corner": [10, 0, 0]}}]}).to_string(),
    )
    .unwrap();
    for command in ["eval", "inspect"] {
        // Both existing one-input forms still measure just the original body.
        for input in [&base, &dir.script()] {
            for named_input in [false, true] {
                let mut c = cli();
                c.arg(command);
                if named_input {
                    c.arg("--in");
                }
                let measured = json_output(c.arg(input).output().unwrap());
                assert_eq!(measured["body_count"], 1);
                assert!((measured["total"]["volume_mm3"].as_f64().unwrap() - 24.0).abs() < 1e-6);
            }
        }
        for input_first in [false, true] {
            for to_file in [false, true] {
                let mut c = cli();
                c.arg(command);
                if input_first {
                    c.arg("--in").arg(&base).arg(&script);
                } else {
                    c.arg(&script).arg("--in").arg(&base);
                }
                let report = dir.0.join("measurement.json");
                if to_file {
                    c.arg("--json-out").arg(&report);
                }
                let output = c.output().unwrap();
                let measured = if to_file {
                    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                    assert!(output.stdout.is_empty());
                    serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap()
                } else {
                    json_output(output)
                };
                assert_eq!(measured["body_count"], 2, "{command}, input_first={input_first}, to_file={to_file}");
                assert!((measured["total"]["volume_mm3"].as_f64().unwrap() - 84.0).abs() < 1e-6);
                assert_eq!(measured["timeline"].as_array().unwrap().len(), 2, "the script runs exactly once");
                assert_eq!(std::fs::read(&base).unwrap(), original, "evaluation never saves its changes to the input");
            }
        }
    }
}

#[test]
fn eval_propagates_script_errors_before_writing_a_report() {
    let dir = Scratch::new();
    let base = dir.script();
    let script = dir.0.join("bad.json");
    for command in ["eval", "inspect"] {
        for contents in ["{", r#"{"commands":[{"command":"missing.command","params":{}}]}"#] {
            std::fs::write(&script, contents).unwrap();
            let report = dir.0.join("report.json");
            let output = cli().arg(command).arg(&script).arg("--in").arg(&base).arg("--json-out").arg(&report).output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(!output.stderr.is_empty());
            assert!(output.stdout.is_empty());
            assert!(!report.exists());
        }
    }
}

#[test]
fn eval_rejects_unknown_options_and_missing_input_values_before_writing_a_report() {
    let dir = Scratch::new();
    let script = dir.script();
    let report = dir.0.join("report.json");
    for command in ["eval", "inspect"] {
        for (options, message) in [(&["--frobnicate", "1"][..], "doesn't take --frobnicate"), (&["--in"][..], "--in needs a value")] {
            for options_first in [false, true] {
                std::fs::write(&report, b"previous report").unwrap();
                let mut c = cli();
                c.arg(command);
                if options_first {
                    c.args(options).arg("--json-out").arg(&report).arg(&script);
                } else {
                    c.arg(&script).args(options).arg("--json-out").arg(&report);
                }
                // A following option also proves that --in cannot consume a flag as its value.
                let output = c.output().unwrap();
                assert_eq!(output.status.code(), Some(1), "{command}, {options:?}");
                let error = String::from_utf8_lossy(&output.stderr);
                assert!(error.contains(message), "{command}: {error}");
                assert!(output.stdout.is_empty());
                assert_eq!(std::fs::read(&report).unwrap(), b"previous report");
            }
        }
    }
}

#[test]
fn results_propagate_output_errors() {
    let dir = Scratch::new();
    for flag in ["--save", "--out"] {
        let path = dir.0.join("missing").join(if flag == "--save" { "box.solidcraft" } else { "box.stl" });
        let output = cli().arg("run").arg(dir.script()).arg("--results").arg(flag).arg(&path).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(!output.stderr.is_empty());
        assert!(output.stdout.is_empty());
        assert!(!path.exists());
    }
}

#[test]
fn results_without_script_still_fail() {
    let dir = Scratch::new();
    let base = dir.0.join("base.solidcraft");
    json_output(cli().arg("run").arg(dir.script()).arg("--save").arg(&base).output().unwrap());
    let output = cli().args(["run", "--results", "--in"]).arg(&base).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--results needs a script"));
}

/// Compatibility shim (docs/compatibility.md): a design saved before the rename, with the
/// legacy extension and format tag, still opens; re-saving writes the current tag.
#[test]
fn legacy_extension_and_format_tag_still_open() {
    let dir = Scratch::new();
    // JSON is explicit here because the fixture edits its saved format tag.
    let saved = dir.0.join("box.json");
    json_output(cli().arg("run").arg(dir.script()).arg("--save").arg(&saved).output().unwrap());
    let text = std::fs::read_to_string(&saved).unwrap();
    assert!(text.contains("\"solidcraft/"), "new designs carry the current tag");
    let legacy = dir.0.join("old.solvecraft");
    std::fs::write(&legacy, text.replace("\"solidcraft/", "\"solvecraft/")).unwrap();
    assert_eq!(body_count(&legacy), 1);
    let resaved = dir.0.join("resaved.solidcraft");
    json_output(cli().args(["run", "--in"]).arg(&legacy).arg(dir.script()).arg("--save").arg(&resaved).output().unwrap());
    assert_eq!(body_count(&resaved), 2);
    let reopened = solidcraft_engine::io::read_design(&std::fs::read(&resaved).unwrap()).unwrap();
    assert!(reopened.to_json().contains("\"solidcraft/"));
    // `.solidcraft` (before `.sc`) opens as `--in` and as the design to evaluate; `.sc` too.
    let current = dir.0.join("current.sc");
    json_output(cli().args(["run", "--in"]).arg(&resaved).arg(dir.script()).arg("--save").arg(&current).output().unwrap());
    assert_eq!(body_count(&current), 3);
    assert!(solidcraft_engine::io::is_design_container(&std::fs::read(&current).unwrap()), "a .sc file is a design container");
}

/// `snapshot` starts from `--in` as `run` does (its script runs on that design), and refuses an
/// option it doesn't take rather than ignoring it.
#[test]
fn snapshot_runs_its_script_on_the_in_design() {
    let dir = Scratch::new();
    let base = dir.0.join("base.solidcraft");
    json_output(cli().arg("run").arg(dir.script()).arg("--save").arg(&base).output().unwrap());
    let shifted = dir.0.join("shifted.json");
    std::fs::write(
        &shifted,
        json!({"commands": [{"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4, "corner": [10, 0, 0]}}]}).to_string(),
    )
    .unwrap();
    let shot = |name: &str, args: &[&std::ffi::OsStr]| {
        let png = dir.0.join(name);
        json_output(cli().arg("snapshot").args(args).arg("--out").arg(&png).args(["--width", "64", "--height", "48"]).output().unwrap());
        std::fs::read(&png).unwrap()
    };
    // Pictures are drawn on the GPU: without an adapter only the option checks below run.
    match solidcraft_gpu::capture::shared() {
        Ok(_) => {
            let alone = shot("alone.png", &[shifted.as_os_str()]);
            let on_base = shot("on_base.png", &[shifted.as_os_str(), "--in".as_ref(), base.as_os_str()]);
            let base_only = shot("base.png", &["--in".as_ref(), base.as_os_str()]);
            assert_ne!(alone, on_base, "the script ran on the --in design");
            assert_ne!(base_only, on_base);
        }
        Err(e @ solidcraft_gpu::GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("pictures left out: {e}")
        }
        Err(e) => panic!("{e}"),
    }
    for (bad, says) in [(["--frobnicate", "1"], "doesn't take --frobnicate"), (["--width", "wide"], "--width must be 16 to 8192")] {
        let output = cli().arg("snapshot").arg(&base).arg("--out").arg(dir.0.join("x.png")).args(bad).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains(says), "{}", String::from_utf8_lossy(&output.stderr));
    }
}

/// An unknown command's error suggests the id that was meant.
#[test]
fn unknown_command_suggests_the_closest_id() {
    let output = cli().args(["exec", "file.open"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("did you mean `doc.open` or `macro.open`?"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// #1833: `--timings` reports each command's time instead of the measurement, `--profile` adds
/// the kernel's share, `--results` puts the results beside them, and files are still written.
#[test]
fn timings_and_profile_report_each_command() {
    let dir = Scratch::new();
    let saved = dir.0.join("timed.solidcraft");
    let t = json_output(cli().arg("run").arg(dir.script()).arg("--timings").arg("--save").arg(&saved).output().unwrap());
    let rows = t["timings"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["index"].as_u64(), rows[0]["command"].as_str()), (Some(0), Some("solid.box")));
    assert!(rows[0]["ms"].as_f64().is_some() && t["total_ms"].as_f64().is_some(), "{t}");
    assert!(rows[0].get("profile").is_none() && t.get("results").is_none(), "{t}");
    assert_eq!(body_count(&saved), 1);

    // --profile alone implies --timings; a flag before the script doesn't eat it.
    let t = json_output(cli().arg("run").arg("--profile").arg(dir.script()).arg("--results").output().unwrap());
    let p = &t["timings"][0]["profile"];
    for k in ["boolean_ms", "tessellate_ms", "other_ms"] {
        assert!(p[k].as_f64().is_some_and(|ms| ms >= 0.0), "{k}: {t}");
    }
    assert_eq!(t["results"].as_array().map(Vec::len), Some(1), "{t}");

    // --results alone stays a plain array.
    assert!(json_output(cli().arg("run").arg(dir.script()).arg("--results").output().unwrap()).is_array());

    let output = cli().arg("run").arg("--in").arg(&saved).arg("--timings").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--timings needs a script"));
}

/// #6921: `--param-check` checks a script's steps against their contracts on every path `run`
/// takes (a script alone, on an `--in` design or script, with `--results`, `--timings` or
/// `--profile`), and a warning reaches stderr even when a later step fails.
#[test]
fn param_check_reaches_every_script_run_takes() {
    let dir = Scratch::new();
    let typo = dir.0.join("typo.json");
    let step = json!({"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4, "operaton": "new"}});
    std::fs::write(&typo, json!({"commands": [step.clone()]}).to_string()).unwrap();
    let base = dir.0.join("base.solidcraft");
    json_output(cli().arg("run").arg(dir.script()).arg("--save").arg(&base).output().unwrap());
    let saved = dir.0.join("saved.solidcraft");
    let start = dir.script();
    let (typo, base, saved_arg, start) = (typo.to_str().unwrap(), base.to_str().unwrap(), saved.to_str().unwrap(), start.to_str().unwrap());
    let paths = [&[][..], &["--in", base], &["--in", start], &["--results"], &["--timings"], &["--timings", "--profile"]];
    let run = |level: Option<&str>, extra: &[&str]| {
        std::fs::remove_file(&saved).ok();
        let check = level.map(|l| ["--param-check", l]).into_iter().flatten();
        cli().args(["run", typo, "--save", saved_arg]).args(check).args(extra).output().unwrap()
    };

    // Off, the default: the step runs as it always has, without a word.
    let off = run(None, &[]);
    assert!(off.status.success() && !String::from_utf8_lossy(&off.stderr).contains("operaton"), "{off:?}");
    assert_eq!(body_count(&saved), 1);

    // Warn: each path runs the step, saves, and reports it on stderr; `--results` keeps it too.
    for extra in paths {
        let output = run(Some("warn"), extra);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{extra:?}: {stderr}");
        assert!(stderr.contains("step 1: unknown key `operaton`; did you mean `operation`?"), "{extra:?}: {stderr}");
        assert!(saved.exists(), "{extra:?}");
        if extra == ["--results"] {
            let results: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(results[0]["param_warnings"][0].as_str().is_some_and(|w| w.contains("operaton")), "{results}");
        }
    }

    // Strict: on each path the run stops at the step, and nothing is written.
    for extra in paths {
        let output = run(Some("strict"), extra);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{extra:?}: {stderr}");
        assert!(stderr.contains("unknown key `operaton`"), "{extra:?}: {stderr}");
        assert!(!saved.exists(), "{extra:?}");
    }

    // Warn, with a later step that fails: the run fails, and the first step's warning is still
    // reported, timed or not.
    let failing = dir.0.join("failing.json");
    let fail = json!({"command": "solid.extrude", "params": {"sketch": "Nope"}});
    std::fs::write(&failing, json!({"commands": [step, fail]}).to_string()).unwrap();
    for extra in [&[][..], &["--timings"]] {
        let output = cli().arg("run").arg(&failing).args(["--param-check", "warn"]).args(extra).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{extra:?}: {stderr}");
        assert!(stderr.contains("step 1: unknown key `operaton`") && stderr.contains("no sketch `Nope`"), "{extra:?}: {stderr}");
    }

    // A level it doesn't know and a missing value fail.
    for (bad, says) in [
        (&["--param-check", "loud"][..], "--param-check wants off, warn or strict, not `loud`"),
        (&["--param-check"], "run: --param-check wants off, warn or strict"),
    ] {
        let output = cli().args(["run", typo]).args(bad).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{bad:?}: {stderr}");
        assert!(stderr.contains(says), "{bad:?}: {stderr}");
    }
}

fn render_setup_script(dir: &Scratch, job: Value) -> PathBuf {
    let path = dir.0.join("render.json");
    std::fs::write(
        &path,
        json!({"commands":[
            {"command":"solid.box","params":{"length":10,"width":10,"height":10}},
            {"command":"render.settings.set","params":{"name":"Portable setup","set":{"width":16,"height":16,"job":job}}}
        ]})
        .to_string(),
    )
    .unwrap();
    path
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn render_uses_typed_cpu_setups_from_scripts_and_saved_designs() {
    for options in [json!({}), json!({"progressive":{"samples_per_pixel":3,"seed":81},"lights":[]})] {
        let dir = Scratch::new();
        let script = render_setup_script(&dir, json!({"method":"cpu_raytrace","options":options}));
        let image = dir.0.join("script.png");
        let result = json_output(
            cli()
                .arg("render")
                .arg(&script)
                .args(["--setup", "Portable setup", "--out"])
                .arg(&image)
                .env("SOLIDCRAFT_GPU", "must-not-be-read-for-cpu")
                .output()
                .unwrap(),
        );
        assert_eq!(result["preflight"]["resolved"]["executor"], "cpu-raytrace");
        assert_eq!(result["export"]["frame"]["executor"], "cpu-raytrace");
        assert_eq!(result["preflight"]["request"]["integrator"], "raytrace");
        assert_eq!(result["preflight"]["request"]["raytrace"].get("progressive"), options.get("progressive"));
        let png = image::load_from_memory(&std::fs::read(&image).unwrap()).unwrap();
        assert_eq!((png.width(), png.height()), (16, 16));

        let saved = dir.0.join("portable.solidcraft");
        json_output(cli().arg("run").arg(&script).arg("--save").arg(&saved).output().unwrap());
        let original = std::fs::read(&saved).unwrap();
        let overridden = dir.0.join("overridden.png");
        let result = json_output(
            cli()
                .arg("render")
                .arg(&saved)
                .args(["--setup", "Portable setup", "--out"])
                .arg(&overridden)
                .args(["--width", "12", "--height", "8", "--exposure", "1.25"])
                .env("SOLIDCRAFT_GPU", "must-not-be-read-for-cpu")
                .output()
                .unwrap(),
        );
        assert_eq!(result["preflight"]["request"]["width"], 12);
        assert_eq!(result["preflight"]["request"]["height"], 8);
        assert_eq!(result["preflight"]["request"]["raytrace"]["exposure_ev"], 1.25);
        assert_eq!(result["export"]["frame"]["executor"], "cpu-raytrace");
        assert_eq!(result["preflight"]["request"]["raytrace"].get("progressive"), options.get("progressive"));
        let png = image::load_from_memory(&std::fs::read(overridden).unwrap()).unwrap();
        assert_eq!((png.width(), png.height()), (12, 8));
        assert_eq!(std::fs::read(saved).unwrap(), original, "CLI overrides must not rewrite saved intent");
    }
}

#[test]
fn render_preserves_typed_raster_and_legacy_runtime_controls() {
    for job in [Value::Null, json!({"method":"raster","transparent_background":true})] {
        let dir = Scratch::new();
        let script = render_setup_script(&dir, job.clone());
        let out = dir.0.join("raster.png");
        let mut command = cli();
        command
            .arg("render")
            .arg(&script)
            .args(["--setup", "Portable setup", "--out"])
            .arg(&out)
            .env("SOLIDCRAFT_GPU", "must-not-be-read-for-raster");
        if job.is_null() {
            command.arg("--transparent");
        }
        let result = json_output(command.output().unwrap());
        assert_eq!(result["preflight"]["resolved"]["executor"], "cpu-product");
        assert_eq!(result["export"]["frame"]["executor"], "cpu-product");
        assert_eq!(result["preflight"]["request"]["transparent_background"], true);
        assert!(std::fs::read(out).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}

#[test]
fn typed_render_rejects_explicit_runtime_flags_before_gpu_or_option_file_reads() {
    let dir = Scratch::new();
    let script = render_setup_script(&dir, json!({"method":"gpu_path","options":{"samples":64,"seed":0}}));
    let out = dir.0.join("existing.png");
    std::fs::write(&out, b"previous completed image").unwrap();
    for flags in [
        &["--progressive"][..],
        &["--transparent"][..],
        &["--samples", "64"][..],
        &["--max-depth", "8"][..],
        &["--seed", "0"][..],
        &["--lighting", "deliberately-missing-lighting.json"][..],
        &["--tone-map", "reinhard"][..],
        &["--environment", "deliberately-missing-environment.json"][..],
    ] {
        let output = cli()
            .arg("render")
            .arg(&script)
            .args(["--setup", "Portable setup", "--out"])
            .arg(&out)
            .arg("--overwrite")
            .args(flags)
            .env("SOLIDCRAFT_GPU", "must-not-be-read-for-conflicts")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(&format!("{} cannot accompany a typed saved render setup", flags[0])), "{error}");
        assert!(output.stdout.is_empty());
        assert_eq!(std::fs::read(&out).unwrap(), b"previous completed image");
    }
}

#[test]
fn typed_gpu_render_selects_its_host_without_a_progressive_runtime_flag() {
    // A deliberately invalid policy proves that dispatch reached the GPU host
    // without depending on an adapter or accepting a CPU fallback. The legacy
    // --progressive path must still reach that same host-selection boundary.
    for job in [json!({"method":"gpu_path","options":{"samples":2}}), Value::Null] {
        let dir = Scratch::new();
        let script = render_setup_script(&dir, job.clone());
        let out = dir.0.join("gpu.png");
        let mut command = cli();
        command.arg("render").arg(&script).args(["--setup", "Portable setup", "--out"]).arg(&out).env("SOLIDCRAFT_GPU", "not-a-gpu-policy");
        if job.is_null() {
            command.arg("--progressive");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("SOLIDCRAFT_GPU must be auto, hardware, software or cpu"), "{error}");
        assert!(!out.exists());
    }
}

#[test]
fn render_setup_overrides_obey_shared_validation_before_host_initialization() {
    let dir = Scratch::new();
    let script = render_setup_script(&dir, json!({"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":64}}}));
    let out = dir.0.join("invalid.png");
    // 1024 * 1024 * 64 exceeds the unchanged default Compact camera-sample
    // budget. Admission must name that envelope before initializing any host.
    for (flags, message) in [
        (&["--width", "1024", "--height", "1024"][..], "CPU Compact exceeds its 16777216 camera-sample admission"),
        (&["--width", "0"][..], "render output must be at least"),
        (&["--exposure", "NaN"][..], "render exposure must be finite"),
    ] {
        let output = cli()
            .arg("render")
            .arg(&script)
            .args(["--setup", "Portable setup", "--out"])
            .arg(&out)
            .args(flags)
            .env("SOLIDCRAFT_GPU", "must-not-be-read-for-invalid-settings")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(message), "{error}");
        assert!(!out.exists());
    }
}
