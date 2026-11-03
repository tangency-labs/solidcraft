//! The session's guards: each test fails without the guard it names.

use std::sync::{Arc, Mutex};

use solidcore::SolidError;

use super::*;
use crate::make_box;

fn all_fixes() {
    set_fixes(UpstreamFixes::NAMES.iter().copied()).unwrap();
}

/// The hook is process-wide: the tests that install one take turns.
pub(crate) static HOOKED: Mutex<()> = Mutex::new(());

#[test]
fn a_panic_is_an_error_and_the_kernel_is_fresh_after() {
    all_fixes();
    assert!(!in_call());
    let r: Result<()> = run("test", |_| panic!("boom"));
    assert!(
        matches!(r, Err(Error::Internal(ref m)) if m.contains("boom")),
        "{r:?}"
    );
    assert!(matches!(crash_test(), Err(Error::Internal(ref m)) if m.contains("crash test")));
    assert!(make_box([0.0; 3], [1.0; 3]).is_ok());
    assert!(!in_call());
    let inside = run("inside", |_| {
        Ok(in_call() && current_call().as_deref() == Some("inside"))
    })
    .unwrap();
    assert!(inside);
}

/// After a panic the kernel is replaced (a mark the panicking call left on it is gone, and the
/// tolerance it changed is the default again), its fixes are the application's, and the
/// process-lifetime latch of the fillet solver's step survives.
#[test]
fn the_kernel_after_a_panic_is_configured_as_before() {
    all_fixes();
    let latched = run("latch", |k| Ok(k.latch_fillet_point_step(0.123))).unwrap();
    let r: Result<()> = run("panics part way", |k| {
        k.tolerances.zone_tol_3d = 7.0;
        k.upstream_ub_passed.push("marker");
        panic!("part way")
    });
    assert!(matches!(r, Err(Error::Internal(_))));
    let default_zone = solidcore::Tolerances::default().zone_tol_3d;
    let after = run("after", |k| {
        Ok(Some((
            k.fillet_point_step.get(),
            k.tolerances.zone_tol_3d,
            k.upstream_ub_passed.is_empty(),
            k.fixes == UpstreamFixes::all(),
        )))
    })
    .unwrap();
    assert_eq!(after, Some((Some(latched), default_zone, true, true)));
}

/// A panicking failure callback loses nothing of the call's result.
#[test]
fn a_panicking_hook_keeps_the_calls_result() {
    let _turn = HOOKED.lock().unwrap_or_else(PoisonError::into_inner);
    all_fixes();
    set_failure_hook(Some(Arc::new(|_: &Failure<'_>| panic!("the hook panics"))));
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let r: Result<()> = with_body("refused", &b, |_, _| {
        Err(SolidError::invalid_input("test", "refused"))
    });
    set_failure_hook(None);
    assert_eq!(r, Err(Error::Status(crate::error::INVALID_INPUT)));
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
}

/// Ops called from the failure callback, or from inside a kernel call, is refused, not a
/// deadlock.
#[test]
fn reentry_is_refused() {
    let _turn = HOOKED.lock().unwrap_or_else(PoisonError::into_inner);
    all_fixes();
    let seen: Arc<Mutex<Vec<Result<Body>>>> = Arc::default();
    let s = Arc::clone(&seen);
    set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
        if f.call == "refused for reentry" {
            s.lock().unwrap().push(make_box([0.0; 3], [1.0; 3]));
        }
    })));
    let r: Result<()> = run("refused for reentry", |_| {
        Err(SolidError::invalid_input("test", "refused"))
    });
    set_failure_hook(None);
    assert!(r.is_err());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].as_ref().err(), Some(&Error::Reentered));
    let inner = run("outer", |_| Ok(Some(make_box([0.0; 3], [1.0; 3]).err()))).unwrap();
    assert_eq!(inner, Some(Some(Error::Reentered)));
}

/// A panic in a call that only reads a body leaves the body usable: it still saves.
#[test]
fn a_read_only_panic_leaves_the_body_usable() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let r: Result<()> = read_body("reads and panics", &b, |_, _| panic!("reader"));
    assert!(matches!(r, Err(Error::Internal(_))));
    let r: Result<()> = measure_body("measures and panics", &b, |_, _| panic!("measure"));
    assert!(matches!(r, Err(Error::Internal(_))));
    let bytes = b.to_smb(false).unwrap();
    assert_eq!(
        Body::from_smb(bytes, false).unwrap().counts().unwrap(),
        [6, 12, 8]
    );
}

/// A failed call hands its name, its reason and its inputs as `.smb` bytes to the callback; a
/// body whose edit panicked is put back first, so it is written as it came in, and stays usable.
#[test]
fn a_failed_call_reaches_the_hook_with_its_consistent_inputs() {
    type Seen = Vec<(
        String,
        String,
        Vec<(&'static str, std::result::Result<Vec<u8>, String>)>,
    )>;
    let _turn = HOOKED.lock().unwrap_or_else(PoisonError::into_inner);
    all_fixes();
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let s = Arc::clone(&seen);
    set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
        if f.call.starts_with("on purpose") {
            s.lock()
                .unwrap()
                .push((f.call.to_owned(), f.why.to_owned(), f.inputs.clone()));
        }
    })));
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let r: Result<()> = with_body("on purpose: refused", &b, |_, _| {
        Err(SolidError::invalid_input("test", "refused"))
    });
    assert_eq!(r, Err(Error::Status(crate::error::INVALID_INPUT)));
    let edited = make_box([0.0; 3], [1.0; 3]).unwrap();
    let r: Result<()> = with_body("on purpose: panics editing", &edited, |_, br| {
        let v = br.vertex_list()[0];
        br.vertex_mut(v)?.point = solidcore::Vec3::new(9.0, 9.0, 9.0);
        panic!("half way")
    });
    set_failure_hook(None);
    assert!(matches!(r, Err(Error::Internal(_))));
    assert_eq!(edited.counts().unwrap(), [6, 12, 8]);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let (call, why, inputs) = &seen[0];
    assert_eq!(call, "on purpose: refused");
    assert!(why.contains("1007") || why.contains("rejected"), "{why}");
    assert_eq!(inputs[0].0, "brep");
    let back = Body::from_smb(inputs[0].1.clone().unwrap(), false).unwrap();
    assert_eq!(back.counts().unwrap(), [6, 12, 8]);
    let unedited = make_box([0.0; 3], [1.0; 3]).unwrap().to_smb(false).unwrap();
    assert_eq!(seen[1].2[0].1.as_ref().unwrap(), &unedited);
    assert_eq!(edited.to_smb(false).unwrap(), unedited);
}

type Written = Vec<std::result::Result<Vec<u8>, String>>;

/// A call that consumes its operand reports it as it came in, not the empty body it leaves.
#[test]
fn a_consuming_call_reports_its_operands_as_they_came() {
    let _turn = HOOKED.lock().unwrap_or_else(PoisonError::into_inner);
    all_fixes();
    let seen: Arc<Mutex<Written>> = Arc::default();
    let s = Arc::clone(&seen);
    set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
        if f.call == "on purpose: consumes" {
            s.lock()
                .unwrap()
                .extend(f.inputs.iter().map(|(_, b)| b.clone()));
        }
    })));
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let r: Result<()> = run_taking("on purpose: consumes", &[("brep", &b)], |_| {
        let taken = std::mem::replace(&mut *b.lock(), Brep::new_with_infinite_region()?);
        drop(taken);
        Err(SolidError::invalid_input("test", "refused"))
    });
    set_failure_hook(None);
    assert!(r.is_err());
    let seen = seen.lock().unwrap();
    let back = Body::from_smb(seen[0].clone().unwrap(), false).unwrap();
    assert_eq!(back.counts().unwrap(), [6, 12, 8]);
}

/// Runs `f` on its own thread and gives its value, or `None` when it does not finish within ten
/// seconds (a deadlock: the thread is left blocked).
fn within_ten_seconds<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10)).ok()
}

/// Setting the fixes from inside a kernel call (an application's provenance rule runs in one)
/// is refused, not a wait on the kernel the call holds.
#[test]
fn setting_the_fixes_inside_a_call_is_refused() {
    let r = within_ten_seconds(|| {
        all_fixes();
        run("sets the fixes", |_| Ok(set_fixes([]).err()))
    });
    assert_eq!(r, Some(Ok(Some(FixesRefused::Reentered))));
    assert_eq!(fixes().len(), UpstreamFixes::NAMES.len());
}

/// The callback a failure reports to is the one installed when the call started: one installed
/// while a consuming call runs never sees that call's emptied operands.
#[test]
fn a_hook_installed_during_a_call_is_not_that_calls() {
    let _turn = HOOKED.lock().unwrap_or_else(PoisonError::into_inner);
    all_fixes();
    set_failure_hook(None);
    let seen: Arc<Mutex<Written>> = Arc::default();
    let s = Arc::clone(&seen);
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let r: Result<()> = run_taking("on purpose: hooked late", &[("brep", &b)], |_| {
        set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
            if f.call == "on purpose: hooked late" {
                s.lock()
                    .unwrap()
                    .extend(f.inputs.iter().map(|(_, b)| b.clone()));
            }
        })));
        *b.lock() = Brep::new_with_infinite_region()?;
        Err(SolidError::invalid_input("test", "refused"))
    });
    set_failure_hook(None);
    assert!(r.is_err());
    assert!(
        seen.lock().unwrap().is_empty(),
        "the late hook saw the call"
    );
}

/// A failed call's input that is still held (by its caller here) is reported unwritten, never
/// waited for under the kernel's lock.
#[test]
fn a_held_input_is_reported_not_waited_for() {
    let r = within_ten_seconds(|| {
        let _turn = HOOKED.lock().unwrap_or_else(PoisonError::into_inner);
        all_fixes();
        let seen: Arc<Mutex<Written>> = Arc::default();
        let s = Arc::clone(&seen);
        set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
            if f.call == "on purpose: held" {
                s.lock()
                    .unwrap()
                    .extend(f.inputs.iter().map(|(_, b)| b.clone()));
            }
        })));
        let b = make_box([0.0; 3], [1.0; 3]).unwrap();
        let held = b.lock();
        let r: Result<()> = run_on("on purpose: held", &[("brep", &b)], |_| {
            Err(SolidError::invalid_input("test", "refused"))
        });
        drop(held);
        set_failure_hook(None);
        let seen = seen.lock().unwrap().clone();
        (r.is_err(), seen)
    });
    let (failed, seen) = r.expect("the report waited on the held body");
    assert!(failed);
    assert!(
        matches!(&seen[..], [Err(why)] if why.contains("held")),
        "{seen:?}"
    );
}
