//! Test-only ratchet on the library sites that build an *unreviewed stage failure*: an
//! area's `StageFailed` (`BlendFailure`, `CurveFailure`, `GeomFailure`, `IntersectFailure`,
//! `BSplineFailure`, `OffsetFailure`, and `BooleanFailure::StageFailed { stage, what }`), the
//! form of a defect built before its site was reviewed. The count only falls: a reviewed site
//! builds the area's `Defect { why }` (the step that failed and why that is the kernel's
//! fault), a named cause, or a refusal, and the PR that converts it lowers its file's pin
//! (see [`PINS`]; the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! **What counts as one site**, in the library code of `solidcore` and `solidcore-ops`
//! (comments, string contents and every `#[cfg(test)]` item blanked, test files skipped), as
//! [`super::site_scan`] counts it for [`STAGE`]:
//! - a construction of `<Area>Failure::StageFailed` by any path ending in it (a pattern is
//!   not one);
//! - a call of a `SolidError` helper that builds one, `curve_stage_failed(`,
//!   `geom_stage_failed(`, `intersect_stage_failed(`, `offset_stage_failed(` or
//!   `boolean_stage_failed(`, written `SolidError::<name>(` or `Self::<name>(` (their own
//!   definitions are not sites; a local `fn` of the same name is a wrapper, not the helper);
//! - a call of a local wrapper: a `let` closure or a private `fn` whose body is one
//!   expression that builds one of the above, or a `macro_rules!` whose body builds one
//!   (a macro is a wrapper whatever its body holds, `let`s included: each call expands it)
//!   ([`super::site_scan::Wrappers::Thin`]: `let missing = ||
//!   SolidError::offset_stage_failed(OP, SM_ERR_NULL_POINTER)` counts at each call, and at
//!   each use of its name as a value, `x.ok_or_else(missing)`). Its body is not a site.
//!
//! Comments, strings and tests are not sites. A function visible outside its file is not a
//! wrapper (its calls elsewhere would not be seen), so its body is the site, as is the body
//! of a wrapper never called.

use super::site_scan::{Builder, Scan, Wrappers};

/// The unreviewed stage failures: every area's `StageFailed`, and the helpers that build one.
pub(in crate::error) const STAGE: Scan = Scan {
    builders: &[
        Builder::Variant("BlendFailure", "StageFailed"),
        Builder::Variant("BooleanFailure", "StageFailed"),
        Builder::Variant("BSplineFailure", "StageFailed"),
        Builder::Variant("CurveFailure", "StageFailed"),
        Builder::Variant("GeomFailure", "StageFailed"),
        Builder::Variant("IntersectFailure", "StageFailed"),
        Builder::Variant("OffsetFailure", "StageFailed"),
        Builder::Method("boolean_stage_failed", Some(2)),
        Builder::Method("curve_stage_failed", None),
        Builder::Method("geom_stage_failed", None),
        Builder::Method("intersect_stage_failed", None),
        Builder::Method("offset_stage_failed", None),
    ],
    wrappers: Wrappers::Thin,
};

/// The pins of the unreviewed stage-failure sites: every library file that builds one has
/// one file in `src/error/pins/stage/`, `<path>.pin` holding its exact number of sites (see
/// the module docs for what counts; `site_scan::pins` for the layout). Converting a site
/// lowers its file's pin in the same PR, and a file that reaches 0 loses its pin. The count
/// never rises: new code builds `Defect { why }`, a named cause or a refusal. Code that
/// moves between files moves its sites: the PR lowers one pin and raises the other by the
/// same number. One file per pin, so conversions in different files never touch the same
/// line.
const PINS: &str = "stage";

#[cfg(test)]
mod tests {
    use super::super::site_scan::crate_sites;
    use super::*;

    /// The number of sites that build an unreviewed stage failure only falls.
    #[test]
    fn the_unreviewed_stage_failure_sites_only_shrink() {
        super::super::site_scan::assert_pins(
            "unreviewed stage-failure sites",
            &crate_sites(&STAGE),
            PINS,
        );
    }
}
