use std::collections::HashMap;

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;

use super::{Lattice, chunk_counts, chunk_mesh, chunk_meshes, dirty_chunks, extract, mesh_volume};
use crate::dexel::Stock;
use crate::sweep::{FlatTool, LinearFixedAxis};

/// Chunk meshes welded by exact position (chunks share their boundary vertices bit for bit), so the
/// result is one mesh whose edges can be counted.
fn weld(meshes: &[Mesh]) -> Mesh {
    let mut index: HashMap<[u64; 3], u32> = HashMap::new();
    let mut out = Mesh::default();
    for m in meshes {
        let map: Vec<u32> = m
            .positions
            .iter()
            .map(|p| {
                let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                *index.entry(key).or_insert_with(|| {
                    out.positions.push(*p);
                    (out.positions.len() - 1) as u32
                })
            })
            .collect();
        for t in &m.triangles {
            out.triangles.push([map[t[0] as usize], map[t[1] as usize], map[t[2] as usize]]);
        }
    }
    out.tri_face = vec![0; out.triangles.len()];
    out
}

/// Every undirected edge is shared by exactly two triangles, and each directed edge appears once.
fn assert_two_per_edge(m: &Mesh) {
    let mut count: HashMap<(u32, u32), u32> = HashMap::new();
    let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
    for t in &m.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            *directed.entry((a, b)).or_insert(0) += 1;
        }
    }
    let bad: Vec<_> = count.iter().filter(|(_, n)| **n != 2).take(5).collect();
    assert!(bad.is_empty(), "edges not shared by two triangles: {bad:?} of {} edges", count.len());
    assert!(directed.values().all(|n| *n == 1), "a directed edge repeats");
    assert!(!m.triangles.is_empty());
}

/// A synthetic lattice of `n` cells per axis, with material given by a predicate on nodes. The
/// crossing of each edge is its midpoint, so tests check the topology and not the placement.
struct Synthetic<F: Fn([i64; 3]) -> bool> {
    n: i64,
    material: F,
}

impl<F: Fn([i64; 3]) -> bool> Lattice for Synthetic<F> {
    fn cell_counts(&self) -> [i64; 3] {
        [self.n; 3]
    }

    fn inside(&self, node: [i64; 3]) -> bool {
        node.iter().all(|&x| x >= 0 && x < self.n) && (self.material)(node)
    }

    fn crossing(&self, axis: usize, lower: [i64; 3]) -> Option<Vec3> {
        let p: [f64; 3] = std::array::from_fn(|k| lower[k] as f64 + if k == axis { 0.5 } else { 0.0 });
        Some(Vec3::from(p))
    }
}

fn slot_stock(h: f64) -> Stock {
    let mut s = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(30.0, 30.0, 0.0), h, 1 << 32).expect("stock");
    let tool = FlatTool::new(2.5, 20.0, Vec3::Z).expect("tool");
    let slot = LinearFixedAxis { from: Vec3::new(8.0, 15.0, -4.0), to: Vec3::new(22.0, 15.0, -4.0), tool };
    s.apply(&slot, 1).expect("cut");
    s
}

#[test]
fn uncut_box_mesh_is_closed_and_its_volume_is_near_the_box() {
    let s = Stock::new_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0), 0.25, 1 << 32).expect("stock");
    let m = weld(&chunk_meshes(&s));
    assert_two_per_edge(&m);
    let v = m.measure().volume;
    assert!(v > 0.0, "outward orientation: {v}");
    assert!((v - 8.0).abs() < 0.5, "box volume {v}");
}

#[test]
fn cut_stock_mesh_is_closed_across_chunk_seams() {
    let s = slot_stock(0.25);
    let meshes = chunk_meshes(&s);
    assert!(meshes.len() > 1, "more than one chunk");
    assert_two_per_edge(&weld(&meshes));
}

#[test]
fn checkerboard_lattice_has_two_triangles_per_edge() {
    // Every face of every cell is a checkerboard: the hardest case for the pairing.
    for n in [3, 4, 40] {
        let lat = Synthetic { n, material: |node: [i64; 3]| (node[0] + node[1] + node[2]) % 2 == 0 };
        let m = weld(&extract(&lat));
        assert_two_per_edge(&m);
    }
}

#[test]
fn diagonal_wall_lattice_has_two_triangles_per_edge() {
    // Material on one side of a diagonal plane: faces are checkerboards along the plane.
    for n in [4, 40] {
        let lat = Synthetic { n, material: |node: [i64; 3]| node[0] >= node[1] };
        assert_two_per_edge(&weld(&extract(&lat)));
    }
}

#[test]
fn cut_stock_mesh_volume_is_within_h_of_the_dexel_volume() {
    let h = 0.25;
    let s = slot_stock(h);
    let m = weld(&chunk_meshes(&s));
    let mesh_v = m.measure().volume;
    let dexel_v = s.volume();
    let area = m.measure().area;
    eprintln!("mesh {mesh_v:.3} dexel {dexel_v:.3} area {area:.3}");
    assert!((mesh_v - dexel_v).abs() <= h * area, "mesh {mesh_v} vs dexel {dexel_v}, area {area}");
}

#[test]
fn cut_stock_volume_matches_the_analytic_block_minus_slot() {
    let h = 0.25;
    let s = slot_stock(h);
    let m = weld(&chunk_meshes(&s));
    let analytic = 30.0 * 30.0 * 10.0 - 4.0 * (2.0 * 2.5 * 14.0 + std::f64::consts::PI * 2.5 * 2.5);
    let mesh_v = m.measure().volume;
    assert!((mesh_v - analytic).abs() < 2.0 * h * 30.0 * 4.0, "mesh {mesh_v} vs analytic {analytic}");
}

#[test]
fn every_chunk_mesh_is_non_empty_and_positions_are_finite() {
    let s = slot_stock(0.25);
    for m in chunk_meshes(&s) {
        assert!(!m.triangles.is_empty());
        assert!(m.positions.iter().all(|p| p.is_finite()));
        assert!(m.triangles.iter().all(|t| t.iter().all(|i| (*i as usize) < m.positions.len())));
    }
}

/// A lattice given by its material nodes, for the review's example.
struct NodeSet {
    n: i64,
    set: std::collections::HashSet<[i64; 3]>,
}

impl Lattice for NodeSet {
    fn cell_counts(&self) -> [i64; 3] {
        [self.n; 3]
    }

    fn inside(&self, node: [i64; 3]) -> bool {
        self.set.contains(&node)
    }

    fn crossing(&self, axis: usize, lower: [i64; 3]) -> Option<Vec3> {
        let p: [f64; 3] = std::array::from_fn(|k| lower[k] as f64 + if k == axis { 0.5 } else { 0.0 });
        Some(Vec3::from(p))
    }
}

#[test]
fn example_lattice_has_even_edge_counts_including_across_a_seam() {
    // Four material nodes, all else air: one group holds both pairs of a checkerboard face, so an
    // edge can be shared by four triangles. Every edge must still be shared an even number of times.
    let example = [[0i64, 0, -1], [0, 0, 0], [0, 0, 1], [1, 1, 0]];
    for (n, offset) in [(2i64, 0i64), (64, 30)] {
        let set = example.iter().map(|p| [p[0] + offset, p[1] + offset, p[2] + offset]).collect();
        let lat = NodeSet { n, set };
        let m = weld(&extract(&lat));
        assert!(!m.triangles.is_empty());
        let mut count: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &m.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        let odd: Vec<_> = count.iter().filter(|(_, c)| *c % 2 != 0).take(5).collect();
        assert!(odd.is_empty(), "offset {offset}: edges with an odd count {odd:?}");
    }
}

#[test]
fn chunk_meshes_are_the_same_as_chunk_by_chunk() {
    // chunk_mesh meshes a chunk a cut has reached, and only those: together they are a part of the
    // whole surface (the untouched chunks' box faces are in chunk_meshes alone).
    let s = slot_stock(0.25);
    let all = chunk_meshes(&s);
    let mut by_index = Vec::new();
    let mut touched = 0;
    for cz in 0..4usize {
        for cy in 0..4usize {
            for cx in 0..4usize {
                let idx = [cx, cy, cz];
                assert_eq!(chunk_mesh(&s, idx).is_some(), s.is_touched(idx), "{idx:?}");
                touched += usize::from(s.is_touched(idx));
                if let Some(m) = chunk_mesh(&s, idx) {
                    by_index.push(m);
                }
            }
        }
    }
    assert_eq!(by_index.len(), touched);
    let tris = |ms: &[Mesh]| ms.iter().map(|m| m.triangles.len()).sum::<usize>();
    assert!(tris(&by_index) <= tris(&all), "{} vs {}", tris(&by_index), tris(&all));
    assert!(tris(&by_index) > 0);
}

fn fingerprint(m: Option<Mesh>) -> (usize, u64) {
    match m {
        None => (0, 0),
        Some(m) => {
            let h = m
                .positions
                .iter()
                .fold(0u64, |acc, p| acc.wrapping_mul(31).wrapping_add(p.x.to_bits() ^ p.y.to_bits().rotate_left(7) ^ p.z.to_bits().rotate_left(13)));
            (m.triangles.len(), h)
        }
    }
}

#[test]
fn a_second_cut_changes_only_dirty_chunks() {
    let mut s = slot_stock(0.25);
    s.clear_dirty();
    assert!(dirty_chunks(&s).is_empty());
    let before: Vec<(usize, u64)> = (0..4usize)
        .flat_map(|cz| (0..4usize).flat_map(move |cy| (0..4usize).map(move |cx| (cx, cy, cz))))
        .map(|(cx, cy, cz)| fingerprint(chunk_mesh(&s, [cx, cy, cz])))
        .collect();
    // A small pocket far from the slot, near one corner.
    let tool = FlatTool::new(1.0, 20.0, Vec3::Z).expect("tool");
    let pocket = LinearFixedAxis { from: Vec3::new(26.0, 26.0, -6.0), to: Vec3::new(27.0, 26.0, -6.0), tool };
    s.apply(&pocket, 9).expect("cut");
    let dirty = dirty_chunks(&s);
    assert!(!dirty.is_empty());
    let after: Vec<(usize, u64)> = (0..4usize)
        .flat_map(|cz| (0..4usize).flat_map(move |cy| (0..4usize).map(move |cx| (cx, cy, cz))))
        .map(|(cx, cy, cz)| fingerprint(chunk_mesh(&s, [cx, cy, cz])))
        .collect();
    let mut idx = 0usize;
    for cz in 0..4usize {
        for cy in 0..4usize {
            for cx in 0..4usize {
                if before[idx] != after[idx] {
                    assert!(dirty.contains(&[cx, cy, cz]), "chunk {cx} {cy} {cz} changed but was not dirty");
                }
                idx += 1;
            }
        }
    }
    s.clear_dirty();
    assert!(dirty_chunks(&s).is_empty());
}

#[test]
fn mesh_volume_is_the_precise_number() {
    let s = slot_stock(0.25);
    let analytic = 30.0 * 30.0 * 10.0 - 4.0 * (2.0 * 2.5 * 14.0 + std::f64::consts::PI * 2.5 * 2.5);
    let err_mesh = (mesh_volume(&s) - analytic).abs();
    let err_dexel = (s.volume() - analytic).abs();
    eprintln!("analytic {analytic:.3}: mesh error {err_mesh:.4}, midpoint error {err_dexel:.4}");
    assert!(err_mesh < 6.0, "mesh error {err_mesh}");
}

fn odd_edges(m: &Mesh) -> Vec<((u32, u32), u32)> {
    let mut count: HashMap<(u32, u32), u32> = HashMap::new();
    for t in &m.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    count.into_iter().filter(|(_, c)| c % 2 != 0).take(5).collect()
}

#[test]
fn material_on_the_last_node_gives_a_closed_surface() {
    // The node at n is the last one the lattice has. Its edges leave the lattice, and must still close.
    let lat = NodeSet { n: 2, set: [[2i64, 2, 2]].into_iter().collect() };
    let m = weld(&extract(&lat));
    assert!(!m.triangles.is_empty());
    let odd = odd_edges(&m);
    assert!(odd.is_empty(), "edges with an odd count: {odd:?}");
}

#[test]
fn a_small_cut_in_a_large_stock_marks_only_nearby_chunks() {
    // A 130 mm stock at h = 0.5 has 81 chunks. A 1 mm pocket in one corner must mark only a few.
    let mut s = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(130.0, 130.0, 0.0), 0.5, 1 << 34).expect("stock");
    s.clear_dirty();
    let all: Vec<[usize; 3]> = (0..9usize).flat_map(|z| (0..9usize).flat_map(move |y| (0..9usize).map(move |x| [x, y, z]))).collect();
    let fingerprints = |s: &Stock| -> Vec<(usize, u64)> {
        all.iter()
            .map(|idx| match chunk_mesh(s, *idx) {
                None => (0, 0),
                Some(m) => {
                    let h = m.positions.iter().fold(0u64, |acc, p| {
                        acc.wrapping_mul(31).wrapping_add(p.x.to_bits() ^ p.y.to_bits().rotate_left(7) ^ p.z.to_bits().rotate_left(13))
                    });
                    (m.triangles.len(), h)
                }
            })
            .collect()
    };
    let before = fingerprints(&s);
    let tool = FlatTool::new(1.0, 3.0, Vec3::Z).expect("tool");
    let pocket = LinearFixedAxis { from: Vec3::new(119.0, 119.0, -6.0), to: Vec3::new(120.0, 119.0, -6.0), tool };
    s.apply(&pocket, 9).expect("cut");
    let dirty = dirty_chunks(&s);
    let after = fingerprints(&s);
    assert!(dirty.len() <= 9, "dirty chunks {dirty:?}");
    for (i, idx) in all.iter().enumerate() {
        if before[i] != after[i] {
            assert!(dirty.contains(idx), "chunk {idx:?} changed but was not dirty");
        }
    }
    assert!(before.iter().zip(&after).any(|(a, b)| a != b), "the cut changed some chunk");
}

#[test]
fn a_fresh_box_has_no_dirty_chunk_and_asking_for_untouched_chunks_is_instant() {
    // 300 x 200 x 50 mm at 0.1 mm: about 95 000 chunks, all still the uncut box.
    let s = Stock::new_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(300.0, 200.0, 0.0), 0.1, 1 << 32).expect("stock");
    assert!(dirty_chunks(&s).is_empty(), "{} dirty chunks on a fresh box", dirty_chunks(&s).len());
    let start = std::time::Instant::now();
    let meshed = (0..100usize).filter(|k| chunk_mesh(&s, [*k, 0, 0]).is_some()).count();
    let elapsed = start.elapsed();
    assert_eq!(meshed, 0, "no cut reached these chunks, so none is meshed");
    assert!(elapsed.as_millis() < 50, "100 untouched chunks took {elapsed:?}");
}

#[test]
fn a_small_cut_dirties_only_its_chunks_and_the_surface_stays_closed() {
    // A 130 x 130 x 10 mm stock at 0.5 mm, with one 5 mm slot 4 mm deep in its middle.
    let mut s = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(130.0, 130.0, 0.0), 0.5, 1 << 34).expect("stock");
    let tool = FlatTool::new(2.5, 20.0, Vec3::Z).expect("tool");
    s.apply(&LinearFixedAxis { from: Vec3::new(60.0, 65.0, -4.0), to: Vec3::new(70.0, 65.0, -4.0), tool }, 1).expect("cut");
    let dirty = dirty_chunks(&s);
    let total: i64 = chunk_counts(&s).iter().product();
    assert!(!dirty.is_empty() && (dirty.len() as i64) <= 9, "{} of {total} chunks dirty", dirty.len());
    assert_two_per_edge(&weld(&chunk_meshes(&s)));
}

#[test]
fn a_display_lod_mesh_has_far_fewer_triangles_than_the_full_surface() {
    // Step 4 samples every fourth node: one coarse chunk covers this whole small stock.
    let s = slot_stock(0.25);
    let full: usize = chunk_meshes(&s).iter().map(|m| m.triangles.len()).sum();
    let coarse = super::chunk_mesh_lod(&s, [0, 0, 0], 4).expect("the slot reaches this coarse chunk");
    assert!(!coarse.triangles.is_empty());
    assert!(coarse.triangles.len() * 4 < full, "coarse {} vs full {full} triangles", coarse.triangles.len());
    // Step 1 is the full-resolution chunk mesh.
    assert_eq!(super::chunk_mesh_lod(&s, [0, 0, 0], 1).map(|m| m.triangles.len()), chunk_mesh(&s, [0, 0, 0]).map(|m| m.triangles.len()));
    // A chunk with no mesh is the uncut box only where no cut reached it.
    let fresh = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(30.0, 30.0, 0.0), 0.25, 1 << 32).expect("stock");
    let cut = s.touched_chunks().first().copied().expect("the slot touched a chunk");
    assert!(super::display_reached(&s, cut, 1) && super::display_reached(&s, [0, 0, 0], 4));
    for step in [1, 2, 4, 8] {
        assert!(!super::display_reached(&fresh, [0, 0, 0], step), "step {step}: no cut reached the fresh box");
    }
}

/// The uncut box's faces, cut per display chunk, cover the box's whole surface at every step; a chunk
/// its own mesh covers leaves its faces out.
#[test]
fn box_faces_cover_the_uncut_box_at_every_step() {
    let s = Stock::new_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 6.0, 4.0), 0.5, 1 << 32).expect("stock");
    let surface = 2.0 * (10.0 * 6.0 + 6.0 * 4.0 + 10.0 * 4.0);
    for step in [1, 2, 4, 8] {
        let faces = super::box_faces(&s, step, |_| false);
        let area = faces.measure().area;
        assert!((area - surface).abs() < 1e-6, "step {step}: area {area}, box surface {surface}");
        let open = super::box_faces(&s, step, |_| true);
        assert!(open.triangles.is_empty(), "step {step}: covered chunks left faces");
    }
    assert_eq!(super::display_step(0), 1);
    assert_eq!(super::display_step(128), 1);
    assert_eq!(super::display_step(129), 2);
}

/// Distance from `p` to the axis-aligned rectangle (a plane piece, one axis degenerate) `lo..=hi`.
fn dist_to_rect(p: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> f64 {
    (0..3).map(|k| (lo[k] - p[k]).max(p[k] - hi[k]).max(0.0).powi(2)).sum::<f64>().sqrt()
}

#[test]
fn a_through_slot_keeps_sharp_floor_and_wall_edges() {
    // A 20 x 20 x 10 mm box with a 5 mm flat-bottomed slot through it along x, floor at z = -4. The
    // true surface is the rectangles below; every mesh vertex must lie on one of them.
    let h = 0.5;
    let mut s = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(20.0, 20.0, 0.0), h, 1 << 32).expect("stock");
    let tool = FlatTool::new(2.5, 20.0, Vec3::Z).expect("tool");
    s.apply(&LinearFixedAxis { from: Vec3::new(-5.0, 10.0, -4.0), to: Vec3::new(25.0, 10.0, -4.0), tool }, 1).expect("cut");
    let pieces: [([f64; 3], [f64; 3]); 14] = [
        ([0.0, 7.5, -4.0], [20.0, 12.5, -4.0]),
        ([0.0, 7.5, -4.0], [20.0, 7.5, 0.0]),
        ([0.0, 12.5, -4.0], [20.0, 12.5, 0.0]),
        ([0.0, 0.0, 0.0], [20.0, 7.5, 0.0]),
        ([0.0, 12.5, 0.0], [20.0, 20.0, 0.0]),
        ([0.0, 0.0, -10.0], [20.0, 20.0, -10.0]),
        ([0.0, 0.0, -10.0], [0.0, 7.5, 0.0]),
        ([0.0, 12.5, -10.0], [0.0, 20.0, 0.0]),
        ([0.0, 7.5, -10.0], [0.0, 12.5, -4.0]),
        ([20.0, 0.0, -10.0], [20.0, 7.5, 0.0]),
        ([20.0, 12.5, -10.0], [20.0, 20.0, 0.0]),
        ([20.0, 7.5, -10.0], [20.0, 12.5, -4.0]),
        ([0.0, 0.0, -10.0], [20.0, 0.0, 0.0]),
        ([0.0, 20.0, -10.0], [20.0, 20.0, 0.0]),
    ];
    let (count, worst) = worst_vertex_distance(&s, |q| pieces.iter().map(|(lo, hi)| dist_to_rect(q, *lo, *hi)).fold(f64::INFINITY, f64::min));
    eprintln!("slot vertices {count}: worst distance to the true surface {worst:.6} = {:.3} h", worst / h);
    assert!(count > 0);
    assert!(worst < 0.1 * h, "worst vertex {worst} from the true surface, h {h}");
}

/// The number of vertices of the cut stock's surface, and the largest distance from one to `surface`.
fn worst_vertex_distance(s: &Stock, surface: impl Fn([f64; 3]) -> f64) -> (usize, f64) {
    let mut count = 0usize;
    let mut worst = 0.0f64;
    for m in chunk_meshes(s) {
        for p in &m.positions {
            count += 1;
            worst = worst.max(surface((*p).into()));
        }
    }
    (count, worst)
}

#[test]
fn a_plunged_cylindrical_pocket_keeps_its_wall_and_floor() {
    // A flat 10 mm tool plunged straight down at the middle of a 30 x 30 x 10 mm box: a vertical
    // cylinder of radius 5 with a flat floor at z = -4. The wall is curved, so its crossings are
    // oblique to the lattice, which the slot test does not exercise. The reference surface is the
    // wall, the floor disc and the box faces, all exact here, so no vertex is excluded.
    let h = 0.5;
    let (cx, cy, r) = (15.0, 15.0, 5.0);
    let mut s = Stock::new_box(Vec3::new(0.0, 0.0, -10.0), Vec3::new(30.0, 30.0, 0.0), h, 1 << 32).expect("stock");
    let tool = FlatTool::new(r, 20.0, Vec3::Z).expect("tool");
    s.apply(&LinearFixedAxis { from: Vec3::new(cx, cy, 5.0), to: Vec3::new(cx, cy, -4.0), tool }, 1).expect("plunge");
    let box_faces: [([f64; 3], [f64; 3]); 6] = [
        ([0.0, 0.0, 0.0], [30.0, 30.0, 0.0]),
        ([0.0, 0.0, -10.0], [30.0, 30.0, -10.0]),
        ([0.0, 0.0, -10.0], [0.0, 30.0, 0.0]),
        ([30.0, 0.0, -10.0], [30.0, 30.0, 0.0]),
        ([0.0, 0.0, -10.0], [30.0, 0.0, 0.0]),
        ([0.0, 30.0, -10.0], [30.0, 30.0, 0.0]),
    ];
    let surface = |q: [f64; 3]| {
        let rho = ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt();
        // The floor disc: the nearest point of the disc is the radial projection, clamped to it.
        let floor = (rho - r).max(0.0).hypot(q[2] + 4.0);
        // The wall: the nearest point is at the same angle, with z clamped to the wall's -4..0.
        let dz = (-4.0 - q[2]).max(q[2] - 0.0).max(0.0);
        let wall = (rho - r).hypot(dz);
        box_faces.iter().map(|(lo, hi)| dist_to_rect(q, *lo, *hi)).fold(floor.min(wall), f64::min)
    };
    let (count, worst) = worst_vertex_distance(&s, surface);
    eprintln!("pocket vertices {count}: worst distance to the true surface {worst:.6} = {:.3} h", worst / h);
    assert!(count > 0);
    assert!(worst < 0.1 * h, "worst vertex {worst} from the true surface, h {h}");
}
