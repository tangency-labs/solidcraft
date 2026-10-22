//! T-spline sheets: a T-spline surface (`solidcraft_geom::tspline`) as a surface body of NURBS
//! faces. A T-mesh without T-junctions becomes one clamped tensor face; otherwise each Bézier
//! element is a face, and the faces share their edges and vertices — a side that meets two
//! neighbours is split at the T-point between them — so the sheet is one sewn open shell.
//!
//! Every edge is an exact piece of its patch's side (a Bézier segment of the side's control
//! points) and its pcurve on each face it bounds a straight line in that patch's parameters.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use solidcraft_geom::brep as b;
use solidcraft_geom::bspline::{BezierPatch, HPoint, NurbsSurface, SplineError, bezier_segment};
use solidcraft_geom::tspline::TSpline;

use crate::body::Body;
use crate::surfaces::{Parts, spline_curve, spline_surface, surface_body, uv_line};
use crate::{KernelError, Result};

/// A T-spline as a surface body (an open shell of NURBS faces whose normals follow
/// `∂S/∂s × ∂S/∂t`).
pub fn tspline_sheet(ts: &TSpline) -> Result<Body> {
    if ts.mesh().is_regular() {
        let n = ts.to_nurbs().and_then(|n| n.clamped()).map_err(spline_err)?;
        return crate::surfaces::tensor_sheet(&n);
    }
    let patches = ts.bezier_elements().map_err(spline_err)?;
    surface_body(&patch_faces(&patches)?.canonical()?.brep)
}

fn spline_err(e: SplineError) -> KernelError {
    KernelError::Invalid(format!("t-spline: {e}"))
}

fn fail(m: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("t-spline sheet: {m}"))
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| KernelError::TooLarge("t-spline sheet: too many patches".into()))
}

/// A parameter point, exact (knot values are shared, so equal points have equal bits).
type Key = (u64, u64);

fn key(s: f64, t: f64) -> Key {
    ((s + 0.0).to_bits(), (t + 0.0).to_bits())
}

fn st(k: Key) -> [f64; 2] {
    [f64::from_bits(k.0), f64::from_bits(k.1)]
}

/// One face per Bézier patch, sharing vertices and edges; sides are split where a neighbour's
/// corner lands on them.
fn patch_faces(patches: &[BezierPatch]) -> Result<Parts> {
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
    let mut parts = Parts::default();
    let mut verts: HashMap<Key, u32> = HashMap::new();
    // Each edge by its ends' parameters, in the direction it was made.
    let mut edges: HashMap<(Key, Key), u32> = HashMap::new();
    let mut corners: Vec<[f64; 3]> = Vec::new();
    for p in patches {
        // The patch's own (u, v) on [0, 1]² of a global (s, t).
        let local = |k: Key| {
            let [s, t] = st(k);
            [(s - p.s[0]) / (p.s[1] - p.s[0]), (t - p.t[0]) / (p.t[1] - p.t[0])]
        };
        let mut uses: Vec<b::OrientedEdge> = Vec::new();
        for side in 0..4 {
            // Side 0: t = t0, 1: s = s1, 2: t = t1, 3: s = s0; each runs with its parameter rising,
            // sides 2 and 3 are walked backwards.
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
                // The edge as made (a to b with its parameter rising), and whether this side runs
                // along it.
                let (edge, made) = match (edges.get(&(ka, kb)), edges.get(&(kb, ka))) {
                    (Some(e), _) => (*e, (ka, kb)),
                    (None, Some(e)) => (*e, (kb, ka)),
                    (None, None) => {
                        let seg = bezier_segment(&ctrl, (x0 - lo) / (hi - lo), (x1 - lo) / (hi - lo)).map_err(spline_err)?;
                        let (Some(&first), Some(&last)) = (seg.first(), seg.last()) else { return Err(fail("an empty side")) };
                        let mut corner = |k: Key, h: HPoint| -> Result<u32> {
                            if let Some(v) = verts.get(&k) {
                                return Ok(*v);
                            }
                            let q = h.project().ok_or_else(|| fail("a corner has no position"))?;
                            let v = index(corners.len())?;
                            corners.push([q.x, q.y, q.z]);
                            verts.insert(k, v);
                            Ok(v)
                        };
                        let vertices = [corner(ka, first)?, corner(kb, last)?];
                        let degree = seg.len().saturating_sub(1);
                        let knots = std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect();
                        let (curve, interval) = spline_curve(&seg, knots)?;
                        let e = index(parts.edges.len())?;
                        parts.edges.push(b::Edge { curve, interval, vertices, tolerance: 0.0, uses: Vec::new() });
                        edges.insert((ka, kb), e);
                        (e, (ka, kb))
                    }
                };
                let pcurve = uv_line(local(made.0), local(made.1), [0.0, 1.0]);
                pieces.push(b::OrientedEdge { edge, forward: made == (ka, kb), pcurve });
            }
            if side >= 2 {
                pieces.reverse();
                pieces.iter_mut().for_each(|u| u.forward = !u.forward);
            }
            uses.extend(pieces);
        }
        let [ps, pt] = p.degree;
        let knots = |d: usize| std::iter::repeat_n(0.0, d + 1).chain(std::iter::repeat_n(1.0, d + 1)).collect::<Vec<_>>();
        let ctrl: Vec<HPoint> = (0..=pt).flat_map(|j| (0..=ps).map(move |i| (i, j))).map(|(i, j)| p.point(i, j)).collect();
        let surface = NurbsSurface::from_homogeneous([ps, pt], [knots(ps), knots(pt)], ctrl).map_err(spline_err)?;
        parts.faces.push((
            b::Face { surface: spline_surface(&surface), domain: [[0.0, 1.0], [0.0, 1.0]], loops: vec![b::Loop::Edges { edges: uses }] },
            true,
        ));
    }
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for c in &corners {
        bounds.add(solidcraft_geom::Vec3::new(c[0], c[1], c[2]));
    }
    parts.tolerance = (bounds.diagonal() * 1e-6).max(1e-5);
    parts.vertices = corners.into_iter().map(|point| b::Vertex { point, tolerance: parts.tolerance }).collect();
    for e in &mut parts.edges {
        e.tolerance = parts.tolerance;
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::Vec3;
    use solidcraft_geom::bspline::NurbsSurface;
    use solidcraft_geom::tspline::{Cut, TSpline};

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
        let record = crate::native::brep_record(&body).unwrap();
        assert!(
            record.edges.iter().all(|e| e.uses.len() == 1 || (e.uses.len() == 2 && e.uses[0].upper != e.uses[1].upper)),
            "neighbours share edges, run opposite ways"
        );
        let euler = body.vertex_count() as i64 - body.edge_count() as i64 + body.face_count() as i64;
        assert_eq!(euler, 1, "one sewn sheet (a disc)");
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
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
                let _ = body.mesh(&crate::MeshTolerance::chord(0.01));
            }
            if let Ok(r) = ts.refine(&[Cut::S { s: 0.33, t0: 0.3, t1: 0.6 }])
                && let Ok(body) = tspline_sheet(&r)
            {
                let _ = body.mesh(&crate::MeshTolerance::chord(0.01));
            }
        }
    }
}
