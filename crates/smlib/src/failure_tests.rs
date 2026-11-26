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

/// #22148: a call this crate makes on the kernel keeps the class solidcore gives the error it
/// failed with in its code, read down the error's cause chain, beside the kind.
#[test]
fn a_kernel_failure_keeps_its_class_in_its_code() {
    use solidcore::SolidError;
    use solidcore::blend::fillet_executive::NoRoomReason;
    use solidcore::error::blend::BlendFailure;
    use solidcore::error::{Failure, Reason};
    use solidcore_ops::ErrorClass;

    let refusal = || Failure::new("test::blend", BlendFailure::NoRoom(NoRoomReason::BallDoesNotFit)).into_error();
    let over = |cause: SolidError| Failure::new("test::stage", Reason::Failed("a stage failed")).caused_by(cause).into_error();
    for (e, class) in [
        (refusal(), ErrorClass::Refusal),
        (over(refusal()), ErrorClass::Refusal),
        (Failure::new("test::blend", BlendFailure::StageFailed).into_error(), ErrorClass::Defect),
        (SolidError::unported("test::callee", "test.unit"), ErrorClass::PortFailure),
        (SolidError::sm_err("test::status"), ErrorClass::Unclassed),
    ] {
        clear();
        let kind = e.kind().code();
        let r: crate::Result<()> = crate::core::run("classed", |_| Err(e.clone()));
        assert!(r.is_err(), "{e:?}");
        let (code, _) = last().unwrap_or_else(|| panic!("no code: {e:?}"));
        assert_eq!((code.call, code.kind, code.class), ("classed", kind, class), "{e:?}");
    }
    clear();
}
