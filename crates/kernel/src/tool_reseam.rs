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

/// The centre of the sphere a closed body is made of, every face a patch of that one sphere (the
/// sphere primitive's six patches, a sphere read from STEP).
fn sphere_centre(body: &Body) -> Option<Vec3> {
    if body.is_mesh() || body.shell_count() != 1 || body.face_count() > 64 {
        return None;
    }
    let tol = (body.size() * 1e-9).max(1e-10);
    let mut sphere: Option<(Vec3, f64)> = None;
    for i in 0..body.face_count() {
        let crate::FaceSurface::Sphere { center, radius } = crate::face_surface(body, i)? else { return None };
        match sphere {
            None => sphere = Some((center, radius)),
            Some((c, r)) if (center - c).len() < tol && (radius - r).abs() < tol => {}
            Some(_) => return None,
        }
    }
    sphere.map(|(c, _)| c)
}

/// Retry a failed boolean with a sphere operand turned about its centre (#3633): the same solid,
/// with the boundaries between its patches elsewhere. A body whose faces and edges lie along
/// them (a pyramid with its apex at the centre and its edges through the patches' corners) meets
/// the turned sphere across its faces instead.
pub(super) fn sphere(a: &Body, b: &Body, op: BoolOp) -> Option<Body> {
    if ACTIVE.with(|x| x.get()) {
        return None;
    }
    let (k, centre) = match (sphere_centre(a), sphere_centre(b)) {
        (Some(c), _) => (0, c),
        (None, Some(c)) => (1, c),
        (None, None) => return None,
    };
    ACTIVE.with(|x| x.set(true));
    let result = guard("boolean with a turned sphere", || {
        let counts = record::counts(&[a, b]);
        // A skew axis: a turn about one of the frame's axes keeps patch boundaries in the
        // planes a box or a pyramid on that frame has its faces in.
        let axis = Vec3::new(1.0, 2.0, 3.0).normalized().unwrap_or(Vec3::Z);
        for angle in [0.3, -0.3, 0.7, -0.7] {
            let turned = transform(if k == 0 { a } else { b }, Vec3::ZERO, centre, axis, angle)?;
            let (x, y) = if k == 0 { (&turned, b) } else { (a, &turned) };
            let (result, diagnostics) = capture_boolean_diagnostics(|| boolean_via(x, y, op, &boolean_whole));
            if let Ok(Some(body)) = result
                && crate::boolean_guard::check_retry(a, b, op, Some(&body)).is_ok()
            {
                let history = crate::boolean_record::through(&body, k, turned.history());
                let body = body.with_checked_history(history, &counts);
                merge(diagnostics);
                let mut event = BooleanFallback::new(BooleanStrategy::SphereReseam);
                event.rotation_rad = angle;
                record(event);
                return Ok(Some(body));
            }
        }
        Ok(None)
    })
    .ok()
    .flatten();
    ACTIVE.with(|x| x.set(false));
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

    /// A pyramid with its apex at the sphere's centre, its four edges through the corners of the
    /// sphere's -X patch and its sides in the planes of that patch's edges (#3633). The two meet
    /// in exactly a sixth of the sphere.
    #[test]
    fn a_pyramid_along_a_spheres_patches_meets_it_in_a_sixth() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let side = |y: f64, z: f64| crate::HalfSpace { n: Vec3::new(s, y * s, z * s), d: 0.0 };
        let pyramid =
            crate::convex_polyhedron(&[crate::HalfSpace { n: -Vec3::X, d: 10.0 }, side(1.0, 0.0), side(-1.0, 0.0), side(0.0, 1.0), side(0.0, -1.0)])
                .unwrap();
        let sphere = crate::sphere(Vec3::ZERO, 10.0).unwrap();
        assert!(sphere_centre(&sphere).is_some_and(|c| c.len() < 1e-12));
        assert!(sphere_centre(&pyramid).is_none());
        let whole = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        for (a, b) in [(&sphere, &pyramid), (&pyramid, &sphere)] {
            let common = crate::boolean(a, b, BoolOp::Intersect).unwrap().expect("a sixth of the sphere");
            let v = crate::measure(&common).unwrap().volume;
            assert!((v - whole / 6.0).abs() < whole * 1e-3, "{v}");
            assert!(common.validity().is_empty(), "{:?}", common.validity());
        }
        let rest = crate::boolean(&sphere, &pyramid, BoolOp::Cut).unwrap().expect("five sixths of the sphere");
        let v = crate::measure(&rest).unwrap().volume;
        assert!((v - whole * 5.0 / 6.0).abs() < whole * 1e-3, "{v}");
    }
}
