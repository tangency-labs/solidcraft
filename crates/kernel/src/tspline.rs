//! T-spline sheets: a T-spline surface (`solidcraft_geom::tspline`) as a surface body of NURBS
//! faces. A T-mesh without T-junctions becomes one clamped tensor face; otherwise each Bézier
//! element is a face, and the faces share their edges and vertices — a side that meets two
//! neighbours is split at the T-point between them — so the sheet is one sewn open shell.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use solidcraft_geom::bspline::{BezierPatch, HPoint, NurbsSurface, SplineError, bezier_segment};
use solidcraft_geom::tspline::TSpline;
use truck_modeling as mt;

use crate::body::Body;
use crate::spline_conv::{h4, nurbs_curve, nurbs_surface};
use crate::{KernelError, Result, guard};

/// A T-spline as a surface body (an open shell of NURBS faces whose normals follow
/// `∂S/∂s × ∂S/∂t`).
pub fn tspline_sheet(ts: &TSpline) -> Result<Body> {
    let faces = if ts.mesh().is_regular() {
        let n = ts.to_nurbs().and_then(|n| n.clamped()).map_err(spline_err)?;
        guard("t-spline sheet", || tensor_face(&n).map(|f| vec![f]))?
    } else {
        let patches = ts.bezier_elements().map_err(spline_err)?;
        guard("t-spline sheet", || patch_faces(&patches))?
    };
    Ok(crate::surfaces::surface_body(faces))
}

/// A tensor NURBS surface as an open sheet, using the same guarded face constructor.
/// Callers with fitted NURBS data need not rebuild an intermediate T-spline control mesh.
pub fn nurbs_sheet(surface: &NurbsSurface) -> Result<Body> {
    let surface = surface.clamped().map_err(spline_err)?;
    guard("NURBS sheet", || Ok(crate::surfaces::surface_body(vec![tensor_face(&surface)?])))
}

/// Trim one tensor surface by closed straight wires in its parameter plane.
/// Each edge is an exact rational restriction of that surface, including
/// isoparametric seams and oblique guide edges.
pub fn nurbs_region_sheet(surface: &NurbsSurface, boundaries: &[Vec<[f64; 2]>]) -> Result<Body> {
    if boundaries.is_empty() || boundaries.len() > 128 || boundaries.iter().any(|w| !(3..=1024).contains(&w.len())) {
        return Err(fail("a trimmed region requires bounded closed wires"));
    }
    guard("NURBS region", || {
        let mut vertices: HashMap<Key, mt::Vertex> = HashMap::new();
        let mut wires = Vec::with_capacity(boundaries.len());
        for boundary in boundaries {
            let mut edges = Vec::with_capacity(boundary.len());
            for (&a, &b) in boundary.iter().zip(boundary.iter().cycle().skip(1)).take(boundary.len()) {
                let varying = if a[1] == b[1] && a[0] != b[0] {
                    Some(0)
                } else if a[0] == b[0] && a[1] != b[1] {
                    Some(1)
                } else {
                    None
                };
                let (knots, ctrl) = if let Some(varying) = varying {
                    let dir = if varying == 0 { solidcraft_geom::bspline::Dir::S } else { solidcraft_geom::bspline::Dir::T };
                    let range = [a[varying].min(b[varying]), a[varying].max(b[varying])];
                    surface.iso_curve_segment(dir, a[1 - varying], range).map_err(spline_err)?
                } else {
                    surface.parameter_line(a, b).map_err(spline_err)?
                };
                let mut corner = |uv: [f64; 2]| -> Result<mt::Vertex> {
                    let k = key(uv[0], uv[1]);
                    if let Some(v) = vertices.get(&k) {
                        return Ok(v.clone());
                    }
                    let p = surface.eval(uv[0], uv[1]).map_err(spline_err)?;
                    let v = mt::builder::vertex(crate::body::p3(p));
                    vertices.insert(k, v.clone());
                    Ok(v)
                };
                let (va, vb) = (corner(a)?, corner(b)?);
                let curve = nurbs_curve(mt::KnotVec::from(knots), &ctrl)?;
                let edge = if varying.is_none_or(|varying| a[varying] < b[varying]) {
                    mt::Edge::try_new(&va, &vb, curve).map_err(fail)?
                } else {
                    mt::Edge::try_new(&vb, &va, curve).map_err(fail)?.inverse()
                };
                edges.push(edge);
            }
            wires.push(edges.into());
        }
        let face = mt::Face::try_new(wires, crate::spline_conv::surface(surface)?).map_err(fail)?;
        Ok(crate::surfaces::surface_body(vec![face]))
    })
}

fn spline_err(e: SplineError) -> KernelError {
    KernelError::Invalid(format!("t-spline: {e}"))
}

fn fail(m: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("t-spline sheet: {m}"))
}

fn vertex(h: HPoint) -> Result<mt::Vertex> {
    let p = h.project().ok_or_else(|| fail("a corner has no position"))?;
    Ok(mt::builder::vertex(crate::body::p3(p)))
}

/// One face from a clamped tensor surface, bounded by its four boundary curves.
fn tensor_face(n: &NurbsSurface) -> Result<mt::Face> {
    crate::spline_conv::tensor_face(n)
}

/// A parameter point, exact (knot values are shared, so equal points have equal bits).
type Key = (u64, u64);

fn key(s: f64, t: f64) -> Key {
    ((s + 0.0).to_bits(), (t + 0.0).to_bits())
}

/// One face per Bézier patch, sharing vertices and edges; sides are split where a
/// neighbour's corner lands on them.
fn patch_faces(patches: &[BezierPatch]) -> Result<Vec<mt::Face>> {
    // Corner parameters on each line of constant t (their s values) and of constant s.
    let mut on_t: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut on_s: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for p in patches {
        for s in p.s {
            for t in p.t {
                let (ks, kt) = key(s, t);
                on_t.entry(kt).or_default().insert(ks);
                on_s.entry(ks).or_default().insert(kt);
            }
        }
    }
    let mut verts: HashMap<Key, mt::Vertex> = HashMap::new();
    let mut edges: HashMap<(Key, Key), mt::Edge> = HashMap::new();
    let mut faces = Vec::with_capacity(patches.len());
    for p in patches {
        let mut wire: Vec<mt::Edge> = Vec::new();
        for side in 0..4 {
            // Side 0: t = t₀, 1: s = s₁, 2: t = t₁, 3: s = s₀; each runs with its parameter
            // rising, sides 2 and 3 are walked backwards.
            let (along_s, fixed, [lo, hi]) = match side {
                0 => (true, p.t[0], p.s),
                1 => (false, p.s[1], p.t),
                2 => (true, p.t[1], p.s),
                _ => (false, p.s[0], p.t),
            };
            let line = if along_s { on_t.get(&key(0.0, fixed).1) } else { on_s.get(&key(fixed, 0.0).0) };
            let mut breaks: Vec<f64> = line.into_iter().flatten().map(|&b| f64::from_bits(b)).filter(|&x| x > lo && x < hi).collect();
            breaks.insert(0, lo);
            breaks.push(hi);
            let ctrl = p.side(side);
            let at = |x: f64| if along_s { key(x, fixed) } else { key(fixed, x) };
            let mut pieces = Vec::with_capacity(breaks.len());
            for w in breaks.windows(2) {
                let &[x0, x1] = w else { continue };
                let (ka, kb) = (at(x0), at(x1));
                let e = if let Some(e) = edges.get(&(ka, kb)) {
                    e.clone()
                } else if let Some(e) = edges.get(&(kb, ka)) {
                    e.inverse()
                } else {
                    let seg = bezier_segment(&ctrl, (x0 - lo) / (hi - lo), (x1 - lo) / (hi - lo)).map_err(spline_err)?;
                    let (Some(&first), Some(&last)) = (seg.first(), seg.last()) else { return Err(fail("an empty side")) };
                    let mut corner = |k: Key, h: HPoint| -> Result<mt::Vertex> {
                        Ok(match verts.entry(k) {
                            Entry::Occupied(e) => e.get().clone(),
                            Entry::Vacant(e) => e.insert(vertex(h)?).clone(),
                        })
                    };
                    let (va, vb) = (corner(ka, first)?, corner(kb, last)?);
                    let curve = nurbs_curve(mt::KnotVec::bezier_knot(seg.len() - 1), &seg)?;
                    let e = mt::Edge::try_new(&va, &vb, curve).map_err(fail)?;
                    edges.insert((ka, kb), e.clone());
                    e
                };
                pieces.push(e);
            }
            if side >= 2 {
                pieces.reverse();
                pieces.iter_mut().for_each(|e| *e = e.inverse());
            }
            wire.extend(pieces);
        }
        let [ps, pt] = p.degree;
        let rows: Vec<Vec<mt::Vector4>> = (0..=ps).map(|i| (0..=pt).map(|j| h4(p.point(i, j))).collect()).collect();
        let surf = nurbs_surface((mt::KnotVec::bezier_knot(ps), mt::KnotVec::bezier_knot(pt)), rows)?;
        faces.push(mt::Face::try_new(vec![wire.into()], surf).map_err(fail)?);
    }
    Ok(faces)
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::Vec3;
    use solidcraft_geom::bspline::NurbsSurface;
    use solidcraft_geom::tspline::{Cut, TSpline};
    use truck_modeling as mt;

    use super::tspline_sheet;
    use crate::fuzz::Rng;

    /// A rational bicubic with uneven knots over a 10 × 8 mm area, bulging up to ~3 mm.
    fn surface(seed: u64) -> TSpline {
        let mut rng = Rng::new(seed);
        let ku = vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.45, 0.5, 0.8, 1.0, 1.0, 1.0, 1.0];
        let kv = vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 1.0, 1.0, 1.0, 1.0];
        let (nu, nv) = (ku.len() - 4, kv.len() - 4);
        let mut pts = Vec::new();
        for j in 0..nv {
            for i in 0..nu {
                pts.push(Vec3::new(10.0 * i as f64 / (nu - 1) as f64, 8.0 * j as f64 / (nv - 1) as f64, 3.0 * rng.unit()));
            }
        }
        let w: Vec<f64> = (0..pts.len()).map(|_| 0.7 + 0.6 * rng.unit()).collect();
        TSpline::from_nurbs(&NurbsSurface::new([3, 3], [ku, kv], &pts, &w).unwrap()).unwrap()
    }

    /// Distance from `p` to the T-spline: nearest of a sample grid, then Gauss–Newton.
    fn distance(ts: &TSpline, p: Vec3) -> f64 {
        let [ds, dt] = ts.domain();
        let n = 24;
        let mut best = (f64::MAX, 0.0, 0.0);
        for i in 0..=n {
            for j in 0..=n {
                let (s, t) = (ds[0] + (ds[1] - ds[0]) * i as f64 / n as f64, dt[0] + (dt[1] - dt[0]) * j as f64 / n as f64);
                let d = ts.eval(s, t).unwrap().dist(p);
                if d < best.0 {
                    best = (d, s, t);
                }
            }
        }
        let (_, mut s, mut t) = best;
        for _ in 0..60 {
            let [q, a, b] = ts.derivs(s, t).unwrap();
            let r = q - p;
            let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
            let det = aa * bb - ab * ab;
            if det.abs() < 1e-30 {
                break;
            }
            let (ga, gb) = (a.dot(r), b.dot(r));
            s = (s - (bb * ga - ab * gb) / det).clamp(ds[0], ds[1]);
            t = (t - (aa * gb - ab * ga) / det).clamp(dt[0], dt[1]);
        }
        ts.eval(s, t).unwrap().dist(p)
    }

    fn check_sheet(ts: &TSpline, faces: usize) {
        let body = tspline_sheet(ts).unwrap();
        assert!(body.is_surface());
        assert_eq!(body.shell_count(), 1);
        assert_eq!(body.face_count(), faces);
        let shell = &body.solid.boundaries()[0];
        assert_eq!(shell.shell_condition(), mt::ShellCondition::Oriented, "neighbours share edges, run opposite ways");
        let euler = body.vertex_count() as i64 - body.edge_count() as i64 + body.face_count() as i64;
        assert_eq!(euler, 1, "one sewn sheet (a disc)");
        let mesh = body.tessellate(0.01).unwrap();
        assert!(!mesh.triangles.is_empty());
        // Mesh points lie on the surface (not merely within the tessellation tolerance).
        let step = (mesh.positions.len() / 300).max(1);
        let worst = mesh.positions.iter().step_by(step).map(|p| distance(ts, *p)).fold(0.0, f64::max);
        eprintln!("t-spline sheet: {faces} faces, {} mesh points, farthest checked {worst:e} mm from the surface", mesh.positions.len());
        assert!(worst < 1e-6, "a mesh point is {worst} mm off the T-spline");
    }

    #[test]
    fn regular_tspline_becomes_one_nurbs_face() {
        check_sheet(&surface(1), 1);
    }

    #[test]
    fn tspline_with_t_junctions_becomes_sewn_bezier_faces() {
        let ts = surface(2).refine(&[Cut::S { s: 0.33, t0: 0.3, t1: 0.6 }, Cut::T { t: 0.45, s0: 0.2, s1: 0.45 }]).unwrap();
        assert!(!ts.mesh().is_regular());
        let n = ts.bezier_elements().unwrap().len();
        check_sheet(&ts, n);
    }

    #[test]
    fn hostile_tsplines_never_panic() {
        let base = surface(3);
        let n = base.mesh().vertices().len();
        let mesh = base.mesh().clone();
        // Every control point at one place, enormous coordinates, a needle-thin patch.
        let cases: Vec<Vec<Vec3>> = vec![
            vec![Vec3::new(1.0, 2.0, 3.0); n],
            (0..n).map(|k| Vec3::new(1e300, -1e300, k as f64 * 1e299)).collect(),
            (0..n).map(|k| Vec3::new(k as f64 * 1e-12, 0.0, 0.0)).collect(),
        ];
        for pts in cases {
            let Ok(ts) = TSpline::new(mesh.clone(), [3, 3], &pts, &vec![1.0; n]) else { continue };
            if let Ok(body) = tspline_sheet(&ts) {
                let _ = body.tessellate(0.01);
            }
            if let Ok(r) = ts.refine(&[Cut::S { s: 0.33, t0: 0.3, t1: 0.6 }])
                && let Ok(body) = tspline_sheet(&r)
            {
                let _ = body.tessellate(0.01);
            }
        }
    }
}
