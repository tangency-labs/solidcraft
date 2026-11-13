//! `cargo xtask known-failing --junit PATH... [--list FILE]`: grades a nextest run (its junit
//! report) against the tests known to fail today,
//! `tests/KNOWN_FAILING_TESTS` (or `--list FILE`). CI's test jobs run it after nextest, in place of
//! failing on any failed test. It fails on a test that failed
//! and is not listed, and on a listed test that passed (fixed: its line comes off in the same
//! pull request); a listed test that did not run (a pull request tests only the packages it
//! touches) is neither. The list has the kernel rig's format (`kernel_cases::known_failing`):
//! one test a line, `<test> <why it fails today>`, sorted, with `<test>` nextest's
//! `<binary id>::<test name>` (`solidcraft-kernel::tests::...`, or
//! `solidcraft-cli::action_matrix::action_matrix_sample` for an integration test binary).
//!
//! A flaky test goes on `tests/QUARANTINED_TESTS` (`--quarantine FILE`) instead:
//! `<test> <issue> <date added> [why]`. It still runs and is reported (in the log and the job
//! summary), but its failure never fails the gate, so it cannot fail a merge-queue batch. One
//! that passes is reported too, so it can be released. `cargo xtask lists` requires the issue
//! and fails an entry older than 14 days.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

pub const LIST: &str = "tests/KNOWN_FAILING_TESTS";
pub const QUARANTINE: &str = "tests/QUARANTINED_TESTS";
/// A test may stay quarantined this many days (`cargo xtask lists` fails after that).
const QUARANTINE_DAYS: i64 = 14;

pub fn run(root: &Path, rest: &[&str]) -> Result<(), String> {
    let mut reports = Vec::new();
    let mut list = root.join(LIST);
    let mut quarantine = root.join(QUARANTINE);
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--junit" => reports.push(PathBuf::from(args.next().ok_or("--junit needs a junit file or a directory of them")?)),
            "--list" => list = PathBuf::from(args.next().ok_or("--list needs a file")?),
            "--quarantine" => quarantine = PathBuf::from(args.next().ok_or("--quarantine needs a file")?),
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
    let qtext = std::fs::read_to_string(&quarantine).map_err(|e| format!("{}: {e}", quarantine.display()))?;
    let quarantined = parse_quarantine(&qtext).map_err(|e| format!("{}: {e}", quarantine.display()))?;
    let report = quarantine_report(&quarantined, &outcomes);
    if !report.is_empty() {
        println!("{}", report.join("\n"));
        if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(summary) {
                let _ = writeln!(f, "### Quarantined tests\n\n{}\n", report.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"));
            }
        }
    }
    outcomes.retain(|test, _| !quarantined.iter().any(|q| q.test == *test));
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
    let file = root.join(LIST);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let known = crate::kernel_cases::known_failing(&text).map_err(|e| format!("{LIST}: {e}"))?;
    let file = root.join(QUARANTINE);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let today = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| (d.as_secs() / 86_400) as i64);
    check_quarantine(&text, &known, today).map_err(|e| format!("{QUARANTINE}: {e}"))
}

/// The quarantine list parses, none of its tests is also known failing, and none is older than 14 days at `today`.
fn check_quarantine(text: &str, known: &[(&str, &str)], today: i64) -> Result<(), String> {
    let mut problems = Vec::new();
    for q in parse_quarantine(text)? {
        if known.iter().any(|(t, _)| *t == q.test) {
            problems.push(format!("{} is on both lists: a flaky test is quarantined, a test that always fails is known failing", q.test));
        }
        if today - q.added > QUARANTINE_DAYS {
            problems.push(format!(
                "{} has been quarantined {} days (limit {QUARANTINE_DAYS}): fix the test ({}) and remove the line, or take it off the list",
                q.test,
                today - q.added,
                q.issue
            ));
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n  ")) }
}

/// One line of the quarantine list.
#[derive(Debug)]
struct Quarantined<'a> {
    test: &'a str,
    issue: &'a str,
    /// Days since 1970-01-01 of the day it was added.
    added: i64,
}

/// The quarantine list: the known-failing format (sorted, each test once) with
/// `<issue> <YYYY-MM-DD> [why]` as the cause. The issue is `#N` or an issue URL.
fn parse_quarantine(text: &str) -> Result<Vec<Quarantined<'_>>, String> {
    let mut out = Vec::new();
    for (test, rest) in crate::kernel_cases::known_failing(text)? {
        let mut words = rest.split_whitespace();
        let issue = words.next().unwrap_or("");
        let number = issue.strip_prefix('#').or_else(|| issue.strip_prefix("https://github.com/tangency-labs/solidcraft/issues/"));
        if !number.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) {
            return Err(format!("{test}: needs its issue first, `#N` or an issue URL, then the date added (`<test> #N YYYY-MM-DD [why]`)"));
        }
        let added = words.next().and_then(parse_date).ok_or_else(|| format!("{test}: needs the date it was added, YYYY-MM-DD, after the issue"))?;
        out.push(Quarantined { test, issue, added });
    }
    Ok(out)
}

/// `YYYY-MM-DD` as days since 1970-01-01 (proleptic Gregorian).
fn parse_date(s: &str) -> Option<i64> {
    let mut p = s.split('-');
    let (y, m, d): (i64, i64, i64) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    if p.next().is_some() || s.len() != 10 || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// What each quarantined test did in this run, one line each; a test that did not run is left out.
fn quarantine_report(list: &[Quarantined], outcomes: &BTreeMap<String, bool>) -> Vec<String> {
    list.iter()
        .filter_map(|q| match outcomes.get(q.test)? {
            true => Some(format!("quarantined, failed: {}; see {}", q.test, q.issue)),
            false => Some(format!("quarantined, passed: {}; see {}, release it from tests/QUARANTINED_TESTS if it keeps passing", q.test, q.issue)),
        })
        .collect()
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

    use super::{add, check_quarantine, grade, junit_outcomes, parse_date, parse_quarantine, quarantine_report};

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

    #[test]
    fn a_quarantined_test_is_reported_and_needs_an_issue_a_date_and_to_be_under_14_days_old() {
        assert_eq!(parse_date("1970-01-02"), Some(1));
        assert_eq!(parse_date("2026-11-11"), Some(20_768));
        assert_eq!(parse_date("2026-13-01"), None);
        let list = "# flaky\n\
            solidcraft-kernel::tests::fails #12 2026-11-01 times out under load\n\
            solidcraft-kernel::tests::passes https://github.com/tangency-labs/solidcraft/issues/13 2026-11-10\n";
        let q = parse_quarantine(list).unwrap();
        let report = quarantine_report(&q, &outcomes(REPORT));
        assert_eq!(report.len(), 2, "{report:?}");
        assert!(report[0].starts_with("quarantined, failed: solidcraft-kernel::tests::fails; see #12"), "{report:?}");
        assert!(report[1].starts_with("quarantined, passed: solidcraft-kernel::tests::passes"), "{report:?}");
        let day = |s| parse_date(s).unwrap();
        check_quarantine(list, &[], day("2026-11-15")).unwrap();
        let old = check_quarantine(list, &[], day("2026-11-16")).unwrap_err();
        assert!(old.contains("fails has been quarantined 15 days") && !old.contains("passes"), "{old}");
        assert!(parse_quarantine("a why 2026-11-01\n").unwrap_err().contains("needs its issue"));
        assert!(parse_quarantine("a #5\n").unwrap_err().contains("needs the date"));
        assert!(parse_quarantine("a #5 2026-11-01x\n").unwrap_err().contains("needs the date"));
        let both = check_quarantine(list, &[("solidcraft-kernel::tests::fails", "x")], day("2026-11-11")).unwrap_err();
        assert!(both.contains("on both lists"), "{both}");
    }
}
