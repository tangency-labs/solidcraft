use std::time::Duration;

use super::*;
use crate::boolean::{boolean, boolean_under, boolean_with};
use crate::{make_box, make_cylinder, make_sphere, Body, BooleanOp};

fn all_fixes() {
    crate::session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied())
        .unwrap();
}

/// A case: its name, its operands, its boolean.
type Case = (&'static str, fn() -> (Body, Body), BooleanOp);

/// The operand pairs the boolean tests run: planar, cylindrical and spherical cuts and joins.
fn cases() -> Vec<Case> {
    fn boxes() -> (Body, Body) {
        (
            make_box([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]).unwrap(),
            make_box([1.0, 0.5, 0.5], [2.0, 1.0, 1.0]).unwrap(),
        )
    }
    fn plate_hole() -> (Body, Body) {
        (
            make_box([0.0, 0.0, 0.0], [4.0, 4.0, 1.0]).unwrap(),
            make_cylinder([2.0, 2.0, -1.0], 0.75, 3.0).unwrap(),
        )
    }
    fn ball_block() -> (Body, Body) {
        (
            make_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap(),
            make_sphere([2.0, 2.0, 2.0], 1.25).unwrap(),
        )
    }
    vec![
        ("boxes union", boxes, BooleanOp::Union),
        ("boxes subtract", boxes, BooleanOp::Subtract),
        ("boxes intersect", boxes, BooleanOp::Intersect),
        ("plate less hole", plate_hole, BooleanOp::Subtract),
        ("block and ball", ball_block, BooleanOp::Union),
        ("block less ball", ball_block, BooleanOp::Subtract),
    ]
}

fn bytes(r: Result<Option<Body>>) -> Option<Vec<u8>> {
    r.unwrap().map(|b| b.to_smb(false).unwrap())
}

#[test]
fn the_dial_takes_the_last_stop_it_reaches() {
    assert_eq!(Rigor::stop(0.0).unwrap().name, "Preview");
    assert_eq!(Rigor::stop(0.249).unwrap().name, "Preview");
    assert_eq!(Rigor::stop(0.25).unwrap().name, "Fast");
    assert_eq!(Rigor::stop(0.6).unwrap().name, "Standard");
    assert_eq!(Rigor::stop(0.99).unwrap().name, "Careful");
    assert_eq!(Rigor::stop(1.0).unwrap().name, "Proof");
    for bad in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
        assert_eq!(Rigor::dial(bad), Err(Error::Argument), "{bad}");
    }
}

/// Every knob rises (or stays) from stop to stop, and every stop is a rigor a call runs with.
#[test]
fn the_stops_rise_in_every_knob() {
    let certify = |c: Certify| match c {
        Certify::Off => 0,
        Certify::Closed => 1,
        Certify::Proved => 2,
    };
    for w in STOPS.windows(2) {
        let (a, b) = (w[0].rigor, w[1].rigor);
        assert!(w[0].from < w[1].from);
        assert!(certify(a.certify) <= certify(b.certify));
        assert!(a.accuracy.tolerance.unwrap() > b.accuracy.tolerance.unwrap());
        assert!(a.accuracy.chord.unwrap() > b.accuracy.chord.unwrap());
        assert!(a.accuracy.samples.unwrap() < b.accuracy.samples.unwrap());
        assert!(a.effort.percent.unwrap() < b.effort.percent.unwrap());
        assert!(a.time.unwrap() < b.time.unwrap());
    }
    for s in STOPS {
        s.rigor.check().unwrap();
        assert!(!s.rigor.is_legacy(), "legacy is not a stop: {}", s.name);
    }
    assert_eq!(STOPS[0].from, 0.0);
    assert_eq!(STOPS[STOPS.len() - 1].from, 1.0);
}

#[test]
fn a_rigor_no_call_runs_with_is_refused() {
    let bad = [
        Accuracy {
            tolerance: Some(0.0),
            ..Accuracy::default()
        },
        Accuracy {
            chord: Some(f64::NAN),
            ..Accuracy::default()
        },
        Accuracy {
            samples: Some(0),
            ..Accuracy::default()
        },
    ];
    for accuracy in bad {
        let r = Rigor {
            accuracy,
            ..Rigor::LEGACY
        };
        assert_eq!(r.check(), Err(Error::Argument), "{r:?}");
        let (a, b) = (
            make_box([0.0; 3], [1.0; 3]).unwrap(),
            make_box([0.5; 3], [1.0; 3]).unwrap(),
        );
        assert_eq!(
            boolean_under(a, b, BooleanOp::Union, &r).err(),
            Some(Error::Argument)
        );
    }
    let r = Rigor {
        effort: Effort { percent: Some(0) },
        ..Rigor::LEGACY
    };
    assert_eq!(r.check(), Err(Error::Argument));
}

/// The legacy rigor is the call as it was, to the bit: each boolean's result writes the same
/// `.smb` bytes as the plain call outside every scope, explicitly and within a scope.
#[test]
fn legacy_booleans_are_the_plain_calls_to_the_bit() {
    all_fixes();
    assert!(Rigor::default().is_legacy());
    for (name, make, op) in cases() {
        let plain = {
            let (a, b) = make();
            bytes(boolean(a, b, op))
        };
        let under = {
            let (a, b) = make();
            bytes(boolean_under(a, b, op, &Rigor::LEGACY))
        };
        let scoped = {
            let (a, b) = make();
            within(&Rigor::LEGACY, || {
                assert!(current().is_none(), "legacy opens no scope");
                bytes(boolean(a, b, op))
            })
        };
        assert!(plain.is_some(), "{name}");
        assert_eq!(plain, under, "{name}");
        assert_eq!(plain, scoped, "{name}");
    }
}

/// A time budget decides only whether a result comes: within one it has room for, a boolean's
/// result is the unbudgeted one to the bit.
#[test]
fn a_budget_with_room_changes_no_result() {
    all_fixes();
    let budgeted = Rigor {
        time: Some(Duration::from_secs(600)),
        ..Rigor::LEGACY
    };
    for (name, make, op) in cases() {
        let plain = {
            let (a, b) = make();
            bytes(boolean(a, b, op))
        };
        let (a, b) = make();
        assert_eq!(plain, bytes(boolean_under(a, b, op, &budgeted)), "{name}");
    }
}

/// Each certification is the contract it names: Closed and Proved are `boolean_with`'s to the
/// bit, and Off takes the same sound result.
#[test]
fn certification_is_the_contract_it_names() {
    all_fixes();
    for (name, make, op) in cases() {
        for (certify, contract) in [
            (Certify::Closed, Contract::Closed),
            (Certify::Proved, Contract::Proved),
            (Certify::Off, Contract::Closed),
        ] {
            let want = {
                let (a, b) = make();
                bytes(boolean_with(a, b, op, contract))
            };
            let (a, b) = make();
            let r = Rigor {
                certify,
                ..Rigor::LEGACY
            };
            assert_eq!(
                want,
                bytes(boolean_under(a, b, op, &r)),
                "{name} {certify:?}"
            );
        }
    }
}

/// A spent budget is a typed refusal, never a result: a zero budget refuses before the call, a
/// budget that runs out while the work is on refuses every call after it.
#[test]
fn out_of_time_is_a_typed_refusal() {
    all_fixes();
    let none = Rigor {
        time: Some(Duration::ZERO),
        ..Rigor::LEGACY
    };
    let (a, b) = cases()[0].1();
    assert_eq!(
        boolean_under(a, b, BooleanOp::Union, &none).err(),
        Some(Error::OutOfTime {
            budget: Duration::ZERO
        })
    );
    let short = Rigor {
        time: Some(Duration::from_millis(20)),
        ..Rigor::LEGACY
    };
    let (a, b) = cases()[0].1();
    let r = within(&short, || {
        // the work runs past its budget before its boolean
        std::thread::sleep(Duration::from_millis(300));
        boolean(a, b, BooleanOp::Union)
    });
    assert_eq!(
        r.err(),
        Some(Error::OutOfTime {
            budget: Duration::from_millis(20)
        })
    );
    assert!(
        current().is_none() && out_of_time().is_none(),
        "the scope ended"
    );
}

/// The application's own cancellation stays a cancellation within a rigor with time to spare.
#[test]
fn a_cancel_within_a_rigor_stays_a_cancel() {
    all_fixes();
    let token = CancelToken::new();
    token.cancel();
    let r = Rigor::dial(0.5).unwrap();
    let (a, b) = cases()[0].1();
    let got = cancel::within(&token, || boolean_under(a, b, BooleanOp::Union, &r));
    assert_eq!(got.err(), Some(Error::Cancelled));
}

#[test]
fn scopes_nest_and_end() {
    let (outer, inner) = (Rigor::dial(0.0).unwrap(), Rigor::dial(1.0).unwrap());
    within(&outer, || {
        assert_eq!(current(), Some(outer));
        within(&inner, || assert_eq!(current(), Some(inner)));
        within(&Rigor::LEGACY, || {
            assert_eq!(current(), Some(Rigor::LEGACY));
        });
        assert_eq!(current(), Some(outer));
    });
    assert_eq!(current(), None);
}
