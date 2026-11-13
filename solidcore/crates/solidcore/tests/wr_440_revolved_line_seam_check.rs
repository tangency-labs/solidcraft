//! WR-440 (#530; the fork's b7dd4ed) through a revolve and the whole-body check (#1954):
//! one line revolved a full turn makes one face whose edge uses have no UV trims, so
//! `SmTrimmingTools::CheckLoop` takes their ends from vertex projections. On the closed
//! direction those are known only modulo the period, and upstream reports the seam as a
//! UV gap of one whole turn. With `Kernel::fixes.check_loop_projected_seam_ends` a
//! projected end is compared on the period nearest its neighbour, and the face is valid.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::assert_array::labels;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, Point3, Vec3};

/// #530's three profiles in the xz plane, each revolved a full turn about Z: a cylinder,
/// a cone frustum narrowing upwards and one widening upwards.
const LINES: [([f64; 3], [f64; 3]); 3] = [
    ([2.0, 0.0, 0.0], [2.0, 0.0, 3.0]),
    ([2.0, 0.0, 0.0], [1.0, 0.0, 3.0]),
    ([1.0, 0.0, 0.0], [2.0, 0.0, 3.0]),
];

#[test]
fn wr_440_revolved_line_passes_the_body_check_only_with_the_fix() {
    for (start, end) in LINES {
        for fix in [false, true] {
            let mut k = Kernel::default();
            k.fixes.check_loop_projected_seam_ends = fix;
            let line =
                Curve::from(create_line_segment(Point3::from(start), Point3::from(end)).unwrap());
            let mut b = create_rotational_sweep(
                &mut k,
                &[Some(&line)],
                Point3::ZERO,
                Vec3::Z,
                360.0,
                false,
            )
            .unwrap();
            assert_eq!(b.face_list().len(), 1, "{start:?} {end:?}");

            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            let failures: Vec<_> = check
                .failures()
                .map(|r| (r.reporting_type, r.test_index))
                .collect();
            eprintln!("{start:?} to {end:?}, fix {fix}: {:?}", check.lines());
            assert!(check.pointer_reports.is_empty(), "{:?}", check.lines());
            if fix {
                assert!(check.is_valid(), "{start:?} {end:?}: {:?}", check.lines());
                assert_eq!(check.assert_valid, Some(true));
            } else {
                // upstream: the seam's two uses project to the same side, a gap of one
                // period of the face's natural domain. `AssertValid` still returns true
                // (the face test does not fail on `CheckLoop`'s report), but the check
                // lists the report and is not valid.
                assert!(!check.is_valid());
                assert_eq!(check.assert_valid, Some(true));
                assert_eq!(
                    failures,
                    [(labels::TRIMMING_TOOLS_TYPE, 1)],
                    "{start:?} {end:?}: {:?}",
                    check.lines()
                );
            }
        }
    }
}
