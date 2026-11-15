//! The last commands each design ran, in memory only: what a problem report's "What you were
//! doing" attachment lists, when its user ticks it. Nothing here is written to disk or sent
//! anywhere; the report reads it and the user sees exactly what it reads.
//!
//! Only top-level commands that changed the design or failed are kept, like the crash journal:
//! a query or a click that did nothing says nothing about how a problem came about.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::Value;

/// How many commands are kept for each design.
pub const KEEP: usize = 30;
/// Room for one command's parameters, as JSON.
const MAX_PARAMS: usize = 2000;
/// Room for one command's result or error.
const MAX_RESULT: usize = 400;
/// Designs remembered at once (tabs); the oldest is forgotten first.
const MAX_DESIGNS: usize = 32;

/// One command that ran.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Recent {
    pub command: String,
    /// The parameters as given, as JSON (cut short when long).
    pub params: String,
    pub ok: bool,
    /// The result (cut short), or the error the command failed with.
    pub result: String,
}

static RECENT: Mutex<VecDeque<(u64, VecDeque<Recent>)>> = Mutex::new(VecDeque::new());

fn cut(mut s: String, limit: usize) -> String {
    if s.len() > limit {
        let mut end = limit;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let total = s.len();
        s.truncate(end);
        s.push_str(&format!("… ({total} bytes)"));
    }
    s
}

/// Remember a command the design `uid` ran.
pub(crate) fn record(uid: u64, command: &str, params: &Value, outcome: Result<&Value, &str>) {
    let entry = Recent {
        command: command.to_string(),
        params: cut(params.to_string(), MAX_PARAMS),
        ok: outcome.is_ok(),
        result: cut(
            match outcome {
                Ok(v) => v.to_string(),
                Err(e) => e.to_string(),
            },
            MAX_RESULT,
        ),
    };
    let Ok(mut all) = RECENT.lock() else { return };
    let at = match all.iter().position(|(u, _)| *u == uid) {
        Some(at) => at,
        None => {
            if all.len() >= MAX_DESIGNS {
                all.pop_front();
            }
            all.push_back((uid, VecDeque::new()));
            all.len() - 1
        }
    };
    if let Some((_, list)) = all.get_mut(at) {
        if list.len() >= KEEP {
            list.pop_front();
        }
        list.push_back(entry);
    }
}

/// The commands the design `uid` ran, oldest first (at most [`KEEP`]).
pub fn of(uid: u64) -> Vec<Recent> {
    RECENT.lock().ok().and_then(|all| all.iter().find(|(u, _)| *u == uid).map(|(_, list)| list.iter().cloned().collect())).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recent_commands_keep_the_last_thirty_per_design_bounded() {
        let uid = u64::MAX - 7;
        for i in 0..40 {
            record(uid, "solid.box", &json!({"i": i, "pad": "x".repeat(5000)}), Ok(&json!({"id": i})));
        }
        record(uid, "solid.fillet", &json!({}), Err("radius too large"));
        let list = of(uid);
        assert_eq!(list.len(), KEEP);
        assert!(list.iter().all(|r| r.params.len() < MAX_PARAMS + 32), "parameters are cut short");
        let last = list.last().unwrap();
        assert_eq!((last.command.as_str(), last.ok, last.result.as_str()), ("solid.fillet", false, "radius too large"));
        assert!(of(u64::MAX - 8).is_empty(), "another design's list is its own");
    }
}
