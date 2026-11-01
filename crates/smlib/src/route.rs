//! Which kernel this process runs on: solidcore, the Rust port of SMLib, unless the C++ SMLib
//! fork is asked for. One kernel answers every operation.
//!
//! The C++ kernel is an opt-in: for a test, a rig or a user who wants the old behaviour. A call that
//! fails on solidcore fails, and a path solidcore has no answer for says it is not supported: there is
//! no fallback to the other kernel at run time, which would hide its bugs. Asking for the C++ kernel on
//! a build without it (wasm included) stays on it too: every call answers [`crate::Error::NotBuilt`].
//!
//! `SOLIDCRAFT_KERNEL=smlib|solidcore` chooses for the whole process (the rig, the test matrix);
//! otherwise the preference ([`set_choice`]) does, and it defaults to solidcore.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// A kernel the process runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The C++ SMLib fork (tangency-labs/smlib) behind its C API.
    Smlib,
    /// solidcore (tangency-labs/solidcore), the Rust port of SMLib.
    Solidcore,
}

impl Backend {
    /// The id `SOLIDCRAFT_KERNEL`, preferences and reports spell it with.
    pub fn id(self) -> &'static str {
        match self {
            Backend::Smlib => "smlib",
            Backend::Solidcore => "solidcore",
        }
    }

    /// Back from an id; anything else is `None`.
    pub fn from_id(id: &str) -> Option<Backend> {
        [Backend::Smlib, Backend::Solidcore].into_iter().find(|b| b.id() == id)
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: this thread's operations run on the C++ kernel whatever the choice.
    static ON_CPP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Tests: how many times this thread has touched a C++ body, and called solidcore.
    static REACHED: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// Tests: this thread has touched a C++ body.
#[cfg(test)]
pub(crate) fn reached_cpp() {
    REACHED.with(|r| r.set((r.get().0 + 1, r.get().1)));
}

/// Tests: this thread has called solidcore.
#[cfg(test)]
pub(crate) fn reached_core() {
    REACHED.with(|r| r.set((r.get().0, r.get().1 + 1)));
}

/// Tests: how many times this thread has touched a C++ body and called solidcore since the last
/// time this was asked.
#[cfg(test)]
pub(crate) fn take_reached() -> (u64, u64) {
    REACHED.with(|r| r.replace((0, 0)))
}

/// Tests that exercise the C++ kernel opt in: until the guard drops, this thread's operations run
/// on it. Thread by thread, so that the other tests in the binary stay on the process's kernel. The
/// guard fails a test that never touched a C++ body, so a parity test can't quietly compare
/// solidcore with itself.
#[cfg(test)]
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub(crate) fn on_cpp() -> impl Drop {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            ON_CPP.with(|c| c.set(false));
            let (cpp, core) = REACHED.with(std::cell::Cell::get);
            if std::env::var_os("SOLIDCRAFT_TEST_REACHED").is_some() {
                eprintln!("REACHED {} cpp={cpp} core={core}", std::thread::current().name().unwrap_or("?"));
            }
            assert!(cpp > 0 || std::thread::panicking(), "this test opted in to the C++ kernel and never reached it");
        }
    }
    ON_CPP.with(|c| c.set(true));
    REACHED.with(|r| r.set((0, 0)));
    Guard
}

/// The kernel that answers now: `SOLIDCRAFT_KERNEL`, else the preference, else solidcore.
pub fn backend() -> Backend {
    #[cfg(test)]
    if ON_CPP.with(std::cell::Cell::get) {
        return Backend::Smlib;
    }
    choice()
}

/// `SOLIDCRAFT_KERNEL`, read once. An unknown value is no override (and says so in the log).
fn environment() -> Option<Backend> {
    static ENV: OnceLock<Option<Backend>> = OnceLock::new();
    *ENV.get_or_init(|| {
        let v = std::env::var("SOLIDCRAFT_KERNEL").ok()?;
        let c = Backend::from_id(&v);
        if c.is_none() {
            log::warn!(target: "smlib", "SOLIDCRAFT_KERNEL={v}: not smlib or solidcore; ignored");
        }
        c
    })
}

/// The preference: false for solidcore, true for the C++ kernel.
static PREFERS_CPP: AtomicBool = AtomicBool::new(false);

/// The choice in force: `SOLIDCRAFT_KERNEL` when it is set, else the preference.
pub fn choice() -> Backend {
    environment().unwrap_or_else(|| if PREFERS_CPP.load(Ordering::Relaxed) { Backend::Smlib } else { Backend::Solidcore })
}

/// Sets the preference. `SOLIDCRAFT_KERNEL`, when set, still wins.
pub fn set_choice(b: Backend) {
    PREFERS_CPP.store(b == Backend::Smlib, Ordering::Relaxed);
}

/// Whether `SOLIDCRAFT_KERNEL` overrides the preference in this process.
pub fn overridden() -> bool {
    environment().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ids round trip; `routed`, the old default, is not one of them.
    #[test]
    fn ids_round_trip() {
        for b in [Backend::Smlib, Backend::Solidcore] {
            assert_eq!(Backend::from_id(b.id()), Some(b));
        }
        for other in ["routed", "truck", "SMLIB", ""] {
            assert_eq!(Backend::from_id(other), None, "{other}");
        }
    }

    /// With nothing chosen the process is on solidcore. (Only solidcore is set here: the other tests
    /// in this binary build bodies meanwhile.)
    #[test]
    fn the_default_is_solidcore() {
        if overridden() {
            return;
        }
        set_choice(Backend::Solidcore);
        assert_eq!((choice(), backend()), (Backend::Solidcore, Backend::Solidcore));
    }

    /// The guard moves this thread alone onto the C++ kernel, and back when it drops.
    #[test]
    fn a_test_opts_in_to_the_cpp_kernel_thread_by_thread() {
        let before = backend();
        {
            let _cpp = on_cpp();
            reached_cpp();
            assert_eq!(backend(), Backend::Smlib);
            assert_eq!(choice(), before);
        }
        assert_eq!(backend(), before);
    }
}
