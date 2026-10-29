//! Equal-slope round solids can have reducible intersections: two conics meet at
//! tangent points where a general surface tracer cannot choose the next branch.
//! Factor the difference of their implicit quadrics into planes instead. In each
//! cell one quadric dominates; clip that operand by the cell and both solids' end
//! planes, then remove the artificial planar seams. All wall sections are exact
//! rational conics and every retained face comes from an original operand.

use solidcraft_geom::{Plane, Vec3};
use truck_modeling::{BoundedCurve, ParametricCurve};

use crate::boolean_record as recorded;
use crate::history::{Change, FaceOrigin};
use crate::round_cap_cut::{RoundStock, convex_round_stock};
use crate::{Body, KernelError, Result, guard, record};

fn fail(detail: &str) -> KernelError {
    KernelError::Failed(format!("round intersection: {detail}"))
}

fn radical_planes(a: RoundStock, mut b: RoundStock, tol: f64) -> Option<([RoundStock; 2], Vec<Plane>)> {
    let dot = a.axis.dot(b.axis);
    if dot < -1. + 1e-10 {
        b.base += b.axis * b.height;
        b.axis = -b.axis;
        b.radii.swap(0, 1);
        b.caps.swap(0, 1);
    }
    let slope = (a.radii[1] - a.radii[0]) / a.height;
    let other = (b.radii[1] - b.radii[0]) / b.height;
    if (slope - other).abs() * a.height.max(b.height) > tol {
        return None;
    }
    let delta = b.base - a.base;
    let cos = a.axis.dot(b.axis);
    let planes = if a.axis.cross(b.axis).len() < 1e-10 {
        let axial = delta.dot(a.axis);
        let offset = delta - a.axis * axial;
        let ra = a.radii[0];
        let rb = b.radii[0] - slope * axial;
        let n = offset - a.axis * (slope * (ra - rb));
        if n.len() <= tol {
            return None;
        }
        vec![Plane::from_normal(a.base + n * ((offset.len2() + ra * ra - rb * rb) / (2. * n.len2())), n)?]
    } else {
        let denominator = 1. - cos * cos;
        if denominator < 1e-6 {
            return None;
        }
        let ta = (delta.dot(a.axis) - cos * delta.dot(b.axis)) / denominator;
        let tb = (cos * delta.dot(a.axis) - delta.dot(b.axis)) / denominator;
        let pa = a.base + a.axis * ta;
        let pb = b.base + b.axis * tb;
        let radius = a.radii[0] + slope * ta;
        if pa.dist(pb) > tol || radius <= tol || (radius - b.radii[0] - slope * tb).abs() > tol {
            return None;
        }
        let center = (pa + pb) * 0.5;
        let n = (a.axis + b.axis) * (1. + slope * slope);
        if n.len() < 1e-6 {
            return None;
        }
        vec![Plane::from_normal(center, a.axis - b.axis)?, Plane::from_normal(center - n * (2. * radius * slope / n.len2()), n)?]
    };
    Some(([a, b], planes))
}

fn clip(body: Body, stock: RoundStock, plane: Plane, positive: bool, cap: Option<FaceOrigin>, counts: &[(usize, usize)]) -> Result<Option<Body>> {
    let radial = plane.normal().cross(stock.axis).len();
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for (center, radius) in [(stock.base, stock.radii[0]), (stock.base + stock.axis * stock.height, stock.radii[1])] {
        low = low.min(plane.height(center) - radius * radial);
        high = high.max(plane.height(center) + radius * radial);
    }
    let tol = stock.height * 1e-10 + 1e-10;
    // These are bounds on the full certified frustum, hence also on every cell
    // already clipped from it. No sampled outline is used to skip a constraint.
    if (positive && low >= -tol) || (!positive && high <= tol) {
        return Ok(Some(body));
    }
    if (positive && high < -tol) || (!positive && low > tol) {
        return Ok(None);
    }
    let parts = crate::splitface::split_convex_body(&body, &plane, &stock)?;
    let mut kept = None;
    for part in parts {
        let mesh = part.tessellate_with((stock.height * 1e-5).max(1e-6), true)?;
        let side = mesh.positions.iter().map(|p| plane.height(*p)).sum::<f64>();
        if (side > 0.) != positive {
            continue;
        }
        if kept.is_some() {
            return Err(fail("a convex half-space produced several pieces"));
        }
        let own = part.history().ok_or_else(|| fail("section history"))?;
        let generated: Vec<_> = own.faces.iter().enumerate().filter_map(|(i, f)| f.origins.is_empty().then_some(i)).collect();
        let mut history = own.clone().over(&[body.history().ok_or_else(|| fail("cell history"))?])?;
        if let Some(origin) = cap {
            for index in generated {
                let face = history.faces.get_mut(index).ok_or_else(|| fail("section face"))?;
                face.origins = vec![origin];
                face.change = Change::Modified;
            }
        }
        // Newly made sections have no parent edge. Once a real end face replaces
        // the artificial plane, its boundary is the meeting of the retained faces.
        for edge in &mut history.edges {
            if edge.origins.is_empty() {
                edge.origins = record::meeting(&edge.faces, &history.faces);
            }
        }
        kept = Some(part.with_history_counts(history, counts)?);
    }
    Ok(kept)
}

fn glue(a: &Body, b: &Body, counts: &[(usize, usize)]) -> Result<Option<Body>> {
    let va = a.solid.vertex_iter().map(|v| crate::body::from_p3(v.point())).collect::<Vec<_>>();
    let vb = b.solid.vertex_iter().map(|v| crate::body::from_p3(v.point())).collect::<Vec<_>>();
    let refine = |body: &Body, vertices: &[Vec3]| -> Result<Body> {
        let initial = recorded::operand(body, 0, &record::counts(&[body]));
        let refined = crate::splitface::split_edges_at(&initial, vertices)?;
        let history = refined
            .history()
            .ok_or_else(|| fail("seam subdivision history"))?
            .clone()
            .over(&[body.history().ok_or_else(|| fail("seam history"))?])?;
        refined.with_history_counts(history, counts)
    };
    let a = refine(a, &vb)?;
    let b = refine(b, &va)?;
    let Some(joined) = crate::coplanar::glue(&a, &b) else { return Ok(None) };
    let joined = joined?;
    let history = joined
        .history()
        .ok_or_else(|| fail("sewing history"))?
        .clone()
        .over(&[a.history().ok_or_else(|| fail("left seam history"))?, b.history().ok_or_else(|| fail("right seam history"))?])?;
    Ok(Some(joined.with_history_counts(history, counts)?))
}

pub(crate) fn intersect(a: &Body, b: &Body) -> Option<Result<Body>> {
    let sa = convex_round_stock(a)?;
    let sb = convex_round_stock(b)?;
    let size = a.size().max(b.size());
    let (stocks, planes) = radical_planes(sa, sb, (size * 1e-10).max(1e-10))?;
    Some(guard("round intersection", || {
        let operands = [a, b];
        let counts = record::counts(&operands);
        let mut pieces = Vec::new();
        for mask in 0..1usize << planes.len() {
            let left = if planes.len() == 1 { mask == 1 } else { (mask & 1 != 0) != (mask & 2 != 0) };
            let operand = usize::from(!left);
            let stock = *stocks.get(operand).ok_or_else(|| fail("cell operand"))?;
            let mut piece = Some(recorded::operand(operands.get(operand).ok_or_else(|| fail("operand"))?, operand, &counts));
            // First split at the height bisector, then impose both finite stocks'
            // caps, then the second factor. At most four convex cells result.
            let mut cuts = vec![(*planes.first().ok_or_else(|| fail("factor plane"))?, mask & 1 != 0, None)];
            for (i, s) in stocks.iter().enumerate() {
                cuts.push((
                    Plane::from_normal(s.base, s.axis).ok_or_else(|| fail("bottom plane"))?,
                    true,
                    Some(FaceOrigin::Face { operand: i, face: s.caps[0] }),
                ));
                cuts.push((
                    Plane::from_normal(s.base + s.axis * s.height, s.axis).ok_or_else(|| fail("top plane"))?,
                    false,
                    Some(FaceOrigin::Face { operand: i, face: s.caps[1] }),
                ));
            }
            if let Some(plane) = planes.get(1) {
                cuts.push((*plane, mask & 2 != 0, None));
            }
            for (plane, positive, cap) in cuts {
                if let Some(body) = piece.take() {
                    piece = clip(body, stock, plane, positive, cap, &counts)?;
                }
            }
            if let Some(body) = piece {
                pieces.push(body);
            }
        }
        while pieces.len() > 1 {
            let mut combined = None;
            'pairs: for i in 0..pieces.len() {
                for j in i + 1..pieces.len() {
                    let (Some(a), Some(b)) = (pieces.get(i), pieces.get(j)) else { return Err(fail("sewing operands")) };
                    if let Some(body) = glue(a, b, &counts)? {
                        combined = Some((i, j, body));
                        break 'pairs;
                    }
                }
            }
            let Some((i, j, body)) = combined else { return Err(fail("cell seams do not match")) };
            pieces.remove(j);
            pieces.remove(i);
            pieces.push(body);
        }
        let result = pieces.pop().ok_or_else(|| fail("no positive intersection"))?;
        if !result.validity().is_empty() || result.lumps()?.len() != 1 {
            return Err(fail("invalid convex result"));
        }
        let history = result.history().ok_or_else(|| fail("result history"))?;
        history.check(&result, &operands)?;
        if history.faces.iter().any(|f| f.origins.is_empty()) {
            return Err(fail("an artificial section survived"));
        }
        // Every retained boundary must satisfy both finite quadric inequalities.
        let margin = |s: &RoundStock, p: Vec3| {
            let q = p - s.base;
            let h = q.dot(s.axis);
            let r = s.radii[0] + (s.radii[1] - s.radii[0]) * h / s.height;
            h.min(s.height - h).min(r - (q - s.axis * h).len())
        };
        for edge in result.solid.edge_iter() {
            let c = edge.curve();
            let (lo, hi) = c.range_tuple();
            for i in 0..=32 {
                let p = crate::body::from_p3(c.subs(lo + (hi - lo) * i as f64 / 32.));
                if !p.is_finite() {
                    return Err(fail("a non-finite section"));
                }
                let margins = stocks.map(|s| margin(&s, p));
                let tol = size * 1e-7 + 1e-8;
                if margins.iter().any(|m| !m.is_finite() || *m < -tol) || margins.iter().all(|m| m.abs() > tol) {
                    return Err(fail("a section does not lie on the intersection boundary"));
                }
            }
        }
        Ok(result)
    }))
}
