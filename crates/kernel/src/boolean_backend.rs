//! Which kernel runs [`crate::boolean`]: truck (the default) or SMLib.
//!
//! The setting is the environment variable `SOLIDCRAFT_BOOLEANS`, read once per process:
//! `truck` (or unset) or `smlib`. SMLib needs a build with the kernel's `smlib` feature; set
//! without it, or set to anything else, every boolean fails with an error that says so rather
//! than quietly running truck, so a comparison run can never compare truck with itself.
//!
//! Through SMLib a boolean is: each operand's schema 4 record built in SMLib and tagged as
//! operand 0 or 1, `SmBrep` boolean, the result exported to schema 4 and decoded as a kernel
//! body. The result's history is SMLib's own (its tags carried through the operation),
//! renumbered from SMLib's face and edge order into the operands' and the result's ([`History`]'s
//! contract). A result SMLib's own check or the record's geometry check refuses is an error,
//! never a body.

use crate::{KernelError, Result};

/// The environment variable that picks the boolean kernel.
pub const BOOLEANS_ENV: &str = "SOLIDCRAFT_BOOLEANS";

/// A kernel that runs booleans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanBackend {
    Truck,
    Smlib,
}

impl BooleanBackend {
    /// The backend a setting names; `None` (unset) is truck.
    pub fn parse(setting: Option<&str>) -> std::result::Result<BooleanBackend, String> {
        match setting.map(str::trim) {
            None | Some("") | Some("truck") => Ok(BooleanBackend::Truck),
            Some("smlib") if cfg!(feature = "smlib") => Ok(BooleanBackend::Smlib),
            Some("smlib") => Err(format!("{BOOLEANS_ENV}=smlib: not supported yet: this build has no SMLib (feature `smlib`)")),
            Some(other) => Err(format!("{BOOLEANS_ENV}={other}: expected `truck` or `smlib`")),
        }
    }
}

/// The boolean kernel this process uses ([`BOOLEANS_ENV`], read on first use).
pub fn boolean_backend() -> Result<BooleanBackend> {
    static SETTING: std::sync::OnceLock<std::result::Result<BooleanBackend, String>> = std::sync::OnceLock::new();
    SETTING.get_or_init(|| BooleanBackend::parse(std::env::var(BOOLEANS_ENV).ok().as_deref())).clone().map_err(KernelError::Invalid)
}

#[cfg(feature = "smlib")]
pub(crate) use smlib::boolean as smlib_boolean;

#[cfg(feature = "smlib")]
mod smlib {
    use solidcraft_smlib::{BooleanOp, CheckLevel};

    use crate::body::Body;
    use crate::history::History;
    use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
    use crate::ops::BoolOp;
    use crate::{KernelError, Result, record};

    fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib boolean: {what}: {e}"))
    }

    /// `a` and `b` combined by `op` in SMLib, with the history SMLib's tags give (none where a
    /// body's face and edge order cannot be matched to SMLib's). `None`: the result is empty.
    pub(crate) fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
        a.require_brep("a boolean")?;
        b.require_brep("a boolean")?;
        let (sa, oa) = smlib_body_ordered(a)?;
        let (sb, ob) = smlib_body_ordered(b)?;
        let ta = sa.tagged(0).map_err(|e| failed("tag", e))?;
        let tb = sb.tagged(1).map_err(|e| failed("tag", e))?;
        let kind = match op {
            BoolOp::Union => BooleanOp::Union,
            BoolOp::Cut => BooleanOp::Subtract,
            BoolOp::Intersect => BooleanOp::Intersect,
        };
        let Some(r) = ta.boolean(&tb, kind).map_err(|e| failed("operation", e))? else { return Ok(None) };
        let check = r.check(CheckLevel::All).map_err(|e| failed("check", e))?;
        if !check.is_valid() {
            return Err(failed("SMLib's check refuses the result", check));
        }
        let (body, or) = body_from_smlib_ordered(&r)?;
        let history = match (oa, ob, or) {
            (Some(oa), Some(ob), Some(or)) => r.history().map_err(|e| failed("history", e))?.history(&[oa, ob], &or).map(Some).unwrap_or_else(|e| {
                log::debug!(target: "smlib", "boolean history: {e}");
                None
            }),
            _ => None,
        };
        Ok(Some(with_history(body, history, a, b)))
    }

    /// `body` with `history` where it describes `body` made from `a` and `b`.
    fn with_history(body: Body, history: Option<History>, a: &Body, b: &Body) -> Body {
        body.with_checked_history(history, &record::counts(&[a, b]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_names_a_backend_or_is_refused() {
        assert_eq!(BooleanBackend::parse(None), Ok(BooleanBackend::Truck));
        assert_eq!(BooleanBackend::parse(Some("truck")), Ok(BooleanBackend::Truck));
        assert_eq!(BooleanBackend::parse(Some(" ")), Ok(BooleanBackend::Truck));
        assert_eq!(BooleanBackend::parse(Some("smlib")).is_ok(), cfg!(feature = "smlib"));
        assert!(BooleanBackend::parse(Some("occt")).is_err());
    }
}
