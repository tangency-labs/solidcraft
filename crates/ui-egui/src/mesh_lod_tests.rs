use super::*;
use solidcraft_engine::geom::prelude::*;

/// A sphere mesh of about `2 n²` triangles, each corner its own point.
fn sphere(n: u32) -> Arc<Mesh> {
    use solidcore_record::Vec3;
    let at = |i: u32, j: u32| {
        let (t, p) = (std::f64::consts::PI * f64::from(i) / f64::from(n), std::f64::consts::TAU * f64::from(j) / f64::from(2 * n));
        let (st, sp, cp) =
            (if i == 0 || i == n { 0.0 } else { t.sin() }, if j == 2 * n { 0.0 } else { p.sin() }, if j == 2 * n { 1.0 } else { p.cos() });
        Vec3::new(50.0 * st * cp, 50.0 * st * sp, 50.0 * t.cos())
    };
    let mut m = Mesh::default();
    for i in 0..n {
        for j in 0..2 * n {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            for [x, y, z] in [[a, c, d], [a, b, c]] {
                let k = m.positions.len() as u32;
                let nrm = (y - x).cross(z - x).normalized().unwrap_or(Vec3::Z);
                m.positions.extend([x, y, z]);
                m.normals.extend([nrm; 3]);
                m.triangles.push([k, k + 1, k + 2]);
            }
        }
    }
    Arc::new(m)
}

/// Every mesh's levels are made once, on a worker, whatever its size: asked for, `None` comes
/// back at once and the work starts; when done, the generation moves on and the levels come back,
/// coarser and coarser, each with its error.
#[test]
fn a_meshs_levels_are_made_once_on_a_worker() {
    let lods = MeshLods::default();
    let mesh = sphere(110);
    let before = lods.generation();
    assert!(lods.levels(&mesh).is_none(), "not made in the asking frame");
    let began = std::time::Instant::now();
    let levels = loop {
        if let Some(l) = lods.levels(&mesh) {
            break l;
        }
        assert!(began.elapsed() < std::time::Duration::from_secs(60), "made on a worker");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // The worker stores the levels before it retires: wait for that too.
    while lods.busy() {
        assert!(began.elapsed() < std::time::Duration::from_secs(60), "the worker retires");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_ne!(lods.generation(), before);
    // A quarter, a sixteenth and a sixty-fourth.
    assert_eq!(levels.len(), 3, "{} triangles", mesh.triangles.len());
    let mut last = (mesh.triangles.len(), 0.0f32);
    for (tris, error) in levels.iter() {
        let n = tris.indices.len() / 3;
        assert!(n * 3 <= last.0, "{n} after {}", last.0);
        assert!(*error >= last.1 && *error < 6.0, "error {error}");
        assert!(tris.flat);
        last = (n, *error);
    }
    assert!(Arc::ptr_eq(&levels, &lods.levels(&mesh).unwrap()), "made once");
    // Let go when no longer drawn.
    lods.retain(&HashSet::new());
    assert!(lods.levels(&mesh).is_none(), "made again when asked again");
}

/// Small meshes get levels too, coarser than a level of a large mesh went before, each with its
/// measured error (which decides where it is drawn); a mesh that cannot simplify gets none.
/// Requests made while the workers are busy wait their turn, and the viewport is told once per
/// worker when the queue is empty, not once per mesh.
#[test]
fn every_mesh_gets_levels_whatever_its_size() {
    let lods = MeshLods::default();
    let small: Vec<Arc<Mesh>> = (0..5).map(|k| sphere(8 + k)).collect();
    // One triangle: nothing to simplify.
    let single = Arc::new(Mesh {
        positions: vec![solidcore_record::Vec3::ZERO, solidcore_record::Vec3::X, solidcore_record::Vec3::Y],
        normals: vec![solidcore_record::Vec3::Z; 3],
        triangles: vec![[0, 1, 2]],
        ..Default::default()
    });
    let before = lods.generation();
    // Queued under one lock, so no worker starts on the queue before it is full and none retires
    // early: each worker retires once, when the queue is empty.
    {
        let mut state = lods.0.lock().unwrap();
        for m in small.iter().chain([&single]) {
            queue(&mut state, &lods.0, Arc::as_ptr(m) as usize, m);
        }
    }
    let began = std::time::Instant::now();
    while lods.busy() {
        assert!(began.elapsed() < std::time::Duration::from_secs(60), "made on the workers");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let after = lods.generation();
    assert!(after > before && after - before <= 2, "told once per worker, not per mesh");
    for m in &small {
        let levels = lods.levels(m).expect("made, all of them, though more were asked than workers");
        assert!(!levels.is_empty(), "{} triangles", m.triangles.len());
        assert!(levels.windows(2).all(|w| w[1].1 >= w[0].1), "errors grow with each level");
    }
    assert!(lods.levels(&single).expect("made").is_empty(), "a single triangle has no coarser copy");
}
