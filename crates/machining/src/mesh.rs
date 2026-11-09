//! Surface mesh of the cut stock, one mesh per chunk of 32 by 32 by 32 cells.
//!
//! Dual marching cubes over the dexel lattice. Lattice nodes are inside or outside by the majority
//! of their three rays. Each lattice edge with a sign change lies on a ray, so its crossing is a
//! ray end. Within each cell the sign-changing edges are paired on every face (a face with two
//! sign changes pairs them; a checkerboard face with four pairs the edges around each outside
//! corner), and the pairs join into groups. Each group gets one vertex, the dual-contouring QEF
//! point: the least-squares point of the planes through its crossings, whose normals are estimated
//! from the neighbouring edges (see `Builder::estimate`), so a sharp edge stays sharp. A vertex may
//! sit up to half a cell outside its cell, the usual dual-contouring trade-off, fine for display.
//! Each quad of the surface takes, from each of its four cells, the vertex of the group holding its
//! edge.
//!
//! The result is closed and oriented: every edge is shared by an even number of triangles, two in
//! the usual case. Where one group holds both pairs of a checkerboard face, the edge can be shared
//! by four. That is not a manifold surface there, and it is not claimed to be.
//!
//! Edges are owned by their lower node, so each quad is emitted once, by the chunk that owns it,
//! and the chunks share their cell vertices bit for bit. Each chunk caches the inside state of its
//! nodes, and the crossing and normal of each edge it meets, so each is evaluated once per chunk
//! rather than once per cell that touches it.

use std::collections::HashMap;

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::dexel::Stock;

pub const CHUNK: i64 = 32;
/// The lowest lower node the mesher walks. Nodes below it are taken as air: the stock never has
/// material there, and a general lattice must keep its material above it.
const LOWEST: i64 = -2;

/// What the mesher needs of a lattice: its cell counts, which nodes are material, and where each
/// sign-changing edge crosses the boundary. Node indices run from `-1` to `n` on each axis.
pub trait Lattice {
    fn cell_counts(&self) -> [i64; 3];
    fn inside(&self, node: [i64; 3]) -> bool;
    fn crossing(&self, axis: usize, lower: [i64; 3]) -> Option<Vec3>;

    /// The coordinate of lattice node `i` along `axis`. Crossings sit on the node positions across
    /// their edge, so this is the same spacing the crossings use. A lattice on integer nodes keeps
    /// the default.
    fn node_position(&self, _axis: usize, i: i64) -> f64 {
        i as f64
    }
}

impl Lattice for Stock {
    fn cell_counts(&self) -> [i64; 3] {
        self.cell_counts().map(|x| x as i64)
    }

    fn inside(&self, node: [i64; 3]) -> bool {
        self.node_inside(node)
    }

    fn crossing(&self, axis: usize, lower: [i64; 3]) -> Option<Vec3> {
        self.edge_crossing(axis, lower)
    }

    fn node_position(&self, axis: usize, i: i64) -> f64 {
        self.pos(axis, i)
    }
}

/// Chunks along each axis: the nodes run from `-1` to `n`, so `n + 2` of them.
pub(crate) fn chunk_counts<L: Lattice>(lat: &L) -> [i64; 3] {
    lat.cell_counts().map(|x| (x - LOWEST + 1 + CHUNK - 1) / CHUNK)
}

/// The whole closed surface of the cut stock, one mesh per non-empty chunk, in chunk order. Untouched
/// chunks are included, with their box faces, so this is the surface for volume and closedness checks.
/// It is costly on a large stock: use [`chunk_mesh`] for drawing.
pub fn chunk_meshes(stock: &Stock) -> Vec<Mesh> {
    extract(stock)
}

/// The surface of any lattice, one mesh per non-empty chunk, in chunk order.
pub fn extract<L: Lattice>(lat: &L) -> Vec<Mesh> {
    let counts = chunk_counts(lat);
    let mut out = Vec::new();
    for cz in 0..counts[2] {
        for cy in 0..counts[1] {
            for cx in 0..counts[0] {
                if let Some(m) = chunk_mesh_of(lat, [cx, cy, cz]) {
                    out.push(m);
                }
            }
        }
    }
    out
}

/// The surface of one chunk of the cut stock: its quads, the box faces included, or `None` when no cut
/// has reached the chunk. The index is the chunk's position along each axis, as [`dirty_chunks`]
/// reports it.
///
/// Design: a chunk no cut reached is the uncut box. It has no mesh here, and the caller draws the box
/// itself. Box faces are per-cell quads, about 34 million triangles for a 300 x 200 mm top at 0.1 mm,
/// so they are never meshed for a chunk that a cut has not reached. A chunk a cut reached keeps its
/// own box faces, so the surface is closed where the cut is. The seam between a touched chunk's box
/// faces and the box the caller draws is not stitched: a hairline gap at most, not a topology error.
/// shortcut: stitch that seam (or draw the box with the same per-cell quads) when a UI needs it.
pub fn chunk_mesh(stock: &Stock, idx: [usize; 3]) -> Option<Mesh> {
    if !stock.is_touched(idx) {
        return None;
    }
    chunk_mesh_of(stock, idx.map(|x| x as i64))
}

/// A display mesh of chunk `idx` with the lattice sampled every `step` nodes (1, 2, 4 or 8): a coarse
/// chunk spans `step` fine chunks along each axis and has about 1/step² of the quads. For drawing a
/// stock the view is too big for only. The surface is not the simulated one and need not be closed.
/// `None` when no cut reached the coarse chunk's fine nodes, as for [`chunk_mesh`]. Coarse node `k` is
/// fine node `k * step`, so coarse chunk `c` covers fine nodes from `step * (c * CHUNK - 2)` on.
pub fn chunk_mesh_lod(stock: &Stock, idx: [usize; 3], step: u32) -> Option<Mesh> {
    let step = i64::from(step.clamp(1, 8));
    if step == 1 {
        return chunk_mesh(stock, idx);
    }
    if !display_reached(stock, idx, step as u32) {
        return None;
    }
    let c = idx.map(|x| x as i64);
    let n = stock.cell_counts().map(|x| x as i64);
    let counts = n.map(|x| (x + step - 1) / step);
    chunk_mesh_of(&Strided { stock, step, counts }, c)
}

/// Whether a cut reached display chunk `idx` at `step`: where it did not, the chunk is the uncut box
/// and [`chunk_mesh_lod`] has no mesh; where it did, no mesh means the cut took all of it.
pub fn display_reached(stock: &Stock, idx: [usize; 3], step: u32) -> bool {
    let step = i64::from(step.clamp(1, 8));
    if step == 1 {
        return stock.is_touched(idx);
    }
    let c = idx.map(|x| x as i64);
    // Fine chunks that the coarse chunk's nodes can reach, with one chunk of margin either side.
    stock.touched_chunks().into_iter().any(|t| (0..3).all(|k| (t[k] as i64) + 1 >= c[k] * step && (t[k] as i64) <= (c[k] + 1) * step))
}

/// A lattice sampled every `step` nodes: coarse node `k` is fine node `k * step`, and a coarse edge
/// crosses where the first fine edge along it changes sign.
struct Strided<'a> {
    stock: &'a Stock,
    step: i64,
    counts: [i64; 3],
}

impl Lattice for Strided<'_> {
    fn cell_counts(&self) -> [i64; 3] {
        self.counts
    }

    fn inside(&self, node: [i64; 3]) -> bool {
        Lattice::inside(self.stock, node.map(|k| k * self.step))
    }

    fn crossing(&self, axis: usize, lower: [i64; 3]) -> Option<Vec3> {
        let base = lower.map(|k| k * self.step);
        let at = |i: i64| {
            let mut p = base;
            p[axis] += i;
            p
        };
        for i in 0..self.step {
            let (p, q) = (at(i), at(i + 1));
            if Lattice::inside(self.stock, p) != Lattice::inside(self.stock, q) {
                return Lattice::crossing(self.stock, axis, p);
            }
        }
        // The coarse edge's ends differ, so a fine edge between them changes; the middle is a fallback.
        let mid = |k: usize| {
            let (a, b) = (self.stock.pos(k, base[k]), self.stock.pos(k, base[k] + if k == axis { self.step } else { 0 }));
            0.5 * (a + b)
        };
        Some(Vec3::new(mid(0), mid(1), mid(2)))
    }

    fn node_position(&self, axis: usize, i: i64) -> f64 {
        self.stock.pos(axis, i * self.step)
    }
}

/// The most display chunks a display draws before it coarsens (see [`display_step`]).
const DISPLAY_CHUNKS: usize = 128;

/// The display step (1, 2, 4 or 8) for a stock with `touched` chunks reached by cuts: the least step
/// whose coarse chunk count, `touched / step^3`, is at most [`DISPLAY_CHUNKS`]. A coarse chunk costs
/// about as much to mesh as a fine one, and there are a step cubed times fewer of them.
pub fn display_step(touched: usize) -> u32 {
    let mut step = 1u32;
    while step < 8 && touched / (step as usize).pow(3) > DISPLAY_CHUNKS {
        step *= 2;
    }
    step
}

/// Chunk counts per axis of the display at `step` (1, 2, 4 or 8), as [`chunk_mesh_lod`] indexes them.
pub fn display_counts(stock: &Stock, step: u32) -> [usize; 3] {
    let step = i64::from(step.clamp(1, 8));
    let counts = stock.cell_counts().map(|x| (x as i64 + step - 1) / step);
    chunk_counts(&Strided { stock, step, counts }).map(|c| c.max(0) as usize)
}

/// The display chunks at `step` whose mesh a change to fine chunk `fine` can alter. At step 1 that is
/// the chunk itself: the stock already marks the neighbours a cut can change. A display chunk spans
/// `step` fine ones, so at a coarser step the chunks within two of `fine / step` either side are taken.
pub fn display_chunks_for(fine: [usize; 3], step: u32, counts: [usize; 3]) -> Vec<[usize; 3]> {
    if step <= 1 {
        return vec![fine];
    }
    let s = step as usize;
    let per_axis: [Vec<usize>; 3] = std::array::from_fn(|k| {
        let base = fine[k] / s;
        (base.saturating_sub(2)..(base + 3).min(counts[k])).collect()
    });
    let mut out = Vec::new();
    for &x in &per_axis[0] {
        for &y in &per_axis[1] {
            for &z in &per_axis[2] {
                out.push([x, y, z]);
            }
        }
    }
    out
}

/// The uncut box's faces at `step`, for the display chunks `covered` says have a mesh of their own.
/// Each face is cut into one quad per display chunk, so a chunk with its own surface (see
/// [`chunk_mesh_lod`]) leaves its part of the box open, as the design note of [`chunk_mesh`] says. A
/// quad belongs to the chunk that owns its edge, so a chunk's box faces are the ones its mesh draws.
pub fn box_faces(stock: &Stock, step: u32, covered: impl Fn([usize; 3]) -> bool) -> Mesh {
    let step = step.clamp(1, 8);
    let s = i64::from(step);
    let n = stock.cell_counts().map(|x| (x as i64).max(1));
    let counts = display_counts(stock, step);
    let (lo, hi) = stock.bounds();
    let cell: [f64; 3] = std::array::from_fn(|k| stock.pos(k, 1) - stock.pos(k, 0));
    let (mut positions, mut normals, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
    for a in 0..3 {
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        for low in [true, false] {
            // The face's quads are the edges along `a` at one lower node: -1 for the low face, the last
            // material node for the high one (in display nodes).
            let l_a = if low { -1 } else { (n[a] - 1) / s };
            let sign = if low { -1.0 } else { 1.0 };
            let plane = if low { lo[a] } else { hi[a] };
            let last = [(n[u] - 1) / s, (n[v] - 1) / s];
            let owner = ((l_a - LOWEST) / CHUNK) as usize;
            // The lower nodes chunk `c` owns along an axis, clipped to the box.
            let span = |c: usize, last: i64| {
                let first = (c as i64) * CHUNK + LOWEST;
                let (from, to) = (first.max(0), (first + CHUNK - 1).min(last));
                (from <= to).then_some((from, to))
            };
            for cu in 0..counts[u] {
                for cv in 0..counts[v] {
                    let mut idx = [0usize; 3];
                    idx[a] = owner;
                    idx[u] = cu;
                    idx[v] = cv;
                    if covered(idx) {
                        continue;
                    }
                    let (Some((ul0, ul1)), Some((vl0, vl1))) = (span(cu, last[0]), span(cv, last[1])) else { continue };
                    let at = |k: usize, from: i64, to: i64| {
                        let stride = s as f64 * cell[k];
                        (lo[k] + from as f64 * stride, (lo[k] + (to + 1) as f64 * stride).min(hi[k]))
                    };
                    let (u0, u1) = at(u, ul0, ul1);
                    let (v0, v1) = at(v, vl0, vl1);
                    let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(pu, pv)| {
                        let mut p = [0.0f64; 3];
                        p[a] = plane;
                        p[u] = pu;
                        p[v] = pv;
                        Vec3::from(p)
                    });
                    let mut nrm = [0.0f64; 3];
                    nrm[a] = sign;
                    let Ok(base) = u32::try_from(positions.len()) else { continue };
                    positions.extend(corners);
                    normals.extend([Vec3::from(nrm); 4]);
                    // Counter-clockwise seen from outside: the (u, v) order of the corners gives +a.
                    let quad = if low { [[0, 2, 1], [0, 3, 2]] } else { [[0, 1, 2], [0, 2, 3]] };
                    triangles.extend(quad.map(|t| t.map(|i| base + i)));
                }
            }
        }
    }
    let tri_face = vec![0u32; triangles.len()];
    Mesh { positions, normals, triangles, tri_face, ..Mesh::default() }
}

/// The chunks whose surface may have changed since the last [`Stock::clear_dirty`]: each once. A cut
/// records them as it applies (see [`chunks_for_nodes`]), so a small cut in a large stock marks only
/// the chunks near it.
pub fn dirty_chunks(stock: &Stock) -> Vec<[usize; 3]> {
    stock.dirty_chunks()
}

/// The chunks whose mesh reads a node of the box `lo..=hi` (node indices per axis), with a margin of
/// two nodes either side. A chunk of first lower node `first` reads the nodes `first - 2` to
/// `first + CHUNK + 1`, so a node affects the chunks whose window holds it. `counts` are the chunk
/// counts of the lattice; the result is clipped to them.
pub(crate) fn chunks_for_nodes(lo: [i64; 3], hi: [i64; 3], counts: [i64; 3]) -> Vec<[usize; 3]> {
    let per_axis: [Vec<i64>; 3] = std::array::from_fn(|k| {
        // Chunk c reads nodes [CHUNK * c + LOWEST - 2, CHUNK * c + LOWEST + CHUNK + 1].
        let a = lo[k] - 2;
        let b = hi[k] + 2;
        let c_min = -((-(a - LOWEST - CHUNK - 1)).div_euclid(CHUNK));
        let c_max = (b - LOWEST + 2).div_euclid(CHUNK);
        let (from, to) = (c_min.max(0), c_max.min(counts[k] - 1));
        (from..=to).collect()
    });
    let mut out = Vec::new();
    for &x in &per_axis[0] {
        for &y in &per_axis[1] {
            for &z in &per_axis[2] {
                out.push([x as usize, y as usize, z as usize]);
            }
        }
    }
    out
}

/// The volume of the cut stock from its surface, by the divergence theorem. The chunk meshes share
/// their boundary vertices exactly, so their signed volumes add. It is not uniformly more precise than
/// [`Stock::volume`]: its vertices are QEF points, which keep sharp edges sharp and sit within half a
/// cell of their cell.
pub fn mesh_volume(stock: &Stock) -> f64 {
    chunk_meshes(stock).iter().map(|m| m.measure().volume).sum()
}

fn chunk_mesh_of<L: Lattice>(lat: &L, idx: [i64; 3]) -> Option<Mesh> {
    let counts = chunk_counts(lat);
    if idx.iter().zip(counts.iter()).any(|(i, c)| *i < 0 || i >= c) {
        return None;
    }
    let first = idx.map(|i| i * CHUNK + LOWEST);
    let mut builder = Builder::new(lat, lat.cell_counts(), first);
    builder.chunk(first);
    if builder.triangles.is_empty() {
        return None;
    }
    Some(builder.finish())
}

/// Edge index within a cell: the axis, then the offsets (0 or 1) along the other two axes.
fn edge_id(axis: usize, off: [i64; 3]) -> usize {
    let p = off[(axis + 1) % 3] as usize;
    let q = off[(axis + 2) % 3] as usize;
    axis * 4 + p * 2 + q
}

/// The lower corner offset (0 or 1 on each axis, zero along the edge's own axis) of an edge index.
fn edge_offset(id: usize) -> (usize, [i64; 3]) {
    let axis = id / 4;
    let mut off = [0i64; 3];
    off[(axis + 1) % 3] = ((id / 2) % 2) as i64;
    off[(axis + 2) % 3] = (id % 2) as i64;
    (axis, off)
}

fn find(parent: &mut [usize; 12], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn join(parent: &mut [usize; 12], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

/// The offsets of corner `c` of a cell: index `x + 2 y + 4 z`.
fn corner_offset(c: usize) -> [i64; 3] {
    [(c & 1) as i64, ((c >> 1) & 1) as i64, ((c >> 2) & 1) as i64]
}

/// The QEF vertex of a group from its crossings (position, normal): the point that minimises the
/// squared distances to the planes through the crossings, plus `0.05 * count` times the squared
/// distance to their mean. Both terms are squared lengths, so the weight is dimensionless and the
/// solve needs no cell size. The mean stands in when the solve fails or the point leaves the cell
/// grown by half a cell. `None` when the group has no crossing.
fn group_vertex<L: Lattice>(lat: &L, cell: [i64; 3], crossings: &[(Vec3, Vec3)]) -> Option<Vec3> {
    if crossings.is_empty() {
        return None;
    }
    let count = crossings.len() as f64;
    let mean = crossings.iter().fold(Vec3::ZERO, |acc, (p, _)| acc + *p) / count;
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for (p, n) in crossings {
        let b = n.dot(*p - mean);
        let n: [f64; 3] = (*n).into();
        for r in 0..3 {
            atb[r] += b * n[r];
            for c in 0..3 {
                ata[r][c] += n[r] * n[c];
            }
        }
    }
    // The weight keeps the solve definite: along a flat direction the vertex stays at the mean.
    for (k, row) in ata.iter_mut().enumerate() {
        row[k] += 0.05 * count;
    }
    let mean_arr: [f64; 3] = mean.into();
    let Some(offset) = solve3(ata, atb) else { return Some(mean) };
    let x: [f64; 3] = std::array::from_fn(|k| mean_arr[k] + offset[k]);
    let inside_cell = (0..3).all(|k| {
        let (a, b) = (lat.node_position(k, cell[k]), lat.node_position(k, cell[k] + 1));
        let half = 0.5 * (b - a).abs();
        x[k] >= a.min(b) - half && x[k] <= a.max(b) + half
    });
    Some(if inside_cell { Vec3::from(x) } else { mean })
}

/// Solves the 3 by 3 system `m x = b` by Cramer's rule. `None` when the determinant is too small to
/// trust or the answer is not finite.
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |c: [[f64; 3]; 3]| {
        c[0][0] * (c[1][1] * c[2][2] - c[1][2] * c[2][1]) - c[0][1] * (c[1][0] * c[2][2] - c[1][2] * c[2][0])
            + c[0][2] * (c[1][0] * c[2][1] - c[1][1] * c[2][0])
    };
    let d = det(m);
    if !(d.is_finite() && d.abs() > 1e-12) {
        return None;
    }
    let mut x = [0.0f64; 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut c = m;
        for (r, row) in c.iter_mut().enumerate() {
            row[k] = b[r];
        }
        *xk = det(c) / d;
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The inside or outside state of a box of nodes, evaluated once.
struct NodeCache {
    origin: [i64; 3],
    dims: [i64; 3],
    inside: Vec<bool>,
}

impl NodeCache {
    fn build<L: Lattice>(lat: &L, origin: [i64; 3], dims: [i64; 3]) -> NodeCache {
        let total = (dims[0] * dims[1] * dims[2]).max(0) as usize;
        let mut inside = Vec::with_capacity(total);
        for x in 0..dims[0] {
            for y in 0..dims[1] {
                for z in 0..dims[2] {
                    inside.push(lat.inside([origin[0] + x, origin[1] + y, origin[2] + z]));
                }
            }
        }
        NodeCache { origin, dims, inside }
    }

    fn get<L: Lattice>(&self, lat: &L, node: [i64; 3]) -> bool {
        let rel: [i64; 3] = std::array::from_fn(|k| node[k] - self.origin[k]);
        if rel.iter().zip(self.dims.iter()).all(|(r, d)| *r >= 0 && r < d) {
            let i = ((rel[0] * self.dims[1] + rel[1]) * self.dims[2] + rel[2]) as usize;
            if let Some(v) = self.inside.get(i) {
                return *v;
            }
        }
        lat.inside(node)
    }
}

struct Builder<'a, L: Lattice> {
    lat: &'a L,
    n: [i64; 3],
    nodes: NodeCache,
    /// Per cell: the vertex of each sign-changing edge of the cell, by edge index.
    cells: HashMap<[i64; 3], [Option<u32>; 12]>,
    /// Per edge (axis, lower node): its crossing and surface normal, or `None` when it has no crossing.
    edges: HashMap<(usize, [i64; 3]), Option<(Vec3, Vec3)>>,
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}

impl<'a, L: Lattice> Builder<'a, L> {
    /// A builder for the chunk whose first lower node is `first`. Quads and cell patches of the
    /// chunk read nodes from `first - 2` to `first + CHUNK + 1` on each axis, so the cache covers that.
    /// The normal of an edge reads one node further each way, which the cache also covers.
    fn new(lat: &'a L, n: [i64; 3], first: [i64; 3]) -> Self {
        let origin = first.map(|f| f - 2);
        let nodes = NodeCache::build(lat, origin, [CHUNK + 4; 3]);
        Builder { lat, n, nodes, cells: HashMap::new(), edges: HashMap::new(), positions: Vec::new(), triangles: Vec::new() }
    }

    fn inside(&self, node: [i64; 3]) -> bool {
        self.nodes.get(self.lat, node)
    }

    /// Whether the edge from `node` along `axis` is sign-changing.
    fn changes(&self, axis: usize, node: [i64; 3]) -> bool {
        let mut upper = node;
        upper[axis] += 1;
        self.inside(node) != self.inside(upper)
    }

    /// The crossing and the surface normal of the edge from `lower` along `axis`, memoised per chunk.
    fn surface(&mut self, axis: usize, lower: [i64; 3]) -> Option<(Vec3, Vec3)> {
        if let Some(v) = self.edges.get(&(axis, lower)) {
            return *v;
        }
        let v = self.estimate(axis, lower);
        self.edges.insert((axis, lower), v);
        v
    }

    /// The crossing of the edge from `lower` along `axis`, and its normal pointing from material to air.
    /// The normal is estimated from the square of edges around the crossing. Along each perpendicular
    /// axis `k` the surface height has a slope from each side: the square towards `k` (the parallel
    /// neighbour, the two `k` edges at its ends) must hold exactly one more sign change, so one sheet
    /// crosses it, and the slope is that crossing's height difference over its `k` difference. A
    /// square with three more changes is a saddle and gives no slope, and so does a side with none.
    /// Of the two sides the smaller-magnitude slope is taken, so a crossing beside a crease takes its
    /// own face's slope. Slopes are clamped to +-1e3. Air-to-material crossings need no separate case:
    /// they are the same single-change squares, and their normal flips with the sign below.
    fn estimate(&self, axis: usize, lower: [i64; 3]) -> Option<(Vec3, Vec3)> {
        let here = self.lat.crossing(axis, lower)?;
        let mut raw = [0.0f64; 3];
        raw[axis] = 1.0;
        for k in [(axis + 1) % 3, (axis + 2) % 3] {
            let s = match (self.side_slope(axis, k, lower, -1, here), self.side_slope(axis, k, lower, 1, here)) {
                (Some(a), Some(b)) => {
                    if a.abs() <= b.abs() {
                        a
                    } else {
                        b
                    }
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => 0.0,
            };
            raw[k] = -s;
        }
        // The height gradient points from material to air when the lower end is material, so flip it
        // when the lower end is air.
        let sign = if self.inside(lower) { 1.0 } else { -1.0 };
        let normal = Vec3::from(raw.map(|x| sign * x)).normalized().unwrap_or(Vec3::ZERO);
        Some((here, normal))
    }

    /// The slope along `k` of the surface through the crossing `here` of the edge from `lower`, towards
    /// `side` (-1 or +1) along `k`, from the one other sign change in that square (see [`Self::estimate`]).
    fn side_slope(&self, axis: usize, k: usize, lower: [i64; 3], side: i64, here: Vec3) -> Option<f64> {
        let mut nb = lower;
        nb[k] += side;
        let mut bottom = lower;
        bottom[k] += side.min(0);
        let mut top = bottom;
        top[axis] += 1;
        let others = [(axis, nb), (k, bottom), (k, top)];
        let mut changed = others.into_iter().filter(|&(dir, node)| self.changes(dir, node));
        let (dir, node) = changed.next()?;
        if changed.next().is_some() {
            return None;
        }
        let other = self.lat.crossing(dir, node)?;
        let (h, t) = (<[f64; 3]>::from(here), <[f64; 3]>::from(other));
        let (dh, dk) = (t[axis] - h[axis], t[k] - h[k]);
        let s = dh / dk;
        // A zero over zero (both crossings at one point) has no slope; an infinite one is clamped.
        Some(if s.is_nan() { 0.0 } else { s.clamp(-1e3, 1e3) })
    }

    /// Emits the quads of the edges owned by the chunk whose first lower node is `first`.
    fn chunk(&mut self, first: [i64; 3]) {
        let hi: [i64; 3] = std::array::from_fn(|k| (first[k] + CHUNK).min(self.n[k] + 1));
        for a in 0..3 {
            for l0 in first[0].max(LOWEST)..hi[0] {
                for l1 in first[1].max(LOWEST)..hi[1] {
                    for l2 in first[2].max(LOWEST)..hi[2] {
                        // Lower nodes run up to n: an edge from material at n leaves the lattice to air,
                        // and its quad must still be emitted.
                        let lower = [l0, l1, l2];
                        self.edge(a, lower);
                    }
                }
            }
        }
    }

    /// The quad for one lattice edge, if it has a sign change.
    fn edge(&mut self, a: usize, lower: [i64; 3]) {
        let mut upper = lower;
        upper[a] += 1;
        let inside = self.inside(lower);
        if inside == self.inside(upper) {
            return;
        }
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        // The four cells around the edge, in counter-clockwise order seen from the +axis direction.
        let offsets = [(-1i64, -1i64), (0, -1), (0, 0), (-1, 0)];
        let mut quad = [0u32; 4];
        for (q, (du, dv)) in quad.iter_mut().zip(offsets) {
            let mut cell = lower;
            cell[u] += du;
            cell[v] += dv;
            let mut local = [0i64; 3];
            local[u] = lower[u] - cell[u];
            local[v] = lower[v] - cell[v];
            let Some(id) = self.vertex(cell, edge_id(a, local)) else { return };
            *q = id;
        }
        let [v0, v1, v2, v3] = quad;
        let tris = if inside { [[v0, v1, v2], [v0, v2, v3]] } else { [[v0, v2, v1], [v0, v3, v2]] };
        self.triangles.extend(tris);
    }

    /// The vertex of edge `edge` of `cell`, computing the cell's vertices once.
    fn vertex(&mut self, cell: [i64; 3], edge: usize) -> Option<u32> {
        if !self.cells.contains_key(&cell) {
            let patch = self.patch(cell);
            self.cells.insert(cell, patch);
        }
        self.cells.get(&cell).and_then(|p| p.get(edge).copied().flatten())
    }

    /// The vertices of one cell: pair the sign-changing edges on each face, group the pairs, and
    /// give each group its QEF vertex (see [`group_vertex`]).
    fn patch(&mut self, cell: [i64; 3]) -> [Option<u32>; 12] {
        let mut sign = [false; 8];
        for (c, s) in sign.iter_mut().enumerate() {
            let off = corner_offset(c);
            *s = self.inside(std::array::from_fn(|k| cell[k] + off[k]));
        }
        let corner = |off: [i64; 3]| (off[0] + 2 * off[1] + 4 * off[2]) as usize;
        let mut change = [false; 12];
        for (id, ch) in change.iter_mut().enumerate() {
            let (axis, off0) = edge_offset(id);
            let mut off1 = off0;
            off1[axis] = 1;
            *ch = sign[corner(off0)] != sign[corner(off1)];
        }
        let mut parent: [usize; 12] = std::array::from_fn(|i| i);
        for a in 0..3 {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            for s in [0i64, 1] {
                let on_face = |axis: usize, other: usize, t: i64| {
                    let mut off = [0i64; 3];
                    off[a] = s;
                    off[other] = t;
                    edge_id(axis, off)
                };
                let face = [on_face(u, v, 0), on_face(u, v, 1), on_face(v, u, 0), on_face(v, u, 1)];
                let changed: Vec<usize> = face.iter().copied().filter(|&e| change[e]).collect();
                match changed.len() {
                    2 => join(&mut parent, changed[0], changed[1]),
                    4 => {
                        // A checkerboard: pair the edges around each outside corner of the face.
                        for pu in [0i64, 1] {
                            for pv in [0i64, 1] {
                                let mut off = [0i64; 3];
                                off[a] = s;
                                off[u] = pu;
                                off[v] = pv;
                                if sign[corner(off)] {
                                    continue;
                                }
                                join(&mut parent, on_face(u, v, pv), on_face(v, u, pu));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        // One vertex per group, from the position and normal of each of its crossings.
        let mut groups: HashMap<usize, (Vec<usize>, Vec<(Vec3, Vec3)>)> = HashMap::new();
        for (id, ch) in change.iter().enumerate() {
            if !ch {
                continue;
            }
            let root = find(&mut parent, id);
            let (axis, off0) = edge_offset(id);
            let lower: [i64; 3] = std::array::from_fn(|k| cell[k] + off0[k]);
            let entry = groups.entry(root).or_insert_with(|| (Vec::new(), Vec::new()));
            entry.0.push(id);
            if let Some(crossing) = self.surface(axis, lower) {
                entry.1.push(crossing);
            }
        }
        let mut patch = [None; 12];
        let mut roots: Vec<usize> = groups.keys().copied().collect();
        roots.sort_unstable();
        for root in roots {
            let Some((edges, crossings)) = groups.get(&root) else { continue };
            let Some(p) = group_vertex(self.lat, cell, crossings) else { continue };
            self.positions.push(p);
            let Ok(id) = u32::try_from(self.positions.len() - 1) else { continue };
            for e in edges {
                if let Some(slot) = patch.get_mut(*e) {
                    *slot = Some(id);
                }
            }
        }
        patch
    }

    fn finish(self) -> Mesh {
        let mut normals = vec![Vec3::ZERO; self.positions.len()];
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| self.positions.get(i as usize).copied().unwrap_or(Vec3::ZERO));
            let nrm = (b - a).cross(c - a);
            for i in t {
                if let Some(acc) = normals.get_mut(*i as usize) {
                    *acc += nrm;
                }
            }
        }
        for nrm in &mut normals {
            *nrm = nrm.normalized().unwrap_or(Vec3::ZERO);
        }
        let tri_face = vec![0u32; self.triangles.len()];
        Mesh { positions: self.positions, normals, triangles: self.triangles, tri_face, ..Mesh::default() }
    }
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
