//! The adapter this crate's GPU tests run on, from the crate's own test variables (an app's
//! variables are the app's):
//!
//! - `VIEWPORT_TEST_GPU` (`auto`, `hardware`, `software`, `cpu`; `auto` when unset): the policy
//!   the tests open their devices under.
//! - `VIEWPORT_TEST_REQUIRE_GPU`: set, a test that finds no adapter fails instead of skipping.
//!
//! CI sets both on its GPU jobs, so a runner that loses its software adapter fails loudly.
#![cfg(not(target_arch = "wasm32"))]

use super::select::Policy;

/// The variable naming the tests' adapter policy.
pub(crate) const POLICY: &str = "VIEWPORT_TEST_GPU";
/// The variable that makes a missing adapter a failure.
pub(crate) const REQUIRE: &str = "VIEWPORT_TEST_REQUIRE_GPU";

/// The policy `value` names (`None`: unset, [`Policy::Auto`]); a value that names none fails the
/// test that asked.
pub(crate) fn policy_from(value: Option<&str>) -> Policy {
    match value {
        Some(v) => Policy::parse(v).unwrap_or_else(|| panic!("{POLICY} must be auto, hardware, software or cpu, not `{v}`")),
        None => Policy::Auto,
    }
}

/// The tests' adapter policy ([`policy_from`] of `VIEWPORT_TEST_GPU`).
pub(crate) fn policy() -> Policy {
    policy_from(std::env::var(POLICY).ok().as_deref())
}

/// Whether a missing adapter fails the test: `VIEWPORT_TEST_REQUIRE_GPU` is set.
pub(crate) fn required() -> bool {
    std::env::var_os(REQUIRE).is_some()
}

/// For a test that could not open a device (`why`): `true`, with the reason printed, when it may
/// skip; a failure when a GPU is `required`.
pub(crate) fn skip_unless_required(required: bool, why: &dyn std::fmt::Display) -> bool {
    assert!(!required, "a GPU is required here ({REQUIRE}): {why}");
    eprintln!("skipped: {why}");
    true
}

/// An adapter, device and queue, as [`crate::open_headless`] hands them out.
type Opened = (wgpu::Adapter, wgpu::Device, wgpu::Queue);

/// Which tests share a device with each other ([`shared_headless`]). A capturer replaces its
/// device's uncaptured-error handler with one that logs, and that handler belongs to the device.
/// The other tests rely on wgpu's own handler, which panics, so a stray GPU error fails them.
/// The capturers therefore share a device apart from the other tests' one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sharers {
    /// Every test that opens a device with [`crate::open_headless`].
    Tests,
    /// The headless capturers ([`crate::capture::Capturer::headless`] and its variants).
    Capturers,
}

/// The device `sharers` of this crate's tests share under `policy`; [`crate::open_headless`] is
/// this for [`Sharers::Tests`] in a test build. The first test that asks opens it, every test that
/// asks while it is open gets the same one, and it closes when the last test thread that asked for
/// it ends.
///
/// With a device of its own, every test closed one as it ended. NVIDIA's Linux driver deadlocks
/// when one thread ends while another is closing a Vulkan device (#10232), and libtest ends a
/// thread with every test, so the test process froze. Shared, a run closes one device or a few.
/// The device is not kept until the process exits: the same driver crashed at exit while a device
/// was still open.
///
/// A test that must have a device to itself (one that loses it, say) opens it with
/// [`crate::open_own_headless`].
pub(crate) fn shared_headless(sharers: Sharers, policy: Policy) -> crate::Result<Opened> {
    shared(sharers, policy).map(|opened| (*opened).clone())
}

/// [`shared_headless`]'s device as its sharers hold it: one allocation for each open device.
fn shared(sharers: Sharers, policy: Policy) -> crate::Result<std::sync::Arc<Opened>> {
    use std::sync::{Arc, Mutex, PoisonError, Weak};
    static SHARED: Mutex<Vec<((Sharers, Policy), Weak<Opened>)>> = Mutex::new(Vec::new());
    thread_local! {
        /// The shared devices this thread asked for, held until the thread ends.
        static HELD: std::cell::RefCell<Vec<Arc<Opened>>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    // Held while a device opens, so tests that start together open one device, not one each.
    let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    let key = (sharers, policy);
    let open = shared.iter().find(|(k, _)| *k == key).and_then(|(_, opened)| opened.upgrade());
    let opened = match open {
        Some(opened) => opened,
        None => {
            let opened = Arc::new(crate::open_own_headless(policy)?);
            shared.retain(|(k, _)| *k != key);
            shared.push((key, Arc::downgrade(&opened)));
            opened
        }
    };
    drop(shared);
    // The last test thread to end closes the device from `HELD`'s destructor, and closing it
    // takes wgpu's snatch lock. In a debug build that lock records itself in a thread-local of
    // wgpu's, and a thread's thread-locals are destroyed in the reverse order of their first use
    // (glibc). A thread that touched `HELD` before wgpu's thread-local destroyed wgpu's first, and
    // the process aborted as the thread ended ("thread local panicked on drop", #10232). A poll
    // takes the snatch lock, so wgpu's thread-local is in use before `HELD` and outlives it.
    let _ = opened.1.poll(wgpu::PollType::Poll);
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if !held.iter().any(|mine| Arc::ptr_eq(mine, &opened)) {
            held.push(opened.clone());
        }
    });
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test that asks twice gets one device, not a second one (#10232). The allocations are
    /// compared, not the devices: wgpu compares a device by an id that the first device of every
    /// instance shares.
    #[test]
    fn a_test_thread_gets_the_same_shared_device_each_time_it_asks() {
        let first = match shared(Sharers::Tests, policy()) {
            Ok(opened) => opened,
            Err(e) if skip_unless_required(required(), &e) => return,
            Err(e) => panic!("{e}"),
        };
        let second = shared(Sharers::Tests, policy()).unwrap_or_else(|e| panic!("{e}"));
        assert!(std::sync::Arc::ptr_eq(&first, &second), "the second ask opened another device");
    }

    /// A capturer logs its device's uncaptured errors, and that handler belongs to the device: the
    /// capturers' device is not the one whose errors fail the other tests.
    #[test]
    fn the_capturers_share_a_device_apart_from_the_other_tests() {
        use std::sync::Arc;
        let tests = match shared(Sharers::Tests, policy()) {
            Ok(opened) => opened,
            Err(e) if skip_unless_required(required(), &e) => return,
            Err(e) => panic!("{e}"),
        };
        let capturers = shared(Sharers::Capturers, policy()).unwrap_or_else(|e| panic!("{e}"));
        assert!(!Arc::ptr_eq(&tests, &capturers), "the capturers got the other tests' device");
        let again = shared(Sharers::Capturers, policy()).unwrap_or_else(|e| panic!("{e}"));
        assert!(Arc::ptr_eq(&capturers, &again), "the second capturer opened another device");
    }

    #[test]
    fn the_tests_policy_comes_from_the_crates_own_variable() {
        assert_eq!(policy_from(None), Policy::Auto);
        assert_eq!(policy_from(Some("software")), Policy::Software);
        assert_eq!(policy_from(Some(" Hardware ")), Policy::Hardware);
    }

    #[test]
    #[should_panic(expected = "VIEWPORT_TEST_GPU must be")]
    fn a_policy_variable_that_names_none_fails() {
        policy_from(Some("fast"));
    }

    /// Without a required GPU a test skips; with one (`VIEWPORT_TEST_REQUIRE_GPU`) it fails.
    #[test]
    fn a_missing_adapter_skips_unless_a_gpu_is_required() {
        assert!(skip_unless_required(false, &"no adapter"));
    }

    #[test]
    #[should_panic(expected = "a GPU is required here (VIEWPORT_TEST_REQUIRE_GPU): no adapter")]
    fn a_missing_adapter_fails_when_a_gpu_is_required() {
        skip_unless_required(true, &"no adapter");
    }
}
