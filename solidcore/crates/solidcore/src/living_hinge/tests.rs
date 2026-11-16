//! Planner tests. The numbers are worked out by hand in design 17.1: plate 100 x 60, area a
//! 40 x 60 band from (30, 0) to (70, 60) whose ends are the plate's edges and whose sides lie
//! inside it; slots 20 mm, bridge 3, rows 1.5 apart, width 0.2, along Y. In the hinge frame the
//! band is `u` 0..60 (along the slots) by `v` 30..70 (across the rows), and every row's material is
//! the one span `[0, 60]` with both ends on the plate's edge.

use super::lattice::{FrameBox, Lattice};
use super::*;

const TOL: f64 = 1e-9;

fn spec(clip: HingeClip, bridge: f64) -> LivingHingeSpec {
    LivingHingeSpec {
        slot_length: 20.0,
        bridge,
        row_spacing: 1.5,
        slot_width: 0.2,
        margin: 0.0,
        clip,
    }
}

const BAND: FrameBox = FrameBox {
    u_min: 0.0,
    u_max: 60.0,
    v_min: 30.0,
    v_max: 70.0,
};

fn plan_band(spec: &LivingHingeSpec) -> (Lattice, LivingHingePlan) {
    let lattice = Lattice::centred(spec, &BAND).unwrap();
    let plan = plan_rows(spec, &lattice, |_, _| {
        vec![RowSpan {
            lo: 0.0,
            hi: 60.0,
            lo_open: true,
            hi_open: true,
            lo_chain: None,
            hi_chain: None,
        }]
    });
    (lattice, plan)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL * b.abs().max(1.0)
}

fn row_ends(plan: &LivingHingePlan, row: i64) -> Vec<(f64, f64, SlotKind)> {
    plan.slots
        .iter()
        .filter(|s| s.row == row)
        .map(|s| (s.a0, s.a1, s.kind))
        .collect()
}

fn assert_ends(got: &[(f64, f64, SlotKind)], want: &[(f64, f64, SlotKind)]) {
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            close(g.0, w.0) && close(g.1, w.1) && g.2 == w.2,
            "{g:?} against {w:?}"
        );
    }
}

#[test]
fn lattice_of_case_a() {
    let s = spec(HingeClip::RunOut, 3.0);
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    // n = floor(39.8 / 1.5) + 1 = 27, odd: rows t = -13 ..= 13, k = 0 ..= 26.
    assert_eq!(lattice.rows, (0, 26));
    assert!(close(lattice.row_v(0), 30.5));
    assert!(close(lattice.row_v(13), 50.0));
    assert!(close(lattice.row_v(26), 69.5));
    // Centres 30 + 23(j + t/2); row 0 has t = -13, so its slots -4.5 .. 64.5 are j = 5 ..= 8.
    assert!(close(lattice.centre(13, 0), 30.0));
    assert!(close(lattice.centre(13, -1), 7.0));
    assert!(close(lattice.centre(0, 7), 41.5));
    assert!(close(lattice.centre(0, 5), -4.5));
    assert_eq!(lattice.slots_meeting(13, 0.0, 60.0), (-1, 1));
    assert_eq!(lattice.slots_meeting(0, 0.0, 60.0), (5, 8));
}

#[test]
fn case_a_runs_out_on_alternate_rows() {
    use SlotKind::*;
    let s = spec(HingeClip::RunOut, 3.0);
    let (_, plan) = plan_band(&s);
    assert_eq!(plan.rows, 27);
    assert_eq!(plan.slots.len(), 95);
    assert_eq!(plan.count(Whole), 41);
    assert_eq!(plan.count(Shortened), 26);
    assert_eq!(plan.count(RunOut), 28);
    assert_eq!(plan.left_out, 0);
    assert!(close(plan.cut_length(), 1366.0), "{}", plan.cut_length());
    assert!(
        close(plan.open_area(0.2), 275.744690),
        "{}",
        plan.open_area(0.2)
    );
    assert!(close(plan.outline_length(0.2), 2782.893801));
    // t = -13 and t = 13 (odd): run out at both edges.
    let odd = [
        (-1.4, 5.4, RunOut),
        (8.6, 28.4, Whole),
        (31.6, 51.4, Whole),
        (54.6, 61.4, RunOut),
    ];
    assert_ends(&row_ends(&plan, 0), &odd);
    assert_ends(&row_ends(&plan, 26), &odd);
    // t = 0 (even): shortened 1.3 mm short of each edge.
    let even = [
        (1.4, 16.9, Shortened),
        (20.1, 39.9, Whole),
        (43.1, 58.6, Shortened),
    ];
    assert_ends(&row_ends(&plan, 13), &even);
}

#[test]
fn case_b_shorten_leaves_a_border() {
    let (_, plan) = plan_band(&spec(HingeClip::Shorten, 3.0));
    assert_eq!(plan.slots.len(), 95);
    assert_eq!(plan.count(SlotKind::Whole), 41);
    assert_eq!(plan.count(SlotKind::Shortened), 54);
    assert_eq!(plan.count(SlotKind::RunOut), 0);
    assert_eq!(plan.left_out, 0);
    assert!(close(plan.cut_length(), 1326.8));
    assert!(close(plan.open_area(0.2), 268.344513));
    assert!(close(plan.outline_length(0.2), 2713.290260));
}

#[test]
fn case_c_whole_keeps_whole_slots_only() {
    let (_, plan) = plan_band(&spec(HingeClip::Whole, 3.0));
    assert_eq!(plan.slots.len(), 41);
    assert_eq!(plan.count(SlotKind::Whole), 41);
    assert_eq!(plan.left_out, 54);
    assert!(close(plan.cut_length(), 811.8));
    assert!(close(plan.open_area(0.2), 163.648053));
}

#[test]
fn case_k_wider_bridge_drops_short_ends() {
    // P = 25: the even-t rows run out (delta 5 against 7.5), the odd-t rows' 1.2 mm ends are left out.
    let (_, plan) = plan_band(&spec(HingeClip::RunOut, 5.0));
    assert_eq!(plan.slots.len(), 67);
    assert_eq!(plan.count(SlotKind::Whole), 41);
    assert_eq!(plan.count(SlotKind::Shortened), 0);
    assert_eq!(plan.count(SlotKind::RunOut), 26);
    assert_eq!(plan.left_out, 28);
    assert!(close(plan.cut_length(), 1199.2));
    assert!(close(plan.open_area(0.2), 241.536460));
}

#[test]
fn closed_ends_keep_the_margin() {
    use SlotKind::*;
    // Both ends inside the plate, margin 0: a slot may touch them (reach r).
    let s = spec(HingeClip::RunOut, 3.0);
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    let plan = plan_rows(&s, &lattice, |_, _| {
        vec![RowSpan {
            lo: 0.0,
            hi: 60.0,
            lo_open: false,
            hi_open: false,
            lo_chain: None,
            hi_chain: None,
        }]
    });
    assert_eq!(plan.count(RunOut), 0);
    let even = [
        (0.1, 16.9, Shortened),
        (20.1, 39.9, Whole),
        (43.1, 59.9, Shortened),
    ];
    assert_ends(&row_ends(&plan, 13), &even);
}

#[test]
fn determinism() {
    let s = spec(HingeClip::RunOut, 3.0);
    assert_eq!(plan_band(&s).1, plan_band(&s).1);
}

#[test]
fn refusals() {
    let mut s = spec(HingeClip::RunOut, 3.0);
    s.row_spacing = 0.2;
    assert!(matches!(s.check(), Err(LivingHingeError::Parameter(_))));
    let mut s = spec(HingeClip::RunOut, 3.0);
    s.slot_length = f64::NAN;
    assert!(matches!(s.check(), Err(LivingHingeError::Parameter(_))));
    let s = spec(HingeClip::RunOut, 3.0);
    let huge = FrameBox {
        u_min: -1e6,
        u_max: 1e6,
        v_min: -1e6,
        v_max: 1e6,
    };
    assert!(matches!(
        Lattice::centred(&s, &huge),
        Err(LivingHingeError::TooManySlots { .. })
    ));
}

#[test]
fn pieces_a_hair_apart_are_pulled_to_the_least_material() {
    use SlotKind::*;
    // A keep-out 0.05 wide at u = 30 splits the middle slot of the even rows into two pieces 0.05
    // apart tip to tip; design 6.8 pulls each facing end back half of the 1.25 shortfall.
    let s = spec(HingeClip::RunOut, 3.0);
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    let plan = plan_rows(&s, &lattice, |_, _| {
        vec![
            RowSpan {
                lo: 0.0,
                hi: 30.0,
                lo_open: true,
                hi_open: false,
                lo_chain: None,
                hi_chain: None,
            },
            RowSpan {
                lo: 30.05,
                hi: 60.0,
                lo_open: false,
                hi_open: true,
                lo_chain: None,
                hi_chain: None,
            },
        ]
    });
    assert_ends(
        &row_ends(&plan, 13),
        &[
            (1.4, 16.9, Shortened),
            (20.1, 29.275, Shortened),
            (30.775, 39.9, Shortened),
            (43.1, 58.6, Shortened),
        ],
    );
    assert_eq!(plan.left_out, 0);
}

/// Case D: a keep-out square (45, 25)-(55, 35) in the sketch, margin 0, so `Z_c` reaches `r` = 0.1
/// past its sides: on rows v 45.5 ..= 54.5 the zone is (24.9, 35.1). Rows with t even split their
/// middle slot into [20.1, 24.9] and [35.1, 39.9]; rows with t odd shorten theirs to [8.6, 24.9] and
/// [35.1, 51.4]. Rows v 44 and 56 pass it whole. 95 slots + 3 splits = 98.
#[test]
fn case_d_closed_keep_out_splits_and_shortens() {
    let s = spec(HingeClip::RunOut, 3.0);
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    let plan = plan_rows_zoned(
        &s,
        &lattice,
        |_, _| {
            vec![RowSpan {
                lo: 0.0,
                hi: 60.0,
                lo_open: true,
                hi_open: true,
                lo_chain: None,
                hi_chain: None,
            }]
        },
        |_, v| {
            if 44.9 < v && v < 55.1 {
                vec![(24.9, 35.1)]
            } else {
                Vec::new()
            }
        },
    );
    assert_eq!(plan.slots.len(), 98);
    let middle = |row: i64| -> Vec<(f64, f64, SlotKind)> {
        row_ends(&plan, row)
            .into_iter()
            .filter(|e| e.0 > 8.0 && e.1 < 52.0)
            .collect()
    };
    let shortened = SlotKind::Shortened;
    for row in [11, 13, 15] {
        assert_ends(
            &middle(row),
            &[(20.1, 24.9, shortened), (35.1, 39.9, shortened)],
        );
    }
    for row in [10, 12, 14, 16] {
        assert_ends(
            &middle(row),
            &[(8.6, 24.9, shortened), (35.1, 51.4, shortened)],
        );
    }
    for row in [9, 17] {
        assert!(middle(row).iter().all(|e| e.2 == SlotKind::Whole));
    }
}

/// Case E: a through hole (46.5, 20)-(53.5, 40) in the plate. Rows inside it have two spans,
/// [0, 20] and [40, 60], whose ends at the hole are open where the crossing is clean (rows v 48.5,
/// 50, 51.5) and closed where a side of the hole lies within `rho` = 1.4 (rows v 47, 53). `Z_o` on
/// a row is the near set of the hole's edges with reach 1.4. Rows 48.5 and 51.5 run out into the
/// hole from both sides: [8.6, 21.4] and [38.6, 51.4]. Rows v 45.5 and 54.5, 1.0 outside, are
/// shortened by the hole's corner discs to 20 - sqrt(1.4^2 - 1) and 40 + sqrt(1.4^2 - 1). The
/// middle slots of rows 47, 50 and 53 lie in the hole: 95 - 3 = 92.
#[test]
fn case_e_runs_out_into_a_hole() {
    let s = spec(HingeClip::RunOut, 3.0);
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    let rho = s.edge_reach();
    let in_hole = |v: f64| 46.5 < v && v < 53.5;
    let side_gap = |v: f64| (v - 46.5).abs().min((v - 53.5).abs());
    let plan = plan_rows_zoned(
        &s,
        &lattice,
        |_, v| {
            if in_hole(v) {
                let clean = side_gap(v) >= rho;
                vec![
                    RowSpan {
                        lo: 0.0,
                        hi: 20.0,
                        lo_open: true,
                        hi_open: clean,
                        lo_chain: None,
                        hi_chain: None,
                    },
                    RowSpan {
                        lo: 40.0,
                        hi: 60.0,
                        lo_open: clean,
                        hi_open: true,
                        lo_chain: None,
                        hi_chain: None,
                    },
                ]
            } else {
                vec![RowSpan {
                    lo: 0.0,
                    hi: 60.0,
                    lo_open: true,
                    hi_open: true,
                    lo_chain: None,
                    hi_chain: None,
                }]
            }
        },
        |_, v| {
            let d = side_gap(v);
            if in_hole(v) && d >= rho {
                vec![(20.0 - rho, 20.0 + rho), (40.0 - rho, 40.0 + rho)]
            } else if in_hole(v) {
                // The side's near set covers the hole's ends' zones: one interval.
                vec![(20.0 - rho, 40.0 + rho)]
            } else if d < rho {
                let reach = (rho * rho - d * d).sqrt();
                vec![(20.0 - reach, 40.0 + reach)]
            } else {
                Vec::new()
            }
        },
    );
    assert_eq!(plan.slots.len(), 92);
    let middle = |row: i64| -> Vec<(f64, f64, SlotKind)> {
        row_ends(&plan, row)
            .into_iter()
            .filter(|e| e.0 > 8.0 && e.1 < 52.0)
            .collect()
    };
    for row in [12, 14] {
        assert_ends(
            &middle(row),
            &[
                (8.6, 21.4, SlotKind::RunOut),
                (38.6, 51.4, SlotKind::RunOut),
            ],
        );
    }
    let corner = (1.4_f64 * 1.4 - 1.0).sqrt();
    assert!((20.0 - corner - 19.020204).abs() < 1e-6);
    for row in [10, 16] {
        assert_ends(
            &middle(row),
            &[
                (8.6, 20.0 - corner, SlotKind::Shortened),
                (40.0 + corner, 51.4, SlotKind::Shortened),
            ],
        );
    }
    for row in [11, 13, 15] {
        assert!(middle(row).is_empty(), "{:?}", middle(row));
    }
}

/// Case J: `margin: 2`. `n = floor(35.8 / 1.5) + 1 = 24`, even: rows at v = 50 +- 0.75 +- 1.5i,
/// slot phases +-P/4 (centres 35.75 + 23j or 24.25 + 23j). Every row strictly inside the band is
/// tried (design 5.1): 26 rows, 31.25 ..= 68.75. The band's sides are closed edges with reach
/// `r + m` = 2.1; the rows at 31.25 and 68.75 lie 1.25 from a side, so the side's near set covers
/// the whole row (to 60 + sqrt(2.1^2 - 1.25^2) past each end) and their crossings are not clean
/// (the caller plays `clean_crossing` and drops the open tags): their 3 slots each are left out.
/// The other 24 rows hold one run-out (its centre 1.25 from the edge it runs out at; its
/// neighbours hold no slot there) and two whole slots: 72 slots, 48 whole, 24 run-outs, 6 left
/// out, cut 24 x (2 x 19.8 + 11.15) = 1218 (python3 hand count; design 17.1 table row J).
#[test]
fn case_j_margin() {
    let mut s = spec(HingeClip::RunOut, 3.0);
    s.margin = 2.0;
    let reach = s.radius() + s.margin;
    assert!(close(s.edge_reach(), 2.1) && close(reach, 2.1));
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    assert_eq!(lattice.rows.1 - lattice.rows.0 + 1, 26);
    let side_gap = |v: f64| (v - 30.0).min(70.0 - v);
    let plan = plan_rows_zoned(
        &s,
        &lattice,
        |_, v| {
            let clean = side_gap(v) >= reach;
            vec![RowSpan {
                lo: 0.0,
                hi: 60.0,
                lo_open: clean,
                hi_open: clean,
                lo_chain: None,
                hi_chain: None,
            }]
        },
        |_, v| {
            let d = side_gap(v);
            if d < reach {
                let past = (reach * reach - d * d).sqrt();
                vec![(-past, 60.0 + past)]
            } else {
                Vec::new()
            }
        },
    );
    assert_eq!(plan.rows, 24);
    assert_eq!(plan.slots.len(), 72);
    assert_eq!(plan.count(SlotKind::Whole), 48);
    assert_eq!(plan.count(SlotKind::RunOut), 24);
    assert_eq!(plan.count(SlotKind::Shortened), 0);
    assert_eq!(plan.left_out, 6);
    assert!(close(plan.cut_length(), 1218.0), "{}", plan.cut_length());
}
