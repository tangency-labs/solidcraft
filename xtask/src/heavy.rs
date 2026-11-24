//! The heavy tests: the ones the `ci` profile of `.config/nextest.toml` leaves out by default
//! (its `default-filter`, the list pull requests skip). Local runs (`cargo xtask test`,
//! `cargo xtask ci`) leave them out too, so a run on a workstation takes minutes, not an hour;
//! `SOLIDCRAFT_TEST_HEAVY=1` runs them as well. The merge queue, `main` and the nightly jobs
//! always run them. nextest evaluates the filter, so the list lives in one place.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::process::Stdio;

use crate::cargo;

/// Whether `SOLIDCRAFT_TEST_HEAVY` asks for the heavy tests too.
pub fn opted_in() -> Result<bool, String> {
    parse(std::env::var_os("SOLIDCRAFT_TEST_HEAVY").as_deref())
}

/// Unset or `0`: leave the heavy tests out; `1`: run them. Anything else is refused before the build.
fn parse(value: Option<&OsStr>) -> Result<bool, String> {
    match value.map(OsStr::to_str) {
        None | Some(Some("0")) => Ok(false),
        Some(Some("1")) => Ok(true),
        _ => Err("SOLIDCRAFT_TEST_HEAVY must be 0 or 1".into()),
    }
}

/// What a run leaves out to skip the heavy tests: the binaries the filter takes whole (nextest
/// lists them without their tests), and for each other binary, by path, the libtest `--skip`
/// filters that leave out its heavy tests.
#[derive(Debug, Default)]
pub struct LeftOut {
    pub binaries: BTreeSet<String>,
    pub skips: BTreeMap<String, Vec<String>>,
}

/// The heavy tests to leave out, from `cargo nextest list` with the `ci` profile. `None` without
/// cargo-nextest: every test runs.
pub fn left_out(cargo_args: &[&str]) -> Result<Option<LeftOut>, String> {
    let mut version = cargo();
    version.args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null());
    if !version.status().is_ok_and(|s| s.success()) {
        eprintln!("test: cargo-nextest is not installed, so the heavy tests run too");
        return Ok(None);
    }
    let mut c = cargo();
    c.args(["nextest", "list", "--workspace", "--cargo-profile", "ci", "--profile", "ci", "--message-format", "json"]).args(cargo_args);
    eprintln!("$ cargo nextest list --profile ci (the heavy tests to leave out)");
    let out = c.stdin(Stdio::null()).stderr(Stdio::inherit()).output().map_err(|e| format!("cargo nextest list: failed to spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo nextest list: exited with {}", out.status));
    }
    let listed: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo nextest list: {e}"))?;
    Ok(Some(heavy_in(&listed)))
}

/// libtest's `--skip` matches by substring, so a heavy test's name is a filter only when it
/// leaves out no other test of its binary; otherwise that heavy test runs, and says why.
fn heavy_in(listed: &serde_json::Value) -> LeftOut {
    let mut left = LeftOut::default();
    let Some(suites) = listed["rust-suites"].as_object() else { return left };
    for suite in suites.values() {
        let Some(path) = suite["binary-path"].as_str() else { continue };
        if suite["status"] == "skipped-default-filter" {
            left.binaries.insert(path.to_string());
            continue;
        }
        let Some(cases) = suite["testcases"].as_object() else { continue };
        let heavy: BTreeSet<&str> =
            cases.iter().filter(|(_, case)| case["filter-match"]["reason"] == "default-filter").map(|(name, _)| name.as_str()).collect();
        let mut filters = Vec::new();
        for name in &heavy {
            match cases.keys().find(|other| !heavy.contains(other.as_str()) && other.contains(name)) {
                Some(other) => eprintln!("test: runs the heavy {name}: skipping it would skip {other} too"),
                None => filters.push((*name).to_string()),
            }
        }
        if !filters.is_empty() {
            left.skips.insert(path.to_string(), filters);
        }
    }
    left
}

#[cfg(test)]
mod tests {
    use super::{heavy_in, parse};
    use std::ffi::OsStr;

    #[test]
    fn the_opt_in_is_zero_or_one() {
        assert_eq!(parse(None), Ok(false));
        assert_eq!(parse(Some(OsStr::new("0"))), Ok(false));
        assert_eq!(parse(Some(OsStr::new("1"))), Ok(true));
        for value in ["", "yes", "true", "2", " 1"] {
            assert!(parse(Some(OsStr::new(value))).is_err(), "{value:?}");
        }
    }

    #[test]
    fn heavy_tests_become_skips_unless_the_skip_would_take_a_light_test() {
        let heavy = serde_json::json!({"status": "mismatch", "reason": "default-filter"});
        let ignored = serde_json::json!({"status": "mismatch", "reason": "ignored"});
        let light = serde_json::json!({"status": "matches"});
        let listed = serde_json::json!({"rust-suites": {
            "engine": {"binary-path": "/t/engine", "status": "listed", "testcases": {
                "tests::hostile_params_never_panic": {"filter-match": heavy},
                "tests::robustness::shell": {"filter-match": heavy},
                "tests::robustness::shell_of_a_box": {"filter-match": light},
                "tests::cube": {"filter-match": light},
                "tests::pending": {"filter-match": ignored},
            }},
            "kernel": {"binary-path": "/t/kernel", "status": "listed", "testcases": {"tests::sphere": {"filter-match": light}}},
            "atomic_commands": {"binary-path": "/t/atomic_commands", "status": "skipped-default-filter", "testcases": {}},
        }});
        let left = heavy_in(&listed);
        assert_eq!(left.skips.len(), 1, "{left:?}");
        assert_eq!(left.skips["/t/engine"], vec!["tests::hostile_params_never_panic".to_string()]);
        assert_eq!(left.binaries.iter().collect::<Vec<_>>(), ["/t/atomic_commands"], "a binary the filter takes whole is left out whole");
        let none = heavy_in(&serde_json::json!({}));
        assert!(none.skips.is_empty() && none.binaries.is_empty());
    }
}
