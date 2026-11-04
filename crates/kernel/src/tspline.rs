//! T-spline sheets: a T-spline surface (`solidcraft_geom::tspline`) as a surface body of NURBS
//! faces. A T-mesh without T-junctions becomes one clamped tensor face; otherwise each Bézier
//! element is a face, and the faces share their edges and vertices — a side that meets two
//! neighbours is split at the T-point between them — so the sheet is one sewn open shell.
//!
//! Every edge is an exact piece of its patch's side (a Bézier segment of the side's control
//! points) and its pcurve on each face it bounds a straight line in that patch's parameters.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use solidcore_record::brep as b;
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

/// A tensor NURBS surface as an open sheet, retaining its exact rational boundaries
/// and original surface parameterization. Chart regularization is opt-in through
/// [`fitted_nurbs_sheet`].
pub fn nurbs_sheet(surface: &NurbsSurface) -> Result<Body> {
    crate::surfaces::tensor_sheet(&surface.clamped().map_err(spline_err)?)
}

/// A subdivision tensor sheet, retaining its exact rim but using a different
/// fitted chart where a smooth rim corner makes the original chart singular.
/// The returned distance is sampled in both directions, not a certified bound.
pub fn fitted_nurbs_sheet(surface: &NurbsSurface, tolerance: f64) -> Result<(Body, f64)> {
    let [[u0, u1], [v0, v1]] = surface.domain();
    let wire = vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let body = nurbs_sheet(surface)?;
    recharted(surface, &[wire], tolerance, body)
}

/// [`fitted_nurbs_sheet`] for a trimmed region of the source tensor patch.
pub fn fitted_nurbs_region_sheet(surface: &NurbsSurface, boundaries: &[Vec<[f64; 2]>], tolerance: f64) -> Result<(Body, f64)> {
    let body = nurbs_region_sheet(surface, boundaries)?;
    recharted(surface, boundaries, tolerance, body)
}

fn recharted(surface: &NurbsSurface, boundaries: &[Vec<[f64; 2]>], tolerance: f64, body: Body) -> Result<(Body, f64)> {
    use solidcraft_geom::prelude::*;
    use solidcraft_geom::tess::EvalSurface;
    if !tolerance.is_finite() || !(0.0..=1.0).contains(&tolerance) {
        return Err(KernelError::Invalid("the subdivision chart error budget must be finite and 0..1 mm".into()));
    }
    // Only this fitted subdivision API may use a different nonplanar atlas.
    let body = planar_supported(surface, body)?;
    if body.record().faces.iter().all(|face| matches!(face.surface, b::Surface::Plane { .. })) {
        return Ok((body, 0.0));
    }
    let eval = EvalSurface::from(surface);
    let [[u0, u1], [v0, v1]] = surface.domain();
    let singular = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]].into_iter().any(|[u, v]| {
        eval.derivs(u, v)
            .and_then(|(_, a, c)| Some(a.normalized()?.cross(c.normalized()?).len()))
            .is_none_or(|angle| angle < 0.5_f64.to_radians().sin())
    });
    if singular { crate::fill::rechart_tensor(&body, surface, boundaries, tolerance) } else { Ok((body, 0.0)) }
}

/// One exact trimmed face, using an analytic support only when its complete
/// positive-weight control hull certifies a plane. Nonplanar charts and their
/// UV map are unchanged: this never fits, composes a pole chart or adds faces.
pub fn nurbs_region_sheet_with_planar_support(surface: &NurbsSurface, boundaries: &[Vec<[f64; 2]>]) -> Result<Body> {
    planar_supported(surface, nurbs_region_sheet(surface, boundaries)?)
}

fn planar_supported(surface: &NurbsSurface, body: Body) -> Result<Body> {
    let mut planar = body.record().clone();
    for face in &mut planar.faces {
        crate::surfaces::planar_tensor_face(surface, &planar.edges, face)?;
    }
    if planar.faces.iter().all(|face| matches!(face.surface, b::Surface::Plane { .. })) { surface_body(&planar) } else { Ok(body) }
}
/// Trim one tensor surface by closed straight wires in its parameter plane.
/// The edge and its pcurve share one parameter interval, including oblique trims.
pub fn nurbs_region_sheet(surface: &NurbsSurface, boundaries: &[Vec<[f64; 2]>]) -> Result<Body> {
    if boundaries.is_empty()
        || boundaries.len() > 128
        || boundaries.iter().any(|w| !(3..=1024).contains(&w.len()))
        || boundaries.iter().map(Vec::len).sum::<usize>() > 100_000
    {
        return Err(fail("a trimmed region requires bounded closed wires"));
    }
    let mut parts = Parts::default();
    let mut vertices = HashMap::<Key, u32>::new();
    let mut loops = Vec::with_capacity(boundaries.len());
    let mut domain = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for wire in boundaries {
        let mut uses = Vec::with_capacity(wire.len());
        for (&a, &z) in wire.iter().zip(wire.iter().cycle().skip(1)).take(wire.len()) {
            if a.iter().chain(&z).any(|v| !v.is_finite()) || a == z {
                return Err(fail("trim edges need finite distinct parameter points"));
            }
            let varying = if a[1] == z[1] {
                Some(0)
            } else if a[0] == z[0] {
                Some(1)
            } else {
                None
            };
            let (knots, controls, from, to, forward) = if let Some(varying) = varying {
                let dir = if varying == 0 { solidcraft_geom::bspline::Dir::S } else { solidcraft_geom::bspline::Dir::T };
                let range = [a[varying].min(z[varying]), a[varying].max(z[varying])];
                let (knots, controls) = surface.iso_curve_segment(dir, a[1 - varying], range).map_err(spline_err)?;
                if a[varying] < z[varying] { (knots, controls, a, z, true) } else { (knots, controls, z, a, false) }
            } else {
                let (knots, controls) = surface.parameter_line(a, z).map_err(spline_err)?;
                (knots, controls, a, z, true)
            };
            let mut corner = |uv: [f64; 2]| -> Result<u32> {
                let k = key(uv[0], uv[1]);
                if let Some(&v) = vertices.get(&k) {
                    return Ok(v);
                }
                let p = surface.eval(uv[0], uv[1]).map_err(spline_err)?;
                if !p.is_finite() {
                    return Err(fail("trim corner does not evaluate to a finite point"));
                }
                let v = index(parts.vertices.len())?;
                parts.vertices.push(b::Vertex { point: [p.x, p.y, p.z], tolerance: 0.0 });
                bounds.add(p);
                vertices.insert(k, v);
                Ok(v)
            };
            let ends = [corner(from)?, corner(to)?];
            for uv in [a, z] {
                for (range, value) in domain.iter_mut().zip(uv) {
                    range[0] = range[0].min(value);
                    range[1] = range[1].max(value);
                }
            }
            let (curve, interval) = spline_curve(&controls, knots)?;
            let edge = index(parts.edges.len())?;
            parts.edges.push(b::Edge { nurbs: None, curve, interval, vertices: ends, tolerance: 0.0, uses: Vec::new() });
            uses.push(b::OrientedEdge { edge, forward, pcurve: uv_line(from, to, interval) });
        }
        loops.push(b::Loop::Edges { edges: uses });
    }
    if domain.iter().any(|r| !(r[1] > r[0])) {
        return Err(fail("trim parameter domain is degenerate"));
    }
    parts.tolerance = (bounds.diagonal() * 1e-6).max(1e-5);
    for vertex in &mut parts.vertices {
        vertex.tolerance = parts.tolerance;
    }
    for edge in &mut parts.edges {
        edge.tolerance = parts.tolerance;
    }
    let face = b::Face { nurbs: None, surface: spline_surface(surface), domain, loops };
    parts.faces.push((face, true));
    surface_body(&parts.canonical()?.brep)
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
                        parts.edges.push(b::Edge { nurbs: None, curve, interval, vertices, tolerance: 0.0, uses: Vec::new() });
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
            b::Face { nurbs: None, surface: spline_surface(&surface), domain: [[0.0, 1.0], [0.0, 1.0]], loops: vec![b::Loop::Edges { edges: uses }] },
            true,
        ));
    }
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for c in &corners {
        bounds.add(solidcore_record::Vec3::new(c[0], c[1], c[2]));
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
    use solidcore_record::Vec3;
    use solidcraft_geom::bspline::NurbsSurface;
    use solidcraft_geom::prelude::*;
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
        let record = body.record();
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
    fn generic_planar_tensor_and_trimmed_sheets_keep_their_original_uv_map() {
        use solidcraft_geom::brep::eval::SurfaceEval;
        let knots = [vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0], vec![-3.0, -3.0, -3.0, 7.0, 7.0, 7.0]];
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for j in 0..3 {
            for i in 0..3 {
                let (x, y) = ([0.0, 0.1, 1.0][i], [0.0, 0.7, 1.0][j]);
                points.push(Vec3::new(10.0 * x, 4.0 * y, 5.0 + 2.0 * x + 3.0 * y));
                weights.push(1.0 + 0.15 * (i + j) as f64);
            }
        }
        let surface = NurbsSurface::new([2, 2], knots, &points, &weights).unwrap();
        let wire = vec![[2.5, -1.0], [4.5, -1.0], [4.5, 6.0], [2.5, 6.0]];
        for (body, domain) in [
            (super::nurbs_sheet(&surface).unwrap(), surface.domain()),
            (super::nurbs_region_sheet(&surface, &[wire]).unwrap(), [[2.5, 4.5], [-1.0, 6.0]]),
        ] {
            let face = &body.record().faces[0];
            assert_eq!(face.domain, domain);
            let exact = SurfaceEval::new(face.parameters()).unwrap();
            for uv in [[2.6, -0.7], [3.1, 2.4], [4.3, 5.8]] {
                assert!(exact.point(uv).unwrap().dist(surface.eval(uv[0], uv[1]).unwrap()) < 1e-11, "original UV map at{uv:?}");
            }
        }
    }
    #[test]
    fn planar_only_regions_preserve_one_face_and_a_nonplanar_singular_chart() {
        let mut points = Vec::new();
        for j in 0..4 {
            for i in 0..4 {
                points.push(Vec3::new(i as f64, j as f64, if (1..3).contains(&i) && (1..3).contains(&j) { 1.0 } else { 0.0 }));
            }
        }
        points[4] = points[1]; // Parallel corner tangents, but a nonplanar interior.
        let surface = NurbsSurface::new([3, 3], std::array::from_fn(|_| vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]), &points, &[1.0; 16]).unwrap();
        let wire = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let exact = super::nurbs_region_sheet(&surface, std::slice::from_ref(&wire)).unwrap();
        let supported = super::nurbs_region_sheet_with_planar_support(&surface, &[wire]).unwrap();
        assert_eq!(supported.face_count(), 1);
        assert_eq!(supported.edge_count(), exact.edge_count());
        assert_eq!(supported.record().faces[0].surface, exact.record().faces[0].surface);
        assert_eq!(supported.record().faces[0].domain, exact.record().faces[0].domain);
        let eval = solidcraft_geom::brep::eval::SurfaceEval::new(supported.record().faces[0].parameters()).unwrap();
        for uv in [[0.1, 0.2], [0.5, 0.5], [0.8, 0.7]] {
            assert!(eval.point(uv).unwrap().dist(surface.eval(uv[0], uv[1]).unwrap()) < 1e-12);
        }
    }
    #[test]
    fn fitted_planar_tensor_sheets_use_regular_supports_with_exact_rational_trims() {
        use solidcore_record::brep::{self as b, Curve2};
        use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
        use solidcraft_geom::bspline::HPoint;
        use solidcraft_geom::subdivision::primitives::Parameters;
        let cage = Parameters { kind: "plane".into(), dimensions: [10.0; 3], segments: [1; 3], ..Default::default() }.build().unwrap();
        let fitted = cage.fit_patches(0.05).unwrap().patches.remove(0);
        let controls = fitted
            .homogeneous()
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let p = h.project().unwrap();
                HPoint::weighted(Vec3::new(p.x + 0.3 * p.y, 0.7 * p.y, 5.0 + 0.5 * p.x + 0.25 * p.y), [0.8, 1.0, 1.2][i % 3])
            })
            .collect();
        let surface = NurbsSurface::from_homogeneous(fitted.degree(), fitted.knots().clone(), controls).unwrap();
        for body in [
            super::fitted_nurbs_sheet(&surface, 0.0).unwrap().0,
            super::fitted_nurbs_region_sheet(&surface, &[vec![[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]]], 0.0).unwrap().0,
            super::nurbs_region_sheet_with_planar_support(&surface, &[vec![[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]]]).unwrap(),
        ] {
            let r = body.record();
            assert!(matches!(r.faces[0].surface, b::Surface::Plane { .. }));
            let support = SurfaceEval::new(r.faces[0].parameters()).unwrap();
            for boundary in &r.faces[0].loops {
                let b::Loop::Edges { edges } = boundary else { panic!("edge loop") };
                for edge_use in edges {
                    assert!(matches!(edge_use.pcurve, Curve2::RationalSpline { .. }));
                    let edge = &r.edges[edge_use.edge as usize];
                    assert!(matches!(edge.curve, b::Curve::RationalSpline { .. }));
                    let exact = CurveEval::new(edge.parameters()).unwrap();
                    for f in [0.0, 0.13, 0.57, 0.91, 1.0] {
                        let t = edge.interval[0] * (1.0 - f) + edge.interval[1] * f;
                        let uv = solidcraft_geom::brep::pcurve::eval(&edge_use.pcurve, t).unwrap();
                        let lifted = support.derivs(uv[0], uv[1]).unwrap().0;
                        assert!(lifted.dist(exact.point(t).unwrap()) < 1e-10);
                    }
                }
            }
            let native = solidcraft_smlib::Brep::from_record(r, true).unwrap();
            assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
            let (sewn, closed) = native.stitch(1e-7).unwrap();
            assert!(!closed);
            assert!(sewn.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
        }
    }

    #[test]
    fn planar_support_never_flattens_a_nonplanar_tensor_interior() {
        let points: Vec<_> = (0..4)
            .flat_map(|j| (0..4).map(move |i| Vec3::new(i as f64, j as f64, if (1..3).contains(&i) && (1..3).contains(&j) { 1e-8 } else { 0.0 })))
            .collect();
        let surface = NurbsSurface::new([3, 3], std::array::from_fn(|_| vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]), &points, &[1.0; 16]).unwrap();
        let (body, deviation) = super::fitted_nurbs_sheet(&surface, 0.0).unwrap();
        assert_eq!(deviation, 0.0);
        assert!(matches!(body.record().faces[0].surface, solidcore_record::brep::Surface::Spline { .. }));
        let exact = solidcraft_geom::brep::eval::SurfaceEval::new(body.record().faces[0].parameters()).unwrap();
        let centre = exact.derivs(0.5, 0.5).unwrap().0;
        assert!((centre.z - 5.625e-9).abs() < 1e-20);
    }

    #[test]
    fn sculpted_open_subdivision_charts_retain_their_rim_and_pass_native_checks() {
        use solidcraft_geom::subdivision::primitives::Parameters;
        for picked in [2, 4] {
            let mut cage = Parameters { kind: "face".into(), dimensions: [10.0; 3], segments: [2; 3], ..Default::default() }.build().unwrap();
            cage.points[picked].z = 2.0;
            let fit = cage.fit_patches(0.05).unwrap();
            let mut sheets = Vec::new();
            for patch in &fit.patches {
                let exact = super::nurbs_sheet(patch).unwrap();
                let (sheet, deviation) = super::fitted_nurbs_sheet(patch, 0.05 - fit.sampled_deviation).unwrap();
                assert!(fit.sampled_deviation + deviation <= 0.05);
                assert_eq!(deviation, 0.0, "exact pole charts must retain the original fitting error");
                let native = solidcraft_smlib::Brep::from_record(sheet.record(), true).unwrap();
                assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
                // Every original rim point remains on an exact output edge;
                // the alternative chart changes only the surface's interior fit.
                let old = crate::face_geometry(&exact, 0).unwrap();
                let new = crate::edge_geometry(&sheet, &(0..sheet.edge_count()).collect::<Vec<_>>()).unwrap();
                for edge in old.loops.into_iter().flatten() {
                    for f in [0.0, 0.13, 0.57, 0.91, 1.0] {
                        let point = edge.at(f).unwrap().0;
                        assert!(new.iter().any(|curve| {
                            let distance = |t: f64| curve.at(t).unwrap().0.dist(point);
                            let best: usize = (0..=32).min_by(|a, b| distance(*a as f64 / 32.0).total_cmp(&distance(*b as f64 / 32.0))).unwrap();
                            let mut lo = best.saturating_sub(1) as f64 / 32.0;
                            let mut hi = (best + 1).min(32) as f64 / 32.0;
                            for _ in 0..72 {
                                let a = lo + (hi - lo) / 3.0;
                                let b = hi - (hi - lo) / 3.0;
                                if distance(a) < distance(b) {
                                    hi = b;
                                } else {
                                    lo = a;
                                }
                            }
                            distance((lo + hi) * 0.5) < 1e-9
                        }));
                    }
                }
                sheets.push(sheet);
            }
            let sewn = crate::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7).unwrap();
            assert!(sewn.is_surface());
            assert_eq!(sewn.face_count(), 20);
            assert_eq!(sewn.edge_count(), 44);
            let native = solidcraft_smlib::Brep::from_record(sewn.record(), true).unwrap();
            assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
        }
    }

    #[test]
    fn dissolved_subdivision_cusps_use_exact_pole_charts_and_close_as_cad() {
        use solidcore_record::brep;
        use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
        use solidcraft_geom::subdivision::primitives::Parameters;
        let cube = Parameters { segments: [1; 3], dimensions: [10.0; 3], ..Default::default() }.build().unwrap();
        let cage = cube.dissolve_edge([cube.faces[0][0], cube.faces[0][1]]).unwrap();
        assert!(cage.faces.iter().any(|face| face.len() > 4));
        let fit = cage.fit_patches(0.05).unwrap();
        let mut sheets = Vec::new();
        for patch in &fit.patches {
            let (sheet, additional_error) = super::fitted_nurbs_sheet(patch, 0.05 - fit.sampled_deviation).unwrap();
            assert_eq!(additional_error, 0.0, "pole composition must preserve the source exactly");
            let record = sheet.record();
            let native = solidcraft_smlib::Brep::from_record(record, true).unwrap();
            assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
            for face in &record.faces {
                let support = SurfaceEval::new(face.parameters()).unwrap();
                for boundary in &face.loops {
                    let brep::Loop::Edges { edges } = boundary else { panic!("edge loop") };
                    for edge_use in edges {
                        let edge = &record.edges[edge_use.edge as usize];
                        let curve = CurveEval::new(edge.parameters()).unwrap();
                        for f in [0.0, 0.13, 0.57, 0.91, 1.0] {
                            let t = edge.interval[0] * (1.0 - f) + edge.interval[1] * f;
                            let uv = solidcraft_geom::brep::pcurve::eval(&edge_use.pcurve, t).unwrap();
                            assert!(support.point(uv).unwrap().dist(curve.point(t).unwrap()) < 1e-10);
                        }
                    }
                }
            }
            sheets.push(sheet);
        }
        let sewn = crate::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7).unwrap();
        assert!(!sewn.is_surface());
        assert!(sewn.validity().is_empty());
        assert_eq!(sewn.face_count(), 38);
        let native = solidcraft_smlib::Brep::from_record(sewn.record(), true).unwrap();
        assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
    }

    #[test]
    fn recharted_leaf_trims_keep_original_curves_and_exact_native_pcurves() {
        use solidcore_record::brep;
        use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
        use solidcraft_geom::subdivision::primitives::Parameters;
        let mut cage = Parameters { kind: "face".into(), dimensions: [10.0; 3], segments: [2; 3], ..Default::default() }.build().unwrap();
        cage.points[2].z = 2.0;
        let source = cage.fit_patches(0.05).unwrap().patches.remove(0);
        for wire in [
            vec![[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [0.0, 1.0]],
            vec![[0.5, 0.0], [1.0, 0.0], [1.0, 1.0], [0.5, 1.0]],
            vec![[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]],
        ] {
            let exact = super::nurbs_region_sheet(&source, std::slice::from_ref(&wire)).unwrap();
            let (fitted, deviation) = super::fitted_nurbs_region_sheet(&source, &[wire], 0.04).unwrap();
            assert!(deviation <= 0.04);
            let original_curves = crate::edge_geometry(&exact, &(0..exact.edge_count()).collect::<Vec<_>>()).unwrap();
            let record = fitted.record();
            for face in &record.faces {
                let support = SurfaceEval::new(face.parameters()).unwrap();
                for boundary in &face.loops {
                    let brep::Loop::Edges { edges } = boundary else { panic!("edge loop") };
                    for edge_use in edges {
                        let edge = &record.edges[edge_use.edge as usize];
                        let curve = CurveEval::new(edge.parameters()).unwrap();
                        for fraction in [0.0, 0.13, 0.57, 0.91, 1.0] {
                            let t = edge.interval[0] * (1.0 - fraction) + edge.interval[1] * fraction;
                            let point = curve.point(t).unwrap();
                            let uv = solidcraft_geom::brep::pcurve::eval(&edge_use.pcurve, t).unwrap();
                            assert!(support.point(uv).unwrap().dist(point) < 1e-7);
                            if edge.uses.len() == 1 {
                                assert!(original_curves.iter().any(|old| {
                                    let distance = |t: f64| old.at(t).unwrap().0.dist(point);
                                    let best: usize =
                                        (0..=32).min_by(|a, b| distance(*a as f64 / 32.0).total_cmp(&distance(*b as f64 / 32.0))).unwrap();
                                    let mut lo = best.saturating_sub(1) as f64 / 32.0;
                                    let mut hi = (best + 1).min(32) as f64 / 32.0;
                                    for _ in 0..72 {
                                        let a = lo + (hi - lo) / 3.0;
                                        let b = hi - (hi - lo) / 3.0;
                                        if distance(a) < distance(b) {
                                            hi = b;
                                        } else {
                                            lo = a;
                                        }
                                    }
                                    distance((lo + hi) * 0.5) < 1e-8
                                }));
                            }
                        }
                    }
                }
            }
            let native = solidcraft_smlib::Brep::from_record(record, true).unwrap();
            assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
        }
    }

    #[test]
    fn sharp_thickened_subdivision_sheet_sews_without_singular_planar_supports() {
        use solidcraft_geom::subdivision::primitives::Parameters;
        let cage = Parameters { kind: "face".into(), dimensions: [10.0; 3], segments: [2; 3], ..Default::default() }
            .build()
            .unwrap()
            .thicken(2.0, true)
            .unwrap();
        let patches = cage.fit_patches(0.02).unwrap();
        let sheets: Vec<_> = patches.patches.iter().map(|surface| super::fitted_nurbs_sheet(surface, 0.0).unwrap().0).collect();
        let solid = crate::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-7).unwrap();
        assert!(!solid.is_surface());
        assert!(solid.validity().is_empty());
        let native = solidcraft_smlib::Brep::from_record(solid.record(), true).unwrap();
        assert!(native.check(solidcraft_smlib::CheckLevel::All).unwrap().is_valid());
        let (lo, hi) = native.bounds().unwrap();
        assert!(lo[2].abs() < 1e-10 && (hi[2] - 2.0).abs() < 1e-10);
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
