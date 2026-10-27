//! Contracts and result types for the commands in `cmd/diagnostics.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Tests that switch the process-wide profiler on, off or reset it run one at a time.
#[cfg(test)]
pub(crate) static PROFILE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
