//! #3142: the edge-continuity check and edge combs through ops, on record order, against closed
//! forms: a box's edges are G0 right angles; a round of radius r between two of its faces meets
//! each tangentially (G1), its curvature jumping by 1 / r; a cylinder's circles have teeth of
//! length 1 / R pointing at the axis. Separate sheets: where their boundary edges meet, each
//! seam measured as the same pair sewn into one body measures it; sheets apart have none; a seam
//! over part of an edge runs over that part only; a seam along a whole closed curve is one seam.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::edge_at;
use solidcore_ops::continuity::{sheet_seams, ContinuityTolerances, G1AllAlong, SeamReport};
use solidcore_ops::stitch::sew;
use solidcore_ops::sweep::{loft, planar_faces, Curve};
use solidcore_ops::{blend, make_box, make_cylinder, Blend, Body, Corners, Error, Order};

/// SolidCraft's tolerances, the caller's: a micron, a degree, 1/100 mm and a tenth of that
/// again per millimetre twice.
fn tolerances() -> ContinuityTolerances {
    ContinuityTolerances {
        gap_mm: 1e-3,
        angle_rad: std::f64::consts::PI / 180.0,
        curvature_per_mm: 1e-2,
        curvature_change_per_mm2: 1e-3,
        curvature_change_rate_per_mm3: 1e-4,
    }
}

#[test]
fn a_box_s_edges_are_right_angles_and_g0_only() {
    let b = make_box([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
    let (_, order) = b.to_record().unwrap();
    let r = b.continuity_report(&order, &[], 9, tolerances()).unwrap();
    assert_eq!(r.seams.len(), 12);
    assert!(r.unmeasured.is_empty());
    for s in &r.seams {
        assert_eq!(s.stations.len(), 9, "edge {}", s.edge);
        for st in &s.stations {
            assert!(st.gap_mm <= 1e-12, "{st:?}");
            assert!((st.normal_angle_deg - 90.0).abs() <= 1e-9, "{st:?}");
        }
        assert_eq!(s.order_at_stations, Some(0), "a crease: G0 only");
        assert_eq!(s.g1_all_along, G1AllAlong::NotTangent, "edge {}", s.edge);
    }
}

#[test]
fn a_round_s_rails_are_tangent_with_a_curvature_jump_of_its_inverse_radius() {
    let b = make_box([0.0, 0.0, 0.0], [40.0, 30.0, 20.0]).unwrap();
    let r = 4.0;
    let e = edge_at(&b, [20.0, 30.0, 20.0]);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    let (_, order) = b.to_record().unwrap();
    let report = b.continuity_report(&order, &[], 9, tolerances()).unwrap();
    let tangent: Vec<_> = report
        .seams
        .iter()
        .filter(|s| s.order_at_stations == Some(1))
        .collect();
    assert_eq!(tangent.len(), 2, "the round's two rails: {report:?}");
    for s in tangent {
        assert_eq!(
            s.g1_all_along,
            G1AllAlong::Tangent,
            "a rail, proved all along"
        );
        for st in &s.stations {
            assert!(st.normal_angle_deg <= 1e-4, "{st:?}");
            assert!((st.curvature_jump_per_mm - 1.0 / r).abs() <= 1e-6, "{st:?}");
        }
    }
    // every other seam is a crease
    assert!(report
        .seams
        .iter()
        .all(|s| s.order_at_stations == Some(1) || s.order_at_stations == Some(0)));
}

#[test]
fn a_cylinder_s_circles_comb_towards_the_axis() {
    let radius = 7.0;
    let b = make_cylinder([0.0, 0.0, 0.0], radius, 12.0).unwrap();
    let (_, order) = b.to_record().unwrap();
    let mut circles = 0;
    for edge in 0..order.edges.len() {
        let teeth = b.edge_comb(&order, edge, 17).unwrap();
        let bent: Vec<_> = teeth
            .iter()
            .filter(|t| t.curvature.iter().any(|c| c.abs() > 1e-12))
            .collect();
        if bent.is_empty() {
            continue;
        }
        circles += 1;
        assert_eq!(
            bent.len(),
            17,
            "a closed circle: 17 teeth, its end its start once"
        );
        for t in bent {
            let [x, y, z] = t.curvature;
            let len2 = x * x + y * y + z * z;
            assert!((len2 * radius * radius - 1.0).abs() <= 1e-12, "{t:?}");
            // towards the axis: the tooth's xy points against the point's
            let dot = x * t.at[0] + y * t.at[1];
            assert!((dot + 1.0).abs() <= 1e-9, "{t:?}");
        }
    }
    assert_eq!(circles, 2, "the top and bottom circles");
}

/// The sheet skinned through lines along x from `x[0]` to `x[1]`, one at each `(y, z)`.
fn skinned(x: [f64; 2], sections: &[(f64, f64)]) -> (Body, Order) {
    let lines: Vec<Vec<Curve>> = sections
        .iter()
        .map(|&(y, z)| {
            vec![Curve::Line {
                start: [x[0], y, z],
                end: [x[1], y, z],
            }]
        })
        .collect();
    let body = loft(&lines, 2, false).unwrap();
    let (_, order) = body.to_record().unwrap();
    (body, order)
}

/// The largest of each order's measures over a seam's stations.
fn worst(s: &SeamReport) -> [f64; 3] {
    let mut w = [0.0_f64; 3];
    for st in &s.stations {
        w[0] = w[0].max(st.gap_mm);
        w[1] = w[1].max(st.normal_angle_rad);
        w[2] = w[2].max(st.curvature_jump_per_mm);
    }
    w
}

/// The zone within which boundary edges count as meeting: a hundredth of a millimetre.
const ZONE: f64 = 1e-2;

/// A flat sheet up to y = 0 and a sheet bending up from it (z = 0, 1, 4 at y = 0, 10, 20),
/// skinned to share the line y = z = 0 from x = 0 to 10, the second either way across: one
/// seam, G0, measured as the same two sewn into one body measure it.
#[test]
fn two_sheets_sharing_an_edge_meet_in_one_seam_measured_as_when_sewn() {
    let flat = skinned([0.0, 10.0], &[(-20.0, 0.0), (-10.0, 0.0), (0.0, 0.0)]);
    let up = [(0.0, 0.0), (10.0, 1.0), (20.0, 4.0)];
    let reversed: Vec<_> = up.iter().rev().copied().collect();
    for sections in [up.to_vec(), reversed] {
        let bent = skinned([0.0, 10.0], &sections);
        let r = sheet_seams(
            &[(&flat.0, &flat.1), (&bent.0, &bent.1)],
            ZONE,
            9,
            tolerances(),
        )
        .unwrap();
        assert_eq!(r.seams.len(), 1, "{r:?}");
        assert!(r.unproved.is_empty() && r.unmeasured.is_empty(), "{r:?}");
        let seam = &r.seams[0];
        assert_eq!(seam.sheets, [0, 1]);
        assert_eq!(seam.report.edge, seam.edges[0]);
        assert_eq!(seam.along, [[0.0, 1.0]; 2], "the whole of both edges");
        assert_eq!(seam.report.stations.len(), 9);
        for st in &seam.report.stations {
            assert!(
                st.point[1].abs() <= 1e-12 && st.point[2].abs() <= 1e-12,
                "{st:?}"
            );
            assert!(st.gap_mm <= 1e-12, "{st:?}");
        }
        assert!(seam.report.order_at_stations >= Some(0), "G0: {seam:?}");

        let sewn = sew(&[&flat.0, &bent.0], ZONE).unwrap();
        let (_, order) = sewn.body.to_record().unwrap();
        let whole = sewn
            .body
            .continuity_report(&order, &[], 9, tolerances())
            .unwrap();
        assert_eq!(whole.seams.len(), 1, "{whole:?}");
        let joined = &whole.seams[0];
        assert_eq!(seam.report.order_at_stations, joined.order_at_stations);
        let (w, j) = (worst(&seam.report), worst(joined));
        for o in 0..3 {
            assert!((w[o] - j[o]).abs() <= 1e-9, "G{o}: {w:?} apart, {j:?} sewn");
        }
        // the proof of G1 all along runs on one edge between two face uses
        assert!(
            matches!(seam.report.g1_all_along, G1AllAlong::NotDecided(_)),
            "{seam:?}"
        );
    }
}

/// Two sheets whose nearest edges are half a millimetre apart do not meet at a hundredth.
#[test]
fn sheets_half_a_millimetre_apart_have_no_seam() {
    let a = skinned([0.0, 10.0], &[(-20.0, 0.0), (-10.0, 0.0), (0.0, 0.0)]);
    let b = skinned([0.0, 10.0], &[(0.5, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    let r = sheet_seams(&[(&a.0, &a.1), (&b.0, &b.1)], ZONE, 9, tolerances()).unwrap();
    assert!(r.seams.is_empty() && r.unproved.is_empty(), "{r:?}");
}

/// A sheet along y = 0 from x = 0 to 10 and one from x = 5 to 15 share x = 5 to 10: one seam
/// over that half of each edge, its stations from x = 5 to 10; the corners where the other
/// edges touch the shared line are points, not seams.
#[test]
fn sheets_sharing_half_an_edge_meet_over_that_half() {
    let a = skinned([0.0, 10.0], &[(-20.0, 0.0), (-10.0, 0.0), (0.0, 0.0)]);
    let b = skinned([5.0, 15.0], &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    let r = sheet_seams(&[(&a.0, &a.1), (&b.0, &b.1)], ZONE, 9, tolerances()).unwrap();
    assert_eq!(r.seams.len(), 1, "{r:?}");
    assert!(r.unproved.is_empty() && r.unmeasured.is_empty(), "{r:?}");
    let seam = &r.seams[0];
    let half = |[s, e]: [f64; 2]| {
        let (lo, hi) = (s.min(e), s.max(e));
        ((lo - 0.5).abs() <= 1e-12 && (hi - 1.0).abs() <= 1e-12)
            || (lo.abs() <= 1e-12 && (hi - 0.5).abs() <= 1e-12)
    };
    assert!(half(seam.along[0]) && half(seam.along[1]), "{seam:?}");
    let xs: Vec<f64> = seam.report.stations.iter().map(|st| st.point[0]).collect();
    assert_eq!(xs.len(), 9);
    let (lo, hi) = xs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &x| {
            (l.min(x), h.max(x))
        });
    assert!(
        (lo - 5.0).abs() <= 1e-9 && (hi - 10.0).abs() <= 1e-9,
        "{xs:?}"
    );
    for st in &seam.report.stations {
        assert!(st.point[1].abs() <= 1e-12, "{st:?}");
        assert!(st.gap_mm <= 1e-12 && st.normal_angle_rad <= 1e-12, "{st:?}");
    }
    assert_eq!(
        seam.report.order_at_stations,
        Some(4),
        "one plane: {seam:?}"
    );
}

/// A circle of radius 5 about the z axis at height `z`, angle 0 (its seam) along `reference`.
fn circle(z: f64, reference: [f64; 3]) -> Curve {
    Curve::Arc {
        center: [0.0, 0.0, z],
        axis: [0.0, 0.0, 1.0],
        reference,
        radius: 5.0,
        start: 0.0,
        end: std::f64::consts::TAU,
    }
}

/// A disc of radius 5 in z = 0 capping a tube of radius 5 from z = 0 to 10 (#18163): they meet
/// along the whole circle, the disc's only edge and the tube's lower one, whether the tube's
/// seam falls on the disc's (both along x) or a quarter turn round (along y). One seam over the
/// whole of both edges, its stations on the circle, the faces square across it (G0, 90
/// degrees), measured as the same pair sewn into one body measures it (along both of its edges
/// where sewing keeps two vertices on the circle).
#[test]
fn a_disc_capping_a_tube_meets_it_in_one_seam_round_the_whole_circle() {
    let disc = planar_faces(&[circle(0.0, [1.0, 0.0, 0.0])]).unwrap();
    let (_, disc_order) = disc.to_record().unwrap();
    for reference in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        let tube = loft(
            &[vec![circle(0.0, reference)], vec![circle(10.0, reference)]],
            1,
            false,
        )
        .unwrap();
        let (_, tube_order) = tube.to_record().unwrap();
        let r = sheet_seams(
            &[(&disc, &disc_order), (&tube, &tube_order)],
            ZONE,
            9,
            tolerances(),
        )
        .unwrap();
        assert_eq!(r.seams.len(), 1, "{reference:?}: {r:?}");
        assert!(
            r.unproved.is_empty() && r.unmeasured.is_empty(),
            "{reference:?}: {r:?}"
        );
        let seam = &r.seams[0];
        assert_eq!(seam.sheets, [0, 1]);
        assert_eq!(seam.along, [[0.0, 1.0]; 2], "the whole of both edges");
        assert_eq!(seam.report.stations.len(), 9);
        for st in &seam.report.stations {
            let [x, y, z] = st.point;
            assert!(
                (x * x + y * y - 25.0).abs() <= 1e-9 && z.abs() <= 1e-12,
                "{st:?}"
            );
            assert!(st.gap_mm <= 1e-9, "{st:?}");
            assert!((st.normal_angle_deg - 90.0).abs() <= 1e-6, "{st:?}");
        }
        assert_eq!(seam.report.order_at_stations, Some(0), "G0: {seam:?}");

        let sewn = sew(&[&disc, &tube], ZONE).unwrap();
        let (_, order) = sewn.body.to_record().unwrap();
        let whole = sewn
            .body
            .continuity_report(&order, &[], 9, tolerances())
            .unwrap();
        // the circle the two were sewn along (the tube's own seam rises from it): sewing keeps
        // both sheets' vertices on it, so it is one edge where the seams fall together and two
        // where they do not
        let joined: Vec<_> = whole
            .seams
            .iter()
            .filter(|s| s.stations.iter().all(|st| st.point[2].abs() <= 1e-9))
            .collect();
        let edges = if reference == [1.0, 0.0, 0.0] { 1 } else { 2 };
        assert_eq!(joined.len(), edges, "{reference:?}: {whole:?}");
        let mut j = [0.0_f64; 3];
        for s in &joined {
            assert_eq!(seam.report.order_at_stations, s.order_at_stations);
            for (m, e) in j.iter_mut().zip(worst(s)) {
                *m = m.max(e);
            }
        }
        let w = worst(&seam.report);
        for o in 0..3 {
            assert!(
                (w[o] - j[o]).abs() <= 1e-9,
                "{reference:?} G{o}: {w:?} apart, {j:?} sewn"
            );
        }
    }
}

/// Hostile input is refused: a zone or a tolerance not finite and positive, a station count
/// out of range, fewer than two sheets, a sheet given twice.
#[test]
fn hostile_sheet_seam_input_is_refused() {
    let a = skinned([0.0, 10.0], &[(-20.0, 0.0), (-10.0, 0.0), (0.0, 0.0)]);
    let b = skinned([0.0, 10.0], &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    let both = [(&a.0, &a.1), (&b.0, &b.1)];
    for zone in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            sheet_seams(&both, zone, 9, tolerances()).unwrap_err(),
            Error::Argument,
            "{zone}"
        );
    }
    for stations in [0, 2, 258, usize::MAX] {
        assert_eq!(
            sheet_seams(&both, ZONE, stations, tolerances()).unwrap_err(),
            Error::Argument
        );
    }
    let mut bad = tolerances();
    bad.curvature_per_mm = f64::NAN;
    assert_eq!(
        sheet_seams(&both, ZONE, 9, bad).unwrap_err(),
        Error::Argument
    );
    bad = tolerances();
    bad.gap_mm = -1.0;
    assert_eq!(
        sheet_seams(&both, ZONE, 9, bad).unwrap_err(),
        Error::Argument
    );
    assert_eq!(
        sheet_seams(&both[..1], ZONE, 9, tolerances()).unwrap_err(),
        Error::Argument
    );
    assert_eq!(
        sheet_seams(&[], ZONE, 9, tolerances()).unwrap_err(),
        Error::Argument
    );
    assert_eq!(
        sheet_seams(&[(&a.0, &a.1), (&a.0, &a.1)], ZONE, 9, tolerances()).unwrap_err(),
        Error::Argument
    );
    // an order of another body: an answer or a refusal, never a panic
    let boxed = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let (_, box_order) = boxed.to_record().unwrap();
    let _ = sheet_seams(&[(&a.0, &box_order), (&b.0, &b.1)], ZONE, 9, tolerances());
}
