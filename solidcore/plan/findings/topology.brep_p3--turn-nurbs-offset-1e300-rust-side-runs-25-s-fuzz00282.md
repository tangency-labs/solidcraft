# topology.brep_p3: a 1e300 `TurnNurbsOffset` kept the Rust side busy for 25 s where the FFI refuses the C++ (nightly fuzz00282) (2026-10-27)

- **Unit:** `topology.brep_p3` (`SmBrep::TurnToNURBS`, the `SmOffsetSurface` arm). PR #1358.
- **Inputs:** nightly fuzz case `topology.brep_p3/fuzz00282` of runs 40177686438 (2026-10-25)
  and 40298730423 (2026-10-26): fixture `nurbs_off_box_with_large_fillet_-0.05`
  (`box_with_large_fillet.smb`, `turn_nurbs_offset`, `editing = -1`) with the mutation
  `/offset = 1e+300`. Verdict both nights: `timeout`, `rust: no result after 20 s`.
- **Both sides:**
  - C++: not run. The FFI refuses the input before calling in:
    `invalid argument: geometry value beyond 1e150 (HANG-16)` (`solidcore-ffi`
    `topology_brep_p3::eval`, `MAX_GEOMETRY`): geometry beyond 1e150 overflows the squared
    distances of `SmCurve::GlobalCoincidenceChecker`, reached by the UV trim curve rebuild
    of `TurnToNURBS`, and upstream never returns.
  - Rust (`--digest-dir`, Rust side alone, Apple M-series): returns after **25.4 s** with
    status `0` and a full output record (every face surface a B-spline, type code 14033,
    and their natural UV domains); so not an endless loop, but far past the fuzz harness's
    20 s limit, and the CI runner is slower.
- **Where the time goes** (macOS `sample` of the running process, 6 s window, 3495
  samples on the main thread, all under `turn_to_nurbs_in`): `topology::brep::p2::
  replace_surface` -> `face::p3::create_uv_trim_curves_notified` -> `edgeuse::p3::
  create_uv_trim_curve` -> `intersect::surface_drop_lib::p1::drop_and_trim_curve` ->
  `drop_curve_trace` -> `surface_tracer::tracer_surface_cache` -> `TrimSrfCache::build_tree`
  -> `SurfaceCache::subdivide…` (`subdivide_node_split`, `patch_boxes`, `is_flat_cheap`,
  `calculate_bounding_box_of`, `net_constants` / `compute_net_constants`, `Surface::eval`,
  `basis_eval_into`). The approximated offset surface has control points of magnitude 1e300
  (the B-rep tolerance is 1e-6 scale), so the trimmed-surface cache that the curve drop
  builds for every edge of the face subdivides until its existing node and depth guards
  stop it. The offset approximation itself (`nurbs::srf_offset`, 24 refinements, 10 000
  samples) and the bounding box / extension steps are not where the time goes.
- **Cause:** a port faithful to upstream in a regime the FFI keeps the C++ out of (HANG-16),
  the class of issue #504 (Rust library loops on inputs the FFI refuses). The fuzz harness
  runs the Rust side on a guard-refused input on purpose, to find exactly this.
- **Fix (harness, PR #502's convention):** `topology_brep_p3::eval` is split into
  `validate` (every check it made before calling the shim) and the call; the runner's
  Rust side applies `validate` to the same input vector the C++ side gets, so the case is a
  harness error on both sides and the Rust port never runs it (`fuzz` verdict `rejected`).
  Regression: `crates/difftest/tests/fuzz_corpus/topology.brep_p3.json`
  (`fuzz00282_turn_nurbs_offset_1e300`, both sides `harness: invalid argument: geometry
  value beyond 1e150 (HANG-16)`) and the runner tests `huge_geometry_is_rejected` (Rust side
  added) and `huge_offset_is_refused_on_both_sides`. A gate fixture is not possible: a
  harness error always fails a case.
- **Library follow-up (not done here, #504):** the cost is the trimmed-surface cache
  subdivision of `drop_and_trim_curve` on a surface whose control points dwarf the
  tolerance; the subdivision is bounded (it returned), only slow. A work bound belongs to
  the owners of `surface.surface_cache_p*` / `surface.trim_srf_cache` (draft #818 covers the
  surface-cache subdivision slice of #504); the reproducer is this corpus case run through
  `turn_to_nurbs` directly.
