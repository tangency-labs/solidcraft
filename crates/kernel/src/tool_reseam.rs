//! Retry a failed cut with different boundaries between a round tool's faces (#2387).
//! Every curved support must be a revolution about the same axis and every planar support
//! perpendicular to it. Turning this closed solid about that axis changes only its seams.

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, InnerSpace, Transform};

use super::{BoolOp, boolean_via, boolean_whole, nudged, transform};
use crate::body::{Body, from_p3};
use crate::boolean_diagnostics::{BooleanFallback, BooleanStrategy, capture_boolean_diagnostics, merge, record};
use crate::{guard, record};

thread_local! {
    static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Exact revolution axes, rather than a fit to sampled normals: splines and affine
/// distortions of round tools must never be mistaken for rotational symmetry.
fn axis(body: &Body) -> Option<(Vec3, Vec3)> {
    if body.is_mesh() || body.shell_count() != 1 || body.face_count() > 64 {
        return None;
    }
    guard("round tool axis", || {
        let mut axes = Vec::new();
        let mut normals = Vec::new();
        for face in body.solid.face_iter() {
            match face.surface() {
                mt::Surface::Plane(p) => {
                    let n = p.normal();
                    let Some(n) = Vec3::new(n.x, n.y, n.z).normalized() else { return Ok(None) };
                    normals.push(n);
                }
                mt::Surface::RevolutedCurve(s) => {
                    let m = s.transform();
                    let (x, y, z) = (m.x.truncate(), m.y.truncate(), m.z.truncate());
                    // Only rigid transforms: no elliptic scaling of the revolution.
                    if [x, y, z].iter().any(|v| (v.magnitude2() - 1.0).abs() > 1e-9)
                        || [x.dot(y), x.dot(z), y.dot(z)].iter().any(|v| v.abs() > 1e-9)
                        || m.x.w != 0.0
                        || m.y.w != 0.0
                        || m.z.w != 0.0
                        || m.w.w != 1.0
                    {
                        return Ok(None);
                    }
                    let o = from_p3(m.transform_point(s.entity().origin()));
                    let a = m.transform_vector(s.entity().axis());
                    let Some(a) = Vec3::new(a.x, a.y, a.z).normalized() else { return Ok(None) };
                    axes.push((o, a));
                }
                _ => return Ok(None),
            }
        }
        let Some(&(o, a)) = axes.first() else { return Ok(None) };
        let tol = (body.size() * 1e-9).max(1e-10);
        let same = axes.iter().all(|(p, d)| a.cross(*d).len() < 1e-9 && (*p - o).cross(a).len() < tol);
        // Surface orientation can reverse the same axis. Choose a stable direction for retries.
        let first = [a.x, a.y, a.z].into_iter().find(|v| v.abs() > 1e-9).unwrap_or(1.0);
        let a = if first < 0.0 { -a } else { a };
        Ok((same && normals.iter().all(|n| n.cross(a).len() < 1e-9)).then_some((o, a)))
    })
    .ok()
    .flatten()
}

pub(super) fn cut(target: &Body, tool: &Body) -> Option<Body> {
    if ACTIVE.with(|a| a.get()) {
        return None;
    }
    let (origin, axis) = axis(tool)?;
    ACTIVE.with(|a| a.set(true));
    let result = guard("cut with reseamed tool", || {
        let counts = record::counts(&[target, tool]);
        // Bounded exact retries. Keep the original error if neither can be verified.
        for angle in [0.3, -0.3, 0.7, -0.7] {
            super::check_attempt_budget()?;
            let turned = transform(tool, Vec3::ZERO, origin, axis, angle)?;
            let (result, diagnostics) = capture_boolean_diagnostics(|| boolean_via(target, &turned, BoolOp::Cut, &boolean_whole));
            if let Ok(Some(body)) = result
                && crate::boolean_guard::check_retry(target, tool, BoolOp::Cut, Some(&body)).is_ok()
            {
                let body = nudged(body, &turned, &counts);
                merge(diagnostics);
                let mut event = BooleanFallback::new(BooleanStrategy::ToolReseam);
                event.rotation_rad = angle;
                record(event);
                return Ok(Some(body));
            }
        }
        Ok(None)
    })
    .ok()
    .flatten();
    ACTIVE.with(|a| a.set(false));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform_matrix;
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};

    fn drill(angle: f64) -> Body {
        let profile =
            Region2 { outer: Loop2::polygon(&[Vec2::new(0., 1.), Vec2::new(2.5, 1.), Vec2::new(2.5, -10.), Vec2::new(0., -11.5)]), holes: vec![] };
        crate::revolve(&Plane::XZ, &[profile], Vec2::ZERO, Vec2::Y, angle).unwrap().pop().unwrap()
    }

    #[test]
    fn only_complete_coaxial_revolutions_can_be_reseamed() {
        let tool = drill(std::f64::consts::TAU);
        assert!(axis(&tool).is_some());
        let placed = transform(&tool, Vec3::new(11., -7., 5.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.8).unwrap();
        let (o, a) = axis(&placed).unwrap();
        let turned = transform(&placed, Vec3::ZERO, o, a, 0.3).unwrap();
        assert!((crate::measure(&placed).unwrap().volume - crate::measure(&turned).unwrap().volume).abs() < 1e-6);
        // A partial turn's radial caps would move; an affine scale can make the bore elliptic.
        assert!(axis(&drill(std::f64::consts::PI)).is_none());
        let scaled = transform_matrix(&tool, [[2., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]).unwrap();
        assert!(axis(&scaled).is_none());
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        assert!(axis(&block).is_none());
    }
}
