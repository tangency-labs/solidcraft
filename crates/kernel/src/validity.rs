//! Is a body one valid solid? The kernel regression rig (`tests/kernel-cases`) asks this of every
//! result: the schema 4 record's structure and geometry (`Brep::validate`, `check_geometry`),
//! and, with the `smlib` feature, SMLib's own check of the same record.

use crate::{Body, brep_record};

/// What the validity checks found for one body. `Err` holds why a check failed.
#[derive(Clone, Debug)]
pub struct Validity {
    /// The record: structure, then geometry against topology within tolerances.
    pub record: Result<(), String>,
    /// SMLib's `check(All)` of the record; `None` in a build without SMLib.
    pub smlib: Option<Result<(), String>>,
}

impl Validity {
    pub fn ok(&self) -> bool {
        self.record.is_ok() && self.smlib.as_ref().is_none_or(Result::is_ok)
    }
}

/// Runs every validity check this build has on `body`.
pub fn validity(body: &Body) -> Validity {
    let record = brep_record(body).map_err(|e| e.to_string());
    let checked = record.as_ref().map_err(Clone::clone).and_then(|r| {
        r.validate().map_err(|e| format!("structure: {e}"))?;
        r.check_geometry().map_err(|e| format!("geometry: {e}"))
    });
    Validity { record: checked, smlib: smlib_validity(body) }
}

#[cfg(feature = "smlib")]
fn smlib_validity(body: &Body) -> Option<Result<(), String>> {
    let checked = crate::smlib_body(body).map_err(|e| e.to_string()).and_then(|b| {
        let r = b.check(solidcraft_smlib::CheckLevel::All).map_err(|e| format!("SMLib check: {e}"))?;
        if r.is_valid() { Ok(()) } else { Err(format!("SMLib check: {r}")) }
    });
    Some(checked)
}

#[cfg(not(feature = "smlib"))]
fn smlib_validity(_: &Body) -> Option<Result<(), String>> {
    None
}

/// Whether this build links SMLib (the `smlib` feature).
pub fn smlib_built() -> bool {
    cfg!(feature = "smlib")
}
