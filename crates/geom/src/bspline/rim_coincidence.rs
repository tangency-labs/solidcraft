//! Continuous distance bounds between fitted rational rim restrictions.
use super::{CurveN, MAX_SPLINE_DEGREE, SplineError, SplineResult, invalid};

const MAX_PIECES: usize = 512;
const PROOF_MAX_DEGREE: usize = 16;
const MAX_DEPTH: usize = 12;

#[derive(Clone, Copy)]
struct Interval {
    lo: f64,
    hi: f64,
}
impl Interval {
    const ZERO: Self = Self::at(0.0);
    const WHOLE: Self = Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY };

    const fn at(x: f64) -> Self {
        Self { lo: x, hi: x }
    }
    fn finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }
    fn rounded(lo: f64, hi: f64) -> Self {
        let interval = Self { lo: lo.next_down(), hi: hi.next_up() };
        if interval.finite() { interval } else { Self::WHOLE }
    }
    fn add(self, b: Self) -> Self {
        Self::rounded(self.lo + b.lo, self.hi + b.hi)
    }
    fn sub(self, b: Self) -> Self {
        if self.finite() && self.lo == self.hi && self.lo == b.lo && b.lo == b.hi {
            Self::ZERO
        } else {
            Self::rounded(self.lo - b.hi, self.hi - b.lo)
        }
    }
    fn mul(self, b: Self) -> Self {
        let products = [self.lo * b.lo, self.lo * b.hi, self.hi * b.lo, self.hi * b.hi];
        if products.iter().any(|x| !x.is_finite()) {
            return Self::WHOLE;
        }
        Self::rounded(products.into_iter().fold(f64::INFINITY, f64::min), products.into_iter().fold(f64::NEG_INFINITY, f64::max))
    }
    fn div(self, b: Self) -> Self {
        if !b.finite() || b.lo <= 0.0 {
            return Self::WHOLE;
        }
        self.mul(Self::rounded(1.0 / b.hi, 1.0 / b.lo))
    }
    fn unit(self) -> SplineResult<Self> {
        let clipped = Self { lo: self.lo.max(0.0), hi: self.hi.min(1.0) };
        if self.finite() && clipped.finite() { Ok(clipped) } else { invalid("unresolved rational rim parameter interval") }
    }
    fn square(self) -> Self {
        if !self.finite() {
            return Self::WHOLE;
        }
        let low = if self.lo <= 0.0 && self.hi >= 0.0 { 0.0 } else { self.lo.abs().min(self.hi.abs()) };
        let high = self.lo.abs().max(self.hi.abs());
        Self::rounded(low * low, high * high)
    }
}

type Coefficient = [Interval; 4];

struct Span<'a> {
    start: Interval,
    end: Interval,
    width: Interval,
    reversed: bool,
    controls: Controls<'a>,
}

enum Controls<'a> {
    Fitted(&'a [[f64; 4]]),
    Native(Vec<Coefficient>),
}

fn charge(work: &mut usize, debit: usize) -> SplineResult<()> {
    *work = work.checked_sub(debit).ok_or_else(|| SplineError::TooLarge("rational rim coincidence work budget exceeded".into()))?;
    Ok(())
}

fn spans<'a>(pieces: &'a [([f64; 2], CurveN<4>)], reverse: bool, work: &mut usize) -> SplineResult<Vec<Span<'a>>> {
    if pieces.is_empty() || pieces.len() > MAX_PIECES {
        return invalid("rational rim requires a bounded complete piece sequence");
    }
    charge(work, pieces.len() * (1 + pieces.len().ilog2() as usize))?;
    let mut pieces: Vec<_> = pieces.iter().collect();
    pieces.sort_by(|(a, _), (b, _)| a[0].total_cmp(&b[0]));
    let mut result = Vec::new();
    let mut previous = 0.0;
    for ([a, b], curve) in pieces {
        let degree = curve.degree;
        let count = curve.ctrl.len();
        if *a != previous
            || !(*a < *b && *b <= 1.0)
            || !(1..=MAX_SPLINE_DEGREE.min(PROOF_MAX_DEGREE)).contains(&degree)
            || count <= degree
            || count > MAX_PIECES * degree + 1
            || !(count - 1).is_multiple_of(degree)
            || curve.knots.len() != count + degree + 1
        {
            return invalid("rational rim pieces must partition the unit side with bounded Bezier spans");
        }
        previous = *b;
        charge(work, count + curve.knots.len())?;
        let k = &curve.knots;
        if k[..=degree].iter().any(|&x| x != 0.0)
            || k[count..].iter().any(|&x| x != 1.0)
            || curve.ctrl.iter().any(|p| p.iter().any(|x| !x.is_finite()) || p[3] <= 0.0)
        {
            return invalid("rational rim needs finite positive-weight clamped controls");
        }
        let span_count = (count - 1) / degree;
        if result.len() + span_count > MAX_PIECES {
            return Err(SplineError::TooLarge("too many rational rim spans".into()));
        }
        let scale = Interval::at(*b).sub(Interval::at(*a));
        let absolute = |t: f64| {
            if t == 0.0 {
                Interval::at(*a)
            } else if t == 1.0 {
                Interval::at(*b)
            } else {
                Interval::at(*a).add(scale.mul(Interval::at(t)))
            }
        };
        let mut start_knot = 0.0;
        for i in 0..span_count {
            let end_knot = k[(i + 1) * degree + 1];
            let end_group = &k[(i + 1) * degree + 1..=(i + 2) * degree];
            if !(start_knot < end_knot && end_knot <= 1.0) || end_group.iter().any(|&x| x != end_knot) {
                return invalid("rational rim has a non-Bezier knot span");
            }
            let width = scale.mul(Interval::at(end_knot).sub(Interval::at(start_knot)));
            if !width.finite() || width.lo <= 0.0 {
                return invalid("rational rim span width cannot be bounded");
            }
            let (start, end) = (absolute(start_knot), absolute(end_knot));
            let (start, end) = if reverse { (Interval::at(1.0).sub(end), Interval::at(1.0).sub(start)) } else { (start, end) };
            let controls = Controls::Fitted(&curve.ctrl[i * degree..=(i + 1) * degree]);
            result.push(Span { start, end, width, reversed: reverse, controls });
            start_knot = end_knot;
        }
    }
    if previous != 1.0 {
        return invalid("rational rim piece sequence is incomplete");
    }
    Ok(result)
}

fn native_spans(curve: &CurveN<4>, reverse: bool, work: &mut usize) -> SplineResult<Vec<Span<'static>>> {
    let p = curve.degree;
    let n = curve.ctrl.len();
    if !(1..=MAX_SPLINE_DEGREE.min(PROOF_MAX_DEGREE)).contains(&p) || n <= p || n > MAX_PIECES || curve.knots.len() != n + p + 1 {
        return invalid("native rim requires a bounded clamped spline");
    }
    charge(work, n + curve.knots.len())?;
    if curve.knots[..=p].iter().any(|&t| t != 0.0)
        || curve.knots[n..].iter().any(|&t| t != 1.0)
        || curve.knots[p + 1..n].iter().any(|&t| !(0.0 < t && t < 1.0))
        || curve.knots.windows(2).any(|t| t[0] > t[1])
        || curve.ctrl.iter().any(|c| c.iter().any(|x| !x.is_finite()) || c[3] <= 0.0)
    {
        return invalid("native rim requires finite positive-weight controls and clamped unit knots");
    }
    let mut knots = curve.knots.clone();
    let mut controls: Vec<_> = curve.ctrl.iter().map(|point| point.map(Interval::at)).collect();
    let mut breaks = vec![0.0];
    for group in curve.knots[p + 1..n].chunk_by(|a, b| a == b) {
        let &u = group.first().ok_or_else(|| SplineError::Invalid("empty native rim knot group".into()))?;
        if group.len() > p {
            return invalid("native rim is discontinuous");
        }
        breaks.push(u);
        for _ in group.len()..p {
            if controls.len() > MAX_PIECES * p {
                return Err(SplineError::TooLarge("native rim spline packing budget exceeded".into()));
            }
            insert_native_knot(p, u, &mut knots, &mut controls, work)?;
        }
    }
    breaks.push(1.0);
    if controls.len() != (breaks.len() - 1) * p + 1 {
        return invalid("native rim did not produce complete Bezier spans");
    }
    breaks
        .windows(2)
        .enumerate()
        .map(|(i, range)| {
            let [a, b] = [Interval::at(range[0]), Interval::at(range[1])];
            let width = b.sub(a);
            if !width.finite() || width.lo <= 0.0 {
                return invalid("native rim span width cannot be bounded");
            }
            let (start, end) = if reverse { (Interval::at(1.0).sub(b), Interval::at(1.0).sub(a)) } else { (a, b) };
            charge(work, p + 1)?;
            let row = controls.get(i * p..=(i + 1) * p).ok_or_else(|| SplineError::Invalid("native rim lost a Bezier span".into()))?;
            Ok(Span { start, end, width, reversed: reverse, controls: Controls::Native(row.to_vec()) })
        })
        .collect()
}

fn insert_native_knot(degree: usize, u: f64, knots: &mut Vec<f64>, controls: &mut Vec<Coefficient>, work: &mut usize) -> SplineResult<()> {
    charge(work, 16 * (controls.len() + 1) + knots.len())?;
    let k = knots.partition_point(|&t| t <= u).checked_sub(1).ok_or_else(|| SplineError::Invalid("missing native rim knot span".into()))?;
    let invalid_row = || SplineError::Invalid("missing native rim control or knot".into());
    let mut next = Vec::with_capacity(controls.len() + 1);
    for i in 0..=controls.len() {
        let point = if i + degree <= k {
            *controls.get(i).ok_or_else(invalid_row)?
        } else if i > k {
            *controls.get(i - 1).ok_or_else(invalid_row)?
        } else {
            let lo = Interval::at(*knots.get(i).ok_or_else(invalid_row)?);
            let hi = Interval::at(*knots.get(i + degree).ok_or_else(invalid_row)?);
            let alpha = Interval::at(u).sub(lo).div(hi.sub(lo)).unit()?;
            let beta = Interval::at(1.0).sub(alpha).unit()?;
            let a = controls.get(i - 1).ok_or_else(invalid_row)?;
            let b = controls.get(i).ok_or_else(invalid_row)?;
            std::array::from_fn(|d| a[d].mul(beta).add(b[d].mul(alpha)))
        };
        next.push(point);
    }
    knots.insert(k + 1, u);
    *controls = next;
    Ok(())
}

fn split(points: &[Coefficient], t: Interval, work: &mut usize) -> SplineResult<(Vec<Coefficient>, Vec<Coefficient>)> {
    let count = points.len();
    charge(work, 8 * count * count)?;
    let mut row = points.to_vec();
    let mut left = Vec::with_capacity(count);
    let mut right = Vec::with_capacity(count);
    let other = Interval::at(1.0).sub(t);
    while let (Some(&a), Some(&b)) = (row.first(), row.last()) {
        left.push(a);
        right.push(b);
        for i in 0..row.len() - 1 {
            row[i] = std::array::from_fn(|axis| row[i][axis].mul(other).add(row[i + 1][axis].mul(t)));
        }
        row.pop();
    }
    right.reverse();
    Ok((left, right))
}

fn restrict(span: &Span<'_>, start: Interval, end: Interval, work: &mut usize) -> SplineResult<Vec<Coefficient>> {
    let a = start.sub(span.start).div(span.width).unit()?;
    let b = end.sub(span.start).div(span.width).unit()?;
    let mut points: Vec<_> = match &span.controls {
        Controls::Fitted(controls) => {
            charge(work, controls.len())?;
            controls.iter().map(|p| p.map(Interval::at)).collect()
        }
        Controls::Native(controls) => {
            charge(work, controls.len())?;
            controls.clone()
        }
    };
    if span.reversed {
        points.reverse();
    }
    // Either affine subdivision sequence represents the same restriction.
    // Choose the ratio with a well-separated denominator even at a shared
    // endpoint where the actual overlap is a singleton.
    if b.lo >= 0.5 {
        let (left, _) = split(&points, b, work)?;
        Ok(split(&left, a.div(b).unit()?, work)?.1)
    } else if a.hi < 1.0 {
        let (_, right) = split(&points, a, work)?;
        let t = b.sub(a).div(Interval::at(1.0).sub(a)).unit()?;
        Ok(split(&right, t, work)?.0)
    } else {
        invalid("rational rim overlap cannot be restricted")
    }
}

fn choose(n: usize, k: usize) -> f64 {
    // Degrees are bounded by 16; n <= 32 and all these integers are
    // represented exactly as f64.
    (1..=k).fold(1u64, |a, i| a * (n + 1 - i) as u64 / i as u64) as f64
}

fn difference(a: &[Coefficient], b: &[Coefficient], work: &mut usize) -> SplineResult<Vec<Coefficient>> {
    let (p, q) = (a.len() - 1, b.len() - 1);
    charge(work, 16 * (p + 1) * (q + 1) + 4 * (p + q + 1) * (p + q + 1))?;
    let weights: Vec<_> = (0..=p + q).map(|k| Interval::at(choose(p + q, k))).collect();
    let first: Vec<_> = (0..=p).map(|i| choose(p, i)).collect();
    let second: Vec<_> = (0..=q).map(|i| choose(q, i)).collect();
    let mut difference = vec![[Interval::ZERO; 4]; p + q + 1];
    for (i, a) in a.iter().enumerate() {
        for (j, b) in b.iter().enumerate() {
            let k = i + j;
            let scale = Interval::at(first[i] * second[j]).div(weights[k]);
            let coefficient =
                [a[0].mul(b[3]).sub(b[0].mul(a[3])), a[1].mul(b[3]).sub(b[1].mul(a[3])), a[2].mul(b[3]).sub(b[2].mul(a[3])), a[3].mul(b[3])];
            for (output, value) in difference[k].iter_mut().zip(coefficient) {
                *output = output.add(value.mul(scale));
            }
        }
    }
    Ok(difference)
}

fn distance(point: Coefficient) -> SplineResult<Interval> {
    let [x, y, z, w] = point;
    let square = [x, y, z].into_iter().fold(Interval::ZERO, |sum, component| sum.add(component.div(w).square()));
    if !square.finite() {
        return invalid("rational rim distance overflowed or lost positive weight");
    }
    Ok(Interval::rounded(square.lo.max(0.0).sqrt(), square.hi.max(0.0).sqrt()))
}

fn qualify(points: &[Coefficient], tolerance: f64, depth: usize, work: &mut usize) -> SplineResult<Option<f64>> {
    charge(work, 16 * points.len())?;
    let mut upper = 0.0_f64;
    for (i, &point) in points.iter().enumerate() {
        let gap = distance(point)?;
        if (i == 0 || i + 1 == points.len()) && gap.lo > tolerance {
            return Ok(None);
        }
        upper = upper.max(gap.hi);
    }
    if upper <= tolerance {
        return Ok(Some(upper));
    }
    if depth == MAX_DEPTH {
        return Ok(None);
    }
    let (left, right) = split(points, Interval::at(0.5), work)?;
    let Some(a) = qualify(&left, tolerance, depth + 1, work)? else { return Ok(None) };
    Ok(qualify(&right, tolerance, depth + 1, work)?.map(|b| a.max(b)))
}

/// Certify the maximum Euclidean gap of two fitted rational rim restrictions
/// over a complete common unit parameter. Each flag reverses one input's
/// parameter and coefficients. The caller supplies the topological pairing.
/// `None` means coincidence could not be proved within the tolerance. Input
/// splines must be packed Bezier spans, as returned by `parameter_line`.
pub fn rational_rim_gap(
    first: &[([f64; 2], CurveN<4>)],
    second: &[([f64; 2], CurveN<4>)],
    reversed: [bool; 2],
    tolerance: f64,
    work: &mut usize,
) -> SplineResult<Option<f64>> {
    check_tolerance(tolerance)?;
    let a = spans(first, reversed[0], work)?;
    let b = spans(second, reversed[1], work)?;
    prove_gap(&a, &b, tolerance, work)
}

/// Certify a fitted rim against a native unit-domain B-spline. Interior
/// knot insertion is enclosed as well as the final curve comparison.
pub fn rational_rim_to_native_gap(
    fitted: &[([f64; 2], CurveN<4>)],
    native: &CurveN<4>,
    reversed: [bool; 2],
    tolerance: f64,
    work: &mut usize,
) -> SplineResult<Option<f64>> {
    check_tolerance(tolerance)?;
    let a = spans(fitted, reversed[0], work)?;
    let b = native_spans(native, reversed[1], work)?;
    prove_gap(&a, &b, tolerance, work)
}

fn check_tolerance(tolerance: f64) -> SplineResult<()> {
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > 1.0 {
        return invalid("rational rim tolerance must be finite and in (0,1] mm");
    }
    Ok(())
}

fn prove_gap(first: &[Span<'_>], second: &[Span<'_>], tolerance: f64, work: &mut usize) -> SplineResult<Option<f64>> {
    let mut upper = 0.0_f64;
    let mut overlap = false;
    for a in first {
        for b in second {
            charge(work, 1)?;
            let start = Interval { lo: a.start.lo.max(b.start.lo), hi: a.start.hi.max(b.start.hi) };
            let end = Interval { lo: a.end.lo.min(b.end.lo), hi: a.end.hi.min(b.end.hi) };
            if end.hi < start.lo {
                continue;
            }
            overlap = true;
            let a = restrict(a, start, end, work)?;
            let b = restrict(b, start, end, work)?;
            // Homogeneous difference (Xa*Wb-Xb*Wa)/(Wa*Wb). The ratios of
            // the resulting Bernstein controls bound the complete curve
            // because the product weights are positive.
            let product = difference(&a, &b, work)?;
            let Some(bound) = qualify(&product, tolerance, 0, work)? else { return Ok(None) };
            upper = upper.max(bound);
        }
    }
    if overlap { Ok(Some(upper)) } else { invalid("rational rim pieces have no common parameter") }
}
