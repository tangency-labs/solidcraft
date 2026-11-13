//! Crash reports, for agents and people (see `crate::crash` and docs/crash-reports.md): list
//! them, read one, delete them. They live on this computer and are never sent anywhere. And the
//! profiler (`solidcraft_profile`, docs/perf.md): where the time goes, SMLib's calls to commands.

use serde_json::Value;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::diagnostics::{
    CrashReport, CrashReportList, CrashReportParams, CrashReportsClearParams, CrashReportsCleared, CrashReportsParams, CrashTestParams,
    CrashTestResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session, crash};

const LIST_CONTRACT: TypedContract<CrashReportsParams> = TypedContract::new();
const READ_CONTRACT: TypedContract<CrashReportParams> = TypedContract::new();
const CLEAR_CONTRACT: TypedContract<CrashReportsClearParams> = TypedContract::new();
const CRASH_TEST_CONTRACT: TypedContract<CrashTestParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("diagnostics.crash_reports", "Crash Reports", crate::checked!("diagnostics.crash_reports", list))
        .icon("bug_report")
        .noundo()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<CrashReportList>)
        .params(
            "limit?: number (default 20). Lists this computer's crash reports, newest first: {dir, reports: [{id, kind, time, message, \
             command, count, version, path, replayable}]}. kind: command_panic | panic | kernel_panic_recovered | unclean_exit. \
             Read one with diagnostics.crash_report; reproduce it with `solidcraft-cli crashes replay <id>`",
        ),
    CommandSpec::new("diagnostics.crash_report", "Crash Report", crate::checked!("diagnostics.crash_report", read))
        .icon("bug_report")
        .noundo()
        .contract(&READ_CONTRACT)
        .output(output_schema_for::<CrashReport>)
        .params(
            "id: report id. The whole report: message, location, backtrace, app version and commit, system, the command that \
             crashed and the recent ones, notes, and design_path / replay_path when the crash can be replayed \
             (`solidcraft-cli run <replay_path> --in <design_path>`)",
        ),
    CommandSpec::new("diagnostics.crash_reports.clear", "Delete Crash Reports", crate::checked!("diagnostics.crash_reports.clear", clear))
        .icon("delete")
        .noundo()
        .contract(&CLEAR_CONTRACT)
        .output(output_schema_for::<CrashReportsCleared>)
        .params("id?: one report (default: all). Returns {deleted}"),
    CommandSpec::new("diagnostics.profile", "Profile", crate::checked!("diagnostics.profile", profile))
        .icon("bug_report")
        .noundo()
        .contract(&PROFILE_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::diagnostics::ProfileReport>)
        .params(
            "enabled?: bool (start or stop recording); reset?: bool (forget what was recorded); top?: rows (default 20, 0: all); \
             trace?: path (write the recorded spans as a Chrome trace-event file, for chrome://tracing or Perfetto). Returns what \
             was recorded before enabled and reset apply: {compiled, enabled, wall_ms, spans, dropped, by_category: [{category, \
             self_ms, share}], rows: [{category, name, count, total_ms, self_ms, max_ms}], rows_total, trace?}, the rows the most \
             self time first. Categories: solidcore (the kernel's calls), kernel, feature (one per feature \
             computed, by its type), doc, command, import (STEP/IGES stages), io. Self time is a span's time less the spans \
             inside it; total counts its outermost run only. Recording also starts with SOLIDCRAFT_PROFILE=1, the CLI's \
             --profile and the app's preference; off, it costs nothing measurable. Read-only",
        ),
    CommandSpec::new("diagnostics.validation", "Validation", crate::checked!("diagnostics.validation", validation))
        .icon("bug_report")
        .noundo()
        .contract(&VALIDATION_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::diagnostics::ValidationReply>)
        .params(
            "mode?: each_step | each_feature | off (default: leave it). When the kernel's full check runs on the features computed \
             from now on: after every sweep, boolean and blend (each_step), once on the bodies each feature makes or changes \
             (each_feature, the default; a body it refuses still fails its feature, with the check's reason), or never (off, \
             experimental). Results already there stand until their features are computed again (timeline.compute_all). \
             The app's preference of the same name sets it. Returns {mode, previous}",
        ),
    CommandSpec::new("diagnostics.crash_test", "Crash Test", crate::checked!("diagnostics.crash_test", crash_test))
        .icon("bug_report")
        .noundo()
        .enabled(crash_test_enabled)
        .contract(&CRASH_TEST_CONTRACT)
        .output(output_schema_for::<CrashTestResult>)
        .params(
            "mode: panic | kernel | worker | worker_raw | abort. Checks that crash reports work: panic = an internal error in this \
             command (caught; the design is unchanged), kernel = one inside the geometry kernel (caught), worker / worker_raw = \
             one on a Rayon worker, caught inside / outside the kernel's guard, abort = the app ends at once (reported at the \
             next start). Only when the SOLIDCRAFT_CRASH_TEST environment variable is set",
        ),
];

fn dir(cmd: &str) -> Result<std::path::PathBuf> {
    crash::default_dir().ok_or_else(|| bad(cmd, "no crash folder on this system"))
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn list(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.crash_reports";
    let d = dir(cmd)?;
    let limit = p.get("limit").and_then(Value::as_u64).map_or(20, |n| n.min(1000)) as usize;
    let mut reports = crash::list(&d);
    reports.truncate(limit);
    reply(cmd, CrashReportList { dir: d.to_string_lossy().into_owned(), reports, param_warnings: Vec::new() })
}

fn read(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.crash_report";
    let id = str_(p, "id").ok_or_else(|| bad(cmd, "`id` is required (see diagnostics.crash_reports)"))?;
    // `crash::read` gives only a report that is a JSON object.
    let report = match crash::read(&dir(cmd)?, id).map_err(|e| bad(cmd, e))? {
        Value::Object(report) => report,
        _ => return Err(bad(cmd, format!("no crash report `{id}`"))),
    };
    reply(cmd, CrashReport { report, param_warnings: Vec::new() })
}

fn clear(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.crash_reports.clear";
    let d = dir(cmd)?;
    reply(cmd, CrashReportsCleared { deleted: crash::clear(&d, str_(p, "id")), param_warnings: Vec::new() })
}

fn crash_test_enabled(_: &Session) -> std::result::Result<(), String> {
    if solidcraft_geom::env::var_os("CRASH_TEST").is_some() { Ok(()) } else { Err("set SOLIDCRAFT_CRASH_TEST to use the crash test".into()) }
}

/// `diagnostics.crash_test`: the one deliberate panic. It proves the crash reports work, and runs
/// only when asked for with SOLIDCRAFT_CRASH_TEST. `mode` picks where the internal error happens
/// ([`CrashTestMode`](crate::schema::diagnostics::CrashTestMode)); every mode but `abort` ends
/// in a caught error.
#[allow(clippy::panic)]
fn crash_test(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.crash_test";
    match str_(p, "mode") {
        Some("panic") => std::panic::panic_any(String::from("crash test: an internal error in a command")),
        Some("kernel") => {
            solidcraft_kernel::crash_test()?;
            Err(EngineError::Other("crash test: the kernel error was not caught".into()))
        }
        Some("worker") => {
            solidcraft_kernel::crash_test_worker_panic("an internal error in the kernel on a worker")?;
            Err(EngineError::Other("crash test: the kernel error was not caught".into()))
        }
        Some("worker_raw") => {
            solidcraft_kernel::crash_test_worker_panic_raw("a worker panic in a command")?;
            Err(EngineError::Other("crash test: the worker panic was not resumed".into()))
        }
        Some("abort") => std::process::abort(),
        _ => Err(bad(cmd, "`mode` must be panic, kernel, worker, worker_raw or abort")),
    }
}

const PROFILE_CONTRACT: crate::schema::TypedContract<crate::schema::diagnostics::ProfileParams> = crate::schema::TypedContract::new();

const VALIDATION_CONTRACT: crate::schema::TypedContract<crate::schema::diagnostics::ValidationParams> = crate::schema::TypedContract::new();

fn validation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.validation";
    let mode = match p.get("mode") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(solidcraft_kernel::Validation::parse)
                .ok_or_else(|| bad(cmd, format!("`mode` {}", crate::schema::diagnostics::VALIDATION_MODES)))?,
        ),
    };
    let previous = s.validation();
    if let Some(mode) = mode {
        s.set_validation(mode);
    }
    let out = crate::schema::diagnostics::ValidationReply { mode: s.validation().into(), previous: previous.into() };
    serde_json::to_value(out).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))
}

/// Rows `diagnostics.profile` returns at most.
const MAX_PROFILE_ROWS: u64 = 10_000;

fn profile(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.profile";
    let flag = |key: &str| match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    };
    let (enabled, reset) = (flag("enabled")?, flag("reset")?);
    let top = match p.get("top") {
        None | Some(Value::Null) => 20,
        Some(v) => v.as_u64().ok_or_else(|| bad(cmd, "`top` must be a whole number of rows (0: all)"))?.min(MAX_PROFILE_ROWS) as usize,
    };
    let trace = match p.get("trace") {
        None | Some(Value::Null) => None,
        Some(_) => Some(str_(p, "trace").filter(|t| !t.trim().is_empty() && t.len() < 4096).ok_or_else(|| bad(cmd, "`trace` must be a file path"))?),
    };
    let report = solidcraft_profile::report();
    let mut out = crate::schema::diagnostics::ProfileReport::new(&report, top);
    if let Some(path) = trace {
        let mut bytes = Vec::new();
        solidcraft_profile::write_chrome_trace(&mut bytes).map_err(|e| bad(cmd, format!("trace: {e}")))?;
        solidcraft_io::vfs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        out.trace = Some(path.to_string());
    }
    if reset == Some(true) {
        solidcraft_profile::reset();
    }
    if let Some(on) = enabled {
        solidcraft_profile::set_enabled(on);
        out.enabled = solidcraft_profile::enabled();
    }
    serde_json::to_value(out).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))
}
