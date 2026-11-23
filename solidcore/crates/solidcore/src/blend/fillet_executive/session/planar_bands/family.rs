//! Retained witnesses of the complete parallel-line minimum family.
//!
//! Upstream: none. Native line maps and all projections stay homogeneous; exact
//! quadratic strip and cap exclusions are subtracted by `interval`. No stations,
//! approximate roots, geometry tolerances, or rebuilds select the witness.
//!
//! Deviations: no recursion. Unsupported selected corner classes retain their
//! existing certification limit; arithmetic range failures are typed errors.

use super::interval::{self, add, mul, scale, sub, Ratio};
use super::{endpoint_cap, inward, kept_witness, Band, ContactWitness, CornerBand, OP};
use crate::curve::line::Line;
use crate::error::blend::BandInput;
use crate::error::{Result, SolidError};
use crate::geom::expansion as exact;
use crate::geom::tol::is_zero;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::topology::{edge, Brep, EdgeId};

type Vector = [Vec<f64>; 3];

fn range() -> SolidError {
    SolidError::invalid_input(OP, "parallel contact outside exact arithmetic range")
}

fn vector(v: Vec3) -> Vector {
    [vec![v.x], vec![v.y], vec![v.z]]
}

fn plus(a: &Vector, b: &Vector) -> Result<Vector> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([add(ax, bx)?, add(ay, by)?, add(az, bz)?])
}

fn minus(a: &Vector, b: &Vector) -> Result<Vector> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([sub(ax, bx)?, sub(ay, by)?, sub(az, bz)?])
}

fn times(a: &Vector, s: &[f64]) -> Result<Vector> {
    let [x, y, z] = a;
    Ok([mul(x, s)?, mul(y, s)?, mul(z, s)?])
}

fn dot(a: &Vector, b: &Vector) -> Result<Vec<f64>> {
    a.iter()
        .zip(b)
        .try_fold(Vec::new(), |sum, (x, y)| add(&sum, &mul(x, y)?))
}

fn cross(a: &Vector, b: &Vector) -> Result<Vector> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([
        sub(&mul(ay, bz)?, &mul(az, by)?)?,
        sub(&mul(az, bx)?, &mul(ax, bz)?)?,
        sub(&mul(ax, by)?, &mul(ay, bx)?)?,
    ])
}

fn ratio(numerator: Vec<f64>, denominator: Vec<f64>) -> Result<Ratio> {
    match exact::sign(&denominator) {
        1 => Ok(Ratio {
            numerator,
            denominator,
        }),
        -1 => Ok(Ratio {
            numerator: scale(&numerator, -1.0)?,
            denominator: scale(&denominator, -1.0)?,
        }),
        _ => Err(range()),
    }
}

fn quotient(r: &Ratio) -> Result<f64> {
    let n = exact::to_f64(&r.numerator);
    let d = exact::to_f64(&r.denominator);
    let value = n / d;
    if !(n.is_finite() && d.is_finite() && d > 0.0 && value.is_finite()) {
        return Err(range());
    }
    Ok(value)
}

/// The physical coordinate in P + coordinate V, using the stored native map.
/// Its divisions stay rational until the final native curve evaluation.
fn coordinate(line: &Line, t: f64) -> Result<Ratio> {
    let natural = line.natural_interval();
    let analytic = line.step_interval();
    let step = if line.is_bounded() && !is_zero(natural.length()) {
        let (start, end) = if line.inside_out() {
            (natural.max(), natural.min())
        } else {
            (natural.min(), natural.max())
        };
        let denominator = sub(&[end], &[start])?;
        ratio(
            add(
                &scale(&denominator, analytic.min())?,
                &mul(
                    &sub(&[t], &[start])?,
                    &sub(&[analytic.max()], &[analytic.min()])?,
                )?,
            )?,
            denominator,
        )?
    } else {
        ratio(
            vec![if !line.is_bounded() && line.inside_out() {
                -t
            } else {
                t
            }],
            vec![1.0],
        )?
    };
    ratio(scale(&step.numerator, line.line_scale())?, step.denominator)
}

/// |origin + t step|^2 - bound, in ascending powers of t.
fn distance_polynomial(origin: &Vector, step: &Vector, bound: &[f64]) -> Result<[Vec<f64>; 3]> {
    Ok([
        sub(&dot(origin, origin)?, bound)?,
        scale(&dot(origin, step)?, 2.0)?,
        dot(step, step)?,
    ])
}

/// Both feet have one positive denominator and the same affine displacement.
struct Family {
    p: Vector,
    q: Vector,
    step: Vector,
    denominator: Vec<f64>,
}

impl Family {
    fn strip(&self, widths: [f64; 2], line: (Point3, Vec3, f64)) -> Result<[Vec<f64>; 3]> {
        let [wa, wb] = widths;
        let (origin, tangent, width) = line;
        let total = add(&[wa], &[wb])?;
        let contact = minus(
            &plus(&times(&self.p, &[wb])?, &times(&self.q, &[wa])?)?,
            &times(&vector(origin), &mul(&self.denominator, &total)?)?,
        )?;
        let tangent = vector(tangent);
        let gap = minus(&self.q, &self.p)?;
        let bound = mul(
            &mul(&dot(&gap, &gap)?, &mul(&[width], &[width])?)?,
            &dot(&tangent, &tangent)?,
        )?;
        distance_polynomial(
            &cross(&contact, &tangent)?,
            &cross(&times(&self.step, &total)?, &tangent)?,
            &bound,
        )
    }

    fn cap(&self, foot: &Vector, endpoint: Point3, tolerance: f64) -> Result<[Vec<f64>; 3]> {
        let origin = minus(foot, &times(&vector(endpoint), &self.denominator)?)?;
        let radius = scale(&self.denominator, tolerance)?;
        distance_polynomial(&origin, &self.step, &mul(&radius, &radius)?)
    }

    fn points(
        &self,
        k: &Kernel,
        a: &Band,
        b: &Band,
        lines: [&Line; 2],
        t: f64,
    ) -> Result<[Point3; 2]> {
        let [line_a, line_b] = lines;
        Ok([
            a.curve
                .evaluate_point(k, self.native_parameter(&self.p, line_a, t)?)?,
            b.curve
                .evaluate_point(k, self.native_parameter(&self.q, line_b, t)?)?,
        ])
    }

    fn native_parameter(&self, foot: &Vector, line: &Line, t: f64) -> Result<f64> {
        let position = plus(foot, &times(&self.step, &[t])?)?;
        let direction = vector(line.line_vector());
        let displacement = minus(
            &position,
            &times(&vector(line.line_point()), &self.denominator)?,
        )?;
        let step = ratio(
            dot(&displacement, &direction)?,
            scale(
                &mul(&self.denominator, &dot(&direction, &direction)?)?,
                line.line_scale(),
            )?,
        )?;
        let value = line.convert_t_from_step_to_nurbs(quotient(&step)?);
        if !value.is_finite() {
            return Err(range());
        }
        Ok(value)
    }
}

/// This is also the final certificate on rounded native evaluations.
pub(super) fn outside_corner_strip(
    witness: &ContactWitness,
    line: (Point3, Vec3, f64),
) -> Result<bool> {
    let [p, q] = witness.feet;
    let family = Family {
        p: vector(p),
        q: vector(q),
        step: vector(Vec3::ZERO),
        denominator: vec![1.0],
    };
    let [constant, _, _] = family.strip(witness.widths, line)?;
    Ok(exact::sign(&constant) > 0)
}

pub(super) fn feet(
    k: &Kernel,
    target: &Brep,
    a: &Band,
    b: &Band,
    lines: [&Line; 2],
    blended: &[EdgeId],
    corners: &[CornerBand],
) -> Result<Option<[Point3; 2]>> {
    let [line_a, line_b] = lines;
    let pa = vector(line_a.line_point());
    let pb = vector(line_b.line_point());
    let va = vector(line_a.line_vector());
    let vb = vector(line_b.line_vector());
    let direction_squared = dot(&va, &va)?;
    let displacement = minus(&pb, &pa)?;
    let projection = dot(&displacement, &va)?;
    let project_b = |t| -> Result<Ratio> {
        let s = coordinate(line_b, t)?;
        ratio(
            add(
                &mul(&projection, &s.denominator)?,
                &mul(&dot(&vb, &va)?, &s.numerator)?,
            )?,
            mul(&direction_squared, &s.denominator)?,
        )
    };
    let mut lo = coordinate(line_a, a.interval.min())?;
    let mut hi = coordinate(line_a, a.interval.max())?;
    if interval::compare_ratios(&lo, &hi)?.is_gt() {
        core::mem::swap(&mut lo, &mut hi);
    }
    let mut b_lo = project_b(b.interval.min())?;
    let mut b_hi = project_b(b.interval.max())?;
    if interval::compare_ratios(&b_lo, &b_hi)?.is_gt() {
        core::mem::swap(&mut b_lo, &mut b_hi);
    }
    if interval::compare_ratios(&b_lo, &lo)?.is_gt() {
        lo = b_lo;
    }
    if interval::compare_ratios(&b_hi, &hi)?.is_lt() {
        hi = b_hi;
    }
    if interval::compare_ratios(&lo, &hi)?.is_gt() {
        return Ok(None);
    }

    // u=(u0+du t)/K on the exact overlapping projection, t in [0,1].
    let denominator_u = mul(&lo.denominator, &hi.denominator)?;
    let u0 = mul(&lo.numerator, &hi.denominator)?;
    let du = sub(&mul(&hi.numerator, &lo.denominator)?, &u0)?;
    let denominator = mul(&direction_squared, &denominator_u)?;
    let p = plus(
        &times(&pa, &denominator)?,
        &times(&va, &mul(&direction_squared, &u0)?)?,
    )?;
    let q = plus(
        &times(&pb, &denominator)?,
        &times(
            &va,
            &sub(
                &mul(&direction_squared, &u0)?,
                &mul(&denominator_u, &projection)?,
            )?,
        )?,
    )?;
    let family = Family {
        p,
        q,
        step: times(&va, &mul(&direction_squared, &du)?)?,
        denominator,
    };
    // A straight support has constant inward normals. Classify their contact
    // before doing any strip arithmetic for this minimizing family.
    if super::normal_contact(k, a, b, family.points(k, a, b, lines, 0.5)?)?.is_none() {
        return Ok(None);
    }
    if exact::is_zero(&du) {
        // A singleton has no interior family to clip. Fixed-cap endpoint
        // certificates decide this one contact, including unsupported remote corners.
        let points = family.points(k, a, b, lines, 0.5)?;
        let [p, q] = points;
        let witness = ContactWitness {
            feet: points,
            widths: [a.width, b.width],
        };
        if !kept_witness(
            k,
            target,
            a,
            p,
            inward(k, a, p)?,
            blended,
            corners,
            &witness,
        )? || !kept_witness(
            k,
            target,
            b,
            q,
            inward(k, b, q)?,
            blended,
            corners,
            &witness,
        )? {
            return Ok(None);
        }
        return Ok(Some(points));
    }
    let mut forbidden = Vec::new();
    let ends = [
        edge::start_vertex(target, a.edge)?,
        edge::end_vertex(target, a.edge)?,
        edge::start_vertex(target, b.edge)?,
        edge::end_vertex(target, b.edge)?,
    ];
    for corner in corners {
        if corner.edge == a.edge
            || corner.edge == b.edge
            || !ends
                .iter()
                .flatten()
                .any(|v| corner.ends.contains(&Some(*v)))
        {
            continue;
        }
        let Some(line) = corner.line else {
            return Ok(None);
        };
        forbidden.push(family.strip([a.width, b.width], line)?);
    }
    for (band, foot) in [(a, &family.p), (b, &family.q)] {
        for (parameter, vertex) in [
            (band.interval.min(), edge::start_vertex(target, band.edge)?),
            (band.interval.max(), edge::end_vertex(target, band.edge)?),
        ] {
            let endpoint = band.curve.evaluate_point(k, parameter)?;
            let vertex = vertex
                .ok_or_else(|| SolidError::blend_band_input_missing(OP, BandInput::LineEndpoint))?;
            if !endpoint_cap(k, target, band, vertex, inward(k, band, endpoint)?, blended)? {
                forbidden.push(family.cap(foot, endpoint, edge::tolerance(target, band.edge)?)?);
            }
        }
    }
    let Some(t) = interval::retained_parameter(&forbidden)? else {
        return Ok(None);
    };
    Ok(Some(family.points(k, a, b, lines, t)?))
}
