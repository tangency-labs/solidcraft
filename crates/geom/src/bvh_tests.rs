use super::*;

/// A small deterministic generator (the tests need no crate for randomness).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
    fn vec(&mut self, r: f64) -> Vec3 {
        Vec3::new(self.range(-r, r), self.range(-r, r), self.range(-r, r))
    }
}

/// A grid of `n` × `n` squares of two triangles on z = 0, and `extra` random triangles above.
fn mesh(n: u32, extra: usize, rng: &mut Lcg) -> Mesh {
    let mut positions: Vec<Vec3> = (0..=n).flat_map(|j| (0..=n).map(move |i| Vec3::new(f64::from(i), f64::from(j), 0.0))).collect();
    let id = |i: u32, j: u32| j * (n + 1) + i;
    let mut triangles = Vec::new();
    for j in 0..n {
        for i in 0..n {
            triangles.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            triangles.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    for _ in 0..extra {
        let c = rng.vec(f64::from(n)) + Vec3::new(0.0, 0.0, f64::from(n));
        let k = positions.len() as u32;
        positions.extend([c + rng.vec(1.5), c + rng.vec(1.5), c + rng.vec(1.5)]);
        triangles.push([k, k + 1, k + 2]);
    }
    let edges =
        (0..n).map(|i| (0..=n).map(|j| Vec3::new(f64::from(i), f64::from(j), 0.0)).collect()).chain([vec![Vec3::new(0.5, 0.5, 2.0)]]).collect();
    Mesh { tri_face: vec![0; triangles.len()], positions, triangles, edges, ..Default::default() }
}

#[test]
fn indexed_raycast_answers_as_the_scan_does() {
    let mut rng = Lcg(7);
    let m = mesh(12, 400, &mut rng);
    let index = MeshIndex::new(&m);
    let mut hits = 0;
    for k in 0..4000 {
        // Every few rays go straight down through grid vertices and edges, where two or more
        // triangles tie.
        let (o, d) = if k % 4 == 0 {
            let p = Vec3::new(f64::from(k % 13), f64::from((k / 13) % 13) * 0.5, 50.0);
            (p, Vec3::new(0.0, 0.0, -1.0))
        } else {
            let o = rng.vec(40.0) + Vec3::new(6.0, 6.0, 0.0);
            let target = Vec3::new(rng.range(-1.0, 13.0), rng.range(-1.0, 13.0), rng.range(-2.0, 14.0));
            let Some(d) = (target - o).normalized() else { continue };
            (o, d)
        };
        let want = m.raycast(o, d);
        assert_eq!(index.raycast(&m, o, d), want, "ray {k} from {o:?} along {d:?}");
        assert_eq!(index.raycast_bounded(&m, o, d, &mut 1_000_000).unwrap(), want, "bounded ray {k} from {o:?} along {d:?}");
        hits += usize::from(want.is_some());
    }
    assert!(hits > 1000, "the rays should mostly hit ({hits})");
}

#[test]
fn indexed_raycast_of_another_mesh_falls_back_to_the_scan() {
    let mut rng = Lcg(3);
    let (a, b) = (mesh(2, 0, &mut rng), mesh(5, 0, &mut rng));
    let index = MeshIndex::new(&a);
    let (o, d) = (Vec3::new(4.5, 4.2, 9.0), Vec3::new(0.0, 0.0, -1.0));
    assert!(b.raycast(o, d).is_some());
    assert_eq!(index.raycast(&b, o, d), b.raycast(o, d));
    assert_eq!(index.raycast_bounded(&b, o, d, &mut 1_000_000).unwrap(), b.raycast(o, d));
}

#[test]
fn bounded_raycast_charges_node_boxes_and_triangles_and_never_returns_a_partial_hit() {
    let m = mesh(1, 0, &mut Lcg(1));
    let index = MeshIndex::new(&m);
    let (o, d) = (Vec3::new(0.5, 0.5, 2.0), -Vec3::Z);
    let expected = index.raycast(&m, o, d);
    assert_eq!(expected, Some((2.0, 0)), "the lower triangle wins a shared-edge tie");
    for mut budget in 0..3 {
        assert!(index.raycast_bounded(&m, o, d, &mut budget).is_err());
    }
    let mut remaining = 6;
    for _ in 0..2 {
        assert_eq!(index.raycast_bounded(&m, o, d, &mut remaining).unwrap(), expected);
    }
    assert_eq!(remaining, 0, "each query examines one root box and two triangles");
    assert!(index.raycast_bounded(&m, o, d, &mut remaining).is_err());
    assert_eq!(remaining, 0);
    let mut miss_budget = 1;
    assert_eq!(index.raycast_bounded(&m, Vec3::new(10.0, 10.0, 2.0), d, &mut miss_budget).unwrap(), None);
    assert_eq!(miss_budget, 0, "a missed root box still consumes work");
}

#[test]
fn bounded_raycast_preserves_spatial_pruning_and_the_mismatched_mesh_scan_budget() {
    let grid = mesh(32, 0, &mut Lcg(1));
    let index = MeshIndex::new(&grid);
    let (o, d) = (Vec3::new(0.25, 0.25, 10.0), -Vec3::Z);
    let mut remaining = 1_000_000;
    assert_eq!(index.raycast_bounded(&grid, o, d, &mut remaining).unwrap(), index.raycast(&grid, o, d));
    assert!(1_000_000 - remaining < grid.triangles.len(), "the traversal must prune instead of becoming a full scan");
    let other = mesh(1, 0, &mut Lcg(1));
    assert_eq!(index.raycast_bounded(&other, o, d, &mut 2).unwrap(), other.raycast(o, d));
    assert!(index.raycast_bounded(&other, o, d, &mut 1).is_err(), "the scan fallback must charge all triangles, even after finding a hit");
    let mut bad = other.clone();
    bad.triangles.push([u32::MAX; 3]);
    assert!(index.raycast_bounded(&bad, o, d, &mut 2).is_err(), "invalid triangle records still require scan work");
    assert_eq!(index.raycast_bounded(&bad, o, d, &mut 3).unwrap(), bad.raycast(o, d));
}

#[test]
fn bounded_raycast_handles_empty_indices_and_hostile_rays_without_budget_wraparound() {
    let empty = Mesh::default();
    let empty_index = MeshIndex::new(&empty);
    assert_eq!(empty_index.raycast_bounded(&empty, Vec3::ZERO, Vec3::Z, &mut 0).unwrap(), None);
    let m = mesh(1, 0, &mut Lcg(1));
    let index = MeshIndex::new(&m);
    for (o, d) in [
        (Vec3::new(f64::NAN, 0.0, 2.0), -Vec3::Z),
        (Vec3::new(f64::INFINITY, 0.0, 2.0), -Vec3::Z),
        (Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0)),
        (Vec3::ZERO, Vec3::new(f64::INFINITY, 0.0, 1.0)),
        (Vec3::ZERO, Vec3::ZERO),
    ] {
        let mut remaining = 0;
        assert_eq!(index.raycast_bounded(&m, o, d, &mut remaining).unwrap(), index.raycast(&m, o, d));
        assert_eq!(empty_index.raycast_bounded(&m, o, d, &mut remaining).unwrap(), m.raycast(o, d));
        assert_eq!(remaining, 0);
    }
    let invalid = Mesh { positions: vec![Vec3::ZERO], triangles: vec![[0, 5, 9]], ..Default::default() };
    let index = MeshIndex::new(&invalid);
    assert_eq!(index.raycast_bounded(&invalid, Vec3::Z, -Vec3::Z, &mut 0).unwrap(), index.raycast(&invalid, Vec3::Z, -Vec3::Z));
}

/// Distance from point `p` to the ray from `o` along unit `d` (from its start on), and how far along.
fn ray_point(o: Vec3, d: Vec3, p: Vec3) -> (f64, f64) {
    let t = (p - o).dot(d).max(0.0);
    ((o + d * t).dist(p), t)
}

#[test]
fn segments_near_the_ray_are_all_reported() {
    let mut rng = Lcg(11);
    let m = mesh(16, 0, &mut rng);
    let index = MeshIndex::new(&m);
    for k in 0..500 {
        let o = rng.vec(30.0) + Vec3::new(8.0, 8.0, 30.0);
        let target = Vec3::new(rng.range(0.0, 16.0), rng.range(0.0, 16.0), 0.0);
        let Some(d) = (target - o).normalized() else { continue };
        let (a, b) = if k % 2 == 0 { (0.3, 0.0) } else { (0.0, 0.01) };
        let mut got = std::collections::HashSet::new();
        index.segments_near(o, d, a, b, |e, s| {
            assert!(got.insert((e, s)), "segment {e}/{s} reported twice");
        });
        for (ei, e) in m.edges.iter().enumerate() {
            for (si, w) in e.windows(2).enumerate() {
                // Sampled along the segment: any point within reach must have its segment reported.
                for f in 0..=8 {
                    let p = w[0] + (w[1] - w[0]) * (f64::from(f) / 8.0);
                    let (dist, t) = ray_point(o, d, p);
                    if dist <= a + b * t {
                        assert!(got.contains(&(ei, si)), "ray {k}: segment {ei}/{si} is within reach but was not reported");
                    }
                }
            }
        }
        // The one-point edge stands as its own segment.
        let last = m.edges.len() - 1;
        let (dist, t) = ray_point(o, d, Vec3::new(0.5, 0.5, 2.0));
        if dist <= a + b * t {
            assert!(got.contains(&(last, 0)));
        }
    }
}

#[test]
fn hostile_boxes_never_panic() {
    let nan = Vec3::new(f64::NAN, 0.0, 0.0);
    let boxes = [Aabb3::EMPTY, Aabb3 { min: nan, max: nan }, Aabb3 { min: Vec3::ZERO, max: Vec3::new(1.0, 1.0, 1.0) }];
    let bvh = Bvh::new(&boxes);
    assert_eq!(bvh.nearest(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0), |i| Some(f64::from(i))), Some((2.0, 2)));
    assert_eq!(bvh.nearest(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0), |_| Some(1.0)), None);
    let empty = Bvh::new(&[]);
    assert!(empty.nearest(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), |_| Some(1.0)).is_none());
    empty.visit(|_| true, |_| panic!("an empty hierarchy has no items"));
    let m = Mesh { positions: vec![Vec3::ZERO], triangles: vec![[0, 5, 9]], ..Default::default() };
    assert!(MeshIndex::new(&m).raycast(&m, Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0)).is_none());
    MeshIndex::new(&m).segments_near(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), f64::NAN, 0.0, |_, _| panic!("no segments"));
}

#[test]
fn mesh_index_wire_round_trip_preserves_flat_trees_hits_and_segments() {
    let mut rng = Lcg(17);
    let mesh = mesh(8, 40, &mut rng);
    let original = MeshIndex::new(&mesh);
    let encoded = serde_json::to_vec(&original.to_wire()).unwrap();
    let wire: MeshIndexWire = serde_json::from_slice(&encoded).unwrap();
    let restored = MeshIndex::from_wire(wire, &mesh).unwrap();
    assert_eq!(serde_json::to_vec(&restored.to_wire()).unwrap(), encoded);
    for _ in 0..200 {
        let origin = rng.vec(20.0) + Vec3::Z * 20.0;
        let direction = (rng.vec(8.0) - origin).normalized().unwrap();
        let mut a = 1_000_000;
        let mut b = a;
        assert_eq!(restored.raycast_bounded(&mesh, origin, direction, &mut a), original.raycast_bounded(&mesh, origin, direction, &mut b));
        assert_eq!(a, b, "hydration must retain the exact traversal");
        let mut before = Vec::new();
        let mut after = Vec::new();
        original.segments_near(origin, direction, 0.3, 0.01, |e, s| before.push((e, s)));
        restored.segments_near(origin, direction, 0.3, 0.01, |e, s| after.push((e, s)));
        assert_eq!(before, after);
    }
    let empty = Mesh::default();
    assert!(MeshIndex::from_wire(MeshIndex::new(&empty).to_wire(), &empty).is_some());
}

#[test]
fn mesh_index_wire_rejects_malformed_graphs_coverage_bounds_and_dimensions() {
    let mut rng = Lcg(19);
    let mesh = mesh(4, 0, &mut rng);
    let wire = MeshIndex::new(&mesh).to_wire();
    for defect in 0..9 {
        let mut altered = wire.clone();
        match defect {
            0 => altered.triangle_tree.nodes[0].bounds.min.x = f64::NAN,
            1 => altered.triangle_tree.nodes[0].first = u32::MAX,
            2 => altered.triangle_tree.nodes[0].first = 0,
            3 => altered.triangle_tree.nodes.push(altered.triangle_tree.nodes[0]),
            4 => altered.triangle_tree.order[0] = altered.triangle_tree.order[1],
            5 => altered.triangle_tree.nodes[0].count = u32::MAX,
            6 => altered.segment_of[0] = (u32::MAX, 0),
            7 => altered.edge_lengths[0] += 1,
            _ => altered.triangle_tree.nodes[0].bounds.max = altered.triangle_tree.nodes[0].bounds.min,
        }
        assert!(MeshIndex::from_wire(altered, &mesh).is_none(), "defect {defect}");
    }
    let mut other = mesh.clone();
    other.positions.push(Vec3::ZERO);
    assert!(MeshIndex::from_wire(wire.clone(), &other).is_none());
    other = mesh.clone();
    other.triangles[0] = [u32::MAX; 3];
    assert!(MeshIndex::from_wire(wire.clone(), &other).is_none());
    other = mesh.clone();
    for point in &mut other.positions {
        *point += Vec3::X * 1000.0;
    }
    assert!(MeshIndex::from_wire(wire, &other).is_none(), "dimensions alone cannot qualify unrelated coordinates");
}

#[test]
fn mesh_index_wire_has_bounded_decode_and_depth() {
    use serde::de::value::{Error, SeqDeserializer};
    assert!(wire_array::<_, u8>(SeqDeserializer::<_, Error>::new(0u8..4), 3).is_err());
    assert!(wire_array::<_, u8>(SeqDeserializer::<_, Error>::new((0u8..4).filter(|_| true)), 3).is_err());
    assert_eq!(wire_array::<_, u8>(SeqDeserializer::<_, Error>::new(0u8..3), 3).unwrap(), [0, 1, 2]);
    let chain = |depth: usize| {
        let mut nodes = vec![Node { bounds: Aabb3 { min: Vec3::ZERO, max: Vec3::ZERO }, first: 0, count: 0 }; depth * 2 + 1];
        for level in 0..depth {
            nodes[level * 2].first = (level * 2 + 1) as u32;
            nodes[level * 2 + 1].first = level as u32;
            nodes[level * 2 + 1].count = 1;
        }
        nodes[depth * 2].first = depth as u32;
        nodes[depth * 2].count = 1;
        BvhWire { nodes, order: (0..=depth as u32).collect() }
    };
    assert!(chain(MAX_WIRE_DEPTH).hydrate(MAX_WIRE_DEPTH + 1, |_, _| true).is_some());
    assert!(chain(MAX_WIRE_DEPTH + 1).hydrate(MAX_WIRE_DEPTH + 2, |_, _| true).is_none());
    let bounds = Aabb3 { min: Vec3::ZERO, max: Vec3::ZERO };
    let shared = BvhWire {
        nodes: vec![
            Node { bounds, first: 1, count: 0 },
            Node { bounds, first: 3, count: 0 },
            Node { bounds, first: 3, count: 0 },
            Node { bounds, first: 0, count: 1 },
            Node { bounds, first: 1, count: 2 },
        ],
        order: vec![0, 1, 2],
    };
    assert!(shared.hydrate(3, |_, _| true).is_none());
}

#[test]
fn point_cloud_index_reports_every_point_within_cone_and_handles_hostile_inputs() {
    let mut rng = Lcg(42);
    let mut vals = Vec::with_capacity(600 * 3);
    for _ in 0..600 {
        vals.push(rng.range(-25.0, 25.0) as f32);
        vals.push(rng.range(-25.0, 25.0) as f32);
        vals.push(rng.range(-25.0, 25.0) as f32);
    }
    let index = PointCloudIndex::new(&vals);
    assert_eq!(index.point_count(), 600);
    for k in 0..200 {
        let o = rng.vec(40.0) + Vec3::new(0.0, 0.0, 60.0);
        let target = rng.vec(20.0);
        let Some(d) = (target - o).normalized() else { continue };
        let (a, b) = if k % 2 == 0 { (1.5, 0.0) } else { (0.1, 0.03) };
        let mut cap = 120.0;
        let mut visited = std::collections::HashSet::new();
        index.visit_near_ray(o, d, a, b, &mut cap, |i, _| {
            assert!(visited.insert(i), "point {i} visited twice");
        });
        for i in 0..600usize {
            let p = Vec3::new(f64::from(vals[3 * i]), f64::from(vals[3 * i + 1]), f64::from(vals[3 * i + 2]));
            let t = (p - o).dot(d);
            if !(0.0..=120.0).contains(&t) {
                continue;
            }
            let dist = ((p - o) - d * t).len();
            if dist <= a + b * t {
                assert!(visited.contains(&(i as u32)), "ray {k}: point {i} within cone was missed");
            }
        }
    }
    // Empty and non-finite inputs never panic.
    let empty = PointCloudIndex::new(&[]);
    let mut cap = 10.0;
    empty.visit_near_ray(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 1.0, 0.0, &mut cap, |_, _| panic!("no points"));
    let dirty = PointCloudIndex::new(&[f32::NAN, 0.0, 0.0, 1.0, 2.0, 3.0]);
    let mut seen = Vec::new();
    dirty.visit_near_ray(Vec3::new(1.0, 2.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 0.5, 0.0, &mut cap, |i, _| seen.push(i));
    assert_eq!(seen, vec![1]);
}
