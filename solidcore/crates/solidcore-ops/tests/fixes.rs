//! The fix set, taken by name: its own test binary, as the fix set is the process kernel's.

use solidcore_ops::{make_box, make_sphere, session, CheckLevel};

/// The fixes are the ones named, by `UpstreamFixes` field name; an unknown name changes nothing.
/// The kernel runs with them: with every fix on a sphere's volume is the closed form.
#[test]
fn fixes_are_taken_by_name() {
    let names = solidcore::kernel::UpstreamFixes::NAMES;
    let before = session::fixes();
    assert!(session::set_fixes(["no_such_fix"]).is_err());
    assert_eq!(session::fixes(), before);
    session::set_fixes([names[0]]).unwrap();
    assert_eq!(session::fixes(), vec![names[0]]);
    session::set_fixes([]).unwrap();
    assert!(session::fixes().is_empty());
    let ball = make_sphere([0.0; 3], 2.0).unwrap();
    session::set_fixes(names.iter().copied()).unwrap();
    let v = ball.volume(1e-8).unwrap();
    assert!((v - 32.0 / 3.0 * std::f64::consts::PI).abs() < 1e-6, "{v}");

    // A transform's result is valid whatever the fix set.
    for on in [&[][..], names] {
        session::set_fixes(on.iter().copied()).unwrap();
        let b = make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
        let m = [
            [0.0, 2.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 2.0, 3.0, 1.0],
        ];
        b.transform(&m).unwrap();
        assert!(
            b.check(CheckLevel::All).unwrap().is_valid(),
            "{} fixes on",
            on.len()
        );
    }
}
