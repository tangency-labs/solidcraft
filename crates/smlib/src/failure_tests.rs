use super::*;
use crate::Brep;

/// A 2 thick wall, 30 x 2 x 10, both its top edges rounded at `radius`: past 1 the two rounds'
/// bands overlap on the 2 wide top, so the round is refused.
fn round_wall_top(radius: f64) -> crate::Result<Brep> {
    let b = Brep::make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
    b.fillet(&[[15.0, 0.0, 10.0], [15.0, 2.0, 10.0]], 1e-3, radius)
}

/// A round the kernel refuses leaves its code on the thread, found in the error's text and in
/// any text built around it, never in another's; the same failure at another size has the same
/// code; [`clear`] forgets it.
#[test]
fn a_refused_round_leaves_its_kernel_code_in_its_error() {
    clear();
    let e = round_wall_top(1.5).unwrap_err();
    let (code, message) = last().unwrap();
    assert_eq!(message, e.to_string());
    assert_ne!(code.kind, FailureCode::OPS, "a kernel failure: {code}");
    assert_eq!(code_in(&e.to_string()), Some(code));
    assert_eq!(code_in(&format!("feature `Fillet1`: {e}")), Some(code));
    assert_eq!(code_in("sketch `Sketch1` failed: no profile"), None);
    let again = round_wall_top(1.8).unwrap_err();
    assert_eq!(code_in(&again.to_string()), Some(code), "one failure, one code");
    clear();
    assert_eq!(last(), None);
}

/// A failure on another thread is that thread's: this one's stays as it was.
#[test]
fn a_failure_is_its_own_threads() {
    clear();
    std::thread::spawn(|| {
        round_wall_top(1.5).unwrap_err();
        assert!(last().is_some());
    })
    .join()
    .unwrap();
    assert_eq!(last(), None);
}
