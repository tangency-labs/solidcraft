//! Display paths embedded in certified rectangular native surface charts.
use super::*;
use solidcraft_geom::{Mesh, brep::pcurve, tess::uv as eval_uv};

/// A display station with either intrinsic surface coordinates or an exact
/// existing CAD edge identity and original parameter. No authored topology.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DisplayGuidePoint {
    Surface([f64; 2]),
    Edge { edge: u32, parameter: f64 },
}
/// One continuous display path on one native face. Adjacent pieces use the same
/// edge identity/parameter at their shared end.
#[derive(Clone, Debug)]
pub struct DisplayGuide {
    pub face: u32,
    pub points: Vec<DisplayGuidePoint>,
}
pub struct GuidedMesh {
    pub mesh: Mesh,
    pub guides: Vec<Vec<Vec3>>,
}

const MAX_FACES: usize = 64;
const MAX_STATIONS: usize = 20_000;
const MAX_PLAN_NODES: usize = 200_000;

fn fail(why: &str) -> KernelError {
    unsupported(format!("guided rectangular charts: {why}"))
}

/// A full nonperiodic spline rectangle, certified from the actual oriented
/// linear pcurves, not from a trim's bounding box or sampled proximity.
fn rectangle(face: &brep::Face, surface: &EvalSurface) -> Result<[[f64; 2]; 2]> {
    let EvalSurface::Nurbs { .. } = surface else { return Err(fail("requires spline charts")) };
    let domain = eval_uv::domain(surface);
    let ([Some((u0, u1)), Some((v0, v1))], [None, None]) = (domain.range, domain.period) else {
        return Err(fail("requires bounded nonperiodic charts"));
    };
    if ![u0, u1, v0, v1].iter().all(|x| x.is_finite()) || u1 <= u0 || v1 <= v0 {
        return Err(fail("invalid chart interval"));
    }
    let [Loop::Edges { edges }] = face.loops.as_slice() else { return Err(fail("requires one rectangular loop")) };
    if edges.len() != 4 {
        return Err(fail("requires four exact parameter sides"));
    }
    let corners = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let mut used = std::collections::BTreeSet::new();
    let mut previous = None;
    let mut first = None;
    for edge in edges {
        let brep::Curve2::Spline { knots, points } = &edge.pcurve else { return Err(fail("requires linear pcurves")) };
        if points.len() != 2 || knots.len() != 4 || knots.first() != knots.get(1) || knots.get(2) != knots.get(3) {
            return Err(fail("requires linear bounded pcurves"));
        }
        let [low, high] = edge.pcurve.interval().ok_or_else(|| fail("invalid pcurve interval"))?;
        let (a, b) = if edge.forward { (low, high) } else { (high, low) };
        let a = pcurve::eval(&edge.pcurve, a).ok_or_else(|| fail("invalid pcurve start"))?;
        let b = pcurve::eval(&edge.pcurve, b).ok_or_else(|| fail("invalid pcurve end"))?;
        let ia = corners.iter().position(|point| *point == a).ok_or_else(|| fail("pcurve start is not a chart corner"))?;
        let ib = corners.iter().position(|point| *point == b).ok_or_else(|| fail("pcurve end is not a chart corner"))?;
        if (ia + 1) % 4 != ib && (ib + 1) % 4 != ia || previous.is_some_and(|point| point != a) || !used.insert([ia.min(ib), ia.max(ib)]) {
            return Err(fail("pcurve rectangle does not close uniquely"));
        }
        first.get_or_insert(a);
        previous = Some(b);
    }
    if first != previous {
        return Err(fail("open pcurve rectangle"));
    }
    Ok([[u0, u1], [v0, v1]])
}

pub(crate) fn mesh(b: &brep::Brep, tolerance: f64, turn: Option<size::Turn>, guides: &[DisplayGuide]) -> Result<GuidedMesh> {
    if b.faces.is_empty() || b.faces.len() > MAX_FACES || guides.len() > MAX_STATIONS {
        return Err(fail("face/path budget exhausted"));
    }
    let count = guides.iter().try_fold(0usize, |sum, guide| sum.checked_add(guide.points.len())).ok_or_else(|| fail("station count overflow"))?;
    if count > MAX_STATIONS || guides.iter().any(|guide| guide.points.len() < 2) {
        return Err(fail("station budget exhausted or empty path"));
    }
    let mut stations = vec![Vec::new(); b.edges.len()];
    for guide in guides {
        let face = b.faces.get(guide.face as usize).ok_or_else(|| fail("missing guide face"))?;
        for point in &guide.points {
            if let DisplayGuidePoint::Edge { edge, parameter } = point {
                if !parameter.is_finite() || !face_edges(face).any(|use_| use_.edge == *edge) {
                    return Err(fail("edge station does not belong to its face"));
                }
                stations.get_mut(*edge as usize).ok_or_else(|| fail("missing guide edge"))?.push(*parameter);
            }
        }
    }
    let (mut plan, stats) = plan_with_stations(b, tolerance, turn, &stations)?;
    if !stats.lenient.is_empty() || !stats.unmeshed.is_empty() || plan.vertex_count() > MAX_PLAN_NODES || plan.faces.len() != b.faces.len() {
        return Err(fail("native plan is incomplete or exceeds the node budget"));
    }
    let mut output = vec![Vec::new(); guides.len()];
    let mut total = 0usize;
    for fp in &mut plan.faces {
        let fi = fp.face as usize;
        let face = b.faces.get(fi).ok_or_else(|| fail("missing planned face"))?;
        let surface = fp.surface.and_then(|id| plan.surfaces.get(id as usize)).ok_or_else(|| fail("missing planned surface"))?;
        let spans = rectangle(face, surface)?;
        let norm = |point: [f64; 2]| [(point[0] - spans[0][0]) / (spans[0][1] - spans[0][0]), (point[1] - spans[1][0]) / (spans[1][1] - spans[1][0])];
        let denorm = |point: [f64; 2]| [spans[0][0] + point[0] * (spans[0][1] - spans[0][0]), spans[1][0] + point[1] * (spans[1][1] - spans[1][0])];
        let indices: Vec<_> = guides.iter().enumerate().filter(|(_, guide)| guide.face == fp.face).map(|(id, _)| id).collect();
        let mut paths = Vec::with_capacity(indices.len());
        for &id in &indices {
            let guide = guides.get(id).ok_or_else(|| fail("missing guide"))?;
            let mut path = Vec::with_capacity(guide.points.len());
            for point in &guide.points {
                let uv = match *point {
                    DisplayGuidePoint::Surface(uv) => {
                        let normal = norm(uv);
                        if normal.iter().any(|value| !value.is_finite() || *value <= 0.0 || *value >= 1.0) {
                            return Err(fail("chart-boundary station needs an exact edge identity"));
                        }
                        uv
                    }
                    DisplayGuidePoint::Edge { edge, parameter } => {
                        let mut uses = face_edges(face).filter(|use_| use_.edge == edge);
                        let use_ = uses.next().ok_or_else(|| fail("missing pcurve"))?;
                        if uses.next().is_some() {
                            return Err(fail("ambiguous edge use"));
                        }
                        let uv = pcurve::eval(&use_.pcurve, parameter).ok_or_else(|| fail("pcurve station does not evaluate"))?;
                        // This exact parameter was merged before native planning,
                        // so it must already be a canonical fixed prefix node.
                        let node = fp
                            .uv
                            .iter()
                            .take(fp.fixed.len())
                            .position(|point| *point == uv)
                            .ok_or_else(|| fail("guided station lost its shared fixed node"))?;
                        let native = b.edges.get(edge as usize).ok_or_else(|| fail("missing canonical edge"))?;
                        let vertex = if parameter == native.interval[0] {
                            Some(native.vertices[0])
                        } else if parameter == native.interval[1] {
                            Some(native.vertices[1])
                        } else {
                            None
                        };
                        let expected = match vertex {
                            Some(vertex) => {
                                let vertex = b.vertices.get(vertex as usize).ok_or_else(|| fail("missing canonical vertex"))?;
                                Vec3::new(vertex.point[0], vertex.point[1], vertex.point[2])
                            }
                            None => solidcraft_geom::brep::eval::CurveEval::new(native.parameters())
                                .map_err(|_| fail("invalid canonical curve"))?
                                .point(parameter)
                                .ok_or_else(|| fail("canonical curve does not evaluate"))?,
                        };
                        if fp.fixed.get(node).copied() != Some(expected) {
                            return Err(fail("guided station does not retain its canonical native edge or vertex position"));
                        }
                        uv
                    }
                };
                path.push(norm(uv));
            }
            paths.push(path);
        }
        let normalized: Vec<_> = fp.uv.iter().copied().map(norm).collect();
        let coverage = fp
            .triangles
            .iter()
            .try_fold(0.0, |sum, triangle| {
                let (a, b, c) =
                    (*normalized.get(triangle[0] as usize)?, *normalized.get(triangle[1] as usize)?, *normalized.get(triangle[2] as usize)?);
                Some(sum + ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs())
            })
            .ok_or_else(|| fail("invalid native triangle index"))?;
        if (coverage - 2.0).abs() > 2048.0 * f64::EPSILON * fp.triangles.len() as f64 {
            return Err(fail("native plan does not cover its full certified rectangle"));
        }
        // Exact duplicate UVs may share a CDT handle only if their canonical
        // fixed world values agree. A fixed/interior disagreement also refuses.
        let mut same = std::collections::BTreeMap::new();
        for (id, uv) in normalized.iter().enumerate() {
            let key = uv.map(|value| if value == 0.0 { 0 } else { value.to_bits() });
            let world = fp.fixed.get(id).copied().unwrap_or_else(|| surface.eval(fp.uv[id][0], fp.uv[id][1]).0);
            if let Some((previous_uv, previous_world)) = same.insert(key, (fp.uv[id], world))
                && (previous_uv != fp.uv[id] || previous_world != world)
            {
                return Err(fail("duplicate parameters disagree on their canonical world point"));
            }
        }
        let constrained =
            cdt::constrain_existing(&normalized, &fp.triangles, &paths).ok_or_else(|| fail("bounded constraint arrangement was refused"))?;
        total = total.checked_add(constrained.points.len()).ok_or_else(|| fail("display node count overflow"))?;
        if total > MAX_PLAN_NODES {
            return Err(fail("display node budget exhausted"));
        }
        // Retain original UV bits; only new interior nodes are denormalized.
        let original = fp.uv.len();
        if constrained.points.iter().skip(original).any(|point| point.iter().any(|value| *value == 0.0 || *value == 1.0)) {
            return Err(fail("local constraints inserted an unshared chart-boundary node"));
        }
        fp.uv.extend(constrained.points.iter().skip(original).copied().map(denorm));
        fp.triangles = constrained.triangles;
        qualify(surface, fp, tolerance, turn)?;
        for (&id, path) in indices.iter().zip(constrained.paths) {
            let curve = path
                .into_iter()
                .map(|node| {
                    let node = node as usize;
                    let uv = fp.uv.get(node).ok_or_else(|| fail("guide node outside its face"))?;
                    let world = fp.fixed.get(node).copied().unwrap_or_else(|| surface.eval(uv[0], uv[1]).0);
                    if !world.is_finite() {
                        return Err(fail("nonfinite guide node"));
                    }
                    Ok(world)
                })
                .collect::<Result<Vec<_>>>()?;
            *output.get_mut(id).ok_or_else(|| fail("missing output guide"))? = curve;
        }
    }
    Ok(GuidedMesh { mesh: plan.evaluate(), guides: output })
}

/// Apply the ordinary mesher's final centroid/side normal-departure contract to
/// the new cells, plus the caller's facet-angle rule. Refuse this bounded route
/// if more refinement would be needed; never report an unchecked coarse result.
fn qualify(surface: &EvalSurface, face: &FacePlan, tolerance: f64, turn: Option<size::Turn>) -> Result<()> {
    for triangle in &face.triangles {
        let mut points = [Vec3::ZERO; 3];
        let mut parameters = [[0.0; 2]; 3];
        let mut normals = [Vec3::ZERO; 3];
        for ((&id, point), (uv, normal)) in triangle.iter().zip(&mut points).zip(parameters.iter_mut().zip(&mut normals)) {
            let id = id as usize;
            *uv = *face.uv.get(id).ok_or_else(|| fail("invalid constrained triangle index"))?;
            let (evaluated, n) = surface.eval(uv[0], uv[1]);
            *point = face.fixed.get(id).copied().unwrap_or(evaluated);
            *normal = n;
            if !point.is_finite() || !n.is_finite() {
                return Err(fail("invalid constrained cell evaluation"));
            }
        }
        let mut worst = 0.0_f64;
        for weights in [[1.0 / 3.0; 3], [0.5, 0.5, 0.0], [0.0, 0.5, 0.5], [0.5, 0.0, 0.5]] {
            let uv = [0, 1].map(|d| parameters.iter().zip(weights).map(|(uv, weight)| uv[d] * weight).sum::<f64>());
            let chord = points.iter().zip(weights).fold(Vec3::ZERO, |sum, (&point, weight)| sum + point * weight);
            let (point, du, dv) = surface.derivs(uv[0], uv[1]).ok_or_else(|| fail("constrained cell derivatives failed"))?;
            let departure = du.cross(dv).normalized().map_or_else(|| point.dist(chord), |normal| (point - chord).dot(normal).abs());
            if !point.is_finite() || !du.is_finite() || !dv.is_finite() || !departure.is_finite() {
                return Err(fail("nonfinite constrained cell interior sample"));
            }
            worst = worst.max(departure);
        }
        let angle = [(0, 1), (1, 2), (2, 0)].iter().map(|&(a, b)| normals[a].dot(normals[b]).clamp(-1.0, 1.0).acos()).fold(0.0, f64::max);
        if worst > tolerance || turn.is_some_and(|turn| turn.exceeds(angle, worst)) {
            return Err(fail("constrained cells require further display refinement"));
        }
    }
    Ok(())
}
