//! Crash reports, for agents and people (see `crate::crash` and docs/crash-reports.md): list
//! them, read one, delete them. They live on this computer and are never sent anywhere.

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
    CommandSpec::new("diagnostics.crash_test", "Crash Test", crate::checked!("diagnostics.crash_test", crash_test))
        .icon("bug_report")
        .noundo()
        .enabled(crash_test_enabled)
        .contract(&CRASH_TEST_CONTRACT)
        .output(output_schema_for::<CrashTestResult>)
        .params(
            "mode: panic | kernel | abort. Checks that crash reports work: panic = an internal error in this command (caught; \
             the design is unchanged), kernel = one inside the geometry kernel (caught), abort = the app ends at once (reported \
             at the next start). Only when the SOLIDCRAFT_CRASH_TEST environment variable is set",
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

// The one deliberate panic: it proves the crash reports work, and runs only when asked for with
// SOLIDCRAFT_CRASH_TEST.
#[allow(clippy::panic)]
fn crash_test(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "diagnostics.crash_test";
    match str_(p, "mode") {
        Some("panic") => std::panic::panic_any(String::from("crash test: an internal error in a command")),
        Some("kernel") => {
            solidcraft_kernel::guard("crash test", || -> solidcraft_kernel::Result<()> {
                std::panic::panic_any(String::from("an internal error in the kernel"))
            })?;
            Err(EngineError::Other("crash test: the kernel error was not caught".into()))
        }
        Some("abort") => std::process::abort(),
        _ => Err(bad(cmd, "`mode` must be panic, kernel or abort")),
    }
}
