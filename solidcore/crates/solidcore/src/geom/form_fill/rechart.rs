//! Exact Bernstein composition of a rational tensor with an oriented Duffy triangle.
//! The collapsed side retains a bit-identical homogeneous pole column.
use crate::{Result, SolidError};

pub mod chart;

const OP: &str = "geom::form_fill::rechart";

/// A bounded rational tensor in U-varying-fastest order.
pub struct Tensor {
    /// Tensor degrees.
    pub degree: [usize; 2],
    /// Clamped knots in both directions.
    pub knots: [Vec<f64>; 2],
    /// Positive homogeneous controls.
    pub controls: Vec<[f64; 4]>,
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>> {
    if count > 289 {
        return Err(bad("Duffy coefficient budget"));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| bad("Duffy coefficient allocation"))?;
    output.resize(count, value);
    Ok(output)
}

fn multiply(input: &[f64], degree: usize, factor: [f64; 4]) -> Result<Vec<f64>> {
    let width = degree + 1;
    let next = width + 1;
    let mut output = filled(next * next, 0.)?;
    for j in 0..width {
        for i in 0..width {
            let value = *input
                .get(i + width * j)
                .ok_or_else(|| bad("incomplete Bernstein grid"))?;
            for b in 0..2 {
                for a in 0..2 {
                    let along_r = if a == 0 { width - i } else { i + 1 } as f64 / width as f64;
                    let along_t = if b == 0 { width - j } else { j + 1 } as f64 / width as f64;
                    let factor = *factor
                        .get(a + 2 * b)
                        .ok_or_else(|| bad("bilinear factor"))?;
                    *output
                        .get_mut(i + a + next * (j + b))
                        .ok_or_else(|| bad("Bernstein index"))? +=
                        value * factor * along_r * along_t;
                }
            }
        }
    }
    Ok(output)
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1., |value, i| value * (n - i) as f64 / (i + 1) as f64)
}

/// Native chart parameters of three corners numbered around [00,10,11,01].
pub fn triangle_parameters(domain: [[f64; 2]; 2], corners: [usize; 3]) -> Result<[[f64; 2]; 3]> {
    if domain
        .iter()
        .any(|&[a, b]| !a.is_finite() || !b.is_finite() || a >= b)
    {
        return Err(bad("triangle domain"));
    }
    let positions = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let mut output = [[0.; 2]; 3];
    for (out, corner) in output.iter_mut().zip(corners) {
        let [u, v] = *positions
            .get(corner)
            .ok_or_else(|| bad("triangle corner"))?;
        *out = [
            domain[0][0] + (domain[0][1] - domain[0][0]) * u,
            domain[1][0] + (domain[1][1] - domain[1][0]) * v,
        ];
        if !out.iter().all(|x| x.is_finite()) {
            return Err(bad("triangle parameter overflow"));
        }
    }
    Ok(output)
}

/// Compose a single clamped rational Bezier tensor with an oriented Duffy map.
pub fn triangle(
    degree: [usize; 2],
    knots: [&[f64]; 2],
    controls: &[[f64; 4]],
    corners: [usize; 3],
) -> Result<Tensor> {
    let [p, q] = degree;
    let sum = p
        .checked_add(q)
        .filter(|&d| d <= 16 && p > 0 && q > 0)
        .ok_or_else(|| bad("bounded Bezier tensor degree"))?;
    if controls.len() != (p + 1) * (q + 1)
        || controls
            .iter()
            .any(|h| h.iter().any(|x| !x.is_finite()) || h[3] <= 0.)
    {
        return Err(bad("positive complete Bezier controls"));
    }
    for (degree, knots) in degree.into_iter().zip(knots) {
        let (a, b) = (
            *knots.first().ok_or_else(|| bad("Bezier knots"))?,
            *knots.last().ok_or_else(|| bad("Bezier knots"))?,
        );
        if knots.len() != 2 * (degree + 1)
            || !a.is_finite()
            || !b.is_finite()
            || a >= b
            || knots.iter().take(degree + 1).any(|&k| k != a)
            || knots.iter().skip(degree + 1).any(|&k| k != b)
        {
            return Err(bad("one clamped Bezier knot span required"));
        }
    }
    let [a, b, c] = triangle_parameters([[0., 1.], [0., 1.]], corners)?;
    if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) <= 0. {
        return Err(bad("triangle must follow the source orientation"));
    }
    let u = [a[0], b[0], a[0], c[0]];
    let v = [a[1], b[1], a[1], c[1]];
    let width = sum + 1;
    let mut output = filled(width * width, [0.; 4])?;
    for j in 0..=q {
        for i in 0..=p {
            let mut coefficient = vec![1.];
            let mut current = 0;
            for (power, count, factor) in [(i, p, u), (j, q, v)] {
                for k in 0..count {
                    coefficient = multiply(
                        &coefficient,
                        current,
                        if k < power {
                            factor
                        } else {
                            factor.map(|x| 1. - x)
                        },
                    )?;
                    current += 1;
                }
            }
            let point = controls
                .get(i + (p + 1) * j)
                .ok_or_else(|| bad("source control index"))?;
            let scale = binomial(p, i) * binomial(q, j);
            for (out, weight) in output.iter_mut().zip(coefficient) {
                for (out, point) in out.iter_mut().zip(point) {
                    *out += point * scale * weight;
                }
            }
        }
    }
    let pole_i = if a[0] == 0. { 0 } else { p };
    let pole_j = if a[1] == 0. { 0 } else { q };
    let pole = *controls
        .get(pole_i + (p + 1) * pole_j)
        .ok_or_else(|| bad("pole index"))?;
    for j in 0..width {
        *output
            .get_mut(width * j)
            .ok_or_else(|| bad("pole row index"))? = pole;
    }
    if output
        .iter()
        .any(|h| h.iter().any(|x| !x.is_finite()) || h[3] <= 0.)
    {
        return Err(bad("Duffy homogeneous overflow"));
    }
    let knots = std::iter::repeat_n(0., width)
        .chain(std::iter::repeat_n(1., width))
        .collect::<Vec<_>>();
    Ok(Tensor {
        degree: [sum, sum],
        knots: [knots.clone(), knots],
        controls: output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::plan::nurbs_derivs;

    #[test]
    fn duffy_composition_preserves_rational_surface_and_explicit_pole() {
        let knots = [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 0., 1., 1., 1.]];
        let controls = (0..3)
            .flat_map(|j| {
                (0..3).map(move |i| {
                    let (u, v) = (f64::from(i) / 2., f64::from(j) / 2.);
                    let w = 1. + f64::from(i + j) / 8.;
                    [u * w, v * w, u * v * w, w]
                })
            })
            .collect::<Vec<_>>();
        let tensor = triangle([2, 2], [&knots[0], &knots[1]], &controls, [0, 1, 3]).unwrap();
        let degree = tensor.degree[0];
        let count = degree + 1;
        for j in 0..count {
            assert_eq!(tensor.controls[j * count], controls[0]);
        }
        for r in [0., 0.125, 0.5, 1.] {
            for t in [0., 0.2, 0.8, 1.] {
                let source = nurbs_derivs(
                    [2, 2],
                    &knots,
                    [3, 3],
                    &controls,
                    [r * (1. - t), r * t],
                    |h| *h,
                )
                .unwrap()
                .0;
                let actual = nurbs_derivs(
                    tensor.degree,
                    &tensor.knots,
                    [count, count],
                    &tensor.controls,
                    [r, t],
                    |h| *h,
                )
                .unwrap()
                .0;
                assert!(actual.distance(source) < 1e-12, "r={r},t={t}");
            }
        }
        assert!(triangle([2, 2], [&knots[0], &knots[1]], &controls, [0, 3, 1]).is_err());
        assert!(triangle(
            [usize::MAX, 2],
            [&knots[0], &knots[1]],
            &controls,
            [0, 1, 3]
        )
        .is_err());
    }

    fn atlas_quarter_cylinder() -> Tensor {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        Tensor {
            degree: [2, 1],
            knots: [vec![2., 2., 2., 6., 6., 6.], vec![-1., -1., 3., 3.]],
            controls: [0., 2.]
                .into_iter()
                .flat_map(|z| [[1., 0., z, 1.], [w, w, z * w, w], [0., 1., z, 1.]])
                .collect(),
        }
    }
    #[test]
    fn restricted_rechart_retains_rational_positions_derivatives_and_original_parameters() {
        use super::chart::{point, restricted};
        let original = atlas_quarter_cylinder();
        let before = original.controls.clone();
        let crop = restricted(&original, [[3., 5.], [0., 2.]]).unwrap();
        assert_eq!(crop.knots[0].first(), Some(&3.));
        assert_eq!(crop.knots[0].last(), Some(&5.));
        assert_eq!(crop.knots[1].first(), Some(&0.));
        assert_eq!(crop.knots[1].last(), Some(&2.));
        for u in [3., 3.5, 4., 4.5, 5.] {
            for v in [0., 0.5, 1., 1.5, 2.] {
                let p = point(
                    crop.degree,
                    &crop.knots,
                    [3, 2],
                    &crop.controls,
                    [u, v],
                    |h| *h,
                )
                .unwrap();
                let t = (u - 2.) / 4.;
                let w = std::f64::consts::FRAC_1_SQRT_2;
                let den = (1. - t) * (1. - t) + 2. * w * t * (1. - t) + t * t;
                let expected = crate::Vec3::new(
                    ((1. - t) * (1. - t) + 2. * w * t * (1. - t)) / den,
                    (2. * w * t * (1. - t) + t * t) / den,
                    (v + 1.) * 0.5,
                );
                assert!(p.distance(expected) < 1e-12);
                let (_, du, dv) = nurbs_derivs(
                    crop.degree,
                    &crop.knots,
                    [3, 2],
                    &crop.controls,
                    [u, v],
                    |h| *h,
                )
                .unwrap();
                let (_, a, b) = nurbs_derivs(
                    original.degree,
                    &original.knots,
                    [3, 2],
                    &original.controls,
                    [u, v],
                    |h| *h,
                )
                .unwrap();
                assert!(du.distance(a) < 1e-12 && dv.distance(b) < 1e-12);
            }
        }
        assert_eq!(original.controls, before);
        assert!(restricted(&original, [[1., 5.], [0., 2.]]).is_err());
        assert!(restricted(&original, [[3., 5.], [f64::NAN, 2.]]).is_err());
    }
    #[test]
    fn rechart_trim_and_atlas_preserve_concavity_cell_order_and_caps() {
        use super::chart::{atlas_domains, quarters, rectangle, trim};
        let l = [[0., 0.], [2., 0.], [2., 1.], [1., 1.], [1., 2.], [0., 2.]];
        assert!(!trim(&l).unwrap().rectangular);
        assert!(trim(&rectangle([[2., 6.], [-1., 3.]])).unwrap().rectangular);
        assert_eq!(
            quarters([[2., 6.], [-1., 3.]]).unwrap(),
            [
                [[2., 4.], [-1., 1.]],
                [[2., 4.], [1., 3.]],
                [[4., 6.], [-1., 1.]],
                [[4., 6.], [1., 3.]]
            ]
        );
        let cells = atlas_domains(
            &[vec![0., 0., 0.5, 1., 1.], vec![0., 0., 0.25, 0.75, 1., 1.]],
            [[0., 1.], [0., 1.]],
        )
        .unwrap();
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0], [[0., 0.5], [0., 0.25]]);
        assert_eq!(cells[5], [[0.5, 1.], [0.75, 1.]]);
        let many = (0..=17).map(|i| f64::from(i) / 17.).collect::<Vec<_>>();
        assert!(atlas_domains(&[many.clone(), many], [[0., 1.], [0., 1.]]).is_err());
        assert!(trim(&[[0., 0.], [1., 0.], [f64::INFINITY, 1.]]).is_err());
    }
    #[test]
    fn rechart_samples_measure_exact_planar_gap_and_keep_frames_and_guide_order() {
        use super::chart::{append_guides, rectangle, singular_corner, Source};
        let plane = |z| Tensor {
            degree: [1, 1],
            knots: [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]],
            controls: vec![
                [0., 0., z, 1.],
                [1., 0., z, 1.],
                [0., 1., z, 1.],
                [1., 1., z, 1.],
            ],
        };
        assert_eq!(singular_corner(&plane(2.)).unwrap(), None);
        let source = Source::new(plane(2.)).unwrap();
        let fitted = Source::new(plane(2.001)).unwrap();
        let wire = rectangle(source.domain());
        let (gap, misses) = source
            .measure(&wire, source.domain(), std::iter::once(&fitted))
            .unwrap();
        assert!((gap - 0.001).abs() < 1e-12);
        assert!(misses.windows(2).all(|p| p[0].0 >= p[1].0));
        let [leave, normal] = source.frame([0.25, 0.75], crate::Vec3::X).unwrap();
        assert!(leave.distance(crate::Vec3::Y) < 1e-12 && normal.distance(crate::Vec3::Z) < 1e-12);
        let mut guides = source.guides(&wire, source.domain()).unwrap();
        assert_eq!(guides.len(), 49);
        let first = guides[0];
        let target = crate::Vec3::new(0.123, 0.321, 2.);
        append_guides(
            &mut guides,
            &[(1., first), (0.5, target), (0.4, target)],
            0.05,
            64,
        )
        .unwrap();
        assert_eq!(guides.len(), 50);
        assert_eq!(guides.last(), Some(&target));
        assert!(append_guides(&mut guides, &[], 0.05, usize::MAX).is_err());
    }
    #[test]
    fn rechart_point_keeps_tiny_domains_and_rounded_singular_corner_identity() {
        use super::chart::{point, singular_corner};
        let width = 1e-15;
        let knots = [vec![0., 0., width, width], vec![0., 0., 1., 1.]];
        let controls = [
            [0., 0., 0., 1.],
            [1., 0., 0., 1.],
            [0., 1., 0., 1.],
            [1., 1., 0., 1.],
        ];
        let p = point([1, 1], &knots, [2, 2], &controls, [width / 4., 0.1], |h| *h).unwrap();
        assert!(p.distance(crate::Vec3::new(0.25, 0.1, 0.)) < 1e-14);
        assert_eq!(
            point(
                [1, 1],
                &knots,
                [2, 2],
                &controls,
                [
                    f64::from_bits(width.to_bits() + 1),
                    f64::from_bits(1.0f64.to_bits() + 1)
                ],
                |h| *h
            )
            .unwrap(),
            crate::Vec3::new(1., 1., 0.)
        );
        let controls = (0..3)
            .flat_map(|j| {
                (0..2).map(move |i| {
                    let (u, v) = (f64::from(i), f64::from(j) / 2.);
                    [u + v, u * v + if j == 2 { 1. } else { 0. }, u * v, 1.]
                })
            })
            .collect();
        let rounded = Tensor {
            degree: [1, 2],
            knots: [vec![0., 0., 1., 1.], vec![0., 0., 0., 1., 1., 1.]],
            controls,
        };
        assert_eq!(singular_corner(&rounded).unwrap(), Some(0));
    }

    #[test]
    fn rechart_nearest_keeps_the_original_plan_projector_and_exact_curved_witness() {
        use super::chart::Source;
        use crate::eval::plan::{uv, EvalSurface};
        let tensor = atlas_quarter_cylinder();
        let eval = EvalSurface::nurbs(
            tensor.degree,
            tensor.knots.clone(),
            [3, 2],
            tensor.controls.clone(),
        );
        let domain = uv::domain(&eval);
        let source = Source::new(tensor).unwrap();
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let query = crate::Vec3::new(1.02 * w, 1.02 * w, 1.0);
        let (at, point) = source.nearest(query).unwrap();
        assert!((at[0] - 4.0).abs() < 1e-10 && (at[1] - 1.0).abs() < 1e-10);
        assert!(point.distance(crate::Vec3::new(w, w, 1.0)) < 1e-10);
        assert!((point.distance(query) - 0.02).abs() < 1e-10);
        let expected = uv::project_with(
            &eval,
            &domain,
            &uv::Starts::default(),
            query,
            None,
            f64::INFINITY,
        )
        .unwrap();
        assert_eq!(
            at, expected,
            "cache preparation must retain the original authoring projector"
        );
        assert!(source
            .frame([2.0 - f64::EPSILON * 2., 1.0], crate::Vec3::X)
            .is_none());
    }
}
