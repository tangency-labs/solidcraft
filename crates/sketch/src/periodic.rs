//! Explicit cyclic C2 fit splines. Missing periodic markers retain natural interpolation.
use crate::{MAX_SPLINE_POINTS, Sketch, SketchError};
use solidcore_record::Vec2;
#[cfg(test)]
use solidcraft_geom::prelude::*;

type Result<T> = std::result::Result<T, SketchError>;
fn bad() -> SketchError {
    SketchError::Invalid("periodic fit spline needs 3 distinct cyclic samples, an exact closing point, and regular bounded chords".into())
}

pub(crate) fn validate_periodic_points(points: &[Vec2]) -> Result<()> {
    if points.len() > MAX_SPLINE_POINTS {
        return Err(bad());
    }
    solidcore_ops::fit_projection::check_periodic(points).map_err(|_| bad())
}

/// A closed control spline's rule: a degree from 3 to the kernel's Bézier limit, and at least
/// `degree + 1` control points (see [`crate::nurbs::closed_control_beziers`]).
pub fn validate_closed_control(points: usize, degree: u8) -> Result<()> {
    let max = solidcore_record::MAX_BEZIER_DEGREE;
    if !(3..=max).contains(&usize::from(degree)) {
        return Err(SketchError::Invalid(format!("a closed control spline has degree 3 to {max}")));
    }
    let least = usize::from(degree) + 1;
    if !(least..=MAX_SPLINE_POINTS).contains(&points) {
        return Err(SketchError::Invalid(format!("a closed control spline of degree {degree} needs {least} to {MAX_SPLINE_POINTS} control points")));
    }
    Ok(())
}

pub fn periodic_beziers(points: &[Vec2]) -> Result<Vec<[Vec2; 4]>> {
    validate_periodic_points(points)?;
    solidcore_ops::fit_projection::fit_beziers(points, true).map_err(|e| SketchError::Invalid(e.to_string()))
}

impl Sketch {
    pub fn validate_periodic_splines(&self) -> Result<()> {
        let mut work = 0usize;
        for curve in &self.curves {
            if let crate::CurveKind::Spline { pts, control, degree, periodic: true, .. } = &curve.kind {
                work = work.saturating_add(pts.len());
                if work > 1_000_000 || pts.len() > MAX_SPLINE_POINTS {
                    return Err(bad());
                }
                let points = pts.iter().map(|i| self.point(*i).ok_or_else(bad)).collect::<Result<Vec<_>>>()?;
                if *control {
                    validate_closed_control(pts.len(), *degree)?;
                    if points.iter().any(|p| !p.is_finite()) {
                        return Err(SketchError::Invalid("a closed control spline's points must be finite".into()));
                    }
                } else {
                    if *degree != 3 {
                        return Err(bad());
                    }
                    validate_periodic_points(&points)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn periodic_c2_unequal_chords_and_legacy_natural_are_distinct() {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(3.0, 2.0), Vec2::new(-1.0, 1.0), Vec2::new(0.0, 0.0)];
        let curves = periodic_beziers(&p).unwrap();
        for i in 0..curves.len() {
            let j = (i + 1) % curves.len();
            let (a, b) = (curves[i], curves[j]);
            let (ha, hb) = (p[i].dist(p[i + 1]), p[j].dist(p[j + 1]));
            assert_eq!(a[3], b[0]);
            assert!(((a[3] - a[2]) * (3.0 / ha) - (b[1] - b[0]) * (3.0 / hb)).len() < 1e-12);
            assert!(((a[3] - a[2] * 2.0 + a[1]) * (6.0 / (ha * ha)) - (b[2] - b[1] * 2.0 + b[0]) * (6.0 / (hb * hb))).len() < 1e-12);
        }
        let legacy = crate::fit_beziers(&p);
        assert!(
            (legacy[0][1] - legacy[0][0]).normalized().unwrap().dist((legacy.last().unwrap()[3] - legacy.last().unwrap()[2]).normalized().unwrap())
                > 0.01
        );
        let global = |t| crate::spline_point_mode(&p, false, 3, true, None, t);
        let h = 1e-5;
        assert!(((global(h) - global(0.0)) / h - (global(1.0) - global(1.0 - h)) / h).len() < 0.002);
    }
    #[test]
    fn periodic_rejects_two_samples_and_hostile_geometry() {
        let a = Vec2::ZERO;
        let b = Vec2::X;
        assert!(periodic_beziers(&[a, b, a, b, a]).is_err());
        assert!(periodic_beziers(&[a, b, Vec2::new(0.0, 1.0), b]).is_err());
        assert!(periodic_beziers(&[a, b, Vec2::new(f64::INFINITY, 1.0), a]).is_err());
        assert!(periodic_beziers(&vec![a; MAX_SPLINE_POINTS + 1]).is_err());
    }

    #[test]
    fn periodic_solver_closes_shared_roles_and_refuses_immutable_or_collapsed_nodes() {
        let mut sk = Sketch::new();
        let first = sk.add_point(Vec2::ZERO, None).unwrap();
        let a = sk.add_point(Vec2::new(4.0, 0.0), None).unwrap();
        let b = sk.add_point(Vec2::new(3.0, 2.0), None).unwrap();
        let seam = sk.add_point(Vec2::ZERO, None).unwrap();
        let c = sk.add_point(Vec2::new(-4.0, 0.0), None).unwrap();
        let d = sk.add_point(Vec2::new(-3.0, -2.0), None).unwrap();
        let last = sk.add_point(Vec2::ZERO, None).unwrap();
        sk.add_curve(
            crate::CurveKind::Spline { pts: vec![first, a, b, seam], control: false, degree: 3, periodic: true, nurbs: None, handles: Vec::new() },
            None,
        )
        .unwrap();
        sk.add_curve(
            crate::CurveKind::Spline { pts: vec![seam, c, d, last], control: false, degree: 3, periodic: true, nurbs: None, handles: Vec::new() },
            None,
        )
        .unwrap();
        sk.points[first].pos = Vec2::new(0.2, 0.1);
        assert!(crate::solve_with(&mut sk, &crate::Hold { points: vec![first], radii: vec![] }, None, crate::Analysis::Full, None).ok());
        assert_eq!(sk.point(first), sk.point(seam));
        assert_eq!(sk.point(first), sk.point(last));
        sk.validate_periodic_splines().unwrap();
        let mut fixed = sk.clone();
        fixed.points[first].fixed = true;
        fixed.points[seam].fixed = true;
        fixed.points[seam].pos.x += 1e-8;
        let before = fixed.clone();
        assert!(!crate::solve(&mut fixed).ok());
        assert_eq!(fixed, before);
        let mut collapsed = sk;
        collapsed.points[a].pos = collapsed.points[first].pos;
        let before = collapsed.clone();
        assert!(!crate::solve_with(&mut collapsed, &crate::Hold { points: vec![first, a], radii: vec![] }, None, crate::Analysis::Full, None).ok());
        assert_eq!(collapsed, before);
    }

    #[test]
    fn periodic_link_projection_keeps_mode_and_break_geometry() {
        let p = vec![Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(3.0, 2.0), Vec2::new(-1.0, 1.0), Vec2::ZERO];
        let mut sk = Sketch::new();
        let source = crate::LinkSource::SketchCurve { sketch: 99, curve: "source".into() };
        let geometry = crate::LinkGeom::Spline { pts: p.clone(), control: false, degree: 3, periodic: true, nurbs: None };
        let id = sk.add_link(crate::LinkKind::Project, source, &[geometry]).unwrap();
        let curve = sk.link_curves(&id)[0];
        assert!(matches!(sk.curves[curve].kind, crate::CurveKind::Spline { periodic: true, .. }));
        let before = sk.exact_segs(curve).unwrap();
        sk.break_link(&id).unwrap();
        assert_eq!(sk.exact_segs(curve).unwrap(), before);
        sk.validate_periodic_splines().unwrap();
        let mut singular = Sketch::new();
        let points =
            [Vec2::ZERO, Vec2::X, Vec2::ZERO, Vec2::new(0.0, 1.0), Vec2::ZERO].into_iter().map(|p| singular.add_point(p, None).unwrap()).collect();
        let curve = singular
            .add_curve(crate::CurveKind::Spline { pts: points, control: false, degree: 3, periodic: true, nurbs: None, handles: Vec::new() }, None)
            .unwrap();
        let before = singular.clone();
        assert!(singular.add_freeform_offset(curve, true, 0.1).is_err());
        assert_eq!(singular, before);
    }

    /// Six control points around a lopsided loop; the closed control spline wraps them.
    fn closed_control_points() -> Vec<Vec2> {
        [(0.0, 0.0), (12.0, -2.0), (20.0, 8.0), (10.0, 16.0), (-3.0, 12.0), (-6.0, 3.0)].into_iter().map(|(x, y)| Vec2::new(x, y)).collect()
    }

    /// A sketch with one closed control spline of `degree` over `cv`.
    fn closed_control_curve(cv: &[Vec2], degree: u8) -> (Sketch, usize) {
        let mut sk = Sketch::new();
        let pts = cv.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
        let curve = sk.add_curve(crate::CurveKind::Spline { pts, control: true, degree, periodic: true, nurbs: None, handles: Vec::new() }, None);
        (sk, curve.unwrap())
    }

    /// Segment `i` of the closed uniform cubic B-spline over `cv` at `t` in [0, 1]: the textbook
    /// closed form with indices wrapped (Piegl & Tiller, eq. 3.3 basis), independent of the knots.
    fn cyclic_cubic(cv: &[Vec2], i: usize, t: f64) -> Vec2 {
        let n = cv.len() as isize;
        let p = |k: isize| cv[k.rem_euclid(n) as usize];
        let i = i as isize;
        let (b0, b1, b2, b3) = (
            (-t * t * t + 3.0 * t * t - 3.0 * t + 1.0) / 6.0,
            (3.0 * t * t * t - 6.0 * t * t + 4.0) / 6.0,
            (-3.0 * t * t * t + 3.0 * t * t + 3.0 * t + 1.0) / 6.0,
            t * t * t / 6.0,
        );
        p(i - 1) * b0 + p(i) * b1 + p(i + 1) * b2 + p(i + 2) * b3
    }

    /// Area enclosed by a closed loop of points (the last joins the first).
    fn shoelace(pts: &[Vec2]) -> f64 {
        let n = pts.len();
        (0..n).map(|j| pts[j].x * pts[(j + 1) % n].y - pts[(j + 1) % n].x * pts[j].y).sum::<f64>().abs() / 2.0
    }

    #[test]
    fn closed_control_spline_is_c2_across_its_closing_point() {
        use solidcore_record::Seg2;
        let cv = closed_control_points();
        let (sk, curve) = closed_control_curve(&cv, 3);
        let segs = sk.exact_segs(curve).expect("a closed control spline has exact Bézier pieces");
        assert_eq!(segs.len(), cv.len(), "one Bézier piece per control point");
        let cubic = |s: &Seg2| match *s {
            Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3],
            ref other => panic!("a cubic closed spline gives cubic pieces, not {other:?}"),
        };
        for (i, s) in segs.iter().enumerate() {
            let [_, a1, a2, a3] = cubic(s);
            let [b0, b1, b2, _] = cubic(&segs[(i + 1) % segs.len()]);
            assert!(a3.dist(b0) < 1e-9, "the pieces meet at joint {i}");
            assert!((a3 - a2 - (b1 - b0)).len() < 1e-9, "first derivatives differ at joint {i}");
            assert!((a3 - a2 * 2.0 + a1 - (b2 - b1 * 2.0 + b0)).len() < 1e-9, "second derivatives differ at joint {i}");
        }
    }

    #[test]
    fn closed_control_spline_traces_the_closed_cubic_b_spline_of_its_control_points() {
        use solidcore_record::Seg2;
        let cv = closed_control_points();
        let (sk, curve) = closed_control_curve(&cv, 3);
        // Piece k is the span the wrap starts at: piece 0 runs over control points 0..=3, which
        // the textbook segment 1 (points 0..=3 with wrapped indices i-1..=i+2) is.
        let segs = sk.exact_segs(curve).expect("exact pieces");
        for (k, s) in segs.iter().enumerate() {
            let Seg2::Cubic { p0, p1, p2, p3 } = *s else { panic!("a cubic closed spline gives cubic pieces") };
            for step in 0..=20 {
                let t = step as f64 / 20.0;
                let exact = p0 * (1.0 - t).powi(3) + p1 * (3.0 * t * (1.0 - t).powi(2)) + p2 * (3.0 * t * t * (1.0 - t)) + p3 * t.powi(3);
                assert!((exact - cyclic_cubic(&cv, (k + 1) % cv.len(), t)).len() < 1e-9, "piece {k} at t = {t}");
            }
        }
        let reference: Vec<Vec2> =
            (0..cv.len()).flat_map(|i| (0..200).map(move |k| (i, k))).map(|(i, k)| cyclic_cubic(&cv, i, k as f64 / 200.0)).collect();
        let area = shoelace(&reference);
        // A polyline whose chords lie within `tol` of the curve encloses the area to within
        // perimeter × tol (each chord's sliver is at most that), so that is the bound.
        let polyline = sk.polyline(curve);
        let perimeter: f64 = reference.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>() + reference[0].dist(reference[reference.len() - 1]);
        let tol = 1e-3;
        assert!((shoelace(&polyline) - area).abs() < perimeter * tol, "the polyline encloses {} mm², the B-spline {area} mm²", shoelace(&polyline));
        let mut solved = sk.clone();
        assert!(crate::solve(&mut solved).ok(), "a closed control spline solves without moving its points");
        assert_eq!(solved.points, sk.points);
        solved.validate_periodic_splines().unwrap();
    }

    #[test]
    fn closed_control_spline_needs_degree_plus_one_points() {
        let cv = closed_control_points();
        for n in 0..=3 {
            let mut sk = Sketch::new();
            let pts = cv[..n].iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
            let kind = crate::CurveKind::Spline { pts, control: true, degree: 3, periodic: true, nurbs: None, handles: Vec::new() };
            assert!(sk.add_curve(kind, None).is_err(), "{n} control points cannot close a cubic spline");
        }
        let (sk, _) = closed_control_curve(&cv[..4], 3);
        assert!(sk.validate_periodic_splines().is_ok());
        let mut quintic = Sketch::new();
        let pts = cv[..5].iter().map(|p| quintic.add_point(*p, None).unwrap()).collect();
        let kind = crate::CurveKind::Spline { pts, control: true, degree: 5, periodic: true, nurbs: None, handles: Vec::new() };
        assert!(quintic.add_curve(kind, None).is_err(), "a closed quintic needs 6 control points");
    }
}
