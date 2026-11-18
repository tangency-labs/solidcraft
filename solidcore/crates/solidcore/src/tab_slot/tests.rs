//! Design 16.1's layout and laser outline cases; every number is from the design's rules
//! (`v2-numbers.py`, or the `python3 -I` sums named beside them).

use super::*;

const LASER: f64 = 0.7; // laser_g_min(0.1, 0.5)

fn tabs(stretch: (f64, f64), layout: Layout, w: f64, e: f64) -> Vec<f64> {
    match place(stretch, layout, w, e, LASER).unwrap() {
        Ok(v) => v,
        Err(skip) => panic!("skipped: {skip:?}"),
    }
}

fn skip(stretch: (f64, f64), layout: Layout, w: f64, e: f64) -> StretchSkip {
    match place(stretch, layout, w, e, LASER).unwrap() {
        Ok(v) => panic!("made {v:?}"),
        Err(skip) => skip,
    }
}

fn close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= 1e-9 * y.abs().max(1.0), "{a:?} vs {b:?}");
    }
}

#[test]
fn the_laser_least_space_is_the_web_between_two_slots() {
    assert_eq!(laser_g_min(0.1, 0.5).unwrap(), LASER);
    assert!(laser_g_min(-0.1, 0.5).is_err());
    assert!(laser_g_min(0.1, f64::NAN).is_err());
}

#[test]
fn count_lays_three_tabs_on_fixture_f() {
    // F: one stretch x ∈ [20, 140]; count 3, W 20, e 10 -> [30, 50], [70, 90], [110, 130].
    close(
        &tabs((20.0, 140.0), Layout::Count(3), 20.0, 10.0),
        &[30.0, 70.0, 110.0],
    );
    // One tab is centred.
    close(&tabs((20.0, 140.0), Layout::Count(1), 20.0, 10.0), &[70.0]);
}

#[test]
fn auto_follows_the_thickness() {
    // W = e = 3t. L 120, t 6: N 2, g 48: [38, 56], [104, 122] on F.
    close(
        &tabs((20.0, 140.0), Layout::Auto, 18.0, 18.0),
        &[38.0, 104.0],
    );
    // L 200, t 3: N 6, g 25.6.
    let v = tabs((0.0, 200.0), Layout::Auto, 9.0, 9.0);
    assert_eq!(v.len(), 6);
    assert!((v[1] - v[0] - 9.0 - 25.6).abs() < 1e-9);
    // L 50, t 3: N 2, g 14.
    let v = tabs((0.0, 50.0), Layout::Auto, 9.0, 9.0);
    close(&v, &[9.0, 32.0]);
    // L 152, t 6: N 2, g 80.
    close(
        &tabs((0.0, 152.0), Layout::Auto, 18.0, 18.0),
        &[18.0, 116.0],
    );
    // L 68, 60, 54 (U = W just fits), t 6: one centred tab.
    for l in [68.0, 60.0, 54.0] {
        close(
            &tabs((0.0, l), Layout::Auto, 18.0, 18.0),
            &[(l - 18.0) / 2.0],
        );
    }
    // L 53.999: too_short.
    assert!(matches!(
        skip((0.0, 53.999), Layout::Auto, 18.0, 18.0),
        StretchSkip::TooShort { most: 0, .. }
    ));
}

#[test]
fn spacing_centres_the_group() {
    close(
        &tabs((20.0, 140.0), Layout::Spacing(40.0), 20.0, 10.0),
        &[30.0, 70.0, 110.0],
    );
    close(
        &tabs((20.0, 140.0), Layout::Spacing(35.0), 20.0, 10.0),
        &[35.0, 70.0, 105.0],
    );
}

#[test]
fn the_last_tab_does_not_drift() {
    // 3 000 mm, 250 tabs: the last tab's end is b − e within 1e-9 relative.
    let v = tabs((0.0, 3000.0), Layout::Count(250), 10.0, 5.0);
    assert_eq!(v.len(), 250);
    let end = v[249] + 10.0;
    assert!((end - 2995.0).abs() <= 1e-9 * 2995.0, "{end}");
}

#[test]
fn count_names_the_most_that_fit() {
    // Laser g_min 0.7, W 20, e 10 on F: 3·0.7 + 4·20 = 82.1 ≤ 100; five need 102.8.
    match skip((20.0, 140.0), Layout::Count(6), 20.0, 10.0) {
        StretchSkip::TooShort { length, most, .. } => {
            assert_eq!(most, 4);
            assert_eq!(length, 120.0);
        }
        other @ StretchSkip::SpacingTooSmall { .. } => panic!("{other:?}"),
    }
    assert_eq!(tabs((20.0, 140.0), Layout::Count(4), 20.0, 10.0).len(), 4);
    // Auto with a tiny W is capped by g_min too.
    let v = tabs((0.0, 10.0), Layout::Auto, 1.0, 0.0);
    for p in v.windows(2) {
        assert!(p[1] - p[0] - 1.0 >= LASER - 1e-12);
    }
}

#[test]
fn a_hole_splits_the_strip_into_two_stretches() {
    // Ø20 hole at (80, 33): stretches [20, 70] and [90, 140]; count 2, W 10, e 5.
    close(
        &tabs((20.0, 70.0), Layout::Count(2), 10.0, 5.0),
        &[25.0, 55.0],
    );
    close(
        &tabs((90.0, 140.0), Layout::Count(2), 10.0, 5.0),
        &[95.0, 125.0],
    );
}

#[test]
fn the_spacing_rule_at_its_boundary() {
    // Laser, W 20, c 0.1: 20.7001 fits, 20.6999 skips spacing_too_small (20.7 itself is not
    // asserted: 20.7 − 20 is 0.6999999999999993 in f64).
    assert!(!tabs((20.0, 140.0), Layout::Spacing(20.7001), 20.0, 10.0).is_empty());
    match skip((20.0, 140.0), Layout::Spacing(20.6999), 20.0, 10.0) {
        StretchSkip::SpacingTooSmall { space, least } => {
            assert!((space - 0.6999).abs() < 1e-9);
            assert_eq!(least, LASER);
        }
        other @ StretchSkip::TooShort { .. } => panic!("{other:?}"),
    }
}

#[test]
fn a_spacing_fitting_a_whole_number_of_times_keeps_its_last_tab() {
    // U − W = 3 q exactly up to rounding: 0.1 spacing over 0.3 + W; g_min 0 (q − W = 0.05).
    let v = place((0.0, 0.3 + 0.05), Layout::Spacing(0.1), 0.05, 0.0, 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(v.len(), 4, "{v:?}");
}

#[test]
fn laser_outlines() {
    // Tab [30, 50] of F, h = 6 + 0, δ = 0: 20 × 6.
    let tab = tab_outline(30.0, 20.0, 6.0, 0.0).unwrap().unwrap();
    assert_eq!(tab.outer.segs.len(), 4);
    assert!((tab.signed_area().unwrap().value - 120.0).abs() < 1e-9);
    // Its slot, faces w ∈ [30, 36], c 0.1: 20.2 × 6.2 = 125.24.
    let slot = slot_outline(30.0, 20.0, (30.0, 36.0), 0.1).unwrap();
    assert!((slot.signed_area().unwrap().value - 125.24).abs() < 1e-9);
    let (lo, hi) = slot.extent_along(Point2::new(1.0, 0.0), 1e-9).unwrap();
    assert!((lo - 29.9).abs() < 1e-12 && (hi - 50.1).abs() < 1e-12);
    // Protrusion −7 in a 6 plate: no tab.
    assert_eq!(
        tab_outline(30.0, 20.0, 6.0 - 7.0, 0.0).unwrap(),
        Err(TabSkip::TooShort { length: -1.0 })
    );
    // A negative clearance is refused.
    assert!(slot_outline(30.0, 20.0, (30.0, 36.0), -0.1).is_err());
}

#[test]
fn refusals() {
    let s = (0.0, 100.0);
    assert!(place((f64::NAN, 1.0), Layout::Auto, 1.0, 0.0, LASER).is_err());
    assert!(place((2.0, 1.0), Layout::Auto, 1.0, 0.0, LASER).is_err());
    assert!(place(s, Layout::Auto, 0.0, 0.0, LASER).is_err());
    assert!(place(s, Layout::Auto, f64::INFINITY, 0.0, LASER).is_err());
    assert!(place(s, Layout::Auto, 1.0, -1.0, LASER).is_err());
    assert!(place(s, Layout::Auto, 1.0, 0.0, f64::NAN).is_err());
    assert!(place(s, Layout::Count(0), 1.0, 0.0, LASER).is_err());
    assert!(place(s, Layout::Spacing(0.0), 1.0, 0.0, LASER).is_err());
    assert!(place(s, Layout::Spacing(f64::NAN), 1.0, 0.0, LASER).is_err());
    assert!(tab_outline(f64::NAN, 1.0, 1.0, 0.0).is_err());
    assert!(slot_outline(0.0, 1.0, (1.0, 1.0), 0.1).is_err());
    // Too many tabs is refused, not allocated.
    assert!(place((0.0, 1e12), Layout::Auto, 1.0, 0.0, 0.0).is_err());
}

#[test]
fn random_finite_inputs_never_panic() {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let x = f64::from_bits(state);
        if x.is_finite() {
            x
        } else {
            1.0
        }
    };
    for _ in 0..20_000 {
        let (a, b, w, e, g, q) = (next(), next(), next(), next(), next(), next());
        for layout in [Layout::Auto, Layout::Count(3), Layout::Spacing(q)] {
            let _ = place((a, b), layout, w, e, g);
        }
        let _ = tab_outline(a, w, e, g);
        let _ = slot_outline(a, w, (b, e), g);
    }
}

// Design v4 7.4 / 16.1 `over_hole` web: the Stretches fixture's Bottom face with the Ø20 hole
// at (80, 33), slot plate faces w in [30, 36], c = 0.1, laser MIN_LAND 0.5.
const TOL: f64 = 1e-6;
const MIN_LAND: f64 = 0.5;

fn circle(cx: f64, cy: f64, r: f64) -> Loop2 {
    Loop2 {
        segs: vec![Seg2::Arc {
            center: Point2::new(cx, cy),
            radius: r,
            start: 0.0,
            sweep: -std::f64::consts::TAU,
        }],
    }
}

fn bottom(holes: Vec<Loop2>) -> Region2 {
    Region2 {
        outer: rectangle((0.0, 0.0), (160.0, 66.0)).outer,
        holes,
    }
}

/// The slot of the tab ending `end_distance` before the hole's edge at u = 70.
fn slot_before_hole(end_distance: f64) -> Region2 {
    slot_outline(60.0 - end_distance, 10.0, (30.0, 36.0), 0.1).unwrap()
}

#[test]
fn a_slot_leaving_too_thin_a_web_beside_a_hole_is_over_hole() {
    let face = bottom(vec![circle(80.0, 33.0, 10.0)]);
    match check_slot(&slot_before_hole(0.5), &face, MIN_LAND, TOL).unwrap() {
        Some(SlotSkip::OverHole { web }) => assert!((web - 0.4).abs() < 1e-9, "web {web}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        check_slot(&slot_before_hole(0.7), &face, MIN_LAND, TOL).unwrap(),
        None
    );
}

#[test]
fn the_laser_over_hole_boundary_is_an_end_distance_of_c_plus_min_land() {
    let face = bottom(vec![circle(80.0, 33.0, 10.0)]);
    assert_eq!(
        check_slot(&slot_before_hole(0.6 + 1e-4), &face, MIN_LAND, TOL).unwrap(),
        None
    );
    assert!(matches!(
        check_slot(&slot_before_hole(0.6 - 1e-4), &face, MIN_LAND, TOL).unwrap(),
        Some(SlotSkip::OverHole { .. })
    ));
}

#[test]
fn a_tangent_slot_and_a_slot_holding_a_hole_are_over_hole() {
    let tangent = bottom(vec![circle(80.0, 33.0, 10.0)]);
    assert_eq!(
        check_slot(&slot_before_hole(0.1), &tangent, MIN_LAND, TOL).unwrap(),
        Some(SlotSkip::OverHole { web: 0.0 })
    );
    let inside = bottom(vec![circle(64.5, 33.0, 1.0)]);
    assert_eq!(
        check_slot(&slot_before_hole(0.5), &inside, MIN_LAND, TOL).unwrap(),
        Some(SlotSkip::OverHole { web: 0.0 })
    );
}

#[test]
fn a_slot_running_out_through_the_face_edge_is_at_edge() {
    let face = bottom(Vec::new());
    let out = slot_outline(-5.0, 10.0, (30.0, 36.0), 0.1).unwrap();
    assert_eq!(
        check_slot(&out, &face, MIN_LAND, TOL).unwrap(),
        Some(SlotSkip::AtEdge)
    );
    let flush = slot_outline(5.0, 10.0, (60.0, 65.9), 0.1).unwrap();
    assert_eq!(
        check_slot(&flush, &face, MIN_LAND, TOL).unwrap(),
        Some(SlotSkip::AtEdge)
    );
    assert_eq!(
        check_slot(&slot_before_hole(0.5), &face, MIN_LAND, TOL).unwrap(),
        None
    );
}

#[test]
fn check_slot_refuses_bad_tolerances() {
    let face = bottom(Vec::new());
    let s = slot_before_hole(0.5);
    assert!(check_slot(&s, &face, MIN_LAND, 0.0).is_err());
    assert!(check_slot(&s, &face, MIN_LAND, f64::NAN).is_err());
    assert!(check_slot(&s, &face, TOL, TOL).is_err());
}

/// Design 5.7: cut outlines apart by at least the least land pass; nearer, touching, crossing
/// or one holding the other is the first such pair.
#[test]
fn apart_names_the_first_pair_that_meets() {
    let a = rectangle((0.0, 0.0), (10.0, 6.0));
    let far = rectangle((10.5, 0.0), (20.0, 6.0));
    let near = rectangle((10.4, 0.0), (20.0, 6.0));
    let crossing = rectangle((5.0, 3.0), (8.0, 9.0));
    let inner = rectangle((2.0, 2.0), (4.0, 4.0));
    assert_eq!(
        apart(&[a.clone(), far.clone()], MIN_LAND, TOL).unwrap(),
        None
    );
    assert_eq!(
        apart(&[a.clone(), near], MIN_LAND, TOL).unwrap(),
        Some((0, 1))
    );
    assert_eq!(
        apart(&[far.clone(), a.clone(), crossing], MIN_LAND, TOL).unwrap(),
        Some((1, 2))
    );
    assert_eq!(
        apart(&[inner, a.clone()], MIN_LAND, TOL).unwrap(),
        Some((0, 1))
    );
    assert!(apart(&[a, far], TOL, TOL).is_err());
}

/// Design v2 11.3's ids for the stretch, tab and slot skips.
#[test]
fn skips_name_their_reason_ids() {
    let too_short = StretchSkip::TooShort {
        length: 10.0,
        need: 24.0,
        most: 0,
    };
    let spacing = StretchSkip::SpacingTooSmall {
        space: 0.5,
        least: 0.7,
    };
    assert_eq!(too_short.reason_id(), "too_short");
    assert_eq!(spacing.reason_id(), "spacing_too_small");
    assert_eq!(
        TabSkip::TooShort { length: -1.0 }.reason_id(),
        "tab_too_short"
    );
    assert_eq!(SlotSkip::AtEdge.reason_id(), "at_edge");
    assert_eq!(SlotSkip::OverHole { web: 0.0 }.reason_id(), "over_hole");
}

// Seeded width variation (h0witzer's request 3).

fn varied(stretch: (f64, f64), layout: Layout, w: f64, e: f64, seed: u64, j: f64) -> Vec<(f64, f64)> {
    let starts = tabs(stretch, layout, w, e);
    vary(stretch, &starts, w, e, LASER, seed, j).unwrap()
}

#[test]
fn zero_jitter_keeps_every_tab_as_placed() {
    let v = varied((0.0, 100.0), Layout::Count(3), 6.0, 5.0, 7, 0.0);
    let s = tabs((0.0, 100.0), Layout::Count(3), 6.0, 5.0);
    assert_eq!(v, s.iter().map(|&u| (u, 6.0)).collect::<Vec<_>>());
}

#[test]
fn the_same_seed_gives_the_same_widths_and_another_seed_others() {
    let a = varied((0.0, 100.0), Layout::Count(4), 6.0, 5.0, 11, 2.0);
    assert_eq!(a, varied((0.0, 100.0), Layout::Count(4), 6.0, 5.0, 11, 2.0));
    assert_ne!(a, varied((0.0, 100.0), Layout::Count(4), 6.0, 5.0, 12, 2.0));
    assert!(a.iter().any(|&(_, w)| (w - 6.0).abs() > 1e-9));
}

#[test]
fn widths_stay_within_the_jitter_and_the_floor() {
    for seed in 0..200 {
        for &(_, w) in &varied((0.0, 100.0), Layout::Count(4), 6.0, 5.0, seed, 2.0) {
            assert!((4.0 - 1e-9..=8.0 + 1e-9).contains(&w), "{w}");
        }
        // A jitter past the width cannot take a tab under a tenth of it.
        for &(_, w) in &varied((0.0, 100.0), Layout::Count(4), 6.0, 5.0, seed, 50.0) {
            assert!(w >= 0.6 - 1e-9, "{w}");
        }
    }
}

#[test]
fn varied_tabs_stay_in_the_usable_length_and_keep_the_least_space() {
    for layout in [Layout::Count(1), Layout::Count(5), Layout::Auto, Layout::Spacing(12.0)] {
        for seed in 0..200 {
            let v = varied((10.0, 110.0), layout, 6.0, 5.0, seed, 4.0);
            let mut prev_end = 15.0;
            for &(u, w) in &v {
                assert!(u >= prev_end - 1e-9 && (prev_end == 15.0 || u - prev_end >= LASER - 1e-9), "{v:?}");
                prev_end = u + w;
            }
            assert!(prev_end <= 105.0 + 1e-9, "{v:?}");
        }
    }
}

#[test]
fn one_seed_chains_along_the_stretch_and_a_stretch_index_changes_it() {
    let a = varied((0.0, 100.0), Layout::Count(3), 6.0, 5.0, 5, 2.0);
    // The first tab does not depend on how many follow it.
    let b = varied((0.0, 100.0), Layout::Count(3), 6.0, 5.0, 5, 2.0);
    assert_eq!(a[0], b[0]);
    assert_ne!(stretch_seed(5, 0), stretch_seed(5, 1));
}

#[test]
fn a_bad_jitter_is_refused() {
    for j in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(vary((0.0, 100.0), &[47.0], 6.0, 5.0, LASER, 1, j).is_err());
    }
}
