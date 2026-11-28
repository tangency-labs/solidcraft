//! A row's slots from its spans of material (design 6.6 to 6.9), in one dimension.
//!
//! A span is where the area and the plate overlap along one row, each end tagged open (the
//! plate's edge, air past it) or closed (an edge of the area inside the plate). This step assumes
//! every open end is a clean crossing (design 6.6 rule 1): the near-set reader that tells a clean
//! crossing from a slanted or crowded one builds the spans and drops the open tag where a crossing
//! is not clean.

use super::lattice::Lattice;
use super::near::outside;
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
    /// The chain of the plate's edge an open lower end lies on (design 2), `None` when closed or
    /// not known (then rule 2 compares it with the neighbour's nearest open end of any chain).
    pub lo_chain: Option<usize>,
    /// The chain an open upper end lies on, as `lo_chain`.
    pub hi_chain: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Lo,
    Hi,
}

/// Plans every row of `lattice` from the spans `spans(k)` gives for row `k`, with no keep-out
/// zones beyond the spans' own ends.
pub fn plan_rows(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    spans: impl FnMut(i64, f64) -> Vec<RowSpan>,
) -> LivingHingePlan {
    plan_rows_zoned(spec, lattice, spans, |_, _| Vec::new())
}

/// Plans every row of `lattice` from its spans and its keep-out zone `zone(k, v)`: `Z_c ∪ Z_o` on
/// the row (design 6.5, 6.6 rule 4) as sorted, disjoint open intervals. A slot entering the zone is
/// shortened to its boundary, one the zone splits gives two pieces, one inside it none. A runner
/// keeps out of the zone everywhere but the component `K` that holds its crossing.
pub fn plan_rows_zoned(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    mut spans: impl FnMut(i64, f64) -> Vec<RowSpan>,
    mut zone: impl FnMut(i64, f64) -> Vec<(f64, f64)>,
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
        let keep_out = zone(k, lattice.row_v(k));
        let claims = one_claim_per_slot(spec, lattice, k, &row_spans);
        let spans_k = row_spans(k);
        for (i, span) in spans_k.iter().enumerate() {
            let (j0, j1) = lattice.slots_meeting(k, span.lo, span.hi);
            for j in j0..=j1 {
                let claim = claims.iter().find(|c| c.slot() == j);
                let (planned, own) = match claim {
                    // Planned with the span before it, through the hole.
                    Some(&Claim::Through(_, h)) if h + 1 == i => continue,
                    Some(&Claim::Through(_, h)) if h == i => {
                        let next = spans_k.get(i + 1).copied().unwrap_or(*span);
                        let merged = RowSpan {
                            hi: next.hi,
                            hi_open: next.hi_open,
                            hi_chain: next.hi_chain,
                            ..*span
                        };
                        let slot = plan_slot(spec, lattice, k, j, &merged, (false, false));
                        (slot, vec![span.hi, next.lo])
                    }
                    _ => {
                        let run = |side: Side| claim == Some(&Claim::Run(j, i, side));
                        let slot =
                            plan_slot(spec, lattice, k, j, span, (run(Side::Lo), run(Side::Hi)));
                        (slot, slot.and_then(|s| s.crossing).into_iter().collect())
                    }
                };
                match planned {
                    Some(slot) => {
                        let cut = cut_by_zone(&slot, &keep_out, &own);
                        if cut.is_empty() {
                            left_out += 1;
                        }
                        pieces.extend(cut);
                    }
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
        band_width: 0.0,
    }
}

/// `|c - x|` for the lattice slot of row `k` whose centre line holds `x`, if one does.
fn delta(lattice: &Lattice, k: i64, x: f64) -> Option<f64> {
    let (j0, j1) = lattice.slots_meeting(k, x, x);
    (j0..=j1)
        .map(|j| (lattice.centre(k, j) - x).abs())
        .min_by(f64::total_cmp)
}

/// The neighbour's `delta` at its open end on the same side and the same chain nearest `x`,
/// infinite when it has none. An end of unknown chain (`None`) matches any chain.
fn neighbour_delta(
    lattice: &Lattice,
    k: i64,
    side: Side,
    x: f64,
    chain: Option<usize>,
    spans: &[RowSpan],
) -> f64 {
    let same = |c: Option<usize>| chain.is_none() || c.is_none() || c == chain;
    let end = spans
        .iter()
        .filter_map(|s| match side {
            Side::Lo if s.lo_open && same(s.lo_chain) => Some(s.lo),
            Side::Hi if s.hi_open && same(s.hi_chain) => Some(s.hi),
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
    chain: Option<usize>,
    row_spans: &impl Fn(i64) -> &'a [RowSpan],
) -> bool {
    let Some(own) = delta(lattice, k, x) else {
        return false;
    };
    [k - 1, k + 1].into_iter().all(|n| {
        let other = neighbour_delta(lattice, n, side, x, chain, row_spans(n));
        own < other || (own == other && k.rem_euclid(2) == 0)
    })
}

/// The open ends of `span` that lattice slot `(k, j)` wins under design 6.6 rule 2, both when it
/// holds both; `one_claim_per_slot` then decides (design 6.6 B1).
fn run_claims<'a>(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    k: i64,
    j: i64,
    span: &RowSpan,
    row_spans: &impl Fn(i64) -> &'a [RowSpan],
) -> (bool, bool) {
    let (a0, a1) = lattice.line(k, j);
    let holds = |x: f64| a0 <= x && x <= a1;
    let may_run = spec.clip == HingeClip::RunOut;
    let run_lo = may_run
        && span.lo_open
        && holds(span.lo)
        && runs_out(lattice, k, Side::Lo, span.lo, span.lo_chain, row_spans);
    let run_hi = may_run
        && span.hi_open
        && holds(span.hi)
        && runs_out(lattice, k, Side::Hi, span.hi, span.hi_chain, row_spans);
    (run_lo, run_hi)
}

/// What one lattice slot does at the plate's edge (design 6.6 B1).
#[derive(Clone, Copy, PartialEq)]
enum Claim {
    /// Slot `j` runs out through the end `side` of span `i`.
    Run(i64, usize, Side),
    /// Slot `j` is one piece through the air between span `i` and span `i + 1`, no overrun.
    Through(i64, usize),
}

impl Claim {
    fn slot(self) -> i64 {
        match self {
            Self::Run(j, ..) | Self::Through(j, _) => j,
        }
    }
}

/// The claims of row `k`, at most one per lattice slot (design 6.6 B1). A slot that wins the
/// upper end of one span and the lower end of the next cuts through the air between them (a hole);
/// otherwise it runs out only through the end it wins nearest its centre, the lower on a tie, and
/// keeps out of `Z_o` at the others: it never runs out at both ends of one span (a neck, a strip),
/// which would cut that material loose.
fn one_claim_per_slot<'a>(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    k: i64,
    row_spans: &impl Fn(i64) -> &'a [RowSpan],
) -> Vec<Claim> {
    // (j, span index, side, |c - x|) in order of j, then of u.
    let mut all: Vec<(i64, usize, Side, f64)> = Vec::new();
    for (i, span) in row_spans(k).iter().enumerate() {
        let (j0, j1) = lattice.slots_meeting(k, span.lo, span.hi);
        for j in j0..=j1 {
            let (lo, hi) = run_claims(spec, lattice, k, j, span, row_spans);
            let c = lattice.centre(k, j);
            if lo {
                all.push((j, i, Side::Lo, (c - span.lo).abs()));
            }
            if hi {
                all.push((j, i, Side::Hi, (c - span.hi).abs()));
            }
        }
    }
    all.sort_by_key(|claim| claim.0);
    let mut out = Vec::new();
    for group in all.chunk_by(|a, b| a.0 == b.0) {
        // A hole: the upper end of span i and the lower end of span i + 1, nearest the centre first.
        let hole = group
            .iter()
            .filter(|a| a.2 == Side::Hi)
            .filter_map(|a| {
                group
                    .iter()
                    .find(|b| b.2 == Side::Lo && b.1 == a.1 + 1)
                    .map(|b| (a.0, a.1, a.3.min(b.3)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((j, i, _)) = hole {
            out.push(Claim::Through(j, i));
            continue;
        }
        // `min_by` keeps the first of equals: the lower end on a tie.
        let best = group.iter().min_by(|a, b| a.3.total_cmp(&b.3));
        out.extend(best.map(|claim| Claim::Run(claim.0, claim.1, claim.2)));
    }
    out
}

fn plan_slot(
    spec: &LivingHingeSpec,
    lattice: &Lattice,
    k: i64,
    j: i64,
    span: &RowSpan,
    (run_lo, run_hi): (bool, bool),
) -> Option<HingeSlot> {
    let r = spec.radius();
    let reach_open = spec.edge_reach();
    let reach_closed = r + spec.margin;
    let (a0, a1) = lattice.line(k, j);
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

/// The pieces of `slot` outside `zone` (design 6.5, 6.6 rule 4). The components of the zone
/// holding a point of `own` do not count against it: a runner's `K` at its crossing, both
/// crossings of a hole a slot cuts through (6.6 B1). Only the piece through the crossing stays a
/// run-out. A piece the zone changed is `Shortened`.
fn cut_by_zone(slot: &HingeSlot, zone: &[(f64, f64)], own: &[f64]) -> Vec<HingeSlot> {
    let rest: Vec<(f64, f64)> = zone
        .iter()
        .copied()
        .filter(|&(lo, hi)| !own.iter().any(|&x| lo < x && x < hi))
        .collect();
    outside(slot.a0, slot.a1, &rest)
        .into_iter()
        .map(|(a0, a1)| {
            let same = (a0 - slot.a0).abs() <= GRAZE && (a1 - slot.a1).abs() <= GRAZE;
            let through = slot.crossing.filter(|&x| a0 <= x && x <= a1);
            let kind = if through.is_some() {
                SlotKind::RunOut
            } else if same {
                slot.kind
            } else {
                SlotKind::Shortened
            };
            HingeSlot {
                a0,
                a1,
                kind,
                crossing: through,
                ..*slot
            }
        })
        .collect()
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
/// a piece shorter than the bridge inside the material being left out. The bar is never above the
/// slot length: a whole slot is never shorter than itself, and slots no longer than the bridges
/// are a warning (design 3.1), not a plan with no slot.
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
    inside >= spec.bridge.min(spec.slot_length) - GRAZE
}
