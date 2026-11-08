//! Integration tests for cooperative cancellation via [`CancelToken`] on [`Kernel`]
//! (`tangency-labs/solidcore#2136`).
//!
//! Verifies across all six kernel subsystems (boolean & imprint, fillet trace/solver &
//! corners, SSI marcher & start loop, stitch & heal, tessellation/caches/global solver,
//! and mass-property ODE integration):
//! 1. **Parity:** a [`Kernel`] configured with an active uncancelled [`CancelToken`]
//!    produces bit-identical geometry, topology, and integrated mass properties to
//!    `Kernel::default()`.
//! 2. **Determinism:** cancelling via [`CancelToken::cancel`] returns
//!    `Err(SolidError::Cancelled { .. })` (`sm_status() == None`, `!is_unported()`),
//!    never a partial `Ok` result and never a remapped upstream status (`SM_ERR`).
//! 3. **Mid-solve & worker-thread cancellation:** [`GlobalSolver::solve_branch`] stops
//!    deterministically mid-traversal when its shared [`CancelToken`] is signalled; a
//!    host thread cancels an in-flight worker-thread kernel job (and a multi-worker group
//!    sharing clones of one token); and [`CancelToken::reset`] / [`Kernel::clear_cancel_token`]
//!    restore normal execution.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::{
    Branch, GlobalSolve, GlobalSolver, PreferredSolution, SolutionPairType, SolutionRequested,
    SolveRequest, SolverOperation,
};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::solution_array::{Solution, SolutionArray};
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::intersect::adv_surface_intersector::{AdvSurfaceIntersector, NoSurfaceHandle};
use solidcore::intersect::surface_intersector::p5::do_intersection;
use solidcore::mass::integrator::properties::{
    brep_mass_properties, brep_volume, PropertyAttributes,
};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_torus};
use solidcore::query::tess::TessSrfCache;
use solidcore::surface::surface_cache::p1::{SurfaceCache, SurfaceCacheOptions};
use solidcore::topology::brep::p1::{edges, faces, tessellate_boundaries, vertices};
use solidcore::topology::brep::p5::{
    remove_topological_edges_and_vertices, stitch_and_orient, StitchAndOrientOptions,
};
use solidcore::topology::brep::{p2, p3};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, CancelToken, Kernel, SolidError};

fn copy_brep(k: &mut Kernel, b: &Brep, domain_tag: u64) -> Brep {
    let named = |entity, domain: u64| {
        let r = p3::topology_ref(0, entity);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
        }
    };
    let target_key = |entity| named(entity, domain_tag * 100_000_000);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target_key;
    p3::api_brep_copy(k, &mut env, Some(b), &|entity| named(entity, 0)).unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrepFingerprint {
    face_count: usize,
    edge_count: usize,
    vertex_count: usize,
    vertex_bits: Vec<[u64; 3]>,
    volume_bits: u64,
}

fn fingerprint_brep(b: &mut Brep) -> BrepFingerprint {
    let clean = Kernel::default();
    let f = faces(b, None).unwrap();
    let e = edges(b, None).unwrap();
    let v = vertices(b, None).unwrap();
    let mut vertex_bits: Vec<[u64; 3]> = v
        .iter()
        .map(|&vid| {
            let p = vertex::point(b, vid).unwrap();
            [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
        })
        .collect();
    vertex_bits.sort_unstable();

    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let vol = brep_volume(&clean, b, 1e-4, &attrs).unwrap();

    BrepFingerprint {
        face_count: f.len(),
        edge_count: e.len(),
        vertex_count: v.len(),
        vertex_bits,
        volume_bits: vol.to_bits(),
    }
}

fn assert_cancelled_err<T: core::fmt::Debug>(
    context: &str,
    res: solidcore::Result<T>,
) -> SolidError {
    let err = res.expect_err(context);
    assert!(
        err.is_cancelled(),
        "{context}: expected SolidError::Cancelled, got {err:?}"
    );
    assert!(!err.is_unported(), "{context}: Cancelled is not Unported");
    assert_eq!(
        err.sm_status(),
        None,
        "{context}: Cancelled has no SmStatus"
    );
    assert!(
        !err.operation().is_empty(),
        "{context}: Cancelled must report non-empty operation path"
    );
    err
}

fn verify_parity_and_cancellation<T: PartialEq + core::fmt::Debug>(
    name: &str,
    mut run: impl FnMut(&mut Kernel) -> solidcore::Result<T>,
) {
    // 1. Default kernel baseline (`cancel == None`).
    let mut k_default = Kernel::default();
    let baseline = run(&mut k_default).expect(name);

    // 2. Active uncancelled `CancelToken`: must match `Kernel::default()` bit-for-bit.
    let token = CancelToken::new();
    let mut k_token = Kernel::default();
    k_token.set_cancel_token(token.clone());
    let with_uncancelled_token = run(&mut k_token).expect(name);
    assert_eq!(
        with_uncancelled_token, baseline,
        "{name}: active uncancelled CancelToken drifted from default Kernel"
    );

    // 3. Signalled `CancelToken`: must return `SolidError::Cancelled` (never `Ok` or `SM_ERR`).
    token.cancel();
    let mut k_cancelled = Kernel::default();
    k_cancelled.set_cancel_token(token.clone());
    assert_cancelled_err(&format!("{name} (cancelled)"), run(&mut k_cancelled));

    // 4. `reset()` on the shared token restores bit-identical execution on a fresh kernel
    //    holding that token, and `clear_cancel_token()` restores `None`.
    token.reset();
    let mut k_reset = Kernel::default();
    k_reset.set_cancel_token(token);
    let after_reset = run(&mut k_reset).expect(name);
    assert_eq!(
        after_reset, baseline,
        "{name}: reset CancelToken drifted from default Kernel"
    );

    k_reset.clear_cancel_token();
    assert!(k_reset.cancel.is_none());
}

fn corner_edges_at(b: &Brep, corner: Point3) -> Vec<EdgeId> {
    let mut out: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&eid| {
            let (s, e) = edge::start_end_vertices(b, eid).unwrap();
            let ps = vertex::point(b, s.unwrap()).unwrap();
            let pe = vertex::point(b, e.unwrap()).unwrap();
            (ps - corner).length() < 1e-6 || (pe - corner).length() < 1e-6
        })
        .collect();
    assert_eq!(out.len(), 3);
    out.sort_by_key(|id| id.index());
    out
}

#[test]
fn kernel_cancel_token_send_sync_debug_and_resettable() {
    fn assert_send_debug<T: Send + core::fmt::Debug>() {}
    fn assert_token_traits<T: Send + Sync + Clone + core::fmt::Debug + Default>() {}
    assert_token_traits::<CancelToken>();
    assert_send_debug::<Kernel>();

    let mut k = Kernel::default();
    assert!(k.cancel.is_none());
    assert!(k.cancel_token().is_none());
    assert!(k.check_cancelled("test::noop").is_ok());

    let token = CancelToken::new();
    k.set_cancel_token(token.clone());
    assert!(k.cancel_token().is_some());
    assert!(!k.is_cancelled());
    assert!(format!("{k:?}").contains("CancelToken"));

    token.cancel();
    assert!(k.is_cancelled());
    let err = assert_cancelled_err("token fired", k.check_cancelled("test::token"));
    assert_eq!(
        err,
        SolidError::Cancelled {
            operation: "test::token"
        }
    );

    token.reset();
    assert!(!k.is_cancelled());
    assert!(k.check_cancelled("test::token").is_ok());

    k.clear_cancel_token();
    assert!(k.cancel.is_none());
    assert!(k.cancel_token().is_none());
}

#[test]
fn boolean_difference_parity_and_cancellation() {
    let mut setup = Kernel::default();
    let box_template =
        create_box(&mut setup, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let cyl_template = create_cylinder(&mut setup, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();

    verify_parity_and_cancellation("boolean_difference", |k| {
        let mut setup_copy = Kernel::default();
        let a = copy_brep(&mut setup_copy, &box_template, 1);
        let b = copy_brep(&mut setup_copy, &cyl_template, 2);
        let mut diff = boolean_difference(k, a, b)?;
        Ok(fingerprint_brep(&mut diff))
    });
}

#[test]
fn fillet_g2_traced_corner_parity_and_cancellation() {
    let mut setup = Kernel::default();
    let box_template =
        create_box(&mut setup, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 10.0).unwrap();

    verify_parity_and_cancellation("fillet_edges_g2_corner", |k| {
        let mut setup_copy = Kernel::default();
        let mut body = copy_brep(&mut setup_copy, &box_template, 1);
        let target_edges = corner_edges_at(&body, Point3::new(0.0, 0.0, 10.0));
        fillet_edges(k, &mut body, &target_edges, 1.5, 2, 2, 1.0)?;
        Ok(fingerprint_brep(&mut body))
    });
}

#[test]
fn ssi_marched_torus_cylinder_parity_and_cancellation() {
    let mut setup = Kernel::default();
    let torus_brep = create_torus(&mut setup, Point3::new(0.0, 0.0, 0.0), 5.0, 2.0).unwrap();
    let cyl_brep = create_cylinder(&mut setup, Point3::new(4.0, 0.0, -5.0), 2.2, 10.0).unwrap();

    let tf = faces(&torus_brep, None).unwrap()[0];
    let ts = torus_brep.face(tf).unwrap().surface.clone().unwrap();
    let td = ts.natural_uv_domain_with_nurb(&setup).unwrap();

    let cf = faces(&cyl_brep, None).unwrap()[0];
    let cs = cyl_brep.face(cf).unwrap().surface.clone().unwrap();
    let cd = cs.natural_uv_domain_with_nurb(&setup).unwrap();

    verify_parity_and_cancellation("ssi_do_intersection", |k| {
        let out = do_intersection(k, &ts, &td, &cs, &cd, [true, true], None, None)?;
        assert!(
            !out.curves_3d.is_empty(),
            "expected marched SSI curves between torus and cylinder"
        );
        let mut samples = Vec::new();
        for crv in &out.curves_3d {
            let ivl = crv.natural_interval();
            for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let p = crv.evaluate_point(&Kernel::default(), ivl.evaluate(frac))?;
                samples.push([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
            }
        }
        Ok((out.curves_3d.len(), samples))
    });
}

#[test]
fn stitch_and_heal_parity_and_cancellation() {
    let mut setup = Kernel::default();
    let mut unstitched_template =
        create_box(&mut setup, Point3::new(0.0, 0.0, 0.0), 10.0, 8.0, 6.0).unwrap();
    remove_topological_edges_and_vertices(&mut setup, &mut unstitched_template, None, None)
        .unwrap();

    verify_parity_and_cancellation("stitch_and_orient", |k| {
        let mut setup_copy = Kernel::default();
        let mut b = copy_brep(&mut setup_copy, &unstitched_template, 1);
        stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default())?;
        Ok(fingerprint_brep(&mut b))
    });

    let cyl_template = create_cylinder(&mut setup, Point3::new(0.0, 0.0, 0.0), 4.0, 10.0).unwrap();
    verify_parity_and_cancellation("api_heal_brep", |k| {
        let mut setup_copy = Kernel::default();
        let mut b = copy_brep(&mut setup_copy, &cyl_template, 2);
        api_heal_brep(k, Some(&mut b))?;
        Ok(fingerprint_brep(&mut b))
    });
}

struct MidSolveCancelProbe<'t> {
    inner: AdvSurfaceIntersector<'t, 't, NoSurfaceHandle>,
    token: CancelToken,
    cancel_on_branch: u64,
}

impl<'t> GlobalSolve<'t, NoSurfaceHandle> for MidSolveCancelProbe<'t> {
    fn solver(&self) -> &GlobalSolver<'t, NoSurfaceHandle> {
        self.inner.solver()
    }

    fn solver_mut(&mut self) -> &mut GlobalSolver<'t, NoSurfaceHandle> {
        self.inner.solver_mut()
    }

    fn branch_may_contain_answers(&mut self, branch: &Branch) -> solidcore::Result<bool> {
        if self.inner.solver().branches_visited >= self.cancel_on_branch {
            self.token.cancel();
        }
        self.inner.branch_may_contain_answers(branch)
    }

    fn are_ready_for_local_solve(&self, branch: &Branch) -> solidcore::Result<bool> {
        self.inner.are_ready_for_local_solve(branch)
    }

    fn find_best_subdivision_node(&self, branch: &Branch) -> solidcore::Result<(usize, bool)> {
        self.inner.find_best_subdivision_node(branch)
    }

    fn subdivide(&mut self, branch: &Branch) -> solidcore::Result<(Branch, Branch)> {
        self.inner.subdivide(branch)
    }

    fn local_solve(&mut self, branch: &Branch) -> solidcore::Result<bool> {
        self.inner.local_solve(branch)
    }

    fn identical_solutions(
        &self,
        sol1: &Solution<NoSurfaceHandle>,
        sol2: &Solution<NoSurfaceHandle>,
    ) -> solidcore::Result<(SolutionPairType, Option<PreferredSolution>)> {
        self.inner.identical_solutions(sol1, sol2)
    }
}

#[test]
fn tessellation_caches_and_global_solver_parity_and_cancellation() {
    let mut setup = Kernel::default();
    let torus_brep = create_torus(&mut setup, Point3::new(0.0, 0.0, 0.0), 6.0, 2.0).unwrap();
    let cyl_brep = create_cylinder(&mut setup, Point3::new(4.5, 0.0, -5.0), 2.0, 10.0).unwrap();

    let tf = faces(&torus_brep, None).unwrap()[0];
    let torus_surface = torus_brep.face(tf).unwrap().surface.clone().unwrap();
    let td = torus_surface.natural_uv_domain_with_nurb(&setup).unwrap();

    let cf = faces(&cyl_brep, None).unwrap()[0];
    let cyl_surface = cyl_brep.face(cf).unwrap().surface.clone().unwrap();
    let cd = cyl_surface.natural_uv_domain_with_nurb(&setup).unwrap();

    // 1. TessSrfCache + SurfaceCache subdivision + containment walk.
    verify_parity_and_cancellation("tess_srf_cache_build_tree", |k| {
        let opts = SurfaceCacheOptions {
            chord_height_tolerance: 0.05,
            ang_tol_rad: 0.2,
            ..SurfaceCacheOptions::default()
        };
        let mut cache = TessSrfCache::new(ObjectKey(1), torus_surface.clone(), None, None, opts);
        cache.build_tree(k)?;
        let tree = cache.base().tree().expect("built tree");
        let top = tree.top_node().expect("top node");
        let bb = tree.node(top)?.bbox;
        Ok((
            tree.nodes().len(),
            [
                bb.min().x.to_bits(),
                bb.min().y.to_bits(),
                bb.min().z.to_bits(),
                bb.max().x.to_bits(),
                bb.max().y.to_bits(),
                bb.max().z.to_bits(),
            ],
        ))
    });

    // 2. Boundary curve tessellation (`tessellate_boundaries` + `CurveCache::tessellate`).
    verify_parity_and_cancellation("tessellate_boundaries", |k| {
        let tess = tessellate_boundaries(k, &torus_brep, 2.0)?;
        assert!(tess.complete);
        let bits: Vec<[u64; 3]> = tess
            .points
            .iter()
            .map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
            .collect();
        Ok((tess.vertex_counts, bits))
    });

    // 3. GlobalSolver branch-and-bound (`AdvSurfaceIntersector::solve_it`).
    verify_parity_and_cancellation("global_solver_solve_it", |k| {
        let cache_opts = SurfaceCacheOptions {
            chord_height_tolerance: 0.05,
            ang_tol_rad: 0.2,
            ..SurfaceCacheOptions::default()
        };
        let mut caches = vec![
            SurfaceCache::<NoSurfaceHandle>::new(torus_surface.clone(), td, cache_opts),
            SurfaceCache::<NoSurfaceHandle>::new(cyl_surface.clone(), cd, cache_opts),
        ];
        let mut si = AdvSurfaceIntersector::new(k, &torus_surface, &td, &cyl_surface, &cd, false)?;
        si.set_surface_caches(k, &mut caches)?;
        let mut sols = SolutionArray::new();
        let req = SolveRequest {
            operation: SolverOperation::Intersect,
            solution_requested: SolutionRequested::from_code(1).unwrap(),
            tolerance_3d: 1e-3,
            best_answer_so_far_sq: 1e20,
            opt_vectors: &[],
        };
        si.solve_it(&req, &mut sols)?;
        assert!(!sols.as_slice().is_empty());
        let pts: Vec<(u64, [u64; 4])> = sols
            .as_slice()
            .iter()
            .map(|s| {
                (
                    s.start.solution_value.to_bits(),
                    [
                        s.start.parameters[0].to_bits(),
                        s.start.parameters[1].to_bits(),
                        s.start.parameters[2].to_bits(),
                        s.start.parameters[3].to_bits(),
                    ],
                )
            })
            .collect();
        Ok(pts)
    });

    // 4. Deterministic mid-traversal `CancelToken` flip inside `GlobalSolver::solve_branch`.
    let token = CancelToken::new();
    let mut k = Kernel::default();
    k.set_cancel_token(token.clone());
    let cache_opts = SurfaceCacheOptions {
        chord_height_tolerance: 0.005,
        ang_tol_rad: 0.05,
        ..SurfaceCacheOptions::default()
    };
    let mut caches = vec![
        SurfaceCache::<NoSurfaceHandle>::new(torus_surface.clone(), td, cache_opts),
        SurfaceCache::<NoSurfaceHandle>::new(cyl_surface.clone(), cd, cache_opts),
    ];
    caches[0].build_tree(&k, false).unwrap();
    caches[1].build_tree(&k, false).unwrap();
    let mut si =
        AdvSurfaceIntersector::new(&k, &torus_surface, &td, &cyl_surface, &cd, false).unwrap();
    si.set_surface_caches(&k, &mut caches).unwrap();
    let mut probe = MidSolveCancelProbe {
        inner: si,
        token,
        cancel_on_branch: 5,
    };
    let mut sols = SolutionArray::new();
    let req = SolveRequest {
        operation: SolverOperation::Intersect,
        solution_requested: SolutionRequested::All,
        tolerance_3d: 1e-3,
        best_answer_so_far_sq: 1e20,
        opt_vectors: &[],
    };
    let err = assert_cancelled_err("mid-solve branch cancel", probe.solve_it(&req, &mut sols));
    assert_eq!(err.operation(), "geom::global_solver::solve_branch");
    assert!(probe.solver().branches_visited >= 33);
}

#[test]
fn mass_properties_ode_integration_parity_and_cancellation() {
    let mut setup = Kernel::default();
    let centre = Point3::new(1.0, -1.0, 0.5);
    let torus_template = create_torus(&mut setup, centre, 10.0, 3.0).unwrap();

    verify_parity_and_cancellation("brep_mass_properties", |k| {
        let mut setup_copy = Kernel::default();
        let mut b = copy_brep(&mut setup_copy, &torus_template, 1);
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let m = brep_mass_properties(k, &mut b, 1e-3, 1.0, centre, &attrs)?;
        Ok([
            m.properties.area.to_bits(),
            m.properties.volume.to_bits(),
            m.inertia.x.to_bits(),
            m.inertia.y.to_bits(),
            m.inertia.z.to_bits(),
        ])
    });
}

#[test]
fn worker_thread_and_group_cancel_in_flight_via_cancel_token() {
    let mut setup = Kernel::default();
    let centre = Point3::new(0.0, 0.0, 0.0);
    let torus_template = create_torus(&mut setup, centre, 10.0, 3.0).unwrap();
    let box_template =
        create_box(&mut setup, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let cyl_template = create_cylinder(&mut setup, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();

    // 1. Single worker running a heavy NURBS ODE integration (`accuracy = 1e-8`),
    //    cancelled mid-flight from the host thread via its cloned `CancelToken`, then
    //    reused cleanly after `token.reset()`.
    let mut worker_brep = copy_brep(&mut setup, &torus_template, 1);
    let mut retry_brep = copy_brep(&mut setup, &torus_template, 2);

    let token = CancelToken::new();
    let started = Arc::new(AtomicBool::new(false));
    let token_worker = token.clone();
    let started_worker = Arc::clone(&started);

    let handle = thread::spawn(move || {
        let mut k = Kernel::default();
        k.set_cancel_token(token_worker);
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        started_worker.store(true, Ordering::Release);
        let cancelled = brep_mass_properties(&k, &mut worker_brep, 1e-8, 1.0, centre, &attrs);
        (k, cancelled)
    });

    while !started.load(Ordering::Acquire) {
        thread::yield_now();
    }
    thread::sleep(Duration::from_millis(20));
    token.cancel();

    let (worker_kernel, result) = handle.join().expect("worker thread must not panic");
    assert_cancelled_err("worker thread in-flight cancel", result);

    token.reset();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let ok = brep_mass_properties(&worker_kernel, &mut retry_brep, 1e-3, 1.0, centre, &attrs)
        .expect("reset token must allow subsequent operations on the same kernel");
    assert!(ok.properties.volume > 0.0);

    // 2. Group cancellation: one shared `CancelToken` cancels multiple worker kernels
    //    concurrently (boolean, fillet, and mass-property jobs).
    let group_token = CancelToken::new();
    let barrier = Arc::new(Barrier::new(4));

    let b1 = Arc::clone(&barrier);
    let t1 = group_token.clone();
    let box_w1 = copy_brep(&mut setup, &box_template, 10);
    let cyl_w1 = copy_brep(&mut setup, &cyl_template, 11);
    let w1 = thread::spawn(move || -> solidcore::Result<()> {
        let mut k = Kernel::default();
        k.set_cancel_token(t1);
        b1.wait();
        loop {
            let mut sc = Kernel::default();
            let a = copy_brep(&mut sc, &box_w1, 1);
            let b = copy_brep(&mut sc, &cyl_w1, 2);
            boolean_difference(&mut k, a, b)?;
        }
    });

    let b2 = Arc::clone(&barrier);
    let t2 = group_token.clone();
    let box_w2 = copy_brep(&mut setup, &box_template, 20);
    let w2 = thread::spawn(move || -> solidcore::Result<()> {
        let mut k = Kernel::default();
        k.set_cancel_token(t2);
        b2.wait();
        loop {
            let mut sc = Kernel::default();
            let mut body = copy_brep(&mut sc, &box_w2, 1);
            let target_edges = corner_edges_at(&body, Point3::new(0.0, 0.0, 10.0));
            fillet_edges(&mut k, &mut body, &target_edges, 1.5, 2, 2, 1.0)?;
        }
    });

    let b3 = Arc::clone(&barrier);
    let t3 = group_token.clone();
    let mut torus_w3 = copy_brep(&mut setup, &torus_template, 30);
    let w3 = thread::spawn(move || {
        let mut k = Kernel::default();
        k.set_cancel_token(t3);
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        b3.wait();
        brep_mass_properties(&k, &mut torus_w3, 1e-8, 1.0, centre, &attrs).map(|_| ())
    });

    barrier.wait();
    thread::sleep(Duration::from_millis(15));
    group_token.cancel();

    assert_cancelled_err("group worker 1 (boolean)", w1.join().unwrap());
    assert_cancelled_err("group worker 2 (fillet)", w2.join().unwrap());
    assert_cancelled_err("group worker 3 (mass properties)", w3.join().unwrap());
}
