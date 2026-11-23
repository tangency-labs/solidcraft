//! Designs kept in Git (#626). `solidcraft-cli textconv` prints a design file as text for
//! `git diff`, and it never stops the diff for a file it can read. `solidcraft-cli diff A B` says
//! what changed from one design file to another, with exit code 0 for the same design, 1 when they
//! differ and 2 on an error.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::Document;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A folder of its own for one test, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("solidcraft-designs-in-git-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn at(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn textconv(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("textconv").args(args).output().unwrap()
}

fn says(output: &Output) -> String {
    format!("status {}, stdout {}, stderr {}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

/// A `.json` design prints as it was saved, with a final line feed. A `.sc` container of the same
/// design prints its `container.json` and its design as text. Neither writes to stderr, because git
/// would show that in the middle of the diff.
#[test]
fn textconv_prints_a_json_design_as_saved_and_a_container_as_text() {
    let dir = Scratch::new("text");
    let doc = Document::new("Plate");
    let saved = solidcraft_engine::io::write_design(&doc).unwrap();
    let output = textconv(&[&dir.file("plate.json", &saved)]);
    assert!(output.status.success() && output.stderr.is_empty(), "{}", says(&output));
    assert_eq!(output.stdout, [saved, b"\n".to_vec()].concat());

    let container = solidcraft_engine::io::write_design_container(&doc).unwrap();
    let output = textconv(&[&dir.file("plate.sc", &container)]);
    assert!(output.status.success() && output.stderr.is_empty(), "{}", says(&output));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("# SolidCraft container {") && text.contains("\n# document.json\n{\n"), "{text}");
}

/// A file textconv can read but not show prints one line with its size and SHA-256 and exits 0, so
/// `git diff` goes on. That covers a file that is no design, a damaged container and a file over
/// the design file limit. Two files over the limit with one size but different bytes give different
/// lines, so that change still shows. A file that is not there fails with 1.
#[test]
fn textconv_never_stops_git_diff_for_a_file_it_can_read() {
    let dir = Scratch::new("unreadable");
    for (name, bytes) in [("notes.txt", &b"not a design"[..]), ("broken.sc", &b"PK\x03\x04broken"[..])] {
        let output = textconv(&[&dir.file(name, bytes)]);
        assert!(output.status.success() && output.stderr.is_empty(), "{name}: {}", says(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.lines().count() == 1 && stdout.contains("SHA-256 "), "{name}: {}", says(&output));
    }

    // Two sparse files just over the default 256 MB limit, the second with one byte set.
    let size = (256u64 << 20) + 1;
    let mut lines = Vec::new();
    for (name, set) in [("big.sc", false), ("big-changed.sc", true)] {
        let path = dir.0.join(name);
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(size).unwrap();
        if set {
            use std::io::{Seek, SeekFrom, Write};
            let mut file = file;
            file.seek(SeekFrom::Start(size / 2)).unwrap();
            file.write_all(b"x").unwrap();
        }
        let output = textconv(&[&path.to_string_lossy()]);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success() && output.stderr.is_empty() && stdout.lines().count() == 1, "{}", says(&output));
        assert!(stdout.contains("too large") && stdout.contains(&format!("({size} bytes, SHA-256 ")), "{stdout}");
        lines.push(stdout);
    }
    assert_ne!(lines[0], lines[1], "the changed byte shows");

    let output = textconv(&[&dir.0.join("missing.sc").to_string_lossy()]);
    assert_eq!(output.status.code(), Some(1), "{}", says(&output));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error: "), "{}", says(&output));
}

/// textconv takes exactly one file. That one word is always a path, never an option, because git
/// passes a working tree file by its own name, which may start with `--`.
#[test]
fn textconv_takes_one_file_whatever_its_name() {
    for args in [&[][..], &["a.sc", "b.sc"][..], &["--json", "a.sc"][..]] {
        let output = textconv(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {}", says(&output));
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("error: "), "{args:?}: {}", says(&output));
    }
    let dir = Scratch::new("dashes");
    let saved = solidcraft_engine::io::write_design(&Document::new("Draft")).unwrap();
    dir.file("--draft.json", &saved);
    let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("textconv").arg("--draft.json").current_dir(&dir.0).output().unwrap();
    assert!(output.status.success() && output.stderr.is_empty(), "{}", says(&output));
    assert_eq!(output.stdout, [saved, b"\n".to_vec()].concat());
}

/// A reader that stops early (`| head`, or git's pager quitting) does not make textconv fail. The
/// design here prints more than a pipe holds, so the write meets the closed pipe.
#[test]
fn a_closed_pipe_does_not_stop_textconv() {
    let design = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/action_matrix/designs/pi-case-0.8.sc");
    let mut child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .arg("textconv")
        .arg(&design)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success() && output.stderr.is_empty(), "{}", says(&output));
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn diff<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Output {
    cli(&["diff"], args)
}

/// The CLI run with `before` and then `args`.
fn cli<S: AsRef<std::ffi::OsStr>>(before: &[&str], args: &[S]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(before).args(args).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The `--json` reply of `diff A B`, which must exit with `code`.
fn reply(a: &str, b: &str, code: i32) -> Value {
    let out = diff(&[a, b, "--json"]);
    assert_eq!(out.status.code(), Some(code), "{}{}", text(&out.stdout), text(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out.stdout)))
}

fn corpus(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus").join(name).to_string_lossy().into_owned()
}

/// A corpus design saved as `before/part.sc`, edited by `edits` and saved as `after/part.sc`. Both
/// are saved from one session, so what opening does (a re-solve, pinned body names) is in both,
/// and under one file name, so is the name the design takes from it.
fn edited(scratch: &Scratch, design: &str, edits: &[(&str, Value)]) -> (String, String) {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let part = |side: &str| {
        let folder = scratch.0.join(format!("{n}-{side}"));
        std::fs::create_dir(&folder).unwrap();
        folder.join("part.sc").to_string_lossy().into_owned()
    };
    let (before, after) = (part("before"), part("after"));
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": corpus(design)}));
    run(&mut s, "file.save_as", json!({"path": before}));
    for (id, p) in edits {
        run(&mut s, id, p.clone());
    }
    run(&mut s, "file.save_as", json!({"path": after}));
    (before, after)
}

/// `(id, name)` of each listed feature.
fn features(list: &Value) -> Vec<(u64, String)> {
    list.as_array().into_iter().flatten().map(|f| (f["id"].as_u64().unwrap_or(0), f["name"].as_str().unwrap_or("").to_owned())).collect()
}

/// Where each entry of a reply is, such as `features.changed`, once per entry. Every list counts
/// (`other`, `view`, `sketches`, `imports`, the warnings) and so does a rollback marker change.
/// Only `same` and `format` are left out.
fn entries(r: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut count = |at: String, v: &Value| match v {
        Value::Array(list) => out.extend(list.iter().map(|_| at.clone())),
        _ => out.push(at),
    };
    for (key, value) in r.as_object().into_iter().flatten() {
        match (key.as_str(), value) {
            ("same" | "format", _) => {}
            (_, Value::Object(parts)) => parts.iter().for_each(|(part, v)| count(format!("{key}.{part}"), v)),
            _ => count(key.clone(), value),
        }
    }
    out
}

/// Every design the repository keeps compares the same as its container form. The old corpus
/// designs are upgraded first, so the format difference alone is no change, and the containers'
/// imported files and images are compared by what they hold.
#[test]
fn each_design_compares_the_same_as_its_container_form() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folders = [
        ("tests/corpus", "solidcraft"),
        ("../../tests/action_matrix/designs", "sc"),
        ("../../crates/engine/src/tests/fixtures", "sc"),
        ("../../crates/io/tests/fixtures/mixed-version", "sc"),
    ];
    let mut designs: Vec<PathBuf> = folders
        .iter()
        .flat_map(|(dir, ext)| {
            std::fs::read_dir(root.join(dir)).unwrap().flatten().map(|e| e.path()).filter(move |p| p.extension().is_some_and(|x| x == *ext))
        })
        .collect();
    designs.sort();
    assert!(designs.len() >= 22, "{designs:?}");
    let scratch = Scratch::new("diff");
    for (i, path) in designs.iter().enumerate() {
        let doc = solidcraft_engine::io::read_design(&std::fs::read(path).unwrap()).unwrap();
        let packed = scratch.0.join(format!("{i}.sc"));
        std::fs::write(&packed, solidcraft_engine::io::write_design_container(&doc).unwrap()).unwrap();
        let out = diff(&[path.as_path(), packed.as_path()]);
        assert_eq!(out.status.code(), Some(0), "{}: {}{}", path.display(), text(&out.stdout), text(&out.stderr));
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{}: {}{}", path.display(), text(&out.stdout), text(&out.stderr));
    }
    // One design saved by two releases runs either way, 0 or 1.
    let pair = root.join("../../crates/io/tests/fixtures/mixed-version");
    let out = diff(&[pair.join("saved-by-main.sc"), pair.join("saved-again-by-0.45.0.sc")]);
    assert!(matches!(out.status.code(), Some(0 | 1)), "{}{}", text(&out.stdout), text(&out.stderr));
}

#[test]
fn a_parameter_change_is_that_parameter() {
    let scratch = Scratch::new("diff");
    let (a, b) = edited(&scratch, "m0-box-fillet-hole.solidcraft", &[("parameters.change", json!({"name": "width", "expression": "50 mm"}))]);
    let r = reply(&a, &b, 1);
    let changed: Vec<(&str, &str, &str)> = r["parameters"]["changed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| (c["b"]["name"].as_str().unwrap_or(""), c["a"]["expression"].as_str().unwrap_or(""), c["b"]["expression"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(changed, vec![("width", "40 mm", "50 mm")], "{r}");
    assert_eq!(entries(&r), ["parameters.changed"], "nothing else: {r}");
    assert_eq!(r["same"], false);
}

/// Each edit gives exactly one entry, and nothing else in the reply differs. A deleted feature
/// takes its own inputs with it, and they are no parameter entry.
#[test]
fn timeline_edits_are_one_entry_each() {
    let scratch = Scratch::new("diff");
    // The last feature deleted. Its named Distance input goes with it, as a key of the feature.
    let (a, b) = edited(&scratch, "m0-box-fillet-hole.solidcraft", &[("timeline.delete", json!({"feature": "Extrude2"}))]);
    let r = reply(&a, &b, 1);
    assert_eq!(entries(&r), ["features.removed"], "{r}");
    assert_eq!(features(&r["features"]["removed"]), [(5, "Extrude2".to_owned())], "{r}");
    // Renamed.
    let (a, b) = edited(&scratch, "m0-box-fillet-hole.solidcraft", &[("timeline.rename", json!({"feature": "Extrude1", "name": "Base"}))]);
    let r = reply(&a, &b, 1);
    assert_eq!(entries(&r), ["features.changed"], "{r}");
    assert_eq!(features(&r["features"]["changed"]), [(2, "Base".to_owned())], "{r}");
    assert_eq!(r["features"]["changed"][0], json!({"id": 2, "name": "Base", "type": "extrude", "renamed_from": "Extrude1", "keys": []}), "{r}");
    let out = diff(&[a.as_str(), b.as_str()]);
    let format = r["format"]["a"].as_str().unwrap_or_default();
    assert!(format.starts_with("solidcraft/") && r["format"]["b"] == format, "{r}");
    assert_eq!(text(&out.stdout), format!("format {format}\nfeatures\n  ~ Base (extrude, id 2) renamed from Extrude1\n"));
    // Suppressed.
    let (a, b) = edited(&scratch, "m0-box-fillet-hole.solidcraft", &[("timeline.suppress", json!({"feature": "Fillet1", "suppressed": true}))]);
    let r = reply(&a, &b, 1);
    assert_eq!(entries(&r), ["features.changed"], "{r}");
    assert_eq!(r["features"]["changed"][0], json!({"id": 3, "name": "Fillet1", "type": "fillet", "suppressed": true, "keys": []}), "{r}");
    // Box1 moved to the start is one move, from after Revolve1.
    let (a, b) = edited(&scratch, "m0-revolve-primitives.solidcraft", &[("timeline.reorder", json!({"feature": "Box1", "position": 0}))]);
    let r = reply(&a, &b, 1);
    assert_eq!(entries(&r), ["features.moved"], "{r}");
    assert_eq!(features(&r["features"]["moved"]), [(3, "Box1".to_owned())], "{r}");
    let moved = &r["features"]["moved"][0];
    assert_eq!((&moved["after_a"]["name"], &moved["after_b"]), (&json!("Revolve1"), &Value::Null), "{r}");
}

#[test]
fn exit_codes_say_same_different_or_error() {
    let scratch = Scratch::new("diff");
    let design = corpus("m0-box-fillet-hole.solidcraft");
    let same = diff(&[design.as_str(), design.as_str()]);
    assert_eq!(same.status.code(), Some(0), "{}", text(&same.stderr));
    assert!(same.stdout.is_empty(), "{}", text(&same.stdout));
    let missing = scratch.at("missing.sc");
    let drawing = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/io/tests/fixtures/mixed-version/drawing-saved-by-main.scdwg");
    let drawing = drawing.to_string_lossy().into_owned();
    for args in [
        vec![design.as_str(), missing.as_str()],
        vec![drawing.as_str(), design.as_str()],
        vec![design.as_str()],
        vec![design.as_str(), design.as_str(), design.as_str()],
        vec![design.as_str(), design.as_str(), "--jsn"],
    ] {
        let out = diff(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}{}", text(&out.stdout), text(&out.stderr));
        assert!(out.stdout.is_empty() && text(&out.stderr).starts_with("error: "), "{args:?}: {}", text(&out.stderr));
    }
    assert!(text(&diff(&[design.as_str(), missing.as_str()]).stderr).contains("missing.sc"));
    // A global flag before `diff` changes none of that, and neither does a bad size.
    for flag in [&["--max-file-size", "2GB"][..], &["--max-file-size=2GB"], &["--max-file-size", "lots"]] {
        let before: Vec<&str> = flag.iter().copied().chain(["diff"]).collect();
        let out = cli(&before, &[design.as_str(), missing.as_str()]);
        assert_eq!(out.status.code(), Some(2), "{before:?}: {}{}", text(&out.stdout), text(&out.stderr));
        assert!(out.stdout.is_empty() && text(&out.stderr).starts_with("error: "), "{before:?}: {}", text(&out.stderr));
    }
    for flag in [&["--max-file-size", "2GB"][..], &["--max-file-size=2GB"]] {
        let before: Vec<&str> = flag.iter().copied().chain(["diff"]).collect();
        let out = cli(&before, &[design.as_str(), design.as_str()]);
        assert_eq!(out.status.code(), Some(0), "{before:?}: {}{}", text(&out.stdout), text(&out.stderr));
        assert!(out.stdout.is_empty(), "{before:?}: {}", text(&out.stdout));
    }
    // A global flag with no value is an error of the diff too, when `diff` is the first argument
    // left once the flags are out. The flags are taken out in turn (`--max-file-size`, then
    // `--plugin-limits`, then `--plugins-dir` and `--keep-plugin-approvals`). Each flag with a
    // value takes the next argument still there once the flags before it are out, and
    // `--keep-plugin-approvals` takes none. Only `--max-file-size` takes a value that starts with
    // `--`, so the other two have no value when the next argument still there starts with `--`.
    // The designs are the same, so only the bad flag can fail the run. A run with something else
    // first once the flags are out is no diff and exits with 1, as when a flag right before `diff`
    // takes `diff` as its value.
    let d = design.as_str();
    for (args, bad) in [
        (&["--plugins-dir", "p", "diff", d, d, "--max-file-size"][..], "--max-file-size"),
        (&["--keep-plugin-approvals", "diff", d, d, "--plugin-limits"][..], "--plugin-limits"),
        (&["--plugins-dir", "--keep-plugin-approvals", "diff", d, d][..], "--plugins-dir"),
    ] {
        let out = cli(args, &[] as &[&str]);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}{}", text(&out.stdout), text(&out.stderr));
        let error = text(&out.stderr);
        assert!(out.stdout.is_empty() && error.starts_with(&format!("error: {bad} needs a")), "{args:?}: {error}");
    }
    // `--json` before, between or after the two designs.
    let (a, b) = edited(&scratch, "m0-box-fillet-hole.solidcraft", &[("timeline.rename", json!({"feature": "Fillet1", "name": "Round"}))]);
    let (a, b) = (a.as_str(), b.as_str());
    let replies: Vec<Value> = [["--json", a, b], [a, "--json", b], [a, b, "--json"]]
        .iter()
        .map(|args| {
            let out = diff(args);
            assert_eq!(out.status.code(), Some(1), "{args:?}: {}", text(&out.stderr));
            serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{args:?}: {e}: {}", text(&out.stdout)))
        })
        .collect();
    assert!(replies.iter().all(|r| *r == replies[0] && r["features"]["changed"][0]["renamed_from"] == "Fillet1"), "{replies:?}");
    assert_eq!(entries(&replies[0]), ["features.changed"], "exit 1 is the rename alone: {}", replies[0]);
}
