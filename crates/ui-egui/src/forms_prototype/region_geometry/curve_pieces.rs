//! Exact extraction for source restrictions already split into rational Bezier spans.
//! No fitting, knot insertion, screen sampling, or private geom APIs.
use solidcraft_engine::geom::bspline::{CurveN, HPoint};

const MAX_DEGREE: usize = 32;
const MAX_SOURCE_CONTROLS: usize = 16_384;

pub(super) struct BezierPiece {
    /// Original B-spline parameter span. The controls use a local 0..1 parameter.
    #[cfg(test)]
    pub(super) parameter: [f64; 2],
    /// Exact world homogeneous controls, suitable for projected_curve::project_controls.
    pub(super) controls: Vec<HPoint>,
}

/// `EdgeCurve::rational_pieces` returns clamped splines whose interior knot
/// multiplicities are their degree. Such a span's active controls already are
/// its Bernstein controls. Refuse other knot structures instead of treating a
/// general B-spline span's controls as a Bezier polygon.
/// The budget is shared with projection and region classification by the caller.
pub(super) fn bezier_pieces(curve: &CurveN<4>, remaining: &mut usize) -> Result<Vec<BezierPiece>, &'static str> {
    let degree = curve.degree;
    let count = curve.ctrl.len();
    let expected_knots = count.checked_add(degree).and_then(|n| n.checked_add(1)).ok_or("invalid exact Form curve size")?;
    if degree == 0 || degree > MAX_DEGREE || count <= degree || count > MAX_SOURCE_CONTROLS || curve.knots.len() != expected_knots {
        return Err("invalid exact Form Bezier source structure");
    }
    let cost = count.checked_add(curve.knots.len()).ok_or("exact Form curve extraction exceeded its work budget")?;
    *remaining = remaining.checked_sub(cost).ok_or("exact Form curve extraction exceeded its work budget")?;
    if curve.knots.iter().any(|value| !value.is_finite())
        || curve.knots.windows(2).any(|pair| pair[0] > pair[1])
        || curve.ctrl.iter().any(|control| control.iter().any(|value| !value.is_finite()) || control[3] <= 0.0)
    {
        return Err("exact Form curve requires finite knots and positive rational weights");
    }
    let low = *curve.knots.get(degree).ok_or("missing exact Form curve start")?;
    let high = *curve.knots.get(count).ok_or("missing exact Form curve end")?;
    if low >= high
        || curve.knots.get(..=degree).is_none_or(|knots| knots.iter().any(|value| *value != low))
        || curve.knots.get(count..).is_none_or(|knots| knots.iter().any(|value| *value != high))
    {
        return Err("exact Form Bezier source must have a bounded clamped domain");
    }
    // Interior multiplicity >= degree is the extraction certificate. Degree+1
    // also permits a discontinuous exact source; its two sides stay separate.
    let interior = curve.knots.get(degree + 1..count).ok_or("invalid exact Form curve interior")?;
    if interior.chunk_by(|a, b| a == b).any(|run| run.len() < degree || run.len() > degree + 1 || run[0] <= low || run[0] >= high) {
        return Err("exact Form curve is not already split into Bezier spans");
    }
    let mut pieces = Vec::new();
    for (span, interval) in curve.knots.windows(2).enumerate().skip(degree).take(count - degree) {
        let parameter = [interval[0], interval[1]];
        if parameter[0] == parameter[1] {
            continue;
        }
        *remaining = remaining.checked_sub(degree + 1).ok_or("exact Form curve extraction exceeded its work budget")?;
        let controls = curve.ctrl.get(span - degree..=span).ok_or("missing exact Form Bezier span controls")?;
        pieces.push(BezierPiece {
            #[cfg(test)]
            parameter,
            controls: controls.iter().map(|point| HPoint { x: point[0], y: point[1], z: point[2], w: point[3] }).collect(),
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

    fn source() -> CurveN<4> {
        CurveN {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 0.4, 0.4, 1.0, 1.0, 1.0],
            ctrl: vec![[0.0, 0.0, 0.0, 1.0], [2.0, 4.0, 0.0, 2.0], [2.0, 0.0, 0.0, 1.0], [6.0, -4.0, 0.0, 2.0], [4.0, 0.0, 0.0, 1.0]],
        }
    }
    #[test]
    fn extraction_preserves_exact_weighted_geometry_and_original_spans() {
        let curve = source();
        let pieces = bezier_pieces(&curve, &mut 1024).unwrap();
        assert_eq!(pieces.iter().map(|piece| piece.parameter).collect::<Vec<_>>(), vec![[0.0, 0.4], [0.4, 1.0]]);
        for piece in pieces {
            for t in [0.0, 0.21, 0.73, 1.0] {
                let mut row = piece.controls.clone();
                for count in (1..row.len()).rev() {
                    for index in 0..count {
                        row[index] = row[index] * (1.0 - t) + row[index + 1] * t;
                    }
                }
                let actual = row[0].project().unwrap();
                let parameter = piece.parameter[0] * (1.0 - t) + piece.parameter[1] * t;
                let (expected, _) = curve.eval(parameter).unwrap();
                assert!((actual.x - expected[0] / expected[3]).abs() < 1e-12);
                assert!((actual.y - expected[1] / expected[3]).abs() < 1e-12);
                assert!((actual.z - expected[2] / expected[3]).abs() < 1e-12);
            }
        }
    }
    #[test]
    fn extraction_refuses_general_splines_instead_of_using_their_control_polygons() {
        let mut curve = source();
        curve.knots.remove(4);
        curve.ctrl.remove(2);
        assert!(bezier_pieces(&curve, &mut 1024).is_err());
        let mut discontinuous = source();
        discontinuous.knots.insert(4, 0.4);
        discontinuous.ctrl.insert(3, [20.0, 0.0, 0.0, 1.0]);
        let pieces = bezier_pieces(&discontinuous, &mut 1024).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_ne!(pieces[0].controls.last().unwrap().project(), pieces[1].controls.first().unwrap().project());
    }
    #[test]
    fn extraction_refuses_hostile_sizes_weights_knots_and_exhausted_work() {
        assert!(bezier_pieces(&source(), &mut 1).is_err());
        let mut curve = source();
        curve.degree = usize::MAX;
        assert!(bezier_pieces(&curve, &mut 1024).is_err());
        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut curve = source();
            curve.ctrl[0][3] = bad;
            assert!(bezier_pieces(&curve, &mut 1024).is_err());
        }
        let mut curve = source();
        curve.knots[3] = 0.6;
        assert!(bezier_pieces(&curve, &mut 1024).is_err());
        let mut curve = source();
        curve.knots[0] = -1.0;
        assert!(bezier_pieces(&curve, &mut 1024).is_err());
    }
}
