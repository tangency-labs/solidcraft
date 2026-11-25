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

/// `typed_value` when `GOLDENS_TYPED=1`, else `legacy`. With `GOLDENS_CALL_LOG` set, both
/// are computed and a write whose two values differ is logged against the helper's caller
/// ([`crate::call_log`]).
#[track_caller]
fn pick<T: std::fmt::Debug>(helper: &str, legacy: T, typed_value: impl FnOnce() -> T) -> T {
    if crate::call_log::active() {
        let t = typed_value();
        crate::call_log::note(helper, std::panic::Location::caller(), &legacy, &t);
        return if typed() { t } else { legacy };
    }
    if typed() {
        typed_value()
    } else {
        legacy
    }
}

/// A numeric slot type.
pub trait SlotValue: Copy + std::fmt::Debug {
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
#[track_caller]
pub fn slot<T, V: SlotValue>(r: &solidcore::Result<T>, legacy: V) -> V {
    match r {
        Ok(_) => ok_slot(legacy),
        Err(e) => kind_slot(e, legacy),
    }
}

/// A failed call's slot value: the error's kind code.
#[track_caller]
pub fn kind_slot<V: SlotValue>(e: &SolidError, legacy: V) -> V {
    pick("kind_slot", legacy, || V::from_i32(code(e)))
}

/// Whether a match arm takes `e` as a kernel outcome: every error but an unported callee
/// (which defers the case) when `GOLDENS_TYPED=1`, else the arm's own `legacy` guard (a
/// test on the error's status number).
#[track_caller]
pub fn is_kernel(e: &SolidError, legacy: bool) -> bool {
    pick("is_kernel", legacy, || {
        !matches!(e, SolidError::Unported { .. })
    })
}

/// A successful call's slot value: `0`.
#[track_caller]
pub fn ok_slot<V: SlotValue>(legacy: V) -> V {
    pick("ok_slot", legacy, || V::from_i32(0))
}

/// The slot value of a call the runner did not make.
#[track_caller]
pub fn state_slot<V: SlotValue>(s: RunnerState, legacy: V) -> V {
    pick("state_slot", legacy, || V::from_i32(s.value()))
}

/// A record whose shape changes with kinds (a slot pair that becomes one kind, a text that
/// was read back out of a slot): `typed` when `GOLDENS_TYPED=1`, else `legacy`. Goes with the
/// `legacy` arguments.
#[track_caller]
pub fn legacy_or<T: std::fmt::Debug>(legacy: T, typed_value: impl FnOnce() -> T) -> T {
    pick("legacy_or", legacy, typed_value)
}

/// The `error` text of a kernel failure: `kind_<code>`.
#[track_caller]
pub fn kernel_text(e: &SolidError, legacy: impl Into<String>) -> String {
    pick("kernel_text", legacy.into(), || format!("kind_{}", code(e)))
}

/// A drop record's `fail_status` (`DropCurveFail::fail_status`, an upstream status number
/// until the kernel types it as an outcome in the re-bless): `0` for a drop that succeeded
/// (`SM_SUCCESS`), [`RunnerState::NotReached`] for a record no drop has written
/// (`SM_ERR_UNKNOWN`, the kernel's "never used"), else the status's kind code.
#[track_caller]
pub fn drop_status_slot<V: SlotValue>(status: i32, legacy: V) -> V {
    pick("drop_status_slot", legacy, || {
        V::from_i32(match status {
            solidcore::error::SM_SUCCESS => 0,
            solidcore::error::SM_ERR_UNKNOWN => RunnerState::NotReached.value(),
            s => code(&SolidError::Upstream {
                operation: "drop_curve_fail",
                code: s,
                entity: None,
            }),
        })
    })
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

    /// `0` is success alone: no runner state and no kind has the value `0`, so a reader's
    /// `== 0.0` test means the call succeeded. `ErrorKind` is `#[non_exhaustive]` with no list
    /// of its kinds, so its declaration is read: every kind has an explicit discriminant, and
    /// none is `0`.
    #[test]
    fn zero_is_reserved_for_success() {
        for s in [
            RunnerState::NotReached,
            RunnerState::Skipped,
            RunnerState::NoSuchEntity,
        ] {
            assert_ne!(s.value(), 0, "{s:?}");
        }
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/src/error.rs");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let body: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.starts_with("pub enum ErrorKind {"))
            .skip(1)
            .take_while(|l| !l.starts_with('}'))
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//"))
            .collect();
        assert!(
            body.len() > 10,
            "no ErrorKind declaration in {}",
            path.display()
        );
        for line in body {
            let code = line
                .strip_suffix(',')
                .and_then(|l| l.split_once(" = "))
                .and_then(|(_, n)| n.parse::<u16>().ok());
            assert!(
                code.is_some_and(|c| c > 0),
                "ErrorKind kind without a positive code: {line}"
            );
        }
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
