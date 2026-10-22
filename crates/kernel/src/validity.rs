//! Is a body one valid solid? The kernel regression rig (`tests/kernel-cases`) asks this of every
//! result: the schema 4 record's structure and geometry (`Brep::validate`, `check_geometry`),
//! and SMLib's own check of the same record.

use crate::{Body, brep_record};

/// What the validity checks found for one body. `Err` holds why a check failed.
#[derive(Clone, Debug)]
pub struct Validity {
    /// The record: structure, then geometry against topology within tolerances.
    pub record: Result<(), String>,
    /// SMLib's `check(All)` of the record.
    pub smlib: Option<SmlibCheck>,
}

/// SMLib's verdict on a body.
#[derive(Clone, Debug)]
pub enum SmlibCheck {
    Valid,
    Invalid(String),
    /// The record can't be built in SMLib yet (a geometry kind the conversion doesn't take): no
    /// verdict, and no fault of the body's.
    NotSupported(String),
}

impl Validity {
    pub fn ok(&self) -> bool {
        self.record.is_ok() && !matches!(self.smlib, Some(SmlibCheck::Invalid(_)))
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

fn smlib_validity(body: &Body) -> Option<SmlibCheck> {
    let b = match crate::smlib_body(body) {
        Ok(b) => b,
        Err(e) => {
            let e = e.to_string();
            return Some(if e.contains("not supported yet") { SmlibCheck::NotSupported(e) } else { SmlibCheck::Invalid(e) });
        }
    };
    Some(match b.check(solidcraft_smlib::CheckLevel::All) {
        Ok(r) if r.is_valid() => SmlibCheck::Valid,
        Ok(r) => SmlibCheck::Invalid(format!("SMLib check: {r}")),
        Err(e) => SmlibCheck::Invalid(format!("SMLib check: {e}")),
    })
}

/// Whether this build links SMLib (the `smlib` feature).
pub fn smlib_built() -> bool {
    cfg!(feature = "smlib")
}

/// Where every SMLib call that fails from now on leaves its inputs for replay (the C API's
/// `smlib_set_dump_dir`: the call's arguments and B-reps, which `smlib_brep_read` reads back);
/// `None` stops it. A build without SMLib makes no calls to dump.
pub fn smlib_dump_dir(dir: Option<&std::path::Path>) -> crate::Result<()> {
    #[cfg(feature = "smlib")]
    {
        solidcraft_smlib::set_dump_dir(dir).map_err(|e| crate::KernelError::Failed(format!("SMLib dump folder: {e}")))
    }
    #[cfg(not(feature = "smlib"))]
    {
        let _ = dir;
        Ok(())
    }
}
