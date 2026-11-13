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

#[cfg(test)]
mod tests {
    use super::*;

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
