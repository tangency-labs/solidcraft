//! A body a boolean proved to be a solid, unchanged since, is not proved again as the next
//! boolean's operand (#14830), counted by the kernel's volume-sign decisions (feature
//! `profile`: `mass::enclosure::volume_sign`, one call per decision).
//!
//! brep-bench's boolean/drilled-plate: a plate cut by nine drills, one after another. Each cut
//! decides the sign of its two operands' volumes and of its result's: 9 cuts, 27 decisions.
//! The first operand of cuts 2 to 9 is the cut before's result, which that cut's result
//! contract proved: those 8 are taken from the proof, so 19 decisions remain (the plate, the 9
//! drills, the 9 results). A primitive is not proved at creation, so its proof stays.
#![cfg(feature = "profile")]
#![allow(clippy::unwrap_used)]

use solidcore_ops::boolean::boolean_with;
use solidcore_ops::session::{self, CallProfile};
use solidcore_ops::{make_box, make_cylinder, Body, BooleanOp, Contract};

/// The volume-sign decisions in `calls`.
fn decisions(calls: &[CallProfile]) -> u64 {
    calls
        .iter()
        .flat_map(|c| &c.profile.summary)
        .filter(|s| s.name == "mass::enclosure::volume_sign")
        .map(|s| s.count)
        .sum()
}

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn plate() -> Body {
    make_box([0.0, 0.0, 0.0], [100.0, 20.0, 10.0]).unwrap()
}

/// Drill `i` (1 to 9): radius 2 at (10 i, 10), running past both faces.
fn drill(i: u32) -> Body {
    make_cylinder([10.0 * f64::from(i), 10.0, -1.0], 2.0, 12.0).unwrap()
}

/// `a` less `b`, proved ([`Contract::Proved`]: the contract that makes and takes proofs).
fn cut(a: Body, b: Body) -> Body {
    boolean_with(a, b, BooleanOp::Subtract, Contract::Proved)
        .unwrap()
        .unwrap()
}

/// `b` after an edit that changes nothing (the identity transform): any edit, through the
/// body's lock, drops the proof.
fn unproved(b: Body) -> Body {
    b.transform(&[
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    b
}

/// The drilled plate, each cut's result kept as it is (`fresh`: false) or edited by the
/// identity, which drops its proof (`fresh`: true), with the decisions the cuts made.
fn drilled(fresh: bool) -> (Body, u64) {
    let drills: Vec<Body> = (1..=9).map(drill).collect();
    let start = plate();
    let (body, calls) = session::profiled(|| {
        drills.into_iter().fold(start, |a, d| {
            let r = cut(a, d);
            if fresh {
                unproved(r)
            } else {
                r
            }
        })
    });
    (body, decisions(&calls))
}

#[test]
fn a_drilled_plate_proves_each_cut_result_once() {
    all_fixes();
    let (kept, reused) = drilled(false);
    let (edited, every) = drilled(true);
    assert_eq!(every, 27, "each cut proves both operands and its result");
    assert_eq!(
        reused, 19,
        "the 8 results proved by the cut before are not proved again"
    );
    // the same body either way
    assert_eq!(kept.counts().unwrap(), edited.counts().unwrap());
    assert_eq!(
        kept.volume(1e-9).unwrap().to_bits(),
        edited.volume(1e-9).unwrap().to_bits()
    );
}

/// The decisions of cutting drill 2 from `first`, the plate less drill 1.
fn second_cut(first: Body) -> u64 {
    let d = drill(2);
    let (_, calls) = session::profiled(|| cut(first, d));
    decisions(&calls)
}

#[test]
fn a_proved_body_changed_since_is_proved_again() {
    all_fixes();
    // unchanged: the result's proof stands, so the operand drill and the result are decided
    assert_eq!(second_cut(cut(plate(), drill(1))), 2);
    // transformed (here by the identity: any edit, through the body's lock, drops the proof
    // whatever it changes): decided again
    assert_eq!(second_cut(unproved(cut(plate(), drill(1)))), 3);
}

/// A copy of a proved body brings its proof: cut from, it is decided as often as the body
/// itself, and once less than the body with its proof dropped (the copy is not proved again).
#[test]
fn a_copy_of_a_proved_body_is_not_proved_again() {
    all_fixes();
    let proved = second_cut(cut(plate(), drill(1)));
    let copied = second_cut(cut(plate(), drill(1)).copy().unwrap());
    let dropped = second_cut(unproved(cut(plate(), drill(1))));
    assert_eq!(copied, proved);
    assert_eq!(copied + 1, dropped);
}

/// Under the default contract a boolean decides no volume's sign: it tests its operands and
/// its result for closure only.
#[test]
fn a_boolean_under_the_default_contract_proves_nothing() {
    all_fixes();
    let (a, d) = (plate(), drill(1));
    let (r, calls) = session::profiled(|| {
        solidcore_ops::boolean::boolean(a, d, BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    });
    assert_eq!(decisions(&calls), 0);
    assert_eq!(
        r.counts().unwrap(),
        cut(plate(), drill(1)).counts().unwrap()
    );
}

#[test]
fn a_proof_made_under_other_fixes_is_not_taken() {
    all_fixes();
    let first = cut(plate(), drill(1));
    // the next call runs with one fillet fix fewer (one a box less a cylinder never reaches):
    // the proof was made with all of them
    let names = solidcore::kernel::UpstreamFixes::NAMES;
    let dropped = names
        .iter()
        .copied()
        .find(|n| n.starts_with("fillet_"))
        .unwrap();
    session::set_thread_fixes(names.iter().copied().filter(|n| *n != dropped)).unwrap();
    let decided = second_cut(first);
    all_fixes();
    assert_eq!(decided, 3);
}
