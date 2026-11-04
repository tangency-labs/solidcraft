//! A timeline feature that fails fails `run`, and nothing is written: the bodies lack what the
//! feature should have made, and handing them out with exit 0 hides the failure.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-run-strict-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
}

#[test]
fn a_failing_feature_fails_the_run_and_writes_nothing() {
    let dir = Scratch::new();
    let script = dir.broken_script();
    let (step, saved) = (dir.0.join("part.step"), dir.0.join("part.solidcraft"));
    for extra in [&[][..], &["--results"][..], &["--quiet"][..]] {
        let output = cli().arg("run").arg(&script).args(extra).arg("--out").arg(&step).arg("--save").arg(&saved).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{extra:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Fillet1") && stderr.contains("nothing was written"), "{stderr}");
        assert!(output.stdout.is_empty(), "{extra:?}");
        assert!(!step.exists() && !saved.exists(), "{extra:?}");
    }
}

#[test]
fn allow_errors_writes_anyway_and_a_saved_design_keeps_failing() {
    let dir = Scratch::new();
    let (step, saved) = (dir.0.join("part.step"), dir.0.join("part.solidcraft"));
    let output = cli().arg("run").arg(dir.broken_script()).arg("--allow-errors").arg("--out").arg(&step).arg("--save").arg(&saved).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let measured: Value = serde_json::from_slice(&output.stdout).unwrap();
    // SMLib integrates the volume (a relative accuracy of 1e-4 asked for; a box comes out within
    // a few units in the last place), so the box's 4000 mm³ is checked to 1e-9, not to the bit.
    let volume = measured["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - 4000.0).abs() < 4000.0 * 1e-9, "the box without its fillet: {volume}");
    assert!(std::fs::metadata(&step).unwrap().len() > 0);
    // The saved design still holds the failing feature: running it fails too, `eval` reports it.
    let again = dir.0.join("again.step");
    let output = cli().arg("run").arg(&saved).arg("--out").arg(&again).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!again.exists());
    let output = cli().arg("eval").arg(&saved).output().unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["timeline"].as_array().unwrap().iter().any(|f| f["error"].is_string()), "{}", report["timeline"]);
}

#[test]
fn a_clean_timeline_still_writes() {
    let dir = Scratch::new();
    let script = dir.0.join("box.json");
    std::fs::write(&script, json!({"commands": [{"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4}}]}).to_string()).unwrap();
    let step = dir.0.join("box.step");
    let output = cli().arg("run").arg(&script).arg("--out").arg(&step).arg("--quiet").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(std::fs::metadata(&step).unwrap().len() > 0);
}
