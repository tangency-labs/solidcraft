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

use solidcore_ops::boolean::boolean;
use solidcore_ops::session::{self, CallProfile};
use solidcore_ops::{make_box, make_cylinder, Body, BooleanOp};

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

fn cut(a: Body, b: Body) -> Body {
    boolean(a, b, BooleanOp::Subtract).unwrap().unwrap()
}

/// The drilled plate, each cut's result kept as it is (`fresh`: false) or replaced by a copy,
/// which holds no proof (`fresh`: true), with the decisions the cuts made.
fn drilled(fresh: bool) -> (Body, u64) {
    let drills: Vec<Body> = (1..=9).map(drill).collect();
    let start = plate();
    let (body, calls) = session::profiled(|| {
        drills.into_iter().fold(start, |a, d| {
            let r = cut(a, d);
            if fresh {
                r.copy().unwrap()
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
    let (copied, every) = drilled(true);
    assert_eq!(every, 27, "each cut proves both operands and its result");
    assert_eq!(
        reused, 19,
        "the 8 results proved by the cut before are not proved again"
    );
    // the same body either way
    assert_eq!(kept.counts().unwrap(), copied.counts().unwrap());
    assert_eq!(
        kept.volume(1e-9).unwrap().to_bits(),
        copied.volume(1e-9).unwrap().to_bits()
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
    let moved = cut(plate(), drill(1));
    moved
        .transform(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap();
    assert_eq!(second_cut(moved), 3);
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
