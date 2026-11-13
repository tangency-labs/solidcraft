//! The ops profile hook (feature `profile`): each ops call gets the kernel's profile of its own
//! work, checked on counts that follow in closed form.
//!
//! A cut decides the sign of its two operands' volumes and of its result's
//! (`mass::enclosure::volume_sign`, one call per decision, as `proof_reuse.rs` counts them):
//! a box less a cylinder through it makes 3 decisions. No operand and no result holds a blend,
//! so no blend definition is evaluated or solved.
#![cfg(feature = "profile")]
#![allow(clippy::unwrap_used)]

use solidcore::profile::Counter;
use solidcore_ops::boolean::boolean;
use solidcore_ops::session::{self, CallProfile};
use solidcore_ops::{make_box, make_cylinder, Body, BooleanOp, CheckLevel};

/// Where `counter` sits in a profile's counter arrays.
fn at(counter: Counter) -> usize {
    Counter::ALL.iter().position(|&c| c == counter).unwrap()
}

/// The call's own totals: its root scope's counter `counter`.
fn total(call: &CallProfile, counter: Counter) -> u64 {
    let root = call.profile.scopes.first().unwrap();
    assert_eq!(root.name, call.call);
    assert_eq!(root.parent, None);
    assert_eq!(root.calls, 1);
    root.counters.get(at(counter)).copied().unwrap()
}

/// The volume-sign decisions in `call`.
fn decisions(call: &CallProfile) -> u64 {
    call.profile
        .summary
        .iter()
        .filter(|s| s.name == "mass::enclosure::volume_sign")
        .map(|s| s.count)
        .sum()
}

/// A plate and a drill running past both its faces.
fn operands() -> (Body, Body) {
    (
        make_box([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]).unwrap(),
        make_cylinder([10.0, 10.0, -1.0], 2.0, 12.0).unwrap(),
    )
}

fn cut((a, b): (Body, Body)) -> Body {
    boolean(a, b, BooleanOp::Subtract).unwrap().unwrap()
}

#[test]
fn a_cut_counts_its_decisions_in_closed_form() {
    let pair = operands();
    let (_, calls) = session::profiled(|| cut(pair));
    assert_eq!(calls.len(), 1, "{calls:?}");
    let call = &calls[0];
    assert_eq!(call.call, "SmApiBoolean");
    assert_eq!(decisions(call), 3, "both operands and the result");
    for c in [
        Counter::Evaluations,
        Counter::Solves,
        Counter::NewtonSteps,
        Counter::NetEvaluations,
        Counter::TracePasses,
    ] {
        assert_eq!(total(call, c), 0, "{c:?}");
    }
}

#[test]
fn each_call_inside_profiled_has_its_own_profile() {
    let (first, second) = (operands(), operands());
    let (body, calls) = session::profiled(|| {
        cut(first);
        cut(second)
    });
    let names: Vec<&str> = calls.iter().map(|c| c.call).collect();
    assert_eq!(names, ["SmApiBoolean", "SmApiBoolean"]);
    // each cut counted alone: the second does not carry the first's decisions
    for call in &calls {
        assert_eq!(decisions(call), 3);
    }
    // outside profiled nothing is collected
    let ((), none) = session::profiled(|| ());
    assert!(none.is_empty());
    body.check(CheckLevel::All).unwrap();
    let ((), after) = session::profiled(|| ());
    assert!(after.is_empty());
}

#[test]
fn a_nested_profiled_returns_its_calls_and_the_outer_one_gets_them_too() {
    let body = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let ((inner, ()), outer) = session::profiled(|| {
        let r = session::profiled(|| body.check(CheckLevel::All).unwrap());
        body.check(CheckLevel::Basic).unwrap();
        (r.1, ())
    });
    assert_eq!(inner.len(), 1);
    assert_eq!(outer.len(), 2);
    assert_eq!(outer[0], inner[0]);
}
