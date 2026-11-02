//! A batch's steps checked before the first one runs (#5853).
//!
//! A batch runner (the control channel's `engine.batch`, the MCP `batch` tool) runs its steps in
//! order and stops at the first failure, going back to the design as it was. A misspelled
//! command id in a late step was found only at its turn, after every earlier step had run, and
//! then all of that work was undone: a 160-step batch with a typo as its last step ran for a
//! minute and changed nothing. [`check`] finds, before anything runs, the first step the batch
//! would certainly refuse at its turn, so the runner can refuse the batch unrun:
//!
//! - a step without a command id;
//! - a command id no command has (the error names the closest ids, as at the step's turn);
//! - with the session's [`ParamCheck::Strict`], parameters that break the command's typed
//!   contract (the contract check is the one the step would get; it reads only the parameters).
//!
//! Anything that depends on the design (a face that isn't there yet, an expression naming a
//! parameter an earlier step makes) is left to the step's turn, as are parameters that strict
//! checking would not refuse.

use serde_json::{Value, json};

use crate::schema::ParamCheck;
use crate::{EngineError, find_command};

/// The first step a batch would refuse at its turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub index: usize,
    pub command: Option<String>,
    pub error: String,
}

impl Refusal {
    /// The batch reply for a batch refused before its first step: as a failed step's reply,
    /// with nothing completed, nothing undone and every step not run.
    pub fn reply(&self, steps: usize) -> Value {
        json!({
            "completed": 0,
            "results": [],
            "failed": {"index": self.index, "command": self.command, "error": self.error},
            "undone": 0,
            "rolled_back": false,
            "not_run": (0..steps).collect::<Vec<_>>(),
            "note": "nothing was run: the batch was checked before its first step",
        })
    }
}

/// The first of `steps` (`{command | id, params?}`) that would be refused at its turn whatever
/// the steps before it do, checked against the command registry and, at `level`
/// [`ParamCheck::Strict`], the commands' typed contracts. `host` names ids the runner answers
/// itself, outside the registry (the app's `view.pick`).
pub fn check(steps: &[Value], level: ParamCheck, host: &[&str]) -> Option<Refusal> {
    for (index, step) in steps.iter().enumerate() {
        let Some(id) = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str) else {
            return Some(Refusal { index, command: None, error: "missing `command`".into() });
        };
        if host.contains(&id) {
            continue;
        }
        if find_command(id).is_none() {
            return Some(Refusal { index, command: Some(id.to_string()), error: EngineError::UnknownCommand(id.to_string()).to_string() });
        }
        if level == ParamCheck::Strict
            && let Some(refusal) = contract_refusal(index, id, step)
        {
            return Some(refusal);
        }
    }
    None
}

/// The first of `steps` whose parameters break its command's typed contract, for a runner in
/// another process that checks the rest itself (the MCP server bridged to an app at
/// [`ParamCheck::Strict`]): a step naming no command, or one this registry lacks (the app may be
/// another version), is left to it.
pub fn contracts(steps: &[Value]) -> Option<Refusal> {
    steps.iter().enumerate().find_map(|(index, step)| {
        let id = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)?;
        contract_refusal(index, id, step)
    })
}

/// Step `index`'s refusal when its parameters break the typed contract of its command `id`.
fn contract_refusal(index: usize, id: &str, step: &Value) -> Option<Refusal> {
    let first = crate::schema::issues(id, step.get("params").unwrap_or(&Value::Null)).into_iter().next()?;
    Some(Refusal { index, command: Some(id.to_string()), error: crate::params::bad(id, first.message).to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(command: &str, params: Value) -> Value {
        json!({"command": command, "params": params})
    }

    /// 40 circles extruded, as in the issue: 160 good steps.
    fn circles() -> Vec<Value> {
        (0..40)
            .flat_map(|k| {
                [
                    step("sketch.create", json!({"plane": "XY"})),
                    step("sketch.circle.center", json!({"center": [k * 7, (k % 5) * 3], "radius": 5})),
                    json!({"command": "sketch.finish"}),
                    step("solid.extrude", json!({"distance": 20 + k, "operation": "new"})),
                ]
            })
            .collect()
    }

    #[test]
    fn a_typo_in_the_last_step_is_found_before_the_first_runs() {
        let mut steps = circles();
        assert_eq!(check(&steps, ParamCheck::Off, &[]), None, "every step names a command");
        steps.push(step("solid.extrud", json!({"distance": 5})));
        let r = check(&steps, ParamCheck::Off, &[]).unwrap();
        assert_eq!((r.index, r.command.as_deref()), (160, Some("solid.extrud")));
        assert!(r.error.starts_with("unknown command `solid.extrud`") && r.error.contains("solid.extrude"), "{}", r.error);
        let reply = r.reply(steps.len());
        assert_eq!((reply["completed"].clone(), reply["undone"].clone(), reply["rolled_back"].clone()), (json!(0), json!(0), json!(false)));
        assert_eq!(reply["not_run"].as_array().map(Vec::len), Some(161), "nothing ran");
        assert_eq!(reply["failed"]["index"], 160);
    }

    #[test]
    fn a_step_without_a_command_and_the_first_bad_step_are_named() {
        let steps = vec![step("solid.box", json!({"length": 1, "width": 1, "height": 1})), json!({"params": {}}), step("nope.nope", json!({}))];
        assert_eq!(check(&steps, ParamCheck::Off, &[]), Some(Refusal { index: 1, command: None, error: "missing `command`".into() }));
        // `id` is read as `command` is, and an id the runner answers itself passes.
        let steps = vec![json!({"id": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}), step("view.pick", json!({"x": 1, "y": 1}))];
        assert_eq!(check(&steps, ParamCheck::Off, &["view.pick"]), None);
        assert_eq!(check(&steps, ParamCheck::Off, &[]).map(|r| r.index), Some(1), "outside the registry without the runner");
    }

    #[test]
    fn strict_checking_refuses_a_broken_contract_before_the_first_step() {
        let steps =
            vec![step("solid.box", json!({"length": 1, "width": 1, "height": 1})), step("solid.box", json!({"lenght": 1, "width": 1, "height": 1}))];
        // Off and warn run such a step (warn reports it): nothing to refuse up front.
        assert_eq!(check(&steps, ParamCheck::Off, &[]), None);
        assert_eq!(check(&steps, ParamCheck::Warn, &[]), None);
        let r = check(&steps, ParamCheck::Strict, &[]).unwrap();
        assert_eq!(r.index, 1);
        assert!(r.error.contains("solid.box") && r.error.contains("lenght"), "{}", r.error);
        // The same call alone is refused with the same words at its turn.
        let mut s = crate::Session { param_check: ParamCheck::Strict, ..crate::Session::default() };
        let at_turn = s.execute("solid.box", &steps[1]["params"]).unwrap_err().to_string();
        assert_eq!(r.error, at_turn);
        // Another process's runner gets the same refusal, and keeps what it checks itself.
        assert_eq!(contracts(&steps), Some(r));
        let theirs = vec![json!({"params": {}}), step("app.only", json!({"x": 1})), step("view.pick", json!({}))];
        assert_eq!(contracts(&theirs), None);
    }
}
