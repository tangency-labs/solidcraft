//! The layout family: labels spaced along a line. The oracles are closed forms.
#![allow(clippy::unwrap_used)]

use solidcore_ops::layout::spread_along;
use solidcore_ops::Error;

/// Numbers that would overlap move apart round their middle, one pitch apart; the rest stay; bad
/// input is an argument error.
#[test]
fn numbers_spread_along_a_line() {
    assert_eq!(
        spread_along(&[3.0, 3.0, 10.0], 2.0).unwrap(),
        [2.0, 4.0, 10.0]
    );
    assert!(matches!(
        spread_along(&[1.0, 0.0], 1.0),
        Err(Error::Argument)
    ));
    assert!(matches!(
        spread_along(&[0.0, 1.0], -1.0),
        Err(Error::Argument)
    ));
}
