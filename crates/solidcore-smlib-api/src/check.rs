//! SMLib's own validity check as text.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_check.cpp` (`smlib_brep_check`,
//! `advisory`, `collect`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API (SMLib call) |
//! |------|-------|
//! | [`Smlib::brep_check`] | `smlib_brep_check` (`SmBrep::ValidatePointers`, then `SmBrep::AssertValid` with `SM_WALK`) |
//!
//! Deviations: the report is returned, not copied into a buffer (no `SMLIB_ERR_BUFFER`).
//! solidcore's [`check_brep`] already runs the C API's sequence and formats its lines; this
//! drops SMLib's warning (`SmBrep` test 21, "Suspicious Shell Count") as `advisory` does,
//! and does not count the "`AssertValid` skipped" line as a failure, as `collect` does not.
//! The check runs on a copy that keeps its geometry owners (`AssertValid` may make UV trims).

use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;

use crate::session::Smlib;
use crate::status::{Result, Status};

/// The line `check_brep` adds when the pointer check failed and `AssertValid` was skipped.
const SKIPPED: &str = "AssertValid skipped: ValidatePointers failed";

/// `advisory`: SMLib's warning, reported without failing (`SmBrep` test 21).
fn advisory(line: &str) -> bool {
    line.starts_with("SmBrep::Test #21 ")
}

impl Smlib {
    /// `smlib_brep_check`: SMLib's validity checks on `brep` at test `level` (0 the
    /// fewest, 1 adds the geometry checks, 2 every test): the report, one line per failed
    /// test, each ending in a newline, and the number of failed tests (0: valid). A
    /// failing check is still `Ok`; `level` outside `0..=2` is `SMLIB_ERR_ARG`.
    pub fn brep_check(&mut self, brep: &Brep, level: i32) -> Result<(String, i64)> {
        let level = match level {
            0 => CheckLevel::Level0,
            1 => CheckLevel::Level1,
            2 => CheckLevel::Level2,
            _ => return Err(Status::ERR_ARG),
        };
        self.run(
            || format!("smlib_brep_check(brep, level={})", level.code()),
            &[("brep", brep)],
            |api| {
                let mut b = brep.clone_standing_for_self()?;
                let check = check_brep(api.kernel_mut(), &mut b, level)?;
                let mut report = String::new();
                let mut failures = 0i64;
                for line in check.lines().into_iter().filter(|l| !advisory(l)) {
                    if line != SKIPPED {
                        failures += 1;
                    }
                    report.push_str(&line);
                    report.push('\n');
                }
                Ok((report, failures))
            },
        )
    }
}
