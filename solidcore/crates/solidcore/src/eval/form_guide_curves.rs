//! Bounded exact rational restrictions of native Form guide curves.

use super::{form, geometry::Curve, pcurve::insert_guide_knot};
type Result<T> = std::result::Result<T, String>;
/// Largest knot or control array admitted to native guide extraction.
pub const MAX_CONTROLS: usize = 16_384;
const MAX_DEGREE: usize = 32;

fn spend(remaining: &mut usize, work: usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(work)
        .ok_or("exact native Form guide extraction exceeded its work budget")?;
    Ok(())
}

/// Extract the exact Bezier pieces of a native curve on its intrinsic interval.
pub fn source_curve(
    source: &Curve,
    range: [f64; 2],
    remaining: &mut usize,
) -> Result<Vec<Vec<[f64; 4]>>> {
    match source {
        source @ (Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. }) => {
            spend(remaining, 128)?;
            let (form, form_interval) = form::curve(source, range).ok_or("native Form conic has no bounded exact rational form")?;
            // form_interval is essential: angle != form parameter on conics.
            spline_record(&form, form_interval, remaining)
        }
        source @ (Curve::Spline { .. } | Curve::RationalSpline { .. }) => spline_record(source, range, remaining),
        Curve::Polyline { points } => {
            if points.len() < 2 || points.len() > MAX_CONTROLS {
                return Err("native Form polyline exceeds its exact source budget".into());
            }
            spend(remaining, points.len() * 4)?;
            let mut knots = vec![0.0, 0.0];
            knots.extend((1..points.len() - 1).map(|index| index as f64));
            knots.extend([points.len() as f64 - 1.0; 2]);
            let controls = points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect();
            restrict_spline(knots, controls, range, remaining)
        }
        Curve::SurfaceIntersection { .. } => {
            Err("procedural native Form intersection guides need a certified curve restriction; their leader is not an exact replacement".into())
        }
    }
}
fn spline_record(
    source: &Curve,
    range: [f64; 2],
    remaining: &mut usize,
) -> Result<Vec<Vec<[f64; 4]>>> {
    let (knots, count) = match source {
        Curve::Spline { knots, points } => (knots.as_slice(), points.len()),
        Curve::RationalSpline { knots, homogeneous } => (knots.as_slice(), homogeneous.len()),
        _ => return Err("invalid native rational Form curve form".into()),
    };
    if count > MAX_CONTROLS || knots.len() > MAX_CONTROLS {
        return Err("native Form spline exceeds its borrowed-size budget".into());
    }
    spend(
        remaining,
        count
            .checked_mul(4)
            .and_then(|work| work.checked_add(knots.len()))
            .ok_or("native Form spline work overflowed")?,
    )?;
    let controls = match source {
        Curve::Spline { points, .. } => points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
        Curve::RationalSpline { homogeneous, .. } => homogeneous.clone(),
        _ => return Err("invalid native Form spline coefficients".into()),
    };
    restrict_spline(knots.to_vec(), controls, range, remaining)
}
fn restrict_spline(
    mut knots: Vec<f64>,
    mut controls: Vec<[f64; 4]>,
    range @ [low, high]: [f64; 2],
    remaining: &mut usize,
) -> Result<Vec<Vec<[f64; 4]>>> {
    let degree = knots
        .len()
        .checked_sub(controls.len() + 1)
        .ok_or("invalid native Form spline degree")?;
    if range.iter().any(|value| !value.is_finite())
        || low >= high
        || degree == 0
        || degree > MAX_DEGREE
        || controls.len() <= degree
        || controls.len() > MAX_CONTROLS
        || controls.iter().any(|point| {
            !point.iter().all(|v| v.is_finite())
                || point.get(3).is_none_or(|w| *w <= f64::MIN_POSITIVE)
        })
        || knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| matches!(pair, [a, b] if a > b))
        || knots.get(degree).is_none_or(|value| low < *value)
        || knots.get(controls.len()).is_none_or(|value| high > *value)
    {
        return Err(
            "native Form spline restriction leaves its finite positive-weight domain".into(),
        );
    }
    for endpoint in range {
        for _ in knots.iter().filter(|value| **value == endpoint).count()..=degree {
            spend(remaining, controls.len() * (degree + 1))?;
            (knots, controls) = insert_guide_knot(degree, &knots, &controls, endpoint)?;
        }
    }
    let first = knots
        .iter()
        .position(|value| *value == low)
        .ok_or("missing native Form curve restriction start")?;
    let last = knots
        .iter()
        .rposition(|value| *value == high)
        .ok_or("missing native Form curve restriction end")?
        + 1;
    let count = last
        .checked_sub(first)
        .and_then(|n| n.checked_sub(degree + 1))
        .ok_or("invalid native Form curve restriction count")?;
    controls = controls
        .get(first..first + count)
        .ok_or("missing native Form restricted controls")?
        .to_vec();
    knots = knots
        .get(first..last)
        .ok_or("missing native Form restricted knots")?
        .to_vec();
    let original = knots.clone();
    for run in original.chunk_by(|a, b| a == b) {
        let value = *run.first().ok_or("empty native Form knot run")?;
        if value <= low || value >= high {
            continue;
        }
        for _ in run.len()..degree {
            spend(remaining, controls.len() * (degree + 1))?;
            (knots, controls) = insert_guide_knot(degree, &knots, &controls, value)?;
            if controls.len() > MAX_CONTROLS {
                return Err(
                    "native Form curve Bezier extraction exceeds its coefficient budget".into(),
                );
            }
        }
    }
    Ok(bezier_pieces(degree, &knots, &controls, remaining)?
        .into_iter()
        .map(|piece| piece.controls)
        .collect())
}

/// A source interval and its exact homogeneous Bezier controls.
/// A rational Bézier span and its source parameter interval.
pub struct BezierPiece {
    /// Original B-spline parameter span. The controls use a local 0..1 parameter.
    pub parameter: [f64; 2],
    /// Exact world homogeneous controls, in Bernstein order.
    pub controls: Vec<[f64; 4]>,
}

/// Read Bernstein controls from a clamped spline split at its interior knots.
/// Each interior multiplicity must be the degree or one higher. The caller
/// shares the work budget between extraction, projection and classification.
pub fn bezier_pieces(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    remaining: &mut usize,
) -> std::result::Result<Vec<BezierPiece>, &'static str> {
    let count = controls.len();
    let expected_knots = count
        .checked_add(degree)
        .and_then(|n| n.checked_add(1))
        .ok_or("invalid exact Form curve size")?;
    if degree == 0
        || degree > MAX_DEGREE
        || count <= degree
        || count > MAX_CONTROLS
        || knots.len() != expected_knots
    {
        return Err("invalid exact Form Bezier source structure");
    }
    let cost = count
        .checked_add(knots.len())
        .ok_or("exact Form curve extraction exceeded its work budget")?;
    *remaining = remaining
        .checked_sub(cost)
        .ok_or("exact Form curve extraction exceeded its work budget")?;
    if knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| matches!(pair, [a, b] if a > b))
        || controls.iter().any(|control| {
            control.iter().any(|value| !value.is_finite())
                || control.get(3).is_none_or(|weight| *weight <= 0.0)
        })
    {
        return Err("exact Form curve requires finite knots and positive rational weights");
    }
    let low = *knots.get(degree).ok_or("missing exact Form curve start")?;
    let high = *knots.get(count).ok_or("missing exact Form curve end")?;
    if low >= high
        || knots
            .get(..=degree)
            .is_none_or(|knots| knots.iter().any(|value| *value != low))
        || knots
            .get(count..)
            .is_none_or(|knots| knots.iter().any(|value| *value != high))
    {
        return Err("exact Form Bezier source must have a bounded clamped domain");
    }
    // Interior multiplicity >= degree is the extraction certificate. Degree+1
    // also permits a discontinuous exact source; its two sides stay separate.
    let interior = knots
        .get(degree + 1..count)
        .ok_or("invalid exact Form curve interior")?;
    if interior.chunk_by(|a, b| a == b).any(|run| {
        run.len() < degree
            || run.len() > degree + 1
            || run
                .first()
                .is_none_or(|value| *value <= low || *value >= high)
    }) {
        return Err("exact Form curve is not already split into Bezier spans");
    }
    let mut pieces = Vec::new();
    for (span, interval) in knots
        .windows(2)
        .enumerate()
        .skip(degree)
        .take(count - degree)
    {
        let &[a, b] = interval else {
            return Err("invalid exact Form knot span");
        };
        if a == b {
            continue;
        }
        *remaining = remaining
            .checked_sub(degree + 1)
            .ok_or("exact Form curve extraction exceeded its work budget")?;
        let controls = controls
            .get(span - degree..=span)
            .ok_or("missing exact Form Bezier span controls")?;
        pieces.push(BezierPiece {
            parameter: [a, b],
            controls: controls.to_vec(),
        });
    }
    if pieces.is_empty() {
        return Err("exact Form curve has no Bezier spans");
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restriction_retains_rational_controls_and_original_conic_interval() {
        let source = Curve::Circle {
            frame: super::super::geometry::Frame {
                origin: [0.0, 0.0, 0.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius: 3.0,
        };
        let range = [0.2_f64, 1.7_f64];
        let controls = source_curve(&source, range, &mut 4096).unwrap();
        assert_eq!(controls.len(), 2);
        for (control, parameter) in [
            (controls[0][0], range[0]),
            (*controls[1].last().unwrap(), range[1]),
        ] {
            let [x, y, z, w] = control;
            assert!((x / w - 3.0 * parameter.cos()).abs() < 1e-12);
            assert!((y / w - 3.0 * parameter.sin()).abs() < 1e-12);
            assert_eq!(z, 0.0);
        }
        assert!(source_curve(&source, range, &mut 32).is_err());
    }
}
