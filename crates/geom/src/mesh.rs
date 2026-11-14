use crate::prelude::*;
use serde::{Deserialize, Serialize};

use solidcore_record::Vec3;

#[cfg(test)]
thread_local! {
    /// The edges [`Mesh::tangent_chain`] looked at to extend a chain, on this thread: the work a
    /// scan of every edge for every edge made quadratic (#3745), counted for the test that checks
    /// it stays linear, whatever else the machine is doing (#9901).
    pub(crate) static CHAIN_CANDIDATES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The angle, in radians (2°), a tangent chain continues through: the most two edges at a shared
/// vertex, or the two faces along an edge, may turn from tangent and still be one chain. The one
/// rule for fillet, chamfer, shell and the edge selection that picks a chain, so a near-smooth
/// outline (lines and arcs meeting a degree or so off tangent) chains whole. Drawing and hiding
/// edges is a different question with its own, tighter angle (`smooth_edges::ANGLE`, half a degree).
pub const TANGENT_CHAIN: f64 = 2.0 * std::f64::consts::PI / 180.0;

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb3 {
    fn default() -> Self {
        Aabb3::EMPTY
    }
}

impl Aabb3 {
    pub const EMPTY: Aabb3 = Aabb3 {
        min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }
    pub fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    pub fn union(&self, o: &Aabb3) -> Aabb3 {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Aabb3 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn center(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { (self.min + self.max) * 0.5 }
    }
    pub fn size(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { self.max - self.min }
    }
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }
    /// The box grown by `by` on every side; an empty box stays empty.
    pub fn inflated(&self, by: f64) -> Aabb3 {
        if self.is_empty() {
            return *self;
        }
        let d = Vec3::new(by, by, by);
        Aabb3 { min: self.min - d, max: self.max + d }
    }
    /// Whether `p` lies in the box, its sides included (never in an empty box, never for NaN).
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.z >= self.min.z && p.x <= self.max.x && p.y <= self.max.y && p.z <= self.max.z
    }
    /// The box both hold: empty when they do not meet.
    pub fn intersection(&self, o: &Aabb3) -> Aabb3 {
        Aabb3 { min: self.min.max(o.min), max: self.max.min(o.max) }
    }
    /// Whether the endless line through `o` along `d` meets the box, its sides included (never an
    /// empty box, never a zero direction). The kernel clips the line ([`Extent3d::intersect_line`]).
    ///
    /// [`Extent3d::intersect_line`]: solidcore::geom::extent3d::Extent3d::intersect_line
    pub fn meets_line(&self, o: Vec3, d: Vec3) -> bool {
        use solidcore::geom::extent3d::{Extent3d, LineHit};
        if self.is_empty() {
            return false;
        }
        let k = |v: Vec3| solidcore::Vec3::new(v.x, v.y, v.z);
        matches!(Extent3d::from_points(k(self.min), k(self.max)).intersect_line(k(o), k(d)), Ok(LineHit::Touch { .. } | LineHit::Through { .. }))
    }
    /// Whether the two boxes share a point (never when either is empty).
    pub fn meets(&self, o: &Aabb3) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
            && self.min.z <= o.max.z
            && o.min.z <= self.max.z
    }
}

/// The box around the finite points (non-finite ones are skipped, as [`Aabb3::add`] does).
impl FromIterator<Vec3> for Aabb3 {
    fn from_iter<I: IntoIterator<Item = Vec3>>(points: I) -> Aabb3 {
        let mut b = Aabb3::EMPTY;
        b.extend(points);
        b
    }
}

impl Extend<Vec3> for Aabb3 {
    fn extend<I: IntoIterator<Item = Vec3>>(&mut self, points: I) {
        for p in points {
            self.add(p);
        }
    }
}

/// The display mesh is solidcore-tess's: one type from the kernel's tessellation to the viewport,
/// with nothing converted on the way. Its geometry here is [`MeshGeom`].
pub use solidcore_tess::Mesh;

/// A triangle with its bounding box, as [`find_close_pair`] takes them.
pub type BoxedTriangle = ([Vec3; 3], Aabb3);

/// Most grid cells one triangle is filed under; a larger one is tested against every triangle.
const MAX_CELLS_PER_TRIANGLE: usize = 64;

/// Calls `test` on pairs of triangles, one from `a` and one from `b`, whose boxes come within
/// `gap` of each other (each such pair once), until it returns true, and says whether it did.
/// The cost follows the number of close pairs, not `a.len() * b.len()`: `b` is filed in a sparse
/// grid of cells about its typical triangle's size, and each triangle of `a` meets only those in
/// its cells. The work counts against `budget`, one unit each: a grid cell a triangle of `b` is
/// filed under, a cell a triangle of `a` visits, and a call of `test`. `None` when the budget runs out.
pub fn find_close_pair(
    a: &[BoxedTriangle],
    b: &[BoxedTriangle],
    gap: f64,
    budget: &mut usize,
    test: impl FnMut(&BoxedTriangle, &BoxedTriangle) -> bool,
) -> Option<bool> {
    let work = std::cell::Cell::new(*budget);
    let mut test = test;
    let found = find_close_pair_within(a, b, gap, &work, |(_, t), (_, u)| test(t, u));
    *budget = work.get();
    found
}

fn find_close_pair_within(
    a: &[BoxedTriangle],
    b: &[BoxedTriangle],
    gap: f64,
    work: &std::cell::Cell<usize>,
    mut test: impl FnMut((usize, &BoxedTriangle), (usize, &BoxedTriangle)) -> bool,
) -> Option<bool> {
    let spend = |n: usize| -> Option<()> {
        work.set(work.get().checked_sub(n)?);
        Some(())
    };
    let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
    let close = |x: &Aabb3, y: &Aabb3| {
        x.min.x <= y.max.x + gap
            && y.min.x <= x.max.x + gap
            && x.min.y <= y.max.y + gap
            && y.min.y <= x.max.y + gap
            && x.min.z <= y.max.z + gap
            && y.min.z <= x.max.z + gap
    };
    let mut call = |t: (usize, &BoxedTriangle), u: (usize, &BoxedTriangle)| -> Option<bool> {
        spend(1)?;
        Some(test(t, u))
    };
    let (Some(first), true) = (b.first(), !a.is_empty()) else { return Some(false) };
    // Cell size: the median extent of `b`'s boxes (so most of them span a few cells), at least `gap`.
    let mut extents: Vec<f64> = b
        .iter()
        .map(|(_, bx)| {
            let d = bx.size();
            d.x.max(d.y).max(d.z)
        })
        .filter(|e| e.is_finite())
        .collect();
    let mid = extents.len() / 2;
    let median = if extents.is_empty() { 0.0 } else { *extents.select_nth_unstable_by(mid, f64::total_cmp).1 };
    let origin = first.1.min;
    let cell = median.max(gap).max(1e-9);
    if !(origin.is_finite() && cell.is_finite()) {
        return Some(false);
    }
    let index = |v: f64, o: f64| ((v - o) / cell).floor() as i64;
    let span = |bx: &Aabb3, pad: f64| -> Option<[(i64, i64); 3]> {
        let r = [
            (index(bx.min.x - pad, origin.x), index(bx.max.x + pad, origin.x)),
            (index(bx.min.y - pad, origin.y), index(bx.max.y + pad, origin.y)),
            (index(bx.min.z - pad, origin.z), index(bx.max.z + pad, origin.z)),
        ];
        let count = r.iter().try_fold(1usize, |n, (lo, hi)| n.checked_mul(usize::try_from(hi.checked_sub(*lo)?.checked_add(1)?).ok()?))?;
        (count <= MAX_CELLS_PER_TRIANGLE).then_some(r)
    };
    let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>> = std::collections::HashMap::new();
    let mut big = Vec::new();
    for (j, (_, bx)) in b.iter().enumerate() {
        match span(bx, gap) {
            Some([x, y, z]) => {
                for ix in x.0..=x.1 {
                    for iy in y.0..=y.1 {
                        for iz in z.0..=z.1 {
                            spend(1)?;
                            cells.entry((ix, iy, iz)).or_default().push(j as u32);
                        }
                    }
                }
            }
            None => big.push(j),
        }
    }
    // Per triangle of `b`: the last triangle of `a` tested against it, plus one.
    let mut seen = vec![0usize; b.len()];
    for (i, t) in a.iter().enumerate() {
        for &j in &big {
            if let Some(u) = b.get(j).filter(|u| close(&t.1, &u.1)) {
                seen[j] = i + 1;
                if call((i, t), (j, u))? {
                    return Some(true);
                }
            }
        }
        let Some([x, y, z]) = span(&t.1, 0.0) else {
            // A large triangle of `a`: against all of `b`.
            for (j, u) in b.iter().enumerate() {
                if seen[j] != i + 1 && close(&t.1, &u.1) && call((i, t), (j, u))? {
                    return Some(true);
                }
            }
            continue;
        };
        for ix in x.0..=x.1 {
            for iy in y.0..=y.1 {
                for iz in z.0..=z.1 {
                    spend(1)?;
                    for &j in cells.get(&(ix, iy, iz)).into_iter().flatten() {
                        let (Some(mark), Some(u)) = (seen.get_mut(j as usize), b.get(j as usize)) else { continue };
                        if *mark == i + 1 {
                            continue;
                        }
                        *mark = i + 1;
                        if close(&t.1, &u.1) && call((i, t), (j as usize, u))? {
                            return Some(true);
                        }
                    }
                }
            }
        }
    }
    Some(false)
}

/// Two triangles that pass through each other (see [`Mesh::crosses`]).
/// Where two triangles pass through each other by more than `tol` (see [`Mesh::crosses`]): the
/// middle of the stretch of the line their planes share that both span, or `None`.
fn triangles_cross(t: &[Vec3; 3], u: &[Vec3; 3], tol: f64) -> Option<Vec3> {
    // Signed distances of one triangle's corners from the other's plane, when they lie more
    // than `tol` to both sides of it.
    let sides = |tri: &[Vec3; 3], plane: &[Vec3; 3]| -> Option<(Vec3, [f64; 3])> {
        let n = (plane[1] - plane[0]).cross(plane[2] - plane[0]).normalized()?;
        let d = [n.dot(tri[0] - plane[0]), n.dot(tri[1] - plane[0]), n.dot(tri[2] - plane[0])];
        let (lo, hi) = d.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        (lo < -tol && hi > tol).then_some((n, d))
    };
    let (Some((nu, dt)), Some((nt, du))) = (sides(t, u), sides(u, t)) else { return None };
    let line = nt.cross(nu).normalized()?;
    // Where a triangle's edges pass through the other's plane, measured along the line the
    // two planes share.
    // With the points there, for the middle of the stretch both span.
    let reach = |tri: &[Vec3; 3], d: &[f64; 3]| -> ((f64, Vec3), (f64, Vec3)) {
        let (mut lo, mut hi) = ((f64::INFINITY, Vec3::ZERO), (f64::NEG_INFINITY, Vec3::ZERO));
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            let (p, q, dp, dq) = (tri[i], tri[j], d[i], d[j]);
            if (dp < 0.0) != (dq < 0.0) && dp != dq {
                let x = p + (q - p) * (dp / (dp - dq));
                let at = line.dot(x);
                if at < lo.0 {
                    lo = (at, x);
                }
                if at > hi.0 {
                    hi = (at, x);
                }
            }
        }
        (lo, hi)
    };
    let ((t0, t1), (u0, u1)) = (reach(t, &dt), reach(u, &du));
    let (from, to) = (if t0.0 > u0.0 { t0 } else { u0 }, if t1.0 < u1.0 { t1 } else { u1 });
    (to.0 - from.0 > tol).then(|| (from.1 + to.1) * 0.5)
}

/// Mass properties of a closed mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
}

/// Volume, centroid and inertia tensor of a closed mesh at unit density: the inertia is about
/// the centroid, in the mesh's axes (times a density, mass and inertia in that density's units).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub centroid: Vec3,
    pub inertia: [[f64; 3]; 3],
}

/// The geometry of a display mesh (solidcore-tess's [`Mesh`]): its measures, point and ray
/// queries, and the edge chains and face patches picking extends to. In scope with
/// `use solidcraft_geom::prelude::*`.
pub trait MeshGeom {
    /// Generate angle-weighted corner normals without changing positions or connectivity.
    /// Only opposite, two-face manifold edges join a smoothing fan. A group of zero
    /// stays flat; different groups, boundaries and creases never share a normal.
    /// Degenerate faces do not join or weight a fan; their corner metadata uses
    /// a unit fallback so mesh-body validation can discard repeated-index faces.
    /// `groups == None` is an explicitly requested crease policy for unannotated meshes.
    fn crease_corner_normals(&self, angle: f64, groups: Option<&[u32]>) -> Result<Vec<[Vec3; 3]>, String>;
    /// [`MassProperties`] of a closed mesh, by the divergence theorem over the tetrahedra its
    /// triangles make with the origin. `None` unless every edge (by vertex index: weld first) is
    /// shared by exactly two triangles that cross it in opposite directions (closed, consistently
    /// oriented), or for a mesh without volume or with non-finite values.
    fn mass_properties(&self) -> Option<MassProperties>;

    /// Pieces of one surface joined across hidden seams, including `face` itself.
    fn surface_patch(&self, face: usize) -> Vec<usize>;

    /// A complete circular rim may be split into several kernel edges. Keep its pieces
    /// together for picking, without joining other tangent curves or changing edge indices.
    fn circular_rim(&self, edge: usize) -> Vec<usize>;

    /// The edges reached from `e` through vertices where the next edge continues smoothly
    /// (angle between the end tangents below `max_angle`), including `e` itself.
    fn tangent_chain(&self, e: usize, max_angle: f64) -> Vec<usize>;

    /// Whether the two faces at edge `i` meet tangentially (within `max_angle` radians) there: the
    /// join [`MeshGeom::tangent_chain`] passes through. A crease the model protects is never one.
    fn tangent_join(&self, i: usize, max_angle: f64) -> bool;

    fn bounds(&self) -> Aabb3;

    /// Signed volume (divergence theorem), surface area and volume centroid.
    fn measure(&self) -> MeshMeasure;

    /// Append another mesh (face ids offset by `face_offset`).
    fn append(&mut self, o: &Mesh, face_offset: u32);

    /// Number of triangles a ray crosses (for inside/outside tests on closed meshes), each
    /// crossing counted once ([`ray_crosses`]).
    fn ray_crossings(&self, o: Vec3, dir: Vec3) -> usize;

    /// Point inside a closed mesh (majority vote of three skewed rays).
    fn contains(&self, p: Vec3) -> bool;

    /// Whether the surfaces of two meshes pass through each other by more than `tol`: some
    /// triangle of one has corners more than `tol` to both sides of a triangle of the other,
    /// and the reverse, and they share a stretch of the line where their planes meet longer
    /// than `tol`. Faces that only touch, lie on each other or stop on the other surface don't
    /// count, nor do two facetings of one curved surface that are within `tol` of it. Bodies
    /// can still overlap without crossing: one inside the other, or sharing a slab between
    /// faces that are parallel or in one plane.
    fn crosses(&self, o: &Mesh, tol: f64) -> bool;

    /// [`Mesh::crosses`], leaving out the crossings for which `ignored` holds, asked with the
    /// middle of the stretch the two triangles share and the faces of this mesh's and `o`'s
    /// triangle (`u32::MAX` where a mesh has no face for it): where both meshes stand for one
    /// surface of both bodies, and their facets cross by the facetings' error alone.
    fn crosses_except(&self, o: &Mesh, tol: f64, ignored: impl Fn(Vec3, u32, u32) -> bool) -> bool;

    /// Closest intersection of a ray with the mesh: (distance along `dir`, triangle index).
    ///
    /// Watertight (Woop, Benthin & Wald, "Watertight Ray/Triangle Intersection", JCGT 2(1),
    /// 2013): the vertices are moved into a frame where the ray runs along +z through the
    /// origin, and each edge's side test is computed from those per-vertex values alone, so the
    /// two triangles that share an edge get exactly opposite values there. A ray through a
    /// shared edge or vertex hits at least one of them; it never slips between them.
    fn raycast(&self, o: Vec3, dir: Vec3) -> Option<(f64, usize)>;

    /// An index for many `contains` queries on this mesh.
    fn inside_index(&self) -> InsideIndex<'_>;
}

impl MeshGeom for Mesh {
    /// Generate angle-weighted corner normals without changing positions or connectivity.
    /// Only opposite, two-face manifold edges join a smoothing fan. A group of zero
    /// stays flat; different groups, boundaries and creases never share a normal.
    /// Degenerate faces do not join or weight a fan; their corner metadata uses
    /// a unit fallback so mesh-body validation can discard repeated-index faces.
    /// `groups == None` is an explicitly requested crease policy for unannotated meshes.
    fn crease_corner_normals(&self, angle: f64, groups: Option<&[u32]>) -> Result<Vec<[Vec3; 3]>, String> {
        if !angle.is_finite() || !(0.0..=std::f64::consts::PI).contains(&angle) || groups.is_some_and(|g| g.len() != self.triangles.len()) {
            return Err("invalid mesh smoothing angle or group count".into());
        }
        let corners = self.triangles.len().checked_mul(3).ok_or("mesh smoothing size overflow")?;
        let mut parent = Vec::new();
        parent.try_reserve_exact(corners).map_err(|_| "could not reserve mesh smoothing fans")?;
        parent.extend(0..corners);
        let mut weld = std::collections::HashMap::new();
        weld.try_reserve(self.positions.len()).map_err(|_| "could not reserve mesh smoothing vertices")?;
        let mut canonical = Vec::new();
        canonical.try_reserve_exact(self.positions.len()).map_err(|_| "could not reserve mesh smoothing vertex indices")?;
        for p in &self.positions {
            if !p.is_finite() {
                return Err("non-finite mesh smoothing position".into());
            }
            let next = weld.len();
            canonical.push(*weld.entry([(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()]).or_insert(next));
        }
        let mut normals = Vec::new();
        normals.try_reserve_exact(self.triangles.len()).map_err(|_| "could not reserve mesh smoothing normals")?;
        // A third incident face makes an edge non-manifold and prevents a join.
        let mut edges = std::collections::HashMap::<(usize, usize), (usize, Option<usize>, usize)>::new();
        edges.try_reserve(corners).map_err(|_| "could not reserve mesh smoothing edges")?;
        for (i, t) in self.triangles.iter().enumerate() {
            let [a, b, c] = t.map(|v| self.positions.get(v as usize).copied());
            let (Some(a), Some(b), Some(c)) = (a, b, c) else { return Err("mesh smoothing index is missing".into()) };
            let n = (b - a)
                .cross(c - a)
                .normalized()
                .filter(|n| n.is_finite() && [b - a, c - b, a - c].iter().all(|edge| edge.normalized().is_some_and(|v| v.is_finite())));
            let Some(n) = n else {
                // An unusable face must not turn its neighbours' shared edge into
                // a non-manifold edge or contribute a fabricated fan direction.
                normals.push(Vec3::ZERO);
                continue;
            };
            normals.push(n);
            for k in 0..3 {
                let (a, b) = (canonical[t[k] as usize], canonical[t[(k + 1) % 3] as usize]);
                edges
                    .entry((a.min(b), a.max(b)))
                    .and_modify(|e| {
                        e.1 = Some(i * 3 + k);
                        e.2 += 1;
                    })
                    .or_insert((i * 3 + k, None, 1));
            }
        }
        fn root(parent: &mut [usize], mut k: usize) -> usize {
            while parent[k] != k {
                parent[k] = parent[parent[k]];
                k = parent[k];
            }
            k
        }
        let cosine = angle.cos();
        for (_, (first, second, count)) in edges {
            let Some(second) = second.filter(|_| count == 2) else { continue };
            let (a, b) = (first / 3, second / 3);
            if normals[a].dot(normals[b]) < cosine || groups.is_some_and(|g| g[a] == 0 || g[a] != g[b]) {
                continue;
            }
            let (ak, bk) = (first % 3, second % 3);
            // Consistent winding is required; do not smooth across an ambiguous join.
            if canonical[self.triangles[a][ak] as usize] != canonical[self.triangles[b][(bk + 1) % 3] as usize]
                || canonical[self.triangles[a][(ak + 1) % 3] as usize] != canonical[self.triangles[b][bk] as usize]
            {
                continue;
            }
            for (x, y) in [(first, b * 3 + (bk + 1) % 3), (a * 3 + (ak + 1) % 3, second)] {
                let (x, y) = (root(&mut parent, x), root(&mut parent, y));
                parent[y] = x;
            }
        }
        let mut sums = Vec::new();
        sums.try_reserve_exact(corners).map_err(|_| "could not reserve mesh smoothing sums")?;
        sums.resize(corners, Vec3::ZERO);
        for (i, t) in self.triangles.iter().enumerate() {
            if normals[i] == Vec3::ZERO {
                continue;
            }
            for k in 0..3 {
                let p = self.positions[t[k] as usize];
                let a = (self.positions[t[(k + 1) % 3] as usize] - p).normalized().ok_or("degenerate smoothing corner")?;
                let b = (self.positions[t[(k + 2) % 3] as usize] - p).normalized().ok_or("degenerate smoothing corner")?;
                let weight = a.dot(b).clamp(-1.0, 1.0).acos();
                let r = root(&mut parent, i * 3 + k);
                sums[r] += normals[i] * weight;
            }
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.triangles.len()).map_err(|_| "could not reserve mesh shading result")?;
        for (i, n) in normals.into_iter().enumerate() {
            if n == Vec3::ZERO {
                out.push([Vec3::Z; 3]);
                continue;
            }
            out.push(std::array::from_fn(|k| {
                let r = root(&mut parent, i * 3 + k);
                sums[r].normalized().filter(|v| v.dot(n) > 0.0).unwrap_or(n)
            }));
        }
        Ok(out)
    }

    fn mass_properties(&self) -> Option<MassProperties> {
        // Closed and consistently oriented: each edge is crossed once in each direction.
        let mut edges: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a, b)).or_insert(0) += 1;
            }
        }
        if edges.is_empty() || edges.iter().any(|(&(a, b), n)| *n != 1 || edges.get(&(b, a)) != Some(&1)) {
            return None;
        }
        let xyz = |v: Vec3| [v.x, v.y, v.z];
        let (mut v6, mut first, mut second) = (0.0, Vec3::ZERO, [[0.0f64; 3]; 3]);
        for t in &self.triangles {
            let [a, b, c] = self.tri(t)?;
            let det = a.dot(b.cross(c));
            v6 += det;
            first += (a + b + c) * det;
            // ∫ x xᵀ over the tetrahedron (0, a, b, c) = det/120 · (Σ p pᵀ + s sᵀ), s = a + b + c.
            let (p, s) = ([xyz(a), xyz(b), xyz(c)], xyz(a + b + c));
            for (i, row) in second.iter_mut().enumerate() {
                for (j, cell) in row.iter_mut().enumerate() {
                    *cell += det / 120.0 * (p.iter().map(|q| q[i] * q[j]).sum::<f64>() + s[i] * s[j]);
                }
            }
        }
        if !(v6.abs() / 6.0 > 1e-12) {
            return None;
        }
        // Inward-facing: every sum changes sign together.
        let sign = v6.signum();
        let volume = v6.abs() / 6.0;
        let centroid = first / (4.0 * v6);
        let c = xyz(centroid);
        let mut about = [[0.0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                about[i][j] = sign * second[i][j] - volume * c[i] * c[j];
            }
        }
        let trace = about[0][0] + about[1][1] + about[2][2];
        let mut inertia = [[0.0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                inertia[i][j] = if i == j { trace } else { 0.0 } - about[i][j];
            }
        }
        let finite = volume.is_finite() && c.iter().all(|x| x.is_finite()) && inertia.iter().flatten().all(|x| x.is_finite());
        finite.then_some(MassProperties { volume, centroid, inertia })
    }

    fn surface_patch(&self, face: usize) -> Vec<usize> {
        let mut patch = vec![face];
        let mut todo = vec![face];
        while let Some(f) = todo.pop() {
            for (ei, fs) in self.edge_faces.iter().enumerate() {
                if !self.seams.get(ei).copied().unwrap_or(false)
                    || self.sharp_edges.get(ei).copied().unwrap_or(false)
                    || !fs.iter().any(|x| *x as usize == f)
                {
                    continue;
                }
                for &next in fs {
                    let next = next as usize;
                    if !patch.contains(&next) {
                        patch.push(next);
                        todo.push(next);
                    }
                }
            }
        }
        patch.sort_unstable();
        patch
    }

    fn circular_rim(&self, edge: usize) -> Vec<usize> {
        let single = vec![edge];
        let Some(p) = self.edges.get(edge).filter(|p| p.len() >= 3) else { return single };
        let (Some(&a), Some(&b), Some(&c)) = (p.first(), p.get(p.len() / 2), p.last()) else { return single };
        let (u, v) = (b - a, c - a);
        let n = u.cross(v);
        if !(n.len2() > u.len2() * v.len2() * 1e-12) {
            return single;
        }
        let center = a + (v.cross(n) * u.len2() + n.cross(u) * v.len2()) / (2.0 * n.len2());
        let radius = center.dist(a);
        let normal = n.normalized().unwrap_or(Vec3::ZERO);
        let tol = (radius * 1e-6).max(1e-9);
        let mut rim = self.tangent_chain(edge, TANGENT_CHAIN);
        if rim.len() < 2 || !radius.is_finite() {
            return single;
        }
        // All samples must lie on this circle, and every end must join exactly one other
        // piece. Open arcs, ellipses, and branching tangent chains stay separate.
        for &i in &rim {
            let Some(points) = self.edges.get(i) else { return single };
            if points.iter().any(|q| !q.is_finite() || (q.dist(center) - radius).abs() > tol || (*q - center).dot(normal).abs() > tol) {
                return single;
            }
            for end in [points.first(), points.last()].into_iter().flatten() {
                let joins = rim
                    .iter()
                    .filter(|&&j| j != i)
                    .filter_map(|&j| self.edges.get(j))
                    .flat_map(|q| [q.first(), q.last()])
                    .flatten()
                    .filter(|q| q.dist(*end) < tol)
                    .count();
                if joins != 1 {
                    return single;
                }
            }
        }
        rim.sort_unstable();
        rim
    }
    /// Whether the two faces at edge `i` meet tangentially (within `max_angle` radians) there: the
    /// join [`MeshGeom::tangent_chain`] passes through. A crease the model protects is never one.
    fn tangent_join(&self, i: usize, max_angle: f64) -> bool {
        smooth_seam(self, i, max_angle.cos())
    }

    fn tangent_chain(&self, e: usize, max_angle: f64) -> Vec<usize> {
        // (end point, direction leaving the edge there). A polyline's end chord leans inward
        // on a curve by half the turn to the next chord; extrapolating the two end chords
        // recovers the curve's end tangent.
        let end_dir = |p0: Vec3, p1: Vec3, p2: Option<Vec3>| -> Option<Vec3> {
            let u1 = (p0 - p1).normalized()?;
            match p2.and_then(|p2| (p1 - p2).normalized()) {
                Some(u2) => (u1 * 1.5 - u2 * 0.5).normalized(),
                None => Some(u1),
            }
        };
        let ends = |i: usize| -> Option<[(Vec3, Vec3); 2]> {
            let p = self.edges.get(i)?;
            let n = p.len();
            let (a, a1, a2) = (*p.first()?, *p.get(1)?, p.get(2).copied().filter(|_| n > 2));
            let (b, b1) = (*p.last()?, *p.get(n.checked_sub(2)?)?);
            let b2 = n.checked_sub(3).and_then(|k| p.get(k)).copied();
            Some([(a, end_dir(a, a1, a2)?), (b, end_dir(b, b1, b2)?)])
        };
        let size = self.bounds().diagonal().max(1e-9);
        let tol = size * 1e-7;
        let cos = max_angle.cos();
        let mut chain = vec![e];
        if smooth_seam(self, e, cos) {
            return chain;
        }
        // The edges by the grid cell of each end, so each step looks only at the edges that end
        // near it (#3745: scanning every edge for every edge of a long helical chain held a hover
        // for seconds). Cells are twice the join tolerance: two ends within it share a cell or
        // touch neighbouring ones.
        let cell = tol * 2.0;
        let key = |p: Vec3| [(p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64];
        let mut at_end: std::collections::HashMap<[i64; 3], Vec<usize>> = std::collections::HashMap::new();
        for (i, p) in self.edges.iter().enumerate() {
            for q in [p.first(), p.last()].into_iter().flatten().filter(|q| q.is_finite()) {
                let slot = at_end.entry(key(*q)).or_default();
                if slot.last() != Some(&i) {
                    slot.push(i);
                }
            }
        }
        let mut in_chain: std::collections::HashSet<usize> = chain.iter().copied().collect();
        let mut todo = vec![e];
        while let Some(cur) = todo.pop() {
            let Some(ce) = ends(cur) else { continue };
            // The edges with an end near one of `cur`'s, in edge order (as a scan of every edge
            // meets them).
            let mut near: Vec<usize> = Vec::new();
            for (p, _) in &ce {
                let [x, y, z] = key(*p);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            near.extend(at_end.get(&[x + dx, y + dy, z + dz]).into_iter().flatten());
                        }
                    }
                }
            }
            near.sort_unstable();
            near.dedup();
            for i in near {
                #[cfg(test)]
                CHAIN_CANDIDATES.with(|n| n.set(n.get() + 1));
                if in_chain.contains(&i) || !self.edge_pickable(i) || smooth_seam(self, i, cos) {
                    continue;
                }
                let Some(ne) = ends(i) else { continue };
                // Smooth: leaving `cur` at a shared end continues into `i` (opposite directions).
                let smooth = ce.iter().any(|(p, d)| ne.iter().any(|(q, dn)| p.dist(*q) < tol && d.dot(-*dn) > cos));
                if smooth && chain.len() < 10_000 {
                    chain.push(i);
                    in_chain.insert(i);
                    todo.push(i);
                }
            }
        }
        chain
    }

    fn bounds(&self) -> Aabb3 {
        let mut b = Aabb3::EMPTY;
        for p in &self.positions {
            b.add(*p);
        }
        b
    }

    fn measure(&self) -> MeshMeasure {
        let (mut v6, mut area, mut c) = (0.0, 0.0, Vec3::ZERO);
        for t in &self.triangles {
            let Some([a, b, cc]) = self.tri(t) else { continue };
            let d = a.dot(b.cross(cc));
            v6 += d;
            c += (a + b + cc) * d;
            area += (b - a).cross(cc - a).len() * 0.5;
        }
        let volume = v6 / 6.0;
        let centroid = if v6.abs() > 1e-12 { c / (4.0 * v6) } else { self.bounds().center() };
        MeshMeasure { volume, area, centroid }
    }

    fn append(&mut self, o: &Mesh, face_offset: u32) {
        let base = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        self.positions.extend_from_slice(&o.positions);
        self.normals.extend_from_slice(&o.normals);
        self.triangles.extend(o.triangles.iter().map(|t| [t[0].saturating_add(base), t[1].saturating_add(base), t[2].saturating_add(base)]));
        self.tri_face.extend(o.tri_face.iter().map(|f| f.saturating_add(face_offset)));
        self.edges.extend(o.edges.iter().cloned());
    }

    fn ray_crossings(&self, o: Vec3, dir: Vec3) -> usize {
        self.triangles.iter().filter_map(|t| self.tri(t)).filter(|&[a, b, c]| ray_crosses(a, b, c, o, dir)).count()
    }

    fn contains(&self, p: Vec3) -> bool {
        let dirs = [Vec3::new(0.5773, 0.5774, 0.5776), Vec3::new(-0.6123, 0.3141, 0.7254), Vec3::new(0.2718, -0.8414, 0.4673)];
        dirs.iter().filter(|d| self.ray_crossings(p, **d) % 2 == 1).count() >= 2
    }

    fn crosses(&self, o: &Mesh, tol: f64) -> bool {
        self.crosses_except(o, tol, |_, _, _| false)
    }

    fn crosses_except(&self, o: &Mesh, tol: f64, ignored: impl Fn(Vec3, u32, u32) -> bool) -> bool {
        if !tol.is_finite() {
            return false;
        }
        let tol = tol.max(0.0);
        let (ba, bb) = (self.bounds(), o.bounds());
        let region = Aabb3 { min: ba.min.max(bb.min), max: ba.max.min(bb.max) };
        if region.is_empty() {
            return false;
        }
        // The triangles near the other mesh, with their faces.
        let near = |m: &Mesh| -> (Vec<BoxedTriangle>, Vec<u32>) {
            m.triangles
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let t = m.tri(t)?;
                    let mut bx = Aabb3::EMPTY;
                    t.iter().for_each(|p| bx.add(*p));
                    (!bx.is_empty() && bx.meets(&region)).then(|| ((t, bx), m.tri_face.get(i).copied().unwrap_or(u32::MAX)))
                })
                .unzip()
        };
        let ((ta, fa), (tb, fb)) = (near(self), near(o));
        if ta.is_empty() || tb.is_empty() {
            return false;
        }
        let work = std::cell::Cell::new(usize::MAX);
        let face = |faces: &[u32], i: usize| faces.get(i).copied().unwrap_or(u32::MAX);
        find_close_pair_within(&ta, &tb, 0.0, &work, |(i, (t, _)), (j, (u, _))| {
            triangles_cross(t, u, tol).is_some_and(|at| !ignored(at, face(&fa, i), face(&fb, j)))
        })
        .unwrap_or(false)
    }

    fn raycast(&self, o: Vec3, dir: Vec3) -> Option<(f64, usize)> {
        let frame = RayFrame::new(o, dir)?;
        let mut best: Option<(f64, usize)> = None;
        for (i, t) in self.triangles.iter().enumerate() {
            let Some(p) = self.tri(t) else { continue };
            if let Some(t) = frame.hit(p)
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, i));
            }
        }
        best
    }

    fn inside_index(&self) -> InsideIndex<'_> {
        let rays = RAY_DIRS
            .iter()
            .map(|d| {
                let u = d.any_perp();
                let v = d.cross(u);
                let tris: Vec<[(f64, f64); 3]> = self
                    .triangles
                    .iter()
                    .map(|t| match self.tri(t) {
                        Some([a, b, c]) => [(a.dot(u), a.dot(v)), (b.dot(u), b.dot(v)), (c.dot(u), c.dot(v))],
                        None => [(0.0, 0.0); 3],
                    })
                    .collect();
                let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
                for t in &tris {
                    for p in t {
                        lo = (lo.0.min(p.0), lo.1.min(p.1));
                        hi = (hi.0.max(p.0), hi.1.max(p.1));
                    }
                }
                let span = (hi.0 - lo.0).max(hi.1 - lo.1).max(1e-9);
                let side = ((tris.len() as f64).sqrt().ceil() as usize).clamp(1, 256);
                let cell = span / side as f64 * (1.0 + 1e-9);
                let n = (side + 1, side + 1);
                let mut cells = vec![Vec::new(); n.0 * n.1];
                if lo.0.is_finite() {
                    for (i, t) in tris.iter().enumerate() {
                        let (x0, x1) = (t.iter().map(|p| p.0).fold(f64::INFINITY, f64::min), t.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
                        let (y0, y1) = (t.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), t.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max));
                        let c = |x: f64, o: f64, m: usize| (((x - o) / cell).floor().max(0.0) as usize).min(m - 1);
                        for gx in c(x0, lo.0, n.0)..=c(x1, lo.0, n.0) {
                            for gy in c(y0, lo.1, n.1)..=c(y1, lo.1, n.1) {
                                if let Some(slot) = cells.get_mut(gx * n.1 + gy) {
                                    slot.push(i as u32);
                                }
                            }
                        }
                    }
                }
                RayGrid { dir: *d, u, v, min: lo, cell, n, cells }
            })
            .collect();
        InsideIndex { mesh: self, rays }
    }
}

/// Whether the two faces at edge `i` meet tangentially there (a smooth seam, with no corner).
fn smooth_seam(mesh: &Mesh, i: usize, cos: f64) -> bool {
    if mesh.sharp_edges.get(i).copied().unwrap_or(false) {
        return false;
    }
    if mesh.smooth_edges.get(i).copied().unwrap_or(false) {
        return true;
    }
    let Some([a, b]) = mesh.edge_faces.get(i).map(Vec::as_slice) else { return false };
    if a == b {
        return false;
    }
    let Some(p) = mesh.edges.get(i) else { return false };
    let (Some(&first), Some(&mid)) = (p.first(), p.get(p.len() / 2)) else { return false };
    let normal_near = |f: u32, q: Vec3| -> Option<Vec3> {
        mesh.triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter(|(_, tf)| **tf == f)
            .flat_map(|(t, _)| t.iter().copied())
            .filter_map(|v| Some((mesh.positions.get(v as usize)?.dist(q), *mesh.normals.get(v as usize)?)))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .and_then(|(_, n)| n.normalized())
    };
    let both = [first, mid];
    let sample = if p.len() == 2 { &both[..] } else { std::slice::from_ref(&mid) };
    sample.iter().all(|&q| matches!((normal_near(*a, q), normal_near(*b, q)), (Some(na), Some(nb)) if na.dot(nb) > cos))
}

/// A ray in the frame of [`Mesh::raycast`]'s watertight test: its dominant axis becomes z; x and
/// y follow it cyclically, swapped to keep the frame right-handed when the ray runs towards −z.
pub(crate) struct RayFrame {
    o: Vec3,
    k: [usize; 3],
    s: [f64; 3],
}

fn comp(v: Vec3, k: usize) -> f64 {
    match k {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

impl RayFrame {
    /// `None` for a direction that is zero or not finite.
    pub(crate) fn new(o: Vec3, dir: Vec3) -> Option<RayFrame> {
        let a = [dir.x.abs(), dir.y.abs(), dir.z.abs()];
        let kz = if a[0] >= a[1] && a[0] >= a[2] {
            0
        } else if a[1] >= a[2] {
            1
        } else {
            2
        };
        let dz = comp(dir, kz);
        if !(dz.is_finite() && dz != 0.0) {
            return None;
        }
        let (mut kx, mut ky) = ((kz + 1) % 3, (kz + 2) % 3);
        if dz < 0.0 {
            std::mem::swap(&mut kx, &mut ky);
        }
        Some(RayFrame { o, k: [kx, ky, kz], s: [comp(dir, kx) / dz, comp(dir, ky) / dz, 1.0 / dz] })
    }

    /// A vertex in the ray's frame: (x, y) across the ray, z scaled along it.
    fn frame(&self, p: Vec3) -> (f64, f64, f64) {
        let q = p - self.o;
        let z = comp(q, self.k[2]);
        (comp(q, self.k[0]) - self.s[0] * z, comp(q, self.k[1]) - self.s[1] * z, self.s[2] * z)
    }

    /// The distance along the ray to triangle `[pa, pb, pc]` (`None`: missed, or not in front).
    pub(crate) fn hit(&self, [pa, pb, pc]: [Vec3; 3]) -> Option<f64> {
        let ((ax, ay, az), (bx, by, bz), (cx, cy, cz)) = (self.frame(pa), self.frame(pb), self.frame(pc));
        let u = cx * by - cy * bx;
        let v = ax * cy - ay * cx;
        let w = bx * ay - by * ax;
        if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
            return None;
        }
        let det = u + v + w;
        if det == 0.0 {
            return None;
        }
        let t = (u * az + v * bz + w * cz) / det;
        (t.is_finite() && t > 1e-9).then_some(t)
    }
}

/// Point-in-mesh queries in bulk: for each of `Mesh::contains`'s three ray directions, the
/// triangles binned on a grid in the plane square to it, so a ray tests only the triangles
/// whose shadow covers its cell. Answers exactly as `Mesh::contains` does.
pub struct InsideIndex<'a> {
    mesh: &'a Mesh,
    rays: Vec<RayGrid>,
}

struct RayGrid {
    dir: Vec3,
    u: Vec3,
    v: Vec3,
    min: (f64, f64),
    cell: f64,
    n: (usize, usize),
    cells: Vec<Vec<u32>>,
}

const RAY_DIRS: [Vec3; 3] = [Vec3::new(0.5773, 0.5774, 0.5776), Vec3::new(-0.6123, 0.3141, 0.7254), Vec3::new(0.2718, -0.8414, 0.4673)];

impl InsideIndex<'_> {
    fn crossings(&self, g: &RayGrid, o: Vec3) -> usize {
        let (x, y) = (o.dot(g.u), o.dot(g.v));
        if !(g.min.0.is_finite()) {
            return 0;
        }
        let (fx, fy) = ((x - g.min.0) / g.cell, (y - g.min.1) / g.cell);
        if fx < 0.0 || fy < 0.0 {
            return 0;
        }
        let (gx, gy) = (fx.floor() as usize, fy.floor() as usize);
        if gx >= g.n.0 || gy >= g.n.1 {
            return 0;
        }
        let Some(list) = g.cells.get(gx * g.n.1 + gy) else { return 0 };
        list.iter()
            .filter_map(|&i| self.mesh.triangles.get(i as usize))
            .filter_map(|t| self.mesh.tri(t))
            .filter(|&[a, b, c]| ray_crosses(a, b, c, o, g.dir))
            .count()
    }

    /// Same answer as `Mesh::contains`.
    pub fn contains(&self, p: Vec3) -> bool {
        self.rays.iter().filter(|g| self.crossings(g, p) % 2 == 1).count() >= 2
    }
}

/// Whether the ray from `o` along `dir` crosses the triangle `a b c` in front of `o`. Watertight:
/// a ray through an edge two triangles share crosses exactly one of them. Each edge's side of the
/// ray is the signed volume `dir · ((p - o) × (q - o))`, exactly antisymmetric in floating point
/// (swapping `p` and `q` negates every rounded term), so both triangles see one value with
/// opposite signs; a ray exactly on the edge's line is put on one side by the edge's endpoints
/// in lexicographic order, the same for both. A triangle the ray runs along is not crossed.
fn ray_crosses(a: Vec3, b: Vec3, c: Vec3, o: Vec3, dir: Vec3) -> bool {
    let side = |p: Vec3, q: Vec3| {
        let v = dir.dot((p - o).cross(q - o));
        if v != 0.0 { v > 0.0 } else { (p.x, p.y, p.z) < (q.x, q.y, q.z) }
    };
    let normal = (b - a).cross(c - a);
    let along = dir.dot(normal);
    if along == 0.0 || !along.is_finite() {
        return false;
    }
    let s = side(a, b);
    side(b, c) == s && side(c, a) == s && (a - o).dot(normal) / along > 0.0
}
