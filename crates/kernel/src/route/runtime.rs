//! The router's runtime ([`solidcraft_kernel_api::runtime::Runtime`]), as
//! `solidcraft_kernel::runtime`: the cancel token the kernel's work polls, for work outside the
//! kernel that stops with it (a sketch solve), and the code of the kernel's last failed call.

use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::runtime::Runtime;

pub use solidcraft_kernel_api::runtime::{ErrorClass, FailureCode};

use super::{Which, which};
use crate::backend::Solidcore;

/// The kernel whose runtime this is.
fn kernel() -> Solidcore {
    match which(OpKind::Runtime) {
        Which::Solidcore => Solidcore,
    }
}

/// A shared, cooperative stop: clones share it, cancelling any clone stops every one, and work
/// polls it at points fixed by the work itself, never by a clock.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(<Solidcore as Runtime>::CancelToken);

impl CancelToken {
    /// A token that is not cancelled.
    #[must_use]
    pub fn new() -> Self {
        CancelToken(kernel().new_token())
    }

    /// A token that counts its polls and is cancelled from its `n`-th on (0 or 1: from the
    /// first). Clones share the count.
    #[must_use]
    pub fn after_polls(n: u64) -> Self {
        CancelToken(kernel().token_after_polls(n))
    }

    /// Cancels this token and every clone of it.
    pub fn cancel(&self) {
        kernel().cancel(&self.0);
    }

    /// Whether the token is cancelled. A token that counts its polls counts this one.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        kernel().is_cancelled(&self.0)
    }

    /// A token cancelled when this one or `other` is. Cancelling it cancels this one, not `other`.
    #[must_use]
    pub fn either(&self, other: &CancelToken) -> CancelToken {
        CancelToken(kernel().either(&self.0, &other.0))
    }
}

/// The cancel token over the kernel's work on this thread (`crate::Stop::within`), for work
/// outside the kernel that stops with it (a sketch solve). None outside such work.
#[must_use]
pub fn stop_token() -> Option<CancelToken> {
    kernel().stop_token().map(CancelToken)
}

/// The code of the last kernel call that failed on this thread, taken: the next ask gives None
/// until another call fails, and a call that succeeds clears it.
#[must_use]
pub fn take_failure_code() -> Option<FailureCode> {
    kernel().take_failure_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cancelling `either` stops it and the token it was made from, never the other one; a
    /// counting token turns cancelled at its `n`-th poll.
    #[test]
    fn tokens_cancel_as_the_kernels_do() {
        let (a, b) = (CancelToken::new(), CancelToken::new());
        let both = a.either(&b);
        assert!(!both.is_cancelled());
        b.cancel();
        assert!(both.is_cancelled() && !a.is_cancelled());
        let c = CancelToken::new();
        let joined = c.either(&a);
        joined.cancel();
        assert!(c.is_cancelled() && !a.is_cancelled());
        let counted = CancelToken::after_polls(3);
        assert_eq!([counted.is_cancelled(), counted.is_cancelled(), counted.is_cancelled()], [false, false, true]);
        assert!(stop_token().is_none(), "no work runs on this thread");
    }
}
