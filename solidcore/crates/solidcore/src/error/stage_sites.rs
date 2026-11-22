//! Test-only ratchet on the library sites that build an *unreviewed stage failure*: an
//! area's `StageFailed` (`BlendFailure`, `CurveFailure`, `GeomFailure`, `IntersectFailure`,
//! `BSplineFailure`, `OffsetFailure`, and `BooleanFailure::StageFailed { stage, what }`), the
//! form of a defect built before its site was reviewed. The count only falls: a reviewed site
//! builds the area's `Defect { why }` (the step that failed and why that is the kernel's
//! fault), a named cause, or a refusal, and the PR that converts it lowers its file's count
//! in [`PINNED`] (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
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

/// Every library file that builds an unreviewed stage failure, with its exact number of
/// sites (see the module docs for what counts), sorted by path. Converting a site lowers its
/// file's count here in the same PR, and a file that reaches 0 loses its line. The count
/// never rises: new code builds `Defect { why }`, a named cause or a refusal. Code that moves
/// between files moves its sites: the PR lowers one file's count and raises the other's by
/// the same number.
#[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
const PINNED: &[(&str, usize)] = &[
    ("solidcore/src/blend/blend_setback.rs", 3),
    ("solidcore/src/blend/fillet_corner/p1.rs", 6),
    ("solidcore/src/blend/fillet_corner/p3.rs", 1),
    ("solidcore/src/blend/fillet_exact/rational.rs", 2),
    ("solidcore/src/blend/fillet_executive/session.rs", 5),
    ("solidcore/src/blend/fillet_executive/session/law_corners.rs", 1),
    ("solidcore/src/blend/fillet_executive/session/tangent_rollover.rs", 2),
    ("solidcore/src/blend/fillet_executive/session/trim_at_creases.rs", 4),
    ("solidcore/src/blend/fillet_geom/p1.rs", 62),
    ("solidcore/src/blend/fillet_geom/p2.rs", 123),
    ("solidcore/src/blend/fillet_geom/p3.rs", 47),
    ("solidcore/src/blend/fillet_geom/p4.rs", 120),
    ("solidcore/src/blend/fillet_geom/p5.rs", 131),
    ("solidcore/src/blend/fillet_geom/p6.rs", 88),
    ("solidcore/src/blend/fillet_geom/tangent_end_rib.rs", 22),
    ("solidcore/src/blend/fillet_removal.rs", 20),
    ("solidcore/src/boolean/merge/p2.rs", 1),
    ("solidcore/src/boolean/merge/p4/arena_pair.rs", 1),
    ("solidcore/src/boolean/topology_intersector/p1.rs", 5),
    ("solidcore/src/boolean/topology_intersector/p2.rs", 9),
    ("solidcore/src/boolean/topology_intersector/p3.rs", 1),
    ("solidcore/src/curve/curve/p3.rs", 2),
    ("solidcore/src/curve/curve/p7.rs", 3),
    ("solidcore/src/geom/local_solve_nd.rs", 1),
    ("solidcore/src/intersect/surface_intersector/p1.rs", 5),
    ("solidcore/src/intersect/surface_intersector/p5.rs", 6),
    ("solidcore/src/nurbs/b_spline_curve/p3.rs", 2),
    ("solidcore/src/offset/offset_executive/reach.rs", 1),
    ("solidcore/src/offset/offset_executive/step_gaps.rs", 5),
    ("solidcore/src/offset/offset_geometry_creation.rs", 17),
    ("solidcore/src/topology/proto_topology/p2.rs", 3),
];

#[cfg(test)]
mod tests {
    use super::super::site_scan::crate_sites;
    use super::*;

    /// The number of sites that build an unreviewed stage failure only falls.
    #[test]
    fn the_unreviewed_stage_failure_sites_only_shrink() {
        super::super::site_scan::assert_pinned(
            "unreviewed stage-failure sites",
            &crate_sites(&STAGE),
            PINNED,
        );
    }
}
