//! What a failing solidcore call leaves behind, and which one is running: `smlib_set_dump_dir` and
//! `smlib_current_call` for this kernel. While a folder is set, a call that fails leaves its B-rep
//! inputs in it: `<n>-<call>.txt`, what failed, and `<n>-<call>-<input>.smb`, each input as a
//! binary `.smb`, which `Brep::read` reads back. The inputs are the caller's bodies, which it keeps
//! until the call's result is known ([`super::run_on`]); they are serialised only when the call
//! fails, so a call that answers costs nothing more than it would with no folder.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use solidcore::kernel::Kernel;
use solidcore::topology::Brep;

use super::Body;
use crate::{Error, Result};

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A call's B-rep inputs, by the names its dump gives them, in the order it takes them.
pub(crate) type Inputs<'a> = &'a [(&'static str, &'a Body)];

#[cfg(test)]
thread_local! {
    /// How many inputs this thread's calls serialised, for the tests.
    pub(crate) static SERIALISED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Where failing calls leave their inputs; `None` stops it.
pub(crate) fn set_dir(dir: Option<PathBuf>) {
    *DIR.lock().unwrap_or_else(PoisonError::into_inner) = dir;
}

/// `b` as a binary `.smb`.
fn serialise(k: &Kernel, b: &Brep) -> std::result::Result<Vec<u8>, String> {
    #[cfg(test)]
    SERIALISED.with(|n| n.set(n.get() + 1));
    super::file::bytes(k, b, false).map_err(|e| e.to_string())
}

fn safe(call: &str) -> String {
    call.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// The running call failed: its `inputs` go to the folder, as they are now (a call that edits a
/// body in place, a query's caches, leaves it edited). Failing to write them fails nothing else.
pub(super) fn failed(k: &Kernel, call: &str, why: &str, inputs: Inputs<'_>) {
    let Some(dir) = DIR.lock().unwrap_or_else(PoisonError::into_inner).clone() else { return };
    let n = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stem = format!("{n:06}-{}", safe(call));
    let mut text = format!("{call} -> {why}\nnumeric arguments are not recorded\n");
    for &(what, body) in inputs {
        let bytes = serialise(k, &body.0.lock().unwrap_or_else(PoisonError::into_inner));
        match bytes {
            Ok(b) => {
                let name = format!("{stem}-{what}.smb");
                if let Err(e) = std::fs::write(dir.join(&name), b) {
                    log::warn!(target: "smlib", "{name}: {e}");
                }
                text.push_str(&format!("input {what}: {name}\n"));
            }
            Err(e) => text.push_str(&format!("input {what}: could not be written: {e}\n")),
        }
    }
    if let Err(e) = std::fs::write(dir.join(format!("{stem}.txt")), text) {
        log::warn!(target: "smlib", "{stem}.txt: {e}");
    }
}

/// How a call's value reads: whether it is a refusal, for the calls that answer with one.
pub(crate) trait Answer {
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
plain!(
    (),
    bool,
    f64,
    usize,
    [usize; 3],
    ([f64; 3], [f64; 3]),
    Vec<u8>,
    Vec<[f64; 3]>,
    Brep,
    solidcore::geom::vector3d::Point3,
    crate::history::Raw,
    crate::MassProperties,
    Vec<crate::trace::Pick>
);

impl<T> Answer for Option<T> {}

/// The longest call name [`current`] gives.
const NAME_MAX: usize = 96;
static NAME: [AtomicU8; NAME_MAX] = [const { AtomicU8::new(0) }; NAME_MAX];
static NAME_LEN: AtomicUsize = AtomicUsize::new(0);

/// `call` is running now (`None`: none is). One at a time, under the kernel's lock.
pub(super) fn set_current(call: Option<&str>) {
    NAME_LEN.store(0, Ordering::SeqCst);
    let Some(call) = call else { return };
    let bytes = call.as_bytes();
    let n = bytes.len().min(NAME_MAX);
    for (slot, b) in NAME.iter().zip(bytes) {
        slot.store(*b, Ordering::Relaxed);
    }
    NAME_LEN.store(n, Ordering::SeqCst);
}

/// The call running now. Takes no lock, for crash reports, so it may be cut short or mixed with the
/// next call's name if read just as one starts.
pub(crate) fn current() -> Option<String> {
    let n = NAME_LEN.load(Ordering::SeqCst);
    if n == 0 {
        return None;
    }
    let bytes: Vec<u8> = NAME.iter().take(n).map(|b| b.load(Ordering::Relaxed)).collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
