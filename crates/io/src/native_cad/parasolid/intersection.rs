//! Evaluate an intersection from its two source supports and ordered branch guide.
//! The guide selects the branch; it is never exported as the geometric boundary.
use parasolid_core::brep::*;
type R<T> = Result<T, String>;
type V = [f64; 3];
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: V, k: f64) -> V {
    a.map(|v| v * k)
}
fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn norm(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V) -> R<V> {
    let n = norm(a);
    if !n.is_finite() || n < 1e-20 { Err("degenerate intersection direction".into()) } else { Ok(mul(a, 1. / n)) }
}
fn projected_nurbs(m: &BrepModel, id: u32, n: &NurbsSurface, q: V) -> R<(f64, V)> {
    if n.control_vertices.len() > 4096 || n.u_control_vertex_count > 64 || n.v_control_vertex_count > 64 {
        return Err("intersection support exceeds projection budget".into());
    }
    let domain = super::pcurves::surface_domain(n)?;
    let lo = [domain[0][0], domain[1][0]];
    let hi = [domain[0][1], domain[1][1]];
    let span = [hi[0] - lo[0], hi[1] - lo[1]];
    if span.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("surface projection domain".into());
    }
    let sample = |uv: [f64; 2]| super::pcurves::surface(m, id, [uv[0], uv[1], 0.]);
    let frame = |uv: [f64; 2]| -> R<(V, V, V)> {
        let p = sample(uv)?;
        let h = [span[0] * 1e-5, span[1] * 1e-5];
        let u = mul(sub(sample([uv[0] + h[0], uv[1]])?, sample([uv[0] - h[0], uv[1]])?), 0.5 / h[0]);
        let v = mul(sub(sample([uv[0], uv[1] + h[1]])?, sample([uv[0], uv[1] - h[1]])?), 0.5 / h[1]);
        Ok((p, u, v))
    };
    let mut best = None;
    for i in 0..3 {
        for j in 0..3 {
            let mut uv = [lo[0] + span[0] * i as f64 / 2., lo[1] + span[1] * j as f64 / 2.];
            for _ in 0..24 {
                let (p, du, dv) = frame(uv)?;
                let r = sub(q, p);
                let d = norm(r);
                let normal = unit(cross(du, dv));
                if let Ok(normal) = normal
                    && best.as_ref().is_none_or(|(old, _, _): &(f64, V, V)| d < *old)
                {
                    best = Some((d, p, normal));
                }
                let a = dot(du, du);
                let b = dot(du, dv);
                let c = dot(dv, dv);
                let determinant = a * c - b * b;
                if determinant.abs() < a * c * 1e-14 || !determinant.is_finite() {
                    break;
                }
                let ru = dot(du, r);
                let rv = dot(dv, r);
                let step = [(c * ru - b * rv) / determinant, (a * rv - b * ru) / determinant];
                if !step.iter().all(|v| v.is_finite()) {
                    break;
                }
                let mut next = None;
                for scale in [1., 0.5, 0.25, 0.125] {
                    let candidate = [
                        (uv[0] + step[0] * scale).clamp(lo[0] - span[0] * 0.005, hi[0] + span[0] * 0.005),
                        (uv[1] + step[1] * scale).clamp(lo[1] - span[1] * 0.005, hi[1] + span[1] * 0.005),
                    ];
                    if norm(sub(sample(candidate)?, q)) < d {
                        next = Some(candidate);
                        break;
                    }
                }
                let Some(candidate) = next else { break };
                uv = candidate;
            }
        }
    }
    let (_, p, n) = best.ok_or("NURBS surface projection did not converge")?;
    Ok((dot(sub(q, p), n), n))
}
fn residual(m: &BrepModel, id: u32, q: V) -> R<(f64, V)> {
    let s = &m.surfaces.iter().find(|s| s.id == id).ok_or("intersection support")?.kind;
    let (f, g) = match *s {
        SurfaceKind::Plane { point, normal, .. } => (dot(sub(q, point.to_array()), normal.to_array()), normal.to_array()),
        SurfaceKind::Sphere { center, radius, .. } => {
            let d = sub(q, center.to_array());
            (dot(d, d) - radius * radius, mul(d, 2.))
        }
        SurfaceKind::Cylinder { point, axis, radius, .. } => {
            let d = sub(q, point.to_array());
            let r = sub(d, mul(axis.to_array(), dot(d, axis.to_array())));
            (dot(r, r) - radius * radius, mul(r, 2.))
        }
        SurfaceKind::Cone { point, axis, radius, sin_half_angle, cos_half_angle, .. } => {
            let d = sub(q, point.to_array());
            let h = dot(d, axis.to_array());
            let r = sub(d, mul(axis.to_array(), h));
            let t = sin_half_angle / cos_half_angle;
            let rr = radius + h * t;
            (dot(r, r) - rr * rr, sub(mul(r, 2.), mul(axis.to_array(), 2. * rr * t)))
        }
        SurfaceKind::Torus { center, axis, major_radius, minor_radius, .. } => {
            let d = sub(q, center.to_array());
            let h = dot(d, axis.to_array());
            let r = sub(d, mul(axis.to_array(), h));
            let len = norm(r);
            if len < 1e-20 {
                return Err("torus projection on its axis".into());
            }
            let x = len - major_radius;
            (x * x + h * h - minor_radius * minor_radius, add(mul(r, 2. * x / len), mul(axis.to_array(), 2. * h)))
        }
        SurfaceKind::Nurbs(ref n) => return projected_nurbs(m, id, n, q),
        _ => return Err("unsupported intersection support".into()),
    };
    let length = norm(g);
    if !length.is_finite() || length < 1e-20 {
        return Err("singular intersection support".into());
    }
    Ok((f / length, mul(g, 1. / length)))
}
pub(super) fn evaluate(m: &BrepModel, supports: [u32; 2], chart: &[Vector3], t: f64) -> R<V> {
    if chart.len() < 2 || chart.len() > 4096 || !t.is_finite() || t < 0. || t > (chart.len() - 1) as f64 {
        return Err("intersection guide parameter".into());
    }
    let i = (t.floor() as usize).min(chart.len() - 2);
    let a = chart.get(i).ok_or("guide point")?.to_array();
    let b = chart.get(i + 1).ok_or("guide point")?.to_array();
    let direction = sub(b, a);
    // Segment-normal guide planes otherwise jump at chart corners. Blend their
    // directions continuously so the same support branch has one shared endpoint.
    let closed = norm(sub(chart.first().ok_or("guide start")?.to_array(), chart.last().ok_or("guide end")?.to_array())) < 1e-10;
    let tangent = |j: usize| -> R<V> {
        let (before, after) = if closed && (j == 0 || j + 1 == chart.len()) {
            (chart.get(chart.len() - 2), chart.get(1))
        } else {
            (chart.get(j.saturating_sub(1)), chart.get((j + 1).min(chart.len() - 1)))
        };
        unit(sub(after.ok_or("guide tangent")?.to_array(), before.ok_or("guide tangent")?.to_array()))
    };
    let t0 = tangent(i)?;
    let t1 = tangent(i + 1)?;
    let s = t - i as f64;
    let axis = unit(add(mul(t0, 1. - s), mul(t1, s)))?;
    let leader = add(a, mul(direction, t - i as f64));
    let mut q = leader;
    for _ in 0..32 {
        let (a, ga) = residual(m, supports[0], q)?;
        let (b, gb) = residual(m, supports[1], q)?;
        let c = dot(sub(q, leader), axis);
        let det = dot(ga, cross(gb, axis));
        if !det.is_finite() || det.abs() < 1e-12 {
            return Err("intersection branch is singular".into());
        }
        let step = mul(add(add(mul(cross(gb, axis), -a), mul(cross(axis, ga), -b)), mul(cross(ga, gb), -c)), 1. / det);
        if !step.iter().all(|v| v.is_finite()) || norm(sub(q, leader)) > norm(direction) * 2. {
            return Err("intersection projection left its source branch".into());
        }
        if norm(step) < 1e-11 && a.abs().max(b.abs()).max(c.abs()) < 1e-12 {
            return Ok(add(q, step));
        }
        q = add(q, step);
    }
    Err("intersection projection did not converge".into())
}
