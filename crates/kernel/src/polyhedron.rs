//! Convex polyhedra from planes, and the local operations on a body's faces: shell (hollow out
//! with open faces), draft (tilt faces about a neutral plane) and offset faces, which are SMLib's
//! ([`crate::offset_backend`]). A polyhedron is its schema 4 record built directly from its
//! planes (vertices are triple-plane intersections, edges lines, faces planes, pcurves exact),
//! taken by SMLib and checked there.

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self as rec, Curve2, EdgeUseRef, Loop, OrientedEdge, OrientedFace};

use crate::body::Body;
use crate::{KernelError, Result};

/// A half-space `n · x <= d` (n is the outward unit normal).
#[derive(Clone, Copy, Debug)]
pub struct HalfSpace {
    pub n: Vec3,
    pub d: f64,
}

fn det3(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    a.dot(b.cross(c))
}

/// Vertices, and per half-space with a face its vertex loop (counter-clockwise about the
/// outward normal), of the convex polyhedron bounded by the half-spaces.
fn combinatorics(hs: &[HalfSpace]) -> Result<(Vec<Vec3>, Vec<(usize, Vec<usize>)>)> {
    if hs.len() < 4 || hs.len() > 200 {
        return Err(KernelError::Invalid("a polyhedron needs 4…200 planes".into()));
    }
    let scale = hs.iter().map(|h| h.d.abs()).fold(1.0, f64::max);
    let eps = scale * 1e-9;
    // Vertices: intersections of three planes inside all others.
    let mut verts: Vec<Vec3> = Vec::new();
    for i in 0..hs.len() {
        for j in (i + 1)..hs.len() {
            for k in (j + 1)..hs.len() {
                let (a, b, c) = (hs[i], hs[j], hs[k]);
                let den = det3(a.n, b.n, c.n);
                if den.abs() < 1e-12 {
                    continue;
                }
                let x = (b.n.cross(c.n) * a.d + c.n.cross(a.n) * b.d + a.n.cross(b.n) * c.d) / den;
                if !x.is_finite() || hs.iter().any(|h| h.n.dot(x) > h.d + eps * 10.0) {
                    continue;
                }
                if !verts.iter().any(|v| v.dist(x) < scale * 1e-8) {
                    verts.push(x);
                }
            }
        }
    }
    if verts.len() < 4 {
        return Err(KernelError::Invalid("the planes don't enclose a solid".into()));
    }
    // Faces: the vertices on each plane, ordered counter-clockwise about its outward normal.
    let mut faces_idx: Vec<(usize, Vec<usize>)> = Vec::new();
    for (hi, h) in hs.iter().enumerate() {
        let on: Vec<usize> = (0..verts.len()).filter(|i| verts.get(*i).is_some_and(|v| (h.n.dot(*v) - h.d).abs() < scale * 1e-7)).collect();
        if on.len() < 3 {
            continue;
        }
        let c = on.iter().filter_map(|i| verts.get(*i)).fold(Vec3::ZERO, |a, v| a + *v) / on.len() as f64;
        let u = h.n.any_perp();
        let w = h.n.cross(u);
        let mut ordered = on.clone();
        ordered.sort_by(|a, b| {
            let ang = |i: usize| verts.get(i).map(|v| (*v - c).dot(w).atan2((*v - c).dot(u))).unwrap_or(0.0);
            ang(*a).total_cmp(&ang(*b))
        });
        faces_idx.push((hi, ordered));
    }
    Ok((verts, faces_idx))
}

/// The convex polyhedron bounded by the half-spaces.
pub fn convex_polyhedron(hs: &[HalfSpace]) -> Result<Body> {
    let (verts, faces) = combinatorics(hs)?;
    crate::smlib_sweeps::body_from_record(&polyhedron_record(hs, &verts, &faces)?, "polyhedron")
}

/// The record of the closed polyhedron with corners `verts` whose faces are `faces` (the
/// half-space each lies on, and its corners counter-clockwise about that half-space's outward
/// normal): each face on its plane with the outward normal as the natural one, each edge a line
/// used by its two faces, everything numbered in canonical order (faces as given, edges and
/// vertices as the faces first meet them).
fn polyhedron_record(hs: &[HalfSpace], verts: &[Vec3], faces: &[(usize, Vec<usize>)]) -> Result<rec::Brep> {
    let bad = || KernelError::Failed("polyhedron: the planes do not close a solid".into());
    let p = |i: usize| verts.get(i).map(|v| [v.x, v.y, v.z]).ok_or_else(bad);
    let scale = verts.iter().map(|v| v.len()).fold(1.0, f64::max);
    let tolerance = (scale * 1e-9).max(1e-7);
    // Each edge by its corners as first met, with its uses in walking order.
    let mut edge_of: HashMap<(usize, usize), u32> = HashMap::new();
    let mut ends: Vec<(usize, usize)> = Vec::new();
    let mut uses: Vec<Vec<EdgeUseRef>> = Vec::new();
    let mut out = Vec::with_capacity(faces.len());
    for (fi, (hi, cycle)) in faces.iter().enumerate() {
        let n = hs.get(*hi).and_then(|h| h.n.normalized()).ok_or_else(bad)?;
        let first = cycle.first().ok_or_else(bad)?;
        let o = verts.get(*first).copied().ok_or_else(bad)?;
        let (u, v) = (n.any_perp(), n.cross(n.any_perp()));
        let uv = |q: [f64; 3]| {
            let d = Vec3::new(q[0], q[1], q[2]) - o;
            [d.dot(u), d.dot(v)]
        };
        let (mut lo, mut hi_uv) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut edges = Vec::with_capacity(cycle.len());
        for k in 0..cycle.len() {
            let (Some(&a), Some(&b)) = (cycle.get(k), cycle.get((k + 1) % cycle.len())) else { return Err(bad()) };
            let (e, forward) = match edge_of.get(&(b, a)) {
                Some(&e) => (e, false),
                None => {
                    if edge_of.contains_key(&(a, b)) {
                        return Err(bad());
                    }
                    let e = u32::try_from(ends.len()).map_err(|_| bad())?;
                    edge_of.insert((a, b), e);
                    ends.push((a, b));
                    uses.push(Vec::new());
                    (e, true)
                }
            };
            let face = u32::try_from(fi).map_err(|_| bad())?;
            let position = u32::try_from(k).map_err(|_| bad())?;
            uses.get_mut(e as usize).ok_or_else(bad)?.push(EdgeUseRef { face, r#loop: 0, position, upper: !forward });
            let &(s, t) = ends.get(e as usize).ok_or_else(bad)?;
            let (ps, pt) = (uv(p(s)?), uv(p(t)?));
            for q in [ps, pt] {
                for i in 0..2 {
                    lo[i] = lo[i].min(q[i]);
                    hi_uv[i] = hi_uv[i].max(q[i]);
                }
            }
            // The line in the plane's parameters, on the edge's interval in its direction.
            edges.push(OrientedEdge { edge: e, forward, pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![ps, pt] } });
        }
        let w = |x: Vec3| [x.x, x.y, x.z];
        out.push(rec::Face {
            nurbs: None,
            surface: rec::Surface::Plane { origin: w(o), u_axis: w(u), v_axis: w(v) },
            domain: [[lo[0], hi_uv[0]], [lo[1], hi_uv[1]]],
            loops: vec![Loop::Edges { edges }],
        });
    }
    // Vertices as the edges first meet them.
    let mut vertex_of: HashMap<usize, u32> = HashMap::new();
    let mut vertices: Vec<rec::Vertex> = Vec::new();
    let mut edges = Vec::with_capacity(ends.len());
    for ((s, t), uses) in ends.iter().copied().zip(uses) {
        if uses.len() != 2 {
            return Err(bad());
        }
        let mut number = |c: usize| -> Result<u32> {
            if let Some(&i) = vertex_of.get(&c) {
                return Ok(i);
            }
            let i = u32::try_from(vertices.len()).map_err(|_| bad())?;
            vertices.push(rec::Vertex { point: p(c)?, tolerance });
            vertex_of.insert(c, i);
            Ok(i)
        };
        let vs = [number(s)?, number(t)?];
        edges.push(rec::Edge { nurbs: None, curve: rec::Curve::Line { start: p(s)?, end: p(t)? }, interval: [0.0, 1.0], vertices: vs, tolerance, uses });
    }
    let count = u32::try_from(out.len()).map_err(|_| bad())?;
    let brep = rec::Brep {
        version: rec::VERSION,
        tolerance,
        regions: vec![rec::Region { shells: vec![0], void: false }, rec::Region { shells: vec![0], void: false }],
        shells: vec![rec::Shell::Faces { faces: (0..count).map(|face| OrientedFace { face, forward: true }).collect(), wires: vec![] }],
        faces: out,
        edges,
        vertices,
    };
    brep.validate().map_err(|e| KernelError::Failed(format!("polyhedron: {e}")))?;
    Ok(brep)
}

/// Hollow a body: walls of `thickness` inside, open at the faces under `open` (SMLib's shell).
/// The result's history is over `[b]`: the inner walls are offsets of the faces they line.
pub fn shell(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    crate::offset_backend::shell(b, open, thickness)
}

/// Tilt the faces near `faces` by `angle` about their line on the neutral plane, so that they
/// lean in (positive angle) going along `pull` (SMLib's draft).
pub fn draft(b: &Body, faces: &[Vec3], neutral: &solidcraft_geom::Plane, pull: Vec3, angle: f64) -> Result<Body> {
    b.require_brep("draft")?;
    crate::offset_backend::draft(b, faces, neutral, pull, angle)
}

/// Move the faces at the given points along their outward normals by `distance` (negative: into
/// the body), SMLib's offset of faces: the faces around them are extended or trimmed to meet them.
pub fn offset_faces(b: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    b.require_brep("offset faces")?;
    if !distance.is_finite() || distance.abs() > 1e6 {
        return Err(KernelError::Invalid("offset distance".into()));
    }
    let healed = crate::boolean_record::healed(b)?.with_split_lines(b.splits.clone());
    let size = healed.size();
    let mesh = healed.mesh(&crate::MeshTolerance::chord((size * 1e-3).max(1e-3)))?;
    let mut chosen: Vec<usize> = Vec::new();
    for p in at {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (solidcraft_geom::compare::closest_on_triangle(*p, x, y, z).dist(*p), *f as usize)))
            .min_by(|a, c| a.0.total_cmp(&c.0));
        match near {
            Some((d, f)) if d < size * 1e-3 + 1e-6 => {
                if !chosen.contains(&f) {
                    chosen.push(f);
                }
            }
            _ => return Err(KernelError::Invalid(format!("no face at {:?}", [p.x, p.y, p.z]))),
        }
    }
    let chosen = crate::offset::with_same_surface(&healed, &chosen);
    let moved = crate::offset_backend::offset_faces(&healed, &chosen, distance)?;
    Ok(crate::record::after(moved, &healed, &crate::record::counts(&[b])))
}
