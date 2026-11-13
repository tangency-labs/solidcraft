//! The audit recorder is native-only; in the browser it is never on.

use serde_json::Value;

use super::check::Issue;
use crate::Result;

pub fn enabled() -> bool {
    false
}

pub fn note(_command: &str, _issues: &[Issue], _outcome: Option<&Result<Value>>, _params: &Value) {}
