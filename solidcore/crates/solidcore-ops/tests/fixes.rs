//! The fix set, taken by name: its own test binary, as the fix set is the process kernel's.

use solidcore::kernel::UpstreamFixes;
use solidcore_ops::{make_box, make_sphere, session, CheckLevel};

/// Every correction but the held-back ones, in declaration order.
fn corrections() -> Vec<&'static str> {
    UpstreamFixes::NAMES
        .iter()
        .copied()
        .filter(|n| !UpstreamFixes::HELD_BACK.contains(n))
        .collect()
}

/// Every correction is on and stays on; a held-back one is turned on by name, an always-on one
/// named changes nothing, an unknown name is refused and changes nothing. The kernel runs with
/// them: a sphere's volume is the closed form, and a transform's result is valid.
#[test]
fn fixes_are_taken_by_name() {
    assert_eq!(session::fixes(), corrections());
    assert!(session::set_fixes(["no_such_fix"]).is_err());
    assert_eq!(session::fixes(), corrections());

    let [held, ..] = UpstreamFixes::HELD_BACK else {
        panic!("a held-back fix");
    };
    session::set_fixes([*held]).unwrap();
    let mut with_held = corrections();
    with_held.push(held);
    with_held.sort_unstable();
    let mut on = session::fixes();
    on.sort_unstable();
    assert_eq!(on, with_held);

    let always = corrections();
    let [first, ..] = always.as_slice() else {
        panic!("a correction");
    };
    session::set_fixes([*first]).unwrap();
    assert_eq!(session::fixes(), corrections());
    session::set_fixes([]).unwrap();
    assert_eq!(session::fixes(), corrections());

    let ball = make_sphere([0.0; 3], 2.0).unwrap();
    let v = ball.volume(1e-8).unwrap();
    assert!((v - 32.0 / 3.0 * std::f64::consts::PI).abs() < 1e-6, "{v}");

    let b = make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    let m = [
        [0.0, 2.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [1.0, 2.0, 3.0, 1.0],
    ];
    b.transform(&m).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}
