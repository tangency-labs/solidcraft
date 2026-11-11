//! The radius law's data checks (the kernel-calling cases are tests/blend.rs, their own test
//! binary: they hold the process kernel for seconds, which the session's own timing tests must
//! not wait behind).

use super::*;

#[test]
fn laws_are_checked_as_their_data() {
    assert!(law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]));
    assert!(!law_ok(1, &[0.0, 0.0, 1.0], &[1.0, 2.0]));
    assert!(!law_ok(1, &[0.0, 0.5, 1.0, 1.0], &[1.0, 2.0]));
    assert!(!law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[-1.0, 2.0]));
    assert!(!law_ok(4, &[0.0; 7], &[1.0, 2.0]));
    assert!(law_pinches(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 0.0, 1.0]));
    assert!(!law_pinches(1, &[0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]));
    assert!(!law_pinches(
        2,
        &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        &[1.0, 0.0, 1.0, 1.0]
    ));
    assert!(law_pinches(
        2,
        &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        &[1.0, 1.0, 0.0, 1.0, 1.0]
    ));
}
