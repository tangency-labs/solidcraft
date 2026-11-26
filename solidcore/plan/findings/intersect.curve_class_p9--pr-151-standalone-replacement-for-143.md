# intersect.curve_class_p9 (PR #151; standalone replacement for #143)

- **Scope.** Ports `IntersectEdges` and `IntersectTSBound`, their native shim,
  full-record runner and fixtures. Part 1 gains only a single-interval constructor
  and its inline cross-hatch setter. No open dependency PR is bundled. Official
  main `b2ebc087` includes the separately landed parts 5, 6 and 7; part 9 now calls
  part 5's real `insert_solutions` rather than retaining its old stub.
- **Pole search review fix.** `IntersectTSBound` calls `find_uv_nodes_built` with
  `false`, matching the default `FALSE` in `SmSurfaceCache.h:723` at upstream's
  two-argument call (`SmCurveClass.cpp:16225`). The v-split cache recipe stores a
  collapsed pole's valid UV representative in the lower leaf while a curve touches
  the pole in the upper leaf. The lower leaf must not be visited. Generated cases
  now include collapsed surfaces, both split directions and this disjoint-pole path.
- **Query integration review fix.** `CurveOnClassifyObjects` extends `ClassGeometry`
  and requires vertex, edge, vertexuse, edgeuse, edge-curve, trim-curve and face-surface
  handle mappings. This extends the landed part-5 interface; duplicate curve/surface
  resolvers and the self-deferred `query.face_p2` defaults are removed. Concrete
  adapters walk upward loopuses in order, resolve stored curves/tolerances, use the
  existing vertexuse UV computation and edgeuse gap calculation, and call both part-9
  methods. Ordinary `ComputeUVPoint` failures preserve its initialized undefined
  UV as native's ignored status does; unported errors still propagate.
  One private B-rep snapshot retains lazy cache updates only during view
  construction; numerical results are preserved, cache-only updates are discarded.
  Valid cached traversal uses cached UVs without invoking unrelated UV projections.
  Tests exercise both methods with nonempty topology and resolve real edge/trim handles.
  The query's earlier `ReSet` still defers on part 2. Uncached edge-loop UV evaluation
  reaches the genuine `topology.edgeuse_p3` `NormalizedEvaluate` dependency;
  `GetEdges` remains the existing `topology.face_p4` dependency. End-to-end query
  classification is not certified.
- **Shim cache runs with the face point test off.** The p9 shim attaches `P9Trim` with
  `m_bPointTestEnabled = FALSE` when it computes cache vertex UVs (matches the Rust cache default and the
  still-`UNPORTED(query.face_p2)` face-owned branch of `GlobalPointSolve`, `surface/surface/p4.rs`). The
  `query.face_p2` porter should revisit the `fuzz_*` fixtures in `fixtures/intersect.curve_class_p9/` when the
  switch lands in Rust.
- **Oracle guards review fix.** Removed numeric caps on zone tolerance, scale,
  source/edge parameter domains, UV position and vertex gap. Source point intervals
  are accepted. Counts/lengths, finite inputs and constructed coordinates, flag and
  dimension validity, nondegenerate line recipes, ordered intervals and single-owner
  shared-curve rules remain checked on both sides. Fixed fixtures cover zone 5,
  scale 1e8, extended and point intervals, an outside-square vertex and gap 2.
  Native constructor/recipe `SC_ERR_ARG` after those guards is recorded as a rejected
  fixture, not a parity failure or pass; exceptions and operation statuses are not
  reclassified. Collapsed rectangles and tiny zero-zone rectangles are included to
  exercise this distinction. They do not certify those unsupported topology recipes.
- **Faithfulness and coverage.** Real native lines, planes, bilinear/collapsed
  patches, wire edges, isolated vertex loops and rectangles are used. Explicit cache
  recipes certify traversal rather than cache construction. Vertex-before-edge
  order, pointer identity, strict edge-gap times 1.1 bump, cross-hatch tolerances,
  native default arguments, ignored-status output lifecycle and checked-drop aliasing
  are preserved. A 3D trim-boundary source returns `InvalidInput` in Rust instead of
  upstream's debug-only assertion; the module Deviations line records this.
  Source-endpoint vertex fixtures and generation exercise changed classification
  records. Interior partition insertion still reaches part 2, and more complex
  topology/curve-on-surface dependencies remain explicit. Nonzero edge/face gap
  integration has a focused Rust threshold regression; the native wire edges have
  no owning face edgeuse. General surface lifting, automatic lazy trims, complete
  positive insertion and automatic cache construction remain uncertified. No
  absolute floors or widened tolerances are used.
- **Positive-insertion revalidation handoff.** The previous stacked experiment
  (`3184e613`, not a commit in this PR) assumed input UVs instead of projecting them.
  Three generated planar cases disagreed in gap fields, with source UV segment
  `(0.2,0.5)..(0.8,0.5)`, interval `[0,1]`, one zero-gap loop vertex `(u,0.5,0)`:
  seed `12816342691050780751` (`u=0.6315271935871615`, scale 1e6, zone
  0.09999999999999999, check3d true, modified cache) and seed
  `12816342691050780864` (`u=0.7485310025864422`, same scale/zone/check3d,
  unbuilt cache) returned C++ 1.1641532182693481e-10 versus Rust 0;
  seed `12816342691050781069` (`u=0.7111652965919807`, scale 1e-6, zone
  9.999999999999999e-14, check3d false, under-construction cache) returned
  C++ 0 versus Rust 1.1102230246251565e-16. All used cross-hatch true.
  Native `ComputeUVPoint` ignores its stored UV and projects the world vertex;
  the Rust fixture now calls that existing topology operation too and uses the same
  two-point `Line::from_points` constructor as native. Separate probes confirm
  bit-identical projected UVs and UV-drop parameter/gap pairs for all three inputs
  (gap 0), including the formerly mismatched tiny case.
  The two checked cases exposed native argument aliasing: `IntersectTSBound` passes
  `&dParam` as its guess and `dParam` as output; `SmCrvOnSurf::DropPoint` (line 1071)
  initializes the output to zero before reading the guess, as does the base method.
  Native aliased calls return parameters 0.7192119893119358 / 0.9142183376440705
  and gaps 1.1641532182693481e-10; separate retained guesses return gap 0.
  Part 9 now explicitly supplies `Some(0.0)` to preserve this defined behavior.
  A focused kernel regression matches both native parameter/gap pairs exactly.
  These probes certify the pre-insertion drop measurements, not the deferred final
  insertion state. Re-run the complete seeds when insertion dependencies land.
- **Validation.** Numerical source checkpoint `df4756ac` passes strict workspace
  C++-feature Clippy and all three affected native runners on Linux:
  part 9 424 cases / 225 pass / 197 deferred / 2 rejected / 0 failures;
  `intersect.curve_class_p1` 2,125 / 2,048 / 77 / 0;
  `query.face_p2` 1,344 / 1,271 / 73 / 0. All compared values are bit-identical,
  and both affected reports are unchanged. Part-9 deferrals are part 2 (163),
  generic curve-on-surface dispatch (24), and part 4 (10). All 225 perturbations
  are detected, with zero unperturbed cases. The pole-disjoint fixture, source
  endpoint fixtures and point-interval fixture compare successfully; wide zone,
  large scale and extended interval fixtures reach genuine dependency deferrals.
  Source `0b283f27` subsequently preserves ignored ordinary UV errors in the query
  adapter; formatting, strict plain workspace Clippy, all 1,250 kernel tests and
  wasm checking pass on macOS aarch64 / Rust 1.98.0.
  The 1,183 source/config/fixture files match the VM snapshot SHA-256
  `3af634c1a194b44661358cf525fd4407300dc577ba82af89eb76e5a89024115e`,
  with no extra source or fixture files at that checkpoint. The final native
  fixture predicate uses exact zero coordinates for pole identity, matching Rust
  rather than C++'s tolerance-based point equality; its syntax check with the native
  compiler flags passes. Both focused native regressions
  (pole pruning and a changed positive endpoint record) pass, as do the two native
  FFI guard tests. The PR checks record the final complete native integration gate.
  Native work uses Proxmox VM 100 (restarted with explicit user approval), Ubuntu
  26.04.1 x86_64 / Rust 1.98.0 / GCC 15.2.0, pinned upstream `c9c5979`, default
  optimized `difftest` profile, C++ `-O2 -ffp-contract=off -fno-fast-math` and one
  Cargo job under nice, with incremental caching disabled to limit disk use. The
  temporary swap is removed and the original 1 GiB balloon minimum restored.
  Earlier `7c60fc1d` CI passed all eight checks and the then 187-unit native gate,
  but does not certify these review changes.
