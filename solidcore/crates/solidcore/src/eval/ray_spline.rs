//! Conservative knot insertion and trim parity for native ray queries.

// The record adapter assembles bounded grids. It shares these implementation
// details through solidcore-ops without making them modelling operations.
#![allow(missing_docs)]

use super::ray_tensor::{Control, Interval as I, ProofError, MAX_DEGREE};

pub const MAX_CONTROLS: usize = 16_384;
const MAX_KNOTS: usize = MAX_CONTROLS + MAX_DEGREE + 1;
pub const MAX_SPANS: usize = 4_096;
const MAX_TRIM_DEPTH: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplineError {
    InvalidSpline,
    Budget,
    Proof(ProofError),
}
impl From<ProofError> for SplineError {
    fn from(error: ProofError) -> Self {
        Self::Proof(error)
    }
}
type Result<T> = std::result::Result<T, SplineError>;

pub fn reserved<T>(count: usize) -> Result<Vec<T>> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| ProofError::Allocation)?;
    Ok(output)
}

pub struct Span {
    pub start: usize,
    pub range: [f64; 2],
}

pub struct Axis {
    degree: usize,
    knots: Vec<f64>,
}

impl Axis {
    pub fn degree(&self) -> usize {
        self.degree
    }

    pub fn new(knots: &[f64], count: usize, work: &mut usize) -> Result<Self> {
        let (degree, _) = Self::validate(knots, count, work)?;
        let mut owned = reserved(knots.len())?;
        owned.extend_from_slice(knots);
        Ok(Self {
            degree,
            knots: owned,
        })
    }

    pub fn validate(knots: &[f64], count: usize, work: &mut usize) -> Result<(usize, [f64; 2])> {
        if count > MAX_CONTROLS || knots.len() > MAX_KNOTS {
            return Err(SplineError::Budget);
        }
        super::ray_tensor::spend(work, knots.len())?;
        let degree = knots
            .len()
            .checked_sub(count + 1)
            .filter(|n| (1..=MAX_DEGREE).contains(n))
            .ok_or(SplineError::InvalidSpline)?;
        if count <= degree
            || knots.iter().any(|k| !k.is_finite())
            || knots.windows(2).any(|k| matches!(k, [a, b] if a > b))
        {
            return Err(SplineError::InvalidSpline);
        }
        let lo = *knots.get(degree).ok_or(SplineError::InvalidSpline)?;
        let hi = *knots.get(count).ok_or(SplineError::InvalidSpline)?;
        if lo >= hi
            || knots
                .get(..=degree)
                .ok_or(SplineError::InvalidSpline)?
                .iter()
                .any(|&k| k != lo)
            || knots
                .get(count..)
                .ok_or(SplineError::InvalidSpline)?
                .iter()
                .any(|&k| k != hi)
        {
            return Err(SplineError::InvalidSpline);
        }
        for run in knots.chunk_by(|a, b| a == b) {
            let first = *run.first().ok_or(SplineError::InvalidSpline)?;
            let limit = degree + usize::from(first == lo || first == hi);
            if run.len() > limit {
                return Err(SplineError::InvalidSpline);
            }
        }
        Ok((degree, [lo, hi]))
    }

    pub fn domain(&self) -> Result<[f64; 2]> {
        Ok([
            *self.knots.first().ok_or(SplineError::InvalidSpline)?,
            *self.knots.last().ok_or(SplineError::InvalidSpline)?,
        ])
    }

    pub fn spans(
        &mut self,
        grid: &mut Grid,
        axis: usize,
        domain: [f64; 2],
        work: &mut usize,
    ) -> Result<Vec<Span>> {
        let count = *grid.size.get(axis).ok_or(SplineError::InvalidSpline)?;
        if count <= self.degree || self.knots.len() != count + self.degree + 1 {
            return Err(SplineError::InvalidSpline);
        }
        let [lo, hi] = self.domain()?;
        if !(lo <= domain[0] && domain[0] < domain[1] && domain[1] <= hi) {
            return Err(SplineError::InvalidSpline);
        }
        super::ray_tensor::spend(work, self.knots.len())?;
        let mut values = reserved(self.knots.len() + 2)?;
        values.push(domain[0]);
        values.extend(
            self.knots
                .iter()
                .copied()
                .filter(|&t| domain[0] < t && t < domain[1]),
        );
        values.push(domain[1]);
        values.dedup();
        // Insertion at each retained span boundary extracts exactly the piece
        // inside the raw domain. It does not round the requested domain endpoint.
        for value in values.into_iter().filter(|&t| lo < t && t < hi) {
            let mut multiplicity = self.knots.partition_point(|&k| k <= value)
                - self.knots.partition_point(|&k| k < value);
            while multiplicity < self.degree {
                grid.insert(self, axis, value, work)?;
                multiplicity += 1;
            }
        }
        super::ray_tensor::spend(work, self.knots.len())?;
        let mut spans = reserved(self.knots.len().min(MAX_SPANS))?;
        for (i, w) in self
            .knots
            .windows(2)
            .enumerate()
            .skip(self.degree)
            .take(*grid.size.get(axis).ok_or(SplineError::InvalidSpline)? - self.degree)
        {
            let &[a, b] = w else {
                return Err(SplineError::InvalidSpline);
            };
            if domain[0] <= a && a < b && b <= domain[1] {
                if spans.len() >= MAX_SPANS {
                    return Err(SplineError::Budget);
                }
                spans.push(Span {
                    start: i - self.degree,
                    range: [a, b],
                });
            }
        }
        if spans.first().is_none_or(|span| span.range[0] != domain[0])
            || spans.last().is_none_or(|span| span.range[1] != domain[1])
            || spans
                .windows(2)
                .any(|p| matches!(p, [a, b] if a.range[1] != b.range[0]))
        {
            return Err(SplineError::InvalidSpline);
        }
        Ok(spans)
    }
}

pub struct Grid {
    size: [usize; 2],
    controls: Vec<Control>,
}

impl Grid {
    pub fn new(size: [usize; 2], controls: Vec<Control>) -> Result<Self> {
        let count = size[0].checked_mul(size[1]).ok_or(SplineError::Budget)?;
        if count == 0 || count != controls.len() || count > MAX_CONTROLS {
            return Err(SplineError::InvalidSpline);
        }
        Ok(Self { size, controls })
    }

    pub fn get(&self, at: [usize; 2]) -> Result<Control> {
        if at.iter().zip(self.size).any(|(&i, count)| i >= count) {
            return Err(SplineError::InvalidSpline);
        }
        self.controls
            .get(at[1] * self.size[0] + at[0])
            .copied()
            .ok_or(SplineError::InvalidSpline)
    }

    pub fn copy_span(
        &self,
        start: [usize; 2],
        size: [usize; 2],
        target: &mut [Control],
    ) -> Result<()> {
        if size.contains(&0)
            || size[0].checked_mul(size[1]) != Some(target.len())
            || start
                .into_iter()
                .zip(size)
                .zip(self.size)
                .any(|((start, count), limit)| {
                    start.checked_add(count).is_none_or(|end| end > limit)
                })
        {
            return Err(SplineError::InvalidSpline);
        }
        for (i, out) in target.iter_mut().enumerate() {
            *out = self.get([start[0] + i % size[0], start[1] + i / size[0]])?;
        }
        Ok(())
    }

    fn insert(
        &mut self,
        axis: &mut Axis,
        direction: usize,
        knot: f64,
        work: &mut usize,
    ) -> Result<()> {
        let mut size = self.size;
        let size_axis = size.get_mut(direction).ok_or(SplineError::InvalidSpline)?;
        *size_axis = size_axis.checked_add(1).ok_or(SplineError::Budget)?;
        let count = size[0] * size[1];
        if count > MAX_CONTROLS || axis.knots.len() >= MAX_KNOTS {
            return Err(SplineError::Budget);
        }
        super::ray_tensor::spend(work, 12 * count + axis.knots.len())?;
        let mut controls = super::ray_tensor::allocate(count)?;
        let span = axis
            .knots
            .partition_point(|&k| k <= knot)
            .checked_sub(1)
            .ok_or(SplineError::InvalidSpline)?;
        let degree = axis.degree;
        let mut alphas = [I::ZERO; MAX_DEGREE];
        let first = span + 1 - degree;
        for (i, a) in (first..=span).zip(&mut alphas) {
            let lo = *axis.knots.get(i).ok_or(SplineError::InvalidSpline)?;
            let hi = *axis
                .knots
                .get(i + degree)
                .ok_or(SplineError::InvalidSpline)?;
            if !(lo <= knot && knot <= hi && lo < hi) {
                return Err(SplineError::InvalidSpline);
            }
            *a = if knot == lo || knot == hi {
                I::point(f64::from(knot == hi))
            } else {
                let num = I::point(knot).sub(I::point(lo));
                let den = I::point(hi).sub(I::point(lo));
                num.div(den)
                    .intersect(I::UNIT)
                    .ok_or(SplineError::InvalidSpline)?
            };
        }
        for (index, target) in controls.iter_mut().enumerate() {
            let at = [index % size[0], index / size[0]];
            let i = *at.get(direction).ok_or(SplineError::InvalidSpline)?;
            if i + degree <= span {
                *target = self.get(at)?;
                continue;
            }
            let mut previous = at;
            *previous
                .get_mut(direction)
                .ok_or(SplineError::InvalidSpline)? =
                i.checked_sub(1).ok_or(SplineError::InvalidSpline)?;
            let before = self.get(previous)?;
            *target = if i > span {
                before
            } else {
                let after = self.get(at)?;
                let alpha = *alphas.get(i - first).ok_or(SplineError::InvalidSpline)?;
                let beta = I::point(1.0).sub(alpha);
                let mut blended = [I::ZERO; 4];
                for (out, (before, after)) in blended.iter_mut().zip(before.into_iter().zip(after))
                {
                    // Unit weights and equal coordinates are fixed by this
                    // convex blend, even if the knot ratio is unrepresentable.
                    let hull = before.hull(after);
                    *out = before
                        .mul(beta)
                        .add(after.mul(alpha))
                        .intersect(hull)
                        .unwrap_or(hull);
                }
                blended
            };
        }
        axis.knots
            .try_reserve(1)
            .map_err(|_| ProofError::Allocation)?;
        axis.knots.insert(span + 1, knot);
        self.size = size;
        self.controls = controls;
        Ok(())
    }
}

pub fn trim_parity(
    controls: &[Control],
    query: [I; 2],
    at: [f64; 2],
    x: usize,
    depth: usize,
    work: &mut usize,
) -> Result<Option<bool>> {
    if x > 1 {
        return Err(SplineError::InvalidSpline);
    }
    super::ray_tensor::spend(work, 8 * controls.len())?;
    let (Some(first), Some(last)) = (controls.first(), controls.last()) else {
        return Err(SplineError::InvalidSpline);
    };
    let bounds = controls.iter().fold(
        [first[0].div(first[3]), first[1].div(first[3])],
        |[u, v], &[x, y, _, w]| [u.hull(x.div(w)), v.hull(y.div(w))],
    );
    if bounds.iter().any(|b| !b.is_finite()) {
        return Ok(None);
    }
    if bounds
        .iter()
        .zip(query)
        .any(|(a, b)| a.intersect(b).is_none())
    {
        let y = 1usize.checked_sub(x).ok_or(SplineError::InvalidSpline)?;
        let (bx, by, ax, ay) = (
            *bounds.get(x).ok_or(SplineError::InvalidSpline)?,
            *bounds.get(y).ok_or(SplineError::InvalidSpline)?,
            *at.get(x).ok_or(SplineError::InvalidSpline)?,
            *at.get(y).ok_or(SplineError::InvalidSpline)?,
        );
        if bx.hi < ax || by.hi <= ay || by.lo > ay {
            return Ok(Some(false));
        }
        if bx.lo > ax {
            // The complete positive-weight path is in the right half-plane.
            // There it can be deformed into its endpoint chord without crossing
            // the probe, so its parity is the chord's half-open crossing parity.
            let above = |p: &Control| {
                let t = p.get(y)?.div(p[3]);
                if t.lo > ay {
                    Some(true)
                } else if t.hi <= ay {
                    Some(false)
                } else {
                    None
                }
            };
            if let (Some(a), Some(b)) = (above(first), above(last)) {
                return Ok(Some(a != b));
            }
        }
    }
    if depth >= MAX_TRIM_DEPTH {
        return Ok(None);
    }
    let [a, b] = super::ray_tensor::halve_curve(controls, work)?;
    let Some(left) = trim_parity(&a, query, at, x, depth + 1, work)? else {
        return Ok(None);
    };
    Ok(trim_parity(&b, query, at, x, depth + 1, work)?.map(|right| left ^ right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_trim_axes_and_grid_directions_are_checked() {
        let controls = vec![[I::point(1.0); 4]; 4];
        assert!(matches!(
            trim_parity(&controls, [I::UNIT; 2], [0.; 2], usize::MAX, 0, &mut 1000),
            Err(SplineError::InvalidSpline)
        ));
        let mut grid = Grid::new([2, 2], controls).unwrap();
        let mut axis = Axis::new(&[0., 0., 1., 1.], 2, &mut 1000).unwrap();
        assert!(matches!(
            axis.spans(&mut grid, usize::MAX, [0., 1.], &mut 1000),
            Err(SplineError::InvalidSpline)
        ));
        assert!(matches!(
            grid.copy_span([usize::MAX, 0], [1, 1], &mut [[I::ZERO; 4]]),
            Err(SplineError::InvalidSpline)
        ));
    }
}
