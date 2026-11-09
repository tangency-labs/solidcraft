//! Contact between bodies on closed forms: meets, meets against several, the boundary
//! distance, and the refusals.
#![allow(clippy::unwrap_used)]

use solidcore_ops::session;
use solidcore_ops::{make_box, make_sphere, Error};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

#[test]
fn boxes_sharing_a_face_meet() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([1.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    assert!(a.meets(&b, 1e-6).unwrap());
    assert!(b.meets(&a, 1e-6).unwrap());
}

#[test]
fn boxes_one_apart_do_not_meet_and_their_distance_is_one() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([2.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    assert!(!a.meets(&b, 1e-3).unwrap());
    let g = a.boundary_distance(&b, 1e-9).unwrap();
    assert!(g.lower <= 1.0 && 1.0 <= g.upper, "{g:?}");
    assert!(g.upper - g.lower <= 1e-9 * g.upper.max(1.0), "{g:?}");
    let [p, q] = g.witness.unwrap();
    assert!(
        (p[0] - 1.0).abs() <= 1e-12 && (q[0] - 2.0).abs() <= 1e-12,
        "{g:?}"
    );
}

#[test]
fn a_body_meets_itself_and_one_it_encloses() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [3.0, 3.0, 3.0]).unwrap();
    let inner = make_box([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]).unwrap();
    assert!(a.meets(&a, 1e-6).unwrap());
    assert!(a.meets(&inner, 1e-6).unwrap());
    assert!(inner.meets(&a, 1e-6).unwrap());
    // neither body changes
    assert_eq!(a.counts().unwrap(), [6, 12, 8]);
    assert_eq!(inner.counts().unwrap(), [6, 12, 8]);
}

#[test]
fn meets_each_answers_each_with_its_own_tolerance() {
    all_fixes();
    let a = make_sphere([0.0, 0.0, 0.0], 1.0).unwrap();
    let touching = make_box([1.0, -1.0, -1.0], [1.0, 2.0, 2.0]).unwrap();
    let near = make_sphere([2.5, 0.0, 0.0], 1.0).unwrap(); // 0.5 apart
    let far = make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let got = a
        .meets_each(&[(&touching, 1e-6), (&near, 0.1), (&near, 0.6), (&far, 1.0)])
        .unwrap();
    assert_eq!(got, vec![true, false, true, false]);
}

/// The refusal is the kernel's own bracket: a tolerance equal to a gap the kernel brackets
/// to its rounding is inside the bracket, and neither answer is given.
#[test]
fn a_tolerance_inside_the_bracket_is_refused_with_it() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([2.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let far = make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    match a.meets(&b, 1.0) {
        Err(Error::ContactUndecided {
            other: 0,
            lower,
            upper,
        }) => assert!(lower <= 1.0 && 1.0 <= upper, "[{lower}, {upper}]"),
        r => panic!("{r:?}"),
    }
    match a.meets_each(&[(&far, 1e-3), (&b, 1.0)]) {
        Err(Error::ContactUndecided { other: 1, .. }) => {}
        r => panic!("{r:?}"),
    }
}

#[test]
fn a_tolerance_or_width_not_finite_and_positive_is_refused() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([2.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let invalid = Error::Status(solidcore::error::SM_ERR_INVALID_INPUT);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(a.meets(&b, bad).unwrap_err(), invalid, "{bad}");
        assert_eq!(a.boundary_distance(&b, bad).unwrap_err(), invalid, "{bad}");
    }
}
