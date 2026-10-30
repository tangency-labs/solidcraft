//! Analytic polygon and canonical-circle clipping. Uncertain predicates refuse
//! the whole operation.
//!
//! Preparation proves simple, exactly closed loops and disjoint interior holes.
//! Consequently even-odd crossings equal outer minus the union of holes. Boundary
//! intervals are removed separately; isolated contacts split, never join, spans.

use crate::{Region2, Seg2, Vec2};

use super::budget::{Budget, Limits};
use super::phase::Param;
use super::{Invalid, MAX_LENGTH_MM};

mod circle;

type Result<T> = std::result::Result<T, Invalid>;
const UNCERTAIN: Invalid = Invalid("unresolved clipping arithmetic");

/// A closed enclosure of a real arithmetic result on represented f64 inputs.
/// This is deliberately a filter, not an arbitrary-precision fallback.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bound {
    lo: f64,
    hi: f64,
}

impl Bound {
    fn point(x: f64) -> Result<Self> {
        Self::new(x, x)
    }

    fn new(lo: f64, hi: f64) -> Result<Self> {
        if lo.is_finite() && hi.is_finite() && lo <= hi { Ok(Self { lo, hi }) } else { Err(UNCERTAIN) }
    }

    fn parameter(value: Param) -> Result<Self> {
        let bound = Self::new(value.lo(), value.hi())?;
        let nearest = value.nearest();
        if nearest.is_finite() && nearest >= bound.lo && nearest <= bound.hi { Ok(bound) } else { Err(UNCERTAIN) }
    }

    fn add(self, other: Self) -> Result<Self> {
        let low = sum(self.lo, other.lo)?;
        let high = sum(self.hi, other.hi)?;
        Self::new(low.lo, high.hi)
    }

    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }

    fn sub(self, other: Self) -> Result<Self> {
        self.add(other.neg())
    }

    fn mul(self, other: Self) -> Result<Self> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for (a, b) in [(self.lo, other.lo), (self.lo, other.hi), (self.hi, other.lo), (self.hi, other.hi)] {
            let v = product(a, b)?;
            lo = lo.min(v.lo);
            hi = hi.max(v.hi);
        }
        Self::new(lo, hi)
    }

    fn div(self, other: Self) -> Result<Self> {
        if other.lo <= 0.0 && other.hi >= 0.0 {
            return Err(UNCERTAIN);
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for (a, b) in [(self.lo, other.lo), (self.lo, other.hi), (self.hi, other.lo), (self.hi, other.hi)] {
            let q = a / b;
            let v = if a == 0.0 || b.abs() == 1.0 || a == b || a == -b { Self::point(q)? } else { Self::new(q.next_down(), q.next_up())? };
            lo = lo.min(v.lo);
            hi = hi.max(v.hi);
        }
        Self::new(lo, hi)
    }

    fn sign(self) -> Result<i8> {
        if self.lo > 0.0 {
            Ok(1)
        } else if self.hi < 0.0 {
            Ok(-1)
        } else if self.lo == 0.0 && self.hi == 0.0 {
            Ok(0)
        } else {
            Err(UNCERTAIN)
        }
    }

    fn middle(self) -> Result<f64> {
        let value = self.lo + (self.hi - self.lo) * 0.5;
        if value.is_finite() && value >= self.lo && value <= self.hi { Ok(value) } else { Err(UNCERTAIN) }
    }
}

// Error-free addition (with gradual underflow). Reject overflow before using
// the residual. Exact operations retain exact zero, rather than an EPS band.
fn sum(a: f64, b: f64) -> Result<Bound> {
    let value = a + b;
    if !value.is_finite() {
        return Err(UNCERTAIN);
    }
    let part = value - a;
    let error = (a - (value - part)) + (b - part);
    if !error.is_finite() {
        return Err(UNCERTAIN);
    }
    Bound::new(if error < 0.0 { value.next_down() } else { value }, if error > 0.0 { value.next_up() } else { value })
}

fn product(a: f64, b: f64) -> Result<Bound> {
    let value = a * b;
    if !value.is_finite() {
        return Err(UNCERTAIN);
    }
    if a == 0.0 || b == 0.0 || a.abs() == 1.0 || b.abs() == 1.0 {
        return Bound::point(value);
    }
    // Bound the exact product's quantum from BOTH operands, not the exponent
    // of its rounded result (which can carry across a power-of-two boundary).
    // If this quantum is smaller than the least subnormal, even a normal
    // product can have a nonzero FMA residual that rounds to zero.
    if quantum_exponent(a) + quantum_exponent(b) < -1074 {
        return Bound::new(value.next_down(), value.next_up());
    }
    // Here the exact product and the rounded value are both multiples of
    // 2^-1074 or larger. A nonzero residual therefore cannot round to zero.
    let error = a.mul_add(b, -value);
    if !error.is_finite() {
        return Err(UNCERTAIN);
    }
    Bound::new(if error < 0.0 { value.next_down() } else { value }, if error > 0.0 { value.next_up() } else { value })
}

fn quantum_exponent(value: f64) -> i32 {
    let exponent = ((value.to_bits() >> 52) & 0x7ff) as i32;
    if exponent == 0 { -1074 } else { exponent - 1075 }
}

fn difference(a: f64, b: f64) -> Result<Bound> {
    sum(a, -b)
}

fn orient(a: Vec2, b: Vec2, p: Vec2) -> Result<i8> {
    let ax = difference(b.x, a.x)?;
    let ay = difference(b.y, a.y)?;
    let px = difference(p.x, a.x)?;
    let py = difference(p.y, a.y)?;
    ax.mul(py)?.sub(ay.mul(px)?)?.sign()
}

fn in_box(p: Vec2, a: Vec2, b: Vec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

fn intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Result<bool> {
    if a.x.max(b.x) < c.x.min(d.x) || c.x.max(d.x) < a.x.min(b.x) || a.y.max(b.y) < c.y.min(d.y) || c.y.max(d.y) < a.y.min(b.y) {
        return Ok(false);
    }
    let ab_c = orient(a, b, c)?;
    let ab_d = orient(a, b, d)?;
    let cd_a = orient(c, d, a)?;
    let cd_b = orient(c, d, b)?;
    Ok((ab_c == 0 && in_box(c, a, b))
        || (ab_d == 0 && in_box(d, a, b))
        || (cd_a == 0 && in_box(a, c, d))
        || (cd_b == 0 && in_box(b, c, d))
        || (ab_c * ab_d < 0 && cd_a * cd_b < 0))
}

fn push<T>(items: &mut Vec<T>, item: T) -> Result<()> {
    items.try_reserve(1).map_err(|_| Invalid("clipping allocation failed"))?;
    items.push(item);
    Ok(())
}

fn edge(points: &[Vec2], i: usize) -> Result<(Vec2, Vec2)> {
    let a = points.get(i).copied().ok_or(Invalid("invalid prepared edge"))?;
    let next = if i + 1 == points.len() { 0 } else { i + 1 };
    let b = points.get(next).copied().ok_or(Invalid("invalid prepared edge"))?;
    Ok((a, b))
}

fn simple(points: &[Vec2], budget: &mut Budget) -> Result<()> {
    for i in 0..points.len() {
        let (a, b) = edge(points, i)?;
        for j in i + 1..points.len() {
            budget.work(1)?;
            let (c, d) = edge(points, j)?;
            if j == i + 1 || (i == 0 && j + 1 == points.len()) {
                let (p, q, r) = if j == i + 1 { (a, b, d) } else { (c, d, b) };
                if orient(p, q, r)? == 0 && !in_box(q, p, r) {
                    return Err(Invalid("overlapping adjacent boundary edges"));
                }
            } else if intersect(a, b, c, d)? {
                return Err(Invalid("boundary is not a simple loop"));
            }
        }
    }
    Ok(())
}

fn separate(a: &[Vec2], b: &[Vec2], budget: &mut Budget) -> Result<()> {
    for i in 0..a.len() {
        let (p, q) = edge(a, i)?;
        for j in 0..b.len() {
            budget.work(1)?;
            let (r, s) = edge(b, j)?;
            if intersect(p, q, r, s)? {
                return Err(Invalid("region boundaries touch or intersect"));
            }
        }
    }
    Ok(())
}

fn inside(points: &[Vec2], p: Vec2, budget: &mut Budget) -> Result<bool> {
    let mut parity = false;
    for i in 0..points.len() {
        budget.work(1)?;
        let (a, b) = edge(points, i)?;
        let crosses = (a.y > p.y) != (b.y > p.y);
        if crosses || in_box(p, a, b) {
            let side = orient(a, b, p)?;
            if side == 0 && in_box(p, a, b) {
                return Err(Invalid("region boundaries touch"));
            }
            if crosses && (side > 0) == (b.y > a.y) {
                parity = !parity;
            }
        }
    }
    Ok(parity)
}

/// Owned and immutable after complete, bounded topology validation.
#[derive(Debug)]
pub(super) struct PreparedRegion {
    shape: PreparedShape,
}

#[derive(Debug)]
enum PreparedShape {
    Polygon(Vec<Vec<Vec2>>),
    Circular(circle::CircularRegion),
}

impl PreparedRegion {
    pub fn new(region: &Region2, budget: &mut Budget) -> Result<Self> {
        if matches!(region.outer.segs.first(), Some(Seg2::Arc { .. })) {
            return Ok(Self { shape: PreparedShape::Circular(circle::CircularRegion::new(region, budget)?) });
        }
        if region.holes.len() > Limits::default().input_pieces / 3 {
            return Err(Invalid("too many region holes"));
        }
        let mut loops = Vec::new();
        for boundary in region.loops() {
            let n = boundary.segs.len();
            budget.input_pieces(n)?;
            budget.work(n)?;
            if n < 3 {
                return Err(Invalid("a polygon loop needs at least three edges"));
            }
            let mut points = Vec::new();
            for (i, seg) in boundary.segs.iter().enumerate() {
                seg.check(MAX_LENGTH_MM).map_err(Invalid)?;
                let Seg2::Line { a, b } = *seg else { return Err(Invalid("not supported yet: non-Line hatch boundary")) };
                let next = boundary.segs.get(if i + 1 == n { 0 } else { i + 1 }).ok_or(Invalid("invalid boundary edge"))?;
                let Seg2::Line { a: next_start, .. } = *next else { return Err(Invalid("not supported yet: non-Line hatch boundary")) };
                if b != next_start {
                    return Err(Invalid("hatch boundary must be exactly closed"));
                }
                push(&mut points, a)?;
            }
            simple(&points, budget)?;
            push(&mut loops, points)?;
        }
        let (outer, holes) = loops.split_first().ok_or(Invalid("missing outer boundary"))?;
        for (i, hole) in holes.iter().enumerate() {
            separate(outer, hole, budget)?;
            let p = hole.first().copied().ok_or(Invalid("empty hole"))?;
            if !inside(outer, p, budget)? {
                return Err(Invalid("hole is not strictly inside outer boundary"));
            }
            for other in holes.iter().skip(i + 1) {
                separate(hole, other, budget)?;
                let q = other.first().copied().ok_or(Invalid("empty hole"))?;
                if inside(hole, q, budget)? || inside(other, p, budget)? {
                    return Err(Invalid("holes must not overlap or nest"));
                }
            }
        }
        Ok(Self { shape: PreparedShape::Polygon(loops) })
    }

    /// Encloses the complete boundary's normal range, including extremal candidates.
    pub fn normal_bounds(&self, frame: &Frame, budget: &mut Budget) -> Result<(f64, f64)> {
        let loops = match &self.shape {
            PreparedShape::Polygon(loops) => loops,
            PreparedShape::Circular(region) => return region.normal_bounds(frame, budget),
        };
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for points in loops {
            for &p in points {
                budget.work(1)?;
                let (_, h) = frame.project(p)?;
                lo = lo.min(h.lo);
                hi = hi.max(h.hi);
            }
        }
        let bounds = Bound::new(lo, hi)?;
        Ok((bounds.lo, bounds.hi))
    }

    /// Exact scalar offsets retained for the existing clipping fixtures.
    #[cfg(test)]
    pub fn clip(&self, frame: &Frame, offset: f64, budget: &mut Budget) -> Result<ClippedLine> {
        self.clip_bound(frame, Bound::point(offset)?, budget)
    }

    /// Classify the exact represented-input normal offset through its enclosure.
    /// nearest() is used only later, when constructing rounded output points.
    pub fn clip_parameter(&self, frame: &Frame, offset: Param, budget: &mut Budget) -> Result<ClippedLine> {
        budget.work(1)?;
        self.clip_bound(frame, Bound::parameter(offset)?, budget)
    }

    fn clip_bound(&self, frame: &Frame, offset: Bound, budget: &mut Budget) -> Result<ClippedLine> {
        let loops = match &self.shape {
            PreparedShape::Polygon(loops) => loops,
            PreparedShape::Circular(region) => return region.clip(frame, offset, budget),
        };
        let mut events = Vec::new();
        let mut base = 0usize;
        for points in loops {
            let mut projected = Vec::new();
            for (i, &p) in points.iter().enumerate() {
                budget.work(1)?;
                let (t, height) = frame.project(p)?;
                let h = height.sub(offset)?;
                let sign = h.sign()?;
                let key = base.checked_add(i).ok_or(UNCERTAIN)?;
                let vertex = Projected { t, h, sign, key };
                if sign == 0 {
                    event(&mut events, Event { at: t, key: Key::Vertex(key), toggle: false, boundary: 0 }, budget)?;
                }
                push(&mut projected, vertex)?;
            }
            for (i, a) in projected.iter().copied().enumerate() {
                budget.work(1)?;
                let b = projected.get(if i + 1 == projected.len() { 0 } else { i + 1 }).copied().ok_or(UNCERTAIN)?;
                if a.sign == 0 && b.sign == 0 {
                    let (first, last) = if a.t.hi < b.t.lo {
                        (a, b)
                    } else if b.t.hi < a.t.lo {
                        (b, a)
                    } else {
                        return Err(UNCERTAIN);
                    };
                    event(&mut events, Event { at: first.t, key: Key::Vertex(first.key), toggle: false, boundary: 1 }, budget)?;
                    event(&mut events, Event { at: last.t, key: Key::Vertex(last.key), toggle: false, boundary: -1 }, budget)?;
                } else if (a.sign > 0) != (b.sign > 0) {
                    let (at, key) = if a.sign == 0 {
                        (a.t, Key::Vertex(a.key))
                    } else if b.sign == 0 {
                        (b.t, Key::Vertex(b.key))
                    } else {
                        let fraction = a.h.neg().div(b.h.sub(a.h)?)?;
                        (a.t.add(b.t.sub(a.t)?.mul(fraction)?)?, Key::Edge(a.key))
                    };
                    event(&mut events, Event { at, key, toggle: true, boundary: 0 }, budget)?;
                }
            }
            base = base.checked_add(points.len()).ok_or(UNCERTAIN)?;
        }
        sweep(events, budget)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    origin: Vec2,
    tangent: Vec2,
    norm_squared: Bound,
}

impl Frame {
    pub fn new(origin: Vec2, tangent: Vec2) -> Result<Self> {
        if !origin.is_finite() || origin.x.abs() > MAX_LENGTH_MM || origin.y.abs() > MAX_LENGTH_MM || !tangent.is_finite() {
            return Err(Invalid("invalid hatch frame"));
        }
        let scale = tangent.x.abs().max(tangent.y.abs());
        if scale == 0.0 {
            return Err(Invalid("invalid hatch tangent"));
        }
        // Scaling before hypot also handles subnormal and very large directions.
        // Refuse if normalization would erase a nonzero direction component.
        let scaled = tangent / scale;
        let normalized = scaled / scaled.len();
        if !normalized.is_finite() || (tangent.x != 0.0 && normalized.x == 0.0) || (tangent.y != 0.0 && normalized.y == 0.0) {
            return Err(UNCERTAIN);
        }
        let tangent = normalized;
        let norm_squared = product(tangent.x, tangent.x)?.add(product(tangent.y, tangent.y)?)?;
        if norm_squared.sign()? != 1 {
            return Err(UNCERTAIN);
        }
        Ok(Self { origin, tangent, norm_squared })
    }

    pub fn tangent(&self) -> Vec2 {
        self.tangent
    }

    pub fn normal(&self) -> Vec2 {
        self.tangent().perp()
    }

    /// Rounded endpoint on the represented frame; never an accuracy certificate.
    pub fn point(&self, offset: f64, t: f64) -> Result<Vec2> {
        if !offset.is_finite() || !t.is_finite() {
            return Err(UNCERTAIN);
        }
        let p = self.origin + self.normal() * offset + self.tangent * t;
        if !p.is_finite() || p.x.abs() > MAX_LENGTH_MM || p.y.abs() > MAX_LENGTH_MM {
            return Err(Invalid("non-finite or excessive hatch output point"));
        }
        Ok(p)
    }

    fn project(&self, p: Vec2) -> Result<(Bound, Bound)> {
        let x = difference(p.x, self.origin.x)?;
        let y = difference(p.y, self.origin.y)?;
        let u = self.tangent;
        let t = x.mul(Bound::point(u.x)?)?.add(y.mul(Bound::point(u.y)?)?)?.div(self.norm_squared)?;
        let h = x.mul(Bound::point(-u.y)?)?.add(y.mul(Bound::point(u.x)?)?)?.div(self.norm_squared)?;
        Ok((t, h))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Span {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug)]
pub(super) struct ClippedLine {
    spans: Vec<Span>,
    ends: Vec<(Bound, Bound)>,
}

impl ClippedLine {
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    // Each endpoint visit and the ordering classification consume work. The
    // query's entry charge covers checked access and bounded value validation.
    fn span_ends(&self, i: usize, budget: &mut Budget) -> Result<(Bound, Bound)> {
        let &(start, end) = self.ends.get(i).ok_or(Invalid("invalid clipped span index"))?;
        budget.work(1)?;
        let start = Bound::new(start.lo, start.hi)?;
        budget.work(1)?;
        let end = Bound::new(end.lo, end.hi)?;
        budget.work(1)?;
        if start.hi < end.lo { Ok((start, end)) } else { Err(UNCERTAIN) }
    }

    /// Conservative cycle-search window from the retained contact enclosures.
    /// Rounded output endpoints must not determine which cycles are visited.
    pub fn span_window(&self, i: usize, budget: &mut Budget) -> Result<(f64, f64)> {
        budget.work(1)?;
        let (start, end) = self.span_ends(i, budget)?;
        Ok((start.lo, end.hi))
    }

    /// Only a certified strict interior owns a dot. Distinct zero tokens still
    /// produce distinct marks; querying another span cannot own this token too.
    pub fn dot_in_span(&self, i: usize, t: Param, budget: &mut Budget) -> Result<Option<f64>> {
        budget.work(1)?;
        let t_bound = Bound::parameter(t)?;
        let (start, end) = self.span_ends(i, budget)?;
        budget.work(1)?;
        let after_start = t_bound.lo > start.hi;
        budget.work(1)?;
        let before_end = t_bound.hi < end.lo;
        if after_start && before_end {
            return Ok(Some(t.nearest()));
        }
        budget.work(1)?;
        let before_start = t_bound.hi <= start.lo;
        budget.work(1)?;
        let after_end = t_bound.lo >= end.hi;
        if before_start || after_end { Ok(None) } else { Err(UNCERTAIN) }
    }

    /// Clip an exact positive dash using enclosure dominance, retaining the
    /// selected endpoints' bounds until positive length has been certified.
    pub fn dash_in_span(&self, i: usize, a: Param, b: Param, budget: &mut Budget) -> Result<Option<Span>> {
        budget.work(1)?;
        let a_bound = Bound::parameter(a)?;
        let b_bound = Bound::parameter(b)?;
        budget.work(1)?;
        if a_bound.hi >= b_bound.lo {
            return Err(UNCERTAIN);
        }
        let (start, end) = self.span_ends(i, budget)?;
        budget.work(1)?;
        if b_bound.hi <= start.lo {
            return Ok(None);
        }
        budget.work(1)?;
        if a_bound.lo >= end.hi {
            return Ok(None);
        }
        let span = self.spans.get(i).ok_or(Invalid("invalid clipped span index"))?;
        // Keep the existing geometric output value beside its actual bound;
        // never promote that rounded value to a singleton certificate.
        budget.work(1)?;
        let (left, output_start) = if a_bound.hi <= start.lo {
            (start, span.start)
        } else {
            budget.work(1)?;
            if a_bound.lo >= start.hi { (a_bound, a.nearest()) } else { return Err(UNCERTAIN) }
        };
        budget.work(1)?;
        let (right, output_end) = if end.hi <= b_bound.lo {
            (end, span.end)
        } else {
            budget.work(1)?;
            if b_bound.hi <= end.lo { (b_bound, b.nearest()) } else { return Err(UNCERTAIN) }
        };
        budget.work(1)?;
        if left.hi >= right.lo {
            budget.work(1)?;
            if left.lo == left.hi && right.lo == right.hi && left.lo == right.lo {
                return Ok(None);
            }
            return Err(UNCERTAIN);
        }
        budget.work(1)?;
        if !output_start.is_finite() || !output_end.is_finite() || output_start >= output_end {
            return Err(UNCERTAIN);
        }
        Ok(Some(Span { start: output_start, end: output_end }))
    }

    /// Boundary contacts are excluded, including a zero-parity reflex vertex.
    /// A parameter inside an uncertain contact enclosure is refused, not guessed.
    #[cfg(test)]
    pub fn contains_strict(&self, t: f64, budget: &mut Budget) -> Result<bool> {
        // A query costs work even when there are no spans or its input is invalid.
        budget.work(1)?;
        if !t.is_finite() {
            return Err(UNCERTAIN);
        }
        for &(start, end) in &self.ends {
            budget.work(1)?;
            if t > start.hi && t < end.lo {
                return Ok(true);
            }
            for contact in [start, end] {
                if t >= contact.lo && t <= contact.hi {
                    if contact.lo == contact.hi {
                        return Ok(false);
                    }
                    return Err(UNCERTAIN);
                }
            }
        }
        Ok(false)
    }
}

#[derive(Clone, Copy)]
struct Projected {
    t: Bound,
    h: Bound,
    sign: i8,
    key: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Vertex(usize),
    Edge(usize),
    Circle(usize, u8),
}

#[derive(Clone, Copy)]
struct Event {
    at: Bound,
    key: Key,
    toggle: bool,
    boundary: i32,
}

fn event(events: &mut Vec<Event>, e: Event, budget: &mut Budget) -> Result<()> {
    budget.events(1)?;
    budget.work(1)?;
    push(events, e)
}

// Fallible insertion sort charges every comparison/move and can stop immediately
// at the work limit. Ordinary polygon scanlines have few events; large cases may
// honestly exhaust this budget rather than run an unaccounted sorting phase.
fn sort(events: &mut [Event], budget: &mut Budget) -> Result<()> {
    for i in 1..events.len() {
        let current = events.get(i).copied().ok_or(UNCERTAIN)?;
        let mut j = i;
        while j > 0 {
            budget.work(1)?;
            let previous = events.get(j - 1).copied().ok_or(UNCERTAIN)?;
            if previous.at.lo.total_cmp(&current.at.lo).then(previous.key.cmp(&current.key)).is_le() {
                break;
            }
            *events.get_mut(j).ok_or(UNCERTAIN)? = previous;
            j -= 1;
        }
        *events.get_mut(j).ok_or(UNCERTAIN)? = current;
    }
    Ok(())
}

fn sweep(mut events: Vec<Event>, budget: &mut Budget) -> Result<ClippedLine> {
    sort(&mut events, budget)?;
    let mut spans = Vec::new();
    let mut ends = Vec::new();
    let mut events = events.into_iter().peekable();
    let mut previous: Option<Bound> = None;
    let mut parity = false;
    let mut boundary = 0i32;
    while let Some(first) = events.next() {
        budget.work(1)?;
        if let Some(start) = previous {
            if start.hi >= first.at.lo {
                return Err(Invalid("unresolved boundary contact ordering"));
            }
            if parity && boundary == 0 {
                push(&mut spans, Span { start: start.middle()?, end: first.at.middle()? })?;
                push(&mut ends, (start, first.at))?;
            }
        }
        parity ^= first.toggle;
        boundary = boundary.checked_add(first.boundary).ok_or(UNCERTAIN)?;
        while events.peek().is_some_and(|next| next.key == first.key) {
            budget.work(1)?;
            let next = events.next().ok_or(UNCERTAIN)?;
            if next.at != first.at {
                return Err(UNCERTAIN);
            }
            parity ^= next.toggle;
            boundary = boundary.checked_add(next.boundary).ok_or(UNCERTAIN)?;
        }
        if boundary < 0 {
            return Err(Invalid("inconsistent boundary intervals"));
        }
        previous = Some(first.at);
    }
    if parity || boundary != 0 {
        return Err(Invalid("inconsistent boundary crossings"));
    }
    Ok(ClippedLine { spans, ends })
}
