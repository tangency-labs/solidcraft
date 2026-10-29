//! Optional CAD cells over the same filled surface; no second surface fit.
use super::*;
use solidcraft_geom::{
    Vec2,
    bspline::{HPoint, NurbsSurface},
    tess::EvalSurface,
};

pub(super) struct Partition {
    pub body: kernel::Body,
    pub roots: Vec<usize>,
}
pub(super) fn build(patch: &kernel::Body, profile: &Profile, cage: &Cage, placement: &Placement, tolerance: f64) -> Result<Option<Partition>> {
    if !profile.partition || cage.faces.len() < 2 {
        return Ok(None);
    }
    let face = kernel::face_geometry(patch, 0)?;
    if face.loops.len() != 1 {
        return Ok(None);
    }
    let EvalSurface::Nurbs { degree, knots, ctrl, .. } = &face.surface else {
        return Ok(None);
    };
    let surface =
        NurbsSurface::from_homogeneous(*degree, knots.clone(), ctrl.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect())
            .map_err(|e| invalid(e.to_string()))?;
    let span = kernel::face_parameter_span(patch, 0)?;
    let epsilon = span.map(|r| ((r[1] - r[0]) * 1e-6).max(1e-9));
    // Only a rectangular outer trim can be covered by the complete regular grid.
    // Oblique edges INSIDE it are exact parameter-line restrictions.
    for edge in face.loops.iter().flatten() {
        let mut near = None;
        for i in 0..=16 {
            let Some((point, _)) = edge.at(i as f64 / 16.0) else {
                return Ok(None);
            };
            let uv = face.parameters(point, near, tolerance)?;
            near = Some(uv);
            if !(0..2).any(|d| (uv[d] - span[d][0]).abs().min((uv[d] - span[d][1]).abs()) <= epsilon[d]) {
                return Ok(None);
            }
        }
    }
    let mut uv = cage.points.iter().map(|&p| face.parameters(placement.point(p), None, tolerance)).collect::<kernel::Result<Vec<_>>>()?;
    let [u, v] = profile.segments;
    let corners = [0, u, v * (u + 1), v * (u + 1) + u];
    for &i in &corners {
        let Some(point) = uv.get_mut(i) else {
            return Ok(None);
        };
        for d in 0..2 {
            let closest = if (point[d] - span[d][0]).abs() < (point[d] - span[d][1]).abs() { span[d][0] } else { span[d][1] };
            if (point[d] - closest).abs() > epsilon[d] {
                return Ok(None);
            }
            point[d] = closest;
        }
    }
    let (a, b, c) = (uv[0], uv[u], uv[v * (u + 1)]);
    let du = [b[0] - a[0], b[1] - a[1]];
    let dv = [c[0] - a[0], c[1] - a[1]];
    if !((du[0].abs() <= epsilon[0] && dv[1].abs() <= epsilon[1]) || (du[1].abs() <= epsilon[1] && dv[0].abs() <= epsilon[0])) {
        return Ok(None);
    }
    // Snap the fixed perimeter to its fitted parameter sides. This shares exactly
    // the same values at corners and does not move an interior guide's parameters.
    for j in 0..=v {
        for i in 0..=u {
            let p = &mut uv[j * (u + 1) + i];
            if i == 0 || i == u {
                for d in 0..2 {
                    if dv[d].abs() <= epsilon[d] {
                        p[d] = a[d] + du[d] * i as f64 / u as f64;
                    }
                }
            }
            if j == 0 || j == v {
                for d in 0..2 {
                    if du[d].abs() <= epsilon[d] {
                        p[d] = a[d] + dv[d] * j as f64 / v as f64;
                    }
                }
            }
        }
    }
    let sign = (du[0] * dv[1] - du[1] * dv[0]).signum();
    if sign == 0.0 {
        return Ok(None);
    }
    let mut sheets = Vec::with_capacity(cage.faces.len());
    let mut roots = Vec::with_capacity(cage.faces.len());
    let mut orientation = None;
    for (id, face) in cage.faces.iter().enumerate() {
        let mut wire = face.iter().map(|&i| uv.get(i).copied().ok_or_else(|| invalid("a CAD cell lost a guide"))).collect::<Result<Vec<_>>>()?;
        let area = wire.iter().zip(wire.iter().cycle().skip(1)).take(wire.len()).map(|(a, b)| a[0] * b[1] - a[1] * b[0]).sum::<f64>();
        let winding = area.signum();
        if area.abs() <= epsilon[0] * epsilon[1] {
            return Ok(None);
        }
        if orientation.is_some_and(|sign| sign != winding) {
            return Ok(None);
        }
        orientation = Some(winding);
        for i in 0..4 {
            let a = Vec2::new(wire[i][0], wire[i][1]);
            let b = Vec2::new(wire[(i + 1) % 4][0], wire[(i + 1) % 4][1]);
            let c = Vec2::new(wire[(i + 2) % 4][0], wire[(i + 2) % 4][1]);
            let (ab, bc) = (b - a, c - b);
            if (ab.x * bc.y - ab.y * bc.x) * winding <= epsilon[0] * epsilon[1] {
                return Ok(None);
            }
        }
        // Source faces can be reversed; the kernel constructor wants absolute UV wires.
        if winding < 0.0 {
            wire.reverse();
        }
        sheets.push(kernel::nurbs_region_sheet(&surface, &[wire])?);
        roots.push(id);
    }
    let body = kernel::stitch(&sheets.iter().collect::<Vec<_>>(), tolerance)?;
    if !body.is_surface() || body.shell_count() != 1 || body.face_count() != cage.faces.len() {
        return Ok(None);
    }
    Ok(Some(Partition { body, roots }))
}
