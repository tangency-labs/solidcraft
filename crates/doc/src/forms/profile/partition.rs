//! Optional CAD cells over the same filled surface; no second surface fit.
use super::*;
use solidcore_record::Vec2;
use solidcraft_geom::{
    bspline::{HPoint, NurbsSurface},
    tess::EvalSurface,
};

pub(super) struct Partition {
    pub body: kernel::Body,
    pub roots: Vec<usize>,
}
/// The displayed skin and its editable guides share the native edge nodes.
/// Legacy single-face sources keep their authored CAD topology: cells used here
/// only constrain their display tessellation. Unsupported trims retain the
/// existing bounded-chart display; this does not repartition their bounding box.
pub(super) fn display_mesh(
    patch: &kernel::Body,
    roots: Option<&[usize]>,
    profile: &Profile,
    cage: &Cage,
    placement: &Placement,
    tolerance: f64,
    mesh: solidcraft_geom::Mesh,
) -> Result<(solidcraft_geom::Mesh, Option<Vec<Vec<Vec3>>>)> {
    let (mut mesh, roots) = if let Some(roots) = roots {
        (mesh, roots.to_vec())
    } else {
        if patch.face_count() != 1 {
            return Ok((mesh, None));
        }
        let mut display = profile.clone();
        display.partition = true;
        let Ok(Some(cells)) = build(patch, &display, cage, placement, tolerance) else {
            return Ok((mesh, None));
        };
        (cells.body.mesh(&kernel::MeshTolerance::chord(tolerance.min(0.05)))?, cells.roots)
    };
    let edges = cage.edges().map_err(|e| invalid(e.to_string()))?;
    if mesh.edges.len() != edges.len() || mesh.edge_faces.len() != mesh.edges.len() {
        return Err(invalid("Surface display cells lost their logical guide edges"));
    }
    let native_owners = mesh
        .edge_faces
        .iter()
        .map(|faces| {
            faces
                .iter()
                .map(|&face| roots.get(face as usize).copied().ok_or_else(|| invalid("Surface display edge lost its cell owner")))
                .collect::<Result<BTreeSet<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    // The constructor projects the authored guides onto this common support.
    // Certify those fitted endpoints, rather than using the user's fit tolerance
    // as an identity radius (it may exceed a small cell's entire width).
    let support = kernel::face_geometry(patch, 0)?;
    let domain = solidcraft_geom::tess::uv::domain(&support.surface);
    let endpoints = cage
        .points
        .iter()
        .map(|&point| {
            let uv = support.parameters(placement.point(point), None, tolerance)?;
            let (point, du, dv) = support.surface.derivs(uv[0], uv[1]).ok_or_else(|| invalid("Surface display lost its fitted guide endpoint"))?;
            let parameter_scale = [0, 1].map(|d| domain.range[d].map_or(uv[d].abs(), |(a, b)| a.abs().max(b.abs()).max(b - a)).max(uv[d].abs()));
            let roundoff = 1024.0 * f64::EPSILON * (point.len() + du.len() * parameter_scale[0] + dv.len() * parameter_scale[1]);
            Ok((point, roundoff))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut guides = Vec::with_capacity(edges.len());
    let mut smooth = Vec::with_capacity(edges.len());
    let mut sharp = Vec::with_capacity(edges.len());
    let mut used = BTreeSet::new();
    for &[a, b] in &edges {
        let owners: BTreeSet<_> = cage
            .faces
            .iter()
            .enumerate()
            .filter(|(_, face)| face.iter().zip(face.iter().cycle().skip(1)).take(face.len()).any(|(&i, &j)| [i.min(j), i.max(j)] == [a, b]))
            .map(|(id, _)| id)
            .collect();
        let &(start, start_roundoff) = endpoints.get(a).ok_or_else(|| invalid("Surface display lost a guide endpoint"))?;
        let &(end, end_roundoff) = endpoints.get(b).ok_or_else(|| invalid("Surface display lost a guide endpoint"))?;
        let roundoff = start_roundoff.max(end_roundoff);
        let candidates: Vec<_> = mesh
            .edges
            .iter()
            .enumerate()
            .filter(|(id, _)| native_owners.get(*id).is_some_and(|native| *native == owners))
            .filter_map(|(id, curve)| {
                let (first, last) = (*curve.first()?, *curve.last()?);
                let forward = first.dist(start) <= roundoff && last.dist(end) <= roundoff;
                let reverse = first.dist(end) <= roundoff && last.dist(start) <= roundoff;
                (curve.len() >= 2 && forward != reverse).then_some((id, reverse))
            })
            .collect();
        let [(id, reverse)] = candidates.as_slice() else {
            return Err(invalid("Surface display guide does not have one exact cell-edge identity"));
        };
        if !used.insert(*id) {
            return Err(invalid("Surface display reused one native edge for distinct guides"));
        }
        let mut curve = mesh.edges.get(*id).ok_or_else(|| invalid("Surface display lost its native guide edge"))?.clone();
        if *reverse {
            curve.reverse();
        }
        guides.push(curve);
        smooth.push(mesh.smooth_edges.get(*id).copied().unwrap_or(false));
        sharp.push(mesh.sharp_edges.get(*id).copied().unwrap_or(false));
    }
    mesh.smooth_edges = smooth;
    mesh.sharp_edges = sharp;
    Ok((mesh, Some(guides)))
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
    // Inverse projection can put an exactly isoparametric guide a few ulps off
    // a stored knot. Keeping that roundoff creates artificial oblique trims
    // whose knot crossings are only 1e-15 apart; a native curve cannot retain
    // those spans. Restore only knot coordinates within floating-point roundoff,
    // independently of the modelling tolerance or the guide grid resolution.
    for point in &mut uv {
        for (d, coordinate) in point.iter_mut().enumerate() {
            let scale = span[d][0].abs().max(span[d][1].abs()).max(span[d][1] - span[d][0]);
            if let Some(knot) = surface.knots()[d].iter().min_by(|a, b| (*coordinate - **a).abs().total_cmp(&(*coordinate - **b).abs()))
                && (*coordinate - *knot).abs() <= 64.0 * f64::EPSILON * scale
            {
                *coordinate = *knot;
            }
        }
    }
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
        // Preserve one face and the original nonplanar chart for each CAD cell.
        // An exactly planar cell may use a regular support without fitting.
        sheets.push(kernel::nurbs_region_sheet_with_planar_support(&surface, &[wire])?);
        roots.push(id);
    }
    let body = kernel::stitch(&sheets.iter().collect::<Vec<_>>(), tolerance)?;
    if !body.is_surface() || body.shell_count() != 1 || body.face_count() != cage.faces.len() {
        return Ok(None);
    }
    let roots = super::super::stitched_roots(&body, &sheets, &roots)?;
    Ok(Some(Partition { body, roots }))
}
