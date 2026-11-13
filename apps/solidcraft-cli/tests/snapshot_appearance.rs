//! `snapshot --body`, `--tool` and `--section`: a picture of a tool body inside the material it
//! meets, which is the one picture that says whether a cut reaches through.
//!
//! Each option is a shortcut for the command the app itself runs (`appearance.assign`,
//! `inspect.section`), so every test here asserts that the option's picture is the picture of a
//! script that runs those commands — not that the pixels match a stored image. Pictures are
//! compared with a tolerance, and the tests run on whatever renderer the machine has (a GPU, a
//! software adapter, or the CPU rasterizer), because they only ever compare two pictures this
//! same CLI drew.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const SIZE: (u32, u32) = (200, 140);

/// A PNG's size and RGBA pixels.
fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap().to_rgba8();
    (img.width(), img.height(), img.into_raw())
}

/// Pixels of two same-sized pictures that differ by more than a little in some channel.
fn differing(a: &[u8], b: &[u8]) -> usize {
    let ((wa, ha, a), (wb, hb, b)) = (decode(a), decode(b));
    assert_eq!((wa, ha), (wb, hb), "pictures of different sizes");
    a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 16)).count()
}

/// The same picture: a renderer's output may differ by a few levels or a stray edge pixel.
#[track_caller]
fn assert_same(a: &[u8], b: &[u8], what: &str) {
    let (n, total) = (differing(a, b), (SIZE.0 * SIZE.1) as usize);
    assert!(n * 500 <= total, "{what}: {n} of {total} pixels differ");
}

/// Different pictures: more than stray pixels differ.
#[track_caller]
fn assert_differs(a: &[u8], b: &[u8], what: &str) {
    let n = differing(a, b);
    assert!(n >= 100, "{what}: only {n} pixels differ");
}

/// How much of a picture is the tool colour (`preview::TOOL_COLOR`, #e8542c): a red neither
/// the default body grey nor the background has, still recognisable once a see-through body in
/// front of it has tinted it (measured on this machine: (200, 108, 85) through a target at
/// opacity 0.3, against (164, 173, 184) for the plain grey).
fn tool_pixels(png: &[u8]) -> usize {
    let (_, _, px) = decode(png);
    px.as_chunks::<4>().0.iter().filter(|p| p[0] > 140 && p[0] > p[1].saturating_add(50) && p[0] > p[2].saturating_add(50)).count()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("solidcraft-snapshot-appearance-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    /// A plate with a pin standing in it: the tool body a cut would remove, reaching 6 mm short
    /// of the far face, which is exactly what a picture is supposed to show and a volume is not.
    /// `more` runs on top.
    fn script(&self, name: &str, more: &[Value]) -> PathBuf {
        let mut steps = vec![
            json!({"command": "solid.box", "params": {"length": 50, "width": 34, "height": 20, "center": [0, 0, 10], "body_name": "Target"}}),
            json!({"command": "solid.cylinder", "params": {"diameter": 12, "height": 14, "base": [0, 0, 0], "operation": "new_body", "body_name": "Tool"}}),
        ];
        steps.extend(more.iter().cloned());
        let path = self.0.join(name);
        std::fs::write(&path, json!({"commands": steps}).to_string()).unwrap();
        path
    }

    fn run(&self, script: &Path, out: &str, extra: &[&str]) -> Output {
        let (w, h) = (SIZE.0.to_string(), SIZE.1.to_string());
        Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
            .arg("snapshot")
            .arg(script)
            .arg("--out")
            .arg(self.0.join(out))
            .args(["--width", &w, "--height", &h, "--no-grid", "--no-axes"])
            .args(extra)
            .output()
            .unwrap()
    }

    /// The PNG `snapshot` writes.
    fn snapshot(&self, script: &Path, out: &str, extra: &[&str]) -> Vec<u8> {
        let output = self.run(script, out, extra);
        assert!(output.status.success(), "{extra:?}: {}", String::from_utf8_lossy(&output.stderr));
        let png = std::fs::read(self.0.join(out)).unwrap();
        assert!(png.starts_with(b"\x89PNG"), "{extra:?}");
        png
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `--body NAME:opacity=…` draws that body see-through, and the picture is the one a script
/// gets by assigning the same appearance itself: the option is the command, not a second way
/// of drawing.
#[test]
fn a_body_drawn_see_through_is_the_picture_of_the_same_appearance_assigned() {
    let dir = Scratch::new();
    let plain = dir.script("plain.json", &[]);
    let assigned =
        dir.script("assigned.json", &[json!({"command": "appearance.assign", "params": {"bodies": ["Target"], "color": "#b0bac6", "opacity": 0.3}})]);
    let solid = dir.snapshot(&plain, "solid.png", &[]);
    let see_through = dir.snapshot(&plain, "see-through.png", &["--body", "Target:opacity=0.3"]);
    assert_differs(&see_through, &solid, "--body opacity was ignored");
    assert_same(&see_through, &dir.snapshot(&assigned, "by-script.png", &[]), "the same appearance assigned by a script");

    // The option's colour is the design's too. It names the target, not the tool: the tool is
    // inside the target, so a solid target hides whatever colour the tool is given.
    let painted =
        dir.script("painted.json", &[json!({"command": "appearance.assign", "params": {"bodies": ["Target"], "color": "#00aa33", "opacity": 1.0}})]);
    let by_option = dir.snapshot(&plain, "painted.png", &["--body", "Target:color=#00aa33"]);
    assert_differs(&by_option, &solid, "--body color was ignored");
    assert_same(&by_option, &dir.snapshot(&painted, "painted-script.png", &[]), "the same colour assigned by a script");
}

/// `--tool NAME` is the whole point: the tool solid in the tool colour, every other body
/// see-through, so the tool shows inside the material it meets. The tool's colour covers more
/// of the picture than it does without the option, because the target no longer hides it.
#[test]
fn a_tool_shows_through_the_bodies_around_it() {
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let plain = dir.snapshot(&script, "plain.png", &["--view", "front"]);
    let preview = dir.snapshot(&script, "preview.png", &["--view", "front", "--tool", "Tool"]);
    assert_differs(&preview, &plain, "--tool was ignored");
    // Seen from the front the plate hides the pin completely; the preview shows all of it.
    assert_eq!(tool_pixels(&plain), 0, "nothing is the tool colour before the option");
    assert!(tool_pixels(&preview) > 200, "the tool shows through the target: {} pixels", tool_pixels(&preview));

    // It is the two commands it stands for, run in that order.
    let by_script = dir.script(
        "by-script.json",
        &[
            json!({"command": "appearance.assign", "params": {"bodies": ["Target"], "color": "#b0bac6", "opacity": 0.3}}),
            json!({"command": "appearance.assign", "params": {"bodies": ["Tool"], "color": "#e8542c", "opacity": 1.0}}),
        ],
    );
    assert_same(&preview, &dir.snapshot(&by_script, "script.png", &["--view", "front"]), "the appearances --tool assigns");

    // A later --body overrides what --tool said.
    let overridden = dir.snapshot(&script, "override.png", &["--view", "front", "--tool", "Tool", "--body", "Tool:color=#2255cc"]);
    assert!(tool_pixels(&overridden) < 50, "--body after --tool decides the tool's colour");
}

/// `--section` cuts the model where `inspect.section` would, and the same spec written as the
/// command's own JSON, or as the command in a script, gives the same picture.
#[test]
fn a_section_cuts_the_model_as_inspect_section_does() {
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let whole = dir.snapshot(&script, "whole.png", &[]);
    let cut = dir.snapshot(&script, "cut.png", &["--section", "z=12"]);
    assert_differs(&cut, &whole, "--section was ignored");
    assert_same(&cut, &dir.snapshot(&script, "cut-json.png", &["--section", r#"{"plane": "XY", "at": 12}"#]), "the command's own JSON");
    let by_script = dir.script("sectioned.json", &[json!({"command": "inspect.section", "params": {"plane": "XY", "at": 12}})]);
    assert_same(&cut, &dir.snapshot(&by_script, "by-script.png", &[]), "inspect.section in a script");

    // The other side of the same plane is a different picture.
    let flipped = dir.snapshot(&script, "flipped.png", &["--section", "z=12,flip"]);
    assert_differs(&flipped, &cut, "flip keeps the other side");

    // A section through a see-through target still shows the tool inside it.
    let both = dir.snapshot(&script, "both.png", &["--section", "z=12", "--tool", "Tool"]);
    assert_differs(&both, &cut, "--tool and --section together");
    assert!(tool_pixels(&both) > 100, "the tool is in the cut picture");
}

/// Every way of getting the options wrong is an error that says what it takes, and writes no
/// picture: a snapshot that quietly ignored an option would be a lie about the design.
#[test]
fn a_body_tool_or_section_that_cannot_be_used_is_an_error_and_writes_nothing() {
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let cases: [(&[&str], &str); 8] = [
        (&["--body", "Nowhere:opacity=0.3"], "no body `Nowhere`"),
        (&["--body", "Target"], "NAME:key=value"),
        (&["--body", "Target:opacity=3"], "0 to 1"),
        (&["--body", "Target:alpha=0.3"], "unknown key `alpha`"),
        (&["--body", "Target:color=red"], "#rrggbb"),
        (&["--tool", "Nowhere"], "no body `Nowhere`"),
        (&["--section", "q=1"], "unknown axis `q`"),
        (&["--section", "nonsense"], "z=27"),
    ];
    for (extra, expect) in cases {
        let output = dir.run(&script, "never.png", extra);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{extra:?}: {stderr}");
        assert!(stderr.contains(expect), "{extra:?}: {stderr}");
        assert!(output.stdout.is_empty() && !dir.0.join("never.png").exists(), "{extra:?}");
    }
    // An error naming a body says which bodies the design has.
    let stderr = String::from_utf8_lossy(&dir.run(&script, "never.png", &["--tool", "Nowhere"]).stderr).to_string();
    assert!(stderr.contains("Target") && stderr.contains("Tool"), "{stderr}");
}

/// The usage text says what the options do.
#[test]
fn the_usage_names_the_body_tool_and_section_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("snapshot").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    for word in ["--body", "--tool", "--section", "appearance.assign", "inspect.section", "see-through"] {
        assert!(stderr.contains(word), "usage lacks {word}: {stderr}");
    }
}
