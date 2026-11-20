//! Exact directed side normalization and compatible tensor preparation for quad caps.
//! Joins retain the preceding endpoint within the existing 1e-9 source tolerance;
//! bottom/top own tolerated tensor corner joins. No side is sampled or refitted.
use super::rechart::Tensor;
use crate::error::nurbs::NurbsFailure;
use crate::geom::spline_space::{embed, Space};
use crate::{Result, SolidError, Vec3};
const OP: &str = "geom::form_fill::quad_cap";

/// Maximum number of source curve pieces across the four directed sides.
pub const MAX_PIECES: usize = 64;
/// Maximum control count along each cap axis.
pub const MAX_AXIS_CONTROLS: usize = 256;
const JOIN_MM: f64 = super::CORNER_JOIN_MM;
const MAX_DEGREE: usize = crate::eval::plan::MAX_SPLINE_DEGREE;

/// One exact source curve on its share of a directed unit rim side.
#[derive(Clone, Copy)]
pub struct Piece<'a> {
    /// Its increasing share of this side, preserving caller ownership.
    pub range: [f64; 2],
    /// Polynomial degree of this piece.
    pub degree: usize,
    /// Its own clamped knots, on [0,1].
    pub knots: &'a [f64],
    /// Positive homogeneous controls.
    pub controls: &'a [[f64; 4]],
}
struct Curve {
    space: Space,
    points: Vec<[f64; 4]>,
}
fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}
fn invalid<T>(reason: &'static str) -> Result<T> {
    Err(bad(reason))
}
fn project([x, y, z, w]: [f64; 4]) -> Option<Vec3> {
    let point = Vec3::new(x / w, y / w, z / w);
    ([x, y, z, w].into_iter().all(f64::is_finite)
        && w > f64::MIN_POSITIVE
        && point.to_array().into_iter().all(f64::is_finite))
    .then_some(point)
}
fn join(a: [f64; 4], b: [f64; 4], weights: bool) -> bool {
    if weights {
        return super::compatible_corner(a, b);
    }
    project(a)
        .zip(project(b))
        .is_some_and(|(a, b)| a.distance(b) <= JOIN_MM)
}
fn valid(row: &[[f64; 4]]) -> bool {
    row.iter().all(|&p| project(p).is_some())
}
fn interior(space: &Space) -> Vec<(f64, usize)> {
    let [a, b] = space.domain();
    let mut output: Vec<(f64, usize)> = Vec::new();
    for &k in space.knots().iter().filter(|k| **k > a && **k < b) {
        match output.last_mut() {
            Some((old, count)) if *old == k => *count += 1,
            _ => output.push((k, 1)),
        }
    }
    output
}
fn compatible(a: &Curve, b: &Curve) -> Result<(Space, [Vec<[f64; 4]>; 2])> {
    let degree = a.space.degree().max(b.space.degree());
    let mut breaks: Vec<(f64, usize)> = Vec::new();
    for row in [a, b] {
        // Keep the previous refusal: general spline degree elevation is not
        // part of this migration; only polynomial lower-degree rows elevate.
        if row.space.degree() != degree && row.space.count() != row.space.degree() + 1 {
            return invalid("quad cap opposite sides have incompatible spline degrees");
        }
        for (knot, multiplicity) in interior(&row.space) {
            if let Some((_, count)) = breaks.iter_mut().find(|(k, _)| *k == knot) {
                *count = (*count).max(multiplicity);
            } else {
                breaks.push((knot, multiplicity));
            }
        }
    }
    if breaks
        .iter()
        .try_fold(degree + 1, |n, (_, count)| n.checked_add(*count))
        .is_none_or(|n| n > MAX_AXIS_CONTROLS)
    {
        return Err(SolidError::nurbs(OP, NurbsFailure::Storage));
    }
    breaks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut knots = Vec::new();
    knots.extend(std::iter::repeat_n(0.0, degree + 1));
    for (knot, count) in breaks {
        knots.extend(std::iter::repeat_n(knot, count));
    }
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    let target = Space::new(degree, knots)?;
    let a = embed(&a.space, &a.points, &target)?;
    let b = embed(&b.space, &b.points, &target)?;
    Ok((target, [a, b]))
}
/// Normalize four ordered source sides, embed their exact curves in compatible
/// spaces, blend and qualify the resulting positive rational tensor. Side order
/// remains bottom/right/top/left along the rim; internal reversal changes only
/// the two tensor-positive side representations, never source ownership.
pub fn construct(sides: [&[Piece<'_>]; 4]) -> Result<Tensor> {
    if sides
        .iter()
        .any(|side| side.is_empty() || side.len() > MAX_PIECES)
        || sides.iter().map(|side| side.len()).sum::<usize>() > MAX_PIECES
    {
        return invalid("quad cap needs four complete bounded sides");
    }
    let mut curves = Vec::new();
    let mut first = None;
    let mut previous = None;
    for (index, side) in sides.into_iter().enumerate() {
        let degree = side.iter().map(|piece| piece.degree).max().unwrap_or(0);
        if !(1..=MAX_DEGREE).contains(&degree) {
            return invalid("invalid quad cap curve degree");
        }
        let mut knots = Vec::new();
        knots.extend(std::iter::repeat_n(0.0, degree + 1));
        let mut points = Vec::new();
        let mut end = 0.0;
        for (piece, curve) in side.iter().enumerate() {
            let [a, b] = curve.range;
            if a != end
                || !(a < b && b <= 1.0)
                || curve.degree > degree
                || curve.controls.len() > MAX_AXIS_CONTROLS
                || curve.knots.len() != curve.controls.len() + curve.degree + 1
            {
                return invalid("quad cap curve pieces must partition a bounded unit side");
            }
            let space = Space::new(curve.degree, curve.knots.to_vec())?;
            if space.domain() != [0.0, 1.0] {
                return invalid("quad cap piece has a non-unit curve domain");
            }
            let mut row = curve.controls.to_vec();
            if !valid(&row) {
                return invalid("quad cap curve needs positive finite homogeneous controls");
            }
            let start = *row.first().ok_or_else(|| bad("empty quad cap curve"))?;
            if previous.is_some_and(|old| !join(old, start, false)) {
                return invalid("quad cap boundary pieces do not meet");
            }
            let scale = previous.map_or(1.0, |old: [f64; 4]| old[3]) / start[3];
            for point in &mut row {
                *point = point.map(|x| x * scale);
            }
            if !valid(&row) {
                return invalid("quad cap weights cannot be normalized");
            }
            first = first.or_else(|| row.first().copied());
            previous = row.last().copied();
            let space = if curve.degree == degree {
                space
            } else {
                let raised = Space::polynomial(degree, 0.0, 1.0)?;
                row = embed(&space, &row, &raised)?;
                raised
            };
            if points.len().saturating_add(row.len()) > MAX_AXIS_CONTROLS + usize::from(piece > 0) {
                return Err(SolidError::nurbs(OP, NurbsFailure::Storage));
            }
            let inside = space
                .knots()
                .get(degree + 1..space.count())
                .ok_or_else(|| bad("quad cap interior knot range"))?;
            knots.extend(inside.iter().map(|&k| a + (b - a) * k));
            knots.extend(std::iter::repeat_n(
                b,
                degree + usize::from(piece + 1 == side.len()),
            ));
            points.extend(row.into_iter().skip(usize::from(piece > 0)));
            end = b;
        }
        if end != 1.0 {
            return invalid("quad cap boundary side is incomplete");
        }
        if index >= 2 {
            knots = knots.into_iter().rev().map(|k| 1.0 - k).collect();
            points.reverse();
        }
        curves.push(Curve {
            space: Space::new(degree, knots)?,
            points,
        });
    }
    if !first.zip(previous).is_some_and(|(a, b)| join(a, b, true)) {
        return invalid("quad cap boundary positions or weights do not close");
    }
    let [bottom, right, top, left]: [Curve; 4] =
        curves.try_into().map_err(|_| bad("quad cap side count"))?;
    let (u, [bottom, top]) = compatible(&bottom, &top)?;
    let (v, [left, right]) = compatible(&left, &right)?;
    let net = super::coons(
        [u.degree(), v.degree()],
        [u.knots(), v.knots()],
        [&bottom, &top, &left, &right],
    )?;
    // Preserve the old Patch::surface projection/weight round trip exactly;
    // changing that rounding during an ownership migration would alter geometry.
    let controls = net
        .points
        .into_iter()
        .zip(net.weights)
        .map(|(point, w)| [point.x * w, point.y * w, point.z * w, w])
        .collect::<Vec<_>>();
    if !valid(&controls) {
        return invalid("quad cap has invalid homogeneous tensor controls");
    }
    let degree = [u.degree(), v.degree()];
    let count = [u.count(), v.count()];
    let knots = [u.knots().to_vec(), v.knots().to_vec()];
    if !super::regularity::certify(degree, [&knots[0], &knots[1]], count, &controls)? {
        return invalid("quad cap has no conservative regularity certificate");
    }
    Ok(Tensor {
        degree,
        knots,
        controls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::plan::nurbs_derivs;
    type OwnedPiece = ([f64; 2], usize, Vec<f64>, Vec<[f64; 4]>);
    fn borrowed(sides: &[Vec<OwnedPiece>; 4]) -> [Vec<Piece<'_>>; 4] {
        sides.each_ref().map(|side| {
            side.iter()
                .map(|(range, degree, knots, controls)| Piece {
                    range: *range,
                    degree: *degree,
                    knots,
                    controls,
                })
                .collect()
        })
    }
    fn run(sides: &[Vec<OwnedPiece>; 4]) -> Result<Tensor> {
        let view = borrowed(sides);
        let [a, b, c, d] = &view;
        construct([a, b, c, d])
    }

    #[test]
    fn preserves_regular_tangent_sides_and_refuses_a_sub_tolerance_boundary_crossing() {
        // Degree-(1,2) Coons data for S=(u+v,uv+v²+2h(1-u)v(1-v),0).
        // For h=0 the Jacobian is u+v, positive except at the first corner.
        // For h=-e the left side crosses the bottom at 2e/(1+2e), so an
        // exact-side construction cannot repair the cap by reparameterizing it.
        let sides = |h: f64| {
            let controls = [
                [0., 0., 0., 1.],
                [1., 0., 0., 1.],
                [0.5, h, 0., 1.],
                [1.5, 0.5, 0., 1.],
                [1., 1., 0., 1.],
                [2., 2., 0., 1.],
            ];
            let line = vec![0., 0., 1., 1.];
            let quadratic = vec![0., 0., 0., 1., 1., 1.];
            [
                vec![([0., 1.], 1, line.clone(), vec![controls[0], controls[1]])],
                vec![(
                    [0., 1.],
                    2,
                    quadratic.clone(),
                    vec![controls[1], controls[3], controls[5]],
                )],
                vec![([0., 1.], 1, line, vec![controls[5], controls[4]])],
                vec![(
                    [0., 1.],
                    2,
                    quadratic,
                    vec![controls[4], controls[2], controls[0]],
                )],
            ]
        };
        let regular = sides(0.0);
        let original = regular.clone();
        let tensor = run(&regular).unwrap();
        assert_eq!(regular, original);
        assert_eq!(tensor.controls[2], [0.5, 0.0, 0.0, 1.0]);
        let folded = sides(-2.0_f64.powi(-40));
        let original = folded.clone();
        assert!(run(&folded).is_err());
        assert_eq!(
            folded, original,
            "a refused construction keeps the supplied curves"
        );
    }
    fn line(a: [f64; 3], b: [f64; 3]) -> OwnedPiece {
        (
            [0.0, 1.0],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]],
        )
    }
    fn square() -> [Vec<OwnedPiece>; 4] {
        [
            vec![line([0., 0., 0.], [1., 0., 0.])],
            vec![line([1., 0., 0.], [1., 1., 0.])],
            vec![line([1., 1., 0.], [0., 1., 0.])],
            vec![line([0., 1., 0.], [0., 0., 0.])],
        ]
    }
    #[test]
    fn normalized_split_rational_sides_retain_the_closed_form_surface_and_source_order() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let cut = 0.375;
        let arc = [[1., 0., 0., 1.], [w, w, 0., w], [0., 1., 0., 1.]];
        let base = Space::polynomial(2, 0., 1.).unwrap();
        let split = Space::new(2, vec![0., 0., 0., cut, cut, 1., 1., 1.]).unwrap();
        let split = embed(&base, &arc, &split).unwrap();
        let scale = |row: &[[f64; 4]], z: f64, factor: f64, reverse: bool| {
            let mut points = row
                .iter()
                .map(|&[x, y, pz, w]| [x * factor, y * factor, (pz + w * z) * factor, w * factor])
                .collect::<Vec<_>>();
            if reverse {
                points.reverse();
            }
            points
        };
        let bottom = vec![(
            [0., 1.],
            2,
            base.knots().to_vec(),
            scale(&arc, 0., 7., false),
        )];
        let top = vec![
            (
                [0., 1. - cut],
                2,
                base.knots().to_vec(),
                scale(&split[2..], 2., 3., true),
            ),
            (
                [1. - cut, 1.],
                2,
                base.knots().to_vec(),
                scale(&split[..3], 2., 11., true),
            ),
        ];
        let right = vec![(
            [0., 1.],
            3,
            vec![0., 0., 0., 0., 1., 1., 1., 1.],
            (0..4)
                .map(|j| [0., 1., 2. * f64::from(j) / 3., 1.])
                .collect(),
        )];
        let left = vec![line([1., 0., 2.], [1., 0., 0.])];
        let sides = [bottom, right, top, left];
        let saved = sides.clone();
        let tensor = run(&sides).unwrap();
        assert_eq!(
            sides, saved,
            "source piece IDs/order and values are input-only"
        );
        assert_eq!(tensor.degree, [2, 3]);
        assert_eq!(tensor.knots[0].iter().filter(|&&k| k == cut).count(), 2);
        let count = [tensor.knots[0].len() - 3, tensor.knots[1].len() - 4];
        for u in [0., cut / 2., cut, 0.77, 1.] {
            let a = 1. - u;
            let denominator = a * a + 2. * w * a * u + u * u;
            for v in [0., 0.2, 0.5, 1.] {
                let expected = Vec3::new(
                    (a * a + 2. * w * a * u) / denominator,
                    (2. * w * a * u + u * u) / denominator,
                    2. * v,
                );
                let actual = nurbs_derivs(
                    tensor.degree,
                    &tensor.knots,
                    count,
                    &tensor.controls,
                    [u, v],
                    |h| *h,
                )
                .unwrap()
                .0;
                assert!(actual.distance(expected) < 1e-11, "u={u},v={v}");
            }
        }
    }

    #[test]
    fn malformed_or_incomplete_directed_sides_refuse_without_touching_the_source() {
        let mut cases = Vec::new();
        let mut gap = square();
        gap[0][0].0[0] = 0.125;
        cases.push(gap);
        let mut incomplete = square();
        incomplete[0][0].0[1] = 0.75;
        cases.push(incomplete);
        let mut domain = square();
        domain[0][0].2 = vec![0., 0., 2., 2.];
        cases.push(domain);
        let mut weight = square();
        weight[0][0].3[1][3] = -1.;
        cases.push(weight);
        let mut hidden = square();
        hidden[0][0].3[0][0] = f64::INFINITY;
        cases.push(hidden);
        let mut reversed = square();
        reversed.swap(1, 3);
        cases.push(reversed);
        let mut degree = square();
        degree[0][0].1 = usize::MAX;
        cases.push(degree);
        let mut closure = square();
        closure[3][0].3[1][3] = 2.;
        cases.push(closure);
        let mut count = square();
        count[0][0].3 = vec![[0., 0., 0., 1.]; MAX_AXIS_CONTROLS + 1];
        cases.push(count);
        for sides in cases {
            let saved = sides.clone();
            assert!(run(&sides).is_err());
            assert_eq!(sides, saved);
        }
    }

    #[test]
    fn unsupported_multispan_degree_elevation_keeps_its_explicit_refusal() {
        let mut pieces = square();
        pieces[0] = vec![
            (
                [0., 0.5],
                1,
                vec![0., 0., 0.5, 1., 1.],
                vec![[0., 0., 0., 1.], [0.25, 0., 0., 1.], [0.5, 0., 0., 1.]],
            ),
            (
                [0.5, 1.],
                2,
                vec![0., 0., 0., 1., 1., 1.],
                vec![[0.5, 0., 0., 1.], [0.75, 0., 0., 1.], [1., 0., 0., 1.]],
            ),
        ];
        assert!(run(&pieces)
            .err()
            .is_some_and(|error| matches!(error.nurbs_failure(), Some(NurbsFailure::Degree))));
        let mut opposite = square();
        let mut first = line([0., 0., 0.], [0.5, 0., 0.]);
        first.0 = [0., 0.5];
        let mut second = line([0.5, 0., 0.], [1., 0., 0.]);
        second.0 = [0.5, 1.];
        opposite[0] = vec![first, second];
        opposite[2] = vec![(
            [0., 1.],
            2,
            vec![0., 0., 0., 1., 1., 1.],
            vec![[1., 1., 0., 1.], [0.5, 1., 0., 1.], [0., 1., 0., 1.]],
        )];
        assert!(
            matches!(run(&opposite),Err(SolidError::InvalidInput{reason,..}) if reason.contains("incompatible spline degrees"))
        );
    }

    #[test]
    fn accepts_the_retained_rounded_nonplanar_rim_with_a_nonzero_rotated_corner_normal() {
        // Exact saved coefficients from the 64mm,8x8 open refined Form rim.
        // Its first native tile has all reduced X/Y normal Bernstein coefficients
        // strictly positive, while the corner Z coefficient is negative (~1e-14).
        // This is a regular three-dimensional corner, not an exactly-zero normal.
        // Weights varying by an ulp are authoritative and must remain rational.
        let sides: [Vec<OwnedPiece>; 4] = [
            vec![
                (
                    [0.0, 0.5],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -14.666666666666666,
                            -14.666666666666666,
                            0.03333333333333335,
                            1.0,
                        ],
                        [-14.333333333333334, -15.0, 0.03333333333333334, 1.0],
                        [-13.916666666666668, -15.25, 0.03229166666666668, 1.0],
                        [
                            -13.458333333333334,
                            -15.416666666666666,
                            0.03059895833333334,
                            1.0,
                        ],
                        [-13.229166666666668, -15.5, 0.029752604166666672, 1.0],
                        [-12.989583333333334, -15.5625, 0.028743489583333337, 1.0],
                        [
                            -12.744791666666664,
                            -15.604166666666664,
                            0.027620442708333328,
                            0.9999999999999999,
                        ],
                        [-12.5, -15.645833333333334, 0.026497395833333333, 1.0],
                        [-12.25, -15.666666666666666, 0.025260416666666667, 1.0],
                        [
                            -12.0,
                            -15.666666666666664,
                            0.023958333333333325,
                            0.9999999999999999,
                        ],
                    ],
                ),
                (
                    [0.5, 1.0],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -11.999999999999998,
                            -15.666666666666664,
                            0.023958333333333328,
                            0.9999999999999999,
                        ],
                        [-11.75, -15.666666666666666, 0.02265625, 1.0],
                        [
                            -11.500000000000002,
                            -15.645833333333334,
                            0.021289062500000004,
                            1.0,
                        ],
                        [
                            -11.255208333333334,
                            -15.604166666666666,
                            0.019905598958333336,
                            1.0,
                        ],
                        [
                            -11.010416666666671,
                            -15.562500000000002,
                            0.01852213541666668,
                            1.0000000000000002,
                        ],
                        [
                            -10.770833333333336,
                            -15.500000000000002,
                            0.017122395833333345,
                            1.0000000000000002,
                        ],
                        [
                            -10.54166666666667,
                            -15.41666666666667,
                            0.015755208333333343,
                            1.0000000000000002,
                        ],
                        [
                            -10.083333333333336,
                            -15.250000000000004,
                            0.013020833333333344,
                            1.0000000000000002,
                        ],
                        [
                            -9.666666666666673,
                            -15.00000000000001,
                            0.010416666666666675,
                            1.0000000000000002,
                        ],
                        [
                            -9.333333333333337,
                            -14.66666666666667,
                            0.008333333333333335,
                            1.0000000000000002,
                        ],
                    ],
                ),
            ],
            vec![
                (
                    [0.0, 0.5],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -9.333333333333334,
                            -14.666666666666666,
                            0.008333333333333333,
                            1.0,
                        ],
                        [-9.0, -14.333333333333334, 0.0062499999999999995, 1.0],
                        [-8.749999999999998, -13.916666666666668, 0.0046875, 1.0],
                        [
                            -8.583333333333334,
                            -13.458333333333334,
                            0.0035156250000000005,
                            1.0,
                        ],
                        [-8.5, -13.229166666666668, 0.0029296875, 1.0],
                        [-8.437500000000002, -12.989583333333334, 0.00244140625, 1.0],
                        [
                            -8.395833333333334,
                            -12.744791666666664,
                            0.002034505208333333,
                            0.9999999999999999,
                        ],
                        [-8.35416666666667, -12.5, 0.0016276041666666678, 1.0],
                        [-8.333333333333334, -12.25, 0.001302083333333334, 1.0],
                        [
                            -8.333333333333332,
                            -12.0,
                            0.0010416666666666656,
                            0.9999999999999999,
                        ],
                    ],
                ),
                (
                    [0.5, 1.0],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -8.333333333333332,
                            -11.999999999999998,
                            0.0010416666666666664,
                            0.9999999999999999,
                        ],
                        [-8.333333333333334, -11.75, 0.0007812499999999999, 1.0],
                        [
                            -8.354166666666668,
                            -11.500000000000002,
                            0.0005859375000000001,
                            1.0,
                        ],
                        [
                            -8.395833333333334,
                            -11.255208333333334,
                            0.00043945312499999996,
                            1.0,
                        ],
                        [
                            -8.437500000000004,
                            -11.010416666666671,
                            0.00029296875000000015,
                            1.0000000000000002,
                        ],
                        [
                            -8.500000000000002,
                            -10.770833333333336,
                            0.00019531250000000007,
                            1.0000000000000002,
                        ],
                        [
                            -8.583333333333336,
                            -10.54166666666667,
                            0.0001302083333333333,
                            1.0000000000000002,
                        ],
                        [
                            -8.750000000000002,
                            -10.083333333333336,
                            0.0,
                            1.0000000000000002,
                        ],
                        [
                            -9.000000000000005,
                            -9.666666666666673,
                            0.0,
                            1.0000000000000002,
                        ],
                        [
                            -9.333333333333336,
                            -9.333333333333337,
                            0.0,
                            1.0000000000000002,
                        ],
                    ],
                ),
            ],
            vec![
                (
                    [0.0, 0.5],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -9.333333333333337,
                            -9.333333333333336,
                            0.0,
                            1.0000000000000002,
                        ],
                        [
                            -9.666666666666673,
                            -9.000000000000005,
                            0.0,
                            1.0000000000000002,
                        ],
                        [
                            -10.083333333333336,
                            -8.750000000000002,
                            0.0,
                            1.0000000000000002,
                        ],
                        [
                            -10.54166666666667,
                            -8.583333333333336,
                            0.0001302083333333333,
                            1.0000000000000002,
                        ],
                        [
                            -10.770833333333336,
                            -8.500000000000002,
                            0.00019531250000000007,
                            1.0000000000000002,
                        ],
                        [
                            -11.010416666666671,
                            -8.437500000000004,
                            0.00029296875000000015,
                            1.0000000000000002,
                        ],
                        [
                            -11.255208333333334,
                            -8.395833333333334,
                            0.00043945312499999996,
                            1.0,
                        ],
                        [
                            -11.500000000000002,
                            -8.354166666666668,
                            0.0005859375000000001,
                            1.0,
                        ],
                        [-11.75, -8.333333333333334, 0.0007812499999999999, 1.0],
                        [
                            -11.999999999999998,
                            -8.333333333333332,
                            0.0010416666666666664,
                            0.9999999999999999,
                        ],
                    ],
                ),
                (
                    [0.5, 1.0],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -12.0,
                            -8.333333333333332,
                            0.0010416666666666656,
                            0.9999999999999999,
                        ],
                        [-12.25, -8.333333333333334, 0.001302083333333334, 1.0],
                        [-12.5, -8.35416666666667, 0.0016276041666666678, 1.0],
                        [
                            -12.744791666666664,
                            -8.395833333333334,
                            0.002034505208333333,
                            0.9999999999999999,
                        ],
                        [-12.989583333333334, -8.437500000000002, 0.00244140625, 1.0],
                        [-13.229166666666668, -8.5, 0.0029296875, 1.0],
                        [
                            -13.458333333333334,
                            -8.583333333333334,
                            0.0035156250000000005,
                            1.0,
                        ],
                        [-13.916666666666668, -8.749999999999998, 0.0046875, 1.0],
                        [-14.333333333333334, -9.0, 0.0062499999999999995, 1.0],
                        [
                            -14.666666666666666,
                            -9.333333333333334,
                            0.008333333333333333,
                            1.0,
                        ],
                    ],
                ),
            ],
            vec![
                (
                    [0.0, 0.5],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -14.66666666666667,
                            -9.333333333333337,
                            0.008333333333333335,
                            1.0000000000000002,
                        ],
                        [
                            -15.00000000000001,
                            -9.666666666666673,
                            0.010416666666666675,
                            1.0000000000000002,
                        ],
                        [
                            -15.250000000000004,
                            -10.083333333333336,
                            0.013020833333333344,
                            1.0000000000000002,
                        ],
                        [
                            -15.41666666666667,
                            -10.54166666666667,
                            0.015755208333333343,
                            1.0000000000000002,
                        ],
                        [
                            -15.500000000000002,
                            -10.770833333333336,
                            0.017122395833333345,
                            1.0000000000000002,
                        ],
                        [
                            -15.562500000000002,
                            -11.010416666666671,
                            0.01852213541666668,
                            1.0000000000000002,
                        ],
                        [
                            -15.604166666666666,
                            -11.255208333333334,
                            0.019905598958333336,
                            1.0,
                        ],
                        [
                            -15.645833333333334,
                            -11.500000000000002,
                            0.021289062500000004,
                            1.0,
                        ],
                        [-15.666666666666666, -11.75, 0.02265625, 1.0],
                        [
                            -15.666666666666664,
                            -11.999999999999998,
                            0.023958333333333328,
                            0.9999999999999999,
                        ],
                    ],
                ),
                (
                    [0.5, 1.0],
                    3,
                    vec![
                        0.0, 0.0, 0.0, 0.0, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0,
                    ],
                    vec![
                        [
                            -15.666666666666664,
                            -12.0,
                            0.023958333333333325,
                            0.9999999999999999,
                        ],
                        [-15.666666666666666, -12.25, 0.025260416666666667, 1.0],
                        [-15.645833333333334, -12.5, 0.026497395833333333, 1.0],
                        [
                            -15.604166666666664,
                            -12.744791666666664,
                            0.027620442708333328,
                            0.9999999999999999,
                        ],
                        [-15.5625, -12.989583333333334, 0.028743489583333337, 1.0],
                        [-15.5, -13.229166666666668, 0.029752604166666672, 1.0],
                        [
                            -15.416666666666666,
                            -13.458333333333334,
                            0.03059895833333334,
                            1.0,
                        ],
                        [-15.25, -13.916666666666668, 0.03229166666666668, 1.0],
                        [-15.0, -14.333333333333334, 0.03333333333333334, 1.0],
                        [
                            -14.666666666666666,
                            -14.666666666666666,
                            0.03333333333333335,
                            1.0,
                        ],
                    ],
                ),
            ],
        ];
        let before = sides.clone();
        let tensor = run(&sides).unwrap();
        assert_eq!(sides, before);
        assert_eq!(tensor.degree, [3, 3]);
        assert_eq!(tensor.controls.len(), 19 * 19);
        assert!(tensor.controls.iter().any(|p| p[3] != 1.0));
    }
    #[test]
    fn all_four_rounded_corners_keep_nonplanar_positive_jacobian_after_exact_half_splits() {
        // The XY Coons map has:
        // x_u=.75+7.5u(1-u)+1.5v(1-v), y_v with u/v exchanged,
        // x_v=y_u=-.75(2u-1)(2v-1). Thus its Z normal is strictly
        // positive everywhere except the four original corners, exactly zero there.
        // A quartic side bump in Z changes neither this projected Jacobian nor any
        // endpoint tangent, and makes the cap genuinely nonplanar.
        let bottom = [
            [-1., -1., 0., 1.],
            [-0.75, -1.25, 0., 1.],
            [0.75, -1.25, 0., 1.],
            [1., -1., 0., 1.],
        ];
        let cubic = Space::polynomial(3, 0., 1.).unwrap();
        let quartic = Space::polynomial(4, 0., 1.).unwrap();
        let base = embed(&cubic, &bottom, &quartic).unwrap();
        let split = Space::new(
            4,
            vec![0., 0., 0., 0., 0., 0.5, 0.5, 0.5, 0.5, 1., 1., 1., 1., 1.],
        )
        .unwrap();
        for split_sides in [false, true] {
            let mut row = base.clone();
            let sides: [Vec<OwnedPiece>; 4] = std::array::from_fn(|side| {
                let mut controls = row.clone();
                if side == 0 {
                    controls[2][2] = 0.0625;
                }
                let pieces = if split_sides {
                    let refined = embed(&quartic, &controls, &split).unwrap();
                    vec![
                        (
                            [0., 0.5],
                            4,
                            quartic.knots().to_vec(),
                            refined[..5].to_vec(),
                        ),
                        (
                            [0.5, 1.],
                            4,
                            quartic.knots().to_vec(),
                            refined[4..].to_vec(),
                        ),
                    ]
                } else {
                    vec![([0., 1.], 4, quartic.knots().to_vec(), controls)]
                };
                row = row.iter().map(|&[x, y, z, w]| [-y, x, z, w]).collect();
                pieces
            });
            let tensor = run(&sides).unwrap();
            let size = [tensor.knots[0].len() - 5, tensor.knots[1].len() - 5];
            let center = nurbs_derivs(
                tensor.degree,
                &tensor.knots,
                size,
                &tensor.controls,
                [0.5, 0.5],
                |h| *h,
            )
            .unwrap()
            .0;
            assert!(
                (center.z - 3.0 / 256.0).abs() < 1e-14,
                "split={split_sides}"
            );
        }
    }
}
