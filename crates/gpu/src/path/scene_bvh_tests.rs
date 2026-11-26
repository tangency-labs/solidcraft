use super::{MAX_BVH_DEPTH, Node, PARALLEL_BIN_THRESHOLD, PARALLEL_BVH_THRESHOLD, PreparedKey, PreparedSceneCache, Triangle, build, build_bvh};
use solidcore_record::Vec3;
use solidcraft_geom::{Mesh, MeshGeom};
use solidcraft_render::{
    FaceAppearance, ProductMesh, ProductScene,
    glass_model::{Dispersion, GlassModel, ThinFilm},
    job::CancelToken,
    transmission::{Transmission, subsurface::Subsurface},
};
use solidcraft_viewport::{clear_coat::ClearCoat, surface::OpaqueSurface};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Triangle centres spread over 60 decades: a sixteen-bin SAH split on the
// centre range peels only the few largest triangles off at each level, so an
// unbounded build is far deeper than the shader's traversal stack allows.
fn decades(count: usize) -> Vec<Triangle> {
    let surface = OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 0.5, 1.5).unwrap();
    (0..count)
        .map(|i| {
            let x = 10f64.powf(-30.0 + 60.0 * i as f64 / count as f64);
            Triangle {
                p: [Vec3::new(x, 0., 0.), Vec3::new(x * 1.5, 0., 0.), Vec3::new(x, x, x)],
                indices: [0, 1, 2].map(|c| (3 * i + c) as u32),
                surface,
                transmission: None,
                medium: 0,
                scalar_face: false,
            }
        })
        .collect()
}

fn sphere_triangles(rows: u32, columns: u32) -> Vec<Triangle> {
    let surface = OpaqueSurface::new(Vec3::new(0.8, 0.4, 0.2), 0.25, 0.4, 1.5).unwrap();
    let mut positions = Vec::with_capacity(((rows + 1) * (columns + 1)) as usize);
    for i in 0..=rows {
        let theta = f64::from(i) * std::f64::consts::PI / f64::from(rows);
        for j in 0..=columns {
            let phi = f64::from(j) * std::f64::consts::TAU / f64::from(columns);
            positions.push(Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()));
        }
    }
    let mut triangles = Vec::with_capacity((rows * columns * 2) as usize);
    for i in 0..rows {
        for j in 0..columns {
            let a = i * (columns + 1) + j;
            let b = a + columns + 1;
            for idx in [[a, b, a + 1], [a + 1, b, b + 1]] {
                let p = idx.map(|k| positions[k as usize]);
                triangles.push(Triangle { p, indices: idx, surface, transmission: None, medium: 1, scalar_face: false });
            }
        }
    }
    triangles
}

fn assert_bvh_bit_identical(seq_nodes: &[Node], seq_tris: &[Triangle], par_nodes: &[Node], par_tris: &[Triangle]) {
    assert_eq!(par_nodes.len(), seq_nodes.len(), "node count mismatch");
    for (i, (a, b)) in seq_nodes.iter().zip(par_nodes).enumerate() {
        assert_eq!(a.lo.map(f32::to_bits), b.lo.map(f32::to_bits), "node {i} lo mismatch");
        assert_eq!(a.hi.map(f32::to_bits), b.hi.map(f32::to_bits), "node {i} hi mismatch");
        assert_eq!(a.first, b.first, "node {i} first mismatch");
        assert_eq!(a.count, b.count, "node {i} count mismatch");
    }
    assert_eq!(par_tris.len(), seq_tris.len(), "triangle count mismatch");
    for (i, (a, b)) in seq_tris.iter().zip(par_tris).enumerate() {
        assert_eq!(a.indices, b.indices, "triangle {i} indices mismatch");
        assert_eq!(a.medium, b.medium, "triangle {i} medium mismatch");
        assert_eq!(a.scalar_face, b.scalar_face, "triangle {i} scalar_face mismatch");
        for (pa, pb) in a.p.iter().zip(&b.p) {
            assert_eq!(
                [pa.x.to_bits(), pa.y.to_bits(), pa.z.to_bits()],
                [pb.x.to_bits(), pb.y.to_bits(), pb.z.to_bits()],
                "triangle {i} vertex mismatch"
            );
        }
    }
}

fn clone_triangles(src: &[Triangle]) -> Vec<Triangle> {
    src.iter()
        .map(|t| Triangle {
            p: t.p,
            indices: t.indices,
            surface: t.surface,
            transmission: t.transmission,
            medium: t.medium,
            scalar_face: t.scalar_face,
        })
        .collect()
}

#[test]
fn sah_tree_respects_the_shader_stack_depth_and_covers_every_triangle_once() {
    let count = 2048;
    let mut triangles = decades(count);
    let mut nodes = vec![Node::default()];
    build(&mut triangles, 0, &mut nodes, 0, 0, &CancelToken::default()).unwrap();
    assert!(nodes.len() < 2 * count, "{} nodes", nodes.len());
    let mut covered = vec![0; count];
    let mut deepest = 0;
    let mut pending = vec![(0usize, 0usize)];
    while let Some((index, depth)) = pending.pop() {
        let node = &nodes[index];
        let (first, size) = (node.first as usize, node.count as usize);
        deepest = deepest.max(depth);
        if size == 0 {
            for child in [first, first + 1] {
                let inner = &nodes[child];
                for axis in 0..3 {
                    assert!(node.lo[axis] <= inner.lo[axis] && inner.hi[axis] <= node.hi[axis], "child box escapes its parent");
                }
                pending.push((child, depth + 1));
            }
            continue;
        }
        assert!(size <= 4);
        for (slot, triangle) in covered[first..first + size].iter_mut().zip(&triangles[first..first + size]) {
            *slot += 1;
            for p in triangle.p {
                for (axis, value) in [p.x as f32, p.y as f32, p.z as f32].into_iter().enumerate() {
                    assert!(node.lo[axis] <= value && value <= node.hi[axis], "triangle escapes its leaf");
                }
            }
        }
    }
    assert!(covered.iter().all(|&c| c == 1), "{covered:?}");
    assert!(deepest <= MAX_BVH_DEPTH, "depth {deepest}");
    // Covering 2048 triangles in leaves of four needs at least nine levels.
    assert!(deepest >= 9, "depth {deepest}");
}

#[test]
fn parallel_sah_bvh_is_bit_identical_to_sequential_build() {
    // 96x192x2 = 36,864 triangles exercises both PARALLEL_BVH_THRESHOLD and PARALLEL_BIN_THRESHOLD.
    let mesh = sphere_triangles(96, 192);
    assert!(mesh.len() >= PARALLEL_BIN_THRESHOLD && mesh.len() >= PARALLEL_BVH_THRESHOLD);
    let mut seq_tris = clone_triangles(&mesh);
    let mut par_tris = mesh;
    let cancel = CancelToken::default();
    let mut seq_nodes = Vec::with_capacity(seq_tris.len() * 2);
    seq_nodes.push(Node::default());
    build(&mut seq_tris, 0, &mut seq_nodes, 0, 0, &cancel).unwrap();
    let par_nodes = build_bvh(&mut par_tris, &cancel).unwrap();
    assert_bvh_bit_identical(&seq_nodes, &seq_tris, &par_nodes, &par_tris);

    // Pathological 60-decade distribution exercises both SAH splits and the depth-capped median fallback.
    let extreme = decades(20_000);
    let mut seq_extreme = clone_triangles(&extreme);
    let mut par_extreme = extreme;
    let mut seq_extreme_nodes = Vec::with_capacity(seq_extreme.len() * 2);
    seq_extreme_nodes.push(Node::default());
    build(&mut seq_extreme, 0, &mut seq_extreme_nodes, 0, 0, &cancel).unwrap();
    let par_extreme_nodes = build_bvh(&mut par_extreme, &cancel).unwrap();
    assert_bvh_bit_identical(&seq_extreme_nodes, &seq_extreme, &par_extreme_nodes, &par_extreme);

    // Cancellation must abort parallel BVH builds promptly.
    let cancelled = CancelToken::default();
    cancelled.cancel();
    assert!(build_bvh(&mut par_extreme, &cancelled).is_err());
}

fn sample_scene(rows: u32, columns: u32) -> ProductScene {
    let mut mesh = Mesh::default();
    for i in 0..=rows {
        let theta = f64::from(i) * std::f64::consts::PI / f64::from(rows);
        for j in 0..=columns {
            let phi = f64::from(j) * std::f64::consts::TAU / f64::from(columns);
            let n = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
            mesh.positions.push(n);
            mesh.normals.push(n);
        }
    }
    for i in 0..rows {
        for j in 0..columns {
            let a = i * (columns + 1) + j;
            let b = a + columns + 1;
            mesh.triangles.extend([[a, b, a + 1], [a + 1, b, b + 1]]);
            mesh.tri_face.extend([0, 0]);
        }
    }
    let bounds = mesh.bounds();
    ProductScene {
        meshes: vec![ProductMesh {
            name: "fixture sphere".into(),
            mesh: Arc::new(mesh),
            color: [200, 150, 100],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.8, 0.4, 0.2), 0.1, 0.35, 1.5).unwrap()),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds,
    }
}

struct TrackedPayload {
    id: u32,
    live: Arc<AtomicUsize>,
}

impl TrackedPayload {
    fn new(id: u32, live: Arc<AtomicUsize>) -> Self {
        live.fetch_add(1, Ordering::AcqRel);
        Self { id, live }
    }
}

impl Clone for TrackedPayload {
    fn clone(&self) -> Self {
        self.live.fetch_add(1, Ordering::AcqRel);
        Self { id: self.id, live: self.live.clone() }
    }
}

impl Drop for TrackedPayload {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
    }
}

#[test]
fn prepared_scene_cache_key_hit_miss_eviction_and_clear() {
    let cancel = CancelToken::default();
    // 64x144 > 8192 vertices/triangles exercises parallel chunked SHA-256 hashing too.
    let base = sample_scene(64, 144);
    let origin = Vec3::new(1.0, 2.0, 3.0);
    let key_a = PreparedKey::from_scene(&base, origin, false, &cancel).unwrap();
    let key_a_again = PreparedKey::from_scene(&base.clone(), origin, false, &cancel).unwrap();
    assert_eq!(key_a, key_a_again, "identical scene, origin, and nested flag must match");

    // Changing rebase origin or nested_media invalidates the key.
    let key_shifted_origin = PreparedKey::from_scene(&base, origin + Vec3::new(0.0, 0.0, 1e-6), false, &cancel).unwrap();
    assert_ne!(key_a, key_shifted_origin);
    let key_nested = PreparedKey::from_scene(&base, origin, true, &cancel).unwrap();
    assert_ne!(key_a, key_nested);

    // Every material, optical, naming, and geometry field read by flatten_with_media must invalidate the key.
    let mut moved_vertex = base.clone();
    Arc::make_mut(&mut moved_vertex.meshes[0].mesh).positions[42].x += 1e-4;
    assert_ne!(key_a, PreparedKey::from_scene(&moved_vertex, origin, false, &cancel).unwrap());

    let mut moved_normal = base.clone();
    Arc::make_mut(&mut moved_normal.meshes[0].mesh).normals[42].z -= 1e-4;
    assert_ne!(key_a, PreparedKey::from_scene(&moved_normal, origin, false, &cancel).unwrap());

    let mut flipped_tri = base.clone();
    Arc::make_mut(&mut flipped_tri.meshes[0].mesh).triangles[10].swap(1, 2);
    assert_ne!(key_a, PreparedKey::from_scene(&flipped_tri, origin, false, &cancel).unwrap());

    let mut changed_face_id = base.clone();
    Arc::make_mut(&mut changed_face_id.meshes[0].mesh).tri_face[10] = 7;
    assert_ne!(key_a, PreparedKey::from_scene(&changed_face_id, origin, false, &cancel).unwrap());

    let mut stage_named = base.clone();
    stage_named.meshes[0].name = "__render_stage__/floor".into();
    assert_ne!(key_a, PreparedKey::from_scene(&stage_named, origin, false, &cancel).unwrap());

    let mut coated = base.clone();
    coated.meshes[0].surface = Some(coated.meshes[0].surface.unwrap().with_clear_coat(ClearCoat::new(0.5, 0.2, 1.5).unwrap()));
    assert_ne!(key_a, PreparedKey::from_scene(&coated, origin, false, &cancel).unwrap());

    let mut glass = base.clone();
    glass.meshes[0].transmission = Some(
        Transmission::new(Vec3::new(0.1, 0.2, 0.3), None)
            .unwrap()
            .with_shadow_transmission(false)
            .with_model(GlassModel {
                roughness: 0.25,
                dispersion: Some(Dispersion { abbe_number: 55.0 }),
                thin_film: Some(ThinFilm { ior: 1.38, thickness_nm: 300.0 }),
            })
            .unwrap()
            .with_subsurface(Subsurface { scattering_per_mm: 0.5, anisotropy: 0.2 })
            .unwrap(),
    );
    let key_glass = PreparedKey::from_scene(&glass, origin, false, &cancel).unwrap();
    assert_ne!(key_a, key_glass);

    let mut faced = base.clone();
    faced.meshes[0].faces.push(FaceAppearance { face: 0, color: [10, 20, 30], opacity: 1.0, surface: None, transmission: None });
    assert_ne!(key_a, PreparedKey::from_scene(&faced, origin, false, &cancel).unwrap());

    let cancelled = CancelToken::default();
    cancelled.cancel();
    assert!(PreparedKey::from_scene(&base, origin, false, &cancelled).is_err());

    // Single-entry cache lifecycle: peek, hit, immediate eviction on mismatch, and clear.
    let live_a = Arc::new(AtomicUsize::new(0));
    let live_b = Arc::new(AtomicUsize::new(0));
    let mut cache = PreparedSceneCache::default();
    assert!(cache.is_empty());

    assert!(cache.replace(key_a, TrackedPayload::new(1, live_a.clone())).is_none());
    assert_eq!(live_a.load(Ordering::Acquire), 1);

    // Read-only peek hits on match and preserves entry on miss (for preflight).
    assert_eq!(cache.peek(&key_a).unwrap().id, 1);
    assert!(cache.peek(&key_glass).is_none());
    assert_eq!(live_a.load(Ordering::Acquire), 1);

    // Execution lookup hits and clones without evicting.
    let (hit, evicted) = cache.lookup_or_take_mismatch(&key_a);
    assert!(evicted.is_none());
    assert_eq!(hit.as_ref().unwrap().id, 1);
    drop(hit);
    assert_eq!(live_a.load(Ordering::Acquire), 1);

    // Execution lookup on mismatch immediately evicts the previous entry.
    let (hit, evicted) = cache.lookup_or_take_mismatch(&key_glass);
    assert!(hit.is_none());
    assert_eq!(evicted.as_ref().unwrap().id, 1);
    assert!(cache.is_empty());
    drop(evicted);
    assert_eq!(live_a.load(Ordering::Acquire), 0);

    // Insert replacement and verify clear (device loss / error) drops immediately.
    assert!(cache.replace(key_glass, TrackedPayload::new(2, live_b.clone())).is_none());
    assert_eq!(live_b.load(Ordering::Acquire), 1);
    let cleared = cache.take();
    assert!(cache.is_empty());
    drop(cleared);
    assert_eq!(live_b.load(Ordering::Acquire), 0);
}

#[test]
#[ignore = "manual sequential vs parallel 16-bin SAH BVH benchmark"]
fn bench_wt94_bvh_parallel_build() {
    // 256x256x2 = 131,072 triangles: representative mid-size CAD assembly mesh.
    let source = sphere_triangles(256, 256);
    let cancel = CancelToken::default();

    let mut seq_tris = clone_triangles(&source);
    let seq_start = std::time::Instant::now();
    let mut seq_nodes = Vec::with_capacity(seq_tris.len() * 2);
    seq_nodes.push(Node::default());
    build(&mut seq_tris, 0, &mut seq_nodes, 0, 0, &cancel).unwrap();
    let seq_ms = seq_start.elapsed().as_secs_f64() * 1000.0;

    let mut par_tris = source;
    let par_start = std::time::Instant::now();
    let par_nodes = build_bvh(&mut par_tris, &cancel).unwrap();
    let par_ms = par_start.elapsed().as_secs_f64() * 1000.0;

    assert_bvh_bit_identical(&seq_nodes, &seq_tris, &par_nodes, &par_tris);
    eprintln!(
        "BVH_BENCH triangles={} nodes={} seq_ms={:.3} par_ms={:.3} speedup={:.2}x",
        par_tris.len(),
        par_nodes.len(),
        seq_ms,
        par_ms,
        seq_ms / par_ms.max(1e-6),
    );
}
