//! 3D sketch curves from the model: curves projected onto faces (on their display meshes), the
//! bodies and faces an intersection curve is taken on, and spun profiles.

use solidcraft_geom::{Mesh, Plane, Vec2, Vec3};
use solidcraft_sketch::LinkSource;

use crate::eval::ModelState;

/// The body of a body or face reference, and the face (its index in the body's face order) for a
/// face reference: the face nearest its point, on the named body or else the nearest body.
pub(crate) fn part_of<'a>(st: &'a ModelState, src: &LinkSource) -> Option<(&'a crate::ModelBody, Option<usize>)> {
    match src {
        LinkSource::Body { body } => Some((st.body(body)?, None)),
        LinkSource::Face { body, at } => {
            let b = st.body(body).or_else(|| st.bodies.iter().min_by(|x, y| dist_to(&x.mesh(), *at).total_cmp(&dist_to(&y.mesh(), *at))))?;
            let (f, _, _) = crate::project::nearest_face(&b.mesh(), *at)?;
            Some((b, usize::try_from(f).ok()))
        }
        _ => None,
    }
}

fn dist_to(m: &Mesh, p: Vec3) -> f64 {
    crate::project::nearest_face(m, p).map(|x| x.2).unwrap_or(f64::INFINITY)
}

fn tris(m: &Mesh, face: Option<u32>) -> Vec<[Vec3; 3]> {
    m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| face.is_none_or(|x| x == **f)).filter_map(|(t, _)| m.tri(t)).collect()
}

/// Ray–triangle hit parameter (Möller–Trumbore), any sign.
fn ray_hit(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

/// Project a world polyline onto face `f` along `dir` (either way, nearest hit). Points that
/// miss the face split the result.
pub(crate) fn onto_face(m: &Mesh, f: u32, pts: &[Vec3], dir: Vec3) -> Vec<Vec<Vec3>> {
    let Some(d) = dir.normalized() else { return Vec::new() };
    let ts = tris(m, Some(f));
    let size = m.bounds().diagonal().max(1e-9);
    // Resample so the projected curve follows the surface.
    let step = size / 200.0;
    let mut dense: Vec<Vec3> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((a.dist(b) / step).ceil() as usize).clamp(1, 2000);
        for i in 0..n {
            dense.push(a.lerp(b, i as f64 / n as f64));
        }
        if dense.len() > 200_000 {
            break;
        }
    }
    if let Some(l) = pts.last() {
        dense.push(*l);
    }
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    for p in dense {
        let hit = ts.iter().filter_map(|t| ray_hit(p, d, t)).min_by(|a, b| a.abs().total_cmp(&b.abs()));
        match hit {
            Some(t) => cur.push(p + d * t),
            None => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Outline of a body spun about the axis `origin + t·dir`: the largest distance from the axis
/// at each station along it, laid into the sketch plane on the side of the axis the plane's
/// in-plane perpendicular points to.
pub(crate) fn spun_outline(m: &Mesh, origin: Vec3, dir: Vec3, plane: &Plane) -> Vec<Vec2> {
    let Some(d) = dir.normalized() else { return Vec::new() };
    let Some(u) = plane.normal().cross(d).normalized() else { return Vec::new() };
    let ax = |p: Vec3| (p - origin).dot(d);
    let rad = |p: Vec3| {
        let v = p - origin;
        (v - d * v.dot(d)).len()
    };
    let (t0, t1) = m.positions.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(ax(*p)), b.max(ax(*p))));
    if !(t0.is_finite() && t1 > t0) {
        return Vec::new();
    }
    let n = 240;
    let mut out = Vec::new();
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let mut r = f64::NEG_INFINITY;
        for tri in &m.triangles {
            let Some(p) = m.tri(tri) else { continue };
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                let (ha, hb) = (ax(a) - t, ax(b) - t);
                if ha.abs() < 1e-12 {
                    r = r.max(rad(a));
                }
                if (ha > 0.0) != (hb > 0.0) && (ha - hb).abs() > 1e-15 {
                    r = r.max(rad(a.lerp(b, ha / (ha - hb))));
                }
            }
        }
        if r.is_finite() {
            out.push(plane.to_local(origin + d * t + u * r));
        }
    }
    out
}

/// An isoparametric curve of face `f` through `p`: the face cut by a plane through `p`. For
/// curved faces `u` runs around (the plane across the face's straight direction, e.g. a
/// cylinder's axis) and `v` along it; planar faces use the face plane's own axes. An explicit
/// direction `"x,y,z"` cuts along it.
pub(crate) fn iso_curve(m: &Mesh, f: u32, p: Vec3, dir: &str) -> Vec<Vec<Vec3>> {
    let ns: Vec<Vec3> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, x)| **x == f)
        .filter_map(|(t, _)| m.tri(t))
        .filter_map(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized())
        .collect();
    let Some(n0) = crate::project::nearest_face(m, p).and_then(|_| ns.first().copied()) else { return Vec::new() };
    // Normal at p: of the nearest triangle of the face.
    let np = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, x)| **x == f)
        .filter_map(|(t, _)| m.tri(t))
        .min_by(|a, b| tri_dist(p, a).total_cmp(&tri_dist(p, b)))
        .and_then(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized())
        .unwrap_or(n0);
    // The face's straight direction: perpendicular to how its normals vary.
    let straight = ns.iter().map(|n| np.cross(*n)).filter(|c| c.len() > 1e-3).fold(None::<Vec3>, |acc, c| match acc {
        None => c.normalized(),
        Some(a) => (a + if a.dot(c) < 0.0 { -c } else { c }).normalized(),
    });
    let along = match dir.trim().to_ascii_lowercase().as_str() {
        "u" | "v" => {
            let s = straight.unwrap_or_else(|| Plane::from_normal(p, np).map(|pl| pl.x).unwrap_or(Vec3::X));
            if dir.trim().eq_ignore_ascii_case("v") { s } else { np.cross(s) }
        }
        other => {
            let v: Vec<f64> = other.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
            match v[..] {
                [x, y, z] => Vec3::new(x, y, z),
                _ => return Vec::new(),
            }
        }
    };
    // Cutting plane through p containing the face normal and the curve direction.
    let Some(cut_n) = np.cross(along).normalized() else { return Vec::new() };
    // Nudged off the point so the cut does not run exactly along mesh edges (a seam); the side
    // that cuts more of the face wins.
    let nudge = m.bounds().diagonal().max(1e-9) * 1e-8;
    let cut = |off: f64| -> Vec<Vec<Vec3>> {
        let Some(pl) = Plane::from_normal(p + cut_n * off, cut_n) else { return Vec::new() };
        let geom = crate::project::section(&pl, m, Some(f));
        let pieces: Vec<Vec<Vec3>> =
            geom.iter().map(|g| link_polyline(g).into_iter().map(|q| pl.to_world(q)).collect()).filter(|w: &Vec<Vec3>| w.len() >= 2).collect();
        join_wires(pieces, m.bounds().diagonal().max(1e-9) * 1e-6)
    };
    let len = |ws: &Vec<Vec<Vec3>>| ws.iter().flat_map(|w| w.windows(2).map(|x| x[0].dist(x[1]))).sum::<f64>();
    let (a, b) = (cut(nudge), cut(-nudge));
    if len(&a) >= len(&b) { a } else { b }
}

/// Join polylines whose ends meet into longer ones.
fn join_wires(mut ws: Vec<Vec<Vec3>>, tol: f64) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    while let Some(mut cur) = ws.pop() {
        let mut grew = true;
        while grew {
            grew = false;
            for i in 0..ws.len() {
                let (Some(cs), Some(ce)) = (cur.first().copied(), cur.last().copied()) else { break };
                let (Some(ws0), Some(we)) = (ws[i].first().copied(), ws[i].last().copied()) else { continue };
                let mut w = if ce.dist(ws0) < tol || cs.dist(we) < tol || ce.dist(we) < tol || cs.dist(ws0) < tol { ws.remove(i) } else { continue };
                if ce.dist(ws0) < tol {
                    cur.extend(w.drain(1..));
                } else if ce.dist(we) < tol {
                    w.reverse();
                    cur.extend(w.drain(1..));
                } else if cs.dist(we) < tol {
                    w.extend(cur.drain(1..));
                    cur = w;
                } else {
                    w.reverse();
                    w.extend(cur.drain(1..));
                    cur = w;
                }
                grew = true;
                break;
            }
        }
        out.push(cur);
    }
    out
}

fn tri_dist(p: Vec3, t: &[Vec3; 3]) -> f64 {
    let c = (t[0] + t[1] + t[2]) / 3.0;
    c.dist(p)
}

/// Sample a fitted planar curve.
fn link_polyline(g: &solidcraft_sketch::LinkGeom) -> Vec<Vec2> {
    use solidcraft_sketch::LinkGeom;
    match *g {
        LinkGeom::Line(a, b) => vec![a, b],
        LinkGeom::Circle(c, r) => (0..=96).map(|i| c + Vec2::from_angle(i as f64 / 96.0 * std::f64::consts::TAU) * r).collect(),
        LinkGeom::Arc { c, a, b } => {
            let (s0, mut sw) = ((a - c).angle(), (b - c).angle() - (a - c).angle());
            while sw <= 1e-12 {
                sw += std::f64::consts::TAU;
            }
            let r = c.dist(a);
            (0..=48).map(|i| c + Vec2::from_angle(s0 + sw * i as f64 / 48.0) * r).collect()
        }
        LinkGeom::Conic { a, apex, b, rho } => (0..=32).map(|i| solidcraft_sketch::conic_point(a, apex, b, rho, i as f64 / 32.0)).collect(),
        LinkGeom::Ellipse { c, major, minor } => {
            (0..=96).map(|i| solidcraft_sketch::ellipse_point(c, major, minor, i as f64 / 96.0 * std::f64::consts::TAU)).collect()
        }
        LinkGeom::Spline { ref pts, control, degree, periodic } => solidcraft_sketch::spline_polyline_mode(pts, control, degree, periodic),
        LinkGeom::Point(_) => Vec::new(),
    }
}
