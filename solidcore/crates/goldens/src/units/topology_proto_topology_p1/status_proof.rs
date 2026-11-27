//! Proof tests for the proto statuses SMLib drops and solidcore passes on (WR-662) and
//! for `RemoveCoincidentNoEdgeProtoVertex`'s answer on a miss (WR-73), #18927.
//!
//! Each test replays every case of the three proto-topology units (none is picked out)
//! with the run's [`witness`] on and checks an invariant on what each step did:
//!
//! - a proto vertex removal answers `true` exactly when it removed the proto vertex
//!   from the manager (WR-73: a miss removes nothing, so it answers `false`);
//! - a manager list lookup that misses (`NotOnOwnerList`) in a proto-topology script only
//!   happens on a state whose back-pointer lists a script op broke (the library keeps them
//!   consistent; the list scripts remove absent members on purpose);
//! - every row the retirement moved (`status_rows.tsv`, every row whose digest differs
//!   from main 6ea034c437's) records a step error of the class the table names, so its
//!   new status is that real failed step; a row with no failed step moved a WR-73 answer
//!   (a miss, now `false`) or is one of the [`OBJECT_POINT_ROWS`].

use std::collections::BTreeMap;

use solidcore::error::topology::TopologyFailure;
use solidcore::error::Reason;
use solidcore::SolidError;

use super::witness;
use crate::case::{repo_root, CaseContext};

/// The three proto-topology units the scripts drive.
const UNITS: [&str; 3] = [
    "topology.proto_topology_p1",
    "topology.proto_topology_p2",
    "topology.proto_topology_p3",
];

/// The moved rows with no failed step and no WR-73 miss: in each, a vertex definition is
/// resolved with a classification that has no object point (no parameters, or a region),
/// where main read upstream's unset point (`FindObjPoint3d` failing, its status ignored)
/// and the retirement reads the definition's own position (`definition_object_point`).
/// Found by an instrumented replay of every case (the fallback fired in each of these and
/// these are the only quiet rows); the test pins the set, so any other quiet row fails.
const OBJECT_POINT_ROWS: [(&str, &str); 11] = [
    ("topology.proto_topology_p1", "gen0220"),
    ("topology.proto_topology_p1", "gen0778"),
    ("topology.proto_topology_p1", "gen1114"),
    ("topology.proto_topology_p1", "gen1454"),
    ("topology.proto_topology_p2", "gen0814"),
    ("topology.proto_topology_p2", "gen1193"),
    ("topology.proto_topology_p3", "gen0283"),
    ("topology.proto_topology_p3", "gen0953"),
    ("topology.proto_topology_p3", "gen1594"),
    ("topology.proto_topology_p3", "gen2178"),
    ("topology.proto_topology_p3", "gen2623"),
];

/// Each case of the three units with what its run recorded, by `(unit, case)`.
fn replay_all() -> BTreeMap<(String, String), witness::Log> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 0,
    };
    let mut logs = BTreeMap::new();
    for unit in crate::registry() {
        if !UNITS.contains(&unit.id()) {
            continue;
        }
        for case in unit.cases(&ctx).unwrap() {
            witness::start();
            let _ = unit.run_rust(&case);
            logs.insert((unit.id().to_string(), case.name.clone()), witness::take());
        }
    }
    logs
}

/// The class of a step error the row table names: the failing stage's operation and the
/// reason's variant, without its numbers.
fn class(e: &SolidError) -> String {
    match e.failure() {
        Some(f) => {
            let reason = format!("{:?}", f.reason);
            let variant = reason.split(['{', ' ']).next().unwrap_or_default();
            format!("{} {variant}", f.operation)
        }
        None => format!("{:?}", e.kind()),
    }
}

fn not_on_owner_list(e: &SolidError) -> bool {
    std::iter::successors(Some(e), |e| e.cause()).any(|e| {
        e.failure().is_some_and(|f| {
            matches!(
                f.reason,
                Reason::Topology(TopologyFailure::NotOnOwnerList)
            )
        })
    })
}

#[test]
fn status_proof_replays() {
    let logs = replay_all();
    assert!(logs.len() > 1000, "every case of the three units ran: {}", logs.len());

    // WR-73: the answer is whether the proto vertex was removed
    let mut removals = 0;
    for ((unit, case), log) in &logs {
        for r in &log.removals {
            removals += 1;
            assert_eq!(
                r.answer,
                r.listed_before && !r.listed_after,
                "{unit} {case} step {}: answer {} but listed {} -> {}",
                r.step,
                r.answer,
                r.listed_before,
                r.listed_after
            );
        }
    }
    assert!(removals > 0);

    // a list lookup misses only on a state a script op made inconsistent (a list script
    // removes absent members on purpose, with no manager state)
    let mut misses = 0;
    let mut bad = Vec::new();
    for ((unit, case), log) in logs.iter().filter(|(_, log)| log.proto) {
        // ops 24 to 27 (`SmProtoEdge::RemoveEdgeDefinition`, `RemoveProtoVertex`,
        // `SmProtoVertex::RemoveVertexDefinition`, `RemoveProtoEdge`) and 41, 42 (the
        // manager's `RemoveProtoVertex`, `RemoveProtoEdge`) unlist the member the script
        // names: naming a non-member is the miss they report
        let direct = |e: &&witness::StepError| matches!(e.op, 24..=27 | 41 | 42);
        for e in log
            .errors
            .iter()
            .filter(|e| not_on_owner_list(&e.error))
            .filter(|e| !direct(e))
        {
            misses += 1;
            if e.pre.is_none() {
                bad.push(format!("{unit} {case} step {} op {}: {:?}", e.step, e.op, e.error));
            }
        }
    }
    assert!(bad.is_empty(), "{} misses on a consistent state:\n{}", bad.len(), bad.join("\n"));
    assert!(misses > 0);

    // every moved row records exactly the step error classes the table names
    let mut rows = 0;
    let mut quiet = Vec::new();
    for line in include_str!("status_rows.tsv").lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let [unit, case, want] = cols[..] else {
            panic!("bad row {line}");
        };
        rows += 1;
        let log = logs
            .get(&(unit.to_string(), case.to_string()))
            .unwrap_or_else(|| panic!("{unit} {case}: no such case"));
        assert_eq!(classes(log), want, "{unit} {case}");
        if want == "-" {
            // no step failed: a WR-73 answer moved (a miss, now `false`, under the
            // invariant checked above), or a `definition_object_point` row
            if !log.removals.iter().any(|r| !r.listed_before || r.listed_after) {
                quiet.push(format!("{unit} {case}"));
            }
        }
    }
    quiet.sort();
    let mut want: Vec<String> = OBJECT_POINT_ROWS
        .iter()
        .map(|(unit, case)| format!("{unit} {case}"))
        .collect();
    want.sort();
    assert_eq!(
        quiet, want,
        "the rows that moved with no failed step and no removal that missed are the \
         `definition_object_point` rows"
    );
    assert_eq!(rows, 517, "every moved row is listed");
}

/// The distinct classes of a run's step errors, sorted, joined by `|`.
fn classes(log: &witness::Log) -> String {
    let set: std::collections::BTreeSet<String> = log.errors.iter().map(|e| class(&e.error)).collect();
    if set.is_empty() {
        return "-".into();
    }
    set.into_iter().collect::<Vec<_>>().join("|")
}

/// Prints the table rows for the `unit<TAB>case` lines of `$SC_STATUS_ROWS` (how
/// `status_rows.tsv` was made from the moved rows of the goldens diff).
#[test]
#[ignore = "regenerates status_rows.tsv on demand"]
fn status_rows_print() {
    let path = std::env::var("SC_STATUS_ROWS").unwrap();
    let logs = replay_all();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let (unit, case) = line.split_once('\t').unwrap();
        let log = &logs[&(unit.to_string(), case.to_string())];
        println!("{unit}\t{case}\t{}", classes(log));
    }
}
