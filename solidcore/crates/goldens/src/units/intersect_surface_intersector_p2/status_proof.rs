//! Proof test for the surface-intersector rows the retirement of
//! `ssi_proto_topology_statuses_pass_on` moved (WR-662, #18927).
//!
//! `status_rows.tsv` lists every row of `intersect.surface_intersector_p2` and `_p3`
//! whose digest differs from main 6ea034c437's, with the classes of the errors its ops
//! now return. The test replays each listed case with the runners' [`witness`] on and
//! checks that its run returns exactly those error classes, each carrying the
//! `SmStatus` the record now shows: the moved op status is the real failed step's.
//!
//! [`witness`]: super::witness

use solidcore::SolidError;

use crate::case::{repo_root, CaseContext};

/// The class of an op error: the failing stage's operation, the reason's variant
/// (without its numbers) and the `SmStatus` the record shows for it.
fn class(e: &SolidError) -> String {
    let status = match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007,
        e => crate::output::recorded_status(e).unwrap_or(-1),
    };
    match e.failure() {
        Some(f) => {
            let reason = format!("{:?}", f.reason);
            let variant = reason.split(['{', ' ']).next().unwrap_or_default();
            format!("{} {variant} {status}", f.operation)
        }
        None => format!("{:?} {status}", e.kind()),
    }
}

/// The distinct classes of a run's op errors, sorted, joined by `|`; `-` for none.
fn classes(errors: &[SolidError]) -> String {
    let set: std::collections::BTreeSet<String> = errors.iter().map(class).collect();
    if set.is_empty() {
        return "-".into();
    }
    set.into_iter().collect::<Vec<_>>().join("|")
}

/// The op errors of `unit`'s case `name`.
fn replay(unit: &str, name: &str) -> Vec<SolidError> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 0,
    };
    let unit = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .unwrap_or_else(|| panic!("no unit {unit}"));
    let case = unit
        .cases(&ctx)
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    let p3 = unit.id().ends_with("_p3");
    if p3 {
        super::super::intersect_surface_intersector_p3::witness::start();
    } else {
        super::witness::start();
    }
    let _ = unit.run_rust(&case);
    if p3 {
        super::super::intersect_surface_intersector_p3::witness::take()
    } else {
        super::witness::take()
    }
}

#[test]
fn moved_rows_return_their_failed_steps() {
    let mut rows = 0;
    for line in include_str!("status_rows.tsv").lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let [unit, case, want] = cols[..] else {
            panic!("bad row {line}");
        };
        rows += 1;
        assert_eq!(classes(&replay(unit, case)), want, "{unit} {case}");
    }
    assert_eq!(rows, 82, "every moved row is listed");
}

/// Prints the table rows for the `unit<TAB>case` lines of `$SC_SIP_ROWS` (how
/// `status_rows.tsv` was made from the moved rows of the goldens diff).
#[test]
#[ignore = "regenerates status_rows.tsv on demand"]
fn status_rows_print() {
    let path = std::env::var("SC_SIP_ROWS").unwrap();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let (unit, case) = line.split_once('\t').unwrap();
        println!("{unit}\t{case}\t{}", classes(&replay(unit, case)));
        for e in replay(unit, case) {
            println!("#   {e}");
        }
    }
}
