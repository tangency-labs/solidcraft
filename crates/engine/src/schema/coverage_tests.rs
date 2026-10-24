//! New commands come with schemas: every command has a contract (its input schema) and an
//! output schema, apart from `planned!` placeholders and the commands listed in
//! `unconverted.txt`. That list holds the commands that predate the rule; it only shrinks.

use std::collections::BTreeSet;

use serde_json::json;

use crate::Session;
use crate::cmd::command_specs;
use crate::schema::ParamCheck;
use crate::schema::check::MAX_KEYS;

const UNCONVERTED: &str = include_str!("unconverted.txt");

/// The ids in `unconverted.txt`, in file order (`#` lines are comments).
fn unconverted() -> Vec<&'static str> {
    UNCONVERTED.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

#[test]
fn every_command_has_input_and_output_schemas() {
    let listed: BTreeSet<&str> = unconverted().into_iter().collect();
    let mut problems = Vec::new();
    for c in command_specs() {
        let has = (c.contract.is_some(), c.output.is_some());
        let listed = listed.contains(c.id);
        if c.planned.is_some() {
            if listed {
                problems.push(format!("`{}` is a planned! placeholder, which needs no line in unconverted.txt", c.id));
            }
            continue;
        }
        match (has, listed) {
            ((true, true), true) => problems.push(format!("`{}` has its schemas now: remove its line from unconverted.txt", c.id)),
            ((true, true), false) | (_, true) => {}
            ((input, output), false) => problems.push(format!(
                "`{}` has no {} schema. Every new command needs a contract (`.contract(…)`, run through `checked!`) \
                 and an output type (`.output(…)`): see docs/book/src/dev/schemas.md",
                c.id,
                match (input, output) {
                    (true, _) => "output",
                    (false, true) => "input",
                    (false, false) => "input or output",
                }
            )),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A contract that is published but not checked would describe calls nobody refuses: every
/// command with a contract runs through `checked!` with its own id, which checks the contract
/// that id publishes. Calls `run` itself (not `Session::execute`, which first asks `enabled`)
/// in strict with an unknown key, which must be refused under the command's own id before the
/// handler sees it.
#[test]
fn every_contract_is_checked_in_front_of_its_handler() {
    const PROBE: &str = "not_a_key_of_any_command";
    for c in command_specs() {
        let Some(contract) = c.contract else { continue };
        // The check reads at most `MAX_KEYS` keys of a call, so a contract can't take more.
        let keys = contract.allowed_keys().len();
        assert!(keys <= MAX_KEYS, "`{}` takes {keys} keys; a call may have at most {MAX_KEYS}, so no call could use them all", c.id);
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        let run = c.run;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut s, &json!({PROBE: 1}))));
        let fix = format!("give its CommandSpec `crate::checked!(\"{}\", <its handler>)`", c.id);
        let error = match result {
            Ok(Err(e)) => e.to_string(),
            Ok(Ok(v)) => panic!("`{}` ran with an unknown key in strict ({v}): {fix}", c.id),
            Err(_) => panic!("`{}` panicked on an unknown key in strict: {fix}", c.id),
        };
        let expected = format!("invalid parameters for `{}`: unknown key `{PROBE}`", c.id);
        assert!(error.starts_with(&expected), "`{}` should refuse with \"{expected}\", not \"{error}\": {fix}", c.id);
    }
}

#[test]
fn the_unconverted_list_is_sorted_and_names_real_commands() {
    let ids = unconverted();
    for pair in ids.windows(2) {
        assert!(pair[0] < pair[1], "unconverted.txt must be sorted, each id once: `{}` comes before `{}`", pair[0], pair[1]);
    }
    let registered: BTreeSet<&str> = command_specs().iter().map(|c| c.id).collect();
    let stale: Vec<&str> = ids.into_iter().filter(|id| !registered.contains(id)).collect();
    assert!(stale.is_empty(), "unconverted.txt names commands that don't exist (remove their lines): {stale:?}");
}
