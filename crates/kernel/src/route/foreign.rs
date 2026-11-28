//! A second kernel compiled in beside solidcore, for measurement: which operations go to it, and
//! the call that takes a body there and back as the record (`docs/design/kernel-layer.md` §3, §8).
//!
//! Only builds with the `backend-occt` feature have this module, and no build we ship turns it
//! on (`docs/kernel-support.md`). Without it the router is solidcore alone, and every call is
//! the call made before the router.
//!
//! With it, an operation goes to the other kernel when that kernel takes it
//! ([`solidcraft_kernel_api::Backend::supports`]). One it doesn't take runs on solidcore as it
//! would without the feature, and so does one it answers with [`BackendError::Unsupported`],
//! with [`BackendError::Exchange`] (a body it can't take, or a result it can't give back as a
//! record), or with any other failure but a stop: each is logged with its reason and counted
//! ([`fell_back`]), so a measurement sees it, and the user gets solidcore's result or words.
//!
//! `SOLIDCRAFT_KERNEL=solidcore` keeps every operation on solidcore in such a build, and
//! [`set_kernel`] changes the choice while the program runs, so one build can compare both.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use solidcraft_kernel_api::{Backend, BackendError, OpKind, Support};

use crate::body::Body;
use crate::{KernelError, Result};

/// Which kernel a build with another kernel compiled in sends the operations that kernel takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kernel {
    /// Every operation on solidcore, as in a build without the feature.
    Solidcore,
    /// The operations Open CASCADE takes on Open CASCADE, the rest on solidcore.
    Occt,
}

impl Kernel {
    /// The kernel's name, as `SOLIDCRAFT_KERNEL` spells it.
    pub const fn name(self) -> &'static str {
        match self {
            Kernel::Solidcore => "solidcore",
            Kernel::Occt => "occt",
        }
    }
}

const UNREAD: u8 = 0;
const SOLIDCORE: u8 = 1;
const OCCT: u8 = 2;

static CHOICE: AtomicU8 = AtomicU8::new(UNREAD);

static FELL_BACK: AtomicUsize = AtomicUsize::new(0);

/// How many operations so far went to solidcore although Open CASCADE was chosen (it doesn't
/// take the operation or an operand, or the body didn't cross): what a measurement counts as
/// unsupported.
pub fn fell_back() -> usize {
    FELL_BACK.load(Ordering::Relaxed)
}

/// The Open CASCADE build compiled in, as its adapter reports it.
pub fn occt_version() -> &'static str {
    crate::backend::Occt.version()
}

/// The kernel chosen: [`set_kernel`]'s, else `SOLIDCRAFT_KERNEL`'s, else Open CASCADE (the build
/// asked for it). An unknown name is logged and ignored.
pub fn kernel() -> Kernel {
    match CHOICE.load(Ordering::Relaxed) {
        SOLIDCORE => Kernel::Solidcore,
        OCCT => Kernel::Occt,
        _ => {
            let chosen = match std::env::var("SOLIDCRAFT_KERNEL").ok().as_deref().map(str::trim) {
                Some("solidcore") => Kernel::Solidcore,
                None | Some("" | "occt") => Kernel::Occt,
                Some(other) => {
                    log::warn!("SOLIDCRAFT_KERNEL={other}: no such kernel in this build (solidcore, occt); using occt");
                    Kernel::Occt
                }
            };
            // A racing first read chooses the same, unless set_kernel came first: keep that.
            let _ = CHOICE.compare_exchange(UNREAD, code(chosen), Ordering::Relaxed, Ordering::Relaxed);
            kernel()
        }
    }
}

/// Chooses the kernel from now on, for every thread.
pub fn set_kernel(k: Kernel) {
    CHOICE.store(code(k), Ordering::Relaxed);
}

/// Held by every test that chooses a kernel or counts [`fell_back`]: both are the process's, so
/// two such tests running at once would change each other's choice and count.
#[cfg(test)]
pub(crate) fn choosing() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

const fn code(k: Kernel) -> u8 {
    match k {
        Kernel::Solidcore => SOLIDCORE,
        Kernel::Occt => OCCT,
    }
}

/// `run` on `backend`, with `operands` taken there as their records and the result brought back
/// as a body: `None` when the operation stays on solidcore (another kernel chosen, an operation
/// or an operand the backend doesn't take), else the backend's answer. `Ok(None)` is an empty
/// result, as for a cut that removes everything.
pub(crate) fn run<B: Backend>(
    backend: &B,
    op: OpKind,
    operands: &[&Body],
    run: impl FnOnce(&B, &[B::Form]) -> std::result::Result<Option<B::Form>, B::Error>,
) -> Option<Result<Option<Body>>> {
    if kernel() != Kernel::Occt {
        return None;
    }
    if backend.supports(op) == Support::No || operands.iter().any(|b| b.is_mesh()) {
        FELL_BACK.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    let what = op.name();
    let answer = crate::guard(what, || {
        let forms = match import_all(backend, operands)? {
            Ok(forms) => forms,
            Err(e) => return Ok(Err(e)),
        };
        let made = match run(backend, &forms) {
            Ok(Some(form)) => form,
            Ok(None) => return Ok(Ok(None)),
            Err(e) => return Ok(Err(e.into())),
        };
        Ok(export_one(backend, &made, Some(operands))?.map(Some))
    });
    match answer {
        Ok(Ok(body)) => Some(Ok(body)),
        Ok(Err(BackendError::Unsupported(why))) => {
            log::debug!("{what}: {} doesn't take it ({why}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
        Ok(Err(BackendError::Exchange(why))) => {
            log::warn!("{what}: {} couldn't exchange the body ({why}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
        Ok(Err(BackendError::Cancelled(why))) => Some(Err(KernelError::from(BackendError::Cancelled(why)))),
        Ok(Err(e)) => {
            log::warn!("{what}: {} failed ({e}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
        Err(e) => {
            log::warn!("{what}: {} failed ({e}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

/// `run` on `backend` for an operation that takes no body and makes one body per profile region
/// (an extrusion, a revolution), each brought back as a body and handed with the rest to
/// `finish`, which names them as solidcore names its own: `None` when the operation stays on
/// solidcore (another kernel chosen, an operation the backend doesn't take, or `finish` can't
/// name what came back, which is logged with the reason), else the backend's answer.
pub(crate) fn run_each<B: Backend>(
    backend: &B,
    op: OpKind,
    run: impl FnOnce(&B) -> std::result::Result<Vec<B::Form>, B::Error>,
    finish: impl FnOnce(Vec<Body>) -> Result<Vec<Body>>,
) -> Option<Result<Vec<Body>>> {
    run_parts(
        backend,
        op,
        &[],
        |k, _| run(k),
        |bodies| finish(bodies).map_err(|e| BackendError::Exchange(format!("its faces don't name as solidcore's do ({e})"))),
    )
}

/// `run` on `backend` with `operands` taken there as their records, for an operation that makes
/// several bodies (a split's pieces), each brought back as a body and handed with the rest to
/// `finish`, which puts them in solidcore's order or says why it can't: `None` when the
/// operation stays on solidcore (another kernel chosen, an operation or an operand the backend
/// doesn't take, or `finish`'s refusal, each logged with the reason), else the backend's answer.
pub(crate) fn run_parts<B: Backend>(
    backend: &B,
    op: OpKind,
    operands: &[&Body],
    run: impl FnOnce(&B, &[B::Form]) -> std::result::Result<Vec<B::Form>, B::Error>,
    finish: impl FnOnce(Vec<Body>) -> std::result::Result<Vec<Body>, BackendError>,
) -> Option<Result<Vec<Body>>> {
    if kernel() != Kernel::Occt {
        return None;
    }
    if backend.supports(op) == Support::No || operands.iter().any(|b| b.is_mesh()) {
        FELL_BACK.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    let what = op.name();
    let answer = crate::guard(what, || {
        let forms = match import_all(backend, operands)? {
            Ok(forms) => forms,
            Err(e) => return Ok(Err(e)),
        };
        let made = match run(backend, &forms) {
            Ok(made) => made,
            Err(e) => return Ok(Err(e.into())),
        };
        let mut bodies = Vec::with_capacity(made.len());
        for form in &made {
            // Each piece's order is `finish`'s to settle, so it is brought back without history.
            match export_one(backend, form, None)? {
                Ok(body) => bodies.push(body),
                Err(e) => return Ok(Err(e)),
            }
        }
        Ok(finish(bodies))
    });
    match answer {
        Ok(Ok(bodies)) => Some(Ok(bodies)),
        Ok(Err(BackendError::Unsupported(why))) => {
            log::info!("{what}: {} doesn't take it ({why}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
        Ok(Err(BackendError::Exchange(why))) => {
            log::warn!("{what}: {} couldn't give its result back ({why}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
        Ok(Err(BackendError::Cancelled(why))) => Some(Err(KernelError::from(BackendError::Cancelled(why)))),
        Ok(Err(e)) => {
            log::warn!("{what}: {} failed ({e}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
        Err(e) => {
            log::warn!("{what}: {} failed ({e}); solidcore runs it", B::ID);
            FELL_BACK.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

/// `operands` as `backend`'s forms: handed over as exact bodies to a kernel that takes them
/// ([`Backend::takes_exact`]), else, and for a record no exact body holds (a face used on both
/// sides, wire edges), as their records.
fn import_all<B: Backend>(backend: &B, operands: &[&Body]) -> Result<std::result::Result<Vec<B::Form>, BackendError>> {
    let mut forms = Vec::with_capacity(operands.len());
    for b in operands {
        let exact = if backend.takes_exact() { crate::exact::exact_body(b).ok() } else { None };
        let form = match exact.and_then(|e| backend.import_exact(&e)) {
            Some(form) => form,
            None => backend.import(b.record()),
        };
        match form {
            Ok(form) => forms.push(form),
            Err(e) => return Ok(Err(e.into())),
        }
    }
    Ok(Ok(forms))
}

/// `form` brought back from `backend` as a body made from `operands`, with its history where the
/// backend gives one (none where `operands` is `None`): assembled from an exact body where the
/// backend gives one ([`Backend::export_exact`]), else read from its record.
fn export_one<B: Backend>(backend: &B, form: &B::Form, operands: Option<&[&Body]>) -> Result<std::result::Result<Body, BackendError>> {
    if let Some(exact) = backend.export_exact(form) {
        return match exact {
            Ok(mut exact) => {
                if operands.is_none() {
                    exact.history = None;
                }
                crate::exact::body_from_exact(exact, operands.unwrap_or_default()).map(Ok)
            }
            Err(e) => Ok(Err(e.into())),
        };
    }
    let out = match backend.export(form) {
        Ok(out) => out,
        Err(e) => return Ok(Err(e.into())),
    };
    let body = Body::from_record(out.record)?;
    Ok(Ok(match (out.history, operands) {
        (Some(h), Some(operands)) => body.with_history(h, operands)?,
        _ => body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_can_change_while_the_program_runs() {
        let _chosen = choosing();
        set_kernel(Kernel::Solidcore);
        assert_eq!(kernel(), Kernel::Solidcore);
        set_kernel(Kernel::Occt);
        assert_eq!(kernel(), Kernel::Occt);
    }

    #[test]
    fn kernels_are_named_as_the_variable_spells_them() {
        assert_eq!(Kernel::Solidcore.name(), "solidcore");
        assert_eq!(Kernel::Occt.name(), "occt");
    }
}
