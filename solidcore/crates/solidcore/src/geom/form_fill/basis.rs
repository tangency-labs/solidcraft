//! Fixed tensor weights and one-sided rational derivative rows for guided fitting.
//! Knots and weights are validated once; applying a row visits only local controls.
use crate::eval::plan::basis::{binomial, local_basis_array, Side};
use crate::eval::vec::VecExt as _;
use crate::{Result, SolidError, Vec3};

const OP: &str = "geom::form_fill::basis";
const MAX_AXIS: usize = 2048;
const MAX_POINTS: usize = 1 << 20;
const MAX_ORDER: usize = 5;
type Table = [[f64; MAX_ORDER + 1]; MAX_ORDER + 1];
/// The U index, V index and scalar coefficient of a tensor derivative row.
pub type Term = (usize, usize, f64);

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// Validate a clamped guided-fit axis without copying its knots.
pub(super) fn validate_space(degree: usize, knots: &[f64]) -> Result<usize> {
    if !(1..=16).contains(&degree)
        || knots.len() > MAX_AXIS + 17
        || knots.iter().any(|k| !k.is_finite())
        || knots.windows(2).any(|w| matches!(w,[a,b] if a>b))
    {
        return Err(bad("invalid bounded tensor knot space"));
    }
    let count = knots
        .len()
        .checked_sub(degree + 1)
        .filter(|&n| degree < n && n <= MAX_AXIS)
        .ok_or_else(|| bad("invalid tensor count"))?;
    let lo = *knots.first().ok_or_else(|| bad("empty tensor knots"))?;
    let hi = *knots.last().ok_or_else(|| bad("empty tensor knots"))?;
    if lo >= hi
        || knots
            .get(..=degree)
            .ok_or_else(|| bad("tensor knot prefix"))?
            .iter()
            .any(|&k| k != lo)
        || knots
            .get(count..)
            .ok_or_else(|| bad("tensor knot suffix"))?
            .iter()
            .any(|&k| k != hi)
    {
        return Err(bad("tensor knots must be clamped"));
    }
    for run in knots.chunk_by(|a, b| a == b) {
        let first = *run.first().ok_or_else(|| bad("empty knot run"))?;
        if run.len() > degree + usize::from(first == lo || first == hi) {
            return Err(bad("invalid tensor knot multiplicity"));
        }
    }

    Ok(count)
}

#[derive(Clone, Debug)]
struct Axis {
    degree: usize,
    knots: Vec<f64>,
    count: usize,
}
impl Axis {
    fn new(degree: usize, knots: Vec<f64>) -> Result<Self> {
        let count = validate_space(degree, &knots)?;
        Ok(Self {
            degree,
            knots,
            count,
        })
    }
    fn basis(&self, t: f64, order: usize, left: bool) -> Result<(usize, Vec<f64>)> {
        if !t.is_finite() || order > MAX_ORDER {
            return Err(bad("invalid tensor differential request"));
        }
        let lo = *self
            .knots
            .first()
            .ok_or_else(|| bad("empty tensor knots"))?;
        let hi = *self.knots.last().ok_or_else(|| bad("empty tensor knots"))?;
        let t = t.max(lo).min(hi);
        let from_left = left && t > lo || t >= hi;
        let span = if from_left {
            self.knots.partition_point(|&k| k < t).checked_sub(1)
        } else {
            self.knots.partition_point(|&k| k <= t).checked_sub(1)
        }
        .filter(|&s| s >= self.degree && s < self.count)
        .ok_or_else(|| bad("no tensor span"))?;
        let first = span - self.degree;
        let side = if from_left { Side::Left } else { Side::Right };
        let values = (first..=span)
            .map(|i| {
                let knots = self
                    .knots
                    .get(i..i + self.degree + 2)
                    .ok_or_else(|| bad("incomplete local tensor knots"))?;
                local_basis_array(knots, t, side, order)
                    .get(order)
                    .copied()
                    .ok_or_else(|| bad("missing tensor derivative"))
            })
            .collect::<Result<Vec<_>>>()?;
        if values.iter().any(|v| !v.is_finite()) {
            return Err(bad("nonfinite tensor basis"));
        }
        Ok((first, values))
    }
}

/// Validated immutable basis shared by a guided patch's repeated coordinate solves.
#[derive(Clone, Debug)]
pub struct TensorBasis {
    axes: [Axis; 2],
    weights: Vec<f64>,
    rational: bool,
}
impl TensorBasis {
    /// Own bounded clamped axes and positive weights once, independently of coordinates.
    pub fn new(degree: [usize; 2], knots: [Vec<f64>; 2], weights: Vec<f64>) -> Result<Self> {
        let [u, v] = knots;
        let axes = [Axis::new(degree[0], u)?, Axis::new(degree[1], v)?];
        if axes[0]
            .count
            .checked_mul(axes[1].count)
            .filter(|&n| n <= MAX_POINTS)
            != Some(weights.len())
            || weights.iter().any(|&w| !w.is_finite() || w <= 0.)
        {
            return Err(bad("invalid fixed tensor weights"));
        }
        let rational = weights.iter().any(|&w| w != 1.);
        Ok(Self {
            axes,
            weights,
            rational,
        })
    }
    /// One fixed weight at its stable local coefficient identity.
    pub fn weight(&self, i: usize, j: usize) -> Option<f64> {
        if i >= self.axes[0].count || j >= self.axes[1].count {
            return None;
        }
        self.weights.get(j * self.axes[0].count + i).copied()
    }
    /// Tensor derivative row, with exact one-sided knot ownership and total order at most five.
    pub fn row(&self, uv: [f64; 2], orders: [usize; 2], left: [bool; 2]) -> Result<Vec<Term>> {
        let [du, dv] = orders;
        if du.checked_add(dv).is_none_or(|n| n > MAX_ORDER) {
            return Err(bad("tensor derivative order exceeds the guided-fit bound"));
        }
        if !self.rational {
            let (iu, bu) = self.axes[0].basis(uv[0], du, left[0])?;
            let (iv, bv) = self.axes[1].basis(uv[1], dv, left[1])?;
            let mut out = Vec::new();
            for (b, nv) in bv.iter().enumerate() {
                for (a, nu) in bu.iter().enumerate() {
                    out.push((iu + a, iv + b, nu * nv));
                }
            }
            return Ok(out);
        }
        let ders =
            |axis: &Axis, t: f64, top: usize, left: bool| -> Result<(usize, Vec<Vec<f64>>)> {
                let mut first = 0;
                let mut output = Vec::new();
                for order in 0..=top {
                    let (index, row) = axis.basis(t, order, left)?;
                    first = index;
                    output.push(row);
                }
                Ok((first, output))
            };
        let (iu, bu) = ders(&self.axes[0], uv[0], du, left[0])?;
        let (iv, bv) = ders(&self.axes[1], uv[1], dv, left[1])?;
        let width = (
            bu.first().map_or(0, Vec::len),
            bv.first().map_or(0, Vec::len),
        );
        let value = |rows: &[Vec<f64>], order: usize, i: usize| -> Result<f64> {
            rows.get(order)
                .and_then(|row| row.get(i))
                .copied()
                .ok_or_else(|| bad("incomplete tensor derivative table"))
        };
        let mut wd = [[0.; MAX_ORDER + 1]; MAX_ORDER + 1];
        for b in 0..width.1 {
            for a in 0..width.0 {
                let w = self
                    .weight(iu + a, iv + b)
                    .ok_or_else(|| bad("missing tensor weight"))?;
                for (k, row) in wd.iter_mut().enumerate().take(du + 1) {
                    for (l, out) in row.iter_mut().enumerate().take(dv + 1) {
                        *out += w * value(&bu, k, a)? * value(&bv, l, b)?;
                    }
                }
            }
        }
        let w0 = wd[0][0];
        if !w0.is_finite() || w0 <= 0. {
            return Err(bad("a patch's weights vanish"));
        }
        let mut out = Vec::new();
        for b in 0..width.1 {
            for a in 0..width.0 {
                let w = self
                    .weight(iu + a, iv + b)
                    .ok_or_else(|| bad("missing tensor weight"))?;
                let mut r = [[0.; MAX_ORDER + 1]; MAX_ORDER + 1];
                for total in 0..=du + dv {
                    for k in 0..=du.min(total) {
                        let l = total - k;
                        if l > dv {
                            continue;
                        }
                        let mut x = w * value(&bu, k, a)? * value(&bv, l, b)?;
                        for i in 0..=k {
                            for j in 0..=l {
                                if i == 0 && j == 0 {
                                    continue;
                                }
                                x -= binomial(k, i)
                                    * binomial(l, j)
                                    * table(&wd, i, j)?
                                    * table(&r, k - i, l - j)?;
                            }
                        }
                        *r.get_mut(k)
                            .and_then(|row| row.get_mut(l))
                            .ok_or_else(|| bad("tensor derivative table index"))? = x / w0;
                    }
                }
                let coefficient = table(&r, du, dv)?;
                if !coefficient.is_finite() {
                    return Err(bad("nonfinite tensor derivative row"));
                }
                out.push((iu + a, iv + b, coefficient));
            }
        }
        Ok(out)
    }
    /// Apply an exact row to mutable Cartesian solve coordinates, without copying their net.
    pub fn apply<C>(&self, points: &[C], row: &[Term], point: impl Fn(&C) -> Vec3) -> Result<Vec3> {
        if points.len() != self.weights.len() || row.len() > 17 * 17 {
            return Err(bad("tensor row or net dimensions"));
        }
        let mut sum = Vec3::ZERO;
        for &(i, j, c) in row {
            if i >= self.axes[0].count || j >= self.axes[1].count || !c.is_finite() {
                return Err(bad("invalid tensor row identity"));
            }
            let p = point(
                points
                    .get(j * self.axes[0].count + i)
                    .ok_or_else(|| bad("missing tensor row control"))?,
            );
            sum += p * c;
        }
        sum.to_array()
            .into_iter()
            .all(f64::is_finite)
            .then_some(sum)
            .ok_or_else(|| bad("nonfinite tensor row application"))
    }
    /// Normal using the guided fitter's existing 1e-9 differential cutoff.
    pub fn normal<C>(
        &self,
        points: &[C],
        uv: [f64; 2],
        point: impl Fn(&C) -> Vec3,
    ) -> Option<Vec3> {
        let du = self
            .apply(points, &self.row(uv, [1, 0], [false; 2]).ok()?, &point)
            .ok()?;
        let dv = self
            .apply(points, &self.row(uv, [0, 1], [false; 2]).ok()?, &point)
            .ok()?;
        du.cross(dv).normalized()
    }
}
fn table(table: &Table, i: usize, j: usize) -> Result<f64> {
    table
        .get(i)
        .and_then(|row| row.get(j))
        .copied()
        .ok_or_else(|| bad("tensor derivative table index"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rational() -> TensorBasis {
        TensorBasis::new(
            [1, 1],
            [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]],
            vec![1., 2., 1., 2.],
        )
        .unwrap()
    }
    #[test]
    fn rational_tensor_rows_match_closed_form_through_guided_fifth_order() {
        let basis = rational();
        let points = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(1., 1., 0.)];
        for u in [0., 0.125, 0.5, 0.875, 1.] {
            for v in [0., 0.3, 1.] {
                let row = basis.row([u, v], [0, 0], [false; 2]).unwrap();
                let point = basis.apply(&points, &row, |p| *p).unwrap();
                assert!((point.x - 2. * u / (1. + u)).abs() < 1e-14);
                assert!((point.y - v).abs() < 1e-14);
                assert!((row.iter().map(|&(_, _, c)| c).sum::<f64>() - 1.).abs() < 1e-14);
                let mut factorial = 1.;
                for order in 1..=5 {
                    factorial *= order as f64;
                    let row = basis.row([u, v], [order, 0], [false; 2]).unwrap();
                    let actual = basis.apply(&points, &row, |p| *p).unwrap();
                    let expected =
                        2. * if order % 2 == 1 {
                            factorial
                        } else {
                            -factorial
                        } / (1. + u).powi(order as i32 + 1);
                    assert!(
                        (actual.x - expected).abs() < 1e-11,
                        "u={u},v={v},order={order}: {} != {expected}",
                        actual.x
                    );
                    assert!(actual.y.abs() < 1e-11);
                    assert!(row.iter().map(|&(_, _, c)| c).sum::<f64>().abs() < 1e-11);
                }
                let mixed = basis
                    .apply(
                        &points,
                        &basis.row([u, v], [2, 1], [false; 2]).unwrap(),
                        |p| *p,
                    )
                    .unwrap();
                assert!(mixed.length() < 1e-12);
                assert!(
                    basis
                        .normal(&points, [u, v], |p| *p)
                        .unwrap()
                        .distance(Vec3::Z)
                        < 1e-12
                );
            }
        }
    }

    #[test]
    fn one_sided_rows_preserve_c0_knot_ownership_and_mutable_coordinate_updates() {
        let basis = TensorBasis::new(
            [1, 1],
            [vec![0., 0., 0.5, 1., 1.], vec![0., 0., 1., 1.]],
            vec![1.; 6],
        )
        .unwrap();
        let mut points = [
            Vec3::ZERO,
            Vec3::X,
            Vec3::X * 3.,
            Vec3::Y,
            Vec3::new(1., 1., 0.),
            Vec3::new(3., 1., 0.),
        ];
        let left = basis.row([0.5, 0.3], [1, 0], [true, false]).unwrap();
        let right = basis.row([0.5, 0.3], [1, 0], [false; 2]).unwrap();
        assert!(
            basis
                .apply(&points, &left, |p| *p)
                .unwrap()
                .distance(Vec3::X * 2.)
                < 1e-14
        );
        assert!(
            basis
                .apply(&points, &right, |p| *p)
                .unwrap()
                .distance(Vec3::X * 4.)
                < 1e-14
        );
        let saved = basis.weights.clone();
        for point in &mut points {
            *point += Vec3::new(7., 8., 9.);
        }
        assert!(
            basis
                .apply(&points, &right, |p| *p)
                .unwrap()
                .distance(Vec3::X * 4.)
                < 1e-12
        );
        assert_eq!(basis.weights, saved);
        assert!(basis.row([0.5, 0.3], [usize::MAX, 0], [false; 2]).is_err());
        assert!(basis.row([f64::NAN, 0.3], [0, 0], [false; 2]).is_err());
        assert!(basis
            .apply(&points, &[(usize::MAX, 0, 1.)], |p| *p)
            .is_err());
        assert!(TensorBasis::new([usize::MAX, 1], [vec![], vec![]], vec![]).is_err());
        assert!(TensorBasis::new(
            [1, 1],
            [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]],
            vec![1., 0., 1., 1.]
        )
        .is_err());
    }
}
