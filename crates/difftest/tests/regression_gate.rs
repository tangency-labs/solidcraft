//! Exercise the real CLI parent/child gates without running a full native sweep.
#![cfg(feature = "cpp")]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
// Test fixture setup deliberately fails immediately on filesystem errors.
#[allow(clippy::unwrap_used)]
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "solidcore-gate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Test-only child cleanup must fail loudly if the early gate hangs.
#[allow(clippy::unwrap_used, clippy::panic)]
fn run(args: &[String], digests: Option<&PathBuf>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_difftest"));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("DIFFTEST_RUST_DIGEST");
    if let Some(dir) = digests {
        command.env("DIFFTEST_RUST_DIGEST", dir);
    }
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() > Duration::from_secs(20) {
            // This is only our own child. Its jobs workers also watch the parent
            // pipe, so a broken early gate cannot leave a full sweep behind.
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("CLI gate failed to return promptly: {:?}", output.status);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(ToString::to_string).collect()
}

#[test]
fn dropped_units_and_invalid_baselines_fail_before_parallel_dispatch() {
    let scratch = Scratch::new();
    let baseline = scratch.0.join("baseline");
    fs::create_dir(&baseline).unwrap();
    let report = json!({
        "schema_version": 1, "unit": "removed.unit", "status": "green", "pass": true,
        "seeds": 16, "cases_total": 1, "cases_passed": 1, "cases_failed": 0,
        "cases_deferred": 0, "cases_vacuous": 0, "cases_both_errored": 0,
        "cases": [{"name": "kept", "source": "inline", "pass": true,
                   "outcome": "pass", "values_compared": 0, "exact_compared": 1}]
    });
    let path = baseline.join("removed.unit.json");
    fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
    for jobs in ["1", "2"] {
        let mut args = strings(&["--green", "--no-report", "--jobs", jobs, "--baseline"]);
        args.push(baseline.display().to_string());
        let result = run(&args, None);
        assert_eq!(result.status.code(), Some(1));
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert!(stdout.contains("unit `removed.unit` passed in the baseline"));
        assert!(!stdout.contains("TIME "));
        assert!(!stdout.contains("PASS "));
    }
    let mut new_unit = strings(&["--unit", "harness.selftest", "--no-report", "--baseline"]);
    new_unit.push(baseline.display().to_string());
    assert!(run(&new_unit, None).status.success());
    fs::remove_file(path).unwrap();
    for jobs in ["1", "2"] {
        let mut args = strings(&["--unit", "harness.selftest", "--jobs", jobs, "--baseline"]);
        args.push(baseline.display().to_string());
        assert_eq!(run(&args, None).status.code(), Some(2));
    }
    fs::write(baseline.join("harness.selftest.json"), "{").unwrap();
    for jobs in ["1", "2"] {
        let mut args = strings(&["--unit", "harness.selftest", "--jobs", jobs, "--baseline"]);
        args.push(baseline.display().to_string());
        let output = run(&args, None);
        assert_eq!(output.status.code(), Some(2));
        assert!(!String::from_utf8(output.stdout).unwrap().contains("TIME "));
    }
    let mut args = strings(&["--unit", "harness.selftest", "--filter", "x", "--baseline"]);
    args.push(baseline.display().to_string());
    let output = run(&args, None);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("unfiltered comparison"));
}

#[test]
fn child_case_and_value_gates_preserve_reports_and_output_digests() {
    let scratch = Scratch::new();
    let baseline = scratch.0.join("baseline");
    let original_digests = scratch.0.join("original-digests");
    let mut original = strings(&[
        "--unit",
        "harness.selftest",
        "--unit",
        "geom.matrix",
        "--report-dir",
    ]);
    original.push(baseline.display().to_string());
    let output = run(&original, Some(&original_digests));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let path = baseline.join("geom.matrix.json");
    let clean: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    // Explicit unit runs validate their selected reports. The whole-set parent
    // still rejects unrelated corruption before dispatching any native work.
    fs::write(baseline.join("unselected.bad.json"), "{").unwrap();
    let mut whole = strings(&["--green", "--no-report", "--jobs", "2", "--baseline"]);
    whole.push(baseline.display().to_string());
    let output = run(&whole, None);
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8(output.stdout).unwrap().contains("TIME "));
    for jobs in ["1", "2"] {
        let mut selected = strings(&[
            "--unit",
            "harness.selftest",
            "--unit",
            "geom.matrix",
            "--no-report",
            "--jobs",
            jobs,
            "--baseline",
        ]);
        selected.push(baseline.display().to_string());
        assert!(run(&selected, None).status.success());
    }
    for mode in ["missing", "values"] {
        let mut report = clean.clone();
        if mode == "missing" {
            let mut case = report["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| {
                    c["outcome"] == "pass"
                        && (c["values_compared"].as_u64().unwrap() > 0
                            || c["exact_compared"].as_u64().unwrap() > 0)
                })
                .unwrap()
                .clone();
            case["name"] = json!("previously-covered-case");
            report["cases"].as_array_mut().unwrap().push(case);
            for key in ["cases_total", "cases_passed"] {
                report[key] = json!(report[key].as_u64().unwrap() + 1);
            }
        } else {
            let index = report["cases"]
                .as_array()
                .unwrap()
                .iter()
                .position(|c| {
                    c["outcome"] == "pass" && c["values_compared"] == 0 && c["exact_compared"] == 0
                })
                .unwrap();
            for (flag, aggregate) in [
                ("both_errored", "cases_both_errored"),
                ("vacuous", "cases_vacuous"),
            ] {
                if report["cases"][index][flag] == true {
                    report[aggregate] = json!(report[aggregate].as_u64().unwrap() - 1);
                }
                report["cases"][index].as_object_mut().unwrap().remove(flag);
            }
            report["cases"][index]["exact_compared"] = json!(1);
        }
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        for jobs in ["1", "2"] {
            let digests = scratch.0.join(format!("{mode}-{jobs}"));
            let mut args = strings(&[
                "--unit",
                "harness.selftest",
                "--unit",
                "geom.matrix",
                "--no-report",
                "--jobs",
                jobs,
                "--baseline",
            ]);
            args.push(baseline.display().to_string());
            let output = run(&args, Some(&digests));
            assert_eq!(
                output.status.code(),
                Some(1),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("GATE-FAIL (ratchet)"), "{text}");
            assert!(
                text.contains(if mode == "missing" {
                    "is gone"
                } else {
                    "now compares nothing"
                }),
                "{text}"
            );
            for unit in ["harness.selftest", "geom.matrix"] {
                for suffix in ["digest", "cppdigest"] {
                    let file = format!("{unit}.{suffix}");
                    assert_eq!(
                        fs::read(digests.join(&file)).unwrap(),
                        fs::read(original_digests.join(file)).unwrap()
                    );
                }
            }
        }
    }
}

// `--compare-reports` applies the ratchet to finished reports (CI sweeps in another job).
#[test]
#[allow(clippy::unwrap_used)]
fn compare_reports_ratchets_finished_reports() {
    let scratch = Scratch::new();
    let report = |cases: &[(&str, &str)]| {
        let cases: Vec<Value> = cases
            .iter()
            .map(|(name, outcome)| {
                json!({"name": name, "source": "inline", "pass": *outcome == "pass",
                       "outcome": outcome, "values_compared": 0, "exact_compared": 1})
            })
            .collect();
        json!({"schema_version": 1, "unit": "some.unit", "status": "green", "pass": true,
               "seeds": 16, "cases_total": cases.len(), "cases_passed": cases.len(),
               "cases_failed": 0, "cases_deferred": 0, "cases_vacuous": 0,
               "cases_both_errored": 0, "cases": cases})
    };
    let write = |dir: &str, value: &Value| {
        let dir = scratch.0.join(dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("some.unit.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
        dir.display().to_string()
    };
    let base = write("base", &report(&[("a", "pass"), ("b", "pass")]));
    let compare = |current: String| {
        let mut args = strings(&["--compare-reports"]);
        args.push(current);
        args.push("--baseline".into());
        args.push(base.clone());
        run(&args, None)
    };
    let same = compare(write(
        "same",
        &report(&[("a", "pass"), ("b", "pass"), ("c", "pass")]),
    ));
    assert_eq!(same.status.code(), Some(0));
    let lost = compare(write("lost", &report(&[("a", "pass")])));
    assert_eq!(lost.status.code(), Some(1));
    assert!(String::from_utf8(lost.stdout).unwrap().contains("case `b`"));
    let empty = scratch.0.join("empty");
    fs::create_dir(&empty).unwrap();
    assert_eq!(compare(empty.display().to_string()).status.code(), Some(1));
    // Without --baseline the mode refuses to run.
    let mut args = strings(&["--compare-reports"]);
    args.push(base.clone());
    assert_eq!(run(&args, None).status.code(), Some(2));
}
