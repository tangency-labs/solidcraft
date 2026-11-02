//! Contracts and result types for the commands in `cmd/diagnostics.rs`: the crash reports on
//! this computer (#10909), the profiler and the validation mode.

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

/// `true`, `false` or null, with the handler's message for anything else.
fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(serde::de::Error::custom("must be true or false")),
    }
}

/// A row count, with the handler's message for anything else.
fn rows<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v.as_u64().map(Some).ok_or_else(|| serde::de::Error::custom("must be a whole number of rows (0: all)")),
    }
}

/// A file path, with the handler's message for anything else.
fn path<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) if !s.trim().is_empty() && s.len() < 4096 => Ok(Some(s)),
        _ => Err(serde::de::Error::custom("must be a file path")),
    }
}

/// What `diagnostics.profile` should do besides reporting.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProfileParams {
    /// Start (`true`) or stop (`false`) recording, after the report is taken.
    #[serde(default, deserialize_with = "flag")]
    pub enabled: Option<bool>,
    /// Forget what was recorded, after the report is taken.
    #[serde(default, deserialize_with = "flag")]
    pub reset: Option<bool>,
    /// How many rows to return, the most self time first (default 20; 0: all of them).
    #[serde(default, deserialize_with = "rows")]
    pub top: Option<u64>,
    /// Write the recorded spans here as a Chrome trace-event file (chrome://tracing, Perfetto).
    #[serde(default, deserialize_with = "path")]
    pub trace: Option<String>,
}

/// One category's share of the recorded self time.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileCategory {
    /// `smlib` (SMLib's own time, measured by its C API), `kernel`, `feature`, `doc`, `command`,
    /// `import`, `io`.
    pub category: String,
    pub self_ms: f64,
    /// Of all recorded self time, 0–1.
    pub share: f64,
}

/// One span name's aggregate.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileRow {
    pub category: String,
    pub name: String,
    pub count: u64,
    /// Its time, counting only its outermost occurrence on each thread's stack.
    pub total_ms: f64,
    /// Its time less the time of the spans inside it.
    pub self_ms: f64,
    /// Its longest single run.
    pub max_ms: f64,
}

/// What the profiler recorded: the reply of `diagnostics.profile`, and an import's `profile`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileReport {
    /// The profiler is compiled into this build (the apps' `profile` feature).
    pub compiled: bool,
    /// It is recording (after this call's `enabled`).
    pub enabled: bool,
    /// Wall time the report covers.
    pub wall_ms: f64,
    /// Spans recorded.
    pub spans: u64,
    /// Spans left out of the trace (past its cap); they are in the rows.
    pub dropped: u64,
    /// Self time per category, the most first.
    pub by_category: Vec<ProfileCategory>,
    /// The span names with the most self time, the most first.
    pub rows: Vec<ProfileRow>,
    /// Span names recorded (rows lists `top` of them).
    pub rows_total: usize,
    /// Where the Chrome trace was written, when `trace` asked for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
}

fn ms(d: std::time::Duration) -> f64 {
    (d.as_secs_f64() * 1e6).round() / 1e3
}

impl ProfileReport {
    /// `report` with its `top` rows of most self time (0: all of them).
    pub fn new(report: &solidcraft_profile::Report, top: usize) -> ProfileReport {
        let sorted = report.sorted();
        let all: f64 = report.rows.iter().map(|r| r.self_time.as_secs_f64()).sum();
        let by_category = report
            .by_category()
            .into_iter()
            .map(|(c, d)| ProfileCategory {
                category: c.to_string(),
                self_ms: ms(d),
                share: if all > 0.0 { (d.as_secs_f64() / all * 1000.0).round() / 1000.0 } else { 0.0 },
            })
            .collect();
        let n = if top == 0 { sorted.len() } else { top.min(sorted.len()) };
        let rows = sorted
            .iter()
            .take(n)
            .map(|r| ProfileRow {
                category: r.category.to_string(),
                name: r.name.to_string(),
                count: r.count,
                total_ms: ms(r.total),
                self_ms: ms(r.self_time),
                max_ms: ms(r.max),
            })
            .collect();
        ProfileReport {
            compiled: report.compiled,
            enabled: report.enabled,
            wall_ms: ms(report.wall),
            spans: (report.events as u64).saturating_add(report.dropped),
            dropped: report.dropped,
            by_category,
            rows,
            rows_total: sorted.len(),
            trace: None,
        }
    }
}

/// A validation mode by name, with the handler's message for anything else.
fn validation_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<ValidationMode>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) => ValidationMode::parse(&s).map(Some).ok_or_else(|| serde::de::Error::custom(VALIDATION_MODES)),
        _ => Err(serde::de::Error::custom(VALIDATION_MODES)),
    }
}

/// What a `mode` that names no validation mode is told.
pub(crate) const VALIDATION_MODES: &str = "must be each_step, each_feature or off";

/// When SMLib's full check runs while features are computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidationMode {
    /// After every sweep, boolean and blend inside a feature: a step whose result fails the
    /// check fails the feature there.
    EachStep,
    /// Once per feature (the default): each body the feature makes or changes is checked before
    /// it is kept, and a body the check refuses fails the feature with the check's reason.
    EachFeature,
    /// Never (experimental): a body the check would refuse is kept.
    Off,
}

impl ValidationMode {
    fn parse(id: &str) -> Option<ValidationMode> {
        solidcraft_kernel::Validation::parse(id).map(ValidationMode::from)
    }
}

impl From<solidcraft_kernel::Validation> for ValidationMode {
    fn from(v: solidcraft_kernel::Validation) -> Self {
        match v {
            solidcraft_kernel::Validation::EachStep => ValidationMode::EachStep,
            solidcraft_kernel::Validation::EachFeature => ValidationMode::EachFeature,
            solidcraft_kernel::Validation::Off => ValidationMode::Off,
        }
    }
}

/// Read, or set, which kernel answers every operation.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct KernelParams {
    /// The kernel to use from now on. Default: leave it as it is. `smlib` is refused on a build
    /// without the C++ kernel; `SOLIDCRAFT_KERNEL`, when set, wins over it.
    #[serde(default, deserialize_with = "kernel_choice")]
    pub choice: Option<KernelChoice>,
}

/// A geometry kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KernelChoice {
    /// solidcore, the Rust port of SMLib (the default).
    Solidcore,
    /// The C++ SMLib kernel.
    Smlib,
}

fn kernel_choice<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<KernelChoice>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) if s == "solidcore" => Ok(Some(KernelChoice::Solidcore)),
        Value::String(s) if s == "smlib" => Ok(Some(KernelChoice::Smlib)),
        _ => Err(serde::de::Error::custom(super::check::whole("`choice` is solidcore or smlib"))),
    }
}

/// The reply of `diagnostics.kernel`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KernelReply {
    /// The kernel in force now.
    pub choice: KernelChoice,
    /// Whether `SOLIDCRAFT_KERNEL` chose it.
    pub environment: bool,
}

/// Read, or set, when SMLib's full check runs on the features computed from now on.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ValidationParams {
    /// The mode to compute features under from now on. Default: leave it as it is. The results
    /// already there stand until their features are computed again (`timeline.compute_all`).
    #[serde(default, deserialize_with = "validation_mode")]
    pub mode: Option<ValidationMode>,
}

/// The reply of `diagnostics.validation`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ValidationReply {
    /// The mode features are computed under now.
    pub mode: ValidationMode,
    /// The mode before this call.
    pub previous: ValidationMode,
}

/// Tests that switch the process-wide profiler on, off or reset it run one at a time.
#[cfg(test)]
pub(crate) static PROFILE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
