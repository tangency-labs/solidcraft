//! The ops profile hook (feature `profile`): each ops call's kernel counters, on closed forms.
//!
//! A box's validity check at its full level runs the kernel's topology coincidence test, whose
//! work follows from the box's topology alone (as the kernel's own test pins it,
//! `validation::coincidence`'s `cube_counts_distinguish_incidence_candidates_from_geometric_queries`):
//! - vertex against edge: each of the 12 edges has 2 end vertices, 24 candidates, all incident,
//!   so no geometric query;
//! - vertex against face: each of the 6 faces has 4 vertices, 24 candidates, no query;
//! - edge against face: each face meets 8 edges at an end (its 4 own and 4 transverse), 48
//!   candidates, and only the 24 transverse ones are queried; none is classified.
//! - edge against edge: each of the 12 edges is queried once against the edges with both its
//!   ends, of which a box has none: 0 candidates, 12 queries;
//! - vertex against vertex: the 8 vertices are all apart, no candidate and no query.
#![cfg(feature = "profile")]
#![allow(clippy::unwrap_used)]

use solidcore::profile::{Counter, Profile};
use solidcore_ops::session::{self, CallProfile};
use solidcore_ops::{make_box, CheckLevel};

/// Where `counter` sits in a profile's counter arrays.
fn at(counter: Counter) -> usize {
    Counter::ALL.iter().position(|&c| c == counter).unwrap()
}

/// The counter `counter` of the paths named `name`, outside their children, summed.
fn self_count(profile: &Profile, name: &str, counter: Counter) -> u64 {
    profile
        .summary
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.self_counters.get(at(counter)).copied().unwrap())
        .sum()
}

/// The call's own totals: its root scope's counter `counter`.
fn total(call: &CallProfile, counter: Counter) -> u64 {
    let root = call.profile.scopes.first().unwrap();
    assert_eq!(root.name, call.call);
    assert_eq!(root.parent, None);
    assert_eq!(root.calls, 1);
    root.counters.get(at(counter)).copied().unwrap()
}

#[test]
fn a_box_check_counts_its_coincidence_work_in_closed_form() {
    let body = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
    let (report, calls) = session::profiled(|| body.check(CheckLevel::All).unwrap());
    assert!(report.is_valid());
    assert_eq!(calls.len(), 1, "{calls:?}");
    let check = &calls[0];
    assert_eq!(check.call, "check");
    let p = &check.profile;
    for phase in ["vertex_edge", "vertex_face"] {
        let name = format!("validation::coincidence::{phase}");
        assert_eq!(
            self_count(p, &name, Counter::CoincidenceCandidates),
            24,
            "{name}"
        );
        assert_eq!(
            self_count(p, &name, Counter::CoincidenceQueries),
            0,
            "{name}"
        );
    }
    let edge_edge = "validation::coincidence::edge_edge";
    assert_eq!(self_count(p, edge_edge, Counter::CoincidenceCandidates), 0);
    assert_eq!(self_count(p, edge_edge, Counter::CoincidenceQueries), 12);
    let vertex_vertex = "validation::coincidence::vertex_vertex";
    assert_eq!(
        self_count(p, vertex_vertex, Counter::CoincidenceCandidates),
        0
    );
    assert_eq!(self_count(p, vertex_vertex, Counter::CoincidenceQueries), 0);
    let edge_face = "validation::coincidence::edge_face";
    assert_eq!(self_count(p, edge_face, Counter::CoincidenceCandidates), 48);
    assert_eq!(self_count(p, edge_face, Counter::CoincidenceQueries), 24);
    assert_eq!(
        self_count(p, edge_face, Counter::CoincidenceClassifications),
        0
    );
    // the call's totals: those, and nothing else counted (a box has no blend, no march)
    assert_eq!(total(check, Counter::CoincidenceCandidates), 24 + 24 + 48);
    assert_eq!(total(check, Counter::CoincidenceQueries), 24 + 12);
    for c in Counter::ALL {
        if !matches!(
            c,
            Counter::CoincidenceCandidates | Counter::CoincidenceQueries
        ) {
            assert_eq!(total(check, c), 0, "{c:?}");
        }
    }
}

#[test]
fn each_call_inside_profiled_has_its_own_profile() {
    let (body, calls) = session::profiled(|| {
        let body = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        body.check(CheckLevel::All).unwrap();
        body.check(CheckLevel::All).unwrap();
        body
    });
    let names: Vec<&str> = calls.iter().map(|c| c.call).collect();
    assert_eq!(names, ["create_box", "check", "check"]);
    // each check counted alone: the second does not carry the first's work
    for check in &calls[1..] {
        assert_eq!(total(check, Counter::CoincidenceCandidates), 96);
        assert_eq!(total(check, Counter::CoincidenceQueries), 36);
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
