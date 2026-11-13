//! Exact extraction for source restrictions already split into rational Bezier spans.
//! No fitting, knot insertion, screen sampling, or private geom APIs.
use solidcraft_engine::geom::bspline::{CurveN, HPoint};

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
    solidcore_ops::form_guide_curves::bezier_pieces(curve.degree, &curve.knots, &curve.ctrl, remaining).map(|pieces| {
        pieces
            .into_iter()
            .map(|piece| BezierPiece {
                #[cfg(test)]
                parameter: piece.parameter,
                controls: piece.controls.into_iter().map(|[x, y, z, w]| HPoint { x, y, z, w }).collect(),
            })
            .collect()
    })
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
