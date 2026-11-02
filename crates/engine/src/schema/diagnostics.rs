//! Contracts and result types for the commands in `cmd/diagnostics.rs`: the crash reports on
//! this computer (#10909).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

/// List this computer's crash reports, newest first. They stay on this computer and are never
/// sent anywhere. Read one with `diagnostics.crash_report`; reproduce one with
/// `solidcraft-cli crashes replay <id>`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CrashReportsParams {
    /// How many reports to list, newest first. Default: 20; at most 1000 are listed, whatever
    /// the limit.
    #[serde(default)]
    pub limit: Option<u64>,
}

/// What `diagnostics.crash_reports` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CrashReportList {
    /// The crash report folder on this computer.
    pub dir: String,
    /// The reports, newest first.
    pub reports: Vec<crate::crash::Summary>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Read one crash report: the whole report as it was saved, with the paths of its files.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CrashReportParams {
    /// The report's id, as `diagnostics.crash_reports` lists it.
    pub id: String,
}

/// What `diagnostics.crash_report` returns: the report as saved, with where its files are. A
/// report from an earlier release may lack some of these fields, and a report can carry more
/// (`startup_phase`, `native`).
#[derive(Debug, Serialize)]
pub struct CrashReport {
    /// The report's fields.
    #[serde(flatten)]
    pub report: Map<String, Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

impl JsonSchema for CrashReport {
    fn schema_name() -> Cow<'static, str> {
        "CrashReport".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let path =
            |what: &str| json!({"description": format!("Where the report's {what} is on this computer; only when it has one."), "type": "string"});
        json_schema!({
            "description": "A crash report as saved, with where its files are. A report from an earlier release may lack some fields, and a report can carry more.",
            "type": "object",
            "properties": {
                "id": {"description": "The report's id.", "type": "string"},
                "kind": {"description": "command_panic, panic, kernel_panic_recovered or unclean_exit.", "type": "string"},
                "signature": {"description": "What tells this crash apart from others, so a repeat counts against the same report."},
                "count": {"description": "How many times it happened.", "type": "integer", "minimum": 0},
                "first_seen": {"description": "When it was first recorded (UTC). An unclean exit is recorded at a later launch."},
                "last_seen": {"description": "When it was last recorded (UTC)."},
                "app": {"description": "The app's version, commit and build date."},
                "system": {"description": "The operating system, architecture and graphics environment."},
                "session": {"description": "How long the app had been running."},
                "message": {"description": "The internal error's message."},
                "location": {"description": "Where in the code it happened."},
                "thread": {"description": "The thread it happened on."},
                "backtrace": {"description": "The stack, one frame `symbol (file:line)` each."},
                "command": {"description": "The command running when it happened, with its parameters, if one was."},
                "base": {"description": "What the replay starts from."},
                "recent_commands": {"description": "The commands run before it."},
                "files": {"description": "The files saved with the report, by what they are."},
                "reproduce": {"description": "The command line that replays the crash, when it can be."},
                "notes": {"description": "What the report could not record, and why."},
                "path": {"description": "The report's folder on this computer.", "type": "string"},
                "design_path": path("design"),
                "replay_path": path("replay (`solidcraft-cli run <replay_path> --in <design_path>`)"),
                "minidump_path": path("minidump"),
                "param_warnings": {"description": "In the warn parameter-check mode: what the call's parameters got wrong.", "type": "array", "items": {"type": "string"}}
            }
        })
    }
}

/// Delete one crash report, or all of them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CrashReportsClearParams {
    /// The report to delete, as `diagnostics.crash_reports` lists it. Default: every report. An
    /// id that names no report deletes nothing.
    #[serde(default)]
    pub id: Option<String>,
}

/// What `diagnostics.crash_reports.clear` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CrashReportsCleared {
    /// How many reports were deleted.
    pub deleted: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Check that crash reports work, by crashing on purpose. Only when the SOLIDCRAFT_CRASH_TEST
/// environment variable is set.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CrashTestParams {
    /// What to break.
    pub mode: CrashTestMode,
}

/// What `diagnostics.crash_test` breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CrashTestMode {
    /// An internal error in this command, caught: the design is unchanged.
    Panic,
    /// An internal error inside the geometry kernel, caught.
    Kernel,
    /// The app ends at once; the report is made at the next start.
    Abort,
}

impl<'de> Deserialize<'de> for CrashTestMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if s == "panic" => Ok(CrashTestMode::Panic),
            Value::String(s) if s == "kernel" => Ok(CrashTestMode::Kernel),
            Value::String(s) if s == "abort" => Ok(CrashTestMode::Abort),
            _ => Err(D::Error::custom("must be panic, kernel or abort")),
        }
    }
}

/// What `diagnostics.crash_test` would return. It never succeeds: it ends in an internal error
/// that is caught and reported (`panic`, `kernel`), or it ends the app (`abort`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct CrashTestResult {}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
