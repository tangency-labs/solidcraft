//! The session: one kernel for the process (SMLib's context is process-global too, and some of
//! its state, such as the fillet solver's latched step, lives for the whole process), every call
//! under its lock and a panic guard.
//!
//! - **Configuration:** the kernel runs with the opt-in fixes the application names
//!   ([`set_fixes`], by `UpstreamFixes` flag name); which ones is the application's choice, never
//!   ops'. Everything else about the kernel (its tolerances, context, caches) is the kernel's
//!   default: ops offers no way to change it, so every kernel the session makes is configured
//!   alike ([`fresh`]).
//! - **Panics:** a panic inside a call is caught; the kernel, which it may have left half-updated,
//!   is replaced by a fresh one of the same configuration, carrying the process-lifetime state the
//!   kernel keeps as upstream keeps it for the process (the fillet solver's latched step), and the
//!   call fails with [`Error::Internal`]. A call that only reads a body never leaves it unusable:
//!   it reads it under its own guard ([`read_body`]) or measures a copy ([`measure_body`]). A
//!   call that edits a body and panics puts it back as it came in ([`with_body`]).
//! - **Failures:** an application may install a callback ([`set_failure_hook`]) that is handed a
//!   failed call's name, why it failed and its input bodies as binary `.smb` bytes, written under
//!   the kernel's lock from bodies no panic left half-edited. The callback runs with no lock held
//!   and under its own panic guard; calling ops from it is refused ([`Error::Reentered`]), as is
//!   calling ops from inside a kernel call. Ops itself writes nothing and logs nothing.
//! - **The call running now:** [`current_call`] reads it without a lock, for crash reports.

use std::cell::Cell;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::topology::brep::p3::brep_copy;
use solidcore::{Brep, SolidError};

use crate::body::Body;
use crate::error::status;
use crate::{Error, Result};

static KERNEL: Mutex<Option<Kernel>> = Mutex::new(None);
static FIXES: Mutex<Option<UpstreamFixes>> = Mutex::new(None);

/// A failed call, as the failure callback sees it.
#[derive(Debug)]
pub struct Failure<'a> {
    /// The call's name.
    pub call: &'a str,
    /// Why it failed: its error, or the refusal its answer carries.
    pub why: &'a str,
    /// Its input bodies by the names the call gives them, each as a binary `.smb` (or why it
    /// could not be written), as they are after the call: a refused or failed edit has put its
    /// body back as it came in ([`with_body`]).
    pub inputs: Vec<(&'static str, std::result::Result<Vec<u8>, String>)>,
}

/// The failure callback an application installs.
pub type Hook = Arc<dyn Fn(&Failure<'_>) + Send + Sync>;
static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

/// A call's input bodies, by the names its failure report gives them, in the order it takes them.
pub(crate) type Inputs<'a> = &'a [(&'static str, &'a Body)];

/// Why [`set_fixes`] changed nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FixesRefused {
    /// A flag name `UpstreamFixes` does not have.
    Unknown(String),
    /// Called from inside a kernel call (an application's provenance rule, say), which holds
    /// the kernel the fixes are set on.
    Reentered,
}

impl std::fmt::Display for FixesRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FixesRefused::Unknown(name) => write!(f, "solidcore has no upstream fix named {name}"),
            FixesRefused::Reentered => write!(f, "the fixes were set from inside a kernel call"),
        }
    }
}

impl std::error::Error for FixesRefused {}

/// The kernel runs with exactly the fixes named in `on` (each an `UpstreamFixes` field name;
/// every other flag off) from the next call on. Nothing changes if a name is unknown.
///
/// # Errors
/// [`FixesRefused::Unknown`] naming the first name `UpstreamFixes` does not have;
/// [`FixesRefused::Reentered`] from inside a kernel call.
pub fn set_fixes<'a>(
    on: impl IntoIterator<Item = &'a str>,
) -> std::result::Result<(), FixesRefused> {
    if in_call() {
        return Err(FixesRefused::Reentered);
    }
    let mut fixes = UpstreamFixes::default();
    for name in on {
        *fixes
            .flag_mut(name)
            .ok_or_else(|| FixesRefused::Unknown(name.to_owned()))? = true;
    }
    *FIXES.lock().unwrap_or_else(PoisonError::into_inner) = Some(fixes);
    let mut kernel = KERNEL.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(k) = kernel.as_mut() {
        k.fixes = fixes;
    }
    Ok(())
}

/// The names of the fixes the kernel runs with now.
#[must_use]
pub fn fixes() -> Vec<&'static str> {
    current_fixes()
        .flags()
        .filter(|(_, on)| *on)
        .map(|(name, _)| name)
        .collect()
}

fn current_fixes() -> UpstreamFixes {
    FIXES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .unwrap_or_default()
}

/// Installs the failure callback (`None` removes it). It runs with no lock held; a call to ops
/// from it is refused with [`Error::Reentered`], and a panic in it is caught and dropped.
pub fn set_failure_hook(hook: Option<Hook>) {
    *HOOK.lock().unwrap_or_else(PoisonError::into_inner) = hook;
}

/// A kernel of the session's configuration: the kernel's default options and the fixes the
/// application named. The first kernel and every kernel after a panic are made here, so they
/// are configured alike.
fn fresh() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = current_fixes();
    k
}

/// The kernel that replaces `old` after a panic: [`fresh`], with the process-lifetime state
/// upstream keeps for the whole process (the fillet solver's latched step, a value set once).
fn replacement(old: Option<&Kernel>) -> Kernel {
    let k = fresh();
    if let Some(step) = old.and_then(|o| o.fillet_point_step.get()) {
        k.fillet_point_step.set(Some(step));
    }
    k
}

thread_local! {
    /// How many kernel calls this thread is inside: a panic now is caught ([`run_on`]).
    static IN_CALL: Cell<u32> = const { Cell::new(0) };
    /// Whether this thread is running the failure callback.
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
}

/// Is this thread inside a kernel call, whose panic the session catches? (Crash reports mark
/// such a panic recovered.)
#[must_use]
pub fn in_call() -> bool {
    IN_CALL.with(Cell::get) > 0
}

/// The longest call name [`current_call`] gives.
const NAME_MAX: usize = 96;
static NAME: [AtomicU8; NAME_MAX] = [const { AtomicU8::new(0) }; NAME_MAX];
static NAME_LEN: AtomicUsize = AtomicUsize::new(0);

/// `call` is running now (`None`: none is). One at a time, under the kernel's lock.
fn set_current(call: Option<&str>) {
    NAME_LEN.store(0, Ordering::SeqCst);
    let Some(call) = call else { return };
    let bytes = call.as_bytes();
    let n = bytes.len().min(NAME_MAX);
    for (slot, b) in NAME.iter().zip(bytes) {
        slot.store(*b, Ordering::Relaxed);
    }
    NAME_LEN.store(n, Ordering::SeqCst);
}

/// The kernel call running now, if any. Takes no lock, for crash reports, so it may be cut short
/// or mixed with the next call's name if read just as one starts.
#[must_use]
pub fn current_call() -> Option<String> {
    let n = NAME_LEN.load(Ordering::SeqCst);
    if n == 0 {
        return None;
    }
    let bytes: Vec<u8> = NAME
        .iter()
        .take(n)
        .map(|b| b.load(Ordering::Relaxed))
        .collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// How a call's value reads: whether it is a refusal, for the calls whose answer can carry one.
pub(crate) trait Answer {
    /// The refusal this value carries, if it is one.
    fn refusal(&self) -> Option<String> {
        None
    }
}

impl<T> Answer for Result<T> {
    fn refusal(&self) -> Option<String> {
        self.as_ref().err().map(Error::to_string)
    }
}

/// The values that are no refusal.
macro_rules! plain {
    ($($t:ty),* $(,)?) => { $(impl Answer for $t {})* };
}
plain!((), bool, f64, usize, [usize; 3], Vec<u8>, Brep);

impl<T> Answer for Option<T> {}

/// Runs `f` on the process's kernel ([`run_on`] with no inputs).
///
/// # Errors
/// As [`run_on`].
pub(crate) fn run<T: Answer>(
    call: &'static str,
    f: impl FnOnce(&mut Kernel) -> solidcore::Result<T>,
) -> Result<T> {
    run_on(call, &[], f)
}

/// The panic's message.
fn message(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

/// Runs `f` on the process's kernel, for a call that reads the bodies `inputs`. Should it fail
/// while a failure callback is installed, the callback gets them. `f` must not hold a body of
/// `inputs` locked past its return (they are written for the callback after it).
///
/// # Errors
/// [`Error::Reentered`] from inside a kernel call or the failure callback; the kernel's error
/// as [`status`] types it; [`Error::Internal`] for a panic.
pub(crate) fn run_on<T: Answer>(
    call: &'static str,
    inputs: Inputs<'_>,
    f: impl FnOnce(&mut Kernel) -> solidcore::Result<T>,
) -> Result<T> {
    run_with(call, inputs, false, f)
}

/// [`run_on`] for a call that consumes its operands (a boolean, a stitch: the kernel takes the
/// bodies and leaves them empty): while a failure callback is installed, the operands are
/// written before the call, so that a failure reports them as they came in.
///
/// # Errors
/// As [`run_on`].
pub(crate) fn run_taking<T: Answer>(
    call: &'static str,
    inputs: Inputs<'_>,
    f: impl FnOnce(&mut Kernel) -> solidcore::Result<T>,
) -> Result<T> {
    run_with(call, inputs, true, f)
}

fn run_with<T: Answer>(
    call: &'static str,
    inputs: Inputs<'_>,
    before: bool,
    f: impl FnOnce(&mut Kernel) -> solidcore::Result<T>,
) -> Result<T> {
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            IN_CALL.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    if in_call() || IN_HOOK.with(Cell::get) {
        return Err(Error::Reentered);
    }
    let mut guard = KERNEL.lock().unwrap_or_else(PoisonError::into_inner);
    let k = guard.get_or_insert_with(fresh);
    set_current(Some(call));
    IN_CALL.with(|d| d.set(d.get() + 1));
    let depth = Depth;
    // the callback as the call starts: the one a failure of this call reports to
    let hook = HOOK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    // the operands as they come in, for a call that consumes them, when someone will read them
    let snapshot = (before && hook.is_some()).then(|| written(k, inputs));
    let caught = catch_unwind(AssertUnwindSafe(|| f(k)));
    let r = match caught {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(status(e)),
        Err(p) => {
            let next = replacement(guard.as_ref());
            *guard = Some(next);
            Err(Error::Internal(format!("{call}: {}", message(p.as_ref()))))
        }
    };
    set_current(None);
    let why = match &r {
        Ok(v) => v.refusal(),
        Err(e) => Some(e.to_string()),
    };
    let report = why.and_then(|why| {
        let hook = hook?;
        // The kernel the call ran on, or the one that replaced it after a panic; still inside
        // the call, so nothing reenters, and the writer's own panic is caught.
        let inputs = match snapshot {
            Some(taken) => taken,
            None => written(guard.get_or_insert_with(fresh), inputs),
        };
        Some((hook, why, inputs))
    });
    drop(depth);
    drop(guard);
    if let Some((hook, why, inputs)) = report {
        IN_HOOK.with(|h| h.set(true));
        // A panicking callback loses nothing of the call's result.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            hook(&Failure {
                call,
                why: &why,
                inputs,
            });
        }));
        IN_HOOK.with(|h| h.set(false));
    }
    r
}

/// The failed call's inputs as `.smb` bytes, each from a consistent body: one a panic left
/// half-edited is not written, nor one still held (by the call or its caller), which is never
/// waited for under the kernel's lock.
fn written(
    k: &Kernel,
    inputs: Inputs<'_>,
) -> Vec<(&'static str, std::result::Result<Vec<u8>, String>)> {
    inputs
        .iter()
        .map(|&(what, body)| {
            let bytes = match body.try_read() {
                Err(why) => Err(why.to_owned()),
                Ok(b) => {
                    match catch_unwind(AssertUnwindSafe(|| crate::file::bytes(k, &b, false))) {
                        Ok(r) => r.map_err(|e| status(e).to_string()),
                        Err(p) => Err(format!("writing it panicked: {}", message(p.as_ref()))),
                    }
                }
            };
            (what, bytes)
        })
        .collect()
}

/// A call the kernel answers with a status, any status but success standing for no answer: an
/// error that is exactly a kernel status ([`SolidError::sm_status`]) is `None`. Every other
/// error (unported code, invalid input to an adapter, a missing record...) fails the call
/// instead of hiding as a skipped entity.
///
/// # Errors
/// The kernel's error when it stands for no status.
pub(crate) fn answer<T>(r: solidcore::Result<T>) -> solidcore::Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.sm_status().is_some() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Runs `f`, which edits `body`, on the kernel. A refused, failed or panicking edit leaves the
/// body exactly as it came in: the kernel's snapshot of it is taken first
/// ([`Brep::with_rollback`]: its naming, key, locked box, every attribute and so its
/// provenance, its notification identities and its geometry), `f` edits the body itself (so a
/// kept result is the edited body, its entities where they were), and the snapshot is put back
/// when `f` fails, answers a refusal ([`Answer::refusal`]) or panics. A
/// panic then goes on to the session with the body put back and released, so the body stays
/// usable; a panic while taking the snapshot leaves the body untouched and usable too.
///
/// # Errors
/// [`Error::Unsupported`] up front, the body untouched, for a body the kernel cannot snapshot
/// (its refusal, [`Brep::with_rollback`]); [`Error::Internal`] for a body an earlier panic left
/// unfinished; those of [`run_on`].
pub(crate) fn with_body<T: Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut Brep) -> solidcore::Result<T>,
) -> Result<T> {
    if body.is_poisoned() {
        return Err(unfinished(call));
    }
    run_on(call, &[("brep", body)], |k| {
        let mut b = body.lock();
        // the kernel puts the body back on a refusal or a panic; the panic is caught again
        // here only to release the body before it goes on
        let r = catch_unwind(AssertUnwindSafe(|| {
            b.with_rollback(
                |br| f(k, br),
                |r| r.as_ref().is_ok_and(|v| v.refusal().is_none()),
            )
        }));
        drop(b);
        r.unwrap_or_else(|p| resume_unwind(p))
    })
}

/// Runs `f`, which only reads `body`, on the kernel. The body is read under its guard, released
/// before a panic goes on, so a panic leaves the body as it was and usable.
///
/// # Errors
/// As [`with_body`].
pub(crate) fn read_body<T: Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &Brep) -> solidcore::Result<T>,
) -> Result<T> {
    if body.is_poisoned() {
        return Err(unfinished(call));
    }
    run_on(call, &[("brep", body)], |k| {
        let b = body.lock();
        let r = catch_unwind(AssertUnwindSafe(|| f(k, &b)));
        drop(b);
        r.unwrap_or_else(|p| resume_unwind(p))
    })
}

/// Runs `f`, a measure that takes the kernel's `&mut Brep` (for its caches) but answers about
/// the body without editing it, on a copy of `body`: a panic in it leaves the body as it was.
///
/// # Errors
/// As [`with_body`]; those of the kernel's copy.
pub(crate) fn measure_body<T: Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut Brep) -> solidcore::Result<T>,
) -> Result<T> {
    if body.is_poisoned() {
        return Err(unfinished(call));
    }
    run_on(call, &[("brep", body)], |k| {
        let b = body.lock();
        let copy = catch_unwind(AssertUnwindSafe(|| brep_copy(k, &b)));
        drop(b);
        let mut copy = copy.unwrap_or_else(|p| resume_unwind(p))?;
        f(k, &mut copy)
    })
}

fn unfinished(call: &'static str) -> Error {
    Error::Internal(format!(
        "{call}: the body was left unfinished by an earlier panic"
    ))
}

/// A deliberate panic inside a kernel call, which the session catches as it catches any
/// ([`Error::Internal`], the kernel replaced by a fresh one): for an application's crash-report
/// check.
///
/// # Errors
/// Always [`Error::Internal`].
#[allow(clippy::panic)]
pub fn crash_test() -> Result<()> {
    run("crash test", |_| -> std::result::Result<(), SolidError> {
        std::panic::panic_any(String::from("crash test: an internal error in the kernel"))
    })
}

#[cfg(test)]
pub(crate) mod tests;
