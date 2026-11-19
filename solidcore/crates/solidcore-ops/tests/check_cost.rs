//! The validity check's work on a plate with round holes (tangency-labs/solidcraft#11574),
//! counted by the kernel's profiler scopes (feature `profile`).
//!
//! Each hole's wall is a cylinder whose face tests 29, 31 and 36 classify the cylinder's seam
//! against the face (`topology::face::classify_seam`, one per wall). The seam is a ruling of the
//! cylinder, so its 3d curve is a line (`Surface::seam_curve`): the closed-form intersector
//! meets it with the wall's two rims and its seam edge, and no intersection of the
//! classification builds curve caches for a subdivision solve
//! (`curve::global_curve_intersect::cached`). The check's own face properties have nobody to
//! read their touch lists on a cylinder, so no classification collects them
//! (`topology::face::seam_loop_crossings`).
#![cfg(feature = "profile")]
#![allow(clippy::unwrap_used)]

use std::f64::consts::TAU;

use solidcore_ops::session::{self, CallProfile};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{Body, CheckLevel};

/// A 100 x 100 x 1 plate with a 3 x 3 grid of holes of radius 5.
fn holed_plate() -> Body {
    let l = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let mut profile = vec![
        l([0., 0., 0.], [100., 0., 0.]),
        l([100., 0., 0.], [100., 100., 0.]),
        l([100., 100., 0.], [0., 100., 0.]),
        l([0., 100., 0.], [0., 0., 0.]),
    ];
    for i in 0..3 {
        for j in 0..3 {
            profile.push(Curve::Arc {
                center: [20.0 + 30.0 * f64::from(i), 20.0 + 30.0 * f64::from(j), 0.0],
                axis: [0.0, 0.0, 1.0],
                reference: [1.0, 0.0, 0.0],
                radius: 5.0,
                start: 0.0,
                end: TAU,
            });
        }
    }
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        1.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap()
}

/// How many times a scope named `name` ran in `calls`: anywhere below a scope named `under`, or
/// anywhere at all with `under` `None`.
fn runs(calls: &[CallProfile], name: &str, under: Option<&str>) -> u64 {
    let mut n = 0;
    for c in calls {
        let scopes = &c.profile.scopes;
        for s in scopes.iter().filter(|s| s.name == name) {
            let inside = match under {
                None => true,
                Some(u) => {
                    let mut p = s.parent;
                    let mut found = false;
                    while let Some(id) = p {
                        let Some(parent) = scopes.iter().find(|x| x.id == id) else {
                            break;
                        };
                        if parent.name == u {
                            found = true;
                            break;
                        }
                        p = parent.parent;
                    }
                    found
                }
            };
            if inside {
                n += s.calls;
            }
        }
    }
    n
}

#[test]
fn a_holed_plates_check_classifies_each_seam_in_closed_form() {
    let plate = holed_plate();
    assert_eq!(plate.counts().unwrap(), [15, 39, 26]);
    let (report, calls) = session::profiled(|| plate.check(CheckLevel::All).unwrap());
    assert!(report.is_valid(), "{report}");
    // the seam test still runs, once per hole's wall
    assert_eq!(runs(&calls, "topology::face::classify_seam", None), 9);
    // none of its intersections goes through curve caches and a subdivision solve
    assert_eq!(
        runs(
            &calls,
            "curve::global_curve_intersect::cached",
            Some("topology::face::classify_seam")
        ),
        0
    );
    // and none collects touch lists that nothing reads
    assert_eq!(runs(&calls, "topology::face::seam_loop_crossings", None), 0);
}

/// The plate read back from its record, as an application checks a body it keeps: its UV
/// trims are rational quadratic circles (the holes' rims on the two caps) and straight
/// segments (the rest), whose speed and radius of curvature are bounded in closed form above
/// the check's tolerances (`span_bounds`), so the check runs no first-derivative or radius
/// search on them (`nurbs::b_spline_curve::property_analysis`). The analysis memo knows none
/// of these curves: a decoded body's trims differ from the extrude's in their last bits.
#[test]
fn a_decoded_holed_plates_check_bounds_its_trims_in_closed_form() {
    let (record, _) = holed_plate().to_record().unwrap();
    let (plate, _) = Body::from_record(&record, true).unwrap();
    let (report, calls) = session::profiled(|| plate.check(CheckLevel::All).unwrap());
    assert!(report.is_valid(), "{report}");
    assert_eq!(
        runs(&calls, "nurbs::b_spline_curve::property_analysis", None),
        0
    );
}
