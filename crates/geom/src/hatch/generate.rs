//! Bounded hatch expansion over polygons and canonical circular regions.

use crate::{Region2, Vec2};

use super::budget::{Budget, Limits};
use super::clip::{ClippedLine, Frame, PreparedRegion};
use super::phase::{Dyadic, Param};
use super::{Invalid, Pattern};

type Result<T> = std::result::Result<T, Invalid>;

// Leave representable integer neighbours for conservative range padding and cycle ends.
const MAX_INDEX: f64 = 4_503_599_627_370_496.0;
const MAX_EXACT_INDEX: i128 = 1i128 << 52;

/// One generated mark in paper millimetres. Dots remain points, not degenerate lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    Line { a: Vec2, b: Vec2 },
    Dot { at: Vec2 },
}

/// Expand a validated pattern over a supported polygon, disk or concentric annulus.
/// Circular regions use the canonical two-semicircle encoding, with at most one
/// concentric circular hole; general arc boundaries remain unsupported.
/// The result is complete or an error; inputs are never changed. Stored lengths already
/// use paper millimetres, so this function applies no unit conversion or view scale.
///
/// Output follows family order, ascending signed line index and ascending dash cycles
/// within each clipped span. A hole or boundary contact does not restart dash phase.
/// Strokes retain their clipped endpoints; dots require strict interior membership.
/// Every explicit zero retains its own dot, including coincident zeros across cycles.
///
/// Preparation, candidate lines, clipping, dash visits and marks share cumulative limits.
/// Unsupported boundaries, exhausted limits, nonfinite arithmetic, unsafe indices or
/// lost numeric progress return an error rather than truncated or repaired geometry.
/// Normal offsets, dash phase, prefixes and periods use exact stored-binary64 values
/// within a checked i128 envelope; clipping receives their enclosing neighbours.
/// No rounded normal offset is promoted to an exact line for contact classification.
/// Excessive exponent alignment or arithmetic overflow is refused.
/// Parameters round once to nearest-even after the complete exact phase expression;
/// clipping uses their enclosures. Ambiguous contacts, collapsed rounded steps and
/// uncertain normal contacts can still refuse valid patterns. Final frame coordinates are
/// rounded f64 values, not an exact geometry or universal accuracy certificate.
pub fn generate(pattern: &Pattern, region: &Region2) -> Result<Vec<Mark>> {
    generate_with_limits(pattern, region, Limits::default())
}

pub(super) fn generate_with_limits(pattern: &Pattern, region: &Region2, limits: Limits) -> Result<Vec<Mark>> {
    pattern.validate()?;
    let mut budget = Budget::new(limits);
    let region = PreparedRegion::new(region, &mut budget)?;
    let mut marks = Vec::new();
    for family in &pattern.families {
        budget.work(1)?;
        let frame = Frame::new(family.origin, Vec2::from_angle(family.angle))?;
        let (low, high) = region.normal_bounds(&frame, &mut budget)?;
        let a = finite(low / family.delta_y)?;
        let b = finite(high / family.delta_y)?;
        // Include neighbouring indices when projection/division rounds onto an integer.
        // These are candidates, not output promises; exterior candidates also cost budget.
        let lines = IndexRange::new(a.min(b).floor() - 1.0, a.max(b).ceil() + 1.0)?;
        budget.candidate_lines(lines.count()?)?;
        let cycle = Cycle::new(&family.dashes, &mut budget)?;
        let spacing = Dyadic::from_f64(family.delta_y, &mut budget)?;
        let mut previous_offset = None;
        for k in lines.start..=lines.end {
            budget.work(1)?;
            let offset = spacing.mul_index(k, &mut budget)?.param(&mut budget)?;
            let rounded_offset = offset.nearest();
            if let Some(previous) = previous_offset {
                let advances = if family.delta_y > 0.0 { rounded_offset > previous } else { rounded_offset < previous };
                if !advances {
                    return Err(Invalid("line spacing loses numeric progress"));
                }
            }
            previous_offset = Some(rounded_offset);
            let clipped = region.clip_parameter(&frame, offset, &mut budget)?;
            if clipped.spans().is_empty() {
                continue;
            }
            let phase =
                if cycle.entries.is_empty() { Dyadic::ZERO } else { Dyadic::from_f64(family.delta_x, &mut budget)?.mul_index(k, &mut budget)? };
            let member = Member { clipped: &clipped, frame: &frame, offset: rounded_offset, phase };
            for (i, span) in clipped.spans().iter().enumerate() {
                budget.work(1)?;
                if !span.start.is_finite() || !span.end.is_finite() || span.start >= span.end {
                    return Err(Invalid("invalid clipped span"));
                }
                if cycle.entries.is_empty() {
                    push_line(&frame, rounded_offset, span.start, span.end, &mut budget, &mut marks)?;
                } else {
                    cycle.expand(&member, i, &mut budget, &mut marks)?;
                }
            }
        }
    }
    Ok(marks)
}

struct IndexRange {
    start: i64,
    end: i64,
}

impl IndexRange {
    fn new(start: f64, end: f64) -> Result<Self> {
        if !start.is_finite() || !end.is_finite() || start.abs() > MAX_INDEX || end.abs() > MAX_INDEX || start > end {
            return Err(Invalid("hatch line or cycle index is not safely representable"));
        }
        // Callers supply floor/ceil values, and the checked range lies inside exact i64/f64 integers.
        Ok(Self { start: start as i64, end: end as i64 })
    }

    fn count(&self) -> Result<usize> {
        let count = self.end.checked_sub(self.start).and_then(|n| n.checked_add(1)).ok_or(Invalid("hatch index count overflow"))?;
        usize::try_from(count).map_err(|_| Invalid("hatch index count exceeds the platform limit"))
    }

    fn from_exact(start: i128, end: i128) -> Result<Self> {
        let error = Invalid("hatch line or cycle index is not safely representable");
        if !(-MAX_EXACT_INDEX..=MAX_EXACT_INDEX).contains(&start) || !(-MAX_EXACT_INDEX..=MAX_EXACT_INDEX).contains(&end) || start > end {
            return Err(error);
        }
        Ok(Self { start: i64::try_from(start).map_err(|_| error.clone())?, end: i64::try_from(end).map_err(|_| error)? })
    }
}

struct Entry {
    start: Dyadic,
    end: Dyadic,
    value: f64,
}

struct Cycle {
    entries: Vec<Entry>,
    period: Dyadic,
}

struct Member<'a> {
    clipped: &'a ClippedLine,
    frame: &'a Frame,
    offset: f64,
    phase: Dyadic,
}

impl Cycle {
    fn new(dashes: &[f64], budget: &mut Budget) -> Result<Self> {
        let mut entries = Vec::new();
        entries.try_reserve_exact(dashes.len()).map_err(|_| Invalid("cannot allocate hatch dash entries"))?;
        let mut period = Dyadic::from_parts(0, 0, budget)?;
        // Pattern::validate has already bounded this list to six entries.
        for &value in dashes {
            budget.work(1)?;
            let end = period.add(Dyadic::from_f64(value.abs(), budget)?, budget)?;
            if value != 0.0 && end == period {
                return Err(Invalid("dash length loses numeric progress within its cycle"));
            }
            entries.push(Entry { start: period, end, value });
            period = end;
        }
        if !entries.is_empty() && !period.is_positive() {
            return Err(Invalid("dash cycle must have a positive period"));
        }
        Ok(Self { entries, period })
    }

    fn expand(&self, member: &Member<'_>, span_index: usize, budget: &mut Budget, marks: &mut Vec<Mark>) -> Result<()> {
        let (low, high) = member.clipped.span_window(span_index, budget)?;
        let first = Dyadic::from_f64(low, budget)?.sub(member.phase, budget)?.floor_ratio(self.period, budget)?;
        let last = Dyadic::from_f64(high, budget)?.sub(member.phase, budget)?.floor_ratio(self.period, budget)?;
        // Floor acts on exact dyadics, before separately checked padding and index
        // admission. The window uses retained contact bounds, never rounded spans.
        let first = first.checked_sub(1).ok_or(Invalid("hatch cycle index padding overflow"))?;
        let last = last.checked_add(1).ok_or(Invalid("hatch cycle index padding overflow"))?;
        let cycles = IndexRange::from_exact(first, last)?;
        let visits = cycles.count()?.checked_mul(self.entries.len()).ok_or(Invalid("dash visit count overflow"))?;
        // Reserve all visits before iteration, including gaps and cycles outside the span.
        // The previous cycle also owns any trailing zero coincident with this cycle's start.
        budget.dash_steps(visits)?;
        for j in cycles.start..=cycles.end {
            budget.work(1)?;
            let base = member.phase.add(self.period.mul_index(j, budget)?, budget)?;
            let next_j = j.checked_add(1).ok_or(Invalid("dash cycle index overflow"))?;
            // This checked successor is an endpoint, not another enumerated cycle.
            let next_base = member.phase.add(self.period.mul_index(next_j, budget)?, budget)?;
            if next_base.param(budget)?.nearest() <= base.param(budget)?.nearest() {
                return Err(Invalid("dash cycle loses numeric progress"));
            }
            for entry in &self.entries {
                budget.work(1)?;
                let start = self.position(entry.start, base, next_base, budget)?;
                if entry.value == 0.0 {
                    // The full enclosure establishes unique strict span membership
                    // before its one rounded parameter becomes an output coordinate.
                    if let Some(t) = member.clipped.dot_in_span(span_index, start, budget)? {
                        let at = member.frame.point(member.offset, t)?;
                        if !at.is_finite() {
                            return Err(Invalid("dot position is not finite"));
                        }
                        budget.marks(1)?;
                        marks.try_reserve(1).map_err(|_| Invalid("cannot allocate hatch output marks"))?;
                        marks.push(Mark::Dot { at });
                    }
                } else {
                    let end = self.position(entry.end, base, next_base, budget)?;
                    if end.nearest() <= start.nearest() {
                        return Err(Invalid("dash or gap loses numeric progress on its line"));
                    }
                    if entry.value > 0.0
                        && let Some(span) = member.clipped.dash_in_span(span_index, start, end, budget)?
                    {
                        push_line(member.frame, member.offset, span.start, span.end, budget, marks)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn position(&self, prefix: Dyadic, base: Dyadic, next_base: Dyadic, budget: &mut Budget) -> Result<Param> {
        // Canonical cycle endpoints keep final and initial zeros exactly coincident.
        let t = if prefix == self.period { next_base } else { base.add(prefix, budget)? };
        t.param(budget)
    }
}

fn push_line(frame: &Frame, offset: f64, start: f64, end: f64, budget: &mut Budget, marks: &mut Vec<Mark>) -> Result<()> {
    let a = frame.point(offset, start)?;
    let b = frame.point(offset, end)?;
    let progress = (b - a).dot(frame.tangent());
    if !a.is_finite() || !b.is_finite() || a == b || !progress.is_finite() || progress <= 0.0 {
        return Err(Invalid("positive hatch span loses finite directional progress"));
    }
    budget.marks(1)?;
    marks.try_reserve(1).map_err(|_| Invalid("cannot allocate hatch output marks"))?;
    marks.push(Mark::Line { a, b });
    Ok(())
}

fn finite(value: f64) -> Result<f64> {
    if value.is_finite() { Ok(value) } else { Err(Invalid("nonfinite hatch arithmetic")) }
}
