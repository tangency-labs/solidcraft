//! The session: the kernels ops calls run on, each call on a kernel of its own and under a panic
//! guard.
//!
//! - **Kernels:** a call takes an idle kernel, the one used last first, or makes a fresh one when
//!   none is idle, and gives it back when it ends ([`take`], [`give_back`]). The pool's lock is
//!   held only to take or give back a kernel, never during a call, so calls on different bodies
//!   run at once and a long call holds up only the calls on its own bodies: a call waits for a
//!   body another call holds on that body's own lock. A call that holds two bodies at once
//!   locks them in one order ([`Body::lock_pair`]), so calls on the same bodies in other orders
//!   never wait on each other for good. Every call starts from the state a fresh kernel has
//!   ([`start_clean`]: no blend station remembered; the kernel holds no position in a random
//!   sequence, each sampling call draws from the start of the seed's),
//!   so its result does not depend on the calls before it or on the kernel it is handed. At most
//!   [`most_idle`] kernels wait between calls, however many once ran side by side. A kernel
//!   holds no global state and a body's identities are valid only in the kernel that issued
//!   them, so any kernel takes any body.
//! - **Configuration:** the kernel runs with the opt-in fixes the application names
//!   ([`set_fixes`], by `UpstreamFixes` flag name); which ones is the application's choice, never
//!   ops'. Everything else about the kernel (its tolerances, context, caches) is the kernel's
//!   default: ops offers no way to change it, so every kernel the session makes is configured
//!   alike ([`fresh`]). Each call takes them as it starts. A thread may name its own
//!   ([`set_thread_fixes`]), so tests with different fixes run side by side without racing
//!   (#2983).
//! - **Panics:** a panic inside a call is caught; the call's kernel, which it may have left
//!   half-updated, is dropped, and the call fails with [`Error::Internal`]. So is the kernel of a
//!   call cancelled part way. A call that only reads a body never leaves it unusable: it reads it
//!   under its own guard ([`read_body`]) or measures a copy ([`measure_body`]). A call that edits
//!   a body and panics puts it back as it came in ([`with_body`]).
//! - **Failures:** an application may install a callback ([`set_failure_hook`]) that is handed a
//!   failed call's name, why it failed and its input bodies as binary `.smb` bytes, written on
//!   the call's kernel (a fresh one after a panic) from bodies no panic left half-edited. The
//!   callback runs with no lock held and under its own panic guard; calling ops from it is
//!   refused ([`Error::Reentered`]), as is calling ops from inside a kernel call. Ops itself
//!   writes nothing and logs nothing.
//! - **The call running now:** [`current_call`] gives the call running on the calling thread,
//!   without a lock, for crash reports: a panic hook runs on the thread that panicked.
#![cfg_attr(
    feature = "profile",
    doc = "- **Profiles** (feature `profile`): [`profiled`] returns the kernel's profile of each \
           ops call made inside it, one per call, each taken from the call's own kernel so that \
           calls on other threads never mix into it. Without the feature none of this exists."
)]

use std::cell::Cell;
use std::num::NonZeroUsize;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use solidcore::blend::fillet_section::definition::StationMemo;
use solidcore::kernel::{CancelToken, Kernel, KernelOptions, UpstreamFixes};
use solidcore::topology::brep::p3::brep_copy;
use solidcore::{Brep, SolidError};

use crate::body::Body;
use crate::error::status;
use crate::{Error, FailureCode, Result};

/// The kernels waiting between calls, the one given back last at the end.
static IDLE: Mutex<Vec<Kernel>> = Mutex::new(Vec::new());
static FIXES: Mutex<Option<UpstreamFixes>> = Mutex::new(None);

/// A failed call, as the failure callback sees it. More fields come (the call's arguments,
/// `docs/failure-codes-and-reproduction.md` 3.2): read it by its fields, never build it.
#[derive(Debug)]
#[non_exhaustive]
pub struct Failure<'a> {
    /// The call's name.
    pub call: &'a str,
    /// Why it failed: its error, or the refusal its answer carries.
    pub why: &'a str,
    /// What failed, as stable data ([`FailureCode`]): the same code
    /// [`take_failure_code`] gives after the call.
    pub code: FailureCode,
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

/// The fix set `on` names.
fn named<'a>(
    on: impl IntoIterator<Item = &'a str>,
) -> std::result::Result<UpstreamFixes, FixesRefused> {
    if in_call() {
        return Err(FixesRefused::Reentered);
    }
    let mut fixes = UpstreamFixes::default();
    for name in on {
        *fixes
            .flag_mut(name)
            .ok_or_else(|| FixesRefused::Unknown(name.to_owned()))? = true;
    }
    Ok(fixes)
}

/// The kernel runs with exactly the fixes named in `on` (each an `UpstreamFixes` field name;
/// every other flag off) from the next call on. Nothing changes if a name is unknown.
///
/// # Errors
/// [`FixesRefused::Unknown`] naming the first name `UpstreamFixes` does not have;
/// [`FixesRefused::Reentered`] from inside a kernel call.
pub fn set_fixes<'a>(
    on: impl IntoIterator<Item = &'a str>,
) -> std::result::Result<(), FixesRefused> {
    let fixes = named(on)?;
    *FIXES.lock().unwrap_or_else(PoisonError::into_inner) = Some(fixes);
    Ok(())
}

/// [`set_fixes`] for the calling thread alone: its calls run with exactly these fixes whatever
/// other threads name, so tests on parallel threads with different fixes share the session's
/// kernels without racing. A thread that never calls it runs with the process's ([`set_fixes`]).
///
/// Not part of the API: it is public only so that this crate's integration tests, which are
/// separate crates, can name their fixes per test. An application names its fixes once
/// ([`set_fixes`]); a result that depends on the calling thread is not one an application
/// should rely on.
///
/// # Errors
/// As [`set_fixes`].
#[doc(hidden)]
pub fn set_thread_fixes<'a>(
    on: impl IntoIterator<Item = &'a str>,
) -> std::result::Result<(), FixesRefused> {
    let fixes = named(on)?;
    THREAD_FIXES.with(|f| f.set(Some(fixes)));
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
    if let Some(fixes) = THREAD_FIXES.with(Cell::get) {
        return fixes;
    }
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

/// What a work counter of the kernel reads ([`counted`]).
#[cfg(test)]
pub(crate) type Counter = fn(&solidcore::kernel::WorkCounters) -> u64;

/// `f`'s value, and how much the kernels' work counter `counter` grew in the ops calls this
/// thread makes inside it (tests: what the kernel did, whichever kernels the calls ran on).
#[cfg(test)]
pub(crate) fn counted<T>(counter: Counter, f: impl FnOnce() -> T) -> (T, u64) {
    let outer = COUNTED.with(|c| c.replace(Some((counter, 0))));
    let value = f();
    let grew = COUNTED.with(|c| c.replace(outer)).map_or(0, |(_, n)| n);
    (value, grew)
}

/// A kernel of the session's configuration: the kernel's default options and the fixes the
/// application named. Every kernel the pool makes is made here, so they are configured alike.
fn fresh() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = current_fixes();
    k
}

/// The most kernels kept idle between calls: as many as calls the machine runs at once. More
/// would only be dropped later; fewer leave a fresh kernel to be made for the next calls side by
/// side. One where the machine cannot say (wasm32, where calls run one at a time).
fn most_idle() -> usize {
    static MOST: OnceLock<usize> = OnceLock::new();
    *MOST.get_or_init(|| std::thread::available_parallelism().map_or(1, NonZeroUsize::get))
}

/// A kernel for a call: the idle one given back last, or a fresh one when none is idle.
fn take() -> Kernel {
    let idle = IDLE.lock().unwrap_or_else(PoisonError::into_inner).pop();
    idle.unwrap_or_else(fresh)
}

/// A call starts from the state a fresh kernel has, whatever kernel it was handed and whatever
/// ran on it before: it holds no position in a random sequence, and the stations the blend
/// definitions solved last are forgotten. A call's result is then the same after any history
/// and on any kernel of the pool, so calls side by side on several threads give what they give
/// one after another. (The kernel's other caches never change a result, only its speed, and
/// stay.)
fn start_clean(k: &mut Kernel) {
    *k.blend_station_memo.borrow_mut() = StationMemo::default();
}

/// A call's kernel, fit to use again, back among the idle ones (dropped when [`most_idle`] wait
/// already).
fn give_back(k: Kernel) {
    let surplus = {
        let mut idle = IDLE.lock().unwrap_or_else(PoisonError::into_inner);
        if idle.len() < most_idle() {
            idle.push(k);
            None
        } else {
            Some(k)
        }
    };
    // dropped with no lock held
    drop(surplus);
}

/// A call's fuel as [`with_body`] found it: `(spent, limit)` when the tank was empty.
#[derive(Clone, Copy)]
struct Empty(Option<(u64, u64)>);

thread_local! {
    /// This thread's own fixes, when it named some ([`set_thread_fixes`]).
    static THREAD_FIXES: Cell<Option<UpstreamFixes>> = const { Cell::new(None) };
    /// How many kernel calls this thread is inside: a panic now is caught ([`run_on`]).
    static IN_CALL: Cell<u32> = const { Cell::new(0) };
    /// Whether this thread is running the failure callback.
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
    /// Whether the call running on this thread was cancelled, as [`with_body`] decided it when it
    /// kept or put back its body: [`run_with`] then answers the same, so a cancel landing between
    /// the two never leaves an edit in place under a `Cancelled` answer.
    static STOPPED: Cell<Option<bool>> = const { Cell::new(None) };
    /// The fuel tank of the call running on this thread ([`crate::fuel`]), for [`with_body`]
    /// to read whether it ran out when it keeps or puts back its body.
    static TANK: std::cell::RefCell<Option<CancelToken>> = const { std::cell::RefCell::new(None) };
    /// Whether the call running on this thread ran out of fuel, and how far, as [`with_body`]
    /// decided it when it kept or put back its body: [`run_with`] then answers the same.
    static EMPTY: Cell<Option<Empty>> = const { Cell::new(None) };
    /// The kernel's own statement of the last error a call on this thread failed with
    /// ([`take_failure_detail`]).
    static DETAIL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// The call running on this thread ([`current_call`]).
    static CURRENT: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The code of the last failed call on this thread ([`take_failure_code`]).
    static CODE: Cell<Option<FailureCode>> = const { Cell::new(None) };
    /// The last kernel error the call running on this thread typed into an [`Error`] ([`noted`]):
    /// its kind's code, its operation, and the name of the variant it became.
    static NOTED: Cell<Option<(u16, &'static str, &'static str)>> = const { Cell::new(None) };
}

/// `r`, the kernel's error `e` as ops types it, noted for the call running on this thread: when
/// the call's answer refuses with an error of `r`'s variant (`r` itself, or `r` with what the
/// call adds to it, a point, an index), its [`FailureCode`] is `e`'s kind and operation, not a
/// failure ops decided itself. Every place ops types a kernel error into an [`Error`] passes it
/// here; outside a call it is `r`, untouched. The last note wins: a call that types two kernel
/// errors into one variant, drops one and refuses with the other, reports the later one's code,
/// so a call that tries and gives up something puts its earlier note back ([`note_back`]).
pub(crate) fn noted(e: &SolidError, r: Error) -> Error {
    noted_as(e.kind().code(), e.operation(), r)
}

/// What the call running on this thread noted last ([`noted`]), for work that tries something
/// and gives it up to put back ([`note_back`]): a refusal it returns is the one noted before.
pub(crate) type Note = Option<(u16, &'static str, &'static str)>;

/// The note of the call running on this thread, to put back with [`note_back`].
pub(crate) fn note() -> Note {
    NOTED.try_with(Cell::get).ok().flatten()
}

/// Puts back a note taken with [`note`], forgetting what was noted since.
pub(crate) fn note_back(n: Note) {
    let _ = NOTED.try_with(|c| c.set(n));
}

/// [`noted`] for a kernel error already taken apart: its kind's code and its operation.
pub(crate) fn noted_as(kind: u16, operation: &'static str, r: Error) -> Error {
    if in_call() {
        // (a thread being torn down notes nothing)
        let _ = NOTED.try_with(|n| n.set(Some((kind, operation, r.name()))));
    }
    r
}

#[cfg(test)]
thread_local! {
    /// The work counter this thread counts while it runs inside [`counted`], and how much it
    /// grew so far.
    static COUNTED: Cell<Option<(Counter, u64)>> = const { Cell::new(None) };
}

#[cfg(feature = "profile")]
thread_local! {
    /// The profiles of this thread's calls, while it runs inside [`profiled`]. Per thread:
    /// calls on other threads run at the same time, and each collects only its own calls.
    static PROFILES: std::cell::RefCell<Option<Vec<CallProfile>>> =
        const { std::cell::RefCell::new(None) };
}

/// One ops call's kernel profile (feature `profile`): what the kernel's profiler recorded
/// inside it ([`solidcore::profile`]).
#[cfg(feature = "profile")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallProfile {
    /// The call's name, as [`current_call`] gives it.
    pub call: &'static str,
    /// What the profiler recorded inside it. Its first scope is the call itself, named
    /// `call`, whose counters are the call's totals; the kernel's own scopes nest under it.
    pub profile: solidcore::profile::Profile,
}

/// Runs `f` and returns, beside its value, the kernel profile of each ops call this thread
/// makes inside it, in order (feature `profile`). Each call's profile is taken from its own
/// kernel as the call ends, so calls other threads make meanwhile are in none of them;
/// a call that panics or is cancelled reports what ran before. Nested, the inner `profiled`
/// returns its calls and the outer one gets them too.
#[cfg(feature = "profile")]
pub fn profiled<T>(f: impl FnOnce() -> T) -> (T, Vec<CallProfile>) {
    /// Puts the enclosing `profiled`'s collection back (this one's calls added to it), on a
    /// return or a panic out of `f`.
    struct Restore(Option<Vec<CallProfile>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let outer = self.0.take();
            PROFILES.with(|p| {
                let mut p = p.borrow_mut();
                let inner = p.take();
                *p = outer.map(|mut o| {
                    o.extend(inner.into_iter().flatten());
                    o
                });
            });
        }
    }
    let outer = PROFILES.with(|p| p.borrow_mut().replace(Vec::new()));
    let restore = Restore(outer);
    let value = f();
    let mine = PROFILES.with(|p| p.borrow().clone().unwrap_or_default());
    drop(restore);
    (value, mine)
}

/// Is this thread inside a kernel call, whose panic the session catches? (Crash reports mark
/// such a panic recovered.)
#[must_use]
pub fn in_call() -> bool {
    IN_CALL.with(Cell::get) > 0
}

/// The kernel call running on the calling thread, if any: for crash reports, which a panic hook
/// writes on the thread that panicked. Calls on other threads run at the same time, each its
/// own thread's. Takes no lock; `None` too on a thread being torn down.
#[must_use]
pub fn current_call() -> Option<String> {
    CURRENT
        .try_with(Cell::get)
        .ok()
        .flatten()
        .map(str::to_owned)
}

/// The kernel's own statement of why the last call on this thread failed with a kernel error:
/// the error and the chain of what caused it, in the kernel's words, as it was before the call
/// typed it into an [`Error`] (which keeps the kernel's status or its kind, not its words).
/// For an application's logs and failure reports, beside the typed error it words for its
/// users. Taken: the next call returns `None` until a call on this thread fails again, so an
/// application takes it before a call (to drop an older one) and after the call fails. `None`
/// too for a failure that is no kernel error (a refusal the call itself types, a panic, a
/// cancellation).
#[must_use]
pub fn take_failure_detail() -> Option<String> {
    DETAIL.with(std::cell::RefCell::take)
}

/// What the last call on this thread that failed failed with, as stable data
/// ([`FailureCode`]): for an application's failure reports, to group them by and to match a
/// replay against, beside the typed error it words for its users. Every failure of a call the
/// session runs gives one, a refusal its answer carries included; a cancellation is no failure
/// and gives none. An argument an operation refuses before it starts a call is no call's
/// failure and leaves none: an application that wants a code for it makes one
/// ([`FailureCode::of_ops`]). Taken, as
/// [`take_failure_detail`] is: the next call returns `None` until a call on this thread fails
/// again, so an application takes it before a call (to drop an older one) and after the call
/// fails.
///
/// The code is the kernel's for a call the kernel failed: its error's kind and operation, the
/// error ops' answer refuses with being that kernel error as ops types it. It is
/// [`FailureCode::OPS`] with the [`Error`]'s variant name for a failure ops decided itself (an
/// argument refused, a result its own checks refuse, a panic caught).
#[must_use]
pub fn take_failure_code() -> Option<FailureCode> {
    CODE.with(Cell::take)
}

/// How a call's value reads: whether it is a refusal, for the calls whose answer can carry one.
pub(crate) trait Answer {
    /// The refusal this value carries, if it is one.
    fn refusal(&self) -> Option<Error> {
        None
    }
}

impl<T> Answer for Result<T> {
    fn refusal(&self) -> Option<Error> {
        self.as_ref().err().cloned()
    }
}

/// The values that are no refusal.
macro_rules! plain {
    ($($t:ty),* $(,)?) => { $(impl Answer for $t {})* };
}
plain!(
    (),
    bool,
    f64,
    usize,
    [usize; 3],
    Vec<u8>,
    Vec<usize>,
    Vec<solidcore::query::brep::FaceHit>,
    Vec<Vec<[f64; 3]>>,
    Brep,
);

impl<T> Answer for Option<T> {}

/// Runs `f` on a kernel of its own ([`run_on`] with no inputs).
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

/// Runs `f` on a kernel of its own ([`take`]), for a call that reads the bodies `inputs`. Should
/// it fail while a failure callback is installed, the callback gets them. `f` must not hold a
/// body of `inputs` locked past its return (they are written for the callback after it).
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
    /// The call ends on this thread, by its return or a panic.
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(None));
            IN_CALL.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    if in_call() || IN_HOOK.with(Cell::get) {
        return Err(Error::Reentered);
    }
    // Cancelled already: refused before it waits for the kernel.
    let token = crate::cancel::current();
    if token.as_ref().is_some_and(CancelToken::is_cancelled) {
        return Err(Error::Cancelled);
    }
    let mut k = take();
    start_clean(&mut k);
    k.fixes = current_fixes();
    // The call's fuel: a tank the kernel's checks spend, joined with the application's token
    // so that either stops it. Counting changes nothing the kernel computes.
    let tank = crate::fuel::limit(call).map(CancelToken::with_fuel);
    k.cancel = match (&tank, &token) {
        (Some(t), Some(c)) => Some(t.either(c)),
        (Some(t), None) => Some(t.clone()),
        (None, c) => c.clone(),
    };
    TANK.with(|t| t.replace(tank.clone()));
    EMPTY.with(|d| d.set(None));
    CURRENT.with(|c| c.set(Some(call)));
    IN_CALL.with(|d| d.set(d.get() + 1));
    let depth = Depth;
    // the callback as the call starts: the one a failure of this call reports to
    let hook = HOOK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    // the operands as they come in, for a call that consumes them, when someone will read them
    let snapshot = (before && hook.is_some()).then(|| written(&k, inputs));
    STOPPED.with(|d| d.set(None));
    // a kernel error an earlier call typed is not this call's
    NOTED.with(|n| n.set(None));
    // this thread collects profiles: the call's alone (earlier calls on this kernel left
    // records in it, dropped here)
    #[cfg(feature = "profile")]
    let profiling = PROFILES.with(|p| p.borrow().is_some());
    #[cfg(feature = "profile")]
    if profiling {
        let _ = k.take_profile();
    }
    #[cfg(test)]
    let counting = COUNTED
        .with(Cell::get)
        .map(|(counter, _)| counter(&k.work_counters));
    let caught = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(feature = "profile")]
        let _scope = profiling.then(|| k.profiler.scope(call));
        f(&mut k)
    }));
    // (its scope closed with the closure, a panic's unwinding included)
    #[cfg(feature = "profile")]
    if profiling {
        let profile = k.take_profile();
        PROFILES.with(|p| {
            if let Some(v) = p.borrow_mut().as_mut() {
                v.push(CallProfile { call, profile });
            }
        });
    }
    #[cfg(test)]
    if let (Some(start), Some((counter, n))) = (counting, COUNTED.with(Cell::get)) {
        let grew = counter(&k.work_counters).saturating_sub(start);
        COUNTED.with(|c| c.set(Some((counter, n.saturating_add(grew)))));
    }
    let stopped = STOPPED
        .with(Cell::take)
        .unwrap_or_else(|| token.as_ref().is_some_and(CancelToken::is_cancelled));
    let empty = EMPTY
        .with(Cell::take)
        .map_or_else(|| tank.as_ref().and_then(CancelToken::out_of_fuel), |e| e.0);
    TANK.with(|t| t.replace(None));
    let noted = NOTED.with(Cell::take);
    let mut kernel_code = None;
    let (r, fit) = match caught {
        // Cancelled during the call: no result, even one the kernel finished first, and a
        // kernel whose caches it may have left half-built is not used again.
        Ok(_) if stopped => (Err(Error::Cancelled), false),
        // Out of fuel: the same, with the fuel it spent. The kernel's check that found the
        // tank empty names where it stopped, when its error came back up.
        Ok(r) if empty.is_some() => {
            let (spent, limit) = empty.unwrap_or_default();
            let operation = match &r {
                Err(e) if e.is_out_of_fuel() => e.operation(),
                _ => "OutOfFuel",
            };
            let e = Error::OutOfFuel {
                op: call,
                spent,
                limit,
            };
            DETAIL.with(|d| d.replace(Some(e.to_string())));
            kernel_code = Some(FailureCode {
                call,
                kind: solidcore::error::ErrorKind::OutOfFuel.code(),
                operation,
            });
            (Err(e), false)
        }
        Ok(Ok(v)) => (Ok(v), true),
        Ok(Err(e)) => {
            DETAIL.with(|d| d.replace(Some(e.to_string())));
            kernel_code = Some(FailureCode::of_kernel(call, &e));
            (Err(status(e)), true)
        }
        Err(p) => (
            Err(Error::Internal(format!("{call}: {}", message(p.as_ref())))),
            false,
        ),
    };
    // the kernel a panic or a cancel may have left half-updated is dropped: another writes the
    // report and goes back to the pool in its place
    if !fit {
        k = take();
    }
    k.cancel = None;
    // A cancellation is the application's own: no failure to report.
    let failed = match &r {
        Err(Error::Cancelled) => None,
        Ok(v) => v.refusal(),
        Err(e) => Some(e.clone()),
    };
    let why = failed.map(|e| {
        // the kernel's error the call failed with, or the one its refusal is as ops typed it
        let code = kernel_code.unwrap_or_else(|| match noted {
            Some((kind, operation, typed)) if typed == e.name() => FailureCode {
                call,
                kind,
                operation,
            },
            _ => FailureCode::of_ops(call, &e),
        });
        CODE.with(|c| c.set(Some(code)));
        (e.to_string(), code)
    });
    let report = why.and_then(|(why, code)| {
        let hook = hook?;
        // The kernel the call ran on, or the one that replaced it after a panic; still inside
        // the call, so nothing reenters, and the writer's own panic is caught.
        let inputs = match snapshot {
            Some(taken) => taken,
            None => written(&k, inputs),
        };
        Some((hook, why, code, inputs))
    });
    drop(depth);
    give_back(k);
    if let Some((hook, why, code, inputs)) = report {
        IN_HOOK.with(|h| h.set(true));
        // A panicking callback loses nothing of the call's result.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            hook(&Failure {
                call,
                why: &why,
                code,
                inputs,
            });
        }));
        IN_HOOK.with(|h| h.set(false));
    }
    r
}

/// The failed call's inputs as `.smb` bytes, each from a consistent body: one a panic left
/// half-edited is not written, nor one still held (by the call, its caller or another call),
/// which is never waited for.
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
/// instead of hiding as a skipped entity. So does a status error whose cause chain
/// ([`SolidError::cause`]) holds a solidcore-side failure ([`SolidError::is_port_failure`]) or a
/// cancellation, or a blend certificate that spent its work budget
/// ([`SolidError::is_not_certified`]): a stage that failed because a call was unported, because
/// the caller cancelled or because the work was given up, is no answer of the operation on its
/// input.
///
/// # Errors
/// The kernel's error when it stands for no status, or when its cause chain holds a port
/// failure, a cancellation or a certificate past its budget.
pub(crate) fn answer<T>(r: solidcore::Result<T>) -> solidcore::Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e)
            if e.is_status_answer()
                && !std::iter::successors(Some(&e), |e| e.cause())
                    .any(SolidError::is_not_certified) =>
        {
            Ok(None)
        }
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
        let r = catch_unwind(AssertUnwindSafe(|| b.with_rollback(|br| f(k, br), kept)));
        drop(b);
        r.unwrap_or_else(|p| resume_unwind(p))
    })
}

/// Whether [`with_body`] keeps the edit that answered `r`: an answer that is no refusal, of a
/// call not cancelled part way and not out of fuel (such a call returns no result
/// ([`run_with`]): its edit goes too).
fn kept<T: Answer>(r: &solidcore::Result<T>) -> bool {
    let stopped = crate::cancel::is_cancelled();
    STOPPED.with(|d| d.set(Some(stopped)));
    // out of fuel part way: no result either, so the edit goes
    let empty = TANK.with(|t| t.borrow().as_ref().and_then(CancelToken::out_of_fuel));
    EMPTY.with(|d| d.set(Some(Empty(empty))));
    r.as_ref().is_ok_and(|v| v.refusal().is_none()) && !stopped && empty.is_none()
}

/// Runs a measure with a read-only form on the kernel, under one lock of `body`: `read`
/// through the body's shared door, which leaves the body and its proof as they are; where it
/// answers `None` (it found, before doing any work, that the body lacks what the read needs),
/// `edit` as [`with_body`] runs it, under the same lock: the body's snapshot taken first and
/// put back on a refusal, a failure or a panic. A panic leaves the body as it was and usable,
/// released before the panic goes on.
///
/// # Errors
/// As [`with_body`].
pub(crate) fn read_else_edit<T: Answer>(
    call: &'static str,
    body: &Body,
    read: impl FnOnce(&mut Kernel, &Brep) -> solidcore::Result<Option<T>>,
    edit: impl FnOnce(&mut Kernel, &mut Brep) -> solidcore::Result<T>,
) -> Result<T> {
    if body.is_poisoned() {
        return Err(unfinished(call));
    }
    run_on(call, &[("brep", body)], |k| {
        let mut b = body.lock();
        let r = catch_unwind(AssertUnwindSafe(|| match read(k, &b)? {
            Some(v) => Ok(v),
            None => b.with_rollback(|br| edit(k, br), kept),
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
    read_held(call, body, |k, held| f(k, held))
}

/// [`read_body`] for a call that also reads what the body holds beside its B-rep (its proof,
/// its template), under the one lock: `f` is given the body's guard.
///
/// # Errors
/// As [`read_body`].
pub(crate) fn read_held<T: Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &crate::body::BodyGuard<'_>) -> solidcore::Result<T>,
) -> Result<T> {
    if body.is_poisoned() {
        return Err(unfinished(call));
    }
    run_on(call, &[("brep", body)], |k| read_locked(body, |b| f(k, b)))
}

/// Reads a body within a running kernel call. Release the unchanged body before sending
/// a panic on to the session, so a failed read does not poison the input.
pub(crate) fn read_locked<T>(body: &Body, f: impl FnOnce(&crate::body::BodyGuard<'_>) -> T) -> T {
    let b = body.lock();
    let r = catch_unwind(AssertUnwindSafe(|| f(&b)));
    drop(b);
    r.unwrap_or_else(|p| resume_unwind(p))
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
/// ([`Error::Internal`], its kernel dropped): for an application's crash-report
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
