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
                if *control || *degree != 3 || work > 1_000_000 || pts.len() > MAX_SPLINE_POINTS {
                    return Err(bad());
                }
                let points = pts.iter().map(|i| self.point(*i).ok_or_else(bad)).collect::<Result<Vec<_>>>()?;
                validate_periodic_points(&points)?;
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
}
