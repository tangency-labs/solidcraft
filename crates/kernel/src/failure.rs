//! A failed kernel call's stable code, kept with the error it became: a feature's result, a
//! command's error, a report. The code ([`ErrorCode`]) names the call, the kernel's numbered kind
//! of failure and the kernel routine, nothing from the model, so two reports of one failure
//! carry equal codes whatever the design, its numbers or its names (RFC 0016).
//!
//! The kernel leaves the code of the last call that failed on each thread
//! ([`solidcraft_smlib::failure`]). The application takes it for work that failed in the kernel
//! ([`clear`] before the work, [`last`] after it failed); [`code_in`] gives it only for an error
//! whose text carries that call's error word for word. A failure on another thread (a pool thread
//! of a feature that works on its bodies side by side) leaves its code there: the work's failure
//! then has none.

use serde::{Deserialize, Serialize};

/// A failed kernel call's code as data to keep and send: the call (`"blend"`), the kernel's
/// stable number for the kind of failure (`kind`, 0 for a failure solidcore-ops decided itself),
/// the kernel routine that failed (`operation`, a module path; for kind 0, the name of
/// solidcore-ops' error) and the kernel's class of the error (`class`: `refusal`, `defect`,
/// `port_failure` or `unclassed`), copied from the kernel's code, never derived here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ErrorCode {
    pub call: String,
    pub kind: u16,
    pub operation: String,
    /// A code saved before the class was kept (#22148) has none: it reads as `unclassed`, the
    /// class that claims nothing, so a report or result saved then still opens.
    #[serde(default = "unclassed")]
    pub class: String,
}

/// The class of a code saved without one: `unclassed`.
pub(crate) fn unclassed() -> String {
    "unclassed".to_owned()
}

impl From<solidcraft_smlib::failure::FailureCode> for ErrorCode {
    fn from(c: solidcraft_smlib::failure::FailureCode) -> Self {
        ErrorCode { call: c.call.to_owned(), kind: c.kind, operation: c.operation.to_owned(), class: c.class.name().to_owned() }
    }
}

/// `call · kind · operation`, as a failure signature writes it.
impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · {} · {}", self.call, self.kind, self.operation)
    }
}

/// Forgets the last failed call on this thread: before the work whose failure [`code_in`] is
/// asked about.
pub fn clear() {
    solidcraft_smlib::failure::clear();
}

/// The code of the last kernel call that failed on this thread since [`clear`]: for work that
/// failed in the kernel, the call it failed with.
#[must_use]
pub fn last() -> Option<ErrorCode> {
    solidcraft_smlib::failure::last().map(|(code, _)| ErrorCode::from(code))
}

/// The code of the last kernel call that failed on this thread, when `error` (an error's text,
/// as the application words it) carries that call's error: the failure `error` was made from.
#[must_use]
pub fn code_in(error: &str) -> Option<ErrorCode> {
    solidcraft_smlib::failure::code_in(error).map(ErrorCode::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ErrorClass, FailureCode};

    /// #22148: the code keeps the kernel's class by its name, for every class, without deriving
    /// it from the kind (a refusal of kind 6 stays a refusal).
    #[test]
    fn the_code_keeps_the_kernels_class_by_name() {
        for (class, name) in [
            (ErrorClass::Refusal, "refusal"),
            (ErrorClass::Defect, "defect"),
            (ErrorClass::PortFailure, "port_failure"),
            (ErrorClass::Unclassed, "unclassed"),
        ] {
            let code = ErrorCode::from(FailureCode { call: "blend", kind: 6, operation: "blend::op", class });
            assert_eq!(code, ErrorCode { call: "blend".into(), kind: 6, operation: "blend::op".into(), class: name.into() });
            assert_eq!(serde_json::to_value(&code).unwrap()["class"], name);
        }
    }

    /// A code saved before #22148 has no `class`: it still reads, as `unclassed`.
    #[test]
    fn a_code_saved_before_the_class_still_reads_as_unclassed() {
        let saved = r#"{"call":"blend","kind":7,"operation":"smlib::blend::rolling"}"#;
        let code: ErrorCode = serde_json::from_str(saved).expect("an old code reads");
        assert_eq!(code, ErrorCode { call: "blend".into(), kind: 7, operation: "smlib::blend::rolling".into(), class: "unclassed".into() });
    }
}
