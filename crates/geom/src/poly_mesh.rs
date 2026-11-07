//! Polygon meshes for programs that edit what they are given (a live link to a modelling or
//! rendering program): one welded, watertight mesh per body, made of the polygons a person
//! would draw rather than the display triangles.
//!
//! A display tessellation ([`TessPlan`]) is a set of triangles per B-rep face. Here each face
//! becomes polygons: a planar face with one boundary loop is a single n-gon, a planar face with
//! holes a few convex n-gons through its boundary points (interior points are dropped either
//! way), the cells of structured grids, strips and patches become quads again (two
//! triangles that together form a convex, nearly rectangular quad), and what is left stays
//! triangles. Vertices are shared across faces, every corner keeps the face's own normal (so
//! creases stay crisp), every polygon names its B-rep face, and the segments of B-rep edges
//! carry flags (sharp, tangent, a seam within one surface) for the receiving program to mark.

use std::collections::{HashMap, HashSet};

use crate::convex_parts::convex_parts;
use crate::tess::{EvalSurface, TessPlan};
use crate::{Mesh, Vec2, Vec3};

/// Most vertices a polygon mesh may be made from (the plan's or mesh's vertices).
pub const MAX_POLY_VERTICES: usize = 1 << 24;

/// What polygons a face may become.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PolyTopology {
    /// Triangles only, welded.
    Triangles,
    /// Triangles paired into quads where they form good ones.
    Quads,
    /// As [`PolyTopology::Quads`], and a planar face with one boundary loop is one n-gon (with
    /// holes, a few convex n-gons).
    #[default]
    NGons,
}

/// How triangles are joined into polygons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolyOptions {
    pub topology: PolyTopology,
    /// Most a quad's corner may differ from a right angle (radians).
    pub max_corner_dev: f64,
    /// Most the two triangles of a quad may be folded against each other (radians).
    pub max_fold: f64,
    /// Points closer than this on open edges are joined (mm); 0 chooses from the mesh's size.
    pub weld_eps: f64,
    /// Faces whose normals along an edge differ by at most this meet tangentially (radians):
    /// the edge is marked tangent rather than sharp.
    pub tangent_angle: f64,
}

impl Default for PolyOptions {
    fn default() -> Self {
        PolyOptions {
            topology: PolyTopology::NGons,
            max_corner_dev: 45f64.to_radians(),
            max_fold: 30f64.to_radians(),
            weld_eps: 0.0,
            tangent_angle: 1f64.to_radians(),
        }
    }
}

/// Flags of a B-rep edge segment ([`PolyMesh::edge_flags`]).
pub mod edge_flag {
    /// Part of a B-rep edge (every listed segment has it).
    pub const BREP: u8 = 1;
    /// A crease: the faces meet at an angle.
    pub const SHARP: u8 = 2;
    /// The faces meet tangentially.
    pub const TANGENT: u8 = 4;
    /// A seam between two pieces of one smooth surface.
    pub const SURFACE_SEAM: u8 = 8;
}

/// Counts that describe a polygon mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolyStats {
    pub triangles: usize,
    pub quads: usize,
    pub ngons: usize,
    /// Edges used by one polygon only (zero for a closed solid).
    pub open_edges: usize,
}

/// A welded polygon mesh; see the module documentation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyMesh {
    pub positions: Vec<Vec3>,
    /// Corners of each polygon, in order.
    pub poly_sizes: Vec<u32>,
    /// Vertex of every corner, polygon after polygon (counter-clockwise seen from outside).
    pub loops: Vec<u32>,
    /// Normal of every corner.
    pub loop_normals: Vec<Vec3>,
    /// B-rep face of each polygon.
    pub poly_face: Vec<u32>,
    /// Segments of B-rep edges as vertex pairs (smaller index first), each once.
    pub edges: Vec<[u32; 2]>,
    /// [`edge_flag`] bits per segment.
    pub edge_flags: Vec<u8>,
    /// B-rep edge of each segment.
    pub edge_brep: Vec<u32>,
    pub stats: PolyStats,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PolyMeshError {
    #[error("the mesh has {0} vertices, more than a polygon mesh may have ({MAX_POLY_VERTICES})")]
    TooLarge(usize),
}

/// One corner: a vertex (welded by position) and the face's normal there.
#[derive(Clone, Copy, Debug)]
struct Corner {
    v: u32,
    n: Vec3,
}

/// One face's triangles before they are joined.
struct FaceTris {
    face: u32,
    planar: bool,
    tris: Vec<[Corner; 3]>,
}

/// A polygon and the triangles it was made from (used again if welding spoils it).
struct Poly {
    face: u32,
    corners: Vec<Corner>,
    tris: Vec<[Corner; 3]>,
}

/// Vertices welded by exact position (so the shared edge nodes of neighbouring faces meet).
#[derive(Default)]
struct Welder {
    positions: Vec<Vec3>,
    index: HashMap<[u64; 3], u32>,
}

impl Welder {
    fn at(&mut self, p: Vec3) -> Option<u32> {
        if !p.is_finite() {
            return None;
        }
        let key = bits(p);
        if let Some(&i) = self.index.get(&key) {
            return Some(i);
        }
        let i = u32::try_from(self.positions.len()).ok()?;
        self.positions.push(p);
        self.index.insert(key, i);
        Some(i)
    }

    fn find(&self, p: Vec3) -> Option<u32> {
        p.is_finite().then(|| self.index.get(&bits(p)).copied()).flatten()
    }
}

/// The bits of a position, with -0.0 read as 0.0.
fn bits(p: Vec3) -> [u64; 3] {
    [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()]
}

impl PolyMesh {
    /// The polygon mesh of a body's display plan. `evaluated` is the plan evaluated with its
    /// edges flagged (the body's display mesh of the plan); when it does not match the plan,
    /// the plan is evaluated again here and no edge is flagged beyond [`edge_flag::BREP`].
    pub fn from_plan(plan: &TessPlan, evaluated: &Mesh, o: &PolyOptions) -> Result<PolyMesh, PolyMeshError> {
        let count = plan.vertex_count();
        if count > MAX_POLY_VERTICES {
            return Err(PolyMeshError::TooLarge(count));
        }
        let matches = evaluated.positions.len() == count && evaluated.normals.len() == count;
        let own;
        let m = if matches {
            evaluated
        } else {
            own = plan.evaluate();
            &own
        };
        let mut w = Welder::default();
        let mut faces = Vec::with_capacity(plan.faces.len());
        let mut base = 0usize;
        for f in &plan.faces {
            let n = f.uv.len();
            let planar = matches!(f.surface.and_then(|s| plan.surfaces.get(s as usize)), Some(EvalSurface::Plane { .. }));
            let mut corner = |k: u32| -> Option<Corner> {
                let i = base.checked_add(k as usize).filter(|_| (k as usize) < n)?;
                let v = w.at(*m.positions.get(i)?)?;
                Some(Corner { v, n: m.normals.get(i).copied().unwrap_or(Vec3::ZERO) })
            };
            let tris = f.triangles.iter().filter_map(|t| Some([corner(t[0])?, corner(t[1])?, corner(t[2])?])).collect();
            faces.push(FaceTris { face: f.face, planar, tris });
            base = base.saturating_add(n);
        }
        let flags = (evaluated.seams.len() == plan.edges.len()
            && evaluated.smooth_edges.len() == plan.edges.len()
            && evaluated.sharp_edges.len() == plan.edges.len()
            && matches)
            .then_some(evaluated);
        Ok(build(w, faces, &plan.edges, flags, o))
    }

    /// The polygon mesh of a triangle mesh with a B-rep face per triangle (a body that has no
    /// plan). A face is planar when all its normals agree.
    pub fn from_mesh(m: &Mesh, o: &PolyOptions) -> Result<PolyMesh, PolyMeshError> {
        if m.positions.len() > MAX_POLY_VERTICES {
            return Err(PolyMeshError::TooLarge(m.positions.len()));
        }
        let mut w = Welder::default();
        let mut faces: Vec<FaceTris> = Vec::new();
        let mut slot: HashMap<u32, usize> = HashMap::new();
        for (k, t) in m.triangles.iter().enumerate() {
            let face = m.tri_face.get(k).copied().unwrap_or(0);
            let mut corner = |i: u32| -> Option<Corner> {
                let v = w.at(*m.positions.get(i as usize)?)?;
                Some(Corner { v, n: m.normals.get(i as usize).copied().unwrap_or(Vec3::ZERO) })
            };
            let [Some(a), Some(b), Some(c)] = [corner(t[0]), corner(t[1]), corner(t[2])] else { continue };
            let tri = [a, b, c];
            let at = *slot.entry(face).or_insert_with(|| {
                faces.push(FaceTris { face, planar: true, tris: Vec::new() });
                faces.len() - 1
            });
            if let Some(f) = faces.get_mut(at) {
                f.tris.push(tri);
            }
        }
        for f in &mut faces {
            let first = f.tris.first().and_then(|t| t[0].n.normalized());
            f.planar = first.is_some_and(|n0| f.tris.iter().flatten().all(|c| c.n.normalized().is_some_and(|n| n.dot(n0) > 1.0 - 1e-9)));
        }
        let flags = (m.seams.len() == m.edges.len() && m.smooth_edges.len() == m.edges.len() && m.sharp_edges.len() == m.edges.len()).then_some(m);
        Ok(build(w, faces, &m.edges, flags, o))
    }

    /// Signed volume enclosed (mm³), each polygon fanned from its first corner: exact for
    /// planar polygons, convex or not.
    pub fn volume(&self) -> f64 {
        let mut v = 0.0;
        let mut start = 0usize;
        for &size in &self.poly_sizes {
            let corners = self.loops.get(start..start + size as usize).unwrap_or(&[]);
            let p = |i: usize| corners.get(i).and_then(|&k| self.positions.get(k as usize)).copied().unwrap_or(Vec3::ZERO);
            for i in 1..corners.len().saturating_sub(1) {
                v += p(0).dot(p(i).cross(p(i + 1))) / 6.0;
            }
            start += size as usize;
        }
        v
    }

    /// Distinct edges of the polygons.
    pub fn edge_count(&self) -> usize {
        edge_uses(&self.poly_sizes, &self.loops).len()
    }
}

/// How many polygons use each edge (smaller vertex first).
fn edge_uses(sizes: &[u32], loops: &[u32]) -> HashMap<(u32, u32), u32> {
    let mut uses = HashMap::new();
    let mut start = 0usize;
    for &size in sizes {
        let corners = loops.get(start..start + size as usize).unwrap_or(&[]);
        for (i, &a) in corners.iter().enumerate() {
            let b = corners.get((i + 1) % corners.len()).copied().unwrap_or(a);
            *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
        start += size as usize;
    }
    uses
}

fn build(w: Welder, faces: Vec<FaceTris>, brep_edges: &[Vec<Vec3>], flags: Option<&Mesh>, o: &PolyOptions) -> PolyMesh {
    let mut polys = Vec::new();
    for f in faces {
        // A triangle whose corners weld together has no area and joins nothing.
        let tris: Vec<[Corner; 3]> = f.tris.into_iter().filter(|t| t[0].v != t[1].v && t[1].v != t[2].v && t[2].v != t[0].v).collect();
        if tris.is_empty() {
            continue;
        }
        if o.topology == PolyTopology::NGons && f.planar {
            match boundary_loops(&tris) {
                Some(mut loops) if loops.len() == 1 => {
                    let corners = loops.pop().unwrap_or_default();
                    polys.push(Poly { face: f.face, corners, tris });
                    continue;
                }
                Some(loops) => {
                    if let Some(parts) = holed_ngons(f.face, &tris, &loops, &w.positions) {
                        polys.extend(parts);
                        continue;
                    }
                }
                None => {}
            }
        }
        if o.topology == PolyTopology::Triangles {
            polys.extend(tris.into_iter().map(|t| Poly { face: f.face, corners: t.to_vec(), tris: vec![t] }));
        } else {
            polys.extend(quads(f.face, tris, &w.positions, o));
        }
    }
    let remap = weld_open(&w.positions, &polys, o.weld_eps);
    let root = |v: u32| remap.get(v as usize).copied().unwrap_or(v);

    // Welding may join two corners of one polygon: next to each other they become one; apart,
    // the polygon goes back to its triangles.
    let mut out_polys: Vec<(u32, Vec<Corner>)> = Vec::with_capacity(polys.len());
    for p in polys {
        let mut corners: Vec<Corner> = p.corners.iter().map(|c| Corner { v: root(c.v), n: c.n }).collect();
        corners.dedup_by(|a, b| a.v == b.v);
        while corners.len() > 1 && corners.first().map(|c| c.v) == corners.last().map(|c| c.v) {
            corners.pop();
        }
        let mut seen: Vec<u32> = corners.iter().map(|c| c.v).collect();
        seen.sort_unstable();
        let repeats = seen.windows(2).any(|s| s[0] == s[1]);
        if corners.len() >= 3 && !repeats {
            out_polys.push((p.face, corners));
        } else {
            for t in p.tris {
                let t = t.map(|c| Corner { v: root(c.v), n: c.n });
                if t[0].v != t[1].v && t[1].v != t[2].v && t[2].v != t[0].v {
                    out_polys.push((p.face, t.to_vec()));
                }
            }
        }
    }

    // Points along straight edges between n-gons are not needed: a box is eight corners, not
    // its edges' sample points. B-rep vertices stay, so every B-rep edge keeps its ends.
    let ends: HashSet<u32> = brep_edges.iter().flat_map(|e| [e.first(), e.last()]).flatten().filter_map(|p| w.find(*p).map(root)).collect();
    dissolve_collinear(&mut out_polys, &w.positions, &ends);

    // Only the vertices polygons use, in the order they are first used.
    let mut new_index = vec![u32::MAX; w.positions.len()];
    let mut out = PolyMesh::default();
    for (face, corners) in &out_polys {
        for c in corners {
            let Some(slot) = new_index.get_mut(c.v as usize) else { continue };
            if *slot == u32::MAX {
                *slot = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
                out.positions.push(w.positions.get(c.v as usize).copied().unwrap_or(Vec3::ZERO));
            }
        }
        let fallback = polygon_normal(corners, &w.positions);
        out.poly_sizes.push(u32::try_from(corners.len()).unwrap_or(u32::MAX));
        out.poly_face.push(*face);
        for c in corners {
            out.loops.push(new_index.get(c.v as usize).copied().unwrap_or(0));
            out.loop_normals.push(c.n.normalized().unwrap_or(fallback));
        }
        match corners.len() {
            3 => out.stats.triangles += 1,
            4 => out.stats.quads += 1,
            _ => out.stats.ngons += 1,
        }
    }
    out.stats.open_edges = edge_uses(&out.poly_sizes, &out.loops).values().filter(|&&n| n == 1).count();

    // The normals on either side of every polygon edge, for telling creases from tangent joins.
    let mut sides: HashMap<[u32; 2], Vec<(Vec3, Vec3)>> = HashMap::new();
    let mut start = 0usize;
    for &size in &out.poly_sizes {
        let end = start + size as usize;
        for i in start..end {
            let j = if i + 1 == end { start } else { i + 1 };
            let (Some(&a), Some(&b)) = (out.loops.get(i), out.loops.get(j)) else { continue };
            let (Some(&na), Some(&nb)) = (out.loop_normals.get(i), out.loop_normals.get(j)) else { continue };
            let (key, normals) = if a < b { ([a, b], (na, nb)) } else { ([b, a], (nb, na)) };
            sides.entry(key).or_default().push(normals);
        }
        start = end;
    }
    let cos_tangent = if o.tangent_angle.is_finite() { o.tangent_angle.clamp(0.0, std::f64::consts::FRAC_PI_2).cos() } else { 1.0 };
    let tangent = |key: &[u32; 2]| match sides.get(key).map(Vec::as_slice) {
        Some([(a1, b1), (a2, b2)]) => a1.dot(*a2) >= cos_tangent && b1.dot(*b2) >= cos_tangent,
        _ => false,
    };

    // The segments of every B-rep edge, through the shared edge nodes that are left.
    let mut segment: HashMap<[u32; 2], usize> = HashMap::new();
    for (e, points) in brep_edges.iter().enumerate() {
        let known = flags.map(|m| {
            (
                m.seams.get(e).copied().unwrap_or(false),
                m.smooth_edges.get(e).copied().unwrap_or(false),
                m.sharp_edges.get(e).copied().unwrap_or(false),
            )
        });
        let vertex = |p: &Vec3| w.find(*p).map(root).and_then(|v| new_index.get(v as usize).copied()).filter(|&v| v != u32::MAX);
        let mut chain: Vec<u32> = points.iter().filter_map(vertex).collect();
        chain.dedup();
        for pair in chain.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let key = [a.min(b), a.max(b)];
            let flag = edge_flag::BREP
                | match known {
                    Some((true, _, _)) => edge_flag::SURFACE_SEAM,
                    Some((_, _, true)) => edge_flag::SHARP,
                    Some((_, true, _)) => edge_flag::TANGENT,
                    _ if tangent(&key) => edge_flag::TANGENT,
                    _ => edge_flag::SHARP,
                };
            if let Some(&i) = segment.get(&key) {
                // A segment two edges share keeps the stronger mark.
                if let Some(f) = out.edge_flags.get_mut(i)
                    && flag & edge_flag::SHARP != 0
                {
                    *f = (*f & !edge_flag::TANGENT) | edge_flag::SHARP;
                }
                continue;
            }
            segment.insert(key, out.edges.len());
            out.edges.push(key);
            out.edge_flags.push(flag);
            out.edge_brep.push(u32::try_from(e).unwrap_or(u32::MAX));
        }
    }
    out
}

/// Removes the vertices that only continue a straight line: those with two neighbours, lying
/// between them, in polygons that keep at least three corners without them (`keep` stays).
fn dissolve_collinear(polys: &mut [(u32, Vec<Corner>)], positions: &[Vec3], keep: &HashSet<u32>) {
    let mut neighbours: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut member: HashMap<u32, Vec<usize>> = HashMap::new();
    for (k, (_, corners)) in polys.iter().enumerate() {
        for (i, c) in corners.iter().enumerate() {
            let next = corners.get((i + 1) % corners.len()).map_or(c.v, |n| n.v);
            for (a, b) in [(c.v, next), (next, c.v)] {
                let list = neighbours.entry(a).or_default();
                if a != b && !list.contains(&b) {
                    list.push(b);
                }
            }
            member.entry(c.v).or_default().push(k);
        }
    }
    let mut sizes: Vec<usize> = polys.iter().map(|(_, c)| c.len()).collect();
    let mut candidates: Vec<u32> = neighbours.iter().filter(|(v, n)| n.len() == 2 && !keep.contains(v)).map(|(v, _)| *v).collect();
    candidates.sort_unstable();
    let mut removed: HashSet<u32> = HashSet::new();
    let at = |v: u32| positions.get(v as usize).copied();
    for v in candidates {
        let Some(&[a, b]) = neighbours.get(&v).map(Vec::as_slice) else { continue };
        let (Some(pv), Some(pa), Some(pb)) = (at(v), at(a), at(b)) else { continue };
        let ab = pb - pa;
        let l2 = ab.len2();
        let t = if l2 > 0.0 { (pv - pa).dot(ab) / l2 } else { -1.0 };
        let straight = t > 0.0 && t < 1.0 && pv.dist(pa + ab * t) <= 1e-7 * l2.sqrt();
        let polys_of = member.get(&v).map(Vec::as_slice).unwrap_or(&[]);
        let roomy = polys_of.iter().all(|&k| sizes.get(k).is_some_and(|&n| n > 3));
        let joined = neighbours.get(&a).is_some_and(|n| n.contains(&b));
        if !straight || !roomy || joined {
            continue;
        }
        // The polygons around `v` hold `a` and `b` on either side of it already.
        for &k in polys_of {
            if let Some(n) = sizes.get_mut(k) {
                *n -= 1;
            }
        }
        for (end, other) in [(a, b), (b, a)] {
            if let Some(list) = neighbours.get_mut(&end) {
                for x in list.iter_mut().filter(|x| **x == v) {
                    *x = other;
                }
            }
        }
        member.remove(&v);
        removed.insert(v);
    }
    if removed.is_empty() {
        return;
    }
    for (_, corners) in polys.iter_mut() {
        corners.retain(|c| !removed.contains(&c.v));
    }
}

/// The boundary loops of a face's triangles, each wound as the triangles are; `None` when the
/// boundary is not a set of simple loops (a pinch, where two loops share a point).
fn boundary_loops(tris: &[[Corner; 3]]) -> Option<Vec<Vec<Corner>>> {
    let mut directed: HashSet<(u32, u32)> = HashSet::new();
    for t in tris {
        for i in 0..3 {
            directed.insert((t[i].v, t[(i + 1) % 3].v));
        }
    }
    let mut next: HashMap<u32, (u32, Vec3)> = HashMap::new();
    for t in tris {
        for i in 0..3 {
            let (a, b) = (t[i].v, t[(i + 1) % 3].v);
            if directed.contains(&(b, a)) {
                continue;
            }
            if next.insert(a, (b, t[i].n)).is_some() {
                return None;
            }
        }
    }
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut seen: HashSet<u32> = HashSet::with_capacity(next.len());
    let mut loops = Vec::new();
    for start in starts {
        if seen.contains(&start) {
            continue;
        }
        let mut corners = Vec::new();
        let mut at = start;
        loop {
            let &(to, n) = next.get(&at)?;
            if !seen.insert(at) {
                return None;
            }
            corners.push(Corner { v: at, n });
            at = to;
            if at == start {
                break;
            }
        }
        if corners.len() < 3 {
            return None;
        }
        loops.push(corners);
    }
    (!loops.is_empty()).then_some(loops)
}

/// A planar face with holes as convex polygons through its boundary points; `None` when its
/// loops are not one outer loop around holes, or cannot be cut.
fn holed_ngons(face: u32, tris: &[[Corner; 3]], loops: &[Vec<Corner>], positions: &[Vec3]) -> Option<Vec<Poly>> {
    let pos = |c: &Corner| positions.get(c.v as usize).copied().unwrap_or(Vec3::ZERO);
    // The face's normal as its triangles wind, and a frame in its plane.
    let n = tris.iter().fold(Vec3::ZERO, |s, t| s + (pos(&t[1]) - pos(&t[0])).cross(pos(&t[2]) - pos(&t[0]))).normalized()?;
    let (du, dv) = (n.any_perp(), n.cross(n.any_perp()));
    let origin = loops.first().and_then(|l| l.first()).map(pos)?;
    let flat = |l: &Vec<Corner>| -> Vec<Vec2> { l.iter().map(|c| pos(c) - origin).map(|d| Vec2::new(d.dot(du), d.dot(dv))).collect() };
    let signed = |l: &[Vec2]| (0..l.len()).filter_map(|k| Some(l.get(k)?.cross(*l.get((k + 1) % l.len())?))).sum::<f64>();
    // The one loop wound counter-clockwise is the outer one; the rest are holes.
    let mut flats: Vec<(Vec<Vec2>, &Vec<Corner>)> = loops.iter().map(|l| (flat(l), l)).collect();
    if flats.iter().filter(|(f, _)| signed(f) > 0.0).count() != 1 {
        return None;
    }
    flats.sort_by_key(|(f, _)| !(signed(f) > 0.0));
    let flat_loops: Vec<Vec<Vec2>> = flats.iter().map(|(f, _)| f.clone()).collect();
    let parts = convex_parts(&flat_loops)?;
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let corners: Vec<Corner> = part.iter().map(|&(l, i)| flats.get(l).and_then(|(_, c)| c.get(i)).copied()).collect::<Option<_>>()?;
        let (first, rest) = corners.split_first()?;
        let fan = rest.windows(2).map(|w| [*first, w[0], w[1]]).collect();
        out.push(Poly { face, corners, tris: fan });
    }
    Some(out)
}

/// A face's triangles, pairs of them joined into quads where the pair makes a good one.
fn quads(face: u32, tris: Vec<[Corner; 3]>, positions: &[Vec3], o: &PolyOptions) -> Vec<Poly> {
    let pos = |c: &Corner| positions.get(c.v as usize).copied().unwrap_or(Vec3::ZERO);
    let mut by_edge: HashMap<(u32, u32), (usize, usize)> = HashMap::new();
    let mut shared: HashMap<(u32, u32), u8> = HashMap::new();
    for (k, t) in tris.iter().enumerate() {
        for i in 0..3 {
            let (a, b) = (t[i].v, t[(i + 1) % 3].v);
            by_edge.insert((a, b), (k, i));
            *shared.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    // Every inner edge between two triangles that wind the same way is a candidate quad.
    let mut candidates: Vec<(f64, u32, u32, usize, usize, [Corner; 4])> = Vec::new();
    for (k, t) in tris.iter().enumerate() {
        for i in 0..3 {
            let (p, q, r1) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
            if p.v > q.v || shared.get(&(p.v.min(q.v), p.v.max(q.v))).copied() != Some(2) {
                continue;
            }
            let Some(&(k2, j)) = by_edge.get(&(q.v, p.v)) else { continue };
            let Some(t2) = tris.get(k2) else { continue };
            if k2 == k {
                continue;
            }
            let r2 = t2[(j + 2) % 3];
            if r2.v == r1.v {
                continue;
            }
            let quad = [p, r2, q, r1];
            let pts = quad.map(|c| pos(&c));
            if let Some(score) = quad_score(pts, o) {
                candidates.push((score, p.v, q.v, k, k2, quad));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut used = vec![false; tris.len()];
    let mut out = Vec::with_capacity(tris.len());
    for (_, _, _, k, k2, quad) in candidates {
        if used.get(k).copied().unwrap_or(true) || used.get(k2).copied().unwrap_or(true) {
            continue;
        }
        let (Some(a), Some(b)) = (tris.get(k), tris.get(k2)) else { continue };
        out.push(Poly { face, corners: quad.to_vec(), tris: vec![*a, *b] });
        if let Some(u) = used.get_mut(k) {
            *u = true;
        }
        if let Some(u) = used.get_mut(k2) {
            *u = true;
        }
    }
    for (k, t) in tris.into_iter().enumerate() {
        if !used.get(k).copied().unwrap_or(false) {
            out.push(Poly { face, corners: t.to_vec(), tris: vec![t] });
        }
    }
    out
}

/// How far a quad's corners are from right angles in total (radians), or `None` when it is
/// folded too far, not strictly convex, or a corner is too far from square.
fn quad_score(p: [Vec3; 4], o: &PolyOptions) -> Option<f64> {
    // p = [p, r2, q, r1]: the triangles were (p, q, r1) and (q, p, r2).
    let n1 = (p[2] - p[0]).cross(p[3] - p[0]).normalized()?;
    let n2 = (p[0] - p[2]).cross(p[1] - p[2]).normalized()?;
    if n1.dot(n2).clamp(-1.0, 1.0).acos() > o.max_fold {
        return None;
    }
    let n = (n1 + n2).normalized()?;
    let mut score = 0.0;
    for i in 0..4 {
        let (prev, cur, next) = (p[(i + 3) % 4], p[i], p[(i + 1) % 4]);
        let (a, b) = (cur - prev, next - cur);
        let (la, lb) = (a.len(), b.len());
        if la <= 0.0 || lb <= 0.0 || a.cross(b).dot(n) <= la * lb * 1e-9 {
            return None;
        }
        let angle = (prev - cur).dot(next - cur) / (la * lb);
        let dev = (angle.clamp(-1.0, 1.0).acos() - std::f64::consts::FRAC_PI_2).abs();
        if dev.is_nan() || dev > o.max_corner_dev {
            return None;
        }
        score += dev;
    }
    Some(score)
}

/// For each vertex, the vertex it is welded to: vertices on open edges closer than the weld
/// distance are joined (the lowest index stands for them all).
fn weld_open(positions: &[Vec3], polys: &[Poly], eps: f64) -> Vec<u32> {
    let mut parent: Vec<u32> = (0..u32::try_from(positions.len()).unwrap_or(u32::MAX)).collect();
    let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
    for p in polys {
        for (i, a) in p.corners.iter().enumerate() {
            let b = p.corners.get((i + 1) % p.corners.len()).map_or(a.v, |c| c.v);
            *uses.entry((a.v.min(b), a.v.max(b))).or_insert(0) += 1;
        }
    }
    let mut open: Vec<u32> = uses.iter().filter(|(_, n)| **n == 1).flat_map(|(e, _)| [e.0, e.1]).collect();
    if open.is_empty() {
        return parent;
    }
    open.sort_unstable();
    open.dedup();
    let eps = if eps.is_finite() && eps > 0.0 {
        eps
    } else {
        let big = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let (lo, hi) = positions.iter().fold((big, -big), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let diag = (hi - lo).len();
        if diag.is_finite() { (diag * 1e-9).max(1e-9) } else { 1e-9 }
    };
    let cell = |p: Vec3| [(p.x / eps).floor() as i64, (p.y / eps).floor() as i64, (p.z / eps).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for &v in &open {
        if let Some(p) = positions.get(v as usize) {
            grid.entry(cell(*p)).or_default().push(v);
        }
    }
    fn find(parent: &mut [u32], v: u32) -> u32 {
        let mut r = v;
        while let Some(&up) = parent.get(r as usize) {
            if up == r {
                break;
            }
            r = up;
        }
        let mut at = v;
        while let Some(&up) = parent.get(at as usize) {
            if up == r {
                break;
            }
            if let Some(slot) = parent.get_mut(at as usize) {
                *slot = r;
            }
            at = up;
        }
        r
    }
    for &v in &open {
        let Some(&p) = positions.get(v as usize) else { continue };
        let c = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(near) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else { continue };
                    for &u in near {
                        if u <= v || positions.get(u as usize).is_none_or(|q| q.dist(p) > eps) {
                            continue;
                        }
                        let (a, b) = (find(&mut parent, v), find(&mut parent, u));
                        if a != b {
                            let (lo, hi) = (a.min(b), a.max(b));
                            if let Some(slot) = parent.get_mut(hi as usize) {
                                *slot = lo;
                            }
                        }
                    }
                }
            }
        }
    }
    (0..parent.len()).map(|v| find(&mut parent, u32::try_from(v).unwrap_or(u32::MAX))).collect()
}

/// A polygon's normal from its corners (the sum of its edges' cross products), for corners
/// whose normal is missing.
fn polygon_normal(corners: &[Corner], positions: &[Vec3]) -> Vec3 {
    let p = |c: &Corner| positions.get(c.v as usize).copied().unwrap_or(Vec3::ZERO);
    let mut n = Vec3::ZERO;
    for (i, c) in corners.iter().enumerate() {
        n += p(c).cross(corners.get((i + 1) % corners.len()).map_or(p(c), p));
    }
    n.normalized().unwrap_or(Vec3::Z)
}

#[cfg(test)]
#[path = "poly_mesh_tests.rs"]
mod tests;
