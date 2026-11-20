//! Exact tensor restrictions and geometric qualification of a subdivision atlas.
//! Boehm insertion, local bases, differentials and closest-point searches reuse
//! the kernel evaluator. The atlas distance remains explicitly sampled.
use super::Tensor;
use crate::error::nurbs::NurbsFailure;
use crate::eval::pcurve;
use crate::eval::plan::basis::{local_basis_array, Side};
use crate::eval::plan::uv::{Domain, Starts};
use crate::eval::plan::{uv, EvalSurface};
use crate::eval::vec::VecExt;
use crate::{Result, SolidError, Vec3};

const OP: &str = "geom::form_fill::rechart::chart";
const MAX_POINTS: usize = 1 << 20;
const MAX_KNOTS: usize = 1 << 14;
const CHECK_CELLS: usize = 24;
const MAX_CHECK_STATIONS: usize = 129;
const MAX_TRIM_POINTS: usize = 1 << 20;

fn error(reason: NurbsFailure) -> SolidError {
    SolidError::nurbs(OP, reason)
}
fn dimensions(degree: [usize; 2], knots: &[Vec<f64>; 2], len: usize) -> Result<[usize; 2]> {
    let mut count = [0; 2];
    for ((out, degree), knots) in count.iter_mut().zip(degree).zip(knots) {
        if !(1..=16).contains(&degree) {
            return Err(error(NurbsFailure::Degree));
        }
        if knots.len() > MAX_KNOTS {
            return Err(error(NurbsFailure::Storage));
        }
        *out = knots
            .len()
            .checked_sub(degree + 1)
            .filter(|&n| n > degree)
            .ok_or_else(|| error(NurbsFailure::Knots))?;
    }
    if count[0].checked_mul(count[1]) != Some(len) || len > MAX_POINTS {
        return Err(error(NurbsFailure::SurfaceDefinition));
    }
    Ok(count)
}
fn validate(tensor: &Tensor) -> Result<[usize; 2]> {
    let count = dimensions(tensor.degree, &tensor.knots, tensor.controls.len())?;
    if tensor.knots.iter().any(|knots| {
        knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| matches!(w,[a,b] if a>b))
    }) {
        return Err(error(NurbsFailure::Knots));
    }
    for ((knots, degree), count) in tensor.knots.iter().zip(tensor.degree).zip(count) {
        if knots
            .get(degree)
            .zip(knots.get(count))
            .is_none_or(|(a, b)| a >= b)
        {
            return Err(error(NurbsFailure::Knots));
        }
    }
    if tensor
        .controls
        .iter()
        .any(|h| h.iter().any(|v| !v.is_finite()) || h[3] <= 0.0)
    {
        return Err(error(NurbsFailure::Weight));
    }
    Ok(count)
}

/// Read a validated positive tensor at its original parameter, preserving the
/// local-basis and homogeneous accumulation order of the authoring evaluator.
pub fn point<C>(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    controls: &[C],
    uv: [f64; 2],
    components: impl Fn(&C) -> [f64; 4],
) -> Option<Vec3> {
    if dimensions(degree, knots, controls.len()).ok()? != count {
        return None;
    }
    let mut spans = [0; 2];
    let mut basis = [[0.0; 17]; 2];
    for (axis, parameter) in uv.into_iter().enumerate() {
        let p = *degree.get(axis)?;
        let n = *count.get(axis)?;
        let knots = knots.get(axis)?;
        let span = pcurve::find_span(knots, p, n, parameter)?;
        let (lo, hi) = (*knots.get(p)?, *knots.get(n)?);
        if !lo.is_finite() || !hi.is_finite() || lo >= hi {
            return None;
        }
        let t = parameter.max(lo).min(hi);
        let side = if t >= hi { Side::Left } else { Side::Right };
        *spans.get_mut(axis)? = span;
        for (offset, i) in (span.checked_sub(p)?..=span).enumerate() {
            *basis.get_mut(axis)?.get_mut(offset)? =
                local_basis_array(knots.get(i..i + p + 2)?, t, side, 0)[0];
        }
    }
    let mut sum = [0.0; 4];
    for (b, nj) in basis[1].iter().take(degree[1] + 1).enumerate() {
        for (a, ni) in basis[0].iter().take(degree[0] + 1).enumerate() {
            let u = spans[0].checked_sub(degree[0])?.checked_add(a)?;
            let v = spans[1].checked_sub(degree[1])?.checked_add(b)?;
            let h = components(controls.get(v.checked_mul(count[0])?.checked_add(u)?)?);
            for (out, value) in sum.iter_mut().zip(h) {
                *out += value * (ni * nj);
            }
        }
    }
    let p = Vec3::new(sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]);
    (sum[3].abs() > f64::MIN_POSITIVE && p.is_finite()).then_some(p)
}

/// Trim bounds and the existing axis-aligned rectangle classification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trim {
    /// The retained original source parameter box.
    pub domain: [[f64; 2]; 2],
    /// Every side is orthogonal and its area covers that parameter box.
    pub rectangular: bool,
}
/// Qualify a bounded finite source trim without changing its point order.
pub fn trim(wire: &[[f64; 2]]) -> Result<Trim> {
    if !(3..=MAX_TRIM_POINTS).contains(&wire.len()) || wire.iter().flatten().any(|x| !x.is_finite())
    {
        return Err(error(NurbsFailure::Input));
    }
    let domain = [0, 1].map(|d| {
        let values = || wire.iter().filter_map(|p| p.get(d)).copied();
        [
            values().fold(f64::INFINITY, f64::min),
            values().fold(f64::NEG_INFINITY, f64::max),
        ]
    });
    let area = wire
        .iter()
        .zip(wire.iter().cycle().skip(1))
        .take(wire.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        .abs()
        * 0.5;
    let rectangle = (domain[0][1] - domain[0][0]) * (domain[1][1] - domain[1][0]);
    if !area.is_finite() || !rectangle.is_finite() || rectangle <= 0.0 {
        return Err(error(NurbsFailure::Input));
    }
    let orthogonal = wire
        .iter()
        .zip(wire.iter().cycle().skip(1))
        .take(wire.len())
        .all(|(a, b)| a[0] == b[0] || a[1] == b[1]);
    Ok(Trim {
        domain,
        rectangular: orthogonal && (area - rectangle).abs() <= rectangle.abs() * 1e-12,
    })
}
/// Four exact dyadic subdomains, in the existing U-outer/V-inner order.
pub fn quarters(domain: [[f64; 2]; 2]) -> Result<[[[f64; 2]; 2]; 4]> {
    if domain
        .iter()
        .any(|[a, b]| !a.is_finite() || !b.is_finite() || a >= b)
    {
        return Err(error(NurbsFailure::Parameter));
    }
    let halves = domain.map(|[a, b]| [[a, a + (b - a) * 0.5], [a + (b - a) * 0.5, b]]);
    if halves
        .iter()
        .flatten()
        .any(|[a, b]| !a.is_finite() || !b.is_finite() || a >= b)
    {
        return Err(error(NurbsFailure::Numerical));
    }
    Ok([
        [halves[0][0], halves[1][0]],
        [halves[0][0], halves[1][1]],
        [halves[0][1], halves[1][0]],
        [halves[0][1], halves[1][1]],
    ])
}
/// Ordered corners of a rectangular parameter region.
pub fn rectangle(domain: [[f64; 2]; 2]) -> [[f64; 2]; 4] {
    let [[a, b], [c, d]] = domain;
    [[a, c], [b, c], [b, d], [a, d]]
}
/// Trim inclusion, including the original parameter-roundoff rim band.
fn inside(wire: &[[f64; 2]], p: [f64; 2]) -> bool {
    if wire.len() > MAX_TRIM_POINTS || !p.into_iter().all(f64::is_finite) {
        return false;
    }
    let mut result = false;
    for (a, b) in wire
        .iter()
        .zip(wire.iter().cycle().skip(1))
        .take(wire.len())
    {
        let delta = [b[0] - a[0], b[1] - a[1]];
        let cross = delta[0] * (p[1] - a[1]) - delta[1] * (p[0] - a[0]);
        let scale = 1.0 + a[0].abs() + a[1].abs() + b[0].abs() + b[1].abs();
        if cross.abs() <= 1e-10 * scale
            && p.into_iter()
                .zip(a)
                .zip(b)
                .all(|((p, a), b)| p >= a.min(*b) - 1e-10 * scale && p <= a.max(*b) + 1e-10 * scale)
        {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + delta[0] * (p[1] - a[1]) / delta[1] {
            result = !result;
        }
    }
    result
}
/// Distinct source knot cells inside an original parameter region, capped at256.
pub fn atlas_domains(knots: &[Vec<f64>; 2], domain: [[f64; 2]; 2]) -> Result<Vec<[[f64; 2]; 2]>> {
    if knots.iter().any(|k| {
        k.len() > MAX_KNOTS
            || k.iter().any(|x| !x.is_finite())
            || k.windows(2).any(|w| matches!(w,[a,b] if a>b))
    }) || domain
        .iter()
        .any(|[a, b]| !a.is_finite() || !b.is_finite() || a >= b)
    {
        return Err(error(NurbsFailure::Knots));
    }
    let axes: [Vec<f64>; 2] =
        [(&knots[0], domain[0]), (&knots[1], domain[1])].map(|(knots, [lo, hi])| {
            let mut axis = std::iter::once(lo)
                .chain(knots.iter().copied().filter(|k| *k > lo && *k < hi))
                .chain(std::iter::once(hi))
                .collect::<Vec<_>>();
            axis.dedup();
            axis
        });
    if axes[0]
        .len()
        .saturating_sub(1)
        .saturating_mul(axes[1].len().saturating_sub(1))
        > 256
    {
        return Err(error(NurbsFailure::Storage));
    }
    let mut output = Vec::new();
    for u in axes[0].windows(2) {
        for v in axes[1].windows(2) {
            let (&[a, b], &[c, d]) = (u, v) else {
                return Err(error(NurbsFailure::Knots));
            };
            output.push([[a, b], [c, d]]);
        }
    }
    Ok(output)
}
/// Exact tensor restriction, retaining the original source parameter coordinates.
pub fn restricted(source: &Tensor, domain: [[f64; 2]; 2]) -> Result<Tensor> {
    validate(source)?;
    let mut out = Tensor {
        degree: source.degree,
        knots: source.knots.clone(),
        controls: source.controls.clone(),
    };
    for (axis, [a, b]) in domain.into_iter().enumerate() {
        let count = dimensions(out.degree, &out.knots, out.controls.len())?;
        let (lines, width) = if axis == 0 {
            (count[1], count[0])
        } else {
            (count[0], count[1])
        };
        let mut rows = Vec::new();
        let mut new_knots = None;
        for line in 0..lines {
            let mut row = Vec::new();
            for i in 0..width {
                let index = if axis == 0 {
                    i + count[0] * line
                } else {
                    line + count[0] * i
                };
                row.push(
                    *out.controls
                        .get(index)
                        .ok_or_else(|| error(NurbsFailure::Index))?,
                );
            }
            let degree = *out
                .degree
                .get(axis)
                .ok_or_else(|| error(NurbsFailure::Index))?;
            let knots = out
                .knots
                .get(axis)
                .ok_or_else(|| error(NurbsFailure::Index))?;
            let (k, row) = pcurve::segment_guide(degree, knots, &row, a, b)?;
            if row.len().checked_mul(lines).is_none_or(|n| n > MAX_POINTS) {
                return Err(error(NurbsFailure::Storage));
            }
            new_knots = Some(k);
            rows.push(row);
        }
        let width = rows.first().map_or(0, Vec::len);
        if width.checked_mul(lines).is_none_or(|n| n > MAX_POINTS) {
            return Err(error(NurbsFailure::Storage));
        }
        let mut controls = Vec::new();
        if axis == 0 {
            for row in rows {
                controls.extend(row);
            }
        } else {
            for j in 0..width {
                for row in &rows {
                    controls.push(*row.get(j).ok_or_else(|| error(NurbsFailure::Index))?);
                }
            }
        }
        *out.knots
            .get_mut(axis)
            .ok_or_else(|| error(NurbsFailure::Index))? =
            new_knots.ok_or_else(|| error(NurbsFailure::Knots))?;
        out.controls = controls;
    }
    Ok(out)
}

/// The first nearly collinear original corner; this test does not certify regularity.
pub fn singular_corner(tensor: &Tensor) -> Result<Option<usize>> {
    let count = validate(tensor)?;
    let mut domain = [[0.0; 2]; 2];
    for (((out, knots), degree), count) in domain
        .iter_mut()
        .zip(&tensor.knots)
        .zip(tensor.degree)
        .zip(count)
    {
        *out = [
            *knots
                .get(degree)
                .ok_or_else(|| error(NurbsFailure::Knots))?,
            *knots.get(count).ok_or_else(|| error(NurbsFailure::Knots))?,
        ];
    }
    let [[u0, u1], [v0, v1]] = domain;
    Ok([
        [u0, v0, 1.0, 1.0],
        [u1, v0, -1.0, 1.0],
        [u1, v1, -1.0, -1.0],
        [u0, v1, 1.0, -1.0],
    ]
    .into_iter()
    .enumerate()
    .find_map(|(corner, [u, v, su, sv])| {
        let (_, a, b) = crate::eval::plan::nurbs_derivs(
            tensor.degree,
            &tensor.knots,
            count,
            &tensor.controls,
            [u, v],
            |h| *h,
        )?;
        let (a, b) = (a.normalized()? * su, b.normalized()? * sv);
        (a.cross(b).len() < 0.5_f64.to_radians().sin()).then_some(corner)
    }))
}

/// Retained exact tensor evaluator and its per-surface closest-point start cache.
pub struct Source {
    tensor: Tensor,
    count: [usize; 2],
    domain: [[f64; 2]; 2],
    eval: EvalSurface,
    parameter_domain: Domain,
    starts: Starts,
}
impl Source {
    /// Validate before copying into the existing kernel evaluator.
    pub fn new(tensor: Tensor) -> Result<Self> {
        let count = validate(&tensor)?;
        if tensor.controls.len() > crate::eval::MAX_EVAL_POINTS
            || tensor
                .knots
                .iter()
                .any(|k| k.len() > crate::eval::MAX_EVAL_KNOTS)
        {
            return Err(error(NurbsFailure::Storage));
        }
        let mut domain = [[0.0; 2]; 2];
        for (((out, knots), degree), count) in domain
            .iter_mut()
            .zip(&tensor.knots)
            .zip(tensor.degree)
            .zip(count)
        {
            *out = [
                *knots
                    .get(degree)
                    .ok_or_else(|| error(NurbsFailure::Knots))?,
                *knots.get(count).ok_or_else(|| error(NurbsFailure::Knots))?,
            ];
        }
        let eval = EvalSurface::nurbs(
            tensor.degree,
            tensor.knots.clone(),
            count,
            tensor.controls.clone(),
        );
        let parameter_domain = uv::domain(&eval);
        Ok(Self {
            tensor,
            count,
            domain,
            eval,
            parameter_domain,
            starts: Starts::default(),
        })
    }
    /// Original knot domain.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        self.domain
    }
    /// Original ordered-basis point evaluator.
    pub fn point(&self, uv: [f64; 2]) -> Result<Vec3> {
        point(
            self.tensor.degree,
            &self.tensor.knots,
            self.count,
            &self.tensor.controls,
            uv,
            |h| *h,
        )
        .ok_or_else(|| error(NurbsFailure::Numerical))
    }
    /// Exact differential frame used by the guided rechart fit.
    pub fn frame(&self, uv: [f64; 2], tangent: Vec3) -> Option<[Vec3; 2]> {
        if !tangent.is_finite()
            || uv
                .into_iter()
                .zip(self.domain)
                .any(|(t, [a, b])| !t.is_finite() || t < a || t > b)
        {
            return None;
        }
        let (_, du, dv) = crate::eval::plan::nurbs_derivs(
            self.tensor.degree,
            &self.tensor.knots,
            self.count,
            &self.tensor.controls,
            uv,
            |h| *h,
        )?;
        let normal = du.normalized()?.cross(dv.normalized()?).normalized()?;
        let leave = normal.cross(tangent);
        leave.is_finite().then_some([leave, normal])
    }
    pub(super) fn nearest(&self, p: Vec3) -> Option<([f64; 2], Vec3)> {
        let uv = uv::project_with(
            &self.eval,
            &self.parameter_domain,
            &self.starts,
            p,
            None,
            f64::INFINITY,
        )?;
        Some((uv, self.eval.derivs(uv[0], uv[1])?.0))
    }
    fn stations(&self, domain: [[f64; 2]; 2]) -> Result<[Vec<f64>; 2]> {
        let mut result: [Vec<f64>; 2] = domain.map(|[a, b]| {
            std::iter::once(a)
                .chain(
                    (0..CHECK_CELLS).map(|i| a + (b - a) * (i as f64 + 0.5) / CHECK_CELLS as f64),
                )
                .chain(std::iter::once(b))
                .collect()
        });
        for ((axis, knots), [lo, hi]) in result.iter_mut().zip(&self.tensor.knots).zip(domain) {
            for span in knots.windows(2) {
                let &[a, b] = span else {
                    return Err(error(NurbsFailure::Knots));
                };
                let middle = a + (b - a) * 0.5;
                if b > a && middle > lo && middle < hi {
                    axis.push(middle);
                }
            }
            axis.sort_by(f64::total_cmp);
            axis.dedup();
            if axis.len() > MAX_CHECK_STATIONS {
                return Err(error(NurbsFailure::Storage));
            }
        }
        Ok(result)
    }
    /// Original7x7 interior guide distribution, with exact trim inclusion.
    pub fn guides(&self, wire: &[[f64; 2]], domain: [[f64; 2]; 2]) -> Result<Vec<Vec3>> {
        if !(3..=16).contains(&wire.len()) {
            return Err(error(NurbsFailure::Input));
        }
        trim(wire)?;
        let mut guides = Vec::new();
        for i in 1..8 {
            for j in 1..8 {
                let uv = [
                    domain[0][0] + (domain[0][1] - domain[0][0]) * f64::from(i) / 8.0,
                    domain[1][0] + (domain[1][1] - domain[1][0]) * f64::from(j) / 8.0,
                ];
                if inside(wire, uv) {
                    guides.push(self.point(uv)?);
                }
            }
        }
        Ok(guides)
    }
    /// Bidirectional sampled atlas distance and worst-first source guide targets.
    /// Sampling does not certify a global surface approximation bound.
    pub fn measure<'a>(
        &self,
        wire: &[[f64; 2]],
        domain: [[f64; 2]; 2],
        fitted: impl ExactSizeIterator<Item = &'a Self> + Clone,
    ) -> Result<(f64, Vec<(f64, Vec3)>)> {
        if !(3..=16).contains(&wire.len()) || !(1..=64).contains(&fitted.len()) {
            return Err(error(NurbsFailure::Input));
        }
        trim(wire)?;
        let mut misses = Vec::new();
        let mut worst = 0.0_f64;
        let mut remember = |distance: f64, target| -> Result<()> {
            if !distance.is_finite() {
                return Err(error(NurbsFailure::Numerical));
            }
            worst = worst.max(distance);
            misses.push((distance, target));
            Ok(())
        };
        let [us, vs] = self.stations(domain)?;
        for &u in &us {
            for &v in &vs {
                if inside(wire, [u, v]) {
                    let point = self.point([u, v])?;
                    let distance = fitted
                        .clone()
                        .filter_map(|s| s.nearest(point).map(|(_, q)| point.dist(q)))
                        .fold(f64::INFINITY, f64::min);
                    remember(distance, point)?;
                }
            }
        }
        for patch in fitted {
            let [us, vs] = patch.stations(patch.domain())?;
            for &u in &us {
                for &v in &vs {
                    let point = patch.point([u, v])?;
                    let (uv, target) = self
                        .nearest(point)
                        .ok_or_else(|| error(NurbsFailure::NotConverged))?;
                    if !inside(wire, uv) {
                        return Err(error(NurbsFailure::Parameter));
                    }
                    remember(point.dist(target), target)?;
                }
            }
        }
        misses.sort_by(|a, b| b.0.total_cmp(&a.0));
        Ok((worst, misses))
    }
}
/// Preflight guide buffer counts before a caller allocates its representation conversion.
pub fn check_guide_sizes(guides: usize, misses: usize, limit: usize) -> Result<()> {
    if limit > 64 || guides > limit || misses > 65 * MAX_CHECK_STATIONS * MAX_CHECK_STATIONS {
        return Err(error(NurbsFailure::Storage));
    }
    Ok(())
}
/// Add distinct source targets in worst-first order without exceeding the fit's limit.
pub fn append_guides(
    guides: &mut Vec<Vec3>,
    misses: &[(f64, Vec3)],
    tolerance: f64,
    limit: usize,
) -> Result<()> {
    check_guide_sizes(guides.len(), misses.len(), limit)?;
    if !tolerance.is_finite()
        || tolerance <= 0.0
        || guides.iter().any(|point| !point.is_finite())
        || misses
            .iter()
            .any(|(gap, point)| !gap.is_finite() || !point.is_finite())
    {
        return Err(error(NurbsFailure::Input));
    }
    for &(_, point) in misses {
        if guides.len() >= limit {
            break;
        }
        if guides.iter().all(|p| p.dist(point) > tolerance * 1e-4) {
            guides.push(point);
        }
    }
    Ok(())
}
