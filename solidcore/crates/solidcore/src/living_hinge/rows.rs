//! A row's slots from its spans of material (design 6.6 to 6.9), in one dimension.
//!
//! A span is where the area and the plate overlap along one row, each end tagged open (the
//! plate's edge, air past it) or closed (an edge of the area inside the plate). This step assumes
//! every open end is a clean crossing (design 6.6 rule 1): the near-set reader that tells a clean
//! crossing from a slanted or crowded one builds the spans and drops the open tag where a crossing
//! is not clean.

use super::lattice::Lattice;
use super::{HingeClip, HingeSlot, LivingHingePlan, LivingHingeSpec, SlotKind, GRAZE};

/// One stretch of material along a row, in `u`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowSpan {
    /// The lower end.
    pub lo: f64,
    /// The upper end.
    pub hi: f64,
    /// Whether the lower end is the plate's edge (air below it).
    pub lo_open: bool,
    /// Whether the upper end is the plate's edge (air above it).
    pub hi_open: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Lo,
    Hi,
}

/// Plans every row of `lattice` from the spans `spans(k)` gives for row `k`.
pub fn plan_rows(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    mut spans: impl FnMut(i64, f64) -> Vec<RowSpan>,
) -> LivingHingePlan {
    let (first, last) = lattice.rows;
    // Rows first..=last, with one row of no material on each side for the neighbour rule.
    let all: Vec<Vec<RowSpan>> = (first..=last).map(|k| spans(k, lattice.row_v(k))).collect();
    let row_spans = |k: i64| -> &[RowSpan] {
        usize::try_from(k - first)
            .ok()
            .and_then(|i| all.get(i))
            .map_or(&[], Vec::as_slice)
    };
    let mut slots = Vec::new();
    let mut left_out = 0;
    for k in first..=last {
        let mut pieces = Vec::new();
        for span in row_spans(k) {
            let (j0, j1) = lattice.slots_meeting(k, span.lo, span.hi);
            for j in j0..=j1 {
                match plan_slot(spec, lattice, k, j, span, &row_spans) {
                    Some(slot) => pieces.push(slot),
                    None => left_out += 1,
                }
            }
        }
        pieces.sort_by(|a, b| a.a0.total_cmp(&b.a0));
        pull_apart(spec, &mut pieces);
        for piece in pieces {
            if keeps(spec, &piece) {
                slots.push(piece);
            } else {
                left_out += 1;
            }
        }
    }
    let mut rows = 0;
    let mut last_row = None;
    for s in &slots {
        if last_row != Some(s.row) {
            rows += 1;
            last_row = Some(s.row);
        }
    }
    LivingHingePlan {
        slots,
        rows,
        left_out,
    }
}

/// `|c - x|` for the lattice slot of row `k` whose centre line holds `x`, if one does.
fn delta(lattice: &Lattice, k: i64, x: f64) -> Option<f64> {
    let (j0, j1) = lattice.slots_meeting(k, x, x);
    (j0..=j1)
        .map(|j| (lattice.centre(k, j) - x).abs())
        .min_by(f64::total_cmp)
}

/// The neighbour's `delta` at its open end on the same side nearest `x`, infinite when it has none.
fn neighbour_delta(lattice: &Lattice, k: i64, side: Side, x: f64, spans: &[RowSpan]) -> f64 {
    let end = spans
        .iter()
        .filter_map(|s| match side {
            Side::Lo if s.lo_open => Some(s.lo),
            Side::Hi if s.hi_open => Some(s.hi),
            _ => None,
        })
        .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()));
    end.and_then(|e| delta(lattice, k, e))
        .unwrap_or(f64::INFINITY)
}

/// Whether row `k` runs out at its open end `x` (design 6.6 rule 2): its slot centre is nearer the
/// edge than both neighbours', a tie going to the even row.
fn runs_out<'a>(
    lattice: &Lattice,
    k: i64,
    side: Side,
    x: f64,
    row_spans: &impl Fn(i64) -> &'a [RowSpan],
) -> bool {
    let Some(own) = delta(lattice, k, x) else {
        return false;
    };
    [k - 1, k + 1].into_iter().all(|n| {
        let other = neighbour_delta(lattice, n, side, x, row_spans(n));
        own < other || (own == other && k.rem_euclid(2) == 0)
    })
}

fn plan_slot<'a>(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    k: i64,
    j: i64,
    span: &RowSpan,
    row_spans: &impl Fn(i64) -> &'a [RowSpan],
) -> Option<HingeSlot> {
    let r = spec.radius();
    let reach_open = spec.edge_reach();
    let reach_closed = r + spec.margin;
    let (a0, a1) = lattice.line(k, j);
    let holds = |x: f64| a0 <= x && x <= a1;
    let may_run = spec.clip == HingeClip::RunOut;
    let run_lo = may_run
        && span.lo_open
        && holds(span.lo)
        && runs_out(lattice, k, Side::Lo, span.lo, row_spans);
    let run_hi = may_run
        && span.hi_open
        && holds(span.hi)
        && runs_out(lattice, k, Side::Hi, span.hi, row_spans);
    // A slot that would run out at both ends of one span would cut the span's material loose
    // (critique B1): it runs out at neither and keeps material at both.
    let (run_lo, run_hi) = if run_lo && run_hi {
        (false, false)
    } else {
        (run_lo, run_hi)
    };
    let lo_limit = span.lo
        + if span.lo_open {
            reach_open
        } else {
            reach_closed
        };
    let hi_limit = span.hi
        - if span.hi_open {
            reach_open
        } else {
            reach_closed
        };
    let (mut b0, mut b1, mut changed) = (a0, a1, false);
    if run_lo {
        b0 = span.lo - reach_open;
    } else if a0 < lo_limit - GRAZE {
        b0 = lo_limit;
        changed = true;
    }
    if run_hi {
        b1 = span.hi + reach_open;
    } else if a1 > hi_limit + GRAZE {
        b1 = hi_limit;
        changed = true;
    }
    if b1 < b0 {
        return None;
    }
    let (kind, crossing) = if run_lo {
        (SlotKind::RunOut, Some(span.lo))
    } else if run_hi {
        (SlotKind::RunOut, Some(span.hi))
    } else if changed {
        (SlotKind::Shortened, None)
    } else {
        (SlotKind::Whole, None)
    };
    Some(HingeSlot {
        row: k,
        index: j,
        v: lattice.row_v(k),
        a0: b0,
        a1: b1,
        kind,
        crossing,
    })
}

/// Which end of a run-out piece is in the air: the end nearer its crossing, as in
/// `HingeSlot::cut_length` (both ends of a run-out lie either side of the crossing).
fn air_end(slot: &HingeSlot) -> Option<Side> {
    slot.crossing.map(|x| {
        if (x - slot.a0).abs() < (x - slot.a1).abs() {
            Side::Lo
        } else {
            Side::Hi
        }
    })
}

/// Pulls neighbouring pieces of one row, sorted by `a0`, apart to `mu` of material tip to tip
/// (design 6.8): each facing end gives half the shortfall. Two facing ends in the air have no rule.
fn pull_apart(spec: &LivingHingeSpec, pieces: &mut [HingeSlot]) {
    let mu = spec.least_material();
    for i in 1..pieces.len() {
        let (before, after) = pieces.split_at_mut(i);
        let (Some(p), Some(q)) = (before.last_mut(), after.first_mut()) else {
            continue;
        };
        if air_end(p) == Some(Side::Hi) || air_end(q) == Some(Side::Lo) {
            continue;
        }
        let short = mu - (q.a0 - p.a1 - spec.slot_width);
        if short > GRAZE {
            p.a1 -= short / 2.0;
            q.a0 += short / 2.0;
            for s in [p, q] {
                if s.kind == SlotKind::Whole {
                    s.kind = SlotKind::Shortened;
                }
            }
        }
    }
}

/// Whether a planned piece is kept: `clip` (design 6.7) and the short-piece rule (design 6.9),
/// a piece shorter than the bridge inside the material being left out.
fn keeps(spec: &LivingHingeSpec, slot: &HingeSlot) -> bool {
    if spec.clip == HingeClip::Whole && slot.kind != SlotKind::Whole {
        return false;
    }
    if slot.a1 < slot.a0 {
        return false;
    }
    let r = spec.radius();
    let inside = match (air_end(slot), slot.crossing) {
        (Some(Side::Lo), Some(x)) => slot.a1 - x + r,
        (Some(Side::Hi), Some(x)) => x - slot.a0 + r,
        _ => slot.a1 - slot.a0 + spec.slot_width,
    };
    inside >= spec.bridge
}
