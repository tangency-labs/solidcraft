//! Parameter edits in a batch, rebuilt once (#5822).
//!
//! An agent often sends a batch of `parameters.change` steps, one parameter each. Every step
//! evaluates the design again, and an edit early in the timeline rebuilds nearly all of it: a
//! batch of 21 such edits ran for minutes. `parameters.change {changes}` sets several parameters
//! with one rebuild. With `coalesce_parameters`, a batch runner sends each run of consecutive
//! plain edits (a name and an expression, or `changes`) as one such step. When that combined
//! step is refused (a step makes a new parameter, which `changes` can't; an expression is
//! wrong), the runner runs the run's steps one by one as before, so a refused edit is still
//! named by its own step. Any other step (a sketch, an extrude, a rename) ends a run, so it sees
//! the design up to date. A run skips the states between its edits: an edit refused on its own
//! can go through together with the run's later edits.

use serde_json::{Map, Value, json};

/// How a batch's steps run.
#[derive(Clone, Debug, PartialEq)]
pub enum BatchRun {
    /// One step as it is (its index).
    Step(usize),
    /// Consecutive plain parameter edits (their indices, at least two) as one
    /// `parameters.change {changes}`: the parameters and expressions they set, the later of two
    /// for one parameter.
    Params { steps: Vec<usize>, changes: Map<String, Value> },
}

/// A step's command id (`command`, or `id` as the control channel also takes).
fn command(step: &Value) -> Option<&str> {
    step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)
}

/// The parameters a plain `parameters.change` step sets, with their expressions: `{name,
/// expression}` (or `value`), or `{changes}`. `None` for any other step or form (a rename, a
/// unit, a comment, a deletion), which then runs as it is.
fn plain_edit(step: &Value) -> Option<Map<String, Value>> {
    if command(step)? != "parameters.change" {
        return None;
    }
    let p = step.get("params")?.as_object()?;
    let expression = |v: &Value| matches!(v, Value::Number(_) | Value::String(_));
    if let Some(changes) = p.get("changes") {
        let changes = changes.as_object().filter(|c| !c.is_empty() && c.values().all(expression))?;
        return (p.len() == 1).then(|| changes.clone());
    }
    let name = p.get("name")?.as_str()?.trim();
    let e = p.get("expression").or_else(|| p.get("value")).filter(|e| expression(e))?;
    let plain = p.keys().all(|k| matches!(k.as_str(), "name" | "expression" | "value")) && !name.is_empty();
    plain.then(|| Map::from_iter([(name.to_string(), e.clone())]))
}

/// How `steps` run: each on its own, or with `coalesce` each run of two or more consecutive
/// plain parameter edits as one.
pub fn plan(steps: &[Value], coalesce: bool) -> Vec<BatchRun> {
    let mut out = Vec::new();
    let mut run: Vec<(usize, Map<String, Value>)> = Vec::new();
    let flush = |run: &mut Vec<(usize, Map<String, Value>)>, out: &mut Vec<BatchRun>| match run.len() {
        0 => {}
        1 => out.extend(run.drain(..).map(|(i, _)| BatchRun::Step(i))),
        _ => {
            let mut changes = Map::new();
            let mut indices = Vec::with_capacity(run.len());
            for (i, edit) in run.drain(..) {
                indices.push(i);
                // A later edit of a parameter replaces an earlier one, as it would have.
                for (name, e) in edit {
                    changes.remove(&name);
                    changes.insert(name, e);
                }
            }
            out.push(BatchRun::Params { steps: indices, changes });
        }
    };
    for (i, step) in steps.iter().enumerate() {
        match plain_edit(step).filter(|_| coalesce) {
            Some(edit) => run.push((i, edit)),
            None => {
                flush(&mut run, &mut out);
                out.push(BatchRun::Step(i));
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The combined step: `parameters.change {changes}`.
pub fn combined_params(changes: &Map<String, Value>) -> Value {
    json!({"changes": changes})
}

/// Each step's result from the combined step's `result`: what it set, the value now, and what
/// the one rebuild did (`coalesced`: how many steps it took). In the steps' order.
pub fn step_results(steps: &[Value], indices: &[usize], result: &Value) -> Vec<Value> {
    let changed = result.get("changed").and_then(Value::as_object);
    indices
        .iter()
        .map(|i| {
            let set = steps.get(*i).and_then(plain_edit).unwrap_or_default();
            let values: Map<String, Value> =
                set.keys().map(|n| (n.clone(), changed.and_then(|c| c.get(n)).cloned().unwrap_or(Value::Null))).collect();
            let mut r = json!({
                "changed": values,
                "coalesced": indices.len(),
                "recomputed": result.get("recomputed").cloned().unwrap_or(Value::Null),
                "restored": result.get("restored").cloned().unwrap_or(Value::Null),
                "errors": result.get("errors").cloned().unwrap_or(Value::Null),
            });
            // A one-parameter step answers as `parameters.change {name, expression}` does.
            if let (Some((name, e)), 1) = (set.iter().next(), set.len())
                && let Some(o) = r.as_object_mut()
            {
                o.insert("name".into(), json!(name));
                o.insert("expression".into(), e.clone());
                o.insert("value".into(), values.get(name).cloned().unwrap_or(Value::Null));
            }
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(name: &str, e: Value) -> Value {
        json!({"command": "parameters.change", "params": {"name": name, "expression": e}})
    }

    #[test]
    fn runs_of_plain_edits_join_and_anything_else_ends_them() {
        let steps = vec![
            edit("a", json!("2 mm")),
            edit("b", json!(3)),
            json!({"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}),
            edit("c", json!("4")),
            json!({"id": "parameters.change", "params": {"changes": {"d": "5", "a": "6 mm"}}}),
            edit("a", json!("7 mm")),
            json!({"command": "parameters.change", "params": {"name": "e", "new_name": "f"}}),
            edit("g", json!("1")),
        ];
        let got = plan(&steps, true);
        let mut changes = Map::new();
        changes.insert("c".into(), json!("4"));
        changes.insert("d".into(), json!("5"));
        changes.insert("a".into(), json!("7 mm"));
        let mut first = Map::new();
        first.insert("a".into(), json!("2 mm"));
        first.insert("b".into(), json!(3));
        assert_eq!(
            got,
            vec![
                BatchRun::Params { steps: vec![0, 1], changes: first },
                BatchRun::Step(2),
                BatchRun::Params { steps: vec![3, 4, 5], changes },
                BatchRun::Step(6),
                BatchRun::Step(7),
            ]
        );
        // Off (the default): every step on its own.
        assert_eq!(plan(&steps, false), (0..steps.len()).map(BatchRun::Step).collect::<Vec<_>>());
    }

    #[test]
    fn edits_with_more_than_a_name_and_an_expression_run_as_they_are() {
        for params in [
            json!({"name": "a", "expression": "1", "unit": "mm"}),
            json!({"name": "a", "expression": "1", "comment": "x"}),
            json!({"name": "a", "expression": [1]}),
            json!({"delete": "a"}),
            json!({"changes": {}}),
            json!({"changes": {"a": "1"}, "unit": "mm"}),
            json!({"name": " ", "expression": "1"}),
        ] {
            let steps = vec![edit("b", json!("1")), json!({"command": "parameters.change", "params": params}), edit("c", json!("1"))];
            assert_eq!(plan(&steps, true), vec![BatchRun::Step(0), BatchRun::Step(1), BatchRun::Step(2)], "{params}");
        }
    }

    #[test]
    fn each_step_gets_its_own_result() {
        let steps = vec![edit("a", json!("2 mm")), json!({"command": "parameters.change", "params": {"changes": {"b": "3", "c": "4"}}})];
        let result = json!({"changed": {"a": 2.0, "b": 3.0, "c": 4.0}, "recomputed": 5, "restored": 0, "errors": []});
        let rows = step_results(&steps, &[0, 1], &result);
        assert_eq!(rows[0]["name"], "a");
        assert_eq!(rows[0]["value"], 2.0);
        assert_eq!(rows[0]["coalesced"], 2);
        assert_eq!(rows[1]["changed"], json!({"b": 3.0, "c": 4.0}));
        assert_eq!(rows[1]["recomputed"], 5);
        assert!(rows[1].get("name").is_none());
    }
}
