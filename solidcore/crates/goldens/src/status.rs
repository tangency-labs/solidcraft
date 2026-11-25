//! What a runner records for a call's outcome: the only way to write a numeric status slot
//! or an `error` text.
//!
//! A status slot holds one of three things:
//! * `0`: the call succeeded. `0` is not a kind.
//! * a kernel error's kind code ([`solidcore::ErrorKind::code`], `1..`), through [`slot`] or
//!   [`kind_slot`];
//! * a [`RunnerState`] (always negative), through [`state_slot`]: the runner did not get a
//!   kernel outcome for this call.
//!
//! An `error` text is `kind_<code>` for a kernel error ([`crate::Output::kernel`]), or a fixed
//! harness label ([`crate::Output::harness`]). Values that are data (ids, coordinates,
//! presets) never go through this module.
//!
//! Every writer still takes the value the runner recorded before kinds (`legacy`), and
//! returns it unless `GOLDENS_TYPED=1` is set, so the goldens stay as they were until the
//! re-bless switches them over; the argument then goes.

use std::sync::OnceLock;

use solidcore::SolidError;

/// Environment switch: `1` records kinds and runner states, anything else the legacy value.
pub const TYPED_ENV: &str = "GOLDENS_TYPED";

/// True when the run records kinds and runner states (`GOLDENS_TYPED=1`).
pub fn typed() -> bool {
    static TYPED: OnceLock<bool> = OnceLock::new();
    *TYPED.get_or_init(|| std::env::var_os(TYPED_ENV).is_some_and(|v| v == "1"))
}

/// A slot value that is not a kernel outcome. The values are pinned and negative: kind codes
/// are positive and success is `0`, so the three never meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RunnerState {
    /// An earlier call in the same record failed or was not made, so this one never ran.
    NotReached = -1,
    /// The runner did not make the call: upstream's behaviour is undefined there (a null
    /// dereference, undefined behaviour, a loop that never ends, the shim's input contract).
    Skipped = -2,
    /// The record names an entity the B-rep does not have, so there was nothing to call.
    NoSuchEntity = -3,
}

impl RunnerState {
    /// The pinned slot value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self as i32
    }
}

/// A numeric slot type.
pub trait SlotValue: Copy {
    /// The slot value of `v`.
    fn from_i32(v: i32) -> Self;
}

impl SlotValue for f64 {
    fn from_i32(v: i32) -> Self {
        f64::from(v)
    }
}

impl SlotValue for i64 {
    fn from_i32(v: i32) -> Self {
        i64::from(v)
    }
}

impl SlotValue for i32 {
    fn from_i32(v: i32) -> Self {
        v
    }
}

/// The kind code of `e`, as a slot value (never `0`).
fn code(e: &SolidError) -> i32 {
    i32::from(e.kind().code())
}

/// One call's outcome: `0` for success, else the error's kind code.
pub fn slot<T, V: SlotValue>(r: &solidcore::Result<T>, legacy: V) -> V {
    match r {
        Ok(_) => ok_slot(legacy),
        Err(e) => kind_slot(e, legacy),
    }
}

/// A failed call's slot value: the error's kind code.
pub fn kind_slot<V: SlotValue>(e: &SolidError, legacy: V) -> V {
    if typed() {
        V::from_i32(code(e))
    } else {
        legacy
    }
}

/// Whether a match arm takes `e` as a kernel outcome: every error but an unported callee
/// (which defers the case) when `GOLDENS_TYPED=1`, else the arm's own `legacy` guard (a
/// test on the error's status number).
pub fn is_kernel(e: &SolidError, legacy: bool) -> bool {
    if typed() {
        !matches!(e, SolidError::Unported { .. })
    } else {
        legacy
    }
}

/// A successful call's slot value: `0`.
pub fn ok_slot<V: SlotValue>(legacy: V) -> V {
    if typed() {
        V::from_i32(0)
    } else {
        legacy
    }
}

/// The slot value of a call the runner did not make.
pub fn state_slot<V: SlotValue>(s: RunnerState, legacy: V) -> V {
    if typed() {
        V::from_i32(s.value())
    } else {
        legacy
    }
}

/// A record whose shape changes with kinds (a slot pair that becomes one kind, a text that
/// was read back out of a slot): `typed` when `GOLDENS_TYPED=1`, else `legacy`. Goes with the
/// `legacy` arguments.
pub fn legacy_or<T>(legacy: T, typed_value: impl FnOnce() -> T) -> T {
    if typed() {
        typed_value()
    } else {
        legacy
    }
}

/// The `error` text of a kernel failure: `kind_<code>`.
pub fn kernel_text(e: &SolidError, legacy: impl Into<String>) -> String {
    if typed() {
        format!("kind_{}", code(e))
    } else {
        legacy.into()
    }
}

/// The largest size that works, for a refused extension that names one (recorded as
/// `values["largest_size"]` beside the kind).
pub(crate) fn largest_size(e: &SolidError) -> Option<f64> {
    match e {
        SolidError::ExtendRefused { distance, .. } => *distance,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RunnerState;

    #[test]
    fn runner_states_are_pinned_and_negative() {
        assert_eq!(RunnerState::NotReached.value(), -1);
        assert_eq!(RunnerState::Skipped.value(), -2);
        assert_eq!(RunnerState::NoSuchEntity.value(), -3);
    }

    #[test]
    fn kind_codes_are_positive() {
        let e = solidcore::SolidError::InvalidInput {
            operation: "test",
            reason: "test",
        };
        assert!(e.kind().code() > 0);
    }

    /// Every unit says whether an error text is the kernel's or the harness's
    /// (`Output::kernel` / `Output::harness` / `Output::harness_text`), never the untyped
    /// `Output::err`. (The status-slot reads join this ban once every unit records kinds.)
    #[test]
    fn units_do_not_build_untyped_errors() {
        fn walk(dir: &std::path::Path, hits: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, hits);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    for (n, line) in text.lines().enumerate() {
                        if line.contains("Output::err(") {
                            hits.push(format!("{}:{}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
        let units = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/units");
        assert!(units.is_dir(), "{} is missing", units.display());
        let mut hits = Vec::new();
        walk(&units, &mut hits);
        assert!(hits.is_empty(), "untyped Output::err in units: {hits:?}");
    }
}
