//! solidcore's runtime ([`Runtime`]): its cancel token, the one its work polls, and the failure
//! code solidcore-ops' session leaves for the last call that failed on a thread.

use solidcore_ops::cancel::CancelToken;
use solidcore_ops::{ErrorClass as OpsClass, FailureCode as OpsCode, session};
use solidcraft_kernel_api::runtime::{ErrorClass, FailureCode, Runtime};

use super::Solidcore;

impl Runtime for Solidcore {
    type CancelToken = CancelToken;

    fn new_token(&self) -> CancelToken {
        CancelToken::new()
    }

    fn token_after_polls(&self, n: u64) -> CancelToken {
        CancelToken::after_polls(n)
    }

    fn cancel(&self, token: &CancelToken) {
        token.cancel();
    }

    fn is_cancelled(&self, token: &CancelToken) -> bool {
        token.is_cancelled()
    }

    fn either(&self, token: &CancelToken, other: &CancelToken) -> CancelToken {
        token.either(other)
    }

    fn stop_token(&self) -> Option<CancelToken> {
        solidcraft_smlib::stop::token()
    }

    fn take_failure_code(&self) -> Option<FailureCode> {
        session::take_failure_code().map(failure_code)
    }
}

/// solidcore-ops' code, field for field.
pub(crate) fn failure_code(c: OpsCode) -> FailureCode {
    FailureCode { call: c.call, kind: c.kind, operation: c.operation, class: error_class(c.class) }
}

/// solidcore's class; a class solidcore adds later claims nothing here (`Unclassed`).
fn error_class(c: OpsClass) -> ErrorClass {
    match c {
        OpsClass::Refusal => ErrorClass::Refusal,
        OpsClass::Defect => ErrorClass::Defect,
        OpsClass::PortFailure => ErrorClass::PortFailure,
        _ => ErrorClass::Unclassed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every class solidcore has keeps its name across the interface.
    #[test]
    fn classes_keep_their_names() {
        for c in [OpsClass::Refusal, OpsClass::Defect, OpsClass::PortFailure, OpsClass::Unclassed] {
            assert_eq!(error_class(c).name(), c.name());
        }
    }
}
