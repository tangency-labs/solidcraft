//! The kernel's runtime ([`Runtime`]): stopping the work it runs, and the stable code of the last
//! call it failed. Neither makes or reads a body; both are what the application's own work (a
//! sketch solve, a report of a failure) needs of the kernel it runs on.

use crate::Backend;

/// A kernel's runtime ([`crate::OpKind::Runtime`]).
pub trait Runtime: Backend {
    /// A shared, cooperative stop: clones share it, cancelling any clone stops every one, and
    /// work polls it at points fixed by the work itself, never by a clock.
    type CancelToken: Clone + std::fmt::Debug + Default + Send + Sync;

    /// A token that is not cancelled.
    fn new_token(&self) -> Self::CancelToken;
    /// A token that counts its polls and is cancelled from its `n`-th on (0 or 1: from the
    /// first), for work stopped at a point fixed by its own work. Clones share the count.
    fn token_after_polls(&self, n: u64) -> Self::CancelToken;
    /// Cancels `token` and every clone of it.
    fn cancel(&self, token: &Self::CancelToken);
    /// Whether `token` is cancelled. A token that counts its polls counts this one.
    fn is_cancelled(&self, token: &Self::CancelToken) -> bool;
    /// A token cancelled when `token` or `other` is. Cancelling it cancels `token`, not `other`.
    fn either(&self, token: &Self::CancelToken, other: &Self::CancelToken) -> Self::CancelToken;
    /// The token over the kernel's work on this thread, for work outside the kernel that stops
    /// with it. None when no work on this thread can be stopped.
    fn stop_token(&self) -> Option<Self::CancelToken>;
    /// The code of the last call of this kernel that failed on this thread, taken: the next ask
    /// gives None until another call fails, and a call that succeeds clears it.
    fn take_failure_code(&self) -> Option<FailureCode>;
}

/// A failed kernel call's stable code: the call, the kernel's number for the kind of failure,
/// the kernel routine and the kernel's class of the error. Every part is a static string or a
/// number, nothing from the model, so two failures of one kind carry equal codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FailureCode {
    /// The call that failed (`"fillet"`, `"frame_member"`).
    pub call: &'static str,
    /// The kernel's stable number of the kind of failure; [`FailureCode::OPS`] for a failure the
    /// kernel's operations decided themselves, with no kernel error behind it.
    pub kind: u16,
    /// The kernel routine that failed, a module path; for [`FailureCode::OPS`], the name of the
    /// operations' error.
    pub operation: &'static str,
    /// The kernel's class of the error, as the kernel gives it.
    pub class: ErrorClass,
}

impl FailureCode {
    /// The kind of a failure the kernel's operations decided themselves.
    pub const OPS: u16 = 0;
}

/// What a kernel's error says about its way out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorClass {
    /// A fact of the geometry: the way out is a different input.
    Refusal,
    /// The kernel's fault: report it.
    Defect,
    /// A part of the kernel not there yet, or a preparation the caller skipped.
    PortFailure,
    /// None of the classes, claiming nothing.
    Unclassed,
}

impl ErrorClass {
    /// The class's stable name: `refusal`, `defect`, `port_failure` or `unclassed`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Refusal => "refusal",
            Self::Defect => "defect",
            Self::PortFailure => "port_failure",
            Self::Unclassed => "unclassed",
        }
    }
}

impl std::fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
