//! Bounded admission and conservative bounds for the real X1 `BulgedPolyline`.
//! Bounds contain each whole supporting circle, not only the directed arc.
//! No trig/hypot, sampling, output array, topology repair or physical stroke width.

use crate::geom::certified::next_up;
use crate::math::Vec2;
use crate::prism::recognition::RecognitionVector;

use crate::prism::path_export::{PathRefusal, Vertex, MAX_PATH_ITEMS};
type DxfVertex = Vertex<Vec2>;
use super::Bounds as DxfGeometryBounds;
type Result<T> = std::result::Result<T, PathRefusal>;

const MAX_WORK: usize = 64 << 20;
const EDGE_WORK: usize = 512;

fn invalid(message: &str) -> PathRefusal {
    PathRefusal {
        piece: None,
        reason: format!("bulged polyline bounds: {message}"),
    }
}

// Intervals enclose REAL arithmetic on the supplied binary64 endpoints/bulge.
// Nonzero subnormal inputs/intermediates and infinite outward endpoints refuse.
// This deliberately conservative admission is not a tolerance or ULP-accuracy
// promise for any transcendental function or the old reconstructed Arc sampler.
#[derive(Clone, Copy)]
struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    fn checked(lo: f64, hi: f64) -> Result<Self> {
        if lo > hi
            || !lo.is_finite()
            || !hi.is_finite()
            || (lo != 0.0 && !lo.is_normal())
            || (hi != 0.0 && !hi.is_normal())
        {
            return Err(invalid(
                "nonfinite or subnormal support interval is not representable",
            ));
        }
        Ok(Self { lo, hi })
    }

    fn exact(value: f64) -> Result<Self> {
        Self::checked(value, value)
    }

    fn rounded(value: f64) -> Result<Self> {
        if value == 0.0 || !value.is_normal() {
            return Err(invalid("support arithmetic overflow or underflow"));
        }
        Self::checked(-next_up(-value), next_up(value))
    }

    fn neg(self) -> Self {
        Self {
            lo: -self.hi,
            hi: -self.lo,
        }
    }

    fn sum(a: f64, b: f64) -> Result<Self> {
        if a == 0.0 {
            return Self::exact(b);
        }
        if b == 0.0 {
            return Self::exact(a);
        }
        if a == -b {
            return Self::exact(0.0);
        }
        Self::rounded(a + b)
    }

    fn product(a: f64, b: f64) -> Result<Self> {
        if a == 0.0 || b == 0.0 {
            return Self::exact(0.0);
        }
        if a.abs() == 1.0 {
            return Self::exact(if a < 0.0 { -b } else { b });
        }
        if b.abs() == 1.0 {
            return Self::exact(if b < 0.0 { -a } else { a });
        }
        Self::rounded(a * b)
    }

    fn quotient(a: f64, b: f64) -> Result<Self> {
        if b == 0.0 {
            return Err(invalid("support divisor contains zero"));
        }
        if a == 0.0 {
            return Self::exact(0.0);
        }
        if b.abs() == 1.0 {
            return Self::exact(if b < 0.0 { -a } else { a });
        }
        Self::rounded(a / b)
    }

    fn hull(self, other: Self) -> Result<Self> {
        Self::checked(self.lo.min(other.lo), self.hi.max(other.hi))
    }

    fn add(self, rhs: Self) -> Result<Self> {
        let lo = Self::sum(self.lo, rhs.lo)?.lo;
        let hi = Self::sum(self.hi, rhs.hi)?.hi;
        Self::checked(lo, hi)
    }

    fn sub(self, rhs: Self) -> Result<Self> {
        self.add(rhs.neg())
    }

    fn mul(self, rhs: Self) -> Result<Self> {
        Self::product(self.lo, rhs.lo)?
            .hull(Self::product(self.lo, rhs.hi)?)?
            .hull(Self::product(self.hi, rhs.lo)?)?
            .hull(Self::product(self.hi, rhs.hi)?)
    }

    fn div(self, rhs: Self) -> Result<Self> {
        if rhs.lo <= 0.0 && rhs.hi >= 0.0 {
            return Err(invalid("support divisor contains zero"));
        }
        Self::quotient(self.lo, rhs.lo)?
            .hull(Self::quotient(self.lo, rhs.hi)?)?
            .hull(Self::quotient(self.hi, rhs.lo)?)?
            .hull(Self::quotient(self.hi, rhs.hi)?)
    }

    fn abs(self) -> Result<Self> {
        let hi = self.lo.abs().max(self.hi.abs());
        let lo = if self.lo <= 0.0 && self.hi >= 0.0 {
            0.0
        } else {
            self.lo.abs().min(self.hi.abs())
        };
        Self::checked(lo, hi)
    }
}

fn supporting_arc(from: DxfVertex, to: Vec2) -> Result<DxfGeometryBounds> {
    let (x0, y0) = (Interval::exact(from.at.x)?, Interval::exact(from.at.y)?);
    let (x1, y1) = (Interval::exact(to.x)?, Interval::exact(to.y)?);
    let b = Interval::exact(from.bulge)?;
    let b2 = b.mul(b)?;
    let one = Interval::exact(1.0)?;
    let two = Interval::exact(2.0)?;
    let four = Interval::exact(4.0)?;
    let (dx, dy) = (x1.sub(x0)?, y1.sub(y0)?);
    // Centre = midpoint + perp(chord/(4b)) * (1-b²).
    // Dividing the chord components first avoids an unbounded standalone1/b.
    let divisor = four.mul(b)?;
    let (qx, qy) = (dx.div(divisor)?, dy.div(divisor)?);
    let factor = one.sub(b2)?;
    let cx = x0.div(two)?.add(x1.div(two)?)?.sub(qy.mul(factor)?)?;
    let cy = y0.div(two)?.add(y1.div(two)?)?.add(qx.mul(factor)?)?;
    // r = ||chord/(4b)||2 * (1+b²) <= its L1 analogue.
    // All terms are enclosed before selecting the upper radius, so no assumed
    // libm rounding bound or single-final-ULP shortcut is hidden here.
    let radius = qx.abs()?.add(qy.abs()?)?.mul(one.add(b2)?)?.hi;
    if radius <= 0.0 {
        return Err(invalid("support radius has collapsed"));
    }
    let radius = Interval::exact(radius)?;
    let x = cx.sub(radius)?.hull(cx.add(radius)?)?;
    let y = cy.sub(radius)?.hull(cy.add(radius)?)?;
    DxfGeometryBounds::new(Vec2::new(x.lo, y.lo), Vec2::new(x.hi, y.hi))
}

fn edge(from: DxfVertex, to: Vec2) -> Result<DxfGeometryBounds> {
    if from.at == to && from.bulge != 0.0 {
        return Err(invalid("zero chord cannot carry a nonzero bulge"));
    }
    let endpoints = DxfGeometryBounds::new(
        Vec2::new(from.at.x.min(to.x), from.at.y.min(to.y)),
        Vec2::new(from.at.x.max(to.x), from.at.y.max(to.y)),
    )?;
    if from.bulge == 0.0 {
        return Ok(endpoints);
    }
    let support = supporting_arc(from, to)?;
    // Include stored endpoints explicitly even for an extremely wide support
    // interval; neither a support reconstruction nor a hull may discard them.
    Ok(union(endpoints, support))
}

fn union(a: DxfGeometryBounds, b: DxfGeometryBounds) -> DxfGeometryBounds {
    DxfGeometryBounds {
        min: Vec2::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
        max: Vec2::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
    }
}

/// New internal proposal: validate/bound the real X1 vertices, without copying.
/// `max_vertices/max_work` may tighten hard limits only. Work is len +512*edges,
/// checked BEFORE scanning values. Caller must charge that same deterministic
/// cost cumulatively across the file; passing a fresh per-entity cap is not a
/// whole-file budget. No hidden caller or model/variant adoption is assumed.
pub fn bounds(
    vertices: &[DxfVertex],
    closed: bool,
    max_vertices: usize,
    max_work: usize,
) -> Result<DxfGeometryBounds> {
    let count = vertices.len();
    if !(2..=max_vertices.min(MAX_PATH_ITEMS)).contains(&count) {
        return Err(invalid(
            "vertex count exceeds admission or needs at least two vertices",
        ));
    }
    let edges = if closed { count } else { count - 1 };
    let work = edges
        .checked_mul(EDGE_WORK)
        .and_then(|cost| cost.checked_add(count))
        .ok_or_else(|| invalid("work count overflow"))?;
    if work > max_work.min(MAX_WORK) {
        return Err(invalid("work budget exceeded before vertex scan"));
    }
    if vertices
        .iter()
        .any(|v| !v.at.is_finite() || !v.bulge.is_finite() || v.bulge.abs() > 1.0)
    {
        return Err(invalid("nonfinite vertex/bulge or more than a half turn"));
    }
    if !closed && vertices.last().is_some_and(|v| v.bulge != 0.0) {
        return Err(invalid("open terminal bulge must be zero"));
    }
    // Zero-bulge coincident points are valid authored LINE geometry, but must
    // not silently admit the duplicated terminal vertex of a closed polyline.
    if closed
        && vertices
            .first()
            .zip(vertices.last())
            .is_some_and(|(first, last)| first.at == last.at)
    {
        return Err(invalid(
            "closed polyline must not duplicate its first vertex at the seam",
        ));
    }
    let mut pairs = vertices.iter().zip(vertices.iter().skip(1));
    let (first, second) = pairs
        .next()
        .ok_or_else(|| invalid("missing first segment"))?;
    let mut total = edge(*first, second.at)?;
    for (from, to) in pairs {
        total = union(total, edge(*from, to.at)?);
    }
    if closed {
        let (last, first) = vertices
            .last()
            .zip(vertices.first())
            .ok_or_else(|| invalid("missing closed seam"))?;
        total = union(total, edge(*last, first.at)?);
    }
    Ok(total)
}
