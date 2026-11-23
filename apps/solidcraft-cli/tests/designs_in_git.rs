//! Designs kept in Git (#626). `solidcraft-cli textconv` prints a design file as text for
//! `git diff`, and it never stops the diff for a file it can read.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use solidcraft_engine::doc::Document;

/// A folder of its own for one test, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("solidcraft-designs-in-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
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
