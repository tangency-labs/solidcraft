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
fn case_e_by_hand(s: &LivingHingeSpec) -> LivingHingePlan {
    let lattice = Lattice::centred(s, &BAND).unwrap();
    let rho = s.edge_reach();
    let in_hole = |v: f64| 46.5 < v && v < 53.5;
    let side_gap = |v: f64| (v - 46.5).abs().min((v - 53.5).abs());
    plan_rows_zoned(
        s,
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
    )
}

#[test]
fn case_e_runs_out_into_a_hole() {
    let s = spec(HingeClip::RunOut, 3.0);
    let plan = case_e_by_hand(&s);
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
fn case_j_spec() -> LivingHingeSpec {
    let mut s = spec(HingeClip::RunOut, 3.0);
    s.margin = 2.0;
    s
}

fn case_j_by_hand(s: &LivingHingeSpec) -> LivingHingePlan {
    let reach = s.radius() + s.margin;
    let lattice = Lattice::centred(s, &BAND).unwrap();
    assert_eq!(lattice.rows.1 - lattice.rows.0 + 1, 26);
    let side_gap = |v: f64| (v - 30.0).min(70.0 - v);
    plan_rows_zoned(
        s,
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
    )
}

#[test]
fn case_j_margin() {
    let s = case_j_spec();
    assert!(close(s.edge_reach(), 2.1) && close(s.radius() + s.margin, 2.1));
    let plan = case_j_by_hand(&s);
    assert_eq!(plan.rows, 24);
    assert_eq!(plan.slots.len(), 72);
    assert_eq!(plan.count(SlotKind::Whole), 48);
    assert_eq!(plan.count(SlotKind::RunOut), 24);
    assert_eq!(plan.count(SlotKind::Shortened), 0);
    assert_eq!(plan.left_out, 6);
    assert!(close(plan.cut_length(), 1218.0), "{}", plan.cut_length());
}

fn frame_rect(u0: f64, v0: f64, u1: f64, v1: f64) -> Vec<NearEdge> {
    let p = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(&a, &b)| NearEdge::Line(a, b))
        .collect()
}

#[test]
fn case_a_through_the_entry_matches_the_hand_numbers() {
    use SlotKind::*;
    // The band u 0..60 by v 30..70 in the plate u 0..60 by v 0..100; its sides are closed.
    let field = [frame_rect(0.0, 30.0, 60.0, 70.0)];
    let plate = [frame_rect(0.0, 0.0, 60.0, 100.0)];
    let closed = [
        NearEdge::Line((0.0, 30.0), (60.0, 30.0)),
        NearEdge::Line((60.0, 70.0), (0.0, 70.0)),
    ];
    let s = spec(HingeClip::RunOut, 3.0);
    let got = plan(&field, &plate, Some(&closed), &s, None).unwrap();
    let (_, want) = plan_band(&s);
    assert_eq!(got.rows, 27);
    assert_eq!(got.slots.len(), 95);
    assert_eq!(
        (got.count(Whole), got.count(Shortened), got.count(RunOut)),
        (41, 26, 28)
    );
    assert_eq!(got.left_out, 0);
    assert!(close(got.cut_length(), 1366.0), "{}", got.cut_length());
    assert_eq!(got.slots.len(), want.slots.len());
    for (g, w) in got.slots.iter().zip(&want.slots) {
        assert!(
            g.row == w.row && close(g.a0, w.a0) && close(g.a1, w.a1) && g.kind == w.kind,
            "{g:?} against {w:?}"
        );
    }
}

#[test]
fn the_entry_refuses_an_empty_area_and_an_empty_plate() {
    let s = spec(HingeClip::RunOut, 3.0);
    let plate = [frame_rect(0.0, 0.0, 60.0, 100.0)];
    assert!(matches!(
        plan(&[], &plate, None, &s, None),
        Err(LivingHingeError::Field(_))
    ));
    let field = [frame_rect(0.0, 30.0, 60.0, 70.0)];
    assert!(matches!(
        plan(&field, &[], None, &s, None),
        Err(LivingHingeError::Plate(_))
    ));
}

#[test]
fn the_box_of_a_half_disc_holds_its_top() {
    let half = [vec![
        NearEdge::Arc {
            center: (0.0, 0.0),
            radius: 2.0,
            start: 0.0,
            sweep: std::f64::consts::PI,
        },
        NearEdge::Line((-2.0, 0.0), (2.0, 0.0)),
    ]];
    let b = frame_box(&half);
    assert!(close(b.u_min, -2.0) && close(b.u_max, 2.0));
    assert!(close(b.v_min, 0.0) && close(b.v_max, 2.0));
}

/// One row (v 1.5) anchored so slot 0 is centred at `u`, with no neighbours (design 6.6 B1, G8
/// fixtures): every crossing is won by rule 2. The zone is every open end's near set at rho.
fn one_row(s: &LivingHingeSpec, u: f64, spans: &[(f64, f64)]) -> LivingHingePlan {
    let b = FrameBox {
        u_min: -30.0,
        u_max: 90.0,
        v_min: 0.0,
        v_max: 3.0,
    };
    let lattice = Lattice::anchored(s, &b, (u, 1.5)).unwrap();
    assert_eq!(lattice.rows, (0, 0));
    let open = |&(lo, hi): &(f64, f64)| RowSpan {
        lo,
        hi,
        lo_open: true,
        hi_open: true,
        lo_chain: None,
        hi_chain: None,
    };
    let rho = s.edge_reach();
    let zone = near::merge(
        spans
            .iter()
            .flat_map(|&(lo, hi)| [(lo - rho, lo + rho), (hi - rho, hi + rho)])
            .collect(),
    );
    plan_rows_zoned(
        s,
        &lattice,
        |_, _| spans.iter().map(open).collect(),
        |_, _| zone.clone(),
    )
}

#[test]
fn a_slot_over_a_narrow_hole_is_one_piece_through_it() {
    use SlotKind::*;
    // A 2.9 mm hole at u 30..32.9; slot 0 (21.55..41.35) wins both crossings. Two run-outs would
    // end 0.1 apart in the hole; it is one piece through it, ignoring both hole components.
    let got = one_row(
        &spec(HingeClip::RunOut, 3.0),
        31.45,
        &[(0.0, 30.0), (32.9, 60.0)],
    );
    assert_ends(
        &row_ends(&got, 0),
        &[
            (-1.4, 18.35, RunOut),
            (21.55, 41.35, Whole),
            (44.55, 61.4, RunOut),
        ],
    );
    assert_eq!(got.left_out, 0);
}

#[test]
fn a_slot_over_a_wide_hole_is_one_piece_through_it() {
    use SlotKind::*;
    // A 6 mm hole at u 30..36, slot 0 (23.1..42.9) centred in it; slot -1 keeps rho from u 0.
    let got = one_row(
        &spec(HingeClip::RunOut, 3.0),
        33.0,
        &[(0.0, 30.0), (36.0, 60.0)],
    );
    assert_ends(
        &row_ends(&got, 0),
        &[
            (1.4, 19.9, Shortened),
            (23.1, 42.9, Whole),
            (46.1, 61.4, RunOut),
        ],
    );
    assert_eq!(got.left_out, 0);
}

#[test]
fn a_slot_over_a_neck_runs_out_at_one_end_only() {
    use SlotKind::*;
    // Bridge 2.5 (mu 1.25, rho 1.35, pitch 22.5). A 4 mm neck u 0..4 between the plate's edge and
    // a hole u 4..20; slot 0 (-7.9..11.9) wins both of the neck's ends at delta 2 each: it runs out
    // at the lower only and keeps rho from u 4 (inside 2.75 >= bridge). Slot 3 (59.6..79.4) runs
    // out at u 60 with 0.5 inside and is left out.
    let got = one_row(
        &spec(HingeClip::RunOut, 2.5),
        2.0,
        &[(0.0, 4.0), (20.0, 60.0)],
    );
    assert_ends(
        &row_ends(&got, 0),
        &[
            (-1.35, 2.65, RunOut),
            (18.65, 34.4, RunOut),
            (37.1, 56.9, Whole),
        ],
    );
    assert_eq!(got.left_out, 1);
}

#[test]
fn a_slot_over_a_strip_runs_out_at_the_end_nearer_its_centre() {
    use SlotKind::*;
    // A 10 mm strip plate u 0..10; the one slot holds both ends.
    let s = spec(HingeClip::RunOut, 3.0);
    let lower = one_row(&s, 4.0, &[(0.0, 10.0)]);
    assert_ends(&row_ends(&lower, 0), &[(-1.4, 8.6, RunOut)]);
    let upper = one_row(&s, 6.0, &[(0.0, 10.0)]);
    assert_ends(&row_ends(&upper, 0), &[(1.4, 11.4, RunOut)]);
    let tie = one_row(&s, 5.0, &[(0.0, 10.0)]);
    assert_ends(&row_ends(&tie, 0), &[(-1.4, 8.6, RunOut)]);
}

#[test]
fn a_loop_becomes_lines_arcs_and_chords() {
    use crate::math::Vec2;
    use crate::prism::profile::{Loop2, Seg2};
    // A straight cubic from (0, 0) to (3, 0), a half circle about (1.5, 0) back to (0, 0).
    let lp = Loop2 {
        segs: vec![
            Seg2::Cubic {
                p0: Vec2::new(0.0, 0.0),
                p1: Vec2::new(1.0, 0.0),
                p2: Vec2::new(2.0, 0.0),
                p3: Vec2::new(3.0, 0.0),
            },
            Seg2::Arc {
                center: Vec2::new(1.5, 0.0),
                radius: 1.5,
                start: 0.0,
                sweep: std::f64::consts::PI,
            },
        ],
    };
    let edges = near_loop(&lp).unwrap();
    let (last, chords) = edges.split_last().unwrap();
    assert!(chords.len() >= 2);
    assert!(
        matches!(chords.first(), Some(NearEdge::Chord((x, y), _)) if close(*x, 0.0) && close(*y, 0.0))
    );
    assert!(
        matches!(chords.last(), Some(NearEdge::Chord(_, (x, y))) if close(*x, 3.0) && close(*y, 0.0))
    );
    assert!(matches!(last, NearEdge::Arc { radius, .. } if close(*radius, 1.5)));
    let line = Loop2 {
        segs: vec![Seg2::Line {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(1.0, 2.0),
        }],
    };
    let edges = near_loop(&line).unwrap();
    assert_eq!(edges, vec![NearEdge::Line((0.0, 0.0), (1.0, 2.0))]);
}

#[test]
fn the_area_edges_inside_the_plate_are_closed() {
    // Case A: the band's ends lie on the plate's edge (u 0 and 60), its sides v 30 and 70 inside.
    let field = [frame_rect(0.0, 30.0, 60.0, 70.0)];
    let plate = [frame_rect(0.0, 0.0, 60.0, 100.0)];
    let closed = closed_edges(&field, &plate);
    assert_eq!(
        closed,
        vec![
            NearEdge::Line((0.0, 30.0), (60.0, 30.0)),
            NearEdge::Line((60.0, 70.0), (0.0, 70.0)),
        ]
    );
    // A disc of the plate's own boundary is open; one inside it is closed.
    let disc = |r: f64| NearEdge::Arc {
        center: (30.0, 50.0),
        radius: r,
        start: 0.0,
        sweep: std::f64::consts::TAU,
    };
    assert!(closed_edges(&[vec![disc(5.0)]], &[vec![disc(5.0)]]).is_empty());
    assert_eq!(
        closed_edges(&[vec![disc(4.0)]], &[vec![disc(5.0)]]).len(),
        1
    );
}

/// Two plans agree slot by slot.
fn assert_same(got: &LivingHingePlan, want: &LivingHingePlan) {
    assert_eq!((got.rows, got.left_out), (want.rows, want.left_out));
    assert_eq!(got.slots.len(), want.slots.len());
    for (g, w) in got.slots.iter().zip(&want.slots) {
        assert!(
            g.row == w.row && close(g.a0, w.a0) && close(g.a1, w.a1) && g.kind == w.kind,
            "{g:?} against {w:?}"
        );
    }
}

/// Case E through the entry: the hole u 20..40 by v 46.5..53.5 is a loop of the plate; the
/// band's sides come from `closed_edges`. Spans, clean ends and zones match the hand-played test.
#[test]
fn case_e_through_the_entry_matches_the_hand_test() {
    let s = spec(HingeClip::RunOut, 3.0);
    let field = [frame_rect(0.0, 30.0, 60.0, 70.0)];
    let plate = [
        frame_rect(0.0, 0.0, 60.0, 100.0),
        frame_rect(20.0, 46.5, 40.0, 53.5),
    ];
    let got = plan(&field, &plate, None, &s, None).unwrap();
    assert_eq!(got.slots.len(), 92);
    assert_same(&got, &case_e_by_hand(&s));
}

/// Case J through the entry: the sides (closed, reach 2.1) leave the outer rows no clean crossing.
#[test]
fn case_j_through_the_entry_matches_the_hand_test() {
    let s = case_j_spec();
    let field = [frame_rect(0.0, 30.0, 60.0, 70.0)];
    let plate = [frame_rect(0.0, 0.0, 60.0, 100.0)];
    let got = plan(&field, &plate, None, &s, None).unwrap();
    assert_eq!((got.slots.len(), got.left_out), (72, 6));
    assert_same(&got, &case_j_by_hand(&s));
}

#[test]
fn a_chord_reaches_further_by_its_tolerance() {
    let line = NearEdge::Line((0.0, 0.0), (10.0, 0.0));
    let chord = NearEdge::Chord((0.0, 0.0), (10.0, 0.0));
    let (l, c) = (near_set(&line, 1.0, 2.0), near_set(&chord, 1.0, 2.0));
    let reach = |q: f64| (q * q - 1.0).sqrt();
    assert!(
        matches!(l.as_slice(), [(a, b)] if close(*a, -reach(2.0)) && close(*b, 10.0 + reach(2.0)))
    );
    assert!(
        matches!(c.as_slice(), [(a, b)] if close(*a, -reach(2.0 + CHORD)) && close(*b, 10.0 + reach(2.0 + CHORD)))
    );
}

#[test]
fn line_ends_are_snapped_onto_an_arcs_ends() {
    use crate::math::Vec2;
    use crate::prism::profile::{Loop2, Seg2};
    // A half disc of radius 2 whose closing line misses the arc's ends by a rounding.
    let e = 1e-8;
    let arc = Seg2::Arc {
        center: Vec2::new(0.0, 0.0),
        radius: 2.0,
        start: 0.0,
        sweep: std::f64::consts::PI,
    };
    let lp = Loop2 {
        segs: vec![
            arc,
            Seg2::Line {
                a: Vec2::new(-2.0 + e, e),
                b: Vec2::new(2.0 - e, -e),
            },
        ],
    };
    let edges = near_loop(&lp).unwrap();
    let end = (
        2.0 * std::f64::consts::PI.cos(),
        2.0 * std::f64::consts::PI.sin(),
    );
    assert_eq!(edges.get(1), Some(&NearEdge::Line(end, (2.0, 0.0))));
    // A line further than ON_EDGE stays where it is.
    let far = Loop2 {
        segs: vec![
            arc,
            Seg2::Line {
                a: Vec2::new(-2.0, 0.5),
                b: Vec2::new(2.0, 0.0),
            },
        ],
    };
    let edges = near_loop(&far).unwrap();
    assert_eq!(edges.get(1), Some(&NearEdge::Line((-2.0, 0.5), (2.0, 0.0))));
}

#[test]
fn a_slot_region_is_a_closed_stadium() {
    let slot = HingeSlot {
        row: 0,
        index: 0,
        v: 5.0,
        a0: 0.0,
        a1: 10.0,
        kind: SlotKind::Whole,
        crossing: None,
    };
    let region = slot_region(&slot, 0.2);
    assert!(region.holes.is_empty());
    assert_eq!(region.outer.segs.len(), 4);
    assert!(region.outer.is_closed(1e-12));
    // 10 x 0.2 plus a disc of radius 0.1, counter-clockwise.
    let area = region.outer.signed_area().unwrap().value;
    let want = 10.0 * 0.2 + std::f64::consts::PI * 0.01;
    assert!((area - want).abs() < 1e-12, "{area}");
}

fn frame_disc(c: (f64, f64), r: f64) -> Vec<NearEdge> {
    [0.0, std::f64::consts::PI]
        .iter()
        .map(|&start| NearEdge::Arc {
            center: c,
            radius: r,
            start,
            sweep: std::f64::consts::PI,
        })
        .collect()
}

#[test]
fn an_area_meets_a_plate_only_by_overlapping_it() {
    let plate = vec![frame_rect(0.0, 0.0, 120.0, 80.0)];
    let holed = vec![
        frame_rect(0.0, 0.0, 120.0, 80.0),
        frame_rect(20.0, 20.0, 40.0, 40.0),
    ];
    let meets_plate = |field: Vec<NearEdge>, plate: &[Vec<NearEdge>]| meets(&[field], plate);
    assert!(meets_plate(frame_rect(40.0, 0.0, 80.0, 80.0), &plate));
    assert!(meets_plate(frame_rect(0.0, 0.0, 120.0, 80.0), &plate));
    assert!(meets_plate(frame_rect(110.0, 70.0, 130.0, 90.0), &plate));
    assert!(meets_plate(frame_rect(-10.0, -10.0, 130.0, 90.0), &plate));
    assert!(meets_plate(frame_disc((125.0, 40.0), 10.0), &plate));
    assert!(meets_plate(frame_rect(10.0, 10.0, 50.0, 50.0), &holed));
    // Touching along an edge or at a corner, beside, apart, inside a hole: no.
    assert!(!meets_plate(frame_rect(120.0, 0.0, 160.0, 80.0), &plate));
    assert!(!meets_plate(frame_rect(120.0, 80.0, 140.0, 100.0), &plate));
    assert!(!meets_plate(frame_rect(200.0, 0.0, 240.0, 80.0), &plate));
    assert!(!meets_plate(frame_disc((130.0, 40.0), 10.0), &plate));
    assert!(!meets_plate(frame_rect(20.0, 20.0, 40.0, 40.0), &holed));
    assert!(!meets_plate(frame_rect(25.0, 25.0, 35.0, 35.0), &holed));
    // A diamond whose corners lie outside, its sides across the plate's corner.
    let diamond = vec![
        NearEdge::Line((130.0, 75.0), (125.0, 90.0)),
        NearEdge::Line((125.0, 90.0), (110.0, 85.0)),
        NearEdge::Line((110.0, 85.0), (115.0, 70.0)),
        NearEdge::Line((115.0, 70.0), (130.0, 75.0)),
    ];
    assert!(meets_plate(diamond, &plate));
}
