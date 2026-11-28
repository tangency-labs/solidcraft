//! The rows whose split keeps the split face's UV trims on the new face (tests only): every
//! UV trim the B-reps of these rows keep is certified on its face's surface.
//!
//! `MakeEdgeInFaceWithVU` (`topology::brep::p9`) gives the new face of a split the trims of
//! the face it splits, rather than dropping them to be made again, and
//! `BSplineSurface::copy_and_add_analytics` keeps an exact copy of a B-spline where its
//! recognised analytic does not give the same points at the same parameters. [`ROWS`] are the
//! goldens rows where the new face keeps trims that way: `blend.feature_executive`'s fillets
//! on copied faces, `heal.heal_data_p3`'s seam cone splits, `primitive.api_trim_ops`' surface
//! line splits and three `topology.brep_p10` subdivisions.
//!
//! For each row, every edge with a curve in every B-rep the runner writes, every edge use
//! that holds a UV trim: the proved bound `B` on the Hausdorff distance between the edge
//! curve and the trim's image ([`crate::certify::trim_tolerance::trim_bounds_any_surface`]:
//! the kernel's `trim_image_gap_bound`, bisected interval enclosures with rounding carried,
//! a UV line enclosed on any surface with an exact net) is within the edge's tolerance, over
//! the whole edge interval. No point sample decides it. A trim whose kernel bound is over the
//! tolerance is certified by the mean-value certifier ([`crate::trims_on_edges::certify_edgeuse`])
//! instead, where that proves it within the tolerance; the kernel's enclosure is loose for
//! some circle trims on a flat B-spline cap (#22037), and the count of such trims is asserted
//! ([`MEAN_VALUE_CERTIFIED`]). A trim whose bound is not enclosed is
//! accepted only where its edge is in `certify/trim_tolerance_open.tsv` with the class
//! `not-enclosed`, and every such entry of these rows must be reached.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use crate::case::{repo_root, CaseContext};

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// The kept trims whose kernel bound is over their edge's tolerance and which the mean-value
/// certifier proves instead (#22037): the four quarter arcs and two full circles of
/// `subdivide_cylinder_face0_cinf`'s caps. A change in the kernel's enclosure shows here.
const MEAN_VALUE_CERTIFIED: usize = 6;

/// The rows, unit and case.
const ROWS: &[(&str, &str)] = &[
    ("blend.feature_executive", "fillet_each_fillet1019"),
    (
        "blend.feature_executive",
        "fillet_group_torii_fillet_b_rb23",
    ),
    ("blend.feature_executive", "rb_33_box_with_large_fillet_6"),
    (
        "blend.feature_executive",
        "single_face_box_with_large_fillet_1",
    ),
    (
        "blend.feature_executive",
        "single_face_box_with_large_fillet_2",
    ),
    (
        "blend.feature_executive",
        "single_face_box_with_large_fillet_3",
    ),
    (
        "blend.feature_executive",
        "single_face_box_with_large_fillet_4",
    ),
    ("blend.feature_executive", "single_face_fillet1019_3"),
    ("blend.feature_executive", "single_face_fillet1019_6"),
    ("blend.feature_executive", "single_face_fillet1019_8"),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r0.02_0.03_h1_s120_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r0.02_0.03_h1_s60_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r0.02_0.03_h1_s90_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r0.02_0.03_h25_s120_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r0.02_0.03_h25_s60_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r0.02_0.03_h25_s90_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r1_0.5_h1_s120_q0",
    ),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_0.5_h1_s60_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_0.5_h1_s90_q0"),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r1_0.5_h25_s120_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r1_0.5_h25_s60_q0",
    ),
    (
        "heal.heal_data_p3",
        "split_face_seam_cone_r1_0.5_h25_s90_q0",
    ),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h1_s120_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h1_s120_q1"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h1_s60_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h1_s60_q1"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h1_s90_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h1_s90_q1"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h25_s120_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h25_s120_q1"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h25_s60_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h25_s60_q1"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h25_s90_q0"),
    ("heal.heal_data_p3", "split_face_seam_cone_r1_1_h25_s90_q1"),
    ("heal.heal_data_p3", "split_face_seam_cone_wide_10_350"),
    ("heal.heal_data_p3", "split_face_seam_cone_wide_200_340"),
    ("primitive.api_trim_ops", "gen0087"),
    ("primitive.api_trim_ops", "pp_surface_line_delete_point"),
    ("primitive.api_trim_ops", "pp_surface_line_split"),
    ("primitive.api_trim_ops", "pp_surface_line_then_cross_line"),
    ("primitive.api_trim_ops", "pp_surface_line_then_cross_split"),
    ("topology.brep_p10", "subdivide_cylinder_face0_cinf"),
    ("topology.brep_p10", "subdivide_nurb_nurb_2_cinf"),
    ("topology.brep_p10", "subdivide_surf_rev_cinf"),
];

/// Every UV trim kept by the B-reps of [`ROWS`] is certified within its edge's tolerance, or
/// its edge is a `not-enclosed` entry of the trim tolerance certificate's open list, and each
/// such entry of these rows is reached.
#[test]
fn every_kept_trim_is_certified() {
    let k = solidcore::kernel::Kernel::default();
    let open: BTreeSet<(String, String, usize, usize)> =
        crate::certify::trim_tolerance::not_enclosed_entries()
            .into_iter()
            .filter(|(u, c, ..)| ROWS.contains(&(u.as_str(), c.as_str())))
            .collect();
    let mut open_seen = BTreeSet::new();
    let mut failures = Vec::new();
    let mut fallback = Vec::new();
    let (mut kept, mut certified, mut open_trims) = (0_usize, 0_usize, 0_usize);
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let mut by_unit: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for &(unit, case) in ROWS {
        by_unit.entry(unit).or_default().push(case);
    }
    for (unit_id, names) in by_unit {
        let u = crate::registry()
            .into_iter()
            .find(|u| u.id() == unit_id)
            .unwrap_or_else(|| panic!("no unit {unit_id}"));
        let cases = u.cases(&ctx).unwrap();
        for name in names {
            let case = cases
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
            crate::certify::watch_written();
            let out = u.run_rust(case);
            let written = crate::certify::take_written();
            if let Err(e) = out {
                panic!("{unit_id} {name}: {e:?}");
            }
            let at = format!("{unit_id} {name}");
            let mut row_kept = 0_usize;
            for (bi, b) in written.iter().enumerate() {
                for e in solidcore::topology::brep::p1::edges(b, None).expect("edges") {
                    let rec = b.edge(e).expect("edge");
                    if rec.curve.is_none() {
                        continue;
                    }
                    let tol = rec.tolerance;
                    let key = (
                        unit_id.to_string(),
                        name.to_string(),
                        bi,
                        e.index() as usize,
                    );
                    let bounds = crate::certify::trim_tolerance::trim_bounds_any_surface(&k, b, e);
                    for (eu, bound) in bounds {
                        kept += 1;
                        row_kept += 1;
                        // UNPROVED by the kernel's enclosure (#22037): it is thousands of times
                        // loose for the circle trims of `subdivide_cylinder_face0_cinf` (bounds
                        // 7.15e-4 to 0.098 for trims about 5e-15 off). A trim whose kernel bound
                        // is over the tolerance is certified by the mean-value certifier instead,
                        // at the target `certify_brep` uses; each such trim is listed and counted.
                        let mean_value = match bound {
                            Some(x) if x > tol => {
                                crate::trims_on_edges::certify_edgeuse(&k, b, eu, tol / 8.0)
                                    .ok()
                                    .filter(crate::trims_on_edges::TrimCertificate::holds)
                            }
                            _ => None,
                        };
                        match (bound, mean_value) {
                            (Some(x), None) if x <= tol => certified += 1,
                            (Some(x), Some(c)) => {
                                certified += 1;
                                fallback.push(format!(
                                    "{at} body {bi} edge {} use {}: kernel B {x:e} over \
                                     {tol:e}; proved {:e} by trims_on_edges::certify_edgeuse",
                                    e.index(),
                                    eu.index(),
                                    c.bound
                                ));
                            }
                            (None, _) if open.contains(&key) => {
                                open_trims += 1;
                                open_seen.insert(key.clone());
                            }
                            (x, _) => failures.push(format!(
                                "{at} body {bi} edge {} use {}: B {x:?} over {tol:e}",
                                e.index(),
                                eu.index()
                            )),
                        }
                    }
                }
            }
            if row_kept == 0 {
                failures.push(format!("{at}: no kept trim"));
            }
        }
    }
    eprintln!(
        "every_kept_trim_is_certified: {} rows, {kept} kept trims, {certified} certified, \
         {open_trims} open; {} certified by the mean-value certifier (#22037):\n{}",
        ROWS.len(),
        fallback.len(),
        fallback.join("\n")
    );
    assert_eq!(open_seen, open, "not-enclosed entries not reached");
    assert_eq!(
        fallback.len(),
        MEAN_VALUE_CERTIFIED,
        "trims certified by the mean-value certifier, not the kernel's bound (#22037)"
    );
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
