//! `cargo xtask known-failing --junit PATH... [--list FILE]`: grades a nextest run (its junit
//! report) against the tests known to fail today on the default kernel,
//! `tests/KNOWN_FAILING_TESTS` (the C++ kernel's are `tests/KNOWN_FAILING_TESTS.smlib`, for a run
//! with `SOLIDCRAFT_KERNEL=smlib` and `--list`). CI's test jobs run it after nextest, in place of failing on any failed test. It fails on a test that failed
//! and is not listed, and on a listed test that passed (fixed: its line comes off in the same
//! pull request); a listed test that did not run (a pull request tests only the packages it
//! touches) is neither. The list has the kernel rig's format (`kernel_cases::known_failing`):
//! one test a line, `<test> <why it fails today>`, sorted, with `<test>` nextest's
//! `<binary id>::<test name>` (`solidcraft-kernel::tests::...`, or
//! `solidcraft-cli::action_matrix::action_matrix_sample` for an integration test binary).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

pub const LIST: &str = "tests/KNOWN_FAILING_TESTS";
/// The same for the C++ kernel (`SOLIDCRAFT_KERNEL=smlib`), graded with `--list`.
pub const SMLIB_LIST: &str = "tests/KNOWN_FAILING_TESTS.smlib";

pub fn run(root: &Path, rest: &[&str]) -> Result<(), String> {
    let mut reports = Vec::new();
    let mut list = root.join(LIST);
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--junit" => reports.push(PathBuf::from(args.next().ok_or("--junit needs a junit file or a directory of them")?)),
            "--list" => list = PathBuf::from(args.next().ok_or("--list needs a file")?),
            other => return Err(format!("known-failing: unknown argument `{other}`")),
        }
    }
    if reports.is_empty() {
        return Err("known-failing: give the nextest junit report with --junit FILE or DIR".into());
    }
    let mut files = Vec::new();
    for path in &reports {
        xml_files(path, &mut files)?;
    }
    if files.is_empty() {
        return Err(format!(
            "known-failing: no junit (.xml) file in {}",
            reports.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ));
    }
    let mut outcomes = BTreeMap::new();
    for file in &files {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        add(&mut outcomes, &text).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    let text = std::fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    println!("{}", grade(&text, &outcomes).map_err(|e| format!("{}: {e}", list.display()))?);
    Ok(())
}

/// The tests of one junit report into `all`. A test in two reports failed if it failed in either.
fn add(all: &mut BTreeMap<String, bool>, xml: &str) -> Result<(), String> {
    for (test, failed) in junit_outcomes(xml)? {
        *all.entry(test).or_insert(false) |= failed;
    }
    Ok(())
}

/// `cargo xtask lists`: the list is sorted, each test once, with a cause each.
pub fn check_list(root: &Path) -> Result<(), String> {
    for list in [LIST, SMLIB_LIST] {
        let file = root.join(list);
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        crate::kernel_cases::known_failing(&text).map_err(|e| format!("{list}: {e}"))?;
    }
    Ok(())
}

/// `path` itself, or every `.xml` file under it (sorted), into `out`.
fn xml_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            xml_files(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "xml") {
            out.push(p);
        }
    }
    Ok(())
}

/// Every test a junit report ran, as `<suite>::<test case>` with whether it failed (a `failure`
/// or `error` in its case). A skipped case did not run and is left out; a case that passed on a
/// retry (nextest's `flakyFailure`) passed.
fn junit_outcomes(xml: &str) -> Result<Vec<(String, bool)>, String> {
    let mut reader = Reader::from_str(xml);
    let mut suite: Option<String> = None;
    // The open test case: its id, whether it failed, whether it was skipped.
    let mut case: Option<(String, bool, bool)> = None;
    let mut out = Vec::new();
    loop {
        let event = reader.read_event().map_err(|e| format!("not a junit report: {e}"))?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                b"testsuite" if !empty => suite = Some(attribute(&e, "name")?),
                b"testcase" => {
                    let name = attribute(&e, "name")?;
                    let suite = suite.as_deref().ok_or_else(|| format!("test case {name} outside a testsuite"))?;
                    let id = format!("{suite}::{name}");
                    if empty {
                        out.push((id, false));
                    } else {
                        case = Some((id, false, false));
                    }
                }
                b"failure" | b"error" => {
                    if let Some(c) = case.as_mut() {
                        c.1 = true;
                    }
                }
                b"skipped" => {
                    if let Some(c) = case.as_mut() {
                        c.2 = true;
                    }
                }
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                b"testcase" => {
                    if let Some((id, failed, skipped)) = case.take()
                        && !skipped
                    {
                        out.push((id, failed));
                    }
                }
                b"testsuite" => suite = None,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn attribute(e: &BytesStart, name: &str) -> Result<String, String> {
    let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let a = e.try_get_attribute(name).map_err(|err| format!("<{tag}>: {err}"))?.ok_or_else(|| format!("<{tag}> has no {name}"))?;
    a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.into_owned()).map_err(|err| format!("<{tag} {name}>: {err}"))
}

/// The gate: every test that ran passes except those `list` names. A failed test the list does
/// not name is a regression; a listed test that passed is fixed and must leave the list, so the
/// list only shrinks. A listed test that did not run is neither.
fn grade(list: &str, outcomes: &BTreeMap<String, bool>) -> Result<String, String> {
    let listed = crate::kernel_cases::known_failing(list)?;
    let is_listed = |test: &str| listed.iter().any(|(t, _)| *t == test);
    let mut problems = Vec::new();
    let mut known = 0;
    for (test, failed) in outcomes {
        match (is_listed(test), *failed) {
            (false, true) => problems.push(format!("REGRESSION, failed and not on the known-failing list: {test}")),
            (true, false) => problems.push(format!("FIXED, remove it from the known-failing list: {test}")),
            (true, true) => known += 1,
            (false, false) => {}
        }
    }
    let not_run = listed.iter().filter(|(t, _)| !outcomes.contains_key(*t)).count();
    let summary = format!(
        "known-failing: {} ran, {} pass, {known} known failing, {not_run} listed did not run here",
        outcomes.len(),
        outcomes.values().filter(|f| !**f).count()
    );
    if problems.is_empty() { Ok(format!("{summary}; no regression")) } else { Err(format!("{summary}\n  {}", problems.join("\n  "))) }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{add, grade, junit_outcomes};

    /// A nextest report in miniature: a library and an integration test binary, a pass, a
    /// failure, an error, a skipped case and a pass on a retry.
    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="6" failures="1" errors="1">
    <testsuite name="solidcraft-kernel" tests="4" failures="1" errors="1">
        <testcase name="tests::passes" classname="solidcraft-kernel" time="0.1"/>
        <testcase name="tests::fails" classname="solidcraft-kernel" time="0.2">
            <failure message="thread &apos;tests::fails&apos; panicked" type="test failure with exit code 101">panicked: 1 vs 2</failure>
            <system-out>running 1 test</system-out>
        </testcase>
        <testcase name="tests::times_out" classname="solidcraft-kernel" time="60.0">
            <error message="timed out" type="test timeout"/>
        </testcase>
        <testcase name="tests::skipped" classname="solidcraft-kernel"><skipped/></testcase>
    </testsuite>
    <testsuite name="solidcraft-cli::action_matrix" tests="2">
        <testcase name="action_matrix_sample" classname="solidcraft-cli::action_matrix" time="300.0">
            <flakyFailure message="first try failed" type="test failure"/>
        </testcase>
        <testcase name="a_&lt;b&gt;_case" classname="solidcraft-cli::action_matrix" time="0.1"/>
    </testsuite>
</testsuites>
"#;

    fn outcomes(xml: &str) -> BTreeMap<String, bool> {
        junit_outcomes(xml).unwrap().into_iter().collect()
    }

    #[test]
    fn a_junit_report_gives_each_test_that_ran_with_whether_it_failed() {
        let got: Vec<(String, bool)> = junit_outcomes(REPORT).unwrap();
        let want = [
            ("solidcraft-kernel::tests::passes", false),
            ("solidcraft-kernel::tests::fails", true),
            ("solidcraft-kernel::tests::times_out", true),
            ("solidcraft-cli::action_matrix::action_matrix_sample", false),
            ("solidcraft-cli::action_matrix::a_<b>_case", false),
        ];
        assert_eq!(got, want.map(|(t, f)| (t.to_string(), f)));
        assert!(junit_outcomes("<testsuites><testcase name=\"x\"/></testsuites>").unwrap_err().contains("outside a testsuite"));
        assert!(junit_outcomes("<testsuites><testsuite name=\"a\"><testcase/></testsuite></testsuites>").unwrap_err().contains("has no name"));
        assert!(junit_outcomes("<testsuites><testsuite name=\"a\"></testcase></testsuites>").unwrap_err().contains("not a junit report"));
    }

    #[test]
    fn the_gate_passes_listed_failures_and_fails_on_a_new_failure_or_a_fixed_test() {
        let list = "# today\n\
            solidcraft-kernel::tests::fails SMLib status 1001\n\
            solidcraft-kernel::tests::not_run_here volume 1e-4 off (#2422)\n\
            solidcraft-kernel::tests::times_out SMLib hangs on the corner\n";
        let ok = grade(list, &outcomes(REPORT)).unwrap();
        assert_eq!(ok, "known-failing: 5 ran, 3 pass, 2 known failing, 1 listed did not run here; no regression");

        let fixed = REPORT.replace("<error message=\"timed out\" type=\"test timeout\"/>", "");
        let err = grade(list, &outcomes(&fixed)).unwrap_err();
        assert!(err.contains("FIXED, remove it from the known-failing list: solidcraft-kernel::tests::times_out"), "{err}");
        assert!(err.starts_with("known-failing: 5 ran, 4 pass, 1 known failing"), "{err}");

        let shorter = list.replace("solidcraft-kernel::tests::fails SMLib status 1001\n", "");
        let err = grade(&shorter, &outcomes(REPORT)).unwrap_err();
        assert!(err.contains("REGRESSION, failed and not on the known-failing list: solidcraft-kernel::tests::fails"), "{err}");
        assert!(!err.contains("FIXED"), "{err}");
    }

    #[test]
    fn a_test_failing_in_any_report_failed() {
        let passed = REPORT.replace(
            "<failure message=\"thread &apos;tests::fails&apos; panicked\" type=\"test failure with exit code 101\">panicked: 1 vs 2</failure>",
            "",
        );
        for reports in [[REPORT, passed.as_str()], [passed.as_str(), REPORT]] {
            let mut all = BTreeMap::new();
            for r in reports {
                add(&mut all, r).unwrap();
            }
            assert_eq!(all.get("solidcraft-kernel::tests::fails"), Some(&true));
            assert_eq!(all.get("solidcraft-kernel::tests::passes"), Some(&false));
        }
        assert_eq!(outcomes(&passed).get("solidcraft-kernel::tests::fails"), Some(&false));
    }

    #[test]
    fn the_list_is_sorted_with_a_cause_each() {
        let none = BTreeMap::new();
        assert!(grade("b x\na y\n", &none).unwrap_err().contains("out of order"));
        assert!(grade("a x\na y\n", &none).unwrap_err().contains("a repeat of"));
        assert!(grade("a\n", &none).unwrap_err().contains("gives no cause"));
    }
}
