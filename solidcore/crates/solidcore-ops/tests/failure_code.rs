//! A failed call's [`FailureCode`]: the kernel's kind and operation of the error it failed
//! with, the same in [`session::take_failure_code`] and in the failure callback, and the same
//! for the same failure whatever its numbers.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use solidcore_ops::session::{self, Failure};
use solidcore_ops::*;

/// A 2 thick wall, 30 x 2 x 10, both its top edges rounded at `radius`: past 1 the two rounds'
/// bands overlap on the 2 wide top.
fn round_wall_top(radius: f64) -> Result<()> {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [15.0, 2.0, 10.0]),
    ];
    blend(&b, Blend::Round, &edges, &[radius], &[], Corners::Filled).map(drop)
}

/// A round larger than the wall's top allows is refused with the size that fits, found from
/// the kernel's failure: its code is that kernel error's kind and operation (a kind the kernel
/// defines, not ops' own), on the call that rounds; the callback is handed the same code the
/// thread takes after the call; two such rounds of other sizes fail with one code; and a round
/// that builds leaves none.
#[test]
fn a_round_too_large_gives_the_kernels_code_to_the_caller_and_the_callback() {
    let seen: Arc<Mutex<Vec<FailureCode>>> = Arc::default();
    let s = Arc::clone(&seen);
    // the callback runs on the calling thread; other tests' calls run on others
    let me = std::thread::current().id();
    session::set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
        if std::thread::current().id() == me {
            s.lock().unwrap().push(f.code);
        }
    })));
    let _ = session::take_failure_code();
    let r = round_wall_top(1.5);
    let code = session::take_failure_code();
    let other = round_wall_top(1.8);
    let other_code = session::take_failure_code();
    session::set_failure_hook(None);

    assert!(matches!(r, Err(Error::TooLarge { .. })), "{r:?}");
    assert!(matches!(other, Err(Error::TooLarge { .. })), "{other:?}");
    let code = code.unwrap();
    assert_eq!(code.call, "blend");
    // the kernel's failure the size was found for, not one ops decided
    assert_ne!(code.kind, FailureCode::OPS, "{code}");
    assert!(!code.operation.is_empty());
    assert_eq!(other_code, Some(code), "one failure, one code");
    assert_eq!(seen.lock().unwrap().first(), Some(&code));

    round_wall_top(0.5).unwrap();
    assert_eq!(
        session::take_failure_code(),
        None,
        "a call that builds leaves no code"
    );
}
