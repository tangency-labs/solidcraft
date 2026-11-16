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
    // t even: centres 30 + 23j; t odd: 30 + 23(j + 1/2).
    assert!(close(lattice.centre(13, 0), 30.0));
    assert!(close(lattice.centre(13, -1), 7.0));
    assert!(close(lattice.centre(0, 1), 41.5));
    assert!(close(lattice.centre(0, -1), -4.5));
    assert_eq!(lattice.slots_meeting(13, 0.0, 60.0), (-1, 1));
    assert_eq!(lattice.slots_meeting(0, 0.0, 60.0), (-2, 1));
}

#[test]
fn case_a_runs_out_on_alternate_rows() {
    let s = spec(HingeClip::RunOut, 3.0);
    let (_, plan) = plan_band(&s);
    assert_eq!(plan.rows, 27);
    assert_eq!(plan.slots.len(), 95);
    assert_eq!(plan.count(SlotKind::Whole), 41);
    assert_eq!(plan.count(SlotKind::Shortened), 26);
    assert_eq!(plan.count(SlotKind::RunOut), 28);
    assert_eq!(plan.left_out, 0);
    assert!(close(plan.cut_length(), 1366.0), "{}", plan.cut_length());
    assert!(
        close(plan.open_area(0.2), 275.744690),
        "{}",
        plan.open_area(0.2)
    );
    assert!(close(plan.outline_length(0.2), 2782.893801));
    use SlotKind::*;
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
    // Both ends inside the plate, margin 0: a slot may touch them (reach r).
    let s = spec(HingeClip::RunOut, 3.0);
    let lattice = Lattice::centred(&s, &BAND).unwrap();
    let plan = plan_rows(&s, &lattice, |_, _| {
        vec![RowSpan {
            lo: 0.0,
            hi: 60.0,
            lo_open: false,
            hi_open: false,
        }]
    });
    assert_eq!(plan.count(SlotKind::RunOut), 0);
    use SlotKind::*;
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
