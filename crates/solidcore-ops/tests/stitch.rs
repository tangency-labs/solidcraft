//! The stitch and merge family on boxes.
#![allow(clippy::unwrap_used)]

use solidcore_ops::stitch::stitch;
use solidcore_ops::{make_box, session, Error};

fn all_fixes() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

#[test]
fn a_closed_box_stitches_to_itself_closed() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let (s, closed) = stitch(b, 1e-6).unwrap();
    assert!(closed);
    assert_eq!(s.counts().unwrap(), [6, 12, 8]);
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        assert_eq!(stitch(b, t).unwrap_err(), Error::Argument, "{t}");
    }
}
