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
fn contribution_indexed_and_scanned_picking_skip_the_removed_front_surface() {
    let m = Mesh {
        positions: vec![
            Vec3::new(-1.0, -1.0, 1.0),
            Vec3::new(1.0, -1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2], [3, 4, 5]],
        tri_face: vec![0, 1],
        ..Default::default()
    };
    let index = MeshIndex::new(&m);
    let (o, d) = (Vec3::new(0.0, 0.0, 2.0), -Vec3::Z);
    assert_eq!(m.raycast(o, d), Some((1.0, 0)));
    let kept = |p: Vec3| p.z < 0.5;
    assert_eq!(m.raycast_filtered(o, d, kept), Some((2.0, 1)));
    assert_eq!(index.raycast_filtered(&m, o, d, kept), Some((2.0, 1)));
    assert_eq!(index.raycast_filtered(&m, o, d, |_| false), None);
}
