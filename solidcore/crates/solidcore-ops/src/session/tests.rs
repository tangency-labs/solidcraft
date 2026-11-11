//! The session's guards: each test fails without the guard it names.

use std::sync::{Arc, Mutex};

use solidcore::SolidError;

use super::*;
use crate::make_box;

fn all_fixes() {
    set_thread_fixes(UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    in_own_process(
        "session::tests::the_kernel_after_a_panic_is_configured_as_before",
        || {
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
        },
    );
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
    in_own_process(
        "session::tests::a_failed_call_reaches_the_hook_with_its_consistent_inputs",
        || {
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
        },
    );
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

/// A configuration-sensitive test gets its own process, so corpus tests cannot replace its
/// process-wide fixes between session calls. Child completion, rather than elapsed time,
/// determines the result. The child runs the same test body exactly once.
fn in_own_process(test: &str, f: impl FnOnce()) {
    const CHILD: &str = "SOLIDCORE_SESSION_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(test) {
        f();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, test)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{test}: {}\n{stdout}\n{stderr}",
        output.status
    );
    assert!(
        stdout.contains("1 passed"),
        "child must execute the selected test: {stdout}"
    );
}

/// Two threads with different fixes run alternating calls on the one kernel, each with its own
/// from its first call to its last: the race #2983 was.
#[test]
fn a_threads_fixes_are_its_own_call_by_call() {
    use std::sync::Barrier;
    let turns = Arc::new(Barrier::new(2));
    let thread = |on: &'static [&'static str]| {
        let turns = Arc::clone(&turns);
        std::thread::spawn(move || {
            set_thread_fixes(on.iter().copied()).unwrap();
            assert_eq!(fixes(), on);
            let mut want = UpstreamFixes::default();
            for name in on {
                *want.flag_mut(name).unwrap() = true;
            }
            let mut own = true;
            for _ in 0..16 {
                turns.wait();
                own &= run("reads its fixes", |k| Ok(k.fixes == want)).unwrap();
            }
            assert!(own);
        })
    };
    let first = thread(&UpstreamFixes::NAMES[..1]);
    let second = thread(&UpstreamFixes::NAMES[1..2]);
    first.join().unwrap();
    second.join().unwrap();
    // an unknown name changes nothing, on this thread either
    all_fixes();
    assert!(set_thread_fixes(["no_such_fix"]).is_err());
    assert_eq!(fixes(), UpstreamFixes::NAMES);
}

/// A thread's own fixes are invisible to the threads that named none, which run with the
/// process's: a test that names fixes cannot change what a test beside it runs with.
#[test]
fn a_threads_fixes_do_not_reach_other_threads() {
    // the process's selection before any thread names its own: read after, a leak of the
    // thread's fixes into the process would be read back as the process's
    let process = FIXES.lock().unwrap().unwrap_or_default();
    std::thread::spawn(|| {
        set_thread_fixes(UpstreamFixes::NAMES.iter().copied()).unwrap();
        assert_eq!(fixes(), UpstreamFixes::NAMES);
    })
    .join()
    .unwrap();
    assert_eq!(FIXES.lock().unwrap().unwrap_or_default(), process);
    let beside_is_the_process =
        std::thread::spawn(move || run("reads its fixes", |k| Ok(k.fixes == process)).unwrap())
            .join()
            .unwrap();
    assert!(beside_is_the_process);
}

/// Setting the fixes from inside a kernel call (an application's provenance rule runs in one)
/// is refused, not a wait on the kernel the call holds.
#[test]
fn setting_the_fixes_inside_a_call_is_refused() {
    all_fixes();
    let r = run("sets the fixes", |k| {
        let before = k.fixes;
        let refusal = set_fixes([]).err();
        Ok(Some((refusal, k.fixes == before)))
    });
    assert_eq!(r, Ok(Some((Some(FixesRefused::Reentered), true))));
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

/// A failure reports a caller-held input as unwritten while the caller still owns the guard.
/// The report's contents and the call's return decide this test, not a scheduling deadline.
#[test]
fn a_held_input_is_reported_not_waited_for() {
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
    let seen = seen.lock().unwrap().clone();
    drop(held);
    set_failure_hook(None);
    assert_eq!(r, Err(Error::Status(crate::error::INVALID_INPUT)));
    assert!(
        matches!(&seen[..], [Err(why)] if why.contains("held")),
        "{seen:?}"
    );
}

/// A status error is no answer, unless a solidcore-side failure or a cancellation is in its cause
/// chain: then the call fails. (Each assertion on a chain fails when `answer` reads only the
/// error on top.)
#[test]
fn a_status_error_with_a_port_failure_or_cancellation_as_cause_fails_the_call() {
    use solidcore::error::{Failure, Reason};

    let wrap = |cause: SolidError| {
        Failure::new("test::stage", Reason::Failed("a stage failed"))
            .caused_by(cause)
            .into_error()
    };
    let status = wrap(SolidError::sm_err("test::inner"));
    assert!(status.sm_status().is_some());
    assert!(matches!(answer::<()>(Err(status)), Ok(None)));

    let unported = wrap(SolidError::unported("test::callee", "test.unit"));
    assert!(unported.sm_status().is_some());
    assert!(matches!(answer::<()>(Err(unported)), Err(e) if e.is_port_failure()));
    let deep = wrap(wrap(SolidError::unported("test::callee", "test.unit")));
    assert!(answer::<()>(Err(deep)).is_err());

    let cancelled = wrap(SolidError::cancelled("test::cancelled"));
    assert!(cancelled.sm_status().is_some() && !cancelled.is_port_failure());
    assert!(answer::<()>(Err(cancelled)).is_err());

    assert!(answer::<()>(Err(SolidError::invalid_input("test::input", "bad"))).is_err());
}

/// The body's vertex points, in its order.
fn points(b: &Body) -> Vec<solidcore::Vec3> {
    let brep = b.lock();
    brep.vertex_list()
        .iter()
        .map(|&v| brep.vertex(v).unwrap().point)
        .collect()
}

/// A call whose token is cancelled already is refused before it reaches the kernel.
#[test]
fn a_cancelled_token_refuses_the_call_before_it_runs() {
    all_fixes();
    let token = CancelToken::new();
    token.cancel();
    let ran = Cell::new(false);
    let r: Result<()> = crate::cancel::within(&token, || {
        run("never runs", |_| {
            ran.set(true);
            Ok(())
        })
    });
    assert_eq!(r, Err(Error::Cancelled));
    assert!(!ran.get());
    assert!(
        make_box([0.0; 3], [1.0; 3]).is_ok(),
        "outside the scope calls run"
    );
}

/// The kernel polls the scope's token during the call: a cancel from another thread part way
/// is `Cancelled`, the body the call was editing is as it came in, nothing reaches the failure
/// callback, and the kernel the call ran on is replaced (the mark it left is gone).
#[test]
fn a_call_cancelled_part_way_leaves_its_body_and_no_kernel_state() {
    in_own_process(
        "session::tests::a_call_cancelled_part_way_leaves_its_body_and_no_kernel_state",
        || {
            all_fixes();
            let reported: Arc<Mutex<Vec<String>>> = Arc::default();
            let seen = Arc::clone(&reported);
            set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
                seen.lock().unwrap().push(f.call.to_owned());
            })));
            let b = make_box([0.0; 3], [1.0; 3]).unwrap();
            let before = points(&b);
            let token = CancelToken::new();
            let r: Result<()> = crate::cancel::within(&token, || {
                with_body("cancelled part way", &b, |k, br| {
                    let v = br.vertex_list()[0];
                    br.vertex_mut(v)?.point = solidcore::Vec3::new(9.0, 9.0, 9.0);
                    k.upstream_ub_passed.push("marker");
                    // the application cancels from its own thread; the kernel's next check sees it
                    let other = token.clone();
                    std::thread::spawn(move || other.cancel()).join().unwrap();
                    k.check_cancelled("session::tests")
                })
            });
            set_failure_hook(None);
            assert_eq!(r, Err(Error::Cancelled));
            assert_eq!(before, points(&b), "the edit was put back");
            assert!(
                reported.lock().unwrap().is_empty(),
                "a cancellation is no failure"
            );
            let fresh = run("after", |k| {
                Ok(k.upstream_ub_passed.is_empty() && k.cancel.is_none())
            })
            .unwrap();
            assert!(fresh, "the kernel was replaced and carries no token");
        },
    );
}

/// A token cancelled while the call runs takes its result away even when the kernel finished
/// first: the edit goes, and the answer is `Cancelled`, never a result that came in late.
#[test]
fn a_result_finished_after_the_cancel_is_not_returned() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let token = CancelToken::new();
    let r: Result<()> = crate::cancel::within(&token, || {
        with_body("finished after the cancel", &b, |_, br| {
            let v = br.vertex_list()[0];
            br.vertex_mut(v)?.point = solidcore::Vec3::new(9.0, 9.0, 9.0);
            token.cancel();
            Ok(())
        })
    });
    assert_eq!(r, Err(Error::Cancelled));
    assert!(
        !points(&b).contains(&solidcore::Vec3::new(9.0, 9.0, 9.0)),
        "the edit was put back"
    );
    let r: Result<bool> = run("reads", |k| Ok(k.cancel.is_none()));
    assert_eq!(r, Ok(true), "the token is off the kernel after the call");
}
