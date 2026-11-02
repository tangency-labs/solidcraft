//! AP242 indexed triangles, read from the public EXPRESS entity definitions:
//! <https://steptools.com/docs/stp_aim/html/t_triangulated_surface_set.html>.

use solidcraft_geom::Vec3;

use super::{Body, Ctx, Entity, Param, geom::R};

// Bound allocations and mesh repair work independently of the exchange parser's text limit.
const MAX_VERTICES: usize = 1_000_000;
const MAX_TRIANGLES: usize = 2_000_000;

pub(super) fn is_item(e: &Entity) -> bool {
    e.records.iter().any(|r| (r.name.contains("TESSELLATED") || r.name.contains("TRIANGULATED")) && !r.name.ends_with("REPRESENTATION"))
}

fn count(p: Option<&Param>, limit: usize) -> R<usize> {
    let n = p.and_then(Param::as_i64).and_then(|n| usize::try_from(n).ok()).ok_or("invalid tessellated count")?;
    if n == 0 || n > limit {
        return Err(format!("tessellated count {n} outside 1..={limit}"));
    }
    Ok(n)
}

fn list(p: Option<&Param>) -> R<&[Param]> {
    p.and_then(Param::as_list).ok_or_else(|| "missing tessellated list".into())
}

fn index(p: &Param, len: usize) -> R<usize> {
    let i = p.as_i64().and_then(|i| usize::try_from(i).ok()).and_then(|i| i.checked_sub(1));
    i.filter(|i| *i < len).ok_or_else(|| "tessellated index refers to a missing vertex".into())
}

pub(super) fn read(cx: &Ctx<'_>, id: u64) -> R<Body> {
    let e = cx.ex.get(id).ok_or("missing tessellated item")?;
    // Other surface sets, faces, shells and solids need their own topology decoding.
    if e.is_complex() || e.name() != "TRIANGULATED_SURFACE_SET" {
        return Err(format!("tessellated geometry {} is not supported yet", e.name()));
    }
    let p = e.params();
    let coords = p.get(1).and_then(Param::as_ref_id).and_then(|id| cx.ex.get(id)).ok_or("missing tessellated coordinates")?;
    if coords.is_complex() || coords.name() != "COORDINATES_LIST" {
        return Err("unsupported tessellated coordinates".into());
    }
    let cp = coords.params();
    let npoints = count(cp.get(1), MAX_VERTICES)?;
    let points = list(cp.get(2))?;
    if points.len() != npoints {
        return Err("tessellated coordinate count does not match npoints".into());
    }
    let pnmax = count(p.get(2), MAX_VERTICES)?;
    let normals = list(p.get(3))?;
    if !normals.is_empty() && normals.len() != 1 && normals.len() != pnmax {
        return Err("invalid tessellated normal count".into());
    }
    let pnindex = list(p.get(4))?;
    if (pnindex.is_empty() && pnmax != npoints) || (!pnindex.is_empty() && pnindex.len() != pnmax) {
        return Err("invalid tessellated coordinate mapping count".into());
    }
    let facets = list(p.get(5))?;
    if facets.is_empty() || facets.len() > MAX_TRIANGLES {
        return Err(format!("tessellated triangle count outside 1..={MAX_TRIANGLES}"));
    }
    let mut positions = Vec::with_capacity(npoints);
    for point in points {
        let xyz = list(Some(point))?;
        if xyz.len() != 3 {
            return Err("tessellated coordinate must have three numbers".into());
        }
        let number = |i| xyz.get(i).and_then(Param::as_f64).map(|n| n * cx.len).ok_or_else(|| "invalid tessellated coordinate".to_string());
        let v = Vec3::new(number(0)?, number(1)?, number(2)?);
        if !v.is_finite() || v.x.abs().max(v.y.abs()).max(v.z.abs()) >= 1e9 {
            return Err("non-finite or huge tessellated coordinate".into());
        }
        positions.push(v);
    }
    let positions = if pnindex.is_empty() {
        positions
    } else {
        pnindex
            .iter()
            .map(|p| positions.get(index(p, npoints)?).copied().ok_or_else(|| "missing tessellated coordinate mapping".into()))
            .collect::<R<Vec<_>>>()?
    };
    let mut triangles = Vec::with_capacity(facets.len());
    for facet in facets {
        let corners = list(Some(facet))?;
        let [a, b, c] = corners else { return Err("tessellated triangle must have three indices".into()) };
        let vertex = |p: &Param| -> R<u32> {
            let i = index(p, pnmax)?;
            u32::try_from(i).map_err(|_| "too many tessellated vertices".into())
        };
        triangles.push([vertex(a)?, vertex(b)?, vertex(c)?]);
    }
    crate::mesh_body(&positions, &triangles).map_err(|e| e.to_string())
}
