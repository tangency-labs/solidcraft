//! A snapshot's internal consistency: what [`super::Snapshot::validate`] checks.

use std::collections::BTreeSet;

use super::{Body, Bounds, Knots, Snapshot, Surface};

type Result = std::result::Result<(), String>;

fn check(ok: bool, why: impl FnOnce() -> String) -> Result {
    if ok { Ok(()) } else { Err(why()) }
}

pub(super) fn snapshot(s: &Snapshot) -> Result {
    let materials: BTreeSet<&str> = s.materials.iter().map(|m| m.name.as_str()).collect();
    let mut names = BTreeSet::new();
    for b in &s.bodies {
        check(names.insert(b.name.as_str()), || format!("two bodies are named `{}`", b.name))?;
        if let Some(m) = &b.material {
            check(materials.contains(m.as_str()), || format!("body `{}`: no material `{m}` in the snapshot", b.name))?;
        }
        body(b).map_err(|e| format!("body `{}`: {e}", b.name))?;
    }
    Ok(())
}

fn body(b: &Body) -> Result {
    let (np, nf, ne, nv) = (b.positions.len(), b.faces.len(), b.edges.len(), b.vertices.len());
    check(b.positions.iter().flat_map(|p| p.0).all(f64::is_finite), || "a position is not finite".into())?;
    for (i, f) in b.faces.iter().enumerate() {
        check(f.index.index() == i, || format!("face {i} says it is face {}", f.index))?;
        check(f.triangles.iter().flatten().all(|v| v.index() < np), || format!("face `{}`: a triangle's vertex is out of range", f.name))?;
        surface(&f.surface).map_err(|e| format!("face `{}`: {e}", f.name))?;
    }
    for (i, e) in b.edges.iter().enumerate() {
        check(e.index.index() == i, || format!("edge {i} says it is edge {}", e.index))?;
        check(e.bounds.faces().all(|f| f.index() < nf), || format!("edge `{}`: a face it bounds is out of range", e.name))?;
        if let Bounds::Between(a, c) = e.bounds {
            check(a != c, || format!("edge `{}` is between face {a} and itself (a seam)", e.name))?;
        }
        check(e.ends.iter().flatten().all(|v| v.index() < nv), || format!("edge `{}`: an end is out of range", e.name))?;
        check(e.path.iter().all(|v| v.index() < np), || format!("edge `{}`: a path vertex is out of range", e.name))?;
    }
    for (i, v) in b.vertices.iter().enumerate() {
        check(v.index.index() == i, || format!("corner {i} says it is corner {}", v.index))?;
        check(v.point.0.iter().all(|c| c.is_finite()), || format!("corner `{}` is not finite", v.name))?;
        check(v.position.is_none_or(|p| p.index() < np), || format!("corner `{}`: its mesh vertex is out of range", v.name))?;
        check(v.edges.iter().all(|e| e.index() < ne), || format!("corner `{}`: an edge is out of range", v.name))?;
    }
    Ok(())
}

fn surface(s: &Surface) -> Result {
    let Surface::Nurbs { u, v, net } = s else { return Ok(()) };
    let rows = net.len();
    let cols = net.first().map_or(0, Vec::len);
    check(rows > 0 && cols > 0 && net.iter().all(|r| r.len() == cols), || "the NURBS net is not a full grid".into())?;
    knots(u, rows).map_err(|e| format!("NURBS u: {e}"))?;
    knots(v, cols).map_err(|e| format!("NURBS v: {e}"))?;
    check(net.iter().flatten().all(|c| c.point.0.iter().all(|x| x.is_finite()) && c.weight.is_finite() && c.weight > 0.0), || {
        "a NURBS control point is not finite or its weight not positive".into()
    })
}

fn knots(k: &Knots, points: usize) -> Result {
    let want = points.checked_add(k.degree as usize + 1);
    check(want == Some(k.knots.len()), || format!("{} knots for {points} points of degree {} (want points + degree + 1)", k.knots.len(), k.degree))?;
    check(k.knots.iter().all(|x| x.is_finite()) && k.knots.windows(2).all(|w| w[0] <= w[1]), || "the knots are not finite and non-decreasing".into())
}
