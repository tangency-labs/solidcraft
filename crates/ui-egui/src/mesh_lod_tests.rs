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
    assert!(lods.levels(&mesh, None).is_none(), "not made in the asking frame");
    let began = std::time::Instant::now();
    let levels = loop {
        if let Some(l) = lods.levels(&mesh, None) {
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
    assert!(Arc::ptr_eq(&levels, &lods.levels(&mesh, None).unwrap()), "made once");
    // Let go when no longer drawn.
    lods.retain(&HashSet::new());
    assert!(lods.levels(&mesh, None).is_none(), "made again when asked again");
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
            queue(&mut state, &lods.0, Arc::as_ptr(m) as usize, m, None);
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
        let levels = lods.levels(m, None).expect("made, all of them, though more were asked than workers");
        assert!(!levels.is_empty(), "{} triangles", m.triangles.len());
        assert!(levels.windows(2).all(|w| w[1].1 >= w[0].1), "errors grow with each level");
    }
    assert!(lods.levels(&single, None).expect("made").is_empty(), "a single triangle has no coarser copy");
}

#[test]
fn large_lod_jobs_wait_for_an_exclusive_worker() {
    let mut state = State::default();
    let small = work_cost(3_000_000, 1_000_000);
    let large = work_cost(57_000_000, 19_000_000);
    for (key, cost) in [small, small, large, small].into_iter().enumerate() {
        state.queue.push_back(Job { key, source: Arc::default(), indexed: None, cost });
    }

    let first = state.take_job().unwrap();
    let second = state.take_job().unwrap();
    assert_eq!((first.key, second.key), (0, 1));
    assert!(state.take_job().is_none());

    state.release(first.cost);
    assert!(state.take_job().is_none(), "a large job waits for remaining scratch to be released");
    state.release(second.cost);
    let exclusive = state.take_job().expect("large inputs still receive levels");
    assert_eq!(exclusive.key, 2);
    assert!(state.take_job().is_none(), "smaller jobs wait while the exclusive job is running");

    state.release(exclusive.cost);
    let last = state.take_job().unwrap();
    assert_eq!(last.key, 3);
    state.release(last.cost);
    assert_eq!((state.making, state.reserved), (0, 0));
}

/// Queue small test meshes at known costs without starting threads or allocating large inputs.
fn queued_jobs(state: &mut State, costs: [usize; 3]) -> [usize; 3] {
    costs.map(|cost| {
        let source = Arc::<Mesh>::default();
        let key = Arc::as_ptr(&source) as usize;
        state.by_mesh.insert(key, (source.clone(), None));
        state.queue.push_back(Job { key, source, indexed: None, cost });
        key
    })
}

#[test]
fn hiding_the_waiting_exclusive_job_starts_the_next_mesh() {
    let lods = MeshLods::default();
    let (running, hidden, ready) = {
        let mut state = lods.0.lock().unwrap();
        let [_, hidden, ready] = queued_jobs(&mut state, [1, WORK_BUDGET, 1]);
        let running = state.take_job().unwrap();
        assert!(state.take_job().is_none());
        (running, hidden, ready)
    };

    lods.retain(&HashSet::from([running.key, ready]));
    let mut state = lods.0.lock().unwrap();
    assert!(!state.by_mesh.contains_key(&hidden));
    assert!(state.queue.is_empty(), "the second worker takes the ready job before the first finishes");
    state.release(running.cost);
}

#[test]
fn a_failed_worker_start_leaves_jobs_for_an_active_worker() {
    let mut state = State::default();
    let [_, retry, waiting] = queued_jobs(&mut state, [1, 1, 1]);
    let running = state.take_job().unwrap();
    let failed = state.take_job().unwrap();
    state.failed_to_start(failed);

    assert_eq!((state.making, state.reserved), (1, running.cost));
    assert_eq!(state.generation, 0);
    assert!(state.by_mesh[&retry].1.is_none() && state.by_mesh[&waiting].1.is_none());
    assert_eq!(state.queue.iter().map(|job| job.key).collect::<Vec<_>>(), [retry, waiting]);

    state.release(running.cost);
    let failed = state.take_job().unwrap();
    state.failed_to_start(failed);
    assert_eq!((state.making, state.reserved), (0, 0));
    assert_eq!(state.generation, 1);
    assert!(state.queue.is_empty());
    for key in [retry, waiting] {
        assert!(state.by_mesh[&key].1.as_ref().unwrap().is_empty(), "without a worker the mesh draws in full");
    }
}
